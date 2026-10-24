//! The hero controller against walls, solids, steps, drops and slopes
//! (openspec AG.16): hero-movement's Walls, Slopes, Steps and drops and He
//! leans with the ground, on parry and synthetic geometry, with AG.7's and
//! AG.9's numbers. Cases the oracle couldn't separate are named hypotheses
//! where they're tested.

mod common;

use common::*;
use guac_motion::hero::Hero;
use guac_motion::scenarios;
use guac_motion::seam::{BodyId, Pose, Vec3};
use guac_motion::stick::Stick;
use guac_motion::tuning::Tuning;

/// The wall cases live in `guac_motion::scenarios`, so guacamole runs them
/// on Avian too.
#[test]
fn along_a_wall() {
    on_parry(scenarios::along_a_wall());
}

#[test]
fn half_stick_along_a_wall() {
    on_parry(scenarios::half_stick_along_a_wall());
}

#[test]
fn head_on_he_stops_dead() {
    on_parry(scenarios::head_on_he_stops_dead());
}

#[test]
fn put_down_under_an_overhang_he_stays() {
    on_parry(scenarios::put_down_under_an_overhang_he_stays());
}

#[test]
fn let_go_along_a_wall() {
    on_parry(scenarios::let_go_along_a_wall());
}

#[test]
fn the_gate_door_stops_him() {
    on_parry(scenarios::the_gate_door_stops_him());
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
/// never higher; 44.9° holds him leaned into it, as the cliff under the
/// ledge route held theirs (to 44.92°, not from 45.04°). Upright on it his
/// pivot rides 0.38 m over the slope, past the snap's reach, where his lean
/// turns back toward upright as off the ground (AG.71), so he's set there
/// leaned.
#[test]
fn ground_steeper_than_45_doesnt_hold_him() {
    for (degrees, holds) in [(50.0, false), (44.9, true)] {
        let world = world(&ramp(degrees), &[]);
        let a = f32::to_radians(degrees);
        let mut hero = Hero::new(
            eruptor_solid(),
            Tuning::ERUPTOR,
            Vec3::new(0.0, 8.0, 6.0),
            0.0,
        );
        if holds {
            hero.up = Vec3::new(0.0, a.cos(), -a.sin());
        }
        hero.settle(&world, &[]);
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

/// Steps and drops over a convex crest: walked at full stick off level
/// ground onto a face falling away at 50°, 44°, 30° and 17.3°, he never rises
/// past the ground he left. The snap sets him down, never up: the face's
/// plane runs on under him past the crest, and setting him clear of it put
/// him 0.34, 0.22 and 0.06 m up (AG.16's review).
#[test]
fn over_a_crest_he_never_rises() {
    for degrees in [50.0f32, 44.0, 30.0, 17.3] {
        let mut soup = Soup::default();
        soup.floor(0.0, [-20.0, 20.0], [-20.0, 3.0], 2.0);
        soup.ramp(Vec3::new(0.0, 0.0, 3.0), -degrees, [-20.0, 20.0], 12.0);
        let states = walk_at(&soup, 45);
        let top = states.iter().map(|s| s.pivot.y).fold(f32::MIN, f32::max);
        assert!(top <= 0.005 + 1e-5, "{degrees}°: {top}");
        let end = states.last().unwrap();
        assert!(
            end.pivot.z > 4.0 && end.pivot.y < -0.1,
            "{degrees}°: {end:?}"
        );
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
/// stood on, he's put back 10 mm over it in one frame on a frame his lean
/// turns (AG.9's corridor: from 171 and 189 mm, each as the ground's normal
/// under him changed 8.6° and his lean turned 1.8-2.0°), and with his lean
/// holding he stays there, supported (AG.70: the snap runs inside the
/// post-step's lean).
#[test]
fn snapped_back_down_from_189_mm() {
    let world = world(&open_ground(), &[]);
    let mut hero = eruptor(&world, Vec3::new(0.0, 1.0, 0.0), 0.0);
    hero.pivot.y += 0.189 - 0.005;
    let mut leaned = hero.clone();
    let held = run(&world, &mut hero, 3, |_| Stick::CENTRED);
    assert!(
        held.iter()
            .all(|s| s.supported && (s.pivot.y - 0.189).abs() < 1e-5),
        "{held:?}"
    );
    let a = 2f32.to_radians();
    leaned.up = Vec3::new(a.sin(), a.cos(), 0.0);
    let after = run(&world, &mut leaned, 1, |_| Stick::CENTRED)[0];
    assert!((after.pivot.y - 0.01).abs() < 1e-5, "{after:?}");
}

/// A moving solid carries him along its normal at its own speed (the
/// surface's velocity in the solve, kept by whoever moves it): a 2 m box
/// coming at him across level ground at 1 m/s, moved after each of his
/// steps, pushes him back at 1 m/s once it reaches him, its face 5 mm off
/// his shape each time it has moved and never into it (the frame it first
/// reaches him it ends 2.3 mm off, inside the keep, as it moved after he
/// did).
#[test]
fn a_moving_solid_pushes_him_at_its_speed() {
    use guac_motion::proxy::Surfaces;
    use guac_motion::seam::{Body, BodyKind, Collider, PhysicsSeam, Shape};
    let block = BodyId(2);
    let half = 1.0;
    let mut at = Vec3::new(0.0, half, 3.0);
    let mut bodies = vec![level(&open_ground())];
    bodies.push(Body {
        id: block,
        kind: BodyKind::Kinematic,
        pose: Pose::at(at),
        collider: Collider::Convex(Shape::Box {
            half_extents: Vec3::splat(half),
            margin: 0.0,
        }),
    });
    let mut world = guac_motion::parry::ParryWorld::new(&bodies).unwrap();
    let mut hero = eruptor(&world, Vec3::new(0.0, 1.0, 0.0), 0.0);
    let mut surfaces = Surfaces::default();
    let towards = Vec3::new(0.0, 0.0, -1.0);
    let mut gaps = Vec::new();
    for _ in 0..180 {
        hero.step_among(
            &world,
            &guac_motion::hero::Input::stick(Stick::CENTRED, 0.0),
            DT,
            &[],
            &surfaces,
        );
        at += towards * DT;
        world.set_pose(block, Pose::at(at)).unwrap();
        surfaces.set(block, towards);
        gaps.push((at.z - half) - (hero.pivot.z + 0.9144));
    }
    // 1.0856 m apart at the start, closed at 1 m/s: met about 65 frames in.
    assert!(gaps.iter().all(|&g| g > 0.0), "{gaps:?}");
    for g in &gaps[80..] {
        assert!((g - 0.005).abs() < 1e-4, "{g}");
    }
    assert!((hero.velocity.z + 1.0).abs() < 1e-3, "{:?}", hero.velocity);
    let shape = Shape::Sphere { radius: 0.9 };
    let centre = Pose::at(hero.pivot + Vec3::Y * 0.9144);
    assert!(world.overlaps(&shape, centre, &[]).is_empty());
}
