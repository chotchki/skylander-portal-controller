//! The start's pass (openspec AE.25, AG.42, AG.43; the figure-turntable
//! spec's "A start gets the graphics ready first"). macOS compiles a
//! render pipeline inside the frame that first draws with it and keeps it in
//! a per-user cache, so the first time a Mac draws the characters, the cards
//! and chapter 1 costs frames of hundreds of milliseconds, and AE.14 and
//! AG.22 met those under a portal already up, in an extraction and a level
//! load. chotchki on AE.25 (2026-10-19): "With the emulator it takes a long
//! time to first start due to shader complile, its fine to do that once at
//! startup." So before the TV's first interactive screen, and before any
//! extraction (a pick starts one only once the portal shows, which waits on
//! this), the TV compiles everything it will draw Giants with, under a
//! screen that says so:
//!
//! - the census's keys ([`guacamole::census`]): the 57 the roster's figures
//!   draw on a card (33 on skinned meshes, 24 on the static parts they
//!   carry), each with its glow pass, and chapter 1's 55, through the warm
//!   ([`crate::warm`]) on its hidden stage, with its camera's own
//!   pipelines (the remaster's main pass, tonemapping, the glow chain);
//! - a card's UI pipelines: a card drawn offscreen by a camera of its own
//!   ([`crate::card_view::spawn_sample_card`]);
//! - the level camera's output to the TV's target, with the write the level
//!   draws with ([`crate::level::view_camera`]), under the TV's own as the
//!   level is, so the TV writes over it as it does over the level
//!   ([`crate::level::TV_OVER`], AG.50, the write its overlays reach the TV
//!   by) and the pass shows nothing of it;
//! - then, those extras gone, the TV's own output alone for
//!   [`EXTRA_FRAMES`] frames, the write it takes once nothing sits under it
//!   (AE.30).
//!
//! Every start runs it. On a warm cache it lasts as long as the cache takes
//! to answer (AE.25 has the numbers), so no marker in settings decides it,
//! and a Mac whose shared cache let guacamole's pipelines go (other apps'
//! compiles, AE.25's first probe after a night of RPCS3), or a build whose
//! shaders changed, compiles them again here rather than under the portal.
//! It draws up to [`MAX_BATCH`] keys a frame while frames stay quick, and
//! one a frame once one runs long (a cold cache), so the screen keeps
//! moving between compiles.
//!
//! Without a renderer (the headless tests) there's nothing to compile and
//! nothing to show: the pass is done before the first frame.

use std::sync::Arc;
use std::time::{Duration, Instant};

use bevy::asset::RenderAssetUsages;
use bevy::camera::visibility::RenderLayers;
use bevy::camera::{ClearColorConfig, ImageRenderTarget, RenderTarget};
use bevy::prelude::*;
use bevy::render::RenderApp;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};
use guac_ir::shading::Scene;
use guac_server::state::LauncherScreen;
use guacamole::census;
use guacamole::character::warm::WarmKey;
use serde::Serialize;
use tracing::{info, warn};

use crate::level;
use crate::theme::TvTheme;
use crate::turntable::TurntableSystems;
use crate::tv_state::{ServerLink, TvAssets, TvCamera};
use crate::warm::{CensusKey, PipelineWatch, Warm};

/// The most keys the pass draws in one frame: on a warm cache each takes a
/// few milliseconds, and a frame of this many cold ones stays a few seconds
/// at worst.
pub(crate) const MAX_BATCH: usize = 8;

/// A frame this quick lets the next one draw twice the keys...
const QUICK: Duration = Duration::from_millis(25);
/// ...and one this slow brings it back to one.
const SLOW: Duration = Duration::from_millis(50);

/// Frames the offscreen card and the level's output stay up (the card's
/// layout, its images and its first draw), and then frames the TV draws
/// alone before the pass lets it go: with the output camera under it the
/// TV's own output is the write it lays over the level with
/// ([`crate::level::TV_OVER`]), and alone it's Bevy's replacing one,
/// another pipeline, which would otherwise compile in the first frame after
/// the pass (AE.30).
const EXTRA_FRAMES: u32 = 4;

/// How long the pass waits on pipelines it didn't ask for (the card's, the
/// output's) once its keys are compiled, before it lets the TV go on.
const SETTLE_TIMEOUT: Duration = Duration::from_secs(30);

/// Camera orders under the TV's (0) and the turntables' (-1, -2), each
/// unique on its target.
const OUTPUT_ORDER: isize = -10;
const SAMPLE_ORDER: isize = -11;

