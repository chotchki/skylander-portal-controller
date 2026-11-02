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
