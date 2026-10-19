use super::builder::{Sample, havok_bytes, inverse_joint, lip_sync_bytes, sample};
use super::*;
use crate::fnv1a32;
use crate::igz::builder::IgzBuilder;
use crate::igz::{MEMORY_PRESENT, kind};

fn index(igz: &Igz<'_>, at: Location) -> usize {
    igz.object_at(at).expect("an object there").0
}

fn sampled() -> (Sample, Vec<u8>) {
    let s = sample();
    let bytes = s.bytes.clone();
    (s, bytes)
}

/// A present memory ref's size word for `len` bytes at `4 << n` alignment.
fn size_word(len: u32, n: u32) -> u32 {
    MEMORY_PRESENT | (n << 27) | len
}

fn at(base: Location, offset: u32) -> Location {
    base.offset_by(offset).unwrap()
}

#[test]
fn the_table_is_closed_and_tagged() {
    assert_eq!(TAG, "IGZ v6 / BLUS30968 01.00");
    assert!(METHOD.contains("no statistical fallback"));
    for t in TYPES {
        assert_eq!(TYPES.iter().filter(|u| u.name == t.name).count(), 1);
        if let Some(parent) = t.parent {
            let p = type_def(parent).unwrap_or_else(|| panic!("{parent} missing"));
            assert!(t.size >= p.size, "{} smaller than {parent}", t.name);
        }
        assert!(
            is_a(t.name, "igObject"),
            "{} doesn't reach igObject",
            t.name
        );
    }
    let readers = [
        Image2::TYPE,
        EdgeGeometrySegment::TYPE,
        EdgeGeometry::TYPE,
        EdgeGeometryAttr::TYPE,
        Skeleton2::TYPE,
        SkeletonBoneList::TYPE,
        SkeletonBone::TYPE,
        AnimationSystem2::TYPE,
        PhysicsCombiner::TYPE,
        PhysicsCombinerLink::TYPE,
        Node::TYPE,
        Group::TYPE,
        NodeList::TYPE,
        AttrSet::TYPE,
        BlendMatrixSelect::TYPE,
        IntList::TYPE,
        Actor2::TYPE,
        RuntimeTechniqueInstance::TYPE,
        BuildDataInfo::TYPE,
        TfbAnimationInfo::TYPE,
        PhysicsAnimation::TYPE,
        BodyInfo::TYPE,
        PhysicsBody::TYPE,
        ModelInfo::TYPE,
        PhysicsModel::TYPE,
        AnimationInfoList::TYPE,
        AnimationInfo::TYPE,
        ObjectList::TYPE,
        AttrList::TYPE,
        TextureBindAttr2::TYPE,
        AlphaTextureUnitIdAttr::TYPE,
        TextureAttr2::TYPE,
        StringInfo::TYPE,
        ValueInfo::TYPE,
        ScriptSet::TYPE,
        ScriptSetList::TYPE,
        DirectionalLightInfo::TYPE,
        LanguagePackInfo::TYPE,
        SpriteInfo::TYPE,
        AnimatableTextureDataInfo::TYPE,
        RootList::TYPE,
        OpCode::TYPE,
        ScriptInfo::TYPE,
        OpCodeList::TYPE,
        OpUseMacro::TYPE,
        OpMacroInterface::TYPE,
        OpDefineMacro::TYPE,
        Variable::TYPE,
        OpMacroParameter::TYPE,
        OpChangeMembership::TYPE,
        OpLoopValue::TYPE,
        Stack::TYPE,
        RhsValueStack::TYPE,
        RhsReferenceStack::TYPE,
        ValueRhsVariant::TYPE,
    ];
    for r in readers {
        assert!(type_def(r).is_some(), "{r} has no TypeDef");
    }
    assert!(is_a("tfbPhysicsBody", "tfbBuildDataInfo"));
    assert!(is_a("igBlendMatrixSelect", "igGroup"));
    assert!(!is_a("igGroup", "igBlendMatrixSelect"));
    assert!(!is_a("igSomethingElse", "igObject"));
}

#[test]
fn image2_reads_back() {
    let (s, bytes) = sampled();
    let igz = Igz::parse(&bytes).unwrap();
    let image = Image2::read(&igz, index(&igz, s.image)).unwrap();
    assert_eq!(image.name, Some("eruptor_body"));
    assert_eq!(image.color_scale, [1.0, 0.5, 0.25, 2.0]);
    assert_eq!(image.color_bias, [0.125, 0.0, -1.0, 0.0]);
    assert_eq!(
        (
            image.width,
            image.height,
            image.depth,
            image.level_count,
            image.image_count
        ),
        (8, 4, 1, 2, 6)
    );
    assert_eq!(image.format.id.namespace, fnv1a32(b"metaimages"));
    assert_eq!(image.format.id.object, fnv1a32(b"dxt5_tile_big_ps3"));
    assert_eq!(image.format_name(), Some("dxt5_tile_big_ps3"));
    assert_eq!(image.usage_deprecated, 2);
    assert_eq!(image.padding_deprecated, 0);
    let data = image.data.unwrap();
    assert_eq!(data.index, 0);
    assert_eq!(data.block.data, [0xAB; 128]);
    assert_eq!(data.block.alignment(), 128);
    assert_eq!(
        (image.lock_count, image.tex_handle, image.locked_memory),
        (0, -1, 0)
    );
    assert!(image.ogl_discard_original_image);
}

