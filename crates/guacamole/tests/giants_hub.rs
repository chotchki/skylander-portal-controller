//! The hub between chapters (openspec AS.46, AS.47, AU.6), headless with
//! guacamole's host on the TV's listing for each hub stage. Ignored by
//! default; they read `GUAC_GIANTS_DUMP` (process env, else `.env.dev`):
//!
//! `cargo test -p guacamole --profile disc --test giants_hub -- --ignored --nocapture`

use guac_script::{Mode, Value};
use guac_server::saves::SaveValues;
use guacamole::story_save::{self, File, Layout, Live, Session};

mod common;

use common::*;

const HUB_1: &str = "level_hub_2_stage_1";

fn setup(scripts: &'static [&'static str]) -> Setup {
    Setup {
        interface: true,
        props: true,
        sounds: true,
        scripts,
        ..Setup::default()
    }
}

fn global(run: &Run, name: &str) -> Option<Value> {
    let g = run.vm.program().global_named(name)?;
    run.vm.global_peek(g).map(|v| run.vm.value(v))
}

fn zone(run: &Run) -> Option<Value> {
    let data = global(run, "level control data")?;
    run.vm
        .field(data, "atmospheric zone current")
        .map(|v| run.vm.value(v))
}

const STAGES: [(&str, &str); 4] = [
    ("level_hub_2_stage_1", "CS_Intro_Controller"),
    ("level_hub_2_stage_2", "CS_Hub_Intro_Stage2"),
    ("level_hub_2_stage_3", "CS_Hub_Intro_Stage3"),
    ("level_hub_2_stage_4", "CS_Hub_Intro_Stage4"),
];

/// Where every stage's intro hands the first player over: the first
/// waypoint of `CS_Hub_Intro_Final_Positions`, as each stage's file holds it.
const HANDED_OVER: [f32; 3] = [11.42, -12.62, 23.18];

/// The sky clones standing: each one's template's name, place, scale and
/// facing.
fn skies(run: &Run) -> Vec<(String, guac_motion::seam::Vec3, [f32; 3], f32)> {
    run.world
        .clones()
        .into_iter()
        .filter(|h| !run.world.removed(*h))
        .filter_map(|h| {
            let a = run.world.actor(h)?;
            let name = run.world.name(h).to_owned();
            let sky = name.to_ascii_lowercase().starts_with("skydome");
            sky.then_some((name, a.position, a.scale, a.angles.facing))
        })
        .collect()
}

/// openspec AS.47: every hub stage the resume can go to loads (stage 2
/// held no placement running `InitializeLevel.ai`, AS.50's "no start") and
/// starts at `CS_Hub_Intro_Final_Positions`' first waypoint; its intro, on
/// the TV's listing (`scripts::INTROS`), waits for the hero, plays once he's
/// in and its last scene hands him over at that same place; the stage's
/// sky is its controller's clone; nothing stops.
#[test]
#[ignore = "reads the Giants dump"]
fn every_hub_stage_starts_where_its_intro_hands_him_over() {
    let started = std::time::Instant::now();
    println!("load at start: {}", load());
    for (stage, intro) in STAGES {
        let (mut run, _found) = run_on_level(Mode::Play, stage, setup(&[]));
        let start = run.hero().pivot;
        let intro = run.placement(intro);
        let mut stops = Vec::new();
        for _ in 0..2 {
            stops.extend(run.step_play().unwrap());
        }
        assert!(run.vm.is_suspended(intro), "{stage}: held for a hero");
        assert_eq!(run.vm.behaviour(intro), None, "{stage}");
        run.join().unwrap_or_else(|h| panic!("{stage}: {h}"));
        let joined = run.steps;
        let (mut began, mut gone) = (None, None);
        while gone.is_none() {
            assert!(
                run.steps < joined + 3600,
                "{stage}: no hand-over by step {}",
                run.steps
            );
            stops.extend(run.step_play().unwrap());
            if began.is_none() && cutscene_name(&run).is_some() {
                began = Some((run.steps, cutscene_name(&run)));
            }
            if run.vm.removed(intro) {
                gone = Some(run.steps);
            }
        }
        for _ in 0..30 {
            stops.extend(run.step_play().unwrap());
        }
        let at = run.hero().pivot;
        println!(
            "{stage}: start {start:?}; joined at {joined}, the intro's cutscene {began:?}, the intro off at {gone:?}; handed over at {at:?}; Levels_Completed {:?}, zone {:?}; skies {:?}",
            global(&run, "Levels_Completed"),
            zone(&run),
            skies(&run)
        );
        let handed = guac_motion::seam::Vec3::from_array(HANDED_OVER);
        let flat = |v: guac_motion::seam::Vec3| guac_motion::seam::Vec3::new(v.x, 0.0, v.z);
        assert!(
            flat(start).distance(flat(handed)) < 0.01,
            "{stage}: the load's start {start:?}"
        );
        assert!(
            flat(at).distance(flat(handed)) < 0.05,
            "{stage}: handed over at {at:?}"
        );
        assert!(began.is_some(), "{stage}: its intro's cutscene plays");
        assert!(
            stops.is_empty(),
            "{stage}: {:#?}",
            stops.iter().map(ToString::to_string).collect::<Vec<_>>()
        );
    }
    println!(
        "{:.1} s; load at end: {}",
        started.elapsed().as_secs_f64(),
        load()
    );
}

/// openspec AS.46: hub stage 1 after level 214 (`Levels_Completed` 2): its
/// stage's controller has taken off what the story moved past, and its sky
/// is its controller's clone of `Skydome_201` at the controller's place, at
/// 400 % on every axis, still with no player near its height and turning
/// 0.015° a step once one stands there, its atmospheric zone 201 on the VM,
/// the sky's model off the props part drawn by AS.63's rule; none of the
/// other three skydomes cloned.
#[test]
#[ignore = "reads the Giants dump"]
fn hub_stage_1s_sky_is_its_controllers_skydome() {
    let (mut run, _found) = run_on_level(Mode::Play, HUB_1, setup(&[]));
    let mut stops = Vec::new();
    for _ in 0..2 {
        stops.extend(run.step_play().unwrap());
    }
    // The stage's controller at its STARTUP after 214 (`Levels_Completed`
    // 2): the snowmen and the pirates' lights always, then its loop's
    // passes 0 and 1 (its index counting from 0), pass 1 the ice and snow,
    // the turret and Dreadbeard; pass 2's Mabu repairers and the crane's
    // blocker stand, and the snow barrels are the intro's to take off.
    let off = |name: &str| run.world.removed(run.placed(name).0);
    for name in [
        "Snowman(1)",
        "Snowman(2)",
        "PirateLights",
        "PirateLights2",
        "Ice",
        "IcePatch_03(6)",
        "Turret",
        "Dreadbeard",
    ] {
        assert!(off(name), "{name} off");
    }
    for name in [
        "MabuRepairGuy1",
        "CraneDoorBlocker",
        "barrel_snow_intact(1)",
    ] {
        assert!(!off(name), "{name} stands");
    }
    let controller = run.at("Skydome_Controller_Stage1");
    let sky = skies(&run);
    println!(
        "Levels_Completed {:?}, zone {:?}, the controller at {controller:?}, skies {sky:?}",
        global(&run, "Levels_Completed"),
        zone(&run)
    );
    assert_eq!(global(&run, "Levels_Completed"), Some(Value::Int(2)));
    assert_eq!(zone(&run), Some(Value::Int(201)));
    assert_eq!(sky.len(), 1, "{sky:?}");
    let (name, at, scale, facing) = sky[0].clone();
    assert_eq!(name, "Skydome_201");
    assert!(at.distance(controller) < 0.01, "{at:?}");
    assert_eq!(scale, [400.0; 3]);
    // Still until a player's in reach of its height, then turning once the
    // intro's cutscene is over (its idle holds through a cutscene).
    for _ in 0..60 {
        stops.extend(run.step_play().unwrap());
    }
    assert_eq!(skies(&run)[0].3, facing, "turning with no player");
    run.join().unwrap_or_else(|h| panic!("{h}"));
    while cutscene_name(&run).is_some() || run.steps < 200 {
        assert!(run.steps < 3600, "the intro's end");
        stops.extend(run.step_play().unwrap());
    }
    let (_, _, _, from) = skies(&run)[0].clone();
    for _ in 0..60 {
        stops.extend(run.step_play().unwrap());
    }
    let (_, _, _, later) = skies(&run)[0].clone();
    assert!(
        ((from - later) - 60.0 * 0.015).abs() < 0.001,
        "turned {}° in 60 steps",
        from - later
    );
    // Its model draws by AS.63's rule, off the props part.
    let usr = dump().join("PS3_GAME/USRDIR");
    let bytes = guac_extract::scripts::entry(&usr, &format!("level/{HUB_1}.bld")).unwrap();
    let level = guac_formats::igz::Igz::parse(&bytes).unwrap();
    let props = level_props_of(&level, HUB_1);
    let drawn: Vec<&str> = props
        .spawn_models
        .iter()
        .map(|&i| props.models[i].path.as_str())
        .filter(|p| p.contains("HuB_SKY_Models"))
        .collect();
    println!("the skies' models its clones draw: {drawn:?}");
    assert!(
        drawn.iter().any(|p| p.ends_with("/sky_201.mdl")),
        "{drawn:?}"
    );
    assert!(stops.is_empty(), "{stops:#?}");
}

/// The pads with the talk button (△, the walking pad's fourth action) down
/// or up, the stick at rest.
fn talk_button(run: &mut Run, down: bool) {
    run.bench.pads[0] = guacamole::script_engine::Pad {
        stick: guac_motion::stick::Stick::new(0.0, 0.0),
        actions: [false, false, false, down],
        connected: true,
    };
}

/// A question asked on a talk: its string's name (the dialogue's id; its
/// line is never in the tree), a hash of the line the interface read for it
/// at run time (none unread), Yes first, and the answer given.
#[derive(Debug, Clone, PartialEq)]
struct Asked {
    id: Option<String>,
    line: Option<u64>,
    yes_first: bool,
    yes: bool,
}

/// What a talk came to: the cutscene it started, when it ended, each
/// question asked on the way with the answer given, and the steps the
/// portal rested under the game's pause (AS.125's review).
#[derive(Debug, Default)]
struct Talk {
    cutscene: Option<(u32, Option<String>)>,
    ended: Option<u32>,
    asked: Vec<Asked>,
    rested: Vec<u32>,
}

fn hash(line: &str) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    line.hash(&mut h);
    h.finish()
}

/// How long a question stays up before its answer, steps: a player reads
/// it, and the menu's own tick runs meanwhile (the au-chests review: its
/// focus stopped the UI on a colour's multiply 40 steps in, which an answer
/// on the first step never reached).
const READING: u32 = 90;

