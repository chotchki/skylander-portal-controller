//! Rpcs3SettingsOverlay — full-screen "configuring on the TV…" cover
//! (PLAN 16.9.3).
//!
//! Shown while RPCS3's settings GUI is open on the TV for per-game config
//! (driven by the WS `Event::Rpcs3SettingsChanged { open }` → the
//! `rpcs3_settings_open` signal). The portal is unavailable for the duration —
//! a grown-up is editing a game's Custom Configuration on the HTPC with the
//! keyboard/mouse — so the phone blocks interaction and points the user at the
//! TV. There's no phone-side action: the overlay dismisses itself when RPCS3 is
//! closed on the TV and the server broadcasts `open: false`.

use leptos::prelude::*;

use crate::components::{DisplayHeading, HeadingSize};

#[component]
pub(crate) fn Rpcs3SettingsOverlay(visible: RwSignal<bool>) -> impl IntoView {
    view! {
        <Show when=move || visible.get() fallback=|| ()>
            <section class="rpcs3-settings-overlay">
                <div class="rpcs3-settings-backdrop"></div>
                <div class="rpcs3-settings-viewport">
                    <div class="rpcs3-settings-mark">{"\u{1F5A5}"}</div>

                    <DisplayHeading size=HeadingSize::Lg>
                        "CONFIGURING ON THE TV"
                    </DisplayHeading>

                    <p class="rpcs3-settings-body">
                        "RPCS3\u{2019}s settings are open on the TV. A grown-up can tune \
                         this game there \u{2014} then close RPCS3 to come back to the portal."
                    </p>
                </div>
            </section>
        </Show>
    }
}
