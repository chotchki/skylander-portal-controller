//! Live (openspec AF.9): Giants' actors through PINE at `PAUSE_AT` pauses, in a
//! fresh gameplay savestate, with nobody at the Mac. AF.8's fresh state (or
//! whatever gameplay state `GUAC_ORACLE_SAVESTATE` names) boots to Start
//! Paused's pause, gets `RECONNECT`, and the frame is read twice there (equal).
//! Then, the way design decision 7's replay goes, a fresh copy of the figure on
//! the portal and a frame-locked idle script (both pads at rest):
//!
//! - the figure's arrival, a frame every 60 flips over 600, recorded;
//! - the hold: 10 more checkpoints 60 flips apart, across which the hero's
//!   position and heading may not move past float noise;
//! - then the walk script: the left stick forward for 90 flips, left for 90, at
//!   rest after, a frame every 30. Each steady 30-flip move has to cover ground
//!   along the way the stick points (forward: the camera's heading; left: 90
//!   degrees round from it, since the stick is read against the camera) with the
//!   hero facing it, and at rest he has to stop.
//!
//! Every checkpoint records the actor count (listed, read, skinned, players)
//! and checks each actor's `_xform` against its angles through `euler_matrix`.
//! A `SAVESTATE` at the walk's end leaves a state with the hero turned, for the
//! offline cross-check against the Edge job records (lead 1).
//!
//! Everything lands in `$GUAC_CAPTURE_DIR/BLUS30968/af9-actors-<unix time>/`
//! (refused inside any work tree), `run.txt` opening on the identity line; each
//! frame is written beside it as JSON, since it's guest memory. Mac, against
//! `GUAC_ORACLE_EXE` from `GUAC_ORACLE_HOME`; reads `GUAC_GIANTS_DUMP`,
//! `GUAC_ORACLE_FIGURE`, `GUAC_CAPTURE_DIR` and `GUAC_ORACLE_SAVESTATE` (a
//! gameplay state this build accepts, booted in a home holding its trophy
//! files, as `live_pad` needs) from the env or `.env.dev`, and skips when one is
//! unset.
//!
//! `cargo test -p oracle-harness --test live_actors -- --ignored --nocapture`

#![cfg(unix)]

mod common;

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use common::{Record, hid_idle, load_average, screen_lock};
use guac_oracle::input::InputScript;
use oracle_harness::actors::{self, Actor, ActorFrame, angle_from, wrap_deg};
use oracle_harness::env;
use oracle_harness::identity::Identity;
use oracle_harness::launch::{self, BootOptions};
use oracle_harness::pine::PineClient;
use oracle_harness::snapshot::Snapshot;
use skylander_core::SlotIndex;
use skylander_rpcs3_control::PortalDriver;

const FIGURE: &str = "GUAC_ORACLE_FIGURE";
const STATE: &str = "GUAC_ORACLE_SAVESTATE";
const WAIT: Duration = Duration::from_secs(120);
/// The figure's arrival after the `LOAD`, read every [`EVERY`] flips.
const ARRIVAL: u64 = 600;
const EVERY: u64 = 60;
const HOLD_CHECKPOINTS: u64 = 10;
/// Float noise for a hero standing still: 13 ulps of a coordinate near 100 m.
const HOLD_METRES: f64 = 1e-4;
const HOLD_DEGREES: f64 = 1e-3;
const IDLE: &str = "(steps: [(at: 0, pads: {0: (), 1: ()})])";
/// Forward (x 128, y 0) for 90 flips, then left (x 0, y 128) for 90, then rest.
const WALK: &str = "(steps: [
    (at: 0, pads: {0: (left: (x: 128, y: 0)), 1: ()}),
    (at: 90, pads: {0: (left: (x: 0, y: 128))}),
    (at: 180, pads: {0: ()}),
])";
const WALK_EVERY: u64 = 30;
const WALK_CHECKPOINTS: u64 = 8;
/// Heading error a steady move may have against the stick, degrees.
const WALK_DEGREES: f64 = 10.0;
/// Ground a steady 30-flip move has to cover (Eruptor's top speed, 21 ft/s,
/// covers about 3.2 m in 30 flips at 60 fps).
const WALK_METRES: f64 = 1.0;
/// What a hero at rest may still drift in 30 flips.
const REST_METRES: f64 = 0.05;
/// How far an actor's `_xform` may sit from `euler_matrix` of its angles (2.4e-7
/// in every savestate read offline).
const POSE_ERROR: f64 = 1e-5;

