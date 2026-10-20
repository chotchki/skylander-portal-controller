//! The stall detector the headless runs use (design decision 3): a value
//! cell can stall a script that waits for the engine to animate it, so a
//! whitelisted placement whose behaviour the oracle's trace shows advancing
//! must leave it within that behaviour's longest recorded stay.

use std::collections::HashMap;

/// The longest stay the oracle recorded for a placement in a behaviour.
#[derive(Debug, Clone, PartialEq)]
pub struct StallLimit {
    pub placement: String,
    /// As [`crate::Vm::behaviour`] names it: lowercased.
    pub behaviour: String,
    /// Game seconds.
    pub longest_stay: f32,
}

/// A placement past its behaviour's longest recorded stay.
#[derive(Debug, Clone, PartialEq)]
pub struct Stall {
    pub placement: String,
    pub behaviour: String,
    /// Game seconds in the behaviour when it fired.
    pub stayed: f32,
    pub longest_stay: f32,
}

/// Watches each placement's behaviour over game time against the limits;
/// a behaviour with no limit is never watched.
#[derive(Debug, Clone, Default)]
pub struct StallDetector {
    limits: HashMap<(String, String), f32>,
    /// Per placement: its behaviour, when it entered it, and whether this
    /// stay has fired already.
    seen: HashMap<String, (Option<String>, f32, bool)>,
}

impl StallDetector {
    pub fn new(limits: impl IntoIterator<Item = StallLimit>) -> Self {
        Self {
            limits: limits
                .into_iter()
                .map(|l| ((l.placement, l.behaviour), l.longest_stay))
                .collect(),
            seen: HashMap::new(),
        }
    }

    /// Records `placement` in `behaviour` at game time `now` (seconds);
    /// returns a stall the first time a stay passes its limit.
    pub fn observe(&mut self, now: f32, placement: &str, behaviour: Option<&str>) -> Option<Stall> {
        let entry = self
            .seen
            .entry(placement.to_owned())
            .or_insert((None, now, false));
        if entry.0.as_deref() != behaviour {
            *entry = (behaviour.map(str::to_owned), now, false);
            return None;
        }
        let behaviour = behaviour?;
        let limit = *self
            .limits
            .get(&(placement.to_owned(), behaviour.to_owned()))?;
        let stayed = now - entry.1;
        if stayed > limit && !entry.2 {
            entry.2 = true;
            return Some(Stall {
                placement: placement.to_owned(),
                behaviour: behaviour.to_owned(),
                stayed,
                longest_stay: limit,
            });
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn detector() -> StallDetector {
        StallDetector::new([StallLimit {
            placement: "Enemy_Chompy(1)".into(),
            behaviour: "windup".into(),
            longest_stay: 0.5,
        }])
    }

    #[test]
    fn fires_on_a_stalled_behaviour_once() {
        let mut d = detector();
        let mut fired = Vec::new();
        for frame in 0..120 {
            let t = frame as f32 / 60.0;
            if let Some(s) = d.observe(t, "Enemy_Chompy(1)", Some("windup")) {
                fired.push((frame, s));
            }
        }
        assert_eq!(fired.len(), 1, "{fired:?}");
        let (frame, s) = &fired[0];
        assert_eq!(*frame, 31, "the first frame past half a second");
        assert!(s.stayed > 0.5 && s.longest_stay == 0.5);
    }

    #[test]
    fn stays_quiet_on_a_moving_behaviour() {
        let mut d = detector();
        // Windup for 0.4 s, lunge for 0.3 s, back to windup for 0.4 s: no
        // stay passes its limit, and lunge has none.
        let plan = [
            ("windup", 24),
            ("lunge", 18),
            ("windup", 24),
            ("recovery", 60),
        ];
        let mut frame = 0;
        for (behaviour, frames) in plan {
            for _ in 0..frames {
                let t = frame as f32 / 60.0;
                assert_eq!(d.observe(t, "Enemy_Chompy(1)", Some(behaviour)), None);
                frame += 1;
            }
        }
        // Another placement in windup has no limit of its own.
        for f in 0..120 {
            assert_eq!(
                d.observe(f as f32 / 60.0, "Enemy_Chompy(2)", Some("windup")),
                None
            );
        }
    }
}
