// Release builds open no console window beside the TV on Windows.
#![cfg_attr(not(feature = "dev-tools"), windows_subsystem = "windows")]
// Bevy system signatures trip both by design (design decision 7).
#![allow(clippy::type_complexity, clippy::too_many_arguments)]
//! guacamole: the shared server with the native driver, and a Bevy TV.
//!
//! Threading: the main thread runs Bevy; `guac_server::boot` runs the server
//! and driver worker on its own tokio thread.

mod alert;
mod card_motion;
mod card_view;
mod cards;
mod cli;
mod creatures;
mod dev_entry;
mod driver;
mod extraction;
mod hero;
mod hero_scripts;
mod input;
mod intro;
mod intro_player;
mod level;
mod look;
mod mirror;
mod motion;
mod pad;
mod pause;
mod physics;
mod picker;
mod play;
mod prepare;
mod progress;
mod screen;
#[cfg(feature = "dev-tools")]
mod shots;
mod starfield;
mod surfaces;
mod swap;
#[cfg(test)]
mod test_support;
mod theme;
mod tints;
mod title;
mod turntable;
mod tv;
mod tv_state;
mod video;
mod warm;

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::AtomicUsize;
use std::time::Duration;

use anyhow::{Context, Result};
use bevy::app::PluginGroupBuilder;
use bevy::gilrs::GilrsPlugin;
use bevy::log::LogPlugin;
use bevy::prelude::{
    App, AppExit, DefaultPlugins, MessageReader, Plugin, PluginGroup, ResMut, Window, WindowPlugin,
    default,
};
use bevy::window::{MonitorSelection, WindowMode as BevyWindowMode, WindowOccluded};
use bevy::winit::{UpdateMode, WinitSettings};
use crossbeam_channel::{Receiver, Sender};
use guac_extract::known::KnownDump;
use guac_server::boot::{BootOptions, BuiltDriver, Catalogue, ServerHandles, boot};
use guac_server::config::{DriverKind, WindowMode};
use guac_server::figures::RosterState;
use guac_server::hooks::Hooks;
use guac_server::logging::WorkerGuard;
use guac_server::progress::ProgressSink;
use guac_server::state::{LauncherScreen, LauncherStatus};
use guac_server::{logging, paths, setup};
use guacamole::character::{CharacterShadingPlugin, GlowPlugin};
use tracing::{info, warn};

use crate::cards::PortalFeed;
use crate::cli::Command;
use crate::driver::{NativePortalDriver, PortalChange, portal_channel};
use crate::extraction::ExtractJob;
use crate::intro_player::OutputLatency;
use crate::mirror::TvMirror;
use crate::picker::{DataRoot, Picker};
use crate::screen::SetupView;
use crate::turntable::TurntablePlugin;
use crate::tv::{FAREWELL_BEAT, ServerBridge, Tv};
use crate::tv_state::{Framing, ServerLink, Setup};

