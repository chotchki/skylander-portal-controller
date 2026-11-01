//! The interface's bitmap fonts (openspec AT.1): a text sprite's
//! `_contextDataInfo` is a `FontDataInfo` whose `_font` is an `igBitmapFont`,
//! a glyph atlas (`_textureBind`, a texture bind down to its `igImage2`) and
//! one `igCharMetrics` per character in its `igCharMetricsList`.
//!
//! Three on the disc: `bootstrap.bld`'s `simple`, `global.bld`'s `contemp`
//! and `title.bld`'s `contemp_ns`. Every one carries an MTSZ of 0x50, 16 bytes
//! past the registry's 0x40 ([`super::FILE_PADDED`]): the extra words differ
//! file to file (0x40 on `simple`'s first, zero on the others', heap-looking
//! words after) and no file marks one, so they're read as padding.

use super::{BitField, BuildDataInfo, DataList, Layout, View};
use crate::FormatError;

/// `igFont`'s fields (registry size 0x40, parent `igNamedObject`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Font<'a> {
    pub name: Option<&'a str>,
    /// The line height in pixels at the font's own size.
    pub height: f32,
    pub base_line: f32,
    /// An `igAttrList` (a blend state on all three).
    pub attr_list: Option<usize>,
    pub color: [f32; 4],
    pub char_padding: f32,
}

impl<'a> Font<'a> {
    pub const NAME: u32 = 0x08;
    pub const HEIGHT: u32 = 0x0C;
    pub const BASE_LINE: u32 = 0x10;
    pub const ATTR_LIST: u32 = 0x14;
    pub const COLOR: u32 = 0x20;
    pub const CHAR_PADDING: u32 = 0x30;

    pub(super) fn fields(v: &mut View<'_, 'a>) -> Result<Self, FormatError> {
        Ok(Self {
            name: v.string(Self::NAME, "_name")?,
            height: v.f32(Self::HEIGHT, "_height")?,
            base_line: v.f32(Self::BASE_LINE, "_baseLine")?,
            attr_list: v.object(Self::ATTR_LIST, "_attrList", "igAttrList")?,
            color: v.floats(Self::COLOR, "_color")?,
            char_padding: v.f32(Self::CHAR_PADDING, "_charPadding")?,
        })
    }
}

/// `igBitmapFont`: registry size 0x40, files 0x50.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BitmapFont<'a> {
    pub font: Font<'a>,
    /// Pixels added between characters.
    pub kern_amount: f32,
    /// The atlas: an `igTextureBindAttr2` (a `tfbAlphaTextureUnitIDAttr` on
    /// all three).
    pub texture_bind: Option<usize>,
    /// An `igCharMetricsList`.
    pub char_metrics_list: Option<usize>,
}

impl BitmapFont<'_> {
    pub const KERN_AMOUNT: u32 = 0x34;
    pub const TEXTURE_BIND: u32 = 0x38;
    pub const CHAR_METRICS_LIST: u32 = 0x3C;
}

impl<'a> Layout<'a> for BitmapFont<'a> {
    const TYPE: &'static str = "igBitmapFont";

    fn fields(v: &mut View<'_, 'a>) -> Result<Self, FormatError> {
        Ok(Self {
            font: Font::fields(v)?,
            kern_amount: v.f32(Self::KERN_AMOUNT, "_kernAmount")?,
            texture_bind: v.object(Self::TEXTURE_BIND, "_textureBind", "igTextureBindAttr2")?,
            char_metrics_list: v.object(
                Self::CHAR_METRICS_LIST,
                "_charMetricsList",
                CharMetricsList::TYPE,
            )?,
        })
    }
}

/// `igCharMetrics._properties`' four kern corners (AT.1, read off the
/// class's field registration function at 0x20F9D8): four bits each, from
/// the low end of the u16, top-left first.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct KernCorners {
    pub top_left: u8,
    pub top_right: u8,
    pub bottom_left: u8,
    pub bottom_right: u8,
}

