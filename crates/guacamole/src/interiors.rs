//! The interiors (openspec AS.96): a house or cave whose controller
//! (`Interior_Controller.ai`, [`crate::scripts::INTERIOR_SCRIPTS`]) takes a
//! hero walking into its box to a room elsewhere in the level and back. As
//! its script has it, on chapter 1's cave and level 214's houses alike:
//!
//! - **The way in.** While no other interior holds the level (`Global` 372,
//!   the one in use) and no player is dying, a hero inside the controller's
//!   box (its `CollisionInfo`, 26 × 15 × 16 ft on chapter 1's cave) starts
//!   it; a Giant too big for a house enters by facing its door from 6 ft past
//!   the box's length instead (its field-of-view check), unless the
//!   placement's `disable_fov` tag says not.
//! - **The darkness.** The screen goes dark over 0.5 s: a grey square
//!   stretched over the screen whose blend cuts the level's darkness sprite
//!   out of it, its alpha slid 255 to 0, so less of the darkness is cut out
//!   as it goes ([`darkness`]); a cutscene holds the level (no switching,
//!   the HUD hidden) and the entry's stinger plays.
//! - **The warp.** Dark, it clones a marker 1 ft behind the next of its
//!   waypoints (its facing turned about: the room's arrival), another 7 ft
//!   on for a second player, and asks the portal to warp each player there
//!   (`Portal - Warp Player to Location`, no effects, no portal entry, in
//!   mid-air, during cutscenes); its interior, enemy and loot sets stay
//!   awake; at the waypoint after that it clones the exit's box (an 8 × 12 ft
//!   cylinder unless its `exit collision` tag gives another shape).
//! - **The way back to light.** 0.3 s on, the markers go, the level
//!   switches to the interior's atmospheric zone and camera (its `interior
//!   atmospherics` and `interior camera` tags), and the darkness lifts over
//!   0.5 s; the cutscene ends and it waits for the exit.
//! - **The way out.** A hero walking into the exit's box goes the same way
//!   back to the waypoint after that one (round to the first: outside the
//!   door), the exterior's zone and camera, the box taken off.
//!
//! Chapter 1's cave, as AH.10's oracle run walked into it: the hero lands
//! 0.7 s after the darkness begins (42 frames there, 42 steps here), facing
//! the room's way, ours on the marker his warp aims at and the original's
//! 0.84 m from it (its re-entry's place, unread); the controller waits for
//! his exit 1.35 s after it began (79 frames there, 81 steps here).
//!
//! What's ours: the warp. The original's portal manager takes the minion
//! off the portal (`Portal - Exit Object`) and, 0.2 s on, puts him back at
//! the marker; here the macro is answered ([`answer`]) and the hero is set
//! down at the marker 0.2 s on, his minion kept (stand-in ledger: `warp`).
//! The atmospheric zone the switch names isn't drawn yet (AG.155: the TV
//! keeps the level's start zone), and the darkness is drawn as a black
//! screen of that alpha, the mask's own art not drawn ([`darkness`]).

use guac_motion::seam::Vec3;
use guac_script::{Handle, PlacementId, Value, Vm};
use tracing::{info, warn};

use crate::script_engine::Engine;
use crate::scripts::ScriptWorld;

/// The macro that asks the portal to warp a player (`global.bld`'s
/// `Spyro_PortalEntry_Macros.ai`), which the TV answers ([`answer`]).
pub const WARP: &str = "Portal - Warp Player to Location";

/// The least delay the original's warp takes before the minion is back:
/// `Portal - Warp Player to Location` raises any shorter one to 0.2 s.
pub const WARP_DELAY: f32 = 0.2;

/// The controllers' script, matched as a path's end: every level's file
/// links its own copy under its own path.
pub const CONTROLLER: &str = "Interior_Controller.ai";

/// A warp the TV owes: the minion (an actor or a hero, by handle), where
/// to (what the macro's target location named) and the game time left
/// until it lands.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Warp {
    pub minion: Handle,
    pub target: Value,
    pub left: f32,
}

/// A warp landed: who, where and facing which way, and the step it landed
/// on (the world's engine frames).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Landed {
    pub minion: Handle,
    pub at: Vec3,
    pub facing: f32,
    pub frame: u64,
}

/// A parameter of an answered call by name, ignoring case and the spaces
/// round it (the warp's own `minion ` carries one).
fn arg(args: &[(&str, Value)], name: &str) -> Option<Value> {
    args.iter()
        .find(|(n, _)| n.trim().eq_ignore_ascii_case(name))
        .map(|(_, v)| *v)
}

