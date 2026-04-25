//! Skylander Portal Controller — entry point.
//!
//! Threading model:
//!  - Main OS thread owns the eframe event loop.
//!  - A dedicated background OS thread hosts the tokio multi-threaded runtime
//!    running the Axum server + the driver worker task.
//!  - Shared state lives inside `Arc<AppState>` and an `AtomicUsize` client
//!    counter that both sides read.

use skylander_server::{config, http, logging, profiles, state, ui};

use std::collections::HashMap;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::sync::Arc;
use std::sync::atomic::AtomicUsize;

use anyhow::{Context, Result};
use skylander_core::{Figure, SLOT_COUNT, SlotState};
use skylander_rpcs3_control::{PortalDriver, RpcsProcess};
use tokio::sync::{Mutex, broadcast};
use tracing::{info, warn};

use crate::config::DriverKind;
use crate::state::{
    AppState, RpcsLifecycle, spawn_crash_watchdog, spawn_driver_worker,
    spawn_shader_compile_watchdog,
};
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

    // --- Index the firmware pack + runtime scanned dir. ---
    // PLAN 6.5.4 made the pack optional; PLAN 6.5.5a merges the scanned
    // dir in alongside. Two rules govern the merge:
    //   1. Pack .sky files are fresh-state masters (reset-to-zero); scans
    //      capture the physical tag's *current* state. Pack wins on any
    //      `(figure_id, variant)` collision so new-profile working copies
    //      fork from a clean slate rather than the user's current XP.
    //   2. A pack with zero hits + no scans = empty library; the phone
    //      already handles that gracefully (empty-state copy).
    let pack_figures: Vec<Figure> = if cfg.firmware_pack_root.as_os_str().is_empty() {
        info!("no firmware pack configured — starting with an empty library");
        Vec::new()
    } else {
        skylander_indexer::scan(&cfg.firmware_pack_root).context("scan firmware pack")?
    };
    info!(count = pack_figures.len(), "indexed pack figures");

    // Runtime state (profile db, scanned dumps, working copies) lives under
    // `resolve_runtime_dir()` — `./dev-data/` in dev, `%APPDATA%/...` in
    // release. Distinct from `cfg.data_root` which is the tracked-asset
    // root (images/, figures.json, games/). Conflating the two earlier
    // (setting DATA_ROOT=./dev-data in .env.dev) broke image lookup
    // because the server then looked for `dev-data/images/...`.
    let runtime_dir =
        skylander_server::paths::resolve_runtime_dir().context("resolve runtime dir")?;

    // Merge scanned figures + build the tag-identity map only when the
    // NFC feature is enabled — without it there are no scans to dedup
    // against, so the ~1s pack re-parse would be pure cost.
    #[cfg(feature = "nfc-import")]
    let (figures, tag_identity_map) = {
        use skylander_core::{FigureId, TagIdentity};
        use std::collections::HashMap as StdHashMap;

        // PLAN 6.6.1e: `Figure.tag_identity` is populated by the indexer
        // now, so we build the lookup from `f.tag_identity` directly
        // instead of re-parsing every `.sky` at boot. Saves ~1s on a
        // 500-figure pack and collapses a layer of "where does the
        // identity come from" confusion.

        let mut pack_figures = pack_figures;
        let pack_index_by_id: StdHashMap<FigureId, usize> = pack_figures
            .iter()
            .enumerate()
            .map(|(i, f)| (f.id.clone(), i))
            .collect();

        let mut tag_identity_map: StdHashMap<TagIdentity, FigureId> = StdHashMap::new();
        for f in &pack_figures {
            if let Some(id) = f.tag_identity {
                tag_identity_map.entry(id).or_insert_with(|| f.id.clone());
            }
        }
        info!(
            count = tag_identity_map.len(),
            "built tag-identity map from pack"
        );

        let scanned_dir = runtime_dir.join("scanned");
        let scan_figures =
            skylander_indexer::scan_runtime(&scanned_dir).context("walk scanned-figure dir")?;
        let mut scanned_kept = 0usize;
        let mut nicknames_promoted = 0usize;
        let mut scan_only_figures: Vec<Figure> = Vec::new();
        for f in scan_figures {
            let Some(key) = f.tag_identity else {
                // Parse failed inside scan_runtime — no identity, no dedup.
                // Accept into the library as scan-only with whatever
                // canonical_name/variant_tag the indexer produced.
                scanned_kept += 1;
                scan_only_figures.push(f);
                continue;
            };
            if let Some(pack_id) = tag_identity_map.get(&key).cloned() {
                // Pack wins — suppress the scan entry from the library
                // (still exists on disk as a physical-tag record, just
                // not a distinct library card). BUT: if the scan carried
                // a user-chosen nickname AND the pack's variant_tag is
                // still the default "base", promote the nickname onto
                // the pack card so the user sees their customization
                // (common for Creation Crystals — you name "DELFOX"
                // what the pack just calls "CRYSTAL_-_FIRE_Reactor").
                // PLAN 6.5.5a option B.
                let scan_variant = f.variant_tag.trim();
                if !scan_variant.is_empty()
                    && scan_variant != "base"
                    && let Some(&idx) = pack_index_by_id.get(&pack_id)
                {
                    let pack_fig = &mut pack_figures[idx];
                    if pack_fig.variant_tag == "base" && pack_fig.canonical_name != scan_variant {
                        pack_fig.variant_tag = scan_variant.to_string();
                        nicknames_promoted += 1;
                    }
                }
                continue;
            }
            tag_identity_map.insert(key, f.id.clone());
            scanned_kept += 1;
            scan_only_figures.push(f);
        }
        info!(
            kept = scanned_kept,
            nicknames_promoted, "merged scanned figures (pack wins on collision)"
        );

        let mut figures: Vec<Figure> = pack_figures;
        figures.extend(scan_only_figures);
        (figures, std::sync::Arc::new(tag_identity_map))
    };
    #[cfg(not(feature = "nfc-import"))]
    let figures: Vec<Figure> = pack_figures;
    info!(count = figures.len(), "total library figures");

    let figure_index: HashMap<_, _> = figures
        .iter()
        .enumerate()
        .map(|(i, f)| (f.id.clone(), i))
        .collect();

    // Game catalogue now comes from `driver.list_installed_games()` —
    // constructed inside the tokio task (COM apartment constraint) and
    // populated into AppState there.

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

    // Pre-render the round-QR PNG once — the URL is fixed for the life
    // of the server, so `/api/join-qr.png` is just an `Arc<Vec<u8>>`
    // clone per request. Produced here (synchronously, cheap) so the
    // eframe launcher and the HTTP endpoint share the same committed
    // image buffer (one source of truth for the rendered QR).
    let join_qr_png = match skylander_server::round_qr::render_png(
        &phone_url,
        &skylander_server::round_qr::RoundQrConfig::launcher_default(),
    ) {
        Ok(bytes) => Arc::new(bytes),
        Err(e) => {
            tracing::error!("render join QR PNG: {e}");
            Arc::new(Vec::new())
        }
    };

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
    // Only consumed under `nfc-import` (used by the scanner spawn
    // below). Bind unconditionally so the feature flag doesn't leak
    // into this prep block; the cfg gate on the consumer is enough.
    #[cfg_attr(not(feature = "nfc-import"), allow(unused_variables))]
    let runtime_dir_for_task = runtime_dir.clone();
    let hmac_key = cfg.hmac_key.clone();
    let rpcs3_lifecycle = Arc::new(tokio::sync::Mutex::new(RpcsLifecycle::default()));
    let rpcs3_for_task = rpcs3_lifecycle.clone();
    let portal_for_task = portal.clone();
    let events_for_task = events_tx.clone();
    let clients_for_task = connected_clients.clone();
    let status_for_task = launcher_status.clone();
    let figures_for_task = figures.clone();
    let figure_index_for_task = figure_index.clone();
    #[cfg(feature = "nfc-import")]
    let tag_identity_map_for_task = tag_identity_map.clone();
    let join_qr_png_for_task = join_qr_png.clone();

    let status_for_errors = launcher_status.clone();
    let status_for_ready = launcher_status.clone();
    let _server_thread = std::thread::Builder::new()
        .name("tokio".into())
        .spawn(move || {
            let rt = tokio::runtime::Builder::new_multi_thread()
                .enable_all()
                .build()
                .expect("build tokio runtime");
            // Helper: log + flip the launcher screen to ServerError so the
            // egui surface shows a real diagnostic instead of a QR that
            // points at a server that isn't there. Each failure path below
            // calls this and then returns — the tokio thread exits but
            // egui keeps running and renders the message.
            let report_fatal = |scope: &str, err: &dyn std::fmt::Display| {
                tracing::error!("{scope}: {err}");
                if let Ok(mut st) = status_for_errors.lock() {
                    st.screen = state::LauncherScreen::ServerError {
                        message: format!("{scope}: {err}"),
                    };
                }
            };
            rt.block_on(async move {
                let (driver, test_mock): (Arc<dyn PortalDriver>, _) =
                    match build_driver(driver_kind) {
                        Ok(d) => d,
                        Err(e) => {
                            report_fatal("failed to construct driver", &e);
                            return;
                        }
                    };
                let db_path = match crate::profiles::resolve_db_path() {
                    Ok(p) => p,
                    Err(e) => {
                        report_fatal("resolve db path", &e);
                        return;
                    }
                };
                info!(db = %db_path.display(), "opening profile db");
                let profile_store = match crate::profiles::ProfileStore::open(&db_path).await {
                    Ok(s) => s,
                    Err(e) => {
                        report_fatal("open profile store", &e);
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
                    rpcs3_exe.clone(),
                    std::time::Duration::from_millis(500),
                );
                // Shader-compile watchdog — polls RPCS3's main window
                // title for "Compiling shaders…" progress so the
                // launcher's loading surface can show the count
                // (first-run compile can take several minutes).
                spawn_shader_compile_watchdog(
                    status_for_task.clone(),
                    rpcs3_exe.clone(),
                    std::time::Duration::from_millis(500),
                );
                // NFC scanner worker (PLAN 6.5.1 + 6.5.5a). Feature-gated:
                // off by default so users without an ACR122U aren't
                // pulling in pcsc linkage. Dumps land under
                // `<runtime_dir>/scanned/` as `<uid>.sky` (next to the
                // profile db, per paths::resolve_runtime_dir); scanner
                // emits `Event::FigureScanned` on the broadcast channel
                // with `is_duplicate` set by consulting the library
                // identity map (pack + prior scans).
                #[cfg(feature = "nfc-import")]
                skylander_server::nfc::spawn(
                    events_for_task.clone(),
                    runtime_dir_for_task.join("scanned"),
                    tag_identity_map_for_task.clone(),
                );
                // Bind is the most common startup failure (port already
                // in use, permission denied on a privileged port). Don't
                // panic — flip the launcher to ServerError so the user
                // sees the actual reason instead of a closed window.
                let listener = match tokio::net::TcpListener::bind(bind_addr).await {
                    Ok(l) => l,
                    Err(e) => {
                        report_fatal(&format!("bind {bind_addr}"), &e);
                        return;
                    }
                };
                // Spawn RPCS3 at startup (PLAN 4.15.16) so the emulator
                // is ready by the time the phone reaches the game
                // picker. Under the mock driver we install a Mock
                // variant that always reports alive — same lifecycle
                // shape, no real process — so `/api/launch` passes its
                // `process.is_some()` gate on Mac/Linux dev without
                // `/api/_test/*` hacks. Hard-fails on spawn error per
                // the "starting-screen makes sense" decision: egui's
                // LaunchPhase::Startup already gates on rpcs3_running,
                // so the cloud vortex persists while we wait, and on
                // error flips straight to ServerError with the
                // diagnostic.
                //
                // This block intentionally runs BEFORE the "serving on"
                // log: test harnesses (TestServer::spawn_live) scrape
                // that line as their readiness signal, so it must imply
                // "RPCS3 is up + axum is about to accept connections",
                // not "listener bound but RPCS3 still starting up".
                let spawn_result: Result<RpcsProcess, anyhow::Error> = match driver_kind {
                    crate::config::DriverKind::Uia => {
                        let rpcs3_exe_clone = rpcs3_exe.clone();
                        match tokio::task::spawn_blocking(move || -> anyhow::Result<RpcsProcess> {
                            let mut proc = RpcsProcess::launch_library(&rpcs3_exe_clone)?;
                            proc.wait_ready(std::time::Duration::from_secs(45))?;
                            Ok(proc)
                        })
                        .await
                        {
                            Ok(inner) => inner,
                            Err(e) => Err(anyhow::anyhow!("RPCS3 spawn task panicked: {e}")),
                        }
                    }
                    crate::config::DriverKind::Mock => Ok(RpcsProcess::mock()),
                };
                match spawn_result {
                    Ok(proc) => {
                        let mut guard = rpcs3_for_task.lock().await;
                        guard.process = Some(proc);
                        drop(guard);
                        if let Ok(mut st) = status_for_task.lock() {
                            st.rpcs3_running = true;
                        }
                        info!(?driver_kind, "RPCS3 lifecycle ready");
                    }
                    Err(e) => {
                        report_fatal("spawn RPCS3 at startup", &e);
                        return;
                    }
                }

                // PLAN 3.7.8 phase 2: game catalogue is now truth-from-UIA.
                // Enumerate the library via the driver worker (same job the
                // /api/launch handler uses for verify-at-launch), then filter
                // to supported Skylanders serials using SKYLANDERS_SERIALS.
                // Drops the games.yml dependency entirely — if a user removes
                // a game from RPCS3's library, the picker reflects that on
                // the next server restart. A failure here logs + starts with
                // an empty catalogue rather than aborting; user sees "no games
                // installed" in the picker and can re-scan in RPCS3.
                let games = {
                    let (etx, erx) = tokio::sync::oneshot::channel();
                    if let Err(e) = driver_tx
                        .send(crate::state::DriverJob::EnumerateGames {
                            timeout: std::time::Duration::from_secs(5),
                            done: etx,
                        })
                        .await
                    {
                        warn!("queue EnumerateGames at startup: {e}");
                        Vec::<skylander_core::InstalledGame>::new()
                    } else {
                        match erx.await {
                            Ok(Ok(serials)) => {
                                let catalogue = serials_to_catalogue(&serials);
                                info!(
                                    installed = catalogue.len(),
                                    enumerated = serials.len(),
                                    "loaded Skylanders game catalogue from RPCS3 library",
                                );
                                catalogue
                            }
                            Ok(Err(e)) => {
                                warn!("enumerate_games at startup failed: {e}");
                                Vec::new()
                            }
                            Err(e) => {
                                warn!("EnumerateGames ack dropped: {e}");
                                Vec::new()
                            }
                        }
                    }
                };

                let state = Arc::new(AppState {
                    figures: figures_for_task,
                    figure_index: figure_index_for_task,
                    driver_tx,
                    portal: portal_for_task,
                    events: events_for_task,
                    connected_clients: clients_for_task,
                    launcher_status: status_for_task,
                    games,
                    rpcs3_exe,
                    data_root,
                    phone_dist: phone_dist.clone(),
                    hmac_key,
                    boot_id: {
                        // Random u64 from OsRng (already a dep via argon2's
                        // rand_core re-export). Phones compare against the
                        // last-seen value to detect a server restart.
                        use rand_core::RngCore;
                        rand_core::OsRng.next_u64()
                    },
                    rpcs3: rpcs3_for_task,
                    profiles: profile_store,
                    sessions,
                    join_qr_png: join_qr_png_for_task,
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

                info!("serving on http://{bind_addr}");
                // Tell the launcher UI the server is healthy. Until
                // this flips, the launcher's intro animations stay
                // gated — calm starfield + brand intro only — so a
                // startup failure routes straight to the error
                // surface without first running the spin animation
                // (PLAN 4.19.x).
                if let Ok(mut st) = status_for_ready.lock() {
                    st.server_ready = true;
                }
                if let Err(e) = axum::serve(listener, app).await {
                    // axum exits non-cleanly: same surface as a startup
                    // failure (the phone QR no longer points at a live
                    // server, so showing it would be dishonest).
                    report_fatal("axum server exited", &e);
                }
            });
        })
        .expect("spawn server thread");

    // --- Fullscreen eframe window on the main thread. ---
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
                cc, ui_clients, ui_status, url_for_ui,
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

/// Convert a list of RPCS3 library serials (from `enumerate_games`) into the
/// phone-facing `InstalledGame` catalogue. Filters out non-Skylanders serials
/// the library happens to hold and attaches the canonical display name from
/// `SKYLANDERS_SERIALS`. Return order matches `SKYLANDERS_SERIALS` (release
/// order) so the phone picker is stable across sessions.
fn serials_to_catalogue(serials: &[String]) -> Vec<skylander_core::InstalledGame> {
    skylander_core::SKYLANDERS_SERIALS
        .iter()
        .filter(|(serial, _)| serials.iter().any(|s| s == serial))
        .map(|(serial, display)| skylander_core::InstalledGame {
            serial: skylander_core::GameSerial::new(*serial),
            display_name: (*display).to_string(),
        })
        .collect()
}

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
    const PROD: &[u8] = include_bytes!("../../../phone/assets/icons/icon-192.png");
    const DEV: &[u8] = include_bytes!("../../../phone/assets/icons/icon-dev-192.png");
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
