// Bevy system signatures trip both by design.
#![allow(clippy::type_complexity, clippy::too_many_arguments)]
//! hero_view: Eruptor off the user's Giants dump, skinned, playing his idle
//! on a turntable, drawn in the remaster (openspec AE.10, design decision
//! 17) or, with `--stock`, on Bevy's `StandardMaterial` with his colour map
//! alone, the comparison Open question 1's answer leaves chotchki.
//!
//! It reads his archive in memory through the model stage's own reader
//! (`guac_extract::models::read`, AE.6), so it draws what the turntable
//! loads: the portal model's fresh segments in IR v2, skinned on the GPU with
//! Alchemy's inverse joints (AC.17), the idle evaluated by `guac-ir`, each
//! material through `guacamole::character` (the WESL shader AE.8 checked
//! against the character-shading spec's CPU model) with its colour, spec and
//! rim maps decoded through sRGB views and its normal maps as stored. The
//! camera is HDR into TonyMcMapface at scale 1 with 4x MSAA, the same in both
//! modes, so `--stock` changes the material and its lights and nothing else.
//!
//! At an oracle frame (AC.18): `--view-proj` takes the capture's
//! model-view-projection (the vertex program's c0-c3), `--frame` the frame's
//! rig and fog from a file beside the capture, `--at` its idle time and
//! `--size` its output size. Nothing of a frame is in the repo: its numbers
//! are read at run time, and a frame file inside a git work tree is refused.
//! Without `--frame` both modes light with the viewer's own light, one white
//! directional light fixed to the camera (Bevy's ambient beside it in
//! `--stock`), not a light of the game's.
//!
//! It writes nothing but `--screenshot`, refused inside any git work tree or
//! through a symlink, and exits non-zero when the screenshot isn't written.
//!
//! `cargo run -p guacamole --release --example hero_view -- [--stock [--unlit]]
//! [--at <s>] [--yaw <deg>] [--view-proj <16 floats>] [--frame <json>]
//! [--size <w>x<h>] [--screenshot <png> [--frames <n>] [--offscreen]]`
//!
//! A frame file (JSON): `{"source": "...", "lights": [three of {"travel":
//! [x, y, z], "ambient": [r, g, b], "diffuse": [r, g, b], "specular": [r, g,
//! b]}], "fog": {"colour": [r, g, b], "near": n, "far": f} or null}`. Each
//! travel is the way the light goes, a unit vector in the frame's eye space
//! (the game's `ig_light_directional_direction`); colours are the 8-bit
//! codes the game stores, which the remaster decodes; fog distances run
//! along the view direction. `source` is printed and otherwise ignored.

use std::f32::consts::PI;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use anyhow::{Context, Result, anyhow, bail, ensure};
use bevy::app::ScheduleRunnerPlugin;
use bevy::asset::RenderAssetUsages;
use bevy::camera::{
    CameraProjection, Exposure, Hdr, PerspectiveProjection, Projection, RenderTarget, SubCameraView,
};
use bevy::color::{LinearRgba, Srgba};
use bevy::core_pipeline::tonemapping::{DebandDither, Tonemapping};
use bevy::image::{
    CompressedImageFormats, ImageAddressMode, ImageFilterMode, ImageSampler,
    ImageSamplerDescriptor, ImageType,
};
use bevy::log::LogPlugin;
use bevy::math::{DMat3, DMat4, DQuat, DVec3, DVec4, Vec3A};
use bevy::mesh::skinning::SkinnedMeshInverseBindposes;
use bevy::pbr::{DistanceFog, FogFalloff};
// Not the glob: its `ContextExt` clashes with anyhow's `Context`.
use bevy::prelude::{
    App, AppExit, Assets, Camera3d, ClearColor, Color, Commands, DefaultPlugins, DirectionalLight,
    Entity, GlobalAmbientLight, Handle, Image, Local, Mat4, Mesh, MessageWriter, On, PluginGroup,
    Quat, Query, Res, ResMut, Resource, StandardMaterial, Startup, Time, Transform, Update, Vec3,
    Window, WindowPlugin, default, warn,
};
use bevy::render::render_resource::TextureFormat;
use bevy::render::renderer::RenderDevice;
use bevy::render::view::Msaa;
use bevy::render::view::window::screenshot::{Screenshot, ScreenshotCaptured};
use bevy::window::ExitCondition;
use bevy::winit::WinitPlugin;
use guac_extract::models;
use guac_ir::model::{Filter, Sampler, TextureRole, Wrap};
use guac_ir::shading::{self, Scene};
use guac_ir::{DumpIdentity, Model, Sha256};
use guacamole::character::mesh::pose;
use guacamole::character::{
    CharacterMaterial, CharacterShadingPlugin, ShadowLookup, SpawnedCharacter, spawn_character,
};
use serde::Deserialize;

/// Eruptor's character load file: `character/009_eruptor.bld`.
const LOAD_FILE: &str = "009_Eruptor";
const SIZE: (u32, u32) = (1280, 720);
/// Radians per second.
const TURNTABLE: f32 = 0.5;
/// The default camera, and the viewer's light, both in the model's space.
const EYE: Vec3 = Vec3::new(0.0, 1.3, 5.5);
const TARGET: Vec3 = Vec3::new(0.0, 1.0, 0.0);
const VIEWER_LIGHT: Vec3 = Vec3::new(2.0, 4.0, 5.0);
/// The viewer's light in `--stock`, as before AE.10: lux and ambient.
const VIEWER_LUX: f32 = 4000.0;
const VIEWER_AMBIENT: f32 = 300.0;
/// Fog for the viewer's light: on, so a spec map has its eye direction, and
/// beyond anything drawn, as the turntable sets it (design decision 14).
const VIEWER_FOG: (f32, f32) = (1.0e4, 2.0e4);

fn main() -> Result<()> {
    let args = Args::parse(std::env::args().skip(1))?;
    let shot = args
        .screenshot
        .as_deref()
        .map(screenshot_path)
        .transpose()?;
    let camera = match &args.view_proj {
        Some(m) => Some(FrameCamera::from_view_proj(m)?),
        None => None,
    };
    let rig = match &args.frame {
        Some(path) => Rig::read(path)?,
        None => Rig::viewer(),
    };
    let decoded = decode(&dump_root()?)?;
    ensure!(
        run(args, shot, camera, rig, decoded).is_success(),
        "hero_view failed"
    );
    Ok(())
}

#[derive(Debug, Default, PartialEq)]
struct Args {
    /// Freezes the clip at this time and stops the turntable.
    at: Option<f64>,
    /// The turntable's angle while frozen, degrees.
    yaw: f32,
    /// Saves a frame after `frames` frames, then exits.
    screenshot: Option<PathBuf>,
    frames: u32,
    /// No window: renders into an image (needs `screenshot`).
    offscreen: bool,
    /// `StandardMaterial` with the colour map alone instead of the remaster.
    stock: bool,
    /// `--stock`'s colour map without lighting.
    unlit: bool,
    /// The capture's model-view-projection, row by row.
    view_proj: Option<[f32; 16]>,
    /// The frame's rig and fog.
    frame: Option<PathBuf>,
    /// The window's or the offscreen image's size.
    size: (u32, u32),
}

