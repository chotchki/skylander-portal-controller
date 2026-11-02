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
use guac_motion::stick::{DEAD_ZONE, Stick};
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
    // Its burst booms once: a second start the step after was refused while
    // the burst bomb stayed in its burst (the as-214-walk review).
    let mut booms: std::collections::BTreeSet<u64> = run
        .world
        .plays()
        .iter()
        .filter(|p| p.name.as_deref() == Some("Bomb_Boom"))
        .map(|p| p.serial)
        .collect();
    // Its burst done, it goes round: back where it was cloned, waiting.
    let spawner = Vec3::from_array(BOMB_SPAWNER);
    let mut back = None;
    for _ in 0..240 {
        step(&mut run, &mut stops, [false; 4]);
        booms.extend(
            run.world
                .plays()
                .iter()
                .filter(|p| p.name.as_deref() == Some("Bomb_Boom"))
                .map(|p| p.serial),
        );
        let at = run.world.actor(bomb).map(|a| a.position);
        if back.is_none()
            && behaviour(&run, bomb).as_deref() == Some("wait to be grabbed")
            && at.is_some_and(|p| Vec3::new(p.x - spawner.x, 0.0, p.z - spawner.z).length() < 0.5)
        {
            back = Some((run.steps, at.unwrap()));
        }
    }
    let back = back.expect("the bomb back on its spawner, waiting");
    let refused = run
        .world
        .sounds_refused()
        .filter(|r| r.name.as_deref() == Some("Bomb_Boom"))
        .count();
    println!(
        "taken at {taken}, thrown at {thrown}, the wall opened at {opened}, burst at {burst_at:?}; back at step {} at {}; booms {booms:?}, refused {refused}",
        back.0, back.1
    );
    assert_eq!(booms.len(), 1, "one boom: {booms:?}");
    assert_eq!(refused, 0, "no second start refused");
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

/// The stick that pushes at `strength` along `heading` against a view
/// looking along `view`, past the dead zone per axis
/// (`guac_motion::stick`), as giants_route's driver makes it.
fn stick_for(view: f32, heading: f32, strength: f32) -> Stick {
    let d = (heading - view).to_radians();
    let raw = |a: f32| {
        let a = a * strength;
        if a.abs() < 1e-4 {
            0.0
        } else {
            a.signum() * (DEAD_ZONE + (1.0 - DEAD_ZONE) * a.abs())
        }
    };
    Stick::new(raw(-d.sin()), raw(d.cos()))
}

/// A step with the stick held toward `to` (across the ground), the view
/// the step before's.
fn push_toward(run: &mut Run, stops: &mut Vec<String>, to: [f32; 2]) {
    let at = run.hero().pivot;
    let heading = (to[0] - at.x).atan2(to[1] - at.z).to_degrees();
    run.bench.pads[0] = Pad {
        stick: stick_for(run.bench.view, heading, 1.0),
        actions: [false; 4],
        connected: true,
    };
    stops.extend(run.step_play().unwrap().iter().map(ToString::to_string));
}

/// Walked on the stick through `points` (across the ground), each within
/// 1 m, at most 1200 steps each, then 20 steps at rest: whether he got to
/// each, and where he stopped if not.
fn walk(run: &mut Run, stops: &mut Vec<String>, points: &[[f32; 2]]) -> Result<(), Vec3> {
    for &to in points {
        let mut reached = false;
        for _ in 0..1200 {
            let at = run.hero().pivot;
            if (to[0] - at.x).hypot(to[1] - at.z) <= 1.0 {
                reached = true;
                break;
            }
            push_toward(run, stops, to);
        }
        if !reached {
            return Err(run.hero().pivot);
        }
    }
    for _ in 0..20 {
        step(run, stops, [false; 4]);
    }
    Ok(())
}

