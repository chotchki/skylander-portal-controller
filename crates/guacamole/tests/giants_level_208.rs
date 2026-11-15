//! Level 208 (openspec AS.343, AS.344), headless with guacamole's host on the TV's
//! listing for the level: Eruptor joined at its start, its intro played to
//! its end or skipped as a player skips it, then the stick his. Ignored by
//! default; they read `GUAC_GIANTS_DUMP` (process env, else `.env.dev`):
//!
//! `cargo test -p guacamole --profile disc --test giants_level_208 -- --ignored --nocapture`
//!
//! No recording of 208 exists: every step is the level's own scripts.

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
    walks_on(&mut run, &mut stops);
    assert!(stops.is_empty(), "{stops:#?}");
    println!("load at end: {}", load());
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
