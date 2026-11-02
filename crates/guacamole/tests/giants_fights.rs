//! Level 214 fought through past its first fight (openspec AS.58, AS.59,
//! AS.67), headless with guacamole's host on the TV's listing for the level.
//! Ignored by default; they read `GUAC_GIANTS_DUMP` (process env, else
//! `.env.dev`):
//!
//! `cargo test -p guacamole --profile disc --test giants_fights -- --ignored --nocapture --test-threads=1`
//!
//! No recording of 214 exists: every step is the level's own scripts on the
//! TV's listing, the hero set down where a walk would take him.

use guac_motion::seam::Vec3;
use guac_motion::stick::Stick;
use guac_script::{Handle, Mode};
use guacamole::script_engine::Pad;

mod common;

use common::*;

/// The level the fights are in.
const LEVEL_214: &str = "level_214_tutotial_2";

/// Where level 214's first bomb spawner stands (metres, its file's place).
const BOMB_SPAWNER: [f32; 3] = [39.005, -16.018, 27.887];
/// Short of the castle wall in front of the pods, facing it.
const BEFORE_THE_WALL: ([f32; 3], f32) = ([50.0, -17.9, 30.0], 77.7);
/// Among the pods, short of the castle wall past them
/// (`Breakable_Stone_Wall_Castle_206(6)`), facing it.
const BEFORE_THE_SECOND_WALL: ([f32; 3], f32) = ([67.6, -17.9, 36.0], 73.9);

fn setup() -> Setup {
    Setup {
        interface: true,
        props: true,
        sounds: true,
        ..Setup::default()
    }
}

/// The level's run with Eruptor joined and his arrival played out (the
/// TV's entry settles him so), and what stopped on the way.
fn joined_214() -> (Run, Vec<String>) {
    let (mut run, _found) = run_on_level(Mode::Play, LEVEL_214, setup());
    let mut stops = Vec::new();
    for _ in 0..2 {
        stops.extend(run.step_play().unwrap());
    }
    run.join().unwrap_or_else(|h| panic!("{h}"));
    for _ in 0..60 {
        stops.extend(run.step_play().unwrap());
    }
    (run, stops.iter().map(ToString::to_string).collect())
}

fn step(run: &mut Run, stops: &mut Vec<String>, actions: [bool; 4]) {
    run.bench.pads[0] = Pad {
        stick: Stick::new(0.0, 0.0),
        actions,
        connected: true,
    };
    stops.extend(run.step_play().unwrap().iter().map(ToString::to_string));
}

fn behaviour(run: &Run, h: Handle) -> Option<String> {
    run.vm
        .placement_of(h)
        .and_then(|id| run.vm.behaviour(id))
        .map(str::to_owned)
}

/// The bombs standing: a burst bomb that lay where it burst would be one.
fn bombs(run: &Run) -> Vec<Handle> {
    run.world
        .clones()
        .into_iter()
        .filter(|h| run.world.name(*h) == "Bomb_Template")
        .filter(|h| run.world.actor(*h).is_some_and(|a| !a.removed))
        .collect()
}

/// Set down 3 m short of the first spawner until its bomb is carried, at
/// most `steps`: the bomb and the step it was taken at.
fn take_the_bomb(run: &mut Run, stops: &mut Vec<String>, steps: u32) -> Option<(Handle, u32)> {
    run.put(Vec3::from_array(BOMB_SPAWNER) - Vec3::X * 3.0, 90.0);
    for _ in 0..steps {
        step(run, stops, [false; 4]);
        if let Some(&b) = bombs(run).first()
            && behaviour(run, b).as_deref() == Some("following player")
        {
            return Some((b, run.steps));
        }
    }
    None
}

