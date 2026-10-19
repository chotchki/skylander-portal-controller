// Bevy system signatures trip both by design.
#![allow(clippy::type_complexity, clippy::too_many_arguments)]
//! The remaster on Eruptor at an oracle frame (openspec AE.12, design
//! decision 12): his main pass against the character-shading spec's CPU
//! model, then the tonemapped look measured the way ae-color measured
//! Open question 1's options, and one tinted hero material recorded.
//!
//! The frame is AC.13's savestate frame 4, read at run time from
//! `GUAC_CAPTURE_DIR` (`BLUS30968_1_1.frame4.json`, its rig, fog and idle
//! time, and `.frame4.view-proj.txt`, its camera; AE.10 wrote both beside
//! the savestate), through hero_view's own frame reader, so the lane draws
//! at the camera hero_view draws at. The models come off `GUAC_GIANTS_DUMP`
//! through the model stage's reader, in memory.
//!
//! 1. The main pass: each character's opaque parts through the real
//!    pipeline into `Rgba16Float` with tonemapping and MSAA off, and 13
//!    probe passes of the same draw ([`probe.wesl`](remaster_gpu_lane/probe.wesl):
//!    the character shader's own vertex stage, a fragment stage writing what
//!    the colour pass reads). The model shades each pixel's probed inputs
//!    and every channel has to land within one half-precision step of it,
//!    as AE.8's lane bounds it (the M3 Max truncates to half on store; under
//!    2^-10 the step is 2^-10's). Textures are the disc's level 0 decoded
//!    exactly (sRGB's curve on colour, spec and rim maps' RGB, code / 255
//!    otherwise), stored as halves and sampled nearest, so the shader and
//!    the model read the same texels: AE.8 measured the M3 Max's own 8-bit
//!    sRGB decode up to 1.44 half steps off the exact one.
//! 2. The look: the remaster as `character::remaster` draws it (the disc's
//!    BC textures through their views with the IR's samplers, 4x MSAA,
//!    TonyMcMapface into `Rgba8UnormSrgb`), with and without the glow.
//!    Oklch over his pixels (ae-color's oracle-free hero mask, his coverage
//!    eroded 2 px; rock and lava by the colour map's luma, under 0.20 and
//!    over 0.45, each eroded 1 px), against the design's (d) row, and the
//!    remaster-look spec's lava requirement asserted.
//! 3. Sunburn's body (`PlantDragon`, tint (201, 132, 80)): what the tint does
//!    in linear light, beside what it does in the game's arithmetic.
//!
//! Nothing is written. `cargo test -p guacamole --release --test
//! remaster_gpu_lane -- --ignored --nocapture`

#[allow(dead_code)]
#[path = "../examples/hero_view/frame.rs"]
mod frame;

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use bevy::app::ScheduleRunnerPlugin;
use bevy::asset::io::embedded::EmbeddedAssetRegistry;
use bevy::asset::{Asset, RenderAssetUsages};
use bevy::camera::visibility::RenderLayers;
use bevy::camera::{Camera, ClearColorConfig, Hdr, Projection, RenderTarget};
use bevy::color::{Color, LinearRgba};
use bevy::core_pipeline::tonemapping::{DebandDither, Tonemapping};
use bevy::image::{
    CompressedImageFormats, ImageAddressMode, ImageFilterMode, ImageSampler, ImageSamplerDescriptor,
};
use bevy::log::LogPlugin;
use bevy::material::AlphaMode;
use bevy::math::{UVec2, Vec3};
use bevy::mesh::MeshVertexBufferLayoutRef;
use bevy::mesh::skinning::SkinnedMeshInverseBindposes;
use bevy::pbr::{Material, MaterialPipeline, MaterialPipelineKey, MaterialPlugin};
use bevy::prelude::{
    App, AppExit, Assets, Camera3d, ClearColor, Commands, DefaultPlugins, Handle, Image, Local,
    Mesh, MessageWriter, On, PluginGroup, Res, ResMut, Resource, Startup, Transform, Update,
    WindowPlugin, default,
};
use bevy::reflect::TypePath;
use bevy::render::render_resource::{
    AsBindGroup, Extent3d, RenderPipelineDescriptor, SpecializedMeshPipelineError,
    TextureDimension, TextureFormat,
};
use bevy::render::renderer::RenderDevice;
use bevy::render::view::Msaa;
use bevy::render::view::window::screenshot::{Screenshot, ScreenshotCaptured};
use bevy::shader::{ShaderDefVal, ShaderRef};
use bevy::window::ExitCondition;
use bevy::winit::WinitPlugin;
use guac_extract::models;
use guac_formats::texture::{Format, Texture};
use guac_ir::model::{self, Blend, Model, TextureRole, Wrap};
use guac_ir::shading::{self, Flat, Fragment, Scene, Textures};
use guac_ir::{DumpIdentity, Sha256};
use guacamole::character::material::{
    CharacterUniform, SHADER, reference_material, specialize_for,
};
use guacamole::character::{
    CharacterMaterial, CharacterShadingPlugin, GlowPlugin, MaterialKey, ShadowLookup, remaster,
    spawn_character, spawn_glow_camera, spawn_glow_masks,
};

use frame::{FrameCamera, FrameFile, Rig, view_proj};

/// AC.13's frame 4, as AE.10 wrote it beside the savestate.
const FRAME: &str = "BLUS30968_1_1.frame4.json";
const VIEW_PROJ: &str = "BLUS30968_1_1.frame4.view-proj.txt";
const ERUPTOR: &str = "009_Eruptor";
/// Sunburn: his body (`PlantDragon`) is a whole opaque body tinted a colour,
/// (201, 132, 80). AE.12's scan of the 237 hero materials:
/// 142 carry a tint other than 128, 17 of them coloured, on 9 models; the
/// Storm Giant's (Lightning Rod's) are grey.
const SUNBURN: &str = "008_FireDragon";
const TINTED: &str = "PlantDragon";
const NEUTRAL: [u8; 4] = [128, 128, 128, 255];
/// The game's own surface.
const SIZE: UVec2 = UVec2::new(960, 540);
const PROBES: u32 = 13;
/// The main pass's clear colour, which no shaded pixel lands on.
const CLEAR: [f32; 4] = [0.25, 0.5, 0.125, 0.75];
const PROBE_SHADER: &str = "embedded://guacamole_lane/probe.wesl";
/// hero_view's background.
const BACKGROUND: Color = Color::srgb(0.05, 0.07, 0.16);
/// The look's bound and the design's (d) row (ae-color's knob table: hero
/// mean chroma, mean lightness, share at full red and green).
const LAVA_HUE: f64 = 95.0;
const LAVA_CLIP: f64 = 0.01;
const D_ROW: (f64, f64, f64) = (0.0963, 0.522, 0.0);

// ---- inputs ----

/// The process environment, else the workspace's `.env.dev`.
fn env_or_dotenv(key: &str) -> Option<String> {
    if let Ok(v) = std::env::var(key) {
        return Some(v);
    }
    let dotenv = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../.env.dev");
    let text = std::fs::read_to_string(dotenv).ok()?;
    text.lines().find_map(|line| {
        let (k, v) = line.trim().split_once('=')?;
        (k.trim() == key).then(|| v.trim().trim_matches('"').to_owned())
    })
}

/// One level of a texture as 8-bit codes, top row first.
#[derive(Debug, Clone)]
struct Texels {
    width: u32,
    height: u32,
    codes: Vec<[u8; 4]>,
}

