//! A level as the live game holds it, read at a pause through PINE (openspec
//! AG.7): every placement's words where the level's data section sits in guest
//! memory, the collision mesh the live `hkpWorld` queries, and each simulated
//! actor's solid as Havok holds it. `live_level` reads level 213 with these and
//! compares them against the disc (AG.2's and AG.3's readers).
//!
//! Where each comes from, Giants 01.00 (`BLUS30968`), offsets from each
//! object's start, big-endian, found in AF.8's fresh state and checked live:
//! - A level file's data sections load in place: an object at offset `o` of a
//!   section sits at that section's base plus `o`, its first word now its
//!   class's vtable. Level 213's section 0 (70,834 objects, every placement and
//!   `CollisionInfo` among them) sat at `0x34304E00` in the June state and in
//!   AF.8's. [`section_base`] finds it from the actors [`crate::actors`]
//!   reads: a simulated actor's placement is that section's copy, so its
//!   address less the placement's offset on the disc is the base.
//! - A placement ([`placements`]): `_name` +0x08, `_flagPool` +0x1C,
//!   `_currentMatrix` +0x3C, `_activityState` +0x54, `_scaleVec` +0xB8,
//!   `_collision` +0xC4, and a `tfbActorInfo`'s `_gameActor` +0xF4 (its corpus,
//!   when it has one).
//! - A static at [`HKP_WORLD`] points at the `hkpWorld`. Its fixed simulation
//!   island (+0x20) lists the fixed bodies (`m_entities` +0x4C, count +0x50):
//!   the world's own fixed body (no shape) and the level's, whose shape is the
//!   `tfbTriangleCollisionQuery` the level file's packfile holds (+0x34 its
//!   `tfbSimpleMeshShape`). The body and the shapes are AG.2's layouts
//!   ([`collision`]); the world's phantoms are an array at +0x11C (count +0x120).
//! - A simulated actor's solid ([`solids`]): its corpus's `_animationBlock`
//!   (+0x7C, a `tfbPhysicsLink::animationComponent`) holds its
//!   `tfbShapePhanton` at +0x1C. A phantom keeps its world at +0x08, its shape
//!   at +0x10, its broadphase handle at +0x24, its collision filter info at
//!   +0x2C and its transform at +0xA0 (three rotation columns, then the
//!   translation at +0xD0). Its shape is an `hkpConvexTransformShape` (child
//!   +0x18; translation +0x20, rotation quaternion +0x30, scale +0x40) over a
//!   box (half extents +0x20), sphere, capsule (ends +0x20, +0x30) or cylinder
//!   (radius +0x14, ends +0x20, +0x30), each with its convex radius at +0x10.
//!   The placement's live `CollisionInfo` (`_collision`) keeps `_properties`
//!   at +0x18.
//!
//! Every pointer is checked against its class's vtable before it's followed.
//! Every address is the 01.00 EBOOT's: [`read_collision`] and [`read_solids`]
//! check PINE's title and version first and that it stayed paused.

use std::collections::{BTreeMap, BTreeSet};

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};

use crate::actors::{
    self, ActorFrame, Gather, be32, c_string, expect_vtable, f32_at, floats, object_ptr, one,
};
use crate::pine::{EmuStatus, PineClient};

/// The EBOOT's static slot holding the live `hkpWorld` (`0x3091F730` in AF.8's
/// fresh state, where the world's gravity reads -9.8).
pub const HKP_WORLD: u32 = 0x008E_1ED0;

/// Each class's vtable, every instance's first word.
pub mod vtable {
    pub const WORLD: u32 = 0x006F_6830;
    pub const SIMULATION_ISLAND: u32 = 0x006F_7010;
    pub const RIGID_BODY: u32 = 0x006F_6268;
    pub const TRIANGLE_COLLISION_QUERY: u32 = 0x0063_4268;
    pub const SIMPLE_MESH_SHAPE: u32 = 0x0063_4178;
    pub const SHAPE_PHANTOM: u32 = 0x0063_3F08;
    pub const CONVEX_TRANSFORM_SHAPE: u32 = 0x006F_23B0;
    pub const BOX_SHAPE: u32 = 0x006F_22F0;
    pub const SPHERE_SHAPE: u32 = 0x006F_24F8;
    pub const CAPSULE_SHAPE: u32 = 0x006F_2350;
    pub const CYLINDER_SHAPE: u32 = 0x006F_2498;
    pub const ACTOR_INFO: u32 = super::actors::vtable::ACTOR_INFO;
    pub const PARTICLE_INFO: u32 = 0x0062_A3C8;
    pub const LIGHT_INFO: u32 = 0x0062_C0F8;
}