/// Set down at `before`, facing its wall, △ tapped a step: the step it was
/// thrown at and the step `wall` went, within 300 steps.
fn throw_at(
    run: &mut Run,
    stops: &mut Vec<String>,
    bomb: Handle,
    before: ([f32; 3], f32),
    wall: Handle,
) -> (Option<u32>, Option<u32>) {
    run.put(Vec3::from_array(before.0), before.1);
    for _ in 0..30 {
        step(run, stops, [false; 4]);
    }
    let (mut thrown, mut opened) = (None, None);
    for k in 0..300u32 {
        step(run, stops, [false, false, false, k == 0]);
        if thrown.is_none() && behaviour(run, bomb).as_deref() == Some("bomb") {
            thrown = Some(run.steps);
        }
        if opened.is_none() && run.world.actor(wall).is_some_and(|a| a.removed) {
            opened = Some(run.steps);
        }
        if opened.is_some() {
            break;
        }
    }
    (thrown, opened)
}

/// openspec AS.67: level 214's bomb back on its spawner after its burst.
/// The first bomb opens the wall before the pods (AS.57's throw); its burst
/// done, it resets itself, which sends its script round from its STARTUP and
/// stands it again where its spawner cloned it, waiting to be taken: the
/// same bomb, nothing left standing at the blast, the spawner cloning no
/// other. Taken again from the spawner and carried among the pods, a second
/// tap of △ throws it at the castle wall past them,
/// `Breakable_Stone_Wall_Castle_206(6)`, which its burst opens. Nothing
/// stops.
#[test]
#[ignore = "reads the Giants dump"]
fn level_214s_bomb_comes_back_for_the_next_wall() {
    let started = std::time::Instant::now();
    println!("load at start: {}", load());
    let (mut run, mut stops) = joined_214();
    let (bomb, taken) = take_the_bomb(&mut run, &mut stops, 240).expect("the bomb taken");
    let (first_wall, _) = run.placed("Breakable_Stone_Wall_Castle_206(1)");
    let (second_wall, _) = run.placed("Breakable_Stone_Wall_Castle_206(6)");
    let (thrown, opened) = throw_at(&mut run, &mut stops, bomb, BEFORE_THE_WALL, first_wall);
    let (thrown, opened) = (thrown.expect("thrown"), opened.expect("the wall opened"));
    let burst_at = run.world.actor(bomb).map(|a| a.position);
    // Its burst done, it goes round: back where it was cloned, waiting.
    let spawner = Vec3::from_array(BOMB_SPAWNER);
    let mut back = None;
    for _ in 0..240 {
        step(&mut run, &mut stops, [false; 4]);
        let at = run.world.actor(bomb).map(|a| a.position);
        if back.is_none()
            && behaviour(&run, bomb).as_deref() == Some("wait to be grabbed")
            && at.is_some_and(|p| Vec3::new(p.x - spawner.x, 0.0, p.z - spawner.z).length() < 0.5)
        {
            back = Some((run.steps, at.unwrap()));
        }
    }
    let back = back.expect("the bomb back on its spawner, waiting");
    println!(
        "taken at {taken}, thrown at {thrown}, the wall opened at {opened}, burst at {burst_at:?}; back at step {} at {}",
        back.0, back.1
    );
    // The second spawner, past the pods, clones its own as he nears it.
    let from_first: Vec<Handle> = bombs(&run)
        .into_iter()
        .filter(|h| {
            run.world
                .actor(*h)
                .is_some_and(|a| a.origin.distance(spawner) < 3.0)
        })
        .collect();
    assert_eq!(from_first, [bomb], "the same bomb, no other");
    assert!(
        run.world.actor(bomb).unwrap().origin.distance(spawner) < 0.01,
        "cloned on the spawner"
    );
    // Off the spawner first, so the take is a walk-up again.
    run.put(Vec3::from_array(BEFORE_THE_WALL.0), BEFORE_THE_WALL.1);
    for _ in 0..30 {
        step(&mut run, &mut stops, [false; 4]);
    }
    let (again, retaken) = take_the_bomb(&mut run, &mut stops, 240).expect("taken again");
    assert_eq!(again, bomb);
    let (thrown2, opened2) = throw_at(
        &mut run,
        &mut stops,
        bomb,
        BEFORE_THE_SECOND_WALL,
        second_wall,
    );
    println!(
        "taken again at {retaken}, thrown at {thrown2:?}, the second wall opened at {opened2:?}; {:.1} s; load at end: {}",
        started.elapsed().as_secs_f64(),
        load()
    );
    assert!(thrown2.is_some(), "thrown again");
    assert!(opened2.is_some(), "the second wall opened");
    assert!(stops.is_empty(), "{stops:#?}");
}

