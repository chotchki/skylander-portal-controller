//! AG.28's verify, headless on the dump and the oracle's recordings
//! (`#[ignore]`d: `GUAC_GIANTS_DUMP` and `GUAC_CAPTURE_DIR` from the process
//! env, else `.env.dev`). Run with `cargo test --release -p guacamole --test
//! giants_camera -- --ignored --nocapture --test-threads 1`.
//!
//! The camera is the original's scripts on the VM, strict, with the level's
//! props standing. The recordings drive it as AG.17 drove its chain: the
//! hero's pivot and the frame's step are the recording's own, one game frame
//! a record (his controller is left out: the pivot is set where the next
//! record has it, after the scripts' step), and the VM's tick is that step.

mod common;

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use common::*;
use guac_motion::camera::CameraDef;
use guac_motion::seam::Vec3;
use guac_oracle::recording::{Frame, Recording};
use guac_script::{Halt, Mode, Value};
use guacamole::scripts;

const AG13: &str = "ag13-camera-1792480351";
const MAGIC: &str = "follow.33be0364";
const TARGET: &str = "follow.33bf25d8";
const FOLLOW_7: &str = "follow.33bd1f40";
const AG40: [(&str, &str); 6] = [
    ("ag40-cross-1792512907", "g40_letgo-W-00"),
    ("ag40-cross-1792512907", "g40_letgo-W-01"),
    ("ag40-cross3-1792513842", "g40_full3-W-00"),
    ("ag40-cross3-1792513842", "g40_full3-W-01"),
    ("ag40-cross3-1792513842", "g40_half4-W-00"),
    ("ag40-cross3-1792513842", "g40_half4-W-01"),
];

// ---- the recordings (as guac-motion's tests/oracle reads them) ----

fn replay_dirs(run: &str) -> Vec<(String, PathBuf)> {
    let root = env_or_dotenv("GUAC_CAPTURE_DIR").expect("set GUAC_CAPTURE_DIR");
    let run_dir = PathBuf::from(root).join("BLUS30968").join(run);
    assert!(run_dir.is_dir(), "no {}", run_dir.display());
    let mut out = Vec::new();
    for entry in std::fs::read_dir(&run_dir).unwrap().flatten() {
        let label = entry.file_name().to_string_lossy().into_owned();
        if entry.path().join("recording.json").is_file() {
            out.push((label, entry.path()));
            continue;
        }
        let Ok(replays) = std::fs::read_dir(entry.path()) else {
            continue;
        };
        for replay in replays.flatten() {
            if replay.path().join("recording.json").is_file() {
                let name = format!("{label}/{}", replay.file_name().to_string_lossy());
                out.push((name, replay.path()));
            }
        }
    }
    out.sort();
    assert!(!out.is_empty(), "no recordings under {}", run_dir.display());
    out
}

fn frames(dir: &Path) -> Vec<Frame> {
    let text = std::fs::read_to_string(dir.join("recording.json")).unwrap();
    let recording: Recording = serde_json::from_str(&text).unwrap();
    recording.frames().to_vec()
}

fn followers(dir: &Path, frames: &[Frame]) -> BTreeMap<String, Vec<Vec3>> {
    let text = std::fs::read_to_string(dir.join("watch.json")).unwrap();
    let watch: serde_json::Value = serde_json::from_str(&text).unwrap();
    let list = &watch["watch"]["list"];
    let ranges: Vec<(u64, u64)> = list["ranges"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| (r["addr"].as_u64().unwrap(), r["len"].as_u64().unwrap()))
        .collect();
    let mut at = Vec::new();
    let mut offset = 0;
    for &(_, len) in &ranges {
        at.push(offset);
        offset += len;
    }
    let mut fields = BTreeMap::new();
    for f in list["fields"].as_array().unwrap() {
        let name = f["name"].as_str().unwrap();
        if !name.starts_with("follow.") {
            continue;
        }
        let addr = f["addr"].as_u64().unwrap();
        let (i, (start, _)) = ranges
            .iter()
            .enumerate()
            .find(|(_, (a, l))| *a <= addr && addr + 12 <= a + l)
            .expect("a follower inside a watched range");
        fields.insert(name.to_owned(), (i, (at[i] + addr - start) as usize));
    }
    let records = watch["records"].as_array().unwrap();
    assert_eq!(records.len(), frames.len(), "{}", dir.display());
    let mut out: BTreeMap<String, Vec<Vec3>> =
        fields.keys().map(|k| (k.clone(), Vec::new())).collect();
    for (r, f) in records.iter().zip(frames) {
        assert_eq!(r["flip"].as_u64(), Some(f.flip));
        let missing = r["missing"].as_u64().unwrap_or(0);
        let hex = r["payload"].as_str().unwrap();
        for (name, &(range, off)) in &fields {
            let v = if missing & (1 << range) != 0 {
                Vec3::splat(f32::NAN)
            } else {
                let word = |k: usize| {
                    let s = &hex[(off + 4 * k) * 2..(off + 4 * k + 4) * 2];
                    f32::from_bits(u32::from_str_radix(s, 16).unwrap())
                };
                Vec3::new(word(0), word(1), word(2))
            };
            out.get_mut(name).unwrap().push(v);
        }
    }
    out
}

