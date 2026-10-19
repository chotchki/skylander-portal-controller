//! A level's collision (openspec AG.2, design decisions 8 and 10): each
//! `tfbPhysicsWorld` of a level's IGZ read through the Havok packfile its
//! `_data` holds to the triangle mesh the hero stands on.
//!
//! - One world per scene: 75 over the disc's 143 `level.bld` files, 68 with
//!   a body (ag-levels). The rest hold an empty system, a grass scene's
//!   among them.
//! - The mesh is its own proxy, far simpler than what's drawn (level 213:
//!   3,080 triangles against 432,954), so it's read, never rebuilt from the
//!   render geometry.
//! - The level's frame: a vertex plus its body's translation
//!   ([`Body::world_vertices`]). Every body on the disc is unrotated; a
//!   rotated one is refused.
//! - Placements' own solids (`CollisionInfo`) are AG.3's.

use crate::igz::Igz;
use crate::igz::layouts::{HAVOK_MAGIC, Layout, Payload, PhysicsWorld, is_a};
use crate::{ErrorKind, FormatError};

pub use crate::havok::physics::{Body, Collision, Mesh, Mopp, Triangle};

#[cfg(any(test, feature = "test-fixtures"))]
pub mod builder;
#[cfg(test)]
mod tests;

/// A `tfbPhysicsWorld` and the collision its `_data` holds.
#[derive(Debug, Clone, PartialEq)]
pub struct World<'a> {
    pub layout: PhysicsWorld<'a>,
    pub collision: Collision,
}

impl<'a> World<'a> {
    /// The scene's Maya file (`Levels/Level_213/Level_213_ART_MASTER.ma`).
    pub fn name(&self) -> Option<&'a str> {
        self.layout.world.data_info.info.name
    }
}

/// The `tfbPhysicsWorld` at `object`, read through to its mesh. A world
/// without a packfile in `_data` is refused: every one on the disc has one.
pub fn world<'a>(igz: &Igz<'a>, object: usize) -> Result<World<'a>, FormatError> {
    let layout = PhysicsWorld::read(igz, object)?;
    let at = igz.objects()[object].at;
    let bytes = match layout.world.data_info.payload() {
        Some(Payload::Havok(bytes)) => bytes,
        Some(Payload::LipSync(bytes) | Payload::Other(bytes)) => {
            let kind = ErrorKind::BadMagic {
                expected: &HAVOK_MAGIC,
                found: bytes.iter().take(HAVOK_MAGIC.len()).copied().collect(),
            };
            return Err(igz.err_at(at, "_data", kind));
        }
        None => return Err(igz.err_at(at, "_data", ErrorKind::Missing)),
    };
    Ok(World {
        layout,
        collision: Collision::parse(bytes)?,
    })
}

/// Every `tfbPhysicsWorld` in a level's IGZ, in object order, each read.
pub fn worlds<'s, 'a>(
    igz: &'s Igz<'a>,
) -> impl Iterator<Item = (usize, Result<World<'a>, FormatError>)> + 's {
    igz.objects()
        .iter()
        .enumerate()
        .filter(|(_, o)| is_a(igz.type_name(o), PhysicsWorld::TYPE))
        .map(|(i, _)| (i, world(igz, i)))
}