/// Where level 214's second bomb spawner stands, on the rise past the
/// pods (metres, its file's place).
const SECOND_SPAWNER: [f32; 3] = [57.78, -12.1, 65.85];
/// Short of the castle wall shutting the two elves in
/// (`Breakable_Stone_Wall_Castle_206(3)`), facing it.
const BEFORE_THE_ELVES_WALL: ([f32; 3], f32) = ([51.0, -11.5, 73.0], 307.8);

/// The placement named `name` in the level's file, by handle.
fn handle(run: &Run, name: &str) -> Handle {
    run.placed(name).0
}

/// Whether `h` is beaten: off the run or its actor gone.
fn gone(run: &Run, h: Handle) -> bool {
    run.world.actor(h).is_none_or(|a| a.removed)
        || run.vm.placement_of(h).is_none_or(|id| run.vm.removed(id))
}

/// Up to `steps` steps of a fight with `foes`: every 20 steps the hero set
/// down 1.5 m short of the nearest standing, facing it, his first attack
/// pulsed two steps in four (a hand mashing South), until every one is
/// beaten. The step each was beaten at.
fn fight(run: &mut Run, stops: &mut Vec<String>, foes: &[Handle], steps: u32) -> Vec<Option<u32>> {
    let mut beaten: Vec<Option<u32>> = vec![None; foes.len()];
    for k in 0..steps {
        if k % 20 == 0 {
            let me = run.hero().pivot;
            let near = foes
                .iter()
                .filter(|h| !gone(run, **h))
                .filter_map(|h| run.world.actor(*h).map(|a| a.position))
                .min_by(|a, b| a.distance(me).total_cmp(&b.distance(me)));
            if let Some(at) = near {
                let to = Vec3::new(at.x - me.x, 0.0, at.z - me.z);
                let back = to.normalize_or_zero() * 1.5;
                let facing = guacamole::script_engine::heading_of(to);
                run.put(Vec3::new(at.x - back.x, at.y, at.z - back.z), facing);
            }
        }
        step(run, stops, [k % 4 < 2, false, false, false]);
        for (i, h) in foes.iter().enumerate() {
            if beaten[i].is_none() && gone(run, *h) {
                beaten[i] = Some(run.steps);
            }
        }
        if beaten.iter().all(Option::is_some) {
            break;
        }
    }
    beaten
}

