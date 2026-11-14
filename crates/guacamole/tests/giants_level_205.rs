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
    note_card(run);
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
        if (run.steps - start) % 15 == 0 {
            fought.trail.push(run.hero().pivot);
        }
        if behaviour(run, controller) == "removed" {
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
        let swing = near.is_some() && far <= 5.0 && run.steps % 20 == 0;
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