/// The hero set down at `at`, then the talk button pressed every 20 steps
/// until a cutscene starts, and the steps run until it ends (or the run
/// changes level), each question answered by `answer` (its string's name to
/// yes or no) as the TV's modal answers it ([`guacamole::prompt::answer`]),
/// [`READING`] steps on.
fn talk(
    run: &mut Run,
    stops: &mut Vec<guac_script::Halt>,
    at: guac_motion::seam::Vec3,
    answer: impl Fn(&str) -> bool,
) -> Talk {
    run.put(at, 0.0);
    for _ in 0..10 {
        stops.extend(run.step_play().unwrap());
    }
    let mut out = Talk::default();
    let mut read = 0;
    for k in 0..3600u32 {
        talk_button(run, out.cutscene.is_none() && k % 20 == 0);
        stops.extend(run.step_play().unwrap());
        if guacamole::scripts::portal_suspended(&run.vm) {
            out.rested.push(run.steps);
        }
        let cs = cutscene_name(run);
        if out.cutscene.is_none() && cs.is_some() {
            out.cutscene = Some((run.steps, cs.clone()));
        }
        if let Some(p) = guacamole::prompt::up(&run.vm, &run.world) {
            read += 1;
            if read < READING {
                continue;
            }
            read = 0;
            let id = match p.question {
                Value::Object(o) => run.vm.program().name_of(o).map(str::to_owned),
                _ => None,
            };
            let yes = answer(id.as_deref().unwrap_or_default());
            let answered = run
                .with_host(|vm, host| guacamole::prompt::answer(vm, host, &p, yes))
                .unwrap_or_else(|h| panic!("the answer: {h}"));
            assert!(answered, "{id:?}");
            // The line the modal draws is the one the interface reads for
            // that string, read again here at run time.
            assert_eq!(p.text, run.world.text_of(p.question), "{id:?}");
            out.asked.push(Asked {
                line: p.text.as_deref().filter(|t| !t.trim().is_empty()).map(hash),
                id,
                yes_first: p.yes_first,
                yes,
            });
        }
        if out.cutscene.is_some() && cs.is_none() {
            out.ended = Some(run.steps);
            break;
        }
        if run.world.level_change().is_some() {
            break;
        }
    }
    talk_button(run, false);
    out
}

/// Flynn at the helm and Cali on the main deck, where a player talks to
/// each (within their dialogues' active ranges, 25 ft and 15 ft).
const BY_FLYNN: [f32; 3] = [0.10, -4.64, 47.5];
const BY_CALI: [f32; 3] = [-10.30, -12.61, 16.91];

/// openspec AU.6: hub stage 1's way into the next chapter after level 214,
/// on the TV's listing (`scripts::HUB_SCRIPTS`): Flynn first sends the
/// player to Cali (a pan to her, no question, no level change); Cali's
/// intro asks her challenge question (`03_Cali_WantToTry?`, its line read),
/// answered no as the TV's modal answers, and ends, `Cali_Intro` 1; then
/// Flynn's outro asks his ready question (`201_Flynn_YES/NO_string`, its
/// line read and another), answered yes, and ends on `Change Level` set to
/// `Level_201`, which
/// the run asks for as the level switch would: level 201 by name, the
/// first chapter the hub offers after 214. Nothing stops.
#[test]
#[ignore = "reads the Giants dump"]
fn hub_stage_1s_way_on_is_cali_then_flynn_into_level_201() {
    let started = std::time::Instant::now();
    println!("load at start: {}", load());
    let (mut run, _found) = run_on_level(Mode::Play, HUB_1, setup(&[]));
    let mut stops = Vec::new();
    for _ in 0..2 {
        stops.extend(run.step_play().unwrap());
    }
    run.join().unwrap_or_else(|h| panic!("{h}"));
    let intro = run.placement("CS_Intro_Controller");
    while !run.vm.removed(intro) {
        assert!(run.steps < 3600, "the intro's end");
        stops.extend(run.step_play().unwrap());
    }
    assert_eq!(global(&run, "Cali_Intro"), Some(Value::Int(0)));
    // openspec AU.28, AU.34: the people the TV draws, Flynn and Cali among
    // them, their own scripts running, unwalked, each on a model of the
    // level's creatures. A template among them runs nothing and draws
    // nothing.
    let skinned: Vec<String> = skinned_models_of(HUB_1)
        .into_iter()
        .map(|(path, _)| path)
        .collect();
    let mut people: Vec<(String, bool, Option<String>)> = run
        .world
        .people()
        .into_iter()
        .map(|h| {
            let name = run.world.name(h).to_owned();
            let model = run
                .files
                .placed
                .iter()
                .find(|p| p.obj.unit == LEVEL && p.name == name)
                .and_then(|p| p.model.clone());
            (name, run.vm.placement_of(h).is_some(), model)
        })
        .collect();
    people.sort();
    println!("the people: {people:?}; the creature models: {skinned:?}");
    let running: Vec<_> = people.iter().filter(|p| p.1).collect();
    for name in ["Cali", "Flynn"] {
        assert!(running.iter().any(|p| p.0 == name), "{name}'s script runs");
    }
    for (name, _, model) in running {
        let model = model.as_ref().expect("a model");
        assert!(skinned.contains(model), "{name}'s {model}");
    }
    let v = guac_motion::seam::Vec3::from_array;
    let first = talk(&mut run, &mut stops, v(BY_FLYNN), |_| true);
    println!(
        "Flynn first: {first:?}; Cali_Intro {:?}",
        global(&run, "Cali_Intro")
    );
    assert!(
        first.cutscene.is_some() && first.ended.is_some(),
        "{first:?}"
    );
    assert!(first.asked.is_empty(), "{first:?}");
    assert!(run.world.level_change().is_none());
    let cali = talk(&mut run, &mut stops, v(BY_CALI), |_| false);
    println!(
        "Cali: {cali:?}; Cali_Intro {:?}",
        global(&run, "Cali_Intro")
    );
    assert_eq!(
        cali.cutscene.as_ref().and_then(|c| c.1.as_deref()),
        Some("IGC_Cali_Intro")
    );
    assert!(cali.ended.is_some(), "{cali:?}");
    let [asked] = cali.asked.as_slice() else {
        panic!("one question: {cali:?}");
    };
    assert_eq!(asked.id.as_deref(), Some("03_Cali_WantToTry?"), "{cali:?}");
    assert!(asked.line.is_some(), "its line read: {cali:?}");
    assert!(asked.yes_first && !asked.yes, "{cali:?}");
    let cali_line = asked.line;
    assert_eq!(global(&run, "Cali_Intro"), Some(Value::Int(1)));
    assert!(run.world.level_change().is_none());
    let flynn = talk(&mut run, &mut stops, v(BY_FLYNN), |_| true);
    let change = run.world.level_change().cloned();
    println!(
        "Flynn: {flynn:?}; Change Level {:?}; the level change {change:?}",
        global(&run, "Change Level")
    );
    assert_eq!(
        flynn.cutscene.as_ref().and_then(|c| c.1.as_deref()),
        Some("HS2 - Outro")
    );
    let [asked] = flynn.asked.as_slice() else {
        panic!("one question: {flynn:?}");
    };
    assert_eq!(
        asked.id.as_deref(),
        Some("201_Flynn_YES/NO_string"),
        "{flynn:?}"
    );
    assert!(asked.line.is_some(), "its line read: {flynn:?}");
    assert_ne!(asked.line, cali_line, "a line of its own");
    assert!(asked.yes_first && asked.yes, "{flynn:?}");
    let change = change.expect("a level change");
    assert_eq!(change.name, "level_201");
    assert!(
        stops.is_empty(),
        "{:#?}",
        stops.iter().map(ToString::to_string).collect::<Vec<_>>()
    );
    println!(
        "{:.1} s; load at end: {}",
        started.elapsed().as_secs_f64(),
        load()
    );
}

/// The AS.125 review: Cali's challenge question answered yes by one player
/// asks for her challenges' screen, which the TV answers with nothing
/// shown and a line in the log (AS.453, once), then pauses the game at 0 until that screen's stream reads
/// unloaded, so the portal rests meanwhile: a step, the stream never asked
/// for, and the portal answers again, never left deaf on the hub. Nothing
/// stops.
#[test]
#[ignore = "reads the Giants dump"]
fn calis_yes_leaves_the_portal_answering() {
    use guacamole::scripts::portal_suspended;
    println!("load at start: {}", load());
    let (mut run, _found) = run_on_level(Mode::Play, HUB_1, setup(&[]));
    let mut stops = Vec::new();
    for _ in 0..2 {
        stops.extend(run.step_play().unwrap());
    }
    run.join().unwrap_or_else(|h| panic!("{h}"));
    let intro = run.placement("CS_Intro_Controller");
    while !run.vm.removed(intro) {
        assert!(run.steps < 3600, "the intro's end");
        stops.extend(run.step_play().unwrap());
    }
    assert!(!portal_suspended(&run.vm), "the portal answers in play");
    let v = guac_motion::seam::Vec3::from_array;
    talk(&mut run, &mut stops, v(BY_FLYNN), |_| true);
    let cali = talk(&mut run, &mut stops, v(BY_CALI), |_| true);
    let [asked] = cali.asked.as_slice() else {
        panic!("one question: {cali:?}");
    };
    assert_eq!(asked.id.as_deref(), Some("03_Cali_WantToTry?"), "{cali:?}");
    assert!(asked.yes, "{cali:?}");
    let mut rested = cali.rested.clone();
    for _ in 0..600 {
        stops.extend(run.step_play().unwrap());
        if portal_suspended(&run.vm) {
            rested.push(run.steps);
        }
    }
    println!(
        "Cali: {cali:?}; the portal rested {} steps ({:?}..{:?})",
        rested.len(),
        rested.first(),
        rested.last()
    );
    assert!(!rested.is_empty(), "her yes paused the game");
    // openspec AS.453: her yes asked for the Heroic Challenges, logged as
    // not built, once (Flynn's yes asked for no screen).
    assert_eq!(
        run.world.unbuilt,
        [guacamole::prompt::UNBUILT[2].1],
        "what her yes asked for"
    );
    assert!(!portal_suspended(&run.vm), "the portal answers again");
    assert!(rested.len() < 60, "rested {} steps", rested.len());
    // The rest her pause made went with it (openspec AS.205).
    let vm_resting = (0u32..)
        .map_while(|id| run.vm.handle_of(id).map(|_| id))
        .filter(|&id| run.vm.resting(id))
        .count();
    assert_eq!(
        (run.world.resting_count(), vm_resting),
        (0, 0),
        "the hub still rests"
    );
    assert!(
        stops.is_empty(),
        "{:#?}",
        stops.iter().map(ToString::to_string).collect::<Vec<_>>()
    );
    println!("load at end: {}", load());
}

/// The object the program names `name`, on any of its units.
fn object_named(run: &Run, name: &str) -> Option<Value> {
    (0..8u16)
        .flat_map(|unit| (0..400_000u32).map(move |index| guac_script::Obj { unit, index }))
        .find(|o| run.vm.program().name_of(*o) == Some(name))
        .map(Value::Object)
}

