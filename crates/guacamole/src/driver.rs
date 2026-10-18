//! The native portal driver (AA design decision 4), minimal until AA.7 adds
//! the channel to the TV: places by identity and keeps its own slots.

use std::path::Path;
use std::sync::Mutex;

use anyhow::Result;
use skylander_core::{SLOT_COUNT, SlotIndex, SlotState};
use skylander_rpcs3_control::{Placement, PortalDriver};

pub struct NativePortalDriver {
    slots: Mutex<[SlotState; SLOT_COUNT]>,
}

impl NativePortalDriver {
    pub fn new() -> Self {
        Self {
            slots: Mutex::new(std::array::from_fn(|_| SlotState::Empty)),
        }
    }

    fn set(&self, slot: SlotIndex, state: SlotState) {
        let mut slots = self.slots.lock().unwrap_or_else(|e| e.into_inner());
        slots[slot.as_usize()] = state;
    }
}

impl PortalDriver for NativePortalDriver {
    fn open_dialog(&self) -> Result<()> {
        Ok(())
    }

    fn read_slots(&self) -> Result<[SlotState; SLOT_COUNT]> {
        Ok(self.slots.lock().unwrap_or_else(|e| e.into_inner()).clone())
    }

    /// Always an error: under Native a file path means a missed arm.
    fn load(&self, _slot: SlotIndex, path: &Path) -> Result<String> {
        anyhow::bail!(
            "the native driver places by identity, not a file ({})",
            path.display()
        )
    }

    fn clear(&self, slot: SlotIndex) -> Result<()> {
        self.set(slot, SlotState::Empty);
        Ok(())
    }

    fn place(&self, slot: SlotIndex, placement: &Placement) -> Result<()> {
        self.set(
            slot,
            SlotState::Loaded {
                figure_id: Some(placement.figure_id.clone()),
                display_name: placement.name.clone(),
                placed_by: placement.owner.as_ref().map(|o| o.profile_id.clone()),
            },
        );
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use skylander_core::FigureId;
    use skylander_rpcs3_control::PlacementOwner;

    fn eruptor() -> Placement {
        let figure_id = FigureId::new("000009-0000".to_string());
        let (toy_type, variant) = figure_id.tag_parts().expect("tag-form id");
        Placement {
            figure_id,
            toy_type,
            variant,
            name: "Eruptor".into(),
            element: None,
            portrait: None,
            owner: Some(PlacementOwner {
                profile_id: "p1".into(),
                initial: "A".into(),
                color: "#ff0000".into(),
            }),
        }
    }

    #[test]
    fn place_and_clear_show_in_read_slots() {
        let d = NativePortalDriver::new();
        let slot = SlotIndex::new(2).unwrap();
        d.place(slot, &eruptor()).unwrap();
        match &d.read_slots().unwrap()[2] {
            SlotState::Loaded {
                figure_id,
                display_name,
                placed_by,
            } => {
                assert_eq!(figure_id.as_ref().unwrap().as_str(), "000009-0000");
                assert_eq!(display_name, "Eruptor");
                assert_eq!(placed_by.as_deref(), Some("p1"));
            }
            other => panic!("expected Loaded, got {other:?}"),
        }
        d.clear(slot).unwrap();
        assert!(matches!(d.read_slots().unwrap()[2], SlotState::Empty));
    }

    #[test]
    fn load_with_a_path_errors_and_changes_nothing() {
        let d = NativePortalDriver::new();
        let slot = SlotIndex::new(0).unwrap();
        assert!(d.load(slot, Path::new("/pack/Eruptor.sky")).is_err());
        assert!(matches!(d.read_slots().unwrap()[0], SlotState::Empty));
    }
}
