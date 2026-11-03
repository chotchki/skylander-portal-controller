//! A level's end on the TV's run (openspec AS.15, AS.53): what stands
//! between the scripts' own level end and the next level. Chapter 1's end
//! runs on the VM from `Temp_Level_Ender`'s wake (its fade, the outro woken,
//! the outro's cutscene played to its end since AS.52:
//! [`crate::scripts::LEVEL_END_SCRIPTS`]) to the outro's own `Game - Level
//! End`, which records the level to load next in `level progression data`
//! (`level end seq` 1, `level end load to level`) and adds `System - End of
//! Level Sequencing` ([`SEQUENCING`]) to the game systems' update set,
//! which the player loop's game systems' tick runs where the original's
//! does ([`crate::player_loop::UPDATES`]). The sequencing's states are the
//! scripts', one a tick:
//!
//! 1. the gameplay paused, the HUD hidden, the music faded, the fanfare
//!    (`ui_success_fanfare`), `Game - Cutscene`, and the objectives screen's
//!    stream queued (`MiscLoad_Objectives`, read as loaded the next step:
//!    the stream loader runs since AS.53, a stream loading nothing, the
//!    `level-request` row);
//! 2. the completion screen, until it raises `level end seq` to 3;
//! 3. its stream let go; on to the movie (4) or past it (6);
//! 4. the movie the level end queued shown (`Level End Play Movie Macro`:
//!    its sprite cloned, its fader made);
//! 5. until its sprite is gone;
//! 6. `Change Level` set to the level end's level;
//!
//! and then the Pause Controller's level switch writes it to `system.load
//! level`. Each step after the walk ([`step`]) the TV meets them where the
//! original's engine or another of its scripts would:
//!
//! - A cutscene the TV doesn't play to its end ([`SKIPPED_CUTSCENES`], none
//!   since AS.81 and AS.82) is skipped once it has run a frame, by the
//!   original's own skip (`CS Internal - Skip Cutscene` on the cutscene in
//!   play, as a player's skip press runs it), so the scripts after it run as
//!   they would after a skip. (Stand-in ledger: `cutscene-skip`, struck.)
//!   A dev entry's settle on a level with an intro skips any cutscene so
//!   ([`crate::scripts::ScriptWorld::skip_cutscenes`]), so its anchor's
//!   set-down comes after the intro's hand-over, not before it.
//! - State 2: the objectives screen's own `UI - Objectives Menu - Collect
//!   Data` run over a fresh `Objectives Screen Data` ([`collect`]), what it
//!   holds then read as the [`LevelResults`] the TV's screen shows (its
//!   sprite tree never built: openspec AU.5, design decision 2); its
//!   continue runs the screen's own `UI - Objectives Menu - Shutdown`, which
//!   raises the sequencing on ([`continue_on`]). A run that shows no screen
//!   (a headless one) continues at once.
//! - State 5: the movie (FMV02 after chapter 1) handed to the TV's player
//!   ([`crate::scripts::LevelEndMovie`], openspec AS.19), and once it's over
//!   the fader's own clean-up (`UI - Clean up Movie Fader`), which takes the
//!   movie's sprite off as the fader's update does at a movie's end, so
//!   state 5 sees it gone (stand-in ledger: `level-end`: the player and its
//!   skip in place of the movie sprite and the fader's update).
//! - `Change Level` set: its level written to `system.load level` once, as
//!   the Pause Controller's level switch writes it, the switch's clean-up
//!   not run (the TV takes the run down at a level change; `level-end`).
//!   The host takes it from there ([`crate::scripts::ScriptWorld::end_step`]),
//!   holding it and saying why when it can't be read as a level on the disc
//!   ([`crate::scripts::Unread`]: a first run's chapter 1 has no interface's
//!   model to read its English text by), and the TV reads it again
//!   ([`crate::scripts::ScriptWorld::read_level_end`]).
//!
//! A run that can't sequence it ([`sequencing`]: no game systems' placement
//! running, or its script stopped) takes the level on as AS.15's stand-in
//! did: the movie the level end queued, then its level written.

use guac_script::{Halt, PlacementId, Value, Vm};
use tracing::{info, warn};

use crate::scripts::{LevelEndMovie, MovieEnd, MovieState, ScriptWorld, WorldHost, running};

