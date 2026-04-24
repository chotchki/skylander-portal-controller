//! REST helpers for talking to the server.

use std::cell::{Cell, RefCell};

/// Short git hash (+ `-dirty`) baked in by `build.rs`. Sent to the
/// server's `/api/version` endpoint at boot + after every WS reconnect
/// for a stale-bundle handshake: mismatch → `StaleVersion` overlay
/// tells the user to refresh.
pub const BUILD_TOKEN: &str = env!("BUILD_TOKEN");

/// Outcome of the `GET /api/version` handshake. Drives both the
/// PairingRequired overlay (auth failure) and the StaleVersion overlay
/// (token mismatch).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum VersionCheck {
    /// Haven't called yet, or in-flight — show nothing.
    Pending,
    /// Server's token matches ours. All good.
    Matches,
    /// Server accepted our signature but its token differs — our
    /// bundle is stale (or newer). String is the server's token so
    /// the overlay can surface both sides for diagnosis.
    Stale { server_token: String },
    /// Server rejected our key (401) or missing pairing entirely.
    /// Folds into the existing PairingRequired flow.
    Unauthorized,
    /// Couldn't reach the server at all. Shown as a soft hint, not a
    /// blocking overlay — the existing ConnectionLost surface owns
    /// real-time WS health.
    Unreachable,
}

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
    /// Last-seen `boot_id` from `Event::Welcome`. Persists across WS
    /// reconnects (unlike `SESSION_ID`, which is replaced each connect).
    /// A new Welcome whose `boot_id` differs from this means the server
    /// restarted, which `ws.rs` handles by reloading the page so the
    /// phone's UI state can't drift from the server's empty state.
    static LAST_BOOT_ID: Cell<Option<u64>> = const { Cell::new(None) };
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

/// Compare `incoming` against the previously stored boot id (and store it
/// if absent). Returns `true` when the value changed from a known prior —
/// the caller should reload the page so any cached UI state is discarded.
/// First call after page load always returns `false`.
pub fn observe_boot_id(incoming: u64) -> bool {
    LAST_BOOT_ID.with(|c| match c.get() {
        Some(prev) if prev != incoming => true,
        Some(_) => false,
        None => {
            c.set(Some(incoming));
            false
        }
    })
}

/// localStorage key under which the HMAC shared-secret is cached so
/// PWA-home-screen launches (and plain page reloads) don't need to
/// re-scan the QR. Overwritten every time `?k=...` is present in the
/// URL, so regenerating the server key + scanning the fresh QR is the
/// re-pair path.
const HMAC_STORAGE_KEY: &str = "hmac_key";

/// Parse the URL for `?k=<hex>` (query string) or `#k=<hex>` (fragment
/// — legacy, for home-screen shortcuts pinned before 2026-04-24) and
/// install the key for HMAC signing. Called once at app boot.
///
/// Lookup order:
///   1. `?k=<hex>` in the URL query — fresh pair from a QR scan. iOS
///      preserves query params through "Add to Home Screen" snapshots;
///      fragments get stripped. Wins over any cached key.
///   2. `#k=<hex>` in the URL fragment — legacy path for users whose
///      home-screen shortcut was pinned with a fragment-style URL.
///   3. `localStorage["hmac_key"]` — fallback for subsequent reloads
///      after a successful pair.
///
/// On a successful URL-path read we ALSO write to localStorage so any
/// reload / tab-restore / PWA launch without the key in the URL still
/// finds it via the fallback.
pub fn install_key_from_hash() {
    let loc = match web_sys::window().map(|w| w.location()) {
        Some(l) => l,
        None => return,
    };

    // 1) Query string path — fresh pair from a QR scan. iOS PWA pinning
    //    preserves query params (unlike fragments).
    let search = loc.search().unwrap_or_default();
    if let Some(hex) = parse_key_query(&search) {
        if try_install(&hex) {
            crate::dev_log!(
                "hmac: installed key from query ?k= ({} chars, prefix {})",
                hex.len(),
                &hex[..hex.len().min(8)]
            );
            return;
        } else {
            crate::dev_warn!("hmac: ?k= present but try_install rejected ({} chars)", hex.len());
        }
    }

    // 2) Fragment fallback — legacy home-screen shortcuts pinned before
    //    the server moved to `?k=`.
    let hash = loc.hash().unwrap_or_default();
    if let Some(hex) = parse_key_fragment(&hash) {
        if try_install(hex) {
            crate::dev_log!(
                "hmac: installed key from fragment #k= ({} chars, prefix {})",
                hex.len(),
                &hex[..hex.len().min(8)]
            );
            return;
        }
    }

    // 3) localStorage fallback — cached from a previous successful pair.
    if let Some(storage) = web_sys::window()
        .and_then(|w| w.local_storage().ok().flatten())
    {
        if let Ok(Some(hex)) = storage.get_item(HMAC_STORAGE_KEY) {
            if let Ok(bytes) = hex::decode(&hex) {
                if bytes.len() == 32 {
                    HMAC_KEY.with(|c| *c.borrow_mut() = Some(bytes));
                    crate::dev_log!(
                        "hmac: installed key from localStorage (prefix {})",
                        &hex[..hex.len().min(8)]
                    );
                    return;
                }
            }
            crate::dev_warn!("hmac: localStorage has malformed key ({} chars)", hex.len());
        }
    }

    crate::dev_warn!("hmac: NO KEY installed — query/fragment/localStorage all empty; signed requests will 401");
}

