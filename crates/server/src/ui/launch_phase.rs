//! Launcher phase + animation choreography (PLAN 4.19.2a + intro/close
//! transitions).
//!
//! Phases the launcher cycles through:
//!
//!   - **IntroTransitioning** — vortex iris reveals from centre, badge
//!     spins in like a coin tipping flat, text fades in late so it
//!     isn't illegible mid-rotation. Begins at app mount: before
//!     2026-04-24 there was a 1s "calm starfield only" Startup beat
//!     with a separate heraldic "STARTING" title; that title folded
//!     into the card's `BackFace::Starting` so the intro now begins
//!     immediately and the user sees one continuous spin-up.
//!   - **AwaitingConnect** — steady state. Vortex parked at 1.5, badge
//!     face-on, text full opacity.
//!   - **ClosingToInGame** — triggered when RPCS3 starts. Reverse
//!     choreography: text fades first (badge goes blank), badge spins
//!     edge-on while alpha fades, then dark-hole iris accelerates to
//!     cover the screen. Once `progress` hits 1.0 the dispatcher flips
//!     to `in_game::render` and the transparent panel reveals RPCS3.
//!
//! All four animation outputs (`iris_radius`, `badge_scale`,
//! `badge_alpha`, `badge_text_alpha`) are derived from the same
//! `progress` value per phase so timing offsets between them stay in
//! one place — adjust the windows here and the renderer picks it up
//! without further edits.

const INTRO_TRANSITION_S: f32 = 1.8;
const CLOSE_TRANSITION_S: f32 = 1.0;

/// Fraction of intro reserved for the "calm starfield only" hold
/// before the badge starts animating in. Keeps the iris-reveal
/// beat from racing the badge spin.
const STARTUP_HOLD_FRACTION: f32 = 0.20;

/// Fraction of intro reserved for the quick alpha fade-in after
/// the startup hold (PLAN 10.7.6). Disc is at edge-on / 10% size
/// during this window, alpha smoothsteps 0 → 1; spin begins
/// once it concludes.
const FADE_IN_FRACTION: f32 = 0.05;

/// How many full Y-axis turns the badge spins through on
/// intro/close (PLAN 10.7.6). 3 full revolutions + the 90° lap to
/// or from face-on. Picked to read as a definite "spinning coin"
/// gesture rather than a single tip.
const SPIN_TURNS: f32 = 3.0;

/// Minimum geometry scale during intro fade-in / close fade-out.
/// Disc starts/ends at this size (a small but visible coin) and
/// grows to / shrinks from 1.0 over the spin window.
const SCALE_MIN_3D: f32 = 0.10;

/// `iris_radius` value the launcher parks at once steady-state. Bumped
/// 1.2 → 1.5 on 2026-04-19 after the vortex shader spike settled here
/// as the "fills past the screen edges" value Chris was happy with.
pub(crate) const IRIS_FULL: f32 = 1.5;

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum LaunchPhase {
    IntroTransitioning {
        progress: f32,
    },
    AwaitingConnect,
    ClosingToInGame {
        progress: f32,
    },
    /// Returning to the launcher after an in-game session ended. Plays
    /// the same iris-reveal + badge spin-in curves as
    /// IntroTransitioning. Driven by the dispatcher's
    /// `returning_from_game_at` timestamp.
    ReturnFromGame {
        progress: f32,
    },
}

// Re-export so callers can `use launch_phase::IrisMode` without
// reaching into vortex; the canonical definition lives there because
// the vortex shader consumes it.
pub(crate) use crate::vortex::IrisMode;

impl LaunchPhase {
    /// Derive the current phase from elapsed-time inputs.
    ///
    /// `elapsed_s` is the launcher's mount age (drives intro).
    /// `closing_elapsed_s` is `Some` once a close has been triggered
    /// (RPCS3 transitioned from not-running to running while on the
    /// Main screen). Close takes priority over intro — once it's in
    /// flight the intro state is irrelevant. `has_activity` short-
    /// circuits the intro for re-mounts where a phone is already
    /// connected.
    pub(crate) fn compute(
        elapsed_s: f32,
        closing_elapsed_s: Option<f32>,
        returning_elapsed_s: Option<f32>,
        has_activity: bool,
    ) -> Self {
        if let Some(close) = closing_elapsed_s {
            let progress = (close / CLOSE_TRANSITION_S).clamp(0.0, 1.0);
            return Self::ClosingToInGame { progress };
        }
        if let Some(returning) = returning_elapsed_s {
            let progress = (returning / INTRO_TRANSITION_S).clamp(0.0, 1.0);
            if progress >= 1.0 {
                return Self::AwaitingConnect;
            }
            return Self::ReturnFromGame { progress };
        }
        if has_activity {
            return Self::AwaitingConnect;
        }
        if elapsed_s >= INTRO_TRANSITION_S {
            return Self::AwaitingConnect;
        }
        let progress = (elapsed_s / INTRO_TRANSITION_S).clamp(0.0, 1.0);
        Self::IntroTransitioning { progress }
    }

