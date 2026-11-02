//! AS.15's and AS.51's disc tests, headless with guacamole's host: chapter
//! 1's end asking for the next level by name, and that level's own scripts
//! running with Eruptor joined. Ignored by default; they read
//! `GUAC_GIANTS_DUMP` (process env, else `.env.dev`):
//!
//! `cargo test -p guacamole --profile disc --test giants_level_end -- --ignored --nocapture --test-threads=1`
//!
//! Chapter 1's end is woken as `Rope_around_Post(2)`'s last behaviour wakes
//! it (`Temp_Level_Ender`'s range and buffer to 0), the hero set down on the
//! outro's player waypoint, as the TV's dev entry's end anchor does; from
//! there every step is the scripts' (the fade, the outro woken and its
//! cutscene begun, the outro skipped by the original's skip and its own
//! `Game - Level End`), then `guacamole::level_end`'s level change
//! (stand-in ledger: `cutscene-skip`, `level-end`).

use guac_motion::seam::Vec3;
use guac_script::{Mode, Value};
use guacamole::level_end;

mod common;

use common::*;

/// The outro's player waypoint (`213_Outro_Player_WP`) as its file places
/// it, facing `Temp_Level_Ender`: the dev entry's end anchor.
const END: ([f32; 3], f32) = ([-10.282436, -153.01357, 318.01202], 17.6);
/// The level chapter 1's end names, as the run resolves it.
const NEXT: &str = "level_214_tutotial_2";

