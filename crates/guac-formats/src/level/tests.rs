use super::builder::{
    self, GROUP, QUARTER_TURN, SIMPLE_USER_INFO, SPATIAL_NODE, Sample, TRANSFORM, geometry, group,
    placement, sample, scene_over, technique,
};
use super::*;
use crate::edge::builder::{segment, unskinned};
use crate::igz::Location;
use crate::igz::builder::IgzBuilder;
use crate::igz::layouts::builder::list;
use crate::igz::layouts::{ObjectRef, Shape, TAG, type_def};

fn index(igz: &Igz<'_>, at: Location) -> usize {
    igz.object_at(at).expect("an object there").0
}

fn at(base: Location, offset: u32) -> Location {
    base.offset_by(offset).unwrap()
}

fn sampled() -> (Sample, Vec<u8>) {
    let s = sample();
    let bytes = s.bytes.clone();
    (s, bytes)
}

/// The refusal's field and kind.
fn refused<T: std::fmt::Debug>(r: Result<T, FormatError>) -> (&'static str, ErrorKind) {
    let e = r.unwrap_err();
    (e.field, e.kind)
}

fn wrong_type(expected: &'static str, found: &str) -> ErrorKind {
    ErrorKind::WrongType {
        expected,
        found: found.to_owned(),
    }
}

#[test]
fn every_seed_reads_every_scene_and_placement() {
    for (i, seed) in builder::seeds().iter().enumerate() {
        let igz = Igz::parse(seed).unwrap();
        let read: Vec<_> = scenes(&igz).collect();
        assert!(!read.is_empty(), "seed {i}");
        for (_, s) in read {
            s.unwrap_or_else(|e| panic!("seed {i}: {e}"));
        }
        Placements::read(&igz).unwrap_or_else(|e| panic!("seed {i}: {e}"));
        directional_lights(&igz).unwrap_or_else(|e| panic!("seed {i}: {e}"));
    }
}

#[test]
fn mul_composes_row_vectors_left_to_right() {
    let p = [1.0, 0.0, 0.0];
    let m = mul(&QUARTER_TURN, &builder::TRANSLATION);
    assert_eq!(
        transform_point(p, &m),
        transform_point(transform_point(p, &QUARTER_TURN), &builder::TRANSLATION)
    );
    assert_eq!(transform_point(p, &m), [10.0, 20.0, 29.0]);
    assert_eq!(mul(&IDENTITY, &m), m);
    assert_eq!(mul(&m, &IDENTITY), m);
}

