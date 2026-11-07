//! The engine's own steering on the parry twin (openspec AH.18): an actor no
//! stick drives, fed a destination, on open level ground of 2 m cells. A
//! Chompy-shaped body (its script's 6 x 3 x 3 ft sphere) with its script's
//! numbers (20 ft/s, 160 up, 80 down, 720°/s) walks to a point and from one;
//! creatures' A Chompy closes in, as far as the engine carries it.

use guac_motion::hero::Hero;
use guac_motion::parry::ParryWorld;
use guac_motion::proxy::Surfaces;
use guac_motion::scenarios;
use guac_motion::seam::Vec3;
use guac_motion::solid::{HavokSolid, SolidInfo, SolidKind};
use guac_motion::steer::{Aim, Physics, reached};
use guac_motion::tuning::FOOT;

const DT: f32 = 1.0 / 60.0;

fn chompy_solid() -> HavokSolid {
    SolidInfo {
        kind: SolidKind::Sphere,
        offset: [0.0; 3],
        rotation: [0.0; 3],
        width: 6.0,
        height: 3.0,
        length: 3.0,
    }
    .havok([100.0; 3])
    .expect("a sphere")
}

fn open() -> ParryWorld {
    ParryWorld::new(&scenarios::bodies(&scenarios::open_ground(), &[])).unwrap()
}

fn settled(world: &ParryWorld, at: Vec3, facing: f32) -> Hero {
    let mut c = Hero::actor(chompy_solid(), at, facing);
    c.settle(world, &[]);
    c
}

fn wrap(d: f32) -> f32 {
    (d + 180.0).rem_euclid(360.0) - 180.0
}

/// Steps it toward `aim` until `done` or `frames`, each frame's pivot,
/// facing and speed across the ground (ft/s).
fn walk(
    world: &ParryWorld,
    c: &mut Hero,
    aim: Aim,
    frames: usize,
    done: impl Fn(&Hero) -> bool,
) -> Vec<(Vec3, f32, f32)> {
    let mut out = Vec::new();
    for _ in 0..frames {
        if done(c) {
            break;
        }
        let before = c.pivot;
        c.step_steered(world, &Physics::CHOMPY, aim, DT, &[], Surfaces::NONE);
        let across = Vec3::new(c.pivot.x - before.x, 0.0, c.pivot.z - before.z).length();
        out.push((c.pivot, c.facing, across / DT / FOOT));
    }
    out
}

#[test]
fn a_chompy_reaches_a_destination_at_its_speed_and_turn_rate() {
    let world = open();
    let mut c = settled(&world, Vec3::new(0.0, 1.0, 0.0), 0.0);
    let y0 = c.pivot.y;
    // 10 m off to its right and behind: a turn first, then the walk.
    let goal = Vec3::new(8.0, 0.0, -6.0);
    let path = walk(&world, &mut c, Aim::To(goal), 300, |c| {
        reached(c.pivot, goal, 5.0)
    });
    assert!(reached(c.pivot, goal, 5.0), "{:?}", c.pivot);
    let frames = path.len();
    // 10 m less the 5 ft is 8.5 m, 84 frames at 20 ft/s; the turn to within
    // 45° (127° off, 7 frames) and the speeding up (15) add about 12.
    assert!((92..=100).contains(&frames), "{frames} frames");
    let top = path.iter().map(|p| p.2).fold(0.0f32, f32::max);
    assert!(top <= 20.0 + 1e-3, "never faster than 20 ft/s: {top}");
    assert!(top > 19.9, "and up to it: {top}");
    let mut facing = 0.0;
    for (k, p) in path.iter().enumerate() {
        let turned = wrap(p.1 - facing).abs();
        assert!(turned <= 12.0 + 1e-3, "frame {k} turned {turned}°");
        facing = p.1;
    }
    // Level ground: the walk never changes its height.
    assert!(path.iter().all(|p| (p.0.y - y0).abs() < 1e-3), "{y0}");
    assert!(c.footing.supported);
}

#[test]
fn let_go_at_the_destination_it_stops_within_a_quarter_second() {
    let world = open();
    let mut c = settled(&world, Vec3::new(0.0, 1.0, 0.0), 0.0);
    let goal = Vec3::new(0.0, 0.0, 6.0);
    walk(&world, &mut c, Aim::To(goal), 300, |c| {
        reached(c.pivot, goal, 2.0)
    });
    let path = walk(&world, &mut c, Aim::Still, 30, |_| false);
    let stopped = path.iter().position(|p| p.2 < 1e-3).expect("it stops");
    // 20 ft/s at 80 ft/s²: 15 frames.
    assert!(stopped <= 16, "{stopped}");
}

#[test]
fn moving_from_a_point_goes_away_from_it() {
    let world = open();
    let mut c = settled(&world, Vec3::new(0.0, 1.0, 0.0), 0.0);
    let from = Vec3::new(0.0, 0.0, -3.0);
    let start = (c.pivot - from).length();
    walk(&world, &mut c, Aim::From(from), 60, |_| false);
    assert!((c.pivot - from).length() > start + 5.0, "{:?}", c.pivot);
    assert!(wrap(c.facing).abs() < 1e-2, "facing away, {}", c.facing);
}

