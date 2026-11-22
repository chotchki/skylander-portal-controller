//! Level 203 (openspec AS.703), headless with guacamole's host on the TV's
//! listing for the level: Eruptor joined at its start, its intro played to
//! its end or skipped as a player skips it, then the stick his. Ignored by
//! default; they read `GUAC_GIANTS_DUMP` (process env, else `.env.dev`):
//!
//! `cargo test -p guacamole --profile disc --test giants_level_203 -- --ignored --nocapture`
//!
//! No recording of 203 exists: every step is the level's own scripts.

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
