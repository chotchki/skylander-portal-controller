//! Skylander Portal Controller — entry point.
//!
//! Threading model:
//!  - Main OS thread owns the eframe event loop.
//!  - A dedicated background OS thread hosts the tokio multi-threaded runtime
//!    running the Axum server + the driver worker task.
//!  - Shared state lives inside `Arc<AppState>` and an `AtomicUsize` client
//!    counter that both sides read.

mod config;
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
use crate::state::{spawn_driver_worker, AppState};
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

    // --- Build the driver. ---
    let driver: Arc<dyn PortalDriver> = build_driver(cfg.driver_kind)?;

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
    let phone_dist = cfg.phone_dist_dir.clone();
    let bind_addr = bind;
    let portal_for_task = portal.clone();
    let events_for_task = events_tx.clone();
    let clients_for_task = connected_clients.clone();
    let driver_for_task = driver.clone();
    let figures_for_task = figures.clone();
    let figure_index_for_task = figure_index.clone();

    let _server_thread = std::thread::Builder::new()
        .name("tokio".into())
        .spawn(move || {
            let rt = tokio::runtime::Builder::new_multi_thread()
                .enable_all()
                .build()
                .expect("build tokio runtime");
            rt.block_on(async move {
                let driver_tx = spawn_driver_worker(
                    driver_for_task,
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
                });

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
    let native_options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("Skylander Portal Controller")
            .with_fullscreen(true),
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

fn build_driver(kind: DriverKind) -> Result<Arc<dyn PortalDriver>> {
    match kind {
        DriverKind::Uia => {
            #[cfg(windows)]
            {
                let d = skylander_rpcs3_control::UiaPortalDriver::new()?;
                Ok(Arc::new(d))
            }
            #[cfg(not(windows))]
            anyhow::bail!("UIA driver only available on Windows");
        }
        DriverKind::Mock => {
            #[cfg(feature = "dev-tools")]
            {
                let d = skylander_rpcs3_control::MockPortalDriver::new();
                Ok(Arc::new(d))
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

