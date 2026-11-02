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
//!   The host takes it from there ([`crate::scripts::ScriptWorld::end_step`]).

use guac_script::{Halt, Value, Vm};
use tracing::info;

use crate::scripts::{WorldHost, running};

/// The cutscenes the TV skips once they've run a frame, by the name of the
/// macro defining them: chapter 1's outro, whose first scene's sixth step
/// (Norticus' animation) waits on a thread time our host never brings past
/// its delay of 0.
pub const SKIPPED_CUTSCENES: [&str; 1] = ["213 Outro IGC"];

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
    info!(level = ?host.world.text_of(to), "the level end recorded: its level asked for as the level switch would");
    host.world.ask_level(to);
    Ok(Some(Ended::Asked(to)))
}