#[test]
fn a_scenes_segments_sit_under_their_nodes() {
    let (s, bytes) = sampled();
    let igz = Igz::parse(&bytes).unwrap();
    let i = |at| index(&igz, at);
    let all: Vec<_> = scenes(&igz).collect();
    assert_eq!(all.len(), 1);
    let scene = all.into_iter().next().unwrap().1.unwrap();
    assert_eq!(scene.index, i(s.scene));

    let info = &scene.info;
    assert_eq!(info.info.name, Some("igSceneInfo0"));
    assert!(info.info.resolve_state);
    assert_eq!(info.scene_graph, Some(i(s.root)));
    assert_eq!(info.up_vector, [0.0, 1.0, 0.0]);
    assert_eq!(info.animation_begin, 0x1_0000_0002);
    assert_eq!(info.animation_end, -2);
    assert_eq!(info.cameras, ObjectRef::Null);

    // The groups, their parents, children and frames.
    assert_eq!(scene.root, i(s.root));
    let keys: Vec<usize> = scene.groups.keys().copied().collect();
    let mut expected = vec![i(s.root), i(s.spatial), i(s.sky), i(s.outer), i(s.inner)];
    expected.sort();
    assert_eq!(keys, expected);
    let g = |at| &scene.groups[&i(at)];
    assert!(matches!(g(s.root).kind, GroupKind::Group(_)));
    assert_eq!(g(s.root).kind.group().node.name, Some("sceneGraph"));
    assert_eq!(g(s.root).parent, None);
    assert_eq!(g(s.root).children, [i(s.spatial), i(s.outer)]);
    assert!(matches!(g(s.spatial).kind, GroupKind::Spatial(_)));
    assert_eq!(g(s.spatial).parent, Some(i(s.root)));
    assert_eq!(g(s.spatial).children, [i(s.techniques[0]), i(s.sky)]);
    assert!(matches!(g(s.sky).kind, GroupKind::UserInfo(_)));
    assert_eq!(g(s.sky).parent, Some(i(s.spatial)));
    assert_eq!(g(s.sky).user_info, Some(i(s.sky)));
    assert_eq!(g(s.spatial).user_info, None);
    let GroupKind::Transform(outer) = g(s.outer).kind else {
        panic!("outer isn't a transform");
    };
    let GroupKind::Transform(inner) = g(s.inner).kind else {
        panic!("inner isn't a transform");
    };
    assert_eq!(outer.m, builder::TRANSLATION);
    assert_eq!(outer.target, 4);
    assert!(!outer.is_animated());
    assert_eq!(inner.m, QUARTER_TURN);
    assert_eq!(inner.target, -1);
    assert_eq!(inner.transform_input, ObjectRef::Null);
    assert_eq!(inner.transform_input3, ObjectRef::Object(i(s.keyframes)));
    assert!(inner.is_animated());
    assert_eq!(g(s.inner).parent, Some(i(s.outer)));
    assert_eq!(g(s.outer).world, builder::TRANSLATION);
    assert_eq!(g(s.inner).world, mul(&QUARTER_TURN, &builder::TRANSLATION));
    assert_eq!(scene.transforms().count(), 2);

    // The draws, depth first, each under its node in its node's frame.
    let parents: Vec<usize> = scene.draws.iter().map(|d| d.parent).collect();
    assert_eq!(parents, [i(s.spatial), i(s.sky), i(s.inner)]);
    let techniques: Vec<usize> = scene.draws.iter().map(|d| d.technique_index).collect();
    assert_eq!(techniques, s.techniques.map(i));
    let names: Vec<_> = scene.draws.iter().map(|d| d.technique.node.name).collect();
    assert_eq!(
        names,
        [Some("Ground"), Some("Sky_morning"), Some("Windmill_Blades")]
    );
    let user_infos: Vec<_> = scene.draws.iter().map(|d| d.user_info).collect();
    assert_eq!(user_infos, [None, Some(i(s.sky)), None]);
    for (k, d) in scene.draws.iter().enumerate() {
        assert_eq!(d.attr_index, i(s.attrs[k]));
        assert_eq!(d.geometry, i(s.geometries[k]));
    }
    assert_eq!(scene.draws[0].world, IDENTITY);
    assert_eq!(scene.draws[1].world, IDENTITY);
    assert_eq!(scene.draws[2].world, g(s.inner).world);
    let segments =
        |d: &Draw<'_>| -> Vec<usize> { scene.draw_segments(d).map(|(k, _)| k).collect() };
    assert_eq!(
        segments(&scene.draws[0]),
        [i(s.segments[0]), i(s.segments[1])]
    );
    assert_eq!(segments(&scene.draws[1]), [i(s.segments[2])]);
    assert_eq!(segments(&scene.draws[2]), [i(s.segments[3])]);
    assert_eq!(scene.segments.len(), 4);

    // The windmill's vertices, decoded and placed: turned, then moved.
    let seg = crate::edge::Segment::read(&igz, i(s.segments[3])).unwrap();
    let placed: Vec<[f32; 3]> = seg
        .positions()
        .take(2)
        .map(|p| transform_point(p, &scene.draws[2].world))
        .collect();
    assert_eq!(
        &unskinned().expect.positions[..2],
        [[0.0, 1.0, 0.0], [0.5, 0.0, 0.25]]
    );
    assert_eq!(placed, [[10.0, 21.0, 30.0], [10.25, 20.0, 29.5]]);
}

