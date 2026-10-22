//! The TV's half of a figure's progress (openspec AH.1, design decision 8):
//! each figure on the portal with the values the TV holds for it, handed to
//! the server's latest-wins map whenever they change, in whole when the
//! figure leaves the portal or the level ends. The quit's flush is `main`'s,
//! once the app has exited. Once the extract's progression tables are in
//! (AH.2), what a figure holds stops at the game's caps on the TV too, so
//! nothing past them is sent or carried on.
//!
//! What changes the values (AH.23):
//! - The hero's figure in his scripts ([`crate::hero_scripts`]): his tag's
//!   words are made from his slot's values when his scripts join, and the
//!   experience and gold the scripts leave in them are read back after every
//!   step ([`Earning::earned`]), under the generation they were made from:
//!   values from before an edit or a reset are dropped.
//! - Playtime ([`count_playtime`]): each figure on the portal counts its own
//!   time there while a level shows, in play, paused, held under an alert
//!   and with no hero in it alike, as the original's save control counts it
//!   on a timer that runs with every game frame (AH.11). The original folds
//!   that time into the figure's words only at its tag writes, which don't
//!   run here, so the words keep the playtime they were made with and the
//!   map gets the count (design decision 2's stand-in row, Playtime).
//!
//! A figure placed with no profile unlocked earns too, its values starting
//! fresh, but they go nowhere and are dropped when it leaves the portal.

use std::collections::HashMap;
use std::time::Duration;

use bevy::prelude::*;
use guac_server::progress::{ProgressSink, ProgressionSlot};
use guacamole::figure::{Tag, Values};
use skylander_core::{FigureId, SLOT_COUNT};
use skylander_rpcs3_control::{FigureProgress, Placement};

use crate::cards::PortalChanged;
use crate::driver::PortalChange;
use crate::play::Play;

/// The server's end of the map; absent when the server never came up.
#[derive(Resource, Clone)]
pub struct ProgressOut(pub ProgressSink);

/// The server's progression slot, which the extraction fills (AH.2): the
/// caps the TV holds every figure's values to once it's filled. Absent when
/// the server never came up.
#[derive(Resource, Clone)]
pub struct ProgressionIn(pub ProgressionSlot);

/// Whatever ends the level writes this, so every figure's values go in whole.
#[derive(Message, Debug, Clone, Copy, PartialEq, Eq)]
pub struct LevelEnded;

/// Made when a figure is placed, dropped when it leaves. `progress` is the
/// TV's; the server's row lags it until the drain.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Earner {
    /// The placing profile; `None` when no profile was unlocked.
    pub profile_id: Option<String>,
    pub figure_id: FigureId,
    /// The toy and its subtype, as its tag names them.
    pub toy_id: u32,
    pub subtype: u32,
    /// The serial its tag stands under: one per (profile, figure) for the
    /// session, as a toy's own serial is one per toy.
    pub serial: u32,
    pub progress: FigureProgress,
    /// What the server was last handed.
    sent: FigureProgress,
    /// Its playtime as it arrived, and its time on the portal since while a
    /// level showed.
    playtime_at: u32,
    played: Duration,
}

impl Earner {
    /// The tag its figure's words are made from (AH.23): its toy, its
    /// serial and its values now.
    pub fn tag(&self) -> Tag {
        Tag {
            toy_id: self.toy_id,
            subtype: self.subtype,
            serial: self.serial,
            values: Values {
                xp: self.progress.xp,
                gold: self.progress.gold,
                playtime_secs: self.progress.playtime_secs,
            },
        }
    }
}

/// Per slot, the figure earning on it.
#[derive(Resource, Default, Debug)]
pub struct Earning {
    pub slots: [Option<Earner>; SLOT_COUNT],
    /// What each (profile, figure) last held, kept across a removal. Put back
    /// under the same generation it carries on from these, which can be ahead
    /// of the row it was placed from; a newer generation (an edit or a reset)
    /// starts from the row.
    held: HashMap<(String, FigureId), FigureProgress>,
    /// Each (profile or none, figure)'s serial for the session.
    serials: HashMap<(Option<String>, FigureId), u32>,
}

impl Earning {
    /// The slot's figure left: an owned one's values go to the server in
    /// whole; an ownerless one's go nowhere.
    fn leave(&mut self, slot: usize, out: Option<&ProgressSink>) {
        let Some(e) = self.slots[slot].take() else {
            return;
        };
        let Some(profile_id) = e.profile_id else {
            return;
        };
        if let Some(out) = out {
            out.send_final(&profile_id, &e.figure_id, e.progress);
        }
        self.held.insert((profile_id, e.figure_id), e.progress);
    }

