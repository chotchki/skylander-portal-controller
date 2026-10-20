//! The game's clock against the display's (openspec AG.16, design decision 5;
//! hero-movement's The same walk at any frame rate): the hero and the camera
//! step at a fixed 1/60 s of game time, as the original does at full speed,
//! however often the TV draws, and what's drawn between two steps is
//! interpolated by [`FixedClock::alpha`]. guacamole's `FixedUpdate` is this
//! clock; it lives here so the headless runner and the tests share it.

/// Display time in, whole game steps out: at 1/60 s, the original's
/// nominal frame ([`crate::tuning::NOMINAL_STEP`]), kept in f64 so a
/// display's frames sum to whole steps.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FixedClock {
    step: f64,
    owed: f64,
}

/// Display time under this short of a whole step still makes one (f64
/// rounding of a sum of 1/144 s frames).
const EPSILON: f64 = 1e-9;

impl Default for FixedClock {
    fn default() -> Self {
        Self::new(1.0 / 60.0)
    }
}

impl FixedClock {
    pub fn new(step: f64) -> FixedClock {
        FixedClock { step, owed: 0.0 }
    }

    pub fn step(&self) -> f32 {
        self.step as f32
    }

    /// Takes a display frame's time and returns how many game steps are due.
    pub fn advance(&mut self, frame: f64) -> u32 {
        self.owed += frame.max(0.0);
        let mut due = 0;
        while self.owed + EPSILON >= self.step {
            self.owed -= self.step;
            due += 1;
        }
        self.owed = self.owed.max(0.0);
        due
    }

    /// How far the display is into the next step, 0 to 1.
    pub fn alpha(&self) -> f32 {
        (self.owed / self.step).clamp(0.0, 1.0) as f32
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_display_rate_owes_the_same_steps() {
        for rate in [30.0, 60.0, 120.0, 144.0] {
            let mut clock = FixedClock::default();
            let steps: u32 = (0..(rate as usize * 2))
                .map(|_| clock.advance(1.0 / rate))
                .sum();
            assert_eq!(steps, 120, "{rate} Hz");
        }
        let mut clock = FixedClock::default();
        assert_eq!(clock.advance(0.025), 1);
        assert!((clock.alpha() - 0.5).abs() < 1e-5);
    }
}
