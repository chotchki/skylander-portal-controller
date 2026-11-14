//! Level 205 (openspec AS.282, AS.283, AS.286), headless with guacamole's host on the TV's
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
    // The hint's own pan and nothing else: any cutscene alone would let
    // another placement's stand in for it.
    assert!(
        scenes.len() == 1 && scenes[0].1.contains("Pan"),
        "the hint panned to the wheel: {scenes:?}"
    );

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
    assert_eq!(scenes.len(), 1, "no other cutscene on the way: {scenes:?}");
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

/// Whether `h`'s placement runs: on, not taken off, awake and not inert.
fn awake(run: &Run, h: guac_script::Handle) -> bool {
    run.vm
        .placement_of(h)
        .is_some_and(|id| !run.vm.removed(id) && !run.vm.asleep(id))
        && !run.vm.inert().contains(&h)
        && run.world.actor(h).is_some_and(|a| a.active && !a.removed)
}

/// The live clones within `r` m of `at`, by name.
fn clones_near(run: &Run, at: Vec3, r: f32) -> Vec<String> {
    run.world
        .clones()
        .into_iter()
        .filter(|h| {
            run.world
                .actor(*h)
                .is_some_and(|a| a.active && !a.removed && a.position.distance(at) < r)
        })
        .map(|h| run.world.name(h))
        .collect()
}

/// The talk prompt the level's dialogue hangs over a person a hero is near
/// (the exclamation the talk button's prompt goes with).
const TALK_PROMPT: &str = "Dialog_ExPoint_NoScript";

/// The talk button (△) pressed every 20 steps until a cutscene starts, then
/// the steps run until it ends and lets the hero go: the cutscene's name
/// and the steps it began and ended at.
fn talk(run: &mut Run, stops: &mut Vec<String>, cap: u32) -> Option<(String, u32, u32)> {
    let mut began = None;
    for k in 0..cap {
        let press = began.is_none() && k % 20 == 10;
        still(run, stops, [false, false, false, press]);
        let cs = cutscene_name(run);
        match (&began, cs) {
            (None, Some(n)) => began = Some((n, run.steps)),
            (Some((n, b)), None) if !guacamole::anchors::scene_holds(&run.vm) => {
                return Some((n.clone(), *b, run.steps));
            }
            _ => {}
        }
    }
    None
}

/// The stick held away from `from` (the view turned that way) after a
/// talk: him handed back, the HUD up and walking on.
fn walks_off(run: &mut Run, stops: &mut Vec<String>, from: Vec3) {
    let at = run.hero().pivot;
    run.bench.view = (at.x - from.x).atan2(at.z - from.z).to_degrees();
    run.bench.hold_view = true;
    walks_on(run, stops);
}

/// openspec AS.286: the mechanic by 205's start once its intro hands the
/// hero over (asleep through it): Bowers stands at his place, drawn awake on his own model,
/// his hat on; walked to him, the talk prompt hangs by him, and nothing
/// starts until △, which starts his talk (a pan with its lines), played to
/// its end and the hero handed back to walk on, Bowers still there.
/// Nothing stops.
#[test]
#[ignore = "reads the Giants dump"]
fn level_205s_mechanic_stands_by_the_start_and_talks() {
    println!("load at start: {}", load());
    let mut stops = Vec::new();
    let mut run = joined_205(&mut stops);
    // The intro holds him asleep (its own mechanic talks in his stead) and
    // lets him go at its end, here its skip.
    let (bowers, placed) = run.placed("Bowers");
    let mut held = 0;
    for k in 0..400u32 {
        still(
            &mut run,
            &mut stops,
            [false, false, false, k > 120 && k % 20 == 10],
        );
        if cutscene_name(&run).as_deref() == Some("205 intro igc") {
            assert!(
                !awake(&run, bowers),
                "asleep through the intro, step {}",
                run.steps
            );
            held += 1;
        }
    }
    assert!(held > 60, "the intro ran: {held}");
    for _ in 0..120 {
        still(&mut run, &mut stops, [false; 4]);
    }
    let start = run.hero().pivot;
    assert!(start.distance(HAND_OVER) < 0.5, "handed over: {start}");
    let at = run.world.actor(bowers).unwrap().position;
    let behaviour = |run: &Run| {
        run.vm
            .placement_of(bowers)
            .and_then(|id| run.vm.behaviour(id))
            .map(str::to_owned)
    };
    let far = clones_near(&run, at, 4.0);
    println!(
        "Bowers at {at} (placed {placed}), {:?}, by him {far:?}",
        behaviour(&run)
    );
    assert!(awake(&run, bowers), "Bowers runs");
    assert!(at.distance(placed) < 0.05, "at his place: {at}");
    assert!(
        run.world.people().contains(&bowers),
        "one of the TV's people"
    );
    let skinned: Vec<String> = skinned_models_of(LEVEL_205)
        .into_iter()
        .map(|(p, _)| p)
        .collect();
    let model = run
        .files
        .placed
        .iter()
        .find(|p| p.obj.unit == LEVEL && p.name == "Bowers")
        .and_then(|p| p.model.clone())
        .expect("his model");
    assert!(skinned.contains(&model), "drawn on his {model}");
    assert!(far.iter().any(|c| c == "Pirate_Hat_Showman"), "his hat on");
    assert!(
        clones_drawn(&run, &skinned, "Pirate_Hat_Showman"),
        "his hat drawn"
    );
    assert!(!far.iter().any(|c| c == TALK_PROMPT), "no prompt from afar");

    // Walked to him, 1.5 m short.
    let (way, off) = way_to(&run, start, at);
    println!("to Bowers: {} cells, {off:.1} m off", way.len());
    assert!(off < 1.5, "{off}");
    let mut scenes = Vec::new();
    assert!(
        walk(
            &mut run,
            &mut stops,
            &mut scenes,
            &way[..way.len() - 1],
            2000
        ),
        "by him: {}",
        run.hero().pivot
    );
    for _ in 0..120 {
        still(&mut run, &mut stops, [false; 4]);
    }
    let me = run.hero().pivot;
    let near = clones_near(&run, at, 4.0);
    println!(
        "by him at {me}, {:.2} m; scenes {scenes:?}; by him {near:?}",
        me.distance(at)
    );
    assert!(
        scenes.is_empty() && cutscene_name(&run).is_none(),
        "{scenes:?}"
    );
    assert!(
        near.iter().any(|c| c == TALK_PROMPT),
        "the talk prompt: {near:?}"
    );

    // △: his talk, to its end.
    let talked = talk(&mut run, &mut stops, 4000);
    println!("his talk {talked:?}, {:?}", behaviour(&run));
    let (name, began, ended) = talked.expect("his talk plays and ends");
    assert!(name.contains("Simple Pan"), "{name}");
    assert!(ended - began > 60, "{began} to {ended}");
    assert!(awake(&run, bowers), "Bowers still there");
    walks_off(&mut run, &mut stops, at);
    assert!(stops.is_empty(), "{stops:#?}");
    println!("load at end: {}", load());
}

