use super::builder::{
    self, BodySpec, Built, MeshSpec, WorldSpec, body_spec, grid, level, ramp, world_packfile,
    world_spec,
};
use super::*;
use crate::havok::builder::{PayloadSpec, payload};
use crate::havok::physics::{PhysicsSystem, SimpleMeshShape, TriangleCollisionQuery};
use crate::havok::{HEADER_LEN, SECTION_HEADER_LEN};
use crate::igz::Ref;
use crate::igz::builder::IgzBuilder;
use crate::igz::layouts::{ObjectRef, TAG, WorldInfo, type_def};

fn one_body(body: BodySpec) -> Built {
    world_packfile(&world_spec(vec![body]))
}

/// The refusal's field and kind.
fn refused(bytes: &[u8]) -> (&'static str, ErrorKind) {
    let e = Collision::parse(bytes).unwrap_err();
    (e.field, e.kind)
}

fn wrong_type(expected: &'static str, found: &str) -> ErrorKind {
    ErrorKind::WrongType {
        expected,
        found: found.to_owned(),
    }
}

#[test]
fn every_seed_reads_every_world() {
    for (i, seed) in builder::seeds().iter().enumerate() {
        let igz = Igz::parse(seed).unwrap();
        let read: Vec<_> = worlds(&igz).collect();
        assert!(!read.is_empty(), "seed {i}");
        for (_, w) in read {
            w.unwrap_or_else(|e| panic!("seed {i}: {e}"));
        }
    }
}

#[test]
fn a_mesh_reads_back_vertex_for_vertex() {
    let spec = body_spec(ramp());
    let c = Collision::parse(&one_body(spec.clone()).bytes).unwrap();
    let b = c.body.unwrap();
    assert_eq!(b.translation, [-425.5, -37.625, -16.0]);
    assert_eq!(
        b.mesh.vertices,
        [
            [0.0, 0.0, 0.0],
            [2.0, 0.0, 0.0],
            [2.0, 1.0, 2.0],
            [0.0, 1.0, 2.0]
        ]
    );
    assert_eq!(
        b.mesh.triangles,
        [
            Triangle {
                indices: [0, 2, 1],
                welding: 0x0101
            },
            Triangle {
                indices: [0, 3, 2],
                welding: 0x0202
            }
        ]
    );
    assert_eq!(b.mesh.materials, [0, 40]);
    assert_eq!(b.mesh.material_counts(), [(0, 1), (40, 1)].into());
    assert_eq!((b.mesh.radius, b.mesh.welding_type), (0.05, 4));
    assert_eq!(b.mesh.bounds(), Some(([0.0, 0.0, 0.0], [2.0, 1.0, 2.0])));
    assert_eq!(
        b.mopp,
        Mopp {
            offset: [-1.0, -1.0, -1.0],
            scale: 16_777_216.0 / 3.0,
            code_len: 24
        }
    );
    // The level's frame: each vertex plus the translation.
    let world: Vec<[f32; 3]> = b.world_vertices().collect();
    assert_eq!(world[0], [-425.5, -37.625, -16.0]);
    assert_eq!(world[2], [-423.5, -36.625, -14.0]);
}

#[test]
fn a_bigger_mesh_and_local_pointers_read_the_same() {
    let mut spec = world_spec(vec![body_spec(grid(3))]);
    let global = Collision::parse(&world_packfile(&spec).bytes).unwrap();
    spec.global = false;
    let local = Collision::parse(&world_packfile(&spec).bytes).unwrap();
    assert_eq!(global, local);
    let mesh = &local.body.unwrap().mesh;
    assert_eq!((mesh.vertices.len(), mesh.triangles.len()), (16, 18));
    assert_eq!(mesh.vertices[5], [1.0, 0.0, 1.0]);
    assert_eq!(mesh.materials, (0..18).collect::<Vec<u8>>());
}

#[test]
fn an_empty_system_has_no_body() {
    let built = world_packfile(&world_spec(vec![]));
    assert_eq!(Collision::parse(&built.bytes).unwrap().body, None);
}

