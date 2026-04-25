//! Walks the phone SPA through the canonical first-time-user flow and
//! saves a PNG per screen to `docs/assets/screens/` for the docs
//! site's flow-through gallery (PLAN 8.4 follow-up).
//!
//! Not a regression test in the usual sense — it exists to *generate*
//! marketing assets, not to assert behavior. Failures here mean a
//! selector drifted out of sync with the phone DOM, or a screen
//! transition timed out; either way the user re-runs after fixing.
//!
//! Run with:
//!
//! ```text
//! cargo test -p skylander-e2e-tests --test screenshot_tour \
//!     -- --ignored --nocapture
//! ```
//!
//! Requirements: same as the rest of the e2e suite — Chrome,
//! ChromeDriver, a built phone bundle, and the firmware pack at the
//! standard dev path (or `SKYLANDER_PACK_ROOT` overriding it).

use std::path::PathBuf;
use std::time::Duration;

use anyhow::{Context, Result};
use fantoccini::Locator;
use skylander_e2e_tests::{
    Phone, TestServer, fire_kaos_taunt, fire_takeover, inject_load_outcomes, inject_profile,
    launch_giants, set_session_profile, unlock_session,
};

/// Where we drop the captured PNGs. Resolved at test start so any
/// failure halfway through still leaves the partially-populated
/// directory for inspection.
fn screens_dir() -> Result<PathBuf> {
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let repo = manifest
        .ancestors()
        .nth(2)
        .context("locate repo root from CARGO_MANIFEST_DIR")?
        .to_path_buf();
    let dir = repo.join("docs").join("assets").join("screens");
    std::fs::create_dir_all(&dir).with_context(|| format!("mkdir {}", dir.display()))?;
    Ok(dir)
}

/// Settle margin between an action that should produce a transition
/// and the screenshot, so animations + WS broadcasts reach steady
/// state before the camera fires. Tuned for the staggered iris /
/// lid / takeover animations — bump if a frame catches mid-motion.
async fn settle() {
    tokio::time::sleep(Duration::from_millis(550)).await;
}

/// Dispatch synthetic `pointerdown` + `pointerup` events on the
/// element matched by `selector`, simulating a tap. The toy-box
/// lid's tap detector listens to PointerEvents specifically (not
/// click), and WebDriver's `client.click()` doesn't synthesize
/// PointerEvents reliably in headless Chrome — so the only way
/// to reach `apply_tap` is to dispatch the events ourselves.
/// No-op if the selector matches nothing.
async fn tap_via_pointer(phone: &Phone, selector: &str) -> Result<()> {
    let js = format!(
        r#"
        const el = document.querySelector('{sel}');
        if (!el) return null;
        const r = el.getBoundingClientRect();
        const opts = {{
            pointerId: 1, isPrimary: true, bubbles: true,
            clientX: r.left + r.width / 2,
            clientY: r.top + r.height / 2,
        }};
        el.dispatchEvent(new PointerEvent('pointerdown', opts));
        el.dispatchEvent(new PointerEvent('pointerup',   opts));
        return true;
        "#,
        sel = selector,
    );
    let _ = phone.client.execute(&js, vec![]).await?;
    Ok(())
}