impl Args {
    fn parse(it: impl Iterator<Item = String>) -> Result<Self> {
        let mut it = it.peekable();
        let mut a = Self {
            frames: 240,
            size: SIZE,
            ..Self::default()
        };
        while let Some(arg) = it.next() {
            if arg == "--view-proj" {
                a.view_proj = Some(view_proj(&mut it)?);
                continue;
            }
            let mut value = || it.next().ok_or_else(|| anyhow!("{arg} needs a value"));
            match arg.as_str() {
                "--at" => a.at = Some(value()?.parse().context("--at")?),
                "--yaw" => a.yaw = value()?.parse().context("--yaw")?,
                "--screenshot" => a.screenshot = Some(value()?.into()),
                "--frames" => a.frames = value()?.parse().context("--frames")?,
                "--offscreen" => a.offscreen = true,
                "--stock" => a.stock = true,
                "--unlit" => a.unlit = true,
                "--frame" => a.frame = Some(value()?.into()),
                "--size" => a.size = size(&value()?)?,
                other => bail!("unknown argument {other}"),
            }
        }
        ensure!(
            !a.offscreen || a.screenshot.is_some(),
            "--offscreen needs --screenshot"
        );
        ensure!(a.frames > 0, "--frames must be at least 1");
        ensure!(!a.unlit || a.stock, "--unlit is --stock's");
        Ok(a)
    }
}

/// The 16 numbers after `--view-proj`, as separate arguments or pasted as
/// one (commas, brackets and whitespace between them, so a JSON matrix
/// pastes as is).
fn view_proj(it: &mut impl Iterator<Item = String>) -> Result<[f32; 16]> {
    let mut values = Vec::with_capacity(16);
    while values.len() < 16 {
        let arg = it
            .next()
            .with_context(|| format!("--view-proj needs 16 numbers, got {}", values.len()))?;
        for token in arg
            .split(|c: char| c == ',' || c == '[' || c == ']' || c.is_whitespace())
            .filter(|t| !t.is_empty())
        {
            let v: f32 = token
                .parse()
                .with_context(|| format!("--view-proj: {token} isn't a number"))?;
            ensure!(v.is_finite(), "--view-proj: {token} isn't finite");
            values.push(v);
        }
    }
    values
        .try_into()
        .map_err(|v: Vec<f32>| anyhow!("--view-proj needs 16 numbers, got {}", v.len()))
}

/// `<w>x<h>`, each at least 1.
fn size(s: &str) -> Result<(u32, u32)> {
    let (w, h) = s
        .split_once('x')
        .with_context(|| format!("--size {s}: want <w>x<h>"))?;
    let w: u32 = w.parse().with_context(|| format!("--size {s}"))?;
    let h: u32 = h.parse().with_context(|| format!("--size {s}"))?;
    ensure!(w > 0 && h > 0, "--size {s}: both sides at least 1");
    Ok((w, h))
}

/// `GUAC_GIANTS_DUMP` from the environment, else the workspace's `.env.dev`.
fn dump_root() -> Result<PathBuf> {
    if let Ok(v) = std::env::var("GUAC_GIANTS_DUMP") {
        return Ok(v.into());
    }
    let dotenv = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../.env.dev");
    let text = std::fs::read_to_string(dotenv).unwrap_or_default();
    text.lines()
        .find_map(|line| {
            let (k, v) = line.trim().split_once('=')?;
            (k.trim() == "GUAC_GIANTS_DUMP").then(|| v.trim().trim_matches('"').into())
        })
        .ok_or_else(|| anyhow!("set GUAC_GIANTS_DUMP to the Giants dump"))
}

/// The git work tree `dir` sits in (a `.git` dir or file at or above it),
/// with `dir` already canonical, so `..` and symlinks can't slip past.
fn work_tree(dir: &Path) -> Option<&Path> {
    dir.ancestors().find(|a| a.join(".git").exists())
}

/// The screenshot's path, refused when its directory sits in a git work
/// tree (a `.git` dir or file above it, symlinks resolved) or the file is a
/// symlink. [`write_png`] renames over it, so a hard link's other names keep
/// their bytes.
fn screenshot_path(path: &Path) -> Result<PathBuf> {
    ensure!(
        path.extension()
            .is_some_and(|e| e.eq_ignore_ascii_case("png")),
        "--screenshot wants a .png path"
    );
    let name = path.file_name().context("--screenshot needs a file name")?;
    let parent = match path.parent() {
        Some(p) if !p.as_os_str().is_empty() => p.to_owned(),
        _ => PathBuf::from("."),
    };
    let dir = parent
        .canonicalize()
        .with_context(|| format!("--screenshot's directory {}", parent.display()))?;
    if let Some(tree) = work_tree(&dir) {
        bail!(
            "refusing to write a screenshot inside the git work tree {}",
            tree.display()
        );
    }
    let out = dir.join(name);
    if let Ok(m) = std::fs::symlink_metadata(&out) {
        ensure!(
            !m.file_type().is_symlink(),
            "--screenshot {} is a symlink",
            out.display()
        );
        ensure!(m.is_file(), "--screenshot {} isn't a file", out.display());
    }
    Ok(out)
}

/// `image` as an RGB PNG at `path`: re-checked, written to a fresh file
/// beside it, then renamed over it, so nothing is followed or truncated.
fn write_png(path: &Path, image: Image) -> Result<()> {
    let path = screenshot_path(path)?;
    let dir = path.parent().context("no directory")?;
    let rgb = image.try_into_dynamic()?.to_rgb8();
    let mut tmp = tempfile::Builder::new()
        .prefix(".hero_view")
        .suffix(".png")
        .tempfile_in(dir)?;
    rgb.write_to(tmp.as_file_mut(), image::ImageFormat::Png)?;
    tmp.persist(&path)?;
    Ok(())
}

// ---- the frame's camera ----

/// Bevy's clip from GL's: Bevy's depth runs from 1 at the near plane to 0
/// at the far one (reverse Z), where a GL-style projection's clip z runs
/// from -w to w, so z' = (w - z) / 2.
const BEVY_FROM_GL: DMat4 = DMat4::from_cols_array(&[
    1.0, 0.0, 0.0, 0.0, //
    0.0, 1.0, 0.0, 0.0, //
    0.0, 0.0, -0.5, 0.0, //
    0.0, 0.0, 0.5, 1.0,
]);

/// A capture's model-view-projection as Bevy places it: the model's uniform
/// scale (the game's local-to-world carries Eruptor's 1.5 inside its
/// modelview), the camera's turn and position in the scaled model's world,
/// and the projection that leaves the two composing back to the capture's
/// matrix. View space is then the game's eye space, the space the frame's
/// rig is given in and the character shader lights in.
#[derive(Debug, Clone)]
struct FrameCamera {
    scale: f32,
    transform: Transform,
    projection: CaptureProjection,
    /// Vertical field of view, degrees, and the near and far planes: for
    /// the record, read off the projection.
    fov: f64,
    near: f64,
    far: f64,
}

/// What the GL-style perspective of a frame leaves Bevy: its clip from view
/// with Bevy's depth, fixed whatever the target's size (the frame's NDC
/// fills the target, as the game's fills its 960x540 surface).
#[derive(Debug, Clone, PartialEq)]
struct CaptureProjection {
    clip_from_view: Mat4,
    far: f32,
}

impl CameraProjection for CaptureProjection {
    fn get_clip_from_view(&self) -> Mat4 {
        self.clip_from_view
    }

    /// hero_view has no sub-views.
    fn get_clip_from_view_for_sub(&self, _sub_view: &SubCameraView) -> Mat4 {
        self.clip_from_view
    }

    fn update(&mut self, _width: f32, _height: f32) {}

