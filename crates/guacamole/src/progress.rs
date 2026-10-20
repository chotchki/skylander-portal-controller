//! The TV's half of a figure's progress (openspec AH.1, design decision 8):
//! each owned figure on the portal with the values the TV holds for it, handed
//! to the server's latest-wins map whenever they change, in whole when the
//! figure leaves the portal or the level ends. The quit's flush is `main`'s,
//! once the app has exited. AH.23's figure buffer writes [`Earner::progress`]
//! after each logic tick; nothing else changes it yet. Once the extract's
//! progression tables are in (AH.2), what a figure holds stops at the game's
//! caps on the TV too, so nothing past them is sent or carried on.

use std::collections::HashMap;

use bevy::prelude::*;
use guac_server::progress::{ProgressSink, ProgressionSlot};
use skylander_core::{FigureId, SLOT_COUNT};
use skylander_rpcs3_control::{FigureProgress, Placement};

use crate::cards::PortalChanged;
use crate::driver::PortalChange;

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

/// Made when an owned figure is placed, dropped when it leaves. `progress` is
/// the TV's; the server's row lags it until the drain.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Earner {
    pub profile_id: String,
    pub figure_id: FigureId,
    pub progress: FigureProgress,
    /// What the server was last handed.
    sent: FigureProgress,
}

/// Per slot, the owned figure earning on it. A figure placed with no profile
/// unlocked has none: it earns for the session alone and nothing is kept.
#[derive(Resource, Default, Debug)]
pub struct Earning {
    pub slots: [Option<Earner>; SLOT_COUNT],
    /// What each (profile, figure) last held, kept across a removal. Put back
    /// under the same generation it carries on from these, which can be ahead
    /// of the row it was placed from; a newer generation (an edit or a reset)
    /// starts from the row.
    held: HashMap<(String, FigureId), FigureProgress>,
}

impl Earning {
    /// The slot's figure left: its values go to the server in whole.
    fn leave(&mut self, slot: usize, out: Option<&ProgressSink>) {
        let Some(e) = self.slots[slot].take() else {
            return;
        };
        if let Some(out) = out {
            out.send_final(&e.profile_id, &e.figure_id, e.progress);
        }
        self.held.insert((e.profile_id, e.figure_id), e.progress);
    }

    fn arrive(&mut self, slot: usize, placement: &Placement) {
        let Some(owner) = &placement.owner else {
            return;
        };
        let key = (owner.profile_id.clone(), placement.figure_id.clone());
        let progress = match self.held.get(&key) {
            Some(held) if held.generation == owner.progress.generation => *held,
            _ => owner.progress,
        };
        self.slots[slot] = Some(Earner {
            profile_id: key.0,
            figure_id: key.1,
            progress,
            sent: progress,
        });
    }
}

pub struct ProgressPlugin;

impl Plugin for ProgressPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Earning>()
            .add_message::<LevelEnded>()
            .add_systems(
                Update,
                (follow_portal, send_changes, flush_at_level_end).chain(),
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
        if e.progress != e.sent {
            out.0.send(&e.profile_id, &e.figure_id, e.progress);
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

    #[test]
    fn a_placement_with_no_owner_earns_nothing_for_anyone() {
        let mut tv = tv();
        tv.place(3, eruptor(None));
        assert!(tv.app.world().resource::<Earning>().slots[3].is_none());
        tv.clear(3);
        assert_eq!(tv.waiting(), vec![]);
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
        assert_eq!(earning.slots[0].as_ref().unwrap().profile_id, "bob");
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
