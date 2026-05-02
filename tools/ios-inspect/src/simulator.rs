//! `xcrun simctl` wrappers.

use anyhow::{bail, Context, Result};
use serde::Deserialize;
use std::path::Path;
use tokio::process::Command;

#[derive(Debug, Clone)]
pub struct Device {
    pub udid: String,
    pub name: String,
    pub runtime: String,
}

/// Pick a single device by `name` (substring match) or auto-select the
/// most recent Dynamic-Island-capable iPhone if `name` is `None`.
pub fn pick_device(name: Option<&str>) -> Result<Device> {
    let candidates = list_available()?;
    if candidates.is_empty() {
        bail!("no iOS simulator devices available — install one via Xcode › Settings › Platforms");
    }
    if let Some(wanted) = name {
        return pick_by_name(&candidates, wanted);
    }
    Ok(auto_pick(candidates))
}

/// Pick multiple devices by name. Each name is matched independently
/// (substring, case-insensitive). When `names` is empty, falls back to
/// auto-picking a single device — matches the pre-10.2 behaviour.
///
/// Returns devices in the order specified, deduplicated by UDID. Errors
/// if any name doesn't match a device, so the caller fails fast on
/// typos rather than booting half the requested set.
pub fn pick_devices(names: &[String]) -> Result<Vec<Device>> {
    if names.is_empty() {
        return Ok(vec![pick_device(None)?]);
    }
    let candidates = list_available()?;
    if candidates.is_empty() {
        bail!("no iOS simulator devices available — install one via Xcode › Settings › Platforms");
    }
    let mut out: Vec<Device> = Vec::new();
    let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
    for n in names {
        let dev = pick_by_name(&candidates, n)?;
        if seen.insert(dev.udid.clone()) {
            out.push(dev);
        }
    }
    Ok(out)
}

fn list_available() -> Result<Vec<Device>> {
    let out = std::process::Command::new("xcrun")
        .args(["simctl", "list", "devices", "available", "--json"])
        .output()
        .context("run `xcrun simctl list --json`")?;
    if !out.status.success() {
        bail!(
            "simctl list failed: {}",
            String::from_utf8_lossy(&out.stderr)
        );
    }
    let listing: SimctlList =
        serde_json::from_slice(&out.stdout).context("parse simctl list --json")?;

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
    Ok(candidates)
}

fn pick_by_name(candidates: &[Device], wanted: &str) -> Result<Device> {
    let wanted_lc = wanted.to_lowercase();
    for c in candidates {
        if c.name.to_lowercase().contains(&wanted_lc) {
            return Ok(c.clone());
        }
    }
    bail!("no device name contains {wanted:?}");
}

fn auto_pick(mut candidates: Vec<Device>) -> Device {
    // Highest iOS version + Dynamic-Island-capable iPhone. Runtime keys
    // are like "com.apple.CoreSimulator.SimRuntime.iOS-26-2"; lexicographic
    // sort matches version order across the range we care about.
    candidates.sort_by(|a, b| b.runtime.cmp(&a.runtime));
    if let Some(d) = candidates
        .iter()
        .find(|c| is_dynamic_island_iphone(&c.name))
    {
        return d.clone();
    }
    if let Some(d) = candidates.iter().find(|c| c.name.starts_with("iPhone")) {
        return d.clone();
    }
    candidates.into_iter().next().unwrap()
}

fn is_dynamic_island_iphone(name: &str) -> bool {
    // iPhone 14 Pro onward has Dynamic Island. Heuristic only checks
    // the runtime names Xcode 15+ ships.
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
    Command::new("open")
        .args(["-a", "Simulator"])
        .status()
        .await?;
    Ok(())
}

/// Open a URL on a specific simulator (by UDID). Multi-device tools
/// must NOT use `simctl openurl booted ...` because `booted` resolves
/// to the first booted device — ambiguous when iPad + iPhone are both
/// up.
pub async fn openurl(udid: &str, url: &str) -> Result<()> {
    let st = Command::new("xcrun")
        .args(["simctl", "openurl", udid, url])
        .status()
        .await?;
    if !st.success() {
        bail!("simctl openurl on {udid} failed");
    }
    Ok(())
}

/// Take a full-device-frame PNG of a specific simulator. Same
/// `booted` caveat as `openurl` — pass an explicit UDID.
pub async fn screenshot(udid: &str, path: &Path) -> Result<()> {
    let st = Command::new("xcrun")
        .args(["simctl", "io", udid, "screenshot"])
        .arg(path)
        .status()
        .await?;
    if !st.success() {
        bail!("simctl io screenshot on {udid} failed");
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
