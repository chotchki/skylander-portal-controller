//! The hub's utilities controller and its doors (openspec AS.138), headless
//! with guacamole's host on the TV's listing for each hub stage. Ignored by
//! default; they read `GUAC_GIANTS_DUMP` (process env, else `.env.dev`):
//!
//! `cargo test -p guacamole --profile disc --test giants_hub_doors -- --ignored --nocapture --test-threads=1`

use guac_motion::stick::{DEAD_ZONE, Stick};
use guac_script::{Handle, Mode, Value};
use guacamole::script_engine::Pad;

mod common;

use common::*;

const STAGES: [&str; 4] = [
    "level_hub_2_stage_1",
    "level_hub_2_stage_2",
    "level_hub_2_stage_3",
    "level_hub_2_stage_4",
];

fn setup() -> Setup {
    Setup {
        interface: true,
        props: true,
        sounds: true,
        ..Setup::default()
    }
}

fn int_global(run: &Run, name: &str) -> Option<i32> {
    let g = run.vm.program().global_named(name)?;
    run.vm.global_peek(g).and_then(|v| run.vm.value(v).as_i32())
}

fn behaviour(run: &Run, h: Handle) -> Option<String> {
    run.vm
        .placement_of(h)
        .and_then(|id| run.vm.behaviour(id))
        .map(str::to_owned)
}

/// Each stage joined and settled: the run and what stopped on the way.
fn joined(level: &str) -> (Run, Vec<String>) {
    let (mut run, _found) = run_on_level(Mode::Play, level, setup());
    let mut stops = Vec::new();
    for _ in 0..2 {
        stops.extend(run.step_play().unwrap());
    }
    run.join().unwrap_or_else(|h| panic!("{level}: {h}"));
    for _ in 0..300 {
        stops.extend(run.step_play().unwrap());
    }
    (run, stops.iter().map(ToString::to_string).collect())
}

/// The doors the rooms' controllers stand behind, by what each door's
/// `unlock value` holds: above 2, stage 1's floor, and at or under 7, stage
/// 2's.
const LATER_ROOMS: [&str; 2] = [
    "Transition_controller_Crane",
    "Transition_controller_GunDeck",
];
const FIRST_ROOMS: [&str; 5] = [
    "Transition_controller_Galley",
    "Transition_controller_Calis",
    "Transition_controller_Flynns",
    "Transition_controller_Persephones",
    "Transition_controller_Tower",
];

/// openspec AS.138: on every stage each door has made its art and rests by
/// its `unlock value` against the adventures done: stage 1 (2 done) leaves
/// the crane deck and the gun deck shut, their rooms' controllers off and
/// their doors `locked`, and every other room open; from stage 2 (7 done)
/// all stand open. The orbit's Hatterson is off and the galley's blocker
/// stands with no Dragon Engine. Nothing stops.
#[test]
#[ignore = "reads the Giants dump"]
fn every_stages_doors_stand_by_their_unlock_values() {
    let started = std::time::Instant::now();
    println!("load at start: {}", load());
    for (i, level) in STAGES.into_iter().enumerate() {
        let (mut run, stops) = joined(level);
        println!(
            "{level}: {:?} done, {} clones",
            int_global(&run, "Levels_Completed"),
            run.world.clones().len()
        );
        assert!(stops.is_empty(), "{level}: {stops:#?}");
        for room in LATER_ROOMS {
            let off = run.world.removed(run.placed(room).0);
            assert_eq!(off, i == 0, "{level}: {room}");
        }
        for room in FIRST_ROOMS {
            assert!(!run.world.removed(run.placed(room).0), "{level}: {room}");
        }
        let cali = run.placed("Door_Cali's_Room").0;
        assert_eq!(behaviour(&run, cali).as_deref(), Some(OPENS), "{level}");
        // Its art: a door's own clone, standing.
        let door1 = run
            .vm
            .placement_of(cali)
            .and_then(|id| run.vm.local(id, "door1"))
            .map(|v| run.vm.value(v));
        let Some(Value::Entity(art)) = door1 else {
            panic!("{level}: no art: {door1:?}");
        };
        assert!(!run.world.removed(art), "{level}");
        assert!(
            run.world.removed(run.placed("Hatterson(Orbit)").0),
            "{level}"
        );
        assert!(
            !run.world.removed(run.placed("Galley_Blocker").0),
            "{level}"
        );
        // The game room's door, on the upper deck with the crane deck's: on
        // stage 1, behind its lock, it sleeps at its file's range while he's
        // on the main deck (the hub's intro wakes it and resets the range at
        // its end, AU.53). Later stages differ: stage 2's scripts wake it
        // at the start, stage 3's leave it asleep until he's there.
        let locks = run.files.placed.iter().any(|p| p.name == "PuzzleLock");
        let room = run.placed("Door_Game_Room").0;
        let file = run
            .files
            .placed
            .iter()
            .find(|p| p.name == "Door_Game_Room")
            .map(|p| p.range);
        let start = run.world.actor(room).map(|a| (a.active, a.range));
        println!("{level}: the game room's door at the start {start:?}, its file's range {file:?}");
        if locks {
            assert_eq!(start, file.map(|r| (false, r)), "{level}: the game room");
        }
        // The crane deck's stands far off the deck, asleep past stage 2's
        // range: a hero by it wakes it.
        let (crane, at) = run.placed("Door_Crane_Deck");
        run.put(at + guac_motion::seam::Vec3::new(0.0, 1.0, 3.0), 0.0);
        for _ in 0..30 {
            run.step_play().unwrap();
        }
        let now = behaviour(&run, crane);
        match i {
            0 => assert_eq!(now.as_deref(), Some("locked"), "{level}"),
            _ => assert!(
                matches!(now.as_deref(), Some(OPENS | CLOSES)),
                "{level}: {now:?}"
            ),
        }
        // The game room's door on the same deck, awake with him there,
        // waits on its lock, a puzzle nothing on the TV plays (openspec
        // AS.154; stand-in ledger: `game-room`), and with none opens to him.
        assert!(
            run.world.actor(room).is_some_and(|a| a.active),
            "{level}: the game room awake"
        );
        println!(
            "{level}: game room behind a lock: {locks}, its door {:?}",
            behaviour(&run, room)
        );
        if locks {
            assert_eq!(
                behaviour(&run, room).as_deref(),
                Some("check for unlock"),
                "{level}"
            );
            assert!(run.vm.inert().contains(&run.placed("PuzzleLock").0));
        } else {
            assert_eq!(behaviour(&run, room).as_deref(), Some(OPENS), "{level}");
        }
    }
    println!(
        "{:.1} s; load at end: {}",
        started.elapsed().as_secs_f64(),
        load()
    );
}

