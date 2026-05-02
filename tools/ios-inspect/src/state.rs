//! Session state persisted across CLI invocations.
//!
//! Multi-device shape: `State.devices` holds one entry per booted
//! simulator, each carrying its own proxy PID + port range. The CLI
//! supports up to a small handful of devices (typical is iPad +
//! iPhone for the 2-phone product flow); there's no hard cap, the
//! port range starts at 9221 and grows by 2 per device.
//!
//! Backwards-compat: an older state file (pre PLAN 10.2) was a flat
//! single-device JSON object. `load` recognises that shape and
//! migrates it into the new `{ devices: [...] }` envelope so a
//! returning user doesn't have to re-`boot`.

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

const STATE_PATH: &str = "/tmp/ios-inspect-state.json";

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct State {
    pub devices: Vec<DeviceState>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct DeviceState {
    pub udid: String,
    pub device_name: String,
    pub runtime: String,
    pub socket_path: PathBuf,
    pub proxy_pid: u32,
    /// Control port the proxy listens on (HTTP root listing). One per
    /// device, allocated `9221 + 2*N` at boot.
    pub control_port: u16,
    /// Per-device sub-port the proxy serves WS on. `control_port + 1`
    /// by convention; pinned in state so the WS URL is stable across
    /// reconnects without re-parsing the proxy's HTML.
    pub device_port: u16,
}

impl State {
    pub fn summary(&self) -> String {
        if self.devices.is_empty() {
            return "(no devices)".into();
        }
        self.devices
            .iter()
            .map(|d| d.summary())
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// Find a device by `name` (substring, case-insensitive) or by
    /// exact UDID. Returns `None` if no match.
    pub fn find(&self, key: &str) -> Option<&DeviceState> {
        let lc = key.to_lowercase();
        self.devices
            .iter()
            .find(|d| d.udid == key || d.device_name.to_lowercase().contains(&lc))
    }

    /// Mutable variant of [`find`].
    pub fn find_mut(&mut self, key: &str) -> Option<&mut DeviceState> {
        let lc = key.to_lowercase();
        self.devices
            .iter_mut()
            .find(|d| d.udid == key || d.device_name.to_lowercase().contains(&lc))
    }
}

impl DeviceState {
    pub fn summary(&self) -> String {
        format!(
            "{} ({}) · proxy pid {} · ports {}/{} · socket {}",
            self.device_name,
            self.runtime,
            self.proxy_pid,
            self.control_port,
            self.device_port,
            self.socket_path.display()
        )
    }

    /// Short label suitable for output prefixing — lower-cased,
    /// spaces collapsed to dashes. e.g. "iphone-17-pro".
    pub fn label(&self) -> String {
        self.device_name
            .to_lowercase()
            .chars()
            .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
            .collect::<String>()
            .split('-')
            .filter(|s| !s.is_empty())
            .collect::<Vec<_>>()
            .join("-")
    }
}

pub fn load() -> Result<Option<State>> {
    let path = std::path::Path::new(STATE_PATH);
    if !path.exists() {
        return Ok(None);
    }
    let raw = std::fs::read_to_string(path).context("read state file")?;
    // Try the new shape first; fall back to migrating an old single-
    // device file from before PLAN 10.2.
    if let Ok(s) = serde_json::from_str::<State>(&raw) {
        return Ok(Some(s));
    }
    if let Ok(legacy) = serde_json::from_str::<LegacyState>(&raw) {
        let migrated = State {
            devices: vec![DeviceState {
                udid: legacy.udid,
                device_name: legacy.device_name,
                runtime: legacy.runtime,
                socket_path: legacy.socket_path,
                proxy_pid: legacy.proxy_pid,
                // Pre-10.2 always used 9221 control / 9222 device.
                control_port: 9221,
                device_port: 9222,
            }],
        };
        // Persist the migrated shape so subsequent loads don't repeat
        // the migration path.
        save(&migrated)?;
        return Ok(Some(migrated));
    }
    Err(anyhow::anyhow!(
        "state file at {} is neither current nor legacy shape — \
         delete it and re-run `ios-inspect boot`",
        STATE_PATH,
    ))
}

pub fn save(s: &State) -> Result<()> {
    let raw = serde_json::to_string_pretty(s)?;
    std::fs::write(STATE_PATH, raw).context("write state file")?;
    Ok(())
}

pub fn clear() -> Result<()> {
    let path = std::path::Path::new(STATE_PATH);
    if path.exists() {
        std::fs::remove_file(path).context("remove state file")?;
    }
    Ok(())
}

/// Allocate the next free `(control_port, device_port)` pair for a new
/// device. Walks `9221 + 2*N` until both ports in the pair are unused
/// across the existing devices. Caller is responsible for not racing
/// against another concurrent allocator (CLI is single-process so
/// this is fine in practice).
pub fn next_port_pair(existing: &[DeviceState]) -> (u16, u16) {
    let used: std::collections::HashSet<u16> = existing
        .iter()
        .flat_map(|d| [d.control_port, d.device_port])
        .collect();
    let mut n = 0u16;
    loop {
        let ctrl = 9221 + 2 * n;
        let dev = ctrl + 1;
        if !used.contains(&ctrl) && !used.contains(&dev) {
            return (ctrl, dev);
        }
        n += 1;
    }
}

/// Pre-10.2 single-device shape. Read-only — kept solely for the
/// migration path in [`load`].
#[derive(Deserialize)]
struct LegacyState {
    udid: String,
    device_name: String,
    runtime: String,
    socket_path: PathBuf,
    proxy_pid: u32,
}
