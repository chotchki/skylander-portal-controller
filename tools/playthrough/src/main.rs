//! PLAN 15.11 — desktop-mode single-scene play-through (MVP).
//!
//! Boots the launcher in Phase-20 **Desktop mode** (windowed) with the mock
//! driver (no RPCS3) and drives the phone SPA in a VISIBLE Chrome window beside
//! it via the `skylander-e2e-tests` harness, drops a screenshot, and holds the
//! scene briefly so the desktop (launcher + phone) is observable. This MVP
//! proves the harness reuse + headed drive; the `windows-capture` MP4 backend
//! (PLAN 15.10) — capturing the whole desktop to video — layers on next.
//!
//! Run (build the phone with the harness's pinned token first, else the
//! stale-version overlay blocks every click):
//!   cd phone && BUILD_TOKEN=e2e-test trunk build
//!   cargo run -p skylander-playthrough

use std::time::Duration;

use anyhow::{Context, Result};
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

    // 3. Visible Chrome parked to the right of the windowed launcher (which
    //    opens ~top-left at 1100x760).
    let phone_url = server.phone_url().await?;
    let phone = Phone::new_headed(&phone_url, &server.chromedriver_url, 1180, 40, 470, 940)
        .await
        .context("open headed phone browser")?;
    tracing::info!("phone browser open (headed) — driving the flow");

    // 4. Drive: profile picker → unlock Alice (PIN bypass) → portal.
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
        tracing::warn!("portal view not reached in time — capturing whatever rendered");
    }
    tokio::time::sleep(Duration::from_secs(1)).await;

    // 5. Artifact + hold so the desktop scene is observable.
    let out = std::env::temp_dir().join("playthrough-desktop.png");
    phone.screenshot(&out).await?;
    tracing::info!(screenshot = %out.display(), "captured phone screenshot");
    tracing::info!("holding 8s — look at the desktop (launcher + phone side-by-side)…");
    tokio::time::sleep(Duration::from_secs(8)).await;

    phone.close().await.ok();
    tracing::info!("done");
    Ok(())
}
