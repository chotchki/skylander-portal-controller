//! Test-only RRC writer: builds synthetic captures in code so no real capture enters the repo.
//! It mirrors RPCS3's writer (shared id counter, batch tails with a zero header, blocks on the END).

use std::io::Write;

use flate2::Compression;
use flate2::write::GzEncoder;

use crate::capture::{
    CONSTANT_SLOTS, DISPLAY_BUFFERS_BYTES, PROGRAM_WORDS, REGISTER_WORDS, TILE_STATE_BYTES,
};

struct Cmd {
    header: u32,
    value: u32,
    memory: Vec<u64>,
    tile: u64,
}

pub struct Builder {
    pub version: u32,
    pub le_format: u32,
    pub registers: Vec<u32>,
    pub constants: Option<Vec<[u32; 4]>>,
    blocks: Vec<(u64, u32, u32, u64)>,
    data: Vec<(u64, Vec<u8>)>,
    cmds: Vec<Cmd>,
    next_id: u64,
    tile_id: u64,
}

fn varint(out: &mut Vec<u8>, mut v: usize) {
    loop {
        let byte = (v & 0x7f) as u8;
        v >>= 7;
        out.push(if v == 0 { byte } else { byte | 0x80 });
        if v == 0 {
            break;
        }
    }
}

impl Builder {
    pub fn new() -> Self {
        let mut b = Builder {
            version: 6,
            le_format: 1,
            registers: vec![0; REGISTER_WORDS],
            constants: None,
            blocks: Vec::new(),
            data: Vec::new(),
            cmds: Vec::new(),
            next_id: 0x1234,
            tile_id: 0,
        };
        b.tile_id = b.id();
        // RPCS3 opens every capture with a NOP carrying the first tile state, stored as the
        // bare register index (0x40, so count 0), not as a `reg << 2 | 1 << 18` header.
        b.cmds.push(Cmd {
            header: crate::nv4097::NO_OPERATION,
            value: 0,
            memory: Vec::new(),
            tile: b.tile_id,
        });
        b
    }

    fn id(&mut self) -> u64 {
        self.next_id += 1;
        self.next_id
    }

    pub fn set_reg(&mut self, reg: u32, value: u32) -> &mut Self {
        self.registers[reg as usize] = value;
        self
    }

    pub fn method(&mut self, reg: u32, value: u32) -> &mut Self {
        self.cmds.push(Cmd {
            header: (reg << 2) | (1 << 18),
            value,
            memory: Vec::new(),
            tile: 0,
        });
        self
    }

    /// A ranged upload: count in the leading header, the tail with zero headers.
    pub fn batch(&mut self, reg: u32, values: &[u32]) -> &mut Self {
        for (k, &value) in values.iter().enumerate() {
            let header = if k == 0 {
                (reg << 2) | ((values.len() as u32) << 18)
            } else {
                0
            };
            self.cmds.push(Cmd {
                header,
                value,
                memory: Vec::new(),
                tile: 0,
            });
        }
        self
    }

    /// Records a memory block and attaches it to the last command, the way a draw's END carries its inputs.
    pub fn block(&mut self, location: u32, offset: u32, bytes: &[u8]) -> &mut Self {
        let data_id = match self.data.iter().find(|(_, d)| d == bytes) {
            Some((id, _)) => *id,
            None => {
                let id = self.id();
                self.data.push((id, bytes.to_vec()));
                id
            }
        };
        let block_id = self.id();
        self.blocks.push((block_id, offset, location, data_id));
        self.cmds.last_mut().unwrap().memory.push(block_id);
        self
    }

    /// A data blob no block references, the way an earlier capture in the same RPCS3 process
    /// leaves one behind (its data map is never cleared).
    pub fn stale_blob(&mut self, bytes: &[u8]) -> &mut Self {
        let id = self.id();
        self.data.push((id, bytes.to_vec()));
        self
    }

    pub fn stream(&self) -> Vec<u8> {
        let mut s = Vec::new();
        s.extend_from_slice(b"RRC\0");
        s.extend_from_slice(&self.version.to_le_bytes());
        s.extend_from_slice(&self.le_format.to_le_bytes());

        varint(&mut s, 1);
        s.extend(std::iter::repeat_n(0xab, TILE_STATE_BYTES));
        s.extend_from_slice(&self.tile_id.to_le_bytes());

        varint(&mut s, self.blocks.len());
        for (id, offset, location, data_id) in &self.blocks {
            s.extend_from_slice(&offset.to_le_bytes());
            s.extend_from_slice(&location.to_le_bytes());
            s.extend_from_slice(&data_id.to_le_bytes());
            s.extend_from_slice(&id.to_le_bytes());
        }

        varint(&mut s, self.data.len());
        for (id, bytes) in &self.data {
            varint(&mut s, bytes.len());
            s.extend_from_slice(bytes);
            s.extend_from_slice(&id.to_le_bytes());
        }

        varint(&mut s, 1);
        s.extend(std::iter::repeat_n(0xcd, DISPLAY_BUFFERS_BYTES));
        s.extend_from_slice(&0x9999u64.to_le_bytes());

        varint(&mut s, self.cmds.len());
        for c in &self.cmds {
            s.extend_from_slice(&c.header.to_le_bytes());
            s.extend_from_slice(&c.value.to_le_bytes());
            varint(&mut s, c.memory.len());
            for id in &c.memory {
                s.extend_from_slice(&id.to_le_bytes());
            }
            s.extend_from_slice(&c.tile.to_le_bytes());
            s.extend_from_slice(&0u64.to_le_bytes());
        }

        for w in 0..PROGRAM_WORDS as u32 {
            s.extend_from_slice(&w.to_le_bytes());
        }
        if let Some(constants) = &self.constants {
            assert_eq!(constants.len(), CONSTANT_SLOTS);
            for w in constants.iter().flatten() {
                s.extend_from_slice(&w.to_le_bytes());
            }
        }
        for w in &self.registers {
            s.extend_from_slice(&w.to_le_bytes());
        }
        s
    }

