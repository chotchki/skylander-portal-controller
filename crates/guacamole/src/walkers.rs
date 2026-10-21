//! Who walks the level (openspec AG.21, design decision 1): a character
//! walks only on numbers measured for it. Until AG.27 reads every
//! character's tuning off its scripts (Open question 1's (b)), that is
//! Eruptor alone, on AG.8's live reads, and any other character placed
//! keeps the portal view (Open question 1's (a) meanwhile).
//!
//! The hero's drawn model and its keys are the card's model (AE's), drawn
//! in [`crate::level_scene::hero_scene`]: the keys the census's card tables
//! hold, so the start's pass has compiled them (`tests/giants_census.rs`).

use guac_motion::locomotion::Gaits;
use guac_motion::solid::{HavokSolid, SolidInfo};
use guac_motion::tuning::{self, Tuning};

/// A character that walks: its load file (the roster's, the model's key)
/// and the numbers it walks on.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Walker {
    pub load_file: &'static str,
    pub tuning: Tuning,
    /// Its `CollisionInfo`, before its scale.
    pub shape: SolidInfo,
    /// Its scale in the level, which its solid and its drawn model take.
    pub scale: f32,
    /// Its walk's and run's authored speeds, which its locomotion mixes
    /// them by (AG.30; every character's off its scripts is AG.27's).
    pub gaits: Gaits,
}

impl Walker {
    /// The solid the original builds for it: its shape at its scale.
    pub fn solid(&self) -> Option<HavokSolid> {
        self.shape.havok([self.scale * 100.0; 3])
    }
}

/// Every character that walks: Eruptor on AG.8's numbers (21.5 ft/s,
/// 1440°/s, a 4 x 6 x 3 ft sphere at 150 %) and AG.11's gaits (walk 3.658,
/// run 6.401 m/s).
pub const WALKERS: &[Walker] = &[Walker {
    load_file: "009_Eruptor",
    tuning: Tuning::ERUPTOR,
    shape: tuning::ERUPTOR_SHAPE,
    scale: tuning::ERUPTOR_SCALE,
    gaits: Gaits::ERUPTOR,
}];

/// `None` for a character that keeps the portal view.
pub fn walker(load_file: &str) -> Option<&'static Walker> {
    WALKERS.iter().find(|w| w.load_file == load_file)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn eruptor_walks_on_his_live_numbers_and_no_one_else_yet() {
        let eruptor = walker("009_Eruptor").expect("Eruptor walks");
        assert_eq!(eruptor.tuning, Tuning::ERUPTOR);
        assert_eq!(eruptor.solid(), Some(tuning::eruptor_solid()));
        assert_eq!(eruptor.scale, 1.5);
        assert_eq!(eruptor.gaits, Gaits::ERUPTOR);
        assert!(walker("070_TreeRex").is_none());
        assert!(walker("009_e2e").is_none());
        assert_eq!(WALKERS.len(), 1);
    }
}
