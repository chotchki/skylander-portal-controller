//! Wire protocol between server and phone. Both REST (for `Command`-shaped
//! POST bodies) and the `/ws` WebSocket channel (for `Event`s).

use serde::{Deserialize, Serialize};

use crate::figure::FigureId;
use crate::portal::{SlotIndex, SlotState, SLOT_COUNT};

/// Client → server. Delivered as REST bodies in MVP; a WebSocket command
/// channel may land later.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Command {
    LoadFigure {
        slot: SlotIndex,
        figure_id: FigureId,
    },
    ClearSlot {
        slot: SlotIndex,
    },
    /// Ask the server to re-read the portal from RPCS3 and broadcast a fresh
    /// `PortalSnapshot`.
    RefreshPortal,
}

/// Server → client. Delivered on `/ws`.
///
/// Several variants carry a `session_id` so a shared broadcast channel can
/// fan messages out to all two connected clients, and each client filters
/// for events addressed to it. The id is the opaque u64 minted by the
/// server's [`SessionRegistry`] — the phone receives its own id in the
/// initial [`Event::Welcome`] and stores it for filtering + REST routing.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Event {
    /// First event on every WS connection. Tells the phone the session id
    /// it should attach as `X-Session-Id` on subsequent REST calls, and
    /// which id to watch for in targeted events like `ProfileChanged` and
    /// `TakenOver`.
    Welcome {
        session_id: u64,
    },
    /// Full snapshot of all 8 slots. Sent on connect and after `RefreshPortal`.
    PortalSnapshot {
        slots: [SlotState; SLOT_COUNT],
    },
    /// One slot changed state.
    SlotChanged {
        slot: SlotIndex,
        state: SlotState,
    },
    /// Non-fatal error surfaced as a toast on the phone.
    Error {
        message: String,
    },
    /// Game state changed. `None` means "no game running".
    GameChanged {
        current: Option<GameLaunched>,
    },
    /// Profile unlock/lock transition, scoped to one session. `None` means
    /// the session is locked. Broadcast; clients ignore events not for
    /// their `session_id`.
    ProfileChanged {
        session_id: u64,
        profile: Option<UnlockedProfile>,
    },
    /// The target session has been forcibly evicted by a 3rd connection
    /// (FIFO — oldest out). The evicted phone shows the "Chaos took over"
    /// screen. Broadcast; clients ignore events not for their `session_id`.
    TakenOver {
        session_id: u64,
        by_chaos: String,
    },
}

/// Public profile description included in [`Event::ProfileChanged`] and the
/// initial WS snapshot. Never carries the PIN hash.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UnlockedProfile {
    pub id: String,
    pub display_name: String,
    pub color: String,
}

/// Announcement payload included in `Event::GameChanged`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GameLaunched {
    pub serial: crate::figure::GameSerial,
    pub display_name: String,
}
