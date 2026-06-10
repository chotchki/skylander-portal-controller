//! PLAN 15.10/15.11 — desktop-mode single-scene play-through recorder.
//!
//! Boots the launcher in Phase-20 **Desktop mode** (windowed) with the mock
//! driver (no RPCS3), records the **whole primary monitor to an MP4** via
//! `windows-capture` (no external ffmpeg), and — while recording — drives the
//! phone SPA in a VISIBLE Chrome window beside the launcher through a
//! profile → game → portal flow. Outputs an MP4 + a still PNG.
//!
//! Run (build the phone with the harness's pinned token first, else the
//! stale-version overlay blocks every click; and point CHROMEDRIVER at a build
//! matching your installed Chrome):
//!   cd phone && BUILD_TOKEN=e2e-test trunk build
//!   CHROMEDRIVER=<matching chromedriver.exe> cargo run -p skylander-playthrough

mod capture;

use std::time::Duration;

use anyhow::{Context, Result};
use capture::DesktopCapture;
use fantoccini::Locator;
use skylander_e2e_tests::{Phone, TestServer, inject_profile, launch_giants, unlock_session};

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::INFO)
        .init();

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

    // 5. Drive: profile picker → unlock Alice (PIN bypass) → portal.
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

    // 6. Hold so the portal is on screen for a few seconds of footage.
    tokio::time::sleep(Duration::from_secs(6)).await;

    // 7. Stop + flush the MP4, drop a still, tear down.
    cap.stop().context("stop + flush desktop capture")?;
    tracing::info!(mp4 = %mp4.display(), "MP4 written");
    let png = std::env::temp_dir().join("playthrough-desktop.png");
    phone.screenshot(&png).await?;
    tracing::info!(screenshot = %png.display(), "still captured");

    phone.close().await.ok();
    tracing::info!("done");
    Ok(())
}
