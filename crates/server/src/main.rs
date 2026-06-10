// Release builds are a pure GUI app (eframe launcher + first-launch wizard),
// so suppress the console window Windows otherwise spawns alongside the egui
// surface — logs go to files (`logging::init`), not stdout. Gated to the
// production build: `dev-tools` (on for `cargo run`) keeps the console so dev
// iteration still sees tracing output. The release lane builds with
// `--no-default-features` (no `dev-tools`), so the attribute takes effect there.
#![cfg_attr(not(feature = "dev-tools"), windows_subsystem = "windows")]
//! Skylander Portal Controller — entry point.
//!
//! Threading model:
//!  - Main OS thread owns the eframe event loop.
//!  - A dedicated background OS thread hosts the tokio multi-threaded runtime
//!    running the Axum server + the driver worker task.
//!  - Shared state lives inside `Arc<AppState>` and an `AtomicUsize` client
//!    counter that both sides read.

use skylander_server::{config, gl_fallback, http, logging, profiles, state, ui};

use std::collections::HashMap;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::AtomicUsize;

use anyhow::{Context, Result};
use skylander_core::{Figure, SLOT_COUNT, SlotState};
use skylander_rpcs3_control::{PortalDriver, RpcsProcess};
use tokio::sync::{Mutex, broadcast};
use tracing::{info, warn};

use crate::config::DriverKind;
use crate::state::{
    AppState, RpcsLifecycle, spawn_connectivity_watchdog, spawn_crash_watchdog,
    spawn_driver_worker, spawn_state_poller,
};
use crate::ui::LauncherApp;

