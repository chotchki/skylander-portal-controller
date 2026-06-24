//! Live test for the **16.6.1 no-GUI launch path** against the real patched
//! RPCS3 — `RpcsProcess::launch_no_gui` + IPC readiness + the published window
//! handle. This is the focused acceptance test for what the server's `BootDirect`
//! IPC branch does, minus the server's config / `games.yml` machinery.
//!
//! Runs on **Windows (HTPC)** and **macOS / unix** (PLAN 16.12.4a — the patched
//! RPCS3 + IPC path is cross-platform since the Phase-16 pivot; `UnixRpcsProcess`
//! is wired in 16.11). Spawns + renders a real game, so it stays `#[ignore]`d.
//!
//! Set (paths differ per OS — env always wins over the built-in default):
//!   RPCS3_EXE        = the PATCHED binary
//!                      win: D:\workspace\rpcs3\bin\rpcs3.exe
//!                      mac: <repo>/vendor/rpcs3/build/bin/rpcs3.app/Contents/MacOS/rpcs3
//!   RPCS3_EBOOT      = ...\Skylanders Giants\PS3_GAME\USRDIR\EBOOT.BIN
//!   RPCS3_CONFIG_DIR = the user's RPCS3 data root (firmware + games.yml; inherited
//!                      by the child). mac: ~/Library/Application Support/rpcs3
//!   (optional) SKYLANDER_IPC_PATH    — else the per-platform default is used
//!   (optional) RPCS3_SAMPLE_SECS     — fps sampling window after playable (default 30)
//! then:
//!   cargo test -p skylander-rpcs3-control --test live_launch -- --ignored --nocapture
//!
//! It boots Giants no-GUI, waits for the IPC liveness signal to report playable,
//! then **samples the STATE frame counter over a window to report sustained fps**
//! (the real "is it playable on this hardware" metric — reaching playable only
//! proves boot), asserts the frame counter advanced (not frozen) and that a
//! borderless window handle is published, then shuts down + cleans the lockfile/
//! socket. Missing RPCS3_EXE / EBOOT → skips cleanly.

#![cfg(any(windows, unix))]

use std::path::{Path, PathBuf};
use std::thread::sleep;
use std::time::{Duration, Instant};

use skylander_rpcs3_control::RpcsProcess;
use skylander_rpcs3_control::ipc::{IpcPortalDriver, default_socket_path};

#[cfg(windows)]
const DEFAULT_EXE: &str = r"D:\workspace\rpcs3\bin\rpcs3.exe";
#[cfg(windows)]
const DEFAULT_EBOOT: &str = r"C:\games\ps3\Skylanders Giants\PS3_GAME\USRDIR\EBOOT.BIN";

// macOS dev defaults (this repo's bundled patched build + the user's game backups).
// Any of these can be overridden via the matching env var.
#[cfg(all(unix, target_os = "macos"))]
const DEFAULT_EXE: &str = "vendor/rpcs3/build/bin/rpcs3.app/Contents/MacOS/rpcs3";
#[cfg(all(unix, not(target_os = "macos")))]
const DEFAULT_EXE: &str = "/usr/bin/rpcs3";
#[cfg(unix)]
const DEFAULT_EBOOT: &str = "Skylanders Giants/PS3_GAME/USRDIR/EBOOT.BIN";

fn env_path(key: &str, default: &str) -> PathBuf {
    std::env::var_os(key)
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(default))
}

