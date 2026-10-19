//! Havok 2011.2 physics packfiles (openspec AG.2): a level's collision, the
//! payload `tfbPhysicsWorld._data` holds ([`crate::collision`] reads that
//! wrapper).
//!
//! ag-levels found one chain in all 75 worlds on the disc, and nothing else
//! is read: `hkpPhysicsData` → one `hkpPhysicsSystem` → at most one fixed
//! `hkpRigidBody` → `tfbTriangleCollisionQuery` (an `hkpMoppBvTreeShape`) →
//! `tfbSimpleMeshShape` (an `hkpSimpleMeshShape`), whose own arrays hold f32
//! vertices, u16 triangles with a welding word each and a u8 material per
//! triangle.
//!
//! - Offsets are ag-levels' reading of the data, each class pinned by its
//!   2011.2 signature ([`SIGNATURES`]); field names are the EBOOT's member
//!   names. The two tfb classes' sizes are the EBOOT's registrations, and
//!   no other object may start inside one.
//! - The MOPP code is skipped: it only speeds up queries over triangles held
//!   here (its info and length are kept).
//! - Refused by name, since the disc never shows one: another shape class, a
//!   second system or body, a body that isn't fixed or is rotated, a
//!   triangle index past the vertices, and counts that don't divide or
//!   don't agree.

use std::collections::BTreeMap;

use super::classes::Ctx;
use super::{CLASSNAMES, Packfile, Ptr, RootLevelContainer, err, unsupported};
use crate::{ErrorKind, FormatError};

/// The classes read here and their 2011.2 signatures, as every collision
/// packfile on the disc lists them.
pub const SIGNATURES: [(&str, u32); 7] = [
    (PhysicsData::CLASS, 0xC2A4_61E4),
    (WORLD_CINFO, 0x03E4_D5AF),
    (PhysicsSystem::CLASS, 0xFF72_4C17),
    (RigidBody::CLASS, 0x0921_DC44),
    (TriangleCollisionQuery::CLASS, 0x168B_7066),
    (MoppCode::CLASS, 0x924C_2661),
    (SimpleMeshShape::CLASS, 0x9B7A_D9F7),
];

/// What `hkpPhysicsData.worldCinfo` points at. Only its class is checked:
/// its gravity (-9.81) and broadphase (±500) don't describe how Giants
/// moves anything (ag-levels: 17 meshes reach past that broadphase, and
/// the hero's gravity is 128 in its `ActorParameters`).
pub const WORLD_CINFO: &str = "hkpWorldCinfo";

/// `hkpPhysicsData`: the root's "Physics Data" variant.
pub struct PhysicsData;

impl PhysicsData {
    pub const CLASS: &'static str = "hkpPhysicsData";
    pub const WORLD_CINFO: usize = 0x08;
    pub const SYSTEMS: usize = 0x0C;
}

/// `hkpPhysicsSystem` ("Fixed Rigid Bodies" on the disc); constraints,
/// actions and phantoms must be empty.
pub struct PhysicsSystem;

impl PhysicsSystem {
    pub const CLASS: &'static str = "hkpPhysicsSystem";
    pub const RIGID_BODIES: usize = 0x08;
    pub const CONSTRAINTS: usize = 0x14;
    pub const ACTIONS: usize = 0x20;
    pub const PHANTOMS: usize = 0x2C;
}

/// `hkpRigidBody`: where the mesh sits and the shape that holds it.
pub struct RigidBody;

impl RigidBody {
    pub const CLASS: &'static str = "hkpRigidBody";
    /// `collidable.shape`.
    pub const SHAPE: usize = 0x10;
    /// `motion.type`, one byte.
    pub const MOTION_TYPE: usize = 0xE8;
    /// `motion.motionState.transform`: three rotation columns of four
    /// floats, then the translation.
    pub const ROTATION: usize = 0xF0;
    pub const TRANSLATION: usize = 0x120;
    /// `hkpMotion::MOTION_FIXED`, every body's on the disc.
    pub const FIXED: u8 = 5;
}

/// `tfbTriangleCollisionQuery`, Toys for Bob's `hkpMoppBvTreeShape`.
pub struct TriangleCollisionQuery;