const WORLD_LEN: usize = 0x128;
const WORLD_FIXED_ISLAND: usize = 0x20;
const WORLD_PHANTOMS: usize = 0x11C;
const ISLAND_LEN: usize = 0x58;
const ISLAND_WORLD: usize = 0x18;
const ISLAND_ENTITIES: usize = 0x4C;
const BODY_LEN: usize = 0x130;
const BODY_WORLD: usize = 0x08;
const BODY_SHAPE: usize = 0x10;
const BODY_MOTION_TYPE: usize = 0xE8;
const BODY_ROTATION: usize = 0xF0;
const BODY_TRANSLATION: usize = 0x120;
/// `hkpMotion::MOTION_FIXED`.
pub const MOTION_FIXED: u8 = 5;
const QUERY_LEN: usize = 0x38;
const QUERY_CHILD: usize = 0x34;
const MESH_LEN: usize = 0x68;
const MESH_RADIUS: usize = 0x3C;
const MESH_WELDING: usize = 0x40;
const MESH_VERTICES: usize = 0x44;
const MESH_INDICES: usize = 0x50;
const MESH_MATERIALS: usize = 0x5C;
/// Elements an `hkArray` may hold before it's taken for garbage: level 213's
/// mesh has 6,504 floats and 12,320 index words; the disc's largest body
/// 11,488 triangles.
const MAX_ARRAY: u32 = 1 << 20;
const PLACEMENT_LEN: usize = 0xF8;
const PLACEMENT_NAME: usize = 0x08;
const PLACEMENT_FLAG_POOL: usize = 0x1C;
const PLACEMENT_MATRIX: usize = 0x3C;
const PLACEMENT_STATE: usize = 0x54;
const PLACEMENT_SCALE: usize = 0xB8;
const PLACEMENT_COLLISION: usize = 0xC4;
const ACTOR_GAME_ACTOR: usize = 0xF4;
const COLLISION_LEN: usize = 0x58;
const COLLISION_PROPERTIES: usize = 0x18;
const NAME_LEN: usize = 64;
const CORPUS_ANIMATION_BLOCK: usize = 0x7C;
const COMPONENT_LEN: usize = 0x20;
const COMPONENT_PHANTOM: usize = 0x1C;
const PHANTOM_LEN: usize = 0xE0;
const PHANTOM_WORLD: usize = 0x08;
const PHANTOM_SHAPE: usize = 0x10;
const PHANTOM_BROADPHASE: usize = 0x24;
const PHANTOM_FILTER: usize = 0x2C;
const PHANTOM_ROTATION: usize = 0xA0;
const PHANTOM_TRANSLATION: usize = 0xD0;
const SHAPE_LEN: usize = 0x50;
const SHAPE_RADIUS: usize = 0x10;
const CT_CHILD: usize = 0x18;
const CT_TRANSLATION: usize = 0x20;
const CT_ROTATION: usize = 0x30;
const CT_SCALE: usize = 0x40;
const BOX_HALF: usize = 0x20;
const CYLINDER_RADIUS: usize = 0x14;
const END_A: usize = 0x20;
const END_B: usize = 0x30;
/// Shapes nested under a convex transform before the chain is taken for a loop.
const MAX_DEPTH: usize = 4;

/// The level's collision as the live `hkpWorld` holds it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LiveCollision {
    pub world: u32,
    pub body: u32,
    pub query: u32,
    pub mesh: u32,
    /// The world's fixed bodies, its own (no shape) included.
    pub fixed_bodies: usize,
    pub motion_type: u8,
    /// The body's rotation, three rows of four.
    pub rotation: [f32; 12],
    pub translation: [f32; 3],
    pub radius: f32,
    pub welding_type: u8,
    /// Body-local, as the game's arrays hold them.
    pub vertices: Vec<[f32; 3]>,
    /// Three indices and the welding word.
    pub triangles: Vec<[u16; 4]>,
    pub materials: Vec<u8>,
}

