//! Chapter 1's props against the disc and the oracle (openspec AG.25):
//! level 213's placements and props read off `GUAC_GIANTS_DUMP` through the
//! extractor's own readers (in memory, no extract written) into the props
//! the TV draws and stands ([`guacamole::props`]), then held to what AG.7
//! read live at AF's fresh state (its solids, `solids-f0.json`, under
//! `GUAC_CAPTURE_DIR`) and to its walk into the gate's door, replayed on
//! the parry side of the seam over the level's own collision. Then the
//! start's scripts run on them (openspec AG.60), headless, and what they
//! leave stands against AG.7's live solids and placements and the clones
//! its live actor list holds.

mod common;

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use guac_extract::levels;
use guac_formats::igz::Igz;
use guac_ir::{DumpIdentity, Sha256};
use guac_motion::hero::{Hero, Input};
use guac_motion::parry::ParryWorld;
use guac_motion::seam::{Body, BodyKind, Collider, Mesh, Pose, Shape, Vec3};
use guac_motion::stick::Stick;
use guac_motion::tuning::Tuning;
use guacamole::level_scene;
use guacamole::props::{LeftOut, LevelProps, PROP_BODIES};
use serde_json::Value;

/// `key` from the environment, else from the repo's `.env.dev`.
fn env_or_dotenv(key: &str) -> Option<String> {
    if let Ok(v) = std::env::var(key) {
        return Some(v);
    }
    let dotenv = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../.env.dev");
    let text = std::fs::read_to_string(dotenv).ok()?;
    text.lines().find_map(|line| {
        let (k, v) = line.trim().split_once('=')?;
        (k.trim() == key).then(|| v.trim().trim_matches('"').to_owned())
    })
}

fn identity() -> DumpIdentity {
    DumpIdentity {
        title_id: "BLUS30968".into(),
        app_ver: "01.00".into(),
        manifest_hash: Sha256([0; 32]),
    }
}

/// Eruptor at AF's fresh state (AF.9's idle pivot), where AG.7 read the
/// placements and the solids.
const FRESH: [f32; 3] = [10.190_27, 43.382_67, -93.394_06];
/// Where AG.7's walk into the gate's door stopped him (its first run's x).
const AT_THE_DOOR: [f32; 3] = [14.135_68, 43.40, -83.702_29];
/// AG.7's first run, every output of it.
const AG7_RUN: &str = "BLUS30968/ag7-level-1792410996";
/// AG.7's third run, whose solids carry each `CollisionInfo`'s bits.
const AG7_BITS: &str = "BLUS30968/ag7-level-1792412070";

/// Chapter 1's props, its collision mesh and where the hero starts.
struct Chapter1 {
    props: LevelProps,
    mesh: Body,
    start: Vec3,
}

fn chapter_1(dump: &Path) -> Chapter1 {
    let usrdir = dump.join("PS3_GAME/USRDIR");
    let level = levels::LEVELS[0];
    let archive = std::fs::read(levels::archive_path(&usrdir, level)).expect("the level archive");
    let bld = levels::level_bld(&archive).expect("its level.bld");
    let igz = Igz::parse(&bld).expect("the level parses");
    let props = levels::props(&igz, level, identity())
        .expect("the props")
        .ir;
    let placements = levels::placements(&igz, level, identity())
        .expect("the placements")
        .ir;
    let collision = levels::collision(&igz, level, identity())
        .expect("the collision")
        .ir;
    let start = level_scene::start_point(&placements).expect("the start");
    let m = &collision.meshes[0];
    let mesh = Body {
        id: guac_motion::seam::BodyId(level_scene::LEVEL_BODIES),
        kind: BodyKind::Static,
        pose: Pose::IDENTITY,
        collider: Collider::Mesh(Arc::new(
            Mesh::new(
                m.vertices.iter().map(|v| Vec3::from_array(*v)).collect(),
                m.triangles.iter().map(|t| t.map(u32::from)).collect(),
                m.materials.clone(),
            )
            .unwrap(),
        )),
    };
    Chapter1 {
        props: LevelProps::new(&props, &placements).expect("the props"),
        mesh,
        start,
    }
}

fn v3(v: &Value) -> Vec3 {
    let f = |i: usize| v[i].as_f64().unwrap() as f32;
    Vec3::new(f(0), f(1), f(2))
}

