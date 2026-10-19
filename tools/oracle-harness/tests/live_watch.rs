//! Live (openspec AG.1): O5 records a walk every frame without a pause. AF.8's
//! fresh state (or whatever gameplay state `GUAC_ORACLE_SAVESTATE` names)
//! boots to Start Paused's pause (F0), gets `RECONNECT` and a fresh copy of the
//! figure, and there Giants' full watch list is resolved through PINE and
//! armed. AF.11's walk script ([`oracle_harness::divergence::WALK`]) loads with
//! offset 0 on F0 + 1 and the run goes to F0 + 600 with no pause on the way.
//!
//! Then the ring has to hold one record a flip, every range readable, with
//! `_curFrame` stepping by one a flip; O4's readback is joined to it per flip.
//! What's measured and recorded, not gated past float noise: each position
//! step against velocity times the clamped step under both alignments (the
//! step read in the earlier record or the later one) and both integration
//! orders (the velocity before or after the step), on open ground and over the
//! whole walk; the game's own `_timeStamp` on the hero against the same steps;
//! the stick floats on each leg; the clip at the top of the blend on each leg
//! and whether its time follows the steps; the last record against PINE's
//! read at the end pause. On open ground one alignment and order has to fit
//! within [`FLOAT_NOISE`].
//!
//! Everything lands in `$GUAC_CAPTURE_DIR/BLUS30968/ag1-watch-<unix time>/`
//! (refused inside any work tree): `run.txt` opening on the identity line, the
//! list, the ring raw (`watch.json`), the recording and the two actor frames,
//! since all of it is guest memory. Mac, against `GUAC_ORACLE_EXE` from
//! `GUAC_ORACLE_HOME`; reads `GUAC_GIANTS_DUMP`, `GUAC_ORACLE_FIGURE`,
//! `GUAC_CAPTURE_DIR` and `GUAC_ORACLE_SAVESTATE` from the env or `.env.dev`,
//! and skips when one is unset.
//!
//! `cargo test -p oracle-harness --test live_watch -- --ignored --nocapture`

#![cfg(unix)]

mod common;

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use common::{Record, hid_idle, load_average, screen_lock};
use guac_oracle::input::InputScript;
use guac_oracle::recording::{Alignment, Order, Recording};
use oracle_harness::actors::{self, ActorFrame};
use oracle_harness::divergence::{RECORDING_FILE, WALK};
use oracle_harness::env;
use oracle_harness::identity::Identity;
use oracle_harness::launch::{self, BootOptions};
use oracle_harness::snapshot::Snapshot;
use oracle_harness::watch::{GiantsWatch, RawRecord, RingCheck, WatchFile};
use skylander_core::SlotIndex;
use skylander_rpcs3_control::PortalDriver;

const FIGURE: &str = "GUAC_ORACLE_FIGURE";
const STATE: &str = "GUAC_ORACLE_SAVESTATE";
const WAIT: Duration = Duration::from_secs(180);
/// The walk's length in flips, recorded without a pause.
const FLIPS: u64 = 600;
/// The ring: the walk with room to spare.
const RING: u32 = 1_024;
/// A position step's miss on open ground that's float noise: positions near
/// 10-25 m are f32 with a 1.9e-6 m ulp, and a step adds a few of them.
const FLOAT_NOISE: f64 = 1e-4;
/// Level ground, by the hero's tilt (he tilts with the ground): a step on a
/// 0.5 deg slope shortens on the ground by 4e-5 of itself, 4 µm at a step's
/// 0.11 m, under float noise.
const LEVEL_TILT: f32 = 0.5;

fn sha256(path: &Path) -> String {
    use sha2::Digest;
    let digest = sha2::Sha256::digest(std::fs::read(path).unwrap());
    digest.iter().map(|b| format!("{b:02x}")).collect()
}

fn uptime() -> String {
    std::process::Command::new("uptime")
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_owned())
        .unwrap_or_default()
}

