//! The edge probe in the controller (openspec AG.16 on AG.36 and AG.39):
//! hero-movement's An edge with nothing below, on parry and synthetic
//! platforms. AG.36's stops are measured from the oracle's own steps; at a
//! steady 1/60 s the probe's 1.2105 m reach and his 0.109 m frames put every
//! stop 1.10-1.21 m short, where AG.36 saw 1.11-1.21 m.

mod common;

use common::*;
use guac_motion::edge::FAST_REACH;
use guac_motion::hero::Input;
use guac_motion::seam::Vec3;
use guac_motion::stick::Stick;
use guac_motion::tuning::FOOT;

const EDGE: f32 = 5.0;

/// A platform ending at z = 5 with nothing under its edge.
fn platform() -> Soup {
    let mut soup = Soup::default();
    soup.floor(0.0, [-20.0, 20.0], [-20.0, EDGE], 1.0);
    soup
}

fn walk(soup: &Soup, from: Vec3, frames: usize, stick_at: impl Fn(usize) -> Stick) -> Vec<State> {
    let world = world(soup, &[]);
    let mut hero = eruptor(&world, from, 0.0);
    (0..frames)
        .map(|k| {
            hero.step(&world, &Input::stick(stick_at(k), 0.0), DT, &[]);
            state(&hero)
        })
        .collect()
}

/// Where a walk stopped dead: the frame after its last moving one.
fn stop(states: &[State]) -> usize {
    states
        .windows(2)
        .position(|w| w[0].velocity != Vec3::ZERO && w[1].velocity == Vec3::ZERO)
        .unwrap()
        + 1
}

/// Head-on at full stick: he stops dead from top speed the frame the point
/// 1.2105 m ahead along his walk is over nothing, supported, and stays while
/// the stick holds and once it's let go.
#[test]
fn head_on_he_stops_dead_short_of_it() {
    let states = walk(&platform(), Vec3::new(0.0, 1.0, -5.0), 150, |k| {
        if k < 120 {
            stick(1.0, 0.0)
        } else {
            Stick::CENTRED
        }
    });
    let at = stop(&states);
    assert!((ground_speed(states[at - 1].velocity) - TOP).abs() < 1e-3);
    let left = EDGE - states[at].pivot.z;
    assert!(
        (FAST_REACH - TOP * DT..FAST_REACH).contains(&left),
        "{left}"
    );
    assert!(
        states[at..]
            .iter()
            .all(|s| s.pivot == states[at].pivot && s.supported)
    );
}

/// Along an edge, 30° and 82° from head-on: he stops dead with the edge
/// 1.2105 m ahead along his walk at most, his pivot 0.97-1.05 and 0.14-0.17 m
/// from it, and never slides along it.
#[test]
fn along_an_edge_he_never_slides() {
    for (degrees, pivot_off, from) in [(30.0, 0.95..=1.05, 1.0), (82.0, 0.14..=0.17, 3.5)] {
        let states = walk(&platform(), Vec3::new(0.0, 1.0, from), 300, |_| {
            stick(1.0, -degrees)
        });
        let at = stop(&states);
        let off = EDGE - states[at].pivot.z;
        assert!(pivot_off.contains(&off), "{degrees}°: {off}");
        let ahead = off / degrees.to_radians().cos();
        assert!(
            ahead < FAST_REACH && ahead > FAST_REACH - TOP * DT,
            "{degrees}°: {ahead}"
        );
        assert!(
            states[at..].iter().all(|s| s.pivot == states[at].pivot),
            "{degrees}°"
        );
    }
}

/// Let go short of it: let go 1.6 m short at full stick, his slide stops
/// dead from 4.6-5.8 m/s with 1.10-1.21 m left (AG.36: 1.16-1.20 m from
/// 4.6-5.8 m/s at its steps).
#[test]
fn let_go_short_of_it() {
    let soup = platform();
    let world = world(&soup, &[]);
    let mut hero = eruptor(&world, Vec3::new(0.0, 1.0, -5.0), 0.0);
    while hero.pivot.z < EDGE - 1.6 {
        hero.step(&world, &Input::stick(stick(1.0, 0.0), 0.0), DT, &[]);
    }
    let states: Vec<State> = (0..60)
        .map(|_| {
            hero.step(&world, &Input::stick(Stick::CENTRED, 0.0), DT, &[]);
            state(&hero)
        })
        .collect();
    let at = stop(&states);
    let from = ground_speed(states[at - 1].velocity);
    assert!((4.6..=5.8).contains(&from), "{from}");
    let left = EDGE - states[at].pivot.z;
    assert!(
        (FAST_REACH - TOP * DT..FAST_REACH).contains(&left),
        "{left}"
    );
    assert!(states[at..].iter().all(|s| s.pivot == states[at].pivot));
}

