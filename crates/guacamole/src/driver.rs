//! The native portal driver (AA design decision 4): places by identity, keeps
//! its own slots for `read_slots`, and tells the TV over a bounded channel.
//! Placing the figure a slot already holds for the same owner (a resumed
//! layout still on the portal, AB.21) tells the TV nothing, so no intro
//! replays; the product's RPCS3 drivers still reload it.

use std::collections::VecDeque;
use std::path::Path;
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};

use anyhow::Result;
use crossbeam_channel::{Receiver, Sender, TrySendError};
use skylander_core::{FigureId, SLOT_COUNT, SlotIndex, SlotState};
use skylander_rpcs3_control::{InjectedOutcome, LoadInjector, Placement, PortalDriver};
use tracing::{info, warn};

/// Room for a burst of changes; the TV drains it every frame.
pub const PORTAL_CHANNEL_CAPACITY: usize = 64;

/// What the TV hears from the driver.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PortalChange {
    Placed {
        slot: SlotIndex,
        placement: Placement,
    },
    Cleared {
        slot: SlotIndex,
    },
}

/// The driver's sender and the TV's receiver.
pub fn portal_channel() -> (Sender<PortalChange>, Receiver<PortalChange>) {
    crossbeam_channel::bounded(PORTAL_CHANNEL_CAPACITY)
}

/// The portal as the driver holds it.
struct Slots {
    held: [SlotState; SLOT_COUNT],
    /// The TV got the slot's last change. A dropped one leaves it false, so
    /// placing the same figure again repairs the card instead of skipping.
    shown: [bool; SLOT_COUNT],
}

impl Slots {
    /// The slot holds `figure_id` placed by `owner` and the TV shows it.
    fn shows(&self, slot: SlotIndex, figure_id: &FigureId, owner: Option<&str>) -> bool {
        let i = slot.as_usize();
        self.shown[i]
            && matches!(
                &self.held[i],
                SlotState::Loaded { figure_id: Some(held), placed_by, .. }
                    if held == figure_id && placed_by.as_deref() == owner
            )
    }
}

pub struct NativePortalDriver {
    slots: Mutex<Slots>,
    tv: Sender<PortalChange>,
    /// Outcomes from `/api/_test/inject_load`, consumed by `place`.
    injected: Mutex<VecDeque<InjectedOutcome>>,
    dropped: AtomicU64,
}

impl NativePortalDriver {
    pub fn new(tv: Sender<PortalChange>) -> Self {
        Self {
            slots: Mutex::new(Slots {
                held: std::array::from_fn(|_| SlotState::Empty),
                shown: [true; SLOT_COUNT],
            }),
            tv,
            injected: Mutex::new(VecDeque::new()),
            dropped: AtomicU64::new(0),
        }
    }

