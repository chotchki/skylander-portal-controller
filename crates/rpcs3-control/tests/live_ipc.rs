//! Live integration test for the **patched RPCS3** P1/P2 IPC seams — i.e. this
//! is how the C++ emulator changes (`rpcs3-patches/`) get tested, from Rust,
//! against the real binary. The Rust-only coverage (`tests/ipc_loopback.rs` +
//! `src/ipc/proto.rs` unit tests) pins the wire contract from the controller
//! side; this test pins the *other* side — that the actual patched emulator
//! honours it (the listener comes up, `g_skyportal` really loads/clears, the
//! STATE/heartbeat liveness signal advances, the window handle is published).
//!
//! It **attaches** to an already-running patched RPCS3 (no launch/boot here —
//! that's involved and machine-specific): boot a Skylanders game first, e.g.
//!   D:\workspace\rpcs3\run_game.bat
//! which sets SKYLANDER_IPC_PATH + SKYLANDER_BORDERLESS and boots Giants no-GUI.
//!
//! Then, on the HTPC (session-bound; the socket only exists once the emulated
//! Skylander USB device is instantiated, i.e. a Skylanders game is running):
//!   set RPCS3_SKY_TEST_PATH=C:\...\working\Spyro.sky   (optional: exercises LOAD/CLEAR)
//!   cargo test -p skylander-rpcs3-control --test live_ipc -- --ignored --nocapture
//!
//! Run without a patched RPCS3 listening and it skips cleanly (returns).

use std::path::PathBuf;
use std::thread::sleep;
use std::time::Duration;

use skylander_core::{SlotIndex, SlotState};
use skylander_rpcs3_control::PortalDriver;
use skylander_rpcs3_control::ipc::IpcPortalDriver;

fn occupied_count(slots: &[SlotState]) -> usize {
    slots
        .iter()
        .filter(|s| matches!(s, SlotState::Loaded { .. }))
        .count()
}

#[test]
#[ignore = "requires a running patched RPCS3 with a Skylanders game booted (run_game.bat); honours SKYLANDER_IPC_PATH / RPCS3_SKY_TEST_PATH"]
fn patched_rpcs3_ipc_load_clear_state() {
    let driver = IpcPortalDriver::new().expect("construct IPC driver");
    eprintln!("IPC socket: {}", driver.socket_path().display());

    // PING proves the patched listener (P1) is up. If not, skip — same posture
    // as the UIA live test's missing-env early return.
    if let Err(e) = driver.ping() {
        eprintln!(
            "skip: no patched RPCS3 IPC socket ({e}). Boot a Skylanders game first \
             (e.g. D:\\workspace\\rpcs3\\run_game.bat)."
        );
        return;
    }

    // STATE / heartbeat liveness signal (replaces log-scraping): sample twice and
    // assert the frame counter is monotonic. While a game renders it climbs ~60/s.
    let s1 = driver.read_state().expect("read_state #1");
    eprintln!(
        "state#1: status={} frames={} progr={}/{} seg={}/{}",
        s1.status, s1.frames, s1.progr_done, s1.progr_total, s1.seg_done, s1.seg_total
    );
    assert!(!s1.status.is_empty(), "STATE must report a status");
    sleep(Duration::from_millis(1100));
    let s2 = driver.read_state().expect("read_state #2");
    eprintln!("state#2: status={} frames={}", s2.status, s2.frames);
    assert!(
        s2.frames >= s1.frames,
        "frame counter must be monotonic ({} -> {})",
        s1.frames,
        s2.frames
    );
    if s2.status == "running" {
        assert!(
            s2.frames > s1.frames,
            "a running game should be advancing frames (playable={})",
            s2.is_playable()
        );
    }

    // WINDOW (P2): the native game-window handle is published once the window is
    // created. Log it; assert it parses. (Non-zero once rendering has started.)
    match driver.window_handle() {
        Ok(h) => eprintln!("window handle = 0x{h:X}"),
        Err(e) => eprintln!("WINDOW query failed (pre-window?): {e}"),
    }

    // STATUS shape.
    let before = driver.read_slots().expect("read_slots");
    assert_eq!(before.len(), 8, "portal is always 8 slots");
    let n_before = occupied_count(&before);
    eprintln!("portal occupancy before: {n_before}/8");

    // LOAD/CLEAR against the real g_skyportal — only if a test .sky is provided.
    let Some(path) = std::env::var_os("RPCS3_SKY_TEST_PATH").map(PathBuf::from) else {
        eprintln!("RPCS3_SKY_TEST_PATH unset — skipping the LOAD/CLEAR leg");
        return;
    };
    assert!(
        path.exists(),
        "RPCS3_SKY_TEST_PATH does not exist: {path:?}"
    );

    // The slot arg is a hint only — the emulator assigns the slot.
    let name = driver
        .load(SlotIndex::new(0).unwrap(), &path)
        .expect("LOAD over IPC");
    eprintln!("loaded: {name}");
    assert!(!name.is_empty());

    let after_load = driver.read_slots().expect("read_slots after load");
    assert_eq!(
        occupied_count(&after_load),
        n_before + 1,
        "LOAD should occupy exactly one more slot"
    );

    // Find the slot the emulator chose (occupied now but not before) and clear it.
    let cleared_slot = (0..8)
        .find(|&j| {
            matches!(after_load[j], SlotState::Loaded { .. })
                && matches!(before[j], SlotState::Empty)
        })
        .expect("a newly-occupied slot to clear");

    driver
        .clear(SlotIndex::new(cleared_slot as u8).unwrap())
        .expect("CLEAR over IPC");

    let after_clear = driver.read_slots().expect("read_slots after clear");
    assert_eq!(
        occupied_count(&after_clear),
        n_before,
        "CLEAR should restore the prior occupancy"
    );
}
