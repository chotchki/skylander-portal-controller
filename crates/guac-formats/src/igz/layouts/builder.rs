//! Writes a small hero in code for tests and fuzz seeds (`test-fixtures`):
//! every gate type once, each field a known value, at the offsets AC.4's
//! tables give, written as literals so a wrong reader constant shows.

use crate::igz::Location;
use crate::igz::builder::IgzBuilder;

pub use super::tfb::{HAVOK_MAGIC, LIP_SYNC_MAGIC};

/// Where [`sample`] put each object. Indexes follow creation order.
#[derive(Debug, Clone)]
pub struct Sample {
    pub bytes: Vec<u8>,
    pub model: Location,
    pub body: Location,
    pub actor: Location,
    pub actor_children: Location,
    pub select: Location,
    pub palette: Location,
    pub select_children: Location,
    pub techniques: [Location; 2],
    pub attrs: [Location; 2],
    pub geometries: [Location; 2],
    pub segments: [Location; 3],
    pub combiner: Location,
    pub skeleton: Location,
    pub bone_list: Location,
    pub bones: [Location; 3],
    pub clip_table: Location,
    pub clip_info: Location,
    pub idle: Location,
    pub lip_sync: Location,
    pub image: Location,
    /// Each technique's `_materialPass`.
    pub materials: [Location; 2],
    /// Units 0 (shared by both materials), 4 and 3.
    pub binds: [Location; 3],
    /// Colour (with the image) and a null-image normal map.
    pub textures: [Location; 2],
    /// An attribute outside the gate, in material 0.
    pub shader_mask: Location,
}

/// The tag byte tells the skeleton's payload from the clip's.
pub fn havok_bytes(tag: u8) -> Vec<u8> {
    [&HAVOK_MAGIC[..], &[tag; 8]].concat()
}

pub fn lip_sync_bytes() -> Vec<u8> {
    [&LIP_SYNC_MAGIC[..], &[0x11; 12]].concat()
}

/// Inverse joint `i`: element `(r, c)` is `i * 16 + r * 4 + c`.
pub fn inverse_joint(i: usize) -> [[f32; 4]; 4] {
    std::array::from_fn(|r| std::array::from_fn(|c| (i * 16 + r * 4 + c) as f32))
}

fn at(base: Location, offset: u32) -> Location {
    base.offset_by(offset).expect("inside the section")
}

fn u16_be(b: &mut IgzBuilder, base: Location, offset: u32, values: &[u16]) {
    let bytes: Vec<u8> = values.iter().flat_map(|v| v.to_be_bytes()).collect();
    b.put_bytes(at(base, offset), &bytes);
}

fn floats(b: &mut IgzBuilder, base: Location, offset: u32, values: &[f32]) {
    let bytes: Vec<u8> = values.iter().flat_map(|v| v.to_be_bytes()).collect();
    b.put_bytes(at(base, offset), &bytes);
}

/// An `igDataList` at `list`: count, capacity and a memory ref to the
/// element pointers (`None` stays an unmarked zero).
pub fn list(b: &mut IgzBuilder, list: Location, blocks: u8, elements: &[Option<Location>]) {
    let n = elements.len() as u32;
    b.put_u32(at(list, 0x08), n);
    b.put_u32(at(list, 0x0C), n);
    if elements.is_empty() {
        b.null_memory_ref(at(list, 0x10), 0);
        return;
    }
    let data = b.memory_ref(at(list, 0x10), blocks, &vec![0; 4 * elements.len()], 0);
    for (i, e) in elements.iter().enumerate() {
        if let Some(target) = e {
            b.pointer(at(data, 4 * i as u32), *target);
        }
    }
}

/// `igNode`'s fields: name, null bound and parent list, weights -2 and 3,
/// handle -1, flags 0x40.
fn node(b: &mut IgzBuilder, o: Location, name: &str) {
    b.string(at(o, 0x08), name);
    b.put_bytes(at(o, 0x14), &[0xFF, 0xFE, 0x00, 0x03]);
    b.put_u32(at(o, 0x18), u32::MAX);
    b.put_u32(at(o, 0x1C), 0x40);
}