#[test]
fn worlds_read_through_the_level() {
    let ramp = one_body(body_spec(ramp())).bytes;
    let grass = world_packfile(&world_spec(vec![])).bytes;
    let bytes = level(&[
        ("Level_ART_MASTER.ma", Some(&ramp)),
        ("Level_GRASS.ma", Some(&grass)),
    ]);
    let igz = Igz::parse(&bytes).unwrap();
    let read: Vec<(usize, World<'_>)> = worlds(&igz).map(|(i, w)| (i, w.unwrap())).collect();
    // Each world is followed by its scene.
    assert_eq!(read.iter().map(|(i, _)| *i).collect::<Vec<_>>(), [0, 2]);
    let (_, art) = &read[0];
    assert_eq!(art.name(), Some("Level_ART_MASTER.ma"));
    assert_eq!(art.layout.id_code, 0x1234);
    let info = &art.layout.world.data_info;
    assert!(info.info.resolve_state);
    assert_eq!(info.stream_archive_hash, 0x5EED);
    assert_eq!(info.data.unwrap().data, &ramp[..]);
    assert_eq!(art.layout.world.scene_info, ObjectRef::Object(1));
    assert_eq!(art.layout.world.global_level_texture_info, ObjectRef::Null);
    assert_eq!(art.collision.body.as_ref().unwrap().mesh.triangles.len(), 2);
    let (_, grass) = &read[1];
    assert_eq!(grass.name(), Some("Level_GRASS.ma"));
    assert_eq!(grass.collision.body, None);
}

#[test]
fn the_world_layouts_are_the_registrys() {
    assert_eq!(TAG, "IGZ v6 / BLUS30968 01.00");
    let world = type_def(PhysicsWorld::TYPE).unwrap();
    assert_eq!((world.parent, world.size), (Some(WorldInfo::TYPE), 0x2C));
    let info = type_def(WorldInfo::TYPE).unwrap();
    assert_eq!((info.parent, info.size), (Some("tfbBuildDataInfo"), 0x28));
    for wrong in [0x28, 0x30] {
        let mut b = IgzBuilder::new();
        let o = b.object(0, PhysicsWorld::TYPE, wrong);
        b.null_memory_ref(o.offset_by(0x14).unwrap(), 0);
        let bytes = b.build();
        let e = PhysicsWorld::read(&Igz::parse(&bytes).unwrap(), 0).unwrap_err();
        assert_eq!(
            (e.field, e.kind),
            (
                "MTSZ",
                ErrorKind::Unsupported {
                    value: wrong.into()
                }
            )
        );
    }
}

#[test]
fn a_fixup_on_the_id_code_is_refused() {
    let mut b = IgzBuilder::new();
    let o = b.object(0, PhysicsWorld::TYPE, 0x2C);
    b.null_memory_ref(o.offset_by(0x14).unwrap(), 0);
    b.pointer(o.offset_by(0x28).unwrap(), o);
    let bytes = b.build();
    let igz = Igz::parse(&bytes).unwrap();
    assert!(matches!(
        igz.reference(o.offset_by(0x28).unwrap(), "t"),
        Ok(Ref::Object(0))
    ));
    let e = PhysicsWorld::read(&igz, 0).unwrap_err();
    assert_eq!(e.kind, ErrorKind::Unsupported { value: 0x28 });
}

#[test]
fn a_world_without_a_packfile_is_refused() {
    let bytes = level(&[("none", None)]);
    let igz = Igz::parse(&bytes).unwrap();
    let e = world(&igz, 0).unwrap_err();
    assert_eq!((e.field, e.kind), ("_data", ErrorKind::Missing));

    let lip_sync = [0xDE, 0xAD, 0xBE, 0xEF, 1, 2, 3, 4, 5];
    let bytes = level(&[("voice", Some(&lip_sync))]);
    let igz = Igz::parse(&bytes).unwrap();
    let e = world(&igz, 0).unwrap_err();
    assert_eq!(e.field, "_data");
    assert!(matches!(e.kind, ErrorKind::BadMagic { found, .. } if found == lip_sync[..8]));
}

#[test]
fn another_shape_class_is_refused() {
    let mut spec = body_spec(ramp());
    spec.shape = ("hkpBoxShape", 0x1234_5678);
    assert_eq!(
        refused(&one_body(spec).bytes),
        (
            "shape",
            wrong_type(TriangleCollisionQuery::CLASS, "hkpBoxShape")
        )
    );
    let mut spec = body_spec(ramp());
    spec.child = ("hkpExtendedMeshShape", 0x1234_5678);
    assert_eq!(
        refused(&one_body(spec).bytes),
        (
            "childShape",
            wrong_type(SimpleMeshShape::CLASS, "hkpExtendedMeshShape")
        )
    );
}

#[test]
fn a_second_system_or_none_is_refused() {
    let spec = WorldSpec {
        systems: vec![vec![body_spec(ramp())], vec![]],
        global: true,
    };
    let two = ErrorKind::CountMismatch {
        found: 2,
        expected: 1,
    };
    assert_eq!(refused(&world_packfile(&spec).bytes), ("systems", two));
    let none = WorldSpec {
        systems: vec![],
        global: true,
    };
    let zero = ErrorKind::CountMismatch {
        found: 0,
        expected: 1,
    };
    assert_eq!(refused(&world_packfile(&none).bytes), ("systems", zero));
}

#[test]
fn a_second_body_is_refused() {
    let spec = world_spec(vec![body_spec(ramp()), body_spec(grid(1))]);
    let kind = ErrorKind::CountMismatch {
        found: 2,
        expected: 1,
    };
    assert_eq!(refused(&world_packfile(&spec).bytes), ("rigidBodies", kind));
}

#[test]
fn a_rotated_body_is_refused() {
    let mut spec = body_spec(ramp());
    // A quarter turn about y.
    spec.rotation = [
        [0.0, 0.0, -1.0, 0.0],
        [0.0, 1.0, 0.0, 0.0],
        [1.0, 0.0, 0.0, 0.0],
    ];
    assert_eq!(
        refused(&one_body(spec).bytes),
        ("rotation", ErrorKind::Unsupported { value: 0 })
    );
    // Off by one ulp is still a rotation the disc never shows.
    let mut spec = body_spec(ramp());
    spec.rotation[1][1] = f32::from_bits(1.0f32.to_bits() - 1);
    let bits = u64::from(spec.rotation[1][1].to_bits());
    assert_eq!(
        refused(&one_body(spec).bytes),
        ("rotation", ErrorKind::Unsupported { value: bits })
    );
}

#[test]
fn a_body_that_isnt_fixed_is_refused() {
    let mut spec = body_spec(ramp());
    // MOTION_KEYFRAMED.
    spec.motion_type = 4;
    assert_eq!(
        refused(&one_body(spec).bytes),
        ("motion type", ErrorKind::Unsupported { value: 4 })
    );
}

#[test]
fn an_index_past_the_vertices_is_refused() {
    let mut built = one_body(body_spec(ramp()));
    let indexes = built.index_arrays[0].unwrap();
    // The second triangle's c.
    built.put_u16(indexes + 12, 4);
    let kind = ErrorKind::OutOfRange { value: 4, limit: 4 };
    assert_eq!(refused(&built.bytes), ("indexArray", kind));
    // The last vertex is fine.
    let mut built = one_body(body_spec(ramp()));
    built.put_u16(indexes + 12, 3);
    Collision::parse(&built.bytes).unwrap();
}

#[test]
fn counts_that_dont_divide_or_agree_are_refused() {
    let mut mesh = ramp();
    mesh.materials.pop();
    let kind = ErrorKind::CountMismatch {
        found: 1,
        expected: 2,
    };
    assert_eq!(
        refused(&one_body(body_spec(mesh)).bytes),
        ("tfbProperties", kind)
    );

    let mut mesh = ramp();
    mesh.floats.pop();
    assert_eq!(
        refused(&one_body(body_spec(mesh)).bytes),
        ("vertexArray", ErrorKind::Unsupported { value: 11 })
    );

    let mut mesh = ramp();
    mesh.words.pop();
    assert_eq!(
        refused(&one_body(body_spec(mesh)).bytes),
        ("indexArray", ErrorKind::Unsupported { value: 7 })
    );
}

#[test]
fn a_non_finite_translation_radius_or_mopp_info_is_refused() {
    let mut spec = body_spec(ramp());
    spec.translation[1] = f32::INFINITY;
    let bits = u64::from(f32::INFINITY.to_bits());
    assert_eq!(
        refused(&one_body(spec).bytes),
        ("translation", ErrorKind::Unsupported { value: bits })
    );
    let mut spec = body_spec(ramp());
    spec.mopp_info[3] = f32::INFINITY;
    assert_eq!(
        refused(&one_body(spec).bytes),
        ("info", ErrorKind::Unsupported { value: bits })
    );
    let mut built = one_body(body_spec(ramp()));
    let radius = built.meshes[0] + SimpleMeshShape::RADIUS as u32;
    built.put_u32(radius, f32::INFINITY.to_bits());
    assert_eq!(
        refused(&built.bytes),
        ("radius", ErrorKind::Unsupported { value: bits })
    );
}

#[test]
fn an_array_that_must_be_empty_is_refused_when_it_declares_elements() {
    let base = one_body(body_spec(ramp()));
    let (system, query, mesh) = (base.systems[0], base.queries[0], base.meshes[0]);
    type S = PhysicsSystem;
    type M = SimpleMeshShape;
    let mut arrays = vec![
        (system, S::CONSTRAINTS, "constraints"),
        (system, S::ACTIONS, "actions"),
        (system, S::PHANTOMS, "phantoms"),
        (mesh, M::VERTICES, "vertices"),
        (mesh, M::TRIANGLES, "triangles"),
        (mesh, M::MATERIAL_INDICES, "materialIndices"),
    ];
    arrays.extend(TriangleCollisionQuery::OWN_ARRAYS.map(|(offset, field)| (query, offset, field)));
    for (object, offset, field) in arrays {
        // One element and the don't-deallocate flag, no data behind it.
        let mut built = base.clone();
        let size = object + offset as u32 + 4;
        built.put_u32(size, 1);
        built.put_u32(size + 4, 0x8000_0001);
        assert_eq!(
            refused(&built.bytes),
            (field, ErrorKind::Missing),
            "{field}"
        );
    }
}

#[test]
fn a_non_finite_vertex_is_refused() {
    let mut mesh = ramp();
    mesh.floats[4] = f32::NAN;
    let (field, kind) = refused(&one_body(body_spec(mesh)).bytes);
    assert_eq!(field, "vertexArray");
    assert!(matches!(kind, ErrorKind::Unsupported { .. }));
}

#[test]
fn an_empty_mesh_reads() {
    let c = Collision::parse(&one_body(body_spec(MeshSpec::new(&[], &[], &[]))).bytes).unwrap();
    let mesh = c.body.unwrap().mesh;
    assert!(mesh.vertices.is_empty() && mesh.triangles.is_empty());
    assert_eq!(mesh.bounds(), None);
}

#[test]
fn a_wrong_signature_is_refused() {
    let mut built = one_body(body_spec(ramp()));
    let name = b"tfbSimpleMeshShape\0";
    let at = built
        .bytes
        .windows(name.len())
        .position(|w| w == name)
        .unwrap();
    // The signature is the word before the 0x09 separator.
    built.bytes[at - 5..at - 1].copy_from_slice(&0x0BAD_F00Du32.to_be_bytes());
    assert_eq!(
        refused(&built.bytes),
        (
            "class signature",
            ErrorKind::Unsupported { value: 0x0BAD_F00D }
        )
    );
}

/// Where the data section's virtual fixups start and end, absolute.
fn virtual_table(bytes: &[u8]) -> (usize, usize) {
    let n = u32::from_be_bytes(bytes[0x14..0x18].try_into().unwrap()) as usize;
    let h = HEADER_LEN + SECTION_HEADER_LEN * (n - 1);
    let w =
        |k: usize| u32::from_be_bytes(bytes[h + 20 + 4 * k..h + 24 + 4 * k].try_into().unwrap());
    let start = w(0) as usize;
    (start + w(3) as usize, start + w(4) as usize)
}

#[test]
fn an_object_inside_a_registered_size_is_refused() {
    let mut built = one_body(body_spec(ramp()));
    let query = built.queries[0];
    let code = built.mopp_codes[0];
    // Move the MOPP code's class marker 0x40 into the query: another
    // object now starts inside the query's 0x70 bytes.
    let (from, to) = virtual_table(&built.bytes);
    let entry = (from..to)
        .step_by(12)
        .find(|&e| built.bytes[e..e + 4] == code.to_be_bytes())
        .unwrap();
    built.bytes[entry..entry + 4].copy_from_slice(&(query + 0x40).to_be_bytes());
    assert_eq!(
        refused(&built.bytes),
        ("registered size", ErrorKind::Unsupported { value: 0x40 })
    );
}

#[test]
fn an_object_inside_the_meshs_registered_size_is_refused() {
    let mut built = one_body(body_spec(ramp()));
    let mesh = built.meshes[0];
    // Move the collision filter's class marker 0x40 into the mesh shape:
    // nothing follows the filter, so only the mesh's 0x68 bytes notice.
    let word = |b: &[u8], at: usize| u32::from_be_bytes(b[at..at + 4].try_into().unwrap());
    let name = b"hkpConstraintCollisionFilter\0";
    let at = built
        .bytes
        .windows(name.len())
        .position(|w| w == name)
        .unwrap();
    // The first section's absoluteDataStart: `__classnames__`.
    let names = word(&built.bytes, HEADER_LEN + 20) as usize;
    let (from, to) = virtual_table(&built.bytes);
    let entry = (from..to)
        .step_by(12)
        .find(|&e| word(&built.bytes, e + 8) as usize == at - names)
        .unwrap();
    built.bytes[entry..entry + 4].copy_from_slice(&(mesh + 0x40).to_be_bytes());
    assert_eq!(
        refused(&built.bytes),
        ("registered size", ErrorKind::Unsupported { value: 0x40 })
    );
}

#[test]
fn a_packfile_without_physics_data_is_refused() {
    let animation = payload(&PayloadSpec::default()).bytes;
    assert_eq!(refused(&animation), ("namedVariants", ErrorKind::Missing));
}
