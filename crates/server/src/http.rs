//! HTTP + WebSocket routes.

use std::sync::Arc;
use std::time::Duration;

use axum::Router;
use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::{Path as AxumPath, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use futures_util::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use skylander_core::{
    Event, FigureId, GameLaunched, GameSerial, PublicFigure, SLOT_COUNT, SlotIndex, SlotState,
    UnlockedProfile,
};
use tokio::sync::broadcast;
use tower::ServiceBuilder;
use tower_http::services::ServeDir;
use tower_http::set_header::SetResponseHeaderLayer;
use tracing::{debug, info, warn};

use crate::games::InstalledGame;
use crate::profiles::{LockoutCheck, MAX_PROFILES, PublicProfile, RegistrationOutcome, SessionId};
use crate::state::{AppState, DriverJob};

/// Axum extractor that validates the HMAC signature on a mutating request
/// and hands back the raw body bytes. Each `X-Skyportal-Sig` is
/// `HMAC-SHA256(key, "{ts}.{method}.{path}.{body_bytes}")` hex-encoded, and
/// each `X-Skyportal-Timestamp` is unix-ms — rejected outside a ±30s window
/// to keep replay costs bounded. See PLAN 3.13.
///
/// Dev builds (`dev-tools` feature on) allow *unsigned* requests so the e2e
/// harness keeps working without scraping the key from server logs. When the
/// phone does send a signature in dev, it's still validated. Release builds
/// reject anything unsigned with 401.
///
/// Handlers that used to take `Json<T>` now take `Signed` + deserialize the
/// bytes themselves with `serde_json::from_slice`. Slightly more code at the
/// call site than a fancy `SignedJson<T>` wrapper, but keeps the extractor
/// simple and lets callers surface good error messages on malformed JSON.
pub struct Signed(pub axum::body::Bytes);

const SIG_TIMESTAMP_HEADER: &str = "x-skyportal-timestamp";
const SIG_SIG_HEADER: &str = "x-skyportal-sig";
const SIG_MAX_SKEW: Duration = Duration::from_secs(30);

#[async_trait::async_trait]
impl axum::extract::FromRequest<Arc<AppState>> for Signed {
    type Rejection = Response;

    async fn from_request(
        req: axum::extract::Request,
        state: &Arc<AppState>,
    ) -> Result<Self, Self::Rejection> {
        let method = req.method().clone();
        let path = req.uri().path().to_string();
        let headers = req.headers().clone();
        let body = axum::body::to_bytes(req.into_body(), usize::MAX)
            .await
            .map_err(|e| (StatusCode::BAD_REQUEST, format!("read body: {e}")).into_response())?;

        let sig_hdr = headers.get(SIG_SIG_HEADER);
        let ts_hdr = headers.get(SIG_TIMESTAMP_HEADER);

        // Dev bypass: unsigned requests are allowed when `dev-tools` is on.
        // Present signatures are still validated (catches typos / key drift
        // in the test harness before they become production issues).
        #[cfg(feature = "dev-tools")]
        {
            if sig_hdr.is_none() && ts_hdr.is_none() {
                return Ok(Signed(body));
            }
        }

        let sig = sig_hdr
            .ok_or_else(|| {
                (StatusCode::UNAUTHORIZED, "missing X-Skyportal-Sig header").into_response()
            })?
            .to_str()
            .map_err(|_| (StatusCode::BAD_REQUEST, "X-Skyportal-Sig not ASCII").into_response())?;
        let ts = ts_hdr
            .ok_or_else(|| {
                (
                    StatusCode::UNAUTHORIZED,
                    "missing X-Skyportal-Timestamp header",
                )
                    .into_response()
            })?
            .to_str()
            .map_err(|_| {
                (StatusCode::BAD_REQUEST, "X-Skyportal-Timestamp not ASCII").into_response()
            })?;

        let ts_ms: u64 = ts.parse().map_err(|_| {
            (
                StatusCode::BAD_REQUEST,
                "X-Skyportal-Timestamp not a unix-ms integer",
            )
                .into_response()
        })?;
        let now_ms = now_unix_ms();
        let diff_ms = now_ms.abs_diff(ts_ms);
        if diff_ms > SIG_MAX_SKEW.as_millis() as u64 {
            return Err((
                StatusCode::UNAUTHORIZED,
                format!(
                    "timestamp skew {}ms exceeds {}ms window",
                    diff_ms,
                    SIG_MAX_SKEW.as_millis()
                ),
            )
                .into_response());
        }

        let expected = compute_hmac(&state.hmac_key, ts_ms, method.as_str(), &path, &body);
        let provided = hex::decode(sig)
            .map_err(|_| (StatusCode::BAD_REQUEST, "X-Skyportal-Sig not hex").into_response())?;
        // Constant-time compare — `subtle::ConstantTimeEq` returns a `Choice`.
        use subtle::ConstantTimeEq;
        if expected.ct_eq(&provided).unwrap_u8() != 1 {
            return Err((StatusCode::UNAUTHORIZED, "bad signature").into_response());
        }
        Ok(Signed(body))
    }
}

fn now_unix_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

fn compute_hmac(key: &[u8], ts_ms: u64, method: &str, path: &str, body: &[u8]) -> Vec<u8> {
    use hmac::{Hmac, Mac};
    type HmacSha256 = Hmac<sha2::Sha256>;
    let mut mac =
        HmacSha256::new_from_slice(key).expect("HMAC accepts any key length; 32 bytes fine");
    // Domain-separated framing: ts.method.path.bodyLen.body — the bodyLen
    // prevents a body-suffix-matching path confusion attack.
    mac.update(ts_ms.to_string().as_bytes());
    mac.update(b".");
    mac.update(method.as_bytes());
    mac.update(b".");
    mac.update(path.as_bytes());
    mac.update(b".");
    mac.update(body.len().to_string().as_bytes());
    mac.update(b".");
    mac.update(body);
    mac.finalize().into_bytes().to_vec()
}

/// Axum extractor that pulls the caller's session id from the `X-Session-Id`
/// request header. The phone receives its session id in the initial `Welcome`
/// WS event and attaches it to every REST call so the server can route
/// per-session state correctly. Rejects with 400 if the header is missing
/// or malformed.
pub struct CurrentSession(pub SessionId);

#[async_trait::async_trait]
impl<S> axum::extract::FromRequestParts<S> for CurrentSession
where
    S: Send + Sync,
{
    type Rejection = Response;

    async fn from_request_parts(
        parts: &mut axum::http::request::Parts,
        _state: &S,
    ) -> Result<Self, Self::Rejection> {
        let Some(raw) = parts.headers.get("x-session-id") else {
            return Err((
                StatusCode::BAD_REQUEST,
                "missing X-Session-Id header — connect WS and read Event::Welcome first",
            )
                .into_response());
        };
        let s = raw
            .to_str()
            .map_err(|_| (StatusCode::BAD_REQUEST, "X-Session-Id not ASCII").into_response())?;
        let id = s
            .parse::<u64>()
            .map_err(|_| (StatusCode::BAD_REQUEST, "X-Session-Id not a u64").into_response())?;
        Ok(CurrentSession(SessionId(id)))
    }
}

pub fn router(state: Arc<AppState>, phone_dist: std::path::PathBuf) -> Router {
    #[allow(unused_mut)]
    let mut api = Router::new()
        .route("/api/figures", get(list_figures))
        .route("/api/figures/:id/image", get(figure_image))
        .route("/api/games/:serial/image", get(game_image))
        .route("/api/portal", get(get_portal))
        .route("/api/portal/slot/:n/load", post(load_slot))
        .route("/api/portal/slot/:n/clear", post(clear_slot))
        .route("/api/portal/slot/:n/reset", post(reset_slot))
        .route("/api/portal/refresh", post(refresh_portal))
        .route("/api/games", get(list_games))
        .route("/api/status", get(get_status))
        .route("/api/launch", post(launch_game))
        .route("/api/quit", post(quit_game))
        .route("/api/shutdown", post(shutdown_launcher))
        .route("/api/profiles", get(list_profiles).post(create_profile))
        .route("/api/profiles/:id", axum::routing::delete(delete_profile))
        .route("/api/profiles/:id/unlock", post(unlock_profile))
        .route("/api/profiles/:id/lock", post(lock_profile))
        .route("/api/profiles/:id/reset_pin", post(reset_pin))
        .route("/ws", get(ws_handler));

    #[cfg(feature = "sky-stats")]
    {
        api = api.route(
            "/api/profiles/:profile_id/figures/:figure_id/stats",
            get(crate::sky_stats::get_figure_stats),
        );
    }

    #[cfg(feature = "dev-tools")]
    {
        api = api.route("/api/_dev/log", post(dev_log));
    }

    // PWA icons + manifest. The phone's index.html references stable URLs
    // (`/icons/icon-180.png`, `/manifest.webmanifest`); the server picks
    // which on-disk file to serve based on the `dev-tools` feature so a
    // dev build's home-screen install ends up with a Kaos-tinted icon
    // and "Skylander Portal (DEV)" name. Production swaps in the gold
    // variant + stock manifest. See phone/index.html for the contract.
    api = api
        .route("/icons/:filename", get(serve_icon))
        .route("/manifest.webmanifest", get(serve_manifest));

    #[cfg(feature = "test-hooks")]
    {
        api = api
            .route("/api/_test/inject_load", post(inject_load))
            .route("/api/_test/set_game", post(set_game_state))
            .route("/api/_test/inject_profile", post(inject_profile))
            .route("/api/_test/unlock_session", post(unlock_session_testhook))
            .route("/api/_test/hmac_key", get(hmac_key_testhook))
            .route(
                "/api/_test/clear_eviction_cooldown",
                post(clear_eviction_cooldown_testhook),
            )
            .route(
                "/api/_test/set_session_profile",
                post(set_session_profile_testhook),
            )
            .route("/api/_test/layout/:profile_id", get(layout_testhook));
    }

    // Static phone SPA (dev mode — ServeDir). When the dist directory isn't
    // present yet (before 2.7 builds the SPA), fall back to a placeholder.
    let static_dir = if phone_dist.exists() {
        ServeDir::new(&phone_dist)
    } else {
        warn!(
            "phone dist at {} doesn't exist — SPA will not be served; serving placeholder",
            phone_dist.display()
        );
        ServeDir::new(std::env::current_dir().unwrap_or_default())
    };

    // Cache-busting layer for static assets. iOS Safari (especially in
    // PWA mode after Add-to-Home-Screen) caches the SPA's entry HTML
    // aggressively; without an explicit no-store header, every code
    // change requires deleting + re-adding the home-screen icon to see
    // updates. `no-store` forces the browser to refetch on every load,
    // which lets it discover new hashed wasm/js filenames trunk emits
    // per build. Trade-off: every PWA cold-start re-downloads the
    // bundle (~1MB) — fine on a LAN, would matter over WAN. We may
    // tune to `no-cache` (revalidate, allow cached on 304) once a
    // service worker handles update detection. Layer applies to the
    // fallback only so /api/* responses are unaffected.
    let static_with_cache_headers = ServiceBuilder::new()
        .layer(SetResponseHeaderLayer::overriding(
            axum::http::header::CACHE_CONTROL,
            axum::http::HeaderValue::from_static("no-store"),
        ))
        .service(static_dir);

    api.fallback_service(static_with_cache_headers).with_state(state)
}

async fn list_figures(
    State(state): State<Arc<AppState>>,
    headers: axum::http::HeaderMap,
) -> impl IntoResponse {
    let session_id = headers
        .get("x-session-id")
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.parse::<u64>().ok())
        .map(SessionId);

    let profile_id = match session_id {
        Some(sid) => state.sessions.profile_of(sid).await,
        None => None,
    };

    let usage: std::collections::HashMap<String, String> = match &profile_id {
        Some(pid) => state.profiles.fetch_usage(pid).await.unwrap_or_else(|e| {
            warn!("fetch_usage({pid}): {e}");
            std::collections::HashMap::new()
        }),
        None => std::collections::HashMap::new(),
    };

    let current_game = {
        let rpcs3 = state.rpcs3.lock().await;
        rpcs3
            .current
            .as_ref()
            .and_then(|g| skylander_core::game_of_origin_from_serial(&g.serial))
    };

    let mut figs: Vec<(bool, Option<String>, PublicFigure)> = state
        .figures
        .iter()
        .map(|f| {
            let compat = current_game
                .map(|cg| skylander_core::is_compatible(f.game, f.category, cg))
                .unwrap_or(false);
            let last_used = usage.get(f.id.as_str()).cloned();
            (compat, last_used, f.to_public())
        })
        .collect();

    // WHY: `last_used` is `Option<String>` holding RFC3339 — None sorts
    // last so never-used figures come after used ones. Reverse on the
    // timestamp gives most-recent-first.
    figs.sort_by(|a, b| {
        b.0.cmp(&a.0)
            .then_with(|| match (&a.1, &b.1) {
                (Some(_), None) => std::cmp::Ordering::Less,
                (None, Some(_)) => std::cmp::Ordering::Greater,
                (Some(x), Some(y)) => y.cmp(x),
                (None, None) => std::cmp::Ordering::Equal,
            })
            .then_with(|| a.2.canonical_name.cmp(&b.2.canonical_name))
    });

    let out: Vec<PublicFigure> = figs.into_iter().map(|(_, _, f)| f).collect();
    axum::Json(out)
}

