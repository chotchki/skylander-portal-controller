//! AT.1: the interface's layouts on objects built in code.

use super::builder::{InterfaceSample, interface_sample};
use super::*;
use crate::igz::builder::IgzBuilder;

fn index(igz: &Igz<'_>, at: Location) -> usize {
    igz.object_at(at).expect("an object there").0
}

fn at(base: Location, offset: u32) -> Location {
    base.offset_by(offset).unwrap()
}

fn sampled() -> (InterfaceSample, Vec<u8>) {
    let s = interface_sample();
    let bytes = s.bytes.clone();
    (s, bytes)
}

#[test]
fn a_text_sprite_reads_its_bits_and_list() {
    let (s, bytes) = sampled();
    let igz = Igz::parse(&bytes).unwrap();
    let sprite = SpriteInfo::read(&igz, index(&igz, s.sprite)).unwrap();
    assert_eq!(sprite.properties, 0x20A9_4649);
    assert_eq!(
        sprite.bits(),
        SpriteBits {
            visual_type: 1,
            blend_mode: 1,
            justification: 1,
            alignment: 6,
            align_to_parent: 4,
            visible: true,
            point_size: 84,
            dirty: true,
            string_width_valid: false,
        }
    );
    assert_eq!(sprite.bits().kind(), VisualType::Text);
    assert_eq!(VISUAL_TYPES[1], "text");
    assert_eq!(ALIGNMENTS[6], "bottom-left");
    // 12.4 halves: x 20, y -20.
    assert_eq!(sprite.position, [0x0140, -0x0140]);
    assert_eq!(
        sprite.placement.attached_objects,
        ObjectRef::Object(index(&igz, s.sprite_list))
    );
    let list = SpriteInfoList::read(&igz, index(&igz, s.sprite_list)).unwrap();
    assert_eq!((list.list.count, list.elements.len()), (0, 0));
    assert_eq!(
        sprite.context_data_info,
        ObjectRef::Object(index(&igz, s.font_data))
    );
    // A text sprite has no texture context.
    assert_eq!(sprite.texture_data(&igz), None);
}

#[test]
fn a_font_reads_back_down_to_its_glyphs_and_atlas() {
    let (s, bytes) = sampled();
    let igz = Igz::parse(&bytes).unwrap();
    let data = FontDataInfo::read(&igz, index(&igz, s.font_data)).unwrap();
    assert_eq!(data.data_info.info.name, Some("fonts/simple.png"));
    assert_eq!(data.font, Some(index(&igz, s.font)));

    let font = BitmapFont::read(&igz, index(&igz, s.font)).unwrap();
    assert_eq!(font.font.name, Some("hdd:/tfb/content/fonts/simple"));
    assert_eq!((font.font.height, font.font.base_line), (42.0, 9.0));
    assert_eq!(font.font.attr_list, Some(index(&igz, s.attrs)));
    assert_eq!(font.font.color, [0.25, 0.5, 0.75, 1.0]);
    assert_eq!(font.font.char_padding, 1.5);
    assert_eq!(font.kern_amount, 2.0);
    assert_eq!(font.texture_bind, Some(index(&igz, s.bind)));
    assert_eq!(font.char_metrics_list, Some(index(&igz, s.glyph_list)));

    let glyphs = CharMetricsList::read(&igz, index(&igz, s.glyph_list)).unwrap();
    assert_eq!(
        glyphs.glyphs,
        vec![index(&igz, s.glyphs[0]), index(&igz, s.glyphs[1])]
    );
    let a = CharMetrics::read(&igz, glyphs.glyphs[0]).unwrap();
    assert_eq!(
        a,
        CharMetrics {
            char_number: 0x41,
            properties: 0x4321,
            step: 24.0,
            offset: [1.0, -2.0],
            size: [23.0, 42.0],
            tex_coord_min: [0.125, 0.25],
            tex_coord_max: [0.375, 0.5],
        }
    );
    assert_eq!(
        a.kern(),
        KernCorners {
            top_left: 1,
            top_right: 2,
            bottom_left: 3,
            bottom_right: 4,
        }
    );
    let quote = CharMetrics::read(&igz, glyphs.glyphs[1]).unwrap();
    assert_eq!(quote.char_number, u16::from(b'"'));
    assert_eq!(
        quote.kern(),
        KernCorners {
            top_left: 0,
            top_right: 0,
            bottom_left: 1,
            bottom_right: 1,
        }
    );
    // The atlas chain reads with the existing layouts.
    let bind = AlphaTextureUnitIdAttr::read(&igz, font.texture_bind.unwrap()).unwrap();
    let texture = TextureAttr2::read(&igz, bind.bind.texture.unwrap()).unwrap();
    assert_eq!(texture.image, Some(index(&igz, s.image)));
}

