//! Shared state + driver job queue.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use anyhow::Result;
use skylander_core::{Event, Figure, FigureId, GameLaunched, GameSerial, SlotIndex, SlotState, SLOT_COUNT};
use skylander_rpcs3_control::{PortalDriver, RpcsProcess};
use tokio::sync::{broadcast, mpsc, Mutex};
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
) -> mpsc::Sender<DriverJob> {
    let (tx, mut rx) = mpsc::channel::<DriverJob>(32);

    tokio::spawn(async move {
        // Initial snapshot — best effort; a subsequent RefreshPortal will retry
        // if this fails (e.g. dialog not open yet).
        if let Err(e) = refresh(&driver, &portal, &events).await {
            info!("initial portal refresh failed (expected if dialog isn't open yet): {e}");
        }

        while let Some(job) = rx.recv().await {
            let mutation = matches!(
                &job,
                DriverJob::LoadFigure { .. } | DriverJob::ClearSlot { .. }
            );
            if let Err(e) = handle_job(job, &driver, &portal, &events).await {
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
                    restore_after_failure(driver, portal, events, slot, &e.to_string()).await;
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
                    restore_after_failure(driver, portal, events, slot, &e.to_string()).await;
                }
            }
        }
        DriverJob::RefreshPortal => {
            refresh(driver, portal, events).await?;
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
) {
    let _ = events.send(Event::Error {
        message: message.to_string(),
    });

    let d = driver.clone();
    let snapshot = tokio::task::spawn_blocking(move || d.read_slots()).await;

    let truth = match snapshot {
        Ok(Ok(snap)) => snap[slot.as_usize()].clone(),
        _ => SlotState::Empty,
    };
    set_and_broadcast(portal, events, slot, truth).await;
}

async fn refresh(
    driver: &Arc<dyn PortalDriver>,
    portal: &Arc<Mutex<[SlotState; SLOT_COUNT]>>,
    events: &broadcast::Sender<Event>,
) -> Result<()> {
    let d = driver.clone();
    let snap = tokio::task::spawn_blocking(move || -> Result<[SlotState; SLOT_COUNT]> {
        d.open_dialog()?;
        d.read_slots()
    })
    .await??;

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