/// The cutscenes the TV skips once they've run a frame, by the name of the
/// macro defining them: none. Chapter 1's outro left the list at AS.52 (its
/// animation step read its dialogue's data as its own, its walks reached
/// `move to` on actors no creature walks); level 214's intro (AS.81) and
/// its sail from its first island to its second (AS.82) at AS.82, whose
/// walks each held on Flynn's or Cali's object reading no actor: the
/// placements whose STARTUP defines those objects weren't on the TV's
/// listing ([`crate::scripts::with_cutscene_objects`]). Their skip is the
/// player's now, △ through the cutscene system's own check.
pub const SKIPPED_CUTSCENES: [&str; 0] = [];

/// The macro the original's skip runs, and the game systems' script whose
/// tick runs the cutscene system's update.
const SKIP: &str = "CS Internal - Skip Cutscene";
const GAME_SYSTEMS: &str = "GameSystems.ai";

/// The end-of-level sequencing, an update macro of the game systems' set.
pub const SEQUENCING: &str = "System - End of Level Sequencing";
/// The objectives screen's (`misc/ui_objectivesscreen.bld`): what it shows,
/// collected into its data, and its page's shutdown.
pub const COLLECT: &str = "UI - Objectives Menu - Collect Data";
const SHUTDOWN: &str = "UI - Objectives Menu - Shutdown";
const SCREEN_DATA: &str = "Objectives Screen Data";
const CURRENT_SCREEN: &str = "CurrentObjectivesScreen";
/// The movie fader's clean-up, and the set of faders it takes one from.
const CLEAN_UP_FADER: &str = "UI - Clean up Movie Fader";
const FADERS: &str = "movie faders";

/// The sequencing's states the TV meets (the module docs).
pub const COMPLETION: i32 = 2;
pub const MOVIE: i32 = 5;

/// What a step of [`step`] did.
#[derive(Debug, Clone, PartialEq)]
pub enum Ended {
    /// A listed cutscene skipped, by its macro's name.
    Skipped(String),
    /// The level end's level written to `system.load level`.
    Asked(Value),
    /// The movie the level end queued handed to the player, by the disc's
    /// name for it.
    Movie(String),
    /// The completion screen's state reached: its results read (or why
    /// not).
    Completion,
    /// The completion screen's continue run: the sequencing raised on.
    Continued,
}

/// The sequencing as the TV meets it on one run (the module docs).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Sequence {
    /// The completion screen, once state 2 was reached.
    pub completion: Option<Completion>,
    /// Whether the embedder shows the completion screen (the TV's): one
    /// that doesn't (a headless run) continues it at once.
    pub shows_completion: bool,
    /// The movie the level end queued, read while its sprite is still the
    /// template it named (states 1-4), by the disc's name for it.
    pub movie: Option<String>,
    /// Whether the fader's clean-up has run for the movie.
    pub fader_cleaned: bool,
}

/// The completion screen on the run: what it shows, and its continue.
#[derive(Debug, Clone, PartialEq)]
pub struct Completion {
    /// What the objectives screen's scripts computed, or why they couldn't.
    pub results: Result<LevelResults, String>,
    /// The TV's screen took a continue press: the next step continues.
    pub continue_asked: bool,
    /// The continue has run: the sequencing is past the screen.
    pub continued: bool,
}

/// What the completion screen shows (`UI - Objectives Menu - Collect
/// Data`'s, read off its data): the level's objectives in its three tabs
/// and the three stars, each text as the run reads it in English.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct LevelResults {
    /// The screen's own words: its page's title (`Objectives Menu Title`)
    /// and its continue prompt (`Objectives Menu Prompt - Continue`).
    pub title: Option<String>,
    pub prompt: Option<String>,
    /// The adventure the level is (`Current Adventure Title`).
    pub level: Option<String>,
    /// The three tabs' titles (`Item Titles`): story, dares, collection.
    pub tabs: Vec<Option<String>>,
    pub objectives: Vec<Objective>,
    /// The level clear, completion and collection stars.
    pub stars: Vec<bool>,
}

/// One objective as the screen defines it.
#[derive(Debug, Clone, PartialEq)]
pub struct Objective {
    /// Its tab: 0 story, 1 dares, 2 collection.
    pub category: i32,
    pub kind: ObjectiveKind,
    pub status: Status,
    pub description: Option<String>,
    /// How many of how many (a count), or the time taken and the target
    /// (a time, seconds).
    pub count: f32,
    pub total: f32,
}

