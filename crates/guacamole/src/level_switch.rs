//! A level changed by a script's own `Change Level` (openspec AU.6): the
//! hub's way into the next chapter. Flynn's outro on the hub (`HS2 -
//! Outro` after level 214) ends on `Cutscene Sequence - Change Level` with
//! no level end macro, which sets `Change Level` to the chapter Flynn
//! offers (`Level_to_Load`, `Level_201` after 214) and records no level end;
//! the original's Pause Controller sees it and switches level, writing it to
//! `system.load level` behind its load screen.
//!
//! Each step after the walk, once a script has set `Change Level` and no
//! level end is recorded (that one is [`crate::level_end`]'s), its level is
//! written to `system.load level` once, as the switch writes it, and kept as
//! the run's level end, so one whose string doesn't read as a level on the
//! disc is held and said rather than read as a stream. The switch's load
//! screen and clean-up don't run (the TV takes the run down at a level
//! change; stand-in ledger: `level-end`).
//!
//! **The village** (openspec AU.27). A level change naming
//! `999_Level_Village` (level 214's level end, the pause menu's return)
//! goes, in the original's switch, to the hub stage the village opens now
//! (`Global - Get Village Load`). Its text names no level, so once the
//! step's level end names it, this step resolves it by the same rule the
//! title's load uses ([`crate::story_save::village`]) and the step's end
//! asks that stage by name.

use guac_script::{Value, Vm};
use tracing::{info, warn};

use crate::scripts::ScriptWorld;

/// The level string a level change names the hub by.
pub const VILLAGE: &str = "999_Level_Village";

/// `Change Level`, none while it holds nothing (the switch reads it so).
pub fn change_level(vm: &Vm) -> Option<Value> {
    let g = vm.program().global_named("Change Level")?;
    vm.global_peek(g)
        .map(|v| vm.value(v))
        .filter(|v| *v != Value::Null)
}

/// One step's switch (the module docs): the level asked, once, when a
/// script set `Change Level` with no level end recorded; then a level end
/// naming the village, this step's or the level end's, resolved.
pub fn step(vm: &Vm, world: &mut ScriptWorld) -> Option<Value> {
    let asked = ask(vm, world);
    resolve_village(vm, world);
    asked
}

fn ask(vm: &Vm, world: &mut ScriptWorld) -> Option<Value> {
    if world.level_end_asked || crate::level_end::recorded(vm).is_some() {
        return None;
    }
    let to = change_level(vm)?;
    world.level_end_asked = true;
    let string = match to {
        Value::Object(o) => vm.program().name_of(o).map(str::to_owned),
        _ => None,
    };
    info!(?string, text = ?world.text_of(to), "a script changed level: asked for as the level switch would");
    world.ask_level_end(to, string);
    Some(to)
}

/// The level end, when it names the village and isn't resolved yet,
/// resolved to the hub stage the village opens (the module docs).
fn resolve_village(vm: &Vm, world: &mut ScriptWorld) {
    let Some(end) = world.level_end() else {
        return;
    };
    if end.resolved.is_some() || end.string.as_deref() != Some(VILLAGE) {
        return;
    }
    match crate::story_save::village(vm) {
        Some(stage) => {
            info!(%stage, "the level end names the village: the hub's stage its unlocks open");
            world.resolve_level_end(stage);
        }
        // Said once: the step's end holds it unread from here.
        None if end.unread.is_none() => {
            warn!("the level end names the village, and the run holds no village unlocks")
        }
        None => {}
    }
}

#[cfg(test)]
mod tests {
    use std::collections::{HashMap, HashSet};
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
    const PATH: &str = "C:/tfb/Content/Levels/Level_Hub_2/Scripts/Flynn.ai";
    const LEVEL_201: &str = "Level_201";

