//! The hero's own numbers as the live game holds them (openspec AG.8): his
//! scale, the `CollisionInfo` his placement points at, the `ActorParameters`
//! it points at and his solid as his phantom holds it.
//!
//! The parameters aren't what walks him. In AF.8's fresh state Eruptor's
//! hold 10,000 ft/s, no acceleration or deceleration, 10,000 deg/s and motion
//! type 2 (unconstrained) at rest and through every walk (AG.8, every game
//! frame of 59 O5b recordings); they change, to 21.5 ft/s, 160 and 80 ft/s2,
//! turn 0 and motion type 0, only for the 5 frames an action button plays
//! (AG.1's walk script presses South at F0 + 300). His walk follows the stick
//! by numbers held elsewhere: 21.5 ft/s is `Speed_Slow`'s live value
//! (ah-combat).
//!
//! The placement's offsets are the 01.00 EBOOT's `tfbActorInfo` (AG.3's
//! layouts): `_collision` +0xC4, `_actorParameters` +0xE0, `_scaleVec`
//! +0xB8. A `CollisionInfo` is AG.3's (`_properties` +0x18, `_transform`
//! +0x1C, then width, height, length, weight and rebound from +0x34), an
//! `ActorParameters` AG.3's too (the movement floats from +0x0C, motion type
//! +0x40, tilts +0x44, `_objectTimeScale` +0x48).

use anyhow::{Result, bail};
use serde::{Deserialize, Serialize};

use crate::actors::{ActorFrame, Gather, be32, expect_vtable, f32_at, floats, one, vtable};
use crate::level::{self, LiveSolid};
use crate::pine::PineClient;

const PLACEMENT_LEN: usize = 0xF8;
const PLACEMENT_SCALE: usize = 0xB8;
const PLACEMENT_COLLISION: usize = 0xC4;
const PLACEMENT_PARAMETERS: usize = 0xE0;
const COLLISION_LEN: usize = 0x58;
const PARAMETERS_LEN: usize = 0x4C;

/// The tuning data's feet.
pub const FOOT: f32 = 0.3048;
/// What a solid's sizes lose all round, and the convex radius it gets back
/// (ag-motion's table, AG.7's check against 31 live phantoms).
const SHRINK: f32 = 0.1;

/// A `CollisionInfo`, as stored: sizes in feet, weight in pounds, rebound in
/// percent, the offset in metres before the placement's scale.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Collision {
    pub address: u32,
    pub properties: u32,
    pub position: [f32; 3],
    pub angles: [f32; 3],
    pub width: f32,
    pub height: f32,
    pub length: f32,
    pub weight: f32,
    pub rebound: f32,
}

impl Collision {
    /// Bits 0-2: 0 none, 1 box, 2 capsule, 3 sphere, 4 cylinder, 5 mesh.
    pub fn shape(&self) -> u32 {
        self.properties & 7
    }

    /// For a sphere: its radius and how high its centre sits over the pivot,
    /// metres, at `scale` percent: (w - 0.1) / 2 and w / 2 plus the offset's
    /// y, w in metres times the scale.
    pub fn sphere(&self, scale: f32) -> Option<(f32, f32)> {
        if self.shape() != 3 {
            return None;
        }
        let s = scale / 100.0;
        let w = self.width * FOOT * s;
        Some(((w - SHRINK) / 2.0, w / 2.0 + self.position[1] * s))
    }
}

/// An `ActorParameters`, as stored: feet, feet per second (squared) and
/// degrees per second.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Parameters {
    pub address: u32,
    pub ground_max_speed: f32,
    pub ground_acceleration: f32,
    pub ground_deceleration: f32,
    pub ground_turn_rate: f32,
    pub air_max_speed: f32,
    pub air_acceleration: f32,
    pub air_deceleration: f32,
    pub air_turn_rate: f32,
    pub gravity_max_speed: f32,
    pub gravity_acceleration: f32,
    pub jump_max_speed: f32,
    pub jump_acceleration: f32,
    pub jump_initial_speed: f32,
    pub motion_type: u32,
    pub tilts_with_ground: bool,
    pub object_time_scale: f32,
}

