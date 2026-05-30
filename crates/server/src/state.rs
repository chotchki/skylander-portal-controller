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

    tokio::spawn(async move {
        let mut ticker = tokio::time::interval(interval);
        ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        ticker.tick().await;

        let mut ready_run = 0usize;
        let mut last_frames = 0u64;

        loop {
            ticker.tick().await;

            // emu_state() + the window handle are blocking IPC round-trips — off-reactor.
            let d = driver.clone();
            let (state, game_window_handle) =
                match tokio::task::spawn_blocking(move || (d.emu_state(), d.game_window_handle()))
                    .await
                {
                    Ok((s, h)) => (s.ok().flatten(), h.ok().flatten()),
                    Err(_) => (None, None),
                };

            // Ready = compile complete + actually rendering (frames advancing).
            let ready = match &state {
                Some(s) => {
                    let advancing = s.frames > last_frames;
                    last_frames = s.frames;
                    s.is_playable() && advancing
                }
                None => {
                    last_frames = 0;
                    false
                }
            };
            ready_run = if ready { ready_run + 1 } else { 0 };
            let stable = ready_run >= SAMPLE_BUFFER;

            if let Ok(mut st) = launcher_status.lock() {
                // Publish the game-window handle as soon as the window exists (during
                // boot/compile) so the launcher slots it BELOW itself right away —
                // otherwise RPCS3's freshly-created window sits on TOP of the launcher
                // and the user sees the compile through it for the whole boot, no
                // matter what the launcher paints (PLAN 16.6.2, HTPC 2026-05-30).
                st.game_window_handle = game_window_handle;
                if !st.rpcs3_running {
                    ready_run = 0;
                    last_frames = 0;
                }
                // Latch: once playable, stay playable while running (freeze
                // detection that un-latches is the 16.7 supervisor's job).
                let new_playable = st.rpcs3_running && (st.game_playable || stable);
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

/// Spawn the RPCS3 crash watchdog. Polls the lifecycle lock once per
/// `interval` and, the first frame it sees the spawned process has died
/// while `current` is still set (i.e. nobody called `/api/quit`), treats it
/// as an unexpected exit: takes the dead `RpcsProcess` out of the lifecycle,
/// clears `current`, resets the portal snapshot, and broadcasts
/// `Event::GameCrashed` + `Event::GameChanged { current: None }` so phones
/// can render the "game crashed" overlay (PLAN 4.15.14 /
/// `docs/aesthetic/navigation.md` §3.8).
///
/// Auto-respawn (PLAN 4.15.16): after reporting the crash, the watchdog
/// immediately tries to relaunch RPCS3 at library view so the
/// always-running contract holds. If respawn fails `MAX_RESPAWNS` times
/// the launcher flips to `ServerError` with a diagnostic.
///
/// `/api/quit` (in normal mode) uses `DriverJob::StopEmulation` which
/// doesn't touch `guard.process` — the watchdog naturally won't fire on
/// clean quits. `/api/quit?force=true` and `/api/shutdown` both take the
/// process out of `guard.process` before killing it, so the watchdog
/// won't treat those as crashes either.
pub fn spawn_crash_watchdog(
    rpcs3: Arc<Mutex<RpcsLifecycle>>,
    portal: Arc<Mutex<[SlotState; SLOT_COUNT]>>,
    events: broadcast::Sender<Event>,
    launcher_status: Arc<std::sync::Mutex<LauncherStatus>>,
    rpcs3_exe: std::path::PathBuf,
    interval: std::time::Duration,
) {
    /// Cap on consecutive respawn attempts before we give up and flip
    /// the launcher to ServerError. If RPCS3 is crashing on launch
    /// repeatedly something is fundamentally wrong (bad install,
    /// missing firmware, etc.) — spamming retries just wastes cycles.
    const MAX_RESPAWNS: u32 = 3;

    tokio::spawn(async move {
        let mut ticker = tokio::time::interval(interval);
        // Skip the immediate first tick — `interval` fires once on start.
        ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        ticker.tick().await;
        let mut consecutive_failures: u32 = 0;
        loop {
            ticker.tick().await;

            let mut guard = rpcs3.lock().await;
            // Fire if we own a process and it's dead. Under 4.15.16
            // RPCS3 can be alive at library view (no current game),
            // so we check process.is_alive() regardless of `current`.
            let crashed = match guard.process.as_mut() {
                Some(proc) => !proc.is_alive(),
                None => false,
            };
            if !crashed {
                continue;
            }

            // Drop the dead handle so we never double-report.
            let _dead = guard.process.take();
            let game = guard.current.take();
            // Capture the EBOOT so auto-respawn can re-launch the same
            // game (PLAN 10.8.4 direct-boot — no library-view fallback).
            let crashed_eboot = guard.current_eboot.take();
            drop(guard);

            let had_game = game.is_some();
            let message = match game.as_ref() {
                Some(g) => format!("{} exited unexpectedly", g.display_name),
                None => "RPCS3 exited unexpectedly".into(),
            };
            warn!(message = %message, "detected RPCS3 crash");

            // Reset the portal snapshot — the emulator is gone, so any
            // previously-loaded slots are meaningless.
            *portal.lock().await = std::array::from_fn(|_| SlotState::Empty);
            let _ = events.send(Event::PortalSnapshot {
                slots: std::array::from_fn(|_| SlotState::Empty),
            });
            // Only surface the full crash overlay to phones + Crashed
            // screen on the TV when a GAME was running. A library-view
            // crash during auto-respawn is invisible to the user — the
            // cloud vortex covers it on the TV; phones just see a
            // transient `rpcs3_running = false` window.
            if had_game {
                if let Ok(mut st) = launcher_status.lock() {
                    st.rpcs3_running = false;
                    st.current_game = None;
                    st.screen = LauncherScreen::Crashed {
                        message: message.clone(),
                    };
                }
                let _ = events.send(Event::GameCrashed {
                    message: message.clone(),
                });
                let _ = events.send(Event::GameChanged { current: None });
            } else {
                if let Ok(mut st) = launcher_status.lock() {
                    st.rpcs3_running = false;
                    st.current_game = None;
                }
            }

            // Auto-respawn (PLAN 10.8.4 direct-boot): only if a game was
            // running and we know its EBOOT. Without an EBOOT we have
            // nothing useful to launch — RPCS3 with no args drops into
            // library view, but the launcher no longer drives that
            // surface, so the user would see a foreign window. Skip
            // respawn instead and leave the user on the Crashed screen
            // with RESTART (re-fires /api/launch) or RETURN TO GAMES.
            let Some(eboot) = crashed_eboot else {
                if had_game {
                    info!(
                        "RPCS3 crashed mid-game but no EBOOT recorded; \
                         leaving user on Crashed screen to manually restart"
                    );
                }
                continue;
            };
            // Small delay so OS cleanup (handle release, child process
            // teardown) doesn't collide with the new launch. 500ms
            // matches the watchdog tick and is empirically enough on
            // Windows 11.
            tokio::time::sleep(std::time::Duration::from_millis(500)).await;
            let exe = rpcs3_exe.clone();
            let eboot_for_blocking = eboot.clone();
            let respawn = tokio::task::spawn_blocking(
                move || -> anyhow::Result<skylander_rpcs3_control::RpcsProcess> {
                    let mut proc = skylander_rpcs3_control::RpcsProcess::launch_with_eboot(
                        &exe,
                        &eboot_for_blocking,
                    )?;
                    proc.wait_ready(std::time::Duration::from_secs(45))?;
                    Ok(proc)
                },
            )
            .await;
            match respawn {
                Ok(Ok(proc)) => {
                    let mut guard = rpcs3.lock().await;
                    guard.process = Some(proc);
                    guard.current = game.clone();
                    guard.current_eboot = Some(eboot.clone());
                    drop(guard);
                    if let Ok(mut st) = launcher_status.lock() {
                        st.rpcs3_running = true;
                        if let Some(g) = &game {
                            st.current_game = Some(g.display_name.clone());
                        }
                    }
                    consecutive_failures = 0;
                    info!("RPCS3 auto-respawn succeeded");
                }
                Ok(Err(e)) => {
                    consecutive_failures = consecutive_failures.saturating_add(1);
                    warn!(consecutive_failures, "RPCS3 auto-respawn failed: {e}");
                    if consecutive_failures >= MAX_RESPAWNS
                        && let Ok(mut st) = launcher_status.lock()
                    {
                        st.screen = LauncherScreen::ServerError {
                            message: format!(
                                "RPCS3 keeps crashing ({} attempts): {}",
                                consecutive_failures, e
                            ),
                        };
                    }
                }
                Err(e) => {
                    consecutive_failures = consecutive_failures.saturating_add(1);
                    warn!(
                        consecutive_failures,
                        "RPCS3 auto-respawn task panicked: {e}"
                    );
                }
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
            // PLAN 10.8.4: spawn RPCS3 with the picked game's EBOOT.BIN,
            // wait for the FPS: viewport, then open the Skylanders dialog.
            // The whole spawn-and-wait dance is Windows-only — on non-
            // Windows (Mac/Linux dev with the mock driver) we skip the
            // spawn but still update lifecycle state so /api/launch
            // round-trips cleanly. Mock-driver tests don't have a real
            // RPCS3 to talk to anyway.
            #[cfg(windows)]
            let result: Result<Option<RpcsProcess>> = {
                let d = driver.clone();
                let exe_owned = rpcs3_exe.to_path_buf();
                let config_dir_owned = config_dir.to_path_buf();
                let eboot_owned = eboot_path.clone();
                let display_name_for_blocking = display_name.clone();
                // Match the viewport title on `[<SERIAL>]` (e.g.
                // `[BLUS30968]`). Serial brackets are deterministic;
                // matching on display_name fails when RPCS3's
                // viewport title drops punctuation we put in
                // SKYLANDERS_SERIALS — `"Skylanders: Giants"` (catalogue)
                // vs `"Skylanders Giants [BLUS30968]"` (viewport).
                let expected_marker = format!("[{}]", serial);
                let _ = &expected_name;
                let status_for_blocking = launcher_status.clone();
                tokio::task::spawn_blocking(move || -> Result<Option<RpcsProcess>> {
                    // PLAN 16.6.1 — IPC path (patched RPCS3): launch --no-gui with the
                    // borderless window + IPC socket, then wait on the clean liveness
                    // signal (STATE: status=running + frames advancing) instead of
                    // scraping the FPS viewport title. No Skylanders Manager dialog, so
                    // no open_dialog. Falls through to the legacy UIA path otherwise.
                    if let Some(ipc_path) = d.ipc_socket_path() {
                        let mut proc = RpcsProcess::launch_no_gui(
                            &exe_owned,
                            &eboot_owned,
                            &ipc_path,
                            Some(&config_dir_owned),
                        )?;
                        let deadline = std::time::Instant::now() + timeout;
                        loop {
                            if !proc.is_alive() {
                                return Err(anyhow::anyhow!(
                                    "patched RPCS3 exited during no-GUI boot"
                                ));
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
                        // game_window_handle is published continuously by the STATE
                        // poller (early, as soon as the window exists) — not here.
                        if let Ok(mut st) = status_for_blocking.lock() {
                            st.rpcs3_running = true;
                            st.current_game = Some(display_name_for_blocking.clone());
                        }
                        return Ok(Some(proc));
                    }

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
                    // Flip rpcs3_running + current_game on launcher_status
                    // BEFORE open_dialog. The launcher's per-frame
                    // `push_rpcs3_main_to_bottom_via_win32` is gated on
                    // `rpcs3_running && current_game.is_some()`; without
                    // that gate satisfied first, open_dialog's UIA Invoke
                    // transiently promotes the main window over the game
                    // viewport and the launcher doesn't push it down,
                    // leaving the RPCS3 menu bar visible on top of the
                    // game (HTPC bug 2026-05-04). This `std::sync::Mutex`
                    // lock is fine from spawn_blocking — it's not the
                    // tokio Mutex.
                    if let Ok(mut st) = status_for_blocking.lock() {
                        st.rpcs3_running = true;
                        st.current_game = Some(display_name_for_blocking.clone());
                        // UIA fallback playable signal (PLAN 16.6.3.2): the loop above
                        // only breaks once the `FPS:` viewport with the serial marker
                        // is up — i.e. the game is rendering — so reveal it now. The
                        // retired FPS sampler used to derive this; in-band is simpler
                        // and the one signal the UIA path still needs.
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
            let result: Result<Option<RpcsProcess>> = {
                let _ = (driver, rpcs3_exe, config_dir, expected_name, timeout);
                // Mock driver: no real RPCS3, no spawn, no viewport poll.
                // Keep whatever process handle the lifecycle already has
                // (a `RpcsProcess::mock()` installed at startup).
                Ok(None)
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
