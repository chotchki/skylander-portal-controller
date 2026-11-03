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
//! Chapter 1's cave, as AH.10's oracle run walked into it, lands the hero
//! 1.35 s after his entry (the controller's behaviour 2 for 79 frames there,
//! 81 steps here), 0.84 m from the marker his warp aims at (the portal's
//! re-entry, unread), facing the room's way.
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
    controllers(vm, world)
        .filter_map(|id| vm.local(id, "darkness alpha mask"))
        .filter_map(|v| model.sprite_of(vm.value(v)))
        .map(|s| model.sprite(s))
        .filter(|s| {
            s.state != guac_interface::sprite::State::Removed
                && s.properties & guac_interface::sprite::props::VISIBLE != 0
        })
        .map(|s| 1.0 - f32::from(s.color[3]) / 255.0)
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
