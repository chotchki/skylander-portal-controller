//! 3.6b.1 — enumerate every top-level window owned by an rpcs3.exe process,
//! print title / classname / HWND / visible / rect / pid.
//!
//! Run while RPCS3 is up (game booted or not — try both):
//!   cargo run -p skylander-rpcs3-control --example rpcs3_windows
//!
//! Goal: identify a stable distinguisher (almost certainly classname) between
//! the menu-bar main window and the game-viewport window, so `main_window()`
//! in src/uia.rs can stop relying on the title prefix.

#![cfg(windows)]

use windows::Win32::Foundation::{HWND, LPARAM, RECT};
use windows::core::BOOL;
use windows::Win32::System::ProcessStatus::{
    EnumProcesses, GetModuleBaseNameW, K32EnumProcessModules,
};
use windows::Win32::System::Threading::{
    OpenProcess, PROCESS_QUERY_INFORMATION, PROCESS_VM_READ,
};
use windows::Win32::UI::WindowsAndMessaging::{
    EnumWindows, GetClassNameW, GetWindow, GetWindowRect, GetWindowTextLengthW,
    GetWindowTextW, GetWindowThreadProcessId, IsWindowVisible, GW_OWNER,
};

fn main() -> anyhow::Result<()> {
    let rpcs3_pids = find_rpcs3_pids();
    if rpcs3_pids.is_empty() {
        eprintln!("no rpcs3.exe processes found — start RPCS3 first");
        std::process::exit(1);
    }
    eprintln!("rpcs3.exe PIDs: {rpcs3_pids:?}");

    let windows = enumerate_windows(&rpcs3_pids);
    println!(
        "{:>8} {:>5} {:>1} {:>5} {:>5} {:>5} {:>5}  {:<28}  {}",
        "HWND", "PID", "V", "x", "y", "w", "h", "classname", "title"
    );
    for w in &windows {
        println!(
            "{:>8x} {:>5} {:>1} {:>5} {:>5} {:>5} {:>5}  {:<28.28}  {}",
            w.hwnd_raw,
            w.pid,
            if w.visible { "Y" } else { "N" },
            w.rect.left,
            w.rect.top,
            w.rect.right - w.rect.left,
            w.rect.bottom - w.rect.top,
            w.classname,
            w.title,
        );
    }
    eprintln!("\n{} windows total", windows.len());
    Ok(())
}

#[derive(Debug, Clone)]
struct WinInfo {
    hwnd_raw: isize,
    pid: u32,
    visible: bool,
    title: String,
    classname: String,
    rect: RECT,
}

fn enumerate_windows(rpcs3_pids: &[u32]) -> Vec<WinInfo> {
    struct Ctx {
        wanted_pids: Vec<u32>,
        found: Vec<WinInfo>,
    }
    extern "system" fn proc(hwnd: HWND, lparam: LPARAM) -> BOOL {
        let ctx = unsafe { &mut *(lparam.0 as *mut Ctx) };
        let mut pid = 0u32;
        unsafe {
            GetWindowThreadProcessId(hwnd, Some(&mut pid));
        }
        if !ctx.wanted_pids.contains(&pid) {
            return BOOL(1);
        }
        // Skip windows owned by another window — these are typically
        // ephemeral child popups (tooltips etc.) and just add noise.
        let owner = unsafe { GetWindow(hwnd, GW_OWNER) }.unwrap_or_default();
        let _ = owner; // keep for debugging if needed; not filtering on it
        let title = window_title(hwnd).unwrap_or_default();
        let classname = window_class(hwnd).unwrap_or_default();
        let mut rect = RECT::default();
        unsafe {
            let _ = GetWindowRect(hwnd, &mut rect);
        }
        let visible = unsafe { IsWindowVisible(hwnd) }.as_bool();
        ctx.found.push(WinInfo {
            hwnd_raw: hwnd.0 as isize,
            pid,
            visible,
            title,
            classname,
            rect,
        });
        BOOL(1)
    }
    let mut ctx = Ctx {
        wanted_pids: rpcs3_pids.to_vec(),
        found: Vec::new(),
    };
    unsafe {
        let lparam = LPARAM(&mut ctx as *mut _ as isize);
        let _ = EnumWindows(Some(proc), lparam);
    }
    ctx.found
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

fn find_rpcs3_pids() -> Vec<u32> {
    let mut pids = vec![0u32; 1024];
    let mut bytes_returned = 0u32;
    unsafe {
        if EnumProcesses(
            pids.as_mut_ptr(),
            (pids.len() * std::mem::size_of::<u32>()) as u32,
            &mut bytes_returned,
        )
        .is_err()
        {
            return Vec::new();
        }
    }
    let count = bytes_returned as usize / std::mem::size_of::<u32>();
    pids.truncate(count);

    pids.into_iter()
        .filter(|&pid| pid != 0)
        .filter(|&pid| {
            process_name(pid)
                .map(|n| n.eq_ignore_ascii_case("rpcs3.exe"))
                .unwrap_or(false)
        })
        .collect()
}

fn process_name(pid: u32) -> Option<String> {
    unsafe {
        let handle =
            OpenProcess(PROCESS_QUERY_INFORMATION | PROCESS_VM_READ, false, pid).ok()?;
        let mut module = windows::Win32::Foundation::HMODULE::default();
        let mut needed = 0u32;
        if K32EnumProcessModules(
            handle,
            &mut module,
            std::mem::size_of::<windows::Win32::Foundation::HMODULE>() as u32,
            &mut needed,
        )
        .as_bool()
        {
            let mut buf = [0u16; 260];
            let n = GetModuleBaseNameW(handle, Some(module), &mut buf);
            if n > 0 {
                let _ = windows::Win32::Foundation::CloseHandle(handle);
                return Some(String::from_utf16_lossy(&buf[..n as usize]));
            }
        }
        let _ = windows::Win32::Foundation::CloseHandle(handle);
        None
    }
}
