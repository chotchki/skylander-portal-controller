//! IGA v0x08, the container every `.arc` and `.bld` on the Giants disc is,
//! laid out as read off all 893 of them (openspec AB.3), little-endian even on PS3:
//! - header (0x34): magic `IGA\x1a`, version 8, TOC size, file count, sector
//!   size (0x800), hash-search divider and slop, name-table offset and size,
//!   large, medium and small block counts, flags (0)
//! - TOC from 0x34: a u32 name hash per file (ascending), a 12-byte record per
//!   file in that order (`offset`, `size`, `block_word`), then the block
//!   tables (u32 large, u16 medium, u8 small)
//! - name table, last in the file: a u32 offset per file into the table, each
//!   to a NUL-terminated name and a u32 stamp
//!
//! A `block_word` of 0xFFFFFFFF is a stored entry; otherwise its top nibble is
//! the codec (2 = LZMA, Giants' only one) and the rest indexes slots in the
//! table the entry's size picks (small up to 0x7F sectors, medium up to 0x7FFF,
//! large past that). Slot `j` holds where 0x8000-byte chunk `j` starts, in
//! sectors from `offset` with the top bit set when compressed, and slot
//! `j + 1` where it ends; a compressed chunk is a u16 packed length, 5 LZMA
//! property bytes and raw LZMA1 with no end marker, an uncompressed one is raw.

#[cfg(any(test, feature = "test-fixtures"))]
pub mod builder;

use std::borrow::Cow;
use std::io::Read;

use crate::reader::Reader;
use crate::{ErrorKind, FormatError};

const FORMAT: &str = "IGA";
pub const MAGIC: &[u8; 4] = b"IGA\x1a";
/// How Swap Force and every later game's archives start: the same magic,
/// written big-endian.
pub const MAGIC_BIG_ENDIAN: &[u8; 4] = b"\x1aAGI";
pub const VERSION: u32 = 8;
pub const HEADER_LEN: usize = 0x34;
pub const RECORD_LEN: usize = 12;
/// What every chunk but an entry's last unpacks to.
pub const CHUNK_LEN: usize = 0x8000;
pub const STORED: u32 = 0xFFFF_FFFF;
pub const CODEC_LZMA: u8 = 2;
/// Packed length (u16) plus the 5 LZMA property bytes.
pub const LZMA_CHUNK_HEADER_LEN: usize = 7;
/// The most a compressed entry may declare. Giants' largest is 112 MiB (a
/// level's IGZ), so this only stops a corrupt size from sizing work.
pub const MAX_UNPACKED_LEN: u32 = 256 << 20;
/// The largest sector size accepted. Giants uses 0x800 throughout.
pub const MAX_SECTOR_SIZE: u32 = 0x10000;

const SUPPORTED_LE: &str = "only IGA v0x08, Giants' archives, is read";
const SUPPORTED_BE: &str =
    "big-endian IGA is Swap Force's and later; only little-endian v0x08, Giants', is read";

/// The header as stored; [`Archive::parse`] has checked the counts and
/// offsets against the input.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Header {
    pub version: u32,
    pub toc_size: u32,
    pub file_count: u32,
    pub sector_size: u32,
    /// `0xFFFFFFFF / file_count` on every Giants archive: a hash divided by
    /// it is the engine's first guess at the hash's index.
    pub hash_search_divider: u32,
    /// The furthest that guess lands from the real index, over this TOC.
    pub hash_search_slop: u32,
    pub name_table_offset: u32,
    pub name_table_size: u32,
    pub large_blocks: u32,
    pub medium_blocks: u32,
    pub small_blocks: u32,
    pub flags: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Compression {
    Stored,
    Lzma,
    /// A codec nibble Giants never uses; reading such an entry errors.
    Other(u8),
}

/// One file in the archive. Built only by [`Archive::parse`], so its offset,
/// size and block slots are known to fit the archive it came from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry<'a> {
    index: u32,
    name: &'a str,
    hash: u32,
    stamp: u32,
    offset: u32,
    size: u32,
    block_word: u32,
}

impl<'a> Entry<'a> {
    /// Position in the TOC (hash order).
    pub fn index(&self) -> usize {
        self.index as usize
    }