/// Level 0 of a KTX2 the model stage wrote (AC.6's writer: one image, no
/// supercompression), decoded by `guac-formats`' own BC rules.
fn level0(ktx2: &[u8]) -> Texels {
    let u32_at = |o: usize| u32::from_le_bytes(ktx2[o..o + 4].try_into().unwrap());
    let u64_at = |o: usize| u64::from_le_bytes(ktx2[o..o + 8].try_into().unwrap());
    let format = match u32_at(12) {
        133 => Format::Bc1RgbaUnorm,
        137 => Format::Bc3Unorm,
        9 => Format::R8Unorm,
        f => panic!("vkFormat {f}"),
    };
    let (width, height) = (u32_at(20), u32_at(24));
    let (at, len) = (u64_at(80) as usize, u64_at(88) as usize);
    let t = Texture {
        format,
        width,
        height,
        levels: vec![ktx2[at..at + len].to_vec()],
    };
    let rgba = t.rgba8(0).expect("level 0 decodes");
    Texels {
        width,
        height,
        codes: rgba.as_chunks::<4>().0.to_vec(),
    }
}

struct Character {
    load_file: &'static str,
    model: Model,
    /// Each distinct texture's extract path and KTX2.
    ktx2: Vec<(String, Vec<u8>)>,
    texels: HashMap<String, Texels>,
}

fn read_character(root: &Path, load_file: &'static str) -> Character {
    let path = models::archive_path(&root.join("PS3_GAME/USRDIR"), load_file);
    let archive = models::archive(&path, load_file).expect("the archive");
    let level = models::level(&archive, load_file).expect("its level");
    let identity = DumpIdentity {
        title_id: String::new(),
        app_ver: String::new(),
        manifest_hash: Sha256([0; 32]),
    };
    let extracted = models::read(&level, load_file, identity).expect("the model");
    let texels = extracted
        .textures
        .iter()
        .map(|(p, k)| (p.clone(), level0(k)))
        .collect();
    Character {
        load_file,
        model: extracted.model,
        ktx2: extracted.textures,
        texels,
    }
}

/// The parts the main-pass check draws: opaque and not alpha-tested, so a
/// pixel holds exactly what one fragment shaded.
fn opaque(m: &model::Material) -> bool {
    m.states.blend == Blend::Opaque && m.states.alpha_test.is_none()
}

/// The segments the check draws ([`opaque`]); materials keep their indices.
fn opaque_only(model: &Model) -> Model {
    let mut m = model.clone();
    m.segments
        .retain(|s| opaque(&model.materials[usize::from(s.material)]));
    m
}

/// The tinted material's comparison: the same model with `tint` at 128.
fn neutralized(model: &Model, tint: [u8; 4]) -> Model {
    let mut m = model.clone();
    for mat in &mut m.materials {
        if mat.params.material_tint == tint {
            mat.params.material_tint = NEUTRAL;
        }
    }
    m
}

// ---- halves ----

fn f16_to_f32(h: u16) -> f32 {
    let sign = if h >> 15 == 1 { -1.0 } else { 1.0 };
    let exp = i32::from(h >> 10 & 0x1F);
    let frac = f32::from(h & 0x3FF);
    sign * match exp {
        0 => frac * 2f32.powi(-24),
        31 => {
            if frac == 0.0 {
                f32::INFINITY
            } else {
                f32::NAN
            }
        }
        e => (1.0 + frac / 1024.0) * 2f32.powi(e - 15),
    }
}

/// The half nearest `v`, ties to even, as its bits; `v` finite and under
/// 65520 in size.
fn f32_to_f16(v: f32) -> u16 {
    let x = v.to_bits();
    let sign = ((x >> 16) & 0x8000) as u16;
    let a = x & 0x7FFF_FFFF;
    if a < 0x3880_0000 {
        // Under 2^-14: a multiple of 2^-24.
        return sign | (f32::from_bits(a) * 16_777_216.0).round_ties_even() as u16;
    }
    assert!(a < 0x477F_F000, "{v} past half's range");
    // Rebiased from f32's 127 to half's 15; at least 1 past the branch above.
    let e = (a >> 23) - 112;
    let m = a & 0x7F_FFFF;
    let mut h = (e << 10) | (m >> 13);
    let rest = m & 0x1FFF;
    if rest > 0x1000 || (rest == 0x1000 && h & 1 == 1) {
        h += 1;
    }
    sign | h as u16
}

fn half(v: f32) -> f32 {
    f16_to_f32(f32_to_f16(v))
}

/// The spacing of the halves around `v`: 2^(e - 10) for a normal half of
/// exponent e, 2^-24 among the subnormals.
fn half_step(v: f32) -> f32 {
    let a = v.abs();
    if a < 2f32.powi(-14) {
        2f32.powi(-24)
    } else {
        2f32.powi(a.log2().floor() as i32 - 10)
    }
}

/// The half toward zero from `v`: how the M3 Max stores a half target.
fn truncate_half(v: f32) -> f32 {
    let step = half_step(v);
    (v / step).trunc() * step
}

/// Below this the bound is 2^-10's step (AE.8's lane: a value under a
/// thousandth is terms of order one cancelling, where the two evaluations'
/// f32 rounding outruns half's grid).
const FLOOR: f32 = 1.0 / 1024.0;

/// A probe pair's value: hi plus lo over 4096.
fn unsplit(hi: f32, lo: f32) -> f32 {
    hi + lo / 4096.0
}

// ---- decoding ----

/// sRGB's curve in f64.
fn srgb_decode(e: f64) -> f64 {
    if e <= 0.04045 {
        e / 12.92
    } else {
        ((e + 0.055) / 1.055).powf(2.4)
    }
}

fn srgb_encode(l: f64) -> f64 {
    if l <= 0.003_130_8 {
        l * 12.92
    } else {
        1.055 * l.powf(1.0 / 2.4) - 0.055
    }
}

/// What a texel's code decodes to exactly, as the remaster decodes it:
/// sRGB's curve on an sRGB view's colour channels, code / 255 on its alpha
/// and on every channel of a normal map.
fn exact(code: u8, srgb: bool, channel: usize) -> f32 {
    let v = f64::from(code) / 255.0;
    (if srgb && channel < 3 {
        srgb_decode(v)
    } else {
        v
    }) as f32
}

/// The lane's copy of a texture: each texel's exact decode stored as a
/// half, sampled nearest from level 0 with the IR's wrap.
fn lane_image(t: &Texels, srgb: bool, wrap: (Wrap, Wrap)) -> Image {
    let mut bytes = Vec::with_capacity(t.codes.len() * 8);
    for texel in &t.codes {
        for (ch, &code) in texel.iter().enumerate() {
            bytes.extend_from_slice(&f32_to_f16(exact(code, srgb, ch)).to_le_bytes());
        }
    }
    let mut image = Image::new(
        Extent3d {
            width: t.width,
            height: t.height,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        bytes,
        TextureFormat::Rgba16Float,
        RenderAssetUsages::RENDER_WORLD,
    );
    let mode = |w: Wrap| match w {
        Wrap::Clamp => ImageAddressMode::ClampToEdge,
        Wrap::Repeat => ImageAddressMode::Repeat,
    };
    image.sampler = ImageSampler::Descriptor(ImageSamplerDescriptor {
        address_mode_u: mode(wrap.0),
        address_mode_v: mode(wrap.1),
        mag_filter: ImageFilterMode::Nearest,
        min_filter: ImageFilterMode::Nearest,
        mipmap_filter: ImageFilterMode::Nearest,
        lod_max_clamp: 0.0,
        ..default()
    });
    image
}

/// The texel `uv` lands on, nearest, with the sampler's wrap.
fn texel_at(t: &Texels, uv: [f32; 2], wrap: (Wrap, Wrap)) -> usize {
    let axis = |c: f32, n: u32, w: Wrap| -> u32 {
        let i = (c * n as f32).floor() as i64;
        match w {
            Wrap::Repeat => i.rem_euclid(i64::from(n)) as u32,
            Wrap::Clamp => i.clamp(0, i64::from(n) - 1) as u32,
        }
    };
    let x = axis(uv[0], t.width, wrap.0);
    let y = axis(uv[1], t.height, wrap.1);
    (y * t.width + x) as usize
}

// ---- the probe material ----

/// What a probe pass specializes on: the colour pass's key and the probe.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct ProbeKey {
    material: MaterialKey,
    probe: u32,
}

