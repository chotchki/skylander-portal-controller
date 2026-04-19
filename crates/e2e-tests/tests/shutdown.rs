//! Regression for `POST /api/shutdown` (PLAN 4.15.11).
//!
//! Pinned contracts:
//!   - The endpoint exists and accepts unsigned POSTs in dev (the dev
//!     bypass on the `Signed` extractor lets the harness exercise
//!     mutating endpoints without scraping the HMAC key).
//!   - It returns 202 Accepted with a "farewell" body — the contract
//!     the phone's `MenuOverlay` SHUT DOWN action depends on.
//!
//! What we DON'T test here: that the egui launcher actually flips to
//! the Farewell screen + closes the viewport ~3s later. That side is
//! visual + tied to eframe's event loop, hard to observe from a
//! reqwest test, and gets a manual on-device check whenever 4.19's
//! launcher work changes the Farewell render. The HTTP-side contract
//! (this test) is the boundary the phone code sees.

use skylander_e2e_tests::TestServer;

#[tokio::test(flavor = "current_thread")]
#[ignore = "spawns full server (cargo build); ~30s first run"]
async fn shutdown_endpoint_returns_accepted_with_farewell_body() {
    let server = TestServer::spawn().expect("spawn server");

    let response = reqwest::Client::new()
        .post(format!("{}/api/shutdown", server.url))
        .send()
        .await
        .expect("POST /api/shutdown");

    assert_eq!(
        response.status(),
        reqwest::StatusCode::ACCEPTED,
        "/api/shutdown should respond 202 ACCEPTED"
    );

    let body = response.text().await.expect("read body");
    assert_eq!(body, "farewell", "/api/shutdown body contract");

    // Server may close its eframe viewport in ~3s and then exit. We
    // drop TestServer here, which kills the spawned process either way
    // — no point waiting around for the natural exit.
}
