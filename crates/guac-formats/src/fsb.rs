//! FSB5, FMOD's sound bank, the container every sound on the Giants disc is
//! stored in (openspec AR.1): a sound effect's bank sits inside the IGZ that
//! names it, as a `tfbSoundDataInfo`'s `_data`; a voice's, music's or
//! ambience's is an entry of the file's `.arc`. This reads the bank from a
//! byte slice and knows nothing of where it came from.
//!
//! Layout, little-endian throughout, as read off the disc and cross-checked
//! with vgmstream's `fsb5.c` (a reading aid, MIT):
//! - a header: magic `FSB5`, version (0 or 1), sample count, sample-header
//!   size, name-table size, data size and the codec word, then 0x24 more
//!   bytes in version 0 (header 0x40) or 0x20 in version 1 (header 0x3C):
//!   zeros, flags and a 16-byte hash nothing here reads;
//! - one packed u64 per sample in the sample-header block: bit 0 says a
//!   chunk follows, bits 1-4 a rate code, bits 5-6 a channel code, bits
//!   7-33 the sample's data offset in 32-byte units from the data block's
//!   start, bits 34-63 its length in samples;
//! - after each packed header with bit 0 set, a chain of chunks, each a u32
//!   (bit 0 another follows, bits 1-24 the length, bits 25-31 the kind) and
//!   that many bytes: kind 1 a channel count (u8), 2 a rate (u32), 3 the
//!   loop's start and end in samples (two u32), the rest kept as read;
//! - the name table: a u32 offset per sample from the table's start, each to
//!   a NUL-terminated name (empty when the bank has no table);
//! - the data block: each sample's bytes from its offset to the next
//!   sample's, the last to the block's end.
//!
//! Giants' banks are version 1, one sample, MPEG, as far as AR.2's census
//! read them; any other codec is read and named, never decoded here.

#[cfg(any(test, feature = "test-fixtures"))]
pub mod builder;

use crate::reader::Reader;
use crate::{ErrorKind, FormatError};

const FORMAT: &str = "FSB5";
pub const MAGIC: &[u8; 4] = b"FSB5";
/// Version 0's header, with its extra word.
pub const HEADER_LEN_V0: usize = 0x40;
/// Version 1's header, Giants'.
pub const HEADER_LEN_V1: usize = 0x3C;
/// The packed u64 that opens each sample's header.
pub const SAMPLE_HEADER_LEN: usize = 8;
/// What a sample's packed data offset counts in.
pub const DATA_ALIGN: usize = 32;
/// A chunk's word before its bytes.
pub const CHUNK_HEADER_LEN: usize = 4;

/// The rate each packed rate code names; a code past the table is refused.
pub const RATES: [u32; 11] = [
    4_000, 8_000, 11_000, 11_025, 16_000, 22_050, 24_000, 32_000, 44_100, 48_000, 96_000,
];
/// The channel count each packed channel code names.
pub const CHANNELS: [u32; 4] = [1, 2, 6, 8];

/// Chunk kinds this reader applies; every other kind is kept as read.
pub mod chunk {
    pub const CHANNELS: u8 = 1;
    pub const RATE: u8 = 2;
    pub const LOOP: u8 = 3;
}

/// The codec word, by FMOD's numbering for FSB5.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Codec {
    None,
    Pcm8,
    Pcm16,
    Pcm24,
    Pcm32,
    PcmFloat,
    GcAdpcm,
    ImaAdpcm,
    /// Sony's PS-ADPCM.
    Vag,
    HeVag,
    Xma,
    /// MPEG audio frames (Giants': MPEG-1 Layer III).
    Mpeg,
    Celt,
    Atrac9,
    Xwma,
    Vorbis,
    FAdpcm,
    Opus,
    /// A word past FMOD's list, kept so a census can count it.
    Other(u32),
}

impl Codec {
    pub fn from_word(word: u32) -> Self {
        match word {
            0x00 => Self::None,
            0x01 => Self::Pcm8,
            0x02 => Self::Pcm16,
            0x03 => Self::Pcm24,
            0x04 => Self::Pcm32,
            0x05 => Self::PcmFloat,
            0x06 => Self::GcAdpcm,
            0x07 => Self::ImaAdpcm,
            0x08 => Self::Vag,
            0x09 => Self::HeVag,
            0x0A => Self::Xma,
            0x0B => Self::Mpeg,
            0x0C => Self::Celt,
            0x0D => Self::Atrac9,
            0x0E => Self::Xwma,
            0x0F => Self::Vorbis,
            0x10 => Self::FAdpcm,
            0x11 => Self::Opus,
            other => Self::Other(other),
        }
    }