/// A macro the TV answers in place of its body (`guac_script::Config::
/// answered`): the warp, by name. A warp of a minion that names nothing
/// (the second player's, with no second player) is answered as done; one of
/// an actor is owed, landing [`WARP_DELAY`] or its own delay on, whichever
/// is longer (stand-in ledger: `warp`). Anything else runs its body.
pub fn answer(world: &mut ScriptWorld, name: &str, args: &[(&str, Value)]) -> bool {
    if !name.eq_ignore_ascii_case(WARP) {
        return false;
    }
    let minion = arg(args, "minion").unwrap_or(Value::Null);
    let target = arg(args, "target location").unwrap_or(Value::Null);
    let delay = arg(args, "delay")
        .and_then(|v| v.as_f32())
        .unwrap_or(0.0)
        .max(WARP_DELAY);
    let Some(h) = world.actor_of(Some(minion)) else {
        return true;
    };
    info!(
        minion = h,
        ?target,
        delay,
        "a warp asked: the minion lands at its target"
    );
    world.warps.retain(|w| w.minion != h);
    world.warps.push(Warp {
        minion: h,
        target,
        left: delay,
    });
    true
}

/// The warps owed `dt` seconds on: each due one landed, the minion set down
/// at its target facing as the target does, a hero at rest, as a script's
/// `teleport to` sets him down (stand-in ledger: `warp`). One whose target
/// names no place by then is dropped, logged.
pub(crate) fn land(world: &mut ScriptWorld, dt: f32, engine: &mut dyn Engine) {
    if world.warps.is_empty() {
        return;
    }
    for w in &mut world.warps {
        w.left -= dt;
    }
    let (due, owed): (Vec<Warp>, Vec<Warp>) = world.warps.drain(..).partition(|w| w.left <= 0.0);
    world.warps = owed;
    for w in due {
        let Some(at) = world.point(w.target, engine) else {
            warn!(minion = w.minion, target = ?w.target, "a warp's target names no place: it lands nowhere");
            continue;
        };
        let facing = facing_of(world, w.target).unwrap_or(0.0).rem_euclid(360.0);
        if let Some(hero) = engine.hero(w.minion) {
            hero.pivot = at;
            hero.facing = facing;
            hero.walk.velocity = Vec3::ZERO;
        } else if let Some(a) = world.actors.get_mut(&w.minion) {
            a.position = at;
            a.angles.facing = facing;
        } else {
            continue;
        }
        info!(minion = w.minion, ?at, facing, "a warp landed");
        world.landed.push(Landed {
            minion: w.minion,
            at,
            facing,
            frame: world.sound_clock,
        });
    }
}

/// The way `target` faces: an actor's facing, a file waypoint's turn as the
/// scripts last left it.
fn facing_of(world: &ScriptWorld, target: Value) -> Option<f32> {
    if let Some(h) = world.actor_of(Some(target)) {
        return world.actors.get(&h).map(|a| a.angles.facing);
    }
    match target {
        Value::Object(o) => world.point_turns.get(&o).map(|t| t.facing),
        _ => None,
    }
}

/// How dark the level's interiors leave the screen, 0 clear to 1 black:
/// each controller's darkness while one is up (its `darkness alpha mask`, a
/// sprite of the run's interface model, visible and not taken off), the
/// screen darkened by 1 - its alpha / 255, the darkest of them; 0 with no
/// interface model, whose run spawns no mask.
pub fn darkness(vm: &Vm, world: &ScriptWorld) -> f32 {
    let Some(keeper) = world.interface() else {
        return 0.0;
    };
    let model = keeper.model();
    darkest(
        controllers(vm, world)
            .filter_map(|id| vm.local(id, "darkness alpha mask"))
            .filter_map(|v| model.sprite_of(vm.value(v)))
            .map(|s| model.sprite(s))
            .map(|s| {
                let shows = s.state != guac_interface::sprite::State::Removed
                    && s.properties & guac_interface::sprite::props::VISIBLE != 0;
                (shows, s.color[3])
            }),
    )
}

/// The darkest of the masks, each whether it shows (not taken off, visible)
/// and its alpha: one that shows darkens by 1 - alpha / 255, one that
/// doesn't by nothing.
fn darkest(masks: impl Iterator<Item = (bool, u8)>) -> f32 {
    masks
        .filter(|(shows, _)| *shows)
        .map(|(_, alpha)| 1.0 - f32::from(alpha) / 255.0)
        .fold(0.0, f32::max)
}

/// Each file placement of [`CONTROLLER`] the VM runs (one off the
/// listing, a template, runs none).
fn controllers<'a>(vm: &'a Vm, world: &'a ScriptWorld) -> impl Iterator<Item = PlacementId> + 'a {
    world
        .templates
        .values()
        .filter(|p| p.script.as_deref().is_some_and(|s| s.ends_with(CONTROLLER)))
        .filter_map(|p| world.objects.get(&p.obj))
        .filter_map(|&h| vm.placement_of(h))
}