#[derive(Deserialize, Default)]
struct ImageQuery {
    #[serde(default)]
    size: Option<String>,
}

/// GET /api/figures/:id/image?size=thumb|hero
///
/// Serves the scraped wiki hero portrait from
/// `<data_root>/images/<id>/<size>.png`. Falls back to the firmware-pack's
/// element-symbol PNG when the scrape didn't land a match. Returns 404 only
/// if neither exists.
async fn figure_image(
    State(state): State<Arc<AppState>>,
    AxumPath(id): AxumPath<String>,
    axum::extract::Query(q): axum::extract::Query<ImageQuery>,
) -> Response {
    // Input validation: ids are 16 hex chars from the indexer. Be strict to
    // keep this from becoming an arbitrary-file-read vector.
    if id.len() != 16 || !id.chars().all(|c| c.is_ascii_hexdigit()) {
        return (StatusCode::BAD_REQUEST, "bad figure id").into_response();
    }
    let size = match q.size.as_deref().unwrap_or("thumb") {
        "thumb" => "thumb",
        "hero" => "hero",
        other => {
            return (StatusCode::BAD_REQUEST, format!("unknown size: {other}")).into_response();
        }
    };

    let scraped = state
        .data_root
        .join("images")
        .join(&id)
        .join(format!("{size}.png"));

    if let Ok(bytes) = tokio::fs::read(&scraped).await {
        return image_response(bytes);
    }

    // Fallback: element icon from the firmware pack.
    if let Some(fig) = state.lookup_figure(&FigureId::new(&id))
        && let Some(icon_path) = &fig.element_icon_path
        && let Ok(bytes) = tokio::fs::read(icon_path).await
    {
        return image_response(bytes);
    }

    (StatusCode::NOT_FOUND, "no image available").into_response()
}

fn image_response(bytes: Vec<u8>) -> Response {
    use axum::http::header;
    (
        [
            (header::CONTENT_TYPE, "image/png"),
            (header::CACHE_CONTROL, "public, max-age=86400"),
        ],
        bytes,
    )
        .into_response()
}

/// GET /api/games/:serial/image
///
/// Serves the scraped box-art PNG from `<data_root>/games/<serial>.png` for
/// the phone's game-picker cards. Box art is optional polish — a miss returns
/// 404 with no fallback (cards render fine without an image thanks to their
/// per-slug CSS gradient).
async fn game_image(
    State(state): State<Arc<AppState>>,
    AxumPath(serial): AxumPath<String>,
) -> Response {
    serve_game_image(&state.data_root, &serial).await
}

/// Shared core of `game_image`, factored out so tests can exercise the
/// validation + filesystem logic against a temp `data_root` without having
/// to construct a full `AppState`.
async fn serve_game_image(data_root: &std::path::Path, serial: &str) -> Response {
    // Strict format check — path-traversal guard analogous to the figure
    // route's hex validation. All supported PS3 serials match
    // ^(BLUS|BLES|BCUS|BCES)\d{5}$; anything else is rejected without
    // touching the filesystem.
    if !is_valid_ps3_serial(serial) {
        return (StatusCode::BAD_REQUEST, "bad game serial").into_response();
    }

    let path = data_root.join("games").join(format!("{serial}.png"));
    match tokio::fs::read(&path).await {
        Ok(bytes) => image_response(bytes),
        Err(_) => (StatusCode::NOT_FOUND, "").into_response(),
    }
}

fn is_valid_ps3_serial(s: &str) -> bool {
    if s.len() != 9 {
        return false;
    }
    let prefix = &s[..4];
    if !matches!(prefix, "BLUS" | "BLES" | "BCUS" | "BCES") {
        return false;
    }
    s[4..].chars().all(|c| c.is_ascii_digit())
}

async fn get_portal(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    let portal = state.portal.lock().await.clone();
    axum::Json(portal)
}

#[derive(Deserialize)]
struct LoadBody {
    figure_id: FigureId,
}

async fn load_slot(
    State(state): State<Arc<AppState>>,
    AxumPath(n): AxumPath<u8>,
    CurrentSession(sid): CurrentSession,
    Signed(body_bytes): Signed,
) -> Response {
    let slot = match SlotIndex::from_display(n) {
        Ok(s) => s,
        Err(_) => {
            return (StatusCode::BAD_REQUEST, format!("slot {n} out of range")).into_response();
        }
    };
    let body: LoadBody = match serde_json::from_slice(&body_bytes) {
        Ok(b) => b,
        Err(e) => return (StatusCode::BAD_REQUEST, format!("bad load body: {e}")).into_response(),
    };
    let figure = match state.lookup_figure(&body.figure_id) {
        Some(f) => f.clone(),
        None => return (StatusCode::NOT_FOUND, "unknown figure_id").into_response(),
    };
    let figure_id = figure.id.clone();

    // Who placed this figure? The caller's WS session's unlocked profile.
    // `None` means the session is locked — still allowed to load (placed_by
    // just falls back to None on the resulting SlotState); a locked session
    // can still operate the portal while other phones do their own thing.
    let placed_by = state.sessions.profile_of(sid).await;

    // Resolve the actual file path we'll hand to the driver. With a profile
    // unlocked, route through the per-profile working copy (PLAN 3.11.2)
    // so progress persists across loads. Without a profile (no session
    // unlocked yet), fall back to the pack's fresh file — read-only use.
    let path = match &placed_by {
        Some(profile_id) => match crate::working_copies::resolve_load_path(profile_id, &figure) {
            Ok(p) => p,
            Err(e) => {
                return (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    format!("working-copy resolve failed: {e}"),
                )
                    .into_response();
            }
        },
        None => figure.sky_path.clone(),
    };

    // Bump figure_usage.last_used_at. Best-effort — we don't fail the load
    // just because the usage row can't be written.
    if let Some(profile_id) = &placed_by
        && let Err(e) = state
            .profiles
            .record_figure_usage(profile_id, &figure_id.0)
            .await
    {
        warn!("record_figure_usage failed: {e}");
    }

    // Back pressure: atomically flip the slot to Loading, rejecting if it's
    // already in-flight. Avoids queueing a second load that would open a
    // duplicate file dialog on top of the first one still in progress.
    let loading = SlotState::Loading {
        figure_id: Some(figure_id.clone()),
        placed_by: placed_by.clone(),
    };
    {
        let mut p = state.portal.lock().await;
        if matches!(p[slot.as_usize()], SlotState::Loading { .. }) {
            return (StatusCode::TOO_MANY_REQUESTS, "slot busy").into_response();
        }
        p[slot.as_usize()] = loading.clone();
    }
    let _ = state.events.send(skylander_core::Event::SlotChanged {
        slot,
        state: loading,
    });

    let job = DriverJob::LoadFigure {
        slot,
        figure_id,
        path,
        placed_by,
        canonical_name: figure.canonical_name.clone(),
    };
    if state.driver_tx.send(job).await.is_err() {
        return (StatusCode::SERVICE_UNAVAILABLE, "driver channel closed").into_response();
    }
    (StatusCode::ACCEPTED, "queued").into_response()
}

