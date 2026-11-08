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
//! ones, so [`crate::IR_VERSION`] holds. [`SlotEntry::Blocks`] (openspec
//! AS.78) and [`SlotEntry::Random`] (AS.180) are variants after the others:
//! every file written before them reads unchanged, so it holds that too.

use serde::{Deserialize, Serialize};

use crate::DumpIdentity;
use crate::clip::{Blocks, Clip};

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
    /// A clip past one Havok block (openspec AS.78), with the same flags as
    /// [`SlotEntry::Clip`].
    Blocks {
        additive: bool,
        partial: bool,
        layered: bool,
        motion_extraction: u8,
        clip: Blocks,
    },
    /// A random animation network (openspec AS.180): the clips the original
    /// chooses among each time the slot plays, in the network's order. Every
    /// network on the levels that play is one (a Mabu's idle, its talk, a
    /// hero's or a Chompy's hurt), its clips alike in their params.
    Random { choices: Vec<Choice> },
}

/// One clip a random network chooses among, with the same flags as
/// [`SlotEntry::Clip`] and its row of the network's params as stored (5
/// floats, `1, 0, 0, 1, 1` on every network the levels that play hold;
/// what they mean is unread).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Choice {
    pub additive: bool,
    pub partial: bool,
    pub layered: bool,
    pub motion_extraction: u8,
    pub clip: Clip,
    pub params: Vec<f32>,
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

    /// The clip in `slot`, if it holds one of a single block: a random
    /// network's none, as what a locomotion weights it never is.
    pub fn clip(&self, slot: u16) -> Option<&Clip> {
        match &self.get(slot)?.entry {
            SlotEntry::Clip { clip, .. } => Some(clip),
            SlotEntry::Network | SlotEntry::Blocks { .. } | SlotEntry::Random { .. } => None,
        }
    }

    /// What `slot`'s clip poses `t` seconds in: the clip and `t`, or for one
    /// past a block, the block playing then and the time within it; for a
    /// random network, its first clip, the one a play of it plays here
    /// ([`Palette::duration`]).
    pub fn clip_at(&self, slot: u16, t: f64) -> Option<(&Clip, f64)> {
        match &self.get(slot)?.entry {
            SlotEntry::Clip { clip, .. } => Some((clip, t)),
            SlotEntry::Blocks { clip, .. } => clip.at(t),
            SlotEntry::Random { choices } => choices.first().map(|c| (&c.clip, t)),
            SlotEntry::Network => None,
        }
    }

    /// `slot`'s clip's length in seconds, of one block or more, a random
    /// network's first clip's (where the original picks one of them each
    /// play); none for a network that doesn't read or a slot that doesn't.
    pub fn duration(&self, slot: u16) -> Option<f32> {
        match &self.get(slot)?.entry {
            SlotEntry::Clip { clip, .. } => Some(clip.duration),
            SlotEntry::Blocks { clip, .. } => Some(clip.duration),
            SlotEntry::Random { choices } => choices.first().map(|c| c.clip.duration),
            SlotEntry::Network => None,
        }
    }

    /// What `slot` poses at rest `t` seconds in, looping: its clip at `t`
    /// within its length; a random network's clips each played through in
    /// turn, in their order (each alike in its params, so in turn stands in
    /// for the original's pick at random). None as [`Palette::clip_at`]'s.
    pub fn looped_at(&self, slot: u16, t: f64) -> Option<(&Clip, f64)> {
        let within = |d: f32, t: f64| {
            if d > 0.0 {
                t.rem_euclid(f64::from(d))
            } else {
                0.0
            }
        };
        match &self.get(slot)?.entry {
            SlotEntry::Random { choices } => {
                let all: f64 = choices.iter().map(|c| f64::from(c.clip.duration)).sum();
                let mut t = if all > 0.0 { t.rem_euclid(all) } else { 0.0 };
                for c in choices {
                    let d = f64::from(c.clip.duration);
                    if t < d {
                        return Some((&c.clip, t));
                    }
                    t -= d;
                }
                choices
                    .last()
                    .map(|c| (&c.clip, f64::from(c.clip.duration)))
            }
            _ => {
                let d = self.duration(slot)?;
                self.clip_at(slot, within(d, t))
            }
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
                PaletteSlot {
                    slot: 41,
                    name: None,
                    label: None,
                    entry: SlotEntry::Random {
                        choices: ["Eruptor_DamageFrontv00.ma", "Eruptor_DamageFrontv01.ma"]
                            .map(|name| Choice {
                                additive: false,
                                partial: true,
                                layered: false,
                                motion_extraction: 0,
                                clip: Clip {
                                    duration: if name.ends_with("0.ma") { 0.5 } else { 0.25 },
                                    ..clip(name, 3)
                                },
                                params: vec![1.0, 0.0, 0.0, 1.0, 1.0],
                            })
                            .to_vec(),
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

    /// openspec AS.180: a random network plays its first clip, that clip's
    /// length; at rest its clips play through in turn, looping; it's no
    /// locomotion's clip. A clip loops within its own length; a network
    /// that doesn't read plays nothing.
    #[test]
    fn a_random_network_plays_its_first_clip_and_rests_on_each_in_turn() {
        let p = palette();
        let at = |slot, t| p.clip_at(slot, t).map(|(c, t)| (c.name.as_str(), t));
        assert_eq!(at(41, 0.1), Some(("Eruptor_DamageFrontv00.ma", 0.1)));
        assert_eq!(p.duration(41), Some(0.5));
        assert!(p.clip(41).is_none());
        let rest = |slot, t| p.looped_at(slot, t).map(|(c, t)| (c.name.as_str(), t));
        assert_eq!(rest(41, 0.1), Some(("Eruptor_DamageFrontv00.ma", 0.1)));
        let (name, t) = rest(41, 0.6).unwrap();
        assert_eq!(name, "Eruptor_DamageFrontv01.ma");
        assert!((t - 0.1).abs() < 1e-9, "{t}");
        let (name, t) = rest(41, 0.85).unwrap();
        assert_eq!(name, "Eruptor_DamageFrontv00.ma", "round again");
        assert!((t - 0.1).abs() < 1e-9, "{t}");
        let (name, t) = rest(20, 0.5).unwrap();
        assert_eq!(name, "Eruptor_LavaLob.ma");
        assert!((t - (0.5 - 0.3333)).abs() < 1e-6, "{t}");
        assert!(rest(7, 0.1).is_none() && p.duration(7).is_none());
    }
}