fn main() -> Result<()> {
    let command = match cli::parse(std::env::args_os().skip(1)) {
        Ok(command) => command,
        Err(e) => {
            eprintln!("guacamole: {e}\n{}", cli::USAGE);
            std::process::exit(2);
        }
    };
    if command == Command::Help {
        println!("{}", cli::USAGE);
        return Ok(());
    }
    let root = paths::guac_data_root()?;
    // Before `load`, so a settings failure is logged and shown, not silent.
    let log_guard = logging::init(&setup::log_dir(&root))?;
    let picked = match command {
        Command::SetupFolders(folders) => {
            let code = cli::setup_folders(&root, &folders, &mut std::io::stdout());
            drop(log_guard);
            std::process::exit(code);
        }
        Command::SetupPick => pick_before_the_window(&root),
        Command::Run | Command::Help => false,
    };
    let (portal_tx, portal_rx) = portal_channel();
    let (shelf_tx, shelf) = turntable::shelf_channel();
    let (level_tx, level_feed) = level::level_channel();
    let mirror = TvMirror::default();
    let asked_health = hero_scripts::AskedHealth::default();
    let entry_asks = dev_entry::EntryAsks::default();
    // Writes nothing, so a second copy that loses the port leaves no trace.
    let loaded = match setup::load(&root).context("load guacamole's settings") {
        Ok(loaded) => loaded,
        Err(e) => {
            tracing::error!("{e:#}");
            let mode = setup::window_mode(&setup::env());
            let no_setup = SetupView::new(false, &[], &Default::default());
            let link = failed_link(format!("{e:#}"));
            return run_tv(
                link,
                None,
                portal_rx,
                shelf,
                level_feed,
                no_setup,
                mode,
                root,
                None,
                OutputLatency::default(),
                (mirror, asked_health, entry_asks),
                None,
                log_guard,
            );
        }
    };
    let cfg = loaded.config.clone();
    let latency = loaded
        .output_latency_ms
        .map(|ms| OutputLatency(Duration::from_millis(ms.into())))
        .unwrap_or_default();

    let needs_setup = loaded.needs_setup();
    let scan = if needs_setup {
        Default::default()
    } else {
        loaded.scan()
    };
    info!(
        root = %root.display(),
        port = cfg.bind_port,
        window_mode = ?cfg.window_mode,
        needs_setup,
        dumps_from_env = loaded.dump_dirs_from_env,
        found = scan.found.len(),
        giants = scan.offered().is_some(),
        output_latency_ms = latency.0.as_millis() as u64,
        "starting guacamole",
    );
    let setup_view = SetupView::at_start(picked, needs_setup, &loaded.dump_dirs, &scan);
    let mut job = ExtractJob {
        test_known: test_known_dump(),
        shelf: Some(shelf_tx),
        level: Some(level_tx),
        ..ExtractJob::new(&root, loaded.extract_dir.clone(), loaded.ffmpeg.clone())
    };
    // No figures until the disc's roster lands (AE.4); without a Giants dump
    // there's none to read.
    let roster = match scan.offered() {
        Some(_) => RosterState::Reading,
        None => RosterState::Unrecognized,
    };

    let mut progress = None;
    let mut level_hero = None;
    let link = match boot(BootOptions {
        config: cfg.clone(),
        runtime_root: root.clone(),
        migrator: &setup::MIGRATOR,
        driver_factory: Box::new(move |kind| build_driver(kind, portal_tx)),
        hooks: Hooks::default(),
        extra_routes: test_routes(&mirror, &asked_health, &entry_asks),
        catalogue: Catalogue::Native {
            games: scan.games(),
            roster,
        },
        bind_ip: None,
    }) {
        Ok(server) => {
            keep_pairing_key(&root, &server, &cfg.hmac_key);
            job.figures = Some(server.figures.clone());
            job.progression = Some(server.progression.clone());
            progress = Some(server.progress.clone());
            level_hero = Some(server.level_hero.clone());
            if let Some(giants) = scan.offered() {
                extraction::preload_roster(giants.dir.clone(), job.clone());
            }
            link_to(&server)
        }
        // Show it rather than exit silently.
        Err(e) => {
            tracing::error!("boot failed: {e:#}");
            failed_link(format!("{e:#}"))
        }
    };
    run_tv(
        link,
        progress,
        portal_rx,
        shelf,
        level_feed,
        setup_view,
        cfg.window_mode,
        root,
        Some(job),
        latency,
        (mirror, asked_health, entry_asks),
        level_hero,
        log_guard,
    )
}

/// `.env.dev`'s `GUAC_TEST_KNOWN_DUMP`, `test-hooks` builds only: the e2e
/// harness's synthetic dump, trusted beside the known-dump table.
fn test_known_dump() -> Option<KnownDump> {
    if !cfg!(feature = "test-hooks") {
        return None;
    }
    let value = setup::env().remove("GUAC_TEST_KNOWN_DUMP")?;
    match extraction::parse_test_known(&value) {
        Ok(known) => {
            warn!(?known, "trusting the e2e harness's synthetic dump");
            Some(known)
        }
        Err(e) => {
            tracing::error!("GUAC_TEST_KNOWN_DUMP: {e}");
            None
        }
    }
}

