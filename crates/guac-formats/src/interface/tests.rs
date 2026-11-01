use super::builder::{at, glyph, image, sample, template};
use super::*;
use crate::igz::Location;
use crate::igz::builder::IgzBuilder;
use crate::igz::layouts::builder::list;

fn index(igz: &Igz<'_>, at: Location) -> usize {
    igz.object_at(at).expect("an object there").0
}

#[test]
fn one_template_of_each_kind_reads_with_what_it_draws() {
    let s = sample();
    let igz = Igz::parse(&s.bytes).unwrap();
    let ui = Interface::read(&igz).unwrap();
    assert_eq!(ui.templates.len(), 8);
    let t = |n: usize| &ui.templates[n];
    for (n, at) in s.templates.iter().enumerate() {
        assert_eq!(t(n).object, index(&igz, *at));
    }

    assert_eq!(t(0).content, Content::Container);
    assert_eq!(t(0).name, Some("Generic_Container"));
    assert_eq!(
        (t(0).bits.alignment, t(0).bits.align_to_parent),
        (6, 6),
        "the box's anchors"
    );
    // Every field copied: template 1 (n = 1).
    assert_eq!(t(0).flags, 1);
    assert_eq!(t(0).position, [16, -17]);
    assert_eq!(
        (t(0).z_depth, t(0).priority, t(0).width, t(0).height),
        (1.0, -1.0, 10.0, 5.0)
    );
    assert_eq!(t(0).scale, [100.0, 101.0, 100.0]);
    assert_eq!(
        (t(0).color, t(0).replace_color),
        ([0x80, 0x80, 1, 0xFF], [0x80, 0x80, 0x80, 1])
    );
    assert_eq!((t(0).num_value, t(0).depth), (3, 100.0));

    assert_eq!(
        t(1).content,
        Content::Image(Art {
            source: Some("Levels/Includes/UI_Main/autoSave_portrait.png"),
            image: Some(index(&igz, s.image)),
            stream_archive_hash: 0,
            desired: [128, 128],
        })
    );
    assert_eq!(
        t(2).content,
        Content::Image(Art {
            source: Some("Levels/Includes/PortalMessaging/batteries_low_1.png"),
            image: None,
            stream_archive_hash: 0x3E79_592D,
            desired: [512, 512],
        })
    );
    assert!(matches!(
        t(3).content,
        Content::Movie(Art {
            source: Some("Movies/Whirlwind.mov"),
            image: None,
            desired: [-1, -1],
            ..
        })
    ));
    assert_eq!(t(4).content, Content::Text { face: 0 });
    assert_eq!(t(4).bits.point_size, 84);
    assert_eq!(
        t(4).string,
        Some(TextKey::Local(1)),
        "the second key in address order"
    );
    assert_eq!(
        t(5).string,
        Some(TextKey::Imported(ExternalId {
            object: 0x40,
            namespace: crate::fnv1a32(crate::exports::NAMESPACE.as_bytes()),
        }))
    );
    assert_eq!(t(1).string, None);
    match &t(5).content {
        Content::Imported { kind, id } => {
            assert_eq!(*kind, VisualType::Text);
            assert_eq!(id.object, 0x33);
            assert_eq!(
                id.namespace,
                crate::fnv1a32(crate::exports::NAMESPACE.as_bytes())
            );
        }
        other => panic!("{other:?}"),
    }
    assert_eq!(
        t(6).content,
        Content::Model {
            path: Some("C:/tfb/Content/Models/UI/Exclamation_Point3.mdl")
        }
    );
    assert_eq!(
        t(7).content,
        Content::Emitter {
            name: Some("UI_LevelUp_ArrowParticle_VFX")
        }
    );
}

