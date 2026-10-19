//! Guest memory searched through PINE at a pause (openspec AF.8): which pages
//! are mapped, then every aligned big-endian word equal to one of a set. It's
//! how a census finds a class's live objects (words equal to its vtable) and
//! their owners (words equal to an object's address) without a savestate.
//!
//! PINE has no "is this mapped" call, so a page counts as mapped when a one-byte
//! read of its first byte comes back; a failed batch answers nothing, which is
//! why every probe is a batch of its own. RPCS3 checks each read against its
//! 4 KiB page table, so a range whose 64 KiB probes pass can still hold an
//! unmapped 4 KiB page: a chunk that fails is read again 4 KiB at a time and
//! what still fails is reported, not guessed at.

use std::collections::{BTreeMap, BTreeSet};
use std::ops::Range;

use anyhow::{Result, bail};

use crate::pine::{EmuStatus, Op, PineClient, PineError, Width};

/// What a read of the whole chunk asks for; 8,192 64-bit reads, one batch.
const CHUNK: u64 = 64 << 10;
/// RPCS3's page-table granularity.
const SMALL_PAGE: u64 = 4 << 10;

/// A stretch of guest addresses probed at `page` granularity.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Area {
    pub start: u32,
    pub end: u64,
    pub page: u32,
}

/// Where a PS3 game's objects and the pointers to them live on RPCS3: the
/// main, user and RSX-context areas (`0x0001_0000`-`0x5000_0000`, 64 KiB
/// pages: the ELF's segments and every heap) and the stacks (`0xD000_0000`,
/// 4 KiB pages). RSX local memory (`0xC000_0000`) and the SPU areas above
/// `0xE000_0000` hold no PPU objects and aren't searched.
pub const PPU_AREAS: [Area; 2] = [
    Area {
        start: 0x0001_0000,
        end: 0x5000_0000,
        page: 0x1_0000,
    },
    Area {
        start: 0xD000_0000,
        end: 0xD100_0000,
        page: 0x1000,
    },
];

/// What [`find_words`] turned up, and how much it read to get there.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Found {
    /// Each wanted word and the 4-aligned addresses holding it.
    pub hits: BTreeMap<u32, Vec<u32>>,
    /// Mapped ranges searched, coalesced.
    pub mapped: Vec<Range<u64>>,
    /// 4 KiB pages inside a mapped range that still failed to read.
    pub unreadable: Vec<u32>,
    /// Guest bytes read and searched, across every area.
    pub bytes: u64,
    /// PINE messages sent, probes included.
    pub messages: u64,
}

/// Probes `area` page by page and returns the mapped stretches, coalesced.
pub fn mapped(pine: &mut PineClient, area: Area, messages: &mut u64) -> Result<Vec<Range<u64>>> {
    let mut out: Vec<Range<u64>> = Vec::new();
    let mut at = u64::from(area.start);
    let page = u64::from(area.page);
    while at < area.end {
        *messages += 1;
        let probe = [Op::Read {
            width: Width::W8,
            addr: at as u32,
        }];
        match pine.batch(&probe) {
            Ok(_) => match out.last_mut() {
                Some(last) if last.end == at => last.end = at + page,
                _ => out.push(at..at + page),
            },
            Err(PineError::Failed) => {}
            Err(e) => return Err(e.into()),
        }
        at += page;
    }
    Ok(out)
}

/// Every 4-aligned big-endian word in `areas` equal to one of `words`, read at
/// a pause: PINE has to report the emulator paused before the first probe and
/// after the last read, or the search fails.
pub fn find_words(pine: &mut PineClient, areas: &[Area], words: &BTreeSet<u32>) -> Result<Found> {
    require_paused(pine, "before the search")?;
    let mut found = Found::default();
    for &area in areas {
        let mapped = mapped(pine, area, &mut found.messages)?;
        for range in &mapped {
            let mut at = range.start;
            while at < range.end {
                let len = CHUNK.min(range.end - at);
                found.messages += 1;
                match pine.read_bytes(at as u32, len as usize) {
                    Ok(bytes) => search(&bytes, at as u32, words, &mut found),
                    Err(PineError::Failed) => {
                        let mut page = at;
                        while page < at + len {
                            found.messages += 1;
                            match pine.read_bytes(page as u32, SMALL_PAGE as usize) {
                                Ok(bytes) => search(&bytes, page as u32, words, &mut found),
                                Err(PineError::Failed) => found.unreadable.push(page as u32),
                                Err(e) => return Err(e.into()),
                            }
                            page += SMALL_PAGE;
                        }
                    }
                    Err(e) => return Err(e.into()),
                }
                at += len;
            }
        }
        found.mapped.extend(mapped);
    }
    require_paused(pine, "after the search")?;
    Ok(found)
}

fn require_paused(pine: &mut PineClient, when: &str) -> Result<()> {
    match pine.status()? {
        EmuStatus::Paused => Ok(()),
        other => bail!("PINE reports the emulator {other:?} {when}; a search runs at a pause"),
    }
}

/// The words in `bytes` (guest memory from `base`, 4-aligned) found in `words`.
pub fn search(bytes: &[u8], base: u32, words: &BTreeSet<u32>, found: &mut Found) {
    for (i, chunk) in bytes.as_chunks::<4>().0.iter().enumerate() {
        let word = u32::from_be_bytes(*chunk);
        if words.contains(&word) {
            found
                .hits
                .entry(word)
                .or_default()
                .push(base + 4 * i as u32);
        }
    }
    found.bytes += bytes.len() as u64;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn words_are_found_big_endian_and_aligned() {
        let mut bytes = vec![0u8; 32];
        bytes[4..8].copy_from_slice(&0x0064_6638u32.to_be_bytes());
        // Straddling two words: not a hit.
        bytes[10..14].copy_from_slice(&0x0064_6638u32.to_be_bytes());
        bytes[28..32].copy_from_slice(&0x3028_E7F0u32.to_be_bytes());
        // Little-endian: not a hit either.
        bytes[16..20].copy_from_slice(&0x0064_6638u32.to_le_bytes());
        let words = BTreeSet::from([0x0064_6638, 0x3028_E7F0, 7]);
        let mut found = Found::default();
        search(&bytes, 0x1000, &words, &mut found);
        assert_eq!(
            found.hits,
            BTreeMap::from([(0x0064_6638, vec![0x1004]), (0x3028_E7F0, vec![0x101C])])
        );
        assert_eq!(found.bytes, 32);
    }
}
