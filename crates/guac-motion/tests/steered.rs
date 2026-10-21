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
