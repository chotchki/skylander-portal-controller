// Bevy system signatures trip both by design.
#![allow(clippy::type_complexity, clippy::too_many_arguments)]
//! The grass and the warp on the GPU (openspec AG.24): the character
//! material's two effect passes (`character::effect`, the shader's `grass`
//! and `warp`) drawn as the TV draws them, against their rules written out
//! here on the CPU.
//!
//! Each case is a frame of its own under an orthographic camera, one unit a
//! pixel, into an `Rgba16Float` target with tonemapping and MSAA off and
//! Bevy's screen-space transmission on (as `remaster::camera` has it). An
//! unlit background stands behind a quad drawn with the effect, both with
//! tints multiplying to 1, so a colour pass writes its colour map's RGB.
//!
//! - The grass: a shell over the whole frame, its blade and height maps one
//!   value each (so every pixel's reach is the same), its height up the
//!   stack the case's. It draws its colour map darkened toward the roots
//!   where the reach clears the floor raised by its height, and leaves the
//!   background where it doesn't.
//! - The warp: a surface over the whole frame in front of a background
//!   whose left half is one colour and right half another. Its ripple map
//!   at a half bends nothing, so it shows the background as it is; at 1 in
//!   red it reads the frame `BEND` of its width to the right, moving the
//!   halves' edge left by that much.
//!
//! `cargo test -p guacamole --release --test effect_gpu_lane -- --ignored --nocapture --test-threads 1`

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
use bevy::pbr::ScreenSpaceTransmission;
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
use guac_ir::model::Segment;
use guac_ir::shading::{self, Fixed, Scene};
use guacamole::character::effect::{Effect, GRASS_FLOOR};
use guacamole::character::{
    CharacterMaterial, CharacterShadingPlugin, CharacterTextures, MaterialKey, Pass, ShaderKey,
    StateKey, static_mesh,
};

/// The frame, in pixels (one unit each).
const WIDTH: u32 = 64;
const HEIGHT: u32 = 32;
/// The grass's colour map, the background behind it and the warp's
/// background's two halves.
const GRASS: [f32; 4] = [0.5, 0.25, 0.125, 1.0];
const BACK: [f32; 4] = [0.125, 0.5, 0.25, 1.0];
const LEFT: [f32; 4] = [0.25, 0.125, 0.5, 1.0];
const RIGHT: [f32; 4] = [0.5, 0.5, 0.0625, 1.0];
/// How far down the view the effect and its background stand.
const EFFECT: f32 = 10.0;
const BACKGROUND: f32 = 20.0;
/// The warp's bend in the cases, a share of the frame: 16 pixels, past the
/// sampler's reach either side of an edge.
const BEND: f32 = 0.25;
/// Pixels kept from an edge either side.
const MARGIN: u32 = 2;
const CLEAR: [f32; 4] = [0.0, 0.0, 0.0, 0.0];
const EVERY: u32 = 20;
const MAX_WAIT: Duration = Duration::from_secs(180);
const SETTLED: u32 = 3;

#[derive(Debug, Clone, Copy)]
enum Kind {
    /// A grass shell `shell` up its stack, its blade and height maps' value.
    Grass { shell: f32, blade: f32, height: f32 },
    /// A warp, its ripple map's red and green.
    Warp { ripple: [f32; 2] },
}

#[derive(Debug, Clone, Copy)]
struct Case {
    name: &'static str,
    kind: Kind,
}

const CASES: [Case; 6] = [
    Case {
        name: "the ground's shell, past the floor",
        kind: Kind::Grass {
            shell: 0.0,
            blade: 1.0,
            height: 0.5,
        },
    },
    Case {
        name: "the ground's shell, under the floor",
        kind: Kind::Grass {
            shell: 0.0,
            blade: 0.25,
            height: 0.5,
        },
    },
    Case {
        name: "half way up, reaching it",
        kind: Kind::Grass {
            shell: 0.5,
            blade: 1.0,
            height: 0.875,
        },
    },
    Case {
        name: "half way up, short of it",
        kind: Kind::Grass {
            shell: 0.5,
            blade: 1.0,
            height: 0.6875,
        },
    },
    Case {
        name: "a warp bending nothing",
        kind: Kind::Warp { ripple: [0.5, 0.5] },
    },
    Case {
        name: "a warp bending right",
        kind: Kind::Warp { ripple: [1.0, 0.5] },
    },
];

/// The grass's rule: a shell draws where its reach (the blade map's value
/// times the height map's squared) clears the floor raised by its height up
/// the stack, darkened toward the roots; `None` where it leaves the
/// background.
fn grass_drawn(shell: f32, blade: f32, height: f32) -> Option<f32> {
    let reach = blade * height * height;
    let floor = GRASS_FLOOR + (1.0 - GRASS_FLOOR) * shell;
    (reach > floor).then_some(0.7 + 0.3 * shell)
}

