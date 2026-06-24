//! Shared state + driver job queue.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::Result;
// `Context` is only consumed inside the `cfg(windows)` BootDirect path
// below; gating the import this way keeps non-Windows clippy clean
// (no `unused_imports`) without breaking the Windows lane (which
// failed the v1.4.2 release).
#[cfg(windows)]
use anyhow::Context;
use skylander_core::{
    Event, Figure, FigureId, GameLaunched, GameSerial, SLOT_COUNT, SlotIndex, SlotState,
};
use skylander_rpcs3_control::{PortalDriver, RpcsProcess};
use tokio::sync::{Mutex, broadcast, mpsc};
use tracing::{error, info, warn};

use skylander_core::InstalledGame;

use crate::profiles::{ProfileStore, SessionRegistry};

pub struct AppState {
    pub figures: Vec<Figure>,
    /// Map figure_id → index into `figures` for quick lookup.
    pub figure_index: HashMap<FigureId, usize>,
    /// Which portal driver is active. Read by the `/api/launch` handler
    /// to skip the games.yml → EBOOT.BIN resolution under the mock
    /// driver (BootDirect's mock branch ignores `eboot_path` entirely,
    /// and on macOS there's no RPCS3 install so games.yml doesn't
    /// exist — the lookup would always 404). Driven by config /
    /// SKYLANDER_PORTAL_DRIVER env var (`config.rs::DriverKind`).
    pub driver_kind: crate::config::DriverKind,
    /// How the launcher window is presented (PLAN 20.6). Set from
    /// `cfg.window_mode` at startup; read by `GET /api/launcher/window-mode`
    /// so the phone's Konami-gated admin toggle reflects the running mode.
    pub window_mode: crate::config::WindowMode,
    pub driver_tx: mpsc::Sender<DriverJob>,
    pub portal: Arc<Mutex<[SlotState; SLOT_COUNT]>>,
    pub events: broadcast::Sender<Event>,
    pub connected_clients: Arc<std::sync::atomic::AtomicUsize>,
    /// Snapshot of launcher-visible state, polled by the eframe UI each
    /// frame (PLAN 4.15.4). Kept in a *sync* `Mutex` — the eframe event
    /// loop runs on the main OS thread and can't `await` a `tokio::Mutex`.
    /// Updated by `/api/launch` on successful boot and `/api/quit` on
    /// shutdown. Safe to hold briefly: the UI read is a single clone per
    /// ~250ms frame.
    pub launcher_status: Arc<std::sync::Mutex<LauncherStatus>>,

    /// Installed Skylanders games, loaded from RPCS3's games.yml at startup.
    pub games: Vec<InstalledGame>,
    /// Full serial → game-directory map from `<rpcs3>/config/games.yml`.
    /// Used by `/api/launch` to resolve a picked serial to its on-disk
    /// EBOOT.BIN path so RPCS3 can be spawned directly into that game
    /// (PLAN 10.8.4 direct-boot flow). `games` filters this down to
    /// known Skylanders titles for the phone picker; this map keeps the
    /// raw paths for every game RPCS3 knows about.
    pub games_yml: HashMap<String, PathBuf>,
    pub rpcs3_exe: PathBuf,
    /// RPCS3's data/config root (installed firmware + `config/games.yml` + the
    /// per-game `config/custom_configs/`). May live apart from `rpcs3_exe` under
    /// the Phase-16 bundled-binary model. Used to read `games.yml` at startup and
    /// passed to RPCS3 launches as `RPCS3_CONFIG_DIR` — including the on-demand
    /// settings GUI (PLAN 16.9.3), which persists per-game Custom Configurations
    /// here for the `--no-gui` boots to consume.
    pub config_dir: PathBuf,
    /// Root of the committed static-data bundle served at `/api/figures/:id/image`.
    /// Points at `<repo>/data/` in dev; populated at startup from config.
    pub data_root: PathBuf,
    /// Root of the built phone SPA (trunk's `dist/`). Used by handlers
    /// that need to read static assets directly — e.g. the icon-serving
    /// route in `http.rs`, which swaps in dev-tinted variants when the
    /// `dev-tools` feature is on. The general SPA fallback uses ServeDir
    /// against this same path.
    pub phone_dist: PathBuf,
    /// 32-byte HMAC-SHA256 key shared with the phone via the TV's QR code.
    /// Used by the `Signed` extractor on mutating REST endpoints (PLAN 3.13).
    pub hmac_key: Vec<u8>,
    /// Random u64 generated once at server startup. Sent to phones in the
    /// WS `Welcome` event so they can detect a server restart by comparing
    /// against the last-seen boot id and reset their in-memory UI state
    /// (the server has no record of any prior session/profile/screen
    /// after a restart). Chris flagged 2026-04-19, "force the phone app
    /// to reset its state if the server application has relaunched".
    pub boot_id: u64,
    /// Lifecycle lock around the currently-running RPCS3 instance.
    pub rpcs3: Arc<Mutex<RpcsLifecycle>>,

    /// SQLite-backed profile store + argon2 PIN hashes + lockout map.
    pub profiles: ProfileStore,
    /// Per-connection session registry. Tracks which profile (if any) is
    /// unlocked for each WS session. 3.9 is single-session; 3.10 extends
    /// this to a 2-slot FIFO registry.
    pub sessions: Arc<SessionRegistry>,

    /// Pre-rendered round-QR PNG of the phone's join URL (same URL the
    /// TV launcher encodes). Computed once at startup and served as-is
    /// from `GET /api/join-qr.png` for the phone's INVITE menu card.
    /// `Arc` so the handler can clone cheaply without duplicating the
    /// ~few-KB buffer per request.
    pub join_qr_png: Arc<Vec<u8>>,

    /// Concrete mock driver handle, populated only when running with the
    /// mock driver + test-hooks feature. The /api/_test/* endpoints use
    /// this to inject failure outcomes.
    #[cfg(feature = "test-hooks")]
    pub test_mock: Option<Arc<skylander_rpcs3_control::MockPortalDriver>>,
}

#[derive(Default)]
pub struct RpcsLifecycle {
    pub process: Option<RpcsProcess>,
    pub current: Option<GameLaunched>,
    /// EBOOT.BIN path of the game RPCS3 was launched with — populated
    /// by the BootDirect flow (PLAN 10.8.4) and consumed by the crash
    /// watchdog so an auto-respawn re-launches the same game rather
    /// than dropping into library view (which we no longer use).
    pub current_eboot: Option<PathBuf>,
    /// A **separate** RPCS3 instance launched in full-GUI mode for on-demand
    /// per-game configuration (PLAN 16.9.3 — the CONFIGURE GAME admin action).
    /// Tracked apart from `process` (the `--no-gui` game) on purpose: it's not a
    /// game, so the crash/freeze supervisor — which only watches `process` —
    /// never treats it as a crash, and `/api/launch` refuses to boot a game while
    /// it's alive (the two RPCS3 instances would fight over the singleton
    /// lockfile + IPC socket). A one-shot watcher clears it when the user closes
    /// the settings window.
    pub config_gui: Option<RpcsProcess>,
}

/// UI-polled snapshot of the launcher's status indicators (PLAN 4.15.4).
/// This is a *derived* view of `RpcsLifecycle` + broadcast events, written
/// from the handler threads and read by the eframe main thread. Kept as a
/// flat struct with primitives so a single `lock().clone()` per frame is
/// cheap and never contends on async work.
#[derive(Default, Debug, Clone)]
pub struct LauncherStatus {
    /// `true` while a spawned RPCS3 process is alive. Drives the header
    /// connection dot (dim → `SUCCESS_GLOW`).
    pub rpcs3_running: bool,
    /// Name of the currently-booted game, if any. Rendered in Titan One
    /// near the connection dot when present.
    pub current_game: Option<String>,
    /// Display name of a game that's currently being launched but isn't
    /// yet visible (RPCS3 is spawning + UIA-booting, takes ~10–30s).
    /// Set by `/api/launch` at the start of the boot path, cleared on
    /// success (alongside `rpcs3_running = true` + `current_game =
    /// Some`) or failure. Drives the launcher's loading screen — gives
    /// the user immediate visual feedback that their game pick was
    /// received instead of ~30s of unchanged Awaiting Connect (Chris
    /// flagged 2026-04-19, "the game loading state never shows").
    pub loading_game: Option<String>,
    /// Categorised loading stage, derived from RPCS3 log activity by
    /// the shader-compile watchdog. Values today: `"Building SPU
    /// cache"`, `"Building PPU cache"`, `"Compiling shaders"`. Drives
    /// the subtitle text on the LOADING badge so the user knows what
    /// phase the boot is in (first-launch shader compile can take
    /// minutes; the per-stage text reassures them progress is being
    /// made).
    pub shader_compile_text: Option<String>,
    /// `true` when `rpcs3_running` AND we've seen no compile/cache
    /// activity in the log for ~2s. The launcher waits for this
    /// signal — not just `rpcs3_running` — to trigger the close-to-
    /// in-game animation, because RPCS3 reports "running" the moment
    /// the UIA boot completes (well before shaders are compiled and
    /// the game is actually playable). Without the wait the user
    /// would see the launcher animate closed onto a black RPCS3
    /// window that's still mid-compile.
    pub game_playable: bool,
    /// Which full-screen launcher surface the egui UI should render on the
    /// next frame. Default is `Main` — the QR + status strip layout.
    /// Flipped by the crash watchdog (PLAN 4.15.10) and `/api/shutdown`
    /// (PLAN 4.15.11) into `Crashed` / `Farewell` respectively.
    pub screen: LauncherScreen,
    /// Number of currently-registered phone sessions (0..=MAX_SESSIONS).
    /// Drives the count of visible player-orbit pips (PLAN 4.15.7).
    pub session_count: u8,
    /// `true` when the session registry is at the `MAX_SESSIONS` cap.
    /// Triggers the QR card-flip animation (PLAN 4.15.6).
    pub session_slots_full: bool,
    /// One entry per currently-registered session. Ordered by session id
    /// ascending (oldest first) so pips keep a stable slot when a new
    /// session joins. Length matches `session_count`.
    pub session_profiles: Vec<SessionPip>,
    /// `true` once the server has bound its listener and is serving HTTP.
    /// Set by the tokio thread right before `axum::serve()`. Drives the
    /// launcher's intro-animation gate: with this `false` the launcher
    /// holds in the calm-starfield Startup beat indefinitely instead of
    /// auto-advancing to the iris reveal + badge spin. If the server
    /// fails to start (port-in-use, db-open error) the screen flips to
    /// `ServerError` before this is ever set, so the user sees the
    /// error directly rather than watching the intro spin only to be
    /// interrupted (Chris flagged 2026-04-19).
    pub server_ready: bool,
    /// `true` between the phone's HOLD TO SWITCH GAMES action firing and
    /// the next `/api/launch` arriving. PLAN 4.15.9 — without this flag
    /// the launcher runs its ReturnFromGame animation (iris reveals, QR
    /// card spins back in) the moment the outgoing game is stopped,
    /// which reads as "back to the join screen" not "changing games".
    /// With this set, the launcher pins iris at fully-closed DarkHole
    /// and shows a "SWITCHING GAMES" heading until the new boot fires.
    /// Cleared by `/api/launch` on entry.
    pub switching: bool,
    /// `true` during a graceful-quit transition (PLAN 10.8.7b): set
    /// at the start of `/api/quit`, held while the launcher renders
    /// its opaque cover (sky + starfield + vortex + RETURNING badge),
    /// cleared after RPCS3 has been killed and post-quit cleanup is
    /// done. The in-game render predicate gates on
    /// `!cover_active` so the launcher flips out of the transparent
    /// CentralPanel BEFORE the kill runs — RPCS3 dies behind an
    /// opaque cover, no flash of desktop. Distinct from `switching`
    /// (which signals "next /api/launch is coming") so the
    /// back-face precedence can render `Returning` vs `Switching`
    /// correctly.
    pub cover_active: bool,
    /// `true` when the production driver is the Phase-16 IPC driver (no-GUI +
    /// borderless). Set once at startup. Tells the launcher to use the simplified
    /// z-order (overlay directly above the game, never desktop-topmost — so the
    /// user can alt-tab away) instead of the legacy UIA topmost-fighting, since
    /// there are no Skylanders Manager / menu-bar windows to out-fight (PLAN 16.6.2.2).
    pub driver_is_ipc: bool,
    /// Native handle of the borderless game window, reported by RPCS3 over IPC once
    /// the game is playable (PLAN 16.6.2). `Some` ⇒ the launcher slots itself
    /// directly above this window in the z-order. Only meaningful while
    /// `rpcs3_running`; a stale value from a prior game is ignored.
    pub game_window_handle: Option<u64>,
    /// macOS `CAContextID` of the game's published render layer, reported by
    /// RPCS3 over IPC (P8 surface-embed). `Some(non-zero)` ⇒ the launcher hosts
    /// this layer tree INSIDE its own egui window via `CALayerHost`
    /// (`crate::compositor::CompositorHost`) — compositing the game behind
    /// egui's chrome — instead of tiling a second top-level window beneath
    /// itself (the P7 `WINDOW_SET` fallback). The id is **stable for the whole
    /// game session** (survives swapchain recreate / resize / resolution
    /// change), so the launcher attaches once and never re-fetches. Only
    /// meaningful while `rpcs3_running`; a stale value from a prior game is
    /// ignored. `None` on non-IPC / non-macOS drivers.
    pub game_surface_context_id: Option<u32>,
    /// `true` while the on-demand RPCS3 **settings GUI** is open (PLAN 16.9.3).
    /// The full Qt settings window needs the whole TV + the HTPC keyboard/mouse,
    /// so the always-on-top launcher **minimises itself** for the duration and
    /// restores when the user closes RPCS3. Set by `/api/rpcs3/settings`, cleared
    /// by the config watcher on GUI exit.
    pub config_gui_open: bool,
    /// `true` when the game has been **playable** but its frame counter
    /// (`EmuState.frames`, the RSX flip index) has stalled while RPCS3 still
    /// reports `running` for `FREEZE_AFTER` — i.e. the game hung (PLAN 16.7.1).
    /// Detected by the IPC STATE poller off the 1 Hz heartbeat's frame field.
    /// The recovery action (auto-restart) is 16.7.2; this flag is the signal.
    pub frozen: bool,
    /// `true` once the connectivity watchdog (PLAN 17.1) has decided phones are
    /// probably unable to reach us: the server's been up + showing the QR past a
    /// grace window with **zero** clients ever connected this session. Drives the
    /// launcher's "Trouble connecting?" card (raw-IP URL + firewall fix). Cleared
    /// the instant any client connects.
    pub connectivity_warning: bool,
    /// Firewall-rule health snapshot (PLAN 17.2), filled in when
    /// `connectivity_warning` is raised so the card can specialise its copy
    /// ("no firewall rule for port …" vs "firewall is off — check Wi-Fi/mDNS").
    pub firewall_status: crate::firewall::FirewallStatus,
}

