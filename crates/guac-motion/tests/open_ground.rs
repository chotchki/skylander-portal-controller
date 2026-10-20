//! The hero controller on open level ground (openspec AG.16): hero-movement's
//! The stick walks the hero as seen from the camera, Top speed, Speeding up
//! and slowing down, Turning and hard reversals, No jumping and The same walk
//! at any frame rate, on parry and a synthetic floor of 2 m cells. Numbers are
//! AG.8's, at its own 1/60 s predictions where its recordings' steps vary.

mod common;

use common::*;
use guac_motion::clock::FixedClock;
use guac_motion::hero::Input;
use guac_motion::seam::Vec3;
use guac_motion::stick::Stick;

const START: Vec3 = Vec3::new(0.0, 1.0, 0.0);

/// Settled, he rides 5 mm over level ground, supported (AF.9's idle pivot
/// 5.0 mm over the ground at the spawn).
#[test]
fn settled_he_rides_5_mm_up() {
    let world = world(&open_ground(), &[]);
    let hero = eruptor(&world, START, 0.0);
    assert!((hero.pivot.y - 0.005).abs() < 1e-4, "{}", hero.pivot.y);
    assert!(hero.footing.supported);
    let mut hero = hero;
    let after = run(&world, &mut hero, 60, |_| Stick::CENTRED);
    assert!(after.iter().all(|s| s.pivot == after[0].pivot));
}

/// Top speed: 6.553 m each game second within 1 mm at full stick, 21.5 ft/s x
/// the strength at part stick, on level ground.
#[test]
fn top_speed_is_21_5_ft_s_times_the_strength() {
    let world = world(&open_ground(), &[]);
    for s in [1.0, 0.5, 0.2] {
        let mut hero = eruptor(&world, START, 0.0);
        let states = run(&world, &mut hero, 120, |_| stick(s, 0.0));
        let (a, b) = (states[59].pivot, states[119].pivot);
        let metres = ground_speed(b - a);
        assert!((metres - TOP * s).abs() < 1e-3, "S {s}: {metres}");
        assert!(
            states[60..]
                .iter()
                .all(|st| st.pivot.y == states[59].pivot.y)
        );
    }
}

/// Speeding up and slowing down, from rest: 90 % of top from the step that
/// starts 0.1167 s in, 2.8905 m in the first half second (AG.8's 1/60 s
/// predictions, inside its 0.115-0.119 s and 2.88-2.90 m); let go at top
/// speed, 0.826 m in 0.267 s, then still.
#[test]
fn from_rest_and_letting_go() {
    let world = world(&open_ground(), &[]);
    let mut hero = eruptor(&world, START, 15.0);
    let start = hero.pivot;
    let states = run(&world, &mut hero, 30, |_| stick(1.0, 0.0));
    let first = states
        .iter()
        .position(|s| ground_speed(s.velocity) >= 0.9 * TOP)
        .unwrap();
    assert!((first as f32 * DT - 0.1167).abs() < 1e-3, "{first}");
    let half = ground_speed(states[29].pivot - start);
    assert!((half - 2.8905).abs() < 1e-3, "{half}");

    let released = hero.pivot;
    let stop = run(&world, &mut hero, 90, |_| Stick::CENTRED);
    let moving = stop.iter().filter(|s| s.velocity != Vec3::ZERO).count();
    assert_eq!(moving, 16, "0.2667 s at 1/60");
    let slid = ground_speed(stop[89].pivot - released);
    assert!((slid - 0.8263).abs() < 1e-3, "{slid}");
    assert!((0.81..=0.85).contains(&slid));
    assert!(stop[16..].iter().all(|s| s.pivot == stop[16].pivot));
}

/// The stick walks the hero: forward at the start walks him along the view's
/// 15° from his first frame, facing it; left along 105°, his facing reaching
/// it in the fifth frame.
#[test]
fn the_stick_walks_him_against_the_view() {
    let world = world(&open_ground(), &[]);
    for (degrees, want, facing_by) in [(0.0, 15.0, 1), (90.0, 105.0, 5)] {
        let mut hero = eruptor(&world, START, 0.0);
        let states = run(&world, &mut hero, 6, |_| stick(1.0, degrees));
        for s in &states {
            assert!(
                angle_between(bearing(s.velocity), want) < 0.01,
                "{degrees}: {s:?}"
            );
        }
        let reached = states.iter().position(|s| s.facing == want).unwrap() + 1;
        assert_eq!(reached, facing_by, "{degrees}");
    }
}

/// From rest at the start: no gate at 15° from his facing. Either side of it
/// by 0.01° the first frame runs the same law: the stick's way at 160 ft/s² x
/// the step, his facing on it.
#[test]
fn no_gate_either_side_of_15_degrees() {
    let world = world(&open_ground(), &[]);
    let first = |off: f32| {
        let mut hero = eruptor(&world, START, VIEW - off);
        run(&world, &mut hero, 1, |_| stick(1.0, 0.0))[0]
    };
    let (under, over) = (first(15.0 - 0.01), first(15.0 + 0.01));
    for s in [under, over] {
        assert!(
            (ground_speed(s.velocity) - 160.0 * 0.3048 * DT).abs() < 1e-4,
            "{s:?}"
        );
        assert!(angle_between(bearing(s.velocity), VIEW) < 0.01);
        assert!((s.facing - VIEW).abs() < 1e-3);
    }
    assert!((ground_speed(under.velocity) - ground_speed(over.velocity)).abs() < 1e-6);
}