/// openspec AU.27: level 214's end into the hub, its outro's level end
/// driven directly (214's end isn't reachable on foot yet): the original's
/// `Game - Level End` run on the game systems' placement naming the village
/// (`999_Level_Village`, what `214_Outro_IGC` names) is asked as the
/// level end asks it and resolved as the Pause Controller's switch resolves
/// it, to the hub stage one past the village unlocks done: stage 1 after
/// 214, stage 2 with level 210's adventure marked complete first. Nothing
/// stops.
#[test]
#[ignore = "reads the Giants dump"]
fn level_214s_end_takes_the_hero_to_the_stage_the_village_opens() {
    let started = std::time::Instant::now();
    println!("load at start: {}", load());
    for (done, stage) in [(0, HUB_1), (1, "level_hub_2_stage_2")] {
        let (mut run, _found) = run_on_level(Mode::Play, "level_214_tutotial_2", setup(&[]));
        let mut stops = Vec::new();
        for _ in 0..2 {
            stops.extend(run.step_play().unwrap());
        }
        run.join().unwrap_or_else(|h| panic!("{h}"));
        stops.extend(skip_214s_intro(&mut run));
        let village = object_named(&run, "999_Level_Village").expect("the village's string");
        let unlocks: Vec<Value> = global(&run, "level progression data")
            .and_then(|d| run.vm.field(d, "village unlocks"))
            .and_then(|s| run.vm.set(s).map(<[_]>::to_vec))
            .expect("the village unlocks");
        assert_eq!(unlocks.len(), 3, "levels 210, 202 and 211");
        let titles: Vec<Option<String>> = unlocks
            .iter()
            .map(|ad| {
                let t = run.vm.field(*ad, "title").map(|t| run.vm.value(t));
                match t {
                    Some(Value::Object(o)) => run.vm.program().name_of(o).map(str::to_owned),
                    _ => None,
                }
            })
            .collect();
        println!("the village unlocks: {titles:?}");
        for ad in unlocks.iter().take(done) {
            assert!(run.vm.set_field(*ad, "complete", Value::Int(2)));
        }
        let gs = run
            .with_host(|vm, host| guacamole::scripts::running(vm, host.world, "GameSystems.ai"))
            .expect("the game systems");
        run.with_host(|vm, host| {
            vm.call_for(
                gs,
                "Game - Level End",
                &[("load to level", village)],
                &[],
                host,
            )
        })
        .unwrap_or_else(|h| panic!("the level end: {h}"));
        let asked = run.steps;
        while run.world.level_change().is_none() {
            assert!(
                run.steps < asked + 300,
                "no level change by step {}",
                run.steps
            );
            stops.extend(run.step_play().unwrap());
        }
        let change = run.world.level_change().cloned().unwrap();
        let end = run.world.level_end().cloned();
        println!(
            "{done} done: asked at {asked}, changed at {}: {change:?}; the level end {end:?}",
            run.steps
        );
        assert_eq!(change.name, stage, "{done} done");
        let end = end.expect("the level end");
        assert_eq!(end.string.as_deref(), Some("999_Level_Village"));
        assert_eq!(end.value, village);
        assert!(
            stops.is_empty(),
            "{:#?}",
            stops.iter().map(ToString::to_string).collect::<Vec<_>>()
        );
    }
    println!(
        "{:.1} s; load at end: {}",
        started.elapsed().as_secs_f64(),
        load()
    );
}

/// The stick held so he walks toward `to` as the view lies.
fn stick_toward(run: &Run, to: guac_motion::seam::Vec3) -> guac_motion::stick::Stick {
    let at = run.hero().pivot;
    let want = (to.x - at.x).atan2(to.z - at.z).to_degrees();
    let d = (want - run.bench.view).to_radians();
    let raw = |a: f32| {
        if a.abs() < 1e-4 {
            0.0
        } else {
            a.signum()
                * (guac_motion::stick::DEAD_ZONE + (1.0 - guac_motion::stick::DEAD_ZONE) * a.abs())
        }
    };
    guac_motion::stick::Stick::new(raw(-d.sin()), raw(d.cos()))
}

/// The hero set down `from` and held walking straight at `name` for 3 s:
/// how near across the ground his pivot came, and whether he came out the
/// far side.
fn walk_into(
    run: &mut Run,
    stops: &mut Vec<guac_script::Halt>,
    name: &str,
    from: guac_motion::seam::Vec3,
) -> (f32, bool) {
    run.put(from, 0.0);
    let them = run.at(name);
    let flat = |v: guac_motion::seam::Vec3| guac_motion::seam::Vec3::new(v.x, 0.0, v.z);
    let start = flat(them) - flat(from);
    let mut closest = start.length();
    for _ in 0..180 {
        let stick = stick_toward(run, them);
        run.bench.pads[0] = guacamole::script_engine::Pad {
            stick,
            actions: [false; 4],
            connected: true,
        };
        stops.extend(run.step_play().unwrap());
        closest = closest.min(flat(them).distance(flat(run.hero().pivot)));
    }
    run.bench.pads[0].stick = guac_motion::stick::Stick::new(0.0, 0.0);
    let end = flat(them) - flat(run.hero().pivot);
    (closest, end.dot(start) < 0.0)
}

/// Who should be on each stage at its floor (`Levels_Completed` 2, 7, 14,
/// 16), and who has taken itself off by then.
const STAGE_PEOPLE: [(&[&str], &[&str]); 4] = [
    (
        &[
            "Flynn",
            "Cali",
            "Persephone",
            "Auric",
            "CS_Auric",
            "Hatterson",
            "Breakstone",
            "Background_Fairy",
            "MabuRepairGuy1",
            "MabuRepairGuy2",
            "MabuRepairGuy3",
            "MabuWalker1",
            "MabuWalker2",
        ],
        &["Ermit", "Brock"],
    ),
    (
        &[
            "Flynn",
            "Cali",
            "Persephone",
            "Auric",
            "Hatterson",
            "Dreadbeard",
            "Ermit",
            "Brock",
            "Wilikin_Girl_01",
            "Background_Fairy",
        ],
        &["Mabu_Repairer", "Barnsy"],
    ),
    (
        &[
            "Flynn",
            "Cali",
            "Persephone",
            "Auric",
            "Hatterson",
            "Dreadbeard",
            "Ermit",
            "Brock",
        ],
        &[],
    ),
    (
        &[
            "Flynn",
            "Cali",
            "Persephone",
            "Auric",
            "Hatterson",
            "Dreadbeard",
            "Ermit",
            "Brock",
        ],
        &[],
    ),
];

/// openspec AU.34: every hub stage with its people on the TV's listing:
/// each stage's stand where their scripts stand them, drawn on models of the
/// level's and solid where their scripts say, those a stage's count takes
/// off gone; a talk with each that has one ends, the store's yes shows
/// nothing and takes nothing from the next talk, Persephone's upgrade menu
/// backs out to its no, and Flynn still talks after them all. The hero
/// walking at Flynn and at Cali stops on them. Nothing stops.
#[test]
#[ignore = "reads the Giants dump"]
fn every_hub_stages_people_stand_talk_and_stop_the_hero() {
    let started = std::time::Instant::now();
    println!("load at start: {}", load());
    for ((stage, intro), (here, gone)) in STAGES.into_iter().zip(STAGE_PEOPLE) {
        let (mut run, _found) = run_on_level(Mode::Play, stage, setup(&[]));
        let mut stops = Vec::new();
        for _ in 0..2 {
            stops.extend(run.step_play().unwrap());
        }
        run.join().unwrap_or_else(|h| panic!("{stage}: {h}"));
        let intro = run.placement(intro);
        while !run.vm.removed(intro) {
            assert!(run.steps < 3600, "{stage}: the intro's end");
            stops.extend(run.step_play().unwrap());
        }
        for _ in 0..30 {
            stops.extend(run.step_play().unwrap());
        }
        let skinned: Vec<String> = skinned_models_of(stage)
            .into_iter()
            .map(|(path, _)| path)
            .collect();
        let placed = |run: &Run, name: &str| {
            run.files
                .placed
                .iter()
                .find(|p| p.obj.unit == LEVEL && p.name == name)
                .unwrap_or_else(|| panic!("{stage}: {name}'s placement"))
                .clone()
        };
        for name in here {
            let (h, _) = run.placed(name);
            let id = run.vm.placement_of(h).expect("run");
            assert!(!run.vm.removed(id), "{stage}: {name} is here");
            assert!(run.world.people().contains(&h), "{stage}: {name}");
            assert!(!run.vm.inert().contains(&h), "{stage}: {name} runs");
            let model = placed(&run, name).model.expect("a model");
            assert!(skinned.contains(&model), "{stage}: {name}'s {model}");
        }
        for name in gone {
            let id = run.placement(name);
            assert!(run.vm.removed(id), "{stage}: {name} took itself off");
        }
        // Every stage's utilities controller takes him off at its STARTUP,
        // and it's off the TV's listing (AS.138): no person of the TV's.
        let (orbit, _) = run.placed("Hatterson(Orbit)");
        assert!(!run.world.people().contains(&orbit), "{stage}: the orbit's");
        // A talk with each that has one, the stores and the arena said yes.
        let mut talks = Vec::new();
        for (name, yes) in [
            ("Auric", true),
            ("Hatterson", true),
            ("Persephone", false),
            ("Brock", true),
            ("Ermit", false),
            ("Dreadbeard", true),
            ("Cali", true),
        ] {
            if !here.contains(&name) {
                continue;
            }
            let at = guac_motion::seam::Vec3::from_array(placed(&run, name).position);
            let t = talk(
                &mut run,
                &mut stops,
                at + guac_motion::seam::Vec3::new(1.5, 0.5, 1.5),
                |_| yes,
            );
            println!("{stage}: {name}: {t:?}");
            assert!(t.cutscene.is_some(), "{stage}: {name} talks: {t:?}");
            assert!(t.ended.is_some(), "{stage}: {name}'s talk ends: {t:?}");
            talks.push(t);
            // The card game's yes reads as a game quit: his quit scene, and
            // his talk back (a game left waiting on, it never came).
            if name == "Dreadbeard" {
                let id = run.placement(name);
                for yes in [true, false] {
                    while run.vm.behaviour(id) != Some("wait for player") {
                        assert!(run.steps < 20_000, "{stage}: the game's quit");
                        stops.extend(run.step_play().unwrap());
                    }
                    let again = talk(
                        &mut run,
                        &mut stops,
                        at + guac_motion::seam::Vec3::new(1.5, 0.5, 1.5),
                        |_| yes,
                    );
                    println!("{stage}: {name} again: {again:?}");
                    assert!(again.cutscene.is_some(), "{stage}: {again:?}");
                    assert!(again.ended.is_some(), "{stage}: {again:?}");
                }
                // The card game's streams link beside every stage (openspec
                // AS.364), and the TV's quit asks none of them: no game is
                // counted won in play (AS.364's review).
                assert_eq!(
                    run.world.card_streams.units.len(),
                    guacamole::card_game::STREAMS.len() + guacamole::card_game::stones().len(),
                    "{stage}"
                );
                assert!(
                    run.world.card_streams.joined.is_empty(),
                    "{stage}: {:?}",
                    run.world.card_streams.joined
                );
            }
        }
        let branch = global(&run, "CutsceneMenu_LastPickedBranch");
        println!("{stage}: the last menu's branch {branch:?}");
        // The stores' yes took nothing from the talk button.
        let by_flynn = run.at("Flynn") + guac_motion::seam::Vec3::new(1.5, 0.5, -2.5);
        let flynn = talk(&mut run, &mut stops, by_flynn, |_| false);
        println!("{stage}: Flynn after them: {flynn:?}");
        assert!(flynn.cutscene.is_some(), "{stage}: {flynn:?}");
        assert!(flynn.ended.is_some(), "{stage}: {flynn:?}");
        // His solid and Cali's in the bench, and each stops him.
        for (name, from) in [
            ("Flynn", guac_motion::seam::Vec3::new(0.0, 0.0, -6.0)),
            ("Cali", guac_motion::seam::Vec3::new(6.0, 0.0, 0.0)),
        ] {
            let (h, _) = run.placed(name);
            assert!(
                run.world.creature_bodies().iter().any(|b| b.0 == h),
                "{stage}: {name}'s solid"
            );
            let at = run.at(name) + from;
            let (closest, through) = walk_into(&mut run, &mut stops, name, at);
            println!(
                "{stage}: into {name}: {closest:.3} m at the closest, through {through}, from {at:?} to {:?}, cs {:?}",
                run.hero().pivot,
                cutscene_name(&run)
            );
            assert!(!through, "{stage}: through {name}");
            assert!(closest > 1.0, "{stage}: {closest} m from {name}");
        }
        assert!(stops.is_empty(), "{stage}: {stops:#?}");
    }
    println!(
        "{:.1} s, load at end: {}",
        started.elapsed().as_secs_f64(),
        load()
    );
}

