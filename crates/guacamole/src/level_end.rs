//! A level's end on the TV's run (openspec AS.15): what stands between the
//! scripts' own level end and the next level, ours where the host doesn't
//! run the original's yet. Chapter 1's end runs on the VM from
//! `Temp_Level_Ender`'s wake (its fade, the outro woken, the outro's
//! cutscene played to its end since AS.52:
//! [`crate::scripts::LEVEL_END_SCRIPTS`]) to the outro's own `Game - Level
//! End`, which records the level to load next in `level progression data`;
//! then, each step after the walk ([`step`]):
//!
//! - A cutscene the TV doesn't play to its end ([`SKIPPED_CUTSCENES`]:
//!   level 214's intro and its first sail, each waiting on our host on a
//!   walk whose scene object has no actor, AS.81 and AS.82) is skipped once
//!   it has run a frame, by the original's own skip (`CS Internal - Skip
//!   Cutscene` on the cutscene in play, as a player's skip press runs it),
//!   so the scripts after it run as they would after a skip.
//!   (Stand-in ledger: `cutscene-skip`.)
//! - The level end recorded (`level end seq` 1 or more with `level end load
//!   to level` set), the movie it queued (`Game - Level End - Play Movie`'s
//!   `level end movie sprite`: chapter 1's FMV02) is handed to the player
//!   first ([`crate::scripts::LevelEndMovie`], openspec AS.19), and once
//!   it's over the level it names is written to `system.load level` once,
//!   as the Pause Controller's level switch writes it, in place of the
//!   end-of-level sequencing between them (the completion screen and the
//!   switch's clean-up; stand-in ledger: `level-end`).
//!   The host takes it from there ([`crate::scripts::ScriptWorld::end_step`]),
//!   holding it and saying why when it can't be read as a level on the disc
//!   ([`crate::scripts::Unread`]: a first run's chapter 1 has no interface's
//!   model to read its English text by), and the TV reads it again
//!   ([`crate::scripts::ScriptWorld::read_level_end`]).

use guac_script::{Halt, Value, Vm};
use tracing::info;

use crate::scripts::{LevelEndMovie, MovieEnd, MovieState, ScriptWorld, WorldHost, running};

/// The cutscenes the TV skips once they've run a frame, by the name of the
/// macro defining them: level 214's intro (openspec AS.47), which ran on
/// past 70 s headless and since AS.52's typed read waits at its sixth scene
/// of ten on a walk whose scene object has no actor (AS.81), and 214's sail
/// from its first island to its second (AS.59), which the first ship part's
/// pickup wakes and whose second scene's walker never arrives: its `CS Walk
/// Obj to Location` holds on an object whose actor reads nothing, one of the
/// cutscene file's own structures (AS.82), 32 s of it headless.
/// Chapter 1's outro left the list at AS.52: its first scene's step for
/// Norticus' animation read the step's dialogue's data as its own, the
/// scripts' typed reference read unmodelled, and its walks reached `move
/// to` on actors no creature's body walks.
pub const SKIPPED_CUTSCENES: [&str; 2] = ["214 Intro IGC", "214 recover 1st part IGC"];

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
    /// The movie the level end queued handed to the player, by the disc's
    /// name for it.
    Movie(String),
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

/// The movie the level end queued, by the disc's name for it (openspec
/// AS.19): `level progression data`'s `level end movie sprite` (a movie
/// sprite of the interface's), named by its source's file (`Movies/FMV02.mov`
/// is `fmv02`), or by its own name failing that (`FMV02_v40_0321.mov(1)`);
/// none queued, or a name that's no cinematic of the disc's.
pub fn queued_movie(vm: &Vm, world: &ScriptWorld) -> Option<String> {
    let data = vm.global_peek(vm.program().global_named("level progression data")?)?;
    let sprite = vm.field(data, "level end movie sprite")?;
    let source = world.interface().and_then(|k| {
        let m = k.model();
        match &m.sprite(m.sprite_of(sprite)?).content {
            guac_ir::interface::Content::Movie(art) => art.source.clone(),
            _ => None,
        }
    });
    let named = match sprite {
        Value::Object(o) => vm.program().name_of(o).map(str::to_owned),
        _ => None,
    };
    let name = movie_name(source.as_deref().or(named.as_deref())?);
    guac_extract::transcode::is_cinematic(&name).then_some(name)
}