    fn far(&self) -> f32 {
        self.far
    }

    /// The view-space points at depths `z_near` and `z_far` (negative, in
    /// front of the camera) that land on the corners of NDC, in the order
    /// Bevy's cascades want: bottom right, top right, top left, bottom left.
    fn get_frustum_corners(&self, z_near: f32, z_far: f32) -> [Vec3A; 8] {
        let m = self.clip_from_view.as_dmat4();
        let corner = |sx: f64, sy: f64, z: f32| {
            let z = f64::from(z);
            // Rows 0 and 1 against row 3 at NDC (sx, sy): two equations
            // in x and y.
            let r = |i: usize| m.row(i);
            let (r0, r1, r3) = (r(0), r(1), r(3));
            let e0 = r0 - sx * r3;
            let e1 = r1 - sy * r3;
            let (b0, b1) = (-(e0.z * z + e0.w), -(e1.z * z + e1.w));
            let det = e0.x * e1.y - e0.y * e1.x;
            let x = (b0 * e1.y - e0.y * b1) / det;
            let y = (e0.x * b1 - b0 * e1.x) / det;
            Vec3A::new(x as f32, y as f32, z as f32)
        };
        let at = |z| {
            [
                corner(1.0, -1.0, z),
                corner(1.0, 1.0, z),
                corner(-1.0, 1.0, z),
                corner(-1.0, -1.0, z),
            ]
        };
        let (n, f) = (at(z_near), at(z_far));
        [n[0], n[1], n[2], n[3], f[0], f[1], f[2], f[3]]
    }
}

impl FrameCamera {
    /// Splits `m` (row by row, column vectors, GL clip space) into
    /// P · [s·R | t], P a perspective whose last row is (0, 0, -1, 0), R a
    /// rotation and s a uniform scale. A matrix that isn't one is refused:
    /// one with no perspective divide, a turn that isn't a rotation, a
    /// mirror, near and far out of order.
    fn from_view_proj(m: &[f32; 16]) -> Result<Self> {
        let rows: [DVec4; 4] = std::array::from_fn(|r| {
            DVec4::from_array(std::array::from_fn(|c| f64::from(m[4 * r + c])))
        });
        let [m0, m1, m2, m3] = rows;
        // Row 3 is minus the modelview's row 2: the view direction times s.
        let u = -m3.truncate();
        let t2 = -m3.w;
        let s = u.length();
        ensure!(
            s > 1e-9 && s.is_finite(),
            "--view-proj isn't a perspective: its last row has no depth"
        );
        let s2 = s * s;
        // A row of P · MV: its own axis plus an off-centre share of row 2.
        let split = |p: DVec4| {
            let v = p.truncate();
            let off = v.dot(u) / s2;
            (off, v - off * u, p.w)
        };
        let (c, w0, m03) = split(m0);
        let (d, w1, m13) = split(m1);
        let (a, b) = (w0.length() / s, w1.length() / s);
        ensure!(
            a > 1e-9 && b > 1e-9,
            "--view-proj flattens the picture to a line"
        );
        let (r0, r1, r2) = (w0 / (a * s), w1 / (b * s), u / s);
        let square = r0.dot(r1).abs();
        ensure!(
            square < 1e-4,
            "--view-proj's turn isn't a rotation: its x and y axes are {square:.1e} off square"
        );
        let handed = r0.cross(r1).dot(r2);
        ensure!(
            handed > 0.999,
            "--view-proj mirrors the picture (axes {handed:.4})"
        );
        let e = m2.truncate().dot(u) / s2;
        let stray = (m2.truncate() - e * u).length() / m2.truncate().length().max(1.0);
        ensure!(
            stray < 1e-4,
            "--view-proj's depth row leans sideways ({stray:.1e}): not a perspective"
        );
        let f = m2.w - e * t2;
        let t = DVec3::new((m03 - c * t2) / a, (m13 - d * t2) / b, t2);
        let (near, far) = (f / (e - 1.0), f / (e + 1.0));
        ensure!(
            near > 0.0 && far > near && far.is_finite(),
            "--view-proj's near and far ({near}, {far}) aren't in front of the camera in order"
        );

        // The camera in the scaled model's world: R's transpose and -Rᵀt.
        let turn = DMat3::from_cols(r0, r1, r2);
        let rotation = DQuat::from_mat3(&turn).normalize().as_quat();
        let translation = (-(turn * t)).as_vec3();
        let transform = Transform {
            translation,
            rotation,
            scale: Vec3::ONE,
        };
        let scale = s as f32;
        // Whatever the rounding left in the turn, the chain composes back
        // to the capture's matrix: clip from view = C · M · S⁻¹ · camera.
        let mat = DMat4::from_cols_array(&m.map(f64::from)).transpose();
        let camera = DMat4::from_rotation_translation(
            transform.rotation.as_dquat(),
            transform.translation.as_dvec3(),
        );
        let unscale = DMat4::from_scale(DVec3::splat(1.0 / f64::from(scale)));
        let clip_from_view = (BEVY_FROM_GL * mat * unscale * camera).as_mat4();
        Ok(Self {
            scale,
            transform,
            projection: CaptureProjection {
                clip_from_view,
                far: far as f32,
            },
            fov: 2.0 * (1.0 / b).atan().to_degrees(),
            near,
            far,
        })
    }
}

/// The camera and the model's scale: a frame's, else the viewer's own
/// (Bevy's default perspective, looking at Eruptor from the front).
fn camera_setup(frame: Option<&FrameCamera>) -> (Transform, Projection, f32) {
    match frame {
        Some(f) => (
            f.transform,
            Projection::custom(f.projection.clone()),
            f.scale,
        ),
        None => (
            Transform::from_translation(EYE).looking_at(TARGET, Vec3::Y),
            Projection::Perspective(PerspectiveProjection::default()),
            1.0,
        ),
    }
}

// ---- the frame's rig and fog ----

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
struct FrameFile {
    #[serde(default)]
    source: Option<String>,
    lights: [FrameLight; 3],
    fog: Option<FrameFog>,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
struct FrameLight {
    travel: [f32; 3],
    ambient: [u8; 3],
    diffuse: [u8; 3],
    specular: [u8; 3],
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
struct FrameFog {
    colour: [u8; 3],
    near: f32,
    far: f32,
}

/// The fog a rig carries: its colour in linear light and its distances
/// along the view direction.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Fog {
    colour: [f32; 3],
    near: f32,
    far: f32,
}

/// Three directional lights fixed to the camera (each travel in view
/// space), their colours in linear light, and the fog.
#[derive(Debug, Clone, PartialEq, Resource)]
struct Rig {
    lights: [shading::DirectionalLight; 3],
    fog: Option<Fog>,
}

/// A colour the disc or the game stores, decoded as the remaster decodes
/// it (design decision 12).
fn linear(c: [u8; 3]) -> [f32; 3] {
    let l = LinearRgba::from(Srgba::rgb_u8(c[0], c[1], c[2]));
    [l.red, l.green, l.blue]
}

impl Rig {
    /// The viewer's own light: white, from up and to the right of the
    /// default camera, fixed to the camera; fog on beyond anything drawn.
    fn viewer() -> Self {
        let camera = Transform::from_translation(EYE).looking_at(TARGET, Vec3::Y);
        let travel = camera.rotation.inverse() * (TARGET - VIEWER_LIGHT).normalize();
        let mut lights = [shading::DirectionalLight::OFF; 3];
        lights[0] = shading::DirectionalLight {
            travel: travel.to_array(),
            ambient: [0.0; 3],
            diffuse: [1.0; 3],
            specular: [1.0; 3],
        };
        Self {
            lights,
            fog: Some(Fog {
                colour: [0.0; 3],
                near: VIEWER_FOG.0,
                far: VIEWER_FOG.1,
            }),
        }
    }

