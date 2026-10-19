//! A character archive in code, for tests and fuzz seeds (`test-fixtures`):
//! a portal sprite drawing a skinned model whose every part decodes (AC.7's
//! skinned Edge fixture, AC.8's Havok skeleton and clip, AC.6's images) and
//! whose techniques carry tfbmulti2 materials, beside a sprite drawing an
//! attachment. [`sample`] is Eruptor-shaped (a body and an eye material on
//! the normal-map, spec-map and rim variant); tests bend a [`CharacterSpec`]
//! to make the shapes the readers refuse.

use crate::edge::builder::{Fixture, segment, skinned};
use crate::fnv1a32;
use crate::havok::builder::{ClipSpec, PayloadSpec, SkeletonSpec, clip, payload, skeleton3};
use crate::iga::builder::IgaBuilder;
use crate::igz::Location;
use crate::igz::builder::IgzBuilder;
use crate::igz::layouts::builder::list;
use crate::material::{EFFECTS, TFBMULTI2, TFN1};
use crate::texture::builder::{SPECS, Spec, named_image};

fn at(base: Location, offset: u32) -> Location {
    base.offset_by(offset).expect("inside the section")
}

fn floats(b: &mut IgzBuilder, o: Location, offset: u32, values: &[f32]) {
    let bytes: Vec<u8> = values.iter().flat_map(|v| v.to_be_bytes()).collect();
    b.put_bytes(at(o, offset), &bytes);
}

/// One texture bind. Binds go in list order, which the reader takes as the
/// variant's sampler order.
#[derive(Debug, Clone, PartialEq)]
pub struct BindSpec {
    /// `igAttr._cachedUnitID`.
    pub unit: i16,
    /// `igTextureAttr2._formatHint`: [`TFN1`] on a normal map.
    pub hint: u32,
    /// Into [`CharacterSpec::images`].
    pub image: usize,
    /// Wrap S and T (0 clamp, 1 repeat) and mag and min filters.
    pub wrap: [u32; 2],
    pub filter: [u32; 2],
}

impl BindSpec {
    pub fn new(unit: i16, image: usize) -> Self {
        Self {
            unit,
            hint: 0,
            image,
            wrap: [1, 1],
            filter: [1, 5],
        }
    }

    pub fn normal_map(mut self) -> Self {
        self.hint = TFN1;
        self
    }
}

