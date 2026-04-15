//! REST helpers for talking to the server.

use std::cell::{Cell, RefCell};

use serde_json::json;
use wasm_bindgen::JsValue;
use wasm_bindgen_futures::JsFuture;
use web_sys::{Request, RequestInit, Response};

use crate::model::{GameLaunched, InstalledGame, PublicFigure, PublicProfile, UnlockedProfile};

// Session-id storage shared between `ws.rs` (writes it on `Event::Welcome`)
// and this module (reads it for the `X-Session-Id` header on every mutating
// call). Thread-local Cell is enough since WASM is single-threaded — no lock
// contention, no cost at read-time.
thread_local! {
    static SESSION_ID: Cell<Option<u64>> = const { Cell::new(None) };
    /// HMAC-SHA256 key shared with the server via the TV's QR fragment
    /// (`#k=<hex>`). Read once from `window.location.hash` on boot. None if
    /// the phone was loaded via a bare URL (e.g. e2e tests, typed-URL access);
    /// the server's dev build accepts unsigned requests in that case.
    static HMAC_KEY: RefCell<Option<Vec<u8>>> = const { RefCell::new(None) };
}

/// Called by `ws.rs` when the server sends `Event::Welcome`. Replaces any
/// previously stored id — a WS reconnect yields a new session.
pub fn set_session_id(id: u64) {
    SESSION_ID.with(|c| c.set(Some(id)));
}

/// Read the current session id, if the WS has connected and sent Welcome.
pub fn current_session_id() -> Option<u64> {
    SESSION_ID.with(|c| c.get())
}

/// Parse `window.location.hash` looking for `#k=<hex>` and install the key
/// for HMAC signing. Called once at app boot. Any existing hash is preserved
/// in the URL; we strip the key fragment after reading so browser history
/// screenshots / URL bar don't keep the shared secret around.
pub fn install_key_from_hash() {
    let loc = match web_sys::window().map(|w| w.location()) {
        Some(l) => l,
        None => return,
    };
    let hash = loc.hash().unwrap_or_default();
    let Some(hex) = parse_key_fragment(&hash) else {
        return;
    };
    if let Ok(bytes) = hex::decode(hex) {
        if bytes.len() == 32 {
            HMAC_KEY.with(|c| *c.borrow_mut() = Some(bytes));
            // Wipe `#k=...` from the address bar without triggering a
            // navigation. `history.replaceState` keeps the rest of the URL.
            if let Some(hist) = web_sys::window().and_then(|w| w.history().ok()) {
                let _ = hist.replace_state_with_url(
                    &JsValue::NULL,
                    "",
                    Some(loc.pathname().unwrap_or_default().as_str()),
                );
            }
        }
    }
}

fn parse_key_fragment(hash: &str) -> Option<&str> {
    // hash looks like `#k=abcd...` or `#foo=bar&k=abcd...`. Strip leading `#`.
    let hash = hash.strip_prefix('#')?;
    for part in hash.split('&') {
        if let Some(rest) = part.strip_prefix("k=") {
            return Some(rest);
        }
    }
    None
}

fn origin() -> String {
    let loc = web_sys::window().unwrap().location();
    let origin = loc.origin().unwrap_or_else(|_| "".into());
    origin
}

fn sign(method: &str, path: &str, body: &[u8]) -> Option<(String, String)> {
    use hmac::{Hmac, Mac};
    type HmacSha256 = Hmac<sha2::Sha256>;
    let key = HMAC_KEY.with(|c| c.borrow().clone())?;
    let ts_ms = js_sys::Date::now() as u64;
    let mut mac = HmacSha256::new_from_slice(&key).ok()?;
    // Matches server's compute_hmac in http.rs exactly — any drift breaks
    // the tag compare on the other side.
    mac.update(ts_ms.to_string().as_bytes());
    mac.update(b".");
    mac.update(method.as_bytes());
    mac.update(b".");
    mac.update(path.as_bytes());
    mac.update(b".");
    mac.update(body.len().to_string().as_bytes());
    mac.update(b".");
    mac.update(body);
    let tag = mac.finalize().into_bytes();
    Some((ts_ms.to_string(), hex::encode(tag)))
}

/// Extract the path portion of a request URL (`/api/...`) for use in the
/// HMAC input. The server sees only the path on its side — `X-Forwarded-*`
/// etc. aren't in play on a trusted LAN.
fn path_of(url: &str) -> &str {
    // Skip scheme + host.
    if let Some(rest) = url.strip_prefix("http://").or_else(|| url.strip_prefix("https://")) {
        if let Some(slash) = rest.find('/') {
            return &rest[slash..];
        }
    }
    url
}

pub async fn fetch_figures() -> Vec<PublicFigure> {
    let url = format!("{}/api/figures", origin());
    match do_fetch(&url, "GET", None).await {
        Ok(text) => serde_json::from_str(&text).unwrap_or_default(),
        Err(e) => {
            web_sys::console::warn_1(&JsValue::from_str(&format!("fetch_figures: {e}")));
            Vec::new()
        }
    }
}

pub async fn post_load(slot: u8, figure_id: &str) -> Result<(), String> {
    let url = format!("{}/api/portal/slot/{slot}/load", origin());
    let body = json!({ "figure_id": figure_id }).to_string();
    match do_fetch(&url, "POST", Some(&body)).await {
        Ok(_) => Ok(()),
        Err(e) => Err(e),
    }
}

