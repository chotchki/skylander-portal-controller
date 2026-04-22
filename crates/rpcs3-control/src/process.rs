//! UIA-driven RPCS3 process management (Windows-only).
//!
//! `UiaRpcsProcess` owns either a spawned child (launch) or a handle to an
//! already-running RPCS3 (attach). In either case it exposes a uniform API
//! to wait for readiness, check liveness, and shut it down gracefully.
//!
//! The top-level `RpcsProcess` enum in `lib.rs` wraps this alongside
//! `MockRpcsProcess`; callers use the enum so Mac/Linux dev mode and
//! Windows production share one lifecycle API.
//!
//! Phase 3.1.

use std::path::{Path, PathBuf};
use std::process::{Child, Command};
use std::thread::sleep;
use std::time::{Duration, Instant};

use anyhow::{Context, Result, anyhow, bail};
use tracing::{debug, info, warn};
use uiautomation::types::ControlType;
use uiautomation::{UIAutomation, UIElement};

use crate::ShutdownPath;
use windows::Win32::Foundation::{CloseHandle, HANDLE, HWND, LPARAM, WPARAM};
use windows::Win32::System::JobObjects::{
    AssignProcessToJobObject, CreateJobObjectW, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
    JOBOBJECT_BASIC_LIMIT_INFORMATION, JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
    JobObjectExtendedLimitInformation, SetInformationJobObject, TerminateJobObject,
};
use windows::Win32::System::Threading::{
    GetExitCodeProcess, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_SET_QUOTA,
    PROCESS_TERMINATE, WaitForSingleObject,
};
use windows::Win32::UI::WindowsAndMessaging::{
    EnumWindows, GetWindowTextW, GetWindowThreadProcessId, IsWindowVisible, PostMessageW, WM_CLOSE,
};

const READY_POLL_INTERVAL: Duration = Duration::from_millis(150);
const WINDOW_TITLE_PREFIX: &str = "RPCS3 ";

#[derive(Debug)]
pub struct UiaRpcsProcess {
    inner: ProcessOwnership,
    /// Cached PID from UIA at attach / Child::id at launch.
    pid: u32,
    /// Directory containing `rpcs3.exe` — used to locate `RPCS3.buf` for
    /// post-forced-kill cleanup. `None` for attached processes (we'd have to
    /// resolve the exe path from the PID, which isn't worth it for a feature
    /// that only matters to tests).
    install_dir: Option<PathBuf>,
    /// Job object that holds the spawned RPCS3 *and any descendants it
    /// spawns*. Configured with `KILL_ON_JOB_CLOSE` so terminating the job
    /// (or dropping this handle) reliably kills the whole process tree.
    /// RPCS3 often re-execs itself or forks workers; `Child::kill` alone is
    /// not enough. `None` for attached processes.
    job: Option<JobHandle>,
}

/// `Send + Sync` wrapper around a Job Object HANDLE.
#[derive(Debug)]
struct JobHandle(HANDLE);
// SAFETY: HANDLE is an opaque kernel object id; transferring it between
// threads is safe. The only access we do is Terminate/Close, both of which
// are documented as thread-safe.
unsafe impl Send for JobHandle {}
unsafe impl Sync for JobHandle {}

impl Drop for JobHandle {
    fn drop(&mut self) {
        unsafe {
            // KILL_ON_JOB_CLOSE means the OS terminates every process still
            // assigned to this job when the last handle closes. Belt and
            // braces: also call TerminateJobObject explicitly in case the
            // kernel decides to keep a reference alive longer than us.
            let _ = TerminateJobObject(self.0, 1);
            let _ = CloseHandle(self.0);
        }
    }
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

impl UiaRpcsProcess {
    /// Launch RPCS3 into its **library view** (no EBOOT argument). This is the
    /// path used by UIA-driven control: the main window's menu bar responds to
    /// synthesised keystrokes, and a game is booted afterwards via
    /// `UiaPortalDriver::boot_game_by_serial`.
    ///
    /// Prefer this over `launch(exe, eboot)` whenever anything needs to drive
    /// the menu bar — the EBOOT-argument path puts RPCS3 into direct-boot
    /// state where Alt + arrow keystrokes are swallowed.
    pub fn launch_library(exe: &Path) -> Result<Self> {
        if !exe.is_file() {
            bail!("rpcs3.exe not found at {}", exe.display());
        }
        info!(exe = %exe.display(), "launching RPCS3 (library view)");

        let child = Command::new(exe)
            .spawn()
            .with_context(|| format!("spawn {}", exe.display()))?;
        Self::wrap_spawned(child, exe)
    }

    /// Launch RPCS3 directly into a game by passing its `EBOOT.BIN`. **Legacy
    /// path** — kept for the server handler but not menu-drivable. Prefer
    /// `launch_library` + UIA boot-by-serial for anything that needs the
    /// Manage menu.
    pub fn launch(exe: &Path, eboot: &Path) -> Result<Self> {
        if !exe.is_file() {
            bail!("rpcs3.exe not found at {}", exe.display());
        }
        if !eboot.is_file() {
            bail!("EBOOT.BIN not found at {}", eboot.display());
        }
        info!(exe = %exe.display(), eboot = %eboot.display(), "launching RPCS3 (EBOOT-direct)");

        let child = Command::new(exe)
            .arg(eboot)
            .spawn()
            .with_context(|| format!("spawn {}", exe.display()))?;
        Self::wrap_spawned(child, exe)
    }

