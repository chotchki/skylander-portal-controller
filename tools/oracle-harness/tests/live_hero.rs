//! Live (openspec AG.8): the hero's own numbers as the live game holds them,
//! against what walks him. AF.8's fresh state (or whatever gameplay state
//! `GUAC_ORACLE_SAVESTATE` names) boots to Start Paused's pause, gets
//! `RECONNECT` and a fresh copy of the figure, and at F0 the hero's scale,
//! `CollisionInfo`, `ActorParameters` and his solid through his phantom are
//! read ([`hero::read_paused`]) and the frame shot; his sphere has to be the
//! one the `CollisionInfo` rule builds. Then a frame-locked walk along the
//! level ground the spawn sits in (the stick full left: the view's +90, the
//! world's 105) read at top speed and again stopped: the parameters he walks
//! by are the ones he stood by (motion type 2, 10,000 ft/s, no acceleration),
//! while his speed is 21.5 ft/s.
//!
//! Everything lands in `$GUAC_CAPTURE_DIR/BLUS30968/ag8-hero-<unix time>/`
//! (refused inside any work tree), `run.txt` opening on the identity line, the
//! reads beside it as JSON. Mac, against `GUAC_ORACLE_EXE` from
//! `GUAC_ORACLE_HOME`; reads `GUAC_GIANTS_DUMP`, `GUAC_ORACLE_FIGURE`,
//! `GUAC_CAPTURE_DIR` and `GUAC_ORACLE_SAVESTATE` from the env or `.env.dev`,
//! and skips when one is unset.
//!
//! `cargo test -p oracle-harness --test live_hero -- --ignored --nocapture`

#![cfg(unix)]

mod common;

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use common::{Record, hid_idle, load_average, screen_lock};
use guac_oracle::input::InputScript;
use oracle_harness::actors;
use oracle_harness::env;
use oracle_harness::hero::{self, FOOT, Hero};
use oracle_harness::identity::Identity;
use oracle_harness::launch::{self, BootOptions};
use oracle_harness::level::Shape;
use oracle_harness::pine::PineClient;
use oracle_harness::snapshot::Snapshot;
use skylander_core::SlotIndex;
use skylander_rpcs3_control::PortalDriver;

const FIGURE: &str = "GUAC_ORACLE_FIGURE";
const STATE: &str = "GUAC_ORACLE_SAVESTATE";
const WAIT: Duration = Duration::from_secs(120);
/// At rest 30 flips, the stick full left for 90, then at rest.
const WALK: &str = "(steps: [
    (at: 0, pads: {0: (), 1: ()}),
    (at: 30, pads: {0: (left: (x: 0, y: 128))}),
    (at: 120, pads: {0: ()}),
])";
/// Read at top speed (60 flips into the push) and stopped.
const READS: [(u64, &str); 2] = [(90, "walking"), (170, "stopped")];
/// Eruptor's speed, 21.5 ft/s.
const TOP: f64 = 21.5 * 0.3048;

fn sha256(path: &Path) -> String {
    use sha2::Digest;
    let digest = sha2::Sha256::digest(std::fs::read(path).unwrap());
    digest.iter().map(|b| format!("{b:02x}")).collect()
}

fn describe(h: &Hero) -> String {
    let c = h.collision.as_ref().map_or("none".into(), |c| {
        format!(
            "{:#010x}: properties {:#x} (shape {}), offset {:?}, angles {:?}, {} x {} x {} ft, \
             {} lb, rebound {} %",
            c.address,
            c.properties,
            c.shape(),
            c.position,
            c.angles,
            c.width,
            c.height,
            c.length,
            c.weight,
            c.rebound
        )
    });
    let p = h.parameters.as_ref().map_or("none".into(), |p| {
        format!(
            "{:#010x}: ground {} ft/s, accel {}, decel {}, turn {} deg/s; air {} ft/s, accel {}, \
             decel {}, turn {} deg/s; gravity terminal {} ft/s, accel {} ft/s2; jump {} / {} / \
             {}; motion type {}, tilts with ground {}, time scale {}",
            p.address,
            p.ground_max_speed,
            p.ground_acceleration,
            p.ground_deceleration,
            p.ground_turn_rate,
            p.air_max_speed,
            p.air_acceleration,
            p.air_deceleration,
            p.air_turn_rate,
            p.gravity_max_speed,
            p.gravity_acceleration,
            p.jump_max_speed,
            p.jump_acceleration,
            p.jump_initial_speed,
            p.motion_type,
            p.tilts_with_ground,
            p.object_time_scale
        )
    });
    format!(
        "{:?} (corpus {:#010x}, placement {:#010x}), scale {:?} %; CollisionInfo {c}; \
         ActorParameters {p}",
        h.name, h.corpus, h.placement, h.scale
    )
}