/// What one of 205's people or scenes past the first yard does with a hero
/// put beside it (openspec AS.286), as far as today's level goes.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Met {
    /// △ starts its talk, a cutscene that plays to its end.
    Talks,
    /// A hero coming near (or touching the placement named) starts its
    /// scene, which plays to its end.
    Scene(Option<&'static str>),
    /// Awake on its model, the talk prompt by it, its talk waiting on a
    /// fight no listing runs yet (openspec AS.284).
    Waits,
    /// Awake on its model, turning to him; no talk.
    Turns,
    /// Runs to its end and lets the camera go.
    Pops,
    /// Awake, waiting for an approach that a hero beside it doesn't make:
    /// nothing plays.
    Quiet,
    /// Asleep: woken by a script past what runs today.
    Asleep,
}

/// 205's people and scenes past the first yard, by placement, and what each
/// does with a hero put beside it.
const PAST_THE_YARD: [(&str, Met); 26] = [
    ("T-Bone", Met::Talks),
    ("CardGame_Template(1)_Bandit", Met::Talks),
    ("Blobbers", Met::Scene(Some("SignalFace_CraneMabu_Intro"))),
    ("Card_Pirate_05_Cine", Met::Scene(None)),
    ("Hint_Pan_Show_Island", Met::Scene(None)),
    ("Hint_Pan_Show_Wheel(1)", Met::Scene(None)),
    ("Cali", Met::Waits),
    ("Cali_Town", Met::Waits),
    ("Slave_1_hammer(5)", Met::Turns),
    ("Slave_Carry(5)", Met::Turns),
    ("Switch_To_Cam(6.1)", Met::Pops),
    ("Switch_To_Cam(6)", Met::Pops),
    ("SwitchToCam7", Met::Pops),
    ("CardGame_Template(4)_Dreadbeard", Met::Scene(None)),
    ("CardPirate_Bandit_Cine", Met::Talks),
    ("Card_Pirate_04_Gate_Woof", Met::Asleep),
    ("CardGame_Template(2)", Met::Asleep),
    ("CardGame_Chance", Met::Asleep),
    ("205_Encounter_At_Galleon_IGC", Met::Asleep),
    ("CardPirate_Boathouse_Cine", Met::Asleep),
    ("CardPirate_Boathouse_Cine_B", Met::Asleep),
    ("CardPirate_Casino_Cine", Met::Asleep),
    ("CardPirate_Casino_Cine_B", Met::Asleep),
    ("Hint_Pan_Show_Turntable", Met::Asleep),
    ("Switch_To_Cam(6.3)", Met::Asleep),
    ("SpecialPan", Met::Quiet),
];

