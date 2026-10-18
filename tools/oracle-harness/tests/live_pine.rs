//! Live (openspec AF.6): PINE against the real oracle, at the pause `Start
//! Paused` makes 2 flips into a savestate boot (it acts on nothing else). Runs on
//! the Mac against `GUAC_ORACLE_EXE` with `GUAC_ORACLE_HOME` and
//! `GUAC_GIANTS_DUMP` as `live_boot` reads them, plus `GUAC_ORACLE_SAVESTATE`: a
//! state the binary accepts (on the `09d602fd5` copy, a copy of the June state;
//! the harness boots a further copy of it). Skips when a key is unset.
//!
//! `cargo test -p oracle-harness --test live_pine -- --ignored --nocapture`

#![cfg(unix)]

mod common;

use std::path::Path;
use std::time::{Duration, Instant};

use oracle_harness::launch::{self, BootOptions};
use oracle_harness::pine::{EmuStatus, PineClient};
use oracle_harness::snapshot::Snapshot;

/// The start of the main heap (the savestate pass found it mapped, 0x9f00000
/// bytes); 64 KiB of it is 8,192 64-bit reads, one batch.
const RANGE: (u32, usize) = (0x3000_0000, 64 << 10);

#[test]
#[ignore = "live: boots GUAC_ORACLE_SAVESTATE in GUAC_ORACLE_HOME and reads guest memory over PINE (Mac)"]
fn pine_reads_and_writes_guest_memory_at_a_start_paused_pause() {
    let Some(state) = oracle_harness::env::path("GUAC_ORACLE_SAVESTATE") else {
        eprintln!("skip: set GUAC_ORACLE_SAVESTATE to a savestate the oracle binary accepts");
        return;
    };
    let Some(live) = common::live() else { return };
    let before = Snapshot::take(&live.dev_dirs).unwrap();

    let mut oracle = launch::boot(
        &live.exe,
        &live.home,
        &BootOptions {
            savestate: Some(state),
            borderless: true,
        },
    )
    .unwrap();
    let paused = oracle
        .wait_paused(common::secs("ORACLE_READY_SECS", 600))
        .unwrap();
    eprintln!("paused: {paused:?}");

    let mut pine = connect(&oracle.run_paths().pine_socket);
    let id = pine.title_id().unwrap();
    eprintln!(
        "PINE: id={id} title={:?} version={:?} status={:?}",
        pine.title().unwrap(),
        pine.version().unwrap(),
        pine.status().unwrap()
    );
    assert_eq!(id, "BLUS30968");
    assert_eq!(pine.status().unwrap(), EmuStatus::Paused);

    let (addr, len) = RANGE;
    let t = Instant::now();
    let first = pine.read_bytes(addr, len).unwrap();
    let read_time = t.elapsed();
    let second = pine.read_bytes(addr, len).unwrap();
    assert_eq!(first.len(), len);
    assert!(first == second, "two reads at one pause differ");
    let nonzero = first.iter().filter(|&&b| b != 0).count();
    eprintln!(
        "read {len} bytes at 0x{addr:08x} in {read_time:?}: identical twice, {nonzero} non-zero"
    );

    pine.write_bytes(addr, &first).unwrap();
    let third = pine.read_bytes(addr, len).unwrap();
    assert!(first == third, "writing back what was read changed it");

    // Still at the pause: nothing above moved the emulator.
    let after = oracle.wait_for(Duration::from_secs(5), |_| true).unwrap();
    assert_eq!(
        (after.status.as_str(), after.frames),
        ("paused", paused.frames)
    );
    eprintln!("shutdown: {:?}", oracle.shutdown().unwrap());

    let changed = before.diff(&Snapshot::take(&live.dev_dirs).unwrap());
    assert!(
        changed.is_empty(),
        "the dev's RPCS3 dirs changed:\n{}",
        changed.join("\n")
    );
}

/// PINE's server starts with the game, so its socket can trail the pause.
fn connect(socket: &Path) -> PineClient {
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        match PineClient::connect(socket) {
            Ok(c) => return c,
            Err(e) if Instant::now() > deadline => panic!("PINE at {}: {e}", socket.display()),
            Err(_) => std::thread::sleep(Duration::from_millis(250)),
        }
    }
}
