//! The gameplay camera against the oracle's recordings (openspec AG.17's
//! Verify, on AG.13 and AG.40). `#[ignore]`d: the recordings are guest memory
//! under `GUAC_CAPTURE_DIR`, and AG.40's plane and look-at placements come off
//! the dump at `GUAC_GIANTS_DUMP` (each from the process env, else
//! `.env.dev`). Run with `cargo test -p guac-motion --release --test camera
//! -- --ignored --nocapture --test-threads 1`.
//!
//! Each camera is driven by the recording's own pivots and steps, one game
//! frame a record, so what's measured is the camera's law and not the
//! controller's drift (AG.18 and AG.31 run the two together). The followers
//! are AG.13's: every `Generic_FollowActor` on the watch, the follow points
//! and the camera target among them, decoded off `watch.json`.

mod oracle;

use guac_motion::camera::{CameraDef, FOV_Y, FollowPoint, GameCamera, LookFrom, TransitionPlane};
use guac_motion::seam::Vec3;
use guac_motion::tuning::FOOT;
use guac_oracle::recording::Frame;
use oracle::*;

/// AG.13's eight walks from AF's fresh state with every follower watched
/// (set W: AG.8's three level walks and AG.9's corridor walk, two replays
/// each).
const AG13: &str = "ag13-camera-1792480351";
/// Camera 0's follow point and the player's camera target, by their
/// placements (AG.13's review; the same in AG.40's runs).
const MAGIC: &str = "follow.33be0364";
const TARGET: &str = "follow.33bf25d8";
/// Camera 7's follow point in AG.40's runs, made at the rock slide's switch
/// before hop C's state was saved, so on the watch there.
const FOLLOW_7: &str = "follow.33bd1f40";

/// AG.40's crossings kept by its rule (no sheep in the contact planes): the
/// run and replay.
const AG40: [(&str, &str); 6] = [
    ("ag40-cross-1792512907", "g40_letgo-W-00"),
    ("ag40-cross-1792512907", "g40_letgo-W-01"),
    ("ag40-cross3-1792513842", "g40_full3-W-00"),
    ("ag40-cross3-1792513842", "g40_full3-W-01"),
    ("ag40-cross3-1792513842", "g40_half4-W-00"),
    ("ag40-cross3-1792513842", "g40_half4-W-01"),
];

fn pivot(f: &Frame) -> Vec3 {
    Vec3::from(f.hero.position)
}

fn eye(f: &Frame) -> Vec3 {
    Vec3::from(f.camera.expect("the watch held the camera").position)
}

fn far(f: &Frame) -> f32 {
    f.camera.expect("the watch held the camera").far_clip
}

fn quantile(v: &mut [f32], q: f32) -> f32 {
    v.sort_by(f32::total_cmp);
    v[((v.len() - 1) as f32 * q).round() as usize]
}

/// Record k + 1 is the game frame after k's: a lost frame's step is lost
/// with it, so a run stops there.
fn consecutive_from(frames: &[Frame], k: usize) -> bool {
    frames[k + 1].game_frame == frames[k].game_frame.wrapping_add(1)
}

/// gameplay-camera's Eruptor at the start, against every recording that
/// opens on AF's fresh state (his pivot at `213_Init_Location`'s waypoint,
/// camera 0): the eye where camera 0's definition puts it off his pivot.
/// AG.17's Verify: within 1e-3 m.
#[test]
#[ignore]
fn the_rest_pose_against_every_fresh_recording() {
    let spawn = Vec3::new(10.19027, 43.38267, -93.39406);
    let runs = [
        AG13,
        "ag8-level-1792424606",
        "ag8-motion-1792423086",
        "ag9-contacts-1792471160",
        "ag9-contacts-1792472400",
        "ag38-switch-1792509065",
    ];
    let mut worst = 0.0f32;
    let mut angles = [0.0f32; 2];
    let mut n = 0;
    for run in runs {
        for (name, dir) in replay_dirs(run) {
            let frames = frames(&dir);
            let f = &frames[0];
            if (pivot(f) - spawn).length() > 1e-3 || far(f) != 800.0 {
                continue;
            }
            let view = GameCamera::new(CameraDef::LEVEL_213_START, pivot(f)).view();
            let miss = (view.eye - eye(f)).length();
            let c = f.camera.unwrap();
            let heading = (view.heading() - c.rotation[1]).abs();
            let pitch = (view.pitch() - c.rotation[0]).abs();
            assert!(miss < 1e-3, "{run}/{name}: {miss}");
            worst = worst.max(miss);
            angles = [angles[0].max(heading), angles[1].max(pitch)];
            n += 1;
        }
    }
    eprintln!(
        "rest pose over {n} recordings' first records: eye within {worst:.2e} m, heading {:.2e}°, pitch {:.2e}°",
        angles[0], angles[1]
    );
    assert!(n >= 8, "{n}");
}

