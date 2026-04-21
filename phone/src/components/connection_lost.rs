//! ConnectionLost overlay (PLAN 4.18.21).
//!
//! Highest-priority modal in the phone stack — renders on top of every
//! other screen once we've been off-WS long enough to cross the grace
//! window. Per `docs/aesthetic/navigation.md` §3.8 modal priority: this
//! beats GameCrashed, KaosTakeover, KaosSwap, and the normal flow.
//!
//! Drives off two signals from `ws.rs`:
//!   - `reconnect_attempts` — bumped on every onclose, reset to 0 on a
//!     successful onopen (or on manual retry). Non-zero == "we've lost
//!     the connection and haven't gotten it back yet." We deliberately
//!     don't watch `conn` directly because the reconnect cycle flips it
//!     between `Disconnected` (briefly, at each onclose) and `Connecting`
//!     (during each backoff-fired spawn) — driving the overlay off `conn`
//!     causes it to flash on every tick.
//!   - `manual_retry` — bumped by the TRY AGAIN button; `ws.rs` watches
//!     it and reconnects immediately, cancelling any pending backoff
//!     timer and resetting `reconnect_attempts` to 0 (which hides the
//!     overlay until the next failure).
//!
//! ~1s grace absorbs momentary drops (server restart, brief radio dip):
//! we only flip `visible` on the 0→≥1 transition, after the timer
//! confirms `reconnect_attempts` is *still* > 0. Subsequent attempts in
//! the same disconnected stretch don't re-arm — they just keep ticking,
//! and `visible` stays true until a successful reconnect resets attempts.

use leptos::prelude::*;

use crate::components::{DisplayHeading, HeadingSize};
use crate::{dev_log, gloo_timer};

/// Show the manual TRY AGAIN button after this many failed reconnect
/// attempts. Three attempts at 500ms, 1s, 2s = ~3.5s of waiting before we
/// surface the manual retry. Beyond that the auto-backoff hits its 8s
/// ceiling and a button is the only way to feel agency.
const RETRY_THRESHOLD: u32 = 3;

/// Grace before the overlay actually appears, to absorb momentary drops.
const GRACE_MS: i32 = 1000;

#[component]
pub(crate) fn ConnectionLost(
    reconnect_attempts: RwSignal<u32>,
    manual_retry: RwSignal<u32>,
) -> impl IntoView {
    let visible = RwSignal::new(false);

    // 0 → hide. 0 → ≥1 transition → arm a deferred reveal that re-checks
    // after the grace window. Subsequent ticks within the stretch are
    // no-ops (we only re-arm when crossing back through 0).
    Effect::new(move |prev: Option<u32>| {
        let now = reconnect_attempts.get();
        dev_log!(
            "[overlay] effect prev={prev:?} now={now} visible={}",
            visible.get_untracked()
        );
        if now == 0 {
            dev_log!("[overlay] visible→false (attempts=0)");
            visible.set(false);
        } else if prev.unwrap_or(0) == 0 {
            dev_log!("[overlay] arming grace timer");
            leptos::task::spawn_local(async move {
                gloo_timer(GRACE_MS).await;
                let attempts_after_grace = reconnect_attempts.get_untracked();
                dev_log!("[overlay] grace fired, attempts={attempts_after_grace}");
                if attempts_after_grace > 0 {
                    dev_log!("[overlay] visible→true");
                    visible.set(true);
                }
            });
        }
        now
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
