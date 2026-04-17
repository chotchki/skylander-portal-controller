//! 3.6b — script the full menu navigation to open the Skylanders Manager
//! dialog and verify it appeared.
//!
//!   cargo run -p skylander-rpcs3-control --example open_skylanders_dialog
//!
//! Sequence: minimise viewport if present → AttachThreadInput +
//! SetForegroundWindow on main → Alt tap → Right×3 → Down → Down×3 → Right →
//! Enter. Between each keystroke we re-assert foreground (in case an
//! update-check popup or the viewport stole focus) and read the focused
//! MenuItem name via UIA so we can see if our counts are right.
//!
//! After Enter, poll UIA up to 3s for a Window named "Skylanders Manager".
//! Restores the viewport on exit.

#![cfg(windows)]

use std::thread::sleep;
use std::time::{Duration, Instant};

use anyhow::{Context, Result, anyhow, bail};
use uiautomation::types::ControlType;
use uiautomation::{UIAutomation, UIElement, UITreeWalker};

use windows::Win32::Foundation::RECT;
use windows::Win32::Foundation::{HWND, LPARAM};
use windows::Win32::System::Threading::{AttachThreadInput, GetCurrentThreadId};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    INPUT, INPUT_0, INPUT_KEYBOARD, KEYBD_EVENT_FLAGS, KEYBDINPUT, KEYEVENTF_KEYUP, SendInput,
    VIRTUAL_KEY, VK_DOWN, VK_LEFT, VK_MENU, VK_RETURN, VK_RIGHT,
};
use windows::Win32::UI::WindowsAndMessaging::{
    EnumWindows, GetClassNameW, GetForegroundWindow, GetWindowRect, GetWindowTextLengthW,
    GetWindowTextW, GetWindowThreadProcessId, SW_MINIMIZE, SW_RESTORE, SWP_NOSIZE, SWP_NOZORDER,
    SetForegroundWindow, SetWindowPos, ShowWindow,
};
use windows::core::BOOL;

const STEP_PAUSE: Duration = Duration::from_millis(200);

