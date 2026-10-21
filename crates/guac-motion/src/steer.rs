//! The engine's own steering for an actor no stick drives (openspec AH.18,
//! AH design decision 4): the per-frame velocity model every actor with
//! movement numbers runs, fed a destination by the scripts' `move to`, `move
//! from` and `turn to` instead of a stick. Norticus walks his waypoints this
//! way, and the ambient Chompies theirs; a creature its scripts register as a
//! mover (the Chompies of a fight) is steered by Giants' own movement macros
//! on the VM instead, and the engine only moves it ([`crate::hero::Hero::step_engine`]).
//!
//! As ag-motion read the EBOOT's model (its section 1, the velocity model and
//! the AI input), with the constants read off the binary, in metres as the
//! engine keeps the velocity (the movement numbers are feet, × 0.3048):
//!
//! 1. The input: with a destination, a heading to it across the ground (away
//!    from it for `move from`) and a strength of 1, unless it's within 0.05 m
//!    on both axes, when the heading is the facing; with none, no strength.
//!    `turn to` sets a facing to turn toward for the frame.
//! 2. Up in the air (last frame unsupported, or the walk leaving ground
//!    steeper than 0.69 or rising off it by more than 0.01 m/s) the vertical
//!    part is set aside and gravity takes it, down to the terminal speed; on
//!    the ground the walk is laid along it at its own length.
//! 3. The target facing is the input's heading when it has strength (not for
//!    the unconstrained move type); the facing turns toward it at the turn
//!    rate (air turn rate in the air), never past it.
//! 4. On foot, a walk within 45° of the facing is re-aimed along the facing
//!    and pushed; one further off isn't pushed (it turns in place).
//! 5. The deceleration always takes its share; at 0.001 m/s or less the walk
//!    stops. The acceleration pushes along the move heading on top, and the
//!    whole is cut to the larger of the top speed (× the input's strength)
//!    and what's left after the deceleration.
//!
//! What no stick drives has no controller, so the stick's own refinements
//! (the turn rate scaled by the stick, the top speed cut while turning, the
//! stop dead past the heading gate, the jump) never apply here.
//!
//! Hypotheses, each a reading the oracle hasn't settled:
//! - a `move to`'s destination is reached when the actor is within its
//!   distance of it across the ground, as the input steers (the op's own
//!   test isn't read), and the engine then has no destination; the aim
//!   lasts the tick a script asks for it (one that stops asking stops
//!   steering it);
//! - the walk is re-aimed along the facing at its own length, level, then
//!   laid along the ground (`FUN_00113ab8` and the ground projection after
//!   it, read by name, not instruction by instruction);
//! - the model runs on every actor it moves, steered or not, as the
//!   original's runs on every actor with movement numbers: AH.10's Chompies
//!   (no movers of the movement scripts') lunge at 33.7 ft/s from the 35
//!   their scripts set and slow through the recovery after it.

use crate::seam::Vec3;
use crate::tuning::FOOT;

/// How an actor moves (`actorphysics.move type`'s labels).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum MoveType {
    #[default]
    OnFoot,
    Wheeled,
    Unconstrained,
    Physical,
}

impl MoveType {
    /// From the member's enum value; anything past the labels is on foot.
    pub fn from_value(v: i32) -> MoveType {
        match v {
            1 => MoveType::Wheeled,
            2 => MoveType::Unconstrained,
            3 => MoveType::Physical,
            _ => MoveType::OnFoot,
        }
    }
}

/// An actor's movement numbers as its `actorphysics` members hold them: feet,
/// ft/s, ft/s², degrees a second.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Physics {
    pub ground_max_speed: f32,
    pub ground_acceleration: f32,
    pub ground_deceleration: f32,
    pub ground_turn_rate: f32,
    pub air_max_speed: f32,
    pub air_acceleration: f32,
    pub air_deceleration: f32,
    pub air_turn_rate: f32,
    pub gravity_acceleration: f32,
    pub gravity_max_speed: f32,
    pub move_type: MoveType,
}

impl Physics {
    /// A chapter 1 Chompy's, as its script sets them (`Set Actor Move
    /// Settings`: 20 ft/s, 160 up, 80 down, 720°/s, the same in the air; no
    /// gravity of its own, -1).
    pub const CHOMPY: Physics = Physics {
        ground_max_speed: 20.0,
        ground_acceleration: 160.0,
        ground_deceleration: 80.0,
        ground_turn_rate: 720.0,
        air_max_speed: 20.0,
        air_acceleration: 160.0,
        air_deceleration: 80.0,
        air_turn_rate: 720.0,
        gravity_acceleration: -1.0,
        gravity_max_speed: -1.0,
        move_type: MoveType::OnFoot,
    };
}

