//! Manual verification of 3.2 off-screen hide / restore.
//!
//!   cargo run -p skylander-rpcs3-control --example hide_dialog -- hide
//!   cargo run -p skylander-rpcs3-control --example hide_dialog -- show
//!   cargo run -p skylander-rpcs3-control --example hide_dialog -- probe
//!
//! Requires RPCS3 running with the Skylanders Manager dialog open.

#![cfg(windows)]

use std::env;
use std::time::Duration;

use anyhow::{bail, Result};
use skylander_core::{SlotIndex, SlotState};
use skylander_rpcs3_control::{PortalDriver, UiaPortalDriver};

fn main() -> Result<()> {
    let arg = env::args().nth(1).unwrap_or_else(|| "probe".into());
    let driver = UiaPortalDriver::new()?;

    match arg.as_str() {
        "hide" => {
            driver.hide_dialog_offscreen()?;
            println!("moved dialog off-screen");
            verify_still_drivable(&driver)?;
        }
        "show" => {
            driver.restore_dialog_visible(400, 300)?;
            println!("moved dialog back on-screen at 400,300");
        }
        "probe" => {
            verify_still_drivable(&driver)?;
        }
        "enumerate" => enumerate_all_matching()?,
        other => bail!("unknown command {other:?} — expected hide|show|probe|enumerate"),
    }
    Ok(())
}

fn enumerate_all_matching() -> Result<()> {
    use windows::Win32::Foundation::{HWND, LPARAM, RECT};
    use windows::Win32::UI::WindowsAndMessaging::{
        EnumWindows, GetClassNameW, GetWindowRect, GetWindowTextLengthW, GetWindowTextW,
        IsWindowVisible,
    };
    use windows::core::BOOL;

    struct Ctx {
        hits: Vec<(HWND, String, String, RECT, bool)>,
    }
    extern "system" fn cb(hwnd: HWND, lparam: LPARAM) -> BOOL {
        let ctx = unsafe { &mut *(lparam.0 as *mut Ctx) };
        unsafe {
            let len = GetWindowTextLengthW(hwnd);
            if len > 0 {
                let mut buf = vec![0u16; (len + 1) as usize];
                let read = GetWindowTextW(hwnd, &mut buf);
                let title = String::from_utf16_lossy(&buf[..read as usize]);
                if title == "Skylanders Manager" {
                    let mut cbuf = [0u16; 256];
                    let n = GetClassNameW(hwnd, &mut cbuf);
                    let class = String::from_utf16_lossy(&cbuf[..n.max(0) as usize]);
                    let mut rect = RECT::default();
                    let _ = GetWindowRect(hwnd, &mut rect);
                    let vis = IsWindowVisible(hwnd).as_bool();
                    ctx.hits.push((hwnd, title, class, rect, vis));
                }
            }
        }
        BOOL(1)
    }
    let mut ctx = Ctx { hits: Vec::new() };
    unsafe {
        let _ = EnumWindows(Some(cb), LPARAM(&mut ctx as *mut _ as isize));
    }
    for (hwnd, title, class, rect, vis) in &ctx.hits {
        println!(
            "hwnd={:?}  class={:<25}  rect={}x{}@{},{}  vis={}  title={:?}",
            hwnd.0,
            class,
            rect.right - rect.left,
            rect.bottom - rect.top,
            rect.left,
            rect.top,
            vis,
            title,
        );
    }
    if ctx.hits.is_empty() {
        println!("(no windows titled 'Skylanders Manager' found)");
    }
    Ok(())
}

fn verify_still_drivable(driver: &UiaPortalDriver) -> Result<()> {
    // Give the window manager a moment after any move.
    std::thread::sleep(Duration::from_millis(250));
    let slots = driver.read_slots()?;
    println!("read_slots succeeded:");
    for (i, s) in slots.iter().enumerate() {
        let idx = SlotIndex::new(i as u8).unwrap();
        let desc = match s {
            SlotState::Empty => "empty".into(),
            SlotState::Loading { .. } => "loading".into(),
            SlotState::Loaded { display_name, .. } => format!("loaded({display_name})"),
            SlotState::Error { message } => format!("error({message})"),
        };
        println!("  {idx}: {desc}");
    }
    Ok(())
}
