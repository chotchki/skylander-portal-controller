//! Simplest possible e2e test — verifies the harness can spawn the server
//! and the SPA renders the game picker.
//!
//! Requires chromedriver running at http://localhost:4444 and the phone
//! SPA built (`cd phone && trunk build`). See crates/e2e-tests/README.md.

use std::time::Duration;

use fantoccini::Locator;
use skylander_e2e_tests::{Phone, TestServer, unlock_default_profile};

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
        .wait_for(Locator::Css(".game-picker .game-card"), Duration::from_secs(10))
        .await
        .expect("first game card");
    let cards = phone
        .client
        .find_all(Locator::Css(".game-picker .game-card"))
        .await
        .expect("all game cards");
    assert_eq!(
        cards.len(),
        6,
        "expected six Skylanders game cards in the picker, found {}",
        cards.len()
    );

    phone.close().await.unwrap();
}
