//! Mirror types for what the server sends us. We don't share crates/core
//! directly because the phone crate is intentionally separate from the root
//! workspace (trunk's wasm target lives on its own).

use serde::{Deserialize, Serialize};

pub const SLOT_COUNT: usize = 8;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Category {
    Figure,
    Sidekick,
    Giant,
    Item,
    Trap,
    AdventurePack,
    CreationCrystal,
    Vehicle,
    Kaos,
    Other,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum GameOfOrigin {
    SpyrosAdventure,
    Giants,
    SwapForce,
    TrapTeam,
    Superchargers,
    Imaginators,
    CrossGame,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PublicFigure {
    pub id: String,
    pub canonical_name: String,
    pub variant_group: String,
    pub variant_tag: String,
    pub game: GameOfOrigin,
    pub element: Option<Element>,
    pub category: Category,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Slot {
    pub state: SlotState,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SlotState {
    Empty,
    Loading {
        #[serde(default)]
        #[allow(dead_code)]
        figure_id: Option<String>,
    },
    Loaded {
        #[serde(default)]
        figure_id: Option<String>,
        display_name: String,
    },
    Error {
        message: String,
    },
}

/// Wire event from the server's `/ws`.
#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Event {
    PortalSnapshot {
        slots: Vec<SlotState>,
    },
    SlotChanged {
        slot: u8, // 0-indexed on the wire
        state: SlotState,
    },
    Error {
        message: String,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConnState {
    Connecting,
    Connected,
    Disconnected,
}