#[test]
fn a_placements_transform_scale_shape_and_bits_read_field_for_field() {
    let (s, bytes) = sampled();
    let igz = Igz::parse(&bytes).unwrap();
    let i = |at| index(&igz, at);
    let p = Placements::read(&igz).unwrap();
    assert_eq!(p.actors.len(), 2);
    let a = p.actor("213_Init_Location").unwrap();
    assert_eq!(a.index, i(s.actor));
    let abstract_ = &a.placement().placement;
    assert_eq!(abstract_.id, 7);
    assert_eq!(abstract_.init_matrix.position, [10.191, 43.379, -93.394]);
    assert_eq!(abstract_.init_matrix.rotation, [7.0, 310.0, -50.0]);
    assert_eq!(abstract_.current_matrix.position, [0.0; 3]);
    assert_eq!(abstract_.activity_state, 4);
    assert_eq!(abstract_.move_index, -1);
    assert_eq!(abstract_.active_time_layer, 3);
    let placed = a.placement();
    assert_eq!(placed.properties, 0x800);
    assert_eq!(placed.major_index, 2);
    assert_eq!(placed.script, Some(i(s.script)));
    assert_eq!(placed.active_time, 0.5);
    assert_eq!(placed.scale, [150.0, 125.0, 100.0]);
    assert_eq!(placed.collision, Some(i(s.collisions[0])));
    assert_eq!(placed.tint, [0x80, 0x80, 0x80, 0xFF]);
    assert_eq!(placed.replace_color, [0x80, 0x80, 0x80, 0x00]);
    assert_eq!(placed.activation_range, 160.0);
    assert_eq!(placed.activation_range_buffer, 8.0);
    assert_eq!(a.info.model, Some(i(s.model)));
    assert_eq!(a.info.actor_parameters, Some(i(s.parameters)));
    assert_eq!(a.info.waypoints, Some(i(s.waypoint_list)));
    assert_eq!(a.info.destination, [1.0, 2.0, 3.0]);
    assert_eq!(a.info.controller, ObjectRef::Null);
    assert_eq!(
        a.script,
        Some("Levels/Synthetic/Scripts/InitializeLevel.ai")
    );
    assert_eq!(p.model_path(a), Some("Models/Objects/Door_Breakable.mdl"));

    // A solid sphere that interacts with the world, lifted and turned.
    let c = a.collision.unwrap();
    assert_eq!(c.properties, 0x1B);
    assert_eq!(c.shape(), Shape::Sphere);
    assert!(c.is_solid() && c.interacts_with_world() && !c.is_actor_surface());
    assert_eq!(c.transform.position, [0.0, 1.5, 0.0]);
    assert_eq!(c.transform.rotation, [0.0, 90.0, 0.0]);
    assert_eq!(
        (c.width, c.height, c.length, c.weight, c.rebound),
        (4.0, 6.0, 3.0, 10.0, 25.0)
    );

    // Its movement numbers.
    let m = a.parameters.unwrap();
    let q = &m.parameters;
    assert_eq!(
        [
            q.ground_max_speed,
            q.ground_acceleration,
            q.ground_deceleration,
            q.ground_turn_rate,
            q.air_max_speed,
            q.air_acceleration,
            q.air_deceleration,
            q.air_turn_rate,
            q.gravity_max_speed,
            q.gravity_acceleration,
            q.jump_max_speed,
            q.jump_acceleration,
            q.jump_initial_speed,
        ],
        [
            18.0, 160.0, 80.0, 360.0, 20.0, 150.0, 70.0, 1440.0, 120.0, 128.0, 300.0, 4.8, 32.0
        ]
    );
    assert_eq!(q.motion_type, 2);
    assert!(q.tilts_with_ground);
    assert_eq!(m.object_time_scale, 1.25);

    // The camera volume: a box that isn't solid, the same model, nothing else.
    let v = p.actor("Camera_Transition_36-37").unwrap();
    assert_eq!(v.index, i(s.volume));
    assert_eq!(v.placement().placement.activity_state, 5);
    assert_eq!(
        v.placement().placement.init_matrix.rotation,
        [7.0, 168.0, 0.0]
    );
    let c = v.collision.unwrap();
    assert_eq!(c.shape(), Shape::Box);
    assert!(!c.is_solid() && !c.interacts_with_world());
    assert_eq!((c.width, c.height, c.length), (60.0, 21.0, 100.0));
    assert_eq!((v.script, v.parameters), (None, None));
    assert_eq!(v.info.model, a.info.model);
    assert_eq!(p.models.len(), 1);
    assert!(p.waypoints(v).is_empty());
}