    /// Changes the TV never got because the channel was full or closed.
    #[cfg(test)]
    pub fn dropped(&self) -> u64 {
        self.dropped.load(Ordering::Relaxed)
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Slots> {
        self.slots.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Never blocks: a full channel costs a stale card, not a hung load.
    /// False when the TV didn't get it.
    fn tell_tv(&self, change: PortalChange) -> bool {
        let why = match self.tv.try_send(change) {
            Ok(()) => return true,
            Err(TrySendError::Full(_)) => "full",
            Err(TrySendError::Disconnected(_)) => "closed",
        };
        let dropped = self.dropped.fetch_add(1, Ordering::Relaxed) + 1;
        warn!(
            dropped,
            "the TV's portal channel is {why}; dropped a change"
        );
        false
    }
}

impl LoadInjector for NativePortalDriver {
    fn queue_load_outcomes(&self, outcomes: Vec<InjectedOutcome>) {
        self.injected
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .extend(outcomes);
    }
}

impl PortalDriver for NativePortalDriver {
    fn open_dialog(&self) -> Result<()> {
        Ok(())
    }

    fn read_slots(&self) -> Result<[SlotState; SLOT_COUNT]> {
        Ok(self.lock().held.clone())
    }

    /// Always an error: under Native a file path means a missed arm.
    fn load(&self, _slot: SlotIndex, path: &Path) -> Result<String> {
        anyhow::bail!(
            "the native driver places by identity, not a file ({})",
            path.display()
        )
    }

    fn clear(&self, slot: SlotIndex) -> Result<()> {
        let mut slots = self.lock();
        slots.held[slot.as_usize()] = SlotState::Empty;
        slots.shown[slot.as_usize()] = self.tell_tv(PortalChange::Cleared { slot });
        Ok(())
    }

    fn place(&self, slot: SlotIndex, placement: &Placement) -> Result<()> {
        let injected = self
            .injected
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .pop_front();
        if let Some(outcome) = injected {
            outcome.into_result()?;
        }
        let owner = placement.owner.as_ref().map(|o| o.profile_id.clone());
        let mut slots = self.lock();
        if slots.shows(slot, &placement.figure_id, owner.as_deref()) {
            info!(
                slot = slot.display(),
                figure = %placement.figure_id.as_str(),
                "already on the portal for the same owner; nothing to place"
            );
            return Ok(());
        }
        slots.held[slot.as_usize()] = SlotState::Loaded {
            figure_id: Some(placement.figure_id.clone()),
            display_name: placement.name.clone(),
            placed_by: owner,
        };
        slots.shown[slot.as_usize()] = self.tell_tv(PortalChange::Placed {
            slot,
            placement: placement.clone(),
        });
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;
    use std::sync::Arc;
    use std::time::{Duration, Instant};

    use guac_server::profiles::ProfileStore;
    use guac_server::state::{DriverJob, LauncherStatus, RpcsLifecycle, spawn_driver_worker};
    use skylander_core::{
        Category, Element, Event, Figure, FigureId, GameOfOrigin, SubtypeWord, ToyTypeId,
    };
    use skylander_rpcs3_control::PlacementOwner;
    use tokio::sync::{Mutex as AsyncMutex, broadcast, mpsc};

    use super::*;

    const ERUPTOR: &str = "000009-0000";
    /// The disc's id: bit 10, which the product's mask drops, kept (AE.4).
    const LEGENDARY_IGNITOR: &str = "00000a-0403";

    fn slot(n: u8) -> SlotIndex {
        SlotIndex::new(n).unwrap()
    }

    fn record(id: &str, name: &str, element: Element) -> Figure {
        Figure {
            id: FigureId::new(id),
            canonical_name: name.into(),
            variant_group: name.into(),
            variant_tag: "base".into(),
            game: GameOfOrigin::Giants,
            element: Some(element),
            category: Category::Figure,
            vehicle_terrain: None,
            sky_path: PathBuf::new(),
            element_icon_path: None,
            tag_identity: None,
        }
    }

    fn eruptor(owner: Option<&str>) -> Placement {
        let figure_id = FigureId::new(ERUPTOR);
        let (toy_type, variant) = figure_id.subtype_parts().unwrap();
        Placement {
            figure_id,
            toy_type,
            variant,
            name: "Eruptor".into(),
            element: Some(Element::Fire),
            portrait: None,
            owner: owner.map(|id| PlacementOwner {
                profile_id: id.into(),
                initial: "A".into(),
                color: "#ff0000".into(),
            }),
        }
    }

    /// The server's worker over a native driver, as guacamole boots it.
    struct Worker {
        jobs: mpsc::Sender<DriverJob>,
        profiles: ProfileStore,
        tv: Receiver<PortalChange>,
        driver: Arc<NativePortalDriver>,
    }

    async fn worker() -> Worker {
        let (tx, tv) = portal_channel();
        let driver = Arc::new(NativePortalDriver::new(tx));
        let profiles = ProfileStore::open_in_memory(&guac_server::setup::MIGRATOR)
            .await
            .unwrap();
        let figures =
            guac_server::figures::FigureList::new(guac_server::figures::FigureSet::pack(vec![
                record(ERUPTOR, "Eruptor", Element::Fire),
                record(LEGENDARY_IGNITOR, "Legendary Ignitor", Element::Fire),
            ]));
        let (events, _) = broadcast::channel::<Event>(64);
        let jobs = spawn_driver_worker(
            driver.clone(),
            Arc::new(AsyncMutex::new(std::array::from_fn(|_| SlotState::Empty))),
            events,
            profiles.clone(),
            Arc::default(),
            figures,
            Arc::new(AsyncMutex::new(RpcsLifecycle::default())),
            PathBuf::new(),
            PathBuf::new(),
            Arc::new(std::sync::Mutex::new(LauncherStatus::default())),
            None,
        );
        Worker {
            jobs,
            profiles,
            tv,
            driver,
        }
    }

    impl Worker {
        async fn place(&self, n: u8, id: &str, name: &str, placed_by: Option<&str>) {
            self.jobs
                .send(DriverJob::LoadFigure {
                    slot: slot(n),
                    figure_id: FigureId::new(id),
                    path: None,
                    placed_by: placed_by.map(str::to_string),
                    canonical_name: name.into(),
                })
                .await
                .unwrap();
        }

        /// The next change the TV hears, waiting up to 5 s.
        async fn next(&self) -> PortalChange {
            let tv = self.tv.clone();
            tokio::task::spawn_blocking(move || tv.recv_timeout(Duration::from_secs(5)))
                .await
                .unwrap()
                .expect("the TV heard nothing")
        }
    }

    fn placed(change: PortalChange) -> (SlotIndex, Placement) {
        match change {
            PortalChange::Placed { slot, placement } => (slot, placement),
            other => panic!("expected Placed, got {other:?}"),
        }
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn a_placement_carries_the_parsed_id_and_the_owners_initial_and_colour() {
        let w = worker().await;
        let kaylee = w
            .profiles
            .create("kaylee", "1234", "#e63946")
            .await
            .unwrap();
        w.place(0, LEGENDARY_IGNITOR, "Legendary Ignitor", Some(&kaylee))
            .await;

        let (at, p) = placed(w.next().await);
        assert_eq!(at, slot(0));
        assert_eq!(p.figure_id.as_str(), LEGENDARY_IGNITOR);
        assert_eq!(p.toy_type, ToyTypeId(0x0a));
        assert_eq!(p.variant, SubtypeWord(0x0403));
        assert_eq!(p.name, "Legendary Ignitor");
        assert_eq!(p.element, Some(Element::Fire));
        let owner = p.owner.expect("an unlocked profile placed it");
        assert_eq!(owner.profile_id, kaylee);
        assert_eq!(owner.initial, "K");
        assert_eq!(owner.color, "#e63946");
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn the_same_figure_by_two_profiles_is_two_placements_each_with_its_owner() {
        let w = worker().await;
        let kaylee = w
            .profiles
            .create("Kaylee", "1234", "#e63946")
            .await
            .unwrap();
        let sam = w.profiles.create("Sam", "4321", "#2a9d8f").await.unwrap();
        w.place(0, ERUPTOR, "Eruptor", Some(&kaylee)).await;
        let first = placed(w.next().await);
        w.place(1, ERUPTOR, "Eruptor", Some(&sam)).await;
        let second = placed(w.next().await);

        assert_eq!(first.0, slot(0));
        assert_eq!(second.0, slot(1));
        assert_eq!(first.1.figure_id, second.1.figure_id);
        let owner = |p: &Placement| {
            let o = p.owner.clone().unwrap();
            (o.profile_id, o.initial, o.color)
        };
        assert_eq!(owner(&first.1), (kaylee, "K".into(), "#e63946".into()));
        assert_eq!(owner(&second.1), (sam, "S".into(), "#2a9d8f".into()));
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn a_placement_with_no_profile_unlocked_has_no_owner() {
        let w = worker().await;
        w.place(4, ERUPTOR, "Eruptor", None).await;
        let (at, p) = placed(w.next().await);
        assert_eq!(at, slot(4));
        assert_eq!(p.owner, None);
        match &w.driver.read_slots().unwrap()[4] {
            SlotState::Loaded { placed_by, .. } => assert_eq!(placed_by, &None),
            other => panic!("expected Loaded, got {other:?}"),
        }
    }

    /// Quit under Native: the driver's slots and the TV's cards both empty.
    #[tokio::test(flavor = "multi_thread")]
    async fn a_reset_clears_the_tv_and_the_drivers_slots() {
        let w = worker().await;
        w.place(0, ERUPTOR, "Eruptor", None).await;
        placed(w.next().await);

        w.jobs.send(DriverJob::ResetPortal).await.unwrap();
        assert_eq!(w.next().await, PortalChange::Cleared { slot: slot(0) });
        assert!(
            w.driver
                .read_slots()
                .unwrap()
                .iter()
                .all(|s| matches!(s, SlotState::Empty))
        );
        assert!(w.tv.try_recv().is_err(), "an empty slot was cleared too");
    }

    /// AB.21: a resume re-sends the layout through the worker; what the
    /// portal already holds reaches the TV once, the next new one in order.
    #[tokio::test(flavor = "multi_thread")]
    async fn a_resumed_figure_already_on_the_portal_reaches_the_tv_once() {
        let w = worker().await;
        let kaylee = w
            .profiles
            .create("Kaylee", "1234", "#e63946")
            .await
            .unwrap();
        w.place(0, ERUPTOR, "Eruptor", Some(&kaylee)).await;
        assert_eq!(placed(w.next().await).0, slot(0));
        // The resume: Eruptor again, then Legendary Ignitor new in slot 2.
        w.place(0, ERUPTOR, "Eruptor", Some(&kaylee)).await;
        w.place(1, LEGENDARY_IGNITOR, "Legendary Ignitor", Some(&kaylee))
            .await;
        let (at, p) = placed(w.next().await);
        assert_eq!((at, p.figure_id.as_str()), (slot(1), LEGENDARY_IGNITOR));
        assert!(w.tv.try_recv().is_err());
        match &w.driver.read_slots().unwrap()[0] {
            SlotState::Loaded { placed_by, .. } => assert_eq!(placed_by, &Some(kaylee)),
            other => panic!("expected Loaded, got {other:?}"),
        }
    }

    #[test]
    fn placing_what_a_slot_holds_for_the_same_owner_tells_the_tv_nothing() {
        let (tx, tv) = portal_channel();
        let d = NativePortalDriver::new(tx);
        d.place(slot(0), &eruptor(Some("p1"))).unwrap();
        assert!(matches!(tv.try_recv(), Ok(PortalChange::Placed { .. })));
        let before = d.read_slots().unwrap();
        d.place(slot(0), &eruptor(Some("p1"))).unwrap();
        assert!(tv.try_recv().is_err(), "the TV heard it twice");
        assert_eq!(d.read_slots().unwrap(), before);

        // Anything else is a placement: another owner, no owner, another slot.
        for (n, owner) in [(0, Some("p2")), (0, None), (1, Some("p2"))] {
            d.place(slot(n), &eruptor(owner)).unwrap();
            let (at, p) = placed(tv.try_recv().unwrap());
            assert_eq!(at, slot(n));
            assert_eq!(p.owner.map(|o| o.profile_id).as_deref(), owner);
        }
        // Lifted and put back: a new placement.
        d.clear(slot(1)).unwrap();
        d.place(slot(1), &eruptor(Some("p2"))).unwrap();
        assert_eq!(tv.try_recv(), Ok(PortalChange::Cleared { slot: slot(1) }));
        assert!(matches!(tv.try_recv(), Ok(PortalChange::Placed { .. })));
    }

    #[test]
    fn a_placement_the_tv_missed_is_sent_again() {
        let (tx, tv) = crossbeam_channel::bounded(1);
        let d = NativePortalDriver::new(tx);
        d.clear(slot(7)).unwrap();
        d.place(slot(0), &eruptor(Some("p1"))).unwrap();
        assert_eq!(d.dropped(), 1);
        assert_eq!(tv.try_recv(), Ok(PortalChange::Cleared { slot: slot(7) }));
        d.place(slot(0), &eruptor(Some("p1"))).unwrap();
        assert!(matches!(tv.try_recv(), Ok(PortalChange::Placed { .. })));
        // Now the TV has it, so a third is nothing.
        d.place(slot(0), &eruptor(Some("p1"))).unwrap();
        assert!(tv.try_recv().is_err());
    }

    #[test]
    fn clear_sends_cleared() {
        let (tx, tv) = portal_channel();
        let d = NativePortalDriver::new(tx);
        d.clear(slot(5)).unwrap();
        assert_eq!(tv.try_recv(), Ok(PortalChange::Cleared { slot: slot(5) }));
    }

    #[test]
    fn read_slots_mirrors_places_and_clears() {
        let (tx, _tv) = portal_channel();
        let d = NativePortalDriver::new(tx);
        d.place(slot(2), &eruptor(Some("p1"))).unwrap();
        match &d.read_slots().unwrap()[2] {
            SlotState::Loaded {
                figure_id,
                display_name,
                placed_by,
            } => {
                assert_eq!(figure_id.as_ref().unwrap().as_str(), ERUPTOR);
                assert_eq!(display_name, "Eruptor");
                assert_eq!(placed_by.as_deref(), Some("p1"));
            }
            other => panic!("expected Loaded, got {other:?}"),
        }
        d.clear(slot(2)).unwrap();
        assert!(matches!(d.read_slots().unwrap()[2], SlotState::Empty));
    }

    #[test]
    fn an_injected_failure_is_an_error_and_sends_nothing() {
        let (tx, tv) = portal_channel();
        let d = NativePortalDriver::new(tx);
        d.queue_load_outcomes(vec![InjectedOutcome::QtModal {
            message: "Failed to open the skylander file!".into(),
        }]);
        let err = d.place(slot(0), &eruptor(Some("p1"))).unwrap_err();
        assert!(err.to_string().contains("Failed to open"), "{err}");
        assert!(tv.try_recv().is_err(), "the TV heard a failed placement");
        assert!(matches!(d.read_slots().unwrap()[0], SlotState::Empty));

        // Consumed: the next placement goes through.
        d.place(slot(0), &eruptor(Some("p1"))).unwrap();
        assert!(matches!(tv.try_recv(), Ok(PortalChange::Placed { .. })));
    }

    #[test]
    fn a_full_channel_drops_and_counts_without_blocking_place() {
        let (tx, tv) = crossbeam_channel::bounded(1);
        let d = NativePortalDriver::new(tx);
        d.clear(slot(7)).unwrap();

        let started = Instant::now();
        d.place(slot(0), &eruptor(None)).unwrap();
        assert!(started.elapsed() < Duration::from_secs(1), "place blocked");
        assert_eq!(d.dropped(), 1);
        assert!(
            matches!(d.read_slots().unwrap()[0], SlotState::Loaded { .. }),
            "the slot still holds the placement"
        );
        assert_eq!(tv.try_recv(), Ok(PortalChange::Cleared { slot: slot(7) }));
        assert!(tv.try_recv().is_err(), "the dropped Placed arrived");
    }

    #[test]
    fn load_with_a_path_errors_and_sends_nothing() {
        let (tx, tv) = portal_channel();
        let d = NativePortalDriver::new(tx);
        assert!(d.load(slot(0), Path::new("/pack/Eruptor.sky")).is_err());
        assert!(matches!(d.read_slots().unwrap()[0], SlotState::Empty));
        assert!(tv.try_recv().is_err());
    }
}
