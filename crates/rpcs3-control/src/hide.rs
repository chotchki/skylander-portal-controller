//! Win32 off-screen move for the Qt portal dialog. UIA's `TransformPattern`
//! reports success but doesn't actually move the window (see Phase 1a research).
//! This helper goes directly to `SetWindowPos` via the dialog's native HWND.

use anyhow::{anyhow, Context, Result};
use uiautomation::UIElement;
use windows::Win32::Foundation::HWND;
use windows::Win32::UI::WindowsAndMessaging::{
    SetWindowPos, SET_WINDOW_POS_FLAGS, SWP_NOSIZE, SWP_NOZORDER,
};

pub fn set_position(el: &UIElement, x: i32, y: i32) -> Result<()> {
    let raw = el
        .get_native_window_handle()
        .context("UIA element exposes no native HWND")?;
    let hwnd_raw: isize = raw.into();
    if hwnd_raw == 0 {
        return Err(anyhow!("native HWND is null"));
    }
    let hwnd = HWND(hwnd_raw as _);
    let flags: SET_WINDOW_POS_FLAGS = SWP_NOSIZE | SWP_NOZORDER;
    unsafe {
        SetWindowPos(hwnd, None, x, y, 0, 0, flags).context("SetWindowPos failed")?;
    }
    Ok(())
}
