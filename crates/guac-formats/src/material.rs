//! tfbmulti2's materials (openspec AE.6, design decision 9): what a hero
//! technique's `_materialPass` asks of the one effect that draws every
//! character, read into the terms the character-shading spec uses.
//!
//! - The variant: the material's pixel and vertex masks, base and ignore
//!   (ae-materials). The runtime owns the bits outside the ignore mask
//!   (shadows, fog, point lights); the base's 14 feature bits are named as
//!   `global.bld`'s variant attrs name them ([`PIXEL_BITS`], [`VERTEX_BITS`]).
//! - The textures: a bind's role is its place in the list, read against the
//!   pixel variant's samplers, which tfbmulti2 lists alphabetically
//!   ([`samplers`], checked against all 4,096 compiled pixel variants). Each
//!   bind's cached unit must be its role's unit, and the `TFN1` hint must sit
//!   on the normal maps and nowhere else (ae-materials: 329 of 329).
//! - The parameters, with the original's defaults where an attr is absent
//!   ([`Params::DEFAULT`]).
//! - The render states and the scroll tracks that move a layer's UVs.
//!
//! Bits 6, 7 and 9 (rim by position, gloss map, reflections) are never set
//! on a character and vertex bit 11 (skinning) is never compiled (ae-shader
//! section 4), so a material carrying one is refused with
//! [`ErrorKind::Feature`] rather than drawn without it.

use crate::fnv1a32;
use crate::igz::Igz;
use crate::igz::layouts::{
    AlphaFunctionAttr, AlphaStateAttr, BlendFunctionAttr, BlendStateAttr, CullFaceAttr,
    DepthWriteStateAttr, DestinationAlphaAttr, Hero, Image2, Layout, MaterialAnim,
    MaterialAnimTrackList, MaterialUvAnimTrack, Mesh, ObjectRef, PixelShaderVariantMaskAttr,
    ScrollUvAnimData, ShaderConstantIntAttr, ShaderConstantVec4fAttr, TextureAttr2, VariantMask,
    VertexShaderVariantMaskAttr,
};
use crate::{ErrorKind, FormatError};

#[cfg(test)]
mod tests;

/// The archive whose exports `_effect` imports.
pub const EFFECTS: &str = "global.bld";
/// `_effect`'s export ids in [`EFFECTS`] (AE.1): `tfbmulti2.fx`, which
/// draws every hero, and `tfbwarp.fx`, which draws none (AE.5).
pub const TFBMULTI2: u32 = 0xAA7;
pub const TFBWARP: u32 = 0xAA9;
/// `igTextureAttr2._formatHint` on every normal map and on nothing else.
pub const TFN1: u32 = u32::from_be_bytes(*b"TFN1");

/// The pixel variant's bits, as `global.bld` names them.
pub const PIXEL_BITS: [&str; 14] = [
    "layer2Active",
    "layer3Active",
    "layer4Active",
    "useLayer1BaseNormalMap",
    "useLayer1BaseSpecMap",
    "isDirectionalLit",
    "layer4UsesPositions",
    "layer4UsesGlossmap",
    "receivesShadows",
    "receivesReflections",
    "ig_fog_enable",
    "isPointLit",
    "isPointLit2",
    "isPointLit3",
];

/// The vertex variant's bits: the pixel side's 0-10, then its own three.
pub const VERTEX_BITS: [&str; 14] = [
    "layer2Active",
    "layer3Active",
    "layer4Active",
    "useLayer1BaseNormalMap",
    "useLayer1BaseSpecMap",
    "isDirectionalLit",
    "layer4UsesPositions",
    "layer4UsesGlossmap",
    "receivesShadows",
    "receivesReflections",
    "ig_fog_enable",
    "vertexSkinning",
    "hasVertexColor",
    "isPointLit",
];

/// Pixel bits no character sets, which the shader port leaves out.
pub const PIXEL_UNDRAWN: [u8; 3] = [6, 7, 9];
/// Vertex bits no character sets, plus skinning, which no variant compiles.
pub const VERTEX_UNDRAWN: [u8; 4] = [6, 7, 9, 11];

fn has(mask: u32, bit: u8) -> bool {
    mask & (1 << bit) != 0
}

