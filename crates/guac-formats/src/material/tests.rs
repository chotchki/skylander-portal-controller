use super::*;
use crate::igz::layouts::Hero;
use crate::model::builder::{
    AttrSpec, BindSpec, CharacterSpec, MaterialSpec, TrackSpec, character, sample,
};
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

/// `spec`'s portal model, its first technique and its hero walk, for the
/// level readers (AG.15).
fn first_mesh(spec: &CharacterSpec) -> (&'static Igz<'static>, &'static Hero<'static>) {
    let bytes: &'static [u8] = Vec::leak(character(spec));
    let igz: &'static Igz<'static> = Box::leak(Box::new(Igz::parse(bytes).unwrap()));
    let hero: &'static Hero<'static> = Box::leak(Box::new(portal_model(igz).unwrap().hero));
    (igz, hero)
}

/// The first technique's material read as a level's.
fn level(spec: &CharacterSpec) -> Result<Material<'static>, FormatError> {
    let (igz, hero) = first_mesh(spec);
    let m = &hero.meshes[0];
    Material::read_technique(
        igz,
        hero.passes(),
        m.technique_index,
        &m.technique,
        m.material,
        Drawn::Level,
    )
}

/// The first technique's material read as another effect's.
fn effect_material(spec: &CharacterSpec) -> Result<EffectMaterial<'static>, FormatError> {
    let (igz, hero) = first_mesh(spec);
    let m = &hero.meshes[0];
    EffectMaterial::read(
        igz,
        hero.passes(),
        m.technique_index,
        &m.technique,
        m.material,
    )
}

