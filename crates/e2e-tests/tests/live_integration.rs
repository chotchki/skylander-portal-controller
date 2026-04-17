//! PLAN 3.7.7 — integrated live e2e test.
//!
//! Drives the phone SPA against a server backed by the **real UIA driver** and
//! **real RPCS3**. Boots a game, loads a figure, and clears it — the phone-
//! facing analogue of `crates/rpcs3-control/tests/live_lifecycle.rs`.
//!
//! Why this exists: the 3.6-era regression tests use the mock driver plus
//! `/api/_test/inject_load` failure injection, which is great for protocol
//! coverage but can't catch driver/server/phone integration regressions. 3.7.2
//! proved the real driver works from bare Rust; this test glues the phone on
//! top so we actually exercise the whole stack.
//!
//! Requirements (same contract as `live_lifecycle.rs`):
//!   RPCS3_EXE=C:\emuluators\rpcs3\rpcs3.exe
//!   RPCS3_TEST_SERIAL=BLUS31076           # a game the HTPC has installed
//!   RPCS3_SKY_TEST_PATH=C:\...\Eruptor.sky
//!   SKYLANDER_PACK_ROOT=...               # optional; defaults per CLAUDE.md
//!   CHROMEDRIVER=...                      # optional; same PATH/winget fallback as 3.6
//!
//! Run:
//!   cargo test -p skylander-e2e-tests --test live_integration -- --ignored --nocapture
//!
//! CI does NOT run this — no interactive desktop, no RPCS3 install. Stays
//! `#[ignore]`-gated. On the HTPC + over RDP it works (PLAN 3.7.2 confirmed
//! the session isolation assumptions hold for RDP sessions).

use std::path::PathBuf;
use std::time::Duration;

use fantoccini::Locator;
use reqwest::Client;

use skylander_e2e_tests::{Phone, TestServer, unlock_default_profile};

/// Resolve the test's env vars up-front. Returns `None` if any are missing —
/// mirrors `live_lifecycle.rs`'s early-return pattern so running the test with
/// the env unset is a silent skip, not a hard failure.
fn require_env() -> Option<(String, String)> {
    let serial = std::env::var("RPCS3_TEST_SERIAL").ok()?;
    let sky = std::env::var("RPCS3_SKY_TEST_PATH").ok()?;
    Some((serial, sky))
}

/// Extract the figure's canonical name from the `.sky` file stem. The pack
/// indexer produces canonical names by cleaning up the filename; the lookup
/// in `Browser`'s grid matches against `Figure.canonical_name` which *is* the
/// filename stem for the standard pack layout, so this round-trip works.
fn canonical_name_from_path(sky_path: &str) -> String {
    PathBuf::from(sky_path)
        .file_stem()
        .and_then(|s| s.to_str())
        .map(String::from)
        .expect("RPCS3_SKY_TEST_PATH has no filename stem")
}

/// Look up the server's advertised short name for a given serial, so the
/// phone UI selector can target the right `.game-card`. The phone's
/// GamePicker strips the "Skylanders: " prefix when rendering; we replicate
/// that here rather than exposing more selectors in the DOM.
async fn short_name_for_serial(base: &str, serial: &str) -> String {
    #[derive(serde::Deserialize)]
    struct Game {
        serial: String,
        display_name: String,
    }
    let games: Vec<Game> = Client::new()
        .get(format!("{base}/api/games"))
        .send()
        .await
        .expect("GET /api/games")
        .json()
        .await
        .expect("parse /api/games");
    let display_name = games
        .into_iter()
        .find(|g| g.serial == serial)
        .unwrap_or_else(|| panic!("serial {serial} not in /api/games — check games.yml"))
        .display_name;
    display_name
        .strip_prefix("Skylanders: ")
        .unwrap_or(&display_name)
        .to_string()
}