/// The warp's rule: pixel column `x` shows the background's column `x`
/// moved right by the ripple's red, mapped from 0..1 to -1..1, times `BEND`
/// of the frame.
fn warp_reads(x: u32, ripple: [f32; 2]) -> f32 {
    x as f32 + 0.5 + (2.0 * ripple[0] - 1.0) * BEND * WIDTH as f32
}

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

/// `v` as the nearest half's bits (`v` 0 or a normal half).
fn half_bits(v: f32) -> u16 {
    if v == 0.0 {
        return 0;
    }
    let e = v.log2().floor() as i32;
    let m = ((v / 2f32.powi(e) - 1.0) * 1024.0).round() as u16;
    ((e + 15) as u16) << 10 | m
}

/// The gap between neighbouring halves at `v`: what the target stores to.
fn half_step(v: f32) -> f32 {
    2f32.powi(v.abs().log2().floor() as i32 - 10)
}

/// A quad over x0..x1 (pixels from the frame's left) by the frame's height
/// at `depth`, facing the camera.
fn quad(x0: f32, x1: f32, depth: f32) -> Segment {
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
        joints: Vec::new(),
        weights: Vec::new(),
        indices: vec![0, 1, 2, 0, 2, 3],
    }
}

/// An unlit part with tints of a half each (4 x 0.5 x 0.5 = 1), drawing
/// `textures`' colour map in the colour pass.
fn material(textures: CharacterTextures) -> CharacterMaterial {
    let key = MaterialKey {
        shader: ShaderKey::new(0, 0).unwrap(),
        states: StateKey::OPAQUE,
        pass: Pass::Colour,
    };
    let mut m = shading::Material::new();
    m.tint = [0.5, 0.5, 0.5, 1.0];
    m.fixed = Fixed {
        shadows: Some(false),
        fog: Some(false),
        point_lights: Some(0),
    };
    let mut scene = Scene::new();
    scene.placement_tint = [0.5, 0.5, 0.5, 1.0];
    CharacterMaterial::new(key, &m, &scene, 0.0, textures).expect("a material")
}

/// A 1 x 1 image of `rgba`, sampled nearest and clamped.
fn texel(rgba: [f32; 4]) -> Image {
    let data = rgba
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
    mut materials: ResMut<Assets<CharacterMaterial>>,
    mut targets: ResMut<Targets>,
) {
    let mut colour = |rgba| CharacterTextures {
        colour: Some(images.add(texel(rgba))),
        ..Default::default()
    };
    let back = materials.add(material(colour(BACK)));
    let left = materials.add(material(colour(LEFT)));
    let right = materials.add(material(colour(RIGHT)));
    let half = WIDTH as f32 / 2.0;
    for (n, c) in CASES.iter().enumerate() {
        let layer = RenderLayers::layer(n + 1);
        let mut part = |segment: Segment, m: Handle<CharacterMaterial>| {
            commands.spawn((
                Mesh3d(meshes.add(static_mesh(&segment).expect("a mesh"))),
                MeshMaterial3d(m),
                Transform::IDENTITY,
                layer.clone(),
            ));
        };
        match c.kind {
            Kind::Grass {
                shell,
                blade,
                height,
            } => {
                part(quad(0.0, WIDTH as f32, BACKGROUND), back.clone());
                let mut m = material(CharacterTextures {
                    colour: Some(images.add(texel(GRASS))),
                    normal_map: Some(images.add(texel([blade, 0.0, 0.0, 1.0]))),
                    second_normal_map: Some(images.add(texel([height, 0.0, 0.0, 1.0]))),
                    ..Default::default()
                });
                Effect::Grass { shell }.apply(&mut m);
                part(quad(0.0, WIDTH as f32, EFFECT), materials.add(m));
            }
            Kind::Warp { ripple } => {
                part(quad(0.0, half, BACKGROUND), left.clone());
                part(quad(half, WIDTH as f32, BACKGROUND), right.clone());
                let map = images.add(texel([ripple[0], ripple[1], 0.0, 1.0]));
                let mut m = material(CharacterTextures {
                    colour: Some(map.clone()),
                    normal_map: Some(map),
                    ..Default::default()
                });
                Effect::Warp.apply(&mut m);
                m.uniform.effect.x = BEND;
                part(quad(0.0, WIDTH as f32, EFFECT), materials.add(m));
            }
        }
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
            ScreenSpaceTransmission::default(),
            layer,
        ));
        targets.0.push(handle);
    }
}