/// UI-polled view of one connected phone session. Colour / initial are
/// `None` when the session is registered but not yet unlocked — the pip
/// then renders as a neutral gold placeholder with a dot instead of a
/// letter.
#[derive(Debug, Clone, Default)]
pub struct SessionPip {
    /// Profile hex colour (e.g. `#ff00aa`). `None` means "session has no
    /// profile unlocked yet".
    pub color: Option<String>,
    /// First grapheme of the profile's display name, uppercased. `None`
    /// means unlocked state unknown.
    pub initial: Option<String>,
    /// `true` for ghosted sessions (PLAN 8.1.6). The pip renders in a
    /// dimmed "(away)" treatment — the player's figures still occupy
    /// their portal slots, but their phone isn't responsive right now.
    /// Cleared to `false` when the same profile reconnects via
    /// `?reclaim=` and the session goes live again.
    pub is_ghost: bool,
}

/// Which top-level surface the egui TV launcher is rendering right now.
/// Polled by the eframe `update` loop each frame; writers flip this from
/// HTTP handlers (`/api/shutdown`) and background tasks (the crash
/// watchdog). See `docs/aesthetic/navigation.md` §3 for the 8-state mock
/// — this enum collapses the design-doc states down to the three the egui
/// port cares about today. Other states (Booting, Awaiting Connect, etc.)
/// are implicit in `rpcs3_running` / `current_game` / `connected_clients`
/// and don't need their own variants yet.
#[derive(Default, Debug, Clone)]
pub enum LauncherScreen {
    /// Default surface: title, QR bezel, status strip, connected-clients
    /// counter, Exit-to-Desktop button.
    #[default]
    Main,
    /// RPCS3 died unexpectedly. `message` is the human-readable string the
    /// watchdog broadcasts alongside `Event::GameCrashed` so the egui
    /// screen and the phone overlay carry the same copy.
    Crashed { message: String },
    /// User asked to quit the launcher via the phone menu's SHUT DOWN
    /// action (or a dev `/api/shutdown` curl). The egui screen displays a
    /// short farewell then calls `ViewportCommand::Close` after ~3s.
    Farewell,
    /// Backend startup failed — the tokio thread couldn't construct the
    /// driver, open the profile DB, bind the listener, etc. Phones can't
    /// connect because nothing's serving HTTP, so the QR screen would be
    /// dishonest. Set by the tokio thread on each failure path; the egui
    /// surface shows the human-readable `message` and an Exit button.
    /// (Recovery is manual — the typical fix is "free port 8080" or
    /// "restore the corrupt db file", neither of which the launcher can
    /// do for the user.)
    ServerError { message: String },
}

impl AppState {
    pub fn lookup_game(&self, serial: &GameSerial) -> Option<&InstalledGame> {
        self.games.iter().find(|g| &g.serial == serial)
    }
}

impl AppState {
    pub fn lookup_figure(&self, id: &FigureId) -> Option<&Figure> {
        self.figure_index.get(id).and_then(|i| self.figures.get(*i))
    }

    /// Sweep ghosted sessions whose `ghosted_at` exceeded the configured
    /// `GHOST_TIMEOUT` (PLAN 8.1.4). Each removed ghost has its placed
    /// figures cleared from the portal and a snapshot published so the
    /// TV's player-orbit pip count updates. Schedule on a 60s tick from
    /// `main.rs`; idempotent on quiet ticks.
    pub async fn sweep_expired_ghosts(&self) {
        let evicted = self
            .sessions
            .expire_ghosts_older_than(crate::profiles::GHOST_TIMEOUT, std::time::Instant::now())
            .await;
        if evicted.is_empty() {
            return;
        }
        for (sid, pid) in evicted {
            info!(
                session_id = sid.0,
                profile_id = pid.as_deref().unwrap_or("<none>"),
                "ghost session expired — running deferred slot cleanup"
            );
            if let Some(pid) = pid {
                self.clear_slots_for_profile(&pid).await;
            }
        }
        self.publish_session_snapshot().await;
    }

    /// Kaos timer tick (PLAN 8.2b.2). Walks every registered session
    /// and, for those whose profile has `kaos_enabled = true`:
    ///
    /// - **No schedule yet** → seed `kaos_next_fire_at = now + WARMUP`.
    ///   Gives the user a 20-min grace window after unlock before any
    ///   disruption; also used to re-arm after a toggle flip.
    /// - **Due** (`kaos_next_fire_at <= now`) → attempt a swap via
    ///   `kaos::select_swap`. On success, execute the swap + reschedule
    ///   to `now + random_gap()` (1min..=1hr). On failure (no eligible
    ///   slots or no compatible replacements), reschedule to `now +
    ///   MIN_GAP` so we try again shortly.
    ///
    /// Sessions whose profile has `kaos_enabled = false` get their
    /// schedule cleared — flipping back on starts a fresh warmup.
    /// Ghosted sessions are skipped entirely (their replay buffer
    /// would accumulate the taunt on next fire, but firing into
    /// nobody feels wasteful; the next tick picks them up if they
    /// reconnect).
    ///
    /// Intended to be called on a ~10s tokio interval from main.rs;
    /// no-ops when no session is eligible.
    pub async fn tick_kaos(&self, now: std::time::Instant) {
        use rand_core::RngCore;
        let ids = self.sessions.all_ids().await;
        let mut rng = rand_core::OsRng;
        for sid in ids {
            let Some(sess) = self.sessions.get(sid).await else {
                continue;
            };
            if sess.is_ghost() {
                continue;
            }
            let Some(pid) = sess.profile_id else {
                continue;
            };
            let Ok(Some(row)) = self.profiles.get(&pid).await else {
                continue;
            };
            if !row.kaos_enabled {
                if sess.kaos_next_fire_at.is_some() {
                    self.sessions.set_kaos_schedule(sid, None).await;
                }
                continue;
            }
            let Some(due_at) = sess.kaos_next_fire_at else {
                // First tick after unlock — seed the warmup. Use a
                // small random jitter so multiple sessions unlocking
                // in the same tick don't end up firing in lockstep.
                let jitter = std::time::Duration::from_secs(rng.next_u64() % 30);
                self.sessions
                    .set_kaos_schedule(sid, Some(now + crate::kaos::WARMUP + jitter))
                    .await;
                continue;
            };
            if due_at > now {
                continue;
            }
            // Pick + execute the swap. Fall through to a short
            // reschedule on "nothing to swap" — the common case is
            // "portal is empty right now, try again in a minute."
            let portal = self.portal.lock().await.clone();
            let current_game = self
                .current_game_of_origin()
                .await
                .unwrap_or(skylander_core::GameOfOrigin::Imaginators);
            let swap =
                crate::kaos::select_swap(current_game, &portal, &self.figures, &pid, &mut rng);
            let next = match swap {
                Some(s) => {
                    let taunt = crate::kaos::random_swap_taunt(&mut rng);
                    self.execute_kaos_swap(&s, &pid, taunt).await;
                    now + crate::kaos::random_gap(&mut rng)
                }
                None => {
                    // No-op — try again in MIN_GAP so we don't hot-
                    // spin. Typical causes: portal is empty, profile
                    // has no figures placed, current game has no
                    // compatible library match.
                    now + crate::kaos::MIN_GAP
                }
            };
            self.sessions.set_kaos_schedule(sid, Some(next)).await;
        }
    }

    /// Look up the `GameOfOrigin` of the currently-running game, if any.
    /// `None` means the launcher hasn't booted a game (or the booted
    /// serial isn't in our catalogue). Used by the Kaos ticker to
    /// gate compatibility.
    pub async fn current_game_of_origin(&self) -> Option<skylander_core::GameOfOrigin> {
        let lifecycle = self.rpcs3.lock().await;
        lifecycle
            .current
            .as_ref()
            .and_then(|g| skylander_core::compat::game_of_origin_from_serial(&g.serial))
    }