/// What a follow point does each record on camera 0, AG.13's followers
/// driving it: the eye off camera 0's own follow point, record by record,
/// and records outside the rock slide's shake.
struct Chain {
    frames: Vec<Frame>,
    magic: Vec<Vec3>,
    target: Vec<Vec3>,
    /// Records whose eye sits on camera 0's offset from the magic point
    /// (outside the rock slide's shake and before its switch).
    calm: Vec<bool>,
}

fn chain(dir: &std::path::Path) -> Chain {
    let frames = frames(dir);
    let followers = followers(dir, &frames);
    let magic = followers[MAGIC].clone();
    let target = followers[TARGET].clone();
    let def = CameraDef::LEVEL_213_START;
    // The rock slide's shake starts from nothing 0.25-0.40 s after the wake
    // (AG.13's review), so its first records sit under any threshold: the
    // half second before the first record it moves 1 mm is set apart too;
    // it lasts until the switch to camera 7.
    let off = |k: usize| (def.view(magic[k]).eye - eye(&frames[k])).length();
    let first = (0..frames.len()).find(|&k| far(&frames[k]) == 800.0 && off(k) > 1e-3);
    let calm = (0..frames.len())
        .map(|k| far(&frames[k]) == 800.0 && first.is_none_or(|s| k + 30 < s))
        .collect();
    Chain {
        frames,
        magic,
        target,
        calm,
    }
}

/// gameplay-camera's The camera follows on AG.13's eight walks, the law
/// alone: a follow point stepped from the recorded one at F0 toward 35 % of
/// the way from his pivot to the recorded camera target, a game frame a
/// record, against the recorded follow point; and the eye camera 0's
/// definition puts off the recorded follow point, against the recorded eye.
#[test]
#[ignore]
fn the_follow_point_on_ag13s_recorded_target() {
    let def = CameraDef::LEVEL_213_START;
    let mut worst_all = 0.0f32;
    let mut eye_all = 0.0f32;
    let (mut steps, mut shaken) = (0, 0);
    for (name, dir) in replay_dirs(AG13) {
        let c = chain(&dir);
        let mut follow = FollowPoint::at(c.magic[0]);
        let mut worst = 0.0f32;
        let mut raised = 0.0f32;
        for k in 0..c.frames.len() - 1 {
            if far(&c.frames[k + 1]) != 800.0 || !consecutive_from(&c.frames, k) {
                break;
            }
            let feet = pivot(&c.frames[k]);
            let run = if k > 0 {
                let d = feet - pivot(&c.frames[k - 1]);
                d.x.hypot(d.z) / FOOT
            } else {
                0.0
            };
            follow.step(
                feet + (c.target[k] - feet) * def.aim_share(),
                run,
                c.frames[k].clock.step(),
            );
            worst = worst.max((follow.point - c.magic[k + 1]).length());
            raised = raised.max(follow.speed());
            steps += 1;
        }
        let mut eye_worst = 0.0f32;
        for (k, f) in c.frames.iter().enumerate() {
            if far(f) != 800.0 {
                break;
            }
            if c.calm[k] {
                eye_worst = eye_worst.max((def.view(c.magic[k]).eye - eye(f)).length());
            } else {
                shaken += 1;
            }
        }
        eprintln!(
            "{name}: follow point within {worst:.2e} m of the recorded one (speed up to {raised:.4} ft); eye off it within {eye_worst:.2e} m"
        );
        worst_all = worst_all.max(worst);
        eye_all = eye_all.max(eye_worst);
    }
    eprintln!(
        "AG.13's 8 walks: {steps} steps on camera 0, the follow point within {worst_all:.2e} m, the eye within {eye_all:.2e} m on its offset ({shaken} records of the rock slide's shake set apart)"
    );
    assert!(worst_all < 1e-4, "{worst_all}");
    assert!(eye_all < 1e-4, "{eye_all}");
}

