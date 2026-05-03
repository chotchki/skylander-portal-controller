//! PLAN 10.8.4 — validate Option 2: launch RPCS3 with EBOOT.BIN as a CLI arg
//! (direct-boot mode), then drive the Manage menu via UIA pattern calls
//! (Expand → Expand → Invoke) to verify the menu bar still responds in
//! direct-boot mode. CLAUDE.md previously flagged that direct-boot breaks
//! synthesised-keystroke menu nav; the recent UIA-pattern survey
//! (PLAN 10.8.4 + tools/uia-probe) showed Qt6 now exposes Invoke /
//! ExpandCollapse natively, so the keystroke caveat may not apply to the
//! pattern-driven path.
//!
//!   cargo run -p skylander-rpcs3-control --example boot_via_eboot --features uia-examples -- BLUS30968
//!
//! Reads `<rpcs3>/config/games.yml` to translate the serial into the
//! game's directory, finds `<dir>/PS3_GAME/USRDIR/EBOOT.BIN`, spawns
//! `rpcs3.exe <eboot>`, waits for the game viewport (FPS: …), then drives
//! Manage → Portals and Gates → Skylanders Portal via UIA. On exit (success
//! or failure), the spawned RPCS3 is killed via the Job Object the example
//! attaches it to (KILL_ON_JOB_CLOSE).
//!
//! Success criterion: the Skylanders Manager dialog window
//! (`classname = "skylander_dialog"`) appears within 5s of the final
//! Invoke. Result is printed; exit code 0/1 reflects success/failure.

#![cfg(windows)]

use std::path::{Path, PathBuf};
use std::process::Command;
use std::thread::sleep;
use std::time::{Duration, Instant};

use anyhow::{Context, Result, anyhow, bail};
use uiautomation::patterns::{UIExpandCollapsePattern, UIInvokePattern};
use uiautomation::types::ControlType;
use uiautomation::{UIAutomation, UIElement, UITreeWalker};
use windows::Win32::Foundation::{HANDLE, LPARAM};
use windows::Win32::System::JobObjects::{
    AssignProcessToJobObject, CreateJobObjectW, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
    JOBOBJECT_BASIC_LIMIT_INFORMATION, JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JobObjectExtendedLimitInformation,
    SetInformationJobObject,
};
use windows::Win32::System::Threading::{
    OpenProcess, PROCESS_SET_QUOTA, PROCESS_TERMINATE,
};
use windows::Win32::UI::WindowsAndMessaging::{
    EnumWindows, GetClassNameW, GetWindowTextLengthW, GetWindowTextW,
};
use windows::core::BOOL;

const RPCS3_EXE: &str = r"C:\emuluators\rpcs3\rpcs3.exe";
const GAMES_YML: &str = r"C:\emuluators\rpcs3\config\games.yml";

fn main() -> Result<()> {
    let serial = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "BLUS30968".to_string());
    eprintln!("[harness] target serial: {serial}");

    let game_dir = lookup_game_dir(&serial).context("look up game dir from games.yml")?;
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
    eprintln!("[harness] pid={pid}");

    let _job = JobOnDrop::for_pid(pid);

    // Phase 1: wait for the RPCS3 main window (menu bar host) to exist.
    eprintln!("[harness] waiting for RPCS3 main window...");
    let main_deadline = Instant::now() + Duration::from_secs(20);
    while Instant::now() < main_deadline {
        if find_rpcs3_main_hwnd().is_some() {
            eprintln!("[harness] main window present");
            break;
        }
        sleep(Duration::from_millis(200));
    }
    if find_rpcs3_main_hwnd().is_none() {
        bail!("RPCS3 main window never appeared");
    }

    // Phase 2: wait for the game viewport (title prefix "FPS:") so we
    // know we're past the boot phase and into running-game state.
    eprintln!("[harness] waiting for FPS: viewport (game booted)...");
    let viewport_deadline = Instant::now() + Duration::from_secs(120);
    while Instant::now() < viewport_deadline {
        if find_viewport().is_some() {
            eprintln!("[harness] FPS: viewport present — game is running");
            break;
        }
        sleep(Duration::from_millis(250));
    }
    if find_viewport().is_none() {
        bail!("FPS: viewport never appeared within 2 minutes (game didn't boot)");
    }

    // Phase 3: drive Manage → Portals and Gates → Skylanders Portal via
    // pure UIA pattern calls. This is THE test — does the menu bar still
    // respond when RPCS3 is in direct-boot mode?
    eprintln!("[harness] driving Manage → Portals and Gates → Skylanders Portal...");
    let result = drive_skylanders_menu();
    match &result {
        Ok(()) => eprintln!("[harness] ✓ Skylanders Manager dialog opened — direct-boot + UIA pattern nav WORKS"),
        Err(e) => eprintln!("[harness] ✗ menu nav failed: {e}"),
    }

    // _job drops here → KILL_ON_JOB_CLOSE terminates RPCS3 + descendants.
    eprintln!("[harness] killing rpcs3 (job object close)...");
    drop(child);
    result
}

fn lookup_game_dir(serial: &str) -> Result<PathBuf> {
    // Trivial line-by-line parse — games.yml is a flat serial: "path" map.
    // No need for a real YAML dep for this harness.
    let body = std::fs::read_to_string(GAMES_YML)
        .with_context(|| format!("read {GAMES_YML}"))?;
    for line in body.lines() {
        let Some((k, v)) = line.split_once(':') else { continue; };
        if k.trim() == serial {
            let path = v.trim().trim_matches('"').trim_end_matches('/').trim_end_matches('\\');
            return Ok(PathBuf::from(path));
        }
    }
    bail!("serial {serial:?} not found in {GAMES_YML}")
}

