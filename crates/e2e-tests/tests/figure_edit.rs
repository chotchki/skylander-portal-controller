//! Figure stat editing e2e (PLAN 11).
//!
//! Covers the level + gold edit flow end-to-end: phone opens a figure's
//! detail screen, taps STATS, the new edit sheet renders, level + gold
//! steppers update, SAVE round-trips through the server's
//! `/api/profiles/:pid/figures/:fid/edit` endpoint, and the stats strip
//! refreshes with the new values.
//!
//! Also covers the portal-occupancy guard: a figure currently on the
//! portal has its STATS button disabled with a "remove from portal first"
//! tooltip.
//!
//! Mock driver-backed (`SKYLANDER_PORTAL_DRIVER=mock` from `.env.dev`);
//! no real RPCS3 needed. `#[ignore = "requires chromedriver"]` follows
//! the same opt-in pattern as the other PLAN-driven e2e tests in this
//! crate — these run locally, not in CI.

use std::time::Duration;

use fantoccini::Locator;
use serde_json::json;

use skylander_e2e_tests::{
    Phone, TestServer, inject_load_outcomes, launch_giants, unlock_default_profile,
};

/// Open the toy-box-lid search, type `query`, give the filtered grid a
/// brief moment to render, then tap the first non-`scan-new` card it
/// shows. Avoids the exact-name match in `tap_figure_named` so the test
/// doesn't break when the indexer collapses variants under a slightly
/// different canonical label. Clears any existing search-input value
/// first so a second call after a prior placement doesn't double-stack
/// the query.
async fn open_first_matching_figure(phone: &Phone, query: &str) -> anyhow::Result<()> {
    phone.open_search().await?;
    let input = phone
        .client
        .find(Locator::Css(".search-input-p4"))
        .await?;
    input.clear().await?;
    input.send_keys(query).await?;
    tokio::time::sleep(Duration::from_millis(400)).await;
    let first = phone
        .wait_for(
            Locator::Css(".fig-card-p4:not(.scan-new)"),
            Duration::from_secs(3),
        )
        .await?;
    first.click().await?;
    Ok(())
}

#[tokio::test(flavor = "current_thread")]
#[ignore = "requires chromedriver"]
async fn edit_level_and_gold_round_trips_through_stats_strip() {
    let server = TestServer::spawn().expect("spawn");
    unlock_default_profile(&server.url).await.unwrap();
    launch_giants(&server.url).await.unwrap();

    let phone_url = server.phone_url().await.unwrap();
    let phone = Phone::new(&phone_url, &server.chromedriver_url)
        .await
        .unwrap();
    phone
        .wait_for_portal(Duration::from_secs(10))
        .await
        .unwrap();

    // Open a Spyro-family figure detail without placing.
    open_first_matching_figure(&phone, "Spyro").await.unwrap();

    // STATS button should be enabled for an SSA figure that's off the portal.
    let stats_btn = phone
        .wait_for(
            Locator::Css(".detail-action-btn[aria-label='Edit stats']"),
            Duration::from_secs(5),
        )
        .await
        .unwrap();
    assert!(
        stats_btn.attr("disabled").await.unwrap().is_none(),
        "STATS should be enabled for an off-portal Standard figure"
    );

    // Tap STATS → edit sheet appears.
    stats_btn.click().await.unwrap();
    phone
        .wait_for(Locator::Css(".edit-scrim"), Duration::from_secs(3))
        .await
        .unwrap();

    // Bump level 1 → 5 via four `+` taps on the level stepper.
    for _ in 0..4 {
        phone
            .client
            .find(Locator::Css("[aria-label='Increase level']"))
            .await
            .unwrap()
            .click()
            .await
            .unwrap();
    }
    // Bump gold 0 → 300 via three `+100` taps on the gold stepper.
    for _ in 0..3 {
        phone
            .client
            .find(Locator::Css("[aria-label='Increase gold by 100']"))
            .await
            .unwrap()
            .click()
            .await
            .unwrap();
    }

    // SAVE → sheet closes; the post_edit_figure call returns 202, on_saved
    // bumps stats_rev, the stats LocalResource re-fetches.
    phone
        .client
        .find(Locator::Css(".edit-btn-primary"))
        .await
        .unwrap()
        .click()
        .await
        .unwrap();
    phone
        .wait_until(Duration::from_secs(5), || async {
            phone
                .client
                .find(Locator::Css(".edit-scrim"))
                .await
                .is_err()
        })
        .await
        .expect("edit sheet should close after save");

    // Stats strip below should show the new values once the LocalResource
    // re-runs against the now-written working copy.
    phone
        .wait_until(Duration::from_secs(5), || async {
            phone
                .inner_text(".detail-stat-cell:nth-of-type(1) .detail-stat-v")
                .await
                .ok()
                .flatten()
                .map(|t| t.trim() == "5")
                .unwrap_or(false)
        })
        .await
        .expect("stats strip LEVEL should refresh to 5");
    let gold_after = phone
        .inner_text(".detail-stat-cell:nth-of-type(2) .detail-stat-v")
        .await
        .unwrap()
        .unwrap_or_default();
    assert_eq!(gold_after.trim(), "300", "stats strip GOLD should be 300");

    phone.close().await.unwrap();
}

#[tokio::test(flavor = "current_thread")]
#[ignore = "requires chromedriver"]
async fn stats_button_disabled_while_figure_on_portal() {
    let server = TestServer::spawn().expect("spawn");
    unlock_default_profile(&server.url).await.unwrap();
    launch_giants(&server.url).await.unwrap();
    // Queue one mock load outcome so the placement step lands.
    inject_load_outcomes(&server.url, json!([{"kind": "ok"}]))
        .await
        .unwrap();

    let phone_url = server.phone_url().await.unwrap();
    let phone = Phone::new(&phone_url, &server.chromedriver_url)
        .await
        .unwrap();
    phone
        .wait_for_portal(Duration::from_secs(10))
        .await
        .unwrap();

    // Place the first Spyro card onto slot 1.
    phone.open_search().await.unwrap();
    phone.search("Spyro").await.unwrap();
    tokio::time::sleep(Duration::from_millis(300)).await;
    let first = phone
        .client
        .find(Locator::Css(".fig-card-p4:not(.scan-new)"))
        .await
        .unwrap();
    first.click().await.unwrap();
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
                .map(|t| t.to_lowercase().contains("spyro"))
                .unwrap_or(false)
        })
        .await
        .expect("slot 1 should show the placed figure");

    // Re-open the same figure's detail screen.
    open_first_matching_figure(&phone, "Spyro").await.unwrap();
    let stats_btn = phone
        .wait_for(
            Locator::Css(".detail-action-btn[aria-label='Edit stats']"),
            Duration::from_secs(5),
        )
        .await
        .unwrap();

    // Disabled + tooltip explains why.
    assert!(
        stats_btn.attr("disabled").await.unwrap().is_some(),
        "STATS should be disabled while the figure is on the portal"
    );
    let title = stats_btn
        .attr("title")
        .await
        .unwrap()
        .unwrap_or_default()
        .to_lowercase();
    assert!(
        title.contains("portal"),
        "title should mention portal, got: {title:?}"
    );

    phone.close().await.unwrap();
}