    /// The stored name: a bare file name in a `.bld`, a full build path
    /// (`c:/tfb/build/ps3/...`) in a character `.arc`.
    pub fn name(&self) -> &'a str {
        self.name
    }

    /// The TOC's hash for this entry. In a `.bld` it's the FNV-1a 32 of the
    /// stored name; what a `.arc` hashes isn't known yet.
    pub fn hash(&self) -> u32 {
        self.hash
    }

    /// The u32 after the name, never zero on Giants. Probably a timestamp;
    /// its epoch isn't confirmed.
    pub fn stamp(&self) -> u32 {
        self.stamp
    }

    pub fn offset(&self) -> u32 {
        self.offset
    }

    /// Unpacked size in bytes.
    pub fn size(&self) -> u32 {
        self.size
    }

    pub fn block_word(&self) -> u32 {
        self.block_word
    }

    pub fn compression(&self) -> Compression {
        if self.block_word == STORED {
            return Compression::Stored;
        }
        match (self.block_word >> 28) as u8 {
            CODEC_LZMA => Compression::Lzma,
            other => Compression::Other(other),
        }
    }

    /// Pieces [`Archive::chunks`] yields: one for a stored entry, one per
    /// 0x8000 unpacked bytes otherwise.
    pub fn chunk_count(&self) -> usize {
        if self.block_word == STORED {
            1
        } else {
            (self.size as usize).div_ceil(CHUNK_LEN)
        }
    }

    fn first_slot(&self) -> usize {
        (self.block_word & 0x0FFF_FFFF) as usize
    }

    fn record_at(&self, count: usize) -> usize {
        HEADER_LEN + count * 4 + self.index() * RECORD_LEN
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Table {
    Small,
    Medium,
    Large,
}

impl Table {
    /// The table an entry of `size` bytes indexes, by how many sectors it
    /// could span.
    fn for_size(size: u32, sector: u32) -> Self {
        let size = u64::from(size);
        let sector = u64::from(sector);
        if size <= 0x7F * sector {
            Self::Small
        } else if size <= 0x7FFF * sector {
            Self::Medium
        } else {
            Self::Large
        }
    }

    fn width(self) -> usize {
        match self {
            Self::Small => 1,
            Self::Medium => 2,
            Self::Large => 4,
        }
    }

    /// The compressed-chunk bit; the bits below it are the sector.
    fn flag(self) -> u32 {
        match self {
            Self::Small => 0x80,
            Self::Medium => 0x8000,
            Self::Large => 0x8000_0000,
        }
    }
}

/// A parsed archive. Names and stored data borrow from the input; only LZMA
/// output is allocated.
#[derive(Debug, Clone)]
pub struct Archive<'a> {
    bytes: &'a [u8],
    header: Header,
    entries: Vec<Entry<'a>>,
    /// Where each block table starts, absolute.
    large_at: usize,
    medium_at: usize,
    small_at: usize,
}

