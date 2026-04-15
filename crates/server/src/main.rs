//! Skylander Portal Controller — entry point.
//!
//! Threading model:
//!  - Main OS thread owns the eframe event loop.
//!  - A dedicated background OS thread hosts the tokio multi-threaded runtime
//!    running the Axum server + the driver worker task.
//!  - Shared state lives inside `Arc<AppState>` and an `AtomicUsize` client
//!    counter that both sides read.

mod config;
mod games;
mod http;
mod logging;
mod state;
mod ui;

use std::collections::HashMap;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::sync::Arc;
use std::sync::atomic::AtomicUsize;

use anyhow::{Context, Result};
use skylander_core::{Figure, SlotState, SLOT_COUNT};
use skylander_rpcs3_control::PortalDriver;
use tokio::sync::{broadcast, Mutex};
use tracing::{info, warn};

use crate::config::DriverKind;
use crate::state::{spawn_driver_worker, AppState, RpcsLifecycle};
use crate::ui::LauncherApp;

fn main() -> Result<()> {
    let cfg = config::load().context("load config")?;
    let _log_guard = logging::init(&cfg.log_dir)?;

    info!(
        rpcs3 = %cfg.rpcs3_exe.display(),
        pack = %cfg.firmware_pack_root.display(),
        port = cfg.bind_port,
        driver = ?cfg.driver_kind,
        "starting server",
    );

    // --- Index the firmware pack. ---
    let figures: Vec<Figure> = skylander_indexer::scan(&cfg.firmware_pack_root)
        .context("scan firmware pack")?;
    info!(count = figures.len(), "indexed figures");
    let figure_index: HashMap<_, _> = figures
        .iter()
        .enumerate()
        .map(|(i, f)| (f.id.clone(), i))
        .collect();

    // --- Load installed games (best-effort). ---
    let games = match games::load_installed(&cfg.games_yaml) {
        Ok(g) => {
            info!(count = g.len(), "loaded Skylanders game catalogue");
            g
        }
        Err(e) => {
            warn!("couldn't read games.yml at {}: {e}", cfg.games_yaml.display());
            Vec::new()
        }
    };

    // --- Pick bind address. ---
    let ip = first_non_loopback_ipv4().unwrap_or(Ipv4Addr::LOCALHOST);
    let bind = SocketAddr::from((ip, cfg.bind_port));
    let url = format!("http://{bind}");

    // --- Shared between Axum and the eframe UI. ---
    let portal: Arc<Mutex<[SlotState; SLOT_COUNT]>> =
        Arc::new(Mutex::new(std::array::from_fn(|_| SlotState::Empty)));
    let (events_tx, _) = broadcast::channel::<skylander_core::Event>(64);
    let connected_clients = Arc::new(AtomicUsize::new(0));

    // --- Start the Axum server + driver worker on a dedicated thread. ---
    //
    // The UIA driver is constructed inside the tokio thread, not on main —
    // `UIAutomation::new()` initializes COM as MTA on the calling thread, and
    // eframe needs main to stay uninitialized so it can OleInitialize(STA).
    // Doing both on the same thread crashes with RPC_E_CHANGED_MODE.
    let phone_dist = cfg.phone_dist_dir.clone();
    let bind_addr = bind;
    let driver_kind = cfg.driver_kind;
    let rpcs3_exe = cfg.rpcs3_exe.clone();
    let rpcs3_lifecycle = Arc::new(tokio::sync::Mutex::new(RpcsLifecycle::default()));
    let rpcs3_for_task = rpcs3_lifecycle.clone();
    let portal_for_task = portal.clone();
    let events_for_task = events_tx.clone();
    let clients_for_task = connected_clients.clone();
    let figures_for_task = figures.clone();
    let figure_index_for_task = figure_index.clone();
    let games_for_task = games.clone();

    let _server_thread = std::thread::Builder::new()
        .name("tokio".into())
        .spawn(move || {
            let rt = tokio::runtime::Builder::new_multi_thread()
                .enable_all()
                .build()
                .expect("build tokio runtime");
            rt.block_on(async move {
                let (driver, test_mock): (Arc<dyn PortalDriver>, _) = match build_driver(driver_kind) {
                    Ok(d) => d,
                    Err(e) => {
                        tracing::error!("failed to construct driver: {e}");
                        return;
                    }
                };
                let driver_tx = spawn_driver_worker(
                    driver,
                    portal_for_task.clone(),
                    events_for_task.clone(),
                );
                let state = Arc::new(AppState {
                    figures: figures_for_task,
                    figure_index: figure_index_for_task,
                    driver_tx,
                    portal: portal_for_task,
                    events: events_for_task,
                    connected_clients: clients_for_task,
                    games: games_for_task,
                    rpcs3_exe,
                    rpcs3: rpcs3_for_task,
                    #[cfg(feature = "test-hooks")]
                    test_mock,
                });
                #[cfg(not(feature = "test-hooks"))]
                let _ = test_mock;

                let app = http::router(state.clone(), phone_dist);
                let listener = tokio::net::TcpListener::bind(bind_addr)
                    .await
                    .expect("bind");
                info!("serving on http://{bind_addr}");
                if let Err(e) = axum::serve(listener, app).await {
                    warn!("axum server exited: {e}");
                }
            });
        })
        .expect("spawn server thread");

    // --- Fullscreen eframe window on the main thread. ---
    let figure_count = figures.len();
    let ui_clients = connected_clients.clone();
    // Dev builds are windowed so you can alt-tab away; release launches
    // fullscreen (it's invoked from Steam Big Picture with no window chrome).
    let viewport = {
        let mut vb = egui::ViewportBuilder::default().with_title("Skylander Portal Controller");
        if cfg!(feature = "dev-tools") {
            vb = vb.with_inner_size([900.0, 1000.0]);
        } else {
            vb = vb.with_fullscreen(true);
        }
        vb
    };
    let native_options = eframe::NativeOptions {
        viewport,
        ..Default::default()
    };
    let url_for_ui = url.clone();
    eframe::run_native(
        "skylander-portal-controller",
        native_options,
        Box::new(move |cc| {
            Ok(Box::new(LauncherApp::new(
                cc,
                ui_clients,
                figure_count,
                url_for_ui,
            )))
        }),
    )
    .map_err(|e| anyhow::anyhow!("eframe error: {e}"))
}