fn main() -> Result<()> {
    // Modes (combine any):
    //   --no-minimise      : leave viewport visible (default: minimise)
    //   --hide-main        : move main window off-screen before focusing
    //                        (restore original rect afterwards)
    let args: Vec<String> = std::env::args().skip(1).collect();
    let minimise_viewport = !args.iter().any(|a| a == "--no-minimise");
    let hide_main = args.iter().any(|a| a == "--hide-main");
    eprintln!("mode: minimise_viewport={minimise_viewport}, hide_main={hide_main}");

    let (main_hwnd, viewport_hwnd) = find_rpcs3_windows()?;
    eprintln!(
        "main HWND {:x}, viewport HWND {:?}",
        main_hwnd.0 as isize,
        viewport_hwnd.map(|h| format!("{:x}", h.0 as isize))
    );

    // Save main's rect so we can restore it if we move it off-screen.
    let mut main_rect = RECT::default();
    unsafe {
        let _ = GetWindowRect(main_hwnd, &mut main_rect);
    }

    if let Some(vp) = viewport_hwnd {
        if minimise_viewport {
            unsafe {
                let _ = ShowWindow(vp, SW_MINIMIZE);
            }
            eprintln!("viewport minimised");
            sleep(STEP_PAUSE);
        }
    }

    if hide_main {
        unsafe {
            let _ = SetWindowPos(
                main_hwnd,
                None,
                -4000,
                -4000,
                0,
                0,
                SWP_NOSIZE | SWP_NOZORDER,
            );
        }
        eprintln!("main window moved off-screen");
        sleep(STEP_PAUSE);
        log_rect("after move offscreen", main_hwnd);
    }

    let automation = UIAutomation::new().context("UIA init")?;
    let walker = automation.create_tree_walker().context("walker")?;
    let main_el = automation
        .element_from_handle(main_hwnd.into())
        .context("UIA element_from_handle")?;

    focus_main(main_hwnd).context("initial focus")?;
    log_rect("after focus_main", main_hwnd);
    report_focus(&walker, &main_el, "after initial focus")?;

    // Alt tap → File should highlight.
    focus_main(main_hwnd)?;
    send_key(VK_MENU)?;
    sleep(STEP_PAUSE);
    report_focus(&walker, &main_el, "Alt tap")?;

    // Right × 3 → Manage.
    for i in 1..=3 {
        focus_main(main_hwnd)?;
        send_key(VK_RIGHT)?;
        sleep(STEP_PAUSE);
        report_focus(&walker, &main_el, &format!("Right #{i}"))?;
    }

    // Down → opens Manage submenu (first item highlighted).
    focus_main(main_hwnd)?;
    send_key(VK_DOWN)?;
    sleep(STEP_PAUSE);
    report_focus(&walker, &main_el, "Down (open submenu)")?;

    // Down × 3 → Portals and Gates.
    for i in 1..=3 {
        focus_main(main_hwnd)?;
        send_key(VK_DOWN)?;
        sleep(STEP_PAUSE);
        report_focus(&walker, &main_el, &format!("Down #{i}"))?;
    }

    // Right → expand into Portals-and-Gates subsubmenu (Skylander Portal first).
    focus_main(main_hwnd)?;
    send_key(VK_RIGHT)?;
    sleep(STEP_PAUSE);
    report_focus(&walker, &main_el, "Right (open sub-submenu)")?;

    // Enter → activate.
    focus_main(main_hwnd)?;
    send_key(VK_RETURN)?;
    eprintln!("Enter sent — waiting for Skylanders Manager dialog");

    // Poll for the dialog.
    let deadline = Instant::now() + Duration::from_secs(5);
    let mut found = None;
    while Instant::now() < deadline {
        let root = automation.get_root_element()?;
        if let Some(hit) = find_descendant(&walker, &root, |el| {
            el.get_control_type()
                .map(|c| c == ControlType::Window)
                .unwrap_or(false)
                && el
                    .get_name()
                    .map(|n| n == "Skylanders Manager")
                    .unwrap_or(false)
        }) {
            found = Some(hit);
            break;
        }
        sleep(Duration::from_millis(100));
    }

    match &found {
        Some(_) => {
            eprintln!("✅ Skylanders Manager dialog appeared");
            // Immediately sling it off-screen so the user doesn't see it
            // linger. Uses the same find_dialog_hwnd + SetWindowPos primitive
            // the driver's hide_dialog_offscreen already relies on.
            if let Ok(dialog_hwnd) = skylander_rpcs3_control::hide::find_dialog_hwnd() {
                unsafe {
                    let _ = SetWindowPos(
                        dialog_hwnd,
                        None,
                        -4000,
                        -4000,
                        0,
                        0,
                        SWP_NOSIZE | SWP_NOZORDER,
                    );
                }
                eprintln!("  dialog moved off-screen immediately");
            }
        }
        None => {
            // Dismiss any open menu so we don't leave RPCS3 in a weird state.
            send_key(VK_LEFT)?;
            send_key(VK_LEFT)?;
            send_key(VK_MENU)?;
            eprintln!("❌ Skylanders Manager dialog NOT found within 5s");
        }
    }

    if hide_main {
        unsafe {
            let _ = SetWindowPos(
                main_hwnd,
                None,
                main_rect.left,
                main_rect.top,
                0,
                0,
                SWP_NOSIZE | SWP_NOZORDER,
            );
        }
        eprintln!("main window position restored");
    }

    if let Some(vp) = viewport_hwnd {
        if minimise_viewport {
            unsafe {
                let _ = ShowWindow(vp, SW_RESTORE);
            }
            eprintln!("viewport restored");
        }
    }

    Ok(())
}

fn log_rect(label: &str, hwnd: HWND) {
    let mut r = RECT::default();
    unsafe {
        let _ = GetWindowRect(hwnd, &mut r);
    }
    eprintln!(
        "  rect [{label}]: L={} T={} R={} B={}",
        r.left, r.top, r.right, r.bottom
    );
}

