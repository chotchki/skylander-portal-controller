//! Phone bundle embedded into the release binary (PLAN 7.1).
//!
//! `rust-embed` glues the contents of `phone/dist/` into the compiled
//! exe so the released app is a single-file drop without an external
//! `phone/dist` directory. Debug builds (no `debug-embed` feature)
//! read from disk on every request, so `trunk serve --watch` still
//! iterates live; release builds bake the bytes in.
//!
//! The fallback static-file service in `http::router` calls into here
//! to serve any path that didn't match an `/api/*` route. Manifest +
//! icon handlers also read through this module so they get the same
//! debug-vs-release behavior automatically.

use axum::body::Body;
use axum::http::{HeaderValue, StatusCode, header};
use axum::response::{IntoResponse, Response};
use rust_embed::RustEmbed;

/// Embedded snapshot of `phone/dist/` at compile time. The path is
/// relative to *this crate's* manifest (`crates/server/Cargo.toml`),
/// not the workspace root, so it walks up two levels to the `phone`
/// crate.
///
/// `RustEmbed` re-runs each build in release mode (the macro emits
/// the file list at compile time); the workspace's `phone` build
/// pipeline (`trunk build --release` per the README) needs to run
/// before `cargo build --release` for the embedded files to reflect
/// any phone-side changes. CI wires that order in `.github/workflows/`.
#[derive(RustEmbed)]
#[folder = "../../phone/dist"]
pub struct PhoneBundle;

/// Look up an embedded path. Returns the bytes + a guessed
/// content-type. `None` for paths not in the bundle. The lookup is
/// case-sensitive — the bundle ships lowercase paths.
pub fn lookup(path: &str) -> Option<(Vec<u8>, &'static str)> {
    let asset = PhoneBundle::get(path)?;
    let content_type = guess_content_type(path);
    Some((asset.data.into_owned(), content_type))
}

/// Map a filename extension to a Content-Type the browser will
/// accept. `text/plain` is the safe fallback for anything we don't
/// recognise; the iOS PWA flow specifically needs the WASM, JS, and
/// `webmanifest` types right or the bundle won't load.
pub fn guess_content_type(path: &str) -> &'static str {
    let lower = path.to_ascii_lowercase();
    match () {
        _ if lower.ends_with(".html") => "text/html; charset=utf-8",
        _ if lower.ends_with(".js") => "application/javascript",
        _ if lower.ends_with(".wasm") => "application/wasm",
        _ if lower.ends_with(".css") => "text/css; charset=utf-8",
        _ if lower.ends_with(".webmanifest") => "application/manifest+json",
        _ if lower.ends_with(".json") => "application/json",
        _ if lower.ends_with(".svg") => "image/svg+xml",
        _ if lower.ends_with(".png") => "image/png",
        _ if lower.ends_with(".jpg") || lower.ends_with(".jpeg") => "image/jpeg",
        _ if lower.ends_with(".woff2") => "font/woff2",
        _ if lower.ends_with(".woff") => "font/woff",
        _ if lower.ends_with(".ico") => "image/x-icon",
        _ if lower.ends_with(".txt") => "text/plain; charset=utf-8",
        _ => "application/octet-stream",
    }
}

/// Axum response: serve `path` from the embedded bundle, with the
/// matching Content-Type header. Returns a 404 if the path isn't in
/// the bundle. Used by the SPA fallback service AND by the per-asset
/// handlers (icons, manifest) to keep their debug/release behavior
/// uniform.
pub fn serve(path: &str) -> Response {
    match lookup(path) {
        Some((bytes, ct)) => {
            let mut resp: Response = (StatusCode::OK, bytes).into_response();
            if let Ok(value) = HeaderValue::from_str(ct) {
                resp.headers_mut().insert(header::CONTENT_TYPE, value);
            }
            resp
        }
        None => (StatusCode::NOT_FOUND, "asset not found").into_response(),
    }
}

/// SPA fallback handler. Maps the request URI's path to an embedded
/// asset; on 404, falls back to `index.html` so the SPA's client-side
/// router can take over (the WASM bundle handles in-app navigation
/// for any unknown route). Trims the leading `/` since RustEmbed's
/// keys are bare relative paths.
pub async fn fallback_handler(req: axum::extract::Request) -> Response {
    let raw = req.uri().path();
    let trimmed = raw.trim_start_matches('/');
    let key = if trimmed.is_empty() {
        "index.html"
    } else {
        trimmed
    };
    if let Some((bytes, ct)) = lookup(key) {
        let mut resp: Response = (StatusCode::OK, bytes).into_response();
        if let Ok(value) = HeaderValue::from_str(ct) {
            resp.headers_mut().insert(header::CONTENT_TYPE, value);
        }
        return resp;
    }
    // Unknown path → SPA shell so client-side routing can render.
    if let Some((bytes, _)) = lookup("index.html") {
        let mut resp: Response = (
            StatusCode::OK,
            [(header::CONTENT_TYPE, "text/html; charset=utf-8")],
            bytes,
        )
            .into_response();
        // Match the prior ServeDir's no-store behavior so iOS PWA
        // installs see new bundles after a cold launch.
        resp.headers_mut()
            .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
        return resp;
    }
    Response::builder()
        .status(StatusCode::NOT_FOUND)
        .body(Body::from("phone bundle not embedded"))
        .unwrap()
}
