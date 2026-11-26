//! Level 203 (openspec AS.703), headless with guacamole's host on the TV's
//! listing for the level: Eruptor joined at its start, its intro played to
//! its end or skipped as a player skips it, then the stick his. Ignored by
//! default; they read `GUAC_GIANTS_DUMP` (process env, else `.env.dev`):
//!
//! `cargo test -p guacamole --profile disc --test giants_level_203 -- --ignored --nocapture`
//!
//! No recording of 203 exists: every step is the level's own scripts.

use std::collections::HashMap;

use guac_motion::seam::Vec3;
use guac_motion::stick::Stick;
use guac_script::Mode;
use guacamole::script_engine::Pad;

mod common;

use common::*;

const LEVEL_203: &str = "level_203_the_machine";

/// Where the intro walks the first player in to: `203_Intro_P1_WP`'s last
/// waypoint, by the start area's Molekin.
const HAND_OVER: Vec3 = Vec3::new(-165.180, 17.136, -225.177);

/// The TV's listing with the props, sounds and interface, and the rigs posed
/// as the TV poses them (the landing pod's riders hang on its rig).
fn setup() -> Setup {
    Setup {
        interface: true,
        props: true,
        sounds: true,
        posed_rigs: true,
        static_nodes: true,
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
fn joined_203(stops: &mut Vec<String>) -> Run {
    let (mut run, _) = run_on_level(Mode::Play, LEVEL_203, setup());
    for _ in 0..2 {
        stops.extend(run.step_play().unwrap().iter().map(ToString::to_string));
    }
    run.join().unwrap_or_else(|h| panic!("{h}"));
    run
}

/// The intro's own placement taken off by its end, as it ends either way.
fn intro_off(run: &Run) -> bool {
    run.vm.removed(run.placement("203_Intro_IGC"))
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
    for _ in 0..200 {
        step(run, stops, Stick::new(0.0, 1.0), [false; 4]);
    }
    let moved = run.hero().pivot.distance(at);
    println!("walked {moved:.2} m from {at}");
    assert!(moved > 3.0, "{moved}");
}

/// openspec AS.703: 203's intro played to its end: him hung on the landing
/// pod as it flies in, walked in along his path and handed over at its end
/// by the start area's Molekin, the intro off, then walking on. Nothing
/// stops.
#[test]
#[ignore = "reads the Giants dump"]
fn level_203s_intro_plays_to_its_end_and_hands_him_over() {
    println!("load at start: {}", load());
    let mut stops = Vec::new();
    let mut run = joined_203(&mut stops);
    let (mut scenes, mut cast, mut hung) = (Vec::new(), Vec::new(), 0);
    let mut ended = None;
    let mut played = false;
    for k in 0..6000u32 {
        still(&mut run, &mut stops, [false; 4]);
        hung += u32::from(run.hero().hung);
        played |= cutscene_name(&run).is_some();
        if let Some((i, n)) = scene_of(&run)
            && scenes.last() != Some(&(i, n))
        {
            scenes.push((i, n));
        }
        for a in scene_actors(&run).into_iter().flatten() {
            if !cast.contains(&a) {
                cast.push(a);
            }
        }
        if k > 200
            && played
            && scene_of(&run).is_none()
            && !guacamole::anchors::scene_holds(&run.vm)
        {
            ended = Some(run.steps);
            break;
        }
    }
    let at = run.hero().pivot;
    println!("scenes {scenes:?}, cast {cast:?}, hung {hung} steps, over at {ended:?}: {at}");
    assert!(played, "its intro ran");
    assert!(ended.is_some(), "it ends");
    assert_eq!(
        scenes,
        (0..4).map(|i| (i, 4)).collect::<Vec<_>>(),
        "its four scenes"
    );
    for who in ["203_Intro_Lander", "Flynn_NPC"] {
        assert!(cast.iter().any(|a| a == who), "{who} in its scenes");
    }
    assert!(hung > 100, "{hung}");
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

/// openspec AS.703: 203's start read off its intro's walk-in, the intro
/// held for the hero and skipped by △: him set down at the path's end, the
/// HUD back, walking on the stick. Nothing stops.
#[test]
#[ignore = "reads the Giants dump"]
fn level_203s_intro_skipped_hands_him_over_and_he_walks() {
    println!("load at start: {}", load());
    let mut stops = Vec::new();
    let mut run = joined_203(&mut stops);
    let start = run.hero().pivot;
    println!("his start {start}");
    assert!(
        start.distance(HAND_OVER) < 0.1,
        "where the TV starts him: {start}"
    );
    let mut played = false;
    for k in 0..3000u32 {
        still(
            &mut run,
            &mut stops,
            [false, false, false, k > 120 && k % 20 == 10],
        );
        played |= cutscene_name(&run).is_some();
        if k > 200 && played && !guacamole::anchors::scene_holds(&run.vm) {
            break;
        }
    }
    assert!(played, "its intro ran");
    let at = run.hero().pivot;
    println!("set down at {at}");
    assert!(at.distance(HAND_OVER) < 0.5, "set down at its place: {at}");
    assert!(intro_off(&run), "the intro off");
    walks_on(&mut run, &mut stops);
    assert!(stops.is_empty(), "{stops:#?}");
    println!("load at end: {}", load());
}

/// Skipped by △ once it plays, then the stick his: the run past its intro.
fn past_intro(stops: &mut Vec<String>) -> Run {
    let mut run = joined_203(stops);
    for k in 0..3000u32 {
        still(
            &mut run,
            stops,
            [false, false, false, k > 120 && k % 20 == 10],
        );
        if k > 200 && !guacamole::anchors::scene_holds(&run.vm) {
            break;
        }
    }
    assert!(intro_off(&run), "the intro off");
    run
}

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

/// A placement's place now.
fn at(run: &Run, name: &str) -> Vec3 {
    let h = run.placed(name).0;
    run.world.actor(h).map(|a| a.position).expect(name)
}

/// The way along the start area's conveyors to its gate: the first
/// conveyor's foot, the second's, the gate.
const ALONG: [(f32, f32); 3] = [(-143.6, -221.6), (-115.5, -252.9), (-98.0, -222.0)];

/// The start area's gate, Setup_001's fight's.
const GATE: &str = "enemy_gate_start_area";

/// The start area's two crushers.
const CRUSHERS: [&str; 2] = ["203_Crusher_begining_a", "203_Crusher_begining_b"];

/// The stick toward each of `goals` in turn, held at each until within
/// 1.5 m across the ground; cutscenes wait with it let go. Each step
/// `each` sees the run. Fails on a stall: 3 s with him moved under 0.3 m.
fn walk(run: &mut Run, stops: &mut Vec<String>, goals: &[(f32, f32)], each: &mut dyn FnMut(&Run)) {
    for &(gx, gz) in goals {
        let (mut last, mut still_for) = (run.hero().pivot, 0);
        for k in 0..2400u32 {
            each(run);
            let me = run.hero().pivot;
            if Vec3::new(me.x - gx, 0.0, me.z - gz).length() < 1.5 {
                break;
            }
            let held = cutscene_name(run).is_some();
            run.bench.view = (gx - me.x).atan2(gz - me.z).to_degrees();
            run.bench.hold_view = true;
            step(
                run,
                stops,
                Stick::new(0.0, if held { 0.0 } else { 1.0 }),
                [false; 4],
            );
            if k % 180 == 179 {
                let m = run.hero().pivot;
                still_for = if m.distance(last) < 0.3 && !held {
                    still_for + 1
                } else {
                    0
                };
                last = m;
                assert!(still_for < 1, "held at {m} toward {gx},{gz}");
            }
        }
        let me = run.hero().pivot;
        assert!(
            Vec3::new(me.x - gx, 0.0, me.z - gz).length() < 1.5,
            "short of {gx},{gz}: {me}"
        );
        println!("at {gx},{gz} step {}", run.steps);
    }
}

/// openspec AS.704: from where the intro hands him over, the stick along
/// the start area's two conveyors down to its gate: the Molekin at his
/// talk, the two crushers dropping and lifting on their trigger's timer,
/// each conveyor running while he's on it. Nothing stops.
#[test]
#[ignore = "reads the Giants dump"]
fn level_203s_start_area_runs_and_he_walks_to_its_gate() {
    println!("load at start: {}", load());
    let mut stops = Vec::new();
    let mut run = past_intro(&mut stops);
    let mut seen: HashMap<String, Vec<String>> = HashMap::new();
    let mut heights: HashMap<&str, (f32, f32)> = HashMap::new();
    let watched = [
        "molekin_at_start",
        "_TRIGGER_crushers_begining",
        "new_conveyor",
        "new_conveyor(1)",
        CRUSHERS[0],
        CRUSHERS[1],
        GATE,
    ];
    let mut ran = std::collections::HashSet::new();
    let mut watch = |run: &Run| {
        for n in guacamole::level_203::START {
            if !matches!(behaviour(run, n).as_str(), "-" | "asleep" | "removed") {
                ran.insert(n);
            }
        }
        for n in watched {
            let b = behaviour(run, n);
            let e = seen.entry(n.to_owned()).or_default();
            if e.last() != Some(&b) {
                e.push(b);
            }
        }
        for c in CRUSHERS {
            let y = at(run, c).y;
            let e = heights.entry(c).or_insert((y, y));
            *e = (e.0.min(y), e.1.max(y));
        }
    };
    walk(&mut run, &mut stops, &ALONG[..2], &mut watch);
    // Down both conveyors unhurt; the yard past the second's foot is in
    // the riveter Troll's sight (he wakes by his own range and, seeing him
    // there, shoots: `lifted-reference` on 203, as-203-walkway).
    let conveyors = run.health();
    walk(&mut run, &mut stops, &ALONG[2..], &mut watch);
    println!("behaviours {seen:#?} crushers' heights {heights:?}");
    assert!(
        seen["molekin_at_start"]
            .iter()
            .any(|b| b == "norticus end talker")
    );
    for c in ["new_conveyor", "new_conveyor(1)"] {
        assert!(seen[c].iter().any(|b| b == "escalate"), "{c} ran under him");
    }
    for c in CRUSHERS {
        let (lo, hi) = heights[c];
        assert!(hi - lo > 4.0, "{c} dropped and lifted: {lo}..{hi}");
        assert!(seen[c].iter().any(|b| b == "drop"), "{c}");
        assert!(seen[c].iter().any(|b| b == "lift"), "{c}");
    }
    assert_eq!(seen[GATE], ["wait to be opened"], "the gate shut");
    // Every listing before the gate runs its script on the way (the
    // as-203-start review: 17 of the level file's 36 names' removal went
    // unnoticed, seven of them doing nothing).
    println!("ran {ran:?}");
    for n in guacamole::level_203::START {
        if PAST_THE_GATE.contains(&n) {
            continue;
        }
        assert!(ran.contains(n), "{n} listed and idle");
    }
    assert_eq!(conveyors, Some(290.0), "unhurt down the conveyors");
    println!("health at the gate {:?}", run.health());
    assert!(stops.is_empty(), "{stops:#?}");
    println!("load at end: {}", load());
}

/// Setup_001's trigger, at the second conveyor's foot.
const TRIGGER: (f32, f32) = (-106.4, -253.2);

/// Setup_001's fight: the Troll, the three Fire Chompies in the yard and
/// the five its Chompy pipe sends.
const FOES: [&str; 9] = [
    "Enemy_Troll_BFG(1)",
    "Enemy_Chompy_Fire(1)",
    "Enemy_Chompy_Fire(2)",
    "Enemy_Chompy_Fire(3)",
    "Enemy_Chompy_Fire(4)",
    "Enemy_Chompy_Fire(5)",
    "Enemy_Chompy_Fire(66)",
    "Enemy_Chompy_Fire(67)",
    "Enemy_Chompy_Fire(68)",
];

/// What else the fight touches: its controller, its pipe and the gate.
const FIGHT_WATCH: [&str; 3] = ["Setup_001_Controller", "GasJet_Pipe(13)", GATE];

/// What a fight did ([`fight_to`]).
#[derive(Debug, Default)]
struct Fought {
    /// Each watched placement's behaviour as it changed: step, name, state.
    log: Vec<(u32, String, String)>,
    /// The hero's health at the fight's start and his lowest in it.
    health: (f32, f32),
    /// Whether `done` held before the cap.
    ended: bool,
    /// Where he stood every quarter second.
    trail: Vec<Vec3>,
    /// Each clone template's count of clones seen, by name.
    clones: HashMap<String, usize>,
}

/// A fight as a kid fights it (giants_level_208's): the nearest of `foes`
/// awake within 40 m and 12 m of his height chased over the flood's cells
/// and hit, stepping back out after 4 s pressed into one; with none awake
/// near, on to `goal`. His health topped up below a third (a kid's food;
/// the fight's foes outlast a level-1 hero who only swings). Over once
/// `done` holds or after `cap` steps.
#[allow(clippy::too_many_arguments)]
fn fight_to(
    run: &mut Run,
    stops: &mut Vec<String>,
    foes: &[&str],
    watched: &[&str],
    goal: Vec3,
    cap: u32,
    done: &dyn Fn(&Run) -> bool,
) -> Fought {
    let mut fought = Fought::default();
    let hp = run.health().unwrap_or(0.0);
    fought.health = (hp, hp);
    let mut last: HashMap<String, String> = HashMap::new();
    let mut seen_clones = std::collections::HashSet::new();
    let (mut pressed, start) = (0u32, run.steps);
    let (mut route, mut routed, mut next): (Vec<Vec3>, u32, usize) = (Vec::new(), 0, 0);
    let flat = |a: Vec3, b: Vec3| Vec3::new(a.x - b.x, 0.0, a.z - b.z).length();
    loop {
        for n in foes.iter().chain(watched) {
            let b = behaviour(run, n);
            if last.get(*n) != Some(&b) {
                fought.log.push((run.steps, (*n).to_owned(), b.clone()));
                last.insert((*n).to_owned(), b);
            }
        }
        if let Some(hp) = run.health() {
            fought.health.1 = fought.health.1.min(hp);
        }
        for h in run.world.clones() {
            let n = run.world.name(h).to_owned();
            if seen_clones.insert(h) {
                *fought.clones.entry(n).or_default() += 1;
            }
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
        if cutscene_name(run).is_some() || guacamole::anchors::scene_holds(&run.vm) {
            still(run, stops, [false; 4]);
            continue;
        }
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
            route = way_to(run, me, target).0;
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

/// The step `name` was first in one of `states`, from `log`.
fn first_in(log: &[(u32, String, String)], name: &str, states: &[&str]) -> Option<u32> {
    log.iter()
        .find(|(_, n, b)| n == name && states.contains(&b.as_str()))
        .map(|e| e.0)
}

/// Past the intro, down the conveyors to Setup_001's trigger and the fight
/// fought to the gate's opening: what it did.
fn fought_open(stops: &mut Vec<String>) -> (Run, Fought) {
    let mut run = past_intro(stops);
    walk(&mut run, stops, &[ALONG[0], ALONG[1], TRIGGER], &mut |_| {});
    let gate = |run: &Run| behaviour(run, GATE) != "wait to be opened";
    let out = fight_to(
        &mut run,
        stops,
        &FOES,
        &FIGHT_WATCH,
        Vec3::new(-100.0, 7.7, -238.0),
        12000,
        &gate,
    );
    (run, out)
}

/// openspec AS.705: Setup_001's fight by its own trigger. Down the start
/// area's conveyors to the trigger at the second's foot, the Troll comes
/// down his path and shoots and the three Fire Chompies in the yard wake;
/// with them beaten the pipe sends five more one at a time; with all
/// beaten the controller takes itself off and the gate lowers. Nothing
/// stops.
#[test]
#[ignore = "reads the Giants dump"]
fn level_203s_first_fight_opens_the_start_areas_gate() {
    println!("load at start: {}", load());
    let mut stops = Vec::new();
    let (run, out) = fought_open(&mut stops);
    for e in &out.log {
        println!("  {e:?}");
    }
    println!("clones {:?}", out.clones);
    println!(
        "ended {} at step {}, health {:?}, at {}",
        out.ended,
        run.steps,
        out.health,
        run.hero().pivot
    );
    assert!(out.ended, "the gate opened");
    assert_eq!(behaviour(&run, GATE), "lower spikes");
    for f in FOES {
        assert!(first_in(&out.log, f, &["removed"]).is_some(), "{f} beaten");
    }
    assert!(first_in(&out.log, "Setup_001_Controller", &["removed"]).is_some());
    assert!(
        out.clones.get("Troll_BFG_Bullet").copied().unwrap_or(0) > 0,
        "the Troll shoots"
    );

    assert!(stops.is_empty(), "{stops:#?}");
    println!("load at end: {}", load());
}

/// What [`guacamole::level_203::START`] lists past the gate.
const PAST_THE_GATE: [&str; 7] = [
    "new_conveyor(2)",
    "new_conveyor(3)",
    "reset_point_ON_2",
    "reset_point_OFF_2",
    "TeleporterStart_introduction",
    "TeleporterEnd_introduction",
    "TeleporterHelper",
];

/// Past the gate: the two conveyors east, then south onto the introduction
/// teleporter's start.
const PAST_GATE: [(f32, f32); 3] = [(-87.0, -219.6), (-73.6, -226.8), (-73.7, -249.0)];

/// Where the introduction teleporter sets him down: its end's place.
const TELEPORTER_END: Vec3 = Vec3::new(-10.811, 2.346, -245.256);

/// openspec AS.706: past the opened gate, along the next two conveyors
/// onto the introduction teleporter's start, which carries him to its end
/// on the lower walkway. Nothing stops.
#[test]
#[ignore = "reads the Giants dump"]
fn level_203s_introduction_teleporter_takes_him_on() {
    println!("load at start: {}", load());
    let mut stops = Vec::new();
    let (mut run, out) = fought_open(&mut stops);
    assert!(out.ended, "the gate opened");
    let mut carried: HashMap<&str, bool> = HashMap::new();
    walk(&mut run, &mut stops, &PAST_GATE, &mut |run| {
        for c in &PAST_THE_GATE[..2] {
            *carried.entry(c).or_default() |= behaviour(run, c) == "escalate";
        }
        for r in &PAST_THE_GATE[2..4] {
            *carried.entry(r).or_default() |=
                !matches!(behaviour(run, r).as_str(), "-" | "asleep" | "removed");
        }
    });
    for c in &PAST_THE_GATE[..4] {
        assert!(carried[c], "{c} ran as he passed");
    }
    let start = run.steps;
    let mut arrived = None;
    // 300 steps: Setup_002's first Fire Chompy reaches him after that
    // (AS.765) and its blast moves him.
    for _ in 0..300u32 {
        still(&mut run, &mut stops, [false; 4]);
        if arrived.is_none() && run.hero().pivot.distance(TELEPORTER_END) < 1.0 {
            arrived = Some(run.steps - start);
        }
    }
    let at = run.hero().pivot;
    println!("arrived after {arrived:?} steps, at {at}");
    assert!(arrived.is_some(), "carried to its end");
    assert!(at.distance(TELEPORTER_END) < 1.0, "stays there: {at}");
    assert!(run.hero().footing.supported, "on the ground");
    assert!(stops.is_empty(), "{stops:#?}");
    println!("load at end: {}", load());
}

/// The name of the template `h` was cloned from, `""` for none.
fn template_of(run: &Run, h: guac_script::Handle) -> String {
    run.world
        .actor(h)
        .and_then(|a| a.template)
        .and_then(|t| run.vm.program().name_of(t).map(str::to_owned))
        .unwrap_or_default()
}

/// His health topped up to 270 below 90 (a kid's food: a level-1 hero
/// standing in a fight outlasts none of it); true when it did.
fn top_up(run: &mut Run) -> bool {
    match run.minion {
        Some(m) if run.health().is_some_and(|h| h > 0.0 && h < 90.0) => {
            guacamole::scripts::set_health(&mut run.vm, m, 270)
        }
        _ => false,
    }
}

/// In the yard in the riveter Troll's line, past Setup_001's trigger.
const IN_THE_YARD: (f32, f32) = (-101.0, -234.0);

/// openspec AS.766, AS.787: Setup_001's fight with him standing in the
/// yard in the riveter Troll's line. The Chompy pipe by the gate launches
/// the controller's five, two flame clones at its node each time; each of
/// the Troll's bullets runs its own STARTUP (its glow clone) and flies on
/// the scripts' projectile manager, which hurts him with one that meets
/// him and takes every bullet off once it's done.
#[test]
#[ignore = "reads the Giants dump"]
fn level_203s_first_fights_pipe_flames_and_the_trolls_shots_land() {
    println!("load at start: {}", load());
    let mut stops = Vec::new();
    let mut run = past_intro(&mut stops);
    walk(
        &mut run,
        &mut stops,
        &[ALONG[0], ALONG[1], TRIGGER, IN_THE_YARD],
        &mut |_| {},
    );
    let pipe = run.placement("GasJet_Pipe(13)");
    let mut seen = std::collections::HashSet::new();
    let mut bullets: Vec<(guac_script::Handle, u32)> = Vec::new();
    let (mut glows, mut flames, mut launches, mut hits, mut topups) = (0, 0, 0, 0, 0);
    let mut was_launching = false;
    let mut hp = run.health();
    let mut flying: Vec<guac_script::Handle> = Vec::new();
    for _ in 0..2400u32 {
        still(&mut run, &mut stops, [false; 4]);
        for h in run.world.clones() {
            if !seen.insert(h) {
                continue;
            }
            let t = template_of(&run, h);
            match t.as_str() {
                "Troll_BFG_Bullet" => {
                    bullets.push((h, run.steps));
                    flying.push(h);
                }
                "Troll_BFG_BulletGlow" => glows += 1,
                t if t.starts_with("203_Chompy_Pipe_Fire") => flames += 1,
                _ => {}
            }
        }
        let launching = run.vm.local(pipe, "state") == Some(guac_script::Value::Int(1));
        launches += u32::from(launching && !was_launching);
        was_launching = launching;
        // A bullet taken off this step within reach of him as his health
        // falls is a shot that met him.
        let now = run.health();
        let fell = now.zip(hp).is_some_and(|(n, h)| n < h);
        let me = run.hero().pivot;
        flying.retain(|h| {
            let a = run.world.actor(*h);
            let gone = a.is_none_or(|a| a.removed);
            if gone && fell && a.is_some_and(|a| a.position.distance(me) < 2.5) {
                hits += 1;
            }
            !gone
        });
        hp = now;
        if top_up(&mut run) {
            topups += 1;
            hp = run.health();
        }
        if launches >= 5 && hits >= 1 && run.vm.removed(run.placement("Setup_001_Controller")) {
            break;
        }
    }
    // Every bullet out 2 s or more is off: none left in the world.
    for _ in 0..150u32 {
        still(&mut run, &mut stops, [false; 4]);
    }
    let left = bullets
        .iter()
        .filter(|(h, _)| run.world.actor(*h).is_some_and(|a| !a.removed))
        .count();
    println!(
        "launches {launches}, flames {flames}, bullets {}, glows {glows}, shots that met him {hits}, left {left}, top-ups {topups}, step {}",
        bullets.len(),
        run.steps
    );
    assert_eq!(launches, 5, "the pipe's five");
    assert!(flames >= 2 * launches, "two flames a launch: {flames}");
    assert!(!bullets.is_empty(), "the Troll shoots");
    assert_eq!(glows, bullets.len(), "each bullet's own STARTUP");
    assert!(hits >= 1, "a shot hurts him");
    assert_eq!(left, 0, "every bullet taken off");
    assert!(stops.is_empty(), "{stops:#?}");
    println!("load at end: {}", load());
}

/// Setup_002's fight (`level_203::WAY_2`): the nine Fire Chompies.
const CHOMPIES_2: [&str; 9] = [
    "Enemy_Chompy_Fire(70)",
    "Enemy_Chompy_Fire(71)",
    "Enemy_Chompy_Fire(72)",
    "Enemy_Chompy_Fire(18)",
    "Enemy_Chompy_Fire(19)",
    "Enemy_Chompy_Fire(20)",
    "Enemy_Chompy_Fire(21)",
    "Enemy_Chompy_Fire(22)",
    "Enemy_Chompy_Fire(23)",
];

/// Setup_002's lobber, on the ledge past the lower gate's side ramp.
const LOBBER: &str = "Enemy_PandoranLobber(1)";

/// The lower walkway's gate.
const GATE_2: &str = "enemy_gate_lower_conveyor_01";

/// Down the side ramp west of the gate into the trench below the lobber's
/// ledge, where its sight clears the ledge's lip.
const DOWN_THE_RAMP: [(f32, f32); 3] = [(-15.0, -256.0), (-15.0, -266.0), (-15.0, -272.0)];

/// The lobber's ledge: where a stand-in sets him (the way onto it before the
/// gate opens is unread, AS.990).
const ON_THE_LEDGE: Vec3 = Vec3::new(-12.0, -1.9, -289.0);

/// Back from the trench up the ramp and through the opened gate onto the
/// conveyor under the lower crushers.
const THROUGH_THE_GATE: [(f32, f32); 5] = [
    (-15.0, -280.0),
    (-15.0, -262.0),
    (-12.0, -256.0),
    (-4.3, -255.5),
    (-4.3, -261.5),
];

/// openspec AS.765: the lower walkway past the introduction teleporter. As
/// he lands the start's sets are taken off, the lower crushers are back on
/// their timer and the Molekin by the first crusher talks; Setup_002's
/// controller sends its nine Fire Chompies out of its pipe (two flames a
/// launch) and they're fought down; its lobber, woken, bombs him once he's
/// down the side ramp in its sight (bombs thrown at him); its gate waits on
/// it. Set on the
/// lobber's ledge (the stand-in, AS.990) he beats it, the controller ends
/// and the gate lowers its spikes, and he walks back up and through it.
/// Nothing stops.
#[test]
#[ignore = "reads the Giants dump"]
fn level_203s_lower_walkways_fight_opens_its_gate() {
    println!("load at start: {}", load());
    let mut stops = Vec::new();
    let (mut run, out) = fought_open(&mut stops);
    assert!(out.ended, "the first gate opened");
    walk(&mut run, &mut stops, &PAST_GATE, &mut |_| {});
    for _ in 0..60u32 {
        still(&mut run, &mut stops, [false; 4]);
    }
    assert!(run.hero().pivot.distance(TELEPORTER_END) < 1.5, "landed");
    // The start's clean-up: its conveyors and crushers off.
    for n in ["new_conveyor", "203_Crusher_begining_a", "molekin_at_start"] {
        assert_eq!(behaviour(&run, n), "removed", "{n} taken off");
    }
    let mut seen = std::collections::HashSet::new();
    let mut flames = 0;
    let watched = ["Setup_002_Controller", "GasJet_Pipe(14)", GATE_2, LOBBER];
    let chompies_gone = |run: &Run| CHOMPIES_2.iter().all(|n| behaviour(run, n) == "removed");
    let fight = fight_to(
        &mut run,
        &mut stops,
        &CHOMPIES_2,
        &watched,
        TELEPORTER_END,
        6000,
        &|run| chompies_gone(run),
    );
    for e in &fight.log {
        println!("  {e:?}");
    }
    for h in run.world.clones() {
        if seen.insert(h) && template_of(&run, h).starts_with("203_Chompy_Pipe_Fire") {
            flames += 1;
        }
    }
    println!(
        "Chompies beaten {} at step {}, health {:?}, pipe flames {flames}",
        fight.ended, run.steps, fight.health
    );
    assert!(fight.ended, "the nine beaten");
    assert!(flames >= 18, "two flames a launch: {flames}");
    assert!(
        first_in(&fight.log, LOBBER, &["idle"]).is_some(),
        "the lobber woken"
    );
    assert_eq!(
        behaviour(&run, GATE_2),
        "wait to be opened",
        "it waits on the lobber"
    );
    // Down the side ramp into the lobber's sight: it bombs him.
    let seen_lobber: std::cell::RefCell<Vec<String>> = Default::default();
    let (talked, dropped) = (std::cell::Cell::new(false), std::cell::Cell::new(false));
    let mut each = |run: &Run| {
        if behaviour(run, "molekin_first_crusher") == "norticus end talker" {
            talked.set(true);
        }
        if behaviour(run, "203_Crusher_lower_a") == "drop" {
            dropped.set(true);
        }
        let b = behaviour(run, LOBBER);
        let mut s = seen_lobber.borrow_mut();
        if s.last() != Some(&b) {
            s.push(b);
        }
    };
    walk(&mut run, &mut stops, &DOWN_THE_RAMP, &mut each);
    let before = run.health();
    for _ in 0..600u32 {
        still(&mut run, &mut stops, [false; 4]);
        each(&run);
        top_up(&mut run);
    }
    println!(
        "the lobber {:?}, him {before:?} -> {:?}",
        seen_lobber.borrow(),
        run.health()
    );
    assert!(
        seen_lobber.borrow().iter().any(|b| b == "throwbomb"),
        "it throws its bombs at him"
    );
    assert_eq!(behaviour(&run, GATE_2), "wait to be opened");
    // The stand-in (AS.990): on its ledge, he beats it and the gate opens.
    run.put(ON_THE_LEDGE, 180.0);
    let opened = |run: &Run| behaviour(run, GATE_2) != "wait to be opened";
    let fight = fight_to(
        &mut run,
        &mut stops,
        &[LOBBER],
        &watched,
        ON_THE_LEDGE,
        6000,
        &opened,
    );
    for e in &fight.log {
        println!("  {e:?}");
    }
    assert!(fight.ended, "the gate opened");
    assert!(
        first_in(&fight.log, LOBBER, &["removed"]).is_some(),
        "the lobber beaten"
    );
    assert!(
        first_in(&fight.log, "Setup_002_Controller", &["removed"]).is_some(),
        "the controller off"
    );
    assert_eq!(behaviour(&run, GATE_2), "lower spikes");
    walk(&mut run, &mut stops, &THROUGH_THE_GATE, &mut each);
    let (molekin, crushed) = (talked.get(), dropped.get());
    println!(
        "through at {} step {}, molekin talked {molekin}, crusher dropped {crushed}",
        run.hero().pivot,
        run.steps
    );
    assert!(run.hero().pivot.z < -260.0, "past the gate");
    assert!(molekin, "the Molekin by the first crusher talks");
    assert!(crushed, "the lower crushers on their timer");
    assert!(stops.is_empty(), "{stops:#?}");
    println!("load at end: {}", load());
}
