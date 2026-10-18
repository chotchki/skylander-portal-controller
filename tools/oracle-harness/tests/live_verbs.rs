//! Live (openspec AF.5): the O-verbs drive AC.13's scenario with nobody at the
//! Mac (design decision 10). A cold boot from the oracle home, CROSS and a
//! figure `LOAD` until the hero's archive opens in a level (AF.4's route,
//! `common::reach_hero`), a few seconds of idle, then: `PAUSE_AT` a flip and
//! hold it; two `SCREENSHOT`s of that frame; `RSXCAP` and `RESUME` until the
//! capture lands and pauses, parsed with `rrc-dump`'s reader; a `SCREENSHOT` of
//! the captured frame; `SAVESTATE`, the reboot's Start Paused pause, `RECONNECT`
//! and a `SCREENSHOT` of the restored frame; `RESUME` and the run going on, a
//! second capture from the restored session; the figure `CLEAR`ed and `LOAD`ed
//! again after `RECONNECT`, with P4's portal events and before/after shots, and
//! a third capture there; shutdown. Then a second process boots the saved state
//! `--savestate`, gets the figure after `RECONNECT` and plays on, shot at an
//! exact flip. The dev's RPCS3 dirs are checked untouched at the end.
//!
//! Everything lands in `$GUAC_CAPTURE_DIR/BLUS30968/af5-verbs-<unix time>/`
//! (refused inside any work tree), with `run.txt` opening on the identity line.
//! Mac, against `GUAC_ORACLE_EXE` from `GUAC_ORACLE_HOME`; reads
//! `GUAC_GIANTS_DUMP`, `GUAC_ORACLE_FIGURE` and `GUAC_CAPTURE_DIR` too, from the
//! env or `.env.dev`, and skips when one is unset. `ORACLE_IDLE_SECS` (default
//! 10) is the idle before the first pause.
//!
//! `cargo test -p oracle-harness --test live_verbs -- --ignored --nocapture`

#![cfg(unix)]

mod common;

use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use oracle_harness::control::{Captured, Control, Shot};
use oracle_harness::env;
use oracle_harness::identity::Identity;
use oracle_harness::launch::{self, BootOptions};
use oracle_harness::rpcs3_log::LogTail;
use oracle_harness::snapshot::Snapshot;
use oracle_harness::verbs::{Refusal, ReplyError, Verb};
use skylander_core::{SlotIndex, SlotState};
use skylander_rpcs3_control::PortalDriver;
use skylander_rpcs3_control::ipc::IpcPortalDriver;

const FIGURE: &str = "GUAC_ORACLE_FIGURE";
const WAIT: Duration = Duration::from_secs(120);

/// `run.txt` in the output dir, each line echoed to stderr.
struct Record(std::fs::File);

impl Record {
    fn line(&mut self, text: impl AsRef<str>) {
        eprintln!("{}", text.as_ref());
        writeln!(self.0, "{}", text.as_ref()).unwrap();
    }
}

/// `"CGSSessionScreenIsLocked"=Yes` out of `ioreg`'s console-session entry, which
/// carries the key only while the screen is locked.
fn screen_lock() -> String {
    const KEY: &str = "\"CGSSessionScreenIsLocked\"=";
    std::process::Command::new("ioreg")
        .args(["-n", "Root", "-d1"])
        .output()
        .ok()
        .and_then(|o| {
            let text = String::from_utf8_lossy(&o.stdout).into_owned();
            let at = text.find(KEY)?;
            let value: String = text[at + KEY.len()..]
                .chars()
                .take_while(char::is_ascii_alphabetic)
                .collect();
            Some(format!("{KEY}{value}"))
        })
        .unwrap_or_else(|| "no CGSSessionScreenIsLocked key (unlocked)".into())
}

fn load_average() -> String {
    std::process::Command::new("sysctl")
        .args(["-n", "vm.loadavg"])
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_owned())
        .unwrap_or_default()
}

fn shot_line(label: &str, shot: &Shot) -> String {
    format!(
        "{label}: {} flip={} {}x{} lit {:.1}% pixels {}",
        shot.path.file_name().unwrap().to_string_lossy(),
        shot.flip,
        shot.frame.width,
        shot.frame.height,
        shot.frame.lit_share() * 100.0,
        &shot.frame.pixels_sha256[..16]
    )
}

