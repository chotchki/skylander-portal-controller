//! Figures and games.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

/// Stable figure identifier. First 16 hex chars of SHA-256 of
/// `"<game>|<element-or-empty>|<relative_path>"` — same scheme the Phase 1c
/// indexer uses. The value is treated as opaque elsewhere.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct FigureId(pub String);

impl FigureId {
    pub fn new(s: impl Into<String>) -> Self {
        FigureId(s.into())
    }
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for FigureId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

/// PS3 game serial (e.g. `BLUS30968` for Skylanders Giants).
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct GameSerial(pub String);

impl GameSerial {
    pub fn new(s: impl Into<String>) -> Self {
        GameSerial(s.into())
    }
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for GameSerial {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

/// Skylander element. `None` at the outer layer means the item isn't
/// elementally typed (Items, Adventure Packs, Kaos).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Element {
    Air,
    Dark,
    Earth,
    Fire,
    Life,
    Light,
    Magic,
    Tech,
    Undead,
    Water,
}

/// Broad classification derived from folder/category at index time.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Category {
    /// Regular player-controlled skylander.
    Figure,
    /// Sidekick (mini helper).
    Sidekick,
    /// Giants-generation giant (e.g. Tree Rex, Bouncer).
    Giant,
    /// Consumable/support item.
    Item,
    /// Trap Team elemental trap.
    Trap,
    /// Adventure-pack place.
    AdventurePack,
    /// Imaginators creation crystal.
    CreationCrystal,
    /// SuperChargers vehicle.
    Vehicle,
    /// Kaos-specific entry.
    Kaos,
    /// Fallback for anything the indexer couldn't classify.
    Other,
}

/// Game origin, used in `Figure` and surfaced in the filter UI.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum GameOfOrigin {
    SpyrosAdventure,
    Giants,
    SwapForce,
    TrapTeam,
    Superchargers,
    Imaginators,
    /// Cross-cutting Items / Adventure Packs / Sidekicks collections at the
    /// firmware pack root that don't tie to a single game of origin.
    CrossGame,
}

/// Server-side figure record. Holds filesystem details — **never** serialize
/// this directly to the phone; convert to `PublicFigure`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Figure {
    pub id: FigureId,
    /// Display name shown to users (from the filename, with punctuation/casing fixed).
    pub canonical_name: String,
    /// Groups reposes together. For reposes this is the base figure name;
    /// for base figures it's the same as `canonical_name`.
    pub variant_group: String,
    /// `"base"` for the primary figure; a short tag (`"Legendary"`, `"Dark"`,
    /// etc.) for reposes. Used by the phone UI to cycle variants.
    pub variant_tag: String,
    pub game: GameOfOrigin,
    pub element: Option<Element>,
    pub category: Category,
    /// Absolute path on disk. **Server-private.**
    pub sky_path: PathBuf,
    /// Path to the element-symbol PNG, if present in the pack. **Server-private.**
    pub element_icon_path: Option<PathBuf>,
}

impl Figure {
    pub fn to_public(&self) -> PublicFigure {
        PublicFigure {
            id: self.id.clone(),
            canonical_name: self.canonical_name.clone(),
            variant_group: self.variant_group.clone(),
            variant_tag: self.variant_tag.clone(),
            game: self.game,
            element: self.element,
            category: self.category,
        }
    }
}

/// Phone-safe figure. No filesystem paths. No relative_path. This is what
/// `GET /api/figures` returns.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PublicFigure {
    pub id: FigureId,
    pub canonical_name: String,
    pub variant_group: String,
    pub variant_tag: String,
    pub game: GameOfOrigin,
    pub element: Option<Element>,
    pub category: Category,
}

/// Game the user can launch. MVP-scope: Phase 2 just lists games; Phase 3
/// wires up the CLI boot via `sky_root`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Game {
    pub id: GameSerial,
    pub display_name: String,
    /// Filesystem root of the game (from `RPCS3/config/games.yml`).
    /// Server-private.
    #[serde(skip)]
    pub sky_root: Option<PathBuf>,
}
