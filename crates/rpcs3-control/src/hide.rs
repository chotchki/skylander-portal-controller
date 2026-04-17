//! Win32 off-screen move for the Qt portal dialog. UIA's `TransformPattern`
//! reports success but doesn't actually move the window (see Phase 1a
//! research), and UIA's tree walker prunes off-screen child windows — so
//! once the dialog has been hidden we can't re-find it through UIA. All
//! positioning goes through SetWindowPos on a raw HWND; `find_dialog_hwnd`
//! uses FindWindowEx so it works whether the dialog is on- or off-screen.

use anyhow::{Context, Result, anyhow};
use uiautomation::UIElement;
use windows::Win32::Foundation::{HWND, LPARAM};
use windows::Win32::UI::WindowsAndMessaging::{
    SET_WINDOW_POS_FLAGS, SWP_NOSIZE, SWP_NOZORDER, SetWindowPos,
};
use windows::core::BOOL;

pub fn set_position(el: &UIElement, x: i32, y: i32) -> Result<()> {
    let raw = el
        .get_native_window_handle()
        .context("UIA element exposes no native HWND")?;
    let hwnd_raw: isize = raw.into();
    if hwnd_raw == 0 {
        return Err(anyhow!("native HWND is null"));
    }
    let hwnd = HWND(hwnd_raw as _);
    set_position_raw(hwnd, x, y)
}

pub fn set_position_raw(hwnd: HWND, x: i32, y: i32) -> Result<()> {
    let flags: SET_WINDOW_POS_FLAGS = SWP_NOSIZE | SWP_NOZORDER;
    unsafe {
        SetWindowPos(hwnd, None, x, y, 0, 0, flags).context("SetWindowPos failed")?;
    }
    Ok(())
}

/// Move the dialog on-screen AND make sure it's shown + on top. Used for
/// `restore_dialog_visible` so a hidden-then-offscreen dialog reappears
/// for the user.
pub fn set_position_and_show(hwnd: HWND, x: i32, y: i32) -> Result<()> {
    use windows::Win32::Graphics::Gdi::{
        InvalidateRect, RDW_ERASE, RDW_INVALIDATE, RDW_UPDATENOW, RedrawWindow,
    };
    use windows::Win32::UI::WindowsAndMessaging::{
        BringWindowToTop, GetWindowRect, IsWindow, IsWindowVisible, SW_HIDE, SW_SHOW,
        SetForegroundWindow, ShowWindow,
    };
    unsafe {
        if !IsWindow(Some(hwnd)).as_bool() {
            return Err(anyhow!("HWND is no longer a valid window"));
        }
        // SW_HIDE/SW_SHOW cycle to nudge Qt into a repaint.
        let _ = ShowWindow(hwnd, SW_HIDE);
        SetWindowPos(hwnd, None, x, y, 0, 0, SWP_NOSIZE | SWP_NOZORDER)
            .context("SetWindowPos failed")?;
        let _ = ShowWindow(hwnd, SW_SHOW);
        let _ = BringWindowToTop(hwnd);
        let _ = SetForegroundWindow(hwnd);
        let _ = InvalidateRect(Some(hwnd), None, true);
        let _ = RedrawWindow(
            Some(hwnd),
            None,
            None,
            RDW_INVALIDATE | RDW_ERASE | RDW_UPDATENOW,
        );
        // Debug sanity — tiny log line in case future versions regress.
        let mut rect = windows::Win32::Foundation::RECT::default();
        if GetWindowRect(hwnd, &mut rect).is_ok() {
            tracing::debug!(
                class = ?window_class(hwnd).unwrap_or_default(),
                visible = IsWindowVisible(hwnd).as_bool(),
                width = rect.right - rect.left,
                height = rect.bottom - rect.top,
                x = rect.left,
                y = rect.top,
                "dialog restored",
            );
        }
    }
    Ok(())
}

