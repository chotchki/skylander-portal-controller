//! Live (openspec AF.3): the oracle boots Giants to playable from its isolated
//! home, and the dev's own RPCS3 dirs come out with the same file list, sizes
//! and mtimes. Runs on the Mac against `GUAC_ORACLE_EXE` (the `09d602fd5` copy
//! until AF.4's build exists); reads `GUAC_ORACLE_HOME` and `GUAC_GIANTS_DUMP`
//! too, from the env or `.env.dev`, and skips when one is unset. A cold home
//! compiles Giants' PPU modules first, so `ORACLE_READY_SECS` defaults to 600.
//!
//! `cargo test -p oracle-harness --test live_boot -- --ignored --nocapture`

#![cfg(unix)]

mod common;

use oracle_harness::launch::{self, BootOptions};
use oracle_harness::snapshot::Snapshot;

#[test]
#[ignore = "live: boots GUAC_ORACLE_EXE from GUAC_ORACLE_HOME (Mac)"]
fn the_oracle_boots_from_its_home_and_leaves_the_devs_rpcs3_alone() {
    // Taken before init too: init only reads the dev's dir.
    let Some(dev_dirs) = std::env::var_os("HOME")
        .map(|h| oracle_harness::snapshot::dev_rpcs3_dirs(std::path::Path::new(&h)))
    else {
        return;
    };
    let before = Snapshot::take(&dev_dirs).unwrap();
    let Some(live) = common::live() else { return };
    assert!(!before.is_empty(), "no dev RPCS3 dir to compare against");

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
    let state = oracle
        .wait_playable(common::secs("ORACLE_READY_SECS", 600))
        .unwrap();
    eprintln!("playable: {state:?}");
    let fps = oracle
        .sample_fps(common::secs("ORACLE_SAMPLE_SECS", 30))
        .unwrap();
    eprintln!(
        "sustained: {} frames in {:.1} s = {:.1} fps",
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