/// Each field's own bits, set one at a time in an otherwise clear word.
#[test]
fn each_sprite_bit_lands_in_its_own_field() {
    let fields: [(&str, BitField); 9] = [
        ("visual type", SpriteBits::VISUAL_TYPE),
        ("blend mode", SpriteBits::BLEND_MODE),
        ("justification", SpriteBits::JUSTIFICATION),
        ("alignment", SpriteBits::ALIGNMENT),
        ("align to parent", SpriteBits::ALIGN_TO_PARENT),
        ("visible", SpriteBits::VISIBLE),
        ("point size", SpriteBits::POINT_SIZE),
        ("dirty", SpriteBits::DIRTY),
        ("string width valid", SpriteBits::STRING_WIDTH_VALID),
    ];
    let read = |b: SpriteBits| -> [u32; 9] {
        [
            u32::from(b.visual_type),
            u32::from(b.blend_mode),
            u32::from(b.justification),
            u32::from(b.alignment),
            u32::from(b.align_to_parent),
            u32::from(b.visible),
            u32::from(b.point_size),
            u32::from(b.dirty),
            u32::from(b.string_width_valid),
        ]
    };
    // The registration function's shifts and widths, as literals.
    let expected: [(u32, u32); 9] = [
        (0, 3),
        (3, 3),
        (6, 2),
        (8, 4),
        (12, 4),
        (16, 1),
        (17, 12),
        (29, 1),
        (30, 1),
    ];
    for ((name, field), (shift, width)) in fields.iter().zip(expected) {
        assert_eq!((field.shift, field.width), (shift, width), "{name}");
    }
    for bit in 0..32 {
        let word = 1u32 << bit;
        let got = read(SpriteBits::from_word(word));
        let owner = expected
            .iter()
            .position(|&(shift, width)| bit >= shift && bit < shift + width);
        for (i, value) in got.iter().enumerate() {
            let want = match owner {
                Some(o) if o == i => 1 << (bit - expected[o].0),
                _ => 0,
            };
            assert_eq!(*value, want, "bit {bit}, {}", fields[i].0);
        }
        // Every bit but 31 round-trips.
        let back = SpriteBits::from_word(word).to_word();
        assert_eq!(back, if bit == 31 { 0 } else { word }, "bit {bit}");
    }
}

#[test]
fn each_kern_bit_lands_in_its_corner() {
    for bit in 0..16u16 {
        let k = KernCorners::from_properties(1 << bit);
        let got = [k.top_left, k.top_right, k.bottom_left, k.bottom_right];
        for (corner, value) in got.iter().enumerate() {
            let want = if usize::from(bit / 4) == corner {
                1 << (bit % 4)
            } else {
                0
            };
            assert_eq!(*value, want, "bit {bit}, corner {corner}");
        }
    }
}

#[test]
fn a_bitfield_sets_only_its_bits() {
    let f = BitField::new(8, 4);
    assert_eq!(f.mask(), 0xF);
    assert_eq!(f.set(0xFFFF_FFFF, 0), 0xFFFF_F0FF);
    assert_eq!(f.set(0, 0x1F), 0xF00);
    assert_eq!(f.get(0x0000_0A00), 0xA);
}

/// The new types at their files' MTSZ read zeroed; four short or long is
/// refused by its type's size check, and a reader handed another type names
/// the type it wanted.
#[test]
fn each_interface_layout_refuses_a_short_object_and_a_stranger() {
    type Read = fn(&Igz<'_>, usize) -> Result<(), FormatError>;
    let table: [(&str, u32, &[u32], Read); 5] = [
        ("SpriteInfoList", 0x18, &[0x10], |i, o| {
            SpriteInfoList::read(i, o).map(drop)
        }),
        ("FontDataInfo", 0x24, &[0x14], |i, o| {
            FontDataInfo::read(i, o).map(drop)
        }),
        ("igBitmapFont", 0x50, &[], |i, o| {
            BitmapFont::read(i, o).map(drop)
        }),
        ("igCharMetrics", 0x30, &[], |i, o| {
            CharMetrics::read(i, o).map(drop)
        }),
        ("igCharMetricsList", 0x18, &[0x10], |i, o| {
            CharMetricsList::read(i, o).map(drop)
        }),
    ];
    for (ty, size, nulls, read) in table {
        assert_eq!(file_size(ty), Some(size), "{ty}");
        let zeroed = |mtsz: u32| {
            let mut b = IgzBuilder::new();
            let o = b.object(0, ty, mtsz);
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
        // Another type of the same size, refused with the wanted type named.
        let mut b = IgzBuilder::new();
        b.object(0, "igSomethingElse", size);
        let bytes = b.build();
        match read(&Igz::parse(&bytes).unwrap(), 0).unwrap_err().kind {
            ErrorKind::WrongType { expected, found } => {
                assert_eq!((expected, found.as_str()), (ty, "igSomethingElse"))
            }
            other => panic!("{ty}: {other:?}"),
        }
    }
}

#[test]
fn a_marker_in_the_fonts_padding_is_refused() {
    let mut b = IgzBuilder::new();
    let font = b.object(0, "igBitmapFont", 0x50);
    let other = b.object(0, "igCharMetrics", 0x30);
    b.pointer(at(font, 0x44), other);
    let bytes = b.build();
    let igz = Igz::parse(&bytes).unwrap();
    let e = BitmapFont::read(&igz, index(&igz, font)).unwrap_err();
    assert_eq!(e.kind, ErrorKind::Unsupported { value: 0x40 });
}

#[test]
fn a_glyph_list_refuses_a_null_and_a_stranger() {
    for stranger in [false, true] {
        let mut b = IgzBuilder::new();
        let list = b.object(0, "igCharMetricsList", 0x18);
        let other = b.object(0, "igImage2", 0x60);
        let blocks = b.section(0x10);
        super::builder::list(
            &mut b,
            list,
            blocks,
            &[if stranger { Some(other) } else { None }],
        );
        let bytes = b.build();
        let igz = Igz::parse(&bytes).unwrap();
        let e = CharMetricsList::read(&igz, index(&igz, list)).unwrap_err();
        assert_eq!(e.field, "element", "stranger {stranger}");
        if stranger {
            assert_eq!(
                e.kind,
                ErrorKind::WrongType {
                    expected: "igCharMetrics",
                    found: "igImage2".into(),
                }
            );
        }
    }
}
