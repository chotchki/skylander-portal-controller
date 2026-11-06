//! The hub between chapters (openspec AS.46, AS.47, AU.6), headless with
//! guacamole's host on the TV's listing for each hub stage. Ignored by
//! default; they read `GUAC_GIANTS_DUMP` (process env, else `.env.dev`):
//!
//! `cargo test -p guacamole --profile disc --test giants_hub -- --ignored --nocapture`

use guac_script::{Mode, Value};

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

/// What a talk came to: the cutscene it started, when it ended, and each
/// question asked on the way with the answer given.
#[derive(Debug, Default)]
struct Talk {
    cutscene: Option<(u32, Option<String>)>,
    ended: Option<u32>,
    asked: Vec<Asked>,
}

fn hash(line: &str) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    line.hash(&mut h);
    h.finish()
}

/// The hero set down at `at`, then the talk button pressed every 20 steps
/// until a cutscene starts, and the steps run until it ends (or the run
/// changes level), each question answered by `answer` (its string's name to
/// yes or no) as the TV's modal answers it ([`guacamole::prompt::answer`]).
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
    for k in 0..3600u32 {
        talk_button(run, out.cutscene.is_none() && k % 20 == 0);
        stops.extend(run.step_play().unwrap());
        let cs = cutscene_name(run);
        if out.cutscene.is_none() && cs.is_some() {
            out.cutscene = Some((run.steps, cs.clone()));
        }
        if let Some(p) = guacamole::prompt::up(&run.vm, &run.world) {
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
    // openspec AU.28: the people the TV draws are Flynn and Cali, their own
    // scripts running, unwalked, each on a model of the level's creatures.
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
    let names: Vec<&str> = people.iter().map(|p| p.0.as_str()).collect();
    assert_eq!(names, ["Cali", "Flynn"]);
    for (name, runs, model) in &people {
        assert!(runs, "{name}'s script runs");
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
            "Hatterson(Orbit)",
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
        // A talk with each that has one, the stores and the arena said yes.
        let mut talks = Vec::new();
        for (name, yes) in [
            ("Auric", true),
            ("Hatterson", true),
            ("Persephone", false),
            ("Brock", true),
            ("Ermit", false),
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