/// What the scripts last asked of the engine's steering.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub enum Aim {
    /// Nothing: the input has no strength.
    #[default]
    Still,
    /// `move to`: toward a point.
    To(Vec3),
    /// `move from`: away from a point.
    From(Vec3),
    /// `turn to`: a facing, degrees, to turn toward this frame.
    Face(f32),
}

/// Within this of the destination on both axes, the input's heading is the
/// facing (0.05 m, read off the binary).
pub const AT_DESTINATION: f32 = 0.05;
/// The ground a walk leaves: steeper than this normal's y (0.69).
pub const LEAVE_NORMAL: f32 = 0.69;
/// A strength, a speed or a rise over this counts (0.01).
pub const SOME: f32 = 0.01;
/// The walk stops at or under this speed, m/s.
pub const STOP: f32 = 0.001;
/// Off the facing by more than this, an on-foot walk isn't pushed (45°).
pub const HEADING_GATE: f32 = 45.0;

/// What a frame of the model reads and leaves: the walk in m/s, the facing
/// in degrees and whether the frame's walk is up in the air.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Frame {
    pub velocity: Vec3,
    pub facing: f32,
    pub airborne: bool,
}

/// `degrees` into [-180, 180).
fn wrap(degrees: f32) -> f32 {
    let d = (degrees + 180.0).rem_euclid(360.0) - 180.0;
    if d >= 180.0 { d - 360.0 } else { d }
}

fn along(heading: f32) -> Vec3 {
    let (s, c) = heading.to_radians().sin_cos();
    Vec3::new(s, 0.0, c)
}

/// The input an aim gives from `at` facing `facing`: its heading and
/// strength, and the facing to turn toward this frame.
pub fn input(aim: Aim, at: Vec3, facing: f32) -> (f32, f32, f32) {
    match aim {
        Aim::Still => (facing, 0.0, facing),
        Aim::Face(f) => (facing, 0.0, f),
        Aim::To(p) | Aim::From(p) => {
            let (mut dx, mut dz) = (p.x - at.x, p.z - at.z);
            if dx.abs() > AT_DESTINATION || dz.abs() > AT_DESTINATION {
                if matches!(aim, Aim::From(_)) {
                    dx = -dx;
                    dz = -dz;
                }
                let h = dx.atan2(dz).to_degrees();
                (h, 1.0, facing)
            } else {
                (facing, 1.0, facing)
            }
        }
    }
}

/// One frame of `dt` seconds of the engine's velocity model for an actor
/// with `physics` under `aim`, standing at `at` on ground with normal
/// `ground` (none when there's no ground found) and `supported` by it after
/// last frame's move.
pub fn step(
    frame: Frame,
    physics: &Physics,
    aim: Aim,
    at: Vec3,
    ground: Option<Vec3>,
    supported: bool,
    dt: f32,
) -> Frame {
    let mut v = frame.velocity;
    let g = -physics.gravity_acceleration * FOOT;
    let n = ground.unwrap_or(Vec3::Y);
    let (move_heading, strength, mut target) = input(aim, at, frame.facing);
    let mut airborne = !supported || frame.airborne;
    if !airborne {
        let rising = n.dot(Vec3::new(v.x, v.y + g * dt, v.z));
        airborne = n.y < LEAVE_NORMAL || rising > SOME;
    }
    let mut vertical = 0.0;
    let speed = if !airborne {
        let len = v.length();
        if len >= STOP {
            let along_n = (n.dot(v) / len).abs();
            if along_n <= 0.99999 && along_n > 1e-5 {
                v.y -= n.dot(v) / n.y;
                let l = v.length();
                if l >= STOP {
                    v *= len / l;
                }
            }
        }
        if len >= STOP { len } else { 0.0 }
    } else {
        let terminal = -physics.gravity_max_speed * FOOT;
        vertical = v.y;
        if terminal < v.y {
            vertical = (v.y + g * dt).max(terminal);
        }
        v.y = 0.0;
        v.length()
    };
    let max_speed = if airborne {
        physics.air_max_speed
    } else {
        physics.ground_max_speed
    } * FOOT;
    let (accel, decel, turn_rate) = if airborne {
        (
            physics.air_acceleration,
            physics.air_deceleration,
            physics.air_turn_rate,
        )
    } else {
        (
            physics.ground_acceleration,
            physics.ground_deceleration,
            physics.ground_turn_rate,
        )
    };
    if physics.move_type != MoveType::Unconstrained && strength > SOME {
        target = move_heading;
    }
    let mut facing = frame.facing;
    let turn = wrap(target - facing);
    if turn != 0.0 {
        let most = turn_rate * dt;
        facing = if most < turn.abs() {
            wrap(facing + most.copysign(turn))
        } else {
            wrap(target)
        };
    }
    let mut push = accel * FOOT;
    let mut heading = move_heading;
    let mut speed = speed;
    match physics.move_type {
        MoveType::Physical => {
            heading = facing;
            push = 0.0;
        }
        MoveType::Wheeled => heading = facing,
        MoveType::Unconstrained => {}
        MoveType::OnFoot => {
            if wrap(facing - move_heading).abs() <= HEADING_GATE {
                if speed > SOME && strength > SOME && push != 0.0 {
                    heading = facing;
                    let level = v.x.hypot(v.z).hypot(v.y);
                    v = along(facing) * level;
                    if !airborne {
                        v.y -= n.dot(v) / n.y.max(1e-3);
                        let l = v.length();
                        if l > 0.0 {
                            v *= level / l;
                        }
                    }
                    speed = v.length();
                }
            } else {
                push = 0.0;
            }
        }
    }
    let left = speed - decel * FOOT * dt;
    if left <= STOP {
        v = Vec3::ZERO;
    } else if v.length() > STOP {
        v *= left / v.length();
    }
    let left = left.max(0.0);
    if push != 0.0 && strength > SOME {
        let mut add = along(heading) * (push * dt);
        if !airborne {
            add.y -= n.dot(add) / n.y.max(1e-3);
        }
        v += add;
    }
    let mut cap = max_speed;
    if strength != 0.0 {
        cap *= strength;
    }
    let cap = cap.max(left);
    let len = v.length();
    if cap < len {
        v *= cap / len;
    }
    v.y += vertical;
    Frame {
        velocity: v,
        facing: facing.rem_euclid(360.0),
        airborne,
    }
}

