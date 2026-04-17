//! PLAN 3.7.7 / 3.7.9 — integrated live e2e tests.
//!
//! Drives the phone SPA against a server backed by the **real UIA driver** and
//! **real RPCS3**. The mock-driver e2e suite (regressions, multi_phone, etc.)
//! covers protocol and UI behaviour; this file exercises the interactions
//! those tests cannot reach: real Qt modal latency, real file-dialog timing,
//! real working-copy resolve on disk, real RPCS3 lifecycle.
//!
//! Requirements (same contract as `live_lifecycle.rs`):
//!   RPCS3_EXE=C:\emuluators\rpcs3\rpcs3.exe
//!   RPCS3_TEST_SERIAL=BLUS31076           # a game the HTPC has installed
//!   RPCS3_SKY_TEST_PATH=C:\...\Eruptor.sky
//!   RPCS3_SKY_TEST_PATH_2=C:\...\Fryno.sky   # only for the 2-figure scenarios
//!   SKYLANDER_PACK_ROOT=...               # optional; defaults per CLAUDE.md
//!   CHROMEDRIVER=...                      # optional; same PATH/winget fallback as 3.6
//!
//! Run (HTPC only — CI doesn't have an interactive desktop or RPCS3):
//!   cargo test -p skylander-e2e-tests --test live_integration -- --ignored --nocapture --test-threads=1
//!
//! `--test-threads=1` matters: every test spawns its own RPCS3 and only one
//! can own the Skylanders Manager dialog at a time. Running in parallel would
//! cross-contaminate the UIA tree walks.

use std::path::PathBuf;
use std::time::Duration;

use fantoccini::Locator;
use reqwest::Client;

use skylander_e2e_tests::{
    Phone, TestServer, inject_profile, set_session_profile, unlock_default_profile, unlock_session,
};

// ======================================================================
// Env resolution
// ======================================================================

/// Single-figure env: used by the 1-figure scenarios (3.7.7, 3.7.9.2).
fn require_env() -> Option<(String, String)> {
    let serial = std::env::var("RPCS3_TEST_SERIAL").ok()?;
    let sky = std::env::var("RPCS3_SKY_TEST_PATH").ok()?;
    Some((serial, sky))
}

/// Two-figure env: used by the sequential-ops and resume scenarios
/// (3.7.9.1, 3.7.9.3). Figure B comes from `RPCS3_SKY_TEST_PATH_2`.
fn require_env_two_figures() -> Option<(String, String, String)> {
    let (serial, sky_a) = require_env()?;
    let sky_b = std::env::var("RPCS3_SKY_TEST_PATH_2").ok()?;
    Some((serial, sky_a, sky_b))
}

/// Canonical name = `.sky` file stem. The indexer builds `Figure.canonical_name`
/// from the same stem, so the round-trip is exact for standard pack filenames.
fn canonical_name_from_path(sky_path: &str) -> String {
    PathBuf::from(sky_path)
        .file_stem()
        .and_then(|s| s.to_str())
        .map(String::from)
        .expect("sky path has no filename stem")
}

// ======================================================================
// Server + phone helpers
// ======================================================================

/// Look up the phone-visible short name for a serial. GamePicker strips the
/// `"Skylanders: "` prefix when rendering; replicate that so selectors can
/// match on the visible text without adding more DOM hooks.
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

