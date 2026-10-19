use super::builder::{SPECS, Spec, images};
use super::*;
use crate::igz::Location;
use crate::igz::builder::IgzBuilder;
use crate::igz::layouts::KNOWN_FORMATS;

fn padded(bytes: &[u8]) -> Vec<u8> {
    let mut v = bytes.to_vec();
    v.resize(v.len().next_multiple_of(128).max(128), 0xEE);
    v
}

fn by_name(name: &str) -> &'static MetaImage {
    MetaImage::by_name(name).unwrap()
}

#[test]
fn every_known_metaimage_has_a_mapping() {
    assert_eq!(META_IMAGES.len(), KNOWN_FORMATS.len());
    for name in KNOWN_FORMATS {
        let m = by_name(name);
        assert_eq!(m.tiled, name.contains("_tile_"), "{name}");
        let want = match &name[..4] {
            "dxt1" => Format::Bc1RgbaUnorm,
            "dxt5" => Format::Bc3Unorm,
            _ => Format::R8Unorm,
        };
        assert_eq!(m.format, want, "{name}");
    }
    assert!(MetaImage::by_name("dxt3_tile_big_ps3").is_none());
}

#[test]
fn morton_matches_hand_tables() {
    let table = |w: u32, h: u32| -> Vec<usize> {
        (0..h)
            .flat_map(|y| (0..w).map(move |x| morton(x, y, w, h)))
            .collect()
    };
    assert_eq!(
        table(4, 4),
        [0, 1, 4, 5, 2, 3, 6, 7, 8, 9, 12, 13, 10, 11, 14, 15]
    );
    // Wider than tall: x's spare bits sit above the interleave.
    assert_eq!(
        table(8, 2),
        [0, 1, 4, 5, 8, 9, 12, 13, 2, 3, 6, 7, 10, 11, 14, 15]
    );
    // Two wide: the swizzle is the identity.
    assert_eq!(table(2, 8), (0..16).collect::<Vec<_>>());
    assert_eq!(table(1, 1), [0]);
}

#[test]
fn tiled_l8_untiles_from_morton_order() {
    // Byte i is the texel at swizzled index i.
    let swizzled: Vec<u8> = (0..16).collect();
    let t = by_name("l8_tile_big_ps3")
        .untile(4, 4, 1, &padded(&swizzled))
        .unwrap();
    assert_eq!(t.format, Format::R8Unorm);
    assert_eq!(
        t.levels,
        [vec![0, 1, 4, 5, 2, 3, 6, 7, 8, 9, 12, 13, 10, 11, 14, 15]]
    );
    let t = by_name("l8_tile_big_ps3")
        .untile(8, 2, 1, &padded(&swizzled))
        .unwrap();
    assert_eq!(
        t.levels,
        [vec![0, 1, 4, 5, 8, 9, 12, 13, 2, 3, 6, 7, 10, 11, 14, 15]]
    );
}

#[test]
fn tiled_l8_mips_untile_each_at_their_own_size() {
    // 4x4 then 2x2 then 1x1, packed with no padding between.
    let data: Vec<u8> = (0..21).collect();
    let t = by_name("l8_tile_big_ps3")
        .untile(4, 4, 3, &padded(&data))
        .unwrap();
    assert_eq!(t.levels.len(), 3);
    assert_eq!(t.levels[1], [16, 17, 18, 19]);
    assert_eq!(t.levels[2], [20]);
}

/// A grid of blocks, block k's bytes all k.
fn blocks(count: u8, size: usize) -> Vec<u8> {
    (0..count).flat_map(|k| vec![k; size]).collect()
}

#[test]
fn tiled_dxt_blocks_stay_in_rows() {
    // 16x8 DXT5: level 0 is 4x2 blocks, level 1 is 2x1.
    let data = blocks(10, 16);
    let t = by_name("dxt5_tile_big_ps3")
        .untile(16, 8, 2, &padded(&data))
        .unwrap();
    assert_eq!(t.format, Format::Bc3Unorm);
    assert_eq!(t.levels[0], blocks(8, 16));
    assert_eq!(t.levels[1], [vec![8; 16], vec![9; 16]].concat());
    // Bytes inside a block keep their order: no endian swap.
    let ramp: Vec<u8> = (0..8).collect();
    let t = by_name("dxt1_tile_big_ps3")
        .untile(4, 4, 1, &padded(&ramp))
        .unwrap();
    assert_eq!(t.levels[0], ramp);
}