/// The residual grid over the flips `keep` accepts, one line each, best first.
fn residual_lines(
    r: &Recording,
    label: &str,
    keep: impl Fn(u64) -> bool + Copy,
) -> Vec<(f64, String)> {
    let mut lines = Vec::new();
    for alignment in Alignment::ALL {
        for order in Order::ALL {
            let x = r.residuals(alignment, order, keep);
            lines.push((
                x.max_ground,
                format!(
                    "  {label}: {alignment:?} step, {order:?}: {} pairs, ground miss max {:.3e} m \
                     (flip {}) rms {:.3e} m, height miss max {:.3e} m",
                    x.pairs,
                    x.max_ground,
                    x.worst_flip.map_or("-".into(), |f| f.to_string()),
                    x.rms_ground,
                    x.max_height
                ),
            ));
        }
    }
    lines.sort_by(|a, b| a.0.total_cmp(&b.0));
    lines
}

#[test]
#[ignore = "live: boots a gameplay GUAC_ORACLE_SAVESTATE, arms O5 on Giants' watch list and records a 600-flip walk without a pause, outputs to GUAC_CAPTURE_DIR (Mac)"]
fn a_walk_is_recorded_every_flip_without_a_pause() {
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
            .join(format!("ag1-watch-{stamp}")),
    )
    .unwrap();
    std::fs::create_dir_all(&out).unwrap();
    let mut rec = Record(std::fs::File::create(out.join("run.txt")).unwrap());
    let identity = Identity::of(&live.exe, &live.home.config_dir())
        .unwrap()
        .line();
    rec.line(&identity);
    rec.line(format!(
        "start: {}; {}; load average {}; uptime: {}",
        screen_lock(),
        hid_idle(),
        load_average(),
        uptime()
    ));

    // ---- F0: the state's pause, a fresh figure, the list armed ---------------
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
    let copy = oracle.run_paths().tmp.join("figure.sky");
    std::fs::copy(&figure, &copy).unwrap();
    control
        .driver()
        .load(SlotIndex::new(0).unwrap(), &copy)
        .unwrap();
    let mut pine = common::pine_connect(&oracle.run_paths().pine_socket);
    let mut at_f0: ActorFrame = actors::read_frame(&mut pine).unwrap();
    at_f0.flip = Some(f0);
    at_f0.write(&out.join("actors-f0.json")).unwrap();
    let t = Instant::now();
    let watch = GiantsWatch::resolve(&mut pine).unwrap();
    let resolve_ms = t.elapsed().as_secs_f64() * 1e3;
    std::fs::write(
        out.join("list.json"),
        serde_json::to_string_pretty(&watch).unwrap(),
    )
    .unwrap();
    rec.line(format!(
        "LOAD of a fresh figure copy; the list resolved in {resolve_ms:.0} ms: {} fields in {} \
         ranges, {} bytes a record; hero {} (corpus {:#010x}, placement {:#010x}), controller \
         {:#010x}, parameters {:#010x}, viewport {:?}, logic scale {:#010x}",
        watch.list.fields().len(),
        watch.list.ranges().len(),
        watch.list.bytes(),
        watch.hero_name.as_deref().unwrap_or("?"),
        watch.hero_corpus,
        watch.hero_placement,
        watch.controller,
        watch.parameters,
        watch.viewport.map(|v| format!("{v:#010x}")),
        watch.logic_scale
    ));
    for (i, clip) in watch.clips.iter().enumerate() {
        rec.line(format!(
            "  clip {i}: id {:#010x} {}",
            clip.id,
            clip.name.as_deref().unwrap_or("?")
        ));
    }
    control.watch_arm(&watch.list, RING).unwrap();
    let script = InputScript::from_ron(WALK).unwrap();
    let loaded = control.load_script(&script).unwrap();
    rec.line(format!(
        "armed a ring of {RING} records; AF.11's walk script, offset 0 on flip {}",
        loaded.base
    ));

    // ---- the walk, no pause until its end -------------------------------------
    let t = Instant::now();
    let end = control.pause_at(f0 + FLIPS, WAIT).unwrap();
    let ran = t.elapsed();
    let mut at_end: ActorFrame = actors::read_frame(&mut pine).unwrap();
    at_end.flip = Some(end.frames);
    at_end.write(&out.join("actors-end.json")).unwrap();
    // A stop waits on PINE's thread, which blocks on an open client (AF.8).
    drop(pine);
    let t = Instant::now();
    let ring = control.watch_read().unwrap();
    let flips: Vec<u64> = ring.records.iter().map(|r| r.flip).collect();
    let pads: BTreeMap<u64, _> = control
        .pad_reads(0, &flips)
        .unwrap()
        .into_iter()
        .zip(&flips)
        .filter_map(|(read, &flip)| read.map(|r| (flip, r.pad)))
        .collect();
    let read_ms = t.elapsed().as_secs_f64() * 1e3;
    let file = WatchFile {
        identity: identity.clone(),
        watch: watch.clone(),
        held: ring.summary.held,
        total: ring.summary.total,
        frames: ring.summary.frames,
        armed: ring.summary.armed,
        records: ring.records.iter().map(RawRecord::of).collect(),
        pads,
    };
    file.write(&out.join(WatchFile::FILE)).unwrap();
    let recording = file.recording().unwrap();
    std::fs::write(
        out.join(RECORDING_FILE),
        serde_json::to_string(&recording).unwrap(),
    )
    .unwrap();
    let check = RingCheck::of(&recording, &ring.records);
    rec.line(format!(
        "paused at {} after {:.2} s running; the ring read and the readback joined in \
         {read_ms:.0} ms: {} held of {} taken (armed at {}), {} readbacks; {check:?}",
        end.frames,
        ran.as_secs_f64(),
        ring.summary.held,
        ring.summary.total,
        ring.summary.armed,
        file.pads.len()
    ));
    let frames = recording.frames();

    // ---- one record a flip, _curFrame a step a flip -------------------------
    assert!(check.holds(), "{check:?}");
    assert_eq!(check.last_flip, Some(f0 + FLIPS - 1), "{check:?}");
    assert!(
        matches!(check.first_flip, Some(f) if f == f0 || f == f0 + 1),
        "{check:?}"
    );
    assert_eq!(file.pads.len(), frames.len(), "a flip without a readback");

    // ---- the walk's legs, keyed by flip ----------------------------------------
    let base = loaded.base;
    let leg = |from: u64, to: u64| move |f: u64| f >= base + from && f < base + to;
    let steps = recording.steps();
    let hitches: Vec<String> = frames
        .iter()
        .zip(&steps)
        .filter(|(f, _)| f.clock.frame_delta > f.clock.presentation_rate * 1.5)
        .map(|(f, s)| format!("{} ({:.4} s, step {s:.4})", f.flip, f.clock.frame_delta))
        .collect();
    let (min_d, max_d) = frames.iter().fold((f32::MAX, 0f32), |(lo, hi), f| {
        (lo.min(f.clock.frame_delta), hi.max(f.clock.frame_delta))
    });
    rec.line(format!(
        "frame time {min_d:.4}-{max_d:.4} s, nominal {:.7}; logic scale {:?}, stopped {:?}, \
         object scale {:?}; {} frames over 1.5 nominal: {}",
        frames[0].clock.presentation_rate,
        frames
            .iter()
            .map(|f| f.clock.logic_scale)
            .fold(Vec::<f32>::new(), |mut v, s| {
                if !v.contains(&s) {
                    v.push(s);
                }
                v
            }),
        frames.iter().any(|f| f.clock.logic_stopped),
        frames
            .iter()
            .map(|f| f.clock.object_scale)
            .fold(Vec::<f32>::new(), |mut v, s| {
                if !v.contains(&s) {
                    v.push(s);
                }
                v
            }),
        hitches.len(),
        hitches.join(", ")
    ));
    for alignment in Alignment::ALL {
        let t = recording
            .game_time(alignment)
            .expect("no game frame lost: the ring check held above");
        rec.line(format!(
            "game time under {alignment:?}: {:.4} s over {} records ({} flips at 1/60 s would \
             be {:.4} s)",
            t.last().unwrap(),
            t.len(),
            t.len() - 1,
            (t.len() - 1) as f64 / 60.0
        ));
    }

    // ---- the position steps against velocity times step ---------------------
    let windows: [(&str, u64, u64); 5] = [
        ("from rest, forward (offsets 60-149)", 60, 150),
        ("the first 12 flips from rest (60-71)", 60, 72),
        ("left, before the corner (150-169)", 150, 170),
        ("at rest after (240-299)", 240, 300),
        ("the whole walk", 0, FLIPS),
    ];
    for (label, from, to) in windows {
        for (_, line) in residual_lines(&recording, label, leg(from, to)) {
            rec.line(line);
        }
    }
    // Open ground: a step from a moving record to the next with the hero level
    // at both ends (he tilts with the ground, so his tilt says it's level).
    let level: std::collections::BTreeSet<u64> = frames
        .windows(2)
        .filter(|w| {
            let speed = f64::from(w[1].hero.velocity[0]).hypot(f64::from(w[1].hero.velocity[2]));
            speed > 0.01 && w.iter().all(|f| f.hero.rotation[0].abs() <= LEVEL_TILT)
        })
        .map(|w| w[0].flip)
        .collect();
    let lines = residual_lines(
        &recording,
        "open level ground, moving, the whole walk",
        |f| level.contains(&f),
    );
    let best_open = lines[0].0;
    for (_, line) in lines {
        rec.line(line);
    }
    // On a slope: the step's length in 3D against the speed times the step,
    // the stored velocity staying level while he climbs.
    let mut slope_miss: f64 = 0.0;
    let mut slope_pairs = 0;
    for (k, w) in frames.windows(2).enumerate() {
        let speed = f64::from(w[1].hero.velocity[0]).hypot(f64::from(w[1].hero.velocity[2]));
        if speed <= 0.01 || w[1].flip != w[0].flip + 1 || w[1].hero.rotation[0].abs() <= LEVEL_TILT
        {
            continue;
        }
        let d = |i: usize| f64::from(w[1].hero.position[i]) - f64::from(w[0].hero.position[i]);
        let length = (d(0) * d(0) + d(1) * d(1) + d(2) * d(2)).sqrt();
        slope_miss = slope_miss.max((length - speed * steps[k]).abs());
        slope_pairs += 1;
    }
    rec.line(format!(
        "on slopes (|tilt| over {LEVEL_TILT} deg), the step's 3D length against speed x the \
         earlier record's step: worst miss {slope_miss:.3e} m over {slope_pairs} pairs"
    ));
    // The game's own clock on him, against the same steps.
    let stamps: Vec<Option<f32>> = ring.records.iter().map(|r| watch.time_stamp(r)).collect();
    for alignment in Alignment::ALL {
        let mut worst: f64 = 0.0;
        for k in 0..frames.len() - 1 {
            if let (Some(a), Some(b)) = (stamps[k], stamps[k + 1]) {
                let step = match alignment {
                    Alignment::Earlier => steps[k],
                    Alignment::Later => steps[k + 1],
                };
                worst = worst.max(((f64::from(b) - f64::from(a)) - step).abs());
            }
        }
        rec.line(format!(
            "the hero's _timeStamp step against the {alignment:?} record's step: worst miss {worst:.3e} s \
             (first {:?}, last {:?})",
            stamps.first().copied().flatten(),
            stamps.last().copied().flatten()
        ));
    }

    // ---- the stick floats, the clips and the camera on each leg -------------
    for (label, offset) in [
        ("at rest (offset 30)", 30u64),
        ("forward (offset 100)", 100),
        ("left (offset 160)", 160),
        ("at rest (offset 280)", 280),
        ("South held (offset 302)", 302),
        ("West held (offset 362)", 362),
        ("at rest (offset 590)", 590),
    ] {
        let Some(f) = frames.iter().find(|f| f.flip == base + offset) else {
            continue;
        };
        let top = f.top_clip();
        let name = top.and_then(|c| {
            watch
                .clips
                .iter()
                .find(|w| w.id == c.id)
                .and_then(|w| w.name.as_deref())
                .map(|n| n.rsplit('/').next().unwrap_or(n).to_owned())
        });
        let speed = f64::from(f.hero.velocity[0]).hypot(f64::from(f.hero.velocity[2]));
        rec.line(format!(
            "{label}, flip {}: stick floats {:?}, pad {:?}; hero at ({:.4}, {:.4}, {:.4}) \
             heading {:.3} deg, {speed:.4} m/s; top clip {} (weight {:?}, rate {:?}%, {:.4} of \
             {:.4} s); weights {:?}; camera heading {:?}, fov {:?}",
            f.flip,
            f.stick,
            f.pad.map(|p| (p.left.x, p.left.y, p.digital1, p.digital2)),
            f.hero.position[0],
            f.hero.position[1],
            f.hero.position[2],
            f.hero.rotation[1],
            name.unwrap_or_else(|| "?".into()),
            top.map(|c| c.weight),
            top.map(|c| c.rate),
            top.map_or(0.0, |c| c.time()),
            top.map_or(0.0, |c| c.period),
            f.clips.iter().map(|c| c.weight).collect::<Vec<_>>(),
            f.camera.map(|c| c.rotation[1]),
            f.camera.map(|c| c.fov),
        ));
    }
    // Which of the two controllers' floats move with the stick.
    for field in ["hero.pad", "pad0"] {
        let words: Vec<Vec<f32>> = ring
            .records
            .iter()
            .filter_map(|r| watch.list.field(r, field))
            .map(|b| {
                b.chunks(4)
                    .map(|w| f32::from_be_bytes(w.try_into().unwrap()))
                    .collect()
            })
            .collect();
        let Some(first) = words.first() else {
            rec.line(format!("{field}: not watched"));
            continue;
        };
        let moving: Vec<String> = (0..first.len())
            .filter(|&i| words.iter().any(|w| w[i].to_bits() != first[i].to_bits()))
            .map(|i| {
                let at = |offset: u64| {
                    frames
                        .iter()
                        .position(|f| f.flip == base + offset)
                        .map(|k| words[k][i])
                };
                format!(
                    "+{:#05x} (rest {:?}, forward {:?}, left {:?}, South {:?}, West {:?})",
                    0x148 + 4 * i,
                    at(30),
                    at(100),
                    at(160),
                    at(302),
                    at(362)
                )
            })
            .collect();
        rec.line(format!(
            "{field}: {} of {} words move: {}",
            moving.len(),
            first.len(),
            moving.join("; ")
        ));
    }
    // The top clip's time against the steps while it stays the top clip.
    let mut clip_miss: f64 = 0.0;
    let mut clip_pairs = 0;
    for (k, w) in frames.windows(2).enumerate() {
        let (Some(a), Some(b)) = (w[0].top_clip(), w[1].top_clip()) else {
            continue;
        };
        if a.id != b.id || b.time() < a.time() || a.weight < 100.0 {
            continue;
        }
        let rate = f64::from(a.rate) / 100.0;
        clip_miss = clip_miss.max((f64::from(b.time() - a.time()) - steps[k] * rate).abs());
        clip_pairs += 1;
    }
    rec.line(format!(
        "the top clip's time against the earlier record's step x its rate: worst miss \
         {clip_miss:.3e} s over {clip_pairs} pairs; records without clips {}",
        frames.iter().filter(|f| f.clips.is_empty()).count()
    ));
    let viewports: Vec<Option<u32>> = ring
        .records
        .iter()
        .map(|r| watch.current_viewport(r))
        .collect();
    rec.line(format!(
        "camera viewport the system names: {:?} throughout: {}",
        viewports.first().copied().flatten(),
        viewports.windows(2).all(|w| w[0] == w[1])
    ));

    // ---- the last record against PINE's read at the end pause ----------------
    let last = frames.last().unwrap();
    let hero_end = at_end.hero().unwrap();
    let d = |i: usize| f64::from(hero_end.position[i]) - f64::from(last.hero.position[i]);
    rec.line(format!(
        "end pause at {} (game frame {}, dt {:.4}): hero ({:.5}, {:.5}, {:.5}); the last record \
         (flip {}, game frame {}): ({:.5}, {:.5}, {:.5}), the pause {:.5} m on in x and {:.5} in z",
        end.frames,
        at_end.game_frame,
        at_end.frame_delta,
        hero_end.position[0],
        hero_end.position[1],
        hero_end.position[2],
        last.flip,
        last.game_frame,
        last.hero.position[0],
        last.hero.position[1],
        last.hero.position[2],
        d(0),
        d(2)
    ));

    // ---- shut down --------------------------------------------------------------
    rec.line(format!("shutdown: {:?}", oracle.shutdown().unwrap()));
    assert_eq!(
        sha256(&figure),
        figure_hash,
        "the figure source changed (only its copy may)"
    );
    let changed = before.diff(&Snapshot::take(&live.dev_dirs).unwrap());
    assert!(
        changed.is_empty(),
        "the dev's RPCS3 dirs changed:\n{}",
        changed.join("\n")
    );
    rec.line(format!(
        "end: {}; {}; load average {}; uptime: {}",
        screen_lock(),
        hid_idle(),
        load_average(),
        uptime()
    ));
    assert!(
        best_open < FLOAT_NOISE,
        "no alignment and order fits the open-ground steps within {FLOAT_NOISE} m (best {best_open:.3e})"
    );
}