    /// A frame file's rig: refused inside a git work tree, so a frame's
    /// numbers stay beside its capture.
    fn read(path: &Path) -> Result<Self> {
        let canonical = path
            .canonicalize()
            .with_context(|| format!("--frame {}", path.display()))?;
        if let Some(tree) = canonical.parent().and_then(work_tree) {
            bail!(
                "refusing a frame file inside the git work tree {}: it belongs beside its capture",
                tree.display()
            );
        }
        let text = std::fs::read_to_string(&canonical)
            .with_context(|| format!("--frame {}", canonical.display()))?;
        let file: FrameFile = serde_json::from_str(&text)
            .with_context(|| format!("--frame {}", canonical.display()))?;
        if let Some(source) = &file.source {
            println!("frame {}: {source}", canonical.display());
        }
        Self::from_file(&file)
    }

    fn from_file(file: &FrameFile) -> Result<Self> {
        let mut lights = [shading::DirectionalLight::OFF; 3];
        for (i, (l, out)) in file.lights.iter().zip(&mut lights).enumerate() {
            let length = Vec3::from_array(l.travel).length();
            ensure!(
                (length - 1.0).abs() < 1e-3,
                "frame light {i}: its travel is {length} long, not a unit vector"
            );
            *out = shading::DirectionalLight {
                travel: l.travel,
                ambient: linear(l.ambient),
                diffuse: linear(l.diffuse),
                specular: linear(l.specular),
            };
        }
        let fog = match &file.fog {
            Some(f) => {
                ensure!(
                    f.near.is_finite() && f.far.is_finite() && f.near < f.far,
                    "frame fog: near {} isn't before far {}",
                    f.near,
                    f.far
                );
                Some(Fog {
                    colour: linear(f.colour),
                    near: f.near,
                    far: f.far,
                })
            }
            None => None,
        };
        Ok(Self { lights, fog })
    }

    /// The scene the character material draws in: this rig, fog as it
    /// says, shadows and point lights off, neutral placement, no flash.
    fn scene(&self) -> Scene {
        let mut scene = Scene::new();
        scene.lights = self.lights;
        if let Some(f) = self.fog {
            scene.fog = true;
            scene.fog_colour = f.colour;
            scene.fog_near = f.near;
            scene.fog_far = f.far;
        }
        scene
    }
}

/// A Bevy directional light shining along `travel` (view space) from a
/// camera at `camera`.
fn shining(camera: &Transform, travel: [f32; 3]) -> Transform {
    let world = camera.rotation * Vec3::from_array(travel);
    Transform::from_rotation(Quat::from_rotation_arc(Vec3::NEG_Z, world.normalize()))
}

// ---- Eruptor ----

#[derive(Resource)]
struct Decoded {
    model: Model,
    /// Each distinct texture: its extract path and its KTX2.
    textures: Vec<(String, Vec<u8>)>,
}

fn decode(root: &Path) -> Result<Decoded> {
    let started = Instant::now();
    let path = models::archive_path(&root.join("PS3_GAME/USRDIR"), LOAD_FILE);
    let archive = models::archive(&path, LOAD_FILE)?;
    let level = models::level(&archive, LOAD_FILE)?;
    let read = started.elapsed();
    // Never written anywhere, so no dump identity to carry.
    let identity = DumpIdentity {
        title_id: String::new(),
        app_ver: String::new(),
        manifest_hash: Sha256([0; 32]),
    };
    let extracted = models::read(&level, LOAD_FILE, identity)?;
    let model = extracted.model;
    let ms = |d: Duration| d.as_secs_f64() * 1e3;
    let vertices: usize = model.segments.iter().map(|s| s.positions.len()).sum();
    let triangles: usize = model.segments.iter().map(|s| s.indices.len() / 3).sum();
    println!(
        "decoded {} in {:.1} ms (archive {:.1}): {} segments, {} vertices, {} triangles, {} joints; idle {} {:.4} s; {} textures",
        model.name,
        ms(started.elapsed()),
        ms(read),
        model.segments.len(),
        vertices,
        triangles,
        model.skeleton.joints.len(),
        model.idle.name,
        model.idle.duration,
        extracted.textures.len(),
    );
    Ok(Decoded {
        model,
        textures: extracted.textures,
    })
}

/// An IR sampler as Bevy's; one without a mipmap filter samples level 0
/// alone, as the original does.
fn sampler(s: &Sampler) -> ImageSamplerDescriptor {
    let wrap = |w: Wrap| match w {
        Wrap::Clamp => ImageAddressMode::ClampToEdge,
        Wrap::Repeat => ImageAddressMode::Repeat,
    };
    let filter = |f: Filter| match f {
        Filter::Nearest => ImageFilterMode::Nearest,
        Filter::Linear => ImageFilterMode::Linear,
    };
    ImageSamplerDescriptor {
        address_mode_u: wrap(s.wrap_u),
        address_mode_v: wrap(s.wrap_v),
        mag_filter: filter(s.mag),
        min_filter: filter(s.min),
        mipmap_filter: s.mip.map_or(ImageFilterMode::Nearest, filter),
        lod_max_clamp: if s.mip.is_some() { 32.0 } else { 0.0 },
        ..ImageSamplerDescriptor::default()
    }
}

/// Each texture a role reads, loaded once per view and sampler: colour,
/// spec and rim maps through sRGB views (the remaster decodes them), normal
/// maps as stored.
struct Textures<'a> {
    decoded: &'a Decoded,
    formats: CompressedImageFormats,
    loaded: Vec<((String, bool, Sampler), Handle<Image>)>,
}

impl Textures<'_> {
    fn get(
        &mut self,
        images: &mut Assets<Image>,
        path: &str,
        role: TextureRole,
        s: &Sampler,
    ) -> Handle<Image> {
        let key = (path.to_owned(), role.is_color(), *s);
        if let Some((_, h)) = self.loaded.iter().find(|(k, _)| *k == key) {
            return h.clone();
        }
        let (_, ktx2) = self
            .decoded
            .textures
            .iter()
            .find(|(p, _)| p == path)
            .unwrap_or_else(|| panic!("{path} isn't among the model's textures"));
        let image = Image::from_buffer(
            ktx2,
            ImageType::Extension("ktx2"),
            self.formats,
            key.1,
            ImageSampler::Descriptor(sampler(s)),
            RenderAssetUsages::RENDER_WORLD,
        )
        .unwrap_or_else(|e| panic!("{path}: {e}"));
        let handle = images.add(image);
        self.loaded.push((key, handle.clone()));
        handle
    }
}

// ---- the app ----

#[derive(Resource)]
struct View {
    at: Option<f64>,
    yaw: f32,
    frames: u32,
    shot: Option<PathBuf>,
    offscreen: bool,
    stock: bool,
    unlit: bool,
    size: (u32, u32),
    frame: Option<FrameCamera>,
    /// The image the camera renders into when offscreen.
    target: Option<Handle<Image>>,
}

#[derive(Resource)]
struct Spawned(SpawnedCharacter);

