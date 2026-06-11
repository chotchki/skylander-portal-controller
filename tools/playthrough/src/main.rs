//! PLAN 15 — desktop-mode play-through recorder.
//!
//! Boots the launcher in Phase-20 **Desktop mode** (windowed) with the mock
//! driver (no RPCS3), records the **whole primary monitor to an MP4** via
//! `windows-capture` (no external ffmpeg), and — while recording — drives the
//! phone SPA in a VISIBLE Chrome window beside the launcher through a chosen
//! scenario. Outputs an MP4 + a still PNG.
//!
//! Scenarios (first CLI arg, default `place`):
//!   - `portal` — reach the empty portal and hold (mock driver).
//!   - `place`  — open the toy box and place two figures on the portal (mock, hero).
//!   - `ingame` — PLAN 15.12: boot a real Spyro SAVE STATE on the patched RPCS3
//!     over IPC (straight to the in-game portal) and place a figure from the
//!     phone (a real IPC LOAD) so it shows up on the GAME's own portal. HTPC-only;
//!     requires RPCS3_EXE + RPCS3_CONFIG_DIR + SKYLANDER_BOOT_SAVESTATE in the env.
//!
//! Run (build the phone with the harness's pinned token first; point
//! CHROMEDRIVER at a build matching your installed Chrome):
//!   cd phone && BUILD_TOKEN=e2e-test trunk build
//!   CHROMEDRIVER=<matching chromedriver.exe> cargo run -p skylander-playthrough -- place
//!   # in-game (HTPC, patched RPCS3 + a Spyro save state at the portal):
//!   RPCS3_EXE=…\rpcs3.exe  RPCS3_CONFIG_DIR=…\rpcs3\  \
//!     SKYLANDER_BOOT_SAVESTATE=…\BLUS30779_1_0.SAVESTAT.zst  \
//!     CHROMEDRIVER=…  cargo run -p skylander-playthrough -- ingame

mod capture;

use std::time::Duration;

use anyhow::{Context, Result};
use capture::DesktopCapture;
use fantoccini::Locator;
use serde_json::json;
use skylander_e2e_tests::{
    Phone, TestServer, inject_load_outcomes, inject_profile, launch_giants, unlock_session,
};

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::INFO)
        .init();
    let scenario = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "place".to_string());
    tracing::info!(%scenario, "play-through scenario");

    // PLAN 15.12 — the in-game tier is a different harness (real IPC + patched
    // RPCS3 booting a save state), so it runs its own path, not the mock setup.
    if scenario == "ingame" {
        return ingame().await;
    }

    // 1. Launcher in Desktop mode (windowed), mock driver + test-hooks.
    let server = TestServer::spawn_with_env_lines("WINDOW_MODE=desktop\n")
        .context("spawn server in desktop window mode")?;
    tracing::info!(url = %server.url, "server up — desktop mode, mock driver");

    // 2. Seed a small family + boot Giants (test-hooks; no RPCS3 needed).
    let alice = inject_profile(&server.url, "Alice", "1111", "#f5c634").await?;
    let _bob = inject_profile(&server.url, "Bob", "2222", "#da28a8").await?;
    launch_giants(&server.url).await?;

    // 3. Start whole-desktop MP4 capture (launcher is up + visible).
    let mp4 = std::env::temp_dir().join("playthrough-desktop.mp4");
    let cap = DesktopCapture::start(&mp4).context("start desktop capture")?;
    tracing::info!(mp4 = %mp4.display(), "recording the desktop…");
    tokio::time::sleep(Duration::from_secs(1)).await; // a beat with just the launcher

    // 4. Visible Chrome parked to the right of the windowed launcher.
    let phone_url = server.phone_url().await?;
    let phone = Phone::new_headed(&phone_url, &server.chromedriver_url, 1180, 40, 470, 940)
        .await
        .context("open headed phone browser")?;
    tracing::info!("phone browser open (headed) — driving the flow");

    // 5. Reach the portal: profile picker → unlock Alice (PIN bypass) → portal.
    phone
        .wait_for(Locator::Css(".profile-picker"), Duration::from_secs(20))
        .await
        .context("profile picker never mounted")?;
    unlock_session(&server.url, &alice).await?;
    if phone
        .wait_for(Locator::Css(".portal-p4"), Duration::from_secs(15))
        .await
        .is_err()
    {
        tracing::warn!("portal view not reached in time — recording whatever rendered");
    }

    // 6. Scenario-specific drive.
    match scenario.as_str() {
        "portal" => {
            tokio::time::sleep(Duration::from_secs(5)).await;
        }
        _ => {
            // "place" (default): open the toy box + place two figures.
            place_figures(&phone, &server, &phone_url)
                .await
                .context("place-figures scenario")?;
        }
    }

    // 7. Hold so the final state is on screen, then stop + flush the MP4.
    tokio::time::sleep(Duration::from_secs(3)).await;
    cap.stop().context("stop + flush desktop capture")?;
    tracing::info!(mp4 = %mp4.display(), "MP4 written");
    let png = std::env::temp_dir().join("playthrough-desktop.png");
    phone.screenshot(&png).await?;
    tracing::info!(screenshot = %png.display(), "still captured");

    phone.close().await.ok();
    tracing::info!("done");
    Ok(())
}