/// Reset a figure's working copy to the pack's fresh contents (destroys
/// level/gold/playtime). Expects the phone to have confirmed with the user
/// — extra-confirmation flow for Creation Crystals lives on the phone side.
///
/// Flow: clear the slot, re-fork the working copy from the pack, re-load.
/// Net effect: the same figure is back on the slot but with zero progress.
async fn reset_slot(
    State(state): State<Arc<AppState>>,
    AxumPath(n): AxumPath<u8>,
    CurrentSession(sid): CurrentSession,
    Signed(body_bytes): Signed,
) -> Response {
    let slot = match SlotIndex::from_display(n) {
        Ok(s) => s,
        Err(_) => {
            return (StatusCode::BAD_REQUEST, format!("slot {n} out of range")).into_response();
        }
    };
    #[derive(Deserialize)]
    struct ResetBody {
        figure_id: FigureId,
    }
    let body: ResetBody = match serde_json::from_slice(&body_bytes) {
        Ok(b) => b,
        Err(e) => return (StatusCode::BAD_REQUEST, format!("bad reset body: {e}")).into_response(),
    };
    let figure = match state.lookup_figure(&body.figure_id) {
        Some(f) => f.clone(),
        None => return (StatusCode::NOT_FOUND, "unknown figure_id").into_response(),
    };

    // Reset requires an unlocked profile — it's a destructive per-profile
    // action. A locked session isn't allowed to reset someone else's save.
    let Some(profile_id) = state.sessions.profile_of(sid).await else {
        return (StatusCode::FORBIDDEN, "unlock a profile first").into_response();
    };

    // Back pressure: reject if the slot is already mid-transition.
    let loading = SlotState::Loading {
        figure_id: Some(figure.id.clone()),
        placed_by: Some(profile_id.clone()),
    };
    {
        let mut p = state.portal.lock().await;
        if matches!(p[slot.as_usize()], SlotState::Loading { .. }) {
            return (StatusCode::TOO_MANY_REQUESTS, "slot busy").into_response();
        }
        p[slot.as_usize()] = loading.clone();
    }
    let _ = state.events.send(skylander_core::Event::SlotChanged {
        slot,
        state: loading,
    });

    // Overwrite the working copy with fresh pack bytes. If this fails the
    // driver never gets called; surface the error to the phone as a toast.
    let path = match crate::working_copies::reset_to_fresh(&profile_id, &figure) {
        Ok(p) => p,
        Err(e) => {
            let _ = state.events.send(skylander_core::Event::Error {
                message: format!("Reset failed: {e}"),
            });
            return (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response();
        }
    };

    let job = DriverJob::LoadFigure {
        slot,
        figure_id: figure.id.clone(),
        path,
        placed_by: Some(profile_id),
        canonical_name: figure.canonical_name.clone(),
    };
    if state.driver_tx.send(job).await.is_err() {
        return (StatusCode::SERVICE_UNAVAILABLE, "driver channel closed").into_response();
    }
    (StatusCode::ACCEPTED, "queued").into_response()
}

async fn clear_slot(
    State(state): State<Arc<AppState>>,
    AxumPath(n): AxumPath<u8>,
    Signed(_body): Signed,
) -> Response {
    let slot = match SlotIndex::from_display(n) {
        Ok(s) => s,
        Err(_) => {
            return (StatusCode::BAD_REQUEST, format!("slot {n} out of range")).into_response();
        }
    };

    // placed_by=None for clears — the slot is emptying, ownership doesn't
    // apply. Any inbound Loaded after this keeps its own placed_by.
    let loading = SlotState::Loading {
        figure_id: None,
        placed_by: None,
    };
    {
        let mut p = state.portal.lock().await;
        if matches!(p[slot.as_usize()], SlotState::Loading { .. }) {
            return (StatusCode::TOO_MANY_REQUESTS, "slot busy").into_response();
        }
        p[slot.as_usize()] = loading.clone();
    }
    let _ = state.events.send(skylander_core::Event::SlotChanged {
        slot,
        state: loading,
    });

    if state
        .driver_tx
        .send(DriverJob::ClearSlot { slot })
        .await
        .is_err()
    {
        return (StatusCode::SERVICE_UNAVAILABLE, "driver channel closed").into_response();
    }
    (StatusCode::ACCEPTED, "queued").into_response()
}

async fn refresh_portal(State(state): State<Arc<AppState>>, Signed(_body): Signed) -> Response {
    if state
        .driver_tx
        .send(DriverJob::RefreshPortal)
        .await
        .is_err()
    {
        return (StatusCode::SERVICE_UNAVAILABLE, "driver channel closed").into_response();
    }
    (StatusCode::ACCEPTED, "queued").into_response()
}

#[derive(Serialize)]
struct PublicGame {
    serial: GameSerial,
    display_name: String,
}

impl From<&InstalledGame> for PublicGame {
    fn from(g: &InstalledGame) -> Self {
        PublicGame {
            serial: g.serial.clone(),
            display_name: g.display_name.clone(),
        }
    }
}

async fn list_games(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    let list: Vec<PublicGame> = state.games.iter().map(PublicGame::from).collect();
    axum::Json(list)
}

#[derive(Serialize)]
struct StatusBody {
    /// True iff an `RpcsProcess` handle is held in the lifecycle.
    /// Under PLAN 4.15.16 this stays true across game quits (stop_emulation
    /// leaves the process alive); it's false only during the brief startup
    /// window before the initial spawn completes and during crash-respawn.
    /// The phone game picker can use this to gate launch affordances on
    /// "emulator actually ready".
    rpcs3_running: bool,
    current_game: Option<GameLaunched>,
}

async fn get_status(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    let rpcs3 = state.rpcs3.lock().await;
    axum::Json(StatusBody {
        rpcs3_running: rpcs3.process.is_some(),
        current_game: rpcs3.current.clone(),
    })
}

#[derive(Deserialize)]
struct LaunchBody {
    serial: GameSerial,
}

/// RAII handle that clears `LauncherStatus::loading_game` on drop UNLESS
/// explicitly disarmed via [`LoadingGuard::disarm`].
///
/// The launch handler arms a guard right after setting `loading_game` so
/// every error-return path scrubs the LOADING surface (otherwise the TV
/// would sit on it forever after a boot failure). Once boot succeeds, the
/// success path disarms the guard so `loading_game` survives all the way
/// through shader/cache compile (~minutes for first runs) — the launcher
/// dispatcher clears it later when it transitions to the in-game render.
/// Disarming is essential: without it, the QR card flashes back the
/// instant `launch_game` returns (Chris flagged 2026-04-19).
struct LoadingGuard<'a> {
    status: &'a Arc<std::sync::Mutex<crate::state::LauncherStatus>>,
    clear_on_drop: bool,
}

impl<'a> LoadingGuard<'a> {
    /// New guard armed to clear on drop. Use after writing
    /// `loading_game = Some(...)` so that any early-return error path
    /// implicitly scrubs the surface.
    fn armed(status: &'a Arc<std::sync::Mutex<crate::state::LauncherStatus>>) -> Self {
        Self {
            status,
            clear_on_drop: true,
        }
    }

    /// Cancel the on-drop clear. Call only on the success path, after the
    /// post-boot `LauncherStatus` write — the launcher dispatcher then owns
    /// clearing `loading_game` on the in-game transition.
    fn disarm(&mut self) {
        self.clear_on_drop = false;
    }
}

impl Drop for LoadingGuard<'_> {
    fn drop(&mut self) {
        if !self.clear_on_drop {
            return;
        }
        if let Ok(mut st) = self.status.lock() {
            st.loading_game = None;
        }
    }
}

