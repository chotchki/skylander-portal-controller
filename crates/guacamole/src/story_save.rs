//! The original's save system on guacamole's host (openspec AS.38, AU.3,
//! AU.4; AU design decision 4): the save scripts run on the VM, the host
//! answers `system.save state`, and what they save is kept, by the names
//! they give it, in the store's row for the host profile
//! ([`guac_server::saves`]).
//!
//! # What the original saves, and when (a reading of `global.bld`'s and the title's scripts)
//!
//! - **What a save is.** Two sets of 32-bit words, each the save of one
//!   file: the slot's (`SaveGame`, saved under `SaveNames_Slots`' slot name,
//!   `Save_Slot_1` for slot 0) and the header (`SaveGame_Header`, under
//!   `Save_Header`). Every value the game keeps is a *save param*
//!   (`console save params`' fields, defined once by `Save - Define All Save
//!   Params` at the boot): a bit field of so many fields of so many bits at a
//!   bit index, signed or not, in the header or the slot. On the 01.00 disc
//!   the slot holds 35 (178 words: `level flags`, 12 bits for each of the 20
//!   adventures, the first bit the adventure done; `level flags -
//!   objectives` and `- areas`; `flags - global` and `- player`; the
//!   collections, from the story tablets and hats to the toys seen; the
//!   difficulty and its tracking; `enemy intros complete`; the village's
//!   numbers) and the header 9 (9 words: the save's version, the options,
//!   the PvP maps open, and per slot its level, minions, percent complete
//!   and time played). A param's fields pack low bit first across the words,
//!   one field running on into the next word where it crosses one, as the
//!   scripts' own `Save - Bit Field` macros read and write them.
//! - **How a script saves.** It asks `Save - Save Game`, which queues a
//!   request per file (the slot's, then the header's with `do global data`)
//!   on the save system's control data, and does nothing at all while the
//!   system isn't `activated` (only the title's first load, `save when
//!   inactive`, activates it). The system monitor's macro list runs `Save -
//!   Run Savegame` every frame while requests wait: it takes the next, names
//!   the file in `system.save name`, points `system.save data` at its set,
//!   and writes `system.save state` through a reference: 1 to load, 2 to
//!   save, 3 to finalize (a request whose `save` is -1: once the last save
//!   leaves the queue, `Save - Add Finalize Request` asks a global save whose
//!   first request finalizes, the header saved again behind it). Each frame after, it reads the state back: 0
//!   is the request done (`Save - Savegame Request Complete`); over 5 an
//!   error, which it writes back to 0 to clear, then answers by its error's
//!   handler, but for 7 on a load that may create (a load that finds no save
//!   saves one); anything from 1 to 5 still running.
//! - **When.** Each level's start (the level initializer's `Game - Init
//!   Minions in Level`: slot and header, the percent complete worked out
//!   first); a collection found (`Save - Collection Get/Set Internal`: the
//!   slot); a player or global progress flag set with `do save`; the
//!   header's options and PvP unlocks; quitting from the pause menu. A
//!   level's end saves nothing itself: `Game - Level End` marks its
//!   adventure done in `level flags` in memory, and the next level's start
//!   saves it, the Pause Controller's switch having written the header's
//!   slot level (`Save - Set Savegame Level`) on the way.
//! - **The load, and where it goes.** The title loads the chosen slot, then
//!   fills each adventure's `complete` from its `level flags` (`Title Fill
//!   in Live Game Data Based on Savegame Data`) and starts the game at chapter
//!   1 if its adventure isn't done, else level 214 if its isn't, else the hub
//!   (`Title - Load Appropriate Adventure Level and Start Game`), whose stage
//!   is one past the number of the hub's unlocking adventures done (levels
//!   210, 202 and 211: `Global - Get Village Load`, `Level_Hub_2_Stage_`).
//!
//! `system.save state`'s values, as the registry labels them: 0 inactive, 1
//! load, 2 save, 3 finalize, 4 active, 5 complete, then the errors, 6
//! device not found through 21 general error and the Xbox 360's login and
//! storage ones to 29 ([`label`]). No script compares it with 4 or 5.
//!
//! # What the host does
//!
//! - **The state.** A script's write of 1, 2 or 3 while nothing is asked is
//!   a request: the state reads 4 (active) until guacamole's store has done
//!   it, then 0, or the error as the labels name it (7 invalid path for a
//!   load with no save, 14 write error for a save the store refused, 13 read
//!   error for a load whose words don't fit the run's sets, 21 general error
//!   with no layout to read). Any other write is kept as written ([`SaveState`]).
//! - **A save** names each param of its file as the scripts name it and
//!   keeps it with every other value the session holds, the adventures in
//!   the order `level flags` indexes them; the host profile's row is written
//!   whole, under the run's layout ([`Layout::id`]). A session with no host
//!   keeps it for the session alone.
//! - **A load** writes the host's stored values into its file's words.
//!
//! What's ours (stand-in ledger: `story-save`): the run's start, each run
//! of the TV's being a fresh VM where the original's memory carries on.
//! [`start_run`] stands in for the title (the system activated, the save
//! loaded, the adventures filled, a fresh save marked in use as the title's
//! new game does) and for the Pause Controller's switch (the adventure the
//! level is named the current one, its slot level written), then makes the
//! level initializer's save; the system monitor's two save macros run
//! ahead of each step's walk in `Macros_Animation`'s name ([`tick`]), the
//! monitor's other macros not at all; and the words a run leaves are
//! carried into the next ([`Live`]), as the original's memory carries them.

use std::collections::BTreeMap;

use guac_script::{Halt, MemberId, Value, Vm};
use guac_server::saves::SaveValues;
use serde::Serialize;
use tracing::{info, warn};

use crate::scripts::{ScriptWorld, WorldHost};