/// How an objective reads (its `type`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ObjectiveKind {
    /// Done or not (type 0).
    Check,
    /// N of M (type 2).
    Count,
    /// A time against a target (type 3).
    Time,
}

impl ObjectiveKind {
    fn of(code: i32) -> Self {
        match code {
            2 => ObjectiveKind::Count,
            3 => ObjectiveKind::Time,
            _ => ObjectiveKind::Check,
        }
    }
}

/// An objective's completion (its `completion status`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Status {
    /// Not done (0).
    Open,
    /// Done this time (1).
    NewlyDone,
    /// Done before (2).
    Done,
}

impl Status {
    fn of(code: i32) -> Self {
        match code {
            1 => Status::NewlyDone,
            2 => Status::Done,
            _ => Status::Open,
        }
    }

    pub fn done(self) -> bool {
        self != Status::Open
    }
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

/// `level progression data`, once something has made it.
fn progression(vm: &Vm) -> Option<Value> {
    vm.global_peek(vm.program().global_named("level progression data")?)
}

/// The level end `level progression data` records: the level to load,
/// once `Game - Level End` has run through.
pub fn recorded(vm: &Vm) -> Option<Value> {
    let data = progression(vm)?;
    let seq = vm.field(data, "level end seq").and_then(|v| v.as_i32())?;
    let to = vm.field(data, "level end load to level")?;
    (seq >= 1 && to != Value::Null).then_some(to)
}

/// `level end seq`: where the sequencing stands.
pub fn seq(vm: &Vm) -> Option<i32> {
    vm.field(progression(vm)?, "level end seq")
        .and_then(|v| v.as_i32())
}

/// `Change Level`, once the sequencing (or anything else) has set it.
pub fn change_level(vm: &Vm) -> Option<Value> {
    vm.global_peek(vm.program().global_named("Change Level")?)
        .filter(|v| *v != Value::Null)
}

/// The game systems' placement, when it runs and hasn't stopped, and the
/// program holds the sequencing: then the run sequences a level's end on
/// the VM.
pub fn sequencing(vm: &Vm, world: &ScriptWorld) -> Option<PlacementId> {
    let gs = running(vm, world, GAME_SYSTEMS)?;
    (vm.stopped(gs).is_none() && vm.program().macro_named(SEQUENCING).is_some()).then_some(gs)
}

/// The movie the level end queued, by the disc's name for it (openspec
/// AS.19): `level progression data`'s `level end movie sprite` (a movie
/// sprite of the interface's), named by its source's file (`Movies/FMV02.mov`
/// is `fmv02`), or by its own name failing that (`FMV02_v40_0321.mov(1)`);
/// none queued, or a name that's no cinematic of the disc's.
pub fn queued_movie(vm: &Vm, world: &ScriptWorld) -> Option<String> {
    let data = progression(vm)?;
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

/// The completion screen's results, as its own scripts compute them: a
/// fresh `Objectives Screen Data` made `CurrentObjectivesScreen`, as the
/// screen's create makes one (entered at the level's end, not mid-level,
/// no debug, no player in control), then `UI - Objectives Menu - Collect
/// Data` run over it in the game systems' name (`gs`), then what it holds
/// read ([`read_results`]). Why not when the screen's scripts aren't linked
/// (the extract's `screens` part not in), or the collection stopped.
pub fn collect(
    vm: &mut Vm,
    host: &mut WorldHost<'_, '_>,
    gs: PlacementId,
) -> Result<LevelResults, String> {
    if vm.program().macro_named(COLLECT).is_none() {
        return Err("the objectives screen's scripts aren't linked".into());
    }
    let current = vm
        .program()
        .global_named(CURRENT_SCREEN)
        .ok_or("no CurrentObjectivesScreen")?;
    let screen = vm
        .new_structure(SCREEN_DATA)
        .ok_or("no Objectives Screen Data structure")?;
    if !vm.set_global_reference(current, screen) {
        return Err("CurrentObjectivesScreen holds no reference".into());
    }
    vm.call_for(gs, COLLECT, &[], &[], host)
        .map_err(|h| format!("its collection stopped: {h}"))?;
    Ok(read_results(vm, host.world, screen))
}

/// The results an `Objectives Screen Data` holds once collected: each of
/// its `objective def set` (category, type, completion status, description,
/// `count of total` as its two screen halves, `time value` for a time) and
/// its `star status completion set`, with the adventure's title and the
/// tabs' (each string's English text, as the run's
/// interface reads it), and the screen's own title and continue prompt.
pub fn read_results(vm: &mut Vm, world: &ScriptWorld, screen: Value) -> LevelResults {
    // The two file variables the screen reads, made as a script's read
    // makes them.
    let global = |vm: &mut Vm, name: &str| {
        vm.program()
            .global_named(name)
            .and_then(|g| vm.global(g))
            .map(|v| vm.value(v))
    };
    let item_titles = global(vm, "Item Titles");
    let adventure = global(vm, "Current Adventure Title");
    let vm = &*vm;
    let int = |v: Value, name: &str| vm.field(v, name).and_then(|f| f.as_i32()).unwrap_or(0);
    let text = |v: Option<Value>| {
        v.filter(|v| *v != Value::Null)
            .and_then(|v| world.text_of(v))
    };
    let set = |v: Option<Value>| -> Vec<Value> {
        v.and_then(|s| vm.set(s).map(<[Value]>::to_vec))
            .unwrap_or_default()
    };
    let objectives = set(vm.field(screen, "objective def set"))
        .into_iter()
        .map(|e| vm.value(e))
        .map(|d| {
            let kind = ObjectiveKind::of(int(d, "type"));
            let (count, total) = match vm.field(d, "count of total").map(|v| vm.value(v)) {
                Some(Value::Screen(bits)) => (half(bits >> 16), half(bits)),
                _ => (0.0, 0.0),
            };
            let count = match kind {
                ObjectiveKind::Time => vm
                    .field(d, "time value")
                    .and_then(|v| v.as_f32())
                    .unwrap_or(count),
                _ => count,
            };
            Objective {
                category: int(d, "category"),
                kind,
                status: Status::of(int(d, "completion status")),
                description: text(vm.field(d, "description")),
                count,
                total,
            }
        })
        .collect();
    let stars = set(vm.field(screen, "star status completion set"))
        .into_iter()
        .map(|e| vm.value(e).as_i32().is_some_and(|n| n != 0))
        .collect();
    let tabs = set(item_titles)
        .into_iter()
        .map(|e| text(Some(vm.value(e))))
        .collect();
    let own = |name: &str| {
        vm.program()
            .object_named(name)
            .and_then(|o| world.text_of(Value::Object(o)))
    };
    LevelResults {
        title: own("Objectives Menu Title"),
        prompt: own("Objectives Menu Prompt - Continue"),
        level: text(adventure),
        tabs,
        objectives,
        stars,
    }
}

/// A screen measurement's half, in sixteenths, as a number.
fn half(bits: u32) -> f32 {
    f32::from((bits & 0xFFFF) as u16 as i16) / 16.0
}

/// The completion screen's continue (state 2's way on): the screen's own
/// `UI - Objectives Menu - Shutdown` in the game systems' name when it's
/// linked and its data was made, which raises `level end seq` as the
/// original's continue does; else, or if it didn't, the raise alone.
pub fn continue_on(vm: &mut Vm, host: &mut WorldHost<'_, '_>, gs: PlacementId) -> Result<(), Halt> {
    let before = seq(vm);
    let made = host
        .world
        .sequence
        .completion
        .as_ref()
        .is_some_and(|c| c.results.is_ok());
    let mut halted = None;
    if made && vm.program().macro_named(SHUTDOWN).is_some() {
        halted = vm.call_for(gs, SHUTDOWN, &[], &[], host).err();
    }
    if seq(vm) == before
        && let (Some(data), Some(n)) = (progression(vm), before)
    {
        vm.set_field(data, "level end seq", Value::Int(n + 1));
    }
    match halted {
        Some(h) => Err(h),
        None => Ok(()),
    }
}

/// The movie's fader cleaned up once the TV's player is done with it: the
/// fader whose `movie sprite` is the level end's (`movie faders`), handed
/// to `UI - Clean up Movie Fader`, which takes the sprite off; with no such
/// fader, `level end seq` raised past the movie's wait alone.
fn clean_up_fader(vm: &mut Vm, host: &mut WorldHost<'_, '_>, gs: PlacementId) -> Result<(), Halt> {
    let sprite = progression(vm).and_then(|d| vm.field(d, "level end movie sprite"));
    let fader = vm
        .program()
        .global_named(FADERS)
        .and_then(|g| vm.global_peek(g))
        .and_then(|s| vm.set(s).map(<[Value]>::to_vec))
        .unwrap_or_default()
        .into_iter()
        .map(|f| vm.value(f))
        .find(|&f| sprite.is_some() && vm.field(f, "movie sprite") == sprite);
    match fader {
        Some(f) if vm.program().macro_named(CLEAN_UP_FADER).is_some() => {
            info!("the movie's over: its fader cleaned up, its sprite taken off");
            vm.call_for(gs, CLEAN_UP_FADER, &[("return movie fader", f)], &[], host)?;
        }
        _ => {
            info!("the movie's over, no fader of its: the sequencing raised past it");
            if let Some(data) = progression(vm) {
                vm.set_field(data, "level end seq", Value::Int(MOVIE + 1));
            }
        }
    }
    Ok(())
}

/// One step's level end, after the walk and before the host takes the
/// step's requests (the module docs). A skip that stops is the error; it
/// isn't tried again for that cutscene.
pub fn step(vm: &mut Vm, host: &mut WorldHost<'_, '_>) -> Result<Option<Ended>, Halt> {
    if let Some((current, name, time)) = cutscene(vm)
        && time > 0.0
        && (SKIPPED_CUTSCENES.contains(&name.as_str()) || host.world.skip_cutscenes)
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
    match sequencing(vm, host.world) {
        Some(gs) => sequenced(vm, host, gs),
        None => stand_in(vm, host, to),
    }
}

/// The step over a run that sequences the level's end (the module docs).
fn sequenced(
    vm: &mut Vm,
    host: &mut WorldHost<'_, '_>,
    gs: PlacementId,
) -> Result<Option<Ended>, Halt> {
    let now = seq(vm).unwrap_or(0);
    // The movie it queued, by name while its sprite is still the template.
    if host.world.sequence.movie.is_none() && (1..=4).contains(&now) {
        host.world.sequence.movie = queued_movie(vm, host.world);
    }
    if let Some(to) = change_level(vm) {
        host.world.level_end_asked = true;
        let string = match to {
            Value::Object(o) => vm.program().name_of(o).map(str::to_owned),
            _ => None,
        };
        info!(?string, text = ?host.world.text_of(to), "the sequencing set its level: asked for as the level switch would");
        host.world.ask_level_end(to, string);
        return Ok(Some(Ended::Asked(to)));
    }
    match now {
        COMPLETION => {
            let shows = host.world.sequence.shows_completion;
            let Some(c) = &host.world.sequence.completion else {
                let results = collect(vm, host, gs);
                match &results {
                    Ok(r) => info!(
                        objectives = r.objectives.len(),
                        stars = r.stars.iter().filter(|s| **s).count(),
                        "the completion screen's state: its results read off the objectives screen's scripts"
                    ),
                    Err(why) => {
                        warn!(reason = %why, "the completion screen's state: its results unread")
                    }
                }
                host.world.sequence.completion = Some(Completion {
                    results,
                    continue_asked: !shows,
                    continued: false,
                });
                if shows {
                    return Ok(Some(Ended::Completion));
                }
                return continue_now(vm, host, gs);
            };
            if c.continue_asked && !c.continued {
                return continue_now(vm, host, gs);
            }
            Ok(None)
        }
        MOVIE => {
            if host.world.level_end_movie.is_none() {
                let (state, name) = match host.world.sequence.movie.clone() {
                    Some(n) if host.world.plays_movies => (MovieState::Queued, n),
                    Some(n) => (MovieState::Over(MovieEnd::Unplayed), n),
                    None => (MovieState::Over(MovieEnd::Unplayed), String::new()),
                };
                info!(movie = %name, ?state, "the sequencing shows the level end's movie");
                let queued = state == MovieState::Queued;
                host.world.level_end_movie = Some(LevelEndMovie {
                    name: name.clone(),
                    state,
                });
                if queued {
                    return Ok(Some(Ended::Movie(name)));
                }
            }
            let over = host
                .world
                .level_end_movie
                .as_ref()
                .is_some_and(|m| matches!(m.state, MovieState::Over(_)));
            if over && !host.world.sequence.fader_cleaned {
                host.world.sequence.fader_cleaned = true;
                clean_up_fader(vm, host, gs)?;
            }
            Ok(None)
        }
        _ => Ok(None),
    }
}

/// The completion screen's continue run now (state 2's way on).
fn continue_now(
    vm: &mut Vm,
    host: &mut WorldHost<'_, '_>,
    gs: PlacementId,
) -> Result<Option<Ended>, Halt> {
    if let Some(c) = &mut host.world.sequence.completion {
        c.continued = true;
    }
    info!("the completion screen's continue: the sequencing raised on");
    continue_on(vm, host, gs)?;
    Ok(Some(Ended::Continued))
}

/// AS.15's stand-in over a run that can't sequence the level's end: the
/// movie the level end queued handed to the player, then its level asked.
fn stand_in(vm: &mut Vm, host: &mut WorldHost<'_, '_>, to: Value) -> Result<Option<Ended>, Halt> {
    if host.world.level_end_movie.is_none()
        && let Some(name) = host
            .world
            .sequence
            .movie
            .clone()
            .or_else(|| queued_movie(vm, host.world))
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

    /// The file of [`sequenced`]'s program, as the game systems' script.
    const GS_PATH: &str = "C:/tfb/Content/Levels/Includes/Global/scripts/GameSystems.ai";

    /// A file in the original's shape for the sequencing (AS.53), our ops:
    /// `level progression data` recording chapter 1's level end and FMV02 in
    /// its STARTUP (`level end seq` 1), `Change Level` and
    /// `CurrentObjectivesScreen` file references, the objectives screen's
    /// two structures, and the macros the TV calls: the sequencing (empty:
    /// the tests move `level end seq` as its states would), the screen's
    /// collection (one dare, 3 of 5, done this time; one star) and its
    /// shutdown (`level end seq` raised, `Shut` set). Its script is the
    /// game systems', so the run sequences.
    fn sequenced() -> Arc<Program> {
        let mut b = IgzBuilder::new();
        {
            let mut w = Writer::new(&mut b);
            let w = &mut w;
            let lpd_ty = w.op(Class::DefineStructure, "Level Progression Data");
            let seq_var = w.variable("level end seq", "ValueInfo", Some("IntMeasurement"));
            let to = w.variable("level end load to level", "ScriptReference", None);
            let sprite = w.variable("level end movie sprite", "ScriptReference", None);
            let def_ty = w.op(
                Class::DefineStructure,
                "UI - Objectives Menu - Objective Def",
            );
            let category = w.variable("category", "ValueInfo", Some("IntMeasurement"));
            let kind = w.variable("type", "ValueInfo", Some("IntMeasurement"));
            let status = w.variable("completion status", "ValueInfo", Some("IntMeasurement"));
            let description = w.variable("description", "ScriptReference", None);
            let count = w.variable(
                "count of total",
                "ScriptScreenInfo",
                Some("ScreenMeasurement"),
            );
            let time = w.variable("time value", "ValueInfo", Some("FloatMeasurement"));
            let screen_ty = w.op(Class::DefineStructure, "Objectives Screen Data");
            let defs = w.variable("objective def set", "ScriptSet", None);
            w.refer(defs, 0x24, def_ty);
            let stars = w.variable("star status completion set", "ScriptSet", Some("ValueInfo"));
            let data = w.b.object(0, "ScriptStructure", 0x20);
            w.text(data, 0x08, "level progression data");
            w.refer(data, 0x1C, lpd_ty);
            let change = w.b.object(0, "ScriptReference", 0x20);
            w.text(change, 0x08, "Change Level");
            let current = w.b.object(0, "ScriptReference", 0x20);
            w.text(current, 0x08, "CurrentObjectivesScreen");
            let shut = w.b.object(0, "ValueInfo", 0x24);
            w.text(shut, 0x08, "Shut");
            w.meta_word(shut, 0x1C, "IntMeasurement");
            let marker = w.op(Class::DefineMacro, "Level_214");
            let fmv = w.op(Class::DefineMacro, "FMV02_v40_0321.mov(1)");
            let sequencing = w.op(Class::DefineMacro, SEQUENCING);
            let collect = w.op(Class::DefineMacro, COLLECT);
            let shutdown = w.op(Class::DefineMacro, SHUTDOWN);
            let setref = |w: &mut Writer<'_>, var, to_what| {
                let l = w.value(&[El::Local(data), El::Local(var)]);
                let r = w.reference(&[El::Local(to_what)], None);
                let o = w.op(Class::SetReference, "setref||=");
                w.refer(o, 0x1C, l);
                w.refer(o, 0x20, r);
                w.push(o);
            };
            let (pre, _) = w.open(Class::PreScript, "PRESCRIPT");
            let (start, _) = w.open(Class::StartUp, "STARTUP");
            let one = w.int(1);
            w.set(&[El::Local(data), El::Local(seq_var)], one);
            setref(w, to, marker);
            setref(w, sprite, fmv);
            w.end_block(start);
            let (shut_down, _) = w.open(Class::ShutDown, "SHUTDOWN");
            w.end_block(shut_down);
            let (section, _) = w.open(Class::MacroSection, "MACROS");
            for def in [marker, fmv, sequencing, collect, shutdown] {
                let p = w.push(def);
                let (iface, _) = w.open(Class::MacroInterface, "INTERFACE");
                let enter = w.flow(Class::FlowMacro);
                w.push(enter);
                w.close(iface);
                if def == collect {
                    let set = [El::Local(current), El::Local(defs)];
                    w.inc(&[set[0], set[1], El::Member("setvariant", "count")]);
                    let last = |field| {
                        [
                            El::Local(current),
                            El::Local(defs),
                            El::Member("setvariant", "last"),
                            El::Local(field),
                        ]
                    };
                    for (field, v) in [(category, 1), (kind, 2), (status, 1)] {
                        let v = w.int(v);
                        w.set(&last(field), v);
                    }
                    let three_of_five = w.literal("ScreenMeasurement", 0x0030_0050, false);
                    w.set(&last(count), three_of_five);
                    let star = [El::Local(current), El::Local(stars)];
                    w.inc(&[star[0], star[1], El::Member("setvariant", "count")]);
                    let one = w.int(1);
                    w.set(
                        &[
                            El::Local(current),
                            El::Local(stars),
                            El::Member("setvariant", "last"),
                            El::Member("abstractscriptgroup", "valuedereferencer"),
                        ],
                        one,
                    );
                }
                if def == sequencing {
                    // Never run here; it names `Change Level` as the
                    // original's does, which links the file variable.
                    let l = w.value(&[El::Local(data), El::Local(to)]);
                    let r = w.reference(&[El::Local(change)], None);
                    let o = w.op(Class::SetReference, "setref||=");
                    w.refer(o, 0x1C, l);
                    w.refer(o, 0x20, r);
                    w.push(o);
                }
                if def == shutdown {
                    w.inc(&[El::Local(data), El::Local(seq_var)]);
                    let one = w.int(1);
                    w.set(&[El::Local(shut)], one);
                }
                w.end_block(p);
            }
            w.end_block(section);
            let (types, _) = w.open(Class::StructureSection, "STRUCTURES");
            for (ty, fields) in [
                (lpd_ty, vec![seq_var, to, sprite]),
                (
                    def_ty,
                    vec![category, kind, status, description, count, time],
                ),
                (screen_ty, vec![defs, stars]),
            ] {
                let p = w.push(ty);
                for f in fields {
                    w.push(f);
                }
                w.end_block(p);
            }
            w.end_block(types);
            w.end_block(pre);
            w.end(GS_PATH);
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

    /// [`sequenced`]'s file placed as the game systems' placement and ticked
    /// once (its STARTUP: the level end recorded), `prepare` then given the
    /// VM and the world, then `steps` of [`step`], `between` called after
    /// each: what each returned, the VM and the world after.
    fn run_sequenced(
        prepare: &mut dyn FnMut(&mut Vm, &mut ScriptWorld),
        steps: usize,
        between: &mut dyn FnMut(usize, &mut Vm, &mut ScriptWorld),
    ) -> (Vec<Option<Ended>>, Vm, ScriptWorld) {
        let program = sequenced();
        let files = Files {
            placed: vec![Placed::named(FILE, "Macros_GameSystems", Some(GS_PATH), 4)],
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
        {
            let mut host = WorldHost {
                world: &mut world,
                engine: &mut engine,
                generator: &mut generator,
                spawn: &mut spawn,
            };
            vm.step(&mut host).unwrap();
        }
        prepare(&mut vm, &mut world);
        let mut out = Vec::new();
        for k in 0..steps {
            let mut host = WorldHost {
                world: &mut world,
                engine: &mut engine,
                generator: &mut generator,
                spawn: &mut spawn,
            };
            out.push(step(&mut vm, &mut host).unwrap());
            between(k, &mut vm, &mut world);
        }
        (out, vm, world)
    }

    /// `level end seq` set as the sequencing's states would set it.
    fn set_seq(vm: &mut Vm, n: i32) {
        let data = progression(vm).unwrap();
        assert!(vm.set_field(data, "level end seq", Value::Int(n)));
    }

    /// `Shut`, which the screen's shutdown sets.
    fn shut(vm: &mut Vm) -> Value {
        let g = vm.program().global_named("Shut").unwrap();
        vm.global(g).unwrap()
    }

    /// AS.53, AU.5: the completion screen's state reads the screen's own
    /// collection over a fresh screen data (one dare, 3 of 5, done this
    /// time; one star) and waits there while the TV shows it; the continue
    /// runs the screen's own shutdown, raising the sequencing to 3. A run
    /// that shows no screen continues in the step it collects.
    #[test]
    fn the_completion_screen_reads_the_screens_own_collection_and_continues_by_its_shutdown() {
        let mut shows = |vm: &mut Vm, w: &mut ScriptWorld| {
            w.sequence.shows_completion = true;
            set_seq(vm, COMPLETION);
        };
        let mut ask = |k: usize, _: &mut Vm, w: &mut ScriptWorld| {
            if k == 2 {
                w.sequence.completion.as_mut().unwrap().continue_asked = true;
            }
        };
        let (out, mut vm, world) = run_sequenced(&mut shows, 5, &mut ask);
        assert_eq!(
            out,
            [
                Some(Ended::Completion),
                None,
                None,
                Some(Ended::Continued),
                None
            ]
        );
        assert_eq!(seq(&vm), Some(COMPLETION + 1), "raised by the shutdown");
        assert_eq!(shut(&mut vm), Value::Int(1), "the screen's own shutdown");
        let c = world.sequence.completion.as_ref().unwrap();
        assert!(c.continued);
        let r = c.results.as_ref().unwrap();
        assert_eq!(
            r.objectives,
            [Objective {
                category: 1,
                kind: ObjectiveKind::Count,
                status: Status::NewlyDone,
                description: None,
                count: 3.0,
                total: 5.0,
            }]
        );
        assert_eq!(r.stars, [true]);
        // No screen shown: continued as it's collected.
        let mut hidden = |vm: &mut Vm, _: &mut ScriptWorld| set_seq(vm, COMPLETION);
        let (out, mut vm, world) = run_sequenced(&mut hidden, 2, &mut |_, _, _| {});
        assert_eq!(out, [Some(Ended::Continued), None]);
        assert_eq!(seq(&vm), Some(COMPLETION + 1));
        assert_eq!(shut(&mut vm), Value::Int(1));
        assert!(world.sequence.completion.unwrap().results.is_ok());
    }

    /// AS.53: the movie is read by name while its sprite is the template the
    /// level end named, handed to the player only once the sequencing shows
    /// it (state 5), waited on while it plays, and once it's over the
    /// sequencing raised past it (the program holding no fader); then
    /// `Change Level`, once set, asked as the level switch would, once.
    #[test]
    fn the_movie_waits_for_its_state_and_the_sequencings_level_is_asked_once() {
        let mut seen = Vec::new();
        let mut drive = |k: usize, vm: &mut Vm, w: &mut ScriptWorld| {
            seen.push((
                seq(vm),
                w.level_end_movie().map(|m| m.state.clone()),
                w.sequence.movie.clone(),
            ));
            match k {
                1 => set_seq(vm, MOVIE),
                3 => w.movie_playing(),
                5 => w.movie_over(MovieEnd::Finished),
                7 => {
                    let to = recorded(vm).unwrap();
                    let g = vm.program().global_named("Change Level").unwrap();
                    assert!(vm.set_global_reference(g, to));
                }
                _ => {}
            }
        };
        let (out, vm, world) = run_sequenced(&mut |_, w| w.plays_movies = true, 10, &mut drive);
        let to = recorded(&vm).unwrap();
        assert_eq!(
            out,
            [
                None,
                None,
                Some(Ended::Movie("fmv02".into())),
                None,
                None,
                None,
                None,
                None,
                Some(Ended::Asked(to)),
                None
            ]
        );
        assert_eq!(
            seen[0],
            (Some(1), None, Some("fmv02".into())),
            "read by name in state 1, not handed"
        );
        assert_eq!(seen[2].1, Some(MovieState::Queued));
        assert_eq!(seen[4].0, Some(MOVIE), "waiting while it plays");
        assert_eq!(seen[6].0, Some(MOVIE + 1), "raised past it once it's over");
        assert_eq!(world.requests, [] as [Value; 0], "the host takes it after");
        assert_eq!(world.level_end().map(|e| e.value), Some(to));
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
