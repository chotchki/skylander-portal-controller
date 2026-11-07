//! What's off the way (openspec AU.39): the treasure chests and the hat
//! boxes a kid walks to, on every level, run on their own scripts.
//!
//! - **A chest** (`Chest.ai`) rises and turns to a hero who comes within
//!   18 ft (30 for a Giant) and 5 ft of its height, then hangs there for as
//!   long as he stays within 28 ft (40). Mashing his fourth action (△, the
//!   gesture manager's alternate gesture: a press within the last 0.25 s)
//!   shakes it, the two locks popping at 0.4 and 0.75 and the master lock at
//!   1 after 3 s of it (a second for a Giant). It flings out treasure
//!   pieces, each worth what its own template's id says, in batches until
//!   its count, a piece's worth and one more a piece, passes the level's
//!   chest value (`Loot - Set Treasure Chest Value`: 63 on chapter 1, so
//!   62-72 gold in 10-12 pieces), for him to walk up, one past a rim brought
//!   back by its own `already corrected trajectory`,
//!   sets its slot of the level's saved flags (`chest 1` to `chest
//!   4`, by its id) and takes itself off; a chest whose flag reads set takes
//!   itself off at its start. It's drawn as the hub's people are
//!   ([`is_treasure`]), a skinned model of the level's, unwalked.
//! - **A hat box** (`Hat_Pickup.ai`) shakes the same way, opens over 3 s of
//!   mashing and hands over its hat: `Save - Collection Get/Set - Hat
//!   Collectable` marks it collected in the story save, so the box takes
//!   itself off at every start after, and the level's hat objective counts
//!   it. The original then shows the hat in a report (its bar, its buffs, a
//!   continue prompt) and puts it on the hero if the player says so. Hats
//!   are AK's: the TV shows the hat's name as a hint for the report's own
//!   stages, passes its continue and answers the question no ([`answer_hats`];
//!   stand-in ledger: `hats`), so a taken hat is kept in the collection and
//!   worn by nobody, its buffs off, and no hat model draws in the box.

use std::collections::HashMap;

use guac_script::{Obj, PlacementId, Value, Vm};

use crate::script_engine::Actor;
use crate::scripts::{Placed, ScriptWorld};

/// A chest and what it spawns (openspec AU.39, design decision 2's
/// whitelist), every tick, by script, on every level: the chest, its three
/// locks and their flash, the treasure pieces (`Chest_Loot`, whose script is
/// empty), the sound ducking under its fanfare and the confetti rockets.
pub const CHEST_SCRIPTS: [&str; 6] = [
    "/Chest.ai",
    "/Chest_Lock.ai",
    "/Lock_Flash.ai",
    "/Treasure_unit.ai",
    "/ducking_actor.ai",
    "/ConfettiRocket_Dummy.ai",
];

/// A hat box and what it spawns (openspec AU.39), every tick, by script, on
/// every level: the box, its lid and bow, and the report that names the hat
/// (`UI_TheatricalReporter.ai`), which the box waits out.
pub const HAT_SCRIPTS: [&str; 3] = ["/Hat_Pickup.ai", "/Hat_Lid.ai", "/UI_TheatricalReporter.ai"];

/// Whether a placement whose script is `script` is treasure the TV draws as
/// it does the hub's people, on its placement's skinned model, unwalked: a
/// chest. A hat box's model is static: a prop's draw has it.
pub fn is_treasure(script: &str) -> bool {
    script.ends_with(CHEST_SCRIPTS[0])
}

/// Whether `actor` is a chest or hat box of the file, whose STARTUP reads
/// the story save and takes it off if it's been opened: it stays asleep
/// until the run's save is in ([`crate::story_save::start_run`]), which the
/// original's memory holds before a level loads and the TV's run only makes
/// after its first step, past any resume.
pub(crate) fn waits_for_the_save(templates: &HashMap<Obj, Placed>, actor: &Actor) -> bool {
    actor
        .obj
        .and_then(|o| templates.get(&o))
        .and_then(|p| p.script.as_deref())
        .is_some_and(|s| s.ends_with(CHEST_SCRIPTS[0]) || s.ends_with(HAT_BOX))
}