/// The whole camera on AG.13's eight walks, driven by the recorded pivots
/// and steps from F0's recorded follow point and target: the camera target
/// (a hypothesis), the follow point and the eye against the recorded ones,
/// on camera 0 up to the rock slide's switch.
#[test]
#[ignore]
fn the_camera_on_ag13s_recorded_pivots() {
    let def = CameraDef::LEVEL_213_START;
    let mut level = [0.0f32; 3];
    let mut corridor = [0.0f32; 3];
    for (name, dir) in replay_dirs(AG13) {
        let c = chain(&dir);
        let mut camera = GameCamera::resting(def, c.magic[0], c.target[0], pivot(&c.frames[0]));
        let (mut t, mut m, mut e) = (Vec::new(), Vec::new(), Vec::new());
        let mut ours = vec![camera.view().eye];
        for k in 0..c.frames.len() - 1 {
            if far(&c.frames[k + 1]) != 800.0 || !consecutive_from(&c.frames, k) {
                break;
            }
            camera.step(pivot(&c.frames[k]), c.frames[k].clock.step(), &[]);
            ours.push(camera.view().eye);
            t.push((camera.target() - c.target[k + 1]).length());
            m.push((camera.follow_point(0).unwrap() - c.magic[k + 1]).length());
            if c.calm[k + 1] {
                e.push((camera.view().eye - eye(&c.frames[k + 1])).length());
            }
        }
        let theirs: Vec<Vec3> = c.frames[..ours.len()].iter().map(eye).collect();
        for (stop, settled) in settles(&c.frames[..ours.len()], &ours)
            .into_iter()
            .zip(settles(&c.frames[..ours.len()], &theirs))
        {
            eprintln!(
                "  stop at record {}: ours settles {} frames on, {:+.3}°, {:.4} m; theirs {} frames, {:+.3}°, {:.4} m",
                stop.0, stop.1, stop.2, stop.3, settled.1, settled.2, settled.3
            );
            assert_eq!(stop.0, settled.0);
            assert!(stop.1.abs_diff(settled.1) <= 1, "{stop:?} {settled:?}");
        }
        let worst = [
            quantile(&mut t, 1.0),
            quantile(&mut m, 1.0),
            quantile(&mut e, 1.0),
        ];
        eprintln!(
            "{name} ({} steps): target within {:.4} m (median {:.4}), follow point {:.4} m, eye {:.4} m (median {:.4})",
            t.len(),
            worst[0],
            quantile(&mut t, 0.5),
            worst[1],
            worst[2],
            quantile(&mut e, 0.5)
        );
        let into = if name.contains("corridor") {
            &mut corridor
        } else {
            &mut level
        };
        for (a, b) in into.iter_mut().zip(worst) {
            *a = a.max(b);
        }
    }
    eprintln!(
        "level walks: target {:.4} m, follow point {:.4} m, eye {:.4} m; corridor walks: {:.4}, {:.4}, {:.4} m",
        level[0], level[1], level[2], corridor[0], corridor[1], corridor[2]
    );
    // The hypothesis's own numbers (AG.17's box: 2.64 and 2.89 cm, 0.92 and
    // 0.90 cm): a regression past them fails here.
    assert!(level[0] < 0.03 && level[2] < 0.015, "{level:?}");
    assert!(corridor[0] < 0.03 && corridor[2] < 0.015, "{corridor:?}");
}

