//! The hero side of a character model (openspec AG.14, design decisions 8
//! and 14): what walking the character takes beside AE's model and idle.
//! Per character load file, the placement its archive plays the character
//! as, with its solid and disc movement parameters, and the clips in its
//! animation set's locomotion slots, as spline data on the model's
//! skeleton ([`crate::clip`]).
//!
//! A slot left empty on the disc is `None`: 7 of the 67 characters have no
//! walk and 8 Giants no run, and only 23 a landing. Which of the clips the
//! character's locomotion uses, and at what speeds, is its scripts' (AG.30
//! picks them meanwhile; stand-in ledger: `locomotion`).

use serde::{Deserialize, Serialize};

use crate::DumpIdentity;
use crate::actor::{Movement, Solid};
use crate::clip::Clip;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Hero {
    pub identity: DumpIdentity,
    /// The roster's load file (`009_Eruptor`), the model's key too.
    pub load_file: String,
    /// The placement's name (`009_Eruptor`).
    pub actor: String,
    /// Per axis in percent, as stored (100 on every hero on the disc; a
    /// script sets each character's default scale).
    pub scale: [f32; 3],
    pub solid: Solid,
    /// The disc's base, which the game's tuning scripts override.
    pub movement: Movement,
    pub locomotion: Locomotion,
}

/// The locomotion slots' clips, on the model's skeleton.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Locomotion {
    pub walk: Option<SlotClip>,
    pub jog: Option<SlotClip>,
    pub run: Option<SlotClip>,
    pub falling: Option<SlotClip>,
    /// Played over the others as the hero touches down.
    pub landing: Option<SlotClip>,
}

/// What a locomotion slot is for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub enum Gait {
    Walk,
    Jog,
    Run,
    Falling,
    Landing,
}

impl Gait {
    pub const ALL: [Gait; 5] = [
        Gait::Walk,
        Gait::Jog,
        Gait::Run,
        Gait::Falling,
        Gait::Landing,
    ];
}

/// A clip with the animation-set slot it came from and the slot's stored
/// label.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SlotClip {
    /// Its index in the model's animation set.
    pub slot: u16,
    /// The slot's own label as stored, if it has one (`3.658 m/sec` on
    /// Eruptor's walk).
    pub label: Option<String>,
    pub clip: Clip,
}

impl Locomotion {
    pub fn get(&self, gait: Gait) -> Option<&SlotClip> {
        match gait {
            Gait::Walk => self.walk.as_ref(),
            Gait::Jog => self.jog.as_ref(),
            Gait::Run => self.run.as_ref(),
            Gait::Falling => self.falling.as_ref(),
            Gait::Landing => self.landing.as_ref(),
        }
    }

    pub fn get_mut(&mut self, gait: Gait) -> &mut Option<SlotClip> {
        match gait {
            Gait::Walk => &mut self.walk,
            Gait::Jog => &mut self.jog,
            Gait::Run => &mut self.run,
            Gait::Falling => &mut self.falling,
            Gait::Landing => &mut self.landing,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::actor::Shape;
    use crate::clip::{RotationTrack, Track, VecTrack};
    use crate::{Sha256, from_postcard, from_ron, to_postcard, to_ron};

    fn slot(slot: u16, label: Option<&str>, name: &str) -> SlotClip {
        SlotClip {
            slot,
            label: label.map(Into::into),
            clip: Clip {
                name: name.into(),
                duration: 1.0,
                frame_duration: 1.0 / 30.0,
                frames: 31,
                tracks: vec![Track {
                    joint: 0,
                    translation: VecTrack {
                        constant: [0.0, 1.0, 0.0],
                        spline: None,
                    },
                    rotation: RotationTrack::Constant([0.0, 0.0, 0.0, 1.0]),
                    scale: VecTrack {
                        constant: [1.0; 3],
                        spline: None,
                    },
                }],
            },
        }
    }

    /// Eruptor's numbers as the disc stores them, his falling clip missing a
    /// label and no jog, run or landing.
    fn hero() -> Hero {
        Hero {
            identity: crate::DumpIdentity {
                title_id: "BLUS30968".into(),
                app_ver: "01.00".into(),
                manifest_hash: Sha256([9; 32]),
            },
            load_file: "009_Eruptor".into(),
            actor: "009_Eruptor".into(),
            scale: [100.0; 3],
            solid: Solid {
                shape: Shape::Sphere,
                solid: true,
                world: true,
                actor_surface: false,
                offset: [0.0; 3],
                rotation: [0.0; 3],
                width: 4.0,
                height: 6.0,
                length: 3.0,
                weight: 10.0,
                rebound: 0.0,
            },
            movement: Movement {
                ground_max_speed: 18.0,
                ground_acceleration: 160.0,
                ground_deceleration: 80.0,
                ground_turn_rate: 360.0,
                air_max_speed: 30.0,
                air_acceleration: 160.0,
                air_deceleration: 20.0,
                air_turn_rate: 360.0,
                gravity_max_speed: 120.0,
                gravity_acceleration: 128.0,
                jump_max_speed: 0.0,
                jump_acceleration: 0.0,
                jump_initial_speed: 0.0,
                motion_type: 0,
                tilts_with_ground: true,
                time_scale: 1.0,
            },
            locomotion: Locomotion {
                walk: Some(slot(1, Some("3.658 m/sec"), "Eruptor_Walk.ma")),
                jog: None,
                run: None,
                falling: Some(slot(57, None, "Eruptor_Falling.ma")),
                landing: None,
            },
        }
    }

    #[test]
    fn a_hero_round_trips_through_both_formats() {
        let h = hero();
        assert_eq!(from_ron::<Hero>(&to_ron(&h).unwrap()).unwrap(), h);
        assert_eq!(from_postcard::<Hero>(&to_postcard(&h).unwrap()).unwrap(), h);
    }

    #[test]
    fn a_gait_names_its_slot_in_the_locomotion() {
        let mut h = hero();
        let present: Vec<Gait> = Gait::ALL
            .into_iter()
            .filter(|&g| h.locomotion.get(g).is_some())
            .collect();
        assert_eq!(present, [Gait::Walk, Gait::Falling]);
        assert_eq!(h.locomotion.get(Gait::Walk).unwrap().slot, 1);
        *h.locomotion.get_mut(Gait::Landing) = Some(slot(61, None, "Eruptor_Landing.ma"));
        assert_eq!(h.locomotion.landing.as_ref().unwrap().slot, 61);
    }
}
