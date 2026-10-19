// Bevy system signatures trip both by design.
#![allow(clippy::type_complexity, clippy::too_many_arguments)]
//! The glow chain on the GPU against its CPU reference (openspec AE.11,
//! design decision 10): `guacamole::character::glow` and its shader, fed
//! through the same pieces hero_view and the turntable use (a character
//! material's glow pass drawn by `spawn_glow_masks` for the camera
//! `spawn_glow_camera` adds), against [`guac_ir::glow::chain`] on the same
//! frame and mask.
//!
//! Each case is one quad over the whole frame, its material key 0 with
//! tints multiplying to 1, so the colour pass writes exactly its colour
//! map's sample (half grey times two, nothing else), and the glow pass its
//! glow: the colour map's alpha (the glow attribute off), an amount, or
//! nothing. The colour map is a half-float texture the frame's size,
//! sampled nearest at texel centres, so the frame and the mask the chain
//! starts from are known exactly: the map's RGB and its alpha. The frame
//! goes into an `Rgba16Float` target with tonemapping off, MSAA off.
//!
//! `cargo test -p guacamole --release --test glow_gpu_lane -- --ignored --nocapture --test-threads 1`

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
use bevy::math::UVec2;
use bevy::mesh::skinning::SkinnedMeshInverseBindposes;
use bevy::prelude::{
    App, AppExit, Assets, Camera, Camera3d, Color, Commands, DefaultPlugins, Handle, Image, Local,
    Mesh, MessageWriter, On, PluginGroup, Res, ResMut, Resource, Startup, Transform, Update,
    WindowPlugin, default,
};
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};
use bevy::render::view::Msaa;
use bevy::render::view::window::screenshot::{Screenshot, ScreenshotCaptured};
use bevy::window::ExitCondition;
use bevy::winit::WinitPlugin;
use guac_ir::clip::Clip;
use guac_ir::glow::{self as reference, Image as Plane};
use guac_ir::model::{self, Joint, Model, Segment, Skeleton};
use guac_ir::shading::{self, Fixed, Glow, Scene};
use guacamole::character::{
    CharacterMaterial, CharacterShadingPlugin, CharacterTextures, GlowPlugin, MaterialKey, Pass,
    ShaderKey, ShadowLookup, StateKey, spawn_character, spawn_glow_camera, spawn_glow_masks,
};

/// The clear colour, exact in half precision; the quad covers every pixel.
const CLEAR: [f32; 4] = [0.0, 0.0, 0.0, 0.0];
/// Frames between captures, and the longest to wait for the pipelines.
const EVERY: u32 = 20;
const MAX_WAIT: Duration = Duration::from_secs(180);
/// Captures in a row that agree before a target counts as done.
const SETTLED: u32 = 3;

// ---- numbers ----

struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }

    /// Uniform in [0, 1).
    fn unit(&mut self) -> f32 {
        (self.next() >> 40) as f32 / (1u64 << 24) as f32
    }
}

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

/// One half-precision step at `v`: the spacing of the halves around it.
fn half_step(v: f32) -> f32 {
    let a = v.abs();
    if a < 2f32.powi(-14) {
        2f32.powi(-24)
    } else {
        2f32.powi(a.log2().floor() as i32 - 10)
    }
}

/// The half at or below `v` (`v` in [0, 65504]) and its bits: the
/// textures hold exactly what the reference reads.
fn to_half(v: f32) -> (f32, u16) {
    if v < 2f32.powi(-24) {
        return (0.0, 0);
    }
    let e = v.log2().floor() as i32;
    if e < -14 {
        let m = (v / 2f32.powi(-24)) as u16;
        return (f32::from(m) * 2f32.powi(-24), m);
    }
    let m = ((v / 2f32.powi(e) - 1.0) * 1024.0) as u16;
    let bits = (((e + 15) as u16) << 10) | m;
    (f16_to_f32(bits), bits)
}

// ---- cases ----

#[derive(Clone)]
struct Case {
    name: String,
    width: u32,
    height: u32,
    glow: Glow,
    /// The colour map, texel for texel: the frame's RGB and, through layer
    /// 1's alpha, its mask.
    texels: Vec<[f32; 4]>,
    /// A part nearer the camera with no glow, over pixels x0..x1 by
    /// y0..y1, in one flat colour.
    front: Option<Front>,
}

#[derive(Clone, Copy)]
struct Front {
    rect: [u32; 4],
    colour: [f32; 4],
}

impl Front {
    fn covers(&self, x: usize, y: usize) -> bool {
        let [x0, y0, x1, y1] = self.rect.map(|v| v as usize);
        (x0..x1).contains(&x) && (y0..y1).contains(&y)
    }
}

