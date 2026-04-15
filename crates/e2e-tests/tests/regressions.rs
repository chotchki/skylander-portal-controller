//! Phase 3 regression scenarios (PLAN 3.6).
//!
//! Every test is `#[ignore]`-gated; run with:
//!
//!   cargo test -p skylander-e2e-tests --test regressions -- --ignored --nocapture
//!
//! Prerequisites: chromedriver running at http://localhost:4444, phone SPA
//! built (`cd phone && trunk build`). See crates/e2e-tests/README.md.

use std::time::Duration;

use fantoccini::Locator;
use serde_json::json;

use skylander_e2e_tests::{
    inject_load_outcomes, launch_giants, unlock_default_profile, Phone, TestServer,
};

// ---- Test 3.6.1: spam_click_same_slot -------------------------------------

#[tokio::test(flavor = "current_thread")]
#[ignore = "requires chromedriver"]
async fn spam_click_same_slot() {
    let server = TestServer::spawn().expect("spawn");
    unlock_default_profile(&server.url).await.unwrap();
    launch_giants(&server.url).await.unwrap();

    // Queue exactly ONE Ok outcome so we can detect extra loads via the
    // server running out of injected outcomes and falling back to the
    // normal mock path (which would also succeed — so we assert on the
    // single SlotChanged broadcast instead by watching the slot text).
    inject_load_outcomes(&server.url, json!([{"kind": "ok"}])).await.unwrap();

    let phone = Phone::new(&server.url, &server.chromedriver_url).await.unwrap();
    phone.wait_for_portal(Duration::from_secs(10)).await.unwrap();
    phone.tap_slot(1).await.unwrap();

    // Rapid-fire five clicks on the first card.
    let cards = phone.client.find_all(Locator::Css(".card")).await.unwrap();
    let first = cards.first().expect("at least one figure");
    for _ in 0..5 {
        let _ = first.clone().click().await;
    }

    // Wait for the slot to end up Loaded.
    phone
        .wait_until(Duration::from_secs(5), || async {
            phone
                .slot_text(1)
                .await
                .map(|t| !t.is_empty() && t != "Empty" && t != "Loading…")
                .unwrap_or(false)
        })
        .await
        .unwrap();

    // At most one toast (and ideally zero — back-pressure should be silent).
    let toasts = phone.toast_count().await.unwrap();
    if toasts > 1 {
        let all = phone.client.find_all(Locator::Css(".toast")).await.unwrap();
        let mut texts = Vec::new();
        for t in all {
            texts.push(t.text().await.unwrap_or_default());
        }
        panic!("expected <=1 toast, got {toasts}: {texts:?}");
    }

    phone.close().await.unwrap();
}

// ---- Test 3.6.2: dup_figure_across_slots ----------------------------------

#[tokio::test(flavor = "current_thread")]
#[ignore = "requires chromedriver"]
async fn dup_figure_across_slots() {
    let server = TestServer::spawn().expect("spawn");
    unlock_default_profile(&server.url).await.unwrap();
    launch_giants(&server.url).await.unwrap();

    // First load OK, second load simulates the Windows "file in use" path.
    inject_load_outcomes(
        &server.url,
        json!([
            { "kind": "ok" },
            { "kind": "file_in_use", "message": "This file is in use." },
        ]),
    )
    .await
    .unwrap();

    let phone = Phone::new(&server.url, &server.chromedriver_url).await.unwrap();
    phone.wait_for_portal(Duration::from_secs(10)).await.unwrap();
    phone.search("Spyro").await.unwrap();
    tokio::time::sleep(Duration::from_millis(300)).await;
    phone.tap_slot(1).await.unwrap();

    // Click the first matching "Spyro" card.
    let card = phone.client.find(Locator::Css(".card")).await.unwrap();
    card.click().await.unwrap();

    phone
        .wait_until(Duration::from_secs(5), || async {
            phone
                .slot_text(1)
                .await
                .map(|t| t.to_lowercase().contains("spyro"))
                .unwrap_or(false)
        })
        .await
        .unwrap();

    // That card should now render with the "on-portal" class.
    let class = phone
        .client
        .find(Locator::Css(".card"))
        .await
        .unwrap()
        .attr("class")
        .await
        .unwrap()
        .unwrap_or_default();
    assert!(
        class.contains("on-portal"),
        "expected card to be marked on-portal; class = {class:?}",
    );

    // Tapping the disabled card produces a toast and doesn't populate slot 2.
    phone.tap_slot(2).await.unwrap();
    let _ = phone
        .client
        .find(Locator::Css(".card"))
        .await
        .unwrap()
        .click()
        .await;
    tokio::time::sleep(Duration::from_millis(400)).await;
    let slot2 = phone.slot_text(2).await.unwrap();
    assert_eq!(slot2, "Empty", "slot 2 should stay empty, got {slot2:?}");

    phone.close().await.unwrap();
}

