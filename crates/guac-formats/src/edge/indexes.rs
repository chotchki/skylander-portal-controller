//! Edge's compressed triangle lists (index flavour 3), big-endian with bit
//! fields MSB first:
//!
//! - an 8-byte header: u16 explicit count, u16 bias, u16 first-use bytes,
//!   u8 explicit width (0 is legal: every raw value is 0), u8 zero;
//! - first-use bits, one per index taken: 0 = the next unseen vertex,
//!   1 = the next explicit index;
//! - a 2-bit code per triangle against the previous one (a, b, c):
//!   0 -> (a, c, new), 1 -> (c, b, new), 2 -> (b, a, new), 3 -> three new;
//! - the explicit indexes, `width` bits each, delta-coded in 8 lanes:
//!   `v[i] = raw - bias + v[i - 8]`.
//!
//! Each section ends on a byte with zero bits; under 16 zero bytes follow.
//! Explicit indexes only point back, and the first code is always 3, on all
//! 2,321 character segments, so anything else is refused.

use crate::{ErrorKind, FormatError};

use super::FORMAT;

pub const HEADER_LEN: usize = 8;
/// The 8 lanes the explicit deltas run in.
pub const LANES: usize = 8;

/// A decompressed triangle list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IndexStream {
    /// Three per triangle, each below the segment's vertex count.
    pub indexes: Vec<u16>,
    pub explicit_count: u16,
    pub bias: u16,
    pub width: u8,
    /// Triangles per code.
    pub codes: [u32; 4],
}

impl IndexStream {
    pub fn triangles(&self) -> impl Iterator<Item = [u16; 3]> + '_ {
        self.indexes.as_chunks::<3>().0.iter().copied()
    }
}

fn err(field: &'static str, at: usize, kind: ErrorKind) -> FormatError {
    FormatError::new(FORMAT, field, at, kind)
}

fn unsupported(field: &'static str, at: usize, value: u64) -> FormatError {
    err(field, at, ErrorKind::Unsupported { value })
}

/// `n` (at most 16) bits from bit `at`. Callers bound `at + n` first.
fn bits(data: &[u8], at: usize, n: u32) -> u32 {
    if n == 0 {
        return 0;
    }
    let first = at >> 3;
    let window = (0..3).fold(0u32, |w, k| {
        (w << 8) | u32::from(data.get(first + k).copied().unwrap_or(0))
    });
    (window >> (24 - (at & 7) as u32 - n)) & ((1 << n) - 1)
}

/// Whether bits `from..to` are all zero.
fn zero_bits(data: &[u8], from: usize, to: usize) -> bool {
    (from..to).all(|b| bits(data, b, 1) == 0)
}

fn be16(data: &[u8], at: usize) -> u16 {
    u16::from_be_bytes([data[at], data[at + 1]])
}

/// Hands out each index's source in stream order.
struct Taker<'a> {
    data: &'a [u8],
    first_use_bit: usize,
    first_use_end: usize,
    explicit_bit: usize,
    width: u32,
    bias: i64,
    explicit_count: usize,
    explicit_used: usize,
    lanes: [i64; LANES],
    seen: u32,
    vertexes: u32,
}

impl Taker<'_> {
    fn take(&mut self) -> Result<u16, FormatError> {
        if self.first_use_bit == self.first_use_end {
            let kind = ErrorKind::Truncated {
                needed: self.first_use_end / 8 + 1,
                available: self.first_use_end / 8,
            };
            return Err(err("first-use bits", HEADER_LEN, kind));
        }
        let explicit = bits(self.data, self.first_use_bit, 1) == 1;
        self.first_use_bit += 1;
        if !explicit {
            if self.seen == self.vertexes {
                let kind = ErrorKind::OutOfRange {
                    value: self.seen.into(),
                    limit: self.vertexes.into(),
                };
                return Err(err("first-use bits", (self.first_use_bit - 1) / 8, kind));
            }
            self.seen += 1;
            return Ok((self.seen - 1) as u16);
        }
        let i = self.explicit_used;
        if i == self.explicit_count {
            let kind = ErrorKind::OutOfRange {
                value: (i + 1) as u64,
                limit: self.explicit_count as u64,
            };
            return Err(err("explicit count", 0, kind));
        }
        let at = self.explicit_bit + i * self.width as usize;
        let raw = i64::from(bits(self.data, at, self.width));
        let lane = &mut self.lanes[i % LANES];
        *lane += raw - self.bias;
        let value = *lane;
        self.explicit_used += 1;
        // Back references only: a vertex is first used through a 0 bit.
        if !(0..i64::from(self.seen)).contains(&value) {
            let kind = ErrorKind::OutOfRange {
                value: value as u64,
                limit: self.seen.into(),
            };
            return Err(err("explicit index", at / 8, kind));
        }
        Ok(value as u16)
    }
}

