//! Server startup as one call (openspec AA, design decision 2): everything
//! between "config loaded" and the binary's UI.
//!
//! The listener binds before anything with a side effect (RPCS3 Net config
//! write, driver, database), so the port doubles as the single-instance lock.

use std::collections::HashMap;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::AtomicUsize;

use anyhow::{Context, Result};
use axum::Router;
use skylander_core::{Event, Figure, SLOT_COUNT, SlotState};
use skylander_rpcs3_control::{PortalDriver, RpcsProcess};
use sqlx::migrate::Migrator;
use tokio::sync::{Mutex, broadcast, mpsc};
use tracing::{info, warn};

use crate::config::{Config, DriverKind};
use crate::figures::{FigureList, FigureSet, RosterState};
use crate::hooks::Hooks;
use crate::progress::{LevelHero, ProgressSink, ProgressionSlot, spawn_drainer};
use crate::state::{
    AppState, DriverJob, GameList, LauncherScreen, LauncherStatus, RpcsLifecycle,
    spawn_connectivity_watchdog, spawn_crash_watchdog, spawn_driver_worker, spawn_state_poller,
};

/// What [`boot`] needs from the binary.
pub struct BootOptions {
    /// The loaded config; each binary fills it with its own loader.
    pub config: Config,
    /// The binary's runtime-state root; `db.sqlite` sits directly under it,
    /// created only after the listener binds.
    pub runtime_root: PathBuf,
    /// The migration set run on `db.sqlite` (the product passes its own three).
    pub migrator: &'static Migrator,
    /// Builds the portal driver ON the tokio thread, because UIA initializes
    /// COM as MTA there and eframe's main thread needs STA.
    pub driver_factory: DriverFactory,
    /// The product's hooks; all four for the product, none for guacamole.
    pub hooks: Hooks,
    /// Routes merged into the router as they are (guacamole's test hook).
    pub extra_routes: Option<Router<Arc<AppState>>>,
    /// Where the figures and games come from.
    pub catalogue: Catalogue,
    /// The address to bind and build the raw-IP URL from (the QR carries
    /// `<hostname>.local` instead when the OS hostname reads). `None` uses
    /// the first non-loopback IPv4 (else loopback) and listens on every
    /// address, so the port lock holds across a route change.
    pub bind_ip: Option<Ipv4Addr>,
}

/// See [`BootOptions::driver_factory`].
pub type DriverFactory = Box<dyn FnOnce(DriverKind) -> Result<BuiltDriver> + Send>;

/// A driver from a [`DriverFactory`], plus the handle the `/api/_test/*`
/// endpoints inject failures through.
pub struct BuiltDriver {
    pub driver: Arc<dyn PortalDriver>,
    #[cfg(feature = "test-hooks")]
    pub test_injector: Option<Arc<dyn skylander_rpcs3_control::LoadInjector>>,
}

impl BuiltDriver {
    pub fn new(driver: Arc<dyn PortalDriver>) -> Self {
        Self {
            driver,
            #[cfg(feature = "test-hooks")]
            test_injector: None,
        }
    }

    /// A driver the test endpoints can also inject failures into.
    #[cfg(feature = "test-hooks")]
    pub fn with_injector<D>(driver: Arc<D>) -> Self
    where
        D: PortalDriver + skylander_rpcs3_control::LoadInjector + 'static,
    {
        Self {
            driver: driver.clone(),
            test_injector: Some(driver),
        }
    }
}

/// Where the figure library and the game picker's list come from.
pub enum Catalogue {
    /// The product's: the firmware pack indexed by `scan` (the caller's
    /// `skylander_indexer::scan`, kept out because it links the `.sky` parser)
    /// plus vehicle terrains, and games from RPCS3's `games.yml` (Mock: all).
    FirmwarePack {
        scan: fn(&Path) -> Result<Vec<Figure>>,
    },
    /// guacamole's (`DriverKind::Native`): no figures until the binary
    /// publishes the disc roster ([`ServerHandles::figures`], AE.4), `roster`
    /// saying why meanwhile, and the game list setup found. Reads no pack, no
    /// `games.yml` and nothing under `data_root`.
    Native {
        games: Vec<skylander_core::InstalledGame>,
        roster: RosterState,
    },
}

/// What the binary's UI reads once the server is up.
pub struct ServerHandles {
    /// Polled by the UI each frame (sync mutex: the UI loop can't `await`).
    pub launcher_status: Arc<std::sync::Mutex<LauncherStatus>>,
    pub connected_clients: Arc<AtomicUsize>,
    /// The QR's URL: `<hostname>.local` when the OS hostname reads, else the IP.
    pub phone_url: String,
    /// Always the literal IP (the connectivity card and fallback QR).
    pub raw_ip_url: String,
    /// The round-QR PNG of `phone_url`, the same buffer `/api/join-qr.png` serves.
    pub join_qr_png: Arc<Vec<u8>>,
    pub events: broadcast::Sender<Event>,
    /// The phone's game list (`/api/games`); guacamole replaces it after setup.
    pub games: GameList,
    /// The figure list; guacamole publishes the disc roster into it (AE.4).
    pub figures: FigureList,
    /// Where guacamole's TV hands over a figure's progress (AH.1); drained
    /// into the store only on guacamole's arm, so the product's never is.
    pub progress: ProgressSink,
    /// The game's progression for the store's caps and levels, once the
    /// extract holds it.
    pub progression: ProgressionSlot,
    /// Which slot's figure is the hero in a level; guacamole's TV writes it
    /// (AH.36) and the slot reset refuses that slot (AH.3).
    pub level_hero: LevelHero,
    /// The driver worker's job queue (PLAN B.2's macOS game-window fit);
    /// closed when the server died before the worker started.
    pub driver_tx: mpsc::Sender<DriverJob>,
    pub bind_addr: SocketAddr,
    /// The tokio thread; it exits early only after a fatal startup error.
    pub server_thread: std::thread::JoinHandle<()>,
}

