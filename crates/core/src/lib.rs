//! Shared types for the Skylander Portal Controller.
//!
//! This crate has no I/O. It defines the domain model (figures, slots, portal
//! state) and the wire protocol between server and phone. Phase 2 MVP scope —
//! profiles, PINs, working copies, and game launching come in Phase 3.

pub mod compat;
pub mod figure;
pub mod game;
pub mod portal;
pub mod protocol;

pub use compat::{game_of_origin_from_serial, is_compatible};
pub use figure::{
    Category, Element, Figure, FigureId, Game, GameOfOrigin, GameSerial, MaskedVariant,
    MifareNuid, PublicFigure, TagIdentity, TagVariant, ToyTypeId, VARIANT_IDENTITY_MASK,
};
pub use game::{InstalledGame, SKYLANDERS_SERIALS};
pub use portal::{SLOT_COUNT, SlotIndex, SlotIndexOutOfRange, SlotState};
pub use protocol::{Command, Event, GameLaunched, UnlockedProfile};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slot_index_range() {
        for n in 0..8u8 {
            let s = SlotIndex::new(n).unwrap();
            assert_eq!(s.as_u8(), n);
            assert_eq!(s.display(), n + 1);
        }
        assert!(SlotIndex::new(8).is_err());
        assert!(SlotIndex::from_display(0).is_err());
        assert_eq!(SlotIndex::from_display(8).unwrap().as_u8(), 7);
    }

    #[test]
    fn slot_state_serde_roundtrip() {
        let states = [
            SlotState::Empty,
            SlotState::Loading {
                figure_id: None,
                placed_by: None,
            },
            SlotState::Loading {
                figure_id: Some(FigureId::new("deadbeefcafebabe")),
                placed_by: Some("alice".into()),
            },
            SlotState::Loaded {
                figure_id: Some(FigureId::new("deadbeefcafebabe")),
                display_name: "Eruptor".into(),
                placed_by: Some("alice".into()),
            },
            SlotState::Error {
                message: "file locked".into(),
            },
        ];
        for s in &states {
            let j = serde_json::to_string(s).unwrap();
            let back: SlotState = serde_json::from_str(&j).unwrap();
            assert_eq!(s, &back);
        }
    }

    #[test]
    fn command_discriminants() {
        let load = Command::LoadFigure {
            slot: SlotIndex::new(0).unwrap(),
            figure_id: FigureId::new("abc"),
        };
        let j = serde_json::to_string(&load).unwrap();
        assert!(j.contains(r#""kind":"load_figure""#));

        let clear = Command::ClearSlot {
            slot: SlotIndex::new(7).unwrap(),
        };
        let j = serde_json::to_string(&clear).unwrap();
        assert!(j.contains(r#""kind":"clear_slot""#));
        assert!(j.contains(r#""slot":7"#));
    }

    #[test]
    fn event_snapshot_roundtrip() {
        let snap = Event::PortalSnapshot {
            slots: std::array::from_fn(|i| {
                if i == 0 {
                    SlotState::Loaded {
                        figure_id: None,
                        display_name: "Eruptor".into(),
                        placed_by: None,
                    }
                } else {
                    SlotState::Empty
                }
            }),
        };
        let j = serde_json::to_string(&snap).unwrap();
        let back: Event = serde_json::from_str(&j).unwrap();
        if let Event::PortalSnapshot { slots } = back {
            assert!(matches!(slots[1], SlotState::Empty));
            if let SlotState::Loaded { display_name, .. } = &slots[0] {
                assert_eq!(display_name, "Eruptor");
            } else {
                panic!("slot 0 not loaded");
            }
        } else {
            panic!("not a snapshot");
        }
    }

    #[test]
    fn figure_to_public_drops_paths() {
        use std::path::PathBuf;
        let f = Figure {
            id: FigureId::new("abc"),
            canonical_name: "Eruptor".into(),
            variant_group: "Eruptor".into(),
            variant_tag: "base".into(),
            game: GameOfOrigin::SpyrosAdventure,
            element: Some(Element::Fire),
            category: Category::Figure,
            sky_path: PathBuf::from("C:/pack/fire/Eruptor.sky"),
            element_icon_path: Some(PathBuf::from("C:/pack/fire/FireSymbolSkylanders.png")),
            tag_identity: None,
        };
        let j = serde_json::to_string(&f.to_public()).unwrap();
        assert!(!j.contains("C:/pack"));
        assert!(!j.contains("sky_path"));
    }
}
