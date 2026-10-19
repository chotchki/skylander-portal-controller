//! One level to straight RGBA8 rows (openspec AE.3), what a PNG portrait
//! holds. The usual reference rules: 565 endpoints widened by bit
//! replication, in-between points rounded to nearest. BC1 with `c0 <= c1` is
//! three colours plus transparent black (RSX honours DXT1's one-bit alpha);
//! BC3's colour half always picks from four, its alpha from eight steps, or
//! six plus 0 and 255 when `a0 <= a1`. A GPU may land one step off on an
//! in-between point; endpoints are exact.

use super::Format;

/// `data`, one `width` x `height` level as [`super::Texture`] holds it (rows
/// of blocks, or rows of L8 texels), as RGBA8 rows top down. L8 reads as grey
/// with alpha 255, KTX2's `rrr1`. `None` when `data` is shorter than the
/// level; bytes past it are ignored.
pub fn decode(format: Format, width: u32, height: u32, data: &[u8]) -> Option<Vec<u8>> {
    let need = u128::from(width.div_ceil(format.block_edge()))
        * u128::from(height.div_ceil(format.block_edge()))
        * u128::from(format.block_bytes());
    if need > data.len() as u128 {
        return None;
    }
    // Under 8 output bytes per input byte, which `data` holds.
    let (w, h) = (width as usize, height as usize);
    let mut out = vec![0u8; w * h * 4];
    if format == Format::R8Unorm {
        for (texel, &v) in out.as_chunks_mut::<4>().0.iter_mut().zip(data) {
            *texel = [v, v, v, 255];
        }
        return Some(out);
    }
    let size = format.block_bytes() as usize;
    let across = width.div_ceil(format.block_edge()) as usize;
    for (n, block) in data
        .chunks_exact(size)
        .take(need as usize / size)
        .enumerate()
    {
        let texels = match format {
            Format::Bc1RgbaUnorm => colours(block.try_into().expect("8 bytes"), true),
            _ => {
                let mut t = colours(block[8..].try_into().expect("8 bytes"), false);
                for (texel, a) in t.iter_mut().zip(alphas(block[..8].try_into().expect("8"))) {
                    texel[3] = a;
                }
                t
            }
        };
        let (bx, by) = ((n % across) * 4, (n / across) * 4);
        for (i, texel) in texels.iter().enumerate() {
            let (x, y) = (bx + i % 4, by + i / 4);
            if x < w && y < h {
                out[(y * w + x) * 4..][..4].copy_from_slice(texel);
            }
        }
    }
    Some(out)
}

/// `(wa * a + wb * b) / (wa + wb)`, rounded to nearest.
fn mix(a: u8, b: u8, wa: u32, wb: u32) -> u8 {
    let sum = wa + wb;
    ((wa * u32::from(a) + wb * u32::from(b) + sum / 2) / sum) as u8
}

/// RGB565 widened to 8 bits a channel by bit replication.
fn widen(c: u16) -> [u8; 3] {
    let (r, g, b) = ((c >> 11) as u8, ((c >> 5) & 63) as u8, (c & 31) as u8);
    [
        (r << 3) | (r >> 2),
        (g << 2) | (g >> 4),
        (b << 3) | (b >> 2),
    ]
}

/// A colour half's 16 texels; `bc1` allows its three-colour mode.
fn colours(half: [u8; 8], bc1: bool) -> [[u8; 4]; 16] {
    let c0 = u16::from_le_bytes([half[0], half[1]]);
    let c1 = u16::from_le_bytes([half[2], half[3]]);
    let (p0, p1) = (widen(c0), widen(c1));
    let point = |w0, w1| {
        let [r, g, b] = std::array::from_fn(|i| mix(p0[i], p1[i], w0, w1));
        [r, g, b, 255]
    };
    let palette = if bc1 && c0 <= c1 {
        [point(1, 0), point(0, 1), point(1, 1), [0; 4]]
    } else {
        [point(1, 0), point(0, 1), point(2, 1), point(1, 2)]
    };
    let bits = u32::from_le_bytes([half[4], half[5], half[6], half[7]]);
    std::array::from_fn(|i| palette[((bits >> (2 * i)) & 3) as usize])
}

/// A BC3 alpha half: two endpoints, then 3-bit indexes into eight steps.
fn alphas(half: [u8; 8]) -> [u8; 16] {
    let (a0, a1) = (half[0], half[1]);
    let steps: [u8; 8] = if a0 > a1 {
        std::array::from_fn(|i| match i {
            0 => a0,
            1 => a1,
            _ => mix(a0, a1, 8 - i as u32, i as u32 - 1),
        })
    } else {
        std::array::from_fn(|i| match i {
            0 => a0,
            1 => a1,
            6 => 0,
            7 => 255,
            _ => mix(a0, a1, 6 - i as u32, i as u32 - 1),
        })
    };
    let mut bits = [0u8; 8];
    bits[..6].copy_from_slice(&half[2..]);
    let bits = u64::from_le_bytes(bits);
    std::array::from_fn(|i| steps[((bits >> (3 * i)) & 7) as usize])
}