/// A live phantom's shape: its centre in the world (the phantom's rotation
/// rows as row vectors) and its sizes as the seam keeps them, less the
/// 0.05 m the hero's query shape takes from the world.
fn live_shape(solid: &Value) -> Option<(Vec3, Shape)> {
    let t = &solid["shape"]["Transformed"];
    let child = &t["child"];
    let rows: Vec<Vec3> = (0..3).map(|i| v3(&solid["rotation"][i])).collect();
    let local = v3(&t["translation"]);
    let centre =
        v3(&solid["translation"]) + rows[0] * local.x + rows[1] * local.y + rows[2] * local.z;
    let less = |v: &Value| v.as_f64().unwrap() as f32 - 0.05;
    let shape = if let Some(b) = child.get("Box") {
        Shape::Box {
            half_extents: v3(&b["half_extents"]),
            margin: less(&b["radius"]),
        }
    } else if let Some(s) = child.get("Sphere") {
        Shape::Sphere {
            radius: less(&s["radius"]),
        }
    } else if let Some(c) = child.get("Capsule") {
        Shape::Capsule {
            radius: less(&c["radius"]),
            half_segment: v3(&c["a"]).y,
        }
    } else {
        let c = child.get("Cylinder")?;
        Shape::Cylinder {
            radius: c["cylinder_radius"].as_f64().unwrap() as f32,
            half_height: v3(&c["a"]).y,
            margin: less(&c["radius"]),
        }
    };
    Some((centre, shape))
}

/// Two shapes of one kind with every size within `tol`; kinds that differ
/// never match.
fn same(a: &Shape, b: &Shape, tol: f32) -> bool {
    let n = |x: f32, y: f32| (x - y).abs() <= tol;
    match (a, b) {
        (Shape::Sphere { radius: r }, Shape::Sphere { radius: s }) => n(*r, *s),
        (
            Shape::Capsule {
                radius: r,
                half_segment: h,
            },
            Shape::Capsule {
                radius: s,
                half_segment: k,
            },
        ) => n(*r, *s) && n(*h, *k),
        (
            Shape::Box {
                half_extents: e,
                margin: m,
            },
            Shape::Box {
                half_extents: f,
                margin: o,
            },
        ) => (*e - *f).abs().max_element() <= tol && n(*m, *o),
        (
            Shape::Cylinder {
                radius: r,
                half_height: h,
                margin: m,
            },
            Shape::Cylinder {
                radius: s,
                half_height: k,
                margin: o,
            },
        ) => n(*r, *s) && n(*h, *k) && n(*m, *o),
        _ => false,
    }
}