fn pivot(f: &Frame) -> Vec3 {
    Vec3::from(f.hero.position)
}
fn eye(f: &Frame) -> Vec3 {
    Vec3::from(f.camera.expect("the watch held the camera").position)
}
fn far(f: &Frame) -> f32 {
    f.camera.expect("the watch held the camera").far_clip
}
fn consecutive_from(frames: &[Frame], k: usize) -> bool {
    frames[k + 1].game_frame == frames[k].game_frame.wrapping_add(1)
}
fn quantile(v: &mut [f32], q: f32) -> f32 {
    if v.is_empty() {
        return f32::NAN;
    }
    v.sort_by(f32::total_cmp);
    v[((v.len() - 1) as f32 * q).round() as usize]
}
fn turn(a: f32, b: f32) -> f32 {
    let d = (a - b).rem_euclid(360.0);
    d.min(360.0 - d)
}

// ---- the VM's camera ----

fn global(run: &mut Run, name: &str) -> Value {
    let g = run.vm.program().global_named(name).expect(name);
    let v = run.vm.global(g).expect(name);
    run.vm.value(v)
}

fn field(run: &Run, v: Value, name: &str) -> Option<Value> {
    run.vm.field(v, name).map(|f| run.vm.value(f))
}

fn path(run: &Run, mut v: Value, names: &[&str]) -> Option<Value> {
    for n in names {
        v = field(run, v, n)?;
    }
    Some(v)
}

fn actor_at(run: &Run, v: Option<Value>) -> Option<(guac_script::Handle, Vec3)> {
    let Some(Value::Entity(h)) = v else {
        return None;
    };
    run.world.actor(h).map(|a| (h, a.position))
}

/// The follower camera `id` keeps its look-from and look-at points on.
fn follower(run: &mut Run, id: i64) -> Option<(guac_script::Handle, Vec3)> {
    let cc = global(run, "camera control data");
    let infos = field(run, cc, "cam infos")?;
    let all = run.vm.set(infos)?.to_vec();
    for e in all {
        let e = run.vm.value(e);
        let cid = path(run, e, &["cam def", "id"]).and_then(|v| v.as_f32());
        if cid == Some(id as f32) {
            return actor_at(run, field(run, e, "camera"));
        }
    }
    None
}

/// The player's camera target.
fn cam_target(run: &Run) -> Option<(guac_script::Handle, Vec3)> {
    let pm = scripts::portal_manager_placement(&run.vm, &run.world)?;
    let players = run.vm.local(pm, "player data").map(|v| run.vm.value(v))?;
    let p0 = run.vm.value(*run.vm.set(players)?.first()?);
    actor_at(run, path(run, p0, &["cam control data", "cam target"]))
}

/// One step as `Run::step` makes it, the hero moved by the recording: his
/// pivot is where the next record has it after the scripts' step, with the
/// velocity (m/s) the move gives over `step`; the tick is `step` long.
fn tick(run: &mut Run, step: f32, next: Vec3, facing: f32) -> Result<(), Halt> {
    run.vm.set_tick_seconds(step);
    let pads = run.bench.pads;
    run.world.read_pads(pads);
    for (h, active) in run.world.activity(&mut run.bench) {
        run.vm.set_asleep(h, !active);
    }
    let joined = run.minion.is_some();
    let player_loop = run.player_loop.clone();
    let mut stopped = Vec::new();
    let r = run.with_host(|vm, host| {
        player_loop.frame_with(vm, host, joined, &mut stopped, &mut |_, _, _| Ok(()))
    });
    run.world.end_step();
    scripts::add_scripted(&mut run.vm, &mut run.world);
    run.bench.stand_props(&run.world);
    let report = r?;
    assert!(stopped.is_empty() && report.stopped.is_empty());
    let hero = run.bench.heroes.get_mut(&HERO).unwrap();
    hero.velocity = (next - hero.pivot) / step;
    hero.pivot = next;
    hero.facing = facing;
    run.world.advance(step, &mut run.bench);
    run.steps += 1;
    run.pose_hands();
    run.bench.place_bodies(&run.world);
    Ok(())
}

fn fresh() -> Run {
    let (mut run, _found) = joined_with(Mode::Strict, Setup { props: true });
    run.bench.hold_view = false;
    run
}

fn set_hero(run: &mut Run, f: &Frame) {
    let hero = run.bench.heroes.get_mut(&HERO).unwrap();
    hero.pivot = pivot(f);
    hero.facing = f.hero.rotation[1];
    hero.walk.velocity = Vec3::ZERO;
    hero.velocity = Vec3::ZERO;
    let (at, facing) = (hero.pivot, hero.facing);
    // His actor stands where his controller does, as the step's end leaves
    // it: what the camera's points are attached to.
    run.world.place(HERO, at, facing);
}

fn place(run: &mut Run, h: guac_script::Handle, at: Vec3) {
    assert!(run.world.place(h, at, 0.0), "no actor {h}");
}

// ---- (1) the definition ----

