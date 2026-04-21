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

use leptos::prelude::*;

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
                    on:click=move |_| {
                        if let Some(loc) = web_sys::window().map(|w| w.location()) {
                            let _ = loc.reload();
                        }
                    }
                >
                    "KICK BACK IN"
                </button>
            </div>

            <div class="takeover-vignette"></div>
        </section>
    }
}
