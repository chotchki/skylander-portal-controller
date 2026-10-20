//! A scroll track's motion (openspec AE.21; the character-shading spec's
//! scroll requirements): the 2x3 transform a [`Scroll`] gives the
//! texture-coordinate set its layer names, a pure function of the track as
//! stored and the seconds it has run, beside [`crate::clip`]'s evaluator.
//! Worked in f64, handed out in f32 as the shader takes it.
//!
//! - The first two columns turn the set by the rotation phase (radians)
//!   and scale each axis by its own scale.
//! - The third is each axis's offset: velocity times t plus amplitude times
//!   the sine of (frequency times t plus phase), less its whole part with
//!   its sign kept (-1.25 leaves -0.25).
//!
//! The original runs a track on the scene's clock less the track's start
//! time, or at a time the track fixes. None of the 4,650 tracks on the disc
//! stores either (AE.21's census), so a track's time here is the caller's
//! clock, from whatever zero the caller picks: the original's clock counts
//! seconds from the console's start, so where its slide stands when a part
//! appears is arbitrary anyway.
//!
//! Where the two part: the original sums an offset in single precision
//! before dropping the whole part, so it's off the exact value by up to half
//! an f32 ulp of the sum (3.1e-5 once the sum passes 512, Hot Dog's fire
//! 274 s into its clock); this sums in f64.

use crate::model::Scroll;
use crate::shading::UvTransform;

/// The texture-coordinate sets a material transforms, so the layers a track
/// can name.
pub const SETS: usize = 3;

impl Scroll {
    /// The set this track moves, its layer; `None` past the three (no track
    /// on the disc names one).
    pub fn set(&self) -> Option<usize> {
        usize::try_from(self.layer).ok().filter(|&s| s < SETS)
    }

    /// What its set reads through once the track has run `seconds` on the
    /// caller's clock.
    pub fn transform(&self, seconds: f64) -> UvTransform {
        let (sin, cos) = f64::from(self.rotation_phase).sin_cos();
        let axis = |i: usize| {
            let f = |x: f32| f64::from(x);
            let angle = f(self.frequency[i]) * seconds + f(self.phase[i]);
            let slide = f(self.velocity[i]) * seconds + f(self.amplitude[i]) * angle.sin();
            (f(self.scale[i]), slide.fract())
        };
        let (su, ou) = axis(0);
        let (sv, ov) = axis(1);
        UvTransform {
            u: [(su * cos) as f32, (-su * sin) as f32, ou as f32],
            v: [(sv * sin) as f32, (sv * cos) as f32, ov as f32],
        }
    }
}

