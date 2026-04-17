//! Portal slot state.

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::figure::FigureId;

/// Number of slots the emulated RPCS3 portal exposes. Fixed at 8 per the
/// RPCS3 source code (`UI_SKY_NUM`).
pub const SLOT_COUNT: usize = 8;

/// Zero-indexed slot (0..=7). Phone displays it 1-indexed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct SlotIndex(u8);

#[derive(Debug, Clone, Error)]
#[error("slot index {0} out of range (must be 0..{SLOT_COUNT})")]
pub struct SlotIndexOutOfRange(pub u8);

impl SlotIndex {
    pub fn new(n: u8) -> Result<Self, SlotIndexOutOfRange> {
        if (n as usize) < SLOT_COUNT {
            Ok(SlotIndex(n))
        } else {
            Err(SlotIndexOutOfRange(n))
        }
    }

    /// Build from a 1-indexed phone-side value.
    pub fn from_display(n: u8) -> Result<Self, SlotIndexOutOfRange> {
        if n == 0 {
            return Err(SlotIndexOutOfRange(0));
        }
        SlotIndex::new(n - 1)
    }

    pub fn as_u8(self) -> u8 {
        self.0
    }

    pub fn as_usize(self) -> usize {
        self.0 as usize
    }

    /// 1-indexed value for UI display.
    pub fn display(self) -> u8 {
        self.0 + 1
    }
}

impl std::fmt::Display for SlotIndex {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "slot {}", self.display())
    }
}

/// What's on a slot right now.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SlotState {
    Empty,
    /// A load or clear is in flight. The phone shows a spinner on the slot.
    Loading {
        figure_id: Option<FigureId>,
        /// Profile id of the session that initiated this load. Carried
        /// through to `Loaded` so both phones can render an ownership
        /// indicator on the slot (SPEC Round 4 Q104). `None` for legacy
        /// unauthenticated loads and for `RefreshPortal`-sourced reads where
        /// we don't know who placed the figure.
        placed_by: Option<String>,
    },
    Loaded {
        figure_id: Option<FigureId>,
        /// Display name as RPCS3 reports it. `figure_id` may be `None` if we
        /// haven't reconciled the name back to a pack figure yet.
        display_name: String,
        /// Same meaning as on `Loading`. Preserved across the Loading→Loaded
        /// transition; cleared back to `None` on `Empty` / `Error`.
        placed_by: Option<String>,
    },
    /// The last action failed. UI surfaces `message` as a toast; slot reverts
    /// to its prior state on the next successful update.
    Error {
        message: String,
    },
}

impl SlotState {
    pub fn is_empty(&self) -> bool {
        matches!(self, SlotState::Empty)
    }
}