/// Each stop the hero stands 90 records or more after (AG.13's settle):
/// its record, the records from it to the last whose eye moved more than
/// 1e-4 m, and where the eye rests at the stand's end, its trail angle off
/// the view and its distance from his pivot.
fn settles(frames: &[Frame], eyes: &[Vec3]) -> Vec<(usize, usize, f32, f32)> {
    let still = |k: usize| (pivot(&frames[k + 1]) - pivot(&frames[k])).length() <= 1e-7;
    let mut out = Vec::new();
    let mut k = 1;
    while k + 1 < frames.len() {
        if still(k) && !still(k - 1) {
            let mut j = k;
            while j + 1 < frames.len() && still(j) {
                j += 1;
            }
            if j - k >= 90 {
                let last = (k..j).rfind(|&i| (eyes[i + 1] - eyes[i]).length() > 1e-4);
                if let Some(last) = last {
                    let off = pivot(&frames[j]) - eyes[j];
                    let bearing = off.x.atan2(off.z).to_degrees();
                    let view = frames[j].camera.unwrap().rotation[1];
                    let angle = (bearing - view + 540.0).rem_euclid(360.0) - 180.0;
                    out.push((
                        k,
                        last + 1 - k,
                        angle,
                        (eyes[j] - pivot(&frames[j])).length(),
                    ));
                }
            }
            k = j + 1;
        } else {
            k += 1;
        }
    }
    out
}

/// `Transition 7-1` and its cameras off the dump: the box from its
/// `CollisionInfo` (41 x 14 x 35 ft at 92°, sitting on its placement), camera
/// 7 in front facing `Look_At_Target_0` (57 ft at 28°, aimed 6 ft up, far
/// 1,000 ft) and camera 1 behind facing `Look_At_Target_1` (50 ft at 35°,
/// aimed 3 ft up, far 800 ft), AG.13's and AG.40's definitions.
fn transition_7_1() -> TransitionPlane {
    with_placements_213(|p| {
        let at = |name: &str| {
            Vec3::from(
                p.actor(name)
                    .unwrap_or_else(|| panic!("{name}"))
                    .placement()
                    .placement
                    .init_matrix
                    .position,
            )
        };
        let plane = p.actor("Transition 7-1").expect("the plane");
        let c = plane.collision.expect("its box");
        let init = plane.placement().placement.init_matrix;
        let size = Vec3::new(c.width, c.height, c.length) * FOOT;
        TransitionPlane {
            centre: Vec3::from(init.position) + Vec3::Y * (size.y / 2.0),
            facing: init.rotation[1],
            size,
            front: CameraDef {
                id: 7,
                distance: 57.0,
                pitch: 28.0,
                look_from: LookFrom::Facing(at("Look_At_Target_0")),
                look_at: 6.0,
                fov_y: FOV_Y,
                far_clip: 1000.0,
            },
            back: CameraDef {
                id: 1,
                distance: 50.0,
                pitch: 35.0,
                look_from: LookFrom::Facing(at("Look_At_Target_1")),
                look_at: 3.0,
                fov_y: FOV_Y,
                far_clip: 800.0,
            },
        }
    })
}

/// gameplay-camera's A transition plane blends by where he stands, on
/// AG.40's six kept crossings: at every record the weight on camera 1 off
/// his pivot the record before, against the recorded far clip's (1,000 −
/// far) / 200.
#[test]
#[ignore]
fn the_planes_weight_on_ag40s_crossings() {
    let plane = transition_7_1();
    eprintln!(
        "Transition 7-1: centre {:?}, facing {}°, {:?} m",
        plane.centre, plane.facing, plane.size
    );
    let mut worst = 0.0f32;
    let (mut n, mut blended) = (0, 0);
    for (run, replay) in AG40 {
        let dir = replay_dirs(run)
            .into_iter()
            .find(|(name, _)| name == replay)
            .unwrap_or_else(|| panic!("{run}/{replay}"))
            .1;
        let frames = frames(&dir);
        let mut own = 0.0f32;
        for k in 1..frames.len() {
            let w = (1000.0 - far(&frames[k])) / 200.0;
            let ours = plane.weight(pivot(&frames[k - 1]));
            own = own.max((ours - w).abs());
            n += 1;
            if w > 0.0 && w < 1.0 {
                blended += 1;
            }
        }
        eprintln!("{run}/{replay}: the weight within {own:.2e}");
        worst = worst.max(own);
    }
    eprintln!("{n} records, {blended} inside a blend: the weight within {worst:.2e}");
    assert!(blended > 2000, "{blended}");
    assert!(worst < 1e-5, "{worst}");
}