    /// Iris radius the vortex should render with this frame.
    pub(crate) fn iris_radius(self) -> f32 {
        match self {
            Self::IntroTransitioning { progress } | Self::ReturnFromGame { progress } => {
                // Ease-out cubic — fast at first, gentle landing.
                IRIS_FULL * ease_out_cubic(progress)
            }
            Self::AwaitingConnect => IRIS_FULL,
            Self::ClosingToInGame { progress } => {
                // Iris growth lags the badge by 20% so the focal
                // element (badge) starts moving before the screen-
                // wide iris dominates attention.
                let p = ((progress - 0.2) / 0.8).clamp(0.0, 1.0);
                IRIS_FULL * ease_in_cubic(p)
            }
        }
    }

    /// Which side of the iris boundary is opaque. Reveal grows the
    /// visible region; DarkHole grows the hidden region.
    pub(crate) fn iris_mode(self) -> IrisMode {
        match self {
            Self::ClosingToInGame { .. } => IrisMode::DarkHole,
            _ => IrisMode::Reveal,
        }
    }

    /// Y-axis rotation (radians) for the 3D badge under PLAN 10.7.
    /// 0 = face-on (front face directly toward camera), π/2 =
    /// edge-on; rotations of 2π are full coin-spins.
    ///
    /// Intro / return: edge-on → 3 full turns + the lap to face-on
    /// (3.25 turns total). Decelerating ease-out so the spin
    /// slows into its landing rather than jamming to face-on at
    /// terminal velocity. ChrisCheck 2026-05-02: pure linear
    /// 1-rotation rotation "looked good" in motion but lost the
    /// "physical object materialising" beat the legacy 2D
    /// `badge_scale` sine provided — combining multi-turn
    /// rotation with `badge_scale_3d`'s 0.1 → 1.0 growth restores
    /// it.
    ///
    /// Close: mirror — 3 turns + 90° from face-on out to edge-on
    /// over the first 60% of close, then locked at edge-on for
    /// the alpha fade.
    pub(crate) fn badge_rotation_y(self) -> f32 {
        use std::f32::consts::{FRAC_PI_2, PI};
        // Total angular sweep: SPIN_TURNS full revolutions + the
        // 90° lap that gets the disc from edge-on to face-on (or
        // the other way for close).
        const SWEEP: f32 = SPIN_TURNS * 2.0 * PI + FRAC_PI_2;
        match self {
            Self::IntroTransitioning { progress } | Self::ReturnFromGame { progress } => {
                let spin_start = STARTUP_HOLD_FRACTION + FADE_IN_FRACTION;
                let p = ((progress - spin_start) / (1.0 - spin_start)).clamp(0.0, 1.0);
                let eased = ease_out_cubic(p);
                FRAC_PI_2 - SWEEP * eased
            }
            Self::AwaitingConnect => 0.0,
            Self::ClosingToInGame { progress } => {
                let p = (progress / 0.6).clamp(0.0, 1.0);
                let eased = ease_out_cubic(p);
                -SWEEP * eased
            }
        }
    }

    /// Geometry scale for the 3D badge (PLAN 10.7.6). 1.0 = full
    /// size, 0.1 = 10% size (the materialise-from-nothing pose).
    /// Smoothstep-eased growth over the spin window so the disc
    /// inflates from 10% to full while it's also rotating — the
    /// "physical object appearing" feel that the 2D `badge_scale`
    /// got from a sine on the X axis. Decoupled from rotation so
    /// the cosine cycles of multi-turn spinning don't pulse the
    /// envelope.
    pub(crate) fn badge_scale_3d(self) -> f32 {
        match self {
            Self::IntroTransitioning { progress } | Self::ReturnFromGame { progress } => {
                let spin_start = STARTUP_HOLD_FRACTION + FADE_IN_FRACTION;
                let p = ((progress - spin_start) / (1.0 - spin_start)).clamp(0.0, 1.0);
                let eased = smoothstep(p);
                SCALE_MIN_3D + eased * (1.0 - SCALE_MIN_3D)
            }
            Self::AwaitingConnect => 1.0,
            Self::ClosingToInGame { progress } => {
                let p = (progress / 0.6).clamp(0.0, 1.0);
                let eased = smoothstep(1.0 - p);
                SCALE_MIN_3D + eased * (1.0 - SCALE_MIN_3D)
            }
        }
    }