/// PLAN 15.12 — in-game tier. Boots a real Spyro SAVE STATE on the patched RPCS3
/// over IPC (straight to the in-game portal, no menu nav), then places a figure
/// from the phone — a REAL IPC `LOAD` onto the resumed game — so it appears on
/// the GAME's own portal. The whole desktop (launcher + RPCS3 window + phone) is
/// recorded. HTPC-only; the server reads RPCS3_EXE / RPCS3_CONFIG_DIR /
/// SKYLANDER_BOOT_SAVESTATE from the env (see `spawn_ipc_savestate`), and the
/// save-state RPCS3 config (ASMJIT + Compatible Savestate Mode) is swapped in
/// transiently by the server for the boot.
async fn ingame() -> Result<()> {
    let server = TestServer::spawn_ipc_savestate().context(
        "spawn IPC server (in-game tier) — set RPCS3_EXE, RPCS3_CONFIG_DIR, SKYLANDER_BOOT_SAVESTATE",
    )?;
    tracing::info!(url = %server.url, "server up — IPC driver, save-state boot armed");

    let alice = inject_profile(&server.url, "Alice", "1111", "#f5c634").await?;

    // Start whole-desktop capture before the heavy work so the boot is recorded.
    let mp4 = std::env::temp_dir().join("playthrough-ingame.mp4");
    let cap = DesktopCapture::start(&mp4).context("start desktop capture")?;
    tracing::info!(mp4 = %mp4.display(), "recording the desktop…");
    tokio::time::sleep(Duration::from_secs(1)).await;

    let phone_url = server.phone_url().await?;
    let phone = Phone::new_headed(&phone_url, &server.chromedriver_url, 1180, 40, 470, 940)
        .await
        .context("open headed phone browser")?;

    // Profile picker → unlock Alice → GAME PICKER (no game launched yet — unlike
    // the mock scenarios, we want the phone to fire a real /api/launch).
    phone
        .wait_for(Locator::Css(".profile-picker"), Duration::from_secs(20))
        .await
        .context("profile picker never mounted")?;
    unlock_session(&server.url, &alice).await?;
    phone
        .wait_for(Locator::Css(".game-card"), Duration::from_secs(15))
        .await
        .context("game picker never showed game cards")?;

    // Tap the Spyro card (falls back to the first) — a REAL signed /api/launch,
    // which the server turns into a save-state boot (SKYLANDER_BOOT_SAVESTATE
    // overrides the resolved EBOOT).
    let launched = phone
        .client
        .execute(
            r#"const cards=[...document.querySelectorAll('.game-card')];
               const c=cards.find(el=>/spyro/i.test(el.textContent||''))||cards[0];
               if(c){c.click();return true;} return false;"#,
            vec![],
        )
        .await?
        .as_bool()
        .unwrap_or(false);
    if !launched {
        anyhow::bail!("no game card to launch in the picker");
    }
    tracing::info!("launched a game from the picker → server booting the save state");

    // Save-state resume + the server's is_playable wait can take a while
    // (shader compile is mostly cached for a resumed state, but allow headroom).
    phone
        .wait_for(Locator::Css(".portal-p4"), Duration::from_secs(180))
        .await
        .context("in-game portal never reached (save-state boot)")?;
    tracing::info!("portal reached — RPCS3 resumed the save state at the in-game portal");
    tokio::time::sleep(Duration::from_secs(2)).await; // let the portal settle

    // Place a figure — a REAL IPC LOAD onto the resumed save state (NO mock
    // outcomes). It should appear on the GAME's portal, captured via RPCS3's
    // window.
    phone.tap_pointer(".lid-grabber-p4").await?;
    tokio::time::sleep(Duration::from_millis(300)).await;
    phone.tap_pointer(".lid-grabber-p4").await?;
    phone
        .wait_for(Locator::Css(".fig-card-p4"), Duration::from_secs(8))
        .await
        .context("toy box cards never appeared")?;
    tokio::time::sleep(Duration::from_millis(400)).await;
    place_one(&phone, ".fig-card-p4:not(.scan-new)").await?;
    let _ = phone
        .wait_until(Duration::from_secs(15), || async {
            phone
                .client
                .find(Locator::Css(".p4-slot--loaded"))
                .await
                .is_ok()
        })
        .await;
    tracing::info!("figure placed — real IPC LOAD onto the save state");

    // Hold so the in-game portal (with the placed figure) is on screen, then
    // stop + flush the MP4.
    tokio::time::sleep(Duration::from_secs(4)).await;
    cap.stop().context("stop + flush desktop capture")?;
    tracing::info!(mp4 = %mp4.display(), "MP4 written");
    let png = std::env::temp_dir().join("playthrough-ingame.png");
    phone.screenshot(&png).await?;
    tracing::info!(screenshot = %png.display(), "still captured");

    phone.close().await.ok();
    tracing::info!("done (in-game tier)");
    Ok(())
}