    fn arrive(&mut self, slot: usize, placement: &Placement) {
        let profile_id = placement.owner.as_ref().map(|o| o.profile_id.clone());
        let progress = match &placement.owner {
            Some(owner) => {
                let key = (owner.profile_id.clone(), placement.figure_id.clone());
                match self.held.get(&key) {
                    Some(held) if held.generation == owner.progress.generation => *held,
                    _ => owner.progress,
                }
            }
            // Fresh for the session alone.
            None => FigureProgress::default(),
        };
        let next = self.serials.len() as u32 + 1;
        let serial = *self
            .serials
            .entry((profile_id.clone(), placement.figure_id.clone()))
            .or_insert(next);
        self.slots[slot] = Some(Earner {
            profile_id,
            figure_id: placement.figure_id.clone(),
            toy_id: placement.toy_type.get(),
            subtype: u32::from(placement.variant.0),
            serial,
            progress,
            sent: progress,
            playtime_at: progress.playtime_secs,
            played: Duration::ZERO,
        });
    }

    /// The experience and gold the slot's figure's words hold after a step
    /// (AH.23), made from its values under `generation`: false, and nothing
    /// changed, when the slot's figure is under another generation now (an
    /// edit or a reset came since) or has left.
    pub fn earned(&mut self, slot: usize, values: Values, generation: i64) -> bool {
        match self.slots.get_mut(slot).and_then(Option::as_mut) {
            Some(e) if e.progress.generation == generation => {
                e.progress.xp = values.xp;
                e.progress.gold = values.gold;
                true
            }
            _ => false,
        }
    }
}

pub struct ProgressPlugin;

impl Plugin for ProgressPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Earning>()
            .add_message::<LevelEnded>()
            .add_systems(
                Update,
                (
                    follow_portal,
                    count_playtime,
                    send_changes,
                    flush_at_level_end,
                )
                    .chain(),
            );
    }
}

/// A placement over an occupied slot arrives as one `Placed`: the old figure
/// leaves first.
fn follow_portal(
    mut heard: MessageReader<PortalChanged>,
    mut earning: ResMut<Earning>,
    out: Option<Res<ProgressOut>>,
) {
    let out = out.as_deref().map(|o| &o.0);
    for PortalChanged(change) in heard.read() {
        match change {
            PortalChange::Placed { slot, placement } => {
                earning.leave(slot.as_usize(), out);
                earning.arrive(slot.as_usize(), placement);
            }
            PortalChange::Cleared { slot } => earning.leave(slot.as_usize(), out),
        }
    }
}

/// Every figure on the portal counts its own time there while a level shows
/// (the module docs), on the TV's real clock, so a pause or a hold counts
/// as play does.
fn count_playtime(time: Res<Time<Real>>, play: Option<Res<Play>>, mut earning: ResMut<Earning>) {
    if !play.is_some_and(|p| p.started()) {
        return;
    }
    let dt = time.delta();
    for e in earning.slots.iter_mut().flatten() {
        e.played += dt;
        let secs = u32::try_from(e.played.as_secs()).unwrap_or(u32::MAX);
        e.progress.playtime_secs = e.playtime_at.saturating_add(secs);
    }
}

/// Holds each figure's values to the game's caps, then hands over whatever
/// changed since the last frame, latest wins.
fn send_changes(
    mut earning: ResMut<Earning>,
    out: Option<Res<ProgressOut>>,
    progression: Option<Res<ProgressionIn>>,
) {
    if let Some(caps) = progression.and_then(|p| p.0.get()).map(|p| p.caps()) {
        for e in earning.slots.iter_mut().flatten() {
            (e.progress.xp, e.progress.gold) = caps.clamp(e.progress.xp, e.progress.gold);
        }
    }
    let Some(out) = out else {
        return;
    };
    for e in earning.slots.iter_mut().flatten() {
        let Some(profile_id) = &e.profile_id else {
            continue;
        };
        if e.progress != e.sent {
            out.0.send(profile_id, &e.figure_id, e.progress);
            e.sent = e.progress;
        }
    }
}

fn flush_at_level_end(mut ended: MessageReader<LevelEnded>, out: Option<Res<ProgressOut>>) {
    if ended.read().count() > 0
        && let Some(out) = out
    {
        out.0.flush();
    }
}