    /// Alpha for the 3D badge under PLAN 10.7.6. Quick fade-in
    /// (smoothstep over `FADE_IN_FRACTION` of intro after the
    /// startup hold) so the disc materialises edge-on at 10% size
    /// before the spin begins; held at 1 throughout the spin (the
    /// growth + rotation supply the visual interest); fades out
    /// over the last 40% of close after the spin parks at
    /// edge-on, so the badge dissolves into the iris-close beat
    /// rather than freezing as a tiny gold sliver.
    pub(crate) fn badge_alpha_3d(self) -> f32 {
        match self {
            Self::IntroTransitioning { progress } | Self::ReturnFromGame { progress } => {
                let p = ((progress - STARTUP_HOLD_FRACTION) / FADE_IN_FRACTION).clamp(0.0, 1.0);
                smoothstep(p)
            }
            Self::AwaitingConnect => 1.0,
            Self::ClosingToInGame { progress } => {
                let p = ((progress - 0.6) / 0.4).clamp(0.0, 1.0);
                1.0 - smoothstep(p)
            }
        }
    }

    /// Alpha for text/QR content inside (or beneath) the badge. Fades
    /// in late during intro so the spin isn't reading illegible
    /// mid-rotation, fades out early during close so the badge spins
    /// out blank.
    pub(crate) fn badge_text_alpha(self) -> f32 {
        match self {
            Self::IntroTransitioning { progress } | Self::ReturnFromGame { progress } => {
                ((progress - 0.5) / 0.5).clamp(0.0, 1.0)
            }
            Self::AwaitingConnect => 1.0,
            Self::ClosingToInGame { progress } => (1.0 - progress / 0.4).clamp(0.0, 1.0),
        }
    }

    /// True once the close transition has fully run. The dispatcher
    /// uses this to flip from rendering Main-with-close-animation to
    /// rendering the in-game surface (which uses a transparent panel
    /// so RPCS3 shows through).
    pub(crate) fn close_complete(self) -> bool {
        matches!(self, Self::ClosingToInGame { progress } if progress >= 1.0)
    }
}

fn ease_out_cubic(t: f32) -> f32 {
    let inv = 1.0 - t;
    1.0 - inv * inv * inv
}

fn ease_in_cubic(t: f32) -> f32 {
    t * t * t
}

fn smoothstep(t: f32) -> f32 {
    let c = t.clamp(0.0, 1.0);
    c * c * (3.0 - 2.0 * c)
}

/// Per-screen entry animation for non-Main surfaces (Crashed,
/// Farewell, ServerError). Drives the same badge spin + content fade
/// the QR card uses during the launcher intro, just gated on
/// per-screen entry time instead of launcher startup.
///
/// Reuses the curve shapes from `LaunchPhase::badge_*` so the visual
/// language is identical — same coin-spin sine, same scale-gate to
/// avoid the thin-line phase, same late text fade-in.
#[derive(Debug, Clone, Copy)]
pub(crate) struct ScreenIntro {
    pub elapsed_s: f32,
}

impl ScreenIntro {
    /// Total duration of the screen-entry animation. Slightly shorter
    /// than the launcher intro because there's no startup hold or
    /// brand-fade hand-off — the badge just needs to land.
    const DURATION_S: f32 = 1.2;

    fn progress(self) -> f32 {
        (self.elapsed_s / Self::DURATION_S).clamp(0.0, 1.0)
    }

    /// Horizontal scale for the centre badge (0 = edge-on, 1 = face-on).
    pub(crate) fn badge_scale(self) -> f32 {
        (self.progress() * std::f32::consts::FRAC_PI_2).sin()
    }

    /// Alpha for the bezel layers. Gated on `badge_scale` via
    /// smoothstep(0.05, 0.25) so the bezel only becomes visible once
    /// the badge has enough width to read as a circle, not as a thin
    /// vertical line.
    pub(crate) fn badge_alpha(self) -> f32 {
        let scale = self.badge_scale();
        let t = ((scale - 0.05) / 0.20).clamp(0.0, 1.0);
        t * t * (3.0 - 2.0 * t)
    }