    pub fn word(self) -> u32 {
        match self {
            Self::None => 0x00,
            Self::Pcm8 => 0x01,
            Self::Pcm16 => 0x02,
            Self::Pcm24 => 0x03,
            Self::Pcm32 => 0x04,
            Self::PcmFloat => 0x05,
            Self::GcAdpcm => 0x06,
            Self::ImaAdpcm => 0x07,
            Self::Vag => 0x08,
            Self::HeVag => 0x09,
            Self::Xma => 0x0A,
            Self::Mpeg => 0x0B,
            Self::Celt => 0x0C,
            Self::Atrac9 => 0x0D,
            Self::Xwma => 0x0E,
            Self::Vorbis => 0x0F,
            Self::FAdpcm => 0x10,
            Self::Opus => 0x11,
            Self::Other(w) => w,
        }
    }
}

/// A chunk of a sample's header, as read.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Chunk<'a> {
    pub kind: u8,
    /// Where its bytes start in the bank.
    pub at: usize,
    pub bytes: &'a [u8],
}

/// One sample: its facts, with any chunk's override applied, and its bytes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Sample<'a> {
    /// The name table's entry, `None` when the bank has no table.
    pub name: Option<&'a str>,
    pub rate: u32,
    pub channels: u32,
    /// Length in samples per channel, as the header gives it.
    pub samples: u32,
    /// The loop chunk's start and end, in samples.
    pub loop_points: Option<(u32, u32)>,
    /// Every chunk, the applied ones included.
    pub chunks: Vec<Chunk<'a>>,
    /// Where its bytes start in the bank.
    pub data_at: usize,
    pub data: &'a [u8],
}

/// One FSB5 file read: its version and codec, each sample with its bytes,
/// and where its data block lies.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Bank<'a> {
    pub version: u32,
    pub codec: Codec,
    pub samples: Vec<Sample<'a>>,
    /// The header's length, by version.
    pub header_len: usize,
    /// Where the data block starts, and its length.
    pub data_at: usize,
    pub data_len: usize,
}