impl Parameters {
    /// The object's whole 0x4C bytes, read at `address`.
    pub fn decode(address: u32, b: &[u8]) -> Self {
        let f = |at| f32_at(b, at);
        Self {
            address,
            ground_max_speed: f(0x0C),
            ground_acceleration: f(0x10),
            ground_deceleration: f(0x14),
            ground_turn_rate: f(0x18),
            air_max_speed: f(0x1C),
            air_acceleration: f(0x20),
            air_deceleration: f(0x24),
            air_turn_rate: f(0x28),
            gravity_max_speed: f(0x2C),
            gravity_acceleration: f(0x30),
            jump_max_speed: f(0x34),
            jump_acceleration: f(0x38),
            jump_initial_speed: f(0x3C),
            motion_type: be32(b, 0x40),
            tilts_with_ground: b[0x44] != 0,
            object_time_scale: f(0x48),
        }
    }
}

/// The hero, read at a pause.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Hero {
    pub name: Option<String>,
    pub corpus: u32,
    pub placement: u32,
    /// `_scaleVec`, percent.
    pub scale: [f32; 3],
    pub collision: Option<Collision>,
    pub parameters: Option<Parameters>,
    /// His phantom's solid, when the world holds one for him.
    pub solid: Option<LiveSolid>,
}

/// The hero of `frame` (the skinned actor a pad drives) and what his
/// placement points at, his solid left out; refused when the parameters'
/// class isn't `ActorParameters`.
pub fn read(g: &mut impl Gather, frame: &ActorFrame) -> Result<Hero> {
    let hero = frame.hero()?.clone();
    let p = one(g, hero.placement, PLACEMENT_LEN, "the hero's placement")?;
    let collision = match be32(&p, PLACEMENT_COLLISION) {
        0 => None,
        at => {
            let c = one(g, at, COLLISION_LEN, "the hero's CollisionInfo")?;
            Some(Collision {
                address: at,
                properties: be32(&c, 0x18),
                position: floats::<3>(&c, 0x1C),
                angles: floats::<3>(&c, 0x28),
                width: f32_at(&c, 0x34),
                height: f32_at(&c, 0x38),
                length: f32_at(&c, 0x3C),
                weight: f32_at(&c, 0x40),
                rebound: f32_at(&c, 0x44),
            })
        }
    };
    let parameters = match be32(&p, PLACEMENT_PARAMETERS) {
        0 => None,
        at => {
            let b = one(g, at, PARAMETERS_LEN, "the hero's ActorParameters")?;
            expect_vtable(&b, vtable::ACTOR_PARAMETERS, at, "ActorParameters")?;
            Some(Parameters::decode(at, &b))
        }
    };
    Ok(Hero {
        name: hero.name.clone(),
        corpus: hero.corpus,
        placement: hero.placement,
        scale: floats::<3>(&p, PLACEMENT_SCALE),
        collision,
        parameters,
        solid: None,
    })
}

