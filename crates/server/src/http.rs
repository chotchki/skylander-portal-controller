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
use skylander_core::{Event, FigureId, GameLaunched, GameSerial, PublicFigure, SlotIndex, SlotState, SLOT_COUNT};
use skylander_rpcs3_control::RpcsProcess;
use tokio::sync::broadcast;
use tower_http::services::ServeDir;
use tracing::{debug, info, warn};

use crate::games::InstalledGame;
use crate::state::{AppState, DriverJob};

pub fn router(state: Arc<AppState>, phone_dist: std::path::PathBuf) -> Router {
    #[allow(unused_mut)]
    let mut api = Router::new()
        .route("/api/figures", get(list_figures))
        .route("/api/portal", get(get_portal))
        .route("/api/portal/slot/:n/load", post(load_slot))
        .route("/api/portal/slot/:n/clear", post(clear_slot))
        .route("/api/portal/refresh", post(refresh_portal))
        .route("/api/games", get(list_games))
        .route("/api/status", get(get_status))
        .route("/api/launch", post(launch_game))
        .route("/api/quit", post(quit_game))
        .route("/ws", get(ws_handler));

    #[cfg(feature = "test-hooks")]
    {
        api = api
            .route("/api/_test/inject_load", post(inject_load))
            .route("/api/_test/set_game", post(set_game_state));
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

    // Back pressure: atomically flip the slot to Loading, rejecting if it's
    // already in-flight. Avoids queueing a second load that would open a
    // duplicate file dialog on top of the first one still in progress.
    let loading = SlotState::Loading {
        figure_id: Some(figure_id.clone()),
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

    let loading = SlotState::Loading { figure_id: None };
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

    // Send the initial snapshot.
    {
        let snap: [SlotState; SLOT_COUNT] = state.portal.lock().await.clone();
        let evt = Event::PortalSnapshot { slots: snap };
        if let Ok(j) = serde_json::to_string(&evt) {
            let _ = sender.send(Message::Text(j)).await;
        }
    }

    let mut rx: broadcast::Receiver<Event> = state.events.subscribe();

    // Writer task — forward broadcast events to the socket.
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

    state
        .connected_clients
        .fetch_sub(1, std::sync::atomic::Ordering::Relaxed);
}

