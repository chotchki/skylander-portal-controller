//! Per-figure level + gold edit endpoint (PLAN 11).
//!
//! Feature-gated behind `sky-stats` — the read-side stats endpoint is the
//! natural companion. Both ride on the same `skylander-sky-parser` dep.
//!
//! Validation chain (all return 4xx on failure with toast-ready messages):
//! - figure exists in catalog → 404 if not
//! - `Category` is one of `Figure / Sidekick / Giant / Kaos` → 422 otherwise
//! - figure NOT currently in any portal slot → 409 otherwise
//! - level within `1..=max_level_for(figure.generation)` → 422 otherwise
//!
//! Mutation pipeline: resolve working copy (forks on first edit) → read →
//! decrypt → `set_gold` + `set_xp` → encrypt → atomic write (tmp + rename) →
//! broadcast `Event::FigureUpdated`. Returns 202 on success.

use std::sync::Arc;

use axum::extract::{Path as AxumPath, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use serde::Deserialize;
use skylander_core::figure::{Category, FigureId, GameOfOrigin};
use skylander_core::portal::SlotState;
use skylander_core::protocol::Event;
use skylander_sky_parser::{
    SKY_FILE_LEN, SkyGeneration, decrypt_figure, distribute_xp,
    encrypt_figure_preserving_unwritten, max_level_for, set_gold, set_xp, xp_for_level,
};
use tracing::{info, warn};

use crate::state::AppState;
use crate::working_copies;

/// POST body for the edit endpoint. Both fields are required.
#[derive(Debug, Deserialize)]
pub struct EditBody {
    /// Target level, clamped server-side to `1..=max_level_for(generation)`
    /// where `generation` is derived from the figure's game-of-origin.
    pub level: u8,
    /// Target gold value (always within `0..=u16::MAX` by virtue of the type).
    pub gold: u16,
}

/// `POST /api/profiles/:profile_id/figures/:figure_id/edit`.
pub async fn edit_figure(
    State(state): State<Arc<AppState>>,
    AxumPath((profile_id, figure_id)): AxumPath<(String, String)>,
    axum::Json(body): axum::Json<EditBody>,
) -> Response {
    let figure_id_key = FigureId(figure_id.clone());

    // 1. Look up figure in catalog.
    let figure = match state.figures.iter().find(|f| f.id == figure_id_key) {
        Some(f) => f.clone(),
        None => {
            return (StatusCode::NOT_FOUND, "figure not in catalog").into_response();
        }
    };

    // 2. Category check — only player-controllable kinds are editable.
    if !matches!(
        figure.category,
        Category::Figure | Category::Sidekick | Category::Giant | Category::Kaos
    ) {
        return (
            StatusCode::UNPROCESSABLE_ENTITY,
            format!("editing not supported for {:?}", figure.category),
        )
            .into_response();
    }

    // 3. Portal check — can't edit a figure that's currently on the portal.
    {
        let portal = state.portal.lock().await;
        for slot in portal.iter() {
            let slot_fig_id = match slot {
                SlotState::Loaded { figure_id, .. } | SlotState::Loading { figure_id, .. } => {
                    figure_id.as_ref()
                }
                _ => None,
            };
            if slot_fig_id == Some(&figure_id_key) {
                return (StatusCode::CONFLICT, "remove from portal before editing").into_response();
            }
        }
    }

    // 4. Level range validation (gold is always valid as u16).
    let generation = game_to_generation(figure.game);
    let max_level = max_level_for(generation);
    if body.level < 1 || body.level > max_level {
        return (
            StatusCode::UNPROCESSABLE_ENTITY,
            format!("level must be in 1..={max_level} for this figure"),
        )
            .into_response();
    }

    // 5. Resolve working copy path (forks from pack on first edit).
    let path = match working_copies::resolve_load_path(&profile_id, &figure) {
        Ok(p) => p,
        Err(e) => {
            warn!(error = ?e, profile = %profile_id, figure = %figure_id, "working copy resolve failed");
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                "working copy resolve failed",
            )
                .into_response();
        }
    };

    // 6. Read encrypted bytes.
    let raw = match tokio::fs::read(&path).await {
        Ok(b) => b,
        Err(e) => {
            warn!(path = %path.display(), error = %e, "read failed");
            return (StatusCode::INTERNAL_SERVER_ERROR, "read failed").into_response();
        }
    };
    if raw.len() != SKY_FILE_LEN {
        warn!(
            path = %path.display(),
            actual = raw.len(),
            expected = SKY_FILE_LEN,
            "working copy is wrong size"
        );
        return (
            StatusCode::UNPROCESSABLE_ENTITY,
            "working copy is wrong size",
        )
            .into_response();
    }
    // Keep the original ciphertext around so encrypt can preserve truly-
    // unwritten blocks (PLAN 11.11.1 — RPCS3 rejects edited figures if those
    // blocks aren't preserved at zero in the output).
    let mut source_cipher = [0u8; SKY_FILE_LEN];
    source_cipher.copy_from_slice(&raw);
    let mut bytes = source_cipher;

    // 7. Decrypt → mutate → encrypt.
    decrypt_figure(&mut bytes);
    let target_xp = xp_for_level(body.level);
    let slots = distribute_xp(target_xp, generation);
    set_gold(&mut bytes, body.gold);
    set_xp(&mut bytes, slots);
    encrypt_figure_preserving_unwritten(&mut bytes, &source_cipher);

    // 8. Atomic write (tmp file + rename).
    let tmp_path = path.with_extension("sky.tmp");
    if let Err(e) = tokio::fs::write(&tmp_path, &bytes).await {
        warn!(path = %tmp_path.display(), error = %e, "tmp write failed");
        return (StatusCode::INTERNAL_SERVER_ERROR, "write failed").into_response();
    }
    if let Err(e) = tokio::fs::rename(&tmp_path, &path).await {
        warn!(path = %path.display(), error = %e, "rename failed");
        // Best-effort cleanup of the tmp file.
        let _ = tokio::fs::remove_file(&tmp_path).await;
        return (StatusCode::INTERNAL_SERVER_ERROR, "rename failed").into_response();
    }

    info!(
        profile = %profile_id,
        figure = %figure_id,
        level = body.level,
        gold = body.gold,
        "edited working copy"
    );

    // 9. Broadcast — silent if no subscribers (e.g. no phones connected).
    let _ = state.events.send(Event::FigureUpdated {
        figure_id: figure_id_key,
        level: body.level,
        gold: body.gold,
    });

    StatusCode::ACCEPTED.into_response()
}