#[test]
fn the_font_reads_its_metrics_glyphs_and_atlas() {
    let s = sample();
    let igz = Igz::parse(&s.bytes).unwrap();
    let ui = Interface::read(&igz).unwrap();
    assert_eq!(ui.fonts.len(), 1);
    let f = &ui.fonts[0];
    assert_eq!(f.object, index(&igz, s.font));
    assert_eq!(f.data, Some(index(&igz, s.font_data)));
    assert_eq!(f.source, Some("fonts/contemp.png"));
    assert_eq!(f.name, Some("hdd:/tfb/content/fonts/contemp"));
    assert_eq!(
        (f.height, f.base_line, f.char_padding, f.kern_amount),
        (82.0, 9.0, 0.5, 2.0)
    );
    assert_eq!(f.atlas, index(&igz, s.atlas));
    let chars: Vec<u16> = f.glyphs.iter().map(|g| g.char).collect();
    assert_eq!(chars, vec![0x41, 0x42, 0x20]);
    assert_eq!(
        f.glyphs[0],
        Glyph {
            char: 0x41,
            step: 30.0,
            offset: [1.0, 2.0],
            size: [29.0, 42.0],
            uv_min: [0.0, 0.0],
            uv_max: [0.25, 0.5],
            kern: KernCorners {
                top_left: 0,
                top_right: 0,
                bottom_left: 0,
                bottom_right: 0,
            },
        }
    );
    assert_eq!(
        (
            f.glyphs[1].kern.bottom_left,
            f.glyphs[1].kern.bottom_right,
            f.glyphs[2].kern.top_left
        ),
        (1, 1, 2)
    );
    // An empty box (the space) sits inside the atlas.
    assert_eq!(f.glyphs[2].uv_min, f.glyphs[2].uv_max);
}

/// A one-template file whose context is `ty`.
fn with_context(ty: &str, size: u32, properties: u32) -> Vec<u8> {
    let mut b = IgzBuilder::new();
    let t = b.object(0, "tfbSpriteInfo", 0xE4);
    let c = b.object(0, ty, size);
    template(&mut b, t, "Odd", properties, 1);
    b.pointer(at(t, 0xD0), c);
    b.build()
}

#[test]
fn a_context_the_reader_doesnt_know_is_refused_by_name() {
    let bytes = with_context("igImage2", 0x60, 0x2001_4408);
    let igz = Igz::parse(&bytes).unwrap();
    let e = Interface::read(&igz).unwrap_err();
    assert_eq!(e.field, "_contextDataInfo");
    assert_eq!(
        e.kind,
        ErrorKind::WrongType {
            expected: CONTEXT,
            found: "igImage2".into(),
        }
    );
}

#[test]
fn a_visual_type_off_its_context_is_refused() {
    // A model context on a template that says text.
    let bytes = with_context("tfbPhysicsModel", 0x30, 0x2001_4409);
    let igz = Igz::parse(&bytes).unwrap();
    let e = Interface::read(&igz).unwrap_err();
    assert_eq!(
        (e.field, e.kind),
        ("_visualType", ErrorKind::Unsupported { value: 1 })
    );
    // With no context: a container is one, art or text is kept empty, and
    // a visual type past the enum is refused.
    for (props, want) in [
        (0x2001_440B, Some(Content::Container)),
        (
            0x2001_4408,
            Some(Content::Empty {
                kind: VisualType::Art,
            }),
        ),
        (
            0x2001_4409,
            Some(Content::Empty {
                kind: VisualType::Text,
            }),
        ),
        (0x2001_440E, None),
    ] {
        let mut b = IgzBuilder::new();
        let t = b.object(0, "tfbSpriteInfo", 0xE4);
        template(&mut b, t, "Bare", props, 1);
        let bytes = b.build();
        let igz = Igz::parse(&bytes).unwrap();
        let got = Interface::read(&igz).map(|ui| ui.templates[0].content.clone());
        assert_eq!(got.ok(), want, "{props:#x}");
    }
    // An import can't be a container.
    let mut b = IgzBuilder::new();
    let t = b.object(0, "tfbSpriteInfo", 0xE4);
    template(&mut b, t, "Imported", 0x2001_440B, 1);
    b.import_raw(at(t, 0xD0), 0x33, 0x2694_BE8E);
    let bytes = b.build();
    let igz = Igz::parse(&bytes).unwrap();
    let e = Interface::read(&igz).unwrap_err();
    assert_eq!(
        (e.field, e.kind),
        ("_visualType", ErrorKind::Unsupported { value: 3 })
    );
}

/// A font whose one glyph's box is `uv`.
fn font_with(uv: [f32; 4]) -> Vec<u8> {
    let mut b = IgzBuilder::new();
    let font = b.object(0, "igBitmapFont", 0x50);
    let glyphs = b.object(0, "igCharMetricsList", 0x18);
    let one = b.object(0, "igCharMetrics", 0x30);
    let bind = b.object(0, "tfbAlphaTextureUnitIDAttr", 0x20);
    let texture = b.object(0, "igTextureAttr2", 0x40);
    let atlas = b.object(0, "igImage2", 0x60);
    let blocks = b.section(0x10);
    b.pointer(at(font, 0x38), bind);
    b.pointer(at(font, 0x3C), glyphs);
    list(&mut b, glyphs, blocks, &[Some(one)]);
    glyph(&mut b, one, 0x61, 0, 12.0, uv);
    b.pointer(at(bind, 0x10), texture);
    b.pointer(at(texture, 0x30), atlas);
    image(&mut b, atlas, blocks, "simple.png", 0x11);
    b.build()
}