pub async fn post_clear(slot: u8) -> Result<(), String> {
    let url = format!("{}/api/portal/slot/{slot}/clear", origin());
    match do_fetch(&url, "POST", None).await {
        Ok(_) => Ok(()),
        Err(e) => Err(e),
    }
}

pub async fn fetch_games() -> Vec<InstalledGame> {
    let url = format!("{}/api/games", origin());
    match do_fetch(&url, "GET", None).await {
        Ok(text) => serde_json::from_str(&text).unwrap_or_default(),
        Err(_) => Vec::new(),
    }
}

#[derive(serde::Deserialize)]
struct StatusBody {
    current_game: Option<GameLaunched>,
}

pub async fn fetch_status() -> Option<GameLaunched> {
    let url = format!("{}/api/status", origin());
    match do_fetch(&url, "GET", None).await {
        Ok(text) => serde_json::from_str::<StatusBody>(&text)
            .ok()
            .and_then(|s| s.current_game),
        Err(_) => None,
    }
}

pub async fn post_launch(serial: &str) -> Result<(), String> {
    let url = format!("{}/api/launch", origin());
    let body = json!({ "serial": serial }).to_string();
    do_fetch(&url, "POST", Some(&body)).await.map(|_| ())
}

pub async fn fetch_profiles() -> Vec<PublicProfile> {
    let url = format!("{}/api/profiles", origin());
    match do_fetch(&url, "GET", None).await {
        Ok(text) => serde_json::from_str(&text).unwrap_or_default(),
        Err(_) => Vec::new(),
    }
}

pub async fn create_profile(
    display_name: &str,
    pin: &str,
    color: &str,
) -> Result<PublicProfile, String> {
    let url = format!("{}/api/profiles", origin());
    let body = json!({
        "display_name": display_name,
        "pin": pin,
        "color": color,
    })
    .to_string();
    let text = do_fetch(&url, "POST", Some(&body)).await?;
    serde_json::from_str(&text).map_err(|e| e.to_string())
}

pub async fn delete_profile(id: &str, pin: &str) -> Result<(), String> {
    let url = format!("{}/api/profiles/{id}", origin());
    let body = json!({ "pin": pin }).to_string();
    do_fetch(&url, "DELETE", Some(&body)).await.map(|_| ())
}

pub async fn unlock_profile(id: &str, pin: &str) -> Result<UnlockedProfile, String> {
    let url = format!("{}/api/profiles/{id}/unlock", origin());
    let body = json!({ "pin": pin }).to_string();
    let text = do_fetch(&url, "POST", Some(&body)).await?;
    serde_json::from_str(&text).map_err(|e| e.to_string())
}

pub async fn reset_pin(id: &str, current_pin: &str, new_pin: &str) -> Result<(), String> {
    let url = format!("{}/api/profiles/{id}/reset_pin", origin());
    let body = json!({
        "current_pin": current_pin,
        "new_pin": new_pin,
    })
    .to_string();
    do_fetch(&url, "POST", Some(&body)).await.map(|_| ())
}

pub async fn post_quit(force: bool) -> Result<(), String> {
    let url = if force {
        format!("{}/api/quit?force=true", origin())
    } else {
        format!("{}/api/quit", origin())
    };
    do_fetch(&url, "POST", None).await.map(|_| ())
}

async fn do_fetch(url: &str, method: &str, body: Option<&str>) -> Result<String, String> {
    let opts = RequestInit::new();
    opts.set_method(method);
    if let Some(b) = body {
        opts.set_body(&JsValue::from_str(b));
    }
    let req = Request::new_with_str_and_init(url, &opts).map_err(js_err)?;
    if body.is_some() {
        req.headers()
            .set("Content-Type", "application/json")
            .map_err(js_err)?;
    }
    // Attach the caller's session id on every mutating request so the server
    // can route per-session state correctly. Safe to set on GETs too — the
    // server's `CurrentSession` extractor is only required on handlers where
    // session routing matters; elsewhere the header is simply ignored.
    if let Some(sid) = current_session_id() {
        req.headers()
            .set("X-Session-Id", &sid.to_string())
            .map_err(js_err)?;
    }
    // HMAC signature for every request (PLAN 3.13). Server requires it on
    // mutating endpoints in release; dev build accepts unsigned. Attach
    // unconditionally when the key is known — harmless on read endpoints.
    let body_bytes = body.map(|s| s.as_bytes()).unwrap_or(&[]);
    if let Some((ts, sig)) = sign(method, path_of(url), body_bytes) {
        req.headers()
            .set("X-Skyportal-Timestamp", &ts)
            .map_err(js_err)?;
        req.headers()
            .set("X-Skyportal-Sig", &sig)
            .map_err(js_err)?;
    }
    let window = web_sys::window().ok_or_else(|| "no window".to_string())?;
    let resp_val = JsFuture::from(window.fetch_with_request(&req))
        .await
        .map_err(js_err)?;
    let resp: Response = resp_val.dyn_into().map_err(js_err)?;
    if !resp.ok() && resp.status() != 202 {
        return Err(format!("HTTP {}", resp.status()));
    }
    let text = JsFuture::from(resp.text().map_err(js_err)?)
        .await
        .map_err(js_err)?;
    Ok(text.as_string().unwrap_or_default())
}

fn js_err(v: JsValue) -> String {
    v.as_string().unwrap_or_else(|| format!("{:?}", v))
}

// `dyn_into` is from wasm_bindgen's JsCast trait.
use wasm_bindgen::JsCast;
