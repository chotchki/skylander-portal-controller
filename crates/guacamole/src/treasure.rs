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
//!   worn by nobody, its buffs off, and no hat model draws in the box. The
//!   hat's stream registers nothing here, so the TV registers an outfit with
//!   no actor in its place ([`answer_hat_load`], openspec AS.158): the box's
//!   clone of it is skipped, where a stale scratch reference once cloned the
//!   hero and the box's `Remove` of its hat took him off.
//! - A legendary treasure (`Loot_Specials.ai`, openspec AS.151) is a prop till
//!   a hero comes within 15 ft, when it takes itself into the collection
//!   (its STARTUP takes it off once it's there, so it waits for the save as
//!   a chest does), spins up in a scene of its own and pays out its
//!   definition's gold past the same report, passed here. Equipping a ship
//!   part is the treasure menu's, a screen the TV doesn't have
//!   (stand-in ledger: `ship-parts`).

use std::collections::HashMap;

use guac_script::{Halt, Obj, PlacementId, Value, Vm};

use crate::script_engine::Actor;
use crate::scripts::{Placed, ScriptWorld, WorldHost};

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

/// A legendary treasure and its definition (openspec AS.151), by script: the
/// `Legendary_Treasure_*` a level places, walked up to, spun and paid out in
/// a scene of its own, and each level's `Def_Ship_Part_<level>.ai`, which
/// defines what it pays. Equipping one is the treasure menu's, a screen the
/// TV doesn't have.
pub const LEGENDARY_SCRIPTS: [&str; 17] = [
    "/Loot_Specials.ai",
    "/Def_Ship_Part_200.ai",
    "/Def_Ship_Part_201.ai",
    "/Def_Ship_Part_202.ai",
    "/Def_Ship_Part_203.ai",
    "/Def_Ship_Part_204.ai",
    "/Def_Ship_Part_205.ai",
    "/Def_Ship_Part_206.ai",
    "/Def_Ship_Part_207.ai",
    "/Def_Ship_Part_208.ai",
    "/Def_Ship_Part_209.ai",
    "/Def_Ship_Part_210.ai",
    "/Def_Ship_Part_211.ai",
    "/Def_Ship_Part_212.ai",
    "/Def_Ship_Part_213.ai",
    "/Def_Ship_Part_214.ai",
    "/Def_Ship_Part_215.ai",
];

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
        .is_some_and(|s| {
            s.ends_with(CHEST_SCRIPTS[0])
                || s.ends_with(HAT_BOX)
                || s.ends_with(LEGENDARY_SCRIPTS[0])
        })
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
/// A legendary treasure's name, its `show player` holding the same report
/// (openspec AS.151).
const TREASURE_NAME: &str = "treasure name";
/// What a hat's stream runs once it's in, leaving the level's outfit record
/// for the box to clone.
const REGISTER_OUTFIT: &str = "Portal - Register Outfit Item";

/// The placements the run holds of a script ending `script`.
fn placed_by(vm: &Vm, world: &ScriptWorld, script: &str) -> Vec<PlacementId> {
    world
        .templates
        .values()
        .filter(|p| p.script.as_deref().is_some_and(|s| s.ends_with(script)))
        .filter_map(|p| world.objects.get(&p.obj))
        .filter_map(|&h| vm.placement_of(h))
        .collect()
}

/// The hat boxes the run holds, by their placements.
fn hat_boxes(vm: &Vm, world: &ScriptWorld) -> Vec<PlacementId> {
    placed_by(vm, world, HAT_BOX)
}

