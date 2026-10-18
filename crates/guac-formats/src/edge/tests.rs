use super::builder::{self, Fixture, Wire, cmp, cmp_value, desc, file, sample, skinned, unskinned};
use super::indexes::{compress, pack};
use super::*;
use crate::igz::Igz;

/// Triangles rotated to start at their smallest index.
fn canon(tris: impl IntoIterator<Item = [u16; 3]>) -> Vec<[u16; 3]> {
    tris.into_iter()
        .map(|t| {
            let r = (0..3).min_by_key(|&r| t[r]).unwrap();
            [t[r], t[(r + 1) % 3], t[(r + 2) % 3]]
        })
        .collect()
}

/// A stream assembled bit by bit, independent of the encoder.
fn assemble(explicit: &[u32], bias: u16, width: u8, first_use: &[u32], codes: &[u32]) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend((explicit.len() as u16).to_be_bytes());
    out.extend(bias.to_be_bytes());
    out.extend((first_use.len().div_ceil(8) as u16).to_be_bytes());
    out.extend([width, 0]);
    out.extend(pack(first_use, 1));
    out.extend(pack(codes, 2));
    out.extend(pack(explicit, width.into()));
    out.resize(out.len().next_multiple_of(16), 0);
    out
}

#[test]
fn a_hand_assembled_stream_decodes_every_code() {
    // (0,1,2) code 3, (0,2,3) code 0, (3,2,4) code 1, (2,3,0) code 2 taking
    // explicit 0, then (1,4,5) code 3 taking explicit 1 and 4 and new 5.
    let s = assemble(
        &[0, 1, 4],
        0,
        3,
        &[0, 0, 0, 0, 0, 1, 1, 1, 0],
        &[3, 0, 1, 2, 3],
    );
    assert_eq!(
        s,
        [
            0, 3, 0, 0, 0, 2, 3, 0, 0x07, 0x00, 0xC6, 0xC0, 0x06, 0x00, 0, 0
        ]
    );
    let d = decompress(&s, 15, 6).unwrap();
    assert_eq!(d.indexes, [0, 1, 2, 0, 2, 3, 3, 2, 4, 2, 3, 0, 1, 4, 5]);
    assert_eq!(d.codes, [1, 1, 1, 2]);
    assert_eq!((d.explicit_count, d.bias, d.width), (3, 0, 3));
}

#[test]
fn explicit_indexes_run_in_eight_delta_lanes_under_the_bias() {
    // 12 new vertices, then three code-3 triangles of explicit indexes
    // 1,2,3 5,6,7 8,4,0: the ninth delta is 0 - 1, so bias 1.
    let explicit = [1, 2, 3, 5, 6, 7, 8, 4, 0];
    let raw: Vec<u32> = explicit
        .iter()
        .enumerate()
        .map(|(i, &v)| {
            let back = if i >= 8 { explicit[i - 8] } else { 0 };
            (v - back + 1) as u32
        })
        .collect();
    assert_eq!(raw, [2, 3, 4, 6, 7, 8, 9, 5, 0]);
    let first_use: Vec<u32> = [vec![0; 12], vec![1; 9]].concat();
    let s = assemble(&raw, 1, 4, &first_use, &[3; 7]);
    let d = decompress(&s, 21, 12).unwrap();
    assert_eq!(&d.indexes[12..], [1, 2, 3, 5, 6, 7, 8, 4, 0]);
    // Re-encoding the decoded triangles gives back the stream byte for byte.
    let tris: Vec<[u16; 3]> = d.triangles().collect();
    let c = compress(&tris).unwrap();
    assert_eq!(c, s);
}

#[test]
fn width_zero_is_one_explicit_zero() {
    // (0,1,2), (2,1,3) code 1, then (3,1,0) code 1 taking explicit 0.
    let s = assemble(&[0], 0, 0, &[0, 0, 0, 0, 1], &[3, 1, 1]);
    let d = decompress(&s, 9, 4).unwrap();
    assert_eq!(d.indexes, [0, 1, 2, 2, 1, 3, 3, 1, 0]);
    assert_eq!(d.width, 0);
}