#[cfg(feature = "test-hooks")]
type TestMockHandle = Option<Arc<skylander_rpcs3_control::MockPortalDriver>>;
#[cfg(not(feature = "test-hooks"))]
type TestMockHandle = ();

type DriverBundle = (Arc<dyn PortalDriver>, TestMockHandle);

fn build_driver(kind: DriverKind) -> Result<DriverBundle> {
    match kind {
        DriverKind::Uia => {
            #[cfg(windows)]
            {
                let d = skylander_rpcs3_control::UiaPortalDriver::new()?;
                let arc: Arc<dyn PortalDriver> = Arc::new(d);
                #[cfg(feature = "test-hooks")]
                return Ok((arc, None));
                #[cfg(not(feature = "test-hooks"))]
                return Ok((arc, ()));
            }
            #[cfg(not(windows))]
            anyhow::bail!("UIA driver only available on Windows");
        }
        DriverKind::Mock => {
            #[cfg(feature = "dev-tools")]
            {
                let mock = Arc::new(skylander_rpcs3_control::MockPortalDriver::new());
                let arc: Arc<dyn PortalDriver> = mock.clone();
                #[cfg(feature = "test-hooks")]
                return Ok((arc, Some(mock)));
                #[cfg(not(feature = "test-hooks"))]
                {
                    let _ = mock;
                    return Ok((arc, ()));
                }
            }
            #[cfg(not(feature = "dev-tools"))]
            anyhow::bail!("mock driver only available with the dev-tools feature");
        }
    }
}

fn first_non_loopback_ipv4() -> Option<Ipv4Addr> {
    match local_ip_address::local_ip() {
        Ok(IpAddr::V4(v4)) if !v4.is_loopback() => Some(v4),
        _ => None,
    }
}