    /// Execute a Kaos mid-game swap (PLAN 8.2b.4). Queues a ClearSlot
    /// followed by a LoadFigure for the same slot, flips portal state
    /// into Loading, broadcasts `SlotChanged` + `KaosTaunt`, and pushes
    /// the taunt into any matching ghost's replay buffer so a
    /// backgrounded phone still sees it on reconnect.
    ///
    /// `placed_by` on the new load is the same profile_id — the swap
    /// preserves ownership (it's still Alice's slot, just a different
    /// figure). Figures not in the library (lookup miss) → fail silently;
    /// this is an unreachable state in practice because the caller
    /// picked `new_figure_id` from the library.
    pub async fn execute_kaos_swap(
        &self,
        swap: &crate::kaos::KaosSwap,
        profile_id: &str,
        taunt: &str,
    ) {
        let Some(new_figure) = self.lookup_figure(&swap.new_figure_id) else {
            warn!(
                new_figure_id = %swap.new_figure_id.as_str(),
                "kaos swap: replacement figure vanished from library — aborting",
            );
            return;
        };
        let new_path = match crate::working_copies::resolve_load_path(profile_id, new_figure) {
            Ok(p) => p,
            Err(e) => {
                warn!(
                    profile_id,
                    new_figure_id = %swap.new_figure_id.as_str(),
                    "kaos swap: resolve working copy failed: {e}"
                );
                return;
            }
        };
        // Flip portal state immediately so phones animate the slot
        // transitioning rather than waiting on the async clear+load
        // round-trip.
        {
            let mut portal = self.portal.lock().await;
            portal[swap.slot.as_u8() as usize] = SlotState::Loading {
                figure_id: Some(new_figure.id.clone()),
                placed_by: Some(profile_id.to_string()),
            };
        }
        let _ = self.events.send(Event::SlotChanged {
            slot: swap.slot,
            state: SlotState::Loading {
                figure_id: Some(new_figure.id.clone()),
                placed_by: Some(profile_id.to_string()),
            },
        });

        if let Err(e) = self
            .driver_tx
            .send(DriverJob::ClearSlot { slot: swap.slot })
            .await
        {
            warn!("kaos swap: queue ClearSlot failed: {e}");
            return;
        }
        if let Err(e) = self
            .driver_tx
            .send(DriverJob::LoadFigure {
                slot: swap.slot,
                figure_id: new_figure.id.clone(),
                path: new_path,
                placed_by: Some(profile_id.to_string()),
                canonical_name: new_figure.canonical_name.clone(),
            })
            .await
        {
            warn!("kaos swap: queue LoadFigure failed: {e}");
            return;
        }

        info!(
            profile_id,
            slot = swap.slot.as_u8(),
            old = %swap.old_figure_id.as_str(),
            new = %swap.new_figure_id.as_str(),
            "kaos swap executed — clear + load + taunt",
        );

        let evt = crate::kaos::build_taunt_event(swap, taunt, profile_id);
        // Push into the ghost's replay buffer BEFORE broadcast so a
        // reconnect that happens in the same tick still flushes the
        // taunt out (push_replay_for_profile filters by is_ghost so
        // live sessions are a no-op here; they'll get it via broadcast).
        let _ = self
            .sessions
            .push_replay_for_profile(profile_id, &evt)
            .await;
        let _ = self.events.send(evt);
    }

    /// Drop every slot on the portal whose `placed_by` matches `profile_id`.
    /// Called when a phone disconnects so the departing player's figures
    /// come off the portal instead of lingering ownerless (PLAN 3.10.9 —
    /// simple MVP 2-player disconnect policy).
    ///
    /// Only `Loaded` slots are touched. `Loading` slots aren't rewritten —
    /// the in-flight driver job will complete normally; any subsequent
    /// Loaded doesn't retro-clear because the snapshot is taken up-front.
    /// `Empty` and `Error` slots have no owner by definition.
    ///
    /// Each matched slot is flipped to `Loading { placed_by: None }` in
    /// portal state, broadcast as `SlotChanged` so connected phones
    /// animate the desat+shrink transition (4.6.5), and enqueued as a
    /// `DriverJob::ClearSlot` for RPCS3 to drop the `.sky` file.
    pub async fn clear_slots_for_profile(&self, profile_id: &str) {
        let to_clear = {
            let mut p = self.portal.lock().await;
            flip_loaded_owned_to_loading(&mut p, profile_id)
        };
        if to_clear.is_empty() {
            return;
        }
        info!(
            profile_id,
            count = to_clear.len(),
            "disconnect cleanup — clearing departing profile's slots",
        );
        for (slot, loading) in to_clear {
            let _ = self.events.send(Event::SlotChanged {
                slot,
                state: loading,
            });
            if let Err(e) = self.driver_tx.send(DriverJob::ClearSlot { slot }).await {
                warn!(
                    slot = slot.as_u8(),
                    "disconnect cleanup: queue ClearSlot failed: {e}",
                );
            }
        }
    }

    /// Recompute the session-related fields on `launcher_status`
    /// (`session_count`, `session_slots_full`, `session_profiles`) from the
    /// current registry state + profile store and publish the snapshot for
    /// the eframe UI thread. Call after every mutation of the session
    /// registry: `register`, `remove`, `set_profile`, and the `test-hooks`
    /// `set_pending_unlock` / `set_session_profile` paths (PLAN 4.15.6 /
    /// 4.15.7).
    ///
    /// Best-effort: profile-store errors fall back to a neutral pip so the
    /// UI can still render a count. A poisoned `launcher_status` mutex
    /// (eframe thread panicked) silently no-ops — we keep serving phones.
    pub async fn publish_session_snapshot(&self) {
        let mut ids = self.sessions.all_ids().await;
        // Stable order by session id so pips don't swap slots when a
        // session joins or leaves. Session ids are minted monotonically, so
        // ascending = oldest first, which matches how the mock assigns
        // pip1/pip2.
        ids.sort_by_key(|s| s.0);

        let mut pips = Vec::with_capacity(ids.len());
        for sid in &ids {
            let session_state = self.sessions.get(*sid).await;
            let is_ghost = session_state
                .as_ref()
                .map(|s| s.is_ghost())
                .unwrap_or(false);
            let profile_id = session_state.and_then(|s| s.profile_id);
            let mut pip = match profile_id {
                Some(pid) => match self.profiles.get(&pid).await {
                    Ok(Some(row)) => SessionPip {
                        color: Some(row.color),
                        initial: first_grapheme_uppercase(&row.display_name),
                        is_ghost: false,
                    },
                    _ => SessionPip::default(),
                },
                None => SessionPip::default(),
            };
            pip.is_ghost = is_ghost;
            pips.push(pip);
        }

        let count = pips.len() as u8;
        let full = (pips.len()) >= crate::profiles::MAX_SESSIONS;

        if let Ok(mut st) = self.launcher_status.lock() {
            st.session_count = count;
            st.session_slots_full = full;
            st.session_profiles = pips;
        }
    }
}

/// Extract the first grapheme of a display name and uppercase it for use
/// as a pip initial. Returns `None` for empty strings.
fn first_grapheme_uppercase(name: &str) -> Option<String> {
    let trimmed = name.trim();
    if trimmed.is_empty() {
        return None;
    }
    // Unicode-naive first-char uppercase — display names are validated to
    // be 1–32 chars ASCII-ish in `validate_name`, so `chars().next()`
    // lines up with the user's intent without needing a grapheme crate.
    trimmed
        .chars()
        .next()
        .map(|c| c.to_uppercase().collect::<String>())
}

/// Reverse-lookup a figure by its display name (case and surrounding-whitespace
/// insensitive match against `canonical_name`). Used on `RefreshPortal` — and
/// driver-failure re-reads — where the driver returns a raw RPCS3 name with no
/// `figure_id` context. Returns `None` for the empty string.
pub fn find_figure_by_display_name<'a>(figures: &'a [Figure], name: &str) -> Option<&'a Figure> {
    let target = name.trim().to_lowercase();
    if target.is_empty() {
        return None;
    }
    figures
        .iter()
        .find(|f| f.canonical_name.trim().to_lowercase() == target)
}

/// Apply reverse name-matching to any `SlotState::Loaded` that arrived without
/// a `figure_id` (i.e. came from `driver.read_slots()`, not from an outgoing
/// `LoadFigure` job). Matched slots get their `figure_id` populated and their
/// `display_name` canonicalised; unmatched slots are left alone so the phone
/// can render the raw name with a "?" badge (PLAN 3.8.2).
fn reconcile_slot_names(
    mut snap: [SlotState; SLOT_COUNT],
    figures: &[Figure],
) -> [SlotState; SLOT_COUNT] {
    for slot in snap.iter_mut() {
        if let SlotState::Loaded {
            figure_id: figure_id @ None,
            display_name,
            ..
        } = slot
            && let Some(fig) = find_figure_by_display_name(figures, display_name)
        {
            *figure_id = Some(fig.id.clone());
            *display_name = fig.canonical_name.clone();
        }
    }
    snap
}

#[derive(Debug)]
pub enum DriverJob {
    LoadFigure {
        slot: SlotIndex,
        figure_id: FigureId,
        path: PathBuf,
        /// Profile id of the session that initiated this load. Threaded
        /// through into `SlotState::Loaded.placed_by` so both phones can
        /// render an ownership indicator. `None` if the caller wasn't
        /// authenticated (pre-3.10d REST calls without X-Session-Id).
        placed_by: Option<String>,
        /// Canonical display name from the pack index. Authoritative — the
        /// driver's own read (file-stem for the mock, UIA ValueValue for
        /// UIA) is observational and less reliable, especially with
        /// per-profile working copies whose filenames are figure-id hashes.
        canonical_name: String,
    },
    ClearSlot {
        slot: SlotIndex,
    },
    RefreshPortal,
    /// **PLAN 10.8.4 direct-boot path.** Spawn RPCS3 with the given
    /// game's EBOOT.BIN as the first CLI arg, wait for the FPS: viewport
    /// (game running), then `driver.open_dialog()` so the Skylanders
    /// Manager dialog is ready before the user touches their first
    /// figure. `expected_name` lets the worker verify the booted game's
    /// viewport title matches what the user picked (catches a stale
    /// games.yml mapping or a path-collision mis-launch).
    BootDirect {
        eboot_path: PathBuf,
        expected_name: String,
        /// Display name of the picked serial — fed to `current_game`
        /// in `RpcsLifecycle` after a successful boot.
        display_name: String,
        serial: String,
        /// Total budget for spawn → ready → viewport → open_dialog.
        /// First-launch shader compile can take 60–120s, so this
        /// runs longer than the old `BootGame` timeout.
        timeout: std::time::Duration,
        done: tokio::sync::oneshot::Sender<Result<()>>,
    },
    /// **PLAN B.2 (macOS window fit).** Move + resize the running game window to
    /// the given screen-space rect via the IPC P7 `WINDOW_SET` command. On
    /// Windows the launcher repositions the game HWND directly (`SetWindowPos`);
    /// on macOS it can't touch another app's window, so the Desktop-mode fit is
    /// routed through the driver here. Not a portal mutation — no layout persist.
    WindowSet {
        x: i32,
        y: i32,
        w: u32,
        h: u32,
    },
}

/// Frame-stall freeze detector (PLAN 16.7.1). The patched RPCS3's 1 Hz heartbeat
/// (and `STATE`) expose the RSX flip index (`EmuState.frames`), which advances
/// ~60/s while a game renders — the heartbeat was built to carry it precisely
/// because "certain games love to freeze". Once the game is **playable**, a
/// `running` status whose `frames` stops advancing for `threshold` consecutive
/// observations means the game hung. Pure + tick-driven, so it unit-tests without
/// a live emulator. (Recovery / auto-restart is 16.7.2; this only raises the flag.)
struct FreezeDetector {
    stall_ticks: usize,
    threshold: usize,
}

impl FreezeDetector {
    fn new(threshold: usize) -> Self {
        Self {
            stall_ticks: 0,
            threshold: threshold.max(1),
        }
    }

    /// Feed one observation. `advancing` = frames increased since the previous
    /// tick; `running` = RPCS3 reports `status == "running"`; `playable` = the
    /// game reached the rendering state (no freeze is declared before then —
    /// pre-play frame stalls are normal during compile/boot). Returns whether the
    /// game is currently considered frozen (true once stalled `threshold` ticks,
    /// and stays true until frames resume).
    fn observe(&mut self, advancing: bool, running: bool, playable: bool) -> bool {
        if playable && running && !advancing {
            self.stall_ticks = self.stall_ticks.saturating_add(1);
        } else {
            self.stall_ticks = 0;
        }
        self.stall_ticks >= self.threshold
    }