fn run(
    args: Args,
    shot: Option<PathBuf>,
    frame: Option<FrameCamera>,
    rig: Rig,
    decoded: Decoded,
) -> AppExit {
    if let Some(f) = &frame {
        let at = f.transform.translation;
        let forward = f.transform.forward();
        println!(
            "frame camera: model scale {:.6}, at ({:.4}, {:.4}, {:.4}) facing ({:.5}, {:.5}, {:.5}), vertical FOV {:.4} deg, near {:.5}, far {:.3}",
            f.scale, at.x, at.y, at.z, forward.x, forward.y, forward.z, f.fov, f.near, f.far
        );
    }
    let mut app = App::new();
    let plugins = DefaultPlugins.set(LogPlugin {
        filter: "wgpu=error,naga=warn".into(),
        ..default()
    });
    let title = if args.stock { "stock" } else { "remaster" };
    if args.offscreen {
        app.add_plugins((
            plugins
                .set(WindowPlugin {
                    primary_window: None,
                    exit_condition: ExitCondition::DontExit,
                    ..default()
                })
                .disable::<WinitPlugin>(),
            ScheduleRunnerPlugin::run_loop(Duration::from_secs_f64(1.0 / 60.0)),
        ));
    } else {
        app.add_plugins(plugins.set(WindowPlugin {
            primary_window: Some(Window {
                title: format!("hero_view: Eruptor, {title}"),
                resolution: args.size.into(),
                ..default()
            }),
            ..default()
        }));
    }
    app.add_plugins(CharacterShadingPlugin)
        .insert_resource(decoded)
        .insert_resource(rig)
        .insert_resource(View {
            at: args.at,
            yaw: args.yaw.to_radians(),
            frames: args.frames,
            shot,
            offscreen: args.offscreen,
            stock: args.stock,
            unlit: args.unlit,
            size: args.size,
            frame,
            target: None,
        })
        .insert_resource(ClearColor(Color::srgb(0.05, 0.07, 0.16)))
        .add_systems(Startup, setup)
        .add_systems(Update, (animate, screenshot))
        .run()
}

fn setup(
    mut commands: Commands,
    mut view: ResMut<View>,
    decoded: Res<Decoded>,
    rig: Res<Rig>,
    device: Option<Res<RenderDevice>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut characters: ResMut<Assets<CharacterMaterial>>,
    mut stock: ResMut<Assets<StandardMaterial>>,
    mut images: ResMut<Assets<Image>>,
    mut bindposes: ResMut<Assets<SkinnedMeshInverseBindposes>>,
) {
    let formats = match device {
        Some(d) => CompressedImageFormats::from_features(d.features()),
        None => {
            warn!("no RenderDevice yet; assuming BC support");
            CompressedImageFormats::BC
        }
    };
    let mut textures = Textures {
        decoded: &decoded,
        formats,
        loaded: Vec::new(),
    };
    let (eye, projection, scale) = camera_setup(view.frame.as_ref());
    let root = Transform {
        rotation: Quat::from_rotation_y(view.yaw),
        scale: Vec3::splat(scale),
        ..Transform::IDENTITY
    };
    let model = &decoded.model;
    let at = view.at.unwrap_or(0.0);
    let spawned = if view.stock {
        // The colour map alone, as hero_view drew before AE.10; vertex
        // colours (a multiplier the remaster reads) would darken it.
        let handles: Vec<Handle<StandardMaterial>> = model
            .materials
            .iter()
            .map(|m| {
                let colour = m
                    .textures
                    .iter()
                    .find(|t| t.role == TextureRole::Layer1ColorMap)
                    .map(|t| textures.get(&mut images, &t.texture, t.role, &t.sampler));
                stock.add(StandardMaterial {
                    base_color_texture: colour,
                    perceptual_roughness: 0.8,
                    unlit: view.unlit,
                    ..default()
                })
            })
            .collect();
        let mut plain = model.clone();
        for s in &mut plain.segments {
            s.colors = None;
        }
        spawn_character(
            &mut commands,
            &plain,
            root,
            at,
            &mut meshes,
            &mut bindposes,
            &handles,
        )
    } else {
        let scene = rig.scene();
        let handles: Vec<Handle<CharacterMaterial>> = model
            .materials
            .iter()
            .map(|m| {
                let material = CharacterMaterial::from_ir(m, &scene, ShadowLookup::NONE, |slot| {
                    textures.get(&mut images, &slot.texture, slot.role, &slot.sampler)
                })
                .unwrap_or_else(|e| panic!("{}: {e}", m.name));
                println!(
                    "{}: pixel {:#05x} vertex {:#06x} drawn, {:?}",
                    m.name,
                    material.key.shader.pixel(),
                    material.key.shader.vertex(),
                    material.key.states,
                );
                characters.add(material)
            })
            .collect();
        spawn_character(
            &mut commands,
            model,
            root,
            at,
            &mut meshes,
            &mut bindposes,
            &handles,
        )
    }
    .unwrap_or_else(|e| panic!("{}: {e}", model.name));
    commands.insert_resource(Spawned(spawned));

    let camera = commands
        .spawn((
            Camera3d::default(),
            eye,
            projection,
            Hdr,
            Tonemapping::TonyMcMapface,
            // Off so a shot comes out the same twice and its background is
            // one colour.
            DebandDither::Disabled,
            Msaa::Sample4,
        ))
        .id();
    if view.offscreen {
        let mut target = Image::new_target_texture(
            view.size.0,
            view.size.1,
            TextureFormat::Rgba8UnormSrgb,
            None,
        );
        target.asset_usage = RenderAssetUsages::default();
        let handle = images.add(target);
        commands
            .entity(camera)
            .insert(RenderTarget::Image(handle.clone().into()));
        view.target = Some(handle);
    }
    if view.stock {
        stock_lights(&mut commands, camera, &eye, &rig, view.frame.is_some());
    }
}

/// `--stock`'s lights, fixed to the camera as the rig is. Without a frame,
/// the viewer's light at the lux and ambient hero_view had before AE.10.
/// With one, each light's diffuse colour as a Bevy directional light and
/// the ambients' sum as Bevy's ambient, at the scale where a light of
/// linear colour c adds c · albedo · N·L (ae-color's stock render), and the
/// fog as Bevy's linear distance fog; Bevy has one colour a light, so the
/// rig's specular colours go unused.
fn stock_lights(commands: &mut Commands, camera: Entity, eye: &Transform, rig: &Rig, frame: bool) {
    if !frame {
        commands.spawn((
            DirectionalLight {
                illuminance: VIEWER_LUX,
                ..default()
            },
            shining(eye, rig.lights[0].travel),
        ));
        commands.insert_resource(GlobalAmbientLight {
            brightness: VIEWER_AMBIENT,
            ..default()
        });
        return;
    }
    let exposure = Exposure::default().exposure();
    let colour = |c: [f32; 3]| Color::LinearRgba(LinearRgba::rgb(c[0], c[1], c[2]));
    for l in &rig.lights {
        if l.diffuse == [0.0; 3] {
            continue;
        }
        commands.spawn((
            DirectionalLight {
                color: colour(l.diffuse),
                illuminance: PI / exposure,
                ..default()
            },
            shining(eye, l.travel),
        ));
    }
    let ambient = rig.lights.iter().fold([0.0; 3], |s, l| {
        std::array::from_fn(|i| s[i] + l.ambient[i])
    });
    commands.insert_resource(GlobalAmbientLight {
        color: colour(ambient),
        brightness: 1.0 / exposure,
        ..default()
    });
    if let Some(f) = rig.fog {
        commands.entity(camera).insert(DistanceFog {
            color: colour(f.colour),
            falloff: FogFalloff::Linear {
                start: f.near,
                end: f.far,
            },
            ..default()
        });
    }
}

