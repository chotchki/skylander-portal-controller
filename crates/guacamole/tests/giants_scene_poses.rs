//! The people in scenes play the clips their scripts ask for (openspec
//! AS.442), headless over `GUAC_GIANTS_DUMP` on the TV's listing: each actor
//! by the clip's name in its model's palette, its time advancing over the
//! scene's steps and going round where the palette marks it looping. The
//! 214 outro's and the final island's are `giants_third_island`'s.
//!
//! `cargo test -p guacamole --profile disc --test giants_scene_poses -- --ignored --nocapture`

mod common;

use common::*;
use guac_script::Mode;

const LEVEL_214: &str = "level_214_tutotial_2";
const HUB_1: &str = "level_hub_2_stage_1";

/// Level 214's intro played through, no skip: Flynn's flight idle goes round
/// past its 1.07 s through the flight (the palette marks it looping; on 2.0
/// it played once and he stood in the air), Cali's idle goes round past its
/// 2.13 s, and both are read under the intro's name (AS.461: what a capture
/// names).
#[test]
#[ignore = "reads the Giants dump"]
fn level_214s_intro_plays_flynns_and_calis_clips_round() {
    println!("load at start: {}", load());
    let started = std::time::Instant::now();
    let (mut run, _) = run_on_level(
        Mode::Play,
        LEVEL_214,
        Setup {
            props: true,
            interface: true,
            posed_rigs: true,
            ..Setup::default()
        },
    );
    for _ in 0..2 {
        run.step_play().unwrap();
    }
    run.join().unwrap_or_else(|h| panic!("{h}"));
    let intro = run.placement("214_Intro_IGC");
    let mut watch = ClipWatch::default();
    let mut stops = Vec::new();
    while !run.vm.removed(intro) {
        assert!(run.steps < 4000, "the intro over");
        stops.extend(run.step_play().unwrap());
        watch.record(&run, &["Flynn_NPC", "Cali_NPC"]);
    }
    for ((actor, clip), w) in &watch.clips {
        println!("{actor} {clip}: {w:?}");
    }
    let fly = watch.get("Flynn_NPC", "Balloonist_FlyIdleStill.ma");
    assert!(fly.rounds >= 3, "his flight idle goes round: {fly:?}");
    assert!(fly.advanced > 3.0, "{fly:?}");
    assert_eq!(fly.scene.as_deref(), Some("214 Intro IGC"));
    // Her scene asks her talk, then her idle 11 steps on, which takes the
    // talk's place (one whole clip at a time) and goes round past 2.13 s.
    let idle = watch.get("Cali_NPC", "AdventureGirl_Idle.ma");
    assert!(
        idle.rounds >= 1 && idle.advanced > 4.3,
        "her idle: {idle:?}"
    );
    assert_eq!(idle.scene.as_deref(), Some("214 Intro IGC"));
    assert!(stops.is_empty(), "{stops:?}");
    println!(
        "{} steps, {:.1} s; load at end: {}",
        run.steps,
        started.elapsed().as_secs_f64(),
        load()
    );
}

/// Hub stage 1 (AS.442's places 5 and 6): Cali's intro, talked to on the
/// main deck, plays her talk on her through its talk scene (her scene tells
/// her, a placement of the hub's own file, whose palette under the tell read
/// nothing on 2.0, AS.80), round past its 7 s; then Auric's intro (`HUB_IGC_
/// Intro`) plays his talk and idle on him, and his walk, once he's greeted,
/// no longer plays (one whole clip at a time).
#[test]
#[ignore = "reads the Giants dump"]
fn hub_stage_1s_cali_and_auric_play_their_scenes_clips() {
    println!("load at start: {}", load());
    let started = std::time::Instant::now();
    let (mut run, _) = run_on_level(
        Mode::Play,
        HUB_1,
        Setup {
            interface: true,
            props: true,
            sounds: true,
            ..Setup::default()
        },
    );
    for _ in 0..2 {
        run.step_play().unwrap();
    }
    run.join().unwrap_or_else(|h| panic!("{h}"));
    let intro = run.placement("CS_Intro_Controller");
    let mut stops = Vec::new();
    while !run.vm.removed(intro) {
        assert!(run.steps < 3600, "the intro's end");
        stops.extend(run.step_play().unwrap());
    }
    let mut watch = ClipWatch::default();
    let mut auric_end = Vec::new();
    for at in [[0.10f32, -4.64, 47.5], [-10.30, -12.61, 16.91]] {
        run.put(guac_motion::seam::Vec3::from_array(at), 0.0);
        for _ in 0..10 {
            stops.extend(run.step_play().unwrap());
        }
        let mut began = false;
        let mut read = 0;
        for k in 0..3600u32 {
            run.bench.pads[0] = guacamole::script_engine::Pad {
                stick: guac_motion::stick::Stick::new(0.0, 0.0),
                actions: [false, false, false, !began && k % 20 == 0],
                connected: true,
            };
            stops.extend(run.step_play().unwrap());
            watch.record(&run, &["Cali", "CS_Auric"]);
            let up = guacamole::level_end::scene_up(&run.vm);
            if up.as_ref().is_some_and(|u| u.cutscene == "HUB_IGC_Intro") {
                let auric = run.placed("CS_Auric").0;
                auric_end = run.world.clips_on(auric);
            }
            began |= up.is_some();
            if let Some(p) = guacamole::prompt::up(&run.vm, &run.world) {
                read += 1;
                if read >= 90 {
                    read = 0;
                    let answered = run
                        .with_host(|vm, host| guacamole::prompt::answer(vm, host, &p, false))
                        .unwrap();
                    assert!(answered);
                }
            }
            if began && up.is_none() {
                break;
            }
        }
    }
    run.bench.pads[0].actions = [false; 4];
    for ((actor, clip), w) in &watch.clips {
        println!("{actor} {clip}: {w:?}");
    }
    let talk = watch.get("Cali", "AdventureGirl_Talkv00.ma");
    assert_eq!(talk.scene.as_deref(), Some("IGC_Cali_Intro"), "{talk:?}");
    assert!(
        talk.rounds >= 1 && talk.advanced > 7.0,
        "her talk: {talk:?}"
    );
    let speak = watch.get("CS_Auric", "Banker_SpeakStandingv00.ma");
    assert_eq!(speak.scene.as_deref(), Some("HUB_IGC_Intro"), "{speak:?}");
    assert!(speak.advanced > 3.0, "his talk: {speak:?}");
    let walk = watch.get("CS_Auric", "Banker_Walk.ma");
    assert!(walk.steps > 0, "he walks: {walk:?}");
    let names: Vec<Option<&str>> = auric_end.iter().map(|c| c.name.as_deref()).collect();
    println!("Auric at his intro's end: {names:?}");
    assert!(
        !names.contains(&Some("Banker_Walk.ma")),
        "his walk under his greet: {auric_end:?}"
    );
    assert!(stops.is_empty(), "{stops:?}");
    println!(
        "{} steps, {:.1} s; load at end: {}",
        run.steps,
        started.elapsed().as_secs_f64(),
        load()
    );
}
