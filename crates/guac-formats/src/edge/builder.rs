//! Synthetic Edge segments in an IGZ, for tests and fuzz seeds
//! (`test-fixtures`): one unskinned and one skinned, every value known.

use super::indexes::compress;
use super::{Kind, Semantic, SpuConfig, VertexFormat};
use crate::igz::builder::IgzBuilder;
use crate::igz::{Location, MEMORY_PRESENT};

/// An RSX CMP word from signed x (11 bits), y (11) and z (10).
pub fn cmp(x: i32, y: i32, z: i32) -> u32 {
    (x as u32 & 0x7FF) | ((y as u32 & 0x7FF) << 11) | ((z as u32 & 0x3FF) << 22)
}

/// What [`cmp`] packs, decoded: x and y over 1023, z over 511, floor -1.
pub fn cmp_value(x: i32, y: i32, z: i32) -> [f32; 3] {
    [
        (x as f32 / 1023.0).max(-1.0),
        (y as f32 / 1023.0).max(-1.0),
        (z as f32 / 511.0).max(-1.0),
    ]
}

/// One descriptor block.
pub fn block(offset: u8, format: VertexFormat, count: u8, semantic: Semantic, slot: u8) -> [u8; 8] {
    let size = format.size(count) as u8;
    [
        offset,
        format.code(),
        count,
        semantic.id(),
        size,
        slot,
        0,
        0,
    ]
}

/// A descriptor, zero-padded to 16 bytes as Giants' are.
pub fn desc(stride: u8, blocks: &[[u8; 8]]) -> Vec<u8> {
    let n = blocks.len() as u8;
    let mut out = vec![n, stride, n, 0, 0, 0, 0, 0];
    out.extend(blocks.iter().flatten());
    out.resize(out.len().next_multiple_of(16), 0);
    out
}

/// A segment's fields as stored; size fields are separate so tests can
/// break them.
#[derive(Debug, Clone)]
pub struct Wire {
    pub config: Vec<u8>,
    pub indexes: Vec<u8>,
    pub indexes_sizes: [u16; 2],
    pub spu0: Vec<u8>,
    pub spu1: Option<Vec<u8>>,
    pub desc0: Option<Vec<u8>>,
    pub desc1: Option<Vec<u8>>,
    pub desc_sizes: [u16; 2],
    pub output_desc: Option<Vec<u8>>,
    pub output_desc_size: u16,
    pub rsx: Option<Vec<u8>>,
    pub rsx_size: u32,
    pub rsx_desc: Option<Vec<u8>>,
    pub rsx_desc_size: u16,
    pub matrix_offsets: [u16; 2],
    pub matrix_sizes: [u16; 2],
    pub skin: Option<Vec<u8>>,
    pub skin_sizes: [u16; 2],
    pub num_blend_shapes: u32,
}

/// The values a [`Wire`] was made from.
#[derive(Debug, Clone)]
pub struct Expect {
    pub tris: Vec<[u16; 3]>,
    pub positions: Vec<[f32; 3]>,
    /// [`cmp`] inputs.
    pub normals: Vec<[i32; 3]>,
    pub tangents: Vec<[i32; 3]>,
    /// f16 bits, then the value they hold.
    pub uv0: Vec<[(u16, f32); 2]>,
    pub colors: Vec<[u8; 4]>,
    /// (weight, local) x 4.
    pub influences: Vec<[(u8, u8); 4]>,
    pub palette_slots: Vec<u16>,
}

#[derive(Debug, Clone)]
pub struct Fixture {
    pub wire: Wire,
    pub expect: Expect,
}

const UV: [(u16, f32); 9] = [
    (0x3C00, 1.0),
    (0x3800, 0.5),
    (0xB400, -0.25),
    (0x0000, 0.0),
    (0x4900, 10.0),
    (0x0001, 5.960_464_5e-8),
    (0x7BFF, 65504.0),
    (0x8000, -0.0),
    (0xC500, -5.0),
];

