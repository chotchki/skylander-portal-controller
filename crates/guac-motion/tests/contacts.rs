//! The hero controller against walls, solids, steps, drops and slopes
//! (openspec AG.16): hero-movement's Walls, Slopes, Steps and drops and He
//! leans with the ground, on parry and synthetic geometry, with AG.7's and
//! AG.9's numbers. Cases the oracle couldn't separate are named hypotheses
//! where they're tested.

mod common;

use common::*;
use guac_motion::seam::{BodyId, Pose, Vec3};
use guac_motion::solid::{SolidInfo, SolidKind, euler};
use guac_motion::stick::Stick;

/// The wall behind the start, square to the view's forward.
fn walled() -> Soup {
    let mut soup = open_ground();
    soup.wall_z(6.0, [-40.0, 40.0], [-1.0, 5.0], Vec3::NEG_Z);
    soup
}

/// Walks him from 3 m short of the wall at `degrees` from head-on, `s` of
/// the stick, for `frames` frames, the view facing +z so forward is
/// head-on.
fn into_the_wall(degrees: f32, s: f32, frames: usize) -> Vec<State> {
    let world = world(&walled(), &[]);
    let mut hero = eruptor(&world, Vec3::new(0.0, 1.0, 3.0), 0.0);
    (0..frames)
        .map(|_| {
            hero.step(
                &world,
                &guac_motion::hero::Input::stick(stick(s, -degrees), 0.0),
                DT,
                &[],
            );
            state(&hero)
        })
        .collect()
}

/// Walls: along the wall at his speed x 1 − 0.1235 (1 − sin θ), θ from
/// head-on, within 6 mm/s of the medians AG.9 measured at 1.99°, 5.04°,
/// 10.1°, 20°, 45° and 69.83° (5.7669, 5.8103, 5.8820, 6.0183, 6.3164 and
/// 6.5030-6.5037 m/s: w ran 0.1218-0.1250 per recording, 0.1235 their
/// middle), held 5 mm off it and never more than 1.1 mm higher than where he
/// met it.
#[test]
fn along_a_wall() {
    for (degrees, median) in [
        (1.99, 5.7669),
        (5.04, 5.8103),
        (10.1, 5.8820),
        (20.0, 6.0183),
        (45.0, 6.3164),
        (69.83, 6.5034),
    ] {
        let states = into_the_wall(degrees, 1.0, 90);
        let met = states
            .iter()
            .position(|s| s.pivot.z > 6.0 - 0.9144 - 0.0051)
            .unwrap();
        let sliding = &states[met + 2..];
        assert!(!sliding.is_empty(), "{degrees}");
        for s in sliding {
            let speed = ground_speed(s.velocity);
            assert!((speed - median).abs() < 6e-3, "{degrees}°: {speed}");
            assert!(s.velocity.z.abs() < 1e-4, "{degrees}°: {s:?}");
            let gap = 6.0 - (s.pivot.z + 0.9144);
            assert!((gap - 0.005).abs() < 2e-4, "{degrees}°: gap {gap}");
            assert!(
                s.pivot.y - states[met].pivot.y <= 1.1e-3,
                "{degrees}°: rose"
            );
        }
    }
}

/// Half stick along a wall: 45.7° from head-on, 0.965 of his speed.
#[test]
fn half_stick_along_a_wall() {
    let states = into_the_wall(45.7, 0.5, 120);
    let s = states.last().unwrap();
    let share = ground_speed(s.velocity) / (TOP * 0.5);
    assert!((share - 0.965).abs() < 1e-3, "{share}");
}