/// The camera through `Transition 7-1` on AG.40's six kept crossings, driven
/// by the recorded pivots and steps from hop C's state (camera 7 at rest, its
/// follow point and the target where the recording found them), twice: its
/// follow points on the recorded camera target, the laws alone (the far
/// clip, the eye and the view's angles through the blend, which only hold
/// if camera 1's follow point, off the watch, sits on its aim its first
/// 0.5 s; camera 7's follow point standing still out of use and jumping
/// back onto its aim), and the whole camera on its own target (the
/// hypothesis's numbers). The eye from the frame after his eruptions' shake
/// has died.
#[test]
#[ignore]
fn the_camera_through_ag40s_plane() {
    let plane = transition_7_1();
    let planes = [plane];
    let front = plane.front;
    let mut worst = [0.0f32; 5];
    for (run, replay) in AG40 {
        let dir = replay_dirs(run)
            .into_iter()
            .find(|(name, _)| name == replay)
            .unwrap()
            .1;
        let frames = frames(&dir);
        let followers = followers(&dir, &frames);
        let (f7, target) = (&followers[FOLLOW_7], &followers[TARGET]);
        // The eruptions' shake: records wholly camera 7's whose eye is off
        // its offset from the watched follow point.
        let shaken: Vec<usize> = (0..frames.len())
            .filter(|&k| {
                far(&frames[k]) == 1000.0
                    && (front.view(f7[k]).eye - eye(&frames[k])).length() > 1e-3
            })
            .collect();
        let calm_from = shaken.last().map_or(0, |&k| k + 1);
        // The law alone on the recorded target, and the whole camera.
        let mut law = GameCamera::resting(front, f7[0], target[0], pivot(&frames[0]));
        let mut camera = law.clone();
        let (mut far_miss, mut ang) = (0.0f32, 0.0f32);
        let (mut e_law, mut e, mut t, mut m7) = (Vec::new(), Vec::new(), Vec::new(), 0.0f32);
        for k in 0..frames.len() - 1 {
            if !consecutive_from(&frames, k) {
                break;
            }
            let (feet, step) = (pivot(&frames[k]), frames[k].clock.step());
            law.step_on(feet, step, &planes, Some(target[k]));
            camera.step(feet, step, &planes);
            let next = &frames[k + 1];
            far_miss = far_miss.max((law.view().far_clip - far(next)).abs());
            t.push((camera.target() - target[k + 1]).length());
            if law.camera() != Some(1) {
                m7 = m7.max((law.follow_point(7).unwrap() - f7[k + 1]).length());
            }
            if k + 1 >= calm_from {
                let c = next.camera.unwrap();
                let view = law.view();
                e_law.push((view.eye - eye(next)).length());
                e.push((camera.view().eye - eye(next)).length());
                let turn = (view.heading() - c.rotation[1]).rem_euclid(360.0);
                ang = ang
                    .max(turn.min(360.0 - turn))
                    .max((view.pitch() - c.rotation[0]).abs());
            }
        }
        let worst_law = quantile(&mut e_law, 1.0);
        let worst_e = quantile(&mut e, 1.0);
        eprintln!(
            "{run}/{replay}: shaken to record {calm_from}; on the recorded target: far clip within {far_miss:.2e} ft, eye within {worst_law:.2e} m, view's angles {ang:.2e}°, camera 7's follow point {m7:.2e} m; whole camera: eye within {worst_e:.4} m (median {:.4}), target {:.4} m (median {:.4})",
            quantile(&mut e, 0.5),
            quantile(&mut t, 1.0),
            quantile(&mut t, 0.5),
        );
        for (w, v) in worst
            .iter_mut()
            .zip([far_miss, worst_law, ang, m7, worst_e])
        {
            *w = w.max(v);
        }
    }
    eprintln!(
        "AG.40's 12 crossings on the recorded target: far clip {:.2e} ft, eye {:.2e} m, angles {:.2e}°, camera 7's follow point {:.2e} m; the whole camera's eye {:.4} m",
        worst[0], worst[1], worst[2], worst[3], worst[4]
    );
    assert!(
        worst[0] < 1e-3 && worst[1] < 3e-4 && worst[2] < 2e-4,
        "{worst:?}"
    );
    assert!(worst[3] < 1e-5, "{worst:?}");
    assert!(worst[4] < 0.5, "{worst:?}");
}