impl<'a> Bank<'a> {
    /// Reads a bank from the start of `bytes`; anything past its data block
    /// is left alone (an `.arc` entry can run past it).
    pub fn parse(bytes: &'a [u8]) -> Result<Self, FormatError> {
        let r = Reader::new(FORMAT, bytes);
        r.magic(0, MAGIC, "magic")?;
        let version = r.u32_le(4, "version")?;
        let header_len = match version {
            0 => HEADER_LEN_V0,
            1 => HEADER_LEN_V1,
            v => {
                return Err(r.err(
                    "version",
                    4,
                    ErrorKind::UnsupportedVersion {
                        found: v.into(),
                        supported: "FSB5 versions 0 and 1",
                    },
                ));
            }
        };
        let count = r.u32_le(8, "sample_count")? as usize;
        let headers_len = r.u32_le(0x0C, "sample_headers_size")? as usize;
        let names_len = r.u32_le(0x10, "name_table_size")? as usize;
        let data_len = r.u32_le(0x14, "data_size")? as usize;
        let codec = Codec::from_word(r.u32_le(0x18, "codec")?);
        r.bytes(0, header_len, "header")?;

        // Each block must fit before any count sizes anything.
        let headers_at = header_len;
        r.bytes(headers_at, headers_len, "sample_headers")?;
        let names_at = headers_at + headers_len;
        r.bytes(names_at, names_len, "name_table")?;
        let data_at = names_at + names_len;
        let data = r.bytes(data_at, data_len, "data")?;
        if count > headers_len / SAMPLE_HEADER_LEN {
            return Err(r.err(
                "sample_count",
                8,
                ErrorKind::OutOfRange {
                    value: count as u64,
                    limit: (headers_len / SAMPLE_HEADER_LEN) as u64,
                },
            ));
        }
        if names_len > 0 && count > names_len / 4 {
            return Err(r.err(
                "name_table_size",
                0x10,
                ErrorKind::OutOfRange {
                    value: names_len as u64,
                    limit: count as u64 * 4,
                },
            ));
        }

        // The packed headers and their chunks, inside the header block.
        let headers = Reader::new(FORMAT, &bytes[..names_at]);
        let mut at = headers_at;
        let mut heads = Vec::with_capacity(count);
        for _ in 0..count {
            let lo = headers.u32_le(at, "sample_header")?;
            let hi = headers.u32_le(at + 4, "sample_header")?;
            let packed = u64::from(lo) | (u64::from(hi) << 32);
            let head_at = at;
            at += SAMPLE_HEADER_LEN;
            let rate_code = ((packed >> 1) & 0xF) as usize;
            let mut rate = *RATES.get(rate_code).ok_or_else(|| {
                r.err(
                    "rate_code",
                    head_at,
                    ErrorKind::OutOfRange {
                        value: rate_code as u64,
                        limit: RATES.len() as u64 - 1,
                    },
                )
            })?;
            let mut channels = CHANNELS[((packed >> 5) & 0x3) as usize];
            let offset = (((packed >> 7) & 0x07FF_FFFF) as usize) * DATA_ALIGN;
            let samples = (packed >> 34) as u32;
            let mut loop_points = None;
            let mut chunks = Vec::new();
            let mut more = packed & 1 != 0;
            while more {
                let word = headers.u32_le(at, "chunk")?;
                let word_at = at;
                more = word & 1 != 0;
                let len = ((word >> 1) & 0x00FF_FFFF) as usize;
                let kind = (word >> 25) as u8;
                at += CHUNK_HEADER_LEN;
                let body = headers.bytes(at, len, "chunk_body")?;
                match kind {
                    chunk::CHANNELS => {
                        let &[n, ..] = body else {
                            return Err(short(&r, "channels_chunk", word_at, 1, len));
                        };
                        channels = n.into();
                    }
                    chunk::RATE => {
                        rate = Reader::new(FORMAT, body)
                            .u32_le(0, "rate_chunk")
                            .map_err(|_| short(&r, "rate_chunk", word_at, 4, len))?;
                    }
                    chunk::LOOP => {
                        let lr = Reader::new(FORMAT, body);
                        let start = lr
                            .u32_le(0, "loop_chunk")
                            .map_err(|_| short(&r, "loop_chunk", word_at, 8, len))?;
                        let end = lr
                            .u32_le(4, "loop_chunk")
                            .map_err(|_| short(&r, "loop_chunk", word_at, 8, len))?;
                        loop_points = Some((start, end));
                    }
                    _ => {}
                }
                chunks.push(Chunk {
                    kind,
                    at,
                    bytes: body,
                });
                at += len;
            }
            heads.push(Head {
                at: head_at,
                offset,
                rate,
                channels,
                samples,
                loop_points,
                chunks,
            });
        }

        // Each sample's bytes run to the next one's offset, the last to the
        // block's end; offsets out of order or past the block are refused.
        let mut out = Vec::with_capacity(count);
        for (i, h) in heads.iter().enumerate() {
            let (head_at, offset) = (h.at, h.offset);
            let end = heads.get(i + 1).map_or(data_len, |next| next.offset);
            if offset > end || end > data_len {
                return Err(r.err(
                    "data_offset",
                    head_at,
                    ErrorKind::OutOfRange {
                        value: offset as u64,
                        limit: end.min(data_len) as u64,
                    },
                ));
            }
            let name = if names_len == 0 {
                None
            } else {
                let entry_at = names_at + i * 4;
                let rel = r.u32_le(entry_at, "name_offset")? as usize;
                if rel >= names_len {
                    return Err(r.err(
                        "name_offset",
                        entry_at,
                        ErrorKind::OutOfRange {
                            value: rel as u64,
                            limit: names_len as u64,
                        },
                    ));
                }
                let start = names_at + rel;
                let raw = r.cstr(start, names_at + names_len, "name")?;
                Some(r.utf8(raw, start, "name")?)
            };
            out.push(Sample {
                name,
                rate: h.rate,
                channels: h.channels,
                samples: h.samples,
                loop_points: h.loop_points,
                chunks: h.chunks.clone(),
                data_at: data_at + offset,
                data: &data[offset..end],
            });
        }
        Ok(Self {
            version,
            codec,
            samples: out,
            header_len,
            data_at,
            data_len,
        })
    }

    /// The bank's whole length: header, sample headers, names and data.
    pub fn len(&self) -> usize {
        self.data_at + self.data_len
    }

    /// Never true for a bank that parsed: the header alone is 0x3C bytes.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

/// A sample's packed header and chunks, before its bytes are sliced.
struct Head<'a> {
    at: usize,
    offset: usize,
    rate: u32,
    channels: u32,
    samples: u32,
    loop_points: Option<(u32, u32)>,
    chunks: Vec<Chunk<'a>>,
}

fn short(r: &Reader<'_>, field: &'static str, at: usize, needed: usize, len: usize) -> FormatError {
    r.err(
        field,
        at,
        ErrorKind::Truncated {
            needed,
            available: len,
        },
    )
}

#[cfg(test)]
mod tests;
