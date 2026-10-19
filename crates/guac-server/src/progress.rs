//! The TV-to-server path for a figure's progress (openspec AH.1, design
//! decision 8). The TV hands over absolute values, never changes: per
//! (profile, figure) the latest experience, gold and playtime with the
//! generation they were earned under, in a latest-wins map the server thread
//! drains, so a burst can't overflow it and nothing queues behind a slow write.
//!
//! The drainer writes experience and gold at once and playtime every
//! [`PLAYTIME_WRITE_SECS`] of play. An entry goes in whole when the TV says the
//! figure left the portal, when the level ends and when guacamole quits.
//! Values under a generation older than the row's are dropped: an edit or a
//! reset came since. `FigureUpdated` goes out when the level or the gold
//! changes and with each playtime write, never per second.
//!
//! Runs on guacamole's arm only: `boot` spawns the drainer when there's no
//! `FigureFiles` hook, and the product's eframe TV never sends.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, RwLock};
use std::time::Duration;

use skylander_core::{Event, FigureId};
use skylander_rpcs3_control::FigureProgress;
use tokio::sync::{Notify, broadcast};
use tracing::{debug, info, warn};

use crate::instances::Caps;
use crate::profiles::ProfileStore;

/// Playtime reaches the row once this much more of it has been played, so a
/// phone shows it at most this far behind.
pub const PLAYTIME_WRITE_SECS: u32 = 60;

/// The game's progression, read off the disc (AH.2): the experience each
/// level starts at, level 1's 0 first, and the most gold a figure holds.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Progression {
    pub level_xp: Vec<u32>,
    pub gold_cap: u32,
}

impl Progression {
    /// The level a total reaches: how many thresholds it's at or past, so
    /// 999 is level 1 and 1,000 level 2 on Giants' table.
    pub fn level_for(&self, xp: u32) -> u8 {
        let level = self.level_xp.partition_point(|&t| t <= xp).max(1);
        u8::try_from(level).unwrap_or(u8::MAX)
    }

    pub fn max_level(&self) -> u8 {
        u8::try_from(self.level_xp.len()).unwrap_or(u8::MAX)
    }

    /// Experience stops at the table's last entry, gold at the clamp.
    pub fn caps(&self) -> Caps {
        Caps {
            xp: self.level_xp.last().copied().unwrap_or(0),
            gold: self.gold_cap,
        }
    }
}

/// The progression once the current extract holds it, `None` until then.
/// Without it the store clamps nothing and every level reads 0 (unknown), so
/// only gold and playtime writes announce themselves.
#[derive(Clone, Default)]
pub struct ProgressionSlot(Arc<RwLock<Option<Arc<Progression>>>>);

impl ProgressionSlot {
    pub fn get(&self) -> Option<Arc<Progression>> {
        self.0.read().unwrap_or_else(|e| e.into_inner()).clone()
    }

    pub fn publish(&self, progression: Progression) {
        *self.0.write().unwrap_or_else(|e| e.into_inner()) = Some(Arc::new(progression));
    }
}

type Key = (String, FigureId);

/// A map entry: the latest values for a (profile, figure); a newer send replaces it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Sent {
    pub progress: FigureProgress,
    /// Written whole at the next drain: the figure left the portal.
    pub whole: bool,
}

#[derive(Default)]
struct Mailbox {
    latest: HashMap<Key, Sent>,
    /// The level ended or guacamole is quitting: every entry goes in whole.
    flush: bool,
    /// Who's waiting for the flush to land.
    waiters: Vec<std::sync::mpsc::Sender<()>>,
}

#[derive(Default)]
struct Shared {
    mailbox: Mutex<Mailbox>,
    wake: Notify,
    drained: AtomicBool,
}

/// The TV's handle on the map. Every call returns at once and never fails;
/// only [`ProgressSink::flush_and_wait`] blocks, for the quit.
#[derive(Clone, Default)]
pub struct ProgressSink {
    shared: Arc<Shared>,
}

impl ProgressSink {
    pub fn new() -> Self {
        Self::default()
    }

    /// The latest values for `profile_id`'s `figure_id`, replacing any not
    /// yet drained.
    pub fn send(&self, profile_id: &str, figure_id: &FigureId, progress: FigureProgress) {
        self.put(profile_id, figure_id, progress, false);
    }

