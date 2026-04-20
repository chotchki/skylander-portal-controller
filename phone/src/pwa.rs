//! PWA install-hint detection (PLAN 4.18.1b).
//!
//! The phone runs in two modes on iOS:
//!   * **Browser tab** — loaded from the TV QR or a typed URL. iOS Safari
//!     keeps its chrome pinned (address bar + toolbar) because our app
//!     doesn't overflow its viewport, so roughly 25% of vertical real
//!     estate is permanently unusable. The only workable fix is Share →
//!     Add to Home Screen, which promotes the site to a fullscreen PWA.
//!   * **Installed PWA** — launched from the home-screen icon. iOS Safari
//!     drops its chrome and the app gets the full viewport.
//!
//! Kids can't be expected to know the install ritual, so the ProfilePicker
//! surfaces a one-time banner explaining it. That banner must skip:
//!   - already-installed PWAs (no point; they're in the good state)
//!   - non-iOS browsers (Android Chrome has its own install prompt)
//!   - users who've already dismissed it (persisted in `localStorage`)
//!
//! The real detection calls into `web_sys`, but the decision logic is
//! split into pure helpers so the truth table can be unit-tested
//! without standing up a wasm-bindgen runtime.

use wasm_bindgen::JsValue;

const DISMISS_KEY: &str = "skylander-pwa-hint-dismissed";

/// Combined predicate: should the ProfilePicker render the install banner?
/// Pure — the three inputs come from thin wrappers around `web_sys` below.
pub(crate) fn should_show_hint(
    is_ios_safari: bool,
    is_standalone: bool,
    dismissed: bool,
) -> bool {
    is_ios_safari && !is_standalone && !dismissed
}

/// Rough iOS-Safari classification from a user-agent string + iPad-Pro
/// hint. iPad-Pro running iOS 13+ reports a Mac user-agent
/// (`Macintosh; Intel Mac OS X`) for compat, so the only reliable way to
/// distinguish it from a real Mac is a secondary touch-points probe —
/// callers pass `is_touch_mac = navigator.maxTouchPoints > 1`.
///
/// Excludes alternate iOS browsers whose UAs carry `CriOS` / `FxiOS` /
/// `EdgiOS`. They also use WebKit and the Share sheet works identically,
/// but their PWA-install flow differs, so the banner copy (which is
/// Safari-specific) shouldn't target them.
pub(crate) fn classify_ua(user_agent: &str, is_touch_mac: bool) -> bool {
    let ua = user_agent.to_lowercase();
    let is_ios_device =
        ua.contains("iphone") || ua.contains("ipod") || ua.contains("ipad");
    let is_ipad_pro_as_mac = ua.contains("macintosh") && is_touch_mac;
    let is_apple_mobile = is_ios_device || is_ipad_pro_as_mac;
    if !is_apple_mobile {
        return false;
    }
    // Exclude alternate iOS browsers.
    !ua.contains("crios") && !ua.contains("fxios") && !ua.contains("edgios")
}

// ---- web_sys wrappers (not unit-tested; exercised on-device) ----

/// Launched from the home-screen icon (iOS PWA) OR running in any
/// browser's standalone display mode (Android Chrome TWA, desktop PWA).
pub(crate) fn is_standalone() -> bool {
    let Some(win) = web_sys::window() else {
        return false;
    };
    // Modern: display-mode media query. Android + desktop Chrome + iOS
    // Safari ≥17 all report this correctly.
    if let Ok(Some(mql)) = win.match_media("(display-mode: standalone)") {
        if mql.matches() {
            return true;
        }
    }
    // iOS legacy: `navigator.standalone`. Not in the WHATWG Navigator IDL,
    // so web-sys doesn't expose it — reach via Reflect. This is the only
    // signal pre-iOS-17 and still the most reliable.
    let nav = win.navigator();
    if let Ok(value) = js_sys::Reflect::get(&nav, &JsValue::from_str("standalone")) {
        if value.as_bool().unwrap_or(false) {
            return true;
        }
    }
    false
}

pub(crate) fn is_ios_safari() -> bool {
    let Some(win) = web_sys::window() else {
        return false;
    };
    let nav = win.navigator();
    let Ok(ua) = nav.user_agent() else {
        return false;
    };
    let is_touch_mac = nav.max_touch_points() > 1;
    classify_ua(&ua, is_touch_mac)
}

