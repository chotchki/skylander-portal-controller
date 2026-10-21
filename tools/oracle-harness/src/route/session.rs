//! `oracle goto`'s live half (openspec AH.7): a fresh RPCS3 boots a gameplay
//! state to Start Paused's pause (F0), gets P5's `RECONNECT` and a fresh copy
//! of the figure, F0 is read (the actors and the generator), O5's watch (and
//! O6's chains) are armed if asked, and the route is driven from there
//! ([`super::drive`]), each pause read through PINE and each order loaded as
//! O4's `PAD` lines with a `PAUSE_AT` after. At the end the watch is read, PINE
//! closed, and, when the plan asks and the drive got there, the state saved.
//!
//! Everything lands in one new dir outside every work tree: `record.json`
//! (the identity line, the hosts' loads and `uptime` before and after, the
//! hashes, the outcome, the shots, the reads, the watch, the end state),
//! `route.json` (every pause and what was done there), `input.ron` (what port
//! 0 held from F0 + 1, AF.7's format), each read's actors, generator and fight
//! state, each shot, the ring and the end state, and RPCS3's log.

use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail};
use guac_oracle::input::InputScript;
use serde::{Deserialize, Serialize};
use skylander_core::SlotIndex;
use skylander_rpcs3_control::PortalDriver;

use super::census::{self, Census, Disc, RouteRecord, Wake};
use super::{FigureField, Knobs, Outcome, RoutePlan, Session, Step, Trace, drive};
use crate::actors::{self, ActorFrame};
use crate::chains::{self, GiantsChains};
use crate::control::Control;
use crate::divergence::{
    self, EndState, FATAL, Host, SILENT, STEP_WAIT, WatchRun, file_name, log_lines, read_watch,
    save_retrying, saved_in, sha256_file,
};
use crate::fight::{self, FightWatch, Rows};
use crate::home::OracleHome;
use crate::identity::Identity;
use crate::launch::{self, BootOptions};
use crate::outputs::{Kind, Rpcs3Outputs};
use crate::pine::PineClient;
use crate::rng::{self, Peek};
use crate::watch::{GiantsWatch, WatchFile};

/// Everything a drive needs: the oracle build and its home, the state it
/// boots, the figure, the rows a read decodes the figure with, and the
/// recorders armed at F0.
pub struct Inputs<'a> {
    pub exe: &'a Path,
    pub home: &'a OracleHome,
    /// The gameplay savestate it boots.
    pub state: &'a Path,
    /// The `.sky` a fresh copy of goes on the portal at F0.
    pub figure: &'a Path,
    pub boot_wait: Duration,
    /// AH.2's rows off the dump: with them a read takes the fight's state
    /// (AH.4) too.
    pub rows: Option<&'a Rows>,
    /// Level 213's placements off the dump (AH.8): with them and the rows a
    /// read takes a census too ([`census`]), and a watch carries the route's
    /// placements' wake state per frame ([`census::WATCHED`]).
    pub disc: Option<&'a Disc>,
    /// O5's watch on Giants' list armed at F0, and O6's chains with it.
    pub watch: bool,
    pub chains: bool,
    /// The ring's records.
    pub ring: u32,
    /// `sgenrand(seed)` written into the game's generator at F0, before
    /// anything is read or runs (AH.10's reseeded replays, `rng seed`'s
    /// write); none leaves the state's own.
    pub seed: Option<u32>,
    /// Script variables set at F0, after the seed and before F0's reads
    /// (AH.10's stagings).
    pub pokes: &'a [Poke],
}

/// A placement's script variable set at F0 before anything runs:
/// `<placement>:<path, dotted>=<value>`, the placement one the actor list
/// holds, the cell a number (an integer cell takes the value truncated).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Poke {
    pub placement: String,
    pub path: Vec<String>,
    pub value: f64,
}

impl Poke {
    pub fn parse(text: &str) -> Result<Self> {
        let (placement, rest) = text
            .split_once(':')
            .with_context(|| format!("{text:?}: a poke is <placement>:<path>=<value>"))?;
        let (path, value) = rest
            .rsplit_once('=')
            .with_context(|| format!("{text:?}: a poke is <placement>:<path>=<value>"))?;
        let path: Vec<String> = path.split('.').map(str::to_owned).collect();
        if placement.is_empty() || path.iter().any(String::is_empty) {
            bail!("{text:?}: an empty placement or step");
        }
        let value: f64 = value
            .parse()
            .with_context(|| format!("{text:?}: {value:?} isn't a number"))?;
        if !value.is_finite() {
            bail!("{text:?}: {value} isn't finite");
        }
        Ok(Self {
            placement: placement.to_owned(),
            path,
            value,
        })
    }
}

/// A poke as it went: the cell and what it held before and after.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PokeDone {
    pub poke: Poke,
    pub cell: u32,
    pub before: crate::script::Number,
    pub after: crate::script::Number,
}

/// The bytes a number cell's value word takes for `value`, by the cell's
/// kind.
pub fn poked_word(was: crate::script::Number, value: f64) -> (crate::script::Number, [u8; 4]) {
    match was {
        crate::script::Number::Float(_) => {
            let v = value as f32;
            (crate::script::Number::Float(v), v.to_bits().to_be_bytes())
        }
        crate::script::Number::Int(_) => {
            let v = value.trunc() as i32;
            (crate::script::Number::Int(v), v.to_be_bytes())
        }
    }
}