impl TriangleCollisionQuery {
    pub const CLASS: &'static str = "tfbTriangleCollisionQuery";
    /// The EBOOT's registration, over `hkpMoppBvTreeShape`'s 0x40.
    pub const SIZE: usize = 0x70;
    pub const CODE: usize = 0x14;
    /// `child.childShape`.
    pub const CHILD_SHAPE: usize = 0x34;
    /// Two arrays of its own, empty in every world on the disc.
    pub const OWN_ARRAYS: [(usize, &'static str); 2] =
        [(0x44, "query array 0x44"), (0x50, "query array 0x50")];
}

/// `hkpMoppCode`: the tree over the mesh, kept as its info and length.
pub struct MoppCode;

impl MoppCode {
    pub const CLASS: &'static str = "hkpMoppCode";
    /// The code's origin, then its scale (2^24 over about the largest extent).
    pub const INFO: usize = 0x10;
    pub const DATA: usize = 0x20;
}

/// `tfbSimpleMeshShape`: the mesh, in arrays of its own; its parent's are
/// empty.
pub struct SimpleMeshShape;

impl SimpleMeshShape {
    pub const CLASS: &'static str = "tfbSimpleMeshShape";
    /// The EBOOT's registration, over `hkpSimpleMeshShape`'s 0x44.
    pub const SIZE: usize = 0x68;
    /// `hkpSimpleMeshShape`'s arrays, empty on the disc.
    pub const VERTICES: usize = 0x18;
    pub const TRIANGLES: usize = 0x24;
    pub const MATERIAL_INDICES: usize = 0x30;
    pub const RADIUS: usize = 0x3C;
    pub const WELDING_TYPE: usize = 0x40;
    /// Flat f32 x, y, z.
    pub const VERTEX_ARRAY: usize = 0x44;
    /// u16 a, b, c and a welding word per triangle.
    pub const INDEX_ARRAY: usize = 0x50;
    /// One u8 material per triangle.
    pub const TFB_PROPERTIES: usize = 0x5C;
}

/// One triangle: three vertex indices and Havok's welding word, which the
/// extract drops (a trimesh's own internal-edge fix stands in for it).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Triangle {
    pub indices: [u16; 3],
    pub welding: u16,
}

/// The mesh, body-local.
#[derive(Debug, Clone, PartialEq)]
pub struct Mesh {
    pub vertices: Vec<[f32; 3]>,
    pub triangles: Vec<Triangle>,
    /// One per triangle. What each value means is open (ag-levels: 28
    /// values on the disc, a bitfield by the look of it).
    pub materials: Vec<u8>,
    /// The shape's convex radius (0.05 on the disc).
    pub radius: f32,
    /// `weldingType` (4 on the disc).
    pub welding_type: u8,
}

impl Mesh {
    /// Triangles per material, keyed by the byte (the tally ag-levels
    /// printed per level).
    pub fn material_counts(&self) -> BTreeMap<u8, usize> {
        let mut out = BTreeMap::new();
        for m in &self.materials {
            *out.entry(*m).or_default() += 1;
        }
        out
    }

    /// The vertices' bounds, `None` without any.
    pub fn bounds(&self) -> Option<([f32; 3], [f32; 3])> {
        let first = *self.vertices.first()?;
        Some(self.vertices.iter().fold((first, first), |(lo, hi), v| {
            (
                std::array::from_fn(|i| lo[i].min(v[i])),
                std::array::from_fn(|i| hi[i].max(v[i])),
            )
        }))
    }
}

/// The MOPP code's info; its bytes are skipped.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Mopp {
    pub offset: [f32; 3],
    pub scale: f32,
    pub code_len: usize,
}

/// The fixed body: where its mesh sits in the level, and the mesh.
#[derive(Debug, Clone, PartialEq)]
pub struct Body {
    /// Added to each vertex to place it; the rotation is identity.
    pub translation: [f32; 3],
    pub mopp: Mopp,
    pub mesh: Mesh,
}

impl Body {
    /// The vertices where the level has them: body-local plus the
    /// translation, in f32 as the game adds them.
    pub fn world_vertices(&self) -> impl ExactSizeIterator<Item = [f32; 3]> + '_ {
        let t = self.translation;
        self.mesh
            .vertices
            .iter()
            .map(move |v| [v[0] + t[0], v[1] + t[1], v[2] + t[2]])
    }
}

/// A world's collision: the one system's body, if it has one (7 of the
/// disc's 75 worlds don't, a grass scene's among them).
#[derive(Debug, Clone, PartialEq)]
pub struct Collision {
    pub body: Option<Body>,
}

