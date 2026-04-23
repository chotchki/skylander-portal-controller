//! Per-figure `.sky` stats endpoint. Feature-gated behind `sky-stats` so
//! release builds can opt out while the parser's decrypt path matures.
//!
//! Path layout mirrors PLAN 3.11's working-copy convention:
//!
//! * dev builds (`dev-tools`): `./dev-data/working/<profile_id>/<figure_id>.sky`
//! * release: `%APPDATA%/skylander-portal-controller/working/<profile_id>/<figure_id>.sky`
//!
//! 3.11 hasn't landed the copy-on-first-use machinery yet; until it does,
//! this endpoint 404s on figures the user hasn't loaded through the portal
//! with that profile. That's fine — the endpoint is dark until the phone UI
//! wires it up in Phase 5.

use std::path::PathBuf;
use std::sync::Arc;

use axum::extract::{Path as AxumPath, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use chrono::NaiveDateTime;
use serde::Serialize;
use skylander_sky_parser::{FigureKind, HatId, SkyFigureStats, SkyGeneration, VariantInfo, parse};
use tracing::{debug, warn};

use crate::state::AppState;

/// Public JSON shape — same as [`SkyFigureStats`] minus `raw_blocks`, which
/// exists for debug tooling only and would bloat every response with 1 KiB of
/// bytes the phone can't use.
#[derive(Debug, Serialize)]
pub struct PublicSkyStats {
    pub figure_id: u32,
    pub variant: u16,
    pub variant_decoded: VariantInfo,
    pub serial: u32,
    pub trading_card_id: u64,
    pub web_code: String,
    pub error_byte: u8,
    pub figure_kind: FigureKind,
    pub source_game_gen: SkyGeneration,

    pub level: u8,
    pub xp_2011: u32,
    pub xp_2012: u16,
    pub xp_2013: u32,
    pub gold: u16,
    pub playtime_secs: u32,
    pub nickname: String,
    pub hero_points: u16,
    pub hat_current: HatId,
    pub hat_history: [HatId; 4],
    pub trinket: u8,
    pub last_placed: Option<NaiveDateTime>,
    pub last_reset: Option<NaiveDateTime>,
    pub heroic_challenges_ssa: u32,
    pub heroic_challenges_sg: u32,
    pub battlegrounds_flags: u32,
    pub quests_giants: u128,
    pub quests_swap_force: u128,
    pub checksums_valid: bool,
}

impl From<SkyFigureStats> for PublicSkyStats {
    fn from(s: SkyFigureStats) -> Self {
        Self {
            figure_id: s.figure_id.get(),
            variant: s.variant.get(),
            variant_decoded: s.variant_decoded,
            serial: s.serial,
            trading_card_id: s.trading_card_id,
            web_code: s.web_code,
            error_byte: s.error_byte,
            figure_kind: s.figure_kind,
            source_game_gen: s.source_game_gen,
            level: s.level,
            xp_2011: s.xp_2011,
            xp_2012: s.xp_2012,
            xp_2013: s.xp_2013,
            gold: s.gold,
            playtime_secs: s.playtime_secs,
            nickname: s.nickname,
            hero_points: s.hero_points,
            hat_current: s.hat_current,
            hat_history: s.hat_history,
            trinket: s.trinket,
            last_placed: s.last_placed,
            last_reset: s.last_reset,
            heroic_challenges_ssa: s.heroic_challenges_ssa,
            heroic_challenges_sg: s.heroic_challenges_sg,
            battlegrounds_flags: s.battlegrounds_flags,
            quests_giants: s.quests_giants,
            quests_swap_force: s.quests_swap_force,
            checksums_valid: s.checksums_valid,
        }
    }
}

/// Resolve the working-copy root — matches PLAN 3.11's path contract so when
/// that phase lands, no call sites need to move. A release build without
/// `APPDATA` set returns an error rather than falling back to CWD (we never
/// want to touch unexpected locations).
pub fn working_root() -> std::io::Result<PathBuf> {
    #[cfg(feature = "dev-tools")]
    {
        Ok(PathBuf::from("dev-data").join("working"))
    }
    #[cfg(not(feature = "dev-tools"))]
    {
        let base = std::env::var_os("APPDATA")
            .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::NotFound, "APPDATA not set"))?;
        Ok(PathBuf::from(base)
            .join("skylander-portal-controller")
            .join("working"))
    }
}

/// Compose the working-copy path for a `(profile_id, figure_id)` pair.
/// Inputs are validated to not contain path separators — untrusted ids must
/// never escape the `working/` root.
pub fn working_copy_path(profile_id: &str, figure_id: &str) -> Result<PathBuf, &'static str> {
    if profile_id.is_empty()
        || figure_id.is_empty()
        || profile_id.contains(['/', '\\', '.'])
        || figure_id.contains(['/', '\\', '.'])
    {
        return Err("id contains illegal characters");
    }
    let root = working_root().map_err(|_| "cannot resolve working root")?;
    Ok(root.join(profile_id).join(format!("{figure_id}.sky")))
}

/// GET /api/profiles/:profile_id/figures/:figure_id/stats
pub async fn get_figure_stats(
    State(_state): State<Arc<AppState>>,
    AxumPath((profile_id, figure_id)): AxumPath<(String, String)>,
) -> Response {
    let path = match working_copy_path(&profile_id, &figure_id) {
        Ok(p) => p,
        Err(msg) => {
            warn!(%profile_id, %figure_id, "rejecting stats lookup: {msg}");
            return (StatusCode::BAD_REQUEST, msg).into_response();
        }
    };

    debug!(path = %path.display(), "reading working copy for stats");
    let bytes = match tokio::fs::read(&path).await {
        Ok(b) => b,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            return (
                StatusCode::NOT_FOUND,
                "no working copy for that profile+figure",
            )
                .into_response();
        }
        Err(e) => {
            warn!(path = %path.display(), error = %e, "failed to read working copy");
            return (StatusCode::INTERNAL_SERVER_ERROR, "read failed").into_response();
        }
    };

    match parse(&bytes) {
        Ok(stats) => axum::Json(PublicSkyStats::from(stats)).into_response(),
        Err(e) => {
            warn!(path = %path.display(), error = %e, "sky parse failed");
            (StatusCode::UNPROCESSABLE_ENTITY, e.to_string()).into_response()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn path_rejects_separators() {
        assert!(working_copy_path("../etc", "x").is_err());
        assert!(working_copy_path("a", "b/c").is_err());
        assert!(working_copy_path("a\\b", "c").is_err());
        assert!(working_copy_path("", "c").is_err());
        assert!(working_copy_path("a", "").is_err());
    }

    #[test]
    fn path_accepts_plain_ids() {
        let p = working_copy_path("01JABCDEFG", "deadbeefcafef00d").unwrap();
        let s = p.to_string_lossy();
        assert!(s.contains("working"));
        assert!(s.ends_with("deadbeefcafef00d.sky"));
    }
}
