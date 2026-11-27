//! A hero's last autoaim as his scripts left it (openspec AS.1042), for a
//! playtest's capture (`capture.json`'s `aim` and `aim_pick`) and the disc
//! tests: the VM reads each of [`AIM_MACROS`]' calls off its frame as the
//! use block closes ([`guac_script::Vm::watch_call`]), so nothing the
//! scripts do changes; [`aim_of`] puts the hero's newest beside the world
//! now and [`AimPicks`] keeps his newest that picked something.

use std::collections::HashMap;

use bevy::math::Vec3;
use guac_script::{CallRecord, Handle, Recorded, Value};
use serde::Serialize;

use crate::script_engine::Actor;
use crate::scripts::ScriptWorld;

/// The macros a hero's autoaim runs, combat's first: the record [`aim_of`]
/// reads is the newest of either his placement made.
pub const AIM_MACROS: [&str; 2] = ["Combat - Autoaim At Targets", "Calc - Autoaim At Actors"];

/// `awake`, `asleep` or `gone`, as a capture names an actor's state.
pub fn state_of(a: &Actor) -> &'static str {
    if a.removed {
        "gone"
    } else if a.active {
        "awake"
    } else {
        "asleep"
    }
}

/// The hero's last autoaim as his scripts left it (openspec AS.1042): the
/// newest of [`AIM_MACROS`]' calls his placement made, read off the frame
/// as its use block closed ([`guac_script::Vm::watch_call`]), so nothing
/// the scripts do changes.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct AimShot {
    /// The macro the record is of.
    pub call: String,
    /// The VM's step it closed on, and how many steps before the capture.
    pub step: u64,
    pub steps_ago: u64,
    /// What it picked: none when it found no target.
    pub target: Option<AimTarget>,
    /// `autoaim info`'s heading and pitch, degrees, and its lob's speed,
    /// ft/s, as the call left them.
    pub heading: Option<f32>,
    pub pitch: Option<f32>,
    pub lobbing_velocity: Option<f32>,
    /// How many targets it had left to choose from (`available targets`).
    pub candidates: Option<usize>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct AimTarget {
    pub handle: u64,
    pub name: String,
    /// Where it stands now, m, its state and behaviour now.
    pub at: Option<[f32; 3]>,
    pub state: Option<&'static str>,
    pub behaviour: Option<String>,
    /// From the hero now, m.
    pub distance: Option<f32>,
}

/// The hero's (`hero`, his host handle) last autoaim among `vm`'s watched
/// calls, read against `world` now; none when he has made none. For a hero
/// whose attack aims again every step it runs (Cynder's breath, which skips
/// the foes it has already hurt), this is mostly one that found nothing:
/// [`AimPicks`] keeps the last that picked.
pub fn aim_of(
    vm: &guac_script::Vm,
    world: &ScriptWorld,
    hero: Handle,
    at: Option<Vec3>,
) -> Option<AimShot> {
    let call = vm
        .watched_calls()
        .filter(|c| c.placement == hero && AIM_MACROS.contains(&c.def.as_str()))
        .last()?;
    Some(shot_of(vm, world, call, at))
}

/// `call`'s `autoaim info` field `name`, as the call left it.
fn info_field(call: &CallRecord, name: &str) -> Option<Value> {
    match call.var("autoaim info") {
        Some(Recorded::Fields(f)) => f
            .iter()
            .find(|(n, _)| n.eq_ignore_ascii_case(name))
            .map(|(_, v)| *v),
        _ => None,
    }
}

/// What `call` picked, if anything.
fn picked(call: &CallRecord) -> Option<Handle> {
    match info_field(call, "target") {
        Some(Value::Entity(h)) => Some(h),
        _ => None,
    }
}

/// Each placement's newest autoaim that picked a target (openspec AS.1042):
/// the VM keeps its last [`guac_script::CALLS_KEPT`] watched calls of every
/// placement, which a breath aiming twice a step fills with calls that
/// found nothing within a fraction of a second, so the TV hands these the
/// new calls every frame and a capture's `aim_pick` reads the hero's.
#[derive(Debug, Default, Clone)]
pub struct AimPicks {
    by: HashMap<Handle, CallRecord>,
    steps: u64,
}

