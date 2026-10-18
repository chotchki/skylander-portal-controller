//! Edge geometry (openspec AC.7): a segment's indexes, vertex streams and
//! skin, decoded to bind pose.
//!
//! - Two SPU configs exist on the characters ([`Kind`]); others are refused.
//! - Descriptors give each attribute's format, semantic and RSX slot; SPU
//!   input 0 is predefined F32x3 positions.
//! - Formats: F32, F16, U8N and X11Y11Z10N (RSX's CMP word, read the
//!   SPU's way on the streams the SPU reads and signed normalized on the
//!   RSX-only one); the rest are refused.
//! - Skin: 4 x (u8 weight, u8 local matrix) per vertex, summing to 255.

pub mod indexes;

#[cfg(any(test, feature = "test-fixtures"))]
pub mod builder;
#[cfg(test)]
mod tests;

pub use indexes::{IndexStream, decompress};

use crate::igz::layouts::{EdgeGeometrySegment, Layout, SkeletonBone};
use crate::igz::{Igz, Location, MemoryRef};
use crate::{ErrorKind, FormatError};

pub(crate) const FORMAT: &str = "Edge";

pub const CONFIG_LEN: usize = 16;
/// Bytes per skin matrix (3x4 f32).
pub const MATRIX_BYTES: u16 = 48;

fn unsupported(field: &'static str, at: usize, value: u64) -> FormatError {
    FormatError::new(FORMAT, field, at, ErrorKind::Unsupported { value })
}

/// The two SPU configs on the disc's characters.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Kind {
    /// Flags 8, 1 uniform table, input 0, no secondary (6), output 0.
    Unskinned,
    /// Flags 8, 3 uniform tables, input 0, a secondary descriptor (0xFF),
    /// output 1, skin flavour 1 (no scaling).
    Skinned,
}

/// Config bytes a kind fixes. The others: command hole (1), counts (8-11).
const FIXED: [usize; 11] = [0, 2, 3, 4, 5, 6, 7, 12, 13, 14, 15];

impl Kind {
    /// Its config record with zero counts and command hole. Both have delta
    /// 0xFF, index flavour 3, matrix format 0 and indexes offset 0xFFFFFFFF.
    pub const fn template(self) -> [u8; CONFIG_LEN] {
        let (tables, secondary, output, skin) = match self {
            Self::Unskinned => (1, 6, 0, 0),
            Self::Skinned => (3, 0xFF, 1, 1),
        };
        [
            0x80 | tables,
            0,
            0,
            secondary,
            output,
            0xFF,
            0x30 | skin,
            0,
            0,
            0,
            0,
            0,
            0xFF,
            0xFF,
            0xFF,
            0xFF,
        ]
    }
}

/// `_spuConfigInfo`, Edge's 16-byte segment config.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SpuConfig {
    pub kind: Kind,
    /// Quadwords the SPU reserves for RSX commands.
    pub command_hole: u8,
    pub num_vertexes: u16,
    pub num_indexes: u16,
}

impl SpuConfig {
    /// Error offsets are into `bytes`.
    pub fn parse(bytes: &[u8]) -> Result<Self, FormatError> {
        if bytes.len() < CONFIG_LEN {
            let kind = ErrorKind::Truncated {
                needed: CONFIG_LEN,
                available: bytes.len(),
            };
            return Err(FormatError::new(FORMAT, "_spuConfigInfo", 0, kind));
        }
        if bytes.len() > CONFIG_LEN {
            return Err(unsupported(
                "_spuConfigInfo",
                CONFIG_LEN,
                bytes.len() as u64,
            ));
        }
        let kind = match bytes[6] & 0xF {
            0 => Kind::Unskinned,
            1 => Kind::Skinned,
            flavour => return Err(unsupported("skin flavour", 6, flavour.into())),
        };
        let t = kind.template();
        if let Some(&at) = FIXED.iter().find(|&&i| bytes[i] != t[i]) {
            return Err(unsupported("_spuConfigInfo", at, bytes[at].into()));
        }
        let num_indexes = u16::from_be_bytes([bytes[10], bytes[11]]);
        if !num_indexes.is_multiple_of(3) {
            return Err(unsupported("numIndexes", 10, num_indexes.into()));
        }
        Ok(Self {
            kind,
            command_hole: bytes[1],
            num_vertexes: u16::from_be_bytes([bytes[8], bytes[9]]),
            num_indexes,
        })
    }

