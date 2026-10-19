//! Live (openspec AF.7): O4's frame-locked input in a level, with nobody at the
//! Mac. A gameplay savestate (AF.5's, Eruptor in chapter one's clearing) boots
//! to Start Paused's pause, gets `RECONNECT` and a fresh copy of the figure, and
//! then:
//!
//! - a script with entries at offsets N and N+k on both ports, read back with
//!   `PAD_STATE` flip by flip: the guest's first read at or after flip N
//!   carries N's state on port 0 and on port 1, every read of a flip agrees
//!   with that flip's first, and port 1 is read at all (connected to the guest);
//! - recorded, not gated (AC.24 and AC.25 want it): an idle stretch, then the
//!   left stick held forward and then left, a `SCREENSHOT` before and after
//!   each, with the share of pixels that changed;
//! - START over `PAD` opens the pause menu, CROSS (South) pressed at an exact
//!   flip steps it into Collections, CIRCLE (East) backs out, then P6's
//!   `BUTTON_PRESS CROSS` takes the same step: the two stepped screens match
//!   where a step changes half the frame, and `PAD_STATE` shows which flips
//!   each press held.
//!
//! Everything lands in `$GUAC_CAPTURE_DIR/BLUS30968/af7-pad-<unix time>/`
//! (refused inside any work tree), `run.txt` opening on the identity line. Mac,
//! against `GUAC_ORACLE_EXE` from `GUAC_ORACLE_HOME`; reads `GUAC_GIANTS_DUMP`,
//! `GUAC_ORACLE_FIGURE`, `GUAC_CAPTURE_DIR` and `GUAC_ORACLE_SAVESTATE` (here a
//! gameplay state this oracle build accepts, AF.5's `SAVESTATE`) from the env
//! or `.env.dev`, and skips when one is unset. A state boots only in a home
//! holding its trophy files (`dev_hdd0/home/00000001/trophy/NPWR03657_00`, from
//! the home it was taken in): RPCS3 reloads them by path and dies without.
//!
//! `cargo test -p oracle-harness --test live_pad -- --ignored --nocapture`

#![cfg(unix)]

mod common;

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use common::{Record, hid_idle, load_average, screen_lock};
use guac_oracle::input::{Button, Ds3Pad, InputScript};
use oracle_harness::control::{Control, Shot};
use oracle_harness::env;
use oracle_harness::identity::Identity;
use oracle_harness::launch::{self, BootOptions};
use oracle_harness::snapshot::Snapshot;
use skylander_core::SlotIndex;
use skylander_rpcs3_control::PortalDriver;
use skylander_rpcs3_control::ipc::proto::PadButton;

const FIGURE: &str = "GUAC_ORACLE_FIGURE";
const STATE: &str = "GUAC_ORACLE_SAVESTATE";
const WAIT: Duration = Duration::from_secs(120);
/// A pixel counts as changed past this on any channel (PNG noise is 0; a
/// menu's light rays move a few percent of the frame).
const CHANGED: u8 = 32;

/// The readback script: entries at offsets `N` and `N + K` on both ports.
const N: u64 = 60;
const K: u64 = 30;
const READBACK: &str = r#"(
    steps: [
        (at: 60, pads: {
            0: (buttons: [RightTrigger2], right: (x: 200, y: 60)),
            1: (buttons: [LeftTrigger2], left: (x: 30, y: 220)),
        }),
        (at: 90, pads: {0: (), 1: (left: (x: 30, y: 220))}),
    ],
)"#;

/// 7 flips, about what P6's 120 ms press holds at 60 fps, so the two compare.
fn press(button: Button, at: u64) -> InputScript {
    InputScript::from_ron(&format!(
        "(steps: [(at: {at}, pads: {{0: (buttons: [{button:?}]), 1: ()}}), (at: {}, pads: {{0: ()}})])",
        at + 7
    ))
    .unwrap()
}

fn hold_left(x: u8, y: u8, flips: u64) -> InputScript {
    InputScript::from_ron(&format!(
        "(steps: [(at: 0, pads: {{0: (left: (x: {x}, y: {y})), 1: ()}}), (at: {flips}, pads: {{0: ()}})])"
    ))
    .unwrap()
}

/// How much of the frame moved: a menu step moves about half of it, a menu's
/// light rays a few percent.
fn changed(a: &Shot, b: &Shot) -> f64 {
    let a = image::open(&a.path).unwrap().to_rgb8();
    let b = image::open(&b.path).unwrap().to_rgb8();
    assert_eq!(a.dimensions(), b.dimensions());
    let moved = a
        .pixels()
        .zip(b.pixels())
        .filter(|(p, q)| p.0.iter().zip(q.0).any(|(x, y)| x.abs_diff(y) > CHANGED))
        .count();
    moved as f64 / f64::from(a.width() * a.height())
}