#[test]
fn linear_dxt_blocks_stay_in_rows() {
    // 12x6 DXT1: 3x2 blocks, tight pitch.
    let data = blocks(6, 8);
    let t = by_name("dxt1_big_ps3")
        .untile(12, 6, 1, &padded(&data))
        .unwrap();
    assert_eq!(t.format, Format::Bc1RgbaUnorm);
    assert_eq!((t.width, t.height), (12, 6));
    assert_eq!(t.levels, [data]);
    // Under 4 texels still takes a whole block.
    let t = by_name("dxt5_big_ps3")
        .untile(2, 87, 1, &padded(&blocks(22, 16)))
        .unwrap();
    assert_eq!(t.levels[0], blocks(22, 16));
}

#[test]
fn linear_l8_copies_rows() {
    let linear = MetaImage {
        name: "l8_big_ps3",
        format: Format::R8Unorm,
        tiled: false,
    };
    let data: Vec<u8> = (0..15).collect();
    let t = linear.untile(5, 3, 1, &padded(&data)).unwrap();
    assert_eq!(t.levels, [data]);
}

#[test]
fn sizes_follow_the_mip_chain() {
    let f = Format::Bc1RgbaUnorm;
    assert_eq!(f.level_size(8, 8), 32);
    assert_eq!(f.level_size(1, 1), 8);
    assert_eq!(f.chain_size(8, 8, 4), 56);
    assert_eq!(
        Format::Bc3Unorm.chain_size(16, 8, 5),
        128 + 32 + 16 + 16 + 16
    );
    assert_eq!(Format::R8Unorm.chain_size(128, 2, 1), 256);
    assert_eq!(level_dim(8, 2), 2);
    assert_eq!(level_dim(8, 9), 1);
    assert_eq!(level_dim(8, 40), 1);
}

fn refusal(m: &MetaImage, w: u16, h: u16, n: u16, len: usize) -> (&'static str, usize, ErrorKind) {
    let e = m.untile(w, h, n, &vec![0; len]).unwrap_err();
    (e.field, e.offset, e.kind)
}

#[test]
fn the_wrong_shape_is_refused() {
    let tile = by_name("dxt5_tile_big_ps3");
    let linear = by_name("dxt5_big_ps3");
    let unsupported = |value: u64| ErrorKind::Unsupported { value };
    assert_eq!(refusal(tile, 0, 4, 1, 128), ("_width", 0, unsupported(0)));
    assert_eq!(refusal(tile, 4, 0, 1, 128), ("_height", 0, unsupported(0)));
    assert_eq!(refusal(tile, 12, 4, 1, 128), ("_width", 0, unsupported(12)));
    assert_eq!(refusal(tile, 4, 6, 1, 128), ("_height", 0, unsupported(6)));
    let too_many = ErrorKind::OutOfRange { value: 4, limit: 3 };
    assert_eq!(refusal(tile, 4, 4, 4, 128), ("_levelCount", 0, too_many));
    let none = ErrorKind::OutOfRange { value: 0, limit: 3 };
    assert_eq!(refusal(tile, 4, 4, 0, 128), ("_levelCount", 0, none));
    assert_eq!(
        refusal(linear, 8, 8, 2, 128),
        ("_levelCount", 0, unsupported(2))
    );
    let short = ErrorKind::Truncated {
        needed: 128,
        available: 127,
    };
    assert_eq!(refusal(tile, 4, 4, 1, 127), ("_data", 0, short));
    assert_eq!(
        refusal(tile, 4, 4, 1, 256),
        ("_data", 128, unsupported(256))
    );
    // The largest image sizes its chain without overflow, then is refused.
    let big = refusal(tile, 32768, 32768, 16, 128);
    assert_eq!(big.0, "_data");
}

#[test]
fn builder_images_read_and_untile() {
    let built = images();
    let igz = Igz::parse(&built.bytes).unwrap();
    for (spec, &at) in SPECS.iter().zip(&built.images) {
        let (index, _) = igz.object_at(at).unwrap();
        let t = Texture::read(&igz, index).unwrap();
        let direct = by_name(spec.format)
            .untile(spec.width, spec.height, spec.levels, &spec.data())
            .unwrap();
        assert_eq!(t, direct, "{}", spec.format);
        assert_eq!(t.levels.len(), usize::from(spec.levels));
    }
}

