//! PLAN 4.15.15 research probe — validate how to keep an egui-like topmost
//! window covering RPCS3 while still being able to drive UIA-mouse clicks
//! INTO RPCS3 (so the egui launcher can stay on top without breaking
//! `boot_game_by_serial`).
//!
//! Three options tested — pick one via the `--variant` positional arg:
//!
//!   a  Cover window has `WS_EX_TRANSPARENT` → click-through. `SendInput`
//!      at screen coords is expected to pass through to RPCS3 normally.
//!      Downside: the cover window itself can't receive mouse — fine if the
//!      launcher is phone-driven with no on-screen interactivity.
//!   b  Cover is opaque topmost. We briefly `SetWindowPos(HWND_BOTTOM)` the
//!      cover during the click, then restore topmost. Creates a visible
//!      ~100–300ms flash of the game behind it.
//!   c  Cover stays opaque topmost. We `PostMessage(WM_LBUTTONDOWN/UP)` +
//!      `PostMessage(WM_KEYDOWN/UP)` directly to RPCS3's main HWND with
//!      window-relative coords. Bypasses Z-order entirely; Qt handles the
//!      posted messages regardless of whether the window is covered.
//!
//! Success criterion = an RPCS3 viewport window (class `Qt6110QWindowIcon`,
//! title prefix `FPS:`) appears within 30s of the boot attempt.
//!
//! ```text
//! cargo run -p skylander-rpcs3-control --example zorder_probe -- a
//! cargo run -p skylander-rpcs3-control --example zorder_probe -- b
//! cargo run -p skylander-rpcs3-control --example zorder_probe -- c
//! ```
//!
//! Env vars (same as `live_lifecycle.rs`):
//!   RPCS3_EXE=C:\emuluators\rpcs3\rpcs3.exe
//!   RPCS3_TEST_SERIAL=BLUS31076
//!
//! Deliverable: pick the winner, recommend into PLAN 4.15.8 + 4.15.9.

#![cfg(windows)]

use std::thread::sleep;
use std::time::{Duration, Instant};

use anyhow::{Context, Result, anyhow, bail};
use skylander_rpcs3_control::{PortalDriver, RpcsProcess, UiaPortalDriver};
use uiautomation::types::ControlType;
use uiautomation::{UIAutomation, UIElement, UITreeWalker};
use windows::Win32::Foundation::{COLORREF, HWND, LPARAM, LRESULT, RECT, WPARAM};
use windows::Win32::Graphics::Gdi::CreateSolidBrush;
use windows::Win32::UI::Input::KeyboardAndMouse::VK_RETURN;
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, EnumWindows, GetClassNameW, GetSystemMetrics,
    GetWindowRect, GetWindowTextLengthW, GetWindowTextW, HWND_BOTTOM, HWND_TOPMOST, LWA_ALPHA,
    PostMessageW, RegisterClassW, SM_CXSCREEN, SM_CYSCREEN, SW_SHOW, SWP_NOMOVE, SWP_NOSIZE,
    SetLayeredWindowAttributes, SetWindowPos, ShowWindow, WM_KEYDOWN, WM_KEYUP, WM_LBUTTONDOWN,
    WM_LBUTTONUP, WNDCLASSW, WS_EX_LAYERED, WS_EX_TOPMOST, WS_EX_TRANSPARENT, WS_POPUP, WS_VISIBLE,
};
use windows::core::{BOOL, PCWSTR};

fn main() -> Result<()> {
    let variant = std::env::args()
        .nth(1)
        .context("usage: zorder_probe <a|b|c>")?;
    if !matches!(variant.as_str(), "a" | "b" | "c") {
        bail!("unknown variant {variant:?}; expected a, b, or c");
    }

    let rpcs3_exe = std::env::var("RPCS3_EXE").context("RPCS3_EXE env var required")?;
    let serial =
        std::env::var("RPCS3_TEST_SERIAL").context("RPCS3_TEST_SERIAL env var required")?;

    // Defensive lockfile clear — previous forced kills can leave RPCS3.buf.
    if let Some(dir) = std::path::Path::new(&rpcs3_exe).parent() {
        let _ = std::fs::remove_file(dir.join("RPCS3.buf"));
    }

    eprintln!("[{variant}] launching RPCS3 (library view)");
    let mut proc = RpcsProcess::launch_library(std::path::Path::new(&rpcs3_exe))?;
    proc.wait_ready(Duration::from_secs(45))?;

    let driver = UiaPortalDriver::new()?;
    eprintln!("[{variant}] open_dialog (cold-library nav)");
    driver.open_dialog()?;

    eprintln!("[{variant}] creating cover window");
    let cover_hwnd = create_cover_window(&variant)?;
    // Brief pause so an operator watching the HTPC can see the cover appear.
    sleep(Duration::from_millis(800));

    eprintln!("[{variant}] attempting boot: serial {serial}");
    let started = Instant::now();
    let result = match variant.as_str() {
        "a" => boot_a_sendinput_through_transparent(&driver, &serial),
        "b" => boot_b_lower_zorder_then_click(&driver, &serial, cover_hwnd),
        "c" => boot_c_postmessage_to_rpcs3(&serial),
        _ => unreachable!(),
    };
    let elapsed = started.elapsed();

    match &result {
        Ok(()) => eprintln!("[{variant}] ✅ BOOT SUCCEEDED in {elapsed:?}"),
        Err(e) => eprintln!("[{variant}] ❌ BOOT FAILED after {elapsed:?}: {e}"),
    }

    // Teardown: destroy cover, shutdown RPCS3.
    unsafe {
        let _ = DestroyWindow(cover_hwnd);
    }
    eprintln!("[{variant}] shutting down RPCS3");
    if let Err(e) = proc.shutdown_graceful(Duration::from_secs(15)) {
        eprintln!("shutdown_graceful error (probably benign): {e}");
    }

    result
}