/// A segment with every field set; the skinned one also has stream 1 and
/// skin weights, plus a two-entry `_blendShapes` vector.
fn segment(b: &mut IgzBuilder, o: Location, blocks: u8, skinned: bool) {
    let mut config = [0u8; 16];
    config[8..10].copy_from_slice(&84u16.to_be_bytes());
    config[10..12].copy_from_slice(&360u16.to_be_bytes());
    b.memory_ref(at(o, 0x08), blocks, &config, 2);
    b.memory_ref(at(o, 0x10), blocks, &[1, 2, 3, 4, 5, 6], 5);
    u16_be(b, o, 0x18, &[6, 0]);
    b.memory_ref(at(o, 0x1C), blocks, &[7; 12], 2);
    if skinned {
        b.memory_ref(at(o, 0x24), blocks, &[8; 8], 2);
        u16_be(b, o, 0x2C, &[4, 4, 4, 2, 2, 4]);
    } else {
        b.null_memory_ref(at(o, 0x24), 0);
        u16_be(b, o, 0x2C, &[4, 4, 4, 0, 0, 0]);
    }
    b.memory_ref(at(o, 0x38), blocks, &[9; 10], 0);
    b.put_u32(at(o, 0x40), 10);
    u16_be(b, o, 0x44, &[0, 96]);
    u16_be(b, o, 0x48, &[48, 96]);
    if skinned {
        u16_be(b, o, 0x4C, &[8, 8]);
        b.memory_ref(at(o, 0x50), blocks, &[10; 16], 2);
    } else {
        b.null_memory_ref(at(o, 0x50), 0);
    }
    b.put_u32(at(o, 0x58), 0x1000);
    b.put_u32(at(o, 0x5C), 0x200);
    b.memory_ref(at(o, 0x60), blocks, &[11; 8], 2);
    b.null_memory_ref(at(o, 0x68), 0);
    b.memory_ref(at(o, 0x70), blocks, &[12; 16], 2);
    b.memory_ref(at(o, 0x78), blocks, &[13; 24], 2);
    u16_be(b, o, 0x80, &[8, 0]);
    u16_be(b, o, 0x84, &[16, 24]);
    b.null_memory_ref(at(o, 0x90), 0);
    b.null_memory_ref(at(o, 0x98), 0);
    if skinned {
        b.put_u32(at(o, 0xA0), 2);
        b.memory_ref(at(o, 0xA4), blocks, &[14; 8], 0);
    } else {
        b.null_memory_ref(at(o, 0xA4), 0);
    }
}

/// `tfbInfo`'s fields: name, null directory, resolve state 1.
fn info(b: &mut IgzBuilder, o: Location, name: &str) {
    b.string(at(o, 0x08), name);
    b.put_bytes(at(o, 0x10), &[1]);
}

