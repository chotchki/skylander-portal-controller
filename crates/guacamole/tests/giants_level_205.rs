//! Level 205 (openspec AS.282, AS.283), headless with guacamole's host on the TV's
//! listing for the level: Eruptor joined at its start, its intro played to
//! its end or skipped as a player skips it, then the stick his. Ignored by
//! default; they read `GUAC_GIANTS_DUMP` (process env, else `.env.dev`):
//!
//! `cargo test -p guacamole --profile disc --test giants_level_205 -- --ignored --nocapture`
//!
//! No recording of 205 exists: every step is the level's own scripts.

use std::collections::{HashMap, VecDeque};

use guac_motion::ground;
use guac_motion::seam::Vec3;
use guac_motion::stick::Stick;
use guac_script::Mode;
use guacamole::script_engine::Pad;

mod common;

use common::*;

const LEVEL_205: &str = "level_205_piratecards";

/// Where the intro sets the first player down: its cast's places' third
/// waypoint (`205_Intro_Player_Pos_WP`).
const HAND_OVER: Vec3 = Vec3::new(146.82, 14.60, 251.97);

fn setup() -> Setup {
    Setup {
        interface: true,
        props: true,
        sounds: true,
        ..Setup::default()
    }
}

fn step(run: &mut Run, stops: &mut Vec<String>, stick: Stick, actions: [bool; 4]) {
    run.bench.pads[0] = Pad {
        stick,
        actions,
        connected: true,
    };
    stops.extend(run.step_play().unwrap().iter().map(ToString::to_string));
}

fn still(run: &mut Run, stops: &mut Vec<String>, actions: [bool; 4]) {
    step(run, stops, Stick::new(0.0, 0.0), actions);
}

/// The run joined at its start, its intro held until the join lets it go.
fn joined_205(stops: &mut Vec<String>) -> Run {
    let (mut run, _) = run_on_level(Mode::Play, LEVEL_205, setup());
    for _ in 0..2 {
        stops.extend(run.step_play().unwrap().iter().map(ToString::to_string));
    }
    run.join().unwrap_or_else(|h| panic!("{h}"));
    run
}

/// After the intro: the HUD back, him on the ground, and the stick walking
/// him on from where he stands.
fn walks_on(run: &mut Run, stops: &mut Vec<String>) {
    for _ in 0..120 {
        still(run, stops, [false; 4]);
    }
    assert!(
        !guacamole::scripts::player_hud_hidden(&run.vm),
        "the HUD back"
    );
    assert!(run.hero().footing.supported, "on the ground");
    let at = run.hero().pivot;
    for _ in 0..300 {
        step(run, stops, Stick::new(0.0, 1.0), [false; 4]);
    }
    let moved = run.hero().pivot.distance(at);
    println!("walked {moved:.2} m from {at}");
    assert!(moved > 3.0, "{moved}");
}

/// openspec AS.282: 205's intro played to its end: its eleven scenes in
/// order, the landing pod, Flynn, Cali and the mechanic among their actors,
/// him hung on the pod in its first scenes, then set down where it hands
/// him over, its own Flynn, Cali and mechanic taken off. Nothing stops.
#[test]
#[ignore = "reads the Giants dump"]
fn level_205s_intro_plays_to_its_end_and_hands_him_over() {
    println!("load at start: {}", load());
    let mut stops = Vec::new();
    let mut run = joined_205(&mut stops);
    let (mut scenes, mut cast, mut hung) = (Vec::new(), Vec::new(), 0);
    let mut ended = None;
    for k in 0..6000u32 {
        still(&mut run, &mut stops, [false; 4]);
        hung += u32::from(run.hero().hung);
        if let Some((i, n)) = scene_of(&run)
            && scenes.last() != Some(&i)
        {
            assert_eq!(n, 11, "its eleven scenes");
            scenes.push(i);
        }
        for a in scene_actors(&run).into_iter().flatten() {
            if !cast.contains(&a) {
                cast.push(a);
            }
        }
        if k > 200 && scene_of(&run).is_none() && !guacamole::anchors::scene_holds(&run.vm) {
            ended = Some(run.steps);
            break;
        }
    }
    let at = run.hero().pivot;
    println!("scenes {scenes:?}, cast {cast:?}, hung {hung} steps, over at {ended:?}: {at}");
    assert_eq!(scenes, (0..11).collect::<Vec<_>>());
    assert!(ended.is_some(), "it ends");
    for who in ["205_Landing_Pod", "Flynn_NPC", "Cali_NPC", "Mechanic_Clone"] {
        assert!(cast.iter().any(|a| a == who), "{who} in its scenes");
    }
    assert!(hung > 300, "{hung}");
    assert!(!run.hero().hung);
    assert!(
        at.distance(HAND_OVER) < 1.5,
        "where it hands him over: {at}"
    );
    for who in ["Flynn_NPC", "Cali_NPC", "Mechanic_Clone"] {
        let theirs: Vec<_> = run
            .world
            .clones()
            .into_iter()
            .filter(|h| run.world.name(*h) == who)
            .collect();
        assert!(!theirs.is_empty(), "{who} cloned by the intro");
        assert!(
            theirs
                .iter()
                .all(|h| run.world.actor(*h).is_none_or(|a| a.removed)),
            "{who} off"
        );
    }
    walks_on(&mut run, &mut stops);
    assert!(stops.is_empty(), "{stops:#?}");
    println!("load at end: {}", load());
}

