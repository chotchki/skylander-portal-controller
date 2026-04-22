//! Shared state + driver job queue.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::Result;
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
            let profile_id = self.sessions.profile_of(*sid).await;
            let pip = match profile_id {
                Some(pid) => match self.profiles.get(&pid).await {
                    Ok(Some(row)) => SessionPip {
                        color: Some(row.color),
                        initial: first_grapheme_uppercase(&row.display_name),
                    },
                    _ => SessionPip::default(),
                },
                None => SessionPip::default(),
            };
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
    /// Boot a game into the already-running RPCS3. Prereq: the `/api/launch`
    /// handler just spawned RPCS3 via `RpcsProcess::launch_library` and
    /// `wait_ready`'d it, so the library view is visible. The worker calls
    /// `driver.open_dialog()` (cold-library 3.6b-proven nav path) then
    /// `driver.boot_game_by_serial(...)`. Result is delivered via the
    /// oneshot so the handler can wait synchronously — the REST caller wants
    /// a success/failure response for the launch, not fire-and-forget.
    BootGame {
        serial: String,
        timeout: std::time::Duration,
        done: tokio::sync::oneshot::Sender<Result<()>>,
    },
    /// Walk RPCS3's library view and return every visible serial. PLAN
    /// 3.7.8 phase 1 — `/api/launch` calls this between `wait_ready` and
    /// `BootGame` so a request for a stale `games.yml` serial fails fast
    /// with a specific error instead of hitting `boot_game_by_serial`'s
    /// generic timeout.
    EnumerateGames {
        timeout: std::time::Duration,
        done: tokio::sync::oneshot::Sender<Result<Vec<String>>>,
    },
    /// Stop the current game and return RPCS3 to library view without
    /// killing the process. Replaces `/api/quit`'s old `shutdown_graceful`
    /// path under the always-running RPCS3 contract (PLAN 4.15.16). The
    /// worker calls `driver.stop_emulation(timeout)`; result comes back
    /// through the oneshot so the handler can sync on it.
    StopEmulation {
        timeout: std::time::Duration,
        done: tokio::sync::oneshot::Sender<Result<()>>,
    },
}

