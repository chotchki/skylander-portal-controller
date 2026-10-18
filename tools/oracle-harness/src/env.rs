//! The dev keys the harness reads: the process environment first, then the
//! workspace's `.env.dev` (the same lookup the `#[ignore]`d disc tests use).

use std::path::{Path, PathBuf};

/// The oracle binary, outside the repo (`.../RPCS3.app/Contents/MacOS/rpcs3`).
pub const ORACLE_EXE: &str = "GUAC_ORACLE_EXE";
/// The oracle's isolated HOME, built by `oracle home init`.
pub const ORACLE_HOME: &str = "GUAC_ORACLE_HOME";
/// The Giants dump root, the folder holding `PS3_GAME`.
pub const GIANTS_DUMP: &str = "GUAC_GIANTS_DUMP";
/// Where captures, states and run output go, outside the repo.
pub const CAPTURE_DIR: &str = "GUAC_CAPTURE_DIR";

/// `key` from the environment, else from the workspace's `.env.dev`. An empty
/// value counts as unset.
pub fn key(key: &str) -> Option<String> {
    if let Ok(value) = std::env::var(key) {
        return (!value.is_empty()).then_some(value);
    }
    let dotenv = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../.env.dev");
    let text = std::fs::read_to_string(dotenv).ok()?;
    dotenv_value(&text, key)
}

/// `None` when the key is unset or empty in both places.
pub fn path(name: &str) -> Option<PathBuf> {
    key(name).map(PathBuf::from)
}

/// The value `.env.dev` text gives `key`: the last `KEY=value` line wins, as a
/// shell's `source` would have it, and surrounding quotes are dropped.
fn dotenv_value(text: &str, key: &str) -> Option<String> {
    text.lines()
        .rev()
        .filter_map(|line| {
            let line = line.trim();
            if line.starts_with('#') {
                return None;
            }
            let (k, v) = line.split_once('=')?;
            (k.trim() == key).then(|| v.trim().trim_matches('"').to_owned())
        })
        .find(|v| !v.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dotenv_lines_resolve_the_last_uncommented_value() {
        let text = "# GUAC_ORACLE_HOME=/commented\nGUAC_ORACLE_HOME=/a\nOTHER=1\nGUAC_ORACLE_HOME = \"/b c\"\nEMPTY=\n";
        assert_eq!(
            dotenv_value(text, "GUAC_ORACLE_HOME").as_deref(),
            Some("/b c")
        );
        assert_eq!(dotenv_value(text, "OTHER").as_deref(), Some("1"));
        assert_eq!(dotenv_value(text, "EMPTY"), None);
        assert_eq!(dotenv_value(text, "MISSING"), None);
    }
}