/// Part stick: at half and 0.6 of the stick he stops dead short, then creeps
/// on a step every third frame and stops for good inside half his width and
/// a foot (3 ft, 0.914 m) of it; at 0.088 he creeps straight to there and
/// stops; at 0.9 he stays at the first stop.
#[test]
fn part_stick_creeps_on_to_three_feet() {
    let slow = 3.0 * FOOT;
    for s in [0.5, 0.6] {
        let states = walk(&platform(), Vec3::new(0.0, 1.0, -2.0), 300, |_| {
            stick(s, 0.0)
        });
        let first = stop(&states);
        let short = EDGE - states[first].pivot.z;
        assert!(
            (FAST_REACH - TOP * s * DT..FAST_REACH).contains(&short),
            "{s}: {short}"
        );
        let moves: Vec<usize> = (first..states.len())
            .filter(|&k| states[k].velocity != Vec3::ZERO)
            .collect();
        assert!(moves.len() > 5, "{s}: {moves:?}");
        assert!(moves.windows(2).all(|w| w[1] - w[0] == 3), "{s}: {moves:?}");
        let end = EDGE - states.last().unwrap().pivot.z;
        assert!(end < slow && end > slow - 0.01, "{s}: {end}");
    }
    let creep = walk(&platform(), Vec3::new(0.0, 1.0, 3.0), 400, |_| {
        stick(0.088, 0.0)
    });
    let end = EDGE - creep.last().unwrap().pivot.z;
    assert!(end < slow && end > slow - 0.01, "creep: {end}");
    let held = walk(&platform(), Vec3::new(0.0, 1.0, -2.0), 200, |_| {
        stick(0.9, 0.0)
    });
    let first = stop(&held);
    assert!(held[first..].iter().all(|s| s.pivot == held[first].pivot));
}

/// Off the ledge route: where ground lies within 225 ft below, he walks off
/// at top speed without slowing and falls.
#[test]
fn ground_below_lets_him_walk_off() {
    let mut soup = platform();
    soup.floor(-3.0, [-20.0, 20.0], [EDGE, 30.0], 1.0);
    soup.wall_z(EDGE, [-20.0, 20.0], [-3.0, 0.0], Vec3::Z);
    let states = walk(&soup, Vec3::new(0.0, 1.0, -5.0), 160, |_| stick(1.0, 0.0));
    let over = states.iter().position(|s| s.pivot.z > EDGE).unwrap();
    for s in &states[20..over + 5] {
        assert!((ground_speed(s.velocity) - TOP).abs() < 1e-3, "{s:?}");
    }
    assert!(states.iter().any(|s| s.walk.y < -40.0), "fell");
    let end = states.last().unwrap();
    assert!(
        (end.pivot.y + 3.0 - 0.005).abs() < 6e-3 && end.pivot.z > EDGE + 3.0,
        "{end:?}"
    );
    assert!(
        (ground_speed(end.velocity) - TOP).abs() < 1e-3,
        "walks on: {end:?}"
    );
}

/// The probe runs in the air too (AG.39's `g39_b45_in`, where it stopped him
/// with his pivot already over the edge): walked off an edge with ground
/// below, he's stopped once his point comes over nothing, his walk across
/// the ground zeroed before he moves, and drops straight down with it held
/// at 0 while the point stays over nothing.
#[test]
fn the_probe_stops_him_in_the_air() {
    // The ledge at x = 0; under it ground for z < 0 only.
    let mut soup = Soup::default();
    soup.floor(0.0, [-20.0, 0.0], [-20.0, 20.0], 1.0);
    soup.floor(-3.0, [0.0, 20.0], [-20.0, 0.0], 1.0);
    soup.wall_x(0.0, [-20.0, 0.0], [-3.0, 0.0], Vec3::X);
    let world = world(&soup, &[]);
    // Heading 70° (mostly +x): at the ledge his point is over the ground
    // below; 1.6 m past it, over nothing.
    let mut hero = eruptor(&world, Vec3::new(-3.0, 1.0, -2.09), 0.0);
    let held = |k: usize| if k < 34 { 70.0 } else { 0.0 };
    let states: Vec<State> = (0..60)
        .map(|k| {
            hero.step(&world, &Input::stick(stick(1.0, held(k)), 0.0), DT, &[]);
            state(&hero)
        })
        .collect();
    let refused = states.iter().position(|s| s.held_off).unwrap();
    let r = states[refused];
    assert!(r.pivot.x > 0.0 && !r.supported, "{r:?}");
    assert_eq!(r.walk.x, 0.0);
    assert_eq!(r.walk.z, 0.0);
    let before = states[refused - 1];
    assert!((r.pivot.x - before.pivot.x).abs() < 1e-6 && r.pivot.y < before.pivot.y);
    // Turned toward +z after it, his point stays over nothing: held, he
    // drops straight down.
    for s in states[refused..].iter().filter(|s| s.held_off) {
        assert!((s.pivot.x - r.pivot.x).abs() < 1e-5 && (s.pivot.z - r.pivot.z).abs() < 1e-5);
    }
}
