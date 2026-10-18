// Bevy system signatures trip both by design.
#![allow(clippy::type_complexity, clippy::too_many_arguments)]
//! hero_view: Eruptor off the user's Giants dump, skinned, textured and
//! playing his idle on a turntable (openspec AC.10, design decision 8).
//! It decodes in memory: IGA, the `level.bld` IGZ, the hero walk, Edge
//! segments, each technique's unit-0 colour texture as KTX2, the Havok
//! skeleton and `Eruptor_Idle.ma`. Joints map by name between `igSkeleton2`
//! and Havok, and skin with Havok's pose times an inverse bind: Alchemy's
//! inverse joint by default (AC.8's working bind), or the inverse of Havok's
//! reference model pose with `--bind havok`. The two disagree on J_wrist_R's
//! subtree by 20.8 deg; AC.17 settles which the game uses. It writes nothing
//! but `--screenshot`, refused inside any git work tree or through a symlink,
//! and exits non-zero when the screenshot isn't written.
//!
//! `cargo run -p guacamole --release --example hero_view -- [--at <s>]
//! [--yaw <deg>] [--bind alchemy|havok] [--screenshot <png> [--frames <n>]
//! [--offscreen]] [--unlit]`

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use anyhow::{Context, Result, anyhow, bail, ensure};
use bevy::app::ScheduleRunnerPlugin;
use bevy::asset::RenderAssetUsages;
use bevy::camera::RenderTarget;
use bevy::camera::visibility::NoFrustumCulling;
use bevy::image::{
    CompressedImageFormats, ImageAddressMode, ImageFilterMode, ImageSampler,
    ImageSamplerDescriptor, ImageType,
};
use bevy::log::LogPlugin;
use bevy::mesh::skinning::{SkinnedMesh, SkinnedMeshInverseBindposes};
use bevy::mesh::{Indices, PrimitiveTopology, VertexAttributeValues};
// Not the glob: its `ContextExt` clashes with anyhow's `Context`.
use bevy::prelude::{
    App, AppExit, Assets, Camera3d, ChildOf, ClearColor, Color, Commands, Component,
    DefaultPlugins, DirectionalLight, Entity, GlobalAmbientLight, Handle, Image, Local, Mat4, Mesh,
    Mesh3d, MeshMaterial3d, MessageWriter, Name, On, PluginGroup, Quat, Query, Res, ResMut,
    Resource, StandardMaterial, Startup, Time, Transform, Update, Vec3, Visibility, Window,
    WindowPlugin, With, Without, default, warn,
};
use bevy::render::render_resource::TextureFormat;
use bevy::render::renderer::RenderDevice;
use bevy::render::view::window::screenshot::{Screenshot, ScreenshotCaptured};
use bevy::window::ExitCondition;
use bevy::winit::WinitPlugin;
use guac_formats::edge::{BoneMap, BoneWeight, Segment, Semantic};
use guac_formats::havok::{self, QsTransform};
use guac_formats::iga::Archive;
use guac_formats::igz::Igz;
use guac_formats::igz::layouts::{
    self, Hero, Image2, Layout, Mat44, Payload, PhysicsModel, SkeletonBone, TextureAttr2,
};
use guac_formats::texture::Texture;

const ARCHIVE: &str = "PS3_GAME/USRDIR/character/009_eruptor.bld";
const MODEL: &str = "/Eruptor.mdl";
const IDLE: &str = "/Eruptor_Idle.ma";
/// The colour map's unit: every character technique binds it once.
const COLOUR_UNIT: i16 = 0;
const SIZE: (u32, u32) = (1280, 720);
/// Radians per second.
const TURNTABLE: f32 = 0.5;

fn main() -> Result<()> {
    let args = Args::parse(std::env::args().skip(1))?;
    let shot = args
        .screenshot
        .as_deref()
        .map(screenshot_path)
        .transpose()?;
    let decoded = decode(&dump_root()?, args.bind)?;
    ensure!(run(args, shot, decoded).is_success(), "hero_view failed");
    Ok(())
}

/// Which inverse bind the skin uses (AC.17 decides).
#[derive(Debug, Default, Clone, Copy, PartialEq)]
enum Bind {
    /// `igSkeleton2`'s inverse joints, AC.8's working bind.
    #[default]
    Alchemy,
    /// The inverse of Havok's reference model pose.
    Havok,
}

#[derive(Debug, Default, PartialEq)]
struct Args {
    /// Freezes the clip at this time and stops the turntable.
    at: Option<f64>,
    /// The turntable's angle while frozen, degrees.
    yaw: f32,
    bind: Bind,
    /// Saves a frame after `frames` frames, then exits.
    screenshot: Option<PathBuf>,
    frames: u32,
    /// No window: renders into an image (needs `screenshot`).
    offscreen: bool,
    /// The texture without lighting.
    unlit: bool,
}

