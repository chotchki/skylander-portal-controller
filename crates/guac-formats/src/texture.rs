//! `igImage2` pixels to KTX2 (openspec AC.6): the original blocks, untiled,
//! never re-encoded.
//!
//! - `_tile` is RSX's swizzled layout: power-of-two only, mips packed with no
//!   row padding, the chain rounded up to 128 bytes. Linear images are
//!   single-level on this disc.
//! - RSX ignores the swizzle for DXT, so blocks run in rows either way;
//!   uncompressed texels are Morton-ordered under it ([`morton`]).
//! - Blocks are already in BC byte order, so the endian fix is the identity.

mod ktx;

#[cfg(any(test, feature = "test-fixtures"))]
pub mod builder;
#[cfg(test)]
mod tests;

use crate::igz::Igz;
use crate::igz::layouts::{Image2, Layout};
use crate::{ErrorKind, FormatError};

const FORMAT: &str = "igImage2";

/// A KTX2 target format.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Format {
    /// DXT1; RSX honours its one-bit alpha.
    Bc1RgbaUnorm,
    /// DXT5.
    Bc3Unorm,
    /// L8, written with a `rrr1` swizzle.
    R8Unorm,
}

impl Format {
    /// The `VkFormat` KTX2 stores.
    pub const fn vk_format(self) -> u32 {
        match self {
            Self::Bc1RgbaUnorm => 133,
            Self::Bc3Unorm => 137,
            Self::R8Unorm => 9,
        }
    }

    /// Texels along one edge of a block.
    pub const fn block_edge(self) -> u32 {
        match self {
            Self::Bc1RgbaUnorm | Self::Bc3Unorm => 4,
            Self::R8Unorm => 1,
        }
    }

    pub const fn block_bytes(self) -> u32 {
        match self {
            Self::Bc1RgbaUnorm => 8,
            Self::Bc3Unorm => 16,
            Self::R8Unorm => 1,
        }
    }

    /// Bytes in one `width` x `height` level.
    pub fn level_size(self, width: u32, height: u32) -> u64 {
        let e = self.block_edge();
        u64::from(width.div_ceil(e)) * u64::from(height.div_ceil(e)) * u64::from(self.block_bytes())
    }

    /// Bytes in a chain of `levels` levels, each half the last, down to 1.
    pub fn chain_size(self, width: u32, height: u32, levels: u32) -> u64 {
        (0..levels)
            .map(|l| self.level_size(level_dim(width, l), level_dim(height, l)))
            .sum()
    }
}

/// `dim` at mip `level`, never below 1.
pub fn level_dim(dim: u32, level: u32) -> u32 {
    dim.checked_shr(level).unwrap_or(0).max(1)
}

/// A metaimage `igImage2._format` imports, by name.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MetaImage {
    pub name: &'static str,
    pub format: Format,
    /// `igMetaImage._isTile`: RSX's swizzled layout.
    pub tiled: bool,
}

/// Every metaimage the character archives use (AC.6's census).
pub const META_IMAGES: [MetaImage; 5] = [
    meta("dxt1_big_ps3", Format::Bc1RgbaUnorm, false),
    meta("dxt1_tile_big_ps3", Format::Bc1RgbaUnorm, true),
    meta("dxt5_big_ps3", Format::Bc3Unorm, false),
    meta("dxt5_tile_big_ps3", Format::Bc3Unorm, true),
    meta("l8_tile_big_ps3", Format::R8Unorm, true),
];

const fn meta(name: &'static str, format: Format, tiled: bool) -> MetaImage {
    MetaImage {
        name,
        format,
        tiled,
    }
}

/// An image ready for KTX2: level 0 first, each in rows of blocks, top down.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Texture {
    pub format: Format,
    pub width: u32,
    pub height: u32,
    pub levels: Vec<Vec<u8>>,
}

/// Why an image was refused, before it has a place in a file.
enum Refusal {
    /// An `igImage2` field, by its offset in the object.
    Field(&'static str, u32, ErrorKind),
    /// The pixel block, by an offset into it.
    Data(usize, ErrorKind),
}

impl MetaImage {
    pub fn by_name(name: &str) -> Option<&'static MetaImage> {
        META_IMAGES.iter().find(|m| m.name == name)
    }

    /// Untiles a whole pixel block. Error offsets are into `data`.
    pub fn untile(
        &self,
        width: u16,
        height: u16,
        level_count: u16,
        data: &[u8],
    ) -> Result<Texture, FormatError> {
        self.untile_inner(width, height, level_count, data)
            .map_err(|r| match r {
                Refusal::Field(field, _, kind) => FormatError::new(FORMAT, field, 0, kind),
                Refusal::Data(at, kind) => FormatError::new(FORMAT, "_data", at, kind),
            })
    }