/// Start the server; returns once the driver worker runs or the tokio thread
/// gave up (reported as `LauncherScreen::ServerError`, not as `Err`).
/// `Err` is only what fails before the thread starts: indexing the pack, or a
/// catalogue that doesn't fit the driver kind.
pub fn boot(options: BootOptions) -> Result<ServerHandles> {
    let BootOptions {
        config: cfg,
        runtime_root,
        migrator,
        driver_factory,
        hooks,
        extra_routes,
        catalogue,
        bind_ip,
    } = options;

    let figures = match (&catalogue, cfg.driver_kind) {
        (
            Catalogue::FirmwarePack { scan },
            DriverKind::Uia | DriverKind::Ipc | DriverKind::Mock,
        ) => FigureSet::pack(index_firmware_pack(&cfg, *scan)?),
        (Catalogue::Native { roster, .. }, DriverKind::Native) => FigureSet::waiting(*roster),
        (Catalogue::FirmwarePack { .. }, DriverKind::Native) => {
            anyhow::bail!("the native driver has no firmware pack to index")
        }
        (Catalogue::Native { .. }, kind) => {
            anyhow::bail!("the native catalogue needs the native driver, not {kind:?}")
        }
    };

    let figures = FigureList::new(figures);

    // --- Pick bind address. ---
    let ip = bind_ip
        .or_else(first_non_loopback_ipv4)
        .unwrap_or(Ipv4Addr::LOCALHOST);
    let bind = SocketAddr::from((ip, cfg.bind_port));
    let listen = match bind_ip {
        Some(_) => bind,
        None => SocketAddr::from((Ipv4Addr::UNSPECIFIED, cfg.bind_port)),
    };
    let key_hex = hex::encode(&cfg.hmac_key);

    // `<os-hostname>.local` survives a DHCP-lease refresh (PLAN 4.18.1a / 4.19.10b; `mdns` has the history).
    let (phone_url, used_mdns) = crate::mdns::build_phone_url(ip, cfg.bind_port, &key_hex);
    if used_mdns {
        info!("phone URL {phone_url}");
    } else {
        tracing::warn!("OS hostname unavailable; QR will use the raw-IP URL: {phone_url}");
    }
    // Always the literal IP, for the PLAN 17.1 connectivity card / 17.4 fallback QR.
    let raw_ip_url = crate::mdns::raw_ip_url(ip, cfg.bind_port, &key_hex);

    // Rendered once: the launcher and `/api/join-qr.png` share this buffer.
    let join_qr_png = match crate::round_qr::render_png(
        &phone_url,
        &crate::round_qr::RoundQrConfig::launcher_default(),
    ) {
        Ok(bytes) => Arc::new(bytes),
        Err(e) => {
            tracing::error!("render join QR PNG: {e}");
            Arc::new(Vec::new())
        }
    };

    // --- Shared between Axum and the launcher UI. ---
    let portal: Arc<Mutex<[SlotState; SLOT_COUNT]>> =
        Arc::new(Mutex::new(std::array::from_fn(|_| SlotState::Empty)));
    let (events_tx, _) = broadcast::channel::<Event>(64);
    let connected_clients = Arc::new(AtomicUsize::new(0));
    let launcher_status = Arc::new(std::sync::Mutex::new(LauncherStatus {
        // Drives the launcher's z-order strategy (PLAN 16.6.2.2): IPC = overlay
        // above the game, never desktop-topmost; UIA = legacy topmost-fighting.
        driver_is_ipc: matches!(cfg.driver_kind, DriverKind::Ipc),
        ..Default::default()
    }));

    // --- Start the Axum server + driver worker on a dedicated thread. ---
    let phone_dist = cfg.phone_dist_dir.clone();
    let bind_addr = bind;
    let listen_addr = listen;
    let driver_kind = cfg.driver_kind;
    let rpcs3_exe = cfg.rpcs3_exe.clone();
    let config_dir = cfg.config_dir.clone();
    let data_root = cfg.data_root.clone();
    let hmac_key = cfg.hmac_key.clone();
    // Read back by the admin window-mode toggle (PLAN 20.6).
    let window_mode = cfg.window_mode;
    // Read back by the admin 2× toggle (PLAN S); the product exports SKYLANDER_SURFACE_2X itself.
    let render_2x = cfg.render_2x;
    let rpcs3_lifecycle = Arc::new(tokio::sync::Mutex::new(RpcsLifecycle::default()));
    let rpcs3_for_task = rpcs3_lifecycle.clone();
    let portal_for_task = portal.clone();
    let events_for_task = events_tx.clone();
    let clients_for_task = connected_clients.clone();
    let status_for_task = launcher_status.clone();
    let figures_for_task = figures.clone();
    let join_qr_png_for_task = join_qr_png.clone();
    // Native's list is known now; the product's is read from games.yml on the thread.
    let games: GameList = Arc::new(std::sync::RwLock::new(match &catalogue {
        Catalogue::Native { games, .. } => games.clone(),
        Catalogue::FirmwarePack { .. } => Vec::new(),
    }));
    let games_for_task = games.clone();
    let progress = ProgressSink::new();
    let progress_for_task = progress.clone();
    let progression = ProgressionSlot::default();
    let progression_for_task = progression.clone();
    let progression_for_state = progression.clone();
    let level_hero = LevelHero::default();
    let level_hero_for_state = level_hero.clone();

    let status_for_errors = launcher_status.clone();
    let status_for_ready = launcher_status.clone();
    // PLAN B.2: the worker's sender is born on the tokio thread and shipped back here for the UI.
    let (driver_tx_back_tx, driver_tx_back_rx) =
        std::sync::mpsc::sync_channel::<mpsc::Sender<DriverJob>>(1);
    let server_thread = std::thread::Builder::new()
        .name("tokio".into())
        .spawn(move || {
            let rt = tokio::runtime::Builder::new_multi_thread()
                .enable_all()
                .build()
                .expect("build tokio runtime");
            // Log and flip the launcher to ServerError; callers then return and the UI shows it.
            let report_fatal = |scope: &str, err: &dyn std::fmt::Display| {
                let message = fatal_message(scope, err);
                tracing::error!("{message}");
                if let Ok(mut st) = status_for_errors.lock() {
                    st.screen = LauncherScreen::ServerError { message };
                }
            };
            rt.block_on(async move {
                // First, so a second copy (port taken) stops before any side effect.
                let listener = match tokio::net::TcpListener::bind(listen_addr).await {
                    Ok(l) => l,
                    Err(e) => {
                        if e.kind() == std::io::ErrorKind::AddrInUse
                            && let Ok(mut st) = status_for_ready.lock()
                        {
                            st.port_in_use = true;
                        }
                        report_fatal(&format!("bind {listen_addr}"), &e);
                        return;
                    }
                };
                // PLAN 16.10.1: Internet Connected, PSN Disconnected, DNS 8.8.8.8; real emulators only, non-fatal.
                let real_emulator = match driver_kind {
                    DriverKind::Uia | DriverKind::Ipc => true,
                    DriverKind::Mock | DriverKind::Native => false,
                };
                if real_emulator
                    && let Some(rpcs3_config) = &hooks.rpcs3_config
                    && let Err(e) = rpcs3_config.ensure_net_config(&config_dir)
                {
                    tracing::warn!("ensure RPCS3 Net config failed (non-fatal): {e}");
                }
                let built = match driver_factory(driver_kind) {
                    Ok(d) => d,
                    Err(e) => {
                        report_fatal("failed to construct driver", &e);
                        return;
                    }
                };
                let driver = built.driver;
                let db_path = match crate::paths::db_path_under(&runtime_root) {
                    Ok(p) => p,
                    Err(e) => {
                        report_fatal("resolve db path", &e);
                        return;
                    }
                };
                info!(db = %db_path.display(), "opening profile db");
                let profile_store =
                    match crate::profiles::ProfileStore::open(&db_path, migrator).await {
                        Ok(s) => s,
                        Err(e) => {
                            report_fatal("open profile store", &e);
                            return;
                        }
                    };
                let sessions = Arc::new(crate::profiles::SessionRegistry::default());
                // Figure progress lives in guacamole's own store: its arm alone (AH.1).
                if hooks.figure_files.is_none() {
                    spawn_drainer(
                        progress_for_task,
                        profile_store.clone(),
                        events_for_task.clone(),
                        progression_for_task,
                    );
                }
                // The IPC STATE poller's copy; the worker takes `driver` by value.
                let driver_for_poller = driver.clone();
                let driver_tx = spawn_driver_worker(
                    driver,
                    portal_for_task.clone(),
                    events_for_task.clone(),
                    profile_store.clone(),
                    sessions.clone(),
                    figures_for_task.clone(),
                    rpcs3_for_task.clone(),
                    rpcs3_exe.clone(),
                    config_dir.clone(),
                    status_for_task.clone(),
                    hooks.rpcs3_config.clone(),
                );
                // PLAN B.2, best-effort: a gone receiver just drops it.
                let _ = driver_tx_back_tx.send(driver_tx.clone());
                // Crash/freeze supervisor (PLAN 16.7): cover, restart via BootDirect, restore the portal.
                spawn_crash_watchdog(
                    rpcs3_for_task.clone(),
                    portal_for_task.clone(),
                    events_for_task.clone(),
                    status_for_task.clone(),
                    driver_tx.clone(),
                    figures_for_task.clone(),
                    hooks.figure_files.clone(),
                    driver_kind,
                    std::time::Duration::from_millis(500),
                );
                // Only IPC polls STATE for `game_playable` (PLAN 16.6.3.1); UIA sets it in BootDirect, Mock never.
                match driver_kind {
                    DriverKind::Ipc => {
                        spawn_state_poller(
                            driver_for_poller,
                            status_for_task.clone(),
                            std::time::Duration::from_millis(250),
                        );
                    }
                    DriverKind::Uia | DriverKind::Mock | DriverKind::Native => {
                        let _ = driver_for_poller;
                    }
                }
                // PLAN 17.1: the connectivity diagnostic after 90s with no phone, polled every 5s.
                spawn_connectivity_watchdog(
                    status_for_task.clone(),
                    clients_for_task.clone(),
                    bind.port(),
                    std::time::Duration::from_secs(90),
                    std::time::Duration::from_secs(5),
                );
                // RPCS3 spawns on /api/launch (PLAN 10.8.4); Mock just installs a stand-in process
                // and Native tracks none, ever.
                match driver_kind {
                    DriverKind::Mock => {
                        let mut guard = rpcs3_for_task.lock().await;
                        guard.process = Some(RpcsProcess::mock());
                        drop(guard);
                        if let Ok(mut st) = status_for_task.lock() {
                            st.rpcs3_running = true;
                        }
                        info!("Mock RPCS3 lifecycle ready");
                    }
                    DriverKind::Uia | DriverKind::Ipc | DriverKind::Native => {}
                }

                let games_yml = match catalogue {
                    Catalogue::FirmwarePack { .. } => {
                        let (list, yml) = games_from_games_yml(driver_kind, &config_dir);
                        *games_for_task.write().unwrap_or_else(|e| e.into_inner()) = list;
                        yml
                    }
                    Catalogue::Native { .. } => HashMap::new(),
                };

                let state = Arc::new(AppState {
                    figures: figures_for_task,
                    held_resumes: Mutex::new(Vec::new()),
                    driver_kind,
                    driver_tx,
                    portal: portal_for_task,
                    events: events_for_task,
                    connected_clients: clients_for_task,
                    launcher_status: status_for_task,
                    games: games_for_task,
                    games_yml,
                    rpcs3_exe,
                    config_dir,
                    data_root,
                    phone_dist: phone_dist.clone(),
                    hmac_key,
                    window_mode,
                    render_2x,
                    boot_id: {
                        // Phones compare it with the last-seen one to detect a restart.
                        use rand_core::RngCore;
                        rand_core::OsRng.next_u64()
                    },
                    rpcs3: rpcs3_for_task,
                    profiles: profile_store,
                    sessions,
                    join_qr_png: join_qr_png_for_task,
                    hooks,
                    progression: progression_for_state,
                    level_hero: level_hero_for_state,
                    #[cfg(feature = "test-hooks")]
                    test_injector: built.test_injector,
                });

                // Ghost-expiry sweep every 60s (PLAN 8.1.4).
                {
                    let sweep_state = state.clone();
                    tokio::spawn(async move {
                        let mut tick = tokio::time::interval(std::time::Duration::from_secs(60));
                        // Skip, not Burst: the first tick waits a full period instead of racing startup.
                        tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
                        loop {
                            tick.tick().await;
                            sweep_state.sweep_expired_ghosts().await;
                        }
                    });
                }

                // guacamole's held resume prompts go out when the roster lands (AE.4).
                crate::http::spawn_resume_release(state.clone());

                // Kaos timer ticker every ~10s (PLAN 8.2b.2): one task, not per-session.
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

                let mut app = crate::http::router(state.clone(), phone_dist);
                if let Some(extra) = extra_routes {
                    app = app.merge(extra.with_state(state.clone()));
                }

                // The e2e harness reads the first token after "serving on".
                info!("serving on http://{bind_addr} (listening on {listen_addr})");
                // Ungates the launcher's intro animations (PLAN 4.19.x).
                if let Ok(mut st) = status_for_ready.lock() {
                    st.server_ready = true;
                }
                if let Err(e) = axum::serve(listener, app).await {
                    // The QR no longer points at a live server, so this is fatal too.
                    report_fatal("axum server exited", &e);
                }
            });
        })
        .expect("spawn server thread");

    // A fatal startup path drops the sender; a closed dummy channel lets the UI still come up.
    let driver_tx = driver_tx_back_rx.recv().unwrap_or_else(|_| {
        let (tx, _rx) = mpsc::channel::<DriverJob>(1);
        tx
    });

    Ok(ServerHandles {
        launcher_status,
        connected_clients,
        phone_url,
        raw_ip_url,
        join_qr_png,
        events: events_tx,
        games,
        figures,
        progress,
        progression,
        level_hero,
        driver_tx,
        bind_addr: bind,
        server_thread,
    })
}