/// The push blocks by the pods (openspec AS.74), each with where its push
/// is made from (on its far side from its track, across the ground) and the
/// end marker its track drops it onto: `Push_Block_Template(1)` from the
/// east terrace into the channel below at `Push_block_End_Template(2)`,
/// then `(2)` beside it (carrying `(3)`, stacked on it) at `(1)`, then
/// `(3)` from `(1)`'s top at `(3)`, the third span of the bridge.
const PUSHES: [(&str, [f32; 2], &str); 3] = [
    (
        "Push_Block_Template(1)",
        [66.9, 49.8],
        "Push_block_End_Template(2)",
    ),
    (
        "Push_Block_Template(2)",
        [63.2, 52.4],
        "Push_block_End_Template(1)",
    ),
    (
        "Push_Block_Template(3)",
        [61.3, 43.8],
        "Push_block_End_Template(3)",
    ),
];

/// What each push came to: the block, the step it lay inactive at its end
/// (`special - push block inactive`) and where.
#[derive(Debug)]
struct Pushed {
    block: &'static str,
    inactive: Option<u32>,
    at: Vec3,
}

/// openspec AS.74: from the east terrace above the pods (where Setup_002's
/// second trigger leaves him), the three push blocks pushed on the stick
/// into the channel along their tracks, across the bridge they make to the
/// west terrace and up its slope onto the rise, by the second bomb spawner;
/// each push's outcome, then where he stopped if a walk didn't get there.
fn up_the_push_blocks(run: &mut Run, stops: &mut Vec<String>) -> (Vec<Pushed>, Result<(), Vec3>) {
    let mut pushed = Vec::new();
    // Up onto the terrace, round its rocks, to the first block's far side.
    if let Err(at) = walk(
        run,
        stops,
        &[[72.0, 62.0], [68.0, 58.0], [68.0, 52.0], PUSHES[0].1],
    ) {
        return (pushed, Err(at));
    }
    for (k, (block, from, end)) in PUSHES.into_iter().enumerate() {
        let (h, _) = run.placed(block);
        if k > 0
            && let Err(at) = walk(run, stops, &[from])
        {
            return (pushed, Err(at));
        }
        let end_at = run.at(end);
        let mut inactive = None;
        for _ in 0..360 {
            push_toward(run, stops, [end_at.x, end_at.z]);
            if behaviour(run, h).as_deref() == Some("special - push block inactive") {
                inactive = Some(run.steps);
                break;
            }
        }
        for _ in 0..30 {
            step(run, stops, [false; 4]);
        }
        let at = run.world.actor(h).map_or(Vec3::ZERO, |a| a.position);
        pushed.push(Pushed {
            block,
            inactive,
            at,
        });
    }
    // Across the bridge to the west terrace, up its slope onto the rise.
    let walked = walk(
        run,
        stops,
        &[[56.0, 47.5], [50.5, 50.0], [51.0, 57.0], [56.0, 63.0]],
    );
    (pushed, walked)
}

/// openspec AS.74: level 214's rise past the pods reached on foot over its
/// push blocks. From the east terrace above the pods the hero pushes
/// `Push_Block_Template(1)` off its edge into the channel below, where it
/// drops onto its end marker and goes inactive, a stepping stone level
/// with the terrace; `(2)`, with `(3)` stacked on it, the same, so `(3)`
/// rides down onto it; and `(3)`, pushed from `(1)`'s top, off `(2)` onto
/// the last marker: the three make a bridge from the east terrace to the
/// west one, whose slope climbs onto the rise holding the second bomb
/// spawner. Every push and walk is the stick; every block moves by its own
/// script (`PushBlock_Template.ai`, its track and its drop to
/// `Push_Block_End.ai`). Nothing stops.
#[test]
#[ignore = "reads the Giants dump"]
fn level_214s_push_blocks_bridge_the_way_up_to_the_rise() {
    let started = std::time::Instant::now();
    println!("load at start: {}", load());
    let (mut run, mut stops) = joined_214();
    // Where Setup_002's second trigger leaves him (AS.58's walk).
    run.put(Vec3::new(78.0, -16.5, 61.2), 0.0);
    let (pushed, walked) = up_the_push_blocks(&mut run, &mut stops);
    let at = run.hero().pivot;
    println!(
        "{pushed:?}; walked {walked:?}, on the rise at {at} at step {}; {:.1} s; load at end: {}",
        run.steps,
        started.elapsed().as_secs_f64(),
        load()
    );
    assert_eq!(pushed.len(), 3, "{pushed:?}");
    for p in &pushed {
        let end = PUSHES.iter().find(|(b, ..)| *b == p.block).unwrap().2;
        let end_at = run.at(end);
        assert!(p.inactive.is_some(), "{} pushed in", p.block);
        assert!(
            Vec3::new(p.at.x - end_at.x, 0.0, p.at.z - end_at.z).length() < 0.6
                && (p.at.y - end_at.y).abs() < 0.2,
            "{} on {end}: {}",
            p.block,
            p.at
        );
    }
    assert_eq!(walked, Ok(()), "across the bridge and up");
    assert!(at.y > -12.6, "on the rise: {at}");
    assert!(
        at.distance(Vec3::from_array(SECOND_SPAWNER)) < 4.0,
        "by the second spawner: {at}"
    );
    assert!(stops.is_empty(), "{stops:#?}");
}