async fn launch_game(State(state): State<Arc<AppState>>, Signed(body_bytes): Signed) -> Response {
    let body: LaunchBody = match serde_json::from_slice(&body_bytes) {
        Ok(b) => b,
        Err(e) => {
            return (StatusCode::BAD_REQUEST, format!("bad launch body: {e}")).into_response();
        }
    };
    let game = match state.lookup_game(&body.serial) {
        Some(g) => g.clone(),
        None => return (StatusCode::NOT_FOUND, "unknown serial").into_response(),
    };

    // Hold the rpcs3 lock across the whole launch so we can't race.
    // Under the always-running RPCS3 contract (PLAN 4.15.16): the
    // process should already exist from server startup. If it's
    // missing, either startup is still in flight or the crash watchdog
    // is mid-respawn — return 503 so the phone retries rather than
    // mis-reporting a "conflict".
    let guard = state.rpcs3.lock().await;
    if guard.process.is_none() {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            "RPCS3 isn't ready yet; hold tight",
        )
            .into_response();
    }
    if guard.current.is_some() {
        return (
            StatusCode::CONFLICT,
            "another game is already running; quit it first",
        )
            .into_response();
    }
    // Drop the lock before we start sending driver jobs so other
    // handlers (crash-aware status reads, /api/quit) aren't serialised
    // behind the enumerate + boot round-trip.
    drop(guard);

    info!(
        serial = %body.serial,
        display_name = %game.display_name,
        "booting game (RPCS3 already running — UIA-pick + boot by serial)",
    );

    // Tell the launcher UI we're loading. RAII so any error return
    // path below automatically clears the flag — the launcher would
    // otherwise sit on a stale "LOADING ..." surface forever after a
    // boot failure. The success path also clears (drop on function
    // return) and then sets `rpcs3_running = true` + `current_game =
    // Some(name)`, which transitions the launcher into in-game.
    if let Ok(mut st) = state.launcher_status.lock() {
        st.loading_game = Some(game.display_name.clone());
    }
    let mut loading_guard = LoadingGuard::armed(&state.launcher_status);

    // PLAN 3.7.8 phase 1: verify the requested serial is actually in
    // RPCS3's library before committing to a boot. Under 4.15.16 we
    // don't kill RPCS3 on miss anymore — it stays at library view so
    // the user can pick a different game without waiting for a
    // respawn. Empty-list / UIA-error fall through to boot's own
    // error handling as before.
    let (etx, erx) = tokio::sync::oneshot::channel();
    if let Err(e) = state
        .driver_tx
        .send(crate::state::DriverJob::EnumerateGames {
            timeout: Duration::from_secs(5),
            done: etx,
        })
        .await
    {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("queue EnumerateGames: {e}"),
        )
            .into_response();
    }
    match erx.await {
        Ok(Ok(serials)) if !serials.is_empty() => {
            if !serials.iter().any(|s| s == body.serial.as_str()) {
                warn!(
                    serial = %body.serial,
                    available = serials.len(),
                    "requested serial not in RPCS3 library — refusing boot",
                );
                return (
                    StatusCode::NOT_FOUND,
                    format!(
                        "{} ({}) isn't in RPCS3's library. \
                         Re-scan games in RPCS3 and try again.",
                        game.display_name,
                        body.serial.as_str(),
                    ),
                )
                    .into_response();
            }
        }
        Ok(Ok(_empty)) => {
            warn!("library enumeration returned empty; skipping pre-boot verify");
        }
        Ok(Err(e)) => {
            warn!("library enumeration failed: {e}; falling through to boot");
        }
        Err(e) => {
            warn!("EnumerateGames ack dropped: {e}; falling through to boot");
        }
    }

    let (tx, rx) = tokio::sync::oneshot::channel();
    if let Err(e) = state
        .driver_tx
        .send(crate::state::DriverJob::BootGame {
            serial: body.serial.as_str().to_string(),
            timeout: Duration::from_secs(60),
            done: tx,
        })
        .await
    {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("queue BootGame: {e}"),
        )
            .into_response();
    }
    match rx.await {
        Ok(Ok(())) => {}
        Ok(Err(e)) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("UIA-boot failed: {e}"),
            )
                .into_response();
        }
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("BootGame ack dropped: {e}"),
            )
                .into_response();
        }
    }

    let launched = GameLaunched {
        serial: game.serial.clone(),
        display_name: game.display_name.clone(),
    };
    // Re-acquire the rpcs3 lock to publish `current`. The window between
    // drop + reacquire is fine: /api/quit and /api/shutdown both check
    // `current` (not `process`) to decide whether a game is running, and
    // neither can preempt us because of the driver worker's serialisation.
    state.rpcs3.lock().await.current = Some(launched.clone());

    // Publish to the launcher status snapshot (PLAN 4.15.4). `rpcs3_running`
    // stayed true across the boot (the process was live the whole time
    // under 4.15.16); only `current_game` flips here.
    if let Ok(mut st) = state.launcher_status.lock() {
        st.current_game = Some(game.display_name.clone());
    }
    // Boot succeeded — disarm the loading guard so `loading_game`
    // persists through shader/cache compile until the launcher
    // dispatcher clears it on the in-game transition.
    loading_guard.disarm();

    let _ = state.events.send(Event::GameChanged {
        current: Some(launched),
    });

    (StatusCode::ACCEPTED, "launched").into_response()
}

#[derive(Deserialize)]
struct QuitQuery {
    #[serde(default)]
    force: bool,
}

#[cfg(feature = "test-hooks")]
#[derive(Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum InjectLoadOutcome {
    Ok,
    FileInUse { message: Option<String> },
    QtModal { message: String },
    Timeout,
}

#[cfg(feature = "test-hooks")]
#[derive(Deserialize)]
struct InjectLoadBody {
    /// Sequence of outcomes for upcoming `load` calls.
    outcomes: Vec<InjectLoadOutcome>,
}

#[cfg(feature = "test-hooks")]
#[derive(Deserialize)]
struct SetGameBody {
    /// `None` → clear the current game.
    current: Option<GameLaunched>,
}

#[cfg(feature = "test-hooks")]
async fn set_game_state(
    State(state): State<Arc<AppState>>,
    axum::Json(body): axum::Json<SetGameBody>,
) -> Response {
    let mut guard = state.rpcs3.lock().await;
    guard.current = body.current.clone();
    let _ = state.events.send(Event::GameChanged {
        current: body.current,
    });
    (StatusCode::ACCEPTED, "set").into_response()
}

#[cfg(feature = "test-hooks")]
async fn inject_load(
    State(state): State<Arc<AppState>>,
    axum::Json(body): axum::Json<InjectLoadBody>,
) -> Response {
    let mock = match &state.test_mock {
        Some(m) => m.clone(),
        None => {
            return (
                StatusCode::CONFLICT,
                "/api/_test/inject_load requires the mock driver",
            )
                .into_response();
        }
    };
    let mapped: Vec<skylander_rpcs3_control::MockOutcome> = body
        .outcomes
        .into_iter()
        .map(|o| match o {
            InjectLoadOutcome::Ok => skylander_rpcs3_control::MockOutcome::Ok,
            InjectLoadOutcome::FileInUse { message } => {
                skylander_rpcs3_control::MockOutcome::FileInUse {
                    message: message.unwrap_or_else(|| "file is in use".into()),
                }
            }
            InjectLoadOutcome::QtModal { message } => {
                skylander_rpcs3_control::MockOutcome::QtModal { message }
            }
            InjectLoadOutcome::Timeout => skylander_rpcs3_control::MockOutcome::Timeout,
        })
        .collect();
    mock.queue_load_outcomes(mapped);
    (StatusCode::ACCEPTED, "queued").into_response()
}

async fn quit_game(
    State(state): State<Arc<AppState>>,
    axum::extract::Query(q): axum::extract::Query<QuitQuery>,
    Signed(_body): Signed,
) -> Response {
    // Under the always-running RPCS3 contract (PLAN 4.15.16), "quit"
    // means "stop the current game and return RPCS3 to library view".
    // The process stays alive; only `current` clears. `force=true`
    // keeps the old escape hatch — kill the process and let the crash
    // watchdog respawn it — for cases where the UIA stop path is
    // wedged or the game is unresponsive.
    let mut guard = state.rpcs3.lock().await;
    if guard.current.is_none() {
        return (StatusCode::CONFLICT, "no game is running").into_response();
    }
    // Reset current immediately — the quit is committed the moment we
    // decide to stop, whether the UIA click takes a moment or not.
    guard.current = None;

    if q.force {
        // Force path: take the process, kill it, let the watchdog
        // respawn a fresh library-view RPCS3. Rare; useful when
        // UIA stop doesn't work and the user needs a hard reset.
        let mut proc = match guard.process.take() {
            Some(p) => p,
            None => {
                drop(guard);
                return (
                    StatusCode::CONFLICT,
                    "no RPCS3 process to force-kill",
                )
                    .into_response();
            }
        };
        drop(guard);
        let result = tokio::task::spawn_blocking(move || {
            proc.shutdown_graceful(Duration::from_millis(500))
        })
        .await;
        match result {
            Ok(Ok(path)) => info!(?path, "force-quit: process killed"),
            Ok(Err(e)) => warn!("force-quit errored: {e}"),
            Err(e) => warn!("force-quit task panicked: {e}"),
        }
        if let Ok(mut st) = state.launcher_status.lock() {
            st.rpcs3_running = false;
            st.current_game = None;
        }
    } else {
        // Normal path: stop emulation via UIA. Process stays alive at
        // library view; next launch is a BootGame away.
        drop(guard);
        let (tx, rx) = tokio::sync::oneshot::channel();
        if let Err(e) = state
            .driver_tx
            .send(crate::state::DriverJob::StopEmulation {
                timeout: Duration::from_secs(10),
                done: tx,
            })
            .await
        {
            warn!("queue StopEmulation: {e}");
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("queue StopEmulation: {e}"),
            )
                .into_response();
        }
        match rx.await {
            Ok(Ok(())) => info!("stop_emulation succeeded; RPCS3 back at library"),
            Ok(Err(e)) => {
                warn!("stop_emulation failed: {e}; leaving current cleared anyway");
                // Don't revert `current` — the user's intent was to
                // quit, and retrying with ?force=true is the escape
                // hatch for a wedged stop.
            }
            Err(e) => warn!("StopEmulation ack dropped: {e}"),
        }
        if let Ok(mut st) = state.launcher_status.lock() {
            // rpcs3_running stays true — process is alive. Only the
            // current-game field clears.
            st.current_game = None;
        }
    }

    // Reset the portal snapshot since the game is no longer loaded.
    *state.portal.lock().await = std::array::from_fn(|_| SlotState::Empty);
    let _ = state.events.send(Event::PortalSnapshot {
        slots: std::array::from_fn(|_| SlotState::Empty),
    });
    let _ = state.events.send(Event::GameChanged { current: None });

    (StatusCode::ACCEPTED, "quit").into_response()
}

