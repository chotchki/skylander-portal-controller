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

use guac_script::{Value, Vm};
use tracing::info;

use crate::scripts::ScriptWorld;

/// `Change Level`, none while it holds nothing (the switch reads it so).
pub fn change_level(vm: &Vm) -> Option<Value> {
    let g = vm.program().global_named("Change Level")?;
    vm.global_peek(g)
        .map(|v| vm.value(v))
        .filter(|v| *v != Value::Null)
}

/// One step's switch (the module docs): the level asked, once, when a
/// script set `Change Level` with no level end recorded.
pub fn step(vm: &Vm, world: &mut ScriptWorld) -> Option<Value> {
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

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::sync::Arc;

    use guac_formats::igz::Igz;
    use guac_formats::igz::builder::IgzBuilder;
    use guac_formats::script::builder::{El, Writer};
    use guac_formats::script::{Class, Graph};
    use guac_script::{Config, Generator, Mode, Obj, Program, Role, Unit, Whitelist};

    use super::*;
    use crate::script_engine::NoEngine;
    use crate::scripts::{Files, Placed, WorldHost, headless_objects, headless_spawner};

    const FILE: Obj = Obj { unit: 0, index: 2 };
    const HANDLE: u64 = 0x30;
    const PATH: &str = "C:/tfb/Content/Levels/Level_Hub_2/Scripts/Flynn.ai";

    /// A file in the original's shape, our ops: `Change Level` a file
    /// reference its STARTUP sets to a marker named as Flynn's `Level_201`
    /// when `changes`, and `level progression data` recording a level end
    /// (`level end seq` 1, its level another marker) when `ended`.
    fn flynn(changes: bool, ended: bool) -> Arc<Program> {
        let mut b = IgzBuilder::new();
        {
            let mut w = Writer::new(&mut b);
            let w = &mut w;
            let ty = w.op(Class::DefineStructure, "Level Progression Data");
            let seq = w.variable("level end seq", "ValueInfo", Some("IntMeasurement"));
            let to = w.variable("level end load to level", "ScriptReference", None);
            let data = w.b.object(0, "ScriptStructure", 0x20);
            w.text(data, 0x08, "level progression data");
            w.refer(data, 0x1C, ty);
            let change = w.b.object(0, "ScriptReference", 0x20);
            w.text(change, 0x08, "Change Level");
            let level_201 = w.op(Class::DefineMacro, "Level_201");
            let village = w.op(Class::DefineMacro, "999_Level_Village");
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
            if ended {
                let one = w.int(1);
                w.set(&[El::Local(data), El::Local(seq)], one);
                setref(w, &[El::Local(data), El::Local(to)], village);
            }
            if changes {
                setref(w, &[El::Local(change)], level_201);
            } else {
                // Named, so the file variable links.
                let l = w.value(&[El::Local(data), El::Local(to)]);
                let r = w.reference(&[El::Local(change)], None);
                let o = w.op(Class::SetReference, "setref||=");
                w.refer(o, 0x1C, l);
                w.refer(o, 0x20, r);
                w.push(o);
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
            let p = w.push(ty);
            w.push(seq);
            w.push(to);
            w.end_block(p);
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

    /// The file's placement ticked once (its STARTUP), then `steps` of
    /// [`step`]: what each asked, the world after and the `Level_201`
    /// marker.
    fn run(changes: bool, ended: bool, steps: usize) -> (Vec<Option<Value>>, ScriptWorld, Value) {
        let program = flynn(changes, ended);
        let files = Files {
            placed: vec![Placed::named(FILE, "Flynn", Some(PATH), 4)],
            ..Files::default()
        };
        let handles = HashMap::from([(FILE, HANDLE)]);
        let mut world = ScriptWorld::new(&files, &handles, headless_objects(&handles));
        let mut config = Config::new(Mode::Strict);
        config.whitelist = Whitelist::only(&[], &["Flynn.ai"]);
        let sid = program.script_named("Flynn.ai").unwrap();
        let marker = program
            .defs
            .iter()
            .find(|d| d.display == "Level_201")
            .map(|d| Value::Object(d.obj))
            .expect("the marker");
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
        let out = (0..steps).map(|_| step(&vm, host.world)).collect();
        (out, world, marker)
    }

    /// openspec AU.6: a script's `Change Level` with no level end recorded
    /// (Flynn's outro) is asked once, kept as the run's level end with its
    /// string's name and written where a script's `system.load level` write
    /// lands; one with a level end recorded is the level end's to ask, and
    /// nothing set asks nothing.
    #[test]
    fn a_scripts_change_level_is_asked_once_as_the_switch_would() {
        let (out, mut world, marker) = run(true, false, 3);
        assert_eq!(out, [Some(marker), None, None]);
        assert_eq!(
            world.level_end().and_then(|e| e.string.clone()).as_deref(),
            Some("Level_201")
        );
        world.set_levels(Arc::new(std::collections::HashSet::new()));
        world.end_step();
        assert_eq!(world.requests, [marker], "the request the host takes");
        let (out, world, _) = run(true, true, 3);
        assert_eq!(out, [None, None, None], "the level end's own");
        assert!(world.level_end().is_none());
        let (out, world, _) = run(false, false, 3);
        assert_eq!(out, [None, None, None]);
        assert!(world.level_end().is_none());
    }
}