/// One image built alone, `edit` applied to the builder after it.
fn lone_spec(spec: &Spec, data: &[u8], edit: impl FnOnce(&mut IgzBuilder, Location)) -> Vec<u8> {
    let mut b = IgzBuilder::new();
    let blocks = b.section(0x80);
    let at = builder::image(&mut b, blocks, spec, data);
    edit(&mut b, at);
    b.build()
}

fn lone(spec_index: usize, data: &[u8], edit: impl FnOnce(&mut IgzBuilder, Location)) -> Vec<u8> {
    lone_spec(&SPECS[spec_index], data, edit)
}

fn read_lone(bytes: &[u8]) -> Result<Texture, FormatError> {
    let igz = Igz::parse(bytes).unwrap();
    Texture::read(&igz, 0)
}

#[test]
fn read_refuses_fields_at_their_file_offsets() {
    let spec = &SPECS[0];
    let ok = lone(0, &spec.data(), |_, _| {});
    assert!(read_lone(&ok).is_ok());

    let depth = lone(0, &spec.data(), |b, at| {
        b.put_bytes(at.offset_by(0x34).unwrap(), &2u16.to_be_bytes())
    });
    let e = read_lone(&depth).unwrap_err();
    assert_eq!(
        (e.field, e.kind),
        ("_depth", ErrorKind::Unsupported { value: 2 })
    );
    let base = read_lone(&lone(0, &spec.data(), |b, at| {
        b.put_bytes(at.offset_by(0x38).unwrap(), &3u16.to_be_bytes())
    }))
    .unwrap_err();
    assert_eq!(base.field, "_imageCount");
    // Field offsets are the object's plus the field's.
    assert_eq!(base.offset - e.offset, 4);

    // Scale, bias and padding: the first non-identity component, at its word.
    let scale = read_lone(&lone(0, &spec.data(), |b, at| {
        b.put_bytes(at.offset_by(0x18).unwrap(), &0.5f32.to_be_bytes())
    }))
    .unwrap_err();
    assert_eq!(
        (scale.field, scale.kind),
        (
            "_colorScale",
            ErrorKind::Unsupported {
                value: 0.5f32.to_bits().into()
            }
        )
    );
    assert_eq!(base.offset - scale.offset, 0x38 - 0x18);
    let bias = read_lone(&lone(0, &spec.data(), |b, at| {
        b.put_bytes(at.offset_by(0x20).unwrap(), &1f32.to_be_bytes())
    }))
    .unwrap_err();
    assert_eq!(
        (bias.field, bias.kind),
        (
            "_colorBias",
            ErrorKind::Unsupported {
                value: 1f32.to_bits().into()
            }
        )
    );
    assert_eq!(base.offset - bias.offset, 0x38 - 0x20);
    let nan = read_lone(&lone(0, &spec.data(), |b, at| {
        b.put_bytes(at.offset_by(0x2C).unwrap(), &f32::NAN.to_be_bytes())
    }));
    assert_eq!(nan.unwrap_err().field, "_colorBias");
    let negative_zero = lone(0, &spec.data(), |b, at| {
        b.put_bytes(at.offset_by(0x24).unwrap(), &(-0f32).to_be_bytes())
    });
    assert!(read_lone(&negative_zero).is_ok());
    let padding = read_lone(&lone(0, &spec.data(), |b, at| {
        b.put_bytes(at.offset_by(0x44).unwrap(), &4u16.to_be_bytes())
    }))
    .unwrap_err();
    assert_eq!(
        (padding.field, padding.kind),
        ("_paddingDeprecated", ErrorKind::Unsupported { value: 4 })
    );
    assert_eq!(padding.offset - base.offset, 0x44 - 0x38);

    let dxt3 = Spec {
        format: "dxt3_tile_big_ps3",
        ..SPECS[0]
    };
    let unknown = lone_spec(&dxt3, &spec.data(), |_, _| {});
    let e = read_lone(&unknown).unwrap_err();
    assert_eq!(e.field, "_format");

    let width = lone(0, &spec.data(), |b, at| {
        b.put_bytes(at.offset_by(0x30).unwrap(), &12u16.to_be_bytes())
    });
    let e = read_lone(&width).unwrap_err();
    assert_eq!(
        (e.field, e.kind),
        ("_width", ErrorKind::Unsupported { value: 12 })
    );
    assert_eq!(e.offset + 8, base.offset);

    // A block one chain too long is refused where the chain ends.
    let long = lone(0, &[spec.data(), vec![0; 128]].concat(), |_, _| {});
    let e = read_lone(&long).unwrap_err();
    assert_eq!(e.field, "_data");
    assert_eq!(e.kind, ErrorKind::Unsupported { value: 256 });
    assert_eq!(&long[e.offset - 128..e.offset], &spec.data()[..]);
}

