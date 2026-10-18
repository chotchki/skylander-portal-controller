//! A small KTX2 writer: one 2D image, no supercompression, one basic DFD
//! block, levels stored smallest first as the format requires.

use super::{Format, Texture};

pub const MAGIC: [u8; 12] = [
    0xAB, 0x4B, 0x54, 0x58, 0x20, 0x32, 0x30, 0xBB, 0x0D, 0x0A, 0x1A, 0x0A,
];
const HEADER: usize = 80;
const LEVEL_ENTRY: usize = 24;
const WRITER: &str = "guac-formats";

/// One DFD sample: bit offset, bit length, channel id, lower, upper.
type Sample = (u16, u8, u8, u32, u32);

/// Color model and samples per the Khronos Data Format spec.
fn dfd_model(format: Format) -> (u8, &'static [Sample]) {
    match format {
        // BC1A, one sample: the alpha-present channel over the 64-bit block.
        Format::Bc1RgbaUnorm => (128, &[(0, 64, 1, 0, u32::MAX)]),
        // BC3: alpha in the first 64 bits, color in the second.
        Format::Bc3Unorm => (130, &[(0, 64, 15, 0, u32::MAX), (64, 64, 0, 0, u32::MAX)]),
        // RGBSDA, red only.
        Format::R8Unorm => (1, &[(0, 8, 0, 0, 255)]),
    }
}

/// The DFD section: its total length, then one basic block.
pub fn dfd(format: Format) -> Vec<u8> {
    let (model, samples) = dfd_model(format);
    let block = 24 + 16 * samples.len();
    let mut out = Vec::with_capacity(4 + block);
    out.extend_from_slice(&((4 + block) as u32).to_le_bytes());
    out.extend_from_slice(&0u32.to_le_bytes()); // Khronos vendor, basic type
    out.extend_from_slice(&2u16.to_le_bytes()); // version 1.3
    out.extend_from_slice(&(block as u16).to_le_bytes());
    // Model, BT.709 primaries, linear transfer, straight alpha.
    out.extend_from_slice(&[model, 1, 1, 0]);
    let edge = (format.block_edge() - 1) as u8;
    out.extend_from_slice(&[edge, edge, 0, 0]);
    let mut planes = [0u8; 8];
    planes[0] = format.block_bytes() as u8;
    out.extend_from_slice(&planes);
    for &(offset, bits, channel, lower, upper) in samples {
        out.extend_from_slice(&offset.to_le_bytes());
        out.extend_from_slice(&[bits - 1, channel, 0, 0, 0, 0]);
        out.extend_from_slice(&lower.to_le_bytes());
        out.extend_from_slice(&upper.to_le_bytes());
    }
    out
}

/// Key/value data, keys sorted, each entry padded to 4.
fn kvd(format: Format) -> Vec<u8> {
    let mut pairs = vec![("KTXwriter", WRITER)];
    if format == Format::R8Unorm {
        // L8 reads as luminance.
        pairs.insert(0, ("KTXswizzle", "rrr1"));
    }
    let mut out = Vec::new();
    for (key, value) in pairs {
        let len = key.len() + 1 + value.len() + 1;
        out.extend_from_slice(&(len as u32).to_le_bytes());
        out.extend_from_slice(key.as_bytes());
        out.push(0);
        out.extend_from_slice(value.as_bytes());
        out.push(0);
        out.resize(out.len().next_multiple_of(4), 0);
    }
    out
}

/// `lcm(block bytes, 4)`, the alignment of every level.
fn level_alignment(format: Format) -> usize {
    match format.block_bytes() {
        8 => 8,
        16 => 16,
        _ => 4,
    }
}

pub fn write(t: &Texture) -> Vec<u8> {
    let n = t.levels.len();
    let dfd = dfd(t.format);
    let kvd = kvd(t.format);
    let dfd_at = HEADER + LEVEL_ENTRY * n;
    let kvd_at = dfd_at + dfd.len();
    let align = level_alignment(t.format);

    // Smallest level first in the file; the index stays level 0 first.
    let mut offsets = vec![0usize; n];
    let mut end = kvd_at + kvd.len();
    for l in (0..n).rev() {
        end = end.next_multiple_of(align);
        offsets[l] = end;
        end += t.levels[l].len();
    }

    let mut out = Vec::with_capacity(end);
    out.extend_from_slice(&MAGIC);
    for word in [
        t.format.vk_format(),
        1, // typeSize: 1 for block-compressed and 8-bit formats
        t.width,
        t.height,
        0, // pixelDepth: 2D
        0, // layerCount: not an array
        1, // faceCount
        n as u32,
        0, // no supercompression
        dfd_at as u32,
        dfd.len() as u32,
        kvd_at as u32,
        kvd.len() as u32,
    ] {
        out.extend_from_slice(&word.to_le_bytes());
    }
    out.extend_from_slice(&[0; 16]); // no SGD
    for (level, &at) in t.levels.iter().zip(&offsets) {
        let len = level.len() as u64;
        for v in [at as u64, len, len] {
            out.extend_from_slice(&v.to_le_bytes());
        }
    }
    out.extend_from_slice(&dfd);
    out.extend_from_slice(&kvd);
    for l in (0..n).rev() {
        out.resize(offsets[l], 0);
        out.extend_from_slice(&t.levels[l]);
    }
    out
}