impl Args {
    fn parse(mut it: impl Iterator<Item = String>) -> Result<Self> {
        let mut a = Self {
            frames: 240,
            ..Self::default()
        };
        while let Some(arg) = it.next() {
            let mut value = || it.next().ok_or_else(|| anyhow!("{arg} needs a value"));
            match arg.as_str() {
                "--at" => a.at = Some(value()?.parse().context("--at")?),
                "--yaw" => a.yaw = value()?.parse().context("--yaw")?,
                "--bind" => {
                    a.bind = match value()?.as_str() {
                        "alchemy" => Bind::Alchemy,
                        "havok" => Bind::Havok,
                        v => bail!("--bind takes alchemy or havok, not {v}"),
                    }
                }
                "--screenshot" => a.screenshot = Some(value()?.into()),
                "--frames" => a.frames = value()?.parse().context("--frames")?,
                "--offscreen" => a.offscreen = true,
                "--unlit" => a.unlit = true,
                other => bail!("unknown argument {other}"),
            }
        }
        ensure!(
            !a.offscreen || a.screenshot.is_some(),
            "--offscreen needs --screenshot"
        );
        ensure!(a.frames > 0, "--frames must be at least 1");
        Ok(a)
    }
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
    if let Some(tree) = dir.ancestors().find(|a| a.join(".git").exists()) {
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

/// One segment, ready for a Bevy mesh.
struct SegmentMesh {
    label: String,
    positions: Vec<[f32; 3]>,
    normals: Vec<[f32; 3]>,
    uvs: Vec<[f32; 2]>,
    joints: Vec<[u16; 4]>,
    weights: Vec<[f32; 4]>,
    indices: Vec<u16>,
    /// Into [`Decoded::textures`].
    texture: Option<usize>,
}

struct DecodedTexture {
    name: String,
    ktx2: Vec<u8>,
    sampler: ImageSamplerDescriptor,
}

#[derive(Resource)]
struct Decoded {
    segments: Vec<SegmentMesh>,
    textures: Vec<DecodedTexture>,
    inverse_bindposes: Vec<Mat4>,
    skeleton: havok::Skeleton<'static>,
    clip: havok::Clip<'static>,
}

fn decode(root: &Path, bind: Bind) -> Result<Decoded> {
    let started = Instant::now();
    let path = root.join(ARCHIVE);
    let bytes = std::fs::read(&path).with_context(|| format!("read {}", path.display()))?;
    let archive = Archive::parse(&bytes)?;
    let entry = archive
        .entries()
        .iter()
        .find(|e| e.name().ends_with("level.bld"))
        .context("no level.bld in the hero's archive")?;
    // Leaked so the decoded borrows live as long as the app.
    let level: &'static [u8] = Vec::leak(archive.read(entry)?.into_owned());
    let read = started.elapsed();

    let igz = Igz::parse(level)?;
    let (model, _) = layouts::read_all::<PhysicsModel>(&igz)
        .find(|(_, m)| {
            m.as_ref()
                .is_ok_and(|m| m.model.info.name.is_some_and(|n| n.ends_with(MODEL)))
        })
        .context("no Eruptor.mdl")?;
    let hero = Hero::read(&igz, model)?;
    let walked = started.elapsed();

    let skeleton = match hero.body.body.data_info.payload() {
        Some(Payload::Havok(b)) => havok::Container::parse(b)?
            .skeletons
            .into_iter()
            .next()
            .context("no hkaSkeleton in the body")?,
        _ => bail!("the body holds no Havok skeleton"),
    };
    let idle = hero
        .clips
        .values()
        .filter_map(|c| c.animation.map(|(_, a)| a))
        .find(|a| {
            a.info
                .data_info
                .info
                .name
                .is_some_and(|n| n.ends_with(IDLE))
        })
        .and_then(|a| match a.info.data_info.payload() {
            Some(Payload::Havok(b)) => Some(b),
            _ => None,
        })
        .context("no Eruptor_Idle.ma")?;
    let clip = havok::Container::parse(idle)?
        .clip
        .context("the idle holds no clip")?;
    let joints = JointMap::new(&skeleton, &hero.bones)?;
    let alchemy = joints.inverse_bindposes(&hero.skeleton.inverse_joints)?;
    let reference = havok_inverse_bindposes(&skeleton)?;
    let off: Vec<(usize, f32)> = bind_disagreement(&reference, &alchemy)
        .into_iter()
        .enumerate()
        .filter(|(_, (deg, moved))| *deg > 1e-3 || *moved > 1e-4)
        .map(|(j, (deg, _))| (j, deg))
        .collect();
    println!(
        "binds: Havok and Alchemy disagree on {} of {} joints {:?}, worst {:.2} deg; skinning with {bind:?}",
        off.len(),
        alchemy.len(),
        off.iter()
            .map(|(j, _)| skeleton.bones[*j].name.unwrap_or("?"))
            .collect::<Vec<_>>(),
        off.iter().map(|(_, d)| *d).fold(0.0, f32::max),
    );
    let inverse_bindposes = match bind {
        Bind::Alchemy => alchemy,
        Bind::Havok => reference,
    };
    let havoked = started.elapsed();

    let mut textures: Vec<(usize, DecodedTexture)> = Vec::new();
    let mut segments = Vec::new();
    for mesh in &hero.meshes {
        let palette = mesh
            .palette
            .and_then(|p| hero.palettes.get(&p)?.indices)
            .and_then(|i| hero.int_lists.get(&i))
            .context("a mesh without a skin palette")?;
        let bone_map = BoneMap::new(&palette.values, &hero.bones);
        let texture = match hero.texture_at(mesh, COLOUR_UNIT) {
            Some((t, attr)) => Some(texture_slot(&igz, &mut textures, t, attr)?),
            None => None,
        };
        for (index, layout) in hero.mesh_segments(mesh) {
            let segment = Segment::decode(&igz, index, layout)?;
            let label = format!("{} #{index}", mesh.technique.node.name.unwrap_or("?"));
            segments.push(segment_mesh(label, &segment, &bone_map, &joints, texture)?);
        }
    }
    let done = started.elapsed();
    let ms = |d: Duration| d.as_secs_f64() * 1e3;
    println!(
        "decoded in {:.1} ms: IGA {:.1}, IGZ + hero walk {:.1}, Havok {:.1}, Edge + KTX2 {:.1}",
        ms(done),
        ms(read),
        ms(walked - read),
        ms(havoked - walked),
        ms(done - havoked),
    );
    println!(
        "{} segments, {} vertices, {} triangles, {} joints; idle {:.4} s over {} frames; colour {:?}",
        segments.len(),
        segments.iter().map(|s| s.positions.len()).sum::<usize>(),
        segments.iter().map(|s| s.indices.len() / 3).sum::<usize>(),
        inverse_bindposes.len(),
        clip.animation.duration,
        clip.animation.num_frames,
        textures.iter().map(|(_, t)| &t.name).collect::<Vec<_>>(),
    );
    Ok(Decoded {
        segments,
        textures: textures.into_iter().map(|(_, t)| t).collect(),
        inverse_bindposes,
        skeleton,
        clip,
    })
}

/// The decoded texture for `igTextureAttr2` `t`, converted once.
fn texture_slot(
    igz: &Igz<'_>,
    textures: &mut Vec<(usize, DecodedTexture)>,
    t: usize,
    attr: &TextureAttr2,
) -> Result<usize> {
    if let Some(i) = textures.iter().position(|(k, _)| *k == t) {
        return Ok(i);
    }
    let image = attr.image.context("the colour texture has no image")?;
    let name = Image2::read(igz, image)?
        .name
        .and_then(|n| n.rsplit('/').next())
        .unwrap_or("?")
        .to_owned();
    let ktx2 = Texture::read(igz, image)?.to_ktx2();
    textures.push((
        t,
        DecodedTexture {
            name,
            ktx2,
            sampler: sampler(attr)?,
        },
    ));
    Ok(textures.len() - 1)
}

/// `igTextureAttr2`'s filters and wraps as a sampler, for the values the
/// character files use; others are refused. Wrap 0 is clamp, 1 repeat;
/// filters run nearest, linear, then the four mipmapped (5 is linear-linear).
fn sampler(attr: &TextureAttr2) -> Result<ImageSamplerDescriptor> {
    let wrap = |v: u32| match v {
        0 => Ok(ImageAddressMode::ClampToEdge),
        1 => Ok(ImageAddressMode::Repeat),
        _ => Err(anyhow!("texture wrap {v}")),
    };
    let mag = match attr.mag_filter {
        0 => ImageFilterMode::Nearest,
        1 => ImageFilterMode::Linear,
        v => bail!("mag filter {v}"),
    };
    let (min, mip) = match attr.min_filter {
        0 => (ImageFilterMode::Nearest, None),
        1 => (ImageFilterMode::Linear, None),
        5 => (ImageFilterMode::Linear, Some(ImageFilterMode::Linear)),
        v => bail!("min filter {v}"),
    };
    Ok(ImageSamplerDescriptor {
        address_mode_u: wrap(attr.wrap_s)?,
        address_mode_v: wrap(attr.wrap_t)?,
        mag_filter: mag,
        min_filter: min,
        mipmap_filter: mip.unwrap_or(ImageFilterMode::Nearest),
        lod_max_clamp: if mip.is_some() { 32.0 } else { 0.0 },
        ..ImageSamplerDescriptor::default()
    })
}

/// Havok joints against the Alchemy bones, by name.
#[derive(Debug, PartialEq)]
struct JointMap {
    /// Alchemy bone -> Havok joint; `None` for a bone Havok lacks.
    joint_of_bone: Vec<Option<usize>>,
    /// Havok joint -> its bone's `_blendMatrixIndex`.
    blend_of_joint: Vec<i32>,
}

impl JointMap {
    /// Every Havok bone must name exactly one Alchemy bone.
    fn new(skeleton: &havok::Skeleton<'_>, bones: &[SkeletonBone<'_>]) -> Result<Self> {
        let mut joint_of_bone = vec![None; bones.len()];
        let mut blend_of_joint = Vec::with_capacity(skeleton.bones.len());
        for (j, b) in skeleton.bones.iter().enumerate() {
            let name = b
                .name
                .with_context(|| format!("Havok bone {j} has no name"))?;
            let mut found = bones
                .iter()
                .enumerate()
                .filter(|(_, a)| a.name == Some(name));
            let (i, bone) = match (found.next(), found.next()) {
                (Some(one), None) => one,
                (None, _) => bail!("no Alchemy bone named {name}"),
                (Some(_), Some(_)) => bail!("two Alchemy bones named {name}"),
            };
            joint_of_bone[i] = Some(j);
            blend_of_joint.push(bone.blend_matrix_index);
        }
        Ok(Self {
            joint_of_bone,
            blend_of_joint,
        })
    }

    /// Each joint's inverse joint matrix as Bevy's `Mat4`.
    fn inverse_bindposes(&self, inverse_joints: &[Mat44]) -> Result<Vec<Mat4>> {
        self.blend_of_joint
            .iter()
            .enumerate()
            .map(|(j, &b)| {
                let m = usize::try_from(b)
                    .ok()
                    .and_then(|b| inverse_joints.get(b))
                    .with_context(|| format!("joint {j}: blend matrix {b} has no inverse joint"))?;
                Ok(inverse_joint(m))
            })
            .collect()
    }
}

/// The inverse of each joint's Havok reference model pose.
fn havok_inverse_bindposes(skeleton: &havok::Skeleton<'_>) -> Result<Vec<Mat4>> {
    let model = skeleton
        .model_pose(&skeleton.reference_pose)
        .context("reference pose and parents differ in length")?;
    Ok(model
        .iter()
        .map(|q| transform(q).to_matrix().inverse())
        .collect())
}

/// Per joint, how far `havok^-1 x alchemy` is from identity: its rotation
/// in degrees and how far it moves the joint's own origin.
fn bind_disagreement(havok: &[Mat4], alchemy: &[Mat4]) -> Vec<(f32, f32)> {
    havok
        .iter()
        .zip(alchemy)
        .map(|(h, a)| {
            let world = h.inverse();
            let s = world * *a;
            let (_, r, _) = s.to_scale_rotation_translation();
            let origin = world.transform_point3(Vec3::ZERO);
            let moved = (s.transform_point3(origin) - origin).length();
            (r.angle_between(Quat::IDENTITY).to_degrees(), moved)
        })
        .collect()
}

/// Stored row-vector (translation in row 3), so its rows are the
/// column-vector matrix's columns.
fn inverse_joint(m: &Mat44) -> Mat4 {
    Mat4::from_cols_array_2d(m)
}

/// A segment's skin as Bevy joints and weights: palette slots -> Alchemy
/// bones (`Skin::bones`) -> Havok joints. Zero weights become joint 0.
fn skin_attributes(
    bones: &[[BoneWeight; 4]],
    joints: &JointMap,
) -> Result<(Vec<[u16; 4]>, Vec<[f32; 4]>)> {
    let mut indices = Vec::with_capacity(bones.len());
    let mut weights = Vec::with_capacity(bones.len());
    for (v, influences) in bones.iter().enumerate() {
        let mut j = [0u16; 4];
        let mut w = [0f32; 4];
        for (k, b) in influences.iter().enumerate() {
            if b.weight == 0 {
                continue;
            }
            let joint = joints
                .joint_of_bone
                .get(b.bone)
                .copied()
                .flatten()
                .with_context(|| format!("vertex {v}: bone {} has no Havok joint", b.bone))?;
            j[k] = u16::try_from(joint)?;
            w[k] = f32::from(b.weight) / 255.0;
        }
        indices.push(j);
        weights.push(w);
    }
    Ok((indices, weights))
}

fn segment_mesh(
    label: String,
    segment: &Segment,
    bone_map: &BoneMap,
    joints: &JointMap,
    texture: Option<usize>,
) -> Result<SegmentMesh> {
    let n = segment.vertex_count();
    let positions: Vec<[f32; 3]> = segment.positions().collect();
    let normals: Vec<[f32; 3]> = segment
        .attribute(Semantic::Normal)
        .with_context(|| format!("{label}: no normals"))?
        .vertices()
        .map(|v| [v[0], v[1], v[2]])
        .collect();
    let uvs: Vec<[f32; 2]> = segment
        .attribute(Semantic::Uv0)
        .with_context(|| format!("{label}: no uv0"))?
        .vertices()
        .map(|v| [v[0], v[1]])
        .collect();
    let skin = segment
        .skin
        .as_ref()
        .with_context(|| format!("{label}: unskinned"))?;
    let (joint_indices, weights) = skin_attributes(&skin.bones(bone_map)?, joints)?;
    ensure!(
        [normals.len(), uvs.len(), weights.len(), positions.len()] == [n; 4],
        "{label}: attribute counts differ"
    );
    Ok(SegmentMesh {
        label,
        positions,
        normals,
        uvs,
        joints: joint_indices,
        weights,
        indices: segment.triangles().flatten().collect(),
        texture,
    })
}

/// The clip's model-space pose at `t` seconds, looped over its duration.
fn pose(skeleton: &havok::Skeleton<'_>, clip: &havok::Clip<'_>, t: f64) -> Option<Vec<Transform>> {
    let d = f64::from(clip.animation.duration);
    let t = if d > 0.0 { t.rem_euclid(d) } else { 0.0 };
    let model = skeleton.model_pose(&clip.local_pose(skeleton, t)?)?;
    Some(model.iter().map(transform).collect())
}

fn transform(q: &QsTransform) -> Transform {
    let [x, y, z, w] = q.rotation.map(|c| c as f32);
    Transform {
        translation: Vec3::from_array(q.translation.map(|c| c as f32)),
        rotation: Quat::from_xyzw(x, y, z, w).normalize(),
        scale: Vec3::from_array(q.scale.map(|c| c as f32)),
    }
}

#[derive(Resource)]
struct View {
    at: Option<f64>,
    yaw: f32,
    frames: u32,
    shot: Option<PathBuf>,
    offscreen: bool,
    unlit: bool,
    /// The image the camera renders into when offscreen.
    target: Option<Handle<Image>>,
}

#[derive(Component)]
struct Turntable;

#[derive(Resource)]
struct Joints(Vec<Entity>);

fn run(args: Args, shot: Option<PathBuf>, decoded: Decoded) -> AppExit {
    let mut app = App::new();
    let plugins = DefaultPlugins.set(LogPlugin {
        filter: "wgpu=error,naga=warn".into(),
        ..default()
    });
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
                title: "hero_view: Eruptor".into(),
                resolution: SIZE.into(),
                ..default()
            }),
            ..default()
        }));
    }
    app.insert_resource(decoded)
        .insert_resource(View {
            at: args.at,
            yaw: args.yaw.to_radians(),
            frames: args.frames,
            shot,
            offscreen: args.offscreen,
            unlit: args.unlit,
            target: None,
        })
        .insert_resource(ClearColor(Color::srgb(0.05, 0.07, 0.16)))
        .insert_resource(GlobalAmbientLight {
            brightness: 300.0,
            ..default()
        })
        .add_systems(Startup, setup)
        .add_systems(Update, (animate, screenshot))
        .run()
}