fn ktx2_format(f: Format) -> ktx2::Format {
    match f {
        Format::Bc1RgbaUnorm => ktx2::Format::BC1_RGBA_UNORM_BLOCK,
        Format::Bc3Unorm => ktx2::Format::BC3_UNORM_BLOCK,
        Format::R8Unorm => ktx2::Format::R8_UNORM,
    }
}

#[test]
fn ktx2_header_and_level_index_validate() {
    let built = images();
    let igz = Igz::parse(&built.bytes).unwrap();
    for (spec, &at) in SPECS.iter().zip(&built.images) {
        let t = Texture::read(&igz, igz.object_at(at).unwrap().0).unwrap();
        let file = t.to_ktx2();
        let r = ktx2::Reader::new(&file[..]).unwrap_or_else(|e| panic!("{}: {e:?}", spec.format));
        let h = r.header();
        assert_eq!(h.format, Some(ktx2_format(t.format)), "{}", spec.format);
        assert_eq!(h.format.unwrap().value(), t.format.vk_format());
        assert_eq!(
            (h.type_size, h.pixel_width, h.pixel_height, h.pixel_depth),
            (1, t.width, t.height, 0)
        );
        assert_eq!((h.layer_count, h.face_count), (0, 1));
        assert_eq!(h.level_count as usize, t.levels.len());
        assert_eq!(h.supercompression_scheme, None);
        assert_eq!((h.index.sgd_byte_offset, h.index.sgd_byte_length), (0, 0));
        // The DFD follows the level index, then the KVD.
        assert_eq!(h.index.dfd_byte_offset as usize, 80 + 24 * t.levels.len());
        assert_eq!(
            h.index.kvd_byte_offset,
            h.index.dfd_byte_offset + h.index.dfd_byte_length
        );
        let levels: Vec<&[u8]> = r.levels().map(|l| l.data).collect();
        assert_eq!(
            levels,
            t.levels.iter().map(Vec::as_slice).collect::<Vec<_>>()
        );
        for l in r.levels() {
            assert_eq!(l.uncompressed_byte_length as usize, l.data.len());
        }
        // Each level aligned to lcm(block bytes, 4), smallest first.
        let align = (t.format.block_bytes() as usize).max(4);
        let mut last = file.len();
        for i in 0..t.levels.len() {
            let entry = 80 + 24 * i;
            let off = u64::from_le_bytes(file[entry..entry + 8].try_into().unwrap()) as usize;
            assert_eq!(off % align, 0, "{} level {i}", spec.format);
            assert!(
                off < last,
                "{} level {i} not before level {}",
                spec.format,
                i + 1
            );
            last = off;
        }
        // The smallest level starts past the KVD; level 0 ends the file.
        assert!(last >= (h.index.kvd_byte_offset + h.index.kvd_byte_length) as usize);
        assert_eq!(
            u64::from_le_bytes(file[80..88].try_into().unwrap()) as usize + t.levels[0].len(),
            file.len()
        );
        // The DFD is the one the KTX2 spec gives the format.
        let (want, type_size) = ktx2::dfd::Basic::from_format(ktx2_format(t.format)).unwrap();
        assert_eq!(r.basic_dfd(), Some(&want), "{}", spec.format);
        assert_eq!(type_size, h.type_size);
        let kv: Vec<(&str, &[u8])> = r.key_value_data().collect();
        assert_eq!(r.writer(), Some("guac-formats\0"));
        let swizzle = kv.iter().find(|(k, _)| *k == "KTXswizzle").map(|(_, v)| *v);
        if t.format == Format::R8Unorm {
            assert_eq!(swizzle, Some(&b"rrr1\0"[..]));
        } else {
            assert_eq!(swizzle, None);
        }
    }
}

