//! AP.8's title spike (openspec AP, design decision 7): the TV's titles drawn
//! through a first `TitleMaterial`, ships nothing. Each title's string renders
//! once, white, into an `Image` by its own order -1 `Camera2d` cleared to
//! `Color::NONE` (a UI root under `UiTargetCamera`); the embedded WESL fragment
//! ([`title_spike.wesl`](title_spike.wesl)) builds design_language.md §2's
//! treatment from that image's alpha over a surface laid absolutely over the
//! title's own `Text`, which stays hidden as the layout box.
//!
//! `cargo run --release -p guacamole --features title-spike --example title_spike -- <cmd>`
//! - `draw [--offscreen WxH [--scale S]] [--fullscreen] [--seconds N]
//!   [--title-scale S] [--titles N] [--slots N] [--no-plain] [--always-active]
//!   [--debug-alpha] [--out DIR --name NAME]`: the TV's background with the
//!   real titles, each through the material and then as the plain `Text` the
//!   TV draws today; in a window on the Mac's display, or offscreen at WxH
//!   (`--scale 2` is a Retina window's scale factor). `--title-scale` overrides
//!   the title cameras' `ImageRenderTarget::scale_factor`, which otherwise
//!   follows the TV camera's. Prints `check:` lines (the sizes, the cleared
//!   alpha, the inactive camera) and with `--out` saves the TV and each
//!   title's texture as PNGs, outside any checkout.
//! - `probe [--title-order N]`: offscreen; whether the TV samples a title's
//!   target the frame it renders (a colour that changes every frame, read
//!   back each frame); `--title-order 1` draws the titles after the TV, the
//!   probe's own failing case.
//! - `bench [--offscreen WxH] [--titles N] [--always-active] [--frames N]`:
//!   render frame times with the GPU waited on each frame.
//! - `headless`: `portal_app`'s plugins (no render app) with
//!   `Assets<TitleMaterial>`, then with `UiMaterialPlugin`.

use std::collections::HashMap;
use std::panic::AssertUnwindSafe;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use anyhow::{Result, bail};
use bevy::app::ScheduleRunnerPlugin;
use bevy::asset::embedded_asset;
use bevy::camera::{
    ClearColorConfig, ComputedCameraValues, ImageRenderTarget, RenderTarget, RenderTargetInfo,
};
use bevy::diagnostic::FrameCount;
use bevy::prelude::*;
use bevy::reflect::TypePath;
use bevy::render::render_resource::{AsBindGroup, PollType, ShaderType, TextureFormat};
use bevy::render::renderer::RenderDevice;
use bevy::render::view::Msaa;
use bevy::render::view::window::screenshot::{Screenshot, ScreenshotCaptured, save_to_disk};
use bevy::render::{Render, RenderApp, RenderSystems};
use bevy::shader::ShaderRef;
use bevy::text::{FontSize, LetterSpacing};
use bevy::ui::{ComputedNode, UiGlobalTransform, UiSystems, UiTargetCamera};
use bevy::window::{ExitCondition, MonitorSelection, PrimaryWindow, WindowMode};
use bevy::winit::WinitPlugin;

// The TV's own theme, as the binary has it.
#[path = "../src/theme.rs"]
#[allow(dead_code)]
mod theme;
use theme::{Role, Step, TvTheme};

static TITAN_ONE: &[u8] = include_bytes!("../../server/assets/fonts/TitanOne-Regular.ttf");

const SHADER: &str = "embedded://title_spike/title_spike.wesl";

/// The real titles: a screen title (§3.7's state titles), Join's call and
/// Found's verdict, in caps (§2: Titan One is for caps).
const TITLES: [(&str, Step); 3] = [
    ("FOUND YOUR GAMES", Step::Hero),
    ("SCAN TO PLAY", Step::Lg),
    ("SKYLANDERS: GIANTS IS READY!", Step::Lg),
];

/// §1's darker gold stops. `TvTheme` holds only what the TV draws today
/// (AP.9); AP.10 moves these in when the titles draw with them.
const GOLD_SHADOW: Color = Color::srgb_u8(0x6e, 0x4a, 0x00); // --gs
const GOLD_INNER: Color = Color::srgb_u8(0x3a, 0x25, 0x00); // --gi

/// §2's display letter spacing, at its tight end for title sizes.
const LETTER_SPACING_EM: f32 = 0.04;

/// Frames between the title cameras' one render and the settled shots.
const SETTLE_FRAMES: u32 = 60;

fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("draw") => run(Opts::parse(&args[1..])?),
        Some("probe") => {
            let mut opts = Opts::parse(&args[1..])?;
            opts.probe = true;
            opts.offscreen.get_or_insert(UVec2::new(1920, 1080));
            opts.always_active = true;
            opts.mode = 2;
            opts.slots = 1;
            opts.titles = 1;
            opts.plain = false;
            run(opts)
        }
        Some("bench") => {
            let mut opts = Opts::parse(&args[1..])?;
            opts.bench = true;
            opts.offscreen.get_or_insert(UVec2::new(1920, 1080));
            opts.slots = 2;
            opts.plain = false;
            run(opts)
        }
        Some("headless") => headless(),
        _ => bail!(
            "usage: title_spike draw [--offscreen WxH [--scale S]] [--fullscreen] [--seconds N] [--title-scale S] [--titles N] [--slots N] [--no-plain] [--always-active] [--debug-alpha] [--out DIR --name NAME] | probe [--title-order N] | bench [--offscreen WxH] [--titles N] [--always-active] [--frames N] | headless"
        ),
    }
}