#[test]
#[ignore = "disc and oracle recordings: GUAC_GIANTS_DUMP and AG.7's run under GUAC_CAPTURE_DIR (AG.25)"]
fn chapter_1s_props_stand_as_the_originals_do() {
    let dump = PathBuf::from(
        env_or_dotenv("GUAC_GIANTS_DUMP").expect("set GUAC_GIANTS_DUMP to the Giants dump"),
    );
    let captures = PathBuf::from(
        env_or_dotenv("GUAC_CAPTURE_DIR").expect("set GUAC_CAPTURE_DIR to the oracle's captures"),
    );
    let started = std::time::Instant::now();
    let Chapter1 { props, mesh, start } = chapter_1(&dump);
    let left = |why| props.left_out.iter().filter(|(_, w)| *w == why).count();
    eprintln!(
        "chapter 1: {} props on {} models; left out {} templates, {} creatures, {} by script, {} with nothing to stand, {} in another state; {} drawing nothing ({:?}); read in {:.1} s",
        props.props.len(),
        props.models.len(),
        left(LeftOut::Template),
        left(LeftOut::Creature),
        left(LeftOut::ByScript),
        left(LeftOut::Nothing),
        left(LeftOut::State),
        props.undrawn.len(),
        props.undrawn,
        started.elapsed().as_secs_f64()
    );

    // What stands at AF's fresh state, against AG.7's live solids there.
    let fresh = Vec3::from_array(FRESH);
    let standing: Vec<usize> = props.standing(fresh).collect();
    let names: Vec<&str> = standing
        .iter()
        .map(|&i| props.props[i].name.as_str())
        .collect();
    for want in [
        "Breakable_Stone_Wall_Castle_206(1)",
        "gourd_01(1)",
        "gourd_01(2)",
        "Titan_CarriableObject_Boulder_v3_(4)",
    ] {
        assert!(names.contains(&want), "{want} doesn't stand at the start");
    }
    let drawn = standing
        .iter()
        .filter(|&&i| props.props[i].model.is_some())
        .count();
    let kind = |k| {
        standing
            .iter()
            .filter(|&&i| props.props[i].body.as_ref().map(|b| b.kind) == Some(k))
            .count()
    };
    eprintln!(
        "at the fresh state {} stand: {drawn} drawn, {} solid, {} sensing",
        standing.len(),
        kind(BodyKind::Kinematic),
        kind(BodyKind::Sensor)
    );
    let path = captures.join(AG7_RUN).join("solids-f0.json");
    let live: Vec<Value> = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
    let ours: Vec<(&str, &Body)> = standing
        .iter()
        .filter_map(|&i| Some((props.props[i].name.as_str(), props.props[i].body.as_ref()?)))
        .collect();
    let (mut matched, mut differ, mut theirs_only) = (Vec::new(), Vec::new(), Vec::new());
    for s in live
        .iter()
        .filter(|s| s["in_world"] == true && s["filter_info"] != 4)
    {
        let name = s["name"].as_str().unwrap();
        match ours.iter().find(|(n, _)| *n == name) {
            None => {
                let why = props.left_out.iter().find(|(n, _)| n == name).map_or(
                    "not the level file's",
                    |(_, w)| match w {
                        LeftOut::Template => "a template",
                        LeftOut::Creature => "a creature",
                        LeftOut::ByScript => "woken by its script",
                        LeftOut::Nothing => "nothing to stand",
                        LeftOut::State => "another state",
                    },
                );
                theirs_only.push(format!("{name} ({why})"));
            }
            Some((_, body)) => {
                let (centre, shape) = live_shape(s).expect("a live shape");
                let Collider::Convex(ours) = &body.collider else {
                    panic!("{name}: a mesh")
                };
                if same(ours, &shape, 1e-4) && body.pose.position.distance(centre) < 1e-3 {
                    matched.push(name);
                } else {
                    differ.push(format!(
                        "{name}: ours {ours:?} at {}, live {shape:?} at {centre}",
                        body.pose.position
                    ));
                }
            }
        }
    }
    let ours_only: Vec<&str> = ours
        .iter()
        .filter(|(n, _)| !live.iter().any(|s| s["name"] == *n))
        .map(|(n, _)| *n)
        .collect();
    eprintln!(
        "solid at the fresh state, ours and live alike: {} {matched:?}",
        matched.len()
    );
    eprintln!("ours and live differ ({}):", differ.len());
    for d in &differ {
        eprintln!("  {d}");
    }
    eprintln!("live only ({}): {theirs_only:?}", theirs_only.len());
    eprintln!("ours only: {ours_only:?}");
    // AG.7's one exception with a shape: in range, but its own script had
    // taken it off (a non-solid trigger, so it senses and stops nothing).
    assert_eq!(
        ours_only,
        ["Eon_1_Move_along_path"],
        "solids the original hasn't"
    );
    // What differs is what the start's scripts change (AG.7: the Titan
    // boulders made cylinders, the cannon's push block made smaller; and the
    // gourds `Scale.ai` shrinks, their placements' scale, not their solids).
    for d in &differ {
        assert!(
            d.starts_with("Titan_CarriableObject_Boulder")
                || d.starts_with("Push_Block_Cannon")
                || d.starts_with("gourd_01"),
            "a solid the scripts don't change differs: {d}"
        );
    }
    assert_eq!((matched.len(), differ.len()), (21, 7));
    // What's live and not ours is a placement its script wakes, a creature,
    // a template's clone (named as the template) or the game's own (the
    // hero's lava).
    for t in &theirs_only {
        assert!(
            t.ends_with("(woken by its script)")
                || t.ends_with("(a creature)")
                || t.ends_with("(a template)")
                || t.ends_with("(not the level file's)"),
            "{t}"
        );
    }

    // The spinning coin: 49.25 m off at the start, past its 160 ft; there
    // once he's at the gate's door, as on AG.7's walk.
    let coin = props
        .props
        .iter()
        .find(|p| p.name == "Coin_spinning")
        .expect("the spinning coin");
    assert!(!coin.stands(fresh));
    assert!(coin.stands(Vec3::from_array(AT_THE_DOOR)));

    // AG.7's walk into the gate's door on parry's side: at rest 60 game
    // frames, the stick at the view's +75 (x 0, y 70 as bytes) for 40, then
    // at -15 (x 186, y 0) for 180, let go; the props standing by where he
    // is each step. The original stopped him with his pivot 1.4781 m from
    // the door's middle plane (z -82.2242).
    let solid = guac_motion::solid::SolidInfo {
        kind: guac_motion::solid::SolidKind::Sphere,
        offset: [0.0; 3],
        rotation: [0.0; 3],
        width: 4.0,
        height: 6.0,
        length: 3.0,
    }
    .havok([150.0; 3])
    .unwrap();
    let world_at = |at: Vec3| {
        let mut bodies = vec![mesh.clone()];
        bodies.extend(props.bodies(at));
        ParryWorld::new(&bodies).unwrap()
    };
    let mut hero = Hero::new(solid, Tuning::ERUPTOR, start, 0.0);
    let mut world = world_at(start);
    hero.settle(&world, &[]);
    let mut stood: Vec<usize> = props.standing(hero.pivot).collect();
    let byte = |b: f32, up: bool| {
        if up {
            (128.0 - b) / 128.0
        } else {
            (b - 128.0) / 127.0
        }
    };
    let script = |frame: usize| match frame {
        0..60 => Stick::CENTRED,
        60..100 => Stick::new(byte(0.0, false).max(-1.0), byte(70.0, true)),
        100..280 => Stick::new(byte(186.0, false), byte(0.0, true).min(1.0)),
        _ => Stick::CENTRED,
    };
    for frame in 0..360 {
        let now: Vec<usize> = props.standing(hero.pivot).collect();
        if now != stood {
            world = world_at(hero.pivot);
            stood = now;
        }
        hero.step(&world, &Input::stick(script(frame), 15.0), 1.0 / 60.0, &[]);
    }
    let door = props
        .props
        .iter()
        .find(|p| p.name == "Breakable_Stone_Wall_Castle_206(1)")
        .unwrap();
    let middle = door.position[2];
    let from_middle = middle - hero.pivot.z;
    eprintln!(
        "AG.7's walk on parry: stopped at {} ({from_middle} m from the door's middle; the original's 1.4781 at {AT_THE_DOOR:?}), {} props standing",
        hero.pivot,
        stood.len()
    );
    assert_eq!(hero.velocity, Vec3::ZERO);
    assert!((from_middle - 1.4781).abs() < 0.01, "{from_middle}");
    assert!(
        (hero.pivot.x - AT_THE_DOOR[0]).abs() < 0.05,
        "{}",
        hero.pivot
    );
    // The door's solid among those standing there.
    let door_id = PROP_BODIES
        + props
            .props
            .iter()
            .find(|p| p.name == door.name)
            .unwrap()
            .object;
    assert!(props.bodies(hero.pivot).iter().any(|b| b.id.0 == door_id));
}