#[test]
fn random_lists_round_trip_through_the_reference_encoder() {
    // xorshift, so the trials are fixed.
    let mut state = 0x9E37_79B9_7F4A_7C15u64;
    let mut next = |n: u64| {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        state % n.max(1)
    };
    let mut trials = 0;
    for _ in 0..200 {
        let vertexes = 3 + next(400) as u16;
        let (mut tris, mut used, mut prev) = (Vec::new(), 0u16, None::<[u16; 3]>);
        for _ in 0..1 + next(600) {
            if let Some([a, b, c]) = prev.filter(|_| next(10) < 7) {
                let e = [(a, c), (c, b), (b, a)][next(3) as usize];
                let v = if used < vertexes && next(10) < 6 {
                    used
                } else {
                    next(used.max(1).into()) as u16
                };
                if v == e.0 || v == e.1 {
                    continue;
                }
                used += u16::from(v == used);
                prev = Some([e.0, e.1, v]);
            } else {
                let mut fresh = used;
                let mut t = [0; 3];
                for x in &mut t {
                    *x = if fresh < vertexes && next(2) == 0 {
                        fresh += 1;
                        fresh - 1
                    } else {
                        next(fresh.max(1).into()) as u16
                    };
                }
                if t[0] == t[1] || t[1] == t[2] || t[0] == t[2] {
                    continue;
                }
                used = fresh;
                prev = Some(t);
            }
            tris.push(prev.unwrap());
        }
        if tris.is_empty() {
            continue;
        }
        let s = compress(&tris).unwrap();
        let d = decompress(&s, 3 * tris.len() as u16, used).unwrap();
        assert_eq!(canon(d.triangles()), canon(tris.iter().copied()));
        trials += 1;
    }
    assert!(trials > 150, "{trials}");
}

fn refusal(s: &[u8], indexes: u16, vertexes: u16) -> (usize, &'static str) {
    let e = decompress(s, indexes, vertexes).unwrap_err();
    (e.offset, e.field)
}

#[test]
fn malformed_index_streams_are_refused() {
    let good = assemble(
        &[0, 1, 4],
        0,
        3,
        &[0, 0, 0, 0, 0, 1, 1, 1, 0],
        &[3, 0, 1, 2, 3],
    );
    decompress(&good, 15, 6).unwrap();
    let with = |at: usize, v: u8| {
        let mut s = good.clone();
        s[at] = v;
        s
    };
    assert_eq!(refusal(&good[..7], 15, 6), (0, "index header"));
    assert_eq!(refusal(&with(7, 1), 15, 6), (7, "index header"));
    assert_eq!(refusal(&with(6, 17), 15, 6), (6, "explicit width"));
    assert_eq!(refusal(&good, 14, 6), (0, "numIndexes"));
    // Sections past the end; more explicit indexes than first-use bits.
    assert_eq!(refusal(&good[..13], 15, 6), (0, "index sections"));
    let mut s = with(1, 17);
    s[6] = 0;
    assert_eq!(refusal(&s, 15, 6), (0, "explicit count"));
    // The stream introduces six vertices: the sixth's bit (byte 9) overruns
    // five, and seven are never all used.
    assert_eq!(refusal(&good, 15, 5), (9, "first-use bits"));
    assert_eq!(refusal(&good, 15, 7), (8, "first-use bits"));
    // A first code other than 3.
    assert_eq!(refusal(&with(10, 0x06), 15, 6), (10, "triangle code"));
    // Explicit 4 made 6: a forward reference.
    assert_eq!(refusal(&with(12, 0x07), 15, 6), (12, "explicit index"));
    // A set pad bit in each section, then in the tail.
    assert_eq!(refusal(&with(9, 0x40), 15, 6), (9, "first-use bits"));
    assert_eq!(refusal(&with(11, 0xC1), 15, 6), (11, "triangle codes"));
    assert_eq!(refusal(&with(13, 0x01), 15, 6), (13, "explicit indexes"));
    assert_eq!(refusal(&with(15, 1), 15, 6), (15, "index tail"));
    // 16 zero bytes past the end are more than Giants pads.
    let long = [&good[..], &[0; 16]].concat();
    assert_eq!(refusal(&long, 15, 6), (14, "index tail"));
    // A first-use byte count one too many.
    let mut s = assemble(
        &[0, 1, 4],
        0,
        3,
        &[0, 0, 0, 0, 0, 1, 1, 1, 0],
        &[3, 0, 1, 2, 3],
    );
    s[5] = 3;
    s.insert(10, 0);
    assert_eq!(refusal(&s, 15, 6), (4, "first-use bytes"));
    // An explicit index header count with one left over.
    let s = assemble(
        &[0, 1, 4, 0],
        0,
        3,
        &[0, 0, 0, 0, 0, 1, 1, 1, 0],
        &[3, 0, 1, 2, 3],
    );
    assert_eq!(refusal(&s, 15, 6), (0, "explicit count"));
}

