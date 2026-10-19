//! IR v2's 3D-UI light rig (openspec AE.6, design decision 14, Open
//! question 4): `permanent/global.bld`'s three `_spriteLight`s, the lights
//! the game keeps for 3D drawn in its UI and the turntable's rig.

use serde::{Deserialize, Serialize};

use crate::DumpIdentity;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Rig {
    pub identity: DumpIdentity,
    /// `_spriteLight1`, 2 and 3: the key, the fill and the back light.
    pub lights: Vec<RigLight>,
}

/// One directional light as stored. Its colours are RGBA encoded for
/// display, which the remaster decodes before lighting with them.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RigLight {
    pub name: String,
    pub diffuse: [u8; 4],
    pub ambient: [u8; 4],
    pub specular: [u8; 4],
    pub multiplier: f32,
    pub position: [f32; 3],
    /// Euler angles in degrees, as stored; the first is the pitch (the
    /// key's -45).
    pub rotation: [f32; 3],
}
