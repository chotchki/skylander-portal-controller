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
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Event {
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
}