#[test]
fn edge_segments_read_back() {
    let (s, bytes) = sampled();
    let igz = Igz::parse(&bytes).unwrap();
    let seg = EdgeGeometrySegment::read(&igz, index(&igz, s.segments[0])).unwrap();
    let config = seg.spu_config_info.unwrap();
    assert_eq!(
        (config.size_word, config.data.len()),
        (size_word(16, 2), 16)
    );
    assert_eq!(seg.counts(), Some((84, 360)));
    let indexes = seg.indexes.unwrap();
    assert_eq!(
        (indexes.size_word, indexes.data),
        (size_word(6, 5), &[1, 2, 3, 4, 5, 6][..])
    );
    assert_eq!(seg.indexes_sizes, [6, 0]);
    assert_eq!(seg.spu_vertexes0.unwrap().data, [7; 12]);
    assert_eq!(seg.spu_vertexes1.unwrap().data, [8; 8]);
    assert_eq!(seg.spu_vertexes_sizes, [4, 4, 4, 2, 2, 4]);
    assert_eq!(seg.rsx_only_vertexes.unwrap().data, [9; 10]);
    assert_eq!(seg.rsx_only_vertexes_size, 10);
    assert_eq!(seg.skin_matrices_byte_offsets, [0, 96]);
    assert_eq!(seg.skin_matrices_sizes, [48, 96]);
    assert_eq!(seg.skin_indexes_and_weights_sizes, [8, 8]);
    assert_eq!(seg.skin_indexes_and_weights.unwrap().data, [10; 16]);
    assert!(seg.is_skinned());
    assert_eq!((seg.io_buffer_size, seg.scratch_size), (0x1000, 0x200));
    assert_eq!(seg.spu_input_stream_descs0.unwrap().data, [11; 8]);
    assert_eq!(seg.spu_input_stream_descs1, None);
    assert_eq!(seg.spu_output_stream_desc.unwrap().data, [12; 16]);
    assert_eq!(seg.rsx_only_stream_desc.unwrap().data, [13; 24]);
    assert_eq!(seg.spu_input_stream_desc_sizes, [8, 0]);
    assert_eq!(
        (
            seg.spu_output_stream_desc_size,
            seg.rsx_only_stream_desc_size
        ),
        (16, 24)
    );
    assert_eq!(seg.num_blend_shapes, 0);
    assert_eq!(
        seg.blend_shape_sizes,
        Vector {
            count: 0,
            data: None
        }
    );
    assert_eq!(seg.blend_shape_data, None);
    assert_eq!(seg.blend_shapes.count, 2);
    assert_eq!(seg.blend_shapes.data.unwrap().data, [14; 8]);

    let unskinned = EdgeGeometrySegment::read(&igz, index(&igz, s.segments[2])).unwrap();
    assert!(!unskinned.is_skinned());
    assert_eq!(unskinned.spu_vertexes1, None);
    assert_eq!(unskinned.spu_vertexes_sizes, [4, 4, 4, 0, 0, 0]);
    assert_eq!(unskinned.skin_indexes_and_weights_sizes, [0, 0]);
    assert_eq!(
        unskinned.blend_shapes,
        Vector {
            count: 0,
            data: None
        }
    );
}

#[test]
fn edge_geometry_and_attr_read_back() {
    let (s, bytes) = sampled();
    let igz = Igz::parse(&bytes).unwrap();
    let g = EdgeGeometry::read(&igz, index(&igz, s.geometries[0])).unwrap();
    assert_eq!(
        g.segments,
        [index(&igz, s.segments[0]), index(&igz, s.segments[1])]
    );
    assert_eq!((g.list.count, g.list.capacity), (2, 2));
    assert_eq!(g.hash_code, 0x00C0_FFEE);
    let attr = EdgeGeometryAttr::read(&igz, index(&igz, s.attrs[1])).unwrap();
    assert_eq!(
        attr.attr,
        Attr {
            cached_unit_id: 0,
            cached_attr_index: -1,
            read_only_copy: false
        }
    );
    assert_eq!(attr.geometry, Some(index(&igz, s.geometries[1])));
    // A segment list read untyped sees the same elements.
    let plain = ObjectList::read(&igz, index(&igz, s.geometries[0])).unwrap();
    assert_eq!(
        plain.elements,
        [
            Some(index(&igz, s.segments[0])),
            Some(index(&igz, s.segments[1]))
        ]
    );
}

#[test]
fn skeleton_and_combiner_read_back() {
    let (s, bytes) = sampled();
    let igz = Igz::parse(&bytes).unwrap();
    let skel = Skeleton2::read(&igz, index(&igz, s.skeleton)).unwrap();
    assert_eq!(skel.name, Some("DefaultSkeleton_0"));
    assert_eq!(skel.bone_list, Some(index(&igz, s.bone_list)));
    assert_eq!(
        skel.inverse_joint_array.unwrap().size_word,
        size_word(128, 2)
    );
    assert_eq!(skel.inverse_joints, [inverse_joint(0), inverse_joint(1)]);
    assert_eq!(skel.inverse_joints[1][3], [28.0, 29.0, 30.0, 31.0]);

    let list = SkeletonBoneList::read(&igz, index(&igz, s.bone_list)).unwrap();
    assert_eq!(list.bones, s.bones.map(|b| index(&igz, b)));
    let head = SkeletonBone::read(&igz, index(&igz, s.bones[2])).unwrap();
    assert_eq!(
        head,
        SkeletonBone {
            name: Some("J_head"),
            parent_index: 1,
            blend_matrix_index: 0,
            translation: [0.25, 0.5, -0.125]
        }
    );
    let root = SkeletonBone::read(&igz, index(&igz, s.bones[0])).unwrap();
    assert_eq!((root.parent_index, root.blend_matrix_index), (-1, 1));

    let link = PhysicsCombinerLink::read(&igz, index(&igz, s.combiner)).unwrap();
    assert_eq!(link.id_code, 0xBB);
    assert_eq!(link.combiner.id_code_padding, 0xAA);
    assert_eq!(
        (link.combiner.joint_index_array, link.combiner.anim_array),
        (None, None)
    );
    let system = link.combiner.system;
    assert_eq!(system.name, Some("Eruptor_combiner"));
    assert_eq!(system.skeleton, Some(index(&igz, s.skeleton)));
    assert_eq!(system.concatenated_bone_matrix_array, None);
    assert_eq!(system.concatenated_blend_matrix_array, None);
    assert_eq!(system.matrix_array_time, i32::MIN);
    // A base reader on the subtype reads the same fields.
    assert_eq!(
        AnimationSystem2::read(&igz, index(&igz, s.combiner)).unwrap(),
        system
    );
}

#[test]
fn scene_graph_reads_back() {
    let (s, bytes) = sampled();
    let igz = Igz::parse(&bytes).unwrap();
    let actor = Actor2::read(&igz, index(&igz, s.actor)).unwrap();
    assert_eq!(
        actor.group.node,
        Node {
            name: Some("J_root"),
            bound: ObjectRef::Null,
            parent_list: ObjectRef::Null,
            traversal_weight: -2,
            traversal_spu_weight: 3,
            handle: -1,
            flags: 0x40
        }
    );
    assert_eq!(actor.group.child_list, Some(index(&igz, s.actor_children)));
    assert_eq!(actor.animation_system, Some(index(&igz, s.combiner)));
    let children = NodeList::read(&igz, index(&igz, s.actor_children)).unwrap();
    assert_eq!(children.nodes, [index(&igz, s.select)]);

    let select = BlendMatrixSelect::read(&igz, index(&igz, s.select)).unwrap();
    assert_eq!(select.attr_set.group.node.name, Some("skin"));
    assert_eq!(
        select.attr_set.group.child_list,
        Some(index(&igz, s.select_children))
    );
    assert_eq!(select.attr_set.attributes, ObjectRef::Null);
    assert!(!select.attr_set.trigger);
    assert_eq!(select.skeleton_transform[0], [1.0, 2.0, 3.0, 4.0]);
    assert_eq!(select.skeleton_transform[3], [13.0, 14.0, 15.0, 16.0]);
    assert_eq!(select.skeleton_inverse_transform[0][0], 17.0);
    assert_eq!(select.skeleton_inverse_transform[3][3], 32.0);
    assert!(select.use_previous);
    assert_eq!(select.blend_matrix_indices, Some(index(&igz, s.palette)));
    let palette = IntList::read(&igz, index(&igz, s.palette)).unwrap();
    assert_eq!(palette.values, [1, 0]);

    let t = RuntimeTechniqueInstance::read(&igz, index(&igz, s.techniques[1])).unwrap();
    assert_eq!(t.node.name, Some("EruptorEyes_Teeth"));
    assert_eq!((t.ti_flags, t.properties), (0x11, 0x22));
    assert_eq!(t.geom_attr, Some(index(&igz, s.attrs[1])));
    assert_eq!(
        (t.material_anim, t.material_pass),
        (
            ObjectRef::Null,
            ObjectRef::Object(index(&igz, s.materials[1]))
        )
    );
    let ObjectRef::Import(effect) = t.effect else {
        panic!("an import")
    };
    assert!(effect.id.is("effects", "skin_fx"));
    // Base readers on subtypes.
    assert_eq!(
        Group::read(&igz, index(&igz, s.select)).unwrap(),
        select.attr_set.group
    );
    assert_eq!(
        Node::read(&igz, index(&igz, s.techniques[1])).unwrap(),
        t.node
    );
}

