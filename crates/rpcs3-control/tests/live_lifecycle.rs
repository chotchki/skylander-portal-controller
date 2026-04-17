//! Heavier live e2e: launch → boot-by-serial → drive → quit via File→Exit.
//!
//! Unlike `tests/live.rs` (assumes RPCS3 + manager already open) and
//! `tests/process.rs` (just launches and shuts down), these combine the full
//! path. All tests are `#[ignore]` — they require:
//!
//!   RPCS3_EXE=C:/emuluators/rpcs3/rpcs3.exe
//!   RPCS3_TEST_SERIAL=BLUS30968      # game serial in the RPCS3 library
//!   RPCS3_SKY_TEST_PATH=C:/.../Eruptor.sky
//!
//! Run:
//!   cargo test -p skylander-rpcs3-control --test live_lifecycle -- --ignored --nocapture
//!
//! The .sky path should point at a figure supported by the test game — Eruptor
//! works for SSA/Giants.
//!
//! Session isolation: UIA + SendInput are session-bound. Run from the user's
//! interactive desktop (not SSH/RDP from another machine) or these tests
//! cannot see the RPCS3 window at all.

#![cfg(windows)]

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use skylander_core::{SlotIndex, SlotState};
use skylander_rpcs3_control::{PortalDriver, RpcsProcess, ShutdownPath, UiaPortalDriver};

use windows::Win32::Foundation::{HWND, LPARAM, RECT};
use windows::core::BOOL;
use windows::Win32::UI::WindowsAndMessaging::{
    EnumWindows, GetClassNameW, GetWindowRect, GetWindowTextLengthW, GetWindowTextW,
    IsWindowVisible,
};

// ---------- env gating ----------

fn env_path(key: &str) -> Option<PathBuf> {
    std::env::var(key).ok().map(PathBuf::from)
}

fn env_string(key: &str) -> Option<String> {
    std::env::var(key).ok()
}

/// (exe, serial, sky). Serial is the library-grid cell name (e.g.
/// `"BLUS30968"`), not a path.
fn require_env() -> Option<(PathBuf, String, PathBuf)> {
    let exe = env_path("RPCS3_EXE")?;
    let serial = env_string("RPCS3_TEST_SERIAL")?;
    let sky = env_path("RPCS3_SKY_TEST_PATH")?;
    Some((exe, serial, sky))
}

// ---------- shared setup helper ----------

/// Launch RPCS3, open the Skylanders Manager dialog from a cold library view,
/// then UIA-boot the test game by serial. Returns the owned process handle
/// (caller must run it through `teardown`) and the driver.
///
/// **Order matters**: dialog first, game second. This mirrors the real-app
/// flow (the server opens the manager dialog before the user picks a game)
/// and avoids the much harder post-boot menu-navigation case — with a game
/// running, Qt's focus state is too scrambled for reliable Alt-menu
/// traversal (see PLAN 3.7 debug trail). Game-change workflows instead
/// kill + relaunch RPCS3 so each session starts from this cold state.
fn open_and_boot() -> (RpcsProcess, UiaPortalDriver) {
    let (exe, serial, _sky) = require_env().expect("env vars set (pre-checked)");

    // If a previous test's teardown hit the Forced path and the cleanup
    // didn't settle before this one starts, the lockfile blocks launch with
    // "Another instance of RPCS3 is running". Best-effort clear.
    if let Some(dir) = exe.parent() {
        let _ = std::fs::remove_file(dir.join("RPCS3.buf"));
    }

    let mut proc = RpcsProcess::launch_library(&exe).expect("launch RPCS3 library view");
    proc.wait_ready(Duration::from_secs(45))
        .expect("RPCS3 window ready within 45s");

    let driver = UiaPortalDriver::new().expect("construct driver");

    // Open dialog from the cold library view — this is the 3.6b-proven path.
    driver.open_dialog().expect("open Skylanders Manager dialog");

    // Now boot the game by serial. The dialog is already off-screen; it
    // stays open for the rest of the session and the driver short-circuits
    // subsequent `open_dialog` calls.
    driver
        .boot_game_by_serial(&serial, Duration::from_secs(60))
        .expect("UIA-boot game by serial");

    // Verify the booted game matches the requested serial. Without this
    // check the test silently passed even when `boot_game_by_serial` booted
    // the default game instead of the target, because `load()` operates on
    // the Skylanders Manager (cross-game) and doesn't care which game is
    // running. The viewport title contains the game's display name; we
    // match that against a serial→name map rather than the serial itself
    // (RPCS3 doesn't put the serial in the title).
    let expected_name = expected_game_name_for_serial(&serial);
    // Give Qt a beat to finalise the title once the FPS counter starts.
    thread::sleep(Duration::from_secs(1));
    let title = driver
        .running_viewport_title()
        .expect("viewport title readable after boot");
    assert!(
        title.contains(expected_name),
        "booted wrong game: expected viewport title to contain {expected_name:?}, got {title:?} \
         (requested serial {serial})"
    );

    (proc, driver)
}