/// Where level 214's second bomb spawner stands, on the rise past the
/// pods (metres, its file's place).
const SECOND_SPAWNER: [f32; 3] = [57.78, -12.1, 65.85];
/// On the rise at the ridge along its south edge, facing the castle wall
/// that shuts the two elves in (`Breakable_Stone_Wall_Castle_206(3)`) over
/// it, 9.9 m off: a bomb thrown there lands on it (AS.74).
const BEFORE_THE_ELVES_WALL: ([f32; 3], f32) = ([51.7, -11.8, 70.1], 318.7);
/// On the rise at the ridge, facing the castle wall
/// (`Breakable_Stone_Wall_Castle_206(5)`) that shuts the rise's south part
/// off from the east terrace over it, 7.9 m off.
const BEFORE_THE_RIDGE_WALL: ([f32; 3], f32) = ([58.3, -11.9, 70.6], 29.6);
/// From the rise's ridge down to the elves (AS.74): east off its edge onto
/// the terrace below, round through the opened `(5)` into the rise's south
/// part, west across it through the opened `(3)`.
const ROUND_TO_THE_ELVES: [[f32; 2]; 9] = [
    [63.0, 66.0],
    [68.0, 67.0],
    [68.0, 72.0],
    [64.5, 77.5],
    [60.0, 77.5],
    [55.0, 77.0],
    [50.0, 77.0],
    [46.0, 77.5],
    [42.0, 76.5],
];