    fn untile_inner(
        &self,
        width: u16,
        height: u16,
        level_count: u16,
        data: &[u8],
    ) -> Result<Texture, Refusal> {
        let (w, h, n) = (u32::from(width), u32::from(height), u32::from(level_count));
        let unsupported = |v: u32| ErrorKind::Unsupported { value: v.into() };
        if w == 0 {
            return Err(Refusal::Field("_width", Image2::WIDTH, unsupported(0)));
        }
        if h == 0 {
            return Err(Refusal::Field("_height", Image2::HEIGHT, unsupported(0)));
        }
        // RSX swizzles power-of-two images only.
        if self.tiled && !w.is_power_of_two() {
            return Err(Refusal::Field("_width", Image2::WIDTH, unsupported(w)));
        }
        if self.tiled && !h.is_power_of_two() {
            return Err(Refusal::Field("_height", Image2::HEIGHT, unsupported(h)));
        }
        let most = 1 + w.max(h).ilog2();
        if n == 0 || n > most {
            let kind = ErrorKind::OutOfRange {
                value: n.into(),
                limit: most.into(),
            };
            return Err(Refusal::Field("_levelCount", Image2::LEVEL_COUNT, kind));
        }
        // Linear mips would share level 0's pitch; Giants writes none.
        if !self.tiled && n > 1 {
            return Err(Refusal::Field(
                "_levelCount",
                Image2::LEVEL_COUNT,
                unsupported(n),
            ));
        }
        let expected = self.format.chain_size(w, h, n).next_multiple_of(128);
        let len = data.len() as u64;
        if len < expected {
            let kind = ErrorKind::Truncated {
                needed: expected as usize,
                available: data.len(),
            };
            return Err(Refusal::Data(0, kind));
        }
        if len > expected {
            return Err(Refusal::Data(
                expected as usize,
                ErrorKind::Unsupported { value: len },
            ));
        }
        let mut levels = Vec::with_capacity(n as usize);
        let mut at = 0;
        for l in 0..n {
            let (lw, lh) = (level_dim(w, l), level_dim(h, l));
            let size = self.format.level_size(lw, lh) as usize;
            let src = &data[at..at + size];
            levels.push(if self.tiled && self.format == Format::R8Unorm {
                unswizzle(src, lw, lh)
            } else {
                src.to_vec()
            });
            at += size;
        }
        Ok(Texture {
            format: self.format,
            width: w,
            height: h,
            levels,
        })
    }
}

/// Where texel (x, y) of a power-of-two `w` x `h` image sits in RSX's
/// swizzled order: x and y bits interleaved (x first) up to the smaller
/// side, the longer side's remaining bits above.
pub fn morton(x: u32, y: u32, w: u32, h: u32) -> usize {
    let shared = w.min(h).trailing_zeros();
    let mut i = 0usize;
    for b in 0..shared {
        i |= (((x >> b) & 1) as usize) << (2 * b);
        i |= (((y >> b) & 1) as usize) << (2 * b + 1);
    }
    let rest = ((x >> shared) | (y >> shared)) as usize;
    i | (rest << (2 * shared))
}

/// One byte per texel, swizzled order to rows.
fn unswizzle(src: &[u8], w: u32, h: u32) -> Vec<u8> {
    (0..h)
        .flat_map(|y| (0..w).map(move |x| src[morton(x, y, w, h)]))
        .collect()
}

impl Texture {
    /// Reads `igz.objects()[image]`, an `igImage2`, and untiles its pixels.
    pub fn read(igz: &Igz<'_>, image: usize) -> Result<Self, FormatError> {
        let im = Image2::read(igz, image)?;
        let object = igz.objects()[image].at;
        let field = |name: &'static str, offset: u32, kind: ErrorKind| {
            let at = object.offset_by(offset).unwrap_or(object);
            igz.err_at(at, name, kind)
        };
        if im.depth != 1 {
            let kind = ErrorKind::Unsupported {
                value: im.depth.into(),
            };
            return Err(field("_depth", Image2::DEPTH, kind));
        }
        if im.image_count != 1 {
            let kind = ErrorKind::Unsupported {
                value: im.image_count.into(),
            };
            return Err(field("_imageCount", Image2::IMAGE_COUNT, kind));
        }
        // Constant on the disc and not in our output, so refused, not dropped.
        // Float compares: -0.0 passes as zero bias, NaN is refused.
        if let Some(i) = im.color_scale.iter().position(|&c| c != 1.0) {
            let kind = ErrorKind::Unsupported {
                value: im.color_scale[i].to_bits().into(),
            };
            return Err(field(
                "_colorScale",
                Image2::COLOR_SCALE + 4 * i as u32,
                kind,
            ));
        }
        if let Some(i) = im.color_bias.iter().position(|&c| c != 0.0) {
            let kind = ErrorKind::Unsupported {
                value: im.color_bias[i].to_bits().into(),
            };
            return Err(field("_colorBias", Image2::COLOR_BIAS + 4 * i as u32, kind));
        }
        if im.padding_deprecated != 0 {
            let kind = ErrorKind::Unsupported {
                value: im.padding_deprecated.into(),
            };
            return Err(field(
                "_paddingDeprecated",
                Image2::PADDING_DEPRECATED,
                kind,
            ));
        }
        let Some(meta) = im.format_name().and_then(MetaImage::by_name) else {
            let kind = ErrorKind::Unsupported {
                value: im.format.id.object.into(),
            };
            return Err(field("_format", Image2::FORMAT, kind));
        };
        let Some(handle) = im.data else {
            return Err(field("_data", Image2::DATA, ErrorKind::Missing));
        };
        let block = handle.block;
        meta.untile_inner(im.width, im.height, im.level_count, block.data)
            .map_err(|r| match r {
                Refusal::Field(name, offset, kind) => field(name, offset, kind),
                Refusal::Data(at, kind) => {
                    let mut e = igz.err_at(block.at, "_data", kind);
                    e.offset += at;
                    e
                }
            })
    }

    /// The image as a KTX2 file.
    pub fn to_ktx2(&self) -> Vec<u8> {
        ktx::write(self)
    }
}