/// Decision 7's step-size term on the camera: the controller and the camera
/// over open ground from AG.13's level walks' own scripts, once at each
/// recording's own frame times and once at 1/60 s, against when the
/// original's camera came to rest after the last let-go (AG.13's settle,
/// off the recorded eye). The camera target's caps scale with the frame's
/// step, so the emulator's frame-time jitter pulls its velocity point back
/// on each short frame: at the recordings' own steps the settle is the
/// original's, at 1/60 s it comes sooner.
#[test]
#[ignore]
fn the_step_size_term() {
    use guac_motion::runner;
    use guac_motion::scenarios::{eruptor, open_ground};
    use guac_oracle::input::InputScript;
    let world =
        guac_motion::parry::ParryWorld::new(&guac_motion::scenarios::bodies(&open_ground(), &[]))
            .unwrap();
    for (name, dir) in replay_dirs(AG13) {
        if !name.contains("_full") {
            continue;
        }
        let frames = frames(&dir);
        let theirs: Vec<Vec3> = frames.iter().map(eye).collect();
        let Some(&(stop, their_frames, _, _)) = settles(&frames, &theirs).last() else {
            continue;
        };
        let script =
            InputScript::from_ron(&std::fs::read_to_string(dir.join("script.ron")).unwrap())
                .unwrap();
        let mut ours = Vec::new();
        for own in [true, false] {
            let mut hero = eruptor(&world, Vec3::new(0.0, 1.0, 0.0), 0.0);
            let mut camera = GameCamera::new(CameraDef::LEVEL_213_START, hero.pivot);
            let (mut eyes, mut pivots) = (vec![camera.view().eye], vec![hero.pivot]);
            // Record k's step moves it to record k + 1, as in the camera's
            // own runs above.
            for (k, frame) in frames[..frames.len() - 1].iter().enumerate() {
                let step = if own { frame.clock.step() } else { 1.0 / 60.0 };
                let input = runner::input(&script, k as u64, camera.view().heading());
                let feet = hero.pivot;
                hero.step(&world, &input, step, &[]);
                camera.step(feet, step, &[]);
                eyes.push(camera.view().eye);
                pivots.push(hero.pivot);
            }
            let still = (stop - 30..eyes.len() - 1)
                .find(|&i| (pivots[i + 1] - pivots[i]).length() <= 1e-7)
                .unwrap();
            let last = (still..eyes.len() - 1)
                .rfind(|&i| (eyes[i + 1] - eyes[i]).length() > 1e-4)
                .unwrap();
            ours.push(last + 1 - still);
        }
        eprintln!(
            "{name}: the original's camera rests {their_frames} frames after he stops; ours at its frame times {}, at 1/60 s {}",
            ours[0], ours[1]
        );
        assert!(
            ours[0].abs_diff(their_frames) <= 1,
            "{ours:?} {their_frames}"
        );
        assert!(ours[1] < ours[0], "{ours:?}");
    }
}