/// ae-shader's existence rules for the 4,096 pixel variants: bits 6 and 7
/// need 2, 12 needs 11, 13 needs 12, and 1 never comes with 7.
pub fn pixel_variant_exists(mask: u32) -> bool {
    (!has(mask, 6) || has(mask, 2))
        && (!has(mask, 7) || has(mask, 2))
        && (!has(mask, 12) || has(mask, 11))
        && (!has(mask, 13) || has(mask, 12))
        && !(has(mask, 1) && has(mask, 7))
        && mask < 1 << 14
}

/// The 5,120 vertex variants: bits 6 and 7 need 2, and 11 is never set.
pub fn vertex_variant_exists(mask: u32) -> bool {
    (!has(mask, 6) || has(mask, 2))
        && (!has(mask, 7) || has(mask, 2))
        && !has(mask, 11)
        && mask < 1 << 14
}

/// A texture's role: the pixel variant's sampler it binds to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Role {
    Layer1BaseNormalMap,
    Layer1BaseSpecMap,
    Layer1ColorMap,
    Layer2BaseNormalMap,
    Layer2BaseSpecMap,
    Layer2ColorMap,
    Layer3ColorMap,
    Layer4ColorMap,
}

impl Role {
    /// Every sampler, in the order tfbmulti2 lists them: alphabetical.
    pub const ALL: [Role; 8] = [
        Role::Layer1BaseNormalMap,
        Role::Layer1BaseSpecMap,
        Role::Layer1ColorMap,
        Role::Layer2BaseNormalMap,
        Role::Layer2BaseSpecMap,
        Role::Layer2ColorMap,
        Role::Layer3ColorMap,
        Role::Layer4ColorMap,
    ];

    /// The sampler's name in tfbmulti2.
    pub const fn name(self) -> &'static str {
        match self {
            Self::Layer1BaseNormalMap => "layer1BaseNormalMap",
            Self::Layer1BaseSpecMap => "layer1BaseSpecMap",
            Self::Layer1ColorMap => "layer1ColorMap",
            Self::Layer2BaseNormalMap => "layer2BaseNormalMap",
            Self::Layer2BaseSpecMap => "layer2BaseSpecMap",
            Self::Layer2ColorMap => "layer2ColorMap",
            Self::Layer3ColorMap => "layer3ColorMap",
            Self::Layer4ColorMap => "layer4ColorMap",
        }
    }

    /// Its texture unit, which a bind's cached unit must equal: the colour
    /// maps take 0-3, the normal and spec maps the even units from 4.
    pub const fn unit(self) -> i16 {
        match self {
            Self::Layer1ColorMap => 0,
            Self::Layer2ColorMap => 1,
            Self::Layer3ColorMap => 2,
            Self::Layer4ColorMap => 3,
            Self::Layer1BaseNormalMap => 4,
            Self::Layer1BaseSpecMap => 6,
            Self::Layer2BaseNormalMap => 8,
            Self::Layer2BaseSpecMap => 10,
        }
    }

    pub const fn is_normal_map(self) -> bool {
        matches!(self, Self::Layer1BaseNormalMap | Self::Layer2BaseNormalMap)
    }

    /// Whether the pixel variant `mask` samples it: layer 2's maps with bit
    /// 0 (its normal and spec maps with 3 and 4 too), layer 3's with 1 or the
    /// gloss map's 7, layer 4's with 2, layer 1's normal and spec maps with 3
    /// and 4, its colour map always.
    pub fn sampled(self, mask: u32) -> bool {
        let b = |bit: u8| mask & (1 << bit) != 0;
        match self {
            Self::Layer1ColorMap => true,
            Self::Layer2ColorMap => b(0),
            Self::Layer3ColorMap => b(1) || b(7),
            Self::Layer4ColorMap => b(2),
            Self::Layer1BaseNormalMap => b(3),
            Self::Layer1BaseSpecMap => b(4),
            Self::Layer2BaseNormalMap => b(0) && b(3),
            Self::Layer2BaseSpecMap => b(0) && b(4),
        }
    }
}

/// The samplers the pixel variant `mask` binds, in list order.
pub fn samplers(mask: u32) -> Vec<Role> {
    Role::ALL.into_iter().filter(|r| r.sampled(mask)).collect()
}