/// The character material's bindings and uniform, drawn by the character
/// shader's vertex stage and the probe's fragment stage.
#[derive(Asset, TypePath, AsBindGroup, Debug, Clone)]
#[bind_group_data(ProbeKey)]
struct ProbeMaterial {
    #[uniform(0)]
    uniform: CharacterUniform,
    #[texture(1, visibility(fragment))]
    #[sampler(2, visibility(fragment))]
    colour: Option<Handle<Image>>,
    #[texture(3, visibility(fragment))]
    #[sampler(4, visibility(fragment))]
    normal_map: Option<Handle<Image>>,
    #[texture(5, visibility(fragment))]
    #[sampler(6, visibility(fragment))]
    spec_map: Option<Handle<Image>>,
    #[texture(7, visibility(fragment))]
    #[sampler(8, visibility(fragment))]
    second_colour: Option<Handle<Image>>,
    #[texture(9, visibility(fragment))]
    #[sampler(10, visibility(fragment))]
    second_normal_map: Option<Handle<Image>>,
    #[texture(11, visibility(fragment))]
    #[sampler(12, visibility(fragment))]
    second_spec_map: Option<Handle<Image>>,
    #[texture(13, visibility(fragment))]
    #[sampler(14, visibility(fragment))]
    third: Option<Handle<Image>>,
    #[texture(15, visibility(fragment))]
    #[sampler(16, visibility(fragment))]
    rim: Option<Handle<Image>>,
    #[texture(17, visibility(fragment))]
    #[sampler(18, visibility(fragment))]
    shadow: Option<Handle<Image>>,
    key: ProbeKey,
}

impl From<&ProbeMaterial> for ProbeKey {
    fn from(m: &ProbeMaterial) -> Self {
        m.key
    }
}

impl ProbeMaterial {
    /// `m`'s probe `probe`, its uniform's spare word the material's index
    /// plus one.
    fn new(m: &CharacterMaterial, index: usize, probe: u32) -> Self {
        let mut uniform = m.uniform;
        uniform.misc.w = (index + 1) as f32;
        Self {
            uniform,
            colour: m.colour.clone(),
            normal_map: m.normal_map.clone(),
            spec_map: m.spec_map.clone(),
            second_colour: m.second_colour.clone(),
            second_normal_map: m.second_normal_map.clone(),
            second_spec_map: m.second_spec_map.clone(),
            third: m.third.clone(),
            rim: m.rim.clone(),
            shadow: m.shadow.clone(),
            key: ProbeKey {
                material: m.key,
                probe,
            },
        }
    }
}

impl Material for ProbeMaterial {
    fn vertex_shader() -> ShaderRef {
        SHADER.into()
    }

    fn fragment_shader() -> ShaderRef {
        PROBE_SHADER.into()
    }

    fn alpha_mode(&self) -> AlphaMode {
        AlphaMode::Opaque
    }

    fn enable_prepass() -> bool {
        false
    }

    fn enable_shadows() -> bool {
        false
    }

    fn specialize(
        _pipeline: &MaterialPipeline,
        descriptor: &mut RenderPipelineDescriptor,
        layout: &MeshVertexBufferLayoutRef,
        key: MaterialPipelineKey<Self>,
    ) -> Result<(), SpecializedMeshPipelineError> {
        specialize_for(&key.bind_group_data.material, descriptor, layout)?;
        if let Some(fragment) = descriptor.fragment.as_mut() {
            fragment.shader_defs.push(ShaderDefVal::UInt(
                "PROBE".into(),
                key.bind_group_data.probe,
            ));
        }
        Ok(())
    }
}

// ---- the app ----

#[derive(Debug, Clone, Copy, PartialEq)]
enum Kind {
    /// The main pass, half floats, tonemapping off.
    Check,
    Probe(u32),
    /// The remaster as it ships, or with the glow off, or with the tinted
    /// material neutral.
    Look {
        glow: bool,
        neutral: bool,
    },
}

struct Target {
    character: usize,
    kind: Kind,
    image: Handle<Image>,
}

impl Target {
    fn half(&self) -> bool {
        !matches!(self.kind, Kind::Look { .. })
    }
}

#[derive(Resource)]
struct Plan {
    characters: Arc<Vec<Character>>,
    frame: FrameCamera,
    at: f64,
    scene: Scene,
    /// Which characters get their tinted material neutralized too.
    tinted: Vec<Option<[u8; 4]>>,
}

#[derive(Resource, Default)]
struct Targets(Vec<Target>);

/// Each target's character and kind, in order, for after the app has run
/// (`App::run` hands its world to the runner).
#[derive(Resource, Clone, Default)]
struct Listed(Arc<Mutex<Vec<(usize, Kind)>>>);

#[derive(Debug, Clone, PartialEq)]
enum Shot {
    Half(Vec<[f32; 4]>),
    Byte(Vec<[u8; 4]>),
}

impl Shot {
    fn half(&self) -> &[[f32; 4]] {
        match self {
            Shot::Half(p) => p,
            Shot::Byte(_) => panic!("an 8-bit target"),
        }
    }

    fn byte(&self) -> &[[u8; 4]] {
        match self {
            Shot::Byte(p) => p,
            Shot::Half(_) => panic!("a half target"),
        }
    }
}

#[derive(Resource, Clone, Default)]
struct Captured(Arc<Mutex<Vec<Option<Shot>>>>);

#[derive(Resource, Clone, Default)]
struct Outcome(Arc<Mutex<Option<Result<Vec<Shot>, String>>>>);

fn half_target(images: &mut Assets<Image>) -> Handle<Image> {
    let mut image = Image::new_target_texture(SIZE.x, SIZE.y, TextureFormat::Rgba16Float, None);
    image.asset_usage = RenderAssetUsages::default();
    images.add(image)
}

