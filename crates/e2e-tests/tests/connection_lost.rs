//! ConnectionLost overlay regression (PLAN 4.18.21).
//!
//! Pins the disconnect → overlay path: server up means no overlay; server
//! killed means the overlay appears past the grace window with the right
//! heading. The reconnect-success → overlay-vanishes path isn't asserted
//! here because the harness can't bring a dead server back to life on the
//! same port; that side is covered by manual on-device smoke tests until
//! we add a "restart server" affordance to the harness.
//!
//! Prerequisites: chromedriver running at http://localhost:4444, phone SPA
//! built (`cd phone && trunk build`). See crates/e2e-tests/README.md.

use std::time::Duration;

use fantoccini::Locator;
use skylander_e2e_tests::{Phone, TestServer};

#[tokio::test(flavor = "current_thread")]
#[ignore = "requires chromedriver + built phone SPA"]
async fn overlay_appears_after_server_dies() {
    let mut server = TestServer::spawn().expect("spawn server");
    let phone = Phone::new(&server.phone_url().await.unwrap(), &server.chromedriver_url)
        .await
        .expect("connect phone");

    // Wait until the SPA reaches a steady-state screen so we know the WS
    // handshake completed. Profile picker is the first surface a fresh
    // session lands on (no profile unlocked, no game launched).
    phone
        .wait_for(
            Locator::Css(".profile-picker, .game-picker"),
            Duration::from_secs(15),
        )
        .await
        .expect("phone reached a steady state");

    // Sanity: while the server is healthy, the overlay must not be shown.
    assert!(
        phone
            .client
            .find(Locator::Css("[data-testid=connection-lost]"))
            .await
            .is_err(),
        "ConnectionLost overlay must not be visible while the server is up"
    );

    // Kill only the server. The chromedriver and the SPA in the headless
    // browser stay alive; the phone's WS sees onclose and starts the
    // reconnect dance against the now-dead address.
    server.kill_server();

    // Grace is 1s in `connection_lost.rs`; allow generous slack for the
    // backoff timer + browser repaint.
    let overlay = phone
        .wait_for(
            Locator::Css("[data-testid=connection-lost]"),
            Duration::from_secs(8),
        )
        .await
        .expect("ConnectionLost overlay should appear after server dies");

    let text = overlay.text().await.unwrap();
    assert!(
        text.contains("LOST CONNECTION"),
        "overlay should carry the LOST CONNECTION heading; got: {text}"
    );
    assert!(
        text.contains("reconnecting") || text.contains("TRY AGAIN"),
        "overlay should show either the auto-reconnect spinner or the manual retry button; got: {text}"
    );

    phone.close().await.unwrap();
}
