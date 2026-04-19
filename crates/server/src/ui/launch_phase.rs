//! Launcher start-of-life phasing (PLAN 4.19.2a).
//!
//! Spec `docs/aesthetic/navigation.md` §3.1 state 1 calls for a brief
//! "Startup" beat — calm starfield, no clouds (irisRadius=0), no QR —
//! that transitions into the QR-bearing Awaiting Connect surface. The
//! launcher used to skip this and pop straight into the QR view; the
//! pop felt unintentional. This module derives the current phase from
//! time-since-mount + activity state and hands the renderer (a) what
//! `iris_radius` to feed the vortex and (b) whether to show the main
//! Awaiting-Connect content yet.
//!
//! The transition into "Booting" / "Compiling Shaders" / etc. is left to
//! 4.19.2 / 4.19.4. This module's only job is the very first ~1.6s of
//! the launcher's life.

/// How long the calm starfield stays on its own before the transition
/// begins. Long enough to register as "the launcher woke up," short
/// enough to not feel like a splash screen the user has to skip.
///
/// **Bumped to 5.0 for 4.19.2a validation (2026-04-19)** — gives the
/// user enough wall-clock to actually observe the calm-starfield beat
/// on the HTPC. Bring back down (~1.0) before release.
const STARTUP_HOLD_S: f32 = 5.0;

/// How long the iris-close + content-reveal transition runs. Picked to
/// feel like a deliberate sweep — too fast reads as a snap, too slow
/// outstays its welcome before the actual QR work begins.
const STARTUP_TRANSITION_S: f32 = 0.6;

/// `iris_radius` value the launcher parks at once steady-state. Matches
/// `VortexParams::default().iris_radius`; duplicated as a const here so
/// the interpolation math stays self-contained.
const IRIS_CLOSED: f32 = 1.2;

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum LaunchPhase {
    /// State 1: calm starfield, vortex iris hidden, no QR / heading. The
    /// "launcher woke up" beat.
    Startup,
    /// In-between: vortex iris closes (clouds spiral in from invisible
    /// to closed) and the main content snaps in at the start of the
    /// window so the clouds frame it. `progress` is `0.0..=1.0`.
    Transitioning { progress: f32 },
    /// Steady state: vortex closed, main Awaiting-Connect content shown.
    /// Becomes the de-facto "Awaiting Connect" surface until something
    /// explicit (Crashed, Farewell, in-game transparent) overrides.
    AwaitingConnect,
}

impl LaunchPhase {
    /// Compute the current phase from (a) time since the launcher mounted
    /// and (b) whether the launcher is observing any activity.
    ///
    /// `has_activity` skips the startup reveal entirely. Triggers:
    ///
    ///   - RPCS3 already running (the launcher is being re-shown after a
    ///     game ends, not freshly woken).
    ///   - One or more phones already connected (server restarted while
    ///     a phone's WS reconnect loop was pending — not a fresh boot
    ///     from the user's perspective).
    ///
    /// In both cases a dramatic startup beat would feel off; jumping
    /// straight to the steady surface is the right answer.
    pub(crate) fn compute(elapsed_s: f32, has_activity: bool) -> Self {
        if has_activity {
            return Self::AwaitingConnect;
        }
        if elapsed_s < STARTUP_HOLD_S {
            return Self::Startup;
        }
        // Compare against the absolute transition-end timestamp instead
        // of `(elapsed - hold) < transition_duration`. The latter loses
        // precision via f32 subtraction at non-trivial hold values
        // (e.g. STARTUP_HOLD_S=5.0 + 0.6 then minus 5.0 ≠ exactly 0.6),
        // which made the boundary fall on the wrong side. Keeping the
        // numbers in absolute terms avoids the round-trip.
        let transition_end = STARTUP_HOLD_S + STARTUP_TRANSITION_S;
        if elapsed_s >= transition_end {
            return Self::AwaitingConnect;
        }
        let progress = ((elapsed_s - STARTUP_HOLD_S) / STARTUP_TRANSITION_S).clamp(0.0, 1.0);
        Self::Transitioning { progress }
    }

    /// `iris_radius` the vortex should render with for this phase.
    /// Startup hides clouds entirely; the transition ramps them up
    /// linearly; Awaiting Connect parks at the standard closed value.
    pub(crate) fn iris_radius(self) -> f32 {
        match self {
            Self::Startup => 0.0,
            Self::Transitioning { progress } => IRIS_CLOSED * progress,
            Self::AwaitingConnect => IRIS_CLOSED,
        }
    }

