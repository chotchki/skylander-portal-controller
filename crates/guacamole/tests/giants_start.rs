//! The start's scripts on chapter 1's props (openspec AG.60), headless over
//! `GUAC_GIANTS_DUMP` in strict mode: what they wake, clone, rescale and
//! reshape at the level's start, the members they reach, and the clones
//! the TV draws ([`guacamole::props::DRAWN_SPAWNS`] static,
//! [`guacamole::props::SKINNED_SPAWNS`] as creatures) held to what they
//! make.
//! Their places against AG.7's live reads are `giants_props`'.

mod common;

use std::collections::{BTreeMap, BTreeSet};

use common::*;
use guac_script::Mode;
use guacamole::props::{DRAWN_SPAWNS, SKINNED_SPAWNS};

/// The start's clones' templates that draw nothing on the TV, each with
/// why (`DRAWN_SPAWNS`' doc): every other template the start clones with a
/// model is drawn, static or skinned.
const UNDRAWN: [(&str, &str); 9] = [
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
    ("Chest_Lock", "the chest's, a creature's"),
    ("Canon_Barrel_Marker", "invisible"),
    ("Dummy_Inviso", "invisible"),
    ("Turban_hat", "Norticus' hat: the fight's, a creature's"),
];

#[test]
#[ignore = "disc: GUAC_GIANTS_DUMP (AG.60)"]
fn the_starts_scripts_run_strict_and_make_what_the_tv_draws() {
    let started = std::time::Instant::now();
    let (mut run, _) = run_on_with(
        Mode::Strict,
        Setup {
            props: true,
            ..Default::default()
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
    let props = run.bench.props.as_ref().unwrap();
    // The skinned models the TV draws the clones of SKINNED_SPAWNS'
    // templates with (AG.150), as the census's disc test reads them.
    let skinned: Vec<String> = skinned_models().into_iter().map(|(p, _)| p).collect();
    let drawn: BTreeSet<&str> = clones
        .keys()
        .map(String::as_str)
        .filter(|name| {
            run.files.placed.iter().any(|p| {
                p.obj.unit == LEVEL
                    && p.name == *name
                    && p.model.as_ref().is_some_and(|m| {
                        props
                            .level
                            .spawn_models
                            .iter()
                            .any(|&i| &props.level.models[i].path == m)
                            || (SKINNED_SPAWNS.contains(name) && skinned.contains(m))
                    })
            })
        })
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
    for name in DRAWN_SPAWNS.iter().chain(SKINNED_SPAWNS.iter()) {
        assert!(
            run.files
                .placed
                .iter()
                .any(|p| p.obj.unit == LEVEL && p.name == *name),
            "{name} isn't a template of the level"
        );
    }
}
