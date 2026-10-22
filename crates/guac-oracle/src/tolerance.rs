//! The comparisons' defaults (openspec AF.12, design decision 9), calibrated on
//! AF.11's measurement (10 replays each of an idle and a walk from one gameplay
//! savestate, Eruptor in chapter one's clearing, observed at the end flip only
//! and every 60 flips, oracle build `730baa6c…`) and AF.12's walk with a shot
//! at every checkpoint. The tick notes in the change's tasks.md have every
//! number; a rung that needs others argues them in its own design.
//!
//! A floor is the largest spread the replays reached while they still agreed,
//! the replay that hitched included, rounded up: in the walk, up to the gate's
//! corner (F0 + 180), which split them into two outcomes a metre and more
//! apart. The acceptance is leave-one-out against the other replays' own
//! horizon ([`crate::trace::leave_one_out`]): each replay a candidate against
//! the rest, as a Guacamole run will be, so a floor that's too tight fails a
//! replay that strays alone while the others still agree (the AF.11-AF.13
//! review's re-check set the actor and animation floors that way).
//!
//! The fight's event floors are per anchor, on game time: [`anchor_a`]'s
//! (AH.51, from AH.9's replays through [`crate::fight::log::floors_needed`]).
//! Not set: anchor B's event floors (AH.10 measured them; AH.64) and the
//! totals floors ([`crate::totals`]; AH.30).

use std::collections::BTreeMap;

use crate::fight::extract::press_kind;
use crate::fight::log::{Gate, Judge};
use crate::input::Button;
use crate::trace::TrajectoryTolerance;

/// The multiple of the replays' spread a candidate may stray past their
/// [min, max]; it matters only past a horizon. At these floors and k = 1,
/// AF.11's and AF.12's leave-one-out over whole runs failed no position or
/// angle, only replays that hitched (32.2-55.6 s of running for 30 s of flips,
/// on MT draws and actor counts) and one- or two-step animation LCG
/// excursions; every other excursion stayed within 0.85 of a spread. A
/// branch only one replay takes fails at any k, so past a horizon a
/// comparison is statistical over more replays or re-anchors inside it.
pub const K: f64 = 1.0;

/// Positions in metres: hero, camera, actors. AF.11: idle, the hero and the
/// camera held bit for bit to F0 + 7,200; walking, set B's hero spread was 0
/// to F0 + 60 (the stick moves at 61) and 8.8 cm at F0 + 120 (2.0 cm without
/// the replay that hitched; AF.12's walk 2.9 cm), and at F0 + 180 the gate's
/// corner had split them by 1.01 m. So the walk's horizon at 0.1 m covers its
/// first 60 flips of walking, where a frame of motion (11 cm) shows.
pub const POSITION: TrajectoryTolerance = TrajectoryTolerance { floor: 0.1, k: K };

/// Headings, tilts and rolls in degrees. The facing follows the stick exactly
/// and the camera's angles drift by float rounding alone: AF.11's largest
/// spread before the split was 1.5e-5 deg (2 ulps of an f32 near 105) and the
/// camera's anywhere 2.1e-5, while the facing parted by 6.1e-3 at F0 + 180.
pub const ANGLE: TrajectoryTolerance = TrajectoryTolerance { floor: 1e-4, k: K };

/// Counts that agree exactly or not at all: game frames since F0 (exactly the
/// flips since F0 at every checkpoint of all 45 AF.11 replays) and MT draws
/// since F0 (identical states only at F0, then hundreds of draws apart by
/// F0 + 60).
pub const COUNT: TrajectoryTolerance = TrajectoryTolerance { floor: 0.0, k: K };

/// Actors listed and skinned: birds, leaves and butterflies come and go on
/// host time, and a replay that hitched gets one a checkpoint early or late
/// while the rest still agree. AF.11's idle set B: idle-B-09 (32.9 s of
/// running) listed one actor more at F0 + 1,260 while the other nine agreed
/// through 1,500, so at floor 0 it fails the others' envelope; at 1 every
/// reference group passes its leave-one-out (the review's re-check), and the
/// walk's counts still part at F0 + 240, by 7, where its outcomes split.
pub const ACTORS: TrajectoryTolerance = TrajectoryTolerance { floor: 1.0, k: K };

