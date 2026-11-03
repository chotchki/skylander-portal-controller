//! The interiors (openspec AS.96), headless with guacamole's host on the
//! TV's listing for each level. Ignored by default; they read
//! `GUAC_GIANTS_DUMP` (process env, else `.env.dev`):
//!
//! `cargo test -p guacamole --profile disc --test giants_interiors -- --ignored --nocapture`
//!
//! Chapter 1's cave against AH.10's oracle run into it
//! (`ah10-staged-1792556887/leave`); level 214 against its own scripts, no
//! recording of it existing.

use guac_motion::seam::Vec3;
use guac_motion::stick::{DEAD_ZONE, Stick};
use guac_script::{Handle, Mode, Value};
use guacamole::script_engine::Pad;

mod common;

use common::*;

const LEVEL_214: &str = "level_214_tutotial_2";

/// The interiors' behaviours, as their script names them.
const CHECKING: &str = "1. check for enterance";
const FADING: &str = "2. fade out fade in";
const INSIDE: &str = "3. wait for exit";

fn setup() -> Setup {
    Setup {
        interface: true,
        props: true,
        sounds: false,
        ..Setup::default()
    }
}

fn joined(level: &str) -> (Run, Vec<String>) {
    let (mut run, _found) = run_on_level(Mode::Play, level, setup());
    let mut stops = Vec::new();
    for _ in 0..2 {
        stops.extend(run.step_play().unwrap());
    }
    run.join().unwrap_or_else(|h| panic!("{h}"));
    for _ in 0..30 {
        stops.extend(run.step_play().unwrap());
    }
    (run, stops.iter().map(ToString::to_string).collect())
}

fn behaviour(run: &Run, h: Handle) -> Option<String> {
    run.vm
        .placement_of(h)
        .and_then(|id| run.vm.behaviour(id))
        .map(str::to_owned)
}

fn handle(run: &Run, name: &str) -> Handle {
    run.placed(name).0
}

fn stick_for(view: f32, heading: f32) -> Stick {
    let d = (heading - view).to_radians();
    let raw = |a: f32| {
        if a.abs() < 1e-4 {
            0.0
        } else {
            a.signum() * (DEAD_ZONE + (1.0 - DEAD_ZONE) * a.abs())
        }
    };
    Stick::new(raw(-d.sin()), raw(d.cos()))
}

/// A step with the stick held toward `to` (across the ground), or at rest
/// with the actions given.
fn step(run: &mut Run, stops: &mut Vec<String>, to: Option<[f32; 2]>, actions: [bool; 4]) {
    let stick = match to {
        Some(to) => {
            let at = run.hero().pivot;
            stick_for(
                run.bench.view,
                (to[0] - at.x).atan2(to[1] - at.z).to_degrees(),
            )
        }
        None => Stick::new(0.0, 0.0),
    };
    run.bench.pads[0] = Pad {
        stick,
        actions,
        connected: true,
    };
    stops.extend(run.step_play().unwrap().iter().map(ToString::to_string));
}

/// What a walk into an interior came to, each step counted from the walk's
/// start: the controller's darkness begun, the warp landed (where and
/// facing which way), the controller waiting for the exit again, and the
/// darkest the screen got.
#[derive(Debug, Default)]
struct Passage {
    dark_at: Option<u32>,
    landed_at: Option<u32>,
    landed: Option<(Vec3, f32)>,
    done_at: Option<u32>,
    darkest: f32,
}

/// Walked on the stick toward `to` until `controller` takes him (its
/// darkness), then at rest until it's back to `settled`: within `steps`.
fn pass(
    run: &mut Run,
    stops: &mut Vec<String>,
    controller: Handle,
    to: [f32; 2],
    settled: &str,
    steps: u32,
) -> Passage {
    let mut p = Passage::default();
    let landed0 = run.world.landed.len();
    for k in 0..steps {
        let walking = p.dark_at.is_none();
        step(run, stops, walking.then_some(to), [false; 4]);
        let b = behaviour(run, controller);
        if p.dark_at.is_none() && b.as_deref() == Some(FADING) {
            p.dark_at = Some(k);
        }
        p.darkest = p
            .darkest
            .max(guacamole::interiors::darkness(&run.vm, &run.world));
        if p.landed.is_none()
            && let Some(l) = run.world.landed.get(landed0)
        {
            p.landed_at = Some(k);
            p.landed = Some((l.at, l.facing));
        }
        if p.dark_at.is_some() && b.as_deref() == Some(settled) {
            p.done_at = Some(k);
            break;
        }
    }
    p
}