/// The product's figure library: the firmware pack, indexed and decorated.
/// A startup failure as the log and the ServerError screen show it: the scope,
/// then every cause in the chain (`{:#}` on an `anyhow::Error`, which `{}`
/// cuts to the outermost context).
fn fatal_message(scope: &str, err: &dyn std::fmt::Display) -> String {
    format!("{scope}: {err:#}")
}

fn index_firmware_pack(
    cfg: &Config,
    scan: fn(&Path) -> Result<Vec<Figure>>,
) -> Result<Vec<Figure>> {
    // No pack is an empty library, which the phone handles (PLAN 6.5.4).
    let figures: Vec<Figure> = if cfg.firmware_pack_root.as_os_str().is_empty() {
        info!("no firmware pack configured — starting with an empty library");
        Vec::new()
    } else {
        scan(&cfg.firmware_pack_root).context("scan firmware pack")?
    };
    info!(count = figures.len(), "indexed pack figures");

    // Vehicle terrain isn't in the .sky file or the wiki scrape (PLAN 9.7), so it's hand-curated.
    let figures = decorate_vehicle_terrain(figures, &cfg.data_root);
    info!(count = figures.len(), "total library figures");
    Ok(figures)
}

/// The product's game picker: `games.yml` filtered to the Skylanders titles
/// (Mock: all of them), plus the raw serial → game-dir map for `/api/launch`.
fn games_from_games_yml(
    driver_kind: DriverKind,
    config_dir: &Path,
) -> (Vec<skylander_core::InstalledGame>, HashMap<String, PathBuf>) {
    // `boot` pairs Native with `Catalogue::Native`; never read games.yml for it.
    if matches!(driver_kind, DriverKind::Native) {
        return (Vec::new(), HashMap::new());
    }
    // Non-fatal (PLAN 10.8.4): a failed read is an empty picker until games.yml is fixed.
    let games_yml = match skylander_rpcs3_control::games_yml::read_games_yml(config_dir) {
        Ok(map) => {
            info!(entries = map.len(), "loaded RPCS3 games.yml");
            map
        }
        Err(e) => {
            warn!("read games.yml failed: {e}");
            HashMap::new()
        }
    };

    // Mock lists every title so dev on Mac/Linux has a picker without a games.yml.
    let games: Vec<skylander_core::InstalledGame> = match driver_kind {
        DriverKind::Uia | DriverKind::Ipc => {
            let serials: Vec<String> = games_yml.keys().cloned().collect();
            let catalogue = serials_to_catalogue(&serials);
            info!(
                installed = catalogue.len(),
                yml_entries = serials.len(),
                "loaded Skylanders game catalogue from RPCS3 games.yml",
            );
            catalogue
        }
        DriverKind::Native => Vec::new(),
        DriverKind::Mock => {
            let all_serials: Vec<String> = skylander_core::SKYLANDERS_SERIALS
                .iter()
                .map(|(s, _)| (*s).to_string())
                .collect();
            serials_to_catalogue(&all_serials)
        }
    };
    (games, games_yml)
}