    /// A file in the original's shape, our ops: `Change Level` a file
    /// reference its STARTUP sets to the marker named `change` (Flynn's
    /// `Level_201`, or the village) when given; `level progression data`
    /// recording a level end to the village (`level end seq` 1) when
    /// `ended`; and its `village unlocks`, three adventures' data, the first
    /// `done` of them `complete` (2, as `Game - Level End` sets it).
    fn flynn(change: Option<&str>, ended: bool, done: usize) -> Arc<Program> {
        let mut b = IgzBuilder::new();
        {
            let mut w = Writer::new(&mut b);
            let w = &mut w;
            let adventure_ty = w.op(Class::DefineStructure, "Adventure Data");
            let complete = w.variable("complete", "ValueInfo", Some("IntMeasurement"));
            let ty = w.op(Class::DefineStructure, "Level Progression Data");
            let seq = w.variable("level end seq", "ValueInfo", Some("IntMeasurement"));
            let to = w.variable("level end load to level", "ScriptReference", None);
            let unlocks = w.variable("village unlocks", "ScriptSet", None);
            w.refer(unlocks, 0x24, adventure_ty);
            let data = w.b.object(0, "ScriptStructure", 0x20);
            w.text(data, 0x08, "level progression data");
            w.refer(data, 0x1C, ty);
            let adventures: Vec<_> = ["Level_210", "Level_202", "Level_211"]
                .iter()
                .map(|name| {
                    let a = w.b.object(0, "ScriptStructure", 0x20);
                    w.text(a, 0x08, name);
                    w.refer(a, 0x1C, adventure_ty);
                    a
                })
                .collect();
            let change_level = w.b.object(0, "ScriptReference", 0x20);
            w.text(change_level, 0x08, "Change Level");
            let level_201 = w.op(Class::DefineMacro, LEVEL_201);
            let village = w.op(Class::DefineMacro, VILLAGE);
            let marker = |name: &str| if name == VILLAGE { village } else { level_201 };
            let setref = |w: &mut Writer<'_>, lhs: &[El<'_>], what| {
                let l = w.value(lhs);
                let r = w.reference(&[El::Local(what)], None);
                let o = w.op(Class::SetReference, "setref||=");
                w.refer(o, 0x1C, l);
                w.refer(o, 0x20, r);
                w.push(o);
            };
            let (pre, _) = w.open(Class::PreScript, "PRESCRIPT");
            let (start, _) = w.open(Class::StartUp, "STARTUP");
            for (k, &a) in adventures.iter().enumerate() {
                let set = w.stack(StackClass::Set, &[El::Local(data), El::Local(unlocks)]);
                let element = w.group(&[El::Local(a)], None);
                let o = w.op(Class::ChangeMembership, "set [set]||to");
                w.refer(o, 0x1C, set);
                w.word(o, 0x20, 0);
                w.refer(o, 0x24, element);
                w.push(o);
                let n = w.int(if k < done { 2 } else { 0 });
                w.set(&[El::Local(a), El::Local(complete)], n);
            }
            if ended {
                let one = w.int(1);
                w.set(&[El::Local(data), El::Local(seq)], one);
                setref(w, &[El::Local(data), El::Local(to)], village);
            }
            match change {
                Some(name) => setref(w, &[El::Local(change_level)], marker(name)),
                None => {
                    // Named, so the file variable links.
                    let l = w.value(&[El::Local(data), El::Local(to)]);
                    let r = w.reference(&[El::Local(change_level)], None);
                    let o = w.op(Class::SetReference, "setref||=");
                    w.refer(o, 0x1C, l);
                    w.refer(o, 0x20, r);
                    w.push(o);
                }
            }
            w.end_block(start);
            let (shut, _) = w.open(Class::ShutDown, "SHUTDOWN");
            w.end_block(shut);
            let (section, _) = w.open(Class::MacroSection, "MACROS");
            for def in [level_201, village] {
                let p = w.push(def);
                let (iface, _) = w.open(Class::MacroInterface, "INTERFACE");
                let enter = w.flow(Class::FlowMacro);
                w.push(enter);
                w.close(iface);
                w.end_block(p);
            }
            w.end_block(section);
            let (types, _) = w.open(Class::StructureSection, "STRUCTURES");
            for (ty, vars) in [(adventure_ty, vec![complete]), (ty, vec![seq, to, unlocks])] {
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
            name: "level",
            role: Role::Other,
            igz: &igz,
            graph: &graph,
        };
        Arc::new(Program::link(&[unit]).unwrap())
    }

    /// What a run of [`flynn`] leaves for the test: the program and VM after
    /// its STARTUP, and the world.
    struct Ran {
        vm: Vm,
        world: ScriptWorld,
    }

    impl Ran {
        fn marker(&self, name: &str) -> Value {
            self.vm
                .program()
                .defs
                .iter()
                .find(|d| d.display == name)
                .map(|d| Value::Object(d.obj))
                .expect("the marker")
        }

        /// The step's end with the disc's levels `levels`: the level change
        /// it holds, by name.
        fn end_step(&mut self, levels: &[&str]) -> Option<String> {
            let levels: HashSet<String> = levels.iter().map(|l| (*l).to_owned()).collect();
            self.world.set_levels(Arc::new(levels));
            self.world.end_step();
            self.world.level_change().map(|c| c.name.clone())
        }
    }