#[test]
fn a_turn_to_turns_it_in_place() {
    let world = open();
    let mut c = settled(&world, Vec3::new(0.0, 1.0, 0.0), 0.0);
    let at = c.pivot;
    let path = walk(&world, &mut c, Aim::Face(90.0), 10, |_| false);
    assert!((path[0].1 - 12.0).abs() < 1e-3, "{:?}", path[0]);
    assert!((c.facing - 90.0).abs() < 1e-3, "{}", c.facing);
    assert!((c.pivot - at).length() < 1e-4, "it doesn't walk");
}

/// Another Chompy's body standing at `at` (a kinematic sphere, as the
/// creatures' bodies are in the seam), on the open ground.
fn with_another(at: Vec3) -> ParryWorld {
    let solid = chompy_solid();
    let mut probe = Hero::actor(solid, at, 0.0);
    probe.settle(&open(), &[]);
    let pose = guac_motion::seam::Pose::new(probe.pivot, guac_motion::seam::Quat::IDENTITY);
    ParryWorld::new(&scenarios::bodies(
        &scenarios::open_ground(),
        &[(guac_motion::seam::BodyId(7), solid, pose)],
    ))
    .unwrap()
}

/// Steps it toward `aim` for `frames`, each frame's pivot, the walk it
/// keeps across the ground (ft/s) and the move it made across the ground
/// (ft/s).
fn walk_kept(world: &ParryWorld, c: &mut Hero, aim: Aim, frames: usize) -> Vec<(Vec3, f32, f32)> {
    (0..frames)
        .map(|_| {
            let before = c.pivot;
            c.step_steered(world, &Physics::CHOMPY, aim, DT, &[], Surfaces::NONE);
            let across = Vec3::new(c.pivot.x - before.x, 0.0, c.pivot.z - before.z).length();
            let kept = Vec3::new(c.walk.velocity.x, 0.0, c.walk.velocity.z).length();
            (c.pivot, kept, across / DT / FOOT)
        })
        .collect()
}

/// A Chompy whose destination lies through another's middle walks into its
/// body and stands there (openspec AH.232): pressed head-on it keeps none of
/// its walk, so what its scripts and its locomotion read is still, where it
/// had walked on the spot at 20 ft/s (AH.10's `still_p1`: a Chompy behind
/// another at 0.00 m/s for 120 frames in 'battle').
#[test]
fn aimed_through_anothers_middle_it_stands_against_its_body() {
    let other = Vec3::new(0.0, 1.0, 3.0);
    let world = with_another(other);
    let mut c = settled(&world, Vec3::new(0.0, 1.0, 0.0), 0.0);
    let path = walk_kept(&world, &mut c, Aim::To(Vec3::new(0.0, 0.0, 12.0)), 120);
    let touched = path
        .iter()
        .position(|p| p.2 < 0.5)
        .expect("it meets the other's body");
    assert!(touched > 5, "it walks there first: {touched}");
    let gap = Vec3::new(other.x - c.pivot.x, 0.0, other.z - c.pivot.z).length();
    assert!((1.6..1.9).contains(&gap), "against it: {gap} m");
    for (k, p) in path.iter().enumerate().skip(touched + 1) {
        assert!(p.2 < 0.05, "frame {k} moved {} ft/s", p.2);
        assert!(p.1 < 0.05, "frame {k} keeps a walk of {} ft/s", p.1);
    }
}

/// A Chompy whose destination lies through another's body off its middle
/// slides round it to its destination (openspec AH.232; AH.10's `stun-01`:
/// one walking round another at its full 20 ft/s, touching), every frame
/// keeping the walk it made: never walking faster than it moves.
#[test]
fn aimed_through_anothers_body_off_its_middle_it_gets_round() {
    let other = Vec3::new(0.7, 1.0, 3.0);
    let world = with_another(other);
    let mut c = settled(&world, Vec3::new(0.0, 1.0, 0.0), 0.0);
    let goal = Vec3::new(0.0, 0.0, 9.0);
    let path = walk_kept(&world, &mut c, Aim::To(goal), 150);
    // Up to speed, then slowed: its body met.
    let met = path
        .windows(2)
        .position(|w| w[0].2 > 19.99 && w[1].2 < 19.9)
        .expect("it meets the other's body")
        + 1;
    let past = path
        .iter()
        .position(|p| p.0.z > other.z + 1.0)
        .expect("it gets round");
    assert!(
        past - met < 60,
        "round it in a second: {} frames",
        past - met
    );
    for (k, p) in path.iter().enumerate() {
        assert!(
            p.1 <= p.2 + 0.05,
            "frame {k} keeps {} ft/s, moved {} ft/s",
            p.1,
            p.2
        );
    }
    assert!(reached(c.pivot, goal, 5.0), "{:?}", c.pivot);
}