/// openspec AS.282: 205's start read off its intro's hand-over, the intro
/// held for the hero and skipped by △: him set down there to the
/// centimetre, the HUD back, walking on the stick. Nothing stops.
#[test]
#[ignore = "reads the Giants dump"]
fn level_205s_intro_skipped_hands_him_over_and_he_walks() {
    println!("load at start: {}", load());
    let mut stops = Vec::new();
    let mut run = joined_205(&mut stops);
    let start = run.hero().pivot;
    println!("his start {start}");
    assert!(
        start.distance(HAND_OVER) < 0.5,
        "where it hands him over: {start}"
    );
    let mut played = false;
    for k in 0..3000u32 {
        still(
            &mut run,
            &mut stops,
            [false, false, false, k > 120 && k % 20 == 10],
        );
        played |= cutscene_name(&run).as_deref() == Some("205 intro igc");
        if k > 200 && played && !guacamole::anchors::scene_holds(&run.vm) {
            break;
        }
    }
    assert!(played, "its intro ran");
    let at = run.hero().pivot;
    assert!(at.distance(HAND_OVER) < 0.1, "set down at its place: {at}");
    walks_on(&mut run, &mut stops);
    assert!(stops.is_empty(), "{stops:#?}");
    println!("load at end: {}", load());
}

/// How far from a wall at knee height a cell of [`way_to`]'s lies, metres.
const CLEAR: f32 = 0.6;

/// The ground a hero walks to from `from` as `giants_swap_fit`'s flood has
/// it (cells a metre across, a step up to 0.7 m and down to 4 m, never
/// through a wall at knee height, here none within [`CLEAR`] of one), with the props and the clones standing
/// as the run has them now; the way from `from` to the cell nearest `to`,
/// first to last, and how far that cell is from `to`.
fn way_to(run: &Run, from: Vec3, to: Vec3) -> (Vec<Vec3>, f32) {
    let key = |p: Vec3| {
        (
            p.x.floor() as i32,
            (p.y / 1.5).floor() as i32,
            p.z.floor() as i32,
        )
    };
    let world = &run.bench.world;
    let mut seen = HashMap::from([(key(from), 0usize)]);
    let (mut cells, mut parent) = (vec![from], vec![0usize]);
    let mut queue = VecDeque::from([0usize]);
    while let Some(at) = queue.pop_front() {
        let p = cells[at];
        for (dx, dz) in [(1.0f32, 0.0f32), (-1.0, 0.0), (0.0, 1.0), (0.0, -1.0)] {
            let over = p + Vec3::new(dx, 0.7, dz);
            let Some(hit) = ground::ray(world, over, Vec3::NEG_Y, 4.7, &[HERO_BODY]) else {
                continue;
            };
            if hit.normal.y < 0.65 || p.y - hit.point.y > 4.0 {
                continue;
            }
            let to = hit.point;
            let (a, b) = (
                p + Vec3::Y * 0.8,
                Vec3::new(to.x, to.y.max(p.y) + 0.8, to.z),
            );
            if ground::ray(world, a, b - a, (b - a).length(), &[HERO_BODY]).is_some() {
                continue;
            }
            // Room for him: nothing within `CLEAR` of the cell at knee
            // height, which the flood's cell-to-cell ray alone lets a way
            // hug (a railing up the crane deck's ramp).
            let knee = to + Vec3::Y * 0.8;
            let walled = [Vec3::X, Vec3::NEG_X, Vec3::Z, Vec3::NEG_Z]
                .iter()
                .any(|d| ground::ray(world, knee, *d, CLEAR, &[HERO_BODY]).is_some());
            if walled {
                continue;
            }
            if let std::collections::hash_map::Entry::Vacant(v) = seen.entry(key(to)) {
                v.insert(cells.len());
                cells.push(to);
                parent.push(at);
                queue.push_back(cells.len() - 1);
            }
        }
    }
    let end = (0..cells.len())
        .min_by(|a, b| cells[*a].distance(to).total_cmp(&cells[*b].distance(to)))
        .unwrap();
    let mut way = vec![cells[end]];
    let mut at = end;
    while at != 0 {
        at = parent[at];
        way.push(cells[at]);
    }
    way.reverse();
    (way, cells[end].distance(to))
}