#[tokio::test(flavor = "current_thread")]
#[ignore = "requires chromedriver + manual run; populates docs/assets/screens/"]
async fn capture_full_phone_tour() -> Result<()> {
    let server = TestServer::spawn().context("spawn test server")?;
    let dir = screens_dir()?;

    // ---- profiles + game state for a representative shoot --------------
    //
    // Inject 3 profiles so the profile picker has visible diversity,
    // not just one card. Colours match the design tokens we use
    // elsewhere — gold, magenta, teal — so the captured chips read
    // as a real family setup.
    let alice = inject_profile(&server.url, "Alice", "1111", "#f5c634").await?;
    let _bob = inject_profile(&server.url, "Bob", "2222", "#da28a8").await?;
    let _cami = inject_profile(&server.url, "Cami", "3333", "#39d39f").await?;
    launch_giants(&server.url).await?;

    // ---------------------------------------------------------------- 01
    // Profile picker — first phone connecting after the QR scan.
    let phone_url = server.phone_url().await?;
    let phone = Phone::new(&phone_url, &server.chromedriver_url).await?;
    // Wait for the picker section first; profile-card buttons populate
    // after the async fetch_profiles roundtrip. Catching the wrapper
    // proves the right screen mounted; then we wait for cards to settle.
    phone
        .wait_for(Locator::Css(".profile-picker"), Duration::from_secs(15))
        .await?;
    if phone
        .wait_until(Duration::from_secs(8), || async {
            phone
                .client
                .find(Locator::Css(".profile-card"))
                .await
                .is_ok()
        })
        .await
        .is_err()
    {
        let body = phone
            .client
            .find(Locator::Css("body"))
            .await?
            .html(false)
            .await
            .unwrap_or_default();
        eprintln!("[tour] profile-card never appeared. body snippet:");
        eprintln!("{}", &body[..body.len().min(2000)]);
    }
    settle().await;
    phone.screenshot(dir.join("01-profile-picker.png")).await?;

    // ---------------------------------------------------------------- 02
    // PIN entry — tap Alice's profile card; PIN keypad screen slides
    // up. Existing visual_scroll_probes test confirms the wrapper is
    // `.pin-entry-screen`.
    let cards = phone
        .client
        .find_all(Locator::Css(".profile-card:not(.add)"))
        .await?;
    let alice_card = cards.first().context("no profile cards")?.clone();
    alice_card.click().await?;
    phone
        .wait_for(Locator::Css(".pin-entry-screen"), Duration::from_secs(6))
        .await?;
    settle().await;
    phone.screenshot(dir.join("02-pin-entry.png")).await?;

    // ---------------------------------------------------------------- 03
    // Game picker — clear the current game so all 6 cards render
    // selectable (no "currently playing" highlight), bypass the PIN
    // via the test-hook session-bind, and let the phone transition
    // through `unlocked_profile = Some(...)` + `current_game = None`
    // → game picker. No page reload — staying inside the same phone
    // session keeps the localStorage / fetch state stable.
    skylander_e2e_tests::set_game(&server.url, None).await?;
    // Tap the PIN screen's back button so the local `picked` signal
    // clears; without this the PIN keypad sticks around even after
    // ProfileChanged comes through, since the picker's child is
    // chosen by the local picked-signal not the unlocked one.
    if let Ok(back) = phone.client.find(Locator::Css(".btn-back")).await {
        back.click().await.ok();
        tokio::time::sleep(Duration::from_millis(250)).await;
    }
    phone
        .wait_until(Duration::from_secs(5), || async {
            phone.session_id().await.ok().flatten().is_some()
        })
        .await
        .context("session id never populated")?;
    let sid = phone.session_id().await?.context("session id missing")?;
    set_session_profile(&server.url, sid, &alice).await?;
    phone
        .wait_for(Locator::Css(".game-card"), Duration::from_secs(10))
        .await?;
    settle().await;
    phone.screenshot(dir.join("03-game-picker.png")).await?;

    // ---------------------------------------------------------------- 04
    // Empty portal — Giants is now booted; no figures placed; the
    // toy-box arrow hint is the lone call-to-action.
    launch_giants(&server.url).await?;
    phone
        .wait_for(Locator::Css(".portal-p4"), Duration::from_secs(15))
        .await?;
    // The empty-hint should be visible. Settle for the iris/swap-in
    // animations to complete.
    settle().await;
    phone.screenshot(dir.join("04-portal-empty.png")).await?;

    // ---------------------------------------------------------------- 05
    // Toy box open — tap the lid grabber to cycle Closed → Compact
    // → Expanded. The grabber listens to PointerEvents, not click;
    // see `tap_via_pointer` doc comment.
    tap_via_pointer(&phone, ".lid-grabber-p4").await?;
    tokio::time::sleep(Duration::from_millis(280)).await;
    tap_via_pointer(&phone, ".lid-grabber-p4").await?;
    let _ = phone
        .wait_for(Locator::Css(".fig-card-p4"), Duration::from_secs(8))
        .await;
    settle().await;
    phone.screenshot(dir.join("05-toy-box.png")).await?;

    // ---------------------------------------------------------------- 06a
    // Figure detail — tapping a fig-card in the toy box opens the
    // FigureDetail overlay (PLACE ON PORTAL primary CTA, BACK TO BOX
    // secondary). Worth its own screen since the detail overlay is
    // distinct UI.
    let cards = phone
        .client
        .find_all(Locator::Css(".fig-card-p4:not(.scan-new)"))
        .await
        .unwrap_or_default();
    let first_card = cards.first().context("no figure cards in toy box")?.clone();
    first_card.click().await?;
    phone
        .wait_for(Locator::Css(".detail-btn-primary"), Duration::from_secs(5))
        .await?;
    settle().await;
    phone.screenshot(dir.join("06-figure-detail.png")).await?;

    // ---------------------------------------------------------------- 06b
    // Place this figure + a second one through the same flow so the
    // portal-loaded shot shows two occupied slots (more visually
    // honest than a single placed figure).
    inject_load_outcomes(
        &server.url,
        serde_json::json!([{"kind": "ok"}, {"kind": "ok"}]),
    )
    .await?;
    // Place the figure currently shown in the detail.
    phone
        .client
        .find(Locator::Css(".detail-btn-primary"))
        .await?
        .click()
        .await?;
    // Click BACK TO BOX (.detail-btn-secondary) to dismiss detail
    // and re-enter the figure grid for the second pick.
    if let Ok(back) = phone
        .client
        .find(Locator::Css(".detail-btn-secondary"))
        .await
    {
        back.click().await.ok();
        tokio::time::sleep(Duration::from_millis(280)).await;
    }
    // Wait for first slot to show Loaded.
    let _ = phone
        .wait_until(Duration::from_secs(8), || async {
            phone
                .client
                .find(Locator::Css(".p4-slot--loaded"))
                .await
                .is_ok()
        })
        .await;
    // Pick a second figure (skip the one already on the portal — its
    // card now has the `.on-portal` modifier).
    let cards = phone
        .client
        .find_all(Locator::Css(".fig-card-p4:not(.scan-new):not(.on-portal)"))
        .await
        .unwrap_or_default();
    if let Some(card) = cards.first() {
        card.clone().click().await.ok();
        tokio::time::sleep(Duration::from_millis(280)).await;
        if let Ok(place) = phone.client.find(Locator::Css(".detail-btn-primary")).await {
            place.click().await.ok();
            tokio::time::sleep(Duration::from_millis(280)).await;
        }
        if let Ok(back) = phone
            .client
            .find(Locator::Css(".detail-btn-secondary"))
            .await
        {
            back.click().await.ok();
            tokio::time::sleep(Duration::from_millis(280)).await;
        }
    }
    // Wait for two Loaded slots, then collapse the lid so the
    // portal slots are foreground.
    let _ = phone
        .wait_until(Duration::from_secs(8), || async {
            let loaded = phone
                .client
                .find_all(Locator::Css(".p4-slot--loaded"))
                .await
                .unwrap_or_default();
            loaded.len() >= 2
        })
        .await;
    // Reload the phone so it lands in the default lid-Closed state
    // with the placed figures still on the portal (ghost reclaim
    // adopts the session via the localStorage `reclaim_profile_id`
    // hint set on the prior ProfileChanged). Driving the lid via
    // synthetic pointer events doesn't reliably reach the tap
    // detector in headless Chrome, so a reload is the cleanest
    // path to the foreground-portal screenshot.
    phone.client.goto(&phone_url).await?;
    phone
        .wait_for(Locator::Css(".portal-p4"), Duration::from_secs(15))
        .await?;
    let _ = phone
        .wait_until(Duration::from_secs(8), || async {
            phone
                .client
                .find(Locator::Css(".p4-slot--loaded"))
                .await
                .is_ok()
        })
        .await;
    settle().await;
    phone.screenshot(dir.join("07-portal-loaded.png")).await?;

    // ---------------------------------------------------------------- 07
    // Menu overlay — tap the header kebab.
    if let Ok(kebab) = phone.client.find(Locator::Css(".kebab-btn")).await {
        kebab.click().await?;
        phone
            .wait_for(Locator::Css(".menu-overlay-panel"), Duration::from_secs(4))
            .await?;
        settle().await;
        phone.screenshot(dir.join("08-menu-overlay.png")).await?;
        // Close it before continuing.
        if let Ok(close) = phone.client.find(Locator::Css(".menu-close")).await {
            close.click().await.ok();
        }
        tokio::time::sleep(Duration::from_millis(300)).await;
    }

    // ---------------------------------------------------------------- 08
    // Kaos swap overlay — fire a synthetic taunt via test-hook,
    // capture the overlay before the 5s auto-dismiss timer fires.
    fire_kaos_taunt(
        &server.url,
        &alice,
        0,
        "spyro",
        "wham-shell",
        "Behold \u{2014} I have IMPROVED your little team!",
    )
    .await?;
    phone
        .wait_for(
            Locator::Css(".takeover-viewport--swap"),
            Duration::from_secs(4),
        )
        .await?;
    settle().await;
    phone.screenshot(dir.join("09-kaos-swap.png")).await?;
    // Wait for auto-dismiss before continuing.
    tokio::time::sleep(Duration::from_secs(5)).await;

    // ---------------------------------------------------------------- 09
    // Kaos takeover overlay — fire synthetic TakenOver against the
    // current session id. cooldown = 47 to capture the disabled
    // KICK BACK IN button mid-countdown.
    let sid = phone.session_id().await?.context("session id missing")?;
    fire_takeover(
        &server.url,
        sid,
        "Hahahaha! The portal kneels before Kaos!",
        47,
    )
    .await?;
    phone
        .wait_for(Locator::Css(".takeover-kick-btn"), Duration::from_secs(4))
        .await?;
    settle().await;
    phone.screenshot(dir.join("10-kaos-takeover.png")).await?;

    // -----------------------------------------------------------------
    let _ = unlock_session; // silence unused-import on this path
    eprintln!("[tour] saved {} screens under {}", 10, dir.display());
    phone.close().await?;
    Ok(())
}