#[cfg(test)]
mod tests {
    use guac_server::progress::Sent;
    use skylander_core::SlotIndex;
    use skylander_rpcs3_control::PlacementOwner;

    use super::*;

    const ERUPTOR: &str = "000009-0000";

    fn eruptor(owner: Option<(&str, FigureProgress)>) -> Placement {
        let figure_id = FigureId::new(ERUPTOR);
        let (toy_type, variant) = figure_id.subtype_parts().unwrap();
        Placement {
            figure_id,
            toy_type,
            variant,
            name: "Eruptor".into(),
            element: None,
            portrait: None,
            owner: owner.map(|(id, progress)| PlacementOwner {
                profile_id: id.into(),
                initial: "A".into(),
                color: "#e63946".into(),
                progress,
            }),
        }
    }

    fn row(xp: u32, generation: i64) -> FigureProgress {
        FigureProgress {
            xp,
            generation,
            ..FigureProgress::default()
        }
    }

    struct Tv {
        app: App,
        sink: ProgressSink,
    }

    fn tv() -> Tv {
        let sink = ProgressSink::new();
        let mut app = App::new();
        app.add_plugins(MinimalPlugins)
            .add_message::<PortalChanged>()
            .insert_resource(ProgressOut(sink.clone()))
            .add_plugins(ProgressPlugin);
        app.update();
        Tv { app, sink }
    }

    impl Tv {
        fn change(&mut self, change: PortalChange) {
            self.app.world_mut().write_message(PortalChanged(change));
            self.app.update();
        }

        fn place(&mut self, n: u8, placement: Placement) {
            let slot = SlotIndex::new(n).unwrap();
            self.change(PortalChange::Placed { slot, placement });
        }

        fn clear(&mut self, n: u8) {
            let slot = SlotIndex::new(n).unwrap();
            self.change(PortalChange::Cleared { slot });
        }

        /// What AH.23's figure buffer will do after a logic tick.
        fn earn(&mut self, n: usize, xp: u32) {
            let mut earning = self.app.world_mut().resource_mut::<Earning>();
            earning.slots[n].as_mut().expect("an earner").progress.xp = xp;
            self.app.update();
        }

        fn waiting(&self) -> Vec<(String, Sent)> {
            self.sink
                .waiting()
                .into_iter()
                .map(|(p, f, s)| {
                    assert_eq!(f.as_str(), ERUPTOR);
                    (p, s)
                })
                .collect()
        }
    }

    fn sent(xp: u32, generation: i64, whole: bool) -> Sent {
        Sent {
            progress: row(xp, generation),
            whole,
        }
    }

    #[test]
    fn a_placement_sends_nothing_until_it_earns_and_a_removal_sends_it_whole() {
        let mut tv = tv();
        tv.place(0, eruptor(Some(("alice", row(16, 7)))));
        assert_eq!(tv.waiting(), vec![], "nothing changed yet");

        tv.earn(0, 20);
        assert_eq!(tv.waiting(), vec![("alice".into(), sent(20, 7, false))]);
        tv.clear(0);
        assert_eq!(tv.waiting(), vec![("alice".into(), sent(20, 7, true))]);
        assert!(tv.app.world().resource::<Earning>().slots[0].is_none());
    }

    /// An ownerless figure earns on the TV for its time on the portal, fresh
    /// (its words made from nothing, AH.23), and none of it goes anywhere or
    /// comes back: placed again, it's fresh again.
    #[test]
    fn a_placement_with_no_owner_earns_nothing_for_anyone() {
        let mut tv = tv();
        tv.place(3, eruptor(None));
        let e = tv.app.world().resource::<Earning>().slots[3]
            .clone()
            .unwrap();
        assert_eq!(
            (e.profile_id, e.progress),
            (None, FigureProgress::default())
        );
        tv.earn(3, 4);
        let earning = tv.app.world().resource::<Earning>();
        assert_eq!(earning.slots[3].as_ref().unwrap().progress.xp, 4);
        assert_eq!(tv.waiting(), vec![], "sent nowhere");
        tv.clear(3);
        assert_eq!(tv.waiting(), vec![]);
        tv.place(3, eruptor(None));
        let earning = tv.app.world().resource::<Earning>();
        assert_eq!(
            earning.slots[3].as_ref().unwrap().progress,
            FigureProgress::default(),
            "nothing kept"
        );
    }

