//! HMAC request-signing regression (PLAN 3.13). Verifies the happy path
//! (the harness's default flow through `TestServer::phone_url` → `Phone::new`
//! already exercises signing) plus two negative cases: a tampered signature
//! rejected with 401, and a stale timestamp rejected with 401.
//!
//! Does *not* run over the phone SPA — we hit the REST endpoints directly
//! with crafted headers. That keeps the test focused on the protocol layer.

use std::time::Duration;

use hmac::{Hmac, Mac};
use reqwest::StatusCode;
use serde_json::json;
use sha2::Sha256;

use skylander_e2e_tests::TestServer;

type HmacSha256 = Hmac<Sha256>;

/// Pull the server's HMAC key (hex) via the test hook. Mirrors the
/// in-crate helper but keeps this test self-contained.
async fn fetch_key(base: &str) -> String {
    let resp = reqwest::Client::new()
        .get(format!("{base}/api/_test/hmac_key"))
        .send()
        .await
        .unwrap();
    assert!(resp.status().is_success(), "hmac_key hook returned {}", resp.status());
    resp.json::<serde_json::Value>()
        .await
        .unwrap()
        .get("hmac_key")
        .and_then(|v| v.as_str())
        .unwrap()
        .to_string()
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

#[tokio::test(flavor = "current_thread")]
#[ignore = "requires chromedriver (TestServer spawns it)"]
async fn signed_unlock_succeeds() {
    let server = TestServer::spawn().expect("spawn");
    let key_hex = fetch_key(&server.url).await;
    let key = hex::decode(&key_hex).unwrap();

    // Create a profile first (the inject_profile test hook is itself
    // unsigned — it's feature-gated separately). We need an id to sign a
    // real /api/profiles/:id/unlock call.
    let profile_id =
        skylander_e2e_tests::inject_profile(&server.url, "SigTest", "1234", "#ffffff")
            .await
            .unwrap();

    // Sign a PIN-unlock request for this profile.
    let body = json!({ "pin": "1234" }).to_string();
    let path = format!("/api/profiles/{profile_id}/unlock");
    let ts = now_ms();
    let sig = sign(&key, ts, "POST", &path, body.as_bytes());

    // Unlock needs an X-Session-Id too — spawn a WS connection first to
    // mint one, then close. Simpler: use the test-hook set_session which
    // gives us a session id we can use directly.
    let client = reqwest::Client::new();

    // Pre-register the unlock so a subsequent WS connect would inherit —
    // but actually we want to call the real unlock_profile endpoint, which
    // needs a session id. Open a WS to mint one.
    let ws_url = server.url.replace("http://", "ws://") + "/ws";
    let (mut ws, _) = tokio_tungstenite::connect_async(&ws_url).await.unwrap();
    // Read the first message (Welcome) to get session_id.
    use futures_util::StreamExt;
    let first = tokio::time::timeout(Duration::from_secs(5), ws.next())
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    let msg = match first {
        tokio_tungstenite::tungstenite::Message::Text(t) => t,
        other => panic!("expected text, got {other:?}"),
    };
    let ev: serde_json::Value = serde_json::from_str(&msg).unwrap();
    assert_eq!(ev["kind"], "welcome", "first WS message must be Welcome");
    let sid = ev["session_id"].as_u64().unwrap();

    let resp = client
        .post(format!("{}{path}", server.url))
        .header("Content-Type", "application/json")
        .header("X-Session-Id", sid.to_string())
        .header("X-Skyportal-Timestamp", ts.to_string())
        .header("X-Skyportal-Sig", &sig)
        .body(body)
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK, "signed unlock should succeed");
}

#[tokio::test(flavor = "current_thread")]
#[ignore = "requires chromedriver (TestServer spawns it)"]
async fn tampered_signature_rejected() {
    let server = TestServer::spawn().expect("spawn");
    let _key_hex = fetch_key(&server.url).await;
    let profile_id =
        skylander_e2e_tests::inject_profile(&server.url, "SigTest", "1234", "#ffffff")
            .await
            .unwrap();

    let body = json!({ "pin": "1234" }).to_string();
    let path = format!("/api/profiles/{profile_id}/unlock");
    let ts = now_ms();
    // Not signing with the real key — pretend we're an attacker with the
    // wrong key.
    let wrong_key = [0u8; 32];
    let sig = sign(&wrong_key, ts, "POST", &path, body.as_bytes());

    let resp = reqwest::Client::new()
        .post(format!("{}{path}", server.url))
        .header("Content-Type", "application/json")
        .header("X-Session-Id", "1")
        .header("X-Skyportal-Timestamp", ts.to_string())
        .header("X-Skyportal-Sig", &sig)
        .body(body)
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    let body_text = resp.text().await.unwrap();
    assert!(
        body_text.contains("bad signature"),
        "expected 'bad signature', got {body_text:?}"
    );
}

#[tokio::test(flavor = "current_thread")]
#[ignore = "requires chromedriver (TestServer spawns it)"]
async fn stale_timestamp_rejected() {
    let server = TestServer::spawn().expect("spawn");
    let key_hex = fetch_key(&server.url).await;
    let key = hex::decode(&key_hex).unwrap();
    let profile_id =
        skylander_e2e_tests::inject_profile(&server.url, "SigTest", "1234", "#ffffff")
            .await
            .unwrap();

    let body = json!({ "pin": "1234" }).to_string();
    let path = format!("/api/profiles/{profile_id}/unlock");
    // 61 seconds in the past — outside the ±30s skew window.
    let ts = now_ms().saturating_sub(61_000);
    let sig = sign(&key, ts, "POST", &path, body.as_bytes());

    let resp = reqwest::Client::new()
        .post(format!("{}{path}", server.url))
        .header("Content-Type", "application/json")
        .header("X-Session-Id", "1")
        .header("X-Skyportal-Timestamp", ts.to_string())
        .header("X-Skyportal-Sig", &sig)
        .body(body)
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    let body_text = resp.text().await.unwrap();
    assert!(
        body_text.contains("skew"),
        "expected timestamp-skew rejection, got {body_text:?}"
    );
}
