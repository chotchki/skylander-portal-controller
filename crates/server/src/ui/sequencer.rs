//! Pure-state-machine extracts from the dispatcher in `ui/mod.rs`.
//!
//! The dispatcher's `update()` is intertwined with egui (UI context,
//! painting, input) which makes its decision logic hard to unit-test.
//! Each piece extracted into this module is a pure function over
//! `(prev state, current LauncherStatus, now)` → new state, so it can
//! be driven through arbitrary scenarios in tests without standing up
//! a real eframe context.
//!
//! Coverage focus: the bugs the launcher state machine has hit so far
//! during live testing — close-timer flapping during mid-game shader
//! compile, shutdown not reaching the farewell render, etc. (Chris
//! 2026-04-19, "this sequence should really be in an e2e test").

use std::time::Instant;

use crate::state::{LauncherScreen, LauncherStatus};

/// Persistent timestamps for the in-game and shutdown close
/// animations. The dispatcher stores one of these and calls
/// [`tick`](Self::tick) once per frame; the resulting [`elapsed_s`]
/// (Self::elapsed_s) feeds `LaunchPhase::compute`.
#[derive(Debug, Default, Clone, Copy)]
pub(super) struct CloseTimers {
    pub(super) in_game_at: Option<Instant>,
    pub(super) shutdown_at: Option<Instant>,
}

impl CloseTimers {
    /// Update timers based on the latest status snapshot.
    ///
    /// `in_game_at` semantics:
    ///   - **Set** when the game becomes playable on the Main screen
    ///     and we don't already have a timer running.
    ///   - **Persists** through `game_playable` flapping — mid-game
    ///     shader compile briefly flips playable false → true, and
    ///     the launcher would otherwise reset the timer and re-render
    ///     the QR card over the running game (Chris flagged
    ///     2026-04-19, "watcher fired a bunch of times threw game
    ///     back into weird loading but still scan to connect").
    ///   - **Cleared** only when the in-game session genuinely ends:
    ///     `!rpcs3_running` (game stopped) OR `screen != Main`
    ///     (e.g. flipped to Farewell by a shutdown request).
    ///
    /// `shutdown_at` semantics:
    ///   - **Set** the first frame `screen == Farewell`.
    ///   - **Cleared** if screen ever leaves Farewell (defensive — no
    ///     code path does that today).
    pub(super) fn tick(&mut self, now: Instant, status: &LauncherStatus) {
        // PLAN 4.15.9 post-regression fix: the close-to-in-game
        // animation must track `current_game` presence, not just
        // `game_playable && rpcs3_running`. Under 4.15.16's always-
        // running RPCS3 contract, `game_playable` flips true the
        // moment RPCS3 lands at its library view (no compile
        // activity), and `rpcs3_running` stays true across quits —
        // so without the current_game gate the close timer would
        // latch at server startup and never unlatch, putting the
        // launcher into close_complete with a blank dark badge
        // instead of showing SCAN TO CONNECT.
        //
        // PLAN 10.8.7b: gate on `!cover_active` too so a graceful-
        // quit cover doesn't re-arm the close-to-in-game timer
        // mid-cover. The cover transition wants ReturnFromGame
        // (iris 0→open with Returning badge), not ClosingToInGame.
        let want_close_start = status.game_playable
            && status.current_game.is_some()
            && !status.switching
            && !status.cover_active
            && matches!(status.screen, LauncherScreen::Main);
        // Clear when: game stopped, current_game gone, screen changed,
        // a switch was requested, or a quit cover started. The
        // switching/cover cases cover the window between the request
        // and `current_game` clearing on kill completion — without
        // them, `closing_elapsed_s` would still feed
        // `LaunchPhase::ClosingToInGame { progress ≥ 1.0 }` and pin
        // the iris fully closed instead of letting it open back up
        // behind the SWITCHING / RETURNING back-face.
        let kill_close = !status.rpcs3_running
            || status.current_game.is_none()
            || status.switching
            || status.cover_active
            || !matches!(status.screen, LauncherScreen::Main);
        if want_close_start && self.in_game_at.is_none() {
            self.in_game_at = Some(now);
        }
        if kill_close {
            self.in_game_at = None;
        }

        if matches!(status.screen, LauncherScreen::Farewell) {
            if self.shutdown_at.is_none() {
                self.shutdown_at = Some(now);
            }
        } else {
            self.shutdown_at = None;
        }
    }