#[test]
fn cmp_words_decode_signed_normalized() {
    for [x, y, z] in [
        [0, 0, 0],
        [1023, -1023, 511],
        [-1024, 5, -512],
        [300, -2, 17],
        [-1, 1, -1],
    ] {
        assert_eq!(x11y11z10n(cmp(x, y, z)), cmp_value(x, y, z), "{x} {y} {z}");
    }
    // Bits by hand: x = 1 (bit 0), y = -1 (bits 11-21), z = 1 (bit 22).
    let w = 1 | (0x7FF << 11) | (1 << 22);
    assert_eq!(x11y11z10n(w), [1.0 / 1023.0, -1.0 / 1023.0, 1.0 / 511.0]);
}

#[test]
fn halves_decode_exactly() {
    for (bits, value) in [
        (0x3C00, 1.0f32),
        (0xC000, -2.0),
        (0x3555, 0.333_251_95),
        (0x0001, 2f32.powi(-24)),
        (0x03FF, 1023.0 * 2f32.powi(-24)),
        (0x0400, 2f32.powi(-14)),
        (0x7BFF, 65504.0),
    ] {
        assert_eq!(f16_to_f32(bits), value, "{bits:#x}");
    }
    assert!(f16_to_f32(0x8000).is_sign_negative());
    assert_eq!(f16_to_f32(0x7C00), f32::INFINITY);
    assert_eq!(f16_to_f32(0xFC00), f32::NEG_INFINITY);
    assert!(f16_to_f32(0x7E00).is_nan());
}

#[test]
fn only_the_two_configs_are_read() {
    for kind in [Kind::Unskinned, Kind::Skinned] {
        let c = SpuConfig {
            kind,
            command_hole: 9,
            num_vertexes: 300,
            num_indexes: 600,
        };
        assert_eq!(SpuConfig::parse(&c.to_bytes()), Ok(c));
        for at in [0, 2, 3, 4, 5, 7, 12, 13, 14, 15] {
            let mut b = c.to_bytes();
            b[at] ^= 0x10;
            let e = SpuConfig::parse(&b).unwrap_err();
            assert_eq!((e.offset, e.field), (at, "_spuConfigInfo"), "{kind:?} {at}");
        }
        // Index flavour lives in byte 6's high nibble.
        let mut b = c.to_bytes();
        b[6] ^= 0x10;
        assert_eq!(SpuConfig::parse(&b).unwrap_err().offset, 6);
    }
    let mut b = Kind::Skinned.template();
    b[6] = 0x34;
    assert_eq!(SpuConfig::parse(&b).unwrap_err().field, "skin flavour");
    b[6] = 0x31;
    b[11] = 4;
    assert_eq!(SpuConfig::parse(&b).unwrap_err().offset, 10);
    assert!(SpuConfig::parse(&b[..15]).is_err());
    assert!(SpuConfig::parse(&[0; 17]).is_err());
}

fn stream_refusal(d: &[u8]) -> usize {
    let e = StreamDesc::parse(d).unwrap_err();
    assert_eq!(e.field, "stream descriptor");
    e.offset
}

