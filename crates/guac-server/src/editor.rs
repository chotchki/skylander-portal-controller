//! guacamole's figure editor on the rows (openspec AH.3, design decision 10):
//! its arms of the stats, edit and reset routes and of the slot reset, keyed
//! on the `FigureFiles` hook's absence as AA's Native table does. The product
//! keeps its `.sky` handlers behind the hook.
//!
//! - Stats read the profile's row: the level by the extract's table (AH.2),
//!   with the game's caps, so the phone caps its steppers at the game's own
//!   numbers. A figure the profile never placed is a 404 carrying the caps;
//!   before the extract holds the table, a 409.
//! - An edit sets experience to the level's threshold and the gold within the
//!   clamp, under a new generation (so nothing the TV sent before it lands on
//!   top), making the row for a never-placed figure.
//! - A reset drops the row, so the next placement is fresh.
//! - The slot reset makes the slot's figure fresh where it stands: its row
//!   made again under a new generation, then placed again, which tells the TV.
//!   Refused for the hero in a level.
//!
//! Edits and resets answer 403 unless the path's profile is the one the
//! requesting session has unlocked: the routes sit behind the pairing key,
//! which every phone holds, and PINs are anti-sibling. Each holds the portal's
//! lock from its on-the-portal check to its write, so a placement can't slip
//! between them and carry the row from before.

use std::sync::Arc;

use axum::Router;
use axum::extract::{Path as AxumPath, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use serde::{Deserialize, Serialize};
use skylander_core::{Category, Event, FigureId, SlotIndex, SlotState};
use tracing::{info, warn};

use crate::http::{CurrentSession, Signed};
use crate::progress::Progression;
use crate::state::{AppState, DriverJob};

/// The three figure routes, guacamole's arm.
pub(crate) fn routes() -> Router<Arc<AppState>> {
    Router::new()
        .route(
            "/api/profiles/:profile_id/figures/:figure_id/stats",
            get(stats),
        )
        .route(
            "/api/profiles/:profile_id/figures/:figure_id/edit",
            post(edit),
        )
        .route(
            "/api/profiles/:profile_id/figures/:figure_id/reset",
            post(reset),
        )
}

/// The game's caps, off its disc: what the phone's steppers stop at.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct GameCaps {
    pub max_level: u8,
    pub max_gold: u32,
}

impl GameCaps {
    fn of(progression: &Progression) -> Self {
        Self {
            max_level: progression.max_level(),
            max_gold: progression.gold_cap,
        }
    }
}

/// A profile's progress for a figure, as the phone's details show it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FigureStats {
    pub level: u8,
    /// The total.
    pub xp: u32,
    pub gold: u32,
    pub playtime_secs: u32,
    #[serde(flatten)]
    pub caps: GameCaps,
}

/// `GET .../stats`: the row's numbers, a 404 with the caps for a figure the
/// profile never placed, a 409 before the extract holds the table.
async fn stats(
    State(state): State<Arc<AppState>>,
    AxumPath((profile_id, figure_id)): AxumPath<(String, String)>,
) -> Response {
    let Some(progression) = state.progression.get() else {
        return (
            StatusCode::CONFLICT,
            "the game's progression isn't read yet",
        )
            .into_response();
    };
    let caps = GameCaps::of(&progression);
    match state
        .profiles
        .instances()
        .read(&profile_id, &figure_id)
        .await
    {
        Ok(Some(row)) => {
            let p = row.progress;
            axum::Json(FigureStats {
                level: progression.level_for(p.xp),
                xp: p.xp,
                gold: p.gold,
                playtime_secs: p.playtime_secs,
                caps,
            })
            .into_response()
        }
        Ok(None) => (StatusCode::NOT_FOUND, axum::Json(caps)).into_response(),
        Err(e) => {
            warn!(profile_id, figure_id, "read the figure's row: {e:#}");
            (StatusCode::INTERNAL_SERVER_ERROR, "read failed").into_response()
        }
    }
}

#[derive(Debug, Deserialize)]
struct EditBody {
    level: u8,
    gold: u32,
}

