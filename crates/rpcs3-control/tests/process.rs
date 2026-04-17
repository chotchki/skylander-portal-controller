//! Live integration tests for `RpcsProcess`. Require a real RPCS3 install;
//! they are `#[ignore]` by default.
//!
//! Usage:
//!   RPCS3_EXE=C:/emuluators/rpcs3/rpcs3.exe \
//!     cargo test -p skylander-rpcs3-control --test process -- --ignored --nocapture
//!
//! These exercise process lifecycle only (launch + wait_ready + shutdown) —
//! no UIA menu driving — so launching into library view is sufficient.

#![cfg(windows)]

use std::path::PathBuf;
use std::time::Duration;

use skylander_rpcs3_control::{RpcsProcess, ShutdownPath};

fn env_path(key: &str) -> Option<PathBuf> {
    std::env::var(key).ok().map(PathBuf::from)
}

#[test]
#[ignore = "requires RPCS3_EXE env var"]
fn launch_wait_shutdown_graceful() {
    let exe = match env_path("RPCS3_EXE") {
        Some(p) => p,
        None => return,
    };

    let mut proc = RpcsProcess::launch_library(&exe).expect("launch_library");
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
        matches!(
            path,
            ShutdownPath::Graceful | ShutdownPath::Forced | ShutdownPath::AlreadyExited
        ),
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