impl KernCorners {
    pub const TOP_LEFT: BitField = BitField::new(0, 4);
    pub const TOP_RIGHT: BitField = BitField::new(4, 4);
    pub const BOTTOM_LEFT: BitField = BitField::new(8, 4);
    pub const BOTTOM_RIGHT: BitField = BitField::new(12, 4);

    pub fn from_properties(word: u16) -> Self {
        let word = u32::from(word);
        // Each field is 4 bits wide, so the casts are lossless.
        Self {
            top_left: Self::TOP_LEFT.get(word) as u8,
            top_right: Self::TOP_RIGHT.get(word) as u8,
            bottom_left: Self::BOTTOM_LEFT.get(word) as u8,
            bottom_right: Self::BOTTOM_RIGHT.get(word) as u8,
        }
    }
}

/// `igCharMetrics`: size 0x30, one glyph.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CharMetrics {
    /// The character, a UTF-16 unit.
    pub char_number: u16,
    /// Storage for the kern corners ([`CharMetrics::kern`]).
    pub properties: u16,
    /// The advance in pixels at the font's own size.
    pub step: f32,
    pub offset: [f32; 2],
    pub size: [f32; 2],
    /// The glyph's box in the atlas, 0-1.
    pub tex_coord_min: [f32; 2],
    pub tex_coord_max: [f32; 2],
}

impl CharMetrics {
    pub const CHAR_NUMBER: u32 = 0x08;
    pub const PROPERTIES: u32 = 0x0A;
    pub const STEP: u32 = 0x0C;
    pub const OFFSET: u32 = 0x10;
    pub const SIZE: u32 = 0x18;
    pub const TEX_COORD_MIN: u32 = 0x20;
    pub const TEX_COORD_MAX: u32 = 0x28;

    pub fn kern(&self) -> KernCorners {
        KernCorners::from_properties(self.properties)
    }
}

impl<'a> Layout<'a> for CharMetrics {
    const TYPE: &'static str = "igCharMetrics";

    fn fields(v: &mut View<'_, 'a>) -> Result<Self, FormatError> {
        Ok(Self {
            char_number: v.u16(Self::CHAR_NUMBER, "_charNumber")?,
            properties: v.u16(Self::PROPERTIES, "_properties")?,
            step: v.f32(Self::STEP, "_step")?,
            offset: v.floats(Self::OFFSET, "_offset")?,
            size: v.floats(Self::SIZE, "_size")?,
            tex_coord_min: v.floats(Self::TEX_COORD_MIN, "_texCoordMin")?,
            tex_coord_max: v.floats(Self::TEX_COORD_MAX, "_texCoordMax")?,
        })
    }
}

/// `igCharMetricsList`: an object list of non-null `igCharMetrics`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CharMetricsList<'a> {
    pub list: DataList<'a>,
    pub glyphs: Vec<usize>,
}

impl<'a> Layout<'a> for CharMetricsList<'a> {
    const TYPE: &'static str = "igCharMetricsList";

    fn fields(v: &mut View<'_, 'a>) -> Result<Self, FormatError> {
        let list = DataList::fields(v)?;
        let glyphs = v
            .elements(&list, Some(CharMetrics::TYPE))?
            .into_iter()
            .flatten()
            .collect();
        Ok(Self { list, glyphs })
    }
}

/// `FontDataInfo`: size 0x24, a text sprite's context. The build data's name
/// is the atlas image's source path (`fonts/simple.png`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FontDataInfo<'a> {
    pub data_info: BuildDataInfo<'a>,
    /// An `igFont` (an `igBitmapFont` on all three).
    pub font: Option<usize>,
}

impl FontDataInfo<'_> {
    pub const FONT: u32 = 0x20;
}

impl<'a> Layout<'a> for FontDataInfo<'a> {
    const TYPE: &'static str = "FontDataInfo";

    fn fields(v: &mut View<'_, 'a>) -> Result<Self, FormatError> {
        Ok(Self {
            data_info: BuildDataInfo::fields(v)?,
            font: v.object(Self::FONT, "_font", "igFont")?,
        })
    }
}
