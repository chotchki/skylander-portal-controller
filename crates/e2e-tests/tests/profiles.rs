//! End-to-end: profile picker → PIN entry → game picker (PLAN 3.9 +
//! 10.3.6d). The original Phase-3 test walked the in-SPA "create
//! profile" wizard; that's now a multi-step Konami-gated flow whose
//! steps don't add coverage we can't get more cheaply via the
//! `inject_profile` test-hook. This test instead exercises the
//! daily-use flow: an existing profile is on the picker, user taps it,
//! enters PIN, lands on the game picker.
//!
//! Prereqs: chromedriver + built phone SPA. See
//! `crates/e2e-tests/README.md`.

use std::time::Duration;

use fantoccini::Locator;
use skylander_e2e_tests::{Phone, TestServer, inject_profile};

#[tokio::test(flavor = "current_thread")]
#[ignore = "requires chromedriver + built phone SPA"]
async fn existing_profile_unlock_lands_on_game_picker() {
    let server = TestServer::spawn().expect("spawn");
    // Seed a profile via the test-hook so we don't have to drive the
    // multi-step Konami-gated create wizard. PIN matches what we'll
    // type into the keypad below.
    inject_profile(&server.url, "TestKid", "1234", "#39d39f")
        .await
        .expect("inject profile");

    let phone_url = server.phone_url().await.unwrap();
    let phone = Phone::new(&phone_url, &server.chromedriver_url)
        .await
        .expect("connect phone");

    // Land on the ProfilePicker — wait for the named profile card to
    // populate after the async fetch_profiles roundtrip.
    phone
        .wait_for(Locator::Css(".profile-picker"), Duration::from_secs(15))
        .await
        .expect("profile picker");
    phone
        .wait_until(Duration::from_secs(8), || async {
            phone
                .client
                .find(Locator::Css(".profile-card:not(.add)"))
                .await
                .is_ok()
        })
        .await
        .expect("seeded profile card appears");
    let card = phone
        .client
        .find(Locator::Css(".profile-card:not(.add)"))
        .await
        .unwrap();
    card.click().await.unwrap();

    // PIN keypad surfaces in the heraldic variant for the post-tap
    // unlock prompt (`.pin-keypad-heraldic` wraps the digit buttons).
    // Tap 1, 2, 3, 4 — pad auto-submits on the fourth keystroke.
    phone
        .wait_for(Locator::Css(".pin-keypad-heraldic"), Duration::from_secs(5))
        .await
        .expect("pin keypad");
    for d in ["1", "2", "3", "4"] {
        // Heraldic keys use `.pin-hkey`; ghost + backspace are
        // disabled so a positional/text match is enough. Use XPath
        // because each `.pin-hkey` button renders the digit as its
        // text content directly (no inner span to target by class).
        let xpath =
            format!("//button[contains(@class,'pin-hkey') and normalize-space(text())='{d}']");
        let key = phone
            .client
            .find(Locator::XPath(&xpath))
            .await
            .unwrap_or_else(|_| panic!("no keypad button {d}"));
        key.click().await.unwrap();
    }

    // GamePicker should mount now that the unlock landed. The post-
    // Phase-4 picker uses `.game-picker .game-card` (no `<h2>` —
    // title is rendered through `<DisplayHeading>`); count cards as
    // the structural marker matching `tests/smoke.rs`.
    phone
        .wait_for(
            Locator::Css(".game-picker .game-card"),
            Duration::from_secs(10),
        )
        .await
        .expect("game picker .game-card");
    let cards = phone
        .client
        .find_all(Locator::Css(".game-picker .game-card"))
        .await
        .unwrap();
    assert_eq!(
        cards.len(),
        6,
        "expected six Skylanders game cards in the picker, found {}",
        cards.len()
    );

    phone.close().await.unwrap();
}