/// `GUAC_TEST_POSTED_PADS_ONLY` in the process env (any value), `test-hooks`
/// builds only: the TV leaves real pads out (gilrs and, on macOS,
/// GameController), so a pad paired with the Mac can't walk the hero ahead
/// of `/api/_test/tv/pad`'s (AG.52). The e2e harness sets it; a look build
/// with a hand on a pad doesn't.
fn posted_pads_only() -> bool {
    cfg!(feature = "test-hooks") && std::env::var_os("GUAC_TEST_POSTED_PADS_ONLY").is_some()
}

/// Bevy's plugins for the TV: no log plugin (`logging::init` owns the global
/// subscriber), [`TvPacing`] after Bevy's own (AG.55) and one source per pad
/// (AG.53). On macOS that's GameController and gilrs is off: it reads
/// nothing of an 8BitDo there (AG.5); it's still built in, through Bevy's
/// `ui` collection, only its plugin is left out. Everywhere else gilrs.
/// [`posted_pads_only`] leaves both out, so only `/api/_test/tv/pad`'s pads
/// reach the TV.
fn tv_plugins(mode: WindowMode, posted_pads_only: bool) -> PluginGroupBuilder {
    let plugins = DefaultPlugins
        .set(WindowPlugin {
            primary_window: Some(window(mode)),
            ..default()
        })
        .disable::<LogPlugin>();
    let plugins = if posted_pads_only {
        plugins.disable::<GilrsPlugin>()
    } else {
        native_pads(plugins)
    };
    plugins.add(TvPacing)
}

/// The platform's own pad source (AG.53): GameController on macOS, gilrs
/// off beside it.
#[cfg(target_os = "macos")]
fn native_pads(plugins: PluginGroupBuilder) -> PluginGroupBuilder {
    plugins
        .disable::<GilrsPlugin>()
        .add(guacamole::game_controller::GameControllerPads)
}

/// The platform's own pad source: gilrs, already in Bevy's group.
#[cfg(not(target_os = "macos"))]
fn native_pads(plugins: PluginGroupBuilder) -> PluginGroupBuilder {
    plugins
}

/// The TV draws every frame the display takes, focused or not (AG.55):
/// Bevy's default (`WinitSettings::game()`) updates an unfocused window
/// only every 1/60 s off a timer, and the walk eased off that timer's
/// uneven ticks judders against the display's even ones. Unfocused is the
/// usual case: the phone and the pad drive the TV, not the window's keys.
/// Unfocused waits at most [`UNFOCUSED_WAIT`] between updates, which a
/// display's vsync outlasts up to 240 Hz, so it updates as continuously as
/// focused, while a window nothing paces (occluded: macOS's lock screen)
/// stops at 240 a second rather than spinning.
fn tv_updates() -> WinitSettings {
    WinitSettings {
        focused_mode: UpdateMode::Continuous,
        unfocused_mode: UpdateMode::reactive_low_power(UNFOCUSED_WAIT),
    }
}

/// The longest an unfocused TV waits from one update's start to the next
/// (an event wakes it sooner): 1/240 s, under a frame of any display up to
/// 240 Hz, so the display's vsync paces it, not this timer.
const UNFOCUSED_WAIT: Duration = Duration::from_nanos(4_166_667);

/// How often the TV updates while its window is occluded: nothing of it
/// shows and nothing paces its presents, so Bevy's unfocused 1/60 s rather
/// than a spin (AG.55: occluded under the Mac's lock, the portal drew 895
/// frames a second).
const OCCLUDED_WAIT: Duration = Duration::from_nanos(16_666_667);

fn occluded_updates() -> WinitSettings {
    WinitSettings {
        focused_mode: UpdateMode::reactive_low_power(OCCLUDED_WAIT),
        unfocused_mode: UpdateMode::reactive_low_power(OCCLUDED_WAIT),
    }
}

/// [`tv_updates`] while the window shows, [`occluded_updates`] while it's
/// occluded, on winit's word.
fn pace_on_occlusion(
    mut heard: MessageReader<WindowOccluded>,
    mut settings: ResMut<WinitSettings>,
) {
    let Some(last) = heard.read().last() else {
        return;
    };
    *settings = if last.occluded {
        occluded_updates()
    } else {
        tv_updates()
    };
    info!(
        occluded = last.occluded,
        "the TV's window occlusion changed"
    );
}