fn main() -> Result<()> {
    // Decide the OpenGL backend BEFORE anything else — `config::load()` can
    // pop the wizard window and the server binds a port; both must happen on
    // the right GL stack. On a GPU-less box (winget validation VM, headless
    // RDP) this RE-EXECS into the bundled Mesa software renderer (and the
    // parent then exits), so the window actually appears instead of failing
    // silently. PLAN 19. Runs before `logging::init` (which needs
    // `cfg.log_dir`), so its decision is logged just below once the
    // subscriber is up; if it re-execs, this call never returns.
    let gl = gl_fallback::bootstrap();

    let cfg = config::load(gl.software).context("load config")?;
    let _log_guard = logging::init(&cfg.log_dir)?;

    info!(gl = %gl.detail, software = gl.software, "graphics backend selected");
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

    // PLAN 9.7 playtest 2026-05-04 — decorate Vehicle figures with their
    // hand-curated Land/Sky/Sea terrain from `data/vehicle_terrain.json`.
    // The terrain isn't derivable from the .sky file or the wiki scrape
    // alone (wiki coverage is patchy: 1/8 vehicles in `data/figures.json`
    // had `terrain` populated). Substring match with longest-key-first
    // so "gold rusher" beats "rusher" for the figure literally named
    // "Power Blue Gold Rusher". Missing/malformed file → log + leave
    // everything `None`; the badge just doesn't render. Non-fatal.
    let figures = decorate_vehicle_terrain(figures, &cfg.data_root);
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
    // Raw-IP form for the PLAN 17.1 connectivity card / 17.4 fallback QR — always
    // the literal IP, even when the QR above uses the `.local` hostname.
    let raw_ip_url = skylander_server::mdns::raw_ip_url(ip, cfg.bind_port, &key_hex);

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
        skylander_server::state::LauncherStatus {
            // Drives the launcher's z-order strategy (PLAN 16.6.2.2): IPC = overlay
            // above the game, never desktop-topmost; UIA = legacy topmost-fighting.
            driver_is_ipc: matches!(cfg.driver_kind, crate::config::DriverKind::Ipc),
            ..Default::default()
        },
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
    let config_dir = cfg.config_dir.clone();
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
                // PLAN 16.10.1 — ensure the user's RPCS3 config.yml carries the
                // one Net configuration Skylanders games run under: Internet
                // *Connected* (so the game's hardcoded 8.8.8.8 DNS probe connects
                // instead of busy-retry-storming the CPU into the freeze-watchdog
                // "flap") + PSN *Disconnected* (so it never hits the RPCN-required
                // fatal) + DNS 8.8.8.8. Surgical: sets just those three keys and
                // preserves every other RPCS3 setting; idempotent. Runs once at
                // startup, well before the first phone-triggered launch reads the
                // config. Real-emulator drivers only — Mock launches nothing.
                // Non-fatal: a write failure just leaves the prior config (the
                // pre-fix behaviour), so log and carry on rather than block boot.
                if !matches!(driver_kind, crate::config::DriverKind::Mock)
                    && let Err(e) =
                        skylander_server::rpcs3_config::ensure_rpcs3_net_config(&config_dir)
                {
                    tracing::warn!("ensure RPCS3 Net config failed (non-fatal): {e}");
                }
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
                // Clone the driver handle for the IPC STATE poller — the worker
                // takes `driver` by value below.
                let driver_for_poller = driver.clone();
                let driver_tx = spawn_driver_worker(
                    driver,
                    portal_for_task.clone(),
                    events_for_task.clone(),
                    profile_store.clone(),
                    sessions.clone(),
                    figures_for_driver.clone(),
                    rpcs3_for_task.clone(),
                    rpcs3_exe.clone(),
                    config_dir.clone(),
                    status_for_task.clone(),
                );
                // Unified crash/freeze supervisor (PLAN 16.7). Polls the
                // lifecycle handle every 500ms; on a dead process (crash) or a
                // raised `frozen` flag (the IPC STATE poller's freeze detector)
                // it runs auto cover → restart-via-BootDirect → restore the
                // portal figures, falling back to the terminal crash overlay
                // only if every restart attempt fails. Clean quits drain
                // `process` in `/api/quit` before the process dies, so the
                // supervisor doesn't fire on those. Needs `driver_tx` (to drive
                // the restart through the worker's IPC/UIA boot path) and the
                // figure library (to re-resolve working copies on restore).
                spawn_crash_watchdog(
                    rpcs3_for_task.clone(),
                    portal_for_task.clone(),
                    events_for_task.clone(),
                    status_for_task.clone(),
                    driver_tx.clone(),
                    figures_for_driver,
                    std::time::Duration::from_millis(500),
                );
                // Playable-signal source depends on the driver. IPC (PLAN 16.6.3.1):
                // the clean STATE poller — `game_playable` waits for compile-complete
                // (not just FPS), so the launcher reveals the game only once it's
                // truly rendering (no early iris / compile→in-game flicker). The
                // legacy UIA fallback sets `game_playable` in-band from BootDirect
                // (which already waits for the rendering `FPS:` viewport before it
                // returns); Mock has no real game underneath, so it stays unset (no
                // iris punch-through) — both correct without the retired FPS/shader
                // title scrapers (PLAN 16.6.3.2/.3).
                match driver_kind {
                    crate::config::DriverKind::Ipc => {
                        spawn_state_poller(
                            driver_for_poller,
                            status_for_task.clone(),
                            std::time::Duration::from_millis(250),
                        );
                    }
                    _ => {
                        let _ = driver_for_poller;
                    }
                }
                // Connectivity watchdog (PLAN 17.1): if no phone connects within
                // a grace window after the QR is up, raise the "Trouble
                // connecting?" diagnostic (raw-IP URL + firewall check/fix). 90s
                // grace, 5s poll — long enough that a user walking over to scan
                // the QR doesn't trip it, short enough to help promptly.
                spawn_connectivity_watchdog(
                    status_for_task.clone(),
                    clients_for_task.clone(),
                    bind.port(),
                    std::time::Duration::from_secs(90),
                    std::time::Duration::from_secs(5),
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
                // PLAN 10.8.4 direct-boot: don't spawn RPCS3 at server
                // startup any more. The launch state is "no game running"
                // until the user picks one in the phone picker; /api/launch
                // then spawns rpcs3.exe with the picked game's EBOOT.BIN
                // and only at that point does RPCS3 come up. This removes
                // the always-running RPCS3 + library-table boot path from
                // PLAN 4.15.16 (the keystroke menu nav was the fragility
                // PLAN 10.8.4 set out to fix).
                //
                // Mock driver still installs a Mock variant so /api/launch
                // can flow through unchanged on Mac/Linux dev.
                if matches!(driver_kind, crate::config::DriverKind::Mock) {
                    let mut guard = rpcs3_for_task.lock().await;
                    guard.process = Some(RpcsProcess::mock());
                    drop(guard);
                    if let Ok(mut st) = status_for_task.lock() {
                        st.rpcs3_running = true;
                    }
                    info!("Mock RPCS3 lifecycle ready");
                }

                // PLAN 10.8.4 — read RPCS3's games.yml so /api/launch can
                // resolve a picked serial to the EBOOT path it needs to spawn
                // RPCS3 directly into the game. Failure here is non-fatal:
                // the catalogue ends up empty, picker shows "no games"
                // and the user can fix games.yml + restart.
                let games_yml = {
                    // Phase 16: read games.yml from config_dir (the data/config root),
                    // which may differ from the exe's location for a patched binary.
                    let install_dir = Some(config_dir.clone());
                    match install_dir {
                        Some(dir) => match skylander_rpcs3_control::games_yml::read_games_yml(&dir)
                        {
                            Ok(map) => {
                                info!(entries = map.len(), "loaded RPCS3 games.yml");
                                map
                            }
                            Err(e) => {
                                warn!("read games.yml failed: {e}");
                                std::collections::HashMap::new()
                            }
                        },
                        None => {
                            warn!("rpcs3 exe has no parent dir; skipping games.yml read");
                            std::collections::HashMap::new()
                        }
                    }
                };

                // Game catalogue: filter games.yml against the supported
                // Skylanders SKYLANDERS_SERIALS list. Mock driver gets the
                // full list so dev/test on Mac/Linux still sees a populated
                // picker without needing a games.yml.
                let games: Vec<skylander_core::InstalledGame> = match driver_kind {
                    // IPC (patched RPCS3) resolves games from games.yml exactly like UIA.
                    crate::config::DriverKind::Uia | crate::config::DriverKind::Ipc => {
                        let serials: Vec<String> = games_yml.keys().cloned().collect();
                        let catalogue = serials_to_catalogue(&serials);
                        info!(
                            installed = catalogue.len(),
                            yml_entries = serials.len(),
                            "loaded Skylanders game catalogue from RPCS3 games.yml",
                        );
                        catalogue
                    }
                    crate::config::DriverKind::Mock => {
                        let all_serials: Vec<String> = skylander_core::SKYLANDERS_SERIALS
                            .iter()
                            .map(|(s, _)| (*s).to_string())
                            .collect();
                        serials_to_catalogue(&all_serials)
                    }
                };

                let state = Arc::new(AppState {
                    figures: figures_for_task,
                    figure_index: figure_index_for_task,
                    driver_kind,
                    driver_tx,
                    portal: portal_for_task,
                    events: events_for_task,
                    connected_clients: clients_for_task,
                    launcher_status: status_for_task,
                    games,
                    games_yml,
                    rpcs3_exe,
                    config_dir,
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

                // Periodic ghost-expiry sweep (PLAN 8.1.4). Ghosts hold
                // their portal slots until either the same profile
                // reconnects with a `?reclaim=` hint, a 3rd connection
                // forces eviction, or this sweep notices the ghost has
                // sat past `GHOST_TIMEOUT`. 60s cadence keeps the
                // wall-clock cleanup latency bounded without burning
                // wakeups on a quiet day.
                {
                    let sweep_state = state.clone();
                    tokio::spawn(async move {
                        let mut tick = tokio::time::interval(std::time::Duration::from_secs(60));
                        // Defaults to Burst — first tick fires immediately,
                        // which would race with startup work; Skip waits a
                        // full period instead.
                        tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
                        loop {
                            tick.tick().await;
                            sweep_state.sweep_expired_ghosts().await;
                        }
                    });
                }

                // Kaos timer ticker (PLAN 8.2b.2). Walks sessions every
                // ~10s; for kaos_enabled profiles, seeds a 20-min
                // warmup on first sight and fires swaps at uniformly-
                // random 1min..=1hr gaps thereafter. Ticker cadence
                // sets the precision on "fire at" — 10s means fires
                // can happen up to ~10s after their scheduled instant,
                // which is fine given the random gap is measured in
                // minutes. Single task, not per-session, so no
                // cancellation plumbing needed when sessions come
                // and go.
                {
                    let kaos_state = state.clone();
                    tokio::spawn(async move {
                        let mut tick = tokio::time::interval(std::time::Duration::from_secs(10));
                        tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
                        loop {
                            tick.tick().await;
                            kaos_state.tick_kaos(std::time::Instant::now()).await;
                        }
                    });
                }

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
    // PLAN 20.3: Desktop window mode (chosen in the wizard / persisted in config,
    // or `.env.dev` WINDOW_MODE=desktop in dev) makes the launcher a normal
    // resizable window; TV mode stays fullscreen + (release) always-on-top. The
    // PLAN 20.1 spike flag (`SKYLANDER_SPIKE_DESKTOP`) also forces windowed and
    // additionally stays opaque so its stand-in window is visible.
    #[cfg(windows)]
    let spike_desktop = skylander_server::spike_desktop::enabled();
    #[cfg(not(windows))]
    let spike_desktop = false;
    let windowed = spike_desktop || matches!(cfg.window_mode, config::WindowMode::Desktop);
    let viewport = {
        let mut vb = egui::ViewportBuilder::default().with_title("Skylander Portal Controller");
        if windowed {
            vb = vb
                .with_inner_size([1100.0, 720.0])
                .with_min_inner_size([480.0, 360.0])
                .with_resizable(true);
        } else {
            vb = vb.with_fullscreen(true);
            if !cfg!(feature = "dev-tools") {
                vb = vb.with_always_on_top();
            }
        }
        // Transparent in every mode except the 20.1 spike (which wants an opaque
        // window so its stand-in is visible). The panel paints an opaque
        // starfield in Main / Crashed / Farewell, so only the in-game path sees
        // through to RPCS3 behind egui — Desktop mode (20.4) needs that too.
        if !spike_desktop {
            vb = vb.with_transparent(true);
        }
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
    #[cfg(windows)]
    if spike_desktop {
        skylander_server::spike_desktop::spawn_standin();
        info!(
            "PLAN 20.1 spike active: windowed launcher + stand-in window (SKYLANDER_SPIKE_DESKTOP)"
        );
    }
    let url_for_ui = phone_url.clone();
    let raw_ip_url_for_ui = raw_ip_url.clone();
    let ui_bind_port = bind.port();
    let ui_window_mode = cfg.window_mode;
    eframe::run_native(
        "skylander-portal-controller",
        native_options,
        Box::new(move |cc| {
            Ok(Box::new(LauncherApp::new(
                cc,
                ui_clients,
                ui_status,
                url_for_ui,
                raw_ip_url_for_ui,
                ui_bind_port,
                ui_window_mode,
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
/// Decorate every `Vehicle`-category figure with a `vehicle_terrain` value
/// from `<data_root>/vehicle_terrain.json`. Substring match against the
/// figure's lowercased canonical name; the lookup table is sorted longest-
/// key-first so "gold rusher" wins over "rusher" when both keys could match
/// the same figure (e.g. "Power Blue Gold Rusher"). Missing file or parse
/// error logs a warning and leaves every figure's `vehicle_terrain` as
/// whatever it was (typically `None`). PLAN 9.7 playtest 2026-05-04.
fn decorate_vehicle_terrain(mut figures: Vec<Figure>, data_root: &Path) -> Vec<Figure> {
    use skylander_core::{Category, VehicleTerrain};

    let path = data_root.join("vehicle_terrain.json");
    let bytes = match std::fs::read(&path) {
        Ok(b) => b,
        Err(e) => {
            tracing::warn!(
                path = %path.display(),
                error = %e,
                "vehicle_terrain.json missing — vehicle badges will not render",
            );
            return figures;
        }
    };

    #[derive(serde::Deserialize)]
    struct File {
        vehicles: HashMap<String, String>,
    }
    let file: File = match serde_json::from_slice(&bytes) {
        Ok(f) => f,
        Err(e) => {
            tracing::warn!(
                path = %path.display(),
                error = %e,
                "vehicle_terrain.json failed to parse — vehicle badges will not render",
            );
            return figures;
        }
    };

    // Sort by key length descending so longer matches (e.g. "gold rusher")
    // get tried before shorter substrings ("rusher"). `String` keys are
    // already lowercased per the file's contract.
    let mut entries: Vec<(String, VehicleTerrain)> = file
        .vehicles
        .into_iter()
        .filter_map(|(k, v)| {
            let terrain = match v.as_str() {
                "Land" => VehicleTerrain::Land,
                "Sky" => VehicleTerrain::Sky,
                "Sea" => VehicleTerrain::Sea,
                other => {
                    tracing::warn!(
                        key = %k,
                        value = %other,
                        "vehicle_terrain.json: unknown terrain value, skipping",
                    );
                    return None;
                }
            };
            Some((k, terrain))
        })
        .collect();
    entries.sort_by_key(|(k, _)| std::cmp::Reverse(k.len()));

    let mut decorated = 0usize;
    for f in figures.iter_mut() {
        if !matches!(f.category, Category::Vehicle) {
            continue;
        }
        let name_lc = f.canonical_name.to_lowercase();
        if let Some((_, terrain)) = entries.iter().find(|(k, _)| name_lc.contains(k)) {
            f.vehicle_terrain = Some(*terrain);
            decorated += 1;
        }
    }
    info!(
        decorated,
        total_vehicles = figures
            .iter()
            .filter(|f| matches!(f.category, Category::Vehicle))
            .count(),
        "decorated vehicle terrains from data/vehicle_terrain.json",
    );
    figures
}

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
        DriverKind::Ipc => {
            // Patched-RPCS3 IPC driver (PLAN 16.5) — cross-platform (no #[cfg(windows)]).
            // It is the production portal path for Phase 16; the no-GUI launch that
            // gives it a socket to talk to lands in 16.6.
            let d = skylander_rpcs3_control::IpcPortalDriver::new()?;
            let arc: Arc<dyn PortalDriver> = Arc::new(d);
            #[cfg(feature = "test-hooks")]
            let mock: TestMockHandle = None;
            #[cfg(not(feature = "test-hooks"))]
            let mock: TestMockHandle = ();
            Ok((arc, mock))
        }
        DriverKind::Mock => {
            // Available under EITHER dev-tools (the standard
            // contributor build) OR mock-driver-runtime (the macOS
            // production build that has no UIA path). Gating Mock
            // behind dev-tools alone would have made the Mac release
            // binary refuse to start (PLAN 10.6).
            #[cfg(any(feature = "dev-tools", feature = "mock-driver-runtime"))]
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
            #[cfg(not(any(feature = "dev-tools", feature = "mock-driver-runtime")))]
            anyhow::bail!(
                "mock driver only available with the dev-tools or mock-driver-runtime feature"
            );
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
