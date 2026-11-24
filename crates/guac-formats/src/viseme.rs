//! A voice line's viseme stream (openspec AS.820): the `_data` of the
//! `tfbPhysicsAnimation` a `tfbSoundDataInfo._visemeStream` names, starting
//! DE AD BE EF ([`crate::igz::layouts::LIP_SYNC_MAGIC`]). Read off all
//! 16,945 English streams on the Giants disc, every one to its end (the
//! as-mouths lane's notes):
//!
//! - big-endian; a 48-byte header: the magic, the frame count, two words
//!   (16 and 1 on every stream, unread), the track count (9 on every
//!   stream), the line's length in seconds (`(frames - 1) * frame time`), a
//!   block's length (10.625 s, 255 frames), the frame time (1/24 s), the
//!   block count twice, a size word that doesn't cover the data and a 0;
//! - the data, running to the 4-byte footer B0 0F FE ED: `2 * blocks` words,
//!   the first `blocks - 1` each a later block's start counted from the
//!   words' end (block 0 starts there), the rest unread; then the blocks;
//! - a block: one kind byte per track (0x10 a spline, 0x01 a constant),
//!   padded to 4, then each track: a constant is an f32; a spline is `u16
//!   n`, `u8 degree`, `n + degree + 2` u8 knots (frames into the block),
//!   padded to 4, an f32 low and high, `n + 1` u8 control points (low plus
//!   `v / 255` of the way to high), padded to 4. A clamped B-spline, as
//!   Havok's spline-compressed float tracks are.
//!
//! Track `i` weighs mouth shape `i` of the speaker's viseme set
//! ([`crate::model::visemes`]). Nothing here reads past the slice; a block
//! that overruns the data or the next block refuses the stream.

use crate::{ErrorKind, FormatError};

/// The stream's first four bytes.
pub const MAGIC: [u8; 4] = [0xDE, 0xAD, 0xBE, 0xEF];
/// Its last four.
pub const FOOTER: [u8; 4] = [0xB0, 0x0F, 0xFE, 0xED];
/// Frames a block holds at most: 255, a u8 knot's reach.
pub const BLOCK_FRAMES: u32 = 255;
/// A spline track's kind byte.
pub const SPLINE: u8 = 0x10;
/// A constant track's.
pub const CONSTANT: u8 = 0x01;
/// Havok's spline tracks go to cubic; the disc's are quadratic or linear.
pub const MAX_DEGREE: u8 = 3;

const HEADER: usize = 48;
const FORMAT: &str = "viseme stream";

/// A stream: its frames at `frame_time`, cut into blocks of
/// [`BLOCK_FRAMES`].
#[derive(Debug, Clone, PartialEq)]
pub struct Stream {
    pub frames: u32,
    /// Seconds: 1/24 on every stream.
    pub frame_time: f32,
    /// Seconds, as stored: `(frames - 1) * frame_time`.
    pub length: f32,
    pub blocks: Vec<Block>,
}