#[test]
fn stream_descriptors_read_and_refuse_untested_formats() {
    let blocks = [
        builder::block(0, VertexFormat::X11Y11Z10N, 1, Semantic::Normal, 2),
        builder::block(4, VertexFormat::F16, 2, Semantic::Uv0, 8),
        builder::block(8, VertexFormat::U8N, 4, Semantic::Color, 3),
        builder::block(12, VertexFormat::F32, 2, Semantic::Uv1, 9),
    ];
    let d = desc(20, &blocks);
    let parsed = StreamDesc::parse(&d).unwrap();
    assert_eq!(parsed.stride, 20);
    assert_eq!(
        parsed
            .blocks
            .iter()
            .map(|b| (b.semantic, b.slot))
            .collect::<Vec<_>>(),
        [
            (Semantic::Normal, 2),
            (Semantic::Uv0, 8),
            (Semantic::Color, 3),
            (Semantic::Uv1, 9)
        ]
    );
    let with = |at: usize, v: u8| {
        let mut x = d.clone();
        x[at] = v;
        x
    };
    // I16N, I16, U8, FIXED_POINT, UNIT_VECTOR and an unknown code.
    for code in [1, 5, 7, 8, 9, 10, 0] {
        assert_eq!(stream_refusal(&with(9, code)), 9, "format {code}");
    }
    assert_eq!(stream_refusal(&with(10, 2)), 10, "CMP with two components");
    assert_eq!(stream_refusal(&with(11, 0)), 11, "semantic 0");
    assert_eq!(stream_refusal(&with(11, 10)), 11, "semantic 10");
    assert_eq!(stream_refusal(&with(19, 2)), 19, "a duplicate semantic");
    assert_eq!(stream_refusal(&with(12, 8)), 12, "size against format");
    assert_eq!(stream_refusal(&with(13, 16)), 13, "slot 16");
    assert_eq!(stream_refusal(&with(14, 1)), 14, "fixed block offset");
    assert_eq!(stream_refusal(&with(15, 1)), 15, "padding");
    assert_eq!(stream_refusal(&with(16, 2)), 16, "overlap");
    assert_eq!(stream_refusal(&with(1, 19)), 32, "overruns the stride");
    assert_eq!(stream_refusal(&with(2, 5)), 2, "fixed-point blocks");
    assert_eq!(stream_refusal(&with(5, 1)), 5, "header padding");
    assert_eq!(stream_refusal(&with(47, 1)), 47, "tail");
    assert!(StreamDesc::parse(&d[..30]).is_err());
}

fn read_one(w: &Wire) -> Result<Segment, crate::FormatError> {
    let (bytes, at) = file(w);
    let igz = Igz::parse(&bytes).unwrap();
    Segment::read(&igz, igz.object_at(at).unwrap().0)
}

fn check(f: &Fixture, s: &Segment) {
    let e = &f.expect;
    assert_eq!(s.vertex_count(), e.positions.len());
    assert_eq!(canon(s.triangles()), canon(e.tris.iter().copied()));
    assert_eq!(s.positions().collect::<Vec<_>>(), e.positions);
    let floats = |sem: Semantic| s.attribute(sem).unwrap().values.clone();
    let cmps = |v: &[[i32; 3]]| -> Vec<f32> {
        v.iter().flat_map(|&[x, y, z]| cmp_value(x, y, z)).collect()
    };
    assert_eq!(floats(Semantic::Normal), cmps(&e.normals));
    assert_eq!(floats(Semantic::Tangent), cmps(&e.tangents));
    let uv: Vec<f32> = e.uv0.iter().flatten().map(|(_, v)| *v).collect();
    let got = floats(Semantic::Uv0);
    assert_eq!(got, uv);
    // -0.0 equals 0.0 above; its sign survives too.
    assert_eq!(
        got.iter().map(|v| v.to_bits()).collect::<Vec<_>>(),
        uv.iter().map(|v| v.to_bits()).collect::<Vec<_>>()
    );
    if !e.colors.is_empty() {
        let c: Vec<f32> = e
            .colors
            .iter()
            .flatten()
            .map(|&b| f32::from(b) / 255.0)
            .collect();
        assert_eq!(floats(Semantic::Color), c);
    }
}

