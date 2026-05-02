//! iOS Simulator PwaHint gating — verifies that the "Pin this to your
//! home screen" banner shows on iPhone Safari but is suppressed on
//! iPad. Catches regressions of PLAN 9.7.1 against real WebKit.
//!
//! Background: PwaHint exists to escape iPhone Safari's bottom address
//! bar that crowds the portrait layout. iPad puts the address bar at
//! the *top* and the viewport is much larger, so the banner is mostly
//! noise on tablet — `pwa::should_show_hint` was extended to accept an
//! `is_tablet` parameter and false-out on tablet form factors. This
//! test boots both form factors and asserts the banner only renders on
//! iPhone, exactly the way users would see it.
//!
//! The bug this test catches: a previous version of `should_show_hint`
//! ignored form factor entirely and showed the banner whenever
//! `is_ios_safari && !is_standalone && !dismissed` — running this
//! test against that version would fail the iPad assertion.
//!
//! Macos-only because the underlying `xcrun simctl` +
//! `ios-webkit-debug-proxy` only exist there. `#[ignore]` because
//! cold-booting both sims takes ~30–90 s.

#![cfg(target_os = "macos")]

use std::time::Duration;

use ios_inspect::state::DeviceState;
use skylander_e2e_tests::TestServer;

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "boots two iOS Simulators; long-running; mac-only"]
async fn ios_pwa_hint_shows_on_iphone_only() {
    let server = TestServer::spawn().expect("spawn server");
    // No profile injection — both phones land on the ProfilePicker,
    // which is the screen that mounts <PwaHint />.

    let session = ios_inspect::boot_devices(&[
        "iPhone 17 Pro".to_string(),
        "iPad Pro 13-inch (M5)".to_string(),
    ])
    .await
    .expect("boot iPhone + iPad sims");

    let _teardown = TeardownGuard;

    let iphone: &DeviceState = session
        .devices
        .iter()
        .find(|d| d.device_name.to_lowercase().contains("iphone"))
        .expect("iPhone sim in session");
    let ipad: &DeviceState = session
        .devices
        .iter()
        .find(|d| d.device_name.to_lowercase().contains("ipad"))
        .expect("iPad sim in session");

    let phone_url = server.phone_url().await.expect("phone URL with HMAC");

    ios_inspect::open_url(iphone, &phone_url)
        .await
        .expect("open SPA on iPhone");
    ios_inspect::open_url(ipad, &phone_url)
        .await
        .expect("open SPA on iPad");

    // Wait for the ProfilePicker container to render on each device —
    // that's the host of the conditional PwaHint. After it renders,
    // the hint either is or isn't in the DOM (decided once on mount,
    // see PwaHint::initial signal).
    ios_inspect::wait_for_selector(iphone, ".profile-picker", Duration::from_secs(30))
        .await
        .expect("iPhone profile picker renders");
    ios_inspect::wait_for_selector(ipad, ".profile-picker", Duration::from_secs(30))
        .await
        .expect("iPad profile picker renders");

    // Give Leptos one extra render tick so any post-mount effects
    // settle. `should_show_hint` is computed synchronously on mount
    // so this is paranoia, but the cost is 200 ms.
    tokio::time::sleep(Duration::from_millis(200)).await;

    let iphone_hint_count = ios_inspect::query_selector_count(iphone, ".pwa-hint")
        .await
        .expect("count .pwa-hint on iPhone");
    let ipad_hint_count = ios_inspect::query_selector_count(ipad, ".pwa-hint")
        .await
        .expect("count .pwa-hint on iPad");

    assert_eq!(
        iphone_hint_count, 1,
        "iPhone should render exactly one .pwa-hint banner, found {iphone_hint_count}"
    );
    assert_eq!(
        ipad_hint_count, 0,
        "iPad should suppress the .pwa-hint banner (PLAN 9.7.1), found {ipad_hint_count}"
    );
}

/// Drop guard mirroring the other iOS tests — schedules
/// `shutdown_all` on test exit (success or panic).
struct TeardownGuard;

impl Drop for TeardownGuard {
    fn drop(&mut self) {
        let result = tokio::task::block_in_place(|| {
            tokio::runtime::Handle::current().block_on(ios_inspect::shutdown_all())
        });
        if let Err(e) = result {
            eprintln!("teardown: shutdown_all errored: {e}");
        }
    }
}
