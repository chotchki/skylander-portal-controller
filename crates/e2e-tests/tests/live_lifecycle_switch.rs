//! PLAN 4.15.16 validation — RPCS3 lifecycle under the always-running
//! contract.
//!
//! Exercises the full new lifecycle via signed REST against a live
//! server + real UIA driver + real RPCS3:
//!
//! 1. Server startup spawned RPCS3 at library view — verify
//!    `/api/status` reports `rpcs3_running: true, current_game: None`.
//! 2. POST `/api/launch` with game A → game boots, current = A.
//! 3. POST `/api/quit` → game stops, current = None, rpcs3_running
//!    STAYS true (this is the key 4.15.16 assertion — the process is
//!    not killed).
//! 4. POST `/api/launch` with game A again → same-game relaunch works,
//!    proving the always-running path handles re-boot without a spawn.
//! 5. POST `/api/quit`.
//! 6. POST `/api/launch` with game B → different-game switch works.
//!
//! Runs HTPC-only — needs RPCS3_EXE + two installed game serials.
//! Env:
//!   RPCS3_EXE=C:\emuluators\rpcs3\rpcs3.exe
//!   RPCS3_TEST_SERIAL=BLUS31076      # game A
//!   RPCS3_TEST_SERIAL_2=BLUS30968    # game B (different from A)
//!
//! Run:
//!   cargo test -p skylander-e2e-tests --test live_lifecycle_switch
//!       -- --ignored --nocapture
//!
//! Does NOT go through the phone UI; focused on backend lifecycle
//! correctness, not UX flow. The phone-driven game-picker → portal
//! happy path lives in `live_integration.rs` (3.7.7).

use hmac::{Hmac, Mac};
use reqwest::{Client, StatusCode};
use serde_json::json;
use sha2::Sha256;

use skylander_e2e_tests::TestServer;

type HmacSha256 = Hmac<Sha256>;

// ======================================================================
// Env resolution
// ======================================================================

fn require_env() -> Option<(String, String)> {
    let a = std::env::var("RPCS3_TEST_SERIAL").ok()?;
    let b = std::env::var("RPCS3_TEST_SERIAL_2").ok()?;
    if a == b {
        panic!("RPCS3_TEST_SERIAL and RPCS3_TEST_SERIAL_2 must differ; both are {a}");
    }
    Some((a, b))
}

// ======================================================================
// Signed REST plumbing (mirror of hmac.rs's sign + fetch_key)
// ======================================================================

async fn fetch_key(base: &str) -> Vec<u8> {
    let resp = Client::new()
        .get(format!("{base}/api/_test/hmac_key"))
        .send()
        .await
        .expect("GET hmac_key");
    assert!(
        resp.status().is_success(),
        "hmac_key hook returned {}",
        resp.status()
    );
    let hex_key: String = resp
        .json::<serde_json::Value>()
        .await
        .expect("parse hmac_key")
        .get("hmac_key")
        .and_then(|v| v.as_str())
        .expect("hmac_key field")
        .to_string();
    hex::decode(hex_key).expect("decode hmac_key hex")
}

fn sign(key: &[u8], ts_ms: u64, method: &str, path: &str, body: &[u8]) -> String {
    let mut mac = HmacSha256::new_from_slice(key).unwrap();
    mac.update(ts_ms.to_string().as_bytes());
    mac.update(b".");
    mac.update(method.as_bytes());
    mac.update(b".");
    mac.update(path.as_bytes());
    mac.update(b".");
    mac.update(body.len().to_string().as_bytes());
    mac.update(b".");
    mac.update(body);
    hex::encode(mac.finalize().into_bytes())
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as u64
}

async fn signed_post(
    client: &Client,
    base: &str,
    key: &[u8],
    path: &str,
    body: serde_json::Value,
) -> reqwest::Response {
    let body_bytes = body.to_string();
    let ts = now_ms();
    let sig = sign(key, ts, "POST", path, body_bytes.as_bytes());
    client
        .post(format!("{base}{path}"))
        .header("Content-Type", "application/json")
        .header("X-Skyportal-Timestamp", ts.to_string())
        .header("X-Skyportal-Sig", sig)
        .body(body_bytes)
        .send()
        .await
        .expect("send signed POST")
}

// ======================================================================
// Status probe
// ======================================================================

#[derive(serde::Deserialize, Debug, Clone)]
struct Status {
    rpcs3_running: bool,
    current_game: Option<CurrentGame>,
}

#[derive(serde::Deserialize, Debug, Clone)]
struct CurrentGame {
    serial: String,
    #[serde(rename = "display_name")]
    _display_name: String,
}

async fn fetch_status(client: &Client, base: &str) -> Status {
    client
        .get(format!("{base}/api/status"))
        .send()
        .await
        .expect("GET /api/status")
        .json()
        .await
        .expect("parse /api/status")
}

