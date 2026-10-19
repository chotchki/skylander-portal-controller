use super::*;
use crate::igz::layouts::Hero;
use crate::model::builder::{AttrSpec, BindSpec, CharacterSpec, MaterialSpec, character, sample};
use crate::model::portal_model;

/// Each material of `spec`'s portal model, read.
fn read(spec: &CharacterSpec) -> Vec<Result<Material<'static>, FormatError>> {
    let bytes: &'static [u8] = Vec::leak(character(spec));
    let igz: &'static Igz<'static> = Box::leak(Box::new(Igz::parse(bytes).unwrap()));
    let hero: &'static Hero<'static> = Box::leak(Box::new(portal_model(igz).unwrap().hero));
    hero.meshes
        .iter()
        .map(|m| Material::read(igz, hero, m))
        .collect()
}

fn first(spec: &CharacterSpec) -> Result<Material<'static>, FormatError> {
    read(spec).remove(0)
}

fn with_body(f: impl FnOnce(&mut MaterialSpec)) -> CharacterSpec {
    let mut s = sample();
    f(&mut s.materials[0]);
    s
}

#[test]
fn samplers_run_alphabetically_and_follow_the_bits() {
    // Eruptor's body: normal, spec, colour, rim.
    assert_eq!(
        samplers(0x1C),
        [
            Role::Layer1BaseNormalMap,
            Role::Layer1BaseSpecMap,
            Role::Layer1ColorMap,
            Role::Layer4ColorMap
        ]
    );
    // His spikes: colour and rim; a bare variant: colour alone.
    assert_eq!(samplers(0x24), [Role::Layer1ColorMap, Role::Layer4ColorMap]);
    assert_eq!(samplers(0), [Role::Layer1ColorMap]);
    // Every sampler: layer 2 with its own normal and spec maps.
    assert_eq!(samplers(0x1F).len(), 8);
    let names: Vec<&str> = samplers(0x1F).iter().map(|r| r.name()).collect();
    let mut sorted = names.clone();
    sorted.sort_unstable();
    assert_eq!(names, sorted);
    // The gloss map (bit 7) reads layer 3's sampler; runtime bits add none.
    assert_eq!(
        samplers(0x84),
        [
            Role::Layer1ColorMap,
            Role::Layer3ColorMap,
            Role::Layer4ColorMap
        ]
    );
    assert_eq!(samplers(0x1C | 0x3D00), samplers(0x1C));
    let units: Vec<i16> = Role::ALL.iter().map(|r| r.unit()).collect();
    assert_eq!(units, [4, 6, 0, 8, 10, 1, 2, 3]);
}

#[test]
fn the_existence_rules_hold() {
    assert!(pixel_variant_exists(0x1C) && pixel_variant_exists(0x3C1C));
    assert!(!pixel_variant_exists(0x41));
    assert!(!pixel_variant_exists(0x1000), "point light 2 without 1");
    assert!(!pixel_variant_exists(0x86), "layer 3 with the gloss map");
    assert!(!pixel_variant_exists(1 << 14));
    assert!(vertex_variant_exists(0x101C));
    assert!(!vertex_variant_exists(0x800), "skinning is never compiled");
}

#[test]
fn eruptors_body_maps_its_textures_by_sampler_order() {
    let m = read(&sample());
    let body = m[0].as_ref().unwrap();
    assert_eq!(body.name, Some("EruptorBody"));
    assert_eq!(
        (body.pixel.base_mask, body.pixel.ignore_mask),
        (0x1C, 0x2FF)
    );
    assert_eq!(
        (body.vertex.base_mask, body.vertex.ignore_mask),
        (0x1C, 0x1AFF)
    );
    let roles: Vec<(Role, Option<&str>)> = body
        .textures
        .iter()
        .map(|t| (t.role, t.image_name))
        .collect();
    assert_eq!(
        roles,
        [
            (Role::Layer1BaseNormalMap, Some("eruptor_n")),
            (Role::Layer1BaseSpecMap, Some("eruptor_sp")),
            (Role::Layer1ColorMap, Some("eruptor_c")),
            (Role::Layer4ColorMap, Some("eruptorrim")),
        ]
    );
    let n = body.texture(Role::Layer1BaseNormalMap).unwrap();
    assert_eq!(n.sampler.format_hint, TFN1);
    assert_eq!((n.sampler.wrap_s, n.sampler.min_filter), (1, 5));
    // His parameters, the rest at the original's defaults.
    let p = body.params;
    assert_eq!(p.layer1_extra, [1.0; 4]);
    assert_eq!(p.fixed_alpha, [-1.0, -1.0, -1.0, 1.0]);
    assert_eq!(p.glow, Glow::Amount(64));
    assert_eq!(p.layer4_blend, [0.0, 1.0, 0.0, 0.0]);
    assert_eq!(p.layer4_values, [0.0, 1.0, 0.0, 0.0]);
    assert_eq!(p.material_tint, [128, 128, 128, 255]);
    assert_eq!(body.states, States::default());
    assert!(body.scrolls.is_empty());
    // The turntable's scene turns fog on: 0x41C, as AC.13 drew him.
    assert_eq!(
        (body.drawn_pixel_mask(), body.drawn_vertex_mask()),
        (0x41C, 0x41C)
    );
    // The eyes have no glow attr, so they glow nowhere.
    let eyes = m[1].as_ref().unwrap();
    assert_eq!(eyes.params.glow, Glow::None);
    assert_eq!(
        eyes.texture(Role::Layer4ColorMap).unwrap().image_name,
        Some("specrim_eye6")
    );
}