/// `POST .../edit` with `{level, gold}`: experience to the level's threshold,
/// gold as asked, a new generation; 202.
async fn edit(
    State(state): State<Arc<AppState>>,
    AxumPath((profile_id, figure_id)): AxumPath<(String, String)>,
    CurrentSession(sid): CurrentSession,
    Signed(body): Signed,
) -> Response {
    let body: EditBody = match serde_json::from_slice(&body) {
        Ok(b) => b,
        Err(e) => return (StatusCode::BAD_REQUEST, format!("bad edit body: {e}")).into_response(),
    };
    if let Some(refused) = unless_own_profile(&state, sid, &profile_id).await {
        return refused;
    }
    let figure_id = FigureId::new(figure_id);
    if let Some(refused) = unless_editable(&state, &figure_id) {
        return refused;
    }
    let Some(progression) = state.progression.get() else {
        return (
            StatusCode::CONFLICT,
            "the game's progression isn't read yet",
        )
            .into_response();
    };
    let Some(xp) = progression.threshold(body.level) else {
        let max = progression.max_level();
        return (
            StatusCode::UNPROCESSABLE_ENTITY,
            format!("level must be in 1..={max}"),
        )
            .into_response();
    };
    if body.gold > progression.gold_cap {
        let cap = progression.gold_cap;
        return (
            StatusCode::UNPROCESSABLE_ENTITY,
            format!("gold must be in 0..={cap}"),
        )
            .into_response();
    }

    let portal = state.portal.lock().await;
    if holds_own(&portal[..], &figure_id, &profile_id) {
        return (StatusCode::CONFLICT, "remove from portal before editing").into_response();
    }
    let set = state.profiles.instances().set(
        &profile_id,
        figure_id.as_str(),
        xp,
        body.gold,
        progression.caps(),
    );
    let row = match set.await {
        Ok(row) => row,
        Err(e) => {
            warn!(profile_id, %figure_id, "edit the figure's row: {e:#}");
            return (StatusCode::INTERNAL_SERVER_ERROR, "edit failed").into_response();
        }
    };
    drop(portal);
    info!(
        profile_id,
        %figure_id,
        level = body.level,
        xp,
        gold = body.gold,
        generation = row.progress.generation,
        "edited the figure's row",
    );
    let _ = state.events.send(Event::FigureUpdated {
        figure_id,
        level: body.level,
        gold: gold_u16(row.progress.gold),
    });
    StatusCode::ACCEPTED.into_response()
}

/// `POST .../reset`: the row dropped, so the next placement is fresh; 202
/// whether there was one or not.
async fn reset(
    State(state): State<Arc<AppState>>,
    AxumPath((profile_id, figure_id)): AxumPath<(String, String)>,
    CurrentSession(sid): CurrentSession,
    Signed(_body): Signed,
) -> Response {
    if let Some(refused) = unless_own_profile(&state, sid, &profile_id).await {
        return refused;
    }
    let figure_id = FigureId::new(figure_id);
    if let Some(refused) = unless_editable(&state, &figure_id) {
        return refused;
    }
    let portal = state.portal.lock().await;
    if holds_own(&portal[..], &figure_id, &profile_id) {
        return (StatusCode::CONFLICT, "remove from portal before resetting").into_response();
    }
    let dropped = match state
        .profiles
        .instances()
        .delete(&profile_id, figure_id.as_str())
        .await
    {
        Ok(dropped) => dropped,
        Err(e) => {
            warn!(profile_id, %figure_id, "drop the figure's row: {e:#}");
            return (StatusCode::INTERNAL_SERVER_ERROR, "reset failed").into_response();
        }
    };
    drop(portal);
    info!(profile_id, %figure_id, dropped, "reset the figure to fresh");
    let _ = state.events.send(Event::FigureUpdated {
        figure_id,
        level: 1,
        gold: 0,
    });
    StatusCode::ACCEPTED.into_response()
}

#[derive(Debug, Deserialize)]
struct SlotResetBody {
    figure_id: FigureId,
}