#[test]
fn a_waypoint_list_reads_in_order() {
    let (s, bytes) = sampled();
    let igz = Igz::parse(&bytes).unwrap();
    let p = Placements::read(&igz).unwrap();
    let a = p.actor("213_Init_Location").unwrap();
    let points = p.waypoints(a);
    let names: Vec<_> = points.iter().map(|w| w.name).collect();
    assert_eq!(names, [Some("wp0"), Some("wp1"), Some("wp2")]);
    let heights: Vec<f32> = points.iter().map(|w| w.height).collect();
    assert_eq!(heights, [0.0, 2.5, -1.0]);
    let positions: Vec<[f32; 3]> = points.iter().map(|w| w.init_matrix.position).collect();
    assert_eq!(
        positions,
        [
            [10.191, 43.379, -93.394],
            [12.0, 43.0, -90.0],
            [14.0, 44.0, -88.0]
        ]
    );
    assert_eq!(points[1].init_matrix.rotation, [0.0, 45.0, 0.0]);
    assert_eq!(
        p.waypoint_lists.keys().copied().collect::<Vec<_>>(),
        [index(&igz, s.waypoint_list)]
    );
}

#[test]
fn a_light_reads_field_for_field() {
    let (s, bytes) = sampled();
    let igz = Igz::parse(&bytes).unwrap();
    let p = Placements::read(&igz).unwrap();
    let [(li, light)] = p.lights[..] else {
        panic!("{} lights", p.lights.len());
    };
    assert_eq!(li, index(&igz, s.light));
    assert_eq!(
        light.placement.placement.name,
        Some("213_EvilRobot_Beam_light")
    );
    assert_eq!(light.light_type, 1);
    assert!(light.visible);
    assert_eq!(
        [
            light.falloff_rate,
            light.cutoff_angle,
            light.constant_attenuation,
            light.linear_attenuation,
            light.quadratic_attenuation,
            light.multiplier,
        ],
        [0.25, 30.0, 1.0, 0.5, 0.125, 1.5]
    );
    assert_eq!(light.diffuse, [0x55, 0x2A, 0x11, 0xFF]);
    assert_eq!(light.ambient, [0, 0, 0, 0xFF]);
    assert_eq!(light.specular, [0x2C, 0x1F, 0x13, 0xFF]);
    assert_eq!(light.light_attr, ObjectRef::Null);

    // The emitter beside it, and the level's rig.
    let [(ei, emitter)] = p.emitters[..] else {
        panic!("{} emitters", p.emitters.len());
    };
    assert_eq!(ei, index(&igz, s.emitter));
    assert_eq!(
        emitter.placement.placement.init_matrix.position,
        [111.728, 19.05, 78.483]
    );
    assert!(matches!(
        emitter.generator_info_list,
        ObjectRef::Named { .. }
    ));
    let [(ri, rig)] = directional_lights(&igz).unwrap()[..] else {
        panic!("not one rig light");
    };
    assert_eq!(ri, index(&igz, s.rig));
    assert_eq!(rig.name, Some("_worldLight1"));
    assert_eq!(rig.diffuse, [200, 190, 170, 255]);
    assert_eq!(rig.ambient, [40, 45, 60, 255]);
    assert_eq!(rig.multiplier, 1.0);
    assert_eq!(rig.init_matrix.rotation, [-45.0, 30.0, 0.0]);
}

