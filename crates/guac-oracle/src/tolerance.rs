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
//! apart. Inside a usable horizon read at these floors the oracle agrees with
//! itself by construction, so a candidate fails there only by straying past
//! the oracle's own noise.
//!
//! Not set: the event-sequence and totals floors ([`crate::events`],
//! [`crate::totals`]). AF observes no game events and no XP or gold, so they
//! wait for the first rung that does (AH's combat) to calibrate them the same
//! way.

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

/// Counts: MT draws and animation LCG steps since F0, actors listed and
/// skinned, game frames since F0. A count agrees exactly or not, and AF.11's
/// RNG agreed only at F0 (identical MT states, design decision 9).
pub const COUNT: TrajectoryTolerance = TrajectoryTolerance { floor: 0.0, k: K };

/// The frame comparison's floor over the largest mean FLIP between two
/// replays' frames at a checkpoint ([`crate::frames::FrameBand::check`]),
/// gated inside the hero's and the camera's horizons. At floor 0 the two
/// replays of a checkpoint's largest pair fail their own leave-one-out, so
/// this is the largest lead any replay had there over the others' band,
/// rounded up: 0.0262 (AF.11's idle set A at F0 + 1,800, the replay that ran
/// slow); AF.12's walk needed 0.0199 at most (F0 + 120).
pub const FRAME_FLOOR: f64 = 0.03;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_defaults_are_tolerances_the_metrics_take() {
        for t in [POSITION, ANGLE, COUNT] {
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
}