/// Head-on: he stops dead the frame he reaches the wall, from top speed,
/// 5 mm off its face, and stays while the stick holds; within 0.15° of
/// head-on too; at 1.06° he creeps along at the push's share, 0.12 m/s.
#[test]
fn head_on_he_stops_dead() {
    let stop = 6.0 - 0.9144 - 0.005;
    let states = into_the_wall(0.0, 1.0, 90);
    let met = states.iter().position(|s| s.pivot.z > stop - 1e-4).unwrap();
    assert!(
        (states[met].pivot.z - stop).abs() < 1e-4,
        "{:?}",
        states[met]
    );
    assert!((ground_speed(states[met - 1].velocity) - TOP).abs() < 1e-3);
    assert!(
        states[met + 1..]
            .iter()
            .all(|s| s.pivot == states[met].pivot)
    );
    for angle in [0.04, 0.15] {
        let states = into_the_wall(angle, 1.0, 90);
        assert_eq!(states.last().unwrap().velocity, Vec3::ZERO, "{angle}");
    }
    let creep = into_the_wall(1.06, 1.0, 90);
    let speed = ground_speed(creep.last().unwrap().velocity);
    assert!(
        (speed - TOP * 1.06f32.to_radians().sin()).abs() < 2e-3,
        "{speed}"
    );
}

/// Let go along a wall at 45°, he slows along it as on open ground, his
/// ground speed 0.9639 of his walk's (AG.9: stopped 0.81-0.84 m on).
#[test]
fn let_go_along_a_wall() {
    let world = world(&walled(), &[]);
    let mut hero = eruptor(&world, Vec3::new(0.0, 1.0, 4.5), 0.0);
    let input = |s| guac_motion::hero::Input::stick(s, 0.0);
    for _ in 0..30 {
        hero.step(&world, &input(stick(1.0, -45.0)), DT, &[]);
    }
    let released = hero.pivot;
    for _ in 0..40 {
        hero.step(&world, &input(Stick::CENTRED), DT, &[]);
        if hero.velocity != Vec3::ZERO {
            let share = ground_speed(hero.velocity) / (hero.walk.velocity.length() * 0.3048);
            assert!((share - 0.9639).abs() < 1e-3, "{share}");
        }
    }
    let slid = ground_speed(hero.pivot - released);
    assert!((0.78..=0.82).contains(&slid), "{slid}");
}

/// The gate's door, built from its CollisionInfo (4 x 21 x 34 ft, its own
/// quarter turn, heading 180): walked into head-on he stops with his pivot
/// 1.4781 m from its middle (AG.7) within the millimetre his 5 mm keep makes
/// of its 4.1 mm.
#[test]
fn the_gate_door_stops_him() {
    let door = SolidInfo {
        kind: SolidKind::Box,
        offset: [0.0; 3],
        rotation: [0.0, 90.0, 0.0],
        width: 4.0,
        height: 21.0,
        length: 34.0,
    }
    .havok([100.0; 3])
    .unwrap();
    let at = Pose::new(Vec3::new(0.0, 0.0, 5.0), euler([0.0, 180.0, 0.0]));
    let world = world(&open_ground(), &[(BodyId(2), door, at)]);
    let mut hero = eruptor(&world, Vec3::new(2.0, 1.0, 0.0), 0.0);
    let states = run(&world, &mut hero, 90, |_| stick(1.0, -VIEW));
    let s = states.last().unwrap();
    assert_eq!(s.velocity, Vec3::ZERO);
    let from_middle = 5.0 - s.pivot.z;
    assert!((from_middle - 1.4781).abs() < 1.5e-3, "{from_middle}");
    // Along x the door is 10.36 m wide: walked along it 4 m off, nothing.
    let mut past = eruptor(&world, Vec3::new(-4.0, 1.0, 3.5), 0.0);
    let along = run(&world, &mut past, 60, |_| stick(1.0, -VIEW - 90.0));
    assert!(
        along
            .iter()
            .all(|s| (ground_speed(s.velocity) - TOP).abs() < 1e-3 || s.velocity.x < 0.0)
    );
}