/// openspec AS.58: level 214's second fight, on the first island past the
/// pods, on the TV's listing (`scripts::ENCOUNTERS`). The first spawner's
/// bomb, taken twice (AS.67), opens the wall before the pods and the one
/// past them. Past it `Setup_002_Controller` sees its first trigger, sets
/// its four undead Chompies going for a scene and waits for Eon's elemental
/// hint (`Explain_Elemental`) to take itself off: the hero walking into it,
/// its line plays and it goes. At its second trigger the Chompies run in and
/// fight; beaten, the second spawner's bomb opens the wall the two Elf
/// Nears spar behind (`Setup_002_Controller_Helper`), whose scene sets them
/// on him; both beaten, the helper takes itself off. Nothing stops.
#[test]
#[ignore = "reads the Giants dump"]
fn level_214s_second_fight_is_fought_on_the_first_island() {
    let started = std::time::Instant::now();
    println!("load at start: {}", load());
    let (mut run, mut stops) = joined_214();
    let controller = run.placement("Setup_002_Controller");
    let helper = handle(&run, "Setup_002_Controller_Helper");
    let hint = handle(&run, "Explain_Elemental");
    let chompies: Vec<Handle> = ["E", "F", "G", "H"]
        .iter()
        .map(|c| handle(&run, &format!("Enemy_Chompy2_{c}")))
        .collect();
    let elves: Vec<Handle> = ["Enemy_Elf_Near_S2(3)", "Enemy_Elf_Near_S2(4)"]
        .iter()
        .map(|n| handle(&run, n))
        .collect();
    // The two walls the first spawner's bomb opens (AS.57, AS.67).
    let (first_wall, second_wall) = (
        handle(&run, "Breakable_Stone_Wall_Castle_206(1)"),
        handle(&run, "Breakable_Stone_Wall_Castle_206(6)"),
    );
    for (before, wall) in [
        (BEFORE_THE_WALL, first_wall),
        (BEFORE_THE_SECOND_WALL, second_wall),
    ] {
        let (bomb, _) = take_the_bomb(&mut run, &mut stops, 300).expect("the bomb taken");
        let (_, opened) = throw_at(&mut run, &mut stops, bomb, before, wall);
        assert!(opened.is_some(), "the wall opened");
        for _ in 0..60 {
            step(&mut run, &mut stops, [false; 4]);
        }
    }
    let (health_before, xp_before) = (
        run.health().unwrap(),
        run.figure_values().map_or(0, |v| v.xp),
    );
    // Its first trigger: the Chompies set going, the wait for the hint.
    run.put(Vec3::new(82.6, -18.0, 38.6), 0.0);
    let mut waits = None;
    for _ in 0..300 {
        step(&mut run, &mut stops, [false; 4]);
        if waits.is_none() && run.vm.behaviour(controller) == Some("wait for cutscene to finish") {
            waits = Some(run.steps);
        }
    }
    let waits = waits.expect("it waits on Eon's hint");
    // Into the hint: its line, then it goes, and the controller with it.
    run.put(Vec3::new(89.4, -16.5, 51.9), 0.0);
    let mut hint_gone = None;
    for _ in 0..900 {
        step(&mut run, &mut stops, [false; 4]);
        if hint_gone.is_none() && gone(&run, hint) {
            hint_gone = Some(run.steps);
        }
        if run.vm.behaviour(controller) == Some("wait for next trigger") {
            break;
        }
    }
    let hint_gone = hint_gone.expect("Eon's hint took itself off");
    assert_eq!(run.vm.behaviour(controller), Some("wait for next trigger"));
    // Its second trigger: the Chompies run in; fought.
    run.put(Vec3::new(78.0, -16.5, 61.2), 0.0);
    let chompies_beaten = fight(&mut run, &mut stops, &chompies, 2400);
    println!(
        "the wait from step {waits}, the hint gone at {hint_gone}; Chompies beaten at {chompies_beaten:?}"
    );
    assert!(
        chompies_beaten.iter().all(Option::is_some),
        "the four beaten: {chompies_beaten:?}"
    );
    // The elves spar behind their wall until a bomb opens it.
    let elves_wall = handle(&run, "Breakable_Stone_Wall_Castle_206(3)");
    run.put(Vec3::from_array(SECOND_SPAWNER) - Vec3::X * 3.0, 90.0);
    let mut bomb = None;
    for _ in 0..300 {
        step(&mut run, &mut stops, [false; 4]);
        bomb = bombs(&run)
            .into_iter()
            .find(|b| behaviour(&run, *b).as_deref() == Some("following player"));
        if bomb.is_some() {
            break;
        }
    }
    let bomb = bomb.expect("the second spawner's bomb taken");
    let (_, opened) = throw_at(
        &mut run,
        &mut stops,
        bomb,
        BEFORE_THE_ELVES_WALL,
        elves_wall,
    );
    let opened = opened.expect("the elves' wall opened");
    println!(
        "the elves' wall opened at {opened}; health {:?}, the helper {:?}",
        run.health(),
        behaviour(&run, helper)
    );
    let elves_beaten = fight(&mut run, &mut stops, &elves, 3600);
    println!(
        "the elves beaten at {elves_beaten:?}; health {:?}, hero at {}, his script {:?}",
        run.health(),
        run.hero().pivot,
        run.minion
            .and_then(|m| run.vm.stopped(m))
            .map(|h| h.to_string())
    );
    for _ in 0..120 {
        step(&mut run, &mut stops, [false; 4]);
    }
    let (health, xp) = (
        run.health().unwrap(),
        run.figure_values().map_or(0, |v| v.xp),
    );
    println!(
        "the elves' wall opened at {opened}, the elves beaten at {elves_beaten:?}, the helper gone {}; health {health_before} -> {health}, experience {xp_before} -> {xp}; {:.1} s; load at end: {}",
        gone(&run, helper),
        started.elapsed().as_secs_f64(),
        load()
    );
    assert!(
        elves_beaten.iter().all(Option::is_some),
        "both elves beaten: {elves_beaten:?}"
    );
    assert!(gone(&run, helper), "the helper's done");
    assert!(xp > xp_before, "his figure's paid: {xp}");
    assert!(stops.is_empty(), "{stops:#?}");
}

