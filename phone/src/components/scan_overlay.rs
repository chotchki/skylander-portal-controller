//! Scan-to-import overlay (PLAN 6.5.2).
//!
//! Three states co-exist in the markup; the active one is driven by the
//! app-level [`crate::ScanOverlayState`] signal:
//!
//! - **Prompt**: "tap your figure on the reader" + pulsing radar viz.
//!   Entered when the user taps the `+` card in the toy-box grid.
//! - **Success**: confirmation that a figure landed in the collection,
//!   with the parser-derived name. Entered when a matching
//!   [`skylander_core::Event::FigureScanned`] arrives while the overlay
//!   is in Prompt.
//! - **Timeout**: "we didn't see a tap" with retry/cancel. Fired by the
//!   internal 30s timer if no scan arrives.
//!
//! When the overlay is Closed, `FigureScanned` events surface as a
//! passive success toast instead — see `ws.rs`. Ambient scans (figure
//! left on the reader without opening the overlay) shouldn't vanish.
//!
//! Visual language: see `docs/aesthetic/mocks/scan_import.html`. The
//! scan-center bezel mirrors the portal's empty-slot construction
//! (filled gold donut wrapping a dark-blue plate) so the scanner
//! reads as "just another slot" in the app's vocabulary.

#![allow(private_interfaces)]

use std::cell::Cell;
use std::rc::Rc;

use leptos::prelude::*;
use wasm_bindgen::JsCast;
use wasm_bindgen::closure::Closure;

use crate::{ScanOverlayState, ToastMsg};

/// How long the Prompt state waits for a scan before flipping to Timeout.
const PROMPT_TIMEOUT_MS: i32 = 30_000;

#[component]
pub fn ScanOverlay(
    scan_overlay: RwSignal<ScanOverlayState>,
    /// Reserved for future "import failed" surfacing (e.g. bytes-written
    /// but parser couldn't decode) — present so the call site doesn't
    /// need to change when we add it. `_toasts` on purpose.
    #[allow(unused_variables)]
    toasts: RwSignal<Vec<ToastMsg>>,
) -> impl IntoView {
    // Own the pending setTimeout handle so Prompt → Success (scan arrived)
    // can cancel the pending Timeout flip. Rc<Cell> because the effect
    // reads/writes it across closures.
    let pending: Rc<Cell<Option<i32>>> = Rc::new(Cell::new(None));

    // Watch state transitions: schedule a Prompt-timeout timer when we
    // enter Prompt; cancel any pending timer when we leave.
    {
        let pending = pending.clone();
        Effect::new(move |_| {
            let state = scan_overlay.get();
            // Always clear any pending timer on a state change — the only
            // state that wants one is Prompt, and if we re-enter Prompt
            // we want a fresh full-length timeout.
            if let Some(handle) = pending.take() {
                if let Some(w) = web_sys::window() {
                    w.clear_timeout_with_handle(handle);
                }
            }
            if state == ScanOverlayState::Prompt {
                let cb = Closure::once_into_js(move || {
                    // Only flip if we're still in Prompt — a scan may have
                    // landed between the timer fire and this callback.
                    if scan_overlay.get_untracked() == ScanOverlayState::Prompt {
                        scan_overlay.set(ScanOverlayState::Timeout);
                    }
                });
                let handle = web_sys::window()
                    .and_then(|w| {
                        w.set_timeout_with_callback_and_timeout_and_arguments_0(
                            cb.as_ref().unchecked_ref(),
                            PROMPT_TIMEOUT_MS,
                        )
                        .ok()
                    });
                pending.set(handle);
            }
        });
    }

    let close = move || scan_overlay.set(ScanOverlayState::Closed);
    let retry = move || scan_overlay.set(ScanOverlayState::Prompt);

    let is_open = move || scan_overlay.get() != ScanOverlayState::Closed;
    let state_class = move || match scan_overlay.get() {
        ScanOverlayState::Closed => "",
        ScanOverlayState::Prompt => "state-prompt",
        ScanOverlayState::Success { .. } => "state-success",
        ScanOverlayState::Timeout => "state-timeout",
    };

    view! {
        <Show when=is_open fallback=|| ()>
            <div class=move || format!("scan-overlay open {}", state_class())
                 role="dialog" aria-modal="true">
                <div class="scan-card">
                    // PROMPT
                    <Show
                        when=move || matches!(scan_overlay.get(), ScanOverlayState::Prompt)
                        fallback=|| ()
                    >
                        <h2 class="scan-title">"TAP YOUR SKYLANDER"</h2>
                        <p class="scan-sub">
                            "Place a figure on the reader next to the TV. \
                             We'll add it to your collection."
                        </p>
                        <div class="scan-viz">
                            <div class="scan-pulse"></div>
                            <div class="scan-pulse"></div>
                            <div class="scan-pulse"></div>
                            <div class="scan-center">
                                <div class="scan-bezel"></div>
                                <div class="scan-plate">"NFC"</div>
                            </div>
                        </div>
                        <div class="scan-actions">
                            <button
                                class="scan-btn ghost"
                                on:click=move |_| close()
                            >"CANCEL"</button>
                        </div>
                    </Show>

                    // SUCCESS
                    <Show
                        when=move || matches!(scan_overlay.get(), ScanOverlayState::Success { .. })
                        fallback=|| ()
                    >
                        <div class="scan-viz">
                            <div class="scan-center done">
                                <div class="scan-bezel"></div>
                                <div class="scan-plate">"✓"</div>
                            </div>
                        </div>
                        <p class="scan-figure-name">
                            {move || match scan_overlay.get() {
                                ScanOverlayState::Success { display_name, .. } => {
                                    display_name.to_uppercase()
                                }
                                _ => String::new(),
                            }}
                        </p>
                        <p class="scan-sub">
                            {move || match scan_overlay.get() {
                                ScanOverlayState::Success { is_duplicate: true, .. } => {
                                    "Already in your collection."
                                }
                                _ => "Added to your collection.",
                            }}
                        </p>
                        <div class="scan-actions">
                            <button
                                class="scan-btn"
                                on:click=move |_| close()
                            >"DONE"</button>
                        </div>
                    </Show>

                    // TIMEOUT
                    <Show
                        when=move || scan_overlay.get() == ScanOverlayState::Timeout
                        fallback=|| ()
                    >
                        <div class="scan-viz">
                            <div class="scan-center timed">
                                <div class="scan-bezel"></div>
                                <div class="scan-plate">"✕"</div>
                            </div>
                        </div>
                        <h2 class="scan-title">"NO FIGURE YET"</h2>
                        <p class="scan-sub">
                            "We didn't see a tap. Make sure the figure is \
                             centered on the reader and try again."
                        </p>
                        <div class="scan-actions">
                            <button
                                class="scan-btn"
                                on:click=move |_| retry()
                            >"TRY AGAIN"</button>
                            <button
                                class="scan-btn ghost"
                                on:click=move |_| close()
                            >"CANCEL"</button>
                        </div>
                    </Show>
                </div>
            </div>
        </Show>
    }
}