    /// Alpha for text/content inside the badge. Fades in late so it
    /// isn't readable mid-rotation.
    pub(crate) fn content_alpha(self) -> f32 {
        ((self.progress() - 0.5) / 0.5).clamp(0.0, 1.0)
    }

    /// Vortex iris radius for the screen entry. Used by Crashed coming
    /// from in-game (where the vortex wasn't visible) to reveal the
    /// vortex alongside the badge spin-in. Other screens (Farewell,
    /// ServerError when the vortex is already at full extent) ignore
    /// this and keep their existing iris.
    pub(crate) fn iris_radius(self) -> f32 {
        IRIS_FULL * ease_out_cubic(self.progress())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn approx(a: f32, b: f32) -> bool {
        (a - b).abs() < 1e-5
    }

    #[test]
    fn fresh_boot_starts_in_intro_transitioning() {
        // After the Startup pre-beat was retired (STARTING folded into
        // the card back-face), the app boots straight into the intro
        // spin-up at progress 0.
        match LaunchPhase::compute(0.0, None, None, false) {
            LaunchPhase::IntroTransitioning { progress } => assert!(approx(progress, 0.0)),
            other => panic!("expected IntroTransitioning, got {other:?}"),
        }
    }

    #[test]
    fn intro_progress_interpolates_linearly() {
        let mid = INTRO_TRANSITION_S * 0.5;
        match LaunchPhase::compute(mid, None, None, false) {
            LaunchPhase::IntroTransitioning { progress } => assert!(approx(progress, 0.5)),
            other => panic!("expected IntroTransitioning, got {other:?}"),
        }
    }

    #[test]
    fn intro_end_lands_in_awaiting_connect() {
        assert_eq!(
            LaunchPhase::compute(INTRO_TRANSITION_S, None, None, false),
            LaunchPhase::AwaitingConnect
        );
    }

    #[test]
    fn activity_short_circuits_to_awaiting_connect() {
        assert_eq!(
            LaunchPhase::compute(0.0, None, None, true),
            LaunchPhase::AwaitingConnect
        );
    }

    #[test]
    fn close_overrides_intro() {
        // Close in flight → ClosingToInGame regardless of where intro
        // would have placed us. Startup-time + close = close.
        match LaunchPhase::compute(0.5, Some(0.0), None, false) {
            LaunchPhase::ClosingToInGame { progress } => assert!(approx(progress, 0.0)),
            other => panic!("expected ClosingToInGame, got {other:?}"),
        }
    }

    #[test]
    fn close_progress_clamps_at_one() {
        match LaunchPhase::compute(0.0, Some(CLOSE_TRANSITION_S * 5.0), None, false) {
            LaunchPhase::ClosingToInGame { progress } => assert!(approx(progress, 1.0)),
            other => panic!("expected ClosingToInGame, got {other:?}"),
        }
    }

    #[test]
    fn iris_radius_progresses_with_phase() {
        assert!(approx(
            LaunchPhase::IntroTransitioning { progress: 0.0 }.iris_radius(),
            0.0
        ));
        assert!(approx(
            LaunchPhase::IntroTransitioning { progress: 1.0 }.iris_radius(),
            IRIS_FULL
        ));
        assert!(approx(
            LaunchPhase::AwaitingConnect.iris_radius(),
            IRIS_FULL
        ));
        assert!(approx(
            LaunchPhase::ClosingToInGame { progress: 0.0 }.iris_radius(),
            0.0
        ));
        assert!(approx(
            LaunchPhase::ClosingToInGame { progress: 1.0 }.iris_radius(),
            IRIS_FULL
        ));
    }

    #[test]
    fn iris_mode_flips_during_close() {
        assert_eq!(
            LaunchPhase::IntroTransitioning { progress: 0.5 }.iris_mode(),
            IrisMode::Reveal,
        );
        assert_eq!(LaunchPhase::AwaitingConnect.iris_mode(), IrisMode::Reveal);
        assert_eq!(
            LaunchPhase::ClosingToInGame { progress: 0.5 }.iris_mode(),
            IrisMode::DarkHole
        );
    }

    #[test]
    fn close_complete_only_at_progress_one() {
        assert!(!LaunchPhase::AwaitingConnect.close_complete());
        assert!(!LaunchPhase::ClosingToInGame { progress: 0.5 }.close_complete());
        assert!(!LaunchPhase::ClosingToInGame { progress: 0.99 }.close_complete());
        assert!(LaunchPhase::ClosingToInGame { progress: 1.0 }.close_complete());
    }

    #[test]
    fn badge_alpha_3d_zero_during_startup_hold() {
        // First 20% of intro is the startup hold — badge stays
        // hidden until the iris reveals it.
        assert!(approx(
            LaunchPhase::IntroTransitioning { progress: 0.0 }.badge_alpha_3d(),
            0.0
        ));
        assert!(approx(
            LaunchPhase::IntroTransitioning { progress: 0.20 }.badge_alpha_3d(),
            0.0
        ));
    }

    #[test]
    fn badge_alpha_3d_full_after_fade_in() {
        // Quick fade-in completes at progress = 0.20 + 0.05 = 0.25;
        // alpha should hit 1.0 there and stay there through the spin.
        assert!(approx(
            LaunchPhase::IntroTransitioning { progress: 0.25 }.badge_alpha_3d(),
            1.0
        ));
        assert!(approx(
            LaunchPhase::IntroTransitioning { progress: 0.5 }.badge_alpha_3d(),
            1.0
        ));
        assert!(approx(LaunchPhase::AwaitingConnect.badge_alpha_3d(), 1.0));
    }

    #[test]
    fn badge_alpha_3d_zero_at_close_end() {
        // Spin parks at 60% of close, then alpha fades to 0 over
        // the remaining 40%.
        assert!(approx(
            LaunchPhase::ClosingToInGame { progress: 0.6 }.badge_alpha_3d(),
            1.0
        ));
        assert!(approx(
            LaunchPhase::ClosingToInGame { progress: 1.0 }.badge_alpha_3d(),
            0.0
        ));
    }

    #[test]
    fn badge_scale_3d_starts_small_grows_to_full() {
        // During the startup hold + fade-in, scale stays at the
        // 10% materialise-from-nothing value.
        assert!(approx(
            LaunchPhase::IntroTransitioning { progress: 0.0 }.badge_scale_3d(),
            SCALE_MIN_3D
        ));
        assert!(approx(
            LaunchPhase::IntroTransitioning { progress: 0.25 }.badge_scale_3d(),
            SCALE_MIN_3D
        ));
        // End of intro lands at full size.
        assert!(approx(
            LaunchPhase::IntroTransitioning { progress: 1.0 }.badge_scale_3d(),
            1.0
        ));
        assert!(approx(LaunchPhase::AwaitingConnect.badge_scale_3d(), 1.0));
    }

    #[test]
    fn badge_rotation_y_does_multiple_turns() {
        use std::f32::consts::{FRAC_PI_2, PI};
        // At spin start (progress = 0.25) rotation is the full
        // 3-turn-plus-90° sweep magnitude away from face-on.
        let start = LaunchPhase::IntroTransitioning { progress: 0.25 }.badge_rotation_y();
        assert!(approx(start, FRAC_PI_2));
        // Halfway through the spin (progress = 0.625), the eased
        // sweep has covered ~87.5% of the angular distance — disc
        // is past several turns into the spin (well below the
        // start angle).
        let mid = LaunchPhase::IntroTransitioning { progress: 0.625 }.badge_rotation_y();
        assert!(
            mid < FRAC_PI_2 - 4.0 * PI,
            "expected mid-spin rotation past 2 full turns, got {mid}"
        );
        // End of intro: lands on face-on. The angle is -6π (3 full
        // turns past 0), which is face-on modulo 2π but not
        // numerically zero — assert via the cos/sin pose instead.
        let end = LaunchPhase::IntroTransitioning { progress: 1.0 }.badge_rotation_y();
        assert!(
            approx(end.cos(), 1.0) && approx(end.sin(), 0.0),
            "end pose not face-on: rotation={end} cos={} sin={}",
            end.cos(),
            end.sin()
        );
    }

    #[test]
    fn iris_radius_monotonic_across_intro() {
        let mut prev = -1.0;
        let mut t = 0.0;
        while t <= INTRO_TRANSITION_S + 0.1 {
            let now = LaunchPhase::compute(t, None, None, false).iris_radius();
            assert!(
                now >= prev - 1e-5,
                "iris dropped at t={t}: prev={prev}, now={now}"
            );
            prev = now;
            t += 0.05;
        }
    }
}