    /// Combined elapsed seconds for whichever trigger is active.
    /// In-game close takes priority when both are set (can't happen
    /// in practice — they require different `screen` values — but
    /// the priority makes the function total).
    pub(super) fn elapsed_s(&self, now: Instant) -> Option<f32> {
        self.in_game_at
            .or(self.shutdown_at)
            .map(|t| now.duration_since(t).as_secs_f32())
    }
}

/// Detect the moment the launcher should kick off the
/// `LaunchPhase::ReturnFromGame` animation: previous frame rendered
/// the in-game surface, this frame doesn't, AND screen is still Main
/// (game ended cleanly via `/api/quit` or RPCS3 process exit, NOT
/// by transitioning to Crashed/Farewell which have their own paths).
pub(super) fn detect_returning_from_game(was_in_game: bool, status: &LauncherStatus) -> bool {
    // PLAN 10.8.7b: cover_active flips us out of in-game mid-flight
    // (graceful quit). Treat a covered state as "not in-game" so the
    // ReturnFromGame iris-open animation triggers when the cover
    // arms — not just on RPCS3 process death. RPCS3 is still alive
    // at that point (kill happens after the cover renders) but the
    // user-visible transition is starting.
    let want_in_game_now = status.rpcs3_running
        && !status.cover_active
        && matches!(status.screen, LauncherScreen::Main);
    was_in_game && !want_in_game_now && matches!(status.screen, LauncherScreen::Main)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    fn status_main(rpcs3: bool, playable: bool) -> LauncherStatus {
        // Default: a game is booted (`current_game = Some`) so
        // want_close_start's 3-way gate passes for the latch-related
        // tests. Tests that exercise the "no game booted" state call
        // `status_main_no_game` instead.
        LauncherStatus {
            rpcs3_running: rpcs3,
            game_playable: playable,
            screen: LauncherScreen::Main,
            current_game: Some("Skylanders: Giants".into()),
            ..Default::default()
        }
    }

    /// PLAN 10.8.7b: graceful-quit cover state. RPCS3 still alive
    /// (kill happens AFTER the launcher renders the cover), game
    /// still playable, but `cover_active = true`. Modelled tests
    /// assert the close-timer + return-detection logic gates
    /// correctly so the launcher transitions out of the
    /// transparent in-game render and into ReturnFromGame.
    fn status_main_covering() -> LauncherStatus {
        LauncherStatus {
            rpcs3_running: true,
            game_playable: true,
            screen: LauncherScreen::Main,
            current_game: Some("Skylanders: Giants".into()),
            cover_active: true,
            ..Default::default()
        }
    }

    /// Variant with `current_game = None` — models the launcher
    /// between quit and next boot. PLAN 4.15.9 post-regression:
    /// in this state the close timer must NOT latch, otherwise
    /// the launcher holds a blank dark badge instead of reverting
    /// to SCAN TO CONNECT.
    fn status_main_no_game(rpcs3: bool, playable: bool) -> LauncherStatus {
        LauncherStatus {
            rpcs3_running: rpcs3,
            game_playable: playable,
            screen: LauncherScreen::Main,
            current_game: None,
            ..Default::default()
        }
    }

    fn status_farewell() -> LauncherStatus {
        LauncherStatus {
            screen: LauncherScreen::Farewell,
            ..Default::default()
        }
    }

    #[test]
    fn close_timer_set_when_game_playable_on_main() {
        let mut t = CloseTimers::default();
        let now = Instant::now();
        t.tick(now, &status_main(true, true));
        assert!(t.in_game_at.is_some());
    }

    #[test]
    fn close_timer_not_set_when_no_game_booted() {
        // PLAN 4.15.9 post-regression: at boot the launcher has
        // rpcs3_running=true (always-running RPCS3), screen=Main,
        // game_playable=true (RPCS3 at library view, not compiling)
        // — but current_game=None. The close timer must NOT latch
        // in this state, otherwise the launcher holds close_complete
        // with a blank dark badge instead of showing SCAN TO CONNECT.
        let mut t = CloseTimers::default();
        let now = Instant::now();
        t.tick(now, &status_main_no_game(true, true));
        assert!(
            t.in_game_at.is_none(),
            "close timer must not latch without a booted game",
        );
    }

    #[test]
    fn close_timer_cleared_when_game_quits() {
        // Quit path under 4.15.16: rpcs3_running stays true (process
        // alive at library), screen stays Main, but current_game
        // flips to None. The timer must clear so the launcher
        // reverts to AwaitingConnect with iris open.
        let mut t = CloseTimers::default();
        let now = Instant::now();
        t.tick(now, &status_main(true, true));
        assert!(t.in_game_at.is_some(), "timer latches while game booted");

        t.tick(
            now + Duration::from_secs(1),
            &status_main_no_game(true, true),
        );
        assert!(
            t.in_game_at.is_none(),
            "timer clears when current_game drops to None (quit)",
        );
    }

    #[test]
    fn close_timer_not_set_when_not_playable() {
        let mut t = CloseTimers::default();
        let now = Instant::now();
        // RPCS3 running but compile activity in progress.
        t.tick(now, &status_main(true, false));
        assert!(t.in_game_at.is_none());
    }

    #[test]
    fn close_timer_persists_through_playable_flap() {
        // The flap bug: game_playable goes false during mid-game
        // shader compile, then back to true. The close timer must
        // stay set across the flap so the launcher doesn't reset
        // its state and re-render the QR card over the game.
        let mut t = CloseTimers::default();
        let t0 = Instant::now();

        t.tick(t0, &status_main(true, true));
        let initial = t.in_game_at;
        assert!(initial.is_some());

        // Mid-game compile bursts: playable flips false then true,
        // many times.
        for i in 1..=10 {
            let later = t0 + Duration::from_millis(i * 100);
            let playable = i % 2 == 0;
            t.tick(later, &status_main(true, playable));
        }

        assert_eq!(
            t.in_game_at, initial,
            "close timer must not reset on game_playable flap",
        );
    }

    #[test]
    fn close_timer_cleared_when_rpcs3_stops() {
        let mut t = CloseTimers::default();
        let now = Instant::now();
        t.tick(now, &status_main(true, true));
        assert!(t.in_game_at.is_some());

        t.tick(now + Duration::from_secs(1), &status_main(false, false));
        assert!(t.in_game_at.is_none());
    }

    #[test]
    fn close_timer_cleared_when_screen_leaves_main() {
        let mut t = CloseTimers::default();
        let now = Instant::now();
        t.tick(now, &status_main(true, true));
        assert!(t.in_game_at.is_some());

        t.tick(now + Duration::from_secs(1), &status_farewell());
        assert!(
            t.in_game_at.is_none(),
            "shutdown (screen=Farewell) must end the in-game close",
        );
    }

    #[test]
    fn shutdown_timer_set_first_frame_of_farewell() {
        let mut t = CloseTimers::default();
        let now = Instant::now();
        t.tick(now, &status_farewell());
        assert!(t.shutdown_at.is_some());
    }

    #[test]
    fn shutdown_timer_persists_across_frames() {
        let mut t = CloseTimers::default();
        let t0 = Instant::now();
        t.tick(t0, &status_farewell());
        let initial = t.shutdown_at;

        for i in 1..=10 {
            t.tick(t0 + Duration::from_millis(i * 100), &status_farewell());
        }
        assert_eq!(t.shutdown_at, initial);
    }

    #[test]
    fn shutdown_timer_cleared_when_screen_leaves_farewell() {
        let mut t = CloseTimers::default();
        let now = Instant::now();
        t.tick(now, &status_farewell());
        assert!(t.shutdown_at.is_some());

        t.tick(now + Duration::from_secs(1), &status_main(false, false));
        assert!(t.shutdown_at.is_none());
    }

    #[test]
    fn elapsed_returns_in_game_when_only_in_game_set() {
        let mut t = CloseTimers::default();
        let t0 = Instant::now();
        t.tick(t0, &status_main(true, true));

        let elapsed = t.elapsed_s(t0 + Duration::from_millis(500));
        assert!(matches!(elapsed, Some(s) if (s - 0.5).abs() < 0.01));
    }

    #[test]
    fn elapsed_returns_shutdown_when_only_shutdown_set() {
        let mut t = CloseTimers::default();
        let t0 = Instant::now();
        t.tick(t0, &status_farewell());

        let elapsed = t.elapsed_s(t0 + Duration::from_secs(2));
        assert!(matches!(elapsed, Some(s) if (s - 2.0).abs() < 0.01));
    }

    #[test]
    fn elapsed_returns_none_when_neither_set() {
        let t = CloseTimers::default();
        assert!(t.elapsed_s(Instant::now()).is_none());
    }

    #[test]
    fn elapsed_prefers_in_game_when_both_set() {
        // Synthetic case — in real code `tick` resets in_game_at when
        // screen flips to Farewell, so both can't be set at once. But
        // the priority keeps `elapsed_s` total, so test it.
        let t0 = Instant::now();
        let t = CloseTimers {
            in_game_at: Some(t0),
            shutdown_at: Some(t0 + Duration::from_secs(1)),
        };
        let elapsed = t.elapsed_s(t0 + Duration::from_secs(2));
        // Should be 2s (since in_game_at), not 1s (since shutdown_at).
        assert!(matches!(elapsed, Some(s) if (s - 2.0).abs() < 0.01));
    }

    #[test]
    fn flap_then_shutdown_starts_shutdown_timer_cleanly() {
        // Composite scenario: game playable, then flap, then user
        // requests shutdown. The shutdown timer should start fresh
        // on the Farewell frame; the in-game timer should clear.
        let mut t = CloseTimers::default();
        let t0 = Instant::now();
        t.tick(t0, &status_main(true, true));
        t.tick(t0 + Duration::from_millis(100), &status_main(true, false));
        t.tick(t0 + Duration::from_millis(200), &status_main(true, true));

        let in_game_before = t.in_game_at;
        assert!(in_game_before.is_some());
        assert!(t.shutdown_at.is_none());

        // Phone shutdown.
        let t_shutdown = t0 + Duration::from_millis(500);
        t.tick(t_shutdown, &status_farewell());
        assert!(t.in_game_at.is_none(), "in-game timer must clear");
        assert_eq!(t.shutdown_at, Some(t_shutdown));
    }

    #[test]
    fn returning_detected_when_in_game_ends_with_screen_main() {
        let status = status_main(false, false);
        assert!(detect_returning_from_game(true, &status));
    }

    #[test]
    fn returning_not_detected_when_screen_flipped_away() {
        let status = status_farewell();
        assert!(
            !detect_returning_from_game(true, &status),
            "shutdown is its own path, not return-from-game",
        );
    }

    #[test]
    fn returning_not_detected_when_was_not_in_game() {
        let status = status_main(false, false);
        assert!(!detect_returning_from_game(false, &status));
    }

    #[test]
    fn returning_not_detected_when_still_in_game() {
        // rpcs3 still running, screen=Main → still in-game, not returning.
        let status = status_main(true, true);
        assert!(!detect_returning_from_game(true, &status));
    }

    // ---- PLAN 10.8.7b: cover_active state machine ----

    #[test]
    fn cover_active_clears_close_timer() {
        // Game in-game (close_timer latched), then user quits → server
        // sets cover_active=true (RPCS3 still alive briefly). Close
        // timer must clear so launch_phase transitions out of
        // ClosingToInGame{1.0} and into ReturnFromGame.
        let mut t = CloseTimers::default();
        let t0 = Instant::now();
        t.tick(t0, &status_main(true, true));
        assert!(t.in_game_at.is_some(), "in-game close timer latched");

        t.tick(t0 + Duration::from_millis(100), &status_main_covering());
        assert!(
            t.in_game_at.is_none(),
            "cover_active must clear the close timer",
        );
    }

    #[test]
    fn cover_active_blocks_close_timer_set() {
        // Even from a fresh state, want_close_start must NOT fire if
        // cover_active is set. The cover is the cover; ClosingToInGame
        // would re-pin the iris closed.
        let mut t = CloseTimers::default();
        t.tick(Instant::now(), &status_main_covering());
        assert!(
            t.in_game_at.is_none(),
            "cover_active gate prevents close timer from latching",
        );
    }

    #[test]
    fn returning_detected_when_cover_arms_mid_game() {
        // The full cover-before-kill semantics: RPCS3 is still alive
        // and current_game is still Some when cover_active flips
        // true. detect_returning_from_game must fire so the
        // ReturnFromGame iris-open animation starts BEFORE the kill,
        // ensuring the cover is up before RPCS3 dies.
        let status = status_main_covering();
        assert!(
            detect_returning_from_game(true, &status),
            "cover arming mid-game must trigger return-from-game",
        );
    }

    #[test]
    fn returning_not_detected_when_covering_without_prior_in_game() {
        // If we never entered in-game (was_in_game=false), cover
        // shouldn't trigger a phantom return animation. e.g. a server
        // bug that sets cover_active without a prior game shouldn't
        // animate a fake "return".
        let status = status_main_covering();
        assert!(!detect_returning_from_game(false, &status));
    }
}