/// The TV's pacing (AG.55): [`tv_updates`] from the start, and
/// [`pace_on_occlusion`] in `First`. Its own plugin so the test runs the
/// wiring the window runs, added by [`tv_plugins`] so a test sees the
/// window get it.
struct TvPacing;

impl Plugin for TvPacing {
    fn build(&self, app: &mut App) {
        app.insert_resource(tv_updates())
            .add_systems(bevy::app::First, pace_on_occlusion);
    }
}

/// `/api/_test/tv` for the e2e harness, and AH.62's health and AH.25's
/// dev entry beside it; nothing outside `test-hooks`.
fn test_routes(
    mirror: &TvMirror,
    asked_health: &hero_scripts::AskedHealth,
    entry_asks: &dev_entry::EntryAsks,
) -> Option<axum::Router<std::sync::Arc<guac_server::state::AppState>>> {
    cfg!(feature = "test-hooks").then(|| {
        mirror::routes(mirror)
            .merge(hero_scripts::health_routes(asked_health))
            .merge(dev_entry::routes(entry_asks))
    })
}

/// `--setup` alone: the plain dialog, on the main thread before the event
/// loop exists, then a normal start. A cancel keeps the stored folders.
/// True when a pick was stored, so the TV lists what it found.
fn pick_before_the_window(root: &Path) -> bool {
    let picked = rfd::FileDialog::new()
        .set_title(picker::PICKER_TITLE)
        .pick_folder();
    let Some(folder) = picked else {
        info!("--setup: no folder chosen");
        println!("No folder chosen; keeping the stored folder.");
        return false;
    };
    let code = cli::setup_folders(root, &[folder], &mut std::io::stdout());
    if code != 0 {
        warn!("--setup: the picked folder wasn't stored");
    }
    code == 0
}

/// Runs the TV until it exits, then writes every figure's progress before the
/// file log's guard flushes it (AH.1).
fn run_tv(
    link: ServerLink,
    progress: Option<ProgressSink>,
    portal: Receiver<PortalChange>,
    shelf: turntable::ShelfFeed,
    level: level::LevelFeed,
    setup_view: SetupView,
    mode: WindowMode,
    root: PathBuf,
    job: Option<ExtractJob>,
    latency: OutputLatency,
    (mirror, asked_health, entry_asks): (TvMirror, hero_scripts::AskedHealth, dev_entry::EntryAsks),
    level_hero: Option<guac_server::progress::LevelHero>,
    log_guard: WorkerGuard,
) -> Result<()> {
    exit_after_farewell(link.status.clone(), progress.clone());

    let mut app = App::new();
    app.add_plugins(tv_plugins(mode, posted_pads_only()))
        .insert_resource(link)
        .insert_resource(PortalFeed(portal))
        .insert_resource(shelf)
        .insert_resource(level)
        .insert_resource(Setup(setup_view))
        .insert_resource(Framing(mode))
        .insert_resource(DataRoot(root))
        .insert_resource(latency)
        .add_plugins((
            ServerBridge,
            // The TV, composed here (openspec AQ.7): a lane adding a screen
            // or what draws under one edits this list or its own plugin.
            (
                // Before `Tv`'s screens, whose `Titles` needs only what this
                // registers.
                title::TitleShader,
                Tv,
                // The cards' turntables (AE.13): the character material, the
                // game's glow and the per-slot scenes.
                (CharacterShadingPlugin, GlowPlugin, TurntablePlugin),
                // The level, loaded behind the portal from the Giants pick
                // (AG.19); the warm compiles its keys.
                level::LevelPlugin,
                // The start's pass (AE.25): every pipeline the TV draws
                // Giants with, before the first screen a phone or the setup
                // acts on.
                prepare::PreparePlugin,
            ),
            input::InputRouter,
            Picker,
            extraction::ExtractPlugin,
            intro_player::IntroPlugin,
            physics::SeamPlugin,
            play::PlayPlugin,
            alert::AlertPlugin,
            progress::ProgressPlugin,
            hero_scripts::HeroScriptsPlugin,
            tints::CloneTintsPlugin,
            creatures::CreaturesPlugin,
        ));
    // AH.25's dev entry, in a dev build only.
    if let Some(entry) = dev_entry::plugin(cfg!(feature = "dev-tools"), entry_asks) {
        app.add_plugins(entry);
    }
    if let Some(progress) = &progress {
        app.insert_resource(progress::ProgressOut(progress.clone()));
    }
    if let Some(hero) = level_hero {
        app.insert_resource(play::LevelHeroOut(hero));
    }
    // The TV reads the caps from the same slot the extraction fills.
    if let Some(slot) = job.as_ref().and_then(|j| j.progression.clone()) {
        app.insert_resource(progress::ProgressionIn(slot));
    }
    if let Some(job) = job {
        app.insert_resource(job);
    }
    #[cfg(feature = "dev-tools")]
    app.add_plugins(shots::Shots);
    if cfg!(feature = "test-hooks") {
        app.insert_resource(mirror)
            .insert_resource(asked_health)
            .add_plugins(mirror::Mirror);
    }
    let exit = app.run();
    info!(?exit, "guacamole exited");
    flush_progress(progress.as_ref(), PROGRESS_FLUSH);
    drop(log_guard);
    match exit {
        AppExit::Success => Ok(()),
        AppExit::Error(code) => std::process::exit(i32::from(code.get())),
    }
}

