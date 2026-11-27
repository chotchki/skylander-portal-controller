//! Level 208 (openspec AS.343, AS.344, AS.345), headless with guacamole's host on the TV's
//! listing for the level: Eruptor joined at its start, its intro played to
//! its end or skipped as a player skips it, then the stick his. Ignored by
//! default; they read `GUAC_GIANTS_DUMP` (process env, else `.env.dev`):
//!
//! `cargo test -p guacamole --profile disc --test giants_level_208 -- --ignored --nocapture`
//!
//! No recording of 208 exists: every step is the level's own scripts.

use std::collections::{HashMap, VecDeque};

use guac_motion::ground;
use guac_motion::seam::Vec3;
use guac_motion::stick::Stick;
use guac_script::Mode;
use guacamole::script_engine::Pad;

mod common;

use common::*;

const LEVEL_208: &str = "level_208_ice";

/// Where the intro walks the first player in to: `208_Intro_P1_WP`'s last
/// waypoint.
const HAND_OVER: Vec3 = Vec3::new(12.085, 10.184, -9.541);

/// The TV's listing with the props, sounds and interface, and the rigs posed
/// as the TV poses them: unposed, the landing pod lets its riders go at the
/// world's origin under the plaza and the walk-ins never arrive.
fn setup() -> Setup {
    Setup {
        interface: true,
        props: true,
        sounds: true,
        posed_rigs: true,
        static_nodes: true,
        lightning: true,
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
fn joined_208(stops: &mut Vec<String>) -> Run {
    let (mut run, _) = run_on_level(Mode::Play, LEVEL_208, setup());
    for _ in 0..2 {
        stops.extend(run.step_play().unwrap().iter().map(ToString::to_string));
    }
    run.join().unwrap_or_else(|h| panic!("{h}"));
    run
}

/// The intro's own placement taken off by its end, as it ends either way.
fn intro_off(run: &Run) -> bool {
    run.vm.removed(run.placement("208_Intro_IGC"))
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

/// openspec AS.343: 208's intro played to its end: its five scenes in
/// order, the landing pod, Flynn, Cali, the Hermit and the foes of its
/// flyover among their actors, him hung on the pod as it flies in, then
/// walked in along his path and handed over at its end, the intro off.
/// Nothing stops.
#[test]
#[ignore = "reads the Giants dump"]
fn level_208s_intro_plays_to_its_end_and_hands_him_over() {
    println!("load at start: {}", load());
    let mut stops = Vec::new();
    let mut run = joined_208(&mut stops);
    let (mut scenes, mut cast, mut hung) = (Vec::new(), Vec::new(), 0);
    let mut ended = None;
    for k in 0..6000u32 {
        still(&mut run, &mut stops, [false; 4]);
        hung += u32::from(run.hero().hung);
        if let Some((i, n)) = scene_of(&run)
            && scenes.last() != Some(&i)
        {
            assert_eq!(n, 5, "its five scenes");
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
    let pod = pod_at(&run).expect("the landing pod");
    println!("the landing pod played out at {pod}");
    assert!(pod.distance(POD_LANDED) < 0.5, "the pod landed: {pod}");
    assert_eq!(scenes, (0..5).collect::<Vec<_>>());
    assert!(ended.is_some(), "it ends");
    for who in [
        "208_Intro_Landing_Pod",
        "208_Intro_Flynn_NPC",
        "208_Intro_Cali_NPC",
        "208_Intro_Ermit_NPC",
        "Enemy_Biclops_Heavy_01",
    ] {
        assert!(cast.iter().any(|a| a == who), "{who} in its scenes");
    }
    assert!(hung > 300, "{hung}");
    assert!(!run.hero().hung);
    assert!(
        at.distance(HAND_OVER) < 1.5,
        "where it hands him over: {at}"
    );
    assert!(intro_off(&run), "the intro off");
    walks_on(&mut run, &mut stops);
    assert!(stops.is_empty(), "{stops:#?}");
    println!("load at end: {}", load());
}

/// openspec AS.343: 208's start read off its intro's walk-in, the intro
/// held for the hero and skipped by △: him set down at the path's end to
/// the centimetre, the HUD back, walking on the stick. Nothing stops.
#[test]
#[ignore = "reads the Giants dump"]
fn level_208s_intro_skipped_hands_him_over_and_he_walks() {
    println!("load at start: {}", load());
    let mut stops = Vec::new();
    let mut run = joined_208(&mut stops);
    let start = run.hero().pivot;
    println!("his start {start}");
    assert!(
        start.distance(HAND_OVER) < 0.1,
        "where it hands him over: {start}"
    );
    let mut played = false;
    for k in 0..3000u32 {
        still(
            &mut run,
            &mut stops,
            [false, false, false, k > 120 && k % 20 == 10],
        );
        played |= cutscene_name(&run).as_deref() == Some("208 Intro IGC");
        if k > 200 && played && !guacamole::anchors::scene_holds(&run.vm) {
            break;
        }
    }
    assert!(played, "its intro ran");
    let at = run.hero().pivot;
    assert!(at.distance(HAND_OVER) < 0.1, "set down at its place: {at}");
    assert!(intro_off(&run), "the intro off");
    // openspec AS.702: the landing pod's parts ride the pod where the skip
    // left it, not the camera's eye (on the TV its base stood over the
    // camera as a dark shell). Headless the rule is never reached (the
    // intro's rig reads posed or held), so this reads the same with it
    // undone: `a_rigs_node_unposed_reads_as_its_last` pins the rule, this
    // the outcome.
    let eye = run.world.viewport().eye;
    let parts: Vec<(String, Vec3)> = run
        .world
        .clones()
        .into_iter()
        .filter(|h| run.world.name(*h).starts_with("LP_"))
        .filter_map(|h| {
            let a = run.world.actor(h)?;
            (!a.removed).then(|| (run.world.name(h).to_owned(), a.position))
        })
        .collect();
    println!("the pod's parts after the skip {parts:?}, the eye {eye}");
    // openspec AS.707: the skip lands the intro's rig on its clip's end (its
    // animator camera shot's `time remaining` written 0), the pod hung on its
    // node with it: where the intro played out leaves it, not in the air
    // where the skip found it.
    let pod = pod_at(&run).expect("the landing pod");
    println!("the landing pod skipped at {pod}");
    assert!(pod.distance(POD_LANDED) < 0.5, "the pod landed: {pod}");
    assert!(!parts.is_empty(), "the pod's parts");
    for (name, p) in &parts {
        assert!(p.distance(eye) > 5.0, "{name} at the camera's eye: {p}");
    }
    walks_on(&mut run, &mut stops);
    assert!(stops.is_empty(), "{stops:#?}");
    println!("load at end: {}", load());
}

/// Where the intro played to its end leaves its landing pod (openspec
/// AS.707): landed by the plaza, its rig's last clip at its end.
const POD_LANDED: Vec3 = Vec3::new(-18.402, 9.359, 1.941);

/// Where the intro's landing pod stands, its clone in play (openspec AS.707).
fn pod_at(run: &Run) -> Option<Vec3> {
    run.world
        .clones()
        .into_iter()
        .filter(|h| run.world.name(*h) == "208_Intro_Landing_Pod")
        .filter_map(|h| run.world.actor(h).filter(|a| !a.removed))
        .map(|a| a.position)
        .next()
}

/// Setup_002's controller's place, past the plaza's exit (openspec AS.344).
const SETUP_002: Vec3 = Vec3::new(12.4, 9.0, -49.2);

/// The exit's reveal: its trigger and the cutscene it plays.
const TRIGGER: &str = "Ice_Chompy_IGC_Trigger";
const REVEAL: &str = "IGC_Ice_Chompy_Reveal";

/// The brick wall past the trigger.
const WALL: &str = "Wall_Spawner_Template(5)";

/// A placement's behaviour now: `removed`, `asleep`, or its own.
fn behaviour(run: &Run, name: &str) -> String {
    let h = run.placed(name).0;
    match run.vm.placement_of(h) {
        None => "-".to_owned(),
        Some(id) if run.vm.removed(id) => "removed".to_owned(),
        Some(id) if run.vm.asleep(id) => "asleep".to_owned(),
        Some(id) => run.vm.behaviour(id).unwrap_or("?").to_ascii_lowercase(),
    }
}

/// The wall's bricks still standing (clones of `brick` and `brick2` within
/// 30 m of its place: the river's wall past Setup_002, 50 m off, lays its
/// own, openspec AS.346): how many, and the farthest of each template's
/// from the wall's place.
fn bricks(run: &Run, wall: Vec3) -> (usize, [f32; 2]) {
    let mut n = 0;
    let mut far = [0.0f32; 2];
    for h in run.world.clones() {
        let name = run.world.name(h);
        let Some(k) = ["brick", "brick2"].iter().position(|t| *t == name) else {
            continue;
        };
        if let Some(a) = run
            .world
            .actor(h)
            .filter(|a| !a.removed && a.position.distance(wall) < 30.0)
        {
            n += 1;
            far[k] = far[k].max(a.position.distance(wall));
        }
    }
    (n, far)
}

/// What the walk out of the plaza met ([`walk_out`]).
#[derive(Debug, Default)]
struct WalkedOut {
    /// Each cutscene and its first step.
    scenes: Vec<(u32, String)>,
    /// The step the reveal's trigger took itself off.
    trigger_off: Option<u32>,
    /// The step the wall was beaten (its bricks blown apart).
    beaten: Option<u32>,
    /// The most bricks standing before it was beaten, and the farthest of
    /// each template's (`brick`, `brick2`) flew from it once beaten.
    laid: usize,
    flown: [f32; 2],
    /// How far south he got while the wall stood.
    south_while_standing: f32,
    /// Attacks pressed at the wall.
    swings: u32,
    /// The step he reached Setup_002's place.
    reached: Option<u32>,
}

/// The stick held from where he stands toward Setup_002's place, as a kid
/// heads out of the plaza: cutscenes waited out with the stick let go, and
/// the attack pressed every third of a second while the wall stands within
/// 8 m. Over once he's within 2 m of the place (across the ground) or after
/// `cap` steps.
fn walk_out(run: &mut Run, stops: &mut Vec<String>, cap: u32) -> WalkedOut {
    let flat = |a: Vec3, b: Vec3| Vec3::new(a.x - b.x, 0.0, a.z - b.z).length();
    let wall = run.at(WALL);
    let mut out = WalkedOut {
        south_while_standing: f32::MAX,
        ..WalkedOut::default()
    };
    let start = run.steps;
    while run.steps < start + cap {
        if out.trigger_off.is_none() && behaviour(run, TRIGGER) == "removed" {
            out.trigger_off = Some(run.steps);
        }
        let wall_now = behaviour(run, WALL);
        let standing = wall_now == "main loop";
        let gone = [
            "wait a few seconds before shutting down",
            "reset",
            "removed",
        ];
        if out.beaten.is_none() && gone.contains(&wall_now.as_str()) {
            out.beaten = Some(run.steps);
        }
        if out.beaten.is_some() {
            let far = bricks(run, wall).1;
            out.flown = [out.flown[0].max(far[0]), out.flown[1].max(far[1])];
        } else {
            out.laid = out.laid.max(bricks(run, wall).0);
        }
        if let Some(cs) = cutscene_name(run) {
            if out.scenes.last().is_none_or(|(_, n)| *n != cs) {
                out.scenes.push((run.steps, cs));
            }
            still(run, stops, [false; 4]);
            continue;
        }
        if guacamole::anchors::scene_holds(&run.vm) {
            still(run, stops, [false; 4]);
            continue;
        }
        let me = run.hero().pivot;
        if standing {
            out.south_while_standing = out.south_while_standing.min(me.z);
        }
        if flat(me, SETUP_002) < 2.0 {
            out.reached = Some(run.steps);
            break;
        }
        run.bench.view = (SETUP_002.x - me.x).atan2(SETUP_002.z - me.z).to_degrees();
        run.bench.hold_view = true;
        let swing = standing && flat(me, wall) < 8.0 && run.steps.is_multiple_of(20);
        out.swings += u32::from(swing);
        step(
            run,
            stops,
            Stick::new(0.0, 1.0),
            [swing, false, false, false],
        );
    }
    out
}

/// openspec AS.344: out of 208's start plaza on its own scripts. The intro
/// played to its end, the stick held from the hand-over toward Setup_002's
/// place: walking into the exit's trigger plays the Ice Chompy's reveal
/// once and takes the trigger off; the wall past it holds him until his
/// attacks beat it, its bricks flying apart; then he walks on to Setup_002's
/// place and the wall takes itself off. Nothing stops.
#[test]
#[ignore = "reads the Giants dump"]
fn level_208s_exit_opens_by_its_reveal_and_its_wall() {
    println!("load at start: {}", load());
    let mut stops = Vec::new();
    let mut run = joined_208(&mut stops);
    let mut ended = false;
    for k in 0..6000u32 {
        still(&mut run, &mut stops, [false; 4]);
        if k > 200 && scene_of(&run).is_none() && !guacamole::anchors::scene_holds(&run.vm) {
            ended = true;
            break;
        }
    }
    assert!(ended && intro_off(&run), "the intro played out");
    let from = run.hero().pivot;
    println!("handed over at step {} at {from}", run.steps);
    let out = walk_out(&mut run, &mut stops, 6000);
    println!("{out:?}, at {}", run.hero().pivot);
    let reveal: Vec<u32> = out
        .scenes
        .iter()
        .filter(|(_, n)| n == REVEAL)
        .map(|(s, _)| *s)
        .collect();
    assert_eq!(reveal.len(), 1, "its reveal once: {:?}", out.scenes);
    assert!(
        out.trigger_off.is_some_and(|s| s > reveal[0]),
        "the trigger off after its reveal"
    );
    let wall = run.at(WALL);
    assert!(
        out.south_while_standing > wall.z,
        "held behind the wall while it stood: {}",
        out.south_while_standing
    );
    assert!(out.laid > 10, "its bricks laid: {}", out.laid);
    assert!(
        out.beaten.is_some() && out.swings > 0,
        "beaten by his attacks"
    );
    // Both templates' bricks fly: either one unlisted, its bricks stand
    // where they were laid (within 6 m) while the other's fly.
    assert!(
        out.flown.iter().all(|f| *f > 10.0),
        "its bricks blown apart, each template's: {:?} m",
        out.flown
    );
    assert!(out.reached.is_some(), "at Setup_002's place");
    for _ in 0..600 {
        if behaviour(&run, WALL) == "removed" {
            break;
        }
        still(&mut run, &mut stops, [false; 4]);
    }
    assert_eq!(behaviour(&run, WALL), "removed", "the wall off");
    assert_eq!(bricks(&run, wall).0, 0, "its bricks gone");
    assert!(stops.is_empty(), "{stops:#?}");
    println!("load at end: {}", load());
}

/// How far from a wall at knee height a cell of [`way_to`]'s lies, metres.
const CLEAR: f32 = 0.6;

/// The ground a hero walks to from `from` as `giants_swap_fit`'s flood has
/// it (giants_level_205's: cells a metre across, a step up to 0.7 m and
/// down to 4 m, none within [`CLEAR`] of a wall at knee height), with the
/// props and the clones standing as the run has them now; the way from
/// `from` to the cell nearest `to`, first to last.
fn way_to(run: &Run, from: Vec3, to: Vec3) -> Vec<Vec3> {
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
    way
}

/// What a fight did ([`fight_to`]).
#[derive(Debug, Default)]
struct Fought {
    /// Each watched placement's behaviour as it changed: step, name, state.
    log: Vec<(u32, String, String)>,
    /// Each cutscene and its first step.
    scenes: Vec<(u32, String)>,
    /// The hero's health at the fight's start and his lowest in it.
    health: (f32, f32),
    /// Steps the stick was his (cutscenes not counted).
    played: u32,
    /// Whether `done` held before the cap.
    ended: bool,
    /// Where he stood every quarter second: what the Mac's driver follows.
    trail: Vec<Vec3>,
}

/// A fight as a kid fights it (giants_level_205's, with no top-up): the
/// nearest of `foes` awake within 40 m and 12 m of his height chased over
/// the flood's cells and hit, stepping back out after 4 s pressed into
/// one; with none awake near, on to `goal`. Cutscenes wait out with the
/// stick let go. `watch` and the foes logged as their behaviours change;
/// over once `done` holds or after `cap` steps.
fn fight_to(
    run: &mut Run,
    stops: &mut Vec<String>,
    foes: &[&str],
    watch: &[&str],
    goal: Vec3,
    cap: u32,
    done: &dyn Fn(&Run) -> bool,
) -> Fought {
    let mut fought = Fought::default();
    let hp = run.health().unwrap_or(0.0);
    fought.health = (hp, hp);
    let mut last: HashMap<String, String> = HashMap::new();
    let (mut pressed, start) = (0u32, run.steps);
    let (mut route, mut routed, mut next): (Vec<Vec3>, u32, usize) = (Vec::new(), 0, 0);
    let flat = |a: Vec3, b: Vec3| Vec3::new(a.x - b.x, 0.0, a.z - b.z).length();
    loop {
        for n in foes.iter().chain(watch) {
            let b = behaviour(run, n);
            if last.get(*n) != Some(&b) {
                fought.log.push((run.steps, (*n).to_owned(), b.clone()));
                last.insert((*n).to_owned(), b);
            }
        }
        if let Some(hp) = run.health() {
            fought.health.1 = fought.health.1.min(hp);
        }
        if (run.steps - start).is_multiple_of(15) {
            fought.trail.push(run.hero().pivot);
        }
        if done(run) {
            fought.ended = true;
            return fought;
        }
        if run.steps >= start + cap {
            return fought;
        }
        if let Some(cs) = cutscene_name(run) {
            if fought.scenes.last().is_none_or(|(_, n)| *n != cs) {
                fought.scenes.push((run.steps, cs));
            }
            still(run, stops, [false; 4]);
            continue;
        }
        if guacamole::anchors::scene_holds(&run.vm) {
            still(run, stops, [false; 4]);
            continue;
        }
        fought.played += 1;
        let me = run.hero().pivot;
        let near = foes
            .iter()
            .map(|n| run.placed(n).0)
            .filter(|h| {
                run.vm
                    .placement_of(*h)
                    .is_some_and(|i| !run.vm.removed(i) && !run.vm.asleep(i))
            })
            .filter_map(|h| {
                run.world
                    .actor(h)
                    .filter(|a| !a.removed)
                    .map(|a| a.position)
            })
            .filter(|p| (p.y - me.y).abs() < 12.0 && flat(*p, me) < 40.0)
            .min_by(|a, b| a.distance(me).total_cmp(&b.distance(me)));
        let target = near.unwrap_or(goal);
        let far = flat(target, me);
        if near.is_none() && far < 1.0 {
            still(run, stops, [false; 4]);
            continue;
        }
        let stale = route.last().is_none_or(|e: &Vec3| e.distance(target) > 3.0);
        if routed + 90 <= run.steps || stale {
            route = way_to(run, me, target);
            (routed, next) = (run.steps, 0);
        }
        let aim = if far <= 4.0 || route.is_empty() {
            target
        } else {
            next = (next..(next + 6).min(route.len()))
                .min_by(|a, b| flat(me, route[*a]).total_cmp(&flat(me, route[*b])))
                .unwrap_or(next);
            route[next..]
                .iter()
                .find(|c| flat(me, **c) > 1.5)
                .copied()
                .unwrap_or(target)
        };
        pressed = if near.is_some() && far <= 2.5 {
            pressed + 1
        } else {
            0
        };
        let to = if pressed > 240 {
            if pressed > 270 {
                pressed = 0;
            }
            Vec3::new(me.x * 2.0 - aim.x, me.y, me.z * 2.0 - aim.z)
        } else {
            aim
        };
        run.bench.view = (to.x - me.x).atan2(to.z - me.z).to_degrees();
        run.bench.hold_view = true;
        let push = if far > 2.5 || pressed > 240 {
            1.0
        } else {
            0.35
        };
        let swing = near.is_some() && far <= 5.0 && run.steps.is_multiple_of(20);
        step(
            run,
            stops,
            Stick::new(0.0, push),
            [swing, false, false, false],
        );
    }
}

/// The reveal's five Ice Chompies (`Setup_001_Enemies`).
const CHOMPIES: [&str; 5] = [
    "Enemy_Chompy_Ice(2)",
    "Enemy_Chompy_Ice(3)",
    "Enemy_Chompy_Ice(20)",
    "Enemy_Chompy_Ice(21)",
    "Enemy_Chompy_Ice(40)",
];

/// Setup_002's controller, its trigger and its six (`Setup_002_Enemies`).
const SETUP_002_CONTROLLER: &str = "Setup_002_Controller";
const SETUP_002_TRIGGER: &str = "Setup_002_Trigger(1)";
const SETUP_002_FOES: [&str; 6] = [
    "Enemy_Cyclops_Near(20)",
    "Enemy_Cyclops_Near(21)",
    "Enemy_Cyclops_Near(22)",
    "Enemy_Cyclops_Near(23)",
    "Enemy_Cyclops_Near(24)",
    "Enemy_Biclops_Heavy_S2(1)",
];

/// What else Setup_002's fight touches: Noodles' scene at the first gate
/// and the spike gate by its spinners.
const SETUP_002_WATCH: [&str; 3] = [
    SETUP_002_CONTROLLER,
    "IGC_Noodles_FirstGate",
    "Spike_Gate_Metal_Template(1)",
];

fn removed(run: &Run, name: &str) -> bool {
    behaviour(run, name) == "removed"
}

/// The step `name` was first in one of `states`, from `log`.
fn first_in(log: &[(u32, String, String)], name: &str, states: &[&str]) -> Option<u32> {
    log.iter()
        .find(|(_, n, b)| n == name && states.contains(&b.as_str()))
        .map(|e| e.0)
}

/// openspec AS.345: level 208's first fights on their own scripts. The
/// intro played out and the exit opened as AS.344's test opens it, the
/// reveal's five Ice Chompies, walked, notice him, chase him and breathe
/// ice on him as he stands among them, and are beaten. Then, put by
/// Setup_002's trigger on the frozen river (the way down from the yard
/// is AS.346's), its controller sets its two spinning Cyclopes on him;
/// with both beaten Noodles' first-gate scene plays, then the heavy
/// Biclops comes and the three adds walk in, and with all six beaten the
/// controller takes its set off and the spike gate by the spinners
/// lowers. Nothing stops.
#[test]
#[ignore = "reads the Giants dump"]
fn level_208s_first_fights_by_their_own_triggers() {
    println!("load at start: {}", load());
    let mut stops = Vec::new();
    let mut run = joined_208(&mut stops);
    for k in 0..6000u32 {
        still(&mut run, &mut stops, [false; 4]);
        if k > 200 && scene_of(&run).is_none() && !guacamole::anchors::scene_holds(&run.vm) {
            break;
        }
    }
    assert!(intro_off(&run), "the intro played out");
    println!("handed over at step {} at {}", run.steps, run.hero().pivot);
    let out = walk_out(&mut run, &mut stops, 6000);
    println!("{out:?}, at {}", run.hero().pivot);
    assert!(out.reached.is_some(), "at Setup_002's place");
    // Stood among them: they come for him and hurt him.
    let (hp0, start) = (run.health().unwrap(), run.steps);
    let mut stood: Vec<(u32, String, String)> = Vec::new();
    let mut last: HashMap<&str, String> = HashMap::new();
    let mut nearest = [f32::MAX; 5];
    for _ in 0..600 {
        for (k, n) in CHOMPIES.iter().enumerate() {
            let b = behaviour(&run, n);
            if last.get(n) != Some(&b) {
                stood.push((run.steps, (*n).to_owned(), b.clone()));
                last.insert(n, b.clone());
            }
            if b != "removed"
                && let Some(a) = run.world.actor(run.placed(n).0)
            {
                nearest[k] = nearest[k].min(a.position.distance(run.hero().pivot));
            }
        }
        if run.health().unwrap() < hp0 - 10.0 {
            break;
        }
        still(&mut run, &mut stops, [false; 4]);
    }
    let hurt = hp0 - run.health().unwrap();
    println!(
        "stood {} steps: {hurt} health lost, nearest {nearest:?}, {stood:?}",
        run.steps - start
    );
    assert!(hurt > 0.0, "the Chompies hurt him");
    let fighting = CHOMPIES
        .iter()
        .filter(|n| first_in(&stood, n, &["battle", "attack - freeze"]).is_some())
        .count();
    assert!(fighting >= 3, "they fight him: {stood:?}");
    let chompies = fight_to(
        &mut run,
        &mut stops,
        &CHOMPIES,
        &[],
        SETUP_002,
        6000,
        &|run: &Run| CHOMPIES.iter().all(|n| removed(run, n)),
    );
    for (step, n, b) in &chompies.log {
        println!("  {step} {n} {b}");
    }
    println!(
        "chompies: ended {} at step {}, health {:?}",
        chompies.ended, run.steps, chompies.health
    );
    assert!(chompies.ended, "the five beaten");
    // The yard's ways down to the river (its ice traps and slide) are
    // openspec AS.346's: put by Setup_002's trigger.
    let trigger = run.at(SETUP_002_TRIGGER);
    run.put(trigger, 270.0);
    // How far short of the river past the spike gate a walk from him
    // ends (the flood's cells).
    let past_gate = |run: &Run| {
        let to = Vec3::new(-30.0, 4.6, -43.8);
        way_to(run, run.hero().pivot, to)
            .last()
            .unwrap()
            .distance(to)
    };
    let before = past_gate(&run);
    let fought = fight_to(
        &mut run,
        &mut stops,
        &SETUP_002_FOES,
        &SETUP_002_WATCH,
        trigger,
        12000,
        &|run: &Run| removed(run, SETUP_002_CONTROLLER),
    );
    // What its end opens, once its scene's out.
    let mut tail: Vec<(u32, String, String)> = Vec::new();
    let mut last: HashMap<&str, String> = HashMap::new();
    for _ in 0..1800 {
        for n in SETUP_002_WATCH {
            let b = behaviour(&run, n);
            if last.get(n) != Some(&b) {
                tail.push((run.steps, n.to_owned(), b.clone()));
                last.insert(n, b);
            }
        }
        if cutscene_name(&run).is_none()
            && !guacamole::anchors::scene_holds(&run.vm)
            && removed(&run, "IGC_Noodles_FirstGate")
        {
            break;
        }
        still(&mut run, &mut stops, [false; 4]);
    }
    let log: Vec<_> = fought.log.iter().chain(&tail).cloned().collect();
    for (step, n, b) in &log {
        println!("  {step} {n} {b}");
    }
    let gate = run
        .world
        .actor(run.placed("Spike_Gate_Metal_Template(1)").0);
    println!(
        "setup_002: ended {} at step {}, health {:?}, scenes {:?}, played {} steps, gate {:?}",
        fought.ended,
        run.steps,
        fought.health,
        fought.scenes,
        fought.played,
        gate.map(|g| (
            g.removed,
            g.stands_for_actors(),
            g.solid.interacts_with_actors
        ))
    );
    let trail: Vec<String> = chompies
        .trail
        .iter()
        .chain(&fought.trail)
        .map(|p| format!("{:.2},{:.2},{:.2}", p.x, p.y, p.z))
        .collect();
    println!("TRAIL {}", trail.join(";"));
    assert!(fought.ended, "Setup_002's controller off: {log:?}");
    let off = first_in(&log, SETUP_002_CONTROLLER, &["removed"]).unwrap();
    let woke = |n: &str| {
        log.iter()
            .find(|(_, m, b)| m == n && !["asleep", "removed"].contains(&b.as_str()))
            .map(|e| e.0)
    };
    // The spinners first, by its trigger.
    let spinners = ["Enemy_Cyclops_Near(20)", "Enemy_Cyclops_Near(21)"];
    for n in spinners {
        assert!(woke(n).is_some(), "{n} woke: {log:?}");
    }
    let spinners_off = spinners
        .iter()
        .map(|n| first_in(&log, n, &["removed"]).unwrap())
        .max()
        .unwrap();
    // Noodles' scene once both are beaten, then the Biclops, then the adds.
    let noodles = first_in(&log, "IGC_Noodles_FirstGate", &["init"]).unwrap();
    assert!(noodles >= spinners_off, "Noodles after the spinners");
    assert!(
        fought.scenes.iter().any(|(s, _)| *s >= spinners_off),
        "his scene plays: {:?}",
        fought.scenes
    );
    let biclops = first_in(&log, SETUP_002_CONTROLLER, &["biclops - execute"]).unwrap();
    assert!(biclops > noodles);
    assert!(woke("Enemy_Biclops_Heavy_S2(1)").is_some_and(|w| w >= biclops));
    for n in [
        "Enemy_Cyclops_Near(22)",
        "Enemy_Cyclops_Near(23)",
        "Enemy_Cyclops_Near(24)",
    ] {
        assert!(
            woke(n).is_some_and(|w| w >= biclops),
            "{n} after the Biclops"
        );
    }
    for n in SETUP_002_FOES {
        let gone = first_in(&log, n, &["removed"]);
        assert!(gone.is_some_and(|g| g <= off), "{n}: {gone:?}");
    }
    // Its end opens the spike gate by the spinners.
    let lowered = first_in(&log, "Spike_Gate_Metal_Template(1)", &["lower spikes"]);
    assert!(
        lowered.is_some_and(|l| l >= off),
        "the gate lowers: {log:?}"
    );
    let after = past_gate(&run);
    println!("past the gate {before:.2} m off before, {after:.2} after");
    assert!(before > 3.0 && after < 1.0, "{before} m, {after} m");
    assert!(stops.is_empty(), "{stops:#?}");
    println!("load at end: {}", load());
}

/// The AI's hint in the yard and the blocker it takes off (openspec
/// AS.406).
const AI: &str = "IGC_AI_Intro";
const AI_BLOCKER: &str = "Pandoran_Hint_Template(1)";

/// The first ice block, across the lower terrace's way to the river
/// (openspec AS.406, AS.431).
const ICE_BLOCK: &str = "Melting_Ice_blocks_reflect_1";

/// Whether placement `name`'s actor is off the level.
fn gone_actor(run: &Run, name: &str) -> bool {
    run.world
        .actor(run.placed(name).0)
        .is_none_or(|a| a.removed)
}

/// The lower terrace east of the river, below the yard, by the first ice
/// block's switch (openspec AS.406).
const TERRACE: Vec3 = Vec3::new(3.6, 6.7, -65.0);

/// openspec AS.406, AS.431: level 208 from the intro's hand-over down to the
/// lower terrace and on to Setup_002's trigger by the stick, the attack and
/// the talk button alone: out of the plaza and through the reveal's five
/// Chompies as AS.344's and AS.345's tests go, then to the AI in the yard,
/// which waits on the five gone and a player within 30 ft of it, plays its
/// pan and takes its hint's blocker off the yard's east side; then east and
/// down to the terrace. Before the hint the yard's ground doesn't reach the
/// terrace. On toward the trigger the first ice block holds him on the
/// terrace until its beam puzzle melts it: the talk button at the pivot's
/// switch turns the pivot until the start's beam, through its crystal,
/// reaches the block; then the stick takes him to the trigger. On from there
/// (openspec AS.401, AS.346, AS.701): Setup_002's six beaten by the stick and
/// the attack, its spike gate lowered, the brick wall across the river past
/// it beaten, the push block pushed west along its track to its end and the
/// second pivot turned until its beam lights the block's crystal, whose own
/// beam reaches the second ice block. Nothing stops.
#[test]
#[ignore = "reads the Giants dump"]
fn level_208s_way_by_the_stick_to_the_second_beam() {
    println!("load at start: {}", load());
    let mut stops = Vec::new();
    let mut run = joined_208(&mut stops);
    for k in 0..6000u32 {
        still(&mut run, &mut stops, [false; 4]);
        if k > 200 && scene_of(&run).is_none() && !guacamole::anchors::scene_holds(&run.vm) {
            break;
        }
    }
    assert!(intro_off(&run), "the intro played out");
    println!("handed over at step {} at {}", run.steps, run.hero().pivot);
    let out = walk_out(&mut run, &mut stops, 6000);
    assert!(out.reached.is_some(), "at Setup_002's place: {out:?}");
    let chompies = fight_to(
        &mut run,
        &mut stops,
        &CHOMPIES,
        &[],
        SETUP_002,
        6000,
        &|run: &Run| CHOMPIES.iter().all(|n| removed(run, n)),
    );
    println!(
        "chompies beaten {} at step {}, health {:?}",
        chompies.ended, run.steps, chompies.health
    );
    assert!(chompies.ended, "the five beaten");
    // Before the hint, the yard's ground ends short of the terrace.
    let short = |run: &Run| {
        way_to(run, run.hero().pivot, TERRACE)
            .last()
            .unwrap()
            .distance(TERRACE)
    };
    let before = short(&run);
    assert!(!gone_actor(&run, AI_BLOCKER), "the blocker stands");
    // To the AI: its hint takes the blocker off.
    let ai = run.at(AI);
    let hint = fight_to(&mut run, &mut stops, &[], &[AI], ai, 3000, &|run: &Run| {
        gone_actor(run, AI_BLOCKER)
    });
    for (step, n, b) in &hint.log {
        println!("  {step} {n} {b}");
    }
    println!(
        "the AI's hint: blocker off {} at step {}, scenes {:?}, at {}",
        hint.ended,
        run.steps,
        hint.scenes,
        run.hero().pivot
    );
    assert!(hint.ended, "the AI's blocker off: {:?}", hint.log);
    let woke = first_in(&hint.log, AI, &["wait", "go"]);
    assert!(woke.is_some(), "the AI talked: {:?}", hint.log);
    assert!(
        hint.scenes.iter().any(|(s, _)| *s >= woke.unwrap()),
        "its pan: {:?}",
        hint.scenes
    );
    let after = short(&run);
    println!("the terrace {before:.2} m off the yard's ground before the hint, {after:.2} after");
    assert!(before > 5.0 && after < 1.0, "{before} m, {after} m");
    // East and down to the terrace.
    let flat = |a: Vec3, b: Vec3| Vec3::new(a.x - b.x, 0.0, a.z - b.z).length();
    let down = fight_to(
        &mut run,
        &mut stops,
        &[],
        &[],
        TERRACE,
        3000,
        &|run: &Run| flat(run.hero().pivot, TERRACE) < 2.0,
    );
    let trail: Vec<String> = hint
        .trail
        .iter()
        .chain(&down.trail)
        .map(|p| format!("{:.2},{:.2},{:.2}", p.x, p.y, p.z))
        .collect();
    println!("TRAIL {}", trail.join(";"));
    println!(
        "down: on the terrace {} at step {}, at {}, health {:?}",
        down.ended,
        run.steps,
        run.hero().pivot,
        down.health
    );
    assert!(down.ended, "on the terrace: {}", run.hero().pivot);
    assert!(run.hero().pivot.y < 7.5, "below the yard");
    // On west toward Setup_002's trigger: the first ice block, across the
    // terrace's way to the river, holds him.
    let block = run.at(ICE_BLOCK);
    let on = fight_to(&mut run, &mut stops, &[], &[], block, 1800, &|run: &Run| {
        flat(run.hero().pivot, block) < 1.0
    });
    let held = run.hero().pivot;
    println!(
        "on toward the trigger: held at {held} by the ice block {:.2} m off, {:.2} m short of the trigger",
        flat(held, block),
        flat(held, run.at(SETUP_002_TRIGGER))
    );
    assert!(
        !on.ended && held.x > block.x,
        "held east of the block: {held}"
    );
    // The Chompies' freeze crystals came off him with the freeze (openspec
    // AS.432): made in the fight, each by the terrace off his node at alpha
    // 0 (drawn as nothing) or gone. Its going waits on its scale falling
    // under 60 percent, which nothing run here lowers.
    let prisons: Vec<(bool, Option<String>)> = run
        .world
        .clones()
        .into_iter()
        .filter(|h| run.world.name(*h) == "Chompy_Ice_Prison")
        .map(|h| {
            let a = run.world.actor(h);
            let off = a.is_none_or(|a| {
                a.removed || (a.attach == guac_script::Value::Null && a.tint & 0xFF == 0)
            });
            let b = run.vm.placement_of(h).and_then(|i| run.vm.behaviour(i));
            (off, b.map(str::to_owned))
        })
        .collect();
    println!("freeze crystals made {}: {prisons:?}", prisons.len());
    assert!(!prisons.is_empty(), "the Chompies froze him");
    assert!(prisons.iter().all(|(off, _)| *off), "a crystal left on him");
    // The beam puzzle: the talk button at the pivot's switch until the
    // block melts and is gone (openspec AS.431).
    let mut melt = turn_the_beam(&mut run, &mut stops, 6000);
    for (step, n, b) in &melt.log {
        println!("  {step} {n} {b}");
    }
    println!(
        "the beam: {} presses at the switch {:?}, the block powered at {:?}, gone at {:?}, scenes {:?}",
        melt.presses, melt.switch, melt.powered, melt.melted, melt.scenes
    );
    // The block's scene played on his way to it (its pan; nothing powers
    // the start before it ends) and took itself off.
    println!("scenes on the way to the block {:?}", on.scenes);
    assert!(
        removed(&run, "IGC_FirstIceBlock"),
        "its scene played out: {:?}",
        melt.log
    );
    assert!(melt.melted.is_some(), "the block melted: {:?}", melt.log);
    // Both beams drawn as the block melts (openspec AS.700): the start's to
    // the pivot and the pivot's to the block, each with the width its
    // scripts gave it, the template's.
    println!("the beams as the block melts {:?}", melt.beams);
    let block_at = run.at(ICE_BLOCK);
    let pivot_at = run.at(PIVOT);
    // The second set's own two shine already, west past the river (openspec
    // AS.701): its start wakes powered.
    melt.beams
        .retain(|(from, _, _)| flat(*from, pivot_at) < 20.0);
    assert_eq!(melt.beams.len(), 2, "{:?}", melt.beams);
    let to_block = melt
        .beams
        .iter()
        .filter(|(_, end, _)| flat(*end, block_at) < 3.0)
        .count();
    let to_pivot = melt
        .beams
        .iter()
        .filter(|(_, end, _)| flat(*end, pivot_at) < 3.0)
        .count();
    assert_eq!((to_block, to_pivot), (1, 1), "{:?}", melt.beams);
    assert!(
        melt.beams
            .iter()
            .all(|(from, end, w)| from.distance(*end) > 3.0 && w.is_some_and(|w| w > 0.0)),
        "{:?}",
        melt.beams
    );
    assert!((1..=8).contains(&melt.presses), "{} presses", melt.presses);
    // The pivot's crystal hangs on its model's last node, over the pivot,
    // not at it (AS.431: a set of 32 put `last` past the model's list).
    let pivot = run.at(PIVOT);
    let crystal = run
        .world
        .clones()
        .into_iter()
        .filter(|h| run.world.name(*h) == "Reflect_Crystal")
        .filter_map(|h| run.world.actor(h).filter(|a| !a.removed))
        .map(|a| a.position.distance(pivot))
        .fold(f32::INFINITY, f32::min);
    println!("the pivot's crystal {crystal:.2} m off it");
    assert!((1.0..4.0).contains(&crystal), "{crystal} m off the pivot");
    // On west to Setup_002's trigger.
    let trigger = run.at(SETUP_002_TRIGGER);
    let down = fight_to(
        &mut run,
        &mut stops,
        &[],
        &[SETUP_002_CONTROLLER],
        trigger,
        3000,
        &|run: &Run| flat(run.hero().pivot, trigger) < 2.0,
    );
    let trail: Vec<String> = down
        .trail
        .iter()
        .map(|p| format!("{:.2},{:.2},{:.2}", p.x, p.y, p.z))
        .collect();
    println!("TRAIL-TRIGGER {}", trail.join(";"));
    println!(
        "to the trigger {} at step {}, at {} ({:.2} m off)",
        down.ended,
        run.steps,
        run.hero().pivot,
        flat(run.hero().pivot, trigger)
    );
    assert!(down.ended, "at Setup_002's trigger: {}", run.hero().pivot);
    // Setup_002 on its trigger (openspec AS.401): its six beaten by the
    // stick and the attack, the controller off, the spike gate lowered.
    let hp = run.health();
    let river = fight_to(
        &mut run,
        &mut stops,
        &SETUP_002_FOES,
        &SETUP_002_WATCH,
        trigger,
        12000,
        &|run: &Run| removed(run, SETUP_002_CONTROLLER),
    );
    println!(
        "setup 2 {} at step {}, health {hp:?} -> {:?}, lowest {:?}, gate {}",
        river.ended,
        run.steps,
        run.health(),
        river.health,
        behaviour(&run, GATE)
    );
    assert!(river.ended, "Setup_002 beaten: {:?}", river.log);
    assert_eq!(behaviour(&run, GATE), "lower spikes");
    // West past the gate to the river wall, which holds him until it's
    // beaten (openspec AS.346).
    let past_gate = Vec3::new(-31.0, 4.6, -43.8);
    let gate = fight_to(
        &mut run,
        &mut stops,
        &[],
        &[],
        past_gate,
        3000,
        &|run: &Run| flat(run.hero().pivot, past_gate) < 2.0,
    );
    assert!(gate.ended, "past the gate: {}", run.hero().pivot);
    let wall_at = run.at(RIVER_WALL);
    let past_wall = Vec3::new(-42.6, 3.3, -49.2);
    let wall = fight_to(
        &mut run,
        &mut stops,
        &[RIVER_WALL],
        &[RIVER_WALL],
        past_wall,
        3000,
        &|run: &Run| flat(run.hero().pivot, past_wall) < 2.0,
    );
    for (step, n, b) in &wall.log {
        println!("  {step} {n} {b}");
    }
    println!(
        "past the river wall {} at step {} at {}, bricks laid {}",
        wall.ended,
        run.steps,
        run.hero().pivot,
        wall.log.iter().any(|(_, _, b)| b == "main loop")
    );
    assert!(wall.ended, "past the river wall: {:?}", wall.log);
    assert!(removed(&run, RIVER_WALL), "the wall beaten");
    assert!(run.hero().pivot.x < wall_at.x - 4.0, "{}", run.hero().pivot);
    // The second beam puzzle (openspec AS.701's set 2): the push block
    // pushed west by the stick to its track's end, the pivot's switch, the
    // talk button until the pivot's beam lights the block's crystal.
    let second = second_beam(&mut run, &mut stops);
    println!("the second beam: {second:?}");
    let block_at = second.block.expect("the push block");
    assert!(
        flat(block_at, PUSH_END) < 0.5,
        "pushed to its track's end: {block_at}"
    );
    let relay = second.relay.expect("the crystal's beam");
    let ice = run.at(ICE_BLOCK_2);
    assert!(
        relay.1.x < relay.0.x - 4.0 && flat(relay.1, ice) < 5.0,
        "the crystal's beam west at the second ice block: {relay:?}"
    );
    // Melted (openspec AS.755): the relay's cone, taken from the push
    // block it hangs on, holds the block; it melts and takes itself off.
    // A stall (no change in 600 steps) ends it.
    let (mut last, mut since, mut melted) = (behaviour(&run, ICE_BLOCK_2), 0u32, None);
    for _ in 0..3000 {
        still(&mut run, &mut stops, [false; 4]);
        if removed(&run, ICE_BLOCK_2) {
            melted = Some(run.steps);
            break;
        }
        let b = behaviour(&run, ICE_BLOCK_2);
        if b != last {
            println!("  {} {ICE_BLOCK_2} {b}", run.steps);
            (last, since) = (b, 0);
        } else {
            since += 1;
            assert!(since < 600, "the second ice block stalled at {last}");
        }
    }
    println!("the second ice block melted at step {melted:?}");
    assert!(melted.is_some(), "the second ice block melted");
    // And the way west past where it stood is open to the stick.
    let past_block = Vec3::new(-68.0, 3.4, -58.0);
    let west = fight_to(
        &mut run,
        &mut stops,
        &[],
        &[],
        past_block,
        3000,
        &|run: &Run| flat(run.hero().pivot, past_block) < 2.0,
    );
    println!(
        "past the second ice block {} at {}",
        west.ended,
        run.hero().pivot
    );
    assert!(
        west.ended,
        "past the second ice block: {}",
        run.hero().pivot
    );
    assert!(stops.is_empty(), "{stops:#?}");
    println!("load at end: {}", load());
}

/// openspec AS.776 and AS.796: west of the second ice block, set down where
/// the way test leaves him: the stick and the attack through the two brick
/// walls (each laid as he comes, beaten, taken off), then the third push
/// block pushed south by the stick into the third start's beam until it
/// stops, and the third ice block melted and taken off; then the fourth
/// ([`fourth_ice_block`]) and on by the stick to Setup_003's first trigger,
/// which lay inside its box. A stall (no change in 600 steps) ends it.
#[test]
#[ignore = "reads the Giants dump"]
fn level_208s_way_west_of_the_second_ice_block() {
    println!("load at start: {}", load());
    let mut stops = Vec::new();
    let run = west_to_setup_003(&mut stops);
    println!("at Setup_003's first trigger at step {}", run.steps);
    assert!(stops.is_empty(), "{stops:#?}");
    println!("load at end: {}", load());
}

/// [`level_208s_way_west_of_the_second_ice_block`]'s walk, from where the
/// way test leaves him to Setup_003's first trigger, asserted on the way.
fn west_to_setup_003(stops: &mut Vec<String>) -> Run {
    let mut run = joined_208(stops);
    for k in 0..6000u32 {
        still(&mut run, stops, [false; 4]);
        if k > 200 && scene_of(&run).is_none() && !guacamole::anchors::scene_holds(&run.vm) {
            break;
        }
    }
    run.put(Vec3::new(-68.0, 3.4, -58.0), 270.0);
    let flat = |a: Vec3, b: Vec3| Vec3::new(a.x - b.x, 0.0, a.z - b.z).length();
    let mut walls = Vec::new();
    for (wall, past) in [
        ("Wall_Spawner_Template(6)", Vec3::new(-95.0, 5.0, -57.0)),
        ("Wall_Spawner_Template(7)", Vec3::new(-103.0, 5.0, -40.0)),
    ] {
        let f = fight_to(
            &mut run,
            stops,
            &[wall],
            &[wall],
            past,
            3000,
            &|run: &Run| flat(run.hero().pivot, past) < 2.0,
        );
        let laid = f.log.iter().any(|(_, _, b)| b == "main loop");
        println!(
            "{wall}: past {} at step {} at {}, laid {laid}, removed {}",
            f.ended,
            run.steps,
            run.hero().pivot,
            removed(&run, wall)
        );
        walls.push((f.ended, laid, removed(&run, wall)));
    }
    // Unlisted, (6) held him short of (-95, -57) and (7) of the way north
    // past the push block, each an invisible wall.
    assert!(
        walls.iter().all(|w| *w == (true, true, true)),
        "both walls laid, beaten and past: {walls:?}"
    );
    // The third set: north of the push block, the stick south until it
    // stops moving.
    let block = run.at(PUSH_BLOCK_9);
    let north = Vec3::new(block.x, block.y, block.z + 4.5);
    let to = fight_to(&mut run, stops, &[], &[], north, 3000, &|run: &Run| {
        flat(run.hero().pivot, north) < 1.0
    });
    assert!(
        to.ended,
        "north of the third push block: {}",
        run.hero().pivot
    );
    let (mut last, mut moves, mut still_for) = (behaviour(&run, PUSH_BLOCK_9), 0u32, 0u32);
    for _ in 0..1500 {
        let me = run.hero().pivot;
        let b = run.at(PUSH_BLOCK_9);
        run.bench.view = (b.x - me.x).atan2(-10.0).to_degrees();
        run.bench.hold_view = true;
        step(&mut run, stops, Stick::new(0.0, 1.0), [false; 4]);
        let bh = behaviour(&run, PUSH_BLOCK_9);
        if bh != last {
            moves += u32::from(bh == "2. moving");
            (last, still_for) = (bh, 0);
        } else {
            still_for += 1;
        }
        if moves > 0 && still_for > 300 {
            break;
        }
    }
    run.bench.hold_view = false;
    println!(
        "the third push block: {moves} moves to {}",
        run.at(PUSH_BLOCK_9)
    );
    assert!(moves >= 2, "pushed south: {moves}");
    let (mut last, mut since, mut melted) = (behaviour(&run, ICE_BLOCK_3), 0u32, None);
    for _ in 0..3000 {
        still(&mut run, stops, [false; 4]);
        if removed(&run, ICE_BLOCK_3) {
            melted = Some(run.steps);
            break;
        }
        let b = behaviour(&run, ICE_BLOCK_3);
        if b != last {
            println!("  {} {ICE_BLOCK_3} {b}", run.steps);
            (last, since) = (b, 0);
        } else {
            since += 1;
            assert!(since < 600, "the third ice block stalled at {last}");
        }
    }
    println!("the third ice block melted at step {melted:?}");
    assert!(melted.is_some(), "the third ice block melted");
    // The fourth set (openspec AS.776): listed, its block stands across the
    // way to Setup_003's first trigger as an ice block waiting on its beam
    // (unlisted, an invisible wall), and its start beams to the pivot.
    assert_eq!(behaviour(&run, ICE_BLOCK_4), "wait for beam to hit");
    // Its post runs its script, drawn where it stands (unlisted, its box
    // stands undrawn: an invisible wall).
    assert_eq!(behaviour(&run, "Reflect_Post(1)"), "wait forever");
    let (start_4, pivot_3) = (run.at("Reflect_Start(4)"), run.at("Reflect_Pivot_Round(3)"));
    let fourth = beams(&run);
    println!("the fourth set's beams: {fourth:?}");
    assert!(
        fourth
            .iter()
            .any(|(from, end, _)| flat(*from, start_4) < 4.0 && flat(*end, pivot_3) < 1.0),
        "the fourth start's beam to its pivot: {fourth:?}"
    );
    fourth_ice_block(&mut run, stops);
    // The block's box off, the way to Setup_003's first trigger is open.
    let trigger = run.at("Setup_003_Trigger(1)");
    let to = fight_to(&mut run, stops, &[], &[], trigger, 3000, &|run: &Run| {
        flat(run.hero().pivot, trigger) < 2.0
    });
    assert!(
        to.ended,
        "at Setup_003's first trigger: {}",
        run.hero().pivot
    );
    run
}

/// openspec AS.796: the fourth set as a kid works it, with the third block
/// melted and its push block at its track's end: the push block taken back
/// by the stick, north to where it started and east to its track's first
/// waypoint (its own placed spot), then the talk button at the fourth
/// pivot's switch every 2 s, at most eight presses, until the block melts.
/// Start 4 beams to the pivot, the pivot to the push block's crystal, the
/// crystal south to the post and the post to the block.
fn fourth_ice_block(run: &mut Run, stops: &mut Vec<String>) {
    let placed = run.placed(PUSH_BLOCK_9).1;
    let north = push_until_still(run, stops, PUSH_BLOCK_9, Vec3::new(0.0, 0.0, 1.0));
    let east = push_until_still(run, stops, PUSH_BLOCK_9, Vec3::new(1.0, 0.0, 0.0));
    let block = run.at(PUSH_BLOCK_9);
    println!("the third push block back: {north} north, {east} east, to {block}");
    assert!(
        (north, east) == (2, 2) && flat(block, placed) < 0.2,
        "pushed back to its placed spot {placed}: {north} north, {east} east, at {block}"
    );
    let pivot = run.at(PIVOT_3);
    let switch = run
        .world
        .clones()
        .into_iter()
        .filter(|h| run.world.name(*h).starts_with("Reflect_Switchbase"))
        .filter_map(|h| {
            run.world
                .actor(h)
                .filter(|a| !a.removed)
                .map(|a| a.position)
        })
        .find(|p| flat(*p, pivot) < 10.0)
        .expect("the fourth set's switch");
    let to = fight_to(run, stops, &[], &[], switch, 3000, &|run: &Run| {
        flat(run.hero().pivot, switch) < 2.5
    });
    assert!(to.ended, "at the fourth switch: {}", run.hero().pivot);
    let mut presses = 0;
    while presses < 8 && behaviour(run, ICE_BLOCK_4) == "wait for beam to hit" {
        presses += 1;
        for k in 0..120 {
            still(run, stops, [false, false, false, k == 0]);
        }
    }
    let chain = beams(run);
    println!("the fourth set after {presses} presses: {chain:?}");
    let post = run.at(POST);
    let ice = run.at(ICE_BLOCK_4);
    // Each beam from its carrier to its target (the block's on its face, a
    // box 6.9 by 10.3 m).
    for (what, from, to, reach) in [
        ("the pivot to the push block", pivot, block, 1.0),
        ("the push block to the post", block, post, 1.0),
        ("the post to the block", post, ice, 4.0),
    ] {
        assert!(
            chain
                .iter()
                .any(|(f, e, _)| flat(*f, from) < 3.0 && flat(*e, to) < reach),
            "{what}: {chain:?}"
        );
    }
    let (mut last, mut since, mut melted) = (behaviour(run, ICE_BLOCK_4), 0u32, None);
    for _ in 0..3000 {
        still(run, stops, [false; 4]);
        if removed(run, ICE_BLOCK_4) {
            melted = Some(run.steps);
            break;
        }
        let b = behaviour(run, ICE_BLOCK_4);
        if b != last {
            println!("  {} {ICE_BLOCK_4} {b}", run.steps);
            (last, since) = (b, 0);
        } else {
            since += 1;
            assert!(since < 600, "the fourth ice block stalled at {last}");
        }
    }
    println!("the fourth ice block melted at step {melted:?} after {presses} presses");
    assert!(melted.is_some(), "the fourth ice block melted");
}

/// The stick at a push block from the side opposite `dir`, held along
/// `dir` until it stops moving (300 steps with no change): how many moves
/// it made.
fn push_until_still(run: &mut Run, stops: &mut Vec<String>, name: &str, dir: Vec3) -> u32 {
    let b = run.at(name);
    let behind = Vec3::new(b.x - dir.x * 3.6, b.y, b.z - dir.z * 3.6);
    let to = fight_to(run, stops, &[], &[], behind, 3000, &|run: &Run| {
        flat(run.hero().pivot, behind) < 1.0
    });
    assert!(to.ended, "behind {name} for {dir}: {}", run.hero().pivot);
    let (mut last, mut moves, mut still_for) = (behaviour(run, name), 0u32, 0u32);
    for _ in 0..1500 {
        run.bench.view = dir.x.atan2(dir.z).to_degrees();
        run.bench.hold_view = true;
        step(run, stops, Stick::new(0.0, 1.0), [false; 4]);
        let bh = behaviour(run, name);
        if bh != last {
            moves += u32::from(bh == "2. moving");
            (last, still_for) = (bh, 0);
        } else {
            still_for += 1;
        }
        if still_for > 300 {
            break;
        }
    }
    run.bench.hold_view = false;
    moves
}

/// Ground distance.
fn flat(a: Vec3, b: Vec3) -> f32 {
    Vec3::new(a.x - b.x, 0.0, a.z - b.z).length()
}

/// openspec AS.402: Setup_003 on its own triggers, set down on its first
/// trigger (the walked way there is
/// [`level_208s_setup_003_walked_from_the_fourth_ice_block`]'s); then the
/// stick and the attack at its foes until its controller is off, 24,000
/// steps at most.
#[test]
#[ignore = "reads the Giants dump"]
fn level_208s_setup_003_by_its_own_triggers() {
    println!("load at start: {}", load());
    let mut stops = Vec::new();
    let mut run = joined_208(&mut stops);
    for k in 0..6000u32 {
        still(&mut run, &mut stops, [false; 4]);
        if k > 200 && scene_of(&run).is_none() && !guacamole::anchors::scene_holds(&run.vm) {
            break;
        }
    }
    assert_eq!(behaviour(&run, SETUP_003_CONTROLLER), "asleep");
    let trigger = run.at("Setup_003_Trigger(1)");
    run.put(trigger, 300.0);
    let mut held = 0;
    while held < 600 && behaviour(&run, SETUP_003_CONTROLLER).starts_with("bomber - execute")
        || held < 60
    {
        still(&mut run, &mut stops, [false; 4]);
        held += 1;
    }
    println!(
        "on the trigger {held} steps: {}, the reveal {}",
        behaviour(&run, SETUP_003_CONTROLLER),
        behaviour(&run, "Cyclops_Bomber_IGC_Trigger")
    );
    // On the trigger the controller runs the bomber's reveal: past its
    // wait, the reveal playing or played (a break fails here, not at the
    // cap).
    let on = behaviour(&run, SETUP_003_CONTROLLER);
    assert!(
        !["asleep", "bomber - execute trigger"].contains(&on.as_str()),
        "Setup_003 woken by its trigger: {on}"
    );
    assert!(
        ["cutscene out", "done", "removed"]
            .contains(&behaviour(&run, "Cyclops_Bomber_IGC_Trigger").as_str()),
        "the bomber's reveal"
    );
    let fought = fight_to(
        &mut run,
        &mut stops,
        &SETUP_003_FOES,
        &[SETUP_003_CONTROLLER, "Cyclops_Bomber_IGC_Trigger"],
        Vec3::new(-151.8, 8.2, -33.9),
        24000,
        &|run: &Run| removed(run, SETUP_003_CONTROLLER),
    );
    for (step, n, b) in &fought.log {
        println!("  {step} {n} {b}");
    }
    println!(
        "setup_003: ended {} at step {}, health {:?}, scenes {:?}, at {}",
        fought.ended,
        run.steps,
        fought.health,
        fought.scenes,
        run.hero().pivot
    );
    assert!(fought.ended, "Setup_003's controller off: {:?}", fought.log);
    for n in SETUP_003_FOES {
        assert!(
            first_in(&fought.log, n, &["removed"]).is_some(),
            "{n} beaten"
        );
    }
    assert!(stops.is_empty(), "{stops:#?}");
    println!("load at end: {}", load());
}

/// openspec AS.402: Setup_003 fought by a hero walked to it
/// ([`west_to_setup_003`]: through the fourth ice block onto its first
/// trigger), the stick and the attack at its foes in 600-step rounds until
/// its controller is off, his health set back to where it started at each
/// round (the bench neither dodges a spin nor picks up food: without it
/// Eruptor fell to the three armored spinners together). Each foe beaten;
/// each armored one's armour knocked off by his hits before it fell; the
/// spike gate past them lowered by the controller's end. Four rounds with
/// no change end it.
#[test]
#[ignore = "reads the Giants dump"]
fn level_208s_setup_003_walked_from_the_fourth_ice_block() {
    use std::cell::RefCell;
    println!("load at start: {}", load());
    let mut stops = Vec::new();
    let mut run = west_to_setup_003(&mut stops);
    let full = run.health().expect("his health");
    // Each foe's armour level as its own script holds it: first read, lowest.
    let armour: RefCell<HashMap<&str, (i64, i64)>> = RefCell::new(HashMap::new());
    let done = |run: &Run| {
        for n in SETUP_003_FOES {
            if let Some(a) = armour_of(run, n) {
                let mut m = armour.borrow_mut();
                let e = m.entry(n).or_insert((a, a));
                e.1 = e.1.min(a);
            }
        }
        removed(run, SETUP_003_CONTROLLER)
    };
    let (mut log, mut last, mut scenes) = (Vec::new(), HashMap::new(), Vec::new());
    let (mut lowest, mut ended, mut quiet, mut top_ups) = (full, false, 0, 0u32);
    for _ in 0..40 {
        top_ups += u32::from(top_up(&mut run, full));
        let f = fight_to(
            &mut run,
            &mut stops,
            &SETUP_003_FOES,
            &[SETUP_003_CONTROLLER, GATE_3],
            Vec3::new(-151.8, 8.2, -33.9),
            600,
            &done,
        );
        lowest = lowest.min(f.health.1);
        scenes.extend(f.scenes);
        let mut changed = false;
        for (step, n, b) in f.log {
            if last.get(&n) != Some(&b) {
                last.insert(n.clone(), b.clone());
                log.push((step, n, b));
                changed = true;
            }
        }
        if f.ended {
            ended = true;
            break;
        }
        assert!(
            run.health().is_some_and(|h| h > 0.0),
            "he fell at {}",
            run.steps
        );
        quiet = if changed { 0 } else { quiet + 1 };
        assert!(quiet < 4, "Setup_003 stalled: {log:?}");
    }
    for (step, n, b) in &log {
        println!("  {step} {n} {b}");
    }
    let armour = armour.into_inner();
    println!(
        "setup_003 walked: ended {ended} at step {}, health {full} lowest {lowest}, {top_ups} top-ups, scenes {scenes:?}, armour {armour:?}",
        run.steps
    );
    // The top-up stands in for the dodging and the food the bench doesn't
    // do: how many it took is held, so a harder fight shows.
    assert!(top_ups <= TOP_UPS_003, "{top_ups} top-ups");
    assert!(ended, "Setup_003's controller off: {log:?}");
    for n in SETUP_003_FOES {
        assert!(first_in(&log, n, &["removed"]).is_some(), "{n} beaten");
    }
    // They hurt him as their scripts say (spins, grenades): his health fell.
    assert!(lowest < full, "never hurt: {lowest}");
    // The armored spinners' reveal, and each armored one stripped by his
    // hits before it fell.
    assert!(
        scenes
            .iter()
            .any(|(_, s)| s == "IGC_Armored_Spinner_Reveal"),
        "{scenes:?}"
    );
    let armored: Vec<_> = armour.iter().filter(|(_, (a, _))| *a > 0).collect();
    assert!(!armored.is_empty(), "no armored foe: {armour:?}");
    assert!(
        armored.iter().all(|(_, (_, low))| *low == 0),
        "armour left on: {armour:?}"
    );
    // The gate past them lowered by the controller's end.
    let mut gate = String::new();
    for _ in 0..600 {
        still(&mut run, &mut stops, [false; 4]);
        gate = behaviour(&run, GATE_3);
        if gate.starts_with("spikes in down position") {
            break;
        }
    }
    println!("the gate past Setup_003: {gate}");
    assert!(gate.starts_with("spikes in down position"), "{gate}");
    assert!(stops.is_empty(), "{stops:#?}");
    println!("load at end: {}", load());
}

/// openspec AS.403, AS.798 and AS.1010: Setup_004 on its own triggers,
/// set down on its first (past Setup_003's gate), by Eruptor, whose lob
/// reaches. The controller wakes into its set-up scene, circling four Ice
/// Chompies round its anchor; the stick down the slope past the `Blocker`,
/// which turns its box off as he comes in front of it, to the second
/// trigger, where the scene goes on; then the stick and the attack at the
/// fifteen of its foes on the yard's floor, in 600-step rounds with his
/// health set back each round, until each is beaten. Its two bomb throwers
/// stand on ledges above the yard no walk reaches: each is beaten with its
/// own bombs ([`beat_the_bombers`]), the controller's set comes off, and
/// the jump pads' activator pans to pads (4) and (5) and turns them on.
/// Four rounds with no change end it.
#[test]
#[ignore = "reads the Giants dump"]
fn level_208s_setup_004_by_its_own_triggers() {
    setup_004_to_the_pads(None, TOP_UPS_004);
}

/// openspec AS.1010: Setup_004 as [`level_208s_setup_004_by_its_own_triggers`]
/// with Ninjini, whose swords reach nothing on the ledges: the bombers
/// beaten with their own bombs all the same.
#[test]
#[ignore = "reads the Giants dump"]
fn level_208s_setup_004_with_a_melee_hero() {
    setup_004_to_the_pads(Some(NINJINI), TOP_UPS_004_MELEE);
}

/// Ninjini, the magic Giant: swords and a bottle she throws at her feet.
const NINJINI: Character = Character {
    archive: "character/109_djini.bld",
    load_file: "109_Djini",
    toy: 109,
};

/// The run joined at its start as `character` (Eruptor if none).
fn joined_208_as(character: Option<Character>, stops: &mut Vec<String>) -> Run {
    let Some(character) = character else {
        return joined_208(stops);
    };
    let (mut run, _) = run_on_level(
        Mode::Play,
        LEVEL_208,
        Setup {
            character: Some(character),
            ..setup()
        },
    );
    for _ in 0..2 {
        stops.extend(run.step_play().unwrap().iter().map(ToString::to_string));
    }
    run.join().unwrap_or_else(|h| panic!("{h}"));
    run
}

/// Setup_004 fought as a kid fights it to the jump pads ([`level_208s_setup_004_by_its_own_triggers`]).
fn setup_004_to_the_pads(character: Option<Character>, most_top_ups: u32) {
    println!("load at start: {}", load());
    let mut stops = Vec::new();
    let mut run = joined_208_as(character, &mut stops);
    for k in 0..6000u32 {
        still(&mut run, &mut stops, [false; 4]);
        if k > 200 && scene_of(&run).is_none() && !guacamole::anchors::scene_holds(&run.vm) {
            break;
        }
    }
    assert_eq!(behaviour(&run, SETUP_004_CONTROLLER), "asleep");
    // The foes are the level's own group, which the controller's end waits
    // on whole (AS.1020), each one walked.
    let mut group = group_of(&mut run, "Setup_004_Enemies");
    let mut foes = SETUP_004_FOES.map(str::to_owned).to_vec();
    group.sort();
    foes.sort();
    assert_eq!(group, foes, "Setup_004_Enemies");
    for n in SETUP_004_FOES {
        let p = run.files.placed.iter().find(|p| p.name == n).expect(n);
        let script = p.script.as_deref().expect(n);
        assert!(run.vm.config().whitelist.lists(n, script), "{n} listed");
    }
    let first = run.at("Setup_004_Trigger(1)");
    run.put(first, 0.0);
    for _ in 0..120 {
        still(&mut run, &mut stops, [false; 4]);
    }
    assert_eq!(
        behaviour(&run, SETUP_004_CONTROLLER),
        "circling chompies - set up scene"
    );
    // Down the slope: the blocker's box solid until he's in front of it.
    let blocker = run.placed("Blocker").0;
    let solid = |run: &Run| {
        run.world
            .actor(blocker)
            .unwrap()
            .solid
            .interacts_with_actors
            != 0
    };
    assert!(solid(&run), "the blocker stands before he comes");
    let second = run.at("Setup_004_Trigger(2)");
    let opened = std::cell::Cell::new(None);
    let down = fight_to(
        &mut run,
        &mut stops,
        &[],
        &[],
        second,
        4000,
        &|run: &Run| {
            if opened.get().is_none() && !solid(run) {
                opened.set(Some(run.steps));
            }
            flat(run.hero().pivot, second) < 2.0
        },
    );
    println!(
        "down to the second trigger: {} at step {}, the blocker off at {:?}",
        down.ended,
        run.steps,
        opened.get()
    );
    assert!(down.ended, "held on the way down at {}", run.hero().pivot);
    assert!(opened.get().is_some(), "the blocker never opened");
    let full = run.health().expect("his health");
    let walked: Vec<&str> = SETUP_004_FOES
        .iter()
        .copied()
        .filter(|n| !SETUP_004_LEDGE.contains(n))
        .collect();
    let (mut log, mut last) = (Vec::new(), HashMap::new());
    let (mut lowest, mut quiet, mut top_ups) = (full, 0, 0u32);
    let beaten = |run: &Run| walked.iter().all(|n| removed(run, n));
    for _ in 0..30 {
        top_ups += u32::from(top_up(&mut run, full));
        let f = fight_to(
            &mut run,
            &mut stops,
            &walked,
            &[SETUP_004_CONTROLLER],
            second,
            600,
            &beaten,
        );
        lowest = lowest.min(f.health.1);
        let mut changed = false;
        for (step, n, b) in f.log {
            if last.get(&n) != Some(&b) {
                last.insert(n.clone(), b.clone());
                log.push((step, n, b));
                changed = true;
            }
        }
        if f.ended {
            break;
        }
        assert!(
            run.health().is_some_and(|h| h > 0.0),
            "he fell at {}",
            run.steps
        );
        quiet = if changed { 0 } else { quiet + 1 };
        assert!(quiet < 4, "Setup_004 stalled: {log:?}");
    }
    for (step, n, b) in &log {
        println!("  {step} {n} {b}");
    }
    println!(
        "setup_004's floor: step {}, health {full} lowest {lowest}, {top_ups} top-ups",
        run.steps
    );
    assert!(top_ups <= most_top_ups, "{top_ups} top-ups");
    for n in &walked {
        assert!(first_in(&log, n, &["removed"]).is_some(), "{n} beaten");
    }
    assert!(lowest < full, "never hurt: {lowest}");
    // The ledges: each bomber beaten by a bomb of its own thrown back.
    let bombers = beat_the_bombers(&mut run, &mut stops, full);
    println!("the bombers: {bombers:?}");
    for (i, n) in SETUP_004_LEDGE.iter().enumerate() {
        assert!(removed(&run, n), "{n} beaten");
        assert!(bombers.hurt[i] > 0, "{n} hurt by no bomb");
    }
    // Its set gone, the controller comes off, and the activator shows the
    // pads with its pan, turns them on and goes.
    let mut scenes = Vec::new();
    for _ in 0..900 {
        if let Some(cs) = cutscene_name(&run)
            && scenes.last() != Some(&cs)
        {
            scenes.push(cs);
        }
        still(&mut run, &mut stops, [false; 4]);
        if removed(&run, PADS_ACTIVATOR) && scenes.len() > 0 {
            break;
        }
    }
    println!(
        "after the bombers: step {}, controller {}, scenes {scenes:?}, pads {} {}",
        run.steps,
        behaviour(&run, SETUP_004_CONTROLLER),
        behaviour(&run, PAD_4),
        behaviour(&run, PAD_5)
    );
    assert!(
        removed(&run, SETUP_004_CONTROLLER),
        "the controller's set off"
    );
    assert!(removed(&run, PADS_ACTIVATOR), "the activator done");
    assert!(!scenes.is_empty(), "the pan to the pads");
    for pad in [PAD_4, PAD_5] {
        assert!(!run.vm.asleep(run.placement(pad)), "{pad} on");
    }
    assert!(stops.is_empty(), "{stops:#?}");
    println!("load at end: {}", load());
}

/// What beating the ledge bombers took ([`beat_the_bombers`]).
#[derive(Debug, Default)]
struct Bombers {
    /// Bombs he picked up, and threw.
    taken: u32,
    thrown: u32,
    /// Hurts each bomber took, in [`SETUP_004_LEDGE`]'s order.
    hurt: [u32; 2],
    /// Steps it took.
    steps: u32,
}

/// The ledge bombers beaten as a kid beats them (openspec AS.1010): a
/// bomber's grenade lands as a bomb that waits to be grabbed
/// (`Bomb_Rocket_Template.ai`'s); he walks into the nearest one on his
/// floor, it follows him, he turns toward the nearest bomber standing and
/// taps △ every 20 steps until it's thrown, and the bomb flies to the
/// bombers, which the jump pads' activator made its targets
/// (`Bomb_Targets`). His health set back every 600 steps. Fails on no hurt
/// and no bomb taken in 1,800 steps.
fn beat_the_bombers(run: &mut Run, stops: &mut Vec<String>, full: f32) -> Bombers {
    let mut out = Bombers::default();
    let start = run.steps;
    let stand = Vec3::new(-124.0, -11.7, 70.0);
    let (mut carried, mut held_last, mut hp, mut since) = (0u32, false, [None; 2], 0u32);
    loop {
        if (run.steps - start).is_multiple_of(600) {
            set_health(run, full);
        }
        if SETUP_004_LEDGE.iter().all(|n| removed(run, n)) {
            out.steps = run.steps - start;
            return out;
        }
        let now = SETUP_004_LEDGE.map(|n| foe_hp(run, n));
        since += 1;
        for i in 0..2 {
            if hp[i].is_some() && now[i] != hp[i] {
                out.hurt[i] += 1;
                since = 0;
            }
            hp[i] = now[i];
        }
        assert!(
            since < 1800,
            "the bombers held {since} steps at {}: {out:?}",
            run.hero().pivot
        );
        if cutscene_name(run).is_some() || guacamole::anchors::scene_holds(&run.vm) {
            still(run, stops, [false; 4]);
            continue;
        }
        let me = run.hero().pivot;
        let bombs = chucker_bombs(run);
        let held = bombs.iter().any(|(_, b)| b == "following player");
        if held && !held_last {
            out.taken += 1;
            since = 0;
            carried = 0;
        }
        held_last = held;
        if held {
            carried += 1;
            let at = SETUP_004_LEDGE
                .iter()
                .filter(|n| !removed(run, n))
                .map(|n| run.at(n))
                .min_by(|a, b| flat(*a, me).total_cmp(&flat(*b, me)))
                .unwrap_or(stand);
            run.bench.view = (at.x - me.x).atan2(at.z - me.z).to_degrees();
            run.bench.hold_view = true;
            let tap = carried > 20 && carried.is_multiple_of(20);
            out.thrown += u32::from(tap);
            let push = if carried < 12 { 0.5 } else { 0.0 };
            step(
                run,
                stops,
                Stick::new(0.0, push),
                [false, false, false, tap],
            );
            continue;
        }
        let goal = bombs
            .iter()
            .filter(|(p, b)| {
                b.starts_with("wait") && (p.y - me.y).abs() < 2.5 && flat(*p, me) < 20.0
            })
            .map(|(p, _)| *p)
            .min_by(|a, b| flat(*a, me).total_cmp(&flat(*b, me)))
            .unwrap_or(stand);
        run.bench.view = (goal.x - me.x).atan2(goal.z - me.z).to_degrees();
        run.bench.hold_view = true;
        let push = if flat(goal, me) > 0.6 { 1.0 } else { 0.0 };
        step(run, stops, Stick::new(0.0, push), [false; 4]);
    }
}

/// The bombers' bombs standing: each one's place and behaviour.
fn chucker_bombs(run: &Run) -> Vec<(Vec3, String)> {
    run.world
        .clones()
        .into_iter()
        .filter(|h| run.world.name(*h) == "Bomb_Chucker_Spawned_Bomb")
        .filter_map(|h| {
            let a = run.world.actor(h).filter(|a| !a.removed)?;
            let b = run
                .vm
                .placement_of(h)
                .and_then(|i| run.vm.behaviour(i))
                .unwrap_or("-")
                .to_owned();
            Some((a.position, b))
        })
        .collect()
}

/// A creature's health as its scripts hold it, while it's run.
fn foe_hp(run: &Run, name: &str) -> Option<f32> {
    let id = run.vm.placement_of(run.placed(name).0)?;
    if run.vm.removed(id) {
        return None;
    }
    let vm = &run.vm;
    let d = vm.local(id, "description").map(|v| vm.value(v))?;
    let c = vm.field(d, "creature info").map(|v| vm.value(v))?;
    match vm.field(c, "hp").map(|v| vm.value(v))? {
        guac_script::Value::Float(f) => Some(f),
        guac_script::Value::Int(i) => Some(i as f32),
        _ => None,
    }
}

/// The jump pads up from Setup_004's yard and their activator (openspec
/// AS.1010, AS.1014).
const PADS_ACTIVATOR: &str = "_JumpPad_Activator(1)";
const PAD_4: &str = "PvP_JumpPad_NORMAL(4)";
const PAD_5: &str = "PvP_JumpPad_NORMAL(5)";

/// openspec AS.1021: the snowman by Setup_004's second trigger bursts as a
/// kid runs into it: set down 5 m west of it, the stick held at it until
/// it's taken off, and its parts (carrot, pipe, arms, hat) off after it.
#[test]
#[ignore = "reads the Giants dump"]
fn level_208s_snowman_bursts_as_he_runs_into_it() {
    println!("load at start: {}", load());
    let mut stops = Vec::new();
    let mut run = joined_208(&mut stops);
    for k in 0..6000u32 {
        still(&mut run, &mut stops, [false; 4]);
        if k > 200 && scene_of(&run).is_none() && !guacamole::anchors::scene_holds(&run.vm) {
            break;
        }
    }
    let man = run.at(SNOWMAN);
    assert_ne!(behaviour(&run, SNOWMAN), "-", "listed");
    run.put(Vec3::new(man.x - 5.0, man.y + 1.0, man.z), 90.0);
    let parts = |run: &Run| {
        run.world
            .clones()
            .into_iter()
            .filter(|h| {
                let n = run.world.name(*h);
                (n.starts_with("Snowman_") || n == "TopHat")
                    && run.world.actor(*h).is_some_and(|a| !a.removed)
            })
            .count()
    };
    let (mut burst, mut most) = (None, 0);
    for _ in 0..1200 {
        let me = run.hero().pivot;
        run.bench.view = (man.x - me.x).atan2(man.z - me.z).to_degrees();
        run.bench.hold_view = true;
        step(&mut run, &mut stops, Stick::new(0.0, 1.0), [false; 4]);
        most = most.max(parts(&run));
        if removed(&run, SNOWMAN) {
            burst = Some(run.steps);
            break;
        }
    }
    run.bench.hold_view = false;
    println!("the snowman burst at step {burst:?}, its parts {most}");
    assert!(
        burst.is_some(),
        "the snowman stood: {}",
        behaviour(&run, SNOWMAN)
    );
    let mut left = parts(&run);
    for _ in 0..1200 {
        still(&mut run, &mut stops, [false; 4]);
        most = most.max(parts(&run));
        left = parts(&run);
        if left == 0 && most > 0 {
            break;
        }
    }
    println!(
        "its parts: {most} at most, {left} left at step {}",
        run.steps
    );
    assert!(most > 0, "no parts flew");
    assert_eq!(left, 0, "parts left");
    assert!(stops.is_empty(), "{stops:#?}");
    println!("load at end: {}", load());
}

/// The snowman beside Setup_004's second trigger (openspec AS.1021).
const SNOWMAN: &str = "Snowman(7)";

/// A creature's armour level as its own script holds it (`armorlevel`, the
/// Cyclopes' local), while it's run.
fn armour_of(run: &Run, name: &str) -> Option<i64> {
    let id = run.vm.placement_of(run.placed(name).0)?;
    if run.vm.removed(id) {
        return None;
    }
    match run.vm.local(id, "armorlevel")? {
        guac_script::Value::Int(a) => Some(a.into()),
        _ => None,
    }
}

/// The names in a level set (a file variable holding placements), as the
/// scripts read it.
fn group_of(run: &mut Run, name: &str) -> Vec<String> {
    let g = run.vm.program().global_named(name).expect(name);
    let v = run.vm.global(g).expect(name);
    let v = run.vm.value(v);
    let members = run.vm.set(v).expect(name).to_vec();
    members
        .into_iter()
        .filter_map(|m| match m {
            guac_script::Value::Entity(h) => Some(run.world.name(h)),
            _ => None,
        })
        .collect()
}

/// His health set back to `full` at a fight round's start (the walked fights'
/// stand-in for a kid's dodging and food, which the bench doesn't do):
/// whether he'd lost any.
fn top_up(run: &mut Run, full: f32) -> bool {
    let lost = run.health().is_some_and(|h| h < full - 0.5);
    set_health(run, full);
    lost
}

/// The most top-ups the walked Setup_003 and Setup_004 fights take
/// (openspec AS.402, AS.403): as-208-reflect-review measured 5 and 4
/// headless with Eruptor, 2026-11-26.
const TOP_UPS_003: u32 = 8;
const TOP_UPS_004: u32 = 7;
/// Ninjini's (openspec AS.1010): as-208-battle measured 2 headless,
/// 2026-11-26.
const TOP_UPS_004_MELEE: u32 = 5;

/// His health set as his scripts hold it.
fn set_health(run: &mut Run, hp: f32) {
    let m = run.minion.expect("joined");
    let vm = &mut run.vm;
    let d = vm.local(m, "description").map(|v| vm.value(v)).unwrap();
    let c = vm.field(d, "creature info").map(|v| vm.value(v)).unwrap();
    assert!(vm.set_field(c, "hp", guac_script::Value::Float(hp)));
}

/// openspec AS.757: over the river's north edge and back. The level mesh's
/// wall there has a 4 m gap (x -30.5, z -31 to -27): the stick north from
/// the river takes him over it, 20 m down, and the level's jump pad at the
/// drop's foot (`PvP_JumpPad_NORMAL(3)`) carries him back up onto the
/// river. Unlisted, he stood on it and stayed down. A stall (no move in 300
/// steps off the pad) ends it.
#[test]
#[ignore = "reads the Giants dump"]
fn level_208s_river_edge_drops_him_and_its_jump_pad_brings_him_back() {
    println!("load at start: {}", load());
    let mut stops = Vec::new();
    let mut run = joined_208(&mut stops);
    for k in 0..6000u32 {
        still(&mut run, &mut stops, [false; 4]);
        if k > 200 && scene_of(&run).is_none() && !guacamole::anchors::scene_holds(&run.vm) {
            break;
        }
    }
    let flat = |a: Vec3, b: Vec3| Vec3::new(a.x - b.x, 0.0, a.z - b.z).length();
    run.put(Vec3::new(-31.0, 4.7, -31.0), 0.0);
    let over = Vec3::new(-31.0, 0.0, -20.0);
    let mut fell = None;
    for _ in 0..600 {
        let me = run.hero().pivot;
        run.bench.view = (over.x - me.x).atan2(over.z - me.z).to_degrees();
        run.bench.hold_view = true;
        step(&mut run, &mut stops, Stick::new(0.0, 1.0), [false; 4]);
        if run.hero().pivot.y < -10.0 && run.hero().footing.supported {
            fell = Some(run.steps);
            break;
        }
    }
    println!("over the edge at step {fell:?}, at {}", run.hero().pivot);
    assert!(
        fell.is_some(),
        "over the gap and down: {}",
        run.hero().pivot
    );
    let pad = run.at(UNDER_RIVER_PAD);
    let (mut back, mut last, mut since) = (None, run.hero().pivot, 0u32);
    for _ in 0..1800 {
        let me = run.hero().pivot;
        if me.y > 3.0 && run.hero().footing.supported {
            back = Some((run.steps, me));
            break;
        }
        let to = if me.y < -10.0 {
            pad
        } else {
            Vec3::new(-28.0, 0.0, -31.0)
        };
        run.bench.view = (to.x - me.x).atan2(to.z - me.z).to_degrees();
        run.bench.hold_view = true;
        let push = if flat(me, pad) < 0.5 && me.y < -10.0 {
            0.0
        } else {
            1.0
        };
        step(&mut run, &mut stops, Stick::new(0.0, push), [false; 4]);
        if run.hero().pivot.distance(last) < 0.01 {
            since += 1;
            assert!(
                since < 300,
                "held at {} by the pad ({}): {}",
                run.hero().pivot,
                behaviour(&run, UNDER_RIVER_PAD),
                run.steps
            );
        } else {
            (last, since) = (run.hero().pivot, 0);
        }
    }
    println!(
        "back up at {back:?}, the pad {}",
        behaviour(&run, UNDER_RIVER_PAD)
    );
    assert!(back.is_some(), "back up on the river: {}", run.hero().pivot);
    assert!(stops.is_empty(), "{stops:#?}");
    println!("load at end: {}", load());
}

/// The jump pad under the river's north edge (openspec AS.757).
const UNDER_RIVER_PAD: &str = "PvP_JumpPad_NORMAL(3)";

/// Setup_003's controller and the foes its script names (openspec AS.402).
const SETUP_003_CONTROLLER: &str = "Setup_003_Controller";
const SETUP_003_FOES: [&str; 9] = [
    "Enemy_BombThrower(3)",
    "Enemy_Cyclops_Near(4)",
    "Enemy_Cyclops_Near(5)",
    "Enemy_Cyclops_Near(16)",
    "Enemy_Cyclops_Near(17)",
    "Enemy_Cyclops_Near(18)",
    "Enemy_Cyclops_Near(19)",
    "Enemy_Cyclops_Near(25)",
    "Enemy_Cyclops_Near(26)",
];

/// The third beam set's push block and ice block (openspec AS.776).
const PUSH_BLOCK_9: &str = "Push_Block_Template(9)";
const ICE_BLOCK_3: &str = "Melting_Ice_blocks_reflect_3";

/// The fourth beam set's ice block, pivot and post (openspec AS.776,
/// AS.796).
const ICE_BLOCK_4: &str = "Melting_Ice_blocks_reflect_4";
const PIVOT_3: &str = "Reflect_Pivot_Round(3)";
const POST: &str = "Reflect_Post(1)";

/// Setup_004's controller and its foes, the level's `Setup_004_Enemies`
/// (openspec AS.403, AS.1020): the fifteen its script names and Ice
/// Chompies (8) and (9); its two bomb throwers walk onto ledges above the
/// yard (AS.1010).
const SETUP_004_CONTROLLER: &str = "Setup_004_Controller";
const SETUP_004_FOES: [&str; 17] = [
    "Enemy_Chompy_Ice(1)",
    "Enemy_Chompy_Ice(11)",
    "Enemy_Chompy_Ice(12)",
    "Enemy_Chompy_Ice(13)",
    "Enemy_Chompy_Ice(14)",
    "Enemy_Chompy_Ice(15)",
    "Enemy_Chompy_Ice(16)",
    "Enemy_Biclops_Heavy_S2(3)",
    "Enemy_Biclops_Heavy_S2(4)",
    "Enemy_BombThrower(1)",
    "Enemy_BombThrower(2)",
    "Enemy_Cyclops_Near(1)",
    "Enemy_Cyclops_Near(2)",
    "Enemy_Cyclops_Near(3)",
    "Enemy_Cyclops_Near(6)",
    "Enemy_Chompy_Ice(8)",
    "Enemy_Chompy_Ice(9)",
];
const SETUP_004_LEDGE: [&str; 2] = ["Enemy_BombThrower(1)", "Enemy_BombThrower(2)"];

/// The spike gate past Setup_003 (openspec AS.402).
const GATE_3: &str = "Spike_Gate_Metal_Template(3)";

/// Setup_002's spike gate by the spinners (openspec AS.401).
const GATE: &str = "Spike_Gate_Metal_Template(1)";

/// The brick wall across the river past Setup_002's gate (openspec AS.346).
const RIVER_WALL: &str = "Wall_Spawner_Template(9)";

/// The second beam puzzle's push block, pivot and ice block (openspec
/// AS.701's set 2).
const PUSH_BLOCK: &str = "Push_Block_Template(1)";
const PIVOT_2: &str = "Reflect_Pivot_Round(2)";
const ICE_BLOCK_2: &str = "Melting_Ice_blocks_reflect_2";

/// Where the push block's track ends: its last waypoint (-49.38, -61.20)
/// as its script snaps the track to its facing.
const PUSH_END: Vec3 = Vec3::new(-49.46, 3.34, -61.17);

/// What the second beam puzzle came to ([`second_beam`]).
#[derive(Debug, Default)]
struct SecondBeam {
    /// Where the push block stood after the pushes.
    block: Option<Vec3>,
    /// Pushes that moved it.
    moves: u32,
    /// Talk presses at the pivot's switch.
    presses: u32,
    /// The pivot's beam's end once it lit the crystal.
    lit: Option<Vec3>,
    /// The crystal's own beam, from and to.
    relay: Option<(Vec3, Vec3)>,
}

/// The second beam puzzle as a kid works it: to the push block's east side,
/// the stick held west until it stops moving (its track's end), then to the
/// pivot's switch and the talk button every 2 s until a beam starts at the
/// block's crystal, at most eight presses.
fn second_beam(run: &mut Run, stops: &mut Vec<String>) -> SecondBeam {
    let flat = |a: Vec3, b: Vec3| Vec3::new(a.x - b.x, 0.0, a.z - b.z).length();
    let mut out = SecondBeam::default();
    let block = run.at(PUSH_BLOCK);
    let behind = Vec3::new(block.x + 3.0, block.y, block.z);
    let to = fight_to(run, stops, &[], &[], behind, 2000, &|run: &Run| {
        flat(run.hero().pivot, behind) < 1.0
    });
    assert!(to.ended, "behind the push block: {}", run.hero().pivot);
    let (mut last, mut still_for) = (behaviour(run, PUSH_BLOCK), 0u32);
    for _ in 0..900 {
        let me = run.hero().pivot;
        let aim = Vec3::new(me.x - 10.0, me.y, run.at(PUSH_BLOCK).z);
        run.bench.view = (aim.x - me.x).atan2(aim.z - me.z).to_degrees();
        run.bench.hold_view = true;
        step(run, stops, Stick::new(0.0, 1.0), [false; 4]);
        let b = behaviour(run, PUSH_BLOCK);
        if b != last {
            out.moves += u32::from(b == "2. moving");
            still_for = 0;
            last = b;
        } else {
            still_for += 1;
        }
        if out.moves > 0 && still_for > 240 {
            break;
        }
    }
    out.block = Some(run.at(PUSH_BLOCK));
    let pivot = run.at(PIVOT_2);
    let switch = run
        .world
        .clones()
        .into_iter()
        .filter(|h| run.world.name(*h).starts_with("Reflect_Switchbase"))
        .filter_map(|h| {
            run.world
                .actor(h)
                .filter(|a| !a.removed)
                .map(|a| a.position)
        })
        .find(|p| flat(*p, pivot) < 10.0)
        .expect("the second pivot's switch");
    let to = fight_to(run, stops, &[], &[], switch, 3000, &|run: &Run| {
        flat(run.hero().pivot, switch) < 2.5
    });
    assert!(to.ended, "at the second switch: {}", run.hero().pivot);
    let block = out.block.unwrap();
    while out.presses < 8 {
        out.presses += 1;
        for k in 0..120 {
            still(run, stops, [false, false, false, k == 0]);
        }
        let beams = beams(run);
        out.lit = beams
            .iter()
            .map(|(_, end, _)| *end)
            .find(|end| flat(*end, block) < 1.0);
        out.relay = beams
            .iter()
            .find(|(from, _, _)| flat(*from, block) < 3.0)
            .map(|(from, end, _)| (*from, *end));
        if out.lit.is_some() && out.relay.is_some() {
            break;
        }
    }
    out
}

/// The first beam puzzle's pivot (openspec AS.431).
const PIVOT: &str = "Reflect_Pivot_Round(1)";

/// What turning the beam came to ([`turn_the_beam`]).
#[derive(Debug, Default)]
struct Beam {
    /// Watched placements' behaviours as they changed.
    log: Vec<(u32, String, String)>,
    /// Each cutscene and its first step.
    scenes: Vec<(u32, String)>,
    /// Talk presses at the switch.
    presses: u32,
    /// Where the switch stood, once made.
    switch: Option<Vec3>,
    /// The step the block was first powered (melting).
    powered: Option<u32>,
    /// The step the block was off.
    melted: Option<u32>,
    /// Each reflect beam's clone, its end and its width (metres) as its
    /// scripts set them at the step the block was first powered (openspec
    /// AS.700).
    beams: Vec<(Vec3, Vec3, Option<f32>)>,
}

/// Every reflect beam in play whose scripts set it an end: where its clone
/// stands, where its end does and its width in metres (openspec AS.700).
fn beams(run: &Run) -> Vec<(Vec3, Vec3, Option<f32>)> {
    let mut held = guacamole::script_engine::HeroesOnly(run.bench.heroes.clone());
    run.world
        .clones()
        .into_iter()
        .filter(|h| run.world.emitter_of(*h) == Some("Reflect_Beam"))
        .filter_map(|h| {
            let a = run.world.actor(h).filter(|a| !a.removed)?;
            let (end, width) = run.world.beam_end(h, &mut held)?;
            Some((a.position, Vec3::from_array(end), width))
        })
        .collect()
}

/// To the pivot's switch (a clone at its first waypoint) across the
/// flood's cells, then the talk button there every 2 s as a kid turns it,
/// until the beam reaches the ice block; then still while it melts, until
/// it's off or `cap` steps. Cutscenes wait out with the stick let go.
fn turn_the_beam(run: &mut Run, stops: &mut Vec<String>, cap: u32) -> Beam {
    let flat = |a: Vec3, b: Vec3| Vec3::new(a.x - b.x, 0.0, a.z - b.z).length();
    let watch = ["Reflect_Start(1)", PIVOT, ICE_BLOCK, "IGC_FirstIceBlock"];
    let mut beam = Beam::default();
    let mut last: HashMap<&str, String> = HashMap::new();
    let (mut route, mut routed, start, mut at_switch) = (Vec::new(), 0u32, run.steps, None);
    while run.steps < start + cap {
        for n in watch {
            let b = behaviour(run, n);
            if last.get(n) != Some(&b) {
                beam.log.push((run.steps, n.to_owned(), b.clone()));
                last.insert(n, b);
            }
        }
        if gone_actor(run, ICE_BLOCK) {
            beam.melted = Some(run.steps);
            break;
        }
        if beam.powered.is_none() && behaviour(run, ICE_BLOCK) == "melt" {
            beam.powered = Some(run.steps);
            beam.beams = beams(run);
        }
        if let Some(cs) = cutscene_name(run) {
            if beam.scenes.last().is_none_or(|(_, n)| *n != cs) {
                beam.scenes.push((run.steps, cs));
            }
            still(run, stops, [false; 4]);
            continue;
        }
        if guacamole::anchors::scene_holds(&run.vm) || beam.powered.is_some() {
            still(run, stops, [false; 4]);
            continue;
        }
        let switch = run
            .world
            .clones()
            .into_iter()
            .filter(|h| run.world.name(*h).starts_with("Reflect_Switchbase"))
            .find_map(|h| run.world.actor(h).filter(|a| !a.removed))
            .map(|a| a.position);
        beam.switch = beam.switch.or(switch);
        let goal = switch.unwrap_or_else(|| run.at(PIVOT));
        let me = run.hero().pivot;
        if switch.is_some() && flat(me, goal) < 3.0 {
            let since = *at_switch.get_or_insert(run.steps);
            let press = (run.steps - since).is_multiple_of(120);
            beam.presses += u32::from(press);
            still(run, stops, [false, false, false, press]);
            continue;
        }
        if routed + 90 <= run.steps || route.is_empty() {
            route = way_to(run, me, goal);
            routed = run.steps;
        }
        let aim = route
            .iter()
            .find(|c| flat(me, **c) > 1.5)
            .copied()
            .unwrap_or(goal);
        run.bench.view = (aim.x - me.x).atan2(aim.z - me.z).to_degrees();
        run.bench.hold_view = true;
        step(run, stops, Stick::new(0.0, 1.0), [false; 4]);
    }
    beam
}
