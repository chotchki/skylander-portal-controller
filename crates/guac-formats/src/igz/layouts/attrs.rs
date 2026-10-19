//! A tfbmulti2 material's attributes besides its texture binds (openspec
//! AE.6): the two shader-variant masks, the `tfbMulti2*` constants, the tint,
//! the render states, the glow amount, and the material animation that
//! scrolls a layer's texture coordinates.
//!
//! Every type sits under `igVisualAttribute` (`igAttr`'s cached unit, attr
//! index and read-only flag, [`Attr`]) except the animation's, which sit
//! under `igObject`. The four render-state types carry an MTSZ 4 over the
//! registry in every file ([`super::FILE_PADDED`], AC.14: the loader reads
//! sections in place, so the 4 bytes sit past the layout and are never read).

use super::{Attr, DataList, Layout, ObjectRef, View};
use crate::FormatError;

/// `igPixelShaderVariantMaskAttr` and `igVertexShaderVariantMaskAttr`, size
/// 0x18: the variant bits the material sets and the bits it owns (those the
/// runtime may not set).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VariantMask {
    pub attr: Attr,
    pub base_mask: u32,
    pub ignore_mask: u32,
}

impl VariantMask {
    pub const BASE_MASK: u32 = 0x10;
    pub const IGNORE_MASK: u32 = 0x14;

    fn fields(v: &mut View<'_, '_>) -> Result<Self, FormatError> {
        Ok(Self {
            attr: Attr::fields(v)?,
            base_mask: v.u32(Self::BASE_MASK, "_baseMask")?,
            ignore_mask: v.u32(Self::IGNORE_MASK, "_ignoreMask")?,
        })
    }
}

/// The pixel variant's bits: textures, lighting, the layers, plus the
/// runtime's shadows, fog and point lights.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PixelShaderVariantMaskAttr(pub VariantMask);

impl<'a> Layout<'a> for PixelShaderVariantMaskAttr {
    const TYPE: &'static str = "igPixelShaderVariantMaskAttr";

    fn fields(v: &mut View<'_, 'a>) -> Result<Self, FormatError> {
        VariantMask::fields(v).map(Self)
    }
}

/// The vertex variant's bits: the pixel side's 0-10 (outputs only), bit 11
/// skinning (never compiled), 12 vertex colour, 13 point lights.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VertexShaderVariantMaskAttr(pub VariantMask);

impl<'a> Layout<'a> for VertexShaderVariantMaskAttr {
    const TYPE: &'static str = "igVertexShaderVariantMaskAttr";

    fn fields(v: &mut View<'_, 'a>) -> Result<Self, FormatError> {
        VariantMask::fields(v).map(Self)
    }
}

/// `tfbShaderConstantVec4fAttr`, size 0x20: one vec4 the shader reads. Every
/// `tfbMulti2*Attr` (fixed alpha, the layers' extra values and blend
/// factors, layer 4's values) is one, told apart by its type.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ShaderConstantVec4fAttr {
    pub attr: Attr,
    pub data: [f32; 4],
}

impl ShaderConstantVec4fAttr {
    pub const DATA: u32 = 0x10;
}

impl<'a> Layout<'a> for ShaderConstantVec4fAttr {
    const TYPE: &'static str = "tfbShaderConstantVec4fAttr";

    fn fields(v: &mut View<'_, 'a>) -> Result<Self, FormatError> {
        Ok(Self {
            attr: Attr::fields(v)?,
            data: v.floats(Self::DATA, "_data")?,
        })
    }
}

/// `tfbShaderConstantIntAttr`, size 0x14: one packed int, `tfbMaterialTintAttr`'s
/// colour (red in the low byte, ae-shader).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ShaderConstantIntAttr {
    pub attr: Attr,
    pub data: i32,
}

impl ShaderConstantIntAttr {
    pub const DATA: u32 = 0x10;
}

impl<'a> Layout<'a> for ShaderConstantIntAttr {
    const TYPE: &'static str = "tfbShaderConstantIntAttr";