// ---------------------------------------------------------------------------
// Cover window
// ---------------------------------------------------------------------------

extern "system" fn cover_wnd_proc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) }
}

/// Build a fullscreen topmost window. Variant (a) adds `WS_EX_LAYERED +
/// WS_EX_TRANSPARENT` and a semi-transparent alpha so mouse events fall
/// through but the cover is still visually present. Variants (b) and (c)
/// keep the cover fully opaque and catchable — so the Z-order / message
/// routing differences are what we're actually measuring.
fn create_cover_window(variant: &str) -> Result<HWND> {
    let class_name: Vec<u16> = "ZOrderProbeCover\0".encode_utf16().collect();
    let window_name: Vec<u16> = "zorder-probe-cover\0".encode_utf16().collect();

    unsafe {
        // Deep-blue fill so it's visually obvious the cover is up.
        let brush = CreateSolidBrush(COLORREF(0x00_80_30_10));
        let wc = WNDCLASSW {
            lpfnWndProc: Some(cover_wnd_proc),
            hInstance: Default::default(),
            lpszClassName: PCWSTR(class_name.as_ptr()),
            hbrBackground: brush,
            ..Default::default()
        };
        // RegisterClassW returns 0 if it fails, but duplicate-class errors
        // are fine — we only fail if CreateWindowExW then fails.
        RegisterClassW(&wc);

        let sw = GetSystemMetrics(SM_CXSCREEN).max(1);
        let sh = GetSystemMetrics(SM_CYSCREEN).max(1);

        let ex_style = if variant == "a" {
            WS_EX_TOPMOST | WS_EX_LAYERED | WS_EX_TRANSPARENT
        } else {
            WS_EX_TOPMOST
        };

        let hwnd = CreateWindowExW(
            ex_style,
            PCWSTR(class_name.as_ptr()),
            PCWSTR(window_name.as_ptr()),
            WS_POPUP | WS_VISIBLE,
            0,
            0,
            sw,
            sh,
            None,
            None,
            None,
            None,
        )
        .context("CreateWindowExW")?;

        if variant == "a" {
            // Alpha 180/255 so the operator can see RPCS3 behind but the
            // window is still "there" — matches what an egui launcher with
            // a mostly-opaque background would look like.
            SetLayeredWindowAttributes(hwnd, COLORREF(0), 180, LWA_ALPHA)
                .context("SetLayeredWindowAttributes")?;
        }

        let _ = ShowWindow(hwnd, SW_SHOW);
        Ok(hwnd)
    }
}

// ---------------------------------------------------------------------------
// Variant (a): cover is click-through → SendInput goes through
// ---------------------------------------------------------------------------

fn boot_a_sendinput_through_transparent(driver: &UiaPortalDriver, serial: &str) -> Result<()> {
    // The cover is WS_EX_TRANSPARENT, so SendInput at screen coords hits
    // whatever window is underneath at those coords — RPCS3, in this case.
    // `boot_game_by_serial` already uses `SendInput`; nothing special to do.
    driver.boot_game_by_serial(serial, Duration::from_secs(30))
}

// ---------------------------------------------------------------------------
// Variant (b): lower cover Z-order during click, restore after
// ---------------------------------------------------------------------------