/// POST /api/shutdown — phone's SHUT DOWN menu action (PLAN 4.15.11 +
/// 4.15.16).
///
/// Coordinated exit:
/// 1. If a game is running, stop emulation first so RPCS3 can flush its
///    caches cleanly. Best-effort — if stop fails we still proceed.
/// 2. Gracefully shut down the RPCS3 process (File → Exit via WM_CLOSE).
///    The launcher's Job Object would kill it on viewport-close anyway,
///    but a graceful exit gives RPCS3 a chance to write its config +
///    shader caches before it dies.
/// 3. Flip the launcher into the `Farewell` surface. The egui side
///    runs a ~3s countdown, then sends `ViewportCommand::Close` which
///    is the same mechanism the Exit-to-Desktop button uses.
///
/// We don't exit the server process directly because (a) the eframe
/// loop owns the only legal way to close the viewport cleanly, and
/// (b) the farewell screen gives the user visual confirmation before
/// the window disappears.
async fn shutdown_launcher(State(state): State<Arc<AppState>>, Signed(_body): Signed) -> Response {
    info!("shutdown requested via /api/shutdown");

    // Step 1: stop the current game if any. Brief timeout since we're
    // on the way out — don't block the shutdown on a wedged UIA call.
    let has_game = {
        let guard = state.rpcs3.lock().await;
        guard.current.is_some()
    };
    if has_game {
        let (tx, rx) = tokio::sync::oneshot::channel();
        if state
            .driver_tx
            .send(crate::state::DriverJob::StopEmulation {
                timeout: Duration::from_secs(5),
                done: tx,
            })
            .await
            .is_ok()
        {
            match rx.await {
                Ok(Ok(())) => info!("shutdown: stop_emulation ok"),
                Ok(Err(e)) => warn!("shutdown: stop_emulation failed: {e}"),
                Err(e) => warn!("shutdown: StopEmulation ack dropped: {e}"),
            }
        }
        let mut guard = state.rpcs3.lock().await;
        guard.current = None;
    }

    // Step 2: graceful RPCS3 exit. Take the process so the crash
    // watchdog doesn't see it die and try to respawn.
    let process = state.rpcs3.lock().await.process.take();
    if let Some(mut proc) = process {
        let result = tokio::task::spawn_blocking(move || {
            proc.shutdown_graceful(Duration::from_secs(5))
        })
        .await;
        match result {
            Ok(Ok(path)) => info!(?path, "shutdown: RPCS3 exited"),
            Ok(Err(e)) => warn!("shutdown: RPCS3 shutdown errored: {e}"),
            Err(e) => warn!("shutdown: RPCS3 shutdown task panicked: {e}"),
        }
    }

    // Step 3: flip the launcher to Farewell. eframe's countdown fires
    // ViewportCommand::Close when it hits zero.
    if let Ok(mut st) = state.launcher_status.lock() {
        st.screen = crate::state::LauncherScreen::Farewell;
        st.rpcs3_running = false;
        st.current_game = None;
    }
    (StatusCode::ACCEPTED, "farewell").into_response()
}

async fn ws_handler(ws: WebSocketUpgrade, State(state): State<Arc<AppState>>) -> impl IntoResponse {
    ws.on_upgrade(move |socket| handle_ws(socket, state))
}

async fn handle_ws(socket: WebSocket, state: Arc<AppState>) {
    state
        .connected_clients
        .fetch_add(1, std::sync::atomic::Ordering::Relaxed);

    let (mut sender, mut receiver) = socket.split();

    // Register with the 2-slot FIFO. Three outcomes:
    //   - Admitted(sid): seat was free, proceed.
    //   - AdmittedByEvicting { session, evicted }: we took the oldest seat;
    //     broadcast TakenOver to the evicted session so its phone flips to
    //     the Kaos screen.
    //   - RejectedByCooldown: both seats full and the 1-min forced-evict
    //     cooldown hasn't elapsed; send an Error event and close.
    let sid = match state.sessions.register().await {
        RegistrationOutcome::Admitted(sid) => sid,
        RegistrationOutcome::AdmittedByEvicting { session, evicted } => {
            let _ = state.events.send(Event::TakenOver {
                session_id: evicted.0,
                by_kaos: "Kaos".to_string(),
            });
            session
        }
        RegistrationOutcome::RejectedByCooldown { retry_after } => {
            let secs = retry_after.as_secs_f32().ceil() as u64;
            let evt = Event::Error {
                message: format!(
                    "Portal is full and a takeover just happened. Try again in {secs}s."
                ),
            };
            if let Ok(j) = serde_json::to_string(&evt) {
                let _ = sender.send(Message::Text(j)).await;
            }
            let _ = sender.send(Message::Close(None)).await;
            state
                .connected_clients
                .fetch_sub(1, std::sync::atomic::Ordering::Relaxed);
            return;
        }
    };
    // Publish the new session snapshot so the TV launcher can update its
    // orbit pip count + max-players card flip (PLAN 4.15.6 / 4.15.7).
    state.publish_session_snapshot().await;

    // First message: tell the client its session id so it can attach it on
    // REST and filter targeted broadcast events for itself.
    {
        let evt = Event::Welcome {
            session_id: sid.0,
            boot_id: state.boot_id,
        };
        if let Ok(j) = serde_json::to_string(&evt) {
            let _ = sender.send(Message::Text(j)).await;
        }
    }

    // Portal snapshot.
    {
        let snap: [SlotState; SLOT_COUNT] = state.portal.lock().await.clone();
        let evt = Event::PortalSnapshot { slots: snap };
        if let Ok(j) = serde_json::to_string(&evt) {
            let _ = sender.send(Message::Text(j)).await;
        }
    }

    // Session's unlocked profile (may have been seeded by `pending_unlock`
    // from a test-hook, or stay None in production until the phone
    // unlocks).
    {
        let current_profile = state.sessions.profile_of(sid).await;
        let unlocked = match &current_profile {
            Some(pid) => match state.profiles.get(pid).await {
                Ok(Some(row)) => Some(UnlockedProfile {
                    id: row.id,
                    display_name: row.display_name,
                    color: row.color,
                }),
                _ => None,
            },
            None => None,
        };
        let evt = Event::ProfileChanged {
            session_id: sid.0,
            profile: unlocked,
        };
        if let Ok(j) = serde_json::to_string(&evt) {
            let _ = sender.send(Message::Text(j)).await;
        }
        // If this session came in with a profile already bound (via
        // `pending_unlock`, typically after a reload or test-hook seed),
        // offer the resume prompt here. Without this hook the WS-reconnect
        // path would silently miss it — `unlock_profile` is only the
        // explicit-PIN-entry path.
        //
        // Sent on `sender` directly (not broadcast) because we're still
        // pre-writer-task: broadcast messages emitted before
        // `state.events.subscribe()` below would be dropped.
        if let Some(pid) = current_profile
            && let Some(evt) = build_resume_prompt(&state, sid.0, &pid).await
            && let Ok(j) = serde_json::to_string(&evt)
        {
            let _ = sender.send(Message::Text(j)).await;
        }
    }

    let mut rx: broadcast::Receiver<Event> = state.events.subscribe();

    // Writer task — forward broadcast events to the socket. No server-side
    // filtering: each client gets every event, and client-side JS filters
    // session-targeted variants (`ProfileChanged`, `TakenOver`) by
    // `session_id`. The set of possible values is tiny (≤2) so the network
    // overhead is negligible.
    let writer = tokio::spawn(async move {
        while let Ok(evt) = rx.recv().await {
            match serde_json::to_string(&evt) {
                Ok(j) => {
                    if sender.send(Message::Text(j)).await.is_err() {
                        break;
                    }
                }
                Err(e) => debug!("serialize event: {e}"),
            }
        }
    });

    // Reader loop — no inbound commands yet (REST covers those); just watch
    // for close.
    while let Some(Ok(msg)) = receiver.next().await {
        if matches!(msg, Message::Close(_)) {
            break;
        }
    }
    writer.abort();

    state.sessions.remove(sid).await;
    state.publish_session_snapshot().await;
    state
        .connected_clients
        .fetch_sub(1, std::sync::atomic::Ordering::Relaxed);
}

// ============================================================ profile routes

async fn list_profiles(State(state): State<Arc<AppState>>) -> Response {
    match state.profiles.list().await {
        Ok(rows) => {
            let public: Vec<PublicProfile> = rows.iter().map(PublicProfile::from).collect();
            axum::Json(public).into_response()
        }
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    }
}

#[derive(Deserialize)]
struct CreateProfileBody {
    display_name: String,
    pin: String,
    color: String,
}

