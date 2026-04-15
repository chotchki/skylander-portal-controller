//! Force the RPCS3 main window visible. Useful when RPCS3 was backgrounded
//! at launch and sat with an invisible main window.
#![cfg(windows)]

use anyhow::{anyhow, Result};
use windows::Win32::Foundation::{HWND, LPARAM};
use windows::Win32::UI::WindowsAndMessaging::{
    EnumWindows, GetClassNameW, GetWindowTextLengthW, GetWindowTextW, ShowWindow, SW_SHOWNORMAL,
};
use windows::core::BOOL;

fn main() -> Result<()> {
    let hwnd = find_main().ok_or_else(|| anyhow!("no RPCS3 main window"))?;
    unsafe {
        let _ = ShowWindow(hwnd, SW_SHOWNORMAL);
    }
    eprintln!("forced main {:x} to SW_SHOWNORMAL", hwnd.0 as isize);
    Ok(())
}

fn find_main() -> Option<HWND> {
    struct Ctx {
        hit: Option<HWND>,
    }
    extern "system" fn proc(hwnd: HWND, lparam: LPARAM) -> BOOL {
        let ctx = unsafe { &mut *(lparam.0 as *mut Ctx) };
        let cls = read_class(hwnd).unwrap_or_default();
        let title = read_title(hwnd).unwrap_or_default();
        if cls == "Qt6110QWindowIcon" && title.starts_with("RPCS3 ") {
            ctx.hit = Some(hwnd);
            return BOOL(0);
        }
        BOOL(1)
    }
    let mut ctx = Ctx { hit: None };
    unsafe {
        let lp = LPARAM(&mut ctx as *mut _ as isize);
        let _ = EnumWindows(Some(proc), lp);
    }
    ctx.hit
}

fn read_title(hwnd: HWND) -> Option<String> {
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

fn read_class(hwnd: HWND) -> Option<String> {
    unsafe {
        let mut buf = [0u16; 256];
        let n = GetClassNameW(hwnd, &mut buf);
        if n <= 0 {
            return None;
        }
        Some(String::from_utf16_lossy(&buf[..n as usize]))
    }
}