#[test]
fn materials_read_back() {
    let (s, bytes) = sampled();
    let igz = Igz::parse(&bytes).unwrap();
    let i = |l: Location| index(&igz, l);
    let m0 = AttrList::read(&igz, i(s.materials[0])).unwrap();
    assert_eq!(
        m0.attrs,
        [
            Some(i(s.binds[0])),
            Some(i(s.binds[1])),
            Some(i(s.shader_mask))
        ]
    );
    let m1 = AttrList::read(&igz, i(s.materials[1])).unwrap();
    assert_eq!(m1.attrs, [Some(i(s.binds[2])), Some(i(s.binds[0]))]);

    let unit = |l| AlphaTextureUnitIdAttr::read(&igz, i(l)).unwrap();
    let b0 = unit(s.binds[0]);
    assert_eq!(
        b0.bind.attr,
        Attr {
            cached_unit_id: 0,
            cached_attr_index: -1,
            read_only_copy: false
        }
    );
    assert_eq!(b0.bind.texture, Some(i(s.textures[0])));
    assert_eq!((b0.bind.unit_id, b0.data), (-2, -3));
    assert_eq!(b0.alpha_texture, ObjectRef::Null);
    let b1 = unit(s.binds[1]);
    assert_eq!((b1.bind.unit(), b1.bind.unit_id, b1.data), (4, -4, -5));
    assert_eq!(b1.alpha_texture, ObjectRef::Object(i(s.textures[1])));
    assert_eq!(unit(s.binds[2]).bind.unit(), 3);
    // The base reader on the subtype.
    assert_eq!(
        TextureBindAttr2::read(&igz, i(s.binds[1])).unwrap(),
        b1.bind
    );

    let t0 = TextureAttr2::read(&igz, i(s.textures[0])).unwrap();
    assert_eq!(
        t0,
        TextureAttr2 {
            attr: Attr {
                cached_unit_id: -1,
                cached_attr_index: 2,
                read_only_copy: false
            },
            format_hint: 0x5446_4E31,
            load_in_vram_for_ctr: true,
            mag_filter: 1,
            min_filter: 2,
            wrap_s: 3,
            wrap_t: 4,
            msaa_type: 5,
            source: 6,
            image: Some(i(s.image)),
            tex_handle: -1,
            image_handle: 0x0A,
            wrap_r: 0x0B,
        }
    );
    let t1 = TextureAttr2::read(&igz, i(s.textures[1])).unwrap();
    assert_eq!((t1.image, t1.mag_filter, t1.wrap_r), (None, 0x11, 0x0C));
}

#[test]
fn havok_wrappers_read_back() {
    let (s, bytes) = sampled();
    let igz = Igz::parse(&bytes).unwrap();
    let body = PhysicsBody::read(&igz, index(&igz, s.body)).unwrap();
    let data_info = body.body.data_info;
    assert_eq!(
        data_info.info,
        Info {
            name: Some("Eruptor.ma"),
            directory: ObjectRef::Null,
            resolve_state: true
        }
    );
    let data = data_info.data.unwrap();
    assert_eq!(data.size_word, size_word(16, 2));
    assert_eq!(data.alignment(), 16);
    assert_eq!(
        data_info.payload(),
        Some(Payload::Havok(&havok_bytes(0xA5)))
    );
    assert_eq!(data_info.stream_archive_hash, -7);
    assert_eq!(body.body.node, Some(index(&igz, s.actor)));
    assert_eq!(body.body.combiner_prototype, Some(index(&igz, s.combiner)));
    assert_eq!(body.body.bone_mapping_table, None);
    assert_eq!(body.body.viseme_set, ObjectRef::Null);
    assert_eq!(
        body.body.eye_blink,
        ObjectRef::Object(index(&igz, s.lip_sync))
    );
    assert_eq!(
        body.body.footprint_data,
        ObjectRef::Object(index(&igz, s.idle))
    );
    assert_eq!(body.id_code, 0x55);
    assert_eq!(
        BuildDataInfo::read(&igz, index(&igz, s.body)).unwrap(),
        data_info
    );

    let idle = PhysicsAnimation::read(&igz, index(&igz, s.idle)).unwrap();
    let info = idle.info;
    assert_eq!(info.data_info.info.name, Some("Eruptor_Idle.ma"));
    assert_eq!(
        info.data_info.payload(),
        Some(Payload::Havok(&havok_bytes(0x5A)))
    );
    assert_eq!((info.properties, info.animation_slot_index), (0x610, 7));
    assert_eq!(info.motion_extraction_flags(), 0);
    assert!(info.additive_blend());
    assert!(!info.partial_animation() && !info.layered());
    assert_eq!(info.compression_quality(), 4);
    assert!(info.status_fields_set());
    assert_eq!(idle.id_code, 0);

    let lip = PhysicsAnimation::read(&igz, index(&igz, s.lip_sync)).unwrap();
    let lip_info = lip.info;
    assert_eq!(
        lip_info.data_info.payload(),
        Some(Payload::LipSync(&lip_sync_bytes()))
    );
    assert_eq!(lip_info.animation_slot_index, -1);
    assert_eq!(lip_info.compression_quality(), 4);
    assert!(!lip_info.additive_blend() && !lip_info.status_fields_set());
}