pub(crate) fn hint_dismissed() -> bool {
    let Some(store) = web_sys::window().and_then(|w| w.local_storage().ok().flatten()) else {
        return false;
    };
    matches!(store.get_item(DISMISS_KEY), Ok(Some(_)))
}

pub(crate) fn dismiss_hint() {
    if let Some(store) = web_sys::window().and_then(|w| w.local_storage().ok().flatten()) {
        let _ = store.set_item(DISMISS_KEY, "1");
    }
}

#[cfg(test)]
mod tests {
    //! Truth-table coverage for `should_show_hint` and `classify_ua`.
    //! The `web_sys` wrappers above call live browser APIs and are
    //! covered by on-device smoke; these pure helpers carry the
    //! decision logic and are the part most likely to regress silently.
    use super::*;

    // UA samples captured from real devices / public references.
    // Keep these as consts so adding a new case is a one-liner.
    const UA_IPHONE_SAFARI: &str =
        "Mozilla/5.0 (iPhone; CPU iPhone OS 17_4 like Mac OS X) AppleWebKit/605.1.15 \
         (KHTML, like Gecko) Version/17.4 Mobile/15E148 Safari/604.1";
    const UA_IPAD_SAFARI: &str =
        "Mozilla/5.0 (iPad; CPU OS 16_6 like Mac OS X) AppleWebKit/605.1.15 \
         (KHTML, like Gecko) Version/16.6 Mobile/15E148 Safari/604.1";
    const UA_IPAD_PRO_AS_MAC: &str =
        "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/605.1.15 \
         (KHTML, like Gecko) Version/17.4 Safari/605.1.15";
    const UA_IOS_CHROME: &str =
        "Mozilla/5.0 (iPhone; CPU iPhone OS 17_4 like Mac OS X) AppleWebKit/605.1.15 \
         (KHTML, like Gecko) CriOS/122.0.6261.89 Mobile/15E148 Safari/604.1";
    const UA_DESKTOP_SAFARI: &str =
        "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/605.1.15 \
         (KHTML, like Gecko) Version/17.4 Safari/605.1.15";
    const UA_ANDROID_CHROME: &str =
        "Mozilla/5.0 (Linux; Android 14; Pixel 8) AppleWebKit/537.36 (KHTML, like Gecko) \
         Chrome/122.0.0.0 Mobile Safari/537.36";

    #[test]
    fn classify_ua_matches_iphone_safari() {
        assert!(classify_ua(UA_IPHONE_SAFARI, false));
    }

    #[test]
    fn classify_ua_matches_ipad_safari() {
        assert!(classify_ua(UA_IPAD_SAFARI, true));
    }

    #[test]
    fn classify_ua_matches_ipad_pro_via_touch_probe() {
        // iPadOS 13+ masquerades as Mac. The banner should still show
        // because the install ritual is identical to iPad Safari.
        assert!(classify_ua(UA_IPAD_PRO_AS_MAC, true));
    }

    #[test]
    fn classify_ua_rejects_desktop_mac_safari() {
        // Same UA as iPad Pro but no touch — it's a real Mac, where
        // add-to-home-screen doesn't apply.
        assert!(!classify_ua(UA_IPAD_PRO_AS_MAC, false));
        assert!(!classify_ua(UA_DESKTOP_SAFARI, false));
    }

    #[test]
    fn classify_ua_rejects_ios_chrome() {
        // iOS Chrome is WebKit under the hood but has its own UX for
        // install prompts and our copy says "tap Share" — wrong button.
        assert!(!classify_ua(UA_IOS_CHROME, false));
    }

    #[test]
    fn classify_ua_rejects_android_chrome() {
        // Android Chrome shows its own install banner; don't stack a
        // second one.
        assert!(!classify_ua(UA_ANDROID_CHROME, false));
    }

    #[test]
    fn should_show_hint_true_only_for_ios_safari_browser_not_dismissed() {
        assert!(should_show_hint(true, false, false));
    }

    #[test]
    fn should_show_hint_false_when_already_installed() {
        // Installed PWA → no nag; user already did the thing.
        assert!(!should_show_hint(true, true, false));
    }

    #[test]
    fn should_show_hint_false_when_dismissed() {
        // User tapped "not now" — stop asking.
        assert!(!should_show_hint(true, false, true));
    }

    #[test]
    fn should_show_hint_false_on_non_ios_safari() {
        // Android Chrome etc. get their own native install UX.
        assert!(!should_show_hint(false, false, false));
    }
}
