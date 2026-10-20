//! The move stick as the original reads it (pad-input's The move stick's dead
//! zone; hero-movement's The stick walks the hero as seen from the camera):
//! per axis, a quarter of the travel or less is centred and the rest counts
//! from nothing to full; the strength is the two axes' length, capped at 1;
//! the direction is the rescaled axes' against the view's heading, forward
//! along it and left at its +90° (AG.8).

/// A stick as the game holds it before its dead zone: x −1 full left, y +1
/// full forward ((byte − 128)/128 and (128 − byte)/128, AG.8).
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Stick {
    pub x: f32,
    pub y: f32,
}

/// A quarter of an axis's travel reads as centred: 32 of the 128 steps.
pub const DEAD_ZONE: f32 = 0.25;

/// What the stick asks of the hero: how hard (0 when it's centred) and which
/// way across the ground, a heading in degrees (θ faces (sin θ, 0, cos θ)).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Push {
    pub strength: f32,
    pub heading: f32,
}

impl Push {
    pub const NONE: Push = Push {
        strength: 0.0,
        heading: 0.0,
    };

    pub fn held(&self) -> bool {
        self.strength > 0.0
    }
}

fn axis(a: f32) -> f32 {
    if a.abs() <= DEAD_ZONE {
        0.0
    } else {
        a.signum() * (a.abs() - DEAD_ZONE) / (1.0 - DEAD_ZONE)
    }
}

impl Stick {
    pub const CENTRED: Stick = Stick { x: 0.0, y: 0.0 };

    pub fn new(x: f32, y: f32) -> Stick {
        Stick { x, y }
    }

    /// From a pad's bytes, as the game reads them (AG.8: (x − 128)/128 and
    /// (128 − y)/128, so 255 is 0.992 of full).
    pub fn from_bytes(x: u8, y: u8) -> Stick {
        Stick {
            x: (f32::from(x) - 128.0) / 128.0,
            y: (128.0 - f32::from(y)) / 128.0,
        }
    }

    /// The push against a view looking along `view_heading` degrees across
    /// the ground.
    pub fn push(&self, view_heading: f32) -> Push {
        let (x, y) = (axis(self.x), axis(self.y));
        let strength = x.hypot(y).min(1.0);
        if strength <= 0.0 {
            return Push::NONE;
        }
        Push {
            strength,
            heading: view_heading + (-x).atan2(y).to_degrees(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_quarter_is_centred_and_one_step_past_it_creeps_at_a_96th() {
        // pad-input's A stick resting off centre and Just past the dead zone.
        assert_eq!(Stick::from_bytes(128 + 32, 128).push(15.0), Push::NONE);
        assert_eq!(Stick::from_bytes(128, 128 - 32).push(15.0), Push::NONE);
        assert_eq!(Stick::new(0.2, -0.2).push(15.0), Push::NONE);
        let creep = Stick::from_bytes(128 + 33, 128).push(15.0);
        assert!((creep.strength - 1.0 / 96.0).abs() < 1e-6, "{creep:?}");
    }

    #[test]
    fn forward_is_the_view_and_left_its_plus_90() {
        // AF.9 and AG.8: forward along 15° at the start, left along 105°.
        let forward = Stick::new(0.0, 1.0).push(15.0);
        assert_eq!(forward.strength, 1.0);
        assert!((forward.heading - 15.0).abs() < 1e-5);
        let left = Stick::new(-1.0, 0.0).push(15.0);
        assert!((left.heading - 105.0).abs() < 1e-5, "{left:?}");
        let right = Stick::new(1.0, 0.0).push(15.0);
        assert!((right.heading + 75.0).abs() < 1e-5, "{right:?}");
    }

    #[test]
    fn both_axes_full_is_full_and_between_the_axes_bends_toward_the_nearer() {
        // pad-input's Full on both axes and Between the axes (bytes 177, 10).
        let both = Stick::new(1.0, 1.0).push(0.0);
        assert_eq!(both.strength, 1.0);
        let between = Stick::from_bytes(177, 10).push(0.0);
        assert!((between.strength - 0.9132).abs() < 1e-4, "{between:?}");
        assert!((between.heading + 11.18).abs() < 0.01, "{between:?}");
    }
}