/// `system.save state`'s values the host answers with, as the registry
/// labels them.
pub mod label {
    pub const INACTIVE: i32 = 0;
    pub const LOAD: i32 = 1;
    pub const SAVE: i32 = 2;
    pub const FINALIZE: i32 = 3;
    pub const ACTIVE: i32 = 4;
    pub const INVALID_PATH: i32 = 7;
    pub const READ_ERROR: i32 = 13;
    pub const WRITE_ERROR: i32 = 14;
    pub const GENERAL_ERROR: i32 = 21;
}

/// What guacamole keeps a save by: bumped whenever what's kept changes, so
/// a row from before reads as another layout (AU design decision 4).
pub const FORMAT: u32 = 1;

/// The game the save scripts are Giants'.
pub const GAME: &str = "BLUS30968";

/// A request a script made by writing the state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Asked {
    Load,
    Save,
    Finalize,
}

/// The host's half of `system.save state`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SaveState {
    state: i32,
    /// Made and not yet taken by the embedder.
    asked: Option<Asked>,
    /// Taken, and waiting on the store.
    waiting: Option<Asked>,
    /// Why the system monitor's save macros stopped on this run, which ends
    /// them for it ([`tick`]).
    monitor_stopped: Option<String>,
}

impl SaveState {
    pub fn get(&self) -> i32 {
        self.state
    }

    /// A script's write: 1, 2 or 3 with nothing asked or waiting is a
    /// request, read as active until it's done; anything else is kept as
    /// written.
    pub fn set(&mut self, v: i32) {
        let ask = match v {
            label::LOAD => Some(Asked::Load),
            label::SAVE => Some(Asked::Save),
            label::FINALIZE => Some(Asked::Finalize),
            _ => None,
        };
        match ask {
            Some(a) if self.asked.is_none() && self.waiting.is_none() => {
                self.asked = Some(a);
                self.state = label::ACTIVE;
            }
            Some(_) => {}
            None => self.state = v,
        }
    }

    /// The request made since the last take, now waiting on the store.
    pub fn take(&mut self) -> Option<Asked> {
        let a = self.asked.take()?;
        self.waiting = Some(a);
        Some(a)
    }

    /// The request waiting on the store, if one is.
    pub fn waiting(&self) -> Option<Asked> {
        self.waiting
    }

    /// The store has done the waiting request: the state reads `label` (0,
    /// or an error's).
    pub fn done(&mut self, label: i32) {
        self.waiting = None;
        self.state = label;
    }

    /// Why the system monitor's save macros stopped on this run, if they did.
    pub fn monitor_stopped(&self) -> Option<&str> {
        self.monitor_stopped.as_deref()
    }
}

/// A save param as `Save - Define Save Param` defines it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Param {
    pub name: String,
    pub header: bool,
    pub bit: u32,
    pub bits: u32,
    pub fields: u32,
    pub signed: bool,
}

/// Which of the two files a set of words is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum File {
    Slot,
    Header,
}

/// The save's layout as a run's scripts define it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Layout {
    pub params: Vec<Param>,
    /// The adventures' titles in the order `level flags` indexes them.
    pub adventures: Vec<String>,
    pub slot_words: usize,
    pub header_words: usize,
}

/// A number a script left as a whole number (a float truncated).
fn int_of(vm: &Vm, v: Value) -> Option<i64> {
    match vm.value(v) {
        Value::Int(i) => Some(i64::from(i)),
        Value::Float(f) => Some(f as i64),
        Value::Double(d) => Some(d as i64),
        _ => None,
    }
}

fn global(vm: &Vm, name: &str) -> Option<Value> {
    vm.global_peek(vm.program().global_named(name)?)
}

impl Layout {
    /// The layout off `vm`'s `console save params` and `level progression
    /// data`, once a run's init has defined them; `None` before.
    pub fn read(vm: &Vm) -> Option<Layout> {
        let csp = global(vm, "console save params")?;
        let mut params = Vec::new();
        for (name, v) in vm.fields(csp) {
            if !matches!(vm.value(v), Value::Struct(_)) {
                continue;
            }
            let field = |n: &str| vm.field(v, n).and_then(|x| int_of(vm, x));
            let (Some(header), Some(bit), Some(bits), Some(fields), Some(signed)) = (
                field("header param"),
                field("bit index"),
                field("num bits"),
                field("fields total"),
                field("signed"),
            ) else {
                continue;
            };
            params.push(Param {
                name: name.to_owned(),
                header: header != 0,
                bit: u32::try_from(bit).ok()?,
                bits: u32::try_from(bits).ok()?.min(32),
                fields: u32::try_from(fields).ok()?,
                signed: signed != 0,
            });
        }
        if params.is_empty() {
            return None;
        }
        let words = |n: &str| vm.field(csp, n).and_then(|s| vm.set(s)).map(<[_]>::len);
        let lpd = global(vm, "level progression data")?;
        let ads = vm.field(lpd, "adventure data")?;
        let adventures = vm
            .set(ads)?
            .iter()
            .map(|a| match vm.field(*a, "title").map(|t| vm.value(t)) {
                Some(Value::Object(o)) => vm.program().name_of(o).unwrap_or("").to_owned(),
                _ => String::new(),
            })
            .collect();
        Some(Layout {
            params,
            adventures,
            slot_words: words("save val set")?,
            header_words: words("header save val set")?,
        })
    }

    /// The layout's name in the store: [`FORMAT`] and an FNV-1a of every
    /// param's shape, the adventures' order and the files' lengths.
    pub fn id(&self) -> String {
        let mut h: u32 = 0x811c_9dc5;
        let mut eat = |s: &str| {
            for b in s.bytes().chain([0u8]) {
                h ^= u32::from(b);
                h = h.wrapping_mul(0x0100_0193);
            }
        };
        let mut params: Vec<&Param> = self.params.iter().collect();
        params.sort_by(|a, b| a.name.cmp(&b.name));
        for p in params {
            eat(&format!(
                "{}|{}|{}|{}|{}|{}",
                p.name, p.header, p.bit, p.bits, p.fields, p.signed
            ));
        }
        for a in &self.adventures {
            eat(a);
        }
        eat(&format!("{}|{}", self.slot_words, self.header_words));
        format!("{FORMAT}:{h:08x}")
    }

