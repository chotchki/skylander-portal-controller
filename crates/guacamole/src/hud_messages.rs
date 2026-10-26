//! The HUD's replace messages as the TV steps them (openspec AH.222;
//! stand-in ledger: `hud-messages`): the banners over a player's HUD (a
//! level-up's, a rank-up's, a toy accolade's), each queued by the
//! original's own `MainUI - Player HUD - Replace Message - Add Message` and
//! taken off by its own `Clear Message`, stepped here on the original's
//! timing in place of its `Update Messages`, which draws them with sprites
//! and a keyframer nothing here draws. What a banner does past the drawing
//! is its definition's: a level-up's end sets his health to the new level's
//! full health and clears the flag that kept him from harm since the award.
//!
//! Each portal manager tick, for each player (the tick's own loop, ahead of
//! the level-init gate, joined or not):
//!
//! - his queue's clock (`hud replace message timer`) runs on by the time
//!   since the tick before;
//! - with none showing, the queue's first is the one showing from the next
//!   tick (`cur hud replace message`), unless a level change is under way;
//! - showing at state 0: state 1, its timer the clock + 0.4 s, its
//!   definition's init macro run (a level-up's turns the gold's pot
//!   theatrics off);
//! - at state 1: once the slam its icon plays (`UI Animations`' `slam no
//!   fade in`) is three frames in (the original waits on its keyframer's
//!   fourth frame; 3 x 0.166 s on the disc), state 2, its timer the clock +
//!   1.8 s;
//! - at state 2: once the clock reaches its timer, the original's `Clear
//!   Message` (a 0.25 s fade) takes it off and runs its definition's
//!   shutdown macro.
//!
//! So a level-up's ends 2 ticks + 0.5 s + 1.8 s after the award: 141 steps
//! at 60 a second (2.35 s), where AH.11 read 142 records (2.37-2.44 s) on the
//! oracle. Not
//! stepped: the containers' hide states (the TV's HUD is AH.26's and always
//! shows), the banner's and icon's sprites, the keyframer and the slides. A
//! call that stops in play mode is logged, the portal manager runs on and
//! the banner is parked at a state the original never sets (3), holding the
//! queue behind it; strict mode halts.

use guac_script::{Halt, Host, Mode, PlacementId, Value, Vm};
use tracing::warn;

/// The keyframer definition a banner's icon slams in with.
const SLAM: &str = "slam no fade in";
/// The keyframer's frame the banner waits for, counted from 0.
const SLAM_FRAME: usize = 3;
/// Seconds from showing to the slam's wait, as the original sets its timer
/// (read by nothing while it waits on the slam).
const SHOWN: f32 = 0.4;
/// Seconds a banner holds once its icon has slammed in.
const HELD: f32 = 1.8;
/// The fade `Clear Message` is asked for.
const FADE: f32 = 0.25;
/// A state the original never sets: a banner whose call stopped in play.
pub const PARKED: i32 = 3;

/// One tick of `player`'s replace messages (the module docs), for the
/// portal manager `pm`, `since` seconds of its clock after the tick before.
pub fn step(
    vm: &mut Vm,
    host: &mut dyn Host,
    pm: PlacementId,
    player: Value,
    since: f32,
) -> Result<(), Halt> {
    let clock = number(vm, player, "hud replace message timer") + since;
    vm.set_field(player, "hud replace message timer", Value::Float(clock));
    let queued = vm
        .field(player, "hud replace messages")
        .and_then(|s| vm.set(s).map(<[Value]>::to_vec))
        .unwrap_or_default();
    if queued.is_empty() {
        return Ok(());
    }
    let showing = vm
        .field(player, "cur hud replace message")
        .map(|v| vm.value(v))
        .unwrap_or(Value::Null);
    if showing == Value::Null {
        if !changing_level(vm) {
            vm.set_reference(player, "cur hud replace message", vm.value(queued[0]));
        }
        return Ok(());
    }
    let message = showing;
    match number(vm, message, "state") as i32 {
        0 => {
            vm.set_field(message, "state", Value::Int(1));
            vm.set_field(message, "timer", Value::Float(clock + SHOWN));
            if let Some(name) = macro_of(vm, message, "init macro") {
                let args = [
                    ("cur player joined data", player),
                    ("message info", message),
                ];
                let ran = vm.call_for(pm, &name, &args, &[], host).map(|_| ());
                park_on_stop(vm, message, &name, ran)?;
            }
        }
        1 => {
            let shown = number(vm, message, "timer") - SHOWN;
            if clock >= shown + slam(vm) {
                vm.set_field(message, "state", Value::Int(2));
                vm.set_field(message, "timer", Value::Float(clock + HELD));
            }
        }
        2 if clock >= number(vm, message, "timer") => {
            let name = "MainUI - Player HUD - Replace Message - Clear Message";
            let args = [
                ("clear message", message),
                ("clear for player joined data", player),
                ("fade time", Value::Float(FADE)),
            ];
            let ran = vm.call_for(pm, name, &args, &[], host).map(|_| ());
            park_on_stop(vm, message, name, ran)?;
        }
        _ => {}
    }
    Ok(())
}

/// A stop in a banner's call: strict mode halts; play mode logs it and
/// parks the banner.
fn park_on_stop(
    vm: &mut Vm,
    message: Value,
    name: &str,
    ran: Result<(), Halt>,
) -> Result<(), Halt> {
    match ran {
        Err(h) if vm.config().mode == Mode::Play => {
            warn!(call = name, stop = %h, "a HUD banner's call stopped; the banner is parked, the portal manager runs on");
            vm.set_field(message, "state", Value::Int(PARKED));
            Ok(())
        }
        other => other,
    }
}

/// A number field, 0 when it reads none.
fn number(vm: &Vm, v: Value, name: &str) -> f32 {
    vm.field(v, name)
        .and_then(|f| vm.value(f).as_f32())
        .unwrap_or(0.0)
}

/// The macro a banner's definition names for `field`, by its name.
fn macro_of(vm: &Vm, message: Value, field: &str) -> Option<String> {
    let def = vm.field(message, "def").map(|v| vm.value(v))?;
    let Value::Object(o) = vm.field(def, field).map(|v| vm.value(v))? else {
        return None;
    };
    let p = vm.program();
    let d = p.def_of(o)?;
    let d = p.resolve(d).unwrap_or(d);
    Some(p.defs.get(d as usize)?.display.clone())
}

/// Whether `Change Level` names a level being changed to: the queue holds.
fn changing_level(vm: &mut Vm) -> bool {
    vm.program()
        .global_named("Change Level")
        .and_then(|g| vm.global(g))
        .is_some_and(|v| vm.value(v) != Value::Null)
}

/// Seconds the slam's first three frames take (`UI Animations`' `slam no
/// fade in`); 0 where the program holds none.
fn slam(vm: &mut Vm) -> f32 {
    let Some(anims) = vm
        .program()
        .global_named("UI Animations")
        .and_then(|g| vm.global(g))
    else {
        return 0.0;
    };
    let frames = vm
        .field(anims, SLAM)
        .and_then(|s| vm.field(s, "frames"))
        .and_then(|f| vm.set(f).map(<[Value]>::to_vec))
        .unwrap_or_default();
    frames
        .iter()
        .take(SLAM_FRAME)
        .map(|f| number(vm, vm.value(*f), "time"))
        .sum()
}
