//! ConnectionLost overlay (PLAN 4.18.21).
//!
//! Highest-priority modal in the phone stack — renders on top of every
//! other screen when the WS has been disconnected for the grace window.
//! Per `docs/aesthetic/navigation.md` §3.8 modal priority: this beats
//! GameCrashed, KaosTakeover, KaosSwap, and the normal flow.
//!
//! Drives off three signals from `ws.rs`:
//!   - `conn` — current connection state. Anything other than
//!     `Disconnected` hides the overlay.
//!   - `reconnect_attempts` — incremented on each onclose. Once it crosses
//!     [`RETRY_THRESHOLD`], the spinner gives way to a manual "TRY AGAIN"
//!     button so the user isn't stuck watching an 8-second backoff.
//!   - `manual_retry` — bumped by the TRY AGAIN button; `ws.rs` watches it
//!     and reconnects immediately, cancelling any pending backoff timer.
//!
//! A ~1s grace prevents a momentary disconnect (e.g., the dev server's
//! restart, or a brief radio drop) from flashing the overlay. The grace
//! is implemented as a deferred set-on-true: we mount immediately but only
//! flip `visible` after the timer if `conn` is *still* Disconnected.

use leptos::prelude::*;

use crate::components::{DisplayHeading, HeadingSize};
use crate::gloo_timer;
use crate::model::ConnState;

/// Show the manual TRY AGAIN button after this many failed reconnect
/// attempts. Three attempts at 500ms, 1s, 2s = ~3.5s of waiting before we
/// surface the manual retry. Beyond that the auto-backoff hits its 8s
/// ceiling and a button is the only way to feel agency.
const RETRY_THRESHOLD: u32 = 3;

/// Grace before the overlay actually appears, to absorb momentary drops.
const GRACE_MS: i32 = 1000;

#[component]
pub(crate) fn ConnectionLost(
    conn: RwSignal<ConnState>,
    reconnect_attempts: RwSignal<u32>,
    manual_retry: RwSignal<u32>,
) -> impl IntoView {
    let visible = RwSignal::new(false);

    // Watch `conn`. On entering Disconnected, schedule a check after the
    // grace window — only flip to visible if we're still disconnected. On
    // leaving Disconnected, hide immediately. Effect re-runs on every
    // `conn` change; the spawn_local check at the end of the grace
    // re-reads `conn` so a quick reconnect won't show the overlay.
    Effect::new(move |_| {
        let state = conn.get();
        if state == ConnState::Disconnected {
            leptos::task::spawn_local(async move {
                gloo_timer(GRACE_MS).await;
                if conn.get_untracked() == ConnState::Disconnected {
                    visible.set(true);
                }
            });
        } else {
            visible.set(false);
        }
    });

    let show_retry = Memo::new(move |_| reconnect_attempts.get() >= RETRY_THRESHOLD);
    let on_retry = move |_| {
        manual_retry.update(|n| *n = n.saturating_add(1));
    };

    view! {
        <Show when=move || visible.get() fallback=|| ()>
            <section class="conn-lost-overlay" data-testid="connection-lost">
                <div class="conn-lost-backdrop"></div>
                <div class="conn-lost-viewport">
                    <div class="conn-lost-pip" aria-hidden="true">{"\u{2715}"}</div>

                    <DisplayHeading size=HeadingSize::Lg>
                        "LOST CONNECTION"
                    </DisplayHeading>

                    <p class="conn-lost-body">
                        "Your portal drifted out of range. Hang tight \u{2014} we\u{2019}re trying to reconnect."
                    </p>

                    <Show
                        when=move || show_retry.get()
                        fallback=|| view! {
                            <div class="conn-lost-reconnect-bar" role="status">
                                <div class="conn-lost-spinner" aria-hidden="true"></div>
                                <div class="conn-lost-label">"reconnecting\u{2026}"</div>
                            </div>
                        }
                    >
                        <button
                            class="conn-lost-retry-btn"
                            on:click=on_retry
                        >
                            "TRY AGAIN"
                        </button>
                    </Show>

                    <p class="conn-lost-hint">
                        "make sure the TV is on and you\u{2019}re on the same Wi-Fi"
                    </p>
                </div>
            </section>
        </Show>
    }
}
