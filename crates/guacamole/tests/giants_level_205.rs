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
    note_card(run);
    note_beds(run);
}

thread_local! {
    /// The music and ambience plays (`mu_*`, `am_*`) this test's run has
    /// started, each as it was first seen: the step, its name and the sound
    /// clock it started on ([`note_beds`]).
    static BEDS: std::cell::RefCell<Vec<(u32, String, u64)>> =
        const { std::cell::RefCell::new(Vec::new()) };
}

/// Each music or ambience play started since the last step onto [`BEDS`].
fn note_beds(run: &Run) {
    BEDS.with_borrow_mut(|beds| {
        for p in run.world.plays() {
            let Some(name) = p
                .name
                .as_deref()
                .filter(|n| n.starts_with("mu_") || n.starts_with("am_"))
            else {
                continue;
            };
            if !beds.iter().any(|(_, n, at)| n == name && *at == p.started) {
                beds.push((run.steps, name.to_owned(), p.started));
            }
        }
    });
}

/// An enemy intro's card as it stood: its first and last step, its
/// string's object by name and the scene it stood in.
type Card = (u32, u32, Option<String>, Option<String>);

thread_local! {
    /// The cards this test's run has put up, in order ([`note_card`]).
    static CARDS: std::cell::RefCell<Vec<Card>> = const { std::cell::RefCell::new(Vec::new()) };
}

/// The enemy card up this step, if any, onto [`CARDS`]: a step with the
/// same card up as the last extends it.
fn note_card(run: &Run) {
    let up = guacamole::announcements::enemy_card(&run.vm, &run.world);
    let Some(string) = up.and_then(|c| match c.string {
        guac_script::Value::Object(o) => run.vm.program().name_of(o).map(str::to_owned),
        _ => None,
    }) else {
        return;
    };
    CARDS.with_borrow_mut(|cards| match cards.last_mut() {
        Some(c) if c.2.as_deref() == Some(&string) && c.1 + 1 == run.steps => c.1 = run.steps,
        _ => cards.push((run.steps, run.steps, Some(string), cutscene_name(run))),
    });
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
    let scenes = over_the_crane(&mut run, &mut stops);
    // The yard's own Cyclops' intro may start as he comes in (openspec
    // AS.284): nothing else on the way.
    assert!(
        scenes[1..].iter().all(|(_, n)| n == CYCLOPS_INTRO),
        "no other cutscene on the way: {scenes:?}"
    );
    assert!(stops.is_empty(), "{stops:#?}");
    println!("load at end: {}", load());
}

/// The yard's first enemy intro, the Cyclops Near's, as its controller
/// names the scene.
const CYCLOPS_INTRO: &str = "ICG_Cyclops_Near";

