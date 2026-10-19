//! The extraction's stages in guacamole's order (openspec AE.3, AE.6, AG.14
//! and AG.15, design decision 5). Identity is [`crate::open`], the caller's;
//! then the roster with its portraits, then the character models with their
//! textures, each with its hero side, and the 3D-UI rig, then the levels the
//! build plays, part by part, then ffmpeg is looked for and the intros are
//! indexed and transcoded. The roster lands first so a phone can place
//! figures while the rest runs, and the intros are the only thing a missing
//! or unusable ffmpeg costs. A model that won't read is reported and its
//! figures keep their portraits; a hero side that doesn't settle is reported
//! and its model stays; a level part that won't read is reported and the
//! other parts land; the others extract. Every file goes through the identity
//! dir's per-file resume, each level part is recorded once its files are in,
//! and the manifest is marked complete only once every stage is done, so a
//! run that stopped anywhere picks up on the next start (and a level part
//! missing from a complete extract resumes alone).

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
use crate::{levels, models, roster};

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
    /// `done` of `total` level parts written, kept or reported (AG.15).
    Level {
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

/// A character model that didn't extract (its figures keep their
/// portraits), or a hero side that didn't settle (its figures don't walk).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelFailure {
    /// The load file, or `ir/rig` when the rig didn't read.
    pub load_file: String,
    /// [`ExtractError::ModelUnreadable`]'s or
    /// [`ExtractError::HeroUnsettled`]'s message.
    pub reason: String,
}

#[derive(Debug, Clone)]
pub struct ModelStage {
    /// Character load files whose model is in the extract.
    pub models: usize,
    pub failed: Vec<ModelFailure>,
    /// Of those, the ones whose hero side is in it too (AG.14), and the
    /// ones whose hero didn't settle.
    pub heroes: usize,
    pub unsettled: Vec<ModelFailure>,
    /// Of [`ModelStage::took`], what reading and writing the hero sides
    /// took.
    pub heroes_took: Duration,
    /// Distinct texture files, and texture binds over every material.
    pub textures: usize,
    pub binds: usize,
    /// The model and rig IR and the textures.
    pub files: Files,
    pub took: Duration,
}

/// A level part that didn't read: its level and part, and
/// [`ExtractError::LevelUnreadable`]'s message.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LevelFailure {
    pub level: String,
    pub part: String,
    pub reason: String,
}

#[derive(Debug, Clone)]
pub struct LevelStage {
    /// Levels the build plays with every part in the extract.
    pub levels: usize,
    /// Parts read and written this run, and parts already in and kept.
    pub parts_read: usize,
    pub parts_kept: usize,
    pub failed: Vec<LevelFailure>,
    /// Props read this run, and those refused (in the props part).
    pub props: usize,
    pub refused_props: Vec<(String, String)>,
    /// Distinct texture files the parts read this run bind, and of those the
    /// ones the store already held when the stage began (the characters').
    pub textures: usize,
    pub shared: usize,
    /// The parts' IR and textures.
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
    pub levels: LevelStage,
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
    let levels = level_stage(dir, ir, send)?;
    let intros = intro_stage(dir, &roster.roster, ffmpeg, ir, send);
    if intros.is_ok() {
        dir.mark_complete()?;
    }
    Ok(Extracted {
        roster,
        models,
        levels,
        intros,
    })
}

