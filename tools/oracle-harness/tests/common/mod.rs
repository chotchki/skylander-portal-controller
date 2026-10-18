//! What the `#[ignore]`d live tests share: the keys they need, and a skip (not a
//! failure) when one is unset, as the disc tests do.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use oracle_harness::env;
use oracle_harness::home::{self, InitOptions, OracleHome};
use oracle_harness::launch::Oracle;
use oracle_harness::rpcs3_log::{LogTail, opened_path};
use oracle_harness::snapshot::dev_rpcs3_dirs;
use oracle_harness::template::ConfigTemplate;
use skylander_core::SlotIndex;
use skylander_rpcs3_control::PortalDriver;
use skylander_rpcs3_control::ipc::proto::{EmuState, PadButton};

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
/// its shader mode when set, and the game's sound on when `ORACLE_AUDIO` is set
/// to anything but empty or `0` (silent otherwise).
pub fn template() -> ConfigTemplate {
    let mut config = ConfigTemplate {
        audio: std::env::var("ORACLE_AUDIO").is_ok_and(|v| !v.is_empty() && v != "0"),
        ..ConfigTemplate::default()
    };
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

/// What getting a hero into a level took ([`reach_hero`]).
#[allow(dead_code)] // not every live test goes into a level
#[derive(Debug)]
pub struct Hero {
    pub level: String,
    pub archive: String,
    pub presses: u32,
    pub placements: u32,
    pub state: EmuState,
    /// The run dir's copy of the figure, the file the game writes.
    pub figure_copy: PathBuf,
}

/// AC.13's scenario from a cold boot's playable (AF.4): CROSS over P6 once a
/// second through the opening (videos, menu, story, save slot, Eon's monologue:
/// `tools/playthrough`'s gates.json) until RPCS3's log shows a level other than
/// the title opened, then a fresh copy of `figure` on the portal over P1,
/// cleared and placed again every 20 s in case the game only takes a placement
/// at its prompt, until a character archive opens after it (the hero loading).
/// Bounded by `ORACLE_LEVEL_SECS` (default 600); panics past it.
#[allow(dead_code)] // not every live test goes into a level
pub fn reach_hero(
    oracle: &mut Oracle,
    log: &mut LogTail,
    figure: &Path,
    launched: Instant,
) -> Hero {
    const REPLACE_EVERY: Duration = Duration::from_secs(20);
    let figure_copy = oracle.run_paths().tmp.join("figure.sky");
    std::fs::copy(figure, &figure_copy).unwrap();
    let slot = SlotIndex::new(0).unwrap();
    let deadline = Instant::now() + secs("ORACLE_LEVEL_SECS", 600);
    let (mut level, mut placed, mut placements, mut presses) = (None, None::<Instant>, 0, 0);
    let archive = loop {
        assert!(
            Instant::now() < deadline,
            "no hero after {presses} CROSS presses and {placements} placements (level {level:?})"
        );
        // Fails fast when RPCS3 has exited or reports frozen or stopped.
        let state = oracle.wait_for(Duration::from_secs(5), |_| true).unwrap();
        let mut hero = None;
        for line in log.lines() {
            let Some(path) = opened_path(&line) else {
                continue;
            };
            if level.is_none() && path.contains("/USRDIR/level/") && !path.contains("/level/title")
            {
                eprintln!(
                    "+{:.0} s, frame {}: level {path}",
                    launched.elapsed().as_secs_f64(),
                    state.frames
                );
                level = Some(path.to_owned());
            } else if placed.is_some()
                && path.contains("/USRDIR/character/")
                && path.ends_with(".arc")
            {
                hero = Some(path.to_owned());
            }
        }
        if let Some(hero) = hero {
            break hero;
        }
        let driver = oracle.driver();
        if level.is_some() && placed.is_none_or(|t| t.elapsed() >= REPLACE_EVERY) {
            if placed.is_some() {
                driver.clear(slot).unwrap();
            }
            driver.load(slot, &figure_copy).unwrap();
            placed = Some(Instant::now());
            placements += 1;
        }
        driver.press_button(PadButton::Cross, 120).unwrap();
        presses += 1;
        std::thread::sleep(Duration::from_millis(880));
    };
    let state = oracle.wait_for(Duration::from_secs(5), |_| true).unwrap();
    eprintln!(
        "+{:.0} s, frame {}: hero {archive} after {presses} CROSS presses and {placements} placement(s)",
        launched.elapsed().as_secs_f64(),
        state.frames
    );
    Hero {
        level: level.unwrap_or_default(),
        archive,
        presses,
        placements,
        state,
        figure_copy,
    }
}