/// Where level 214's first ship part floats, on the rise the jump pad
/// reaches (metres, its file's place).
const SHIP_PART_1: [f32; 3] = [29.45, -6.92, 56.37];
/// Where the sail's cutscene sets the hero down on the second island's
/// ship (metres, the run's own read: its second scene's player waypoint).
const ON_THE_SECOND_ISLAND: [f32; 3] = [-383.178, -12.581, 714.414];

/// openspec AS.59: level 214's way from its first island to its second, on
/// the TV's listing (`scripts::PASSAGES`). The hero at the first ship part
/// takes it; it waits on a press (South, pulsed as a hand would: with none
/// it never goes), then goes and its shutdown wakes the sail; the TV skips
/// the sail's cutscene a frame in (`level_end::SKIPPED_CUTSCENES`), which
/// sets him down on the second island's ship and takes the sail off, so
/// Setup_003, which waits for it, can start. Nothing stops.
#[test]
#[ignore = "reads the Giants dump"]
fn level_214s_first_ship_part_sails_to_the_second_island() {
    let started = std::time::Instant::now();
    println!("load at start: {}", load());
    let (mut run, mut stops) = joined_214();
    let part = handle(&run, "Ship_part_1(1)");
    let sail = handle(&run, "214_Recover_First_Part_IGC");
    run.put(Vec3::from_array(SHIP_PART_1), 0.0);
    let (mut taken, mut sailed) = (None, None);
    for k in 0..1200u32 {
        step(&mut run, &mut stops, [k % 20 < 2, false, false, false]);
        if taken.is_none() && gone(&run, part) {
            taken = Some(run.steps);
        }
        if sailed.is_none() && gone(&run, sail) {
            sailed = Some(run.steps);
            break;
        }
    }
    for _ in 0..120 {
        step(&mut run, &mut stops, [false; 4]);
    }
    let at = run.hero().pivot;
    println!(
        "the part taken at {taken:?}, the sail done at {sailed:?}; the hero at {at}, the HUD hidden {}; {:.1} s; load at end: {}",
        guacamole::scripts::player_hud_hidden(&run.vm),
        started.elapsed().as_secs_f64(),
        load()
    );
    assert!(taken.is_some(), "the part taken");
    assert!(sailed.is_some(), "the sail done");
    assert!(
        at.distance(Vec3::from_array(ON_THE_SECOND_ISLAND)) < 2.0,
        "on the second island: {at}"
    );
    assert!(
        !guacamole::scripts::player_hud_hidden(&run.vm),
        "the HUD back"
    );
    assert!(stops.is_empty(), "{stops:#?}");
}
