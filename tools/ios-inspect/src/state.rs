//! Session state persisted across CLI invocations.

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

const STATE_PATH: &str = "/tmp/ios-inspect-state.json";

#[derive(Serialize, Deserialize, Clone)]
pub struct State {
    pub udid: String,
    pub device_name: String,
    pub runtime: String,
    pub socket_path: PathBuf,
    pub proxy_pid: u32,
}

impl State {
    pub fn summary(&self) -> String {
        format!(
            "{} ({}) · proxy pid {} · socket {}",
            self.device_name,
            self.runtime,
            self.proxy_pid,
            self.socket_path.display()
        )
    }
}

pub fn load() -> Result<Option<State>> {
    let path = std::path::Path::new(STATE_PATH);
    if !path.exists() {
        return Ok(None);
    }
    let raw = std::fs::read_to_string(path).context("read state file")?;
    let s: State = serde_json::from_str(&raw).context("parse state file")?;
    Ok(Some(s))
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