/// Across the ground, as the input steers.
fn across(a: Vec3, b: Vec3) -> f32 {
    (a.x - b.x).hypot(a.z - b.z)
}

/// Whether `at` is within `within` feet of `point` across the ground (a
/// `move to` reached; a hypothesis, the module docs).
pub fn reached(at: Vec3, point: Vec3, within: f32) -> bool {
    across(at, point) <= within * FOOT
}

/// Whether `at` is `beyond` feet or more from `point` across the ground (a
/// `move from` done).
pub fn cleared(at: Vec3, point: Vec3, beyond: f32) -> bool {
    across(at, point) >= beyond * FOOT
}

#[cfg(test)]
mod tests {
    use super::*;

    const DT: f32 = 1.0 / 60.0;

    fn still(facing: f32) -> Frame {
        Frame {
            velocity: Vec3::ZERO,
            facing,
            airborne: false,
        }
    }

    #[test]
    fn an_aim_gives_a_heading_toward_or_away_and_none_at_the_point() {
        let at = Vec3::new(1.0, 0.0, 1.0);
        let (h, s, t) = input(Aim::To(Vec3::new(2.0, 5.0, 1.0)), at, 30.0);
        assert!((h - 90.0).abs() < 1e-4, "{h}");
        assert_eq!((s, t), (1.0, 30.0));
        let (h, ..) = input(Aim::From(Vec3::new(2.0, 0.0, 1.0)), at, 30.0);
        assert!((wrap(h) + 90.0).abs() < 1e-4, "{h}");
        // Within 5 cm on both axes: the facing, still at full strength.
        let (h, s, _) = input(Aim::To(Vec3::new(1.04, 0.0, 0.97)), at, 30.0);
        assert_eq!((h, s), (30.0, 1.0));
        assert_eq!(input(Aim::Still, at, 30.0), (30.0, 0.0, 30.0));
        assert_eq!(input(Aim::Face(120.0), at, 30.0), (30.0, 0.0, 120.0));
    }

    #[test]
    fn a_turn_takes_its_rate_a_frame_and_stops_on_the_target() {
        // 720°/s: 12° a frame, never past the target.
        let mut f = still(0.0);
        let mut seen = Vec::new();
        for _ in 0..10 {
            f = step(
                f,
                &Physics::CHOMPY,
                Aim::Face(50.0),
                Vec3::ZERO,
                Some(Vec3::Y),
                true,
                DT,
            );
            seen.push(f.facing);
        }
        assert!((seen[0] - 12.0).abs() < 1e-4, "{seen:?}");
        assert!((seen[3] - 48.0).abs() < 1e-4, "{seen:?}");
        assert!(
            seen[4..].iter().all(|&a| (a - 50.0).abs() < 1e-4),
            "{seen:?}"
        );
        // The short way round, through 0.
        let f = step(
            still(350.0),
            &Physics::CHOMPY,
            Aim::Face(10.0),
            Vec3::ZERO,
            Some(Vec3::Y),
            true,
            DT,
        );
        assert!((f.facing - 2.0).abs() < 1e-3, "{}", f.facing);
        assert_eq!(f.velocity, Vec3::ZERO, "a turn pushes nothing");
    }

