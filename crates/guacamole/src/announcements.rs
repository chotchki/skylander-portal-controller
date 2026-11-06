//! What the scripts put up in words over a level, as the TV reads it
//! (openspec AS.131): the original's sprites for both are built on the VM
//! and drawn nowhere, so the TV's UI draws them from what the scripts hold.
//!
//! - **The enemy intro's card** (`Cutscene Sequence - Enemy Intro UI Init`):
//!   the step puts an enemy's name up for its duration, its structure in the
//!   scene's `extra data` holding the sprite it made and the definition with
//!   the string; the scene's maintain action takes it out of the set once
//!   the scene's time reaches `end at scene time`, so while it's there the
//!   card is up, on the scripts' own clock.
//! - **The hints** (`UI - Announcement` of type 0, the post-cutscene hint
//!   among them): `Notification - Add Notification` queues each on `all
//!   notifications`, and `Notification - Manage Queue`, one of the loop's
//!   update macros since AS.131, takes it off after its stay. Only the
//!   hints: the collection counters and the player pokes the queue also
//!   holds are the HUD's (stand-in ledger: `hud`).

use guac_script::{Value, Vm};

use crate::scripts::ScriptWorld;

/// A line the scripts put up, as a report and the TV's UI read it.
#[derive(Debug, Clone, PartialEq)]
pub struct Line {
    /// The string, an object of the level's or `global.bld`'s.
    pub string: Value,
    /// Its English, once the interface's model reads it.
    pub text: Option<String>,
}

fn global(vm: &Vm, name: &str) -> Option<Value> {
    let g = vm.program().global_named(name)?;
    vm.global_peek(g).map(|v| vm.value(v))
}

fn field(vm: &Vm, v: Value, name: &str) -> Option<Value> {
    vm.field(v, name).map(|f| vm.value(f))
}

fn elements(vm: &Vm, set: Value) -> Vec<Value> {
    vm.set(set)
        .map(|e| e.iter().map(|v| vm.value(*v)).collect())
        .unwrap_or_default()
}

/// The enemy intro's card up now: the current scene's `extra data` holding
/// a structure whose sprite stands and whose definition names the string.
pub fn enemy_card(vm: &Vm, world: &ScriptWorld) -> Option<Line> {
    let scene = global(vm, "cs control data")
        .and_then(|c| field(vm, c, "current cs"))
        .and_then(|c| field(vm, c, "cur scene data"))
        .filter(|s| *s != Value::Null)?;
    elements(vm, field(vm, scene, "extra data")?)
        .into_iter()
        .filter(|e| field(vm, *e, "generated container").is_some_and(|c| c != Value::Null))
        .find_map(|e| field(vm, field(vm, e, "def")?, "string"))
        .filter(|s| *s != Value::Null)
        .map(|string| Line {
            string,
            text: world.text_of(string),
        })
}

/// The hints on the notifications' queue now, oldest first.
pub fn hints(vm: &Vm, world: &ScriptWorld) -> Vec<Line> {
    let Some(all) = global(vm, "all notifications").and_then(|n| field(vm, n, "all notifications"))
    else {
        return Vec::new();
    };
    elements(vm, all)
        .into_iter()
        .filter(|n| field(vm, *n, "is a hint?").and_then(|h| h.as_i32()) > Some(0))
        .filter_map(|n| field(vm, n, "associated text"))
        .filter(|s| *s != Value::Null)
        .map(|string| Line {
            string,
            text: world.text_of(string),
        })
        .collect()
}
