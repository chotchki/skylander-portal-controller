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
//!   holds are the HUD's (stand-in ledger: `hud`). A scene that hides the
//!   interface hides them with it, as `UI - Adjust UI Visibility` leaves
//!   the announce container's `hide state` over 0, and their stay waits.
//! - **A gesture's prompt** (openspec AU.39: `UI_Controller_Prompt.ai`, what
//!   tells a player to mash at a chest): each manager on `GestureData` holds
//!   its words in `wiimote prompt text ref` and shows them while its `show
//!   sprite` holds and its box is made. On the TV it's a hint, on the same
//!   box (stand-in ledger: `level-words`).

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

/// The scripts hide the announcements: `UI Containers`' `announce container`
/// holds a `hide state` over 0. False until the UI's scripts have made it.
fn hidden(vm: &Vm) -> bool {
    global(vm, "UI Containers")
        .and_then(|c| field(vm, c, "announce container"))
        .and_then(|c| field(vm, c, "hide state"))
        .and_then(|s| s.as_f32())
        .is_some_and(|s| s > 0.0)
}

/// The hints on the notifications' queue that show now, oldest first.
pub fn hints(vm: &Vm, world: &ScriptWorld) -> Vec<Line> {
    if hidden(vm) {
        return Vec::new();
    }
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

/// The hats a box is reporting (openspec AU.39), each by its name, in place
/// of the report's bar ([`crate::treasure::hats_shown`]).
pub fn hat_reports(vm: &Vm, world: &ScriptWorld) -> Vec<Line> {
    crate::treasure::hats_shown(vm, world)
        .into_iter()
        .map(|string| Line {
            string,
            text: world.text_of(string),
        })
        .collect()
}

/// The gesture prompts that show now, by each manager on `GestureData`,
/// their box made (`main loop`) and `show sprite` set; hidden with the
/// hints.
pub fn gesture_prompts(vm: &Vm, world: &ScriptWorld) -> Vec<Line> {
    if hidden(vm) {
        return Vec::new();
    }
    let Some(data) = global(vm, "GestureData") else {
        return Vec::new();
    };
    prompts_of(vm, world, data)
}

fn prompts_of(vm: &Vm, world: &ScriptWorld, managers: Value) -> Vec<Line> {
    elements(vm, managers)
        .into_iter()
        .filter_map(|g| field(vm, g, "attached actor"))
        .filter_map(|a| match a {
            Value::Entity(h) => vm.placement_of(h),
            _ => None,
        })
        .filter(|id| vm.behaviour(*id) == Some("main loop"))
        .filter(|id| {
            vm.local(*id, "show sprite")
                .and_then(|v| vm.value(v).as_i32())
                .is_some_and(|s| s > 0)
        })
        .filter_map(|id| vm.local(id, "wiimote prompt text ref"))
        .map(|v| vm.value(v))
        .filter(|s| *s != Value::Null)
        .map(|string| Line {
            string,
            text: world.text_of(string),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::sync::Arc;

    use guac_formats::igz::Igz;
    use guac_formats::igz::builder::IgzBuilder;
    use guac_formats::script::builder::{El, Writer};
    use guac_formats::script::{Class, Graph, StackClass};
    use guac_script::{Config, Generator, Mode, Obj, Program, Role, Unit, Whitelist};

    use super::*;
    use crate::script_engine::NoEngine;
    use crate::scripts::{Files, Placed, WorldHost, headless_objects, headless_spawner};

    const FILE: Obj = Obj { unit: 0, index: 2 };
    const HANDLE: u64 = 0x30;
    const PATH: &str = "C:/tfb/Content/Levels/Includes/Global/scripts/GameSystems.ai";

    /// A file in the original's shape, our ops: the current scene's `extra
    /// data` holding a card's structure (its sprite made when `sprite`), and
    /// `all notifications` holding a hint, then a counter (`is a hint?` 0),
    /// and `UI Containers`' announce container at `hide state`.
    fn up(sprite: bool, hide_state: i32) -> Arc<Program> {
        let mut b = IgzBuilder::new();
        {
            let mut w = Writer::new(&mut b);
            let w = &mut w;
            let def_ty = w.op(Class::DefineStructure, "CS Seq - Enemy Intro UI Def");
            let string = w.variable("string", "ScriptReference", None);
            let data_ty = w.op(Class::DefineStructure, "CS Seq - Enemy Intro UI Data");
            let container = w.variable("generated container", "ScriptReference", None);
            let def = w.variable("def", "ScriptStructure", None);
            w.refer(def, 0x24, def_ty);
            let scene_ty = w.op(Class::DefineStructure, "CS - Scene Data");
            let extra = w.variable("extra data", "ScriptSet", None);
            w.refer(extra, 0x24, data_ty);
            let held = w.variable("the card", "ScriptStructure", None);
            w.refer(held, 0x24, data_ty);
            let cs_ty = w.op(Class::DefineStructure, "CS - Data");
            let cur = w.variable("cur scene data", "ScriptStructure", None);
            w.refer(cur, 0x24, scene_ty);
            let control_ty = w.op(Class::DefineStructure, "CS - Control Data");
            let current = w.variable("current cs", "ScriptStructure", None);
            w.refer(current, 0x24, cs_ty);
            let control = w.b.object(0, "ScriptStructure", 0x20);
            w.text(control, 0x08, "cs control data");
            w.refer(control, 0x1C, control_ty);
            let note_ty = w.op(Class::DefineStructure, "Notification - Notification Data");
            let is_hint = w.variable("is a hint?", "ValueInfo", Some("IntMeasurement"));
            let text = w.variable("associated text", "ScriptReference", None);
            let queue_ty = w.op(Class::DefineStructure, "Notification Queue");
            let notes = w.variable("all notifications", "ScriptSet", None);
            w.refer(notes, 0x24, note_ty);
            let hint = w.variable("the hint", "ScriptStructure", None);
            w.refer(hint, 0x24, note_ty);
            let counter = w.variable("the counter", "ScriptStructure", None);
            w.refer(counter, 0x24, note_ty);
            let queue = w.b.object(0, "ScriptStructure", 0x20);
            w.text(queue, 0x08, "all notifications");
            w.refer(queue, 0x1C, queue_ty);
            let box_ty = w.op(Class::DefineStructure, "UI Container");
            let hide = w.variable("hide state", "ValueInfo", Some("IntMeasurement"));
            let containers_ty = w.op(Class::DefineStructure, "UI Containers");
            let announce = w.variable("announce container", "ScriptStructure", None);
            w.refer(announce, 0x24, box_ty);
            let containers = w.b.object(0, "ScriptStructure", 0x20);
            w.text(containers, 0x08, "UI Containers");
            w.refer(containers, 0x1C, containers_ty);
            let name = w.op(Class::DefineMacro, "the name");
            let line = w.op(Class::DefineMacro, "the line");
            let count = w.op(Class::DefineMacro, "the count");
            let at = [El::Local(control), El::Local(current), El::Local(cur)];
            let (pre, _) = w.open(Class::PreScript, "PRESCRIPT");
            let (start, _) = w.open(Class::StartUp, "STARTUP");
            let refer = |w: &mut Writer<'_>, lhs: &[El<'_>], to| {
                let l = w.value(lhs);
                let r = w.reference(&[El::Local(to)], None);
                let o = w.op(Class::SetReference, "setref||=");
                w.refer(o, 0x1C, l);
                w.refer(o, 0x20, r);
                w.push(o);
            };
            let join = |w: &mut Writer<'_>, set: &[El<'_>], element: &[El<'_>]| {
                let set = w.stack(StackClass::Set, set);
                let element = w.group(element, None);
                let o = w.op(Class::ChangeMembership, "set [set]||to");
                w.refer(o, 0x1C, set);
                w.word(o, 0x20, 0);
                w.refer(o, 0x24, element);
                w.push(o);
            };
            let card = [at[0], at[1], at[2], El::Local(held)];
            refer(
                w,
                &[
                    card[0],
                    card[1],
                    card[2],
                    card[3],
                    El::Local(def),
                    El::Local(string),
                ],
                name,
            );
            if sprite {
                refer(
                    w,
                    &[card[0], card[1], card[2], card[3], El::Local(container)],
                    name,
                );
            }
            join(w, &[at[0], at[1], at[2], El::Local(extra)], &card);
            let one = w.int(1);
            w.set(
                &[El::Local(queue), El::Local(hint), El::Local(is_hint)],
                one,
            );
            let hide_at = w.int(hide_state);
            w.set(
                &[El::Local(containers), El::Local(announce), El::Local(hide)],
                hide_at,
            );
            refer(
                w,
                &[El::Local(queue), El::Local(hint), El::Local(text)],
                line,
            );
            refer(
                w,
                &[El::Local(queue), El::Local(counter), El::Local(text)],
                count,
            );
            join(
                w,
                &[El::Local(queue), El::Local(notes)],
                &[El::Local(queue), El::Local(hint)],
            );
            join(
                w,
                &[El::Local(queue), El::Local(notes)],
                &[El::Local(queue), El::Local(counter)],
            );
            w.end_block(start);
            let (shut, _) = w.open(Class::ShutDown, "SHUTDOWN");
            w.end_block(shut);
            let (section, _) = w.open(Class::MacroSection, "MACROS");
            for d in [name, line, count] {
                let p = w.open_macro(d, &[]);
                w.end_block(p);
            }
            w.end_block(section);
            let (types, _) = w.open(Class::StructureSection, "STRUCTURES");
            for (ty, vars) in [
                (def_ty, vec![string]),
                (data_ty, vec![container, def]),
                (scene_ty, vec![extra, held]),
                (cs_ty, vec![cur]),
                (control_ty, vec![current]),
                (note_ty, vec![is_hint, text]),
                (queue_ty, vec![notes, hint, counter]),
                (box_ty, vec![hide]),
                (containers_ty, vec![announce]),
            ] {
                let p = w.push(ty);
                for v in vars {
                    w.push(v);
                }
                w.end_block(p);
            }
            w.end_block(types);
            w.end_block(pre);
            w.end(PATH);
        }
        let bytes = b.build();
        let igz = Igz::parse(&bytes).unwrap();
        let graph = Graph::read(&igz).unwrap();
        let unit = Unit {
            name: "global",
            role: Role::Other,
            igz: &igz,
            graph: &graph,
        };
        Arc::new(Program::link(&[unit]).unwrap())
    }

    fn read(sprite: bool, hide_state: i32) -> (Option<String>, Vec<String>) {
        let program = up(sprite, hide_state);
        let files = Files {
            placed: vec![Placed::named(FILE, "Macros_GameSystems", Some(PATH), 4)],
            ..Files::default()
        };
        let handles = HashMap::from([(FILE, HANDLE)]);
        let mut world = ScriptWorld::new(&files, &handles, headless_objects(&handles));
        let mut config = Config::new(Mode::Strict);
        config.whitelist = Whitelist::only(&[], &["GameSystems.ai"]);
        let sid = program.script_named("GameSystems.ai").unwrap();
        let mut vm = Vm::new(program, config);
        vm.add_at(HANDLE, "Macros_GameSystems", sid, FILE);
        let mut generator = Generator::seeded(1);
        let mut engine = NoEngine;
        let mut spawn = headless_spawner(0x1000);
        let mut host = WorldHost {
            world: &mut world,
            engine: &mut engine,
            generator: &mut generator,
            spawn: &mut spawn,
        };
        vm.step(&mut host).unwrap();
        let name = |vm: &Vm, l: Line| match l.string {
            Value::Object(o) => vm
                .program()
                .def_of(o)
                .map(|d| vm.program().defs[d as usize].display.clone())
                .unwrap_or_default(),
            other => format!("{other:?}"),
        };
        let card = enemy_card(&vm, host.world).map(|l| name(&vm, l));
        let hints = hints(&vm, host.world)
            .into_iter()
            .map(|l| name(&vm, l))
            .collect();
        (card, hints)
    }

    /// openspec AS.131: the card reads its string while its sprite stands,
    /// nothing past it; the queue's hints read, its counters don't.
    #[test]
    fn the_card_reads_while_its_sprite_stands_and_the_queue_gives_its_hints() {
        assert_eq!(
            read(true, 0),
            (Some("the name".to_owned()), vec!["the line".to_owned()])
        );
        assert_eq!(read(false, 0).0, None, "no sprite, no card");
    }

    /// openspec AS.131: a scene hiding the announce container hides the
    /// hints; the card's its own scene's and stays.
    #[test]
    fn a_hidden_announce_container_hides_the_hints_and_not_the_card() {
        assert_eq!(
            read(true, 1),
            (Some("the name".to_owned()), Vec::new()),
            "hidden"
        );
    }

    /// openspec AU.39: a gesture manager's words show while it's in its main
    /// loop with `show sprite` set, under no hide; a manager still making
    /// its box, hidden or hiding, none.
    #[test]
    fn a_gesture_prompt_shows_while_its_manager_shows_it() {
        let (vm, world, _, managers) = gesture_manager("initialize");
        assert_eq!(
            prompts_of(&vm, &world, managers).len(),
            0,
            "its box isn't made"
        );
        let (mut vm, world, id, managers) = gesture_manager("main loop");
        assert_eq!(prompts_of(&vm, &world, managers).len(), 1);
        assert!(vm.set_local(id, "show sprite", Value::Int(0)));
        assert_eq!(prompts_of(&vm, &world, managers).len(), 0, "hidden");
    }

    /// A manager in the original's shape, our ops, at `behaviour`, attached
    /// to its placement on a set of managers.
    fn gesture_manager(behaviour: &str) -> (Vm, ScriptWorld, guac_script::PlacementId, Value) {
        const PROMPT: &str =
            "C:/tfb/Content/Levels/Includes/UI_Controller_Prompt/Scripts/UI_Controller_Prompt.ai";
        const PROMPT_FILE: Obj = Obj {
            unit: 0,
            index: 0x7000,
        };
        const MANAGER: u64 = 0x31;
        let mut b = IgzBuilder::new();
        {
            let mut w = Writer::new(&mut b);
            let w = &mut w;
            let data_ty = w.op(Class::DefineStructure, "Gesture Data");
            let attached = w.variable("attached actor", "ScriptReference", None);
            let holder_ty = w.op(Class::DefineStructure, "Gesture Holder");
            let managers = w.variable("the managers", "ScriptSet", None);
            w.refer(managers, 0x24, data_ty);
            let one = w.variable("the data", "ScriptStructure", None);
            w.refer(one, 0x24, data_ty);
            let holder = w.b.object(0, "ScriptStructure", 0x20);
            w.text(holder, 0x08, "the holder");
            w.refer(holder, 0x1C, holder_ty);
            let show = w.variable("show sprite", "ValueInfo", Some("IntMeasurement"));
            let words = w.variable("wiimote prompt text ref", "ScriptReference", None);
            let line = w.op(Class::DefineMacro, "the words");
            let main = w.op(Class::UserBehavior, "main loop");
            let waits = w.op(Class::UserBehavior, "initialize");
            let (pre, _) = w.open(Class::PreScript, "PRESCRIPT");
            let (start, _) = w.open(Class::StartUp, "STARTUP");
            let l = w.value(&[El::Local(words)]);
            let r = w.reference(&[El::Local(line)], None);
            let o = w.op(Class::SetReference, "setref||=");
            w.refer(o, 0x1C, l);
            w.refer(o, 0x20, r);
            w.push(o);
            let on = w.int(1);
            w.set(&[El::Local(show)], on);
            let set = w.stack(StackClass::Set, &[El::Local(holder), El::Local(managers)]);
            let element = w.group(&[El::Local(holder), El::Local(one)], None);
            let o = w.op(Class::ChangeMembership, "set [set]||to");
            w.refer(o, 0x1C, set);
            w.word(o, 0x20, 0);
            w.refer(o, 0x24, element);
            w.push(o);
            w.set_behavior(if behaviour == "main loop" {
                main
            } else {
                waits
            });
            w.end_block(start);
            let (shut, _) = w.open(Class::ShutDown, "SHUTDOWN");
            w.end_block(shut);
            let (section, _) = w.open(Class::MacroSection, "MACROS");
            let p = w.open_macro(line, &[]);
            w.end_block(p);
            w.end_block(section);
            let (types, _) = w.open(Class::StructureSection, "STRUCTURES");
            for (ty, vars) in [(data_ty, vec![attached]), (holder_ty, vec![managers, one])] {
                let p = w.push(ty);
                for v in vars {
                    w.push(v);
                }
                w.end_block(p);
            }
            w.end_block(types);
            w.end_block(pre);
            for k in [main, waits] {
                let p = w.push(k);
                w.end_block(p);
            }
            w.end(PROMPT);
        }
        let bytes = b.build();
        let igz = Igz::parse(&bytes).unwrap();
        let graph = Graph::read(&igz).unwrap();
        let unit = Unit {
            name: "global",
            role: Role::Other,
            igz: &igz,
            graph: &graph,
        };
        let program = Arc::new(Program::link(&[unit]).unwrap());
        let files = Files {
            placed: vec![Placed::named(
                PROMPT_FILE,
                "UI_Controller_Prompt",
                Some(PROMPT),
                4,
            )],
            ..Files::default()
        };
        let handles = HashMap::from([(PROMPT_FILE, MANAGER)]);
        let mut world = ScriptWorld::new(&files, &handles, headless_objects(&handles));
        let mut config = Config::new(Mode::Strict);
        config.whitelist = Whitelist::only(&[], &["UI_Controller_Prompt.ai"]);
        let sid = program.script_named("UI_Controller_Prompt.ai").unwrap();
        let mut vm = Vm::new(program, config);
        let id = vm.add_at(MANAGER, "UI_Controller_Prompt", sid, PROMPT_FILE);
        let mut generator = Generator::seeded(1);
        vm.step(&mut WorldHost {
            world: &mut world,
            engine: &mut NoEngine,
            generator: &mut generator,
            spawn: &mut headless_spawner(0x1000),
        })
        .unwrap();
        let holder = global(&vm, "the holder").unwrap();
        let managers = field(&vm, holder, "the managers").unwrap();
        let manager = elements(&vm, managers)[0];
        assert!(vm.set_reference(manager, "attached actor", Value::Entity(MANAGER)));
        assert_eq!(vm.behaviour(id), Some(behaviour));
        (vm, world, id, managers)
    }
}
