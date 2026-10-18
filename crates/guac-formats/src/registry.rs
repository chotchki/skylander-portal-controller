//! Every parser in this crate, as the property test and the fuzz targets see it.
//!
//! Registering a parser is one entry in [`PARSERS`]: how to run it, the valid
//! files its builder makes (the property test mutates those) and the most heap
//! it may hold at once for an input of a given length.

use crate::FormatError;
use crate::{edge, iga, igz, sfo, texture};

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
    RegisteredParser {
        name: "igz_layouts",
        parse: walk_layouts,
        seeds: igz::layouts::builder::seeds,
        // The IGZ parse, plus one reader's output at a time and a hero's
        // maps, each entry read from at least 4 input bytes.
        heap_budget: |len| 128 * len + (64 << 10),
    },
    RegisteredParser {
        name: "igimage2_ktx2",
        parse: walk_textures,
        seeds: texture::builder::seeds,
        // The IGZ parse, plus one image's levels and its KTX2 at a time:
        // each about its pixel block, which the input holds.
        heap_budget: |len| 64 * len + 2 * len + (64 << 10),
    },
    RegisteredParser {
        name: "edge_geometry",
        parse: walk_edge,
        seeds: edge::builder::seeds,
        // The IGZ parse, plus one segment at a time: indexes are 2 bytes
        // each, at least 3 bits of input per triangle (24x), and attributes
        // are at most 4 floats per input byte (U8N, 16x).
        heap_budget: |len| 64 * len + 48 * len + (256 << 10),
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

/// Parses an IGZ, then runs every AC.4 layout reader and the hero walk on
/// every object, dropping each result. The property test and the fuzz
/// targets run it.
pub fn walk_layouts(bytes: &[u8]) -> Result<(), FormatError> {
    use igz::layouts::*;
    let file = igz::Igz::parse(bytes)?;
    for i in 0..file.objects().len() {
        let _ = ObjectList::read(&file, i);
        let _ = Image2::read(&file, i);
        let _ = EdgeGeometrySegment::read(&file, i);
        let _ = EdgeGeometry::read(&file, i);
        let _ = EdgeGeometryAttr::read(&file, i);
        let _ = Skeleton2::read(&file, i);
        let _ = SkeletonBoneList::read(&file, i);
        let _ = SkeletonBone::read(&file, i);
        let _ = PhysicsCombinerLink::read(&file, i);
        let _ = NodeList::read(&file, i);
        let _ = BlendMatrixSelect::read(&file, i);
        let _ = IntList::read(&file, i);
        let _ = Actor2::read(&file, i);
        let _ = RuntimeTechniqueInstance::read(&file, i);
        let _ = PhysicsAnimation::read(&file, i);
        let _ = PhysicsBody::read(&file, i);
        let _ = PhysicsModel::read(&file, i);
        let _ = AnimationInfoList::read(&file, i);
        let _ = AnimationInfo::read(&file, i);
        let _ = Hero::read(&file, i);
    }
    Ok(())
}

/// How many untiled bytes [`walk_textures`] converts before it stops. Images
/// can share one block, so an input can ask for far more than it holds.
pub const TEXTURE_WALK_BUDGET: usize = 4 << 20;

/// Parses an IGZ, then untiles every `igImage2` and writes it as KTX2,
/// dropping each and skipping errors, until [`TEXTURE_WALK_BUDGET`] untiled
/// bytes have come out. The property test and the fuzz targets run it.
pub fn walk_textures(bytes: &[u8]) -> Result<(), FormatError> {
    converted_textures(bytes).map(drop)
}

/// [`walk_textures`], counting the images it converted.
fn converted_textures(bytes: &[u8]) -> Result<usize, FormatError> {
    let file = igz::Igz::parse(bytes)?;
    let mut left = TEXTURE_WALK_BUDGET;
    let mut converted = 0;
    for (i, _) in igz::layouts::read_all::<igz::layouts::Image2>(&file) {
        // A refused image returns before untiling, so it costs nothing.
        if let Ok(t) = texture::Texture::read(&file, i) {
            let n: usize = t.levels.iter().map(Vec::len).sum();
            drop(t.to_ktx2());
            converted += 1;
            left = left.saturating_sub(n.max(1));
            if left == 0 {
                break;
            }
        }
    }
    Ok(converted)
}

/// How many buffer bytes [`walk_edge`] hands the decoder before it stops.
/// Segments can share buffers, so an input can ask for far more than it holds.
pub const EDGE_WALK_BUDGET: usize = 4 << 20;

/// Parses an IGZ, then decodes every `igPS3EdgeGeometrySegment`, dropping
/// each and skipping errors, until [`EDGE_WALK_BUDGET`] is spent. Refused
/// segments are charged too. The property test and the fuzz targets run it.
pub fn walk_edge(bytes: &[u8]) -> Result<(), FormatError> {
    decoded_segments(bytes).map(drop)
}

/// A segment's charge: its buffers plus its decoded indexes.
pub fn edge_cost(s: &igz::layouts::EdgeGeometrySegment<'_>) -> usize {
    let len = |m: Option<igz::MemoryRef<'_>>| m.map_or(0, |m| m.data.len());
    let indexes = s.counts().map_or(0, |(_, i)| 2 * usize::from(i));
    [
        s.indexes,
        s.spu_vertexes0,
        s.spu_vertexes1,
        s.rsx_only_vertexes,
    ]
    .into_iter()
    .map(len)
    .sum::<usize>()
        + len(s.skin_indexes_and_weights)
        + indexes
}

/// [`walk_edge`], counting the segments it decoded.
fn decoded_segments(bytes: &[u8]) -> Result<usize, FormatError> {
    use igz::layouts::{EdgeGeometrySegment, read_all};
    let file = igz::Igz::parse(bytes)?;
    let mut left = EDGE_WALK_BUDGET;
    let mut decoded = 0;
    for (i, layout) in read_all::<EdgeGeometrySegment>(&file) {
        let Ok(layout) = layout else { continue };
        left = left.saturating_sub(edge_cost(&layout).max(1));
        decoded += usize::from(edge::Segment::decode(&file, i, &layout).is_ok());
        if left == 0 {
            break;
        }
    }
    Ok(decoded)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn edge_walk_stops_at_its_budget_on_shared_buffers() {
        // 600 positions (7,200 bytes), a 144-byte index stream and 1,200
        // bytes of decoded indexes per segment: 491 fill the budget.
        let bytes = edge::builder::shared(2000, 200);
        assert_eq!(
            decoded_segments(&bytes),
            Ok(EDGE_WALK_BUDGET.div_ceil(8_544))
        );
        assert_eq!(decoded_segments(&edge::builder::shared(3, 40)), Ok(3));
    }

    #[test]
    fn texture_walk_stops_at_its_budget_on_a_shared_block() {
        // 500 images over one 1 MiB block: four fill the budget.
        let bytes = texture::builder::shared_l8(500, 1024);
        assert!(bytes.len() < (2 << 20));
        assert_eq!(converted_textures(&bytes), Ok(4));
        assert_eq!(
            converted_textures(&texture::builder::shared_l8(3, 64)),
            Ok(3)
        );
    }
}