pub fn sample() -> Sample {
    let mut b = IgzBuilder::new();
    let model = b.object(0, "tfbPhysicsModel", 0x30);
    let body = b.object(0, "tfbPhysicsBody", 0x40);
    let actor = b.object(0, "igActor2", 0x28);
    let actor_children = b.object(0, "igNodeList", 0x18);
    let select = b.object(0, "igBlendMatrixSelect", 0xC0);
    let palette = b.object(0, "igIntList", 0x18);
    let select_children = b.object(0, "igNodeList", 0x18);
    let techniques = [(); 2].map(|_| b.object(0, "tfbRuntimeTechniqueInstance", 0x38));
    let attrs = [(); 2].map(|_| b.object(0, "igEdgeGeometryAttr", 0x14));
    let geometries = [(); 2].map(|_| b.object(0, "igPS3EdgeGeometry", 0x1C));
    let segments = [(); 3].map(|_| b.object(0, "igPS3EdgeGeometrySegment", 0xB0));
    let combiner = b.object(0, "tfbPhysicsCombinerLink", 0x3C);
    let skeleton = b.object(0, "igSkeleton2", 0x18);
    let bone_list = b.object(0, "igSkeletonBoneList", 0x18);
    let bones = [(); 3].map(|_| b.object(0, "igSkeletonBone", 0x20));
    let clip_table = b.object(0, "AnimationInfoList", 0x18);
    let clip_info = b.object(0, "AnimationInfo", 0x44);
    let idle = b.object(0, "tfbPhysicsAnimation", 0x2C);
    let lip_sync = b.object(0, "tfbPhysicsAnimation", 0x2C);
    let image = b.object(0, "igImage2", 0x60);
    let materials = [(); 2].map(|_| b.object(0, "igAttrList", 0x18));
    let binds = [(); 3].map(|_| b.object(0, "tfbAlphaTextureUnitIDAttr", 0x20));
    let textures = [(); 2].map(|_| b.object(0, "igTextureAttr2", 0x40));
    let shader_mask = b.object(0, "igPixelShaderVariantMaskAttr", 0x18);
    let blocks = b.section(0x80);

    // tfbPhysicsModel.
    info(&mut b, model, "Eruptor.mdl");
    b.pointer(at(model, 0x14), body);
    b.import(at(model, 0x18), "models", "ActorParameters");
    b.pointer(at(model, 0x1C), clip_table);
    b.meta_object(at(model, 0x24), "objects", "EruptorAttachList");
    b.string(at(model, 0x28), "templates/eruptor");
    b.put_u32(at(model, 0x2C), 0x1234);

    // tfbPhysicsBody.
    info(&mut b, body, "Eruptor.ma");
    b.memory_ref(at(body, 0x14), blocks, &havok_bytes(0xA5), 2);
    b.put_u32(at(body, 0x1C), (-7i32) as u32);
    b.pointer(at(body, 0x20), actor);
    b.pointer(at(body, 0x24), combiner);
    b.null_memory_ref(at(body, 0x28), 0);
    b.pointer(at(body, 0x34), lip_sync);
    b.pointer(at(body, 0x38), idle);
    b.put_u32(at(body, 0x3C), 0x55);

    // The scene graph: actor -> select -> two techniques.
    node(&mut b, actor, "J_root");
    b.pointer(at(actor, 0x20), actor_children);
    b.pointer(at(actor, 0x24), combiner);
    list(&mut b, actor_children, blocks, &[Some(select)]);
    node(&mut b, select, "skin");
    b.pointer(at(select, 0x20), select_children);
    let transform: Vec<f32> = (1..=16).map(|i| i as f32).collect();
    let inverse: Vec<f32> = (17..=32).map(|i| i as f32).collect();
    floats(&mut b, select, 0x30, &transform);
    floats(&mut b, select, 0x70, &inverse);
    b.put_bytes(at(select, 0xB0), &[1]);
    b.pointer(at(select, 0xB4), palette);
    b.put_u32(at(palette, 0x08), 2);
    b.put_u32(at(palette, 0x0C), 2);
    let ints: Vec<u8> = [1i32, 0].iter().flat_map(|v| v.to_be_bytes()).collect();
    b.memory_ref(at(palette, 0x10), blocks, &ints, 0);
    list(
        &mut b,
        select_children,
        blocks,
        &[Some(techniques[0]), Some(techniques[1])],
    );
    for (i, name) in ["EruptorBody", "EruptorEyes_Teeth"].into_iter().enumerate() {
        let t = techniques[i];
        node(&mut b, t, name);
        b.put_u32(at(t, 0x20), 0x11);
        b.put_u32(at(t, 0x24), 0x22);
        b.pointer(at(t, 0x28), attrs[i]);
        b.pointer(at(t, 0x30), materials[i]);
        b.import(at(t, 0x34), "effects", "skin_fx");
        b.put_bytes(at(attrs[i], 0x0A), &[0xFF, 0xFF]);
        b.pointer(at(attrs[i], 0x10), geometries[i]);
        b.put_u32(at(geometries[i], 0x18), 0x00C0_FFEE + i as u32);
    }
    list(
        &mut b,
        geometries[0],
        blocks,
        &[Some(segments[0]), Some(segments[1])],
    );
    list(&mut b, geometries[1], blocks, &[Some(segments[2])]);
    segment(&mut b, segments[0], blocks, true);
    segment(&mut b, segments[1], blocks, true);
    segment(&mut b, segments[2], blocks, false);

    // tfbPhysicsCombinerLink -> igSkeleton2 -> three bones.
    b.string(at(combiner, 0x08), "Eruptor_combiner");
    b.pointer(at(combiner, 0x0C), skeleton);
    b.null_memory_ref(at(combiner, 0x10), 0);
    b.null_memory_ref(at(combiner, 0x18), 0);
    b.put_u32(at(combiner, 0x20), 0x8000_0000);
    b.null_memory_ref(at(combiner, 0x24), 0);
    b.null_memory_ref(at(combiner, 0x2C), 0);
    b.put_u32(at(combiner, 0x34), 0xAA);
    b.put_u32(at(combiner, 0x38), 0xBB);
    b.string(at(skeleton, 0x08), "DefaultSkeleton_0");
    b.pointer(at(skeleton, 0x0C), bone_list);
    let joints: Vec<u8> = (0..2)
        .flat_map(|i| inverse_joint(i).into_iter().flatten())
        .flat_map(f32::to_be_bytes)
        .collect();
    b.memory_ref(at(skeleton, 0x10), blocks, &joints, 2);
    list(&mut b, bone_list, blocks, &bones.map(Some));
    let bone_values = [
        ("J_root", -1, 1, [0.0, 0.0, 0.0]),
        ("J_hip", 0, -1, [0.0, 1.5, 0.0]),
        ("J_head", 1, 0, [0.25, 0.5, -0.125]),
    ];
    for (o, (name, parent, blend, t)) in bones.into_iter().zip(bone_values) {
        b.string(at(o, 0x08), name);
        b.put_u32(at(o, 0x0C), parent as u32);
        b.put_u32(at(o, 0x10), blend as u32);
        floats(&mut b, o, 0x14, &t);
    }

    // Giants' clip tables have empty slots.
    list(&mut b, clip_table, blocks, &[Some(clip_info), None]);
    b.string(at(clip_info, 0x08), "Idle");
    b.put_u32(at(clip_info, 0x30), 0x610);
    b.string(at(clip_info, 0x34), "Eruptor_Idle");
    floats(&mut b, clip_info, 0x38, &[0.5]);
    b.pointer(at(clip_info, 0x3C), idle);
    info(&mut b, idle, "Eruptor_Idle.ma");
    b.memory_ref(at(idle, 0x14), blocks, &havok_bytes(0x5A), 2);
    b.put_u32(at(idle, 0x20), 0x610);
    b.put_u32(at(idle, 0x24), 7);
    info(&mut b, lip_sync, "vo_eruptor_01");
    b.memory_ref(at(lip_sync, 0x14), blocks, &lip_sync_bytes(), 2);
    b.put_u32(at(lip_sync, 0x20), 0x200);
    b.put_u32(at(lip_sync, 0x24), (-1i32) as u32);

    // igImage2, 8x4 DXT5 with two mips.
    b.string(at(image, 0x08), "eruptor_body");
    floats(&mut b, image, 0x10, &[1.0, 0.5, 0.25, 2.0]);
    floats(&mut b, image, 0x20, &[0.125, 0.0, -1.0, 0.0]);
    u16_be(&mut b, image, 0x30, &[8, 4, 1, 2, 6]);
    b.import(at(image, 0x3C), super::METAIMAGES, "dxt5_tile_big_ps3");
    b.put_u32(at(image, 0x40), 2);
    b.handle(at(image, 0x48), blocks, &[0xAB; 128], 5);
    b.put_u32(at(image, 0x50), u32::MAX);
    b.put_bytes(at(image, 0x58), &[1]);

    // Materials: binds (unit, texture, `_unitID`, `_data`), then textures.
    list(
        &mut b,
        materials[0],
        blocks,
        &[Some(binds[0]), Some(binds[1]), Some(shader_mask)],
    );
    list(
        &mut b,
        materials[1],
        blocks,
        &[Some(binds[2]), Some(binds[0])],
    );
    for (o, (unit, texture, unit_id, data)) in binds.into_iter().zip([
        (0u16, textures[0], -2i32, -3i32),
        (4, textures[1], -4, -5),
        (3, textures[0], -6, -7),
    ]) {
        u16_be(&mut b, o, 0x08, &[unit, 0xFFFF]);
        b.pointer(at(o, 0x10), texture);
        b.put_u32(at(o, 0x14), unit_id as u32);
        b.put_u32(at(o, 0x1C), data as u32);
    }
    b.pointer(at(binds[1], 0x18), textures[1]);
    for (i, o) in textures.into_iter().enumerate() {
        let i = i as u32;
        u16_be(&mut b, o, 0x08, &[0xFFFF, 2 + i as u16]);
        b.put_u32(at(o, 0x10), 0x5446_4E31 + i);
        b.put_bytes(at(o, 0x14), &[1]);
        for (k, offset) in [0x18, 0x1C, 0x20, 0x24, 0x28, 0x2C].into_iter().enumerate() {
            b.put_u32(at(o, offset), 0x10 * i + 1 + k as u32);
        }
        b.put_u32(at(o, 0x34), u32::MAX - i);
        b.put_u32(at(o, 0x38), 0x0A + i);
        b.put_u32(at(o, 0x3C), 0x0B + i);
    }
    b.pointer(at(textures[0], 0x30), image);
    u16_be(&mut b, shader_mask, 0x10, &[0, 0x1C]);

    Sample {
        bytes: b.build(),
        model,
        body,
        actor,
        actor_children,
        select,
        palette,
        select_children,
        techniques,
        attrs,
        geometries,
        segments,
        combiner,
        skeleton,
        bone_list,
        bones,
        clip_table,
        clip_info,
        idle,
        lip_sync,
        image,
        materials,
        binds,
        textures,
        shader_mask,
    }
}

