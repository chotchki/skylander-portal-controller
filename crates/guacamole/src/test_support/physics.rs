//! A seam body spawned at once, from physics.rs; hero's and play's tests
//! stand on it too.

use bevy::prelude::*;
use guac_motion::parry::WorldError;
use guac_motion::seam::Body;

use crate::physics::*;

/// Spawns a seam body at once ([`PreparedBody`]'s checks and kinds): the
/// tests' worlds; the level's are prepared off the frame.
pub(crate) fn spawn_body(commands: &mut Commands, body: &Body) -> Result<Entity, WorldError> {
    Ok(PreparedBody::new(body)?.spawn(commands))
}