/// The second spawner's bomb taken on the stick, then carried to `before`
/// on the stick, turned there to face its wall and thrown at it: the step
/// it was thrown at and the step `wall` went (AS.74).
fn bomb_over_the_ridge(
    run: &mut Run,
    stops: &mut Vec<String>,
    before: ([f32; 3], f32),
    wall: Handle,
) -> (Option<u32>, Option<u32>) {
    let mut bomb = None;
    for _ in 0..600 {
        push_toward(run, stops, [SECOND_SPAWNER[0], SECOND_SPAWNER[2]]);
        bomb = bombs(run)
            .into_iter()
            .find(|b| behaviour(run, *b).as_deref() == Some("following player"));
        if bomb.is_some() {
            break;
        }
    }
    let Some(bomb) = bomb else {
        return (None, None);
    };
    if walk(run, stops, &[[before.0[0], before.0[2]]]).is_err() {
        return (None, None);
    }
    // Turned where he stands, as a tap of the stick would.
    let at = run.hero().pivot;
    throw_at(run, stops, bomb, (at.to_array(), before.1), wall)
}

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
/// on him; both beaten, the helper takes itself off. Nothing stops. The
/// hero is set down where a walk would take him up to the Chompies' fight;
/// from there he walks on the stick (AS.74): up to the rise over the push
/// blocks ([`up_the_push_blocks`]), to the second spawner for a bomb to
/// throw over the ridge along the rise's south edge at the wall between its
/// south part and the terrace, and another at the elves' wall, then down
/// off the rise's east edge and round through both walls to the elves.
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
    // The elves spar behind their wall until a bomb opens it, invulnerable
    // meanwhile (AS.83). Up to the rise on foot over the push blocks
    // (AS.74), from where the Chompies' fight leaves him, then to the
    // second spawner: its bomb his as he comes within its reach.
    let elves_wall = handle(&run, "Breakable_Stone_Wall_Castle_206(3)");
    run.put(Vec3::new(78.0, -16.5, 61.2), 0.0);
    let (pushed, walked) = up_the_push_blocks(&mut run, &mut stops);
    assert!(
        pushed.len() == 3 && pushed.iter().all(|p| p.inactive.is_some()) && walked.is_ok(),
        "up the push blocks: {pushed:?}, {walked:?}"
    );
    // The rise's ridge shuts its south part off: the second spawner's bombs
    // thrown over it open the wall between that part and the terrace, then
    // the elves' wall; down off the rise's east edge, round through the one
    // and across through the other to the elves (AS.74).
    let ridge_wall = handle(&run, "Breakable_Stone_Wall_Castle_206(5)");
    let (_, ridge_opened) =
        bomb_over_the_ridge(&mut run, &mut stops, BEFORE_THE_RIDGE_WALL, ridge_wall);
    let ridge_opened = ridge_opened.expect("the ridge's wall opened");
    for _ in 0..90 {
        step(&mut run, &mut stops, [false; 4]);
    }
    let (_, opened) = bomb_over_the_ridge(&mut run, &mut stops, BEFORE_THE_ELVES_WALL, elves_wall);
    let opened = opened.expect("the elves' wall opened");
    println!(
        "the ridge's wall opened at {ridge_opened}, the elves' at {opened}; health {:?}, the helper {:?}",
        run.health(),
        behaviour(&run, helper)
    );
    assert_eq!(
        walk(&mut run, &mut stops, &ROUND_TO_THE_ELVES),
        Ok(()),
        "round to the elves"
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

/// A creature's hit points as its scripts hold them
/// (`description.creature info.hp`), once its STARTUP has made them.
fn hit_points(run: &Run, h: Handle) -> Option<f32> {
    let vm = &run.vm;
    let id = vm.placement_of(h)?;
    let d = vm.local(id, "description").map(|v| vm.value(v))?;
    let c = vm.field(d, "creature info").map(|v| vm.value(v))?;
    vm.field(c, "hp").map(|v| vm.value(v))?.as_f32()
}

/// openspec AS.83: level 214's two Elf Nears can't be hurt while they spar.
/// At Setup_002's first trigger their helper gives them a range that wakes
/// them and, a tick on, asks combat for each to make them invulnerable and
/// immune to every state. They wake, and so register with combat, the step
/// of the write (the asleep placements' checks after the walk, `guac_script`'s
/// crate docs, The tick), so the helper's writes land: set down among them
/// behind their shut wall, the hero mashing his first attack hurts neither,
/// where on the step-late wake he beat both. Once the second spawner's bomb
/// opens their wall their scene clears it, and both are beaten. Nothing
/// stops.
#[test]
#[ignore = "reads the Giants dump"]
fn level_214s_elves_cant_be_hurt_while_they_spar() {
    let started = std::time::Instant::now();
    println!("load at start: {}", load());
    let (mut run, mut stops) = joined_214();
    let helper = handle(&run, "Setup_002_Controller_Helper");
    let elves: Vec<Handle> = ["Enemy_Elf_Near_S2(3)", "Enemy_Elf_Near_S2(4)"]
        .iter()
        .map(|n| handle(&run, n))
        .collect();
    let elves_wall = handle(&run, "Breakable_Stone_Wall_Castle_206(3)");
    // Its first trigger: the helper wakes them and makes them invulnerable.
    run.put(Vec3::new(82.6, -18.0, 38.6), 0.0);
    for _ in 0..60 {
        step(&mut run, &mut stops, [false; 4]);
    }
    let full: Vec<Option<f32>> = elves.iter().map(|h| hit_points(&run, *h)).collect();
    assert!(full.iter().all(Option::is_some), "both awake: {full:?}");
    // Among them behind their shut wall, mashing (the review beat both by
    // step 1477 on the step-late wake).
    let mashed = fight(&mut run, &mut stops, &elves, 1500);
    let after: Vec<Option<f32>> = elves.iter().map(|h| hit_points(&run, *h)).collect();
    let sparring = behaviour(&run, helper);
    println!(
        "hit points {full:?} -> {after:?} after 1500 steps of mashing, beaten {mashed:?}, the helper {sparring:?}, health {:?}",
        run.health()
    );
    assert_eq!(mashed, [None, None], "neither beaten while they spar");
    assert_eq!(after, full, "neither hurt");
    assert!(!gone(&run, elves_wall), "their wall still shut");
    // Their wall opened by the second spawner's bomb: their scene, then the
    // fight.
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
    let beaten = fight(&mut run, &mut stops, &elves, 3600);
    println!(
        "the wall opened at {opened}, the elves beaten at {beaten:?}; health {:?}; {:.1} s; load at end: {}",
        run.health(),
        started.elapsed().as_secs_f64(),
        load()
    );
    assert!(
        beaten.iter().all(Option::is_some),
        "both beaten: {beaten:?}"
    );
    assert!(stops.is_empty(), "{stops:#?}");
}

/// Where level 214's first ship part floats, on the rise the jump pad
/// reaches (metres, its file's place).
const SHIP_PART_1: [f32; 3] = [29.45, -6.92, 56.37];
/// Where the sail's cutscene sets the hero down on the second island's
/// ship (metres, the run's own read: its second scene's player waypoint).
const ON_THE_SECOND_ISLAND: [f32; 3] = [-383.178, -12.581, 714.414];

/// Whether `h`'s actor is off the world (a placement whose script the
/// listing leaves out has no run to be off).
fn off_the_world(run: &Run, h: Handle) -> bool {
    run.world.actor(h).is_none_or(|a| a.removed)
}

/// What the sail's STARTUP takes off and its SHUTDOWN resets (its
/// `deactivated actors`, openspec AS.77): the second island's ship, an NPC
/// of the island's set and a door of the ship's.
const OFF_FOR_THE_SAIL: [&str; 3] = ["The_Ship_2", "Flynn_Island_2", "Door_(1)"];

/// openspec AS.59: level 214's way from its first island to its second, on
/// the TV's listing (`scripts::PASSAGES`). The hero at the first ship part
/// takes it; it waits on a press (South, pulsed as a hand would: with none
/// it never goes), then goes and its shutdown wakes the sail; the TV skips
/// the sail's cutscene a frame in (`level_end::SKIPPED_CUTSCENES`), which
/// sets him down on the second island's ship and takes the sail off, so
/// Setup_003, which waits for it, can start. AS.77: what the sail took off
/// stands again once its SHUTDOWN resets the set holding it, the ship he
/// stands on among it. Nothing stops.
#[test]
#[ignore = "reads the Giants dump"]
fn level_214s_first_ship_part_sails_to_the_second_island() {
    let started = std::time::Instant::now();
    println!("load at start: {}", load());
    let (mut run, mut stops) = joined_214();
    let part = handle(&run, "Ship_part_1(1)");
    let sail = handle(&run, "214_Recover_First_Part_IGC");
    let off: Vec<Handle> = OFF_FOR_THE_SAIL.iter().map(|n| handle(&run, n)).collect();
    run.put(Vec3::from_array(SHIP_PART_1), 0.0);
    let (mut taken, mut sailed) = (None, None);
    let mut taken_off = vec![false; off.len()];
    for k in 0..1200u32 {
        step(&mut run, &mut stops, [k % 20 < 2, false, false, false]);
        if taken.is_none() && gone(&run, part) {
            taken = Some(run.steps);
        }
        for (seen, h) in taken_off.iter_mut().zip(&off) {
            *seen |= off_the_world(&run, *h);
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
    let standing: Vec<bool> = off.iter().map(|h| !off_the_world(&run, *h)).collect();
    println!(
        "the part taken at {taken:?}, the sail done at {sailed:?}; the hero at {at}, the HUD hidden {}; {OFF_FOR_THE_SAIL:?} taken off {taken_off:?}, standing after {standing:?}; {:.1} s; load at end: {}",
        guacamole::scripts::player_hud_hidden(&run.vm),
        started.elapsed().as_secs_f64(),
        load()
    );
    assert_eq!(taken_off, [true; 3], "taken off for the sail");
    assert_eq!(standing, [true; 3], "back once the sail's done");
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

/// The first island's jump pad (its file's place) and where the walk onto
/// it starts, 4 m south of it in the elves' yard.
const JUMP_PAD: [f32; 3] = [25.149, -10.869, 71.414];
const ONTO_THE_PAD: [f32; 2] = [25.15, 75.4];

/// openspec AS.75: level 214's way off its first island on foot. Both elves
/// beaten, `Jumpad_Activator_First_Island` pans to `PvP_JumpPad_NORMAL(2)`
/// and gives it a range. Stepped onto with the stick let go, the pad
/// launches him at its own pitch and speed (80° and 90 ft/s, its
/// placement's `pitch` and `velocity`) along his facing, and the mover
/// update (`movementUpdate.ai`, on the VM) takes the launch's ground speed
/// off at his air deceleration while no stick pushes, as it does any
/// mover's: he goes up 9 m and lands back on it. Walked onto with the stick
/// held toward `Ship_part_1(1)`, the same launch, the ground speed his
/// stick's, carries him up onto the rise holding the part, a ledge no walk
/// climbs; at the part, a press (South) takes it, and the sail takes him to
/// the second island's ship, which stands (AS.77). The elves are fought as
/// AS.58's are, set down by them; from there every step is the stick.
#[test]
#[ignore = "reads the Giants dump"]
fn level_214s_jump_pad_carries_him_to_the_first_ship_part() {
    let started = std::time::Instant::now();
    println!("load at start: {}", load());
    let (mut run, mut stops) = joined_214();
    let elves: Vec<Handle> = ["Enemy_Elf_Near_S2(3)", "Enemy_Elf_Near_S2(4)"]
        .iter()
        .map(|n| handle(&run, n))
        .collect();
    let elves_wall = handle(&run, "Breakable_Stone_Wall_Castle_206(3)");
    let activator = handle(&run, "Jumpad_Activator_First_Island");
    let part = handle(&run, "Ship_part_1(1)");
    let sail = handle(&run, "214_Recover_First_Part_IGC");
    let ship = handle(&run, "The_Ship_2");
    // Setup_002's first trigger wakes the elves; the second spawner's bomb
    // opens their wall; both beaten (AS.58, AS.83).
    run.put(Vec3::new(82.6, -18.0, 38.6), 0.0);
    for _ in 0..60 {
        step(&mut run, &mut stops, [false; 4]);
    }
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
    let (_, opened) = throw_at(
        &mut run,
        &mut stops,
        bomb.expect("the second spawner's bomb taken"),
        BEFORE_THE_ELVES_WALL,
        elves_wall,
    );
    assert!(opened.is_some(), "the elves' wall opened");
    let beaten = fight(&mut run, &mut stops, &elves, 3600);
    assert!(
        beaten.iter().all(Option::is_some),
        "both beaten: {beaten:?}"
    );
    // The activator's pan, then the pad is given its range.
    let mut panned = None;
    for _ in 0..900 {
        step(&mut run, &mut stops, [false; 4]);
        if gone(&run, activator) {
            panned = Some(run.steps);
            break;
        }
    }
    let panned = panned.expect("the activator's pan done");
    // In the elves' yard (the fight's set-downs can leave him wedged at its
    // edge), then south of the pad on the stick.
    run.put(Vec3::new(24.0, -10.8, 79.0), 0.0);
    assert_eq!(walk(&mut run, &mut stops, &[ONTO_THE_PAD]), Ok(()));
    // Onto the pad, the stick let go once he's on it: up and back down.
    let pad = Vec3::from_array(JUMP_PAD);
    let on_pad = |run: &Run| {
        let at = run.hero().pivot;
        Vec3::new(at.x - pad.x, 0.0, at.z - pad.z).length() < 1.0
    };
    for _ in 0..240 {
        if on_pad(&run) {
            break;
        }
        push_toward(&mut run, &mut stops, [pad.x, pad.z]);
    }
    let (mut top, mut left_at, mut bounces, mut was) = (f32::MIN, None, 0, 0.0);
    let mut drift = 0.0f32;
    for _ in 0..240 {
        step(&mut run, &mut stops, [false; 4]);
        let at = run.hero().pivot;
        top = top.max(at.y);
        if left_at.is_none() && at.y > pad.y + 0.2 {
            left_at = Some(run.steps);
        }
        let vy = run.hero().walk.velocity.y;
        bounces += u32::from(was < 0.0 && vy > 0.0);
        was = vy;
        drift = drift.max(Vec3::new(at.x - pad.x, 0.0, at.z - pad.z).length());
    }
    println!(
        "let go on the pad: launched at {left_at:?}, {:.2} m up, {bounces} bounces more in 240 steps, at most {drift:.2} m off it",
        top - pad.y
    );
    assert!(left_at.is_some(), "launched");
    assert!(bounces >= 2, "bouncing on it: {bounces}");
    assert!(drift < 1.5, "straight up and down over it: {drift}");
    assert!(
        (8.0..10.0).contains(&(top - pad.y)),
        "9 m up: {}",
        top - pad.y
    );
    // Off it: the stick toward the yard as he bounces again, then at rest.
    for _ in 0..600 {
        push_toward(&mut run, &mut stops, ONTO_THE_PAD);
        let at = run.hero().pivot;
        if Vec3::new(at.x - pad.x, 0.0, at.z - pad.z).length() > 2.5
            && run.hero().walk.velocity.y == 0.0
            && at.y < pad.y + 0.3
        {
            break;
        }
    }
    assert_eq!(walk(&mut run, &mut stops, &[ONTO_THE_PAD]), Ok(()));
    // Onto it again with the stick held toward the part.
    let target = [SHIP_PART_1[0], SHIP_PART_1[2]];
    let (mut launched, mut landed, mut taken, mut sailed) = (None, None, None, None);
    let mut flight = Vec::new();
    for k in 0..1800u32 {
        let at = run.hero().pivot;
        // South pulsed as a hand would once he's by the part (with none it
        // never goes, AS.59).
        let by_part = Vec3::new(at.x - target[0], 0.0, at.z - target[1]).length() < 6.0;
        if by_part && taken.is_none() && landed.is_some() {
            step(&mut run, &mut stops, [k % 20 < 2, false, false, false]);
        } else if sailed.is_none() && taken.is_none() {
            push_toward(&mut run, &mut stops, target);
        } else {
            step(&mut run, &mut stops, [false; 4]);
        }
        let at = run.hero().pivot;
        if launched.is_none() && at.y > pad.y + 0.2 {
            launched = Some((run.steps, run.hero().walk.velocity));
        }
        if launched.is_some() && landed.is_none() {
            flight.push(at);
            if run.hero().walk.velocity.y.abs() < 1e-3 && at.y > SHIP_PART_1[1] - 0.5 {
                landed = Some((run.steps, at));
            }
        }
        if taken.is_none() && gone(&run, part) {
            taken = Some(run.steps);
        }
        if sailed.is_none() && gone(&run, sail) {
            sailed = Some(run.steps);
        }
        if sailed.is_some() && run.steps > sailed.unwrap() + 120 {
            break;
        }
    }
    let apex = flight.iter().map(|p| p.y).fold(f32::MIN, f32::max);
    let at = run.hero().pivot;
    println!(
        "on the stick: launched at {launched:?}, apex {apex:.2} (pad {:.2}), landed at {landed:?}, the part taken at {taken:?}, the sail done at {sailed:?}; the hero at {at}, The_Ship_2 standing {}; the pan done at {panned}; {:.1} s; load at end: {}",
        pad.y,
        !off_the_world(&run, ship),
        started.elapsed().as_secs_f64(),
        load()
    );
    let (_, down) = landed.expect("landed on the part's rise");
    assert!(
        down.y > SHIP_PART_1[1] - 0.5 && down.z < pad.z - 3.0,
        "up on the part's rise, toward it: {down}"
    );
    assert!(taken.is_some(), "the part taken");
    assert!(sailed.is_some(), "the sail done");
    assert!(
        at.distance(Vec3::from_array(ON_THE_SECOND_ISLAND)) < 2.0,
        "on the second island's ship: {at}"
    );
    assert!(!off_the_world(&run, ship), "The_Ship_2 standing under him");
    assert!(stops.is_empty(), "{stops:#?}");
}