/// `poke` written through PINE at the pause the run is in, read back.
fn poke(pine: &mut PineClient, frame: &ActorFrame, poke: &Poke) -> Result<PokeDone> {
    let actor = frame
        .actors
        .iter()
        .find(|a| a.name.as_deref() == Some(poke.placement.as_str()))
        .with_context(|| format!("no `{}` on the actor list", poke.placement))?;
    let cells = crate::script::variables(pine, actor.placement)?
        .with_context(|| format!("`{}` never started its script", poke.placement))?;
    let path: Vec<&str> = poke.path.iter().map(String::as_str).collect();
    let mut types = crate::script::Types::default();
    let (cell, before) =
        crate::script::number_at(pine, &mut types, &cells, &path, &poke.placement)?;
    let (after, bytes) = poked_word(before, poke.value);
    pine.write_paused(cell.at + 0x20, &bytes)
        .with_context(|| format!("writing {}:{}", poke.placement, poke.path.join(".")))?;
    let read = crate::script::number(pine, &mut types, cell.at, "the poked cell")?;
    if read != after {
        bail!(
            "{}:{} reads {read:?} after writing {after:?}",
            poke.placement,
            poke.path.join(".")
        );
    }
    Ok(PokeDone {
        poke: poke.clone(),
        cell: cell.at,
        before,
        after,
    })
}

/// A figure value set at a pause (AH.11), through AH.2's rows: the hero's
/// figure before and after, and his health and next level's threshold.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FigurePoke {
    pub field: FigureField,
    pub value: u32,
    pub hero: Option<String>,
    pub before: FigureSeen,
    pub after: FigureSeen,
}

/// The hero's figure and the cells beside it, as a figure poke saw them.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FigureSeen {
    pub experience: u32,
    pub gold: u32,
    pub playtime_secs: u32,
    pub level: u32,
    pub words: Vec<u32>,
    pub exp_target: crate::script::Number,
    pub health: crate::script::Number,
    pub health_max: crate::script::Number,
}

impl FigureSeen {
    fn of(h: &fight::Hero) -> Self {
        Self {
            experience: h.figure.experience,
            gold: h.figure.gold,
            playtime_secs: h.figure.playtime_secs,
            level: h.figure.level,
            words: h.figure.words.clone(),
            exp_target: h.exp_target,
            health: h.health,
            health_max: h.health_max,
        }
    }
}

/// `field` of the hero's figure set to `value` at the pause the run is in,
/// each word it changes written through PINE, read back.
fn figure_poke(
    pine: &mut PineClient,
    frame: &ActorFrame,
    rows: &Rows,
    field: FigureField,
    value: u32,
) -> Result<FigurePoke> {
    crate::rng::check_title(pine)?;
    let hero = frame.player_one()?;
    let mut types = crate::script::Types::default();
    let before = fight::hero(pine, &mut types, hero.placement, rows)?;
    let mut words = before.figure.words.clone();
    rows.set(&mut words, field, value)?;
    for (i, (&new, &old)) in words.iter().zip(&before.figure.words).enumerate() {
        if new != old {
            pine.write_paused(before.figure.cells[i] + 0x20, &new.to_be_bytes())
                .with_context(|| format!("writing word {i} of the save area"))?;
        }
    }
    let after = fight::hero(pine, &mut types, hero.placement, rows)?;
    if after.figure.words != words {
        bail!(
            "the save area reads {:?} after writing {words:?}",
            after.figure.words
        );
    }
    crate::level::still_paused(pine, "the figure poke")?;
    Ok(FigurePoke {
        field,
        value,
        hero: hero.name.clone(),
        before: FigureSeen::of(&before),
        after: FigureSeen::of(&after),
    })
}

/// One of AH.11's steps as it went: where, what, and what it changed.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ActDone {
    pub flip: u64,
    pub game_frame: u32,
    pub step: String,
    pub detail: serde_json::Value,
}

/// A `SCREENSHOT` by its plan label, with the flip it shows, the file it was
/// copied to and its pixels' hash.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ShotTaken {
    pub label: String,
    pub flip: u64,
    pub file: String,
    pub pixels_sha256: String,
}

/// A read at a pause: where it went and the numbers it's taken for.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ReadTaken {
    pub label: String,
    pub flip: u64,
    pub game_frame: u32,
    pub hero: String,
    /// The generator: draws since its `sgenrand` seeding, if found.
    pub seed: Option<u32>,
    pub drawn: Option<u64>,
    /// The fight's state was read too (`fight-<label>.json`).
    pub fight: bool,
    /// A census was taken too (`census-<label>.json`), in this many ms.
    #[serde(default)]
    pub census_ms: Option<f64>,
    /// Each simulated actor's solid read too (`solids-<label>.json`, AG.7's
    /// reader: whether a break leaves anything solid, AH.9), or why not.
    #[serde(default)]
    pub solids: Option<Result<usize, String>>,
}

/// One drive's record (`record.json`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GotoRecord {
    pub identity: String,
    pub route: String,
    pub state: String,
    pub state_sha256: String,
    pub figure_sha256: String,
    /// The seed written at F0, if any ([`Inputs::seed`]).
    #[serde(default)]
    pub seed: Option<u32>,
    /// The pokes written at F0 ([`Inputs::pokes`]).
    #[serde(default)]
    pub pokes: Vec<PokeDone>,
    /// AH.11's steps carried out at pauses, in order.
    #[serde(default)]
    pub acts: Vec<ActDone>,
    /// Each `.sky` a portal step loaded, with its SHA-256 before the drive and
    /// whether it read the same after.
    #[serde(default)]
    pub sources: Vec<(String, String, bool)>,
    pub before: Host,
    pub after: Host,
    pub boot_secs: f64,
    pub f0: u64,
    pub outcome: Outcome,
    /// Flips from F0 to the last pause, and the pauses.
    pub flips: u64,
    pub looks: usize,
    pub shots: Vec<ShotTaken>,
    pub reads: Vec<ReadTaken>,
    pub watch: Option<WatchRun>,
    /// Why the ring didn't read, when it didn't.
    #[serde(default)]
    pub watch_error: Option<String>,
    pub saved: Option<EndState>,
    pub save_refusals: Vec<String>,
    pub save_error: Option<String>,
    pub silent: bool,
    pub fatal: Vec<String>,
    pub wall_secs: f64,
    pub shutdown: String,
}

