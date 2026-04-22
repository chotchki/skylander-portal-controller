//! `xcrun simctl` wrappers.

use anyhow::{Context, Result, bail};
use serde::Deserialize;
use std::path::Path;
use tokio::process::Command;

pub struct Device {
    pub udid: String,
    pub name: String,
    pub runtime: String,
}

/// Pick a device by `name` (substring match) or auto-select the most recent
/// Dynamic-Island-capable iPhone if unspecified. The iPhone 15/16/17 Pro
/// lines + "iPhone Air" all qualify; plain "iPhone 15/16/17" (no Pro) also
/// has Dynamic Island from iPhone 15 onward, so use that as the heuristic.
pub fn pick_device(name: Option<&str>) -> Result<Device> {
    let out = std::process::Command::new("xcrun")
        .args(["simctl", "list", "devices", "available", "--json"])
        .output()
        .context("run `xcrun simctl list --json`")?;
    if !out.status.success() {
        bail!("simctl list failed: {}", String::from_utf8_lossy(&out.stderr));
    }
    let listing: SimctlList = serde_json::from_slice(&out.stdout)
        .context("parse simctl list --json")?;

    let mut candidates: Vec<Device> = Vec::new();
    for (runtime, devices) in &listing.devices {
        // Prefer iOS runtimes, skip tvOS / watchOS.
        if !runtime.contains("iOS") {
            continue;
        }
        for d in devices {
            if !d.is_available {
                continue;
            }
            candidates.push(Device {
                udid: d.udid.clone(),
                name: d.name.clone(),
                runtime: runtime.clone(),
            });
        }
    }
    if candidates.is_empty() {
        bail!("no iOS simulator devices available — install one via Xcode › Settings › Platforms");
    }

    if let Some(wanted) = name {
        // Substring match, case-insensitive.
        let wanted_lc = wanted.to_lowercase();
        for c in candidates {
            if c.name.to_lowercase().contains(&wanted_lc) {
                return Ok(c);
            }
        }
        bail!("no device name contains {wanted:?}");
    }

    // Auto-pick: highest iOS version + Dynamic-Island-capable iPhone. The
    // runtime key is like "com.apple.CoreSimulator.SimRuntime.iOS-26-2";
    // sorting lexicographically on that key lines up with version order
    // because the zero-padding is absent but the segments sort the same
    // digit-by-digit for our range (17 through 26+).
    candidates.sort_by(|a, b| b.runtime.cmp(&a.runtime));
    for c in &candidates {
        if is_dynamic_island_iphone(&c.name) {
            return Ok(Device {
                udid: c.udid.clone(),
                name: c.name.clone(),
                runtime: c.runtime.clone(),
            });
        }
    }
    // Fallback: first available iPhone of any kind.
    for c in &candidates {
        if c.name.starts_with("iPhone") {
            return Ok(Device {
                udid: c.udid.clone(),
                name: c.name.clone(),
                runtime: c.runtime.clone(),
            });
        }
    }
    Ok(candidates.into_iter().next().unwrap())
}

fn is_dynamic_island_iphone(name: &str) -> bool {
    // iPhone 14 Pro onward has Dynamic Island. Our heuristic only checks
    // the ones Xcode 15+ ships.
    let lc = name.to_lowercase();
    ["iphone 15", "iphone 16", "iphone 17", "iphone air"]
        .iter()
        .any(|k| lc.contains(k))
}

pub async fn boot_if_needed(udid: &str) -> Result<()> {
    let out = Command::new("xcrun")
        .args(["simctl", "list", "devices", "booted"])
        .output()
        .await?;
    let stdout = String::from_utf8_lossy(&out.stdout);
    if stdout.contains(udid) {
        return Ok(());
    }
    let st = Command::new("xcrun")
        .args(["simctl", "boot", udid])
        .status()
        .await?;
    if !st.success() {
        bail!("simctl boot {udid} failed");
    }
    Ok(())
}

pub async fn launch_simulator_app() -> Result<()> {
    // `open -a Simulator` is the standard way to surface the sim window.
    // Idempotent: if Simulator.app is already running, this brings it to
    // the front.
    Command::new("open").args(["-a", "Simulator"]).status().await?;
    Ok(())
}

pub async fn openurl(url: &str) -> Result<()> {
    let st = Command::new("xcrun")
        .args(["simctl", "openurl", "booted", url])
        .status()
        .await?;
    if !st.success() {
        bail!("simctl openurl failed");
    }
    Ok(())
}

pub async fn screenshot(path: &Path) -> Result<()> {
    let st = Command::new("xcrun")
        .args(["simctl", "io", "booted", "screenshot"])
        .arg(path)
        .status()
        .await?;
    if !st.success() {
        bail!("simctl io screenshot failed");
    }
    Ok(())
}

pub async fn shutdown(udid: &str) -> Result<()> {
    let _ = Command::new("xcrun")
        .args(["simctl", "shutdown", udid])
        .status()
        .await?;
    // Don't hard-fail if the device was already shut down.
    Ok(())
}

// ----- JSON shape of `simctl list devices --json` -----

#[derive(Deserialize)]
struct SimctlList {
    devices: std::collections::HashMap<String, Vec<SimctlDevice>>,
}

#[derive(Deserialize)]
struct SimctlDevice {
    udid: String,
    name: String,
    #[serde(rename = "isAvailable", default)]
    is_available: bool,
}