/// Decompresses `num_indexes` indexes over `num_vertexes` vertices. Error
/// offsets are into `stream`.
pub fn decompress(
    stream: &[u8],
    num_indexes: u16,
    num_vertexes: u16,
) -> Result<IndexStream, FormatError> {
    if !num_indexes.is_multiple_of(3) {
        return Err(unsupported("numIndexes", 0, num_indexes.into()));
    }
    if stream.len() < HEADER_LEN {
        let kind = ErrorKind::Truncated {
            needed: HEADER_LEN,
            available: stream.len(),
        };
        return Err(err("index header", 0, kind));
    }
    let explicit_count = be16(stream, 0);
    let bias = be16(stream, 2);
    let first_use_bytes = usize::from(be16(stream, 4));
    let width = stream[6];
    if stream[7] != 0 {
        return Err(unsupported("index header", 7, stream[7].into()));
    }
    if width > 16 {
        return Err(unsupported("explicit width", 6, width.into()));
    }
    let triangles = usize::from(num_indexes / 3);
    let code_at = HEADER_LEN + first_use_bytes;
    let explicit_at = code_at + (2 * triangles).div_ceil(8);
    let end = explicit_at + (usize::from(explicit_count) * usize::from(width)).div_ceil(8);
    if end > stream.len() {
        let kind = ErrorKind::Truncated {
            needed: end,
            available: stream.len(),
        };
        return Err(err("index sections", 0, kind));
    }
    // Every explicit index is taken through a first-use bit.
    if usize::from(explicit_count) > 8 * first_use_bytes {
        let kind = ErrorKind::OutOfRange {
            value: explicit_count.into(),
            limit: (8 * first_use_bytes) as u64,
        };
        return Err(err("explicit count", 0, kind));
    }

    let mut t = Taker {
        data: stream,
        first_use_bit: 8 * HEADER_LEN,
        first_use_end: 8 * code_at,
        explicit_bit: 8 * explicit_at,
        width: width.into(),
        bias: bias.into(),
        explicit_count: explicit_count.into(),
        explicit_used: 0,
        lanes: [0; LANES],
        seen: 0,
        vertexes: num_vertexes.into(),
    };
    let mut indexes = Vec::with_capacity(usize::from(num_indexes));
    let mut codes = [0u32; 4];
    let (mut a, mut b, mut c) = (0, 0, 0);
    for tri in 0..triangles {
        let code = bits(stream, 8 * code_at + 2 * tri, 2);
        // The first triangle has no edge to share.
        if tri == 0 && code != 3 {
            return Err(unsupported("triangle code", code_at, code.into()));
        }
        codes[code as usize] += 1;
        (a, b, c) = match code {
            0 => (a, c, t.take()?),
            1 => (c, b, t.take()?),
            2 => (b, a, t.take()?),
            _ => (t.take()?, t.take()?, t.take()?),
        };
        indexes.extend([a, b, c]);
    }

    if t.seen != t.vertexes {
        return Err(unsupported("first-use bits", HEADER_LEN, t.seen.into()));
    }
    if t.explicit_used != t.explicit_count {
        return Err(unsupported("explicit count", 0, t.explicit_used as u64));
    }
    let used = t.first_use_bit - 8 * HEADER_LEN;
    if first_use_bytes != used.div_ceil(8) {
        return Err(unsupported("first-use bytes", 4, first_use_bytes as u64));
    }
    let pads = [
        (t.first_use_bit, 8 * code_at, "first-use bits"),
        (
            8 * code_at + 2 * triangles,
            8 * explicit_at,
            "triangle codes",
        ),
        (
            t.explicit_bit + usize::from(explicit_count) * usize::from(width),
            8 * end,
            "explicit indexes",
        ),
    ];
    for (from, to, field) in pads {
        if !zero_bits(stream, from, to) {
            return Err(unsupported(field, from / 8, stream[from / 8].into()));
        }
    }
    let tail = &stream[end..];
    if let Some(i) = tail.iter().position(|&x| x != 0) {
        return Err(unsupported("index tail", end + i, tail[i].into()));
    }
    if tail.len() >= 16 {
        return Err(unsupported("index tail", end, tail.len() as u64));
    }
    Ok(IndexStream {
        indexes,
        explicit_count,
        bias,
        width,
        codes,
    })
}