fn f32s(values: &[[f32; 3]]) -> Vec<u8> {
    values
        .iter()
        .flatten()
        .flat_map(|v| v.to_be_bytes())
        .collect()
}

fn config(kind: Kind, vertexes: usize, tris: usize) -> Vec<u8> {
    let c = SpuConfig {
        kind,
        command_hole: 5,
        num_vertexes: vertexes as u16,
        num_indexes: 3 * tris as u16,
    };
    c.to_bytes().to_vec()
}

fn positions(n: usize) -> Vec<[f32; 3]> {
    (0..n)
        .map(|i| [i as f32 * 0.5, 1.0 - i as f32, 0.25 * i as f32])
        .collect()
}

fn base(kind: Kind, tris: &[[u16; 3]], positions: &[[f32; 3]]) -> Wire {
    let indexes = compress(tris).expect("small deltas");
    let spu0 = f32s(positions);
    Wire {
        config: config(kind, positions.len(), tris.len()),
        indexes_sizes: [indexes.len() as u16, 0],
        indexes,
        spu0,
        spu1: None,
        desc0: None,
        desc1: None,
        desc_sizes: [0, 0],
        output_desc: None,
        output_desc_size: 0,
        rsx: None,
        rsx_size: 0,
        rsx_desc: None,
        rsx_desc_size: 0,
        matrix_offsets: [0, 0],
        matrix_sizes: [0, 0],
        skin: None,
        skin_sizes: [0, 0],
        num_blend_shapes: 0,
    }
}

impl Wire {
    fn set_rsx(&mut self, data: Vec<u8>, desc: Vec<u8>) {
        self.rsx_size = data.len() as u32;
        self.rsx_desc_size = desc.len() as u16;
        self.rsx = Some(data);
        self.rsx_desc = Some(desc);
    }
}

/// Six vertices, five triangles using every code; the RSX-only stream
/// holds normal, tangent, UV0 and colour, as on the props.
pub fn unskinned() -> Fixture {
    let tris = vec![[0, 1, 2], [0, 2, 3], [3, 2, 4], [2, 3, 0], [1, 4, 5]];
    let positions = positions(6);
    let normals = vec![
        [1023, 0, 0],
        [-1024, -1023, 0],
        [0, 0, 511],
        [0, 0, -512],
        [300, -2, 17],
        [0, 0, 0],
    ];
    let tangents = vec![
        [0, 1023, 0],
        [511, 511, 0],
        [-1, 1, -1],
        [0, -1024, 255],
        [1, 2, 3],
        [-300, 700, -100],
    ];
    let uv0: Vec<[(u16, f32); 2]> = (0..6).map(|i| [UV[i], UV[(i + 3) % 9]]).collect();
    let colors: Vec<[u8; 4]> = (0..6u8).map(|i| [i, 255 - i, 128, 17 * i]).collect();
    let mut wire = base(Kind::Unskinned, &tris, &positions);
    let mut rsx = Vec::new();
    for i in 0..6 {
        let [n, t] = [normals[i], tangents[i]].map(|[x, y, z]| cmp(x, y, z));
        rsx.extend(n.to_be_bytes());
        rsx.extend(t.to_be_bytes());
        rsx.extend(uv0[i].iter().flat_map(|(bits, _)| bits.to_be_bytes()));
        rsx.extend(colors[i]);
    }
    let d = desc(
        16,
        &[
            block(0, VertexFormat::X11Y11Z10N, 1, Semantic::Normal, 2),
            block(4, VertexFormat::X11Y11Z10N, 1, Semantic::Tangent, 14),
            block(8, VertexFormat::F16, 2, Semantic::Uv0, 8),
            block(12, VertexFormat::U8N, 4, Semantic::Color, 3),
        ],
    );
    wire.set_rsx(rsx, d);
    Fixture {
        wire,
        expect: Expect {
            tris,
            positions,
            normals,
            tangents,
            uv0,
            colors,
            influences: Vec::new(),
            palette_slots: Vec::new(),
        },
    }
}