/// A material's three transforms `seconds` into its tracks: the identity on
/// a set no track moves. On a set two tracks move, the later's, a choice:
/// no material on the disc has two on one set, so the original's way with
/// it wasn't read.
pub fn transforms(scrolls: &[Scroll], seconds: f64) -> [UvTransform; SETS] {
    let mut out = [UvTransform::IDENTITY; SETS];
    for s in scrolls {
        if let Some(set) = s.set() {
            out[set] = s.transform(seconds);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::f32::consts::{FRAC_1_SQRT_2, FRAC_PI_2, FRAC_PI_6, PI};

    const STILL: Scroll = Scroll {
        layer: 0,
        scale: [1.0, 1.0],
        velocity: [0.0, 0.0],
        amplitude: [0.0, 0.0],
        frequency: [0.0, 0.0],
        phase: [0.0, 0.0],
        rotation_phase: 0.0,
    };

    fn close(a: UvTransform, b: UvTransform) {
        let pairs = a.u.iter().chain(&a.v).zip(b.u.iter().chain(&b.v));
        for (x, y) in pairs {
            assert!((x - y).abs() < 1e-6, "{a:?} vs {b:?}");
        }
    }

    /// The identity with these offsets: a track that only slides.
    fn slid(u: f32, v: f32) -> UvTransform {
        UvTransform {
            u: [1.0, 0.0, u],
            v: [0.0, 1.0, v],
        }
    }

    #[test]
    fn a_track_that_holds_still_is_the_identity_at_any_time() {
        for t in [0.0, 0.5, 3.25, 1000.0] {
            close(STILL.transform(t), UvTransform::IDENTITY);
        }
    }

    #[test]
    fn hot_dogs_fire_slides_down_its_colour_map() {
        // Velocity -1.869 on v, as on the disc: -1.869 t less its whole part.
        let fire = Scroll {
            velocity: [0.0, -1.869],
            ..STILL
        };
        close(fire.transform(0.5), slid(0.0, -0.9345));
        close(fire.transform(1.0), slid(0.0, -0.869));
        close(fire.transform(2.0), slid(0.0, -0.738));
        // Ninjini's: 0.77 t, 1.54 at 2 s, so 0.54.
        let genie = Scroll {
            velocity: [0.0, 0.77],
            ..STILL
        };
        close(genie.transform(2.0), slid(0.0, 0.54));
    }

    #[test]
    fn the_offset_keeps_only_the_part_after_the_point_and_its_sign() {
        let up = Scroll {
            velocity: [1.25, 0.0],
            ..STILL
        };
        close(up.transform(1.0), slid(0.25, 0.0));
        let down = Scroll {
            velocity: [-1.25, 0.0],
            ..STILL
        };
        close(down.transform(1.0), slid(-0.25, 0.0));
        // A whole turn of the texture lands back on 0, so the slide loops
        // every 1 / velocity seconds: 0.5 a second repeats every 2.
        let slow = Scroll {
            velocity: [0.5, 0.0],
            ..STILL
        };
        close(slow.transform(2.0), slid(0.0, 0.0));
        close(slow.transform(0.4), slow.transform(2.4));
    }

    #[test]
    fn the_amplitude_swings_by_the_sine_of_frequency_times_t_plus_phase() {
        // 0.5 sin(pi t): 0.5 at 0.5 s, back to 0 at 1 s, -0.5 at 1.5 s.
        let swing = Scroll {
            amplitude: [0.5, 0.0],
            frequency: [PI, 0.0],
            ..STILL
        };
        close(swing.transform(0.5), slid(0.5, 0.0));
        close(swing.transform(1.0), slid(0.0, 0.0));
        close(swing.transform(1.5), slid(-0.5, 0.0));
        // The phase alone at t = 0: 0.5 sin(pi/6) = 0.25, here on v.
        let phased = Scroll {
            amplitude: [0.0, 0.5],
            phase: [0.0, FRAC_PI_6],
            ..STILL
        };
        close(phased.transform(0.0), slid(0.0, 0.25));
        // With a velocity under it the whole part comes off the sum:
        // 2.75 + 0.5 sin(pi/2) = 3.25 at 1 s, so 0.25 (the slide's 0.75 plus
        // the swing would be 1.25).
        let both = Scroll {
            velocity: [2.75, 0.0],
            amplitude: [0.5, 0.0],
            frequency: [FRAC_PI_2, 0.0],
            ..STILL
        };
        close(both.transform(1.0), slid(0.25, 0.0));
    }

    #[test]
    fn each_axis_swings_at_its_own_frequency() {
        // 0.5 sin(pi t) on u and 0.5 sin(2 pi t) on v: at 0.5 s u at its
        // peak and v back at 0; at 0.75 s u at 0.5 sin(3 pi/4) = 0.354 and v
        // at its trough.
        let two = Scroll {
            amplitude: [0.5, 0.5],
            frequency: [PI, 2.0 * PI],
            ..STILL
        };
        close(two.transform(0.5), slid(0.5, 0.0));
        close(two.transform(0.75), slid(0.5 * FRAC_1_SQRT_2, -0.5));
    }

    #[test]
    fn the_rotation_turns_the_set_and_each_axis_scales_by_its_own() {
        // Scale (2, 1) turned a quarter: u' = 2 cos r u - 2 sin r v = -2 v,
        // v' = sin r u + cos r v = u.
        let turned = Scroll {
            scale: [2.0, 1.0],
            rotation_phase: FRAC_PI_2,
            ..STILL
        };
        let t = turned.transform(7.0);
        close(
            t,
            UvTransform {
                u: [0.0, -2.0, 0.0],
                v: [1.0, 0.0, 0.0],
            },
        );
        let [u, v] = t.apply([0.25, 0.5]);
        assert!((u + 1.0).abs() < 1e-6 && (v - 0.25).abs() < 1e-6);
        // Unturned, the scales stand alone on the diagonal.
        let stretched = Scroll {
            scale: [3.0, 0.5],
            ..STILL
        };
        close(
            stretched.transform(0.0),
            UvTransform {
                u: [3.0, 0.0, 0.0],
                v: [0.0, 0.5, 0.0],
            },
        );
    }

    #[test]
    fn each_track_moves_the_set_its_layer_names() {
        let second = Scroll {
            layer: 1,
            velocity: [0.25, 0.0],
            ..STILL
        };
        let t = transforms(&[second], 1.0);
        assert_eq!(t[0], UvTransform::IDENTITY);
        close(t[1], slid(0.25, 0.0));
        assert_eq!(t[2], UvTransform::IDENTITY);
        // No track: every set as stored.
        assert_eq!(transforms(&[], 5.0), [UvTransform::IDENTITY; SETS]);
        // A layer past the three, or below them, moves nothing.
        for layer in [3, -1] {
            let stray = Scroll { layer, ..second };
            assert_eq!(stray.set(), None);
            assert_eq!(transforms(&[stray], 1.0), [UvTransform::IDENTITY; SETS]);
        }
        // Two on one set: the later's.
        let later = Scroll {
            layer: 1,
            velocity: [0.5, 0.0],
            ..STILL
        };
        close(transforms(&[second, later], 1.0)[1], slid(0.5, 0.0));
    }
}
