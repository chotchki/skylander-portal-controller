//! The extraction's stages in guacamole's order (openspec AE.3 and AE.6,
//! design decision 5). Identity is [`crate::open`], the caller's; then the
//! roster with its portraits, then the character models with their textures
//! and the 3D-UI rig, then ffmpeg is looked for and the intros are indexed
//! and transcoded. The roster lands first so a phone can place figures while
//! the rest runs, and the intros are the only thing a missing or unusable
//! ffmpeg costs. A model that won't read is reported and its figures keep
//! their portraits; the others extract. Every file goes through the identity
//! dir's per-file resume, and the manifest is marked complete only once
//! every stage is done, so a run that stopped anywhere picks up on the next
//! start.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use guac_ir::{Roster, Sha256};
use sha2::Digest;

use crate::error::{ExtractError, Result};
use crate::extract::IdentityDir;
use crate::ffmpeg::Ffmpeg;
use crate::intros::{self, IntroIndex, IrFormat};
use crate::transcode::{self, Summary, TranscodeEvent};
use crate::{models, roster};

/// What a run reports as it goes.
#[derive(Debug, Clone)]
pub enum Event {
    /// Reading the roster's archives off the dump.
    ReadingRoster,
    /// `done` of `total` portraits written or found already there.
    Portraits {
        done: usize,
        total: usize,
    },
    /// The roster and every portrait are in the extract: the figure list can
    /// be published now (AE.4).
    RosterReady(Arc<Roster>),
    /// `done` of `total` character models written, kept or reported.
    Models {
        done: usize,
        total: usize,
    },
    /// ffmpeg was found; mapping toy types to intro movies.
    Indexing,
    Transcode(TranscodeEvent),
}

/// Files a stage needed: written this run or kept from an earlier one.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Files {
    pub written: usize,
    pub kept: usize,
    /// Both together.
    pub bytes: u64,
}

#[derive(Debug, Clone)]
pub struct RosterStage {
    pub roster: Arc<Roster>,
    /// The IR file and the portraits.
    pub files: Files,
    pub portraits: usize,
    pub took: Duration,
}

/// A character model that didn't extract: its figures keep their
/// portraits.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelFailure {
    /// The load file, or `ir/rig` when the rig didn't read.
    pub load_file: String,
    /// [`ExtractError::ModelUnreadable`]'s message.
    pub reason: String,
}

#[derive(Debug, Clone)]
pub struct ModelStage {
    /// Character load files whose model is in the extract.
    pub models: usize,
    pub failed: Vec<ModelFailure>,
    /// Distinct texture files, and texture binds over every material.
    pub textures: usize,
    pub binds: usize,
    /// The model and rig IR and the textures.
    pub files: Files,
    pub took: Duration,
}

#[derive(Debug)]
pub struct IntroStage {
    pub ffmpeg: Ffmpeg,
    pub index: IntroIndex,
    pub summary: Summary,
    pub took: Duration,
}

#[derive(Debug)]
pub struct Extracted {
    pub roster: RosterStage,
    pub models: ModelStage,
    /// The intros, or why there are none. The roster stays either way, and
    /// the manifest stays unmarked, so the next start tries the intros again.
    pub intros: Result<IntroStage>,
}

/// Every stage after identity, on a dir [`crate::open`] returned as anything
/// but current. A roster that doesn't read stops the run; anything that stops
/// the intros is reported in [`Extracted::intros`].
pub fn run(
    dir: &mut IdentityDir,
    ffmpeg: Option<&Path>,
    ir: IrFormat,
    send: &(dyn Fn(Event) + Sync),
) -> Result<Extracted> {
    let roster = roster_stage(dir, ir, send)?;
    send(Event::RosterReady(roster.roster.clone()));
    let models = model_stage(dir, &roster.roster, ir, send)?;
    let intros = intro_stage(dir, &roster.roster, ffmpeg, ir, send);
    if intros.is_ok() {
        dir.mark_complete()?;
    }
    Ok(Extracted {
        roster,
        models,
        intros,
    })
}