/// The native driver, telling the TV through `tv`; test builds also hand it
/// to `/api/_test/inject_load`.
fn build_driver(kind: DriverKind, tv: Sender<PortalChange>) -> Result<BuiltDriver> {
    anyhow::ensure!(
        kind == DriverKind::Native,
        "guacamole runs only the native driver, not {kind:?}"
    );
    let driver = Arc::new(NativePortalDriver::new(tv));
    #[cfg(feature = "test-hooks")]
    let built = BuiltDriver::with_injector(driver);
    #[cfg(not(feature = "test-hooks"))]
    let built = BuiltDriver::new(driver);
    Ok(built)
}

/// Stores the first start's key, only once the server is up (a closed
/// `driver_tx` means it never came up).
fn keep_pairing_key(root: &Path, server: &ServerHandles, key: &[u8]) {
    if server.driver_tx.is_closed() {
        return;
    }
    let kept = setup::Setup::open(root).and_then(|s| s.keep_pairing_key(key));
    match kept {
        Ok(kept) if kept == key => {}
        Ok(_) => warn!("the stored pairing key differs from the one in the QR"),
        Err(e) => warn!("store the pairing key: {e:#}"),
    }
}

fn link_to(server: &ServerHandles) -> ServerLink {
    ServerLink {
        status: server.launcher_status.clone(),
        clients: server.connected_clients.clone(),
        phone_url: server.phone_url.clone(),
        join_qr_png: server.join_qr_png.clone(),
        games: server.games.clone(),
    }
}

fn failed_link(message: String) -> ServerLink {
    let status = LauncherStatus {
        screen: LauncherScreen::ServerError { message },
        ..Default::default()
    };
    ServerLink {
        status: Arc::new(std::sync::Mutex::new(status)),
        clients: Arc::new(AtomicUsize::new(0)),
        phone_url: String::new(),
        join_qr_png: Arc::new(Vec::new()),
        games: Default::default(),
    }
}

fn window(mode: WindowMode) -> Window {
    let title = "guacamole".to_string();
    match mode {
        WindowMode::Desktop => Window {
            title,
            resolution: (1280, 720).into(),
            resizable: true,
            ..default()
        },
        WindowMode::Tv => Window {
            title,
            mode: BevyWindowMode::BorderlessFullscreen(MonitorSelection::Primary),
            ..default()
        },
    }
}

/// How long a quit waits for the figures' progress to reach the store; the
/// farewell's fallback waits less, to keep its 5 s.
const PROGRESS_FLUSH: Duration = Duration::from_secs(3);
const PROGRESS_FLUSH_AFTER_FAREWELL: Duration = Duration::from_millis(300);

/// The farewell fallback's steps: how often it looks for the farewell, how
/// long past the farewell's beat it gives the TV, and the file log's catch-up.
const FAREWELL_POLL: Duration = Duration::from_millis(100);
const FAREWELL_GRACE: Duration = Duration::from_millis(1500);
const LOG_CATCH_UP: Duration = Duration::from_millis(100);

