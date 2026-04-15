//! RPCS3 process management.
//!
//! `RpcsProcess` owns either a spawned child (launch) or a handle to an
//! already-running RPCS3 (attach). In either case it exposes a uniform API
//! to wait for readiness, check liveness, and shut it down gracefully.
//!
//! Phase 3.1.

use std::path::Path;
use std::process::{Child, Command};
use std::thread::sleep;
use std::time::{Duration, Instant};

use anyhow::{anyhow, bail, Context, Result};
use tracing::{debug, info, warn};
use uiautomation::types::ControlType;
use uiautomation::{UIAutomation, UIElement};
use windows::Win32::Foundation::{CloseHandle, HANDLE, HWND, LPARAM, WPARAM};
use windows::Win32::System::Threading::{
    GetExitCodeProcess, OpenProcess, WaitForSingleObject, PROCESS_QUERY_LIMITED_INFORMATION,
};
use windows::Win32::UI::WindowsAndMessaging::{
    GetWindowThreadProcessId, PostMessageW, WM_CLOSE,
};

const READY_POLL_INTERVAL: Duration = Duration::from_millis(150);
const WINDOW_TITLE_PREFIX: &str = "RPCS3 ";

#[derive(Debug)]
pub struct RpcsProcess {
    inner: ProcessOwnership,
    /// Cached PID from UIA at attach / Child::id at launch.
    pid: u32,
}

/// How we got hold of this RPCS3.
///
/// `Spawned` owns the `std::process::Child` and can kill it. `Attached` only
/// has the PID — shutdown goes through WM_CLOSE and a WaitForSingleObject on
/// the process handle.
#[derive(Debug)]
enum ProcessOwnership {
    Spawned(Child),
    Attached,
}

impl RpcsProcess {
    /// Launch a fresh RPCS3 instance with the given game's EBOOT.BIN. Does
    /// NOT pass `--no-gui` — we need the Manage menu to drive the portal
    /// dialog.
    pub fn launch(exe: &Path, eboot: &Path) -> Result<Self> {
        if !exe.is_file() {
            bail!("rpcs3.exe not found at {}", exe.display());
        }
        if !eboot.is_file() {
            bail!("EBOOT.BIN not found at {}", eboot.display());
        }
        info!(exe = %exe.display(), eboot = %eboot.display(), "launching RPCS3");

        let child = Command::new(exe)
            .arg(eboot)
            .spawn()
            .with_context(|| format!("spawn {}", exe.display()))?;
        let pid = child.id();
        Ok(Self {
            inner: ProcessOwnership::Spawned(child),
            pid,
        })
    }

    /// Adopt an already-running RPCS3. Finds the first top-level window whose
    /// name starts with "RPCS3 " and resolves its owning PID.
    pub fn attach() -> Result<Self> {
        let (_el, pid) = find_rpcs3_main_window_with_pid()?
            .ok_or_else(|| anyhow!("no running RPCS3 window found"))?;
        info!(pid, "attached to running RPCS3");
        Ok(Self {
            inner: ProcessOwnership::Attached,
            pid,
        })
    }

    pub fn pid(&self) -> u32 {
        self.pid
    }

    /// Poll UIA until the RPCS3 main window is present, or the child exits,
    /// or we hit `timeout`.
    pub fn wait_ready(&mut self, timeout: Duration) -> Result<()> {
        let deadline = Instant::now() + timeout;
        loop {
            // If we spawned the child, check it hasn't already died.
            if let ProcessOwnership::Spawned(ref mut child) = self.inner {
                if let Ok(Some(status)) = child.try_wait() {
                    bail!("RPCS3 exited before it was ready (status: {:?})", status);
                }
            }
            if let Some(_el) = find_rpcs3_main_window() {
                debug!("RPCS3 main window detected");
                return Ok(());
            }
            if Instant::now() >= deadline {
                bail!("RPCS3 main window didn't appear within {timeout:?}");
            }
            sleep(READY_POLL_INTERVAL);
        }
    }

    /// Non-blocking liveness check.
    pub fn is_alive(&mut self) -> bool {
        match &mut self.inner {
            ProcessOwnership::Spawned(child) => matches!(child.try_wait(), Ok(None)),
            ProcessOwnership::Attached => is_pid_alive(self.pid).unwrap_or(false),
        }
    }