/// Full setup up to the portal screen: spawn server, unlock default profile,
/// connect phone, pick the game card for `serial`, wait for the portal view
/// to render after RPCS3 boots. `TestServer` comes back so the caller can
/// hit REST hooks (e.g. `unlock_default_profile` again after a reload).
async fn spawn_and_land_on_portal(serial: &str) -> (TestServer, Phone) {
    let server = TestServer::spawn_live().expect("spawn live server");
    unlock_default_profile(&server.url)
        .await
        .expect("unlock default profile");

    let phone = Phone::new(&server.phone_url().await.unwrap(), &server.chromedriver_url)
        .await
        .expect("connect phone");

    phone
        .wait_for(Locator::Css(".game-picker"), Duration::from_secs(10))
        .await
        .expect("game picker renders");

    // The picker animates cards in with a staggered `gp-card-rise`
    // keyframe (opacity 0 → 1, ~820ms until the last card settles).
    // WebDriver's `.text()` returns the empty string for opacity-0
    // elements, so iterating too early sees a mix of "real label" and
    // "". Poll until the target card's text matches instead.
    let short_name = short_name_for_serial(&server.url, serial).await;
    phone
        .wait_until(Duration::from_secs(10), || async {
            let cards = match phone.client.find_all(Locator::Css(".game-card")).await {
                Ok(v) => v,
                Err(_) => return false,
            };
            for card in cards {
                if let Ok(label_el) = card.find(Locator::Css(".game-name")).await
                    && let Ok(text) = label_el.text().await
                    && text.eq_ignore_ascii_case(&short_name)
                {
                    return true;
                }
            }
            false
        })
        .await
        .unwrap_or_else(|_| panic!("no .game-card rendered matching {short_name:?}"));

    // Now find + click it. One more pass because `wait_until`'s closure
    // doesn't give us back the matched element.
    let cards = phone
        .client
        .find_all(Locator::Css(".game-card"))
        .await
        .expect("game cards findable");
    for card in cards {
        let label = card
            .find(Locator::Css(".game-name"))
            .await
            .unwrap()
            .text()
            .await
            .unwrap_or_default();
        if label.eq_ignore_ascii_case(&short_name) {
            card.click().await.expect("click game card");
            break;
        }
    }

    // Real RPCS3 boot: ~10-30s typical, 120s timeout for headroom.
    phone
        .wait_for(Locator::Css(".screen-portal"), Duration::from_secs(120))
        .await
        .expect("portal screen appears after RPCS3 boots");

    (server, phone)
}

/// Tap slot N (1-indexed), find the figure card with the given canonical
/// name, click it to open FigureDetail, and click PLACE ON PORTAL. Does NOT
/// wait for the slot to transition — caller uses `wait_slot_label`.
async fn place_figure(phone: &Phone, slot: u8, figure_name: &str) {
    let slots = phone
        .client
        .find_all(Locator::Css(".p4-slot"))
        .await
        .expect("slots findable");
    let slot_el = slots
        .get((slot - 1) as usize)
        .unwrap_or_else(|| panic!("no slot {slot}"));
    slot_el.clone().click().await.expect("tap slot");

    let mut opened = false;
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
            opened = true;
            break;
        }
    }
    assert!(opened, "no .fig-card-p4 matched {figure_name:?}");

    let place = phone
        .wait_for(Locator::Css(".detail-btn-primary"), Duration::from_secs(5))
        .await
        .expect("PLACE ON PORTAL button");
    place.click().await.expect("click PLACE");
}

/// Close the FigureDetail overlay by clicking BACK TO BOX. The detail view
/// doesn't auto-dismiss after a successful place, so tests that want to
/// re-enter the figure grid (e.g. to place a second figure) must do this
/// explicitly. No-op if the detail isn't open.
async fn dismiss_figure_detail(phone: &Phone) {
    if let Ok(btn) = phone
        .client
        .find(Locator::Css(".detail-btn-secondary"))
        .await
    {
        let _ = btn.click().await;
    }
}

/// Block until the Nth slot's `.p4-slot-label` text equals `expected`.
async fn wait_slot_label(phone: &Phone, slot: u8, expected: &str) {
    let idx = (slot - 1) as usize;
    phone
        .wait_until(Duration::from_secs(30), || async {
            let labels = match phone.client.find_all(Locator::Css(".p4-slot-label")).await {
                Ok(v) => v,
                Err(_) => return false,
            };
            match labels.get(idx) {
                Some(l) => l.text().await.map(|t| t == expected).unwrap_or(false),
                None => false,
            }
        })
        .await
        .unwrap_or_else(|_| panic!("slot {slot} never showed {expected:?} within 30s"));
}