/// Known game-serial → substring of viewport title. Kept as a flat match
/// rather than anything cleverer because this only backs the live-lifecycle
/// test harness.
fn expected_game_name_for_serial(serial: &str) -> &'static str {
    match serial {
        "BLUS30719" | "BLES00867" => "Spyro's Adventure",
        "BLUS30968" => "Giants",
        "BLUS31076" => "SWAP Force",
        "BLUS31442" => "Trap Team",
        "BLUS31545" => "SuperChargers",
        "BLUS31600" => "Imaginators",
        _ => panic!("unknown test serial {serial}; extend expected_game_name_for_serial"),
    }
}

/// Prefer a clean File→Exit shutdown so RPCS3 releases its lockfile normally.
/// On nav failure, fall back to the forced path so tests don't leak RPCS3.
fn teardown(mut proc: RpcsProcess, driver: UiaPortalDriver) {
    let path = match driver.quit_via_file_menu() {
        Ok(()) => proc
            .wait_for_exit_or_force(Duration::from_secs(30))
            .expect("wait_for_exit_or_force"),
        Err(e) => {
            eprintln!("quit_via_file_menu failed ({e}); falling back to shutdown_graceful");
            proc.shutdown_graceful(Duration::from_secs(30))
                .expect("shutdown_graceful")
        }
    };
    assert!(
        matches!(
            path,
            ShutdownPath::Graceful | ShutdownPath::Forced | ShutdownPath::AlreadyExited
        ),
        "unexpected shutdown path {path:?}"
    );
    assert!(!proc.is_alive(), "process should be dead after shutdown");
}

// ---------- 3.7.2 ----------

#[test]
#[ignore = "requires RPCS3_EXE, RPCS3_TEST_SERIAL, RPCS3_SKY_TEST_PATH"]
fn lifecycle_launch_load_clear_quit() {
    let (_exe, _serial, sky) = match require_env() {
        Some(t) => t,
        None => return,
    };

    let (proc, driver) = open_and_boot();

    // Tear down regardless of assertion outcome to avoid leaking RPCS3
    // processes across test runs.
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let slot = SlotIndex::new(0).unwrap();
        driver.clear(slot).expect("clear slot 0");
        assert!(matches!(driver.read_slots().unwrap()[0], SlotState::Empty));

        let name = driver.load(slot, &sky).expect("load figure");
        eprintln!("loaded: {name}");
        assert!(!name.is_empty());

        match &driver.read_slots().unwrap()[0] {
            SlotState::Loaded { display_name, .. } => assert_eq!(display_name, &name),
            s => panic!("expected Loaded, got {s:?}"),
        }

        driver.clear(slot).expect("clear at end");
        assert!(matches!(driver.read_slots().unwrap()[0], SlotState::Empty));
    }));

    teardown(proc, driver);
    if let Err(panic) = result {
        std::panic::resume_unwind(panic);
    }
}

// ---------- 3.7.3 ----------

#[test]
#[ignore = "requires RPCS3_EXE, RPCS3_TEST_SERIAL, RPCS3_SKY_TEST_PATH"]
fn offscreen_hide_really_hides() {
    if require_env().is_none() {
        return;
    }
    let (proc, driver) = open_and_boot();

    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        // Pre-condition: manager is on-screen with a positive x.
        let before = find_window_by_title("Skylanders Manager")
            .expect("dialog HWND findable pre-hide");
        let rect_before = window_rect(before).expect("rect pre-hide");
        assert!(
            rect_before.left >= 0 && rect_before.top >= 0,
            "dialog should start on-screen, got {rect_before:?}"
        );

        driver
            .hide_dialog_offscreen()
            .expect("hide_dialog_offscreen");
        thread::sleep(Duration::from_millis(200));

        let hidden = find_window_by_title("Skylanders Manager")
            .expect("dialog HWND still findable after hide");
        let rect_hidden = window_rect(hidden).expect("rect post-hide");
        assert!(
            rect_hidden.left < -1000 && rect_hidden.top < -1000,
            "dialog should be far off-screen, got {rect_hidden:?}"
        );

        // Restore so teardown's graceful shutdown doesn't trip on a hidden dialog.
        driver
            .restore_dialog_visible(100, 100)
            .expect("restore_dialog_visible");
        thread::sleep(Duration::from_millis(200));

        let restored = find_window_by_title("Skylanders Manager")
            .expect("dialog HWND findable post-restore");
        let rect_restored = window_rect(restored).expect("rect post-restore");
        assert!(
            rect_restored.left >= 0 && rect_restored.top >= 0,
            "dialog should be on-screen after restore, got {rect_restored:?}"
        );
    }));

    teardown(proc, driver);
    if let Err(panic) = result {
        std::panic::resume_unwind(panic);
    }
}

// ---------- 3.7.4 ----------