/// One texture the material binds, in its role.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Bound<'a> {
    pub role: Role,
    /// The `tfbAlphaTextureUnitIDAttr`.
    pub bind: usize,
    /// Its `igTextureAttr2`: filters, wraps and the format hint.
    pub texture: usize,
    pub sampler: TextureAttr2,
    /// The `igImage2`.
    pub image: usize,
    /// The image's file name without its folders (`eruptor_n`).
    pub image_name: Option<&'a str>,
}

/// The shader constants a material sets, or the original's value where it
/// sets none.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Params {
    /// x layer 1's fixed alpha (from -0.001 it replaces the colour map's), y
    /// layer 2's, z layer 3's (or the gloss map's) strength, w layer 4's.
    pub fixed_alpha: [f32; 4],
    /// z the spec exponent, w the normal map's detail weight; x and y unread.
    pub layer1_extra: [f32; 4],
    /// Layer 2's own exponent and weight, when it sets them.
    pub layer2_extra: Option<[f32; 4]>,
    /// Over, add and multiply weights for blending layer 2, then layer 3.
    pub layer2_blend: Option<[f32; 4]>,
    pub layer3_blend: Option<[f32; 4]>,
    /// Over, add and multiply weights for layer 4 (the rim).
    pub layer4_blend: [f32; 4],
    /// y the rim's UV scale, w its colour/alpha split; x and z unread.
    pub layer4_values: [f32; 4],
    /// RGBA, unpacked from the stored int (red in the low byte).
    pub material_tint: [u8; 4],
    /// `tfbDestinationAlphaAttr._alpha` of 255 while its state is on; `None`
    /// leaves the glow to the colour map's alpha.
    pub glow: Option<u8>,
}

impl Params {
    /// What tfbmulti2 runs with when a material sets nothing (ae-shader
    /// section 3, measured on the tutorial's patched copies): layer-1 extra
    /// (1, 1, 1, 0.25), layer-4 blend (0, 1, 0, 0) (add), layer-4 values (0,
    /// 1, 0, 0), the EBOOT's material tint reset 0xFF808080. Fixed alpha's
    /// (-1, -1, -1, -1) is the tutorial's value on the variants without layer
    /// 4; no hero with layer 4 goes without the attr (AE.6's disc run).
    pub const DEFAULT: Params = Params {
        fixed_alpha: [-1.0; 4],
        layer1_extra: [1.0, 1.0, 1.0, 0.25],
        layer2_extra: None,
        layer2_blend: None,
        layer3_blend: None,
        layer4_blend: [0.0, 1.0, 0.0, 0.0],
        layer4_values: [0.0, 1.0, 0.0, 0.0],
        material_tint: [128, 128, 128, 255],
        glow: None,
    };
}

/// RGBA from a packed tint: red in the low byte (ae-shader: 12 of 12
/// tutorial tints match that order, 0 the other).
pub fn unpack_tint(packed: u32) -> [u8; 4] {
    let [r, g, b, a] = packed.to_le_bytes();
    [r, g, b, a]
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Blend {
    Opaque,
    /// Blended by source alpha.
    Alpha,
    /// Source alpha plus what's there (`igBlendFunctionAttr` SRC_ALPHA, ONE).
    Additive,
}

/// The pipeline state a material sets; absent attrs leave blending off,
/// depth writes on, back faces culled and no alpha test.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct States {
    pub blend: Blend,
    pub depth_write: bool,
    pub two_sided: bool,
    /// The alpha test's reference: alpha passes above it.
    pub alpha_test: Option<f32>,
}

impl Default for States {
    fn default() -> Self {
        Self {
            blend: Blend::Opaque,
            depth_write: true,
            two_sided: false,
            alpha_test: None,
        }
    }
}

/// IG_GFX's blend factors SRC_ALPHA and ONE, and the add equation.
const SRC_ALPHA: u32 = 4;
const ONE: u32 = 1;
const ADD: u32 = 0;
/// IG_GFX's GREATER, the one alpha test on the disc.
const GREATER: u32 = 4;

/// A scroll track: what moves a layer's texture coordinates over time.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Scroll {
    pub layer: i32,
    pub data: ScrollUvAnimData,
}

