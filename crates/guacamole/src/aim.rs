//! A hero's last autoaim as his scripts left it (openspec AS.1042), for a
//! playtest's capture (`capture.json`'s `aim`) and the disc tests: the VM
//! reads each of [`AIM_MACROS`]' calls off its frame as the use block
//! closes ([`guac_script::Vm::watch_call`]), so nothing the scripts do
//! changes, and [`aim_of`] puts the hero's newest beside the world now.

use bevy::math::Vec3;
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
/// calls, read against `world` now; none when he has made none.
pub fn aim_of(
    vm: &guac_script::Vm,
    world: &ScriptWorld,
    hero: guac_script::Handle,
    at: Option<Vec3>,
) -> Option<AimShot> {
    use guac_script::{Recorded, Value};
    let call = vm
        .watched_calls()
        .filter(|c| c.placement == hero && AIM_MACROS.contains(&c.def.as_str()))
        .last()?;
    let info = match call.var("autoaim info") {
        Some(Recorded::Fields(f)) => f.as_slice(),
        _ => &[],
    };
    let field = |name: &str| {
        info.iter()
            .find(|(n, _)| n.eq_ignore_ascii_case(name))
            .map(|(_, v)| *v)
    };
    let number = |name: &str| field(name).and_then(|v| v.as_f32());
    let target = match field("target") {
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
    Some(AimShot {
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
    })
}