/// The animation variety LCG's steps since F0 (AF.8's pair at `0x008E20F0`):
/// a weighted clip pick a frame early or late steps it once more. AF.11's idle
/// set B: idle-B-01 stepped once at F0 + 240, the other nine not until later,
/// so at floor 0 it fails the others' envelope; at 1 every reference group
/// passes (the review's re-check), while the replays are up to 4 apart by
/// F0 + 1,800.
pub const ANIMATION_STEPS: TrajectoryTolerance = TrajectoryTolerance { floor: 1.0, k: K };

/// The frame comparison's floor over the largest mean FLIP between two
/// replays' frames at a checkpoint ([`crate::frames::FrameBand::check`]),
/// gated inside the hero's and the camera's horizons. At floor 0 the two
/// replays of a checkpoint's largest pair fail their own leave-one-out, so
/// this is the largest lead any replay had there over the others' band,
/// rounded up: 0.0262 (AF.11's idle set A at F0 + 1,800, the replay that ran
/// slow); AF.12's walk needed 0.0199 at most (F0 + 120).
pub const FRAME_FLOOR: f64 = 0.03;

/// One anchor's event floors in game seconds by event kind (the kinds
/// [`crate::fight::extract::logs`] writes) and the kinds a roll times there.
/// Design decision 18 judges by them under AH.45's (b), chotchki's call on
/// 2026-10-21 ("I agree on option B until we see if it breaks stuff"):
/// [`EventFloors::judge`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct EventFloors {
    /// Every `press <button>` kind: the pad's own stamp, alike for every
    /// button.
    pub press: f64,
    /// The other kinds by name.
    pub kinds: &'static [(&'static str, f64)],
    /// A kind not named.
    pub default: f64,
    /// The kinds a roll times: stamped inside the anchor's horizon, by order
    /// alone past it.
    pub rolled: &'static [&'static str],
}

impl EventFloors {
    /// The judge at this anchor (AH.30's): every kind no roll times stamped
    /// wherever it falls, the horizon (the last game frame since F0 every
    /// replay's generator drew alike by, [`crate::fight::draws`]) gating only
    /// the rolled kinds. A rolled kind stamped inside the horizon lines up
    /// between oracle replays but not against guacamole, whose generator reads
    /// the stream at other places (decision 18); no event falls inside anchor
    /// A's or B's horizon, so nothing yet depends on which.
    pub fn judge(&self, horizon: Option<u32>) -> Judge {
        let mut floors: BTreeMap<String, f64> = Button::ALL
            .iter()
            .map(|&b| (press_kind(b), self.press))
            .collect();
        floors.extend(self.kinds.iter().map(|&(k, f)| (k.to_owned(), f)));
        Judge {
            default: self.default,
            floors,
            horizon,
            ordered: self.rolled.iter().map(|&k| k.to_owned()).collect(),
            gate: Gate::RolledKinds,
        }
    }
}

/// Anchor A's event floors (openspec AH.51): AF.12's method over AH.9's 50
/// pause-free replays from anchor A (`245dec5d…`, Eruptor at rest in chapter
/// one's clearing), 10 each of `lob`, `erupt`, `lobs`, `erupts` and `coin`,
/// every kind stamped: a kind's floor is the least that passes every replay's
/// leave-one-out in all five groups, rounded up to 5 ms. What they need is
/// a game frame (1/60 s) or two, where a replay keyed one later; the events
/// came out alike to the frame while the generator parted 1 to 4 frames from
/// F0 on particles drawn on host time (AH.45). At these floors every replay
/// passes and the first press moved 1 or 2 flips either way fails, on the
/// throw as well as the press: the `#[ignore]`d `anchor_a` test in
/// `tools/oracle-harness` re-derives both from the recordings.
pub mod anchor_a {
    use super::EventFloors;

    /// `press <button>`: 1.8 ms at most (`erupts`).
    pub const PRESS: f64 = 0.005;
    /// `throw`, the attack's clip starting: 1.5 ms (`lob`).
    pub const THROW: f64 = 0.005;
    /// `spawn`, a ball, an eruption, debris or a coin listed: 33.6 ms (`lobs`).
    pub const SPAWN: f64 = 0.035;
    /// `break`: 18.0 ms (`lob`).
    pub const BREAK: f64 = 0.020;
    /// `gone`, a ball bursting or a breakable, its debris or an eruption
    /// leaving: 33.4 ms (`lobs`).
    pub const GONE: f64 = 0.035;
    /// The eruption's `state LavaBlobbing`: 2.1 ms (`erupts`).
    pub const LAVA_BLOBBING: f64 = 0.005;
    /// Its `state Going Black`: 16.5 ms (`erupts`).
    pub const GOING_BLACK: f64 = 0.020;
    /// Its `state Fading`: 16.5 ms (`erupts`).
    pub const FADING: f64 = 0.020;
    /// `pickup`, a coin taken leaving at the end of its slide: 18.1 ms
    /// (`coin`). The 0.25 s check it slides off is a roll's to time (AH.45),
    /// so this judges only inside a horizon, and anchor A's ends 0 to 3
    /// frames past F0 by script, a second before any coin is taken.
    pub const PICKUP: f64 = 0.020;
    /// A kind AH.9 never logged: the largest above.
    pub const DEFAULT: f64 = 0.035;