#[test]
fn the_unskinned_sample_decodes_to_its_values() {
    let f = unskinned();
    let s = read_one(&f.wire).unwrap();
    assert_eq!(s.config.kind, Kind::Unskinned);
    assert_eq!(s.config.command_hole, 5);
    assert!(s.skin.is_none());
    check(&f, &s);
    let slots: Vec<_> = s
        .attributes
        .iter()
        .map(|a| (a.semantic, a.stream, a.slot, a.components))
        .collect();
    assert_eq!(
        slots,
        [
            (Semantic::Position, Stream::Spu0, 0, 3),
            (Semantic::Normal, Stream::RsxOnly, 2, 3),
            (Semantic::Tangent, Stream::RsxOnly, 14, 3),
            (Semantic::Uv0, Stream::RsxOnly, 8, 2),
            (Semantic::Color, Stream::RsxOnly, 3, 4),
        ]
    );
    assert_eq!(s.bounds(), Some([[0.0, -4.0, 0.0], [2.5, 1.0, 1.25]]));
}

#[test]
fn the_skinned_sample_decodes_and_maps_to_bones() {
    let f = skinned();
    let s = read_one(&f.wire).unwrap();
    assert_eq!(s.config.kind, Kind::Skinned);
    check(&f, &s);
    let streams: Vec<_> = s
        .attributes
        .iter()
        .map(|a| (a.semantic, a.stream))
        .collect();
    assert_eq!(
        streams,
        [
            (Semantic::Position, Stream::Spu0),
            (Semantic::Normal, Stream::Spu1),
            (Semantic::Tangent, Stream::Spu1),
            (Semantic::Uv0, Stream::RsxOnly),
        ]
    );
    let skin = s.skin.as_ref().unwrap();
    assert_eq!(skin.palette_slots, f.expect.palette_slots);
    let got: Vec<[(u8, u8); 4]> = skin
        .influences
        .iter()
        .map(|v| v.map(|i| (i.weight, i.local)))
        .collect();
    assert_eq!(got, f.expect.influences);

    // Slots 2, 3 and 10 hold blend matrices 5, 0 and 7; bones claim them.
    let mut indices = vec![-1; 11];
    (indices[2], indices[3], indices[10]) = (5, 0, 7);
    let bone = |blend| SkeletonBone {
        name: None,
        parent_index: -1,
        blend_matrix_index: blend,
        translation: [0.0; 3],
    };
    let bones = [bone(0), bone(-1), bone(7), bone(5)];
    let map = BoneMap::new(&indices, &bones);
    assert_eq!(map.slots[2..4], [Some(3), Some(0)]);
    assert_eq!(map.slots[10], Some(2));
    let w = |bone, weight| BoneWeight { bone, weight };
    let z = BoneWeight::default();
    assert_eq!(
        skin.bones(&map).unwrap(),
        [
            [w(3, 200), w(2, 55), z, z],
            [w(0, 255), z, z, z],
            [w(3, 128), w(0, 127), z, z],
            [w(2, 100), w(0, 100), w(3, 55), z],
        ]
    );
    // A slot past the list, and a matrix two bones claim.
    let short = BoneMap::new(&indices[..10], &bones);
    assert_eq!(skin.bones(&short).unwrap_err().field, "_blendMatrixIndices");
    let twice = BoneMap::new(&indices, &[bone(0), bone(5), bone(7), bone(5)]);
    assert_eq!(twice.slots[2], None);
    assert_eq!(skin.bones(&twice).unwrap_err().field, "_blendMatrixIndex");
}