fn report_focus(walker: &UITreeWalker, main: &UIElement, label: &str) -> Result<()> {
    // Walk menu items, report anything with keyboard focus.
    let mut stack: Vec<UIElement> = vec![main.clone()];
    let mut focused: Vec<String> = Vec::new();
    while let Some(node) = stack.pop() {
        let ct = node.get_control_type().ok();
        if matches!(
            ct,
            Some(ControlType::MenuItem) | Some(ControlType::MenuBar) | Some(ControlType::Menu)
        ) && node.has_keyboard_focus().unwrap_or(false)
        {
            focused.push(format!(
                "[{:?}] {:?}",
                ct.unwrap(),
                node.get_name().unwrap_or_default()
            ));
        }
        if let Ok(child) = walker.get_first_child(&node) {
            let mut cur = Some(child);
            while let Some(c) = cur {
                stack.push(c.clone());
                cur = walker.get_next_sibling(&c).ok();
            }
        }
    }

    // Also peek at the desktop root for a highlighted menu item that may have
    // detached as an owned popup.
    if focused.is_empty() {
        let automation = UIAutomation::new()?;
        let root = automation.get_root_element()?;
        let mut stack: Vec<UIElement> = vec![root];
        while let Some(node) = stack.pop() {
            let ct = node.get_control_type().ok();
            if matches!(ct, Some(ControlType::MenuItem))
                && node.has_keyboard_focus().unwrap_or(false)
            {
                focused.push(format!(
                    "[desktop {:?}] {:?}",
                    ct.unwrap(),
                    node.get_name().unwrap_or_default()
                ));
            }
            if let Ok(child) = walker.get_first_child(&node) {
                let mut cur = Some(child);
                while let Some(c) = cur {
                    stack.push(c.clone());
                    cur = walker.get_next_sibling(&c).ok();
                }
            }
        }
    }

    if focused.is_empty() {
        eprintln!("{label}: (no focused menu item found)");
    } else {
        eprintln!("{label}: {}", focused.join(", "));
    }
    Ok(())
}

fn focus_main(hwnd: HWND) -> Result<()> {
    let our_thread = unsafe { GetCurrentThreadId() };
    let fg = unsafe { GetForegroundWindow() };
    if fg.0 == hwnd.0 {
        return Ok(());
    }
    let mut fg_tid = 0u32;
    unsafe {
        let _ = GetWindowThreadProcessId(fg, Some(&mut fg_tid));
    }
    let mut target_pid = 0u32;
    let target_thread = unsafe { GetWindowThreadProcessId(hwnd, Some(&mut target_pid)) };
    unsafe {
        if fg_tid != 0 && fg_tid != our_thread {
            let _ = AttachThreadInput(our_thread, fg_tid, true);
        }
        if target_thread != 0 && target_thread != our_thread {
            let _ = AttachThreadInput(our_thread, target_thread, true);
        }
        let ok = SetForegroundWindow(hwnd).as_bool();
        if fg_tid != 0 && fg_tid != our_thread {
            let _ = AttachThreadInput(our_thread, fg_tid, false);
        }
        if target_thread != 0 && target_thread != our_thread {
            let _ = AttachThreadInput(our_thread, target_thread, false);
        }
        if !ok {
            bail!("SetForegroundWindow returned false");
        }
    }
    Ok(())
}

fn send_key(vk: VIRTUAL_KEY) -> Result<()> {
    let inputs = [key_input(vk, false), key_input(vk, true)];
    unsafe {
        let n = SendInput(&inputs, std::mem::size_of::<INPUT>() as i32);
        if n as usize != inputs.len() {
            return Err(anyhow!("SendInput only sent {n}/{}", inputs.len()));
        }
    }
    Ok(())
}

