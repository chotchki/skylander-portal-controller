// Bevy system signatures trip both by design.
#![allow(clippy::type_complexity, clippy::too_many_arguments)]
//! The shadow term on the GPU (openspec AG.23): a character's part casting
//! into the key light's cascades and a level's part receiving it, through
//! the same pieces the TV uses (`character::shadow::shadow_light`, the
//! character material's shadow-receive term, `mark_casters`), against the
//! mapping's CPU half, `remaster::shadow_sample`.
//!
//! Each case is a receiving quad over the whole frame, one unit a pixel
//! under an orthographic camera, static (so it casts none) and unlit with
//! tints multiplying to 1, so its colour pass writes its colour map's RGB
//! times the shadow term and nothing else. A skinned opaque quad nearer the
//! camera covers the frame's left half on a render layer the camera doesn't
//! draw and the key light does, the light travelling straight into the
//! frame, so the left half is wholly shadowed (visibility 0) and the right
//! wholly lit (1). The frame goes into an `Rgba16Float` target with
//! tonemapping and MSAA off. Two cases receive in shadow modes 1 and 2
//! (the remaster's sample is grey, so the mode changes nothing); a third
//! draws on a layer the light doesn't reach and stays lit behind the
//! caster; a fourth receives 40 units down the view, in the last cascade,
//! so the shader picks a cascade by the view's depth (the shadows review:
//! one picked the wrong way drew the first three cases as they should).
//!
//! `cargo test -p guacamole --release --test shadow_gpu_lane -- --ignored --nocapture --test-threads 1`

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use bevy::app::ScheduleRunnerPlugin;
use bevy::asset::RenderAssetUsages;
use bevy::camera::visibility::RenderLayers;
use bevy::camera::{
    ClearColorConfig, Hdr, OrthographicProjection, Projection, RenderTarget, ScalingMode,
};
use bevy::color::LinearRgba;
use bevy::core_pipeline::tonemapping::{DebandDither, Tonemapping};
use bevy::image::{ImageAddressMode, ImageFilterMode, ImageSampler, ImageSamplerDescriptor};
use bevy::log::LogPlugin;
use bevy::math::Vec3;
use bevy::mesh::skinning::SkinnedMeshInverseBindposes;
use bevy::prelude::{
    App, AppExit, Assets, Camera, Camera3d, Color, Commands, DefaultPlugins, Handle, Image, Local,
    Mesh, Mesh3d, MeshMaterial3d, MessageWriter, On, PluginGroup, Res, ResMut, Resource, Startup,
    Transform, Update, WindowPlugin, default,
};
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};
use bevy::render::view::Msaa;
use bevy::render::view::window::screenshot::{Screenshot, ScreenshotCaptured};
use bevy::window::ExitCondition;
use bevy::winit::WinitPlugin;
use guac_ir::clip::Clip;
use guac_ir::model::{self, Joint, Model, Segment, Skeleton};
use guac_ir::shading::{self, Fixed, Scene};
use guacamole::character::{
    CharacterMaterial, CharacterShadingPlugin, CharacterTextures, MaterialKey, Pass, ShaderKey,
    StateKey, remaster, shadow, spawn_character, static_mesh,
};

/// The frame, in pixels (one unit each), and the receiver's colour map.
const WIDTH: u32 = 64;
const HEIGHT: u32 = 32;
const COLOUR: [f32; 4] = [0.5, 0.25, 0.125, 1.0];
/// How far down the view the receiver and the caster stand: the caster
/// between it and the light (which travels down the view), both inside the
/// key light's first cascade.
const RECEIVER: f32 = 10.0;
const CASTER: f32 = 5.0;
/// The far case's receiver: past the second cascade's far bound, inside
/// the last's.
const FAR_RECEIVER: f32 = 40.0;
/// The layer the caster is on: the light's, no camera's.
const CASTER_LAYER: usize = 9;
/// Pixels kept from the shadow's edge either side, past its filter.
const MARGIN: u32 = 4;
const CLEAR: [f32; 4] = [0.0, 0.0, 0.0, 0.0];
const EVERY: u32 = 20;
const MAX_WAIT: Duration = Duration::from_secs(180);
const SETTLED: u32 = 3;

