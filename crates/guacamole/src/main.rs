// Release builds open no console window beside the TV on Windows.
#![cfg_attr(not(feature = "dev-tools"), windows_subsystem = "windows")]
// Bevy system signatures trip both by design (design decision 7).
#![allow(clippy::type_complexity, clippy::too_many_arguments)]
//! guacamole: the shared server with the native driver, and a Bevy TV.
//!
//! Threading: the main thread runs Bevy; `guac_server::boot` runs the server
//! and driver worker on its own tokio thread.

mod driver;
mod screen;
mod tv;

use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::AtomicUsize;
use std::time::Duration;

use anyhow::{Context, Result};
use bevy::log::LogPlugin;
use bevy::prelude::{App, AppExit, DefaultPlugins, PluginGroup, Window, WindowPlugin, default};
use bevy::window::{MonitorSelection, WindowMode as BevyWindowMode};
use guac_server::boot::{BootOptions, BuiltDriver, Catalogue, ServerHandles, boot};
use guac_server::config::{DriverKind, WindowMode};
use guac_server::hooks::Hooks;
use guac_server::logging::WorkerGuard;
use guac_server::state::{LauncherScreen, LauncherStatus};
use guac_server::{logging, paths, setup};
use tracing::{info, warn};

use crate::driver::NativePortalDriver;
use crate::screen::SetupView;
use crate::tv::{FAREWELL_BEAT, ServerBridge, ServerLink, Setup, Tv};

fn main() -> Result<()> {
    let root = paths::guac_data_root()?;
    // Before `load`, so a settings failure is logged and shown, not silent.
    let log_guard = logging::init(&setup::log_dir(&root))?;
    // Writes nothing, so a second copy that loses the port leaves no trace.
    let loaded = match setup::load(&root).context("load guacamole's settings") {
        Ok(loaded) => loaded,
        Err(e) => {
            tracing::error!("{e:#}");
            let mode = setup::window_mode(&setup::env());
            let no_setup = SetupView::new(false, &[], &Default::default());
            return run_tv(failed_link(format!("{e:#}")), no_setup, mode, log_guard);
        }
    };
    let cfg = loaded.config.clone();

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
        "starting guacamole",
    );
    let setup_view = SetupView::new(needs_setup, &loaded.dump_dirs, &scan);

    let link = match boot(BootOptions {
        config: cfg.clone(),
        runtime_root: root.clone(),
        migrator: &setup::MIGRATOR,
        driver_factory: Box::new(build_driver),
        hooks: Hooks::default(),
        extra_routes: None,
        catalogue: Catalogue::Native {
            games: scan.games(),
        },
        bind_ip: None,
    }) {
        Ok(server) => {
            keep_pairing_key(&root, &server, &cfg.hmac_key);
            link_to(&server)
        }
        // A missing figures.json: show it rather than exit silently.
        Err(e) => {
            tracing::error!("boot failed: {e:#}");
            failed_link(format!("{e:#}"))
        }
    };
    run_tv(link, setup_view, cfg.window_mode, log_guard)
}

/// Runs the TV until it exits; the guard flushes the file log first.
fn run_tv(
    link: ServerLink,
    setup_view: SetupView,
    mode: WindowMode,
    log_guard: WorkerGuard,
) -> Result<()> {
    exit_after_farewell(link.status.clone());

    let exit = App::new()
        .add_plugins(
            DefaultPlugins
                .set(WindowPlugin {
                    primary_window: Some(window(mode)),
                    ..default()
                })
                // `logging::init` owns the global subscriber.
                .disable::<LogPlugin>(),
        )
        .insert_resource(link)
        .insert_resource(Setup(setup_view))
        .add_plugins((ServerBridge, Tv))
        .run();
    info!(?exit, "guacamole exited");
    drop(log_guard);
    match exit {
        AppExit::Success => Ok(()),
        AppExit::Error(code) => std::process::exit(i32::from(code.get())),
    }
}

/// The native driver; test-hook injection arrives with AA.7.
fn build_driver(kind: DriverKind) -> Result<BuiltDriver> {
    anyhow::ensure!(
        kind == DriverKind::Native,
        "guacamole runs only the native driver, not {kind:?}"
    );
    Ok(BuiltDriver::new(Arc::new(NativePortalDriver::new())))
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

/// Exits even if frames stall (an occluded window), so SHUT DOWN always
/// closes the app within 5 s.
fn exit_after_farewell(status: Arc<std::sync::Mutex<LauncherStatus>>) {
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
            // The file log's worker writes asynchronously; let it catch up.
            std::thread::sleep(Duration::from_millis(100));
            std::process::exit(0);
        })
        .expect("spawn the farewell thread");
}
