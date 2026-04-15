//! In-memory `PortalDriver`. Use via the `mock` feature.

use std::path::Path;
use std::sync::Mutex;
use std::thread::sleep;
use std::time::Duration;

use anyhow::Result;
use skylander_core::{SlotIndex, SlotState, SLOT_COUNT};

use crate::PortalDriver;

/// Mock driver. Tracks per-slot state; `load` pulls the figure name from the
/// filename stem. Default latency 50ms per op (tune for tests).
pub struct MockPortalDriver {
    slots: Mutex<[SlotState; SLOT_COUNT]>,
    dialog_open: Mutex<bool>,
    latency: Duration,
}

impl MockPortalDriver {
    pub fn new() -> Self {
        Self::with_latency(Duration::from_millis(50))
    }

    pub fn with_latency(latency: Duration) -> Self {
        Self {
            slots: Mutex::new(std::array::from_fn(|_| SlotState::Empty)),
            dialog_open: Mutex::new(false),
            latency,
        }
    }

    fn delay(&self) {
        if !self.latency.is_zero() {
            sleep(self.latency);
        }
    }
}

impl Default for MockPortalDriver {
    fn default() -> Self {
        Self::new()
    }
}

impl PortalDriver for MockPortalDriver {
    fn open_dialog(&self) -> Result<()> {
        self.delay();
        *self.dialog_open.lock().unwrap() = true;
        Ok(())
    }

    fn read_slots(&self) -> Result<[SlotState; SLOT_COUNT]> {
        Ok(self.slots.lock().unwrap().clone())
    }

    fn load(&self, slot: SlotIndex, path: &Path) -> Result<String> {
        self.delay();
        let name = path
            .file_stem()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_else(|| "Unknown".into());
        let mut slots = self.slots.lock().unwrap();
        slots[slot.as_usize()] = SlotState::Loaded {
            figure_id: None,
            display_name: name.clone(),
        };
        Ok(name)
    }

    fn clear(&self, slot: SlotIndex) -> Result<()> {
        self.delay();
        self.slots.lock().unwrap()[slot.as_usize()] = SlotState::Empty;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn load_then_clear() {
        let d = MockPortalDriver::with_latency(Duration::ZERO);
        d.open_dialog().unwrap();

        let before = d.read_slots().unwrap();
        assert!(matches!(before[0], SlotState::Empty));

        let name = d
            .load(
                SlotIndex::new(0).unwrap(),
                &PathBuf::from("/pack/Fire/Eruptor.sky"),
            )
            .unwrap();
        assert_eq!(name, "Eruptor");

        let after_load = d.read_slots().unwrap();
        match &after_load[0] {
            SlotState::Loaded { display_name, .. } => assert_eq!(display_name, "Eruptor"),
            s => panic!("expected Loaded, got {s:?}"),
        }

        d.clear(SlotIndex::new(0).unwrap()).unwrap();
        let after_clear = d.read_slots().unwrap();
        assert!(matches!(after_clear[0], SlotState::Empty));
    }

    #[test]
    fn slots_are_independent() {
        let d = MockPortalDriver::with_latency(Duration::ZERO);
        d.load(SlotIndex::new(2).unwrap(), &PathBuf::from("/pack/Spyro.sky"))
            .unwrap();
        d.load(SlotIndex::new(5).unwrap(), &PathBuf::from("/pack/Chop Chop.sky"))
            .unwrap();
        let s = d.read_slots().unwrap();
        assert!(matches!(s[0], SlotState::Empty));
        assert!(matches!(s[1], SlotState::Empty));
        match &s[2] {
            SlotState::Loaded { display_name, .. } => assert_eq!(display_name, "Spyro"),
            other => panic!("{other:?}"),
        }
        assert!(matches!(s[3], SlotState::Empty));
        assert!(matches!(s[4], SlotState::Empty));
        match &s[5] {
            SlotState::Loaded { display_name, .. } => assert_eq!(display_name, "Chop Chop"),
            other => panic!("{other:?}"),
        }
    }
}