/// A render layer nothing is on, for the output camera: it draws no
/// geometry, only its output.
const NOTHING: usize = 3;

/// The offscreen card's image, in pixels: a card's at 1080p, about.
const SAMPLE_SIZE: UVec2 = UVec2::new(448, 448);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Phase {
    /// Before the first frame's [`begin`].
    Waiting,
    Compiling,
    Done,
}

/// What `/api/_test/tv` reports of the pass.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub(crate) struct PrepareReport {
    /// `waiting`, `compiling` or `done` (also with no renderer, or a
    /// start whose server didn't come up).
    pub state: String,
    /// The census's keys, and those compiled (or given up on) so far.
    pub keys: usize,
    pub compiled: usize,
    /// Frames the pass ran and the most keys any one drew.
    pub frames: usize,
    pub largest_batch: usize,
    /// From its first frame to done.
    pub took_ms: Option<u64>,
}

/// The pass: its keys, what it drew besides them and how far it's got.
#[derive(Resource)]
pub(crate) struct Prepare {
    phase: Phase,
    keys: Vec<WarmKey>,
    started: Option<Instant>,
    /// When its keys were all compiled, for [`SETTLE_TIMEOUT`].
    covered: Option<Instant>,
    /// The output camera, the card's camera and the card's root.
    extras: Vec<Entity>,
    extra_frames: u32,
    batch: usize,
    report: PrepareReport,
}

impl Prepare {
    /// Done before it starts when there's no renderer.
    pub(crate) fn new(gpu: bool) -> Self {
        let phase = if gpu { Phase::Waiting } else { Phase::Done };
        Self {
            phase,
            keys: Vec::new(),
            started: None,
            covered: None,
            extras: Vec::new(),
            extra_frames: 0,
            batch: 1,
            report: PrepareReport {
                state: name(phase).into(),
                ..Default::default()
            },
        }
    }

    /// Whether the TV holds its screens for the pass.
    pub(crate) fn preparing(&self) -> bool {
        self.phase != Phase::Done
    }

    pub(crate) fn report(&self) -> &PrepareReport {
        &self.report
    }

    /// Plays the pass's count, for the screen's tests.
    #[cfg(test)]
    pub(crate) fn set_progress(&mut self, compiled: usize, keys: usize) {
        (self.report.compiled, self.report.keys) = (compiled, keys);
    }
}

fn name(phase: Phase) -> &'static str {
    match phase {
        Phase::Waiting => "waiting",
        Phase::Compiling => "compiling",
        Phase::Done => "done",
    }
}

/// The pass, after the turntables' systems (whose warm draws its keys).
pub(crate) struct PreparePlugin;

impl Plugin for PreparePlugin {
    fn build(&self, app: &mut App) {
        let gpu = app.get_sub_app(RenderApp).is_some();
        app.insert_resource(Prepare::new(gpu))
            .add_systems(Update, (begin, step).chain().after(TurntableSystems));
    }
}

/// Every census key with the material that draws it and its scene, the
/// cards' first (each with its glow pass), then the level's (the census
/// owns its masks whole, so the scene keys nothing).
fn census_entries() -> Vec<CensusKey> {
    let scene = Arc::new(Scene {
        fog: true,
        ..Scene::new()
    });
    let cards = census::turntable_keys().into_iter().map(|k| (k, true));
    let level = census::level_keys().into_iter().map(|k| (k, false));
    cards
        .chain(level)
        .map(|(key, card)| CensusKey {
            key,
            material: census::material_for(&key),
            scene: scene.clone(),
            card,
        })
        .collect()
}

/// What [`begin`] spawns with.
#[derive(bevy::ecs::system::SystemParam)]
pub(crate) struct ExtraSpawner<'w, 's> {
    commands: Commands<'w, 's>,
    images: ResMut<'w, Assets<Image>>,
    assets: Option<Res<'w, TvAssets>>,
    theme: Option<Res<'w, TvTheme>>,
    time: Res<'w, Time>,
}