/// Find the Skylanders Manager dialog by enumerating top-level windows for
/// one titled "RPCS3 …", then enumerating its children for one titled
/// "Skylanders Manager". Works whether the dialog is on-screen or hidden
/// at `(-4000, -4000)` — `EnumWindows` / `EnumChildWindows` return hidden
/// windows, unlike UIA's default tree walker which prunes them.
pub fn find_dialog_hwnd() -> Result<HWND> {
    // Qt's QDialog::show() creates a Win32 top-level window regardless of
    // the QWidget parent/child relationship UIA shows, so scan top-level
    // windows directly.
    if let Some(hwnd) = find_top_level_by_exact_title("Skylanders Manager") {
        return Ok(hwnd);
    }
    // Fall back to the child walk in case a future RPCS3 reparents the dialog.
    let main = find_top_level_by_title_prefix("RPCS3 ")
        .ok_or_else(|| anyhow!("RPCS3 main window not found"))?;
    find_child_by_exact_title(main, "Skylanders Manager")
        .ok_or_else(|| anyhow!("Skylanders Manager dialog not found"))
}

fn find_top_level_by_exact_title(title: &str) -> Option<HWND> {
    use windows::Win32::UI::WindowsAndMessaging::EnumWindows;

    struct Ctx {
        title: String,
        candidates: Vec<HWND>,
    }
    extern "system" fn proc(hwnd: HWND, lparam: LPARAM) -> BOOL {
        let ctx = unsafe { &mut *(lparam.0 as *mut Ctx) };
        if let Some(t) = window_title(hwnd) {
            if t == ctx.title {
                ctx.candidates.push(hwnd);
            }
        }
        BOOL(1)
    }
    let mut ctx = Ctx {
        title: title.to_string(),
        candidates: Vec::new(),
    };
    unsafe {
        let lparam = LPARAM(&mut ctx as *mut _ as isize);
        let _ = EnumWindows(Some(proc), lparam);
    }

    // Qt creates multiple HWNDs per QDialog: the user-visible one has a
    // class containing "QWindowIcon"; helpers like "QWindowToolSaveBits"
    // are intentionally invisible. Prefer QWindowIcon if we see both.
    if ctx.candidates.is_empty() {
        return None;
    }
    for hwnd in &ctx.candidates {
        if let Some(cls) = window_class(*hwnd) {
            if cls.contains("QWindowIcon") || cls.contains("QWindow") && !cls.contains("ToolSave") {
                return Some(*hwnd);
            }
        }
    }
    ctx.candidates.first().copied()
}

fn window_class(hwnd: HWND) -> Option<String> {
    use windows::Win32::UI::WindowsAndMessaging::GetClassNameW;
    unsafe {
        let mut buf = [0u16; 256];
        let n = GetClassNameW(hwnd, &mut buf);
        if n <= 0 {
            return None;
        }
        Some(String::from_utf16_lossy(&buf[..n as usize]))
    }
}

fn find_top_level_by_title_prefix(prefix: &str) -> Option<HWND> {
    use windows::Win32::UI::WindowsAndMessaging::EnumWindows;

    struct Ctx {
        prefix: String,
        hit: Option<HWND>,
    }
    extern "system" fn proc(hwnd: HWND, lparam: LPARAM) -> BOOL {
        let ctx = unsafe { &mut *(lparam.0 as *mut Ctx) };
        if let Some(title) = window_title(hwnd) {
            if title.starts_with(&ctx.prefix) {
                ctx.hit = Some(hwnd);
                return BOOL(0); // stop
            }
        }
        BOOL(1) // continue
    }
    let mut ctx = Ctx {
        prefix: prefix.to_string(),
        hit: None,
    };
    unsafe {
        let lparam = LPARAM(&mut ctx as *mut _ as isize);
        let _ = EnumWindows(Some(proc), lparam);
    }
    ctx.hit
}

fn find_child_by_exact_title(parent: HWND, title: &str) -> Option<HWND> {
    use windows::Win32::UI::WindowsAndMessaging::EnumChildWindows;

    struct Ctx {
        title: String,
        hit: Option<HWND>,
    }
    extern "system" fn proc(hwnd: HWND, lparam: LPARAM) -> BOOL {
        let ctx = unsafe { &mut *(lparam.0 as *mut Ctx) };
        if let Some(t) = window_title(hwnd) {
            if t == ctx.title {
                ctx.hit = Some(hwnd);
                return BOOL(0);
            }
        }
        BOOL(1)
    }
    let mut ctx = Ctx {
        title: title.to_string(),
        hit: None,
    };
    unsafe {
        let lparam = LPARAM(&mut ctx as *mut _ as isize);
        let _ = EnumChildWindows(Some(parent), Some(proc), lparam);
    }
    ctx.hit
}

fn window_title(hwnd: HWND) -> Option<String> {
    use windows::Win32::UI::WindowsAndMessaging::{GetWindowTextLengthW, GetWindowTextW};
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
