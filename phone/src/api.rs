//! REST helpers for talking to the server.

use serde_json::json;
use wasm_bindgen::JsValue;
use wasm_bindgen_futures::JsFuture;
use web_sys::{Request, RequestInit, Response};

use crate::model::{GameLaunched, InstalledGame, PublicFigure, PublicProfile, UnlockedProfile};

fn origin() -> String {
    let loc = web_sys::window().unwrap().location();
    let origin = loc.origin().unwrap_or_else(|_| "".into());
    origin
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
