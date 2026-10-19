//! AF.12 (openspec, design decision 9's acceptance) over recorded oracle runs;
//! nothing here boots the oracle.
//!
//! - `leave_one_out`: every reference group, AF.11's four (`GUAC_ORACLE_DIVERGENCE`,
//!   its output dir) and AF.12's walk set C (`GUAC_ORACLE_CALIBRATION`, a shot
//!   at every checkpoint), passes its leave-one-out at `guac_oracle::tolerance`'s
//!   defaults: each replay a candidate against the others, inside the OTHERS'
//!   horizon on every compared observable, on the RNG's identical states and on
//!   each shot's FLIP against the band of the others plus the frame floor. It
//!   also checks the AF.11 numbers each default cites, so a default that
//!   drifts from its measurement fails here: the walk's horizons, the hitching
//!   replay that sets the position floor's lower edge, and the two replays that
//!   fail the actor and animation counts at floor 0. AF.12's six controls are
//!   reported, and fail on the one that alone took the corner's other branch.
//! - `perturbations_fail`: every other walk replay under
//!   `GUAC_ORACLE_CALIBRATION` against that dir's walk set C (the same pauses
//!   and shots). The unperturbed controls (`walk_control`, the walk's own
//!   script under another name) pass both metrics; the stick pushed 3 flips
//!   late and the push turned 19.4 deg (x 173; the game's dead zone swallows a
//!   push under 14 deg off an axis, so a smaller turn changes nothing to catch)
//!   both fail the trajectory metric and the frame metric inside the horizon.
//!   AF.12's sensitivities are pinned rung by rung (the push moved 1 and 2
//!   flips, the stick at x 160, 161 and 162), so a floor or k loosened enough
//!   to move one fails here.
//!
//! Both read their dirs from the env or `.env.dev` and skip when unset.
//!
//! `cargo test -p oracle-harness --test calibration -- --ignored --nocapture`

use std::path::{Path, PathBuf};

use guac_oracle::tolerance;
use guac_oracle::trace::{self, TrajectoryTolerance};
use oracle_harness::divergence::calibrate::{self, CandidateCheck, GroupLoo, Reference};
use oracle_harness::divergence::{self, Report, Set};
use oracle_harness::env;

const DIVERGENCE: &str = "GUAC_ORACLE_DIVERGENCE";
const CALIBRATION: &str = "GUAC_ORACLE_CALIBRATION";

fn dir(key: &str) -> Option<PathBuf> {
    let dir = env::path(key);
    if dir.is_none() {
        eprintln!("skipping: {key} is unset (env or .env.dev)");
    }
    dir
}

fn loo(dir: &Path) -> Vec<GroupLoo> {
    let records = divergence::load_records(dir).unwrap();
    let groups = calibrate::leave_one_out(&records, Some(dir)).unwrap();
    for g in &groups {
        let mut text = String::new();
        g.write_text(&mut text);
        print!("{text}");
    }
    groups
}

