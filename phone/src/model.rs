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

impl Element {
    pub fn css_class(self) -> &'static str {
        match self {
            Self::Air => "el-air",
            Self::Dark => "el-dark",
            Self::Earth => "el-earth",
            Self::Fire => "el-fire",
            Self::Life => "el-life",
            Self::Light => "el-light",
            Self::Magic => "el-magic",
            Self::Tech => "el-tech",
            Self::Undead => "el-undead",
            Self::Water => "el-water",
        }
    }
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
        /// Profile id of whoever initiated this load. Preserved across
        /// Loading→Loaded so the phone can render a per-slot ownership
        /// badge in 3.10e. `serde(default)` so older/unknown payloads round-
        /// trip cleanly.
        #[serde(default)]
        placed_by: Option<String>,
    },
    Loaded {
        #[serde(default)]
        figure_id: Option<String>,
        display_name: String,
        #[serde(default)]
        placed_by: Option<String>,
    },
    Error {
        message: String,
    },
}

/// Wire event from the server's `/ws`. Each session-targeted variant carries
/// a `session_id` so a shared broadcast channel can fan out to both clients
/// with each filtering by their own id.
#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Event {
    /// First event on every WS connection. Tells the phone the session id it
    /// should attach as `X-Session-Id` on every mutating REST request + filter
    /// session-targeted broadcasts by. `boot_id` is the server's per-startup
    /// random u64 — phones compare against the last-seen value and reload on
    /// mismatch so a server restart wipes any stale UI state.
    Welcome {
        session_id: u64,
        boot_id: u64,
    },
    /// This session was forcibly evicted by a 3rd connection (FIFO). Phone
    /// shows the Kaos takeover screen with a "kick back" button.
    TakenOver {
        session_id: u64,
        by_kaos: String,
    },
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
    GameChanged {
        current: Option<GameLaunched>,
    },
    ProfileChanged {
        session_id: u64,
        profile: Option<UnlockedProfile>,
    },
    /// Offered post-unlock when the just-unlocked profile has a stored
    /// portal layout. PLAN 3.12.
    ResumePrompt {
        session_id: u64,
        slots: Vec<SlotState>,
    },
    /// RPCS3 crashed while a game was running. Phone renders a full-screen
    /// "GAME CRASHED" overlay (see `GameCrashScreen`). Auto-dismissed on the
    /// next `GameChanged { current: Some(_) }`. PLAN 4.15.14 /
    /// `docs/aesthetic/navigation.md` §3.8.
    GameCrashed {
        #[serde(default)]
        message: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
pub struct PublicProfile {
    pub id: String,
    pub display_name: String,
    pub color: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
pub struct UnlockedProfile {
    pub id: String,
    pub display_name: String,
    pub color: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct GameLaunched {
    pub serial: String,
    pub display_name: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
pub struct InstalledGame {
    pub serial: String,
    pub display_name: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConnState {
    Connecting,
    Connected,
    Disconnected,
}