/// The least hero [`Hero::read`](super::Hero::read) takes: model, body,
/// actor, combiner and a boneless skeleton. `children(b, blocks, actor,
/// list)` returns the actor's children; `list` is the actor's child list.
pub fn bare_hero(
    children: impl FnOnce(&mut IgzBuilder, u8, Location, Location) -> Vec<Location>,
) -> Vec<u8> {
    let mut b = IgzBuilder::new();
    let blocks = b.section(0x10);
    let model = b.object(0, "tfbPhysicsModel", 0x30);
    let body = b.object(0, "tfbPhysicsBody", 0x40);
    let actor = b.object(0, "igActor2", 0x28);
    let actor_children = b.object(0, "igNodeList", 0x18);
    let combiner = b.object(0, "tfbPhysicsCombinerLink", 0x3C);
    let skeleton = b.object(0, "igSkeleton2", 0x18);
    b.pointer(at(model, 0x14), body);
    b.pointer(at(body, 0x20), actor);
    b.pointer(at(body, 0x24), combiner);
    b.pointer(at(actor, 0x20), actor_children);
    b.pointer(at(combiner, 0x0C), skeleton);
    for (o, off) in [
        (body, 0x14),
        (body, 0x28),
        (combiner, 0x10),
        (combiner, 0x18),
        (combiner, 0x24),
        (combiner, 0x2C),
        (skeleton, 0x10),
    ] {
        b.null_memory_ref(at(o, off), 0);
    }
    let nodes: Vec<_> = children(&mut b, blocks, actor, actor_children)
        .into_iter()
        .map(Some)
        .collect();
    list(&mut b, actor_children, blocks, &nodes);
    b.build()
}