    fn fields(v: &mut View<'_, 'a>) -> Result<Self, FormatError> {
        Ok(Self {
            attr: Attr::fields(v)?,
            data: v.i32(Self::DATA, "_data")?,
        })
    }
}

/// The byte every switch-like state attr keeps its `_enabled` in.
const ENABLED: u32 = 0x0D;

/// `igBlendStateAttr`, size 0x10 (0x14 in files).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BlendStateAttr {
    pub attr: Attr,
    pub enabled: bool,
    pub alpha_to_coverage_enabled: bool,
}

impl BlendStateAttr {
    pub const ENABLED: u32 = ENABLED;
    pub const ALPHA_TO_COVERAGE_ENABLED: u32 = 0x0E;
}

impl<'a> Layout<'a> for BlendStateAttr {
    const TYPE: &'static str = "igBlendStateAttr";

    fn fields(v: &mut View<'_, 'a>) -> Result<Self, FormatError> {
        Ok(Self {
            attr: Attr::fields(v)?,
            enabled: v.bool(Self::ENABLED, "_enabled")?,
            alpha_to_coverage_enabled: v
                .bool(Self::ALPHA_TO_COVERAGE_ENABLED, "_alphaToCoverageEnabled")?,
        })
    }
}

/// `igDepthWriteStateAttr`, size 0x10 (0x14 in files).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DepthWriteStateAttr {
    pub attr: Attr,
    pub enabled: bool,
}

impl<'a> Layout<'a> for DepthWriteStateAttr {
    const TYPE: &'static str = "igDepthWriteStateAttr";

    fn fields(v: &mut View<'_, 'a>) -> Result<Self, FormatError> {
        Ok(Self {
            attr: Attr::fields(v)?,
            enabled: v.bool(ENABLED, "_enabled")?,
        })
    }
}

/// `igAlphaStateAttr`, size 0x10 (0x14 in files): the alpha test's switch.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AlphaStateAttr {
    pub attr: Attr,
    pub enabled: bool,
}

impl<'a> Layout<'a> for AlphaStateAttr {
    const TYPE: &'static str = "igAlphaStateAttr";

    fn fields(v: &mut View<'_, 'a>) -> Result<Self, FormatError> {
        Ok(Self {
            attr: Attr::fields(v)?,
            enabled: v.bool(ENABLED, "_enabled")?,
        })
    }
}

/// `igCullFaceAttr`, size 0x14 (0x18 in files). `_mode` is an enum, kept raw.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CullFaceAttr {
    pub attr: Attr,
    pub enabled: bool,
    pub mode: u32,
}

impl CullFaceAttr {
    pub const MODE: u32 = 0x10;
}

impl<'a> Layout<'a> for CullFaceAttr {
    const TYPE: &'static str = "igCullFaceAttr";

    fn fields(v: &mut View<'_, 'a>) -> Result<Self, FormatError> {
        Ok(Self {
            attr: Attr::fields(v)?,
            enabled: v.bool(ENABLED, "_enabled")?,
            mode: v.u32(Self::MODE, "_mode")?,
        })
    }
}

/// `igBlendFunctionAttr`, size 0x34. The enums stay raw: IG_GFX's blend
/// factors run ZERO, ONE, SRC_COLOR, ONE_MINUS_SRC_COLOR, SRC_ALPHA, ...
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BlendFunctionAttr {
    pub attr: Attr,
    pub src: u32,
    pub dst: u32,
    pub eq: u32,
    pub unused: u32,
    pub blend_constant: u8,
    pub blend_stage: i16,
    pub blend_abcd: [u32; 4],
}

impl BlendFunctionAttr {
    pub const SRC: u32 = 0x10;
    pub const DST: u32 = 0x14;
    pub const EQ: u32 = 0x18;
    pub const UNUSED: u32 = 0x1C;
    pub const BLEND_CONSTANT: u32 = 0x20;
    pub const BLEND_STAGE: u32 = 0x22;
    pub const BLEND_A: u32 = 0x24;
}

