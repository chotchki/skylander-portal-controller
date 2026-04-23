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

// ---- 3.10e.6 ownership_pip_shows_correct_owner_per_slot ------------------
//
// Companion to 3.10.7's aesthetic pass. Two profiles with distinct colours
// each place a figure; every connected phone should see each slot's pip
// render the placing-profile's initial + colour, so a mixed 2-player
// session can tell whose figure is whose at a glance.
//
// Uses current `.p4-*` selectors directly rather than going through the
// stale `.portal .slot` helpers — 4.16.1 owns the broader selector
// migration.

#[tokio::test(flavor = "current_thread")]
#[ignore = "requires chromedriver"]
async fn ownership_pip_shows_correct_owner_per_slot() {
    let server = TestServer::spawn().expect("spawn");
    launch_giants(&server.url).await.unwrap();

    // Two profiles with distinct, recognisable colours so we can
    // substring-match the CSS custom-property echo.
    let alice_id = inject_profile(&server.url, "Alice", "1111", "#ff6b2a")
        .await
        .unwrap();
    let bob_id = inject_profile(&server.url, "Bob", "2222", "#5ac96b")
        .await
        .unwrap();

    unlock_session(&server.url, &alice_id).await.unwrap();
    let p1 = new_phone(&server).await;
    let s1 = wait_for_session_id(&p1).await;
    let p2 = new_phone(&server).await;
    let s2 = wait_for_session_id(&p2).await;

    // Rebind each session to its owner explicitly — pending_unlock
    // only drives the first-to-register phone (see independent_profile_unlock).
    set_session_profile(&server.url, s1, &alice_id).await.unwrap();
    set_session_profile(&server.url, s2, &bob_id).await.unwrap();

    // Two successful loads — one per slot, same mock outcome.
    inject_load_outcomes(&server.url, json!([{"kind": "ok"}, {"kind": "ok"}]))
        .await
        .unwrap();

    // Wait for both phones to reach the portal grid.
    for phone in [&p1, &p2] {
        phone
            .wait_for(Locator::Css(".portal-p4"), Duration::from_secs(10))
            .await
            .unwrap();
    }

    // P1 (Alice) places into slot 1; P2 (Bob) places into slot 2.
    let p1_slots = p1.client.find_all(Locator::Css(".p4-slot")).await.unwrap();
    p1_slots[0].clone().click().await.unwrap();
    p1.client.find_all(Locator::Css(".fig-card-p4")).await.unwrap()[0]
        .clone()
        .click()
        .await
        .unwrap();

    let p2_slots = p2.client.find_all(Locator::Css(".p4-slot")).await.unwrap();
    p2_slots[1].clone().click().await.unwrap();
    p2.client.find_all(Locator::Css(".fig-card-p4")).await.unwrap()[1]
        .clone()
        .click()
        .await
        .unwrap();

    // Poll until both phones see both ownership plates settled (not pending).
    // The `--pending` class is stripped once the slot flips from Loading
    // to Loaded; if either slot stays in-flight the assertion would fire
    // against transient state.
    for phone in [&p1, &p2] {
        phone
            .wait_until(Duration::from_secs(10), || async {
                let plates = phone
                    .client
                    .find_all(Locator::Css(".p4-slot-owner:not(.p4-slot-owner--pending) .p4-slot-owner-plate"))
                    .await
                    .unwrap_or_default();
                plates.len() >= 2
            })
            .await
            .unwrap();
    }

    // Each connected phone should see both pips with the correct owner
    // initial and tinted plate — ownership follows the placing profile,
    // not the viewing phone.
    for (name, phone) in [("P1", &p1), ("P2", &p2)] {
        let slot1_owner = phone
            .client
            .find(Locator::Css(".p4-slot:nth-child(1) .p4-slot-owner"))
            .await
            .unwrap_or_else(|_| panic!("{name}: slot 1 ownership pip should exist"));
        let slot1_style = slot1_owner.attr("style").await.unwrap().unwrap_or_default();
        let slot1_initial = phone
            .client
            .find(Locator::Css(".p4-slot:nth-child(1) .p4-slot-owner-plate"))
            .await
            .unwrap()
            .text()
            .await
            .unwrap();
        assert_eq!(
            slot1_initial.trim(),
            "A",
            "{name}: slot 1 initial expected Alice's A, got {slot1_initial:?}",
        );
        assert!(
            slot1_style.to_ascii_lowercase().contains("#ff6b2a"),
            "{name}: slot 1 style should carry Alice's #ff6b2a; got {slot1_style:?}",
        );

        let slot2_owner = phone
            .client
            .find(Locator::Css(".p4-slot:nth-child(2) .p4-slot-owner"))
            .await
            .unwrap_or_else(|_| panic!("{name}: slot 2 ownership pip should exist"));
        let slot2_style = slot2_owner.attr("style").await.unwrap().unwrap_or_default();
        let slot2_initial = phone
            .client
            .find(Locator::Css(".p4-slot:nth-child(2) .p4-slot-owner-plate"))
            .await
            .unwrap()
            .text()
            .await
            .unwrap();
        assert_eq!(
            slot2_initial.trim(),
            "B",
            "{name}: slot 2 initial expected Bob's B, got {slot2_initial:?}",
        );
        assert!(
            slot2_style.to_ascii_lowercase().contains("#5ac96b"),
            "{name}: slot 2 style should carry Bob's #5ac96b; got {slot2_style:?}",
        );
    }

    p1.close().await.unwrap();
    p2.close().await.unwrap();
}