#[test]
fn defaults_fill_what_a_material_leaves_out() {
    let m = first(&with_body(|b| b.attrs.clear())).unwrap();
    assert_eq!(m.params, Params::DEFAULT);
    assert_eq!(Params::DEFAULT.layer1_extra, [1.0, 1.0, 1.0, 0.25]);
    assert_eq!(m.attrs.len(), 2 + 4);
}

#[test]
fn a_packed_tint_reads_red_from_its_low_byte() {
    assert_eq!(unpack_tint(0xFF00_F0F0), [0xF0, 0xF0, 0x00, 0xFF]);
    assert_eq!(unpack_tint(0xFF91_9191), [0x91, 0x91, 0x91, 0xFF]);
    let m = first(&with_body(|b| b.attrs.push(AttrSpec::Tint(0xFF00_F0F0)))).unwrap();
    assert_eq!(m.params.material_tint, [0xF0, 0xF0, 0, 0xFF], "yellow");
}

#[test]
fn render_states_come_off_their_attrs() {
    let states = |attrs: Vec<AttrSpec>| first(&with_body(|b| b.attrs = attrs)).map(|m| m.states);
    assert_eq!(
        states(vec![AttrSpec::BlendState(true)]).unwrap().blend,
        Blend::Alpha
    );
    let additive = states(vec![
        AttrSpec::BlendState(true),
        AttrSpec::BlendFunction(4, 1),
        AttrSpec::DepthWrite(false),
        AttrSpec::Cull(false),
    ])
    .unwrap();
    assert_eq!(
        additive,
        States {
            blend: Blend::Additive,
            depth_write: false,
            two_sided: true,
            alpha_test: None
        }
    );
    let tested = states(vec![
        AttrSpec::AlphaState(true),
        AttrSpec::AlphaFunction(4, 0.25),
    ]);
    assert_eq!(tested.unwrap().alpha_test, Some(0.25));
    // A state switched off is the default.
    assert_eq!(
        states(vec![AttrSpec::BlendState(false)]).unwrap(),
        States::default()
    );
    // A function without blending, any function but additive, a test with
    // no function or not GREATER, culling switched on: refused.
    let field = |attrs| states(attrs).unwrap_err().field;
    assert_eq!(
        field(vec![AttrSpec::BlendFunction(4, 1)]),
        "igBlendStateAttr"
    );
    assert_eq!(
        field(vec![
            AttrSpec::BlendState(true),
            AttrSpec::BlendFunction(4, 5)
        ]),
        "igBlendFunctionAttr"
    );
    assert_eq!(
        field(vec![AttrSpec::AlphaState(true)]),
        "igAlphaFunctionAttr"
    );
    assert_eq!(
        field(vec![
            AttrSpec::AlphaState(true),
            AttrSpec::AlphaFunction(2, 0.5)
        ]),
        "_func"
    );
    assert_eq!(field(vec![AttrSpec::Cull(true)]), "igCullFaceAttr");
    assert_eq!(
        field(vec![AttrSpec::BlendState(true), AttrSpec::BlendState(true)]),
        "igBlendStateAttr"
    );
}