#[tokio::test(flavor = "current_thread")]
#[ignore = "requires RPCS3_EXE, RPCS3_TEST_SERIAL, RPCS3_SKY_TEST_PATH, chromedriver, built phone SPA"]
async fn phone_drives_real_rpcs3_load_and_clear() {
    let (serial, sky_path) = match require_env() {
        Some(t) => t,
        None => {
            eprintln!("skipping: set RPCS3_EXE / RPCS3_TEST_SERIAL / RPCS3_SKY_TEST_PATH to run");
            return;
        }
    };
    let figure_name = canonical_name_from_path(&sky_path);

    let server = TestServer::spawn_live().expect("spawn live server");
    unlock_default_profile(&server.url)
        .await
        .expect("unlock default profile");

    let phone = Phone::new(&server.phone_url().await.unwrap(), &server.chromedriver_url)
        .await
        .expect("connect phone");

    // --- 1. Game picker: click the card that maps to our serial. ---
    phone
        .wait_for(Locator::Css(".game-picker"), Duration::from_secs(10))
        .await
        .expect("game picker renders");

    let short_name = short_name_for_serial(&server.url, &serial).await;
    let cards = phone
        .client
        .find_all(Locator::Css(".game-card"))
        .await
        .expect("game cards findable");
    let mut clicked = false;
    for card in cards {
        let label = card
            .find(Locator::Css(".game-name"))
            .await
            .unwrap()
            .text()
            .await
            .unwrap_or_default();
        if label == short_name {
            card.click().await.expect("click game card");
            clicked = true;
            break;
        }
    }
    assert!(clicked, "no .game-card matched short name {short_name:?}");

    // --- 2. Wait for the portal view. Real RPCS3 boot = ~30-60s worst case. ---
    phone
        .wait_for(Locator::Css(".screen-portal"), Duration::from_secs(120))
        .await
        .expect("portal screen appears after RPCS3 boots");

    // --- 3. Tap slot 1 so the next figure-card click targets slot 1. ---
    let slot1 = phone
        .client
        .find(Locator::Css(".p4-slot"))
        .await
        .expect("at least one slot");
    slot1.click().await.expect("click slot 1");

    // --- 4. Find the figure card for our test figure in the Browser. ---
    // The pack has hundreds of figures; iterate all `.fig-card-p4` and
    // match the `.fig-name-p4` text. This is O(n) but runs client-side
    // in the browser — fine for a single test on the HTPC.
    let mut detail_opened = false;
    for card in phone
        .client
        .find_all(Locator::Css(".fig-card-p4"))
        .await
        .expect("fig cards findable")
    {
        let name = card
            .find(Locator::Css(".fig-name-p4"))
            .await
            .unwrap()
            .text()
            .await
            .unwrap_or_default();
        if name == figure_name {
            card.click().await.expect("click figure card");
            detail_opened = true;
            break;
        }
    }
    assert!(
        detail_opened,
        "no .fig-card-p4 matched figure name {figure_name:?}"
    );

    // --- 5. In the FigureDetail overlay, click PLACE ON PORTAL. ---
    let place_btn = phone
        .wait_for(Locator::Css(".detail-btn-primary"), Duration::from_secs(5))
        .await
        .expect("place-on-portal button");
    place_btn.click().await.expect("click place");

    // --- 6. Wait for slot 1's label to show the canonical name. The
    //        driver's ~10s load + dialog navigation lives inside this. ---
    phone
        .wait_until(Duration::from_secs(30), || async {
            let labels = match phone.client.find_all(Locator::Css(".p4-slot-label")).await {
                Ok(v) => v,
                Err(_) => return false,
            };
            let slot1 = match labels.first() {
                Some(l) => l,
                None => return false,
            };
            slot1
                .text()
                .await
                .map(|t| t == figure_name)
                .unwrap_or(false)
        })
        .await
        .unwrap_or_else(|_| panic!("slot 1 never showed {figure_name:?} within 30s"));

    // --- 7. REMOVE — slot 1's `.p4-slot-action--remove` button. ---
    let remove_btn = phone
        .client
        .find(Locator::Css(".p4-slot-action--remove"))
        .await
        .expect("remove button on loaded slot 1");
    remove_btn.click().await.expect("click remove");

    // --- 8. Wait for slot 1 to go back to empty. ---
    phone
        .wait_until(Duration::from_secs(30), || async {
            let labels = match phone.client.find_all(Locator::Css(".p4-slot-label")).await {
                Ok(v) => v,
                Err(_) => return false,
            };
            match labels.first() {
                Some(l) => {
                    let class = l
                        .attr("class")
                        .await
                        .unwrap_or_default()
                        .unwrap_or_default();
                    class.contains("p4-slot-label--empty")
                }
                None => false,
            }
        })
        .await
        .expect("slot 1 never went back to empty within 30s");

    // --- 9. Quit RPCS3 cleanly so the next run doesn't trip on the lockfile. ---
    // The server's /api/quit is signed, so we go through the phone's menu
    // if we've wired that up — but for this first test, use the harness's
    // Drop-based teardown: ChildGuard kills cargo-run, the Job Object
    // takes RPCS3 down with it, and `spawn_live` does a defensive
    // lockfile clear on the *next* launch. A follow-up can switch this to
    // an in-test graceful quit once we have a signed REST client.
    phone.close().await.ok();
}