// ---- 3.10.9 disconnect_clears_departing_profile_slots --------------------
//
// Simple MVP 2-player disconnect policy: when Phone A drops, any slots
// they placed figures in clear; Phone B's figures stay. Exercises the
// `state.rs::flip_loaded_owned_to_loading` free fn end-to-end through
// the WS-close hook in `http.rs::ws_handler`.

#[tokio::test(flavor = "current_thread")]
#[ignore = "requires chromedriver"]
async fn disconnect_clears_departing_profile_slots() {
    let server = TestServer::spawn().expect("spawn");
    launch_giants(&server.url).await.unwrap();

    let alice_id = inject_profile(&server.url, "Alice", "1111", "#ff6b2a")
        .await
        .unwrap();
    let bob_id = inject_profile(&server.url, "Bob", "2222", "#5ac96b")
        .await
        .unwrap();

    unlock_session(&server.url, &alice_id).await.unwrap();
    let p1 = new_phone(&server).await;
    let s1 = wait_for_session_id(&p1).await;
    let p2 = new_phone(&server).await;
    let s2 = wait_for_session_id(&p2).await;
    set_session_profile(&server.url, s1, &alice_id).await.unwrap();
    set_session_profile(&server.url, s2, &bob_id).await.unwrap();

    inject_load_outcomes(&server.url, json!([{"kind": "ok"}, {"kind": "ok"}]))
        .await
        .unwrap();

    for phone in [&p1, &p2] {
        phone
            .wait_for(Locator::Css(".portal-p4"), Duration::from_secs(10))
            .await
            .unwrap();
    }

    // Alice places in slot 1; Bob places in slot 2.
    let p1_slots = p1.client.find_all(Locator::Css(".p4-slot")).await.unwrap();
    p1_slots[0].clone().click().await.unwrap();
    p1.client.find_all(Locator::Css(".fig-card-p4")).await.unwrap()[0]
        .clone()
        .click()
        .await
        .unwrap();

    let p2_slots = p2.client.find_all(Locator::Css(".p4-slot")).await.unwrap();
    p2_slots[1].clone().click().await.unwrap();
    p2.client.find_all(Locator::Css(".fig-card-p4")).await.unwrap()[1]
        .clone()
        .click()
        .await
        .unwrap();

    // Wait for both slots to settle Loaded on P2 (the witness).
    p2.wait_until(Duration::from_secs(10), || async {
        let loaded_owners = p2
            .client
            .find_all(Locator::Css(".p4-slot-owner:not(.p4-slot-owner--pending)"))
            .await
            .unwrap_or_default();
        loaded_owners.len() >= 2
    })
    .await
    .unwrap();

    // Alice disconnects.
    p1.close().await.unwrap();

    // P2 should see slot 1 go Empty while slot 2 (Bob's) stays Loaded.
    // The driver-side ClearSlot round-trip has to complete, so poll
    // with a generous timeout — mock driver's 50ms default latency
    // puts this in the ~200ms range but CI loads vary.
    p2.wait_until(Duration::from_secs(10), || async {
        let s1 = p2
            .client
            .find(Locator::Css(".p4-slot:nth-child(1)"))
            .await;
        if let Ok(slot) = s1 {
            let cls = slot.attr("class").await.unwrap().unwrap_or_default();
            if !cls.contains("p4-slot--empty") {
                return false;
            }
        } else {
            return false;
        }
        // Bob's slot 2 should still be Loaded.
        let s2 = p2
            .client
            .find(Locator::Css(".p4-slot:nth-child(2)"))
            .await;
        if let Ok(slot) = s2 {
            let cls = slot.attr("class").await.unwrap().unwrap_or_default();
            cls.contains("p4-slot--loaded")
        } else {
            false
        }
    })
    .await
    .expect("slot 1 should empty on Alice disconnect; slot 2 (Bob) should stay loaded");

    p2.close().await.unwrap();
}