    pub fn to_bytes(&self) -> [u8; CONFIG_LEN] {
        let mut b = self.kind.template();
        b[1] = self.command_hole;
        b[8..10].copy_from_slice(&self.num_vertexes.to_be_bytes());
        b[10..12].copy_from_slice(&self.num_indexes.to_be_bytes());
        b
    }
}

/// An attribute format. Edge's codes are RSX's `CELL_GCM_VERTEX_*` types.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum VertexFormat {
    F32,
    F16,
    U8N,
    /// One big-endian word: x bits 0-10, y 11-21, z 22-31, two's complement.
    X11Y11Z10N,
}

impl VertexFormat {
    pub const fn code(self) -> u8 {
        match self {
            Self::F32 => 2,
            Self::F16 => 3,
            Self::U8N => 4,
            Self::X11Y11Z10N => 6,
        }
    }

    /// I16N (1), I16 (5) and U8 (7) are never used, so they're refused
    /// with FIXED_POINT (8) and UNIT_VECTOR (9).
    pub const fn from_code(code: u8) -> Option<Self> {
        match code {
            2 => Some(Self::F32),
            3 => Some(Self::F16),
            4 => Some(Self::U8N),
            6 => Some(Self::X11Y11Z10N),
            _ => None,
        }
    }

    /// Bytes for `count` components.
    pub const fn size(self, count: u8) -> u32 {
        let c = count as u32;
        match self {
            Self::F32 => 4 * c,
            Self::F16 => 2 * c,
            Self::U8N => c,
            Self::X11Y11Z10N => 4,
        }
    }
}

/// Edge attribute ids 1-9.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Semantic {
    Position,
    Normal,
    Tangent,
    Binormal,
    Uv0,
    Uv1,
    Uv2,
    Uv3,
    Color,
}

impl Semantic {
    pub const ALL: [Self; 9] = [
        Self::Position,
        Self::Normal,
        Self::Tangent,
        Self::Binormal,
        Self::Uv0,
        Self::Uv1,
        Self::Uv2,
        Self::Uv3,
        Self::Color,
    ];

    pub const fn id(self) -> u8 {
        self as u8 + 1
    }

    pub fn from_id(id: u8) -> Option<Self> {
        Self::ALL.get(usize::from(id).checked_sub(1)?).copied()
    }
}

/// One attribute of a stream descriptor.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AttributeBlock {
    pub offset: u8,
    pub format: VertexFormat,
    pub count: u8,
    pub semantic: Semantic,
    pub size: u8,
    /// The RSX vertex input register.
    pub slot: u8,
}

pub const DESC_HEADER_LEN: usize = 8;
pub const BLOCK_LEN: usize = 8;

/// A vertex stream's layout: 8-byte header (attributes, stride, blocks, 5
/// zero bytes), then one 8-byte block per attribute.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StreamDesc {
    pub stride: u8,
    pub blocks: Vec<AttributeBlock>,
}

impl StreamDesc {
    /// SPU input format 0, which carries no descriptor.
    pub fn positions() -> Self {
        Self {
            stride: 12,
            blocks: vec![AttributeBlock {
                offset: 0,
                format: VertexFormat::F32,
                count: 3,
                semantic: Semantic::Position,
                size: 12,
                slot: 0,
            }],
        }
    }

