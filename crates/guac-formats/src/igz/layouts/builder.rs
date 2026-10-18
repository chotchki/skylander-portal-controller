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
        ("J_root", -1, -1, [0.0, 0.0, 0.0]),
        ("J_hip", 0, 0, [0.0, 1.5, 0.0]),
        ("J_head", 1, 1, [0.25, 0.5, -0.125]),
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
    // A script field in the skipped range.
    b.string(at(clip_info, 0x0C), "script");
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
    u16_be(&mut b, image, 0x30, &[8, 4, 1, 2, 1]);
    b.import(at(image, 0x3C), super::METAIMAGES, "dxt5_tile_big_ps3");
    b.put_u32(at(image, 0x40), 2);
    b.handle(at(image, 0x48), blocks, &[0xAB; 128], 5);
    b.put_u32(at(image, 0x50), u32::MAX);
    b.put_bytes(at(image, 0x58), &[1]);

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
    }
}

/// Seeds for the property test and the fuzz corpus.
pub fn seeds() -> Vec<Vec<u8>> {
    vec![sample().bytes]
}