/// Chapter 1's end on the TV's listing (`scripts::LEVEL_END_SCRIPTS`), the
/// interface's model loaded (the level string's English text is what names
/// the archive): `Temp_Level_Ender` woken 120 steps after his join, the run
/// stepped in play mode until a level change is held or 600 steps pass.
/// The scripts' own level end names `Level_214`, whose text
/// `Level_214_Tutotial_2` is the archive `level_214_tutotial_2`; the outro
/// is skipped once its cutscene has run a frame; `system.load level` holds
/// the request on later steps; nothing stops.
#[test]
#[ignore = "reads the Giants dump"]
fn chapter_1s_end_asks_for_the_next_level_by_name() {
    let started = std::time::Instant::now();
    println!("load at start: {}", load());
    let setup = Setup {
        interface: true,
        ..Setup::default()
    };
    let (mut run, _found) = run_on_with(Mode::Play, setup);
    let mut stops = Vec::new();
    for _ in 0..2 {
        stops.extend(run.step_play().unwrap());
    }
    run.join().unwrap_or_else(|h| panic!("{h}"));
    for _ in 0..120 {
        stops.extend(run.step_play().unwrap());
    }
    run.put(Vec3::from_array(END.0), END.1);
    let (ender, _) = run.placed("Temp_Level_Ender");
    assert!(run.world.wake(ender));
    let woken_at = run.steps;
    let outro = run.placement("213_Outro_IGC");
    let mut outro_began = None;
    let mut recorded_at = None;
    while run.world.level_change().is_none() {
        assert!(
            run.steps < woken_at + 600,
            "no level change by step {}",
            run.steps
        );
        stops.extend(run.step_play().unwrap());
        if outro_began.is_none() && run.vm.behaviour(outro).is_some() {
            outro_began = Some(run.steps);
        }
        if recorded_at.is_none() && level_end::recorded(&run.vm).is_some() {
            recorded_at = Some(run.steps);
        }
    }
    let change = run.world.level_change().cloned().unwrap();
    println!(
        "woken at step {woken_at}; outro at {outro_began:?}; level end recorded at {recorded_at:?}; level change at {}: {change:?}",
        run.steps
    );
    assert_eq!(change.name, NEXT);
    assert_eq!(change.text, "Level_214_Tutotial_2");
    let to = level_end::recorded(&run.vm).expect("the level end recorded");
    assert_eq!(change.value, to, "the level end's own string");
    let named = match to {
        Value::Object(o) => run.vm.program().name_of(o),
        _ => None,
    };
    assert_eq!(named, Some("Level_214"));
    assert!(
        outro_began.is_some() && run.vm.removed(outro),
        "the outro ran through and took itself off"
    );
    // Held: the request reads on, taken once.
    let asked = run.world.requests.iter().filter(|r| **r == to).count();
    for _ in 0..30 {
        stops.extend(run.step_play().unwrap());
    }
    assert_eq!(
        run.world.requests.iter().filter(|r| **r == to).count(),
        asked
    );
    assert_eq!(asked, 1);
    assert_eq!(run.world.level_change(), Some(&change));
    // The init chain's own request (`DEBUG_LevelLoad`, `debug/debug`) is a
    // stream, read as loaded.
    assert_eq!(
        run.world.streams.iter().cloned().collect::<Vec<_>>(),
        [Some("debug/debug".to_owned())]
    );
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

/// AS.15's review: chapter 1's end on a first run's run, which has no
/// interface's model (the part lands after chapter 1's hand-off, AT.26), so
/// the level string's English text doesn't read. The level end is held as
/// a level change would be, said by its string's name and why, never read
/// as a stream; given the text (as the TV reads it off the part once it's
/// in) it's the change to level 214. Nothing stops.
#[test]
#[ignore = "reads the Giants dump"]
fn chapter_1s_end_without_the_interfaces_model_is_held_and_said() {
    use guacamole::scripts::Unread;
    let started = std::time::Instant::now();
    println!("load at start: {}", load());
    let (mut run, _found) = run_on_with(Mode::Play, Setup::default());
    let mut stops = Vec::new();
    for _ in 0..2 {
        stops.extend(run.step_play().unwrap());
    }
    run.join().unwrap_or_else(|h| panic!("{h}"));
    for _ in 0..120 {
        stops.extend(run.step_play().unwrap());
    }
    run.put(Vec3::from_array(END.0), END.1);
    let (ender, _) = run.placed("Temp_Level_Ender");
    assert!(run.world.wake(ender));
    let woken_at = run.steps;
    while run.world.level_end().is_none_or(|e| e.unread.is_none()) {
        assert!(
            run.steps < woken_at + 600,
            "no level end by step {}",
            run.steps
        );
        stops.extend(run.step_play().unwrap());
    }
    let end = run.world.level_end().cloned().unwrap();
    println!(
        "woken at step {woken_at}; level end held at {}: {end:?}",
        run.steps
    );
    assert_eq!(end.string.as_deref(), Some("Level_214"));
    assert_eq!(end.unread, Some(Unread::NoText));
    assert_eq!(run.world.level_change(), None);
    for _ in 0..30 {
        stops.extend(run.step_play().unwrap());
    }
    let to = level_end::recorded(&run.vm).expect("the level end recorded");
    assert_eq!(end.value, to);
    assert_eq!(
        run.world.requests.iter().filter(|r| **r == to).count(),
        1,
        "taken once"
    );
    assert!(
        run.world.streams.iter().all(Option::is_none) && run.world.streams.len() == 1,
        "only the init chain's own stream, unread: {:?}",
        run.world.streams
    );
    run.world
        .read_level_end(Some("Level_214_Tutotial_2".into()), None);
    assert_eq!(
        run.world.level_change().map(|c| c.name.as_str()),
        Some(NEXT)
    );
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

/// Level 214 on the TV's listing for it (`scripts::level_config`: AH.27's,
/// its definition and its spellings of the listed scripts), its props
/// standing and the interface's model loaded, Eruptor joined at its start
/// (`_Initialize2`) and walked 6 s on the stick: his scripts joined and on
/// to the end, no script stopped, his place moved off the start.
#[test]
#[ignore = "reads the Giants dump"]
fn level_214_runs_its_own_scripts_with_eruptor_joined() {
    use guac_motion::stick::Stick;
    use guacamole::script_engine::Pad;
    let started = std::time::Instant::now();
    println!("load at start: {}", load());
    let setup = Setup {
        interface: true,
        props: true,
        ..Setup::default()
    };
    let (mut run, _found) = run_on_level(Mode::Play, NEXT, setup);
    let start = run.hero().pivot;
    let listing = run.vm.config().whitelist.clone();
    for script in ["Level_Definition_214.ai", "Pop_to_Camera_X.ai"] {
        assert!(listing.lists("", &format!("C:/x/{script}")), "{script}");
    }
    assert!(
        !listing.lists("Enemy_Chompy(4)", "C:/x/Nothing.ai"),
        "chapter 1's fight's names aren't 214's listing"
    );
    let mut stops = Vec::new();
    for _ in 0..2 {
        stops.extend(run.step_play().unwrap());
    }
    run.join().unwrap_or_else(|h| panic!("{h}"));
    for k in 0..480u32 {
        if k >= 120 {
            run.bench.pads[0] = Pad {
                stick: Stick::new(0.0, 1.0),
                actions: [false; 4],
                connected: true,
            };
        }
        stops.extend(run.step_play().unwrap());
    }
    let minion = run.minion.expect("joined");
    assert_eq!(run.vm.stopped(minion), None, "his script runs on");
    assert!(run.health().is_some_and(|h| h > 0.0), "{:?}", run.health());
    let moved = run.hero().pivot.distance(start);
    println!(
        "start {start:?}; after {} steps {:?} ({moved:.2} m on); health {:?}",
        run.steps,
        run.hero().pivot,
        run.health()
    );
    assert!(moved > 1.0, "he walked: {moved}");
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

/// openspec AS.54: level 214's music by its own scripts on the TV's listing
/// (`scripts::MUSIC_SCRIPTS`), its sounds read off its own archive as the
/// TV's book reads its level's record (AR.30): its dynamic sound controller
/// starts its explore bed at the level's start, looping, the docks'
/// ambience under it; with the music scripts set apart
/// (`scripts::without_music`) no music plays. Nothing stops, Eruptor joined
/// and standing.
#[test]
#[ignore = "reads the Giants dump"]
fn level_214s_music_plays_from_its_start() {
    let started = std::time::Instant::now();
    println!("load at start: {}", load());
    let follow = |no_music: bool| {
        let setup = Setup {
            interface: true,
            props: true,
            sounds: true,
            no_music,
            ..Setup::default()
        };
        let (mut run, _found) = run_on_level(Mode::Play, NEXT, setup);
        let listing = run.vm.config().whitelist.clone();
        let mut stops = Vec::new();
        for _ in 0..2 {
            stops.extend(run.step_play().unwrap());
        }
        run.join().unwrap_or_else(|h| panic!("{h}"));
        let mut first: std::collections::BTreeMap<String, (u64, f32, bool)> = Default::default();
        for _ in 0..600 {
            stops.extend(run.step_play().unwrap());
            for p in run.world.plays() {
                let Some(name) = p.name.clone() else { continue };
                let rate = run.world.sequences()[p.sequence as usize].rate;
                let looping = run.world.play_params(p).looping;
                first.entry(name).or_insert((p.started, rate, looping));
            }
        }
        (first, stops, listing)
    };
    let (with, stops, listing) = follow(false);
    for script in [
        "snd_dynamic_sound_controller_214.ai",
        "snd_combat_watcher_214.ai",
        "snd_jukebox_controller.ai",
        "IGC_Music_Player.ai",
    ] {
        assert!(listing.lists("", &format!("C:/x/{script}")), "{script}");
    }
    let beds: Vec<(&String, &(u64, f32, bool))> = with
        .iter()
        .filter(|(n, _)| n.starts_with("mu_") || n.starts_with("am_"))
        .collect();
    println!("214's beds: {beds:?}; {} sounds started", with.len());
    let explore = with
        .get("mu_214_explore_v2")
        .expect("214's explore bed plays");
    assert!(explore.0 <= 2, "from the level's start: {explore:?}");
    assert_eq!(explore.1, 100.0);
    assert!(explore.2, "looping");
    assert!(with.contains_key("am_air_docks_wind_birds"), "the ambience");
    assert!(
        stops.is_empty(),
        "{:#?}",
        stops.iter().map(ToString::to_string).collect::<Vec<_>>()
    );
    let (without, stops, _) = follow(true);
    assert!(
        !without.keys().any(|n| n.starts_with("mu_")),
        "no music without its scripts: {:?}",
        without.keys().collect::<Vec<_>>()
    );
    assert!(stops.is_empty());
    println!(
        "{:.1} s; load at end: {}",
        started.elapsed().as_secs_f64(),
        load()
    );
}

/// Where Setup_001's first trigger stands on 214's first island, past the
/// dock, as its file places it: the hero set down here sets off the pods'
/// scene.
const POD_TRIGGER: [f32; 3] = [17.934004, -13.715616, 20.65423];
/// Among the pods, on the ground the Chompies run on.
const AMONG_THE_PODS: [f32; 3] = [55.0, -17.5, 35.0];

/// A beaten creature's or clone's hit points, as its scripts hold them.
fn hit_points(run: &Run, h: guac_script::Handle) -> Option<f32> {
    let vm = &run.vm;
    let id = vm.placement_of(h)?;
    let d = vm.local(id, "description").map(|v| vm.value(v))?;
    let c = vm.field(d, "creature info").map(|v| vm.value(v))?;
    vm.field(c, "hp").map(|v| vm.value(v))?.as_f32()
}

/// openspec AS.54: level 214's first encounter on the TV's listing for it
/// (`scripts::ENCOUNTERS`), fought with Eruptor, its palettes off the level
/// (AS.54's rule: every skinned model a placement names). Its two spawners
/// walk from the start; set down past Setup_001's trigger, the controller
/// plays the pods' scene and wakes them; among the pods, they clone Chompies
/// off the undead template, each walked as it's made; the Chompies notice
/// him and bite, and his attack (South, pulsed) beats them, paying his
/// figure experience (a spawner beaten too, some runs: printed). Nothing
/// stops.
#[test]
#[ignore = "reads the Giants dump"]
fn level_214s_first_encounter_is_fought_with_eruptor() {
    use guac_motion::stick::Stick;
    use guacamole::script_engine::Pad;
    use std::collections::BTreeMap;
    let started = std::time::Instant::now();
    println!("load at start: {}", load());
    let setup = Setup {
        interface: true,
        props: true,
        sounds: true,
        ..Setup::default()
    };
    let (mut run, _found) = run_on_level(Mode::Play, NEXT, setup);
    let mut stops = Vec::new();
    for _ in 0..2 {
        stops.extend(run.step_play().unwrap());
    }
    run.join().unwrap_or_else(|h| panic!("{h}"));
    let spawners: Vec<guac_script::Handle> = ["Enemy_Chompy_Spawner(1)", "Enemy_Chompy_Spawner(5)"]
        .iter()
        .map(|n| run.placed(n).0)
        .collect();
    assert_eq!(run.world.creatures(), spawners, "the spawners walk");
    let controller = run.placement("Setup_001_Controller");
    run.put(Vec3::from_array(POD_TRIGGER), 0.0);
    let mut scene = None;
    for _ in 0..400 {
        stops.extend(run.step_play().unwrap());
        if scene.is_none() && run.vm.behaviour(controller) == Some("chompy pod - cutscene out") {
            scene = Some(run.steps);
        }
    }
    let scene = scene.expect("the pods' scene");
    run.put(Vec3::from_array(AMONG_THE_PODS), 90.0);
    let health_before = run.health().unwrap();
    let xp_before = run.figure_values().map_or(0, |v| v.xp);
    // Each clone walked, by when it was first seen and when it went.
    let mut chompies: BTreeMap<guac_script::Handle, (u32, Option<u32>)> = BTreeMap::new();
    for k in 0..1200u32 {
        run.bench.pads[0] = Pad {
            stick: Stick::new(0.0, 0.0),
            actions: [k % 20 < 2, false, false, false],
            connected: true,
        };
        stops.extend(run.step_play().unwrap());
        for h in run.world.creatures() {
            if spawners.contains(&h) {
                continue;
            }
            let gone = run.world.actor(h).is_none_or(|a| a.removed)
                || run.vm.placement_of(h).is_none_or(|i| run.vm.removed(i));
            let e = chompies.entry(h).or_insert((run.steps, None));
            if gone && e.1.is_none() {
                e.1 = Some(run.steps);
            }
        }
    }
    let beaten = chompies.values().filter(|c| c.1.is_some()).count();
    let spawners_beaten = spawners
        .iter()
        .filter(|h| hit_points(&run, **h).is_some_and(|hp| hp <= 0.0))
        .count();
    let (health, xp) = (
        run.health().unwrap(),
        run.figure_values().map_or(0, |v| v.xp),
    );
    println!(
        "the scene at step {scene}; {} Chompies cloned, {beaten} beaten; spawners beaten {spawners_beaten}; health {health_before} -> {health}; experience {xp_before} -> {xp}",
        chompies.len()
    );
    for (h, (seen, gone)) in &chompies {
        println!(
            "  {h:#x} {}: from {seen}, gone {gone:?}",
            run.world.name(*h)
        );
    }
    assert!(
        chompies.len() >= 3,
        "the spawners clone Chompies: {chompies:?}"
    );
    assert!(
        chompies
            .keys()
            .all(|h| run.world.name(*h) == "_Template_Undead_Chompy"),
        "off the undead template"
    );
    assert!(beaten >= 2, "he beats them: {beaten}");
    assert!(health < health_before, "they bite: {health}");
    assert!(xp > xp_before, "his figure's paid: {xp}");
    assert_eq!(
        run.vm.stopped(run.minion.unwrap()),
        None,
        "his script runs on"
    );
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