/// openspec AS.286: each of 205's people and scenes past the first yard,
/// on 205's listing, with the hero put beside it (a test-only placement:
/// no walk reaches past the first yard yet), each in a run of its own after
/// the intro's skip: its scene or talk played to its end and the hero
/// handed back, or awake on its model with the prompt by it, or turning to
/// him, or asleep where a script past today's wakes it. Nothing stops.
#[test]
#[ignore = "reads the Giants dump"]
fn level_205s_people_and_scenes_past_the_yard_as_far_as_put() {
    println!("load at start: {}", load());
    let skinned: Vec<String> = skinned_models_of(LEVEL_205)
        .into_iter()
        .map(|(p, _)| p)
        .collect();
    let lists = guacamole::level_205::PEOPLE
        .iter()
        .chain(&guacamole::level_205::SCENES)
        .count();
    assert_eq!(lists, PAST_THE_YARD.len() + 1, "all listed, Bowers aside");
    for (name, met) in PAST_THE_YARD {
        assert!(
            guacamole::level_205::listed(LEVEL_205).any(|n| n == name),
            "{name} listed"
        );
        let mut stops = Vec::new();
        let mut run = joined_205(&mut stops);
        for k in 0..400u32 {
            still(
                &mut run,
                &mut stops,
                [false, false, false, k > 120 && k % 20 == 10],
            );
        }
        let (h, placed) = run.placed(name);
        let model = run
            .files
            .placed
            .iter()
            .find(|p| p.obj.unit == LEVEL && p.name == name)
            .and_then(|p| p.model.clone());
        let mut scenes: Vec<String> = Vec::new();
        let mut watch = |run: &mut Run, stops: &mut Vec<String>, n: u32| {
            for _ in 0..n {
                still(run, stops, [false; 4]);
                if let Some(cs) = cutscene_name(run)
                    && scenes.last() != Some(&cs)
                {
                    scenes.push(cs);
                }
            }
        };
        if let Met::Scene(Some(first)) = met {
            let (_, at) = run.placed(first);
            run.put(at, 0.0);
            watch(&mut run, &mut stops, 200);
        }
        run.put(placed + Vec3::new(1.5, 0.5, 1.5), 0.0);
        watch(&mut run, &mut stops, 300);
        let ended = |run: &mut Run, stops: &mut Vec<String>| {
            for _ in 0..4000 {
                if cutscene_name(run).is_none() && !guacamole::anchors::scene_holds(&run.vm) {
                    return true;
                }
                still(run, stops, [false; 4]);
            }
            false
        };
        let behaviour = run
            .vm
            .placement_of(h)
            .and_then(|id| run.vm.behaviour(id))
            .map(str::to_owned);
        let near = clones_near(&run, placed, 5.0);
        println!("{name}: {met:?}: {behaviour:?}, scenes {scenes:?}, by it {near:?}");
        let drawn = |run: &Run| {
            let model = model.as_ref().expect("a model");
            assert!(run.world.people().contains(&h), "{name} one of the people");
            assert!(skinned.contains(model), "{name} drawn on its {model}");
        };
        match met {
            Met::Talks => {
                assert!(awake(&run, h), "{name} awake");
                if !name.ends_with("_Cine") {
                    drawn(&run);
                }
                assert!(near.iter().any(|c| c == TALK_PROMPT), "{name}: {near:?}");
                let t = talk(&mut run, &mut stops, 4000);
                println!("  its talk {t:?}");
                let (_, began, end) = t.unwrap_or_else(|| panic!("{name} talks to its end"));
                assert!(end - began > 60, "{name}: {began} to {end}");
            }
            Met::Scene(_) => {
                if guacamole::level_205::PEOPLE.contains(&name) {
                    assert!(awake(&run, h), "{name} awake");
                    drawn(&run);
                }
                assert!(!scenes.is_empty(), "{name}'s scene");
                assert!(ended(&mut run, &mut stops), "{name}'s scene ends");
            }
            Met::Waits => {
                assert!(awake(&run, h), "{name} awake");
                drawn(&run);
                assert!(near.iter().any(|c| c == TALK_PROMPT), "{name}: {near:?}");
                assert!(scenes.is_empty(), "{name}: {scenes:?}");
            }
            Met::Turns => {
                assert!(awake(&run, h), "{name} awake");
                drawn(&run);
                assert_eq!(behaviour.as_deref(), Some("turn to player"), "{name}");
            }
            Met::Pops => {
                assert!(awake(&run, h), "{name} awake");
                assert!(
                    matches!(behaviour.as_deref(), Some("end" | "done")),
                    "{name}: {behaviour:?}"
                );
            }
            Met::Quiet => {
                assert!(awake(&run, h), "{name} awake");
                assert!(scenes.is_empty(), "{name}: {scenes:?}");
            }
            Met::Asleep => {
                assert!(!awake(&run, h), "{name} asleep");
                assert!(scenes.is_empty(), "{name}: {scenes:?}");
            }
        }
        for _ in 0..120 {
            still(&mut run, &mut stops, [false; 4]);
        }
        assert!(
            !guacamole::anchors::scene_holds(&run.vm) && cutscene_name(&run).is_none(),
            "{name}: nothing holds him"
        );
        assert!(
            !guacamole::scripts::player_hud_hidden(&run.vm),
            "{name}: the HUD back"
        );
        assert!(stops.is_empty(), "{name}: {stops:#?}");
    }
    println!("load at end: {}", load());
}