/// Every layout AG.3 adds: its registry parent and size, the memory refs a
/// zeroed one needs marked null, and its reader.
type Reader = fn(&Igz<'_>, usize) -> Result<(), FormatError>;
const LAYOUTS: [(&str, &str, u32, &[u32], Reader); 14] = [
    ("igSceneInfo", "igInfo", 0x48, &[], |i, o| {
        SceneInfo::read(i, o).map(drop)
    }),
    ("igSpatialNode", "igGroup", 0x28, &[], |i, o| {
        SpatialNode::read(i, o).map(drop)
    }),
    ("igTransform", "igGroup", 0x80, &[], |i, o| {
        Transform::read(i, o).map(drop)
    }),
    ("igUserInfo", "igGroup", 0x28, &[], |i, o| {
        UserInfo::read(i, o).map(drop)
    }),
    ("igSimpleUserInfo", "igUserInfo", 0x28, &[], |i, o| {
        UserInfo::read(i, o).map(drop)
    }),
    ("Placement", "AbstractPlacement", 0xD8, &[], |i, o| {
        Placement::read(i, o).map(drop)
    }),
    ("tfbActorInfo", "Placement", 0xF8, &[], |i, o| {
        ActorInfo::read(i, o).map(drop)
    }),
    ("tfbParticleInfo", "Placement", 0xE0, &[], |i, o| {
        ParticleInfo::read(i, o).map(drop)
    }),
    ("tfbLightInfo", "Placement", 0x10C, &[], |i, o| {
        LightInfo::read(i, o).map(drop)
    }),
    ("CollisionInfo", "MatrixMeasurement", 0x58, &[], |i, o| {
        CollisionInfo::read(i, o).map(drop)
    }),
    ("ModelParameters", "ModelPhysics", 0x48, &[], |i, o| {
        crate::igz::layouts::ModelParameters::read(i, o).map(drop)
    }),
    ("ActorParameters", "ModelParameters", 0x4C, &[], |i, o| {
        ActorParameters::read(i, o).map(drop)
    }),
    ("ActorWaypoint", "AbstractAttachNode", 0x38, &[], |i, o| {
        ActorWaypoint::read(i, o).map(drop)
    }),
    (
        "ActorWaypointList",
        "ScriptObjectList",
        0x18,
        &[0x10],
        |i, o| ActorWaypointList::read(i, o).map(drop),
    ),
];

#[test]
fn the_level_layouts_are_the_registrys() {
    assert_eq!(TAG, "IGZ v6 / BLUS30968 01.00");
    let physics = type_def("ModelPhysics").unwrap();
    assert_eq!((physics.parent, physics.size), (Some("igObject"), 0xC));
    for (ty, parent, size, nulls, read) in LAYOUTS {
        let t = type_def(ty).unwrap_or_else(|| panic!("{ty} has no TypeDef"));
        assert_eq!((t.parent, t.size), (Some(parent), size), "{ty}");
        let zeroed = |mtsz| {
            let mut b = IgzBuilder::new();
            let o = b.object(0, ty, mtsz);
            // The MTSZ check comes first, so a short object skips them.
            for &offset in nulls.iter().filter(|&&n| n + 8 <= mtsz) {
                b.null_memory_ref(at(o, offset), 0);
            }
            b.build()
        };
        let bytes = zeroed(size);
        read(&Igz::parse(&bytes).unwrap(), 0).unwrap_or_else(|e| panic!("{ty}: {e}"));
        for wrong in [size - 4, size + 4] {
            let bytes = zeroed(wrong);
            let e = read(&Igz::parse(&bytes).unwrap(), 0).unwrap_err();
            assert_eq!(
                (e.field, e.kind),
                (
                    "MTSZ",
                    ErrorKind::Unsupported {
                        value: wrong.into()
                    }
                ),
                "{ty} at {wrong:#x}"
            );
        }
    }
}

#[test]
fn a_fixup_on_a_scalar_is_refused() {
    // `_activityState` and `_majorIndex` aren't references.
    for offset in [0x54, 0xA4] {
        let mut b = IgzBuilder::new();
        let o = b.object(0, "tfbActorInfo", 0xF8);
        b.pointer(at(o, offset), o);
        let bytes = b.build();
        let igz = Igz::parse(&bytes).unwrap();
        let e = ActorInfo::read(&igz, 0).unwrap_err();
        assert_eq!(
            e.kind,
            ErrorKind::Unsupported {
                value: offset.into()
            }
        );
    }
}

#[test]
fn a_shape_or_bit_the_disc_never_sets_is_refused() {
    let read = |properties: u32| {
        let mut b = IgzBuilder::new();
        let o = b.object(0, "CollisionInfo", 0x58);
        b.put_u32(at(o, 0x18), properties);
        let bytes = b.build();
        CollisionInfo::read(&Igz::parse(&bytes).unwrap(), 0).map(|c| c.shape())
    };
    for (code, shape) in Shape::ALL.into_iter().enumerate() {
        assert_eq!(read(code as u32 | 0x38), Ok(shape));
    }
    for code in [6, 7] {
        assert_eq!(
            refused(read(code | 0x08)),
            (
                "_shapeType",
                ErrorKind::Unsupported {
                    value: (code | 0x08).into()
                }
            )
        );
    }
    assert_eq!(
        refused(read(0x41)),
        ("_properties", ErrorKind::Unsupported { value: 0x41 })
    );
}

/// A scene whose root group holds `children` as `build` makes them.
fn rooted(build: impl FnOnce(&mut IgzBuilder, u8) -> Vec<Location>) -> Vec<u8> {
    scene_over(|b, blocks| {
        let children = build(b, blocks);
        Some(group(b, blocks, GROUP, "root", &children))
    })
}

/// One drawable technique over one fresh segment.
fn drawn(b: &mut IgzBuilder, blocks: u8, name: &str) -> Location {
    let s = segment(b, blocks, &unskinned().wire);
    let g = geometry(b, blocks, &[s]);
    technique(b, name, g).0
}

fn walk(bytes: &[u8]) -> Result<Scene<'_>, FormatError> {
    let igz = Igz::parse(bytes).unwrap();
    // The scene's borrow is of the bytes, not of the parse.
    let i = igz
        .objects()
        .iter()
        .position(|o| igz.type_name(o) == "igSceneInfo")
        .unwrap();
    Scene::read(&igz, i)
}