/// Whether a capture shows every case drawn: no pixel left clear.
fn drawn(shots: &[Vec<[f32; 4]>]) -> bool {
    shots.len() == CASES.len() && shots.iter().all(|s| s.iter().all(|p| *p != CLEAR))
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
        let middle = previous.as_ref().map(|p| {
            p.iter()
                .map(|s| s[(HEIGHT / 2 * WIDTH + WIDTH / 2) as usize])
                .collect::<Vec<_>>()
        });
        *outcome.0.lock().unwrap() = Some(Err(format!(
            "nothing settled after {MAX_WAIT:?} ({} frames); the middles: {middle:?}",
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
        .expect("the effects drew")
}

/// The worst distance of `xs`' pixels from `want(x)`, in half steps.
fn worst_steps(shot: &[[f32; 4]], xs: std::ops::Range<u32>, want: impl Fn(u32) -> [f32; 3]) -> f32 {
    let mut worst = 0.0f32;
    for y in 0..HEIGHT {
        for x in xs.clone() {
            let p = shot[(y * WIDTH + x) as usize];
            let w = want(x);
            for ch in 0..3 {
                worst = worst.max((p[ch] - w[ch]).abs() / half_step(w[ch]));
            }
        }
    }
    worst
}

fn rgb(c: [f32; 4], k: f32) -> [f32; 3] {
    [c[0] * k, c[1] * k, c[2] * k]
}

#[test]
#[ignore = "GPU lane: run on the Mac (AG.24)"]
fn the_grass_and_the_warp_draw_by_their_rules() {
    let started = Instant::now();
    let shots = render();
    eprintln!(
        "rendered {} cases in {:.1?}",
        CASES.len(),
        started.elapsed()
    );
    for (c, shot) in CASES.iter().zip(&shots) {
        match c.kind {
            Kind::Grass {
                shell,
                blade,
                height,
            } => {
                let want = match grass_drawn(shell, blade, height) {
                    Some(k) => rgb(GRASS, k),
                    None => rgb(BACK, 1.0),
                };
                let worst = worst_steps(shot, 0..WIDTH, |_| want);
                eprintln!(
                    "{}: {} at {want:?}, worst {worst:.2} half steps (the middle {:?})",
                    c.name,
                    if grass_drawn(shell, blade, height).is_some() {
                        "drawn"
                    } else {
                        "cut"
                    },
                    shot[(HEIGHT / 2 * WIDTH + WIDTH / 2) as usize]
                );
                // Within two half steps: the target's store.
                assert!(worst <= 2.0, "{}", c.name);
            }
            Kind::Warp { ripple } => {
                // The column the background's edge lands on, and every
                // column clear of it either side.
                let column = |at: f32| {
                    (0..WIDTH)
                        .find(|&x| warp_reads(x, ripple) >= at)
                        .unwrap_or(WIDTH)
                };
                let edge = column(WIDTH as f32 / 2.0);
                // Past the frame's right edge the copy is read off its
                // sampler's clamp or repeat, which isn't the rule's to say.
                let inside = column(WIDTH as f32).min(WIDTH);
                let left = worst_steps(shot, 0..edge - MARGIN, |_| rgb(LEFT, 1.0));
                let right = worst_steps(shot, edge + MARGIN..inside, |_| rgb(RIGHT, 1.0));
                let row: Vec<f32> = (0..WIDTH)
                    .map(|x| shot[(HEIGHT / 2 * WIDTH + x) as usize][0])
                    .collect();
                eprintln!(
                    "{}: the edge at column {edge}, left worst {left:.2} half steps, right {right:.2}; the middle row's red {row:.3?}",
                    c.name
                );
                assert!(left <= 2.0, "{}: left of the edge", c.name);
                assert!(right <= 2.0, "{}: right of the edge", c.name);
            }
        }
    }
}

#[test]
fn the_fixtures_are_what_the_lane_says() {
    // The textures' values are halves exactly.
    for v in [0.25, 0.5, 0.6875, 0.875, 1.0, 0.0625, 0.125] {
        assert_eq!(f16_to_f32(half_bits(v)), v);
    }
    // Two of the grass's four cases draw and two leave the background, the
    // floor and the height's square each deciding one.
    let drawn: Vec<bool> = CASES
        .iter()
        .filter_map(|c| match c.kind {
            Kind::Grass {
                shell,
                blade,
                height,
            } => Some(grass_drawn(shell, blade, height).is_some()),
            Kind::Warp { .. } => None,
        })
        .collect();
    assert_eq!(drawn, [true, false, true, false]);
    // Under the floor but past nothing: without the floor it would draw.
    const { assert!(0.25 * 0.5 * 0.5 > 0.0 && 0.25 * 0.5 * 0.5 <= GRASS_FLOOR) };
    // Short with the height squared, reaching without.
    let floor = GRASS_FLOOR + (1.0 - GRASS_FLOOR) * 0.5;
    assert!((0.6875f32 * 0.6875..0.6875).contains(&floor));
    // A half bends nothing; 1 moves the edge left by the bend.
    assert_eq!(warp_reads(10, [0.5, 0.5]), 10.5);
    assert_eq!(warp_reads(10, [1.0, 0.5]), 26.5);
    const { assert!(EFFECT < BACKGROUND) };
}