/// Hero interaction: open the toy box and place two figures on the portal,
/// then reload to the lid-closed foreground so the loaded slots are the focus.
/// Mirrors the `screenshot_tour` §05–07 flow. Mock load outcomes are injected
/// so the slots complete (Loading → Loaded) without a real portal.
async fn place_figures(phone: &Phone, server: &TestServer, phone_url: &str) -> Result<()> {
    inject_load_outcomes(&server.url, json!([{"kind": "ok"}, {"kind": "ok"}])).await?;

    // Open the toy box: the lid grabber cycles Closed → Compact → Expanded
    // on PointerEvents (two taps to fully open).
    phone.tap_pointer(".lid-grabber-p4").await?;
    tokio::time::sleep(Duration::from_millis(300)).await;
    phone.tap_pointer(".lid-grabber-p4").await?;
    phone
        .wait_for(Locator::Css(".fig-card-p4"), Duration::from_secs(8))
        .await
        .context("toy box cards never appeared")?;
    tokio::time::sleep(Duration::from_millis(400)).await;

    // Place the first figure, wait for a Loaded slot.
    place_one(phone, ".fig-card-p4:not(.scan-new)").await?;
    let _ = phone
        .wait_until(Duration::from_secs(8), || async {
            phone
                .client
                .find(Locator::Css(".p4-slot--loaded"))
                .await
                .is_ok()
        })
        .await;

    // Place a second (skipping the one now on the portal), wait for two Loaded.
    place_one(phone, ".fig-card-p4:not(.scan-new):not(.on-portal)").await?;
    let _ = phone
        .wait_until(Duration::from_secs(8), || async {
            phone
                .client
                .find_all(Locator::Css(".p4-slot--loaded"))
                .await
                .unwrap_or_default()
                .len()
                >= 2
        })
        .await;

    // Reload to the default lid-Closed foreground (synthetic lid taps don't
    // reliably reach the tap detector); placed figures persist via ghost
    // reclaim, so the loaded slots come back as the focus.
    phone.client.goto(phone_url).await?;
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
    Ok(())
}

/// Tap the first card matching `card_sel`, hit PLACE ON PORTAL, return to box.
/// All clicks go through `js_click` (bypasses WebDriver interactability so a
/// card caught mid-animation or behind a closing overlay still fires), and we
/// wait for the detail overlay to fully dismiss before returning so the next
/// pick lands on an interactable grid.
async fn place_one(phone: &Phone, card_sel: &str) -> Result<()> {
    if !phone.js_click(card_sel).await? {
        tracing::warn!(card_sel, "no figure card to place");
        return Ok(());
    }
    phone
        .wait_for(Locator::Css(".detail-btn-primary"), Duration::from_secs(5))
        .await
        .context("figure detail PLACE button")?;
    tokio::time::sleep(Duration::from_millis(300)).await;
    phone.js_click(".detail-btn-primary").await?; // PLACE ON PORTAL
    tokio::time::sleep(Duration::from_millis(400)).await;
    phone.js_click(".detail-btn-secondary").await.ok(); // BACK TO BOX (best-effort)
    let _ = phone
        .wait_until(Duration::from_secs(4), || async {
            phone
                .client
                .find(Locator::Css(".detail-btn-primary"))
                .await
                .is_err()
        })
        .await;
    tokio::time::sleep(Duration::from_millis(300)).await;
    Ok(())
}