/// Block until the Nth slot shows the empty-label class.
async fn wait_slot_empty(phone: &Phone, slot: u8) {
    let idx = (slot - 1) as usize;
    phone
        .wait_until(Duration::from_secs(30), || async {
            let labels = match phone.client.find_all(Locator::Css(".p4-slot-label")).await {
                Ok(v) => v,
                Err(_) => return false,
            };
            match labels.get(idx) {
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
        .unwrap_or_else(|_| panic!("slot {slot} never emptied within 30s"));
}

/// Click REMOVE on the Nth slot. The action overlay is scoped to the slot
/// container via `:nth-child`, so multi-slot tests can target a specific
/// one instead of grabbing whichever remove button renders first.
async fn remove_slot(phone: &Phone, slot: u8) {
    let sel = format!(".portal-p4 .p4-slot:nth-child({slot}) .p4-slot-action--remove");
    let btn = phone
        .client
        .find(Locator::Css(&sel))
        .await
        .unwrap_or_else(|_| panic!("no remove button for slot {slot} (selector {sel})"));
    btn.click().await.expect("click REMOVE");
}

// ======================================================================
// Tests
// ======================================================================

/// PLAN 3.7.7 — baseline end-to-end load + clear through the phone.
#[tokio::test(flavor = "current_thread")]
#[ignore = "requires RPCS3_EXE, RPCS3_TEST_SERIAL, RPCS3_SKY_TEST_PATH, chromedriver, built phone SPA"]
async fn phone_drives_real_rpcs3_load_and_clear() {
    let (serial, sky_path) = match require_env() {
        Some(t) => t,
        None => {
            eprintln!("skipping: set RPCS3_EXE / RPCS3_TEST_SERIAL / RPCS3_SKY_TEST_PATH");
            return;
        }
    };
    let figure_name = canonical_name_from_path(&sky_path);
    let (_server, phone) = spawn_and_land_on_portal(&serial).await;

    place_figure(&phone, 1, &figure_name).await;
    wait_slot_label(&phone, 1, &figure_name).await;

    remove_slot(&phone, 1).await;
    wait_slot_empty(&phone, 1).await;

    phone.close().await.ok();
}

/// PLAN 3.7.9.1 — load A, REMOVE, load B. Exercises sequential ops against
/// the real Skylanders Manager dialog: the clear path, the once-per-session
/// short-circuit on `open_dialog`, the second full load without any
/// lingering file-dialog or modal state from the first.
#[tokio::test(flavor = "current_thread")]
#[ignore = "requires RPCS3_EXE, RPCS3_TEST_SERIAL, RPCS3_SKY_TEST_PATH, RPCS3_SKY_TEST_PATH_2"]
async fn live_clear_then_load_different() {
    let (serial, sky_a, sky_b) = match require_env_two_figures() {
        Some(t) => t,
        None => {
            eprintln!("skipping: set RPCS3_SKY_TEST_PATH_2 to a second .sky in the same pack");
            return;
        }
    };
    let fig_a = canonical_name_from_path(&sky_a);
    let fig_b = canonical_name_from_path(&sky_b);
    assert_ne!(fig_a, fig_b, "sky path 1 and 2 must be different figures");

    let (_server, phone) = spawn_and_land_on_portal(&serial).await;

    place_figure(&phone, 1, &fig_a).await;
    wait_slot_label(&phone, 1, &fig_a).await;
    dismiss_figure_detail(&phone).await;

    remove_slot(&phone, 1).await;
    wait_slot_empty(&phone, 1).await;

    place_figure(&phone, 1, &fig_b).await;
    wait_slot_label(&phone, 1, &fig_b).await;

    phone.close().await.ok();
}

/// PLAN 3.7.9.2 — rapid 5× click on PLACE ON PORTAL. Validates that the
/// `DetailState::Loading` guard in `FigureDetail::on_place` plus the
/// server's per-slot `Loading` back-pressure prevent duplicate loads under
/// real driver latency (file-dialog open takes ~2s; any race between click
/// 1 and click 2 would fire two POSTs).
#[tokio::test(flavor = "current_thread")]
#[ignore = "requires RPCS3_EXE, RPCS3_TEST_SERIAL, RPCS3_SKY_TEST_PATH"]
async fn live_spam_click_same_slot() {
    let (serial, sky_path) = match require_env() {
        Some(t) => t,
        None => {
            eprintln!("skipping: set RPCS3_EXE / RPCS3_TEST_SERIAL / RPCS3_SKY_TEST_PATH");
            return;
        }
    };
    let figure_name = canonical_name_from_path(&sky_path);
    let (_server, phone) = spawn_and_land_on_portal(&serial).await;

    // Tap slot 1, open detail for the test figure.
    let slots = phone
        .client
        .find_all(Locator::Css(".p4-slot"))
        .await
        .unwrap();
    slots[0].clone().click().await.expect("tap slot 1");

    let mut opened = false;
    for card in phone
        .client
        .find_all(Locator::Css(".fig-card-p4"))
        .await
        .unwrap()
    {
        let name = card
            .find(Locator::Css(".fig-name-p4"))
            .await
            .unwrap()
            .text()
            .await
            .unwrap_or_default();
        if name == figure_name {
            card.click().await.unwrap();
            opened = true;
            break;
        }
    }
    assert!(opened, "no card named {figure_name:?}");

    // 5× rapid click on PLACE ON PORTAL. Client-side guard should no-op
    // clicks 2..5 because DetailState transitions to Loading on click 1.
    let place = phone
        .wait_for(Locator::Css(".detail-btn-primary"), Duration::from_secs(5))
        .await
        .expect("PLACE button");
    for _ in 0..5 {
        let _ = place.clone().click().await;
    }

    // Slot must still land on the correct figure (not error, not stuck).
    wait_slot_label(&phone, 1, &figure_name).await;

    // No error toast should have fired — a duplicate-load slip-through
    // would surface as a "slot busy" or similar. `toast_count` counts
    // currently-rendered toasts; "Launched …" etc. are auto-dismissed
    // within a few seconds, so we don't assert count == 0 at arbitrary
    // time. Instead, look for a visible error banner via `.detail-err-icon`
    // which renders only when `DetailState::Errored`.
    let err_visible = phone
        .client
        .find_all(Locator::Css(".detail-errored"))
        .await
        .map(|v| !v.is_empty())
        .unwrap_or(false);
    assert!(
        !err_visible,
        "spam-click surfaced a detail error — back-pressure leaked a second load"
    );

    phone.close().await.ok();
}

/// PLAN 3.7.9.3 — load two figures, `location.reload()`, wait for the
/// ResumeModal, click RESUME, assert both slots re-materialize. Exercises
/// the 3.12 resume-prompt path end-to-end with the real driver: profile
/// unlock on the new WS session triggers `Event::ResumePrompt`, which
/// drives per-slot `post_load` calls that re-run the real driver.
#[tokio::test(flavor = "current_thread")]
#[ignore = "requires RPCS3_EXE, RPCS3_TEST_SERIAL, RPCS3_SKY_TEST_PATH, RPCS3_SKY_TEST_PATH_2"]
async fn live_resume_after_reload() {
    let (serial, sky_a, sky_b) = match require_env_two_figures() {
        Some(t) => t,
        None => {
            eprintln!("skipping: set RPCS3_SKY_TEST_PATH_2 to a second .sky in the same pack");
            return;
        }
    };
    let fig_a = canonical_name_from_path(&sky_a);
    let fig_b = canonical_name_from_path(&sky_b);
    assert_ne!(fig_a, fig_b, "sky path 1 and 2 must be different figures");

    let (server, phone) = spawn_and_land_on_portal(&serial).await;

    // Load both figures. Saved layout now has A@1 and B@2.
    place_figure(&phone, 1, &fig_a).await;
    wait_slot_label(&phone, 1, &fig_a).await;
    dismiss_figure_detail(&phone).await;

    place_figure(&phone, 2, &fig_b).await;
    wait_slot_label(&phone, 2, &fig_b).await;
    dismiss_figure_detail(&phone).await;

    // Reload the browser. This drops the WS session and the phone's
    // in-memory state; on reconnect, the new session needs a profile
    // unlock before the server will emit ResumePrompt. Mirrors the
    // pattern in `regressions.rs::ws_reconnect`.
    let _ = phone.client.execute("location.reload();", vec![]).await;
    unlock_default_profile(&server.url)
        .await
        .expect("re-seed profile unlock post-reload");

    // ResumeModal appears. The panel class comes from modals.rs.
    phone
        .wait_for(Locator::Css(".resume-panel"), Duration::from_secs(30))
        .await
        .expect("ResumeModal didn't appear post-reload");

    // Click RESUME — fires per-slot post_load for each saved slot.
    let resume_btn = phone
        .client
        .find(Locator::Css(".resume-btn-primary"))
        .await
        .expect(".resume-btn-primary");
    resume_btn.click().await.expect("click RESUME");

    // Modal dismissed + both slots land Loaded again. Since the portal
    // state in memory was never cleared (server didn't restart), resume
    // playback re-runs the driver load for already-Loaded slots; the
    // end state is still correct, which is what we're validating.
    wait_slot_label(&phone, 1, &fig_a).await;
    wait_slot_label(&phone, 2, &fig_b).await;

    phone.close().await.ok();
}

// ======================================================================
// 3.10f — multi-phone scenarios against the live driver
// ======================================================================

/// Helper: poll until the phone's session id is exposed in the DOM. Matches
/// the pattern in the mock `multi_phone.rs` tests. Returns `u64` for parity
/// with `Phone::session_id()`'s return shape.
async fn wait_for_session_id(phone: &Phone) -> u64 {
    for _ in 0..50 {
        if let Ok(Some(id)) = phone.session_id().await {
            return id;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    panic!("phone never received Event::Welcome");
}

/// Click the matching `.game-card` on the given phone. Extracted out of
/// `spawn_and_land_on_portal` so multi-phone tests can drive the launch from
/// one phone and observe the auto-transition on the other.
async fn click_game_card_for_serial(phone: &Phone, serial_short_name: &str) {
    phone
        .wait_for(Locator::Css(".game-picker"), Duration::from_secs(10))
        .await
        .expect("game picker renders");
    // Poll around the staggered `gp-card-rise` animation — see
    // `spawn_and_land_on_portal` for the full explanation.
    phone
        .wait_until(Duration::from_secs(10), || async {
            let cards = match phone.client.find_all(Locator::Css(".game-card")).await {
                Ok(v) => v,
                Err(_) => return false,
            };
            for card in cards {
                if let Ok(label_el) = card.find(Locator::Css(".game-name")).await
                    && let Ok(text) = label_el.text().await
                    && text.eq_ignore_ascii_case(serial_short_name)
                {
                    return true;
                }
            }
            false
        })
        .await
        .unwrap_or_else(|_| panic!("no .game-card rendered matching {serial_short_name:?}"));

    for card in phone
        .client
        .find_all(Locator::Css(".game-card"))
        .await
        .unwrap()
    {
        let label = card
            .find(Locator::Css(".game-name"))
            .await
            .unwrap()
            .text()
            .await
            .unwrap_or_default();
        if label.eq_ignore_ascii_case(serial_short_name) {
            card.click().await.expect("click game card");
            return;
        }
    }
}

/// PLAN 3.10f.1 — two phones on the same profile, each places a different
/// figure into a different slot. Both phones observe both slots Loaded via
/// the shared `SlotChanged` broadcast. Validates the driver worker
/// serialises the two concurrent `LoadFigure` jobs through the single
/// Skylanders Manager dialog without dropping one or racing the file
/// picker's offscreen sling.
#[tokio::test(flavor = "current_thread")]
#[ignore = "requires RPCS3_EXE, RPCS3_TEST_SERIAL, RPCS3_SKY_TEST_PATH, RPCS3_SKY_TEST_PATH_2"]
async fn live_concurrent_edits_both_phones() {
    let (serial, sky_a, sky_b) = match require_env_two_figures() {
        Some(t) => t,
        None => {
            eprintln!("skipping: set RPCS3_SKY_TEST_PATH_2 to a second .sky in the same pack");
            return;
        }
    };
    let fig_a = canonical_name_from_path(&sky_a);
    let fig_b = canonical_name_from_path(&sky_b);
    assert_ne!(fig_a, fig_b, "paths 1 and 2 must be different figures");

    let server = TestServer::spawn_live().expect("spawn live server");
    unlock_default_profile(&server.url)
        .await
        .expect("seed pending_unlock for phone 1");

    let p1 = Phone::new(&server.phone_url().await.unwrap(), &server.chromedriver_url)
        .await
        .expect("connect phone 1");
    let s1 = wait_for_session_id(&p1).await;

    // Re-seed pending_unlock so P2 also lands on the profile (otherwise
    // P2 starts at the profile picker).
    unlock_default_profile(&server.url)
        .await
        .expect("re-seed pending_unlock for phone 2");
    let p2 = Phone::new(&server.phone_url().await.unwrap(), &server.chromedriver_url)
        .await
        .expect("connect phone 2");
    let s2 = wait_for_session_id(&p2).await;
    assert_ne!(s1, s2, "each phone must get a distinct session id");

    // P1 launches the game; P2 transitions to portal via GameChanged broadcast.
    let short_name = short_name_for_serial(&server.url, &serial).await;
    click_game_card_for_serial(&p1, &short_name).await;
    for phone in [&p1, &p2] {
        phone
            .wait_for(Locator::Css(".screen-portal"), Duration::from_secs(120))
            .await
            .expect("portal screen");
    }

    // Each phone places a different figure in a different slot. Sequential
    // at the fantoccini layer (one client per phone, each drives synchronously)
    // but the driver worker still serialises — the second LoadFigure is
    // queued while the first is in flight, and the assertion on both phones
    // seeing both slots validates the queued job actually ran.
    place_figure(&p1, 1, &fig_a).await;
    wait_slot_label(&p1, 1, &fig_a).await;
    dismiss_figure_detail(&p1).await;

    place_figure(&p2, 2, &fig_b).await;
    wait_slot_label(&p2, 2, &fig_b).await;
    dismiss_figure_detail(&p2).await;

    // Cross-phone visibility: P1 sees slot 2 Loaded with B, P2 sees slot 1
    // Loaded with A. These come through the shared `SlotChanged` broadcast.
    wait_slot_label(&p1, 2, &fig_b).await;
    wait_slot_label(&p2, 1, &fig_a).await;

    p1.close().await.ok();
    p2.close().await.ok();
}

/// PLAN 3.10f.2 — two phones on two distinct profiles. Each places a
/// figure; assert each profile gets its own `working/<profile_id>/` dir
/// on disk. Validates the real-driver pass through
/// `crate::working_copies::resolve_load_path` — mock-backed tests can't
/// surface this because they don't actually touch the filesystem.
#[tokio::test(flavor = "current_thread")]
#[ignore = "requires RPCS3_EXE, RPCS3_TEST_SERIAL, RPCS3_SKY_TEST_PATH, RPCS3_SKY_TEST_PATH_2"]
async fn live_independent_profiles_loads() {
    let (serial, sky_a, sky_b) = match require_env_two_figures() {
        Some(t) => t,
        None => {
            eprintln!("skipping: set RPCS3_SKY_TEST_PATH_2 to a second .sky in the same pack");
            return;
        }
    };
    let fig_a = canonical_name_from_path(&sky_a);
    let fig_b = canonical_name_from_path(&sky_b);

    let server = TestServer::spawn_live().expect("spawn live server");

    // Two distinct profiles. Same pattern as mock multi-phone
    // `independent_profile_unlock` but with live driver for the subsequent
    // loads.
    let pid_a = inject_profile(&server.url, "Alpha", "1111", "#ff00ff")
        .await
        .expect("inject profile Alpha");
    let pid_b = inject_profile(&server.url, "Beta", "2222", "#00ffff")
        .await
        .expect("inject profile Beta");

    // Seed P1 on profile A.
    unlock_session(&server.url, &pid_a).await.expect("unlock A");
    let p1 = Phone::new(&server.phone_url().await.unwrap(), &server.chromedriver_url)
        .await
        .expect("connect P1");
    let _s1 = wait_for_session_id(&p1).await;

    // P2 inherits pending_unlock (A); flip it to B via set_session_profile
    // on P2's session id specifically.
    let p2 = Phone::new(&server.phone_url().await.unwrap(), &server.chromedriver_url)
        .await
        .expect("connect P2");
    let s2 = wait_for_session_id(&p2).await;
    set_session_profile(&server.url, s2, &pid_b)
        .await
        .expect("flip P2 to profile B");

    // P1 launches the game; P2 transitions to portal via GameChanged.
    let short_name = short_name_for_serial(&server.url, &serial).await;
    click_game_card_for_serial(&p1, &short_name).await;
    for phone in [&p1, &p2] {
        phone
            .wait_for(Locator::Css(".screen-portal"), Duration::from_secs(120))
            .await
            .expect("portal screen");
    }

    // Each phone places its figure under its own profile.
    place_figure(&p1, 1, &fig_a).await;
    wait_slot_label(&p1, 1, &fig_a).await;
    dismiss_figure_detail(&p1).await;

    place_figure(&p2, 2, &fig_b).await;
    wait_slot_label(&p2, 2, &fig_b).await;
    dismiss_figure_detail(&p2).await;

    // Each profile must have its own working directory with a .sky inside.
    // Bug would surface as only one dir existing (loads all routed to one
    // profile) or both dirs being the same path.
    assert_ne!(pid_a, pid_b, "injected profile ids must differ");
    let working_root = server.dev_data_dir().join("working");
    let working_a = working_root.join(&pid_a);
    let working_b = working_root.join(&pid_b);
    assert!(
        working_a.is_dir(),
        "profile A working dir missing: {}",
        working_a.display()
    );
    assert!(
        working_b.is_dir(),
        "profile B working dir missing: {}",
        working_b.display()
    );
    let a_files: Vec<_> = std::fs::read_dir(&working_a)
        .expect("read profile A dir")
        .flatten()
        .collect();
    let b_files: Vec<_> = std::fs::read_dir(&working_b)
        .expect("read profile B dir")
        .flatten()
        .collect();
    assert!(
        !a_files.is_empty(),
        "profile A working dir is empty: {}",
        working_a.display()
    );
    assert!(
        !b_files.is_empty(),
        "profile B working dir is empty: {}",
        working_b.display()
    );

    p1.close().await.ok();
    p2.close().await.ok();
}
