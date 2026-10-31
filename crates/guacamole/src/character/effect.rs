//! The two effects past tfbmulti2 a level draws (openspec AG.24, design
//! decision 9), each on the character material through a pass of its own
//! (stand-in ledger: `grass-warp`): tfbgrass's shells cut to their blades
//! ([`Pass::Grass`]), no wind, and tfbwarp's surfaces bending the frame
//! behind them by tfbwarp's own reach ([`Pass::Warp`]). The third,
//! tfbshadow's `player_shadow`, is the original's shadow map's and draws
//! nothing here (`level_scene::effect_material`).

use bevy::math::Vec4;

use super::key::Pass;
use super::material::CharacterMaterial;

/// How far a warp bends the frame, as a share of it either way at the
/// ripple map's strongest: tfbwarp's own, a 64th of the frame for each unit
/// a ripple channel sits off its middle (read off its program in AG.13's
/// fresh-state capture, the water's draws; the review of AG.24).
pub const WARP_BEND: f32 = 1.0 / 128.0;
/// How many times a grass shell tiles its height map and its blade map over
/// its texture coordinates (the colour map's once): look choices that read
/// as tufts of blades a couple of centimetres across (tfbgrass's own are
/// among its shader parameters, unread).
pub const GRASS_HEIGHT_TILING: f32 = 0.5;
pub const GRASS_BLADE_TILING: f32 = 2.0;
/// The least reach a blade draws at, on the ground's shell, rising to the
/// whole of it at the top's: the share of the ground left to the level's own
/// ground between the blades, so a patch thins out over its edges (a look
/// choice, beside the oracle's fresh-state frame).
pub const GRASS_FLOOR: f32 = 0.08;

/// What a part of an effect draws as, beside its tfbmulti2-shaped material.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Effect {
    /// A tfbgrass shell: its height up its stack, 0 the ground's shell to 1
    /// the top's.
    Grass { shell: f32 },
    /// A tfbwarp surface.
    Warp,
}

impl Effect {
    /// Its fragment entry point's pass, which keys a pipeline apart from the
    /// colour pass's of the same masks and states.
    pub fn pass(&self) -> Pass {
        match self {
            Self::Grass { .. } => Pass::Grass,
            Self::Warp => Pass::Warp,
        }
    }

    /// Its numbers as the uniform's `effect` carries them.
    pub fn uniform(&self) -> Vec4 {
        match *self {
            Self::Grass { shell } => Vec4::new(
                shell.clamp(0.0, 1.0),
                GRASS_HEIGHT_TILING,
                GRASS_BLADE_TILING,
                GRASS_FLOOR,
            ),
            Self::Warp => Vec4::new(WARP_BEND, 0.0, 0.0, 0.0),
        }
    }

    /// `material`, built from the effect's tfbmulti2-shaped material, drawn
    /// as the effect: its pass and its numbers.
    pub fn apply(&self, material: &mut CharacterMaterial) {
        material.key.pass = self.pass();
        material.uniform.effect = self.uniform();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::character::key::{MaterialKey, ShaderKey, StateKey};
    use crate::character::material::CharacterTextures;
    use bevy::asset::Handle;
    use guac_ir::shading::Scene;

    #[test]
    fn an_effect_takes_its_pass_and_its_numbers() {
        let mut m = CharacterMaterial::new(
            MaterialKey {
                shader: ShaderKey::new(0, 0).unwrap(),
                states: StateKey::OPAQUE,
                pass: Pass::Colour,
            },
            &guac_ir::shading::Material::new(),
            &Scene::new(),
            0.5,
            CharacterTextures {
                colour: Some(Handle::default()),
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(m.uniform.effect, Vec4::ZERO, "a tfbmulti2 part's");
        Effect::Grass { shell: 0.25 }.apply(&mut m);
        assert_eq!(m.key.pass, Pass::Grass);
        assert_eq!(
            m.uniform.effect,
            Vec4::new(0.25, GRASS_HEIGHT_TILING, GRASS_BLADE_TILING, GRASS_FLOOR)
        );
        assert!(!m.key.casts_shadow(), "a shell casts none");
        Effect::Grass { shell: 1.5 }.apply(&mut m);
        assert_eq!(m.uniform.effect.x, 1.0, "held to the stack");
        Effect::Warp.apply(&mut m);
        assert_eq!(m.key.pass, Pass::Warp);
        assert_eq!(m.uniform.effect.x, WARP_BEND);
        // tfbwarp's own reach: its program moves the copy's read by
        // (channel - 0.5) / 64 of the frame, (2 x channel - 1) / 128.
        assert_eq!(WARP_BEND, 0.5 / 64.0);
        assert!(!m.key.casts_shadow());
    }
}