#[test]
fn ktx2_dfd_bytes_match_the_spec_generator() {
    for f in [Format::Bc1RgbaUnorm, Format::Bc3Unorm, Format::R8Unorm] {
        let ours = ktx::dfd(f);
        let (want, _) = ktx2::dfd::Basic::from_format(ktx2_format(f)).unwrap();
        let want = want.to_vec();
        // The crate's bytes skip the block header.
        assert_eq!(&ours[12..], &want[..], "{f:?}");
        let samples = want.len() / 16 - 1;
        let size = (24 + 16 * samples) as u16;
        assert_eq!(
            &ours[4..12],
            &[&[0, 0, 0, 0, 2, 0][..], &size.to_le_bytes()].concat()[..]
        );
        assert_eq!(
            u32::from_le_bytes(ours[..4].try_into().unwrap()) as usize,
            ours.len()
        );
    }
}

/// One BC1 colour half: endpoints, then texel i's 2-bit index at bit 2i.
fn bc1(c0: u16, c1: u16, indexes: [u8; 16]) -> Vec<u8> {
    let bits = indexes
        .iter()
        .enumerate()
        .fold(0u32, |b, (i, &x)| b | u32::from(x) << (2 * i));
    [c0.to_le_bytes(), c1.to_le_bytes()]
        .concat()
        .into_iter()
        .chain(bits.to_le_bytes())
        .collect()
}

/// One BC3 alpha half: endpoints, then texel i's 3-bit index at bit 3i.
fn bc3_alpha(a0: u8, a1: u8, indexes: [u8; 16]) -> Vec<u8> {
    let bits = indexes
        .iter()
        .enumerate()
        .fold(0u64, |b, (i, &x)| b | u64::from(x) << (3 * i));
    [a0, a1]
        .into_iter()
        .chain(bits.to_le_bytes()[..6].to_vec())
        .collect()
}

fn texture(format: Format, width: u32, height: u32, level: Vec<u8>) -> Texture {
    Texture {
        format,
        width,
        height,
        levels: vec![level],
    }
}

/// The RGBA8 texel at (x, y) of a `width`-wide decode.
fn px(rgba: &[u8], width: usize, x: usize, y: usize) -> [u8; 4] {
    rgba[(y * width + x) * 4..][..4].try_into().unwrap()
}

/// Indexes 0, 1, 2, 3 along every row.
const RAMP: [u8; 16] = [0, 1, 2, 3, 0, 1, 2, 3, 0, 1, 2, 3, 0, 1, 2, 3];

#[test]
fn bc1_four_colours_widen_and_round() {
    // Red over blue: c0 > c1, four opaque colours.
    let t = texture(Format::Bc1RgbaUnorm, 4, 4, bc1(0xF800, 0x001F, RAMP));
    let rgba = t.rgba8(0).unwrap();
    assert_eq!(rgba.len(), 4 * 4 * 4);
    let row: Vec<[u8; 4]> = (0..4).map(|x| px(&rgba, 4, x, 2)).collect();
    assert_eq!(
        row,
        [
            [255, 0, 0, 255],
            [0, 0, 255, 255],
            [170, 0, 85, 255],
            [85, 0, 170, 255]
        ]
    );
    // 565 widens by bit replication: 1, 2, 1 become 8, 8, 8.
    let t = texture(Format::Bc1RgbaUnorm, 4, 4, bc1(0x0841, 0, [0; 16]));
    assert_eq!(px(&t.rgba8(0).unwrap(), 4, 3, 3), [8, 8, 8, 255]);
}

#[test]
fn bc1_three_colours_keep_transparent_black() {
    // Blue over red: c0 <= c1, the midpoint and transparent black.
    let t = texture(Format::Bc1RgbaUnorm, 4, 4, bc1(0x001F, 0xF800, RAMP));
    let rgba = t.rgba8(0).unwrap();
    let row: Vec<[u8; 4]> = (0..4).map(|x| px(&rgba, 4, x, 0)).collect();
    assert_eq!(
        row,
        [
            [0, 0, 255, 255],
            [255, 0, 0, 255],
            [128, 0, 128, 255],
            [0, 0, 0, 0]
        ]
    );
}

