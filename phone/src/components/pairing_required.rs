//! Full-screen overlay shown when the phone has no HMAC shared secret.
//!
//! Mutating endpoints (profile create, shutdown, launch, quit, load,
//! clear) all require a signed request. Without the key, every such
//! POST silently 401s — which reads to the user as "nothing works".
//! This overlay surfaces the state directly and tells them to scan
//! the QR on the TV to pair. PLAN 4.18.x follow-up 2026-04-24.

use leptos::prelude::*;

/// Render when `visible` is true. Self-contained — no callbacks; the
/// user re-pairs by scanning the TV's QR, which refreshes the app
/// with `#k=<hex>` in the URL, which `api::install_key_from_hash`
/// picks up on the next load.
#[component]
pub fn PairingRequired(visible: Signal<bool>) -> impl IntoView {
    view! {
        <Show when=move || visible.get() fallback=|| ()>
            <div class="pairing-required" role="dialog" aria-modal="true">
                <div class="pairing-card">
                    <h2 class="pairing-title">"PAIR YOUR PHONE"</h2>
                    <p class="pairing-body">
                        "This phone isn't connected to your Skylanders portal yet. \
                         Scan the QR code on the TV to pair."
                    </p>
                    <p class="pairing-hint">
                        "Tip: after pairing once you can add this page to your Home \
                         Screen — the shortcut will remember the pairing."
                    </p>
                </div>
            </div>
        </Show>
    }
}