/// gameplay-camera's The camera starts behind the hero, off the scripts:
/// camera 0's definition read out of the VM's structures (55 ft, 32°, 195°,
/// aimed 9 ft up, FOV 68, far clip 800), the viewport where it puts the
/// eye, and the 35 cameras and 20 transition planes the definition's
/// STARTUP made.
#[test]
#[ignore = "reads the Giants dump"]
fn camera_0_off_the_definition() {
    let mut run = fresh();
    let load = load();
    let cc = global(&mut run, "camera control data");
    let defs = field(&run, cc, "camera definitions").unwrap();
    let planes = field(&run, cc, "transition planes").unwrap();
    let (n_defs, n_planes) = (
        run.vm.set(defs).unwrap().len(),
        run.vm.set(planes).unwrap().len(),
    );
    let ids: Vec<i64> = run
        .vm
        .set(defs)
        .unwrap()
        .to_vec()
        .into_iter()
        .map(|d| {
            let d = run.vm.value(d);
            field(&run, d, "id").and_then(|v| v.as_f32()).unwrap() as i64
        })
        .collect();
    let mut sorted = ids.clone();
    sorted.sort();
    sorted.dedup();
    let d0 = run.vm.value(run.vm.set(defs).unwrap()[0]);
    let num = |names: &[&str]| path(&run, d0, names).and_then(|v| v.as_f32()).unwrap();
    let dist = num(&["look from distance offset"]);
    let pitch = num(&["look from pitch offset"]);
    let heading = num(&["look from heading offset"]);
    let at = num(&["look at distance offset"]);
    let fov = num(&["default data", "fov"]);
    let far_clip = num(&["far clip"]);
    println!("[{load}]");
    println!(
        "camera definitions {n_defs} ({} distinct ids), transition planes {n_planes}",
        sorted.len()
    );
    println!(
        "camera 0 (id {}): {dist} ft at {pitch} deg, look-from heading {heading}, aimed {at} ft up, fov {fov}, far clip {far_clip}",
        ids[0]
    );
    assert_eq!((n_defs, n_planes), (35, 20));
    assert_eq!(sorted.len(), 35);
    assert_eq!(
        (ids[0], dist, pitch, heading, at, fov, far_clip),
        (0, 55.0, 32.0, 195.0, 9.0, 68.0, 800.0)
    );

    // The eye the definition gives off the follow point (the point the
    // scripts keep the look-from target on).
    let (_, magic) = follower(&mut run, 0).expect("camera 0's follower");
    let ft = 0.3048_f64;
    let (sh, ch) = f64::from(heading).to_radians().sin_cos();
    let (sp, cp) = f64::from(pitch).to_radians().sin_cos();
    let out = Vec3::new(
        (f64::from(dist) * ft * cp * sh) as f32,
        (f64::from(dist) * ft * sp) as f32,
        (f64::from(dist) * ft * cp * ch) as f32,
    );
    let want = magic + out;
    let aim = magic + Vec3::Y * (f64::from(at) * ft) as f32;
    let v = *run.world.viewport();
    let look = aim - want;
    let want_facing = look.x.atan2(look.z).to_degrees();
    let want_tilt = look.y.atan2(look.x.hypot(look.z)).to_degrees();
    let miss = (v.eye - want).length();
    let hero = run.hero().pivot;
    println!(
        "viewport (source {:?}): eye {:?}; the definition's eye off the follow point {:?} is {:?}: {miss:.3e} m",
        v.source, v.eye, magic, want
    );
    println!(
        "facing {:.6} (definition {:.6}, off {:.3e} deg), tilt {:.6} (definition {:.6}, off {:.3e} deg), fov {} far clip {}",
        v.angles.facing,
        want_facing,
        turn(v.angles.facing, want_facing),
        v.angles.tilt,
        want_tilt,
        (v.angles.tilt - want_tilt).abs(),
        v.fov,
        v.far_clip
    );
    println!(
        "eye less his pivot {:?} (AG.13's recording: (-3.679540, 8.883592, -13.732210)), the follow point {:.3e} m off his pivot",
        v.eye - hero,
        (magic - hero).length()
    );
    assert_eq!(v.source, guacamole::viewport::Source::Scripts);
    assert!(miss < 1e-4, "{miss}");
    assert!(turn(v.angles.facing, want_facing) < 1e-3);
    assert!((v.angles.tilt - want_tilt).abs() < 1e-3);
    assert_eq!((v.fov, v.far_clip), (68.0, 800.0));
}

// ---- (2) the recordings ----

struct Rec {
    name: String,
    frames: Vec<Frame>,
    magic: Vec<Vec3>,
    target: Vec<Vec3>,
    f7: Option<Vec<Vec3>>,
}

fn rec(name: String, dir: &Path) -> Rec {
    let frames = frames(dir);
    let mut f = followers(dir, &frames);
    Rec {
        name,
        magic: f.remove(MAGIC).unwrap(),
        target: f.remove(TARGET).unwrap(),
        f7: f.remove(FOLLOW_7),
        frames,
    }
}

#[derive(Default)]
struct Gaps {
    /// The follower, the target and the eye, m, per record.
    magic: Vec<f32>,
    target: Vec<f32>,
    eye: Vec<f32>,
    angle: Vec<f32>,
    far: Vec<f32>,
    /// The records driven.
    steps: usize,
    /// Per record after step k: the VM's eye, far clip and share.
    eyes: Vec<Vec3>,
    fars: Vec<f32>,
}

/// The recording through the VM's camera from the recorded rest at F0, the
/// target fed (`fed`: set where the recording has it after each step, as
/// AG.17 fed its chain) or the scripts' own. Stops where a record isn't the
/// next game frame; `camera_0_only` also where the far clip leaves 800.
fn drive(run: &mut Run, r: &Rec, fed: bool, camera_0_only: bool) -> Gaps {
    let f = &r.frames;
    set_hero(run, &f[0]);
    let (mh, _) = follower(run, 0).unwrap();
    place(run, mh, r.magic[0]);
    let (th, _) = cam_target(run).unwrap();
    place(run, th, r.target[0]);
    let mut g = Gaps::default();
    // The viewport as the scripts leave it before the first step is the
    // rest one: the first record's eye against the definition.
    for k in 0..f.len() - 1 {
        if !consecutive_from(f, k) || (camera_0_only && far(&f[k + 1]) != 800.0) {
            break;
        }
        let step = f[k].clock.step();
        if let Err(h) = tick(run, step, pivot(&f[k + 1]), f[k + 1].hero.rotation[1]) {
            panic!("{}: step {k}: {h}", r.name);
        }
        if fed {
            let (th, _) = cam_target(run).unwrap();
            place(run, th, r.target[k + 1]);
        }
        let next = &f[k + 1];
        let c = next.camera.unwrap();
        let v = *run.world.viewport();
        g.steps += 1;
        g.eyes.push(v.eye);
        g.fars.push(v.far_clip);
        if let Some((_, m)) = follower(run, 0) {
            g.magic.push((m - r.magic[k + 1]).length());
        }
        if let Some((_, t)) = cam_target(run) {
            g.target.push((t - r.target[k + 1]).length());
        }
        g.eye.push((v.eye - eye(next)).length());
        g.angle
            .push(turn(v.angles.facing, c.rotation[1]).max((v.angles.tilt - c.rotation[0]).abs()));
        g.far.push((v.far_clip - far(next)).abs());
    }
    g
}