/// `POST /api/portal/slot/:n/reset` with `{figure_id}`, guacamole's arm: the
/// slot's figure fresh where it stands. 403 unless the slot holds the
/// session's own placement of it, 404 when it doesn't hold the figure at all,
/// 409 while it's the hero in a level, 429 mid-load; 202 once the placement is
/// queued.
pub(crate) async fn reset_slot(
    state: &Arc<AppState>,
    n: u8,
    sid: crate::profiles::SessionId,
    body: &[u8],
) -> Response {
    let Ok(slot) = SlotIndex::from_display(n) else {
        return (StatusCode::BAD_REQUEST, format!("slot {n} out of range")).into_response();
    };
    let body: SlotResetBody = match serde_json::from_slice(body) {
        Ok(b) => b,
        Err(e) => return (StatusCode::BAD_REQUEST, format!("bad reset body: {e}")).into_response(),
    };
    let Some(figure) = state.lookup_figure(&body.figure_id) else {
        return (StatusCode::NOT_FOUND, "unknown figure_id").into_response();
    };
    let Some(profile_id) = state.sessions.profile_of(sid).await else {
        return (StatusCode::FORBIDDEN, "unlock a profile first").into_response();
    };

    let mut portal = state.portal.lock().await;
    match &portal[slot.as_usize()] {
        SlotState::Loading { .. } => {
            return (StatusCode::TOO_MANY_REQUESTS, "slot busy").into_response();
        }
        SlotState::Loaded {
            figure_id: Some(id),
            placed_by,
            ..
        } if *id == figure.id => {
            if placed_by.as_deref() != Some(profile_id.as_str()) {
                return (
                    StatusCode::FORBIDDEN,
                    format!("slot {n} holds another player's figure"),
                )
                    .into_response();
            }
        }
        _ => {
            return (
                StatusCode::NOT_FOUND,
                format!("slot {n} doesn't hold that figure"),
            )
                .into_response();
        }
    }
    if state.level_hero.get() == Some(slot) {
        info!(slot = n, figure_id = %figure.id, "slot reset refused: the hero in a level");
        return (
            StatusCode::CONFLICT,
            format!("{} is the hero in the level", figure.canonical_name),
        )
            .into_response();
    }
    let instances = state.profiles.instances();
    let fresh = async {
        instances.delete(&profile_id, figure.id.as_str()).await?;
        instances.fresh(&profile_id, figure.id.as_str()).await
    };
    let row = match fresh.await {
        Ok(row) => row,
        Err(e) => {
            warn!(profile_id, figure_id = %figure.id, "make the figure's row fresh: {e:#}");
            return (StatusCode::INTERNAL_SERVER_ERROR, "reset failed").into_response();
        }
    };
    let loading = SlotState::Loading {
        figure_id: Some(figure.id.clone()),
        placed_by: Some(profile_id.clone()),
    };
    portal[slot.as_usize()] = loading.clone();
    drop(portal);
    info!(
        slot = n,
        profile_id,
        figure_id = %figure.id,
        generation = row.progress.generation,
        "slot reset: the figure fresh where it stands",
    );
    let _ = state.events.send(Event::SlotChanged {
        slot,
        state: loading,
    });
    let _ = state.events.send(Event::FigureUpdated {
        figure_id: figure.id.clone(),
        level: 1,
        gold: 0,
    });
    // The worker places it again with the fresh row; the native driver tells
    // the TV the same figure came back under a new generation.
    let job = DriverJob::LoadFigure {
        slot,
        figure_id: figure.id.clone(),
        path: None,
        placed_by: Some(profile_id),
        canonical_name: figure.canonical_name.clone(),
    };
    if state.driver_tx.send(job).await.is_err() {
        return (StatusCode::SERVICE_UNAVAILABLE, "driver channel closed").into_response();
    }
    (StatusCode::ACCEPTED, "queued").into_response()
}