#[test]
fn a_material_with_bit_6_7_or_9_is_refused_by_name() {
    for (bit, name) in [
        (6, "layer4UsesPositions"),
        (7, "layer4UsesGlossmap"),
        (9, "receivesReflections"),
    ] {
        let spec = with_body(|b| b.pixel = Some((0x1C | 1 << bit, 0x2FF)));
        let e = first(&spec).unwrap_err();
        assert_eq!(e.kind, ErrorKind::Feature { bit, name }, "bit {bit}");
        assert!(e.to_string().contains(name), "{e}");
    }
    let e = first(&with_body(|b| b.vertex = Some((0x81C, 0x1AFF)))).unwrap_err();
    assert_eq!(
        e.kind,
        ErrorKind::Feature {
            bit: 11,
            name: "vertexSkinning"
        }
    );
}

#[test]
fn a_tfn1_image_outside_the_normal_map_slot_is_refused() {
    // The colour map carrying the hint.
    let e = first(&with_body(|b| {
        b.binds[2] = BindSpec::new(0, 2).normal_map()
    }))
    .unwrap_err();
    assert_eq!(
        e.kind,
        ErrorKind::NormalMapHint {
            role: "layer1ColorMap",
            hinted: true
        }
    );
    // The normal-map slot without it.
    let e = first(&with_body(|b| b.binds[0] = BindSpec::new(4, 0))).unwrap_err();
    assert_eq!(
        e.kind,
        ErrorKind::NormalMapHint {
            role: "layer1BaseNormalMap",
            hinted: false
        }
    );
}

#[test]
fn binds_out_of_sampler_order_or_count_are_refused() {
    // Colour and normal swapped: the units no longer match the roles.
    let e = first(&with_body(|b| b.binds.swap(0, 2))).unwrap_err();
    assert_eq!(e.field, "_cachedUnitID");
    // One bind short of the variant's four samplers.
    let e = first(&with_body(|b| {
        b.binds.pop();
    }))
    .unwrap_err();
    assert_eq!(
        e.kind,
        ErrorKind::CountMismatch {
            found: 3,
            expected: 4
        }
    );
}

#[test]
fn only_tfbmulti2_with_both_masks_and_known_attrs_reads() {
    let e = first(&with_body(|b| b.effect = TFBWARP)).unwrap_err();
    assert_eq!(
        (e.field, e.kind),
        ("_effect", ErrorKind::Unsupported { value: 0xAA9 })
    );
    let e = first(&with_body(|b| b.pixel = None)).unwrap_err();
    assert_eq!(
        (e.field, e.kind),
        ("igPixelShaderVariantMaskAttr", ErrorKind::Missing)
    );
    let e = first(&with_body(|b| b.vertex = None)).unwrap_err();
    assert_eq!(e.field, "igVertexShaderVariantMaskAttr");
    let e = first(&with_body(|b| {
        b.attrs.push(AttrSpec::Other("igAttrList", 0x18))
    }))
    .unwrap_err();
    assert!(matches!(e.kind, ErrorKind::WrongType { .. }), "{e}");
    let e = first(&with_body(|b| {
        b.attrs
            .push(AttrSpec::Vec4("tfbMulti2Layer1ExtraValuesAttr", [0.0; 4]))
    }))
    .unwrap_err();
    assert_eq!(
        (e.field, e.kind),
        ("tfbMulti2Layer1ExtraValuesAttr", ErrorKind::Duplicate)
    );
    let e = first(&with_body(|b| b.attrs.push(AttrSpec::Glow(300, true)))).unwrap_err();
    assert_eq!(e.field, "tfbDestinationAlphaAttr", "a second glow attr");
    let e = first(&with_body(|b| b.attrs = vec![AttrSpec::Glow(300, true)])).unwrap_err();
    assert_eq!(e.field, "_alpha");
}

#[test]
fn a_scroll_track_reads_and_other_track_kinds_are_refused() {
    let values: [f32; 11] = std::array::from_fn(|i| i as f32 * 0.5);
    let m = first(&with_body(|b| {
        b.scroll = Some((0, values, "tfbScrollUVAnimTrack"))
    }))
    .unwrap();
    assert_eq!(m.scrolls.len(), 1);
    let s = m.scrolls[0];
    assert_eq!(s.layer, 0);
    assert_eq!((s.data.scale, s.data.velocity), ([0.0, 0.5], [1.0, 1.5]));
    assert_eq!(s.data.rotation_phase, 5.0);
    let e = first(&with_body(|b| {
        b.scroll = Some((0, values, "tfbFlipBookUVAnimTrack"))
    }))
    .unwrap_err();
    assert_eq!(e.field, "_trackList");
}
