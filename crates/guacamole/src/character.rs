//! Characters in 3D (openspec AE.8, design decisions 8 and 9): the
//! character-shading spec as one WESL shader whose `@if`s are tfbmulti2's
//! feature bits ([`shading.wesl`](character/shading.wesl)), the material
//! that specializes it per key, and the skinned meshes built from IR v2.
//! [`remaster`] (AE.12) says what's decoded into linear light and how the
//! camera draws; [`glow`] (AE.11) is the game's glow on that frame.
//!
//! The shader is checked against [`guac_ir::shading`], the spec's CPU
//! model, by an `#[ignore]`d GPU lane ([`tests/character_gpu_lane.rs`]),
//! since no oracle frame checks the remaster's arithmetic.
//!
//! [`tests/character_gpu_lane.rs`]: ../../tests/character_gpu_lane.rs

pub mod glow;
pub mod key;
pub mod material;
pub mod mesh;
pub mod remaster;

use bevy::app::{App, Plugin};
use bevy::asset::embedded_asset;
use bevy::pbr::MaterialPlugin;

pub use glow::{Glow, GlowPlugin, spawn_glow_camera, spawn_glow_masks};
pub use key::{KeyError, MaterialKey, Pass, SceneBits, ShaderKey, StateKey};
pub use material::{
    CharacterMaterial, CharacterTextures, CharacterUniform, MaterialError, ShadowLookup,
    reference_material,
};
pub use mesh::{MeshError, SpawnedCharacter, segment_mesh, spawn_character};

/// Embeds the shader and registers [`CharacterMaterial`].
pub struct CharacterShadingPlugin;

impl Plugin for CharacterShadingPlugin {
    fn build(&self, app: &mut App) {
        embedded_asset!(app, "character/shading.wesl");
        app.add_plugins(MaterialPlugin::<CharacterMaterial>::default());
    }
}