    /// Error offsets are into `bytes`.
    pub fn parse(bytes: &[u8]) -> Result<Self, FormatError> {
        const FIELD: &str = "stream descriptor";
        if bytes.len() < DESC_HEADER_LEN {
            let kind = ErrorKind::Truncated {
                needed: DESC_HEADER_LEN,
                available: bytes.len(),
            };
            return Err(FormatError::new(FORMAT, FIELD, 0, kind));
        }
        let (attributes, stride, count) = (bytes[0], bytes[1], usize::from(bytes[2]));
        if let Some(i) = (3..DESC_HEADER_LEN).find(|&i| bytes[i] != 0) {
            return Err(unsupported(FIELD, i, bytes[i].into()));
        }
        // More blocks than attributes means fixed-point blocks: never used.
        if count != usize::from(attributes) {
            return Err(unsupported(FIELD, 2, count as u64));
        }
        let end = DESC_HEADER_LEN + BLOCK_LEN * count;
        if end > bytes.len() {
            let kind = ErrorKind::Truncated {
                needed: end,
                available: bytes.len(),
            };
            return Err(FormatError::new(FORMAT, FIELD, 0, kind));
        }
        if let Some(i) = bytes[end..].iter().position(|&b| b != 0) {
            return Err(unsupported(FIELD, end + i, bytes[end + i].into()));
        }
        let mut blocks: Vec<AttributeBlock> = Vec::with_capacity(count);
        for (i, b) in bytes[DESC_HEADER_LEN..end]
            .as_chunks::<BLOCK_LEN>()
            .0
            .iter()
            .enumerate()
        {
            let at = DESC_HEADER_LEN + BLOCK_LEN * i;
            let bad = |k: usize| unsupported(FIELD, at + k, b[k].into());
            let [offset, format, count, id, size, slot, fixed, pad] = *b;
            let format = VertexFormat::from_code(format).ok_or_else(|| bad(1))?;
            let counts = if format == VertexFormat::X11Y11Z10N {
                1..=1
            } else {
                1..=4
            };
            if !counts.contains(&count) {
                return Err(bad(2));
            }
            let semantic = Semantic::from_id(id).ok_or_else(|| bad(3))?;
            if blocks.iter().any(|x| x.semantic == semantic) {
                let kind = ErrorKind::Duplicate;
                return Err(FormatError::new(FORMAT, FIELD, at + 3, kind));
            }
            if u32::from(size) != format.size(count) {
                return Err(bad(4));
            }
            if slot >= 16 {
                return Err(bad(5));
            }
            if fixed != 0 {
                return Err(bad(6));
            }
            if pad != 0 {
                return Err(bad(7));
            }
            let (start, end) = (u32::from(offset), u32::from(offset) + u32::from(size));
            let overlaps = blocks.iter().any(|x| {
                start < u32::from(x.offset) + u32::from(x.size) && u32::from(x.offset) < end
            });
            if end > u32::from(stride) || overlaps {
                return Err(bad(0));
            }
            blocks.push(AttributeBlock {
                offset,
                format,
                count,
                semantic,
                size,
                slot,
            });
        }
        Ok(Self { stride, blocks })
    }
}

/// Where an attribute's bytes live.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Stream {
    Spu0,
    Spu1,
    /// RSX reads it in place; the SPU never sees it.
    RsxOnly,
}

/// One decoded attribute: `components` floats per vertex.
#[derive(Debug, Clone, PartialEq)]
pub struct Attribute {
    pub semantic: Semantic,
    pub stream: Stream,
    pub slot: u8,
    pub format: VertexFormat,
    /// 3 for X11Y11Z10N, else the block's count.
    pub components: u8,
    pub values: Vec<f32>,
}

impl Attribute {
    pub fn vertices(&self) -> std::slice::ChunksExact<'_, f32> {
        self.values.chunks_exact(usize::from(self.components))
    }

    /// `block` from `count` vertices `stride` apart; `data` holds them all.
    fn unpack(
        stream: Stream,
        block: &AttributeBlock,
        data: &[u8],
        stride: u8,
        count: usize,
    ) -> Self {
        let components = if block.format == VertexFormat::X11Y11Z10N {
            3
        } else {
            block.count
        };
        let mut values = Vec::with_capacity(count * usize::from(components));
        for v in 0..count {
            let at = v * usize::from(stride) + usize::from(block.offset);
            let field = &data[at..at + usize::from(block.size)];
            match block.format {
                VertexFormat::F32 => {
                    let words = field.as_chunks::<4>().0;
                    values.extend(words.iter().map(|c| f32::from_be_bytes(*c)));
                }
                VertexFormat::F16 => {
                    let halves = field.as_chunks::<2>().0;
                    values.extend(halves.iter().map(|c| f16_to_f32(u16::from_be_bytes(*c))));
                }
                VertexFormat::U8N => values.extend(field.iter().map(|&b| f32::from(b) / 255.0)),
                VertexFormat::X11Y11Z10N => {
                    let word = u32::from_be_bytes([field[0], field[1], field[2], field[3]]);
                    values.extend(match stream {
                        Stream::Spu0 | Stream::Spu1 => x11y11z10n_spu(word),
                        Stream::RsxOnly => x11y11z10n_rsx(word),
                    });
                }
            }
        }
        Self {
            semantic: block.semantic,
            stream,
            slot: block.slot,
            format: block.format,
            components,
            values,
        }
    }
}

