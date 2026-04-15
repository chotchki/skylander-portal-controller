//! Mock portal driver. Populated in 2.3.11.

use std::path::Path;

use anyhow::Result;
use skylander_core::{SlotIndex, SlotState, SLOT_COUNT};

use crate::PortalDriver;

pub struct MockPortalDriver;

impl PortalDriver for MockPortalDriver {
    fn open_dialog(&self) -> Result<()> {
        Ok(())
    }

    fn read_slots(&self) -> Result<[SlotState; SLOT_COUNT]> {
        Ok(std::array::from_fn(|_| SlotState::Empty))
    }

    fn load(&self, _slot: SlotIndex, _path: &Path) -> Result<String> {
        Ok("Mock".into())
    }

    fn clear(&self, _slot: SlotIndex) -> Result<()> {
        Ok(())
    }
}
