//! Launcher phase + animation choreography (PLAN 4.19.2a + intro/close
//! transitions).
//!
//! Phases the launcher cycles through:
//!
//!   - **Startup** — calm starfield only. No vortex iris, no badge.
//!     Brief hold so the launcher reads as "waking up".
//!   - **IntroTransitioning** — vortex iris reveals from centre, badge
//!     spins in like a coin tipping flat, text fades in late so it
//!     isn't illegible mid-rotation. Heavy overlap (Chris 2026-04-19):
//!     the iris reveal leads, the badge spin starts at ~40%, the text
//!     fade-in starts at ~70%. Reads as one fluid motion rather than
//!     three sequential beats.
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

const STARTUP_HOLD_S: f32 = 1.0;
const INTRO_TRANSITION_S: f32 = 1.8;
const CLOSE_TRANSITION_S: f32 = 1.0;

/// `iris_radius` value the launcher parks at once steady-state. Bumped
/// 1.2 → 1.5 on 2026-04-19 after the vortex shader spike settled here
/// as the "fills past the screen edges" value Chris was happy with.
pub(crate) const IRIS_FULL: f32 = 1.5;

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum LaunchPhase {
    Startup,
    IntroTransitioning { progress: f32 },
    AwaitingConnect,
    ClosingToInGame { progress: f32 },
    /// Returning to the launcher after an in-game session ended. Plays
    /// the same iris-reveal + badge spin-in curves as
    /// IntroTransitioning, but `brand_intro_alpha` stays 0 — the user
    /// already knows the launcher, so re-flashing "STARTING" would be
    /// weird. Driven by the dispatcher's `returning_from_game_at`
    /// timestamp.
    ReturnFromGame { progress: f32 },
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
        if elapsed_s < STARTUP_HOLD_S {
            return Self::Startup;
        }
        let intro_end = STARTUP_HOLD_S + INTRO_TRANSITION_S;
        if elapsed_s >= intro_end {
            return Self::AwaitingConnect;
        }
        let progress =
            ((elapsed_s - STARTUP_HOLD_S) / INTRO_TRANSITION_S).clamp(0.0, 1.0);
        Self::IntroTransitioning { progress }
    }

    /// Iris radius the vortex should render with this frame.
    pub(crate) fn iris_radius(self) -> f32 {
        match self {
            Self::Startup => 0.0,
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

    /// Horizontal scale for the centre badge (QR card / error card).
    /// 0 = edge-on (invisible), 1 = face-on (full size). Sine curve
    /// so the spin reads as a coin tipping flat — slowest near
    /// edge-on where rotation rate looks fastest, fastest near
    /// face-on where it slows into the parked pose.
    pub(crate) fn badge_scale(self) -> f32 {
        use std::f32::consts::FRAC_PI_2;
        match self {
            Self::Startup => 0.0,
            Self::IntroTransitioning { progress } | Self::ReturnFromGame { progress } => {
                // Spin starts 20% into intro, lands at 100%.
                let p = ((progress - 0.2) / 0.8).clamp(0.0, 1.0);
                (p * FRAC_PI_2).sin()
            }
            Self::AwaitingConnect => 1.0,
            Self::ClosingToInGame { progress } => {
                // Spin out across the first 60% of close.
                let p = (progress / 0.6).clamp(0.0, 1.0);
                ((1.0 - p) * FRAC_PI_2).sin()
            }
        }
    }

    /// Alpha for the badge body (bezel layers). Tracks badge_scale
    /// loosely but offset so the bezel can fade independently of the
    /// spin — during close the bezel fades out before the spin hits
    /// edge-on, so the badge dissolves rather than collapsing.
    ///
    /// Multiplied by a scale-gate (smoothstep 0.05 → 0.25) so the
    /// bezel is invisible while it's a thin sliver; without the gate
    /// the spin's early/late "edge-on" phase reads as a vertical
    /// line on screen rather than a circular badge becoming
    /// visible (Chris flagged 2026-04-19). The gate also handles
    /// the close: as the badge spins out and gets thin, its alpha
    /// drops to 0 before it reaches the line-shaped phase.
    pub(crate) fn badge_alpha(self) -> f32 {
        let in_window = match self {
            Self::Startup => 0.0,
            Self::IntroTransitioning { progress } | Self::ReturnFromGame { progress } => {
                ((progress - 0.2) / 0.6).clamp(0.0, 1.0)
            }
            Self::AwaitingConnect => 1.0,
            Self::ClosingToInGame { progress } => {
                let p = ((progress - 0.2) / 0.4).clamp(0.0, 1.0);
                1.0 - p
            }
        };
        let scale = self.badge_scale();
        let t = ((scale - 0.05) / 0.20).clamp(0.0, 1.0);
        let scale_gate = t * t * (3.0 - 2.0 * t);
        in_window * scale_gate
    }

    /// Alpha for text/QR content inside (or beneath) the badge. Fades
    /// in late during intro so the spin isn't reading illegible
    /// mid-rotation, fades out early during close so the badge spins
    /// out blank.
    pub(crate) fn badge_text_alpha(self) -> f32 {
        match self {
            Self::Startup => 0.0,
            Self::IntroTransitioning { progress } | Self::ReturnFromGame { progress } => {
                ((progress - 0.5) / 0.5).clamp(0.0, 1.0)
            }
            Self::AwaitingConnect => 1.0,
            Self::ClosingToInGame { progress } => {
                (1.0 - progress / 0.4).clamp(0.0, 1.0)
            }
        }
    }

    /// Whether the main content layer (heading + QR + status strip +
    /// orbit pips) should render this frame at all. Hidden during
    /// Startup so the calm starfield stands alone; everything else
    /// renders, with individual elements scaled/faded by the methods
    /// above.
    pub(crate) fn shows_main_content(self) -> bool {
        !matches!(self, Self::Startup)
    }

    /// Alpha for the "STARTING" brand-intro title. Full opacity
    /// during Startup, fades to 0 across the first 30% of the intro
    /// transition so the title hands off smoothly to the main
    /// content (badge + label) instead of snapping out the moment
    /// the iris begins to grow. Without this the user sees a hard
    /// pop when shows_main_content flips — Chris flagged 2026-04-19.
    pub(crate) fn brand_intro_alpha(self) -> f32 {
        match self {
            Self::Startup => 1.0,
            Self::IntroTransitioning { progress } => {
                (1.0 - progress / 0.3).clamp(0.0, 1.0)
            }
            // ReturnFromGame deliberately omitted — the user already
            // knows the launcher, the "STARTING" brand intro would
            // be jarring on return from a game session.
            _ => 0.0,
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

    /// A `ScreenIntro` already fully landed — every output curve at
    /// its post-animation steady-state value (scale=1, alpha=1,
    /// content=1, iris=full). For screens that should appear instantly
    /// without the badge spin-in, e.g. Farewell after the iris-close
    /// animation finishes (Chris 2026-04-19, "shutdown showed scan to
    /// connect, never showed farewell").
    pub(crate) fn landed() -> Self {
        Self {
            elapsed_s: Self::DURATION_S,
        }
    }

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
    fn fresh_boot_starts_in_startup() {
        assert_eq!(LaunchPhase::compute(0.0, None, None, false), LaunchPhase::Startup);
        assert_eq!(LaunchPhase::compute(0.5, None, None, false), LaunchPhase::Startup);
    }

    #[test]
    fn startup_hold_boundary_enters_intro() {
        match LaunchPhase::compute(STARTUP_HOLD_S, None, None, false) {
            LaunchPhase::IntroTransitioning { progress } => assert!(approx(progress, 0.0)),
            other => panic!("expected IntroTransitioning, got {other:?}"),
        }
    }

    #[test]
    fn intro_progress_interpolates_linearly() {
        let mid = STARTUP_HOLD_S + INTRO_TRANSITION_S * 0.5;
        match LaunchPhase::compute(mid, None, None, false) {
            LaunchPhase::IntroTransitioning { progress } => assert!(approx(progress, 0.5)),
            other => panic!("expected IntroTransitioning, got {other:?}"),
        }
    }

    #[test]
    fn intro_end_lands_in_awaiting_connect() {
        let end = STARTUP_HOLD_S + INTRO_TRANSITION_S;
        assert_eq!(
            LaunchPhase::compute(end, None, None, false),
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
        assert!(approx(LaunchPhase::Startup.iris_radius(), 0.0));
        assert!(approx(
            LaunchPhase::IntroTransitioning { progress: 0.0 }.iris_radius(),
            0.0
        ));
        assert!(approx(
            LaunchPhase::IntroTransitioning { progress: 1.0 }.iris_radius(),
            IRIS_FULL
        ));
        assert!(approx(LaunchPhase::AwaitingConnect.iris_radius(), IRIS_FULL));
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
        assert_eq!(LaunchPhase::Startup.iris_mode(), IrisMode::Reveal);
        assert_eq!(LaunchPhase::AwaitingConnect.iris_mode(), IrisMode::Reveal);
        assert_eq!(
            LaunchPhase::ClosingToInGame { progress: 0.5 }.iris_mode(),
            IrisMode::DarkHole
        );
    }

    #[test]
    fn badge_scale_full_during_steady_state() {
        assert!(approx(LaunchPhase::AwaitingConnect.badge_scale(), 1.0));
    }

    #[test]
    fn badge_scale_zero_at_phase_endpoints() {
        // Beginning of intro spin window (20%): still 0.
        assert!(approx(
            LaunchPhase::IntroTransitioning { progress: 0.2 }.badge_scale(),
            0.0
        ));
        // End of close spin window (60%): back to 0.
        assert!(approx(
            LaunchPhase::ClosingToInGame { progress: 0.6 }.badge_scale(),
            0.0
        ));
        // After close spin window: stays 0.
        assert!(approx(
            LaunchPhase::ClosingToInGame { progress: 0.9 }.badge_scale(),
            0.0
        ));
    }

    #[test]
    fn text_fades_before_badge_during_close() {
        // 30% into close: text already mostly faded, badge mostly
        // intact. This is the "text leads, badge follows" timing.
        let p30 = LaunchPhase::ClosingToInGame { progress: 0.3 };
        let text = p30.badge_text_alpha();
        let badge = p30.badge_alpha();
        assert!(text < badge, "text ({text}) should fade before badge ({badge})");
    }

    #[test]
    fn close_complete_only_at_progress_one() {
        assert!(!LaunchPhase::Startup.close_complete());
        assert!(!LaunchPhase::AwaitingConnect.close_complete());
        assert!(!LaunchPhase::ClosingToInGame { progress: 0.5 }.close_complete());
        assert!(!LaunchPhase::ClosingToInGame { progress: 0.99 }.close_complete());
        assert!(LaunchPhase::ClosingToInGame { progress: 1.0 }.close_complete());
    }

    #[test]
    fn iris_radius_monotonic_across_intro() {
        let mut prev = -1.0;
        let mut t = 0.0;
        while t <= STARTUP_HOLD_S + INTRO_TRANSITION_S + 0.1 {
            let now = LaunchPhase::compute(t, None, None, false).iris_radius();
            assert!(
                now >= prev - 1e-5,
                "iris dropped at t={t}: prev={prev}, now={now}"
            );
            prev = now;
            t += 0.05;
        }
    }

    #[test]
    fn main_content_hidden_only_in_startup() {
        assert!(!LaunchPhase::Startup.shows_main_content());
        assert!(LaunchPhase::IntroTransitioning { progress: 0.0 }.shows_main_content());
        assert!(LaunchPhase::AwaitingConnect.shows_main_content());
        assert!(LaunchPhase::ClosingToInGame { progress: 0.0 }.shows_main_content());
    }
}
