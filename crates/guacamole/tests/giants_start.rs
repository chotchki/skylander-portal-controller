//! The start's scripts on chapter 1's props (openspec AG.60), headless over
//! `GUAC_GIANTS_DUMP` in strict mode: what they wake, clone, rescale and
//! reshape at the level's start, the members they reach, and the clones
//! the TV draws (AS.63's rule, [`guacamole::props::clones_draw`]: static as
//! props, skinned as creatures) held to what they make.
//! Their places against AG.7's live reads are `giants_props`'.

mod common;

use std::collections::{BTreeMap, BTreeSet};

use common::*;
use guac_script::Mode;
use guacamole::props::UNDRAWN_SPAWNS;

/// The start's clones' templates that draw nothing on the TV, each with
/// why (AS.63's rule: the pebbles by name, `UNDRAWN_SPAWNS`, a marker that
/// shows nothing, `props::see_through`): every other template the start
/// clones with a model is drawn, static or skinned.
const UNDRAWN: [(&str, &str); 6] = [
    (
        "Boulder_Titan_Pebble_01(1)",
        "on the boulder's attach nodes",
    ),
    (
        "Boulder_Titan_Pebble_02(1)",
        "on the boulder's attach nodes",
    ),
    (
        "Boulder_Titan_Pebble_03(1)",
        "on the boulder's attach nodes",
    ),
    (
        "Boulder_Titan_Pebble_04(1)",
        "on the boulder's attach nodes",
    ),
    (
        "Boulder_Titan_Pebble_05(1)",
        "on the boulder's attach nodes",
    ),
    ("Canon_Barrel_Marker", "shows nothing"),
];

#[test]
#[ignore = "disc: GUAC_GIANTS_DUMP (AG.60)"]
fn the_starts_scripts_run_strict_and_make_what_the_tv_draws() {
    let started = std::time::Instant::now();
    let (mut run, _) = run_on_with(
        Mode::Strict,
        Setup {
            props: true,
            ..Setup::default()
        },
    );
    for i in 0..600 {
        run.step().unwrap_or_else(|h| panic!("step {i}: {h}"));
    }
    println!(
        "600 steps strict in {:.1} s; {}",
        started.elapsed().as_secs_f64(),
        load()
    );
    // The members the run reached, against AH.34's ceiling as AG.28's
    // camera volumes and activator raised it.
    let reached = run.vm.reached().len();
    println!("members reached: {reached} (AG.28's ceiling 325)");
    assert!(reached <= 325, "{reached}");

    // The clones the start made, by template, and which the TV draws.
    let mut clones: BTreeMap<String, usize> = BTreeMap::new();
    for c in run.world.clones() {
        let a = run.world.actor(c).unwrap();
        let Some(t) = a.template.and_then(|t| run.world.template(t)) else {
            continue;
        };
        if t.obj.unit != LEVEL {
            continue;
        }
        *clones.entry(t.name.clone()).or_default() += 1;
    }
    println!("the level's templates cloned: {clones:?}");
    // The skinned models the TV draws the skinned templates' clones with
    // (AG.150, AS.63), as the census's disc test reads them.
    let skinned: Vec<String> = skinned_models().into_iter().map(|(p, _)| p).collect();
    let drawn: BTreeSet<&str> = clones
        .keys()
        .map(String::as_str)
        .filter(|name| clones_drawn(&run, &skinned, name))
        .collect();
    println!("drawn on the TV: {drawn:?}");
    for name in clones.keys() {
        let undrawn = UNDRAWN.iter().find(|(n, _)| n == name);
        assert!(
            drawn.contains(name.as_str()) != undrawn.is_some(),
            "{name}: drawn {}, listed undrawn {undrawn:?}",
            drawn.contains(name.as_str())
        );
    }
    for name in UNDRAWN_SPAWNS {
        assert!(
            run.files
                .placed
                .iter()
                .any(|p| p.obj.unit == LEVEL && p.name == name),
            "{name} isn't a template of the level"
        );
    }
}

/// The clones level 214's start makes (openspec AS.63), each drawn by the
/// rule off 214's own props part or named left out: its six red push
/// blocks, the spike gate's spikes, holes and enemy gate lock, the Chompy
/// spawners' pods (skinned, scripted by nothing) and the undead Chompies;
/// the Titan boulders' pebbles by name. Play mode, Eruptor joined at its
/// start, 600 steps.
#[test]
#[ignore = "disc: GUAC_GIANTS_DUMP (AS.63)"]
fn level_214s_start_clones_draw_by_the_rule() {
    const LEVEL_214: &str = "level_214_tutotial_2";
    let started = std::time::Instant::now();
    let (mut run, _) = run_on_level(
        Mode::Play,
        LEVEL_214,
        Setup {
            props: true,
            interface: true,
            ..Setup::default()
        },
    );
    let mut stops = Vec::new();
    for _ in 0..2 {
        stops.extend(run.step_play().unwrap());
    }
    run.join().unwrap_or_else(|h| panic!("{h}"));
    for _ in 0..600 {
        stops.extend(run.step_play().unwrap());
    }
    let mut clones: BTreeMap<String, usize> = BTreeMap::new();
    for c in run.world.clones() {
        let a = run.world.actor(c).unwrap();
        let Some(t) = a.template.and_then(|t| run.world.template(t)) else {
            continue;
        };
        if t.obj.unit == LEVEL {
            *clones.entry(t.name.clone()).or_default() += 1;
        }
    }
    let skinned: Vec<String> = skinned_models_of(LEVEL_214)
        .into_iter()
        .map(|(p, _)| p)
        .collect();
    let drawn: BTreeSet<&str> = clones
        .keys()
        .map(String::as_str)
        .filter(|name| clones_drawn(&run, &skinned, name))
        .collect();
    println!(
        "214's start clones {clones:?}; drawn {drawn:?}; {:.1} s, {}",
        started.elapsed().as_secs_f64(),
        load()
    );
    for name in clones.keys() {
        assert_eq!(
            drawn.contains(name.as_str()),
            !UNDRAWN_SPAWNS.contains(&name.as_str()),
            "{name}"
        );
    }
    for name in [
        "0_Push_Block_Art_Stone_Red",
        "Gate_Spikes_Spike",
        "Gate_Spikes_Hole",
        "Lock_EnemyGate",
        "Enemy_ChompySpawner_Pod",
        "_Template_Undead_Chompy",
    ] {
        assert!(drawn.contains(name), "{name} drawn");
    }
    assert_eq!(clones["0_Push_Block_Art_Stone_Red"], 6);
    assert_eq!(clones["Enemy_ChompySpawner_Pod"], 2);
    assert!(stops.is_empty(), "{stops:?}");
}
