//! The wake rule on the levels past 201 (openspec AS.246): each of levels
//! 202-211 started headless on the TV's listing, its hero joined, run 600
//! steps at its start and 300 at every 40th placement its file holds, and the state of every placement it runs
//! (behaviour, asleep) digested, so a run on the rule and one off it can be
//! set side by side (`GUAC_WAKE_DIGEST=<file>` appends a line a level). Ignored
//! by default; it reads `GUAC_GIANTS_DUMP` (process env, else `.env.dev`):
//!
//! `cargo test -p guacamole --profile disc --test giants_wake -- --ignored --nocapture --test-threads=1`

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::io::Write as _;

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

/// How a level starts and stands after its walk: its stops, the count of
/// placements asleep and a digest of every placement's behaviour.
fn stand(level: &str) -> (usize, usize, usize, u64) {
    let setup = Setup {
        interface: true,
        props: true,
        sounds: true,
        ..Setup::default()
    };
    let (mut run, _found) = run_on_level(Mode::Play, level, setup);
    let mut stops = 0;
    for _ in 0..2 {
        stops += run.step_play().unwrap().len();
    }
    run.join().unwrap_or_else(|h| panic!("{h}"));
    for _ in 0..600 {
        stops += run.step_play().unwrap().len();
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
            stops += run.step_play().unwrap().len();
        }
    }
    let mut rows: Vec<(u64, bool, String)> = run
        .handles
        .iter()
        .filter_map(|(o, h)| {
            let id = run.vm.placement_of(*h)?;
            Some((
                u64::from(o.unit) << 32 | u64::from(o.index),
                run.vm.asleep(id),
                run.vm.behaviour(id).unwrap_or("").to_owned(),
            ))
        })
        .collect();
    rows.sort();
    let asleep = rows.iter().filter(|r| r.1).count();
    let mut hasher = DefaultHasher::new();
    rows.hash(&mut hasher);
    (stops, rows.len(), asleep, hasher.finish())
}

#[test]
#[ignore = "reads the Giants dump"]
fn levels_202_to_211_start_headless_and_stand() {
    let started = std::time::Instant::now();
    println!("load at start: {}", load());
    for level in LEVELS {
        let ran = std::panic::catch_unwind(|| stand(level));
        let line = match ran {
            Ok((stops, run, asleep, digest)) => {
                format!(
                    "{level}\tstarted\tstops {stops}\truns {run}\tasleep {asleep}\tdigest {digest:016x}"
                )
            }
            Err(e) => {
                let why = e
                    .downcast_ref::<String>()
                    .cloned()
                    .or_else(|| e.downcast_ref::<&str>().map(|s| (*s).to_owned()))
                    .unwrap_or_default();
                format!(
                    "{level}\tdid not start\t{}",
                    why.lines().next().unwrap_or("")
                )
            }
        };
        println!("{line}");
        if let Ok(path) = std::env::var("GUAC_WAKE_DIGEST")
            && let Ok(mut f) = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(path)
        {
            let _ = writeln!(f, "{line}");
        }
    }
    println!(
        "{:.1} s, load at end: {}",
        started.elapsed().as_secs_f64(),
        load()
    );
}