#[test]
fn a_node_reached_twice_is_refused() {
    // One technique listed under two groups.
    let bytes = rooted(|b, blocks| {
        let t = drawn(b, blocks, "twice");
        let a = group(b, blocks, GROUP, "a", &[t]);
        let c = group(b, blocks, SPATIAL_NODE, "c", &[t]);
        vec![a, c]
    });
    assert_eq!(refused(walk(&bytes)), ("_childList", ErrorKind::Duplicate));
    // A group under itself.
    let bytes = scene_over(|b, blocks| {
        let g = b.object(0, "igGroup", 0x24);
        let list_at = b.object(0, "igNodeList", 0x18);
        b.pointer(at(g, 0x20), list_at);
        list(b, list_at, blocks, &[Some(g)]);
        Some(g)
    });
    assert_eq!(refused(walk(&bytes)), ("_childList", ErrorKind::Duplicate));
}

#[test]
fn a_child_list_two_groups_share_is_refused_unless_empty() {
    let shared = |children: usize| {
        scene_over(|b, blocks| {
            let ts: Vec<_> = (0..children)
                .map(|k| Some(drawn(b, blocks, &format!("t{k}"))))
                .collect();
            let shared_list = b.object(0, "igNodeList", 0x18);
            list(b, shared_list, blocks, &ts);
            let groups: Vec<_> = (0..2)
                .map(|_| {
                    let g = b.object(0, "igGroup", 0x24);
                    b.pointer(at(g, 0x20), shared_list);
                    Some(g)
                })
                .collect();
            let root = b.object(0, "igGroup", 0x24);
            let root_list = b.object(0, "igNodeList", 0x18);
            b.pointer(at(root, 0x20), root_list);
            list(b, root_list, blocks, &groups);
            Some(root)
        })
    };
    assert_eq!(
        refused(walk(&shared(2))),
        ("_childList", ErrorKind::Duplicate)
    );
    let bytes = shared(0);
    let scene = walk(&bytes).unwrap();
    assert_eq!((scene.groups.len(), scene.draws.len()), (3, 0));
}