#[derive(Clone, Debug)]
struct Opts {
    /// The TV's camera renders into an image this size instead of a window.
    offscreen: Option<UVec2>,
    /// The offscreen target's scale factor (a Retina window's is 2).
    scale: f32,
    fullscreen: bool,
    /// Closes the window after this long.
    seconds: Option<f32>,
    /// The title cameras' target scale factor; `None` follows the TV camera's.
    title_scale: Option<f32>,
    /// The title cameras' order: -1, or 1 to show `probe` failing.
    title_order: isize,
    /// How many of the slots draw through the material (the rest plain).
    titles: usize,
    /// How many of [`TITLES`] are on screen.
    slots: usize,
    /// Each title again as plain `Text`, under it.
    plain: bool,
    always_active: bool,
    /// The shader's mode: 0 the treatment, 1 coverage as grey, 2 raw colour.
    mode: u32,
    out: Option<PathBuf>,
    name: String,
    probe: bool,
    bench: bool,
    frames: u32,
}

impl Opts {
    fn parse(args: &[String]) -> Result<Self> {
        let mut opts = Self {
            offscreen: None,
            scale: 1.0,
            fullscreen: false,
            seconds: None,
            title_scale: None,
            title_order: -1,
            titles: TITLES.len(),
            slots: TITLES.len(),
            plain: true,
            always_active: false,
            mode: 0,
            out: None,
            name: "titles".into(),
            probe: false,
            bench: false,
            frames: 1320,
        };
        let mut it = args.iter();
        while let Some(arg) = it.next() {
            let mut value = || {
                it.next()
                    .ok_or_else(|| anyhow::anyhow!("{arg} needs a value"))
            };
            match arg.as_str() {
                "--offscreen" => {
                    let v = value()?;
                    let (w, h) = v
                        .split_once('x')
                        .ok_or_else(|| anyhow::anyhow!("--offscreen WxH"))?;
                    opts.offscreen = Some(UVec2::new(w.parse()?, h.parse()?));
                }
                "--scale" => opts.scale = value()?.parse()?,
                "--fullscreen" => opts.fullscreen = true,
                "--seconds" => opts.seconds = Some(value()?.parse()?),
                "--title-scale" => opts.title_scale = Some(value()?.parse()?),
                "--title-order" => opts.title_order = value()?.parse()?,
                "--titles" => opts.titles = value()?.parse()?,
                "--slots" => opts.slots = value()?.parse::<usize>()?.min(TITLES.len()),
                "--no-plain" => opts.plain = false,
                "--always-active" => opts.always_active = true,
                "--debug-alpha" => opts.mode = 1,
                "--out" => opts.out = Some(outside_checkouts(value()?.into())?),
                "--name" => opts.name = value()?.clone(),
                "--frames" => opts.frames = value()?.parse()?,
                other => bail!("unknown flag {other}"),
            }
        }
        opts.titles = opts.titles.min(opts.slots);
        Ok(opts)
    }
}

/// PNGs of game-free UI, but the shots rule stands: never into a checkout.
fn outside_checkouts(dir: PathBuf) -> Result<PathBuf> {
    let dir = std::path::absolute(&dir)?;
    if let Some(repo) = guac_extract::root::enclosing_git_tree(&dir) {
        bail!(
            "{} is inside the work tree {}",
            dir.display(),
            repo.display()
        );
    }
    std::fs::create_dir_all(&dir)?;
    Ok(dir)
}

// ---- the material -----------------------------------------------------------

#[derive(ShaderType, Clone, Debug)]
struct TitleParams {
    fill_top: LinearRgba,
    fill_bottom: LinearRgba,
    stroke: LinearRgba,
    shadow_near: LinearRgba,
    shadow_far: LinearRgba,
    drop: LinearRgba,
    glow: LinearRgba,
    /// Physical px per design px: the TV camera's scale factor × `UiScale`.
    scale: f32,
    margin: f32,
    stroke_width: f32,
    drop_offset: f32,
    drop_sigma: f32,
    glow_sigma: f32,
    mode: u32,
}

#[derive(Asset, TypePath, AsBindGroup, Clone, Debug)]
struct TitleMaterial {
    #[uniform(0)]
    params: TitleParams,
    /// The title camera's target; `None` (white) only until it exists, and
    /// the surface stays hidden until then.
    #[texture(1)]
    #[sampler(2)]
    glyphs: Option<Handle<Image>>,
}

impl UiMaterial for TitleMaterial {
    fn fragment_shader() -> ShaderRef {
        SHADER.into()
    }
}

/// Per size: the stroke (§2's 1-2 px by size), the glow's spread (§2's
/// 20-40 px blur, as a Gaussian sigma of half that) and the margin the
/// surface grows past the layout box, enough for the glow's 2.5 sigma.
#[derive(Clone, Copy)]
struct Treatment {
    stroke: f32,
    glow_sigma: f32,
    margin: f32,
}