/// An `hkArray`'s data pointer and count, the count held to [`MAX_ARRAY`].
fn hk_array(bytes: &[u8], at: usize, what: &str) -> Result<(u32, u32)> {
    let (ptr, count) = (be32(bytes, at), be32(bytes, at + 4));
    if count > MAX_ARRAY {
        bail!("{what} holds {count} elements");
    }
    if count > 0 && !object_ptr(ptr) {
        bail!("{what}'s data at {ptr:#010x} isn't an object address");
    }
    Ok((ptr, count))
}

fn read_array(g: &mut impl Gather, ptr: u32, len: usize, what: &str) -> Result<Vec<u8>> {
    if len == 0 {
        return Ok(Vec::new());
    }
    one(g, ptr, len, what)
}

/// The level's static body as [`collision`] finds it, without its mesh.
struct LevelBody {
    world: u32,
    body: u32,
    bytes: Vec<u8>,
    query: u32,
    fixed_bodies: usize,
}

/// The level's static body: the one fixed body of the world's fixed island
/// whose shape is a `tfbTriangleCollisionQuery`, checked as [`collision`]
/// checks it up to its shape (its motion type aside).
pub fn level_body_address(g: &mut impl Gather) -> Result<u32> {
    Ok(level_body(g)?.body)
}

fn level_body(g: &mut impl Gather) -> Result<LevelBody> {
    let world = be32(&one(g, HKP_WORLD, 4, "the hkpWorld pointer")?, 0);
    let w = one(g, world, WORLD_LEN, "the hkpWorld")?;
    expect_vtable(&w, vtable::WORLD, world, "hkpWorld")?;
    let island_at = be32(&w, WORLD_FIXED_ISLAND);
    let island = one(g, island_at, ISLAND_LEN, "the fixed island")?;
    expect_vtable(
        &island,
        vtable::SIMULATION_ISLAND,
        island_at,
        "hkpSimulationIsland",
    )?;
    if be32(&island, ISLAND_WORLD) != world {
        bail!("the fixed island at {island_at:#010x} isn't the world's");
    }
    let (entities_at, count) = hk_array(&island, ISLAND_ENTITIES, "the fixed island's entities")?;
    let entities = read_array(g, entities_at, 4 * count as usize, "the fixed entities")?;
    let mut found = None;
    for k in 0..count as usize {
        let at = be32(&entities, 4 * k);
        let b = one(g, at, BODY_LEN, "a fixed body")?;
        expect_vtable(&b, vtable::RIGID_BODY, at, "hkpRigidBody")?;
        let shape = be32(&b, BODY_SHAPE);
        if shape == 0 {
            continue;
        }
        let head = one(g, shape, 4, "a fixed body's shape")?;
        if be32(&head, 0) != vtable::TRIANGLE_COLLISION_QUERY {
            continue;
        }
        if let Some((first, _, _)) = found {
            bail!("two fixed bodies hold a level mesh: {first:#010x} and {at:#010x}");
        }
        found = Some((at, b, shape));
    }
    let Some((body, b, query)) = found else {
        bail!("no fixed body of the world holds a tfbTriangleCollisionQuery");
    };
    if be32(&b, BODY_WORLD) != world {
        bail!("the level body at {body:#010x} isn't the world's");
    }
    Ok(LevelBody {
        world,
        body,
        bytes: b,
        query,
        fixed_bodies: count as usize,
    })
}

