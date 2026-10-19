//! The TV's motion (openspec AP, design decision 9): §5's easing tokens and
//! durations from `docs/aesthetic/design_language.md`, a [`Tween`] that eases
//! a value along one of them, and [`ReducedMotion`], which the starfield's
//! drift, the choices' focus look (`look`) and the portal's cards
//! (`card_motion`: the rise, the impact and the float) read.

use std::time::Duration;

use bevy::curve::CubicSegment;
use bevy::prelude::*;

/// Ambient motion off (the starfield's drift, the cards' idle float; later
/// the halo spin) and entrances at their end state, so a shot of a screen comes
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

/// `--dur-quick`: a state's fade, and the bezel's picking pop.
pub(crate) const DUR_QUICK: Duration = Duration::from_millis(200);

/// `--dur-tap`: a button's press.
pub(crate) const DUR_TAP: Duration = Duration::from_millis(120);

/// `--dur-impact`: a placement's flash on its bezel (§5.4).
pub(crate) const DUR_IMPACT: Duration = Duration::from_millis(600);

/// `--dur-idle-float`: one breath of a loaded bezel (§5.3).
pub(crate) const DUR_IDLE_FLOAT: Duration = Duration::from_millis(4500);

/// §5's curves, each a CSS `cubic-bezier()` through `bevy_curve`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Easing {
    /// `--ease-spring`, `cubic-bezier(.3, 1.3, .5, 1)`: entrances and the
    /// focus pop; runs about 3% past its end before settling.
    Spring,
    /// `--ease-tap`, `cubic-bezier(.4, 2.2, .5, 1)`: a press; runs about a
    /// third past its end before settling.
    Tap,
    /// CSS's `ease-out`, `cubic-bezier(0, 0, .58, 1)`: the phone's portal
    /// impact (`gold_bezel.css`); fast off the mark, never past its end.
    Out,
    /// CSS's `ease-in-out`, `cubic-bezier(.42, 0, .58, 1)`: the phone's idle
    /// float; slow at both ends, never past either.
    InOut,
}

impl Easing {
    fn curve(self) -> CubicSegment<Vec2> {
        match self {
            Self::Spring => CubicSegment::new_bezier_easing((0.3, 1.3), (0.5, 1.0)),
            Self::Tap => CubicSegment::new_bezier_easing((0.4, 2.2), (0.5, 1.0)),
            Self::Out => CubicSegment::new_bezier_easing((0.0, 0.0), (0.58, 1.0)),
            Self::InOut => CubicSegment::new_bezier_easing((0.42, 0.0), (0.58, 1.0)),
        }
    }

    /// How far along the change is at `t` of its time (0 to 1, clamped): 0
    /// at the start, 1 at the end, past 1 on the way.
    pub(crate) fn ease(self, t: f32) -> f32 {
        match t {
            t if t <= 0.0 => 0.0,
            t if t >= 1.0 => 1.0,
            t => self.curve().ease(t),
        }
    }
}

/// A value between 0 (rest) and 1 (on), easing to whichever it's aimed at
/// over a fixed time. A new aim starts from wherever the value is, overshoot
/// included, so a focus that leaves mid-pop eases back without a jump.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Tween {
    easing: Easing,
    over: Duration,
    from: f32,
    to: f32,
    since: Duration,
}

impl Tween {
    /// At rest.
    pub(crate) const fn new(easing: Easing, over: Duration) -> Self {
        Self {
            easing,
            over,
            from: 0.0,
            to: 0.0,
            since: Duration::ZERO,
        }
    }

    /// The value at `now`, on the same clock as [`Tween::aim`]'s.
    pub(crate) fn at(&self, now: Duration) -> f32 {
        let t = now.saturating_sub(self.since).as_secs_f32() / self.over.as_secs_f32();
        if t >= 1.0 {
            return self.to;
        }
        self.from + (self.to - self.from) * self.easing.ease(t)
    }

    /// Ease toward `to` from `now`; true when that's a new aim.
    pub(crate) fn aim(&mut self, to: f32, now: Duration) -> bool {
        if to == self.to {
            return false;
        }
        self.from = self.at(now);
        self.to = to;
        self.since = now;
        true
    }

