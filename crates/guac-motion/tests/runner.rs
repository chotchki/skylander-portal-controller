//! The camera beside the hero and the headless runner (openspec AG.17), on
//! the parry side of the seam over synthetic ground: the runner's trace is
//! the controller and the camera stepped by hand, the same every run; the
//! stick walks him against the view; the camera trails and settles inside
//! gameplay-camera's envelope; and a wall between the eye and him moves
//! nothing.

mod common;

use common::*;
use guac_motion::camera::{CameraDef, GameCamera, LookFrom};
use guac_motion::hero::Hero;
use guac_motion::parry::ParryWorld;
use guac_motion::runner::{self, Runner};
use guac_motion::seam::Vec3;
use guac_oracle::input::InputScript;
use guac_oracle::motion::Motion;

const START: Vec3 = Vec3::new(0.0, 1.0, 0.0);

fn script(ron: &str) -> InputScript {
    InputScript::from_ron(ron).unwrap()
}

/// AG.8's `ag8_level_full`: rest a second, full left 70 frames, let go,
/// full right 45 and full left 30, let go.
fn level_full() -> InputScript {
    script(
        "(steps: [
            (at: 0, pads: {0: (), 1: ()}),
            (at: 60, pads: {0: (left: (x: 0, y: 128))}),
            (at: 130, pads: {0: ()}),
            (at: 180, pads: {0: (left: (x: 255, y: 128))}),
            (at: 225, pads: {0: (left: (x: 0, y: 128))}),
            (at: 255, pads: {0: ()}),
        ])",
    )
}

fn start(seam: &ParryWorld, def: CameraDef) -> (Hero, GameCamera) {
    let hero = eruptor(seam, START, 0.0);
    let camera = GameCamera::new(def, hero.pivot);
    (hero, camera)
}

fn run(world: &ParryWorld, def: CameraDef, script: &InputScript, frames: u64) -> Motion {
    let (mut hero, mut camera) = start(world, def);
    let runner = Runner {
        seam: world,
        exclude: &[],
        planes: &[],
    };
    runner.run(&mut hero, &mut camera, script, frames).unwrap()
}

/// The runner's trace is the hero and the camera stepped by hand from the
/// same script at 1/60 s, sample for sample, and a second run of the same
/// script gives the same trace to the bit.
#[test]
fn the_runner_is_the_controller_and_the_camera_stepped_by_hand() {
    let world = world(&open_ground(), &[]);
    let script = level_full();
    let trace = run(&world, CameraDef::LEVEL_213_START, &script, 420);
    assert_eq!(trace.samples().len(), 421);

    let (mut hero, mut camera) = start(&world, CameraDef::LEVEL_213_START);
    for (k, sample) in trace.samples().iter().enumerate() {
        assert_eq!(sample.time, k as f64 / 60.0);
        assert_eq!(sample.position, hero.pivot.to_array().map(f64::from), "{k}");
        assert_eq!(sample.heading, f64::from(hero.facing), "{k}");
        let eye = sample.camera.unwrap();
        assert_eq!(
            eye.position,
            camera.view().eye.to_array().map(f64::from),
            "{k}"
        );
        assert_eq!(eye.heading, f64::from(camera.view().heading()), "{k}");
        let input = runner::input(&script, k as u64, camera.view().heading());
        assert_eq!(
            sample.stick,
            [f64::from(input.stick.x), f64::from(input.stick.y)]
        );
        let feet = hero.pivot;
        hero.step(&world, &input, DT, &[]);
        camera.step(feet, DT, &[]);
    }
    let again = run(&world, CameraDef::LEVEL_213_START, &script, 420);
    assert_eq!(trace, again);
    // He walked: 70 frames at full stick left of a view along 15°.
    let s = trace.samples();
    let gone = Vec3::from_array(s[130].position.map(|v| v as f32)) - START;
    assert!(gone.x.hypot(gone.z) > 6.0, "{gone}");
}

/// The stick against views facing 0°, 15° and 195° (gameplay-camera's
/// Forward after a switch, design decision 12): forward walks him along the
/// view across the ground, left along its +90°.
#[test]
fn the_stick_walks_him_against_the_view() {
    let world = world(&open_ground(), &[]);
    for view in [0.0f32, 15.0, 195.0] {
        let def = CameraDef {
            look_from: LookFrom::Heading((view + 180.0).rem_euclid(360.0)),
            ..CameraDef::LEVEL_213_START
        };
        for (stick, off) in [("(x: 128, y: 0)", 0.0f32), ("(x: 0, y: 128)", 90.0)] {
            let ron = format!("(steps: [(at: 0, pads: {{0: (left: {stick})}})])");
            let trace = run(&world, def, &script(&ron), 60);
            let s = trace.samples();
            let a = s[30].position;
            let b = s[60].position;
            let heading = ((b[0] - a[0]).atan2(b[2] - a[2]).to_degrees() as f32).rem_euclid(360.0);
            let want = (view + off).rem_euclid(360.0);
            assert!(
                angle_between(heading, want) < 1e-3,
                "view {view}, stick {stick}: {heading} not {want}"
            );
            assert!(angle_between(s[0].camera.unwrap().heading as f32, view) < 1e-3);
        }
    }
}