    /// Send `WM_CLOSE` to the main window and wait up to `timeout` for the
    /// process to exit. If it doesn't, fall back to `Child::kill` for
    /// spawned children, or `TerminateProcess` via a process handle for
    /// attached ones. Returns the path taken.
    pub fn shutdown_graceful(&mut self, timeout: Duration) -> Result<ShutdownPath> {
        if !self.is_alive() {
            return Ok(ShutdownPath::AlreadyExited);
        }

        // Try a polite WM_CLOSE.
        if let Some(el) = find_rpcs3_main_window() {
            if let Some(hwnd) = native_hwnd(&el) {
                debug!(?hwnd, "posting WM_CLOSE to RPCS3");
                unsafe {
                    let _ = PostMessageW(Some(hwnd), WM_CLOSE, WPARAM(0), LPARAM(0));
                }
            }
        } else {
            warn!("no RPCS3 main window found; skipping WM_CLOSE and waiting anyway");
        }

        if wait_for_exit(self, timeout) {
            info!("RPCS3 exited gracefully");
            return Ok(ShutdownPath::Graceful);
        }

        warn!("RPCS3 didn't exit within {timeout:?}; forcing");
        match &mut self.inner {
            ProcessOwnership::Spawned(child) => {
                let _ = child.kill();
                let _ = child.wait();
            }
            ProcessOwnership::Attached => {
                // We don't own the Child, so reach for TerminateProcess.
                if let Err(e) = terminate_pid(self.pid) {
                    warn!("TerminateProcess failed for pid {}: {e}", self.pid);
                }
            }
        }
        Ok(ShutdownPath::Forced)
    }
}

impl Drop for RpcsProcess {
    fn drop(&mut self) {
        // Only reap spawned children — attached processes live on past our
        // handle. No graceful close here; callers are expected to drive
        // `shutdown_graceful` explicitly. This just prevents zombie children
        // if the server process exits without cleanup.
        if let ProcessOwnership::Spawned(ref mut child) = self.inner {
            if matches!(child.try_wait(), Ok(None)) {
                warn!(
                    "RpcsProcess dropped without shutdown_graceful; child {} still running",
                    self.pid
                );
            }
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShutdownPath {
    AlreadyExited,
    Graceful,
    Forced,
}

// --- helpers ---

fn find_rpcs3_main_window() -> Option<UIElement> {
    find_rpcs3_main_window_with_pid().ok().flatten().map(|(el, _)| el)
}

fn find_rpcs3_main_window_with_pid() -> Result<Option<(UIElement, u32)>> {
    let automation = UIAutomation::new().context("init UIAutomation")?;
    let walker = automation.create_tree_walker()?;
    let root = automation.get_root_element()?;
    let mut cur = walker.get_first_child(&root).ok();
    while let Some(el) = cur.clone() {
        let is_window = el
            .get_control_type()
            .map(|c| c == ControlType::Window)
            .unwrap_or(false);
        let name = el.get_name().unwrap_or_default();
        if is_window && name.starts_with(WINDOW_TITLE_PREFIX) {
            if let Some(hwnd) = native_hwnd(&el) {
                let mut pid: u32 = 0;
                unsafe {
                    let _ = GetWindowThreadProcessId(hwnd, Some(&mut pid as *mut u32));
                }
                if pid != 0 {
                    return Ok(Some((el, pid)));
                }
            }
        }
        cur = walker.get_next_sibling(&el).ok();
    }
    Ok(None)
}

fn native_hwnd(el: &UIElement) -> Option<HWND> {
    let raw = el.get_native_window_handle().ok()?;
    let hwnd_raw: isize = raw.into();
    if hwnd_raw == 0 {
        None
    } else {
        Some(HWND(hwnd_raw as _))
    }
}

fn wait_for_exit(proc: &mut RpcsProcess, timeout: Duration) -> bool {
    let deadline = Instant::now() + timeout;
    loop {
        if !proc.is_alive() {
            return true;
        }
        if Instant::now() >= deadline {
            return false;
        }
        sleep(Duration::from_millis(100));
    }
}

fn is_pid_alive(pid: u32) -> Result<bool> {
    if pid == 0 {
        return Ok(false);
    }
    unsafe {
        let handle: HANDLE = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid)
            .context("OpenProcess")?;
        if handle.is_invalid() {
            return Ok(false);
        }
        let mut exit_code: u32 = 0;
        let ok = GetExitCodeProcess(handle, &mut exit_code as *mut u32).is_ok();
        let _ = CloseHandle(handle);
        // STILL_ACTIVE == 259
        Ok(ok && exit_code == 259)
    }
}

fn terminate_pid(pid: u32) -> Result<()> {
    use windows::Win32::System::Threading::{TerminateProcess, PROCESS_TERMINATE};
    unsafe {
        let handle: HANDLE =
            OpenProcess(PROCESS_TERMINATE, false, pid).context("OpenProcess for terminate")?;
        if handle.is_invalid() {
            bail!("couldn't open process {pid}");
        }
        let r = TerminateProcess(handle, 1);
        let _ = CloseHandle(handle);
        r.context("TerminateProcess")?;
    }
    Ok(())
}

/// Convenience for tests/polling code that just wants to block.
#[allow(dead_code)]
fn wait_handle_briefly(pid: u32, timeout: Duration) -> Result<bool> {
    use windows::Win32::System::Threading::PROCESS_SYNCHRONIZE;
    unsafe {
        let handle = OpenProcess(PROCESS_SYNCHRONIZE, false, pid).context("OpenProcess")?;
        if handle.is_invalid() {
            return Ok(true);
        }
        let ms = timeout.as_millis().min(u32::MAX as u128) as u32;
        let wait = WaitForSingleObject(handle, ms);
        let _ = CloseHandle(handle);
        // WAIT_OBJECT_0 == 0 means signalled (process exited).
        Ok(wait.0 == 0)
    }
}
