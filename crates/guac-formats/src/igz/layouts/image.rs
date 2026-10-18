//! `igImage2`, the texture object. Its pixels are a TMHN block; `_format`
//! imports an `igMetaImage` by name hash from [`METAIMAGES`].

use super::{Handle, Import, Layout, View};
use crate::{FormatError, fnv1a32};

/// The namespace `_format` imports from.
pub const METAIMAGES: &str = "metaimages";

/// Every metaimage the 158 character files use.
pub const KNOWN_FORMATS: [&str; 5] = [
    "dxt1_big_ps3",
    "dxt1_tile_big_ps3",
    "dxt5_big_ps3",
    "dxt5_tile_big_ps3",
    "l8_tile_big_ps3",
];

/// `igImage2`: size 0x60, parent `igNamedObject`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Image2<'a> {
    pub name: Option<&'a str>,
    pub color_scale: [f32; 4],
    pub color_bias: [f32; 4],
    pub width: u16,
    pub height: u16,
    pub depth: u16,
    pub level_count: u16,
    pub image_count: u16,
    /// An `igMetaImage` import.
    pub format: Import,
    pub usage_deprecated: i32,
    pub padding_deprecated: u16,
    /// The mip chain, rounded up to 128 bytes.
    pub data: Option<Handle<'a>>,
    pub lock_count: i32,
    pub tex_handle: i32,
    /// `igRawRefMetaField`, 0 in files.
    pub locked_memory: u32,
    pub ogl_discard_original_image: bool,
}

impl Image2<'_> {
    pub const NAME: u32 = 0x08;
    pub const COLOR_SCALE: u32 = 0x10;
    pub const COLOR_BIAS: u32 = 0x20;
    pub const WIDTH: u32 = 0x30;
    pub const HEIGHT: u32 = 0x32;
    pub const DEPTH: u32 = 0x34;
    pub const LEVEL_COUNT: u32 = 0x36;
    pub const IMAGE_COUNT: u32 = 0x38;
    pub const FORMAT: u32 = 0x3C;
    pub const USAGE_DEPRECATED: u32 = 0x40;
    pub const PADDING_DEPRECATED: u32 = 0x44;
    pub const DATA: u32 = 0x48;
    pub const LOCK_COUNT: u32 = 0x4C;
    pub const TEX_HANDLE: u32 = 0x50;
    pub const LOCKED_MEMORY: u32 = 0x54;
    pub const OGL_DISCARD_ORIGINAL_IMAGE: u32 = 0x58;

    /// The format's name, when it's a [`KNOWN_FORMATS`] metaimage.
    pub fn format_name(&self) -> Option<&'static str> {
        if self.format.id.namespace != fnv1a32(METAIMAGES.as_bytes()) {
            return None;
        }
        KNOWN_FORMATS
            .into_iter()
            .find(|f| fnv1a32(f.as_bytes()) == self.format.id.object)
    }
}

impl<'a> Layout<'a> for Image2<'a> {
    const TYPE: &'static str = "igImage2";

    fn fields(v: &mut View<'_, 'a>) -> Result<Self, FormatError> {
        Ok(Self {
            name: v.string(Self::NAME, "_name")?,
            color_scale: v.floats(Self::COLOR_SCALE, "_colorScale")?,
            color_bias: v.floats(Self::COLOR_BIAS, "_colorBias")?,
            width: v.u16(Self::WIDTH, "_width")?,
            height: v.u16(Self::HEIGHT, "_height")?,
            depth: v.u16(Self::DEPTH, "_depth")?,
            level_count: v.u16(Self::LEVEL_COUNT, "_levelCount")?,
            image_count: v.u16(Self::IMAGE_COUNT, "_imageCount")?,
            format: v.import(Self::FORMAT, "_format")?,
            usage_deprecated: v.i32(Self::USAGE_DEPRECATED, "_usageDeprecated")?,
            padding_deprecated: v.u16(Self::PADDING_DEPRECATED, "_paddingDeprecated")?,
            data: v.handle(Self::DATA, "_data")?,
            lock_count: v.i32(Self::LOCK_COUNT, "_lockCount")?,
            tex_handle: v.i32(Self::TEX_HANDLE, "_texHandle")?,
            locked_memory: v.u32(Self::LOCKED_MEMORY, "_lockedMemory")?,
            ogl_discard_original_image: v
                .bool(Self::OGL_DISCARD_ORIGINAL_IMAGE, "_oglDiscardOriginalImage")?,
        })
    }
}