impl<'a> Archive<'a> {
    /// Reads the header, TOC and name table and checks every entry's data
    /// and block slots fit the input. Chunks are only checked when read.
    pub fn parse(bytes: &'a [u8]) -> Result<Self, FormatError> {
        let r = Reader::new(FORMAT, bytes);
        if bytes.starts_with(MAGIC_BIG_ENDIAN) {
            let v = r.bytes(4, 4, "version")?;
            return Err(r.err(
                "magic",
                0,
                ErrorKind::UnsupportedVersion {
                    found: u32::from_be_bytes([v[0], v[1], v[2], v[3]]).into(),
                    supported: SUPPORTED_BE,
                },
            ));
        }
        r.magic(0, MAGIC, "magic")?;
        let version = r.u32_le(4, "version")?;
        if version != VERSION {
            return Err(r.err(
                "version",
                4,
                ErrorKind::UnsupportedVersion {
                    found: version.into(),
                    supported: SUPPORTED_LE,
                },
            ));
        }
        r.bytes(0, HEADER_LEN, "header")?;
        let header = Header {
            version,
            toc_size: r.u32_le(0x08, "toc_size")?,
            file_count: r.u32_le(0x0C, "file_count")?,
            sector_size: r.u32_le(0x10, "sector_size")?,
            hash_search_divider: r.u32_le(0x14, "hash_search_divider")?,
            hash_search_slop: r.u32_le(0x18, "hash_search_slop")?,
            name_table_offset: r.u32_le(0x1C, "name_table_offset")?,
            name_table_size: r.u32_le(0x20, "name_table_size")?,
            large_blocks: r.u32_le(0x24, "large_blocks")?,
            medium_blocks: r.u32_le(0x28, "medium_blocks")?,
            small_blocks: r.u32_le(0x2C, "small_blocks")?,
            flags: r.u32_le(0x30, "flags")?,
        };
        let sector = header.sector_size;
        if !sector.is_power_of_two() || sector > MAX_SECTOR_SIZE {
            return Err(r.err(
                "sector_size",
                0x10,
                ErrorKind::Unsupported {
                    value: sector.into(),
                },
            ));
        }

        // The TOC must sit in the input before its counts size anything, and
        // its size must be exactly what the counts add up to.
        let toc_size = header.toc_size as usize;
        r.bytes(HEADER_LEN, toc_size, "toc")?;
        let count = header.file_count as usize;
        let expected = u64::from(header.file_count) * (4 + RECORD_LEN as u64)
            + u64::from(header.large_blocks) * 4
            + u64::from(header.medium_blocks) * 2
            + u64::from(header.small_blocks);
        if expected != toc_size as u64 {
            return Err(r.err(
                "toc_size",
                0x08,
                ErrorKind::OutOfRange {
                    value: expected,
                    limit: toc_size as u64,
                },
            ));
        }
        // Each term is at most toc_size now, so none of these can overflow.
        let large_at = HEADER_LEN + count * (4 + RECORD_LEN);
        let medium_at = large_at + header.large_blocks as usize * 4;
        let small_at = medium_at + header.medium_blocks as usize * 2;

        let names_at = header.name_table_offset as usize;
        let names_len = header.name_table_size as usize;
        r.bytes(names_at, names_len, "name_table")?;
        let names_end = names_at + names_len;
        if names_len / 4 < count {
            return Err(r.err(
                "name_table_size",
                0x20,
                ErrorKind::OutOfRange {
                    value: names_len as u64,
                    limit: count as u64 * 4,
                },
            ));
        }

        let mut entries = Vec::with_capacity(count);
        for i in 0..count {
            let hash = r.u32_le(HEADER_LEN + i * 4, "hash")?;
            let at = HEADER_LEN + count * 4 + i * RECORD_LEN;
            let offset = r.u32_le(at, "offset")?;
            let size = r.u32_le(at + 4, "size")?;
            let block_word = r.u32_le(at + 8, "block_word")?;

            let name_slot = names_at + i * 4;
            let name_offset = r.u32_le(name_slot, "name_offset")? as usize;
            if name_offset >= names_len {
                return Err(r.err(
                    "name_offset",
                    name_slot,
                    ErrorKind::OutOfRange {
                        value: name_offset as u64,
                        limit: names_len as u64,
                    },
                ));
            }
            let name_at = names_at + name_offset;
            let name = r.cstr(name_at, names_end, "name")?;
            let name = r.utf8(name, name_at, "name")?;
            let stamp_at = name_at + name.len() + 1;
            if stamp_at + 4 > names_end {
                return Err(r.err(
                    "stamp",
                    stamp_at,
                    ErrorKind::Truncated {
                        needed: 4,
                        available: names_end - stamp_at,
                    },
                ));
            }
            let stamp = r.u32_le(stamp_at, "stamp")?;

            let entry = Entry {
                index: i as u32,
                name,
                hash,
                stamp,
                offset,
                size,
                block_word,
            };
            if block_word == STORED {
                r.bytes(offset as usize, size as usize, "data")?;
            } else {
                if offset as usize > r.len() {
                    return Err(r.err(
                        "offset",
                        at,
                        ErrorKind::OutOfRange {
                            value: offset.into(),
                            limit: r.len() as u64,
                        },
                    ));
                }
                if size > MAX_UNPACKED_LEN {
                    return Err(r.err(
                        "size",
                        at + 4,
                        ErrorKind::OutOfRange {
                            value: size.into(),
                            limit: MAX_UNPACKED_LEN.into(),
                        },
                    ));
                }
                let table = Table::for_size(size, sector);
                let slots = match table {
                    Table::Small => header.small_blocks,
                    Table::Medium => header.medium_blocks,
                    Table::Large => header.large_blocks,
                } as usize;
                let last = entry.first_slot() + entry.chunk_count();
                if last >= slots {
                    return Err(r.err(
                        "block_word",
                        at + 8,
                        ErrorKind::OutOfRange {
                            value: last as u64,
                            limit: slots as u64,
                        },
                    ));
                }
            }
            entries.push(entry);
        }

        Ok(Self {
            bytes,
            header,
            entries,
            large_at,
            medium_at,
            small_at,
        })
    }