fn treatment(step: Step) -> Treatment {
    match step {
        Step::Hero => Treatment {
            stroke: 2.0,
            glow_sigma: 15.0,
            margin: 40.0,
        },
        _ => Treatment {
            stroke: 1.5,
            glow_sigma: 10.0,
            margin: 28.0,
        },
    }
}

fn params(theme: &TvTheme, step: Step, scale: f32, mode: u32) -> TitleParams {
    let t = treatment(step);
    TitleParams {
        fill_top: theme.role(Role::Accent).to_linear(),
        fill_bottom: theme.gold.to_linear(),
        stroke: GOLD_INNER.to_linear(),
        shadow_near: GOLD_SHADOW.to_linear(),
        shadow_far: GOLD_INNER.to_linear(),
        drop: Color::srgba(0.0, 0.0, 0.0, 0.5).to_linear(),
        glow: theme.gold.with_alpha(0.3).to_linear(),
        scale,
        margin: t.margin,
        stroke_width: t.stroke,
        drop_offset: 5.0,
        drop_sigma: 5.0,
        glow_sigma: t.glow_sigma,
        mode,
    }
}

// ---- the app ----------------------------------------------------------------

#[derive(Resource)]
struct Spike {
    opts: Opts,
    theme: TvTheme,
    font: Handle<Font>,
    /// The TV camera's target when offscreen.
    target: Option<Handle<Image>>,
}

#[derive(Component)]
struct TvCamera;

/// A title drawn through the material: the surface over its hidden `Text`.
#[derive(Component)]
struct TitleSurface {
    index: usize,
    text: String,
    step: Step,
    layout: Entity,
    material: Handle<TitleMaterial>,
    drawn: Option<Drawn>,
}

struct Drawn {
    camera: Entity,
    root: Entity,
    ink: Entity,
    image: Handle<Image>,
    size: UVec2,
    /// The frame the ink's layout landed, which is the frame its camera
    /// draws the glyphs.
    laid_out: Option<u32>,
}

/// Screenshots by name, filled in by their observers.
#[derive(Resource, Default)]
struct Record {
    shots: Arc<Mutex<HashMap<String, Image>>>,
    requested: usize,
    /// The frame every title's camera drew its glyphs.
    drawn_at: Option<u32>,
    reported: bool,
    /// Surface rects (physical px, min and max) and their cameras' targets,
    /// by title.
    surfaces: Vec<(Rect, ImageRenderTarget)>,
    sizes: Vec<String>,
    probe: Arc<Mutex<Vec<(u32, u8, u8)>>>,
}

/// Render-world instants, one per frame after the GPU finished it.
#[derive(Resource, Clone, Default)]
struct RenderTicks(Arc<Mutex<Vec<Instant>>>);

fn run(opts: Opts) -> Result<()> {
    let mut app = App::new();
    match opts.offscreen {
        None => {
            let window = if opts.fullscreen {
                Window {
                    title: "title_spike".into(),
                    mode: WindowMode::BorderlessFullscreen(MonitorSelection::Primary),
                    ..default()
                }
            } else {
                Window {
                    title: "title_spike".into(),
                    resolution: (1600, 900).into(),
                    ..default()
                }
            };
            app.add_plugins(DefaultPlugins.set(WindowPlugin {
                primary_window: Some(window),
                ..default()
            }));
        }
        Some(_) => {
            app.add_plugins(
                DefaultPlugins
                    .set(WindowPlugin {
                        primary_window: None,
                        exit_condition: ExitCondition::DontExit,
                        ..default()
                    })
                    .disable::<WinitPlugin>(),
            )
            .add_plugins(ScheduleRunnerPlugin::run_loop(Duration::ZERO));
        }
    }
    embedded_asset!(app, "examples", "title_spike.wesl");
    let ticks = RenderTicks::default();
    app.add_plugins(UiMaterialPlugin::<TitleMaterial>::default())
        .insert_resource(ClearColor(TvTheme::default().background()))
        .insert_resource(SpikeOpts(opts.clone()))
        .init_resource::<Record>()
        .add_systems(Startup, setup)
        .add_systems(
            Update,
            (
                scale_ui,
                retire_cameras,
                probe_tick,
                close_after,
                bench_stop,
            ),
        )
        .add_systems(
            PostUpdate,
            (draw_titles, checks).chain().after(UiSystems::PostLayout),
        );
    if opts.bench {
        app.sub_app_mut(RenderApp)
            .insert_resource(ticks.clone())
            .add_systems(Render, gpu_sync.in_set(RenderSystems::Cleanup));
    }
    app.run();
    if opts.bench {
        report_bench(&opts, &ticks);
    }
    Ok(())
}

#[derive(Resource)]
struct SpikeOpts(Opts);