    fn reset(&mut self) {
        self.stall_ticks = 0;
    }
}

/// IPC `STATE`-driven playability signal (PLAN 16.6.3.1) — the clean replacement
/// for the FPS-title sampler under the IPC driver. Drives `game_playable` off the
/// patched emulator's own state: `running`, frames advancing, AND boot/shader
/// compile complete (`progr_done >= progr_total`). Crucially it waits for the
/// **compile** to finish, not just for the loading-screen FPS to rise — so the
/// launcher holds its opaque cover over the PPU/SPU/shader compile and only reveals
/// the game once it's truly rendering (no early iris, no compile→in-game flicker).
/// Requires the signal sustained for `SAMPLE_BUFFER` ticks to ride out the brief
/// gaps between compile phases, and latches `true` while running so a transient IPC
/// hiccup doesn't strobe the reveal. Spawned only under the IPC driver.
pub fn spawn_state_poller(
    driver: Arc<dyn PortalDriver>,
    launcher_status: Arc<std::sync::Mutex<LauncherStatus>>,
    interval: std::time::Duration,
) {
    /// Consecutive ready ticks before declaring playable (8 × 250 ms = 2 s). Wider
    /// than the FPS sampler's 1 s to ride out the brief gaps between RPCS3's compile
    /// phases (progr-done before seg-start) so the reveal lands after the menu paints.
    const SAMPLE_BUFFER: usize = 8;
    /// A playable game whose frames stall this long (while RPCS3 still reports
    /// `running`) is treated as frozen. Generous enough to ride out in-game level
    /// loads / cutscene transitions (which also pause frames but recover), short
    /// enough to catch a real hang promptly. PLAN 16.7.1.
    const FREEZE_AFTER: std::time::Duration = std::time::Duration::from_secs(8);

    // Stall ticks before declaring a freeze, derived from the poll interval.
    let freeze_ticks = (FREEZE_AFTER.as_millis() / interval.as_millis().max(1)) as usize;

    tokio::spawn(async move {
        let mut ticker = tokio::time::interval(interval);
        ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        ticker.tick().await;

        let mut ready_run = 0usize;
        let mut last_frames = 0u64;
        let mut freeze = FreezeDetector::new(freeze_ticks);
        // Sticky: the game has rendered at least once this session. Gates freeze
        // detection so it survives un-latching `game_playable` for the cover.
        let mut ever_playable = false;

        loop {
            ticker.tick().await;

            // emu_state() + the window handle + the macOS surface contextId are
            // blocking IPC round-trips — off-reactor. (P8: `game_surface_context_id`
            // rides the same poll as `game_window_handle`; non-zero ⇒ the launcher
            // hosts the game's render layer in-window via CALayerHost.)
            let d = driver.clone();
            let (state, game_window_handle, game_surface_context_id) =
                match tokio::task::spawn_blocking(move || {
                    (
                        d.emu_state(),
                        d.game_window_handle(),
                        d.game_surface_context_id(),
                    )
                })
                .await
                {
                    Ok((s, h, c)) => (s.ok().flatten(), h.ok().flatten(), c.ok().flatten()),
                    Err(_) => (None, None, None),
                };

            // Ready = compile complete + actually rendering (frames advancing).
            // `advancing` + `running` double as the freeze-detector inputs (16.7.1).
            let advancing;
            let running;
            let ready;
            match &state {
                Some(s) => {
                    advancing = s.frames > last_frames;
                    running = s.status == "running";
                    last_frames = s.frames;
                    ready = s.is_playable() && advancing;
                }
                None => {
                    advancing = false;
                    running = false;
                    last_frames = 0;
                    ready = false;
                }
            }
            ready_run = if ready { ready_run + 1 } else { 0 };
            let stable = ready_run >= SAMPLE_BUFFER;

            if let Ok(mut st) = launcher_status.lock() {
                // Publish the game-window handle as soon as the window exists (during
                // boot/compile) so the launcher slots it BELOW itself right away —
                // otherwise RPCS3's freshly-created window sits on TOP of the launcher
                // and the user sees the compile through it for the whole boot, no
                // matter what the launcher paints (PLAN 16.6.2, HTPC 2026-05-30).
                st.game_window_handle = game_window_handle;
                // P8 surface-embed: publish the macOS render-layer contextId the
                // same way as the window handle — the launcher reads it each frame
                // and attaches a CALayerHost the moment it goes Some (the id is
                // stable for the session, so it attaches once).
                st.game_surface_context_id = game_surface_context_id;
                if !st.rpcs3_running {
                    ready_run = 0;
                    last_frames = 0;
                    freeze.reset();
                    ever_playable = false;
                    st.frozen = false;
                }
                if stable {
                    ever_playable = true;
                }
                // Freeze detection (PLAN 16.7.1) FIRST — once the game has been
                // playable, a `running` status whose frame counter stalls for
                // FREEZE_AFTER means it hung. Gated on the sticky `ever_playable`
                // (not the live latch, which we drop below) so it doesn't oscillate.
                // RPCS3's OWN fatal freeze ("Emulation has been frozen!" — the
                // Simulated-RPCN fatal, an unimplemented-syscall stop, etc.) sets
                // the emulator to `frozen` while the *process stays alive*: neither
                // the crash path (process dead) nor the frame-stall detector (which
                // requires status=="running") catches it, so recovery would never
                // fire (observed 2026-05-31 — "the watchdog isn't restarting").
                // Once the game has been playable, treat a `frozen` status as an
                // IMMEDIATE, definitive freeze — no 8 s stall wait. `paused` /
                // `stopped` are deliberately left alone (user pause / clean quit).
                // PLAN 16.10.2.
                let status_frozen = matches!(&state, Some(s) if s.status == "frozen");
                let frozen_now = freeze.observe(advancing, running, ever_playable)
                    || (ever_playable && status_frozen);
                if st.frozen != frozen_now {
                    if frozen_now {
                        warn!(
                            stalled_at_frame = last_frames,
                            status_frozen,
                            "rpcs3 game FROZEN — recovering (frame stall or RPCS3 status=frozen; PLAN 16.7.1/16.10.2)"
                        );
                    } else {
                        info!("rpcs3 game recovered from freeze — frames advancing again");
                    }
                    st.frozen = frozen_now;
                }
                // Latch: playable while running + stable, but DROP it on freeze so the
                // launcher covers the hung game with its opaque loading surface
                // (16.7.2 "cover" step). Recovery re-latches once frames resume.
                let new_playable = st.rpcs3_running && (st.game_playable || stable) && !frozen_now;
                if st.game_playable != new_playable {
                    if new_playable {
                        info!(
                            "rpcs3 game playable (IPC STATE: compile complete + frames advancing)"
                        );
                    } else if st.game_playable {
                        info!("rpcs3 game no longer playable (IPC STATE)");
                    }
                    st.game_playable = new_playable;
                }
                // Compile subtitle from the boot/shader progress, while it runs.
                let subtitle = match &state {
                    Some(s)
                        if st.rpcs3_running
                            && s.progr_total > 0
                            && s.progr_done < s.progr_total =>
                    {
                        Some(format!("Compiling {}/{}", s.progr_done, s.progr_total))
                    }
                    _ => None,
                };
                if st.shader_compile_text != subtitle {
                    st.shader_compile_text = subtitle;
                }
            }
        }
    });
}

/// Capture the `(slot, figure, owner)` of every restorable slot from a portal
/// snapshot so the supervisor can re-place them after an auto-restart (PLAN
/// 16.7.2). Only `Loaded` slots carrying a `figure_id` qualify — those are the
/// ones with a known library figure + a per-profile working copy to re-load.
/// Skipped: `Empty` / `Error` (nothing there), `Loading` (mid-flight — the
/// emulator's gone, the job will fail), and `Loaded` without a `figure_id`
/// (name-only reads we can't map back to a working copy). Pure over an in-memory
/// snapshot, so the restore-selection logic unit-tests without an emulator.
fn placed_figures_to_restore(
    slots: &[SlotState; SLOT_COUNT],
) -> Vec<(SlotIndex, FigureId, Option<String>)> {
    let mut out = Vec::new();
    for (i, s) in slots.iter().enumerate() {
        if let SlotState::Loaded {
            figure_id: Some(fid),
            placed_by,
            ..
        } = s
            && let Ok(slot) = SlotIndex::new(i as u8)
        {
            out.push((slot, fid.clone(), placed_by.clone()));
        }
    }
    out
}

/// Re-queue the figures that were on the portal before an auto-restart, in the
/// same slots, preserving ownership (PLAN 16.7.2). For each captured entry we
/// resolve the placing profile's working copy (so save progress carries over),
/// flip the slot to `Loading` + broadcast `SlotChanged` (phones animate the
/// re-place), and enqueue a `LoadFigure` job. Entries we can't restore are
/// logged and skipped, never fatal:
///   * **no owner** — `placed_by: None` (legacy / unauthenticated load) has no
///     per-profile working copy to resolve;
///   * **unknown figure** — the id no longer maps to a library figure.
///
/// The driver worker serialises the loads after the restart's `BootDirect`, so
/// they land on a booted emulator.
async fn restore_portal_figures(
    portal: &Arc<Mutex<[SlotState; SLOT_COUNT]>>,
    events: &broadcast::Sender<Event>,
    driver_tx: &mpsc::Sender<DriverJob>,
    figures: &[Figure],
    to_restore: &[(SlotIndex, FigureId, Option<String>)],
) {
    let mut requeued = 0usize;
    for (slot, figure_id, placed_by) in to_restore {
        let Some(profile_id) = placed_by else {
            info!(
                slot = slot.as_u8(),
                figure = figure_id.as_str(),
                "restore: slot had no owner — can't resolve a working copy; skipping"
            );
            continue;
        };
        let Some(figure) = figures.iter().find(|f| &f.id == figure_id) else {
            warn!(
                figure = figure_id.as_str(),
                "restore: figure no longer in library; skipping"
            );
            continue;
        };
        let path = match crate::working_copies::resolve_load_path(profile_id, figure) {
            Ok(p) => p,
            Err(e) => {
                warn!(
                    profile_id,
                    figure = figure_id.as_str(),
                    "restore: resolve working copy failed: {e}"
                );
                continue;
            }
        };
        let loading = SlotState::Loading {
            figure_id: Some(figure_id.clone()),
            placed_by: Some(profile_id.clone()),
        };
        portal.lock().await[slot.as_usize()] = loading.clone();
        let _ = events.send(Event::SlotChanged {
            slot: *slot,
            state: loading,
        });
        let job = DriverJob::LoadFigure {
            slot: *slot,
            figure_id: figure_id.clone(),
            path,
            placed_by: Some(profile_id.clone()),
            canonical_name: figure.canonical_name.clone(),
        };
        if driver_tx.send(job).await.is_err() {
            warn!("restore: driver worker gone; aborting figure restore");
            return;
        }
        requeued += 1;
    }
    info!(
        requeued,
        captured = to_restore.len(),
        "supervisor: re-queued portal figures after restart"
    );
}

