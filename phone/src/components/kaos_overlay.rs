//! Full-screen Kaos overlay (design_language.md §6.10).
//!
//! Two variants are planned per the design doc:
//!
//! - **`takeover`** — shipped now. Rendered when the server sends
//!   `Event::TakenOver` because a third phone joined and the FIFO
//!   registry evicted this session. Shows the Kaos sigil + "KAOS
//!   REIGNS!" title + taunt quote + KICK BACK IN button.
//!
//! - **`swap`** — Phase 5.3 follow-up (mid-game 1-for-1 figure swap
//!   announcement). Will extend this component with an additional
//!   signal prop + a variant dispatch.
//!
//! Extracted from `screens/modals.rs` per PLAN 4.20.5. Renamed from
//! `TakeoverScreen` to `KaosOverlay` so the Phase 5.3 swap variant
//! lands as a new branch inside the same component surface rather
//! than a parallel screen-level one-off.

// `#[component]` macro emits a wrapper around the fn that's effectively
// `pub`; `pub(crate)` parameter types (`TakeoverReason` from `lib.rs`)
// trip the `private_interfaces` lint on the macro expansion. Module-
// scoped allow mirrors what `game_crash_screen.rs` does for the same
// reason.
#![allow(private_interfaces)]

use std::cell::Cell;
use std::rc::Rc;

use leptos::prelude::*;
use wasm_bindgen::JsCast;
use wasm_bindgen::closure::Closure;

use crate::TakeoverReason;

/// Kaos takeover / swap overlay. Today only renders when
/// `takeover.get().is_some()` (takeover variant); the enclosing
/// `App()` controls mount via `<Show>`.
#[component]
pub(crate) fn KaosOverlay(takeover: RwSignal<Option<TakeoverReason>>) -> impl IntoView {
    // Kaos took the slot. "Kick back" does a full page reload — the browser
    // opens a fresh WS, server tries to re-admit via the FIFO path. If the
    // 1-minute cooldown is still active, the reload-WS gets an `Error` event
    // and closes. If cooldown elapsed, we land back at the ProfilePicker
    // (server re-locks all profiles on a fresh session so PIN re-entry is
    // required — SPEC Q46).

    // Local countdown signal driving the disabled state on the
    // KICK BACK IN button (PLAN 8.2a). Initialised from the
    // `cooldown_remaining_secs` carried on the TakenOver event;
    // a 1Hz interval decrements until zero, then the button
    // re-enables. Using `Effect` with `on_cleanup` so the timer
    // is properly torn down if the overlay unmounts mid-countdown
    // (e.g. takeover signal cleared programmatically in tests).
    let cooldown_remaining = RwSignal::new(0u32);

    Effect::new(move |_| {
        // Re-runs whenever `takeover` changes. On a transition to
        // Some(_), seed the countdown and start the interval; on
        // None, leave the countdown at 0 (no timer needed — the
        // overlay is unmounting).
        let Some(t) = takeover.get() else {
            return;
        };
        cooldown_remaining.set(t.cooldown_remaining_secs);
        if t.cooldown_remaining_secs == 0 {
            return;
        }

        // Self-cancelling interval. Stored by reference inside its
        // own callback so the last tick can clear itself once the
        // countdown reaches zero. Single-threaded WASM, so
        // `Rc<Cell<_>>` is fine; `on_cleanup` would need `Send +
        // Sync` which `Rc` isn't, so instead the tick checks
        // `takeover.get_untracked()` and self-cancels if the
        // overlay was dismissed (kick → page reload kills the
        // timer for free; programmatic dismissal goes through the
        // None branch). Closure leaks via `forget` — typical
        // lifecycle ends in a full page reload, so this is bounded.
        let handle: Rc<Cell<Option<i32>>> = Rc::new(Cell::new(None));
        let tick = {
            let handle = handle.clone();
            Closure::<dyn FnMut()>::new(move || {
                let dismissed = takeover.get_untracked().is_none();
                let next = cooldown_remaining.get_untracked().saturating_sub(1);
                cooldown_remaining.set(next);
                if dismissed || next == 0 {
                    if let (Some(h), Some(w)) = (handle.take(), web_sys::window()) {
                        w.clear_interval_with_handle(h);
                    }
                }
            })
        };

        if let Some(w) = web_sys::window() {
            if let Ok(h) = w.set_interval_with_callback_and_timeout_and_arguments_0(
                tick.as_ref().unchecked_ref(),
                1000,
            ) {
                handle.set(Some(h));
            }
        }
        tick.forget();
    });

    let kick_disabled = move || cooldown_remaining.get() > 0;
    let kick_label = move || {
        let remaining = cooldown_remaining.get();
        if remaining > 0 {
            format!("KICK BACK IN \u{00B7} {remaining}s")
        } else {
            "KICK BACK IN".to_string()
        }
    };

    view! {
        <section class="takeover-void">
            <div class="takeover-hexgrid"></div>
            <div class="takeover-sparks"></div>

            <div class="takeover-viewport">
                // Kaos sigil placeholder — actual SVG mask wiring deferred
                <div class="kaos-sigil"></div>

                <h1 class="kaos-title">
                    "KAOS"
                    <span class="kaos-title-line2">"REIGNS!"</span>
                </h1>

                <div class="takeover-quote-card">
                    <div class="takeover-quote-open">{"\u{201C}"}</div>
                    <div class="takeover-quote-body">
                        {move || takeover
                            .get()
                            .map(|t| t.by_kaos.clone())
                            .unwrap_or_else(|| "Behold my magnificent wickedness!".into())}
                    </div>
                    <div class="takeover-quote-close">{"\u{201D}"}</div>
                    <div class="takeover-quote-attrib">{"\u{2014} KAOS"}</div>
                </div>

                <p class="takeover-info">
                    "your seat has been claimed \u{00B7} enter your pin to return"
                </p>

                <button
                    class="takeover-kick-btn"
                    class:takeover-kick-btn--cooldown=kick_disabled
                    disabled=kick_disabled
                    on:click=move |_| {
                        if kick_disabled() {
                            return;
                        }
                        if let Some(loc) = web_sys::window().map(|w| w.location()) {
                            let _ = loc.reload();
                        }
                    }
                >
                    {kick_label}
                </button>
            </div>

            <div class="takeover-vignette"></div>
        </section>
    }
}