impl<'a> Layout<'a> for BlendFunctionAttr {
    const TYPE: &'static str = "igBlendFunctionAttr";

    fn fields(v: &mut View<'_, 'a>) -> Result<Self, FormatError> {
        let attr = Attr::fields(v)?;
        let mut blend_abcd = [0; 4];
        for (i, x) in blend_abcd.iter_mut().enumerate() {
            *x = v.u32(Self::BLEND_A + 4 * i as u32, "_blendABCD")?;
        }
        Ok(Self {
            attr,
            src: v.u32(Self::SRC, "_src")?,
            dst: v.u32(Self::DST, "_dst")?,
            eq: v.u32(Self::EQ, "_eq")?,
            unused: v.u32(Self::UNUSED, "_unused")?,
            blend_constant: v.u8(Self::BLEND_CONSTANT, "_blendConstant")?,
            blend_stage: v.i16(Self::BLEND_STAGE, "_blendStage")?,
            blend_abcd,
        })
    }
}

/// `igAlphaFunctionAttr`, size 0x18: the alpha test, `_func` raw (4 is
/// GREATER in IG_GFX's order).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AlphaFunctionAttr {
    pub attr: Attr,
    pub func: u32,
    pub ref_value: f32,
}

impl AlphaFunctionAttr {
    pub const FUNC: u32 = 0x10;
    pub const REF_VALUE: u32 = 0x14;
}

impl<'a> Layout<'a> for AlphaFunctionAttr {
    const TYPE: &'static str = "igAlphaFunctionAttr";

    fn fields(v: &mut View<'_, 'a>) -> Result<Self, FormatError> {
        Ok(Self {
            attr: Attr::fields(v)?,
            func: v.u32(Self::FUNC, "_func")?,
            ref_value: v.f32(Self::REF_VALUE, "_refValue")?,
        })
    }
}

/// `tfbDestinationAlphaAttr`, size 0x18: the glow amount, `_alpha` of 255,
/// while `_state` is on (ae-materials).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DestinationAlphaAttr {
    pub attr: Attr,
    pub alpha: i32,
    pub state: bool,
}

impl DestinationAlphaAttr {
    pub const ALPHA: u32 = 0x10;
    pub const STATE: u32 = 0x14;
}

impl<'a> Layout<'a> for DestinationAlphaAttr {
    const TYPE: &'static str = "tfbDestinationAlphaAttr";

    fn fields(v: &mut View<'_, 'a>) -> Result<Self, FormatError> {
        Ok(Self {
            attr: Attr::fields(v)?,
            alpha: v.i32(Self::ALPHA, "_alpha")?,
            state: v.bool(Self::STATE, "_state")?,
        })
    }
}

/// `tfbMaterialAnim`, size 0x10: a technique's `_materialAnim`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MaterialAnim<'a> {
    /// A `tfbMaterialAnimTrackList`.
    pub track_list: Option<usize>,
    /// The attributes the tracks drive, unfollowed.
    pub attr_list: ObjectRef<'a>,
}

impl MaterialAnim<'_> {
    pub const TRACK_LIST: u32 = 0x08;
    pub const ATTR_LIST: u32 = 0x0C;
}

impl<'a> Layout<'a> for MaterialAnim<'a> {
    const TYPE: &'static str = "tfbMaterialAnim";

    fn fields(v: &mut View<'_, 'a>) -> Result<Self, FormatError> {
        Ok(Self {
            track_list: v.object(Self::TRACK_LIST, "_trackList", "tfbMaterialAnimTrackList")?,
            attr_list: v.object_ref(Self::ATTR_LIST, "_attrList")?,
        })
    }
}

/// `tfbMaterialAnimTrackList`: an object list of tracks, every one a
/// `tfbMaterialAnimTrack`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MaterialAnimTrackList<'a> {
    pub list: DataList<'a>,
    pub tracks: Vec<usize>,
}