fn key_input(vk: VIRTUAL_KEY, key_up: bool) -> INPUT {
    INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT {
                wVk: vk,
                wScan: 0,
                dwFlags: if key_up {
                    KEYEVENTF_KEYUP
                } else {
                    KEYBD_EVENT_FLAGS(0)
                },
                time: 0,
                dwExtraInfo: 0,
            },
        },
    }
}

fn find_descendant<F>(walker: &UITreeWalker, root: &UIElement, mut pred: F) -> Option<UIElement>
where
    F: FnMut(&UIElement) -> bool,
{
    let mut stack: Vec<UIElement> = vec![root.clone()];
    while let Some(node) = stack.pop() {
        if pred(&node) {
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

fn find_rpcs3_windows() -> Result<(HWND, Option<HWND>)> {
    let pids = rpcs3_pids();
    if pids.is_empty() {
        bail!("no rpcs3.exe process");
    }

    struct Ctx {
        pids: Vec<u32>,
        main: Option<HWND>,
        viewport: Option<HWND>,
    }
    extern "system" fn proc(hwnd: HWND, lparam: LPARAM) -> BOOL {
        let ctx = unsafe { &mut *(lparam.0 as *mut Ctx) };
        let mut pid = 0u32;
        unsafe {
            GetWindowThreadProcessId(hwnd, Some(&mut pid));
        }
        if !ctx.pids.contains(&pid) {
            return BOOL(1);
        }
        let cls = window_class(hwnd).unwrap_or_default();
        let title = window_title(hwnd).unwrap_or_default();
        if cls == "Qt6110QWindowIcon" {
            if title.starts_with("RPCS3 ") && ctx.main.is_none() {
                ctx.main = Some(hwnd);
            } else if title.starts_with("FPS:") && ctx.viewport.is_none() {
                ctx.viewport = Some(hwnd);
            }
        }
        BOOL(1)
    }
    let mut ctx = Ctx {
        pids,
        main: None,
        viewport: None,
    };
    unsafe {
        let lp = LPARAM(&mut ctx as *mut _ as isize);
        let _ = EnumWindows(Some(proc), lp);
    }
    Ok((
        ctx.main.ok_or_else(|| anyhow!("main window not found"))?,
        ctx.viewport,
    ))
}

fn rpcs3_pids() -> Vec<u32> {
    use windows::Win32::System::ProcessStatus::{
        EnumProcesses, GetModuleBaseNameW, K32EnumProcessModules,
    };
    use windows::Win32::System::Threading::{
        OpenProcess, PROCESS_QUERY_INFORMATION, PROCESS_VM_READ,
    };

    let mut pids = vec![0u32; 1024];
    let mut bytes = 0u32;
    unsafe {
        if EnumProcesses(
            pids.as_mut_ptr(),
            (pids.len() * std::mem::size_of::<u32>()) as u32,
            &mut bytes,
        )
        .is_err()
        {
            return Vec::new();
        }
    }
    pids.truncate(bytes as usize / std::mem::size_of::<u32>());
    pids.into_iter()
        .filter(|&pid| pid != 0)
        .filter(|&pid| unsafe {
            let handle = match OpenProcess(PROCESS_QUERY_INFORMATION | PROCESS_VM_READ, false, pid)
            {
                Ok(h) => h,
                Err(_) => return false,
            };
            let mut module = windows::Win32::Foundation::HMODULE::default();
            let mut needed = 0u32;
            let ok = K32EnumProcessModules(
                handle,
                &mut module,
                std::mem::size_of::<windows::Win32::Foundation::HMODULE>() as u32,
                &mut needed,
            )
            .as_bool();
            let name = if ok {
                let mut buf = [0u16; 260];
                let n = GetModuleBaseNameW(handle, Some(module), &mut buf);
                if n > 0 {
                    String::from_utf16_lossy(&buf[..n as usize])
                } else {
                    String::new()
                }
            } else {
                String::new()
            };
            let _ = windows::Win32::Foundation::CloseHandle(handle);
            name.eq_ignore_ascii_case("rpcs3.exe")
        })
        .collect()
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