/// Each level the build plays ([`levels::LEVELS`]), part by part, into
/// `levels/<archive>/<part>.<ron|postcard>` and its textures into the
/// characters' store. A part already in ([`IdentityDir::level_part_done`])
/// is kept without reading the level; one that won't read is reported in
/// [`LevelStage::failed`] and the others land; a file that won't write
/// stops the run.
pub fn level_stage(
    dir: &mut IdentityDir,
    ir: IrFormat,
    send: &(dyn Fn(Event) + Sync),
) -> Result<LevelStage> {
    let started = Instant::now();
    let usrdir = PathBuf::from(&dir.manifest().source_dir).join("PS3_GAME/USRDIR");
    let identity = dir.manifest().identity.clone();
    let store: HashSet<String> = dir
        .manifest()
        .outputs
        .iter()
        .filter(|o| o.path.starts_with(models::TEXTURE_DIR))
        .map(|o| o.path.clone())
        .collect();
    let mut stage = LevelStage {
        levels: 0,
        parts_read: 0,
        parts_kept: 0,
        failed: Vec::new(),
        props: 0,
        refused_props: Vec::new(),
        textures: 0,
        shared: 0,
        files: Files::default(),
        took: Duration::ZERO,
    };
    let total = levels::LEVELS.len() * levels::PARTS.len();
    let mut done = 0;
    send(Event::Level { done, total });
    // Every file counted, kept or written, and the textures the parts read
    // this run bind.
    let mut counted: HashSet<String> = HashSet::new();
    let mut read_textures: HashSet<String> = HashSet::new();
    for level in levels::LEVELS {
        let mut needed = Vec::new();
        for part in levels::PARTS {
            if dir.level_part_done(level, part, &ir.level_path(level, part))? {
                let files = dir.manifest().levels[level].parts[part].clone();
                // A texture two parts bind is one file, counted once.
                for f in files.into_iter().filter(|f| counted.insert(f.clone())) {
                    stage.files.kept += 1;
                    stage.files.bytes += dir
                        .manifest()
                        .outputs
                        .iter()
                        .find(|o| o.path == f)
                        .map_or(0, |o| o.size);
                }
                stage.parts_kept += 1;
                done += 1;
                send(Event::Level { done, total });
            } else {
                needed.push(part);
            }
        }
        if needed.is_empty() {
            stage.levels += 1;
            continue;
        }
        let path = levels::archive_path(&usrdir, level);
        let bytes = match std::fs::read(&path) {
            Ok(b) => Ok(b),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                Err(format!("no archive at {}", path.display()))
            }
            Err(e) => return Err(ExtractError::io(&path)(e)),
        };
        let unpacked = bytes.and_then(|b| levels::level_bld(&b));
        let igz = match &unpacked {
            Ok(b) => guac_formats::igz::Igz::parse(b).map_err(|e| e.to_string()),
            Err(e) => Err(e.clone()),
        };
        let mut whole = true;
        for part in needed {
            let mut props = None;
            let read =
                igz.as_ref().map_err(Clone::clone).and_then(|igz| {
                    let (id, rel) = (identity.clone(), ir.level_path(level, part));
                    match part {
                        levels::SCENE => {
                            levels::scene(igz, level, id).and_then(|r| encoded(ir, dir, &rel, r))
                        }
                        levels::PROPS => levels::props(igz, level, id).and_then(|r| {
                            let refused =
                                r.ir.refused
                                    .iter()
                                    .map(|p| (p.path.clone(), p.reason.clone()));
                            props = Some((r.ir.props.len(), refused.collect::<Vec<_>>()));
                            encoded(ir, dir, &rel, r)
                        }),
                        levels::COLLISION => levels::collision(igz, level, id)
                            .and_then(|r| encoded(ir, dir, &rel, r)),
                        levels::PLACEMENTS => levels::placements(igz, level, id)
                            .and_then(|r| encoded(ir, dir, &rel, r)),
                        levels::LIGHTS => {
                            levels::lights(igz, level, id).and_then(|r| encoded(ir, dir, &rel, r))
                        }
                        other => Err(format!("no reader for a part named {other}")),
                    }
                });
            match read {
                Ok((bytes, part_textures)) => {
                    let mut files = Vec::with_capacity(part_textures.len() + 1);
                    for (rel, ktx2) in &part_textures {
                        if read_textures.insert(rel.clone()) && store.contains(rel) {
                            stage.shared += 1;
                        }
                        if counted.insert(rel.clone()) {
                            put(dir, rel, ktx2, &mut stage.files)?;
                        }
                        files.push(rel.clone());
                    }
                    let rel = ir.level_path(level, part);
                    put(dir, &rel, &bytes, &mut stage.files)?;
                    files.push(rel);
                    dir.record_level_part(level, part, files)?;
                    if let Some((props, refused)) = props {
                        stage.props += props;
                        stage.refused_props.extend(
                            refused
                                .into_iter()
                                .map(|(p, r)| (format!("{level}: {p}"), r)),
                        );
                    }
                    stage.parts_read += 1;
                }
                Err(reason) => {
                    whole = false;
                    let e = ExtractError::LevelUnreadable {
                        level: level.to_owned(),
                        part: part.to_owned(),
                        reason,
                    };
                    stage.failed.push(LevelFailure {
                        level: level.to_owned(),
                        part: part.to_owned(),
                        reason: e.to_string(),
                    });
                }
            }
            done += 1;
            send(Event::Level { done, total });
        }
        if whole {
            stage.levels += 1;
        }
    }
    stage.textures = read_textures.len();
    stage.took = started.elapsed();
    Ok(stage)
}

/// Each character load file `roster` names, its model into
/// `models/<load file>.<ron|postcard>`, its textures into
/// `textures/<sha256>.ktx2` and its hero side into
/// `heroes/<load file>.<ron|postcard>`, then `global.bld`'s rig into
/// `ir/rig.<ron|postcard>`. A model or rig that won't read is reported in
/// [`ModelStage::failed`], a hero that doesn't settle in
/// [`ModelStage::unsettled`]; a file that won't write stops the run.
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
        heroes: 0,
        unsettled: Vec::new(),
        heroes_took: Duration::ZERO,
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
                let started = Instant::now();
                match &m.hero {
                    Ok(hero) => {
                        let rel = ir.hero_path(load);
                        let bytes = ir.encode(hero, dir, &rel)?;
                        put(dir, &rel, &bytes, &mut stage.files)?;
                        stage.heroes += 1;
                    }
                    Err(reason) => stage.unsettled.push(ModelFailure {
                        load_file: (*load).to_owned(),
                        reason: reason.clone(),
                    }),
                }
                stage.heroes_took += m.hero_took + started.elapsed();
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

/// A part's IR in its format, and the textures it binds by path.
type PartFiles = (Vec<u8>, Vec<(String, Vec<u8>)>);

/// `r`'s IR in `ir`'s format, beside its textures.
fn encoded<T: serde::Serialize>(
    ir: IrFormat,
    dir: &IdentityDir,
    rel: &str,
    r: levels::Read<T>,
) -> std::result::Result<PartFiles, String> {
    let bytes = ir.encode(&r.ir, dir, rel).map_err(|e| e.to_string())?;
    Ok((bytes, r.textures))
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
