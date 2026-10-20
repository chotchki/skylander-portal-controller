//! What the `#[ignore]`d tests against the oracle share (openspec AG.16): the
//! recordings, under `GUAC_CAPTURE_DIR`, and level 213's collision, read off
//! the dump at `GUAC_GIANTS_DUMP` through AG.2's reader (each key from the
//! process env, else the workspace's `.env.dev`). Both stay outside the
//! tree: guest memory and disc bytes are read where they are, never copied.

#![allow(dead_code)]

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use guac_formats::collision;
use guac_formats::iga::Archive;
use guac_formats::igz::Igz;
use guac_formats::level::Placements;
use guac_motion::ground::{self, LEAN_SETTLED};
use guac_motion::seam::{Body, BodyId, BodyKind, Collider, Mesh, Pose, Vec3};
use guac_motion::solid::{HavokSolid, SolidInfo, SolidKind, euler};
use guac_motion::walk::Footing;
use guac_oracle::recording::{Frame, Mover, Recording};

/// A key from the environment, else from the workspace's `.env.dev`.
pub fn env_or_dotenv(key: &str) -> Option<String> {
    if let Ok(value) = std::env::var(key) {
        return Some(value);
    }
    let dotenv = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../.env.dev");
    let text = std::fs::read_to_string(dotenv).ok()?;
    text.lines().find_map(|line| {
        let (k, v) = line.trim().split_once('=')?;
        (k.trim() == key).then(|| v.trim().trim_matches('"').to_owned())
    })
}

/// Every replay of a run, by its set and label.
pub fn recordings(run: &str) -> BTreeMap<String, Vec<Frame>> {
    let root = env_or_dotenv("GUAC_CAPTURE_DIR")
        .expect("set GUAC_CAPTURE_DIR (env or .env.dev) to the oracle's captures");
    let run_dir = PathBuf::from(root).join("BLUS30968").join(run);
    assert!(run_dir.is_dir(), "no {}", run_dir.display());
    let mut out = BTreeMap::new();
    for set in std::fs::read_dir(&run_dir).unwrap().flatten() {
        let Ok(replays) = std::fs::read_dir(set.path()) else {
            continue;
        };
        for replay in replays.flatten() {
            let file = replay.path().join("recording.json");
            let Ok(text) = std::fs::read_to_string(&file) else {
                continue;
            };
            let recording: Recording = serde_json::from_str(&text).unwrap();
            let name = format!(
                "{}/{}",
                set.file_name().to_string_lossy(),
                replay.file_name().to_string_lossy()
            );
            out.insert(name, recording.frames().to_vec());
        }
    }
    assert!(!out.is_empty(), "no recordings under {}", run_dir.display());
    out
}

/// The stored walk, ft/s.
pub fn stored(m: &Mover) -> Vec3 {
    let (s, c) = m.heading.to_radians().sin_cos();
    let (sp, cp) = m.pitch.to_radians().sin_cos();
    Vec3::new(cp * s, sp, cp * c) * m.length
}

pub fn field(m: &Mover, name: &str) -> f32 {
    m.fields[name]
}

/// What the script read of the engine at a record.
pub fn footing(f: &Frame) -> Option<Footing> {
    let c = f.contacts.as_ref()?;
    let g = c.ground.as_ref();
    let normal = g.map(|g| Vec3::from(g.normal));
    let up = euler(f.hero.rotation) * Vec3::Y;
    let settled = normal.is_some_and(|n| ground::angle(up, n) <= LEAN_SETTLED);
    Some(Footing {
        supported: c.support == 2,
        height: g.map(|g| f.hero.position[1] - g.location[1]),
        slope: if settled {
            up
        } else {
            normal.unwrap_or(Vec3::Y)
        },
        ground: normal,
    })
}

pub fn consecutive(frames: &[Frame]) -> bool {
    frames
        .windows(2)
        .all(|w| w[1].game_frame == w[0].game_frame.wrapping_add(1))
}