fn walks() -> Vec<Rec> {
    replay_dirs(AG13)
        .into_iter()
        .map(|(n, d)| rec(n, &d))
        .collect()
}

/// Camera 0 at rest: each of AG.13's eight recordings from F0 until his
/// pivot first moves, the VM's follower and target put where the recording
/// has them at F0 and the target fed: the eye and the view's angles against
/// the recorded ones, record by record.
#[test]
#[ignore = "reads the Giants dump and the recordings"]
fn camera_0_at_rest_against_ag13() {
    let mut worst = 0.0f32;
    let mut angles = 0.0f32;
    let mut n = 0;
    let load = load();
    for r in walks() {
        let rest = (0..r.frames.len())
            .find(|&k| (pivot(&r.frames[k]) - pivot(&r.frames[0])).length() > 1e-6)
            .unwrap()
            - 1;
        let mut run = fresh();
        let g = drive(&mut run, &r, true, true);
        let e = g.eye[..rest].iter().cloned().fold(0.0, f32::max);
        let a = g.angle[..rest].iter().cloned().fold(0.0, f32::max);
        println!(
            "{}: {rest} records at rest, eye within {e:.3e} m, the view's angles within {a:.2e} deg",
            r.name
        );
        worst = worst.max(e);
        angles = angles.max(a);
        n += rest;
    }
    println!(
        "[{load}] 8 recordings, {n} records at rest: eye within {worst:.3e} m, angles within {angles:.2e} deg"
    );
    // AG.17's rest: the eye within 5.16e-5 m, the stored angles 4.63e-4 deg off.
    assert!(worst < 5.16e-5 && angles < 5e-4, "{worst} {angles}");
}

/// The follow point on AG.13's eight walks with the target fed, and the
/// eye on camera 0's offset from it, on camera 0 (up to the switch).
#[test]
#[ignore = "reads the Giants dump and the recordings"]
fn the_follow_point_on_ag13s_walks_fed_the_recorded_target() {
    let def = CameraDef::LEVEL_213_START;
    let load = load();
    let (mut steps, mut worst_m, mut worst_e) = (0, 0.0f32, 0.0f32);
    for r in walks() {
        let mut run = fresh();
        let g = drive(&mut run, &r, true, true);
        // The eye on camera 0's offset from the VM's own follow point is
        // the scripts' law; the recorded eye less the recorded point is
        // AG.17's. The eye against the recorded eye, outside the shake:
        // records whose recorded eye sits on camera 0's offset from the
        // recorded point.
        let calm = |k: usize| (def.view(r.magic[k]).eye - eye(&r.frames[k])).length() < 1e-3;
        let first_shaken = (0..r.frames.len())
            .find(|&k| far(&r.frames[k]) == 800.0 && !calm(k))
            .unwrap_or(usize::MAX);
        let mut e = Vec::new();
        for (i, miss) in g.eye.iter().enumerate() {
            if i + 1 + 30 < first_shaken {
                e.push(*miss);
            }
        }
        let mut m = g.magic.clone();
        let (wm, we) = (quantile(&mut m, 1.0), quantile(&mut e, 1.0));
        println!(
            "{}: {} steps, follow point within {wm:.2e} m, eye within {we:.2e} m before the shake ({} records), whole-run eye {:.2e} m (shake included)",
            r.name,
            g.steps,
            e.len(),
            g.eye.iter().cloned().fold(0.0, f32::max)
        );
        steps += g.steps;
        worst_m = worst_m.max(wm);
        worst_e = worst_e.max(we);
    }
    println!(
        "[{load}] AG.13's 8 walks: {steps} steps on camera 0, the follow point within {worst_m:.2e} m (AG.17: 7.63e-6), the eye within {worst_e:.2e} m before the shake (AG.17: 1.03e-5 on the offset)"
    );
    // AG.17's: the follow point 7.63e-6 m, the eye 1.03e-5 m.
    assert!(worst_m < 7.63e-6, "{worst_m}");
    assert!(worst_e < 1.1e-5, "{worst_e}");
}