/// The 403 unless `profile_id` is what session `sid` has unlocked.
async fn unless_own_profile(
    state: &AppState,
    sid: crate::profiles::SessionId,
    profile_id: &str,
) -> Option<Response> {
    match state.sessions.profile_of(sid).await {
        Some(unlocked) if unlocked == profile_id => None,
        unlocked => {
            info!(
                profile_id,
                unlocked = unlocked.as_deref().unwrap_or("none"),
                "figure edit refused: not the session's profile",
            );
            Some(
                (
                    StatusCode::FORBIDDEN,
                    "that figure belongs to a profile this phone hasn't unlocked",
                )
                    .into_response(),
            )
        }
    }
}

/// The 404 off the list, the 422 for a kind with no level or gold.
fn unless_editable(state: &AppState, figure_id: &FigureId) -> Option<Response> {
    let Some(figure) = state.lookup_figure(figure_id) else {
        return Some((StatusCode::NOT_FOUND, "figure not in the list").into_response());
    };
    match figure.category {
        Category::Figure | Category::Sidekick | Category::Giant | Category::Kaos => None,
        other => Some(
            (
                StatusCode::UNPROCESSABLE_ENTITY,
                format!("editing not supported for {other:?}"),
            )
                .into_response(),
        ),
    }
}

/// Whether `profile_id`'s own placement of the figure is on the portal,
/// loaded or loading (AA.18's rule: another profile's copy doesn't count).
fn holds_own(portal: &[SlotState], figure_id: &FigureId, profile_id: &str) -> bool {
    portal
        .iter()
        .any(|slot| is_own_placement(slot, figure_id, profile_id))
}

/// The figure's instance for `profile_id` stands in `slot`. A load still in
/// flight counts: the worker places it next, carrying the row as it reads now.
pub(crate) fn is_own_placement(slot: &SlotState, figure_id: &FigureId, profile_id: &str) -> bool {
    match slot {
        SlotState::Loaded {
            figure_id: Some(id),
            placed_by,
            ..
        }
        | SlotState::Loading {
            figure_id: Some(id),
            placed_by,
        } => id == figure_id && placed_by.as_deref() == Some(profile_id),
        _ => false,
    }
}

/// `FigureUpdated` carries gold as the product's `u16`; every game's clamp
/// the disc has given so far fits.
fn gold_u16(gold: u32) -> u16 {
    u16::try_from(gold).unwrap_or(u16::MAX)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn loaded(figure: &str, by: Option<&str>) -> SlotState {
        SlotState::Loaded {
            figure_id: Some(FigureId::new(figure)),
            display_name: figure.into(),
            placed_by: by.map(Into::into),
        }
    }

    #[test]
    fn only_the_profiles_own_placement_holds_the_figure() {
        let eruptor = FigureId::new("000009-0000");
        let portal = [
            loaded("000009-0000", Some("bob")),
            SlotState::Loading {
                figure_id: Some(FigureId::new("000070-0000")),
                placed_by: Some("alice".into()),
            },
            loaded("000009-0000", None),
            SlotState::Empty,
        ];
        assert!(!holds_own(&portal, &eruptor, "alice"));
        assert!(holds_own(&portal, &eruptor, "bob"));
        let tree_rex = FigureId::new("000070-0000");
        assert!(holds_own(&portal, &tree_rex, "alice"), "still loading");
        assert!(!holds_own(&portal, &tree_rex, "bob"));
    }

    #[test]
    fn the_stats_carry_the_caps_flat() {
        let stats = FigureStats {
            level: 10,
            xp: 33_000,
            gold: 1_234,
            playtime_secs: 61,
            caps: GameCaps {
                max_level: 15,
                max_gold: 65_000,
            },
        };
        let json = serde_json::to_value(&stats).unwrap();
        assert_eq!(
            json,
            serde_json::json!({
                "level": 10, "xp": 33000, "gold": 1234, "playtime_secs": 61,
                "max_level": 15, "max_gold": 65000,
            })
        );
        assert_eq!(gold_u16(65_000), 65_000);
        assert_eq!(gold_u16(70_000), u16::MAX);
    }
}