/// The stick held along `way` (the view turned to the first cell 1.5 m off past
/// the nearest of the next six); while a cutscene holds him the stick is let go
/// and the steps run uncounted, each cutscene's name and first step pushed
/// on `scenes`. Whether he came within 0.7 m of its last cell (across the
/// ground) before `cap` steps.
fn walk(
    run: &mut Run,
    stops: &mut Vec<String>,
    scenes: &mut Vec<(u32, String)>,
    way: &[Vec3],
    cap: u32,
) -> bool {
    let flat = |a: Vec3, b: Vec3| Vec3::new(a.x - b.x, 0.0, a.z - b.z).length();
    let (mut next, mut walked, mut held) = (0, 0, 0);
    while walked < cap && held < 3000 {
        if let Some(cs) = cutscene_name(run) {
            if scenes.last().is_none_or(|(_, n)| *n != cs) {
                scenes.push((run.steps, cs));
            }
            held += 1;
            still(run, stops, [false; 4]);
            continue;
        }
        if guacamole::anchors::scene_holds(&run.vm) {
            held += 1;
            still(run, stops, [false; 4]);
            continue;
        }
        let at = run.hero().pivot;
        if flat(at, *way.last().unwrap()) < 0.7 {
            return true;
        }
        next = (next..(next + 6).min(way.len()))
            .min_by(|a, b| flat(at, way[*a]).total_cmp(&flat(at, way[*b])))
            .unwrap();
        let aim = way[next..]
            .iter()
            .find(|c| flat(at, **c) > 1.5)
            .unwrap_or(way.last().unwrap());
        run.bench.view = (aim.x - at.x).atan2(aim.z - at.z).to_degrees();
        run.bench.hold_view = true;
        step(run, stops, Stick::new(0.0, 1.0), [false; 4]);
        walked += 1;
    }
    false
}

/// An actor's facing now, degrees.
fn facing(run: &Run, name: &str) -> f32 {
    run.world.actor(run.placed(name).0).unwrap().angles.facing
}