/// A material attribute other than a bind or a mask.
#[derive(Debug, Clone, PartialEq)]
pub enum AttrSpec {
    /// A `tfbMulti2*Attr` by type.
    Vec4(&'static str, [f32; 4]),
    Tint(u32),
    /// `_alpha`, `_state`.
    Glow(i32, bool),
    BlendState(bool),
    /// `_src`, `_dst`.
    BlendFunction(u32, u32),
    DepthWrite(bool),
    Cull(bool),
    AlphaState(bool),
    /// `_func`, `_refValue`.
    AlphaFunction(u32, f32),
    /// Any other type, zeroed, at its size.
    Other(&'static str, u32),
}

/// One technique: its material and the one skinned segment it draws.
#[derive(Debug, Clone, PartialEq)]
pub struct MaterialSpec {
    pub name: &'static str,
    /// `_effect`'s export id in `global.bld`.
    pub effect: u32,
    /// (base, ignore); `None` leaves the attr out.
    pub pixel: Option<(u32, u32)>,
    pub vertex: Option<(u32, u32)>,
    pub binds: Vec<BindSpec>,
    pub attrs: Vec<AttrSpec>,
    /// A scroll track's layer and its 11 floats; the type, if not a scroll.
    pub scroll: Option<(i32, [f32; 11], &'static str)>,
}

impl MaterialSpec {
    /// Eruptor's body variant (0x1C over ignores 0x2FF and 0x1AFF): normal,
    /// spec, colour and rim maps in sampler order, on images 0-3.
    pub fn body(name: &'static str) -> Self {
        Self {
            name,
            effect: TFBMULTI2,
            pixel: Some((0x1C, 0x2FF)),
            vertex: Some((0x1C, 0x1AFF)),
            binds: vec![
                BindSpec::new(4, 0).normal_map(),
                BindSpec::new(6, 1),
                BindSpec::new(0, 2),
                BindSpec::new(3, 3),
            ],
            attrs: vec![
                AttrSpec::Vec4("tfbMulti2Layer1ExtraValuesAttr", [1.0, 1.0, 1.0, 1.0]),
                AttrSpec::Vec4("tfbMulti2FixedAlphaValuesAttr", [-1.0, -1.0, -1.0, 1.0]),
                AttrSpec::Glow(64, true),
            ],
            scroll: None,
        }
    }
}

/// What [`character`] builds: the portal sprite and its model, one technique
/// per material, the images they bind, the skeleton, the idle and the
/// attachments.
#[derive(Debug, Clone, PartialEq)]
pub struct CharacterSpec {
    /// `tfbPhysicsModel`'s name.
    pub model: &'static str,
    pub portal_sprite: &'static str,
    pub materials: Vec<MaterialSpec>,
    /// Image specs and names; equal specs make byte-identical images.
    pub images: Vec<(Spec, &'static str)>,
    pub skeleton: SkeletonSpec,
    /// Slot 0's clip, with the tfbPhysicsAnimation's `_properties`.
    pub idle: Option<(ClipSpec, u32)>,
    pub idle_name: &'static str,
    /// Other sprites: their names and whether their model has a skeleton.
    pub attachments: Vec<(&'static str, bool)>,
}

/// AC.6's two decodable tiled images, as named copies.
fn spec(i: usize, seed: u8) -> Spec {
    let s = &SPECS[i];
    Spec {
        format: s.format,
        width: s.width,
        height: s.height,
        levels: s.levels,
        len: s.len,
        seed,
    }
}

/// Eruptor-shaped: a body and an eye material (the eyes' rim its own
/// image), skeleton3 with an unnamed Alchemy root, AC.8's clip as the idle,
/// a `ReposeSpikes` sprite drawing an unskinned attachment. The eye
/// material's normal, spec and colour maps are byte-identical copies of the
/// body's.
pub fn sample() -> CharacterSpec {
    let mut eyes = MaterialSpec::body("EruptorEyes_Teeth");
    eyes.binds = vec![
        BindSpec::new(4, 4).normal_map(),
        BindSpec::new(6, 5),
        BindSpec::new(0, 6),
        BindSpec::new(3, 7),
    ];
    eyes.attrs.retain(|a| !matches!(a, AttrSpec::Glow(..)));
    CharacterSpec {
        model: "Models/Characters/Eruptor/Eruptor.mdl",
        portal_sprite: "009_Portal_Toy",
        materials: vec![MaterialSpec::body("EruptorBody"), eyes],
        images: vec![
            (spec(1, 0x20), "eruptor_n"),
            (spec(0, 0x60), "eruptor_sp"),
            (spec(0, 0x10), "eruptor_c"),
            (spec(0, 0x70), "eruptorrim"),
            (spec(1, 0x20), "eruptor_n"),
            (spec(0, 0x60), "eruptor_sp"),
            (spec(0, 0x10), "eruptor_c"),
            (spec(0, 0x80), "specrim_eye6"),
        ],
        skeleton: skeleton3(),
        idle: Some((clip(), 0x200)),
        idle_name: "Models/Characters/Eruptor/Eruptor_Idle.ma",
        attachments: vec![("009_ReposeSpikes", false)],
    }
}

/// The skinned fixture's palette slots 2, 3 and 10 name blend matrices 0, 1
/// and 2: skeleton3's joints, in order, behind an unnamed root.
const PALETTE: [i32; 11] = [-1, -1, 0, 1, -1, -1, -1, -1, -1, -1, 2];

/// Inverse joint `i`: a translation of -(i + 1) along x, so each differs.
pub fn inverse_joint(i: usize) -> [[f32; 4]; 4] {
    let mut m = [[0.0; 4]; 4];
    for (k, row) in m.iter_mut().enumerate() {
        row[k] = 1.0;
    }
    m[3][0] = -(i as f32 + 1.0);
    m
}

/// `igNode`'s fields: name, weights, handle -1, flags 0x10000.
fn node(b: &mut IgzBuilder, o: Location, name: &str) {
    b.string(at(o, 0x08), name);
    b.put_u32(at(o, 0x18), u32::MAX);
    b.put_u32(at(o, 0x1C), 0x1_0000);
}

/// A `tfbInfo`'s name and resolve state.
fn info(b: &mut IgzBuilder, o: Location, name: &str) {
    b.string(at(o, 0x08), name);
    b.put_bytes(at(o, 0x10), &[1]);
}

fn attr(b: &mut IgzBuilder, blocks: u8, a: &AttrSpec) -> Location {
    let state = |b: &mut IgzBuilder, ty, size, on: bool| {
        let o = b.object(0, ty, size);
        b.put_bytes(at(o, 0x0D), &[u8::from(on)]);
        o
    };
    match *a {
        AttrSpec::Vec4(ty, v) => {
            let o = b.object(0, ty, 0x20);
            floats(b, o, 0x10, &v);
            o
        }
        AttrSpec::Tint(packed) => {
            let o = b.object(0, "tfbMaterialTintAttr", 0x14);
            b.put_u32(at(o, 0x10), packed);
            o
        }
        AttrSpec::Glow(alpha, on) => {
            let o = b.object(0, "tfbDestinationAlphaAttr", 0x18);
            b.put_u32(at(o, 0x10), alpha as u32);
            b.put_bytes(at(o, 0x14), &[u8::from(on)]);
            o
        }
        AttrSpec::BlendState(on) => {
            let o = state(b, "igBlendStateAttr", 0x14, on);
            // What files keep in a blend state's pad.
            b.put_bytes(at(o, 0x10), &[1, 0, 0, 0]);
            o
        }
        AttrSpec::BlendFunction(src, dst) => {
            let o = b.object(0, "igBlendFunctionAttr", 0x34);
            b.put_u32(at(o, 0x10), src);
            b.put_u32(at(o, 0x14), dst);
            o
        }
        AttrSpec::DepthWrite(on) => state(b, "igDepthWriteStateAttr", 0x14, on),
        AttrSpec::Cull(on) => state(b, "igCullFaceAttr", 0x18, on),
        AttrSpec::AlphaState(on) => state(b, "igAlphaStateAttr", 0x14, on),
        AttrSpec::AlphaFunction(func, reference) => {
            let o = b.object(0, "igAlphaFunctionAttr", 0x18);
            b.put_u32(at(o, 0x10), func);
            floats(b, o, 0x14, &[reference]);
            o
        }
        AttrSpec::Other(ty, size) => {
            let o = b.object(0, ty, size);
            if ty == "igAttrList" {
                list(b, o, blocks, &[]);
            }
            o
        }
    }
}

fn mask(b: &mut IgzBuilder, ty: &str, (base, ignore): (u32, u32)) -> Location {
    let o = b.object(0, ty, 0x18);
    b.put_u32(at(o, 0x10), base);
    b.put_u32(at(o, 0x14), ignore);
    o
}

/// The skinned fixture as one geometry under its own attr.
fn geometry(b: &mut IgzBuilder, blocks: u8, f: &Fixture) -> Location {
    let attr = b.object(0, "igEdgeGeometryAttr", 0x14);
    let geometry = b.object(0, "igPS3EdgeGeometry", 0x1C);
    let seg = segment(b, blocks, &f.wire);
    b.put_bytes(at(attr, 0x08), &[0xFF, 0xFF, 0xFF, 0xFF]);
    b.pointer(at(attr, 0x10), geometry);
    list(b, geometry, blocks, &[Some(seg)]);
    attr
}

fn technique(
    b: &mut IgzBuilder,
    blocks: u8,
    m: &MaterialSpec,
    images: &[Location],
    fixture: &Fixture,
) -> Location {
    let t = b.object(0, "tfbRuntimeTechniqueInstance", 0x38);
    node(b, t, m.name);
    let g = geometry(b, blocks, fixture);
    b.pointer(at(t, 0x28), g);
    b.import_raw(at(t, 0x34), m.effect, fnv1a32(EFFECTS.as_bytes()));

    let material = b.object(0, "igAttrList", 0x18);
    b.pointer(at(t, 0x30), material);
    let mut attrs = Vec::new();
    if let Some(p) = m.pixel {
        attrs.push(mask(b, "igPixelShaderVariantMaskAttr", p));
    }
    if let Some(v) = m.vertex {
        attrs.push(mask(b, "igVertexShaderVariantMaskAttr", v));
    }
    for s in &m.binds {
        let bind = b.object(0, "tfbAlphaTextureUnitIDAttr", 0x20);
        let texture = b.object(0, "igTextureAttr2", 0x40);
        b.put_bytes(
            at(bind, 0x08),
            &[(s.unit >> 8) as u8, s.unit as u8, 0xFF, 0xFF],
        );
        b.pointer(at(bind, 0x10), texture);
        b.put_u32(at(bind, 0x14), u32::MAX);
        b.put_u32(at(bind, 0x1C), u32::MAX);
        b.put_bytes(at(texture, 0x08), &[0xFF, 0xFF, 0xFF, 0xFF]);
        b.put_u32(at(texture, 0x10), s.hint);
        b.put_u32(at(texture, 0x18), s.filter[0]);
        b.put_u32(at(texture, 0x1C), s.filter[1]);
        b.put_u32(at(texture, 0x20), s.wrap[0]);
        b.put_u32(at(texture, 0x24), s.wrap[1]);
        b.pointer(at(texture, 0x30), images[s.image]);
        b.put_u32(at(texture, 0x34), u32::MAX);
        b.put_u32(at(texture, 0x3C), 1);
        attrs.push(bind);
    }
    for a in &m.attrs {
        attrs.push(attr(b, blocks, a));
    }
    let elements: Vec<Option<Location>> = attrs.into_iter().map(Some).collect();
    list(b, material, blocks, &elements);

    if let Some((layer, values, ty)) = m.scroll {
        let anim = b.object(0, "tfbMaterialAnim", 0x10);
        let tracks = b.object(0, "tfbMaterialAnimTrackList", 0x18);
        let track = b.object(0, ty, 0x28);
        let data = b.object(0, "tfbScrollUVAnimData", 0x34);
        b.pointer(at(t, 0x2C), anim);
        b.pointer(at(anim, 0x08), tracks);
        list(b, tracks, blocks, &[Some(track)]);
        b.pointer(at(track, 0x10), data);
        b.put_u32(at(track, 0x14), u32::MAX);
        b.put_u32(at(track, 0x24), layer as u32);
        floats(b, data, 0x08, &values);
    }
    t
}

/// The level's IGZ.
pub fn character(spec: &CharacterSpec) -> Vec<u8> {
    let mut b = IgzBuilder::new();
    let blocks = b.section(0x80);
    let fixture = skinned();
    let images: Vec<Location> = spec
        .images
        .iter()
        .map(|(s, name)| named_image(&mut b, blocks, s, &s.data(), name))
        .collect();

    // The portal sprite and its model.
    let sprite = b.object(0, "tfbSpriteInfo", 0xE4);
    let model = b.object(0, "tfbPhysicsModel", 0x30);
    let body = b.object(0, "tfbPhysicsBody", 0x40);
    let actor = b.object(0, "igActor2", 0x28);
    let actor_children = b.object(0, "igNodeList", 0x18);
    let select = b.object(0, "igBlendMatrixSelect", 0xC0);
    let palette = b.object(0, "igIntList", 0x18);
    let select_children = b.object(0, "igNodeList", 0x18);
    let combiner = b.object(0, "tfbPhysicsCombinerLink", 0x3C);
    let skeleton = b.object(0, "igSkeleton2", 0x18);
    let bone_list = b.object(0, "igSkeletonBoneList", 0x18);
    b.string(at(sprite, 0x08), spec.portal_sprite);
    b.pointer(at(sprite, 0xD0), model);

    info(&mut b, model, spec.model);
    b.pointer(at(model, 0x14), body);
    info(&mut b, body, "Eruptor.ma");
    let skeletons = PayloadSpec {
        skeletons: vec![spec.skeleton.clone()],
        ..PayloadSpec::default()
    };
    b.memory_ref(at(body, 0x14), blocks, &payload(&skeletons).bytes, 2);
    b.pointer(at(body, 0x20), actor);
    b.pointer(at(body, 0x24), combiner);
    b.null_memory_ref(at(body, 0x28), 0);

    node(&mut b, actor, "Actor");
    b.pointer(at(actor, 0x20), actor_children);
    list(&mut b, actor_children, blocks, &[Some(select)]);
    node(&mut b, select, "skin");
    b.pointer(at(select, 0x20), select_children);
    b.pointer(at(select, 0xB4), palette);
    b.put_u32(at(palette, 0x08), PALETTE.len() as u32);
    b.put_u32(at(palette, 0x0C), PALETTE.len() as u32);
    let ints: Vec<u8> = PALETTE.iter().flat_map(|v| v.to_be_bytes()).collect();
    b.memory_ref(at(palette, 0x10), blocks, &ints, 0);
    let techniques: Vec<Option<Location>> = spec
        .materials
        .iter()
        .map(|m| Some(technique(&mut b, blocks, m, &images, &fixture)))
        .collect();
    list(&mut b, select_children, blocks, &techniques);

    // The combiner, igSkeleton2 and its bones: an unnamed root, then
    // skeleton3's joints, each on its own blend matrix.
    b.string(at(combiner, 0x08), "Eruptor_combiner");
    b.pointer(at(combiner, 0x0C), skeleton);
    for offset in [0x10, 0x18, 0x24, 0x2C] {
        b.null_memory_ref(at(combiner, offset), 0);
    }
    b.string(at(skeleton, 0x08), "DefaultSkeleton_0");
    b.pointer(at(skeleton, 0x0C), bone_list);
    let joints = spec.skeleton.bones.len();
    let inverse: Vec<u8> = (0..joints)
        .flat_map(|i| inverse_joint(i).into_iter().flatten())
        .flat_map(f32::to_be_bytes)
        .collect();
    b.memory_ref(at(skeleton, 0x10), blocks, &inverse, 2);
    let mut bones = vec![b.object(0, "igSkeletonBone", 0x20)];
    b.string(at(bones[0], 0x08), "Unnamed bone");
    b.put_u32(at(bones[0], 0x0C), u32::MAX);
    b.put_u32(at(bones[0], 0x10), u32::MAX);
    for (i, (name, parent, _)) in spec.skeleton.bones.iter().enumerate() {
        let o = b.object(0, "igSkeletonBone", 0x20);
        b.string(at(o, 0x08), name);
        b.put_u32(at(o, 0x0C), (i32::from(*parent) + 1) as u32);
        b.put_u32(at(o, 0x10), i as u32);
        bones.push(o);
    }
    let elements: Vec<Option<Location>> = bones.into_iter().map(Some).collect();
    list(&mut b, bone_list, blocks, &elements);

    // The animation set: the idle in slot 0, an empty slot 1.
    let table = b.object(0, "AnimationInfoList", 0x18);
    b.pointer(at(model, 0x1C), table);
    let mut slots = vec![None, None];
    if let Some((clip, properties)) = &spec.idle {
        let entry = b.object(0, "AnimationInfo", 0x44);
        let anim = b.object(0, "tfbPhysicsAnimation", 0x2C);
        b.string(at(entry, 0x08), "Idle");
        b.pointer(at(entry, 0x3C), anim);
        info(&mut b, anim, spec.idle_name);
        let clips = PayloadSpec {
            clip: Some(clip.clone()),
            ..PayloadSpec::default()
        };
        b.memory_ref(at(anim, 0x14), blocks, &payload(&clips).bytes, 2);
        b.put_u32(at(anim, 0x20), *properties);
        slots[0] = Some(entry);
    }
    list(&mut b, table, blocks, &slots);

    // Attachments: another sprite each, its model unskinned (no body) or a
    // skinned copy of the portal model's body.
    for (name, skinned) in &spec.attachments {
        let s = b.object(0, "tfbSpriteInfo", 0xE4);
        let m = b.object(0, "tfbPhysicsModel", 0x30);
        b.string(at(s, 0x08), name);
        b.pointer(at(s, 0xD0), m);
        info(&mut b, m, &format!("{name}.mdl"));
        if *skinned {
            b.pointer(at(m, 0x14), body);
        }
    }
    b.build()
}

/// `level` stored the way the readers look for it, as `level.bld`.
pub fn archive(level: &[u8]) -> Vec<u8> {
    IgaBuilder::new().stored("level.bld", level).build()
}

/// The sample, a variant with a scroll track and every state attr, and an
/// archive with a skinned attachment beside the portal model, which the
/// sprite's name tells apart.
pub fn seeds() -> Vec<Vec<u8>> {
    let mut states = sample();
    states.materials[1].attrs.extend([
        AttrSpec::BlendState(true),
        AttrSpec::BlendFunction(4, 1),
        AttrSpec::DepthWrite(false),
        AttrSpec::Cull(false),
        AttrSpec::AlphaState(true),
        AttrSpec::AlphaFunction(4, 0.5),
        AttrSpec::Tint(0xFF00_F0F0),
        AttrSpec::Vec4("tfbMulti2Layer4BlendFactorsAttr", [0.0, 0.0, 1.0, 0.0]),
    ]);
    states.materials[0].scroll = Some((0, [0.5; 11], "tfbScrollUVAnimTrack"));
    let mut tie = sample();
    tie.attachments = vec![("112_Woodpecker", true)];
    vec![character(&sample()), character(&states), character(&tie)]
}
