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
    if level == LEVEL_214 {
        // Its intro skipped as a player would (AS.81: it plays to its end).
        stops.extend(skip_214s_intro(&mut run));
    }
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
/// screen clear. The cave's own scene played out, walked into the exit's box
/// (its third waypoint): back out 1 ft past its first, facing out of the
/// cave, the controller checking for the way in again. Nothing stops.
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
    // The cave's own scene first (AU.33: its Giant freed, on the listing).
    let (mut waited, mut seen) = (0, false);
    while waited < 3000 {
        step(&mut run, &mut stops, None, [false; 4]);
        waited += 1;
        let hidden = guacamole::scripts::player_hud_hidden(&run.vm);
        seen |= hidden;
        if seen && !hidden {
            break;
        }
    }
    println!("the cave's scene over after {waited} steps");
    assert!(seen && waited < 3000, "the scene came and went");
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

/// openspec AS.112: chapter 1's cave keeps the portal while it takes him in.
/// The controller's own `Game - Cutscene` leaves the switching rule at its
/// default, so the lock stands from its darkness to the room (`need keep`
/// holds his toy, `Cutscene State` the scene: 81 steps), while the room's
/// own scene, the cutscene system's, passes switching through. A swap asked
/// in the fade waits for the lock to let go, as the TV holds it, and lands
/// with the warp already done: the next hero joins in the room, nothing
/// stopped, no warp owed.
#[test]
#[ignore = "reads the Giants dump"]
fn a_swap_asked_in_the_caves_fade_waits_for_the_room() {
    use guacamole::scripts::scene_keeps_figures;
    println!("load at start: {}", load());
    let (mut run, mut stops) = joined(guac_extract::levels::FIRST_RUN[0]);
    let cave = handle(&run, "Interior_controller_Cave");
    run.put(Vec3::new(11.0, 22.4, -28.5), 105.0);
    for _ in 0..10 {
        step(&mut run, &mut stops, None, [false; 4]);
    }
    assert!(!scene_keeps_figures(&run.vm), "no scene, no lock");
    // On the stick to the fade.
    let mut walked = 0;
    while behaviour(&run, cave).as_deref() != Some(FADING) {
        step(&mut run, &mut stops, Some([16.5, -30.2]), [false; 4]);
        walked += 1;
        assert!(walked < 600, "into the box");
    }
    let (mut locked, mut landed_locked) = (0, false);
    let landed0 = run.world.landed.len();
    while scene_keeps_figures(&run.vm) {
        step(&mut run, &mut stops, None, [false; 4]);
        locked += 1;
        landed_locked |= run.world.landed.len() > landed0;
        assert!(locked < 600, "the lock lets go");
    }
    println!("walked {walked}, locked {locked} steps");
    assert!((78..=84).contains(&locked), "{locked}");
    assert!(landed_locked, "the warp lands under the lock");
    assert_eq!(behaviour(&run, cave).as_deref(), Some(INSIDE));
    // The swap goes through now: the room holds the next hero.
    let at = run.hero().pivot;
    run.deactivate().unwrap_or_else(|h| panic!("{h}"));
    run.join().unwrap_or_else(|h| panic!("{h}"));
    for _ in 0..30 {
        step(&mut run, &mut stops, None, [false; 4]);
    }
    assert!(run.hero().pivot.distance(at) < 3.0, "{}", run.hero().pivot);
    assert_eq!(run.world.landed.len(), landed0 + 1, "one warp, landed");
    // The room's scene lets switching through.
    let mut seen = false;
    for _ in 0..1500 {
        step(&mut run, &mut stops, None, [false; 4]);
        seen |= guacamole::anchors::scene_holds(&run.vm);
        assert!(
            !scene_keeps_figures(&run.vm),
            "the cutscene system's own keeps nothing"
        );
    }
    assert!(seen, "the room's scene ran");
    println!("load at end: {}", load());
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

/// openspec AS.96's review: the hero's figure off the portal while the
/// cave's warp is owed (the step the darkness is whole, which is the step
/// the controller asks it) and the next hero on, who comes in where the
/// last stood under a handle of his own: he's the one set down inside, 1 ft
/// behind the arrival, the original's warp control being the player's
/// slot's, whoever stands on it; the cave waits for his exit and the
/// darkness lifts. Nothing stops.
#[test]
#[ignore = "reads the Giants dump"]
fn a_hero_swapped_while_the_caves_warp_is_owed_is_the_one_it_lands() {
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
    let arrival = run.files.points[&p.waypoints[1]] - Vec3::Z * 0.3048;
    run.put(Vec3::new(11.0, 22.4, -28.5), 105.0);
    for _ in 0..10 {
        step(&mut run, &mut stops, None, [false; 4]);
    }
    let landed0 = run.world.landed.len();
    let mut whole = None;
    for k in 0..600 {
        let dark = behaviour(&run, cave).as_deref() == Some(FADING);
        step(
            &mut run,
            &mut stops,
            (!dark).then_some([16.5, -30.2]),
            [false; 4],
        );
        if guacamole::interiors::darkness(&run.vm, &run.world) >= 1.0 {
            whole = Some(k);
            break;
        }
    }
    whole.expect("the darkness whole");
    assert_eq!(run.world.landed.len(), landed0, "not landed yet");
    let first = run.hero;
    run.deactivate().unwrap_or_else(|h| panic!("{h}"));
    run.join().unwrap_or_else(|h| panic!("{h}"));
    let next = run.hero;
    assert_ne!(first, next, "the next under a handle of his own");
    let mut inside = None;
    for k in 0..240 {
        step(&mut run, &mut stops, None, [false; 4]);
        if inside.is_none() && behaviour(&run, cave).as_deref() == Some(INSIDE) {
            inside = Some(k);
        }
        if inside.is_some() && guacamole::interiors::darkness(&run.vm, &run.world) == 0.0 {
            break;
        }
    }
    let landed = run.world.landed.get(landed0).copied();
    println!(
        "swapped at the whole darkness; landed {landed:?}, the next {next:#x}, waiting {inside:?}; {:.1} s; load at end: {}",
        started.elapsed().as_secs_f64(),
        load()
    );
    let landed = landed.expect("a warp landed");
    assert_eq!(landed.minion, next, "the next hero set down");
    assert!(
        run.hero().pivot.distance(arrival) < 0.35,
        "{} against {arrival}",
        run.hero().pivot
    );
    assert!(inside.is_some(), "the cave waits for his exit");
    assert_eq!(guacamole::interiors::darkness(&run.vm, &run.world), 0.0);
    assert!(stops.is_empty(), "{stops:#?}");
}

/// Level 214's workshop under the house by Setup_004's yard: by its bomb
/// spawner, and where AS.96's Mac route threw its bomb at its wall to the
/// treasure (`Breakable_Stone_Wall(1)`), facing it.
const WORKSHOP_BOMBS: [f32; 2] = [387.43, 216.0];
const WORKSHOP_THROW: ([f32; 3], f32) = ([376.8054, -299.9297, 219.5375], 283.167);

/// The bombs standing.
fn bombs(run: &Run) -> Vec<Handle> {
    run.world
        .clones()
        .into_iter()
        .filter(|h| run.world.name(*h) == "Bomb_Template")
        .filter(|h| run.world.actor(*h).is_some_and(|a| !a.removed))
        .collect()
}

/// Into the workshop and onto its bomb spawner: whether a bomb other than
/// `not` was taken there.
fn the_workshop_gives_one(run: &mut Run, stops: &mut Vec<String>, not: Handle) -> Option<u32> {
    run.put(Vec3::new(20.0, -16.0, 20.0), 90.0);
    for _ in 0..120 {
        step(run, stops, None, [false; 4]);
    }
    let house = run.placed("Interior_Controller_Island_2_1").1;
    let landed0 = run.world.landed.len();
    run.put(house, 0.0);
    for _ in 0..300 {
        step(run, stops, None, [false; 4]);
        if run.world.landed.len() > landed0 {
            break;
        }
    }
    assert!(run.world.landed.len() > landed0, "into the workshop");
    for k in 0..900u32 {
        let at = run.hero().pivot;
        let far = (at.x - WORKSHOP_BOMBS[0]).hypot(at.z - WORKSHOP_BOMBS[1]) > 0.8;
        step(run, stops, far.then_some(WORKSHOP_BOMBS), [false; 4]);
        if bombs(run)
            .into_iter()
            .any(|b| b != not && behaviour(run, b).as_deref() == Some("following player"))
        {
            return Some(k);
        }
    }
    None
}

/// The first bomb, taken by the first spawner.
fn the_first_bomb(run: &mut Run, stops: &mut Vec<String>) -> Handle {
    run.put(Vec3::new(36.0, -16.0, 27.9), 90.0);
    for _ in 0..240 {
        step(run, stops, None, [false; 4]);
        if let Some(b) = bombs(run)
            .into_iter()
            .find(|b| behaviour(run, *b).as_deref() == Some("following player"))
        {
            return b;
        }
    }
    panic!("the first bomb taken");
}

/// openspec AU.33's review: his figure off the portal with the bomb in his
/// hands and the next on. The bomb's own `no players present` hands it on
/// or it runs out, and either way no spawner stands empty for good: the
/// soft lock the tool's let-go is there to stop. A hero beaten goes the same
/// way, off by the deactivate path.
#[test]
#[ignore = "reads the Giants dump"]
fn a_bomb_held_through_a_swap_leaves_the_workshops_spawner_free() {
    println!("load at start: {}", load());
    let started = std::time::Instant::now();
    let (mut run, mut stops) = joined(LEVEL_214);
    let bomb = the_first_bomb(&mut run, &mut stops);
    let values = run.figure_values().expect("his figure's words");
    let gone = run.deactivate().unwrap_or_else(|h| panic!("{h}"));
    println!("off: {gone:?}");
    run.join_tag(eruptor_tag(values))
        .unwrap_or_else(|h| panic!("{h}"));
    let tool = run.vm.program().global_named("Player 1 Tool").unwrap();
    let mut out = None;
    for k in 0..1500u32 {
        step(&mut run, &mut stops, None, [false; 4]);
        if run.vm.global_peek(tool) == Some(Value::Null)
            && behaviour(&run, bomb).as_deref() != Some("following player")
        {
            out = Some(k);
            break;
        }
    }
    let out = out.expect("let go of, its count run out");
    let taken = the_workshop_gives_one(&mut run, &mut stops, bomb);
    println!(
        "let go after {out} steps, the workshop's taken after {taken:?}; {:.1} s; load at end: {}",
        started.elapsed().as_secs_f64(),
        load()
    );
    assert!(taken.is_some(), "the workshop's own bomb");
    assert!(stops.is_empty(), "{stops:#?}");
}

/// openspec AU.33: a bomb left to run out in his hands is no bomb he carries.
/// Its count at 0 it bursts over him and stands back on the first spawner,
/// and the workshop's spawner, which clones none while a player carries one,
/// clones its own; taken there. Until AU.33 the bomb stayed his, so the
/// workshop stood empty and its wall and the key behind it stayed shut. The
/// first run with no anchor stopped there. Nothing stops.
#[test]
#[ignore = "reads the Giants dump"]
fn a_bomb_run_out_in_his_hands_leaves_the_workshops_spawner_free() {
    println!("load at start: {}", load());
    let started = std::time::Instant::now();
    let (mut run, mut stops) = joined(LEVEL_214);
    let bomb = the_first_bomb(&mut run, &mut stops);
    let mut out = None;
    for k in 0..1200u32 {
        step(&mut run, &mut stops, None, [false; 4]);
        if behaviour(&run, bomb).as_deref() == Some("wait to be grabbed") {
            out = Some(k);
            break;
        }
    }
    let out = out.expect("it ran out and stands on its spawner again");
    let tool = run.vm.program().global_named("Player 1 Tool").unwrap();
    assert_eq!(run.vm.global_peek(tool), Some(Value::Null), "his no more");
    let taken = the_workshop_gives_one(&mut run, &mut stops, bomb);
    println!(
        "ran out after {out} steps, the workshop's taken after {taken:?}; {:.1} s; load at end: {}",
        started.elapsed().as_secs_f64(),
        load()
    );
    assert!(taken.is_some(), "the workshop's own bomb");
    assert!(stops.is_empty(), "{stops:#?}");
}

/// openspec AS.96's review: level 214's workshop wall and the bomb's reach.
/// A bomb's burst opens every bombable wall within `Bomb_Range` of it
/// across the ground, a level global that starts at 20 ft and that the
/// burst sets to 35 ft once it has looked: a run's first burst reaches 20
/// ft, every later one 35. Thrown from where AS.96's Mac route stood (its
/// run's first burst: the `part` anchor skipped the first island's bombs),
/// the bomb bursts 20-22 ft from the wall, which stands; taken again and
/// thrown from the same place, it opens it. Nothing stops.
#[test]
#[ignore = "reads the Giants dump"]
fn level_214s_workshop_wall_takes_a_runs_first_burst_from_20_ft_and_a_later_from_35() {
    println!("load at start: {}", load());
    let started = std::time::Instant::now();
    let (mut run, mut stops) = joined(LEVEL_214);
    let range = run
        .vm
        .program()
        .global_named("Bomb_Range")
        .expect("the bomb's range");
    assert_eq!(run.vm.global(range), Some(Value::Int(20)), "20 ft first");
    let house = run.placed("Interior_Controller_Island_2_1").1;
    let wall = handle(&run, "Breakable_Stone_Wall(1)");
    let wall_at = run.world.actor(wall).unwrap().position;
    let landed0 = run.world.landed.len();
    run.put(house, 0.0);
    for _ in 0..300 {
        step(&mut run, &mut stops, None, [false; 4]);
        if run.world.landed.len() > landed0 {
            break;
        }
    }
    assert!(run.world.landed.len() > landed0, "into the workshop");
    for _ in 0..60 {
        step(&mut run, &mut stops, None, [false; 4]);
    }
    let mut throws = Vec::new();
    for _ in 0..2 {
        let mut bomb = None;
        for _ in 0..900 {
            let at = run.hero().pivot;
            let far = (at.x - WORKSHOP_BOMBS[0]).hypot(at.z - WORKSHOP_BOMBS[1]) > 0.8;
            step(
                &mut run,
                &mut stops,
                far.then_some(WORKSHOP_BOMBS),
                [false; 4],
            );
            bomb = bombs(&run)
                .into_iter()
                .find(|b| behaviour(&run, *b).as_deref() == Some("following player"));
            if bomb.is_some() {
                break;
            }
        }
        let bomb = bomb.expect("its bomb taken");
        for _ in 0..60 {
            step(&mut run, &mut stops, None, [false; 4]);
        }
        run.put(Vec3::from_array(WORKSHOP_THROW.0), WORKSHOP_THROW.1);
        step(&mut run, &mut stops, None, [false; 4]);
        let (mut last, mut burst) = (None, None);
        for k in 0..150u32 {
            step(&mut run, &mut stops, None, [false, false, false, k == 0]);
            if burst.is_none() && behaviour(&run, bomb).as_deref() == Some("bomb") {
                last = run.world.actor(bomb).map(|a| a.position);
            } else if burst.is_none() && last.is_some() {
                burst = last;
            }
        }
        let burst = burst.expect("thrown and burst");
        let ft = (burst.x - wall_at.x).hypot(burst.z - wall_at.z) / 0.3048;
        throws.push((ft, run.world.actor(wall).is_some_and(|a| a.removed)));
    }
    println!(
        "throws (ft from the wall across the ground, open): {throws:?}; {:.1} s; load at end: {}",
        started.elapsed().as_secs_f64(),
        load()
    );
    assert!(
        throws.iter().all(|(ft, _)| (20.0..22.0).contains(ft)),
        "{throws:?}"
    );
    assert!(
        !throws[0].1,
        "the run's first burst reaches 20 ft: {throws:?}"
    );
    assert!(throws[1].1, "a later one 35 ft: {throws:?}");
    assert_eq!(run.vm.global(range), Some(Value::Int(35)));
    assert!(stops.is_empty(), "{stops:#?}");
}