/// The legendary treasures the run holds, by their placements.
fn legendary(vm: &Vm, world: &ScriptWorld) -> Vec<PlacementId> {
    placed_by(vm, world, LEGENDARY_SCRIPTS[0])
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

/// `id`'s report, waiting on the accept button, passed as a press does.
fn pass_report(vm: &mut Vm, id: PlacementId) -> bool {
    if showing(vm, id, REPORTS)
        && let Some(anim) = report_anim(vm, id)
        && vm
            .field(anim, "stage")
            .map(|v| vm.value(v))
            .and_then(|v| v.as_i32())
            == Some(REPORT_WAITS)
    {
        return vm.set_field(anim, "stage", Value::Int(REPORT_WAITS + 1));
    }
    false
}

/// Each hat box past its hat answered as a player's press answers it (the
/// module docs): a report waiting on the accept button passed, a box
/// waiting on `Hat_EquipHatImmediately` told no, and a legendary treasure's
/// report passed the same way (openspec AS.151). How many were. A taken hat
/// or treasure is in the collection by then.
pub fn answer_hats(vm: &mut Vm, world: &ScriptWorld) -> usize {
    let mut answered = 0;
    for id in legendary(vm, world) {
        answered += usize::from(pass_report(vm, id));
    }
    for id in hat_boxes(vm, world) {
        answered += usize::from(pass_report(vm, id));
        if showing(vm, id, ASKS_TO_WEAR)
            && local(vm, id, WEAR_IT).and_then(|v| v.as_i32()) == Some(-1)
        {
            vm.set_local(id, WEAR_IT, Value::Int(0));
            answered += 1;
        }
    }
    answered
}

/// The outfit record a hat box reads once its stream is in, registered with
/// no actor, once a run on a level placing a box (openspec AS.158). With no
/// record the box's two reads through it drop, so its clone copies whatever
/// `Actor Scratch G1` last held, the hero as often as not, and its `my hat`
/// is him: it hid him and took him off. With this one the scratch reads
/// nothing, the clone is skipped and `my hat` stays nothing.
///
/// Whether it did, this step. A stop in the macro isn't tried again.
pub fn answer_hat_load(vm: &mut Vm, host: &mut WorldHost<'_, '_>) -> Result<bool, Halt> {
    if host.world.hat_load_answered
        || hat_boxes(vm, host.world).is_empty()
        || vm.program().macro_named(REGISTER_OUTFIT).is_none()
    {
        return Ok(false);
    }
    host.world.hat_load_answered = true;
    vm.call_with(None, REGISTER_OUTFIT, &[], &[], host)?;
    Ok(true)
}

/// The name of each hat a box and each legendary treasure is showing its
/// report for, as the report's bar names it: the strings the TV puts up in
/// its place.
pub fn hats_shown(vm: &Vm, world: &ScriptWorld) -> Vec<Value> {
    let named = |ids: Vec<PlacementId>, name: &str| {
        ids.into_iter()
            .filter(|id| showing(vm, *id, REPORTS))
            .filter_map(|id| local(vm, id, name))
            .filter(|s| *s != Value::Null)
            .collect::<Vec<_>>()
    };
    let mut shown = named(hat_boxes(vm, world), HAT_NAME);
    shown.extend(named(legendary(vm, world), TREASURE_NAME));
    shown
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
                |w| {
                    let (at, _) = w.define(REGISTER_OUTFIT, &[]);
                    w.flow_end();
                    w.end_block(at);
                },
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

    fn answered(vm: &mut Vm, world: &mut ScriptWorld) -> bool {
        let mut host = WorldHost {
            world,
            engine: &mut NoEngine,
            generator: &mut Generator::seeded(1),
            spawn: &mut headless_spawner(0x2000),
        };
        answer_hat_load(vm, &mut host).unwrap()
    }

    /// openspec AS.158: a level placing a box registers the hat's empty
    /// outfit once a run, and one placing none registers nothing.
    #[test]
    fn a_box_has_its_empty_outfit_registered_once() {
        let (mut vm, mut world, _) = hat_box();
        assert!(answered(&mut vm, &mut world), "the box's level");
        assert!(!answered(&mut vm, &mut world), "registered again");
        let (mut vm, _, _) = hat_box();
        let handles = HashMap::new();
        let mut none = ScriptWorld::new(&Files::default(), &handles, headless_objects(&handles));
        assert!(!answered(&mut vm, &mut none), "a level with no box");
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
        for s in CHEST_SCRIPTS
            .iter()
            .chain(&HAT_SCRIPTS)
            .chain(&LEGENDARY_SCRIPTS)
            .chain(&crate::scripts::ELEMENTAL_SCRIPTS)
        {
            let path = format!("C:/tfb/Content/Levels/Includes/x{s}");
            assert!(c.whitelist.lists("a placement", &path), "{s}");
        }
    }

    /// openspec AS.151: a legendary treasure waits for the save as a chest
    /// does, its STARTUP taking a collected one off.
    #[test]
    fn a_legendary_treasure_waits_for_the_save() {
        const TREASURE: &str = "C:/tfb/Content/Levels/Includes/Loot_Specials/Loot_Specials.ai";
        let placed = Placed::named(FILE, "it", Some(TREASURE), 4);
        let mut actor = Actor::blank();
        actor.obj = Some(FILE);
        let templates = HashMap::from([(FILE, placed)]);
        assert!(waits_for_the_save(&templates, &actor));
    }
}