#[test]
#[ignore = "requires RPCS3_EXE, RPCS3_TEST_SERIAL, RPCS3_SKY_TEST_PATH"]
fn file_dialog_hidden_while_manager_hidden() {
    let (_exe, _serial, sky) = match require_env() {
        Some(t) => t,
        None => return,
    };

    let (proc, driver) = open_and_boot();

    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let slot = SlotIndex::new(0).unwrap();
        driver.clear(slot).expect("clear slot 0");

        driver
            .hide_dialog_offscreen()
            .expect("hide_dialog_offscreen");
        thread::sleep(Duration::from_millis(300));

        let stop = Arc::new(AtomicBool::new(false));
        let samples: Arc<Mutex<Vec<RECT>>> = Arc::new(Mutex::new(Vec::new()));

        let sampler = {
            let stop = Arc::clone(&stop);
            let samples = Arc::clone(&samples);
            thread::spawn(move || {
                while !stop.load(Ordering::Relaxed) {
                    if let Some(hwnd) = find_file_dialog_visible() {
                        if let Some(r) = window_rect(hwnd) {
                            samples.lock().unwrap().push(r);
                        }
                    }
                    thread::sleep(Duration::from_millis(30));
                }
            })
        };

        let load_result = driver.load(slot, &sky);

        stop.store(true, Ordering::Relaxed);
        sampler.join().expect("sampler join");

        load_result.expect("load figure while manager hidden");

        let seen = samples.lock().unwrap().clone();
        // We don't require that the file dialog was observed — UIA may drive
        // it faster than the 30ms poll in some runs — but if it was observed,
        // every sample must be off-screen. An on-screen sample is a regression.
        let onscreen: Vec<_> = seen
            .iter()
            .filter(|r| r.left > -1000 || r.top > -1000)
            .collect();
        assert!(
            onscreen.is_empty(),
            "file-dialog appeared on-screen during load (manager was hidden): {onscreen:?}"
        );
        eprintln!(
            "file_dialog_hidden_while_manager_hidden: saw {} samples, all off-screen",
            seen.len()
        );

        driver.clear(slot).ok();
        driver
            .restore_dialog_visible(100, 100)
            .expect("restore_dialog_visible");
    }));

    teardown(proc, driver);
    if let Err(panic) = result {
        std::panic::resume_unwind(panic);
    }
}

// ---------- window enumeration helpers (test-local) ----------

fn window_rect(hwnd: HWND) -> Option<RECT> {
    let mut r = RECT::default();
    unsafe {
        if GetWindowRect(hwnd, &mut r).is_ok() {
            Some(r)
        } else {
            None
        }
    }
}

fn window_title(hwnd: HWND) -> Option<String> {
    unsafe {
        let len = GetWindowTextLengthW(hwnd);
        if len <= 0 {
            return None;
        }
        let mut buf = vec![0u16; (len + 1) as usize];
        let read = GetWindowTextW(hwnd, &mut buf);
        if read <= 0 {
            return None;
        }
        Some(String::from_utf16_lossy(&buf[..read as usize]))
    }
}

fn window_class(hwnd: HWND) -> Option<String> {
    unsafe {
        let mut buf = [0u16; 256];
        let n = GetClassNameW(hwnd, &mut buf);
        if n <= 0 {
            return None;
        }
        Some(String::from_utf16_lossy(&buf[..n as usize]))
    }
}

fn find_window_by_title(exact: &str) -> Option<HWND> {
    struct Ctx {
        want: String,
        hit: Option<HWND>,
    }
    extern "system" fn proc(hwnd: HWND, lparam: LPARAM) -> BOOL {
        let ctx = unsafe { &mut *(lparam.0 as *mut Ctx) };
        if let Some(t) = window_title(hwnd) {
            if t == ctx.want {
                // Skip the invisible Qt helper HWNDs — pick the QWindowIcon one.
                if let Some(cls) = window_class(hwnd) {
                    if cls.contains("QWindowIcon") {
                        ctx.hit = Some(hwnd);
                        return BOOL(0);
                    }
                    // Fallback: record any match but keep looking.
                    if ctx.hit.is_none() {
                        ctx.hit = Some(hwnd);
                    }
                }
            }
        }
        BOOL(1)
    }
    let mut ctx = Ctx {
        want: exact.to_string(),
        hit: None,
    };
    unsafe {
        let lparam = LPARAM(&mut ctx as *mut _ as isize);
        let _ = EnumWindows(Some(proc), lparam);
    }
    ctx.hit
}

/// Find the visible Windows common file dialog (class `#32770`). Only returns
/// visible windows — Qt's hidden parent helpers are ignored.
fn find_file_dialog_visible() -> Option<HWND> {
    struct Ctx {
        hit: Option<HWND>,
    }
    extern "system" fn proc(hwnd: HWND, lparam: LPARAM) -> BOOL {
        let ctx = unsafe { &mut *(lparam.0 as *mut Ctx) };
        unsafe {
            if !IsWindowVisible(hwnd).as_bool() {
                return BOOL(1);
            }
        }
        if let Some(cls) = window_class(hwnd) {
            if cls == "#32770" {
                ctx.hit = Some(hwnd);
                return BOOL(0);
            }
        }
        BOOL(1)
    }
    let mut ctx = Ctx { hit: None };
    unsafe {
        let lparam = LPARAM(&mut ctx as *mut _ as isize);
        let _ = EnumWindows(Some(proc), lparam);
    }
    ctx.hit
}

// Suppress "unused" warnings when reading samples timing.
#[allow(dead_code)]
fn _now() -> Instant {
    Instant::now()
}