// ---- Test 3.6.3: clear_then_load_sequence ---------------------------------

#[tokio::test(flavor = "current_thread")]
#[ignore = "requires chromedriver"]
async fn clear_then_load_sequence() {
    let server = TestServer::spawn().expect("spawn");
    unlock_default_profile(&server.url).await.unwrap();
    launch_giants(&server.url).await.unwrap();
    inject_load_outcomes(
        &server.url,
        json!([{"kind": "ok"}, {"kind": "ok"}]),
    )
    .await
    .unwrap();

    let phone = Phone::new(&server.url, &server.chromedriver_url).await.unwrap();
    phone.wait_for_portal(Duration::from_secs(10)).await.unwrap();
    phone.tap_slot(1).await.unwrap();
    let cards = phone.client.find_all(Locator::Css(".card")).await.unwrap();
    cards[0].clone().click().await.unwrap();
    phone
        .wait_until(Duration::from_secs(5), || async {
            phone.slot_text(1).await.map(|t| t != "Empty" && t != "Loading…").unwrap_or(false)
        })
        .await
        .unwrap();

    // Remove.
    let remove = phone
        .client
        .find(Locator::Css(".portal .slot .slot-btn.danger"))
        .await
        .unwrap();
    remove.click().await.unwrap();
    phone
        .wait_until(Duration::from_secs(5), || async {
            phone.slot_text(1).await.map(|t| t == "Empty").unwrap_or(false)
        })
        .await
        .unwrap();

    // Load a different figure.
    phone.tap_slot(1).await.unwrap();
    let cards = phone.client.find_all(Locator::Css(".card")).await.unwrap();
    cards[1].clone().click().await.unwrap();
    phone
        .wait_until(Duration::from_secs(5), || async {
            phone
                .slot_text(1)
                .await
                .map(|t| t != "Empty" && t != "Loading…")
                .unwrap_or(false)
        })
        .await
        .unwrap();

    phone.close().await.unwrap();
}

// ---- Test 3.6.4: error_toast_never_populates_slot -------------------------

#[tokio::test(flavor = "current_thread")]
#[ignore = "requires chromedriver"]
async fn error_toast_never_populates_slot() {
    // Covers every failure variant.
    let variants = [
        json!({ "kind": "file_in_use", "message": "…" }),
        json!({ "kind": "qt_modal", "message": "Failed to open" }),
    ];
    for v in &variants {
        let server = TestServer::spawn().expect("spawn");
        unlock_default_profile(&server.url).await.unwrap();
        launch_giants(&server.url).await.unwrap();
        inject_load_outcomes(&server.url, json!([v.clone()])).await.unwrap();

        let phone = Phone::new(&server.url, &server.chromedriver_url).await.unwrap();
        phone.wait_for_portal(Duration::from_secs(10)).await.unwrap();
        phone.tap_slot(1).await.unwrap();
        phone
            .client
            .find(Locator::Css(".card"))
            .await
            .unwrap()
            .click()
            .await
            .unwrap();

        phone
            .wait_until(Duration::from_secs(5), || async {
                phone.toast_count().await.map(|n| n > 0).unwrap_or(false)
            })
            .await
            .unwrap();
        // Slot should end Empty, not with any error-as-text content.
        let t = phone.slot_text(1).await.unwrap();
        assert_eq!(t, "Empty", "slot leaked error text {t:?} for {v:?}");
        phone.close().await.unwrap();
    }
}