fn setup(
    mut commands: Commands,
    plan: Res<Plan>,
    device: Res<RenderDevice>,
    listed: Res<Listed>,
    mut targets: ResMut<Targets>,
    mut images: ResMut<Assets<Image>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut bindposes: ResMut<Assets<SkinnedMeshInverseBindposes>>,
    mut characters: ResMut<Assets<CharacterMaterial>>,
    mut probes: ResMut<Assets<ProbeMaterial>>,
) {
    let formats = CompressedImageFormats::from_features(device.features());
    let frame = &plan.frame;
    let projection = Projection::custom(frame.projection.clone());
    let root = Transform::from_scale(Vec3::splat(frame.scale));
    let mut layer = 0usize;
    let mut next_layer = || {
        layer += 1;
        layer
    };
    let half_camera = |commands: &mut Commands, image: &Handle<Image>, clear: [f32; 4], layer| {
        commands.spawn((
            Camera3d::default(),
            Camera {
                clear_color: ClearColorConfig::Custom(Color::LinearRgba(LinearRgba::new(
                    clear[0], clear[1], clear[2], clear[3],
                ))),
                ..default()
            },
            RenderTarget::Image(image.clone().into()),
            projection.clone(),
            frame.transform,
            Hdr,
            Tonemapping::None,
            DebandDither::Disabled,
            Msaa::Off,
            RenderLayers::layer(layer),
        ));
    };

    for (c, character) in plan.characters.iter().enumerate() {
        // The main pass and its probes: the lane's half-float textures.
        // By path, view and wrap (the IR's sampler types don't hash).
        let mut lane: HashMap<String, Handle<Image>> = HashMap::new();
        let check_materials: Vec<CharacterMaterial> = character
            .model
            .materials
            .iter()
            .map(|m| {
                CharacterMaterial::from_ir(m, &plan.scene, ShadowLookup::NONE, |slot| {
                    let srgb = remaster::is_srgb(slot.role);
                    let wrap = (slot.sampler.wrap_u, slot.sampler.wrap_v);
                    lane.entry(format!("{} {srgb} {wrap:?}", slot.texture))
                        .or_insert_with(|| {
                            images.add(lane_image(&character.texels[&slot.texture], srgb, wrap))
                        })
                        .clone()
                })
                .unwrap_or_else(|e| panic!("{}: {e}", m.name))
            })
            .collect();
        let drawn = opaque_only(&character.model);
        let handles: Vec<Handle<CharacterMaterial>> = check_materials
            .iter()
            .map(|m| characters.add(m.clone()))
            .collect();
        let l = next_layer();
        let spawned = spawn_character(
            &mut commands,
            &drawn,
            root,
            plan.at,
            &mut meshes,
            &mut bindposes,
            &handles,
        )
        .expect("the main pass spawns");
        for e in &spawned.segments {
            commands.entity(*e).insert(RenderLayers::layer(l));
        }
        let image = half_target(&mut images);
        half_camera(&mut commands, &image, CLEAR, l);
        targets.0.push(Target {
            character: c,
            kind: Kind::Check,
            image,
        });

        for probe in 0..PROBES {
            let handles: Vec<Handle<ProbeMaterial>> = check_materials
                .iter()
                .enumerate()
                .map(|(i, m)| probes.add(ProbeMaterial::new(m, i, probe)))
                .collect();
            let l = next_layer();
            let spawned = spawn_character(
                &mut commands,
                &drawn,
                root,
                plan.at,
                &mut meshes,
                &mut bindposes,
                &handles,
            )
            .expect("a probe pass spawns");
            for e in &spawned.segments {
                commands.entity(*e).insert(RenderLayers::layer(l));
            }
            let image = half_target(&mut images);
            half_camera(&mut commands, &image, [0.0; 4], l);
            targets.0.push(Target {
                character: c,
                kind: Kind::Probe(probe),
                image,
            });
        }

        // The look: the disc's textures as the remaster loads them.
        let mut real: HashMap<String, Handle<Image>> = HashMap::new();
        let neutral_too = plan.tinted[c];
        for neutral in [false, true] {
            let model = match (neutral, neutral_too) {
                (false, _) => character.model.clone(),
                (true, Some(tint)) => neutralized(&character.model, tint),
                (true, None) => continue,
            };
            let (handles, glows): (Vec<_>, Vec<_>) = model
                .materials
                .iter()
                .map(|m| {
                    let material =
                        CharacterMaterial::from_ir(m, &plan.scene, ShadowLookup::NONE, |slot| {
                            let srgb = remaster::is_srgb(slot.role);
                            real.entry(format!("{} {srgb} {:?}", slot.texture, slot.sampler))
                                .or_insert_with(|| {
                                    let (_, ktx2) = character
                                        .ktx2
                                        .iter()
                                        .find(|(p, _)| *p == slot.texture)
                                        .expect("the model's texture");
                                    images.add(
                                        remaster::texture(ktx2, slot.role, &slot.sampler, formats)
                                            .expect("a KTX2"),
                                    )
                                })
                                .clone()
                        })
                        .unwrap_or_else(|e| panic!("{}: {e}", m.name));
                    let glow = characters.add(material.glow_mask());
                    (characters.add(material), glow)
                })
                .unzip();
            for glow in [true, false] {
                let l = next_layer();
                let spawned = spawn_character(
                    &mut commands,
                    &model,
                    root,
                    plan.at,
                    &mut meshes,
                    &mut bindposes,
                    &handles,
                )
                .expect("the look spawns");
                for e in &spawned.segments {
                    commands.entity(*e).insert(RenderLayers::layer(l));
                }
                let image = images.add(remaster::target(SIZE));
                let camera = commands
                    .spawn((
                        remaster::camera(),
                        RenderTarget::Image(image.clone().into()),
                        projection.clone(),
                        frame.transform,
                        RenderLayers::layer(l),
                    ))
                    .id();
                if glow {
                    let g = next_layer();
                    spawn_glow_masks(&mut commands, &model, &spawned, &glows, g);
                    spawn_glow_camera(
                        &mut commands,
                        &mut images,
                        camera,
                        SIZE,
                        projection.clone(),
                        remaster::MSAA,
                        g,
                    );
                }
                targets.0.push(Target {
                    character: c,
                    kind: Kind::Look { glow, neutral },
                    image,
                });
            }
        }
    }
    *listed.0.lock().unwrap() = targets.0.iter().map(|t| (t.character, t.kind)).collect();
}

/// Frames between captures, and the longest to wait for every pipeline.
const EVERY: u32 = 20;
const MAX_WAIT: Duration = Duration::from_secs(240);

/// Why the shots aren't final yet, if they aren't: a pass that hasn't
/// drawn, probes that don't cover what the main pass does, a glow chain
/// that hasn't run.
fn waiting(targets: &[Target], shots: &[Shot]) -> Option<String> {
    let find = |c: usize, kind: Kind| {
        targets
            .iter()
            .position(|t| t.character == c && t.kind == kind)
    };
    let characters = targets.iter().map(|t| t.character).max().unwrap_or(0) + 1;
    for c in 0..characters {
        let check = shots[find(c, Kind::Check).unwrap()].half();
        let covered = check.iter().filter(|p| **p != CLEAR).count();
        if covered == 0 {
            return Some(format!("character {c}'s main pass hasn't drawn"));
        }
        let ids = shots[find(c, Kind::Probe(1)).unwrap()].half();
        let probed = ids.iter().filter(|p| p[2] != 0.0).count();
        if probed != covered {
            return Some(format!(
                "character {c}: the probes cover {probed} pixels, the main pass {covered}"
            ));
        }
        for probe in 0..PROBES {
            let shot = shots[find(c, Kind::Probe(probe)).unwrap()].half();
            if shot.iter().all(|p| *p == [0.0; 4]) {
                return Some(format!("character {c}'s probe {probe} hasn't drawn"));
            }
        }
        for neutral in [false, true] {
            let (Some(g), Some(n)) = (
                find(
                    c,
                    Kind::Look {
                        glow: true,
                        neutral,
                    },
                ),
                find(
                    c,
                    Kind::Look {
                        glow: false,
                        neutral,
                    },
                ),
            ) else {
                continue;
            };
            let background = shots[n].byte()[0];
            if shots[n].byte().iter().all(|p| *p == background) {
                return Some(format!("character {c}'s look hasn't drawn"));
            }
            if shots[g] == shots[n] {
                return Some(format!("character {c}'s glow chain hasn't run"));
            }
        }
    }
    None
}