/// The stick that pushes along `heading` against a view looking along
/// `view`, past the dead zone per axis, as giants_route's driver makes it.
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

/// Walked on the stick through `points` (across the ground), each within
/// 1 m, at most 900 steps each: where he stopped if he didn't get there.
fn walk(run: &mut Run, points: &[[f32; 2]]) -> Result<(), guac_motion::seam::Vec3> {
    for &to in points {
        let mut reached = false;
        for _ in 0..900 {
            let at = run.hero().pivot;
            if (to[0] - at.x).hypot(to[1] - at.z) <= 1.0 {
                reached = true;
                break;
            }
            let heading = (to[0] - at.x).atan2(to[1] - at.z).to_degrees();
            run.bench.pads[0] = Pad {
                stick: stick_for(run.bench.view, heading),
                actions: [false; 4],
                connected: true,
            };
            run.step_play().unwrap();
        }
        if !reached {
            return Err(run.hero().pivot);
        }
    }
    Ok(())
}

/// openspec AS.138: with the doors and their art on the listing, a hero
/// handed over on any stage walks to Cali on the main deck and up the helm's
/// stairs to Flynn, the way chotchki's kids go, nothing in his way.
#[test]
#[ignore = "reads the Giants dump"]
fn flynn_and_cali_stay_in_reach_on_every_stage() {
    const TO_CALI: [[f32; 2]; 1] = [[-9.5, 16.0]];
    const TO_FLYNN: [[f32; 2]; 5] = [
        [1.0, 27.0],
        [1.0, 33.0],
        [1.0, 40.0],
        [1.0, 44.0],
        [0.3, 47.5],
    ];
    let started = std::time::Instant::now();
    for level in STAGES {
        let (mut run, _found) = run_on_level(Mode::Play, level, setup());
        for _ in 0..2 {
            run.step_play().unwrap();
        }
        run.world.skip_cutscenes = true;
        run.join().unwrap_or_else(|h| panic!("{level}: {h}"));
        for _ in 0..600 {
            run.step_play().unwrap();
        }
        walk(&mut run, &TO_CALI).unwrap_or_else(|at| panic!("{level}: to Cali, stopped at {at}"));
        let by_cali = run.hero().pivot;
        walk(&mut run, &TO_FLYNN).unwrap_or_else(|at| panic!("{level}: to Flynn, stopped at {at}"));
        println!(
            "{level}: by Cali at {by_cali}, by Flynn at {}",
            run.hero().pivot
        );
    }
    println!(
        "{:.1} s; load at end: {}",
        started.elapsed().as_secs_f64(),
        load()
    );
}

/// The door's own behaviours, by name, as its script spells them.
const OPENS: &str = "tirgger door open";
const CLOSES: &str = "trigger door close";