// ======================================================================
// The test
// ======================================================================

#[tokio::test(flavor = "current_thread")]
#[ignore = "HTPC-only: needs RPCS3_EXE + 2 distinct RPCS3_TEST_SERIAL* envs"]
async fn launch_stop_relaunch_switch() {
    let (serial_a, serial_b) = match require_env() {
        Some(pair) => pair,
        None => {
            eprintln!(
                "skipping — need RPCS3_TEST_SERIAL and RPCS3_TEST_SERIAL_2 (both set, distinct)"
            );
            return;
        }
    };

    let server = TestServer::spawn_live().expect("spawn live server");
    let key = fetch_key(&server.url).await;
    let client = Client::new();

    // === 1. Startup contract: RPCS3 running, no game current ===
    let s0 = fetch_status(&client, &server.url).await;
    assert!(
        s0.rpcs3_running,
        "rpcs3_running should be true right after TestServer::spawn_live under 4.15.16"
    );
    assert!(
        s0.current_game.is_none(),
        "current_game should be None at library view; got {:?}",
        s0.current_game
    );
    eprintln!("✅ step 1: server spawned RPCS3 at library view");

    // === 2. Launch A ===
    let resp = signed_post(
        &client,
        &server.url,
        &key,
        "/api/launch",
        json!({ "serial": serial_a }),
    )
    .await;
    assert_eq!(
        resp.status(),
        StatusCode::ACCEPTED,
        "launch A returned {}: {}",
        resp.status(),
        resp.text().await.unwrap_or_default()
    );
    let s = fetch_status(&client, &server.url).await;
    assert!(s.rpcs3_running);
    assert_eq!(
        s.current_game.as_ref().map(|g| g.serial.as_str()),
        Some(serial_a.as_str()),
        "after launch A, current_game.serial should be {serial_a}"
    );
    eprintln!("✅ step 2: launch A booted ({serial_a})");

    // === 3. Quit → game stops, RPCS3 stays alive ===
    let resp = signed_post(&client, &server.url, &key, "/api/quit", json!({})).await;
    assert_eq!(
        resp.status(),
        StatusCode::ACCEPTED,
        "quit returned {}: {}",
        resp.status(),
        resp.text().await.unwrap_or_default()
    );
    let s = fetch_status(&client, &server.url).await;
    assert!(
        s.rpcs3_running,
        "rpcs3_running MUST stay true after /api/quit (4.15.16 core contract)"
    );
    assert!(
        s.current_game.is_none(),
        "current_game should clear after quit; got {:?}",
        s.current_game
    );
    eprintln!("✅ step 3: quit stopped game; RPCS3 process alive at library");

    // === 4. Relaunch same game A ===
    let resp = signed_post(
        &client,
        &server.url,
        &key,
        "/api/launch",
        json!({ "serial": serial_a }),
    )
    .await;
    assert_eq!(
        resp.status(),
        StatusCode::ACCEPTED,
        "relaunch A returned {}: {}",
        resp.status(),
        resp.text().await.unwrap_or_default()
    );
    let s = fetch_status(&client, &server.url).await;
    assert_eq!(
        s.current_game.as_ref().map(|g| g.serial.as_str()),
        Some(serial_a.as_str()),
        "after relaunch A, current_game.serial should be {serial_a} again"
    );
    eprintln!("✅ step 4: same-game relaunch works");

    // === 5. Quit again ===
    let resp = signed_post(&client, &server.url, &key, "/api/quit", json!({})).await;
    assert_eq!(resp.status(), StatusCode::ACCEPTED);
    let s = fetch_status(&client, &server.url).await;
    assert!(s.rpcs3_running);
    assert!(s.current_game.is_none());
    eprintln!("✅ step 5: second quit ok");

    // === 6. Switch to game B ===
    let resp = signed_post(
        &client,
        &server.url,
        &key,
        "/api/launch",
        json!({ "serial": serial_b }),
    )
    .await;
    assert_eq!(
        resp.status(),
        StatusCode::ACCEPTED,
        "launch B returned {}: {}",
        resp.status(),
        resp.text().await.unwrap_or_default()
    );
    let s = fetch_status(&client, &server.url).await;
    assert_eq!(
        s.current_game.as_ref().map(|g| g.serial.as_str()),
        Some(serial_b.as_str()),
        "after switch to B, current_game.serial should be {serial_b}"
    );
    eprintln!("✅ step 6: switched to different game ({serial_b})");

    // === 7. Final quit (cleanup) ===
    let _ = signed_post(&client, &server.url, &key, "/api/quit", json!({})).await;
    eprintln!("✅ teardown: final quit");
}