/// The hat box's script, by its file name.
const HAT_BOX: &str = "/Hat_Pickup.ai";
/// What a box waits on in its `show player` behaviour, by that behaviour's
/// stage: the report that names the hat, then whether to wear it, whose
/// variable reads -1 waiting, 1 yes and 0 no.
const SHOWING: &str = "show player";
const SHOW_STAGE: &str = "show player stage";
const REPORTS: i32 = 1;
const ASKS_TO_WEAR: i32 = 2;
const WEAR_IT: &str = "equip hat?";
/// The report's stage waiting on the accept button, and the box's variables
/// holding it and the hat's name.
const REPORT_WAITS: i32 = 12;
const REPORT: &str = "tr actor ref";
const HAT_NAME: &str = "hat name";

/// The hat boxes the run holds, by their placements.
fn hat_boxes(vm: &Vm, world: &ScriptWorld) -> Vec<PlacementId> {
    world
        .templates
        .values()
        .filter(|p| p.script.as_deref().is_some_and(|s| s.ends_with(HAT_BOX)))
        .filter_map(|p| world.objects.get(&p.obj))
        .filter_map(|&h| vm.placement_of(h))
        .collect()
}

fn local(vm: &Vm, id: PlacementId, name: &str) -> Option<Value> {
    vm.local(id, name).map(|v| vm.value(v))
}

/// A box in `show player` at `stage`.
fn showing(vm: &Vm, id: PlacementId, stage: i32) -> bool {
    vm.behaviour(id) == Some(SHOWING)
        && local(vm, id, SHOW_STAGE).and_then(|v| v.as_i32()) == Some(stage)
}

/// The `anim info` of the report a box in its report stage shows.
fn report_anim(vm: &Vm, id: PlacementId) -> Option<Value> {
    let Value::Entity(report) = local(vm, id, REPORT)? else {
        return None;
    };
    let def = local(vm, vm.placement_of(report)?, "tr def ref")?;
    vm.field(def, "anim info").map(|v| vm.value(v))
}

/// Each hat box past its hat answered as a player's press answers it (the
/// module docs): a report waiting on the accept button passed, a box
/// waiting on `Hat_EquipHatImmediately` told no. How many were. A taken
/// hat is in the collection by then.
pub fn answer_hats(vm: &mut Vm, world: &ScriptWorld) -> usize {
    let mut answered = 0;
    for id in hat_boxes(vm, world) {
        if showing(vm, id, REPORTS)
            && let Some(anim) = report_anim(vm, id)
            && vm
                .field(anim, "stage")
                .map(|v| vm.value(v))
                .and_then(|v| v.as_i32())
                == Some(REPORT_WAITS)
        {
            vm.set_field(anim, "stage", Value::Int(REPORT_WAITS + 1));
            answered += 1;
        }
        if showing(vm, id, ASKS_TO_WEAR)
            && local(vm, id, WEAR_IT).and_then(|v| v.as_i32()) == Some(-1)
        {
            vm.set_local(id, WEAR_IT, Value::Int(0));
            answered += 1;
        }
    }
    answered
}

