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

/// The wall's bricks still standing (clones of `brick` and `brick2`): how
/// many, and the farthest of each template's from the wall's place.
fn bricks(run: &Run, wall: Vec3) -> (usize, [f32; 2]) {
    let mut n = 0;
    let mut far = [0.0f32; 2];
    for h in run.world.clones() {
        let name = run.world.name(h);
        let Some(k) = ["brick", "brick2"].iter().position(|t| *t == name) else {
            continue;
        };
        if let Some(a) = run.world.actor(h).filter(|a| !a.removed) {
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
/// reaches the block; then the stick takes him to the trigger. Nothing
/// stops.
#[test]
#[ignore = "reads the Giants dump"]
fn level_208s_way_down_to_setup_002s_trigger_by_the_stick() {
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
    let melt = turn_the_beam(&mut run, &mut stops, 6000);
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
    assert!(stops.is_empty(), "{stops:#?}");
    println!("load at end: {}", load());
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
    run.world
        .clones()
        .into_iter()
        .filter(|h| run.world.emitter_of(*h) == Some("Reflect_Beam"))
        .filter_map(|h| {
            let a = run.world.actor(h).filter(|a| !a.removed)?;
            let (end, width) = run.world.beam_end(h)?;
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
