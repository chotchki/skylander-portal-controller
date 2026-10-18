//! Writes IGA v0x08 archives in code, laid out the way Giants' own are, for
//! tests and fuzz seeds (`test-fixtures` feature). No real archive is ever
//! committed; the game-magic guard fails on one.

use std::io::Write;

use super::{CHUNK_LEN, HEADER_LEN, LZMA_CHUNK_HEADER_LEN, MAGIC, RECORD_LEN, STORED, Table};

/// The properties byte Giants writes on every chunk: lc 3, lp 0, pb 2.
pub const GIANTS_LZMA_PROPS: u8 = 0x5D;

pub use crate::fnv1a32;

/// One chunk of `data` as raw LZMA1 with no end marker, as Giants packs it.
pub fn lzma_chunk(data: &[u8]) -> Vec<u8> {
    let mut options = lzma_rust2::LzmaOptions::with_preset(1);
    options.dict_size = CHUNK_LEN as u32;
    let mut writer = lzma_rust2::LzmaWriter::new_no_header(Vec::new(), &options, false)
        .expect("valid LZMA options");
    writer.write_all(data).expect("writing to a Vec");
    writer.finish().expect("writing to a Vec")
}

enum Body {
    Stored(Vec<u8>),
    Lzma { data: Vec<u8>, declared: u32 },
}

/// Entries land in hash order, each at a sector boundary, with the name table
/// last, as on the disc.
pub struct IgaBuilder {
    sector: u32,
    entries: Vec<(String, Body)>,
}

impl Default for IgaBuilder {
    fn default() -> Self {
        Self::new()
    }
}

impl IgaBuilder {
    /// A 0x800-byte sector, Giants' size.
    pub fn new() -> Self {
        Self {
            sector: 0x800,
            entries: Vec::new(),
        }
    }

    /// A power of two. A small one puts test-sized entries in the medium and
    /// large block tables.
    pub fn sector_size(mut self, sector: u32) -> Self {
        assert!(sector.is_power_of_two());
        self.sector = sector;
        self
    }

    pub fn stored(mut self, name: &str, data: &[u8]) -> Self {
        self.entries
            .push((name.into(), Body::Stored(data.to_vec())));
        self
    }

    /// Chunked LZMA; a chunk that wouldn't shrink is stored raw, as Giants
    /// does.
    pub fn lzma(mut self, name: &str, data: &[u8]) -> Self {
        let declared = u32::try_from(data.len()).expect("entry under 4 GiB");
        self.entries.push((
            name.into(),
            Body::Lzma {
                data: data.to_vec(),
                declared,
            },
        ));
        self
    }

    /// One compressed chunk of `data` under a record that declares
    /// `declared` bytes, for size-mismatch tests. Both fit one chunk.
    pub fn lzma_declaring(mut self, name: &str, data: &[u8], declared: u32) -> Self {
        assert!(data.len() <= CHUNK_LEN && (1..=CHUNK_LEN as u32).contains(&declared));
        self.entries.push((
            name.into(),
            Body::Lzma {
                data: data.to_vec(),
                declared,
            },
        ));
        self
    }