#[test]
#[ignore = "live: boots a gameplay GUAC_ORACLE_SAVESTATE, reads the hero's scale, CollisionInfo, ActorParameters and solid over PINE and walks him, outputs to GUAC_CAPTURE_DIR (Mac)"]
fn the_heros_solid_is_his_collision_info_and_his_parameters_dont_walk_him() {
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
            .join(format!("ag8-hero-{stamp}")),
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
    let mut pine: PineClient = common::pine_connect(&oracle.run_paths().pine_socket);

    // ---- F0 ----------------------------------------------------------------------
    let mut frame = actors::read_frame(&mut pine).unwrap();
    frame.flip = Some(f0);
    let at_rest = hero::read_paused(&mut pine, &frame).unwrap();
    rec.line(format!("F0 (flip {f0}): {}", describe(&at_rest)));
    std::fs::write(
        out.join("hero-f0.json"),
        serde_json::to_string_pretty(&at_rest).unwrap(),
    )
    .unwrap();
    let c = at_rest.collision.expect("the hero's CollisionInfo");
    let (radius, up) = c
        .sphere(at_rest.scale[1])
        .expect("the hero's CollisionInfo is a sphere");
    let solid = at_rest.solid.as_ref().expect("the hero's solid");
    let Shape::Transformed {
        translation, child, ..
    } = &solid.shape
    else {
        panic!("the hero's solid isn't transformed: {:?}", solid.shape);
    };
    let Shape::Sphere {
        radius: live_radius,
        ..
    } = **child
    else {
        panic!("the hero's solid isn't a sphere: {child:?}");
    };
    rec.line(format!(
        "  the rule's sphere: radius {radius:.6} m, centre {up:.6} m up ({} ft x {} % x {FOOT}); \
         his phantom's: radius {live_radius:.6} m, centre {:.6} m up, filter {:#x}, in the world \
         {}",
        c.width, at_rest.scale[1], translation[1], solid.filter_info, solid.in_world
    ));
    assert!((radius - live_radius).abs() < 1e-5 && (up - translation[1]).abs() < 1e-5);
    let shot = control.screenshot(&out, WAIT).unwrap();
    rec.line(format!(
        "F0 shot at flip {}: {} [{}, {}]",
        shot.flip,
        shot.path.file_name().unwrap().to_string_lossy(),
        screen_lock(),
        hid_idle()
    ));

    // ---- the walk ----------------------------------------------------------------------
    let loaded = control
        .load_script(&InputScript::from_ron(WALK).unwrap())
        .unwrap();
    rec.line(format!("the walk, offset 0 on flip {}", loaded.base));
    let mut walking_speed = 0.0;
    for (offset, what) in READS {
        let paused = control.pause_at(loaded.base + offset, WAIT).unwrap();
        let mut f = actors::read_frame(&mut pine).unwrap();
        f.flip = Some(paused.frames);
        let h = hero::read_paused(&mut pine, &f).unwrap();
        let a = f.hero().unwrap();
        let speed = f64::from(a.velocity[0]).hypot(f64::from(a.velocity[2]));
        rec.line(format!(
            "+{offset} (flip {}, game frame {}), {what}: at {:?} heading {:.4}, {speed:.5} m/s; {} \
             [{}]",
            paused.frames,
            f.game_frame,
            a.position,
            a.heading(),
            describe(&h),
            load_average()
        ));
        std::fs::write(
            out.join(format!("hero-{offset:03}.json")),
            serde_json::to_string_pretty(&h).unwrap(),
        )
        .unwrap();
        // What he walks by isn't what his parameters hold.
        assert_eq!(h.parameters, at_rest.parameters, "{what}");
        if what == "walking" {
            walking_speed = speed;
        }
    }
    let p = at_rest.parameters.expect("the hero's parameters");
    rec.line(format!(
        "walking at {walking_speed:.5} m/s ({:.4} ft/s) while his parameters hold {} ft/s and \
         motion type {}",
        walking_speed / f64::from(FOOT),
        p.ground_max_speed,
        p.motion_type
    ));
    assert!((walking_speed - TOP).abs() < 1e-3, "{walking_speed}");

    // ---- the end ---------------------------------------------------------------------
    drop(pine);
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