/// One hero technique's material.
#[derive(Debug, Clone, PartialEq)]
pub struct Material<'a> {
    /// The technique's name (`EruptorBody`).
    pub name: Option<&'a str>,
    pub pixel: VariantMask,
    pub vertex: VariantMask,
    /// In the pixel variant's sampler order.
    pub textures: Vec<Bound<'a>>,
    pub params: Params,
    pub states: States,
    pub scrolls: Vec<Scroll>,
    /// Every attribute's type, in list order.
    pub attrs: Vec<&'a str>,
}

/// The attributes a material list holds, sorted by kind; each kind once.
#[derive(Default)]
struct Attrs<'a> {
    pixel: Option<VariantMask>,
    vertex: Option<VariantMask>,
    binds: Vec<usize>,
    fixed_alpha: Option<[f32; 4]>,
    layer1_extra: Option<[f32; 4]>,
    layer2_extra: Option<[f32; 4]>,
    layer2_blend: Option<[f32; 4]>,
    layer3_blend: Option<[f32; 4]>,
    layer4_blend: Option<[f32; 4]>,
    layer4_values: Option<[f32; 4]>,
    tint: Option<i32>,
    glow: Option<DestinationAlphaAttr>,
    blend_state: Option<BlendStateAttr>,
    blend_function: Option<BlendFunctionAttr>,
    depth_write: Option<DepthWriteStateAttr>,
    cull: Option<CullFaceAttr>,
    alpha_state: Option<AlphaStateAttr>,
    alpha_function: Option<AlphaFunctionAttr>,
    names: Vec<&'a str>,
}

impl Default for Params {
    fn default() -> Self {
        Self::DEFAULT
    }
}

fn err(igz: &Igz<'_>, object: usize, field: &'static str, kind: ErrorKind) -> FormatError {
    igz.err_at(igz.objects()[object].at, field, kind)
}

fn unsupported(igz: &Igz<'_>, object: usize, field: &'static str, value: u64) -> FormatError {
    err(igz, object, field, ErrorKind::Unsupported { value })
}

/// Sets `slot` once; a second attr of the same kind is refused.
fn once<T>(
    igz: &Igz<'_>,
    a: usize,
    field: &'static str,
    slot: &mut Option<T>,
    value: T,
) -> Result<(), FormatError> {
    if slot.replace(value).is_some() {
        return Err(err(igz, a, field, ErrorKind::Duplicate));
    }
    Ok(())
}