#[test]
fn a_geometry_or_segment_drawn_twice_is_refused() {
    let bytes = rooted(|b, blocks| {
        let s = segment(b, blocks, &unskinned().wire);
        let g = geometry(b, blocks, &[s]);
        vec![technique(b, "a", g).0, technique(b, "b", g).0]
    });
    assert_eq!(refused(walk(&bytes)), ("_geometry", ErrorKind::Duplicate));
    let bytes = rooted(|b, blocks| {
        let s = segment(b, blocks, &unskinned().wire);
        let g = geometry(b, blocks, &[s]);
        let h = geometry(b, blocks, &[s]);
        vec![technique(b, "a", g).0, technique(b, "b", h).0]
    });
    assert_eq!(refused(walk(&bytes)), ("segment", ErrorKind::Duplicate));
}

#[test]
fn a_node_outside_the_scene_kinds_is_refused() {
    let bytes = rooted(|b, blocks| {
        let actor = b.object(0, "igActor2", 0x28);
        let l = b.object(0, "igNodeList", 0x18);
        b.pointer(at(actor, 0x20), l);
        list(b, l, blocks, &[]);
        vec![actor]
    });
    assert_eq!(
        refused(walk(&bytes)),
        ("_childList", wrong_type(SCENE_NODES, "igActor2"))
    );
    // A technique can't be the root.
    let bytes = scene_over(|b, blocks| Some(drawn(b, blocks, "root")));
    assert_eq!(
        refused(walk(&bytes)),
        (
            "_sceneGraph",
            wrong_type("igGroup", "tfbRuntimeTechniqueInstance")
        )
    );
}

#[test]
fn a_scene_without_a_root_or_with_more_roots_is_refused() {
    let bytes = scene_over(|_, _| None);
    assert_eq!(refused(walk(&bytes)), ("_sceneGraph", ErrorKind::Missing));

    let mut b = IgzBuilder::new();
    let info = b.object(0, "igSceneInfo", 0x48);
    let blocks = b.section(0x10);
    let root = group(&mut b, blocks, GROUP, "root", &[]);
    let other = group(&mut b, blocks, GROUP, "other", &[]);
    let more = b.object(0, "igNodeList", 0x18);
    b.pointer(at(info, 0x14), root);
    b.pointer(at(info, 0x40), more);
    list(&mut b, more, blocks, &[Some(other)]);
    let bytes = b.build();
    let kind = ErrorKind::CountMismatch {
        found: 1,
        expected: 0,
    };
    assert_eq!(refused(walk(&bytes)), ("_sceneGraphList", kind));
}

#[test]
fn a_technique_without_geometry_is_refused() {
    let bytes = rooted(|b, _| {
        let t = b.object(0, "tfbRuntimeTechniqueInstance", 0x38);
        vec![t]
    });
    assert_eq!(refused(walk(&bytes)), ("_geomAttr", ErrorKind::Missing));
    let bytes = rooted(|b, _| {
        let t = b.object(0, "tfbRuntimeTechniqueInstance", 0x38);
        let attr = b.object(0, "igEdgeGeometryAttr", 0x14);
        b.pointer(at(t, 0x28), attr);
        vec![t]
    });
    assert_eq!(refused(walk(&bytes)), ("_geometry", ErrorKind::Missing));
}

#[test]
fn a_transform_that_isnt_finite_is_refused() {
    let bytes = rooted(|b, blocks| {
        let t = drawn(b, blocks, "lost");
        let x = group(b, blocks, TRANSFORM, "x", &[t]);
        let mut m = IDENTITY;
        m[3][1] = f32::NAN;
        let bytes: Vec<u8> = m
            .as_flattened()
            .iter()
            .flat_map(|v| v.to_be_bytes())
            .collect();
        b.put_bytes(at(x, 0x30), &bytes);
        vec![x]
    });
    assert_eq!(
        refused(walk(&bytes)),
        (
            "_m",
            ErrorKind::Unsupported {
                value: f32::NAN.to_bits().into()
            }
        )
    );
}

