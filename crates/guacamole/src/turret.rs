//! A turret's game on the TV's run (openspec AS.76): level 214's two turrets
//! run on the VM from their activators (`Turret_1_Base.ai`, `Turret_2_Base.ai`
//! and `global.bld`'s `commonTurretMacros.ai`), their update macros on the
//! game systems' tick ([`crate::player_loop::UPDATES`]). What the host adds:
//! the interface laid out each step one is active, so a pointer's `screen
//! location` reads where the stick has moved it, which `New Turret - Find
//! Target` turns into the ray it picks a target along.

use guac_script::{Value, Vm};
use serde::Serialize;

use crate::scripts::ScriptWorld;

/// How near the pointer's ray a target is picked, metres: about a padlock's
/// size, so a pointer on one takes it (ours: stand-in ledger `turret-pick`).
pub const PICK_REACH: f32 = 1.5;

/// The string a turret's calibration shows on the PS3.
pub const CALIBRATION_LINE: &str = "PointAtTarget_360/PS3";

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

/// What a TV shows of the active turret: its state, where its first
/// player's pointer is, where it wants calibrating and the line that asks.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct TurretView {
    /// `turret state`'s macro.
    pub state: String,
    /// The pointer's offset from the screen's middle, interface units.
    pub pointer: Option<[f32; 2]>,
    /// Calibrating, the circle's offset; its line's English when it reads.
    pub calibration: Option<[f32; 2]>,
    pub message: Option<String>,
    /// The padlocks hit so far (`DBG_Lockhit`, the turret's objective).
    pub hits: i32,
    /// Where each of its targets standing stands from the screen's middle,
    /// as the pointer's offset would be on it.
    pub targets: Vec<[f32; 2]>,
}

/// A pointer's offset that lands on `(x, y)` on the screen: `New Turret -
/// Find Target` reads its graphic's place 10 pixels left of where the ray
/// goes through, at 1.5 pixels a unit.
pub fn pointer_at(x: f32, y: f32) -> [f32; 2] {
    let (w, h) = (crate::viewport::SCREEN_WIDTH, crate::viewport::SCREEN_HEIGHT);
    [(x - w / 2.0 + 10.0) / 1.5, (y - h / 2.0) / 1.5]
}

/// The first player's data of `turret`.
fn player(vm: &Vm, turret: Value) -> Option<Value> {
    vm.field(turret, "p1 player data").map(|p| vm.value(p))
}

/// [`TurretView`] of the turret running now.
pub fn view(vm: &Vm, world: &ScriptWorld) -> Option<TurretView> {
    let turret = active(vm)?;
    let state = match vm.field(turret, "turret state").map(|s| vm.value(s)) {
        Some(Value::Object(o)) => vm.program().name_of(o).unwrap_or_default().to_owned(),
        _ => String::new(),
    };
    let number = |v: Value, name: &str| vm.field(v, name).and_then(|f| vm.value(f).as_f32());
    let pointer = player(vm, turret)
        .and_then(|p| vm.field(p, "wii pointer data"))
        .map(|p| vm.value(p))
        .filter(|p| *p != Value::Null)
        .and_then(|p| Some([number(p, "xrem")?, number(p, "yrem")?]));
    let calibrating = vm
        .field(turret, "calibration data")
        .map(|c| vm.value(c))
        .filter(|c| *c != Value::Null);
    let calibration = calibrating
        .and_then(|c| vm.field(c, "calibration location"))
        .and_then(|l| match vm.value(l) {
            Value::Screen(s) => {
                let (x, y) = crate::viewport::unpack_screen(s);
                Some([x, y])
            }
            _ => None,
        });
    // The line this platform's screen shows; what the data remembers is the
    // Wii's, whatever it put up.
    let message = calibrating
        .and_then(|_| vm.program().object_named(CALIBRATION_LINE))
        .and_then(|o| world.text_of(Value::Object(o)));
    let hits = vm
        .program()
        .global_named("DBG_Lockhit")
        .and_then(|g| vm.global_peek(g))
        .and_then(|v| vm.value(v).as_i32())
        .unwrap_or(0);
    let targets = vm
        .field(turret, "turret targets")
        .and_then(|t| vm.set(t).map(<[Value]>::to_vec))
        .unwrap_or_default()
        .into_iter()
        .filter_map(|v| match v {
            Value::Entity(h) => Some(h),
            Value::Object(o) => world.handles().get(&o).copied(),
            _ => None,
        })
        .filter(|h| !world.removed(*h))
        .filter_map(|h| world.actor(h).map(|a| a.position))
        .filter_map(|p| world.viewport().project(p))
        .map(|(x, y)| pointer_at(x, y))
        .collect();
    Some(TurretView {
        state,
        pointer,
        calibration,
        message,
        hits,
        targets,
    })
}