    /// What his words held after a step lands on his slot's values under the
    /// generation they were made from (AH.23); after a reset (the slot placed
    /// again under a newer generation, fresh) a read-back from the old words
    /// is dropped, nothing sent.
    #[test]
    fn a_read_back_under_the_old_generation_is_dropped_after_a_reset() {
        let mut tv = tv();
        tv.place(0, eruptor(Some(("alice", row(999, 7)))));
        let earned = |tv: &mut Tv, xp: u32, generation: i64| {
            let v = Values {
                xp,
                gold: 3,
                playtime_secs: 0,
            };
            let ok = tv
                .app
                .world_mut()
                .resource_mut::<Earning>()
                .earned(0, v, generation);
            tv.app.update();
            ok
        };
        assert!(earned(&mut tv, 1_000, 7));
        assert_eq!(
            tv.waiting(),
            vec![(
                "alice".into(),
                Sent {
                    progress: FigureProgress {
                        gold: 3,
                        ..row(1_000, 7)
                    },
                    whole: false
                }
            )]
        );
        // The slot reset: the same figure placed again, fresh under 9; the
        // old one's values go in whole under 7 as it leaves, which the
        // drainer drops against the newer row (AH.1).
        tv.place(0, eruptor(Some(("alice", row(0, 9)))));
        assert!(!earned(&mut tv, 1_004, 7), "the old words' values dropped");
        let earning = tv.app.world().resource::<Earning>();
        assert_eq!(earning.slots[0].as_ref().unwrap().progress, row(0, 9));
        let waiting = tv.waiting();
        assert_eq!(waiting.len(), 1, "{waiting:?}");
        assert_eq!(
            (waiting[0].1.progress.xp, waiting[0].1.progress.generation),
            (1_000, 7),
            "the 1,004 never sent"
        );
        // An empty slot takes nothing.
        tv.clear(0);
        let none = tv
            .app
            .world_mut()
            .resource_mut::<Earning>()
            .earned(0, Values::default(), 9);
        assert!(!none);
    }

    /// A waiting figure's slot reset (AH.3: the same figure placed again,
    /// fresh under a newer generation) rebuilds its tag fresh: the words its
    /// join makes come from the fresh row, not from what it held. Its toy
    /// and its serial stay its own; another profile's copy gets another
    /// serial.
    #[test]
    fn a_waiting_figures_slot_reset_rebuilds_its_tag_fresh() {
        let mut tv = tv();
        tv.place(1, eruptor(Some(("alice", row(999, 7)))));
        let tag = |tv: &Tv, n: usize| {
            let e = tv.app.world().resource::<Earning>();
            e.slots[n].as_ref().unwrap().tag()
        };
        let before = tag(&tv, 1);
        assert_eq!((before.toy_id, before.subtype), (9, 0));
        assert_eq!(before.values.xp, 999);
        tv.place(1, eruptor(Some(("alice", row(0, 9)))));
        let after = tag(&tv, 1);
        assert_eq!(after.values, Values::default(), "fresh");
        assert_eq!(after.serial, before.serial, "the same toy");
        tv.place(2, eruptor(Some(("bob", row(0, 4)))));
        assert_ne!(tag(&tv, 2).serial, before.serial, "another instance");
    }

    /// Each figure on the portal counts its own time there while a level
    /// shows, on the real clock: as the hero, with the game's clock paused
    /// (Start's menu stops `Time<Virtual>`), and beside him under an alert
    /// alike (figure-progress' Playtime counts the time in play, AH.11); not
    /// on the portal view. Each second counted is sent; the drainer writes
    /// it a minute at a time.
    #[test]
    fn playtime_counts_while_a_level_shows_paused_or_not_for_each_figure() {
        use bevy::time::TimeUpdateStrategy;

        let mut tv = tv();
        tv.app
            .insert_resource(TimeUpdateStrategy::ManualDuration(Duration::from_secs(1)));
        tv.place(0, eruptor(Some(("alice", row(0, 7)))));
        let mut rex = eruptor(Some(("bob", row(0, 8))));
        rex.figure_id = FigureId::new("000014-0000");
        tv.place(1, rex);
        let playtime = |tv: &Tv| {
            let e = tv.app.world().resource::<Earning>();
            [0, 1].map(|n| e.slots[n].as_ref().unwrap().progress.playtime_secs)
        };
        // The portal view: the walk's state there, the level not started.
        tv.app.insert_resource(Play::default());
        for _ in 0..30 {
            tv.app.update();
        }
        assert_eq!(playtime(&tv), [0, 0], "the portal view counts nothing");
        let camera = tv.app.world_mut().spawn_empty().id();
        tv.app.insert_resource(Play::started_for_tests(camera));
        for _ in 0..60 {
            tv.app.update();
        }
        assert_eq!(playtime(&tv), [60, 60], "a minute in the level, each");
        tv.app.world_mut().resource_mut::<Time<Virtual>>().pause();
        for _ in 0..60 {
            tv.app.update();
        }
        assert_eq!(playtime(&tv), [120, 120], "a minute paused counts too");
        // Lifted, his count goes in whole; put back, it carries on.
        tv.clear(0);
        let sent = tv.sink.waiting();
        let alice = sent.iter().find(|(p, ..)| p == "alice").unwrap();
        assert_eq!((alice.2.progress.playtime_secs, alice.2.whole), (120, true));
        // Rex counted on through the lift and the placement (two frames),
        // Eruptor from his 120 in the frame he came back.
        tv.place(0, eruptor(Some(("alice", row(0, 7)))));
        assert_eq!(playtime(&tv), [121, 122]);
    }