/// `groups` igGroups under the actor, each with the actor's child list as
/// its own: quadratic for a walk that re-reads a shared list.
pub fn shared_child_list(groups: usize) -> Vec<u8> {
    bare_hero(|b, _, _, list| {
        let gs: Vec<_> = (0..groups).map(|_| b.object(0, "igGroup", 0x24)).collect();
        for &g in &gs {
            b.pointer(at(g, 0x20), list);
        }
        gs
    })
}

/// `selects` igBlendMatrixSelects sharing one `ints`-entry igIntList.
pub fn shared_palette(selects: usize, ints: usize) -> Vec<u8> {
    bare_hero(|b, blocks, _, _| {
        let palette = b.object(0, "igIntList", 0x18);
        b.put_u32(at(palette, 0x08), ints as u32);
        b.put_u32(at(palette, 0x0C), ints as u32);
        b.memory_ref(at(palette, 0x10), blocks, &vec![0; 4 * ints], 0);
        let ss: Vec<_> = (0..selects)
            .map(|_| b.object(0, "igBlendMatrixSelect", 0xC0))
            .collect();
        for &s in &ss {
            b.pointer(at(s, 0xB4), palette);
        }
        ss
    })
}

/// `techniques` techniques under the actor sharing one empty geometry and
/// one material of `binds` binds, all binding `bind_type` (a
/// `tfbAlphaTextureUnitIDAttr` or not) to one texture.
pub fn shared_material(techniques: usize, binds: usize, bind_type: &str) -> Vec<u8> {
    bare_hero(|b, blocks, _, _| {
        let attr = b.object(0, "igEdgeGeometryAttr", 0x14);
        let geometry = b.object(0, "igPS3EdgeGeometry", 0x1C);
        let material = b.object(0, "igAttrList", 0x18);
        let texture = b.object(0, "igTextureAttr2", 0x40);
        b.pointer(at(attr, 0x10), geometry);
        list(b, geometry, blocks, &[]);
        let size = if bind_type == "tfbAlphaTextureUnitIDAttr" {
            0x20
        } else {
            0x18
        };
        let bs: Vec<_> = (0..binds)
            .map(|_| {
                let o = b.object(0, bind_type, size);
                b.pointer(at(o, 0x10), texture);
                Some(o)
            })
            .collect();
        list(b, material, blocks, &bs);
        (0..techniques)
            .map(|_| {
                let t = b.object(0, "tfbRuntimeTechniqueInstance", 0x38);
                b.pointer(at(t, 0x28), attr);
                b.pointer(at(t, 0x30), material);
                t
            })
            .collect()
    })
}