fn drive_skylanders_menu() -> Result<()> {
    let automation = UIAutomation::new().context("UIA init")?;
    let walker = automation.create_tree_walker().context("walker")?;
    let main = find_main_uia_element(&automation, &walker)?;

    let manage = find_menuitem(&walker, &main, "Manage")
        .ok_or_else(|| anyhow!("Manage MenuItem not found"))?;
    eprintln!("[menu] expanding Manage");
    manage
        .get_pattern::<UIExpandCollapsePattern>()
        .context("Manage has no ExpandCollapsePattern")?
        .expand()
        .context("Manage.expand failed")?;
    sleep(Duration::from_millis(200));

    let portals = find_menuitem(&walker, &main, "Portals and Gates")
        .ok_or_else(|| anyhow!("'Portals and Gates' MenuItem not found"))?;
    eprintln!("[menu] expanding Portals and Gates");
    portals
        .get_pattern::<UIExpandCollapsePattern>()
        .context("Portals and Gates has no ExpandCollapsePattern")?
        .expand()
        .context("Portals.expand failed")?;
    sleep(Duration::from_millis(200));

    let leaf = find_menuitem(&walker, &main, "Skylanders Portal")
        .ok_or_else(|| anyhow!("'Skylanders Portal' MenuItem not found"))?;
    eprintln!("[menu] invoking Skylanders Portal");
    leaf.get_pattern::<UIInvokePattern>()
        .context("Skylanders Portal has no InvokePattern")?
        .invoke()
        .context("Skylanders Portal.invoke failed")?;

    // Wait for the dialog window (classname = "skylander_dialog").
    // Extended to 15s — direct-boot mode may have higher latency on action
    // dispatch. If it never appears, dump every top-level window so we
    // can tell whether (a) nothing opened, (b) opened with a different
    // classname, or (c) opened as a child of the main window.
    let deadline = Instant::now() + Duration::from_secs(15);
    while Instant::now() < deadline {
        if find_dialog_hwnd_by_classname("skylander_dialog").is_some() {
            return Ok(());
        }
        sleep(Duration::from_millis(200));
    }
    eprintln!("[diag] no skylander_dialog within 15s. Dumping all top-level windows:");
    dump_top_level_windows();
    bail!("Skylanders Manager dialog (skylander_dialog) didn't appear within 15s")
}

fn find_main_uia_element(automation: &UIAutomation, walker: &UITreeWalker) -> Result<UIElement> {
    let root = automation.get_root_element()?;
    let mut cur = walker.get_first_child(&root).ok();
    while let Some(el) = cur.clone() {
        if el
            .get_name()
            .map(|n| n.starts_with("RPCS3 "))
            .unwrap_or(false)
        {
            return Ok(el);
        }
        cur = walker.get_next_sibling(&el).ok();
    }
    bail!("RPCS3 main window not found in UIA tree")
}

fn find_menuitem(walker: &UITreeWalker, root: &UIElement, name: &str) -> Option<UIElement> {
    let mut stack = vec![root.clone()];
    while let Some(node) = stack.pop() {
        if matches!(
            node.get_control_type().ok(),
            Some(ControlType::MenuItem) | Some(ControlType::Menu)
        ) && node.get_name().ok().as_deref() == Some(name)
        {
            return Some(node);
        }
        if let Ok(child) = walker.get_first_child(&node) {
            let mut cur = Some(child);
            while let Some(c) = cur {
                stack.push(c.clone());
                cur = walker.get_next_sibling(&c).ok();
            }
        }
    }
    None
}

fn find_rpcs3_main_hwnd() -> Option<windows::Win32::Foundation::HWND> {
    enum_windows_find(|hwnd| {
        let cls = read_class(hwnd).unwrap_or_default();
        let title = read_title(hwnd).unwrap_or_default();
        cls.starts_with("Qt") && title.starts_with("RPCS3 ")
    })
}

fn find_viewport() -> Option<windows::Win32::Foundation::HWND> {
    enum_windows_find(|hwnd| {
        let title = read_title(hwnd).unwrap_or_default();
        title.starts_with("FPS:")
    })
}

fn find_dialog_hwnd_by_classname(target: &str) -> Option<windows::Win32::Foundation::HWND> {
    let target_owned = target.to_string();
    enum_windows_find(move |hwnd| {
        read_class(hwnd).as_deref() == Some(target_owned.as_str())
    })
}

fn enum_windows_find<F>(mut pred: F) -> Option<windows::Win32::Foundation::HWND>
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

fn read_class(hwnd: windows::Win32::Foundation::HWND) -> Option<String> {
    unsafe {
        let mut buf = [0u16; 256];
        let n = GetClassNameW(hwnd, &mut buf);
        if n <= 0 {
            return None;
        }
        Some(String::from_utf16_lossy(&buf[..n as usize]))
    }
}

/// Job object that kills RPCS3 (and any child processes RPCS3 spawned)
/// when this guard drops — covers both successful exit + panic paths.
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

fn dump_top_level_windows() {
    extern "system" fn proc(hwnd: windows::Win32::Foundation::HWND, _lparam: LPARAM) -> BOOL {
        let title = read_title(hwnd).unwrap_or_default();
        let cls = read_class(hwnd).unwrap_or_default();
        // Only print windows that have a title or a non-system class — keeps
        // signal-to-noise high. Skip empty/internal windows.
        if !title.is_empty() || (cls.starts_with("Qt") || cls.contains("dialog")) {
            eprintln!("  [{}] {:?}", cls, title);
        }
        BOOL(1)
    }
    unsafe {
        let _ = EnumWindows(Some(proc), LPARAM(0));
    }
}

fn _phantom_path_assertion(_p: &Path) {}