/// The oracle as a [`Session`].
pub struct Live<'a> {
    control: Control,
    pine: PineClient,
    dir: PathBuf,
    rows: Option<&'a Rows>,
    disc: Option<&'a Disc>,
    whole: Vec<String>,
    shots: Vec<ShotTaken>,
    reads: Vec<ReadTaken>,
    censuses: Vec<(String, Census)>,
    /// The run's temp dir, where a portal step's copies go.
    tmp: PathBuf,
    acts: Vec<ActDone>,
    /// Each portal step's source and its SHA-256 then.
    sources: Vec<(PathBuf, String)>,
}

impl Live<'_> {
    fn paused(&self) -> Result<u64> {
        let s = self.control.state()?;
        if s.status != "paused" {
            bail!(
                "the run isn't paused: STATE {} frames={}",
                s.status,
                s.frames
            );
        }
        Ok(s.frames)
    }
}

impl Session for Live<'_> {
    fn look(&mut self) -> Result<(u64, ActorFrame)> {
        let flip = self.paused()?;
        let mut frame =
            actors::read_frame(&mut self.pine).with_context(|| format!("the actors at {flip}"))?;
        frame.flip = Some(flip);
        Ok((flip, frame))
    }

    fn run(&mut self, script: &InputScript, flips: u64) -> Result<()> {
        let at = self.paused()?;
        // Appended, not loaded: the paused flip's reads come after the resume,
        // and they're the last run's.
        self.control.append_script_at(script, at + 1)?;
        // 20 fps, a third of the game's, and a minute.
        let wait = Duration::from_secs(60 + flips / 20);
        self.control
            .pause_at(at + flips, wait)
            .with_context(|| format!("pausing {flips} flips on from {at}"))?;
        Ok(())
    }

    fn shot(&mut self, label: &str) -> Result<()> {
        let shot = self.control.screenshot(&self.dir, STEP_WAIT)?;
        let named = self.dir.join(format!("shot-{label}.png"));
        fs::rename(&shot.path, &named)?;
        self.shots.push(ShotTaken {
            label: label.to_owned(),
            flip: shot.flip,
            file: file_name(&named),
            pixels_sha256: shot.frame.pixels_sha256.clone(),
        });
        Ok(())
    }

    fn read(&mut self, label: &str, frame: &ActorFrame) -> Result<()> {
        let flip = self.paused()?;
        frame.write(&self.dir.join(format!("actors-{label}.json")))?;
        let mut peek = Peek::read(&mut self.pine).with_context(|| format!("the RNG at {flip}"))?;
        peek.flip = Some(flip);
        peek.write(&self.dir.join(format!("peek-{label}.json")))?;
        let origin = peek.origin(rng::ORIGIN_TWISTS);
        // No hero (AH.11: one beaten) reads the rest; two take player 1.
        let hero = frame.player_one().ok();
        let census_ms = match (self.rows, self.disc) {
            (Some(_), Some(disc)) => {
                let t = Instant::now();
                let c = census::read_paused(&mut self.pine, frame, disc, &self.whole)
                    .with_context(|| format!("the census at {flip}"))?;
                let path = rrc_dump::output::refuse_repo_output(
                    &self.dir.join(format!("census-{label}.json")),
                )?;
                fs::write(&path, serde_json::to_string_pretty(&c)?)?;
                self.censuses.push((label.to_owned(), c));
                Some(t.elapsed().as_secs_f64() * 1e3)
            }
            _ => None,
        };
        let fight = match (self.rows, hero) {
            (Some(rows), Some(hero)) => {
                match fight::read_paused(&mut self.pine, hero.placement, rows) {
                    Ok(state) => {
                        let path = rrc_dump::output::refuse_repo_output(
                            &self.dir.join(format!("fight-{label}.json")),
                        )?;
                        fs::write(&path, serde_json::to_string_pretty(&state)?)?;
                        true
                    }
                    // A hero leaving or coming in (AH.11) can break the
                    // chain; the rest of the read stands.
                    Err(e) => {
                        let path = rrc_dump::output::refuse_repo_output(
                            &self.dir.join(format!("fight-{label}.error.txt")),
                        )?;
                        fs::write(&path, format!("{e:#}\n"))?;
                        false
                    }
                }
            }
            _ => false,
        };
        // Every player's figure (AH.11: a second figure in beside the hero,
        // its playtime), each as a hero reads, or why not.
        if let Some(rows) = self.rows {
            let mut types = crate::script::Types::default();
            let players: Vec<serde_json::Value> = frame
                .players()
                .map(|a| {
                    let read = fight::hero(&mut self.pine, &mut types, a.placement, rows);
                    serde_json::json!({
                        "name": a.name,
                        "pad": a.pad,
                        "placement": a.placement,
                        "hero": read.as_ref().ok(),
                        "error": read.as_ref().err().map(|e| format!("{e:#}")),
                    })
                })
                .collect();
            let path = rrc_dump::output::refuse_repo_output(
                &self.dir.join(format!("players-{label}.json")),
            )?;
            fs::write(&path, serde_json::to_string_pretty(&players)?)?;
        }
        let solids = match crate::level::read_solids(&mut self.pine, frame) {
            Ok(s) => {
                let path = rrc_dump::output::refuse_repo_output(
                    &self.dir.join(format!("solids-{label}.json")),
                )?;
                fs::write(&path, serde_json::to_string_pretty(&s)?)?;
                Ok(s.len())
            }
            Err(e) => Err(format!("{e:#}")),
        };
        self.reads.push(ReadTaken {
            label: label.to_owned(),
            flip,
            game_frame: frame.game_frame,
            hero: hero.map_or_else(|| "none".to_owned(), |h| h.summary()),
            seed: origin.map(|o| o.seed),
            drawn: origin.map(|o| o.draws),
            fight,
            census_ms,
            solids: Some(solids),
        });
        Ok(())
    }

    fn act(&mut self, step: &Step, frame: &ActorFrame) -> Result<()> {
        let flip = self.paused()?;
        let detail = match step {
            Step::Poke {
                placement,
                path,
                value,
            } => {
                let p = Poke {
                    placement: placement.clone(),
                    path: path.split('.').map(str::to_owned).collect(),
                    value: *value,
                };
                crate::rng::check_title(&mut self.pine)?;
                serde_json::to_value(poke(&mut self.pine, frame, &p)?)?
            }
            Step::Figure { field, value } => {
                let rows = self
                    .rows
                    .context("a figure poke needs AH.2's rows ($GUAC_GIANTS_DUMP)")?;
                serde_json::to_value(figure_poke(&mut self.pine, frame, rows, *field, *value)?)?
            }
            Step::Portal { slot, figure } => {
                let slot_index =
                    SlotIndex::new(*slot).map_err(|e| anyhow::anyhow!("slot {slot}: {e:?}"))?;
                let loaded = match figure {
                    Some(src) => {
                        let src = PathBuf::from(src);
                        let sha = sha256_file(&src)?;
                        let copy = self
                            .tmp
                            .join(format!("figure-{}.sky", self.sources.len() + 1));
                        fs::copy(&src, &copy)
                            .with_context(|| format!("copying {}", src.display()))?;
                        self.control.driver().load(slot_index, &copy)?;
                        self.sources.push((src.clone(), sha.clone()));
                        Some((src.display().to_string(), sha))
                    }
                    None => {
                        self.control.driver().clear(slot_index)?;
                        None
                    }
                };
                let slots = self.control.driver().read_slots()?;
                serde_json::json!({
                    "slot": slot,
                    "loaded": loaded,
                    "slots": slots.iter().map(|s| format!("{s:?}")).collect::<Vec<_>>(),
                })
            }
            Step::Reconnect => {
                self.control.driver().reconnect()?;
                serde_json::json!({ "reconnect": "OK" })
            }
            Step::Vars { placement, label } => {
                crate::rng::check_title(&mut self.pine)?;
                let actor = frame
                    .actors
                    .iter()
                    .find(|a| a.name.as_deref() == Some(placement.as_str()))
                    .with_context(|| format!("no `{placement}` on the actor list"))?;
                let cells = crate::script::variables(&mut self.pine, actor.placement)?
                    .with_context(|| format!("`{placement}` never started its script"))?;
                let mut types = crate::script::Types::default();
                let nodes = crate::script::tree(
                    &mut self.pine,
                    &mut types,
                    &cells,
                    crate::script::TREE_DEPTH,
                );
                crate::level::still_paused(&mut self.pine, "the dump")?;
                let path = rrc_dump::output::refuse_repo_output(
                    &self.dir.join(format!("vars-{label}.json")),
                )?;
                fs::write(&path, serde_json::to_string_pretty(&nodes)?)?;
                serde_json::json!({ "placement": placement, "file": file_name(&path), "cells": cells.len() })
            }
            Step::Globals { names, label } => {
                crate::rng::check_title(&mut self.pine)?;
                let wanted: Vec<&str> = names.iter().map(String::as_str).collect();
                let found = crate::script::exports(&mut self.pine, crate::script::GLOBAL, &wanted)?;
                let cells: Vec<crate::script::Named> = found
                    .iter()
                    .map(|(name, e)| crate::script::Named {
                        at: e.at,
                        vtable: e.vtable,
                        name: name.clone(),
                    })
                    .collect();
                let mut types = crate::script::Types::default();
                let nodes = crate::script::tree(
                    &mut self.pine,
                    &mut types,
                    &cells,
                    crate::script::TREE_DEPTH,
                );
                crate::level::still_paused(&mut self.pine, "the globals' dump")?;
                let path = rrc_dump::output::refuse_repo_output(
                    &self.dir.join(format!("globals-{label}.json")),
                )?;
                fs::write(&path, serde_json::to_string_pretty(&nodes)?)?;
                serde_json::json!({ "names": names, "file": file_name(&path), "found": cells.len() })
            }
            other => bail!("{other:?} isn't one of AH.11's steps"),
        };
        self.acts.push(ActDone {
            flip,
            game_frame: frame.game_frame,
            step: format!("{step:?}"),
            detail,
        });
        Ok(())
    }
}