/// Decode + install `hex` if valid; also persist to localStorage for
/// the next reload. Returns true when the install stuck.
fn try_install(hex: &str) -> bool {
    if let Ok(bytes) = hex::decode(hex) {
        if bytes.len() == 32 {
            HMAC_KEY.with(|c| *c.borrow_mut() = Some(bytes));
            if let Some(storage) = web_sys::window()
                .and_then(|w| w.local_storage().ok().flatten())
            {
                let _ = storage.set_item(HMAC_STORAGE_KEY, hex);
            }
            return true;
        }
    }
    false
}

/// Parse `window.location.search` looking for `k=<hex>`. Returns the
/// hex payload on match (the caller decodes + length-checks).
fn parse_key_query(search: &str) -> Option<String> {
    let search = search.strip_prefix('?').unwrap_or(search);
    if search.is_empty() {
        return None;
    }
    for pair in search.split('&') {
        if let Some(rest) = pair.strip_prefix("k=") {
            return Some(rest.to_string());
        }
    }
    None
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

/// True iff a shared HMAC secret was loaded — either from a URL
/// fragment scan or the localStorage fallback. Used by the UI to
/// show a visible "scan the TV's QR to pair" banner instead of
/// letting the user poke around and hit silent 401s.
pub fn has_hmac_key() -> bool {
    HMAC_KEY.with(|c| c.borrow().is_some())
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
    if let Some(rest) = url
        .strip_prefix("http://")
        .or_else(|| url.strip_prefix("https://"))
    {
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

/// Reset the figure currently loaded in `slot` to pack-fresh bytes. PLAN
/// 3.11.3. Caller confirms with the user before hitting this — the endpoint
/// is destructive and intentionally has no built-in undo.
pub async fn post_reset(slot: u8, figure_id: &str) -> Result<(), String> {
    let url = format!("{}/api/portal/slot/{slot}/reset", origin());
    let body = json!({ "figure_id": figure_id }).to_string();
    match do_fetch(&url, "POST", Some(&body)).await {
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

#[derive(serde::Deserialize)]
struct VersionBody {
    build_token: String,
}

/// Version handshake — signed GET that detects both a wrong HMAC key
/// (401 → Unauthorized) and a stale phone bundle (server token differs
/// from [`BUILD_TOKEN`] → Stale). Called at app mount + after every
/// successful WS reconnect.
pub async fn fetch_version_check() -> VersionCheck {
    if !has_hmac_key() {
        // No local key — the PairingRequired overlay already handles
        // this case. Return Unauthorized so callers can collapse the
        // reason into one signal.
        return VersionCheck::Unauthorized;
    }
    let url = format!("{}/api/version", origin());
    match do_fetch(&url, "GET", None).await {
        Ok(text) => match serde_json::from_str::<VersionBody>(&text) {
            Ok(body) if body.build_token == BUILD_TOKEN => VersionCheck::Matches,
            Ok(body) => VersionCheck::Stale {
                server_token: body.build_token,
            },
            Err(_) => VersionCheck::Unreachable,
        },
        Err(e) if e.contains("401") => VersionCheck::Unauthorized,
        Err(_) => VersionCheck::Unreachable,
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

/// Like `post_quit(false)` but signals switch-game intent (PLAN 4.15.9).
/// The server sets `launcher_status.switching = true` before stopping
/// emulation; the TV launcher pins iris-closed + "SWITCHING GAMES"
/// heading until the next `/api/launch` clears the flag. Used by the
/// phone menu's HOLD TO SWITCH GAMES action so the TV doesn't flash
/// back to the join screen between the old game stopping and the new
/// one booting.
pub async fn post_quit_for_switch() -> Result<(), String> {
    let url = format!("{}/api/quit?switch=true", origin());
    do_fetch(&url, "POST", None).await.map(|_| ())
}

/// Phone's SHUT DOWN menu action. Flips the TV launcher into the
/// Farewell screen; the egui side runs its ~3s countdown and then
/// closes the viewport (PLAN 4.15.11). NOT the same as `post_quit` —
/// shutdown closes the LAUNCHER, quit closes the GAME. Callers that
/// want both should `post_quit(true).await` first.
pub async fn post_shutdown() -> Result<(), String> {
    let url = format!("{}/api/shutdown", origin());
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
        req.headers().set("X-Skyportal-Sig", &sig).map_err(js_err)?;
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
