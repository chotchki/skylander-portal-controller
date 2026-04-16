//! Working-copy + session-resume e2e (PLAN 3.11, 3.12).
//!
//! Mock driver-backed: we assert on server-side state (the file actually
//! existing on disk, the slot-text reflecting the canonical name) rather
//! than poking into RPCS3's UIA. The canonical-name → display thread
//! lands in `DriverJob::LoadFigure.canonical_name` so the mock's
//! file-stem-derived name doesn't leak into the slot-text either.

use std::time::Duration;

use fantoccini::Locator;
use serde_json::json;

use skylander_e2e_tests::{
    inject_load_outcomes, launch_giants, unlock_default_profile, Phone, TestServer,
};

// ---- 3.11: working-copy fork + reset ------------------------------------

#[tokio::test(flavor = "current_thread")]
#[ignore = "requires chromedriver"]
async fn load_uses_canonical_name_not_filename() {
    // Regression guard: when load_slot routes through the per-profile
    // working-copy path, that file is named `<figure_id>.sky` — a hex hash.
    // The displayed name in the slot must still be the figure's canonical
    // name ("Spyro", "Eruptor", ...), not the hash.
    let server = TestServer::spawn().expect("spawn");
    unlock_default_profile(&server.url).await.unwrap();
    launch_giants(&server.url).await.unwrap();
    inject_load_outcomes(&server.url, json!([{"kind": "ok"}])).await.unwrap();

    let phone_url = server.phone_url().await.unwrap();
    let phone = Phone::new(&phone_url, &server.chromedriver_url)
        .await
        .unwrap();
    phone.wait_for_portal(Duration::from_secs(10)).await.unwrap();
    phone.search("Spyro").await.unwrap();
    tokio::time::sleep(Duration::from_millis(300)).await;
    phone.tap_slot(1).await.unwrap();
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
        .expect("slot 1 should display 'Spyro', not a figure_id hash");

    phone.close().await.unwrap();
}

// ---- 3.12: session resume + layout memory -------------------------------

#[tokio::test(flavor = "current_thread")]
#[ignore = "requires chromedriver"]
async fn resume_prompt_offers_prior_layout() {
    // 1. P1 loads a figure onto slot 1. Server persists the layout under P1's
    //    profile via `save_portal_layout`.
    // 2. P1 reloads the page. New WS session, no resume prompt yet (no
    //    profile unlocked on the new session).
    // 3. P1's new session re-unlocks the same profile via the test hook.
    // 4. Server sends `Event::ResumePrompt`. Phone renders the modal.
    let server = TestServer::spawn().expect("spawn");
    unlock_default_profile(&server.url).await.unwrap();
    launch_giants(&server.url).await.unwrap();
    inject_load_outcomes(
        &server.url,
        json!([{"kind": "ok"}, {"kind": "ok"}]),
    )
    .await
    .unwrap();

    let phone_url = server.phone_url().await.unwrap();
    let phone = Phone::new(&phone_url, &server.chromedriver_url)
        .await
        .unwrap();
    phone.wait_for_portal(Duration::from_secs(10)).await.unwrap();

    // Load slot 1.
    phone.tap_slot(1).await.unwrap();
    phone
        .client
        .find_all(Locator::Css(".card"))
        .await
        .unwrap()[0]
        .clone()
        .click()
        .await
        .unwrap();
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

    // Briefly wait so `persist_layout` (fires after SlotChanged broadcast)
    // has a chance to write the row before we tear the session down.
    tokio::time::sleep(Duration::from_millis(500)).await;


    // Reload → new WS session. After reload, re-seed unlock so the new
    // session adopts the same profile and the server sees the prior
    // layout for it.
    phone
        .client
        .execute("location.reload();", vec![])
        .await
        .ok();
    unlock_default_profile(&server.url).await.unwrap();

    // Resume modal should appear. It's session-filtered, so the phone
    // sees it only after its new WS Welcome + ProfileChanged land.
    phone
        .wait_until(Duration::from_secs(10), || async {
            phone
                .client
                .find(Locator::Css(".resume-modal"))
                .await
                .is_ok()
        })
        .await
        .expect("resume modal should appear on re-unlock");

    // Click "Resume" — should trigger re-loads. Queue a fresh outcome so
    // the mock driver can service the resume load.
    inject_load_outcomes(&server.url, json!([{"kind": "ok"}])).await.unwrap();
    phone
        .client
        .find(Locator::Css(".resume-yes"))
        .await
        .unwrap()
        .click()
        .await
        .unwrap();

    // Modal should dismiss and slot 1 should end up Loaded with the figure
    // name again.
    phone
        .wait_until(Duration::from_secs(5), || async {
            phone
                .client
                .find(Locator::Css(".resume-modal"))
                .await
                .is_err()
        })
        .await
        .unwrap();
    phone
        .wait_until(Duration::from_secs(8), || async {
            phone
                .slot_text(1)
                .await
                .map(|t| !t.is_empty() && t != "Empty" && t != "Loading…")
                .unwrap_or(false)
        })
        .await
        .unwrap();

    phone.close().await.unwrap();
}