/// The capture through `rrc-dump`'s reader and replay, its `--list` lines
/// written beside it; the summary line.
fn read_capture(label: &str, cap: &Captured, out: &Path) -> String {
    let bytes = std::fs::read(&cap.path).unwrap();
    let capture = rrc_dump::Capture::from_file_bytes(&bytes).unwrap();
    let mut list = String::new();
    let stats = rrc_dump::replay(&capture, |draw| -> anyhow::Result<()> {
        list.push_str(&rrc_dump::output::list_line(&draw));
        list.push('\n');
        Ok(())
    })
    .unwrap();
    let name = cap.path.file_name().unwrap().to_string_lossy();
    std::fs::write(out.join(format!("{name}.list.txt")), &list).unwrap();
    let constants = capture
        .reg_state
        .transform_constants
        .as_ref()
        .map_or("none".to_owned(), |c| c.len().to_string());
    format!(
        "{label}: {name} ({} bytes), armed at frame {} ({}), paused at frame {}; RRC v{}, \
         register state: {} transform-program words, {} registers, starting transform \
         constants {constants}; {} commands, {} memory blocks, {} data blobs; {} draws, \
         {} unresolved memory ids, {} orphan commands, {} stray ends",
        bytes.len(),
        cap.armed_at.frames,
        cap.armed_at.status,
        cap.paused_at.frames,
        capture.version,
        capture.reg_state.transform_program.len(),
        capture.reg_state.registers.len(),
        stats.commands,
        capture.blocks.len(),
        capture.data.len(),
        stats.draws,
        stats.unresolved_blocks,
        stats.orphan_commands,
        stats.stray_ends,
    )
}

fn refusal(result: anyhow::Result<oracle_harness::verbs::Reply>) -> Refusal {
    let err = result.expect_err("refused");
    err.downcast_ref::<ReplyError>()
        .and_then(ReplyError::refusal)
        .unwrap_or_else(|| panic!("not a refusal: {err:#}"))
}

/// RPCS3's log of the boot that just ended, kept beside its outputs (the next
/// start rewrites it).
fn keep_log(cache_dir: &Path, out: &Path, boot: &str) {
    std::fs::copy(
        cache_dir.join("RPCS3.log"),
        out.join(format!("{boot}-RPCS3.log")),
    )
    .unwrap();
}

/// A fresh copy of the figure in the run dir: the game writes the one it reads.
fn fresh_figure(figure: &Path, run_tmp: &Path, name: &str) -> PathBuf {
    let copy = run_tmp.join(name);
    std::fs::copy(figure, &copy).unwrap();
    copy
}