#[test]
#[ignore = "reads AF.11's and AF.12's oracle runs, outside the repo"]
fn leave_one_out() {
    let Some(af11) = dir(DIVERGENCE) else {
        return;
    };
    let groups = loo(&af11);
    let names: Vec<String> = groups
        .iter()
        .map(|g| format!("{}-{}", g.script, g.set))
        .collect();
    assert_eq!(names, ["idle-A", "idle-B", "walk-A", "walk-B"]);
    for g in &groups {
        assert!(
            g.passed(),
            "{} set {} fails its leave-one-out",
            g.script,
            g.set
        );
        assert_eq!(g.labels.len(), 10);
    }

    // What the defaults cite. Walk set B: the hero and the camera agree within
    // the position floor through F0 + 120 (8.8 and 5.4 cm) and the gate's
    // corner parts them at 180 (1.01 m); the facing within the angle floor to
    // 120, parting at 180 (6.1e-3 deg); the camera's angles never part.
    let records = divergence::load_records(&af11).unwrap();
    let report = Report::build(&records, None).unwrap();
    let walk_b = report
        .groups
        .iter()
        .find(|g| g.script == "walk" && g.set == Set::B)
        .unwrap();
    let spread_at = |name: &str, offset: u64| {
        walk_b
            .observable(name)
            .unwrap()
            .spreads()
            .into_iter()
            .find(|(o, _)| *o == offset)
            .unwrap()
            .1
    };
    for (name, floor) in [
        ("hero position", tolerance::POSITION.floor),
        ("camera position", tolerance::POSITION.floor),
        ("hero heading", tolerance::ANGLE.floor),
    ] {
        let h = walk_b.observable(name).unwrap().horizon.unwrap();
        assert_eq!(
            (h.last_agreeing, h.parted_at),
            (Some(120), Some(180)),
            "{name}"
        );
        assert!(spread_at(name, 120) < floor && spread_at(name, 180) > floor);
    }
    let hero_120 = spread_at("hero position", 120);
    assert!((0.087..0.089).contains(&hero_120), "{hero_120}");
    // A frame of walking (6.553 m/s, AF.9) is past the floor.
    const { assert!(6.553 / 60.0 > tolerance::POSITION.floor) };
    let camera_angles = walk_b.observable("camera angles").unwrap();
    assert!(
        camera_angles
            .spreads()
            .iter()
            .all(|(_, s)| *s < tolerance::ANGLE.floor / 4.0)
    );
    // Idle, the hero and the camera are bit for bit at every checkpoint.
    for g in report.groups.iter().filter(|g| g.script == "idle") {
        for name in ["hero position", "camera position"] {
            assert!(
                g.observable(name)
                    .unwrap()
                    .spreads()
                    .iter()
                    .all(|(_, s)| *s == 0.0)
            );
        }
    }

    // The floors' lower edges, from the leave-one-out each one has to pass.
    // Positions: walk-B-06 (55.6 s for 30 s of flips) sits 6.8 cm past the
    // other nine at F0 + 120, where they agree to 2.0 cm, so a floor under
    // that fails it.
    let failing = |group: &divergence::Group, name: &str, t: TrajectoryTolerance| -> Vec<String> {
        trace::leave_one_out(&group.observable(name).unwrap().traces, &t)
            .unwrap()
            .iter()
            .zip(&group.labels)
            .filter(|(v, _)| !v.passed())
            .map(|(_, label)| label.clone())
            .collect()
    };
    let k = tolerance::K;
    assert_eq!(
        failing(
            walk_b,
            "hero position",
            TrajectoryTolerance { floor: 0.06, k }
        ),
        ["walk-B-06"]
    );
    assert!(failing(walk_b, "hero position", tolerance::POSITION).is_empty());
    // Actors and the animation LCG: at floor 0 one replay each strays by one
    // while the other nine agree (idle-B-09 at 1,260, idle-B-01 at 240).
    let idle_b = report
        .groups
        .iter()
        .find(|g| g.script == "idle" && g.set == Set::B)
        .unwrap();
    for (name, default, who) in [
        ("actors listed", tolerance::ACTORS, "idle-B-09"),
        ("skinned actors", tolerance::ACTORS, "idle-B-09"),
        (
            "animation LCG steps since F0",
            tolerance::ANIMATION_STEPS,
            "idle-B-01",
        ),
    ] {
        assert_eq!(
            failing(idle_b, name, TrajectoryTolerance { floor: 0.0, k }),
            [who],
            "{name}"
        );
        assert!(failing(idle_b, name, default).is_empty(), "{name}");
    }

    if let Some(af12) = dir(CALIBRATION) {
        let groups = loo(&af12);
        let walk_c = groups
            .iter()
            .find(|g| g.script == "walk" && g.set == Set::C)
            .expect("AF.12's dir holds the walk's set C");
        assert!(walk_c.passed(), "the walk's set C fails its leave-one-out");
        assert_eq!(walk_c.labels.len(), 10);
        // Frames gate inside the scene's horizon, F0 + 120: F0, 60 and 120.
        assert_eq!(walk_c.scene_horizon, Some(120));
        assert_eq!(walk_c.scene_others, vec![Some(120); 10]);
        assert_eq!(walk_c.frames.iter().filter(|f| f.inside).count(), 3);
        assert_eq!(
            walk_c.frames.len(),
            31,
            "a shot at F0 and at 30 checkpoints"
        );
        // The six controls aren't a reference: five took the corner's slide
        // and walk_control-C-04 alone turned free, so the other five agree past
        // the corner and it fails them there, at any floor a corner leaves
        // room for. A reference holds a branch only when it has two replays on
        // it (set C had two, walk-C-00 and -01).
        let controls = groups
            .iter()
            .find(|g| g.script == "walk_control")
            .expect("AF.12's dir holds the controls");
        assert!(!controls.passed());
        for o in controls.observables.iter().filter(|o| !o.failed.is_empty()) {
            let who: Vec<&str> = o.failed.iter().map(|f| f.label.as_str()).collect();
            assert_eq!(who, ["walk_control-C-04"], "{}", o.name);
        }
        assert!(
            controls
                .observables
                .iter()
                .find(|o| o.name == "hero position")
                .is_some_and(|o| o.failed.len() == 1)
        );
    }
}

