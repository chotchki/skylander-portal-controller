//! A collected item's box (openspec AS.441): what a pickup holds the level
//! on until a player presses on.
//!
//! - **What the scripts do.** Level 214's ship parts (`Collectible_Items.ai`,
//!   its three placements told apart by id) take the hero's control when he
//!   reaches one: a cutscene state of their own, the camera round the part
//!   while it rises (3.68 s), confetti half a second before the rise ends,
//!   then `Collectible - Create Collected Dialog Box`
//!   (`CollectibeDialogBox.ai`): a box on the gameplay container with its
//!   title, the item's name and the Continue prompt, kept in the global
//!   `CurrentCollectibleDialog`. One of the system's update macros waits on
//!   it with no timer: any joined player's accept button (`action 1`,
//!   South) freshly pressed while the gameplay container shows takes the
//!   box off (one held from before doesn't); for a part (`collected type` 1) that removes the part, whose
//!   SHUTDOWN fades the camera back, ends its cutscene state, completes its
//!   objective and wakes what comes next (the sail, the flight, the outro).
//! - **What the TV lacks.** The interface draws none of the box's sprites
//!   (the blur box, its frame, its text boxes, the button prompt), so the
//!   level stood held with nothing on the screen saying a press was wanted
//!   (chotchki's playtest, 2026-11-15).
//! - **What's ours** (stand-in ledger: `collected-box`): the host notes the
//!   macro's own title, description and kind as the call binds them
//!   ([`note`], its body still run) and the prompt its body puts up
//!   ([`note_prompt`]); the TV draws them while the box stands ([`up`]).
//!   The pads go on to the scripts as before: the press that ends the hold
//!   is the scripts' own.

use guac_script::{Value, Vm};

use crate::scripts::ScriptWorld;

/// The macro that makes the box.
pub const CREATE: &str = "Collectible - Create Collected Dialog Box";
/// The macro the box's body puts its prompt up with (every menu's).
pub const PROMPT: &str = "MainUI - Button Prompt Create";
/// The global the box stands in while it waits.
pub const DIALOG: &str = "CurrentCollectibleDialog";

/// A string the scripts handed the box: its object's name and its text as
/// the interface reads it (none without the interface's model).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Words {
    pub name: Option<String>,
    pub text: Option<String>,
}

/// A box the scripts made: what it shows and what it was made for.
#[derive(Debug, Clone, PartialEq)]
pub struct Collected {
    /// The placement it was made for (`collectible object`).
    pub collectible: Value,
    /// `collected type`: 1 a ship part; 0 asks to watch a soul gem's movie
    /// and 2 to wear a hat once pressed on, which no level on the disc's
    /// own calls make.
    pub kind: i32,
    pub title: Words,
    pub description: Words,
    /// The prompt its body put up, as [`up`] reads it; none until it has.
    pub prompt: Option<Words>,
    /// The strings its body's prompt was made with, as that call bound
    /// them.
    pub prompt_strings: Option<Value>,
}

fn arg(args: &[(&str, Value)], name: &str) -> Value {
    args.iter()
        .find(|(k, _)| *k == name)
        .map_or(Value::Null, |(_, v)| *v)
}

fn words(world: &ScriptWorld, v: Value) -> Words {
    Words {
        name: world.name_of_value(v),
        text: world.text_of(v),
    }
}

/// A call of [`CREATE`] noted as it binds its parameters; its body runs on.
pub fn note(world: &mut ScriptWorld, name: &str, args: &[(&str, Value)]) {
    if !name.eq_ignore_ascii_case(CREATE) {
        return;
    }
    let kind = arg(args, "collected type").as_i32().unwrap_or(0);
    world.collected = Some(Collected {
        collectible: arg(args, "collectible object"),
        kind,
        title: words(world, arg(args, "collected title")),
        description: words(world, arg(args, "collected description")),
        prompt: None,
        prompt_strings: None,
    });
}

/// A call of [`PROMPT`] noted as the box's when one's made and holds none
/// yet: the box's body makes its prompt last, so the first such call after
/// [`note`] is the box's.
pub fn note_prompt(world: &mut ScriptWorld, name: &str, args: &[(&str, Value)]) {
    if !name.eq_ignore_ascii_case(PROMPT) {
        return;
    }
    let strings = arg(args, "strings");
    if let Some(c) = world
        .collected
        .as_mut()
        .filter(|c| c.prompt_strings.is_none())
    {
        c.prompt_strings = Some(strings);
    }
}

fn global(vm: &Vm, name: &str) -> Option<Value> {
    let g = vm.program().global_named(name)?;
    vm.global_peek(g).map(|v| vm.value(v))
}

/// The box the run waits on: the one noted, while [`DIALOG`] holds a box
/// (only [`CREATE`] sets it, so the box is the one noted), its prompt the first of the strings its
/// body put up (a set the VM keeps while the box stands). Gone the step a
/// press takes it off.
pub fn up(vm: &Vm, world: &ScriptWorld) -> Option<Collected> {
    let c = world.collected.as_ref()?;
    global(vm, DIALOG).filter(|d| *d != Value::Null)?;
    let mut c = c.clone();
    let first = c
        .prompt_strings
        .and_then(|v| vm.set(v).and_then(|s| s.first().copied()));
    c.prompt = first.map(|v| words(world, v));
    Some(c)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(kind: i32) -> Vec<(&'static str, Value)> {
        vec![
            ("collectible object", Value::Entity(7)),
            ("collected item icon", Value::Null),
            ("collected title", Value::Null),
            ("collected description", Value::Null),
            ("collected type", Value::Int(kind)),
            ("vo to play", Value::Null),
        ]
    }

    /// openspec AS.441: the box's call noted with what it binds, its
    /// prompt's strings the first prompt's made after it, a later one left
    /// alone, and any other macro noted not at all.
    #[test]
    fn a_collected_box_is_noted_with_its_kind_and_its_first_prompt() {
        let mut world = ScriptWorld::new(
            &crate::scripts::Files::default(),
            &std::collections::HashMap::new(),
            [1, 2, 3, 4, 5, 6, 7],
        );
        let strings = |world: &ScriptWorld| world.collected.as_ref().and_then(|c| c.prompt_strings);
        note(&mut world, "Some Other Macro", &args(1));
        note_prompt(&mut world, PROMPT, &[("strings", Value::Set(3))]);
        assert_eq!(world.collected, None);
        note(&mut world, &CREATE.to_ascii_lowercase(), &args(1));
        let c = world.collected.clone().expect("noted");
        assert_eq!(
            (c.collectible, c.kind, c.prompt_strings),
            (Value::Entity(7), 1, None)
        );
        note_prompt(
            &mut world,
            "Some Other Macro",
            &[("strings", Value::Set(3))],
        );
        assert_eq!(strings(&world), None);
        note_prompt(
            &mut world,
            &PROMPT.to_ascii_uppercase(),
            &[("strings", Value::Set(3))],
        );
        assert_eq!(strings(&world), Some(Value::Set(3)));
        note_prompt(&mut world, PROMPT, &[("strings", Value::Set(4))]);
        assert_eq!(strings(&world), Some(Value::Set(3)), "the box's own kept");
        note(&mut world, CREATE, &args(2));
        assert_eq!(
            world.collected.as_ref().map(|c| (c.kind, c.prompt_strings)),
            Some((2, None)),
            "a new box notes its own"
        );
    }
}