    fn of(&self, file: File) -> impl Iterator<Item = &Param> {
        self.params
            .iter()
            .filter(move |p| p.header == (file == File::Header))
    }

    /// How many words the run's set for `file` holds: a save whose set
    /// holds another number isn't this layout's.
    pub fn words(&self, file: File) -> usize {
        match file {
            File::Slot => self.slot_words,
            File::Header => self.header_words,
        }
    }

    /// Each of `file`'s params read off its words, as the scripts' `Save -
    /// Get Save Param` reads them: a 32-bit field the word's own value, a
    /// shorter one unsigned or two's complement by its param.
    pub fn decode(&self, file: File, words: &[i32]) -> BTreeMap<String, Vec<i64>> {
        self.of(file)
            .map(|p| {
                let fields = (0..p.fields)
                    .map(|i| field(words, p.bit + i * p.bits, p.bits, p.signed))
                    .collect();
                (p.name.clone(), fields)
            })
            .collect()
    }

    /// `values`' params of `file` written into its words, each field's low
    /// bits as the scripts' `Set Save Param` writes them; a param `values`
    /// lacks, or a field past its own, left as the words hold it.
    pub fn encode(&self, file: File, values: &BTreeMap<String, Vec<i64>>, words: &mut [i32]) {
        for p in self.of(file) {
            let Some(v) = values.get(&p.name) else {
                continue;
            };
            for (i, &x) in v.iter().take(p.fields as usize).enumerate() {
                put_field(words, p.bit + i as u32 * p.bits, p.bits, x);
            }
        }
    }
}

/// `bits` bits from bit `at`, low bit first across the words.
fn field(words: &[i32], at: u32, bits: u32, signed: bool) -> i64 {
    let mut v: u64 = 0;
    for j in 0..bits {
        let b = at + j;
        let w = words.get((b / 32) as usize).copied().unwrap_or(0) as u32;
        v |= u64::from((w >> (b % 32)) & 1) << j;
    }
    if bits >= 32 {
        // The word's own value, as a script's integer holds it.
        return i64::from(v as u32 as i32);
    }
    if signed && bits > 0 && v >= 1 << (bits - 1) {
        v as i64 - (1i64 << bits)
    } else {
        v as i64
    }
}

fn put_field(words: &mut [i32], at: u32, bits: u32, x: i64) {
    for j in 0..bits {
        let b = at + j;
        let Some(w) = words.get_mut((b / 32) as usize) else {
            return;
        };
        let mask = 1u32 << (b % 32);
        let on = (x >> j) & 1 == 1;
        let mut u = *w as u32;
        if on {
            u |= mask;
        } else {
            u &= !mask;
        }
        *w = u as i32;
    }
}

/// The save system's words as a run left them: what the original's memory
/// carries from one level into the next.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Live {
    pub layout: String,
    pub slot: Vec<i32>,
    pub header: Vec<i32>,
}

/// The words of `file`'s set on `vm`, or `None` before the run's init made
/// them.
pub fn words_of(vm: &Vm, file: File) -> Option<Vec<i32>> {
    let set = file_set(vm, file)?;
    vm.set(set)?
        .iter()
        .map(|w| int_of(vm, *w).map(|i| i as i32))
        .collect()
}

/// The set holding `file`'s words: `console save params`' `save val set` or
/// `header save val set`.
fn file_set(vm: &Vm, file: File) -> Option<Value> {
    let csp = global(vm, "console save params")?;
    let name = match file {
        File::Slot => "save val set",
        File::Header => "header save val set",
    };
    vm.field(csp, name).map(|v| vm.value(v))
}

/// `words` written into `file`'s set; false when the set isn't there or
/// holds another number of words.
fn put_words(vm: &mut Vm, file: File, words: &[i32]) -> bool {
    let Some(set) = file_set(vm, file) else {
        return false;
    };
    let Some(elems) = vm.set(set).map(<[_]>::to_vec) else {
        return false;
    };
    if elems.len() != words.len() {
        return false;
    }
    elems
        .iter()
        .zip(words)
        .all(|(e, &w)| vm.put(*e, Value::Int(w)))
}

/// Where a load of a save takes a profile (the module docs): chapter 1, a
/// level by archive name, or the hub's stage.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "to", content = "level", rename_all = "snake_case")]
pub enum Resume {
    /// No save, or chapter 1's adventure not done: chapter 1, as today.
    ChapterOne,
    /// Level 214, chapter 1 done and it not.
    Level(String),
    /// Both done: the hub's stage (`level_hub_2_stage_<n>`).
    Hub(String),
}

/// Chapter 1's and level 214's adventures, as the title's load names them.
const CHAPTER_ONE: &str = "Level_213_Tutorial";
const LEVEL_214: &str = "Level_214_Tutorial_2";
/// Level 214's archive, spelled as the disc spells it.
pub const LEVEL_214_ARCHIVE: &str = "level_214_tutotial_2";
/// The adventures whose being done opens the hub's next stage
/// (`village unlocks`), in order.
const HUB_UNLOCKS: [&str; 3] = [
    "Level_210_Wilikins",
    "Level_202_Arkeyan_Copters",
    "Level_211_Final",
];

/// Whether `values` hold `adventure` done: its `level flags` field's first
/// bit.
pub fn done(values: &SaveValues, adventure: &str) -> bool {
    let Some(i) = values.adventures.iter().position(|a| a == adventure) else {
        return false;
    };
    values
        .params
        .get("level flags")
        .and_then(|f| f.get(i))
        .is_some_and(|f| f & 1 == 1)
}

/// Where the original's load of `values` takes a profile (the module docs):
/// the title's rule over its adventures done.
pub fn resume(values: Option<&SaveValues>) -> Resume {
    let Some(v) = values else {
        return Resume::ChapterOne;
    };
    if !done(v, CHAPTER_ONE) {
        Resume::ChapterOne
    } else if !done(v, LEVEL_214) {
        Resume::Level(LEVEL_214_ARCHIVE.to_owned())
    } else {
        let stage = 1 + HUB_UNLOCKS.iter().filter(|a| done(v, a)).count();
        Resume::Hub(format!("level_hub_2_stage_{stage}"))
    }
}