/// Chapter 1's archive (ag-motion section 4).
pub const LEVEL_213: &str = "level/level_213_tutotial.bld";
/// The level's triangle mesh and the gate's door: every collider the
/// recordings' manifolds name but a Titan boulder's, in 4 records of one
/// (AG.38's `g38_t2673_up` W-01; scripts rewrite the boulders' solids live,
/// AG.7, so the disc's isn't theirs).
pub const LEVEL: BodyId = BodyId(1);
pub const DOOR: BodyId = BodyId(2);
pub const DOOR_NAME: &str = "Breakable_Stone_Wall_Castle_206(1)";

/// Level 213's collision mesh in the level's frame (AG.2, live to the bit by
/// AG.7) and the gate door's solid from its `CollisionInfo` (AG.7: its live
/// one is the disc's), as seam bodies.
pub fn level_213() -> Vec<Body> {
    let root = env_or_dotenv("GUAC_GIANTS_DUMP")
        .expect("set GUAC_GIANTS_DUMP (env or .env.dev) to the Giants dump");
    let path = Path::new(&root).join("PS3_GAME/USRDIR").join(LEVEL_213);
    let bytes = std::fs::read(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    let archive = Archive::parse(&bytes).unwrap();
    let entry = archive.find("level.bld").unwrap();
    let data = archive.read(entry).unwrap().into_owned();
    let igz = Igz::parse(&data).unwrap();
    let body = collision::worlds(&igz)
        .find_map(|(_, w)| w.unwrap().collision.body)
        .expect("level 213's collision body");
    let mesh = Mesh::new(
        body.world_vertices().map(Vec3::from).collect(),
        body.mesh
            .triangles
            .iter()
            .map(|t| t.indices.map(u32::from))
            .collect(),
        body.mesh.materials.clone(),
    )
    .unwrap();
    let placements = Placements::read(&igz).unwrap();
    let door = placements.actor(DOOR_NAME).expect("the gate's door");
    let (solid, at) = solid_of(door).expect("the door is solid");
    vec![
        Body {
            id: LEVEL,
            kind: BodyKind::Static,
            pose: Pose::IDENTITY,
            collider: Collider::Mesh(Arc::new(mesh)),
        },
        Body {
            id: DOOR,
            kind: BodyKind::Kinematic,
            pose: solid.pose(at),
            collider: Collider::Convex(solid.in_world()),
        },
    ]
}

/// An actor's solid as the original builds it, and its placement's pose.
fn solid_of(actor: &guac_formats::level::Actor<'_>) -> Option<(HavokSolid, Pose)> {
    use guac_formats::igz::layouts::Shape;
    let c = actor.collision?;
    let kind = match c.shape() {
        Shape::None => SolidKind::None,
        Shape::Box => SolidKind::Box,
        Shape::Capsule => SolidKind::Capsule,
        Shape::Sphere => SolidKind::Sphere,
        Shape::Cylinder => SolidKind::Cylinder,
        Shape::Mesh => SolidKind::Mesh,
    };
    let info = SolidInfo {
        kind,
        offset: c.transform.position,
        rotation: c.transform.rotation,
        width: c.width,
        height: c.height,
        length: c.length,
    };
    let placement = actor.placement();
    let solid = info.havok(placement.scale)?;
    let at = placement.placement.init_matrix;
    Some((
        solid,
        Pose::new(Vec3::from(at.position), euler(at.rotation)),
    ))
}

/// Eruptor's solid: a 4 x 6 x 3 ft sphere at 150 % (AG.8's live read).
pub fn eruptor_solid() -> HavokSolid {
    SolidInfo {
        kind: SolidKind::Sphere,
        offset: [0.0; 3],
        rotation: [0.0; 3],
        width: 4.0,
        height: 6.0,
        length: 3.0,
    }
    .havok([150.0; 3])
    .unwrap()
}
