//! Live (openspec AG.7): level 213 as the live game holds it, against the
//! disc. AF.8's fresh state (or whatever gameplay state
//! `GUAC_ORACLE_SAVESTATE` names) boots to Start Paused's pause, gets
//! `RECONNECT`, and there, at F0:
//!
//! - every one of the level's placements is read where its data section sits
//!   in guest memory ([`level::section_base`] from the actors' placements) and
//!   its live `_activityState` set beside the disc's and beside what
//!   guac-formats' start rule ([`at_start`]) says for the hero standing where he
//!   does: templates stay templates; a ranged placement is active exactly when
//!   the hero is inside its range (bar the ones its own script took off, each
//!   named in the record); one with no range is the scripts';
//! - the collision mesh the live `hkpWorld` queries is read and has to equal
//!   AG.2's read of the disc bit for bit (vertices, indices with their welding
//!   words, materials, the body's translation);
//! - every simulated actor's solid is read through its phantom: the gate door's
//!   box has to run its length the way guac-formats' `solid_axes` says, and a
//!   shapeless one (the shared 1 m sphere) has to stay out of the world;
//! - a `SCREENSHOT` of the fresh state.
//!
//! Then a fresh copy of the figure and a frame-locked walk into the gate's
//! door: at rest, the stick at the view's +75 deg (the world's +x) for 40
//! flips, then at -15 deg (the world's +z, straight at the door) for 180 flips,
//! read pressing against it and again 70 flips after letting go; the stop
//! recorded as the hero's pivot to the door's centre plane and as the gap left
//! once both shapes' sizes are taken off, and the placements read again with the
//! hero there (the rule at a second position). Then back the other way into the
//! gourd by the spawn (a solid sphere in the other collision layer from the
//! door's), the gap between the two spheres recorded the same way.
//!
//! Everything lands in `$GUAC_CAPTURE_DIR/BLUS30968/ag7-level-<unix time>/`
//! (refused inside any work tree), `run.txt` opening on the identity line;
//! what's read out of guest memory is written beside it as JSON. Mac, against
//! `GUAC_ORACLE_EXE` from `GUAC_ORACLE_HOME`; reads `GUAC_GIANTS_DUMP`,
//! `GUAC_ORACLE_FIGURE`, `GUAC_CAPTURE_DIR` and `GUAC_ORACLE_SAVESTATE` (a
//! gameplay state this build accepts, booted in a home holding its trophy
//! files) from the env or `.env.dev`, and skips when one is unset.
//!
//! `cargo test -p oracle-harness --test live_level -- --ignored --nocapture`

#![cfg(unix)]

mod common;

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use common::{Record, hid_idle, load_average, screen_lock};
use guac_formats::collision;
use guac_formats::iga::Archive;
use guac_formats::igz::Igz;
use guac_formats::igz::layouts::Shape as DiscShape;
use guac_formats::level::{AtStart, FOOT, Placements, at_start, solid_axes};
use guac_oracle::input::InputScript;
use oracle_harness::actors::{self, ActorFrame};
use oracle_harness::env;
use oracle_harness::identity::Identity;
use oracle_harness::launch::{self, BootOptions};
use oracle_harness::level::{self, Kind, LivePlacement, LiveSolid, Shape};
use oracle_harness::pine::PineClient;
use oracle_harness::snapshot::Snapshot;
use skylander_core::SlotIndex;
use skylander_rpcs3_control::PortalDriver;