fn capture(
    mut commands: Commands,
    targets: Res<Targets>,
    captured: Res<Captured>,
    outcome: Res<Outcome>,
    mut frame: Local<u32>,
    mut started: Local<Option<Instant>>,
    mut previous: Local<Option<Vec<Shot>>>,
    mut exit: MessageWriter<AppExit>,
) {
    *frame += 1;
    let started = *started.get_or_insert_with(Instant::now);
    let done = {
        let c = captured.0.lock().unwrap();
        !c.is_empty() && c.iter().all(Option::is_some)
    };
    if done {
        let shots: Vec<Shot> = std::mem::take(&mut *captured.0.lock().unwrap())
            .into_iter()
            .map(Option::unwrap)
            .collect();
        let why = waiting(&targets.0, &shots);
        let stable = previous.as_ref() == Some(&shots);
        if why.is_none() && stable {
            println!(
                "every pass drew and two captures agree: frame {}, {:.1} s",
                *frame,
                started.elapsed().as_secs_f64()
            );
            *outcome.0.lock().unwrap() = Some(Ok(shots));
            exit.write(AppExit::Success);
            return;
        }
        if let Some(why) = why
            && (*frame).is_multiple_of(10 * EVERY)
        {
            eprintln!("frame {}: {why}", *frame);
        }
        *previous = Some(shots);
    }
    if started.elapsed() > MAX_WAIT {
        *outcome.0.lock().unwrap() = Some(Err(format!(
            "not ready after {MAX_WAIT:?} ({} frames)",
            *frame
        )));
        exit.write(AppExit::error());
        return;
    }
    if (*frame).is_multiple_of(EVERY) && captured.0.lock().unwrap().is_empty() {
        *captured.0.lock().unwrap() = vec![None; targets.0.len()];
        for (i, t) in targets.0.iter().enumerate() {
            let store = captured.0.clone();
            let half = t.half();
            commands.spawn(Screenshot::image(t.image.clone())).observe(
                move |c: On<ScreenshotCaptured>| {
                    let data = c.image.data.as_ref().expect("pixels");
                    let shot = if half {
                        Shot::Half(
                            data.as_chunks::<8>()
                                .0
                                .iter()
                                .map(|p| {
                                    std::array::from_fn(|ch| {
                                        f16_to_f32(u16::from_le_bytes([p[2 * ch], p[2 * ch + 1]]))
                                    })
                                })
                                .collect(),
                        )
                    } else {
                        Shot::Byte(data.as_chunks::<4>().0.to_vec())
                    };
                    store.lock().unwrap()[i] = Some(shot);
                },
            );
        }
    }
}

fn render(plan: Plan) -> (Vec<(usize, Kind)>, Vec<Shot>) {
    let outcome = Outcome::default();
    let mut app = App::new();
    app.add_plugins((
        DefaultPlugins
            .set(LogPlugin {
                filter: "wgpu=error,naga=warn,bevy_render=warn,bevy_shader=info".into(),
                ..default()
            })
            .set(WindowPlugin {
                primary_window: None,
                exit_condition: ExitCondition::DontExit,
                ..default()
            })
            .disable::<WinitPlugin>(),
        ScheduleRunnerPlugin::run_loop(Duration::from_millis(1)),
        CharacterShadingPlugin,
        GlowPlugin,
    ));
    // Before the material's plugin, which asks for its shaders as it builds.
    app.world_mut()
        .resource_mut::<EmbeddedAssetRegistry>()
        .insert_asset(
            PathBuf::new(),
            Path::new("guacamole_lane/probe.wesl"),
            include_bytes!("remaster_gpu_lane/probe.wesl").as_slice(),
        );
    app.add_plugins(MaterialPlugin::<ProbeMaterial>::default());
    let listed = Listed::default();
    app.insert_resource(plan)
        .insert_resource(listed.clone())
        .insert_resource(ClearColor(BACKGROUND))
        .insert_resource(Targets::default())
        .insert_resource(Captured::default())
        .insert_resource(outcome.clone())
        .add_systems(Startup, setup)
        .add_systems(Update, capture);
    app.run();
    let targets = std::mem::take(&mut *listed.0.lock().unwrap());
    let shots = outcome
        .0
        .lock()
        .unwrap()
        .take()
        .expect("an outcome")
        .expect("every pass drew");
    (targets, shots)
}

// ---- the check ----

/// What the probes read at one pixel.
#[derive(Debug, Clone, Copy)]
struct Probed {
    /// The material's index in the model.
    material: usize,
    fragment: Fragment,
    colour: [f32; 4],
    normal_map: [f32; 4],
    spec_map: [f32; 4],
    /// Where the GPU looked the rim up.
    rim_uv: [f32; 2],
}

fn probed(probes: &[&[[f32; 4]]], i: usize) -> Option<Probed> {
    let pair = |k: usize| {
        let p = probes[k][i];
        (unsplit(p[0], p[1]), unsplit(p[2], p[3]))
    };
    let (_, id) = pair(1);
    let id = id.round();
    if id < 1.0 {
        return None;
    }
    let (px, py) = pair(0);
    let (pz, _) = pair(1);
    let (nx, ny) = pair(2);
    let (nz, ex) = pair(3);
    let (ey, ez) = pair(4);
    let (tx, ty) = pair(5);
    let (tz, bx) = pair(6);
    let (by, bz) = pair(7);
    let (u, v) = pair(8);
    let (ru, rv) = pair(9);
    Some(Probed {
        material: id as usize - 1,
        fragment: Fragment {
            normal: [nx, ny, nz],
            tangent: [tx, ty, tz],
            bitangent: [bx, by, bz],
            position: [px, py, pz],
            depth: -pz,
            eye: [ex, ey, ez],
            vertex_colour: shading::HALF_GREY,
            uv: [[u, v], [0.0; 2], [0.0; 2]],
            shadow: [0.0; 4],
        },
        colour: probes[10][i],
        normal_map: probes[11][i],
        spec_map: probes[12][i],
        rim_uv: [ru, rv],
    })
}

/// A material's rim map and its wrap, for the model's own lookup.
fn rim_of<'a>(character: &'a Character, m: &model::Material) -> Option<(&'a Texels, (Wrap, Wrap))> {
    m.texture(TextureRole::Layer4ColorMap).map(|slot| {
        (
            &character.texels[&slot.texture],
            (slot.sampler.wrap_u, slot.sampler.wrap_v),
        )
    })
}

/// The model's colour and alpha at one probed pixel: the colour, normal and
/// spec maps as the GPU sampled them, the rim looked up by the model itself
/// on the same texels. Also whether the model's rim lookup lands on another
/// texel than the GPU's.
fn model_at(
    character: &Character,
    materials: &[shading::Material],
    scene: &Scene,
    p: &Probed,
    texel: impl Fn(&Texels, usize) -> [f32; 4],
) -> ([f32; 4], bool) {
    let ir = &character.model.materials[p.material];
    let rim = rim_of(character, ir);
    let flipped = std::cell::Cell::new(false);
    let rim_sample = |uv: [f32; 2]| {
        let (t, wrap) = rim.expect("a rim map");
        let at = texel_at(t, uv, wrap);
        if at != texel_at(t, p.rim_uv, wrap) {
            flipped.set(true);
        }
        texel(t, at)
    };
    let (colour, normal_map, spec_map) = (Flat(p.colour), Flat(p.normal_map), Flat(p.spec_map));
    let textures = Textures {
        colour: Some(&colour),
        normal_map: Some(&normal_map),
        spec_map: Some(&spec_map),
        rim: Some(&rim_sample),
        ..Textures::default()
    };
    let s = shading::shade(&materials[p.material], scene, &p.fragment, &textures)
        .expect("the model shades");
    (
        [s.colour[0], s.colour[1], s.colour[2], s.alpha],
        flipped.get(),
    )
}

#[derive(Debug, Default)]
struct Checked {
    pixels: usize,
    channels: usize,
    /// Pixels where the model's rim lookup lands on another texel than the
    /// GPU's, left out of the bound.
    rim_flips: usize,
    worst: f32,
    worst_at: Option<(usize, usize, f32, f32)>,
    misses: usize,
    /// From the model's own value, unrounded: the worst and past one step.
    raw: f32,
    raw_past_one: usize,
    floored: usize,
    /// Hero pixels with red, green or both past 1 before tonemapping.
    red_past: usize,
    green_past: usize,
    both_past: usize,
    max: [f32; 3],
}