/// Spawn the unified RPCS3 **crash/freeze supervisor** (PLAN 16.7). Polls the
/// lifecycle lock once per `interval` and reacts to either failure mode:
///   * **crash** — the spawned process has died while `current` is still set
///     (nobody called `/api/quit`);
///   * **freeze** — `LauncherStatus.frozen`, raised by the IPC STATE poller's
///     `FreezeDetector` when a *playable* game's heartbeat frame counter stalls
///     while RPCS3 still reports `running` (16.7.1).
///
/// On either trigger it runs the chosen recovery UX — **auto cover → restart →
/// restore** (decided 2026-05-30): kill the dead/hung emulator, flip the
/// launcher to its loading cover + broadcast `Event::GameRecovering` (phones
/// show a transient "reconnecting" overlay rather than the terminal crash
/// screen), relaunch the **same** game via a `DriverJob::BootDirect` (so the
/// restart goes through the one IPC/UIA-aware boot path the worker already
/// owns — no bespoke `launch_with_eboot`), then re-place the figures that were
/// on the portal via [`restore_portal_figures`]. Success is signalled by
/// `BootDirect`'s own `GameChanged { current: Some(_) }`, which dismisses the
/// phone overlay. If every restart attempt (up to `MAX_RESPAWNS`) fails it gives
/// up: flips the launcher to `Crashed` and emits the terminal `GameCrashed`.
///
/// Clean shutdowns don't fire it: `/api/quit?force=true` and `/api/shutdown`
/// drain `guard.process` before killing, and a graceful quit stops the game
/// without the process dying under us.
pub fn spawn_crash_watchdog(
    rpcs3: Arc<Mutex<RpcsLifecycle>>,
    portal: Arc<Mutex<[SlotState; SLOT_COUNT]>>,
    events: broadcast::Sender<Event>,
    launcher_status: Arc<std::sync::Mutex<LauncherStatus>>,
    driver_tx: mpsc::Sender<DriverJob>,
    figures: Arc<Vec<Figure>>,
    interval: std::time::Duration,
) {
    /// Cap on consecutive restart attempts within a single recovery before we
    /// give up and surface the terminal crash overlay. Repeated boot failures
    /// mean something fundamental is wrong (bad install, missing firmware) —
    /// spamming relaunches just wastes cycles.
    const MAX_RESPAWNS: u32 = 3;
    /// Budget for the polite kill of a (possibly hung) emulator before the Job
    /// Object force-terminates it. A frozen RPCS3 won't honour `WM_CLOSE`, so
    /// this is mostly the floor before the forced path runs.
    const KILL_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(5);
    /// Budget for a single restart's spawn → playable. First-launch shader
    /// compile is already cached by the time we're recovering, but keep it
    /// generous to ride out a cold RSX cache after a hard kill.
    const BOOT_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(120);
    /// After a successful restart, how long to hold the loading cover waiting
    /// for the STATE poller to re-latch `game_playable` before revealing.
    const REVEAL_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(15);

    tokio::spawn(async move {
        let mut ticker = tokio::time::interval(interval);
        // Skip the immediate first tick — `interval` fires once on start.
        ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        ticker.tick().await;
        loop {
            ticker.tick().await;

            // Read the freeze flag (set by the IPC STATE poller) without holding
            // the lifecycle lock.
            let frozen = launcher_status.lock().map(|s| s.frozen).unwrap_or(false);

            let mut guard = rpcs3.lock().await;
            let crashed = match guard.process.as_mut() {
                Some(p) => !p.is_alive(),
                None => false,
            };
            if !crashed && !frozen {
                drop(guard);
                continue;
            }
            // Capture recovery context and detach the (dead or hung) process so
            // a second tick can't double-fire while we recover.
            let game = guard.current.take();
            let eboot = guard.current_eboot.take();
            let proc = guard.process.take();
            drop(guard);

            let game_name = game.as_ref().map(|g| g.display_name.clone());
            let verb = if crashed {
                "exited unexpectedly"
            } else {
                "stopped responding"
            };
            let message = match &game_name {
                Some(n) => format!("{n} {verb}"),
                None => format!("RPCS3 {verb}"),
            };
            warn!(crashed, frozen, message = %message, "supervisor: recovery triggered");

            // Snapshot what's on the portal BEFORE we reset it, so we can
            // re-place it after the restart.
            let to_restore = {
                let p = portal.lock().await;
                placed_figures_to_restore(&p)
            };

            // Kill the process. For a crash it's already dead
            // (`shutdown_graceful_to_hwnd` short-circuits on AlreadyExited);
            // for a freeze it's hung — WM_CLOSE to the IPC game-window handle,
            // then a forced Job-Object kill (which also cleans `RPCS3.buf`).
            if let Some(mut proc) = proc {
                let hwnd = launcher_status
                    .lock()
                    .ok()
                    .and_then(|s| s.game_window_handle);
                let _ = tokio::task::spawn_blocking(move || {
                    let _ = proc.shutdown_graceful_to_hwnd(hwnd, KILL_TIMEOUT);
                    // proc drops here — Job Object reaps any survivors.
                })
                .await;
            }

            // Enter the loading cover (NOT the Crashed screen — we're
            // auto-recovering) and tell phones we're reconnecting.
            if let Ok(mut st) = launcher_status.lock() {
                st.rpcs3_running = false;
                st.current_game = None;
                st.game_playable = false;
                st.frozen = false;
                st.game_window_handle = None;
                // Drop the stale render-layer contextId too — the freshly-booted
                // game publishes a new CAContextID; clearing it makes the launcher
                // detach the old CALayerHost and re-attach on the new id (P8).
                st.game_surface_context_id = None;
                st.loading_game = game_name.clone();
                st.screen = LauncherScreen::Main;
            }
            *portal.lock().await = std::array::from_fn(|_| SlotState::Empty);
            let _ = events.send(Event::PortalSnapshot {
                slots: std::array::from_fn(|_| SlotState::Empty),
            });
            let _ = events.send(Event::GameRecovering {
                message: message.clone(),
            });

            // Without a game + EBOOT we have nothing to relaunch — fall back to
            // the terminal crash overlay so the user can pick a game again.
            let (Some(game), Some(eboot)) = (game, eboot) else {
                warn!("supervisor: no game/EBOOT recorded — can't auto-restart; showing Crashed");
                if let Ok(mut st) = launcher_status.lock() {
                    st.loading_game = None;
                    st.screen = LauncherScreen::Crashed {
                        message: message.clone(),
                    };
                }
                let _ = events.send(Event::GameCrashed { message });
                let _ = events.send(Event::GameChanged { current: None });
                continue;
            };

            // Relaunch the same game through the worker's BootDirect path, with
            // retries. BootDirect re-establishes lifecycle + launcher state and
            // broadcasts GameChanged { current: Some(_) } on success.
            let mut attempt = 0u32;
            let recovered = loop {
                attempt += 1;
                // Let OS cleanup (handle release, child teardown, socket
                // re-bind) settle before the new launch.
                tokio::time::sleep(std::time::Duration::from_millis(800)).await;
                let (done_tx, done_rx) = tokio::sync::oneshot::channel();
                let job = DriverJob::BootDirect {
                    eboot_path: eboot.clone(),
                    expected_name: game.display_name.clone(),
                    display_name: game.display_name.clone(),
                    serial: game.serial.as_str().to_string(),
                    timeout: BOOT_TIMEOUT,
                    done: done_tx,
                };
                if driver_tx.send(job).await.is_err() {
                    warn!("supervisor: driver worker gone; aborting recovery");
                    break false;
                }
                match done_rx.await {
                    Ok(Ok(())) => {
                        info!(attempt, "supervisor: RPCS3 restart succeeded");
                        break true;
                    }
                    Ok(Err(e)) => warn!(attempt, "supervisor: restart failed: {e}"),
                    Err(_) => warn!(attempt, "supervisor: restart done-channel dropped"),
                }
                if attempt >= MAX_RESPAWNS {
                    break false;
                }
            };

            if recovered {
                // Re-place the figures that were on the portal, then hold the
                // cover until the STATE poller re-latches `game_playable` so the
                // reveal lands on a rendering game rather than flashing the QR.
                restore_portal_figures(&portal, &events, &driver_tx, &figures, &to_restore).await;
                let reveal_deadline = tokio::time::Instant::now() + REVEAL_TIMEOUT;
                loop {
                    let playable = launcher_status
                        .lock()
                        .map(|s| s.game_playable)
                        .unwrap_or(true);
                    if playable || tokio::time::Instant::now() >= reveal_deadline {
                        break;
                    }
                    tokio::time::sleep(std::time::Duration::from_millis(200)).await;
                }
                if let Ok(mut st) = launcher_status.lock() {
                    st.loading_game = None;
                }
            } else {
                if let Ok(mut st) = launcher_status.lock() {
                    st.loading_game = None;
                    st.screen = LauncherScreen::Crashed {
                        message: message.clone(),
                    };
                }
                let _ = events.send(Event::GameCrashed { message });
                let _ = events.send(Event::GameChanged { current: None });
            }
        }
    });
}

/// One-shot watcher for the on-demand RPCS3 **settings GUI** (PLAN 16.9.3).
/// Spawned by `/api/rpcs3/settings` right after it launches the GUI. Polls the
/// `config_gui` handle every `interval`, and the first time it sees the user has
/// closed RPCS3, it reaps the handle, clears `LauncherStatus.config_gui_open`
/// (the launcher un-minimises), and broadcasts `Event::Rpcs3SettingsChanged {
/// open: false }` so phones dismiss the "configuring on the TV…" overlay. Then it
/// returns — one watcher per settings session, no permanent task.
pub fn spawn_config_gui_watcher(
    rpcs3: Arc<Mutex<RpcsLifecycle>>,
    launcher_status: Arc<std::sync::Mutex<LauncherStatus>>,
    events: broadcast::Sender<Event>,
    interval: std::time::Duration,
) {
    tokio::spawn(async move {
        let mut ticker = tokio::time::interval(interval);
        ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        ticker.tick().await;
        loop {
            ticker.tick().await;
            let mut guard = rpcs3.lock().await;
            let still_open = match guard.config_gui.as_mut() {
                Some(p) => p.is_alive(),
                // Already reaped (e.g. by a relaunch's stale-handle drop) — nothing
                // left for this watcher to do.
                None => false,
            };
            if still_open {
                continue;
            }
            let _ = guard.config_gui.take();
            drop(guard);
            if let Ok(mut st) = launcher_status.lock() {
                st.config_gui_open = false;
            }
            let _ = events.send(Event::Rpcs3SettingsChanged { open: false });
            info!("RPCS3 settings GUI closed — launcher restored, portal available again");
            return;
        }
    });
}

/// Pure decision for the connectivity watchdog (PLAN 17.1): raise the "phones
/// can't reach us" warning only once the server is ready, the grace window has
/// elapsed since it became ready, and **no** client has ever connected this
/// session. Split out so the threshold logic unit-tests without a runtime.
fn should_warn_no_connectivity(
    server_ready: bool,
    ever_connected: bool,
    ready_elapsed: std::time::Duration,
    grace: std::time::Duration,
) -> bool {
    server_ready && !ever_connected && ready_elapsed >= grace
}

