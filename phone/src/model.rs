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

/// How the launcher presents its window (PLAN 20). Mirrors the server's
/// `config::WindowMode` (snake_case wire form). Used by the Konami-gated admin
/// window-mode toggle in the profile picker.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum WindowMode {
    #[default]
    Tv,
    Desktop,
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
    /// Scan-discovered figure with unresolved game-of-origin (PLAN 6.5.5).
    /// Phone hides this from the game filter chip row to avoid an
    /// empty-looking "Unknown" filter; figures with this game still
    /// render normally in the unfiltered library view.
    Unknown,
}

/// Land/Sky/Sea classification for SuperChargers vehicles. Mirrors
/// `core::VehicleTerrain`. Server populates from `data/vehicle_terrain.json`
/// (PLAN 9.7 playtest 2026-05-04).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum VehicleTerrain {
    Land,
    Sky,
    Sea,
}

impl VehicleTerrain {
    pub fn css_class(self) -> &'static str {
        match self {
            Self::Land => "vt-land",
            Self::Sky => "vt-sky",
            Self::Sea => "vt-sea",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Land => "LAND",
            Self::Sky => "SKY",
            Self::Sea => "SEA",
        }
    }
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
    #[serde(default)]
    pub vehicle_terrain: Option<VehicleTerrain>,
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
    /// shows the Kaos takeover screen with a "kick back" button. The
    /// `cooldown_remaining_secs` field is the wall-clock window during
    /// which a reload would still get bounced by the server's
    /// forced-evict cooldown; phone drives a local 1Hz countdown from
    /// it and disables KICK BACK IN until it hits zero (PLAN 8.2a).
    /// Defaulted for backwards compat with the v1.0.0 server (which
    /// didn't include the field).
    TakenOver {
        session_id: u64,
        by_kaos: String,
        #[serde(default)]
        cooldown_remaining_secs: u32,
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
    /// RPCS3 crashed *or* froze and the server is auto-recovering it (kill →
    /// relaunch the same game → re-place the portal figures). Phone shows the
    /// transient "RECONNECTING…" form of `GameCrashScreen` (no action button);
    /// the next `GameChanged { current: Some(_) }` dismisses it once the restart
    /// lands. If recovery exhausts its retries the server sends `GameCrashed`,
    /// which flips the same overlay to its terminal form. PLAN 16.7.2/.3.
    GameRecovering {
        #[serde(default)]
        message: String,
    },
    /// RPCS3's settings GUI opened (`open: true`) or closed (`open: false`) on
    /// the TV for per-game config (PLAN 16.9.3). While open, the portal is
    /// unavailable; the phone shows a "configuring on the TV…" overlay and
    /// dismisses it on `open: false`.
    Rpcs3SettingsChanged {
        #[serde(default)]
        open: bool,
    },
    /// A figure was scanned on the attached NFC reader. PLAN 6.5.2 — phone
    /// listens for this to drive the scan-import overlay from Prompt →
    /// Success (if open) or to fire a passive "Scanned: <name>" toast
    /// otherwise.
    FigureScanned {
        #[serde(default)]
        uid: String,
        #[serde(default)]
        figure_id: u32,
        #[serde(default)]
        variant: u16,
        #[serde(default)]
        display_name: String,
        #[serde(default)]
        is_duplicate: bool,
    },
    /// Kaos just swapped one of this profile's figures for a compat-eligible
    /// replacement. Phone renders the `kaos_swap` overlay variant with the
    /// taunt text for ~5s. Fields mirror the server's payload — PLAN 8.2b.4.
    KaosTaunt {
        #[serde(default)]
        profile_id: String,
        #[serde(default)]
        slot: u8,
        #[serde(default)]
        old_figure_id: String,
        #[serde(default)]
        new_figure_id: String,
        #[serde(default)]
        taunt: String,
    },
    /// A figure's stored stats (level + gold) changed via the edit endpoint
    /// (PLAN 11). Phone uses this to refresh the stats strip on whichever
    /// figure detail screen is open — currently a no-op log; the single-phone
    /// edit flow uses a local re-fetch on save instead. Wired up here so WS
    /// deserialization doesn't fail on the broadcast.
    FigureUpdated {
        #[serde(default)]
        figure_id: String,
        #[serde(default)]
        level: u8,
        #[serde(default)]
        gold: u16,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
pub struct PublicProfile {
    pub id: String,
    pub display_name: String,
    pub color: String,
    /// Mirrors the server-side field so the AdminEdit screen can render
    /// an immediate-fire Kaos toggle (PLAN 9.7 playtest 2026-05-04 —
    /// kaos toggle moved out of the kebab overlay because the
    /// description text overflowed iPhone width). `serde(default)` so
    /// older payloads round-trip cleanly.
    #[serde(default)]
    pub kaos_enabled: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
pub struct UnlockedProfile {
    pub id: String,
    pub display_name: String,
    pub color: String,
    /// PLAN 8.2b.1 — phone-side mirror of the server's Kaos opt-in.
    /// `#[serde(default)]` so older servers (or the pre-8.2b.1
    /// variant with no column) fall through to `false`.
    #[serde(default)]
    pub kaos_enabled: bool,
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