    pub fn build(&self) -> Vec<u8> {
        let sector = self.sector as usize;
        let align = |n: usize| n.next_multiple_of(sector);
        let mut order: Vec<&(String, Body)> = self.entries.iter().collect();
        order.sort_by_key(|(name, _)| fnv1a32(name.as_bytes()));

        // Payloads (sector-padded) and block words, before any offsets.
        let mut tables: [Vec<u32>; 3] = Default::default(); // large, medium, small
        let mut payloads = Vec::new();
        let mut records = Vec::new(); // (size, block_word)
        for (_, body) in &order {
            match body {
                Body::Stored(data) => {
                    payloads.push(data.clone());
                    records.push((data.len() as u32, STORED));
                }
                Body::Lzma { data, declared } => {
                    let table = Table::for_size(*declared, self.sector);
                    let slots = &mut tables[match table {
                        Table::Large => 0,
                        Table::Medium => 1,
                        Table::Small => 2,
                    }];
                    let first = slots.len() as u32;
                    let forced = *declared as usize != data.len();
                    let mut payload = Vec::new();
                    for chunk in data.chunks(CHUNK_LEN) {
                        let start = (payload.len() / sector) as u32;
                        let packed = lzma_chunk(chunk);
                        if forced || LZMA_CHUNK_HEADER_LEN + packed.len() < chunk.len() {
                            payload.extend_from_slice(&(packed.len() as u16).to_le_bytes());
                            payload.push(GIANTS_LZMA_PROPS);
                            payload.extend_from_slice(&(CHUNK_LEN as u32).to_le_bytes());
                            payload.extend_from_slice(&packed);
                            slots.push(start | table.flag());
                        } else {
                            payload.extend_from_slice(chunk);
                            slots.push(start);
                        }
                        payload.resize(align(payload.len()), 0);
                    }
                    slots.push((payload.len() / sector) as u32);
                    let last = *slots.last().unwrap();
                    assert!(last < table.flag(), "entry too big for its block table");
                    payloads.push(payload);
                    records.push((*declared, 0x2000_0000 | first));
                }
            }
        }

        let count = order.len();
        let toc_size =
            count * (4 + RECORD_LEN) + tables[0].len() * 4 + tables[1].len() * 2 + tables[2].len();
        let mut offset = align(HEADER_LEN + toc_size);
        let mut offsets = Vec::new();
        for payload in &payloads {
            offsets.push(offset as u32);
            offset = align(offset + payload.len());
        }
        let names_at = offset;

        let mut names = vec![0u8; count * 4];
        for (i, (name, _)) in order.iter().enumerate() {
            let rel = names.len() as u32;
            names[i * 4..i * 4 + 4].copy_from_slice(&rel.to_le_bytes());
            names.extend_from_slice(name.as_bytes());
            names.push(0);
            names.extend_from_slice(&(0x5EED_0000 | i as u32).to_le_bytes());
        }

        let hashes: Vec<u32> = order.iter().map(|(n, _)| fnv1a32(n.as_bytes())).collect();
        // Both as on every Giants archive.
        let divider = u32::MAX.checked_div(count as u32).unwrap_or(0);
        let slop = hashes
            .iter()
            .enumerate()
            .map(|(i, h)| (u64::from(*h) / u64::from(divider.max(1))).abs_diff(i as u64))
            .max()
            .unwrap_or(0) as u32;

        let mut out = Vec::with_capacity(names_at + names.len());
        out.extend_from_slice(MAGIC);
        for word in [
            super::VERSION,
            toc_size as u32,
            count as u32,
            self.sector,
            divider,
            slop,
            names_at as u32,
            names.len() as u32,
            tables[0].len() as u32,
            tables[1].len() as u32,
            tables[2].len() as u32,
            0,
        ] {
            out.extend_from_slice(&word.to_le_bytes());
        }
        for h in &hashes {
            out.extend_from_slice(&h.to_le_bytes());
        }
        for ((size, word), at) in records.iter().zip(&offsets) {
            out.extend_from_slice(&at.to_le_bytes());
            out.extend_from_slice(&size.to_le_bytes());
            out.extend_from_slice(&word.to_le_bytes());
        }
        for slot in &tables[0] {
            out.extend_from_slice(&slot.to_le_bytes());
        }
        for slot in &tables[1] {
            out.extend_from_slice(&(*slot as u16).to_le_bytes());
        }
        out.extend(tables[2].iter().map(|slot| *slot as u8));
        for (payload, at) in payloads.iter().zip(&offsets) {
            out.resize(*at as usize, 0);
            out.extend_from_slice(payload);
        }
        out.resize(names_at, 0);
        out.extend_from_slice(&names);
        out
    }
}

/// The property test's seeds: between them every block table, stored entries,
/// multi-chunk LZMA, a raw chunk inside an LZMA entry and the empty archive.
/// The 16-byte-sector ones are a few hundred bytes, so a mutation lands on a
/// header or TOC field far more often than in a 0x800-sector file.
pub fn seeds() -> Vec<Vec<u8>> {
    let text = |len: usize| -> Vec<u8> {
        (0..len)
            .map(|i| b"giants "[i % 7] ^ (i / 501) as u8)
            .collect()
    };
    let noise: Vec<u8> = (0..600u32)
        .map(|i| (i.wrapping_mul(2_654_435_761) >> 13) as u8)
        .collect();
    vec![
        IgaBuilder::new()
            .stored("readme.txt", b"stored bytes")
            .lzma("level.igz", &text(CHUNK_LEN + 300))
            .build(),
        IgaBuilder::new().build(),
        IgaBuilder::new().sector_size(16).build(),
        IgaBuilder::new()
            .sector_size(16)
            .stored("a", b"stored")
            .lzma("b", &text(120))
            .build(),
        IgaBuilder::new()
            .sector_size(16)
            .lzma("small", &text(900))
            .lzma("medium", &[text(2100), noise].concat())
            .stored("raw", &text(40))
            .build(),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::iga::Archive;

    #[test]
    fn fnv1a32_matches_the_published_vectors() {
        assert_eq!(fnv1a32(b""), 0x811C_9DC5);
        assert_eq!(fnv1a32(b"a"), 0xE40C_292C);
        assert_eq!(fnv1a32(b"foobar"), 0xBF9C_F968);
    }

    #[test]
    fn layout_matches_the_format() {
        let bytes = IgaBuilder::new().stored("ab", b"xyz").build();
        assert_eq!(&bytes[..4], b"IGA\x1a");
        let word = |at: usize| u32::from_le_bytes(bytes[at..at + 4].try_into().unwrap());
        // Version 8, a 16-byte TOC, one file, 0x800 sectors, a one-file
        // divider and slop as Giants writes them, the data at the first sector
        // boundary, the name table after it ending the file.
        assert_eq!((word(4), word(8), word(0xC), word(0x10)), (8, 16, 1, 0x800));
        assert_eq!((word(0x14), word(0x18)), (u32::MAX, 0));
        assert_eq!(word(0x38), 0x800);
        assert_eq!(&bytes[0x800..0x803], b"xyz");
        assert_eq!(word(0x1C), 0x1000);
        assert_eq!(&bytes[0x1004..0x1007], b"ab\0");
        assert_eq!(bytes.len(), 0x1000 + 4 + 3 + 4);
    }

    #[test]
    fn every_seed_parses_and_reads() {
        for seed in seeds() {
            let archive = Archive::parse(&seed).unwrap();
            for entry in archive.entries() {
                assert_eq!(archive.read(entry).unwrap().len(), entry.size() as usize);
            }
        }
    }
}