/// Each character load file `roster` names, its model into
/// `models/<load file>.<ron|postcard>` and its textures into
/// `textures/<sha256>.ktx2`, then `global.bld`'s rig into
/// `ir/rig.<ron|postcard>`. A model or rig that won't read is reported in
/// [`ModelStage::failed`]; a file that won't write stops the run.
pub fn model_stage(
    dir: &mut IdentityDir,
    roster: &Roster,
    ir: IrFormat,
    send: &(dyn Fn(Event) + Sync),
) -> Result<ModelStage> {
    let started = Instant::now();
    let usrdir = PathBuf::from(&dir.manifest().source_dir).join("PS3_GAME/USRDIR");
    let identity = dir.manifest().identity.clone();
    let loads = models::character_loads(roster);
    let total = loads.len();
    let mut stage = ModelStage {
        models: 0,
        failed: Vec::new(),
        textures: 0,
        binds: 0,
        files: Files::default(),
        took: Duration::ZERO,
    };
    let mut written: HashSet<String> = HashSet::new();
    send(Event::Models { done: 0, total });
    for (i, load) in loads.iter().enumerate() {
        let path = models::archive_path(&usrdir, load);
        let extracted = models::archive(&path, load)
            .and_then(|bytes| models::level(&bytes, load))
            .and_then(|level| models::read(&level, load, identity.clone()));
        match extracted {
            Ok(m) => {
                stage.binds += m.binds;
                for (rel, ktx2) in &m.textures {
                    if written.insert(rel.clone()) {
                        put(dir, rel, ktx2, &mut stage.files)?;
                    }
                }
                let rel = ir.model_path(load);
                let bytes = ir.encode(&m.model, dir, &rel)?;
                put(dir, &rel, &bytes, &mut stage.files)?;
                stage.models += 1;
            }
            Err(e @ ExtractError::ModelUnreadable { .. }) => stage.failed.push(ModelFailure {
                load_file: (*load).to_owned(),
                reason: e.to_string(),
            }),
            Err(e) => return Err(e),
        }
        send(Event::Models { done: i + 1, total });
    }
    stage.textures = written.len();
    let rig = std::fs::read(usrdir.join(roster::GLOBAL))
        .map_err(ExtractError::io(usrdir.join(roster::GLOBAL)))
        .and_then(|bytes| models::level(&bytes, roster::GLOBAL))
        .and_then(|level| models::rig(&level, identity));
    match rig {
        Ok(rig) => {
            let bytes = ir.encode(&rig, dir, ir.rig_path())?;
            put(dir, ir.rig_path(), &bytes, &mut stage.files)?;
        }
        Err(e @ ExtractError::ModelUnreadable { .. }) => stage.failed.push(ModelFailure {
            load_file: "ir/rig".into(),
            reason: e.to_string(),
        }),
        Err(e) => return Err(e),
    }
    stage.took = started.elapsed();
    Ok(stage)
}

/// The roster off the dump `dir` names, into `ir/roster.<ron|postcard>`,
/// then each figure's portrait into `portraits/<figure id>.png`.
pub fn roster_stage(
    dir: &mut IdentityDir,
    ir: IrFormat,
    send: &(dyn Fn(Event) + Sync),
) -> Result<RosterStage> {
    let started = Instant::now();
    send(Event::ReadingRoster);
    let m = dir.manifest();
    let (roster, portraits) =
        roster::read_with_portraits(&PathBuf::from(&m.source_dir), m.identity.clone())?;
    let mut files = Files::default();
    let bytes = ir.encode(&roster, dir, ir.roster_path())?;
    put(dir, ir.roster_path(), &bytes, &mut files)?;
    let total = portraits.len();
    send(Event::Portraits { done: 0, total });
    for (i, p) in portraits.iter().enumerate() {
        put(dir, &p.path, &roster::png(p)?, &mut files)?;
        send(Event::Portraits { done: i + 1, total });
    }
    Ok(RosterStage {
        roster: Arc::new(roster),
        files,
        portraits: total,
        took: started.elapsed(),
    })
}

/// ffmpeg, then the intro index with the roster's names as its fallback, its
/// IR and the transcode.
fn intro_stage(
    dir: &mut IdentityDir,
    roster: &Roster,
    ffmpeg: Option<&Path>,
    ir: IrFormat,
    send: &(dyn Fn(Event) + Sync),
) -> Result<IntroStage> {
    let started = Instant::now();
    let ffmpeg = Ffmpeg::locate(ffmpeg)?;
    send(Event::Indexing);
    let index = intros::index(dir, &roster::base_names(roster))?;
    intros::write(dir, &index.table, ir)?;
    let summary =
        transcode::transcode_intros(dir, &index.table, &ffmpeg, &|e| send(Event::Transcode(e)))?;
    Ok(IntroStage {
        ffmpeg,
        index,
        summary,
        took: started.elapsed(),
    })
}

/// Writes `bytes` to `rel` unless the extract already holds exactly them: the
/// manifest records these bytes and the file still hashes to the record.
fn put(dir: &mut IdentityDir, rel: &str, bytes: &[u8], files: &mut Files) -> Result<()> {
    let sha = Sha256(sha2::Sha256::digest(bytes).into());
    let size = bytes.len() as u64;
    let recorded = dir
        .manifest()
        .outputs
        .iter()
        .any(|o| o.path == rel && (o.size, o.sha256) == (size, sha));
    if recorded && dir.output_done(rel)? {
        files.kept += 1;
    } else {
        dir.write_output(rel, bytes)?;
        files.written += 1;
    }
    files.bytes += size;
    Ok(())
}