fn setup(
    mut commands: Commands,
    mut view: ResMut<View>,
    decoded: Res<Decoded>,
    device: Option<Res<RenderDevice>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
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
    let textures: Vec<Handle<Image>> = decoded
        .textures
        .iter()
        .map(|t| {
            let image = Image::from_buffer(
                &t.ktx2,
                ImageType::Extension("ktx2"),
                formats,
                true,
                ImageSampler::Descriptor(t.sampler.clone()),
                RenderAssetUsages::RENDER_WORLD,
            )
            .unwrap_or_else(|e| panic!("{}: {e}", t.name));
            images.add(image)
        })
        .collect();

    let root = commands
        .spawn((
            Turntable,
            Transform::from_rotation(Quat::from_rotation_y(view.yaw)),
            Visibility::default(),
        ))
        .id();
    let start = pose(&decoded.skeleton, &decoded.clip, view.at.unwrap_or(0.0))
        .expect("a NORMAL clip over this skeleton");
    let joints: Vec<Entity> = start
        .into_iter()
        .map(|t| commands.spawn((t, ChildOf(root))).id())
        .collect();
    let inverse = bindposes.add(SkinnedMeshInverseBindposes::from(
        decoded.inverse_bindposes.clone(),
    ));
    for s in &decoded.segments {
        let mut mesh = Mesh::new(
            PrimitiveTopology::TriangleList,
            RenderAssetUsages::RENDER_WORLD,
        );
        mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, s.positions.clone());
        mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, s.normals.clone());
        mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, s.uvs.clone());
        mesh.insert_attribute(
            Mesh::ATTRIBUTE_JOINT_INDEX,
            VertexAttributeValues::Uint16x4(s.joints.clone()),
        );
        mesh.insert_attribute(Mesh::ATTRIBUTE_JOINT_WEIGHT, s.weights.clone());
        mesh.insert_indices(Indices::U16(s.indices.clone()));
        let material = StandardMaterial {
            base_color_texture: s.texture.map(|t| textures[t].clone()),
            perceptual_roughness: 0.8,
            unlit: view.unlit,
            ..default()
        };
        commands.spawn((
            Name::new(s.label.clone()),
            Mesh3d(meshes.add(mesh)),
            MeshMaterial3d(materials.add(material)),
            SkinnedMesh {
                inverse_bindposes: inverse.clone(),
                joints: joints.clone(),
            },
            NoFrustumCulling,
            ChildOf(root),
        ));
    }
    commands.insert_resource(Joints(joints));

    let eye = Transform::from_xyz(0.0, 1.3, 5.5).looking_at(Vec3::new(0.0, 1.0, 0.0), Vec3::Y);
    let camera = commands.spawn((Camera3d::default(), eye)).id();
    if view.offscreen {
        let mut target =
            Image::new_target_texture(SIZE.0, SIZE.1, TextureFormat::Rgba8UnormSrgb, None);
        target.asset_usage = RenderAssetUsages::default();
        let handle = images.add(target);
        commands
            .entity(camera)
            .insert(RenderTarget::Image(handle.clone().into()));
        view.target = Some(handle);
    }
    commands.spawn((
        DirectionalLight {
            illuminance: 4000.0,
            ..default()
        },
        Transform::from_xyz(2.0, 4.0, 5.0).looking_at(Vec3::new(0.0, 1.0, 0.0), Vec3::Y),
    ));
}