/// Bits per CMP field, low to high: x, y, z.
const CMP_BITS: [u32; 3] = [11, 11, 10];

/// A CMP word's x, y and z fields as two's-complement integers.
fn cmp_fields(word: u32) -> [i32; 3] {
    let mut shift = 0;
    CMP_BITS.map(|bits| {
        let field = word << (32 - bits - shift);
        shift += bits;
        (field as i32) >> (32 - bits)
    })
}

/// A CMP word as the SPU's EdgeGeometry job reads it, the decode for every
/// stream the SPU reads (the disc's bind normals and tangents): a b-bit field
/// s is (2s + 1) / (2^b - 1). The codes span exactly -1..=1, symmetric about
/// zero, with no code at zero (word 0 is (1/2047, 1/2047, 1/1023)).
///
/// Evidence (AC.16, no Sony docs): brute force over AC.13's three RSX
/// captures of Eruptor, 4 decodes x 12 encodes x renormalizing or not. Only
/// this decode, a weighted blend of the bone rotations left unnormalized and
/// the encode floor((x (2^b - 1) - 1) / 2) turn his bind words into the
/// SPU's posed ones: 16,691 of 16,708 exact (next-best rule 9,141), each miss
/// 1 LSB on one component at a rounding tie; the June savestate's frame 4,
/// 5,721 of 5,722. Up to 1.439e-3 rad from [`x11y11z10n_rsx`] on his bind
/// words. It says nothing about how the GPU reads a CMP word.
pub fn x11y11z10n_spu(word: u32) -> [f32; 3] {
    let s = cmp_fields(word);
    std::array::from_fn(|i| (2 * s[i] + 1) as f32 / ((1 << CMP_BITS[i]) - 1) as f32)
}

/// A CMP word as signed normalized, the decode for the RSX-only stream the
/// GPU reads in place: a b-bit field s is s / (2^(b-1) - 1), the extra
/// negative code clamped to -1. How the real RSX reads CMP is unmeasured;
/// RPCS3 reads s 2^(16-b) / 32767, within 0.2% of this (AC.16).
pub fn x11y11z10n_rsx(word: u32) -> [f32; 3] {
    let s = cmp_fields(word);
    std::array::from_fn(|i| (s[i] as f32 / ((1 << (CMP_BITS[i] - 1)) - 1) as f32).max(-1.0))
}

/// IEEE half to f32, exactly.
pub fn f16_to_f32(h: u16) -> f32 {
    let sign = u32::from(h >> 15) << 31;
    let exp = u32::from((h >> 10) & 0x1F);
    let man = u32::from(h & 0x3FF);
    match exp {
        0 => (man as f32 * 2f32.powi(-24)).copysign(f32::from_bits(sign | 0x3F80_0000)),
        0x1F => f32::from_bits(sign | 0x7F80_0000 | (man << 13)),
        e => f32::from_bits(sign | ((e + 112) << 23) | (man << 13)),
    }
}

/// One of a vertex's four skin influences.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Influence {
    pub weight: u8,
    /// A local skin matrix, an index into [`Skin::palette_slots`].
    pub local: u8,
}

/// `_skinIndexesAndWeights` and the matrix ranges it indexes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Skin {
    pub influences: Vec<[Influence; 4]>,
    /// Local matrix to palette slot.
    pub palette_slots: Vec<u16>,
}

/// Local matrices to palette slots: two DMA ranges of 48-byte matrices land
/// back to back. `Err(i)` names the word (offsets 0-1, sizes 2-3) that isn't
/// whole matrices.
pub fn palette_slots(offsets: [u16; 2], sizes: [u16; 2]) -> Result<Vec<u16>, usize> {
    let words = [offsets[0], offsets[1], sizes[0], sizes[1]];
    if let Some(i) = words.iter().position(|w| !w.is_multiple_of(MATRIX_BYTES)) {
        return Err(i);
    }
    Ok((0..2)
        .flat_map(|r| {
            let first = offsets[r] / MATRIX_BYTES;
            (0..sizes[r] / MATRIX_BYTES).map(move |i| first + i)
        })
        .collect())
}

