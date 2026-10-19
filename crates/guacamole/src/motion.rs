//! The TV's motion switches (openspec AP, design decision 9). Today that's
//! [`ReducedMotion`]; the eased focus pop, card rise and float (AP.13, AP.14)
//! read it too.

use bevy::prelude::*;

/// Ambient motion off (the starfield's drift; later the idle float and the
/// halo spin) and entrances at their end state, so a shot of a screen comes
/// out the same on every run. A switch for dev shots, not a user setting:
/// on whenever `GUAC_SHOT_DIR` is set (`shots` inserts it), off otherwise.
#[derive(Resource, Debug, Default, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ReducedMotion(pub(crate) bool);

impl ReducedMotion {
    /// From `GUAC_SHOT_DIR`'s value: set at all, even to a folder `shots`
    /// then refuses, means reduced. A release build has no shots and never
    /// reduces.
    #[cfg(any(feature = "dev-tools", test))]
    pub(crate) fn for_shots(shot_dir: Option<&std::ffi::OsStr>) -> Self {
        Self(shot_dir.is_some())
    }
}

#[cfg(test)]
mod tests {
    use std::ffi::OsStr;

    use super::*;

    #[test]
    fn reduced_motion_follows_guac_shot_dir() {
        assert_eq!(ReducedMotion::for_shots(None), ReducedMotion(false));
        assert_eq!(
            ReducedMotion::for_shots(Some(OsStr::new("/Users/someone/shots"))),
            ReducedMotion(true)
        );
        // Set but empty is still set, as `shots` reads it.
        assert_eq!(
            ReducedMotion::for_shots(Some(OsStr::new(""))),
            ReducedMotion(true)
        );
        assert_eq!(ReducedMotion::default(), ReducedMotion(false));
    }
}