#[test]
fn palette_ranges_land_back_to_back() {
    assert_eq!(palette_slots([0, 0], [48 * 3, 0]), Ok(vec![0, 1, 2]));
    assert_eq!(palette_slots([96, 480], [96, 48]), Ok(vec![2, 3, 10]));
    assert_eq!(palette_slots([1, 0], [48, 0]), Err(0));
    assert_eq!(palette_slots([0, 0], [48, 47]), Err(3));
}

fn refused(w: &Wire) -> crate::FormatError {
    read_one(w).unwrap_err()
}

#[test]
fn skin_refusals_sit_at_their_bytes() {
    let f = skinned();
    let (bytes, at) = file(&f.wire);
    let igz = Igz::parse(&bytes).unwrap();
    let i = igz.object_at(at).unwrap().0;
    let skin_at = {
        let s = crate::igz::layouts::EdgeGeometrySegment::read(&igz, i).unwrap();
        let m = s.skin_indexes_and_weights.unwrap();
        igz.err_at(m.at, "", ErrorKind::Missing).offset
    };

    // A weighted local past the three matrices: vertex 1, influence 1.
    let mut w = f.wire.clone();
    let skin = w.skin.as_mut().unwrap();
    (skin[10], skin[11]) = (1, 3);
    skin[8] = 254;
    let e = refused(&w);
    assert_eq!(
        (e.field, e.offset),
        ("_skinIndexesAndWeights", skin_at + 11)
    );

    // Weights summing to 254 on vertex 2.
    let mut w = f.wire.clone();
    w.skin.as_mut().unwrap()[16] = 127;
    let e = refused(&w);
    assert_eq!(
        (e.field, e.offset),
        ("_skinIndexesAndWeights", skin_at + 16)
    );
    assert_eq!(e.kind, ErrorKind::Unsupported { value: 254 });

    // 16 bytes of slack, a size that doesn't add up, a matrix range off 48.
    let mut w = f.wire.clone();
    w.skin.as_mut().unwrap().resize(48, 0);
    w.skin_sizes = [32, 16];
    assert_eq!(refused(&w).offset, skin_at + 32);
    let mut w = f.wire.clone();
    w.skin_sizes = [24, 15];
    assert_eq!(refused(&w).field, "_skinIndexesAndWeightsSizes");
    let mut w = f.wire.clone();
    w.matrix_sizes = [96, 50];
    assert_eq!(refused(&w).field, "_skinMatricesSizes");
    let mut w = f.wire.clone();
    w.matrix_offsets = [97, 480];
    assert_eq!(refused(&w).field, "_skinMatricesByteOffsets");
    let mut w = f.wire.clone();
    w.skin = None;
    assert_eq!(refused(&w).kind, ErrorKind::Missing);
}

