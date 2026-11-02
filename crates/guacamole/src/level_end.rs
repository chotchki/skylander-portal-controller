//! A level's end on the TV's run (openspec AS.15): what stands between the
//! scripts' own level end and the next level, ours where the host doesn't
//! run the original's yet. Chapter 1's end runs on the VM from
//! `Temp_Level_Ender`'s wake (its fade, the outro woken, the outro's
//! cutscene begun: [`crate::scripts::LEVEL_END_SCRIPTS`]) to the outro's own
//! `Game - Level End`, which records the level to load next in `level
//! progression data`; then, each step after the walk ([`step`]):
//!
//! - A cutscene the TV doesn't play to its end ([`SKIPPED_CUTSCENES`]: the
//!   outro, whose first scene stalls on a step our host never starts) is
//!   skipped once it has run a frame, by the original's own skip (`CS
//!   Internal - Skip Cutscene` on the cutscene in play, as a player's skip
//!   press runs it), so the scripts after it run as they would after a skip
//!   (stand-in ledger: `cutscene-skip`).
//! - The level end recorded (`level end seq` 1 or more with `level end load
//!   to level` set), the level it names is written to `system.load level`
//!   once, as the Pause Controller's level switch writes it, in place of the
//!   end-of-level sequencing between them (the completion screen, the movie
//!   the outro queues, the switch's clean-up; stand-in ledger: `level-end`).
//!   The host takes it from there ([`crate::scripts::ScriptWorld::end_step`]),
//!   holding it and saying why when it can't be read as a level on the disc
//!   ([`crate::scripts::Unread`]: a first run's chapter 1 has no interface's
//!   model to read its English text by), and the TV reads it again
//!   ([`crate::scripts::ScriptWorld::read_level_end`]).

use guac_script::{Halt, Value, Vm};
use tracing::info;

use crate::scripts::{WorldHost, running};

/// The cutscenes the TV skips once they've run a frame, by the name of the
/// macro defining them: chapter 1's outro, whose first scene's sixth step
/// (Norticus' animation) waits on a thread time our host never brings past
/// its delay of 0, level 214's intro (openspec AS.47), whose first scene
/// runs on and never ends on our host (70 s of it, headless), and 214's
/// sail from its first island to its second (AS.59), which the first ship
/// part's pickup wakes and whose second scene's walker never arrives: its
/// `CS Walk Obj to Location` holds on an object whose actor reads nothing,
/// one of the cutscene file's own structures (AS.82), 32 s of it headless.
pub const SKIPPED_CUTSCENES: [&str; 3] =
    ["213 Outro IGC", "214 Intro IGC", "214 recover 1st part IGC"];

/// The macro the original's skip runs, and the game systems' script whose
/// tick runs the cutscene system's update.
const SKIP: &str = "CS Internal - Skip Cutscene";
const GAME_SYSTEMS: &str = "GameSystems.ai";

/// What a step of [`step`] did: a listed cutscene skipped, or the level end's
/// level handed to the host.
#[derive(Debug, Clone, PartialEq)]
pub enum Ended {
    /// A listed cutscene skipped, by its macro's name.
    Skipped(String),
    /// The level end's level written to `system.load level`.
    Asked(Value),
}

/// The cutscene in play: `cs control data`'s current one, its definition's
/// macro's name, and how long it has run.
fn cutscene(vm: &Vm) -> Option<(Value, String, f32)> {
    let data = vm.global_peek(vm.program().global_named("cs control data")?)?;
    let current = vm.field(data, "current cs").filter(|v| *v != Value::Null)?;
    let def = vm.field(current, "cutscene definition")?;
    let name = match vm.field(def, "define macro")? {
        Value::Object(o) => vm.program().name_of(o)?.to_owned(),
        _ => return None,
    };
    let time = vm
        .field(current, "cutscene time")
        .and_then(|v| v.as_f32())
        .unwrap_or(0.0);
    Some((current, name, time))
}

/// The level end `level progression data` records: the level to load,
/// once `Game - Level End` has run through.
pub fn recorded(vm: &Vm) -> Option<Value> {
    let data = vm.global_peek(vm.program().global_named("level progression data")?)?;
    let seq = vm.field(data, "level end seq").and_then(|v| v.as_i32())?;
    let to = vm.field(data, "level end load to level")?;
    (seq >= 1 && to != Value::Null).then_some(to)
}