/// What the session's save system did, for `/api/_test/tv`'s `story`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct Report {
    /// The host profile, none with no profile unlocked at the launch.
    pub host: Option<String>,
    /// The run's layout, once its start was made.
    pub layout: Option<String>,
    /// Whether the host had a save of the run's layout to load.
    pub loaded: bool,
    /// What the run's start loaded from: `carried` (the last run's words),
    /// `stored` (the host's save) or `fresh`.
    pub start: Option<String>,
    /// The requests the scripts made, in order, and how each ended.
    pub requests: Vec<(Asked, i32)>,
    /// Saves the store has written this session.
    pub written: u32,
    /// The last error, as a log line has it.
    pub error: Option<String>,
    /// The saves of other layouts kept and ignored, by layout.
    pub ignored: Vec<String>,
    /// Where the host's save resumed the game at the launch.
    pub resume: Option<Resume>,
}

/// The host's save as the session holds it, across the TV's runs.
#[derive(Debug, Clone, Default)]
pub struct Session {
    /// The host profile; none keeps every save for the session alone.
    pub host: Option<String>,
    /// The host's save of the run's layout as the store last had it: what
    /// a load reads.
    pub stored: Option<(String, SaveValues)>,
    /// Every row of the host's for the game as the store had them at the
    /// launch, until a run's layout picks one ([`guac_server::saves::choose`]).
    pub rows: Option<Vec<guac_server::saves::StorySave>>,
    /// The words the last run left, carried into the next.
    pub live: Option<Live>,
    /// What a save makes of the session's values: the stored ones and each
    /// file saved since.
    saving: Option<SaveValues>,
    /// A write for the embedder to hand the store, under its layout.
    outbox: Option<(String, SaveValues)>,
    /// A write the embedder handed the store, not yet done.
    writing: Option<(String, SaveValues)>,
    /// The run whose start was made, by [`ScriptWorld::run`].
    started: Option<u64>,
    /// The layout of the run whose start was made.
    layout: Option<Layout>,
    pub report: Report,
}

/// The system monitor's save macros, by name: what the save scripts add to
/// its list and [`tick`] runs.
pub const MONITOR_MACROS: [&str; 2] = [
    "Save - Run Savegame Wrapper",
    "Save - Manage Autosave Icon Remove",
];

/// The placement whose name the save macros run in: the first after the
/// monitor in the original's list that ticks every step on the TV, so its
/// clock runs as the monitor's would.
const MONITOR_STAND_IN: &str = "Macros_Animation.ai";

/// The system monitor's save macros, each once in its list's order while it
/// holds them (the module docs), ahead of a step's walk: `Save - Run
/// Savegame` while requests wait and the autosave icon's removal after.
/// Nothing while the list holds neither, as on every run whose save system
/// was never activated. A stop is returned once, and ends them for the run.
pub fn tick(vm: &mut Vm, host: &mut WorldHost<'_, '_>) -> Result<(), Halt> {
    if host.world.save.monitor_stopped.is_some() {
        return Ok(());
    }
    let Some(list) = global(vm, "System Monitor Controls")
        .and_then(|m| vm.field(m, "system monitor macros"))
        .and_then(|s| vm.set(s).map(<[_]>::to_vec))
    else {
        return Ok(());
    };
    let names: Vec<String> = list
        .iter()
        .filter_map(|v| match vm.value(*v) {
            Value::Object(o) => vm.program().name_of(o).map(str::to_owned),
            _ => None,
        })
        .filter(|n| MONITOR_MACROS.contains(&n.as_str()))
        .collect();
    if names.is_empty() {
        return Ok(());
    }
    let Some(stand_in) = crate::scripts::running(vm, host.world, MONITOR_STAND_IN) else {
        return Ok(());
    };
    for name in names {
        if let Err(h) = vm.call_for(stand_in, &name, &[], &[], host) {
            host.world.save.monitor_stopped = Some(h.to_string());
            return Err(h);
        }
    }
    Ok(())
}

impl Session {
    /// A session for `host` (none keeps every save for the session alone),
    /// nothing fetched or carried yet.
    pub fn new(host: Option<String>) -> Self {
        Session {
            host,
            ..Session::default()
        }
    }

    /// A write the store should make, taken: the embedder hands it over and
    /// says how it went with [`Session::written`].
    pub fn take_write(&mut self) -> Option<(String, String, SaveValues)> {
        let host = self.host.clone()?;
        let (layout, values) = self.outbox.take()?;
        self.writing = Some((layout.clone(), values.clone()));
        Some((host, layout, values))
    }

    /// Whether a write is waiting to be handed over or done.
    pub fn writing(&self) -> bool {
        self.outbox.is_some() || self.writing.is_some()
    }

    /// The store's answer to the write handed over: done, the session's
    /// stored save is what it wrote and a save waiting on it reads done;
    /// refused, the save reads a write error.
    pub fn written(&mut self, save: &mut SaveState, result: Result<(), String>) {
        let Some((layout, values)) = self.writing.take() else {
            return;
        };
        let label = match result {
            Ok(()) => {
                self.stored = Some((layout, values));
                self.report.written += 1;
                label::INACTIVE
            }
            Err(e) => {
                warn!("the host's story save wasn't written: {e}");
                self.report.error = Some(e);
                label::WRITE_ERROR
            }
        };
        if save.waiting() == Some(Asked::Save) {
            self.finish(save, Asked::Save, label);
        }
    }

    fn finish(&mut self, save: &mut SaveState, asked: Asked, label: i32) {
        save.done(label);
        self.report.requests.push((asked, label));
        if self.report.requests.len() > 32 {
            self.report.requests.remove(0);
        }
    }

