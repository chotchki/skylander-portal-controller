//! RSX's reading of a captured texture unit (openspec AC.21): the levels RPCS3 lays out over the
//! unit's bytes and the order it reads their texels in, so a capture can be checked against an
//! untile written independently of it. Each level comes back as packed rows of blocks, top down,
//! the shape a KTX2 level has.
//!
//! What it follows, at the pin (`vendor/rpcs3`):
//! - The layout: `get_subresources_layout_impl` (`rpcs3/Emu/RSX/Common/TextureUtils.cpp:626`, fed
//!   by the unit at `:823`). `LN` set reads every level as rows at the unit's pitch; `LN` clear
//!   packs each level's blocks with no row padding, the levels back to back.
//! - The texel order: with `LN` clear a B8 level is Morton-ordered, and RPCS3 walks it with the
//!   carry-mask increment of `convert_linear_swizzle<u8, true>`
//!   (`rpcs3/Emu/RSX/Utils/image_utils.hpp:73`), which [`deswizzle`] does in our own code. The VK
//!   backend runs that walk on the CPU when an image's first level uploads under 1 KiB at the host
//!   row pitch and in a compute shader otherwise (`rpcs3/Emu/RSX/VK/VKTexture.cpp:1148-1158`);
//!   every L8 image on the Giants disc is 128x2 (256 bytes), so the CPU walk reads them.
//! - DXT1/23/45 are never Morton-ordered ([`crate::nv4097::never_swizzled`]): `LN` clear is packed
//!   rows, `LN` set rows at the pitch.
//!
//! Only what the decode gate meets: 2D, no border texels, B8 and the three DXT formats.
//! Everything else is refused by name, not guessed.

use thiserror::Error;

use crate::nv4097 as m;
use crate::replay::TextureUnit;

/// The check a unit failed, so [`levels`] read none of it.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum Unread {
    #[error("format {0:#x} isn't read (B8 and DXT1/23/45 only)")]
    Format(u32),
    #[error("not a plain 2D image: {0}")]
    Shape(&'static str),
    #[error("pitch {pitch} is narrower than a row of blocks ({row} bytes)")]
    Pitch { pitch: u32, row: u32 },
    #[error("the unit's bytes end at {available}, its levels need {needed}")]
    Short { needed: usize, available: usize },
}

/// Texels along a block's edge and bytes per block, by base format (the format byte without
/// `LN`/`UN`).
pub fn block(base: u32) -> Option<(u32, u32)> {
    match base {
        0x81 => Some((1, 1)),
        0x86 => Some((4, 8)),
        0x87 | 0x88 => Some((4, 16)),
        _ => None,
    }
}

/// The unit's levels as RSX reads them: level 0 first, each in packed rows of blocks.
pub fn levels(t: &TextureUnit) -> Result<Vec<Vec<u8>>, Unread> {
    let base = t.format_raw & !(m::TEXTURE_LN | m::TEXTURE_UN);
    let (edge, bytes) = block(base).ok_or(Unread::Format(base))?;
    if t.dimension != 2 || t.cubemap || t.depth != 1 {
        return Err(Unread::Shape("not 2D"));
    }
    if t.border_texels {
        return Err(Unread::Shape("border texels"));
    }
    if t.width == 0 || t.height == 0 || t.mipmaps == 0 {
        return Err(Unread::Shape("empty"));
    }
    let mut out = Vec::with_capacity(t.mipmaps as usize);
    let mut at = 0usize;
    let (mut w, mut h) = (t.width, t.height);
    for _ in 0..t.mipmaps {
        let (bw, bh) = (w.div_ceil(edge), h.div_ceil(edge));
        let row = bw * bytes;
        // LN set: rows at the unit's pitch (the same pitch for every level, `:676`).
        let stride = if t.linear {
            if t.pitch < row {
                return Err(Unread::Pitch {
                    pitch: t.pitch,
                    row,
                });
            }
            t.pitch
        } else {
            row
        };
        let slice = (stride * bh) as usize;
        let src = t.bytes.get(at..at + slice).ok_or(Unread::Short {
            needed: at + slice,
            available: t.bytes.len(),
        })?;
        let rows: Vec<u8> = src
            .chunks(stride as usize)
            .flat_map(|r| &r[..row as usize])
            .copied()
            .collect();
        out.push(if t.swizzled {
            deswizzle(&rows, w, h)
        } else {
            rows
        });
        at += slice;
        w = (w / 2).max(1);
        h = (h / 2).max(1);
    }
    Ok(out)
}

/// One byte per texel, Morton order to rows, walked as RPCS3 walks it: x and y are kept
/// pre-interleaved and stepped with `(offset - mask) & mask`, which adds one in the masked bits
/// and carries across the gaps. Up to the shorter side's bits x and y alternate (x in the even
/// bits); past it the longer side's bits sit above, x's by giving its mask every high bit, y's by
/// adding one whole square of the shorter side each time y's interleaved bits wrap.
pub fn deswizzle(src: &[u8], width: u32, height: u32) -> Vec<u8> {
    let (lw, lh) = (ceil_log2(width), ceil_log2(height));
    let square = 1u32 << (2 * lw.min(lh));
    let x_mask = 0x5555_5555 | !(square - 1);
    let y_mask = 0xAAAA_AAAA & (square - 1);
    let mut out = Vec::with_capacity((width * height) as usize);
    let (mut y_off, mut column) = (0u32, 0u32);
    for _ in 0..height {
        let mut x_off = column;
        for _ in 0..width {
            out.push(src[(y_off + x_off) as usize]);
            x_off = x_off.wrapping_sub(x_mask) & x_mask;
        }
        y_off = y_off.wrapping_sub(y_mask) & y_mask;
        if y_off == 0 {
            column += square;
        }
    }
    out
}