/// openspec AS.283: off 205's island on its own scripts. Handed over (the
/// intro skipped by △), he walks off the Giant Pirate Head's stage over
/// the crane's boat, lying east, to the far pier; at the pier's south end
/// the hint pans to the wheel; on the crane's deck △ turns it, the crane
/// and its mast swinging to facing 180, the boat now lying from the pier's
/// end to the yard; and he walks over it to Setup_001's controller. Before
/// the turn no walk reaches the yard. Nothing stops.
#[test]
#[ignore = "reads the Giants dump"]
fn level_205s_way_off_its_island_turns_the_crane() {
    println!("load at start: {}", load());
    let mut stops = Vec::new();
    let mut run = joined_205(&mut stops);
    for k in 0..400u32 {
        still(
            &mut run,
            &mut stops,
            [false, false, false, k > 120 && k % 20 == 10],
        );
    }
    for _ in 0..120 {
        still(&mut run, &mut stops, [false; 4]);
    }
    let start = run.hero().pivot;
    assert!(start.distance(HAND_OVER) < 0.5, "handed over: {start}");
    let (_, controller) = run.placed("Setup_001_Controller");
    let (_, yard_first) = way_to(&run, start, controller);
    println!("before the turn the yard is {yard_first:.1} m off any walk");
    assert!(yard_first > 10.0, "{yard_first}");
    assert_eq!(facing(&run, "Crane(1)"), 90.0, "the boat lies east");

    // Over the boat, past the crane's deck and down the pier to its south
    // end, where the hint (its own place, within its 20 ft range) pans to the
    // wheel: the pier stops short of the yard.
    let mut scenes = Vec::new();
    let (_, hint) = run.placed("Hint_Pan_Show_Wheel");
    let (way, off) = way_to(&run, start, hint);
    println!(
        "to the pier's end {hint}: {} cells, {off:.1} m off",
        way.len()
    );
    if std::env::var_os("P_WAY").is_some() {
        for c in &way {
            println!("  cell {c}");
        }
    }
    assert!(off < 3.0, "{off}");
    assert!(
        way.iter().any(|c| c.x < 118.0 && c.x > 107.0),
        "over the boat"
    );
    let reached = walk(&mut run, &mut stops, &mut scenes, &way, 3000);
    for _ in 0..240 {
        if cutscene_name(&run).is_none() && !guacamole::anchors::scene_holds(&run.vm) {
            break;
        }
        still(&mut run, &mut stops, [false; 4]);
    }
    println!(
        "at the pier's end {reached} at {}: scenes {scenes:?}",
        run.hero().pivot
    );
    assert!(reached, "at the pier's end: {}", run.hero().pivot);
    assert!(!scenes.is_empty(), "the hint panned to the wheel");

    // Up to the wheel and △ until the crane turns.
    let (_, crank) = run.placed("SpinCrank");
    let (way, off) = way_to(&run, run.hero().pivot, crank);
    println!("to the wheel {crank}: {} cells, {off:.1} m off", way.len());
    assert!(off < 3.0, "{off}");
    assert!(
        walk(&mut run, &mut stops, &mut scenes, &way, 3000),
        "at the wheel: {}",
        run.hero().pivot
    );
    let mut presses = 0;
    for k in 0..600u32 {
        let press = facing(&run, "Crane(1)") == 90.0 && k % 20 == 10;
        presses += u32::from(press);
        still(&mut run, &mut stops, [false, false, false, press]);
        if facing(&run, "Crane(1)") == 180.0 {
            break;
        }
    }
    println!(
        "turned to {} after {presses} presses, step {}",
        facing(&run, "Crane(1)"),
        run.steps
    );
    assert_eq!(facing(&run, "Crane(1)"), 180.0, "the boat lies south");
    assert_eq!(facing(&run, "Crane_Head"), 180.0, "its mast with it");
    let hint_off = run
        .world
        .actor(run.placed("Hint_Pan_Show_Wheel").0)
        .unwrap()
        .removed;
    assert!(hint_off, "the hint off once he's by the wheel");
    for _ in 0..60 {
        still(&mut run, &mut stops, [false; 4]);
    }

    // Over the boat to the yard.
    let (way, off) = way_to(&run, run.hero().pivot, controller);
    println!(
        "to Setup_001's controller {controller}: {} cells, {off:.1} m off",
        way.len()
    );
    assert!(
        walk(&mut run, &mut stops, &mut scenes, &way, 4000),
        "in the yard: {}",
        run.hero().pivot
    );
    println!("scenes on the way {scenes:?}");
    let at = run.hero().pivot;
    println!(
        "at {at}: {:.2} m from the controller, {:.2} m across the ground",
        at.distance(controller),
        Vec3::new(at.x - controller.x, 0.0, at.z - controller.z).length()
    );
    assert!(at.distance(controller) < 3.5, "{at}");
    assert!(stops.is_empty(), "{stops:#?}");
    println!("load at end: {}", load());
}