/// The whole camera on its own scripts on AG.13's eight walks: the target
/// (AG.17's hypothesis missed it by 1.37-2.64 cm on the level walks and
/// 2.49-2.89 cm up the corridor), the follow point and the eye.
#[test]
#[ignore = "reads the Giants dump and the recordings"]
fn the_cameras_own_target_on_ag13s_walks() {
    let load = load();
    let mut level = [0.0f32; 3];
    let mut corridor = [0.0f32; 3];
    for r in walks() {
        let mut run = fresh();
        let g = drive(&mut run, &r, false, true);
        // The eye outside the rock slide's shake, as AG.17 set it apart.
        let def = CameraDef::LEVEL_213_START;
        let first_shaken = (0..r.frames.len())
            .find(|&k| {
                far(&r.frames[k]) == 800.0
                    && (def.view(r.magic[k]).eye - eye(&r.frames[k])).length() >= 1e-3
            })
            .unwrap_or(usize::MAX);
        let calm_eye: Vec<f32> = g
            .eye
            .iter()
            .enumerate()
            .filter(|(i, _)| i + 1 + 30 < first_shaken)
            .map(|(_, e)| *e)
            .collect();
        let (mut t, mut m, mut e) = (g.target.clone(), g.magic.clone(), calm_eye);
        let worst = [
            quantile(&mut t, 1.0),
            quantile(&mut m, 1.0),
            quantile(&mut e, 1.0),
        ];
        println!(
            "{} ({} steps): target within {:.4} m (median {:.4}), follow point {:.4} m, eye {:.4} m (median {:.4})",
            r.name,
            g.steps,
            worst[0],
            quantile(&mut t, 0.5),
            worst[1],
            worst[2],
            quantile(&mut e, 0.5)
        );
        let into = if r.name.contains("corridor") {
            &mut corridor
        } else {
            &mut level
        };
        for (a, b) in into.iter_mut().zip(worst) {
            *a = a.max(b);
        }
    }
    println!(
        "[{load}] level walks: target {:.4} m, follow point {:.4} m, eye {:.4} m; corridor walks (up to the switch): {:.4}, {:.4}, {:.4} m",
        level[0], level[1], level[2], corridor[0], corridor[1], corridor[2]
    );
    // The scripts' own camera target against AG.17's hypothesis' bounds
    // (2.64 and 2.89 cm for the target, 0.92 and 0.90 cm for the follow
    // point and the eye): not worse.
    assert!(
        level[0] < 0.03 && level[1] < 0.01 && level[2] < 0.01,
        "{level:?}"
    );
    assert!(corridor[0] < 0.03 && corridor[1] < 0.01, "{corridor:?}");
}

/// The share of the way a far clip (ft) is from 800 to 1,000.
fn share(far: f32) -> f32 {
    (far - 800.0) / 200.0
}

/// The 40/20/40 trapezoid on game time: the share `t` seconds into a switch
/// of length `len`, speeding up evenly over its first 40 %, steady for 20 %
/// and slowing over the last 40 %.
fn trapezoid(t: f32, len: f32) -> f32 {
    let u = (t / len).clamp(0.0, 1.0);
    // Speed s on [0, .4] rising linearly to v, v on [.4, .6], falling to 0 on
    // [.6, 1]: the area is v * (0.2 + 0.4) = 1.
    let v = 1.0 / 0.6;
    if u < 0.4 {
        v * u * u / (2.0 * 0.4)
    } else if u < 0.6 {
        v * (0.2 + (u - 0.4))
    } else {
        1.0 - v * (1.0 - u) * (1.0 - u) / (2.0 * 0.4)
    }
}

/// The best start of a `len`-long trapezoid for `shares` at `times`: the
/// start and the worst miss.
fn best_curve(times: &[f32], shares: &[f32], len: f32) -> (f32, f32) {
    let miss = |t0: f32| {
        times
            .iter()
            .zip(shares)
            .map(|(t, s)| (trapezoid(t - t0, len) - s).abs())
            .fold(0.0f32, f32::max)
    };
    let mut lo = times[0];
    let mut hi = *times.last().unwrap();
    // Coarse grid then repeated narrowing.
    let mut best = (lo, miss(lo));
    let n = 4000;
    for i in 0..=n {
        let t0 = lo + (hi - lo) * i as f32 / n as f32;
        let m = miss(t0);
        if m < best.1 {
            best = (t0, m);
        }
    }
    let mut width = (hi - lo) / n as f32 * 2.0;
    for _ in 0..40 {
        lo = best.0 - width;
        hi = best.0 + width;
        for i in 0..=200 {
            let t0 = lo + (hi - lo) * i as f32 / 200.0;
            let m = miss(t0);
            if m < best.1 {
                best = (t0, m);
            }
        }
        width /= 3.0;
    }
    best
}

