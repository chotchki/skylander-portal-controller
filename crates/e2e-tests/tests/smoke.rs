//! Simplest possible e2e test — verifies the harness can spawn the server
//! and the SPA renders the game picker.
//!
//! Requires chromedriver running at http://localhost:4444 and the phone
//! SPA built (`cd phone && trunk build`). See crates/e2e-tests/README.md.

use std::time::Duration;

use fantoccini::Locator;
use skylander_e2e_tests::{unlock_default_profile, Phone, TestServer};

#[tokio::test(flavor = "current_thread")]
#[ignore = "requires chromedriver + built phone SPA"]
async fn smoke_game_picker_renders() {
    let server = TestServer::spawn().expect("spawn server");
    unlock_default_profile(&server.url).await.expect("unlock profile");
    let phone = Phone::new(&server.phone_url().await.unwrap(), &server.chromedriver_url).await.expect("connect phone");

    // The GamePicker shows an h2 with "Pick a game". Wait for it.
    let heading = phone
        .wait_for(Locator::Css(".game-picker h2"), Duration::from_secs(10))
        .await
        .expect("game picker heading");
    let text = heading.text().await.unwrap();
    assert_eq!(text, "Pick a game");

    phone.close().await.unwrap();
}
