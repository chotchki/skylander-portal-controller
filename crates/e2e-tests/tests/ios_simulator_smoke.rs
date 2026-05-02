//! iOS Simulator smoke — boots one iPhone sim, loads the SPA via the
//! Bonjour URL, asserts the GamePicker renders. Mirrors the
//! chromedriver-side `tests/smoke.rs` against real WebKit (PLAN
//! 10.4.3).
//!
//! Prereqs (see `crates/e2e-tests/README.md`):
//!  - Xcode + an iOS runtime with at least one iPhone device.
//!  - `brew install ios-webkit-debug-proxy`.
//!  - `cd phone && trunk build` once so `phone/dist/` exists.
//!
//! Macos-only because the underlying `xcrun simctl` +
//! `ios-webkit-debug-proxy` only exist there. `#[ignore]` because
//! cold-booting a sim takes 10–60 s.

#![cfg(target_os = "macos")]

use std::time::Duration;

use ios_inspect::state::DeviceState;
use skylander_e2e_tests::{TestServer, unlock_default_profile};

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "boots an iOS Simulator; long-running; mac-only"]
async fn ios_smoke_game_picker_renders() {
    let server = TestServer::spawn().expect("spawn server");
    unlock_default_profile(&server.url)
        .await
        .expect("unlock default profile");

    // Boot one sim. No name → ios_inspect picks the newest available
    // Dynamic-Island iPhone runtime (same heuristic as the CLI).
    let session = ios_inspect::boot_devices(&[])
        .await
        .expect("boot iPhone sim");
    let device: &DeviceState = session.devices.first().expect("at least one device booted");

    // Use a teardown guard to ensure the sim shuts down even if the
    // assertion below panics. Captures the device label for logging.
    let _teardown = TeardownGuard {
        device_name: device.device_name.clone(),
    };

    let phone_url = server.phone_url().await.expect("phone URL with HMAC");
    ios_inspect::open_url(device, &phone_url)
        .await
        .expect("open SPA URL on sim");

    // Wait for the GamePicker to render. The phone fetches the games
    // catalogue async after WS handshake, so 30 s is generous for a
    // cold sim Safari + cold WS.
    ios_inspect::wait_for_selector(device, ".game-picker .game-card", Duration::from_secs(30))
        .await
        .expect(".game-card never appeared on the iPhone sim");

    let count = ios_inspect::query_selector_count(device, ".game-picker .game-card")
        .await
        .expect("count game cards");
    assert_eq!(
        count, 6,
        "expected six Skylanders game cards in the picker on the sim, found {count}"
    );
}

/// Drop guard that schedules `shutdown_all` on test exit (success or
/// panic). Uses `tokio::task::block_in_place` so it works inside a
/// multi-thread runtime — that's why the test is annotated with
/// `flavor = "multi_thread"`.
struct TeardownGuard {
    device_name: String,
}

impl Drop for TeardownGuard {
    fn drop(&mut self) {
        let label = self.device_name.clone();
        // `block_in_place` lets us drive an async shutdown to
        // completion without blocking the runtime's other workers.
        // Safe inside multi-thread; would panic in current-thread
        // runtimes — the test annotation pins multi-thread.
        let result = tokio::task::block_in_place(|| {
            tokio::runtime::Handle::current().block_on(ios_inspect::shutdown_all())
        });
        if let Err(e) = result {
            eprintln!("teardown for {label}: shutdown_all errored: {e}");
        }
    }
}