/// Spawn the RPCS3 shader-compile watchdog. Tails RPCS3's log file
/// (`RPCS3.log` next to the exe) and surfaces any line containing
/// shader-compile / cache-rebuild markers as
/// `LauncherStatus.shader_compile_text`. The window-title scan we
/// tried first didn't work for RPCS3 0.0.40 — that version surfaces
/// the state as an in-viewport overlay rendered into the OpenGL/
/// Vulkan surface, not in any window title — but the same state is
/// always written to the log as it happens, so tailing is the
/// reliable signal.
///
/// File is opened lazily (RPCS3.log doesn't exist until the first
/// time RPCS3 runs) and the read position is seeked to the end on
/// open so we only see new content. Truncation (RPCS3 rotated its
/// log) is detected by the file shrinking and we re-seek to 0.
pub fn spawn_shader_compile_watchdog(
    launcher_status: Arc<std::sync::Mutex<LauncherStatus>>,
    rpcs3_exe: PathBuf,
    interval: std::time::Duration,
) {
    // RPCS3 0.0.40 writes its log to `<exe_dir>/log/RPCS3.log`. Older
    // / portable installs may put it directly next to the exe. Try
    // the `log/` subdir first, then fall back to portable-style.
    let log_path = rpcs3_exe
        .parent()
        .map(|p| {
            let new_style = p.join("log").join("RPCS3.log");
            let portable = p.join("RPCS3.log");
            if new_style.exists() {
                new_style
            } else if portable.exists() {
                portable
            } else {
                // Default to new-style path; will be created when
                // RPCS3 first runs.
                new_style
            }
        })
        .unwrap_or_else(|| PathBuf::from("RPCS3.log"));
    info!(path = %log_path.display(), "tailing rpcs3 log for shader-compile progress");

    /// How long the watchdog waits after the last compile/cache line
    /// before declaring the game "playable" (assuming RPCS3 is also
    /// running). 2s strikes a balance — long enough that brief gaps
    /// between compile bursts don't trigger the close prematurely,
    /// short enough that the user doesn't wait noticeably after the
    /// last shader finishes.
    const PLAYABLE_QUIET_SECS: f32 = 2.0;

    tokio::spawn(async move {
        let mut ticker = tokio::time::interval(interval);
        ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        ticker.tick().await;

        let mut file: Option<std::fs::File> = None;
        let mut pos: u64 = 0;
        let mut carry = String::new();
        let mut last_text: Option<String> = None;
        // Time of the most recent compile/cache log hit. Used for the
        // playable-detection heuristic: rpcs3_running + no hit in
        // PLAYABLE_QUIET_SECS = game is actually ready.
        let mut last_compile_at: Option<std::time::Instant> = None;

        loop {
            ticker.tick().await;

            let new_compile_text = read_new_compile_text(&log_path, &mut file, &mut pos, &mut carry);

            if let Some(text) = new_compile_text {
                last_compile_at = Some(std::time::Instant::now());
                if Some(&text) != last_text.as_ref() {
                    info!(text = %text, "rpcs3 compile/cache progress detected");
                    last_text = Some(text.clone());
                }
            }

            // Compute playability: rpcs3_running AND quiet on compile
            // for PLAYABLE_QUIET_SECS. If we've never seen a compile
            // line yet (e.g. fully cached on second launch), treat
            // last_compile_at = None as "instantly playable once
            // rpcs3_running is true".
            let now = std::time::Instant::now();
            let quiet = last_compile_at
                .map(|t| now.duration_since(t).as_secs_f32() >= PLAYABLE_QUIET_SECS)
                .unwrap_or(true);

            if let Ok(mut st) = launcher_status.lock() {
                if last_text.is_some() && st.shader_compile_text != last_text {
                    st.shader_compile_text = last_text.clone();
                }
                let playable = st.rpcs3_running && quiet;
                if st.game_playable != playable {
                    if playable {
                        info!("rpcs3 game playable (compile activity quiet)");
                    } else if st.game_playable {
                        info!("rpcs3 game no longer playable (compile activity resumed)");
                    }
                    st.game_playable = playable;
                }
            }
        }
    });
}

/// Read any new bytes appended to RPCS3's log file since the last
/// call, scan the new lines for compile/cache progress markers, and
/// return the cleaned text of the most recent matching line (or
/// `None` if nothing matched).
///
/// `file` / `pos` / `carry` carry state across calls: open-once file
/// handle, last-read byte offset, and any partial trailing line
/// (RPCS3 writes line-by-line but we may catch it mid-flush).
fn read_new_compile_text(
    log_path: &Path,
    file: &mut Option<std::fs::File>,
    pos: &mut u64,
    carry: &mut String,
) -> Option<String> {
    use std::io::{Read, Seek, SeekFrom};

    if file.is_none() {
        let f = std::fs::File::open(log_path).ok()?;
        let len = f.metadata().ok()?.len();
        *file = Some(f);
        *pos = len; // seek to end on first open — only care about NEW content
    }
    let f = file.as_mut()?;

    // Detect truncation / rotation — file shrunk under us.
    let len = f.metadata().ok().map(|m| m.len()).unwrap_or(*pos);
    if len < *pos {
        *pos = 0;
        carry.clear();
    }

    f.seek(SeekFrom::Start(*pos)).ok()?;
    let mut chunk = String::new();
    let n = f.read_to_string(&mut chunk).ok()?;
    *pos += n as u64;
    if n == 0 {
        return None;
    }

    let mut buf = String::new();
    buf.push_str(carry);
    buf.push_str(&chunk);

    // Split on '\n'; the last segment is "partial" (no trailing '\n')
    // unless `buf` ends with '\n'. Stash it in carry for the next call.
    let mut latest_match: Option<String> = None;
    let mut iter = buf.split('\n').peekable();
    let mut last_partial = String::new();
    while let Some(line) = iter.next() {
        if iter.peek().is_none() {
            last_partial = line.to_string();
            break;
        }
        if let Some(stage) = classify_log_line(line) {
            latest_match = Some(stage.to_string());
        }
    }
    *carry = last_partial;
    latest_match
}

