//! End-to-end: profile picker → create profile → unlock → game picker.
//!
//! This test drives the full UI (no test-hook bypass) to pin down the
//! 3.9 flow. Prereqs: chromedriver + built phone SPA.

use std::time::Duration;

use fantoccini::Locator;
use skylander_e2e_tests::{Phone, TestServer};

#[tokio::test(flavor = "current_thread")]
#[ignore = "requires chromedriver + built phone SPA"]
async fn profile_create_and_unlock_lands_on_game_picker() {
    let server = TestServer::spawn().expect("spawn");
    let phone_url = server.phone_url().await.unwrap();
    let phone = Phone::new(&phone_url, &server.chromedriver_url)
        .await
        .expect("connect phone");

    // Should land on the ProfilePicker ("Welcome, portal master").
    let welcome = phone
        .wait_for(Locator::Css(".profile-picker h2"), Duration::from_secs(15))
        .await
        .expect("profile picker heading");
    let txt = welcome.text().await.unwrap();
    assert!(
        txt.to_lowercase().contains("welcome") || txt.to_lowercase().contains("portal master"),
        "unexpected heading: {txt:?}",
    );

    // Tap "+ Create profile".
    let create_btn = phone
        .wait_for(Locator::Css(".create-profile-btn"), Duration::from_secs(5))
        .await
        .expect("create button");
    create_btn.click().await.unwrap();

    // Fill the form.
    let name_input = phone
        .wait_for(
            Locator::Css(".create-profile-form input[type=text]"),
            Duration::from_secs(5),
        )
        .await
        .unwrap();
    name_input.send_keys("TestKid").await.unwrap();

    // Tap digits 1,2,3,4 on the keypad.
    for d in ["1", "2", "3", "4"] {
        let xpath =
            format!("//button[contains(@class,'pin-key') and normalize-space(text())='{d}']");
        let key = phone
            .client
            .find(Locator::XPath(&xpath))
            .await
            .unwrap_or_else(|_| panic!("no keypad button {d}"));
        key.click().await.unwrap();
    }

    // Submit create.
    let create_submit = phone
        .client
        .find(Locator::Css(
            ".create-profile-form .form-actions button.primary",
        ))
        .await
        .unwrap();
    create_submit.click().await.unwrap();

    // Back on the grid. Wait for the profile card to appear.
    phone
        .wait_until(Duration::from_secs(5), || async {
            phone
                .client
                .find(Locator::Css(".profile-card"))
                .await
                .is_ok()
        })
        .await
        .unwrap();

    // Tap the profile to unlock.
    let card = phone
        .client
        .find(Locator::Css(".profile-card"))
        .await
        .unwrap();
    card.click().await.unwrap();

    // PIN entry screen → punch 1 2 3 4 again.
    phone
        .wait_for(Locator::Css(".pin-entry"), Duration::from_secs(5))
        .await
        .expect("pin entry view");
    for d in ["1", "2", "3", "4"] {
        let xpath = format!(
            "//div[contains(@class,'pin-entry')]//button[contains(@class,'pin-key') and normalize-space(text())='{d}']"
        );
        let key = phone.client.find(Locator::XPath(&xpath)).await.unwrap();
        key.click().await.unwrap();
    }
    let unlock_btn = phone
        .client
        .find(Locator::Css(".pin-entry .form-actions button.primary"))
        .await
        .unwrap();
    unlock_btn.click().await.unwrap();

    // Unlocked → GamePicker should render ("Pick a game").
    let heading = phone
        .wait_for(Locator::Css(".game-picker h2"), Duration::from_secs(10))
        .await
        .expect("game picker heading");
    let text = heading.text().await.unwrap();
    assert_eq!(text, "Pick a game");

    phone.close().await.unwrap();
}