    pub fn gzip(&self) -> Vec<u8> {
        let mut gz = GzEncoder::new(Vec::new(), Compression::fast());
        gz.write_all(&self.stream()).unwrap();
        gz.finish().unwrap()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::capture::{Capture, CaptureError};
    use crate::nv4097 as m;
    use crate::output::{DumpWriter, refuse_repo_output};
    use crate::replay::{Draw, DrawKind, ReplayStats, replay};

    fn run(b: &Builder) -> (Vec<Draw>, ReplayStats) {
        let capture = Capture::from_file_bytes(&b.gzip()).expect("synthetic capture parses");
        let mut draws = Vec::new();
        let stats = replay(&capture, |d| -> Result<(), ()> {
            draws.push(d);
            Ok(())
        })
        .unwrap();
        (draws, stats)
    }

    fn be16(values: &[u16]) -> Vec<u8> {
        values.iter().flat_map(|v| v.to_be_bytes()).collect()
    }

    /// 12 vertices of (x, y, z) f32, big-endian like RSX memory, each component distinct.
    fn positions() -> Vec<u8> {
        (0..12 * 3)
            .flat_map(|i| (i as f32 * 0.5).to_bits().to_be_bytes())
            .collect()
    }

    fn colors() -> Vec<u8> {
        (0..12 * 4).map(|i| i as u8).collect()
    }

    const BASE: u32 = 0x100;
    const POS_OFFSET: u32 = 0x0010_0000;
    const COLOR_OFFSET: u32 = 0x2000;
    const INDEX_OFFSET: u32 = 0x4000;

    fn vertex_setup(b: &mut Builder) {
        // Array 0 via the starting registers, array 3 via commands: both paths must land.
        b.set_reg(m::SET_VERTEX_DATA_ARRAY_FORMAT, 2 | (3 << 4) | (12 << 8))
            .set_reg(m::SET_VERTEX_DATA_ARRAY_OFFSET, POS_OFFSET)
            .set_reg(m::SET_VERTEX_DATA_ARRAY_FORMAT + 5, 2 | (4 << 8));
        b.method(m::SET_VERTEX_DATA_BASE_OFFSET, BASE)
            .method(m::SET_VERTEX_DATA_ARRAY_FORMAT + 3, 4 | (4 << 4) | (4 << 8))
            .method(
                m::SET_VERTEX_DATA_ARRAY_OFFSET + 3,
                0x8000_0000 | COLOR_OFFSET,
            )
            .method(m::SET_VERTEX_ATTRIB_INPUT_MASK, 1 | (1 << 3) | (1 << 5));
    }

    #[test]
    fn indexed_draw_vertex_and_index_ranges_land_at_the_right_offsets() {
        let mut b = Builder::new();
        vertex_setup(&mut b);
        // Slots 0..10; the draw reads slots 2..10, one of them the restart marker.
        let indices = [9u16, 9, 0, 1, 2, 0xffff, 3, 4, 5, 7];
        b.method(m::SET_INDEX_ARRAY_ADDRESS, INDEX_OFFSET)
            .method(m::SET_INDEX_ARRAY_DMA, 1 << 4)
            .method(m::SET_VERTEX_DATA_BASE_INDEX, 2)
            .method(m::SET_RESTART_INDEX_ENABLE, 1)
            .method(m::SET_RESTART_INDEX, 0xffff)
            .method(m::SET_BEGIN_END, 5)
            .method(m::DRAW_INDEX_ARRAY, (3 << 24) | 2)
            .method(m::DRAW_INDEX_ARRAY, (3 << 24) | 6)
            .method(m::SET_BEGIN_END, 0);
        // What RPCS3 records for this draw: each index range, then each array from min to max vertex.
        let pos = positions();
        let col = colors();
        b.block(0, INDEX_OFFSET + 4, &be16(&indices[2..6]))
            .block(0, INDEX_OFFSET + 12, &be16(&indices[6..10]))
            .block(
                0,
                BASE + POS_OFFSET + 2 * 12,
                &pos[2 * 12..2 * 12 + 7 * 12 + 12],
            )
            .block(
                1,
                BASE + COLOR_OFFSET + 2 * 4,
                &col[2 * 4..2 * 4 + 7 * 4 + 4],
            );

        let (draws, stats) = run(&b);
        assert_eq!(stats.draws, 1);
        assert_eq!(stats.unresolved_blocks, 0);
        assert_eq!(stats.orphan_commands, 1, "the capture-start NOP");
        let d = &draws[0];
        assert_eq!(d.kind, DrawKind::Indexed);
        assert!(!d.vertex_bindings_changed_mid_draw);
        assert_eq!(d.primitive_name, "triangles");

        let ia = d.index_array.as_ref().unwrap();
        assert_eq!(
            (ia.rsx_offset, ia.location, ia.type_name),
            (INDEX_OFFSET, 0, "u16")
        );
        assert_eq!((ia.first, ia.span, ia.index_count), (2, 8, 8));
        assert_eq!(ia.bytes, be16(&indices[2..10]));
        assert_eq!(ia.missing_bytes, 0);
        assert_eq!(ia.restart_index, Some(0xffff));
        assert_eq!((ia.min_vertex, ia.max_vertex), (Some(2), Some(9)));
        assert_eq!(d.vertex_count, 8);

        assert_eq!(
            d.vertex_arrays.iter().map(|v| v.slot).collect::<Vec<_>>(),
            [0, 3],
            "slot 5 is in the mask but has zero components"
        );
        let p = &d.vertex_arrays[0];
        assert_eq!(
            (p.rsx_offset, p.location, p.type_name),
            (BASE + POS_OFFSET, 0, "f32")
        );
        assert_eq!((p.components, p.stride, p.element_size), (3, 12, 12));
        assert_eq!((p.first_vertex, p.vertex_count), (2, 8));
        assert_eq!(p.bytes, &pos[2 * 12..10 * 12]);
        assert_eq!(p.missing_bytes, 0);
        let c = &d.vertex_arrays[1];
        assert_eq!(
            (c.rsx_offset, c.location, c.type_name),
            (BASE + COLOR_OFFSET, 1, "unorm8")
        );
        assert_eq!(c.bytes, &col[2 * 4..10 * 4]);
    }

    #[test]
    fn array_draw_covers_every_range() {
        let mut b = Builder::new();
        vertex_setup(&mut b);
        b.method(m::SET_BEGIN_END, 6)
            .method(m::DRAW_ARRAYS, (2 << 24) | 1)
            .method(m::DRAW_ARRAYS, (1 << 24) | 5)
            .method(m::SET_BEGIN_END, 0);
        let pos = positions();
        b.block(0, BASE + POS_OFFSET + 12, &pos[12..4 * 12]).block(
            0,
            BASE + POS_OFFSET + 5 * 12,
            &pos[5 * 12..7 * 12],
        );

        let (draws, _) = run(&b);
        let d = &draws[0];
        assert_eq!(d.kind, DrawKind::Arrays);
        assert_eq!(d.vertex_count, 5);
        let p = &d.vertex_arrays[0];
        assert_eq!((p.first_vertex, p.vertex_count), (1, 6));
        assert_eq!(&p.bytes[..3 * 12], &pos[12..4 * 12]);
        assert_eq!(&p.bytes[4 * 12..], &pos[5 * 12..7 * 12]);
        assert_eq!(
            p.missing_bytes, 0,
            "vertex 4 sits between the ranges, which no range reads"
        );
        assert_eq!(
            d.vertex_arrays[1].missing_bytes,
            5 * 4,
            "colors weren't captured at all: both ranges' vertices are missing"
        );
    }

    #[test]
    fn texture_unit_description_and_bytes_are_read() {
        let mut b = Builder::new();
        let unit = 2;
        let r = |base: u32| base + unit * m::TEXTURE_UNIT_STRIDE;
        let surface: Vec<u8> = (0..1344u32).map(|i| (i * 7) as u8).collect();
        // Unit 0 is fully described but not enabled, so it must not show up.
        b.set_reg(m::SET_TEXTURE_OFFSET, 0x500)
            .set_reg(m::SET_TEXTURE_FORMAT, 1 | (2 << 4) | (0x85 << 8));
        // Bit 3 set: CELL_GCM_TEXTURE_BORDER_COLOR, what every Giants unit carries.
        b.method(r(m::SET_TEXTURE_OFFSET), 0x0030_0000)
            .method(
                r(m::SET_TEXTURE_FORMAT),
                1 | (1 << 3) | (2 << 4) | (0xa6 << 8) | (3 << 16),
            )
            .method(r(m::SET_TEXTURE_IMAGE_RECT), (64 << 16) | 32)
            .method(m::SET_TEXTURE_CONTROL3 + unit, (1 << 20) | 128)
            .method(r(m::SET_TEXTURE_CONTROL1), 0xaae4)
            .method(r(m::SET_TEXTURE_CONTROL0), 0x8000_0000)
            .method(m::SET_BEGIN_END, 5)
            .method(m::DRAW_ARRAYS, 2 << 24)
            .method(m::SET_BEGIN_END, 0);
        b.block(0, 0x0030_0000, &surface);

        let (draws, _) = run(&b);
        let textures = &draws[0].textures;
        assert_eq!(textures.len(), 1);
        let t = &textures[0];
        assert_eq!((t.unit, t.offset, t.location), (2, 0x0030_0000, 0));
        assert_eq!((t.format_raw, t.format_name), (0xa6, "COMPRESSED_DXT1"));
        assert!(t.linear && !t.swizzled && !t.unnormalized);
        assert_eq!((t.dimension, t.cubemap, t.mipmaps), (2, false, 3));
        assert!(
            !t.border_texels,
            "bit 3 set is the border colour, no border texels"
        );
        assert_eq!((t.width, t.height, t.depth, t.pitch), (64, 32, 1, 128));
        assert_eq!(t.remap, 0xaae4);
        assert!(t.captured);
        assert_eq!(t.bytes, surface);
        assert_eq!(
            crate::output::list_line(&draws[0]),
            "draw     0 cmd       9 triangles      arrays    vertices=3 arrays=0 [t2:COMPRESSED_DXT1:64x32]",
            "the format name and the size stay apart"
        );
    }

    #[test]
    fn compressed_formats_are_never_swizzled_and_unrecorded_units_say_so() {
        let mut b = Builder::new();
        // Unit 4: DXT1 without LN (0x86). Unit 5: A8R8G8B8 without LN (0x85). Neither recorded.
        for (unit, format) in [(4u32, 0x86u32), (5, 0x85)] {
            let r = |base: u32| base + unit * m::TEXTURE_UNIT_STRIDE;
            b.method(r(m::SET_TEXTURE_OFFSET), 0x0040_0000 + unit * 0x1000)
                .method(r(m::SET_TEXTURE_FORMAT), 1 | (2 << 4) | (format << 8))
                .method(r(m::SET_TEXTURE_CONTROL0), 0x8000_0000);
        }
        b.method(m::SET_BEGIN_END, 5)
            .method(m::DRAW_ARRAYS, 2 << 24)
            .method(m::SET_BEGIN_END, 0);

        let (draws, _) = run(&b);
        let t = &draws[0].textures;
        assert_eq!(t.len(), 2);
        assert_eq!(t[0].format_name, "COMPRESSED_DXT1");
        assert!(
            !t[0].linear && !t[0].swizzled,
            "DXT with LN clear is packed rows"
        );
        assert_eq!(t[1].format_name, "A8R8G8B8");
        assert!(!t[1].linear && t[1].swizzled);
        for t in t {
            assert!(!t.captured && t.bytes.is_empty(), "unit {}", t.unit);
            assert!(t.border_texels, "bit 3 clear is a border in the image");
        }
    }

    #[test]
    fn vertex_rebinds_between_ranges_flag_the_draw() {
        for (mid_draw, reg) in [
            (true, m::SET_VERTEX_DATA_BASE_OFFSET),
            (true, m::SET_VERTEX_DATA_BASE_INDEX),
            (true, m::SET_VERTEX_DATA_ARRAY_OFFSET + 7),
            (false, m::SET_VERTEX_DATA_BASE_OFFSET),
        ] {
            let mut b = Builder::new();
            b.method(m::SET_BEGIN_END, 5);
            if mid_draw {
                b.method(m::DRAW_ARRAYS, 2 << 24);
            }
            b.method(reg, 0x40)
                .method(m::DRAW_ARRAYS, (2 << 24) | 3)
                .method(m::SET_BEGIN_END, 0);
            let (draws, _) = run(&b);
            assert_eq!(
                draws[0].vertex_bindings_changed_mid_draw, mid_draw,
                "reg {reg:#x}, after a range: {mid_draw}"
            );
        }
    }

    #[test]
    fn begin_end_low_byte_and_index_address_mask_match_rpcs3() {
        let mut b = Builder::new();
        let indices = [0u16, 1, 2];
        // Bit 30 set: RPCS3 keeps 29 bits of the index address.
        b.method(m::SET_INDEX_ARRAY_ADDRESS, 0x4000_0000 | INDEX_OFFSET)
            .method(m::SET_INDEX_ARRAY_DMA, 1 << 4)
            .method(m::SET_BEGIN_END, 0x300 | 5)
            .method(m::DRAW_INDEX_ARRAY, 2 << 24)
            .method(m::SET_BEGIN_END, 0x100);
        b.block(0, INDEX_OFFSET, &be16(&indices));

        let (draws, stats) = run(&b);
        assert_eq!(
            (stats.draws, stats.stray_ends),
            (1, 0),
            "0x100 ends the draw"
        );
        assert_eq!(draws[0].primitive, 5);
        let ia = draws[0].index_array.as_ref().unwrap();
        assert_eq!(ia.rsx_offset, INDEX_OFFSET);
        assert_eq!((ia.missing_bytes, ia.bytes.clone()), (0, be16(&indices)));
    }

    #[test]
    fn version_5_is_refused_by_number() {
        let mut b = Builder::new();
        b.version = 5;
        for file in [b.gzip(), b.stream()] {
            let err = Capture::from_file_bytes(&file).unwrap_err();
            assert!(matches!(err, CaptureError::UnsupportedVersion { found: 5 }));
            assert!(
                err.to_string().contains("RRC version 5 is not supported"),
                "{err}"
            );
        }
    }

    #[test]
    fn other_headers_and_shapes_are_refused() {
        let mut b = Builder::new();
        b.le_format = 0;
        assert!(matches!(
            Capture::parse(&b.stream()),
            Err(CaptureError::NotLittleEndian(0))
        ));

        let good = Builder::new().stream();
        let mut bad = good.clone();
        bad[0] = b'X';
        assert!(matches!(
            Capture::parse(&bad),
            Err(CaptureError::BadMagic(_))
        ));
        assert!(matches!(
            Capture::parse(&good[..good.len() - 1]),
            Err(CaptureError::BadRegisterState { .. })
        ));
        assert!(matches!(
            Capture::parse(&good[..40]),
            Err(CaptureError::Truncated { .. })
        ));
    }

    #[test]
    fn constant_uploads_track_load_and_mid_draw_changes() {
        let f = |x: f32| x.to_bits();
        let mut b = Builder::new();
        b.method(m::SET_TRANSFORM_CONSTANT_LOAD, 10).batch(
            m::SET_TRANSFORM_CONSTANT,
            &(1..=8).map(|i| f(i as f32)).collect::<Vec<_>>(),
        );
        // A batch starting mid-window: words 4..8 of load 20 are constant 21.
        b.method(m::SET_TRANSFORM_CONSTANT_LOAD, 20).batch(
            m::SET_TRANSFORM_CONSTANT + 4,
            &[f(-1.0), f(-2.0), f(-3.0), f(-4.0)],
        );
        b.method(m::SET_BEGIN_END, 5)
            .method(m::DRAW_ARRAYS, 2 << 24);
        b.method(m::SET_TRANSFORM_CONSTANT_LOAD, 30)
            .batch(m::SET_TRANSFORM_CONSTANT, &[f(9.0), f(9.0)]);
        b.method(m::DRAW_ARRAYS, (2 << 24) | 3)
            .method(m::SET_BEGIN_END, 0);

        let (draws, stats) = run(&b);
        assert!(!stats.starting_constants);
        let c = &draws[0].constants;
        assert_eq!(c.known_ranges, [(10, 11), (21, 21)]);
        assert_eq!(c.partially_known, 1, "constant 30 got two of four words");
        assert_eq!(c.values[10], [f(1.0), f(2.0), f(3.0), f(4.0)]);
        assert_eq!(c.values[11], [f(5.0), f(6.0), f(7.0), f(8.0)]);
        assert_eq!(c.values[21], [f(-1.0), f(-2.0), f(-3.0), f(-4.0)]);
        assert!(c.changed_mid_draw);
        assert_eq!(
            draws[0].ranges.len(),
            2,
            "the batch tail isn't mistaken for methods"
        );
    }

    #[test]
    fn savestate_shape_carries_starting_constants() {
        let mut b = Builder::new();
        let mut constants = vec![[0u32; 4]; CONSTANT_SLOTS];
        constants[467] = [1, 2, 3, 4];
        b.constants = Some(constants);
        b.method(m::SET_BEGIN_END, 5)
            .method(m::DRAW_ARRAYS, 2 << 24)
            .method(m::SET_BEGIN_END, 0);
        let (draws, stats) = run(&b);
        assert!(stats.starting_constants);
        assert_eq!(draws[0].constants.known_ranges, [(0, 467)]);
        assert_eq!(draws[0].constants.values[467], [1, 2, 3, 4]);
    }

    #[test]
    fn only_blobs_no_block_references_count_as_stale() {
        let mut b = Builder::new();
        b.stale_blob(&[9; 16]).stale_blob(&[7; 8]);
        b.method(m::SET_BEGIN_END, 5)
            .method(m::DRAW_ARRAYS, 2 << 24)
            .method(m::SET_BEGIN_END, 0);
        // Two blocks share one blob by content; the [7; 8] left over from "an earlier capture"
        // is referenced again, so only [9; 16] is stale.
        b.block(0, 0x100, &[1; 4])
            .block(0, 0x200, &[1; 4])
            .block(1, 0x300, &[7; 8]);
        let capture = Capture::from_file_bytes(&b.gzip()).unwrap();
        assert_eq!((capture.blocks.len(), capture.data.len()), (3, 3));
        assert_eq!(capture.unreferenced_blobs(), 1);
        let (draws, stats) = run(&b);
        assert_eq!((draws.len(), stats.unresolved_blocks), (1, 0));
    }

    #[test]
    fn dump_writes_buffers_and_json_outside_the_repo() {
        let mut b = Builder::new();
        vertex_setup(&mut b);
        b.method(m::SET_TEXTURE_OFFSET, 0x700)
            .method(
                m::SET_TEXTURE_FORMAT,
                1 | (2 << 4) | (0x88 << 8) | (1 << 16),
            )
            .method(m::SET_TEXTURE_CONTROL0, 0x8000_0000)
            .method(m::SET_BEGIN_END, 5)
            .method(m::DRAW_ARRAYS, 2 << 24)
            .method(m::SET_BEGIN_END, 0);
        let pos = positions();
        b.block(0, BASE + POS_OFFSET, &pos[..36])
            .block(0, 0x700, &[0x5a; 64]);
        b.method(m::SET_BEGIN_END, 5)
            .method(m::DRAW_ARRAYS, 2 << 24)
            .method(m::SET_BEGIN_END, 0);

        let dir = tempfile::tempdir().unwrap();
        let out = dir.path().join("dump");
        let capture = Capture::from_file_bytes(&b.gzip()).unwrap();
        let mut w = DumpWriter::create(&out).unwrap();
        let stats = replay(&capture, |d| w.write_draw(&d)).unwrap();
        w.finish(std::path::Path::new("synthetic.rrc.gz"), &stats)
            .unwrap();

        assert_eq!(
            std::fs::read(out.join("draw_00000/attr_00.bin")).unwrap(),
            &pos[..36]
        );
        let desc: serde_json::Value =
            serde_json::from_slice(&std::fs::read(out.join("draw_00000/draw.json")).unwrap())
                .unwrap();
        assert_eq!(desc["vertex_arrays"][0]["rsx_offset"], BASE + POS_OFFSET);
        assert_eq!(desc["vertex_arrays"][0]["file"], "attr_00.bin");
        assert_eq!(desc["textures"][0]["format_name"], "COMPRESSED_DXT45");
        let tex_file = desc["textures"][0]["file"].as_str().unwrap().to_string();
        assert_eq!(
            std::fs::read(out.join("draw_00000").join(&tex_file)).unwrap(),
            [0x5a; 64]
        );
        let second: serde_json::Value =
            serde_json::from_slice(&std::fs::read(out.join("draw_00001/draw.json")).unwrap())
                .unwrap();
        assert_eq!(
            second["textures"][0]["file"],
            tex_file.as_str(),
            "shared by content"
        );
        assert_eq!(
            std::fs::read(out.join("draw_00000/constants.bin"))
                .unwrap()
                .len(),
            m::TRANSFORM_CONSTANTS * 16
        );
        let summary: serde_json::Value =
            serde_json::from_slice(&std::fs::read(out.join("capture.json")).unwrap()).unwrap();
        assert_eq!(summary["draws"].as_array().unwrap().len(), 2);
        assert_eq!(summary["draws"][0]["vertex_count"], 3);
    }

    #[test]
    fn render_state_reads_targets_blend_masks_depth_and_samplers() {
        let mut b = Builder::new();
        // A8R8G8B8 into surfaces a and b (MRT1), Z24S8, pitch layout.
        b.method(m::SET_SURFACE_FORMAT, 8 | (2 << 5) | (1 << 8))
            .method(m::SET_SURFACE_COLOR_TARGET, 0x13)
            .method(m::SET_SURFACE_COLOR_AOFFSET, 0x0d4a_0000)
            .method(m::SET_CONTEXT_DMA_COLOR_A, 0xfeed_0000)
            .method(m::SET_SURFACE_PITCH_A, 4096)
            .method(m::SET_SURFACE_COLOR_BOFFSET, 0x0db0_0000)
            .method(m::SET_CONTEXT_DMA_COLOR_B, 0xfeed_0001)
            .method(m::SET_SURFACE_ZETA_OFFSET, 0x0d6c_0000)
            .method(m::SET_SURFACE_CLIP_HORIZONTAL, 512 << 16)
            .method(m::SET_SURFACE_CLIP_VERTICAL, 270 << 16)
            .method(m::SET_VIEWPORT_HORIZONTAL, 512 << 16)
            .method(m::SET_VIEWPORT_VERTICAL, (270 << 16) | 3)
            .method(m::SET_VIEWPORT_SCALE, 256.0f32.to_bits())
            .method(m::SET_VIEWPORT_OFFSET + 1, 135.0f32.to_bits())
            // Additive: src alpha + one on colour, zero + one on alpha.
            .method(m::SET_BLEND_ENABLE, 1)
            .method(m::SET_BLEND_ENABLE_MRT, 0b0010)
            .method(m::SET_BLEND_FUNC_SFACTOR, 0x302)
            .method(m::SET_BLEND_FUNC_DFACTOR, (1 << 16) | 1)
            .method(m::SET_BLEND_EQUATION, (0x8006 << 16) | 0x8006)
            // Surface a writes red, green and blue; b alpha only.
            .method(m::SET_COLOR_MASK, 0x0001_0101)
            .method(m::SET_COLOR_MASK_MRT, 0x1 << 4)
            .method(m::SET_DEPTH_TEST_ENABLE, 1)
            .method(m::SET_DEPTH_FUNC, 0x203)
            .method(m::SET_DEPTH_MASK, 0)
            .method(m::SET_ALPHA_TEST_ENABLE, 1)
            .method(m::SET_ALPHA_FUNC, 0x204)
            .method(m::SET_ALPHA_REF, 0x80)
            .method(m::SET_CULL_FACE_ENABLE, 1)
            .method(m::SET_CULL_FACE, 0x405)
            .method(m::SET_FRONT_FACE, 0x901);
        // One enabled unit: clamp to edge both ways, linear both ways, LOD 0-12.
        b.method(m::SET_TEXTURE_OFFSET, 0x0db0_0000)
            .method(
                m::SET_TEXTURE_FORMAT,
                1 | (2 << 4) | (0xa5 << 8) | (1 << 16),
            )
            .method(m::SET_TEXTURE_IMAGE_RECT, (512 << 16) | 270)
            .method(m::SET_TEXTURE_ADDRESS, 3 | (3 << 8) | (1 << 16))
            .method(m::SET_TEXTURE_FILTER, (2 << 24) | (2 << 16) | 0x80)
            .method(m::SET_TEXTURE_CONTROL0, 0x8000_0000 | (0xc00 << 7))
            .method(m::SET_TEXTURE_BORDER_COLOR, 0xff00_00ff)
            .method(m::SET_BEGIN_END, 6)
            .method(m::DRAW_ARRAYS, 3 << 24)
            .method(m::SET_BEGIN_END, 0);

        let (draws, _) = run(&b);
        let r = &draws[0].render_state;
        let s = &r.surface;
        assert_eq!(
            (s.color_format, s.depth_format, s.layout),
            ("A8R8G8B8", "Z24S8", "pitch")
        );
        let targets: Vec<_> = s
            .color_targets
            .iter()
            .map(|t| (t.surface, t.offset, t.location, t.pitch))
            .collect();
        assert_eq!(
            targets,
            [('a', 0x0d4a_0000, 0, 4096), ('b', 0x0db0_0000, 1, 0)]
        );
        assert_eq!(s.zeta_offset, 0x0d6c_0000);
        assert_eq!((s.clip.width, s.clip.height), (512, 270));
        assert_eq!(
            (
                r.viewport.rect.width,
                r.viewport.rect.height,
                r.viewport.rect.y
            ),
            (512, 270, 3)
        );
        assert_eq!((r.viewport.scale[0], r.viewport.offset[1]), (256.0, 135.0));
        let bl = &r.blend;
        assert!(bl.enabled);
        assert_eq!(bl.enabled_mrt, [true, false, false]);
        assert_eq!(
            (bl.src_rgb, bl.dst_rgb, bl.src_alpha, bl.dst_alpha),
            ("src_alpha", "one", "zero", "one")
        );
        assert_eq!((bl.equation_rgb, bl.equation_alpha), ("add", "add"));
        let w = &r.write_masks;
        assert!(w.color.r && w.color.g && w.color.b && !w.color.a);
        assert!(w.color_mrt[0].a && !w.color_mrt[0].r);
        assert!(!w.depth);
        assert!(r.depth.test);
        assert_eq!(r.depth.func, "lequal");
        assert!(r.alpha_test.enabled);
        assert_eq!(
            (r.alpha_test.func, r.alpha_test.reference_raw),
            ("greater", 0x80)
        );
        assert_eq!(
            (r.cull.enabled, r.cull.face, r.cull.front_face),
            (true, "back", "ccw")
        );
        let t = &draws[0].textures[0].sampler;
        assert_eq!(
            (t.wrap_s, t.wrap_t, t.wrap_r),
            ("clamp_to_edge", "clamp_to_edge", "wrap")
        );
        assert_eq!((t.min_filter, t.mag_filter), ("linear", "linear"));
        assert_eq!((t.lod_bias, t.min_lod, t.max_lod), (0.5, 0.0, 12.0));
        assert_eq!(t.border_color_raw, 0xff00_00ff);
    }

    #[test]
    fn programs_come_from_their_block_and_the_uploads() {
        let mut b = Builder::new();
        // Two fragment instructions, the second marked last (word 0, bit 0,
        // after the halfword swap RSX stores it with).
        let mut fp = vec![0u8; 32];
        fp[0..4].copy_from_slice(&0x0000_0102u32.to_be_bytes());
        fp[16..20].copy_from_slice(&0x0001_0102u32.to_be_bytes());
        b.method(m::SET_SHADER_PROGRAM, 0x0123_4560 | 2)
            .method(m::SET_SHADER_CONTROL, 0x0200_8440);
        // Program slot 4 on: two instructions in one batch, the second
        // marked last, then a third word batch continuing at the next slot.
        let ins = |n: u32, end: bool| [n, n + 1, n + 2, (n + 3) << 1 | u32::from(end)];
        b.method(m::SET_TRANSFORM_PROGRAM_LOAD, 4)
            .batch(
                m::SET_TRANSFORM_PROGRAM,
                &[ins(10, false), ins(20, false)].concat(),
            )
            .batch(m::SET_TRANSFORM_PROGRAM, &ins(30, true))
            .method(m::SET_TRANSFORM_PROGRAM_START, 5)
            .method(m::SET_VERTEX_ATTRIB_OUTPUT_MASK, 0x4020)
            .method(m::SET_BEGIN_END, 5)
            .method(m::DRAW_ARRAYS, 2 << 24)
            .method(m::SET_BEGIN_END, 0);
        b.block(1, 0x0123_4560, &fp);
        // The next draw points at an address nothing recorded.
        b.method(m::SET_SHADER_PROGRAM, 0x0000_1000 | 1)
            .method(m::SET_BEGIN_END, 5)
            .method(m::DRAW_ARRAYS, 2 << 24)
            .method(m::SET_BEGIN_END, 0);

        let (draws, _) = run(&b);
        let f = &draws[0].fragment_program;
        assert_eq!((f.offset, f.location), (0x0123_4560, 1));
        assert_eq!(
            (f.register_count, f.outputs_32bit, f.depth_export),
            (2, true, false)
        );
        assert!(f.captured);
        assert_eq!(f.bytes, fp);
        let v = &draws[0].vertex_program;
        assert_eq!((v.start, v.instructions, v.ended), (5, 2, true));
        assert_eq!(v.words, [ins(20, false), ins(30, true)].concat());
        assert_eq!(v.output_mask, 0x4020);
        assert!(!draws[1].fragment_program.captured);
        assert!(draws[1].fragment_program.bytes.is_empty());
    }

    #[test]
    fn a_vertex_program_with_no_last_instruction_runs_to_the_end_of_memory() {
        let mut b = Builder::new();
        // The last three slots, none marked last; an upload past the end
        // is dropped, as RPCS3 drops it.
        let words: Vec<u32> = (0..16).map(|w| w * 2).collect();
        b.method(m::SET_TRANSFORM_PROGRAM_LOAD, 541)
            .batch(m::SET_TRANSFORM_PROGRAM, &words)
            .method(m::SET_TRANSFORM_PROGRAM_START, 541)
            .method(m::SET_BEGIN_END, 5)
            .method(m::DRAW_ARRAYS, 2 << 24)
            .method(m::SET_BEGIN_END, 0);
        let (draws, _) = run(&b);
        let v = &draws[0].vertex_program;
        assert_eq!((v.instructions, v.ended), (3, false));
        assert_eq!(v.words, words[..12]);
    }

    #[test]
    fn clears_ride_on_the_next_draw_and_trailing_ones_on_the_stats() {
        let mut b = Builder::new();
        b.method(m::SET_SURFACE_COLOR_TARGET, 1)
            .method(m::SET_SURFACE_COLOR_AOFFSET, 0x0d4a_0000)
            .method(m::SET_COLOR_CLEAR_VALUE, 0x8080_8080)
            .method(m::CLEAR_SURFACE, 0xf0)
            .method(m::CLEAR_SURFACE, 0x01)
            .method(m::SET_BEGIN_END, 5)
            .method(m::DRAW_ARRAYS, 2 << 24)
            .method(m::SET_BEGIN_END, 0)
            .method(m::CLEAR_SURFACE, 0x80);
        let (draws, stats) = run(&b);
        let c = &draws[0].clears_before;
        assert_eq!(c.len(), 2);
        assert!(c[0].color.r && c[0].color.a && !c[0].depth);
        assert_eq!(
            (c[0].color_value_raw, c[0].color_targets.as_slice()),
            (0x8080_8080, &[0x0d4a_0000u32][..])
        );
        assert!(c[1].depth && !c[1].color.r);
        assert_eq!(stats.clears, 3);
        assert_eq!(stats.trailing_clears.len(), 1);
        assert!(stats.trailing_clears[0].color.a && !stats.trailing_clears[0].color.r);
    }

    #[test]
    fn the_dump_writes_each_program_once() {
        let mut b = Builder::new();
        let fp = [0x11u8; 16];
        b.method(m::SET_SHADER_PROGRAM, 0x0000_2000 | 1);
        for _ in 0..2 {
            b.method(m::SET_BEGIN_END, 5)
                .method(m::DRAW_ARRAYS, 2 << 24)
                .method(m::SET_BEGIN_END, 0);
            b.block(0, 0x2000, &fp);
        }
        let dir = tempfile::tempdir().unwrap();
        let out = dir.path().join("dump");
        let capture = Capture::from_file_bytes(&b.gzip()).unwrap();
        let mut w = DumpWriter::create(&out).unwrap();
        let stats = replay(&capture, |d| w.write_draw(&d)).unwrap();
        w.finish(std::path::Path::new("synthetic.rrc.gz"), &stats)
            .unwrap();
        let desc = |n: &str| -> serde_json::Value {
            serde_json::from_slice(&std::fs::read(out.join(n).join("draw.json")).unwrap()).unwrap()
        };
        let (first, second) = (desc("draw_00000"), desc("draw_00001"));
        let file = first["fragment_program"]["file"]
            .as_str()
            .unwrap()
            .to_string();
        assert_eq!(second["fragment_program"]["file"], file.as_str());
        assert_eq!(
            std::fs::read(out.join("draw_00000").join(&file)).unwrap(),
            fp
        );
        assert_eq!(first["render_state"]["surface"]["color_format"], "unknown");
        assert!(
            first["vertex_program"]["file"]
                .as_str()
                .unwrap()
                .starts_with("../programs/vp_")
        );
        assert_eq!(std::fs::read_dir(out.join("programs")).unwrap().count(), 2);
    }

    #[test]
    fn output_inside_this_repo_is_refused() {
        let manifest = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
        for inside in [
            manifest.join("target/never-created/dump"),
            manifest.join("../../not-yet/../dump"),
        ] {
            let err = refuse_repo_output(&inside).unwrap_err();
            assert!(err.to_string().contains("refusing"), "{inside:?}: {err}");
        }
        // `..` below a missing dir is refused outright, so it can't sneak back in from outside.
        let tmp = tempfile::tempdir().unwrap();
        let sneaky = tmp
            .path()
            .join("x")
            .join(std::iter::repeat_n("..", 32).collect::<Vec<_>>().join("/"));
        let sneaky = sneaky
            .join(manifest.strip_prefix("/").unwrap())
            .join("dump");
        assert!(refuse_repo_output(&sneaky).is_err());
    }

    #[test]
    fn other_clones_of_this_repo_are_refused_and_unrelated_repos_allowed() {
        let tmp = tempfile::tempdir().unwrap();
        for (name, url, refused) in [
            (
                "clone",
                "git@github.com:chotchki/skylander-portal-controller.git",
                true,
            ),
            (
                "research",
                "git@github.com:chotchki/guacamole-research.git",
                false,
            ),
        ] {
            let root = tmp.path().join(name);
            std::fs::create_dir_all(root.join(".git")).unwrap();
            std::fs::write(
                root.join(".git/config"),
                format!("[remote \"origin\"]\n\turl = {url}\n"),
            )
            .unwrap();
            assert_eq!(
                refuse_repo_output(&root.join("captures/out")).is_err(),
                refused,
                "{name}"
            );
        }
        assert!(refuse_repo_output(&tmp.path().join("plain/out")).is_ok());
    }

    #[test]
    fn nested_repos_and_symlinked_dotdot_inside_a_clone_are_refused() {
        let tmp = tempfile::tempdir().unwrap();
        let clone = tmp.path().join("clone");
        std::fs::create_dir_all(clone.join(".git")).unwrap();
        std::fs::write(
            clone.join(".git/config"),
            "[remote \"origin\"]\n\turl = git@github.com:chotchki/skylander-portal-controller.git\n",
        )
        .unwrap();
        // A nested repo with an unrelated remote, and a submodule-style `.git` file.
        let nested = clone.join("nested");
        std::fs::create_dir_all(nested.join(".git")).unwrap();
        std::fs::write(
            nested.join(".git/config"),
            "[remote \"origin\"]\n\turl = https://github.com/RPCS3/rpcs3.git\n",
        )
        .unwrap();
        std::fs::create_dir_all(clone.join(".git/modules/sub")).unwrap();
        std::fs::create_dir_all(clone.join("sub")).unwrap();
        std::fs::write(clone.join("sub/.git"), "gitdir: ../.git/modules/sub\n").unwrap();
        for inside in [nested.join("dump"), clone.join("sub/dump")] {
            let err = DumpWriter::create(&inside).err().expect("refused");
            assert!(err.to_string().contains("refusing"), "{inside:?}: {err}");
            assert!(!inside.exists());
        }

        #[cfg(unix)]
        {
            let outside = tmp.path().join("outside");
            std::fs::create_dir_all(&outside).unwrap();
            std::os::unix::fs::symlink(&nested, outside.join("link")).unwrap();
            // The OS resolves `link/..` to the clone, not to `outside`.
            let sneaky = outside.join("link/../escaped");
            assert!(refuse_repo_output(&sneaky).is_err());
            assert!(!clone.join("escaped").exists());
            // Writing through the link into a plain dir elsewhere lands at the physical path.
            let plain = tmp.path().join("plain");
            std::fs::create_dir_all(&plain).unwrap();
            std::os::unix::fs::symlink(&plain, outside.join("to_plain")).unwrap();
            let resolved = refuse_repo_output(&outside.join("to_plain/dump")).unwrap();
            assert_eq!(resolved, plain.canonicalize().unwrap().join("dump"));
        }
    }
}
