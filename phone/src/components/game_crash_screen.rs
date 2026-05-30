//! GameCrashScreen — full-screen overlay for unexpected RPCS3 exit
//! (PLAN 4.15.14, design_language.md §6 + navigation.md §3.8).
//!
//! Rendered when the server broadcasts `Event::GameCrashed`; preempts
//! every other screen in the phone's stack except `ConnectionLost`
//! (which has higher z and renders independently from the bottom of
//! `App()`'s view). Auto-dismisses on the next
//! `GameChanged { current: Some(_) }` (the WS handler in `ws.rs`
//! clears `game_crash` when a new game boots).
//!
//! Extracted from `screens/modals.rs` per PLAN 4.20.5a — modals.rs was
//! housing several unrelated overlays; the crash screen is its own thing.
//!
//! MVP copy follows `docs/aesthetic/navigation.md` §3.8:
//!   - Heading: "GAME CRASHED" (gold display treatment)
//!   - Body: short reassurance + the server-supplied diagnostic
//!   - Action: "RETURN TO GAMES" gold button → clear the overlay; the
//!     underlying GamePicker renders because the crash watchdog has
//!     already broadcast `GameChanged { current: None }`.
//!
//! Auto-recovery path (PLAN 16.7.2/.3): when the server is auto-restarting
//! RPCS3 (crash *or* freeze) it broadcasts `Event::GameRecovering`, which sets
//! `GameCrashReason.recovering = true`. This component then renders its
//! transient "RECONNECTING…" form — a spinner, no action button — instead of
//! the terminal crash form. The next `GameChanged { current: Some(_) }`
//! dismisses it; if recovery gives up the server sends `GameCrashed` and the
//! overlay flips to its terminal form (RETURN TO GAMES).

// `#[component]` macro emits a wrapper around the fn that's effectively
// `pub`, so `pub(crate)` parameter types (GameCrashReason, ToastMsg —
// crate-internal helper structs in `crate::lib`) trip the
// private_interfaces lint. Module-scoped allow because the lint fires
// on the macro expansion, not on the function attribute.
#![allow(private_interfaces)]

use leptos::prelude::*;

use crate::api::post_quit;
use crate::components::{DisplayHeading, HeadingSize};
use crate::model::GameLaunched;
use crate::{push_toast, GameCrashReason, ToastMsg};

#[component]
pub(crate) fn GameCrashScreen(
    game_crash: RwSignal<Option<GameCrashReason>>,
    current_game: RwSignal<Option<GameLaunched>>,
    toasts: RwSignal<Vec<ToastMsg>>,
) -> impl IntoView {
    let message = Signal::derive(move || {
        game_crash
            .get()
            .map(|c| c.message)
            .unwrap_or_else(|| "The emulator stopped unexpectedly.".into())
    });
    // `true` while the server is auto-recovering (crash/freeze → restart);
    // `false` for a terminal crash. Drives the two overlay forms.
    let recovering =
        Signal::derive(move || game_crash.get().map(|c| c.recovering).unwrap_or(false));

    view! {
        <section
            class="crash-overlay"
            class:crash-overlay--recovering=move || recovering.get()
        >
            <div class="crash-backdrop"></div>
            <div class="crash-sparks"></div>

            <div class="crash-viewport">
                <Show
                    when=move || recovering.get()
                    fallback=move || view! {
                        <GameCrashTerminal
                            message=message
                            game_crash=game_crash
                            current_game=current_game
                            toasts=toasts
                        />
                    }
                >
                    <div class="crash-spinner" aria-hidden="true"></div>

                    <DisplayHeading size=HeadingSize::Lg>
                        "RECONNECTING\u{2026}"
                    </DisplayHeading>

                    <p class="crash-body">
                        "Your game hiccuped \u{2014} we\u{2019}re putting it back \
                         together and re-placing your figures. One moment\u{2026}"
                    </p>

                    <p class="crash-detail">{move || message.get()}</p>
                </Show>
            </div>
        </section>
    }
}

/// Terminal crash form (RPCS3 died and auto-recovery either wasn't possible or
/// gave up). Carries the RETURN TO GAMES action; split out so the recovering
/// form can stay button-free without duplicating the overlay shell.
#[component]
fn GameCrashTerminal(
    message: Signal<String>,
    game_crash: RwSignal<Option<GameCrashReason>>,
    current_game: RwSignal<Option<GameLaunched>>,
    toasts: RwSignal<Vec<ToastMsg>>,
) -> impl IntoView {
    // "RETURN TO GAMES" — just dismiss. The supervisor already sent
    // `GameChanged { current: None }` alongside `GameCrashed`, so the
    // GamePicker will render under us as soon as we clear the overlay.
    // Belt-and-braces: also clear `current_game` locally in case a
    // future server change stops sending the implicit game-ended event.
    // Fire a best-effort `/api/quit?force=true` so any half-alive RPCS3
    // process that the supervisor missed gets cleaned up server-side.
    let on_return = move |_| {
        game_crash.set(None);
        current_game.set(None);
        leptos::task::spawn_local(async move {
            if let Err(e) = post_quit(true).await {
                push_toast(toasts, &format!("Cleanup quit failed: {e}"));
            }
        });
    };

    view! {
        <div class="crash-warning-mark">{"\u{26A0}"}</div>

        <DisplayHeading size=HeadingSize::Lg>
            "GAME CRASHED"
        </DisplayHeading>

        <p class="crash-body">
            "The emulator stopped unexpectedly. \
             Hang tight \u{2014} we\u{2019}ll get you back to your adventure."
        </p>

        <p class="crash-detail">{move || message.get()}</p>

        <button
            class="crash-return-btn"
            on:click=on_return
        >
            "RETURN TO GAMES"
        </button>
    }
}
