//! RPCS3 portal control.
//!
//! The `PortalDriver` trait and `UiaPortalDriver` implementation are populated
//! in 2.3. The `MockPortalDriver` lives behind the `mock` feature flag.

use std::path::Path;

use anyhow::Result;
use skylander_core::{SlotIndex, SlotState, SLOT_COUNT};

pub trait PortalDriver: Send + Sync {
    fn open_dialog(&self) -> Result<()>;
    fn read_slots(&self) -> Result<[SlotState; SLOT_COUNT]>;
    fn load(&self, slot: SlotIndex, path: &Path) -> Result<String>;
    fn clear(&self, slot: SlotIndex) -> Result<()>;
}

#[cfg(feature = "mock")]
pub mod mock;