/// Where [`script_sample`] put each roster object (AE.1).
#[derive(Debug, Clone)]
pub struct ScriptSample {
    pub bytes: Vec<u8>,
    pub string_info: Location,
    pub value_info: Location,
    pub set: Location,
    pub set_list: Location,
    pub light: Location,
    pub pack: Location,
    pub sprite: Location,
    pub texture_data: Location,
    pub bind: Location,
    pub texture: Location,
    pub image: Location,
}

/// One of each roster layout (AE.1), every field a distinct known value,
/// plus the bind, texture and image a portrait sprite reaches.
pub fn script_sample() -> ScriptSample {
    let mut b = IgzBuilder::new();
    let string_info = b.object(0, "StringInfo", 0x28);
    let value_info = b.object(0, "ValueInfo", 0x24);
    let set = b.object(0, "ScriptSet", 0x1C);
    let set_list = b.object(0, "ScriptSetList", 0x24);
    let light = b.object(0, "DirectionalLightInfo", 0x40);
    let pack = b.object(0, "LanguagePackInfo", 0x20);
    let sprite = b.object(0, "tfbSpriteInfo", 0xE4);
    let texture_data = b.object(0, "AnimatableTextureDataInfo", 0x34);
    let bind = b.object(0, "tfbAlphaTextureUnitIDAttr", 0x20);
    let texture = b.object(0, "igTextureAttr2", 0x40);
    let image = b.object(0, "igImage2", 0x60);
    let blocks = b.section(0x10);

    b.string(at(string_info, 0x08), "ItemName_Anvil");
    for (offset, value) in [(0x1C, 7u32), (0x20, 3), (0x24, (-2i32) as u32)] {
        b.put_u32(at(string_info, offset), value);
    }

    // EXNM 0 is IntMeasurement (the set's flagged `_editType`); the
    // ValueInfo's `_type` indexes it unflagged, as Giants' do.
    b.string(at(set, 0x08), "Element_Values");
    b.pointer(at(set, 0x18), set_list);
    b.put_u32(at(set_list, 0x08), 3);
    b.put_u32(at(set_list, 0x0C), 3);
    let data = b.memory_ref(at(set_list, 0x10), blocks, &[0; 12], 0);
    b.pointer(data, value_info);
    b.import(at(data, 4), "global.bld", "Minion_Names");
    b.meta_object(at(set_list, 0x18), "metaobject", "IntMeasurement");
    b.put_u32(at(set_list, 0x1C), 0x11);
    b.put_u32(at(set_list, 0x20), 0x22);

    b.string(at(value_info, 0x08), "ElementIndex_Fire");
    b.put_u32(at(value_info, 0x18), (-4i32) as u32);
    b.unflagged_meta(at(value_info, 0x1C), 0);
    b.put_u32(at(value_info, 0x20), 5);

    b.string(at(light, 0x08), "_spriteLight1");
    b.put_bytes(
        at(light, 0x18),
        &[80, 78, 66, 255, 21, 21, 21, 255, 145, 145, 145, 255],
    );
    floats(&mut b, light, 0x24, &[1.0, 1.0, 2.0, 3.0, -45.0, 0.0, 30.0]);

    b.string(at(pack, 0x08), "ENGLISH");
    b.put_bytes(at(pack, 0x10), &[1]);
    b.put_u32(at(pack, 0x14), 6);
    b.memory_ref(at(pack, 0x18), blocks, &[0, 0x61, 0, 0, 0, 0x62, 0, 0], 0);

    // The sprite: placement fields, then its own.
    b.string(at(sprite, 0x08), "Collection_MinionIcon_009");
    b.meta_object(at(sprite, 0x18), "metaobject", "TagList");
    b.put_u32(at(sprite, 0x1C), 0x8000_0001);
    b.put_u32(at(sprite, 0x20), 42);
    floats(&mut b, sprite, 0x24, &[1.0, 2.0, 3.0, 4.0, 5.0, 6.0]);
    floats(&mut b, sprite, 0x3C, &[7.0, 8.0, 9.0, 10.0, 11.0, 12.0]);
    b.put_u32(at(sprite, 0x54), 5);
    b.put_u32(at(sprite, 0x68), (-1i32) as u32);
    b.pointer(at(sprite, 0x74), string_info);
    floats(&mut b, sprite, 0x78, &[13.0, 14.0, 15.0, 16.0, 17.0, 18.0]);
    b.import(at(sprite, 0x98), "global.bld", "ResetTemplate");
    b.put_u32(at(sprite, 0x9C), 9);
    b.put_u32(at(sprite, 0xA0), 0x1800_0000);
    u16_be(&mut b, sprite, 0xA4, &[0xFFF0, 0x0020]);
    b.put_u32(at(sprite, 0xA8), 0x2001_4408);
    floats(
        &mut b,
        sprite,
        0xAC,
        &[0.5, -3.0, 256.0, 128.0, 1.0, 2.0, 3.0],
    );
    b.put_bytes(at(sprite, 0xC8), &[0x80, 0x81, 0x82, 0xFF, 1, 2, 3, 0]);
    b.pointer(at(sprite, 0xD0), texture_data);
    b.pointer(at(sprite, 0xD4), string_info);
    b.put_u32(at(sprite, 0xD8), 77);
    floats(&mut b, sprite, 0xDC, &[100.0]);

    info(&mut b, texture_data, "portraits/eruptor.png");
    b.null_memory_ref(at(texture_data, 0x14), 0);
    b.put_u32(at(texture_data, 0x1C), 0x77);
    b.put_u32(at(texture_data, 0x24), 256);
    b.put_u32(at(texture_data, 0x28), 128);
    b.put_u32(at(texture_data, 0x2C), 3);
    b.pointer(at(texture_data, 0x30), bind);

    b.pointer(at(bind, 0x10), texture);
    b.put_u32(at(bind, 0x14), u32::MAX);
    b.put_u32(at(bind, 0x1C), u32::MAX);
    b.pointer(at(texture, 0x30), image);
    b.string(at(image, 0x08), "eruptor.png");
    u16_be(&mut b, image, 0x30, &[4, 4, 1, 1, 1]);
    b.import(at(image, 0x3C), super::METAIMAGES, "dxt5_big_ps3");
    b.handle(at(image, 0x48), blocks, &[0x5A; 0x80], 5);

    ScriptSample {
        bytes: b.build(),
        string_info,
        value_info,
        set,
        set_list,
        light,
        pack,
        sprite,
        texture_data,
        bind,
        texture,
        image,
    }
}

/// Seeds for the property test and the fuzz corpus; the shared shapes
/// overrun the heap budget on a walk that copies per reference.
pub fn seeds() -> Vec<Vec<u8>> {
    vec![
        sample().bytes,
        script_sample().bytes,
        shared_child_list(1000),
        shared_palette(400, 16_000),
        shared_material(400, 400, "tfbAlphaTextureUnitIDAttr"),
        // A level's materials (AG.15): its tint, scroll and multiply, and a
        // character's archive carrying every track kind.
        crate::level::builder::stage_level(&crate::level::builder::stage_sample()),
        crate::model::builder::character(&crate::model::builder::tracked()),
    ]
}