/// Drives `plan` on a fresh RPCS3 into `out` (new, outside every work tree)
/// and writes its record there. What RPCS3 wrote under the home (shots, the
/// end state) is removed once copied out.
pub fn run(inputs: &Inputs, plan: &RoutePlan, knobs: Knobs, out: &Path) -> Result<GotoRecord> {
    plan.check()?;
    let dir = rrc_dump::output::refuse_repo_output(out)?;
    if dir.exists() {
        bail!("{} exists: a drive's dir is never reused", dir.display());
    }
    fs::create_dir_all(&dir).with_context(|| format!("creating {}", dir.display()))?;
    fs::write(dir.join("route.ron"), plan.to_ron())?;
    let started = Instant::now();
    let before = Host::now();
    let identity = Identity::of(inputs.exe, &inputs.home.config_dir())?.line();
    let (state_sha256, figure_sha256) = (sha256_file(inputs.state)?, sha256_file(inputs.figure)?);
    let outputs = Rpcs3Outputs::new(inputs.home.config_dir());
    let kinds = [Kind::Screenshot, Kind::SaveState];
    let had = kinds.map(|k| outputs.list(k));

    let driven = drive_live(inputs, plan, knobs, &dir, &identity);

    for (kind, had) in kinds.into_iter().zip(&had) {
        for path in outputs.list(kind).difference(had) {
            let _ = fs::remove_file(path);
        }
    }
    let log = inputs.home.cache_dir().join("RPCS3.log");
    let text = fs::read(&log).unwrap_or_default();
    let silent = !log_lines(&text, SILENT).is_empty();
    let fatal = log_lines(&text, FATAL);
    let _ = fs::copy(&log, dir.join("RPCS3.log"));
    let d = driven?;
    let record = GotoRecord {
        identity,
        route: plan.to_ron(),
        state: file_name(inputs.state),
        state_sha256,
        figure_sha256,
        seed: inputs.seed,
        pokes: d.pokes,
        acts: d.acts,
        sources: d.sources,
        before,
        after: Host::now(),
        boot_secs: d.boot_secs,
        f0: d.trace.f0,
        outcome: d.trace.outcome.clone(),
        flips: d.trace.flips,
        looks: d.trace.looks.len(),
        shots: d.shots,
        reads: d.reads,
        watch: d.watch,
        watch_error: d.watch_error,
        saved: d.saved,
        save_refusals: d.save_refusals,
        save_error: d.save_error,
        silent,
        fatal,
        wall_secs: started.elapsed().as_secs_f64(),
        shutdown: d.shutdown,
    };
    let path = rrc_dump::output::refuse_repo_output(&dir.join("record.json"))?;
    fs::write(&path, serde_json::to_string_pretty(&record)?)?;
    Ok(record)
}