fn setup(
    mut commands: Commands,
    opts: Res<SpikeOpts>,
    mut fonts: ResMut<Assets<Font>>,
    mut images: ResMut<Assets<Image>>,
    mut materials: ResMut<Assets<TitleMaterial>>,
) {
    let opts = opts.0.clone();
    let theme = TvTheme::default();
    let font = fonts.add(Font::from_bytes(TITAN_ONE.to_vec()));
    let target = opts.offscreen.map(|size| {
        images.add(Image::new_target_texture(
            size.x,
            size.y,
            TextureFormat::Rgba8UnormSrgb,
            None,
        ))
    });
    let mut camera = commands.spawn((Camera2d, TvCamera, IsDefaultUiCamera));
    if let Some(target) = &target {
        camera.insert(RenderTarget::Image(ImageRenderTarget {
            handle: target.clone(),
            scale_factor: opts.scale,
        }));
    }
    let spike = Spike {
        opts,
        theme,
        font,
        target,
    };
    let inset = 6.0;
    commands
        .spawn((
            Node {
                width: percent(100),
                height: percent(100),
                padding: UiRect::axes(vw(inset), vh(inset)),
                flex_direction: FlexDirection::Column,
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                row_gap: px(12),
                ..default()
            },
            BackgroundColor(spike.theme.background()),
        ))
        .with_children(|root| {
            if spike.opts.plain {
                root.spawn((
                    text_bundle(
                        &spike,
                        "AP.8: each title through TitleMaterial, then as plain Text",
                        Step::Caption,
                        spike.theme.role(Role::Muted),
                    ),
                    Node {
                        margin: UiRect::bottom(px(24)),
                        ..default()
                    },
                ));
            }
            for (index, (text, step)) in TITLES.iter().take(spike.opts.slots).enumerate() {
                if index < spike.opts.titles {
                    spawn_title(root, &spike, &mut materials, index, text, *step);
                } else {
                    root.spawn(text_bundle(
                        &spike,
                        text,
                        *step,
                        spike.theme.role(Role::Accent),
                    ));
                }
                if spike.opts.plain {
                    root.spawn((
                        text_bundle(&spike, text, *step, spike.theme.role(Role::Accent)),
                        Node {
                            margin: UiRect::bottom(px(28)),
                            ..default()
                        },
                    ));
                }
            }
        });
    commands.insert_resource(spike);
}

fn text_bundle(spike: &Spike, s: &str, step: Step, color: Color) -> impl Bundle + use<> {
    let size = spike.theme.size(step);
    (
        Text::new(s),
        TextFont {
            font: spike.font.clone().into(),
            font_size: FontSize::Px(size),
            ..default()
        },
        TextColor(color),
        TextLayout::new(Justify::Center, LineBreak::NoWrap),
        LetterSpacing::Px(size * LETTER_SPACING_EM),
    )
}

/// A wrapper laid out by the title's own `Text`, hidden, with the surface
/// over it grown by the treatment's margin.
fn spawn_title(
    parent: &mut ChildSpawnerCommands,
    spike: &Spike,
    materials: &mut Assets<TitleMaterial>,
    index: usize,
    text: &str,
    step: Step,
) {
    let t = treatment(step);
    let material = materials.add(TitleMaterial {
        params: params(&spike.theme, step, 1.0, spike.opts.mode),
        glyphs: None,
    });
    parent.spawn(Node::default()).with_children(|wrap| {
        let layout = wrap
            .spawn((
                text_bundle(spike, text, step, spike.theme.role(Role::Accent)),
                Visibility::Hidden,
            ))
            .id();
        wrap.spawn((
            Node {
                position_type: PositionType::Absolute,
                left: px(-t.margin),
                right: px(-t.margin),
                top: px(-t.margin),
                bottom: px(-t.margin),
                ..default()
            },
            MaterialNode(material.clone()),
            Visibility::Hidden,
            TitleSurface {
                index,
                text: text.into(),
                step,
                layout,
                material,
                drawn: None,
            },
        ));
    });
}

/// As the TV's `scale_ui`: 1080 design px to the target's logical height.
fn scale_ui(camera: Query<&Camera, With<TvCamera>>, mut scale: ResMut<UiScale>) {
    let Some(size) = camera.single().ok().and_then(Camera::logical_target_size) else {
        return;
    };
    let wanted = size.y / 1080.0;
    if size.y > 0.0 && (scale.0 - wanted).abs() > f32::EPSILON {
        scale.0 = wanted;
    }
}