impl Skin {
    /// `count` vertices of influences. Error offsets are into `data`.
    pub fn parse(data: &[u8], count: usize, palette_slots: Vec<u16>) -> Result<Self, FormatError> {
        const FIELD: &str = "_skinIndexesAndWeights";
        let need = 8 * count;
        if data.len() < need {
            let kind = ErrorKind::Truncated {
                needed: need,
                available: data.len(),
            };
            return Err(FormatError::new(FORMAT, FIELD, 0, kind));
        }
        if data.len() >= need + 16 {
            return Err(unsupported(FIELD, need, data.len() as u64));
        }
        let mut influences = Vec::with_capacity(count);
        for (v, raw) in data[..need].as_chunks::<8>().0.iter().enumerate() {
            let mut out = [Influence::default(); 4];
            for (k, slot) in out.iter_mut().enumerate() {
                let (weight, local) = (raw[2 * k], raw[2 * k + 1]);
                if weight != 0 && usize::from(local) >= palette_slots.len() {
                    let kind = ErrorKind::OutOfRange {
                        value: local.into(),
                        limit: palette_slots.len() as u64,
                    };
                    return Err(FormatError::new(FORMAT, FIELD, 8 * v + 2 * k + 1, kind));
                }
                *slot = Influence { weight, local };
            }
            let sum: u32 = out.iter().map(|i| u32::from(i.weight)).sum();
            if sum != 255 {
                return Err(unsupported(FIELD, 8 * v, sum.into()));
            }
            influences.push(out);
        }
        Ok(Self {
            influences,
            palette_slots,
        })
    }

    /// Each vertex's influences as bones; a zero weight comes back as bone 0.
    /// Errors carry offset 0: the map spans several objects.
    pub fn bones(&self, map: &BoneMap) -> Result<Vec<[BoneWeight; 4]>, FormatError> {
        let mut out = Vec::with_capacity(self.influences.len());
        for v in &self.influences {
            let mut bones = [BoneWeight::default(); 4];
            for (b, i) in bones.iter_mut().zip(v) {
                if i.weight == 0 {
                    continue;
                }
                // `parse` checked weighted locals; a hand-built Skin may not hold.
                let Some(&slot) = self.palette_slots.get(usize::from(i.local)) else {
                    let kind = ErrorKind::OutOfRange {
                        value: i.local.into(),
                        limit: self.palette_slots.len() as u64,
                    };
                    return Err(FormatError::new(FORMAT, "_skinIndexesAndWeights", 0, kind));
                };
                let slot = usize::from(slot);
                let Some(bone) = map.slots.get(slot) else {
                    let kind = ErrorKind::OutOfRange {
                        value: slot as u64,
                        limit: map.slots.len() as u64,
                    };
                    return Err(FormatError::new(FORMAT, "_blendMatrixIndices", 0, kind));
                };
                let Some(bone) = *bone else {
                    let kind = ErrorKind::Missing;
                    return Err(FormatError::new(FORMAT, "_blendMatrixIndex", 0, kind));
                };
                *b = BoneWeight {
                    bone,
                    weight: i.weight,
                };
            }
            out.push(bones);
        }
        Ok(out)
    }
}

/// Palette slot to bone: `igBlendMatrixSelect._blendMatrixIndices` gives the
/// slot's blend matrix, and the one bone with that `_blendMatrixIndex` owns it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BoneMap {
    /// `None` when no bone, or more than one, claims the slot's matrix.
    pub slots: Vec<Option<usize>>,
}