    /// At its aim now, with no easing left (`ReducedMotion`).
    pub(crate) fn settle(&mut self) {
        self.from = self.to;
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

    /// The highest point a curve reaches, and where, sampled every 0.001.
    fn peak(easing: Easing) -> (f32, f32) {
        (0..=1000)
            .map(|i| i as f32 / 1000.0)
            .map(|t| (t, easing.ease(t)))
            .fold(
                (0.0, f32::MIN),
                |best, (t, v)| {
                    if v > best.1 { (t, v) } else { best }
                },
            )
    }

    #[test]
    fn each_curve_starts_at_0_overshoots_and_ends_at_1() {
        for easing in [Easing::Spring, Easing::Tap] {
            assert_eq!(easing.ease(0.0), 0.0, "{easing:?}");
            assert_eq!(easing.ease(1.0), 1.0, "{easing:?}");
            assert_eq!(easing.ease(-0.5), 0.0, "{easing:?} before its start");
            assert_eq!(easing.ease(1.5), 1.0, "{easing:?} after its end");
        }
        // cubic-bezier(.3, 1.3, .5, 1) tops out near 1.03 at about 62% of its
        // time; (.4, 2.2, .5, 1) near 1.33 at about 45%.
        let (at, top) = peak(Easing::Spring);
        assert!((1.025..1.035).contains(&top), "spring peaks at {top}");
        assert!((0.55..0.70).contains(&at), "spring peaks at t {at}");
        let (at, top) = peak(Easing::Tap);
        assert!((1.30..1.35).contains(&top), "tap peaks at {top}");
        assert!((0.38..0.52).contains(&at), "tap peaks at t {at}");
        // Both leave 0 going up: no dip under the start.
        for easing in [Easing::Spring, Easing::Tap] {
            assert!(
                (1..100).all(|i| easing.ease(i as f32 / 1000.0) > 0.0),
                "{easing:?}"
            );
        }
    }

    #[test]
    fn ease_out_and_ease_in_out_never_overshoot() {
        let samples = |easing: Easing| -> Vec<f32> {
            (0..=1000).map(|i| easing.ease(i as f32 / 1000.0)).collect()
        };
        for easing in [Easing::Out, Easing::InOut] {
            let s = samples(easing);
            assert_eq!((s[0], s[1000]), (0.0, 1.0), "{easing:?}");
            assert!(s.windows(2).all(|w| w[0] <= w[1]), "{easing:?} only rises");
            assert!(s.iter().all(|v| (0.0..=1.0).contains(v)), "{easing:?}");
        }
        // ease-out is ahead of the clock all the way; ease-in-out behind it
        // in the first half, ahead in the second, half way at half time.
        let (out, in_out) = (samples(Easing::Out), samples(Easing::InOut));
        assert!((1..1000).all(|i| out[i] > i as f32 / 1000.0));
        assert!(out[100] > 0.15, "fast off the mark: {}", out[100]);
        assert!((in_out[500] - 0.5).abs() < 1e-3, "{}", in_out[500]);
        assert!((1..500).all(|i| in_out[i] < i as f32 / 1000.0));
        assert!((501..1000).all(|i| in_out[i] > i as f32 / 1000.0));
        for i in 1..500 {
            assert!(
                (in_out[i] + in_out[1000 - i] - 1.0).abs() < 1e-3,
                "symmetric at {i}"
            );
        }
    }

    const MS: Duration = Duration::from_millis(1);

    #[test]
    fn a_tween_eases_to_its_aim_and_stays() {
        let mut tween = Tween::new(Easing::Spring, DUR_QUICK);
        assert_eq!(tween.at(Duration::ZERO), 0.0, "at rest");
        assert_eq!(tween.at(10 * MS), 0.0, "at rest");
        assert!(tween.aim(1.0, 1000 * MS));
        assert!(
            !tween.aim(1.0, 1010 * MS),
            "the same aim again is nothing new"
        );
        assert_eq!(tween.at(1000 * MS), 0.0, "starts where it was");
        let mid = tween.at(1100 * MS);
        assert!(0.0 < mid && mid < 1.04, "{mid}");
        let top = (1000..1200)
            .map(|ms| tween.at(ms * MS))
            .fold(f32::MIN, f32::max);
        assert!(top > 1.02, "overshoots: {top}");
        assert_eq!(tween.at(1200 * MS), 1.0, "ends on its aim");
        assert_eq!(tween.at(9000 * MS), 1.0);
    }

    #[test]
    fn a_new_aim_starts_from_where_the_value_is() {
        let mut tween = Tween::new(Easing::Tap, DUR_TAP);
        tween.aim(1.0, Duration::ZERO);
        // Let go mid-dip, at its deepest: the release starts there.
        let then = tween.at(50 * MS);
        assert!(then > 1.0, "{then}");
        assert!(tween.aim(0.0, 50 * MS));
        assert_eq!(tween.at(50 * MS), then, "no jump");
        assert!(tween.at(51 * MS) > 1.0, "no jump");
        assert_eq!(tween.at(170 * MS), 0.0);
    }

    #[test]
    fn a_settled_tween_is_at_its_aim_at_once() {
        let mut tween = Tween::new(Easing::Spring, DUR_QUICK);
        tween.aim(1.0, 500 * MS);
        tween.settle();
        assert_eq!(tween.at(500 * MS), 1.0);
        assert_eq!(tween.at(501 * MS), 1.0);
        tween.aim(0.0, 600 * MS);
        tween.settle();
        assert_eq!(tween.at(600 * MS), 0.0);
    }
}
