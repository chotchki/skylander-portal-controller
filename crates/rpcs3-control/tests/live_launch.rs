//! Live test for the **16.6.1 no-GUI launch path** against the real patched
//! RPCS3 — `RpcsProcess::launch_no_gui` + IPC readiness + the published window
//! handle. This is the focused acceptance test for what the server's `BootDirect`
//! IPC branch does, minus the server's config / `games.yml` machinery.
//!
//! Windows + HTPC only (spawns + renders a real game). Set:
//!   RPCS3_EXE        = D:\workspace\rpcs3\bin\rpcs3.exe        (the PATCHED binary)
//!   RPCS3_EBOOT      = ...\Skylanders Giants\PS3_GAME\USRDIR\EBOOT.BIN
//!   RPCS3_CONFIG_DIR = C:\emuluators\rpcs3\   (firmware + games; inherited by the child)
//!   (optional) SKYLANDER_IPC_PATH — else the per-platform default is used
//! then:
//!   cargo test -p skylander-rpcs3-control --test live_launch -- --ignored --nocapture
//!
//! It boots Giants no-GUI, waits for the IPC liveness signal to report playable,
//! asserts a borderless window handle is published, then shuts down + cleans the
//! lockfile/socket. Missing RPCS3_EXE / EBOOT → skips cleanly.

#![cfg(windows)]

use std::path::{Path, PathBuf};
use std::thread::sleep;
use std::time::{Duration, Instant};

use skylander_rpcs3_control::RpcsProcess;
use skylander_rpcs3_control::ipc::{IpcPortalDriver, default_socket_path};

fn env_path(key: &str, default: &str) -> PathBuf {
    std::env::var_os(key)
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(default))
}

#[test]
#[ignore = "HTPC: launches the patched RPCS3 no-GUI; set RPCS3_EXE (patched) + RPCS3_EBOOT + RPCS3_CONFIG_DIR"]
fn no_gui_launch_reaches_playable() {
    let exe = env_path("RPCS3_EXE", r"D:\workspace\rpcs3\bin\rpcs3.exe");
    let eboot = env_path(
        "RPCS3_EBOOT",
        r"C:\games\ps3\Skylanders Giants\PS3_GAME\USRDIR\EBOOT.BIN",
    );
    if !exe.is_file() || !eboot.is_file() {
        eprintln!(
            "skip: RPCS3_EXE ({}) or RPCS3_EBOOT ({}) missing",
            exe.display(),
            eboot.display()
        );
        return;
    }

    let ipc_path = default_socket_path();
    eprintln!(
        "launch_no_gui: exe={} eboot={} ipc={}",
        exe.display(),
        eboot.display(),
        ipc_path.display()
    );

    // Exactly what BootDirect's IPC branch does.
    let config_dir = std::env::var_os("RPCS3_CONFIG_DIR").map(PathBuf::from);
    let mut proc = RpcsProcess::launch_no_gui(&exe, &eboot, &ipc_path, config_dir.as_deref())
        .expect("launch_no_gui");
    let driver = IpcPortalDriver::with_path(&ipc_path);

    // Readiness: poll the liveness signal until playable, bailing if RPCS3 dies.
    let deadline = Instant::now() + Duration::from_secs(180);
    let mut playable = false;
    while Instant::now() < deadline {
        if !proc.is_alive() {
            cleanup(&ipc_path, &exe);
            panic!("RPCS3 exited during no-GUI boot");
        }
        if let Ok(state) = driver.read_state()
            && state.is_playable()
        {
            eprintln!(
                "playable: status={} frames={} progr={}/{}",
                state.status, state.frames, state.progr_done, state.progr_total
            );
            playable = true;
            break;
        }
        sleep(Duration::from_millis(250));
    }
    if !playable {
        let _ = proc.shutdown_graceful(Duration::from_secs(10));
        cleanup(&ipc_path, &exe);
        panic!("patched RPCS3 never reached a playable state within 180s");
    }

    // P2: a borderless game-window handle must be published once rendering.
    let handle = driver.window_handle().expect("WINDOW query");
    eprintln!("window handle = 0x{handle:X}");
    assert!(
        handle != 0,
        "borderless game-window handle should be non-zero once playable"
    );

    // Shut down. The no-GUI borderless window has no "RPCS3 " title, so this likely
    // takes the Job-Object force path today — expected (the 16.6.1.3 refinement adds
    // WM_CLOSE-to-the-WINDOW-handle).
    let outcome = proc.shutdown_graceful(Duration::from_secs(10));
    eprintln!("shutdown: {outcome:?}");
    cleanup(&ipc_path, &exe);
}

fn cleanup(ipc_path: &Path, exe: &Path) {
    let _ = std::fs::remove_file(ipc_path);
    if let Some(dir) = exe.parent() {
        let _ = std::fs::remove_file(dir.join("RPCS3.buf"));
    }
}