/// gameplay-camera's A walk and a stop on open level ground, from two of
/// AG.8's scripts AG.13 recorded with the camera, each to its last let-go
/// and a stand: he drifts at most 8.2° off the view's centre line while
/// pushed and the eye stays 15.16-17.97 m from his feet; once he stops the
/// camera comes to rest within 1.5° of dead ahead and 16.34-16.77 m from
/// his feet (AG.13's envelope over 273 pushes and 25 let-go stops).
///
/// The scenario's 73-83 frames to rest are the oracle's at its own frame
/// times, whose jitter pulls the camera target's velocity point back on
/// every short frame: at a steady 1/60 s the camera target leads further
/// and the same walks settle in 67 and 68 frames, where at the recordings'
/// own steps the same controller and camera settle in 77 and 79, the
/// original's 77 and 78 (`camera.rs`'s `the_step_size_term`): decision 7's
/// step-size term,
/// which AG.31 builds into the settle's floor. Here the settle is held to
/// 60-90 frames.
#[test]
fn the_camera_trails_and_settles_inside_ag13s_envelope() {
    let world = world(&open_ground(), &[]);
    let t = |frame: u64| frame as f64 / 60.0;
    // AG.8's `ag8_rest00_full`: forward and right, then back and left.
    let rest00_full = script(
        "(steps: [
            (at: 0, pads: {0: (), 1: ()}),
            (at: 60, pads: {0: (left: (x: 185, y: 3))}),
            (at: 120, pads: {0: (left: (x: 71, y: 253))}),
            (at: 180, pads: {0: ()}),
        ])",
    );
    for (name, script, pushes, release) in [
        (
            "ag8_level_full",
            level_full(),
            vec![(60, 130), (180, 255)],
            255,
        ),
        (
            "ag8_rest00_full",
            rest00_full,
            vec![(60, 120), (120, 180)],
            180,
        ),
    ] {
        let trace = run(&world, CameraDef::LEVEL_213_START, &script, 420);
        for (from, to) in pushes {
            let trail = trace.trail(t(from), t(to)).unwrap();
            eprintln!(
                "{name} {from}-{to}: trail {:+.3}° at {:.3} s, eye {:.3}-{:.3} m",
                trail.angle, trail.at, trail.eye[0], trail.eye[1]
            );
            assert!(trail.angle.abs() <= 8.2, "{trail:?}");
            assert!(trail.eye[0] >= 15.16 && trail.eye[1] <= 17.97, "{trail:?}");
        }
        let stop = trace.stop(t(release), 1e-7, t(420)).unwrap();
        let settle = trace.settle(stop.at, 1e-4, 360.0, t(420)).unwrap();
        let frames = (settle.seconds * 60.0).round();
        eprintln!(
            "{name}: stops {:.3} s after the let-go; the camera settles {frames} frames later at {:+.3}°, {:.3} m",
            stop.at - t(release),
            settle.angle,
            settle.eye
        );
        assert!((60.0..=90.0).contains(&frames), "{settle:?}");
        assert!(settle.angle.abs() <= 1.5, "{settle:?}");
        assert!((16.34..=16.77).contains(&settle.eye), "{settle:?}");
    }
}

/// gameplay-camera's Nothing pushes the camera in: a wall 7 m tall standing
/// between the eye and his middle through the whole walk leaves the
/// camera's trace, and his, bit for bit what they are without it.
#[test]
fn a_wall_between_the_camera_and_him_moves_nothing() {
    let open = world(&open_ground(), &[]);
    let mut walled = open_ground();
    walled.wall_z(-6.0, [-40.0, 40.0], [-1.0, 6.0], Vec3::Z);
    let walled = world(&walled, &[]);
    let script = level_full();
    let a = run(&open, CameraDef::LEVEL_213_START, &script, 420);
    let b = run(&walled, CameraDef::LEVEL_213_START, &script, 420);
    assert_eq!(a, b);
    // The wall cuts the line from the eye to his middle (0.9144 m up).
    let cut = a
        .samples()
        .iter()
        .filter(|s| {
            let eye = s.camera.unwrap().position;
            let mid = [s.position[0], s.position[1] + 0.9144, s.position[2]];
            let k = (-6.0 - eye[2]) / (mid[2] - eye[2]);
            (0.0..1.0).contains(&k) && eye[1] + k * (mid[1] - eye[1]) < 6.0
        })
        .count();
    eprintln!("the wall stands between the eye and his middle in {cut} of 421 samples");
    assert_eq!(cut, 421);
}