    /// Lifted and put straight back, the row may not have the last orb yet;
    /// the TV's own values for the same generation carry on.
    #[test]
    fn put_back_under_the_same_generation_carries_on_from_the_tvs_values() {
        let mut tv = tv();
        tv.place(0, eruptor(Some(("alice", row(0, 7)))));
        tv.earn(0, 4);
        tv.clear(0);
        tv.place(1, eruptor(Some(("alice", row(0, 7)))));
        let earning = tv.app.world().resource::<Earning>();
        assert_eq!(earning.slots[1].as_ref().unwrap().progress, row(4, 7));

        // After an edit (a newer generation) the row's values win.
        tv.clear(1);
        tv.place(1, eruptor(Some(("alice", row(33_000, 9)))));
        let earning = tv.app.world().resource::<Earning>();
        assert_eq!(earning.slots[1].as_ref().unwrap().progress, row(33_000, 9));
    }

    #[test]
    fn a_figure_placed_over_another_sends_the_old_one_whole() {
        let mut tv = tv();
        tv.place(0, eruptor(Some(("alice", row(0, 7)))));
        tv.earn(0, 4);
        tv.place(0, eruptor(Some(("bob", row(50, 8)))));
        assert_eq!(tv.waiting(), vec![("alice".into(), sent(4, 7, true))]);
        let earning = tv.app.world().resource::<Earning>();
        assert_eq!(
            earning.slots[0].as_ref().unwrap().profile_id.as_deref(),
            Some("bob")
        );
    }

    /// Past the game's caps nothing is sent or held, once the extract's
    /// tables are in; before then nothing is clamped.
    #[test]
    fn values_past_the_games_caps_stop_at_them() {
        let mut tv = tv();
        let slot = ProgressionSlot::default();
        tv.app.insert_resource(ProgressionIn(slot.clone()));
        tv.place(0, eruptor(Some(("alice", row(0, 7)))));
        tv.earn(0, 120_000);
        assert_eq!(
            tv.waiting(),
            vec![("alice".into(), sent(120_000, 7, false))]
        );

        slot.publish(guac_server::progress::Progression {
            level_xp: vec![0, 1000, 96_500],
            gold_cap: 65_000,
        });
        tv.earn(0, 100_000);
        {
            let mut earning = tv.app.world_mut().resource_mut::<Earning>();
            earning.slots[0].as_mut().unwrap().progress.gold = 70_000;
        }
        tv.app.update();
        let want = FigureProgress {
            gold: 65_000,
            ..row(96_500, 7)
        };
        let earning = tv.app.world().resource::<Earning>();
        assert_eq!(earning.slots[0].as_ref().unwrap().progress, want);
        assert_eq!(
            tv.waiting(),
            vec![(
                "alice".into(),
                Sent {
                    progress: want,
                    whole: false
                }
            )]
        );
    }

    /// The drainer writes the flush (guac_server's tests); the TV only asks.
    #[test]
    fn the_level_ending_asks_for_a_flush_without_blocking() {
        let mut tv = tv();
        tv.place(0, eruptor(Some(("alice", row(0, 7)))));
        tv.earn(0, 1);
        assert!(!tv.sink.flush_asked());
        tv.app.world_mut().write_message(LevelEnded);
        tv.app.update();
        assert!(tv.sink.flush_asked());
        assert_eq!(tv.waiting(), vec![("alice".into(), sent(1, 7, false))]);
    }
}