/// Each stage and the story adventures done it can stand at (openspec
/// AS.130): the hub's unlocks open a stage at 7, 14 and 16.
const COUNTS: [(usize, std::ops::RangeInclusive<usize>); 4] =
    [(0, 2..=6), (1, 7..=13), (2, 14..=15), (3, 16..=16)];

/// The exterior zone the definition picks at each count of adventures done
/// (AS.130's runs): the level last done's, as its sky out past the ship.
fn exterior_at(done: usize) -> i32 {
    match done {
        2 => 201,
        3 => 205,
        4..=6 => 208,
        7..=8 => 206,
        9..=10 => 203,
        11..=13 => 200,
        14..=15 => 202,
        _ => 208,
    }
}

/// The first `done` story adventures marked complete on `vm`, as the title's
/// fill leaves them, the last at 2 when `just` (a level end just marked it).
fn mark(vm: &mut guac_script::Vm, done: usize, just: bool) {
    let g = vm.program().global_named("level progression data").unwrap();
    let progression = vm.global(g).unwrap();
    let set = vm.field(progression, "story adventures").unwrap();
    let adventures = vm.set(set).unwrap().to_vec();
    assert!(adventures.len() >= done, "{}", adventures.len());
    for (i, ad) in adventures.iter().take(done).enumerate() {
        let complete = if just && i + 1 == done { 2 } else { 1 };
        vm.set_field(vm.value(*ad), "complete", Value::Int(complete));
    }
}

/// What a stage came to at a count.
#[derive(Debug)]
struct AtCount {
    completed: Option<Value>,
    first_time: Option<Value>,
    zone: Option<Value>,
    sky: Vec<String>,
    began: Option<(u32, Option<String>)>,
    gone: Option<u32>,
    at: guac_motion::seam::Vec3,
    stops: Vec<String>,
}

/// `stage` run with `done` adventures done ahead of its definition: joined,
/// its intro played to its hand-over, 30 steps on.
fn at_count(stage: usize, done: usize, just: bool, scripts: &'static [&'static str]) -> AtCount {
    let (name, intro) = STAGES[stage];
    let (mut run, _found) = run_on_level(Mode::Play, name, setup(scripts));
    let intro = run.placement(intro);
    let definition = guacamole::story_save::definition(&run.vm, &run.world).expect("a definition");
    run.vm.native_first(definition);
    let mut stops: Vec<_> = run
        .step_play_with(&mut |vm, id, _| {
            if id == definition {
                mark(vm, done, just);
            }
            Ok(())
        })
        .unwrap();
    stops.extend(run.step_play().unwrap());
    run.join().unwrap_or_else(|h| panic!("{name}: {h}"));
    let joined = run.steps;
    let (mut began, mut gone) = (None, None);
    while gone.is_none() && run.steps < joined + 18000 {
        stops.extend(run.step_play().unwrap());
        if began.is_none() && cutscene_name(&run).is_some() {
            began = Some((run.steps, cutscene_name(&run)));
        }
        if run.vm.removed(intro) {
            gone = Some(run.steps);
        }
    }
    for _ in 0..30 {
        stops.extend(run.step_play().unwrap());
    }
    AtCount {
        completed: global(&run, "Levels_Completed"),
        first_time: global(&run, "First_Time_Here"),
        zone: zone(&run),
        sky: skies(&run).into_iter().map(|s| s.0).collect(),
        began,
        gone,
        at: run.hero().pivot,
        stops: stops.iter().map(ToString::to_string).collect(),
    }
}

/// openspec AS.130: `stage` at every count its saves can hold, the last
/// adventure just done and not: the definition counts what the save holds,
/// the intro plays and hands him over where every stage does, the exterior
/// zone is the one the definition picks for the count, nothing stops.
fn stage_at_every_count(stage: usize) {
    let (_, counts) = &COUNTS[stage];
    let name = STAGES[stage].0;
    println!("load at start: {}", load());
    let handed = guac_motion::seam::Vec3::from_array(HANDED_OVER);
    let flat = |v: guac_motion::seam::Vec3| guac_motion::seam::Vec3::new(v.x, 0.0, v.z);
    for done in counts.clone() {
        for just in [false, true] {
            let c = at_count(stage, done, just, &[]);
            println!("{name} at {done}, just {just}: {c:?}");
            assert_eq!(c.completed, Some(Value::Int(done as i32)), "{name} {done}");
            assert_eq!(
                c.first_time,
                Some(Value::Int(i32::from(just))),
                "{name} {done}: the scene for the adventure just done"
            );
            assert!(c.began.is_some(), "{name} {done}: no intro");
            assert_eq!(c.zone, Some(Value::Int(exterior_at(done))), "{name} {done}");
            assert!(
                c.sky.len() == 1 && c.sky[0].ends_with(&exterior_at(done).to_string()),
                "{name} {done}: the zone's sky alone: {:?}",
                c.sky
            );
            assert!(c.gone.is_some(), "{name} {done}: no hand-over");
            assert!(
                flat(c.at).distance(flat(handed)) < 0.05,
                "{name} {done}: handed over at {:?}",
                c.at
            );
            assert!(c.stops.is_empty(), "{name} {done}: {:#?}", c.stops);
        }
    }
    println!("load at end: {}", load());
}

#[test]
#[ignore = "reads the Giants dump"]
fn hub_stage_1_at_every_count() {
    stage_at_every_count(0);
}

#[test]
#[ignore = "reads the Giants dump"]
fn hub_stage_2_at_every_count() {
    stage_at_every_count(1);
}

#[test]
#[ignore = "reads the Giants dump"]
fn hub_stage_3_at_every_count() {
    stage_at_every_count(2);
}

#[test]
#[ignore = "reads the Giants dump"]
fn hub_stage_4_at_every_count() {
    stage_at_every_count(3);
}

/// openspec AS.130: a story scene's movie: the Hermit's scene after level 201
/// reaches its movie scene and hands the cinematic to the TV's player by the
/// disc's name, waits there while it plays, and goes on the step it's over,
/// its fader cleaned up; with no player (every other test here) it passes
/// straight on.
#[test]
#[ignore = "reads the Giants dump"]
fn the_hermits_scene_waits_on_its_movie() {
    use guacamole::scripts::{MovieEnd, MovieState};
    println!("load at start: {}", load());
    let (mut run, _found) = run_on_level(Mode::Play, HUB_1, setup(&[]));
    run.world.plays_movies = true;
    let definition = story_save::definition(&run.vm, &run.world).expect("a definition");
    run.vm.native_first(definition);
    let mut stops = run
        .step_play_with(&mut |vm, id, _| {
            if id == definition {
                mark(vm, 3, true);
            }
            Ok(())
        })
        .unwrap();
    stops.extend(run.step_play().unwrap());
    run.join().unwrap_or_else(|h| panic!("{h}"));
    let mut waited = 0;
    while run.world.level_end_movie().is_none() {
        stops.extend(run.step_play().unwrap());
        waited += 1;
        assert!(waited < 6000, "no movie by step {}", run.steps);
    }
    let scene = scene_of(&run);
    let movie = run.world.level_end_movie().unwrap();
    println!("{movie:?} at {scene:?} after {waited} steps");
    assert_eq!(movie.name, "fmv03");
    assert_eq!(movie.state, MovieState::Queued);
    for _ in 0..300 {
        stops.extend(run.step_play().unwrap());
    }
    assert_eq!(scene_of(&run), scene, "held on it while it plays");
    // The AS.125 review: the movie's pause rests the portal while it plays
    // and lifts with it, so the hub's portal is never left deaf.
    let resting = guacamole::scripts::portal_suspended(&run.vm);
    run.world.movie_over(MovieEnd::Finished);
    for _ in 0..60 {
        stops.extend(run.step_play().unwrap());
    }
    assert_ne!(scene_of(&run), scene, "on past it");
    assert!(run.world.level_end_movie().is_none(), "over and let go");
    println!(
        "the portal rested under the movie: {resting}; after it: {}",
        guacamole::scripts::portal_suspended(&run.vm)
    );
    assert!(
        !guacamole::scripts::portal_suspended(&run.vm),
        "the portal answers after it"
    );
    println!("load at end: {}", load());
    assert!(stops.is_empty(), "{stops:#?}");
}

/// The save words and values a fresh `HUB_1` run's scripts define, with the
/// first `done` adventures of its list marked done.
fn save_with(done: usize) -> (Layout, Vec<i32>, Vec<i32>, SaveValues) {
    let (mut run, _found) = run_on_level(Mode::Play, HUB_1, setup(&[]));
    for _ in 0..2 {
        run.step_play().unwrap();
    }
    let layout = Layout::read(&run.vm).expect("the save's layout");
    let (mut slot, header) = (
        story_save::words_of(&run.vm, File::Slot).unwrap(),
        story_save::words_of(&run.vm, File::Header).unwrap(),
    );
    let mut params = layout.decode(File::Slot, &slot);
    params.extend(layout.decode(File::Header, &header));
    for flag in params.get_mut("level flags").unwrap().iter_mut().take(done) {
        *flag |= 1;
    }
    layout.encode(File::Slot, &params, &mut slot);
    let values = SaveValues {
        params,
        adventures: layout.adventures.clone(),
    };
    (layout, slot, header, values)
}

/// `session`'s run on `HUB_1` to its first step, the title's part in its
/// definition's place as the TV's first walk makes it, then the adventures
/// at `just` marked just done there, as a `test-hooks` build's
/// `GUAC_TEST_JUST_DONE` marks them on a resume.
fn titled_step(session: &mut Session, just: &[usize]) -> Run {
    let (mut run, _found) = run_on_level(Mode::Play, HUB_1, setup(&[]));
    let definition = story_save::definition(&run.vm, &run.world).expect("a definition");
    run.vm.native_first(definition);
    let id = run.world.run();
    run.step_play_with(&mut |vm, at, host| {
        if at == definition {
            assert!(story_save::title(vm, host, session, id)?, "the save read");
            assert_eq!(story_save::mark_just_done(vm, just), just.len());
        }
        Ok(())
    })
    .unwrap();
    run
}