/// Renders each title's string into its own image once its surface has a
/// size: a camera, a UI root on it and the white ink, the material pointed at
/// the image.
#[allow(clippy::too_many_arguments)]
fn draw_titles(
    mut commands: Commands,
    spike: Res<Spike>,
    frame: Res<FrameCount>,
    ui_scale: Res<UiScale>,
    tv: Query<&Camera, With<TvCamera>>,
    mut surfaces: Query<(
        &mut TitleSurface,
        &ComputedNode,
        &UiGlobalTransform,
        &mut Visibility,
    )>,
    nodes: Query<&ComputedNode, Without<TitleSurface>>,
    mut images: ResMut<Assets<Image>>,
    mut materials: ResMut<Assets<TitleMaterial>>,
    mut record: ResMut<Record>,
) {
    let Some(tv_scale) = tv.single().ok().and_then(Camera::target_scaling_factor) else {
        return;
    };
    let title_scale = spike.opts.title_scale.unwrap_or(tv_scale);
    for (mut surface, node, transform, mut visibility) in &mut surfaces {
        let size = node.size();
        if size.x < 1.0 || size.y < 1.0 {
            continue;
        }
        let want = size.ceil().as_uvec2();
        if surface.drawn.as_ref().is_none_or(|d| d.size != want) {
            if let Some(old) = surface.drawn.take() {
                commands.entity(old.camera).despawn();
                commands.entity(old.root).despawn();
            }
            let image = images.add(Image::new_target_texture(
                want.x,
                want.y,
                TextureFormat::Rgba8UnormSrgb,
                None,
            ));
            let camera = commands
                .spawn((
                    Camera2d,
                    Camera {
                        order: spike.opts.title_order,
                        clear_color: ClearColorConfig::Custom(Color::NONE),
                        ..default()
                    },
                    RenderTarget::Image(ImageRenderTarget {
                        handle: image.clone(),
                        scale_factor: title_scale,
                    }),
                    Msaa::Off,
                ))
                .id();
            let root = commands
                .spawn((
                    Node {
                        width: percent(100),
                        height: percent(100),
                        justify_content: JustifyContent::Center,
                        align_items: AlignItems::Center,
                        ..default()
                    },
                    UiTargetCamera(camera),
                ))
                .id();
            let ink = commands
                .spawn((
                    text_bundle(&spike, &surface.text, surface.step, Color::WHITE),
                    ChildOf(root),
                ))
                .id();
            if let Some(mut material) = materials.get_mut(&surface.material) {
                material.params = params(
                    &spike.theme,
                    surface.step,
                    tv_scale * ui_scale.0,
                    spike.opts.mode,
                );
                material.glyphs = Some(image.clone());
            }
            *visibility = Visibility::Inherited;
            info!(
                title = surface.text,
                frame = frame.0,
                size = %want,
                "title camera spawned"
            );
            surface.drawn = Some(Drawn {
                camera,
                root,
                ink,
                image,
                size: want,
                laid_out: None,
            });
        }
        let center = transform.translation;
        let rect = Rect::from_center_size(center, size);
        let index = surface.index;
        let layout_size = nodes.get(surface.layout).map(|n| n.size()).ok();
        let text = surface.text.clone();
        let drawn = surface.drawn.as_mut().expect("drawn above");
        if drawn.laid_out.is_none()
            && let Ok(ink) = nodes.get(drawn.ink)
            && ink.size().x > 0.0
        {
            drawn.laid_out = Some(frame.0);
            let layout = layout_size.unwrap_or_default();
            let line = format!(
                "size {index} {text:?}: layout Text {:.0}x{:.0} px, ink {:.0}x{:.0} px (ratio {:.3}); TV target scale {tv_scale}, title target scale {title_scale}, UiScale {:.3}",
                layout.x,
                layout.y,
                ink.size().x,
                ink.size().y,
                ink.size().y / layout.y.max(1.0),
                ui_scale.0,
            );
            println!("check: {line}");
            record.sizes.push(line);
            if record.surfaces.len() <= index {
                record
                    .surfaces
                    .resize(index + 1, (Rect::default(), Handle::default().into()));
            }
            record.surfaces[index] = (
                rect,
                ImageRenderTarget {
                    handle: drawn.image.clone(),
                    scale_factor: title_scale,
                },
            );
        }
    }
}

/// The title cameras go inactive the frame after they drew their glyphs.
fn retire_cameras(
    spike: Option<Res<Spike>>,
    frame: Res<FrameCount>,
    surfaces: Query<&TitleSurface>,
    mut cameras: Query<&mut Camera, Without<TvCamera>>,
) {
    let Some(spike) = spike else {
        return;
    };
    if spike.opts.always_active {
        return;
    }
    for surface in &surfaces {
        let Some(drawn) = &surface.drawn else {
            continue;
        };
        if drawn.laid_out.is_some_and(|f| f < frame.0)
            && let Ok(mut camera) = cameras.get_mut(drawn.camera)
            && camera.is_active
        {
            camera.is_active = false;
            info!(
                title = surface.text,
                frame = frame.0,
                "title camera inactive"
            );
        }
    }
}

