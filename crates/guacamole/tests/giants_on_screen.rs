//! The on-screen test's readers on the disc (openspec AH.107), counted
//! for the host's answer (stand-in ledger: `on-screen`): every script the
//! whitelist runs, with AH.34's
//! route scripts, AG.60's start scripts and AG.28's camera volumes and
//! activator, read for asks of a point
//! against the gameplay camera ([`guacamole::on_screen`]), each with the
//! list and macro it sits in.

mod common;

use std::collections::BTreeMap;

use common::*;
use guac_script::{Mode, Program, ScriptId, Whitelist};
use guacamole::on_screen::{self, Ask};
use guacamole::scripts;

/// AH.34's 23: the scripts AH.8's route ran off AH.12's roots.
const ROUTE: [&str; 23] = [
    "StoneWall_2.ai",
    "StoneWall_Piece.ai",
    "Pan_Show_Norticu.ai",
    "Spyro_CutsceneManager.ai",
    "Pan_Show_Cannon.ai",
    "Pop_to_Camera_5.ai",
    "Hints_Passive.ai",
    "Name_Changer.ai",
    "snd_combat_watcher_213.ai",
    "Placed_Loot.ai",
    "Boulder_Pebbles.ai",
    "Butterfly.ai",
    "Chest_Lock.ai",
    "Activator_Sliding_Rocks.ai",
    "Falling_Rocks.ai",
    "PR_PvP_jumpPad.ai",
    "Hook_Script.ai",
    "Bird_Ambient_new.ai",
    "Bug_Spawner.ai",
    "Leaves_Blowing_Controller.ai",
    "Goat_Running.ai",
    "Eon_Line_Activator.ai",
    "Goat_Jumping_Pit.ai",
];

/// The scripts a whitelist runs: those it lists and those of the
/// placements it names.
fn roots(program: &Program, run: &Run, w: &Whitelist) -> Vec<ScriptId> {
    let Whitelist::Only {
        placements,
        scripts,
        first_tick,
    } = w
    else {
        panic!("the TV's whitelist lists");
    };
    let mut out: Vec<ScriptId> = Vec::new();
    for (i, s) in program.scripts.iter().enumerate() {
        if scripts
            .iter()
            .chain(first_tick)
            .any(|l| s.name.ends_with(l.as_str()))
        {
            out.push(i as ScriptId);
        }
    }
    for p in &run.files.placed {
        if placements.contains(&p.name)
            && let Some(s) = p.script.as_deref()
            && let Some(i) = program.scripts.iter().position(|x| x.name == s)
        {
            out.push(i as ScriptId);
        }
    }
    out.sort_unstable();
    out.dedup();
    out
}

fn short(path: &str) -> &str {
    path.rsplit('/').next().unwrap_or(path)
}

fn place(program: &Program, a: &Ask) -> String {
    let s = &program.scripts[a.script as usize];
    match a.def {
        Some(d) => format!(
            "{} #{} in '{}'",
            short(&s.name),
            a.position,
            program.defs[d as usize].display
        ),
        None => format!("{} #{}", short(&s.name), a.position),
    }
}

/// Each ask the roots reach, keyed by its list and macro, with the roots
/// reaching it: one reader read once however many roots share it.
fn asks_of(program: &Program, roots: &[ScriptId]) -> BTreeMap<String, Vec<String>> {
    let mut out: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for &r in roots {
        let (asks, _) = on_screen::reached(program, r);
        for a in asks {
            out.entry(place(program, &a))
                .or_default()
                .push(short(&program.scripts[r as usize].name).to_owned());
        }
    }
    out
}

#[test]
#[ignore = "disc: GUAC_GIANTS_DUMP (AH.107)"]
fn the_whitelists_on_screen_readers() {
    let started = std::time::Instant::now();
    let (run, _) = run_on(Mode::Strict);
    let program = run.vm.program_arc();
    let every = on_screen::every_ask(&program);
    println!("every ask the part's files hold ({}):", every.len());
    for a in &every {
        println!("  {}", place(&program, a));
    }

    let tv = roots(&program, &run, &scripts::smash_config(Mode::Play).whitelist);
    let fight = roots(&program, &run, &scripts::fight_config(Mode::Play).whitelist);
    let route: Vec<ScriptId> = ROUTE
        .iter()
        .filter_map(|s| program.script_named(s))
        .collect();
    assert_eq!(route.len(), ROUTE.len(), "every route script in the part");
    let on = |label: &str, roots: &[ScriptId]| {
        let asks = asks_of(&program, roots);
        println!(
            "{label}: {} roots, {} asks reached",
            roots.len(),
            asks.len()
        );
        for (at, by) in &asks {
            println!("  {at}, reached from {}", by.join(", "));
        }
        asks
    };
    let tv_asks = on("the TV's whitelist", &tv);
    let fight_asks = on("the fight's whitelist", &fight);
    let route_asks = on("AH.34's route scripts", &route);
    let listed: Vec<&str> = scripts::START_PROPS
        .iter()
        .chain(&scripts::START_PROPS_FIRST_TICK)
        .copied()
        .collect();
    let start: Vec<ScriptId> = listed
        .iter()
        .filter_map(|s| program.script_named(s))
        .collect();
    assert_eq!(start.len(), listed.len(), "every start script in the part");
    let start_asks = on("AG.60's start scripts", &start);
    let camera: Vec<ScriptId> = scripts::CAMERA_SCRIPTS
        .iter()
        .filter_map(|s| program.script_named(s))
        .collect();
    assert_eq!(
        camera.len(),
        scripts::CAMERA_SCRIPTS.len(),
        "every camera script in the part"
    );
    let camera_asks = on("AG.28's camera volumes and activator", &camera);
    println!(
        "read in {:.1} s; {}",
        started.elapsed().as_secs_f64(),
        load()
    );

    // The orbs' idle bounce and the magma ball's roll are the whitelist's
    // two readers, the TV's and the fight's alike.
    let keys = |m: &BTreeMap<String, Vec<String>>| m.keys().cloned().collect::<Vec<_>>();
    assert_eq!(keys(&tv_asks), keys(&fight_asks));
    assert_eq!(tv_asks.len(), 2, "{tv_asks:?}");
    assert!(
        tv_asks
            .keys()
            .any(|k| k.contains("XP Balls - Update Macro - Idle")),
        "{tv_asks:?}"
    );
    assert!(
        tv_asks.keys().any(|k| k.starts_with("009_MagmaBall.ai")),
        "{tv_asks:?}"
    );
    // AH.34's adds the ambient birds' own two.
    let birds: Vec<&String> = route_asks
        .keys()
        .filter(|k| !tv_asks.contains_key(*k))
        .collect();
    assert_eq!(birds.len(), 2, "{route_asks:?}");
    assert!(birds.iter().all(|k| k.starts_with("Bird_Ambient_new.ai")));
    // AG.60's add none: what they reach the whitelist reaches already (the
    // orbs' bounce, through the award macros).
    assert!(
        start_asks.keys().all(|k| tv_asks.contains_key(k)),
        "{start_asks:?}"
    );
    // AG.28's ask none (openspec AG.28): the camera's frame and what the
    // volumes and the activator call place the viewport, they don't test a
    // point against it.
    assert!(camera_asks.is_empty(), "{camera_asks:?}");
}
