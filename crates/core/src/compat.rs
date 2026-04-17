//! Heuristic compatibility rule: does a figure "work" in a given game?
//!
//! Mirrors the Kaos-feature compatibility heuristic documented in
//! `CLAUDE.md`: a figure works in its game of origin and all later games,
//! with known exceptions (vehicles are SuperChargers-only). `CrossGame`
//! entries (top-level Items / Adventure Packs / Sidekicks in the firmware
//! pack) are treated as compatible with everything — the game-specific
//! Adventure Pack subfolders still carry their own `GameOfOrigin`.

use crate::figure::{Category, GameOfOrigin, GameSerial};

/// Release-order index, 0 for the earliest game. `CrossGame` has no single
/// release slot — the comparison logic treats it as always-compatible so
/// it returns `None` here.
fn release_index(g: GameOfOrigin) -> Option<u8> {
    match g {
        GameOfOrigin::SpyrosAdventure => Some(0),
        GameOfOrigin::Giants => Some(1),
        GameOfOrigin::SwapForce => Some(2),
        GameOfOrigin::TrapTeam => Some(3),
        GameOfOrigin::Superchargers => Some(4),
        GameOfOrigin::Imaginators => Some(5),
        GameOfOrigin::CrossGame => None,
    }
}

/// Map a PS3 game serial to the corresponding `GameOfOrigin`. `None` for
/// unknown serials.
pub fn game_of_origin_from_serial(serial: &GameSerial) -> Option<GameOfOrigin> {
    match serial.as_str() {
        "BLUS30906" => Some(GameOfOrigin::SpyrosAdventure),
        "BLUS30968" => Some(GameOfOrigin::Giants),
        "BLUS31076" => Some(GameOfOrigin::SwapForce),
        "BLUS31442" => Some(GameOfOrigin::TrapTeam),
        "BLUS31545" => Some(GameOfOrigin::Superchargers),
        "BLUS31600" => Some(GameOfOrigin::Imaginators),
        _ => None,
    }
}

/// True if a figure of origin `figure_game` + category `category` is
/// compatible with `current_game`. See the Kaos section of `CLAUDE.md`.
pub fn is_compatible(
    figure_game: GameOfOrigin,
    category: Category,
    current_game: GameOfOrigin,
) -> bool {
    // WHY: vehicles were introduced in SuperChargers and only function in
    // that title — the heuristic's single hard exception.
    if matches!(category, Category::Vehicle) {
        return matches!(current_game, GameOfOrigin::Superchargers);
    }

    // CrossGame entries (bare Items, Sidekicks, etc.) have no release slot;
    // treat as always-compatible — the pack layout already pushes
    // game-specific stuff into named subfolders.
    let Some(fig_idx) = release_index(figure_game) else {
        return true;
    };
    let Some(cur_idx) = release_index(current_game) else {
        return true;
    };
    fig_idx <= cur_idx
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn origin_or_later_is_compatible() {
        assert!(is_compatible(
            GameOfOrigin::SpyrosAdventure,
            Category::Figure,
            GameOfOrigin::Imaginators,
        ));
        assert!(is_compatible(
            GameOfOrigin::Giants,
            Category::Figure,
            GameOfOrigin::Giants,
        ));
    }

    #[test]
    fn earlier_game_cannot_use_newer_figure() {
        assert!(!is_compatible(
            GameOfOrigin::TrapTeam,
            Category::Figure,
            GameOfOrigin::Giants,
        ));
    }

    #[test]
    fn vehicles_superchargers_only() {
        assert!(is_compatible(
            GameOfOrigin::Superchargers,
            Category::Vehicle,
            GameOfOrigin::Superchargers,
        ));
        assert!(!is_compatible(
            GameOfOrigin::Superchargers,
            Category::Vehicle,
            GameOfOrigin::Imaginators,
        ));
        assert!(!is_compatible(
            GameOfOrigin::Superchargers,
            Category::Vehicle,
            GameOfOrigin::TrapTeam,
        ));
    }

    #[test]
    fn cross_game_items_work_everywhere() {
        for g in [
            GameOfOrigin::SpyrosAdventure,
            GameOfOrigin::Giants,
            GameOfOrigin::SwapForce,
            GameOfOrigin::TrapTeam,
            GameOfOrigin::Superchargers,
            GameOfOrigin::Imaginators,
        ] {
            assert!(is_compatible(GameOfOrigin::CrossGame, Category::Item, g));
        }
    }

    #[test]
    fn serial_mapping() {
        assert_eq!(
            game_of_origin_from_serial(&GameSerial::new("BLUS30968")),
            Some(GameOfOrigin::Giants),
        );
        assert_eq!(
            game_of_origin_from_serial(&GameSerial::new("NOPE00000")),
            None,
        );
    }
}