#[test]
fn model_and_clip_table_read_back() {
    let (s, bytes) = sampled();
    let igz = Igz::parse(&bytes).unwrap();
    let model = PhysicsModel::read(&igz, index(&igz, s.model)).unwrap();
    let m = model.model;
    assert_eq!(m.info.name, Some("Eruptor.mdl"));
    assert_eq!(m.tfb_body, Some(index(&igz, s.body)));
    let ObjectRef::Import(params) = m.model_parameters else {
        panic!("an import")
    };
    assert!(params.id.is("models", "ActorParameters"));
    assert_eq!(m.animation_set, Some(index(&igz, s.clip_table)));
    assert_eq!(m.collision, ObjectRef::Null);
    let ObjectRef::Named { name, .. } = m.attach_list else {
        panic!("a named ref")
    };
    assert_eq!(
        (name.namespace, name.name),
        ("objects", "EruptorAttachList")
    );
    assert_eq!(m.template_path, Some("templates/eruptor"));
    assert_eq!(model.id_code, 0x1234);

    let table = AnimationInfoList::read(&igz, index(&igz, s.clip_table)).unwrap();
    assert_eq!(table.entries, [Some(index(&igz, s.clip_info)), None]);
    let info = AnimationInfo::read(&igz, index(&igz, s.clip_info)).unwrap();
    assert_eq!(info.name, Some("Idle"));
    assert_eq!(info.properties, 0x610);
    assert_eq!(info.media_name, Some("Eruptor_Idle"));
    assert_eq!(info.blend_factor, 0.5);
    assert_eq!(info.tfb_animation, Some(index(&igz, s.idle)));
    assert_eq!(info.blend_list, ObjectRef::Null);
}

#[test]
fn the_hero_walks_from_its_model() {
    let (s, bytes) = sampled();
    let igz = Igz::parse(&bytes).unwrap();
    let hero = Hero::read(&igz, index(&igz, s.model)).unwrap();
    assert_eq!(hero.body_index, index(&igz, s.body));
    assert_eq!(hero.actor_index, index(&igz, s.actor));
    assert_eq!(hero.combiner_index, index(&igz, s.combiner));
    assert_eq!(hero.skeleton_index, index(&igz, s.skeleton));
    let names: Vec<_> = hero.bones.iter().map(|b| b.name.unwrap()).collect();
    assert_eq!(names, ["J_root", "J_hip", "J_head"]);
    assert_eq!(hero.skeleton.inverse_joints.len(), 2);

    let techniques: Vec<_> = hero.meshes.iter().map(|m| m.technique_index).collect();
    assert_eq!(techniques, s.techniques.map(|t| index(&igz, t)));
    let select = index(&igz, s.select);
    assert!(hero.meshes.iter().all(|m| m.palette == Some(select)));
    let palette = hero.palettes[&select].indices.unwrap();
    assert_eq!(palette, index(&igz, s.palette));
    assert_eq!(hero.int_lists[&palette].values, [1, 0]);
    let per_mesh: Vec<usize> = hero
        .meshes
        .iter()
        .map(|m| hero.mesh_segments(m).count())
        .collect();
    assert_eq!(per_mesh, [2, 1]);
    assert_eq!(hero.segments.len(), 3);

    let materials: Vec<_> = hero.meshes.iter().map(|m| m.material).collect();
    assert_eq!(materials, s.materials.map(|m| Some(index(&igz, m))));
    // Bind 0 is shared, read once; the shader mask stays unread.
    assert_eq!(hero.materials.len(), 2);
    assert_eq!(hero.texture_binds.len(), 3);
    assert_eq!(hero.textures.len(), 2);
    let colour = index(&igz, s.textures[0]);
    for m in &hero.meshes {
        let (t, attr) = hero.texture_at(m, 0).unwrap();
        assert_eq!((t, attr.image), (colour, Some(index(&igz, s.image))));
    }
    let normal = hero.texture_at(&hero.meshes[0], 4).unwrap();
    assert_eq!(
        (normal.0, normal.1.image),
        (index(&igz, s.textures[1]), None)
    );
    assert!(hero.texture_at(&hero.meshes[0], 3).is_none());
    assert_eq!(hero.texture_at(&hero.meshes[1], 3).unwrap().0, colour);

    assert_eq!(hero.clip_slots, [Some(index(&igz, s.clip_info)), None]);
    let clip = &hero.clips[&index(&igz, s.clip_info)];
    let (idle, anim) = clip.animation.unwrap();
    assert_eq!(idle, index(&igz, s.idle));
    assert_eq!(anim.info.data_info.info.name, Some("Eruptor_Idle.ma"));
}

#[test]
fn read_all_takes_subtypes() {
    let (_, bytes) = sampled();
    let igz = Igz::parse(&bytes).unwrap();
    // The body and the two animations.
    assert_eq!(read_all::<BuildDataInfo>(&igz).count(), 3);
    assert_eq!(read_all::<PhysicsAnimation>(&igz).count(), 2);
    // Actor, select and techniques.
    assert_eq!(read_all::<Node>(&igz).count(), 4);
    assert!(read_all::<Node>(&igz).all(|(_, r)| r.is_ok()));
}

/// One object of `ty` (size `size`) in a fresh builder, for broken files.
fn one(ty: &str, size: u32) -> (IgzBuilder, Location) {
    let mut b = IgzBuilder::new();
    let o = b.object(0, ty, size);
    (b, o)
}

fn wrong_type(e: FormatError) -> (&'static str, String) {
    match e.kind {
        ErrorKind::WrongType { expected, found } => (expected, found),
        other => panic!("not WrongType: {other:?}"),
    }
}

#[test]
fn a_reader_refuses_another_type() {
    let (s, bytes) = sampled();
    let igz = Igz::parse(&bytes).unwrap();
    let e = Image2::read(&igz, index(&igz, s.segments[0])).unwrap_err();
    assert_eq!(
        wrong_type(e),
        ("igImage2", "igPS3EdgeGeometrySegment".into())
    );
    // A parent type isn't its subtype.
    let e = PhysicsBody::read(&igz, index(&igz, s.idle)).unwrap_err();
    assert_eq!(
        wrong_type(e),
        ("tfbPhysicsBody", "tfbPhysicsAnimation".into())
    );
    // Past the last object.
    let e = Image2::read(&igz, igz.objects().len()).unwrap_err();
    assert!(matches!(e.kind, ErrorKind::OutOfRange { .. }));

    // A type outside the table.
    let (b, _) = one("igSomethingElse", 0x60);
    let bytes = b.build();
    let igz = Igz::parse(&bytes).unwrap();
    assert_eq!(
        wrong_type(Image2::read(&igz, 0).unwrap_err()),
        ("igImage2", "igSomethingElse".into())
    );
}

#[test]
fn an_mtsz_size_off_the_registry_is_refused() {
    let (b, _) = one("igImage2", 0x64);
    let bytes = b.build();
    let igz = Igz::parse(&bytes).unwrap();
    let e = Image2::read(&igz, 0).unwrap_err();
    assert_eq!(
        (e.field, e.kind),
        ("MTSZ", ErrorKind::Unsupported { value: 0x64 })
    );
}

