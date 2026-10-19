//! An oracle frame as hero_view draws it (openspec AE.10, AC.18's camera
//! options): the capture's model-view-projection split into a Bevy camera,
//! and the frame's rig, fog and idle time from a file beside the capture.
//! `hero_view.rs` and AE.12's GPU lane (`tests/remaster_gpu_lane.rs`) both
//! include this file, so the lane draws at the camera hero_view draws at.
//! Its tests are hero_view's.
//!
//! A frame file (JSON): `{"source": "...", "idle_time": s, "lights": [three
//! of {"travel": [x, y, z], "ambient": [r, g, b], "diffuse": [r, g, b],
//! "specular": [r, g, b]}], "fog": {"colour": [r, g, b], "near": n, "far":
//! f} or null}`. Each travel is the way the light goes, a unit vector in the
//! frame's eye space (the game's `ig_light_directional_direction`); colours
//! are the 8-bit codes the game stores, which the remaster decodes; fog
//! distances run along the view direction; `idle_time`, optional, is the
//! idle clip's time the frame drew. `source` is printed and otherwise
//! ignored.

use std::path::Path;

use anyhow::{Context, Result, anyhow, bail, ensure};
use bevy::camera::{CameraProjection, SubCameraView};
use bevy::math::{DMat3, DMat4, DQuat, DVec3, DVec4, Mat4, Vec3, Vec3A};
use bevy::prelude::{Resource, Transform};
use guac_ir::shading::{self, Scene};
use guacamole::character::remaster;
use serde::Deserialize;

/// The git work tree `dir` sits in (a `.git` dir or file at or above it),
/// with `dir` already canonical, so `..` and symlinks can't slip past.
pub fn work_tree(dir: &Path) -> Option<&Path> {
    dir.ancestors().find(|a| a.join(".git").exists())
}

/// 16 numbers from `--view-proj`'s arguments, or a file's text, separate
/// or pasted as one (commas, brackets and whitespace between them, so a
/// JSON matrix pastes as is).
pub fn view_proj(it: &mut impl Iterator<Item = String>) -> Result<[f32; 16]> {
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

// ---- the frame's camera ----

/// Bevy's clip from GL's: Bevy's depth runs from 1 at the near plane to 0
/// at the far one (reverse Z), where a GL-style projection's clip z runs
/// from -w to w, so z' = (w - z) / 2.
pub const BEVY_FROM_GL: DMat4 = DMat4::from_cols_array(&[
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
pub struct FrameCamera {
    pub scale: f32,
    pub transform: Transform,
    pub projection: CaptureProjection,
    /// Vertical field of view, degrees, and the near and far planes: for
    /// the record, read off the projection.
    pub fov: f64,
    pub near: f64,
    pub far: f64,
}

/// What the GL-style perspective of a frame leaves Bevy: its clip from view
/// with Bevy's depth, fixed whatever the target's size (the frame's NDC
/// fills the target, as the game's fills its 960x540 surface).
#[derive(Debug, Clone, PartialEq)]
pub struct CaptureProjection {
    pub clip_from_view: Mat4,
    pub far: f32,
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
    pub fn from_view_proj(m: &[f32; 16]) -> Result<Self> {
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

// ---- the frame's rig, fog and idle time ----

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FrameFile {
    #[serde(default)]
    pub source: Option<String>,
    #[serde(default)]
    pub idle_time: Option<f64>,
    pub lights: [FrameLight; 3],
    pub fog: Option<FrameFog>,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FrameLight {
    pub travel: [f32; 3],
    pub ambient: [u8; 3],
    pub diffuse: [u8; 3],
    pub specular: [u8; 3],
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FrameFog {
    pub colour: [u8; 3],
    pub near: f32,
    pub far: f32,
}

/// The fog a rig carries: its colour in linear light and its distances
/// along the view direction.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Fog {
    pub colour: [f32; 3],
    pub near: f32,
    pub far: f32,
}

/// Three directional lights fixed to the camera (each travel in view
/// space), their colours in linear light, the fog, and the idle time the
/// frame drew, when it says.
#[derive(Debug, Clone, PartialEq, Resource)]
pub struct Rig {
    pub lights: [shading::DirectionalLight; 3],
    pub fog: Option<Fog>,
    pub idle_time: Option<f64>,
}

impl Rig {
    /// A frame file's rig: refused inside a git work tree, so a frame's
    /// numbers stay beside its capture.
    pub fn read(path: &Path) -> Result<Self> {
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

    /// The colours decoded as the remaster decodes them (design decision
    /// 12), through [`remaster::light`] and [`remaster::linear`].
    pub fn from_file(file: &FrameFile) -> Result<Self> {
        let mut lights = [shading::DirectionalLight::OFF; 3];
        for (i, (l, out)) in file.lights.iter().zip(&mut lights).enumerate() {
            let length = Vec3::from_array(l.travel).length();
            ensure!(
                (length - 1.0).abs() < 1e-3,
                "frame light {i}: its travel is {length} long, not a unit vector"
            );
            *out = remaster::light(l.travel, l.ambient, l.diffuse, l.specular);
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
                    colour: remaster::linear(f.colour),
                    near: f.near,
                    far: f.far,
                })
            }
            None => None,
        };
        if let Some(t) = file.idle_time {
            ensure!(
                t.is_finite() && t >= 0.0,
                "frame idle_time {t} isn't a time"
            );
        }
        Ok(Self {
            lights,
            fog,
            idle_time: file.idle_time,
        })
    }

    /// The scene the character material draws in: this rig, fog as it
    /// says, shadows and point lights off, neutral placement, no flash.
    pub fn scene(&self) -> Scene {
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