    #[test]
    fn behind_it_turns_in_place_then_walks_up_to_its_top_speed() {
        // A destination behind: no push until the facing is within 45° of it.
        let goal = Vec3::new(0.0, 0.0, -20.0);
        let mut f = still(0.0);
        let mut frames = Vec::new();
        for _ in 0..60 {
            f = step(
                f,
                &Physics::CHOMPY,
                Aim::To(goal),
                Vec3::ZERO,
                Some(Vec3::Y),
                true,
                DT,
            );
            frames.push(f);
        }
        let first_push = frames
            .iter()
            .position(|f| f.velocity != Vec3::ZERO)
            .unwrap();
        // 180° at 12° a frame: within 45° after 12 frames.
        assert_eq!(first_push, 11, "{:?}", &frames[..14]);
        assert!(wrap(frames[first_push].facing - 180.0).abs() <= 45.0);
        // (160 - 80) ft/s² net: 1.333 ft/s a frame, capped at 20 ft/s.
        let top = frames
            .iter()
            .map(|f| f.velocity.length() / FOOT)
            .fold(0.0f32, f32::max);
        assert!((top - 20.0).abs() < 1e-3, "{top}");
        // The first push is a whole 160 x dt (the deceleration has nothing to
        // take at rest), then 1.333 ft/s a frame: 13 frames to 20.
        let at_cap = frames
            .iter()
            .position(|f| f.velocity.length() / FOOT > 19.999)
            .unwrap();
        assert_eq!(at_cap, first_push + 13);
    }

    #[test]
    fn let_go_it_slows_by_its_deceleration_and_stops() {
        let mut f = Frame {
            velocity: along(0.0) * (20.0 * FOOT),
            facing: 0.0,
            airborne: false,
        };
        let mut n = 0;
        while f.velocity != Vec3::ZERO {
            f = step(
                f,
                &Physics::CHOMPY,
                Aim::Still,
                Vec3::ZERO,
                Some(Vec3::Y),
                true,
                DT,
            );
            n += 1;
            assert!(n < 100);
        }
        // 20 ft/s at 80 ft/s²: 0.25 s, 15 frames.
        assert_eq!(n, 15);
    }

    #[test]
    fn up_in_the_air_gravity_takes_the_vertical_part() {
        let p = Physics {
            gravity_acceleration: 200.0,
            gravity_max_speed: 100.0,
            ..Physics::CHOMPY
        };
        let f = step(
            Frame {
                velocity: Vec3::new(0.0, 1.0, 0.0),
                facing: 0.0,
                airborne: true,
            },
            &p,
            Aim::Still,
            Vec3::ZERO,
            None,
            false,
            DT,
        );
        assert!(f.airborne);
        assert!(
            (f.velocity.y - (1.0 - 200.0 * FOOT * DT)).abs() < 1e-5,
            "{f:?}"
        );
        // At the terminal speed it falls no faster.
        let f = step(
            Frame {
                velocity: Vec3::new(0.0, -100.0 * FOOT, 0.0),
                facing: 0.0,
                airborne: true,
            },
            &p,
            Aim::Still,
            Vec3::ZERO,
            None,
            false,
            DT,
        );
        assert!((f.velocity.y + 100.0 * FOOT).abs() < 1e-4, "{f:?}");
    }

    #[test]
    fn on_a_slope_the_walk_is_laid_along_it_at_its_own_length() {
        let n = Vec3::new(0.0, 1.0, -0.5).normalize();
        let f = step(
            Frame {
                velocity: Vec3::new(0.0, 0.0, 3.0),
                facing: 0.0,
                airborne: false,
            },
            &Physics {
                ground_deceleration: 0.0,
                ..Physics::CHOMPY
            },
            Aim::Still,
            Vec3::ZERO,
            Some(n),
            true,
            DT,
        );
        assert!(f.velocity.dot(n).abs() < 1e-4, "{f:?}");
        assert!((f.velocity.length() - 3.0).abs() < 1e-4, "{f:?}");
    }

    #[test]
    fn reached_and_cleared_count_feet_across_the_ground() {
        let a = Vec3::ZERO;
        let b = Vec3::new(0.0, 9.0, 2.0 * FOOT);
        assert!(reached(a, b, 2.0));
        assert!(!reached(a, b, 1.9));
        assert!(cleared(a, b, 2.0));
        assert!(!cleared(a, b, 2.1));
        assert_eq!(MoveType::from_value(2), MoveType::Unconstrained);
        assert_eq!(MoveType::from_value(9), MoveType::OnFoot);
    }
}