fn animate(
    time: Res<Time>,
    view: Res<View>,
    decoded: Res<Decoded>,
    spawned: Option<Res<Spawned>>,
    mut transforms: Query<&mut Transform>,
) {
    let Some(spawned) = spawned else { return };
    let t = view.at.unwrap_or_else(|| time.elapsed_secs_f64());
    for (e, p) in spawned.0.joints.iter().zip(pose(&decoded.model, t)) {
        if let Ok(mut tr) = transforms.get_mut(*e) {
            *tr = p;
        }
    }
    if view.at.is_none()
        && let Ok(mut root) = transforms.get_mut(spawned.0.root)
    {
        root.rotation = Quat::from_rotation_y(view.yaw + time.elapsed_secs() * TURNTABLE);
    }
}

fn screenshot(
    mut commands: Commands,
    view: Res<View>,
    mut frame: Local<u32>,
    mut started: Local<Option<Instant>>,
) {
    let Some(path) = &view.shot else { return };
    let started = *started.get_or_insert_with(Instant::now);
    *frame += 1;
    if *frame != view.frames {
        return;
    }
    let target = match &view.target {
        Some(image) => Screenshot::image(image.clone()),
        None => Screenshot::primary_window(),
    };
    let path = path.clone();
    let frames = view.frames;
    commands.spawn(target).observe(
        move |captured: On<ScreenshotCaptured>, mut exit: MessageWriter<AppExit>| {
            let s = started.elapsed().as_secs_f64();
            match write_png(&path, captured.image.clone()) {
                Ok(()) => {
                    println!("screenshot {}: {frames} frames in {s:.2} s", path.display());
                    exit.write(AppExit::Success);
                }
                Err(e) => {
                    eprintln!("screenshot {}: {e:#}", path.display());
                    exit.write(AppExit::error());
                }
            }
        },
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(s: &str) -> Result<Args> {
        Args::parse(s.split_whitespace().map(String::from))
    }

    #[test]
    fn args_parse() {
        let a = parse("--at 0.5 --yaw 90 --screenshot /tmp/x.png --offscreen").unwrap();
        assert_eq!(a.at, Some(0.5));
        assert_eq!(
            (a.yaw, a.frames, a.offscreen, a.stock, a.unlit, a.size),
            (90.0, 240, true, false, false, (1280, 720))
        );
        let s = parse("--stock --unlit --size 960x540 --frame /x/frame.json").unwrap();
        assert!(s.stock && s.unlit);
        assert_eq!(s.size, (960, 540));
        assert_eq!(s.frame, Some(PathBuf::from("/x/frame.json")));
        assert_eq!(s.view_proj, None);

        assert!(parse("--offscreen").is_err());
        assert!(parse("--frames 0").is_err());
        assert!(parse("--at").is_err());
        assert!(parse("--bogus").is_err());
        let e = parse("--unlit").unwrap_err();
        assert!(e.to_string().contains("--stock"), "{e}");
        for bad in ["960", "960x", "x540", "0x540", "960x0", "-960x540", "axb"] {
            assert!(parse(&format!("--size {bad}")).is_err(), "{bad}");
        }
        // One bind, Alchemy's inverse joints (AC.17), so no switch (AC.22).
        let e = parse("--bind havok").unwrap_err();
        assert!(e.to_string().contains("unknown argument --bind"), "{e}");
    }

    #[test]
    fn the_view_projection_takes_16_numbers_in_any_paste() {
        let want: [f32; 16] = std::array::from_fn(|i| i as f32 - 7.5);
        let words: Vec<String> = want.iter().map(|v| v.to_string()).collect();
        let spaced = parse(&format!("--view-proj {} --stock", words.join(" "))).unwrap();
        assert_eq!(spaced.view_proj, Some(want));
        assert!(spaced.stock, "the arguments after the 16 still parse");
        // Pasted as one argument, as a JSON matrix prints.
        let rows: Vec<String> = want
            .chunks(4)
            .map(|r| {
                format!(
                    "[{}]",
                    r.iter().map(f32::to_string).collect::<Vec<_>>().join(", ")
                )
            })
            .collect();
        let json = format!("[{}]", rows.join(", "));
        let pasted = Args::parse(["--view-proj".to_owned(), json].into_iter()).unwrap();
        assert_eq!(pasted.view_proj, Some(want));
        // 1e-3 as Rust and JSON print it.
        let tiny = parse(&format!("--view-proj 1e-3 {}", words[1..].join(" "))).unwrap();
        assert_eq!(tiny.view_proj.unwrap()[0], 1e-3);

        let short = parse(&format!("--view-proj {}", words[..15].join(" "))).unwrap_err();
        assert!(short.to_string().contains("got 15"), "{short}");
        let long = Args::parse(["--view-proj".to_owned(), words.join(",") + ",1"].into_iter());
        assert!(long.unwrap_err().to_string().contains("got 17"));
        let word = parse(&format!("--view-proj {} --stock", words[..8].join(" "))).unwrap_err();
        assert!(
            word.to_string().contains("--stock isn't a number"),
            "{word}"
        );
        assert!(parse(&format!("--view-proj inf {}", words[1..].join(" "))).is_err());
    }

    /// A GL-style perspective: vertical FOV, aspect, near and far, and an
    /// off-centre shift on each axis.
    fn perspective(fov: f64, aspect: f64, near: f64, far: f64, shift: (f64, f64)) -> DMat4 {
        let b = 1.0 / (fov.to_radians() / 2.0).tan();
        let a = b / aspect;
        DMat4::from_cols_array(&[
            a,
            0.0,
            0.0,
            0.0,
            0.0,
            b,
            0.0,
            0.0,
            shift.0,
            shift.1,
            -(far + near) / (far - near),
            -1.0,
            0.0,
            0.0,
            -2.0 * far * near / (far - near),
            0.0,
        ])
    }

    /// A capture's camera made up for the test (none of the game's): a
    /// modelview of uniform scale 1.5 turned 15 deg, pitched -23 and rolled
    /// 3, then a 50 deg 16:9 perspective a little off centre.
    fn synthetic() -> (DMat4, DMat4, [f32; 16]) {
        let turn = DQuat::from_rotation_z(3f64.to_radians())
            * DQuat::from_rotation_x(23f64.to_radians())
            * DQuat::from_rotation_y(-15f64.to_radians());
        let modelview = DMat4::from_scale_rotation_translation(
            DVec3::splat(1.5),
            turn,
            DVec3::new(0.25, -2.5, -16.5),
        );
        let p = perspective(50.0, 16.0 / 9.0, 0.1, 250.0, (0.01, -0.02));
        let m = p * modelview;
        let rows = m.transpose().to_cols_array().map(|v| v as f32);
        (p, modelview, rows)
    }

    fn max_off(a: DMat4, b: DMat4) -> f64 {
        let (a, b) = (a.to_cols_array(), b.to_cols_array());
        let scale = b.iter().fold(1.0f64, |s, v| s.max(v.abs()));
        a.iter()
            .zip(b)
            .map(|(x, y)| (x - y).abs() / scale)
            .fold(0.0, f64::max)
    }

    #[test]
    fn the_view_projection_is_applied_to_the_camera() {
        let (p, modelview, rows) = synthetic();
        let f = FrameCamera::from_view_proj(&rows).unwrap();
        assert!((f.scale - 1.5).abs() < 1e-5, "{}", f.scale);
        assert!((f.fov - 50.0).abs() < 1e-3, "{}", f.fov);
        assert!((f.near - 0.1).abs() < 1e-4 && (f.far - 250.0).abs() < 0.5);

        // Bevy's chain, as the camera and the model's root apply it:
        // clip from view (the projection) · view from world (the camera's
        // inverse) · world from model (the root's scale), against the
        // capture's matrix with Bevy's depth.
        let root = Transform::from_scale(Vec3::splat(f.scale));
        let projection = Projection::custom(f.projection.clone());
        let chain = (projection.get_clip_from_view()
            * f.transform.to_matrix().inverse()
            * root.to_matrix())
        .as_dmat4();
        let m = DMat4::from_cols_array(&rows.map(f64::from)).transpose();
        let off = max_off(chain, BEVY_FROM_GL * m);
        assert!(off < 1e-5, "the chain is {off:.1e} off the capture's");

        // View space is the capture's eye space: the camera's inverse
        // times the root is the modelview, so the rig lights where it did.
        let eye = (f.transform.to_matrix().inverse() * root.to_matrix()).as_dmat4();
        let off = max_off(eye, modelview);
        assert!(off < 1e-5, "eye space is {off:.1e} off the modelview");
        let off = max_off(f.projection.clip_from_view.as_dmat4(), BEVY_FROM_GL * p);
        assert!(off < 1e-5, "the projection is {off:.1e} off");

        // A point in front lands where the capture puts it, its depth
        // between 0 (far) and 1 (near).
        let point = DVec4::new(0.3, 1.2, -0.4, 1.0);
        let gl = m * point;
        let bevy = chain * point;
        let (gl, bevy) = (gl.truncate() / gl.w, bevy.truncate() / bevy.w);
        assert!((gl.x - bevy.x).abs() < 1e-5 && (gl.y - bevy.y).abs() < 1e-5);
        assert!((bevy.z - (1.0 - gl.z) / 2.0).abs() < 1e-5 && (0.0..1.0).contains(&bevy.z));
    }

    #[test]
    fn a_matrix_that_isnt_a_cameras_is_refused() {
        let (p, modelview, rows) = synthetic();
        let flat = |m: DMat4| m.transpose().to_cols_array().map(|v| v as f32);
        let refused = |m: [f32; 16], what: &str| {
            let e = FrameCamera::from_view_proj(&m).unwrap_err().to_string();
            assert!(e.contains(what), "{what}: {e}");
        };
        // An orthographic projection (affine, its last row 0, 0, 0, 1) has
        // no perspective divide.
        let ortho = DMat4::from_scale(DVec3::new(0.5, 0.9, -0.02)) * modelview;
        refused(flat(ortho), "isn't a perspective");
        // A shear in the modelview.
        let shear = DMat4::from_cols_array(&[
            1.0, 0.0, 0.0, 0.0, //
            0.3, 1.0, 0.0, 0.0, //
            0.0, 0.0, 1.0, 0.0, //
            0.0, 0.0, 0.0, 1.0,
        ]);
        refused(flat(p * shear * modelview), "isn't a rotation");
        // A mirror.
        let mirror = DMat4::from_scale(DVec3::new(-1.0, 1.0, 1.0));
        refused(flat(p * mirror * modelview), "mirrors");
        // A depth row that mixes in x.
        let mut lean = rows;
        lean[8] += 0.5;
        refused(lean, "leans sideways");
        // Far in front of near: a perspective built with them swapped.
        let swapped = perspective(50.0, 16.0 / 9.0, 250.0, 0.1, (0.0, 0.0)) * modelview;
        refused(flat(swapped), "in order");
        assert!(FrameCamera::from_view_proj(&[0.0; 16]).is_err());
    }

    #[test]
    fn the_frustums_corners_land_on_the_edges_of_the_picture() {
        let (_, _, rows) = synthetic();
        let f = FrameCamera::from_view_proj(&rows).unwrap();
        let corners = f.projection.get_frustum_corners(-1.0, -100.0);
        let want = [(1.0, -1.0), (1.0, 1.0), (-1.0, 1.0), (-1.0, -1.0)];
        for (i, c) in corners.iter().enumerate() {
            let clip = f.projection.clip_from_view * c.extend(1.0);
            let (x, y) = (clip.x / clip.w, clip.y / clip.w);
            let (wx, wy) = want[i % 4];
            assert!(
                (x - wx).abs() < 1e-4 && (y - wy).abs() < 1e-4,
                "{i}: {x}, {y}"
            );
            assert_eq!(c.z, if i < 4 { -1.0 } else { -100.0 });
        }
    }

    /// A frame file with made-up numbers (none of the game's).
    fn frame_json(travel0: &str, fog: &str) -> String {
        format!(
            r#"{{"source": "made up for the test",
                "lights": [
                  {{"travel": {travel0}, "ambient": [10, 20, 30], "diffuse": [200, 100, 50], "specular": [255, 255, 255]}},
                  {{"travel": [0, 1, 0], "ambient": [0, 0, 0], "diffuse": [40, 40, 90], "specular": [80, 80, 80]}},
                  {{"travel": [0, 0, -1], "ambient": [0, 0, 0], "diffuse": [0, 0, 0], "specular": [0, 0, 0]}}
                ],
                "fog": {fog}}}"#
        )
    }

    #[test]
    fn a_frame_file_becomes_the_rig_in_linear_light() {
        let fog = r#"{"colour": [128, 64, 32], "near": 30, "far": 300}"#;
        let file: FrameFile = serde_json::from_str(&frame_json("[0.6, -0.8, 0]", fog)).unwrap();
        let rig = Rig::from_file(&file).unwrap();
        let key = rig.lights[0];
        assert_eq!(key.travel, [0.6, -0.8, 0.0]);
        // Colours decode through bevy_color as the remaster does: 200 is
        // about 0.578 and 255 exactly 1.
        let want = LinearRgba::from(Srgba::rgb_u8(200, 100, 50));
        assert_eq!(key.diffuse, [want.red, want.green, want.blue]);
        assert!((key.diffuse[0] - 0.578).abs() < 1e-3, "{:?}", key.diffuse);
        assert_eq!(key.specular, [1.0; 3]);
        assert_eq!(rig.lights[2].diffuse, [0.0; 3]);
        let scene = rig.scene();
        assert!(scene.fog && !scene.shadows && scene.point_lights.is_empty());
        assert_eq!((scene.fog_near, scene.fog_far), (30.0, 300.0));
        assert_eq!(scene.fog_colour, linear([128, 64, 32]));
        assert_eq!(scene.lights, rig.lights);

        // No fog: the scene's fog is off, and with it the eye direction.
        let file: FrameFile = serde_json::from_str(&frame_json("[0, 0, 1]", "null")).unwrap();
        assert!(!Rig::from_file(&file).unwrap().scene().fog);

        let bad = |travel: &str, fog: &str| {
            let file: FrameFile = serde_json::from_str(&frame_json(travel, fog)).unwrap();
            Rig::from_file(&file).unwrap_err().to_string()
        };
        assert!(bad("[0.5, 0, 0]", "null").contains("not a unit vector"));
        assert!(bad("[0, 0, 1]", r#"{"colour": [0, 0, 0], "near": 9, "far": 9}"#).contains("far"));
        // Two lights, a code past 255 or a field it doesn't know: refused
        // as it reads.
        let two = r#"{"lights": [], "fog": null}"#;
        assert!(serde_json::from_str::<FrameFile>(two).is_err());
        let past = frame_json("[0, 0, 1]", "null").replace("200, 100", "256, 100");
        assert!(serde_json::from_str::<FrameFile>(&past).is_err());
        let extra = frame_json("[0, 0, 1]", "null").replace("\"source\"", "\"sauce\"");
        assert!(serde_json::from_str::<FrameFile>(&extra).is_err());
    }

    #[test]
    fn a_frame_file_inside_a_work_tree_is_refused() {
        let dir = tempfile::tempdir().unwrap();
        let json = frame_json("[0, 0, 1]", "null");
        let outside = dir.path().join("frame.json");
        std::fs::write(&outside, &json).unwrap();
        assert!(Rig::read(&outside).is_ok());
        let tree = dir.path().join("repo");
        std::fs::create_dir_all(tree.join(".git")).unwrap();
        std::fs::create_dir_all(tree.join("a")).unwrap();
        let inside = tree.join("a/frame.json");
        std::fs::write(&inside, &json).unwrap();
        let e = Rig::read(&inside).unwrap_err().to_string();
        assert!(e.contains("git work tree"), "{e}");
        // Nor through a link from outside.
        #[cfg(unix)]
        {
            let link = dir.path().join("link.json");
            std::os::unix::fs::symlink(&inside, &link).unwrap();
            assert!(Rig::read(&link).is_err());
        }
        assert!(Rig::read(&dir.path().join("missing.json")).is_err());
    }

    #[test]
    fn the_viewers_light_follows_the_camera_from_where_it_was() {
        let rig = Rig::viewer();
        let camera = Transform::from_translation(EYE).looking_at(TARGET, Vec3::Y);
        // At the default camera it shines from (2, 4, 5) at (0, 1, 0), as
        // hero_view's light did before AE.10.
        let light = shining(&camera, rig.lights[0].travel);
        let want = (TARGET - VIEWER_LIGHT).normalize();
        assert!(light.forward().as_vec3().abs_diff_eq(want, 1e-6));
        assert_eq!(rig.lights[1], shading::DirectionalLight::OFF);
        let scene = rig.scene();
        assert!(scene.fog && scene.fog_near > 1000.0);
    }

    #[test]
    fn ir_samplers_map_to_bevys() {
        let d = sampler(&Sampler {
            wrap_u: Wrap::Repeat,
            wrap_v: Wrap::Clamp,
            mag: Filter::Linear,
            min: Filter::Linear,
            mip: Some(Filter::Linear),
        });
        assert_eq!(
            (d.address_mode_u, d.address_mode_v),
            (ImageAddressMode::Repeat, ImageAddressMode::ClampToEdge)
        );
        assert_eq!(
            (d.mag_filter, d.min_filter, d.mipmap_filter, d.lod_max_clamp),
            (
                ImageFilterMode::Linear,
                ImageFilterMode::Linear,
                ImageFilterMode::Linear,
                32.0
            )
        );
        let flat = sampler(&Sampler {
            wrap_u: Wrap::Clamp,
            wrap_v: Wrap::Clamp,
            mag: Filter::Nearest,
            min: Filter::Nearest,
            mip: None,
        });
        assert_eq!(
            (flat.min_filter, flat.lod_max_clamp),
            (ImageFilterMode::Nearest, 0.0)
        );
    }

    #[test]
    fn screenshots_refuse_git_work_trees() {
        let dir = tempfile::tempdir().unwrap();
        let ok = screenshot_path(&dir.path().join("eruptor.png")).unwrap();
        assert_eq!(ok, dir.path().canonicalize().unwrap().join("eruptor.png"));
        assert!(screenshot_path(&dir.path().join("eruptor.jpg")).is_err());
        assert!(screenshot_path(&dir.path().join("missing/eruptor.png")).is_err());

        // A repo (a .git dir) and a worktree (a .git file), from deeper down.
        for (name, as_file) in [("repo", false), ("worktree", true)] {
            let tree = dir.path().join(name);
            let deep = tree.join("a/b");
            std::fs::create_dir_all(&deep).unwrap();
            if as_file {
                std::fs::write(tree.join(".git"), "gitdir: elsewhere\n").unwrap();
            } else {
                std::fs::create_dir(tree.join(".git")).unwrap();
            }
            let e = screenshot_path(&deep.join("x.png")).unwrap_err();
            assert!(e.to_string().contains("git work tree"), "{e}");
            // `..` back into the tree doesn't slip past.
            let around = dir.path().join("out");
            std::fs::create_dir_all(&around).unwrap();
            let sneaky = around.join("..").join(name).join("x.png");
            assert!(screenshot_path(&sneaky).is_err());
        }
        // This crate's own checkout is a work tree.
        let here = Path::new(env!("CARGO_MANIFEST_DIR")).join("x.png");
        assert!(screenshot_path(&here).is_err());
    }

    fn pixel() -> Image {
        Image::new_fill(
            bevy::render::render_resource::Extent3d {
                width: 2,
                height: 1,
                depth_or_array_layers: 1,
            },
            bevy::render::render_resource::TextureDimension::D2,
            &[10, 20, 30, 255],
            TextureFormat::Rgba8UnormSrgb,
            RenderAssetUsages::default(),
        )
    }

    #[cfg(unix)]
    #[test]
    fn screenshots_dont_follow_links_into_a_tree() {
        let dir = tempfile::tempdir().unwrap();
        let tree = dir.path().join("repo");
        std::fs::create_dir_all(tree.join(".git")).unwrap();
        let out = dir.path().join("out");
        std::fs::create_dir(&out).unwrap();

        // A symlink, dangling or not, is refused before and at the write.
        let leak = tree.join("leak.png");
        std::os::unix::fs::symlink(&leak, out.join("x.png")).unwrap();
        let e = screenshot_path(&out.join("x.png")).unwrap_err();
        assert!(e.to_string().contains("symlink"), "{e}");
        assert!(write_png(&out.join("x.png"), pixel()).is_err());
        std::fs::write(&leak, b"tracked").unwrap();
        assert!(screenshot_path(&out.join("x.png")).is_err());
        assert_eq!(std::fs::read(&leak).unwrap(), b"tracked");

        // A hard link is renamed over: the tree's name keeps its bytes.
        std::fs::hard_link(&leak, out.join("y.png")).unwrap();
        write_png(&out.join("y.png"), pixel()).unwrap();
        assert_eq!(std::fs::read(&leak).unwrap(), b"tracked");
        let png = std::fs::read(out.join("y.png")).unwrap();
        assert_eq!(&png[1..4], b"PNG");
        // Only the two links and the PNG; no temp file left behind.
        assert_eq!(std::fs::read_dir(&out).unwrap().count(), 2);
    }

    #[cfg(unix)]
    #[test]
    fn a_failed_screenshot_write_is_an_error() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let ro = dir.path().join("ro");
        std::fs::create_dir(&ro).unwrap();
        std::fs::set_permissions(&ro, std::fs::Permissions::from_mode(0o555)).unwrap();
        let r = write_png(&ro.join("y.png"), pixel());
        std::fs::set_permissions(&ro, std::fs::Permissions::from_mode(0o755)).unwrap();
        assert!(r.is_err());
        assert_eq!(std::fs::read_dir(&ro).unwrap().count(), 0);
        // The directory gone after the check.
        assert!(write_png(&dir.path().join("gone/y.png"), pixel()).is_err());
    }
}
