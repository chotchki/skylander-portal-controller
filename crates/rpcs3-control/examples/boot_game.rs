//! 3.6b — probe: find a game in the RPCS3 library by serial and boot it via
//! UIA, to validate the launch-then-boot strategy before wiring it into
//! `RpcsProcess::launch`.
//!
//!   cargo run -p skylander-rpcs3-control --example boot_game -- BLUS30968
//!
//! Prereq: RPCS3 is already running, showing the library (no game booted).
//! Tries multiple strategies in order and reports which one worked:
//!   1. `UIInvokePattern::invoke()` on the serial cell.
//!   2. Select via `UISelectionItemPattern::select()` + `Enter` keystroke.
//!   3. Synthesised mouse double-click at the cell's centre (last resort).
//!
//! Success criterion: a viewport window with `"FPS:"` in its title appears
//! within 15s (same signal `find_viewport_hwnd` uses in production code).

#![cfg(windows)]

use std::thread::sleep;
use std::time::{Duration, Instant};

use anyhow::{Context, Result, anyhow, bail};
use uiautomation::patterns::{UIInvokePattern, UISelectionItemPattern};
use uiautomation::types::ControlType;
use uiautomation::{UIAutomation, UIElement, UITreeWalker};

use windows::Win32::Foundation::{HWND, LPARAM};
use windows::Win32::System::Threading::{AttachThreadInput, GetCurrentThreadId};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    INPUT, INPUT_0, INPUT_KEYBOARD, INPUT_MOUSE, KEYBD_EVENT_FLAGS, KEYBDINPUT, KEYEVENTF_KEYUP,
    MOUSEEVENTF_ABSOLUTE, MOUSEEVENTF_LEFTDOWN, MOUSEEVENTF_LEFTUP, MOUSEEVENTF_MOVE, MOUSEINPUT,
    SendInput, VIRTUAL_KEY, VK_RETURN,
};
use windows::Win32::UI::WindowsAndMessaging::{
    EnumWindows, GetClassNameW, GetForegroundWindow, GetSystemMetrics, GetWindowTextLengthW,
    GetWindowTextW, GetWindowThreadProcessId, SM_CXSCREEN, SM_CYSCREEN, SetForegroundWindow,
};
use windows::core::BOOL;

fn main() -> Result<()> {
    let serial = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "BLUS30968".to_string());
    eprintln!("booting {serial}");

    let automation = UIAutomation::new().context("UIA init")?;
    let walker = automation.create_tree_walker().context("walker")?;
    let main = find_main_window(&automation, &walker)?;
    let main_hwnd_raw: isize = main.get_native_window_handle().context("main HWND")?.into();
    let main_hwnd = HWND(main_hwnd_raw as _);

    let cell = find_cell_by_name(&walker, &main, &serial)
        .ok_or_else(|| anyhow!("no DataItem named {serial} under main"))?;
    eprintln!("found cell: {:?}", cell.get_name().ok());

    // Bring main forward so any mouse fallback works.
    focus_main(main_hwnd).ok();
    sleep(Duration::from_millis(200));

    // Strategy 1: UIA Invoke.
    match cell.get_pattern::<UIInvokePattern>() {
        Ok(inv) => {
            if let Err(e) = inv.invoke() {
                eprintln!("Invoke err: {e}");
            } else {
                eprintln!("Invoke dispatched");
                if wait_viewport(Duration::from_secs(10)) {
                    eprintln!("✅ Strategy 1 (Invoke) booted the game");
                    return Ok(());
                }
                eprintln!("Invoke didn't boot within 10s — trying strategy 2");
            }
        }
        Err(e) => eprintln!("no InvokePattern on cell ({e}) — skipping strategy 1"),
    }

    // Strategy 2a: Set keyboard focus on cell + Select + Enter.
    if let Ok(sel) = cell.get_pattern::<UISelectionItemPattern>() {
        if let Err(e) = sel.select() {
            eprintln!("select err: {e}");
        } else {
            eprintln!("selected cell; setting focus");
            if let Err(e) = cell.set_focus() {
                eprintln!("set_focus err: {e} (proceeding anyway)");
            } else {
                eprintln!("cell has keyboard focus");
            }
            sleep(Duration::from_millis(150));
            send_key(VK_RETURN)?;
            if wait_viewport(Duration::from_secs(10)) {
                eprintln!("✅ Strategy 2a (Select + focus + Enter) booted the game");
                return Ok(());
            }
            // 2b: same, but try Space in case Qt binds Space to activate.
            eprintln!("Enter didn't boot — trying Space");
            sel.select().ok();
            cell.set_focus().ok();
            sleep(Duration::from_millis(150));
            send_key(windows::Win32::UI::Input::KeyboardAndMouse::VK_SPACE)?;
            if wait_viewport(Duration::from_secs(10)) {
                eprintln!("✅ Strategy 2b (Select + focus + Space) booted the game");
                return Ok(());
            }
            eprintln!("Space didn't boot either — trying strategy 3");
        }
    } else {
        eprintln!("no SelectionItemPattern on cell — skipping strategy 2");
    }

    // Strategy 3: Mouse double-click at cell centre.
    let rect = cell.get_bounding_rectangle().context("cell rect")?;
    let cx = rect.get_left() + (rect.get_right() - rect.get_left()) / 2;
    let cy = rect.get_top() + (rect.get_bottom() - rect.get_top()) / 2;
    eprintln!("double-clicking at ({cx},{cy})");
    mouse_double_click(cx, cy)?;
    if wait_viewport(Duration::from_secs(15)) {
        eprintln!("✅ Strategy 3 (mouse double-click) booted the game");
        return Ok(());
    }
    bail!("none of the strategies booted the game");
}