#[test]
#[ignore = "live: launches the patched RPCS3 no-GUI; set RPCS3_EXE (patched) + RPCS3_EBOOT + RPCS3_CONFIG_DIR"]
fn no_gui_launch_reaches_playable() {
    let exe = env_path("RPCS3_EXE", DEFAULT_EXE);
    let eboot = env_path("RPCS3_EBOOT", DEFAULT_EBOOT);
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
    // A cold first boot compiles PPU/SPU modules and can take minutes, so the
    // deadline is env-tunable (RPCS3_READY_SECS, default 180).
    let ready_secs: u64 = std::env::var("RPCS3_READY_SECS")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(180);
    let deadline = Instant::now() + Duration::from_secs(ready_secs);
    let mut playable = false;
    let mut first_state = None;
    while Instant::now() < deadline {
        if !proc.is_alive() {
            cleanup(&ipc_path, &exe);
            panic!("RPCS3 exited during no-GUI boot");
        }
        if let Ok(state) = driver.read_state() {
            // Fast-fail on a *game* crash. When an emulated thread hits a fatal
            // (the SPU-recompiler range-check, an unhandled guest fault, …) RPCS3
            // freezes that thread and reports `status=frozen` but the *process*
            // stays alive — so `is_alive()` never trips. Without this, a crash
            // would burn the full ready deadline. Detecting `frozen`/`stopped`
            // and killing the emulator is what makes triage iteration fast.
            if state.status == "frozen" || state.status == "stopped" {
                eprintln!(
                    "CRASH: emulator status={} frames={} progr={}/{} — the game crashed/froze during boot",
                    state.status, state.frames, state.progr_done, state.progr_total
                );
                let outcome = proc.shutdown_graceful(Duration::from_secs(10));
                eprintln!("killed emulator: {outcome:?}");
                cleanup(&ipc_path, &exe);
                panic!(
                    "game crashed (status={}) — emulator killed, not waiting out the {ready_secs}s deadline",
                    state.status
                );
            }
            if state.is_playable() {
                eprintln!(
                    "playable: status={} frames={} progr={}/{}",
                    state.status, state.frames, state.progr_done, state.progr_total
                );
                first_state = Some(state);
                playable = true;
                break;
            }
        }
        sleep(Duration::from_millis(250));
    }
    if !playable {
        let _ = proc.shutdown_graceful(Duration::from_secs(10));
        cleanup(&ipc_path, &exe);
        panic!("patched RPCS3 never reached a playable state within {ready_secs}s");
    }

    // Sustained-fps sample: reaching `playable` only proves boot. Sample the STATE
    // frame counter over a window to get the real "does it run at speed on this
    // hardware" number — and to catch a post-boot freeze (running status but a
    // stalled frame counter, the freeze signal `is_playable` can't see in one shot).
    let sample_secs: u64 = std::env::var("RPCS3_SAMPLE_SECS")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(30);
    let frames0 = first_state.map(|s| s.frames).unwrap_or(0);
    let t0 = Instant::now();
    let sample_deadline = t0 + Duration::from_secs(sample_secs);
    let mut last_frames = frames0;
    let mut last_t = t0;
    while Instant::now() < sample_deadline {
        sleep(Duration::from_secs(3));
        if !proc.is_alive() {
            cleanup(&ipc_path, &exe);
            panic!("RPCS3 exited mid-sample");
        }
        if let Ok(state) = driver.read_state() {
            let now = Instant::now();
            let dt = now.duration_since(last_t).as_secs_f64();
            let inst = (state.frames.saturating_sub(last_frames)) as f64 / dt.max(0.001);
            eprintln!(
                "  +{:>4.1}s status={} frames={} (~{:.1} fps inst)",
                now.duration_since(t0).as_secs_f64(),
                state.status,
                state.frames,
                inst
            );
            // A crash mid-sample: explicit `frozen`, or `running` with a stalled
            // frame counter (the subtle freeze the proto warns about). Either way,
            // kill the emulator and fail fast rather than sampling dead air.
            if state.status == "frozen" || state.status == "stopped" {
                let outcome = proc.shutdown_graceful(Duration::from_secs(10));
                eprintln!(
                    "CRASH mid-sample: status={}; killed emulator: {outcome:?}",
                    state.status
                );
                cleanup(&ipc_path, &exe);
                panic!("game crashed mid-sample (status={})", state.status);
            }
            last_frames = state.frames;
            last_t = now;
        }
    }
    let elapsed = last_t.duration_since(t0).as_secs_f64().max(0.001);
    let advanced = last_frames.saturating_sub(frames0);
    let avg_fps = advanced as f64 / elapsed;
    eprintln!(
        "SUSTAINED: {advanced} frames over {elapsed:.1}s = {avg_fps:.1} fps avg \
         (SPU/PPU decoder + renderer from the config_dir)"
    );
    assert!(
        advanced > 0,
        "frame counter did not advance during the {sample_secs}s sample — \
         RPCS3 booted but is frozen (status was running, frames stalled)"
    );

    // P2: a borderless game-window handle should be published once rendering.
    // On Windows this is load-bearing (the launcher coordinates z-order off it);
    // on macOS window coordination is out of scope (16.11), so a missing handle is
    // logged, not fatal — the fps assertion above is the real macOS acceptance gate.
    let handle = driver.window_handle().unwrap_or(0);
    eprintln!("window handle = 0x{handle:X}");
    #[cfg(windows)]
    assert!(
        handle != 0,
        "borderless game-window handle should be non-zero once playable"
    );
    #[cfg(not(windows))]
    if handle == 0 {
        eprintln!("note: no window handle published (macOS window coordination is out of scope)");
    }

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