/// A movie's disc name off a path or a sprite's name: its file name less
/// any extension and anything after its first `_` or `.`, lower-cased
/// (`Movies/FMV02.mov` and `FMV02_v40_0321.mov(1)` are both `fmv02`).
pub fn movie_name(path: &str) -> String {
    let file = path.rsplit(['/', '\\']).next().unwrap_or(path);
    file.split(['_', '.'])
        .next()
        .unwrap_or(file)
        .to_ascii_lowercase()
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
    // The movie the level end queued, first (openspec AS.19).
    if host.world.level_end_movie.is_none()
        && let Some(name) = queued_movie(vm, host.world)
    {
        let state = if host.world.plays_movies {
            MovieState::Queued
        } else {
            MovieState::Over(MovieEnd::Unplayed)
        };
        info!(movie = %name, ?state, "the level end's movie, before its level");
        host.world.level_end_movie = Some(LevelEndMovie {
            name: name.clone(),
            state,
        });
        if host.world.plays_movies {
            return Ok(Some(Ended::Movie(name)));
        }
    }
    if host
        .world
        .level_end_movie
        .as_ref()
        .is_some_and(|m| !matches!(m.state, MovieState::Over(_)))
    {
        return Ok(None);
    }
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
    /// than 0, `level end load to level` set to a marker object, and queues
    /// a movie too when `movie`, as `Game - Level End - Play Movie` does:
    /// `level end movie sprite` set to a marker named as chapter 1's FMV02
    /// sprite is.
    fn level_ends_with(seq: i32, movie: bool) -> Arc<Program> {
        let mut b = IgzBuilder::new();
        {
            let mut w = Writer::new(&mut b);
            let w = &mut w;
            let ty = w.op(Class::DefineStructure, "Level Progression Data");
            let seq_var = w.variable("level end seq", "ValueInfo", Some("IntMeasurement"));
            let to = w.variable("level end load to level", "ScriptReference", None);
            let sprite = w.variable("level end movie sprite", "ScriptReference", None);
            let data = w.b.object(0, "ScriptStructure", 0x20);
            w.text(data, 0x08, "level progression data");
            w.refer(data, 0x1C, ty);
            let marker = w.op(Class::DefineMacro, "Level_214");
            let fmv = w.op(Class::DefineMacro, "FMV02_v40_0321.mov(1)");
            let (pre, _) = w.open(Class::PreScript, "PRESCRIPT");
            let (start, _) = w.open(Class::StartUp, "STARTUP");
            let n = w.int(seq);
            w.set(&[El::Local(data), El::Local(seq_var)], n);
            let setref = |w: &mut Writer<'_>, var, to_what| {
                let l = w.value(&[El::Local(data), El::Local(var)]);
                let r = w.reference(&[El::Local(to_what)], None);
                let o = w.op(Class::SetReference, "setref||=");
                w.refer(o, 0x1C, l);
                w.refer(o, 0x20, r);
                w.push(o);
            };
            setref(w, to, marker);
            if movie {
                setref(w, sprite, fmv);
            }
            w.end_block(start);
            let (shut, _) = w.open(Class::ShutDown, "SHUTDOWN");
            w.end_block(shut);
            let (section, _) = w.open(Class::MacroSection, "MACROS");
            for def in [marker, fmv] {
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
            w.push(seq_var);
            w.push(to);
            w.push(sprite);
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
        run_with(seq, false, false, steps, &mut |_, _| {})
    }

    /// [`run`] over [`level_ends_with`], the world playing movies when
    /// `plays`, `between` called after each step with its index.
    fn run_with(
        seq: i32,
        movie: bool,
        plays: bool,
        steps: usize,
        between: &mut dyn FnMut(usize, &mut ScriptWorld),
    ) -> (Vec<Option<Ended>>, ScriptWorld, Value) {
        let program = level_ends_with(seq, movie);
        let files = Files {
            placed: vec![Placed::named(FILE, "Level End", Some(PATH), 4)],
            ..Files::default()
        };
        let handles = HashMap::from([(FILE, HANDLE)]);
        let mut world = ScriptWorld::new(&files, &handles, headless_objects(&handles));
        world.plays_movies = plays;
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
            .map(|k| {
                let ended = step(&mut vm, &mut host).unwrap();
                between(k, host.world);
                ended
            })
            .collect();
        (out, world, marker)
    }

    /// AS.19: a level end that queued a movie hands it to the player first,
    /// by the disc's name, and asks its level only once the movie is over,
    /// however it ended; with no player (a headless run) the movie is over
    /// as it's queued and the level asked in that step. None queued, the
    /// level is asked at once as before.
    #[test]
    fn a_level_ends_movie_plays_before_its_level_is_asked() {
        let mut hand = |k: usize, w: &mut ScriptWorld| match k {
            1 => w.movie_playing(),
            3 => w.movie_over(MovieEnd::Skipped),
            _ => {}
        };
        let (out, world, marker) = run_with(1, true, true, 6, &mut hand);
        assert_eq!(
            out,
            [
                Some(Ended::Movie("fmv02".into())),
                None,
                None,
                None,
                Some(Ended::Asked(marker)),
                None
            ]
        );
        assert_eq!(
            world.level_end_movie().map(|m| m.state.clone()),
            Some(MovieState::Over(MovieEnd::Skipped))
        );
        // No player: over as it's queued, the level asked at once.
        let (out, world, marker) = run_with(1, true, false, 2, &mut |_, _| {});
        assert_eq!(out, [Some(Ended::Asked(marker)), None]);
        assert_eq!(
            world.level_end_movie(),
            Some(&LevelEndMovie {
                name: "fmv02".into(),
                state: MovieState::Over(MovieEnd::Unplayed),
            })
        );
        // None queued: the level at once, no movie.
        let (out, world, marker) = run_with(1, false, true, 2, &mut |_, _| {});
        assert_eq!(out, [Some(Ended::Asked(marker)), None]);
        assert_eq!(world.level_end_movie(), None);
        // A movie's name off its source or its sprite's name.
        assert_eq!(movie_name("Movies/FMV02.mov"), "fmv02");
        assert_eq!(movie_name("FMV02_v40_0321.mov(1)"), "fmv02");
        assert_eq!(movie_name("c:\\x\\fmv11.mov"), "fmv11");
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