/// One block: a track per mouth shape, every block the same count.
#[derive(Debug, Clone, PartialEq)]
pub struct Block {
    pub tracks: Vec<Track>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Track {
    Constant(f32),
    /// A clamped B-spline over frames into its block; `points` dequantized.
    Spline {
        degree: u8,
        knots: Vec<u8>,
        points: Vec<f32>,
    },
}

fn err(field: &'static str, offset: usize, kind: ErrorKind) -> FormatError {
    FormatError::new(FORMAT, field, offset, kind)
}

fn truncated(field: &'static str, offset: usize, needed: usize, available: usize) -> FormatError {
    err(field, offset, ErrorKind::Truncated { needed, available })
}

fn take<'a>(b: &'a [u8], at: usize, n: usize, field: &'static str) -> Result<&'a [u8], FormatError> {
    at.checked_add(n)
        .and_then(|end| b.get(at..end))
        .ok_or_else(|| truncated(field, at, at.saturating_add(n), b.len()))
}

fn u32_at(b: &[u8], at: usize, field: &'static str) -> Result<u32, FormatError> {
    let w = take(b, at, 4, field)?;
    Ok(u32::from_be_bytes([w[0], w[1], w[2], w[3]]))
}

fn f32_at(b: &[u8], at: usize, field: &'static str) -> Result<f32, FormatError> {
    u32_at(b, at, field).map(f32::from_bits)
}

fn pad4(at: usize) -> usize {
    at.div_ceil(4) * 4
}

/// The stream in `bytes`, the whole of a `_data` block.
pub fn parse(bytes: &[u8]) -> Result<Stream, FormatError> {
    let magic = take(bytes, 0, 4, "magic")?;
    if magic != MAGIC {
        return Err(err(
            "magic",
            0,
            ErrorKind::BadMagic {
                expected: &MAGIC,
                found: magic.to_vec(),
            },
        ));
    }
    if bytes.len() < HEADER + FOOTER.len() {
        return Err(truncated("header", 0, HEADER + 4, bytes.len()));
    }
    let footer = &bytes[bytes.len() - 4..];
    if footer != FOOTER {
        return Err(err(
            "footer",
            bytes.len() - 4,
            ErrorKind::BadMagic {
                expected: &FOOTER,
                found: footer.to_vec(),
            },
        ));
    }
    let frames = u32_at(bytes, 4, "frames")?;
    let tracks = u32_at(bytes, 16, "tracks")? as usize;
    let length = f32_at(bytes, 20, "length")?;
    let frame_time = f32_at(bytes, 28, "frame time")?;
    let blocks = u32_at(bytes, 32, "blocks")? as usize;
    let again = u32_at(bytes, 36, "blocks again")? as usize;
    if again != blocks {
        return Err(err(
            "blocks again",
            36,
            ErrorKind::CountMismatch {
                found: again as u64,
                expected: blocks as u64,
            },
        ));
    }
    // Every block is at least its kind bytes, so neither count can pass
    // the data's length (a fuzzed count allocates nothing big).
    let data = &bytes[HEADER..bytes.len() - 4];
    if blocks == 0 || blocks.saturating_mul(8) > data.len() || tracks > data.len() {
        return Err(err(
            "blocks",
            32,
            ErrorKind::OutOfRange {
                value: blocks as u64,
                limit: (data.len() / 8) as u64,
            },
        ));
    }
    if !frame_time.is_finite() || frame_time <= 0.0 {
        return Err(err(
            "frame time",
            28,
            ErrorKind::Unsupported {
                value: u64::from(frame_time.to_bits()),
            },
        ));
    }
    let words = blocks * 8;
    let mut starts = vec![words];
    for i in 0..blocks - 1 {
        let off = u32_at(data, i * 4, "block offsets")? as usize;
        let at = words.saturating_add(off);
        if at >= data.len() || at <= *starts.last().unwrap_or(&0) {
            return Err(err(
                "block offsets",
                HEADER + i * 4,
                ErrorKind::OutOfRange {
                    value: off as u64,
                    limit: data.len() as u64,
                },
            ));
        }
        starts.push(at);
    }
    let mut out = Vec::with_capacity(blocks);
    for (i, &start) in starts.iter().enumerate() {
        let end = starts.get(i + 1).copied().unwrap_or(data.len());
        out.push(block(&data[..end], start, tracks)?);
    }
    Ok(Stream {
        frames,
        frame_time,
        length,
        blocks: out,
    })
}

/// The block at `at` of `data`, which ends where the block must.
fn block(data: &[u8], at: usize, tracks: usize) -> Result<Block, FormatError> {
    let kinds = take(data, at, tracks, "kinds")?;
    let mut p = pad4(at + tracks);
    let mut out = Vec::with_capacity(tracks);
    for &kind in kinds {
        match kind {
            CONSTANT => {
                out.push(Track::Constant(f32_at(data, p, "constant")?));
                p += 4;
            }
            SPLINE => {
                let head = take(data, p, 3, "spline")?;
                let n = usize::from(u16::from_be_bytes([head[0], head[1]]));
                let degree = head[2];
                if degree == 0 || degree > MAX_DEGREE {
                    return Err(err(
                        "degree",
                        p + 2,
                        ErrorKind::Unsupported {
                            value: u64::from(degree),
                        },
                    ));
                }
                let count = n + usize::from(degree) + 2;
                let knots = take(data, p + 3, count, "knots")?.to_vec();
                if knots.windows(2).any(|w| w[0] > w[1]) {
                    return Err(err("knots", p + 3, ErrorKind::Unsupported { value: 0 }));
                }
                let q = pad4(p + 3 + count);
                let lo = f32_at(data, q, "low")?;
                let hi = f32_at(data, q + 4, "high")?;
                let raw = take(data, q + 8, n + 1, "points")?;
                let points = raw
                    .iter()
                    .map(|&v| lo + f32::from(v) / 255.0 * (hi - lo))
                    .collect();
                out.push(Track::Spline {
                    degree,
                    knots,
                    points,
                });
                p = pad4(q + 8 + n + 1);
            }
            other => {
                return Err(err(
                    "kinds",
                    at,
                    ErrorKind::Unsupported {
                        value: u64::from(other),
                    },
                ));
            }
        }
    }
    if p > data.len() {
        return Err(truncated("block", at, p, data.len()));
    }
    Ok(Block { tracks: out })
}

#[cfg(test)]
mod tests;
