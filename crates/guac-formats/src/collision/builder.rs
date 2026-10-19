//! Synthetic collision for tests and fuzz seeds (`test-fixtures`): Havok
//! packfiles laid out as the disc's worlds are (their classes in the disc's
//! order, three sections, the same empty arrays and constants), every value
//! known, and level IGZ files holding them in `tfbPhysicsWorld`s.

use crate::havok::builder::{Data, REFLECTION, RESOURCE, SectionSpec, class_names, packfile};
use crate::havok::physics::{
    MoppCode, PhysicsData, PhysicsSystem, RigidBody, SIGNATURES, SimpleMeshShape,
    TriangleCollisionQuery, WORLD_CINFO,
};
use crate::havok::{CLASSNAMES, RootLevelContainer};
use crate::igz::Location;
use crate::igz::builder::IgzBuilder;
use crate::igz::layouts::{Layout, PhysicsWorld};

/// The root's class and 2011.2 signature.
pub const ROOT: (&str, u32) = (RootLevelContainer::CLASS, 0x2772_C11E);
/// What the disc's `hkpWorldCinfo` points at; never read.
pub const FILTER: (&str, u32) = ("hkpConstraintCollisionFilter", 0xC3B5_77B1);
/// The size of each object the reader doesn't size itself, as the disc's
/// spacing shows them.
pub const PHYSICS_DATA_SIZE: usize = 0x18;
pub const WORLD_CINFO_SIZE: usize = 0xF0;
pub const SYSTEM_SIZE: usize = 0x44;
pub const RIGID_BODY_SIZE: usize = 0x220;
pub const MOPP_CODE_SIZE: usize = 0x30;

/// A mesh shape's arrays, written as given, so counts that don't divide or
/// don't agree can be built too.
#[derive(Debug, Clone, PartialEq)]
pub struct MeshSpec {
    /// Flat x, y, z.
    pub floats: Vec<f32>,
    /// Flat a, b, c and the welding word.
    pub words: Vec<u16>,
    pub materials: Vec<u8>,
}