    pub fn header(&self) -> &Header {
        &self.header
    }

    /// Entries in TOC order.
    pub fn entries(&self) -> &[Entry<'a>] {
        &self.entries
    }

    /// The first entry with exactly this stored name.
    pub fn find(&self, name: &str) -> Option<&Entry<'a>> {
        self.entries.iter().find(|e| e.name == name)
    }

    /// The whole entry: a view into the archive when stored, unpacked
    /// otherwise. The result is always exactly [`Entry::size`] bytes.
    pub fn read(&self, entry: &Entry<'a>) -> Result<Cow<'a, [u8]>, FormatError> {
        if entry.block_word == STORED {
            return self.read_chunk(entry, 0);
        }
        // Grown chunk by chunk rather than sized from the record, so a corrupt
        // entry costs what it unpacks before failing, not what it claims.
        let mut out = Vec::new();
        for chunk in self.chunks(entry) {
            out.extend_from_slice(&chunk?);
        }
        Ok(Cow::Owned(out))
    }

    /// The entry piece by piece, each at most 0x8000 bytes unpacked (a stored
    /// entry is one piece), so a caller can stream it without holding it.
    pub fn chunks<'s>(
        &'s self,
        entry: &'s Entry<'a>,
    ) -> impl Iterator<Item = Result<Cow<'a, [u8]>, FormatError>> + 's {
        (0..entry.chunk_count()).map(move |j| self.read_chunk(entry, j))
    }

    /// Chunk `j` of the entry, `j < entry.chunk_count()`.
    pub fn read_chunk(&self, entry: &Entry<'a>, j: usize) -> Result<Cow<'a, [u8]>, FormatError> {
        let r = Reader::new(FORMAT, self.bytes);
        let record_at = entry.record_at(self.entries.len());
        if j >= entry.chunk_count() {
            return Err(r.err(
                "chunk",
                record_at,
                ErrorKind::OutOfRange {
                    value: j as u64,
                    limit: entry.chunk_count() as u64,
                },
            ));
        }
        let codec = match entry.compression() {
            Compression::Stored => {
                return r
                    .bytes(entry.offset as usize, entry.size as usize, "data")
                    .map(Cow::Borrowed);
            }
            Compression::Lzma => CODEC_LZMA,
            Compression::Other(codec) => codec,
        };

        let sector = u64::from(self.header.sector_size);
        let want = (entry.size as usize - j * CHUNK_LEN).min(CHUNK_LEN);
        let table = Table::for_size(entry.size, self.header.sector_size);
        let (start_slot, compressed, start) = self.slot(table, entry.first_slot() + j)?;
        let (end_slot, _, end) = self.slot(table, entry.first_slot() + j + 1)?;
        if end <= start {
            return Err(r.err(
                "block_slot",
                end_slot,
                ErrorKind::OutOfRange {
                    value: end.into(),
                    limit: start.into(),
                },
            ));
        }
        // Sectors count from the entry's offset; u64 so nothing wraps.
        let span_at = u64::from(entry.offset) + u64::from(start) * sector;
        let span_len = u64::from(end - start) * sector;
        if span_at + span_len > r.len() as u64 {
            return Err(r.err(
                "block_slot",
                start_slot,
                ErrorKind::OutOfRange {
                    value: span_at + span_len,
                    limit: r.len() as u64,
                },
            ));
        }
        let span_at = span_at as usize;
        let span = r.bytes(span_at, span_len as usize, "chunk")?;

        if !compressed {
            return if want <= span.len() {
                Ok(Cow::Borrowed(&span[..want]))
            } else {
                Err(r.err(
                    "chunk",
                    span_at,
                    ErrorKind::Truncated {
                        needed: want,
                        available: span.len(),
                    },
                ))
            };
        }
        if codec != CODEC_LZMA {
            return Err(r.err(
                "block_word",
                record_at + 8,
                ErrorKind::Unsupported {
                    value: codec.into(),
                },
            ));
        }
        let packed_len = r.u16_le(span_at, "chunk_size")? as usize;
        if packed_len > CHUNK_LEN {
            return Err(r.err(
                "chunk_size",
                span_at,
                ErrorKind::OutOfRange {
                    value: packed_len as u64,
                    limit: CHUNK_LEN as u64,
                },
            ));
        }
        if LZMA_CHUNK_HEADER_LEN + packed_len > span.len() {
            return Err(r.err(
                "chunk_size",
                span_at,
                ErrorKind::OutOfRange {
                    value: (LZMA_CHUNK_HEADER_LEN + packed_len) as u64,
                    limit: span.len() as u64,
                },
            ));
        }
        let props = span[2];
        let dict = u32::from_le_bytes([span[3], span[4], span[5], span[6]]);
        let packed = &span[LZMA_CHUNK_HEADER_LEN..LZMA_CHUNK_HEADER_LEN + packed_len];
        unpack_lzma(&r, span_at, props, dict, packed, want).map(Cow::Owned)
    }

    /// Slot `index` of `table`: its absolute offset, the compressed bit and
    /// the sector it names.
    fn slot(&self, table: Table, index: usize) -> Result<(usize, bool, u32), FormatError> {
        let r = Reader::new(FORMAT, self.bytes);
        let (base, slots) = match table {
            Table::Small => (self.small_at, self.header.small_blocks),
            Table::Medium => (self.medium_at, self.header.medium_blocks),
            Table::Large => (self.large_at, self.header.large_blocks),
        };
        let at = base + index * table.width();
        if index >= slots as usize {
            return Err(r.err(
                "block_slot",
                at,
                ErrorKind::OutOfRange {
                    value: index as u64,
                    limit: slots.into(),
                },
            ));
        }
        let raw = match table {
            Table::Small => r.bytes(at, 1, "block_slot")?[0].into(),
            Table::Medium => r.u16_le(at, "block_slot")?.into(),
            Table::Large => r.u32_le(at, "block_slot")?,
        };
        let flag = table.flag();
        Ok((at, raw & flag != 0, raw & (flag - 1)))
    }
}