#[test]
#[ignore = "live: drives GUAC_ORACLE_EXE through PAUSE_AT, SCREENSHOT, RSXCAP, SAVESTATE and RESUME in a level, outputs to GUAC_CAPTURE_DIR (Mac)"]
fn the_o_verbs_run_ac13s_scenario_unattended() {
    let Some(figure) = env::path(FIGURE) else {
        eprintln!("skip: set {FIGURE} to a .sky from the dev's own pack");
        return;
    };
    let Some(capture_root) = env::path(env::CAPTURE_DIR) else {
        eprintln!("skip: set {} (outside the repo)", env::CAPTURE_DIR);
        return;
    };
    let Some(live) = common::live() else { return };
    let before = Snapshot::take(&live.dev_dirs).unwrap();
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs();
    let out = rrc_dump::output::refuse_repo_output(
        &capture_root
            .join("BLUS30968")
            .join(format!("af5-verbs-{stamp}")),
    )
    .unwrap();
    std::fs::create_dir_all(&out).unwrap();
    let mut rec = Record(std::fs::File::create(out.join("run.txt")).unwrap());
    rec.line(
        Identity::of(&live.exe, &live.home.config_dir())
            .unwrap()
            .line(),
    );
    rec.line(format!(
        "start: {}; load average {}",
        screen_lock(),
        load_average()
    ));
    let slot = SlotIndex::new(0).unwrap();

    // ---- cold boot to a hero in a level ----------------------------------
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
    rec.line(format!("inherited render env: {:?}", oracle.render_env()));
    let mut log = LogTail::new(live.home.cache_dir().join("RPCS3.log"));
    let state = oracle
        .wait_playable(common::secs("ORACLE_READY_SECS", 600))
        .unwrap();
    rec.line(format!(
        "playable at +{:.0} s, frame {}",
        launched.elapsed().as_secs_f64(),
        state.frames
    ));
    let hero = common::reach_hero(&mut oracle, &mut log, &figure, launched);
    rec.line(format!(
        "+{:.0} s, frame {}: {} then {} after {} CROSS presses and {} placement(s)",
        launched.elapsed().as_secs_f64(),
        hero.state.frames,
        hero.level,
        hero.archive,
        hero.presses,
        hero.placements
    ));
    std::thread::sleep(common::secs("ORACLE_IDLE_SECS", 10));
    let control: Control = oracle.control();

    // ---- PAUSE_AT, held ---------------------------------------------------
    let now = control.state().unwrap();
    let target = now.frames + 120;
    let paused = control.pause_at(target, WAIT).unwrap();
    std::thread::sleep(Duration::from_secs(2));
    let held = control.state().unwrap();
    assert_eq!((held.status.as_str(), held.frames), ("paused", target));
    assert_eq!(
        refusal(control.verbs().send(Verb::PauseAt(target))),
        Refusal::Past
    );
    rec.line(format!(
        "PAUSE_AT {target}, sent at frame {}: STATE {} frames={}, the same 2 s later; PAUSE_AT {target} again -> ERR past",
        now.frames, paused.status, paused.frames
    ));

    // ---- two SCREENSHOTs of the paused frame -----------------------------
    let shot1 = control.screenshot(&out, WAIT).unwrap();
    let shot2 = control.screenshot(&out, WAIT).unwrap();
    rec.line(shot_line("SCREENSHOT at the pause", &shot1));
    rec.line(shot_line("SCREENSHOT again", &shot2));
    assert_eq!((shot1.flip, shot2.flip), (target, target));
    assert_eq!(
        shot1.frame.pixels_sha256, shot2.frame.pixels_sha256,
        "one paused frame, two different images"
    );
    assert!(
        shot1.frame.lit_share() > 0.05,
        "the paused frame is black: {:?}",
        shot1.frame
    );
    assert_eq!(control.state().unwrap().frames, target, "a shot moved it");

    // ---- RSXCAP, RESUME, the capture's pause -----------------------------
    let cap1 = control.capture(&out, WAIT).unwrap();
    rec.line(read_capture("RSXCAP (cold session)", &cap1, &out));
    // Armed at a pause at n, the capture's pause reads n+1 (the fake's model).
    assert_eq!(cap1.paused_at.frames, cap1.armed_at.frames + 1);
    let shot3 = control.screenshot(&out, WAIT).unwrap();
    rec.line(shot_line("SCREENSHOT at the capture's pause", &shot3));
    assert_eq!(shot3.flip, cap1.paused_at.frames);

    // ---- SAVESTATE, reboot, RECONNECT ------------------------------------
    let t = Instant::now();
    let saved = control.savestate(&out, Duration::from_secs(600)).unwrap();
    let head = std::fs::read(&saved.path).unwrap();
    rec.line(format!(
        "SAVESTATE at frame {}: {} ({} bytes, starts {:02x?}), rebooted to {} frames={} in {:.1} s",
        saved.saved_at.frames,
        saved.path.file_name().unwrap().to_string_lossy(),
        head.len(),
        &head[..4],
        saved.rebooted.status,
        saved.rebooted.frames,
        t.elapsed().as_secs_f64()
    ));
    assert_eq!(
        saved.rebooted.frames, 2,
        "Start Paused holds a state boot 2 flips in"
    );
    control.driver().reconnect().unwrap();
    let slots = control.driver().read_slots().unwrap();
    rec.line(format!("RECONNECT; portal: {slots:?}"));
    let shot4 = control.screenshot(&out, WAIT).unwrap();
    rec.line(shot_line("SCREENSHOT of the restored frame", &shot4));
    assert_eq!(shot4.flip, 2);

    control.resume().unwrap();
    let going = control
        .wait_state(WAIT, |s| s.status == "running" && s.frames >= 240)
        .unwrap();
    rec.line(format!("RESUME: running on, frame {}", going.frames));
    let cap2 = control.capture(&out, WAIT).unwrap();
    rec.line(read_capture("RSXCAP (restored session)", &cap2, &out));
    control.resume().unwrap();

    // ---- the figure after RECONNECT: CLEAR, then LOAD a fresh copy --------
    let events: Arc<Mutex<Vec<(Duration, String)>>> = Arc::default();
    let watch = {
        let (events, socket) = (Arc::clone(&events), oracle.run_paths().p1_socket.clone());
        std::thread::spawn(move || {
            IpcPortalDriver::with_path(socket).watch_events(Duration::from_secs(60), |at, line| {
                if line.starts_with("PE ") {
                    events.lock().unwrap().push((at, line.to_owned()));
                }
            })
        })
    };
    let watch_start = Instant::now();
    std::thread::sleep(Duration::from_secs(3));
    if matches!(slots[0], SlotState::Loaded { .. }) {
        control.driver().clear(slot).unwrap();
        rec.line("CLEAR 0");
    }
    std::thread::sleep(Duration::from_secs(8));
    let shot5 = control.screenshot(&out, WAIT).unwrap();
    rec.line(shot_line("SCREENSHOT with the portal empty", &shot5));
    let copy = fresh_figure(&figure, &oracle.run_paths().tmp, "figure-2.sky");
    let loaded_at = watch_start.elapsed();
    control.driver().load(slot, &copy).unwrap();
    rec.line(format!(
        "LOAD a fresh copy at +{:.1} s",
        loaded_at.as_secs_f64()
    ));
    std::thread::sleep(Duration::from_secs(12));
    let shot6 = control.screenshot(&out, WAIT).unwrap();
    rec.line(shot_line("SCREENSHOT 12 s after the LOAD", &shot6));
    // The figure's arrival plays out, then the hero is back in the level.
    std::thread::sleep(Duration::from_secs(18));
    let at = control.state().unwrap().frames + 60;
    control.pause_at(at, WAIT).unwrap();
    let shot7 = control.screenshot(&out, WAIT).unwrap();
    rec.line(shot_line(
        "SCREENSHOT 30 s after the LOAD, at PAUSE_AT",
        &shot7,
    ));
    assert_eq!(shot7.flip, at);
    let cap3 = control.capture(&out, WAIT).unwrap();
    rec.line(read_capture("RSXCAP (after the reload)", &cap3, &out));
    assert_eq!(cap3.paused_at.frames, cap3.armed_at.frames + 1);
    control.resume().unwrap();
    watch.join().unwrap().unwrap();
    let events = events.lock().unwrap();
    let after: Vec<&str> = events
        .iter()
        .filter(|(t, _)| *t >= loaded_at)
        .map(|(_, l)| l.as_str())
        .collect();
    let queries = after.iter().filter(|l| **l == "PE cmd=query").count();
    rec.line(format!(
        "P4 after the LOAD: {} portal events, {queries} PE cmd=query (the game reading the figure); before it: {}",
        after.len(),
        events.len() - after.len()
    ));
    assert!(queries > 0, "the game never read the reloaded figure");
    rec.line(format!("shutdown: {:?}", oracle.shutdown().unwrap()));
    keep_log(&live.home.cache_dir(), &out, "boot1");

    // ---- the saved state, booted --savestate -----------------------------
    let launched = Instant::now();
    let mut again = launch::boot(
        &live.exe,
        &live.home,
        &BootOptions {
            savestate: Some(saved.path.clone()),
            borderless: true,
            ..BootOptions::default()
        },
    )
    .unwrap();
    let held = again
        .wait_paused(common::secs("ORACLE_READY_SECS", 600))
        .unwrap();
    rec.line(format!(
        "--savestate boot: Start Paused at frame {} after {:.1} s",
        held.frames,
        launched.elapsed().as_secs_f64()
    ));
    let control = again.control();
    control.driver().reconnect().unwrap();
    let copy = fresh_figure(&figure, &again.run_paths().tmp, "figure.sky");
    control.driver().load(slot, &copy).unwrap();
    control.resume().unwrap();
    let playable = again
        .wait_for(WAIT, |s| s.is_playable() && s.frames > held.frames + 60)
        .unwrap();
    rec.line(format!(
        "RECONNECT, LOAD, RESUME: playable ({} progr={}/{} frames={})",
        playable.status, playable.progr_done, playable.progr_total, playable.frames
    ));
    std::thread::sleep(Duration::from_secs(10));
    let at = control.state().unwrap().frames + 60;
    control.pause_at(at, WAIT).unwrap();
    let shot8 = control.screenshot(&out, WAIT).unwrap();
    rec.line(shot_line("SCREENSHOT from the state's own boot", &shot8));
    assert_eq!(shot8.flip, at);
    control.resume().unwrap();
    let fps = again.sample_fps(Duration::from_secs(10)).unwrap();
    rec.line(format!(
        "{} frames in {:.1} s, {:.1} fps",
        fps.frames,
        fps.seconds,
        fps.fps()
    ));
    rec.line(format!("shutdown: {:?}", again.shutdown().unwrap()));
    keep_log(&live.home.cache_dir(), &out, "boot2");

    let changed = before.diff(&Snapshot::take(&live.dev_dirs).unwrap());
    assert!(
        changed.is_empty(),
        "the dev's RPCS3 dirs changed:\n{}",
        changed.join("\n")
    );
    rec.line(format!(
        "end: {}; load average {}; the dev's RPCS3 dirs, {} entries, unchanged",
        screen_lock(),
        load_average(),
        before.len()
    ));
}