#[test]
fn a_glyph_whose_box_leaves_its_atlas_is_refused() {
    let bytes = font_with([0.0, 0.0, 1.0, 1.0]);
    let igz = Igz::parse(&bytes).unwrap();
    assert_eq!(Interface::read(&igz).unwrap().fonts[0].glyphs.len(), 1);
    for (uv, field) in [
        ([-0.01, 0.0, 0.5, 0.5], "_texCoordMin"),
        ([0.0, 0.0, 1.01, 0.5], "_texCoordMax"),
        ([0.0, 0.0, 0.5, f32::NAN], "_texCoordMax"),
        ([0.5, 0.0, 0.25, 0.5], "_texCoordMax"),
        ([0.0, 0.75, 0.5, 0.5], "_texCoordMax"),
    ] {
        let bytes = font_with(uv);
        let igz = Igz::parse(&bytes).unwrap();
        let e = Interface::read(&igz).unwrap_err();
        assert_eq!(e.field, field, "{uv:?}");
        assert!(matches!(e.kind, ErrorKind::Unsupported { .. }), "{uv:?}");
    }
}

#[test]
fn a_font_without_its_atlas_or_glyphs_is_refused() {
    for missing in ["_textureBind", "_charMetricsList"] {
        let mut b = IgzBuilder::new();
        let font = b.object(0, "igBitmapFont", 0x50);
        let glyphs = b.object(0, "igCharMetricsList", 0x18);
        let bind = b.object(0, "tfbAlphaTextureUnitIDAttr", 0x20);
        let texture = b.object(0, "igTextureAttr2", 0x40);
        let atlas = b.object(0, "igImage2", 0x60);
        let blocks = b.section(0x10);
        if missing != "_textureBind" {
            b.pointer(at(font, 0x38), bind);
        }
        if missing != "_charMetricsList" {
            b.pointer(at(font, 0x3C), glyphs);
        }
        list(&mut b, glyphs, blocks, &[]);
        b.pointer(at(bind, 0x10), texture);
        b.pointer(at(texture, 0x30), atlas);
        image(&mut b, atlas, blocks, "simple.png", 0x11);
        let bytes = b.build();
        let igz = Igz::parse(&bytes).unwrap();
        let e = Interface::read(&igz).unwrap_err();
        assert_eq!((e.field, e.kind), (missing, ErrorKind::Missing));
    }
}

#[test]
fn a_string_ref_to_another_type_is_refused() {
    let mut b = IgzBuilder::new();
    let t = b.object(0, "tfbSpriteInfo", 0xE4);
    let other = b.object(0, "ValueInfo", 0x24);
    template(&mut b, t, "Box", 0x2001_440B, 1);
    b.pointer(at(t, 0xD4), other);
    let bytes = b.build();
    let igz = Igz::parse(&bytes).unwrap();
    let e = Interface::read(&igz).unwrap_err();
    assert_eq!(e.field, "_stringInfo");
}

#[test]
fn each_strings_settings_read_in_the_order_its_keys_take() {
    let s = sample();
    let igz = Igz::parse(&s.bytes).unwrap();
    let ui = Interface::read(&igz).unwrap();
    assert_eq!(
        ui.strings,
        vec![
            Settings {
                flags: 0x400,
                integer_digits: 4,
                decimal_digits: 0,
            },
            Settings {
                flags: 0x2800,
                integer_digits: 5,
                decimal_digits: 1,
            },
        ]
    );
    // The text template's key is the second, and so are its settings.
    let Some(TextKey::Local(n)) = ui.templates[4].string else {
        panic!("{:?}", ui.templates[4].string);
    };
    assert_eq!(ui.strings[n].flags, 0x2800);
}

#[test]
fn a_string_of_another_shape_is_refused() {
    let mut b = IgzBuilder::new();
    b.object(0, "StringInfo", 0x24);
    let bytes = b.build();
    let igz = Igz::parse(&bytes).unwrap();
    let e = Interface::read(&igz).unwrap_err();
    assert_eq!(
        (e.field, &e.kind),
        ("MTSZ", &ErrorKind::Unsupported { value: 0x24 }),
        "its size check's: {e}"
    );
}
