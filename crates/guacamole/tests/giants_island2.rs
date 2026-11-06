//! Level 214's second island past the second locked door (openspec AS.97,
//! AS.98, AS.104), headless with guacamole's host on the TV's listing for
//! the level. Ignored by default; they read `GUAC_GIANTS_DUMP` (process
//! env, else `.env.dev`):
//!
//! `cargo test -p guacamole --profile disc --test giants_island2 -- --ignored --nocapture --test-threads=1`
//!
//! No recording of 214 exists: every step is the level's own scripts on the
//! TV's listing, the hero set down past the second door (`giants_fights`
//! walks there) and on the stick from there.

use guac_motion::seam::Vec3;
use guac_motion::stick::{DEAD_ZONE, Stick};
use guac_script::{Handle, Mode, Value};
use guacamole::script_engine::Pad;

mod common;

use common::*;

const LEVEL_214: &str = "level_214_tutotial_2";

fn setup() -> Setup {
    Setup {
        interface: true,
        props: true,
        sounds: true,
        ..Setup::default()
    }
}

/// The level's run with Eruptor joined and his arrival played out, and what
/// stopped on the way.
fn joined_214() -> (Run, Vec<String>) {
    let (mut run, _found) = run_on_level(Mode::Play, LEVEL_214, setup());
    let mut stops = Vec::new();
    for _ in 0..2 {
        stops.extend(run.step_play().unwrap());
    }
    run.join().unwrap_or_else(|h| panic!("{h}"));
    stops.extend(skip_214s_intro(&mut run));
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
/// the step before's, and `actions` held.
fn push_with(run: &mut Run, stops: &mut Vec<String>, to: [f32; 2], actions: [bool; 4]) {
    let at = run.hero().pivot;
    let heading = (to[0] - at.x).atan2(to[1] - at.z).to_degrees();
    run.bench.pads[0] = Pad {
        stick: stick_for(run.bench.view, heading, 1.0),
        actions,
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
            push_with(run, stops, to, [false; 4]);
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

/// The placement named `name` in the level's file, by handle.
fn handle(run: &Run, name: &str) -> Handle {
    run.placed(name).0
}

/// Whether `h` is beaten: off the run or its actor gone.
fn gone(run: &Run, h: Handle) -> bool {
    run.world.actor(h).is_none_or(|a| a.removed)
        || run.vm.placement_of(h).is_none_or(|id| run.vm.removed(id))
}

/// What a fight came to: the step `until` went, the foes beaten and the
/// bites with their steps.
#[derive(Debug, Default)]
struct Fought {
    done: Option<u32>,
    beaten: usize,
    bites: Vec<(u32, f32)>,
}

/// The foes standing and awake whose names are in `names` (a clone runs
/// under its template's name), nearest first, the same height as him
/// within `reach` metres.
fn foes(run: &Run, names: &[&str], reach: f32) -> Vec<(Handle, Vec3)> {
    let me = run.hero().pivot;
    let mut out: Vec<(Handle, Vec3)> = run
        .world
        .creatures()
        .into_iter()
        .chain(run.world.clones())
        .filter(|h| names.contains(&run.world.name(*h).as_str()) && !gone(run, *h))
        .filter_map(|h| {
            let a = run.world.actor(h)?;
            (a.active && (a.position.y - me.y).abs() < reach).then_some((h, a.position))
        })
        .collect();
    out.sort_by(|a, b| a.1.distance(me).total_cmp(&b.1.distance(me)));
    out.dedup_by_key(|f| f.0);
    out
}

/// Up to `steps` steps on the stick at the nearest of `names` standing, as
/// a hand would: he walks at it, turning to face it, and mashes his first
/// attack within 4.5 m, until `done` holds. With none standing he waits by
/// `wait`.
fn fight_until(
    run: &mut Run,
    stops: &mut Vec<String>,
    done: impl Fn(&Run) -> bool,
    names: &[&str],
    wait: [f32; 2],
    steps: u32,
) -> Fought {
    let mut out = Fought::default();
    let mut seen = std::collections::HashSet::new();
    let mut health = run.health().unwrap_or(0.0);
    for k in 0..steps {
        if done(run) {
            out.done = Some(run.steps);
            break;
        }
        let standing = foes(run, names, 3.5);
        seen.extend(standing.iter().map(|f| f.0));
        let me = run.hero().pivot;
        match standing.first().map(|f| f.1) {
            Some(at) if (at.x - me.x).hypot(at.z - me.z) > 4.5 => {
                push_with(run, stops, [at.x, at.z], [false; 4]);
            }
            Some(at) if k % 8 == 0 => push_with(run, stops, [at.x, at.z], [false; 4]),
            Some(_) => step(run, stops, [k % 4 < 2, false, false, false]),
            None if (wait[0] - me.x).hypot(wait[1] - me.z) > 1.5 => {
                push_with(run, stops, wait, [false; 4]);
            }
            None => step(run, stops, [false; 4]),
        }
        let Some(now) = run.health() else {
            break;
        };
        if now < health {
            out.bites.push((run.steps, health - now));
        }
        health = now;
    }
    out.beaten = seen.iter().filter(|h| gone(run, **h)).count();
    out
}

/// Past the second locked door, by `Setup_004B_Trigger`, across the ground.
const BY_004B_TRIGGER: [f32; 2] = [-232.11, 722.4];
/// The yard's middle, under the closets' ledge.
const IN_004B_YARD: [f32; 2] = [-212.0, 726.0];

/// What Setup_004B's fight lists: its Chompies, its lobber and what its
/// closets let out.
const FOES_004B: [&str; 10] = [
    "Enemy_Chompy(4)",
    "Enemy_Chompy(5)",
    "Enemy_Chompy(7)",
    "Enemy_Chompy(9)",
    "Enemy_Chompy(10)",
    "Enemy_Chompy(11)",
    "Enemy_Chompy(12)",
    "Enemy_Elf_Lobber(13)",
    "Enemy_Elf_Near_S2(0)",
    "Enemy_Elf_Lobber(0)",
];

/// openspec AS.97: level 214's Setup_004B, from its trigger to its end, on
/// the stick. Set down past the second locked door; the trigger queues the
/// lobber down off his ledge and plays his enemy intro; the closets let out
/// their eight waves, each on the last's landing, the Chompies at him; every
/// enemy dead, the controller wakes the gate's swing and goes. Nothing
/// stops.
#[test]
#[ignore = "reads the Giants dump"]
fn level_214s_setup_004b_is_fought_from_its_trigger_to_its_end() {
    let started = std::time::Instant::now();
    println!("load at start: {}", load());
    let (mut run, mut stops) = joined_214();
    let controller = handle(&run, "Setup_004B_Controller");
    let lobber = handle(&run, "Enemy_Elf_Lobber(13)");
    run.put(Vec3::new(BY_004B_TRIGGER[0] - 4.0, -22.3, 722.4), 90.0);
    assert_eq!(walk(&mut run, &mut stops, &[BY_004B_TRIGGER]), Ok(()));
    let from = run.steps;
    let health_before = run.health().unwrap();
    // His walk down: a hop along his ledge, then off it to his path's end,
    // where he stands and shoots. Under the intro, the first of the fight.
    let intro = wait_for(
        &mut run,
        &mut stops,
        |run| {
            run.world
                .actor(lobber)
                .is_some_and(|a| a.position.y < -18.0 && a.velocity.length() < 1.0)
        },
        600,
    )
    .expect("the lobber down off his ledge");
    let landed = run.world.actor(lobber).unwrap().position;
    let path_end = Vec3::new(-196.01, -18.21, 726.83);
    assert!(
        (landed - path_end).length() < 1.5,
        "at his path's end: {landed}"
    );
    let f = fight_until(
        &mut run,
        &mut stops,
        |run| gone(run, controller),
        &FOES_004B,
        IN_004B_YARD,
        9000,
    );
    let clones = run
        .world
        .clones()
        .into_iter()
        .filter(|h| {
            ["Enemy_Elf_Near_S2(0)", "Enemy_Elf_Lobber(0)"].contains(&run.world.name(*h).as_str())
        })
        .count();
    println!(
        "from {from}, the lobber down by {intro} at {landed}, to {:?} ({:.1} s of game): {} beaten, {clones} let out of the closets, bites {:?}; health {health_before} -> {:?}, experience {:?}; the lobber gone {}; {:.1} s; load at end: {}",
        f.done,
        f.done.map_or(0.0, |d| (d - from) as f32 / 60.0),
        f.beaten,
        f.bites,
        run.health(),
        run.figure_values().map(|v| v.xp),
        gone(&run, lobber),
        started.elapsed().as_secs_f64(),
        load()
    );
    assert!(
        f.done.is_some(),
        "the controller done: {:?}",
        behaviour(&run, controller)
    );
    assert!(gone(&run, lobber), "the lobber beaten");
    assert_eq!(clones, 8, "the closets let eight out");
    assert!(stops.is_empty(), "{stops:#?}");
}

/// Through the gate Setup_004B's end opens, into Setup_005's yard, across
/// the ground: where its first trigger plane has fired.
const THROUGH_THE_GATE: [[f32; 2]; 4] = [
    [-214.8, 760.0],
    [-214.5, 770.0],
    [-216.0, 776.0],
    [-225.0, 783.0],
];
const IN_005_YARD: [f32; 2] = [-235.0, 781.0];
/// What chases the Mabu, then him.
const FOES_005_YARD: [&str; 5] = [
    "Enemy_Chompy(41)",
    "Enemy_Chompy(42)",
    "Enemy_Chompy(43)",
    "Enemy_Chompy(44)",
    "Enemy_Chompy(45)",
];
/// Up the yard's west side to the house the Mabu ran into, as he ran.
const TO_THE_MABUS_HOUSE: [[f32; 2]; 5] = [
    [-245.0, 781.0],
    [-255.4, 776.5],
    [-258.0, 780.2],
    [-260.6, 784.0],
    [-268.15, 783.63],
];
/// `Interior_Controller_Island_2_3`'s box, its door's far side.
const HOUSE_BOX: [f32; 2] = [-272.31, 784.29];
/// By the south room's key, and at its way out.
const BY_THE_KEY: [f32; 2] = [384.15, 142.5];
const ROOM_EXIT: [f32; 2] = [403.39, 133.33];
/// Back out of the house to the yard's middle, where the helper's fight
/// comes on.
const BACK_TO_THE_YARD: [[f32; 2]; 4] = [
    [-260.6, 784.0],
    [-258.0, 780.2],
    [-255.4, 776.5],
    [-245.0, 781.0],
];
const IN_THE_HELPERS_YARD: [f32; 2] = [-240.0, 795.0];
/// What the helper wakes in the yard, and on the first ledge.
const FOES_HELPER_YARD: [&str; 7] = [
    "Enemy_TreeRex(3)",
    "Enemy_Elf_Near_S2(1)",
    "Enemy_Chompy(13)",
    "Enemy_Chompy(14)",
    "Enemy_Chompy(15)",
    "Enemy_Chompy(16)",
    "Enemy_Chompy(17)",
];
const FOES_LEDGE: [&str; 1] = ["Enemy_Elf_Lobber(1)"];
const FOES_TOP: [&str; 2] = ["Enemy_Elf_Lobber(2)", "Enemy_Elf_Lobber(3)"];
/// Short of the yard's jump pad, and on the ledge it reaches; short of the
/// ledge's pad, and on the top it reaches, by the second key.
const SHORT_OF_THE_PAD: [f32; 2] = [-236.5, 807.0];
const ON_THE_LEDGE: [f32; 2] = [-238.0, 821.0];
const SHORT_OF_THE_SECOND_PAD: [f32; 2] = [-239.3, 823.2];
const ON_THE_TOP: [f32; 2] = [-243.0, 832.0];
const KEY_4: [f32; 2] = [-242.6, 832.1];
/// Down off the top to the yard, by the door's second lock.
const DOWN_TO_THE_DOOR: [[f32; 2]; 4] = [
    [-244.0, 826.0],
    [-243.0, 817.0],
    [-247.0, 808.0],
    [-251.0, 801.0],
];
/// Past the door, within Auric's 80 ft, then by the second ship part.
const PAST_THE_TWO_LOCKS: [[f32; 2]; 2] = [[-262.0, 804.0], [-275.0, 806.0]];
const BY_THE_SHIP_PART: [f32; 2] = [-291.1, 817.5];

/// Held toward `to` until he stands within 1 m of it at the height of
/// `floor` (a jump pad's flight), at most `steps`; the step he did.
fn fly_to(
    run: &mut Run,
    stops: &mut Vec<String>,
    to: [f32; 2],
    floor: f32,
    steps: u32,
) -> Option<u32> {
    for _ in 0..steps {
        let at = run.hero().pivot;
        if (to[0] - at.x).hypot(to[1] - at.z) <= 1.0 && (at.y - floor).abs() < 0.5 {
            return Some(run.steps);
        }
        push_with(run, stops, to, [false; 4]);
    }
    None
}

/// `steps` steps at rest, or until `done` holds; the step it did.
fn wait_for(
    run: &mut Run,
    stops: &mut Vec<String>,
    done: impl Fn(&Run) -> bool,
    steps: u32,
) -> Option<u32> {
    for _ in 0..steps {
        if done(run) {
            return Some(run.steps);
        }
        step(run, stops, [false; 4]);
    }
    done(run).then_some(run.steps)
}

/// Setup_004B fought from its trigger, the step it ended.
fn setup_004b(run: &mut Run, stops: &mut Vec<String>) -> u32 {
    let controller = handle(run, "Setup_004B_Controller");
    run.put(Vec3::new(BY_004B_TRIGGER[0] - 4.0, -22.3, 722.4), 90.0);
    assert_eq!(walk(run, stops, &[BY_004B_TRIGGER]), Ok(()));
    fight_until(
        run,
        stops,
        |run| gone(run, controller),
        &FOES_004B,
        IN_004B_YARD,
        9000,
    )
    .done
    .expect("Setup_004B fought")
}

/// Into Setup_005's yard: the Chompies chasing the Mabu turn on him and are
/// beaten. The step the last went.
fn the_chase(run: &mut Run, stops: &mut Vec<String>) -> u32 {
    let controller = handle(run, "Setup_005_Controller");
    assert_eq!(
        walk(run, stops, &THROUGH_THE_GATE),
        Ok(()),
        "through the gate"
    );
    assert!(gone(run, controller), "its controller done, the chase on");
    let chompies: Vec<Handle> = FOES_005_YARD.iter().map(|n| handle(run, n)).collect();
    let f = fight_until(
        run,
        stops,
        |run| chompies.iter().all(|h| gone(run, *h)),
        &FOES_005_YARD,
        IN_005_YARD,
        2400,
    );
    f.done.expect("the yard's Chompies beaten")
}

/// Into the house the Mabu ran into and its room under the island: the
/// step he landed.
fn into_the_south_room(run: &mut Run, stops: &mut Vec<String>) -> u32 {
    let house = handle(run, "Interior_Controller_Island_2_3");
    assert_eq!(
        walk(run, stops, &TO_THE_MABUS_HOUSE),
        Ok(()),
        "to the house"
    );
    let landed = run.world.landed.len();
    for _ in 0..300 {
        if behaviour(run, house).as_deref() == Some("2. fade out fade in") {
            break;
        }
        push_with(run, stops, HOUSE_BOX, [false; 4]);
    }
    assert_eq!(
        behaviour(run, house).as_deref(),
        Some("2. fade out fade in"),
        "the house takes him"
    );
    wait_for(run, stops, |run| run.world.landed.len() > landed, 120).expect("he lands inside")
}

/// openspec AS.104: level 214's south room, from Setup_005's yard. Its
/// Chompies beaten, into the house the Mabu ran into
/// (`Interior_Controller_Island_2_3`): its room's set wakes the key spawner
/// and `Activate_Auric`, the other house's the workbenches and the food
/// (both rooms are one room's floor, so the way on is this door); his taking
/// the key sets `Setup_005_Trigger(2)` off, which readies the helper and
/// goes, and `Activate_Auric` wakes Auric's scene and goes. Out by its exit
/// into the yard. Nothing stops.
#[test]
#[ignore = "reads the Giants dump"]
fn level_214s_south_room_is_walked_in_and_out_of_setup_005s_yard() {
    let started = std::time::Instant::now();
    println!("load at start: {}", load());
    let (mut run, mut stops) = joined_214();
    let fought = setup_004b(&mut run, &mut stops);
    let yard = the_chase(&mut run, &mut stops);
    let npc = handle(&run, "Setup_005_NPC");
    let key = handle(&run, "Key Spawner(1)s(2)");
    let trigger = handle(&run, "Setup_005_Trigger(2)");
    let auric = handle(&run, "Activate_Auric");
    let scene = handle(&run, "Moneybags_end");
    let helper = handle(&run, "Setup_005_Controller_Helper");
    let house = handle(&run, "Interior_Controller_Island_2_3");
    assert!(
        run.world.actor(key).is_some_and(|a| !a.active),
        "the key's spawner asleep until its room's door"
    );
    let landed = into_the_south_room(&mut run, &mut stops);
    let inside = run.hero().pivot;
    assert!((inside.y + 300.0).abs() < 1.0, "under the island: {inside}");
    assert_eq!(
        walk(&mut run, &mut stops, &[BY_THE_KEY]),
        Ok(()),
        "to the key"
    );
    let taken = wait_for(&mut run, &mut stops, |run| gone(run, trigger), 240)
        .expect("the key taken, the trigger goes");
    assert!(gone(&run, auric), "Activate_Auric went as he came in");
    assert_eq!(
        behaviour(&run, scene).as_deref(),
        Some("wait for approach"),
        "Auric's scene woken"
    );
    let landed_out = run.world.landed.len();
    for _ in 0..400 {
        if run.world.landed.len() > landed_out {
            break;
        }
        push_with(&mut run, &mut stops, ROOM_EXIT, [false; 4]);
    }
    let out = run
        .world
        .landed
        .get(landed_out)
        .copied()
        .expect("he lands outside");
    assert_eq!(
        walk(&mut run, &mut stops, &BACK_TO_THE_YARD),
        Ok(()),
        "to the yard"
    );
    let awake = wait_for(
        &mut run,
        &mut stops,
        |run| run.world.actor(helper).is_some_and(|a| a.active),
        120,
    );
    println!(
        "Setup_004B done at {fought}, the yard's Chompies beaten at {yard}; landed in the room at {landed} at {inside}; the key taken and the trigger gone at {taken}; out at {:?} facing {}, the house {:?}, the helper awake at {awake:?}, {:?}; health {:?}; {:.1} s; load at end: {}",
        out.at,
        out.facing,
        behaviour(&run, house),
        behaviour(&run, helper),
        run.health(),
        started.elapsed().as_secs_f64(),
        load()
    );
    assert!(gone(&run, npc), "the Mabu ran into the house and went");
    assert!(
        (out.at.y + 20.7).abs() < 1.0,
        "out by the door: {:?}",
        out.at
    );
    assert!(awake.is_some(), "the helper readied");
    assert!(stops.is_empty(), "{stops:#?}");
}

/// openspec AS.98: level 214's way from Setup_005's yard to its second ship
/// part, on the stick. Setup_004B, the yard's Chompies and the south room as
/// AS.97's and AS.104's tests have them; back in the yard the helper's Tree
/// Rex, Elf Near and five Chompies come on, and beaten the first activator
/// pans to the yard's jump pad and wakes it; onto the ledge, its lobber
/// beaten, the second pans to the ledge's pad; up to the top, its two
/// lobbers beaten, the second key taken; down to the door with two locks,
/// △ by its second lock turns both; past it Auric's scene plays, his
/// teleporter wakes the ship part, and a press by it takes it, which wakes
/// the sail to the third island, on the health he brings, no top-up.
/// Nothing stops.
#[test]
#[ignore = "reads the Giants dump"]
fn level_214s_second_island_is_walked_to_its_second_ship_part() {
    let started = std::time::Instant::now();
    println!("load at start: {}", load());
    let (mut run, mut stops) = joined_214();
    let fought = setup_004b(&mut run, &mut stops);
    let yard = the_chase(&mut run, &mut stops);
    let _ = into_the_south_room(&mut run, &mut stops);
    assert_eq!(
        walk(&mut run, &mut stops, &[BY_THE_KEY]),
        Ok(()),
        "to the key"
    );
    let landed = run.world.landed.len();
    for _ in 0..400 {
        if run.world.landed.len() > landed {
            break;
        }
        push_with(&mut run, &mut stops, ROOM_EXIT, [false; 4]);
    }
    assert_eq!(
        walk(&mut run, &mut stops, &BACK_TO_THE_YARD),
        Ok(()),
        "to the yard"
    );
    assert_eq!(
        walk(&mut run, &mut stops, &[IN_THE_HELPERS_YARD]),
        Ok(()),
        "into the helper's yard"
    );
    let helper = handle(&run, "Setup_005_Controller_Helper");
    let rex = handle(&run, "Enemy_TreeRex(3)");
    let near = handle(&run, "Enemy_Elf_Near_S2(1)");
    let pad = handle(&run, "PvP_JumpPad_SUPER(2)");
    let f = fight_until(
        &mut run,
        &mut stops,
        |run| gone(run, rex) && gone(run, near),
        &FOES_HELPER_YARD,
        [-240.0, 800.0],
        3600,
    );
    if f.done.is_none() {
        for (h, at) in foes(&run, &FOES_HELPER_YARD, 99.0) {
            println!(
                "standing: {} at {at} {:?}",
                run.world.name(h),
                behaviour(&run, h)
            );
        }
        println!("hero at {} health {:?}", run.hero().pivot, run.health());
    }
    let yard_done = f.done.expect("the helper's yard beaten");
    let health_after_yard = run.health();
    // The first activator's pan to the jump pad.
    let pad_awake = wait_for(
        &mut run,
        &mut stops,
        |run| run.world.actor(pad).is_some_and(|a| a.active) && cutscene_name(run).is_none(),
        900,
    )
    .expect("the yard's jump pad woken");
    assert_eq!(walk(&mut run, &mut stops, &[SHORT_OF_THE_PAD]), Ok(()));
    let on_ledge = fly_to(&mut run, &mut stops, ON_THE_LEDGE, -21.6, 420).expect("onto the ledge");
    let lobber = handle(&run, "Enemy_Elf_Lobber(1)");
    let pad2 = handle(&run, "PvP_JumpPad_SUPER(1)");
    fight_until(
        &mut run,
        &mut stops,
        |run| gone(run, lobber),
        &FOES_LEDGE,
        ON_THE_LEDGE,
        1800,
    )
    .done
    .expect("the ledge's lobber beaten");
    let ledge_done = run.steps;
    wait_for(
        &mut run,
        &mut stops,
        |run| run.world.actor(pad2).is_some_and(|a| a.active) && cutscene_name(run).is_none(),
        900,
    )
    .expect("the ledge's jump pad woken");
    // He may have dropped off the ledge in the fight: up again by the yard's.
    if run.hero().pivot.y < -23.0 {
        assert_eq!(walk(&mut run, &mut stops, &[SHORT_OF_THE_PAD]), Ok(()));
        fly_to(&mut run, &mut stops, ON_THE_LEDGE, -21.6, 420).expect("onto the ledge again");
    }
    assert_eq!(
        walk(&mut run, &mut stops, &[SHORT_OF_THE_SECOND_PAD]),
        Ok(())
    );
    let on_top = fly_to(&mut run, &mut stops, ON_THE_TOP, -15.1, 420).expect("onto the top");
    let top: Vec<Handle> = FOES_TOP.iter().map(|n| handle(&run, n)).collect();
    fight_until(
        &mut run,
        &mut stops,
        |run| top.iter().all(|h| gone(run, *h)),
        &FOES_TOP,
        ON_THE_TOP,
        2400,
    )
    .done
    .expect("the top's lobbers beaten");
    let top_done = run.steps;
    assert_eq!(walk(&mut run, &mut stops, &[KEY_4]), Ok(()), "the key");
    assert_eq!(
        walk(&mut run, &mut stops, &DOWN_TO_THE_DOOR),
        Ok(()),
        "to the door"
    );
    let door = handle(&run, "Door_Template_Skylanders_2(3)");
    let mut opened = None;
    for k in 0..900u32 {
        step(&mut run, &mut stops, [false, false, false, k % 20 < 2]);
        if behaviour(&run, door).as_deref() == Some("done") {
            opened = Some(run.steps);
            break;
        }
    }
    let opened = opened.expect("the door of two locks opened to its keys");
    assert_eq!(walk(&mut run, &mut stops, &PAST_THE_TWO_LOCKS[..1]), Ok(()));
    let scene = handle(&run, "Moneybags_end");
    let part = handle(&run, "Ship_part_2(1)");
    let sail = handle(&run, "214_Final_Island_IGC");
    let scene_over = wait_for(
        &mut run,
        &mut stops,
        |run| gone(run, scene) && cutscene_name(run).is_none(),
        1800,
    )
    .expect("Auric's scene played");
    let part_awake = wait_for(
        &mut run,
        &mut stops,
        |run| run.world.actor(part).is_some_and(|a| a.active),
        240,
    )
    .expect("the teleporter wakes the part");
    assert_eq!(walk(&mut run, &mut stops, &PAST_THE_TWO_LOCKS[1..]), Ok(()));
    // Near it the part holds him (its own scene) and waits for a press.
    let mut taken = None;
    for k in 0..900u32 {
        push_with(
            &mut run,
            &mut stops,
            BY_THE_SHIP_PART,
            [k % 20 < 2, false, false, false],
        );
        if gone(&run, part) {
            taken = Some(run.steps);
            break;
        }
    }
    let taken = taken.expect("the ship part taken");
    let sailing = wait_for(
        &mut run,
        &mut stops,
        |run| run.world.actor(sail).is_some_and(|a| a.active) && cutscene_name(run).is_some(),
        120,
    )
    .expect("its sail woken");
    println!(
        "Setup_004B at {fought}, the yard's Chompies at {yard}, the helper's yard at {yard_done} (bites {:?}, health {health_after_yard:?}), the pad awake at {pad_awake}, on the ledge at {on_ledge}, its lobber at {ledge_done}, on the top at {on_top}, its lobbers at {top_done}, the door opened at {opened}, Auric's scene over at {scene_over}, the part awake at {part_awake} and taken at {taken}, the sail on at {sailing} ({:?}); helper {:?}; health {:?}, experience {:?}; {:.1} s; load at end: {}",
        f.bites,
        cutscene_name(&run),
        behaviour(&run, helper),
        run.health(),
        run.figure_values().map(|v| v.xp),
        started.elapsed().as_secs_f64(),
        load()
    );
    assert!(stops.is_empty(), "{stops:#?}");
}

/// Where the keys' pan wakes: by the door of two locks' second lock, across
/// the ground, from the yard.
const TO_THE_SECOND_LOCK: [[f32; 2]; 2] = [[-240.0, 795.0], [-247.0, 801.0]];
const BY_THE_SECOND_LOCK: [f32; 2] = [-251.0, 801.0];

/// Up to the door of two locks from the yard, the keys' pan (`PAN- Show
/// Key_2(1)`, a 20 ft range) played out with him held: the steps it came
/// on and went.
fn panned_at_the_door(run: &mut Run, stops: &mut Vec<String>) -> (u32, u32) {
    assert_eq!(walk(run, stops, &TO_THE_SECOND_LOCK), Ok(()), "to the door");
    let mut on = None;
    for _ in 0..120 {
        push_with(run, stops, BY_THE_SECOND_LOCK, [false; 4]);
        if cutscene_name(run).is_some() {
            on = Some(run.steps);
            break;
        }
    }
    let on = on.expect("the keys' pan comes on");
    let over = wait_for(run, stops, |run| cutscene_name(run).is_none(), 900).expect("it ends");
    (on, over)
}

/// openspec AS.98: a hero at the door of two locks short of a key is panned
/// to what he lacks and let go. With neither key taken it shows both, with
/// the south room's taken only the top's, and the door stays shut. Nothing
/// stops.
#[test]
#[ignore = "reads the Giants dump"]
fn level_214s_door_of_two_locks_pans_to_the_keys_he_lacks() {
    let started = std::time::Instant::now();
    println!("load at start: {}", load());
    let (mut run, mut stops) = joined_214();
    setup_004b(&mut run, &mut stops);
    the_chase(&mut run, &mut stops);
    let (on, over) = panned_at_the_door(&mut run, &mut stops);
    let door = handle(&run, "Door_Template_Skylanders_2(3)");
    assert!(
        over - on > 300,
        "both keys shown, a pan each: {on} to {over}"
    );
    assert_ne!(behaviour(&run, door).as_deref(), Some("done"), "still shut");
    let at = run.hero().pivot;
    assert_eq!(walk(&mut run, &mut stops, &[[-240.0, 795.0]]), Ok(()));
    println!(
        "neither key: the pan {on} to {over} with him at {at}, let go, back to {}; {:.1} s; load at end: {}",
        run.hero().pivot,
        started.elapsed().as_secs_f64(),
        load()
    );
    assert!(stops.is_empty(), "{stops:#?}");

    let (mut run, mut stops) = joined_214();
    setup_004b(&mut run, &mut stops);
    the_chase(&mut run, &mut stops);
    into_the_south_room(&mut run, &mut stops);
    assert_eq!(
        walk(&mut run, &mut stops, &[BY_THE_KEY]),
        Ok(()),
        "to the key"
    );
    let landed = run.world.landed.len();
    for _ in 0..400 {
        if run.world.landed.len() > landed {
            break;
        }
        push_with(&mut run, &mut stops, ROOM_EXIT, [false; 4]);
    }
    assert_eq!(
        walk(&mut run, &mut stops, &BACK_TO_THE_YARD),
        Ok(()),
        "to the yard"
    );
    let (on_one, over_one) = panned_at_the_door(&mut run, &mut stops);
    println!(
        "the room's key taken: the pan {on_one} to {over_one}; {:.1} s; load at end: {}",
        started.elapsed().as_secs_f64(),
        load()
    );
    assert!(
        over_one - on_one < over - on,
        "one key shown is the shorter pan: {} against {}",
        over_one - on_one,
        over - on
    );
    assert!(stops.is_empty(), "{stops:#?}");
}

/// The combat system's food counter, kills to the next drop.
fn set_food_counter(run: &mut Run, to: i32) {
    let g = run
        .vm
        .program()
        .global_named("combat control data")
        .expect("the combat system");
    let data = run.vm.global_peek(g).expect("its data");
    assert!(run.vm.set_field(data, "food counter", Value::Int(to)));
}

/// The food out of a drop: the clones of the food set's templates.
fn dropped(run: &Run) -> Vec<Handle> {
    run.world
        .clones()
        .into_iter()
        .filter(|h| run.world.name(*h).starts_with("food_") && !gone(run, *h))
        .collect()
}

/// openspec AS.133: the level's food, by its script. A hurt hero walking
/// into a watermelon on the first island heals by 100 (the script's amount
/// at the default difficulty) and it goes; it didn't stop him on the way in.
/// The pie in Setup_005's yard takes itself off, woken, as its script does
/// until a later adventure is done. Nothing stops.
#[test]
#[ignore = "reads the Giants dump"]
fn level_214s_placed_food_heals_a_hurt_hero_and_goes() {
    let started = std::time::Instant::now();
    println!("load at start: {}", load());
    let (mut run, mut stops) = joined_214();
    let melon = handle(&run, "Food_Watermelon(1)");
    let pie = handle(&run, "Batterson_Special_Pie(1)");
    let at = run.world.actor(melon).unwrap().position;
    run.put(Vec3::new(at.x, at.y + 0.2, at.z - 4.4), 0.0);
    for _ in 0..30 {
        step(&mut run, &mut stops, [false; 4]);
    }
    assert_eq!(
        behaviour(&run, melon).as_deref(),
        Some("wait for player to collect me")
    );
    let minion = run.minion.expect("he's joined");
    assert!(guacamole::scripts::set_health(&mut run.vm, minion, 100));
    assert_eq!(
        walk(&mut run, &mut stops, &[[at.x, at.z]]),
        Ok(()),
        "it stops no one"
    );
    let eaten = wait_for(&mut run, &mut stops, |run| gone(run, melon), 180).expect("eaten");
    let health = run.health().unwrap();
    assert_eq!(health, 200.0, "100 more");
    assert!(!gone(&run, pie), "asleep until a hero's in its range");
    run.put(Vec3::new(-240.5, -24.9, 760.0), 0.0);
    wait_for(&mut run, &mut stops, |run| gone(run, pie), 120).expect("the pie goes");
    println!(
        "the melon eaten at {eaten}, 100 to {health}; the pie off; {:.1} s; load at end: {}",
        started.elapsed().as_secs_f64(),
        load()
    );
    assert!(stops.is_empty(), "{stops:#?}");
}

/// openspec AS.133: a drop. With the combat system's food counter at 1 the
/// next enemy past 5 experience beaten leaves one of the level's food set
/// where it fell: it comes up, falls to the ground and waits there, and a
/// hurt hero walking to it heals by 100. Nothing stops.
#[test]
#[ignore = "reads the Giants dump"]
fn level_214s_kills_drop_food_that_lands_and_heals() {
    let started = std::time::Instant::now();
    println!("load at start: {}", load());
    let (mut run, mut stops) = joined_214();
    run.put(Vec3::new(BY_004B_TRIGGER[0] - 4.0, -22.3, 722.4), 90.0);
    assert_eq!(walk(&mut run, &mut stops, &[BY_004B_TRIGGER]), Ok(()));
    set_food_counter(&mut run, 1);
    let f = fight_until(
        &mut run,
        &mut stops,
        |run| !dropped(run).is_empty(),
        &FOES_004B,
        IN_004B_YARD,
        3000,
    );
    let drop_at = f.done.expect("a kill past the counter drops food");
    let food = dropped(&run)[0];
    let name = run.world.name(food);
    assert!(
        ["food_pizza_slice", "food_pretzel", "food_watermelon"].contains(&name.as_str()),
        "{name}"
    );
    // Well out of its 8 ft while it falls.
    let at = run.world.actor(food).unwrap().position;
    let away = Vec3::new(at.x - 8.0, at.y - 4.0, at.z);
    run.put(away, 90.0);
    let mut rest = at;
    for _ in 0..180 {
        step(&mut run, &mut stops, [false; 4]);
        rest = run.world.actor(food).unwrap().position;
    }
    assert!(!gone(&run, food), "it waits for him");
    assert!(
        (rest.y - run.hero().pivot.y).abs() < 2.0,
        "down on the ground: {rest}, he's at {}",
        run.hero().pivot
    );
    let minion = run.minion.expect("he's joined");
    assert!(guacamole::scripts::set_health(&mut run.vm, minion, 100));
    assert_eq!(walk(&mut run, &mut stops, &[[rest.x, rest.z]]), Ok(()));
    wait_for(&mut run, &mut stops, |run| gone(run, food), 180).expect("eaten");
    let health = run.health().unwrap();
    println!(
        "{name} dropped at {drop_at} at {at}, down at {rest}, eaten, health {health}; {:.1} s; load at end: {}",
        started.elapsed().as_secs_f64(),
        load()
    );
    assert!(health >= 200.0, "100 more: {health}");
    assert!(stops.is_empty(), "{stops:#?}");
}
