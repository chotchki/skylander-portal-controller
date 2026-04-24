//! Full-screen overlay shown when the phone's baked-in `BUILD_TOKEN`
//! disagrees with the server's. iOS caches the PWA's index.html +
//! hashed js/wasm at Add-to-Home-Screen time and doesn't always
//! refresh on launch, so a phone pinned a week ago talking to a
//! fresh server binary silently runs old code — which is how Chris
//! spent an afternoon chasing "the PIN entry is broken" on a build
//! that had already shipped the fix. This overlay turns that kind of
//! cache/bundle drift into a loud, obvious failure mode. Companion
//! to `PairingRequired` (bad/missing key).

use leptos::prelude::*;

#[component]
pub fn StaleVersion(
    /// True when the handshake returned a non-matching token.
    visible: Signal<bool>,
    /// Our baked-in token — shown alongside the server's so the user
    /// can see the drift at a glance.
    local: &'static str,
    /// The server's token, as it came back from `/api/version`.
    /// Optional so the overlay renders safely during first-load
    /// races where the check hasn't completed yet.
    server: Signal<Option<String>>,
) -> impl IntoView {
    let on_refresh = move |_| {
        if let Some(win) = web_sys::window() {
            let _ = win.location().reload();
        }
    };
    view! {
        <Show when=move || visible.get() fallback=|| ()>
            <div class="stale-version" role="dialog" aria-modal="true">
                <div class="stale-card">
                    <h2 class="stale-title">"APP IS OUT OF DATE"</h2>
                    <p class="stale-body">
                        "The Skylanders portal was updated since you last opened \
                         this shortcut. Refresh to load the new version."
                    </p>
                    <button class="stale-refresh-btn" on:click=on_refresh>
                        "REFRESH"
                    </button>
                    <p class="stale-hint">
                        "If the refresh doesn't stick, delete this app from your \
                         Home Screen and re-scan the QR code on the TV."
                    </p>
                    <p class="stale-diag">
                        {"this phone: "}{local}
                        <br/>
                        {"server: "}{move || server.get().unwrap_or_else(|| "(unknown)".into())}
                    </p>
                </div>
            </div>
        </Show>
    }
}
