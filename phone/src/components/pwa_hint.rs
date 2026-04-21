//! PwaHint — "Add to Home Screen" banner for iOS Safari (PLAN 4.18.1b).
//!
//! One-time dismissible banner shown on the profile picker when the phone
//! is running as a plain iOS Safari tab (not an installed PWA) and the
//! user hasn't previously tapped "NOT NOW". The iOS Safari chrome doesn't
//! hide unless the content overflows the viewport, so the only workable
//! fix for the cramped portrait layout is to promote the site to a
//! fullscreen PWA via Share → Add to Home Screen. `pwa::should_show_hint`
//! owns the gating logic (pure, unit-tested); this component is just the
//! rendering + dismiss plumbing.
//!
//! Extracted from `screens/profile_picker.rs` per PLAN 4.20.5a — the
//! banner is reusable in concept (any other screen could mount it during
//! the iOS-tab-without-PWA period) and matches the design_language.md
//! §6 component framing.

use leptos::prelude::*;

#[component]
pub fn PwaHint() -> impl IntoView {
    // Decision is read once on mount — the three inputs (UA, display
    // mode, localStorage) don't meaningfully change while the page is
    // alive, so re-checking each render would waste work. If the user
    // dismisses, we flip `visible` below without re-consulting the gate.
    let initial = crate::pwa::should_show_hint(
        crate::pwa::is_ios_safari(),
        crate::pwa::is_standalone(),
        crate::pwa::hint_dismissed(),
    );
    let visible = RwSignal::new(initial);

    let dismiss = move |_| {
        crate::pwa::dismiss_hint();
        visible.set(false);
    };

    view! {
        <Show when=move || visible.get() fallback=|| ()>
            <div class="pwa-hint" role="note">
                <div class="pwa-hint-body">
                    <div class="pwa-hint-title">"Pin this to your home screen"</div>
                    <div class="pwa-hint-copy">
                        "Tap "
                        <span class="pwa-hint-icon" aria-label="Share">"\u{2B06}"</span>
                        " Share, then "
                        <strong>"Add to Home Screen"</strong>
                        " for a bigger portal."
                    </div>
                </div>
                <button
                    class="pwa-hint-dismiss"
                    on:click=dismiss
                    aria-label="Dismiss install hint"
                >"NOT NOW"</button>
            </div>
        </Show>
    }
}