/// The first frame: the census on the warm's queue and the extras up,
/// unless the server never came up (an error screen; a second copy closes
/// itself), when there's nothing to prepare for.
fn begin(
    mut prepare: ResMut<Prepare>,
    mut warm: ResMut<Warm>,
    link: Option<Res<ServerLink>>,
    tv: Query<Option<&RenderTarget>, With<TvCamera>>,
    mut s: ExtraSpawner,
) {
    if prepare.phase != Phase::Waiting {
        return;
    }
    let failed = link.is_some_and(|l| {
        let status = l.status.lock().unwrap_or_else(|e| e.into_inner());
        !matches!(status.screen, LauncherScreen::Main)
    });
    if failed {
        info!("the server didn't come up; nothing to prepare");
        prepare.phase = Phase::Done;
        prepare.report.state = name(Phase::Done).into();
        return;
    }
    // The TV's camera comes up in `Startup`; the first frame has it.
    let Ok(target) = tv.single() else {
        return;
    };
    let target = target.cloned().unwrap_or_default();
    let entries = census_entries();
    prepare.keys = entries.iter().map(|e| e.key).collect();
    warm.ask_census(entries);
    let output = s
        .commands
        .spawn((
            level::view_camera(target, OUTPUT_ORDER),
            RenderLayers::layer(NOTHING),
            Name::new("prepare: the level's output"),
        ))
        .id();
    // Its alpha clear on its layer, so the level's first frame finds that
    // pipeline compiled too (openspec AH.65).
    guacamole::character::spawn_alpha_clear(&mut s.commands, output, NOTHING);
    prepare.extras.push(output);
    if let (Some(assets), Some(theme)) = (s.assets.as_deref(), s.theme.as_deref()) {
        let image = s.images.add(Image::new_target_texture(
            SAMPLE_SIZE.x,
            SAMPLE_SIZE.y,
            TextureFormat::Rgba8UnormSrgb,
            None,
        ));
        let camera = s
            .commands
            .spawn((
                Camera2d,
                Camera {
                    order: SAMPLE_ORDER,
                    clear_color: ClearColorConfig::Custom(Color::BLACK),
                    ..default()
                },
                RenderTarget::Image(ImageRenderTarget {
                    handle: image,
                    scale_factor: 1.0,
                }),
                Name::new("prepare: a card"),
            ))
            .id();
        let plate = s.images.add(Image::new_fill(
            Extent3d {
                width: 1,
                height: 1,
                depth_or_array_layers: 1,
            },
            TextureDimension::D2,
            &[255; 4],
            TextureFormat::Rgba8UnormSrgb,
            RenderAssetUsages::RENDER_WORLD,
        ));
        let root = crate::card_view::spawn_sample_card(
            &mut s.commands,
            assets,
            theme,
            camera,
            SAMPLE_SIZE,
            plate,
            s.time.elapsed(),
        );
        prepare.extras.extend([camera, root]);
    }
    prepare.phase = Phase::Compiling;
    prepare.started = Some(Instant::now());
    prepare.report.state = name(Phase::Compiling).into();
    prepare.report.keys = prepare.keys.len();
    info!(
        keys = prepare.keys.len(),
        "getting ready: the census's pipelines, a card's and the level's output"
    );
}

/// Each frame of the pass: the next batch's size from the last frame's
/// length, the count compiled, and once every key is, the extras have drawn
/// and nothing else is still compiling, the extras gone; done once the TV
/// has drawn alone as many frames with nothing compiling.
fn step(
    mut commands: Commands,
    mut prepare: ResMut<Prepare>,
    mut warm: ResMut<Warm>,
    watch: Option<Res<PipelineWatch>>,
    time: Res<Time<Real>>,
) {
    if prepare.phase != Phase::Compiling {
        return;
    }
    let p = &mut *prepare;
    p.report.frames += 1;
    p.extra_frames += 1;
    p.batch = next_batch(p.batch, time.delta());
    warm.per_frame = p.batch;
    p.report.largest_batch = p.report.largest_batch.max(p.batch);
    p.report.compiled = p
        .keys
        .iter()
        .filter(|k| warm.covers(std::iter::once(*k)))
        .count();
    if p.report.compiled < p.keys.len() {
        return;
    }
    let covered = *p.covered.get_or_insert_with(Instant::now);
    let pending = watch.as_ref().map_or(0, |w| w.pending());
    let settled = p.extra_frames >= EXTRA_FRAMES && pending == 0;
    if !settled && covered.elapsed() < SETTLE_TIMEOUT {
        return;
    }
    if !settled {
        warn!(
            pending,
            "pipelines still compiling after the census; going on"
        );
    }
    if !p.extras.is_empty() {
        for e in p.extras.drain(..) {
            commands.entity(e).despawn();
        }
        // The TV alone from the next frame: its own output compiles here,
        // under the getting-ready screen.
        p.extra_frames = 0;
        return;
    }
    warm.per_frame = 1;
    p.phase = Phase::Done;
    p.report.state = name(Phase::Done).into();
    p.report.took_ms = p.started.map(|t| t.elapsed().as_millis() as u64);
    info!(
        keys = p.keys.len(),
        frames = p.report.frames,
        largest_batch = p.report.largest_batch,
        took_ms = p.report.took_ms,
        warm = ?warm.report(),
        "ready: every pipeline the TV draws Giants with is compiled"
    );
}