/// The level's collision from [`HKP_WORLD`] down: the world, its fixed
/// island and the one fixed body there with a `tfbTriangleCollisionQuery`,
/// read through to its mesh. Refused: a chain whose vtables aren't theirs, an
/// island that isn't the world's, no such body or a second one, a body that
/// isn't the world's or isn't fixed, and arrays whose counts don't divide or
/// agree.
pub fn collision(g: &mut impl Gather) -> Result<LiveCollision> {
    let LevelBody {
        world,
        body,
        bytes: b,
        query,
        fixed_bodies: count,
    } = level_body(g)?;
    let motion_type = b[BODY_MOTION_TYPE];
    if motion_type != MOTION_FIXED {
        bail!("the level body at {body:#010x} has motion type {motion_type}, not fixed");
    }
    let q = one(g, query, QUERY_LEN, "the triangle collision query")?;
    let mesh = be32(&q, QUERY_CHILD);
    let m = one(g, mesh, MESH_LEN, "the simple mesh shape")?;
    expect_vtable(&m, vtable::SIMPLE_MESH_SHAPE, mesh, "tfbSimpleMeshShape")?;
    let (v_at, v_count) = hk_array(&m, MESH_VERTICES, "vertexArray")?;
    let (i_at, i_count) = hk_array(&m, MESH_INDICES, "indexArray")?;
    let (p_at, p_count) = hk_array(&m, MESH_MATERIALS, "tfbProperties")?;
    if !v_count.is_multiple_of(3) || !i_count.is_multiple_of(4) || p_count != i_count / 4 {
        bail!(
            "the mesh's counts don't agree: {v_count} floats, {i_count} index words, \
             {p_count} materials"
        );
    }
    let mut batch = g.gather(&[
        (v_at, 4 * v_count as usize),
        (i_at, 2 * i_count as usize),
        (p_at, p_count as usize),
    ])?;
    let materials = batch
        .pop()
        .flatten()
        .context("tfbProperties isn't mapped")?;
    let indices = batch.pop().flatten().context("indexArray isn't mapped")?;
    let verts = batch.pop().flatten().context("vertexArray isn't mapped")?;
    let vertices: Vec<[f32; 3]> = (0..v_count as usize / 3)
        .map(|k| floats::<3>(&verts, 12 * k))
        .collect();
    let triangles: Vec<[u16; 4]> = (0..i_count as usize / 4)
        .map(|k| {
            std::array::from_fn(|j| {
                u16::from_be_bytes([indices[8 * k + 2 * j], indices[8 * k + 2 * j + 1]])
            })
        })
        .collect();
    if let Some(t) = triangles
        .iter()
        .find(|t| t[..3].iter().any(|&i| usize::from(i) >= vertices.len()))
    {
        bail!(
            "a triangle {t:?} indexes past the {} vertices",
            vertices.len()
        );
    }
    Ok(LiveCollision {
        world,
        body,
        query,
        mesh,
        fixed_bodies: count,
        motion_type,
        rotation: floats::<12>(&b, BODY_ROTATION),
        translation: floats::<3>(&b, BODY_TRANSLATION),
        radius: f32_at(&m, MESH_RADIUS),
        welding_type: m[MESH_WELDING],
        vertices,
        triangles,
        materials,
    })
}

/// [`collision`] over PINE: Giants 01.00 only, paused before and after.
pub fn read_collision(pine: &mut PineClient) -> Result<LiveCollision> {
    crate::rng::check_title(pine)?;
    let c = collision(pine)?;
    still_paused(pine, "the collision read")?;
    Ok(c)
}

pub(crate) fn still_paused(pine: &mut PineClient, what: &str) -> Result<()> {
    match pine.status()? {
        EmuStatus::Paused => Ok(()),
        other => bail!("PINE reports the emulator {other:?} after {what}; it reads at a pause"),
    }
}

/// A placement's class, as the disc names it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Kind {
    Actor,
    Emitter,
    Light,
}

impl Kind {
    pub fn vtable(self) -> u32 {
        match self {
            Kind::Actor => vtable::ACTOR_INFO,
            Kind::Emitter => vtable::PARTICLE_INFO,
            Kind::Light => vtable::LIGHT_INFO,
        }
    }

    pub fn class(self) -> &'static str {
        match self {
            Kind::Actor => "tfbActorInfo",
            Kind::Emitter => "tfbParticleInfo",
            Kind::Light => "tfbLightInfo",
        }
    }
}

/// One placement as the live section holds it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LivePlacement {
    /// Its offset in the data section, the disc's.
    pub offset: u32,
    pub address: u32,
    pub kind: Kind,
    pub name: Option<String>,
    pub activity_state: u32,
    pub flag_pool: u32,
    /// `_currentMatrix`: position (metres), then tilt, heading, roll.
    pub matrix: [f32; 6],
    pub scale: [f32; 3],
    /// Its `CollisionInfo`, in the section like it.
    pub collision: Option<u32>,
    /// A `tfbActorInfo`'s corpus, which only an active one has.
    pub corpus: Option<u32>,
}

