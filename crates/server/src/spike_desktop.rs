//! TEMPORARY spike (PLAN 20.1) — validate that a *windowed* launcher can slave a
//! sibling top-level window's geometry (move **and** resize, z-ordered directly
//! below) without flicker, as a stand-in for the real RPCS3 game window in the
//! future Desktop window mode. The TV path only ever does z-order
//! (`SWP_NOMOVE | SWP_NOSIZE`); this proves the move+resize tracking is clean
//! before we wire it into the real coordination (20.4).
//!
//! Gated behind the `SKYLANDER_SPIKE_DESKTOP` env var — NOT a config option and
//! NOT wired into release. Spawns a magenta stand-in window on its own thread
//! (a window must be pumped by its creating thread), and the launcher
//! repositions+resizes it to its client rect each frame. **Delete once 20.1 has
//! answered the go/no-go.**

use std::sync::atomic::{AtomicI32, AtomicIsize, Ordering};

/// Published by the stand-in thread once its window exists; read by the launcher.
static STANDIN_HWND: AtomicIsize = AtomicIsize::new(0);
// Last-applied screen rect, so a steady frame loop doesn't thrash SetWindowPos.
static LAST_X: AtomicI32 = AtomicI32::new(i32::MIN);
static LAST_Y: AtomicI32 = AtomicI32::new(i32::MIN);
static LAST_W: AtomicI32 = AtomicI32::new(i32::MIN);
static LAST_H: AtomicI32 = AtomicI32::new(i32::MIN);

/// Is the spike active? Cheap env check; called from `main` + the UI loop.
pub fn enabled() -> bool {
    std::env::var_os("SKYLANDER_SPIKE_DESKTOP").is_some()
}

/// Spawn the stand-in window on a dedicated thread that owns its message pump.
pub fn spawn_standin() {
    std::thread::Builder::new()
        .name("spike-standin".into())
        .spawn(standin_thread)
        .expect("spawn spike standin thread");
}

fn standin_thread() {
    use windows::Win32::Foundation::{COLORREF, HWND, LPARAM, LRESULT, WPARAM};
    use windows::Win32::Graphics::Gdi::CreateSolidBrush;
    use windows::Win32::System::LibraryLoader::GetModuleHandleW;
    use windows::Win32::UI::WindowsAndMessaging::{
        CreateWindowExW, DefWindowProcW, DispatchMessageW, GetMessageW, MSG, RegisterClassW,
        SW_SHOWNOACTIVATE, ShowWindow, TranslateMessage, WINDOW_EX_STYLE, WNDCLASSW,
        WS_OVERLAPPEDWINDOW,
    };
    use windows::core::w;

    unsafe extern "system" fn wndproc(h: HWND, m: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
        unsafe { DefWindowProcW(h, m, wp, lp) }
    }

    unsafe {
        let Ok(hinstance) = GetModuleHandleW(None) else {
            return;
        };
        let class = w!("SkylanderSpikeStandin");
        // Magenta erase brush (COLORREF is 0x00BBGGRR) so the window is obvious.
        let wc = WNDCLASSW {
            lpfnWndProc: Some(wndproc),
            hInstance: hinstance.into(),
            lpszClassName: class,
            hbrBackground: CreateSolidBrush(COLORREF(0x00FF00FF)),
            ..Default::default()
        };
        RegisterClassW(&wc);

        let Ok(hwnd) = CreateWindowExW(
            WINDOW_EX_STYLE(0),
            class,
            w!("SPIKE STAND-IN — pretend RPCS3"),
            WS_OVERLAPPEDWINDOW,
            200,
            200,
            640,
            480,
            None,
            None,
            Some(hinstance.into()),
            None,
        ) else {
            return;
        };
        STANDIN_HWND.store(hwnd.0 as isize, Ordering::SeqCst);
        let _ = ShowWindow(hwnd, SW_SHOWNOACTIVATE);

        let mut msg = MSG::default();
        while GetMessageW(&mut msg, None, 0, 0).as_bool() {
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }
}

/// Reposition + resize the stand-in to the launcher's client rect (in screen
/// coords), z-ordered directly below the launcher. Only re-applies on an actual
/// change. `launcher_hwnd` is the raw HWND value from eframe's window handle.
pub fn slave_to_launcher(launcher_hwnd: isize) {
    let standin = STANDIN_HWND.load(Ordering::SeqCst);
    if standin == 0 || launcher_hwnd == 0 {
        return;
    }

    use windows::Win32::Foundation::{HWND, POINT, RECT};
    use windows::Win32::Graphics::Gdi::ClientToScreen;
    use windows::Win32::UI::WindowsAndMessaging::{GetClientRect, SWP_NOACTIVATE, SetWindowPos};

    unsafe {
        let launcher = HWND(launcher_hwnd as *mut _);
        let standin = HWND(standin as *mut _);

        let mut rc = RECT::default();
        if GetClientRect(launcher, &mut rc).is_err() {
            return;
        }
        // Client (0,0) → screen coords gives the top-left; the rect's
        // right/bottom are the client width/height (left/top are always 0).
        let mut tl = POINT { x: 0, y: 0 };
        let _ = ClientToScreen(launcher, &mut tl);
        let (cw, ch) = (rc.right - rc.left, rc.bottom - rc.top);
        // Place the stand-in just to the RIGHT of the launcher (same size), so
        // both windows are visible and you can watch it track move+resize in
        // lockstep. The real Desktop mode (20.4) places the game window *behind*
        // the launcher at the same rect; the SetWindowPos mechanics are identical
        // — the offset just makes the tracking observable without a live game.
        let (x, y, w, h) = (tl.x + cw + 16, tl.y, cw, ch);

        if LAST_X.load(Ordering::Relaxed) == x
            && LAST_Y.load(Ordering::Relaxed) == y
            && LAST_W.load(Ordering::Relaxed) == w
            && LAST_H.load(Ordering::Relaxed) == h
        {
            return; // unchanged — skip the SetWindowPos
        }
        LAST_X.store(x, Ordering::Relaxed);
        LAST_Y.store(y, Ordering::Relaxed);
        LAST_W.store(w, Ordering::Relaxed);
        LAST_H.store(h, Ordering::Relaxed);

        // Insert `standin` immediately after `launcher` in z-order → launcher
        // sits directly above it. NOACTIVATE so we don't steal focus.
        let _ = SetWindowPos(standin, Some(launcher), x, y, w, h, SWP_NOACTIVATE);
        tracing::info!(
            x,
            y,
            w,
            h,
            "spike(20.1): slaved stand-in to launcher client rect"
        );
    }
}