/// The sliding rocks' switch on AG.13's two corridor walks: the VM's far
/// clip record by record against the recorded one, and each's own best
/// 40/20/40 curve on game time (the recorded steps summed), plus the shake.
#[test]
#[ignore = "reads the Giants dump and the recordings"]
fn the_sliding_rocks_switch_on_ag13s_corridor_walks() {
    let load = load();
    let def = CameraDef::LEVEL_213_START;
    for r in walks().into_iter().filter(|r| r.name.contains("corridor")) {
        let mut run = fresh();
        let g = drive(&mut run, &r, false, false);
        println!(
            "{}: {} steps driven of {}",
            r.name,
            g.steps,
            r.frames.len() - 1
        );
        // Game time at each record, the steps summed.
        let mut t = vec![0.0f32];
        for f in &r.frames[..g.steps] {
            t.push(t.last().unwrap() + f.clock.step());
        }
        let ours: Vec<f32> = std::iter::once(share(800.0))
            .chain(g.fars.iter().map(|&f| share(f)))
            .collect();
        let theirs: Vec<f32> = r.frames[..=g.steps].iter().map(|f| share(far(f))).collect();
        let first_theirs = theirs.iter().position(|&s| s > 0.0);
        let first_ours = ours.iter().position(|&s| s > 0.0);
        let last_theirs = theirs.iter().position(|&s| s >= 1.0);
        let last_ours = ours.iter().position(|&s| s >= 1.0);
        println!(
            "  share leaves 0 at record {first_theirs:?} (VM {first_ours:?}); reaches 1 at {last_theirs:?} (VM {last_ours:?})"
        );
        let (a, b) = (
            first_theirs.unwrap().saturating_sub(5),
            (last_theirs.unwrap() + 5).min(theirs.len()),
        );
        let diff = (a..b)
            .map(|k| (ours[k] - theirs[k]).abs())
            .fold(0.0f32, f32::max);
        println!(
            "  record by record over {a}..{b}: the VM's share off the recording's by at most {diff:.3e}"
        );
        assert_eq!((first_theirs, last_theirs), (first_ours, last_ours));
        assert!(diff < 4e-7, "{diff}");
        let (t0r, mr) = best_curve(&t[a..b], &theirs[a..b], 2.0);
        let (t0o, mo) = best_curve(&t[a..b], &ours[a..b], 2.0);
        println!(
            "  best 2.000 s trapezoid on game time: recording starts {t0r:.4} s (miss {mr:.2e}), VM starts {t0o:.4} s (miss {mo:.2e}); starts differ by {:.4} s",
            (t0o - t0r).abs()
        );
        assert!(
            mr < 3.5e-7 && mo < 3.5e-7 && (t0o - t0r).abs() < 1e-3,
            "{mr} {mo} {t0r} {t0o}"
        );
        // The shake: the eye less camera 0's offset from the follow point.
        let offs = |eyes: &dyn Fn(usize) -> Vec3, magics: &dyn Fn(usize) -> Vec3, k: usize| {
            eyes(k) - def.view(magics(k)).eye
        };
        let mut vm_magic = Vec::new();
        {
            // Re-run to read the VM's follower each record.
            let mut run2 = fresh();
            let f = &r.frames;
            set_hero(&mut run2, &f[0]);
            let (mh, _) = follower(&mut run2, 0).unwrap();
            place(&mut run2, mh, r.magic[0]);
            let (th, _) = cam_target(&run2).unwrap();
            place(&mut run2, th, r.target[0]);
            vm_magic.push(r.magic[0]);
            for k in 0..g.steps {
                tick(&mut run2, f[k].clock.step(), pivot(&f[k + 1]), 0.0).unwrap();
                vm_magic.push(follower(&mut run2, 0).map_or(Vec3::NAN, |m| m.1));
            }
        }
        let vm_eyes: Vec<Vec3> = std::iter::once(eye(&r.frames[0]))
            .chain(g.eyes.iter().copied())
            .collect();
        let rec_shake: Vec<Vec3> = (0..=g.steps)
            .map(|k| offs(&|k| eye(&r.frames[k]), &|k| r.magic[k], k))
            .collect();
        let vm_shake: Vec<Vec3> = (0..=g.steps)
            .map(|k| offs(&|k| vm_eyes[k], &|k| vm_magic[k], k))
            .collect();
        let switch_at = first_theirs.unwrap();
        let axis = Vec3::new(-0.818, 0.411, 0.403).normalize();
        for (what, s) in [("recorded", &rec_shake), ("VM", &vm_shake)] {
            let w: Vec<(usize, f32)> = (0..switch_at).map(|k| (k, s[k].length())).collect();
            let first = w.iter().find(|(_, l)| *l > 1e-3).map(|x| x.0);
            let last = w.iter().rev().find(|(_, l)| *l > 1e-3).map(|x| x.0);
            let peak = w
                .iter()
                .cloned()
                .fold((0, 0.0f32), |a, b| if b.1 > a.1 { b } else { a });
            // Half-periods: the game time between the shake's sign changes
            // along the axis, over the records it is over 1 cm.
            let along: Vec<f32> = (0..switch_at).map(|k| s[k].dot(axis)).collect();
            let mut changes = Vec::new();
            for k in 1..switch_at {
                if along[k - 1].abs() > 1e-2
                    && along[k].abs() > 1e-2
                    && along[k - 1] * along[k] < 0.0
                {
                    changes.push(t[k]);
                }
            }
            let halves: Vec<f32> = changes.windows(2).map(|c| c[1] - c[0]).collect();
            println!(
                "  shake ({what}) before the switch: first record over 1 mm {first:?}, last {last:?}, peak {:.3} m at record {}; along (-0.818, 0.411, 0.403) {} sign changes, half-periods {:.3}-{:.3} s; the part of the peak off that axis {:.3} m",
                peak.1,
                peak.0,
                changes.len(),
                halves.iter().cloned().fold(f32::INFINITY, f32::min),
                halves.iter().cloned().fold(0.0, f32::max),
                (s[peak.0] - axis * s[peak.0].dot(axis)).length()
            );
        }
        // The second rock's wave starts from nothing where the first's is
        // cut (`Calc - Adjust Value Keyframer` stops the running one): the
        // waves' shape is the law, their starts the rocks' random delays.
        let cut = |s: &[Vec3]| -> Option<usize> {
            let first = (0..switch_at).find(|&k| s[k].length() > 1e-3)?;
            let big = (first..switch_at).find(|&k| s[k].length() > 0.3 * 0.3048)?;
            (big..switch_at).find(|&k| s[k].length() < 1e-4)
        };
        // A cut with a whole second wave after it (55 records) on each side.
        let whole = |s: &[Vec3]| {
            cut(s).filter(|&c| (c + 1..c + 56).all(|k| k < switch_at && s[k].length() > 1e-4))
        };
        if let (Some(a), Some(b)) = (whole(&rec_shake), whole(&vm_shake)) {
            // Both on game time since the cut: the VM's series read at the
            // recording's elapsed times.
            let at = |t_now: f32| -> Vec3 {
                let t_cut = t[b];
                let want = t_cut + t_now;
                let i = (b..switch_at).rfind(|&k| t[k] <= want).unwrap();
                let w = ((want - t[i]) / (t[i + 1] - t[i])).clamp(0.0, 1.0);
                vm_shake[i] * (1.0 - w) + vm_shake[i + 1] * w
            };
            let diffs: Vec<f32> = (1..56)
                .map(|j| (rec_shake[a + j] - at(t[a + j] - t[a])).length() / 0.3048)
                .collect();
            let mut d = diffs.clone();
            let peak = |s: &[Vec3], from: usize| {
                (from..switch_at).map(|k| s[k]).fold(Vec3::ZERO, |m, v| {
                    if v.length() > m.length() { v } else { m }
                }) / 0.3048
            };
            println!(
                "  the second wave: cut at record {a} (recording) and {b} (VM); the {} records after, on game time since the cut, the VM's shake off the recording's by at most {:.4} ft (median {:.4}); peak vector (ft) recording {:?} VM {:?}",
                diffs.len(),
                quantile(&mut d, 1.0),
                quantile(&mut d, 0.5),
                peak(&rec_shake, a),
                peak(&vm_shake, b),
            );
        } else {
            let peak = |s: &[Vec3]| {
                (0..switch_at).map(|k| s[k]).fold(Vec3::ZERO, |m, v| {
                    if v.length() > m.length() { v } else { m }
                }) / 0.3048
            };
            println!(
                "  no cut with a whole wave after it on both sides (recording {:?}, VM {:?}); peak vector (ft) recording {:?} VM {:?}",
                whole(&rec_shake),
                whole(&vm_shake),
                peak(&rec_shake),
                peak(&vm_shake)
            );
        }
        // The peak of the shake, as a vector (ft): the law, whatever the
        // rocks' random starts do to when.
        let peak_of = |s: &[Vec3]| {
            (0..switch_at).map(|k| s[k]).fold(Vec3::ZERO, |m, v| {
                if v.length() > m.length() { v } else { m }
            }) / 0.3048
        };
        let (pr, pv) = (peak_of(&rec_shake), peak_of(&vm_shake));
        println!(
            "  peak shake vector (ft): recording {pr:?}, VM {pv:?}, off by {:.2e} ft",
            (pr - pv).length()
        );
        assert!((pr - pv).length() < 1e-3, "{pr:?} {pv:?}");
        println!("[{load}]");
    }
}