/// openspec AS.130: what a save holds reaches the hub's definition. A stored
/// save past level 201 counts three, the exterior zone and sky the third
/// count's, and no first visit; the words a run carries on from a level end
/// that just finished 201 count the same as a first visit. A run with no
/// title ahead of its definition (the walk this replaced) counts none, held
/// at the stage's floor, which is what Flynn's offer went by.
#[test]
#[ignore = "reads the Giants dump"]
fn a_save_past_level_201_reaches_the_hubs_definition() {
    println!("load at start: {}", load());
    let (layout, slot, header, values) = save_with(3);
    let id = layout.id();

    let mut stored = Session::new(Some("host".into()));
    stored.stored = Some((id.clone(), values.clone()));
    let mut run = titled_step(&mut stored, &[]);
    assert_eq!(stored.report.start.as_deref(), Some("stored"));
    assert_eq!(global(&run, "Levels_Completed"), Some(Value::Int(3)));
    assert_eq!(global(&run, "First_Time_Here"), Some(Value::Int(0)));
    assert_eq!(global(&run, "Current Exterior Zone"), Some(Value::Int(205)));
    for _ in 0..30 {
        assert_eq!(run.step_play().unwrap().len(), 0);
    }
    assert_eq!(zone(&run), Some(Value::Int(205)));

    let mut carried = Session::new(Some("host".into()));
    carried.live = Some(Live {
        layout: id.clone(),
        slot,
        header,
        just_done: vec![2],
    });
    let mut run = titled_step(&mut carried, &[]);
    assert_eq!(carried.report.start.as_deref(), Some("carried"));
    assert_eq!(global(&run, "Levels_Completed"), Some(Value::Int(3)));
    assert_eq!(global(&run, "First_Time_Here"), Some(Value::Int(1)));
    // The stored save with level 201 marked just done, as the `test-hooks`
    // key asks a resume: the carried words' count and first visit.
    let mut marked = Session::new(Some("host".into()));
    marked.stored = Some((id, values));
    let resumed = titled_step(&mut marked, &[2]);
    assert_eq!(marked.report.start.as_deref(), Some("stored"));
    assert_eq!(global(&resumed, "Levels_Completed"), Some(Value::Int(3)));
    assert_eq!(global(&resumed, "First_Time_Here"), Some(Value::Int(1)));
    // The start's rest makes no second title: what it carried stays what the
    // run started from.
    {
        let mut spawn = guacamole::scripts::headless_spawner(1 << 50);
        let mut engine = guacamole::script_engine::NoEngine;
        let mut host = guacamole::scripts::WorldHost {
            world: &mut run.world,
            engine: &mut engine,
            generator: &mut run.generator,
            spawn: &mut spawn,
        };
        story_save::serve(&mut run.vm, &mut host, &mut carried, HUB_1)
            .unwrap_or_else(|h| panic!("{h}"));
    }
    assert_eq!(carried.report.start.as_deref(), Some("carried"));

    // Without the title ahead of it the definition counts nothing.
    let (mut run, _found) = run_on_level(Mode::Play, HUB_1, setup(&[]));
    run.step_play().unwrap();
    assert_eq!(global(&run, "Levels_Completed"), Some(Value::Int(2)));
    println!("load at end: {}", load());
}

/// openspec AS.269: hub stage 2's Persephone, made anew at every wake on the
/// original's rule (AS.233), flies in again from where her STARTUP sets her
/// down about her landing, by the facing she wakes with. Her first landing
/// leaves her a few degrees off her file's facing (177 against 180), and
/// from there her circle about its pivot passes 1.2 m inside her landing,
/// where the 3 degree check toward it never passes. A placement made anew
/// takes its file's turns (`ScriptWorld::made_anew`), so every flight
/// lands, from the pivot on either side (the draw takes each over ten
/// wakes).
#[test]
#[ignore = "reads the Giants dump"]
fn a_persephone_woken_anew_lands_from_either_side() {
    let stage = "level_hub_2_stage_2";
    let (mut run, _) = run_on_level(
        Mode::Play,
        stage,
        Setup {
            wake: Wake::Every,
            ..setup(&[])
        },
    );
    for _ in 0..2 {
        run.step_play().unwrap();
    }
    run.join().unwrap_or_else(|h| panic!("{stage}: {h}"));
    let intro = run.placement("CS_Hub_Intro_Stage2");
    while !run.vm.removed(intro) {
        run.step_play().unwrap();
    }
    let id = run.placement("Persephone");
    let (me, home) = run.placed("Persephone");
    let far = run
        .files
        .placed
        .iter()
        .filter(|p| p.obj.unit == LEVEL)
        .map(|p| guac_motion::seam::Vec3::from_array(p.position))
        .max_by(|a, b| a.distance(home).total_cmp(&b.distance(home)))
        .unwrap();
    let mut sides = std::collections::BTreeSet::new();
    for cycle in 0..10 {
        // Away until she sleeps, back until she flies.
        run.put(far + guac_motion::seam::Vec3::new(0.0, 0.5, 0.0), 0.0);
        for _ in 0..30 {
            run.step_play().unwrap();
        }
        run.put(home + guac_motion::seam::Vec3::new(1.5, 0.5, 1.5), 0.0);
        let mut flew = None;
        let mut landed = None;
        for k in 0..3000u32 {
            run.step_play().unwrap();
            let flying = run.vm.behaviour(id) == Some("flyin");
            if flew.is_none() && flying {
                flew = Some(k);
                // The pivot is east or west of her: the facing it sets.
                sides.insert(run.world.actor(me).unwrap().angles.facing > 180.0);
            }
            if flew.is_some() && !flying {
                landed = Some(k);
                break;
            }
        }
        println!("wake {cycle}: flies in at step {flew:?}, lands at {landed:?}");
        assert!(flew.is_some(), "wake {cycle}: she flies in again");
        assert!(landed.is_some(), "wake {cycle}: she lands");
    }
    assert_eq!(sides.len(), 2, "both sides were flown: {sides:?}");
}

