//! What the TV's target shows over a backdrop, from surfaces.rs; the titles'
//! tests stand on it too.

use bevy::prelude::*;

/// What the TV's sRGB target shows for `color` over `backdrop`: the hardware
/// blends in linear light, the half of the mismatch [`css_over`] corrects.
pub(crate) fn linear_over(backdrop: Color, color: Color) -> LinearRgba {
    let (b, c) = (backdrop.to_linear(), color.to_linear());
    let a = c.alpha;
    LinearRgba::rgb(
        b.red + (c.red - b.red) * a,
        b.green + (c.green - b.green) * a,
        b.blue + (c.blue - b.blue) * a,
    )
}
