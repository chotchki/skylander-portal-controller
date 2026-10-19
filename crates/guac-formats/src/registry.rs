//! Every parser in this crate, as the property test and the fuzz targets see it.
//!
//! Registering a parser is one entry in [`PARSERS`]: how to run it, the valid
//! files its builder makes (the property test mutates those) and the most heap
//! it may hold at once for an input of a given length.

use crate::FormatError;
use crate::{edge, exports, havok, iga, igz, language, roster, sfo, texture, toys};

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
        // The IGZ parse, plus one image's levels and its KTX2 at a time,
        // each about its pixel block, which the input holds, then level 0
        // as RGBA8: 8 bytes a BC1 byte, 4 a BC3 or L8 one.
        heap_budget: |len| 64 * len + 2 * len + 8 * len + (64 << 10),
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
    RegisteredParser {
        name: "roster",
        parse: walk_roster,
        seeds: roster_seeds,
        // The IGZ parse, then a language pack's strings (a 24-byte String
        // per NUL, 2 input bytes at least, and up to 3 bytes of UTF-8 per 2
        // of UTF-16, doubled by growth), the join's and the texts' key maps
        // (per StringInfo of 40 bytes) and both root-list copies.
        heap_budget: |len| 64 * len + 64 * len + (64 << 10),
    },
    RegisteredParser {
        name: "toy_table",
        parse: walk_toy_table,
        seeds: toy_table_seeds,
        // Five IGZ parses (64x their parts), then the table, the figures and
        // the texts: each call, set member, sprite or key costs well under
        // its input bytes (a call spans a dozen ops of 28 bytes or more).
        heap_budget: |len| 64 * len + 64 * len + (64 << 10),
    },
    RegisteredParser {
        name: "havok",
        parse: walk_havok,
        seeds: havok::builder::seeds,
        // The fixup maps, then decoded tracks and one sampled pose: each
        // track costs at least its 4 mask bytes, each element its own
        // input bytes (overlapping arrays are refused).
        heap_budget: |len| 160 * len + (64 << 10),
    },
];

/// The roster readers' seeds (AE.1): a level joined to its pack, a
/// bootstrap, a global importing from both files, and the layouts' script
/// sample.
pub fn roster_seeds() -> Vec<Vec<u8>> {
    use exports::builder::{bootstrap, names, rooted};
    vec![
        language::builder::joined(&["a", "b", "c"], language::ENGLISH),
        bootstrap(),
        rooted(&names("global", 24), &[0x03, 0x14, 0x2B, 0x2C]),
        igz::layouts::builder::script_sample().bytes,
    ]
}

/// Parses an IGZ, then runs the roster readers (AE.1) over it: its language
/// pack joined to its own `StringInfo`s, and its root list as both
/// bootstrap and global with every `global.bld` import resolved, each error
/// skipped. The property test and the fuzz targets run it.
pub fn walk_roster(bytes: &[u8]) -> Result<(), FormatError> {
    let file = igz::Igz::parse(bytes)?;
    if let Ok(pack) = language::LanguagePack::read(&file) {
        let _ = language::join(&file, &pack);
        let _ = language::Texts::new(&file, vec![pack]);
    }
    if let Ok(exports) = exports::Exports::new(&file, &file) {
        for id in file.external_ids() {
            let _ = exports.resolve_import(id);
        }
        let _ = exports.resolve(exports.global_base());
    }
    Ok(())
}

/// The toy table's seeds (AE.2): the sample disc bundled with its English
/// pack, and with its French one and no alt characters.
pub fn toy_table_seeds() -> Vec<Vec<u8>> {
    use roster::builder::{bundle, disc, sample, sample_bundle};
    let mut spec = sample();
    spec.alt_characters.clear();
    let d = disc(&spec);
    vec![
        sample_bundle(),
        bundle(&[
            &d.init_setup,
            &d.global,
            &d.bootstrap,
            &d.collection,
            &d.packs[1],
        ]),
    ]
}

/// Splits a bundle ([`roster::builder::bundle`]'s format, a big-endian u32
/// length before each part) into its parts; a length past the end ends it.
pub fn unbundle(mut bytes: &[u8]) -> Vec<&[u8]> {
    let mut out = Vec::new();
    while let Some((len, rest)) = bytes.split_first_chunk::<4>() {
        let len = u32::from_be_bytes(*len) as usize;
        if len > rest.len() {
            break;
        }
        let (part, rest) = rest.split_at(len);
        out.push(part);
        bytes = rest;
    }
    out
}