/// Where a level file's data section sits, voted on by the actors whose
/// placement name `offsets` gives one offset for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct SectionBase {
    pub base: u32,
    /// Actors that put the base there.
    pub votes: usize,
    /// Named actors that put it anywhere else (clones, copies of templates).
    pub dissent: usize,
}

/// One vote can be a clone that kept its template's name and sits anywhere on
/// the heap; three placements agreeing on one base by chance can't.
pub const MIN_VOTES: usize = 3;

/// The data section's base from `frame`'s actors: each actor whose name
/// `offsets` maps to a single offset votes for its placement's address less
/// that offset. Refused: fewer than [`MIN_VOTES`] for the leader, or a
/// runner-up with as many.
pub fn section_base(frame: &ActorFrame, offsets: &BTreeMap<String, u32>) -> Result<SectionBase> {
    let mut votes: BTreeMap<u32, usize> = BTreeMap::new();
    let mut named = 0;
    for a in &frame.actors {
        let Some(off) = a.name.as_ref().and_then(|n| offsets.get(n)) else {
            continue;
        };
        named += 1;
        if let Some(base) = a.placement.checked_sub(*off) {
            *votes.entry(base).or_default() += 1;
        }
    }
    let mut ranked: Vec<(u32, usize)> = votes.into_iter().collect();
    ranked.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
    let Some(&(base, n)) = ranked.first() else {
        bail!("no actor's name is a placement the disc lists once");
    };
    if n < MIN_VOTES {
        bail!("the best base, {base:#010x}, has {n} vote(s) of {named}");
    }
    if ranked.get(1).is_some_and(|r| r.1 == n) {
        bail!(
            "two bases tie at {n} votes: {base:#010x} and {:#010x}",
            ranked[1].0
        );
    }
    Ok(SectionBase {
        base,
        votes: n,
        dissent: named - n,
    })
}

/// The disc's placements, `(offset, kind)` each, read at `base`, with their
/// names. Refused: any whose first word isn't its class's vtable (the base is
/// wrong, or the section isn't there).
pub fn placements(
    g: &mut impl Gather,
    base: u32,
    objects: &[(u32, Kind)],
) -> Result<Vec<LivePlacement>> {
    let ranges: Vec<(u32, usize)> = objects
        .iter()
        .map(|&(off, _)| {
            base.checked_add(off)
                .map(|a| (a, PLACEMENT_LEN))
                .context("a placement's address passes 4 GiB")
        })
        .collect::<Result<_>>()?;
    let read = g.gather(&ranges)?;
    let mut names = Vec::new();
    let mut out = Vec::with_capacity(objects.len());
    for ((&(offset, kind), &(address, _)), bytes) in objects.iter().zip(&ranges).zip(read) {
        let bytes = bytes
            .with_context(|| format!("the {} at {address:#010x} isn't mapped", kind.class()))?;
        expect_vtable(&bytes, kind.vtable(), address, kind.class())?;
        let name_at = be32(&bytes, PLACEMENT_NAME);
        if object_ptr(name_at) {
            names.push((out.len(), name_at));
        }
        let ptr = |at: usize| Some(be32(&bytes, at)).filter(|&p| p != 0);
        out.push(LivePlacement {
            offset,
            address,
            kind,
            name: None,
            activity_state: be32(&bytes, PLACEMENT_STATE),
            flag_pool: be32(&bytes, PLACEMENT_FLAG_POOL),
            matrix: floats::<6>(&bytes, PLACEMENT_MATRIX),
            scale: floats::<3>(&bytes, PLACEMENT_SCALE),
            collision: ptr(PLACEMENT_COLLISION),
            corpus: if kind == Kind::Actor {
                ptr(ACTOR_GAME_ACTOR)
            } else {
                None
            },
        });
    }
    let read = g.gather(
        &names
            .iter()
            .map(|&(_, a)| (a, NAME_LEN))
            .collect::<Vec<_>>(),
    )?;
    for ((i, _), bytes) in names.into_iter().zip(read) {
        out[i].name = bytes.map(|b| c_string(&b));
    }
    Ok(out)
}