impl Case {
    fn frame(&self) -> Plane {
        let w = self.width as usize;
        Plane::from_fn(w, self.height as usize, |x, y| match self.front {
            Some(f) if f.covers(x, y) => f.colour,
            _ => self.texels[y * w + x],
        })
    }

    /// The glow pass's mask: the part's glow, zero where the front part
    /// is the nearer surface.
    fn mask(&self) -> Plane {
        let w = self.width as usize;
        Plane::from_fn(w, self.height as usize, |x, y| {
            if self.front.is_some_and(|f| f.covers(x, y)) {
                return [0.0; 4];
            }
            let m = match self.glow {
                Glow::None => 0.0,
                Glow::Amount(a) => a,
                Glow::LayerAlpha => self.texels[y * w + x][3],
            };
            [m; 4]
        })
    }

    fn want(&self) -> Plane {
        reference::chain(&self.frame(), &self.mask())
    }

    /// The reference as the GPU stores it: the extract, each blur and the
    /// output truncated to half, as the M3 Max stores a half target (AE.8).
    fn stored(&self) -> Plane {
        let truncated = |p: Plane| Plane {
            texels: p.texels.iter().map(|t| t.map(|v| to_half(v).0)).collect(),
            ..p
        };
        let frame = self.frame();
        let size = reference::buffer_size(frame.width, frame.height);
        let e = truncated(reference::extract(&frame, &self.mask(), size));
        let a = truncated(reference::blur(&e, reference::Axis::Across));
        let d = truncated(reference::blur(&a, reference::Axis::Down));
        truncated(reference::add_back(&frame, &d))
    }