struct Driven {
    boot_secs: f64,
    watch_error: Option<String>,
    pokes: Vec<PokeDone>,
    acts: Vec<ActDone>,
    sources: Vec<(String, String, bool)>,
    trace: Trace,
    shots: Vec<ShotTaken>,
    reads: Vec<ReadTaken>,
    watch: Option<WatchRun>,
    saved: Option<EndState>,
    save_refusals: Vec<String>,
    save_error: Option<String>,
    shutdown: String,
}

fn drive_live(
    inputs: &Inputs,
    plan: &RoutePlan,
    knobs: Knobs,
    dir: &Path,
    identity: &str,
) -> Result<Driven> {
    let launched = Instant::now();
    let mut oracle = launch::boot(
        inputs.exe,
        inputs.home,
        &BootOptions {
            savestate: Some(inputs.state.to_path_buf()),
            borderless: true,
            ..BootOptions::default()
        },
    )?;
    let held = oracle.wait_paused(inputs.boot_wait)?;
    let boot_secs = launched.elapsed().as_secs_f64();
    // P4's portal events ride the drive's `STATE` reads (AH.10: the tag
    // writes), into `portal-events.json` at the end.
    let control = oracle.control().with_portal_events();
    control.driver().reconnect()?;
    let copy = oracle.run_paths().tmp.join("figure.sky");
    fs::copy(inputs.figure, &copy)
        .with_context(|| format!("copying {}", inputs.figure.display()))?;
    control
        .driver()
        .load(SlotIndex::new(0).expect("slot 0"), &copy)?;
    let mut pine =
        divergence::pine_connect(&oracle.run_paths().pine_socket, Duration::from_secs(30))?;
    rng::check_title(&mut pine)?;
    if control.state()?.frames != held.frames {
        bail!("the run moved off Start Paused's pause before F0 was read");
    }
    // A reseeded replay's generator, as `oracle rng seed` writes it, before
    // F0's reads (its peek then counts draws from the seed).
    if let Some(seed) = inputs.seed {
        rng::seed(&mut pine, seed).with_context(|| format!("seeding the generator with {seed}"))?;
    }
    let mut pokes = Vec::with_capacity(inputs.pokes.len());
    if !inputs.pokes.is_empty() {
        let frame = actors::read_frame(&mut pine).context("the actors for the pokes")?;
        for p in inputs.pokes {
            pokes.push(poke(&mut pine, &frame, p)?);
        }
    }
    // The route's placements per frame need the fight's cells to ride on
    // (AH.4's watch) and the disc to find them.
    let route_watch = match (inputs.watch, inputs.rows, inputs.disc) {
        (true, Some(rows), Some(disc)) => Some(route_watch(&mut pine, rows, disc)?),
        _ => None,
    };
    let watched = if inputs.watch {
        let mut watch = GiantsWatch::resolve(&mut pine).context("the watch list at F0")?;
        if let Some(fight) = &route_watch {
            watch = watch
                .with_fight(fight.clone())
                .context("the route's placements on the watch list")?;
        }
        let chains = if inputs.chains {
            let chains = GiantsChains::resolve(&mut pine, &watch, &chains::fight_kinds())
                .context("Giants' chains at F0")?;
            control.chains_arm(&chains.chains)?;
            Some(chains)
        } else {
            None
        };
        let armed = chains
            .as_ref()
            .map(|c| c.chains.clone())
            .unwrap_or_default();
        control.watch_arm_chained(&watch.list, &armed, watch.port, inputs.ring)?;
        Some((watch, chains))
    } else {
        None
    };
    let mut live = Live {
        control,
        pine,
        dir: dir.to_path_buf(),
        rows: inputs.rows,
        disc: inputs.disc,
        whole: inputs.disc.map(census::whole_names).unwrap_or_default(),
        shots: Vec::new(),
        reads: Vec::new(),
        censuses: Vec::new(),
        tmp: oracle.run_paths().tmp.clone(),
        acts: Vec::new(),
        sources: Vec::new(),
    };
    // F0 is read before anything runs, as every replay's is.
    let (_, f0_frame) = live.look()?;
    live.read("f0", &f0_frame)?;
    let trace = drive(&mut live, plan, knobs)?;
    let path = rrc_dump::output::refuse_repo_output(&dir.join("route.json"))?;
    fs::write(&path, serde_json::to_string_pretty(&trace)?)?;
    fs::write(dir.join("input.ron"), &trace.input_ron)?;
    let Live {
        control,
        mut pine,
        shots,
        reads,
        censuses,
        acts,
        sources,
        ..
    } = live;
    let sources = sources
        .into_iter()
        .map(|(p, sha)| {
            let same = sha256_file(&p).is_ok_and(|now| now == sha);
            (p.display().to_string(), sha, same)
        })
        .collect();
    // A ring that won't read whole (AH.11: a hero leaving takes O5's ranges
    // with him) is kept as its error; the drive's other outputs stand.
    let (watch, watch_error) = match &watched {
        Some((w, c)) => match read_watch(&control, w, c.as_ref(), &mut pine, identity, dir) {
            Ok(r) => (Some(r), None),
            Err(e) => (None, Some(format!("reading the watch: {e:#}"))),
        },
        None => (None, None),
    };
    let path = rrc_dump::output::refuse_repo_output(&dir.join(PORTAL_EVENTS_FILE))?;
    fs::write(
        &path,
        serde_json::to_string_pretty(&control.portal_events())?,
    )?;
    // The route's record: its steps, what fired per frame (named while PINE
    // is still open), what changed between reads, the scripts that started.
    if inputs.disc.is_some() {
        let fired = match (&watch, &route_watch) {
            (Some(_), Some(_)) => match route_frames(&dir.join(WatchFile::FILE), &mut pine) {
                Ok((frames, fired)) => {
                    let path = rrc_dump::output::refuse_repo_output(&dir.join(ROUTE_FRAMES_FILE))?;
                    fs::write(&path, serde_json::to_string(&frames)?)?;
                    fired
                }
                Err(e) => {
                    let path =
                        rrc_dump::output::refuse_repo_output(&dir.join("route-frames.error.txt"))?;
                    fs::write(&path, format!("the route's frames: {e:#}\n"))?;
                    Vec::new()
                }
            },
            _ => Vec::new(),
        };
        let record = route_record(identity, plan, &trace, &censuses, &reads, fired);
        record.write(dir)?;
    }
    // A held PINE connection hangs a save and the shutdown (AF.8).
    drop(pine);
    let (saved, save_refusals, save_error) = if plan.save && trace.outcome == Outcome::Done {
        let tried = save_retrying(&control, dir);
        let written = match (&tried.saved, tried.asked_at) {
            (Some(saved), _) => Some((saved.path.clone(), saved.saved_at.frames)),
            (None, Some(asked_at)) => saved_in(dir).map(|p| (p, asked_at)),
            (None, None) => None,
        };
        let saved = match written {
            Some((path, saved_at)) => Some(EndState {
                file: file_name(&path),
                saved_at,
                bytes: fs::metadata(&path)?.len(),
                sha256: sha256_file(&path)?,
            }),
            None => None,
        };
        (saved, tried.refusals, tried.error)
    } else {
        (None, Vec::new(), None)
    };
    let shutdown = format!("{:?}", oracle.shutdown()?);
    Ok(Driven {
        boot_secs,
        watch_error,
        pokes,
        acts,
        sources,
        trace,
        shots,
        reads,
        watch,
        saved,
        save_refusals,
        save_error,
        shutdown,
    })
}

