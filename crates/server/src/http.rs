//! HTTP + WebSocket routes.

use std::sync::Arc;

use axum::Router;
use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::{Path as AxumPath, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use futures_util::{SinkExt, StreamExt};
use serde::Deserialize;
use skylander_core::{Event, FigureId, PublicFigure, SlotIndex, SlotState, SLOT_COUNT};
use tokio::sync::broadcast;
use tower_http::services::ServeDir;
use tracing::{debug, warn};

use crate::state::{AppState, DriverJob};

pub fn router(state: Arc<AppState>, phone_dist: std::path::PathBuf) -> Router {
    let api = Router::new()
        .route("/api/figures", get(list_figures))
        .route("/api/portal", get(get_portal))
        .route("/api/portal/slot/:n/load", post(load_slot))
        .route("/api/portal/slot/:n/clear", post(clear_slot))
        .route("/api/portal/refresh", post(refresh_portal))
        .route("/ws", get(ws_handler));

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

