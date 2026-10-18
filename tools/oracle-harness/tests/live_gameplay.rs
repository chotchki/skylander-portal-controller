//! Live (openspec AF.4): the oracle gets past boot into a level with a hero in
//! it, with no screen. A cold boot from the oracle home to playable, then CROSS
//! over P6 once a second through the opening (videos, menu, story, save slot,
//! Eon's monologue: `tools/playthrough`'s gates.json). Once RPCS3's log shows a
//! level other than the title opened, a fresh copy of `GUAC_ORACLE_FIGURE` goes
//! on the portal over P1, cleared and placed again every 20 s in case the game
//! only takes a placement at its prompt, until the log shows a character archive
//! opened after it (the hero loading). Then 30 s of frames in the level, and the
//! dev's RPCS3 dirs checked untouched. RPCS3's own log is the evidence; nothing
//! needs to reach a display.
//!
//! Mac, against `GUAC_ORACLE_EXE`; reads `GUAC_ORACLE_HOME`, `GUAC_GIANTS_DUMP`
//! and `GUAC_ORACLE_FIGURE` (a `.sky` from the dev's own pack; the game writes
//! the run dir's copy, never the file) from the env or `.env.dev`, and skips when
//! one is unset. `ORACLE_LEVEL_SECS` (default 600) bounds playable to hero,
//! `ORACLE_SHADER_MODE` overrides the template's shader mode.
//!
//! `cargo test -p oracle-harness --test live_gameplay -- --ignored --nocapture`

#![cfg(unix)]

mod common;

use std::io::{Read, Seek, SeekFrom};
use std::path::PathBuf;
use std::time::{Duration, Instant};

use oracle_harness::env;
use oracle_harness::launch::{self, BootOptions};
use oracle_harness::snapshot::Snapshot;
use skylander_core::SlotIndex;
use skylander_rpcs3_control::PortalDriver;
use skylander_rpcs3_control::ipc::proto::PadButton;

const FIGURE: &str = "GUAC_ORACLE_FIGURE";
const REPLACE_EVERY: Duration = Duration::from_secs(20);

/// The guest path in a `sys_fs_open(path=“...”, ...)` line, which RPCS3 logs at
/// warning level, so a default config has it.
fn opened_path(line: &str) -> Option<&str> {
    let rest = line.split_once("sys_fs_open(path=\u{201c}")?.1;
    Some(rest.split_once('\u{201d}')?.0)
}

/// The lines RPCS3 has appended to its log since the last call.
struct LogTail {
    path: PathBuf,
    offset: u64,
    partial: String,
}

impl LogTail {
    fn new(path: PathBuf) -> Self {
        Self {
            path,
            offset: 0,
            partial: String::new(),
        }
    }

    fn lines(&mut self) -> Vec<String> {
        let Ok(mut file) = std::fs::File::open(&self.path) else {
            return Vec::new();
        };
        // RPCS3 rewrites its log at every start: a shorter file is a new one.
        if file.metadata().is_ok_and(|m| m.len() < self.offset) {
            self.offset = 0;
            self.partial.clear();
        }
        let mut bytes = Vec::new();
        if file.seek(SeekFrom::Start(self.offset)).is_err() || file.read_to_end(&mut bytes).is_err()
        {
            return Vec::new();
        }
        self.offset += bytes.len() as u64;
        self.partial.push_str(&String::from_utf8_lossy(&bytes));
        let Some(end) = self.partial.rfind('\n') else {
            return Vec::new();
        };
        let complete: Vec<String> = self.partial[..end].lines().map(str::to_owned).collect();
        self.partial.drain(..=end);
        complete
    }
}