/// Set each vehicle's terrain from `<data_root>/vehicle_terrain.json`, longest
/// key first ("gold rusher" beats "rusher"); a missing or bad file is a warning.
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

    // Longest key first; keys are lowercase by the file's contract.
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

/// The Skylanders titles among `serials`, with display names, in
/// `SKYLANDERS_SERIALS` (release) order so the picker is stable.
fn serials_to_catalogue(serials: &[String]) -> Vec<skylander_core::InstalledGame> {
    skylander_core::SKYLANDERS_SERIALS
        .iter()
        .filter(|(serial, _)| serials.iter().any(|s| s == serial))
        .map(|(serial, display)| skylander_core::InstalledGame {
            serial: skylander_core::GameSerial::new(*serial),
            display_name: (*display).to_string(),
            art: None,
        })
        .collect()
}

fn first_non_loopback_ipv4() -> Option<Ipv4Addr> {
    match local_ip_address::local_ip() {
        Ok(IpAddr::V4(v4)) if !v4.is_loopback() => Some(v4),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::time::{Duration, Instant};

    static TEST_MIGRATOR: Migrator = sqlx::migrate!("../server/migrations");

    const KEY: [u8; 32] = [7; 32];

    /// A mock-driver config under `root`: no pack, no RPCS3.
    fn mock_config(root: &Path, port: u16) -> Config {
        Config {
            rpcs3_exe: PathBuf::new(),
            config_dir: root.join("rpcs3"),
            firmware_pack_root: PathBuf::new(),
            bind_port: port,
            driver_kind: DriverKind::Mock,
            window_mode: crate::config::WindowMode::Desktop,
            render_2x: false,
            log_dir: root.join("logs"),
            phone_dist_dir: root.join("phone-dist"),
            data_root: root.join("data"),
            hmac_key: KEY.to_vec(),
        }
    }

    fn boot_with(
        config: Config,
        runtime_root: &Path,
        driver_factory: DriverFactory,
        hooks: Hooks,
    ) -> ServerHandles {
        boot(BootOptions {
            config,
            runtime_root: runtime_root.to_path_buf(),
            migrator: &TEST_MIGRATOR,
            driver_factory,
            hooks,
            extra_routes: None,
            catalogue: Catalogue::FirmwarePack {
                scan: |_| anyhow::bail!("no pack is configured, so this never runs"),
            },
            bind_ip: Some(Ipv4Addr::LOCALHOST),
        })
        .expect("boot")
    }

    fn mock_boot(runtime_root: &Path, port: u16) -> ServerHandles {
        boot_with(
            mock_config(runtime_root, port),
            runtime_root,
            Box::new(|_| {
                Ok(BuiltDriver::new(Arc::new(
                    skylander_rpcs3_control::MockPortalDriver::new(),
                )))
            }),
            Hooks::default(),
        )
    }

    /// Records a Net-config write; a boot test never launches, so the save-state swap is a no-op.
    struct NetConfigProbe(Arc<AtomicBool>);

    impl crate::hooks::Rpcs3Config for NetConfigProbe {
        fn ensure_net_config(&self, _: &Path) -> Result<()> {
            self.0.store(true, Ordering::SeqCst);
            Ok(())
        }

        fn apply_savestate_config(&self, _: &Path) -> Result<Box<dyn Send>> {
            Ok(Box::new(()))
        }
    }

    /// A port the OS just handed out and released.
    fn ephemeral_port() -> u16 {
        std::net::TcpListener::bind((Ipv4Addr::LOCALHOST, 0))
            .and_then(|l| l.local_addr())
            .expect("ephemeral port")
            .port()
    }

    fn wait_until_ready(handles: &ServerHandles) {
        let deadline = Instant::now() + Duration::from_secs(20);
        while !handles.launcher_status.lock().unwrap().server_ready {
            assert!(Instant::now() < deadline, "server never became ready");
            std::thread::sleep(Duration::from_millis(20));
        }
    }

    /// Signed, so it passes with or without `dev-tools`.
    fn get_version(addr: SocketAddr) -> String {
        get(addr, "/api/version")
    }

    fn get(addr: SocketAddr, path: &str) -> String {
        let ts = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis() as u64;
        let sig = hex::encode(crate::http::compute_hmac(&KEY, ts, "GET", path, b""));
        let mut stream = std::net::TcpStream::connect(addr).expect("connect");
        write!(
            stream,
            "GET {path} HTTP/1.1\r\nHost: {addr}\r\nX-Skyportal-Timestamp: {ts}\r\n\
             X-Skyportal-Sig: {sig}\r\nConnection: close\r\n\r\n"
        )
        .unwrap();
        let mut response = String::new();
        stream.read_to_string(&mut response).expect("read response");
        response
    }

    #[test]
    fn mock_boot_serves_api_version() {
        let root = tempfile::tempdir().unwrap();
        let handles = mock_boot(root.path(), ephemeral_port());
        wait_until_ready(&handles);

        let response = get_version(handles.bind_addr);
        assert!(response.starts_with("HTTP/1.1 200"), "{response}");
        let token = format!("\"build_token\":\"{}\"", crate::http::BUILD_TOKEN);
        assert!(response.contains(&token), "{response}");
        assert!(root.path().join("db.sqlite").is_file());
    }

    #[test]
    fn a_fatal_message_keeps_every_cause() {
        let e = anyhow::anyhow!("migration 3 was previously applied but has been modified")
            .context("run migrations");
        assert_eq!(
            fatal_message("open profile store", &e),
            "open profile store: run migrations: migration 3 was previously applied but has been modified"
        );
        // A plain error reads as before.
        let io = std::io::Error::other("Address already in use");
        assert_eq!(
            fatal_message("bind 0.0.0.0:8765", &io),
            "bind 0.0.0.0:8765: Address already in use"
        );
    }

    /// AA.21's case: a database whose applied migration no longer matches.
    #[test]
    fn a_changed_migration_shows_its_cause_on_the_error_screen() {
        let root = tempfile::tempdir().unwrap();
        let db = root.path().join("db.sqlite");
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap()
            .block_on(async {
                let store = crate::profiles::ProfileStore::open(&db, &TEST_MIGRATOR)
                    .await
                    .unwrap();
                sqlx::query("UPDATE _sqlx_migrations SET checksum = X'00'")
                    .execute(&store.pool)
                    .await
                    .unwrap();
                store.pool.close().await;
            });
        let handles = mock_boot(root.path(), ephemeral_port());
        handles.server_thread.join().unwrap();
        let screen = handles.launcher_status.lock().unwrap().screen.clone();
        let LauncherScreen::ServerError { message } = screen else {
            panic!("expected ServerError, got {screen:?}");
        };
        assert!(
            message.starts_with("open profile store: run migrations: migration "),
            "{message}"
        );
        assert!(
            message.ends_with("was previously applied but has been modified"),
            "{message}"
        );
    }

    #[test]
    fn second_boot_on_the_same_port_fails_the_bind_before_any_side_effect() {
        let first_root = tempfile::tempdir().unwrap();
        let first = mock_boot(first_root.path(), ephemeral_port());
        wait_until_ready(&first);

        let second_parent = tempfile::tempdir().unwrap();
        let second_root = second_parent.path().join("root");
        // IPC, so the Net-config write isn't skipped the way it is under Mock.
        let mut config = mock_config(&second_root, first.bind_addr.port());
        config.driver_kind = DriverKind::Ipc;
        let built = Arc::new(AtomicBool::new(false));
        let net_config = Arc::new(AtomicBool::new(false));
        let built_flag = built.clone();
        let second = boot_with(
            config,
            &second_root,
            Box::new(move |_| {
                built_flag.store(true, Ordering::SeqCst);
                Ok(BuiltDriver::new(Arc::new(
                    skylander_rpcs3_control::MockPortalDriver::new(),
                )))
            }),
            Hooks {
                rpcs3_config: Some(Arc::new(NetConfigProbe(net_config.clone()))),
                ..Hooks::default()
            },
        );
        second.server_thread.join().unwrap();
        let screen = second.launcher_status.lock().unwrap().screen.clone();
        let LauncherScreen::ServerError { message } = screen else {
            panic!("expected ServerError, got {screen:?}");
        };
        assert!(
            message.starts_with(&format!("bind {}", first.bind_addr)),
            "{message}"
        );
        assert!(second.launcher_status.lock().unwrap().port_in_use);
        assert!(!second_root.join("db.sqlite").exists());
        assert!(!second_root.exists(), "the runtime root was created");
        assert!(second.driver_tx.is_closed());
        assert!(!built.load(Ordering::SeqCst), "the driver was built");
        assert!(
            !net_config.load(Ordering::SeqCst),
            "the Net config was written"
        );

        // The first copy keeps serving.
        assert!(get_version(first.bind_addr).starts_with("HTTP/1.1 200"));
    }

    #[test]
    fn a_default_boot_listens_everywhere_so_a_second_one_loses_the_port() {
        let port = ephemeral_port();
        let unbound = |root: &Path| {
            boot(BootOptions {
                config: mock_config(root, port),
                runtime_root: root.to_path_buf(),
                migrator: &TEST_MIGRATOR,
                driver_factory: Box::new(|_| {
                    Ok(BuiltDriver::new(Arc::new(
                        skylander_rpcs3_control::MockPortalDriver::new(),
                    )))
                }),
                hooks: Hooks::default(),
                extra_routes: None,
                catalogue: Catalogue::FirmwarePack {
                    scan: |_| anyhow::bail!("no pack is configured, so this never runs"),
                },
                bind_ip: None,
            })
            .expect("boot")
        };
        let first_root = tempfile::tempdir().unwrap();
        let first = unbound(first_root.path());
        wait_until_ready(&first);
        // Loopback answers, so the listener isn't tied to the QR's address.
        let loopback = SocketAddr::from((Ipv4Addr::LOCALHOST, port));
        assert!(get_version(loopback).starts_with("HTTP/1.1 200"));

        let second_root = tempfile::tempdir().unwrap();
        let second = unbound(second_root.path());
        second.server_thread.join().unwrap();
        let status = second.launcher_status.lock().unwrap().clone();
        assert!(status.port_in_use, "{:?}", status.screen);
        let LauncherScreen::ServerError { message } = status.screen else {
            panic!("expected ServerError, got {:?}", status.screen);
        };
        assert!(
            message.starts_with(&format!("bind 0.0.0.0:{port}")),
            "{message}"
        );
        assert!(get_version(loopback).starts_with("HTTP/1.1 200"));
    }

    fn native_boot(root: &Path, catalogue: Catalogue) -> Result<ServerHandles> {
        let mut config = mock_config(root, ephemeral_port());
        config.driver_kind = DriverKind::Native;
        boot(BootOptions {
            config,
            runtime_root: root.to_path_buf(),
            migrator: &crate::setup::MIGRATOR,
            driver_factory: Box::new(|_| {
                Ok(BuiltDriver::new(Arc::new(
                    skylander_rpcs3_control::MockPortalDriver::new(),
                )))
            }),
            hooks: Hooks::default(),
            extra_routes: None,
            catalogue,
            bind_ip: Some(Ipv4Addr::LOCALHOST),
        })
    }

    fn giants() -> Vec<skylander_core::InstalledGame> {
        vec![skylander_core::InstalledGame {
            serial: skylander_core::GameSerial::new("BLUS30968"),
            display_name: "Skylanders: Giants".into(),
            art: None,
        }]
    }

    fn native_catalogue(games: Vec<skylander_core::InstalledGame>) -> Catalogue {
        Catalogue::Native {
            games,
            roster: RosterState::Reading,
        }
    }

    /// The JSON body of a 200.
    fn json(response: &str) -> serde_json::Value {
        assert!(response.starts_with("HTTP/1.1 200"), "{response}");
        let body = response.split("\r\n\r\n").nth(1).unwrap();
        serde_json::from_str(body).unwrap_or_else(|e| panic!("{e}: {body}"))
    }

    /// AE.4: boot reads no catalogue (there's no `data/` under the root); the
    /// roster published after boot shows on `/api/figures` at once.
    #[test]
    fn a_native_boot_lists_no_figures_until_the_roster_is_published() {
        let root = tempfile::tempdir().unwrap();
        let handles = native_boot(root.path(), native_catalogue(giants())).unwrap();
        wait_until_ready(&handles);
        assert!(!root.path().join("data").exists());

        assert_eq!(
            json(&get(handles.bind_addr, "/api/figures")),
            serde_json::json!([])
        );
        let status = json(&get(handles.bind_addr, "/api/status"));
        assert_eq!(status["roster"], "reading", "{status}");

        let roster = crate::figures::fixture::eruptor_and_spyro();
        handles
            .figures
            .publish(FigureSet::from_roster(&roster, root.path()));
        let figures = json(&get(handles.bind_addr, "/api/figures"));
        let ids: Vec<&str> = figures
            .as_array()
            .unwrap()
            .iter()
            .map(|f| f["id"].as_str().unwrap())
            .collect();
        assert_eq!(ids, ["000009-0000", "000010-0000"]);
        assert_eq!(figures[0]["canonical_name"], "Eruptor");
        let status = json(&get(handles.bind_addr, "/api/status"));
        assert_eq!(status["roster"], "ready", "{status}");
        let games = get(handles.bind_addr, "/api/games");
        assert!(games.contains("BLUS30968"), "{games}");
    }

    #[test]
    fn a_native_game_list_replaced_after_boot_is_served() {
        let root = tempfile::tempdir().unwrap();
        let handles = native_boot(root.path(), native_catalogue(Vec::new())).unwrap();
        wait_until_ready(&handles);
        assert!(!get(handles.bind_addr, "/api/games").contains("BLUS30968"));

        *handles.games.write().unwrap() = giants();
        let games = get(handles.bind_addr, "/api/games");
        assert!(games.contains("BLUS30968"), "{games}");
    }

    /// AE.4: guacamole's `/api/status` names where its roster stands; the
    /// product's carries no roster at all.
    #[test]
    fn each_roster_state_shows_on_guacamoles_status_and_none_on_the_products() {
        let root = tempfile::tempdir().unwrap();
        let catalogue = Catalogue::Native {
            games: giants(),
            roster: RosterState::Unrecognized,
        };
        let handles = native_boot(root.path(), catalogue).unwrap();
        wait_until_ready(&handles);
        for (state, wire) in [
            (None, "unrecognized"),
            (Some(RosterState::Reading), "reading"),
            (Some(RosterState::Unreadable), "unreadable"),
            (Some(RosterState::Unrecognized), "unrecognized"),
        ] {
            if let Some(state) = state {
                handles.figures.set_roster_state(state);
            }
            let status = json(&get(handles.bind_addr, "/api/status"));
            assert_eq!(status["roster"], wire, "{status}");
            assert_eq!(status["app"], "guacamole", "{status}");
        }
        handles.figures.publish(FigureSet::from_roster(
            &crate::figures::fixture::eruptor_and_spyro(),
            root.path(),
        ));
        let status = json(&get(handles.bind_addr, "/api/status"));
        assert_eq!(status["roster"], "ready", "{status}");

        let product_root = tempfile::tempdir().unwrap();
        let product = mock_boot(product_root.path(), ephemeral_port());
        wait_until_ready(&product);
        let status = json(&get(product.bind_addr, "/api/status"));
        assert_eq!(status["app"], "product", "{status}");
        assert!(status.get("roster").is_none(), "{status}");
    }

    /// The product's figure-file hook, never called here.
    struct WorkingCopies;

    impl crate::hooks::FigureFiles for WorkingCopies {
        fn routes(&self) -> Router<Arc<AppState>> {
            Router::new()
        }

        fn resolve_load_path(&self, _: &str, _: &Figure) -> Result<PathBuf> {
            anyhow::bail!("no working copies in a boot test")
        }

        fn reset_to_fresh(&self, _: &str, _: &Figure) -> Result<PathBuf> {
            anyhow::bail!("no working copies in a boot test")
        }
    }

    /// AH.1: guacamole's boot drains what its TV sends into its own store,
    /// and the quit's flush waits for it; the product's runs no drainer.
    #[test]
    fn guacamoles_boot_drains_figure_progress_and_the_products_runs_none() {
        let root = tempfile::tempdir().unwrap();
        let handles = native_boot(root.path(), native_catalogue(giants())).unwrap();
        wait_until_ready(&handles);
        let rt = tokio::runtime::Runtime::new().unwrap();
        let db = root.path().join("db.sqlite");
        let store = rt
            .block_on(crate::profiles::ProfileStore::open(
                &db,
                &crate::setup::MIGRATOR,
            ))
            .unwrap();
        let alice = rt
            .block_on(store.create("Alice", "1234", "#e63946"))
            .unwrap();
        let row = rt
            .block_on(store.instances().fresh(&alice, "000009-0000"))
            .unwrap();
        // Under a minute of playtime: only the quit's flush writes it.
        let earned = skylander_rpcs3_control::FigureProgress {
            xp: 16,
            gold: 3,
            playtime_secs: 45,
            ..row.progress
        };
        let eruptor = skylander_core::FigureId::new("000009-0000");
        handles.progress.send(&alice, &eruptor, earned);
        assert!(handles.progress.flush_and_wait(Duration::from_secs(20)));
        let after = rt
            .block_on(store.instances().read(&alice, "000009-0000"))
            .unwrap()
            .unwrap();
        assert_eq!(after.progress, earned);

        let product_root = tempfile::tempdir().unwrap();
        let hooks = Hooks {
            figure_files: Some(Arc::new(WorkingCopies)),
            ..Hooks::default()
        };
        let product = boot_with(
            mock_config(product_root.path(), ephemeral_port()),
            product_root.path(),
            Box::new(|_| {
                Ok(BuiltDriver::new(Arc::new(
                    skylander_rpcs3_control::MockPortalDriver::new(),
                )))
            }),
            hooks,
        );
        wait_until_ready(&product);
        assert!(
            !product.progress.flush_and_wait(Duration::from_secs(1)),
            "the product drains figure progress"
        );
    }
}