fn sha256(path: &Path) -> String {
    use sha2::Digest;
    let digest = sha2::Sha256::digest(std::fs::read(path).unwrap());
    digest.iter().map(|b| format!("{b:02x}")).collect()
}

fn read_at(pine: &mut PineClient, flip: u64, out: &Path) -> (ActorFrame, Duration) {
    let t = Instant::now();
    let mut frame = actors::read_frame(pine).unwrap();
    let took = t.elapsed();
    frame.flip = Some(flip);
    frame
        .write(&out.join(format!("actors-{flip:05}.json")))
        .unwrap();
    (frame, took)
}

fn hero(frame: &ActorFrame) -> Actor {
    frame
        .hero()
        .unwrap_or_else(|e| panic!("flip {:?}: {e}", frame.flip))
        .clone()
}

fn camera_heading(frame: &ActorFrame) -> f64 {
    f64::from(
        frame
            .camera
            .as_ref()
            .unwrap_or_else(|| panic!("flip {:?}: no camera", frame.flip))
            .heading(),
    )
}

/// The heading halfway between two, the short way round.
fn between(a: f64, b: f64) -> f64 {
    wrap_deg(a + angle_from(a, b) / 2.0)
}

/// The actor counts, and how many actors' `_xform` came out of their angles
/// (`euler_matrix`) and how closely; panics past [`POSE_ERROR`].
fn counts(frame: &ActorFrame) -> String {
    let c = frame.counts();
    let errors: Vec<f64> = frame.actors.iter().filter_map(Actor::pose_error).collect();
    let worst = errors.iter().copied().fold(0.0, f64::max);
    assert!(
        worst <= POSE_ERROR,
        "flip {:?}: an actor's matrix is {worst:.1e} off its angles",
        frame.flip
    );
    format!(
        "{} listed, {} read, {} skinned, {} player(s); {} matrices their angles' within {worst:.1e}",
        c.listed,
        c.actors,
        c.skinned,
        c.players,
        errors.len()
    )
}

