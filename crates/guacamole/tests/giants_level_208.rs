//! Level 208 (openspec AS.343), headless with guacamole's host on the TV's
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