/// A walk a Chompy's scripts set with no destination (a lunge) is theirs
/// against another's body as anywhere (openspec AH.232; AH.10's `still_p1`:
/// a lunging Chompy pressed against another held at 9.27 m/s for 12
/// frames): the engine only slows it by its deceleration, keeping none of
/// the move's solve.
#[test]
fn a_lunge_its_scripts_set_stays_theirs_against_a_body() {
    let other = Vec3::new(0.0, 1.0, 1.75);
    let world = with_another(other);
    let mut c = settled(&world, Vec3::new(0.0, 1.0, 0.0), 0.0);
    c.walk.velocity = Vec3::new(0.0, 0.0, 35.0);
    let before = c.pivot;
    c.step_steered(
        &world,
        &Physics::CHOMPY,
        Aim::Still,
        DT,
        &[],
        Surfaces::NONE,
    );
    let moved = (c.pivot - before).length() / DT / FOOT;
    assert!(moved < 5.0, "pressed against it: {moved} ft/s");
    let kept = c.walk.velocity.length();
    assert!(
        (kept - (35.0 - 80.0 / 60.0)).abs() < 1e-3,
        "kept {kept} ft/s"
    );
}

/// A stick pressing a hero into a body keeps the move's solve, as a
/// destination's does (openspec AU.37): the walk doesn't pile up behind it.
#[test]
fn a_stick_into_a_body_keeps_the_solve() {
    let other = Vec3::new(0.0, 1.0, 1.75);
    let world = with_another(other);
    let mut c = settled(&world, Vec3::new(0.0, 1.0, 0.0), 0.0);
    let along = Aim::Along {
        heading: 0.0,
        strength: 1.0,
    };
    for _ in 0..60 {
        c.step_steered(&world, &Physics::CHOMPY, along, DT, &[], Surfaces::NONE);
    }
    let kept = c.walk.velocity.length();
    assert!(kept < 3.0, "kept {kept} ft/s");
}

/// A fall that lands with no destination loses what pressed into the
/// ground (openspec AS.97): its speed across the ground is what it fell
/// with, where the frame after a landing turned the whole fall along the
/// ground (Setup_004B's lobber, off a 12 m ledge, skidding at 85 ft/s).
#[test]
fn a_fall_lands_without_turning_into_a_skid() {
    let world = open();
    let mut c = settled(&world, Vec3::new(0.0, 1.0, 0.0), 0.0);
    let floor = c.pivot.y;
    c.pivot.y += 12.0;
    c.footing.supported = false;
    c.walk.velocity = Vec3::new(0.0, -60.0, 5.0);
    let physics = Physics {
        air_deceleration: 0.0,
        ..Physics::CHOMPY
    };
    let mut fastest = 0.0_f32;
    for _ in 0..240 {
        let before = c.pivot;
        c.step_steered(&world, &physics, Aim::Still, DT, &[], Surfaces::NONE);
        let across = Vec3::new(c.pivot.x - before.x, 0.0, c.pivot.z - before.z).length();
        fastest = fastest.max(across / DT / FOOT);
    }
    assert!((c.pivot.y - floor).abs() < 0.05, "down: {}", c.pivot.y);
    assert!(fastest <= 5.01, "no skid: {fastest} ft/s");
}

/// A landing under `HARD_LANDING` keeps its walk as every steered fall did
/// before AS.97: the 30 ft/s a Chompy lands at off its pod, which the next
/// frame lays along the ground.
#[test]
fn a_landing_under_the_hard_landing_keeps_its_walk() {
    let world = open();
    let mut c = settled(&world, Vec3::new(0.0, 1.0, 0.0), 0.0);
    let floor = c.pivot.y;
    c.pivot.y += 0.2;
    c.footing.supported = false;
    c.walk.velocity = Vec3::new(0.0, -30.0, 5.0);
    let physics = Physics {
        air_deceleration: 0.0,
        ..Physics::CHOMPY
    };
    for _ in 0..60 {
        c.step_steered(&world, &physics, Aim::Still, DT, &[], Surfaces::NONE);
        if c.footing.supported {
            break;
        }
    }
    assert!(c.footing.supported && (c.pivot.y - floor).abs() < 0.05);
    assert!(c.walk.velocity.y < -25.0, "kept: {}", c.walk.velocity);
}

/// Moving from a point, a Chompy backed into another's body stands against
/// it as one walking to a point does (openspec AH.232: any destination's
/// walk is the move's, solved).
#[test]
fn moving_from_a_point_into_anothers_body_it_stands_against_it() {
    let other = Vec3::new(0.0, 1.0, 3.0);
    let world = with_another(other);
    let mut c = settled(&world, Vec3::new(0.0, 1.0, 0.0), 0.0);
    let path = walk_kept(&world, &mut c, Aim::From(Vec3::new(0.0, 0.0, -9.0)), 120);
    let touched = path
        .iter()
        .position(|p| p.2 < 0.5)
        .expect("it meets the other's body");
    for (k, p) in path.iter().enumerate().skip(touched + 1) {
        assert!(p.1 < 0.05, "frame {k} keeps a walk of {} ft/s", p.1);
    }
}