impl<'a> Attrs<'a> {
    fn read(igz: &Igz<'a>, list_index: usize, hero: &Hero<'a>) -> Result<Self, FormatError> {
        let mut out = Self::default();
        for a in &hero.materials[&list_index].attrs {
            let Some(a) = *a else {
                return Err(err(igz, list_index, "_data", ErrorKind::Missing));
            };
            let ty = igz.type_name(&igz.objects()[a]);
            out.names.push(ty);
            let vec4 = || ShaderConstantVec4fAttr::read(igz, a).map(|c| c.data);
            match ty {
                "tfbAlphaTextureUnitIDAttr" if hero.texture_binds.contains_key(&a) => {
                    out.binds.push(a)
                }
                "igPixelShaderVariantMaskAttr" => {
                    let m = PixelShaderVariantMaskAttr::read(igz, a)?.0;
                    once(igz, a, "igPixelShaderVariantMaskAttr", &mut out.pixel, m)?;
                }
                "igVertexShaderVariantMaskAttr" => {
                    let m = VertexShaderVariantMaskAttr::read(igz, a)?.0;
                    once(igz, a, "igVertexShaderVariantMaskAttr", &mut out.vertex, m)?;
                }
                "tfbMulti2FixedAlphaValuesAttr" => once(
                    igz,
                    a,
                    "tfbMulti2FixedAlphaValuesAttr",
                    &mut out.fixed_alpha,
                    vec4()?,
                )?,
                "tfbMulti2Layer1ExtraValuesAttr" => once(
                    igz,
                    a,
                    "tfbMulti2Layer1ExtraValuesAttr",
                    &mut out.layer1_extra,
                    vec4()?,
                )?,
                "tfbMulti2Layer2ExtraValuesAttr" => once(
                    igz,
                    a,
                    "tfbMulti2Layer2ExtraValuesAttr",
                    &mut out.layer2_extra,
                    vec4()?,
                )?,
                "tfbMulti2Layer2BlendFactorsAttr" => once(
                    igz,
                    a,
                    "tfbMulti2Layer2BlendFactorsAttr",
                    &mut out.layer2_blend,
                    vec4()?,
                )?,
                "tfbMulti2Layer3BlendFactorsAttr" => once(
                    igz,
                    a,
                    "tfbMulti2Layer3BlendFactorsAttr",
                    &mut out.layer3_blend,
                    vec4()?,
                )?,
                "tfbMulti2Layer4BlendFactorsAttr" => once(
                    igz,
                    a,
                    "tfbMulti2Layer4BlendFactorsAttr",
                    &mut out.layer4_blend,
                    vec4()?,
                )?,
                "tfbMulti2Layer4ValuesAttr" => once(
                    igz,
                    a,
                    "tfbMulti2Layer4ValuesAttr",
                    &mut out.layer4_values,
                    vec4()?,
                )?,
                "tfbMaterialTintAttr" => {
                    let packed = ShaderConstantIntAttr::read(igz, a)?.data;
                    once(igz, a, "tfbMaterialTintAttr", &mut out.tint, packed)?;
                }
                "tfbDestinationAlphaAttr" => {
                    let d = DestinationAlphaAttr::read(igz, a)?;
                    once(igz, a, "tfbDestinationAlphaAttr", &mut out.glow, d)?;
                    if u8::try_from(d.alpha).is_err() {
                        return Err(unsupported(igz, a, "_alpha", u64::from(d.alpha as u32)));
                    }
                }
                "igBlendStateAttr" => {
                    let s = BlendStateAttr::read(igz, a)?;
                    once(igz, a, "igBlendStateAttr", &mut out.blend_state, s)?;
                }
                "igBlendFunctionAttr" => {
                    let f = BlendFunctionAttr::read(igz, a)?;
                    once(igz, a, "igBlendFunctionAttr", &mut out.blend_function, f)?;
                }
                "igDepthWriteStateAttr" => {
                    let d = DepthWriteStateAttr::read(igz, a)?;
                    once(igz, a, "igDepthWriteStateAttr", &mut out.depth_write, d)?;
                }
                "igCullFaceAttr" => {
                    let c = CullFaceAttr::read(igz, a)?;
                    once(igz, a, "igCullFaceAttr", &mut out.cull, c)?;
                }
                "igAlphaStateAttr" => {
                    let s = AlphaStateAttr::read(igz, a)?;
                    once(igz, a, "igAlphaStateAttr", &mut out.alpha_state, s)?;
                }
                "igAlphaFunctionAttr" => {
                    let f = AlphaFunctionAttr::read(igz, a)?;
                    once(igz, a, "igAlphaFunctionAttr", &mut out.alpha_function, f)?;
                }
                other => {
                    return Err(err(
                        igz,
                        a,
                        "_materialPass",
                        ErrorKind::WrongType {
                            expected: "a tfbmulti2 material attribute",
                            found: other.to_owned(),
                        },
                    ));
                }
            }
        }
        Ok(out)
    }

    /// What the material sets, the original's default for the rest.
    fn params(&self) -> Params {
        let d = Params::DEFAULT;
        Params {
            fixed_alpha: self.fixed_alpha.unwrap_or(d.fixed_alpha),
            layer1_extra: self.layer1_extra.unwrap_or(d.layer1_extra),
            layer2_extra: self.layer2_extra,
            layer2_blend: self.layer2_blend,
            layer3_blend: self.layer3_blend,
            layer4_blend: self.layer4_blend.unwrap_or(d.layer4_blend),
            layer4_values: self.layer4_values.unwrap_or(d.layer4_values),
            material_tint: self.tint.map_or(d.material_tint, |t| unpack_tint(t as u32)),
            // Range-checked when read.
            glow: self
                .glow
                .filter(|g| g.state)
                .and_then(|g| u8::try_from(g.alpha).ok()),
        }
    }

