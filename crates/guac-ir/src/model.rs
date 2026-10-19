//! IR v2's character models (openspec AE.6, design decisions 6-9): per
//! character load file, the model the game draws for the toy, decoded to
//! bind pose with everything a Bevy mesh, skin and material take, and its
//! idle as spline data ([`crate::clip`]).
//!
//! Joints are Havok's, in Havok's order (the order clips name them), each
//! with the inverse joint the game skins with: Alchemy's (AC.17). A skin
//! influence names a joint. Every segment is one a fresh figure draws (the
//! whole of the portal model's scene graph, AE.5). Textures are files of
//! their own, `textures/<sha256>.ktx2`, shared by content across models.

use serde::{Deserialize, Serialize};

use crate::DumpIdentity;
use crate::clip::Clip;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Model {
    pub identity: DumpIdentity,
    /// The roster's load file (`009_Eruptor`), the key figures share.
    pub load_file: String,
    /// The model's file name (`Eruptor.mdl`).
    pub name: String,
    pub skeleton: Skeleton,
    /// One per technique, in scene-graph order; segments index it.
    pub materials: Vec<Material>,
    pub segments: Vec<Segment>,
    pub idle: Clip,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Skeleton {
    pub joints: Vec<Joint>,
}

/// Translation, rotation (x, y, z, w) and per-axis scale, composed the way
/// Havok composes them ([`Transform::then`]).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Transform {
    pub translation: [f32; 3],
    pub rotation: [f32; 4],
    pub scale: [f32; 3],
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Joint {
    pub name: String,
    /// An earlier joint; `None` on a root.
    pub parent: Option<u16>,
    /// Havok's reference pose, local to the parent: what a clip that leaves
    /// the joint untracked poses it at.
    pub reference: Transform,
    /// Alchemy's inverse joint as columns of the column-vector matrix, so
    /// `Mat4::from_cols_array_2d` takes it as is.
    pub inverse_bind: [[f32; 4]; 4],
}

/// One Edge segment in bind pose: a triangle list, every vertex skinned.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Segment {
    /// Into [`Model::materials`].
    pub material: u16,
    /// The `igPS3EdgeGeometrySegment`'s object index in the level, as AC.13
    /// names the draws (Eruptor's 122, 123 and 135).
    pub source: u32,
    pub positions: Vec<[f32; 3]>,
    pub normals: Vec<[f32; 3]>,
    /// xyz and w the handedness the bitangent takes (cross(N, T) * w): +1
    /// on every character, whose tangents are 3-component CMP words the GPU
    /// reads with w = 1.
    pub tangents: Option<Vec<[f32; 4]>>,
    /// UV sets 0, 1 and 2, the first always there.
    pub uvs: Vec<Vec<[f32; 2]>>,
    /// RGBA as stored.
    pub colors: Option<Vec<[u8; 4]>>,
    /// Four joints and their weights per vertex; weights are of 255 and sum
    /// to it, a zero weight's joint 0.
    pub joints: Vec<[u16; 4]>,
    pub weights: Vec<[u8; 4]>,
    pub indices: Vec<u16>,
}

/// A material's tfbmulti2 variant: the bits it sets and those it owns
/// (the runtime sets the rest: shadows 8, fog 10, point lights 11-13 on
/// the pixel side).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Mask {
    pub base: u32,
    pub ignore: u32,
}

/// The pixel variant's samplers, which a texture's role names.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub enum TextureRole {
    Layer1BaseNormalMap,
    Layer1BaseSpecMap,
    Layer1ColorMap,
    Layer2BaseNormalMap,
    Layer2BaseSpecMap,
    Layer2ColorMap,
    Layer3ColorMap,
    Layer4ColorMap,
}

impl TextureRole {
    /// Whether the texture holds a colour encoded for display (the remaster
    /// decodes those): every role but the normal maps.
    pub fn is_color(self) -> bool {
        !matches!(self, Self::Layer1BaseNormalMap | Self::Layer2BaseNormalMap)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Wrap {
    Clamp,
    Repeat,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Filter {
    Nearest,
    Linear,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Sampler {
    pub wrap_u: Wrap,
    pub wrap_v: Wrap,
    pub mag: Filter,
    pub min: Filter,
    /// The mipmap filter; `None` samples level 0 alone.
    pub mip: Option<Filter>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TextureSlot {
    pub role: TextureRole,
    /// `textures/<sha256 of the file>.ktx2`, relative to the extract.
    pub texture: String,
    /// The image's name on the disc (`eruptor_n`).
    pub image: String,
    pub sampler: Sampler,
}

/// The shader constants, the original's defaults filled in where the disc
/// sets none (the character-shading spec's material requirement).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Params {
    /// x layer 1's fixed alpha (it takes over from the colour map's on a ramp
    /// from -0.001 to -0.0009), y layer 2's, z layer 3's strength, w layer 4's.
    pub fixed_alpha: [f32; 4],
    /// z the spec exponent, w the normal map's detail weight.
    pub layer1_extra: [f32; 4],
    pub layer2_extra: Option<[f32; 4]>,
    /// Over, add and multiply weights for layers 2, 3 and 4.
    pub layer2_blend: Option<[f32; 4]>,
    pub layer3_blend: Option<[f32; 4]>,
    pub layer4_blend: [f32; 4],
    /// y the rim's UV scale, w its colour/alpha split.
    pub layer4_values: [f32; 4],
    /// RGBA, neutral at 128: a multiplier, not a colour.
    pub material_tint: [u8; 4],
    pub glow: Glow,
}

/// What a part writes into the glow pass's mask (the character-shading
/// spec's glow mask, read off AC.13's captures at AE.11).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Glow {
    /// No glow attribute: the part glows nowhere, as Eruptor's eyes don't.
    None,
    /// The attribute on: its amount of 255.
    Amount(u8),
    /// The attribute off: layer 1's alpha, switched hard at a fixed alpha
    /// of 0, times the vertex and tint alphas.
    LayerAlpha,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Blend {
    Opaque,
    /// By source alpha.
    Alpha,
    /// Source alpha plus what's there.
    Additive,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct RenderStates {
    pub blend: Blend,
    pub depth_write: bool,
    pub two_sided: bool,
    /// Alpha passes above this.
    pub alpha_test: Option<f32>,
}

/// A scroll track's parameters as stored: what moves a layer's texture
/// coordinates over time.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Scroll {
    pub layer: i32,
    pub scale: [f32; 2],
    pub velocity: [f32; 2],
    pub amplitude: [f32; 2],
    pub frequency: [f32; 2],
    pub phase: [f32; 2],
    pub rotation_phase: f32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Material {
    /// The technique's name (`EruptorBody`).
    pub name: String,
    pub pixel: Mask,
    pub vertex: Mask,
    /// In the pixel variant's sampler order.
    pub textures: Vec<TextureSlot>,
    pub params: Params,
    pub states: RenderStates,
    pub scrolls: Vec<Scroll>,
}

impl Material {
    /// The texture in `role`, if the variant samples one there.
    pub fn texture(&self, role: TextureRole) -> Option<&TextureSlot> {
        self.textures.iter().find(|t| t.role == role)
    }
}

impl Model {
    /// Every segment's vertices: segments share none, each has its own.
    pub fn vertices(&self) -> usize {
        self.segments.iter().map(|s| s.positions.len()).sum()
    }

    pub fn triangles(&self) -> usize {
        self.segments.iter().map(|s| s.indices.len() / 3).sum()
    }
}