    /// Whether the main content (heading + QR + status strip) should
    /// render this frame. Hidden during Startup so the calm starfield
    /// stands alone; revealed at the start of the transition so the
    /// clouds spiral in *around* the content rather than after it.
    pub(crate) fn shows_main_content(self) -> bool {
        !matches!(self, Self::Startup)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Eyeballed approx-equal — the phase math is straightforward but
    /// f32 boundary arithmetic shouldn't deserve full assert_eq! noise.
    fn approx(a: f32, b: f32) -> bool {
        (a - b).abs() < 1e-5
    }

    #[test]
    fn fresh_boot_starts_in_startup() {
        assert_eq!(LaunchPhase::compute(0.0, false), LaunchPhase::Startup);
        assert_eq!(LaunchPhase::compute(0.5, false), LaunchPhase::Startup);
    }

    #[test]
    fn startup_hold_boundary_enters_transition() {
        // Exactly at STARTUP_HOLD the phase flips to Transitioning at
        // progress 0.0 — no skipped frame. The clamp is belt + braces;
        // floating-point at the boundary should land exactly on 0.
        match LaunchPhase::compute(STARTUP_HOLD_S, false) {
            LaunchPhase::Transitioning { progress } => assert!(approx(progress, 0.0)),
            other => panic!("expected Transitioning, got {other:?}"),
        }
    }

    #[test]
    fn transition_progress_interpolates_linearly() {
        // Halfway through the transition window.
        let mid = STARTUP_HOLD_S + STARTUP_TRANSITION_S * 0.5;
        match LaunchPhase::compute(mid, false) {
            LaunchPhase::Transitioning { progress } => assert!(approx(progress, 0.5)),
            other => panic!("expected Transitioning, got {other:?}"),
        }
    }

    #[test]
    fn transition_end_lands_in_awaiting_connect() {
        let end = STARTUP_HOLD_S + STARTUP_TRANSITION_S;
        assert_eq!(LaunchPhase::compute(end, false), LaunchPhase::AwaitingConnect);
        assert_eq!(
            LaunchPhase::compute(end + 100.0, false),
            LaunchPhase::AwaitingConnect
        );
    }

    #[test]
    fn activity_short_circuits_to_awaiting_connect() {
        // Even at t=0, if anything's already happening, skip the reveal.
        // This covers the "server restart while a phone was reconnecting"
        // scenario — the user isn't waking the launcher fresh, so a
        // dramatic intro would feel off.
        assert_eq!(LaunchPhase::compute(0.0, true), LaunchPhase::AwaitingConnect);
        assert_eq!(LaunchPhase::compute(0.5, true), LaunchPhase::AwaitingConnect);
    }

    #[test]
    fn iris_radius_progresses_with_phase() {
        // Hidden → ramping → closed.
        assert!(approx(LaunchPhase::Startup.iris_radius(), 0.0));
        assert!(approx(
            LaunchPhase::Transitioning { progress: 0.0 }.iris_radius(),
            0.0
        ));
        assert!(approx(
            LaunchPhase::Transitioning { progress: 0.5 }.iris_radius(),
            IRIS_CLOSED * 0.5
        ));
        assert!(approx(
            LaunchPhase::Transitioning { progress: 1.0 }.iris_radius(),
            IRIS_CLOSED
        ));
        assert!(approx(LaunchPhase::AwaitingConnect.iris_radius(), IRIS_CLOSED));
    }

    /// Iris radius is monotonic non-decreasing across the whole life
    /// of the startup sequence. No frame should see clouds *recede* on
    /// the way in — a regression like that would manifest as a flicker.
    #[test]
    fn iris_radius_is_monotonic_non_decreasing_across_startup() {
        let mut prev = -1.0;
        let mut t = 0.0;
        while t <= STARTUP_HOLD_S + STARTUP_TRANSITION_S + 0.1 {
            let now = LaunchPhase::compute(t, false).iris_radius();
            assert!(
                now >= prev - 1e-5,
                "iris radius dropped at t={t}: prev={prev}, now={now}"
            );
            prev = now;
            t += 0.05;
        }
    }

    #[test]
    fn main_content_hidden_only_in_startup() {
        assert!(!LaunchPhase::Startup.shows_main_content());
        assert!(LaunchPhase::Transitioning { progress: 0.0 }.shows_main_content());
        assert!(LaunchPhase::Transitioning { progress: 1.0 }.shows_main_content());
        assert!(LaunchPhase::AwaitingConnect.shows_main_content());
    }
}