impl AimPicks {
    /// Takes the picks among `calls`, `steps` being the VM's now: fewer
    /// than last time is a new VM, whose handles and steps start over, so
    /// what was kept goes.
    pub fn update<'a>(&mut self, steps: u64, calls: impl IntoIterator<Item = &'a CallRecord>) {
        if steps < self.steps {
            self.by.clear();
        }
        self.steps = steps;
        for c in calls {
            if !AIM_MACROS.contains(&c.def.as_str()) || picked(c).is_none() {
                continue;
            }
            if self.by.get(&c.placement).is_none_or(|old| old.step <= c.step) {
                self.by.insert(c.placement, c.clone());
            }
        }
    }

    /// `placement`'s newest autoaim that picked, read against `world` now.
    pub fn shot(
        &self,
        vm: &guac_script::Vm,
        world: &ScriptWorld,
        placement: Handle,
        at: Option<Vec3>,
    ) -> Option<AimShot> {
        self.by
            .get(&placement)
            .map(|c| shot_of(vm, world, c, at))
    }
}

/// `call` as a capture shows it, its pick read against `world` now.
fn shot_of(
    vm: &guac_script::Vm,
    world: &ScriptWorld,
    call: &CallRecord,
    at: Option<Vec3>,
) -> AimShot {
    let number = |name: &str| info_field(call, name).and_then(|v| v.as_f32());
    let target = match info_field(call, "target") {
        Some(Value::Entity(h)) => {
            let a = world.actor(h);
            Some(AimTarget {
                handle: h,
                name: world.name(h),
                at: a.map(|a| a.position.to_array()),
                state: a.map(state_of),
                behaviour: vm
                    .placement_of(h)
                    .and_then(|id| vm.behaviour(id))
                    .map(str::to_ascii_lowercase),
                distance: at.zip(a).map(|(p, a)| p.distance(a.position)),
            })
        }
        _ => None,
    };
    AimShot {
        call: call.def.clone(),
        step: call.step,
        steps_ago: vm.steps().saturating_sub(call.step),
        target,
        heading: number("heading"),
        pitch: number("pitch"),
        lobbing_velocity: number("lobbing velocity"),
        candidates: match call.var("available targets") {
            Some(Recorded::Members(m)) => Some(m.len()),
            _ => None,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn call(def: &str, placement: Handle, step: u64, target: Value) -> CallRecord {
        CallRecord {
            def: def.to_owned(),
            placement,
            step,
            vars: vec![(
                "autoaim info".to_owned(),
                Recorded::Fields(vec![("target".to_owned(), target)]),
            )],
        }
    }

    /// openspec AS.1042: a placement's newest pick outlives the calls that
    /// found nothing after it (a breath aiming again every step), another
    /// placement's picks stay its own, and a new VM drops them all.
    #[test]
    fn a_heros_last_pick_outlives_his_empty_aims() {
        let mut picks = AimPicks::default();
        let first = [
            call("Combat - Autoaim At Targets", 7, 10, Value::Entity(40)),
            call("Calc - Autoaim At Actors", 7, 11, Value::Entity(41)),
            call("Combat - Autoaim At Targets", 7, 11, Value::Entity(41)),
            call("Combat - Autoaim At Targets", 9, 11, Value::Entity(50)),
            call("Some Other Macro", 7, 12, Value::Entity(60)),
        ];
        picks.update(12, &first);
        let empty: Vec<_> = (13..60)
            .map(|s| call("Combat - Autoaim At Targets", 7, s, Value::Null))
            .collect();
        picks.update(60, &empty);
        assert_eq!(picks.by[&7].step, 11);
        assert_eq!(picked(&picks.by[&7]), Some(41));
        assert_eq!(picks.by[&7].def, "Combat - Autoaim At Targets");
        assert_eq!(picked(&picks.by[&9]), Some(50));
        // Seen again (the VM's ring read every frame), nothing moves.
        picks.update(60, &first);
        assert_eq!(picks.by[&7].step, 11);
        picks.update(5, std::iter::empty());
        assert!(picks.by.is_empty(), "a new VM");
    }
}