/// Four skinned vertices: normal and tangent in SPU input 1, UV0 alone in
/// the RSX-only stream, matrices from two ranges (slots 2, 3 and 10).
pub fn skinned() -> Fixture {
    let tris = vec![[0, 1, 2], [0, 2, 3]];
    let positions = positions(4);
    let normals = vec![[0, 1023, 0], [1023, 0, 0], [0, 0, -511], [-724, 0, 361]];
    let tangents = vec![[1023, 0, 0], [0, 0, 511], [0, 1023, 0], [0, -1023, 0]];
    let uv0: Vec<[(u16, f32); 2]> = (0..4).map(|i| [UV[i + 4], UV[i]]).collect();
    // The last vertex's zero weight names local 7, past the palette: kept.
    let influences = vec![
        [(200, 0), (55, 2), (0, 0), (0, 0)],
        [(255, 1), (0, 0), (0, 0), (0, 0)],
        [(128, 0), (127, 1), (0, 0), (0, 0)],
        [(100, 2), (100, 1), (55, 0), (0, 7)],
    ];
    let mut wire = base(Kind::Skinned, &tris, &positions);
    let spu1: Vec<u8> = normals
        .iter()
        .zip(&tangents)
        .flat_map(|(&[a, b, c], &[x, y, z])| [cmp(a, b, c), cmp(x, y, z)])
        .flat_map(u32::to_be_bytes)
        .collect();
    let d1 = desc(
        8,
        &[
            block(0, VertexFormat::X11Y11Z10N, 1, Semantic::Normal, 2),
            block(4, VertexFormat::X11Y11Z10N, 1, Semantic::Tangent, 14),
        ],
    );
    wire.desc_sizes = [0, d1.len() as u16];
    wire.desc1 = Some(d1);
    wire.spu1 = Some(spu1);
    let rsx: Vec<u8> = uv0
        .iter()
        .flatten()
        .flat_map(|(bits, _)| bits.to_be_bytes())
        .collect();
    wire.set_rsx(
        rsx,
        desc(4, &[block(0, VertexFormat::F16, 2, Semantic::Uv0, 8)]),
    );
    wire.matrix_offsets = [96, 480];
    wire.matrix_sizes = [96, 48];
    let mut skin: Vec<u8> = influences
        .iter()
        .flatten()
        .flat_map(|&(w, l)| [w, l])
        .collect();
    skin.resize(40, 0);
    wire.skin_sizes = [24, 16];
    wire.skin = Some(skin);
    Fixture {
        wire,
        expect: Expect {
            tris,
            positions,
            normals,
            tangents,
            uv0,
            colors: Vec::new(),
            influences,
            palette_slots: vec![2, 3, 10],
        },
    }
}

fn at(base: Location, offset: u32) -> Location {
    base.offset_by(offset).expect("inside the section")
}

fn u16s(b: &mut IgzBuilder, o: Location, offset: u32, values: &[u16]) {
    let bytes: Vec<u8> = values.iter().flat_map(|v| v.to_be_bytes()).collect();
    b.put_bytes(at(o, offset), &bytes);
}

/// Absent buffers get a null ref marked by kind 0xF, as Giants writes them.
fn mem(b: &mut IgzBuilder, o: Location, offset: u32, blocks: u8, data: Option<&[u8]>) {
    match data {
        Some(d) => {
            b.memory_ref(at(o, offset), blocks, d, 2);
        }
        None => b.null_memory_ref(at(o, offset), 0),
    }
}