/// SHUT DOWN closes the app within 5 s (the shell spec), the progress flush
/// included, however late in a poll the farewell lands.
const _: () = assert!(
    FAREWELL_POLL.as_millis()
        + FAREWELL_BEAT.as_millis()
        + FAREWELL_GRACE.as_millis()
        + PROGRESS_FLUSH_AFTER_FAREWELL.as_millis()
        + LOG_CATCH_UP.as_millis()
        <= 5_000
);

/// Writes everything the TV handed over, playtime included, before exiting.
fn flush_progress(progress: Option<&ProgressSink>, timeout: Duration) {
    let Some(progress) = progress else {
        return;
    };
    if progress.flush_and_wait(timeout) {
        info!("figure progress written");
    } else {
        warn!("figure progress not confirmed written before the exit");
    }
}

/// Exits even if frames stall (an occluded window), so SHUT DOWN always
/// closes the app within 5 s, the figures' progress written first.
fn exit_after_farewell(
    status: Arc<std::sync::Mutex<LauncherStatus>>,
    progress: Option<ProgressSink>,
) {
    std::thread::Builder::new()
        .name("farewell".into())
        .spawn(move || {
            loop {
                std::thread::sleep(FAREWELL_POLL);
                let farewell = status
                    .lock()
                    .map(|s| matches!(s.screen, LauncherScreen::Farewell))
                    .unwrap_or(false);
                if farewell {
                    break;
                }
            }
            std::thread::sleep(FAREWELL_BEAT + FAREWELL_GRACE);
            warn!("the TV didn't exit after the farewell; exiting now");
            flush_progress(progress.as_ref(), PROGRESS_FLUSH_AFTER_FAREWELL);
            // The file log's worker writes asynchronously; let it catch up.
            std::thread::sleep(LOG_CATCH_UP);
            std::process::exit(0);
        })
        .expect("spawn the farewell thread");
}

#[cfg(test)]
mod tests {
    use super::*;

    /// AG.55: the TV updates continuously focused, and unfocused waits no
    /// longer than a 240 Hz display's frame, where Bevy's default waits a
    /// 60 Hz one's.
    #[test]
    fn the_tv_updates_every_frame_focused_or_not() {
        let wait = |mode: UpdateMode| match mode {
            UpdateMode::Continuous => Duration::ZERO,
            UpdateMode::Reactive { wait, .. } => wait,
        };
        let s = tv_updates();
        assert_eq!(s.focused_mode, UpdateMode::Continuous);
        assert!(wait(s.unfocused_mode) <= Duration::from_secs_f64(1.0 / 240.0));
        assert!(
            wait(s.unfocused_mode) > Duration::ZERO,
            "an occluded TV doesn't spin"
        );
        assert!(
            wait(WinitSettings::default().unfocused_mode) >= Duration::from_secs_f64(1.0 / 61.0),
            "Bevy's default throttles an unfocused window to 60"
        );
    }

    /// AG.55: [`TvPacing`] over Bevy's default (`WinitPlugin` puts it
    /// first) paces the TV by [`tv_updates`]; occluded, it updates every
    /// 1/60 s; shown again, every frame.
    #[test]
    fn an_occluded_tv_updates_at_sixty() {
        let mut app = App::new();
        app.add_message::<WindowOccluded>()
            .init_resource::<WinitSettings>()
            .add_plugins(TvPacing);
        app.update();
        let s = app.world().resource::<WinitSettings>();
        assert_eq!(s.focused_mode, UpdateMode::Continuous);
        assert_eq!(
            s.unfocused_mode,
            UpdateMode::reactive_low_power(UNFOCUSED_WAIT)
        );
        let window = app.world_mut().spawn_empty().id();
        app.world_mut().write_message(WindowOccluded {
            window,
            occluded: true,
        });
        app.update();
        let s = app.world().resource::<WinitSettings>();
        assert_eq!(
            s.unfocused_mode,
            UpdateMode::reactive_low_power(OCCLUDED_WAIT)
        );
        assert_eq!(
            s.focused_mode,
            UpdateMode::reactive_low_power(OCCLUDED_WAIT)
        );
        app.world_mut().write_message(WindowOccluded {
            window,
            occluded: false,
        });
        app.update();
        let s = app.world().resource::<WinitSettings>();
        assert_eq!(s.focused_mode, UpdateMode::Continuous);
        assert_eq!(s.unfocused_mode, tv_updates().unfocused_mode);
    }