fn ceil_log2(v: u32) -> u32 {
    v.next_power_of_two().trailing_zeros()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn unit(format_raw: u32, width: u32, height: u32, mipmaps: u32, pitch: u32) -> TextureUnit {
        let base = format_raw & !(m::TEXTURE_LN | m::TEXTURE_UN);
        let linear = format_raw & m::TEXTURE_LN != 0;
        TextureUnit {
            unit: 0,
            offset: 0,
            location: 0,
            format_raw,
            format_name: m::texture_format_name(base),
            linear,
            swizzled: !linear && !m::never_swizzled(base),
            unnormalized: false,
            dimension: 2,
            cubemap: false,
            border_texels: false,
            mipmaps,
            width,
            height,
            depth: 1,
            pitch,
            remap: 0xaae4,
            captured: true,
            bytes: Vec::new(),
        }
    }

    fn count(n: usize) -> Vec<u8> {
        (0..n).map(|i| i as u8).collect()
    }

    #[test]
    fn deswizzle_matches_hand_morton_tables() {
        // Square: x in the even bits, y in the odd.
        assert_eq!(
            deswizzle(&count(16), 4, 4),
            [0, 1, 4, 5, 2, 3, 6, 7, 8, 9, 12, 13, 10, 11, 14, 15]
        );
        // Wide: one shared bit, then x's high bits above it.
        assert_eq!(
            deswizzle(&count(16), 8, 2),
            [0, 1, 4, 5, 8, 9, 12, 13, 2, 3, 6, 7, 10, 11, 14, 15]
        );
        // Tall: y's high bits above the shared one, which on a 2-wide image is plain rows.
        assert_eq!(deswizzle(&count(16), 2, 8), count(16));
        // 4x8: a 4x4 square, then the next one below it.
        let tall = deswizzle(&count(32), 4, 8);
        assert_eq!(&tall[..16], &deswizzle(&count(16), 4, 4)[..]);
        assert_eq!(tall[16..20], [16, 17, 20, 21]);
        // One row or one column is never reordered.
        assert_eq!(deswizzle(&count(8), 8, 1), count(8));
        assert_eq!(deswizzle(&count(8), 1, 8), count(8));
    }

    #[test]
    fn deswizzle_reads_giants_128x2_shape_in_pairs() {
        // Stored as pairs per row (a b a' b' ...), the shape the focus mappers come in.
        let rows = deswizzle(&count(256), 128, 2);
        assert_eq!(rows[..6], [0, 1, 4, 5, 8, 9]);
        assert_eq!(rows[128..134], [2, 3, 6, 7, 10, 11]);
        assert_eq!(rows[127], 253);
        assert_eq!(rows[255], 255);
    }

    #[test]
    fn swizzled_b8_levels_sit_back_to_back_each_deswizzled() {
        // 8x4 with three levels: 32 + 8 + 2 bytes, then the chain's 128-byte padding.
        let mut t = unit(0x81, 8, 4, 3, 0);
        t.bytes = count(128);
        let l = levels(&t).unwrap();
        assert_eq!(l.len(), 3);
        assert_eq!(l[0], deswizzle(&count(32), 8, 4));
        assert_eq!(l[1], deswizzle(&(32..40).collect::<Vec<u8>>(), 4, 2));
        assert_eq!(l[2], [40, 41]);
    }

    #[test]
    fn linear_rows_drop_the_pitch_padding() {
        // DXT5 8x8 with LN: 2 blocks of 16 bytes a row, at a 64-byte pitch.
        let mut t = unit(0x88 | m::TEXTURE_LN, 8, 8, 1, 64);
        t.bytes = count(128);
        let l = levels(&t).unwrap();
        assert_eq!(l.len(), 1);
        assert_eq!(&l[0][..32], &count(32)[..]);
        assert_eq!(&l[0][32..], &(64..96).collect::<Vec<u8>>()[..]);
        // B8 with LN is rows too, never Morton.
        let mut t = unit(0x81 | m::TEXTURE_LN, 4, 2, 1, 8);
        t.bytes = count(16);
        assert_eq!(levels(&t).unwrap()[0], [0, 1, 2, 3, 8, 9, 10, 11]);
    }

    #[test]
    fn dxt_without_ln_is_packed_rows_mips_included() {
        // DXT1 8x8, 2 levels: 4 blocks then 1, 8 bytes each, pitch ignored.
        let mut t = unit(0x86, 8, 8, 2, 4096);
        t.bytes = count(128);
        let l = levels(&t).unwrap();
        assert_eq!(l, [count(32), (32..40).collect::<Vec<u8>>()]);
    }

    #[test]
    fn what_it_doesnt_read_is_refused_by_name() {
        let mut t = unit(0x85, 4, 4, 1, 0);
        t.bytes = count(64);
        assert_eq!(levels(&t), Err(Unread::Format(0x85)));
        let mut b = unit(0x81, 4, 4, 1, 0);
        b.bytes = count(16);
        b.border_texels = true;
        assert_eq!(levels(&b), Err(Unread::Shape("border texels")));
        b.border_texels = false;
        b.cubemap = true;
        assert_eq!(levels(&b), Err(Unread::Shape("not 2D")));
        let mut short = unit(0x81, 4, 4, 2, 0);
        short.bytes = count(16);
        assert_eq!(
            levels(&short),
            Err(Unread::Short {
                needed: 20,
                available: 16
            })
        );
        let mut narrow = unit(0x88 | m::TEXTURE_LN, 8, 4, 1, 16);
        narrow.bytes = count(64);
        assert_eq!(levels(&narrow), Err(Unread::Pitch { pitch: 16, row: 32 }));
    }
}
