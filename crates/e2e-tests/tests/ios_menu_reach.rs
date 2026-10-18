//! openspec AA.20 on real WebKit: the kebab menu in the iOS Simulator's
//! Safari, a browser tab with its toolbar on screen (not the installed PWA),
//! with a profile open and a game running (the longest menu). Every action,
//! HOLD TO SHUT DOWN last, has to come fully inside the visible viewport once
//! a finger scrolls to it, with a tap at its centre landing on it. Chrome's
//! phone emulation runs the same probe in `smoke.rs`
//! (`every_menu_action_is_reachable_in_a_short_viewport`).
//!
//! Prereqs as `ios_simulator_smoke.rs`: Xcode with an iPhone runtime,
//! `brew install ios-webkit-debug-proxy` and a built `phone/dist/`. Runs
//! against either app (the phone is the same).

#![cfg(target_os = "macos")]

use std::time::Duration;

use ios_inspect::state::DeviceState;
use skylander_e2e_tests::{
    E2eApp, MENU_REACH_JS, TestServer, launch_giants, unlock_default_profile,
};

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "boots an iOS Simulator; long-running; mac-only"]
async fn ios_menu_actions_reachable_with_safari_toolbar() {
    let server = TestServer::spawn().expect("spawn server");
    unlock_default_profile(&server.url)
        .await
        .expect("unlock default profile");
    launch_giants(&server.url).await.expect("launch Giants");

    let session = ios_inspect::boot_devices(&[])
        .await
        .expect("boot iPhone sim");
    let device: &DeviceState = session.devices.first().expect("a booted device");
    let _teardown = TeardownGuard;

    let phone_url = server.phone_url().await.expect("phone URL with HMAC");
    ios_inspect::open_url(device, &phone_url)
        .await
        .expect("open the phone in Safari");
    ios_inspect::wait_for_selector(device, ".portal-p4", Duration::from_secs(60))
        .await
        .expect("the portal on the sim");
    ios_inspect::eval_js(device, "document.querySelector('.kebab-btn').click(), true")
        .await
        .expect("tap the kebab");
    ios_inspect::wait_for_selector(device, ".menu-overlay-panel", Duration::from_secs(10))
        .await
        .expect("the menu");
    // The open animation (0.35 s) scales the panel.
    tokio::time::sleep(Duration::from_millis(800)).await;

    let read = ios_inspect::eval_js(device, MENU_REACH_JS)
        .await
        .expect("probe the menu");
    let reach: serde_json::Value =
        serde_json::from_str(read.as_str().expect("a JSON string")).expect("parse the probe");
    eprintln!(
        "{:?} on {} ({}): {reach}",
        E2eApp::from_env(),
        device.device_name,
        device.runtime
    );

    assert_eq!(
        reach["standalone"], false,
        "a Safari tab, not the PWA: {reach}"
    );
    let height = reach["visual_height"]
        .as_f64()
        .or_else(|| reach["viewport"][1].as_f64())
        .unwrap();
    let screen = reach["screen"][1].as_f64().unwrap();
    assert!(
        height < screen,
        "Safari's toolbar should take some of the {screen} pt screen: {reach}"
    );
    let width = reach["viewport"][0].as_f64().unwrap();
    let inside = |r: &serde_json::Value| {
        let r: Vec<f64> = serde_json::from_value(r.clone()).unwrap();
        r[0] >= 0.0 && r[1] >= 0.0 && r[2] <= width && r[3] <= height
    };
    let actions = reach["actions"].as_array().unwrap();
    let last = actions.last().expect("menu actions");
    assert_eq!(last["title"], "HOLD TO SHUT DOWN", "{reach}");
    for a in actions {
        assert!(
            inside(&a["rect"]),
            "{a} isn't inside the visible {width}x{height}"
        );
        assert_eq!(a["tappable"], true, "a tap misses {a}");
    }
}

/// Shuts down the simulator ios-inspect booted, panic or not.
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