#[test]
fn a_user_info_group_marks_what_hangs_below_it() {
    // A transform under the sky: its draw is the sky's too.
    let bytes = rooted(|b, blocks| {
        let t = drawn(b, blocks, "cloud");
        let x = group(b, blocks, TRANSFORM, "x", &[t]);
        let identity: Vec<u8> = IDENTITY
            .as_flattened()
            .iter()
            .flat_map(|v| v.to_be_bytes())
            .collect();
        b.put_bytes(at(x, 0x30), &identity);
        vec![group(b, blocks, SIMPLE_USER_INFO, "sky", &[x])]
    });
    let scene = walk(&bytes).unwrap();
    let [draw] = &scene.draws[..] else {
        panic!("{} draws", scene.draws.len());
    };
    let sky = scene.groups[&scene.root].children[0];
    assert_eq!(draw.user_info, Some(sky));
    assert_ne!(draw.parent, sky);
    assert_eq!(draw.world, IDENTITY);
}

#[test]
fn a_placement_of_another_kind_is_refused() {
    let mut b = IgzBuilder::new();
    let o = b.object(0, "Placement", 0xD8);
    placement(&mut b, o, "bare", 0, &[0.0; 6], 4);
    let bytes = b.build();
    let igz = Igz::parse(&bytes).unwrap();
    assert_eq!(
        refused(Placements::read(&igz)),
        ("placement", wrong_type(PLACEMENTS, "Placement"))
    );
}

#[test]
fn a_waypoint_in_two_lists_or_a_null_one_is_refused() {
    let lists = |shared: bool, null: bool| {
        let mut b = IgzBuilder::new();
        let blocks = b.section(0x10);
        let actors = [(); 2].map(|_| b.object(0, "tfbActorInfo", 0xF8));
        let ls = [(); 2].map(|_| b.object(0, "ActorWaypointList", 0x18));
        let ws = [(); 2].map(|_| b.object(0, "ActorWaypoint", 0x38));
        for k in 0..2 {
            b.pointer(at(actors[k], 0xE4), ls[k]);
        }
        let second = if shared { ws[0] } else { ws[1] };
        list(&mut b, ls[0], blocks, &[Some(ws[0])]);
        let slots = if null {
            vec![Some(second), None]
        } else {
            vec![Some(second)]
        };
        list(&mut b, ls[1], blocks, &slots);
        b.build()
    };
    let read = |bytes: &[u8]| {
        let igz = Igz::parse(bytes).unwrap();
        Placements::read(&igz).map(|p| p.waypoint_lists.len())
    };
    assert_eq!(read(&lists(false, false)), Ok(2));
    assert_eq!(
        refused(read(&lists(true, false))),
        ("_wayPoints", ErrorKind::Duplicate)
    );
    let (field, kind) = refused(read(&lists(false, true)));
    assert_eq!(field, "element");
    assert!(matches!(kind, ErrorKind::Unsupported { .. }));
}

#[test]
fn actors_sharing_a_list_read_it_once() {
    let mut b = IgzBuilder::new();
    let blocks = b.section(0x10);
    let actors = [(); 3].map(|_| b.object(0, "tfbActorInfo", 0xF8));
    let l = b.object(0, "ActorWaypointList", 0x18);
    let w = b.object(0, "ActorWaypoint", 0x38);
    for a in actors {
        b.pointer(at(a, 0xE4), l);
    }
    list(&mut b, l, blocks, &[Some(w)]);
    let bytes = b.build();
    let igz = Igz::parse(&bytes).unwrap();
    let p = Placements::read(&igz).unwrap();
    assert_eq!(p.waypoint_lists.len(), 1);
    assert!(p.actors.iter().all(|a| p.waypoints(a).len() == 1));
}