// ---- Test 3.6.5: ws_reconnect ---------------------------------------------

#[tokio::test(flavor = "current_thread")]
#[ignore = "requires chromedriver"]
async fn ws_reconnect() {
    let server = TestServer::spawn().expect("spawn");
    unlock_default_profile(&server.url).await.unwrap();
    launch_giants(&server.url).await.unwrap();
    inject_load_outcomes(&server.url, json!([{"kind":"ok"}])).await.unwrap();

    let phone = Phone::new(&server.url, &server.chromedriver_url).await.unwrap();
    phone.wait_for_portal(Duration::from_secs(10)).await.unwrap();

    // Load a figure first so we have a known post-reconnect snapshot.
    phone.tap_slot(1).await.unwrap();
    phone
        .client
        .find(Locator::Css(".card"))
        .await
        .unwrap()
        .click()
        .await
        .unwrap();
    phone
        .wait_until(Duration::from_secs(5), || async {
            phone
                .slot_text(1)
                .await
                .map(|t| t != "Empty" && t != "Loading…")
                .unwrap_or(false)
        })
        .await
        .unwrap();
    let before = phone.slot_text(1).await.unwrap();

    // Nudge the WS: force-close the window's WebSocket via JS, wait for
    // reconnect, verify state still shows `before`.
    let _ = phone
        .client
        .execute(
            r#"
              // Find the global WS by monkey-patching onclose to trigger early.
              // If nothing is exposed, fall back to reloading the page.
              try { location.reload(); } catch(e) {}
            "#,
            vec![],
        )
        .await;
    // Post-reload the browser gets a brand-new WS session (per 3.10's
    // per-session unlock model) so we need to re-seed the unlock before the
    // new session registers. `unlock_default_profile` sets the server's
    // `pending_unlock`, which the next `register()` consumes. There's a
    // narrow race window (old session drop → pending set → new session
    // register) but the reload's WS reconnect is much slower than the
    // server's in-process state change, so this lands correctly in practice.
    unlock_default_profile(&server.url).await.unwrap();
    phone.wait_for_portal(Duration::from_secs(10)).await.unwrap();

    phone
        .wait_until(Duration::from_secs(5), || async {
            phone.slot_text(1).await.map(|t| t == before).unwrap_or(false)
        })
        .await
        .unwrap();

    phone.close().await.unwrap();
}

// ---- Test 3.6.6: on_portal_figures_disabled ------------------------------

#[tokio::test(flavor = "current_thread")]
#[ignore = "requires chromedriver"]
async fn on_portal_figures_disabled() {
    let server = TestServer::spawn().expect("spawn");
    unlock_default_profile(&server.url).await.unwrap();
    launch_giants(&server.url).await.unwrap();
    inject_load_outcomes(&server.url, json!([{"kind":"ok"}])).await.unwrap();

    let phone = Phone::new(&server.url, &server.chromedriver_url).await.unwrap();
    phone.wait_for_portal(Duration::from_secs(10)).await.unwrap();
    phone.tap_slot(1).await.unwrap();

    let cards = phone.client.find_all(Locator::Css(".card")).await.unwrap();
    cards[0].clone().click().await.unwrap();

    phone
        .wait_until(Duration::from_secs(5), || async {
            phone
                .client
                .find(Locator::Css(".card.on-portal"))
                .await
                .is_ok()
        })
        .await
        .unwrap();

    // Tap it anyway → toast, not a second load.
    let card = phone
        .client
        .find(Locator::Css(".card.on-portal"))
        .await
        .unwrap();
    let _ = card.click().await;
    phone
        .wait_until(Duration::from_secs(3), || async {
            phone
                .last_toast_text()
                .await
                .map(|t| t.unwrap_or_default().contains("Already"))
                .unwrap_or(false)
        })
        .await
        .unwrap();

    phone.close().await.unwrap();
}