#[test]
#[ignore = "live: boots a gameplay GUAC_ORACLE_SAVESTATE and reads Giants' actors over PINE under an idle and a walk script, outputs to GUAC_CAPTURE_DIR (Mac)"]
fn the_hero_holds_at_idle_and_walks_where_the_stick_points() {
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
    let figure_hash = sha256(&figure);
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs();
    let out: PathBuf = rrc_dump::output::refuse_repo_output(
        &capture_root
            .join("BLUS30968")
            .join(format!("af9-actors-{stamp}")),
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

    // ---- the state at its pause, read twice ---------------------------------
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
    let f0 = held.frames;
    rec.line(format!(
        "--savestate {} ({}) boot: Start Paused at frame {f0} after {:.1} s",
        state.file_name().unwrap().to_string_lossy(),
        &sha256(&state)[..16],
        launched.elapsed().as_secs_f64()
    ));
    let control = oracle.control();
    control.driver().reconnect().unwrap();
    let mut pine = common::pine_connect(&oracle.run_paths().pine_socket);
    let (saved, took) = read_at(&mut pine, f0, &out);
    let again = actors::read_frame(&mut pine).unwrap();
    assert_eq!(
        ActorFrame {
            flip: None,
            ..saved.clone()
        },
        again,
        "two reads at one pause differ"
    );
    rec.line(format!(
        "{} (read twice, equal; {} ms each)",
        saved.summary(),
        took.as_millis()
    ));
    rec.line(format!("  hero as saved: {}", hero(&saved).summary()));

    // ---- a fresh figure, the idle script, the arrival ------------------------
    let copy = oracle.run_paths().tmp.join("figure.sky");
    std::fs::copy(&figure, &copy).unwrap();
    control
        .driver()
        .load(SlotIndex::new(0).unwrap(), &copy)
        .unwrap();
    let loaded = control
        .load_script(&InputScript::from_ron(IDLE).unwrap())
        .unwrap();
    rec.line(format!(
        "LOAD of a fresh figure copy; idle script (both pads at rest), offset 0 on flip {}",
        loaded.base
    ));
    let mut reads = Vec::new();
    let mut prev = hero(&saved);
    for k in 1..=ARRIVAL / EVERY {
        let flip = f0 + EVERY * k;
        let paused = control.pause_at(flip, WAIT).unwrap();
        let (frame, took) = read_at(&mut pine, paused.frames, &out);
        reads.push(took);
        let now = hero(&frame);
        let step = prev.step_to(&now);
        rec.line(format!(
            "arrival, flip {flip} (game frame {}, dt {:.4} s): {}; moved {:.5} m at {:.2} deg, \
             rise {:+.5} m; {}",
            frame.game_frame,
            frame.frame_delta,
            now.summary(),
            step.distance,
            step.heading,
            step.rise,
            counts(&frame)
        ));
        prev = now;
    }

    // ---- the hold: 10 checkpoints, the hero standing --------------------------
    let h0 = f0 + ARRIVAL;
    let start = prev.clone();
    let (mut worst_m, mut worst_deg) = (0.0f64, 0.0f64);
    let mut last = None;
    for k in 1..=HOLD_CHECKPOINTS {
        let flip = h0 + EVERY * k;
        let paused = control.pause_at(flip, WAIT).unwrap();
        let (frame, took) = read_at(&mut pine, paused.frames, &out);
        reads.push(took);
        let now = hero(&frame);
        let (dm, dd) = (
            now.position_delta(&start),
            angle_from(f64::from(start.heading()), f64::from(now.heading())).abs(),
        );
        worst_m = worst_m.max(dm);
        worst_deg = worst_deg.max(dd);
        rec.line(format!(
            "hold, flip {flip} (game frame {}): {}; {dm:.2e} m and {dd:.2e} deg off flip {h0}'s; {}",
            frame.game_frame,
            now.summary(),
            counts(&frame)
        ));
        last = Some((frame, paused.frames));
    }
    rec.line(format!(
        "hold: over {HOLD_CHECKPOINTS} checkpoints the hero moved at most {worst_m:.2e} m and \
         turned at most {worst_deg:.2e} deg from flip {h0}"
    ));
    assert!(
        worst_m <= HOLD_METRES,
        "the idle hero moved {worst_m:.2e} m (bound {HOLD_METRES:.0e})"
    );
    assert!(
        worst_deg <= HOLD_DEGREES,
        "the idle hero turned {worst_deg:.2e} deg"
    );
    let shot = control.screenshot(&out, WAIT).unwrap();
    rec.line(format!(
        "shot at flip {}: {} [{}, {}]",
        shot.flip,
        shot.path.file_name().unwrap().to_string_lossy(),
        screen_lock(),
        hid_idle()
    ));

    // ---- the walk -------------------------------------------------------------
    let (mut prev_frame, _) = last.unwrap();
    let loaded = control
        .load_script(&InputScript::from_ron(WALK).unwrap())
        .unwrap();
    let base = loaded.base;
    rec.line(format!(
        "walk script (left stick forward 0-90, left 90-180, at rest after), offset 0 on flip {base}"
    ));
    for k in 1..=WALK_CHECKPOINTS {
        let offset = WALK_EVERY * k;
        let paused = control.pause_at(base + offset, WAIT).unwrap();
        let (frame, took) = read_at(&mut pine, paused.frames, &out);
        reads.push(took);
        let (was, now) = (hero(&prev_frame), hero(&frame));
        let step = was.step_to(&now);
        let cam = between(camera_heading(&prev_frame), camera_heading(&frame));
        // What the stick held over this stretch, and the heading it points.
        let (what, want) = match offset {
            0..=90 => ("forward", Some(cam)),
            91..=180 => ("left", Some(wrap_deg(cam + 90.0))),
            _ => ("at rest", None),
        };
        let off = want.map(|w| angle_from(w, step.heading));
        let facing = angle_from(step.heading, f64::from(now.heading()));
        rec.line(format!(
            "walk +{offset} (flip {}, game frame {}), {what}: {}; moved {:.4} m at {:.2} deg, \
             rise {:+.4} m; camera at {:.2} deg{}; facing {:+.2} deg off the move; {}",
            paused.frames,
            frame.game_frame,
            now.summary(),
            step.distance,
            step.heading,
            step.rise,
            cam,
            off.map_or(String::new(), |o| format!(
                ", the move {o:+.2} deg off the stick's {:.2}",
                want.unwrap()
            )),
            facing,
            counts(&frame)
        ));
        // The first stretch of each stick holds the turn and the speeding up.
        let steady = !matches!(offset, 30 | 120 | 210);
        match (want, steady) {
            (Some(_), true) => {
                assert!(
                    step.distance >= WALK_METRES,
                    "+{offset}: {what} moved only {:.3} m",
                    step.distance
                );
                assert!(
                    off.unwrap().abs() <= WALK_DEGREES,
                    "+{offset}: {what} moved at {:.2} deg, {:+.2} off the stick",
                    step.heading,
                    off.unwrap()
                );
                assert!(
                    facing.abs() <= WALK_DEGREES,
                    "+{offset}: the hero faces {:.2} deg, {facing:+.2} off his move",
                    now.heading()
                );
            }
            (None, true) => assert!(
                step.distance <= REST_METRES,
                "+{offset}: at rest he still moved {:.3} m",
                step.distance
            ),
            _ => {}
        }
        if offset == 90 {
            let shot = control.screenshot(&out, WAIT).unwrap();
            rec.line(format!(
                "shot at flip {}: {}",
                shot.flip,
                shot.path.file_name().unwrap().to_string_lossy()
            ));
        }
        prev_frame = frame;
    }
    let shot = control.screenshot(&out, WAIT).unwrap();
    rec.line(format!(
        "shot at flip {}: {} [{}, {}]",
        shot.flip,
        shot.path.file_name().unwrap().to_string_lossy(),
        screen_lock(),
        hid_idle()
    ));
    reads.sort();
    rec.line(format!(
        "{} frame reads: median {} ms, slowest {} ms",
        reads.len(),
        reads[reads.len() / 2].as_millis(),
        reads.last().unwrap().as_millis()
    ));

    // ---- a state with the hero turned, for the job-record cross-check ---------
    // RPCS3's PINE thread blocks reading its one client, and the stop a
    // savestate makes waits on that thread: a held connection hangs it.
    drop(pine);
    let saved = control.savestate(&out, WAIT).unwrap();
    control.driver().reconnect().unwrap();
    rec.line(format!(
        "SAVESTATE at frame {}: {} ({} bytes, {}), rebooted to {} frame {}; RECONNECT",
        saved.saved_at.frames,
        saved.path.file_name().unwrap().to_string_lossy(),
        std::fs::metadata(&saved.path).unwrap().len(),
        &sha256(&saved.path)[..16],
        saved.rebooted.status,
        saved.rebooted.frames
    ));

    rec.line(format!("shutdown: {:?}", oracle.shutdown().unwrap()));
    std::fs::copy(
        live.home.cache_dir().join("RPCS3.log"),
        out.join("RPCS3.log"),
    )
    .unwrap();
    assert_eq!(sha256(&figure), figure_hash, "the figure source changed");
    let changed_dirs = before.diff(&Snapshot::take(&live.dev_dirs).unwrap());
    assert!(
        changed_dirs.is_empty(),
        "the dev's RPCS3 dirs changed:\n{}",
        changed_dirs.join("\n")
    );
    rec.line(format!(
        "end: {}; {}; load average {}; the figure source unchanged ({}); the dev's RPCS3 dirs, \
         {} entries, unchanged",
        screen_lock(),
        hid_idle(),
        load_average(),
        &figure_hash[..16],
        before.len()
    ));
}
