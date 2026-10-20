//! Synthetic geometry and a frame runner for the controller's scenario tests
//! (openspec AG.16): the shared [`guac_motion::scenarios`] helpers, on the
//! parry side of the seam.

#![allow(dead_code)]

pub use guac_motion::scenarios::*;

use guac_motion::parry::ParryWorld;
use guac_motion::seam::{BodyId, Pose};
use guac_motion::solid::HavokSolid;

/// The level and any placements' solids, on parry.
pub fn world(soup: &Soup, solids: &[(BodyId, HavokSolid, Pose)]) -> ParryWorld {
    ParryWorld::new(&bodies(soup, solids)).unwrap()
}

/// A shared scenario run on parry.
pub fn on_parry(scenario: Scenario) {
    let world = ParryWorld::new(&scenario.bodies).unwrap();
    if let Err(e) = (scenario.check)(&world) {
        panic!("{}: {e}", scenario.name);
    }
}
