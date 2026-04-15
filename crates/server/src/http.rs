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
use skylander_core::{Event, FigureId, GameLaunched, GameSerial, PublicFigure, SlotIndex, SlotState, UnlockedProfile, SLOT_COUNT};
use skylander_rpcs3_control::RpcsProcess;
use tokio::sync::broadcast;
use tower_http::services::ServeDir;
use tracing::{debug, info, warn};

use crate::games::InstalledGame;
use crate::profiles::{LockoutCheck, PublicProfile, RegistrationOutcome, SessionId, MAX_PROFILES};
use crate::state::{AppState, DriverJob};

/// Axum extractor that pulls the caller's session id from the `X-Session-Id`
/// request header. The phone receives its session id in the initial
/// `Welcome` WS event and attaches it to mutating REST calls so the server
/// can route per-session state correctly.
///
/// The inner `Option` is `None` when the header is absent — pre-3.10d the
/// phone doesn't send it yet, so handlers fall back to "most-recent
/// session". Once the phone is wired, the Option will be removed and
/// callers will reject requests without the header.
pub struct MaybeSession(pub Option<SessionId>);

#[async_trait::async_trait]
impl<S> axum::extract::FromRequestParts<S> for MaybeSession
where
    S: Send + Sync,
{
    type Rejection = Response;

    async fn from_request_parts(
        parts: &mut axum::http::request::Parts,
        _state: &S,
    ) -> Result<Self, Self::Rejection> {
        let Some(raw) = parts.headers.get("x-session-id") else {
            return Ok(MaybeSession(None));
        };
        let s = raw
            .to_str()
            .map_err(|_| (StatusCode::BAD_REQUEST, "X-Session-Id not ASCII").into_response())?;
        let id = s
            .parse::<u64>()
            .map_err(|_| (StatusCode::BAD_REQUEST, "X-Session-Id not a u64").into_response())?;
        Ok(MaybeSession(Some(SessionId(id))))
    }
}

/// Resolve the caller's `SessionId`: use the header-supplied one if
/// present, else the most-recently-registered session. Returns `None`
/// only when no sessions are registered at all (no WS connection yet).
async fn resolve_session(hdr: MaybeSession, state: &AppState) -> Option<SessionId> {
    if let Some(sid) = hdr.0 {
        return Some(sid);
    }
    state.sessions.all_ids().await.into_iter().max_by_key(|s| s.0)
}