/// Screenshots: every title texture the frame its camera draws (a
/// `Screenshot` of an image target captures what a camera renders into it,
/// so one taken while the camera is inactive comes back empty), then the TV
/// two frames later (the cameras inactive since the frame after they drew)
/// and [`SETTLE_FRAMES`] later. A texture screenshot reroutes its camera's
/// output through a capture texture copied back after the frame, so the TV
/// doesn't show the titles the frame they draw here; `probe` checks that
/// without screenshots of the textures.
#[allow(clippy::too_many_arguments)]
fn checks(
    mut commands: Commands,
    spike: Res<Spike>,
    frame: Res<FrameCount>,
    surfaces: Query<&TitleSurface>,
    mut record: ResMut<Record>,
    window: Query<(), With<PrimaryWindow>>,
    mut exit: MessageWriter<AppExit>,
) {
    if spike.opts.bench || spike.opts.probe {
        return;
    }
    let all_drawn = surfaces
        .iter()
        .map(|s| s.drawn.as_ref().and_then(|d| d.laid_out))
        .collect::<Option<Vec<u32>>>();
    let windowed = !window.is_empty();
    match record.drawn_at {
        None => {
            let Some(at) = all_drawn else {
                return;
            };
            // No titles at all: wait for the plain text's glyphs anyway.
            if at.is_empty() && frame.0 < 30 {
                return;
            }
            record.drawn_at = Some(frame.0);
            println!(
                "check: frame {}: {} title(s) laid out, drawn by their cameras this frame",
                frame.0,
                at.len()
            );
            snapshot_textures(&mut commands, &spike, &mut record);
        }
        Some(at) if frame.0 == at + 2 => {
            snapshot_tv(&mut commands, &spike, &mut record, "early", windowed);
        }
        Some(at) if frame.0 == at + SETTLE_FRAMES => {
            snapshot_tv(&mut commands, &spike, &mut record, "settled", windowed);
        }
        Some(at) => {
            let done = frame.0 > at + SETTLE_FRAMES
                && record.shots.lock().unwrap().len() >= record.requested;
            if done && !record.reported {
                record.reported = true;
                report(&spike, &record);
                if spike.opts.offscreen.is_some() {
                    exit.write(AppExit::Success);
                }
            } else if frame.0 > at + SETTLE_FRAMES + 600 && !record.reported {
                println!(
                    "check: screenshots incomplete ({} of {}): window frames not presented?",
                    record.shots.lock().unwrap().len(),
                    record.requested
                );
                record.reported = true;
                if spike.opts.offscreen.is_some() {
                    exit.write(AppExit::from_code(1));
                }
            }
        }
    }
}

fn snapshot_textures(commands: &mut Commands, spike: &Spike, record: &mut Record) {
    let shots: Vec<_> = record
        .surfaces
        .iter()
        .enumerate()
        .map(|(index, (_, image))| {
            let path = spike
                .opts
                .out
                .as_ref()
                .map(|dir| dir.join(format!("{}-title{index}-texture.png", spike.opts.name)));
            (format!("title{index}"), shot_of(image.clone()), path)
        })
        .collect();
    take(commands, record, shots);
}

fn snapshot_tv(
    commands: &mut Commands,
    spike: &Spike,
    record: &mut Record,
    when: &str,
    windowed: bool,
) {
    let shot = match (&spike.target, windowed) {
        (Some(target), _) => shot_of(ImageRenderTarget {
            handle: target.clone(),
            scale_factor: spike.opts.scale,
        }),
        (None, true) => Screenshot::primary_window(),
        (None, false) => return,
    };
    let path = spike
        .opts
        .out
        .as_ref()
        .filter(|_| when == "settled")
        .map(|dir| dir.join(format!("{}.png", spike.opts.name)));
    take(commands, record, vec![(format!("tv-{when}"), shot, path)]);
}

fn take(
    commands: &mut Commands,
    record: &mut Record,
    shots: Vec<(String, Screenshot, Option<PathBuf>)>,
) {
    for (name, shot, path) in shots {
        record.requested += 1;
        let store = record.shots.clone();
        let mut entity = commands.spawn(shot);
        entity.observe(move |captured: On<ScreenshotCaptured>| {
            store
                .lock()
                .unwrap()
                .insert(name.clone(), captured.image.clone());
        });
        if let Some(path) = path {
            println!("check: writing {}", path.display());
            entity.observe(save_to_disk(path));
        }
    }
}

/// A screenshot of an image target. Not `Screenshot::image`: that names the
/// target at scale factor 1, and a target is keyed by handle AND scale
/// factor, so a camera drawing at 2 never draws into its capture.
fn shot_of(target: ImageRenderTarget) -> Screenshot {
    Screenshot(RenderTarget::Image(target))
}

fn rgba(image: &Image, x: u32, y: u32) -> [u8; 4] {
    let c = image
        .get_color_at(x, y)
        .map(Srgba::from)
        .unwrap_or(Srgba::NONE);
    [c.red, c.green, c.blue, c.alpha].map(|v| (v * 255.0).round() as u8)
}

fn hex(p: [u8; 4]) -> String {
    format!("#{:02x}{:02x}{:02x}/{:02x}", p[0], p[1], p[2], p[3])
}