/// `w` as an `igPS3EdgeGeometrySegment`, its buffers in section `blocks`.
pub fn segment(b: &mut IgzBuilder, blocks: u8, w: &Wire) -> Location {
    let o = b.object(0, "igPS3EdgeGeometrySegment", 0xB0);
    mem(b, o, 0x08, blocks, Some(&w.config));
    mem(b, o, 0x10, blocks, Some(&w.indexes));
    u16s(b, o, 0x18, &w.indexes_sizes);
    mem(b, o, 0x1C, blocks, Some(&w.spu0));
    mem(b, o, 0x24, blocks, w.spu1.as_deref());
    let len1 = w.spu1.as_ref().map_or(0, Vec::len) as u16;
    u16s(b, o, 0x2C, &[w.spu0.len() as u16, 0, 0, len1, 0, 0]);
    mem(b, o, 0x38, blocks, w.rsx.as_deref());
    b.put_u32(at(o, 0x40), w.rsx_size);
    u16s(b, o, 0x44, &w.matrix_offsets);
    u16s(b, o, 0x48, &w.matrix_sizes);
    u16s(b, o, 0x4C, &w.skin_sizes);
    mem(b, o, 0x50, blocks, w.skin.as_deref());
    mem(b, o, 0x60, blocks, w.desc0.as_deref());
    mem(b, o, 0x68, blocks, w.desc1.as_deref());
    mem(b, o, 0x70, blocks, w.output_desc.as_deref());
    mem(b, o, 0x78, blocks, w.rsx_desc.as_deref());
    u16s(b, o, 0x80, &w.desc_sizes);
    u16s(b, o, 0x84, &[w.output_desc_size, w.rsx_desc_size]);
    b.put_u32(at(o, 0x88), w.num_blend_shapes);
    for offset in [0x90, 0x98, 0xA4] {
        b.null_memory_ref(at(o, offset), 0);
    }
    o
}

/// One segment alone in an IGZ.
pub fn file(w: &Wire) -> (Vec<u8>, Location) {
    let mut b = IgzBuilder::new();
    let blocks = b.section(0x10);
    let o = segment(&mut b, blocks, w);
    (b.build(), o)
}

pub struct Sample {
    pub bytes: Vec<u8>,
    /// Unskinned, then skinned.
    pub segments: [Location; 2],
}

pub fn sample() -> Sample {
    let mut b = IgzBuilder::new();
    let blocks = b.section(0x10);
    let segments = [unskinned(), skinned()].map(|f| segment(&mut b, blocks, &f.wire));
    Sample {
        bytes: b.build(),
        segments,
    }
}

/// `count` segments whose memory refs all point at one segment's buffers:
/// `tris` triangles over 3 x `tris` vertices, so each decode is sizeable.
pub fn shared(count: usize, tris: u16) -> Vec<u8> {
    let list: Vec<[u16; 3]> = (0..tris).map(|t| [3 * t, 3 * t + 1, 3 * t + 2]).collect();
    let w = base(Kind::Unskinned, &list, &positions(3 * usize::from(tris)));
    let mut b = IgzBuilder::new();
    let blocks = b.section(0x10);
    let first = segment(&mut b, blocks, &w);
    let bytes = b.build();
    // Same buffers, so the walk budget, not the input size, bounds the work.
    let igz = crate::igz::Igz::parse(&bytes).expect("built");
    let words: Vec<u32> = (0..0xB0 / 4)
        .map(|i| igz.u32_at(at(first, 4 * i), "copy").expect("inside"))
        .collect();
    for _ in 1..count {
        let o = b.object(0, "igPS3EdgeGeometrySegment", 0xB0);
        for i in 2..0xB0 / 4 {
            b.put_u32(at(o, 4 * i), words[i as usize]);
        }
        for offset in [
            0x08, 0x10, 0x1C, 0x24, 0x38, 0x50, 0x60, 0x68, 0x70, 0x78, 0x90, 0x98, 0xA4,
        ] {
            let size = words[offset as usize / 4];
            if size & MEMORY_PRESENT != 0 {
                let target = Location(words[offset as usize / 4 + 1]);
                b.pointer(at(o, offset + 4), target);
            } else {
                b.marker(crate::igz::kind::NULL_MEMORY_REFS, at(o, offset));
            }
        }
    }
    b.build()
}

/// The shared set gives the fuzzer a start on aliased buffers.
pub fn seeds() -> Vec<Vec<u8>> {
    vec![
        sample().bytes,
        file(&unskinned().wire).0,
        file(&skinned().wire).0,
        shared(4, 40),
    ]
}