    /// AG.52 and AG.53: with the switch the TV builds without gilrs, so no
    /// real pad connects; without it gilrs stays everywhere but macOS, where
    /// GameController reads the pads instead. The log plugin is out either
    /// way.
    #[test]
    fn posted_pads_only_leaves_gilrs_out() {
        for mode in [WindowMode::Desktop, WindowMode::Tv] {
            let posted = tv_plugins(mode, true);
            assert!(!posted.enabled::<GilrsPlugin>());
            assert!(!posted.enabled::<LogPlugin>());
            let real = tv_plugins(mode, false);
            assert_eq!(real.enabled::<GilrsPlugin>(), !cfg!(target_os = "macos"));
            assert!(!real.enabled::<LogPlugin>());
        }
    }

    /// Whether the TV's group reads pads through GameController; nothing
    /// off macOS, where the plugin doesn't exist.
    #[cfg(target_os = "macos")]
    fn reads_game_controller(plugins: &PluginGroupBuilder) -> bool {
        plugins.enabled::<guacamole::game_controller::GameControllerPads>()
    }

    #[cfg(not(target_os = "macos"))]
    fn reads_game_controller(_: &PluginGroupBuilder) -> bool {
        false
    }

    /// AG.53: gilrs reads the pads everywhere but macOS, where
    /// GameController does, one source per pad; the log plugin is out
    /// either way.
    #[test]
    fn gilrs_reads_the_pads_everywhere_but_macos() {
        for mode in [WindowMode::Desktop, WindowMode::Tv] {
            let plugins = tv_plugins(mode, false);
            assert_eq!(plugins.enabled::<GilrsPlugin>(), !cfg!(target_os = "macos"));
            assert_eq!(reads_game_controller(&plugins), cfg!(target_os = "macos"));
            assert!(!plugins.enabled::<LogPlugin>());
        }
    }

    /// AG.52 over AG.53: the switch keeps GameController's pads out too, so
    /// a pad paired with the Mac reaches the TV through neither source.
    #[test]
    fn posted_pads_only_leaves_game_controller_out() {
        for mode in [WindowMode::Desktop, WindowMode::Tv] {
            let posted = tv_plugins(mode, true);
            assert!(!reads_game_controller(&posted));
            assert!(!posted.enabled::<GilrsPlugin>());
        }
    }

    /// AG.55 under AG.53: the TV gets [`TvPacing`] whichever pads it reads,
    /// from [`tv_plugins`], the one group `run_tv` adds.
    #[test]
    fn the_tv_paces_itself_whichever_pads_it_reads() {
        for mode in [WindowMode::Desktop, WindowMode::Tv] {
            for posted in [false, true] {
                assert!(tv_plugins(mode, posted).enabled::<TvPacing>());
            }
        }
    }

    /// The TV's plugins that run headless, which both fixed-step guards
    /// build: the hero's, his scripts', the level's, the clones' tints
    /// (AH.67), the test hooks' mirror and the library's script runner. A
    /// plugin `run_tv` gains goes here too: one left out is never looked
    /// at, so a fixed-step system it adds outside the sets passes both
    /// guards (AH.67's tints moved into `FixedUpdate` with no set did,
    /// before they were listed).
    fn fixed_step_app() -> App {
        use bevy::prelude::MinimalPlugins;
        let mut app = App::new();
        app.add_plugins((
            MinimalPlugins,
            physics::SeamPlugin,
            hero::HeroPlugin,
            hero_scripts::HeroScriptsPlugin,
            level::LevelPlugin,
            tints::CloneTintsPlugin,
            mirror::Mirror,
            guacamole::scripts::ScriptPlugin,
        ));
        app
    }