/// One raw LZMA1 chunk, which must come out at exactly `want` bytes with
/// the range coder finished: short, long and undecodable streams all error.
fn unpack_lzma(
    r: &Reader<'_>,
    at: usize,
    props: u8,
    dict: u32,
    packed: &[u8],
    want: usize,
) -> Result<Vec<u8>, FormatError> {
    // lc + lp past 4 (LZMA2's limit) would let the file size the literal
    // coder at up to 6 MB a chunk; Giants writes lc 3, lp 0, pb 2.
    let (lc, lp, pb) = (props % 9, props / 9 % 5, props / 45);
    if props >= 9 * 5 * 5 || lc + lp > 4 {
        return Err(r.err(
            "lzma_props",
            at + 2,
            ErrorKind::Unsupported {
                value: props.into(),
            },
        ));
    }
    // No back-reference can reach past the chunk, so a dictionary larger
    // than it buys nothing; Giants declares exactly 0x8000.
    let dict = dict.clamp(4096, CHUNK_LEN as u32);
    let unpack_err = |unpacked: usize, reason: String| {
        r.err(
            "chunk",
            at,
            ErrorKind::Unpack {
                expected: want as u64,
                unpacked: unpacked as u64,
                reason,
            },
        )
    };
    let mut decoder = lzma_rust2::LzmaReader::new(
        packed,
        want as u64,
        lc.into(),
        lp.into(),
        pb.into(),
        dict,
        None,
    )
    .map_err(|e| unpack_err(0, e.to_string()))?;
    // Strict: the stream must end where the size says, not merely reach it.
    decoder.set_relaxed_end_condition(false);
    let mut out = Vec::with_capacity(want);
    if let Err(e) = decoder.read_to_end(&mut out) {
        return Err(unpack_err(out.len(), e.to_string()));
    }
    if out.len() != want {
        return Err(unpack_err(out.len(), "stream ended early".into()));
    }
    Ok(out)
}

#[cfg(test)]
mod tests;