#[test]
fn a_fixup_on_an_undeclared_word_is_refused() {
    let (mut b, image) = one("igImage2", 0x60);
    b.import(at(image, 0x3C), METAIMAGES, "dxt1_big_ps3");
    // `_usageDeprecated` is an i32, never a pointer.
    b.pointer(at(image, 0x40), image);
    let bytes = b.build();
    let igz = Igz::parse(&bytes).unwrap();
    let e = Image2::read(&igz, 0).unwrap_err();
    assert_eq!(e.kind, ErrorKind::Unsupported { value: 0x40 });

    // The same word unmarked reads as the integer it is.
    let (mut b, image) = one("igImage2", 0x60);
    b.import(at(image, 0x3C), METAIMAGES, "dxt1_big_ps3");
    b.put_u32(at(image, 0x40), 3);
    let bytes = b.build();
    let igz = Igz::parse(&bytes).unwrap();
    let image = Image2::read(&igz, 0).unwrap();
    assert_eq!(
        (image.usage_deprecated, image.format_name(), image.data),
        (3, Some("dxt1_big_ps3"), None)
    );
}

#[test]
fn image2_needs_an_imported_format() {
    let (b, _) = one("igImage2", 0x60);
    let bytes = b.build();
    let igz = Igz::parse(&bytes).unwrap();
    let e = Image2::read(&igz, 0).unwrap_err();
    assert_eq!(e.field, "_format");
    // An unknown metaimage reads, with no name.
    let (mut b, image) = one("igImage2", 0x60);
    b.import(at(image, 0x3C), METAIMAGES, "rgba8888_big_ps3");
    let bytes = b.build();
    let igz = Igz::parse(&bytes).unwrap();
    assert_eq!(Image2::read(&igz, 0).unwrap().format_name(), None);
}

#[test]
fn a_bool_other_than_0_or_1_is_refused() {
    let (mut b, image) = one("igImage2", 0x60);
    b.import(at(image, 0x3C), METAIMAGES, "dxt1_big_ps3");
    b.put_bytes(at(image, 0x58), &[2]);
    let bytes = b.build();
    let igz = Igz::parse(&bytes).unwrap();
    let e = Image2::read(&igz, 0).unwrap_err();
    assert_eq!(
        (e.field, e.kind),
        (
            "_oglDiscardOriginalImage",
            ErrorKind::Unsupported { value: 2 }
        )
    );
}

#[test]
fn a_ref_to_the_wrong_target_type_is_refused() {
    let mut b = IgzBuilder::new();
    let attr = b.object(0, "igEdgeGeometryAttr", 0x14);
    let seg = b.object(0, "igPS3EdgeGeometrySegment", 0xB0);
    b.pointer(at(attr, 0x10), seg);
    let bytes = b.build();
    let igz = Igz::parse(&bytes).unwrap();
    let e = EdgeGeometryAttr::read(&igz, 0).unwrap_err();
    assert_eq!(e.field, "_geometry");
    assert_eq!(
        wrong_type(e),
        ("igPS3EdgeGeometry", "igPS3EdgeGeometrySegment".into())
    );

    // An import where a local object belongs.
    let (mut b, attr) = one("igEdgeGeometryAttr", 0x14);
    b.import(at(attr, 0x10), "geometry", "elsewhere");
    let bytes = b.build();
    let igz = Igz::parse(&bytes).unwrap();
    let e = EdgeGeometryAttr::read(&igz, 0).unwrap_err();
    assert_eq!(
        (e.field, e.kind),
        ("_geometry", ErrorKind::Unsupported { value: 0x8000_0000 })
    );
}

#[test]
fn typed_lists_refuse_nulls_strangers_and_bad_counts() {
    let mut b = IgzBuilder::new();
    let blocks = b.section(0x10);
    let geom = b.object(0, "igPS3EdgeGeometry", 0x1C);
    builder::list(&mut b, geom, blocks, &[None]);
    let bytes = b.build();
    let igz = Igz::parse(&bytes).unwrap();
    let e = EdgeGeometry::read(&igz, 0).unwrap_err();
    assert_eq!(
        (e.field, e.kind),
        ("element", ErrorKind::Unsupported { value: 0 })
    );
    // Untyped, the same slot is an empty one.
    assert_eq!(ObjectList::read(&igz, 0).unwrap().elements, [None]);

    let mut b = IgzBuilder::new();
    let blocks = b.section(0x10);
    let geom = b.object(0, "igPS3EdgeGeometry", 0x1C);
    let image = b.object(0, "igImage2", 0x60);
    builder::list(&mut b, geom, blocks, &[Some(image)]);
    let bytes = b.build();
    let igz = Igz::parse(&bytes).unwrap();
    assert_eq!(
        wrong_type(EdgeGeometry::read(&igz, 0).unwrap_err()),
        ("igPS3EdgeGeometrySegment", "igImage2".into())
    );

    for count in [-1i32, 2] {
        let mut b = IgzBuilder::new();
        let blocks = b.section(0x10);
        let ints = b.object(0, "igIntList", 0x18);
        b.put_u32(at(ints, 0x08), count as u32);
        b.memory_ref(at(ints, 0x10), blocks, &5i32.to_be_bytes(), 0);
        let bytes = b.build();
        let igz = Igz::parse(&bytes).unwrap();
        let e = IntList::read(&igz, 0).unwrap_err();
        assert_eq!(e.field, "_count", "count {count}");
    }
}

#[test]
fn a_vector_count_without_data_is_refused() {
    let (mut b, seg) = one("igPS3EdgeGeometrySegment", 0xB0);
    // Every memory ref null, as the reader insists.
    for off in [
        0x08, 0x10, 0x1C, 0x24, 0x38, 0x50, 0x60, 0x68, 0x70, 0x78, 0x90, 0x98, 0xA4,
    ] {
        b.null_memory_ref(at(seg, off), 0);
    }
    b.put_u32(at(seg, 0xA0), 3);
    let bytes = b.build();
    let igz = Igz::parse(&bytes).unwrap();
    let e = EdgeGeometrySegment::read(&igz, 0).unwrap_err();
    assert_eq!(
        (e.field, e.kind),
        ("_blendShapes", ErrorKind::Unsupported { value: 3 })
    );
}

#[test]
fn inverse_joints_must_be_whole_matrices() {
    let mut b = IgzBuilder::new();
    let blocks = b.section(0x10);
    let skel = b.object(0, "igSkeleton2", 0x18);
    b.memory_ref(at(skel, 0x10), blocks, &[0; 60], 2);
    let bytes = b.build();
    let igz = Igz::parse(&bytes).unwrap();
    let e = Skeleton2::read(&igz, 0).unwrap_err();
    assert_eq!(
        (e.field, e.kind),
        ("_inverseJointArray", ErrorKind::Unsupported { value: 60 })
    );
}

