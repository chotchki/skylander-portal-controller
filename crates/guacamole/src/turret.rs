//! A turret's game on the TV's run (openspec AS.76): level 214's two turrets
//! run on the VM from their activators (`Turret_1_Base.ai`, `Turret_2_Base.ai`
//! and `global.bld`'s `commonTurretMacros.ai`), their update macros on the
//! game systems' tick ([`crate::player_loop::UPDATES`]). What the host adds:
//! the interface laid out each step one is active, so a pointer's `screen
//! location` reads where the stick has moved it, which `New Turret - Find
//! Target` turns into the ray it picks a target along.

use guac_script::{Value, Vm};

use crate::scripts::ScriptWorld;

/// How near the pointer's ray a target is picked, metres: about a padlock's
/// size, so a pointer on one takes it (ours: stand-in ledger `turret-pick`).
pub const PICK_REACH: f32 = 1.5;

/// What a turret's shots are named from: each element's template.
pub const SHOTS: &str = "New_Turret_Projectile";

/// The turret the scripts run, if one is active (`Active Turret`).
pub fn active(vm: &Vm) -> Option<Value> {
    let g = vm.program().global_named("Active Turret")?;
    vm.global_peek(g)
        .map(|v| vm.value(v))
        .filter(|v| *v != Value::Null)
}

/// One step's turret: with one active, the interface laid out.
pub fn step(vm: &Vm, world: &mut ScriptWorld) {
    let turret = active(vm);
    world.ray_picks = turret.is_some();
    world.turret_base = turret
        .and_then(|t| vm.field(t, "turret actor"))
        .and_then(|a| match vm.value(a) {
            Value::Entity(h) => Some(h),
            _ => None,
        });
    if world.ray_picks {
        world.lay_out_interface();
    }
}
