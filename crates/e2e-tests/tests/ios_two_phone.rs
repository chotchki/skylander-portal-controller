//! iOS Simulator two-phone — boots iPad + iPhone simultaneously,
//! drives the SPA on each, asserts each phone gets its own server
//! session + profile chip. Mirrors the chromedriver-side
//! `multi_phone.rs::independent_profile_unlock` against real WebKit
//! on real iOS form factors (PLAN 10.4.4).
//!
//! Prereqs (see `crates/e2e-tests/README.md`):
//!  - Xcode + an iOS runtime with both an iPhone and an iPad device
//!    available.
//!  - `brew install ios-webkit-debug-proxy`.
//!  - `cd phone && trunk build` once so `phone/dist/` exists.
//!
//! Macos-only because the underlying `xcrun simctl` +
//! `ios-webkit-debug-proxy` only exist there. `#[ignore]` because
//! cold-booting both sims takes ~30–90 s and the test then drives
//! both Safari processes through the unlock + profile-chip
//! propagation flow.

#![cfg(target_os = "macos")]

use std::time::Duration;

use ios_inspect::state::DeviceState;
use skylander_e2e_tests::{TestServer, inject_profile, set_session_profile, unlock_session};

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "boots two iOS Simulators; long-running; mac-only"]
async fn ios_two_phone_independent_profiles() {
    let server = TestServer::spawn().expect("spawn server");

    // Two distinct profiles — same shape as multi_phone.rs's
    // independent_profile_unlock chromedriver test.
    let pid_alpha = inject_profile(&server.url, "Alpha", "1111", "#ff00ff")
        .await
        .expect("inject Alpha");
    let pid_beta = inject_profile(&server.url, "Beta", "2222", "#00ffff")
        .await
        .expect("inject Beta");

    // Seed pending_unlock for the next-connecting phone (gets Alpha).
    unlock_session(&server.url, &pid_alpha)
        .await
        .expect("seed pending_unlock for Alpha");

    // Boot both sims. Order matters for socket attribution: each
    // device claims the new webinspectord_sim socket that appears
    // after its boot, so booting sequentially via boot_devices is
    // correct.
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

    // Open the SPA on both. iPhone claims the `pending_unlock` (Alpha)
    // since it connects first; iPad inherits it temporarily but we
    // override below with set_session_profile.
    ios_inspect::open_url(iphone, &phone_url)
        .await
        .expect("open SPA on iPhone");
    ios_inspect::open_url(ipad, &phone_url)
        .await
        .expect("open SPA on iPad");

    // Wait for both phones to receive Event::Welcome (which exposes
    // `data-session-id` on body).
    let s_iphone = ios_inspect::wait_for_session_id(iphone, Duration::from_secs(45))
        .await
        .expect("iPhone session id");
    let s_ipad = ios_inspect::wait_for_session_id(ipad, Duration::from_secs(45))
        .await
        .expect("iPad session id");
    assert_ne!(
        s_iphone, s_ipad,
        "iPhone and iPad should be assigned distinct session ids"
    );

    // Bind iPad to Beta. iPhone keeps Alpha (it consumed
    // pending_unlock at connect time).
    set_session_profile(&server.url, s_ipad, &pid_beta)
        .await
        .expect("flip iPad to Beta");

    // Both header chips should populate after the ProfileChanged
    // broadcast lands — wait for the selector first, then read the
    // text. Use the modern `.header-profile-name` (the name span
    // inside `.header-identity`) — it's the post-design-system
    // equivalent of the legacy `.profile-chip` selector.
    ios_inspect::wait_for_selector(
        iphone,
        ".header-identity .header-profile-name",
        Duration::from_secs(15),
    )
    .await
    .expect("iPhone header profile name renders");
    ios_inspect::wait_for_selector(
        ipad,
        ".header-identity .header-profile-name",
        Duration::from_secs(15),
    )
    .await
    .expect("iPad header profile name renders");

    let iphone_name =
        ios_inspect::query_selector_text(iphone, ".header-identity .header-profile-name")
            .await
            .expect("iPhone profile name text")
            .unwrap_or_default();
    let ipad_name = ios_inspect::query_selector_text(ipad, ".header-identity .header-profile-name")
        .await
        .expect("iPad profile name text")
        .unwrap_or_default();

    assert!(
        iphone_name.contains("Alpha"),
        "iPhone expected Alpha, got {iphone_name:?}"
    );
    assert!(
        ipad_name.contains("Beta"),
        "iPad expected Beta, got {ipad_name:?}"
    );
}

/// Drop guard mirroring `ios_simulator_smoke.rs` — schedules
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