impl Collision {
    pub fn parse(bytes: &[u8]) -> Result<Self, FormatError> {
        let pf = Packfile::parse(bytes)?;
        signatures(&pf)?;
        let mut ctx = Ctx {
            pf: &pf,
            claims: BTreeMap::new(),
        };
        let data = physics_data(&mut ctx)?;
        ctx.object(data, PhysicsData::WORLD_CINFO, "worldCinfo", WORLD_CINFO)?;
        let systems = ctx.objects(data, PhysicsData::SYSTEMS, "systems", PhysicsSystem::CLASS)?;
        let [system] = systems[..] else {
            let kind = ErrorKind::CountMismatch {
                found: systems.len() as u64,
                expected: 1,
            };
            return Err(err("systems", ctx.place(data, PhysicsData::SYSTEMS), kind));
        };
        type S = PhysicsSystem;
        let bodies = ctx.objects(system, S::RIGID_BODIES, "rigidBodies", RigidBody::CLASS)?;
        if bodies.len() > 1 {
            let kind = ErrorKind::CountMismatch {
                found: bodies.len() as u64,
                expected: 1,
            };
            let place = ctx.place(system, S::RIGID_BODIES);
            return Err(err("rigidBodies", place, kind));
        }
        for (offset, field) in [
            (S::CONSTRAINTS, "constraints"),
            (S::ACTIONS, "actions"),
            (S::PHANTOMS, "phantoms"),
        ] {
            ctx.empty(system, offset, field)?;
        }
        let body = match bodies.first() {
            Some(&b) => Some(body(&mut ctx, b)?),
            None => None,
        };
        Ok(Self { body })
    }
}

/// The classes read here must carry their 2011.2 signatures.
fn signatures(pf: &Packfile<'_>) -> Result<(), FormatError> {
    let start = pf
        .sections
        .iter()
        .find(|s| s.tag == CLASSNAMES)
        .map_or(0, |s| s.start);
    for (name_at, c) in &pf.classes {
        if let Some((_, want)) = SIGNATURES.iter().find(|(n, _)| *n == c.name)
            && *want != c.signature
        {
            let at = (start + name_at).saturating_sub(5);
            return Err(unsupported("class signature", at, u64::from(c.signature)));
        }
    }
    Ok(())
}

/// The root's one `hkpPhysicsData` variant; others are passed over.
fn physics_data(ctx: &mut Ctx<'_, '_>) -> Result<Ptr, FormatError> {
    let pf = ctx.pf;
    let size = RootLevelContainer::VARIANT_SIZE;
    let a = ctx.array(
        pf.root,
        RootLevelContainer::NAMED_VARIANTS,
        size,
        "namedVariants",
    )?;
    let mut data = None;
    for i in 0..a.len {
        let v = a.element(i, size);
        pf.string(v, 0, "variant name")?;
        let class = pf.string(v, 4, "variant className")?;
        let Some(obj) = pf.pointer(v, 8, "variant")? else {
            continue;
        };
        let found = pf.class_of(obj);
        if found.is_none() || found != class {
            let kind = ErrorKind::WrongType {
                expected: "the variant's className",
                found: found.unwrap_or("no object").to_owned(),
            };
            return Err(err("variant", ctx.place(v, 8), kind));
        }
        if class == Some(PhysicsData::CLASS) && data.replace(obj).is_some() {
            return Err(err("variant", ctx.place(v, 8), ErrorKind::Duplicate));
        }
    }
    let place = pf.abs(pf.root);
    data.ok_or_else(|| err("namedVariants", place, ErrorKind::Missing))
}

/// A tfb class's registered size fits its section, and no other object
/// starts inside it.
fn extent(ctx: &Ctx<'_, '_>, at: Ptr, size: usize, class: &'static str) -> Result<(), FormatError> {
    ctx.pf.bytes(at, 0, size, class)?;
    let inside = ctx.pf.objects().find(|(p, _)| {
        p.section == at.section && p.offset > at.offset && p.offset < at.offset + size
    });
    match inside {
        Some((p, _)) => {
            let value = (p.offset - at.offset) as u64;
            Err(unsupported("registered size", ctx.pf.abs(at), value))
        }
        None => Ok(()),
    }
}

/// A null pointer is refused as missing, another class as the wrong type.
fn required(
    ctx: &Ctx<'_, '_>,
    at: Ptr,
    offset: usize,
    field: &'static str,
    class: &'static str,
) -> Result<Ptr, FormatError> {
    ctx.object(at, offset, field, class)?
        .ok_or_else(|| err(field, ctx.place(at, offset), ErrorKind::Missing))
}

fn body(ctx: &mut Ctx<'_, '_>, at: Ptr) -> Result<Body, FormatError> {
    type R = RigidBody;
    let pf = ctx.pf;
    let motion = pf.u8(at, R::MOTION_TYPE, "motion type")?;
    if motion != R::FIXED {
        let place = ctx.place(at, R::MOTION_TYPE);
        return Err(unsupported("motion type", place, u64::from(motion)));
    }
    for column in 0..3 {
        for row in 0..4 {
            let offset = R::ROTATION + 16 * column + 4 * row;
            let v = pf.f32(at, offset, "rotation")?;
            if v != f32::from(u8::from(row == column)) {
                let place = ctx.place(at, offset);
                return Err(unsupported("rotation", place, u64::from(v.to_bits())));
            }
        }
    }
    let mut translation = [0.0; 3];
    for (i, t) in translation.iter_mut().enumerate() {
        *t = ctx.finite(at, R::TRANSLATION + 4 * i, "translation")?;
    }

    type Q = TriangleCollisionQuery;
    let query = required(ctx, at, R::SHAPE, "shape", Q::CLASS)?;
    extent(ctx, query, Q::SIZE, Q::CLASS)?;
    let code = required(ctx, query, Q::CODE, "code", MoppCode::CLASS)?;
    let child = required(
        ctx,
        query,
        Q::CHILD_SHAPE,
        "childShape",
        SimpleMeshShape::CLASS,
    )?;
    for (offset, field) in Q::OWN_ARRAYS {
        ctx.empty(query, offset, field)?;
    }
    let mopp = mopp(ctx, code)?;
    let mesh = mesh(ctx, child)?;
    Ok(Body {
        translation,
        mopp,
        mesh,
    })
}