const FIGURE: &str = "GUAC_ORACLE_FIGURE";
const STATE: &str = "GUAC_ORACLE_SAVESTATE";
const WAIT: Duration = Duration::from_secs(120);
const LEVEL_213: &str = "level/level_213_tutotial.bld";
const DOOR: &str = "Breakable_Stone_Wall_Castle_206(1)";
/// At rest 60 flips, then the stick at the view's +75 deg (x 0, y 70: the
/// world's heading 89.85 with the view at 15) for 40, then at -15 deg (x 186,
/// y 0: heading -0.15, straight at the door) for 180, then at rest.
const DOOR_WALK: &str = "(steps: [
    (at: 0, pads: {0: (), 1: ()}),
    (at: 60, pads: {0: (left: (x: 0, y: 70))}),
    (at: 100, pads: {0: (left: (x: 186, y: 0))}),
    (at: 280, pads: {0: ()}),
])";
/// Read pressing against the door, then after letting go.
const PRESSING: u64 = 270;
const RELEASED: u64 = 350;
/// The hero's sphere has to have stopped within this of touching the door's
/// box (both sizes and convex radii taken off), and not inside it.
const STOP_SLACK: f64 = 0.2;
/// The gourd by the spawn (a solid sphere in collision layer 3, where the
/// door is in layer 1), and the walk back into it from the door: the stick
/// at the view's -174 deg (x 170, y 255: the world's heading -159, straight
/// at it) for 200 flips, then at rest.
const GOURD: &str = "gourd_01(2)";
const GOURD_WALK: &str = "(steps: [
    (at: 0, pads: {0: (left: (x: 170, y: 255))}),
    (at: 200, pads: {0: ()}),
])";
const GOURD_READ: u64 = 260;

fn sha256(path: &Path) -> String {
    use sha2::Digest;
    let digest = sha2::Sha256::digest(std::fs::read(path).unwrap());
    digest.iter().map(|b| format!("{b:02x}")).collect()
}

fn write_json<T: serde::Serialize>(out: &Path, name: &str, value: &T) {
    std::fs::write(out.join(name), serde_json::to_string_pretty(value).unwrap()).unwrap();
}

/// Level 213's `level.bld` off the dump.
fn level_bld() -> Vec<u8> {
    let root = env::path(env::GIANTS_DUMP).unwrap();
    let bytes = std::fs::read(root.join("PS3_GAME/USRDIR").join(LEVEL_213)).unwrap();
    let arc = Archive::parse(&bytes).unwrap();
    let entry = arc.find("level.bld").unwrap();
    arc.read(entry).unwrap().into_owned()
}

/// One of the disc's placements, as the live read and the rule need it.
struct DiscPlacement {
    offset: u32,
    kind: Kind,
    name: String,
    state: u32,
    range: f32,
    /// The rule's input: the disc's `Placement`, by index into `Placements`.
    actor: Option<usize>,
    emitter: Option<usize>,
    light: Option<usize>,
}

fn heading_deg(v: [f32; 3]) -> f64 {
    f64::from(v[0]).atan2(f64::from(v[2])).to_degrees()
}

fn dot(a: [f32; 3], b: [f32; 3]) -> f64 {
    (0..3).map(|i| f64::from(a[i]) * f64::from(b[i])).sum()
}

/// What the rule says against what the game has, for every placement.
struct Tally {
    rows: BTreeMap<(String, &'static str, u32), usize>,
    /// Ranged placements whose live state the rule doesn't give.
    exceptions: Vec<String>,
}

fn rule_name(r: Option<AtStart>) -> &'static str {
    match r {
        Some(AtStart::Template) => "template",
        Some(AtStart::Active) => "in range",
        Some(AtStart::Inactive) => "out of range",
        Some(AtStart::ByScript) => "no range",
        None => "no rule",
    }
}

fn judge(
    disc: &[DiscPlacement],
    placements: &Placements<'_>,
    live: &[LivePlacement],
    hero: [f32; 3],
) -> Tally {
    let mut rows = BTreeMap::new();
    let mut exceptions = Vec::new();
    for (d, l) in disc.iter().zip(live) {
        let p = if let Some(i) = d.actor {
            placements.actors[i].placement()
        } else if let Some(i) = d.emitter {
            &placements.emitters[i].1.placement
        } else {
            &placements.lights[d.light.unwrap()].1.placement
        };
        let rule = at_start(p, hero);
        let ok = match rule {
            Some(AtStart::Template) => l.activity_state == 5,
            Some(AtStart::Active) => l.activity_state == 1,
            Some(AtStart::Inactive) => l.activity_state == 2,
            Some(AtStart::ByScript) => true,
            None => false,
        };
        if !ok {
            let dist = (0..3)
                .map(|i| (p.placement.init_matrix.position[i] - hero[i]).powi(2))
                .sum::<f32>()
                .sqrt();
            exceptions.push(format!(
                "{} {:?}: disc {}, rule {}, live {} ({:.2} m off, range {} ft = {:.2} m)",
                d.kind.class(),
                d.name,
                d.state,
                rule_name(rule),
                l.activity_state,
                dist,
                d.range,
                d.range * FOOT
            ));
        }
        *rows
            .entry((d.kind.class().to_owned(), rule_name(rule), l.activity_state))
            .or_default() += 1;
    }
    Tally { rows, exceptions }
}