/// What Dreadbeard's win came to on hub stage 2 ([`dreadbeards_win`]): the
/// steps from his entering `wait for 2d sprites to be ready` to his leaving
/// it (none if he never did), the behaviour after it, the reward macro the
/// sprite stream sets, the card streams' joins and every stop.
struct Win {
    waited: Option<u32>,
    next: Option<String>,
    reward_macro: Option<Value>,
    joined: Vec<(&'static str, usize)>,
    stops: Vec<guac_script::Halt>,
}

/// The longest the reward sprites may keep him waiting (the box's bound):
/// the stream asked the step he enters the wait, joined at that step's end
/// and its STARTUP run the next.
const SPRITES_READY: u32 = 60;

/// Hub stage 2's intro played, the hero set by Dreadbeard and his talk
/// button pressed until the game's question, answered yes as the TV's modal
/// answers it; the step he enters `play the game` his `win val` is written
/// 1 by the test (a won game: the TV's own answer stays the quit, openspec
/// AS.342's probe), and the run goes on until he has left `wait for 2d
/// sprites to be ready` or `hold` steps have passed in it.
fn dreadbeards_win(no_card_streams: bool, hold: u32) -> Win {
    let (stage, intro) = STAGES[1];
    let (mut run, _found) = run_on_level(
        Mode::Play,
        stage,
        Setup {
            no_card_streams,
            ..setup(&[])
        },
    );
    let mut stops = Vec::new();
    for _ in 0..2 {
        stops.extend(run.step_play().unwrap());
    }
    run.join().unwrap();
    let intro = run.placement(intro);
    while !run.vm.removed(intro) {
        assert!(run.steps < 3600, "the intro's end");
        stops.extend(run.step_play().unwrap());
    }
    for _ in 0..30 {
        stops.extend(run.step_play().unwrap());
    }
    let id = run.placement("Dreadbeard");
    let at = guac_motion::seam::Vec3::from_array(
        run.files
            .placed
            .iter()
            .find(|p| p.obj.unit == LEVEL && p.name == "Dreadbeard")
            .expect("his placement")
            .position,
    );
    run.put(at + guac_motion::seam::Vec3::new(1.5, 0.5, 1.5), 0.0);
    for _ in 0..10 {
        stops.extend(run.step_play().unwrap());
    }
    const WAIT: &str = "wait for 2d sprites to be ready";
    let (mut won, mut read, mut entered, mut left) = (false, 0, None, None);
    for k in 0..6000u32 {
        let free = cutscene_name(&run).is_none();
        talk_button(&mut run, !won && free && k % 20 == 0);
        stops.extend(run.step_play().unwrap());
        if let Some(p) = guacamole::prompt::up(&run.vm, &run.world) {
            read += 1;
            if read >= READING {
                read = 0;
                let answered = run
                    .with_host(|vm, host| guacamole::prompt::answer(vm, host, &p, true))
                    .unwrap_or_else(|h| panic!("the answer: {h}"));
                assert!(answered);
            }
        }
        let b = run.vm.behaviour(id).map(str::to_owned);
        if !won && b.as_deref() == Some("play the game") {
            assert!(run.vm.set_local(id, "win val", Value::Int(1)));
            won = true;
        }
        match (entered, b.as_deref()) {
            (None, Some(WAIT)) => entered = Some(run.steps),
            (Some(_), Some(WAIT)) => {}
            (Some(e), _) => {
                left = Some((run.steps - e, b.clone()));
                break;
            }
            (None, _) => {}
        }
        if entered.is_some_and(|e| run.steps - e >= hold) {
            break;
        }
    }
    talk_button(&mut run, false);
    assert!(
        entered.is_some(),
        "his win's loot reached the sprites' wait"
    );
    let reward_macro = global(&run, "Game Systems").and_then(|gs| {
        run.vm
            .field(gs, "reward card sprite macro")
            .map(|v| run.vm.value(v))
    });
    println!(
        "Dreadbeard's win (card streams {}): entered the wait at {entered:?}, left {left:?}, reward macro {reward_macro:?}, joined {:?}, streams asked {:?}, steps {}",
        !no_card_streams, run.world.card_streams.joined, run.world.streams, run.steps
    );
    Win {
        waited: left.as_ref().map(|(n, _)| *n),
        next: left.and_then(|(_, b)| b),
        reward_macro,
        joined: run.world.card_streams.joined.clone(),
        stops,
    }
}

/// openspec AS.364: the card game's streams linked beside hub stage 2,
/// which places Dreadbeard running: his won game's loot asks for the reward
/// card's sprites (`misc/CardGame_2dSprites_Streamed`), the stream's two
/// running placements join the run at that step's end, its startup sets the
/// reward macro, and he passes `wait for 2d sprites to be ready` inside
/// [`SPRITES_READY`] steps to `wait for loot theatrics`, nothing stopped.
/// Without the streams (an extract without their part) the same win waits
/// out the 600 steps held, the macro unset, as AS.342's probe found.
#[test]
#[ignore = "reads the Giants dump"]
fn dreadbeards_win_passes_the_reward_sprites_with_the_card_streams() {
    let started = std::time::Instant::now();
    println!("load at start: {}", load());
    let win = dreadbeards_win(false, 600);
    let waited = win.waited.expect("he left the sprites' wait");
    assert!(
        waited <= SPRITES_READY,
        "{waited} steps in the sprites' wait"
    );
    assert_eq!(win.next.as_deref(), Some("wait for loot theatrics"));
    assert!(
        matches!(win.reward_macro, Some(v) if v != Value::Null),
        "{:?}",
        win.reward_macro
    );
    assert_eq!(win.joined, [(guacamole::card_game::SPRITES, 2)]);
    assert!(win.stops.is_empty(), "{:#?}", win.stops);
    let bare = dreadbeards_win(true, 600);
    assert_eq!(bare.waited, None, "no stream: he waits it out");
    assert_eq!(bare.reward_macro, Some(Value::Null));
    assert!(bare.joined.is_empty());
    assert!(bare.stops.is_empty(), "{:#?}", bare.stops);
    println!(
        "{:.1} s, load at end: {}",
        started.elapsed().as_secs_f64(),
        load()
    );
}

/// Each operand an op names, as a list.
fn operands_of(ins: &guac_script::Ins) -> Vec<guac_script::OperandId> {
    use guac_script::Ins;
    let all: Vec<Option<guac_script::OperandId>> = match ins {
        Ins::Parameter(p) => vec![p.rhs, p.index],
        Ins::UseMacro { np, index } | Ins::SetBehavior { np, index } => vec![*np, *index],
        Ins::SetValue { lhs, rhs } => vec![*lhs, *rhs],
        Ins::SetReference { lhs, rhs, index } => vec![*lhs, *rhs, *index],
        Ins::Inc { lhs } | Ins::Dec { lhs } => vec![*lhs],
        Ins::Reset { np } => vec![*np],
        Ins::CheckValue { lhs, rhs, .. }
        | Ins::CheckMembership { lhs, rhs, .. }
        | Ins::ChangeMembership { lhs, rhs, .. }
        | Ins::ForEach { lhs, rhs, .. }
        | Ins::FindSubSet { lhs, rhs, .. } => vec![*lhs, *rhs],
        Ins::CheckReference {
            lhs, rhs, index, ..
        } => vec![*lhs, *rhs, *index],
        Ins::LoopValue { rhs } => vec![*rhs],
        Ins::FindVariable {
            owner, index, skip, ..
        } => vec![*owner, *index, *skip],
        Ins::World { operands, .. } => operands.iter().map(|(_, o)| *o).collect(),
        _ => Vec::new(),
    };
    all.into_iter().flatten().collect()
}

/// Every member a path in operand `o` reaches, its other point and its
/// expression's sides included.
fn members_in(
    program: &guac_script::Program,
    o: guac_script::OperandId,
    out: &mut Vec<guac_script::MemberId>,
) {
    use guac_script::{Elem, Operand};
    match program.operands.get(o as usize) {
        Some(Operand::Path { elems, other, .. }) => {
            out.extend(elems.iter().filter_map(|e| match e {
                Elem::Member(m) => Some(*m),
                _ => None,
            }));
            if let Some(x) = other {
                members_in(program, *x, out);
            }
        }
        Some(Operand::Expr { a, b, .. }) => {
            for x in [a, b].into_iter().flatten() {
                members_in(program, *x, out);
            }
        }
        None => {}
    }
}

/// openspec AS.364: every member the card game's three streams' scripts
/// name, linked beside hub stage 2 with the interface's model on as the
/// TV's run has it, is the VM's own (a builtin or a packed value's part the
/// VM computes), registered, or one of the Wii U instruction page's pair
/// (`wiicontroller.button 1`, `scriptcontroller.wii`), which stay
/// unregistered: `viewportinfo.alternate viewport` among the registered.
#[test]
#[ignore = "reads the Giants dump"]
fn the_card_streams_name_no_member_the_tv_lacks_but_the_wii_pair() {
    use guac_script::MemberId;
    let (run, _found) = run_on_level(Mode::Play, STAGES[1].0, setup(&[]));
    let program = run.vm.program_arc();
    let members = run.vm.config().members.clone();
    let units: Vec<u16> = run
        .world
        .card_streams
        .units
        .iter()
        .filter(|(a, _)| guacamole::card_game::STREAMS.contains(a))
        .map(|(_, u)| *u)
        .collect();
    assert_eq!(units.len(), 3, "{:?}", run.world.card_streams.units);
    let vm_own: Vec<MemberId> = guac_script::members::INIT_CHAIN
        .iter()
        .filter(|r| r.keeper == guac_script::members::Keeper::Vm)
        .map(|r| MemberId::named(r.namespace, r.name))
        .collect();
    let (mut ops, mut seen) = (0, std::collections::BTreeSet::new());
    for s in program.scripts.iter().filter(|s| units.contains(&s.unit)) {
        for &op in &s.ops {
            ops += 1;
            for o in operands_of(&program.ops[op as usize]) {
                let mut ms = Vec::new();
                members_in(&program, o, &mut ms);
                seen.extend(ms);
            }
        }
    }
    let lacking: Vec<String> = seen
        .iter()
        .filter(|&&m| {
            guac_script::Builtin::of(m).is_none()
                && members.get(m).is_none()
                && !vm_own.contains(&m)
        })
        .map(|&m| members.name(m))
        .collect();
    println!("{ops} ops, {} members, lacking {lacking:?}", seen.len());
    let alternate = MemberId::named("viewportinfo", "alternate viewport");
    assert!(seen.contains(&alternate) && members.get(alternate).is_some());
    let wii = [
        MemberId::named("wiicontroller", "button 1"),
        MemberId::named("scriptcontroller", "wii"),
    ];
    let mut lacking_ids: Vec<MemberId> = seen
        .iter()
        .copied()
        .filter(|&m| {
            guac_script::Builtin::of(m).is_none()
                && members.get(m).is_none()
                && !vm_own.contains(&m)
        })
        .collect();
    let mut wii = wii.to_vec();
    lacking_ids.sort();
    wii.sort();
    assert_eq!(lacking_ids, wii, "{lacking:?}");
}

/// What a played game on hub stage 2 came to ([`dreadbeards_games`]): each
/// game's end (the step, the manager's last behaviour and the pirate's next
/// one), the opponent's rolls, the behaviours' stays, every stop, the card
/// streams still running after, and the leftovers before the yes and after
/// the run's last game.
#[derive(Debug)]
struct Played {
    games: Vec<Game>,
    picks: cards::Picks,
    stays: cards::Stays,
    stops: Vec<guac_script::Halt>,
    running: Vec<String>,
    before: std::collections::BTreeMap<String, String>,
    after: std::collections::BTreeMap<String, String>,
    pirate: Option<String>,
    steps: u32,
    moved: f32,
    streams: Vec<Option<String>>,
    /// How many times the hero was put back after a game ([`cards::hero_back`]).
    backs: u32,
    /// The board's two chances as his call set them (`best move - 1 %`,
    /// `best move - 2 %`), read off the first game.
    chances: Option<(i32, i32)>,
}

/// One game's end.
#[derive(Debug, Clone, PartialEq)]
struct Game {
    seed: u32,
    started: u32,
    ended: u32,
    board: Option<String>,
    pirate: Option<String>,
}

/// How a game is played and when it's over ([`dreadbeards_games`]).
#[derive(Debug, Clone, Copy, PartialEq)]
enum Plan {
    /// Played by `policy` to the board's report, accepted: a loss ends in
    /// the replay (the board's next game), a win in his loot.
    Play(cards::Policy),
    /// Quit through the board's own confirm at the player's first pick.
    Quit,
    /// His question answered no: no game, his talk over at his `wait for
    /// player` (the run a game is set beside).
    Decline,
}

/// Hub stage 2's intro played, the hero set by Dreadbeard, his talk button
/// pressed until his question and a yes answered as the TV's modal answers
/// it, in a run that plays the card game (`CardStreams::play_games`); each
/// of `plans` a game, seeded from its seed as the board starts it
/// ([`cards::seed`]); a quit game's next is asked for with another yes. The
/// run ends when the last game is over: a loss at the replay's start, a win
/// at his `wait for loot theatrics` (AS.368's), a quit at his `wait for
/// player`; then a full stick for 150 steps (when he's free to walk).
fn dreadbeards_games(plans: &[(Plan, u32)], cap: u32) -> Played {
    let (stage, intro) = STAGES[1];
    let (mut run, _found) = run_on_level(Mode::Play, stage, setup(&[]));
    run.world.card_streams.play_games();
    let mut stops = Vec::new();
    for _ in 0..2 {
        stops.extend(run.step_play().unwrap());
    }
    run.join().unwrap();
    let intro = run.placement(intro);
    while !run.vm.removed(intro) {
        assert!(run.steps < 3600, "the intro's end");
        stops.extend(run.step_play().unwrap());
    }
    for _ in 0..30 {
        stops.extend(run.step_play().unwrap());
    }
    let id = run.placement("Dreadbeard");
    let at = guac_motion::seam::Vec3::from_array(
        run.files
            .placed
            .iter()
            .find(|p| p.obj.unit == LEVEL && p.name == "Dreadbeard")
            .expect("his placement")
            .position,
    ) + guac_motion::seam::Vec3::new(1.5, 0.5, 1.5);
    run.put(at, 0.0);
    for _ in 0..10 {
        stops.extend(run.step_play().unwrap());
    }
    let before = cards::leftovers(&run);
    let (mut picks, mut stays) = (cards::Picks::default(), cards::Stays::default());
    let mut games: Vec<Game> = Vec::new();
    let mut player = cards::Player::new(cards::Policy::Naive);
    let (mut read, mut asked, mut quit_sent) = (0, false, false);
    let mut was_board: Option<String> = None;
    let mut last_board: Option<String> = None;
    let start = run.steps;
    let debug = std::env::var_os("AS366_DEBUG").is_some();
    let mut declined: Option<u32> = None;
    let mut backs = 0;
    let mut chances = None;
    let mut dbg_pirate: Option<String> = None;
    while run.steps - start < cap {
        // The game on, else the next one asked for.
        let on = games.iter().position(|g| g.ended == 0);
        let Some(&(plan, _)) = plans.get(on.unwrap_or(games.len())) else {
            break;
        };
        if on.is_none() && !asked {
            let press = cutscene_name(&run).is_none() && run.steps.is_multiple_of(20);
            talk_button(&mut run, press);
        } else if let Plan::Play(policy) = plan {
            player.policy = policy;
            let pad = player.pad(&run);
            run.bench.pads[0] = pad;
        } else {
            talk_button(&mut run, false);
        }
        stops.extend(run.step_play().unwrap());
        if let Some(p) = guacamole::prompt::up(&run.vm, &run.world) {
            read += 1;
            if read >= READING {
                read = 0;
                let yes = plan != Plan::Decline;
                let answered = run
                    .with_host(|vm, host| guacamole::prompt::answer(vm, host, &p, yes))
                    .unwrap_or_else(|h| panic!("the answer: {h}"));
                assert!(answered);
                asked = true;
                if !yes {
                    declined = Some(run.steps);
                }
                if debug {
                    println!("AS366 step {} yes", run.steps);
                }
            }
        }
        if debug {
            let b = run.vm.behaviour(id).map(str::to_owned);
            if b != dbg_pirate {
                println!(
                    "AS366 step {} pirate {b:?} board {:?} streams {:?} menu {:?}",
                    run.steps,
                    cards::board(&run),
                    run.world.streams.back(),
                    guacamole::card_game::page_up(&run.vm).map(|p| p.2)
                );
                dbg_pirate = b;
            }
        }
        picks.note(&run);
        stays.note(&run);
        if chances.is_none() {
            chances = cards::chances(&run);
        }
        if cards::hero_back(&mut run) {
            backs += 1;
        }
        let now = cards::board(&run);
        let pirate = run.vm.behaviour(id).map(str::to_owned);
        // The game on over: a quit at his wait for the player, a win at his
        // loot's wait, a loss at the replay's start.
        if let Some(i) = on {
            let over = match plan {
                Plan::Quit | Plan::Decline => pirate.as_deref() == Some("wait for player"),
                Plan::Play(_) => {
                    pirate.as_deref() == Some("wait for loot theatrics")
                        || (was_board.as_deref() == Some(cards::LOST)
                            && now.as_deref() == Some(cards::GAME_START))
                }
            };
            if over {
                let g = &mut games[i];
                g.ended = run.steps;
                g.board = last_board.clone();
                g.pirate = pirate.clone();
                // A quit's next game is asked for again, the hero set by
                // him again as a talk sets him; a loss's replay starts on
                // its own.
                asked = !matches!(plan, Plan::Quit);
                if !asked {
                    run.put(at, 0.0);
                }
            }
        }
        // A declined question: a "game" of no board from the answer to his
        // talk's end.
        if on.is_none()
            && plan == Plan::Decline
            && let Some(at) = declined
            && pirate.as_deref() == Some("wait for player")
        {
            games.push(Game {
                seed: 0,
                started: at,
                ended: run.steps,
                board: None,
                pirate: pirate.clone(),
            });
            declined = None;
        }
        // A game's start, seeded: the first, each replay and each asked again.
        if now.as_deref() == Some(cards::GAME_START)
            && was_board.as_deref() != now.as_deref()
            && let Some(&(_, seed)) = plans.get(games.len())
        {
            cards::seed(&mut run, seed);
            games.push(Game {
                seed,
                started: run.steps,
                ended: 0,
                board: None,
                pirate: None,
            });
            quit_sent = false;
        }
        if games.last().is_some_and(|g| g.ended == 0)
            && matches!(plans[games.len() - 1].0, Plan::Quit)
            && now.as_deref() == Some(cards::PICK_STONE)
            && !quit_sent
        {
            quit_sent = run
                .with_host(guacamole::card_game::quit)
                .unwrap_or_else(|h| panic!("the quit: {h}"));
        }
        if now.is_some() {
            last_board.clone_from(&now);
        }
        // A win ends the pirate's games: his loot waits on AS.368's
        // theatrics, so nothing after it can start.
        if games
            .last()
            .is_some_and(|g| g.ended > 0 && g.pirate.as_deref() == Some("wait for loot theatrics"))
        {
            break;
        }
        was_board = now;
        if games.len() == plans.len() && games.iter().all(|g| g.ended > 0) {
            break;
        }
    }
    talk_button(&mut run, false);
    // His character back once no scene holds him ([`cards::hero_back`]).
    for _ in 0..1200 {
        if !run.minion.is_some_and(|m| run.vm.removed(m)) {
            break;
        }
        stops.extend(run.step_play().unwrap());
        if cards::hero_back(&mut run) {
            backs += 1;
        }
    }
    let after = cards::leftovers(&run);
    if debug {
        let m = run.minion;
        println!(
            "AS366 after: backs {backs}, minion {m:?} removed {:?} asleep {:?} behaviour {:?} stopped {:?}, hero handle {} actor active {:?}, cutscene {:?}, held {:?} suspended {:?} in order {:?}",
            m.map(|m| run.vm.removed(m)),
            m.map(|m| run.vm.asleep(m)),
            m.and_then(|m| run.vm.behaviour(m)),
            m.and_then(|m| run.vm.stopped(m).map(ToString::to_string)),
            run.hero,
            run.world.actor(run.hero).map(|a| (a.removed, a.range)),
            cutscene_name(&run),
            m.map(|m| run.vm.is_held(m)),
            m.map(|m| run.vm.is_suspended(m)),
            m.and_then(|m| run.vm.handle_of(m)).map(|h| {
                let name = run.world.name(h);
                run.vm
                    .tick_order()
                    .iter()
                    .filter(|(_, n)| *n == name)
                    .count()
            })
        );
        if let Some(m) = run.minion {
            let mut last = None;
            for k in 0..20 {
                stops.extend(run.step_play().unwrap());
                let now = (
                    run.vm.active_seconds(m),
                    run.vm.behaviour(m).map(str::to_owned),
                    run.vm.resting(m),
                    run.vm.is_activator(m),
                );
                if last.as_ref() != Some(&now) {
                    println!(
                        "AS366 after +{k}: minion active {:?} behaviour {:?} resting {} activator {}",
                        now.0, now.1, now.2, now.3
                    );
                    last = Some(now);
                }
            }
            if let Some(d) = run.vm.local(m, "description").map(|v| run.vm.value(v)) {
                println!(
                    "AS366 description {d:?}: {:?}",
                    run.vm
                        .fields(d)
                        .iter()
                        .filter(|(n, _)| n.contains("state")
                            || n.contains("disable")
                            || n.contains("lock"))
                        .map(|(n, v)| format!("{n}={v:?}"))
                        .collect::<Vec<_>>()
                );
            }
            let hero = run.hero;
            println!(
                "AS366 hero bench {:?}, world actor {:?}",
                run.bench.heroes.contains_key(&hero),
                run.world.actor(hero).map(|a| (a.position, a.removed))
            );
        }
    }
    let pirate = run.vm.behaviour(id).map(str::to_owned);
    let running = cards::streams_running(&run);
    // Free to walk, where the game's over and his scene let him go.
    let from = run.hero().pivot;
    for _ in 0..150 {
        run.bench.pads[0] = guacamole::script_engine::Pad {
            stick: guac_motion::stick::Stick::new(0.0, 1.0),
            actions: [false; 4],
            connected: true,
        };
        stops.extend(run.step_play().unwrap());
    }
    let moved = run.hero().pivot.distance(from);
    println!(
        "Dreadbeard's games: {games:?}; rolls: first landed {} of {} ({:.1}%), second {:.1}% of {}; longest unpressed stay {:?}; {} stops; running after {running:?}; moved {moved:.1}; steps {}",
        picks.first_landed,
        picks.attempts,
        picks.first(),
        picks.second(),
        picks.count(),
        stays.longest_unpressed(),
        stops.len(),
        run.steps
    );
    Played {
        games,
        picks,
        stays,
        stops,
        running,
        before,
        after,
        pirate,
        steps: run.steps,
        moved,
        streams: run.world.streams.iter().cloned().collect(),
        backs,
        chances,
    }
}

/// The seeds of [`dreadbeards_game_quit_lost_and_won_each_end_in_his_own_next_behaviour`]'s
/// three games: the quit's, the naive player's loss and the strong
/// player's win (each found by playing; a seed's game is the same to the
/// step from the same start).
const DREADBEARDS_SEEDS: [u32; 3] = [1, 2, 4];

/// openspec AS.366: Dreadbeard's card game played on the VM, headless, on
/// hub stage 2 (3 by 3, his call's chances), three games in one run: quit
/// at the first pick through the board's own confirm, which ends in his
/// quit scene and `wait for player`; asked again and lost by a naive player,
/// whose report's accept ends in the replay (the board's next game); that
/// replay won by a strong player, which ends in his loot's `wait for loot
/// theatrics` (AS.368's). The hand the hand select's own recommended-hand
/// macro chose, the cursor and accept by pad 0, the flips, the opponent and
/// the end the scripts'. Nothing stops, no card stream's placement but a
/// stone's runs after, and he walks once the game's over.
#[test]
#[ignore = "reads the Giants dump"]
fn dreadbeards_game_quit_lost_and_won_each_end_in_his_own_next_behaviour() {
    let started = std::time::Instant::now();
    println!("load at start: {}", load());
    let seeds: Vec<u32> = std::env::var("AS366_SEEDS").map_or(DREADBEARDS_SEEDS.to_vec(), |v| {
        v.split(',').map(|s| s.parse().unwrap()).collect()
    });
    let played = dreadbeards_games(
        &[
            (Plan::Quit, seeds[0]),
            (Plan::Play(cards::Policy::Naive), seeds[1]),
            (Plan::Play(cards::Policy::Strong), seeds[2]),
        ],
        40_000,
    );
    for g in &played.games {
        println!("game {g:?}, {} steps", g.ended - g.started);
    }
    for d in cards::differ(&played.before, &played.after) {
        println!("left: {d}");
    }
    println!(
        "streams asked: {:?}; put back {}",
        played.streams, played.backs
    );
    assert_eq!(played.games.len(), 3, "{:?}", played.games);
    let [quit, lost, won] = [&played.games[0], &played.games[1], &played.games[2]];
    assert_eq!(quit.pirate.as_deref(), Some("wait for player"), "{quit:?}");
    assert_eq!(lost.board.as_deref(), Some(cards::LOST), "{lost:?}");
    assert_eq!(won.board.as_deref(), Some(cards::WON), "{won:?}");
    assert_eq!(
        won.pirate.as_deref(),
        Some("wait for loot theatrics"),
        "{won:?}"
    );
    assert_eq!(played.pirate.as_deref(), Some("wait for loot theatrics"));
    assert!(played.stops.is_empty(), "{:#?}", played.stops);
    // The board and the hand select down; the reward card's sprites, which
    // his loot's theatrics wait on (AS.368's), up.
    assert_eq!(
        played.running,
        ["2dSpritesStream_Startup", "CardArt_Macros"],
        "{:?}",
        played.running
    );
    assert_eq!(
        played.backs, 2,
        "the hero put back after the quit and the win"
    );
    // No behaviour of the board's, the player's own waits aside, outstays
    // the game's own longest (the stone's 3D presentation and the flips).
    let (longest, stay) = played.stays.longest_unpressed().expect("a stay");
    println!(
        "longest unpressed stay: {longest} {stay} steps; all {:?}",
        played.stays.longest
    );
    assert!(stay <= STAY_CAP, "{longest}: {stay} steps");
    println!(
        "{:.1} s, {} steps, load at end: {}",
        started.elapsed().as_secs_f64(),
        played.steps,
        load()
    );
}

/// The longest any board behaviour but the player's own waits may last
/// (openspec AS.366): no recording of a card game exists to bound it by, so
/// the bound is the game's own, its slowest stay seen over the 1,000 seeded
/// games ([`dreadbeards_rolls_over_a_thousand_seeded_games`]).
const STAY_CAP: u32 = 600;

/// openspec AS.366: a game is seeded by the run's draws restarted from its
/// seed as the board starts it ([`cards::seed`]), so the same seed from the
/// same start replays it to the step: two runs of hub stage 2, each a naive
/// player's game from seed 7, the board's every behaviour at the same step
/// and the opponent's every pick the same; and a second game after a quit,
/// seeded 7 too, plays the same course as the first, step for step from its
/// own start until the opponent's second pick (the session's generator is
/// the level's scripts' too), the same behaviours and the same end.
#[test]
#[ignore = "reads the Giants dump"]
fn a_seeded_game_replays_to_the_step_and_a_second_plays_the_same() {
    println!("load at start: {}", load());
    let naive = Plan::Play(cards::Policy::Naive);
    let a = dreadbeards_games(&[(naive, 7)], 20_000);
    let b = dreadbeards_games(&[(naive, 7)], 20_000);
    assert_eq!(a.games, b.games);
    assert_eq!(a.stays.order, b.stays.order);
    assert_eq!(a.picks.trace, b.picks.trace);
    assert!(!a.picks.trace.is_empty());
    let second = dreadbeards_games(&[(Plan::Quit, 1), (naive, 7)], 20_000);
    assert_eq!(second.games.len(), 2, "{:?}", second.games);
    let (first, again) = (&a.games[0], &second.games[1]);
    assert_eq!(first.board, again.board);
    let relative = |p: &Played, g: &Game| -> Vec<(u32, String)> {
        p.stays
            .order
            .iter()
            .filter(|(at, _)| *at >= g.started && *at <= g.ended)
            .map(|(at, b)| (at - g.started, b.clone()))
            .collect()
    };
    // The same to the step until the opponent's second pick: from there
    // his draws come off the session's generator, which the level's own
    // scripts draw from too, and a level a game's length on draws
    // differently between the board's steps.
    let (r1, r2) = (relative(&a, first), relative(&second, again));
    let second_pick = |r: &[(u32, String)]| {
        r.iter()
            .enumerate()
            .filter(|(_, (_, b))| b == cards::OPPONENT_PICK)
            .nth(1)
            .map(|(i, _)| i + 1)
            .expect("a second pick")
    };
    let upto = second_pick(&r1);
    assert_eq!(upto, second_pick(&r2));
    assert_eq!(r1[..upto], r2[..upto]);
    let visited = |r: &[(u32, String)]| {
        r.iter()
            .map(|(_, b)| b.clone())
            .collect::<std::collections::BTreeSet<_>>()
    };
    assert_eq!(visited(&r1), visited(&r2), "the same course");
    for p in [&a, &b, &second] {
        assert!(p.stops.is_empty(), "{:#?}", p.stops);
    }
    println!(
        "seed 7: {:?}, {} picks, {} steps; again after a quit from step {}",
        first.board,
        a.picks.trace.len(),
        first.ended - first.started,
        again.started
    );
    println!("load at end: {}", load());
}

/// openspec AS.366: the opponent's rolls over seeded games on hub stage 2
/// (from seed 1000, a weak player's games, each loss's replay the next, a
/// run started again after a win), his call on this stage's 70 and 70
/// ([`cards::Picks`]: the first roll's landings of its draws, the second's
/// of the full lists' picks), no board behaviour but the player's own waits
/// past [`STAY_CAP`], nothing stopped. By default the gate's short run:
/// [`SHORT_GAMES`] games, each roll's share within [`SHORT_POINTS`] of the
/// call's chance (seeded, so the same every run: 294 of 444 first rolls,
/// 66.2%, and 83 of 120 second, 69.2%; 90 s alone at load 7 to 8). The
/// box's 1,000 games, within 5 points, only when `AS366_GAMES=1000` asks
/// (about an hour a third: run as three processes by `AS366_FROM` and
/// `AS366_GAMES`, their counts summed; 300 games or more are held to 5
/// points).
#[test]
#[ignore = "reads the Giants dump; 30 games, 1,000 by AS366_GAMES"]
fn dreadbeards_rolls_over_a_thousand_seeded_games() {
    let started = std::time::Instant::now();
    println!("load at start: {}", load());
    // A share of the seeds by `AS366_FROM` and `AS366_GAMES` (the 1,000 run
    // as three processes side by side, their counts summed); the short run
    // by default.
    let from: u32 = std::env::var("AS366_FROM").map_or(0, |v| v.parse().unwrap());
    let games: u32 = std::env::var("AS366_GAMES").map_or(SHORT_GAMES, |v| v.parse().unwrap());
    // Each third of the 1,000 met 5 points alone; a short run gets more.
    let points = if games >= 300 { 5.0 } else { SHORT_POINTS };
    let naive = Plan::Play(cards::Policy::Weak);
    let mut picks = cards::Picks::default();
    let mut longest: std::collections::BTreeMap<String, u32> = Default::default();
    let (mut played, mut runs, mut won, mut steps) = (from, 0, 0, 0u64);
    let games = from + games;
    let mut stops = Vec::new();
    let mut chances = None;
    while played < games {
        let plans: Vec<(Plan, u32)> = (played..games).map(|i| (naive, 1000 + i)).collect();
        // A board stuck mid-game ends its run (a game is about 3,900 steps)
        // and fails on the run with no game over, where an uncapped run
        // would hold the gate for good.
        let cap = GAME_STEPS.saturating_mul(plans.len() as u32);
        let p = dreadbeards_games(&plans, cap);
        runs += 1;
        steps += u64::from(p.steps);
        let done = p.games.iter().filter(|g| g.ended > 0).count() as u32;
        assert!(done > 0, "a run with no game over: {:?}", p.games);
        won += p
            .games
            .iter()
            .filter(|g| g.board.as_deref() == Some(cards::WON))
            .count() as u32;
        played += done;
        picks.attempts += p.picks.attempts;
        picks.first_landed += p.picks.first_landed;
        picks.second += p.picks.second;
        picks.last += p.picks.last;
        for (b, n) in &p.stays.longest {
            let e = longest.entry(b.clone()).or_default();
            *e = (*e).max(*n);
        }
        stops.extend(p.stops);
        chances = chances.or(p.chances);
        println!(
            "after {played} games ({runs} runs, {won} won): first {:.1}% of {}, second {:.1}% of {}; {:.0} s, load {}",
            picks.first(),
            picks.attempts,
            picks.second(),
            picks.count(),
            started.elapsed().as_secs_f64(),
            load()
        );
    }
    println!(
        "seeds {} to {}: first landed {} of {}, second {} of {} (last {}); his call's chances: {chances:?}; stays {longest:?}",
        1000 + from,
        999 + games,
        picks.first_landed,
        picks.attempts,
        picks.second,
        picks.count(),
        picks.last
    );
    let (best, second) = chances.expect("his call's chances");
    assert!(picks.attempts > 0 && picks.count() > 0, "{picks:?}");
    assert!(
        (picks.first() - f64::from(best)).abs() <= points,
        "first roll {:.1}% against {best}",
        picks.first()
    );
    assert!(
        (picks.second() - f64::from(second)).abs() <= points,
        "second roll {:.1}% against {second}",
        picks.second()
    );
    let worst = longest
        .iter()
        .filter(|(b, _)| !b.starts_with("player turn") && !b.contains("tutorial"))
        .max_by_key(|(_, n)| **n);
    println!("longest unpressed stay over the games: {worst:?}");
    assert!(worst.is_some_and(|(_, n)| *n <= STAY_CAP), "{worst:?}");
    assert!(stops.is_empty(), "{:#?}", &stops[..stops.len().min(10)]);
    println!(
        "{} games in {runs} runs, {steps} steps, {:.0} s, load at end: {}",
        played - from,
        started.elapsed().as_secs_f64(),
        load()
    );
}

/// The steps a run of the rolls' test may take a game, about five
/// times a game's own.
const GAME_STEPS: u32 = 20_000;

/// The gate's short run of [`dreadbeards_rolls_over_a_thousand_seeded_games`]:
/// its games, and how far each roll's share may sit from the call's chance
/// over so few (30 games give 444 first rolls and 120 second).
const SHORT_GAMES: u32 = 30;
const SHORT_POINTS: f64 = 10.0;

/// [`cards::leftovers`] of a run's end with the placements' run numbers off
/// their names and what runs on its own clock (timers, the camera's shake)
/// left out: what two runs settled at the same place should agree on.
fn settled(
    left: &std::collections::BTreeMap<String, String>,
) -> std::collections::BTreeMap<String, String> {
    left.iter()
        .filter(|(k, _)| !k.contains("timer") && !k.contains("shake"))
        .map(|(k, v)| (k.split(" #").next().unwrap_or(k).to_owned(), v.clone()))
        .collect()
}

/// openspec AS.366: what a game leaves behind. Dreadbeard's game quit at
/// its first pick, beside a run whose player answers his question no: each
/// over at his `wait for player`, the scripts' own state of the game
/// systems, the level, the portal, the music and the camera and every
/// running placement by its behaviour the same in both (the board's
/// teardown gave back what its set-up took: the minigame data, the pause
/// menu, the creatures' and the portal's locks, the atmospheric zone, the
/// music, the camera), the hero's own marks aside, no card stream's
/// placement running; after the no a full stick walks him off.
#[test]
#[ignore = "reads the Giants dump"]
fn a_quit_game_leaves_the_hub_as_a_declined_question_does() {
    println!("load at start: {}", load());
    let quit = dreadbeards_games(&[(Plan::Quit, 1)], 20_000);
    let no = dreadbeards_games(&[(Plan::Decline, 0)], 20_000);
    assert_eq!(quit.games.len(), 1, "{:?}", quit.games);
    assert_eq!(no.games.len(), 1, "{:?}", no.games);
    let diff = cards::differ(&settled(&no.after), &settled(&quit.after));
    for d in &diff {
        println!("beside a declined one: {d}");
    }
    for d in cards::differ(&settled(&quit.before), &settled(&quit.after)) {
        println!("before the yes and after the game: {d}");
    }
    assert!(quit.running.is_empty(), "{:?}", quit.running);
    // The hero put back by the harness comes back under designer control
    // (his new description's state, `Minion - setup post descriptions`)
    // and doesn't walk: his reload after a game is AS.481's. After a no he
    // walks.
    println!(
        "walked: {:.1} after the game, {:.1} after a no",
        quit.moved, no.moved
    );
    assert!(no.moved > 3.0);
    // What differs is the hero's own: put back after the game, he's a new
    // entity the camera follows (its target and the def it picked where he
    // stands), his minion's behaviour, and the portal's restore of his
    // stream left its own marks (the reloaded object, the flag no script
    // reads); and the game's quit scene's camera shot.
    let his: [&str; 7] = [
        "camera control data.cam def index",
        "camera control data.trans plane target actor",
        "portal control data.loaded object just changed",
        "portal control data.restore character stream state",
        "placement 009_Eruptor",
        "cs control data.cam shot id counter",
        "cs control data.last cam shot def",
    ];
    let other: Vec<&String> = diff
        .iter()
        .filter(|d| !his.iter().any(|k| d.starts_with(k)))
        .collect();
    assert!(other.is_empty(), "{other:#?}");
    assert!(quit.stops.is_empty() && no.stops.is_empty());
    println!("load at end: {}", load());
}