/// The route's placements' wake state per frame, beside the record
/// (`route-frames.json`).
pub const ROUTE_FRAMES_FILE: &str = "route-frames.json";

/// P4's portal events the drive's `STATE` reads caught, each stamped with its
/// `STATE` ([`crate::control::PortalSeen`]).
pub const PORTAL_EVENTS_FILE: &str = "portal-events.json";

/// One record of a route's watch: the game frame, the hero's pivot, the
/// fight's cells and each watched placement's wake state
/// ([`census::WATCHED`]'s order).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FrameLine {
    pub flip: u64,
    pub read: u32,
    pub game_frame: u32,
    pub pivot: [f32; 3],
    pub health: crate::script::Number,
    pub experience: u32,
    pub gold: u32,
    /// The figure's playtime, seconds, off the same words (AH.11).
    #[serde(default)]
    pub playtime_secs: u32,
    pub loot: crate::script::Number,
    /// The hero's invulnerable flag and the uncommon and rare loot counts,
    /// when the watch holds them (AH.9).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub invulnerable: Option<crate::script::Number>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub loot_uncommon: Option<crate::script::Number>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub loot_rare: Option<crate::script::Number>,
    pub wakes: Vec<Wake>,
}

/// The fight's cells and the route's placements on the watch list, from the
/// pause F0 is read at: refused when a watched placement isn't listed once.
fn route_watch(pine: &mut PineClient, rows: &Rows, disc: &Disc) -> Result<FightWatch> {
    let frame = actors::read_frame(pine).context("the actors for the route's watch")?;
    let hero = frame.hero()?;
    let state = fight::read_paused(pine, hero.placement, rows)?;
    let offsets = disc.offsets();
    let base = crate::level::section_base(&frame, &offsets)?;
    let named = census::WATCHED
        .iter()
        .map(|&n| {
            let off = offsets
                .get(n)
                .with_context(|| format!("`{n}` isn't a placement the disc lists once"))?;
            Ok((n, base.base + off))
        })
        .collect::<Result<Vec<_>>>()?;
    let triggers = fight::triggers(pine, &named)?;
    Ok(FightWatch::of(&state, rows)?.with_triggers(&triggers))
}

