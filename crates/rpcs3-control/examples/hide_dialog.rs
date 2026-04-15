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
        other => bail!("unknown command {other:?} — expected hide|show|probe"),
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