/// openspec AS.96: chapter 1's cave (`Interior_controller_Cave`) walked
/// into and out of on the stick, against AH.10's oracle run into it. Set
/// down past the spike gate (up until the first fight is won, which nothing
/// here stands on) and walked at the cave's mouth: inside the controller's
/// box its darkness begins (the screen black 0.5 s on), the hero lands 1 ft
/// behind its arrival waypoint facing the room's way 42 steps after (the
/// fade's 30 and the warp's 0.2 s), 0.84 m from where the oracle's
/// re-entry stood him, his health kept, and the controller waits for the
/// exit 81 steps after the darkness began (79 frames on the oracle), the
/// screen clear. Walked into the exit's box (its third waypoint): back out
/// 1 ft past its first, facing out of the cave, the controller checking for
/// the way in again. Nothing stops.
#[test]
#[ignore = "reads the Giants dump"]
fn chapter_1s_cave_is_entered_and_left_on_foot() {
    println!("load at start: {}", load());
    let started = std::time::Instant::now();
    let (mut run, mut stops) = joined(guac_extract::levels::FIRST_RUN[0]);
    let cave = handle(&run, "Interior_controller_Cave");
    let p = run
        .files
        .placed
        .iter()
        .find(|p| p.name == "Interior_controller_Cave")
        .unwrap()
        .clone();
    let wp: Vec<Vec3> = p.waypoints.iter().map(|w| run.files.points[w]).collect();
    assert_eq!(wp.len(), 3, "out, the arrival and the exit");
    let health = run.health();
    run.put(Vec3::new(11.0, 22.4, -28.5), 105.0);
    for _ in 0..10 {
        step(&mut run, &mut stops, None, [false; 4]);
    }
    assert_eq!(behaviour(&run, cave).as_deref(), Some(CHECKING));
    let into = pass(&mut run, &mut stops, cave, [16.5, -30.2], INSIDE, 600);
    println!("into the cave: {into:?}");
    let (at, facing) = into.landed.expect("he landed inside");
    let (dark, landed, done) = (
        into.dark_at.unwrap(),
        into.landed_at.unwrap(),
        into.done_at.unwrap(),
    );
    // 1 ft behind the arrival, as its marker's displace leaves it.
    let arrival = wp[1] - Vec3::Z * 0.3048;
    assert!(at.distance(arrival) < 0.01, "{at} against {arrival}");
    assert!(facing.abs() < 0.01, "{facing}");
    assert_eq!(
        landed - dark,
        42,
        "the fade's 30 steps and the warp's 0.2 s"
    );
    // AH.10's oracle run: behaviour 2 for 79 frames (867 to 946), and its
    // re-entry at (-919.13, 29.84, -341.64).
    assert!((79..=83).contains(&(done - dark)), "{}", done - dark);
    let oracle = Vec3::new(-919.13, 29.84, -341.64);
    println!(
        "landed {:.2} m from the oracle's re-entry",
        at.distance(oracle)
    );
    assert!(at.distance(oracle) < 1.0);
    assert!(
        into.darkest >= 1.0,
        "black at its darkest: {}",
        into.darkest
    );
    assert_eq!(guacamole::interiors::darkness(&run.vm, &run.world), 0.0);
    assert_eq!(run.health(), health, "his health kept through the warp");
    // Out through the exit's box.
    let exit = run.vm.placement_of(cave).and_then(|id| {
        let v = run.vm.local(id, "exit teleporter")?;
        match run.vm.value(v) {
            Value::Entity(e) => run.world.actor(e).map(|a| a.position),
            _ => None,
        }
    });
    let exit = exit.expect("the exit's box");
    assert!(
        (exit.x - wp[2].x).hypot(exit.z - wp[2].z) < 0.01,
        "at the third waypoint: {exit}"
    );
    let out = pass(&mut run, &mut stops, cave, [exit.x, exit.z], CHECKING, 900);
    println!("out of the cave: {out:?}");
    let (at, facing) = out.landed.expect("he landed outside");
    // 1 ft past the first waypoint, facing out of the cave.
    let outside = wp[0];
    assert!(
        (at.x - outside.x).hypot(at.z - outside.z) < 0.35,
        "{at} by {outside}"
    );
    assert!((facing - 290.0).abs() < 0.01, "{facing}");
    assert_eq!(out.landed_at.unwrap() - out.dark_at.unwrap(), 42);
    println!(
        "dark {dark}, landed {landed}, waiting {done}; out: {out:?}; {:.1} s; load at end: {}",
        started.elapsed().as_secs_f64(),
        load()
    );
    assert!(stops.is_empty(), "{stops:#?}");
}