#[test]
fn animation_info_refuses_a_marker_on_its_parents_scalars() {
    // Script func structs, `_flags` and `_playbackPercent` hold no refs.
    for offset in [0x0C, 0x18, 0x20] {
        let (mut b, info) = one("AnimationInfo", 0x44);
        b.string(at(info, offset), "not a ref");
        let bytes = b.build();
        let igz = Igz::parse(&bytes).unwrap();
        let e = AnimationInfo::read(&igz, 0).unwrap_err();
        assert_eq!(
            (e.field, e.kind),
            (
                "string",
                ErrorKind::Unsupported {
                    value: offset.into()
                }
            )
        );
    }
    // `_activator`, `_clones` and `_parent` are refs, left unfollowed.
    let (mut b, info) = one("AnimationInfo", 0x44);
    for (offset, _) in AnimationInfo::SEQUENCE_REFS {
        b.pointer(at(info, offset), info);
    }
    let bytes = b.build();
    let igz = Igz::parse(&bytes).unwrap();
    assert!(AnimationInfo::read(&igz, 0).is_ok());
}

#[test]
fn a_child_list_shared_by_many_groups_is_read_once() {
    // Re-reading it per group held groups^2 / 2 stack entries.
    let bytes = builder::shared_child_list(2000);
    let igz = Igz::parse(&bytes).unwrap();
    let hero = Hero::read(&igz, 0).unwrap();
    assert!(hero.meshes.is_empty() && hero.palettes.is_empty());
}

#[test]
fn selects_sharing_an_int_list_share_one_copy() {
    let bytes = builder::shared_palette(50, 1000);
    let igz = Igz::parse(&bytes).unwrap();
    let hero = Hero::read(&igz, 0).unwrap();
    assert_eq!((hero.palettes.len(), hero.int_lists.len()), (50, 1));
    let list = hero.int_lists.values().next().unwrap();
    assert_eq!(list.values.len(), 1000);
    assert!(
        hero.palettes
            .values()
            .all(|p| p.indices == hero.int_lists.keys().next().copied())
    );
}

#[test]
fn techniques_sharing_a_material_read_it_once() {
    let bytes = builder::shared_material(300, 200, "tfbAlphaTextureUnitIDAttr");
    let igz = Igz::parse(&bytes).unwrap();
    let hero = Hero::read(&igz, 0).unwrap();
    assert_eq!(hero.meshes.len(), 300);
    assert_eq!(
        (
            hero.materials.len(),
            hero.texture_binds.len(),
            hero.textures.len()
        ),
        (1, 200, 1)
    );
    assert!(hero.meshes.iter().all(|m| hero.texture_at(m, 0).is_some()));
}

#[test]
fn a_bind_other_than_the_alpha_unit_attr_is_refused() {
    let bytes = builder::shared_material(1, 1, "igTextureBindAttr2");
    let igz = Igz::parse(&bytes).unwrap();
    let e = Hero::read(&igz, 0).unwrap_err();
    assert_eq!(
        wrong_type(e),
        ("tfbAlphaTextureUnitIDAttr", "igTextureBindAttr2".to_owned())
    );
}

#[test]
fn a_hero_missing_a_link_is_refused_and_cycles_end() {
    let mut b = IgzBuilder::new();
    let model = b.object(0, "tfbPhysicsModel", 0x30);
    let bytes = b.build();
    let igz = Igz::parse(&bytes).unwrap();
    let e = Hero::read(&igz, 0).unwrap_err();
    assert_eq!(e.field, "_tfbBody");
    let _ = model;

    // A child list that holds the actor itself.
    let bytes = builder::bare_hero(|_, _, actor, _| vec![actor, actor]);
    let igz = Igz::parse(&bytes).unwrap();
    let hero = Hero::read(&igz, 0).unwrap();
    assert!(hero.meshes.is_empty() && hero.bones.is_empty() && hero.clips.is_empty());
}

#[test]
fn the_sample_carries_every_marker_kind_the_layouts_read() {
    let (_, bytes) = sampled();
    let igz = Igz::parse(&bytes).unwrap();
    for k in [
        kind::POINTERS,
        kind::IMPORTS,
        kind::HANDLES,
        kind::RSTR,
        kind::NULL_MEMORY_REFS,
        kind::META_OBJECTS,
    ] {
        assert!(igz.record(k).is_some(), "kind {k:#x}");
    }
}

fn scripted() -> (super::builder::ScriptSample, Vec<u8>) {
    let s = super::builder::script_sample();
    let bytes = s.bytes.clone();
    (s, bytes)
}

#[test]
fn roster_script_objects_read_back() {
    let (s, bytes) = scripted();
    let igz = Igz::parse(&bytes).unwrap();
    let key = StringInfo::read(&igz, index(&igz, s.string_info)).unwrap();
    assert_eq!(
        key,
        StringInfo {
            name: Some("ItemName_Anvil"),
            unicode_text: 0,
            flags: 7,
            integer_digits: 3,
            decimal_digits: -2,
        }
    );

    let value = ValueInfo::read(&igz, index(&igz, s.value_info)).unwrap();
    assert_eq!(value.name, Some("ElementIndex_Fire"));
    assert_eq!((value.reset_value, value.value), (-4, 5));
    // An unflagged kind-0x10 word names its EXNM entry.
    assert!(matches!(
        value.value_type,
        TypeRef::Indexed { index: 0, .. }
    ));
    assert_eq!(value.value_type.metaobject(), Some("IntMeasurement"));

    let set = ScriptSet::read(&igz, index(&igz, s.set)).unwrap();
    assert_eq!(set.name, Some("Element_Values"));
    assert_eq!(set.list, Some(index(&igz, s.set_list)));
    let list = ScriptSetList::read(&igz, set.list.unwrap()).unwrap();
    assert_eq!(list.list.count, 3);
    assert_eq!(
        list.elements[0],
        ObjectRef::Object(index(&igz, s.value_info))
    );
    let ObjectRef::Import(import) = list.elements[1] else {
        panic!("{:?}", list.elements[1])
    };
    assert!(import.id.is("global.bld", "Minion_Names"));
    assert_eq!(list.elements[2], ObjectRef::Null);
    assert!(matches!(list.edit_type, TypeRef::Named { index: 0, .. }));
    assert_eq!(list.edit_type.metaobject(), Some("IntMeasurement"));
    assert_eq!((list.properties, list.next_history), (0x11, 0x22));

    let light = DirectionalLightInfo::read(&igz, index(&igz, s.light)).unwrap();
    assert_eq!(light.name, Some("_spriteLight1"));
    assert_eq!(
        (light.diffuse, light.ambient, light.specular),
        ([80, 78, 66, 255], [21, 21, 21, 255], [145, 145, 145, 255])
    );
    assert_eq!(light.multiplier, 1.0);
    assert_eq!(
        light.init_matrix,
        EulerTransform {
            position: [1.0, 2.0, 3.0],
            rotation: [-45.0, 0.0, 30.0],
        }
    );

    let pack = LanguagePackInfo::read(&igz, index(&igz, s.pack)).unwrap();
    assert_eq!(pack.info.name, Some("ENGLISH"));
    assert!(pack.info.resolve_state);
    assert_eq!(pack.lang_id, 6);
    assert_eq!(
        pack.string_dict.unwrap().data,
        [0, 0x61, 0, 0, 0, 0x62, 0, 0]
    );
}

