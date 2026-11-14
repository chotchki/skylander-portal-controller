//! The wake rule on the levels past 201 (openspec AS.246): each of levels
//! 202-211 started headless on the TV's listing, its hero joined, run 600
//! steps at its start and 300 at every 40th placement its file holds, once
//! on the original's rule (every sleeper's declared locals made anew,
//! [`Wake::Every`]) and once with every sleeper keeping them
//! ([`Wake::Kept`]). Each level starts and nothing stops on either; on the
//! original's every placement of the file asleep at the end holds none of
//! the locals its script declares, kept some do (the walk puts sleepers
//! through the rule), unless it was seen holding none in the sleep it's in
//! and another script's `find variable` (filter 0) made its list again since
//! (level 205's two undead sound watchers, each one's SHUTDOWN reading the
//! other's `alive`, openspec AS.289), and every placement's behaviour and sleep at the end
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
    /// with the first such local, not seen holding none in this sleep.
    holding: Vec<(String, String)>,
    /// Those asleep holding one that were seen holding none in this sleep:
    /// their list made again since by another script's find.
    found_again: Vec<(String, String)>,
}

/// The first local its script declares that placement `id` holds asleep,
/// one never set up, held after its first tick, taken off or stopped
/// excepted: those never have their locals let go (crate docs, The tick).
fn holds_declared(run: &Run, id: guac_script::PlacementId, script: Option<&str>) -> Option<String> {
    if !run.vm.asleep(id)
        || run.vm.removed(id)
        || !run.vm.set_up(id)
        || run.vm.is_held(id)
        || run.vm.stopped(id).is_some()
    {
        return None;
    }
    let program = run.vm.program();
    let script = script.and_then(|s| program.script_named(s))?;
    let mut declared: Vec<&str> = program.scripts[script as usize]
        .declared
        .keys()
        .map(|k| program.sym(*k))
        .collect();
    declared.sort_unstable();
    declared
        .iter()
        .find(|l| run.vm.local(id, l).is_some())
        .map(|l| (*l).to_owned())
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
    // The sleepers seen holding none of their declared locals in the sleep
    // they're in, by placement: the rule let go of them.
    let mut let_go = std::collections::HashSet::new();
    let mut step = |run: &mut Run, stops: &mut Vec<String>| {
        let halts = run.step_play().unwrap_or_else(|h| panic!("{level}: {h}"));
        stops.extend(halts.iter().map(|h| format!("step {}: {h}", run.steps)));
        for p in &run.files.placed {
            let Some(id) = run
                .handles
                .get(&p.obj)
                .and_then(|h| run.vm.placement_of(*h))
            else {
                continue;
            };
            if !run.vm.asleep(id) {
                let_go.remove(&id);
            } else if !let_go.contains(&id)
                && holds_declared(run, id, p.script.as_deref()).is_none()
            {
                let_go.insert(id);
            }
        }
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
    let mut states = Vec::new();
    let mut holding = Vec::new();
    let mut found_again = Vec::new();
    for p in &run.files.placed {
        let Some(h) = run.handles.get(&p.obj).copied() else {
            continue;
        };
        let Some(id) = run.vm.placement_of(h) else {
            continue;
        };
        states.push((
            u64::from(p.obj.unit) << 32 | u64::from(p.obj.index),
            run.vm.behaviour(id).unwrap_or("").to_owned(),
            run.vm.asleep(id),
        ));
        if let Some(l) = holds_declared(&run, id, p.script.as_deref()) {
            if let_go.contains(&id) {
                found_again.push((p.name.clone(), l));
            } else {
                holding.push((p.name.clone(), l));
            }
        }
    }
    let asleep = states.iter().filter(|s| s.2).count();
    Stood {
        stops,
        states,
        asleep,
        holding,
        found_again,
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
            "{level}: {} placements, {} asleep; on the rule {} asleep holding a declared local ({} made again by a find: {:?}), kept {} ({:?})",
            every.states.len(),
            every.asleep,
            every.holding.len(),
            every.found_again.len(),
            every.found_again,
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