/// openspec AS.138: a door swings open as a hero comes within its reach and
/// shuts once he's gone, its art turning by the script's slide.
#[test]
#[ignore = "reads the Giants dump"]
fn a_door_opens_for_a_hero_and_shuts_behind_him() {
    let (mut run, mut stops) = joined(STAGES[0]);
    let (door, at) = run.placed("Door_Cali's_Room");
    let turned = |run: &Run| {
        let id = run.vm.placement_of(door).unwrap();
        run.vm
            .local(id, "door_rotation_value")
            .and_then(|v| run.vm.value(v).as_f32())
            .unwrap_or(0.0)
    };
    assert_eq!(turned(&run), 0.0);
    run.put(at + guac_motion::seam::Vec3::new(0.0, 0.5, 3.0), 0.0);
    let start = run.steps;
    while behaviour(&run, door).as_deref() != Some(CLOSES) {
        assert!(run.steps < start + 240, "{:?}", behaviour(&run, door));
        stops.extend(run.step_play().unwrap().iter().map(ToString::to_string));
    }
    for _ in 0..30 {
        stops.extend(run.step_play().unwrap().iter().map(ToString::to_string));
    }
    let open = turned(&run);
    println!("open at {open} after {} steps", run.steps - start);
    assert!(open.abs() > 60.0, "{open}");
    let hand_over = guac_motion::seam::Vec3::new(11.42, -12.62, 23.18);
    run.put(hand_over, 0.0);
    let start = run.steps;
    while turned(&run).abs() > 0.5 {
        assert!(run.steps < start + 240, "{}", turned(&run));
        stops.extend(run.step_play().unwrap().iter().map(ToString::to_string));
    }
    assert_eq!(behaviour(&run, door).as_deref(), Some(OPENS));
    assert!(stops.is_empty(), "{stops:#?}");
}

/// openspec AS.138: `_Utilities_Controller` hangs a part the hub's load
/// brought, once it reads equipped, and lifts `Galley_Blocker` for the
/// Dragon Engine (its id 2). A stream's work stood in for by the file's own
/// `System - Ship Parts - Define Ship Part` on a template of id 2; with none
/// defined the blocker stands, as every other test here has it.
#[test]
#[ignore = "reads the Giants dump"]
fn the_dragon_engine_lifts_the_galleys_blocker() {
    use guacamole::story_save::{self, Session};
    let (mut run, _found) = run_on_level(Mode::Play, STAGES[0], setup());
    let mut session = Session::new(Some("01HOSTPROFILE".into()));
    let step = |run: &mut Run, session: &mut Session| {
        let stops = run.step_play().unwrap();
        assert!(stops.is_empty(), "{stops:?}");
        let mut spawn = guacamole::scripts::headless_spawner(1 << 50);
        let mut engine = guacamole::script_engine::NoEngine;
        let mut host = guacamole::scripts::WorldHost {
            world: &mut run.world,
            engine: &mut engine,
            generator: &mut run.generator,
            spawn: &mut spawn,
        };
        story_save::serve(&mut run.vm, &mut host, session, "level_hub_2_stage_1").unwrap();
        if session.take_write().is_some() {
            session.written(&mut run.world.save, Ok(()));
        }
    };
    for _ in 0..2 {
        step(&mut run, &mut session);
    }
    run.join().unwrap();
    for _ in 0..120 {
        step(&mut run, &mut session);
    }
    let controller = run.placed("_Utilities_Controller").0;
    assert_eq!(behaviour(&run, controller).as_deref(), Some("init"));
    let blocker = run.placed("Galley_Blocker").0;
    assert!(!run.world.removed(blocker));
    let template = run
        .files
        .placed
        .iter()
        .find(|p| p.obj.unit == LEVEL && p.id == 2 && p.activity_state == 5)
        .map(|p| Value::Object(p.obj))
        .expect("a template of id 2");
    run.with_host(|vm, host| {
        vm.call_with(
            None,
            "Save - Collection Get/Set - Legendary Treasure ",
            &[
                ("adventure title", Value::Null),
                ("check id", Value::Int(0)),
                ("collected", Value::Int(1)),
                ("equipped", Value::Int(1)),
            ],
            &[],
            host,
        )
        .unwrap();
        vm.call_with(
            None,
            "System - Ship Parts - Define Ship Part",
            &[
                ("attach node", Value::Int(-1)),
                ("actor", template),
                ("sprite", Value::Null),
            ],
            &[],
            host,
        )
        .unwrap();
    });
    for _ in 0..30 {
        step(&mut run, &mut session);
    }
    println!(
        "behaviour {:?}, engine {:?}, blocker off {}",
        behaviour(&run, controller),
        int_global(&run, "Dragon_Engine"),
        run.world.removed(blocker)
    );
    assert_eq!(behaviour(&run, controller).as_deref(), Some("main"));
    assert_eq!(int_global(&run, "Dragon_Engine"), Some(1));
    assert!(run.world.removed(blocker), "the galley stays shut");
}
