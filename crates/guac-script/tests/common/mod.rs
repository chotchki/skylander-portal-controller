//! What the `#[ignore]`d disc tests share: finding the Giants dump.

use std::path::{Path, PathBuf};

/// A key from the environment, else from the workspace's `.env.dev`.
pub fn env_or_dotenv(key: &str) -> Option<String> {
    if let Ok(value) = std::env::var(key) {
        return Some(value);
    }
    let dotenv = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../.env.dev");
    let text = std::fs::read_to_string(dotenv).ok()?;
    text.lines().find_map(|line| {
        let (k, v) = line.trim().split_once('=')?;
        (k.trim() == key).then(|| v.trim().trim_matches('"').to_owned())
    })
}

/// `PS3_GAME/USRDIR` under `GUAC_GIANTS_DUMP`.
pub fn usrdir() -> PathBuf {
    let root = env_or_dotenv("GUAC_GIANTS_DUMP")
        .expect("set GUAC_GIANTS_DUMP (env or .env.dev) to the Giants dump");
    let usrdir = Path::new(&root).join("PS3_GAME/USRDIR");
    assert!(usrdir.is_dir(), "no PS3_GAME/USRDIR under {root}");
    usrdir
}