impl MeshSpec {
    pub fn new(vertices: &[[f32; 3]], triangles: &[[u16; 4]], materials: &[u8]) -> Self {
        Self {
            floats: vertices.concat(),
            words: triangles.concat(),
            materials: materials.to_vec(),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct BodySpec {
    pub translation: [f32; 3],
    /// Rotation columns, identity on the disc.
    pub rotation: [[f32; 4]; 3],
    pub motion_type: u8,
    /// The body's shape and that shape's child: class and signature.
    pub shape: (&'static str, u32),
    pub child: (&'static str, u32),
    /// The MOPP code's offset and scale, and its length.
    pub mopp_info: [f32; 4],
    pub mopp_len: usize,
    pub mesh: MeshSpec,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct WorldSpec {
    /// One entry per `hkpPhysicsSystem`: the disc has exactly one, holding
    /// at most one body.
    pub systems: Vec<Vec<BodySpec>>,
    /// Pointers through global fixups, as the disc's object pointers are,
    /// instead of local ones.
    pub global: bool,
}

/// Where the builder put things, as offsets into the data section.
#[derive(Debug, Clone, Default)]
pub struct Built {
    pub bytes: Vec<u8>,
    /// Absolute offset of the data section.
    pub data_start: usize,
    pub physics_data: u32,
    pub systems: Vec<u32>,
    pub bodies: Vec<u32>,
    pub queries: Vec<u32>,
    pub mopp_codes: Vec<u32>,
    pub meshes: Vec<u32>,
    /// Each mesh's index array, `None` when empty.
    pub index_arrays: Vec<Option<u32>>,
}

impl Built {
    pub fn at(&self, offset: u32) -> usize {
        self.data_start + offset as usize
    }

    pub fn put_u32(&mut self, offset: u32, v: u32) {
        let at = self.at(offset);
        self.bytes[at..at + 4].copy_from_slice(&v.to_be_bytes());
    }

    pub fn put_u16(&mut self, offset: u32, v: u16) {
        let at = self.at(offset);
        self.bytes[at..at + 2].copy_from_slice(&v.to_be_bytes());
    }
}

const fn off(offset: usize) -> u32 {
    offset as u32
}

/// The classes in the order the disc's `__classnames__` lists them, then
/// any other class a body names.
fn names(spec: &WorldSpec) -> Vec<(&'static str, u32)> {
    let sig = |name: &str| {
        SIGNATURES
            .iter()
            .find(|(n, _)| *n == name)
            .map_or(0, |(_, s)| *s)
    };
    let mut names = REFLECTION.to_vec();
    names.extend([ROOT, RESOURCE]);
    names.extend([PhysicsData::CLASS, WORLD_CINFO].map(|n| (n, sig(n))));
    names.push(FILTER);
    for n in [
        PhysicsSystem::CLASS,
        RigidBody::CLASS,
        TriangleCollisionQuery::CLASS,
        MoppCode::CLASS,
        SimpleMeshShape::CLASS,
    ] {
        names.push((n, sig(n)));
    }
    for b in spec.systems.iter().flatten() {
        for c in [b.shape, b.child] {
            if !names.iter().any(|(n, _)| *n == c.0) {
                names.push(c);
            }
        }
    }
    names
}

/// A whole packfile: the root's "Resource Data" and "Physics Data"
/// variants, the world info and its filter, then each system and body.
pub fn world_packfile(spec: &WorldSpec) -> Built {
    let names = names(spec);
    let (cn_data, cn) = class_names(&names);
    let class = |name: &str| cn[names.iter().position(|(n, _)| *n == name).unwrap_or(0)];
    let data_index = 2;
    let mut d = Data {
        global_to: spec.global.then_some(data_index),
        ..Data::default()
    };
    let mut built = Built::default();

    let root = d.object(class(ROOT.0), RootLevelContainer::VARIANT_SIZE);
    let resource = d.object(class(RESOURCE.0), 0x28);
    let data = d.object(class(PhysicsData::CLASS), PHYSICS_DATA_SIZE);
    built.physics_data = data;
    let variants = [
        ("Resource Data", RESOURCE.0, resource),
        ("Physics Data", PhysicsData::CLASS, data),
    ];
    d.array_of(root, RootLevelContainer::VARIANT_SIZE, 2, |d, v, i| {
        let (name, cls, obj) = variants[i];
        d.string_field(v, name);
        d.string_field(v + 4, cls);
        d.ptr(v + 8, obj);
    });
    d.string_field(resource + 0x08, "");
    d.array(resource + 0x10, None, 0);
    d.array(resource + 0x1C, None, 0);

    let cinfo = d.object(class(WORLD_CINFO), WORLD_CINFO_SIZE);
    d.ptr(data + off(PhysicsData::WORLD_CINFO), cinfo);
    d.f32(cinfo + 0x14, -9.81);
    for k in 0..3 {
        d.f32(cinfo + 0x30 + 4 * k, -500.0);
        d.f32(cinfo + 0x40 + 4 * k, 500.0);
    }
    let filter = d.object(class(FILTER.0), 0x50);
    d.ptr(cinfo + 0x58, filter);

    type S = PhysicsSystem;
    let systems: Vec<u32> = spec
        .systems
        .iter()
        .map(|_| d.object(class(S::CLASS), SYSTEM_SIZE))
        .collect();
    d.array_of(
        data + off(PhysicsData::SYSTEMS),
        4,
        systems.len(),
        |d, e, i| d.ptr(e, systems[i]),
    );
    for (&s, bodies) in systems.iter().zip(&spec.systems) {
        let at: Vec<u32> = bodies
            .iter()
            .map(|b| body(&mut d, &class, b, &mut built))
            .collect();
        d.array_of(s + off(S::RIGID_BODIES), 4, at.len(), |d, e, i| {
            d.ptr(e, at[i])
        });
        for empty in [S::CONSTRAINTS, S::ACTIONS, S::PHANTOMS] {
            d.array(s + off(empty), None, 0);
        }
        d.string_field(s + 0x38, "Fixed Rigid Bodies");
        d.bytes[s as usize + 0x40] = 1;
    }
    built.systems = systems;

    let sections = [
        SectionSpec {
            tag: CLASSNAMES,
            data: cn_data,
            ..SectionSpec::default()
        },
        SectionSpec {
            tag: "__types__",
            ..SectionSpec::default()
        },
        SectionSpec {
            tag: "__data__",
            data: d.bytes,
            local: d.local,
            global: d.global,
            virtual_: d.virtual_,
        },
    ];
    built.bytes = packfile(&sections, (data_index, root), (0, class(ROOT.0)));
    built.data_start = 0x40 + 0x30 * sections.len() + sections[0].data.len().next_multiple_of(16);
    built
}

/// One body, its query, MOPP code and mesh; returns the body.
fn body(d: &mut Data, class: &dyn Fn(&str) -> u32, b: &BodySpec, built: &mut Built) -> u32 {
    type R = RigidBody;
    type Q = TriangleCollisionQuery;
    type M = SimpleMeshShape;
    let at = d.object(class(R::CLASS), RIGID_BODY_SIZE);
    let query = d.object(class(b.shape.0), Q::SIZE);
    let code = d.object(class(MoppCode::CLASS), MOPP_CODE_SIZE);
    let mesh = d.object(class(b.child.0), M::SIZE);
    built.bodies.push(at);
    built.queries.push(query);
    built.mopp_codes.push(code);
    built.meshes.push(mesh);

    d.ptr(at + off(R::SHAPE), query);
    d.u32(at + 0x14, u32::MAX);
    d.bytes[at as usize + R::MOTION_TYPE] = b.motion_type;
    for (c, column) in b.rotation.iter().enumerate() {
        for (r, v) in column.iter().enumerate() {
            d.f32(at + off(R::ROTATION + 16 * c + 4 * r), *v);
        }
    }
    // The swept transform's two centres of mass sit on the translation and
    // its two rotations are identity quaternions, as on the disc.
    for (k, t) in b.translation.iter().enumerate() {
        for base in [R::TRANSLATION, 0x130, 0x140] {
            d.f32(at + off(base + 4 * k), *t);
        }
    }
    d.f32(at + 0x15C, 1.0);
    d.f32(at + 0x16C, 1.0);

    d.ptr(query + off(Q::CODE), code);
    d.ptr(query + off(Q::CHILD_SHAPE), mesh);
    for (offset, _) in Q::OWN_ARRAYS {
        d.array(query + off(offset), None, 0);
    }

    for (k, v) in b.mopp_info.iter().enumerate() {
        d.f32(code + off(MoppCode::INFO + 4 * k), *v);
    }
    d.array_of(code + off(MoppCode::DATA), 1, b.mopp_len, |d, e, i| {
        d.bytes[e as usize] = (i * 7) as u8
    });
    d.bytes[code as usize + 0x2C] = 1;

    // hkpShapeCollection's disableWelding and collectionType.
    d.put(mesh + 0x14, &[1, 4]);
    for empty in [M::VERTICES, M::TRIANGLES, M::MATERIAL_INDICES] {
        d.array(mesh + off(empty), None, 0);
    }
    d.f32(mesh + off(M::RADIUS), 0.05);
    d.bytes[mesh as usize + M::WELDING_TYPE] = 4;
    let m = &b.mesh;
    d.array_of(mesh + off(M::VERTEX_ARRAY), 4, m.floats.len(), |d, e, i| {
        d.f32(e, m.floats[i])
    });
    let next = d.bytes.len().next_multiple_of(16) as u32;
    built
        .index_arrays
        .push((!m.words.is_empty()).then_some(next));
    d.array_of(mesh + off(M::INDEX_ARRAY), 2, m.words.len(), |d, e, i| {
        d.put(e, &m.words[i].to_be_bytes())
    });
    d.array_of(
        mesh + off(M::TFB_PROPERTIES),
        1,
        m.materials.len(),
        |d, e, i| d.bytes[e as usize] = m.materials[i],
    );
    at
}

/// A ramp over a 2 x 2 square, rising 1 towards +z: two triangles,
/// materials 0 and 40, welding words 0x0101 and 0x0202.
pub fn ramp() -> MeshSpec {
    MeshSpec::new(
        &[
            [0.0, 0.0, 0.0],
            [2.0, 0.0, 0.0],
            [2.0, 1.0, 2.0],
            [0.0, 1.0, 2.0],
        ],
        &[[0, 2, 1, 0x0101], [0, 3, 2, 0x0202]],
        &[0, 40],
    )
}

/// A flat `n` x `n` grid of unit quads, two triangles each, the materials
/// counting up.
pub fn grid(n: u16) -> MeshSpec {
    let side = n + 1;
    let vertices: Vec<[f32; 3]> = (0..side * side)
        .map(|i| [f32::from(i % side), 0.0, f32::from(i / side)])
        .collect();
    let mut triangles = Vec::new();
    for z in 0..n {
        for x in 0..n {
            let a = z * side + x;
            triangles.push([a, a + side, a + 1, 0x0100]);
            triangles.push([a + 1, a + side, a + side + 1, 0x0001]);
        }
    }
    let materials: Vec<u8> = (0..triangles.len()).map(|i| i as u8).collect();
    MeshSpec::new(&vertices, &triangles, &materials)
}

/// A fixed, unrotated body at an off-origin translation, as level 213's.
pub fn body_spec(mesh: MeshSpec) -> BodySpec {
    BodySpec {
        translation: [-425.5, -37.625, -16.0],
        rotation: [
            [1.0, 0.0, 0.0, 0.0],
            [0.0, 1.0, 0.0, 0.0],
            [0.0, 0.0, 1.0, 0.0],
        ],
        motion_type: RigidBody::FIXED,
        shape: (TriangleCollisionQuery::CLASS, 0x168B_7066),
        child: (SimpleMeshShape::CLASS, 0x9B7A_D9F7),
        mopp_info: [-1.0, -1.0, -1.0, 16_777_216.0 / 3.0],
        mopp_len: 24,
        mesh,
    }
}

/// One system holding these bodies, its pointers global as the disc's.
pub fn world_spec(bodies: Vec<BodySpec>) -> WorldSpec {
    WorldSpec {
        systems: vec![bodies],
        global: true,
    }
}

fn at(base: Location, offset: u32) -> Location {
    base.offset_by(offset).expect("inside the section")
}

/// Writes a world's fields bar `_data`; returns the world.
fn world_object(b: &mut IgzBuilder, name: &str) -> Location {
    let world = b.object(0, PhysicsWorld::TYPE, 0x2C);
    let scene = b.object(0, "igSceneInfo", 0x48);
    b.string(at(world, 0x08), name);
    b.put_bytes(at(world, 0x10), &[1]);
    b.put_u32(at(world, 0x1C), 0x5EED);
    b.pointer(at(world, 0x20), scene);
    b.put_u32(at(world, 0x28), 0x1234);
    world
}

/// A level's IGZ: one `tfbPhysicsWorld` per `(name, _data)`, each with a
/// scene of its own; `None` leaves `_data` null.
pub fn level(worlds: &[(&str, Option<&[u8]>)]) -> Vec<u8> {
    let mut b = IgzBuilder::new();
    let objects: Vec<Location> = worlds
        .iter()
        .map(|(name, _)| world_object(&mut b, name))
        .collect();
    let blocks = b.section(0x80);
    for (&world, (_, data)) in objects.iter().zip(worlds) {
        match data {
            Some(bytes) => {
                b.memory_ref(at(world, 0x14), blocks, bytes, 2);
            }
            None => b.null_memory_ref(at(world, 0x14), 0),
        }
    }
    b.build()
}

/// `worlds` worlds all pointing at one packfile, as a mutated file can:
/// the walk's budget, not the input's size, has to bound the work.
pub fn shared(worlds: usize, packfile: &[u8]) -> Vec<u8> {
    let mut b = IgzBuilder::new();
    let objects: Vec<Location> = (0..worlds)
        .map(|i| world_object(&mut b, &format!("w{i}")))
        .collect();
    let blocks = b.section(0x80);
    let first = b.memory_ref(at(objects[0], 0x14), blocks, packfile, 2);
    for &world in &objects[1..] {
        b.put_u32(
            at(world, 0x14),
            crate::igz::MEMORY_PRESENT | (2 << 27) | packfile.len() as u32,
        );
        b.pointer(at(world, 0x18), first);
    }
    b.build()
}

/// Seeds for the property test and the fuzz corpus: level 213's shape (a
/// world with a body and a grass world without one), a lone grid with local
/// pointers, and a world of nothing but an empty system.
pub fn seeds() -> Vec<Vec<u8>> {
    let ramp = world_packfile(&world_spec(vec![body_spec(ramp())])).bytes;
    let grass = world_packfile(&world_spec(vec![])).bytes;
    let mut local = world_spec(vec![body_spec(grid(4))]);
    local.global = false;
    let grid = world_packfile(&local).bytes;
    vec![
        level(&[
            ("Levels/Synthetic/Synthetic_ART_MASTER.ma", Some(&ramp)),
            ("Levels/Synthetic/Synthetic_GRASS.ma", Some(&grass)),
        ]),
        level(&[("Levels/Synthetic/Grid.ma", Some(&grid))]),
        level(&[("Levels/Synthetic/Empty.ma", Some(&grass))]),
    ]
}