/// Spawn the connectivity watchdog (PLAN 17.1). Once the server is up and
/// showing the join QR, if **no** phone has connected within `grace`, it raises
/// `LauncherStatus.connectivity_warning` (and snapshots the firewall status via
/// PLAN 17.2 so the card can explain it) — the launcher then shows a "Trouble
/// connecting?" card with the raw-IP URL + a one-click firewall fix. The instant
/// any client connects it clears the warning and stops watching: a successful
/// connection proves reachability, so there's nothing left to diagnose this
/// session. Idle after that — re-armed only on the next server start.
pub fn spawn_connectivity_watchdog(
    launcher_status: Arc<std::sync::Mutex<LauncherStatus>>,
    connected_clients: Arc<std::sync::atomic::AtomicUsize>,
    bind_port: u16,
    grace: std::time::Duration,
    interval: std::time::Duration,
) {
    use std::sync::atomic::Ordering;
    tokio::spawn(async move {
        let mut ticker = tokio::time::interval(interval);
        ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        ticker.tick().await;

        let mut ready_at: Option<tokio::time::Instant> = None;
        let mut ever_connected = false;
        let mut warned = false;

        loop {
            ticker.tick().await;

            if connected_clients.load(Ordering::Relaxed) > 0 {
                ever_connected = true;
            }
            if ever_connected {
                // Reachability proven — clear any warning and stop watching.
                if let Ok(mut st) = launcher_status.lock() {
                    st.connectivity_warning = false;
                }
                info!("connectivity watchdog: a phone connected — diagnostics dismissed");
                return;
            }

            let server_ready = launcher_status
                .lock()
                .map(|s| s.server_ready)
                .unwrap_or(false);
            if server_ready && ready_at.is_none() {
                ready_at = Some(tokio::time::Instant::now());
            }
            let ready_elapsed = ready_at.map(|t| t.elapsed()).unwrap_or_default();

            if !warned
                && should_warn_no_connectivity(server_ready, ever_connected, ready_elapsed, grace)
            {
                // Off-reactor: the firewall check is a blocking COM round-trip.
                let status = tokio::task::spawn_blocking(move || {
                    crate::firewall::check_inbound_rule(bind_port)
                })
                .await
                .unwrap_or(crate::firewall::FirewallStatus::Unknown);
                if let Ok(mut st) = launcher_status.lock() {
                    st.firewall_status = status;
                    st.connectivity_warning = true;
                }
                warned = true;
                warn!(
                    ?status,
                    grace_secs = grace.as_secs(),
                    "no phone connected within the grace window — raising connectivity warning"
                );
                // Keep ticking: a later connection still clears + exits above.
            }
        }
    });
}

/// Spawn the driver worker. Owns the `PortalDriver` and serialises all access.
///
/// `profiles` + `sessions` are threaded in so the worker can persist the
/// current portal layout after each successful mutation (PLAN 3.12.1) —
/// each unlocked profile's `sessions` row gets the fresh JSON so that on
/// next unlock we can offer a resume prompt.
#[allow(clippy::too_many_arguments)]
pub fn spawn_driver_worker(
    driver: Arc<dyn PortalDriver>,
    portal: Arc<Mutex<[SlotState; SLOT_COUNT]>>,
    events: broadcast::Sender<Event>,
    profiles: crate::profiles::ProfileStore,
    sessions: Arc<crate::profiles::SessionRegistry>,
    figures: Arc<Vec<Figure>>,
    rpcs3: Arc<Mutex<RpcsLifecycle>>,
    rpcs3_exe: PathBuf,
    config_dir: PathBuf,
    launcher_status: Arc<std::sync::Mutex<LauncherStatus>>,
) -> mpsc::Sender<DriverJob> {
    let (tx, mut rx) = mpsc::channel::<DriverJob>(32);

    tokio::spawn(async move {
        // Initial snapshot — best effort; a subsequent RefreshPortal will retry
        // if this fails (e.g. dialog not open yet).
        if let Err(e) = refresh(&driver, &portal, &events, &figures).await {
            info!("initial portal refresh failed (expected if dialog isn't open yet): {e}");
        }

        while let Some(job) = rx.recv().await {
            let mutation = matches!(
                &job,
                DriverJob::LoadFigure { .. } | DriverJob::ClearSlot { .. }
            );
            if let Err(e) = handle_job(
                job,
                &driver,
                &portal,
                &events,
                &figures,
                &rpcs3,
                &rpcs3_exe,
                &config_dir,
                &launcher_status,
            )
            .await
            {
                error!("driver job error: {e}");
                let _ = events.send(Event::Error {
                    message: e.to_string(),
                });
            }
            if mutation {
                // Best-effort layout persistence: write the current portal
                // snapshot to every unlocked profile's `sessions` row so an
                // unlock-resume prompt can offer it. Failures are logged,
                // not surfaced to the phone — the mutation itself succeeded
                // and a missed layout save is a minor degradation.
                persist_layout(&portal, &profiles, &sessions).await;
            }
        }
    });

    tx
}

#[allow(clippy::too_many_arguments)]
async fn handle_job(
    job: DriverJob,
    driver: &Arc<dyn PortalDriver>,
    portal: &Arc<Mutex<[SlotState; SLOT_COUNT]>>,
    events: &broadcast::Sender<Event>,
    figures: &[Figure],
    rpcs3: &Arc<Mutex<RpcsLifecycle>>,
    rpcs3_exe: &Path,
    config_dir: &Path,
    launcher_status: &Arc<std::sync::Mutex<LauncherStatus>>,
) -> Result<()> {
    match job {
        DriverJob::LoadFigure {
            slot,
            figure_id,
            path,
            placed_by,
            canonical_name,
        } => {
            // HTTP handler already set Loading and broadcast it.
            let d = driver.clone();
            let fid = figure_id.clone();
            let result = tokio::task::spawn_blocking(move || -> Result<String> {
                d.open_dialog()?;
                d.load(slot, &path)
            })
            .await?;

            match result {
                Ok(_driver_reported_name) => {
                    // Use the canonical name from the pack index, not
                    // whatever the driver read back. See comment on
                    // DriverJob::LoadFigure.canonical_name.
                    set_and_broadcast(
                        portal,
                        events,
                        slot,
                        SlotState::Loaded {
                            figure_id: Some(fid),
                            display_name: canonical_name,
                            placed_by,
                        },
                    )
                    .await;
                }
                Err(e) => {
                    restore_after_failure(driver, portal, events, slot, &e.to_string(), figures)
                        .await;
                }
            }
        }
        DriverJob::ClearSlot { slot } => {
            // HTTP handler already set Loading and broadcast it.
            let d = driver.clone();
            let result = tokio::task::spawn_blocking(move || -> Result<()> {
                d.open_dialog()?;
                d.clear(slot)
            })
            .await?;

            match result {
                Ok(()) => {
                    set_and_broadcast(portal, events, slot, SlotState::Empty).await;
                }
                Err(e) => {
                    restore_after_failure(driver, portal, events, slot, &e.to_string(), figures)
                        .await;
                }
            }
        }
        DriverJob::RefreshPortal => {
            refresh(driver, portal, events, figures).await?;
        }
        DriverJob::BootDirect {
            eboot_path,
            expected_name,
            display_name,
            serial,
            timeout,
            done,
        } => {
            // PLAN 16.11: the IPC boot path (patched RPCS3 + AF_UNIX) is
            // **cross-platform** — Windows *and* macOS/Linux drive the same
            // --no-gui launch + STATE-liveness wait, the enum picking the
            // platform process impl (UiaRpcsProcess / UnixRpcsProcess). The
            // legacy UIA path (FPS:-viewport scrape + Skylanders Manager dialog)
            // is Windows-only; the mock driver (any platform) spawns nothing.
            // Keyed on the driver advertising an IPC socket — the mock returns
            // `None` and falls to the no-spawn arm, so /api/launch still
            // round-trips and mock-driver tests don't need a real RPCS3.
            let result: Result<Option<RpcsProcess>> = if let Some(ipc_path) =
                driver.ipc_socket_path()
            {
                let d = driver.clone();
                let exe_owned = rpcs3_exe.to_path_buf();
                let config_dir_owned = config_dir.to_path_buf();
                let eboot_owned = eboot_path.clone();
                let display_name_for_blocking = display_name.clone();
                let status_for_blocking = launcher_status.clone();
                let _ = &expected_name;
                tokio::task::spawn_blocking(move || -> Result<Option<RpcsProcess>> {
                    // PLAN 16.6.1 / 16.11 — patched RPCS3: launch --no-gui with the
                    // IPC socket, then wait on the clean liveness signal (STATE:
                    // status=running + frames advancing + compile complete) instead
                    // of scraping a window title. No Skylanders Manager dialog → no
                    // open_dialog.
                    //
                    // PLAN 15.12 (recorder in-game tier): SKYLANDER_BOOT_SAVESTATE
                    // boots a pre-made save state (straight to the in-game portal)
                    // instead of the EBOOT, with the save-state-only RPCS3 settings
                    // (ASMJIT SPU + Compatible Savestate Mode) swapped into the real
                    // global config transiently for the boot and restored once the
                    // emulator is up — the guard drops when this closure returns,
                    // after `is_playable`, by which point RPCS3 has read the config.
                    // Dev/recorder knob only; unset in normal use.
                    let savestate = std::env::var_os("SKYLANDER_BOOT_SAVESTATE").map(PathBuf::from);
                    let _config_guard = match &savestate {
                        Some(_) => {
                            // RPCS3 keeps config.yml at the config-dir ROOT in the
                            // current layout; older installs used config/config.yml.
                            // Prefer the root, fall back to the legacy path — the same
                            // macOS fix as games.yml (config/ lookup found nothing, so
                            // apply_savestate_config errored before its first log and
                            // the save-state boot silently never started).
                            let root = config_dir_owned.join("config.yml");
                            let legacy = config_dir_owned.join("config").join("config.yml");
                            let cfg = if root.is_file() { root } else { legacy };
                            Some(crate::rpcs3_config::apply_savestate_config(&cfg)?)
                        }
                        None => None,
                    };
                    let boot_target: &std::path::Path =
                        savestate.as_deref().unwrap_or(&eboot_owned);
                    if savestate.is_some() {
                        info!(
                            savestate = %boot_target.display(),
                            "PLAN 15.12: booting save state (recorder in-game tier)"
                        );
                    }
                    let mut proc = RpcsProcess::launch_no_gui(
                        &exe_owned,
                        boot_target,
                        &ipc_path,
                        Some(&config_dir_owned),
                    )?;
                    let deadline = std::time::Instant::now() + timeout;
                    loop {
                        if !proc.is_alive() {
                            return Err(anyhow::anyhow!("patched RPCS3 exited during no-GUI boot"));
                        }
                        // Transient IPC errors (socket not up yet, mid-boot) just
                        // don't match here, so the loop retries until the deadline.
                        if let Ok(Some(state)) = d.emu_state()
                            && state.is_playable()
                        {
                            break;
                        }
                        if std::time::Instant::now() >= deadline {
                            return Err(anyhow::anyhow!(
                                "patched RPCS3 never reached a playable state within {timeout:?}"
                            ));
                        }
                        std::thread::sleep(std::time::Duration::from_millis(250));
                    }
                    // PLAN 15.12.4 — a save-state resume leaves the guest's USB portal
                    // handles stale (RPCS3 rebuilds the LDD registry empty), so the game
                    // can't see the portal and shows "reconnect the Portal of Power".
                    // Once it's playable and has reached that state, hot-plug the portal
                    // (RECONNECT: re-register the LDD + DETACH/ATTACH) so the game
                    // re-enumerates it — subsequent live LOADs are then read in-game.
                    // No-op on a normal (EBOOT) boot. Timing can tighten (proven 2026-06-10).
                    if savestate.is_some() {
                        std::thread::sleep(std::time::Duration::from_secs(10));
                        match d.reconnect() {
                            Ok(()) => info!(
                                "PLAN 15.12.4: hot-plugged the portal (RECONNECT) after save-state resume"
                            ),
                            Err(e) => {
                                warn!(error = %e, "PLAN 15.12.4: portal RECONNECT after resume failed")
                            }
                        }
                    }
                    // game_window_handle is published continuously by the STATE
                    // poller (early, as soon as the window exists) — not here.
                    if let Ok(mut st) = status_for_blocking.lock() {
                        st.rpcs3_running = true;
                        st.current_game = Some(display_name_for_blocking.clone());
                    }
                    Ok(Some(proc))
                })
                .await
                .map_err(|e| anyhow::anyhow!("BootDirect task panicked: {e}"))
                .and_then(|r| r)
            } else {
                // Non-IPC driver. UIA (Windows, stock RPCS3) needs the real spawn
                // + FPS:-viewport poll + Skylanders Manager dialog; the mock
                // driver (any platform) spawns nothing and keeps the
                // `RpcsProcess::mock()` installed at startup.
                #[cfg(windows)]
                let r: Result<Option<RpcsProcess>> = {
                    let d = driver.clone();
                    let exe_owned = rpcs3_exe.to_path_buf();
                    let eboot_owned = eboot_path.clone();
                    let display_name_for_blocking = display_name.clone();
                    // Match the viewport title on `[<SERIAL>]` (e.g. `[BLUS30968]`).
                    // Serial brackets are deterministic; matching on display_name
                    // fails when RPCS3's viewport title drops punctuation we put in
                    // SKYLANDERS_SERIALS — `"Skylanders: Giants"` (catalogue) vs
                    // `"Skylanders Giants [BLUS30968]"` (viewport).
                    let expected_marker = format!("[{}]", serial);
                    let status_for_blocking = launcher_status.clone();
                    tokio::task::spawn_blocking(move || -> Result<Option<RpcsProcess>> {
                        let mut proc = RpcsProcess::launch_with_eboot(&exe_owned, &eboot_owned)?;
                        proc.wait_ready(std::time::Duration::from_secs(45))
                            .context("RPCS3 main window never appeared after EBOOT spawn")?;
                        let deadline = std::time::Instant::now() + timeout;
                        loop {
                            if let Some(title) = skylander_rpcs3_control::read_viewport_title()
                                && title.contains(&expected_marker)
                            {
                                break;
                            }
                            if std::time::Instant::now() >= deadline {
                                return Err(anyhow::anyhow!(
                                    "FPS: viewport with serial marker {expected_marker:?} \
                                     never appeared within {:?}",
                                    timeout
                                ));
                            }
                            std::thread::sleep(std::time::Duration::from_millis(250));
                        }
                        // Flip rpcs3_running + current_game on launcher_status BEFORE
                        // open_dialog: the launcher's per-frame
                        // `push_rpcs3_main_to_bottom_via_win32` is gated on
                        // `rpcs3_running && current_game.is_some()`; without that gate
                        // first, open_dialog's UIA Invoke transiently promotes the main
                        // window over the game viewport and leaves the menu bar on top
                        // (HTPC bug 2026-05-04). This `std::sync::Mutex` lock is fine
                        // from spawn_blocking — it's not the tokio Mutex.
                        if let Ok(mut st) = status_for_blocking.lock() {
                            st.rpcs3_running = true;
                            st.current_game = Some(display_name_for_blocking.clone());
                            // UIA fallback playable signal (PLAN 16.6.3.2): the loop
                            // above only breaks once the rendering `FPS:` viewport is
                            // up, so reveal now (the retired FPS sampler used to).
                            st.game_playable = true;
                        }
                        d.open_dialog().context("open_dialog after EBOOT boot")?;
                        Ok(Some(proc))
                    })
                    .await
                    .map_err(|e| anyhow::anyhow!("BootDirect task panicked: {e}"))
                    .and_then(|r| r)
                };
                #[cfg(not(windows))]
                let r: Result<Option<RpcsProcess>> = {
                    // Mock driver on Mac/Linux: no real RPCS3, no spawn, no poll.
                    let _ = &serial;
                    Ok(None)
                };
                r
            };

            let outcome = match result {
                Ok(maybe_proc) => {
                    let mut guard = rpcs3.lock().await;
                    if let Some(new_proc) = maybe_proc {
                        // Drop any previous process (Drop kills via JobObject).
                        let _ = guard.process.take();
                        guard.process = Some(new_proc);
                    }
                    guard.current = Some(GameLaunched {
                        serial: skylander_core::GameSerial::new(&serial),
                        display_name: display_name.clone(),
                    });
                    guard.current_eboot = Some(eboot_path.clone());
                    drop(guard);
                    if let Ok(mut st) = launcher_status.lock() {
                        st.rpcs3_running = true;
                        st.current_game = Some(display_name.clone());
                    }
                    let _ = events.send(Event::GameChanged {
                        current: Some(GameLaunched {
                            serial: skylander_core::GameSerial::new(&serial),
                            display_name: display_name.clone(),
                        }),
                    });
                    Ok(())
                }
                Err(e) => Err(e),
            };
            let _ = done.send(outcome);
        }
        DriverJob::WindowSet { x, y, w, h } => {
            // PLAN B.2 — route the macOS Desktop-mode game-window fit through the
            // driver's IPC P7 `WINDOW_SET`. Runs off-reactor (the blocking IPC
            // round-trip) like the load/clear ops. Not a mutation — no layout
            // persist. `Context` is gated to `cfg(windows)` at module scope, so
            // pull the extension trait in locally for the cross-platform `.context`.
            use anyhow::Context as _;
            let d = driver.clone();
            tokio::task::spawn_blocking(move || d.window_set(x, y, w, h))
                .await
                .context("window_set task")??;
        }
    }
    Ok(())
}