fn print_tally(rec: &mut Record, what: &str, t: &Tally) {
    rec.line(format!("{what}: rule against live state, count each:"));
    for ((class, rule, state), n) in &t.rows {
        rec.line(format!("  {class}, {rule}, live {state}: {n}"));
    }
    rec.line(format!("  exceptions ({}):", t.exceptions.len()));
    for e in &t.exceptions {
        rec.line(format!("    {e}"));
    }
}

/// Where a solid's primitive is centred: the phantom's transform applied to
/// its convex transform's translation.
fn primitive_centre(s: &LiveSolid) -> [f32; 3] {
    let t = match &s.shape {
        Shape::Transformed { translation, .. } => *translation,
        _ => [0.0; 3],
    };
    std::array::from_fn(|i| s.translation[i] + (0..3).map(|j| s.rotation[j][i] * t[j]).sum::<f32>())
}

fn solid_named<'a>(solids: &'a [LiveSolid], name: &str) -> &'a LiveSolid {
    let found: Vec<_> = solids
        .iter()
        .filter(|s| s.name.as_deref() == Some(name))
        .collect();
    assert_eq!(found.len(), 1, "{name}: {} solids", found.len());
    found[0]
}

#[test]
#[ignore = "live: boots a gameplay GUAC_ORACLE_SAVESTATE, reads level 213's placements, collision and solids over PINE against the disc and walks Eruptor into the gate's door, outputs to GUAC_CAPTURE_DIR (Mac)"]
fn level_213_live_is_the_discs_by_the_start_rule_and_the_door_stops_the_hero() {
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

    // ---- the disc ------------------------------------------------------------
    let data = level_bld();
    let igz = Igz::parse(&data).unwrap();
    let placements = Placements::read(&igz).unwrap();
    let mut disc = Vec::new();
    for (i, a) in placements.actors.iter().enumerate() {
        let p = a.placement();
        assert_eq!(igz.objects()[a.index].at.section(), 0);
        disc.push(DiscPlacement {
            offset: igz.objects()[a.index].at.offset(),
            kind: Kind::Actor,
            name: a.name().unwrap_or("").to_owned(),
            state: p.placement.activity_state,
            range: p.activation_range,
            actor: Some(i),
            emitter: None,
            light: None,
        });
    }
    for (i, (index, e)) in placements.emitters.iter().enumerate() {
        assert_eq!(igz.objects()[*index].at.section(), 0);
        disc.push(DiscPlacement {
            offset: igz.objects()[*index].at.offset(),
            kind: Kind::Emitter,
            name: e.placement.placement.name.unwrap_or("").to_owned(),
            state: e.placement.placement.activity_state,
            range: e.placement.activation_range,
            actor: None,
            emitter: Some(i),
            light: None,
        });
    }
    for (i, (index, l)) in placements.lights.iter().enumerate() {
        assert_eq!(igz.objects()[*index].at.section(), 0);
        disc.push(DiscPlacement {
            offset: igz.objects()[*index].at.offset(),
            kind: Kind::Light,
            name: l.placement.placement.name.unwrap_or("").to_owned(),
            state: l.placement.placement.activity_state,
            range: l.placement.activation_range,
            actor: None,
            emitter: None,
            light: Some(i),
        });
    }
    let mut seen: BTreeMap<&str, usize> = BTreeMap::new();
    for d in &disc {
        *seen.entry(&d.name).or_default() += 1;
    }
    let unique: BTreeMap<String, u32> = disc
        .iter()
        .filter(|d| d.kind == Kind::Actor && seen[d.name.as_str()] == 1)
        .map(|d| (d.name.clone(), d.offset))
        .collect();
    let objects: Vec<(u32, Kind)> = disc.iter().map(|d| (d.offset, d.kind)).collect();
    let body = collision::worlds(&igz)
        .filter_map(|(_, w)| w.unwrap().collision.body)
        .next()
        .unwrap();
    let door = placements.actor(DOOR).unwrap();
    let door_ci = door.collision.unwrap();
    let mesh_shapes = placements
        .actors
        .iter()
        .filter(|a| a.collision.is_some_and(|c| c.shape() == DiscShape::Mesh))
        .count();

    // ---- the run -------------------------------------------------------------
    let before = Snapshot::take(&live.dev_dirs).unwrap();
    let figure_hash = sha256(&figure);
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs();
    let out: PathBuf = rrc_dump::output::refuse_repo_output(
        &capture_root
            .join("BLUS30968")
            .join(format!("ag7-level-{stamp}")),
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
    rec.line(format!(
        "disc: {} placements ({} actors, {} emitters, {} lights), all in section 0; {} actor \
         names the disc lists once; {mesh_shapes} mesh-type CollisionInfo",
        disc.len(),
        placements.actors.len(),
        placements.emitters.len(),
        placements.lights.len(),
        unique.len()
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
    let mut pine: PineClient = common::pine_connect(&oracle.run_paths().pine_socket);

    // ---- F0: the placements ------------------------------------------------------
    let t = Instant::now();
    let mut frame: ActorFrame = actors::read_frame(&mut pine).unwrap();
    frame.flip = Some(f0);
    frame
        .write(&out.join(format!("actors-{f0:05}.json")))
        .unwrap();
    let hero = frame.hero().unwrap().clone();
    let section = level::section_base(&frame, &unique).unwrap();
    let live0 = level::read_placements(&mut pine, section.base, &objects).unwrap();
    rec.line(format!(
        "F0: {}; hero at {:?}; section 0 at {:#010x} ({} actors agree, {} named ones elsewhere); \
         {} placements read in {} ms",
        frame.summary(),
        hero.position,
        section.base,
        section.votes,
        section.dissent,
        live0.len(),
        t.elapsed().as_millis()
    ));
    write_json(&out, &format!("placements-{f0:05}.json"), &live0);
    for (d, l) in disc.iter().zip(&live0) {
        assert_eq!(l.name.as_deref(), Some(d.name.as_str()), "{:#x}", d.offset);
    }
    let tally = judge(&disc, &placements, &live0, hero.position);
    print_tally(&mut rec, "F0", &tally);
    let state_counts: BTreeMap<u32, usize> = live0.iter().fold(BTreeMap::new(), |mut m, l| {
        *m.entry(l.activity_state).or_default() += 1;
        m
    });
    rec.line(format!("F0 live states: {state_counts:?}"));
    // Templates stay templates; a ranged placement is active exactly in range,
    // or taken off by its own script (live 3), which the record names.
    for e in &tally.exceptions {
        assert!(
            e.contains("live 3"),
            "a placement the rule doesn't explain: {e}"
        );
    }
    let corpora: BTreeSet<u32> = frame.actors.iter().map(|a| a.corpus).collect();
    for l in &live0 {
        if l.kind == Kind::Actor {
            assert_eq!(
                l.activity_state == 1,
                l.corpus.is_some_and(|c| corpora.contains(&c)),
                "{:?}: state {} and corpus {:?}",
                l.name,
                l.activity_state,
                l.corpus
            );
        }
    }
    rec.line(
        "F0: every active actor's corpus is on the game's actor list and no other placement has one",
    );
    let clones: Vec<String> = frame
        .actors
        .iter()
        .filter(|a| !live0.iter().any(|l| l.address == a.placement))
        .map(|a| format!("{} at {:?}", a.name.as_deref().unwrap_or("?"), a.position))
        .collect();
    rec.line(format!(
        "F0: {} simulated actors whose placement isn't the level file's (clones and the \
         game's own): {}",
        clones.len(),
        clones.join("; ")
    ));

    // ---- F0: the collision ---------------------------------------------------------
    let t = Instant::now();
    let c = level::read_collision(&mut pine).unwrap();
    rec.line(format!(
        "F0 collision: world {:#010x}, {} fixed bodies, level body {:#010x} (motion {}), \
         query {:#010x}, mesh {:#010x}: {} vertices, {} triangles, {} materials, radius {}, \
         welding type {}, translation {:?}; read in {} ms",
        c.world,
        c.fixed_bodies,
        c.body,
        c.motion_type,
        c.query,
        c.mesh,
        c.vertices.len(),
        c.triangles.len(),
        c.materials.len(),
        c.radius,
        c.welding_type,
        c.translation,
        t.elapsed().as_millis()
    ));
    write_json(&out, "collision.json", &c);
    let bits =
        |v: &[[f32; 3]]| -> Vec<[u32; 3]> { v.iter().map(|p| p.map(f32::to_bits)).collect() };
    assert_eq!(bits(&c.vertices), bits(&body.mesh.vertices), "vertices");
    let disc_tris: Vec<[u16; 4]> = body
        .mesh
        .triangles
        .iter()
        .map(|t| [t.indices[0], t.indices[1], t.indices[2], t.welding])
        .collect();
    assert_eq!(c.triangles, disc_tris, "triangles");
    assert_eq!(c.materials, body.mesh.materials, "materials");
    assert_eq!(
        c.translation.map(f32::to_bits),
        body.translation.map(f32::to_bits),
        "translation"
    );
    assert_eq!(
        (c.motion_type, c.welding_type),
        (level::MOTION_FIXED, body.mesh.welding_type)
    );
    assert_eq!(c.radius.to_bits(), body.mesh.radius.to_bits());
    assert_eq!(
        c.rotation,
        [1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0]
    );
    rec.line(format!(
        "F0 collision: equal to AG.2's read of the disc bit for bit ({} vertices, {} \
         triangles with their welding words, {} materials, the translation, radius and \
         welding type), the body fixed and unrotated",
        body.mesh.vertices.len(),
        body.mesh.triangles.len(),
        body.mesh.materials.len()
    ));

    // ---- F0: the solids ---------------------------------------------------------------
    let solids = level::read_solids(&mut pine, &frame).unwrap();
    write_json(&out, "solids-f0.json", &solids);
    let shared: BTreeSet<u32> = solids
        .iter()
        .filter_map(|s| match s.shape.primitive() {
            Shape::Sphere { radius, address } if *radius == 1.0 => Some(*address),
            _ => None,
        })
        .collect();
    let (out_of_world, in_world): (Vec<&LiveSolid>, Vec<&LiveSolid>) =
        solids.iter().partition(|s| !s.in_world);
    rec.line(format!(
        "F0 solids: {} simulated actors with a phantom, {} in the world, {} out of it; \
         1 m spheres at {:?}",
        solids.len(),
        in_world.len(),
        out_of_world.len(),
        shared
            .iter()
            .map(|a| format!("{a:#010x}"))
            .collect::<Vec<_>>()
    ));
    for s in &solids {
        let shapeless = matches!(s.shape.primitive(), Shape::Sphere { radius, address } if *radius == 1.0 && shared.len() == 1 && shared.contains(address));
        rec.line(format!(
            "  {:?}: {} world {:#x}, broadphase {:#x}, filter {:#x}, CollisionInfo {:?} \
             _properties {:?}, {:?}",
            s.name,
            if s.in_world { "in" } else { "out of" },
            s.world,
            s.broadphase_id,
            s.filter_info,
            s.collision.map(|c| format!("{c:#010x}")),
            s.properties.map(|p| format!("{p:#x}")),
            s.shape
        ));
        assert_eq!(
            shapeless, !s.in_world,
            "{:?}: shapeless {shapeless}, in the world {}",
            s.name, s.in_world
        );
        if !s.in_world {
            assert_eq!((s.world, s.broadphase_id), (0, 0), "{:?}", s.name);
        }
        // The live CollisionInfo's shape bits say which: 0 (none) out of the
        // world, a primitive in it.
        if let Some(p) = s.properties {
            assert_eq!(p & 7 == 0, !s.in_world, "{:?}: _properties {p:#x}", s.name);
        }
    }
    let by_props: BTreeMap<(bool, u32), usize> = solids.iter().fold(BTreeMap::new(), |mut m, s| {
        *m.entry((s.in_world, s.properties.unwrap_or(u32::MAX)))
            .or_default() += 1;
        m
    });
    rec.line(format!(
        "F0 solids by (in the world, live _properties): {:?}",
        by_props
            .iter()
            .map(|((w, p), n)| format!("{w} {p:#x}: {n}"))
            .collect::<Vec<_>>()
    ));
    let d = solid_named(&solids, DOOR);
    let Shape::Box {
        half_extents,
        radius,
    } = d.shape.primitive()
    else {
        panic!("the door's solid isn't a box: {:?}", d.shape);
    };
    let want = [door_ci.width, door_ci.height, door_ci.length].map(|f| (f * FOOT - 0.1) / 2.0);
    assert!(
        (0..3).all(|i| (half_extents[i] - want[i]).abs() < 1e-5),
        "door half extents {half_extents:?}, want {want:?}"
    );
    let live_axes = d.primitive_axes();
    let disc_axes = solid_axes(door.placement(), &door_ci);
    for i in 0..3 {
        let agree = dot(live_axes[i], disc_axes[i]).abs();
        assert!(
            agree > 0.9999,
            "door axis {i}: live {:?}, rule {:?}",
            live_axes[i],
            disc_axes[i]
        );
    }
    rec.line(format!(
        "F0 door: box half extents {half_extents:?} + {radius} convex (CollisionInfo {} x {} x \
         {} ft), centre {:?}; its width runs {:?}, its height {:?}, its length {:?} in the \
         world (heading {:.2} deg), as solid_axes reads the disc",
        door_ci.width,
        door_ci.height,
        door_ci.length,
        d.translation,
        live_axes[0],
        live_axes[1],
        live_axes[2],
        heading_deg(live_axes[2])
    ));
    let hero_solid = solids
        .iter()
        .find(|s| s.corpus == hero.corpus)
        .expect("the hero's solid");
    let Shape::Sphere {
        radius: hero_radius,
        ..
    } = hero_solid.shape.primitive()
    else {
        panic!("the hero's solid isn't a sphere: {:?}", hero_solid.shape);
    };
    rec.line(format!(
        "F0 hero solid: {:?}, filter {:#x}",
        hero_solid.shape, hero_solid.filter_info
    ));

    let shot = control.screenshot(&out, WAIT).unwrap();
    rec.line(format!(
        "F0 shot at flip {}: {} [{}, {}]",
        shot.flip,
        shot.path.file_name().unwrap().to_string_lossy(),
        screen_lock(),
        hid_idle()
    ));

    // ---- the walk into the door ------------------------------------------------------
    let copy = oracle.run_paths().tmp.join("figure.sky");
    std::fs::copy(&figure, &copy).unwrap();
    control
        .driver()
        .load(SlotIndex::new(0).unwrap(), &copy)
        .unwrap();
    let loaded = control
        .load_script(&InputScript::from_ron(DOOR_WALK).unwrap())
        .unwrap();
    let base = loaded.base;
    rec.line(format!(
        "LOAD of a fresh figure copy; the door walk, offset 0 on flip {base}"
    ));
    // The door's centre plane and the way it faces: its width axis.
    let normal = live_axes[0];
    let centre = d.translation;
    let thickness = f64::from(half_extents[0] + radius);
    let gap_of = |p: [f32; 3]| -> (f64, f64) {
        let along = (0..3)
            .map(|i| f64::from(p[i] - centre[i]) * f64::from(normal[i]))
            .sum::<f64>()
            .abs();
        (along, along - thickness - f64::from(*hero_radius))
    };
    let mut stops = Vec::new();
    for (offset, what) in [(PRESSING, "pressing"), (RELEASED, "let go")] {
        let paused = control.pause_at(base + offset, WAIT).unwrap();
        let mut f = actors::read_frame(&mut pine).unwrap();
        f.flip = Some(paused.frames);
        f.write(&out.join(format!("actors-{:05}.json", paused.frames)))
            .unwrap();
        let h = f.hero().unwrap().clone();
        let (along, gap) = gap_of(h.position);
        rec.line(format!(
            "door walk +{offset} (flip {}, game frame {}), {what}: hero {}; velocity {:?}; \
             pivot {along:.4} m from the door's centre plane, gap {gap:+.4} m once the box's \
             {thickness:.4} and his sphere's {hero_radius} are taken off; {} [{}]",
            paused.frames,
            f.game_frame,
            h.summary(),
            h.velocity,
            f.summary(),
            load_average()
        ));
        stops.push((h, along, gap, f));
    }
    let (h, _, gap, f) = stops.last().unwrap();
    assert!(
        (-0.01..=STOP_SLACK).contains(gap),
        "the hero stopped {gap:+.4} m off touching the door"
    );
    assert!(
        h.velocity.iter().all(|v| v.abs() < 1e-3),
        "the hero still moves: {:?}",
        h.velocity
    );
    let shot = control.screenshot(&out, WAIT).unwrap();
    rec.line(format!(
        "door shot at flip {}: {} [{}, {}]",
        shot.flip,
        shot.path.file_name().unwrap().to_string_lossy(),
        screen_lock(),
        hid_idle()
    ));
    let flip = f.flip.unwrap();
    let live1 = level::read_placements(&mut pine, section.base, &objects).unwrap();
    write_json(&out, &format!("placements-{flip:05}.json"), &live1);
    let tally1 = judge(&disc, &placements, &live1, h.position);
    print_tally(
        &mut rec,
        &format!("flip {flip}, the hero at the door"),
        &tally1,
    );
    let changed: Vec<String> = disc
        .iter()
        .zip(live0.iter().zip(&live1))
        .filter(|(_, (a, b))| a.activity_state != b.activity_state)
        .map(|(d, (a, b))| format!("{:?} {} -> {}", d.name, a.activity_state, b.activity_state))
        .collect();
    rec.line(format!(
        "placements whose state changed from F0 ({}): {}",
        changed.len(),
        changed.join("; ")
    ));
    let solids1 = level::read_solids(&mut pine, f).unwrap();
    write_json(&out, &format!("solids-{flip:05}.json"), &solids1);

    // ---- the walk back into the gourd -------------------------------------------------
    let loaded = control
        .load_script(&InputScript::from_ron(GOURD_WALK).unwrap())
        .unwrap();
    let base = loaded.base;
    let paused = control.pause_at(base + GOURD_READ, WAIT).unwrap();
    let mut f = actors::read_frame(&mut pine).unwrap();
    f.flip = Some(paused.frames);
    f.write(&out.join(format!("actors-{:05}.json", paused.frames)))
        .unwrap();
    let h = f.hero().unwrap().clone();
    let solids2 = level::read_solids(&mut pine, &f).unwrap();
    write_json(&out, &format!("solids-{:05}.json", paused.frames), &solids2);
    let g = solid_named(&solids2, GOURD);
    let Shape::Sphere {
        radius: gourd_radius,
        ..
    } = g.shape.primitive()
    else {
        panic!("{GOURD}'s solid isn't a sphere: {:?}", g.shape);
    };
    let hs = solids2
        .iter()
        .find(|s| s.corpus == h.corpus)
        .expect("the hero's solid");
    let (gc, hc) = (primitive_centre(g), primitive_centre(hs));
    let apart = (0..3)
        .map(|i| f64::from(gc[i] - hc[i]).powi(2))
        .sum::<f64>()
        .sqrt();
    let gap = apart - f64::from(*gourd_radius) - f64::from(*hero_radius);
    rec.line(format!(
        "gourd walk +{GOURD_READ} (flip {}, game frame {}): hero {}; velocity {:?}; his sphere's \
         centre {hc:?}, {GOURD}'s {gc:?} (radius {gourd_radius}, filter {:#x}, _properties \
         {:?}): {apart:.4} m apart, gap {gap:+.4} m [{}]",
        paused.frames,
        f.game_frame,
        h.summary(),
        h.velocity,
        g.filter_info,
        g.properties.map(|p| format!("{p:#x}")),
        load_average()
    ));
    assert!(
        (-0.01..=STOP_SLACK).contains(&gap),
        "the hero stopped {gap:+.4} m off touching {GOURD}"
    );
    let shot = control.screenshot(&out, WAIT).unwrap();
    rec.line(format!(
        "gourd shot at flip {}: {} [{}, {}]",
        shot.flip,
        shot.path.file_name().unwrap().to_string_lossy(),
        screen_lock(),
        hid_idle()
    ));

    // ---- the end -------------------------------------------------------------------
    // RPCS3's PINE thread blocks reading its one client: a held connection
    // hangs the shutdown.
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