impl<'a> Layout<'a> for MaterialAnimTrackList<'a> {
    const TYPE: &'static str = "tfbMaterialAnimTrackList";

    fn fields(v: &mut View<'_, 'a>) -> Result<Self, FormatError> {
        let list = DataList::fields(v)?;
        let tracks = v
            .elements(&list, Some("tfbMaterialAnimTrack"))?
            .into_iter()
            .flatten()
            .collect();
        Ok(Self { list, tracks })
    }
}

/// `tfbMaterialUVAnimTrack`, size 0x28 (through `tfbMaterialAnimTrack`):
/// what scrolls, transforms or flips one layer's texture coordinates. The
/// kind is the type (`tfbScrollUVAnimTrack` and its two siblings); the
/// cache fields are the runtime's.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MaterialUvAnimTrack<'a> {
    pub base_time: f32,
    pub cur_time: f32,
    /// A `tfbMaterialAnimData`: the kind's parameters.
    pub data: Option<usize>,
    pub cache_frame: i32,
    pub cache_time: f32,
    pub cache_inst: ObjectRef<'a>,
    pub attr_list: ObjectRef<'a>,
    pub layer: i32,
}

impl MaterialUvAnimTrack<'_> {
    pub const BASE_TIME: u32 = 0x08;
    pub const CUR_TIME: u32 = 0x0C;
    pub const DATA: u32 = 0x10;
    pub const CACHE_FRAME: u32 = 0x14;
    pub const CACHE_TIME: u32 = 0x18;
    pub const CACHE_INST: u32 = 0x1C;
    pub const ATTR_LIST: u32 = 0x20;
    pub const LAYER: u32 = 0x24;
}

impl<'a> Layout<'a> for MaterialUvAnimTrack<'a> {
    const TYPE: &'static str = "tfbMaterialUVAnimTrack";

    fn fields(v: &mut View<'_, 'a>) -> Result<Self, FormatError> {
        Ok(Self {
            base_time: v.f32(Self::BASE_TIME, "_baseTime")?,
            cur_time: v.f32(Self::CUR_TIME, "_curTime")?,
            data: v.object(Self::DATA, "_data", "tfbMaterialAnimData")?,
            cache_frame: v.i32(Self::CACHE_FRAME, "_cacheFrame")?,
            cache_time: v.f32(Self::CACHE_TIME, "_cacheTime")?,
            cache_inst: v.object_ref(Self::CACHE_INST, "_cacheInst")?,
            attr_list: v.object_ref(Self::ATTR_LIST, "_attrList")?,
            layer: v.i32(Self::LAYER, "_layer")?,
        })
    }
}

/// `tfbScrollUVAnimData`, size 0x34: a scroll track's parameters, as
/// stored. How the runtime turns them into a texture-coordinate transform
/// isn't read here.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ScrollUvAnimData {
    pub scale: [f32; 2],
    pub velocity: [f32; 2],
    pub amplitude: [f32; 2],
    pub frequency: [f32; 2],
    pub phase: [f32; 2],
    pub rotation_phase: f32,
}

impl ScrollUvAnimData {
    pub const SCALE_U: u32 = 0x08;
    pub const VELOCITY_U: u32 = 0x10;
    pub const AMPLITUDE_U: u32 = 0x18;
    pub const FREQUENCY_U: u32 = 0x20;
    pub const PHASE_U: u32 = 0x28;
    pub const ROTATION_PHASE: u32 = 0x30;
}

impl<'a> Layout<'a> for ScrollUvAnimData {
    const TYPE: &'static str = "tfbScrollUVAnimData";

    fn fields(v: &mut View<'_, 'a>) -> Result<Self, FormatError> {
        Ok(Self {
            scale: v.floats(Self::SCALE_U, "_scaleUV")?,
            velocity: v.floats(Self::VELOCITY_U, "_velocityUV")?,
            amplitude: v.floats(Self::AMPLITUDE_U, "_amplitudeUV")?,
            frequency: v.floats(Self::FREQUENCY_U, "_frequencyUV")?,
            phase: v.floats(Self::PHASE_U, "_phaseUV")?,
            rotation_phase: v.f32(Self::ROTATION_PHASE, "_rotationPhase")?,
        })
    }
}