impl BoneMap {
    pub fn new(blend_matrix_indices: &[i32], bones: &[SkeletonBone<'_>]) -> Self {
        let slots = blend_matrix_indices
            .iter()
            .map(|&m| {
                let mut owners = bones
                    .iter()
                    .enumerate()
                    .filter(|(_, b)| m >= 0 && b.blend_matrix_index == m);
                match (owners.next(), owners.next()) {
                    (Some((i, _)), None) => Some(i),
                    _ => None,
                }
            })
            .collect();
        Self { slots }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct BoneWeight {
    pub bone: usize,
    pub weight: u8,
}

/// A decoded segment in bind pose.
#[derive(Debug, Clone, PartialEq)]
pub struct Segment {
    pub config: SpuConfig,
    pub indexes: IndexStream,
    /// SPU input 0's positions first, then SPU input 1, then RSX-only.
    pub attributes: Vec<Attribute>,
    pub skin: Option<Skin>,
}

/// Turns a segment's refusals into errors at file offsets.
struct Ctx<'i, 'a> {
    igz: &'i Igz<'a>,
    object: Location,
}

impl<'a> Ctx<'_, 'a> {
    fn field(&self, name: &'static str, offset: u32, kind: ErrorKind) -> FormatError {
        let at = self.object.offset_by(offset).unwrap_or(self.object);
        self.igz.err_at(at, name, kind)
    }

    fn unsupported(&self, name: &'static str, offset: u32, value: u64) -> FormatError {
        self.field(name, offset, ErrorKind::Unsupported { value })
    }

    /// `e`, whose offset is into `m`, at its file offset.
    fn inside(&self, m: &MemoryRef<'_>, e: FormatError) -> FormatError {
        let mut out = self.igz.err_at(m.at, e.field, e.kind);
        out.offset += e.offset;
        out
    }

    fn required(
        &self,
        m: Option<MemoryRef<'a>>,
        name: &'static str,
        offset: u32,
    ) -> Result<MemoryRef<'a>, FormatError> {
        m.ok_or_else(|| self.field(name, offset, ErrorKind::Missing))
    }

    fn absent(
        &self,
        m: Option<MemoryRef<'_>>,
        name: &'static str,
        offset: u32,
    ) -> Result<(), FormatError> {
        match m {
            None => Ok(()),
            Some(m) => Err(self.unsupported(name, offset, m.data.len() as u64)),
        }
    }

    fn stream(
        &self,
        out: &mut Vec<Attribute>,
        stream: Stream,
        desc: &StreamDesc,
        data: &MemoryRef<'_>,
        count: usize,
        name: &'static str,
    ) -> Result<(), FormatError> {
        let need = usize::from(desc.stride) * count;
        let len = data.data.len();
        if len < need {
            let kind = ErrorKind::Truncated {
                needed: need,
                available: len,
            };
            return Err(self.igz.err_at(data.at, name, kind));
        }
        // SPU streams are whole quadwords; RSX-only is exact.
        let max_tail = if stream == Stream::RsxOnly { 0 } else { 15 };
        if len - need > max_tail {
            return Err(self.inside(data, unsupported(name, need, len as u64)));
        }
        for b in &desc.blocks {
            if out.iter().any(|a| a.semantic == b.semantic) {
                return Err(self.igz.err_at(data.at, name, ErrorKind::Duplicate));
            }
            out.push(Attribute::unpack(stream, b, data.data, desc.stride, count));
        }
        Ok(())
    }
}

type S<'a> = EdgeGeometrySegment<'a>;

impl Segment {
    /// Reads `igz.objects()[index]`, an `igPS3EdgeGeometrySegment`, and
    /// decodes it. Errors sit at the field's or the byte's file offset.
    pub fn read(igz: &Igz<'_>, index: usize) -> Result<Self, FormatError> {
        let s = S::read(igz, index)?;
        Self::decode(igz, index, &s)
    }

    /// [`Segment::read`] from a layout already read.
    pub fn decode(igz: &Igz<'_>, index: usize, s: &S<'_>) -> Result<Self, FormatError> {
        let cx = Ctx {
            igz,
            object: igz.objects()[index].at,
        };
        let m = cx.required(s.spu_config_info, "_spuConfigInfo", S::SPU_CONFIG_INFO)?;
        let config = SpuConfig::parse(m.data).map_err(|e| cx.inside(&m, e))?;
        let count = usize::from(config.num_vertexes);

        // Blend shapes are never used.
        if s.num_blend_shapes != 0 {
            let value = s.num_blend_shapes.into();
            return Err(cx.unsupported("_numBlendShapes", S::NUM_BLEND_SHAPES, value));
        }
        if s.blend_shape_sizes.count != 0 {
            let value = s.blend_shape_sizes.count.into();
            return Err(cx.unsupported("_blendShapeSizes", S::BLEND_SHAPE_SIZES, value));
        }
        if s.blend_shapes.count != 0 {
            let value = s.blend_shapes.count.into();
            return Err(cx.unsupported("_blendShapes", S::BLEND_SHAPES, value));
        }
        cx.absent(s.blend_shape_data, "_blendShapeData", S::BLEND_SHAPE_DATA)?;

        let m = cx.required(s.indexes, "_indexes", S::INDEXES)?;
        if usize::from(s.indexes_sizes[0]) != m.data.len() || s.indexes_sizes[1] != 0 {
            let value = s.indexes_sizes[0].into();
            return Err(cx.unsupported("_indexesSizes", S::INDEXES_SIZES, value));
        }
        let indexes = decompress(m.data, config.num_indexes, config.num_vertexes)
            .map_err(|e| cx.inside(&m, e))?;

        let attributes = Self::streams(&cx, s, config.kind, count)?;
        let skin = Self::skin(&cx, s, config.kind, count)?;
        Ok(Self {
            config,
            indexes,
            attributes,
            skin,
        })
    }

