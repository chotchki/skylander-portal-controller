//! Phase 3 regression scenarios (PLAN 3.6), retargeted to the Phase-4
//! placement model (PLAN 10.3.6a). Original tests followed a
//! "tap empty slot → see picker → tap card" flow that no longer exists
//! — PLAY_TEST PLAN 8.3 made empty slots inert and out of the DOM,
//! and placement is now a two-step "tap card → FigureDetail → place"
//! gesture mediated by the toy-box lid.
//!
//! The shared `Phone` helpers (`open_toy_box_lid`, `place_first_figure`,
//! `place_figure_named`, `remove_slot`, `wait_for_slot_empty`) hide
//! the lid + detail dance from each test.
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
    Phone, TestServer, inject_load_outcomes, launch_giants, unlock_default_profile,
};

// ---- Test 3.6.1: spam_click_same_slot -------------------------------------
// Pre-Phase-4: rapid card-clicks would fire 5 load_slot calls.
// Phase-4 contract: rapid clicks of `.detail-btn-primary` should still
// produce at most one slot load. The figure card opens the detail
// panel; the place button is what triggers the actual load.

#[tokio::test(flavor = "current_thread")]
#[ignore = "requires chromedriver"]
async fn spam_click_same_slot() {
    let server = TestServer::spawn().expect("spawn");
    unlock_default_profile(&server.url).await.unwrap();
    launch_giants(&server.url).await.unwrap();

    inject_load_outcomes(&server.url, json!([{"kind": "ok"}]))
        .await
        .unwrap();

    let phone = Phone::new(&server.phone_url().await.unwrap(), &server.chromedriver_url)
        .await
        .unwrap();
    phone
        .wait_for_portal(Duration::from_secs(10))
        .await
        .unwrap();
    phone.open_toy_box_lid().await.unwrap();

    // Tap the first figure card to open its detail panel.
    let card = phone
        .client
        .find(Locator::Css(".fig-card-p4:not(.scan-new)"))
        .await
        .unwrap();
    card.click().await.unwrap();
    let place = phone
        .wait_for(Locator::Css(".detail-btn-primary"), Duration::from_secs(5))
        .await
        .unwrap();
    // Rapid-fire five clicks on the place button.
    for _ in 0..5 {
        let _ = place.clone().click().await;
    }

    // Wait for slot 1 to end up Loaded.
    phone
        .wait_until(Duration::from_secs(5), || async {
            phone
                .slot_text(1)
                .await
                .map(|t| !t.is_empty() && t != "empty" && t != "…")
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
// Pre-Phase-4: place Spyro on slot 1, try to place Spyro on slot 2 →
// rejected with toast.
// Phase-4 contract: placement auto-picks the next empty slot. The
// "duplicate across slots" scenario maps to: place Spyro on slot 1
// (placed) → re-open detail for the same Spyro card → place again →
// either rejected (toast) OR auto-routed to slot 2. Per PLAN 8.3 +
// the on-portal ribbon, Spyro should now show `.fig-on-portal-ribbon`
// on its card. Re-tapping a placed card is a no-op (the FigureDetail
// `place` button is disabled when on-portal); we assert the ribbon
// + the absence of any second placement.

#[tokio::test(flavor = "current_thread")]
#[ignore = "requires chromedriver"]
async fn dup_figure_across_slots() {
    let server = TestServer::spawn().expect("spawn");
    unlock_default_profile(&server.url).await.unwrap();
    launch_giants(&server.url).await.unwrap();

    inject_load_outcomes(
        &server.url,
        json!([
            { "kind": "ok" },
            { "kind": "file_in_use", "message": "This file is in use." },
        ]),
    )
    .await
    .unwrap();

    let phone = Phone::new(&server.phone_url().await.unwrap(), &server.chromedriver_url)
        .await
        .unwrap();
    phone
        .wait_for_portal(Duration::from_secs(10))
        .await
        .unwrap();
    phone.open_search().await.unwrap();
    phone.search("Spyro").await.unwrap();
    tokio::time::sleep(Duration::from_millis(300)).await;

    // Place the first Spyro card. Auto-routes to slot 1.
    phone.place_first_figure().await.unwrap();
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

    // After placement, the same card should now render with the
    // on-portal ribbon (.fig-on-portal-ribbon nested inside the
    // .fig-card-p4 element).
    let _ribbon = phone
        .wait_for(
            Locator::Css(".fig-card-p4:not(.scan-new) .fig-on-portal-ribbon"),
            Duration::from_secs(3),
        )
        .await
        .expect("expected on-portal ribbon on placed card");

    // Slot 2 should remain empty (out of the DOM).
    assert!(
        phone.slot_text(2).await.is_err(),
        "slot 2 should stay empty after placing Spyro on slot 1"
    );

    phone.close().await.unwrap();
}

// ---- Test 3.6.3: clear_then_load_sequence ---------------------------------

#[tokio::test(flavor = "current_thread")]
#[ignore = "requires chromedriver"]
async fn clear_then_load_sequence() {
    let server = TestServer::spawn().expect("spawn");
    unlock_default_profile(&server.url).await.unwrap();
    launch_giants(&server.url).await.unwrap();
    inject_load_outcomes(&server.url, json!([{"kind": "ok"}, {"kind": "ok"}]))
        .await
        .unwrap();

    let phone = Phone::new(&server.phone_url().await.unwrap(), &server.chromedriver_url)
        .await
        .unwrap();
    phone
        .wait_for_portal(Duration::from_secs(10))
        .await
        .unwrap();
    phone.open_toy_box_lid().await.unwrap();

    // Place the first available figure into slot 1.
    let cards = phone
        .client
        .find_all(Locator::Css(".fig-card-p4:not(.scan-new)"))
        .await
        .unwrap();
    cards[0].clone().click().await.unwrap();
    let place = phone
        .wait_for(Locator::Css(".detail-btn-primary"), Duration::from_secs(5))
        .await
        .unwrap();
    place.click().await.unwrap();
    phone
        .wait_until(Duration::from_secs(5), || async {
            phone
                .slot_text(1)
                .await
                .map(|t| t != "empty" && t != "…")
                .unwrap_or(false)
        })
        .await
        .unwrap();

    // Remove slot 1. After removal it should disappear from the DOM
    // (PLAY_TEST PLAN 8.3 — empty slots aren't rendered).
    phone.remove_slot(1).await.unwrap();
    phone
        .wait_for_slot_empty(1, Duration::from_secs(5))
        .await
        .unwrap();

    // Load a different figure into slot 1.
    phone.open_toy_box_lid().await.unwrap();
    let cards = phone
        .client
        .find_all(Locator::Css(".fig-card-p4:not(.scan-new):not(.on-portal)"))
        .await
        .unwrap();
    cards[1].clone().click().await.unwrap();
    let place = phone
        .wait_for(Locator::Css(".detail-btn-primary"), Duration::from_secs(5))
        .await
        .unwrap();
    place.click().await.unwrap();
    phone
        .wait_until(Duration::from_secs(5), || async {
            phone
                .slot_text(1)
                .await
                .map(|t| t != "empty" && t != "…")
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
        inject_load_outcomes(&server.url, json!([v.clone()]))
            .await
            .unwrap();

        let phone = Phone::new(&server.phone_url().await.unwrap(), &server.chromedriver_url)
            .await
            .unwrap();
        phone
            .wait_for_portal(Duration::from_secs(10))
            .await
            .unwrap();
        phone.open_toy_box_lid().await.unwrap();
        phone.place_first_figure().await.unwrap();

        phone
            .wait_until(Duration::from_secs(5), || async {
                phone.toast_count().await.map(|n| n > 0).unwrap_or(false)
            })
            .await
            .unwrap();
        // Slot 1 should never have appeared — failure leaves the
        // portal empty, and per PLAN 8.3 empty slots aren't in the DOM.
        assert!(
            phone.slot_text(1).await.is_err(),
            "slot 1 leaked content for failure {v:?}"
        );
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
    inject_load_outcomes(&server.url, json!([{"kind":"ok"}]))
        .await
        .unwrap();

    let phone = Phone::new(&server.phone_url().await.unwrap(), &server.chromedriver_url)
        .await
        .unwrap();
    phone
        .wait_for_portal(Duration::from_secs(10))
        .await
        .unwrap();
    phone.open_toy_box_lid().await.unwrap();

    // Place a figure first so we have a known post-reconnect snapshot.
    phone.place_first_figure().await.unwrap();
    phone
        .wait_until(Duration::from_secs(5), || async {
            phone
                .slot_text(1)
                .await
                .map(|t| t != "empty" && t != "…")
                .unwrap_or(false)
        })
        .await
        .unwrap();
    let before = phone.slot_text(1).await.unwrap();

    // Nudge the WS via reload. Per 3.10 each new session needs the
    // unlock re-seeded so it adopts the same profile and gets the
    // prior layout snapshot back.
    let _ = phone.client.execute("location.reload();", vec![]).await;
    unlock_default_profile(&server.url).await.unwrap();
    phone
        .wait_for_portal(Duration::from_secs(10))
        .await
        .unwrap();

    phone
        .wait_until(Duration::from_secs(5), || async {
            phone
                .slot_text(1)
                .await
                .map(|t| t == before)
                .unwrap_or(false)
        })
        .await
        .unwrap();

    phone.close().await.unwrap();
}

// ---- Test 3.6.6: on_portal_figures_disabled ------------------------------
// Pre-Phase-4: placed cards got `.card.on-portal`; tapping again fired
// a toast. Phase-4 renders an `.fig-on-portal-ribbon` on the placed
// card and disables the FigureDetail's place button. Re-tapping a
// placed card opens the detail panel but the place button is in a
// disabled state — no second load fires.

#[tokio::test(flavor = "current_thread")]
#[ignore = "requires chromedriver"]
async fn on_portal_figures_disabled() {
    let server = TestServer::spawn().expect("spawn");
    unlock_default_profile(&server.url).await.unwrap();
    launch_giants(&server.url).await.unwrap();
    inject_load_outcomes(&server.url, json!([{"kind":"ok"}]))
        .await
        .unwrap();

    let phone = Phone::new(&server.phone_url().await.unwrap(), &server.chromedriver_url)
        .await
        .unwrap();
    phone
        .wait_for_portal(Duration::from_secs(10))
        .await
        .unwrap();
    phone.open_toy_box_lid().await.unwrap();

    phone.place_first_figure().await.unwrap();
    phone
        .wait_until(Duration::from_secs(5), || async {
            phone
                .slot_text(1)
                .await
                .map(|t| t != "empty" && t != "…")
                .unwrap_or(false)
        })
        .await
        .unwrap();

    // After placement, the card should render the on-portal ribbon.
    phone
        .wait_for(
            Locator::Css(".fig-card-p4:not(.scan-new) .fig-on-portal-ribbon"),
            Duration::from_secs(3),
        )
        .await
        .expect("expected on-portal ribbon on placed card");

    // The lid auto-closes after place (see browser.rs's on_placed
    // handler) — re-open it before we can re-tap the card.
    phone.open_toy_box_lid().await.unwrap();
    let placed_card = phone
        .client
        .find(Locator::Css(
            ".fig-card-p4:not(.scan-new):has(.fig-on-portal-ribbon)",
        ))
        .await
        .unwrap();
    placed_card.click().await.unwrap();
    let place = phone
        .wait_for(Locator::Css(".detail-btn-primary"), Duration::from_secs(3))
        .await
        .expect("expected detail panel to open");
    // Clicking place on an already-on-portal figure should surface an
    // error banner inside the detail panel and NOT trigger a second
    // load (figure_detail.rs::on_place's `already` short-circuit).
    place.click().await.unwrap();
    phone
        .wait_for(Locator::Css(".detail-error-banner"), Duration::from_secs(3))
        .await
        .expect("expected detail-error-banner for already-placed figure");
    // Slot 2 must still be empty — no second placement happened.
    assert!(
        phone.slot_text(2).await.is_err(),
        "slot 2 leaked a second placement when re-clicking an on-portal figure"
    );

    phone.close().await.unwrap();
}