#[test]
fn a_portrait_sprite_reads_back_to_its_image() {
    let (s, bytes) = scripted();
    let igz = Igz::parse(&bytes).unwrap();
    let sprite = SpriteInfo::read(&igz, index(&igz, s.sprite)).unwrap();
    let p = &sprite.placement;
    assert_eq!(p.name, Some("Collection_MinionIcon_009"));
    assert!(matches!(p.tag_list, ObjectRef::Named { .. }));
    assert_eq!((p.flag_pool, p.id), (0x8000_0001, 42));
    assert_eq!(p.init_matrix.position, [1.0, 2.0, 3.0]);
    assert_eq!(p.init_matrix.rotation, [4.0, 5.0, 6.0]);
    assert_eq!(p.current_matrix.rotation, [10.0, 11.0, 12.0]);
    assert_eq!(p.attach_xform.position, [13.0, 14.0, 15.0]);
    assert_eq!((p.activity_state, p.move_index), (5, -1));
    assert_eq!(
        p.attached_objects,
        ObjectRef::Object(index(&igz, s.string_info))
    );
    assert!(matches!(p.reset_template, ObjectRef::Import(_)));
    assert_eq!(p.clones, ObjectRef::Null);
    assert_eq!(p.active_time_layer, 9);
    assert_eq!(
        (sprite.flags, sprite.position, sprite.properties),
        (0x1800_0000, [-16, 32], 0x2001_4408)
    );
    assert_eq!(
        (sprite.z_depth, sprite.priority, sprite.width, sprite.height),
        (0.5, -3.0, 256.0, 128.0)
    );
    assert_eq!(sprite.scale, [1.0, 2.0, 3.0]);
    assert_eq!(
        (sprite.color, sprite.replace_color),
        ([0x80, 0x81, 0x82, 0xFF], [1, 2, 3, 0])
    );
    assert_eq!(
        sprite.string_info,
        ObjectRef::Object(index(&igz, s.string_info))
    );
    assert_eq!((sprite.num_value, sprite.depth), (77, 100.0));
    assert_eq!(sprite.game_trans, ObjectRef::Null);

    let data_index = sprite.texture_data(&igz).unwrap();
    assert_eq!(data_index, index(&igz, s.texture_data));
    let data = AnimatableTextureDataInfo::read(&igz, data_index).unwrap();
    assert_eq!(data.data_info.info.name, Some("portraits/eruptor.png"));
    assert_eq!(data.data_info.stream_archive_hash, 0x77);
    assert_eq!(
        (data.desired_width, data.desired_height, data.updated_count),
        (256, 128, 3)
    );
    assert_eq!(data.loader, ObjectRef::Null);
    assert_eq!(data.tex_attr, Some(index(&igz, s.bind)));
    let bind = AlphaTextureUnitIdAttr::read(&igz, data.tex_attr.unwrap()).unwrap();
    let texture = TextureAttr2::read(&igz, bind.bind.texture.unwrap()).unwrap();
    assert_eq!(texture.image, Some(index(&igz, s.image)));
    // A text sprite's context isn't a texture.
    let (mut b, sprite) = one("tfbSpriteInfo", 0xE4);
    let key = b.object(0, "StringInfo", 0x28);
    b.pointer(at(sprite, 0xD0), key);
    let bytes = b.build();
    let igz = Igz::parse(&bytes).unwrap();
    assert_eq!(SpriteInfo::read(&igz, 0).unwrap().texture_data(&igz), None);
}

#[test]
fn texture_data_binds_only_a_texture_bind() {
    let (mut b, data) = one("AnimatableTextureDataInfo", 0x34);
    b.null_memory_ref(at(data, 0x14), 0);
    let key = b.object(0, "StringInfo", 0x28);
    b.pointer(at(data, 0x30), key);
    let bytes = b.build();
    let igz = Igz::parse(&bytes).unwrap();
    assert_eq!(
        wrong_type(AnimatableTextureDataInfo::read(&igz, 0).unwrap_err()),
        ("igTextureBindAttr2", "StringInfo".into())
    );
}