    fn streams(
        cx: &Ctx<'_, '_>,
        s: &S<'_>,
        kind: Kind,
        count: usize,
    ) -> Result<Vec<Attribute>, FormatError> {
        let mut out = Vec::new();
        // SPU input 0 is predefined format 0 on both kinds.
        let at = S::SPU_INPUT_STREAM_DESCS0;
        cx.absent(s.spu_input_stream_descs0, "_spuInputStreamDescs0", at)?;
        let [desc0_size, desc1_size] = s.spu_input_stream_desc_sizes;
        let sizes_at = S::SPU_INPUT_STREAM_DESC_SIZES;
        if desc0_size != 0 {
            let name = "_spuInputStreamDescSizes";
            return Err(cx.unsupported(name, sizes_at, desc0_size.into()));
        }
        let v0 = cx.required(s.spu_vertexes0, "_spuVertexes0", S::SPU_VERTEXES0)?;
        let positions = StreamDesc::positions();
        cx.stream(
            &mut out,
            Stream::Spu0,
            &positions,
            &v0,
            count,
            "_spuVertexes0",
        )?;

        let desc1_at = S::SPU_INPUT_STREAM_DESCS1;
        match kind {
            Kind::Unskinned => {
                cx.absent(s.spu_vertexes1, "_spuVertexes1", S::SPU_VERTEXES1)?;
                cx.absent(s.spu_input_stream_descs1, "_spuInputStreamDescs1", desc1_at)?;
                if desc1_size != 0 {
                    let name = "_spuInputStreamDescSizes";
                    return Err(cx.unsupported(name, sizes_at + 2, desc1_size.into()));
                }
            }
            Kind::Skinned => {
                let d = s.spu_input_stream_descs1;
                let d = cx.required(d, "_spuInputStreamDescs1", desc1_at)?;
                if usize::from(desc1_size) != d.data.len() {
                    let name = "_spuInputStreamDescSizes";
                    return Err(cx.unsupported(name, sizes_at + 2, desc1_size.into()));
                }
                let desc = StreamDesc::parse(d.data).map_err(|e| cx.inside(&d, e))?;
                let v1 = cx.required(s.spu_vertexes1, "_spuVertexes1", S::SPU_VERTEXES1)?;
                cx.stream(&mut out, Stream::Spu1, &desc, &v1, count, "_spuVertexes1")?;
            }
        }

        // Outputs 0 and 1 are predefined.
        let at = S::SPU_OUTPUT_STREAM_DESC;
        cx.absent(s.spu_output_stream_desc, "_spuOutputStreamDesc", at)?;
        if s.spu_output_stream_desc_size != 0 {
            let value = s.spu_output_stream_desc_size.into();
            let at = S::SPU_OUTPUT_STREAM_DESC_SIZE;
            return Err(cx.unsupported("_spuOutputStreamDescSize", at, value));
        }

        match (s.rsx_only_stream_desc, s.rsx_only_vertexes) {
            // Size words of absent buffers are zero on the disc.
            (None, None) => {
                if s.rsx_only_stream_desc_size != 0 {
                    let value = s.rsx_only_stream_desc_size.into();
                    let at = S::RSX_ONLY_STREAM_DESC_SIZE;
                    return Err(cx.unsupported("_rsxOnlyStreamDescSize", at, value));
                }
                if s.rsx_only_vertexes_size != 0 {
                    let value = s.rsx_only_vertexes_size.into();
                    let at = S::RSX_ONLY_VERTEXES_SIZE;
                    return Err(cx.unsupported("_rsxOnlyVertexesSize", at, value));
                }
            }
            (Some(_), None) => {
                let at = S::RSX_ONLY_VERTEXES;
                return Err(cx.field("_rsxOnlyVertexes", at, ErrorKind::Missing));
            }
            (None, Some(_)) => {
                let at = S::RSX_ONLY_STREAM_DESC;
                return Err(cx.field("_rsxOnlyStreamDesc", at, ErrorKind::Missing));
            }
            (Some(d), Some(v)) => {
                if usize::from(s.rsx_only_stream_desc_size) != d.data.len() {
                    let value = s.rsx_only_stream_desc_size.into();
                    let at = S::RSX_ONLY_STREAM_DESC_SIZE;
                    return Err(cx.unsupported("_rsxOnlyStreamDescSize", at, value));
                }
                if s.rsx_only_vertexes_size as usize != v.data.len() {
                    let value = s.rsx_only_vertexes_size.into();
                    let at = S::RSX_ONLY_VERTEXES_SIZE;
                    return Err(cx.unsupported("_rsxOnlyVertexesSize", at, value));
                }
                let desc = StreamDesc::parse(d.data).map_err(|e| cx.inside(&d, e))?;
                let name = "_rsxOnlyVertexes";
                cx.stream(&mut out, Stream::RsxOnly, &desc, &v, count, name)?;
            }
        }
        Ok(out)
    }

