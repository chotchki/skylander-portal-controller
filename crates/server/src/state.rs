//! Shared state + driver job queue.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use anyhow::Result;
use skylander_core::{Event, Figure, FigureId, SlotIndex, SlotState, SLOT_COUNT};
use skylander_rpcs3_control::PortalDriver;
use tokio::sync::{broadcast, mpsc, Mutex};
use tracing::{error, info};

pub struct AppState {
    pub figures: Vec<Figure>,
    /// Map figure_id → index into `figures` for quick lookup.
    pub figure_index: HashMap<FigureId, usize>,
    pub driver_tx: mpsc::Sender<DriverJob>,
    pub portal: Arc<Mutex<[SlotState; SLOT_COUNT]>>,
    pub events: broadcast::Sender<Event>,
    pub connected_clients: Arc<std::sync::atomic::AtomicUsize>,
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
    },
    ClearSlot {
        slot: SlotIndex,
    },
    RefreshPortal,
}

/// Spawn the driver worker. Owns the `PortalDriver` and serialises all access.
pub fn spawn_driver_worker(
    driver: Arc<dyn PortalDriver>,
    portal: Arc<Mutex<[SlotState; SLOT_COUNT]>>,
    events: broadcast::Sender<Event>,
) -> mpsc::Sender<DriverJob> {
    let (tx, mut rx) = mpsc::channel::<DriverJob>(32);

    tokio::spawn(async move {
        // Initial snapshot — best effort; a subsequent RefreshPortal will retry
        // if this fails (e.g. dialog not open yet).
        if let Err(e) = refresh(&driver, &portal, &events).await {
            info!("initial portal refresh failed (expected if dialog isn't open yet): {e}");
        }

        while let Some(job) = rx.recv().await {
            if let Err(e) = handle_job(job, &driver, &portal, &events).await {
                error!("driver job error: {e}");
                let _ = events.send(Event::Error {
                    message: e.to_string(),
                });
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
        } => {
            set_and_broadcast(
                portal,
                events,
                slot,
                SlotState::Loading {
                    figure_id: Some(figure_id.clone()),
                },
            )
            .await;

            let d = driver.clone();
            let fid = figure_id.clone();
            let result = tokio::task::spawn_blocking(move || -> Result<String> {
                d.open_dialog()?;
                d.load(slot, &path)
            })
            .await?;

            let new_state = match result {
                Ok(display_name) => SlotState::Loaded {
                    figure_id: Some(fid),
                    display_name,
                },
                Err(e) => SlotState::Error {
                    message: e.to_string(),
                },
            };
            set_and_broadcast(portal, events, slot, new_state).await;
        }
        DriverJob::ClearSlot { slot } => {
            set_and_broadcast(portal, events, slot, SlotState::Loading { figure_id: None }).await;
            let d = driver.clone();
            let result = tokio::task::spawn_blocking(move || -> Result<()> {
                d.open_dialog()?;
                d.clear(slot)
            })
            .await?;
            let new_state = match result {
                Ok(()) => SlotState::Empty,
                Err(e) => SlotState::Error {
                    message: e.to_string(),
                },
            };
            set_and_broadcast(portal, events, slot, new_state).await;
        }
        DriverJob::RefreshPortal => {
            refresh(driver, portal, events).await?;
        }
    }
    Ok(())
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
