//! PLAN 10.8.4 — integration smoke for the refactored production driver.
//! Launches RPCS3 with an EBOOT.BIN argument, waits for the game viewport,
//! then calls `UiaPortalDriver::open_dialog()` (the real production path —
//! pattern-driven Manage → Portals and Gates → Skylanders Portal nav) and
//! verifies the Skylanders Manager dialog actually opens.
//!
//!   cargo run -p skylander-rpcs3-control --example production_open_dialog \
//!       --features uia-examples -- BLUS30968
//!
//! Mirrors `boot_via_eboot` but uses the production driver code path
//! instead of inline pattern calls — proves the refactor works end-to-end.

#![cfg(windows)]

use std::path::{Path, PathBuf};
use std::process::Command;
use std::thread::sleep;
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail};
use skylander_rpcs3_control::{PortalDriver, UiaPortalDriver};
use windows::Win32::Foundation::{HANDLE, LPARAM};
use windows::Win32::System::JobObjects::{
    AssignProcessToJobObject, CreateJobObjectW, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
    JOBOBJECT_BASIC_LIMIT_INFORMATION, JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
    JobObjectExtendedLimitInformation, SetInformationJobObject,
};
use windows::Win32::System::Threading::{OpenProcess, PROCESS_SET_QUOTA, PROCESS_TERMINATE};
use windows::Win32::UI::WindowsAndMessaging::{EnumWindows, GetWindowTextLengthW, GetWindowTextW};
use windows::core::BOOL;

const RPCS3_EXE: &str = r"C:\emuluators\rpcs3\rpcs3.exe";
const GAMES_YML: &str = r"C:\emuluators\rpcs3\config\games.yml";

fn main() -> Result<()> {
    let serial = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "BLUS30968".to_string());
    eprintln!("[harness] target serial: {serial}");

    let game_dir = lookup_game_dir(&serial).context("look up game dir")?;
    let eboot = game_dir.join("PS3_GAME").join("USRDIR").join("EBOOT.BIN");
    if !eboot.is_file() {
        bail!("EBOOT.BIN not found at {}", eboot.display());
    }
    eprintln!("[harness] eboot: {}", eboot.display());

    eprintln!("[harness] spawning rpcs3.exe + EBOOT...");
    let child = Command::new(RPCS3_EXE)
        .arg(&eboot)
        .spawn()
        .context("spawn rpcs3.exe")?;
    let pid = child.id();
    let _job = JobOnDrop::for_pid(pid);
    eprintln!("[harness] pid={pid}");

    // Wait for the game to actually be running (FPS: viewport).
    eprintln!("[harness] waiting for FPS: viewport...");
    let deadline = Instant::now() + Duration::from_secs(120);
    while Instant::now() < deadline {
        if find_top_level_with_prefix("FPS:").is_some() {
            eprintln!("[harness] viewport present — game running");
            break;
        }
        sleep(Duration::from_millis(250));
    }
    if find_top_level_with_prefix("FPS:").is_none() {
        bail!("FPS: viewport never appeared (game didn't boot)");
    }

    // The actual test — call the production driver's open_dialog().
    eprintln!("[harness] calling UiaPortalDriver::open_dialog()...");
    let driver = UiaPortalDriver::new().context("construct driver")?;
    driver.open_dialog().context("open_dialog failed")?;

    // Verify the dialog window exists by title.
    if find_top_level_with_exact_title("Skylanders Manager").is_some() {
        eprintln!("[harness] ✓ Skylanders Manager dialog is present (production driver works)");
    } else {
        bail!("open_dialog() returned Ok but no 'Skylanders Manager' top-level window exists");
    }

    eprintln!("[harness] killing rpcs3 (job object close)...");
    drop(child);
    Ok(())
}

fn lookup_game_dir(serial: &str) -> Result<PathBuf> {
    let body = std::fs::read_to_string(GAMES_YML).with_context(|| format!("read {GAMES_YML}"))?;
    for line in body.lines() {
        let Some((k, v)) = line.split_once(':') else {
            continue;
        };
        if k.trim() == serial {
            let path = v
                .trim()
                .trim_matches('"')
                .trim_end_matches('/')
                .trim_end_matches('\\');
            return Ok(PathBuf::from(path));
        }
    }
    bail!("serial {serial:?} not found in {GAMES_YML}")
}

fn find_top_level_with_prefix(prefix: &str) -> Option<windows::Win32::Foundation::HWND> {
    enum_top_level(|hwnd| {
        read_title(hwnd)
            .map(|t| t.starts_with(prefix))
            .unwrap_or(false)
    })
}

fn find_top_level_with_exact_title(title: &str) -> Option<windows::Win32::Foundation::HWND> {
    let want = title.to_string();
    enum_top_level(move |hwnd| read_title(hwnd).map(|t| t == want).unwrap_or(false))
}

fn enum_top_level<F>(mut pred: F) -> Option<windows::Win32::Foundation::HWND>
where
    F: FnMut(windows::Win32::Foundation::HWND) -> bool,
{
    struct Ctx<'a> {
        pred: &'a mut dyn FnMut(windows::Win32::Foundation::HWND) -> bool,
        hit: Option<windows::Win32::Foundation::HWND>,
    }
    extern "system" fn proc(hwnd: windows::Win32::Foundation::HWND, lparam: LPARAM) -> BOOL {
        let ctx = unsafe { &mut *(lparam.0 as *mut Ctx) };
        if (ctx.pred)(hwnd) {
            ctx.hit = Some(hwnd);
            return BOOL(0);
        }
        BOOL(1)
    }
    let mut ctx = Ctx {
        pred: &mut pred,
        hit: None,
    };
    unsafe {
        let lp = LPARAM(&mut ctx as *mut _ as isize);
        let _ = EnumWindows(Some(proc), lp);
    }
    ctx.hit
}

fn read_title(hwnd: windows::Win32::Foundation::HWND) -> Option<String> {
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

struct JobOnDrop {
    handle: HANDLE,
}

impl JobOnDrop {
    fn for_pid(pid: u32) -> Option<Self> {
        unsafe {
            let job = CreateJobObjectW(None, None).ok()?;
            let mut info = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
            info.BasicLimitInformation = JOBOBJECT_BASIC_LIMIT_INFORMATION {
                LimitFlags: JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
                ..Default::default()
            };
            SetInformationJobObject(
                job,
                JobObjectExtendedLimitInformation,
                &info as *const _ as *const _,
                std::mem::size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
            )
            .ok()?;
            let proc = OpenProcess(PROCESS_TERMINATE | PROCESS_SET_QUOTA, false, pid).ok()?;
            AssignProcessToJobObject(job, proc).ok()?;
            let _ = windows::Win32::Foundation::CloseHandle(proc);
            Some(Self { handle: job })
        }
    }
}

impl Drop for JobOnDrop {
    fn drop(&mut self) {
        unsafe {
            let _ = windows::Win32::Foundation::CloseHandle(self.handle);
        }
    }
}

fn _phantom(_p: &Path) {}