/// Twice the keys after a quick frame, up to [`MAX_BATCH`]; one after a
/// slow one; as it was otherwise.
pub(crate) fn next_batch(batch: usize, last_frame: Duration) -> usize {
    if last_frame > SLOW {
        1
    } else if last_frame < QUICK {
        (batch * 2).min(MAX_BATCH)
    } else {
        batch.max(1)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::turntable::{app, extract};
    use crate::warm::PipelineWatch;
    use guacamole::character::warm::MeshShape;

    #[test]
    fn the_batch_doubles_on_quick_frames_and_drops_to_one_on_a_slow_one() {
        let ms = Duration::from_millis;
        let mut batch = 1;
        let mut seen = Vec::new();
        for _ in 0..6 {
            batch = next_batch(batch, ms(8));
            seen.push(batch);
        }
        assert_eq!(seen, [2, 4, 8, 8, 8, 8]);
        assert_eq!(next_batch(8, ms(30)), 8, "neither quick nor slow");
        assert_eq!(next_batch(8, ms(51)), 1);
        assert_eq!(next_batch(1, ms(400)), 1);
        assert_eq!(next_batch(0, ms(30)), 1);
    }

    #[test]
    fn the_census_entries_are_the_tables_keys_each_drawn_as_itself() {
        let entries = census_entries();
        let cards = census::turntable_keys();
        let level = census::level_keys();
        assert_eq!(entries.len(), cards.len() + level.len());
        for (e, want) in entries.iter().zip(cards.iter().chain(&level)) {
            assert_eq!(e.key, *want);
            assert_eq!(e.material, census::material_for(want));
        }
        assert!(entries.iter().all(|e| e.scene.fog));
        // The cards' with their glow pass, the skinned table's then what
        // the figures carry on static meshes; the level's without a card's
        // glow pass, static but for the heroes' (AH.65: his keys in the
        // level and his glow masks there) and the first fight's creatures'
        // (AH.25), its glow masks in the level's glow pass.
        let (on_cards, in_level) = entries.split_at(cards.len());
        assert!(on_cards.iter().all(|e| e.card) && in_level.iter().all(|e| !e.card));
        let shapes: Vec<MeshShape> = on_cards.iter().map(|e| e.key.mesh).collect();
        let skinned = census::TURNTABLE.len();
        assert!(shapes[..skinned].iter().all(|m| m.skinned));
        assert!(shapes[skinned..].iter().all(|m| !m.skinned));
        assert_eq!(shapes.len() - skinned, census::TURNTABLE_STATIC.len());
        let skinned_in_level = census::HEROES.len()
            + census::HEROES_GLOW.len()
            + census::CREATURES.len()
            + census::CREATURES_GLOW.len();
        assert_eq!(
            in_level.iter().filter(|e| e.key.mesh.skinned).count(),
            skinned_in_level
        );
        let glows = census::LEVEL_GLOW.len()
            + census::HEROES_GLOW.len()
            + census::HEROES_GLOW_STATIC.len()
            + census::CREATURES_GLOW.len();
        assert_eq!(
            in_level
                .iter()
                .filter(|e| e.key.material.pass == guacamole::character::Pass::LevelGlow)
                .count(),
            glows
        );
    }

    fn compiling(app: &App) -> bool {
        app.world().resource::<Prepare>().preparing()
    }

    /// Headless, nothing to compile: done before the first frame, and the
    /// warm holds the shelf's list alone, none of the census.
    #[test]
    fn without_a_renderer_there_is_nothing_to_prepare() {
        let tmp = tempfile::tempdir().unwrap();
        let mut app = app(extract(tmp.path()));
        app.add_plugins(PreparePlugin);
        assert!(!compiling(&app));
        app.update();
        let report = app.world().resource::<Prepare>().report().clone();
        assert_eq!(
            (report.state.as_str(), report.keys, report.frames),
            ("done", 0, 0)
        );
        // The list is read on a task, so it lands in the first frame when
        // the task beats the frame (under load) and in a later one when not.
        let until = Instant::now() + Duration::from_secs(5);
        while app.world().resource::<Warm>().listed().is_empty() {
            assert!(Instant::now() < until, "the list never read");
            app.update();
        }
        let warm = app.world().resource::<Warm>();
        assert_eq!(warm.report().keys, warm.listed().len());
    }

    /// With a renderer stood in (AE.14's way): the census goes on the
    /// warm's queue at the first frame and is drawn in growing batches on
    /// quick frames; the pass ends once the render world says every key is
    /// compiled and nothing else is pending, its extras gone.
    #[test]
    fn the_pass_compiles_the_census_then_lets_the_tv_go_on() {
        let tmp = tempfile::tempdir().unwrap();
        let mut app = app(extract(tmp.path()));
        app.insert_resource(Warm::new(true));
        app.insert_resource(Prepare::new(true));
        app.add_systems(Update, (begin, step).chain().after(TurntableSystems))
            .add_systems(PostUpdate, level::tv_over_level);
        // A TV camera to read the target off.
        let tv = app.world_mut().spawn((Camera2d, TvCamera)).id();
        // How the TV writes onto its target: Bevy's own pick when alone.
        let writes = |app: &App| match app.world().get::<Camera>(tv).unwrap().output_mode {
            bevy::camera::CameraOutputMode::Write { blend_state, .. } => blend_state,
            bevy::camera::CameraOutputMode::Skip => panic!("the TV skips its write"),
        };
        app.update();
        let total = census::turntable_keys().len() + census::level_keys().len();
        {
            let p = app.world().resource::<Prepare>();
            assert_eq!(p.report().state, "compiling");
            assert_eq!(p.report().keys, total);
            assert_eq!(p.extras.len(), 1, "the output camera (no TV assets here)");
        }
        // The level's output under it: the TV writes over it as it will over
        // the level (AG.50), so that's the pipeline the pass compiles.
        let over = match level::TV_OVER {
            bevy::camera::CameraOutputMode::Write { blend_state, .. } => blend_state,
            bevy::camera::CameraOutputMode::Skip => unreachable!(),
        };
        assert_eq!(writes(&app), over);
        // Quick frames, nothing compiled: the batches grow and the pass
        // holds the TV.
        for _ in 0..20 {
            app.update();
        }
        assert!(app.world().resource::<Warm>().report().keys >= total);
        assert_eq!(app.world().resource::<Prepare>().report().compiled, 0);
        assert!(compiling(&app), "nothing compiled yet");
        app.world().resource::<PipelineWatch>().compile_all();
        let output = app.world().resource::<Prepare>().extras[0];
        // Frames ending with the extras gone and the TV still held (AE.30).
        let mut alone = 0;
        for _ in 0..200 {
            if !compiling(&app) {
                break;
            }
            app.world().resource::<PipelineWatch>().compile_all();
            app.update();
            let p = app.world().resource::<Prepare>();
            if p.preparing() && p.extras.is_empty() {
                alone += 1;
                assert!(app.world().get_entity(output).is_err(), "the output gone");
                assert_eq!(writes(&app), None, "alone, its own write");
            }
        }
        assert!(!compiling(&app));
        assert_eq!(
            alone, EXTRA_FRAMES,
            "the TV drew alone under the screen before the pass let it go"
        );
        let p = app.world().resource::<Prepare>();
        assert_eq!(p.report().compiled, total);
        assert!(p.report().largest_batch > 1, "{:?}", p.report());
        assert!(p.extras.is_empty());
        assert_eq!(app.world().resource::<Warm>().per_frame, 1);
        // The made-up extract's shelf lands too: its keys beside the census's,
        // each the census lacks counted (AE.25's guard: a Giants dump's
        // shelf counts none, `tests/giants_census.rs`).
        let until = Instant::now() + Duration::from_secs(5);
        while app.world().resource::<Warm>().listed().is_empty() {
            assert!(Instant::now() < until, "the shelf's list never read");
            app.world().resource::<PipelineWatch>().compile_all();
            app.update();
        }
        let census: Vec<WarmKey> = census::turntable_keys()
            .into_iter()
            .chain(census::level_keys())
            .collect();
        let w = app.world().resource::<Warm>();
        let misses = w.listed().iter().filter(|k| !census.contains(k)).count();
        assert!(misses > 0, "the made-up models draw keys Giants doesn't");
        assert_eq!(w.report().outside_census, misses);
        let warm = w.report().clone();
        assert!(warm.compiled >= total && warm.failed == 0, "{warm:?}");
    }
}