    /// The host's rows as the store had them at the launch: where the
    /// game resumes (the newest row of this build's [`FORMAT`]; which row a
    /// run loads is settled at its start, by its layout).
    pub fn fetched(&mut self, rows: Vec<guac_server::saves::StorySave>) -> Resume {
        let prefix = format!("{FORMAT}:");
        let newest = rows
            .iter()
            .filter(|r| r.layout.starts_with(&prefix))
            .filter_map(|r| r.values.as_ref().map(|v| (r.updated_at.as_str(), v)))
            .max_by(|a, b| a.0.cmp(b.0))
            .map(|(_, v)| v.clone());
        let resume = resume(newest.as_ref());
        self.rows = Some(rows);
        self.report.resume = Some(resume.clone());
        resume
    }
}

/// The parameters `Save - Get/Adjust Level Saved Flags` reads, each -1 to
/// read rather than set.
const LEVEL_FLAGS: [&str; 11] = [
    "chest 1",
    "chest 2",
    "chest 3",
    "chest 4",
    "story tablet hint objective",
    "achieved all areas found",
    "achieved target time",
    "achieved no lives lost",
    "achieved enemies killed count",
    "achieved all collection objectives",
    "completed",
];

/// The adventures' data, in order.
fn adventures(vm: &Vm) -> Vec<Value> {
    global(vm, "level progression data")
        .and_then(|l| vm.field(l, "adventure data"))
        .and_then(|s| vm.set(s).map(<[_]>::to_vec))
        .unwrap_or_default()
}

/// The adventure whose level loads name `level` (an archive): a load's
/// text as the run's interface reads it, else its own name read as the
/// archive's first words (`Level_214` names `level_214_tutotial_2`).
fn adventure_of(vm: &Vm, world: &ScriptWorld, level: &str) -> Option<Value> {
    adventures(vm).into_iter().find_map(|ad| {
        let loads = vm.field(ad, "level loads")?;
        let names = vm.set(loads)?.iter().any(|l| {
            let l = vm.value(*l);
            if let Some(t) = world
                .text_of(l)
                .and_then(|t| crate::scripts::level_name(&t))
                && !t.is_empty()
            {
                return t == level;
            }
            match l {
                Value::Object(o) => vm.program().name_of(o).is_some_and(|n| {
                    let n = n.to_ascii_lowercase();
                    level == n || level.starts_with(&format!("{n}_"))
                }),
                _ => false,
            }
        });
        names.then(|| vm.field(ad, "title")).flatten()
    })
}

/// A run's start (the module docs): once its init has defined the save, the
/// title's part (the system activated; the save loaded from the last run's
/// words, else the host's stored save, else marked in use as a new game;
/// each adventure's `complete` filled from it), the Pause Controller's
/// switch's (the adventure `level` is named the current one, its slot level
/// written), then the level initializer's save. Nothing until the init has
/// run; once a run.
pub fn start_run(
    vm: &mut Vm,
    host: &mut WorldHost<'_, '_>,
    session: &mut Session,
    level: &str,
) -> Result<(), Halt> {
    let run = host.world.run();
    if session.started == Some(run) {
        return Ok(());
    }
    let Some(layout) = Layout::read(vm) else {
        return Ok(());
    };
    session.started = Some(run);
    let id = layout.id();
    session.report.host = session.host.clone();
    session.report.layout = Some(id.clone());
    // The host's rows from the launch, picked by the run's layout once.
    if let Some(rows) = session.rows.take() {
        let loaded = guac_server::saves::choose(rows, &id);
        session.report.ignored = loaded.ignored.iter().map(|r| r.layout.clone()).collect();
        session.stored = loaded
            .save
            .and_then(|s| s.values.map(|v| (s.layout.clone(), v)));
    }
    let stored = session
        .stored
        .as_ref()
        .filter(|(l, _)| *l == id)
        .map(|(_, v)| v.clone());
    session.report.loaded = stored.is_some();
    session.saving = stored.clone();

    // The title's part: activated, as its first load leaves the system.
    if let Some(control) = global(vm, "savegame control data") {
        vm.set_field(control, "activated", Value::Int(1));
    }
    let live = session.live.take().filter(|l| l.layout == id);
    let start = if let Some(l) = &live {
        if !(put_words(vm, File::Slot, &l.slot) && put_words(vm, File::Header, &l.header)) {
            warn!(
                "the last run's save words don't fit this run's sets: the run starts as its init left it"
            );
        }
        "carried"
    } else if let Some(v) = &stored {
        for file in [File::Slot, File::Header] {
            if let Some(mut words) = words_of(vm, file) {
                layout.encode(file, &v.params, &mut words);
                put_words(vm, file, &words);
            }
        }
        "stored"
    } else {
        // The title's new game: the slot marked in use.
        vm.call_with(
            None,
            "Save - Get/Set Player Flag",
            &[
                ("setting id", Value::Int(0)),
                ("value", Value::Int(1)),
                ("do save", Value::Int(0)),
            ],
            &[],
            host,
        )?;
        "fresh"
    };
    session.report.start = Some(start.to_owned());
    // The title's fill: each adventure's `complete` off its flags.
    for ad in adventures(vm) {
        let mut args: Vec<(&str, Value)> = vec![("input adventure data", ad)];
        args.extend(LEVEL_FLAGS.iter().map(|f| (*f, Value::Int(-1))));
        let (_, back) = vm.call_with(
            None,
            "Save - Get/Adjust Level Saved Flags",
            &args,
            &["completed"],
            host,
        )?;
        let completed = back
            .first()
            .copied()
            .flatten()
            .and_then(|v| int_of(vm, v))
            .unwrap_or(0);
        vm.set_field(ad, "complete", Value::Int(completed as i32));
        vm.set_field(ad, "unlocked", Value::Int(0));
    }
    for name in [
        "Save - Accolade Set XP Multiplier",
        "Save - UpgradeOTron Set Multipliers",
    ] {
        if vm.program().macro_named(name).is_some() {
            vm.call(None, name, host)?;
        }
    }
    // The switch's part: the level's adventure the current one, its slot
    // level written.
    if let Some(title) = adventure_of(vm, host.world, level)
        && let Some(g) = vm.program().global_named("Current Adventure Title")
    {
        vm.set_global_reference(g, title);
        vm.call_with(
            None,
            "Save - Set Savegame Level",
            &[("adventure title", title)],
            &[],
            host,
        )?;
    }
    // The level initializer's save: slot and header, the percent complete
    // worked out first.
    let slot = global(vm, "Current_SaveGame_Slot").unwrap_or(Value::Int(0));
    vm.call_with(
        None,
        "Save - Save Game",
        &[
            ("slot", slot),
            ("do load", Value::Int(0)),
            ("do global data", Value::Int(1)),
            ("create if not found", Value::Int(0)),
            ("calculate percent complete", Value::Int(1)),
            ("is autosave", Value::Int(1)),
            ("save when inactive", Value::Int(0)),
        ],
        &[],
        host,
    )?;
    info!(
        layout = %id,
        host = ?session.host,
        start,
        "the save system's start: loaded, and the level's save asked"
    );
    session.layout = Some(layout);
    Ok(())
}

