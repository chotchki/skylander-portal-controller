//! What a placement carries for collision and movement (openspec AG.14,
//! design decisions 4 and 8), as the disc stores it: its solid and its
//! movement parameters. The hero side ([`crate::hero`]) carries a
//! character's; the level stage's placements carry the same records.
//!
//! Nothing here is converted. The disc keeps sizes, speeds and
//! accelerations in feet (x 0.3048 for metres), turn rates in degrees per
//! second, weights in pounds and the solid's offset in metres, and a
//! placement's scale (percent) multiplies its solid. How a solid becomes a
//! collision shape (0.1 m smaller all round, a 0.05 m convex radius, the
//! pivot at the bottom centre) is the controller's, not the IR's.

use serde::{Deserialize, Serialize};

/// The primitive a solid is, in the disc's order (bits 0-2 of its
/// properties).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Shape {
    None,
    Box,
    Capsule,
    Sphere,
    Cylinder,
    Mesh,
}

/// A placement's solid (`CollisionInfo`) as stored.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Solid {
    pub shape: Shape,
    /// Blocks what moves into it (bit 3).
    pub solid: bool,
    /// Collides with the level (bit 4).
    pub world: bool,
    /// Others stand on it (bit 5).
    pub actor_surface: bool,
    /// Its offset from the placement in metres, and Euler angles in
    /// degrees.
    pub offset: [f32; 3],
    pub rotation: [f32; 3],
    /// Full extents in feet, before the placement's scale.
    pub width: f32,
    pub height: f32,
    pub length: f32,
    /// Pounds.
    pub weight: f32,
    /// Percent.
    pub rebound: f32,
}

/// A placement's movement parameters (`ActorParameters`) as stored: speeds
/// in feet per second, accelerations in feet per second squared, turn rates
/// in degrees per second. The game's scripts change a hero's at run time,
/// so these are the base what they set overrides.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Movement {
    pub ground_max_speed: f32,
    pub ground_acceleration: f32,
    pub ground_deceleration: f32,
    pub ground_turn_rate: f32,
    pub air_max_speed: f32,
    pub air_acceleration: f32,
    pub air_deceleration: f32,
    pub air_turn_rate: f32,
    /// The terminal falling speed, and gravity.
    pub gravity_max_speed: f32,
    pub gravity_acceleration: f32,
    pub jump_max_speed: f32,
    pub jump_acceleration: f32,
    pub jump_initial_speed: f32,
    /// An enum, kept raw: 0 on foot.
    pub motion_type: u32,
    pub tilts_with_ground: bool,
    /// The placement's own time scale, 1 at normal speed.
    pub time_scale: f32,
}
