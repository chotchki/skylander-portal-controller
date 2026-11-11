//! The wake rule on the levels past 201 (openspec AS.246): each of levels
//! 202-211 started headless on the TV's listing, its hero joined, run 600
//! steps at its start and 300 at every 40th placement its file holds, once
//! on the original's rule (every sleeper's declared locals made anew,
//! [`Wake::Every`]) and once with every sleeper keeping them
//! ([`Wake::Kept`]). Each level starts and nothing stops on either; on the
//! original's every placement of the file asleep at the end holds none of
//! the locals its script declares, kept some do (the walk puts sleepers
//! through the rule), and every placement's behaviour and sleep at the end
//! are the same on both: the rule changes nothing these walks end on, so a
//! difference is AS.246's to read before the TV turns it on. Ignored by
//! default; it reads `GUAC_GIANTS_DUMP` (process env, else `.env.dev`):
//!
//! `cargo test -p guacamole --profile disc --test giants_wake -- --ignored --nocapture --test-threads=1`

use guac_motion::seam::Vec3;
use guac_script::Mode;

mod common;

use common::*;

const LEVELS: [&str; 10] = [
    "level_202_arkeyancopter",
    "level_203_the_machine",
    "level_204_choices",
    "level_205_piratecards",
    "level_206",
    "level_207_arkus_adventure",
    "level_208_ice",
    "level_209_giantrobot",
    "level_210_thevillage",
    "level_211_final_level",
];

/// How a level stands after its walk on a wake rule.
struct Stood {
    stops: Vec<String>,
    /// Every placement's object, behaviour and sleep, in the file's order.
    states: Vec<(u64, String, bool)>,
    asleep: usize,
    /// The file's placements asleep holding a local their script declares,
    /// with the first such local.
    holding: Vec<(String, String)>,
}

fn stand(level: &str, wake: Wake) -> Stood {
    let setup = Setup {
        interface: true,
        props: true,
        sounds: true,
        wake,
        ..Setup::default()
    };
    let (mut run, _found) = run_on_level(Mode::Play, level, setup);
    let mut stops = Vec::new();
    let step = |run: &mut Run, stops: &mut Vec<String>| {
        let halts = run.step_play().unwrap_or_else(|h| panic!("{level}: {h}"));
        stops.extend(halts.iter().map(|h| format!("step {}: {h}", run.steps)));
    };
    for _ in 0..2 {
        step(&mut run, &mut stops);
    }
    run.join().unwrap_or_else(|h| panic!("{level}: {h}"));
    for _ in 0..600 {
        step(&mut run, &mut stops);
    }
    // Then set down by every 40th placement the level's file holds, 300
    // steps each: what sleeps and wakes as a hero crosses it.
    let spots: Vec<Vec3> = run
        .files
        .placed
        .iter()
        .filter(|p| p.obj.unit == LEVEL && p.activity_state == 4)
        .step_by(40)
        .map(|p| Vec3::from_array(p.position))
        .collect();
    for at in &spots {
        run.put(*at + Vec3::new(0.0, 1.0, 0.0), 0.0);
        for _ in 0..300 {
            step(&mut run, &mut stops);
        }
    }
    // Two steps standing, so every sleeper's shutdown has run.
    for _ in 0..2 {
        step(&mut run, &mut stops);
    }
    let program = run.vm.program();
    let mut states = Vec::new();
    let mut holding = Vec::new();
    for p in &run.files.placed {
        let Some(h) = run.handles.get(&p.obj).copied() else {
            continue;
        };
        let Some(id) = run.vm.placement_of(h) else {
            continue;
        };
        let asleep = run.vm.asleep(id);
        states.push((
            u64::from(p.obj.unit) << 32 | u64::from(p.obj.index),
            run.vm.behaviour(id).unwrap_or("").to_owned(),
            asleep,
        ));
        // One asleep since the level's start, held after its first tick or
        // stopped never has its locals let go (crate docs, The tick).
        if !asleep
            || run.vm.removed(id)
            || !run.vm.set_up(id)
            || run.vm.is_held(id)
            || run.vm.stopped(id).is_some()
        {
            continue;
        }
        let Some(script) = p.script.as_deref().and_then(|s| program.script_named(s)) else {
            continue;
        };
        let mut declared: Vec<&str> = program.scripts[script as usize]
            .declared
            .keys()
            .map(|k| program.sym(*k))
            .collect();
        declared.sort_unstable();
        if let Some(l) = declared.iter().find(|l| run.vm.local(id, l).is_some()) {
            holding.push((p.name.clone(), (*l).to_owned()));
        }
    }
    let asleep = states.iter().filter(|s| s.2).count();
    Stood {
        stops,
        states,
        asleep,
        holding,
    }
}

#[test]
#[ignore = "reads the Giants dump"]
fn levels_202_to_211_stand_the_same_on_the_rule_and_off_it() {
    let started = std::time::Instant::now();
    println!("load at start: {}", load());
    let mut kept_holding = 0;
    for level in LEVELS {
        let every = stand(level, Wake::Every);
        let kept = stand(level, Wake::Kept);
        println!(
            "{level}: {} placements, {} asleep; on the rule {} asleep holding a declared local, kept {} ({:?})",
            every.states.len(),
            every.asleep,
            every.holding.len(),
            kept.holding.len(),
            kept.holding.iter().take(3).collect::<Vec<_>>()
        );
        assert!(
            every.stops.is_empty(),
            "{level} on the rule: {:#?}",
            every.stops
        );
        assert!(kept.stops.is_empty(), "{level} kept: {:#?}", kept.stops);
        assert!(every.holding.is_empty(), "{level}: {:?}", every.holding);
        assert_eq!(
            every.states, kept.states,
            "{level}: the rule changed an end state"
        );
        kept_holding += kept.holding.len();
    }
    assert!(kept_holding > 0, "no walk put a sleeper through the rule");
    println!(
        "{:.1} s, load at end: {}",
        started.elapsed().as_secs_f64(),
        load()
    );
}