/// A shape as Havok holds it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Shape {
    /// `hkpConvexTransformShape`: the child placed by a translation, a
    /// rotation quaternion (x, y, z, w) and a scale.
    Transformed {
        translation: [f32; 3],
        rotation: [f32; 4],
        scale: [f32; 3],
        child: Box<Shape>,
    },
    Box {
        half_extents: [f32; 3],
        radius: f32,
    },
    Sphere {
        radius: f32,
        /// Its address: a shapeless `CollisionInfo` gets one shared sphere.
        address: u32,
    },
    Capsule {
        a: [f32; 3],
        b: [f32; 3],
        radius: f32,
    },
    Cylinder {
        a: [f32; 3],
        b: [f32; 3],
        cylinder_radius: f32,
        radius: f32,
    },
    Other {
        address: u32,
        vtable: u32,
    },
}

impl Shape {
    /// What it wraps, through any convex transforms.
    pub fn primitive(&self) -> &Shape {
        match self {
            Shape::Transformed { child, .. } => child.primitive(),
            other => other,
        }
    }
}

fn shape(g: &mut impl Gather, at: u32, depth: usize) -> Result<Shape> {
    if depth > MAX_DEPTH {
        bail!("shapes nest past {MAX_DEPTH} at {at:#010x}");
    }
    let s = one(g, at, SHAPE_LEN, "a shape")?;
    let radius = f32_at(&s, SHAPE_RADIUS);
    Ok(match be32(&s, 0) {
        vtable::CONVEX_TRANSFORM_SHAPE => Shape::Transformed {
            translation: floats::<3>(&s, CT_TRANSLATION),
            rotation: floats::<4>(&s, CT_ROTATION),
            scale: floats::<3>(&s, CT_SCALE),
            child: Box::new(shape(g, be32(&s, CT_CHILD), depth + 1)?),
        },
        vtable::BOX_SHAPE => Shape::Box {
            half_extents: floats::<3>(&s, BOX_HALF),
            radius,
        },
        vtable::SPHERE_SHAPE => Shape::Sphere {
            radius,
            address: at,
        },
        vtable::CAPSULE_SHAPE => Shape::Capsule {
            a: floats::<3>(&s, END_A),
            b: floats::<3>(&s, END_B),
            radius,
        },
        vtable::CYLINDER_SHAPE => Shape::Cylinder {
            a: floats::<3>(&s, END_A),
            b: floats::<3>(&s, END_B),
            cylinder_radius: f32_at(&s, CYLINDER_RADIUS),
            radius,
        },
        other => Shape::Other {
            address: at,
            vtable: other,
        },
    })
}

/// A simulated actor's solid, as its phantom holds it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LiveSolid {
    pub corpus: u32,
    pub placement: u32,
    pub name: Option<String>,
    pub phantom: u32,
    /// The phantom's world; 0 when it was never added to one.
    pub world: u32,
    /// Listed among the world's phantoms.
    pub in_world: bool,
    pub broadphase_id: u32,
    pub filter_info: u32,
    /// Rotation columns, as Havok stores them.
    pub rotation: [[f32; 3]; 3],
    pub translation: [f32; 3],
    pub shape: Shape,
    /// The placement's live `CollisionInfo` and its `_properties` (shape in
    /// bits 0-2, then solid, interacts-with-world, actor surface): scripts
    /// rewrite some at run time, so the disc's can be stale.
    pub collision: Option<u32>,
    pub properties: Option<u32>,
}

/// `v` turned by the unit quaternion `q` (x, y, z, w).
pub fn rotate(q: [f32; 4], v: [f32; 3]) -> [f32; 3] {
    let [x, y, z, w] = q.map(f64::from);
    let v = v.map(f64::from);
    // t = 2 q.xyz x v; v' = v + w t + q.xyz x t
    let t = [
        2.0 * (y * v[2] - z * v[1]),
        2.0 * (z * v[0] - x * v[2]),
        2.0 * (x * v[1] - y * v[0]),
    ];
    let c = [
        y * t[2] - z * t[1],
        z * t[0] - x * t[2],
        x * t[1] - y * t[0],
    ];
    std::array::from_fn(|i| (v[i] + w * t[i] + c[i]) as f32)
}

