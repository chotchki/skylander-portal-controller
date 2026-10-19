// Release builds open no console window beside the TV on Windows.
#![cfg_attr(not(feature = "dev-tools"), windows_subsystem = "windows")]
// Bevy system signatures trip both by design (design decision 7).
#![allow(clippy::type_complexity, clippy::too_many_arguments)]
//! guacamole: the shared server with the native driver, and a Bevy TV.
//!
//! Threading: the main thread runs Bevy; `guac_server::boot` runs the server
//! and driver worker on its own tokio thread.

mod card_motion;
mod cards;
mod cli;
mod driver;
mod extraction;
mod input;
mod intro;
mod intro_player;
mod look;
mod mirror;
mod motion;
mod physics;
mod picker;
mod progress;
mod screen;
#[cfg(feature = "dev-tools")]
mod shots;
mod starfield;
mod surfaces;
mod theme;
mod title;
mod turntable;
mod tv;
mod video;
mod warm;

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::AtomicUsize;
use std::time::Duration;

use anyhow::{Context, Result};
use bevy::log::LogPlugin;
use bevy::prelude::{App, AppExit, DefaultPlugins, PluginGroup, Window, WindowPlugin, default};
use bevy::window::{MonitorSelection, WindowMode as BevyWindowMode};
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
use tracing::{info, warn};

use crate::cards::PortalFeed;
use crate::cli::Command;
use crate::driver::{NativePortalDriver, PortalChange, portal_channel};
use crate::extraction::ExtractJob;
use crate::intro_player::OutputLatency;
use crate::mirror::TvMirror;
use crate::picker::{DataRoot, Picker};
use crate::screen::SetupView;
use crate::tv::{FAREWELL_BEAT, Framing, ServerBridge, ServerLink, Setup, Tv};

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
    let mirror = TvMirror::default();
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
                no_setup,
                mode,
                root,
                None,
                OutputLatency::default(),
                mirror,
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
        ..ExtractJob::new(&root, loaded.extract_dir.clone(), loaded.ffmpeg.clone())
    };
    // No figures until the disc's roster lands (AE.4); without a Giants dump
    // there's none to read.
    let roster = match scan.offered() {
        Some(_) => RosterState::Reading,
        None => RosterState::Unrecognized,
    };

    let mut progress = None;
    let link = match boot(BootOptions {
        config: cfg.clone(),
        runtime_root: root.clone(),
        migrator: &setup::MIGRATOR,
        driver_factory: Box::new(move |kind| build_driver(kind, portal_tx)),
        hooks: Hooks::default(),
        extra_routes: test_routes(&mirror),
        catalogue: Catalogue::Native {
            games: scan.games(),
            roster,
        },
        bind_ip: None,
    }) {
        Ok(server) => {
            keep_pairing_key(&root, &server, &cfg.hmac_key);
            job.figures = Some(server.figures.clone());
            progress = Some(server.progress.clone());
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
        setup_view,
        cfg.window_mode,
        root,
        Some(job),
        latency,
        mirror,
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

/// `/api/_test/tv` for the e2e harness; nothing outside `test-hooks`.
fn test_routes(
    mirror: &TvMirror,
) -> Option<axum::Router<std::sync::Arc<guac_server::state::AppState>>> {
    cfg!(feature = "test-hooks").then(|| mirror::routes(mirror))
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
    setup_view: SetupView,
    mode: WindowMode,
    root: PathBuf,
    job: Option<ExtractJob>,
    latency: OutputLatency,
    mirror: TvMirror,
    log_guard: WorkerGuard,
) -> Result<()> {
    exit_after_farewell(link.status.clone(), progress.clone());

    let mut app = App::new();
    app.add_plugins(
        DefaultPlugins
            .set(WindowPlugin {
                primary_window: Some(window(mode)),
                ..default()
            })
            // `logging::init` owns the global subscriber.
            .disable::<LogPlugin>(),
    )
    .insert_resource(link)
    .insert_resource(PortalFeed(portal))
    .insert_resource(shelf)
    .insert_resource(Setup(setup_view))
    .insert_resource(Framing(mode))
    .insert_resource(DataRoot(root))
    .insert_resource(latency)
    .add_plugins((
        ServerBridge,
        Tv,
        input::InputRouter,
        Picker,
        extraction::ExtractPlugin,
        intro_player::IntroPlugin,
        physics::SeamPlugin,
        progress::ProgressPlugin,
    ));
    if let Some(progress) = &progress {
        app.insert_resource(progress::ProgressOut(progress.clone()));
    }
    if let Some(job) = job {
        app.insert_resource(job);
    }
    #[cfg(feature = "dev-tools")]
    app.add_plugins(shots::Shots);
    if cfg!(feature = "test-hooks") {
        app.insert_resource(mirror).add_plugins(mirror::Mirror);
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
const PROGRESS_FLUSH_AFTER_FAREWELL: Duration = Duration::from_millis(400);

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
                std::thread::sleep(Duration::from_millis(100));
                let farewell = status
                    .lock()
                    .map(|s| matches!(s.screen, LauncherScreen::Farewell))
                    .unwrap_or(false);
                if farewell {
                    break;
                }
            }
            std::thread::sleep(FAREWELL_BEAT + Duration::from_millis(1500));
            warn!("the TV didn't exit after the farewell; exiting now");
            flush_progress(progress.as_ref(), PROGRESS_FLUSH_AFTER_FAREWELL);
            // The file log's worker writes asynchronously; let it catch up.
            std::thread::sleep(Duration::from_millis(100));
            std::process::exit(0);
        })
        .expect("spawn the farewell thread");
}