/// The name of each hat a box is showing its report for, as the report's
/// bar names it: the strings the TV puts up in its place.
pub fn hats_shown(vm: &Vm, world: &ScriptWorld) -> Vec<Value> {
    hat_boxes(vm, world)
        .into_iter()
        .filter(|id| showing(vm, *id, REPORTS))
        .filter_map(|id| local(vm, id, HAT_NAME))
        .filter(|s| *s != Value::Null)
        .collect()
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use guac_formats::igz::Igz;
    use guac_formats::igz::builder::IgzBuilder;
    use guac_formats::script::builder::{El, Writer};
    use guac_formats::script::{Class, Graph};
    use guac_script::{Config, Generator, Handle, Mode, Program, Role, Unit, Whitelist};

    use super::*;
    use crate::script_engine::NoEngine;
    use crate::scripts::{Files, WorldHost, headless_objects, headless_spawner};

    const FILE: Obj = Obj {
        unit: 0,
        index: 0x7000,
    };
    const HANDLE: Handle = 0x30;
    const HAT: &str = "C:/tfb/Content/Levels/Includes/Hats/scripts/Hat_Pickup.ai";
    const CHEST: &str = "C:/tfb/Content/Levels/Includes/Spyro_GenericStuff/scripts/Chest.ai";

    /// A box in the original's shape, our ops: its `show player` behaviour
    /// and the variables the TV answers, `equip hat?` at -1 as the box sets
    /// it before it asks.
    fn hat_box() -> (Vm, ScriptWorld, PlacementId) {
        let mut b = IgzBuilder::new();
        {
            let mut w = Writer::new(&mut b);
            let w = &mut w;
            let stage = w.variable(SHOW_STAGE, "ValueInfo", Some("IntMeasurement"));
            let wear = w.variable(WEAR_IT, "ValueInfo", Some("IntMeasurement"));
            let showing = w.op(Class::UserBehavior, SHOWING);
            let script = w.script(
                HAT,
                |w| {
                    let none = w.int(-1);
                    w.set(&[El::Local(wear)], none);
                    let zero = w.int(0);
                    w.set(&[El::Local(stage)], zero);
                    w.set_behavior(showing);
                },
                |_| {},
                |w| {
                    let p = w.push(showing);
                    w.end_block(p);
                },
            );
            w.declare(script, &[stage, wear]);
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
            placed: vec![Placed::named(FILE, "HatBox_Pickup(1)", Some(HAT), 4)],
            ..Files::default()
        };
        let handles = HashMap::from([(FILE, HANDLE)]);
        let mut world = ScriptWorld::new(&files, &handles, headless_objects(&handles));
        let mut config = Config::new(Mode::Strict);
        config.whitelist = Whitelist::only(&[], &[HAT_BOX]);
        let sid = program.script_named("Hat_Pickup.ai").unwrap();
        let mut vm = Vm::new(program, config);
        let id = vm.add_at(HANDLE, "HatBox_Pickup(1)", sid, FILE);
        let mut generator = Generator::seeded(1);
        vm.step(&mut WorldHost {
            world: &mut world,
            engine: &mut NoEngine,
            generator: &mut generator,
            spawn: &mut headless_spawner(0x1000),
        })
        .unwrap();
        (vm, world, id)
    }

    fn wear(vm: &Vm, id: PlacementId) -> Option<i32> {
        local(vm, id, WEAR_IT).and_then(|v| v.as_i32())
    }

    /// openspec AU.39: a box waiting on the wear question is told no, once;
    /// one showing anything else is left alone, a yes among them.
    #[test]
    fn a_box_asking_to_wear_its_hat_is_told_no_once() {
        let (mut vm, world, id) = hat_box();
        assert_eq!(vm.behaviour(id), Some(SHOWING));
        assert_eq!(answer_hats(&mut vm, &world), 0, "at its first stage");
        assert_eq!(wear(&vm, id), Some(-1));
        assert!(vm.set_local(id, SHOW_STAGE, Value::Int(ASKS_TO_WEAR)));
        assert_eq!(answer_hats(&mut vm, &world), 1);
        assert_eq!(wear(&vm, id), Some(0));
        assert_eq!(answer_hats(&mut vm, &world), 0, "told once");
        assert!(vm.set_local(id, WEAR_IT, Value::Int(1)));
        assert_eq!(answer_hats(&mut vm, &world), 0);
        assert_eq!(wear(&vm, id), Some(1), "a yes stands");
    }

    /// A chest's and hat box's script wait for the save, by their file; any
    /// other, a script merely ending alike among them, and a clone don't.
    #[test]
    fn the_chests_and_hat_boxes_wait_for_the_save() {
        let placed = |script: &str| Placed::named(FILE, "it", Some(script), 4);
        let actor = || {
            let mut a = Actor::blank();
            a.obj = Some(FILE);
            a
        };
        for (script, waits) in [
            (CHEST, true),
            (HAT, true),
            ("C:/l/Not_A_Chest.ai", false),
            ("C:/l/Chest_Lock.ai", false),
            ("C:/l/Scale.ai", false),
        ] {
            let templates = HashMap::from([(FILE, placed(script))]);
            assert_eq!(waits_for_the_save(&templates, &actor()), waits, "{script}");
        }
        let templates = HashMap::from([(FILE, placed(CHEST))]);
        assert!(!waits_for_the_save(&templates, &Actor::blank()), "a clone");
    }

    /// A chest draws as the hub's people do; a hat box is a static prop and
    /// its locks are clones.
    #[test]
    fn a_chest_alone_is_treasure_the_tv_draws_unwalked() {
        assert!(is_treasure(CHEST));
        assert!(!is_treasure(HAT));
        assert!(!is_treasure("C:/l/Chest_Lock.ai"));
        assert!(!is_treasure("C:/l/Not_A_Chest.ai"));
    }

    /// Every level's listing runs them, past the TV's own listing's other
    /// scripts: what's asked here stays on the run however it's built.
    #[test]
    fn the_listing_runs_them_on_every_level() {
        let c = crate::scripts::smash_config(Mode::Play);
        for s in CHEST_SCRIPTS.iter().chain(&HAT_SCRIPTS) {
            let path = format!("C:/tfb/Content/Levels/Includes/x{s}");
            assert!(c.whitelist.lists("a placement", &path), "{s}");
        }
    }
}