fn find_cell_by_name(walker: &UITreeWalker, root: &UIElement, name: &str) -> Option<UIElement> {
    let mut stack: Vec<UIElement> = vec![root.clone()];
    while let Some(node) = stack.pop() {
        if node
            .get_control_type()
            .map(|c| c == ControlType::DataItem)
            .unwrap_or(false)
            && node.get_name().map(|n| n == name).unwrap_or(false)
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

fn wait_viewport(timeout: Duration) -> bool {
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        if find_viewport().is_some() {
            return true;
        }
        sleep(Duration::from_millis(100));
    }
    false
}

fn find_viewport() -> Option<HWND> {
    struct Ctx {
        hit: Option<HWND>,
    }
    extern "system" fn proc(hwnd: HWND, lparam: LPARAM) -> BOOL {
        let ctx = unsafe { &mut *(lparam.0 as *mut Ctx) };
        let cls = read_class(hwnd).unwrap_or_default();
        let title = read_title(hwnd).unwrap_or_default();
        if cls == "Qt6110QWindowIcon" && title.starts_with("FPS:") {
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
            bail!("SendInput only sent {n}");
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

fn mouse_double_click(x: i32, y: i32) -> Result<()> {
    let (sw, sh) = unsafe {
        (
            GetSystemMetrics(SM_CXSCREEN).max(1),
            GetSystemMetrics(SM_CYSCREEN).max(1),
        )
    };
    let ax = (x as i64 * 65535 / sw as i64) as i32;
    let ay = (y as i64 * 65535 / sh as i64) as i32;
    let move_flags = MOUSEEVENTF_MOVE | MOUSEEVENTF_ABSOLUTE;
    let inputs = [
        mouse_input(ax, ay, move_flags),
        mouse_input(0, 0, MOUSEEVENTF_LEFTDOWN),
        mouse_input(0, 0, MOUSEEVENTF_LEFTUP),
        mouse_input(0, 0, MOUSEEVENTF_LEFTDOWN),
        mouse_input(0, 0, MOUSEEVENTF_LEFTUP),
    ];
    unsafe {
        let n = SendInput(&inputs, std::mem::size_of::<INPUT>() as i32);
        if n as usize != inputs.len() {
            bail!("SendInput only sent {n}/{}", inputs.len());
        }
    }
    Ok(())
}

fn mouse_input(
    dx: i32,
    dy: i32,
    flags: windows::Win32::UI::Input::KeyboardAndMouse::MOUSE_EVENT_FLAGS,
) -> INPUT {
    INPUT {
        r#type: INPUT_MOUSE,
        Anonymous: INPUT_0 {
            mi: MOUSEINPUT {
                dx,
                dy,
                mouseData: 0,
                dwFlags: flags,
                time: 0,
                dwExtraInfo: 0,
            },
        },
    }
}

fn find_main_window(automation: &UIAutomation, walker: &UITreeWalker) -> Result<UIElement> {
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
    Err(anyhow!("RPCS3 main window not found"))
}