#[test]
fn bc3_alpha_steps_and_its_colour_half_always_four() {
    let colour = bc1(0x001F, 0xF800, RAMP);
    // Eight steps when a0 > a1: index 2 is (6 * 255 + 0) / 7, rounded.
    let mut idx = [0u8; 16];
    idx[..8].copy_from_slice(&[0, 1, 2, 3, 4, 5, 6, 7]);
    let t = texture(
        Format::Bc3Unorm,
        4,
        4,
        [bc3_alpha(255, 0, idx), colour.clone()].concat(),
    );
    let rgba = t.rgba8(0).unwrap();
    let alphas: Vec<u8> = (0..8).map(|i| px(&rgba, 4, i % 4, i / 4)[3]).collect();
    assert_eq!(alphas, [255, 0, 219, 182, 146, 109, 73, 36]);
    // c0 <= c1 is still four colours in BC3: index 3 is a third of the way.
    assert_eq!(px(&rgba, 4, 3, 0), [170, 0, 85, 182]);
    // Six steps plus 0 and 255 when a0 <= a1.
    let t = texture(
        Format::Bc3Unorm,
        4,
        4,
        [bc3_alpha(0, 255, idx), colour].concat(),
    );
    let rgba = t.rgba8(0).unwrap();
    let alphas: Vec<u8> = (0..8).map(|i| px(&rgba, 4, i % 4, i / 4)[3]).collect();
    assert_eq!(alphas, [0, 255, 51, 102, 153, 204, 0, 255]);
}

#[test]
fn blocks_land_in_rows_and_partial_blocks_crop() {
    // 8x4: a red block, then a blue one.
    let level = [bc1(0xF800, 0, [0; 16]), bc1(0x001F, 0, [0; 16])].concat();
    let rgba = texture(Format::Bc1RgbaUnorm, 8, 4, level).rgba8(0).unwrap();
    let red = [255, 0, 0, 255];
    let blue = [0, 0, 255, 255];
    let row: Vec<[u8; 4]> = (0..8).map(|x| px(&rgba, 8, x, 3)).collect();
    assert_eq!(row, [red, red, red, red, blue, blue, blue, blue]);
    // A 2x2 level keeps the block's top-left four texels.
    let t = Texture {
        format: Format::Bc1RgbaUnorm,
        width: 4,
        height: 4,
        levels: vec![bc1(0, 0, [0; 16]), bc1(0xF800, 0x001F, RAMP)],
    };
    let rgba = t.rgba8(1).unwrap();
    assert_eq!(rgba.len(), 2 * 2 * 4);
    assert_eq!([px(&rgba, 2, 0, 1), px(&rgba, 2, 1, 1)], [red, blue]);
}

#[test]
fn l8_reads_as_grey_and_short_levels_are_refused() {
    let t = texture(Format::R8Unorm, 3, 1, vec![0, 128, 255]);
    assert_eq!(
        t.rgba8(0).unwrap(),
        [0, 0, 0, 255, 128, 128, 128, 255, 255, 255, 255, 255]
    );
    let e = t.rgba8(1).unwrap_err();
    assert_eq!(
        (e.field, e.kind),
        ("levels", ErrorKind::OutOfRange { value: 1, limit: 1 })
    );
    let short = texture(Format::Bc3Unorm, 8, 4, vec![0; 31]);
    let e = short.rgba8(0).unwrap_err();
    assert_eq!(
        (e.field, e.kind),
        (
            "_data",
            ErrorKind::Truncated {
                needed: 32,
                available: 31
            }
        )
    );
    // A huge level with no data is refused before anything is allocated.
    let e = texture(Format::Bc1RgbaUnorm, u32::MAX, u32::MAX, vec![])
        .rgba8(0)
        .unwrap_err();
    assert_eq!(e.field, "_data");
}

#[test]
fn builder_images_decode_level_zero() {
    let built = images();
    let igz = Igz::parse(&built.bytes).unwrap();
    for (spec, &at) in SPECS.iter().zip(&built.images) {
        let (index, _) = igz.object_at(at).unwrap();
        let t = Texture::read(&igz, index).unwrap();
        let rgba = t.rgba8(0).unwrap();
        assert_eq!(
            rgba.len(),
            usize::from(spec.width) * usize::from(spec.height) * 4,
            "{}",
            spec.format
        );
    }
}