fn mopp(ctx: &mut Ctx<'_, '_>, at: Ptr) -> Result<Mopp, FormatError> {
    let mut info = [0.0; 4];
    for (i, v) in info.iter_mut().enumerate() {
        *v = ctx.finite(at, MoppCode::INFO + 4 * i, "info")?;
    }
    let code = ctx.array(at, MoppCode::DATA, 1, "data")?;
    Ok(Mopp {
        offset: [info[0], info[1], info[2]],
        scale: info[3],
        code_len: code.len,
    })
}

fn mesh(ctx: &mut Ctx<'_, '_>, at: Ptr) -> Result<Mesh, FormatError> {
    type S = SimpleMeshShape;
    let pf = ctx.pf;
    extent(ctx, at, S::SIZE, S::CLASS)?;
    for (offset, field) in [
        (S::VERTICES, "vertices"),
        (S::TRIANGLES, "triangles"),
        (S::MATERIAL_INDICES, "materialIndices"),
    ] {
        ctx.empty(at, offset, field)?;
    }
    let radius = ctx.finite(at, S::RADIUS, "radius")?;
    let welding_type = pf.u8(at, S::WELDING_TYPE, "weldingType")?;

    let a = ctx.array(at, S::VERTEX_ARRAY, 4, "vertexArray")?;
    if a.len % 3 != 0 {
        let place = ctx.place(at, S::VERTEX_ARRAY + 4);
        return Err(unsupported("vertexArray", place, a.len as u64));
    }
    let floats = pf.bytes(a.at, 0, 4 * a.len, "vertexArray")?;
    let base = pf.abs(a.at);
    let mut vertices = Vec::with_capacity(a.len / 3);
    for (k, v) in floats.as_chunks::<12>().0.iter().enumerate() {
        let xyz: [f32; 3] = std::array::from_fn(|i| {
            f32::from_be_bytes([v[4 * i], v[4 * i + 1], v[4 * i + 2], v[4 * i + 3]])
        });
        if let Some(i) = xyz.iter().position(|x| !x.is_finite()) {
            let place = base + 12 * k + 4 * i;
            return Err(unsupported(
                "vertexArray",
                place,
                u64::from(xyz[i].to_bits()),
            ));
        }
        vertices.push(xyz);
    }

    let a = ctx.array(at, S::INDEX_ARRAY, 2, "indexArray")?;
    if a.len % 4 != 0 {
        let place = ctx.place(at, S::INDEX_ARRAY + 4);
        return Err(unsupported("indexArray", place, a.len as u64));
    }
    let words = pf.bytes(a.at, 0, 2 * a.len, "indexArray")?;
    let base = pf.abs(a.at);
    let mut triangles = Vec::with_capacity(a.len / 4);
    for (k, t) in words.as_chunks::<8>().0.iter().enumerate() {
        let w: [u16; 4] = std::array::from_fn(|i| u16::from_be_bytes([t[2 * i], t[2 * i + 1]]));
        if let Some(i) = (0..3).find(|&i| usize::from(w[i]) >= vertices.len()) {
            let kind = ErrorKind::OutOfRange {
                value: u64::from(w[i]),
                limit: vertices.len() as u64,
            };
            return Err(err("indexArray", base + 8 * k + 2 * i, kind));
        }
        triangles.push(Triangle {
            indices: [w[0], w[1], w[2]],
            welding: w[3],
        });
    }

    let a = ctx.array(at, S::TFB_PROPERTIES, 1, "tfbProperties")?;
    if a.len != triangles.len() {
        let kind = ErrorKind::CountMismatch {
            found: a.len as u64,
            expected: triangles.len() as u64,
        };
        return Err(err(
            "tfbProperties",
            ctx.place(at, S::TFB_PROPERTIES + 4),
            kind,
        ));
    }
    let materials = pf.bytes(a.at, 0, a.len, "tfbProperties")?.to_vec();
    Ok(Mesh {
        vertices,
        triangles,
        materials,
        radius,
        welding_type,
    })
}