/// `POST /api/profiles/:profile_id/figures/:figure_id/reset` — reset a
/// figure's working copy to pack-fresh bytes (PLAN 11.12).
///
/// Same validation chain as [`edit_figure`] except for the level/gold
/// range check (no body to validate). On success calls
/// `working_copies::reset_to_fresh` which overwrites the working copy
/// with the pack master, then broadcasts `FigureUpdated { level: 1,
/// gold: 0 }` so any figure-detail screen showing the figure refreshes
/// its stats strip back to the pack-defaults state.
pub async fn reset_figure(
    State(state): State<Arc<AppState>>,
    AxumPath((profile_id, figure_id)): AxumPath<(String, String)>,
) -> Response {
    let figure_id_key = FigureId(figure_id.clone());

    let figure = match state.figures.iter().find(|f| f.id == figure_id_key) {
        Some(f) => f.clone(),
        None => {
            return (StatusCode::NOT_FOUND, "figure not in catalog").into_response();
        }
    };

    if !matches!(
        figure.category,
        Category::Figure | Category::Sidekick | Category::Giant | Category::Kaos
    ) {
        return (
            StatusCode::UNPROCESSABLE_ENTITY,
            format!("reset not supported for {:?}", figure.category),
        )
            .into_response();
    }

    {
        let portal = state.portal.lock().await;
        for slot in portal.iter() {
            let slot_fig_id = match slot {
                SlotState::Loaded { figure_id, .. } | SlotState::Loading { figure_id, .. } => {
                    figure_id.as_ref()
                }
                _ => None,
            };
            if slot_fig_id == Some(&figure_id_key) {
                return (StatusCode::CONFLICT, "remove from portal before resetting")
                    .into_response();
            }
        }
    }

    if let Err(e) = working_copies::reset_to_fresh(&profile_id, &figure) {
        warn!(
            error = ?e,
            profile = %profile_id,
            figure = %figure_id,
            "reset_to_fresh failed"
        );
        return (StatusCode::INTERNAL_SERVER_ERROR, "reset failed").into_response();
    }

    info!(
        profile = %profile_id,
        figure = %figure_id,
        "reset working copy to pack-fresh"
    );

    let _ = state.events.send(Event::FigureUpdated {
        figure_id: figure_id_key,
        level: 1,
        gold: 0,
    });

    StatusCode::ACCEPTED.into_response()
}

fn game_to_generation(g: GameOfOrigin) -> SkyGeneration {
    match g {
        GameOfOrigin::SpyrosAdventure => SkyGeneration::SpyrosAdventure,
        GameOfOrigin::Giants => SkyGeneration::Giants,
        GameOfOrigin::SwapForce => SkyGeneration::SwapForce,
        GameOfOrigin::TrapTeam => SkyGeneration::TrapTeam,
        GameOfOrigin::Superchargers => SkyGeneration::SuperChargers,
        GameOfOrigin::Imaginators => SkyGeneration::Imaginators,
        GameOfOrigin::CrossGame | GameOfOrigin::Unknown => SkyGeneration::Unknown,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn game_to_generation_covers_all_arms() {
        // Sanity: every GameOfOrigin variant produces a sensible SkyGeneration.
        // Catch-all for the cross-game / unknown buckets is "Unknown" which is
        // permissive (max level 20).
        assert_eq!(
            game_to_generation(GameOfOrigin::SpyrosAdventure),
            SkyGeneration::SpyrosAdventure
        );
        assert_eq!(
            game_to_generation(GameOfOrigin::Giants),
            SkyGeneration::Giants
        );
        assert_eq!(
            game_to_generation(GameOfOrigin::SwapForce),
            SkyGeneration::SwapForce
        );
        assert_eq!(
            game_to_generation(GameOfOrigin::TrapTeam),
            SkyGeneration::TrapTeam
        );
        assert_eq!(
            game_to_generation(GameOfOrigin::Superchargers),
            SkyGeneration::SuperChargers
        );
        assert_eq!(
            game_to_generation(GameOfOrigin::Imaginators),
            SkyGeneration::Imaginators
        );
        assert_eq!(
            game_to_generation(GameOfOrigin::CrossGame),
            SkyGeneration::Unknown
        );
        assert_eq!(
            game_to_generation(GameOfOrigin::Unknown),
            SkyGeneration::Unknown
        );
    }
}