fn report(spike: &Spike, record: &Record) {
    let shots = record.shots.lock().unwrap();
    for (index, (rect, _)) in record.surfaces.iter().enumerate() {
        if let Some(texture) = shots.get(&format!("title{index}")) {
            let (w, h) = (texture.width(), texture.height());
            let mut zero = 0u64;
            let mut max_a = 0u8;
            let mut border_max = 0u8;
            for y in 0..h {
                for x in 0..w {
                    let a = rgba(texture, x, y)[3];
                    zero += u64::from(a == 0);
                    max_a = max_a.max(a);
                    if x == 0 || y == 0 || x == w - 1 || y == h - 1 {
                        border_max = border_max.max(a);
                    }
                }
            }
            println!(
                "check: alpha {index}: texture {w}x{h}, border alpha max {border_max}, alpha 0 on {:.1}% of texels, max alpha {max_a}",
                100.0 * zero as f64 / f64::from(w * h)
            );
        }
        let (Some(early), Some(settled)) = (shots.get("tv-early"), shots.get("tv-settled")) else {
            continue;
        };
        let (x0, y0) = (rect.min.x.ceil() as u32 + 1, rect.min.y.ceil() as u32 + 1);
        let corner = rgba(settled, x0, y0);
        let background = Srgba::from(spike.theme.background());
        let bg = [background.red, background.green, background.blue, 1.0]
            .map(|v| (v * 255.0).round() as u8);
        let mut differ = 0u32;
        let mut lit = 0u32;
        let mut max_grey = 0u8;
        for y in rect.min.y.max(0.0) as u32..(rect.max.y as u32).min(settled.height()) {
            for x in rect.min.x.max(0.0) as u32..(rect.max.x as u32).min(settled.width()) {
                let e = rgba(early, x, y);
                let s = rgba(settled, x, y);
                differ += u32::from(e != s);
                lit += u32::from(s != bg);
                max_grey = max_grey.max(s[0]);
            }
        }
        if spike.opts.mode == 1 {
            println!(
                "check: sampler {index}: the TV's surface corner reads grey {} (the sampled alpha), the brightest {max_grey}",
                corner[0]
            );
        } else {
            println!(
                "check: sampler {index}: the TV's surface corner {} vs the background {}; {lit} px of the surface differ from the background",
                hex(corner),
                hex(bg)
            );
        }
        println!(
            "check: inactive {index}: the TV's surface 1 frame after its camera went inactive vs {} frames after: {differ} px differ, {lit} px drawn",
            SETTLE_FRAMES - 1
        );
    }
}

// ---- the same-frame probe ---------------------------------------------------

/// The probe's colour for a frame: its red channel names the frame.
fn code(frame: u32) -> u8 {
    20 + (frame % 200) as u8
}

/// Each frame, every title's ink root takes the frame's colour, and for
/// twenty frames the TV is read back where the surface (raw mode) shows it.
fn probe_tick(
    mut commands: Commands,
    spike: Option<Res<Spike>>,
    frame: Res<FrameCount>,
    surfaces: Query<&TitleSurface>,
    mut roots: Query<&mut BackgroundColor>,
    record: Res<Record>,
    mut exit: MessageWriter<AppExit>,
) {
    let Some(spike) = spike else {
        return;
    };
    if !spike.opts.probe {
        return;
    }
    let mut start = None;
    for surface in &surfaces {
        let Some(drawn) = &surface.drawn else {
            continue;
        };
        if let Ok(mut bg) = roots.get_mut(drawn.root) {
            bg.0 = Color::srgb_u8(code(frame.0), 0, 0);
        } else {
            commands
                .entity(drawn.root)
                .insert(BackgroundColor(Color::srgb_u8(code(frame.0), 0, 0)));
        }
        start = drawn.laid_out;
    }
    let (Some(start), Some(target), Some((rect, _))) = (
        start,
        spike.target.clone(),
        record.surfaces.first().cloned(),
    ) else {
        return;
    };
    let first = start + 10;
    if (first..first + 20).contains(&frame.0) {
        let probe = record.probe.clone();
        let at = frame.0;
        let center = rect.center().as_uvec2();
        commands
            .spawn(shot_of(ImageRenderTarget {
                handle: target,
                scale_factor: spike.opts.scale,
            }))
            .observe(move |captured: On<ScreenshotCaptured>| {
                let red = rgba(&captured.image, center.x, center.y)[0];
                probe.lock().unwrap().push((at, code(at), red));
            });
    }
    let results = record.probe.lock().unwrap();
    if results.len() >= 20 || frame.0 > first + 600 {
        let mut same = 0;
        for (at, want, got) in results.iter() {
            let lag = (0..4).find(|k| code(at.saturating_sub(*k)) == *got);
            println!("probe: frame {at}: wrote {want}, the TV read {got} (lag {lag:?})");
            same += u32::from(lag == Some(0));
        }
        println!(
            "check: same-frame: {same} of {} readbacks show the colour their own frame wrote",
            results.len()
        );
        exit.write(AppExit::Success);
    }
}

// ---- timing -----------------------------------------------------------------

fn gpu_sync(device: Res<RenderDevice>, ticks: Res<RenderTicks>) {
    let _ = device.poll(PollType::wait_indefinitely());
    ticks.0.lock().unwrap().push(Instant::now());
}

fn bench_stop(spike: Option<Res<Spike>>, frame: Res<FrameCount>, mut exit: MessageWriter<AppExit>) {
    if spike.is_some_and(|s| s.opts.bench && frame.0 >= s.opts.frames) {
        exit.write(AppExit::Success);
    }
}

fn report_bench(opts: &Opts, ticks: &RenderTicks) {
    let ticks = ticks.0.lock().unwrap();
    // Past startup, pipeline compiles and the title cameras' first frames.
    let skip = 120.min(ticks.len());
    let mut ms: Vec<f64> = ticks[skip..]
        .windows(2)
        .map(|w| (w[1] - w[0]).as_secs_f64() * 1e3)
        .collect();
    if ms.is_empty() {
        println!("bench: no frames");
        return;
    }
    let mean = ms.iter().sum::<f64>() / ms.len() as f64;
    ms.sort_by(f64::total_cmp);
    let at = |q: f64| ms[((ms.len() - 1) as f64 * q).round() as usize];
    let size = opts.offscreen.unwrap_or_default();
    println!(
        "bench: {}x{} scale {}, {} shader title(s), cameras {}: render frame median {:.3} ms, p95 {:.3} ms, mean {:.3} ms over {} frames",
        size.x,
        size.y,
        opts.scale,
        opts.titles,
        if opts.always_active {
            "always active"
        } else {
            "inactive after one draw"
        },
        at(0.5),
        at(0.95),
        mean,
        ms.len()
    );
}