fn check(
    character: &Character,
    scene: &Scene,
    main: &[[f32; 4]],
    probes: &[&[[f32; 4]]],
) -> (Checked, Vec<Option<Probed>>) {
    let materials: Vec<shading::Material> = character
        .model
        .materials
        .iter()
        .map(|m| reference_material(m).expect("a model material"))
        .collect();
    let lane = |t: &Texels, at: usize, srgb: bool| -> [f32; 4] {
        std::array::from_fn(|ch| half(exact(t.codes[at][ch], srgb, ch)))
    };
    let mut out = Checked::default();
    let mut all = Vec::with_capacity(main.len());
    for (i, got) in main.iter().enumerate() {
        let p = probed(probes, i);
        all.push(p);
        let Some(p) = p else {
            assert_eq!(*got, CLEAR, "pixel {i}: drawn but not probed");
            continue;
        };
        assert_ne!(*got, CLEAR, "pixel {i}: probed but not drawn");
        out.pixels += 1;
        let (want, flipped) = model_at(character, &materials, scene, &p, |t, at| lane(t, at, true));
        if flipped {
            out.rim_flips += 1;
            continue;
        }
        out.red_past += usize::from(got[0] > 1.0);
        out.green_past += usize::from(got[1] > 1.0);
        out.both_past += usize::from(got[0] > 1.0 && got[1] > 1.0);
        for (max, v) in out.max.iter_mut().zip(got) {
            *max = max.max(*v);
        }
        for ch in 0..4 {
            out.channels += 1;
            let raw = (got[ch] - want[ch]).abs() / half_step(want[ch]);
            out.raw = out.raw.max(raw);
            out.raw_past_one += usize::from(raw > 1.0);
            let step = half_step(want[ch].abs().max(FLOOR));
            let steps = (got[ch] - truncate_half(want[ch])).abs() / step;
            let steps = if steps.is_nan() { f32::INFINITY } else { steps };
            if want[ch].abs() < FLOOR
                && (got[ch] - truncate_half(want[ch])).abs() > half_step(want[ch])
            {
                out.floored += 1;
            }
            if steps > 1.0 {
                out.misses += 1;
            }
            if steps > out.worst {
                out.worst = steps;
                out.worst_at = Some((i, ch, want[ch], got[ch]));
            }
        }
    }
    (out, all)
}

// ---- the look ----

fn oklch(lin: [f64; 3]) -> (f64, f64, f64) {
    let m1 = [
        [0.412_221_470_8, 0.536_332_536_3, 0.051_445_992_9],
        [0.211_903_498_2, 0.680_699_545_1, 0.107_396_956_6],
        [0.088_302_461_9, 0.281_718_837_6, 0.629_978_700_5],
    ];
    let m2 = [
        [0.210_454_255_3, 0.793_617_785_0, -0.004_072_046_8],
        [1.977_998_495_1, -2.428_592_205_0, 0.450_593_709_9],
        [0.025_904_037_1, 0.782_771_766_2, -0.808_675_766_0],
    ];
    let lms = m1.map(|r| (r[0] * lin[0] + r[1] * lin[1] + r[2] * lin[2]).cbrt());
    let [l, a, b] = m2.map(|r| r[0] * lms[0] + r[1] * lms[1] + r[2] * lms[2]);
    (l, a.hypot(b), b.atan2(a).to_degrees().rem_euclid(360.0))
}

#[derive(Debug, Clone, Copy, Default)]
struct Stats {
    pixels: usize,
    mean_c: f64,
    mean_l: f64,
    /// Chroma-weighted circular mean.
    hue: f64,
    any_255: f64,
    rg_255: f64,
}

/// Oklch over the masked pixels of colours given in linear light, with
/// their stored 8-bit codes for the clip shares.
fn stats(pixels: impl Iterator<Item = ([f64; 3], [u8; 3])>) -> Stats {
    let (mut n, mut c, mut l, mut s, mut co, mut any, mut rg) = (0, 0.0, 0.0, 0.0, 0.0, 0, 0);
    for (lin, bytes) in pixels {
        let (ll, cc, h) = oklch(lin);
        n += 1;
        c += cc;
        l += ll;
        s += cc * h.to_radians().sin();
        co += cc * h.to_radians().cos();
        any += usize::from(bytes.contains(&255));
        rg += usize::from(bytes[0] == 255 && bytes[1] == 255);
    }
    let nf = n.max(1) as f64;
    Stats {
        pixels: n,
        mean_c: c / nf,
        mean_l: l / nf,
        hue: s.atan2(co).to_degrees().rem_euclid(360.0),
        any_255: any as f64 / nf,
        rg_255: rg as f64 / nf,
    }
}

/// An 8-bit sRGB render's stats over a mask.
fn look_stats(shot: &[[u8; 4]], mask: &[bool]) -> Stats {
    stats(shot.iter().zip(mask).filter(|(_, m)| **m).map(|(p, _)| {
        (
            [0, 1, 2].map(|ch| srgb_decode(f64::from(p[ch]) / 255.0)),
            [p[0], p[1], p[2]],
        )
    }))
}

/// Pixels whose whole (2r+1)² neighbourhood is in `m`.
fn erode(m: &[bool], r: i64) -> Vec<bool> {
    let (w, h) = (i64::from(SIZE.x), i64::from(SIZE.y));
    (0..m.len())
        .map(|i| {
            let (x, y) = (i as i64 % w, i as i64 / w);
            (-r..=r).all(|dy| {
                (-r..=r).all(|dx| {
                    let (xx, yy) = (x + dx, y + dy);
                    (0..w).contains(&xx) && (0..h).contains(&yy) && m[(yy * w + xx) as usize]
                })
            })
        })
        .collect()
}

fn and(a: &[bool], b: &[bool]) -> Vec<bool> {
    a.iter().zip(b).map(|(x, y)| *x && *y).collect()
}

fn show(name: &str, s: &Stats) -> String {
    format!(
        "{name}: {} px, C {:.4}, L {:.3}, hue {:.1} deg, any 255 {:.1}%, R and G 255 {:.2}%",
        s.pixels,
        s.mean_c,
        s.mean_l,
        s.hue,
        100.0 * s.any_255,
        100.0 * s.rg_255
    )
}

// ---- the lane ----

