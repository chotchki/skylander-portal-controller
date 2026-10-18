//! What the `#[ignore]`d live tests share: the keys they need, and a skip (not a
//! failure) when one is unset, as the disc tests do.

use std::path::PathBuf;
use std::time::Duration;

use oracle_harness::env;
use oracle_harness::home::{self, InitOptions, OracleHome};
use oracle_harness::snapshot::dev_rpcs3_dirs;
use oracle_harness::template::ConfigTemplate;

pub struct Live {
    pub exe: PathBuf,
    pub home: OracleHome,
    /// The dev's RPCS3 config and cache dirs, which a run must leave as it found them.
    pub dev_dirs: [PathBuf; 2],
}

/// The oracle binary and a freshly (re)initialised home, or `None` with the
/// reason printed when a key is missing.
pub fn live() -> Option<Live> {
    let (Some(exe), Some(home), Some(dump)) = (
        env::path(env::ORACLE_EXE),
        env::path(env::ORACLE_HOME),
        env::path(env::GIANTS_DUMP),
    ) else {
        eprintln!(
            "skip: set {}, {} and {} (env or .env.dev)",
            env::ORACLE_EXE,
            env::ORACLE_HOME,
            env::GIANTS_DUMP
        );
        return None;
    };
    let real_home = PathBuf::from(std::env::var_os("HOME").expect("HOME"));
    let home = home::init(
        &home,
        &InitOptions {
            real_home: real_home.clone(),
            giants_dump: dump,
            config: template(),
        },
    )
    .expect("oracle home init");
    Some(Live {
        exe,
        home,
        dev_dirs: dev_rpcs3_dirs(&real_home),
    })
}

/// The harness's template, with `ORACLE_SHADER_MODE` from the process env as
/// its shader mode when set (AF.4 boots the pin's own default, the mode the two
/// upstream fixes are for).
pub fn template() -> ConfigTemplate {
    let mut config = ConfigTemplate::default();
    if let Some(mode) = std::env::var("ORACLE_SHADER_MODE")
        .ok()
        .filter(|m| !m.is_empty())
    {
        config.shader_mode = mode;
    }
    config
}

/// `key` in seconds from the process env, else `default`.
pub fn secs(key: &str, default: u64) -> Duration {
    Duration::from_secs(
        std::env::var(key)
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(default),
    )
}
