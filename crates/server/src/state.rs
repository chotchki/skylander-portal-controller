//! Shared state + driver job queue.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use anyhow::Result;
use skylander_core::{
    Event, Figure, FigureId, GameLaunched, GameSerial, SLOT_COUNT, SlotIndex, SlotState,
};
use skylander_rpcs3_control::{PortalDriver, RpcsProcess};
use tokio::sync::{Mutex, broadcast, mpsc};
use tracing::{error, info, warn};

use crate::games::InstalledGame;
use crate::profiles::{ProfileStore, SessionRegistry};

pub struct AppState {
    pub figures: Vec<Figure>,
    /// Map figure_id → index into `figures` for quick lookup.
    pub figure_index: HashMap<FigureId, usize>,
    pub driver_tx: mpsc::Sender<DriverJob>,
    pub portal: Arc<Mutex<[SlotState; SLOT_COUNT]>>,
    pub events: broadcast::Sender<Event>,
    pub connected_clients: Arc<std::sync::atomic::AtomicUsize>,

    /// Installed Skylanders games, loaded from RPCS3's games.yml at startup.
    pub games: Vec<InstalledGame>,
    pub rpcs3_exe: PathBuf,
    /// Root of the committed static-data bundle served at `/api/figures/:id/image`.
    /// Points at `<repo>/data/` in dev; populated at startup from config.
    pub data_root: PathBuf,
    /// 32-byte HMAC-SHA256 key shared with the phone via the TV's QR code.
    /// Used by the `Signed` extractor on mutating REST endpoints (PLAN 3.13).
    pub hmac_key: Vec<u8>,
    /// Lifecycle lock around the currently-running RPCS3 instance.
    pub rpcs3: Arc<Mutex<RpcsLifecycle>>,

    /// SQLite-backed profile store + argon2 PIN hashes + lockout map.
    pub profiles: ProfileStore,
    /// Per-connection session registry. Tracks which profile (if any) is
    /// unlocked for each WS session. 3.9 is single-session; 3.10 extends
    /// this to a 2-slot FIFO registry.
    pub sessions: Arc<SessionRegistry>,

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

impl AppState {
    pub fn lookup_game(&self, serial: &GameSerial) -> Option<&InstalledGame> {
        self.games.iter().find(|g| &g.serial == serial)
    }
}

impl AppState {
    pub fn lookup_figure(&self, id: &FigureId) -> Option<&Figure> {
        self.figure_index.get(id).and_then(|i| self.figures.get(*i))
    }
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
/// `/api/quit` takes the process out of the guard *before* calling
/// `shutdown_graceful`, so the watchdog naturally won't fire on clean
/// quits — by the time the process dies, `guard.process` is already `None`.
pub fn spawn_crash_watchdog(
    rpcs3: Arc<Mutex<RpcsLifecycle>>,
    portal: Arc<Mutex<[SlotState; SLOT_COUNT]>>,
    events: broadcast::Sender<Event>,
    interval: std::time::Duration,
) {
    tokio::spawn(async move {
        let mut ticker = tokio::time::interval(interval);
        // Skip the immediate first tick — `interval` fires once on start.
        ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        ticker.tick().await;
        loop {
            ticker.tick().await;

            let mut guard = rpcs3.lock().await;
            // Only act if we own a process and a game is marked current.
            let has_current = guard.current.is_some();
            let crashed = match guard.process.as_mut() {
                Some(proc) if has_current => !proc.is_alive(),
                _ => false,
            };
            if !crashed {
                continue;
            }

            // Drop the dead handle so we never double-report.
            let _dead = guard.process.take();
            let game = guard.current.take();
            drop(guard);

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
            let _ = events.send(Event::GameCrashed { message });
            let _ = events.send(Event::GameChanged { current: None });
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
}