fn shot_line(label: &str, shot: &Shot) -> String {
    format!(
        "{label}: {} flip={} lit {:.1}% pixels {} [{}, {}]",
        shot.path.file_name().unwrap().to_string_lossy(),
        shot.flip,
        shot.frame.lit_share() * 100.0,
        &shot.frame.pixels_sha256[..16],
        screen_lock(),
        hid_idle()
    )
}

fn run_script(control: &Control, script: &InputScript, flips: u64) -> u64 {
    let loaded = control.load_script(script).unwrap();
    let end = loaded.base + flips;
    control.pause_at(end, WAIT).unwrap();
    loaded.base
}

/// Which flips a press held, as the guest read them rather than as it was sent.
fn south_flips(control: &Control, range: std::ops::Range<u64>) -> Vec<u64> {
    range
        .filter(|&f| {
            control
                .pad_state(0, Some(f))
                .unwrap()
                .is_some_and(|r| r.pad.state().buttons.contains(&Button::South))
        })
        .collect()
}

#[test]
#[ignore = "live: drives GUAC_ORACLE_EXE's PAD, PAD_CLEAR and PAD_STATE from a gameplay GUAC_ORACLE_SAVESTATE, outputs to GUAC_CAPTURE_DIR (Mac)"]
fn frame_locked_input_reaches_the_guest_on_both_ports_and_steps_a_menu_like_p6() {
    let (Some(figure), Some(state), Some(capture_root)) = (
        env::path(FIGURE),
        env::path(STATE),
        env::path(env::CAPTURE_DIR),
    ) else {
        eprintln!(
            "skip: set {FIGURE}, {STATE} (a gameplay state) and {} (outside the repo)",
            env::CAPTURE_DIR
        );
        return;
    };
    let Some(live) = common::live() else { return };
    let before = Snapshot::take(&live.dev_dirs).unwrap();
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs();
    let out: PathBuf = rrc_dump::output::refuse_repo_output(
        &capture_root
            .join("BLUS30968")
            .join(format!("af7-pad-{stamp}")),
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
        "start: {}; {}; load average {}",
        screen_lock(),
        hid_idle(),
        load_average()
    ));

    // ---- the gameplay state, booted --------------------------------------
    let launched = Instant::now();
    let mut oracle = launch::boot(
        &live.exe,
        &live.home,
        &BootOptions {
            savestate: Some(state.clone()),
            borderless: true,
            ..BootOptions::default()
        },
    )
    .unwrap();
    let held = oracle
        .wait_paused(common::secs("ORACLE_READY_SECS", 600))
        .unwrap();
    rec.line(format!(
        "--savestate {} boot: Start Paused at frame {} after {:.1} s",
        state.file_name().unwrap().to_string_lossy(),
        held.frames,
        launched.elapsed().as_secs_f64()
    ));
    let control = oracle.control();
    control.driver().reconnect().unwrap();
    let copy = oracle.run_paths().tmp.join("figure.sky");
    std::fs::copy(&figure, &copy).unwrap();
    control
        .driver()
        .load(SlotIndex::new(0).unwrap(), &copy)
        .unwrap();
    for port in 0..2 {
        rec.line(format!(
            "port {port} before any script: {:?}",
            control.pad_state(port, None).unwrap()
        ));
    }

    // ---- entries at N and N+k on both ports, read back flip by flip --------
    let script = InputScript::from_ron(READBACK).unwrap();
    let loaded = control.load_script(&script).unwrap();
    let end = loaded.base + N + K + 30;
    control.pause_at(end, WAIT).unwrap();
    rec.line(format!(
        "readback script: {} entries loaded at the pause at frame {}, offset 0 on flip {}; \
         entries at flips {} and {}; paused at {end}",
        loaded.entries,
        loaded.paused_at.frames,
        loaded.base,
        loaded.base + N,
        loaded.base + N + K
    ));
    for port in 0..2u8 {
        let want = |flip: u64| {
            script
                .state_at(usize::from(port), flip - loaded.base)
                .map_or(Ds3Pad::REST, |s| s.ds3())
        };
        let (mut read_flips, mut unread, mut min, mut max, mut differ) = (0, 0, u32::MAX, 0, 0);
        let mut first_at_n = None;
        for flip in loaded.base..end {
            let Some(read) = control.pad_state(port, Some(flip)).unwrap() else {
                unread += 1;
                continue;
            };
            assert_eq!(read.pad, want(flip), "port {port} flip {flip}: {read:?}");
            read_flips += 1;
            min = min.min(read.reads);
            max = max.max(read.reads);
            differ += read.differ;
            if first_at_n.is_none() && flip >= loaded.base + N {
                first_at_n = Some(read);
            }
        }
        let first = first_at_n.unwrap_or_else(|| panic!("port {port}: no read at or after N"));
        assert_eq!(first.pad, want(loaded.base + N), "port {port}");
        assert_eq!(differ, 0, "port {port}: reads within a flip disagreed");
        assert!(read_flips > 0, "port {port}: the guest never read it");
        rec.line(format!(
            "port {port}: {read_flips} of {} flips read ({unread} without a read), {min}-{max} \
             reads a flip, {differ} reads off their flip's first; first read at or after flip {} \
             was flip {} with {:?}",
            end - loaded.base,
            loaded.base + N,
            first.flip,
            first.pad.state()
        ));
    }

    // ---- recorded, not gated: does a held left stick walk him? -----------
    // The fresh figure's arrival plays first; by flip 600 he's back in the clearing.
    control.pause_at(600.max(end + 1), WAIT).unwrap();
    let a = control.screenshot(&out, WAIT).unwrap();
    rec.line(shot_line("in the clearing", &a));
    let at = control.state().unwrap().frames;
    control.pause_at(at + 120, WAIT).unwrap();
    let b = control.screenshot(&out, WAIT).unwrap();
    rec.line(shot_line("120 flips idle", &b));
    run_script(&control, &hold_left(128, 0, 120), 150);
    let c = control.screenshot(&out, WAIT).unwrap();
    rec.line(shot_line(
        "left stick forward (128, 0) for 120 flips, 30 at rest",
        &c,
    ));
    run_script(&control, &hold_left(0, 128, 90), 120);
    let d = control.screenshot(&out, WAIT).unwrap();
    rec.line(shot_line(
        "left stick left (0, 128) for 90 flips, 30 at rest",
        &d,
    ));
    rec.line(format!(
        "pixels changed: idle {:.1}%, forward {:.1}%, left {:.1}% (look at the shots)",
        changed(&a, &b) * 100.0,
        changed(&b, &c) * 100.0,
        changed(&c, &d) * 100.0
    ));

    // ---- CROSS at an exact flip steps the pause menu as P6's does --------
    run_script(&control, &press(Button::Start, 0), 77);
    let menu = control.screenshot(&out, WAIT).unwrap();
    rec.line(shot_line("START over PAD: the pause menu", &menu));
    let base = run_script(&control, &press(Button::South, 30), 120);
    let pressed = south_flips(&control, base + 25..base + 45);
    let stepped = control.screenshot(&out, WAIT).unwrap();
    rec.line(shot_line(
        "South (CROSS) over PAD at flip base+30: Collections",
        &stepped,
    ));
    assert_eq!(
        pressed,
        (base + 30..base + 37).collect::<Vec<_>>(),
        "PAD's press held other flips"
    );
    run_script(&control, &press(Button::East, 0), 80);
    let back = control.screenshot(&out, WAIT).unwrap();
    rec.line(shot_line("East (CIRCLE) over PAD: back to the menu", &back));

    control.pad_clear().unwrap();
    let from = control.state().unwrap().frames;
    control.resume().unwrap();
    control
        .driver()
        .press_button(PadButton::Cross, 120)
        .unwrap();
    std::thread::sleep(Duration::from_millis(1500));
    let to = control.state().unwrap().frames + 30;
    control.pause_at(to, WAIT).unwrap();
    let p6 = control.screenshot(&out, WAIT).unwrap();
    rec.line(shot_line("P6 BUTTON_PRESS CROSS 120 after PAD_CLEAR", &p6));
    let p6_flips = south_flips(&control, from..to);
    rec.line(format!(
        "PAD held South on flips {}-{} (7, from the script); P6's 120 ms held it on {} flips, {:?}-{:?}",
        pressed[0],
        pressed[pressed.len() - 1],
        p6_flips.len(),
        p6_flips.first(),
        p6_flips.last()
    ));
    let (step_pad, step_p6) = (changed(&menu, &stepped), changed(&back, &p6));
    let (same_menu, same_step) = (changed(&menu, &back), changed(&stepped, &p6));
    rec.line(format!(
        "pixels changed: the PAD step {:.1}%, the P6 step {:.1}%; menu against menu {:.1}%, \
         PAD's Collections against P6's {:.1}%",
        step_pad * 100.0,
        step_p6 * 100.0,
        same_menu * 100.0,
        same_step * 100.0
    ));
    assert!(!p6_flips.is_empty(), "P6's press never reached a read");
    assert!(
        step_pad > 0.2 && step_p6 > 0.2,
        "a press didn't step the menu"
    );
    assert!(
        same_menu < 0.1 && same_step < 0.1,
        "PAD's step and P6's landed on different screens"
    );

    rec.line(format!("shutdown: {:?}", oracle.shutdown().unwrap()));
    keep_log(&live.home.cache_dir(), &out);
    let changed_dirs = before.diff(&Snapshot::take(&live.dev_dirs).unwrap());
    assert!(
        changed_dirs.is_empty(),
        "the dev's RPCS3 dirs changed:\n{}",
        changed_dirs.join("\n")
    );
    rec.line(format!(
        "end: {}; {}; load average {}; the dev's RPCS3 dirs, {} entries, unchanged",
        screen_lock(),
        hid_idle(),
        load_average(),
        before.len()
    ));
}

/// RPCS3's log of the run, kept beside its outputs (the next start rewrites it).
fn keep_log(cache_dir: &Path, out: &Path) {
    std::fs::copy(cache_dir.join("RPCS3.log"), out.join("RPCS3.log")).unwrap();
}