    fn states(&self, igz: &Igz<'_>, list: usize) -> Result<States, FormatError> {
        let mut s = States::default();
        if let Some(b) = &self.blend_state
            && b.alpha_to_coverage_enabled
        {
            return Err(err(
                igz,
                list,
                "_alphaToCoverageEnabled",
                ErrorKind::Unsupported { value: 1 },
            ));
        }
        let blending = self.blend_state.is_some_and(|b| b.enabled);
        s.blend = match (blending, &self.blend_function) {
            (false, None) => Blend::Opaque,
            (true, None) => Blend::Alpha,
            (true, Some(f)) if (f.src, f.dst, f.eq) == (SRC_ALPHA, ONE, ADD) => Blend::Additive,
            (true, Some(f)) => {
                let value = u64::from(f.src) << 32 | u64::from(f.dst);
                return Err(err(
                    igz,
                    list,
                    "igBlendFunctionAttr",
                    ErrorKind::Unsupported { value },
                ));
            }
            (false, Some(_)) => return Err(err(igz, list, "igBlendStateAttr", ErrorKind::Missing)),
        };
        s.depth_write = self.depth_write.is_none_or(|d| d.enabled);
        if let Some(c) = &self.cull {
            // Culling switched on names a mode no hero's file sets.
            if c.enabled {
                return Err(err(
                    igz,
                    list,
                    "igCullFaceAttr",
                    ErrorKind::Unsupported {
                        value: u64::from(c.mode),
                    },
                ));
            }
            s.two_sided = true;
        }
        if self.alpha_state.is_some_and(|a| a.enabled) {
            let f = self
                .alpha_function
                .ok_or_else(|| err(igz, list, "igAlphaFunctionAttr", ErrorKind::Missing))?;
            if f.func != GREATER {
                return Err(err(
                    igz,
                    list,
                    "_func",
                    ErrorKind::Unsupported {
                        value: u64::from(f.func),
                    },
                ));
            }
            s.alpha_test = Some(f.ref_value);
        }
        Ok(s)
    }
}

/// The first undrawn bit `mask` sets, refused by name.
fn undrawn(mask: u32, bits: &[u8], names: &[&'static str; 14]) -> Result<(), ErrorKind> {
    match bits.iter().find(|&&b| has(mask, b)) {
        Some(&bit) => Err(ErrorKind::Feature {
            bit,
            name: names[usize::from(bit)],
        }),
        None => Ok(()),
    }
}

impl<'a> Material<'a> {
    /// `mesh`'s material, read off `hero` (which read its list and binds).
    pub fn read(igz: &Igz<'a>, hero: &Hero<'a>, mesh: &Mesh<'a>) -> Result<Self, FormatError> {
        let t = mesh.technique_index;
        match mesh.technique.effect {
            ObjectRef::Import(imp)
                if imp.id.namespace == fnv1a32(EFFECTS.as_bytes())
                    && imp.id.object == TFBMULTI2 => {}
            ObjectRef::Import(imp) => {
                return Err(unsupported(igz, t, "_effect", u64::from(imp.id.object)));
            }
            _ => return Err(err(igz, t, "_effect", ErrorKind::Missing)),
        }
        let list_index = mesh
            .material
            .ok_or_else(|| err(igz, t, "_materialPass", ErrorKind::Missing))?;
        let attrs = Attrs::read(igz, list_index, hero)?;
        let pixel = attrs.pixel.ok_or_else(|| {
            err(
                igz,
                list_index,
                "igPixelShaderVariantMaskAttr",
                ErrorKind::Missing,
            )
        })?;
        let vertex = attrs.vertex.ok_or_else(|| {
            err(
                igz,
                list_index,
                "igVertexShaderVariantMaskAttr",
                ErrorKind::Missing,
            )
        })?;
        undrawn(pixel.base_mask, &PIXEL_UNDRAWN, &PIXEL_BITS)
            .map_err(|k| err(igz, list_index, "_baseMask", k))?;
        undrawn(vertex.base_mask, &VERTEX_UNDRAWN, &VERTEX_BITS)
            .map_err(|k| err(igz, list_index, "_baseMask", k))?;
        if !pixel_variant_exists(pixel.base_mask) || !vertex_variant_exists(vertex.base_mask) {
            let value = u64::from(pixel.base_mask) << 32 | u64::from(vertex.base_mask);
            return Err(unsupported(igz, list_index, "_baseMask", value));
        }
        let roles = samplers(pixel.base_mask);
        if roles.len() != attrs.binds.len() {
            return Err(err(
                igz,
                list_index,
                "tfbAlphaTextureUnitIDAttr",
                ErrorKind::CountMismatch {
                    found: attrs.binds.len() as u64,
                    expected: roles.len() as u64,
                },
            ));
        }
        let mut textures = Vec::with_capacity(roles.len());
        for (role, &bind) in roles.into_iter().zip(&attrs.binds) {
            textures.push(bound(igz, hero, role, bind)?);
        }
        Ok(Self {
            name: mesh.technique.node.name,
            pixel,
            vertex,
            textures,
            params: attrs.params(),
            states: attrs.states(igz, list_index)?,
            scrolls: scrolls(igz, t, mesh.technique.material_anim)?,
            attrs: attrs.names,
        })
    }

    /// The pixel variant the turntable draws: the material's bits plus fog,
    /// when the runtime owns fog (decision 14's scene: fog on, nothing else).
    pub fn drawn_pixel_mask(&self) -> u32 {
        with_fog(self.pixel)
    }

    pub fn drawn_vertex_mask(&self) -> u32 {
        with_fog(self.vertex)
    }

    /// The texture in `role`; `None` when the variant doesn't sample it.
    pub fn texture(&self, role: Role) -> Option<&Bound<'a>> {
        self.textures.iter().find(|t| t.role == role)
    }
}