    pub const EVENTS: EventFloors = EventFloors {
        press: PRESS,
        kinds: &[
            ("throw", THROW),
            ("spawn", SPAWN),
            ("break", BREAK),
            ("gone", GONE),
            ("state LavaBlobbing", LAVA_BLOBBING),
            ("state Going Black", GOING_BLACK),
            ("state Fading", FADING),
            ("pickup", PICKUP),
        ],
        default: DEFAULT,
        rolled: &["pickup"],
    };
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fight::log::{ActorEvent, ActorLogs, LogMismatch, LogReference};

    #[test]
    fn the_defaults_are_tolerances_the_metrics_take() {
        for t in [POSITION, ANGLE, COUNT, ACTORS, ANIMATION_STEPS] {
            assert!(t.floor.is_finite() && t.floor >= 0.0);
            assert_eq!(t.k, K);
            // A spread under the floor leaves the floor as the margin.
            assert_eq!(t.margin(t.floor / 2.0), t.floor);
        }
        assert!(FRAME_FLOOR.is_finite() && FRAME_FLOOR >= 0.0);
        // A frame of walking (6.55 m/s for 1/60 s) clears the position floor;
        // the walk's live spread (8.8 cm) doesn't.
        const { assert!(6.553 / 60.0 > POSITION.floor) };
        const { assert!(0.088 < POSITION.floor) };
        // An f32 heading near 105 deg rounds to steps far under the floor.
        let ulp = f64::from(105.0f32.next_up() - 105.0f32);
        assert!(10.0 * ulp < ANGLE.floor);
    }

    /// Game time at `frame` in replay `r`: a frame is 1/60 s and each
    /// replay keys 0.1 ms later than the one before.
    fn at(frame: u32, r: u32) -> ActorEvent {
        ActorEvent::new("", f64::from(frame) / 60.0 + 1e-4 * f64::from(r), frame)
    }

    /// AH.9's `lob` from anchor A in frames since F0: South at 31, the throw
    /// at 32, the ball listed at 36 and bursting on the barrel at `hit`, its
    /// coin listed a frame later and taken at `taken`.
    fn lob(r: u32, press: u32, hit: u32, taken: u32) -> ActorLogs {
        let shift = |f: u32| f + press - 31;
        let log = |events: &[(&str, u32)]| -> Vec<ActorEvent> {
            events
                .iter()
                .map(|&(kind, frame)| ActorEvent {
                    kind: kind.to_owned(),
                    ..at(frame, r)
                })
                .collect()
        };
        ActorLogs::new(BTreeMap::from([
            (
                "hero".to_owned(),
                log(&[("press South", shift(31)), ("throw", shift(32))]),
            ),
            (
                "hero>lava_ball#1".to_owned(),
                log(&[("spawn", shift(36)), ("gone", shift(hit))]),
            ),
            (
                "barrel_b_intact(1)".to_owned(),
                log(&[("gone", shift(hit)), ("break", shift(hit))]),
            ),
            (
                "barrel_b_intact(1)>coin#1".to_owned(),
                log(&[("spawn", shift(hit + 1)), ("pickup", taken)]),
            ),
        ]))
        .unwrap()
    }

    /// Ten replays, the burst at 65 or 66 and the coin taken at 120 or 121.
    fn reference() -> LogReference {
        let runs: Vec<ActorLogs> = (0..10)
            .map(|r| lob(r, 31, 65 + r % 2, 120 + r % 2))
            .collect();
        LogReference::from_runs(&runs).unwrap()
    }

    /// Anchor A's horizon: F0 itself (AH.9).
    fn anchor_a_judge() -> Judge {
        anchor_a::EVENTS.judge(Some(0))
    }

