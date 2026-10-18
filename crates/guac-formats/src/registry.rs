//! Every parser in this crate, as the property test and the fuzz targets see it.
//!
//! Registering a parser is one entry in [`PARSERS`]: how to run it, the valid
//! files its builder makes (the property test mutates those) and the most heap
//! it may hold at once for an input of a given length.

use crate::FormatError;
use crate::{iga, igz, sfo};

pub struct RegisteredParser {
    /// Stable name: the property test reports it, the fuzz targets match on it.
    pub name: &'static str,
    /// Runs the parser and throws the value away.
    pub parse: fn(&[u8]) -> Result<(), FormatError>,
    /// Well-formed inputs built in code. Must not be empty: mutating a valid
    /// file is what reaches past the header checks.
    pub seeds: fn() -> Vec<Vec<u8>>,
    /// Peak live heap bytes allowed while parsing an input of this length. A
    /// parser that allocates from a length field before checking it against
    /// the input blows through this.
    pub heap_budget: fn(usize) -> usize,
}

pub const PARSERS: &[RegisteredParser] = &[
    RegisteredParser {
        name: "param_sfo",
        parse: |bytes| sfo::ParamSfo::parse(bytes).map(drop),
        seeds: sfo::builder::seeds,
        // Entries are at most 56 bytes each, one per 16 input bytes, plus the
        // duplicate-key set and the keys and values they copy, which the parser
        // caps at the input length: well under 16x.
        heap_budget: |len| 16 * len + 4096,
    },
    RegisteredParser {
        name: "iga",
        parse: walk_iga,
        seeds: iga::builder::seeds,
        // Entries are 40 bytes for at least 20 input bytes each (a hash, a
        // record, a name offset), and one chunk is unpacked at a time: its
        // output, dictionary and literal coder stay under 128 KiB.
        heap_budget: |len| 4 * len + (256 << 10),
    },
    RegisteredParser {
        name: "igz",
        parse: walk_igz,
        seeds: igz::builder::seeds,
        // Location lists hold up to two values a byte, kept as 8-byte markers
        // or 12-byte objects, plus each list's decode before it's checked;
        // string tables are 16 bytes per entry of at least 1 input byte.
        heap_budget: |len| 64 * len + (64 << 10),
    },
];

/// How many unpacked bytes [`walk_iga`] reads before it stops. Chunks can
/// alias, so an input can declare far more output than it holds.
pub const IGA_WALK_BUDGET: usize = 256 << 10;

/// Parses an archive and then reads every entry chunk by chunk, dropping each
/// as it goes, until [`IGA_WALK_BUDGET`] bytes have come out. An error in one
/// entry moves on to the next. The property test and the fuzz targets run it.
pub fn walk_iga(bytes: &[u8]) -> Result<(), FormatError> {
    let archive = iga::Archive::parse(bytes)?;
    let mut left = IGA_WALK_BUDGET;
    for entry in archive.entries() {
        for chunk in archive.chunks(entry) {
            let Ok(chunk) = chunk else { break };
            left = left.saturating_sub(chunk.len().max(1));
            if left == 0 {
                return Ok(());
            }
        }
    }
    Ok(())
}

/// Parses an IGZ, then resolves every marked word and reads each pointer as
/// the second half of a memory ref, skipping errors. The property test and
/// the fuzz targets run it.
pub fn walk_igz(bytes: &[u8]) -> Result<(), FormatError> {
    let file = igz::Igz::parse(bytes)?;
    for object in file.objects() {
        for (_, resolved) in file.refs_of(object) {
            let _ = resolved;
        }
    }
    for &(at, marker) in file.markers() {
        let _ = file.reference(at, "walk");
        if marker == igz::Marker::Pointer
            && let Some(size_at) = at.0.checked_sub(4)
        {
            let _ = file.memory_ref(igz::Location(size_at), "walk");
        }
    }
    Ok(())
}