/// Classify an RPCS3 log line into a user-facing loading stage, or
/// `None` if it's not compile-related. Returns categorical strings
/// (e.g. "Building SPU cache") rather than raw log content — at the
/// 10-foot TV scale, the per-line counts don't read; the user just
/// needs to know which phase is in progress.
///
/// Lines RPCS3 0.0.40 emits during boot (verified from log dump):
///   - `SPU: Building function 0x...`              → SPU cache
///   - `PPU: Block 0x... will be compiled ...`     → PPU cache
///   - `PPU: ... instructions will be compiled ...` → PPU cache (summary)
///   - `RSX: ...` with `compil` / `shader` / `pipeline` → shaders
///
/// Filters out `ppu_loader: ****` lines (lots of `cellHttp*Pipeline`
/// API listings that match "pipeline" but aren't compile activity).
fn classify_log_line(line: &str) -> Option<&'static str> {
    if line.contains("ppu_loader: ****") {
        return None;
    }
    let low = line.to_ascii_lowercase();
    if low.contains("spu: building") || low.contains("spu cache:") {
        Some("Building SPU cache")
    } else if low.contains("ppu: block") || (low.contains("ppu:") && low.contains("compiled"))
    {
        Some("Building PPU cache")
    } else if low.contains("rsx:") && (low.contains("compil") || low.contains("shader"))
    {
        Some("Compiling shaders")
    } else {
        None
    }
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

            // Auto-respawn. Small delay so OS cleanup (handle release,
            // child process teardown) doesn't collide with the new
            // launch. 500ms matches the watchdog tick and is
            // empirically enough on Windows 11.
            tokio::time::sleep(std::time::Duration::from_millis(500)).await;
            let exe = rpcs3_exe.clone();
            let respawn = tokio::task::spawn_blocking(
                move || -> anyhow::Result<skylander_rpcs3_control::RpcsProcess> {
                    let mut proc = skylander_rpcs3_control::RpcsProcess::launch_library(&exe)?;
                    proc.wait_ready(std::time::Duration::from_secs(45))?;
                    Ok(proc)
                },
            )
            .await;
            match respawn {
                Ok(Ok(proc)) => {
                    let mut guard = rpcs3.lock().await;
                    guard.process = Some(proc);
                    drop(guard);
                    if let Ok(mut st) = launcher_status.lock() {
                        st.rpcs3_running = true;
                        // Leave `.screen` in Crashed if it was a
                        // game-crash — the user taps RESTART or
                        // RETURN TO GAMES to dismiss it. A library-
                        // view respawn silently returns to Main.
                    }
                    consecutive_failures = 0;
                    info!("RPCS3 auto-respawn succeeded");
                }
                Ok(Err(e)) => {
                    consecutive_failures = consecutive_failures.saturating_add(1);
                    warn!(
                        consecutive_failures,
                        "RPCS3 auto-respawn failed: {e}"
                    );
                    if consecutive_failures >= MAX_RESPAWNS {
                        if let Ok(mut st) = launcher_status.lock() {
                            st.screen = LauncherScreen::ServerError {
                                message: format!(
                                    "RPCS3 keeps crashing ({} attempts): {}",
                                    consecutive_failures, e
                                ),
                            };
                        }
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
pub fn spawn_driver_worker(
    driver: Arc<dyn PortalDriver>,
    portal: Arc<Mutex<[SlotState; SLOT_COUNT]>>,
    events: broadcast::Sender<Event>,
    profiles: crate::profiles::ProfileStore,
    sessions: Arc<crate::profiles::SessionRegistry>,
    figures: Arc<Vec<Figure>>,
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
            if let Err(e) = handle_job(job, &driver, &portal, &events, &figures).await {
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

async fn handle_job(
    job: DriverJob,
    driver: &Arc<dyn PortalDriver>,
    portal: &Arc<Mutex<[SlotState; SLOT_COUNT]>>,
    events: &broadcast::Sender<Event>,
    figures: &[Figure],
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
        DriverJob::BootGame {
            serial,
            timeout,
            done,
        } => {
            let d = driver.clone();
            let serial_for_blocking = serial.clone();
            // Dialog first, game second — same order as the 3.7.x live tests.
            // Cold library view is the easiest UIA case; once a game is
            // running, Qt's focus state is too scrambled to re-open the
            // Manage menu reliably.
            let result = tokio::task::spawn_blocking(move || -> Result<()> {
                d.open_dialog()?;
                d.boot_game_by_serial(&serial_for_blocking, timeout)
            })
            .await
            .map_err(|e| anyhow::anyhow!("boot task panicked: {e}"))
            .and_then(|r| r);
            // If the receiver dropped (handler timed out or errored),
            // silently ignore — the worker's contract is fulfilled by
            // having driven the driver; nobody is listening for the ack.
            let _ = done.send(result);
        }
        DriverJob::EnumerateGames { timeout, done } => {
            let d = driver.clone();
            let result = tokio::task::spawn_blocking(move || d.enumerate_games(timeout))
                .await
                .map_err(|e| anyhow::anyhow!("enumerate task panicked: {e}"))
                .and_then(|r| r);
            let _ = done.send(result);
        }
        DriverJob::StopEmulation { timeout, done } => {
            let d = driver.clone();
            let result = tokio::task::spawn_blocking(move || d.stop_emulation(timeout))
                .await
                .map_err(|e| anyhow::anyhow!("stop_emulation task panicked: {e}"))
                .and_then(|r| r);
            let _ = done.send(result);
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
            sky_path: PathBuf::from("/dev/null"),
            element_icon_path: None,
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

    // RPCS3-log compile classification (PLAN 4.18.x). Locks down the
    // keyword-set bug where the watcher missed compile activity because
    // RPCS3 0.0.40 emits "Building" / "Block ... compiled", not the
    // older "Compiling" wording the first implementation looked for.
    // Kids would otherwise see the QR card during multi-minute first-run
    // shader compiles instead of the LOADING badge with stage subtitle.

    #[test]
    fn classify_spu_building_function() {
        assert_eq!(
            classify_log_line("S 0:00:01.234 SPU: Building function 0xdeadbeef"),
            Some("Building SPU cache"),
        );
    }

    #[test]
    fn classify_ppu_block_will_be_compiled() {
        assert_eq!(
            classify_log_line("S 0:00:02.000 PPU: Block 0x100 will be compiled..."),
            Some("Building PPU cache"),
        );
    }

    #[test]
    fn classify_ppu_instructions_compiled_summary() {
        // The "summary" form: "PPU: 1234 instructions will be compiled".
        // Hits the `compiled` branch via the catch-all PPU/compiled match.
        assert_eq!(
            classify_log_line("S 0:00:03.000 PPU: Block 0x... 1234 instructions compiled"),
            Some("Building PPU cache"),
        );
    }

    #[test]
    fn classify_rsx_shader_compile() {
        assert_eq!(
            classify_log_line("S 0:00:04.000 RSX: Compiling pipeline state for shader 0xabc"),
            Some("Compiling shaders"),
        );
        assert_eq!(
            classify_log_line("S 0:00:04.000 RSX: shader 0xabc cached"),
            Some("Compiling shaders"),
        );
    }

    #[test]
    fn classify_filters_ppu_loader_pipeline_noise() {
        // ppu_loader logs `**** cellHttpClientPipelineRedirectMethod` etc.
        // at boot — they match "pipeline" but aren't compile activity.
        // Pre-filter must drop them before the RSX/PPU branches see them.
        assert_eq!(
            classify_log_line(
                "S 0:00:00.001 ppu_loader: **** cellHttpClientPipelineRedirectMethod"
            ),
            None,
        );
    }

    #[test]
    fn classify_returns_none_for_unrelated_lines() {
        assert_eq!(classify_log_line(""), None);
        assert_eq!(
            classify_log_line("S 0:00:00.000 sys_fs: open /dev_hdd0/game/..."),
            None,
        );
        assert_eq!(
            classify_log_line("· 0:00:00.000 RSX: Frame 0 rendered"),
            None,
        );
    }

    #[test]
    fn classify_is_case_insensitive() {
        // RPCS3 capitalisation is consistent today, but the implementation
        // lowercases first to be tolerant of future casing drift.
        assert_eq!(
            classify_log_line("spu: building function 0x0"),
            Some("Building SPU cache"),
        );
        assert_eq!(
            classify_log_line("RSX: SHADER cached"),
            Some("Compiling shaders"),
        );
    }
}