async fn create_profile(
    State(state): State<Arc<AppState>>,
    Signed(body_bytes): Signed,
) -> Response {
    let body: CreateProfileBody = match serde_json::from_slice(&body_bytes) {
        Ok(b) => b,
        Err(e) => {
            return (
                StatusCode::BAD_REQUEST,
                format!("bad create-profile body: {e}"),
            )
                .into_response();
        }
    };
    match state.profiles.count().await {
        Ok(n) if (n as usize) >= MAX_PROFILES => {
            return (
                StatusCode::CONFLICT,
                format!("profile limit reached ({MAX_PROFILES})"),
            )
                .into_response();
        }
        Ok(_) => {}
        Err(e) => return (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    }

    match state
        .profiles
        .create(&body.display_name, &body.pin, &body.color)
        .await
    {
        Ok(id) => (
            StatusCode::CREATED,
            axum::Json(serde_json::json!({
                "id": id,
                "display_name": body.display_name,
                "color": body.color,
            })),
        )
            .into_response(),
        Err(e) => (StatusCode::BAD_REQUEST, e.to_string()).into_response(),
    }
}

#[derive(Deserialize)]
struct PinBody {
    pin: String,
}

async fn delete_profile(
    State(state): State<Arc<AppState>>,
    AxumPath(id): AxumPath<String>,
    Signed(body_bytes): Signed,
) -> Response {
    let body: PinBody = match serde_json::from_slice(&body_bytes) {
        Ok(b) => b,
        Err(e) => {
            return (StatusCode::BAD_REQUEST, format!("bad delete body: {e}")).into_response();
        }
    };
    match state.profiles.verify_pin(&id, &body.pin).await {
        Ok(true) => {}
        Ok(false) => return (StatusCode::UNAUTHORIZED, "wrong pin").into_response(),
        Err(e) => return (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    }

    match state.profiles.delete(&id).await {
        Ok(true) => (StatusCode::OK, "deleted").into_response(),
        Ok(false) => (StatusCode::NOT_FOUND, "no such profile").into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    }
}

async fn unlock_profile(
    State(state): State<Arc<AppState>>,
    AxumPath(id): AxumPath<String>,
    CurrentSession(sid): CurrentSession,
    Signed(body_bytes): Signed,
) -> Response {
    let body: PinBody = match serde_json::from_slice(&body_bytes) {
        Ok(b) => b,
        Err(e) => {
            return (StatusCode::BAD_REQUEST, format!("bad unlock body: {e}")).into_response();
        }
    };
    let now = std::time::Instant::now();
    match state.profiles.lockouts.check(&id, now).await {
        LockoutCheck::LockedOut { retry_after } => {
            let secs = retry_after.as_secs_f32().ceil() as u64;
            return axum::response::Response::builder()
                .status(StatusCode::UNAUTHORIZED)
                .header("Retry-After", secs.max(1).to_string())
                .body(axum::body::Body::from(format!(
                    "locked out; retry in {secs}s"
                )))
                .unwrap();
        }
        LockoutCheck::Allowed => {}
    }

    let ok = match state.profiles.verify_pin(&id, &body.pin).await {
        Ok(v) => v,
        Err(e) => return (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    };

    if !ok {
        let triggered = state.profiles.lockouts.record_failure(&id, now).await;
        if triggered {
            return axum::response::Response::builder()
                .status(StatusCode::UNAUTHORIZED)
                .header(
                    "Retry-After",
                    crate::profiles::LOCKOUT_DURATION.as_secs().to_string(),
                )
                .body(axum::body::Body::from("too many attempts; locked out"))
                .unwrap();
        }
        return (StatusCode::UNAUTHORIZED, "wrong pin").into_response();
    }

    state.profiles.lockouts.record_success(&id).await;

    let row = match state.profiles.get(&id).await {
        Ok(Some(r)) => r,
        Ok(None) => return (StatusCode::NOT_FOUND, "no such profile").into_response(),
        Err(e) => return (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    };

    // Bind the unlocked profile to this specific WS session. Other phones
    // stay independent.
    state.sessions.set_profile(sid, Some(id.clone())).await;
    // Republish session pips so the TV orbit picks up this profile's
    // colour + initial (PLAN 4.15.7).
    state.publish_session_snapshot().await;

    let unlocked = UnlockedProfile {
        id: row.id.clone(),
        display_name: row.display_name.clone(),
        color: row.color.clone(),
    };
    // Broadcast — both connected clients receive it, only the one whose
    // session_id matches applies the update.
    let _ = state.events.send(Event::ProfileChanged {
        session_id: sid.0,
        profile: Some(unlocked.clone()),
    });

    // PLAN 3.12.2: if this profile has a stored portal layout from an earlier
    // session, offer a "Resume last setup?" prompt. Broadcast; the client
    // filters by its own session id. Silent on DB errors — resume is a nice-
    // to-have, not worth failing the unlock for.
    maybe_send_resume_prompt(&state, sid.0, &id).await;

    (StatusCode::OK, axum::Json(unlocked)).into_response()
}

/// Load + parse the persisted layout for `profile_id`, returning an
/// `Event::ResumePrompt` if one should be offered (layout exists and isn't
/// all-Empty). Caller sends via either `sender` (direct, for handshake)
/// or `state.events` (broadcast, for post-subscribe events).
async fn build_resume_prompt(
    state: &Arc<AppState>,
    session_id: u64,
    profile_id: &str,
) -> Option<Event> {
    let json = match state.profiles.load_portal_layout(profile_id).await {
        Ok(Some(j)) => j,
        Ok(None) => return None,
        Err(e) => {
            warn!("load_portal_layout({profile_id}): {e}");
            return None;
        }
    };
    let slots: [SlotState; SLOT_COUNT] = match serde_json::from_str(&json) {
        Ok(a) => a,
        Err(e) => {
            warn!("parse last_portal_layout_json for {profile_id}: {e}");
            return None;
        }
    };
    if slots.iter().all(|s| matches!(s, SlotState::Empty)) {
        return None;
    }
    Some(Event::ResumePrompt { session_id, slots })
}

/// Convenience wrapper that builds + broadcasts. Used from REST handlers
/// (`unlock_profile`, the test-hook unlock path) where the recipient
/// session already has a writer task draining the broadcast channel.
async fn maybe_send_resume_prompt(state: &Arc<AppState>, session_id: u64, profile_id: &str) {
    if let Some(evt) = build_resume_prompt(state, session_id, profile_id).await {
        let _ = state.events.send(evt);
    }
}

async fn lock_profile(
    State(state): State<Arc<AppState>>,
    AxumPath(_id): AxumPath<String>,
    CurrentSession(sid): CurrentSession,
    Signed(_body): Signed,
) -> Response {
    state.sessions.set_profile(sid, None).await;
    // Republish session pips — the lock drops this profile's colour
    // from the TV orbit indicator (PLAN 4.15.7).
    state.publish_session_snapshot().await;
    let _ = state.events.send(Event::ProfileChanged {
        session_id: sid.0,
        profile: None,
    });
    (StatusCode::OK, "locked").into_response()
}

#[derive(Deserialize)]
struct ResetPinBody {
    current_pin: String,
    new_pin: String,
}

async fn reset_pin(
    State(state): State<Arc<AppState>>,
    AxumPath(id): AxumPath<String>,
    Signed(body_bytes): Signed,
) -> Response {
    let body: ResetPinBody = match serde_json::from_slice(&body_bytes) {
        Ok(b) => b,
        Err(e) => return (StatusCode::BAD_REQUEST, format!("bad reset body: {e}")).into_response(),
    };
    match state.profiles.verify_pin(&id, &body.current_pin).await {
        Ok(true) => {}
        Ok(false) => return (StatusCode::UNAUTHORIZED, "wrong pin").into_response(),
        Err(e) => return (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    }
    match state.profiles.reset_pin(&id, &body.new_pin).await {
        Ok(()) => (StatusCode::OK, "updated").into_response(),
        Err(e) => (StatusCode::BAD_REQUEST, e.to_string()).into_response(),
    }
}

// ============================================================ icons + manifest

/// In debug builds, swap icon requests to the Kaos-tinted variants so
/// pinned dev installs are visually distinct from prod installs on the
/// home screen. In release builds, serve the gold variants. The phone's
/// index.html requests stable URLs (`/icons/icon-180.png`,
/// `/icons/icon.svg`); we map them to the appropriate on-disk file.
///
/// Gated on `cfg!(debug_assertions)`, NOT the `dev-tools` Cargo feature:
/// the icon swap is purely a build-flavor visual signal, and the user-
/// intuitive contract is "`cargo run --release` looks like production."
/// Cargo features are orthogonal to profiles, so `--release` alone keeps
/// default features (incl. `dev-tools`) on — feature-gating the icon
/// swap would leave `--release` builds wearing the dev tint. Things that
/// genuinely depend on the dev-tools machinery (mock driver, `.env.dev`,
/// dev_log endpoint) stay feature-gated; this is a cosmetic decision.
///
/// Recognized URL shapes:
///   - `icon-{size}.png` → `icon-dev-{size}.png` (debug) or unchanged (release)
///   - `icon.svg`        → `icon-dev.svg`        (debug) or unchanged (release)
fn dev_swapped(filename: &str) -> Option<String> {
    if !cfg!(debug_assertions) {
        return None;
    }
    // Already-dev names — never double-swap. Check both shapes (with and
    // without trailing dash) before any prefix manipulation; the test
    // `dev_build_does_not_double_swap` caught a bug where `icon-dev.svg`
    // slipped through the dash-required check and became `icon-dev-dev.svg`.
    if filename.starts_with("icon-dev-") || filename == "icon-dev.svg" {
        return None;
    }
    if filename == "icon.svg" {
        return Some("icon-dev.svg".to_string());
    }
    if let Some(rest) = filename.strip_prefix("icon-") {
        return Some(format!("icon-dev-{rest}"));
    }
    None
}

async fn serve_icon(
    State(state): State<Arc<AppState>>,
    AxumPath(filename): AxumPath<String>,
) -> Response {
    // Path-traversal guard. The single-segment matcher already prevents
    // slashes, but `..` could still be slipped in. Belt + braces.
    if filename.contains("..") {
        return (StatusCode::BAD_REQUEST, "bad icon name").into_response();
    }
    let resolved = dev_swapped(&filename).unwrap_or_else(|| filename.clone());
    let content_type = if resolved.ends_with(".svg") {
        "image/svg+xml"
    } else {
        "image/png"
    };
    let path = state.phone_dist.join("icons").join(&resolved);
    serve_static_file(&path, content_type).await
}

async fn serve_manifest(State(state): State<Arc<AppState>>) -> Response {
    // Same gate as the icon swap — see `dev_swapped` doc comment for why
    // this uses `debug_assertions` and not the `dev-tools` feature.
    let filename = if cfg!(debug_assertions) {
        "manifest-dev.webmanifest"
    } else {
        "manifest.webmanifest"
    };
    let path = state.phone_dist.join(filename);
    // PWA manifest spec MIME is application/manifest+json; some browsers
    // also accept application/json. Use the spec one — Chrome and Safari
    // both honor it.
    serve_static_file(&path, "application/manifest+json").await
}

async fn serve_static_file(path: &std::path::Path, content_type: &'static str) -> Response {
    match tokio::fs::read(path).await {
        Ok(bytes) => (
            StatusCode::OK,
            [(axum::http::header::CONTENT_TYPE, content_type)],
            bytes,
        )
            .into_response(),
        Err(_) => (StatusCode::NOT_FOUND, "asset not found").into_response(),
    }
}

// ============================================================ dev-tools

/// Dev-time log forwarder. Phone POSTs a batch of console-mirror entries;
/// we re-emit each as `tracing` output so it lands in whatever sink the
/// launcher process is using (PowerShell on the dev HTPC, daily-rotated
/// files in dev-data/logs/, etc.). Lets us debug real-device behavior
/// without a Mac + Safari Web Inspector tether.
///
/// Compiled into dev/test builds only — gated on the default-on `dev-tools`
/// feature so release builds physically can't accept phone log bodies.
/// PLAN 4.18.21 supporting infra.
#[cfg(feature = "dev-tools")]
#[derive(Deserialize)]
struct DevLogEntry {
    /// Browser-side `Date.now()` (ms since epoch). Useful for ordering
    /// when batches from multiple phones interleave in the server log.
    t: f64,
    level: String,
    msg: String,
}

#[cfg(feature = "dev-tools")]
async fn dev_log(axum::Json(entries): axum::Json<Vec<DevLogEntry>>) -> StatusCode {
    for e in entries {
        // Choose tracing level by the JS source. Anything weird falls
        // through as info — never want a malformed batch to cost a log line.
        match e.level.as_str() {
            "warn" => tracing::warn!(target: "phone", browser_t = e.t, "{}", e.msg),
            "error" => tracing::error!(target: "phone", browser_t = e.t, "{}", e.msg),
            _ => tracing::info!(target: "phone", browser_t = e.t, "{}", e.msg),
        }
    }
    StatusCode::NO_CONTENT
}

// ============================================================ test-hooks

#[cfg(feature = "test-hooks")]
#[derive(Deserialize)]
struct InjectProfileBody {
    name: String,
    pin: String,
    color: String,
}

#[cfg(feature = "test-hooks")]
async fn inject_profile(
    State(state): State<Arc<AppState>>,
    axum::Json(body): axum::Json<InjectProfileBody>,
) -> Response {
    match state
        .profiles
        .create(&body.name, &body.pin, &body.color)
        .await
    {
        Ok(id) => (
            StatusCode::CREATED,
            axum::Json(serde_json::json!({ "id": id })),
        )
            .into_response(),
        Err(e) => (StatusCode::BAD_REQUEST, e.to_string()).into_response(),
    }
}

#[cfg(feature = "test-hooks")]
#[derive(Deserialize)]
struct UnlockSessionBody {
    profile_id: String,
}

#[cfg(feature = "test-hooks")]
async fn layout_testhook(
    State(state): State<Arc<AppState>>,
    AxumPath(profile_id): AxumPath<String>,
) -> Response {
    match state.profiles.load_portal_layout(&profile_id).await {
        Ok(Some(json)) => (StatusCode::OK, json).into_response(),
        Ok(None) => (StatusCode::NO_CONTENT, "").into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    }
}

#[cfg(feature = "test-hooks")]
async fn clear_eviction_cooldown_testhook(State(state): State<Arc<AppState>>) -> Response {
    state.sessions.clear_forced_evict_cooldown().await;
    (StatusCode::OK, "cleared").into_response()
}

#[cfg(feature = "test-hooks")]
#[derive(Deserialize)]
struct SetSessionProfileBody {
    session_id: u64,
    profile_id: String,
}

#[cfg(feature = "test-hooks")]
async fn set_session_profile_testhook(
    State(state): State<Arc<AppState>>,
    axum::Json(body): axum::Json<SetSessionProfileBody>,
) -> Response {
    let sid = crate::profiles::SessionId(body.session_id);
    // Verify the profile exists before binding.
    let row = match state.profiles.get(&body.profile_id).await {
        Ok(Some(r)) => r,
        Ok(None) => return (StatusCode::NOT_FOUND, "no such profile").into_response(),
        Err(e) => return (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    };
    state
        .sessions
        .set_profile(sid, Some(body.profile_id.clone()))
        .await;
    state.publish_session_snapshot().await;
    let _ = state.events.send(Event::ProfileChanged {
        session_id: sid.0,
        profile: Some(UnlockedProfile {
            id: row.id,
            display_name: row.display_name,
            color: row.color,
        }),
    });
    (StatusCode::OK, "bound").into_response()
}

#[cfg(feature = "test-hooks")]
async fn hmac_key_testhook(State(state): State<Arc<AppState>>) -> Response {
    // Dev bypass exists precisely so the e2e harness doesn't HAVE to sign
    // requests, but that leaves the HMAC path unverified. Expose the key
    // here so the harness can build `#k=<hex>` URLs and exercise the real
    // signed flow end-to-end. Only compiled when `test-hooks` is on.
    let hex = hex::encode(&state.hmac_key);
    (
        StatusCode::OK,
        axum::Json(serde_json::json!({ "hmac_key": hex })),
    )
        .into_response()
}

#[cfg(feature = "test-hooks")]
async fn unlock_session_testhook(
    State(state): State<Arc<AppState>>,
    axum::Json(body): axum::Json<UnlockSessionBody>,
) -> Response {
    // Confirm the profile exists before we seed it; otherwise tests get a
    // confusing "unlock appeared but then disappeared" when the WS looks it
    // up on connect.
    match state.profiles.get(&body.profile_id).await {
        Ok(Some(_)) => {}
        Ok(None) => return (StatusCode::NOT_FOUND, "no such profile").into_response(),
        Err(e) => return (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    }
    // Seed the next session's unlock so any freshly-registered connection
    // (including reconnect-after-reload) inherits this profile. pending
    // auto-clears on next `register()`. For 2-phone tests, callers must
    // sequence this between Phone::new()s so the right profile lands on
    // each session.
    state
        .sessions
        .set_pending_unlock(Some(body.profile_id.clone()))
        .await;

    // Also push to the most-recent existing session if any is still
    // connected — covers the "test sets up after Phone::new" flow, and the
    // reload race where a new WS registers before `pending_unlock` is set.
    let ids = state.sessions.all_ids().await;
    if let Some(&sid) = ids.iter().max_by_key(|s| s.0) {
        state
            .sessions
            .set_profile(sid, Some(body.profile_id.clone()))
            .await;
        let row = state.profiles.get(&body.profile_id).await.ok().flatten();
        if let Some(row) = row {
            let _ = state.events.send(Event::ProfileChanged {
                session_id: sid.0,
                profile: Some(UnlockedProfile {
                    id: row.id,
                    display_name: row.display_name,
                    color: row.color,
                }),
            });
        }
        // Mirror the real unlock_profile path: after binding a profile to a
        // session, offer the resume prompt if there's a prior layout. This
        // matters for the post-reload flow where the fresh WS registers
        // with no profile and gets bound via this hook after the fact.
        maybe_send_resume_prompt(&state, sid.0, &body.profile_id).await;
        state.publish_session_snapshot().await;
    }
    (StatusCode::OK, "unlocked").into_response()
}

#[cfg(test)]
mod icon_swap_tests {
    //! `dev_swapped` is gated on `cfg!(debug_assertions)`. `cargo test`
    //! always runs in debug profile (--release would also enable
    //! optimizations on tests, which we don't do), so the dev-side
    //! assertions always run. The "release-build is no-op" branch is
    //! covered indirectly by CI's `cargo build --release --workspace`
    //! step plus manual verification when running `cargo run --release`.

    use super::*;

    #[test]
    fn debug_build_maps_png_filenames_to_dev_variant() {
        assert_eq!(
            dev_swapped("icon-180.png").as_deref(),
            Some("icon-dev-180.png")
        );
        assert_eq!(
            dev_swapped("icon-32.png").as_deref(),
            Some("icon-dev-32.png")
        );
        assert_eq!(
            dev_swapped("icon-512.png").as_deref(),
            Some("icon-dev-512.png")
        );
    }

    #[test]
    fn debug_build_maps_svg_to_dev_variant() {
        assert_eq!(dev_swapped("icon.svg").as_deref(), Some("icon-dev.svg"));
    }

    /// Belt + braces: if the SPA somehow already requests the dev-named
    /// file directly, the swap must NOT prepend another `dev-`. Without
    /// this guard we'd 404 because `icon-dev-dev-180.png` doesn't exist.
    #[test]
    fn debug_build_does_not_double_swap() {
        assert_eq!(dev_swapped("icon-dev-180.png"), None);
        assert_eq!(dev_swapped("icon-dev.svg"), None);
    }

    #[test]
    fn debug_build_passes_through_unknown_filenames() {
        assert_eq!(dev_swapped("manifest.webmanifest"), None);
        assert_eq!(dev_swapped("random.png"), None);
        assert_eq!(dev_swapped(""), None);
    }
}

#[cfg(all(test, feature = "dev-tools"))]
mod dev_log_handler_tests {
    //! Server-side tests for `POST /api/_dev/log` (PLAN 4.18.21).
    //!
    //! Pinned contracts:
    //!   - Well-formed batch with mixed levels → 204 No Content.
    //!   - Levels other than log/warn/error fall through to info (no panic,
    //!     no rejection — handler is forgiving so a SPA-side typo doesn't
    //!     drop the batch).
    //!   - Empty batch → 204 (legitimate "tick fired but nothing to send"
    //!     never happens in practice because flusher skips empty, but the
    //!     handler should not 400 on it).
    //!   - Malformed JSON → 4xx (axum's Json extractor returns 400/415).
    //!   - Missing fields → 4xx (deserialization fails).
    //!
    //! These all run via `Router::oneshot` with no live server — fast,
    //! and validates the route wiring lands as expected.
    //!
    //! NOTE: The "messages from a disconnect window get delivered later"
    //! contract is enforced on the SPA side (see `phone/src/dev_log.rs`
    //! tests). The server endpoint itself is stateless — every batch is
    //! independent — so there's nothing for it to "remember" across the
    //! disconnect; the test value here is at the SPA buffer layer.

    use super::*;
    use axum::body::Body;
    use axum::http::Request;
    use http_body_util::BodyExt;
    use tower::ServiceExt;

    fn router() -> Router {
        Router::new().route("/api/_dev/log", post(dev_log))
    }

    async fn post_json(body: &str) -> axum::http::Response<Body> {
        router()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/_dev/log")
                    .header("content-type", "application/json")
                    .body(Body::from(body.to_string()))
                    .unwrap(),
            )
            .await
            .unwrap()
    }

    #[tokio::test]
    async fn well_formed_batch_returns_204() {
        let body = serde_json::json!([
            {"t": 1.0, "level": "log",   "msg": "[ws] onclose attempts 0→1"},
            {"t": 2.0, "level": "warn",  "msg": "[ws] backoff 8s"},
            {"t": 3.0, "level": "error", "msg": "[overlay] grace fired"},
        ]);
        let resp = post_json(&body.to_string()).await;
        assert_eq!(resp.status(), StatusCode::NO_CONTENT);
    }

    #[tokio::test]
    async fn unknown_level_falls_through_to_info() {
        // Forgiveness contract: a typo'd or future level shouldn't 4xx
        // the whole batch. We don't have a tracing capture here, but the
        // 204 confirms the handler reached the end without panicking on
        // the unknown variant.
        let body = serde_json::json!([
            {"t": 1.0, "level": "trace",  "msg": "future-level"},
            {"t": 2.0, "level": "garble", "msg": "typo"},
        ]);
        let resp = post_json(&body.to_string()).await;
        assert_eq!(resp.status(), StatusCode::NO_CONTENT);
    }

    #[tokio::test]
    async fn empty_batch_returns_204() {
        let resp = post_json("[]").await;
        assert_eq!(resp.status(), StatusCode::NO_CONTENT);
    }

    #[tokio::test]
    async fn malformed_json_is_rejected() {
        let resp = post_json("not json").await;
        assert!(
            resp.status().is_client_error(),
            "expected 4xx, got {}",
            resp.status()
        );
    }

    #[tokio::test]
    async fn missing_required_fields_is_rejected() {
        // Each entry needs t/level/msg; omit `msg`.
        let body = serde_json::json!([
            {"t": 1.0, "level": "log"},
        ]);
        let resp = post_json(&body.to_string()).await;
        assert!(
            resp.status().is_client_error(),
            "expected 4xx, got {}",
            resp.status()
        );
    }

    /// Sanity that we can read the response body — used in case future
    /// changes start returning a JSON ack and a test wants to inspect it.
    /// Today the body is empty for 204.
    #[tokio::test]
    async fn no_content_response_body_is_empty() {
        let resp = post_json("[]").await;
        let bytes = resp.into_body().collect().await.unwrap().to_bytes();
        assert!(bytes.is_empty());
    }
}

#[cfg(test)]
mod loading_guard_tests {
    //! `LoadingGuard` is the RAII handle that protects against the TV
    //! sitting on a stale "LOADING ..." surface after a boot failure.
    //! These tests pin the two semantics that have already broken in
    //! live testing:
    //!
    //!   1. Armed guard clears `loading_game` on drop (any error-return
    //!      path through `launch_game` triggers this).
    //!   2. Disarmed guard preserves `loading_game` (success path; the
    //!      dispatcher clears it on in-game transition instead).
    //!
    //! The bug that motivated #2: pre-disarm, the QR card would flash
    //! back the instant `launch_game` returned, even though the game
    //! was still in the multi-minute first-run shader compile.
    use super::*;
    use crate::state::LauncherStatus;

    fn status_with_loading() -> Arc<std::sync::Mutex<LauncherStatus>> {
        Arc::new(std::sync::Mutex::new(LauncherStatus {
            loading_game: Some("Spyro's Adventure".into()),
            ..Default::default()
        }))
    }

    #[test]
    fn armed_guard_clears_loading_game_on_drop() {
        let status = status_with_loading();
        {
            let _g = LoadingGuard::armed(&status);
        }
        assert!(
            status.lock().unwrap().loading_game.is_none(),
            "armed guard must scrub loading_game so the TV doesn't sit on a stale LOADING surface",
        );
    }

    #[test]
    fn disarmed_guard_preserves_loading_game() {
        // The success-path contract: the launcher dispatcher owns
        // clearing loading_game on the in-game transition, NOT this
        // handler. Without disarm, the QR card flashes back the
        // instant `launch_game` returns.
        let status = status_with_loading();
        {
            let mut g = LoadingGuard::armed(&status);
            g.disarm();
        }
        assert_eq!(
            status.lock().unwrap().loading_game.as_deref(),
            Some("Spyro's Adventure"),
            "disarmed guard must leave loading_game intact through compile",
        );
    }

    #[test]
    fn arming_does_not_touch_loading_game_until_drop() {
        // The handler writes `loading_game = Some(...)` BEFORE arming
        // the guard. Arming itself must not mutate the status — only
        // the eventual drop does.
        let status = status_with_loading();
        let _g = LoadingGuard::armed(&status);
        assert_eq!(
            status.lock().unwrap().loading_game.as_deref(),
            Some("Spyro's Adventure"),
        );
    }
}

#[cfg(test)]
mod game_image_tests {
    //! `GET /api/games/:serial/image` — serves scraped box-art PNGs to the
    //! phone game picker. Pinned contracts:
    //!
    //!   1. Well-formed serial with PNG present → 200 + image/png body.
    //!   2. Well-formed serial with no file → 404 (polish asset; no fallback).
    //!   3. Malformed serial (bad prefix, wrong length, non-digits, path
    //!      traversal attempt) → 400 and the filesystem is never touched.
    //!
    //! The filesystem-touch guard matters: `AxumPath` already decodes %-encoded
    //! input, so a serial of `"..%2F..%2Fetc%2Fpasswd"` would arrive as
    //! `"../../etc/passwd"` without the strict regex check below.

    use super::*;
    use axum::Router;
    use axum::body::Body;
    use axum::http::Request;
    use axum::routing::get;
    use http_body_util::BodyExt;
    use tempfile::TempDir;
    use tower::ServiceExt;

    /// Build a tiny router that wraps `serve_game_image` against a
    /// temp-dir data root — avoids constructing a full `AppState`.
    fn router(data_root: Arc<std::path::PathBuf>) -> Router {
        Router::new().route(
            "/api/games/:serial/image",
            get(move |AxumPath(serial): AxumPath<String>| {
                let root = data_root.clone();
                async move { serve_game_image(&root, &serial).await }
            }),
        )
    }

    async fn get_image(
        data_root: Arc<std::path::PathBuf>,
        serial: &str,
    ) -> axum::http::Response<Body> {
        router(data_root)
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri(format!("/api/games/{serial}/image"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap()
    }

    #[test]
    fn serial_validator_accepts_all_four_region_prefixes() {
        assert!(is_valid_ps3_serial("BLUS30906"));
        assert!(is_valid_ps3_serial("BLES30906"));
        assert!(is_valid_ps3_serial("BCUS30906"));
        assert!(is_valid_ps3_serial("BCES30906"));
    }

    #[test]
    fn serial_validator_rejects_malformed_inputs() {
        // Wrong length / missing digits.
        assert!(!is_valid_ps3_serial(""));
        assert!(!is_valid_ps3_serial("BLUS"));
        assert!(!is_valid_ps3_serial("BLUS3090"));
        assert!(!is_valid_ps3_serial("BLUS309066"));
        // Unknown prefix.
        assert!(!is_valid_ps3_serial("XXXX30906"));
        // Lowercase prefix.
        assert!(!is_valid_ps3_serial("blus30906"));
        // Non-digit tail.
        assert!(!is_valid_ps3_serial("BLUS3090a"));
        // Path traversal / separators.
        assert!(!is_valid_ps3_serial("../etc/passwd"));
        assert!(!is_valid_ps3_serial("BLUS/0906"));
    }

    #[tokio::test]
    async fn well_formed_serial_with_file_returns_200_png() {
        let tmp = TempDir::new().unwrap();
        std::fs::create_dir_all(tmp.path().join("games")).unwrap();
        let png_path = tmp.path().join("games").join("BLUS30906.png");
        // Minimal PNG header — the handler just reads the bytes and sets
        // the content-type; it never decodes. 8-byte signature is enough.
        std::fs::write(&png_path, b"\x89PNG\r\n\x1a\n").unwrap();

        let root = Arc::new(tmp.path().to_path_buf());
        let resp = get_image(root, "BLUS30906").await;
        assert_eq!(resp.status(), StatusCode::OK);
        assert_eq!(
            resp.headers()
                .get(axum::http::header::CONTENT_TYPE)
                .and_then(|v| v.to_str().ok()),
            Some("image/png"),
        );
        let body = resp.into_body().collect().await.unwrap().to_bytes();
        assert_eq!(&body[..], b"\x89PNG\r\n\x1a\n");
    }

    #[tokio::test]
    async fn well_formed_serial_with_missing_file_returns_404() {
        let tmp = TempDir::new().unwrap();
        std::fs::create_dir_all(tmp.path().join("games")).unwrap();
        let root = Arc::new(tmp.path().to_path_buf());
        let resp = get_image(root, "BLES99999").await;
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
        let body = resp.into_body().collect().await.unwrap().to_bytes();
        assert!(body.is_empty(), "404 body should be empty");
    }

    #[tokio::test]
    async fn malformed_serial_returns_400() {
        let tmp = TempDir::new().unwrap();
        let root = Arc::new(tmp.path().to_path_buf());
        let resp = get_image(root.clone(), "XXXX30906").await;
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
        // Wrong length.
        let resp = get_image(root.clone(), "BLUS1").await;
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    }
}