/// A live actor's place, its turn `[tilt, heading, roll]` and its scale.
type Placed = (Vec3, [f32; 3], [f32; 3]);

/// The quaternion's angle between two Euler turns `[tilt, heading, roll]`,
/// degrees.
fn turn_between(a: [f32; 3], b: [f32; 3]) -> f32 {
    let (qa, qb) = (guac_motion::solid::euler(a), guac_motion::solid::euler(b));
    let d = (qa * qb.inverse()).to_axis_angle().1.to_degrees();
    d.min(360.0 - d)
}

/// The start's scripts on the props (openspec AG.60), run headless from AF's
/// fresh state for 2 s (the game's own start-up, its first ticks, is what
/// the original ran before F0): every solid standing then against AG.7's
/// live ones, every drawn prop's place, turn and scale against its live
/// placement, and the clones the TV draws against its live actors.
#[test]
#[ignore = "disc and oracle recordings: GUAC_GIANTS_DUMP and AG.7's run under GUAC_CAPTURE_DIR (AG.60)"]
fn the_starts_scripts_leave_the_props_as_the_originals_are() {
    let captures = PathBuf::from(
        env_or_dotenv("GUAC_CAPTURE_DIR").expect("set GUAC_CAPTURE_DIR to the oracle's captures"),
    );
    let started = std::time::Instant::now();
    let (mut run, _) =
        common::run_on_with(guac_script::Mode::Strict, common::Setup { props: true });
    run.put(Vec3::from_array(FRESH), 0.0);
    // Before any script runs, each prop's actor holds its file's draw and
    // solid to the bit: the one holder starts where the file does.
    {
        let props = run.bench.props.as_ref().unwrap();
        for p in &props.level.props {
            let a = run.world.actor(props.handles[&p.object]).unwrap();
            assert_eq!(
                guacamole::props::actor_transform(a),
                p.transform,
                "{}",
                p.name
            );
            assert_eq!(guacamole::props::actor_body(p, a), p.body, "{}", p.name);
        }
        eprintln!(
            "the {} props' actors hold their files' draws and solids",
            props.level.props.len()
        );
    }
    let mut roll = Vec::new();
    for _ in 0..120 {
        run.step().unwrap_or_else(|h| panic!("{h}"));
        let (h, _) = run.placed("Propellar_Prop");
        roll.push(run.world.actor(h).unwrap().angles.roll);
    }
    eprintln!(
        "120 steps from AF's fresh state in {:.1} s; {}",
        started.elapsed().as_secs_f64(),
        common::load()
    );
    // The propeller turns 1 degree a step, as AG.7's reads have it (roll 61
    // at F0, 52 at flip 353, 351 game frames on).
    for w in roll.windows(2) {
        assert!(
            ((w[1] - w[0]).rem_euclid(360.0) - 1.0).abs() < 1e-3,
            "{w:?}"
        );
    }

    // Solids: what stands, as the scripts left it, against AG.7's live ones.
    let props = run.bench.props.as_ref().unwrap();
    let name_of = |object: u32| {
        props
            .level
            .props
            .iter()
            .find(|p| p.object == object)
            .map(|p| p.name.as_str())
            .unwrap()
    };
    let path = captures.join(AG7_RUN).join("solids-f0.json");
    let live: Vec<Value> = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
    // Each live phantom's `CollisionInfo` bits, which AG.7's third run read
    // at the same F0 (its solids the first's to 1e-3 m but the hero's lava
    // and Norticus'), by name and place.
    let path = captures.join(AG7_BITS).join("solids-f0.json");
    let bits: Vec<Value> = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
    let bits_of = |name: &str, at: Vec3| {
        bits.iter()
            .filter(|b| b["name"] == name)
            .min_by(|x, y| {
                let d = |b: &Value| {
                    let t = |i: usize| b["translation"][i].as_f64().unwrap() as f32;
                    Vec3::new(t(0), t(1), t(2)).distance(at)
                };
                d(x).total_cmp(&d(y))
            })
            .and_then(|b| b["properties"].as_u64())
            .unwrap_or_else(|| panic!("{name}: no bits in {AG7_BITS}"))
    };
    let ours: Vec<(&str, &Body)> = props.holds.iter().map(|(o, b)| (name_of(*o), b)).collect();
    let (mut matched, mut differ, mut theirs_only) = (Vec::new(), Vec::new(), Vec::new());
    for s in live
        .iter()
        .filter(|s| s["in_world"] == true && s["filter_info"] != 4)
    {
        let name = s["name"].as_str().unwrap();
        let (centre, shape) = live_shape(s).expect("a live shape");
        let near = ours.iter().filter(|(n, _)| *n == name).min_by(|a, b| {
            a.1.pose
                .position
                .distance(centre)
                .total_cmp(&b.1.pose.position.distance(centre))
        });
        match near {
            None => {
                let why = props.level.left_out.iter().find(|(n, _)| n == name).map_or(
                    "not the level file's",
                    |(_, w)| match w {
                        LeftOut::Template => "a template's clone",
                        LeftOut::Creature => "a creature",
                        LeftOut::ByScript => "woken by its script",
                        LeftOut::Nothing => "nothing to stand",
                        LeftOut::State => "another state",
                    },
                );
                let why = if props.level.props.iter().any(|p| p.name == name) {
                    "woken by a script off the whitelist"
                } else {
                    why
                };
                theirs_only.push(format!("{name} ({why})"));
            }
            Some((_, body)) => {
                let Collider::Convex(ours) = &body.collider else {
                    panic!("{name}: a mesh")
                };
                // Blocking or sensing as the live phantom's bit 3 (its
                // `interacts with actors`) has it.
                let blocks = bits_of(name, centre) >> 3 & 1 == 1;
                if same(ours, &shape, 1e-4)
                    && body.pose.position.distance(centre) < 1e-3
                    && (body.kind == BodyKind::Kinematic) == blocks
                {
                    matched.push(name);
                } else {
                    differ.push(format!(
                        "{name}: ours {ours:?} {:?} at {}, live {shape:?} blocking {blocks} at {centre}",
                        body.kind, body.pose.position
                    ));
                }
            }
        }
    }
    let ours_only: Vec<&str> = ours
        .iter()
        .filter(|(n, _)| !live.iter().any(|s| s["name"] == *n))
        .map(|(n, _)| *n)
        .collect();
    eprintln!(
        "solid after the start's scripts, ours and live alike (shape, place and bit 3): {} {matched:?}",
        matched.len()
    );
    eprintln!("ours and live differ ({}): {differ:?}", differ.len());
    eprintln!("live only ({}): {theirs_only:?}", theirs_only.len());
    eprintln!("ours only: {ours_only:?}");
    assert!(differ.is_empty(), "{differ:?}");
    assert_eq!(ours_only, ["Eon_1_Move_along_path"], "AG.7's one exception");
    // What's live and not ours: the creatures, the push blocks' tortoises
    // (a skinned template's clones, creatures by the same rule), the hero's
    // lava, and the lush peak's ambience trigger, which the level's sound
    // environment script wakes.
    for t in &theirs_only {
        assert!(
            t.ends_with("(a creature)")
                || t == "2_Push_Block_Art_Sheep (a template's clone)"
                || t == "009_LavaCollider (not the level file's)"
                || t == "trg_lvl_213_lush_peak (woken by a script off the whitelist)",
            "{t}"
        );
    }

    // Placements: every drawn prop standing, where its actor has it, against
    // AG.7's live read at F0.
    let path = captures.join(AG7_RUN).join("placements-00002.json");
    let placements: Vec<Value> =
        serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
    let live_of = |name: &str, at: Vec3| {
        placements
            .iter()
            .filter(|p| p["name"] == name)
            .map(|p| {
                let m: Vec<f32> = (0..6)
                    .map(|i| p["matrix"][i].as_f64().unwrap() as f32)
                    .collect();
                let s: Vec<f32> = (0..3)
                    .map(|i| p["scale"][i].as_f64().unwrap() as f32)
                    .collect();
                (
                    Vec3::new(m[0], m[1], m[2]),
                    [m[3], m[4], m[5]],
                    [s[0], s[1], s[2]],
                )
            })
            .min_by(|a, b| a.0.distance(at).total_cmp(&b.0.distance(at)))
    };
    let (mut alike, mut off) = (0, Vec::new());
    for p in props.level.props.iter().filter(|p| p.model.is_some()) {
        let h = props.handles[&p.object];
        let a = run.world.actor(h).unwrap();
        if !a.active || a.removed {
            continue;
        }
        let Some((at, turn, scale)) = live_of(&p.name, a.position) else {
            off.push(format!("{}: not in the live read", p.name));
            continue;
        };
        let ours = [a.angles.tilt, a.angles.facing, a.angles.roll];
        let d = a.position.distance(at);
        let t = turn_between(ours, turn);
        if d < 1e-3 && t < 0.01 && a.scale == scale {
            alike += 1;
        } else {
            off.push(format!(
                "{}: ours {} {ours:?} {:?}, live {at} {turn:?} {scale:?} ({d} m, {t} deg)",
                p.name, a.position, a.scale
            ));
        }
    }
    eprintln!(
        "drawn props where the original has them: {alike}; off ({}): {off:?}",
        off.len()
    );
    // The propeller's roll is time's: it turns a degree a step from when it
    // woke, which AG.7's read can't date.
    assert!(
        off.iter().all(|o| o.starts_with("Propellar_Prop:")),
        "{off:?}"
    );

    // The clones the TV draws against AG.7's live actors at F0: the fence's
    // cross beam where the fence holds it, its planks where it lays them
    // (their turns and heights rolled), the eggs in their nests (their
    // wobble rolled), the cannon's base on the cannon; and the skinned ones
    // (AG.150): the push blocks' three tortoises on their blocks, the
    // cannon's top on its base.
    let path = captures.join(AG7_RUN).join("actors-00002.json");
    let actors: Value = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
    let mut live_clones: HashMap<&str, Vec<Placed>> = HashMap::new();
    for a in actors["actors"].as_array().unwrap() {
        let name = a["name"].as_str().unwrap();
        if !guacamole::props::DRAWN_SPAWNS.contains(&name)
            && !guacamole::props::SKINNED_SPAWNS.contains(&name)
        {
            continue;
        }
        let v = |k: &str| {
            let f = |i: usize| a[k][i].as_f64().unwrap() as f32;
            [f(0), f(1), f(2)]
        };
        live_clones.entry(name).or_default().push((
            Vec3::from_array(v("position")),
            v("rotation"),
            v("scale"),
        ));
    }
    let mut checked = 0;
    for c in run.world.clones() {
        let a = run.world.actor(c).unwrap();
        let Some(t) = a.template.and_then(|t| run.world.template(t)) else {
            continue;
        };
        let Some(live) = live_clones.get(t.name.as_str()) else {
            continue;
        };
        let (at, turn, scale) = *live
            .iter()
            .min_by(|x, y| {
                x.0.distance(a.position)
                    .total_cmp(&y.0.distance(a.position))
            })
            .unwrap();
        let ours = [a.angles.tilt, a.angles.facing, a.angles.roll];
        let d = a.position.distance(at);
        eprintln!(
            "  {}: ours {} {ours:?} {:?}, live {at} {turn:?} {scale:?} ({d} m)",
            t.name, a.position, a.scale
        );
        match t.name.as_str() {
            "Bird_nest_egg" => assert!(d < 0.05, "{d}"),
            "Wooden Plank_one" | "Wooden Plank_Two" => {
                assert!(d < 1e-3, "{d}");
                assert!([114.0, 294.0].contains(&a.angles.facing.rem_euclid(360.0)));
                assert!([-3.0, 0.0, 3.0].contains(&a.angles.roll));
                assert!((93.0..=107.0).contains(&a.scale[1]), "{:?}", a.scale);
            }
            _ => {
                assert!(d < 1e-3, "{}: {d}", t.name);
                assert!(turn_between(ours, turn) < 0.01, "{}", t.name);
                assert_eq!(a.scale, scale, "{}", t.name);
            }
        }
        checked += 1;
    }
    assert_eq!(
        checked, 14,
        "the beam, six planks, two eggs, the cannon's base, three tortoises and the cannon's top"
    );

    // At the gate's door the walkway's first spinning coin wakes, as on
    // AG.7's walk: it turns by its spin speed each step (4 to 8 degrees,
    // sliding toward 2 over a minute; AH.11's recordings step the walkway's
    // three 2.82, -5.31 and 4.23 a frame later on), and stands with no
    // solid, as AG.7's live solids at the door hold none for it.
    run.put(Vec3::from_array(AT_THE_DOOR), 0.0);
    let (coin, _) = run.placed("Coin_spinning");
    let mut facing = Vec::new();
    for _ in 0..30 {
        run.step().unwrap_or_else(|h| panic!("{h}"));
        let a = run.world.actor(coin).unwrap();
        facing.push((a.active, a.angles.facing));
    }
    let woke = facing
        .iter()
        .position(|(on, _)| *on)
        .expect("the coin woke");
    let turns: Vec<f32> = facing[woke..]
        .windows(2)
        .map(|w| (w[1].1 - w[0].1 + 540.0).rem_euclid(360.0) - 180.0)
        .collect();
    eprintln!("the walkway's coin woke {woke} steps on at the door, turning {turns:?}");
    assert!(turns.len() > 20);
    assert!(
        turns.iter().all(|t| (2.0..=8.0).contains(&t.abs())),
        "{turns:?}"
    );
    let props = run.bench.props.as_ref().unwrap();
    let object = props
        .level
        .props
        .iter()
        .find(|p| p.name == "Coin_spinning")
        .unwrap()
        .object;
    assert!(!props.holds.contains_key(&object), "a coin stops nothing");
}