fn boot_b_lower_zorder_then_click(
    driver: &UiaPortalDriver,
    serial: &str,
    cover: HWND,
) -> Result<()> {
    unsafe {
        // SWP_NOMOVE | SWP_NOSIZE means "just change z-order, don't
        // relayout". HWND_BOTTOM puts the cover at the bottom of the
        // z-order, below RPCS3, so SendInput at screen coords goes to RPCS3.
        SetWindowPos(
            cover,
            Some(HWND_BOTTOM),
            0,
            0,
            0,
            0,
            SWP_NOMOVE | SWP_NOSIZE,
        )
        .context("SetWindowPos HWND_BOTTOM")?;
    }
    // Give the compositor a tick to apply the z-order change before we start
    // synthesising input.
    sleep(Duration::from_millis(120));

    let result = driver.boot_game_by_serial(serial, Duration::from_secs(30));

    // Restore topmost regardless of boot outcome.
    unsafe {
        let _ = SetWindowPos(
            cover,
            Some(HWND_TOPMOST),
            0,
            0,
            0,
            0,
            SWP_NOMOVE | SWP_NOSIZE,
        );
    }
    result
}

// ---------------------------------------------------------------------------
// Variant (c): PostMessage mouse + key events directly to RPCS3's HWND
// ---------------------------------------------------------------------------

fn boot_c_postmessage_to_rpcs3(serial: &str) -> Result<()> {
    let automation = UIAutomation::new()?;
    let walker = automation.create_tree_walker()?;
    let main = find_rpcs3_main(&automation, &walker)?;
    let main_hwnd_raw: isize = main.get_native_window_handle()?.into();
    let main_hwnd = HWND(main_hwnd_raw as _);

    let cell = find_cell_by_name(&walker, &main, serial)
        .ok_or_else(|| anyhow!("no DataItem named {serial} under RPCS3 main"))?;
    let rect = cell.get_bounding_rectangle()?;
    let cx_screen = rect.get_left() + (rect.get_right() - rect.get_left()) / 2;
    let cy_screen = rect.get_top() + (rect.get_bottom() - rect.get_top()) / 2;

    // Convert screen coords to RPCS3-main-window-relative coords.
    // PostMessage WM_LBUTTONDOWN/UP takes window-client coords in lParam.
    // The UIA bounding rect is screen-space; subtract the main window's
    // top-left to get client-relative (close enough — RPCS3 in library
    // view has no menu bar offset for the list widget we care about).
    let mut wrect = RECT::default();
    unsafe {
        let _ = GetWindowRect(main_hwnd, &mut wrect);
    }
    let lx = cx_screen - wrect.left;
    let ly = cy_screen - wrect.top;
    eprintln!(
        "[c] cell screen ({cx_screen},{cy_screen}) → window-rel ({lx},{ly}) \
         under main HWND {:?}",
        main_hwnd.0
    );

    let pack_xy = pack_lparam(lx, ly);
    unsafe {
        // MK_LBUTTON = 0x0001 — per docs, mouse button state flags in
        // wParam should reflect held buttons. 1 for press, 0 for release.
        PostMessageW(Some(main_hwnd), WM_LBUTTONDOWN, WPARAM(0x0001), pack_xy)?;
        sleep(Duration::from_millis(50));
        PostMessageW(Some(main_hwnd), WM_LBUTTONUP, WPARAM(0), pack_xy)?;
        sleep(Duration::from_millis(120));
        // Now the Enter keypress that activates the selected cell.
        let vk = WPARAM(VK_RETURN.0 as usize);
        // lParam for WM_KEYDOWN: bit 0-15 = repeat count (1), others 0.
        let kbd_lparam = LPARAM(0x0001);
        PostMessageW(Some(main_hwnd), WM_KEYDOWN, vk, kbd_lparam)?;
        sleep(Duration::from_millis(30));
        // For WM_KEYUP, bit 30 (prev key state) + bit 31 (transition) should
        // be set — 0xC0000001 is the canonical value.
        PostMessageW(
            Some(main_hwnd),
            WM_KEYUP,
            vk,
            LPARAM(0xC0000001u32 as i32 as isize),
        )?;
    }

    wait_for_viewport(Duration::from_secs(30))
}

fn pack_lparam(x: i32, y: i32) -> LPARAM {
    let xl = (x as u16 as u32) & 0xFFFF;
    let yl = (y as u16 as u32) & 0xFFFF;
    LPARAM((xl | (yl << 16)) as isize)
}

// ---------------------------------------------------------------------------
// Shared helpers (mirrors of the private helpers inside `uia.rs`)
// ---------------------------------------------------------------------------

fn find_rpcs3_main(automation: &UIAutomation, walker: &UITreeWalker) -> Result<UIElement> {
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

fn wait_for_viewport(timeout: Duration) -> Result<()> {
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        if find_viewport().is_some() {
            return Ok(());
        }
        sleep(Duration::from_millis(100));
    }
    bail!("viewport didn't appear within {timeout:?}")
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