/// A case: the receiver's shadow mode, whether the light reaches it and
/// how far down the view it stands.
#[derive(Debug, Clone, Copy)]
struct Case {
    name: &'static str,
    mode: f32,
    lit_by_key: bool,
    depth: f32,
}

const CASES: [Case; 4] = [
    Case {
        name: "shadow mode 1",
        mode: 1.0,
        lit_by_key: true,
        depth: RECEIVER,
    },
    Case {
        name: "shadow mode 2",
        mode: 2.0,
        lit_by_key: true,
        depth: RECEIVER,
    },
    Case {
        name: "no key light",
        mode: 2.0,
        lit_by_key: false,
        depth: RECEIVER,
    },
    Case {
        name: "the last cascade",
        mode: 1.0,
        lit_by_key: true,
        depth: FAR_RECEIVER,
    },
];

fn f16_to_f32(h: u16) -> f32 {
    let sign = if h >> 15 == 1 { -1.0 } else { 1.0 };
    let exp = i32::from(h >> 10 & 0x1F);
    let frac = f32::from(h & 0x3FF);
    sign * match exp {
        0 => frac * 2f32.powi(-24),
        31 => f32::NAN,
        e => (1.0 + frac / 1024.0) * 2f32.powi(e - 15),
    }
}

/// `v` as the nearest half's bits (`v` a normal half, as the colours are).
fn half_bits(v: f32) -> u16 {
    let e = v.log2().floor() as i32;
    let m = ((v / 2f32.powi(e) - 1.0) * 1024.0).round() as u16;
    ((e + 15) as u16) << 10 | m
}

/// One half step at `v`.
fn half_step(v: f32) -> f32 {
    2f32.powi(v.abs().log2().floor() as i32 - 10)
}

/// A quad over x0..x1 (pixels from the frame's left) by the frame's height
/// at `depth`, facing the camera, on joint 0 when `skinned`.
fn quad(x0: f32, x1: f32, depth: f32, skinned: bool) -> Segment {
    let n = if skinned { 4 } else { 0 };
    let (w, h) = (WIDTH as f32 / 2.0, HEIGHT as f32 / 2.0);
    let (l, r) = (x0 - w, x1 - w);
    Segment {
        material: 0,
        source: 0,
        positions: vec![
            [l, h, -depth],
            [l, -h, -depth],
            [r, -h, -depth],
            [r, h, -depth],
        ],
        normals: vec![[0.0, 0.0, 1.0]; 4],
        tangents: None,
        uvs: vec![vec![[0.0, 0.0], [0.0, 1.0], [1.0, 1.0], [1.0, 0.0]]],
        colors: None,
        joints: vec![[0; 4]; n],
        weights: vec![[255, 0, 0, 0]; n],
        indices: vec![0, 1, 2, 0, 2, 3],
    }
}

/// One joint, one segment: the caster.
fn caster_model() -> Model {
    Model {
        identity: guac_ir::DumpIdentity {
            title_id: "LANE00000".into(),
            app_ver: "01.00".into(),
            manifest_hash: guac_ir::Sha256([0; 32]),
        },
        load_file: "caster".into(),
        name: "caster".into(),
        skeleton: Skeleton {
            joints: vec![Joint {
                name: "root".into(),
                parent: None,
                reference: model::Transform::IDENTITY,
                inverse_bind: bevy::math::Mat4::IDENTITY.to_cols_array_2d(),
            }],
        },
        materials: Vec::new(),
        segments: vec![quad(-8.0, WIDTH as f32 / 2.0, CASTER, true)],
        idle: Clip {
            name: "still".into(),
            duration: 1.0,
            frame_duration: 1.0 / 30.0,
            frames: 31,
            tracks: Vec::new(),
        },
        attachments: Vec::new(),
        attachments_unsettled: None,
    }
}

