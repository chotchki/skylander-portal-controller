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
        b.method(r(m::SET_TEXTURE_OFFSET), 0x0030_0000)
            .method(
                r(m::SET_TEXTURE_FORMAT),
                1 | (2 << 4) | (0xa6 << 8) | (3 << 16),
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
        assert_eq!((t.width, t.height, t.depth, t.pitch), (64, 32, 1, 128));
        assert_eq!(t.remap, 0xaae4);
        assert!(t.captured);
        assert_eq!(t.bytes, surface);
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