    #[test]
    fn a_kind_no_roll_times_is_judged_past_the_horizon() {
        let reference = reference();
        let judge = anchor_a_judge();
        let v = reference.check(&lob(0, 31, 66, 120), &judge).unwrap();
        assert!(v.passed(), "{v:?}");
        // Every event past F0 but the rolled pickup is stamped.
        assert_eq!(v.actors["barrel_b_intact(1)"].stamped, 2);
        assert_eq!(v.actors["barrel_b_intact(1)>coin#1"].stamped, 1);
        // The burst two frames past the latest: the break's 20 ms floor
        // fails it, while the leaving's 35 ms lets it by.
        let late = reference.check(&lob(0, 31, 68, 121), &judge).unwrap();
        assert_eq!(late.failed(), ["barrel_b_intact(1)"]);
        assert!(matches!(
            late.actors["barrel_b_intact(1)"].mismatches[..],
            [LogMismatch::Stamp { index: 1, ref kind, .. }] if kind == "break"
        ));
        // Decision 18 as first written stamps nothing past F0.
        let first = Judge {
            gate: Gate::AllKinds,
            ..judge
        };
        let v = reference.check(&lob(0, 31, 68, 121), &first).unwrap();
        assert!(v.passed(), "{v:?}");
        assert!(v.actors.values().all(|a| a.stamped == 0));
    }

    #[test]
    fn a_rolled_kind_past_the_horizon_is_skipped() {
        let reference = reference();
        // The coin taken half a second late passes on order alone.
        let slow = lob(0, 31, 65, 150);
        let v = reference.check(&slow, &anchor_a_judge()).unwrap();
        assert!(v.passed(), "{v:?}");
        // Inside a horizon it's stamped, and fails.
        let v = reference
            .check(&slow, &anchor_a::EVENTS.judge(Some(200)))
            .unwrap();
        assert_eq!(v.failed(), ["barrel_b_intact(1)>coin#1"]);
        // Its order still counts past the horizon: never taken fails.
        let mut logs: BTreeMap<String, Vec<ActorEvent>> = slow.into();
        logs.get_mut("barrel_b_intact(1)>coin#1").unwrap().pop();
        let v = reference
            .check(&ActorLogs::new(logs).unwrap(), &anchor_a_judge())
            .unwrap();
        assert!(matches!(
            v.actors["barrel_b_intact(1)>coin#1"].mismatches[..],
            [LogMismatch::Missing { index: 1, .. }]
        ));
    }

    #[test]
    fn a_press_moved_one_flip_fails_at_anchor_a() {
        let reference = reference();
        let judge = anchor_a_judge();
        for press in [30, 32] {
            let moved = lob(0, press, 65, 120);
            let v = reference.check(&moved, &judge).unwrap();
            // The press and the throw it starts, each out by a frame, past
            // their 5 ms floors.
            let hero: Vec<&str> = v.actors["hero"]
                .mismatches
                .iter()
                .map(|m| match m {
                    LogMismatch::Stamp { kind, .. } => kind.as_str(),
                    other => panic!("{other:?}"),
                })
                .collect();
            assert_eq!(hero, ["press South", "throw"], "press at {press}");
            // On order alone, decision 18's first rule at anchor A, it passes.
            let first = Judge {
                gate: Gate::AllKinds,
                ..judge.clone()
            };
            assert!(reference.check(&moved, &first).unwrap().passed());
        }
    }

    #[test]
    fn anchor_as_floors_are_whole_quanta_with_the_press_under_a_frame() {
        let a = anchor_a::EVENTS;
        let all: Vec<f64> = a
            .kinds
            .iter()
            .map(|&(_, f)| f)
            .chain([a.press, a.default])
            .collect();
        for f in &all {
            let quanta = f / 0.005;
            assert!((quanta - quanta.round()).abs() < 1e-9 && *f > 0.0, "{f}");
        }
        assert_eq!(all.iter().copied().fold(0.0, f64::max), a.default);
        // A press or a throw a frame off can't hide in its floor.
        const { assert!(anchor_a::PRESS < 1.0 / 60.0 && anchor_a::THROW < 1.0 / 60.0) };
        let judge = a.judge(Some(0));
        for b in Button::ALL {
            assert_eq!(judge.floor(&press_kind(b)), anchor_a::PRESS);
        }
        assert_eq!(judge.floor("bite"), anchor_a::DEFAULT);
        for k in a.rolled {
            assert!(a.kinds.iter().any(|(n, _)| n == k), "{k}");
            assert!(judge.ordered.contains(*k));
        }
        assert_eq!(judge.gate, Gate::RolledKinds);
    }
}