/// Every record of the ring at `path` as a [`FrameLine`], and what fired
/// record by record, the behaviours named through PINE at the end pause.
fn route_frames(
    path: &Path,
    pine: &mut PineClient,
) -> Result<(Vec<FrameLine>, Vec<census::Fired>)> {
    let file = WatchFile::load(path)?;
    let fight = file
        .watch
        .fight
        .as_ref()
        .context("the watch carries no fight cells")?;
    let names: Vec<String> = fight.triggers.iter().map(|(n, _)| n.clone()).collect();
    let recording = file.recording()?;
    let mut lines = Vec::with_capacity(file.records.len());
    for (raw, frame) in file.records.iter().zip(recording.frames()) {
        let record = raw.record()?;
        if (record.flip, record.read) != (frame.flip, u64::from(frame.read)) {
            bail!(
                "the recording's frame at flip {} isn't the record at {}",
                frame.flip,
                record.flip
            );
        }
        // A record whose cells don't read (AH.11: the hero gone) is left out.
        let Some(f) = file.watch.fight(&record) else {
            continue;
        };
        lines.push(FrameLine {
            flip: record.flip,
            read: frame.read,
            game_frame: frame.game_frame,
            pivot: frame.hero.position,
            health: f.health,
            experience: f.experience,
            gold: f.gold,
            playtime_secs: f.playtime_secs,
            loot: f.loot,
            invulnerable: Some(f.invulnerable),
            loot_uncommon: f.loot_uncommon,
            loot_rare: f.loot_rare,
            wakes: f
                .triggers
                .iter()
                .map(|t| Wake {
                    activity_state: t.activity_state,
                    behaviour: t.behaviour,
                    active_seconds: t.active_seconds,
                })
                .collect(),
        });
    }
    let pointers: std::collections::BTreeSet<u32> = lines
        .iter()
        .flat_map(|l| l.wakes.iter().map(|w| w.behaviour))
        .filter(|&p| p != 0)
        .collect();
    crate::rng::check_title(pine)?;
    let names_of = census::behaviour_names(pine, &pointers)?;
    let wakes: Vec<(u64, u32, Vec<Wake>)> = lines
        .iter()
        .map(|l| (l.flip, l.game_frame, l.wakes.clone()))
        .collect();
    let fired = census::fired(&names, &wakes, &names_of)?;
    Ok((lines, fired))
}

/// A drive's [`RouteRecord`] out of its trace, its censuses in read order,
/// its reads and what fired per frame.
fn route_record(
    identity: &str,
    plan: &RoutePlan,
    trace: &Trace,
    censuses: &[(String, Census)],
    reads: &[ReadTaken],
    fired: Vec<census::Fired>,
) -> RouteRecord {
    let changes = censuses
        .windows(2)
        .map(|w| {
            (
                w[0].0.clone(),
                w[1].0.clone(),
                w[1].1.changes_since(&w[0].1),
            )
        })
        .collect();
    let scripts = match (censuses.first(), censuses.last()) {
        (Some((_, first)), Some((_, last))) => RouteRecord::scripts_between(first, last),
        _ => Default::default(),
    };
    let anchor = reads.last().map(|r| {
        let look = trace.looks.iter().rev().find(|l| l.seen.flip == r.flip);
        let fired = censuses
            .iter()
            .find(|(l, _)| *l == r.label)
            .map(|(_, c)| {
                c.triggers
                    .iter()
                    .filter(|t| t.fired == Some(true))
                    .map(|t| t.name.clone())
                    .collect()
            })
            .unwrap_or_default();
        let mut files = vec![
            format!("actors-{}.json", r.label),
            format!("peek-{}.json", r.label),
        ];
        if r.fight {
            files.push(format!("fight-{}.json", r.label));
        }
        if r.census_ms.is_some() {
            files.push(format!("census-{}.json", r.label));
        }
        census::AnchorReads {
            label: r.label.clone(),
            flip: r.flip,
            game_frame: r.game_frame,
            pivot: look.map_or([0.0; 3], |l| l.seen.pivot),
            facing: look.map_or(0.0, |l| l.seen.facing),
            files,
            seed: r.seed,
            drawn: r.drawn,
            fired,
        }
    });
    RouteRecord {
        identity: identity.to_owned(),
        route: plan.to_ron(),
        steps: RouteRecord::steps(&trace.looks, plan),
        fired,
        changes,
        scripts,
        anchor,
    }
}

#[cfg(test)]
mod tests {
    use super::super::{Look, Point, Seen, Step};
    use super::*;
    use crate::fight::Trigger;
    use crate::level::SectionBase;
    use crate::script::Number;