fn close_after(spike: Option<Res<Spike>>, time: Res<Time<Real>>, mut exit: MessageWriter<AppExit>) {
    if let Some(seconds) = spike.and_then(|s| s.opts.seconds)
        && time.elapsed_secs() > seconds
    {
        exit.write(AppExit::Success);
    }
}

// ---- headless ---------------------------------------------------------------

/// `portal_app`'s plugin set (crates/guacamole/src/tv.rs), minus guacamole's
/// own, with a camera that has a target size and no render app.
fn portal_like_app() -> App {
    let mut app = App::new();
    app.add_plugins((
        MinimalPlugins,
        bevy::asset::AssetPlugin::default(),
        bevy::image::ImagePlugin::default(),
        bevy::image::TextureAtlasPlugin,
        bevy::input::InputPlugin,
        WindowPlugin {
            primary_window: None,
            exit_condition: ExitCondition::DontExit,
            ..default()
        },
        bevy::transform::TransformPlugin,
        bevy::text::TextPlugin,
        bevy::picking::DefaultPickingPlugins,
        bevy::ui::UiPlugin,
        bevy::state::app::StatesPlugin,
        bevy::input_focus::InputFocusPlugin,
        bevy::ui_widgets::ButtonPlugin,
    ));
    app.insert_resource(UiScale(1.0));
    app.world_mut().spawn((
        Camera2d,
        Camera {
            computed: ComputedCameraValues {
                target_info: Some(RenderTargetInfo {
                    physical_size: UVec2::new(1920, 1080),
                    scale_factor: 1.0,
                }),
                ..default()
            },
            ..default()
        },
    ));
    app
}

/// Spawns one title the way the spike does and lays it out three frames.
fn headless_title(app: &mut App) -> (Vec2, Vec2) {
    let font = app
        .world_mut()
        .resource_mut::<Assets<Font>>()
        .add(Font::from_bytes(TITAN_ONE.to_vec()));
    let theme = TvTheme::default();
    let material = app
        .world_mut()
        .resource_mut::<Assets<TitleMaterial>>()
        .add(TitleMaterial {
            params: params(&theme, Step::Hero, 1.0, 0),
            glyphs: None,
        });
    let margin = treatment(Step::Hero).margin;
    let text = app
        .world_mut()
        .spawn((
            Text::new("FOUND YOUR GAMES"),
            TextFont {
                font: font.into(),
                font_size: FontSize::Px(theme.size(Step::Hero)),
                ..default()
            },
            Visibility::Hidden,
        ))
        .id();
    let surface = app
        .world_mut()
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                left: px(-margin),
                right: px(-margin),
                top: px(-margin),
                bottom: px(-margin),
                ..default()
            },
            MaterialNode(material),
        ))
        .id();
    app.world_mut()
        .spawn(Node::default())
        .add_children(&[text, surface]);
    for _ in 0..3 {
        app.update();
    }
    let size = |e: Entity| app.world().get::<ComputedNode>(e).map(ComputedNode::size);
    (
        size(text).unwrap_or_default(),
        size(surface).unwrap_or_default(),
    )
}

fn headless() -> Result<()> {
    let mut app = portal_like_app();
    app.init_asset::<TitleMaterial>();
    let (text, surface) = headless_title(&mut app);
    println!(
        "check: headless, Assets<TitleMaterial> by init_asset: text {:.0}x{:.0}, surface {:.0}x{:.0} (text + 2 x {} margin: {})",
        text.x,
        text.y,
        surface.x,
        surface.y,
        treatment(Step::Hero).margin,
        surface == text + 2.0 * treatment(Step::Hero).margin
    );
    for with_shaders in [false, true] {
        let plugin = std::panic::catch_unwind(AssertUnwindSafe(|| {
            let mut app = portal_like_app();
            if with_shaders {
                app.init_asset::<bevy::shader::Shader>();
            }
            app.add_plugins(UiMaterialPlugin::<TitleMaterial>::default());
            headless_title(&mut app)
        }));
        let how = if with_shaders {
            "UiMaterialPlugin::<TitleMaterial> after init_asset::<Shader>()"
        } else {
            "UiMaterialPlugin::<TitleMaterial>"
        };
        match plugin {
            Ok((text, surface)) => println!(
                "check: headless, {how} with no render app: builds and lays out, text {:.0}x{:.0}, surface {:.0}x{:.0}",
                text.x, text.y, surface.x, surface.y
            ),
            Err(e) => {
                let msg = e
                    .downcast_ref::<String>()
                    .cloned()
                    .or_else(|| e.downcast_ref::<&str>().map(|s| s.to_string()))
                    .unwrap_or_default();
                println!("check: headless, {how} with no render app: panics: {msg}");
            }
        }
    }
    Ok(())
}