    /// openspec AH.59: every fixed-step system the TV's plugins add
    /// ([`fixed_step_app`]) sits in one of the step's sets, so the step's
    /// order is [`guacamole::frame::ORDER`] and nothing runs outside it.
    #[test]
    fn every_fixed_step_system_is_in_a_frame_set() {
        use bevy::ecs::schedule::{ApplyDeferred, Schedules, SystemSet};
        use bevy::prelude::FixedUpdate;
        use guacamole::frame::{FramePlugin, ORDER};
        let mut app = fixed_step_app();
        assert!(app.is_plugin_added::<FramePlugin>());
        let world = app.world_mut();
        let mut fixed = world
            .resource_mut::<Schedules>()
            .remove(FixedUpdate)
            .expect("FixedUpdate");
        fixed.initialize(world).expect("it builds");
        let mut in_sets = std::collections::HashSet::new();
        for set in ORDER {
            if let Ok(keys) = fixed.graph().systems_in_set(set.intern()) {
                in_sets.extend(keys.iter().copied());
            }
        }
        // Bevy's own sync points between a set's commands and the next set
        // aren't systems of ours.
        let sync = std::any::TypeId::of::<ApplyDeferred>();
        let outside = fixed
            .systems()
            .expect("initialized")
            .filter(|(key, system)| !in_sets.contains(key) && system.system_type() != sync)
            .count();
        assert_eq!(outside, 0, "systems outside every set");
        assert!(in_sets.len() >= 10, "{} systems in the sets", in_sets.len());
    }

    /// openspec AH.59's review: each fixed-step system sits in the set its
    /// phase of the original's frame puts it in ([`guacamole::frame`]'s
    /// docs), told apart by its type: the pads first, his scripts and the
    /// level's before the solids, his move after them, the camera after
    /// him. A system moved to another set fails here, where the sets' own
    /// order and `every_fixed_step_system_is_in_a_frame_set` don't see it.
    #[test]
    fn each_fixed_step_system_runs_in_its_phases_set() {
        use bevy::ecs::schedule::{Schedules, SystemSet};
        use bevy::ecs::system::IntoSystem;
        use bevy::prelude::FixedUpdate;
        use guacamole::frame::{Frame, ORDER};
        use std::any::TypeId;
        let mut app = fixed_step_app();
        let world = app.world_mut();
        let mut fixed = world
            .resource_mut::<Schedules>()
            .remove(FixedUpdate)
            .expect("FixedUpdate");
        fixed.initialize(world).expect("it builds");
        let mut placed: Vec<(TypeId, Frame)> = Vec::new();
        for set in ORDER {
            let Ok(keys) = fixed.graph().systems_in_set(set.intern()) else {
                continue;
            };
            let keys: Vec<_> = keys.iter().copied().collect();
            for (key, system) in fixed.systems().expect("initialized") {
                if keys.contains(&key) {
                    placed.push((system.system_type(), set));
                }
            }
        }
        let want: [(TypeId, Frame, &str); 11] = [
            (
                mirror::count_pad_steps.system_type_id(),
                Frame::Pads,
                "the pads' count",
            ),
            (
                hero_scripts::step_scripts.system_type_id(),
                Frame::Scripts,
                "his scripts' step",
            ),
            (
                guacamole::scripts::step_scripts.system_type_id(),
                Frame::Scripts,
                "the library's script runner",
            ),
            (
                level::stand_props.system_type_id(),
                Frame::Solids,
                "the props' solids",
            ),
            (hero::step_heroes.system_type_id(), Frame::Hero, "his move"),
            (
                hero_scripts::advance_scripts.system_type_id(),
                Frame::Actors,
                "the engine's frame",
            ),
            (
                hero_scripts::draw_clones.system_type_id(),
                Frame::Actors,
                "the clones' draws",
            ),
            (
                hero::step_gaits.system_type_id(),
                Frame::Locomotion,
                "his locomotion",
            ),
            (
                hero_scripts::pose_hands.system_type_id(),
                Frame::Hands,
                "his hands",
            ),
            (
                hero::step_cameras.system_type_id(),
                Frame::Camera,
                "the camera",
            ),
            (
                mirror::log_gaits.system_type_id(),
                Frame::Record,
                "the gaits' log",
            ),
        ];
        for (ty, set, what) in want {
            let sets: Vec<Frame> = placed.iter().filter(|p| p.0 == ty).map(|p| p.1).collect();
            assert_eq!(sets, [set], "{what}");
        }
        assert_eq!(
            placed.len(),
            want.len(),
            "a fixed-step system the table lacks"
        );
    }
}
