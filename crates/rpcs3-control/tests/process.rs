//! Live integration tests for `RpcsProcess`. Require a real RPCS3 install and
//! a valid EBOOT.BIN; they are `#[ignore]` by default.
//!
//! Usage:
//!   RPCS3_EXE=C:/emuluators/rpcs3/rpcs3.exe \
//!   RPCS3_TEST_EBOOT="C:/games/ps3/Skylanders Giants/PS3_GAME/USRDIR/EBOOT.BIN" \
//!     cargo test -p skylander-rpcs3-control --test process -- --ignored --nocapture

#![cfg(windows)]

use std::path::PathBuf;
use std::time::Duration;

use skylander_rpcs3_control::{RpcsProcess, ShutdownPath};

fn env_path(key: &str) -> Option<PathBuf> {
    std::env::var(key).ok().map(PathBuf::from)
}

#[test]
#[ignore = "requires RPCS3_EXE and RPCS3_TEST_EBOOT env vars"]
fn launch_wait_shutdown_graceful() {
    let exe = match env_path("RPCS3_EXE") {
        Some(p) => p,
        None => return,
    };
    let eboot = match env_path("RPCS3_TEST_EBOOT") {
        Some(p) => p,
        None => return,
    };

    let mut proc = RpcsProcess::launch(&exe, &eboot).expect("launch");
    assert!(proc.pid() != 0);
    assert!(proc.is_alive());

    proc.wait_ready(Duration::from_secs(30))
        .expect("wait_ready within 30s");

    // Let the emulator settle briefly before poking it.
    std::thread::sleep(Duration::from_secs(2));

    let path = proc
        .shutdown_graceful(Duration::from_secs(30))
        .expect("shutdown_graceful");
    assert!(
        matches!(path, ShutdownPath::Graceful | ShutdownPath::Forced | ShutdownPath::AlreadyExited),
        "unexpected shutdown path {path:?}",
    );
    assert!(!proc.is_alive(), "process should be dead after shutdown");
}

#[test]
#[ignore = "requires a running RPCS3 instance to attach to"]
fn attach_reports_alive() {
    let mut proc = RpcsProcess::attach().expect("attach");
    assert!(proc.pid() != 0);
    assert!(proc.is_alive());
    // Don't shut down — this is the user's running instance.
}