#[test]
fn opened_paths_come_out_of_rpcs3s_log_line() {
    let line = "\u{b7}W 0:00:54.632944 {PPU[0x1000025] Thread (load) [libfs: 0x00bf2090]} sys_fs: \
                sys_fs_open(path=\u{201c}/dev_bdvd/PS3_GAME/USRDIR/level/a.arc\u{201d}, flags=00, fd=*0x1)";
    assert_eq!(
        opened_path(line),
        Some("/dev_bdvd/PS3_GAME/USRDIR/level/a.arc")
    );
    // The reply line names the file without the quotes; only the request counts.
    assert_eq!(
        opened_path("sys_fs: sys_fs_open(): fd=4, Regular file, '/dev_bdvd/x'"),
        None
    );
    assert_eq!(opened_path("sys_fs_open(path=\u{201c}/unterminated"), None);
}

#[test]
fn the_log_tail_hands_over_whole_lines_once() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("RPCS3.log");
    let mut tail = LogTail::new(path.clone());
    assert!(tail.lines().is_empty(), "no file yet");
    std::fs::write(&path, "one\ntw").unwrap();
    assert_eq!(tail.lines(), ["one"]);
    std::fs::write(&path, "one\ntwo\nthree\n").unwrap();
    assert_eq!(tail.lines(), ["two", "three"]);
    assert!(tail.lines().is_empty());
    std::fs::write(&path, "new\n").unwrap();
    assert_eq!(tail.lines(), ["new"], "a rewritten log starts over");
}

#[test]
#[ignore = "live: drives GUAC_ORACLE_EXE from GUAC_ORACLE_HOME into a level with GUAC_ORACLE_FIGURE on the portal (Mac)"]
fn the_oracle_reaches_a_level_with_a_hero_on_the_portal() {
    let Some(figure) = env::path(FIGURE) else {
        eprintln!("skip: set {FIGURE} to a .sky from the dev's own pack");
        return;
    };
    let Some(live) = common::live() else { return };
    let before = Snapshot::take(&live.dev_dirs).unwrap();

    let launched = Instant::now();
    let mut oracle = launch::boot(
        &live.exe,
        &live.home,
        &BootOptions {
            borderless: true,
            ..BootOptions::default()
        },
    )
    .unwrap();
    eprintln!("pid {} run {:?}", oracle.pid(), oracle.run_paths());
    let mut log = LogTail::new(live.home.cache_dir().join("RPCS3.log"));
    let state = oracle
        .wait_playable(common::secs("ORACLE_READY_SECS", 600))
        .unwrap();
    eprintln!(
        "playable at +{:.0} s: {state:?}",
        launched.elapsed().as_secs_f64()
    );

    let copy = oracle.run_paths().tmp.join("figure.sky");
    std::fs::copy(&figure, &copy).unwrap();
    let slot = SlotIndex::new(0).unwrap();
    let deadline = Instant::now() + common::secs("ORACLE_LEVEL_SECS", 600);
    let (mut level, mut placed, mut placements, mut presses) = (None, None::<Instant>, 0, 0);
    let hero = loop {
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
            driver.load(slot, &copy).unwrap();
            placed = Some(Instant::now());
            placements += 1;
        }
        driver.press_button(PadButton::Cross, 120).unwrap();
        presses += 1;
        std::thread::sleep(Duration::from_millis(880));
    };
    let state = oracle.wait_for(Duration::from_secs(5), |_| true).unwrap();
    eprintln!(
        "+{:.0} s, frame {}: hero {hero} after {presses} CROSS presses and {placements} placement(s)",
        launched.elapsed().as_secs_f64(),
        state.frames
    );

    let fps = oracle
        .sample_fps(common::secs("ORACLE_SAMPLE_SECS", 30))
        .unwrap();
    eprintln!(
        "in the level: {} frames in {:.1} s = {:.1} fps",
        fps.frames,
        fps.seconds,
        fps.fps()
    );
    eprintln!("shutdown: {:?}", oracle.shutdown().unwrap());

    let changed = before.diff(&Snapshot::take(&live.dev_dirs).unwrap());
    assert!(
        changed.is_empty(),
        "the dev's RPCS3 dirs changed:\n{}",
        changed.join("\n")
    );
    eprintln!("the dev's RPCS3 dirs: {} entries, unchanged", before.len());
}