// ---- (2) AG.40's crossings ----

/// The player's three camera points: the intent point, the velocity point
/// and the camera target.
fn target_points(run: &Run) -> Vec<guac_script::Handle> {
    let pm = scripts::portal_manager_placement(&run.vm, &run.world).unwrap();
    let players = run
        .vm
        .local(pm, "player data")
        .map(|v| run.vm.value(v))
        .unwrap();
    let p0 = run.vm.value(run.vm.set(players).unwrap()[0]);
    ["intent target", "velocity target", "cam target"]
        .into_iter()
        .filter_map(|n| match path(run, p0, &["cam control data", n]) {
            Some(Value::Entity(h)) => Some(h),
            _ => None,
        })
        .collect()
}

/// A run at AG.40's hop C: AG.13's corridor walk driven through the rock
/// slide's switch (camera 7 current, its follow point made), then the hero
/// set down where AG.40's recording has him with camera 7's follow point
/// and the player's camera points where it has them.
fn at_hop_c(corridor: &Rec, cross: &Rec) -> Run {
    let mut run = fresh();
    let g = drive(&mut run, corridor, false, false);
    assert_eq!(g.steps, corridor.frames.len() - 1);
    let f7 = cross
        .f7
        .as_ref()
        .expect("camera 7's follow point on the watch");
    set_hero(&mut run, &cross.frames[0]);
    // Standing there a while, so what the scripts remember of his last
    // place is this one.
    for _ in 0..60 {
        let at = pivot(&cross.frames[0]);
        tick(&mut run, 1.0 / 60.0, at, cross.frames[0].hero.rotation[1]).unwrap();
    }
    let (h7, _) = follower(&mut run, 7).expect("camera 7's follower");
    place(&mut run, h7, f7[0]);
    for h in target_points(&run) {
        place(&mut run, h, cross.target[0]);
        // Its intent, velocity and target points are all on the recorded
        // one (AG.17's rest).
        assert!(run.world.rest_remembered(h, cross.target[0]) >= 3);
    }
    run
}

/// One crossing's gaps: `fed` the recorded camera target after each step.
struct Crossing {
    far: f32,
    eye: f32,
    angle: f32,
    f7: f32,
    f7_in_use: f32,
    target: Vec<f32>,
    eyes: Vec<f32>,
    shaken_to: usize,
}

