//! A technique's material: `tfbRuntimeTechniqueInstance._materialPass` is an
//! `igAttrList` whose `tfbAlphaTextureUnitIDAttr` entries bind an
//! `igTextureAttr2` (and through it an `igImage2`) to a texture unit.
//!
//! The unit is `igAttr._cachedUnitID`, set in files although runtime-named:
//! every one of the 2,243 character techniques binds unit 0 exactly once,
//! and the image names put colour at 0, normal maps (`_n`) at 4 and
//! specular (`_s`, `_sp`) at 6. `igTextureBindAttr2._unitID` is -1 on all.

use super::{Attr, DataList, Layout, ObjectRef, View};
use crate::FormatError;

/// `igAttrList`: an object list of attributes of any type, most of them
/// outside the gate (blend state, shader masks, tints).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AttrList<'a> {
    pub list: DataList<'a>,
    /// The attribute objects in list order; `None` for a null slot (none
    /// on the disc).
    pub attrs: Vec<Option<usize>>,
}

impl<'a> Layout<'a> for AttrList<'a> {
    const TYPE: &'static str = "igAttrList";

    fn fields(v: &mut View<'_, 'a>) -> Result<Self, FormatError> {
        let list = DataList::fields(v)?;
        let attrs = v.elements(&list, None)?;
        Ok(Self { list, attrs })
    }
}

/// `igTextureBindAttr2`: size 0x18, parent `igVisualAttribute`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TextureBindAttr2 {
    /// `cached_unit_id` is the unit (module doc).
    pub attr: Attr,
    /// An `igTextureAttr2`.
    pub texture: Option<usize>,
    pub unit_id: i32,
}

impl TextureBindAttr2 {
    pub const TEXTURE: u32 = 0x10;
    pub const UNIT_ID: u32 = 0x14;

    pub fn unit(&self) -> i16 {
        self.attr.cached_unit_id
    }
}

impl<'a> Layout<'a> for TextureBindAttr2 {
    const TYPE: &'static str = "igTextureBindAttr2";

    fn fields(v: &mut View<'_, 'a>) -> Result<Self, FormatError> {
        Ok(Self {
            attr: Attr::fields(v)?,
            texture: v.object(Self::TEXTURE, "_texture", "igTextureAttr2")?,
            unit_id: v.i32(Self::UNIT_ID, "_unitID")?,
        })
    }
}

/// `tfbAlphaTextureUnitIDAttr`: size 0x20, the only bind type on the disc.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AlphaTextureUnitIdAttr<'a> {
    pub bind: TextureBindAttr2,
    /// Null on all 5,816 in the character files, so left unfollowed.
    pub alpha_texture: ObjectRef<'a>,
    /// -1 on all of them.
    pub data: i32,
}

impl AlphaTextureUnitIdAttr<'_> {
    pub const ALPHA_TEXTURE: u32 = 0x18;
    pub const DATA: u32 = 0x1C;
}

impl<'a> Layout<'a> for AlphaTextureUnitIdAttr<'a> {
    const TYPE: &'static str = "tfbAlphaTextureUnitIDAttr";

    fn fields(v: &mut View<'_, 'a>) -> Result<Self, FormatError> {
        Ok(Self {
            bind: TextureBindAttr2::fields(v)?,
            alpha_texture: v.object_ref(Self::ALPHA_TEXTURE, "_alphaTexture")?,
            data: v.i32(Self::DATA, "_data")?,
        })
    }
}

/// `igTextureAttr2`: size 0x40, parent `igVisualAttribute`. The enums stay
/// raw; the EBOOT's name tables list `IG_GFX_TEXTURE_WRAP_CLAMP` then
/// `_REPEAT` and the filters nearest, linear, then the mipmapped ones.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TextureAttr2 {
    pub attr: Attr,
    pub format_hint: u32,
    pub load_in_vram_for_ctr: bool,
    pub mag_filter: u32,
    pub min_filter: u32,
    pub wrap_s: u32,
    pub wrap_t: u32,
    pub msaa_type: u32,
    pub source: u32,
    /// An `igImage2`; null on 1,052 of 5,683 in the character files.
    pub image: Option<usize>,
    pub tex_handle: i32,
    /// `igHandleMetaField`, 0 and unmarked in every file.
    pub image_handle: u32,
    pub wrap_r: u32,
}

impl TextureAttr2 {
    pub const FORMAT_HINT: u32 = 0x10;
    pub const LOAD_IN_VRAM_FOR_CTR: u32 = 0x14;
    pub const MAG_FILTER: u32 = 0x18;
    pub const MIN_FILTER: u32 = 0x1C;
    pub const WRAP_S: u32 = 0x20;
    pub const WRAP_T: u32 = 0x24;
    pub const MSAA_TYPE: u32 = 0x28;
    pub const SOURCE: u32 = 0x2C;
    pub const IMAGE: u32 = 0x30;
    pub const TEX_HANDLE: u32 = 0x34;
    pub const IMAGE_HANDLE: u32 = 0x38;
    pub const WRAP_R: u32 = 0x3C;
}

impl<'a> Layout<'a> for TextureAttr2 {
    const TYPE: &'static str = "igTextureAttr2";

    fn fields(v: &mut View<'_, 'a>) -> Result<Self, FormatError> {
        Ok(Self {
            attr: Attr::fields(v)?,
            format_hint: v.u32(Self::FORMAT_HINT, "_formatHint")?,
            load_in_vram_for_ctr: v.bool(Self::LOAD_IN_VRAM_FOR_CTR, "_loadInVramForCtr")?,
            mag_filter: v.u32(Self::MAG_FILTER, "_magFilter")?,
            min_filter: v.u32(Self::MIN_FILTER, "_minFilter")?,
            wrap_s: v.u32(Self::WRAP_S, "_wrapS")?,
            wrap_t: v.u32(Self::WRAP_T, "_wrapT")?,
            msaa_type: v.u32(Self::MSAA_TYPE, "_msaaType")?,
            source: v.u32(Self::SOURCE, "_source")?,
            image: v.object(Self::IMAGE, "_image", "igImage2")?,
            tex_handle: v.i32(Self::TEX_HANDLE, "_texHandle")?,
            image_handle: v.u32(Self::IMAGE_HANDLE, "_imageHandle")?,
            wrap_r: v.u32(Self::WRAP_R, "_wrapR")?,
        })
    }
}