pub fn router(state: Arc<AppState>, phone_dist: std::path::PathBuf) -> Router {
    #[allow(unused_mut)]
    let mut api = Router::new()
        .route("/api/figures", get(list_figures))
        .route("/api/figures/:id/image", get(figure_image))
        .route("/api/portal", get(get_portal))
        .route("/api/portal/slot/:n/load", post(load_slot))
        .route("/api/portal/slot/:n/clear", post(clear_slot))
        .route("/api/portal/refresh", post(refresh_portal))
        .route("/api/games", get(list_games))
        .route("/api/status", get(get_status))
        .route("/api/launch", post(launch_game))
        .route("/api/quit", post(quit_game))
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

    #[cfg(feature = "test-hooks")]
    {
        api = api
            .route("/api/_test/inject_load", post(inject_load))
            .route("/api/_test/set_game", post(set_game_state))
            .route("/api/_test/inject_profile", post(inject_profile))
            .route("/api/_test/unlock_session", post(unlock_session_testhook));
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

    api.fallback_service(static_dir).with_state(state)
}

async fn list_figures(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    let figs: Vec<PublicFigure> = state.figures.iter().map(|f| f.to_public()).collect();
    axum::Json(figs)
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
            return (StatusCode::BAD_REQUEST, format!("unknown size: {other}"))
                .into_response();
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
    if let Some(fig) = state.lookup_figure(&FigureId::new(&id)) {
        if let Some(icon_path) = &fig.element_icon_path {
            if let Ok(bytes) = tokio::fs::read(icon_path).await {
                return image_response(bytes);
            }
        }
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
    hdr: MaybeSession,
    axum::Json(body): axum::Json<LoadBody>,
) -> Response {
    let slot = match SlotIndex::from_display(n) {
        Ok(s) => s,
        Err(_) => {
            return (StatusCode::BAD_REQUEST, format!("slot {n} out of range")).into_response()
        }
    };
    let figure = match state.lookup_figure(&body.figure_id) {
        Some(f) => f,
        None => return (StatusCode::NOT_FOUND, "unknown figure_id").into_response(),
    };
    let figure_id = figure.id.clone();
    let path = figure.sky_path.clone();

    // Who placed this figure? Pulled from the caller's WS session's
    // currently-unlocked profile (via the X-Session-Id header, falling back
    // to the most-recent session pre-3.10d). Threaded through Loading and
    // Loaded so the phone can render a per-slot ownership badge.
    let placed_by = match resolve_session(hdr, &state).await {
        Some(sid) => state.sessions.profile_of(sid).await,
        None => None,
    };

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
    };
    if state.driver_tx.send(job).await.is_err() {
        return (StatusCode::SERVICE_UNAVAILABLE, "driver channel closed").into_response();
    }
    (StatusCode::ACCEPTED, "queued").into_response()
}

async fn clear_slot(
    State(state): State<Arc<AppState>>,
    AxumPath(n): AxumPath<u8>,
) -> Response {
    let slot = match SlotIndex::from_display(n) {
        Ok(s) => s,
        Err(_) => {
            return (StatusCode::BAD_REQUEST, format!("slot {n} out of range")).into_response()
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

    if state.driver_tx.send(DriverJob::ClearSlot { slot }).await.is_err() {
        return (StatusCode::SERVICE_UNAVAILABLE, "driver channel closed").into_response();
    }
    (StatusCode::ACCEPTED, "queued").into_response()
}

async fn refresh_portal(State(state): State<Arc<AppState>>) -> Response {
    if state.driver_tx.send(DriverJob::RefreshPortal).await.is_err() {
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
    current_game: Option<GameLaunched>,
}

async fn get_status(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    let rpcs3 = state.rpcs3.lock().await;
    axum::Json(StatusBody {
        current_game: rpcs3.current.clone(),
    })
}

#[derive(Deserialize)]
struct LaunchBody {
    serial: GameSerial,
}

async fn launch_game(
    State(state): State<Arc<AppState>>,
    axum::Json(body): axum::Json<LaunchBody>,
) -> Response {
    let game = match state.lookup_game(&body.serial) {
        Some(g) => g.clone(),
        None => return (StatusCode::NOT_FOUND, "unknown serial").into_response(),
    };

    // Hold the rpcs3 lock across the whole launch so we can't race.
    let mut guard = state.rpcs3.lock().await;
    if guard.process.is_some() {
        return (
            StatusCode::CONFLICT,
            "another game is already running; quit it first",
        )
            .into_response();
    }

    let exe = state.rpcs3_exe.clone();
    let eboot = game.eboot_path();
    info!(
        serial = %body.serial,
        display_name = %game.display_name,
        eboot = %eboot.display(),
        "launching game",
    );

    let launch = tokio::task::spawn_blocking(move || -> anyhow::Result<RpcsProcess> {
        let mut proc = RpcsProcess::launch(&exe, &eboot)?;
        proc.wait_ready(Duration::from_secs(45))?;
        Ok(proc)
    })
    .await;

    let proc = match launch {
        Ok(Ok(p)) => p,
        Ok(Err(e)) => {
            return (StatusCode::INTERNAL_SERVER_ERROR, format!("launch failed: {e}"))
                .into_response();
        }
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("launch task panicked: {e}"),
            )
                .into_response();
        }
    };

    let launched = GameLaunched {
        serial: game.serial.clone(),
        display_name: game.display_name.clone(),
    };
    guard.process = Some(proc);
    guard.current = Some(launched.clone());

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
) -> Response {
    let mut guard = state.rpcs3.lock().await;
    let mut proc = match guard.process.take() {
        Some(p) => p,
        None => return (StatusCode::CONFLICT, "no game is running").into_response(),
    };

    // Reset current immediately — the quit is committed even if the process
    // takes time to actually die.
    guard.current = None;
    drop(guard);

    let timeout = if q.force {
        Duration::from_millis(500)
    } else {
        Duration::from_secs(30)
    };

    let result =
        tokio::task::spawn_blocking(move || proc.shutdown_graceful(timeout)).await;

    match result {
        Ok(Ok(path)) => info!(?path, "game shutdown finished"),
        Ok(Err(e)) => warn!("game shutdown errored: {e}"),
        Err(e) => warn!("shutdown task panicked: {e}"),
    }

    // Reset the portal snapshot since the emulator is gone.
    *state.portal.lock().await = std::array::from_fn(|_| SlotState::Empty);
    let _ = state.events.send(Event::PortalSnapshot {
        slots: std::array::from_fn(|_| SlotState::Empty),
    });
    let _ = state.events.send(Event::GameChanged { current: None });

    (StatusCode::ACCEPTED, "quit").into_response()
}

async fn ws_handler(
    ws: WebSocketUpgrade,
    State(state): State<Arc<AppState>>,
) -> impl IntoResponse {
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
    //     the Chaos screen.
    //   - RejectedByCooldown: both seats full and the 1-min forced-evict
    //     cooldown hasn't elapsed; send an Error event and close.
    let sid = match state.sessions.register().await {
        RegistrationOutcome::Admitted(sid) => sid,
        RegistrationOutcome::AdmittedByEvicting { session, evicted } => {
            let _ = state.events.send(Event::TakenOver {
                session_id: evicted.0,
                by_chaos: "Kaos".to_string(),
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

    // First message: tell the client its session id so it can attach it on
    // REST and filter targeted broadcast events for itself.
    {
        let evt = Event::Welcome { session_id: sid.0 };
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
        let unlocked = match current_profile {
            Some(pid) => match state.profiles.get(&pid).await {
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
    axum::Json(body): axum::Json<CreateProfileBody>,
) -> Response {
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
    axum::Json(body): axum::Json<PinBody>,
) -> Response {
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
    hdr: MaybeSession,
    axum::Json(body): axum::Json<PinBody>,
) -> Response {
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

    let Some(sid) = resolve_session(hdr, &state).await else {
        return (
            StatusCode::BAD_REQUEST,
            "no active session — connect WS first",
        )
            .into_response();
    };

    // Bind the unlocked profile to this specific WS session. Other phones
    // stay independent.
    state.sessions.set_profile(sid, Some(id.clone())).await;

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

    (StatusCode::OK, axum::Json(unlocked)).into_response()
}

async fn lock_profile(
    State(state): State<Arc<AppState>>,
    AxumPath(_id): AxumPath<String>,
    hdr: MaybeSession,
) -> Response {
    let Some(sid) = resolve_session(hdr, &state).await else {
        return (
            StatusCode::BAD_REQUEST,
            "no active session — connect WS first",
        )
            .into_response();
    };
    state.sessions.set_profile(sid, None).await;
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
    axum::Json(body): axum::Json<ResetPinBody>,
) -> Response {
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
    // connected — covers the "test sets up after Phone::new" flow.
    if let Some(&sid) = state.sessions.all_ids().await.iter().max_by_key(|s| s.0) {
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
    }
    (StatusCode::OK, "unlocked").into_response()
}