impl LiveSolid {
    /// Where the primitive's own x, y and z run in the world: through every
    /// convex transform's rotation, then the phantom's.
    pub fn primitive_axes(&self) -> [[f32; 3]; 3] {
        let mut axes = [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]];
        let mut quats = Vec::new();
        let mut s = &self.shape;
        while let Shape::Transformed {
            rotation, child, ..
        } = s
        {
            quats.push(*rotation);
            s = child;
        }
        for q in quats.iter().rev() {
            axes = axes.map(|a| rotate(*q, a));
        }
        let r = self.rotation;
        axes.map(|a| std::array::from_fn(|i| (0..3).map(|j| r[j][i] * a[j]).sum()))
    }
}

/// Every simulated actor's solid in `frame`: corpus, `_animationBlock`,
/// phantom, shape chain. An actor whose block holds no phantom has none.
/// Refused: a block or phantom whose vtable isn't its class's.
pub fn solids(g: &mut impl Gather, frame: &ActorFrame) -> Result<Vec<LiveSolid>> {
    let world = be32(&one(g, HKP_WORLD, 4, "the hkpWorld pointer")?, 0);
    let w = one(g, world, WORLD_LEN, "the hkpWorld")?;
    expect_vtable(&w, vtable::WORLD, world, "hkpWorld")?;
    let (listed_at, listed_count) = hk_array(&w, WORLD_PHANTOMS, "the world's phantoms")?;
    let listed = read_array(
        g,
        listed_at,
        4 * listed_count as usize,
        "the world's phantom list",
    )?;
    let in_world: BTreeSet<u32> = (0..listed_count as usize)
        .map(|k| be32(&listed, 4 * k))
        .collect();
    let mut out = Vec::new();
    for a in &frame.actors {
        let corpus = one(g, a.corpus, actors::CORPUS_LEN, "a corpus")?;
        let block = be32(&corpus, CORPUS_ANIMATION_BLOCK);
        if block == 0 {
            continue;
        }
        let c = one(g, block, COMPONENT_LEN, "an animation block")?;
        expect_vtable(
            &c,
            actors::vtable::ANIMATION_COMPONENT,
            block,
            "animationComponent",
        )?;
        let phantom = be32(&c, COMPONENT_PHANTOM);
        if phantom == 0 {
            continue;
        }
        let p = one(g, phantom, PHANTOM_LEN, "a phantom")?;
        expect_vtable(&p, vtable::SHAPE_PHANTOM, phantom, "tfbShapePhanton")?;
        let columns: [[f32; 4]; 3] =
            std::array::from_fn(|k| floats::<4>(&p, PHANTOM_ROTATION + 16 * k));
        let placed = one(g, a.placement, PLACEMENT_LEN, "a placement")?;
        let collision = Some(be32(&placed, PLACEMENT_COLLISION)).filter(|&c| c != 0);
        let properties = match collision {
            Some(c) => Some(be32(
                &one(g, c, COLLISION_LEN, "a CollisionInfo")?,
                COLLISION_PROPERTIES,
            )),
            None => None,
        };
        out.push(LiveSolid {
            corpus: a.corpus,
            placement: a.placement,
            name: a.name.clone(),
            phantom,
            world: be32(&p, PHANTOM_WORLD),
            in_world: in_world.contains(&phantom),
            broadphase_id: be32(&p, PHANTOM_BROADPHASE),
            filter_info: be32(&p, PHANTOM_FILTER),
            rotation: columns.map(|c| [c[0], c[1], c[2]]),
            translation: floats::<3>(&p, PHANTOM_TRANSLATION),
            shape: shape(g, be32(&p, PHANTOM_SHAPE), 0)?,
            collision,
            properties,
        });
    }
    Ok(out)
}

/// [`solids`] over PINE: Giants 01.00 only, paused before and after.
pub fn read_solids(pine: &mut PineClient, frame: &ActorFrame) -> Result<Vec<LiveSolid>> {
    crate::rng::check_title(pine)?;
    let s = solids(pine, frame)?;
    still_paused(pine, "the solids read")?;
    Ok(s)
}

/// [`placements`] over PINE: Giants 01.00 only, paused before and after.
pub fn read_placements(
    pine: &mut PineClient,
    base: u32,
    objects: &[(u32, Kind)],
) -> Result<Vec<LivePlacement>> {
    crate::rng::check_title(pine)?;
    let p = placements(pine, base, objects)?;
    still_paused(pine, "the placement read")?;
    Ok(p)
}

#[cfg(test)]
mod tests;