/// One step's level end, after the walk and before the host takes the
/// step's requests (the module docs). A skip that stops is the error; it
/// isn't tried again for that cutscene.
pub fn step(vm: &mut Vm, host: &mut WorldHost<'_, '_>) -> Result<Option<Ended>, Halt> {
    if let Some((current, name, time)) = cutscene(vm)
        && time > 0.0
        && SKIPPED_CUTSCENES.contains(&name.as_str())
        && host.world.skipped_cutscene != Some(current)
    {
        host.world.skipped_cutscene = Some(current);
        let Some(gs) = running(vm, host.world, GAME_SYSTEMS) else {
            return Ok(None);
        };
        info!(cutscene = %name, "a cutscene the TV doesn't play to its end: skipped as a player's skip would");
        vm.call_for(gs, SKIP, &[("skip cutscene data", current)], &[], host)?;
        return Ok(Some(Ended::Skipped(name)));
    }
    if host.world.level_end_asked {
        return Ok(None);
    }
    let Some(to) = recorded(vm) else {
        return Ok(None);
    };
    host.world.level_end_asked = true;
    let string = match to {
        Value::Object(o) => vm.program().name_of(o).map(str::to_owned),
        _ => None,
    };
    info!(?string, text = ?host.world.text_of(to), "the level end recorded: its level asked for as the level switch would");
    host.world.ask_level_end(to, string);
    Ok(Some(Ended::Asked(to)))
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
    use crate::scripts::{Files, Placed, ScriptWorld, headless_objects, headless_spawner};

    const FILE: Obj = Obj { unit: 0, index: 2 };
    const HANDLE: u64 = 0x30;
    const PATH: &str = "C:/tfb/Content/Levels/Includes/Global/scripts/LevelEnd.ai";

    /// A file in the original's shape, our ops: `level progression data` a
    /// file structure whose STARTUP records a level end when `seq` is more
    /// than 0, `level end load to level` set to a marker object.
    fn level_ends(seq: i32) -> Arc<Program> {
        let mut b = IgzBuilder::new();
        {
            let mut w = Writer::new(&mut b);
            let w = &mut w;
            let ty = w.op(Class::DefineStructure, "Level Progression Data");
            let seq_var = w.variable("level end seq", "ValueInfo", Some("IntMeasurement"));
            let to = w.variable("level end load to level", "ScriptReference", None);
            let data = w.b.object(0, "ScriptStructure", 0x20);
            w.text(data, 0x08, "level progression data");
            w.refer(data, 0x1C, ty);
            let marker = w.op(Class::DefineMacro, "Level_214");
            let (pre, _) = w.open(Class::PreScript, "PRESCRIPT");
            let (start, _) = w.open(Class::StartUp, "STARTUP");
            let n = w.int(seq);
            w.set(&[El::Local(data), El::Local(seq_var)], n);
            let l = w.value(&[El::Local(data), El::Local(to)]);
            let r = w.reference(&[El::Local(marker)], None);
            let o = w.op(Class::SetReference, "setref||=");
            w.refer(o, 0x1C, l);
            w.refer(o, 0x20, r);
            w.push(o);
            w.end_block(start);
            let (shut, _) = w.open(Class::ShutDown, "SHUTDOWN");
            w.end_block(shut);
            let (section, _) = w.open(Class::MacroSection, "MACROS");
            let p = w.push(marker);
            let (iface, _) = w.open(Class::MacroInterface, "INTERFACE");
            let enter = w.flow(Class::FlowMacro);
            w.push(enter);
            w.close(iface);
            w.end_block(p);
            w.end_block(section);
            let (types, _) = w.open(Class::StructureSection, "STRUCTURES");
            let p = w.push(ty);
            w.push(seq_var);
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
            name: "global",
            role: Role::Other,
            igz: &igz,
            graph: &graph,
        };
        Arc::new(Program::link(&[unit]).unwrap())
    }

    /// The file's placement ticked once (its STARTUP), then `steps` of
    /// [`step`] over it: what each returned and the world after.
    fn run(seq: i32, steps: usize) -> (Vec<Option<Ended>>, ScriptWorld, Value) {
        let program = level_ends(seq);
        let files = Files {
            placed: vec![Placed::named(FILE, "Level End", Some(PATH), 4)],
            ..Files::default()
        };
        let handles = HashMap::from([(FILE, HANDLE)]);
        let mut world = ScriptWorld::new(&files, &handles, headless_objects(&handles));
        let mut config = Config::new(Mode::Strict);
        config.whitelist = Whitelist::only(&[], &["LevelEnd.ai"]);
        let sid = program.script_named("LevelEnd.ai").unwrap();
        let marker = program
            .defs
            .iter()
            .find(|d| d.display == "Level_214")
            .map(|d| Value::Object(d.obj))
            .expect("the marker");
        let mut vm = Vm::new(program, config);
        vm.add_at(HANDLE, "Level End", sid, FILE);
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
        let out = (0..steps)
            .map(|_| step(&mut vm, &mut host).unwrap())
            .collect();
        (out, world, marker)
    }

    /// A level end recorded is asked for once, the level it names written
    /// where a script's `system.load level` write lands; none recorded, or
    /// `level end seq` still 0, asks nothing.
    #[test]
    fn a_recorded_level_end_is_asked_for_once() {
        let (out, mut world, marker) = run(1, 3);
        assert_eq!(out, [Some(Ended::Asked(marker)), None, None]);
        world.set_levels(Arc::new(std::collections::HashSet::new()));
        world.end_step();
        assert_eq!(world.requests, [marker], "the request the host takes");
        let (out, world, _) = run(0, 3);
        assert_eq!(out, [None, None, None]);
        assert!(world.requests.is_empty());
    }
}