    fn wrap_spawned(child: Child, exe: &Path) -> Result<Self> {
        let pid = child.id();
        let job = match create_kill_on_close_job_for_pid(pid) {
            Ok(h) => Some(h),
            Err(e) => {
                warn!(
                    "couldn't create Job Object for RPCS3 pid {pid}: {e} \
                     (shutdown may leave stray processes)"
                );
                None
            }
        };
        Ok(Self {
            inner: ProcessOwnership::Spawned(child),
            pid,
            install_dir: exe.parent().map(PathBuf::from),
            job,
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
            install_dir: None,
            job: None,
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
            if let ProcessOwnership::Spawned(ref mut child) = self.inner
                && let Ok(Some(status)) = child.try_wait()
            {
                bail!("RPCS3 exited before it was ready (status: {:?})", status);
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

        self.wait_for_exit_or_force(timeout)
    }

    /// Wait up to `timeout` for the process to exit; if it's still alive at
    /// the deadline, force-kill via the job object / `TerminateProcess` path
    /// and clean up the orphaned `RPCS3.buf` lockfile.
    ///
    /// The caller is responsible for having first asked RPCS3 to quit —
    /// `shutdown_graceful` does this with a `WM_CLOSE` to the main window,
    /// and the test harness uses `UiaPortalDriver::quit_via_file_menu` to go
    /// through Qt's menu-driven shutdown path (which releases the lockfile
    /// cleanly and avoids the "Another instance is running" warning on the
    /// next launch).
    pub fn wait_for_exit_or_force(&mut self, timeout: Duration) -> Result<ShutdownPath> {
        if !self.is_alive() {
            return Ok(ShutdownPath::AlreadyExited);
        }
        if wait_for_exit(self, timeout) {
            info!("RPCS3 exited gracefully");
            return Ok(ShutdownPath::Graceful);
        }

        warn!("RPCS3 didn't exit within {timeout:?}; forcing");
        match &mut self.inner {
            ProcessOwnership::Spawned(child) => {
                // If we have a job, TerminateJobObject kills every process
                // assigned to it in one shot — including any grandchildren
                // RPCS3 may have forked off (shims, workers). Child::kill
                // alone misses those, which is what leaked processes across
                // test runs before we had job objects.
                if let Some(job) = &self.job {
                    unsafe {
                        if let Err(e) = TerminateJobObject(job.0, 1) {
                            warn!("TerminateJobObject failed: {e}");
                        }
                    }
                }
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
        // RPCS3 writes a singleton lockfile (`RPCS3.buf`) next to the exe and
        // normally removes it on clean exit. A forced kill leaves it orphaned,
        // and the next `launch` bails with "Another instance of RPCS3 is
        // running." Clean it up here.
        if let Some(dir) = &self.install_dir {
            let lock = dir.join("RPCS3.buf");
            match std::fs::remove_file(&lock) {
                Ok(()) => info!(path = %lock.display(), "removed orphaned RPCS3 lockfile"),
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                Err(e) => warn!(path = %lock.display(), "failed to remove RPCS3 lockfile: {e}"),
            }
        }
        Ok(ShutdownPath::Forced)
    }
}

impl Drop for UiaRpcsProcess {
    fn drop(&mut self) {
        // Only reap spawned children — attached processes live on past our
        // handle. No graceful close here; callers are expected to drive
        // `shutdown_graceful` explicitly. This just prevents zombie children
        // if the server process exits without cleanup.
        if let ProcessOwnership::Spawned(ref mut child) = self.inner
            && matches!(child.try_wait(), Ok(None))
        {
            warn!(
                "UiaRpcsProcess dropped without shutdown_graceful; child {} still running",
                self.pid
            );
        }
    }
}

// --- helpers ---

fn find_rpcs3_main_window() -> Option<UIElement> {
    find_rpcs3_main_window_with_pid()
        .ok()
        .flatten()
        .map(|(el, _)| el)
}

/// Read the current title of RPCS3's main window. Returns `None` if
/// RPCS3 isn't running. Cheap (~ms), safe to poll a few times per
/// second.
pub fn read_main_window_title() -> Option<String> {
    enum_first_visible_window(|title| title.starts_with(WINDOW_TITLE_PREFIX))
}

/// Find ANY top-level visible window whose title contains `"compil"`
/// (case-insensitive) or `"cache"`. Returns the matched title.
pub fn find_compile_progress_text() -> Option<String> {
    enum_first_visible_window(|title| {
        let low = title.to_ascii_lowercase();
        low.contains("compil") || low.contains("cache")
    })
}

/// Snapshot ALL top-level visible window titles. Used by the
/// shader-compile watchdog as a diagnostic — by logging every new
/// title that appears we can discover where RPCS3 actually surfaces
/// shader-compile / cache-rebuild progress on the running version
/// (the title of the main window, the FPS viewport, a separate Qt
/// progress dialog, or somewhere else entirely).
///
/// Cheap (~ms). Returns titles in EnumWindows order (top of z-stack
/// first). Empty titles are excluded.
pub fn list_all_visible_window_titles() -> Vec<String> {
    use windows::core::BOOL;

    let mut titles: Vec<String> = Vec::new();

    extern "system" fn enum_proc(hwnd: HWND, lparam: LPARAM) -> BOOL {
        unsafe {
            if !IsWindowVisible(hwnd).as_bool() {
                return BOOL(1);
            }
            let mut buf = [0u16; 256];
            let len = GetWindowTextW(hwnd, &mut buf);
            if len > 0 {
                let title = String::from_utf16_lossy(&buf[..len as usize]);
                let titles = &mut *(lparam.0 as *mut Vec<String>);
                titles.push(title);
            }
            BOOL(1)
        }
    }

    unsafe {
        let _ = EnumWindows(
            Some(enum_proc),
            LPARAM(&mut titles as *mut Vec<String> as isize),
        );
    }
    titles
}

/// Internal helper: enumerate top-level visible windows, return the
/// first title for which `predicate` returns true.
fn enum_first_visible_window<F>(predicate: F) -> Option<String>
where
    F: Fn(&str) -> bool,
{
    use windows::core::BOOL;

    // Predicate is held in `state` so the C callback can call it via
    // a fat-pointer reference. Boxing as `&dyn Fn` keeps the type
    // simple and lets predicate capture from its environment.
    struct State<'a> {
        title: Option<String>,
        predicate: &'a dyn Fn(&str) -> bool,
    }

    extern "system" fn enum_proc(hwnd: HWND, lparam: LPARAM) -> BOOL {
        unsafe {
            if !IsWindowVisible(hwnd).as_bool() {
                return BOOL(1);
            }
            let mut buf = [0u16; 256];
            let len = GetWindowTextW(hwnd, &mut buf);
            if len > 0 {
                let title = String::from_utf16_lossy(&buf[..len as usize]);
                let state = &mut *(lparam.0 as *mut State);
                if (state.predicate)(&title) {
                    state.title = Some(title);
                    return BOOL(0); // stop enumeration
                }
            }
            BOOL(1)
        }
    }

    let mut state = State {
        title: None,
        predicate: &predicate,
    };
    // SAFETY: `state` outlives the EnumWindows call; the callback
    // dereferences the pointer only while EnumWindows is on the
    // stack. EnumWindows itself is thread-safe.
    unsafe {
        let _ = EnumWindows(
            Some(enum_proc),
            LPARAM(&mut state as *mut State as isize),
        );
    }
    state.title
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
        if is_window
            && name.starts_with(WINDOW_TITLE_PREFIX)
            && let Some(hwnd) = native_hwnd(&el)
        {
            let mut pid: u32 = 0;
            unsafe {
                let _ = GetWindowThreadProcessId(hwnd, Some(&mut pid as *mut u32));
            }
            if pid != 0 {
                return Ok(Some((el, pid)));
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

fn wait_for_exit(proc: &mut UiaRpcsProcess, timeout: Duration) -> bool {
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
        let handle: HANDLE =
            OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid).context("OpenProcess")?;
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
    use windows::Win32::System::Threading::{PROCESS_TERMINATE, TerminateProcess};
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

/// Create an unnamed Job Object with `KILL_ON_JOB_CLOSE` and assign the
/// given PID to it. Any processes the PID spawns from this point inherit
/// membership, so terminating the job kills the whole tree.
fn create_kill_on_close_job_for_pid(pid: u32) -> Result<JobHandle> {
    unsafe {
        let job =
            CreateJobObjectW(None, windows::core::PCWSTR::null()).context("CreateJobObjectW")?;
        if job.is_invalid() {
            bail!("CreateJobObjectW returned invalid handle");
        }
        let mut info = JOBOBJECT_EXTENDED_LIMIT_INFORMATION {
            BasicLimitInformation: JOBOBJECT_BASIC_LIMIT_INFORMATION {
                LimitFlags: JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
                ..Default::default()
            },
            ..Default::default()
        };
        SetInformationJobObject(
            job,
            JobObjectExtendedLimitInformation,
            &mut info as *mut _ as *mut std::ffi::c_void,
            std::mem::size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
        )
        .context("SetInformationJobObject(KILL_ON_JOB_CLOSE)")?;

        let child_handle = OpenProcess(PROCESS_TERMINATE | PROCESS_SET_QUOTA, false, pid)
            .context("OpenProcess on child to assign to job")?;
        if child_handle.is_invalid() {
            let _ = CloseHandle(job);
            bail!("OpenProcess returned invalid handle for pid {pid}");
        }
        let assign = AssignProcessToJobObject(job, child_handle);
        let _ = CloseHandle(child_handle);
        assign.context("AssignProcessToJobObject")?;

        Ok(JobHandle(job))
    }
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