#[test]
#[ignore = "GPU lane: Eruptor and Sunburn off GUAC_GIANTS_DUMP at AC.13's frame 4 from GUAC_CAPTURE_DIR, on the M3 Max (AE.12)"]
fn the_remaster_at_an_oracle_frame() {
    let started = Instant::now();
    let dump = env_or_dotenv("GUAC_GIANTS_DUMP").expect("set GUAC_GIANTS_DUMP to the Giants dump");
    let captures = env_or_dotenv("GUAC_CAPTURE_DIR")
        .expect("set GUAC_CAPTURE_DIR to the folder holding AC.13's session");
    let captures = Path::new(&captures);
    let rig = Rig::read(&captures.join(FRAME)).expect("frame 4's rig");
    let text = std::fs::read_to_string(captures.join(VIEW_PROJ)).expect("frame 4's camera");
    let camera =
        FrameCamera::from_view_proj(&view_proj(&mut std::iter::once(text)).expect("16 numbers"))
            .expect("a camera");
    let at = rig.idle_time.expect("frame 4's idle time");
    let codes: FrameFile =
        serde_json::from_str(&std::fs::read_to_string(captures.join(FRAME)).unwrap()).unwrap();
    println!(
        "frame 4: idle {at} s, model scale {:.4}, vertical FOV {:.4} deg",
        camera.scale, camera.fov
    );

    let root = Path::new(&dump);
    let characters = vec![read_character(root, ERUPTOR), read_character(root, SUNBURN)];
    let tinted: Vec<Option<[u8; 4]>> = characters
        .iter()
        .map(|c| {
            c.model
                .materials
                .iter()
                .find(|m| m.name == TINTED)
                .map(|m| m.params.material_tint)
        })
        .collect();
    assert_eq!(tinted[1], Some([201, 132, 80, 255]), "Sunburn's body tint");
    for c in &characters {
        let keys: Vec<String> = c
            .model
            .materials
            .iter()
            .map(|m| {
                format!(
                    "{} {:#x}/{:#x}{}",
                    m.name,
                    m.pixel.base,
                    m.vertex.base,
                    if opaque(m) { "" } else { " (not checked)" }
                )
            })
            .collect();
        println!("{}: {}", c.load_file, keys.join(", "));
    }
    let scene = rig.scene();
    let characters = Arc::new(characters);
    let plan = Plan {
        characters: characters.clone(),
        frame: camera,
        at,
        scene: scene.clone(),
        tinted: vec![None, tinted[1]],
    };
    let rendered = Instant::now();
    let (targets, shots) = render(plan);
    println!(
        "{} targets rendered in {:.1} s",
        targets.len(),
        rendered.elapsed().as_secs_f64()
    );
    let shot = |c: usize, kind: Kind| -> &Shot {
        let i = targets
            .iter()
            .position(|t| *t == (c, kind))
            .unwrap_or_else(|| panic!("no {kind:?} for {c}"));
        &shots[i]
    };

    let mut failures = Vec::new();
    let mut probed_all = Vec::new();
    for (c, character) in characters.iter().enumerate() {
        let probes: Vec<&[[f32; 4]]> = (0..PROBES)
            .map(|k| shot(c, Kind::Probe(k)).half())
            .collect();
        let (checked, probed) = check(character, &scene, shot(c, Kind::Check).half(), &probes);
        let channels = checked.channels;
        println!(
            "{} main pass: {} pixels, {channels} channels, worst {:.3} half steps at {:?}, {} past one, {} on the floor; from the model's own value worst {:.2}, {} past one; {} rim lookups on another texel (left out)",
            character.load_file,
            checked.pixels,
            checked.worst,
            checked.worst_at,
            checked.misses,
            checked.floored,
            checked.raw,
            checked.raw_past_one,
            checked.rim_flips
        );
        println!(
            "{} before tonemapping: max ({:.3}, {:.3}, {:.3}), red past 1 on {:.1}%, green {:.1}%, both {:.1}%",
            character.load_file,
            checked.max[0],
            checked.max[1],
            checked.max[2],
            100.0 * checked.red_past as f64 / checked.pixels as f64,
            100.0 * checked.green_past as f64 / checked.pixels as f64,
            100.0 * checked.both_past as f64 / checked.pixels as f64
        );
        if checked.misses > 0 {
            failures.push(format!(
                "{}: {} channels past one half step",
                character.load_file, checked.misses
            ));
        }
        probed_all.push(probed);
    }

    // The look, on Eruptor.
    let covered: Vec<bool> = probed_all[0].iter().map(Option::is_some).collect();
    let luma: Vec<f64> = probed_all[0]
        .iter()
        .map(|p| {
            p.map_or(0.0, |p| {
                let g = [0, 1, 2].map(|ch| srgb_encode(f64::from(p.colour[ch])));
                0.299 * g[0] + 0.587 * g[1] + 0.114 * g[2]
            })
        })
        .collect();
    let hero = erode(&covered, 2);
    let below: Vec<bool> = covered
        .iter()
        .zip(&luma)
        .map(|(c, l)| *c && *l < 0.20)
        .collect();
    let above: Vec<bool> = covered
        .iter()
        .zip(&luma)
        .map(|(c, l)| *c && *l > 0.45)
        .collect();
    let rock = and(&hero, &erode(&below, 1));
    let lava = and(&hero, &erode(&above, 1));
    println!(
        "Eruptor's masks: covered {}, hero (eroded 2 px) {}, rock {}, lava {}",
        covered.iter().filter(|m| **m).count(),
        hero.iter().filter(|m| **m).count(),
        rock.iter().filter(|m| **m).count(),
        lava.iter().filter(|m| **m).count()
    );
    for glow in [false, true] {
        let s = shot(
            0,
            Kind::Look {
                glow,
                neutral: false,
            },
        )
        .byte();
        let label = if glow {
            "with the glow"
        } else {
            "main pass alone"
        };
        let (h, r, l) = (
            look_stats(s, &hero),
            look_stats(s, &rock),
            look_stats(s, &lava),
        );
        println!("Eruptor tonemapped, {label}:");
        println!("  {}", show("hero", &h));
        println!("  {}", show("rock", &r));
        println!("  {}", show("lava", &l));
        println!(
            "  against (d)'s C {}, L {}, R and G 255 {}: C {:+.4}, L {:+.3}",
            D_ROW.0,
            D_ROW.1,
            D_ROW.2,
            h.mean_c - D_ROW.0,
            h.mean_l - D_ROW.1
        );
        if l.hue >= LAVA_HUE {
            failures.push(format!("{label}: lava hue {:.1} deg", l.hue));
        }
        if h.rg_255 >= LAVA_CLIP {
            failures.push(format!(
                "{label}: {:.2}% at full red and green",
                100.0 * h.rg_255
            ));
        }
    }

    // Sunburn's body tint.
    let sunburn = &characters[1];
    let body = sunburn
        .model
        .materials
        .iter()
        .position(|m| m.name == TINTED)
        .unwrap();
    let region: Vec<bool> = erode(
        &probed_all[1]
            .iter()
            .map(|p| p.is_some_and(|p| p.material == body))
            .collect::<Vec<_>>(),
        1,
    );
    let tint = tinted[1].unwrap();
    let neutral_model = neutralized(&sunburn.model, tint);
    let reference = |m: &Model| -> Vec<shading::Material> {
        m.materials
            .iter()
            .map(|x| reference_material(x).unwrap())
            .collect()
    };
    let (tinted_m, neutral_m) = (reference(&sunburn.model), reference(&neutral_model));
    // The game's arithmetic: the same inputs as stored codes over 255, the
    // rig's colours too, clamped per channel at the 8-bit store.
    let mut gamma_scene = scene.clone();
    for (l, f) in gamma_scene.lights.iter_mut().zip(&codes.lights) {
        l.ambient = f.ambient.map(|c| f32::from(c) / 255.0);
        l.diffuse = f.diffuse.map(|c| f32::from(c) / 255.0);
        l.specular = f.specular.map(|c| f32::from(c) / 255.0);
    }
    if let Some(f) = &codes.fog {
        gamma_scene.fog_colour = f.colour.map(|c| f32::from(c) / 255.0);
    }
    // A sampled colour texel back to its code: every code's half is its own.
    let mut srgb_codes = HashMap::new();
    let mut unorm_codes = HashMap::new();
    for code in 0..=255u8 {
        assert!(
            srgb_codes
                .insert(f32_to_f16(exact(code, true, 0)), code)
                .is_none()
        );
        assert!(
            unorm_codes
                .insert(f32_to_f16(exact(code, false, 0)), code)
                .is_none()
        );
    }
    let to_codes = |s: [f32; 4], srgb: bool| -> [f32; 4] {
        std::array::from_fn(|ch| {
            let table = if srgb && ch < 3 {
                &srgb_codes
            } else {
                &unorm_codes
            };
            f32::from(table[&f32_to_f16(s[ch])]) / 255.0
        })
    };
    let (mut lin_t, mut lin_n) = ([0f64; 3], [0f64; 3]);
    let (mut game_t, mut game_n) = (Vec::new(), Vec::new());
    let (mut remaster_t, mut remaster_n) = (Vec::new(), Vec::new());
    let mut n = 0usize;
    for (i, p) in probed_all[1].iter().enumerate() {
        let (Some(p), true) = (p, region[i]) else {
            continue;
        };
        n += 1;
        let lin = |t: &Texels, at: usize| -> [f32; 4] {
            std::array::from_fn(|ch| half(exact(t.codes[at][ch], true, ch)))
        };
        let (t, _) = model_at(sunburn, &tinted_m, &scene, p, lin);
        let (u, _) = model_at(sunburn, &neutral_m, &scene, p, lin);
        for ch in 0..3 {
            lin_t[ch] += f64::from(t[ch]);
            lin_n[ch] += f64::from(u[ch]);
        }
        remaster_t.push(t);
        remaster_n.push(u);
        let mut gp = *p;
        gp.colour = to_codes(p.colour, true);
        gp.spec_map = to_codes(p.spec_map, true);
        let codes_of =
            |t: &Texels, at: usize| -> [f32; 4] { t.codes[at].map(|c| f32::from(c) / 255.0) };
        let (t, _) = model_at(sunburn, &tinted_m, &gamma_scene, &gp, codes_of);
        let (u, _) = model_at(sunburn, &neutral_m, &gamma_scene, &gp, codes_of);
        game_t.push(t);
        game_n.push(u);
    }
    let ratio: [f64; 3] = std::array::from_fn(|ch| lin_t[ch] / lin_n[ch]);
    let game = |v: &[[f32; 4]]| {
        stats(v.iter().map(|c| {
            let stored = [0, 1, 2].map(|ch| (c[ch].clamp(0.0, 1.0) * 255.0).round() as u8);
            (stored.map(|b| srgb_decode(f64::from(b) / 255.0)), stored)
        }))
    };
    let linear_unclamped = |v: &[[f32; 4]]| {
        stats(v.iter().map(|c| {
            let lin = [0, 1, 2].map(|ch| f64::from(c[ch].max(0.0)));
            let stored = lin.map(|l| (srgb_encode(l).clamp(0.0, 1.0) * 255.0).round() as u8);
            (lin, stored)
        }))
    };
    println!(
        "Sunburn's {TINTED}, tint {tint:?} (x{:.3}, {:.3}, {:.3} against neutral's x1.008), {n} px:",
        4.0 * 128.0 / 255.0 * f64::from(tint[0]) / 255.0,
        4.0 * 128.0 / 255.0 * f64::from(tint[1]) / 255.0,
        4.0 * 128.0 / 255.0 * f64::from(tint[2]) / 255.0
    );
    println!(
        "  in linear light (the model): mean colour ({:.4}, {:.4}, {:.4}) against ({:.4}, {:.4}, {:.4}) neutral, x({:.3}, {:.3}, {:.3})",
        lin_t[0] / n as f64,
        lin_t[1] / n as f64,
        lin_t[2] / n as f64,
        lin_n[0] / n as f64,
        lin_n[1] / n as f64,
        lin_n[2] / n as f64,
        ratio[0],
        ratio[1],
        ratio[2]
    );
    println!(
        "  {}",
        show(
            "remaster before tonemapping, tinted",
            &linear_unclamped(&remaster_t)
        )
    );
    println!(
        "  {}",
        show(
            "remaster before tonemapping, neutral",
            &linear_unclamped(&remaster_n)
        )
    );
    println!(
        "  {}",
        show("the game's arithmetic, tinted", &game(&game_t))
    );
    println!(
        "  {}",
        show("the game's arithmetic, neutral", &game(&game_n))
    );
    for glow in [false, true] {
        for neutral in [false, true] {
            let s = shot(1, Kind::Look { glow, neutral }).byte();
            let label = format!(
                "remaster tonemapped{}, {}",
                if glow { " with the glow" } else { "" },
                if neutral { "neutral" } else { "tinted" }
            );
            println!("  {}", show(&label, &look_stats(s, &region)));
        }
    }

    println!(
        "lane: {:.1} s wall ({:.1} s rendering)",
        started.elapsed().as_secs_f64(),
        rendered.elapsed().as_secs_f64()
    );
    assert!(failures.is_empty(), "{failures:#?}");
}

