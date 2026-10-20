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

/// Which app serves the phone, from `/api/status`'s `app` (openspec AA.18,
/// AA.19). `Product` until the status comes back, so the controller's phone
/// behaves exactly as it did.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ServerApp {
    #[default]
    Product,
    Guacamole,
}

impl ServerApp {
    /// Whether a figure `placed_by` put on the portal stops `viewer` placing
    /// it again: the product blocks whoever placed it, guacamole only the
    /// viewer's own (each profile owns its own instance, like two toys).
    pub fn blocks_place(self, placed_by: Option<&str>, viewer: Option<&str>) -> bool {
        match self {
            Self::Product => true,
            Self::Guacamole => placed_by == viewer,
        }
    }

    /// guacamole has no RPCS3 to configure, so its menu leaves HOLD FOR GAME
    /// SETTINGS out.
    pub fn offers_rpcs3_settings(self) -> bool {
        self == Self::Product
    }

    /// What guacamole's refusal of a figure action says to a kid (AH.3), from
    /// the `HTTP <status>` error the request came back with; `None` keeps the
    /// error as the phone showed it before, and always in the product.
    pub fn refusal(self, action: FigureAction, error: &str, name: &str) -> Option<String> {
        if self != Self::Guacamole {
            return None;
        }
        let words = match (action, error.strip_prefix("HTTP ")?) {
            (_, "403") => "This phone can only change its own profile's Skylanders. \
                           Unlock your profile and try again."
                .to_string(),
            (FigureAction::Edit | FigureAction::Reset, "409") => {
                format!("{name} is on the portal! Take {name} off first, then try again.")
            }
            (FigureAction::SlotReset, "409") => {
                format!("{name} is out in the level! Try again once the level's over.")
            }
            _ => return None,
        };
        Some(words)
    }
}

/// Why a load (a PLACE, or one slot of a resume) didn't land.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LoadError {
    /// guacamole's 409 for the viewer's own figure already standing in
    /// another slot (AH.37), told from AB.22's 409 by its body.
    AlreadyOnPortal,
    /// Anything else, worded as every other request's error: `HTTP <status>`,
    /// or what the browser said when no answer came.
    Failed(String),
}

impl LoadError {
    /// A refused load's status and body. Only the 409 whose body names the
    /// slot already holding the figure is AH.37's; AB.22's 409 (the slot holds
    /// another kid's figure) and the product's answers stay `HTTP <status>`.
    pub fn from_answer(status: u16, body: &str) -> Self {
        #[derive(Deserialize)]
        struct AlreadyOnPortal {
            already_on_portal: u8,
        }
        let names_a_slot = || {
            serde_json::from_str::<AlreadyOnPortal>(body)
                .is_ok_and(|a| (1..=SLOT_COUNT as u8).contains(&a.already_on_portal))
        };
        match status {
            409 if names_a_slot() => Self::AlreadyOnPortal,
            _ => Self::Failed(format!("HTTP {status}")),
        }
    }
}

/// What the phone says when `name` is already on the portal for its viewer,
/// whether the phone's own view caught it or the server did (AA.18, AH.37).
pub fn already_on_portal(name: &str) -> String {
    format!("{name} is already on the portal.")
}

/// A figure action the server can refuse (AH.3): the edit sheet's SAVE, the
/// details' reset off the portal, and the reset of the figure where it stands.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FigureAction {
    Edit,
    Reset,
    SlotReset,
}

/// Where guacamole's figure list stands, from `/api/status`'s `roster`
/// (openspec AE.4); the product sends none. Until `Ready` the collection is
/// empty and says why.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RosterState {
    Reading,
    Unrecognized,
    Unreadable,
    Ready,
}

impl RosterState {
    /// What the empty collection says, or `None` once the figures are there.
    pub fn note(self) -> Option<&'static str> {
        match self {
            Self::Reading => Some("Reading the figures off your disc\u{2026}"),
            Self::Unrecognized => {
                Some("This Giants disc isn't one guacamole recognizes, so there are no figures.")
            }
            Self::Unreadable => Some("The figures couldn't be read off your disc."),
            Self::Ready => None,
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
