//! The extraction's stages in guacamole's order (openspec AE.3, design
//! decision 5). Identity is [`crate::open`], the caller's; then the roster
//! with its portraits, then ffmpeg is looked for and the intros are indexed
//! and transcoded. The roster lands first so a phone can place figures while
//! the intros convert, and the intros are the only thing a missing or
//! unusable ffmpeg costs. Every file goes through the identity dir's per-file
//! resume, and the manifest is marked complete only once every stage is done,
//! so a run that stopped anywhere picks up on the next start.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use guac_ir::{Roster, Sha256};
use sha2::Digest;

use crate::error::Result;
use crate::extract::IdentityDir;
use crate::ffmpeg::Ffmpeg;
use crate::intros::{self, IntroIndex, IrFormat};
use crate::roster;
use crate::transcode::{self, Summary, TranscodeEvent};

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
    let intros = intro_stage(dir, &roster.roster, ffmpeg, ir, send);
    if intros.is_ok() {
        dir.mark_complete()?;
    }
    Ok(Extracted { roster, intros })
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