fn cross(run: &mut Run, r: &Rec, fed: bool) -> Crossing {
    let f = &r.frames;
    let f7 = r.f7.as_ref().unwrap();
    // Camera 7 in front of `Transition 7-1`: 57 ft at 28 deg facing
    // `Look_At_Target_0`, aimed 6 ft up (AG.17's reading, AG.40).
    let front = CameraDef {
        id: 7,
        distance: 57.0,
        pitch: 28.0,
        look_from: guac_motion::camera::LookFrom::Facing(Vec3::new(
            11.758146, 53.861694, -39.363586,
        )),
        look_at: 6.0,
        fov_y: guac_motion::camera::FOV_Y,
        far_clip: 1000.0,
    };
    // The eruptions' shake: records wholly camera 7's whose eye is off its
    // offset from the watched follow point.
    let shaken: Vec<usize> = (0..f.len())
        .filter(|&k| far(&f[k]) == 1000.0 && (front.view(f7[k]).eye - eye(&f[k])).length() > 1e-3)
        .collect();
    let calm_from = shaken.last().map_or(0, |&k| k + 1);
    let mut c = Crossing {
        far: 0.0,
        eye: 0.0,
        angle: 0.0,
        f7: 0.0,
        f7_in_use: 0.0,
        target: Vec::new(),
        eyes: Vec::new(),
        shaken_to: calm_from,
    };
    let (h7, _) = follower(run, 7).unwrap();
    for k in 0..f.len() - 1 {
        if !consecutive_from(f, k) {
            break;
        }
        let step = f[k].clock.step();
        if let Err(h) = tick(run, step, pivot(&f[k + 1]), f[k + 1].hero.rotation[1]) {
            panic!("{}: step {k}: {h}", r.name);
        }
        if fed {
            for h in target_points(run).into_iter().rev().take(1) {
                place(run, h, r.target[k + 1]);
            }
        }
        let next = &f[k + 1];
        let v = *run.world.viewport();
        c.far = c.far.max((v.far_clip - far(next)).abs());
        if let Some((_, t)) = cam_target(run) {
            c.target.push((t - r.target[k + 1]).length());
        }
        let m7 = run.world.actor(h7).map_or(Vec3::NAN, |a| a.position);
        let miss7 = (m7 - f7[k + 1]).length();
        c.f7 = c.f7.max(miss7);
        if far(next) > 800.0 + 1e-3 && far(next) <= 1000.0 && (1000.0 - far(next)) / 200.0 < 1.0 {
            c.f7_in_use = c.f7_in_use.max(miss7);
        }
        if k + 1 >= calm_from {
            let cr = next.camera.unwrap();
            let miss = (v.eye - eye(next)).length();
            c.eye = c.eye.max(miss);
            c.eyes.push(miss);
            c.angle = c
                .angle
                .max(turn(v.angles.facing, cr.rotation[1]))
                .max((v.angles.tilt - cr.rotation[0]).abs());
        }
    }
    c
}

fn crossings() -> Vec<Rec> {
    AG40.iter()
        .map(|(run, replay)| {
            let dir = replay_dirs(run)
                .into_iter()
                .find(|(name, _)| name == replay)
                .unwrap_or_else(|| panic!("{run}/{replay}"))
                .1;
            rec(format!("{run}/{replay}"), &dir)
        })
        .collect()
}

/// AG.40's 12 crossings through `Transition 7-1` on the VM's camera, from
/// hop C: the target fed (AG.17's law-alone rows: far clip within 6.1e-5 ft,
/// eye within 1.44e-4 m, angles 9.2e-5 deg, camera 7's follow point 4.3e-6
/// m) and the scripts' own target (AG.17's hypothesis: eye 0.093-0.445 m,
/// target 0.42-0.59 m).
#[test]
#[ignore = "reads the Giants dump and the recordings"]
fn ag40s_crossings_through_the_plane() {
    let load = load();
    let corridor = walks()
        .into_iter()
        .find(|r| r.name.contains("corridor-W-00"))
        .unwrap();
    let mut worst = [0.0f32; 5];
    let mut own = [0.0f32; 3];
    for r in crossings() {
        let mut run = at_hop_c(&corridor, &r);
        let c = cross(&mut run, &r, true);
        println!(
            "{} (fed): shaken to record {}; far clip within {:.2e} ft, eye within {:.2e} m ({} records), angles {:.2e} deg, camera 7's follow point within {:.2e} m (while the view is not wholly camera 1's {:.2e} m)",
            r.name,
            c.shaken_to,
            c.far,
            c.eye,
            c.eyes.len(),
            c.angle,
            c.f7,
            c.f7_in_use
        );
        for (w, v) in worst
            .iter_mut()
            .zip([c.far, c.eye, c.angle, c.f7, c.f7_in_use])
        {
            *w = w.max(v);
        }
        let mut run = at_hop_c(&corridor, &r);
        let mut c = cross(&mut run, &r, false);
        let (mut t, mut e) = (c.target.clone(), c.eyes.clone());
        let row = [quantile(&mut e, 1.0), quantile(&mut t, 1.0), c.f7];
        println!(
            "{} (own target): eye within {:.4} m (median {:.4}), target within {:.4} m (median {:.4}), far clip within {:.2e} ft, camera 7's follow point {:.4} m",
            r.name,
            row[0],
            quantile(&mut e, 0.5),
            row[1],
            quantile(&mut t, 0.5),
            c.far,
            row[2]
        );
        c.target.clear();
        for (w, v) in own.iter_mut().zip(row) {
            *w = w.max(v);
        }
    }
    println!(
        "[{load}] AG.40's 12 crossings, target fed: far clip {:.2e} ft, eye {:.2e} m, angles {:.2e} deg, camera 7's follow point {:.2e} m ({:.2e} m while in use); the scripts' own target: eye {:.4} m, target {:.4} m, camera 7's follow point {:.4} m",
        worst[0], worst[1], worst[2], worst[3], worst[4], own[0], own[1], own[2]
    );
    // AG.17's: far clip 6.1e-5 ft, eye 1.44e-4 m, angles 9.2e-5 deg, camera
    // 7's follow point 4.3e-6 m; the hypothesis' own-target eye 0.093-0.445 m
    // and target 0.42-0.59 m.
    assert!(
        worst[0] <= 6.2e-5 && worst[1] < 1.44e-4 && worst[2] < 9.2e-5,
        "{worst:?}"
    );
    assert!(worst[3] < 4.3e-6 && worst[4] < 4.3e-6, "{worst:?}");
    assert!(own[0] < 0.093 && own[1] < 0.42, "{own:?}");
}
