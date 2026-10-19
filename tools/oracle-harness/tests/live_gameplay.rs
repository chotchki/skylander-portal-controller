//! Live (openspec AF.4): the oracle gets past boot into a level with a hero in
//! it, with no screen. A cold boot from the oracle home to playable, then CROSS
//! over P6 once a second through the opening (videos, menu, story, save slot,
//! Eon's monologue: `tools/playthrough`'s gates.json). Once RPCS3's log shows a
//! level other than the title opened, a fresh copy of `GUAC_ORACLE_FIGURE` goes
//! on the portal over P1, cleared and placed again every 20 s in case the game
//! only takes a placement at its prompt, until the log shows a character archive
//! opened after it (the hero loading). Then 30 s of frames in the level, and the
//! dev's RPCS3 dirs checked untouched. RPCS3's own log is the evidence; nothing
//! needs to reach a display.
//!
//! Mac, against `GUAC_ORACLE_EXE`; reads `GUAC_ORACLE_HOME`, `GUAC_GIANTS_DUMP`
//! and `GUAC_ORACLE_FIGURE` (a `.sky` from the dev's own pack; the game writes
//! the run dir's copy, never the file) from the env or `.env.dev`, and skips when
//! one is unset. `ORACLE_LEVEL_SECS` (default 600) bounds playable to hero,
//! `ORACLE_SHADER_MODE` overrides the template's shader mode, and `ORACLE_AUDIO=1`
//! plays the game's sound (the template is silent).
//!
//! `cargo test -p oracle-harness --test live_gameplay -- --ignored --nocapture`

#![cfg(unix)]

mod common;

use std::time::Instant;

use oracle_harness::env;
use oracle_harness::launch::{self, BootOptions};
use oracle_harness::rpcs3_log::LogTail;
use oracle_harness::snapshot::Snapshot;

const FIGURE: &str = "GUAC_ORACLE_FIGURE";

#[test]
#[ignore = "live: drives GUAC_ORACLE_EXE from GUAC_ORACLE_HOME into a level with GUAC_ORACLE_FIGURE on the portal (Mac)"]
fn the_oracle_reaches_a_level_with_a_hero_on_the_portal() {
    let Some(figure) = env::path(FIGURE) else {
        eprintln!("skip: set {FIGURE} to a .sky from the dev's own pack");
        return;
    };
    let Some(live) = common::live() else { return };
    let before = Snapshot::take(&live.dev_dirs).unwrap();

    let launched = Instant::now();
    let mut oracle = launch::boot(
        &live.exe,
        &live.home,
        &BootOptions {
            borderless: true,
            ..BootOptions::default()
        },
    )
    .unwrap();
    eprintln!("pid {} run {:?}", oracle.pid(), oracle.run_paths());
    let mut log = LogTail::new(live.home.cache_dir().join("RPCS3.log"));
    let state = oracle
        .wait_playable(common::secs("ORACLE_READY_SECS", 600))
        .unwrap();
    eprintln!(
        "playable at +{:.0} s: {state:?}",
        launched.elapsed().as_secs_f64()
    );

    common::reach_hero(&mut oracle, &mut log, &figure, launched);

    let fps = oracle
        .sample_fps(common::secs("ORACLE_SAMPLE_SECS", 30))
        .unwrap();
    eprintln!(
        "in the level: {} frames in {:.1} s = {:.1} fps",
        fps.frames,
        fps.seconds,
        fps.fps()
    );
    eprintln!("shutdown: {:?}", oracle.shutdown().unwrap());

    let changed = before.diff(&Snapshot::take(&live.dev_dirs).unwrap());
    assert!(
        changed.is_empty(),
        "the dev's RPCS3 dirs changed:\n{}",
        changed.join("\n")
    );
    eprintln!("the dev's RPCS3 dirs: {} entries, unchanged", before.len());
}
