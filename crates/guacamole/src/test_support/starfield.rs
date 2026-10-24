//! Where a star sits within the sky's period, from starfield's tests; the
//! TV's layout tests stand on it too.

use bevy::prelude::*;

use crate::starfield::*;

/// Where `at` sits within the sky's period: the same for every copy of a
/// star, so two stars on a screen at the same offset are one star twice.
pub(crate) fn offset_in_period(at: Vec2) -> Vec2 {
    let rows = (at.y / PERIOD.y).floor();
    let at = at - rows * DRIFT;
    Vec2::new(at.x.rem_euclid(PERIOD.x), at.y)
}
