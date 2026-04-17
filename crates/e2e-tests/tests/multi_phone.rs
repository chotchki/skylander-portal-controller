//! Multi-phone e2e scenarios — PLAN 3.10e.1–5 (.6 deferred with 3.10.7).
//!
//! Each test spins up 2+ `Phone` instances against the same `TestServer`.
//! Fantoccini opens a distinct browser session per `Phone::new`, and
//! chromedriver fans them out. ~2–4 Chrome instances per test; plenty on
//! modern hardware.
//!
//! Every Phone navigates to `server.phone_url()` so HMAC signing is active
//! end-to-end (see tests/hmac.rs for the signing protocol coverage).

use std::time::Duration;

use fantoccini::Locator;
use serde_json::json;

use skylander_e2e_tests::{
    Phone, TestServer, clear_eviction_cooldown, inject_load_outcomes, inject_profile,
    launch_giants, set_session_profile, unlock_default_profile, unlock_session,
};

/// Helper: open a fresh Phone against the given server.
async fn new_phone(server: &TestServer) -> Phone {
    Phone::new(&server.phone_url().await.unwrap(), &server.chromedriver_url)
        .await
        .unwrap()
}

/// Helper: poll until the phone's session id is exposed in the DOM.
async fn wait_for_session_id(phone: &Phone) -> u64 {
    for _ in 0..50 {
        if let Ok(Some(id)) = phone.session_id().await {
            return id;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    panic!("phone never received Event::Welcome");
}

// ---- 3.10e.2 concurrent_edits_both_phones --------------------------------

#[tokio::test(flavor = "current_thread")]
#[ignore = "requires chromedriver"]
async fn concurrent_edits_both_phones() {
    let server = TestServer::spawn().expect("spawn");
    unlock_default_profile(&server.url).await.unwrap();
    launch_giants(&server.url).await.unwrap();
    inject_load_outcomes(&server.url, json!([{"kind": "ok"}, {"kind": "ok"}]))
        .await
        .unwrap();

    // Two phones, same shared portal.
    let p1 = new_phone(&server).await;
    let s1 = wait_for_session_id(&p1).await;
    // Before P2 registers, re-seed pending_unlock so P2 also lands on a
    // profile (otherwise P2 starts at the ProfilePicker).
    unlock_default_profile(&server.url).await.unwrap();
    let p2 = new_phone(&server).await;
    let _s2 = wait_for_session_id(&p2).await;
    assert_ne!(
        s1,
        wait_for_session_id(&p2).await,
        "each phone must get a distinct session id"
    );

    p1.wait_for_portal(Duration::from_secs(10)).await.unwrap();
    p2.wait_for_portal(Duration::from_secs(10)).await.unwrap();

    // P1 loads a figure into slot 1.
    p1.tap_slot(1).await.unwrap();
    p1.client.find_all(Locator::Css(".card")).await.unwrap()[0]
        .clone()
        .click()
        .await
        .unwrap();

    // P2 loads a different figure into slot 2.
    p2.tap_slot(2).await.unwrap();
    p2.client.find_all(Locator::Css(".card")).await.unwrap()[1]
        .clone()
        .click()
        .await
        .unwrap();

    // Both phones should see both slots loaded via WS broadcast.
    for phone in [&p1, &p2] {
        phone
            .wait_until(Duration::from_secs(8), || async {
                let s1 = phone.slot_text(1).await.unwrap_or_default();
                let s2 = phone.slot_text(2).await.unwrap_or_default();
                !s1.is_empty()
                    && s1 != "Empty"
                    && s1 != "Loading…"
                    && !s2.is_empty()
                    && s2 != "Empty"
                    && s2 != "Loading…"
            })
            .await
            .unwrap();
    }

    p1.close().await.unwrap();
    p2.close().await.unwrap();
}

// ---- 3.10e.3 third_connection_evicts_oldest ------------------------------

#[tokio::test(flavor = "current_thread")]
#[ignore = "requires chromedriver"]
async fn third_connection_evicts_oldest() {
    let server = TestServer::spawn().expect("spawn");
    unlock_default_profile(&server.url).await.unwrap();
    launch_giants(&server.url).await.unwrap();

    let p1 = new_phone(&server).await;
    let _s1 = wait_for_session_id(&p1).await;
    unlock_default_profile(&server.url).await.unwrap();
    let p2 = new_phone(&server).await;
    let _s2 = wait_for_session_id(&p2).await;
    unlock_default_profile(&server.url).await.unwrap();

    // P3 joins — should evict P1 (the oldest).
    let p3 = new_phone(&server).await;
    let _s3 = wait_for_session_id(&p3).await;

    // P1 should flip to the Kaos "taken over" screen.
    p1.wait_until(Duration::from_secs(10), || async {
        p1.client.find(Locator::Css(".takeover")).await.is_ok()
    })
    .await
    .expect("P1 should see takeover screen");

    // P2 should be unaffected — still on the portal/game view.
    let p2_takeover = p2.client.find(Locator::Css(".takeover")).await;
    assert!(
        p2_takeover.is_err(),
        "P2 should NOT see the takeover screen"
    );

    // P3 is connected on the portal as a new session.
    p3.wait_for_portal(Duration::from_secs(10)).await.unwrap();

    p1.close().await.unwrap();
    p2.close().await.unwrap();
    p3.close().await.unwrap();
}

// ---- 3.10e.4 forced_eviction_cooldown ------------------------------------

#[tokio::test(flavor = "current_thread")]
#[ignore = "requires chromedriver"]
async fn forced_eviction_cooldown() {
    let server = TestServer::spawn().expect("spawn");
    unlock_default_profile(&server.url).await.unwrap();
    launch_giants(&server.url).await.unwrap();

    let _p1 = new_phone(&server).await;
    wait_for_session_id(&_p1).await;
    unlock_default_profile(&server.url).await.unwrap();
    let _p2 = new_phone(&server).await;
    wait_for_session_id(&_p2).await;
    unlock_default_profile(&server.url).await.unwrap();

    // P3 evicts someone; cooldown is now active on the server.
    let _p3 = new_phone(&server).await;
    wait_for_session_id(&_p3).await;

    // Raw WS connect: should get closed with an Error event before handshake
    // completes because the cooldown is still ticking.
    use futures_util::StreamExt;
    let ws_url = server.url.replace("http://", "ws://") + "/ws";
    let (mut ws, _) = tokio_tungstenite::connect_async(&ws_url)
        .await
        .expect("ws connect");
    let msg = tokio::time::timeout(Duration::from_secs(5), ws.next())
        .await
        .expect("ws first message arrives within 5s")
        .expect("some message")
        .expect("non-error message");
    let text = match msg {
        tokio_tungstenite::tungstenite::Message::Text(t) => t,
        other => panic!("expected text, got {other:?}"),
    };
    let ev: serde_json::Value = serde_json::from_str(&text).unwrap();
    assert_eq!(ev["kind"], "error", "expected Error event during cooldown");
    let msg = ev["message"].as_str().unwrap_or_default();
    assert!(
        msg.to_lowercase().contains("taken over") || msg.to_lowercase().contains("full"),
        "expected 'taken over' / 'full' message, got {msg:?}"
    );

    // Clear the cooldown and retry — should be admitted this time.
    clear_eviction_cooldown(&server.url).await.unwrap();
    let (mut ws2, _) = tokio_tungstenite::connect_async(&ws_url)
        .await
        .expect("ws reconnect");
    let msg = tokio::time::timeout(Duration::from_secs(5), ws2.next())
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    let text = match msg {
        tokio_tungstenite::tungstenite::Message::Text(t) => t,
        other => panic!("expected text, got {other:?}"),
    };
    let ev: serde_json::Value = serde_json::from_str(&text).unwrap();
    assert_eq!(
        ev["kind"], "welcome",
        "post-cooldown-clear connect should be Admitted",
    );
}

// ---- 3.10e.5 independent_profile_unlock ----------------------------------

#[tokio::test(flavor = "current_thread")]
#[ignore = "requires chromedriver"]
async fn independent_profile_unlock() {
    let server = TestServer::spawn().expect("spawn");
    // Two distinct profiles.
    let pid_a = inject_profile(&server.url, "Alpha", "1111", "#ff00ff")
        .await
        .unwrap();
    let pid_b = inject_profile(&server.url, "Beta", "2222", "#00ffff")
        .await
        .unwrap();

    // Seed P1 with profile A, P2 with profile B.
    unlock_session(&server.url, &pid_a).await.unwrap();
    let p1 = new_phone(&server).await;
    let s1 = wait_for_session_id(&p1).await;
    let p2 = new_phone(&server).await;
    let s2 = wait_for_session_id(&p2).await;

    // P2 inherited the pending_unlock (A); manually flip it to B without
    // touching P1's session.
    set_session_profile(&server.url, s2, &pid_b).await.unwrap();

    // Both phones should now show their own profile chip in the header.
    // Use `wait_until` — the `ProfileChanged` broadcast is async.
    p1.wait_until(Duration::from_secs(5), || async {
        p1.client
            .find(Locator::Css(".profile-chip"))
            .await
            .ok()
            .map(|_| true)
            .unwrap_or(false)
    })
    .await
    .unwrap();
    p2.wait_until(Duration::from_secs(5), || async {
        p2.client
            .find(Locator::Css(".profile-chip"))
            .await
            .ok()
            .map(|_| true)
            .unwrap_or(false)
    })
    .await
    .unwrap();

    let chip1 = p1
        .client
        .find(Locator::Css(".profile-chip"))
        .await
        .unwrap()
        .text()
        .await
        .unwrap();
    let chip2 = p2
        .client
        .find(Locator::Css(".profile-chip"))
        .await
        .unwrap()
        .text()
        .await
        .unwrap();

    assert!(chip1.contains("Alpha"), "P1 expected Alpha, got {chip1:?}");
    assert!(chip2.contains("Beta"), "P2 expected Beta, got {chip2:?}");
    assert_ne!(s1, s2);
    p1.close().await.unwrap();
    p2.close().await.unwrap();
}