    fn skin(
        cx: &Ctx<'_, '_>,
        s: &S<'_>,
        kind: Kind,
        count: usize,
    ) -> Result<Option<Skin>, FormatError> {
        let name = "_skinIndexesAndWeights";
        let at = S::SKIN_INDEXES_AND_WEIGHTS;
        if kind == Kind::Unskinned {
            cx.absent(s.skin_indexes_and_weights, name, at)?;
            // No skin, so every skin size and offset word is zero.
            let words = [
                (
                    "_skinMatricesByteOffsets",
                    S::SKIN_MATRICES_BYTE_OFFSETS,
                    s.skin_matrices_byte_offsets,
                ),
                (
                    "_skinMatricesSizes",
                    S::SKIN_MATRICES_SIZES,
                    s.skin_matrices_sizes,
                ),
                (
                    "_skinIndexesAndWeightsSizes",
                    S::SKIN_INDEXES_AND_WEIGHTS_SIZES,
                    s.skin_indexes_and_weights_sizes,
                ),
            ];
            for (name, at, w) in words {
                if let Some(i) = w.iter().position(|&x| x != 0) {
                    return Err(cx.unsupported(name, at + 2 * i as u32, w[i].into()));
                }
            }
            return Ok(None);
        }
        let m = cx.required(s.skin_indexes_and_weights, name, at)?;
        let sizes = s.skin_indexes_and_weights_sizes;
        if sizes.iter().map(|&x| usize::from(x)).sum::<usize>() != m.data.len() {
            let at = S::SKIN_INDEXES_AND_WEIGHTS_SIZES;
            let name = "_skinIndexesAndWeightsSizes";
            return Err(cx.unsupported(name, at, sizes[0].into()));
        }
        let (offsets, sizes) = (s.skin_matrices_byte_offsets, s.skin_matrices_sizes);
        let slots = palette_slots(offsets, sizes).map_err(|i| {
            let (name, at, words) = if i < 2 {
                let at = S::SKIN_MATRICES_BYTE_OFFSETS;
                ("_skinMatricesByteOffsets", at, offsets)
            } else {
                ("_skinMatricesSizes", S::SKIN_MATRICES_SIZES, sizes)
            };
            cx.unsupported(name, at + 2 * (i as u32 % 2), words[i % 2].into())
        })?;
        let skin = Skin::parse(m.data, count, slots).map_err(|e| cx.inside(&m, e))?;
        Ok(Some(skin))
    }

    pub fn vertex_count(&self) -> usize {
        usize::from(self.config.num_vertexes)
    }

    pub fn triangles(&self) -> impl Iterator<Item = [u16; 3]> + '_ {
        self.indexes.triangles()
    }

    pub fn attribute(&self, semantic: Semantic) -> Option<&Attribute> {
        self.attributes.iter().find(|a| a.semantic == semantic)
    }

    pub fn positions(&self) -> impl Iterator<Item = [f32; 3]> + '_ {
        self.attribute(Semantic::Position)
            .into_iter()
            .flat_map(Attribute::vertices)
            .map(|p| [p[0], p[1], p[2]])
    }

    /// The positions' min and max corners; `None` with no vertices.
    pub fn bounds(&self) -> Option<[[f32; 3]; 2]> {
        self.positions().fold(None, |acc, p| {
            let [lo, hi] = acc.unwrap_or([p, p]);
            Some([
                std::array::from_fn(|k| lo[k].min(p[k])),
                std::array::from_fn(|k| hi[k].max(p[k])),
            ])
        })
    }
}