/// What he walks up in [`steps_up`].
fn riser(height: f32) -> Soup {
    let mut soup = Soup::default();
    soup.floor(0.0, [-20.0, 20.0], [-20.0, 3.0], 2.0);
    soup.floor(height, [-20.0, 20.0], [3.0, 20.0], 2.0);
    soup.wall_z(3.0, [-20.0, 20.0], [0.0, height], Vec3::NEG_Z);
    soup
}

fn walk_at(soup: &Soup, frames: usize) -> Vec<State> {
    let world = world(soup, &[]);
    let mut hero = eruptor(&world, Vec3::new(0.0, 1.0, 0.0), 0.0);
    (0..frames)
        .map(|_| {
            hero.step(
                &world,
                &guac_motion::hero::Input::stick(stick(1.0, 0.0), 0.0),
                DT,
                &[],
            );
            state(&hero)
        })
        .collect()
}

/// Steps: the highest riser AG.9 saw climbed, 4 cm, is; nothing between 4 cm
/// and 2.36 m lies in the opening area, so the 0.3 m riser here not being
/// climbed is a hypothesis (his sphere's rounded bottom meets a riser's edge
/// at 45° at 0.27 m), and 2.36 m isn't.
#[test]
fn steps_up() {
    let low = walk_at(&riser(0.04), 90);
    let top = low.last().unwrap();
    assert!(top.pivot.z > 6.0, "{top:?}");
    assert!((top.pivot.y - 0.04 - 0.005).abs() < 6e-3, "{top:?}");
    for height in [0.3, 2.36] {
        let states = walk_at(&riser(height), 90);
        let end = states.last().unwrap();
        assert!(end.pivot.z < 3.0 && end.pivot.y < 0.1, "{height}: {end:?}");
    }
}

/// What he walks off in [`drops_down`], its face shared with the floor's
/// edge as a level's cliff is.
fn drop(depth: f32) -> Soup {
    let mut soup = Soup::default();
    soup.floor(0.0, [-20.0, 20.0], [-20.0, 3.0], 2.0);
    soup.floor(-depth, [-20.0, 20.0], [3.0, 20.0], 2.0);
    soup.wall_z(3.0, [-20.0, 20.0], [-depth, 0.0], Vec3::Z);
    soup
}

/// Steps and drops: the deepest drop AG.9 saw him stay grounded over, 7 cm,
/// holds him (supported every frame after it but one at most, never up in
/// the air); the ledge route's 2 m and more he falls; 0.5 m, past the snap's
/// read 0.375 m reach, falls too (a hypothesis: nothing between 7 cm and 2.5
/// m in the opening area).
#[test]
fn drops_down() {
    let held = walk_at(&drop(0.07), 90);
    assert!(held.last().unwrap().pivot.z > 6.0);
    let unsupported = held.iter().filter(|s| !s.supported).count();
    assert!(unsupported <= 1, "{unsupported}");
    assert!(held.iter().all(|s| s.height.unwrap() < 0.4 * 0.3048));
    for depth in [0.5, 2.0] {
        let fell = walk_at(&drop(depth), 90);
        assert!(
            fell.iter().any(|s| s.height.unwrap() >= 0.4 * 0.3048),
            "{depth}"
        );
        assert!(fell.iter().any(|s| s.walk.y < -11.0), "{depth}: gravity");
        assert!(
            (fell.last().unwrap().pivot.y + depth - 0.005).abs() < 6e-3,
            "{depth}"
        );
    }
}

/// Slope tests' ground: level to z = 2, then rising toward +z.
fn ramp(degrees: f32) -> Soup {
    let mut soup = Soup::default();
    soup.floor(0.0, [-20.0, 20.0], [-20.0, 2.0], 2.0);
    soup.ramp(Vec3::new(0.0, 0.0, 2.0), degrees, [-20.0, 20.0], 12.0);
    soup
}

