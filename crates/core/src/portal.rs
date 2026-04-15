//! Placeholder — populated in 2.2.

use serde::{Deserialize, Serialize};

pub const SLOT_COUNT: usize = 8;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct SlotIndex(pub u8);

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SlotState {
    Empty,
}