/// An unlit part with tints of a half each (4 x 0.5 x 0.5 = 1): its colour
/// map times the shadow term when `shadows`, in shadow mode `mode`.
fn material(shadows: bool, mode: f32, colour: Handle<Image>) -> CharacterMaterial {
    let bits = if shadows { 0x100 } else { 0 };
    let key = MaterialKey {
        shader: ShaderKey::new(bits, bits).unwrap(),
        states: StateKey::OPAQUE,
        pass: Pass::Colour,
    };
    let mut m = shading::Material::new();
    m.tint = [0.5, 0.5, 0.5, 1.0];
    m.fixed = Fixed {
        shadows: Some(shadows),
        fog: Some(false),
        point_lights: Some(0),
    };
    let mut scene = Scene::new();
    scene.placement_tint = [0.5, 0.5, 0.5, 1.0];
    scene.shadows = shadows;
    scene.shadow_mode = mode;
    CharacterMaterial::new(
        key,
        &m,
        &scene,
        0.0,
        CharacterTextures {
            colour: Some(colour),
            ..Default::default()
        },
    )
    .expect("a material")
}

fn colour_map() -> Image {
    let data = COLOUR
        .map(half_bits)
        .into_iter()
        .flat_map(u16::to_le_bytes)
        .collect();
    let mut image = Image::new(
        Extent3d {
            width: 1,
            height: 1,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        data,
        TextureFormat::Rgba16Float,
        RenderAssetUsages::RENDER_WORLD,
    );
    image.sampler = ImageSampler::Descriptor(ImageSamplerDescriptor {
        address_mode_u: ImageAddressMode::ClampToEdge,
        address_mode_v: ImageAddressMode::ClampToEdge,
        mag_filter: ImageFilterMode::Nearest,
        min_filter: ImageFilterMode::Nearest,
        ..ImageSamplerDescriptor::default()
    });
    image
}

#[derive(Resource, Default)]
struct Targets(Vec<Handle<Image>>);

#[derive(Resource, Clone, Default)]
struct Captured(Arc<Mutex<Vec<Option<Vec<[f32; 4]>>>>>);

#[derive(Resource, Clone, Default)]
struct Outcome(Arc<Mutex<Option<Result<Vec<Vec<[f32; 4]>>, String>>>>);

fn setup(
    mut commands: Commands,
    mut images: ResMut<Assets<Image>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut bindposes: ResMut<Assets<SkinnedMeshInverseBindposes>>,
    mut materials: ResMut<Assets<CharacterMaterial>>,
    mut targets: ResMut<Targets>,
) {
    let colour = images.add(colour_map());
    // The light reaches the caster's layer and the cases it lights.
    let lit: Vec<usize> = CASES
        .iter()
        .enumerate()
        .filter(|(_, c)| c.lit_by_key)
        .map(|(n, _)| n + 1)
        .chain([CASTER_LAYER])
        .collect();
    commands.spawn((
        shadow::shadow_light(Vec3::NEG_Z),
        RenderLayers::from_layers(&lit),
    ));
    let caster = materials.add(material(false, 2.0, colour.clone()));
    let spawned = spawn_character(
        &mut commands,
        &caster_model(),
        Transform::IDENTITY,
        0.0,
        &mut meshes,
        &mut bindposes,
        &[caster],
    )
    .expect("the caster spawns");
    for e in &spawned.segments {
        commands
            .entity(*e)
            .insert(RenderLayers::layer(CASTER_LAYER));
    }
    for (n, c) in CASES.iter().enumerate() {
        let layer = n + 1;
        let ground =
            meshes.add(static_mesh(&quad(0.0, WIDTH as f32, c.depth, false)).expect("a mesh"));
        commands.spawn((
            Mesh3d(ground),
            MeshMaterial3d(materials.add(material(true, c.mode, colour.clone()))),
            Transform::IDENTITY,
            RenderLayers::layer(layer),
        ));
        let mut target = Image::new_target_texture(WIDTH, HEIGHT, TextureFormat::Rgba16Float, None);
        target.asset_usage = RenderAssetUsages::default();
        let handle = images.add(target);
        commands.spawn((
            Camera3d::default(),
            Camera {
                clear_color: ClearColorConfig::Custom(Color::LinearRgba(LinearRgba::new(
                    CLEAR[0], CLEAR[1], CLEAR[2], CLEAR[3],
                ))),
                order: n as isize,
                ..default()
            },
            RenderTarget::Image(handle.clone().into()),
            Projection::Orthographic(OrthographicProjection {
                scaling_mode: ScalingMode::Fixed {
                    width: WIDTH as f32,
                    height: HEIGHT as f32,
                },
                near: 0.0,
                far: 1000.0,
                ..OrthographicProjection::default_3d()
            }),
            Transform::IDENTITY,
            Hdr,
            Tonemapping::None,
            DebandDither::Disabled,
            Msaa::Off,
            RenderLayers::layer(layer),
        ));
        targets.0.push(handle);
    }
}

/// Whether a capture shows the shadow drawn: every pixel drawn, and the
/// lit cases darker left than right.
fn drawn(shots: &[Vec<[f32; 4]>]) -> bool {
    shots.iter().zip(CASES).all(|(shot, c)| {
        let all = shot.iter().all(|p| *p != CLEAR);
        let left = shot[(HEIGHT / 2 * WIDTH + MARGIN) as usize][0];
        let right = shot[(HEIGHT / 2 * WIDTH + WIDTH - 1 - MARGIN) as usize][0];
        all && (!c.lit_by_key || left < 0.5 * right)
    })
}

fn capture(
    mut commands: Commands,
    targets: Res<Targets>,
    captured: Res<Captured>,
    outcome: Res<Outcome>,
    mut frame: Local<u32>,
    mut started: Local<Option<Instant>>,
    mut previous: Local<Option<Vec<Vec<[f32; 4]>>>>,
    mut agreeing: Local<u32>,
    mut exit: MessageWriter<AppExit>,
) {
    *frame += 1;
    let started = *started.get_or_insert_with(Instant::now);
    let done = {
        let c = captured.0.lock().unwrap();
        !c.is_empty() && c.iter().all(Option::is_some)
    };
    if done {
        let shots: Vec<Vec<[f32; 4]>> = std::mem::take(&mut *captured.0.lock().unwrap())
            .into_iter()
            .map(Option::unwrap)
            .collect();
        let stable = previous.as_ref() == Some(&shots);
        *agreeing = if stable { *agreeing + 1 } else { 0 };
        if drawn(&shots) && *agreeing >= SETTLED {
            *outcome.0.lock().unwrap() = Some(Ok(shots));
            exit.write(AppExit::Success);
            return;
        }
        *previous = Some(shots);
    }
    if started.elapsed() > MAX_WAIT {
        let last = previous.as_ref().map(|p| {
            p.iter()
                .map(|s| {
                    (
                        s[(HEIGHT / 2 * WIDTH + MARGIN) as usize],
                        s[(HEIGHT / 2 * WIDTH + WIDTH - 1 - MARGIN) as usize],
                    )
                })
                .collect::<Vec<_>>()
        });
        *outcome.0.lock().unwrap() = Some(Err(format!(
            "no shadow drawn after {MAX_WAIT:?} ({} frames); last left and right: {last:?}",
            *frame
        )));
        exit.write(AppExit::error());
        return;
    }
    if (*frame).is_multiple_of(EVERY) && captured.0.lock().unwrap().is_empty() {
        *captured.0.lock().unwrap() = vec![None; targets.0.len()];
        for (i, handle) in targets.0.iter().enumerate() {
            let store = captured.0.clone();
            commands.spawn(Screenshot::image(handle.clone())).observe(
                move |c: On<ScreenshotCaptured>| {
                    let data = c.image.data.as_ref().expect("pixels");
                    let px: Vec<[f32; 4]> = data
                        .as_chunks::<8>()
                        .0
                        .iter()
                        .map(|p| {
                            std::array::from_fn(|ch| {
                                f16_to_f32(u16::from_le_bytes([p[2 * ch], p[2 * ch + 1]]))
                            })
                        })
                        .collect();
                    store.lock().unwrap()[i] = Some(px);
                },
            );
        }
    }
}

fn render() -> Vec<Vec<[f32; 4]>> {
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
    ))
    .insert_resource(Targets::default())
    .insert_resource(Captured::default())
    .insert_resource(outcome.clone())
    .add_systems(Startup, setup)
    .add_systems(Update, capture);
    app.run();
    outcome
        .0
        .lock()
        .unwrap()
        .take()
        .expect("an outcome")
        .expect("the shadow drew")
}