#[test]
#[ignore = "reads AF.12's oracle runs, outside the repo"]
fn perturbations_fail() {
    let Some(af12) = dir(CALIBRATION) else {
        return;
    };
    let reference = Reference::load(&af12, "walk", Set::C).unwrap();
    let mut checks: Vec<CandidateCheck> = Vec::new();
    for record in divergence::load_records(&af12).unwrap() {
        if record.script == "walk" {
            continue;
        }
        let check = calibrate::check(&reference, &record, &af12).unwrap();
        let mut text = String::new();
        check.write_text(&mut text);
        print!("{text}");
        checks.push(check);
    }
    println!("== the ladder: per metric, failed (first checkpoint) or passed ==");
    for c in &checks {
        let trajectories: Vec<String> = c
            .trajectory_failures()
            .iter()
            .map(|o| {
                format!(
                    "{} {}",
                    o.name,
                    o.inside
                        .as_ref()
                        .and_then(|v| v.first_outside)
                        .map_or("?".into(), |f| f.to_string())
                )
            })
            .collect();
        let frames = c.frame_failures();
        let past = c.frame_failures_past();
        println!(
            "{}: trajectory {}; frames {}; past the horizon, frames {}",
            c.script,
            if trajectories.is_empty() {
                "passed".into()
            } else {
                trajectories.join(", ")
            },
            if frames.is_empty() {
                "passed".into()
            } else {
                format!("failed at {frames:?}")
            },
            if past.is_empty() {
                "passed".into()
            } else {
                format!("failed at {past:?}")
            }
        );
    }
    let controls: Vec<&CandidateCheck> = checks
        .iter()
        .filter(|c| c.script == "walk_control")
        .collect();
    assert!(!controls.is_empty(), "no control replays");
    for c in &controls {
        assert!(c.passed(), "the unperturbed {} fails", c.label);
    }
    for script in ["walk_shift60p3", "walk_stick60x173y0"] {
        let c = checks
            .iter()
            .find(|c| c.script == script)
            .unwrap_or_else(|| panic!("{script} wasn't run"));
        assert!(
            !c.trajectory_failures().is_empty(),
            "{script} passes the trajectory metric"
        );
        assert!(
            !c.frame_failures().is_empty(),
            "{script} passes the frame metric"
        );
    }

    // AF.12's sensitivities pinned rung by rung (`!name`: must pass): x 162's
    // hero sits 1.8 cm past the position band and x 161's frames 0.003 past
    // the frame limit, so a frame floor over ~0.033, a position floor over
    // ~0.118 or a k that widens set C's spread past either moves a rung.
    let ladder: &[(&str, &[&str], bool)] = &[
        ("walk_shift60m1", &["!hero position", "!hero heading"], true),
        ("walk_shift60p1", &["!hero position", "!hero heading"], true),
        ("walk_shift60m2", &["hero position", "hero heading"], true),
        ("walk_shift60p2", &["hero position", "hero heading"], true),
        (
            "walk_stick60x160y0",
            &["!hero position", "!hero heading"],
            false,
        ),
        (
            "walk_stick60x161y0",
            &["!hero position", "hero heading"],
            true,
        ),
        (
            "walk_stick60x162y0",
            &["hero position", "hero heading"],
            true,
        ),
    ];
    for &(script, names, frames) in ladder {
        let c = checks
            .iter()
            .find(|c| c.script == script)
            .unwrap_or_else(|| panic!("{script} wasn't run"));
        let failed: Vec<&str> = c
            .trajectory_failures()
            .iter()
            .map(|o| o.name.as_str())
            .collect();
        for name in names {
            match name.strip_prefix('!') {
                Some(name) => assert!(!failed.contains(&name), "{script}: {name} fails"),
                None => assert!(failed.contains(name), "{script}: {name} passes"),
            }
        }
        assert_eq!(
            !c.frame_failures().is_empty(),
            frames,
            "{script}: frames {:?}",
            c.frame_failures()
        );
    }
}
