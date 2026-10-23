//! Characters in 3D (openspec AE.8, design decisions 8 and 9): the
//! character-shading spec as one WESL shader whose `@if`s are tfbmulti2's
//! feature bits ([`shading.wesl`](character/shading.wesl)), the material
//! that specializes it per key, and the skinned meshes built from IR v2.
//! [`remaster`] (AE.12) says what's decoded into linear light and how the
//! camera draws; [`glow`] (AE.11) is the game's glow on that frame;
//! [`turntable`] (AE.13) frames and lights a model on a card; [`warm`]
//! (AE.14) names the pipelines a model's draws compile; [`attach`] (AE.23)
//! hangs what a fresh figure carries on its model.
//!
//! The shader is checked against [`guac_ir::shading`], the spec's CPU
//! model, by an `#[ignore]`d GPU lane ([`tests/character_gpu_lane.rs`]),
//! since no oracle frame checks the remaster's arithmetic.
//!
//! [`tests/character_gpu_lane.rs`]: ../../tests/character_gpu_lane.rs

pub mod attach;
pub mod glow;
pub mod key;
pub mod material;
pub mod mesh;
pub mod remaster;
pub mod turntable;
pub mod warm;

use bevy::app::{App, Plugin};
use bevy::asset::{Assets, embedded_asset};
use bevy::pbr::MaterialPlugin;
use bevy::render::storage::ShaderBuffer;

pub use glow::{
    AlphaClear, FrameGlow, Glow, GlowPlugin, GlowView, spawn_alpha_clear, spawn_glow_camera,
    spawn_glow_masks,
};
pub use key::{KeyError, MaterialKey, Pass, SceneBits, ShaderKey, StateKey};
pub use material::{
    CharacterMaterial, CharacterTextures, CharacterUniform, LEVEL_LIGHTS, LIGHTS_FROM_SCENE,
    LIGHTS_IN_VIEW, LIGHTS_IN_WORLD, MaterialError, NO_SCENE_LIGHTS, ScenePointLights,
    ShadowLookup, reference_material,
};
pub use mesh::{MeshError, SpawnedCharacter, segment_mesh, spawn_character, static_mesh};
pub use warm::{MeshShape, WarmError, WarmKey};

/// Embeds the shader, registers [`CharacterMaterial`] and puts its two scene
/// buffers in place, every point light dark: [`NO_SCENE_LIGHTS`], which
/// every material binds unless it reads a scene's, and [`LEVEL_LIGHTS`],
/// which the level rewrites as its lights move (openspec AH.65).
pub struct CharacterShadingPlugin;

impl Plugin for CharacterShadingPlugin {
    fn build(&self, app: &mut App) {
        embedded_asset!(app, "character/shading.wesl");
        app.add_plugins(MaterialPlugin::<CharacterMaterial>::default());
    }

    // After every plugin's build, so the render plugin's buffer assets are
    // there whichever was added first.
    fn finish(&self, app: &mut App) {
        if let Some(mut buffers) = app.world_mut().get_resource_mut::<Assets<ShaderBuffer>>() {
            let dark = ScenePointLights::new(&[]).buffer();
            let _ = buffers.insert(NO_SCENE_LIGHTS.id(), dark.clone());
            let _ = buffers.insert(LEVEL_LIGHTS.id(), dark);
        }
    }
}
