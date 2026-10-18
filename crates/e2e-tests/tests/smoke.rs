//! Simplest possible e2e test — verifies the harness can spawn the server
//! and the SPA renders the game picker.
//!
//! Requires chromedriver running at http://localhost:4444 and the phone
//! SPA built (`cd phone && trunk build`). See crates/e2e-tests/README.md.

use std::time::Duration;

use fantoccini::Locator;
use skylander_e2e_tests::{E2eApp, Phone, TestServer, inject_profile, unlock_default_profile};

#[tokio::test(flavor = "current_thread")]
#[ignore = "requires chromedriver + built phone SPA"]
async fn smoke_game_picker_renders() {
    let server = TestServer::spawn().expect("spawn server");
    unlock_default_profile(&server.url)
        .await
        .expect("unlock profile");
    let phone = Phone::new(&server.phone_url().await.unwrap(), &server.chromedriver_url)
        .await
        .expect("connect phone");

    // GamePicker renders one `.game-card` per supported Skylanders game
    // — six of them since the mock driver enumerates the full
    // `SKYLANDERS_SERIALS` table. Asserting on card count is more robust
    // than chasing the heading text: the title is rendered via
    // `<DisplayHeading>` with `-webkit-text-stroke` + a custom font, and
    // headless Chrome's `getElementText` returns "" for those even though
    // the text is visually present in the DOM.
    phone
        .wait_for(
            Locator::Css(".game-picker .game-card"),
            Duration::from_secs(10),
        )
        .await
        .expect("first game card");
    let cards = phone
        .client
        .find_all(Locator::Css(".game-picker .game-card"))
        .await
        .expect("all game cards");
    // Six under the mock; guacamole offers Giants alone.
    let want = E2eApp::from_env().game_count();
    assert_eq!(
        cards.len(),
        want,
        "expected {want} Skylanders game cards in the picker, found {}",
        cards.len()
    );

    phone.close().await.unwrap();
}

/// The grown-ups menu's HOLD FOR GAME SETTINGS before a game is picked: the
/// product offers it, guacamole leaves it out (openspec AA.19).
#[tokio::test(flavor = "current_thread")]
#[ignore = "requires chromedriver + built phone SPA"]
async fn the_menu_offers_game_settings_only_in_the_product() {
    let server = TestServer::spawn().expect("spawn server");
    inject_profile(&server.url, "TestKid", "1234", "#39d39f")
        .await
        .expect("inject profile");
    let phone = Phone::new(&server.phone_url().await.unwrap(), &server.chromedriver_url)
        .await
        .expect("connect phone");
    phone
        .wait_for(Locator::Css(".profile-picker"), Duration::from_secs(15))
        .await
        .expect("profile picker");
    phone
        .wait_for(Locator::Css(".kebab-btn"), Duration::from_secs(5))
        .await
        .unwrap()
        .click()
        .await
        .unwrap();

    let app = E2eApp::from_env();
    let product = app == E2eApp::Product;
    let offered = |a: &[String]| a.iter().any(|t| t == "HOLD FOR GAME SETTINGS");
    let actions = phone
        .menu_actions_until(move |a| offered(a) == product)
        .await
        .unwrap();
    eprintln!("{app:?} menu: {actions:?}");
    assert!(
        actions.iter().any(|t| t == "MANAGE PROFILES"),
        "{actions:?}"
    );
    assert_eq!(offered(&actions), product, "{app:?}: {actions:?}");

    phone.close().await.unwrap();
}