/// Every roster layout (AE.1): its registry size, its memory refs (a
/// zeroed object needs them marked null) and its reader.
type Reader = fn(&Igz<'_>, usize) -> Result<(), FormatError>;
const ROSTER_LAYOUTS: [(&str, u32, &[u32], Reader); 8] = [
    ("StringInfo", 0x28, &[], |i, o| {
        StringInfo::read(i, o).map(drop)
    }),
    ("ValueInfo", 0x24, &[], |i, o| {
        ValueInfo::read(i, o).map(drop)
    }),
    ("ScriptSet", 0x1C, &[], |i, o| {
        ScriptSet::read(i, o).map(drop)
    }),
    ("ScriptSetList", 0x24, &[0x10], |i, o| {
        ScriptSetList::read(i, o).map(drop)
    }),
    ("DirectionalLightInfo", 0x40, &[], |i, o| {
        DirectionalLightInfo::read(i, o).map(drop)
    }),
    ("LanguagePackInfo", 0x20, &[0x18], |i, o| {
        LanguagePackInfo::read(i, o).map(drop)
    }),
    ("tfbSpriteInfo", 0xE4, &[], |i, o| {
        SpriteInfo::read(i, o).map(drop)
    }),
    ("AnimatableTextureDataInfo", 0x34, &[0x14], |i, o| {
        AnimatableTextureDataInfo::read(i, o).map(drop)
    }),
];

#[test]
fn each_roster_layout_refuses_a_wrong_mtsz() {
    for (ty, size, nulls, read) in ROSTER_LAYOUTS {
        assert_eq!(type_def(ty).unwrap().size, size, "{ty}");
        let zeroed = |mtsz| {
            let (mut b, o) = one(ty, mtsz);
            // The MTSZ check comes first, so a short object skips them.
            for &offset in nulls.iter().filter(|&&n| n + 8 <= mtsz) {
                b.null_memory_ref(at(o, offset), 0);
            }
            b.build()
        };
        // A zeroed object at the registry size reads.
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
fn a_type_field_refuses_a_string() {
    let (mut b, value) = one("ValueInfo", 0x24);
    b.string(at(value, 0x1C), "IntMeasurement");
    let bytes = b.build();
    let igz = Igz::parse(&bytes).unwrap();
    let e = ValueInfo::read(&igz, 0).unwrap_err();
    assert_eq!(e.field, "_type");
}

#[test]
fn an_unflagged_index_past_exnm_names_nothing() {
    let (mut b, value) = one("ValueInfo", 0x24);
    b.unflagged_meta(at(value, 0x1C), 9);
    let bytes = b.build();
    let igz = Igz::parse(&bytes).unwrap();
    let v = ValueInfo::read(&igz, 0).unwrap();
    assert_eq!(
        v.value_type,
        TypeRef::Indexed {
            index: 9,
            name: None
        }
    );
    assert_eq!(v.value_type.metaobject(), None);
}

/// The toy table's op and stack layouts (AE.2): registry size, memory refs
/// a zeroed object needs marked null, reader.
const OP_LAYOUTS: [(&str, u32, &[u32], Reader); 14] = [
    ("tfbScriptInfo", 0x1C, &[], |i, o| {
        ScriptInfo::read(i, o).map(drop)
    }),
    ("OpCodeList", 0x18, &[0x10], |i, o| {
        OpCodeList::read(i, o).map(drop)
    }),
    ("OpUseMacro", 0x24, &[], |i, o| {
        OpUseMacro::read(i, o).map(drop)
    }),
    ("OpMacroInterface", 0x20, &[], |i, o| {
        OpMacroInterface::read(i, o).map(drop)
    }),
    ("OpDefineMacro", 0x38, &[], |i, o| {
        OpDefineMacro::read(i, o).map(drop)
    }),
    ("OpCreateVariable", 0x2C, &[], |i, o| {
        Variable::read(i, o).map(drop)
    }),
    ("OpMacroParameter", 0x48, &[], |i, o| {
        OpMacroParameter::read(i, o).map(drop)
    }),
    ("OpChangeMembership", 0x28, &[], |i, o| {
        OpChangeMembership::read(i, o).map(drop)
    }),
    ("OpLoopValue", 0x28, &[], |i, o| {
        OpLoopValue::read(i, o).map(drop)
    }),
    ("SetStack", 0x18, &[0x10], |i, o| {
        Stack::read(i, o).map(drop)
    }),
    ("ScriptGroupStack", 0x1C, &[0x10], |i, o| {
        Stack::read(i, o).map(drop)
    }),
    ("RHSValueStack", 0x28, &[0x10], |i, o| {
        RhsValueStack::read(i, o).map(drop)
    }),
    ("ValueRHSVariant", 0x1C, &[], |i, o| {
        ValueRhsVariant::read(i, o).map(drop)
    }),
    ("OpFlowMacro", 0x1C, &[], |i, o| {
        OpCode::read(i, o).map(drop)
    }),
];

#[test]
fn each_op_layout_refuses_a_wrong_mtsz() {
    for (ty, size, nulls, read) in OP_LAYOUTS {
        assert_eq!(type_def(ty).unwrap().size, size, "{ty}");
        let zeroed = |mtsz| {
            let (mut b, o) = one(ty, mtsz);
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
            assert_eq!(e.field, "MTSZ", "{ty} at {wrong:#x}");
        }
    }
}

#[test]
fn an_rhs_reference_stack_reads_at_its_files_mtsz_with_a_clean_pad() {
    let stack = |mtsz, pad_marker: bool| {
        let (mut b, o) = one("RHSReferenceStack", mtsz);
        b.null_memory_ref(at(o, 0x10), 0);
        b.put_u32(at(o, 0x20), 7);
        if mtsz == 0x30 {
            if pad_marker {
                b.pointer(at(o, 0x2C), o);
            } else {
                b.put_u32(at(o, 0x2C), 0xE82A_A02A);
            }
        }
        b.build()
    };
    // The file's 0x30, its last word an unmarked value: reads.
    let bytes = stack(0x30, false);
    let igz = Igz::parse(&bytes).unwrap();
    let r = RhsReferenceStack::read(&igz, 0).unwrap();
    assert_eq!((r.value.value, r.value.stack.path.len()), (7, 0));
    // Through its parent's layout too.
    assert_eq!(RhsValueStack::read(&igz, 0).unwrap().value, 7);
    assert_eq!(file_size("RHSReferenceStack"), Some(0x30));
    assert_eq!(file_size("RHSValueStack"), Some(0x28));
    // The registry's 0x2C is not what files carry.
    let bytes = stack(0x2C, false);
    let e = RhsReferenceStack::read(&Igz::parse(&bytes).unwrap(), 0).unwrap_err();
    assert_eq!(e.field, "MTSZ");
    // A fixup in the pad is refused.
    let bytes = stack(0x30, true);
    let e = RhsReferenceStack::read(&Igz::parse(&bytes).unwrap(), 0).unwrap_err();
    assert_eq!(e.kind, ErrorKind::Unsupported { value: 0x2C });
}

#[test]
fn an_op_reads_its_block_and_a_use_its_macro() {
    let (mut b, call) = one("OpUseMacro", 0x24);
    let np = b.object(0, "ScriptGroupStack", 0x1C);
    let def = b.object(0, "OpDefineMacroSpecialization", 0x40);
    let blocks = b.section(0x10);
    b.string(at(call, 0x08), "use macro");
    b.put_u32(at(call, 0x18), 0x0001_000C);
    b.pointer(at(call, 0x1C), np);
    super::builder::list(&mut b, np, blocks, &[Some(def)]);
    b.string(at(def, 0x08), "Portal - Define Object Level Load");
    b.put_u32(at(def, 0x20), 0x01C9_F90B);
    b.put_u32(at(def, 0x24), 0x8FAE_96B0);
    // A specialization's base, past the definition's layout.
    b.pointer(at(def, 0x38), def);
    let bytes = b.build();
    let igz = Igz::parse(&bytes).unwrap();
    let u = OpUseMacro::read(&igz, 0).unwrap();
    assert_eq!(
        (u.op.name, u.op.flags, u.op.branch),
        (Some("use macro"), 1, 12)
    );
    assert_eq!(u.op.block_end(5), Some(17));
    assert_eq!(u.np, Some(index(&igz, np)));
    let path = Stack::read(&igz, u.np.unwrap()).unwrap().path;
    assert_eq!(path, [ObjectRef::Object(index(&igz, def))]);
    let d = OpDefineMacro::read(&igz, index(&igz, def)).unwrap();
    assert_eq!(d.op.name, Some("Portal - Define Object Level Load"));
    assert_eq!(d.creation_handle, 0x01C9_F90B_8FAE_96B0);
    // No block, and `flow end`'s 0xFFFF, open nothing.
    let op = |branch| OpCode {
        name: None,
        flags: 0,
        branch,
    };
    assert_eq!(op(0).block_end(3), None);
    assert_eq!(op(BRANCH_END).block_end(3), None);
}