/// Reads a bundled init_setup, global, bootstrap, collection and pack as the
/// roster does (AE.2): the export ids, the texts, the toy table, the figures
/// and each portrait's image, each error skipped. The property test and the
/// fuzz targets run it.
pub fn walk_toy_table(bytes: &[u8]) -> Result<(), FormatError> {
    let parts = unbundle(bytes);
    let [init, global, boot, collection, pack, ..] = parts[..] else {
        return Ok(());
    };
    let init = igz::Igz::parse(init)?;
    let global = igz::Igz::parse(global)?;
    let boot = igz::Igz::parse(boot)?;
    let collection = igz::Igz::parse(collection)?;
    if let Ok(pack) = igz::Igz::parse(pack).and_then(|p| language::LanguagePack::read(&p)) {
        let _ = language::Texts::new(&global, vec![pack]);
    }
    let exports = exports::Exports::new(&boot, &global)?;
    let table = toys::ToyTable::read(&init, &global, &exports)?;
    let figures = roster::figures(&table, &global, &collection)?;
    for f in &figures {
        if let Some(p) = f.portrait {
            let _ = roster::portrait_image(&collection, p);
        }
    }
    Ok(())
}

/// Parses a packfile, then samples its clip at the start, middle and end,
/// posed on each skeleton it fits. The property test and the fuzz targets
/// run it.
pub fn walk_havok(bytes: &[u8]) -> Result<(), FormatError> {
    let c = havok::Container::parse(bytes)?;
    for s in &c.skeletons {
        drop(s.bind_pose());
    }
    if let Some(clip) = &c.clip {
        let end = f64::from(clip.animation.duration);
        for t in [0.0, end / 2.0, end] {
            drop(clip.animation.sample(t));
            for s in &c.skeletons {
                if let Some(local) = clip.local_pose(s, t) {
                    drop(s.model_pose(&local));
                }
            }
        }
    }
    Ok(())
}

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

/// Parses an IGZ, then runs every AC.4 (and AC.10 material, AE.1 roster) layout reader and the hero walk on
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
        let _ = AttrList::read(&file, i);
        let _ = AlphaTextureUnitIdAttr::read(&file, i);
        let _ = TextureAttr2::read(&file, i);
        let _ = Hero::read(&file, i);
        let _ = StringInfo::read(&file, i);
        let _ = ValueInfo::read(&file, i);
        let _ = ScriptSet::read(&file, i);
        let _ = ScriptSetList::read(&file, i);
        let _ = DirectionalLightInfo::read(&file, i);
        let _ = LanguagePackInfo::read(&file, i);
        let _ = SpriteInfo::read(&file, i);
        let _ = AnimatableTextureDataInfo::read(&file, i);
        let _ = RootList::read(&file, i);
        let _ = OpCode::read(&file, i);
        let _ = ScriptInfo::read(&file, i);
        let _ = OpCodeList::read(&file, i);
        let _ = OpUseMacro::read(&file, i);
        let _ = OpMacroInterface::read(&file, i);
        let _ = OpDefineMacro::read(&file, i);
        let _ = Variable::read(&file, i);
        let _ = OpMacroParameter::read(&file, i);
        let _ = OpChangeMembership::read(&file, i);
        let _ = OpLoopValue::read(&file, i);
        let _ = Stack::read(&file, i);
        let _ = RhsValueStack::read(&file, i);
        let _ = RhsReferenceStack::read(&file, i);
        let _ = ValueRhsVariant::read(&file, i);
    }
    Ok(())
}

/// How many untiled bytes [`walk_textures`] converts before it stops. Images
/// can share one block, so an input can ask for far more than it holds.
pub const TEXTURE_WALK_BUDGET: usize = 4 << 20;

/// Parses an IGZ, then untiles every `igImage2`, writes it as KTX2 and
/// decodes its level 0, dropping each and skipping errors, until
/// [`TEXTURE_WALK_BUDGET`] untiled bytes have come out. The property test and
/// the fuzz targets run it.
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
            // Level 0 as a portrait PNG would hold it (AE.3).
            drop(t.rgba8(0).expect("an untiled level decodes"));
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
    fn a_bundle_splits_into_its_parts_and_stops_at_an_overlong_length() {
        let b = roster::builder::bundle(&[b"ab", b"", b"cde"]);
        assert_eq!(unbundle(&b), [&b"ab"[..], b"", b"cde"]);
        let mut cut = b.clone();
        cut.truncate(b.len() - 1);
        assert_eq!(unbundle(&cut), [&b"ab"[..], b""]);
        assert!(unbundle(&[0, 0, 1]).is_empty());
    }

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