/// AS.283's way from the hand-over to Setup_001's controller, asserted as
/// it goes; each cutscene on the way and its first step.
fn over_the_crane(run: &mut Run, stops: &mut Vec<String>) -> Vec<(u32, String)> {
    for k in 0..400u32 {
        still(run, stops, [false, false, false, k > 120 && k % 20 == 10]);
    }
    for _ in 0..120 {
        still(run, stops, [false; 4]);
    }
    let start = run.hero().pivot;
    assert!(start.distance(HAND_OVER) < 0.5, "handed over: {start}");
    let (_, controller) = run.placed("Setup_001_Controller");
    let (_, yard_first) = way_to(run, start, controller);
    println!("before the turn the yard is {yard_first:.1} m off any walk");
    assert!(yard_first > 10.0, "{yard_first}");
    assert_eq!(facing(run, "Crane(1)"), 90.0, "the boat lies east");

    // Over the boat, past the crane's deck and down the pier to its south
    // end, where the hint (its own place, within its 20 ft range) pans to the
    // wheel: the pier stops short of the yard.
    let mut scenes = Vec::new();
    let (_, hint) = run.placed("Hint_Pan_Show_Wheel");
    let (way, off) = way_to(run, start, hint);
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
    let reached = walk(run, stops, &mut scenes, &way, 3000);
    for _ in 0..240 {
        if cutscene_name(run).is_none() && !guacamole::anchors::scene_holds(&run.vm) {
            break;
        }
        still(run, stops, [false; 4]);
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
    let (way, off) = way_to(run, run.hero().pivot, crank);
    println!("to the wheel {crank}: {} cells, {off:.1} m off", way.len());
    assert!(off < 3.0, "{off}");
    assert!(
        walk(run, stops, &mut scenes, &way, 3000),
        "at the wheel: {}",
        run.hero().pivot
    );
    let mut presses = 0;
    for k in 0..600u32 {
        let press = facing(run, "Crane(1)") == 90.0 && k % 20 == 10;
        presses += u32::from(press);
        still(run, stops, [false, false, false, press]);
        if facing(run, "Crane(1)") == 180.0 {
            break;
        }
    }
    println!(
        "turned to {} after {presses} presses, step {}",
        facing(run, "Crane(1)"),
        run.steps
    );
    assert_eq!(facing(run, "Crane(1)"), 180.0, "the boat lies south");
    assert_eq!(facing(run, "Crane_Head"), 180.0, "its mast with it");
    let hint_off = run
        .world
        .actor(run.placed("Hint_Pan_Show_Wheel").0)
        .unwrap()
        .removed;
    assert!(hint_off, "the hint off once he's by the wheel");
    for _ in 0..60 {
        still(run, stops, [false; 4]);
    }

    // Over the boat to the yard.
    let (way, off) = way_to(run, run.hero().pivot, controller);
    println!(
        "to Setup_001's controller {controller}: {} cells, {off:.1} m off",
        way.len()
    );
    assert!(
        walk(run, stops, &mut scenes, &way, 4000),
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
    scenes
}

/// Setup_001's five, as its set names them (`Setup_001_Enemies`).
const YARD_FOES: [&str; 5] = [
    "Enemy_Cyclops_Near(70)",
    "Enemy_Cyclops_Near(20)",
    "Enemy_Cyclops_Near(21)",
    "Enemy_Elf_Lobber(1)",
    "Enemy_Elf_Lobber(2)",
];

/// A placement's behaviour now: `removed` once taken off, `asleep`, `-`
/// with no script.
fn behaviour(run: &Run, name: &str) -> String {
    let h = run.placed(name).0;
    match run.vm.placement_of(h) {
        None => "-".to_owned(),
        Some(id) if run.vm.removed(id) => "removed".to_owned(),
        Some(id) if run.vm.asleep(id) => "asleep".to_owned(),
        Some(id) => run.vm.behaviour(id).unwrap_or("?").to_ascii_lowercase(),
    }
}

/// What a setup's fight did ([`fight`]).
#[derive(Debug, Default)]
struct Fought {
    /// Each watched placement's behaviour as it changed: step, name, state.
    log: Vec<(u32, String, String)>,
    /// Each cutscene and its first step.
    scenes: Vec<(u32, String)>,
    /// Each top-up's step and the health it found.
    topups: Vec<(u32, f32)>,
    /// The hero's lowest health.
    lowest: Option<f32>,
    /// Whether its controller took itself off.
    ended: bool,
    /// Where he stood every quarter second from the fight's start: what the
    /// Mac's driver follows where its own chase is held.
    trail: Vec<Vec3>,
}

impl Fought {
    /// The step `name` was taken off, if it was.
    fn removed(&self, name: &str) -> Option<u32> {
        self.log
            .iter()
            .find(|(_, n, b)| n == name && b == "removed")
            .map(|e| e.0)
    }
}

/// A setup fought as a kid fights it, until its controller takes itself
/// off or `cap` steps pass: the nearest of `foes` awake within 40 m and
/// 12 m of his height chased over the flood's cells ([`way_to`]) and hit,
/// stepping back out after 4 s pressed into one; with none awake near, on
/// to `goal` (the trigger a controller waits on before it counts its
/// enemies). Cutscenes wait out with the stick let go. A hero under 60
/// health is topped up to 200, counted, as on 201.
fn fight(
    run: &mut Run,
    stops: &mut Vec<String>,
    controller: &str,
    foes: &[&str],
    goal: Vec3,
    cap: u32,
) -> Fought {
    fight_to(run, stops, controller, foes, goal, cap, &|run: &Run| {
        behaviour(run, controller) == "removed"
    })
}

/// [`fight`], over once `done` holds rather than once the controller goes
/// (`controller` still logged).
fn fight_to(
    run: &mut Run,
    stops: &mut Vec<String>,
    controller: &str,
    foes: &[&str],
    goal: Vec3,
    cap: u32,
    done: &dyn Fn(&Run) -> bool,
) -> Fought {
    let mut fought = Fought::default();
    let mut last: HashMap<String, String> = HashMap::new();
    let (mut pressed, start) = (0u32, run.steps);
    let (mut route, mut routed, mut next): (Vec<Vec3>, u32, usize) = (Vec::new(), 0, 0);
    let flat = |a: Vec3, b: Vec3| Vec3::new(a.x - b.x, 0.0, a.z - b.z).length();
    loop {
        for n in foes.iter().chain(&[controller]) {
            let b = behaviour(run, n);
            if last.get(*n) != Some(&b) {
                fought.log.push((run.steps, (*n).to_owned(), b.clone()));
                last.insert((*n).to_owned(), b);
            }
        }
        if let Some(hp) = run.health() {
            fought.lowest = Some(fought.lowest.map_or(hp, |l| l.min(hp)));
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
        if let (Some(hp), Some(m)) = (run.health(), run.minion)
            && hp < 60.0
        {
            guacamole::scripts::set_health(&mut run.vm, m, 200);
            fought.topups.push((run.steps, hp));
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
        // Over the flood's cells to it, the route made again each 1.5 s or
        // as its end falls 3 m off what it chases.
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
            // Pressed into him 4 s: stepped back out half a second.
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

/// openspec AS.284: Setup_001, the first yard past the crane, on its own
/// triggers. Walked over the crane (AS.283's way), the hero comes on its
/// controller's trigger: the yard's gate goes up, the first Cyclops Near
/// steps out of the house by the water and its intro plays with the
/// Cyclops' card, once, for its 3.5 s; then he fights its five until the
/// controller, once he's crossed its far trigger, counts them gone and
/// takes itself off, the gate gone with the first Cyclops. Nothing stops.
#[test]
#[ignore = "reads the Giants dump"]
fn level_205s_first_yard_fights_by_its_own_triggers() {
    println!("load at start: {}", load());
    let mut stops = Vec::new();
    let mut run = joined_205(&mut stops);
    let gate = run.placed("Setup_001_Enemy_Gate_Actor").0;
    let gate_at = |run: &Run| run.world.actor(gate).map(|a| (a.range, a.removed));
    let gate_before = gate_at(&run);
    let mut scenes = over_the_crane(&mut run, &mut stops);
    let gate_in = gate_at(&run);
    let (_, goal) = run.placed("Setup_001_Trigger(2)");
    let fought = fight(
        &mut run,
        &mut stops,
        "Setup_001_Controller",
        &YARD_FOES,
        goal,
        8000,
    );
    scenes.extend(fought.scenes.iter().cloned());
    let cards = CARDS.with_borrow(Clone::clone);
    for (step, n, b) in &fought.log {
        println!("  {step} {n} {b}");
    }
    println!("scenes {scenes:?}");
    println!("cards {cards:?}");
    println!(
        "the gate {gate_before:?} before, {gate_in:?} in the yard, {:?} after; top-ups {:?}, lowest health {:?}",
        gate_at(&run),
        fought.topups,
        fought.lowest
    );
    println!("over at step {}, ended {}", run.steps, fought.ended);
    let trail: Vec<String> = fought
        .trail
        .iter()
        .map(|p| format!("{:.2},{:.2},{:.2}", p.x, p.y, p.z))
        .collect();
    println!("TRAIL {}", trail.join(";"));
    assert!(fought.ended, "the fight never ended: {:?}", fought.log);
    // Its intro played the Cyclops' card in its own scene, once.
    assert!(
        scenes.iter().any(|(_, n)| n == CYCLOPS_INTRO),
        "the Cyclops' intro: {scenes:?}"
    );
    assert_eq!(cards.len(), 1, "{cards:?}");
    let (from, to, string, scene) = &cards[0];
    assert_eq!(string.as_deref(), Some("S2_Enemy_CyclopsNear_Name"));
    assert_eq!(scene.as_deref(), Some(CYCLOPS_INTRO));
    let secs = (to - from) as f32 / 60.0;
    assert!((secs - 3.5).abs() < 0.2, "the card stood {secs} s");
    // Each of the five went, the controller after.
    let controller_off = fought.removed("Setup_001_Controller").unwrap();
    for n in YARD_FOES {
        let off = fought.removed(n);
        assert!(off.is_some_and(|o| o <= controller_off), "{n}: {off:?}");
    }
    // The gate up as he came in, gone at the end.
    assert_eq!(gate_before, Some((0.0, false)), "the gate down before");
    assert_eq!(gate_in, Some((250.0, false)), "the gate up");
    assert!(gate_at(&run).is_some_and(|g| g.1), "the gate gone");
    assert!(stops.is_empty(), "{stops:#?}");
    println!("load at end: {}", load());
}

/// The later setups (openspec AS.284), each its controller, the trigger a
/// test puts the hero at, the behaviour it waits in for the way past it
/// (AS.285's and AS.286's) and the enemies its sets name.
const LATER_SETUPS: [(&str, &str, &str, &[&str]); 4] = [
    (
        "Setup_002_Controller",
        "Setup_002_Trigger(1)",
        // For the key from the card house.
        "wait",
        &[
            "Enemy_Chompy(1)",
            "Enemy_Chompy(2)",
            "Enemy_Chompy(3)",
            "Enemy_Chompy(4)",
            "Enemy_Chompy(5)",
            "Enemy_Chompy(6)",
            "Enemy_Chompy(7)",
            "Enemy_Chompy(8)",
            "Enemy_Chompy(9)",
            "Enemy_Chompy(10)",
            "Enemy_Chompy(11)",
            "Enemy_Chompy(12)",
            "Enemy_Chompy(13)",
            "Enemy_Chompy(14)",
            "Enemy_Chompy(15)",
            "Enemy_Chompy(16)",
            "Enemy_Elf_Heavy_S2(1)",
            "Enemy_Elf_Lobber(3)",
            "Enemy_Elf_Lobber(4)",
        ],
    ),
    (
        "Setup_003_Controller",
        "Setup_003_Trigger(1)",
        "check if key gate is open",
        &[
            "Enemy_Elf_Heavy_S2(2)",
            "Enemy_Elf_Heavy_S2(3)",
            "Enemy_Elf_Heavy_S2(4)",
            "Enemy_Cyclops_Near(1)",
            "Enemy_Cyclops_Near(2)",
            "Enemy_Cyclops_Near(3)",
            "Enemy_Cyclops_Near(4)",
            "Enemy_Cyclops_Near(5)",
            "Enemy_Cyclops_Near(6)",
            "Enemy_Cyclops_Near(7)",
            "Enemy_Cyclops_Near(8)",
            "Enemy_Cyclops_Near(9)",
            "Enemy_Elf_Lobber(5)",
            "Enemy_Elf_Lobber(6)",
            "Enemy_Elf_Lobber(7)",
        ],
    ),
    (
        "Setup_004_Controller",
        "Setup_004_Trigger(1)",
        "check if pushblock is in place",
        &[
            "Enemy_Biclops_Heavy_S2(1)",
            "Enemy_Biclops_Heavy_S2(2)",
            "Enemy_Biclops_Heavy_S2(3)",
            "Enemy_Cyclops_Near(13)",
            "Enemy_Cyclops_Near(14)",
            "Enemy_Cyclops_Near(15)",
            "Enemy_Cyclops_Near(16)",
            "Enemy_Cyclops_Near(17)",
            "Enemy_Cyclops_Near(18)",
            "Enemy_Cyclops_Near(19)",
            "Enemy_Cyclops_Near(22)",
            "Enemy_Elf_Heavy_S2(5)",
            "Enemy_Elf_Lobber(8)",
            "Enemy_Elf_Lobber(9)",
            "Enemy_Elf_Lobber(15)",
            "Enemy_Elf_Lobber(16)",
        ],
    ),
    (
        "Setup_005_Controller",
        "Setup_005_Trigger(1)",
        // For the card pirate's scene by its door.
        "check if pan done equals 1",
        &[
            "Enemy_Biclops_Heavy_S2(6)",
            "Enemy_Biclops_Heavy_S2(7)",
            "Enemy_Cyclops_Near(10)",
            "Enemy_Cyclops_Near(11)",
            "Enemy_Cyclops_Near(12)",
            "Enemy_Cyclops_Near(23)",
            "Enemy_Cyclops_Near(24)",
            "Enemy_Cyclops_Near(25)",
            "Enemy_Cyclops_Near(26)",
            "Enemy_Elf_Heavy_S2(6)",
            "Enemy_Elf_Heavy_S2(7)",
            "Enemy_Elf_Lobber(11)",
            "Enemy_Elf_Lobber(12)",
            "Enemy_Elf_Lobber(13)",
            "Enemy_Elf_Lobber(14)",
            "Enemy_Elf_Lobber(17)",
            "Enemy_Elf_Lobber(18)",
            "Enemy_Chompy(17)",
            "Enemy_Chompy(18)",
            "Enemy_Chompy(19)",
            "Enemy_Chompy(20)",
            "Enemy_Chompy(21)",
            "Enemy_Chompy(22)",
            "Enemy_Chompy(23)",
            "Enemy_Chompy(24)",
            "Enemy_Chompy(25)",
        ],
    ),
];

/// Handed over (the intro skipped by △) and settled.
fn handed_over(run: &mut Run, stops: &mut Vec<String>) {
    for k in 0..400u32 {
        still(run, stops, [false, false, false, k > 120 && k % 20 == 10]);
    }
    for _ in 0..120 {
        still(run, stops, [false; 4]);
    }
}

/// openspec AS.284's later setups, each from a fresh run, the hero put at
/// its trigger (a test-only placement: the ways between the yards are
/// AS.285's) and fighting what wakes for 6,000 steps: each controller
/// wakes by its own trigger and comes to the behaviour it waits in for
/// what the level's way past it does (Setup_002 the card house's key,
/// Setup_003 its key gate, Setup_004 its push block, Setup_005 a card
/// pirate's scene); Setup_004's Biclops' intro puts up its card for its
/// 2.5 s (Setup_005's trigger lies within 160 ft of Setup_004's controller,
/// so its intro plays there too) and its first five fall. Nothing stops.
#[test]
#[ignore = "reads the Giants dump"]
fn level_205s_later_setups_wake_at_their_triggers() {
    println!("load at start: {}", load());
    for (controller, trigger, waits, foes) in LATER_SETUPS {
        CARDS.with_borrow_mut(Vec::clear);
        let mut stops = Vec::new();
        let mut run = joined_205(&mut stops);
        handed_over(&mut run, &mut stops);
        let asleep = behaviour(&run, controller);
        let (_, at) = run.placed(trigger);
        run.put(at, 0.0);
        let fought = fight(&mut run, &mut stops, controller, foes, at, 6000);
        let cards = CARDS.with_borrow(Clone::clone);
        let states: Vec<&str> = fought
            .log
            .iter()
            .filter(|e| e.1 == controller)
            .map(|e| e.2.as_str())
            .collect();
        let gone: Vec<&str> = foes
            .iter()
            .copied()
            .filter(|n| fought.removed(n).is_some())
            .collect();
        println!("== {controller} from {trigger} {at}: {states:?}");
        println!(
            "  gone {gone:?} of {}; scenes {:?}; cards {cards:?}; top-ups {:?}, lowest {:?}; hero at {}",
            foes.len(),
            fought.scenes,
            fought.topups,
            fought.lowest,
            run.hero().pivot
        );
        assert_eq!(asleep, "asleep", "{controller} asleep till he's there");
        assert_eq!(states.last().copied(), Some(waits), "{controller}");
        assert!(!fought.ended);
        if controller == "Setup_004_Controller" || controller == "Setup_005_Controller" {
            assert_eq!(cards.len(), 1, "{cards:?}");
            let (from, to, string, scene) = &cards[0];
            assert_eq!(string.as_deref(), Some("S2_Enemy_Biclops_Name"));
            assert_eq!(scene.as_deref(), Some("IGC_Cyclops_Heavy"));
            let secs = (to - from) as f32 / 60.0;
            assert!((secs - 2.5).abs() < 0.2, "the card stood {secs} s");
        } else {
            assert!(cards.is_empty(), "{cards:?}");
        }
        if controller == "Setup_004_Controller" {
            for n in [
                "Enemy_Biclops_Heavy_S2(1)",
                "Enemy_Biclops_Heavy_S2(2)",
                "Enemy_Biclops_Heavy_S2(3)",
                "Enemy_Elf_Lobber(8)",
                "Enemy_Elf_Lobber(9)",
            ] {
                assert!(gone.contains(&n), "{n}: {gone:?}");
            }
        }
        assert!(stops.is_empty(), "{stops:#?}");
    }
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
    /// fight the hero put there hasn't fought (openspec AS.284); no scene
    /// of its own, though a setup's enemy intro may play as he's put by it
    /// ([`FIGHTS_INTROS`]).
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

/// The setups' enemy intros that play as a hero is put within their reach
/// (openspec AS.284): Setup_004's Biclops' intro, within 160 ft of its
/// controller, as Cali in town stands (86 ft off), where a kid's walk has
/// played it on the way.
const FIGHTS_INTROS: [&str; 1] = ["IGC_Cyclops_Heavy"];

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
                assert!(
                    scenes.iter().all(|s| FIGHTS_INTROS.contains(&s.as_str())),
                    "{name}: {scenes:?}"
                );
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

/// A host profile's id for the save system's session.
const HOST: &str = "01LEVEL205HOST";
const HUB_1: &str = "level_hub_2_stage_1";

fn global(run: &Run, name: &str) -> Option<guac_script::Value> {
    let g = run.vm.program().global_named(name)?;
    run.vm.global_peek(g).map(|v| run.vm.value(v))
}

/// One step with `pad`, then the save system's requests answered as the
/// TV's system answers them for `level`, each write handed back done at
/// once and kept.
fn saved(
    run: &mut Run,
    session: &mut guacamole::story_save::Session,
    level: &str,
    stops: &mut Vec<String>,
    rows: &mut Vec<guac_server::saves::SaveValues>,
    actions: [bool; 4],
) {
    step(run, stops, Stick::new(0.0, 0.0), actions);
    {
        let mut spawn = guacamole::scripts::headless_spawner(1 << 50);
        let mut engine = guacamole::script_engine::NoEngine;
        let mut host = guacamole::scripts::WorldHost {
            world: &mut run.world,
            engine: &mut engine,
            generator: &mut run.generator,
            spawn: &mut spawn,
        };
        guacamole::story_save::serve(&mut run.vm, &mut host, session, level)
            .unwrap_or_else(|h| panic!("{h}"));
    }
    if let Some((_, _, values)) = session.take_write() {
        rows.push(values);
        session.written(&mut run.world.save, Ok(()));
    }
}

/// A stored save past level 201: the first three adventures of the save's
/// list (213, 214 and 201) done, off a fresh run's own definitions.
fn save_past_201() -> (String, guac_server::saves::SaveValues) {
    use guacamole::story_save::{File, Layout};
    let (mut run, _) = run_on_level(Mode::Play, LEVEL_205, setup());
    for _ in 0..2 {
        run.step_play().unwrap();
    }
    let layout = Layout::read(&run.vm).expect("the save's layout");
    let slot = guacamole::story_save::words_of(&run.vm, File::Slot).unwrap();
    let header = guacamole::story_save::words_of(&run.vm, File::Header).unwrap();
    let mut params = layout.decode(File::Slot, &slot);
    params.extend(layout.decode(File::Header, &header));
    for flag in params.get_mut("level flags").unwrap().iter_mut().take(3) {
        *flag |= 1;
    }
    assert_eq!(
        &layout.adventures[..4],
        [
            "Level_213_Tutorial",
            "Level_214_Tutorial_2",
            "Level_201_Rescue_the_Hermit",
            "Level_205_Pirate_Cards"
        ]
    );
    (
        layout.id(),
        guac_server::saves::SaveValues {
            params,
            adventures: layout.adventures.clone(),
        },
    )
}

/// openspec AS.287: level 205's end, from its last fight's end through its
/// outro and completion screen to the hub. The way there isn't run yet, so
/// the run stands in for it: the hero put by Dreadbeard (whose scene's
/// intro plays on his approach), his part of the ship collected first
/// where the level places it (in the boathouse, AS.285's), and
/// `level_205::finale_stand_in` each step making true what Dreadbeard's
/// scene waits on that the TV doesn't run: Setup_005's first waves and its
/// 17 foes beaten (AS.284), and his card game won (AS.315). From there
/// it's the level's own: his last lines, the outro (its scenes, Dreadbeard
/// taken off), the completion screen with 205's objectives and finds, its
/// continue, the level end's movie, the change to the hub's first stage,
/// the words carried with 205 just done; at the hub on them, four
/// adventures done, a first visit, 205's part hung on the ship, the start's
/// save holding 205 done, and Flynn's offer the level after 205. Nothing
/// stops.
#[test]
#[ignore = "reads the Giants dump"]
fn level_205s_end_plays_from_its_last_fight_to_the_hub() {
    use guacamole::level_205::{self, DREADBEARD, FINALE_SCENE};
    use guacamole::level_end::{COMPLETION, Status, seq};
    use guacamole::scripts::MovieEnd;
    use guacamole::ship_parts::{self, Equipped};
    let started = std::time::Instant::now();
    println!("load at start: {}", load());
    let (layout, past_201) = save_past_201();
    let mut session = guacamole::story_save::Session::new(Some(HOST.into()));
    session.stored = Some((layout.clone(), past_201));
    let (mut run, _) = run_on_level(Mode::Play, LEVEL_205, setup());
    run.world.plays_movies = true;
    run.world.sequence.shows_completion = true;
    let (mut stops, mut rows) = (Vec::new(), Vec::new());
    for _ in 0..2 {
        saved(
            &mut run,
            &mut session,
            LEVEL_205,
            &mut stops,
            &mut rows,
            [false; 4],
        );
    }
    assert_eq!(session.report.start.as_deref(), Some("stored"));
    run.join().unwrap_or_else(|h| panic!("{h}"));
    // Its intro skipped as a player skips it, then settled.
    for k in 0..300u32 {
        let skip = k > 20 && k % 20 == 10;
        saved(
            &mut run,
            &mut session,
            LEVEL_205,
            &mut stops,
            &mut rows,
            [false, false, false, skip],
        );
    }
    // The ship part: 205's legendary treasure, in the boathouse.
    let treasures: Vec<(guac_script::Handle, Vec3, String)> = run
        .files
        .placed
        .iter()
        .filter(|p| p.obj.unit == LEVEL && p.activity_state == 4)
        .filter(|p| {
            p.script
                .as_deref()
                .is_some_and(|s| s.ends_with("/Loot_Specials.ai"))
        })
        .map(|p| {
            (
                run.handles[&p.obj],
                Vec3::from_array(p.position),
                p.name.clone(),
            )
        })
        .collect();
    let [(treasure, at, name)] = &treasures[..] else {
        panic!("205's treasures: {treasures:?}");
    };
    println!("205's treasure {name} at {at}");
    assert_eq!(name, level_205::TREASURE);
    run.put(*at + Vec3::new(2.0, 1.0, 0.0), 0.0);
    run.world.wake(*treasure);
    let from = run.steps;
    while !run.world.removed(*treasure) {
        assert!(run.steps < from + 1500, "205's treasure stayed");
        saved(
            &mut run,
            &mut session,
            LEVEL_205,
            &mut stops,
            &mut rows,
            [false; 4],
        );
    }
    let part = ship_parts::report(&run.vm, &run.world);
    println!(
        "collected {} steps on: {:?}",
        run.steps - from,
        part.stand_in
    );
    let [(part_id, Equipped::Collected)] = part.stand_in[..] else {
        panic!("205's part: {:?}", part.stand_in);
    };
    // By Dreadbeard, where his scene's intro plays on his approach.
    let (_, by) = run.placed(level_205::BY_DREADBEARD);
    run.put(by, 180.0);
    let scene_state = |run: &Run| {
        let (h, _) = run.placed(FINALE_SCENE);
        run.vm
            .placement_of(h)
            .and_then(|id| run.vm.local(id, level_205::SCENE_STATE))
            .and_then(|v| run.vm.value(v).as_f32())
    };
    let mut scenes: Vec<(u32, String)> = Vec::new();
    let mut made = level_205::FinaleStep::default();
    let mut asked = Vec::new();
    let mut foes_off_at = None;
    let from = run.steps;
    while run.world.sequence.completion.is_none() {
        assert!(
            run.steps < from + 9000,
            "no completion screen by step {}: the scene at {:?}, {scenes:?}",
            run.steps,
            scene_state(&run)
        );
        saved(
            &mut run,
            &mut session,
            LEVEL_205,
            &mut stops,
            &mut rows,
            [false; 4],
        );
        if let Some(p) = guacamole::prompt::up(&run.vm, &run.world) {
            let id = match p.question {
                guac_script::Value::Object(o) => run.vm.program().name_of(o).map(str::to_owned),
                _ => None,
            };
            assert!(
                run.with_host(|vm, host| guacamole::prompt::answer(vm, host, &p, true))
                    .unwrap_or_else(|h| panic!("the answer: {h}"))
            );
            asked.push(id);
        }
        let step = level_205::finale_stand_in(&mut run.vm, &mut run.world, LEVEL);
        made.fight |= step.fight;
        made.game |= step.game;
        if step.foes > 0 {
            made.foes = step.foes;
            foes_off_at.get_or_insert(run.steps);
        }
        if let Some(cs) = cutscene_name(&run)
            && scenes.last().is_none_or(|(_, n)| *n != cs)
        {
            scenes.push((run.steps, cs));
        }
    }
    let screen_at = run.steps;
    println!(
        "the stand-ins {made:?}, the foes off at {foes_off_at:?}; asked {asked:?}; scenes {scenes:?}; the completion screen at {screen_at}"
    );
    assert!(made.fight && made.game, "{made:?}");
    assert_eq!(made.foes, 17, "Setup_005's foes");
    assert_eq!(asked, [Some("Generic_Skystones_WantToPlay?".to_owned())]);
    let names: Vec<&str> = scenes.iter().map(|(_, n)| n.as_str()).collect();
    for scene in [
        "Dreadbeard Pirate Intro",
        "Dreadbeard Pirate Part 2",
        "Dreadbeard Pirate Part 3",
        "IGC - Intro - CardGame",
        "Dreadbeard Pirate Outro",
        "205 outro igc",
    ] {
        assert!(names.contains(&scene), "{scene}: {names:?}");
    }
    assert!(
        !names.iter().any(|n| n.contains("Quit")),
        "no quit: {names:?}"
    );
    assert!(
        run.world.removed(run.placed(DREADBEARD).0),
        "Dreadbeard off"
    );
    // The outro played to its end, the hero where it leaves him.
    let outro_at = scenes.iter().find(|(_, n)| n == "205 outro igc").unwrap().0;
    println!(
        "the outro {} steps; him at {}",
        screen_at - outro_at,
        run.hero().pivot
    );
    assert!(screen_at - outro_at > 1800, "the outro cut short");
    assert_eq!(seq(&run.vm), Some(COMPLETION));
    let c = run.world.sequence.completion.clone().unwrap();
    let r = c.results.clone().expect("the objectives screen's results");
    println!("{r:#?}");
    assert_eq!(r.level.as_deref(), Some("Cutthroat Carnival"));
    assert_eq!(r.tabs.len(), 3);
    assert!(r.tabs.iter().all(Option::is_some), "{:?}", r.tabs);
    assert!(r.objectives.iter().all(|o| o.description.is_some()));
    let in_tab = |t: i32| r.objectives.iter().filter(|o| o.category == t).count();
    assert_eq!((in_tab(0), in_tab(1), in_tab(2)), (1, 4, 7));
    assert_eq!(
        r.objectives
            .iter()
            .find(|o| o.category == 0)
            .map(|o| o.status),
        Some(Status::NewlyDone),
        "the story goal"
    );
    // The finds, each its count of what 205 places: the legendary treasure
    // taken, its two hat boxes, four chests and one story tablet (with the
    // soul gem, the winged sapphire and the wheel) untouched.
    assert_eq!(
        r.objectives
            .iter()
            .filter(|o| o.category == 2)
            .map(|o| (o.count, o.total))
            .collect::<Vec<_>>(),
        [
            (0.0, 1.0),
            (1.0, 1.0),
            (0.0, 2.0),
            (0.0, 4.0),
            (0.0, 1.0),
            (0.0, 1.0),
            (0.0, 1.0)
        ]
    );
    assert_eq!(r.stars, [true, false, false], "only the level clear star");
    run.world
        .sequence
        .completion
        .as_mut()
        .unwrap()
        .continue_asked = true;
    let continued_at = run.steps;
    while run.world.level_end_movie().is_none() {
        assert!(run.steps < continued_at + 30, "no movie by {}", run.steps);
        saved(
            &mut run,
            &mut session,
            LEVEL_205,
            &mut stops,
            &mut rows,
            [false; 4],
        );
    }
    let movie = run.world.level_end_movie().cloned().unwrap();
    println!("the movie {movie:?}");
    assert_eq!(movie.name, "fmv04");
    run.world.movie_playing();
    for _ in 0..30 {
        saved(
            &mut run,
            &mut session,
            LEVEL_205,
            &mut stops,
            &mut rows,
            [false; 4],
        );
    }
    run.world.movie_over(MovieEnd::Skipped);
    let over_at = run.steps;
    while run.world.level_change().is_none() {
        assert!(run.steps < over_at + 30, "no level change by {}", run.steps);
        saved(
            &mut run,
            &mut session,
            LEVEL_205,
            &mut stops,
            &mut rows,
            [false; 4],
        );
    }
    let change = run.world.level_change().cloned().unwrap();
    println!("the change at {}: {change:?}", run.steps);
    assert_eq!(change.name, HUB_1);
    assert_eq!(
        session.live.as_ref().map(|l| l.just_done.as_slice()),
        Some(&[3][..]),
        "205 just done"
    );
    assert!(stops.is_empty(), "{stops:#?}");
    let steps_205 = run.steps;
    drop(run);

    // The hub on the words carried.
    let hub_setup = Setup {
        streams: true,
        ..setup()
    };
    let (mut hub, _) = run_on_level(Mode::Play, HUB_1, hub_setup);
    let definition = guacamole::story_save::definition(&hub.vm, &hub.world).expect("a definition");
    hub.vm.native_first(definition);
    let id = hub.world.run();
    let mut start_rows = Vec::new();
    {
        let session = &mut session;
        hub.step_play_with(&mut |vm, at, host| {
            if at == definition {
                assert!(
                    guacamole::story_save::title(vm, host, session, id)?,
                    "the save read"
                );
            }
            Ok(())
        })
        .unwrap();
    }
    assert_eq!(session.report.start.as_deref(), Some("carried"));
    assert_eq!(
        global(&hub, "Levels_Completed"),
        Some(guac_script::Value::Int(4))
    );
    assert_eq!(
        global(&hub, "First_Time_Here"),
        Some(guac_script::Value::Int(1))
    );
    let mut hub_stops = Vec::new();
    for _ in 0..2 {
        saved(
            &mut hub,
            &mut session,
            HUB_1,
            &mut hub_stops,
            &mut start_rows,
            [false; 4],
        );
    }
    hub.join().unwrap_or_else(|h| panic!("{h}"));
    let utilities = hub.placed("_Utilities_Controller").0;
    let from = hub.steps;
    while hub
        .vm
        .placement_of(utilities)
        .and_then(|p| hub.vm.behaviour(p))
        != Some("main")
    {
        assert!(hub.steps < from + 600, "the parts never hung");
        saved(
            &mut hub,
            &mut session,
            HUB_1,
            &mut hub_stops,
            &mut start_rows,
            [false; 4],
        );
    }
    for _ in 0..10 {
        saved(
            &mut hub,
            &mut session,
            HUB_1,
            &mut hub_stops,
            &mut start_rows,
            [false; 4],
        );
    }
    let ship = ship_parts::report(&hub.vm, &hub.world);
    println!("the ship at the hub: {ship:?}");
    assert!(ship.equipped.contains(&part_id), "{ship:?}");
    let hung = format!("ShipPart_{part_id:03}");
    assert!(ship.hung.contains(&hung), "{hung}: {ship:?}");
    let saved_row = start_rows.last().expect("the hub's start saved");
    for done in [
        "Level_213_Tutorial",
        "Level_214_Tutorial_2",
        "Level_201_Rescue_the_Hermit",
        "Level_205_Pirate_Cards",
    ] {
        assert!(
            guacamole::story_save::done(saved_row, done),
            "{done} done in the save"
        );
    }
    // Its first visit's intro to its end, then its way on as after 201:
    // Cali's question no, Flynn's yes, which asks the level after 205.
    let intro = hub.placement("CS_Intro_Controller");
    while !hub.vm.removed(intro) {
        assert!(hub.steps < 6000, "the hub's intro never ended");
        saved(
            &mut hub,
            &mut session,
            HUB_1,
            &mut hub_stops,
            &mut start_rows,
            [false; 4],
        );
    }
    let cali = hub_talk(&mut hub, &mut hub_stops, Vec3::from_array(BY_CALI), false);
    println!("Cali: {cali:?}");
    assert!(hub.world.level_change().is_none());
    let flynn = hub_talk(&mut hub, &mut hub_stops, Vec3::from_array(BY_FLYNN), true);
    let change = hub.world.level_change().cloned();
    println!("Flynn: {flynn:?}; the level change {change:?}");
    assert_eq!(cali, [Some("03_Cali_WantToTry?".to_owned())]);
    assert_eq!(flynn, [Some("208_Flynn_YES/NO_string".to_owned())]);
    let change = change.expect("Flynn's yes changes level");
    assert_eq!(change.name, NEXT_LEVEL);
    assert!(hub_stops.is_empty(), "{hub_stops:#?}");
    println!(
        "205 {steps_205} steps, the hub {} steps; {:.1} s; load at end: {}",
        hub.steps,
        started.elapsed().as_secs_f64(),
        load()
    );
}

/// Flynn at the helm and Cali on the main deck of the hub's first stage,
/// where a player talks to each (`giants_hub`'s).
const BY_FLYNN: [f32; 3] = [0.10, -4.64, 47.5];
const BY_CALI: [f32; 3] = [-10.30, -12.61, 16.91];
/// The adventure after 205 in the disc's order, by its archive.
const NEXT_LEVEL: &str = "level_208_ice";

/// The hero set down at `at`, then the talk button pressed every 20 steps
/// until a cutscene starts, and the steps run until it ends or the run
/// changes level, each question answered `yes` as the TV's modal answers
/// it, 90 steps on (`giants_hub`'s `talk`): the questions asked.
fn hub_talk(run: &mut Run, stops: &mut Vec<String>, at: Vec3, yes: bool) -> Vec<Option<String>> {
    run.put(at, 0.0);
    let mut asked = Vec::new();
    let (mut began, mut read) = (false, 0);
    for k in 0..3600u32 {
        still(run, stops, [false, false, false, !began && k % 20 == 0]);
        let cs = cutscene_name(run);
        began |= cs.is_some();
        if let Some(p) = guacamole::prompt::up(&run.vm, &run.world) {
            read += 1;
            if read < 90 {
                continue;
            }
            read = 0;
            asked.push(match p.question {
                guac_script::Value::Object(o) => run.vm.program().name_of(o).map(str::to_owned),
                _ => None,
            });
            assert!(
                run.with_host(|vm, host| guacamole::prompt::answer(vm, host, &p, yes))
                    .unwrap_or_else(|h| panic!("the answer: {h}"))
            );
        }
        if (began && cs.is_none()) || run.world.level_change().is_some() {
            break;
        }
    }
    still(run, stops, [false; 4]);
    asked
}

/// Where the hero has stood every quarter second over AS.325's stretch, for
/// the Mac's driver to follow, and where each of its legs ended (the
/// trail's length then, the leg's name).
#[derive(Default)]
struct Trail(Vec<Vec3>, Vec<(usize, &'static str)>);

impl Trail {
    fn note(&mut self, run: &Run) {
        if run.steps.is_multiple_of(15) {
            self.0.push(run.hero().pivot);
        }
    }

    fn mark(&mut self, run: &Run, leg: &'static str) {
        self.0.push(run.hero().pivot);
        self.1.push((self.0.len(), leg));
    }
}

/// Cutscenes waited out, a prompt's question answered yes as the TV's modal
/// answers it, and Fangs' game stood in won (`level_205::card_house_stand_in`)
/// before the step: whether the step was the scene's.
fn scene_step(
    run: &mut Run,
    stops: &mut Vec<String>,
    scenes: &mut Vec<(u32, String)>,
    stood_in: &mut u32,
) -> bool {
    if let Some(p) = guacamole::prompt::up(&run.vm, &run.world) {
        assert!(
            run.with_host(|vm, host| guacamole::prompt::answer(vm, host, &p, true))
                .unwrap_or_else(|h| panic!("the answer: {h}"))
        );
    }
    if guacamole::level_205::card_house_stand_in(&mut run.vm, &mut run.world, LEVEL) {
        *stood_in += 1;
    }
    if let Some(cs) = cutscene_name(run) {
        if scenes.last().is_none_or(|(_, n)| *n != cs) {
            scenes.push((run.steps, cs));
        }
        still(run, stops, [false; 4]);
        return true;
    }
    if guacamole::anchors::scene_holds(&run.vm) {
        still(run, stops, [false; 4]);
        return true;
    }
    false
}

/// The stick held toward each of `points` (x, z) in turn until he's within
/// 0.7 m of it across the ground, cutscenes waited out: whether he came to
/// the last within `cap` steps a point.
fn go_by(
    run: &mut Run,
    stops: &mut Vec<String>,
    scenes: &mut Vec<(u32, String)>,
    trail: &mut Trail,
    points: &[(f32, f32)],
    cap: u32,
) -> bool {
    let mut stood_in = 0;
    for &(x, z) in points {
        let mut there = false;
        for _ in 0..cap {
            trail.note(run);
            if scene_step(run, stops, scenes, &mut stood_in) {
                continue;
            }
            let me = run.hero().pivot;
            let (dx, dz) = (x - me.x, z - me.z);
            if dx.hypot(dz) < 0.7 {
                there = true;
                break;
            }
            run.bench.view = dx.atan2(dz).to_degrees();
            run.bench.hold_view = true;
            step(run, stops, Stick::new(0.0, 1.0), [false; 4]);
        }
        if !there {
            println!("!! short of ({x}, {z}) at {}", run.hero().pivot);
            return false;
        }
    }
    true
}

/// Over the flood's cells ([`way_to`]) to within 0.7 m of the cell nearest
/// `to`, which must lie within `near` m of it.
fn walk_to(
    run: &mut Run,
    stops: &mut Vec<String>,
    scenes: &mut Vec<(u32, String)>,
    trail: &mut Trail,
    to: Vec3,
    near: f32,
) {
    walk_until(run, stops, scenes, trail, to, near, &|_| false);
}

/// [`walk_to`], over early once `stop` holds (a room's door taking him out).
fn walk_until(
    run: &mut Run,
    stops: &mut Vec<String>,
    scenes: &mut Vec<(u32, String)>,
    trail: &mut Trail,
    to: Vec3,
    near: f32,
    stop: &dyn Fn(&Run) -> bool,
) {
    let (way, off) = way_to(run, run.hero().pivot, to);
    assert!(
        off < near,
        "{to} is {off:.1} m off any walk from {}",
        run.hero().pivot
    );
    // The stick on the first cell 1.5 m off past the nearest of the next
    // six, as [`walk`] aims.
    let flat = |a: Vec3, b: Vec3| Vec3::new(a.x - b.x, 0.0, a.z - b.z).length();
    let (mut next, mut stood_in) = (0, 0);
    for _ in 0..8000 {
        trail.note(run);
        if scene_step(run, stops, scenes, &mut stood_in) {
            continue;
        }
        let at = run.hero().pivot;
        if flat(at, *way.last().unwrap()) < 0.7 || stop(run) {
            return;
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
    }
    panic!(
        "toward {to}: at {}, in {:?} (held {}), scenes {scenes:?}",
        run.hero().pivot,
        cutscene_name(run),
        guacamole::anchors::scene_holds(&run.vm)
    );
}

/// △ tapped every 20 steps until `done` holds or `cap` steps pass: whether
/// it held.
fn tap_until(
    run: &mut Run,
    stops: &mut Vec<String>,
    cap: u32,
    done: &dyn Fn(&Run) -> bool,
) -> bool {
    for k in 0..cap {
        if done(run) {
            return true;
        }
        still(run, stops, [false, false, false, k % 20 == 10]);
    }
    done(run)
}

/// A push block shoved along `heading` (degrees, the stick's) as a kid
/// pushes it, the stick held at a point 3 m past it, until `done` holds:
/// how many moves it made.
fn shove(
    run: &mut Run,
    stops: &mut Vec<String>,
    trail: &mut Trail,
    block: &str,
    heading: f32,
    done: &dyn Fn(&Run) -> bool,
) -> u32 {
    let h = run.placed(block).0;
    let along = Vec3::new(heading.to_radians().sin(), 0.0, heading.to_radians().cos());
    let (mut moves, mut was) = (0, false);
    for _ in 0..1500 {
        trail.note(run);
        if done(run) {
            break;
        }
        let aim = run.world.actor(h).unwrap().position + along * 3.0;
        let me = run.hero().pivot;
        run.bench.view = (aim.x - me.x).atan2(aim.z - me.z).to_degrees();
        run.bench.hold_view = true;
        step(run, stops, Stick::new(0.0, 1.0), [false; 4]);
        let now = behaviour(run, block).starts_with("2.");
        moves += u32::from(now && !was);
        was = now;
    }
    for _ in 0..30 {
        still(run, stops, [false; 4]);
    }
    moves
}

/// Blobbers' crane's wheel, from the ground west of the crane up the north
/// steps onto its deck, and back.
const UP_TO_THE_WHEEL: [(f32, f32); 6] = [
    (50.0, 139.0),
    (56.0, 139.0),
    (60.0, 138.5),
    (60.0, 136.0),
    (57.0, 133.0),
    (54.5, 131.6),
];
const DOWN_FROM_THE_WHEEL: [(f32, f32); 6] = [
    (57.0, 133.0),
    (60.0, 136.0),
    (60.0, 138.5),
    (56.0, 139.0),
    (50.0, 139.0),
    (47.0, 133.0),
];

/// Every enemy Setup_002's sets name, and its heavy elf.
const SETUP_002_FOES: [&str; 15] = [
    "Enemy_Chompy(1)",
    "Enemy_Chompy(2)",
    "Enemy_Chompy(3)",
    "Enemy_Chompy(4)",
    "Enemy_Chompy(5)",
    "Enemy_Chompy(6)",
    "Enemy_Chompy(7)",
    "Enemy_Chompy(8)",
    "Enemy_Chompy(9)",
    "Enemy_Chompy(10)",
    "Enemy_Chompy(11)",
    "Enemy_Chompy(12)",
    "Enemy_Elf_Heavy_S2(1)",
    "Enemy_Elf_Lobber(3)",
    "Enemy_Elf_Lobber(4)",
];

/// The armored Chompy and the market's three, whom its intro sets on him.
const MARKET_FOES: [&str; 4] = [
    "Enemy_Chompy(13)",
    "Enemy_Chompy(14)",
    "Enemy_Chompy(15)",
    "Enemy_Chompy(16)",
];

/// The crane's facing now, 0-360.
fn crane(run: &Run) -> f32 {
    facing(run, "Crane(2)").rem_euclid(360.0)
}

/// openspec AS.325: level 205 from its first yard to Setup_002's end, each
/// thing on the way worked by its own trigger. Over the crane (AS.283) and
/// through Setup_001 (AS.305); the five gone, the jump pad below the key
/// ledge throws him up onto it and he takes the first key; off the ledge
/// the key opens the door west of the yard; in the market the armored
/// Chompy's intro plays and its Chompies are fought; at Blobbers' crane the
/// wheel turns the boat south, the block by it is pushed aboard, the boat
/// turned west carries it and it's pushed off the far end into the
/// channel, and turned south again the boat bridges the gap and the block
/// past it is pushed into the channel there; in Setup_002's yard the
/// pirate with the key runs into the card house, its door opening for him;
/// in the house Fangs' intro plays, his game is stood in won (the TV has
/// no card game, AS.329), his outro plays and the gate before the key
/// lowers, and the key taken, the gate by the door; carried back to the
/// yard the key sets Setup_002's fight off, which ends with its
/// controller. Nothing stops.
#[test]
#[ignore = "reads the Giants dump"]
fn level_205s_way_on_from_the_first_yard_ends_setup_002() {
    let started = std::time::Instant::now();
    println!("load at start: {}", load());
    let mut stops = Vec::new();
    let mut run = joined_205(&mut stops);
    let mut scenes = over_the_crane(&mut run, &mut stops);
    let (_, goal) = run.placed("Setup_001_Trigger(2)");
    let yard = fight(
        &mut run,
        &mut stops,
        "Setup_001_Controller",
        &YARD_FOES,
        goal,
        8000,
    );
    assert!(yard.ended, "the first yard: {:?}", yard.log);
    scenes.extend(yard.scenes.iter().cloned());
    println!("the first yard over at step {}", run.steps);
    let mut trail = Trail::default();
    let mut topups = yard.topups.clone();
    trail.mark(&run, "yard-end");

    // The jump pad, laid once the five are gone, throws him up onto the
    // key ledge.
    let pad = run.placed("PvP_JumpPad_SUPER(8)").0;
    let pad_range = |run: &Run| run.world.actor(pad).map(|a| a.range);
    assert_eq!(pad_range(&run), Some(0.0), "no pad before");
    walk_to(
        &mut run,
        &mut stops,
        &mut scenes,
        &mut trail,
        Vec3::new(88.7, 17.8, 170.0),
        1.5,
    );
    // Its pan to the pad, then it goes.
    let pan_from = run.steps;
    let mut stood_in = 0;
    for _ in 0..600 {
        if behaviour(&run, "SpecialJumpPadActivator_For_JumpPad08") == "removed" {
            break;
        }
        if !scene_step(&mut run, &mut stops, &mut scenes, &mut stood_in) {
            still(&mut run, &mut stops, [false; 4]);
        }
    }
    println!("the pad's activator done {} steps on", run.steps - pan_from);
    assert_eq!(
        behaviour(&run, "SpecialJumpPadActivator_For_JumpPad08"),
        "removed",
        "the pad's activator done"
    );
    assert_eq!(pad_range(&run), Some(230.0), "the pad laid");
    trail.mark(&run, "pad");
    let below = run.hero().pivot.y;
    run.bench.view = 180.0;
    run.bench.hold_view = true;
    for _ in 0..240 {
        trail.note(&run);
        step(&mut run, &mut stops, Stick::new(0.0, 1.0), [false; 4]);
    }
    let ledge = run.hero().pivot;
    println!("thrown from {below:.2} up to {ledge} at step {}", run.steps);
    assert!(ledge.y > below + 3.0, "up on the ledge: {ledge}");
    trail.mark(&run, "ledge");
    let key_held = |run: &Run| {
        run.world.clones().into_iter().any(|h| {
            run.world.name(h) == "Key Pickup"
                && run.world.actor(h).is_some_and(|a| !a.removed)
                && run
                    .vm
                    .placement_of(h)
                    .and_then(|i| run.vm.behaviour(i))
                    .is_some_and(|b| b == "grabbed by player")
        })
    };
    assert!(key_held(&run), "the first key taken");

    // Off the ledge to the door west of the yard: △ turns the key and it
    // opens.
    run.bench.view = 0.0;
    for _ in 0..90 {
        trail.note(&run);
        step(&mut run, &mut stops, Stick::new(0.0, 1.0), [false; 4]);
    }
    walk_to(
        &mut run,
        &mut stops,
        &mut scenes,
        &mut trail,
        Vec3::new(71.2, 17.85, 173.5),
        1.5,
    );
    trail.mark(&run, "door");
    assert!(
        tap_until(&mut run, &mut stops, 900, &|r| behaviour(
            r,
            "Door_Template(3)"
        ) == "done"),
        "the key door open: {}",
        behaviour(&run, "Door_Template(3)")
    );
    assert!(!key_held(&run), "the key in its lock");
    println!("the key door open at step {}", run.steps);

    trail.mark(&run, "door-open");
    // The market: the armored Chompy's intro, then its Chompies fought on
    // the way to the crane.
    walk_to(
        &mut run,
        &mut stops,
        &mut scenes,
        &mut trail,
        Vec3::new(64.2, 20.6, 158.7),
        1.5,
    );
    let west_of_crane = Vec3::new(47.2, 21.12, 133.06);
    trail.mark(&run, "market");
    let market = fight_to(
        &mut run,
        &mut stops,
        "IGC_Armored_Chompy",
        &MARKET_FOES,
        west_of_crane,
        9000,
        &|r: &Run| {
            let me = r.hero().pivot;
            MARKET_FOES
                .iter()
                .all(|n| matches!(behaviour(r, n).as_str(), "removed" | "asleep"))
                && Vec3::new(me.x - west_of_crane.x, 0.0, me.z - west_of_crane.z).length() < 1.5
        },
    );
    scenes.extend(market.scenes.iter().cloned());
    topups.extend(market.topups.iter().copied());
    trail.0.extend(market.trail.iter().copied());
    assert!(market.ended, "the market: {:?}", market.log);
    assert!(
        scenes.iter().any(|(_, n)| n == "205_IGC_Armored_Chompy"),
        "the armored Chompy's intro: {scenes:?}"
    );
    println!("by the crane at step {}", run.steps);

    trail.mark(&run, "crane-west");
    // Blobbers' crane: south, the block aboard, west, the block off into
    // the channel, south again, the far block into the channel there.
    assert_eq!(crane(&run), 270.0, "the boat lies west");
    let turn = |run: &mut Run,
                stops: &mut Vec<String>,
                scenes: &mut Vec<(u32, String)>,
                trail: &mut Trail,
                to: f32| {
        assert!(
            go_by(run, stops, scenes, trail, &UP_TO_THE_WHEEL, 900),
            "at the wheel"
        );
        trail.mark(
            run,
            if to == 180.0 {
                "wheel-south"
            } else {
                "wheel-west"
            },
        );
        assert!(
            tap_until(run, stops, 900, &|r| crane(r) == to
                && behaviour(r, "Crane(2)").starts_with("turned")),
            "the crane turned to {to}: {}",
            crane(run)
        );
        assert_eq!(
            facing(run, "Crane_Head(1)").rem_euclid(360.0),
            to,
            "its mast with it"
        );
        assert!(
            go_by(run, stops, scenes, trail, &DOWN_FROM_THE_WHEEL, 900),
            "down from the wheel"
        );
        trail.mark(run, "down");
    };
    turn(&mut run, &mut stops, &mut scenes, &mut trail, 180.0);
    assert!(go_by(
        &mut run,
        &mut stops,
        &mut scenes,
        &mut trail,
        &[(49.7, 117.0)],
        900
    ));
    trail.mark(&run, "block-1");
    let aboard = shove(
        &mut run,
        &mut stops,
        &mut trail,
        "Push_Block_Template(1)",
        180.0,
        &|r| {
            r.at("Push_Block_Template(1)").z < 108.5
                && behaviour(r, "Push_Block_Template(1)").starts_with("1.")
        },
    );
    trail.mark(&run, "pushed-1");
    let on_boat = run.at("Push_Block_Template(1)");
    println!("the block aboard at {on_boat} in {aboard} moves");
    assert!(on_boat.z < 108.5, "aboard: {on_boat}");
    assert!(go_by(
        &mut run,
        &mut stops,
        &mut scenes,
        &mut trail,
        &[(47.0, 125.0), (47.0, 133.0)],
        900
    ));
    turn(&mut run, &mut stops, &mut scenes, &mut trail, 270.0);
    assert_eq!(
        behaviour(&run, "Push_Block_Template(1)"),
        "asleep",
        "carried off"
    );
    // The crate that draws the block riding the boat went west with it.
    let crate_z = run.at("FAke_Block(2)").z;
    assert!(
        crate_z > 125.0,
        "the riding crate carried west: z {crate_z}"
    );
    assert!(
        behaviour(&run, "Push_Block_Template(2)").starts_with("1."),
        "at the boat's west end: {}",
        behaviour(&run, "Push_Block_Template(2)")
    );
    assert!(go_by(
        &mut run,
        &mut stops,
        &mut scenes,
        &mut trail,
        &[(40.0, 131.0), (31.5, 130.7)],
        900
    ));
    trail.mark(&run, "block-2");
    let off_west = shove(
        &mut run,
        &mut stops,
        &mut trail,
        "Push_Block_Template(2)",
        270.0,
        &|r| r.at("Push_Block_Template(2)").y < 18.5,
    );
    trail.mark(&run, "pushed-2");
    let west = run.at("Push_Block_Template(2)");
    println!("the block in the west channel at {west} in {off_west} moves");
    assert!(west.y < 18.5, "in the channel: {west}");
    assert!(go_by(
        &mut run,
        &mut stops,
        &mut scenes,
        &mut trail,
        &[(24.0, 130.7), (31.0, 130.7), (40.0, 131.0), (47.0, 133.0)],
        900
    ));
    turn(&mut run, &mut stops, &mut scenes, &mut trail, 180.0);
    assert!(go_by(
        &mut run,
        &mut stops,
        &mut scenes,
        &mut trail,
        &[(49.7, 116.0), (49.7, 111.0), (49.7, 102.5)],
        900
    ));
    trail.mark(&run, "block-7");
    let into = shove(
        &mut run,
        &mut stops,
        &mut trail,
        "Push_Block_Template(7)",
        180.0,
        &|r| behaviour(r, "Push_Block_Template(7)") == "special - push block inactive",
    );
    trail.mark(&run, "pushed-7");
    let south = run.at("Push_Block_Template(7)");
    println!(
        "the far block in the channel at {south} in {into} moves, step {}",
        run.steps
    );
    assert_eq!(
        behaviour(&run, "Push_Block_Template(7)"),
        "special - push block inactive"
    );

    // Setup_002's yard: its first Chompies fought as they come, the pirate
    // with the key off into the card house, its door swung open for him.
    let door = Vec3::new(39.5, 26.3, 53.5);
    let part_1 = &SETUP_002_FOES[..6];
    let yard_2 = fight_to(
        &mut run,
        &mut stops,
        "Setup_002_Controller",
        part_1,
        door,
        9000,
        &|r: &Run| {
            let me = r.hero().pivot;
            behaviour(r, "Fake_Card_Pirate_1") == "removed"
                && Vec3::new(me.x - door.x, 0.0, me.z - door.z).length() < 1.5
        },
    );
    scenes.extend(yard_2.scenes.iter().cloned());
    topups.extend(yard_2.topups.iter().copied());
    trail.0.extend(yard_2.trail.iter().copied());
    assert!(yard_2.ended, "to the card house: {:?}", yard_2.log);
    let woke = yard_2
        .log
        .iter()
        .find(|e| e.1 == "Setup_002_Controller" && e.2 == "wait")
        .map(|e| e.0);
    println!(
        "Setup_002 waiting from step {woke:?}; at the card house at step {}",
        run.steps
    );
    assert!(woke.is_some(), "Setup_002 woke: {:?}", yard_2.log);

    trail.mark(&run, "card-house");
    // Into the card house: Fangs' intro, his game (stood in won), his
    // outro, the gate before the key down.
    let mut stood_in = 0;
    for _ in 0..600 {
        trail.note(&run);
        if run.hero().pivot.y < -300.0 {
            break;
        }
        run.bench.view = (38.7f32 - run.hero().pivot.x)
            .atan2(51.2 - run.hero().pivot.z)
            .to_degrees();
        step(&mut run, &mut stops, Stick::new(0.0, 1.0), [false; 4]);
    }
    assert!(
        run.hero().pivot.y < -300.0,
        "in the card house: {}",
        run.hero().pivot
    );
    let by_fangs = Vec3::new(377.0, -304.4, 288.3);
    let mut lowered = false;
    for k in 0..6000u32 {
        trail.note(&run);
        if behaviour(&run, "Spike_Gate_Wooden_Template(2)").starts_with("spikes in down") {
            lowered = true;
            break;
        }
        if scene_step(&mut run, &mut stops, &mut scenes, &mut stood_in) {
            continue;
        }
        let me = run.hero().pivot;
        let (dx, dz) = (by_fangs.x - me.x, by_fangs.z - me.z);
        if dx.hypot(dz) < 1.0 {
            still(&mut run, &mut stops, [false, false, false, k % 20 == 10]);
        } else {
            run.bench.view = dx.atan2(dz).to_degrees();
            step(&mut run, &mut stops, Stick::new(0.0, 1.0), [false; 4]);
        }
    }
    println!(
        "the key's gate down {lowered} at step {}, stood in {stood_in}",
        run.steps
    );
    assert!(
        lowered,
        "the gate before the key: {}",
        behaviour(&run, "Spike_Gate_Wooden_Template(2)")
    );
    assert_eq!(stood_in, 1, "his game stood in once");
    assert_eq!(behaviour(&run, "Gate_Killer"), "removed");
    for scene in ["Casino Pirate Intro", "Casino Pirate Outro"] {
        assert!(
            scenes.iter().any(|(_, n)| n == scene),
            "{scene}: {scenes:?}"
        );
    }
    // The key, then out past the gate by the door.
    walk_to(
        &mut run,
        &mut stops,
        &mut scenes,
        &mut trail,
        Vec3::new(357.5, -304.3, 286.2),
        1.5,
    );
    for _ in 0..60 {
        still(&mut run, &mut stops, [false; 4]);
    }
    assert!(key_held(&run), "the card house's key taken");
    // The second gate killer goes with the key taken, and the door's gate
    // with it; it isn't listed: the walk is the same to the step
    // with it listed or not.
    assert_eq!(
        behaviour(&run, "Gate_Killer(1)"),
        "removed",
        "the door's gate let go"
    );
    walk_until(
        &mut run,
        &mut stops,
        &mut scenes,
        &mut trail,
        Vec3::new(405.0, -304.4, 278.8),
        1.5,
        &|r| r.hero().pivot.y > 0.0,
    );
    for _ in 0..900 {
        trail.note(&run);
        if run.hero().pivot.y > 0.0 {
            break;
        }
        let me = run.hero().pivot;
        run.bench.view = (407.2 - me.x).atan2(278.76 - me.z).to_degrees();
        step(&mut run, &mut stops, Stick::new(0.0, 1.0), [false; 4]);
    }
    for _ in 0..120 {
        still(&mut run, &mut stops, [false; 4]);
    }
    assert!(
        run.hero().pivot.y > 0.0,
        "out of the card house: {}",
        run.hero().pivot
    );
    assert!(key_held(&run), "the key carried out");
    println!(
        "out with the key at {} at step {}",
        run.hero().pivot,
        run.steps
    );

    // Back in the yard with the key: Setup_002's fight to its end.
    let (_, far) = run.placed("Setup_002_Trigger(1)");
    let fight_2 = fight(
        &mut run,
        &mut stops,
        "Setup_002_Controller",
        &SETUP_002_FOES,
        far,
        12000,
    );
    scenes.extend(fight_2.scenes.iter().cloned());
    topups.extend(fight_2.topups.iter().copied());
    trail.0.extend(fight_2.trail.iter().copied());
    for (step, n, b) in &fight_2.log {
        println!("  {step} {n} {b}");
    }
    println!("scenes {scenes:?}");
    println!(
        "over at step {}, ended {}; top-ups {topups:?}, lowest health {:?}; {:.1} s",
        run.steps,
        fight_2.ended,
        fight_2.lowest,
        started.elapsed().as_secs_f64()
    );
    let t: Vec<String> = trail
        .0
        .iter()
        .map(|p| format!("{:.2},{:.2},{:.2}", p.x, p.y, p.z))
        .collect();
    println!("TRAIL {}", t.join(";"));
    let m: Vec<String> = trail.1.iter().map(|(i, n)| format!("{i}:{n}")).collect();
    println!("MARKS {}", m.join(";"));
    println!("the card house's key still carried: {}", key_held(&run));
    assert!(
        fight_2.ended,
        "Setup_002's fight never ended: {:?}",
        fight_2.log
    );
    for n in [
        "Enemy_Elf_Lobber(3)",
        "Enemy_Elf_Lobber(4)",
        "Enemy_Elf_Heavy_S2(1)",
    ] {
        assert_eq!(behaviour(&run, n), "removed", "{n}");
    }
    assert!(stops.is_empty(), "{stops:#?}");
    println!("load at end: {}", load());
}

/// The beds [`BEDS`] holds started at or after `from`, by name.
fn beds_from(from: u32) -> Vec<(u32, String)> {
    BEDS.with_borrow(|beds| {
        beds.iter()
            .filter(|(at, _, _)| *at >= from)
            .map(|(at, n, _)| (*at, n.clone()))
            .collect()
    })
}

/// The dynamic sound controller's flag `name` (the combat watchers' `in
/// combat`), as an int.
fn sound_flag(run: &Run, name: &str) -> Option<i32> {
    let id = run.placement("dynamic_sound_controller_205");
    match run.vm.value(run.vm.local(id, name)?) {
        guac_script::Value::Int(i) => Some(i),
        _ => None,
    }
}

/// openspec AS.289: level 205's own sound scripts play its music, as a kid
/// meets it. At the start the intro scene's track with the night's
/// ambience, the gameplay intro once the intro hands him over, the ship's
/// wind on the boat. At the first yard the first combat watcher wakes
/// before its fight and marks the controller in combat, its scripts'
/// transition to combat keeping the music as it is; the five beaten, it
/// takes itself off on the thin explore bed, out of combat. Then into the
/// house by the yard (`Interior_Controller_2`) at its door: the inside bed
/// and the house's ambience. Nothing stops.
#[test]
#[ignore = "reads the Giants dump"]
fn level_205s_music_follows_its_start_its_first_fight_and_an_interior() {
    println!("load at start: {}", load());
    BEDS.with_borrow_mut(Vec::clear);
    let mut stops = Vec::new();
    let mut run = joined_205(&mut stops);
    over_the_crane(&mut run, &mut stops);
    let to_yard = beds_from(0);
    println!("to the yard {to_yard:?}");
    let first =
        |beds: &[(u32, String)], name: &str| beds.iter().find(|(_, n)| n == name).map(|b| b.0);
    // The intro's track and the night's ambience from the start, the
    // gameplay intro once the intro is skipped (by step 140), the ship's
    // wind on the boat.
    assert!(first(&to_yard, "mu_205_intro_igc").is_some_and(|s| s <= 10));
    assert!(first(&to_yard, "am_night_exterior").is_some_and(|s| s <= 10));
    let gameplay = first(&to_yard, "mu_205_v2_intro_gameplay");
    assert!(
        gameplay.is_some_and(|s| (100..400).contains(&s)),
        "{gameplay:?}"
    );
    assert!(first(&to_yard, "am_windy_night_on_ship").is_some());

    let watcher = "combat_watcher_1 (town entrance)";
    let in_combat_before = sound_flag(&run, "in combat");
    let watching_before = behaviour(&run, watcher);
    let at_yard = run.steps;
    let (_, goal) = run.placed("Setup_001_Trigger(2)");
    let fought = fight(
        &mut run,
        &mut stops,
        "Setup_001_Controller",
        &YARD_FOES,
        goal,
        8000,
    );
    let first_awake = fought
        .log
        .iter()
        .find(|(_, n, b)| n.starts_with("Enemy_") && b != "asleep")
        .map(|e| e.0);
    let last_off = YARD_FOES.iter().filter_map(|n| fought.removed(n)).max();
    for _ in 0..120 {
        still(&mut run, &mut stops, [false; 4]);
    }
    let in_yard = beds_from(at_yard);
    println!(
        "watcher {watching_before} and in combat {in_combat_before:?} at the yard; first awake {first_awake:?}, last off {last_off:?}; beds {in_yard:?}; after, watcher {} and in combat {:?}",
        behaviour(&run, watcher),
        sound_flag(&run, "in combat")
    );
    assert!(fought.ended, "the fight never ended");
    // Woken as he came to the yard, before its fight.
    assert_eq!(watching_before, "watching");
    assert_eq!(in_combat_before, Some(1));
    // The music unchanged through the fight, the thin explore bed as the
    // last foe goes and the watcher with it.
    let last_off = last_off.expect("all five off");
    let explore = first(&in_yard, "mu_205_v2_explore_thin");
    assert!(
        explore.is_some_and(|s| s >= last_off && s <= last_off + 5),
        "{explore:?} against {last_off}"
    );
    assert!(
        in_yard
            .iter()
            .all(|(s, n)| !n.starts_with("mu_") || *s >= last_off),
        "{in_yard:?}"
    );
    assert_eq!(behaviour(&run, watcher), "removed");
    assert_eq!(sound_flag(&run, "in combat"), Some(0));

    // Into the house by the yard at its door.
    let (door, facing) =
        guacamole::interiors::door(&run.world, LEVEL, "Interior_Controller_2").expect("its door");
    run.put(door, facing);
    let at_door = run.steps;
    for k in 0..600u32 {
        let s = if k < 120 {
            let d = (facing - run.bench.view).to_radians();
            Stick::new(-d.sin(), d.cos())
        } else {
            Stick::new(0.0, 0.0)
        };
        step(&mut run, &mut stops, s, [false; 4]);
    }
    let inside = beds_from(at_door);
    println!("inside at {}: {inside:?}", run.hero().pivot);
    assert!(run.hero().pivot.y < -250.0, "in its room");
    assert!(first(&inside, "mu_205_v2_inside").is_some(), "{inside:?}");
    assert!(first(&inside, "am_house_interior").is_some(), "{inside:?}");
    assert!(stops.is_empty(), "{stops:#?}");
    println!("load at end: {}", load());
}

/// Where AS.325's walk ends ([`on_to_setup_002s_end`]): Setup_002's
/// controller gone, the card house's key carried.
struct AtSetup002sEnd {
    run: Run,
    stops: Vec<String>,
    scenes: Vec<(u32, String)>,
    trail: Trail,
    topups: Vec<(u32, f32)>,
}

/// AS.325's walk to Setup_002's controller gone, step for step as
/// `level_205s_way_on_from_the_first_yard_ends_setup_002` walks it (a copy:
/// that test is left as it stands), for the stretch on from there to carry
/// on from: the same run, its state the one that walk leaves.
fn on_to_setup_002s_end() -> AtSetup002sEnd {
    let started = std::time::Instant::now();
    println!("load at start: {}", load());
    let mut stops = Vec::new();
    let mut run = joined_205(&mut stops);
    let mut scenes = over_the_crane(&mut run, &mut stops);
    let (_, goal) = run.placed("Setup_001_Trigger(2)");
    let yard = fight(
        &mut run,
        &mut stops,
        "Setup_001_Controller",
        &YARD_FOES,
        goal,
        8000,
    );
    assert!(yard.ended, "the first yard: {:?}", yard.log);
    scenes.extend(yard.scenes.iter().cloned());
    println!("the first yard over at step {}", run.steps);
    let mut trail = Trail::default();
    let mut topups = yard.topups.clone();
    trail.mark(&run, "yard-end");

    // The jump pad, laid once the five are gone, throws him up onto the
    // key ledge.
    let pad = run.placed("PvP_JumpPad_SUPER(8)").0;
    let pad_range = |run: &Run| run.world.actor(pad).map(|a| a.range);
    assert_eq!(pad_range(&run), Some(0.0), "no pad before");
    walk_to(
        &mut run,
        &mut stops,
        &mut scenes,
        &mut trail,
        Vec3::new(88.7, 17.8, 170.0),
        1.5,
    );
    // Its pan to the pad, then it goes.
    let pan_from = run.steps;
    let mut stood_in = 0;
    for _ in 0..600 {
        if behaviour(&run, "SpecialJumpPadActivator_For_JumpPad08") == "removed" {
            break;
        }
        if !scene_step(&mut run, &mut stops, &mut scenes, &mut stood_in) {
            still(&mut run, &mut stops, [false; 4]);
        }
    }
    println!("the pad's activator done {} steps on", run.steps - pan_from);
    assert_eq!(
        behaviour(&run, "SpecialJumpPadActivator_For_JumpPad08"),
        "removed",
        "the pad's activator done"
    );
    assert_eq!(pad_range(&run), Some(230.0), "the pad laid");
    trail.mark(&run, "pad");
    let below = run.hero().pivot.y;
    run.bench.view = 180.0;
    run.bench.hold_view = true;
    for _ in 0..240 {
        trail.note(&run);
        step(&mut run, &mut stops, Stick::new(0.0, 1.0), [false; 4]);
    }
    let ledge = run.hero().pivot;
    println!("thrown from {below:.2} up to {ledge} at step {}", run.steps);
    assert!(ledge.y > below + 3.0, "up on the ledge: {ledge}");
    trail.mark(&run, "ledge");
    let key_held = |run: &Run| {
        run.world.clones().into_iter().any(|h| {
            run.world.name(h) == "Key Pickup"
                && run.world.actor(h).is_some_and(|a| !a.removed)
                && run
                    .vm
                    .placement_of(h)
                    .and_then(|i| run.vm.behaviour(i))
                    .is_some_and(|b| b == "grabbed by player")
        })
    };
    assert!(key_held(&run), "the first key taken");

    // Off the ledge to the door west of the yard: △ turns the key and it
    // opens.
    run.bench.view = 0.0;
    for _ in 0..90 {
        trail.note(&run);
        step(&mut run, &mut stops, Stick::new(0.0, 1.0), [false; 4]);
    }
    walk_to(
        &mut run,
        &mut stops,
        &mut scenes,
        &mut trail,
        Vec3::new(71.2, 17.85, 173.5),
        1.5,
    );
    trail.mark(&run, "door");
    assert!(
        tap_until(&mut run, &mut stops, 900, &|r| behaviour(
            r,
            "Door_Template(3)"
        ) == "done"),
        "the key door open: {}",
        behaviour(&run, "Door_Template(3)")
    );
    assert!(!key_held(&run), "the key in its lock");
    println!("the key door open at step {}", run.steps);

    trail.mark(&run, "door-open");
    // The market: the armored Chompy's intro, then its Chompies fought on
    // the way to the crane.
    walk_to(
        &mut run,
        &mut stops,
        &mut scenes,
        &mut trail,
        Vec3::new(64.2, 20.6, 158.7),
        1.5,
    );
    let west_of_crane = Vec3::new(47.2, 21.12, 133.06);
    trail.mark(&run, "market");
    let market = fight_to(
        &mut run,
        &mut stops,
        "IGC_Armored_Chompy",
        &MARKET_FOES,
        west_of_crane,
        9000,
        &|r: &Run| {
            let me = r.hero().pivot;
            MARKET_FOES
                .iter()
                .all(|n| matches!(behaviour(r, n).as_str(), "removed" | "asleep"))
                && Vec3::new(me.x - west_of_crane.x, 0.0, me.z - west_of_crane.z).length() < 1.5
        },
    );
    scenes.extend(market.scenes.iter().cloned());
    topups.extend(market.topups.iter().copied());
    trail.0.extend(market.trail.iter().copied());
    assert!(market.ended, "the market: {:?}", market.log);
    assert!(
        scenes.iter().any(|(_, n)| n == "205_IGC_Armored_Chompy"),
        "the armored Chompy's intro: {scenes:?}"
    );
    println!("by the crane at step {}", run.steps);

    trail.mark(&run, "crane-west");
    // Blobbers' crane: south, the block aboard, west, the block off into
    // the channel, south again, the far block into the channel there.
    assert_eq!(crane(&run), 270.0, "the boat lies west");
    let turn = |run: &mut Run,
                stops: &mut Vec<String>,
                scenes: &mut Vec<(u32, String)>,
                trail: &mut Trail,
                to: f32| {
        assert!(
            go_by(run, stops, scenes, trail, &UP_TO_THE_WHEEL, 900),
            "at the wheel"
        );
        trail.mark(
            run,
            if to == 180.0 {
                "wheel-south"
            } else {
                "wheel-west"
            },
        );
        assert!(
            tap_until(run, stops, 900, &|r| crane(r) == to
                && behaviour(r, "Crane(2)").starts_with("turned")),
            "the crane turned to {to}: {}",
            crane(run)
        );
        assert_eq!(
            facing(run, "Crane_Head(1)").rem_euclid(360.0),
            to,
            "its mast with it"
        );
        assert!(
            go_by(run, stops, scenes, trail, &DOWN_FROM_THE_WHEEL, 900),
            "down from the wheel"
        );
        trail.mark(run, "down");
    };
    turn(&mut run, &mut stops, &mut scenes, &mut trail, 180.0);
    assert!(go_by(
        &mut run,
        &mut stops,
        &mut scenes,
        &mut trail,
        &[(49.7, 117.0)],
        900
    ));
    trail.mark(&run, "block-1");
    let aboard = shove(
        &mut run,
        &mut stops,
        &mut trail,
        "Push_Block_Template(1)",
        180.0,
        &|r| {
            r.at("Push_Block_Template(1)").z < 108.5
                && behaviour(r, "Push_Block_Template(1)").starts_with("1.")
        },
    );
    trail.mark(&run, "pushed-1");
    let on_boat = run.at("Push_Block_Template(1)");
    println!("the block aboard at {on_boat} in {aboard} moves");
    assert!(on_boat.z < 108.5, "aboard: {on_boat}");
    assert!(go_by(
        &mut run,
        &mut stops,
        &mut scenes,
        &mut trail,
        &[(47.0, 125.0), (47.0, 133.0)],
        900
    ));
    turn(&mut run, &mut stops, &mut scenes, &mut trail, 270.0);
    assert_eq!(
        behaviour(&run, "Push_Block_Template(1)"),
        "asleep",
        "carried off"
    );
    // The crate that draws the block riding the boat went west with it.
    let crate_z = run.at("FAke_Block(2)").z;
    assert!(
        crate_z > 125.0,
        "the riding crate carried west: z {crate_z}"
    );
    assert!(
        behaviour(&run, "Push_Block_Template(2)").starts_with("1."),
        "at the boat's west end: {}",
        behaviour(&run, "Push_Block_Template(2)")
    );
    assert!(go_by(
        &mut run,
        &mut stops,
        &mut scenes,
        &mut trail,
        &[(40.0, 131.0), (31.5, 130.7)],
        900
    ));
    trail.mark(&run, "block-2");
    let off_west = shove(
        &mut run,
        &mut stops,
        &mut trail,
        "Push_Block_Template(2)",
        270.0,
        &|r| r.at("Push_Block_Template(2)").y < 18.5,
    );
    trail.mark(&run, "pushed-2");
    let west = run.at("Push_Block_Template(2)");
    println!("the block in the west channel at {west} in {off_west} moves");
    assert!(west.y < 18.5, "in the channel: {west}");
    assert!(go_by(
        &mut run,
        &mut stops,
        &mut scenes,
        &mut trail,
        &[(24.0, 130.7), (31.0, 130.7), (40.0, 131.0), (47.0, 133.0)],
        900
    ));
    turn(&mut run, &mut stops, &mut scenes, &mut trail, 180.0);
    assert!(go_by(
        &mut run,
        &mut stops,
        &mut scenes,
        &mut trail,
        &[(49.7, 116.0), (49.7, 111.0), (49.7, 102.5)],
        900
    ));
    trail.mark(&run, "block-7");
    let into = shove(
        &mut run,
        &mut stops,
        &mut trail,
        "Push_Block_Template(7)",
        180.0,
        &|r| behaviour(r, "Push_Block_Template(7)") == "special - push block inactive",
    );
    trail.mark(&run, "pushed-7");
    let south = run.at("Push_Block_Template(7)");
    println!(
        "the far block in the channel at {south} in {into} moves, step {}",
        run.steps
    );
    assert_eq!(
        behaviour(&run, "Push_Block_Template(7)"),
        "special - push block inactive"
    );

    // Setup_002's yard: its first Chompies fought as they come, the pirate
    // with the key off into the card house, its door swung open for him.
    let door = Vec3::new(39.5, 26.3, 53.5);
    let part_1 = &SETUP_002_FOES[..6];
    let yard_2 = fight_to(
        &mut run,
        &mut stops,
        "Setup_002_Controller",
        part_1,
        door,
        9000,
        &|r: &Run| {
            let me = r.hero().pivot;
            behaviour(r, "Fake_Card_Pirate_1") == "removed"
                && Vec3::new(me.x - door.x, 0.0, me.z - door.z).length() < 1.5
        },
    );
    scenes.extend(yard_2.scenes.iter().cloned());
    topups.extend(yard_2.topups.iter().copied());
    trail.0.extend(yard_2.trail.iter().copied());
    assert!(yard_2.ended, "to the card house: {:?}", yard_2.log);
    let woke = yard_2
        .log
        .iter()
        .find(|e| e.1 == "Setup_002_Controller" && e.2 == "wait")
        .map(|e| e.0);
    println!(
        "Setup_002 waiting from step {woke:?}; at the card house at step {}",
        run.steps
    );
    assert!(woke.is_some(), "Setup_002 woke: {:?}", yard_2.log);

    trail.mark(&run, "card-house");
    // Into the card house: Fangs' intro, his game (stood in won), his
    // outro, the gate before the key down.
    let mut stood_in = 0;
    for _ in 0..600 {
        trail.note(&run);
        if run.hero().pivot.y < -300.0 {
            break;
        }
        run.bench.view = (38.7f32 - run.hero().pivot.x)
            .atan2(51.2 - run.hero().pivot.z)
            .to_degrees();
        step(&mut run, &mut stops, Stick::new(0.0, 1.0), [false; 4]);
    }
    assert!(
        run.hero().pivot.y < -300.0,
        "in the card house: {}",
        run.hero().pivot
    );
    let by_fangs = Vec3::new(377.0, -304.4, 288.3);
    let mut lowered = false;
    for k in 0..6000u32 {
        trail.note(&run);
        if behaviour(&run, "Spike_Gate_Wooden_Template(2)").starts_with("spikes in down") {
            lowered = true;
            break;
        }
        if scene_step(&mut run, &mut stops, &mut scenes, &mut stood_in) {
            continue;
        }
        let me = run.hero().pivot;
        let (dx, dz) = (by_fangs.x - me.x, by_fangs.z - me.z);
        if dx.hypot(dz) < 1.0 {
            still(&mut run, &mut stops, [false, false, false, k % 20 == 10]);
        } else {
            run.bench.view = dx.atan2(dz).to_degrees();
            step(&mut run, &mut stops, Stick::new(0.0, 1.0), [false; 4]);
        }
    }
    println!(
        "the key's gate down {lowered} at step {}, stood in {stood_in}",
        run.steps
    );
    assert!(
        lowered,
        "the gate before the key: {}",
        behaviour(&run, "Spike_Gate_Wooden_Template(2)")
    );
    assert_eq!(stood_in, 1, "his game stood in once");
    assert_eq!(behaviour(&run, "Gate_Killer"), "removed");
    for scene in ["Casino Pirate Intro", "Casino Pirate Outro"] {
        assert!(
            scenes.iter().any(|(_, n)| n == scene),
            "{scene}: {scenes:?}"
        );
    }
    // The key, then out past the gate by the door.
    walk_to(
        &mut run,
        &mut stops,
        &mut scenes,
        &mut trail,
        Vec3::new(357.5, -304.3, 286.2),
        1.5,
    );
    for _ in 0..60 {
        still(&mut run, &mut stops, [false; 4]);
    }
    assert!(key_held(&run), "the card house's key taken");
    // The second gate killer goes with the key taken, and the door's gate
    // with it; it isn't listed: the walk is the same to the step
    // with it listed or not.
    assert_eq!(
        behaviour(&run, "Gate_Killer(1)"),
        "removed",
        "the door's gate let go"
    );
    walk_until(
        &mut run,
        &mut stops,
        &mut scenes,
        &mut trail,
        Vec3::new(405.0, -304.4, 278.8),
        1.5,
        &|r| r.hero().pivot.y > 0.0,
    );
    for _ in 0..900 {
        trail.note(&run);
        if run.hero().pivot.y > 0.0 {
            break;
        }
        let me = run.hero().pivot;
        run.bench.view = (407.2 - me.x).atan2(278.76 - me.z).to_degrees();
        step(&mut run, &mut stops, Stick::new(0.0, 1.0), [false; 4]);
    }
    for _ in 0..120 {
        still(&mut run, &mut stops, [false; 4]);
    }
    assert!(
        run.hero().pivot.y > 0.0,
        "out of the card house: {}",
        run.hero().pivot
    );
    assert!(key_held(&run), "the key carried out");
    println!(
        "out with the key at {} at step {}",
        run.hero().pivot,
        run.steps
    );

    // Back in the yard with the key: Setup_002's fight to its end.
    let (_, far) = run.placed("Setup_002_Trigger(1)");
    let fight_2 = fight(
        &mut run,
        &mut stops,
        "Setup_002_Controller",
        &SETUP_002_FOES,
        far,
        12000,
    );
    scenes.extend(fight_2.scenes.iter().cloned());
    topups.extend(fight_2.topups.iter().copied());
    trail.0.extend(fight_2.trail.iter().copied());
    for (step, n, b) in &fight_2.log {
        println!("  {step} {n} {b}");
    }
    println!("scenes {scenes:?}");
    println!(
        "over at step {}, ended {}; top-ups {topups:?}, lowest health {:?}; {:.1} s",
        run.steps,
        fight_2.ended,
        fight_2.lowest,
        started.elapsed().as_secs_f64()
    );
    assert!(
        fight_2.ended,
        "Setup_002's fight never ended: {:?}",
        fight_2.log
    );
    assert!(key_held(&run), "the card house's key carried on");
    AtSetup002sEnd {
        run,
        stops,
        scenes,
        trail,
        topups,
    }
}

/// Setup_003's foes, by its own letters: its heavy elves a to c, its
/// Cyclopes d to i, its lobbers j to l, whose end Cali waits on, then m to
/// o, which wait on the gate's blowing up.
const SETUP_003_FOES: [&str; 15] = [
    "Enemy_Elf_Heavy_S2(2)",
    "Enemy_Elf_Heavy_S2(3)",
    "Enemy_Elf_Heavy_S2(4)",
    "Enemy_Cyclops_Near(1)",
    "Enemy_Cyclops_Near(2)",
    "Enemy_Cyclops_Near(3)",
    "Enemy_Cyclops_Near(4)",
    "Enemy_Cyclops_Near(5)",
    "Enemy_Cyclops_Near(6)",
    "Enemy_Elf_Lobber(5)",
    "Enemy_Elf_Lobber(6)",
    "Enemy_Elf_Lobber(7)",
    "Enemy_Cyclops_Near(7)",
    "Enemy_Cyclops_Near(8)",
    "Enemy_Cyclops_Near(9)",
];

/// Where a hero turns the card house's key in the key gate west of the
/// crane (`Door_Template(6)`), on its east side.
const BY_THE_KEY_GATE: Vec3 = Vec3::new(4.5, 21.1, 133.0);

/// A placement's local `name` as an int (a float's whole part), if it has one.
fn local_int(run: &Run, name: &str, var: &str) -> Option<i32> {
    let id = run.vm.placement_of(run.placed(name).0)?;
    match run.vm.value(run.vm.local(id, var)?) {
        guac_script::Value::Int(i) => Some(i),
        guac_script::Value::Float(f) => Some(f as i32),
        _ => None,
    }
}

/// openspec AS.326: level 205 on from Setup_002's end, each thing on the
/// way worked by its own trigger. Carried on from AS.325's walk (the key in
/// hand, Setup_002's controller gone): the key gate lies past a channel the
/// crane's boat bridges only turned west, so he walks back up to its wheel
/// and turns it west, over the boat and the block in the west channel to
/// the gate, where △ turns the key and it opens; past it, in Setup_003's
/// combat area (its signal face, `CloseDoor`), the gate closes behind him,
/// the controller's foes come out of the house by the galleon and he fights
/// its twelve; with them off Cali's scene at the galleon plays to its end
/// and hands her back at her place, where she waits on the galleon pulled
/// in to the dock, which only a Giant's chain pull does (AS.371), and the
/// controller on the gate's blowing up after it. Nothing stops.
#[test]
#[ignore = "reads the Giants dump"]
fn level_205s_way_on_from_setup_002s_end_to_cali_at_the_galleon() {
    let started = std::time::Instant::now();
    let AtSetup002sEnd {
        mut run,
        mut stops,
        mut scenes,
        mut trail,
        mut topups,
    } = on_to_setup_002s_end();
    println!(
        "Setup_002's end at step {}, {:.1} s",
        run.steps,
        started.elapsed().as_secs_f64()
    );
    let key_held = |run: &Run| {
        run.world.clones().into_iter().any(|h| {
            run.world.name(h) == "Key Pickup"
                && run.world.actor(h).is_some_and(|a| !a.removed)
                && run
                    .vm
                    .placement_of(h)
                    .and_then(|i| run.vm.behaviour(i))
                    .is_some_and(|b| b == "grabbed by player")
        })
    };
    trail.mark(&run, "setup-002-end");

    // The boat lies south: no walk reaches the gate over the channel west
    // of the crane until it's turned west again.
    assert_eq!(crane(&run), 180.0, "the boat lies south");
    let (_, off) = way_to(&run, run.hero().pivot, BY_THE_KEY_GATE);
    println!("the key gate {off:.1} m off any walk with the boat south");
    assert!(off > 10.0, "the gate reached over the channel: {off}");
    walk_to(
        &mut run,
        &mut stops,
        &mut scenes,
        &mut trail,
        Vec3::new(47.0, 21.1, 133.0),
        1.5,
    );
    trail.mark(&run, "crane-west");
    assert!(
        go_by(
            &mut run,
            &mut stops,
            &mut scenes,
            &mut trail,
            &UP_TO_THE_WHEEL,
            900
        ),
        "at the wheel"
    );
    trail.mark(&run, "wheel-west");
    assert!(
        tap_until(&mut run, &mut stops, 900, &|r| crane(r) == 270.0
            && behaviour(r, "Crane(2)").starts_with("turned")),
        "the crane turned west: {}",
        crane(&run)
    );
    assert!(
        go_by(
            &mut run,
            &mut stops,
            &mut scenes,
            &mut trail,
            &DOWN_FROM_THE_WHEEL,
            900
        ),
        "down from the wheel"
    );
    trail.mark(&run, "down");
    println!("the boat west at step {}", run.steps);
    assert!(key_held(&run), "the key carried on");

    // Over the boat and the block in the west channel to the gate, which
    // reaches for the key: △ turns it and the gate opens.
    walk_to(
        &mut run,
        &mut stops,
        &mut scenes,
        &mut trail,
        BY_THE_KEY_GATE,
        1.5,
    );
    trail.mark(&run, "key-gate");
    assert!(
        tap_until(&mut run, &mut stops, 900, &|r| behaviour(
            r,
            "Door_Template(6)"
        ) == "done"),
        "the key gate open: {}",
        behaviour(&run, "Door_Template(6)")
    );
    assert!(!key_held(&run), "the key in its lock");
    println!("the key gate open at step {}", run.steps);
    trail.mark(&run, "key-gate-open");

    // Setup_003: in its combat area the gate closes behind him and its foes
    // come out of the house by the galleon; its twelve fought.
    let (_, s3) = run.placed("Setup_003_Trigger(1)");
    let twelve = &SETUP_003_FOES[..12];
    let fight_3 = fight_to(
        &mut run,
        &mut stops,
        "Setup_003_Controller",
        &SETUP_003_FOES,
        s3,
        12000,
        &|r: &Run| twelve.iter().all(|n| behaviour(r, n) == "removed"),
    );
    scenes.extend(fight_3.scenes.iter().cloned());
    topups.extend(fight_3.topups.iter().copied());
    trail.0.extend(fight_3.trail.iter().copied());
    for (step, n, b) in &fight_3.log {
        println!("  {step} {n} {b}");
    }
    assert!(fight_3.ended, "Setup_003's twelve: {:?}", fight_3.log);
    let came_out = fight_3
        .log
        .iter()
        .find(|e| e.1 == "Setup_003_Controller" && e.2.contains("exit the house part 1"))
        .map(|e| e.0);
    println!(
        "Setup_003's foes out of the house from step {came_out:?}; its twelve off at step {}",
        run.steps
    );
    assert!(
        came_out.is_some(),
        "its foes out of the house: {:?}",
        fight_3.log
    );
    assert_eq!(
        local_int(&run, "Door_Template(6)", "reclose"),
        Some(1),
        "the gate closed behind him"
    );

    // Cali's scene at the galleon, to its end; she's handed back at her
    // place, waiting on the galleon pulled in.
    let mut stood_in = 0;
    let (mut began, mut ended) = (None, None);
    for _ in 0..6000 {
        trail.note(&run);
        let playing = cutscene_name(&run).is_some_and(|n| n == "205 encounter at galleon");
        if playing && began.is_none() {
            began = Some(run.steps);
        }
        if began.is_some() && !playing && cutscene_name(&run).is_none() {
            ended = Some(run.steps);
            break;
        }
        if !scene_step(&mut run, &mut stops, &mut scenes, &mut stood_in) {
            still(&mut run, &mut stops, [false; 4]);
        }
    }
    println!("Cali's scene at the galleon from step {began:?} to {ended:?}");
    assert!(began.is_some(), "Cali's scene: {scenes:?}");
    assert!(ended.is_some(), "Cali's scene held: {:?}", scene_of(&run));
    for _ in 0..120 {
        still(&mut run, &mut stops, [false; 4]);
    }
    assert_eq!(
        behaviour(&run, "205_Encounter_At_Galleon_IGC"),
        "removed",
        "her scene over"
    );
    assert_ne!(behaviour(&run, "Cali"), "removed", "Cali handed back");
    let cali = run.at("Cali");
    println!(
        "Cali at {cali}, her state {:?}",
        local_int(&run, "Cali", "speakerinit")
    );
    assert_eq!(
        local_int(&run, "Cali", "speakerinit"),
        Some(4),
        "Cali waiting on the galleon pulled in"
    );
    assert_eq!(
        behaviour(&run, "Setup_003_Controller"),
        "check if gate has been blown up"
    );
    trail.mark(&run, "cali");
    walks_on(&mut run, &mut stops);
    println!(
        "over at step {}; top-ups {topups:?}, lowest health {:?}; scenes {scenes:?}; {:.1} s",
        run.steps,
        fight_3.lowest,
        started.elapsed().as_secs_f64()
    );
    let t: Vec<String> = trail
        .0
        .iter()
        .map(|p| format!("{:.2},{:.2},{:.2}", p.x, p.y, p.z))
        .collect();
    println!("TRAIL {}", t.join(";"));
    let m: Vec<String> = trail.1.iter().map(|(i, n)| format!("{i}:{n}")).collect();
    println!("MARKS {}", m.join(";"));
    assert!(stops.is_empty(), "{stops:#?}");
    println!("load at end: {}", load());
}