/// openspec AS.96: every interior level 214 places entered and left: the
/// hero set down in each controller's box in turn lands at its arrival,
/// then set down in its exit's box lands back outside, the controller
/// checking for the way in again; the darkness up and gone each way.
/// Nothing stops.
#[test]
#[ignore = "reads the Giants dump"]
fn every_interior_on_level_214_is_entered_and_left() {
    println!("load at start: {}", load());
    let started = std::time::Instant::now();
    let (mut run, mut stops) = joined(LEVEL_214);
    let controllers: Vec<(String, Handle, Vec3, Vec<Vec3>)> = run
        .files
        .placed
        .iter()
        .filter(|p| {
            p.obj.unit == LEVEL
                && p.activity_state != 5
                && p.script
                    .as_deref()
                    .is_some_and(|s| s.ends_with(guacamole::interiors::CONTROLLER))
        })
        .map(|p| {
            (
                p.name.clone(),
                run.handles[&p.obj],
                Vec3::from_array(p.position),
                p.waypoints.iter().map(|w| run.files.points[w]).collect(),
            )
        })
        .collect();
    assert_eq!(controllers.len(), 9);
    let mut rows = Vec::new();
    for (name, h, box_at, wp) in &controllers {
        let landed0 = run.world.landed.len();
        run.put(*box_at, 0.0);
        let mut into = Passage::default();
        for k in 0..300 {
            step(&mut run, &mut stops, None, [false; 4]);
            let b = behaviour(&run, *h);
            if into.dark_at.is_none() && b.as_deref() == Some(FADING) {
                into.dark_at = Some(k);
            }
            into.darkest = into
                .darkest
                .max(guacamole::interiors::darkness(&run.vm, &run.world));
            if b.as_deref() == Some(INSIDE) {
                into.done_at = Some(k);
                break;
            }
        }
        let landed = run.world.landed.get(landed0).copied();
        let arrival = wp[1];
        assert!(
            landed.is_some_and(|l| (l.at.x - arrival.x).hypot(l.at.z - arrival.z) < 0.35),
            "{name}: landed {landed:?}, its arrival {arrival}"
        );
        assert!(into.darkest >= 1.0, "{name}: {into:?}");
        let exit = run.vm.placement_of(*h).and_then(|id| {
            let v = run.vm.local(id, "exit teleporter")?;
            match run.vm.value(v) {
                Value::Entity(e) => run.world.actor(e).map(|a| a.position),
                _ => None,
            }
        });
        let exit = exit.unwrap_or_else(|| panic!("{name}: no exit's box"));
        for _ in 0..30 {
            step(&mut run, &mut stops, None, [false; 4]);
        }
        let landed1 = run.world.landed.len();
        run.put(exit, 0.0);
        let mut back = None;
        for k in 0..300 {
            step(&mut run, &mut stops, None, [false; 4]);
            if behaviour(&run, *h).as_deref() == Some(CHECKING) {
                back = Some(k);
                break;
            }
        }
        let out = run.world.landed.get(landed1).copied();
        let outside = wp[0];
        assert!(
            back.is_some()
                && out.is_some_and(|l| (l.at.x - outside.x).hypot(l.at.z - outside.z) < 0.35),
            "{name}: back {back:?}, landed {out:?}, outside {outside}"
        );
        rows.push(format!(
            "{name}: in {:?}, landed {:?}, out {:?}",
            into.done_at,
            landed.map(|l| l.at),
            out.map(|l| l.at)
        ));
        for _ in 0..30 {
            step(&mut run, &mut stops, None, [false; 4]);
        }
    }
    for r in &rows {
        println!("{r}");
    }
    println!(
        "{} of {} entered and left; {:.1} s; load at end: {}",
        rows.len(),
        controllers.len(),
        started.elapsed().as_secs_f64(),
        load()
    );
    assert!(stops.is_empty(), "{stops:#?}");
}