/// A full reversal: from the next frame he walks back at top speed, never
/// slower, his facing round in 0.13-0.15 s.
#[test]
fn a_full_reversal_goes_straight_back() {
    let world = world(&open_ground(), &[]);
    let mut hero = eruptor(&world, START, VIEW);
    run(&world, &mut hero, 30, |_| stick(1.0, 0.0));
    let before = hero.velocity;
    let back = run(&world, &mut hero, 30, |_| stick(1.0, 180.0));
    assert!((back[0].velocity + before).length() < 1e-3, "{:?}", back[0]);
    assert!(
        back.iter()
            .all(|s| (ground_speed(s.velocity) - TOP).abs() < 1e-3)
    );
    let round = back
        .iter()
        .position(|s| angle_between(s.facing, VIEW + 180.0) < 1e-3)
        .unwrap()
        + 1;
    let seconds = round as f32 * DT;
    assert!((0.13..=0.15).contains(&seconds), "{seconds}");
}

/// Part stick: his facing turns at 720°/s at half stick, while he walks the
/// stick's way at once.
#[test]
fn half_stick_turns_at_720() {
    let world = world(&open_ground(), &[]);
    let mut hero = eruptor(&world, START, VIEW);
    run(&world, &mut hero, 30, |_| stick(0.5, 0.0));
    let turn = run(&world, &mut hero, 3, |_| stick(0.5, 90.0));
    assert!(
        (turn[0].facing - (VIEW + 12.0)).abs() < 1e-3,
        "{:?}",
        turn[0]
    );
    assert!((turn[1].facing - (VIEW + 24.0)).abs() < 1e-3);
    assert!(angle_between(bearing(turn[0].velocity), VIEW + 90.0) < 0.01);
}

/// Let go mid-turn: swung 90° at top speed and let go a frame later, he slows
/// along that one frame's stick and his facing stays where it left it.
#[test]
fn let_go_mid_turn_freezes_his_facing() {
    let world = world(&open_ground(), &[]);
    let mut hero = eruptor(&world, START, VIEW);
    run(&world, &mut hero, 30, |_| stick(1.0, 0.0));
    let swung = run(&world, &mut hero, 1, |_| stick(1.0, 90.0))[0];
    assert!((swung.facing - (VIEW + 24.0)).abs() < 1e-3);
    let slowing = run(&world, &mut hero, 40, |_| Stick::CENTRED);
    for s in slowing.iter().filter(|s| s.velocity != Vec3::ZERO) {
        assert!(
            angle_between(bearing(s.velocity), VIEW + 90.0) < 0.01,
            "{s:?}"
        );
    }
    assert!(slowing.iter().all(|s| s.facing == swung.facing));
    assert!(slowing.last().unwrap().velocity == Vec3::ZERO);
}

/// No jumping: the face buttons change nothing, frame for frame.
#[test]
fn no_button_lifts_him() {
    let world = world(&open_ground(), &[]);
    let walk = |buttons: [bool; 4]| {
        let mut hero = eruptor(&world, START, VIEW);
        (0..60)
            .map(|k| {
                let input = Input {
                    stick: stick(1.0, if k < 30 { 0.0 } else { 45.0 }),
                    view_heading: VIEW,
                    buttons,
                };
                hero.step(&world, &input, DT, &[]);
                (hero.pivot, hero.velocity)
            })
            .collect::<Vec<_>>()
    };
    let none = walk([false; 4]);
    assert_eq!(none, walk([true, false, false, false]));
    assert_eq!(none, walk([true; 4]));
    assert!(none.iter().all(|(p, v)| p.y == none[0].0.y && v.y == 0.0));
}

/// The same walk at any frame rate: the same stick, held for the first
/// second of game time and let go, leaves him in the same place after 2 s
/// whether the display runs at 30, 60 or 144 Hz.
#[test]
fn the_same_path_at_30_60_and_144_hz() {
    let world = world(&open_ground(), &[]);
    let ends: Vec<Vec3> = [30.0, 60.0, 144.0]
        .iter()
        .map(|rate| {
            let mut hero = eruptor(&world, START, VIEW);
            let mut clock = FixedClock::default();
            let mut steps = 0;
            for _ in 0..(*rate as usize * 2) {
                for _ in 0..clock.advance(1.0 / rate) {
                    let stick = if steps < 60 {
                        stick(1.0, 30.0)
                    } else {
                        Stick::CENTRED
                    };
                    hero.step(&world, &Input::stick(stick, VIEW), clock.step(), &[]);
                    steps += 1;
                }
            }
            assert_eq!(steps, 120, "{rate} Hz");
            hero.pivot
        })
        .collect();
    assert_eq!(ends[0], ends[1]);
    assert_eq!(ends[1], ends[2]);
}