/// The fog bit, which every 3D scene in the game sets.
pub const FOG: u8 = 10;

fn with_fog(m: VariantMask) -> u32 {
    if has(m.ignore_mask, FOG) {
        m.base_mask
    } else {
        m.base_mask | 1 << FOG
    }
}

/// `bind` in `role`, checked: its cached unit is the role's, it has an
/// image, and it carries `TFN1` exactly when the role is a normal map.
fn bound<'a>(
    igz: &Igz<'a>,
    hero: &Hero<'a>,
    role: Role,
    bind: usize,
) -> Result<Bound<'a>, FormatError> {
    let b = hero.texture_binds[&bind];
    let unit = b.bind.unit();
    if unit != role.unit() {
        return Err(unsupported(
            igz,
            bind,
            "_cachedUnitID",
            u64::from(unit as u16),
        ));
    }
    let texture = b
        .bind
        .texture
        .ok_or_else(|| err(igz, bind, "_texture", ErrorKind::Missing))?;
    let sampler = hero.textures[&texture];
    let hinted = sampler.format_hint == TFN1;
    if hinted != role.is_normal_map() {
        return Err(err(
            igz,
            texture,
            "_formatHint",
            ErrorKind::NormalMapHint {
                role: role.name(),
                hinted,
            },
        ));
    }
    let image = sampler
        .image
        .ok_or_else(|| err(igz, texture, "_image", ErrorKind::Missing))?;
    let image_name = Image2::read(igz, image)?
        .name
        .map(|n| n.rsplit(['/', '\\']).next().unwrap_or(n));
    Ok(Bound {
        role,
        bind,
        texture,
        sampler,
        image,
        image_name,
    })
}

/// The technique's scroll tracks; any other kind of material track (a
/// transform, flipbook or tint track, none on a hero) is refused.
fn scrolls(
    igz: &Igz<'_>,
    technique: usize,
    anim: ObjectRef<'_>,
) -> Result<Vec<Scroll>, FormatError> {
    let anim = match anim {
        ObjectRef::Null => return Ok(Vec::new()),
        ObjectRef::Object(a) => a,
        _ => return Err(unsupported(igz, technique, "_materialAnim", 0)),
    };
    let Some(list) = MaterialAnim::read(igz, anim)?.track_list else {
        return Ok(Vec::new());
    };
    let mut out = Vec::new();
    for track in MaterialAnimTrackList::read(igz, list)?.tracks {
        let ty = igz.type_name(&igz.objects()[track]);
        if ty != "tfbScrollUVAnimTrack" {
            return Err(err(
                igz,
                track,
                "_trackList",
                ErrorKind::WrongType {
                    expected: "tfbScrollUVAnimTrack",
                    found: ty.to_owned(),
                },
            ));
        }
        let t = MaterialUvAnimTrack::read(igz, track)?;
        let data = t
            .data
            .ok_or_else(|| err(igz, track, "_data", ErrorKind::Missing))?;
        out.push(Scroll {
            layer: t.layer,
            data: ScrollUvAnimData::read(igz, data)?,
        });
    }
    Ok(out)
}