    /// The file's placement ticked once (its STARTUP).
    fn ran(change: Option<&str>, ended: bool, done: usize) -> Ran {
        let program = flynn(change, ended, done);
        let files = Files {
            placed: vec![Placed::named(FILE, "Flynn", Some(PATH), 4)],
            ..Files::default()
        };
        let handles = HashMap::from([(FILE, HANDLE)]);
        let mut world = ScriptWorld::new(&files, &handles, headless_objects(&handles));
        let mut config = Config::new(Mode::Strict);
        config.whitelist = Whitelist::only(&[], &["Flynn.ai"]);
        let sid = program.script_named("Flynn.ai").unwrap();
        let mut vm = Vm::new(program, config);
        vm.add_at(HANDLE, "Flynn", sid, FILE);
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
        Ran { vm, world }
    }

    /// openspec AU.6: a script's `Change Level` with no level end recorded
    /// (Flynn's outro) is asked once, kept as the run's level end with its
    /// string's name and written where a script's `system.load level` write
    /// lands; one with a level end recorded is the level end's to ask, and
    /// nothing set asks nothing.
    #[test]
    fn a_scripts_change_level_is_asked_once_as_the_switch_would() {
        let mut r = ran(Some(LEVEL_201), false, 0);
        let marker = r.marker(LEVEL_201);
        let out: Vec<_> = (0..3).map(|_| step(&r.vm, &mut r.world)).collect();
        assert_eq!(out, [Some(marker), None, None]);
        assert_eq!(
            r.world
                .level_end()
                .and_then(|e| e.string.clone())
                .as_deref(),
            Some(LEVEL_201)
        );
        r.world.set_levels(Arc::new(HashSet::new()));
        r.world.end_step();
        assert_eq!(r.world.requests, [marker], "the request the host takes");
        let mut r = ran(Some(LEVEL_201), true, 0);
        let out: Vec<_> = (0..3).map(|_| step(&r.vm, &mut r.world)).collect();
        assert_eq!(out, [None, None, None], "the level end's own");
        assert!(r.world.level_end().is_none());
        let mut r = ran(None, false, 0);
        let out: Vec<_> = (0..3).map(|_| step(&r.vm, &mut r.world)).collect();
        assert_eq!(out, [None, None, None]);
        assert!(r.world.level_end().is_none());
    }

    /// openspec AU.27: a level end naming the village (214's, asked by the
    /// level end) and a script's `Change Level` naming it (the pause menu's
    /// return, asked here) each go to the hub stage one past the village
    /// unlocks the run holds complete, by name, as the title's load does
    /// ([`crate::story_save::hub_stage`]); a level end naming a level isn't
    /// touched, and a stage the disc's list lacks is held unread.
    #[test]
    fn a_level_end_naming_the_village_asks_the_stage_its_unlocks_open() {
        for done in 0..=3 {
            let stage = format!("level_hub_2_stage_{}", done + 1);
            assert_eq!(crate::story_save::hub_stage(done), stage);
            // The level end's: recorded, asked as `level_end::step` asks it.
            let mut r = ran(None, true, done);
            let village = r.marker(VILLAGE);
            r.world.ask_level_end(village, Some(VILLAGE.to_owned()));
            assert_eq!(step(&r.vm, &mut r.world), None, "the level end's to ask");
            assert_eq!(
                r.world.level_end().and_then(|e| e.resolved.as_deref()),
                Some(stage.as_str())
            );
            let all = [
                "level_hub_2_stage_1",
                "level_hub_2_stage_2",
                "level_hub_2_stage_3",
                "level_hub_2_stage_4",
            ];
            assert_eq!(r.end_step(&all).as_deref(), Some(stage.as_str()), "{done}");
            // A script's own, the switch's to ask and resolve.
            let mut r = ran(Some(VILLAGE), false, done);
            assert_eq!(step(&r.vm, &mut r.world), Some(r.marker(VILLAGE)));
            assert_eq!(r.end_step(&all).as_deref(), Some(stage.as_str()), "{done}");
        }
        // The stage not on the disc's list: held, said why.
        let mut r = ran(Some(VILLAGE), false, 1);
        step(&r.vm, &mut r.world);
        assert_eq!(r.end_step(&["level_hub_2_stage_1"]), None);
        assert_eq!(
            r.world.level_end().and_then(|e| e.unread.clone()),
            Some(crate::scripts::Unread::NotALevel(
                "level_hub_2_stage_2".into()
            ))
        );
        // A level end naming a level is its text's, never the village's.
        let mut r = ran(Some(LEVEL_201), false, 2);
        step(&r.vm, &mut r.world);
        assert_eq!(r.world.level_end().and_then(|e| e.resolved.clone()), None);
    }
}
