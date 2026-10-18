//! Simplest possible e2e test — verifies the harness can spawn the server
//! and the SPA renders the game picker.
//!
//! Requires chromedriver running at http://localhost:4444 and the phone
//! SPA built (`cd phone && trunk build`). See crates/e2e-tests/README.md.

use std::time::Duration;

use fantoccini::Locator;
use skylander_e2e_tests::{
    E2eApp, Phone, TestServer, inject_profile, launch_giants, unlock_default_profile,
    unlock_session,
};

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
/// product offers it, guacamole leaves it out (openspec AA.19). The phone
/// assumes the product until `/api/status` answers, so the product's menu
/// check can pass before the status lands; the server's `app` is asserted
/// first.
#[tokio::test(flavor = "current_thread")]
#[ignore = "requires chromedriver + built phone SPA"]
async fn the_menu_offers_game_settings_only_in_the_product() {
    let server = TestServer::spawn().expect("spawn server");
    let app = E2eApp::from_env();
    let status: serde_json::Value = reqwest::get(format!("{}/api/status", server.url))
        .await
        .expect("GET /api/status")
        .json()
        .await
        .expect("parse /api/status");
    let want = match app {
        E2eApp::Product => "product",
        E2eApp::Guacamole => "guacamole",
    };
    assert_eq!(status["app"], want, "{app:?}: {status}");
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

/// openspec AA.20: in a browser tab that keeps its toolbars on screen the
/// visible viewport is short, and Orion on an iPhone hid HOLD TO SHUT DOWN
/// below the menu's clipped bottom. With a profile open and a game running
/// (the longest menu), every action has to come fully inside the viewport
/// once scrolled to, with a tap at its centre landing on it. 375x548 is an
/// iPhone SE's Safari with both bars up, 390x664 a current iPhone's.
#[tokio::test(flavor = "current_thread")]
#[ignore = "requires chromedriver + built phone SPA"]
async fn every_menu_action_is_reachable_in_a_short_viewport() {
    let server = TestServer::spawn().expect("spawn server");
    let app = E2eApp::from_env();
    let pid = unlock_default_profile(&server.url)
        .await
        .expect("unlock profile");
    launch_giants(&server.url).await.expect("launch Giants");

    for (w, h) in [(375, 548), (390, 664)] {
        // Each phone is its own session, so the next one is unlocked too.
        unlock_session(&server.url, &pid).await.unwrap();
        let phone = Phone::new_sized(
            &server.phone_url().await.unwrap(),
            &server.chromedriver_url,
            w,
            h,
        )
        .await
        .expect("connect phone");
        phone
            .wait_for_portal(Duration::from_secs(15))
            .await
            .expect("portal");
        let viewport = phone
            .client
            .execute("return [window.innerWidth, window.innerHeight];", vec![])
            .await
            .unwrap();
        assert_eq!(viewport, serde_json::json!([w, h]), "viewport");
        phone
            .wait_for(Locator::Css(".kebab-btn"), Duration::from_secs(5))
            .await
            .unwrap()
            .click()
            .await
            .unwrap();
        let titles = phone
            .menu_actions_until(|a| a.iter().any(|t| t == "HOLD TO SWITCH GAMES"))
            .await
            .unwrap();
        for want in [
            "SWITCH PROFILE",
            "MANAGE PROFILES",
            "HOLD TO SWITCH GAMES",
            "HOLD TO SHUT DOWN",
        ] {
            assert!(titles.iter().any(|t| t == want), "{w}x{h}: {titles:?}");
        }
        // The open animation (0.35 s) scales the panel.
        tokio::time::sleep(Duration::from_millis(500)).await;
        let reach = phone.menu_reach().await.unwrap();
        eprintln!("{app:?} {w}x{h}: {reach}");
        let inside = |r: &serde_json::Value| {
            let r: Vec<f64> = serde_json::from_value(r.clone()).unwrap();
            r[0] >= 0.0 && r[1] >= 0.0 && r[2] <= w as f64 && r[3] <= h as f64
        };
        assert!(
            inside(&reach["panel"]),
            "{w}x{h}: the panel leaves the viewport: {reach}"
        );
        let actions = reach["actions"].as_array().unwrap();
        assert_eq!(actions.len(), titles.len(), "{reach}");
        for a in actions {
            assert!(inside(&a["rect"]), "{w}x{h}: {a} isn't fully on screen");
            assert_eq!(a["tappable"], true, "{w}x{h}: a tap misses {a}");
        }
        phone.close().await.unwrap();
    }
}