/// A step's requests answered (the module docs), after its walk: the
/// run's start first ([`start_run`]), then a request a script made, taken.
/// A save waits on [`Session::written`]; a load or a finalize with nothing
/// written still to come reads done at once. Then the words the run holds
/// now, kept to carry into the next run ([`Session::live`]).
pub fn serve(
    vm: &mut Vm,
    host: &mut WorldHost<'_, '_>,
    session: &mut Session,
    level: &str,
) -> Result<(), Halt> {
    start_run(vm, host, session, level)?;
    let answered = answer(vm, host, session);
    if session.started == Some(host.world.run())
        && let Some(layout) = &session.layout
        && let (Some(slot), Some(header)) = (words_of(vm, File::Slot), words_of(vm, File::Header))
    {
        session.live = Some(Live {
            layout: layout.id(),
            slot,
            header,
        });
    }
    answered
}

/// The request a script made this step, taken and answered ([`serve`]).
fn answer(vm: &mut Vm, host: &mut WorldHost<'_, '_>, session: &mut Session) -> Result<(), Halt> {
    // A finalize waits for the write before it.
    let state = &mut host.world.save;
    if state.waiting() == Some(Asked::Finalize) && !session.writing() {
        session.finish(state, Asked::Finalize, label::INACTIVE);
    }
    let Some(asked) = host.world.save.take() else {
        return Ok(());
    };
    let Some(layout) = session.layout.clone() else {
        session.report.error = Some("no save layout on the run".into());
        session.finish(&mut host.world.save, asked, label::GENERAL_ERROR);
        return Ok(());
    };
    match asked {
        Asked::Finalize => {
            if !session.writing() {
                session.finish(&mut host.world.save, asked, label::INACTIVE);
            }
        }
        Asked::Load | Asked::Save => {
            let Some(file) = asked_file(vm) else {
                session.report.error = Some("a request naming no save file".into());
                session.finish(&mut host.world.save, asked, label::GENERAL_ERROR);
                return Ok(());
            };
            if asked == Asked::Load {
                let label = load(vm, &layout, session, file);
                session.finish(&mut host.world.save, asked, label);
            } else {
                save(vm, &layout, session, &mut host.world.save, file);
            }
        }
    }
    Ok(())
}

/// The file `system.save data` names.
fn asked_file(vm: &Vm) -> Option<File> {
    let data = vm.value_cell(None, MemberId::named("system", "save data"))?;
    let data = vm.value(data);
    [File::Slot, File::Header]
        .into_iter()
        .find(|f| file_set(vm, *f) == Some(data))
}

fn load(vm: &mut Vm, layout: &Layout, session: &Session, file: File) -> i32 {
    let id = layout.id();
    let Some((_, values)) = session.stored.as_ref().filter(|(l, _)| *l == id) else {
        return label::INVALID_PATH;
    };
    let Some(mut words) = words_of(vm, file) else {
        return label::READ_ERROR;
    };
    layout.encode(file, &values.params, &mut words);
    match put_words(vm, file, &words) {
        true => label::INACTIVE,
        false => label::READ_ERROR,
    }
}

fn save(vm: &Vm, layout: &Layout, session: &mut Session, state: &mut SaveState, file: File) {
    let Some(words) = words_of(vm, file).filter(|w| w.len() == layout.words(file)) else {
        session.report.error = Some(format!("the {file:?} file's words don't fit the layout"));
        session.finish(state, Asked::Save, label::WRITE_ERROR);
        return;
    };
    session.saved(
        state,
        layout.id(),
        layout.decode(file, &words),
        &layout.adventures,
    );
}