fn animate(
    time: Res<Time>,
    view: Res<View>,
    decoded: Res<Decoded>,
    joints: Option<Res<Joints>>,
    mut transforms: Query<&mut Transform, Without<Turntable>>,
    mut turntable: Query<&mut Transform, With<Turntable>>,
) {
    let Some(joints) = joints else { return };
    let t = view.at.unwrap_or_else(|| time.elapsed_secs_f64());
    if let Some(p) = pose(&decoded.skeleton, &decoded.clip, t) {
        for (e, p) in joints.0.iter().zip(p) {
            if let Ok(mut tr) = transforms.get_mut(*e) {
                *tr = p;
            }
        }
    }
    if view.at.is_none() {
        for mut tr in &mut turntable {
            tr.rotation = Quat::from_rotation_y(time.elapsed_secs() * TURNTABLE);
        }
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
    use guac_formats::edge::{Influence, Skin};
    use guac_formats::havok::spline::{Channel, Curve, QuatTrack, TransformTrack, VecTrack};
    use guac_formats::havok::{Binding, BlendHint, Bone, SplineAnimation};

    fn bone(name: &'static str, parent: i32, blend: i32) -> SkeletonBone<'static> {
        SkeletonBone {
            name: Some(name),
            parent_index: parent,
            blend_matrix_index: blend,
            translation: [0.0; 3],
        }
    }

    /// Havok order differs from Alchemy's, which adds an unnamed root.
    fn skeleton(names: &[&'static str], parents: &[i16]) -> havok::Skeleton<'static> {
        havok::Skeleton {
            name: Some("J_root"),
            parents: parents.to_vec(),
            bones: names
                .iter()
                .map(|n| Bone {
                    name: Some(n),
                    lock_translation: false,
                })
                .collect(),
            reference_pose: vec![QsTransform::IDENTITY; names.len()],
            reference_floats: Vec::new(),
            float_slots: Vec::new(),
        }
    }

    fn alchemy() -> Vec<SkeletonBone<'static>> {
        vec![
            bone("Unnamed bone", -1, -1),
            bone("J_root", 0, 0),
            bone("J_head", 1, 2),
            bone("J_hip", 1, 1),
        ]
    }

    #[test]
    fn bones_map_by_name() {
        let sk = skeleton(&["J_root", "J_hip", "J_head"], &[-1, 0, 0]);
        let map = JointMap::new(&sk, &alchemy()).unwrap();
        assert_eq!(
            map,
            JointMap {
                joint_of_bone: vec![None, Some(0), Some(2), Some(1)],
                blend_of_joint: vec![0, 1, 2],
            }
        );
        let missing = skeleton(&["J_root", "J_tail"], &[-1, 0]);
        let e = JointMap::new(&missing, &alchemy()).unwrap_err();
        assert!(e.to_string().contains("J_tail"), "{e}");
        let mut twice = alchemy();
        twice.push(bone("J_hip", 1, 3));
        assert!(JointMap::new(&sk, &twice).is_err());
    }

    #[test]
    fn inverse_joints_go_in_joint_order_and_transpose() {
        let sk = skeleton(&["J_root", "J_hip", "J_head"], &[-1, 0, 0]);
        let map = JointMap::new(&sk, &alchemy()).unwrap();
        // A distinct translation per blend index catches a wrong lookup.
        let ij: Vec<Mat44> = (0..3)
            .map(|i| {
                let mut m = Mat4::IDENTITY.to_cols_array_2d();
                m[3] = [10.0 + i as f32, 0.0, 0.0, 1.0];
                m
            })
            .collect();
        let ibp = map.inverse_bindposes(&ij).unwrap();
        let xs: Vec<f32> = ibp.iter().map(|m| m.w_axis.x).collect();
        assert_eq!(xs, [10.0, 11.0, 12.0]);
        // A column-vector translation moves a point.
        assert_eq!(
            ibp[1].transform_point3(Vec3::ZERO),
            Vec3::new(11.0, 0.0, 0.0)
        );
        assert!(map.inverse_bindposes(&ij[..2]).is_err());
    }

    #[test]
    fn the_havok_bind_inverts_the_reference_model_pose() {
        let mut sk = skeleton(&["J_root", "J_hip", "J_head"], &[-1, 0, 1]);
        let quarter = [0.0, 0.5f64.sqrt(), 0.0, 0.5f64.sqrt()];
        sk.reference_pose[0].translation = [0.0, 1.0, 0.0];
        sk.reference_pose[1].rotation = quarter;
        sk.reference_pose[2].translation = [2.0, 0.0, 0.0];
        let inv = havok_inverse_bindposes(&sk).unwrap();
        let model = sk.model_pose(&sk.reference_pose).unwrap();
        for (i, q) in inv.iter().zip(&model) {
            let id = transform(q).to_matrix() * *i;
            assert!(id.abs_diff_eq(Mat4::IDENTITY, 1e-6), "{id}");
        }
        // J_head sits 2 along the hip's turned x: -z, so its inverse
        // takes (0, 1, -2) home.
        let home = inv[2].transform_point3(Vec3::new(0.0, 1.0, -2.0));
        assert!(home.length() < 1e-6, "{home}");

        // An Alchemy bind turned 20 deg about the hip shows on the hip and
        // its child, nowhere else; the hip's own origin stays put.
        let mut alchemy = inv.clone();
        let hip = transform(&model[1]).to_matrix();
        let twist = hip * Mat4::from_rotation_y(20f32.to_radians()) * hip.inverse();
        for a in &mut alchemy[1..] {
            *a *= twist;
        }
        let d = bind_disagreement(&inv, &alchemy);
        assert!(d[0].0 < 1e-3 && d[0].1 < 1e-6, "{:?}", d[0]);
        for (deg, _) in &d[1..] {
            assert!((deg - 20.0).abs() < 1e-3, "{deg}");
        }
        assert!(d[1].1 < 1e-5, "{:?}", d[1]);
        assert!(d[2].1 > 0.5, "{:?}", d[2]);
    }

    #[test]
    fn palette_slots_become_joint_indices() {
        let sk = skeleton(&["J_root", "J_hip", "J_head"], &[-1, 0, 0]);
        let bones = alchemy();
        let map = JointMap::new(&sk, &bones).unwrap();
        // Slot 0 -> blend 2 (J_head), slot 1 -> blend 0 (J_root).
        let palette = BoneMap::new(&[2, 0], &bones);
        let skin = Skin {
            influences: vec![
                [
                    Influence {
                        weight: 200,
                        local: 1,
                    },
                    Influence {
                        weight: 55,
                        local: 0,
                    },
                    Influence::default(),
                    Influence::default(),
                ],
                [
                    Influence {
                        weight: 255,
                        local: 0,
                    },
                    Influence::default(),
                    Influence::default(),
                    Influence::default(),
                ],
            ],
            // Local 0 -> slot 1, local 1 -> slot 0.
            palette_slots: vec![1, 0],
        };
        let (j, w) = skin_attributes(&skin.bones(&palette).unwrap(), &map).unwrap();
        assert_eq!(j, [[2, 0, 0, 0], [0, 0, 0, 0]]);
        assert_eq!(w[0], [200.0 / 255.0, 55.0 / 255.0, 0.0, 0.0]);
        assert_eq!(w[1], [1.0, 0.0, 0.0, 0.0]);

        // A weighted bone Havok lacks is refused.
        let unnamed = [[
            BoneWeight {
                bone: 0,
                weight: 255,
            },
            BoneWeight::default(),
            BoneWeight::default(),
            BoneWeight::default(),
        ]];
        assert!(skin_attributes(&unnamed, &map).is_err());
    }

    /// Root rises 0 -> 1 over one second; the child sits 1 along x,
    /// turned a quarter about y.
    fn clip() -> havok::Clip<'static> {
        let fixed = |v: [f64; 3]| VecTrack {
            channels: [Channel::Static; 3],
            constant: v,
            curve: None,
        };
        let root = TransformTrack {
            mask: [0x45, 0, 0, 0],
            position: VecTrack {
                channels: [Channel::Static, Channel::Spline, Channel::Static],
                constant: [0.0; 3],
                curve: Some(Curve {
                    degree: 1,
                    knots: vec![0, 0, 1, 1],
                    points: vec![[0.0; 3], [0.0, 1.0, 0.0]],
                }),
            },
            rotation: QuatTrack::Static([0.0, 0.5f64.sqrt(), 0.0, 0.5f64.sqrt()]),
            scale: fixed([1.0; 3]),
        };
        let child = TransformTrack {
            mask: [0x45, 0, 0, 0],
            position: fixed([1.0, 0.0, 0.0]),
            rotation: QuatTrack::Identity,
            scale: fixed([1.0; 3]),
        };
        havok::Clip {
            animation: SplineAnimation {
                duration: 1.0,
                num_frames: 2,
                max_frames_per_block: 256,
                block_duration: 255.0,
                frame_duration: 1.0,
                data_len: 0,
                data_end: 0,
                tracks: vec![root, child],
            },
            binding: Binding {
                original_skeleton_name: None,
                track_to_bone: vec![0, 1],
                blend_hint: BlendHint::Normal,
            },
        }
    }

    #[test]
    fn the_clip_samples_to_model_space_and_loops() {
        let sk = skeleton(&["J_root", "J_hip"], &[-1, 0]);
        let c = clip();
        let p = pose(&sk, &c, 0.25).unwrap();
        assert!((p[0].translation - Vec3::new(0.0, 0.25, 0.0)).length() < 1e-6);
        // The child's x offset turned a quarter about y lands on -z.
        assert!((p[1].translation - Vec3::new(0.0, 0.25, -1.0)).length() < 1e-6);
        assert!(p[1].rotation.angle_between(p[0].rotation) < 1e-6);
        // Looped: 2.25 s and -0.75 s are 0.25 s.
        for t in [2.25, -0.75] {
            let q = pose(&sk, &c, t).unwrap();
            assert!((q[1].translation - p[1].translation).length() < 1e-6, "{t}");
        }
        // An additive clip has no pose of its own.
        let mut additive = clip();
        additive.binding.blend_hint = BlendHint::Additive;
        assert!(pose(&sk, &additive, 0.0).is_none());
    }

    #[test]
    fn samplers_map_the_disc_values_and_refuse_the_rest() {
        let attr = |mag, min, s, t| TextureAttr2 {
            attr: layouts::Attr {
                cached_unit_id: -1,
                cached_attr_index: -1,
                read_only_copy: false,
            },
            format_hint: 0,
            load_in_vram_for_ctr: false,
            mag_filter: mag,
            min_filter: min,
            wrap_s: s,
            wrap_t: t,
            msaa_type: 0,
            source: 0,
            image: None,
            tex_handle: -1,
            image_handle: 0,
            wrap_r: 1,
        };
        let d = sampler(&attr(1, 5, 1, 0)).unwrap();
        assert_eq!(
            (d.address_mode_u, d.address_mode_v),
            (ImageAddressMode::Repeat, ImageAddressMode::ClampToEdge)
        );
        assert_eq!(
            (d.mag_filter, d.min_filter, d.mipmap_filter),
            (
                ImageFilterMode::Linear,
                ImageFilterMode::Linear,
                ImageFilterMode::Linear
            )
        );
        let flat = sampler(&attr(0, 0, 0, 0)).unwrap();
        assert_eq!(flat.min_filter, ImageFilterMode::Nearest);
        assert_eq!(flat.lod_max_clamp, 0.0);
        assert!(sampler(&attr(2, 5, 1, 1)).is_err());
        assert!(sampler(&attr(1, 3, 1, 1)).is_err());
        assert!(sampler(&attr(1, 5, 4, 1)).is_err());
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

    #[test]
    fn args_parse() {
        let parse = |s: &str| Args::parse(s.split_whitespace().map(String::from));
        let a = parse("--at 0.5 --yaw 90 --screenshot /tmp/x.png --offscreen --unlit").unwrap();
        assert_eq!(a.at, Some(0.5));
        assert_eq!(
            (a.yaw, a.frames, a.offscreen, a.unlit),
            (90.0, 240, true, true)
        );
        assert!(parse("--offscreen").is_err());
        assert!(parse("--frames 0").is_err());
        assert!(parse("--at").is_err());
        assert!(parse("--bogus").is_err());
        assert_eq!(a.bind, Bind::Alchemy);
        assert_eq!(parse("--bind havok").unwrap().bind, Bind::Havok);
        assert_eq!(parse("--bind alchemy").unwrap().bind, Bind::Alchemy);
        assert!(parse("--bind both").is_err());
    }
}
