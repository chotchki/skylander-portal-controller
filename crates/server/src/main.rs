//! Skylander Portal Controller — entry point.
//!
//! Threading model:
//!  - Main OS thread owns the eframe event loop.
//!  - A dedicated background OS thread hosts the tokio multi-threaded runtime
//!    running the Axum server + the driver worker task.
//!  - Shared state lives inside `Arc<AppState>` and an `AtomicUsize` client
//!    counter that both sides read.

use skylander_server::{config, games, http, logging, profiles, state, ui};

use std::collections::HashMap;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::sync::Arc;
use std::sync::atomic::AtomicUsize;

use anyhow::{Context, Result};
use skylander_core::{Figure, SLOT_COUNT, SlotState};
use skylander_rpcs3_control::PortalDriver;
use tokio::sync::{Mutex, broadcast};
use tracing::{info, warn};

use crate::config::DriverKind;
use crate::state::{AppState, RpcsLifecycle, spawn_crash_watchdog, spawn_driver_worker};
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
    let figures: Vec<Figure> =
        skylander_indexer::scan(&cfg.firmware_pack_root).context("scan firmware pack")?;
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
            warn!(
                "couldn't read games.yml at {}: {e}",
                cfg.games_yaml.display()
            );
            Vec::new()
        }
    };

    // --- Pick bind address. ---
    let ip = first_non_loopback_ipv4().unwrap_or(Ipv4Addr::LOCALHOST);
    let bind = SocketAddr::from((ip, cfg.bind_port));
    let key_hex = hex::encode(&cfg.hmac_key);

    // --- Phone-facing URL ---
    //
    // PLAN 4.18.1a / 4.19.10b: prefer `http://<os-hostname>.local:<port>/`
    // so home-screen PWA bookmarks survive a DHCP-lease refresh. Windows
    // ≥10 v2004 auto-publishes the local hostname via its built-in mDNS
    // responder; we just read it and put it in the QR. Falls back to
    // the raw-IP URL if the OS hostname can't be read.
    //
    // Earlier cuts tried to publish a custom `skylander-portal.local`
    // hostname via mdns-sd then via Windows's `DnsServiceRegister` — see
    // the `mdns` module doc for why both failed in practice.
    let (phone_url, used_mdns) =
        skylander_server::mdns::build_phone_url(ip, cfg.bind_port, &key_hex);
    if used_mdns {
        info!("phone URL {phone_url}");
    } else {
        tracing::warn!("OS hostname unavailable; QR will use the raw-IP URL: {phone_url}");
    }

    // --- Shared between Axum and the eframe UI. ---
    let portal: Arc<Mutex<[SlotState; SLOT_COUNT]>> =
        Arc::new(Mutex::new(std::array::from_fn(|_| SlotState::Empty)));
    let (events_tx, _) = broadcast::channel::<skylander_core::Event>(64);
    let connected_clients = Arc::new(AtomicUsize::new(0));
    let launcher_status = Arc::new(std::sync::Mutex::new(
        skylander_server::state::LauncherStatus::default(),
    ));

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
    let data_root = cfg.data_root.clone();
    let hmac_key = cfg.hmac_key.clone();
    let rpcs3_lifecycle = Arc::new(tokio::sync::Mutex::new(RpcsLifecycle::default()));
    let rpcs3_for_task = rpcs3_lifecycle.clone();
    let portal_for_task = portal.clone();
    let events_for_task = events_tx.clone();
    let clients_for_task = connected_clients.clone();
    let status_for_task = launcher_status.clone();
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
                let (driver, test_mock): (Arc<dyn PortalDriver>, _) =
                    match build_driver(driver_kind) {
                        Ok(d) => d,
                        Err(e) => {
                            tracing::error!("failed to construct driver: {e}");
                            return;
                        }
                    };
                let db_path = match crate::profiles::resolve_db_path() {
                    Ok(p) => p,
                    Err(e) => {
                        tracing::error!("resolve db path: {e}");
                        return;
                    }
                };
                info!(db = %db_path.display(), "opening profile db");
                let profile_store = match crate::profiles::ProfileStore::open(&db_path).await {
                    Ok(s) => s,
                    Err(e) => {
                        tracing::error!("open profile store: {e}");
                        return;
                    }
                };
                let sessions = Arc::new(crate::profiles::SessionRegistry::default());
                let figures_for_driver: Arc<Vec<Figure>> = Arc::new(figures_for_task.clone());
                let driver_tx = spawn_driver_worker(
                    driver,
                    portal_for_task.clone(),
                    events_for_task.clone(),
                    profile_store.clone(),
                    sessions.clone(),
                    figures_for_driver,
                );
                // Watchdog for unexpected RPCS3 exits (PLAN 4.15.14). Polls
                // the lifecycle handle every 500ms; on the first frame a
                // spawned process goes dead while `current` is still set,
                // broadcasts `Event::GameCrashed` so phones can render the
                // full-screen crash overlay. Clean quits drain `process` in
                // `/api/quit` before the process actually dies, so the
                // watchdog doesn't fire on those.
                spawn_crash_watchdog(
                    rpcs3_for_task.clone(),
                    portal_for_task.clone(),
                    events_for_task.clone(),
                    status_for_task.clone(),
                    std::time::Duration::from_millis(500),
                );
                let state = Arc::new(AppState {
                    figures: figures_for_task,
                    figure_index: figure_index_for_task,
                    driver_tx,
                    portal: portal_for_task,
                    events: events_for_task,
                    connected_clients: clients_for_task,
                    launcher_status: status_for_task,
                    games: games_for_task,
                    rpcs3_exe,
                    data_root,
                    phone_dist: phone_dist.clone(),
                    hmac_key,
                    rpcs3: rpcs3_for_task,
                    profiles: profile_store,
                    sessions,
                    #[cfg(feature = "test-hooks")]
                    test_mock,
                });
                // When `test-hooks` is disabled, `test_mock` is `()` and needs
                // to be consumed so clippy doesn't warn about an unused binding.
                // `let () = test_mock;` is the idiomatic way to assert+consume
                // a unit value (avoids `clippy::let_unit_value`).
                #[cfg(not(feature = "test-hooks"))]
                let () = test_mock;

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
    let ui_status = launcher_status.clone();
    // Both dev and release fullscreen on launch — same visual model, so
    // what you see at `cargo run` is what the HTPC user gets. Release
    // additionally pins always-on-top (Steam Big Picture invocation,
    // game viewport must not peek through); dev skips that so alt-tab
    // works during iteration. Either way the SHUT DOWN button on the
    // phone (4.15.11) is the supported escape — no need for a windowed
    // dev exception now that the kill path is reliable. Transparency is
    // always on so the in-game surface (PLAN 4.15.8) can render a
    // reconnect QR overlay with the game visible through the viewport.
    let viewport = {
        let mut vb = egui::ViewportBuilder::default()
            .with_title("Skylander Portal Controller")
            .with_fullscreen(true);
        if !cfg!(feature = "dev-tools") {
            vb = vb.with_always_on_top();
        }
        // Transparent window always — the panel still paints an opaque
        // starfield background in Main / Crashed / Farewell, so only the
        // in-game path actually sees through to RPCS3 behind egui.
        vb = vb.with_transparent(true);
        // Window icon — same gold/Kaos asset the phone PWA pins to the
        // home screen. Without this Windows shows the eframe default
        // "egui e" placeholder in the taskbar / alt-tab. Same
        // `debug_assertions` gate as the favicon swap so dev runs are
        // visually distinct from `cargo run --release`.
        if let Some(icon) = load_window_icon() {
            vb = vb.with_icon(icon);
        }
        vb
    };
    let native_options = eframe::NativeOptions {
        viewport,
        ..Default::default()
    };
    let url_for_ui = phone_url.clone();
    eframe::run_native(
        "skylander-portal-controller",
        native_options,
        Box::new(move |cc| {
            Ok(Box::new(LauncherApp::new(
                cc,
                ui_clients,
                ui_status,
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
                // The `return` on the branch below keeps the code symmetric
                // with the `test-hooks` branch above and sidesteps a cfg-type
                // mismatch that would otherwise require an outer `match`
                // rebind. Narrow allow instead of restructuring.
                #[cfg(not(feature = "test-hooks"))]
                #[allow(clippy::needless_return)]
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

/// Decode the embedded PWA icon PNG into the raw-RGBA form eframe wants
/// for the window's taskbar / title-bar / alt-tab icon. The 192px size
/// is the Android PWA standard — large enough that Windows downscales
/// cleanly to the 16/24/32px sizes the OS shell uses, small enough to
/// embed without bloating the binary. Same `debug_assertions` gate as
/// the favicon (`crates/server/src/http.rs` `dev_swapped`) so dev and
/// release builds are visually distinct in the taskbar too.
///
/// PNG bytes are baked at compile time so a missing `phone/assets/icons/`
/// directory is a compile error, not a runtime fallthrough — there's no
/// way to silently end up with the eframe default "egui e" icon again
/// once this lands.
fn load_window_icon() -> Option<egui::IconData> {
    const PROD: &[u8] =
        include_bytes!("../../../phone/assets/icons/icon-192.png");
    const DEV: &[u8] =
        include_bytes!("../../../phone/assets/icons/icon-dev-192.png");
    let bytes: &[u8] = if cfg!(debug_assertions) { DEV } else { PROD };
    let img = image::load_from_memory(bytes).ok()?;
    let rgba = img.to_rgba8();
    let (width, height) = rgba.dimensions();
    Some(egui::IconData {
        rgba: rgba.into_raw(),
        width,
        height,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Decode-roundtrip pin: the embedded PNG bytes must yield a
    /// 192×192 RGBA buffer. If `icon-bake` is rerun and accidentally
    /// resizes (or if someone deletes the file), this test catches it
    /// at `cargo test` time instead of waiting until launcher start.
    #[test]
    fn window_icon_decodes_to_expected_dimensions() {
        let icon = load_window_icon().expect("window icon should decode");
        assert_eq!(icon.width, 192, "icon width should be 192px");
        assert_eq!(icon.height, 192, "icon height should be 192px");
        // 4 bytes per pixel (RGBA).
        assert_eq!(
            icon.rgba.len(),
            (icon.width * icon.height * 4) as usize,
            "rgba buffer size should match width × height × 4"
        );
    }
}