#[test]
fn segments_outside_the_two_configs_are_refused() {
    let u = || unskinned().wire;
    let s = || skinned().wire;
    let field = |w: Wire| refused(&w).field;

    let mut w = u();
    w.num_blend_shapes = 1;
    assert_eq!(field(w), "_numBlendShapes");
    let mut w = u();
    w.desc0 = Some(desc(12, &[]));
    assert_eq!(field(w), "_spuInputStreamDescs0");
    let mut w = u();
    w.output_desc = Some(desc(12, &[]));
    assert_eq!(field(w), "_spuOutputStreamDesc");
    let mut w = u();
    w.skin = s().skin;
    assert_eq!(field(w), "_skinIndexesAndWeights");
    let mut w = u();
    w.spu1 = s().spu1;
    assert_eq!(field(w), "_spuVertexes1");
    let mut w = u();
    w.indexes_sizes[0] += 1;
    assert_eq!(field(w), "_indexesSizes");
    let mut w = u();
    w.rsx_desc_size = 8;
    assert_eq!(field(w), "_rsxOnlyStreamDescSize");
    let mut w = u();
    w.rsx_size -= 1;
    assert_eq!(field(w), "_rsxOnlyVertexesSize");
    let mut w = u();
    w.rsx = None;
    assert_eq!(field(w), "_rsxOnlyVertexes");
    // Size and offset words of absent buffers.
    let mut w = u();
    w.desc_sizes[1] = 16;
    assert_eq!(field(w), "_spuInputStreamDescSizes");
    let mut w = u();
    w.matrix_offsets[1] = 48;
    assert_eq!(field(w), "_skinMatricesByteOffsets");
    let mut w = u();
    w.matrix_sizes[0] = 48;
    assert_eq!(field(w), "_skinMatricesSizes");
    let mut w = u();
    w.skin_sizes[0] = 8;
    assert_eq!(field(w), "_skinIndexesAndWeightsSizes");
    let mut w = u();
    (w.rsx, w.rsx_desc, w.rsx_desc_size) = (None, None, 0);
    assert_eq!(field(w), "_rsxOnlyVertexesSize");
    let mut w = u();
    (w.rsx, w.rsx_desc, w.rsx_size) = (None, None, 0);
    assert_eq!(field(w), "_rsxOnlyStreamDescSize");
    // A stream too short for its vertices.
    let mut w = u();
    w.spu0.truncate(60);
    let e = refused(&w);
    assert_eq!(
        (e.field, e.kind),
        (
            "_spuVertexes0",
            ErrorKind::Truncated {
                needed: 72,
                available: 60
            }
        )
    );
    // SPU streams may carry under a quadword of tail; RSX-only none.
    let mut w = u();
    w.spu0.extend([0; 16]);
    let e = refused(&w);
    assert_eq!(
        (e.field, e.kind),
        ("_spuVertexes0", ErrorKind::Unsupported { value: 88 })
    );
    let mut w = u();
    w.spu0.extend([0; 15]);
    read_one(&w).unwrap();
    let mut w = u();
    w.rsx.as_mut().unwrap().push(0);
    w.rsx_size += 1;
    assert_eq!(field(w), "_rsxOnlyVertexes");
    // Positions again in the RSX-only stream.
    let mut w = u();
    let mut d = w.rsx_desc.clone().unwrap();
    d[8 + 3] = Semantic::Position.id();
    w.rsx_desc = Some(d);
    assert_eq!(refused(&w).kind, ErrorKind::Duplicate);
    // The secondary stream's descriptor and its size word.
    let mut w = s();
    w.desc1 = None;
    assert_eq!(field(w), "_spuInputStreamDescs1");
    let mut w = s();
    w.desc_sizes[1] = 8;
    assert_eq!(field(w), "_spuInputStreamDescSizes");
    // A config mismatch reports the config byte's file offset.
    let mut w = u();
    w.config[3] = 0xFF;
    let e = refused(&w);
    assert_eq!(e.field, "_spuConfigInfo");
    let (bytes, at) = file(&u());
    let igz = Igz::parse(&bytes).unwrap();
    let seg =
        crate::igz::layouts::EdgeGeometrySegment::read(&igz, igz.object_at(at).unwrap().0).unwrap();
    let config_at = igz
        .err_at(seg.spu_config_info.unwrap().at, "", ErrorKind::Missing)
        .offset;
    assert_eq!(e.offset, config_at + 3);
}

#[test]
fn the_sample_file_holds_both() {
    let s = sample();
    let igz = Igz::parse(&s.bytes).unwrap();
    let kinds: Vec<Kind> = s
        .segments
        .iter()
        .map(|&at| {
            Segment::read(&igz, igz.object_at(at).unwrap().0)
                .unwrap()
                .config
                .kind
        })
        .collect();
    assert_eq!(kinds, [Kind::Unskinned, Kind::Skinned]);
    // Another type is refused by the layout.
    let (bytes, _) = file(&unskinned().wire);
    let igz = Igz::parse(&bytes).unwrap();
    assert!(Segment::read(&igz, 99).is_err());
}

#[test]
fn shared_buffers_decode_once_each() {
    let bytes = builder::shared(3, 40);
    let igz = Igz::parse(&bytes).unwrap();
    let n = crate::igz::layouts::read_all::<crate::igz::layouts::EdgeGeometrySegment>(&igz)
        .filter(|(i, _)| Segment::read(&igz, *i).is_ok())
        .count();
    assert_eq!(n, 3);
}