/// Outside the door of the interior whose controller `unit`'s file places
/// as `name` (the dev entry's interior anchor, openspec AS.96): its first
/// waypoint, where its way out sets a hero down, and the heading from there
/// to the controller, the way into its box; none for a name no controller
/// of that file goes by, or one with no waypoint.
pub fn door(world: &ScriptWorld, unit: u16, name: &str) -> Option<(Vec3, f32)> {
    let p = world.templates.values().find(|p| {
        p.obj.unit == unit
            && p.name == name
            && p.script.as_deref().is_some_and(|s| s.ends_with(CONTROLLER))
    })?;
    let at = *world.points.get(p.waypoints.first()?)?;
    let to = Vec3::from_array(p.position) - at;
    Some((at, to.x.atan2(to.z).to_degrees().rem_euclid(360.0)))
}

/// The interiors a run's level holds and what each is doing, for
/// `/api/_test/tv`: each controller's name and behaviour.
pub fn states(vm: &Vm, world: &ScriptWorld) -> Vec<(String, Option<String>)> {
    let mut out: Vec<(String, Option<String>)> = world
        .templates
        .values()
        .filter(|p| p.script.as_deref().is_some_and(|s| s.ends_with(CONTROLLER)))
        .filter_map(|p| {
            let id = vm.placement_of(*world.objects.get(&p.obj)?)?;
            Some((p.name.clone(), vm.behaviour(id).map(str::to_owned)))
        })
        .collect();
    out.sort();
    out
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use guac_motion::hero::Hero;
    use guac_motion::parry::ParryWorld;
    use guac_motion::seam::{BodyId, PhysicsSeam};
    use guac_motion::tuning::{Tuning, eruptor_solid};
    use guac_script::Obj;

    use super::*;
    use crate::script_engine::{Actor, Pad};
    use crate::scripts::{Files, Placed, headless_handles, headless_objects};

    const HERO: Handle = 0x6200_0001;
    const MARKER: Handle = 0x6200_0002;
    const CONTROLLER_AT: [f32; 3] = [10.0, 0.0, 10.0];
    const DOOR: Obj = Obj { unit: 4, index: 90 };
    const ROOM: Obj = Obj { unit: 4, index: 91 };

    /// One hero on nothing: what a warp moves.
    struct One {
        seam: ParryWorld,
        hero: Hero,
    }

    impl Engine for One {
        fn hero(&mut self, h: Handle) -> Option<&mut Hero> {
            (h == HERO).then_some(&mut self.hero)
        }
        fn heroes(&self) -> Vec<Handle> {
            vec![HERO]
        }
        fn seam(&self) -> &dyn PhysicsSeam {
            &self.seam
        }
        fn view_heading(&self) -> f32 {
            0.0
        }
        fn pad(&self, _: usize) -> Pad {
            Pad::default()
        }
        fn body(&self, _: Handle) -> Option<BodyId> {
            None
        }
        fn actor_of(&self, _: BodyId) -> Option<Handle> {
            None
        }
    }

    fn one() -> One {
        let mut hero = Hero::new(eruptor_solid(), Tuning::ERUPTOR, Vec3::ZERO, 45.0);
        hero.walk.velocity = Vec3::new(1.0, 0.0, 0.0);
        One {
            seam: ParryWorld::new(&[]).unwrap(),
            hero,
        }
    }

    /// A level with one interior controller (its door its first waypoint,
    /// 2 m south of it, its room its second, far off) and a template of the
    /// same script under another name, and a marker actor at (3, 1, -4)
    /// facing 90.
    fn world() -> ScriptWorld {
        let controller = |name: &str, index: u32, state: u32| {
            let mut p = Placed::named(
                Obj { unit: 4, index },
                name,
                Some("C:/tfb/Content/Levels/Includes/Interiors/Scripts/Interior_Controller.ai"),
                state,
            );
            p.position = CONTROLLER_AT;
            p
        };
        let mut house = controller("Interior_Controller_House", 1, 4);
        house.waypoints = vec![DOOR, ROOM];
        let files = Files {
            placed: vec![
                house,
                controller("Interior_controller_Template", 2, 5),
                Placed::named(Obj { unit: 4, index: 3 }, "Not_An_Interior", None, 4),
            ],
            points: HashMap::from([
                (DOOR, Vec3::new(10.0, 0.0, 8.0)),
                (ROOM, Vec3::new(400.0, -300.0, 200.0)),
            ]),
            ..Files::default()
        };
        let handles = headless_handles(&files);
        let mut w = ScriptWorld::new(&files, &handles, headless_objects(&handles));
        let mut marker = Actor::blank();
        marker.position = Vec3::new(3.0, 1.0, -4.0);
        marker.angles.facing = 90.0;
        w.actors.insert(MARKER, marker);
        w
    }

    fn warp(minion: Value, target: Value, delay: f32) -> [(&'static str, Value); 3] {
        [
            ("delay", Value::Float(delay)),
            // The macro's own parameter carries a space after its name.
            ("minion ", minion),
            ("target location", target),
        ]
    }

    /// openspec AS.96 (stand-in ledger: `warp`): a warp asked of the hero
    /// is answered and owed; it lands once its 0.2 s are out (a shorter
    /// delay raised to it, as the macro raises one), setting him down at
    /// its target facing as the target does, at rest, and is gone.
    #[test]
    fn a_warp_lands_its_minion_at_its_target_once_its_delay_is_out() {
        let (mut w, mut e) = (world(), one());
        assert!(answer(
            &mut w,
            WARP,
            &warp(Value::Entity(HERO), Value::Entity(MARKER), 0.0)
        ));
        assert_eq!(w.warps.len(), 1);
        for _ in 0..11 {
            land(&mut w, 1.0 / 60.0, &mut e);
        }
        assert_eq!(e.hero.pivot, Vec3::ZERO, "not before its 0.2 s");
        assert!(w.landed.is_empty());
        land(&mut w, 1.0 / 60.0 + 1e-4, &mut e);
        assert_eq!(e.hero.pivot, Vec3::new(3.0, 1.0, -4.0));
        assert_eq!(e.hero.facing, 90.0);
        assert_eq!(e.hero.walk.velocity, Vec3::ZERO);
        assert!(w.warps.is_empty());
        assert_eq!(w.landed.len(), 1);
        assert_eq!(w.landed[0].at, Vec3::new(3.0, 1.0, -4.0));
        // A longer delay is its own.
        let (mut w, mut e) = (world(), one());
        answer(
            &mut w,
            WARP,
            &warp(Value::Entity(HERO), Value::Entity(MARKER), 0.5),
        );
        land(&mut w, 0.4, &mut e);
        assert_eq!(e.hero.pivot, Vec3::ZERO);
        land(&mut w, 0.11, &mut e);
        assert_eq!(e.hero.pivot, Vec3::new(3.0, 1.0, -4.0));
    }

    /// A warp of no one (the second player's with none) is answered with
    /// nothing owed; a second warp of the same minion replaces the first;
    /// one whose target names no place by then lands nowhere and goes; any
    /// other macro isn't answered.
    #[test]
    fn a_warp_of_no_one_owes_nothing_and_a_targetless_one_lands_nowhere() {
        let (mut w, mut e) = (world(), one());
        assert!(answer(
            &mut w,
            WARP,
            &warp(Value::Null, Value::Entity(MARKER), 0.0)
        ));
        assert!(w.warps.is_empty());
        assert!(!answer(&mut w, "Portal - Exit Object", &[]));
        assert!(answer(
            &mut w,
            "portal - warp player to location",
            &warp(Value::Entity(HERO), Value::Entity(MARKER), 0.0)
        ));
        assert!(answer(
            &mut w,
            WARP,
            &warp(Value::Entity(HERO), Value::Null, 0.0)
        ));
        assert_eq!(w.warps.len(), 1, "the second replaces the first");
        land(&mut w, 1.0, &mut e);
        assert!(w.warps.is_empty() && w.landed.is_empty());
        assert_eq!(e.hero.pivot, Vec3::ZERO);
    }

    /// The dev entry's interior anchor: outside the named controller's
    /// door (its first waypoint), facing its box; a template of the same
    /// script under its own name, a name no controller goes by and a
    /// placement of another script give none.
    #[test]
    fn the_door_of_an_interior_is_its_first_waypoint_facing_its_box() {
        let w = world();
        let (at, facing) = door(&w, 4, "Interior_Controller_House").unwrap();
        assert_eq!(at, Vec3::new(10.0, 0.0, 8.0));
        assert!(facing.abs() < 1e-4, "{facing}");
        assert_eq!(door(&w, 4, "Interior_controller_Template"), None);
        assert_eq!(door(&w, 4, "Not_An_Interior"), None);
        assert_eq!(door(&w, 3, "Interior_Controller_House"), None);
    }

    /// The darkness: the darkest mask that shows, 1 - alpha / 255; one
    /// taken off or hidden darkens nothing, nor does no mask at all.
    #[test]
    fn the_darkest_mask_that_shows_darkens_the_screen() {
        assert_eq!(darkest([].into_iter()), 0.0);
        assert_eq!(darkest([(true, 255)].into_iter()), 0.0);
        assert_eq!(darkest([(true, 0)].into_iter()), 1.0);
        assert_eq!(darkest([(false, 0)].into_iter()), 0.0);
        let d = darkest([(true, 204), (false, 0), (true, 51)].into_iter());
        assert!((d - 0.8).abs() < 1e-6, "{d}");
    }
}