    /// The colour map as a half-float image, sampled nearest.
    fn image(&self) -> Image {
        let data = self
            .texels
            .iter()
            .flat_map(|t| t.map(|v| to_half(v).1))
            .flat_map(u16::to_le_bytes)
            .collect();
        let mut image = Image::new(
            Extent3d {
                width: self.width,
                height: self.height,
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
}

/// A frame of smooth HDR colour, a few bright texels and a hard-edged
/// block, its alpha a soft disc, a solid block and a ramp: something for
/// the extract to resample, the blur to spread and the edges to clamp.
fn case(name: &str, width: u32, height: u32, glow: Glow, seed: u64) -> Case {
    let mut rng = Rng(seed);
    let (w, h) = (width as f32, height as f32);
    let phase: [f32; 6] = std::array::from_fn(|_| rng.unit() * std::f32::consts::TAU);
    let bright: Vec<(u32, u32)> = (0..5)
        .map(|_| {
            (
                (rng.unit() * w) as u32 % width,
                (rng.unit() * h) as u32 % height,
            )
        })
        .collect();
    let mut texels = Vec::with_capacity((width * height) as usize);
    for y in 0..height {
        for x in 0..width {
            let (u, v) = (x as f32 / w, y as f32 / h);
            let wave = |k: usize, f: f32| 0.5 + 0.5 * (f * u + 1.7 * f * v + phase[k]).sin();
            let mut rgb = [
                2.0 * wave(0, 7.0) * wave(1, 3.0),
                1.5 * wave(2, 5.0),
                0.75 * wave(3, 11.0) + 0.05 * rng.unit(),
            ];
            if bright.contains(&(x, y)) {
                rgb = [8.0, 6.0, 4.0];
            }
            let block = (0.6..0.75).contains(&u) && (0.2..0.4).contains(&v);
            if block {
                rgb = [3.0, 0.25, 0.5];
            }
            let d = ((u - 0.35) * w / h).hypot(v - 0.5);
            let alpha = if block {
                1.0
            } else if d < 0.2 {
                0.25 + 0.75 * (1.0 - d / 0.2) * wave(4, 3.0)
            } else {
                0.1 * u * wave(5, 2.0)
            };
            let t = [rgb[0], rgb[1], rgb[2], alpha].map(|c| to_half(c).0);
            texels.push(t);
        }
    }
    Case {
        name: name.into(),
        width,
        height,
        glow,
        texels,
        front: None,
    }
}

fn cases() -> Vec<Case> {
    vec![
        case(
            "960x540, the colour map's alpha",
            960,
            540,
            Glow::LayerAlpha,
            0xA11,
        ),
        case(
            "512x512, an amount of 64",
            512,
            512,
            Glow::Amount(64.0 / 255.0),
            0xB0B,
        ),
        case(
            "333x197, the colour map's alpha",
            333,
            197,
            Glow::LayerAlpha,
            0xC0C,
        ),
        case("320x180, no glow", 320, 180, Glow::None, 0xD0D),
        Case {
            front: Some(Front {
                rect: [200, 150, 520, 330],
                colour: [0.5, 1.0, 0.25, 1.0],
            }),
            ..case(
                "960x540, a part in front with no glow",
                960,
                540,
                Glow::Amount(102.0 / 255.0),
                0xE0E,
            )
        },
    ]
}

// ---- the Bevy side ----

/// A quad over pixels x0..x1 by y0..y1 at `depth`, its texture
/// coordinates 0 to 1 over it, drawn with material `material`.
fn quad(c: &Case, rect: [u32; 4], depth: f32, material: u16) -> Segment {
    let (w, h) = (c.width as f32 / 2.0, c.height as f32 / 2.0);
    let [x0, y0, x1, y1] = rect.map(|v| v as f32);
    let (l, r, t, b) = (x0 - w, x1 - w, h - y0, h - y1);
    Segment {
        material,
        source: 0,
        positions: vec![
            [l, t, -depth],
            [l, b, -depth],
            [r, b, -depth],
            [r, t, -depth],
        ],
        normals: vec![[0.0, 0.0, 1.0]; 4],
        tangents: None,
        uvs: vec![vec![[0.0, 0.0], [0.0, 1.0], [1.0, 1.0], [1.0, 0.0]]],
        colors: None,
        joints: vec![[0; 4]; 4],
        weights: vec![[255, 0, 0, 0]; 4],
        indices: vec![0, 1, 2, 0, 2, 3],
    }
}

/// One joint: a quad over the whole frame, one unit a pixel, and the front
/// part's nearer one.
fn quad_model(c: &Case) -> Model {
    let mut segments = vec![quad(c, [0, 0, c.width, c.height], 10.0, 0)];
    if let Some(f) = c.front {
        segments.push(quad(c, f.rect, 5.0, 1));
    }
    Model {
        identity: guac_ir::DumpIdentity {
            title_id: "LANE00000".into(),
            app_ver: "01.00".into(),
            manifest_hash: guac_ir::Sha256([0; 32]),
        },
        load_file: c.name.clone(),
        name: c.name.clone(),
        skeleton: Skeleton {
            joints: vec![Joint {
                name: "root".into(),
                parent: None,
                reference: model::Transform::IDENTITY,
                inverse_bind: bevy::math::Mat4::IDENTITY.to_cols_array_2d(),
            }],
        },
        materials: Vec::new(),
        segments,
        idle: Clip {
            name: "still".into(),
            duration: 1.0,
            frame_duration: 1.0 / 30.0,
            frames: 31,
            tracks: Vec::new(),
        },
    }
}

/// Key 0, tints of a half each (4 x 0.5 x 0.5 = 1): the colour pass writes
/// the colour map's RGB as it is, the glow pass `glow`.
fn material(glow: Glow, colour: Handle<Image>) -> CharacterMaterial {
    let key = MaterialKey {
        shader: ShaderKey::new(0, 0).unwrap(),
        states: StateKey::OPAQUE,
        pass: Pass::Colour,
    };
    let mut m = shading::Material::new();
    m.tint = [0.5, 0.5, 0.5, 1.0];
    m.glow = glow;
    m.fixed = Fixed {
        shadows: Some(false),
        fog: Some(false),
        point_lights: Some(0),
    };
    let mut scene = Scene::new();
    scene.placement_tint = [0.5, 0.5, 0.5, 1.0];
    CharacterMaterial::new(
        key,
        &m,
        &scene,
        ShadowLookup::NONE,
        0.0,
        CharacterTextures {
            colour: Some(colour),
            ..Default::default()
        },
    )
    .expect("a material")
}

#[derive(Resource)]
struct Plan(Arc<Vec<Case>>);

#[derive(Resource, Default)]
struct Targets(Vec<Handle<Image>>);

#[derive(Resource, Clone, Default)]
struct Captured(Arc<Mutex<Vec<Option<Vec<[f32; 4]>>>>>);

#[derive(Resource, Clone, Default)]
struct Outcome(Arc<Mutex<Option<Result<Vec<Vec<[f32; 4]>>, String>>>>);

fn setup(
    mut commands: Commands,
    plan: Res<Plan>,
    mut images: ResMut<Assets<Image>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut bindposes: ResMut<Assets<SkinnedMeshInverseBindposes>>,
    mut materials: ResMut<Assets<CharacterMaterial>>,
    mut targets: ResMut<Targets>,
) {
    for (n, c) in plan.0.iter().enumerate() {
        let (frame_layer, glow_layer) = (2 * n + 1, 2 * n + 2);
        let mut parts = vec![material(c.glow, images.add(c.image()))];
        if let Some(f) = c.front {
            let flat = Case {
                width: 1,
                height: 1,
                texels: vec![f.colour],
                ..c.clone()
            };
            parts.push(material(Glow::None, images.add(flat.image())));
        }
        let glows: Vec<_> = parts.iter().map(|m| materials.add(m.glow_mask())).collect();
        let handles: Vec<_> = parts.into_iter().map(|m| materials.add(m)).collect();
        let model = quad_model(c);
        let spawned = spawn_character(
            &mut commands,
            &model,
            Transform::IDENTITY,
            0.0,
            &mut meshes,
            &mut bindposes,
            &handles,
        )
        .expect("the quad spawns");
        for e in &spawned.segments {
            commands.entity(*e).insert(RenderLayers::layer(frame_layer));
        }
        spawn_glow_masks(&mut commands, &model, &spawned, &glows, glow_layer);

        let mut target =
            Image::new_target_texture(c.width, c.height, TextureFormat::Rgba16Float, None);
        target.asset_usage = RenderAssetUsages::default();
        let handle = images.add(target);
        let projection = Projection::Orthographic(OrthographicProjection {
            scaling_mode: ScalingMode::Fixed {
                width: c.width as f32,
                height: c.height as f32,
            },
            near: 0.0,
            far: 1000.0,
            ..OrthographicProjection::default_3d()
        });
        let camera = commands
            .spawn((
                Camera3d::default(),
                Camera {
                    clear_color: ClearColorConfig::Custom(Color::LinearRgba(LinearRgba::new(
                        CLEAR[0], CLEAR[1], CLEAR[2], CLEAR[3],
                    ))),
                    ..default()
                },
                RenderTarget::Image(handle.clone().into()),
                projection.clone(),
                Transform::IDENTITY,
                Hdr,
                Tonemapping::None,
                DebandDither::Disabled,
                Msaa::Off,
                RenderLayers::layer(frame_layer),
            ))
            .id();
        spawn_glow_camera(
            &mut commands,
            &mut images,
            camera,
            UVec2::new(c.width, c.height),
            projection,
            Msaa::Off,
            glow_layer,
        );
        targets.0.push(handle);
    }
}

fn capture(
    mut commands: Commands,
    plan: Res<Plan>,
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
        // Ready once every pixel has drawn, every case with a glow shows
        // it, and the captures agree.
        let ready = plan.0.iter().zip(&shots).all(|(c, shot)| {
            let drawn = shot.iter().all(|p| *p != CLEAR);
            let frame = c.frame();
            let glowing = c.glow == Glow::None
                || shot
                    .iter()
                    .zip(&frame.texels)
                    .any(|(p, t)| (p[0] - t[0]).abs() > 4.0 * half_step(t[0]));
            drawn && glowing
        });
        let stable = previous.as_ref() == Some(&shots);
        *agreeing = if stable { *agreeing + 1 } else { 0 };
        if ready && *agreeing >= SETTLED {
            *outcome.0.lock().unwrap() = Some(Ok(shots));
            exit.write(AppExit::Success);
            return;
        }
        *previous = Some(shots);
    }
    if started.elapsed() > MAX_WAIT {
        *outcome.0.lock().unwrap() = Some(Err(format!(
            "pipelines not ready after {MAX_WAIT:?} ({} frames)",
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

fn render(cases: Arc<Vec<Case>>) -> Vec<Vec<[f32; 4]>> {
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
    ))
    .insert_resource(Plan(cases))
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
        .expect("every pipeline drew")
}

// ---- comparing ----

/// How far a case's output is from the reference: from its f32 value, in
/// half steps and as a share of the glow it adds (recorded), and from its
/// value as the GPU stores it (the bound).
#[derive(Debug, Default)]
struct Compared {
    channels: usize,
    worst_steps: f32,
    worst_at: (usize, usize, usize),
    worst_share: f32,
    stored_worst_steps: f32,
    /// Channels more than one half step from the stored reference
    /// (recorded), and more than the bound (a failure).
    past_one: usize,
    misses: usize,
    /// The glow added: the most, and the mean over RGB.
    glow_max: f32,
    glow_mean: f32,
}

/// One half step of the stored value, 2^-24 under half's normals.
fn step(stored: f32) -> f32 {
    half_step(stored).max(2f32.powi(-24))
}

/// The bound, in steps of the stored value: one for the target's own
/// store, and one more for each stored step before it (the extract and
/// the two blurs) whose truncation the GPU's f32 arithmetic can land on
/// the other side of, a flip of one step there adding at most one here
/// (the glow it carries is at most the output's).
const BOUND: f32 = 4.0;

fn compare(c: &Case, shot: &[[f32; 4]]) -> Compared {
    let want = c.want();
    let stored = c.stored();
    let frame = c.frame();
    let mut out = Compared::default();
    let mut glow_sum = 0.0f64;
    for y in 0..c.height as usize {
        for x in 0..c.width as usize {
            let i = y * c.width as usize + x;
            let (w, f, st) = (want.get(x, y), frame.get(x, y), stored.get(x, y));
            for ch in 0..3 {
                let g = w[ch] - f[ch];
                glow_sum += f64::from(g);
                out.glow_max = out.glow_max.max(g);
                let err = (shot[i][ch] - w[ch]).abs();
                let steps = err / half_step(w[ch]);
                if steps > out.worst_steps {
                    out.worst_steps = steps;
                    out.worst_at = (x, y, ch);
                }
                if g > 1e-3 {
                    out.worst_share = out.worst_share.max(err / g);
                }
                let off = (shot[i][ch] - st[ch]).abs() / step(st[ch]);
                out.stored_worst_steps = out.stored_worst_steps.max(off);
                if off > 1.0 {
                    out.past_one += 1;
                }
                if off > BOUND {
                    out.misses += 1;
                    if std::env::var_os("GUAC_LANE_DEBUG").is_some() && out.misses <= 8 {
                        eprintln!(
                            "  miss at ({x}, {y}) channel {ch}: frame {} stored {} got {} (f32 {}, glow {g})",
                            f[ch], st[ch], shot[i][ch], w[ch]
                        );
                    }
                }
                out.channels += 1;
            }
        }
    }
    out.glow_mean = (glow_sum / out.channels as f64) as f32;
    out
}

#[test]
#[ignore = "GPU lane: run on the Mac (AE.11)"]
fn the_glow_chain_matches_its_cpu_reference() {
    let cases = Arc::new(cases());
    let started = Instant::now();
    let shots = render(cases.clone());
    eprintln!(
        "rendered {} cases in {:.1?}",
        cases.len(),
        started.elapsed()
    );
    let mut misses = 0;
    for (c, shot) in cases.iter().zip(&shots) {
        let r = compare(c, shot);
        eprintln!(
            "{}: {} channels; from the f32 reference worst {:.3} half steps at {:?}, {:.2e} of the glow added; from the stored reference worst {:.3} steps, {} past one, {} past {BOUND}; glow max {:.4}, mean {:.4}",
            c.name,
            r.channels,
            r.worst_steps,
            r.worst_at,
            r.worst_share,
            r.stored_worst_steps,
            r.past_one,
            r.misses,
            r.glow_max,
            r.glow_mean,
        );
        if c.glow == Glow::None {
            assert_eq!(
                r.worst_steps, 0.0,
                "{}: no glow leaves the frame as drawn",
                c.name
            );
        } else {
            assert!(r.glow_max > 0.1, "{}: the glow shows", c.name);
        }
        misses += r.misses;
    }
    assert_eq!(
        misses, 0,
        "channels more than {BOUND} half steps from the stored reference"
    );
}

#[test]
fn the_cases_start_from_halves_and_the_reference_adds_glow_where_the_mask_is() {
    for (v, _) in [0.0f32, 1.0, 0.1, 3.0, 8.0, 6.1e-5].map(to_half) {
        assert_eq!(to_half(v).0, v, "a half stays itself");
    }
    assert_eq!(to_half(1.0).1, 0x3C00);
    assert!(to_half(0.1).0 <= 0.1 && 0.1 - to_half(0.1).0 < half_step(0.1));
    let c = case("small", 48, 27, Glow::LayerAlpha, 1);
    let (frame, want) = (c.frame(), c.want());
    assert_eq!(frame.width, 48);
    // The mask is the map's alpha: some pixels glow, and no output channel
    // drops below the frame.
    let mut lifted = 0;
    for (f, w) in frame.texels.iter().zip(&want.texels) {
        for ch in 0..3 {
            assert!(w[ch] >= f[ch] - 1e-6);
            if w[ch] > f[ch] + 1e-4 {
                lifted += 1;
            }
        }
    }
    assert!(lifted > 0);
    let none = case("none", 48, 27, Glow::None, 1);
    assert_eq!(none.want(), none.frame());
    let amount = case("amount", 48, 27, Glow::Amount(0.25), 1);
    assert!(amount.mask().texels.iter().all(|t| t[0] == 0.25));
}