/// Save the current 8-slot portal state to `sessions.last_portal_layout_json`
/// for every currently-unlocked profile. See PLAN 3.12 for the resume-prompt
/// consumer side. Best-effort: DB errors are logged, not propagated.
async fn persist_layout(
    portal: &Arc<Mutex<[SlotState; SLOT_COUNT]>>,
    profiles: &crate::profiles::ProfileStore,
    sessions: &Arc<crate::profiles::SessionRegistry>,
) {
    let snapshot: [SlotState; SLOT_COUNT] = portal.lock().await.clone();
    let json = match serde_json::to_string(&snapshot) {
        Ok(s) => s,
        Err(e) => {
            warn!("serialise portal snapshot: {e}");
            return;
        }
    };
    let ids = sessions.all_ids().await;
    let mut seen_profiles = std::collections::HashSet::<String>::new();
    for sid in ids {
        if let Some(profile_id) = sessions.profile_of(sid).await {
            if !seen_profiles.insert(profile_id.clone()) {
                continue; // same profile on two phones — save once
            }
            if let Err(e) = profiles.save_portal_layout(&profile_id, &json).await {
                warn!("save_portal_layout({profile_id}): {e}");
            }
        }
    }
}

/// After a driver error: emit an `Error` event for the toast, then re-read
/// the portal to restore truthful slot state. If the re-read fails (unusual),
/// fall back to `Empty` for the slot so the UI isn't stuck showing Loading.
async fn restore_after_failure(
    driver: &Arc<dyn PortalDriver>,
    portal: &Arc<Mutex<[SlotState; SLOT_COUNT]>>,
    events: &broadcast::Sender<Event>,
    slot: SlotIndex,
    message: &str,
    figures: &[Figure],
) {
    let _ = events.send(Event::Error {
        message: message.to_string(),
    });

    let d = driver.clone();
    let snapshot = tokio::task::spawn_blocking(move || d.read_slots()).await;

    let truth = match snapshot {
        Ok(Ok(snap)) => reconcile_slot_names(snap, figures)[slot.as_usize()].clone(),
        _ => SlotState::Empty,
    };
    set_and_broadcast(portal, events, slot, truth).await;
}

async fn refresh(
    driver: &Arc<dyn PortalDriver>,
    portal: &Arc<Mutex<[SlotState; SLOT_COUNT]>>,
    events: &broadcast::Sender<Event>,
    figures: &[Figure],
) -> Result<()> {
    let d = driver.clone();
    let snap = tokio::task::spawn_blocking(move || -> Result<[SlotState; SLOT_COUNT]> {
        d.open_dialog()?;
        d.read_slots()
    })
    .await??;

    let snap = reconcile_slot_names(snap, figures);
    *portal.lock().await = snap.clone();
    let _ = events.send(Event::PortalSnapshot { slots: snap });
    Ok(())
}

async fn set_and_broadcast(
    portal: &Arc<Mutex<[SlotState; SLOT_COUNT]>>,
    events: &broadcast::Sender<Event>,
    slot: SlotIndex,
    state: SlotState,
) {
    portal.lock().await[slot.as_usize()] = state.clone();
    let _ = events.send(Event::SlotChanged { slot, state });
}