#[test]
fn halves_round_to_nearest_and_split_back() {
    for (v, bits) in [
        (0.0, 0),
        (1.0, 0x3C00),
        (-2.0, 0xC000),
        (65504.0, 0x7BFF),
        (2f32.powi(-14), 0x0400),
        (2f32.powi(-24), 0x0001),
        (1.0 + 2f32.powi(-11), 0x3C00),
        (1.0 + 3.0 * 2f32.powi(-11), 0x3C02),
        (0.333_333_34, 0x3555),
    ] {
        assert_eq!(f32_to_f16(v), bits, "{v}");
        assert_eq!(half(v), f16_to_f32(bits), "{v}");
    }
    // A probe pair: hi the value with 13 low bits cleared (a half holds it
    // exactly), lo the rest times 4096, stored as a half truncated.
    for v in [16.573_708_f32, -0.734_434_4, 0.000_123_45, 3.0e-6] {
        let hi = f32::from_bits(v.to_bits() & 0xFFFF_E000);
        let lo = truncate_half((v - hi) * 4096.0);
        let back = unsplit(half(hi), lo);
        assert!(
            (back - v).abs() <= v.abs() * 2f32.powi(-20) + 1e-7,
            "{v}: {back}"
        );
    }
    assert_eq!(
        truncate_half(1.0 + 1.5 * 2f32.powi(-10)),
        1.0 + 2f32.powi(-10)
    );
    assert_eq!(half_step(1.5), 2f32.powi(-10));
}

#[test]
fn texels_decode_exactly_and_wrap() {
    assert_eq!(exact(255, true, 0), 1.0);
    assert_eq!(exact(0, true, 0), 0.0);
    assert!((exact(172, true, 0) - 0.4125).abs() < 5e-5);
    // Alpha stays linear on an sRGB view; a normal map is code / 255.
    assert_eq!(exact(128, true, 3), 128.0 / 255.0);
    assert_eq!(exact(128, false, 0), 128.0 / 255.0);
    let t = Texels {
        width: 4,
        height: 2,
        codes: (0..8).map(|i| [i as u8; 4]).collect(),
    };
    let r = (Wrap::Repeat, Wrap::Repeat);
    assert_eq!(texel_at(&t, [0.0, 0.0], r), 0);
    assert_eq!(texel_at(&t, [0.99, 0.99], r), 7);
    assert_eq!(texel_at(&t, [1.1, -0.1], r), 4);
    let c = (Wrap::Clamp, Wrap::Clamp);
    assert_eq!(texel_at(&t, [1.1, -0.1], c), 3);
    // The look's colour maths: white is L 1, C 0; sRGB's encode undoes
    // its decode.
    let (l, c, _) = oklch([1.0; 3]);
    assert!((l - 1.0).abs() < 1e-6 && c < 1e-6);
    for code in [0u8, 1, 10, 100, 172, 255] {
        let e = f64::from(code) / 255.0;
        assert!((srgb_encode(srgb_decode(e)) - e).abs() < 1e-12);
    }
    let m = erode(&vec![true; (SIZE.x * SIZE.y) as usize], 2);
    assert!(!m[0] && m[(3 * SIZE.x + 3) as usize]);
}