/// Slopes: a steep bank. Set on 50° ground he slides back to its foot and
/// never higher; 44.9° holds him once he's leaned into it and the snap has
/// set him (the cliff under the ledge route: held to 44.92°, not from
/// 45.04°).
#[test]
fn ground_steeper_than_45_doesnt_hold_him() {
    for (degrees, holds) in [(50.0, false), (44.9, true)] {
        let world = world(&ramp(degrees), &[]);
        let mut hero = eruptor(&world, Vec3::new(0.0, 8.0, 6.0), 0.0);
        let set = hero.pivot;
        let states = run(&world, &mut hero, 120, |_| Stick::CENTRED);
        assert!(
            states.iter().all(|s| s.pivot.y <= set.y + 1e-3),
            "{degrees}"
        );
        let end = states.last().unwrap();
        if holds {
            assert!(
                states[60..].iter().all(|s| s.pivot == end.pivot),
                "{degrees}"
            );
            assert!(end.supported && end.pivot.z > 4.0, "{degrees}: {end:?}");
        } else {
            // In the crease, his sphere on the floor against the bank's foot.
            assert!(
                end.pivot.z < 2.0 && end.pivot.y < 0.01,
                "{degrees}: {end:?}"
            );
        }
    }
}

/// Slopes and Steps and drops: up a 17.3° slope at full stick his speed
/// along it is his walking speed (AG.9: 6.553 m/s up the corridor), and he
/// rides it 10 mm up once the snap has set him there.
#[test]
fn up_a_slope_at_walking_speed() {
    let states = walk_at(&ramp(17.3), 140);
    let on: Vec<&State> = states
        .iter()
        .filter(|s| s.pivot.z > 4.0 && s.pivot.z < 12.0)
        .collect();
    assert!(on.len() > 30);
    for s in &on {
        assert!(
            (s.velocity.length() - TOP).abs() < 1e-3 * TOP * 11.0,
            "{s:?}"
        );
    }
    let settled = &on[10..];
    for s in settled {
        let h = s.height.unwrap();
        assert!((h - 0.01).abs() < 5e-4, "{s:?}");
    }
}

/// He leans with the ground: walking onto a 11.3° slope his up axis turns at
/// 120°/s, 2° a frame at 1/60 s, and stops within half a degree of it.
#[test]
fn he_leans_at_120_a_second() {
    let world = world(&ramp(11.3), &[]);
    let mut hero = eruptor(&world, Vec3::new(0.0, 1.0, 0.0), 0.0);
    let mut ups = vec![hero.up];
    for _ in 0..90 {
        hero.step(
            &world,
            &guac_motion::hero::Input::stick(stick(1.0, 0.0), 0.0),
            DT,
            &[],
        );
        ups.push(hero.up);
    }
    let turns: Vec<f32> = ups
        .windows(2)
        .map(|w| w[0].dot(w[1]).clamp(-1.0, 1.0).acos().to_degrees())
        .filter(|&d| d > 1e-3)
        .collect();
    assert!(turns.len() >= 5, "{turns:?}");
    assert!(turns.iter().all(|&d| d <= 2.0 + 1e-3), "{turns:?}");
    assert!(
        turns[..4].iter().all(|&d| (d - 2.0).abs() < 1e-2),
        "{turns:?}"
    );
    let slope = 11.3f32;
    let tilt = hero.up.y.clamp(-1.0, 1.0).acos().to_degrees();
    assert!((tilt - slope).abs() <= 0.5 + 1e-3, "{tilt}");
}

/// Steps and drops: snapped back down. Lifted 189 mm over level ground he
/// stood on, he's put back 10 mm over it in one frame.
#[test]
fn snapped_back_down_from_189_mm() {
    let world = world(&open_ground(), &[]);
    let mut hero = eruptor(&world, Vec3::new(0.0, 1.0, 0.0), 0.0);
    hero.pivot.y += 0.189 - 0.005;
    let after = run(&world, &mut hero, 1, |_| Stick::CENTRED)[0];
    assert!((after.pivot.y - 0.01).abs() < 1e-5, "{after:?}");
}