/// Walk `slots` and flip every `Loaded { placed_by: Some(p) }` whose `p`
/// equals `profile_id` to `Loading { placed_by: None }`, returning the
/// (index, new-state) pairs so the caller can broadcast + enqueue
/// clears. Pure function over an in-memory snapshot — no I/O — so
/// disconnect-cleanup behavior is unit-testable without spinning up a
/// full `AppState`.
///
/// Deliberately skips:
///   * `Empty` / `Error` — no owner to match.
///   * `Loading` — the in-flight driver job is mid-ack; rewriting it
///     would race with the worker's own broadcast of the Loaded result.
///   * `Loaded { placed_by: None }` — legacy rows from before 4.18.17.
fn flip_loaded_owned_to_loading(
    slots: &mut [SlotState; SLOT_COUNT],
    profile_id: &str,
) -> Vec<(SlotIndex, SlotState)> {
    let mut out = Vec::new();
    for (i, s) in slots.iter_mut().enumerate() {
        if let SlotState::Loaded {
            placed_by: Some(owner),
            ..
        } = s
            && owner == profile_id
        {
            let Ok(slot) = SlotIndex::new(i as u8) else {
                continue;
            };
            let loading = SlotState::Loading {
                figure_id: None,
                placed_by: None,
            };
            *s = loading.clone();
            out.push((slot, loading));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use skylander_core::{Category, Element, GameOfOrigin};
    use std::path::PathBuf;

    // ---- freeze detector (PLAN 16.7.1) --------------------------------------

    #[test]
    fn freeze_detector_fires_only_after_threshold() {
        let mut d = FreezeDetector::new(3);
        // Stalled (not advancing), running, playable — but not yet at threshold.
        assert!(!d.observe(false, true, true));
        assert!(!d.observe(false, true, true));
        // 3rd consecutive stall tick → frozen.
        assert!(d.observe(false, true, true));
        // Stays frozen while still stalled.
        assert!(d.observe(false, true, true));
    }

    #[test]
    fn freeze_detector_resets_when_frames_advance() {
        let mut d = FreezeDetector::new(2);
        assert!(!d.observe(false, true, true)); // stall 1 (< threshold)
        assert!(d.observe(false, true, true)); // stall 2 -> frozen
        // A single advancing frame clears the stall and the frozen state.
        assert!(!d.observe(true, true, true));
        assert!(!d.observe(false, true, true)); // stall back to 1
    }

    #[test]
    fn freeze_detector_ignores_stalls_before_playable() {
        // During compile/boot the game isn't playable yet — frame stalls there
        // are normal and must not be mistaken for a freeze.
        let mut d = FreezeDetector::new(2);
        assert!(!d.observe(false, true, false));
        assert!(!d.observe(false, true, false));
        assert!(!d.observe(false, true, false));
    }

    #[test]
    fn freeze_detector_ignores_paused_status() {
        // A deliberate pause reports a non-"running" status, so `running=false`;
        // that's not a freeze no matter how long frames sit still.
        let mut d = FreezeDetector::new(2);
        assert!(!d.observe(false, false, true));
        assert!(!d.observe(false, false, true));
        assert!(!d.observe(false, false, true));
    }

    #[test]
    fn freeze_detector_reset_clears_stall() {
        let mut d = FreezeDetector::new(2);
        assert!(!d.observe(false, true, true));
        d.reset();
        // After reset the next stall starts the count over.
        assert!(!d.observe(false, true, true));
        assert!(d.observe(false, true, true));
    }

    // ---- connectivity watchdog (PLAN 17.1) ----------------------------------

    #[test]
    fn connectivity_warns_only_after_grace_with_no_clients() {
        let grace = std::time::Duration::from_secs(60);
        // Not ready yet → never warn, regardless of elapsed.
        assert!(!should_warn_no_connectivity(
            false,
            false,
            std::time::Duration::from_secs(120),
            grace
        ));
        // Ready, no clients, but still inside the grace window → don't warn yet.
        assert!(!should_warn_no_connectivity(
            true,
            false,
            std::time::Duration::from_secs(30),
            grace
        ));
        // Ready, no clients, past grace → warn.
        assert!(should_warn_no_connectivity(
            true,
            false,
            std::time::Duration::from_secs(75),
            grace
        ));
        // A client has connected → never warn, even past grace.
        assert!(!should_warn_no_connectivity(
            true,
            true,
            std::time::Duration::from_secs(75),
            grace
        ));
    }

    // ---- portal restore (PLAN 16.7.2) ---------------------------------------

    fn loaded_slot(fid: Option<&str>, owner: Option<&str>) -> SlotState {
        SlotState::Loaded {
            figure_id: fid.map(FigureId::new),
            display_name: "X".into(),
            placed_by: owner.map(|s| s.to_string()),
        }
    }

    #[test]
    fn restore_capture_takes_only_loaded_with_figure_id() {
        let mut slots: [SlotState; SLOT_COUNT] = std::array::from_fn(|_| SlotState::Empty);
        slots[0] = loaded_slot(Some("aaaa"), Some("alice"));
        slots[1] = SlotState::Loading {
            figure_id: Some(FigureId::new("bbbb")),
            placed_by: Some("bob".into()),
        }; // mid-flight — skipped
        slots[2] = loaded_slot(None, Some("carol")); // name-only read — unrestorable
        slots[3] = SlotState::Error {
            message: "boom".into(),
        };
        slots[5] = loaded_slot(Some("ffff"), None); // no owner, but still captured

        let got = placed_figures_to_restore(&slots);
        assert_eq!(got.len(), 2, "only the two Loaded-with-id slots");
        assert_eq!(got[0].0.as_u8(), 0);
        assert_eq!(got[0].1.as_str(), "aaaa");
        assert_eq!(got[0].2.as_deref(), Some("alice"));
        // Ownerless Loaded is captured here (the owner check happens at restore
        // time, where a None owner means "skip — no working copy to resolve").
        assert_eq!(got[1].0.as_u8(), 5);
        assert_eq!(got[1].1.as_str(), "ffff");
        assert_eq!(got[1].2, None);
    }

    #[tokio::test]
    async fn restore_requeues_owned_loads_and_skips_unrestorable() {
        use tempfile::TempDir;
        let tmp = TempDir::new().unwrap();
        let pack = tmp.path().join("restore_known.sky");
        std::fs::write(&pack, b"fresh-pack").unwrap();
        let known = Figure {
            sky_path: pack,
            ..fig("restore_known_id", "Restore Knownburst")
        };
        let figures = vec![known.clone()];

        let portal: Arc<Mutex<[SlotState; SLOT_COUNT]>> =
            Arc::new(Mutex::new(std::array::from_fn(|_| SlotState::Empty)));
        let (events, _erx) = broadcast::channel::<Event>(32);
        let (tx, mut rx) = mpsc::channel::<DriverJob>(32);

        let prof = "restore_test_profile";
        let to_restore = vec![
            // (a) restorable: known figure + owner.
            (
                SlotIndex::new(0).unwrap(),
                known.id.clone(),
                Some(prof.into()),
            ),
            // (b) skipped: no owner → no per-profile working copy.
            (SlotIndex::new(1).unwrap(), known.id.clone(), None),
            // (c) skipped: figure not in the library.
            (
                SlotIndex::new(2).unwrap(),
                FigureId::new("ghost_not_in_library"),
                Some(prof.into()),
            ),
        ];

        restore_portal_figures(&portal, &events, &tx, &figures, &to_restore).await;
        drop(tx); // close the channel so the drain terminates.

        let mut jobs = Vec::new();
        while let Some(j) = rx.recv().await {
            jobs.push(j);
        }
        assert_eq!(jobs.len(), 1, "only the restorable slot enqueues a load");
        let DriverJob::LoadFigure {
            slot,
            figure_id,
            path,
            placed_by,
            canonical_name,
        } = &jobs[0]
        else {
            panic!("expected a LoadFigure job, got {:?}", jobs[0]);
        };
        assert_eq!(slot.as_u8(), 0);
        assert_eq!(figure_id.as_str(), "restore_known_id");
        assert_eq!(placed_by.as_deref(), Some(prof));
        assert_eq!(canonical_name, "Restore Knownburst");
        assert!(path.exists(), "working copy forked from the pack");
        assert_eq!(std::fs::read(path).unwrap(), b"fresh-pack");

        // The restorable slot was flipped to Loading in the mirror.
        assert!(matches!(portal.lock().await[0], SlotState::Loading { .. }));

        let _ = std::fs::remove_file(path); // clean up the dev-data working copy.
    }

    fn fig(id: &str, canonical: &str) -> Figure {
        Figure {
            id: FigureId::new(id),
            canonical_name: canonical.into(),
            variant_group: canonical.into(),
            variant_tag: "base".into(),
            game: GameOfOrigin::SpyrosAdventure,
            element: Some(Element::Fire),
            category: Category::Figure,
            vehicle_terrain: None,
            sky_path: PathBuf::from("/dev/null"),
            element_icon_path: None,
            tag_identity: None,
        }
    }

    #[test]
    fn find_by_display_name_exact() {
        let figures = vec![fig("aaaa", "Lava Barf Eruptor"), fig("bbbb", "Spyro")];
        let hit = find_figure_by_display_name(&figures, "Lava Barf Eruptor").unwrap();
        assert_eq!(hit.id.as_str(), "aaaa");
    }

    #[test]
    fn find_by_display_name_is_case_and_whitespace_insensitive() {
        let figures = vec![fig("cccc", "Spyro")];
        assert!(find_figure_by_display_name(&figures, "spyro").is_some());
        assert!(find_figure_by_display_name(&figures, "  SPYRO  ").is_some());
    }

    #[test]
    fn find_by_display_name_rejects_empty_and_unknown() {
        let figures = vec![fig("dddd", "Spyro")];
        assert!(find_figure_by_display_name(&figures, "").is_none());
        assert!(find_figure_by_display_name(&figures, "   ").is_none());
        assert!(find_figure_by_display_name(&figures, "Unknown (Id:42 Var:0)").is_none());
    }

    #[test]
    fn reconcile_populates_figure_id_and_canonicalises_name() {
        let figures = vec![fig("aaaa", "Lava Barf Eruptor")];
        let mut snap: [SlotState; SLOT_COUNT] = std::array::from_fn(|_| SlotState::Empty);
        // Driver returned a lowercased name with no figure_id — the kind of
        // thing `read_slots()` produces on RefreshPortal.
        snap[3] = SlotState::Loaded {
            figure_id: None,
            display_name: "lava barf eruptor".into(),
            placed_by: None,
        };

        let reconciled = reconcile_slot_names(snap, &figures);

        match &reconciled[3] {
            SlotState::Loaded {
                figure_id: Some(id),
                display_name,
                ..
            } => {
                assert_eq!(id.as_str(), "aaaa");
                assert_eq!(display_name, "Lava Barf Eruptor");
            }
            other => panic!("expected Loaded with figure_id, got {other:?}"),
        }
    }

    #[test]
    fn reconcile_leaves_unmatched_slots_alone() {
        let figures = vec![fig("aaaa", "Spyro")];
        let mut snap: [SlotState; SLOT_COUNT] = std::array::from_fn(|_| SlotState::Empty);
        snap[0] = SlotState::Loaded {
            figure_id: None,
            display_name: "Unknown (Id:42 Var:0)".into(),
            placed_by: None,
        };

        let reconciled = reconcile_slot_names(snap, &figures);

        match &reconciled[0] {
            SlotState::Loaded {
                figure_id: None,
                display_name,
                ..
            } => {
                assert_eq!(display_name, "Unknown (Id:42 Var:0)");
            }
            other => panic!("expected Loaded with None figure_id, got {other:?}"),
        }
    }

    #[test]
    fn reconcile_does_not_touch_slots_with_existing_figure_id() {
        // If an upstream path (LoadFigure broadcast) already set figure_id,
        // reconcile must not overwrite it even if canonical_name happens to
        // match a different figure in the index.
        let figures = vec![fig("aaaa", "Spyro")];
        let mut snap: [SlotState; SLOT_COUNT] = std::array::from_fn(|_| SlotState::Empty);
        snap[0] = SlotState::Loaded {
            figure_id: Some(FigureId::new("bbbb")),
            display_name: "Spyro".into(),
            placed_by: None,
        };

        let reconciled = reconcile_slot_names(snap, &figures);

        match &reconciled[0] {
            SlotState::Loaded {
                figure_id: Some(id),
                ..
            } => assert_eq!(id.as_str(), "bbbb"),
            other => panic!("expected untouched figure_id, got {other:?}"),
        }
    }

    // ---- disconnect-cleanup helper (PLAN 3.10.9) ----------------------------

    fn loaded(owner: Option<&str>) -> SlotState {
        SlotState::Loaded {
            figure_id: Some(FigureId::new("fig")),
            display_name: "Spyro".into(),
            placed_by: owner.map(|s| s.to_string()),
        }
    }

    #[test]
    fn flip_clears_owned_loaded_slots_and_leaves_others() {
        let mut slots: [SlotState; SLOT_COUNT] = std::array::from_fn(|_| SlotState::Empty);
        slots[0] = loaded(Some("alice"));
        slots[2] = loaded(Some("bob"));
        slots[4] = loaded(Some("alice"));
        slots[5] = SlotState::Loading {
            figure_id: None,
            placed_by: Some("alice".into()),
        };
        slots[6] = loaded(None);

        let out = flip_loaded_owned_to_loading(&mut slots, "alice");

        assert_eq!(out.len(), 2, "expected two slots cleared, got {out:?}");
        assert!(matches!(slots[0], SlotState::Loading { .. }));
        assert!(matches!(slots[4], SlotState::Loading { .. }));
        // Bob's slot untouched.
        assert!(matches!(
            slots[2],
            SlotState::Loaded {
                placed_by: Some(ref p),
                ..
            } if p == "bob",
        ));
        // Already-Loading alice slot stays Loading (not rewritten).
        assert!(matches!(slots[5], SlotState::Loading { .. }));
        // Legacy Loaded-without-owner row untouched.
        assert!(matches!(
            slots[6],
            SlotState::Loaded {
                placed_by: None,
                ..
            }
        ));
    }

    #[test]
    fn flip_is_noop_when_profile_has_no_figures() {
        let mut slots: [SlotState; SLOT_COUNT] = std::array::from_fn(|_| SlotState::Empty);
        slots[1] = loaded(Some("bob"));

        let out = flip_loaded_owned_to_loading(&mut slots, "alice");

        assert!(out.is_empty());
        assert!(matches!(
            slots[1],
            SlotState::Loaded {
                placed_by: Some(ref p),
                ..
            } if p == "bob",
        ));
    }

    #[test]
    fn flip_emits_loading_placeholder_with_none_placed_by() {
        // The Loading state we emit must have placed_by=None — otherwise
        // the phone's ownership pip would linger with the departing
        // profile's tint during the clear round-trip.
        let mut slots: [SlotState; SLOT_COUNT] = std::array::from_fn(|_| SlotState::Empty);
        slots[3] = loaded(Some("alice"));

        let out = flip_loaded_owned_to_loading(&mut slots, "alice");

        assert_eq!(out.len(), 1);
        let (idx, new_state) = &out[0];
        assert_eq!(idx.as_usize(), 3);
        assert!(matches!(
            new_state,
            SlotState::Loading {
                figure_id: None,
                placed_by: None,
            }
        ));
    }
}