    /// As [`Self::send`], for a figure that just left the portal: its playtime
    /// goes in with the rest instead of waiting for the next minute.
    pub fn send_final(&self, profile_id: &str, figure_id: &FigureId, progress: FigureProgress) {
        self.put(profile_id, figure_id, progress, true);
    }

    fn put(&self, profile_id: &str, figure_id: &FigureId, progress: FigureProgress, whole: bool) {
        let mut mailbox = self.lock();
        let key = (profile_id.to_string(), figure_id.clone());
        // A figure put back before the drain still owes its whole write.
        let whole = whole || mailbox.latest.get(&key).is_some_and(|s| s.whole);
        mailbox.latest.insert(key, Sent { progress, whole });
        drop(mailbox);
        self.shared.wake.notify_one();
    }

    /// The level ended: everything sent goes in whole at the next drain.
    pub fn flush(&self) {
        self.lock().flush = true;
        self.shared.wake.notify_one();
    }

    /// The quit: [`Self::flush`], then waits up to `timeout` for it to land.
    /// False when it didn't, or no drainer runs. Blocks; call it outside the
    /// server's runtime.
    pub fn flush_and_wait(&self, timeout: Duration) -> bool {
        if !self.shared.drained.load(Ordering::Acquire) {
            return false;
        }
        let (tx, rx) = std::sync::mpsc::channel();
        {
            let mut mailbox = self.lock();
            mailbox.flush = true;
            mailbox.waiters.push(tx);
        }
        self.shared.wake.notify_one();
        rx.recv_timeout(timeout).is_ok()
    }

    /// A flush asked for and not yet drained.
    pub fn flush_asked(&self) -> bool {
        self.lock().flush
    }

    /// What's waiting for the drainer, by profile and figure.
    pub fn waiting(&self) -> Vec<(String, FigureId, Sent)> {
        let mut out: Vec<_> = self
            .lock()
            .latest
            .iter()
            .map(|((p, f), s)| (p.clone(), f.clone(), *s))
            .collect();
        out.sort_by(|a, b| (&a.0, a.1.as_str()).cmp(&(&b.0, b.1.as_str())));
        out
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Mailbox> {
        self.shared
            .mailbox
            .lock()
            .unwrap_or_else(|e| e.into_inner())
    }
}

/// The server thread's side: writes what the TV sent into the store.
pub struct Drainer {
    sink: ProgressSink,
    store: ProfileStore,
    events: broadcast::Sender<Event>,
    progression: ProgressionSlot,
    /// Sent but not all written: playtime short of the next minute, or a
    /// write the store refused (retried at the next drain).
    pending: HashMap<Key, Sent>,
}

/// Starts the drainer on the current runtime. Call it once, for guacamole.
pub fn spawn_drainer(
    sink: ProgressSink,
    store: ProfileStore,
    events: broadcast::Sender<Event>,
    progression: ProgressionSlot,
) -> tokio::task::JoinHandle<()> {
    sink.shared.drained.store(true, Ordering::Release);
    let drainer = Drainer::new(sink, store, events, progression);
    tokio::spawn(drainer.run())
}

impl Drainer {
    pub fn new(
        sink: ProgressSink,
        store: ProfileStore,
        events: broadcast::Sender<Event>,
        progression: ProgressionSlot,
    ) -> Self {
        Self {
            sink,
            store,
            events,
            progression,
            pending: HashMap::new(),
        }
    }

    async fn run(mut self) {
        info!("figure progress drainer running");
        loop {
            self.sink.shared.wake.notified().await;
            self.drain().await;
        }
    }

    /// Takes everything sent since the last drain and writes what's due.
    pub async fn drain(&mut self) {
        let Mailbox {
            latest,
            flush,
            waiters,
        } = std::mem::take(&mut *self.sink.lock());
        let mut due: Vec<Key> = Vec::with_capacity(latest.len());
        for (key, sent) in latest {
            let whole = sent.whole || self.pending.get(&key).is_some_and(|p| p.whole);
            self.pending.insert(key.clone(), Sent { whole, ..sent });
            due.push(key);
        }
        if flush {
            due = self.pending.keys().cloned().collect();
        }
        let progression = self.progression.get();
        for key in due {
            self.settle(key, flush, progression.as_deref()).await;
        }
        for waiter in waiters {
            let _ = waiter.send(());
        }
    }