/// How far each pixel of a half is from `want` times the colour map, in
/// half steps: the worst over the half's pixels.
fn worst_steps(shot: &[[f32; 4]], xs: std::ops::Range<u32>, term: f32) -> f32 {
    let mut worst = 0.0f32;
    for y in 0..HEIGHT {
        for x in xs.clone() {
            let p = shot[(y * WIDTH + x) as usize];
            for ch in 0..3 {
                let want = COLOUR[ch] * term;
                worst = worst.max((p[ch] - want).abs() / half_step(want));
            }
        }
    }
    worst
}

#[test]
#[ignore = "GPU lane: run on the Mac (AG.23)"]
fn a_cast_shadow_scales_the_receiver_by_the_mapped_term() {
    let started = Instant::now();
    let shots = render();
    eprintln!(
        "rendered {} cases in {:.1?}",
        CASES.len(),
        started.elapsed()
    );
    let shadowed = 2.0 * remaster::shadow_sample(0.0)[0];
    let lit = 2.0 * remaster::shadow_sample(1.0)[0];
    assert_eq!(lit, 1.0);
    let half = WIDTH / 2;
    for (c, shot) in CASES.iter().zip(&shots) {
        let left = worst_steps(
            shot,
            0..half - MARGIN,
            if c.lit_by_key { shadowed } else { lit },
        );
        let right = worst_steps(shot, half + MARGIN..WIDTH, lit);
        let edge: Vec<f32> = (half - MARGIN..half + MARGIN)
            .map(|x| shot[(HEIGHT / 2 * WIDTH + x) as usize][0] / COLOUR[0])
            .collect();
        eprintln!(
            "{}: left worst {left:.2} half steps from {:.4} of the colour, right {right:.2} from {lit}; the edge's terms {edge:.3?}",
            c.name,
            if c.lit_by_key { shadowed } else { lit },
        );
        // Within two half steps: the target's store and the GPU's pow.
        assert!(left <= 2.0, "{}: the shadowed half", c.name);
        assert!(right <= 2.0, "{}: the lit half", c.name);
    }
}

#[test]
fn the_fixtures_are_what_the_lane_says() {
    assert_eq!(f16_to_f32(half_bits(0.5)), 0.5);
    assert_eq!(f16_to_f32(half_bits(0.125)), 0.125);
    let caster = &caster_model().segments[0];
    // The caster covers the left half and a little past the frame's edge.
    let xs: Vec<f32> = caster.positions.iter().map(|p| p[0]).collect();
    assert_eq!((xs[0], xs[2]), (-8.0 - WIDTH as f32 / 2.0, 0.0));
    assert!(caster.positions.iter().all(|p| p[2] == -CASTER));
    const { assert!(CASTER < RECEIVER) };
    // The near cases in the first cascade, the far one in the last, past
    // the second's far bound.
    let bounds = shadow::cascades().bounds;
    assert_eq!(bounds.len(), 3);
    assert!(RECEIVER < bounds[0], "{bounds:?}");
    assert!(
        bounds[1] < FAR_RECEIVER && FAR_RECEIVER < bounds[2],
        "{bounds:?}"
    );
}
