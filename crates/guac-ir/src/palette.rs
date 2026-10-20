//! Animation palettes (openspec AH.22, design decision 7): every slot of a
//! model's animation set, the numbers a script plays by
//! (`animation palette [20]` is Eruptor's throw). AG.14's hero side keeps
//! the five locomotion slots, full NORMAL clips only; a palette keeps all
//! of them, additive and partial clips too (the throw is partial), so a
//! character joining the fight needs no re-extract.
//!
//! One file per character load file, `palettes/<load file>.<ron|postcard>`,
//! and per level one for the creatures its whitelisted placements name,
//! `levels/<archive>/palettes.<ron|postcard>`: new files beside unchanged
//! ones, so [`crate::IR_VERSION`] holds.

use serde::{Deserialize, Serialize};

use crate::DumpIdentity;
use crate::clip::Clip;

/// Every filled slot of one model's animation set: the ones that read and
/// the ones refused.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Palette {
    pub identity: DumpIdentity,
    /// A character's load file (`009_Eruptor`), or a level model's path as
    /// its placements name it (`C:/tfb/Content/Models/characters/Chompy.mdl`).
    pub owner: String,
    /// The set's length: a slot past it, or empty, plays nothing.
    pub slots: u16,
    /// Each filled slot that reads, in slot order.
    pub entries: Vec<PaletteSlot>,
    /// Each filled slot that doesn't, with why: it plays nothing here.
    pub refused: Vec<RefusedSlot>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PaletteSlot {
    pub slot: u16,
    /// The entry's own name as stored, if any.
    pub name: Option<String>,
    /// The entry's label as stored (`3.658 m/sec` on Eruptor's walk).
    pub label: Option<String>,
    pub entry: SlotEntry,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum SlotEntry {
    /// A clip on the model's skeleton, with how the original plays it.
    Clip {
        /// Its values add to the pose under it rather than replace it.
        additive: bool,
        /// Its tracks drive some joints, the rest left to what's under it.
        partial: bool,
        /// The wrapper's layered flag, as stored (none on the disc's
        /// characters).
        layered: bool,
        /// The wrapper's motion-extraction bits (0-15), as stored: which
        /// root motion the original takes out of the clip, unread so far.
        motion_extraction: u8,
        clip: Clip,
    },
    /// An animation network, or no animation: no clip of the slot's own.
    Network,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RefusedSlot {
    pub slot: u16,
    pub reason: String,
}

/// A level's creature palettes (`levels/<archive>/palettes`): one per
/// skinned model a placement running a whitelisted script names.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LevelPalettes {
    pub identity: DumpIdentity,
    pub level: String,
    pub palettes: Vec<Palette>,
}

impl Palette {
    /// The entry in `slot`, if it holds one that reads.
    pub fn get(&self, slot: u16) -> Option<&PaletteSlot> {
        self.entries
            .binary_search_by_key(&slot, |e| e.slot)
            .ok()
            .map(|i| &self.entries[i])
    }

    /// The clip in `slot`, if it holds one.
    pub fn clip(&self, slot: u16) -> Option<&Clip> {
        match &self.get(slot)?.entry {
            SlotEntry::Clip { clip, .. } => Some(clip),
            SlotEntry::Network => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::clip::{RotationTrack, Track, VecTrack};
    use crate::{Sha256, from_postcard, from_ron, to_postcard, to_ron};

    fn clip(name: &str, joint: u16) -> Clip {
        Clip {
            name: name.into(),
            duration: 0.3333,
            frame_duration: 1.0 / 30.0,
            frames: 11,
            tracks: vec![Track {
                joint,
                translation: VecTrack {
                    constant: [0.0, 0.25, 0.0],
                    spline: None,
                },
                rotation: RotationTrack::Constant([0.0, 0.0, 0.0, 1.0]),
                scale: VecTrack {
                    constant: [1.0; 3],
                    spline: None,
                },
            }],
        }
    }

    fn palette() -> Palette {
        Palette {
            identity: DumpIdentity {
                title_id: "BLUS30968".into(),
                app_ver: "01.00".into(),
                manifest_hash: Sha256([7; 32]),
            },
            owner: "009_Eruptor".into(),
            slots: 185,
            entries: vec![
                PaletteSlot {
                    slot: 7,
                    name: Some("Network".into()),
                    label: None,
                    entry: SlotEntry::Network,
                },
                PaletteSlot {
                    slot: 20,
                    name: None,
                    label: Some("throw".into()),
                    entry: SlotEntry::Clip {
                        additive: false,
                        partial: true,
                        layered: false,
                        motion_extraction: 3,
                        clip: clip("Eruptor_LavaLob.ma", 2),
                    },
                },
            ],
            refused: vec![RefusedSlot {
                slot: 9,
                reason: "blendHint".into(),
            }],
        }
    }

    #[test]
    fn a_palette_and_a_levels_round_trip_through_both() {
        let p = palette();
        assert_eq!(from_ron::<Palette>(&to_ron(&p).unwrap()).unwrap(), p);
        assert_eq!(
            from_postcard::<Palette>(&to_postcard(&p).unwrap()).unwrap(),
            p
        );
        let level = LevelPalettes {
            identity: p.identity.clone(),
            level: "level_213_tutotial".into(),
            palettes: vec![p.clone(), p],
        };
        assert_eq!(
            from_ron::<LevelPalettes>(&to_ron(&level).unwrap()).unwrap(),
            level
        );
        assert_eq!(
            from_postcard::<LevelPalettes>(&to_postcard(&level).unwrap()).unwrap(),
            level
        );
    }

    #[test]
    fn a_slot_is_found_by_its_number() {
        let p = palette();
        assert_eq!(p.clip(20).unwrap().name, "Eruptor_LavaLob.ma");
        assert_eq!(p.get(7).unwrap().entry, SlotEntry::Network);
        assert!(p.clip(7).is_none());
        // Refused, empty or past the set: nothing.
        for slot in [9, 8, 400] {
            assert!(p.get(slot).is_none(), "{slot}");
        }
    }
}