    async fn settle(&mut self, key: Key, flush: bool, progression: Option<&Progression>) {
        let Some(sent) = self.pending.get(&key).copied() else {
            return;
        };
        let (profile_id, figure_id) = (&key.0, key.1.as_str());
        let whole = flush || sent.whole;
        let instances = self.store.instances();
        let row = match instances.read(profile_id, figure_id).await {
            Ok(Some(row)) => row.progress,
            Ok(None) => {
                debug!(profile_id, figure_id, "no row; dropped what the TV sent");
                self.pending.remove(&key);
                return;
            }
            Err(e) => {
                warn!(profile_id, figure_id, "read the figure's row: {e:#}; kept");
                return;
            }
        };
        if sent.progress.generation < row.generation {
            debug!(
                profile_id,
                figure_id,
                sent = sent.progress.generation,
                row = row.generation,
                "an edit or a reset came since; dropped what the TV sent",
            );
            self.pending.remove(&key);
            return;
        }
        let caps = progression.map_or(Caps::NONE, Progression::caps);
        let p = sent.progress;
        let earned = caps.clamp(p.xp, p.gold) != (row.xp, row.gold);
        let playtime = if whole {
            p.playtime_secs != row.playtime_secs
        } else {
            p.playtime_secs >= row.playtime_secs.saturating_add(PLAYTIME_WRITE_SECS)
        };
        if earned || playtime {
            let after = match instances
                .write(profile_id, figure_id, p, playtime, caps)
                .await
            {
                Ok(Some(after)) => after.progress,
                Ok(None) => {
                    debug!(profile_id, figure_id, "the row moved on mid-write; dropped");
                    self.pending.remove(&key);
                    return;
                }
                Err(e) => {
                    warn!(profile_id, figure_id, "write the figure's row: {e:#}; kept");
                    return;
                }
            };
            let level = |xp| progression.map_or(0, |g| g.level_for(xp));
            if playtime || level(row.xp) != level(after.xp) || row.gold != after.gold {
                let _ = self.events.send(Event::FigureUpdated {
                    figure_id: key.1.clone(),
                    level: level(after.xp),
                    gold: u16::try_from(after.gold).unwrap_or(u16::MAX),
                });
            }
        }
        // Playtime short of the next minute waits; the rest is in.
        let waits = !whole && !playtime && p.playtime_secs != row.playtime_secs;
        if !waits {
            self.pending.remove(&key);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ERUPTOR: &str = "000009-0000";
    const TABLE: [u32; 15] = [
        0, 1000, 2200, 3800, 6000, 9000, 13000, 18200, 24800, 33000, 42700, 53900, 66600, 80800,
        96500,
    ];

    /// Giants' numbers, as AH.2 reads them off the disc.
    fn giants() -> Progression {
        Progression {
            level_xp: TABLE.to_vec(),
            gold_cap: 65_000,
        }
    }

    struct Rig {
        sink: ProgressSink,
        store: ProfileStore,
        events: broadcast::Receiver<Event>,
        drainer: Drainer,
        alice: String,
        eruptor: FigureId,
        /// The row's values when the figure was placed.
        placed: FigureProgress,
    }

    async fn rig() -> Rig {
        let store = ProfileStore::open_in_memory(&crate::setup::MIGRATOR)
            .await
            .unwrap();
        let alice = store.create("Alice", "1234", "#e63946").await.unwrap();
        let placed = store.instances().fresh(&alice, ERUPTOR).await.unwrap();
        let (tx, events) = broadcast::channel(256);
        let sink = ProgressSink::new();
        let slot = ProgressionSlot::default();
        slot.publish(giants());
        let drainer = Drainer::new(sink.clone(), store.clone(), tx, slot);
        Rig {
            sink,
            store,
            events,
            drainer,
            alice,
            eruptor: FigureId::new(ERUPTOR),
            placed: placed.progress,
        }
    }

    impl Rig {
        fn send(&self, xp: u32, gold: u32, playtime_secs: u32) {
            let p = FigureProgress {
                xp,
                gold,
                playtime_secs,
                ..self.placed
            };
            self.sink.send(&self.alice, &self.eruptor, p);
        }

        async fn row(&self) -> FigureProgress {
            let row = self.store.instances().read(&self.alice, ERUPTOR).await;
            row.unwrap().unwrap().progress
        }

        /// The `FigureUpdated`s broadcast since the last call, as (level, gold).
        fn updates(&mut self) -> Vec<(u8, u16)> {
            let mut out = Vec::new();
            while let Ok(e) = self.events.try_recv() {
                if let Event::FigureUpdated {
                    figure_id,
                    level,
                    gold,
                } = e
                {
                    assert_eq!(figure_id.as_str(), ERUPTOR);
                    out.push((level, gold));
                }
            }
            out
        }
    }

    #[test]
    fn a_level_is_how_many_thresholds_the_total_reached() {
        let g = giants();
        let at = |xp| g.level_for(xp);
        assert_eq!(
            [at(0), at(999), at(1000), at(96_499), at(96_500)],
            [1, 1, 2, 14, 15]
        );
        assert_eq!(at(u32::MAX), 15);
        assert_eq!(g.max_level(), 15);
        assert_eq!(
            g.caps(),
            Caps {
                xp: 96_500,
                gold: 65_000
            }
        );
    }

    #[tokio::test]
    async fn experience_under_the_next_level_is_written_and_announces_nothing() {
        let mut r = rig().await;
        r.send(4, 0, 0);
        r.drainer.drain().await;
        assert_eq!(r.row().await.xp, 4, "written at once");
        assert_eq!(r.updates(), vec![], "level 1 both sides");

        r.send(1000, 0, 0);
        r.drainer.drain().await;
        assert_eq!(r.updates(), vec![(2, 0)], "the level changed");
    }

    #[tokio::test]
    async fn a_gold_change_announces_once() {
        let mut r = rig().await;
        r.send(0, 2, 0);
        r.drainer.drain().await;
        assert_eq!(r.row().await.gold, 2);
        assert_eq!(r.updates(), vec![(1, 2)]);
        // The same values again: nothing to write or say.
        r.send(0, 2, 0);
        r.drainer.drain().await;
        assert_eq!(r.updates(), vec![]);
    }

    /// One drain a second, as the TV sends a second of playtime at a time.
    #[tokio::test]
    async fn a_minute_of_play_writes_playtime_and_announces_once() {
        let mut r = rig().await;
        for s in 1..PLAYTIME_WRITE_SECS {
            r.send(0, 0, s);
            r.drainer.drain().await;
        }
        assert_eq!(r.row().await.playtime_secs, 0, "written early");
        assert_eq!(r.updates(), vec![], "announced per second");
        for s in PLAYTIME_WRITE_SECS..PLAYTIME_WRITE_SECS * 2 {
            r.send(0, 0, s);
            r.drainer.drain().await;
        }
        assert_eq!(r.row().await.playtime_secs, PLAYTIME_WRITE_SECS);
        assert_eq!(r.updates(), vec![(1, 0)], "once for the minute");
    }

    #[tokio::test]
    async fn a_removal_writes_the_playtime_waiting() {
        let mut r = rig().await;
        r.send(0, 0, 30);
        r.drainer.drain().await;
        assert_eq!(r.row().await.playtime_secs, 0);

        let p = FigureProgress {
            playtime_secs: 31,
            ..r.placed
        };
        r.sink.send_final(&r.alice, &r.eruptor, p);
        r.drainer.drain().await;
        assert_eq!(r.row().await.playtime_secs, 31);
        assert_eq!(r.updates(), vec![(1, 0)]);
        assert!(r.drainer.pending.is_empty());
    }

    #[tokio::test]
    async fn the_level_ending_writes_every_figures_playtime() {
        let mut r = rig().await;
        r.send(0, 0, 45);
        r.drainer.drain().await;
        r.sink.flush();
        r.drainer.drain().await;
        assert_eq!(r.row().await.playtime_secs, 45);
        assert_eq!(r.updates(), vec![(1, 0)]);
    }

    #[tokio::test]
    async fn values_from_before_an_edit_or_a_reset_are_dropped() {
        let mut r = rig().await;
        // An orb, not drained yet, then the phone's edit.
        r.send(5, 0, 0);
        r.store
            .instances()
            .set(&r.alice, ERUPTOR, 33_000, 50, giants().caps())
            .await
            .unwrap();
        r.drainer.drain().await;
        let row = r.row().await;
        assert_eq!((row.xp, row.gold), (33_000, 50));
        assert_eq!(r.updates(), vec![]);

        // A coin, then a reset; the next placement is fresh.
        r.send(5, 1, 0);
        r.store.instances().delete(&r.alice, ERUPTOR).await.unwrap();
        r.drainer.drain().await;
        let fresh = r.store.instances().fresh(&r.alice, ERUPTOR).await.unwrap();
        r.send(5, 1, 0);
        r.drainer.drain().await;
        assert_eq!((fresh.progress.xp, fresh.progress.gold), (0, 0));
        assert_eq!(r.row().await, fresh.progress);
        assert!(r.drainer.pending.is_empty());
    }

    #[tokio::test]
    async fn the_caps_clamp_what_the_tv_sends() {
        let mut r = rig().await;
        r.send(96_499, 64_999, 0);
        r.drainer.drain().await;
        r.send(96_504, 65_001, 0);
        r.drainer.drain().await;
        let row = r.row().await;
        assert_eq!((row.xp, row.gold), (96_500, 65_000));
        assert_eq!(r.updates(), vec![(14, 64_999), (15, 65_000)]);
    }

    #[tokio::test]
    async fn a_figure_with_no_row_writes_nothing() {
        let mut r = rig().await;
        let p = FigureProgress {
            xp: 4,
            ..FigureProgress::default()
        };
        r.sink.send(&r.alice, &FigureId::new("000070-0000"), p);
        r.drainer.drain().await;
        assert_eq!(
            r.store
                .instances()
                .read(&r.alice, "000070-0000")
                .await
                .unwrap(),
            None
        );
        assert!(r.drainer.pending.is_empty());
    }

    /// The drainer as `boot` runs it, and the quit as `main` waits on it.
    async fn spawned() -> (ProgressSink, ProfileStore, String, FigureProgress) {
        let r = rig().await;
        let Rig {
            sink,
            store,
            drainer,
            alice,
            placed,
            ..
        } = r;
        sink.shared.drained.store(true, Ordering::Release);
        tokio::spawn(drainer.run());
        (sink, store, alice, placed)
    }

    async fn quit(sink: &ProgressSink) {
        let sink = sink.clone();
        let landed =
            tokio::task::spawn_blocking(move || sink.flush_and_wait(Duration::from_secs(30)));
        assert!(landed.await.unwrap(), "the quit's flush never landed");
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn a_burst_then_a_quit_keeps_the_final_values() {
        let (sink, store, alice, placed) = spawned().await;
        let eruptor = FigureId::new(ERUPTOR);
        let last = 20_000;
        for n in 1..=last {
            let p = FigureProgress {
                xp: n,
                gold: n / 4,
                playtime_secs: n / 60,
                ..placed
            };
            sink.send(&alice, &eruptor, p);
        }
        quit(&sink).await;
        let row = store.instances().read(&alice, ERUPTOR).await.unwrap();
        let row = row.unwrap().progress;
        assert_eq!(
            (row.xp, row.gold, row.playtime_secs),
            (last, last / 4, last / 60)
        );
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn a_quit_straight_after_an_orb_keeps_it() {
        let (sink, store, alice, placed) = spawned().await;
        let p = FigureProgress { xp: 1, ..placed };
        sink.send(&alice, &FigureId::new(ERUPTOR), p);
        quit(&sink).await;
        let row = store.instances().read(&alice, ERUPTOR).await.unwrap();
        assert_eq!(row.unwrap().progress.xp, 1);
    }

    #[test]
    fn a_quit_with_no_drainer_returns_at_once() {
        let sink = ProgressSink::new();
        assert!(!sink.flush_and_wait(Duration::from_secs(30)));
    }

    #[test]
    fn the_map_keeps_the_latest_per_figure_and_a_removal_sticks() {
        let sink = ProgressSink::new();
        let eruptor = FigureId::new(ERUPTOR);
        let at = |xp| FigureProgress {
            xp,
            ..FigureProgress::default()
        };
        sink.send_final("alice", &eruptor, at(3));
        sink.send("alice", &eruptor, at(4));
        sink.send("bob", &eruptor, at(9));
        let waiting = sink.waiting();
        assert_eq!(waiting.len(), 2);
        assert_eq!(waiting[0].0, "alice");
        assert_eq!(
            waiting[0].2,
            Sent {
                progress: at(4),
                whole: true
            }
        );
        assert_eq!(
            waiting[1].2,
            Sent {
                progress: at(9),
                whole: false
            }
        );
    }
}