impl Session {
    /// A save of one file, its params read off its words: kept with every
    /// value the session holds and handed to the store as the host's save,
    /// the request read active until [`Session::written`]; with no host,
    /// kept for the session alone and done at once.
    pub fn saved(
        &mut self,
        state: &mut SaveState,
        layout: String,
        params: BTreeMap<String, Vec<i64>>,
        adventures: &[String],
    ) {
        let mut values = self.saving.take().unwrap_or_default();
        values.params.extend(params);
        values.adventures = adventures.to_vec();
        self.saving = Some(values.clone());
        if self.host.is_none() {
            self.stored = Some((layout, values));
            self.finish(state, Asked::Save, label::INACTIVE);
            return;
        }
        self.outbox = Some((layout, values));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn layout() -> Layout {
        Layout {
            params: vec![
                Param {
                    name: "version".into(),
                    header: false,
                    bit: 0,
                    bits: 8,
                    fields: 1,
                    signed: false,
                },
                Param {
                    name: "level flags".into(),
                    header: false,
                    bit: 8,
                    bits: 12,
                    fields: 3,
                    signed: false,
                },
                Param {
                    name: "pct".into(),
                    header: false,
                    bit: 44,
                    bits: 8,
                    fields: 2,
                    signed: true,
                },
                Param {
                    name: "time".into(),
                    header: false,
                    bit: 60,
                    bits: 32,
                    fields: 1,
                    signed: false,
                },
                Param {
                    name: "header - slot level ids".into(),
                    header: true,
                    bit: 0,
                    bits: 8,
                    fields: 4,
                    signed: false,
                },
            ],
            adventures: vec![
                "Level_213_Tutorial".into(),
                "Level_214_Tutorial_2".into(),
                "Level_201_Rescue_the_Hermit".into(),
            ],
            slot_words: 3,
            header_words: 1,
        }
    }

    /// Fields pack low bit first, one crossing a word running on into the
    /// next; a short signed field reads two's complement, a 32-bit one the
    /// word's own value; each reads back what was written.
    #[test]
    fn a_files_params_pack_low_bit_first_and_read_back() {
        let l = layout();
        let values = BTreeMap::from([
            ("version".to_owned(), vec![25]),
            ("level flags".to_owned(), vec![1, 0xABC, 0x801]),
            ("pct".to_owned(), vec![-1, 100]),
            ("time".to_owned(), vec![-123_456]),
        ]);
        let mut words = vec![0; 3];
        l.encode(File::Slot, &values, &mut words);
        // version in byte 0, the first flags field from bit 8.
        assert_eq!(words[0] as u32 & 0xFF, 25);
        assert_eq!((words[0] as u32 >> 8) & 0xFFF, 1);
        // The third flags field (bits 32-43) sits in word 1 alone; the
        // second (bits 20-31) in word 0's top.
        assert_eq!((words[0] as u32 >> 20) & 0xFFF, 0xABC);
        assert_eq!(words[1] as u32 & 0xFFF, 0x801);
        // The time crosses words 1 and 2.
        let time = (u64::from(words[1] as u32) >> 28) | (u64::from(words[2] as u32) << 4);
        assert_eq!(time as u32 as i32, -123_456);
        assert_eq!(l.decode(File::Slot, &words), values);
        // The header's params apart.
        let header = BTreeMap::from([("header - slot level ids".to_owned(), vec![2, 0, 7, 255])]);
        let mut hw = vec![0];
        l.encode(File::Header, &header, &mut hw);
        assert_eq!(hw[0] as u32, 2 | (7 << 16) | (255 << 24));
        assert_eq!(l.decode(File::Header, &hw), header);
        assert!(
            !l.decode(File::Slot, &words)
                .contains_key("header - slot level ids")
        );
    }

    /// A value past its field's bits keeps its low bits; a param the values
    /// lack leaves its words as they were.
    #[test]
    fn encoding_keeps_a_fields_low_bits_and_leaves_the_rest() {
        let l = layout();
        let mut words = vec![-1, -1, -1];
        let values = BTreeMap::from([("version".to_owned(), vec![0x1FF])]);
        l.encode(File::Slot, &values, &mut words);
        assert_eq!(words, [-1, -1, -1], "0x1FF's low byte is 0xFF");
        let values = BTreeMap::from([("version".to_owned(), vec![0])]);
        l.encode(File::Slot, &values, &mut words);
        assert_eq!(words[0] as u32, 0xFFFF_FF00);
        assert_eq!(&words[1..], [-1, -1]);
    }

    /// The layout's name moves with any param's shape, the adventures'
    /// order or a file's length, and not with the params' order.
    #[test]
    fn the_layouts_name_follows_its_shape() {
        let l = layout();
        let id = l.id();
        assert!(id.starts_with(&format!("{FORMAT}:")), "{id}");
        let mut r = l.clone();
        r.params.reverse();
        assert_eq!(r.id(), id);
        let mut b = l.clone();
        b.params[1].bits = 11;
        assert_ne!(b.id(), id);
        let mut a = l.clone();
        a.adventures.swap(0, 1);
        assert_ne!(a.id(), id);
        let mut w = l.clone();
        w.slot_words = 4;
        assert_ne!(w.id(), id);
    }

    fn values(done: &[&str]) -> SaveValues {
        let adventures: Vec<String> = [
            "Level_213_Tutorial",
            "Level_214_Tutorial_2",
            "Level_202_Arkeyan_Copters",
            "Level_210_Wilikins",
            "Level_211_Final",
        ]
        .map(String::from)
        .into();
        let flags = adventures
            .iter()
            .map(|a| i64::from(done.contains(&a.as_str())) | 0b110)
            .collect();
        SaveValues {
            params: BTreeMap::from([("level flags".to_owned(), flags)]),
            adventures,
        }
    }

    /// The title's rule: chapter 1 until its adventure is done, then level
    /// 214 until its is, then the hub's stage past the hub's unlocking
    /// adventures done; no save is chapter 1. Only each flag's first bit
    /// counts.
    #[test]
    fn a_load_goes_where_the_titles_does() {
        assert_eq!(resume(None), Resume::ChapterOne);
        assert_eq!(resume(Some(&values(&[]))), Resume::ChapterOne);
        assert_eq!(
            resume(Some(&values(&["Level_214_Tutorial_2"]))),
            Resume::ChapterOne,
            "214 done, chapter 1 not"
        );
        assert_eq!(
            resume(Some(&values(&["Level_213_Tutorial"]))),
            Resume::Level("level_214_tutotial_2".into())
        );
        assert_eq!(
            resume(Some(&values(&[
                "Level_213_Tutorial",
                "Level_214_Tutorial_2"
            ]))),
            Resume::Hub("level_hub_2_stage_1".into())
        );
        assert_eq!(
            resume(Some(&values(&[
                "Level_213_Tutorial",
                "Level_214_Tutorial_2",
                "Level_210_Wilikins",
                "Level_202_Arkeyan_Copters",
            ]))),
            Resume::Hub("level_hub_2_stage_3".into())
        );
        // An adventure the save doesn't list isn't done.
        let mut v = values(&["Level_213_Tutorial"]);
        v.adventures.retain(|a| a != "Level_214_Tutorial_2");
        assert_eq!(
            resume(Some(&v)),
            Resume::Level("level_214_tutotial_2".into())
        );
    }

    fn row(
        layout: &str,
        updated: &str,
        values: Option<SaveValues>,
    ) -> guac_server::saves::StorySave {
        guac_server::saves::StorySave {
            profile_id: "alice".into(),
            game: GAME.into(),
            layout: layout.into(),
            values,
            created_at: "then".into(),
            updated_at: updated.into(),
        }
    }

    /// The rows at the launch resume the game by the newest of this build's
    /// format that reads; another format's, however new, and one that
    /// doesn't read resume nothing; no row is chapter 1. The rows are kept
    /// for the run's start to pick by layout.
    #[test]
    fn the_launch_resumes_by_the_newest_row_of_this_format() {
        let mut s = Session::new(Some("alice".into()));
        let chapter_1_done = values(&["Level_213_Tutorial"]);
        let both_done = values(&["Level_213_Tutorial", "Level_214_Tutorial_2"]);
        let rows = vec![
            row(
                &format!("{FORMAT}:aaaa"),
                "2026-11-02T10:00",
                Some(chapter_1_done),
            ),
            row(
                &format!("{FORMAT}:bbbb"),
                "2026-11-02T09:00",
                Some(both_done.clone()),
            ),
            row("0:old", "2026-11-02T11:00", Some(both_done.clone())),
            row(&format!("{FORMAT}:cccc"), "2026-11-02T12:00", None),
        ];
        assert_eq!(
            s.fetched(rows.clone()),
            Resume::Level(LEVEL_214_ARCHIVE.into())
        );
        assert_eq!(s.rows.as_deref(), Some(rows.as_slice()));
        assert_eq!(
            s.report.resume,
            Some(Resume::Level(LEVEL_214_ARCHIVE.into()))
        );
        let mut fresh = Session::new(Some("alice".into()));
        assert_eq!(fresh.fetched(Vec::new()), Resume::ChapterOne);
        assert_eq!(
            fresh.fetched(vec![row("0:old", "2026-11-02T11:00", Some(both_done))]),
            Resume::ChapterOne
        );
    }

    /// The host's save cycle: a script's save asked reads active while its
    /// file's values wait for the store and while the store has them, then
    /// done once the store has written them, the stored save what it wrote
    /// (the slot's values and the header's together); a refused write reads
    /// a write error and keeps the stored save; no host, done at once.
    #[test]
    fn a_save_asked_reads_complete_once_the_store_has_written() {
        let mut state = SaveState::default();
        let mut session = Session::new(Some("alice".into()));
        let advs = ["Level_213_Tutorial".to_owned()];
        let slot = BTreeMap::from([("level flags".to_owned(), vec![1])]);
        let header = BTreeMap::from([("header - slot level ids".to_owned(), vec![2])]);

        state.set(label::SAVE);
        assert_eq!(state.take(), Some(Asked::Save));
        session.saved(&mut state, "1:abc".into(), slot.clone(), &advs);
        assert_eq!(state.get(), label::ACTIVE, "done before the store had it");
        assert!(session.writing());
        let (host, layout, values) = session.take_write().unwrap();
        assert_eq!((host.as_str(), layout.as_str()), ("alice", "1:abc"));
        assert_eq!(values.params, slot);
        assert_eq!(state.get(), label::ACTIVE, "done while the store had it");
        session.written(&mut state, Ok(()));
        assert_eq!(state.get(), label::INACTIVE);
        assert_eq!(session.stored, Some(("1:abc".into(), values)));
        assert!(!session.writing());
        assert_eq!(session.report.requests, [(Asked::Save, label::INACTIVE)]);

        // The header next: the save written holds both files' values.
        state.set(label::SAVE);
        state.take();
        session.saved(&mut state, "1:abc".into(), header.clone(), &advs);
        let (_, _, both) = session.take_write().unwrap();
        assert_eq!(both.params.len(), 2);
        assert_eq!(both.params["level flags"], [1]);
        // Refused: a write error, the stored save the slot's alone.
        session.written(&mut state, Err("disk full".into()));
        assert_eq!(state.get(), label::WRITE_ERROR);
        assert_eq!(session.stored.as_ref().unwrap().1.params, slot);
        assert_eq!(session.report.written, 1);

        // No host: done at once, nothing for the store.
        let mut lone = Session::new(None);
        let mut state = SaveState::default();
        state.set(label::SAVE);
        state.take();
        lone.saved(&mut state, "1:abc".into(), slot.clone(), &advs);
        assert_eq!(state.get(), label::INACTIVE);
        assert!(lone.take_write().is_none());
        assert_eq!(lone.stored.unwrap().1.params, slot);
    }

    /// A save asked reads active until the store has written it, then
    /// inactive, the request taken once; a request written while one waits
    /// changes nothing, and a write of any other value is kept as written.
    #[test]
    fn the_states_requests_are_taken_once_and_other_writes_kept() {
        let mut s = SaveState::default();
        assert_eq!(s.get(), label::INACTIVE);
        s.set(label::SAVE);
        assert_eq!(s.get(), label::ACTIVE);
        assert_eq!(s.take(), Some(Asked::Save));
        assert_eq!(s.take(), None, "taken twice");
        s.set(label::LOAD);
        assert_eq!((s.get(), s.waiting()), (label::ACTIVE, Some(Asked::Save)));
        assert_eq!(s.take(), None, "a load asked over a save waiting");
        s.done(label::INACTIVE);
        assert_eq!((s.get(), s.waiting()), (label::INACTIVE, None));
        s.set(label::LOAD);
        assert_eq!(s.take(), Some(Asked::Load));
        s.done(label::INVALID_PATH);
        assert_eq!(s.get(), label::INVALID_PATH);
        // The scripts clear an error with 0; the 360's login writes stay.
        s.set(0);
        assert_eq!(s.get(), label::INACTIVE);
        s.set(23);
        assert_eq!((s.get(), s.take()), (23, None));
    }
}