/// Eruptor's body on the gloss-map variant (0x9C: normal, spec, colour,
/// gloss, rim), its gloss map image 7.
fn gloss(b: &mut MaterialSpec) {
    b.pixel = Some((0x9C, 0x3FF));
    b.vertex = Some((0x109C, 0x1BFF));
    b.binds = vec![
        BindSpec::new(4, 0).normal_map(),
        BindSpec::new(6, 1),
        BindSpec::new(0, 2),
        BindSpec::new(2, 7),
        BindSpec::new(3, 3),
    ];
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

#[test]
fn a_level_takes_the_gloss_map_a_character_refuses() {
    let spec = with_body(gloss);
    let e = first(&spec).unwrap_err();
    assert_eq!(
        e.kind,
        ErrorKind::Feature {
            bit: 7,
            name: "layer4UsesGlossmap"
        }
    );
    let m = level(&spec).unwrap();
    assert_eq!(m.pixel.base_mask, 0x9C);
    let roles: Vec<(Role, Option<&str>)> =
        m.textures.iter().map(|t| (t.role, t.image_name)).collect();
    assert_eq!(
        roles,
        [
            (Role::Layer1BaseNormalMap, Some("eruptor_n")),
            (Role::Layer1BaseSpecMap, Some("eruptor_sp")),
            (Role::Layer1ColorMap, Some("eruptor_c")),
            (Role::Layer3ColorMap, Some("specrim_eye6")),
            (Role::Layer4ColorMap, Some("eruptorrim")),
        ]
    );
    // Bits 6 and 9 stay refused on a level, by name.
    for (pixel, bit, name) in [
        (0x5C, 6, "layer4UsesPositions"),
        (0x21C, 9, "receivesReflections"),
    ] {
        let e = level(&with_body(|b| b.pixel = Some((pixel, 0x3FF)))).unwrap_err();
        assert_eq!(e.kind, ErrorKind::Feature { bit, name }, "{pixel:#x}");
        assert_eq!(e.field, "_baseMask");
    }
    // And on the vertex side, bits 6 and 9 with skinning.
    for (vertex, bit, name) in [
        (0x5C, 6, "layer4UsesPositions"),
        (0x21C, 9, "receivesReflections"),
        (0x81C, 11, "vertexSkinning"),
    ] {
        let e = level(&with_body(|b| b.vertex = Some((vertex, 0x1BFF)))).unwrap_err();
        assert_eq!(e.kind, ErrorKind::Feature { bit, name }, "{vertex:#x}");
        assert_eq!(e.field, "_baseMask");
    }
    assert_eq!(Drawn::Level.pixel_undrawn(), [6, 9]);
    assert_eq!(Drawn::Character.pixel_undrawn(), [6, 7, 9]);
    assert_eq!(Drawn::Level.vertex_undrawn(), [6, 9, 11]);
}

#[test]
fn a_destination_colour_blend_multiplies() {
    let states = |attrs: Vec<AttrSpec>| first(&with_body(|b| b.attrs = attrs)).map(|m| m.states);
    let multiply = states(vec![
        AttrSpec::BlendState(true),
        AttrSpec::BlendFunction(6, 0),
        AttrSpec::DepthWrite(false),
    ])
    .unwrap();
    assert_eq!(
        multiply,
        States {
            blend: Blend::Multiply,
            depth_write: false,
            two_sided: false,
            alpha_test: None
        }
    );
    // Destination colour onto anything but zero is still refused.
    let e = states(vec![
        AttrSpec::BlendState(true),
        AttrSpec::BlendFunction(6, 1),
    ])
    .unwrap_err();
    assert_eq!(
        (e.field, e.kind),
        (
            "igBlendFunctionAttr",
            ErrorKind::Unsupported { value: 6 << 32 | 1 }
        )
    );
}

fn tracks() -> Vec<TrackSpec> {
    vec![
        TrackSpec::Transform {
            layer: 1,
            floats: [0.5, 0.25, 34.906586, -1.0, 2.0],
            rotation_style: 3,
            scale: [0.1, 0.2, 1.0, 1.5, 2.0, 2.5],
            scale_style: [4, 5],
        },
        TrackSpec::FlipBook {
            layer: 0,
            style: 1,
            rows: 3,
            columns: 2,
            rate: 1.0 / 6.0,
            cells: Some(vec![5, 0, 3]),
        },
        TrackSpec::Tint {
            style: 1,
            rate: 1.0,
            frames: vec![0, 15, 30],
            colors: vec![0xFF8E_BBCE, 0xFF57_93D2, 0x0078_CDEF],
        },
    ]
}

#[test]
fn a_level_reads_the_other_track_kinds_a_character_refuses() {
    let spec = with_body(|b| {
        b.scroll = Some((2, [0.5; 11], "tfbScrollUVAnimTrack"));
        b.tracks = tracks();
    });
    let e = first(&spec).unwrap_err();
    assert_eq!(e.field, "_trackList");
    assert!(
        matches!(&e.kind, ErrorKind::WrongType { found, .. } if found == "tfbTransformUVAnimTrack"),
        "{e}"
    );
    let m = level(&spec).unwrap();
    assert_eq!(m.scrolls.len(), 1);
    assert_eq!(m.scrolls[0].layer, 2);
    let [transform, flip, tint] = &m.tracks[..] else {
        panic!("{:?}", m.tracks);
    };
    let Track::Transform { layer: 1, data } = transform else {
        panic!("{transform:?}");
    };
    assert_eq!(data.center, [0.5, 0.25]);
    assert_eq!(
        (data.rotation_rate, data.rotation_min, data.rotation_max),
        (34.906586, -1.0, 2.0)
    );
    assert_eq!(data.rotation_style, 3);
    assert_eq!(
        (data.scale_rate, data.scale_min, data.scale_max),
        ([0.1, 0.2], [1.0, 1.5], [2.0, 2.5])
    );
    assert_eq!(data.scale_style, [4, 5]);
    let Track::FlipBook {
        layer: 0,
        data,
        cells,
    } = flip
    else {
        panic!("{flip:?}");
    };
    assert_eq!(
        (data.style, data.rows, data.columns, data.rate),
        (1, 3, 2, 1.0 / 6.0)
    );
    assert_eq!(cells, &[5, 0, 3]);
    let Track::Tint {
        data,
        frames,
        colors,
    } = tint
    else {
        panic!("{tint:?}");
    };
    assert_eq!((data.style, data.rate), (1, 1.0));
    assert_eq!(frames, &[0, 15, 30]);
    assert_eq!(colors, &[0xFF8E_BBCE, 0xFF57_93D2, 0x0078_CDEF]);
    // A flip book without its cells list plays the grid's order.
    let m = level(&with_body(|b| {
        b.tracks = vec![TrackSpec::FlipBook {
            layer: 0,
            style: 0,
            rows: 1,
            columns: 4,
            rate: 0.5,
            cells: None,
        }]
    }))
    .unwrap();
    assert!(matches!(&m.tracks[0], Track::FlipBook { cells, .. } if cells.is_empty()));
    // A tint whose colours don't pair up with its frames is refused.
    let e = level(&with_body(|b| {
        b.tracks = vec![TrackSpec::Tint {
            style: 0,
            rate: 1.0,
            frames: vec![0, 15],
            colors: vec![0xFFFF_FFFF],
        }]
    }))
    .unwrap_err();
    assert_eq!(
        (e.field, e.kind),
        (
            "_keyColors",
            ErrorKind::CountMismatch {
                found: 1,
                expected: 2
            }
        )
    );
}

/// The grass's material on Eruptor's body: three binds by unit, its
/// states, its shader constants kept unread.
fn grass(b: &mut MaterialSpec) {
    b.effect = GRASS_EFFECT;
    b.pixel = Some((0x50, 0x50));
    b.vertex = Some((0x50, 0x50));
    b.binds = vec![
        BindSpec::new(1, 5),
        BindSpec::new(2, 6),
        BindSpec::new(0, 2),
    ];
    b.attrs = vec![
        AttrSpec::DepthWrite(false),
        AttrSpec::BlendState(true),
        AttrSpec::AlphaState(true),
        AttrSpec::AlphaFunction(4, 0.023529),
        AttrSpec::Other(SHADER_PARAMETERS, 0x18),
    ];
}

#[test]
fn another_effects_material_keeps_its_textures_by_unit() {
    let m = effect_material(&with_body(grass)).unwrap();
    assert_eq!((m.name, m.effect), (Some("EruptorBody"), GRASS_EFFECT));
    assert_eq!(
        m.pixel.map(|p| (p.base_mask, p.ignore_mask)),
        Some((0x50, 0x50))
    );
    let units: Vec<(i16, Option<&str>)> =
        m.textures.iter().map(|t| (t.unit, t.image_name)).collect();
    assert_eq!(
        units,
        [
            (1, Some("eruptor_sp")),
            (2, Some("eruptor_c")),
            (0, Some("eruptor_c"))
        ]
    );
    assert_eq!(
        m.states,
        States {
            blend: Blend::Alpha,
            depth_write: false,
            two_sided: false,
            alpha_test: Some(0.023529)
        }
    );
    assert_eq!(m.unread, [SHADER_PARAMETERS]);
    assert!(m.scrolls.is_empty() && m.tracks.is_empty());
    // A scroll on another effect is read as a level's is.
    let m = effect_material(&with_body(|b| {
        grass(b);
        b.effect = TFBWARP;
        b.pixel = None;
        b.vertex = None;
        b.scroll = Some((0, [0.25; 11], "tfbScrollUVAnimTrack"));
    }))
    .unwrap();
    assert_eq!((m.effect, m.pixel, m.scrolls.len()), (TFBWARP, None, 1));

    // tfbmulti2's reader refuses another effect, and this one tfbmulti2.
    let e = first(&with_body(grass)).unwrap_err();
    assert_eq!(
        (e.field, e.kind),
        (
            "_effect",
            ErrorKind::Unsupported {
                value: u64::from(GRASS_EFFECT)
            }
        )
    );
    let e = effect_material(&sample()).unwrap_err();
    assert_eq!(
        (e.field, e.kind),
        (
            "_effect",
            ErrorKind::Unsupported {
                value: u64::from(TFBMULTI2)
            }
        )
    );
    // A tfbmulti2 attribute, or one of a type nothing reads, is refused.
    let e = effect_material(&with_body(|b| {
        grass(b);
        b.attrs.push(AttrSpec::Tint(0xFF80_8080));
    }))
    .unwrap_err();
    assert!(
        matches!(&e.kind, ErrorKind::WrongType { found, .. } if found == "tfbMaterialTintAttr"),
        "{e}"
    );
    let e = effect_material(&with_body(|b| {
        grass(b);
        b.attrs.push(AttrSpec::Other("igLightStateAttr", 0x14));
    }))
    .unwrap_err();
    assert!(
        matches!(&e.kind, ErrorKind::WrongType { found, .. } if found == "igLightStateAttr"),
        "{e}"
    );
    // A shader's constants on tfbmulti2 are refused as before.
    let e = first(&with_body(|b| {
        b.attrs.push(AttrSpec::Other(SHADER_PARAMETERS, 0x18))
    }))
    .unwrap_err();
    assert!(
        matches!(&e.kind, ErrorKind::WrongType { found, .. } if found == SHADER_PARAMETERS),
        "{e}"
    );
}

/// The file's own `igEffect`s in `igz`, by object.
fn own_effect_objects(igz: &Igz<'_>) -> Vec<usize> {
    (0..igz.objects().len())
        .filter(|&o| igz.type_name(&igz.objects()[o]) == EFFECT_TYPE)
        .collect()
}

/// A material's parts that don't name objects: what one read through an
/// import and one read through the file's own effect must agree on.
fn drawn_parts(m: &Material<'_>) -> String {
    let textures: Vec<_> = m
        .textures
        .iter()
        .map(|t| (t.role, t.sampler, t.image_name))
        .collect();
    format!(
        "{:?} {:?} {:?} {:?} {:?} {:?} {:?} {:?} {textures:?}",
        m.name, m.pixel, m.vertex, m.params, m.states, m.scrolls, m.tracks, m.attrs
    )
}

/// AH.39: a `global.bld`-style technique naming one of its file's own
/// exported `igEffect`s reads as the import of that effect's export id
/// would, as tfbmulti2's material or another effect's; an `igEffect` the
/// file doesn't export, an object that isn't an `igEffect`, and the file's
/// own effect read without its exports (a character's or a level's way)
/// are each refused on `_effect` by name.
#[test]
fn a_technique_naming_its_files_own_exported_effect_reads_as_its_import() {
    let imported = level(&sample()).unwrap();
    let (igz, hero) = first_mesh(&with_body(|b| b.own_effect = true));
    let m = &hero.meshes[0];
    let effects = own_effect_objects(igz);
    assert_eq!(effects.len(), 1, "the builder's one own effect");
    let read = |own: Option<&OwnEffects>| {
        Material::read_technique_in(
            igz,
            hero.passes(),
            m.technique_index,
            &m.technique,
            m.material,
            Drawn::Level,
            own,
        )
    };
    let own: OwnEffects = [(effects[0], TFBMULTI2)].into_iter().collect();
    assert_eq!(
        effect_in(igz, m.technique_index, &m.technique, Some(&own)).unwrap(),
        TFBMULTI2
    );
    assert_eq!(
        drawn_parts(&read(Some(&own)).unwrap()),
        drawn_parts(&imported)
    );
    // At the grass's id, it reads as the grass's own material would, which
    // tfbmulti2's reader refuses.
    let at_grass: OwnEffects = [(effects[0], GRASS_EFFECT)].into_iter().collect();
    assert_eq!(
        read(Some(&at_grass)).unwrap_err().kind,
        ErrorKind::Unsupported {
            value: u64::from(GRASS_EFFECT)
        }
    );
    let imported = effect_material(&with_body(grass)).unwrap();
    let (gigz, ghero) = first_mesh(&with_body(|b| {
        grass(b);
        b.own_effect = true;
    }));
    let gm = &ghero.meshes[0];
    let theirs: OwnEffects = [(own_effect_objects(gigz)[0], GRASS_EFFECT)]
        .into_iter()
        .collect();
    let e = EffectMaterial::read_in(
        gigz,
        ghero.passes(),
        gm.technique_index,
        &gm.technique,
        gm.material,
        Some(&theirs),
    )
    .unwrap();
    assert_eq!(
        (
            e.name,
            e.effect,
            e.pixel,
            e.vertex,
            e.states,
            e.textures.len()
        ),
        (
            imported.name,
            GRASS_EFFECT,
            imported.pixel,
            imported.vertex,
            imported.states,
            imported.textures.len()
        )
    );
    // Not exported: refused by its object.
    let none = OwnEffects::default();
    let e = read(Some(&none)).unwrap_err();
    assert_eq!(
        (e.field, e.kind),
        (
            "_effect",
            ErrorKind::Unsupported {
                value: effects[0] as u64
            }
        )
    );
    // Without the file's exports, as before: missing.
    let e = read(None).unwrap_err();
    assert_eq!((e.field, e.kind), ("_effect", ErrorKind::Missing));
    assert_eq!(
        effect(igz, m.technique_index, &m.technique)
            .unwrap_err()
            .kind,
        ErrorKind::Missing
    );
    // An object that isn't an effect, even one the file exports: refused by
    // its type.
    let list = m.material.unwrap();
    let mut technique = m.technique;
    technique.effect = ObjectRef::Object(list);
    let not_one: OwnEffects = [(list, TFBMULTI2)].into_iter().collect();
    let e = effect_in(igz, m.technique_index, &technique, Some(&not_one)).unwrap_err();
    assert_eq!(
        (e.field, e.kind),
        (
            "_effect",
            ErrorKind::WrongType {
                expected: EFFECT_TYPE,
                found: "igAttrList".to_owned()
            }
        )
    );
}
