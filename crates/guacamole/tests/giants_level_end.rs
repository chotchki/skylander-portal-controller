//! AS.15's, AS.51's and AS.52's disc tests, headless with guacamole's
//! host: chapter 1's end, its outro played, asking for the next level by
//! name, and that level's own scripts running with Eruptor joined. Ignored by default; they read
//! `GUAC_GIANTS_DUMP` (process env, else `.env.dev`):
//!
//! `cargo test -p guacamole --profile disc --test giants_level_end -- --ignored --nocapture --test-threads=1`
//!
//! Chapter 1's end is woken as `Rope_around_Post(2)`'s last behaviour wakes
//! it (`Temp_Level_Ender`'s range and buffer to 0), the hero set down on the
//! outro's player waypoint, as the TV's dev entry's end anchor does; from
//! there every step is the scripts' (the fade, the outro woken and its
//! cutscene played to its end since AS.52, its own `Game - Level End`),
//! then `guacamole::level_end`'s level change.
//! (Stand-in ledger: `level-end`.)

use guac_motion::seam::Vec3;
use guac_script::{Halt, Mode, Value};
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
/// stepped in play mode until a level change is held or 2,000 steps pass
/// (the outro's camera shot 10 s since AS.78).
/// The scripts' own level end names `Level_214`, whose text
/// `Level_214_Tutotial_2` is the archive `level_214_tutotial_2`; the outro
/// plays to its end (AS.52); `system.load level` holds the request on later
/// steps; nothing stops.
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
            run.steps < woken_at + 2000,
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
            run.steps < woken_at + 2000,
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
    let mut start = run.hero().pivot;
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
    // Its intro skipped as a player would (AS.81).
    stops.extend(skip_214s_intro(&mut run));
    for k in 0..480u32 {
        if k == 120 {
            // Where his intro handed him over (openspec AS.47).
            start = run.hero().pivot;
        }
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
    // Its intro skipped as a player would (AS.81).
    stops.extend(skip_214s_intro(&mut run));
    let spawners: Vec<guac_script::Handle> = ["Enemy_Chompy_Spawner(1)", "Enemy_Chompy_Spawner(5)"]
        .iter()
        .map(|n| run.placed(n).0)
        .collect();
    // The spawners walk; the second fight's Chompies and elves walk from the
    // start too (openspec AS.58), and the second island's Tree Rex and four
    // Chompies (AS.75) and Setup_004's Mabu and five (AS.84), none of them a
    // clone, and nothing else.
    let placed = run.world.creatures();
    let mut walking: Vec<String> = placed.iter().map(|h| run.world.name(*h)).collect();
    walking.sort_unstable();
    assert_eq!(
        walking,
        [
            "Enemy_Chompy2_E",
            "Enemy_Chompy2_E(1)",
            "Enemy_Chompy2_E(2)",
            "Enemy_Chompy2_E(3)",
            "Enemy_Chompy2_E(4)",
            "Enemy_Chompy2_F",
            "Enemy_Chompy2_G",
            "Enemy_Chompy2_H",
            "Enemy_Chompy_Spawner(1)",
            "Enemy_Chompy_Spawner(2)",
            "Enemy_Chompy_Spawner(5)",
            "Enemy_Elf_Near_A",
            "Enemy_Elf_Near_B",
            "Enemy_Elf_Near_C",
            "Enemy_Elf_Near_S2(3)",
            "Enemy_Elf_Near_S2(4)",
            "Enemy_TreeRex(1)",
            "Enemy_TreeRex(2)",
            "Setup_004_NPC_Mabu",
        ],
        "the spawners and the later fights' own walk"
    );
    assert!(spawners.iter().all(|s| placed.contains(s)));
    let controller = run.placement("Setup_001_Controller");
    // His arrival and the intro's hand-over played out, as the TV's dev
    // entry settles him (openspec AS.47).
    for _ in 0..120 {
        stops.extend(run.step_play().unwrap());
    }
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
            if placed.contains(&h) {
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

/// Where level 214's first bomb spawner stands (metres, its file's place).
const BOMB_SPAWNER: [f32; 3] = [39.005, -16.018, 27.887];
/// Short of the castle wall in front of the pods, facing it.
const BEFORE_THE_WALL: ([f32; 3], f32) = ([50.0, -17.9, 30.0], 77.7);

/// openspec AS.57: level 214's way to its pods, on the TV's listing for it
/// (`scripts::BOMB_SCRIPTS`, `BOMB_HINTS`). The first spawner clones a bomb;
/// Eruptor coming within its 10 ft takes it, and it rides 12 ft over him;
/// carried before the castle wall that shuts the pods in, a tap of his
/// fourth action (△, pulsed a step) throws it (`bomb`), and its burst opens
/// the wall (`Breakable_Stone_Wall_Castle_206(1)` taken off, its bricks and
/// pieces cloned), its solid gone from the seam. The spawner clones no
/// second bomb while the first stands. Nothing stops.
#[test]
#[ignore = "reads the Giants dump"]
fn level_214s_wall_opens_to_a_bomb() {
    use guac_motion::stick::Stick;
    use guacamole::script_engine::Pad;
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
    // Its intro skipped as a player would (AS.81).
    stops.extend(skip_214s_intro(&mut run));
    for _ in 0..60 {
        stops.extend(run.step_play().unwrap());
    }
    let bombs = |run: &Run| -> Vec<guac_script::Handle> {
        run.world
            .clones()
            .into_iter()
            .filter(|h| run.world.name(*h) == "Bomb_Template")
            .filter(|h| run.world.actor(*h).is_some_and(|a| !a.removed))
            .collect()
    };
    let behaviour = |run: &Run, h: guac_script::Handle| {
        run.vm
            .placement_of(h)
            .and_then(|id| run.vm.behaviour(id))
            .map(str::to_owned)
    };
    // Set down 3 m short of the spawner: the bomb is his.
    let at = Vec3::from_array(BOMB_SPAWNER) - Vec3::X * 3.0;
    run.put(at, 90.0);
    let mut carried = None;
    for _ in 0..240 {
        stops.extend(run.step_play().unwrap());
        if carried.is_none()
            && let Some(&b) = bombs(&run).first()
            && behaviour(&run, b).as_deref() == Some("following player")
        {
            carried = Some((b, run.steps));
        }
    }
    let (bomb, taken) = carried.expect("the bomb taken and carried");
    // The first spawner's: each clone of a bomb starts where it was cloned.
    let from_first = |run: &Run| -> Vec<guac_script::Handle> {
        bombs(run)
            .into_iter()
            .filter(|h| {
                run.world
                    .actor(*h)
                    .is_some_and(|a| a.origin.distance(Vec3::from_array(BOMB_SPAWNER)) < 3.0)
            })
            .collect()
    };
    assert_eq!(from_first(&run), [bomb], "one bomb from the spawner");
    let over = run.world.actor(bomb).unwrap().position - run.hero().pivot;
    println!("carried from step {taken}, {over} over his pivot");
    assert!(
        (over.y - 12.0 / guacamole::script_engine::FEET_PER_METRE).abs() < 0.05
            && over.x.abs() < 0.01
            && over.z.abs() < 0.01,
        "12 ft over him: {over}"
    );
    let (wall, _) = run.placed("Breakable_Stone_Wall_Castle_206(1)");
    let wall_object = run
        .files
        .placed
        .iter()
        .find(|p| p.name == "Breakable_Stone_Wall_Castle_206(1)")
        .unwrap()
        .obj
        .index;
    assert!(
        run.bench
            .props
            .as_ref()
            .unwrap()
            .standing
            .contains(&wall_object),
        "the wall stands, solid"
    );
    let (to, facing) = BEFORE_THE_WALL;
    run.put(Vec3::from_array(to), facing);
    for _ in 0..30 {
        stops.extend(run.step_play().unwrap());
    }
    assert_eq!(
        behaviour(&run, bomb).as_deref(),
        Some("following player"),
        "still carried"
    );
    let mut thrown = None;
    let mut opened = None;
    for k in 0..300u32 {
        run.bench.pads[0] = Pad {
            stick: Stick::new(0.0, 0.0),
            actions: [false, false, false, k == 0],
            connected: true,
        };
        stops.extend(run.step_play().unwrap());
        if thrown.is_none() && behaviour(&run, bomb).as_deref() == Some("bomb") {
            thrown = Some(run.steps);
        }
        if opened.is_none() && run.world.actor(wall).is_some_and(|a| a.removed) {
            opened = Some(run.steps);
        }
    }
    let thrown = thrown.expect("△ throws it");
    let opened = opened.expect("its burst opens the wall");
    let pieces = run
        .world
        .clones()
        .into_iter()
        .filter(|h| {
            let n = run.world.name(*h);
            n == "StoneWall_Brick_Castle_206" || n.starts_with("Wall Piece")
        })
        .count();
    println!(
        "thrown at step {thrown}, the wall opened at {opened}, {pieces} pieces; {:.1} s; load at end: {}",
        started.elapsed().as_secs_f64(),
        load()
    );
    assert!(pieces >= 6, "the wall falls in pieces: {pieces}");
    assert!(
        !run.bench
            .props
            .as_ref()
            .unwrap()
            .standing
            .contains(&wall_object),
        "its solid gone"
    );
    assert!(
        stops.is_empty(),
        "{:#?}",
        stops.iter().map(ToString::to_string).collect::<Vec<_>>()
    );
}

/// Where level 214's intro hands the player over (metres, the run's own
/// read): its cutscene, played to its end or skipped, sets him down on the
/// deck here, 4.3 m from the level's `InitializeLevel.ai` marker.
const HANDED_OVER: [f32; 3] = [-9.144, -12.578118, 18.10268];

/// The intro's lines, in the order its scenes voice them.
const INTRO_LINES: [&str; 10] = [
    "vo_flynn_lvl214_igc01_002",
    "vo_cali_lvl214_igc01_003a",
    "vo_flynn_lvl214_igc01_004",
    // The fourth to sixth scenes' (their animator cameras' shots read since
    // AS.78: until then each lasted a step and voiced nothing).
    "vo_flynn_lvl214_igc01_004a",
    "vo_flynn_lvl214_igc01_005",
    "vo_blobbers_general_028",
    "vo_flynn_lvl214_igc01_006",
    "vo_flynn_lvl214_igc01_013",
    "vo_cali_lvl214_igc01_013a",
    "vo_flynn_lvl214_igc01_014",
];

/// Level 214 entered with Eruptor joined, its intro held for him as the
/// TV's install holds it: the run, its stops, the intro's placement and
/// the ship's parts standing before the join.
fn joined_at_214s_intro() -> (Run, Vec<Halt>, guac_script::PlacementId, Vec<String>) {
    let setup = Setup {
        interface: true,
        props: true,
        sounds: true,
        ..Setup::default()
    };
    let (mut run, _found) = run_on_level(Mode::Play, NEXT, setup);
    let intro = run.placement("214_Intro_IGC");
    let mut stops = Vec::new();
    for _ in 0..2 {
        stops.extend(run.step_play().unwrap());
    }
    assert!(run.vm.is_suspended(intro), "held until a hero joins");
    assert_eq!(run.vm.behaviour(intro), None);
    assert_eq!(cutscene_name(&run), None);
    let parts = ship_parts(&run);
    run.join().unwrap_or_else(|h| panic!("{h}"));
    (run, stops, intro, parts)
}

/// What stands on `The_Ship_1`, by name.
fn ship_parts(run: &Run) -> Vec<String> {
    let (ship, _) = run.placed("The_Ship_1");
    let mut p: Vec<String> = run
        .world
        .clones()
        .into_iter()
        .filter(|h| {
            run.world
                .actor(*h)
                .is_some_and(|a| !a.removed && a.attach == Value::Entity(ship))
        })
        .map(|h| run.world.name(h))
        .collect();
    p.sort();
    p
}

/// After 214's intro, played or skipped: the ship and what its STARTUP took
/// off stand again, its parts once each, the intro's ship clones off
/// (AS.60 and its review).
fn the_ship_stands_after_the_intro(run: &Run, parts_before: &[String]) {
    let (ship, _) = run.placed("The_Ship_1");
    let (template, _) = run.placed("214_Intro_Ship");
    let ship_clones: Vec<(guac_script::Handle, bool)> = run
        .world
        .clones()
        .into_iter()
        .filter(|h| {
            run.world.actor(*h).and_then(|a| a.template)
                == run.world.actor(template).and_then(|a| a.obj)
        })
        .map(|h| (h, run.world.removed(h)))
        .collect();
    let back: Vec<(&str, bool)> = [
        "The_Ship_1",
        "Hatch",
        "barrel_b_intact(17)",
        "barrel_b_intact(18)",
    ]
    .iter()
    .map(|n| (*n, !run.world.removed(run.placed(n).0)))
    .collect();
    let parts_after = ship_parts(run);
    println!(
        "standing after: {back:?}; its parts before {parts_before:?}, after {parts_after:?}; the intro ship's clones (taken off): {ship_clones:?}"
    );
    assert!(back.iter().all(|(_, on)| *on), "back on after it: {back:?}");
    let ship_run = run.vm.placement_of(ship).expect("the ship runs");
    assert!(!run.vm.removed(ship_run), "its script runs again");
    assert_eq!(
        parts_after, parts_before,
        "its parts on it again, once each"
    );
    for part in ["Hub_Engine", "Hub_Masthead", "Hub_Roof"] {
        assert!(parts_after.iter().any(|p| p == part), "{part}");
    }
    let standing = |name: &str| {
        run.world
            .clones()
            .into_iter()
            .filter(|h| run.world.name(*h) == name && !run.world.removed(*h))
            .count()
    };
    for part in [
        "Hub_Engine",
        "Hub_Masthead",
        "Hub_Roof",
        "00_Engine_DefaultStatic",
        "01_Engine_Default_OarLeft",
        "02_Engine_Default_OarRight",
        "03_Engine_Default_Propeller",
    ] {
        assert_eq!(standing(part), 1, "{part} standing once");
    }
    assert!(
        !ship_clones.is_empty() && ship_clones.iter().all(|(_, off)| *off),
        "its cutscene's ship clones off: {ship_clones:?}"
    );
}

/// openspec AS.47, AS.60, AS.81: level 214's own intro (`214_Intro_IGC`),
/// on the TV's listing (`scripts::INTROS`), held until Eruptor's minion is
/// in, then played to its end (`level_end::SKIPPED_CUTSCENES` lists it no
/// more): its ten scenes in order, every scene object reading its actor
/// (Flynn's and Cali's, their definitions listed since AS.82: until then
/// the sixth scene's walk held on an object reading none), its ten lines
/// voiced in order, each its recording's length and over by the cutscene's
/// end (Flynn's sixth scene's line within that scene, where the held walk
/// left it on), the hand-over on the deck, the ship and what
/// its STARTUP took off standing again (AS.60), the HUD back as the last
/// line's box fades. Nothing stops.
#[test]
#[ignore = "reads the Giants dump"]
fn level_214s_intro_plays_to_its_end_on_the_ship() {
    let started = std::time::Instant::now();
    println!("load at start: {}", load());
    assert!(!level_end::SKIPPED_CUTSCENES.contains(&"214 Intro IGC"));
    let (mut run, mut stops, intro, parts_before) = joined_at_214s_intro();
    let marker = run.hero().pivot;
    let joined_at = run.steps;
    let (ship, _) = run.placed("The_Ship_1");
    let (mut began, mut ended, mut gone, mut shown, mut off) = (None, None, None, None, None);
    let mut scenes: Vec<(usize, u32)> = Vec::new();
    let mut unnamed: Vec<(u32, usize)> = Vec::new();
    let mut actors: std::collections::BTreeSet<String> = Default::default();
    let mut lines = Lines::default();
    while gone.is_none() || shown.is_none() {
        assert!(run.steps < joined_at + 6000, "no end by step {}", run.steps);
        stops.extend(run.step_play().unwrap());
        lines.record(&run);
        if off.is_none() && run.world.removed(ship) {
            off = Some(run.steps);
        }
        match scene_of(&run) {
            Some((k, n)) => {
                assert_eq!(n, 10, "the intro's ten scenes");
                began.get_or_insert(run.steps);
                if scenes.last().is_none_or(|(at, _)| *at != k) {
                    scenes.push((k, run.steps));
                }
                for (i, a) in scene_actors(&run).into_iter().enumerate() {
                    match a {
                        Some(a) => {
                            actors.insert(a);
                        }
                        None => unnamed.push((run.steps, i)),
                    }
                }
            }
            None if began.is_some() && ended.is_none() => ended = Some(run.steps),
            None => {}
        }
        if gone.is_none() && run.vm.removed(intro) {
            gone = Some(run.steps);
        }
        if gone.is_some() && shown.is_none() && !guacamole::scripts::player_hud_hidden(&run.vm) {
            shown = Some(run.steps);
        }
    }
    let at = run.hero().pivot;
    let ended = ended.expect("the cutscene ends");
    for (k, (_, from)) in scenes.iter().enumerate() {
        let to = scenes.get(k + 1).map_or(ended, |(_, n)| *n);
        println!(
            "scene {k}: from {from}, {} steps ({:.2} s)",
            to - from,
            f64::from(to - from) / 60.0
        );
    }
    let voices: Vec<(&str, u64, Option<u64>, f32)> = lines
        .voices
        .iter()
        .map(|v| (v.sound.as_str(), v.started, v.ended, v.length))
        .collect();
    println!(
        "joined at {joined_at}; the cutscene from {began:?} to {ended}; The_Ship_1 off at {off:?}; the intro off at {gone:?}, the HUD shown at {shown:?}; handed over at {at:?} ({:.2} m off the marker); actors {actors:?}; voices {voices:?}; boxes {:?}",
        at.distance(marker),
        lines.boxes
    );
    assert!(began.is_some_and(|b| b <= joined_at + 2), "{began:?}");
    assert_eq!(
        scenes.iter().map(|(k, _)| *k).collect::<Vec<_>>(),
        (0..10).collect::<Vec<_>>(),
        "every scene, in order, none skipped"
    );
    // AS.78: the first and the fourth to sixth scenes are animator camera
    // shots (`214_Intro.mdl`'s slots 0, 2, 3 and 4: 3.00, 2.30, 1.97 and
    // 3.97 s), each lasting its clip; until AS.78 a step each.
    let length = |k: usize| scenes.get(k + 1).map_or(ended, |(_, n)| *n) - scenes[k].1;
    for (k, clip) in [(0, 3.0), (3, 2.3), (4, 1.966_666_7), (5, 3.966_666_7)] {
        let want = clip * 60.0;
        assert!(
            (f64::from(length(k)) - want).abs() <= 2.0,
            "scene {k}: {} steps, its camera's clip {want:.0}",
            length(k)
        );
    }
    assert!(unnamed.is_empty(), "objects reading no actor: {unnamed:?}");
    for who in ["Flynn_NPC", "Cali_NPC", "009_Eruptor"] {
        assert!(actors.contains(who), "{who} in its scenes: {actors:?}");
    }
    assert!(off.is_some(), "the intro's STARTUP takes the ship off");
    the_ship_stands_after_the_intro(&run, &parts_before);
    // Its lines in order, each over within the scene it starts in.
    assert_eq!(
        lines
            .voices
            .iter()
            .map(|v| v.sound.as_str())
            .collect::<Vec<_>>(),
        INTRO_LINES
    );
    for v in &lines.voices {
        assert!(
            v.ended.is_some_and(|e| e <= u64::from(ended)),
            "{} on past the cutscene's end at {ended}: {:?}",
            v.sound,
            v.ended
        );
        // Played whole: none cut short by a scene's end.
        let played = v.ended.unwrap() - v.started;
        assert!(
            played as f32 >= v.length * 60.0 - 2.0,
            "{} played {played} steps of {:.2} s",
            v.sound,
            v.length
        );
    }
    // The sixth scene's line (Flynn's) over within that scene: the held
    // walk left it on (AS.81).
    let sixth = (scenes[6].1, scenes[7].1);
    let line = lines
        .voices
        .iter()
        .find(|v| v.sound == INTRO_LINES[6])
        .expect("the sixth scene's line");
    assert!(
        line.started >= u64::from(sixth.0) && line.ended.is_some_and(|e| e <= u64::from(sixth.1)),
        "{line:?} in scene 6 {sixth:?}"
    );
    // The HUD back as the last line's box fades, after the hand-over.
    let last_box = lines.boxes.iter().filter_map(|b| b.gone).max();
    assert!(
        last_box.is_some_and(|g| shown.is_some_and(|s| s >= g && s <= g + 3)),
        "{shown:?} {:?}",
        lines.boxes
    );
    assert!(shown.unwrap() >= gone.unwrap(), "{shown:?} {gone:?}");
    let handed = Vec3::from_array(HANDED_OVER);
    assert!(
        Vec3::new(at.x - handed.x, 0.0, at.z - handed.z).length() < 0.05,
        "set down where the hand-over puts him: {at:?}"
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

/// openspec AS.81, AS.69: the original's own skip still ends 214's intro
/// once it plays: △ (the cutscene system's check, a joined player's `action
/// 4` just pressed with the cutscene past its first second) pressed in its
/// second scene ends it there, Flynn's line with it, and hands over where
/// playing it does, the ship standing as after it; nothing else takes the
/// press (no box after the skip, the HUD back within a few steps), and a
/// press in its first second skips nothing. Nothing stops.
#[test]
#[ignore = "reads the Giants dump"]
fn level_214s_intro_skips_on_triangle() {
    let started = std::time::Instant::now();
    println!("load at start: {}", load());
    let (mut run, mut stops, intro, parts_before) = joined_at_214s_intro();
    let mut lines = Lines::default();
    let press = |run: &mut Run, down: bool| {
        run.bench.pads[0] = guacamole::script_engine::Pad {
            stick: guac_motion::stick::Stick::new(0.0, 0.0),
            actions: [false, false, false, down],
            connected: true,
        };
    };
    // A press in its first second: nothing skipped.
    for k in 0..30 {
        press(&mut run, k == 20);
        stops.extend(run.step_play().unwrap());
        lines.record(&run);
    }
    assert_eq!(cutscene_name(&run).as_deref(), Some("214 Intro IGC"));
    // Into its second scene, then △.
    while scene_of(&run).is_none_or(|(k, _)| k < 1) || run.steps < 120 {
        press(&mut run, false);
        stops.extend(run.step_play().unwrap());
        lines.record(&run);
    }
    let pressed = run.steps;
    press(&mut run, true);
    stops.extend(run.step_play().unwrap());
    lines.record(&run);
    let (mut skipped, mut gone, mut shown) = (None, None, None);
    for _ in 0..60 {
        press(&mut run, false);
        stops.extend(run.step_play().unwrap());
        lines.record(&run);
        if skipped.is_none() && cutscene_name(&run).is_none() {
            skipped = Some(run.steps);
        }
        if gone.is_none() && run.vm.removed(intro) {
            gone = Some(run.steps);
        }
        if gone.is_some() && shown.is_none() && !guacamole::scripts::player_hud_hidden(&run.vm) {
            shown = Some(run.steps);
        }
    }
    let at = run.hero().pivot;
    println!(
        "△ at {pressed}; the cutscene over at {skipped:?}, the intro off at {gone:?}, the HUD at {shown:?}; handed over at {at:?}; voices {:?}; boxes {:?}",
        lines
            .voices
            .iter()
            .map(|v| (v.sound.as_str(), v.started, v.ended))
            .collect::<Vec<_>>(),
        lines.boxes
    );
    let skip = skipped.expect("△ skips it");
    assert!(skip <= pressed + 2, "{skip} {pressed}");
    assert!(gone.is_some_and(|g| g <= skip + 2), "{gone:?}");
    assert!(shown.is_some_and(|s| s <= skip + 3), "{shown:?}");
    assert!(
        lines
            .voices
            .iter()
            .all(|v| v.ended.is_some_and(|e| e <= u64::from(skip))),
        "a line on past the skip: {:?}",
        lines.voices
    );
    assert!(
        lines
            .boxes
            .iter()
            .all(|b| b.made < skip && b.gone.is_some_and(|g| g <= skip + 1)),
        "{:?}",
        lines.boxes
    );
    the_ship_stands_after_the_intro(&run, &parts_before);
    let handed = Vec3::from_array(HANDED_OVER);
    assert!(
        Vec3::new(at.x - handed.x, 0.0, at.z - handed.z).length() < 0.05,
        "set down where the hand-over puts him: {at:?}"
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

/// The outro's Mabu (`213_Outro_Mabu_A` to `_C`) and the placement whose
/// waypoints each one's walk follows.
const OUTRO_MABU: [(&str, &str); 3] = [
    ("213_Outro_Mabu_A", "Outro_Mabu_1_WP"),
    ("213_Outro_Mabu_B", "Outro_Mabu_2_WP"),
    ("213_Outro_Mabu_C", "Outro_Mabu_3_WP"),
];

/// openspec AS.52: chapter 1's outro (`213 Outro IGC`) played to its end on
/// the TV's host, its sounds and the interface's model in, nothing skipped
/// (`level_end::SKIPPED_CUTSCENES` lists it no more): its four scenes in
/// order, Norticus' animations started on him from his palette, the Mabu
/// walked to their last waypoints by `move to` with no creature's body
/// under them, the actors on their own scripts (140 %, a hat each), every
/// line voiced in order; then its own `Game - Level End`
/// names level 214, which the TV asks for. Nothing stops. The second scene
/// lasts its animator camera's clip (AS.78: 10.0 s, past one Havok block).
#[test]
#[ignore = "reads the Giants dump"]
fn chapter_1s_outro_plays_to_its_end() {
    assert!(!level_end::SKIPPED_CUTSCENES.contains(&"213 Outro IGC"));
    let started = std::time::Instant::now();
    println!("load at start: {}", load());
    let setup = Setup {
        interface: true,
        sounds: true,
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
    let woken = run.steps;
    let mut lines = Lines::default();
    let mut scenes: Vec<(usize, u32)> = Vec::new();
    let mut ended = None;
    let mut since = run.world.start_count();
    let mut clips: Vec<(u32, String)> = Vec::new();
    let (mut hats, mut scales) = (0usize, Vec::new());
    while run.world.level_change().is_none() {
        assert!(
            run.steps < woken + 2000,
            "no level change by step {}",
            run.steps
        );
        stops.extend(run.step_play().unwrap());
        lines.record(&run);
        // Their own scripts: each actor at 140 %, his hat on his head.
        let on: Vec<String> = run
            .world
            .clones()
            .into_iter()
            .filter(|h| !run.world.removed(*h))
            .map(|h| run.world.name(h))
            .collect();
        hats = hats.max(
            on.iter()
                .filter(|n| *n == "Turban_hat" || *n == "Propellar_Beenie")
                .count(),
        );
        let actor = |n: &str| n == "213_Outro_Norticus_Actor" || n.starts_with("213_Outro_Mabu_");
        // The step all four are made, then the next: made by the outro's
        // tick, which the wake check runs after the walk (2.0's AS.83), they
        // first tick, and scale themselves, at the next step's walk.
        if scales.len() < 2 && on.iter().filter(|n| actor(n)).count() == 4 {
            scales.push(
                run.world
                    .clones()
                    .into_iter()
                    .filter(|h| !run.world.removed(*h) && actor(&run.world.name(*h)))
                    .map(|h| run.world.actor(h).unwrap().scale[0])
                    .collect::<Vec<f32>>(),
            );
        }
        match scene_of(&run) {
            Some((k, n)) => {
                assert_eq!(n, 4, "the outro's four scenes");
                if scenes.last().is_none_or(|(at, _)| *at != k) {
                    scenes.push((k, run.steps));
                }
            }
            None if !scenes.is_empty() && ended.is_none() => ended = Some(run.steps),
            None => {}
        }
        for i in run.world.started_since(since) {
            let sq = &run.world.sequences()[i];
            // A clip (a palette slot's object), not a line's sound.
            if run.world.name(sq.actor) == "213_Outro_Norticus_Actor"
                && matches!(sq.template, Value::Entity(_))
            {
                clips.push((run.steps, format!("{:.2} s", sq.length)));
            }
        }
        since = run.world.start_count();
    }
    let change = run.world.level_change().cloned().unwrap();
    let voices: Vec<(String, u64)> = lines
        .voices
        .iter()
        .filter(|v| v.started >= u64::from(woken))
        .map(|v| (v.sound.clone(), v.started))
        .collect();
    // AR.17's check kept (the as-ch1-ending review): no line plays on at the
    // level change. Until AS.78 Norticus' last did, its scene a single step
    // with its animator camera's clip unread.
    let playing: Vec<String> = run
        .world
        .plays()
        .iter()
        .filter_map(|p| p.name.clone())
        .filter(|n| n.starts_with("vo_"))
        .collect();
    println!("lines playing at the change: {playing:?}");
    assert!(playing.is_empty(), "{playing:?}");
    println!(
        "woken at {woken}; scenes (index, first step) {scenes:?}; the cutscene over at {ended:?}; \
         Norticus' clips {clips:?}; voices {voices:?}; the change at {}: {change:?}",
        run.steps
    );
    assert_eq!(
        scenes.iter().map(|(k, _)| *k).collect::<Vec<_>>(),
        [0, 1, 2, 3],
        "every scene, in order, none skipped"
    );
    let ended = ended.expect("the cutscene ends");
    for (k, (_, at)) in scenes.iter().enumerate() {
        let next = scenes.get(k + 1).map_or(ended, |(_, n)| *n);
        println!(
            "scene {k}: {} steps ({:.2} s)",
            next - at,
            f64::from(next - at) / 60.0
        );
    }
    // AS.78: the second scene lasts its animator camera's clip (301 frames
    // at 30 a second, 10.0 s, read past its first Havok block), and
    // Norticus' last line, voiced as it opens, is over by its end or a
    // couple of seconds into the next (its 11.78 s is the scene's and the
    // fade's).
    let second = scenes[2].1 - scenes[1].1;
    assert!((599..=603).contains(&second), "the camera's shot: {second}");
    let last = lines
        .voices
        .iter()
        .find(|v| v.sound == "vo_norticus_lvl213_igc24_005")
        .expect("his last line");
    println!("his last line: {last:?}");
    assert!(
        last.ended.is_some_and(
            |e| e <= u64::from(scenes[2].1) + 150 && last.started + 5 >= u64::from(scenes[1].1)
        ),
        "{last:?} against the second scene {:?}",
        (scenes[1].1, scenes[2].1)
    );
    // The first scene's walks run to the end: each Mabu stopped within its
    // walk's 20 ft of its path's last waypoint (a walk not skipped leaves
    // him where he reached it).
    for (name, path) in OUTRO_MABU {
        let last = run
            .files
            .placed
            .iter()
            .find(|p| p.name == path)
            .and_then(|p| p.waypoints.last())
            .and_then(|w| run.files.points.get(w))
            .copied()
            .unwrap_or_else(|| panic!("{path}'s last waypoint"));
        let (x, z) = (last.x, last.z);
        let h = run
            .world
            .clones()
            .into_iter()
            .find(|h| run.world.name(*h) == name)
            .unwrap_or_else(|| panic!("{name} cloned"));
        let at = run.world.actor(h).unwrap().position;
        let off = (at.x - x).hypot(at.z - z);
        println!("{name} stopped {off:.2} m from {path}'s last waypoint");
        assert!(
            off <= 20.0 * 0.3048 + 0.01 && off > 5.0,
            "{name} at {at}: {off}"
        );
    }
    assert!(clips.len() >= 3, "Norticus' animations: {clips:?}");
    println!("hats on at once: {hats}; the actors' scales as made and a step on: {scales:?}");
    assert_eq!(hats, 4, "Norticus' turban and the three Mabu's beanies");
    assert_eq!(
        scales.get(1).map(Vec::as_slice),
        Some([140.0; 4].as_slice()),
        "140 % by their own scripts' first tick"
    );
    assert_eq!(
        voices.iter().map(|(v, _)| v.as_str()).collect::<Vec<_>>(),
        [
            "vo_norticus_lvl213_igc24_001",
            "vo_snucklesicus_lvl213_igc24_002",
            "vo_rizzopolus_lvl213_igc24_003",
            "vo_norticus_lvl213_igc24_005",
        ]
    );
    let to = level_end::recorded(&run.vm).expect("the level end recorded");
    assert_eq!(
        match to {
            Value::Object(o) => run.vm.program().name_of(o),
            _ => None,
        },
        Some("Level_214")
    );
    assert_eq!(change.name, NEXT);
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