/// Reference encoder for tests and fuzz checks; `tris` must number vertices
/// in first-use order. `None` when a delta needs over 16 bits.
#[cfg(any(test, feature = "test-fixtures"))]
pub fn compress(tris: &[[u16; 3]]) -> Option<Vec<u8>> {
    let mut first_use = Vec::new();
    let mut explicit: Vec<i64> = Vec::new();
    let mut codes = Vec::new();
    let mut seen = 0u16;
    let mut emit = |v: u16, first_use: &mut Vec<u32>, explicit: &mut Vec<i64>| {
        if v == seen {
            first_use.push(0);
            seen += 1;
        } else {
            assert!(v < seen, "vertex {v} used before {seen}");
            first_use.push(1);
            explicit.push(v.into());
        }
    };
    let mut prev: Option<[u16; 3]> = None;
    for &tri in tris {
        let shared = prev.and_then(|[pa, pb, pc]| {
            (0..3).find_map(|r| {
                let rot = [tri[r], tri[(r + 1) % 3], tri[(r + 2) % 3]];
                [(pa, pc), (pc, pb), (pb, pa)]
                    .iter()
                    .position(|&e| (rot[0], rot[1]) == e)
                    .map(|code| (code as u32, rot))
            })
        });
        match shared {
            Some((code, rot)) => {
                codes.push(code);
                emit(rot[2], &mut first_use, &mut explicit);
                prev = Some(rot);
            }
            None => {
                codes.push(3);
                for v in tri {
                    emit(v, &mut first_use, &mut explicit);
                }
                prev = Some(tri);
            }
        }
    }
    let deltas: Vec<i64> = (0..explicit.len())
        .map(|i| explicit[i] - if i >= LANES { explicit[i - LANES] } else { 0 })
        .collect();
    let bias = -deltas.iter().copied().min().unwrap_or(0).min(0);
    let width = deltas
        .iter()
        .map(|d| 64 - (d + bias).leading_zeros())
        .max()
        .unwrap_or(0);
    let too_many = explicit.len() > usize::from(u16::MAX)
        || first_use.len().div_ceil(8) > usize::from(u16::MAX);
    if width > 16 || bias > i64::from(u16::MAX) || too_many {
        return None;
    }
    let raws: Vec<u32> = deltas.iter().map(|d| (d + bias) as u32).collect();
    let mut out = Vec::new();
    out.extend((explicit.len() as u16).to_be_bytes());
    out.extend((bias as u16).to_be_bytes());
    out.extend((first_use.len().div_ceil(8) as u16).to_be_bytes());
    out.extend([width as u8, 0]);
    out.extend(pack(&first_use, 1));
    out.extend(pack(&codes, 2));
    out.extend(pack(&raws, width));
    out.resize(out.len().next_multiple_of(16), 0);
    Some(out)
}

/// `values`, `width` bits each MSB first, zero-padded to a byte.
#[cfg(any(test, feature = "test-fixtures"))]
pub fn pack(values: &[u32], width: u32) -> Vec<u8> {
    let mut out = vec![0u8; (values.len() * width as usize).div_ceil(8)];
    for (i, &v) in values.iter().enumerate() {
        for k in 0..width {
            if (v >> (width - 1 - k)) & 1 == 1 {
                let b = i * width as usize + k as usize;
                out[b / 8] |= 0x80 >> (b % 8);
            }
        }
    }
    out
}