    /// AH.10's pokes: a placement, a dotted path through names with spaces,
    /// a number; anything else refused.
    #[test]
    fn a_poke_reads_its_placement_path_and_value_and_refuses_the_rest() {
        let p = Poke::parse("Enemy_Chompy(2):description.creature info.hp=100").unwrap();
        assert_eq!(
            p,
            Poke {
                placement: "Enemy_Chompy(2)".into(),
                path: vec!["description".into(), "creature info".into(), "hp".into()],
                value: 100.0,
            }
        );
        // The value is what follows the last `=`; a path may hold a colon's
        // worth of nothing else.
        let q = Poke::parse("a:b.c=-2.5").unwrap();
        assert_eq!((q.path.len(), q.value), (2, -2.5));
        for bad in [
            "no colon=1",
            "a:b.c",
            ":b=1",
            "a:=1",
            "a:b..c=1",
            "a:b=x",
            "a:b=inf",
            "a:b=NaN",
        ] {
            assert!(Poke::parse(bad).is_err(), "{bad} should be refused");
        }
    }

    /// An integer cell takes the value truncated toward zero, a float cell
    /// the nearest single.
    #[test]
    fn a_poked_word_takes_its_cells_kind() {
        assert_eq!(
            poked_word(Number::Int(1), 100.0),
            (Number::Int(100), 100i32.to_be_bytes())
        );
        assert_eq!(
            poked_word(Number::Int(1), -2.9),
            (Number::Int(-2), (-2i32).to_be_bytes())
        );
        assert_eq!(
            poked_word(Number::Float(0.0), 0.1),
            (Number::Float(0.1), 0.1f32.to_bits().to_be_bytes())
        );
    }

    fn trigger(name: &str, fired: Option<bool>) -> Trigger {
        Trigger {
            name: name.to_owned(),
            placement: 0x3000_0000,
            activity_state: 1,
            started: false,
            active_seconds: 0.0,
            range: 20.0,
            fired,
        }
    }

    fn census(flip: u64, triggers: Vec<Trigger>) -> Census {
        Census {
            flip,
            game_frame: 5_960 + flip as u32,
            base: SectionBase {
                base: 0x3000_0000,
                votes: 3,
                dissent: 0,
            },
            briefs: Vec::new(),
            triggers,
            whole: Vec::new(),
        }
    }

    fn seen(flip: u64, x: f64) -> Seen {
        Seen {
            flip,
            game_frame: 5_960 + flip as u32,
            pivot: [x, 43.4, -93.4],
            facing: 90.0,
            velocity: [0.0; 3],
            view: 15.0,
            eye: [0.0; 3],
        }
    }

    fn read(label: &str, flip: u64) -> ReadTaken {
        ReadTaken {
            label: label.to_owned(),
            flip,
            game_frame: 5_960 + flip as u32,
            hero: "009_Eruptor".to_owned(),
            seed: Some(10_554),
            drawn: Some(133_619),
            fight: true,
            census_ms: Some(40.0),
            solids: Some(Ok(64)),
        }
    }

    #[test]
    fn the_anchors_reads_name_the_range_triggers_fired_there_and_where_he_stood() {
        let plan = RoutePlan {
            steps: vec![
                Step::Via {
                    to: Point { x: 20.0, z: -93.4 },
                    radius: 1.0,
                },
                Step::Read {
                    label: "anchor".into(),
                },
            ],
            save: true,
        };
        let look = |flip, x, step, did: &str| Look {
            seen: seen(flip, x),
            step,
            did: did.to_owned(),
        };
        let trace = Trace {
            f0: 2,
            looks: vec![
                look(2, 10.0, 0, "walk"),
                look(10, 15.0, 0, "walk"),
                look(18, 19.5, 1, "read anchor"),
                look(18, 19.5, 2, "done"),
            ],
            input: None,
            input_ron: String::new(),
            flips: 16,
            outcome: Outcome::Done,
        };
        // Fired at the anchor: the range trigger that has been active, not
        // one asleep and not one without a range (awake from the level's
        // start, decided by its controller).
        let censuses = vec![
            (
                "f0".to_owned(),
                census(
                    2,
                    vec![
                        trigger("Setup_001_Trigger", Some(false)),
                        trigger("Setup_003_Trigger(1)", None),
                    ],
                ),
            ),
            (
                "anchor".to_owned(),
                census(
                    18,
                    vec![
                        trigger("Setup_001_Trigger", Some(true)),
                        trigger("Setup_003_Trigger(1)", None),
                        trigger("Activate_Norticus and Chompies", Some(false)),
                    ],
                ),
            ),
        ];
        let reads = vec![read("f0", 2), read("anchor", 18)];
        let r = route_record("oracle", &plan, &trace, &censuses, &reads, Vec::new());
        let a = r.anchor.as_ref().unwrap();
        assert_eq!(a.fired, vec!["Setup_001_Trigger".to_owned()]);
        assert_eq!(
            (a.label.as_str(), a.flip, a.game_frame, a.pivot[0]),
            ("anchor", 18, 5_978, 19.5)
        );
        assert_eq!(
            a.files,
            [
                "actors-anchor.json",
                "peek-anchor.json",
                "fight-anchor.json",
                "census-anchor.json"
            ]
        );
        assert_eq!((a.seed, a.drawn), (Some(10_554), Some(133_619)));
        assert_eq!(r.changes.len(), 1);
        assert_eq!(
            (r.changes[0].0.as_str(), r.changes[0].1.as_str()),
            ("f0", "anchor")
        );
        assert_eq!(r.steps.len(), 3);
    }
}