/// [`read`] over PINE with his solid through his phantom ([`level::solids`]):
/// Giants 01.00 only, paused throughout.
pub fn read_paused(pine: &mut PineClient, frame: &ActorFrame) -> Result<Hero> {
    let mut hero = read(pine, frame)?;
    let only_hero = ActorFrame {
        actors: frame
            .actors
            .iter()
            .filter(|a| a.corpus == hero.corpus)
            .cloned()
            .collect(),
        ..frame.clone()
    };
    hero.solid = level::read_solids(pine, &only_hero)?.into_iter().next();
    if let Some(s) = &hero.solid
        && s.corpus != hero.corpus
    {
        bail!("the solid read for the hero is {:#010x}'s", s.corpus);
    }
    crate::rng::check_title(pine)?;
    Ok(hero)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::actors::fixture::{Spec, World};
    use crate::actors::read as read_frame;

    /// Eruptor as AF.8's fresh state holds him: scale 150, a 4 x 6 x 3 ft
    /// sphere at 10 lb, and the unconstrained parameters.
    fn eruptor(w: &mut World) -> (u32, u32, u32) {
        let added = w.actor(&Spec {
            name: "009_Eruptor".into(),
            position: [10.19027, 43.38267, -93.39406],
            scale: [150.0; 3],
            skinned: true,
            pad: Some(-1),
            ..Spec::default()
        });
        let collision = w.object(0x0062_0D00, COLLISION_LEN);
        w.put32(collision + 0x18, 0x1B);
        w.put_floats(collision + 0x34, &[4.0, 6.0, 3.0, 10.0, 0.0]);
        w.put32(added.placement + 0xC4, collision);
        let params = w.object(0x0062_0CA8, PARAMETERS_LEN);
        w.put_floats(
            params + 0x0C,
            &[
                10_000.0, 0.0, 0.0, 10_000.0, 10_000.0, 0.0, 0.0, 10_000.0, 0.0, 0.0, 0.0, 0.0, 0.0,
            ],
        );
        w.put32(params + 0x40, 2);
        w.put32(params + 0x44, 0x0100_0000);
        w.putf(params + 0x48, 1.0);
        w.put32(added.placement + 0xE0, params);
        (added.placement, collision, params)
    }

    #[test]
    fn the_heros_scale_collision_and_parameters_read_field_for_field() {
        let mut w = World::new();
        w.actor(&Spec {
            name: "Sheep_1".into(),
            skinned: true,
            ..Spec::default()
        });
        let (placement, collision, params) = eruptor(&mut w);
        let frame = read_frame(&mut w).unwrap();
        let hero = read(&mut w, &frame).unwrap();
        assert_eq!(hero.name.as_deref(), Some("009_Eruptor"));
        assert_eq!(hero.placement, placement);
        assert_eq!(hero.scale, [150.0; 3]);
        let c = hero.collision.unwrap();
        assert_eq!((c.address, c.properties, c.shape()), (collision, 0x1B, 3));
        assert_eq!(
            (c.width, c.height, c.length, c.weight, c.rebound),
            (4.0, 6.0, 3.0, 10.0, 0.0)
        );
        // AG.7's live phantom: radius 0.8644 m, centre 0.9144 m up.
        let (r, up) = c.sphere(hero.scale[1]).unwrap();
        assert!(
            (r - 0.8644).abs() < 1e-6 && (up - 0.9144).abs() < 1e-6,
            "{r} {up}"
        );
        let p = hero.parameters.unwrap();
        assert_eq!(p.address, params);
        assert_eq!(
            (
                p.ground_max_speed,
                p.ground_acceleration,
                p.ground_deceleration,
                p.ground_turn_rate
            ),
            (10_000.0, 0.0, 0.0, 10_000.0)
        );
        assert_eq!((p.air_max_speed, p.air_turn_rate), (10_000.0, 10_000.0));
        assert_eq!(
            (p.motion_type, p.tilts_with_ground, p.object_time_scale),
            (2, true, 1.0)
        );
        // A box isn't a sphere.
        assert!(
            Collision {
                properties: 0x29,
                ..c
            }
            .sphere(150.0)
            .is_none()
        );
    }

    #[test]
    fn a_parameters_pointer_to_another_class_is_refused_and_null_ones_are_none() {
        let mut w = World::new();
        let (placement, _, params) = eruptor(&mut w);
        w.put32(params, 0x0062_0000);
        let frame = read_frame(&mut w).unwrap();
        assert!(read(&mut w, &frame).is_err());
        w.put32(placement + 0xE0, 0);
        w.put32(placement + 0xC4, 0);
        let hero = read(&mut w, &frame).unwrap();
        assert!(hero.parameters.is_none() && hero.collision.is_none());
    }
}
