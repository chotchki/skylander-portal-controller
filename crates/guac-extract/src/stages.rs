//! The extraction's stages in guacamole's order (openspec AE.3, AE.6, AG.14,
//! AG.15, AH.2 and AH.22, design decision 5). Identity is [`crate::open`],
//! the caller's; then the roster with its portraits, then the progression
//! tables (a part of their own: a resume reads them only when they're
//! missing), then the character models with their textures, each with its
//! hero side and its palette, and the 3D-UI rig, then the levels the build
//! plays, part by part, then the fight's script entries and spawn models,
//! then ffmpeg is looked for and the intros are indexed and transcoded. The roster lands first so a phone can place
//! figures while the rest runs, the models are announced as they finish
//! ([`Event::ModelsReady`], AE.13) so the turntable draws while the level
//! and the intros run, and the intros are the only thing a missing
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

use guac_ir::progression::Progression;
use guac_ir::{Roster, Sha256};
use sha2::Digest;

use crate::error::{ExtractError, Result};
use crate::extract::IdentityDir;
use crate::ffmpeg::Ffmpeg;
use crate::intros::{self, IntroIndex, IrFormat};
use crate::transcode::{self, Summary, TranscodeEvent};
use crate::{levels, models, progression, roster, scripts};

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
    /// The progression tables are in the extract (AH.2): the store's caps
    /// and the editor's levels can be published now.
    ProgressionReady(Arc<Progression>),
    /// `done` of `total` character models written, kept or reported.
    Models {
        done: usize,
        total: usize,
    },
    /// Every model the roster names and the rig are in the extract, or in
    /// `failed` (AE.13): the turntable can draw from it now, while the level
    /// and the intros run.
    ModelsReady {
        failed: Vec<ModelFailure>,
    },
    /// `done` of `total` level parts written, kept or reported (AG.15).
    Level {
        done: usize,
        total: usize,
    },
    /// `done` of `total` script parts written, kept or reported (AH.22).
    Scripts {
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

/// The progression tables: read off the dump this run, or kept.
#[derive(Debug, Clone)]
pub struct ProgressionStage {
    /// The tables, or [`ExtractError::ProgressionUnreadable`]'s message:
    /// the other stages run either way, and the next start tries again.
    pub progression: std::result::Result<Arc<Progression>, String>,
    /// Read off the dump this run, not kept from an earlier one.
    pub read: bool,
    pub files: Files,
    pub took: Duration,
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
    /// The palettes written or kept (AH.22), one per model, and what they
    /// hold.
    pub palettes: PaletteCount,
    /// Distinct texture files, and texture binds over every material.
    pub textures: usize,
    pub binds: usize,
    /// The model, hero and rig IR and the textures; the palettes' are
    /// [`PaletteCount::files`].
    pub files: Files,
    pub took: Duration,
}

/// What the palettes a stage wrote or kept hold (AH.22).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct PaletteCount {
    /// Palettes, and their slots that hold a clip, a network or something
    /// refused.
    pub palettes: usize,
    pub clips: usize,
    pub networks: usize,
    pub refused: usize,
    /// Their IR files, written or kept.
    pub files: Files,
    /// Reading and writing them, of the stage's time.
    pub took: Duration,
}

impl PaletteCount {
    fn add(&mut self, p: &guac_ir::palette::Palette) {
        use guac_ir::palette::SlotEntry;
        self.palettes += 1;
        for e in &p.entries {
            match e.entry {
                SlotEntry::Clip { .. } => self.clips += 1,
                SlotEntry::Network => self.networks += 1,
            }
        }
        self.refused += p.refused.len();
    }
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
    /// Scripts in the script parts read this run (AH.22), and what
    /// checking and writing those entries took.
    pub scripts: usize,
    pub script_took: Duration,
    /// The creature palettes read this run (AH.22).
    pub palettes: PaletteCount,
    /// Distinct texture files the parts read this run bind, and of those the
    /// ones the manifest already recorded when the stage began: on a first
    /// run the characters', on a resume the level's own from before too.
    pub textures: usize,
    pub shared: usize,
    /// The parts' IR and textures.
    pub files: Files,
    pub took: Duration,
}

/// A script part that didn't read: its part, the archive it stopped at
/// and [`ExtractError::ScriptsUnreadable`]'s message.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PartFailure {
    pub part: String,
    pub archive: String,
    pub reason: String,
}

/// The fight's script entries and spawn models (AH.22).
#[derive(Debug, Clone, Default)]
pub struct ScriptStage {
    /// Parts read and written this run, and parts already in and kept.
    pub parts_read: usize,
    pub parts_kept: usize,
    pub failed: Vec<PartFailure>,
    /// Entries checked and written this run, and the scripts they hold.
    pub entries: usize,
    pub scripts: usize,
    /// Spawn models read this run, and those refused.
    pub props: usize,
    pub refused_props: Vec<(String, String)>,
    /// Of [`ScriptStage::files`], the script entries' and the spawn
    /// models' (their IR and textures).
    pub script_files: Files,
    pub spawn_files: Files,
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
    pub progression: ProgressionStage,
    pub models: ModelStage,
    pub levels: LevelStage,
    pub scripts: ScriptStage,
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
    let progression = progression_stage(dir, ir)?;
    if let Ok(p) = &progression.progression {
        send(Event::ProgressionReady(p.clone()));
    }
    let models = model_stage(dir, &roster.roster, ir, send)?;
    send(Event::ModelsReady {
        failed: models.failed.clone(),
    });
    let levels = level_stage(dir, ir, send)?;
    let scripts = script_stage(dir, ir, send)?;
    let intros = intro_stage(dir, &roster.roster, ffmpeg, ir, send);
    if intros.is_ok() {
        dir.mark_complete()?;
    }
    Ok(Extracted {
        roster,
        progression,
        models,
        levels,
        scripts,
        intros,
    })
}

/// The progression tables into `ir/progression.<ron|postcard>`, unless the
/// extract already holds them ([`IdentityDir::part_done`]): then they're
/// loaded, not read. Tables that don't read are reported in
/// [`ProgressionStage::progression`]; a file that won't write stops the run.
pub fn progression_stage(dir: &mut IdentityDir, ir: IrFormat) -> Result<ProgressionStage> {
    let started = Instant::now();
    let rel = ir.progression_path();
    let mut files = Files::default();
    if dir.part_done(progression::PART, rel)?
        && let Ok(p) = progression::load(dir, ir)
    {
        files.kept += 1;
        files.bytes += dir
            .manifest()
            .outputs
            .iter()
            .find(|o| o.path == rel)
            .map_or(0, |o| o.size);
        return Ok(ProgressionStage {
            progression: Ok(Arc::new(p)),
            read: false,
            files,
            took: started.elapsed(),
        });
    }
    let usrdir = PathBuf::from(&dir.manifest().source_dir).join("PS3_GAME/USRDIR");
    let identity = dir.manifest().identity.clone();
    let read = match progression::read(&usrdir, identity) {
        Ok(p) => p,
        Err(e @ (ExtractError::ProgressionUnreadable { .. } | ExtractError::Io { .. })) => {
            return Ok(ProgressionStage {
                progression: Err(e.to_string()),
                read: true,
                files,
                took: started.elapsed(),
            });
        }
        Err(e) => return Err(e),
    };
    let bytes = ir.encode(&read, dir, rel)?;
    put(dir, rel, &bytes, &mut files)?;
    dir.record_part(progression::PART, vec![rel.to_owned()])?;
    Ok(ProgressionStage {
        progression: Ok(Arc::new(read)),
        read: true,
        files,
        took: started.elapsed(),
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
        scripts: 0,
        script_took: Duration::ZERO,
        palettes: PaletteCount::default(),
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
            if dir.level_part_done(level, part, &levels::part_path(ir, level, part))? {
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
            if part == levels::SCRIPT {
                // The entry as the disc holds it, once its graph reads.
                let script_started = Instant::now();
                let checked = igz
                    .as_ref()
                    .map_err(Clone::clone)
                    .and_then(levels::script)
                    .and_then(|n| unpacked.as_ref().map(|e| (n, e)).map_err(Clone::clone));
                match checked {
                    Ok((scripts, entry)) => {
                        let rel = levels::script_path(level);
                        put(dir, &rel, entry, &mut stage.files)?;
                        dir.record_level_part(level, part, vec![rel])?;
                        stage.scripts += scripts;
                        stage.parts_read += 1;
                        stage.script_took += script_started.elapsed();
                    }
                    Err(reason) => {
                        whole = false;
                        stage.failed.push(level_failure(level, part, reason));
                    }
                }
                done += 1;
                send(Event::Level { done, total });
                continue;
            }
            let mut props = None;
            let palettes_started = Instant::now();
            let mut palettes = None;
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
                        levels::ATMOSPHERE => levels::atmosphere(igz, level, id)
                            .and_then(|r| encoded(ir, dir, &rel, r)),
                        levels::PALETTES => levels::palettes(igz, level, id).and_then(|r| {
                            let mut count = PaletteCount::default();
                            r.ir.palettes.iter().for_each(|p| count.add(p));
                            palettes = Some(count);
                            encoded(ir, dir, &rel, r)
                        }),
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
                    let before = stage.files;
                    put(dir, &rel, &bytes, &mut stage.files)?;
                    if let Some(mut count) = palettes {
                        count.files = Files {
                            written: stage.files.written - before.written,
                            kept: stage.files.kept - before.kept,
                            bytes: bytes.len() as u64,
                        };
                        count.took = palettes_started.elapsed();
                        stage.palettes = count;
                    }
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
                    stage.failed.push(level_failure(level, part, reason));
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

/// The fight's script data outside a level (AH.22,
/// [`crate::scripts`]): each script archive's entry, checked and copied
/// raw into `scripts/<stem>.igz`, then each spawn archive's models into
/// `spawns/<stem>.<ron|postcard>` and their textures into the store. A part
/// already in ([`IdentityDir::part_holds`]) is kept without reading; one
/// that doesn't read is reported in [`ScriptStage::failed`], and the other
/// lands; a file that won't write stops the run.
pub fn script_stage(
    dir: &mut IdentityDir,
    ir: IrFormat,
    send: &(dyn Fn(Event) + Sync),
) -> Result<ScriptStage> {
    let started = Instant::now();
    let usrdir = PathBuf::from(&dir.manifest().source_dir).join("PS3_GAME/USRDIR");
    let identity = dir.manifest().identity.clone();
    let mut stage = ScriptStage::default();
    let total = 2;
    send(Event::Scripts { done: 0, total });
    // Each archive's entry, read once whichever part needs it.
    let mut entries: std::collections::BTreeMap<String, std::result::Result<Vec<u8>, String>> =
        std::collections::BTreeMap::new();
    let mut entry = |archive: &str| -> std::result::Result<Vec<u8>, String> {
        entries
            .entry(archive.to_owned())
            .or_insert_with(|| scripts::entry(&usrdir, archive))
            .clone()
    };
    let fail = |stage: &mut ScriptStage, part: &str, archive: &str, reason: String| {
        let e = ExtractError::ScriptsUnreadable {
            part: part.to_owned(),
            archive: archive.to_owned(),
            reason,
        };
        stage.failed.push(PartFailure {
            part: part.to_owned(),
            archive: archive.to_owned(),
            reason: e.to_string(),
        });
    };

    let archives = scripts::archives();
    let files: Vec<String> = archives.iter().map(|a| scripts::script_path(a)).collect();
    if dir.part_holds(scripts::SCRIPTS, &files)? {
        count_kept(dir, &files, &mut stage.script_files);
        stage.parts_kept += 1;
    } else {
        // Whole or not at all: the entries load together.
        let mut checked = Vec::with_capacity(archives.len());
        let mut failed = None;
        for archive in &archives {
            match entry(archive).and_then(|bytes| Ok((scripts::check(&bytes)?, bytes))) {
                Ok((names, bytes)) => checked.push((names.len(), bytes)),
                Err(reason) => {
                    failed = Some((archive.clone(), reason));
                    break;
                }
            }
        }
        match failed {
            Some((archive, reason)) => fail(&mut stage, scripts::SCRIPTS, &archive, reason),
            None => {
                for ((n, bytes), rel) in checked.iter().zip(&files) {
                    put(dir, rel, bytes, &mut stage.script_files)?;
                    stage.entries += 1;
                    stage.scripts += n;
                }
                dir.record_part(scripts::SCRIPTS, files.clone())?;
                stage.parts_read += 1;
            }
        }
    }
    send(Event::Scripts { done: 1, total });

    let archives = scripts::spawn_archives();
    let irs: Vec<String> = archives.iter().map(|a| ir.spawn_path(a)).collect();
    if dir.part_holds(scripts::SPAWNS, &irs)? {
        let recorded = dir.manifest().parts[scripts::SPAWNS].clone();
        count_kept(dir, &recorded, &mut stage.spawn_files);
        stage.parts_kept += 1;
    } else {
        let mut read = Vec::with_capacity(archives.len());
        let mut failed = None;
        for (archive, rel) in archives.iter().zip(&irs) {
            let one = entry(archive).and_then(|bytes| {
                let igz = guac_formats::igz::Igz::parse(&bytes).map_err(|e| e.to_string())?;
                let body = scripts::body(&igz, archive);
                let r = scripts::spawn_models(&igz, archive, identity.clone(), body)?;
                let refused: Vec<(String, String)> =
                    r.ir.refused
                        .iter()
                        .map(|p| (format!("{archive}: {}", p.path), p.reason.clone()))
                        .collect();
                let props = r.ir.props.len();
                let (ir_bytes, textures) = encoded_spawns(ir, dir, rel, r)?;
                Ok((props, refused, ir_bytes, textures))
            });
            match one {
                Ok(r) => read.push(r),
                Err(reason) => {
                    failed = Some((archive.clone(), reason));
                    break;
                }
            }
        }
        match failed {
            Some((archive, reason)) => fail(&mut stage, scripts::SPAWNS, &archive, reason),
            None => {
                let mut files = Vec::new();
                let mut counted: HashSet<String> = HashSet::new();
                for ((props, refused, bytes, textures), rel) in read.into_iter().zip(&irs) {
                    for (t, ktx2) in &textures {
                        if counted.insert(t.clone()) {
                            put(dir, t, ktx2, &mut stage.spawn_files)?;
                            files.push(t.clone());
                        }
                    }
                    put(dir, rel, &bytes, &mut stage.spawn_files)?;
                    files.push(rel.clone());
                    stage.props += props;
                    stage.refused_props.extend(refused);
                }
                dir.record_part(scripts::SPAWNS, files)?;
                stage.parts_read += 1;
            }
        }
    }
    send(Event::Scripts { done: 2, total });
    for f in [stage.script_files, stage.spawn_files] {
        stage.files.written += f.written;
        stage.files.kept += f.kept;
        stage.files.bytes += f.bytes;
    }
    stage.took = started.elapsed();
    Ok(stage)
}

/// `files`, each already in, counted as kept with its recorded size.
fn count_kept(dir: &IdentityDir, files: &[String], into: &mut Files) {
    for f in files {
        into.kept += 1;
        into.bytes += dir
            .manifest()
            .outputs
            .iter()
            .find(|o| o.path == *f)
            .map_or(0, |o| o.size);
    }
}

/// A spawn archive's models in `ir`'s format, beside their textures.
fn encoded_spawns(
    ir: IrFormat,
    dir: &IdentityDir,
    rel: &str,
    r: levels::Read<guac_ir::spawn::SpawnModels>,
) -> std::result::Result<PartFiles, String> {
    let bytes = ir.encode(&r.ir, dir, rel).map_err(|e| e.to_string())?;
    Ok((bytes, r.textures))
}

/// [`ExtractError::LevelUnreadable`] for `level`'s `part`, as reported.
fn level_failure(level: &str, part: &str, reason: String) -> LevelFailure {
    let e = ExtractError::LevelUnreadable {
        level: level.to_owned(),
        part: part.to_owned(),
        reason,
    };
    LevelFailure {
        level: level.to_owned(),
        part: part.to_owned(),
        reason: e.to_string(),
    }
}

/// Each character load file `roster` names, its model into
/// `models/<load file>.<ron|postcard>`, its textures into
/// `textures/<sha256>.ktx2`, its hero side into
/// `heroes/<load file>.<ron|postcard>` and its palette into
/// `palettes/<load file>.<ron|postcard>` (the part recorded once every
/// model has been through), then `global.bld`'s rig into
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
        palettes: PaletteCount::default(),
        textures: 0,
        binds: 0,
        files: Files::default(),
        took: Duration::ZERO,
    };
    let mut written: HashSet<String> = HashSet::new();
    let mut palette_files = Vec::new();
    // `global.bld` and `bootstrap.bld`, for the attachments a global
    // template draws (AE.23); without them those sets stay unsettled.
    let level_of = |archive: &str| {
        std::fs::read(usrdir.join(archive))
            .ok()
            .and_then(|bytes| models::level(&bytes, archive).ok())
    };
    let (global_level, bootstrap_level) = (level_of(roster::GLOBAL), level_of(roster::BOOTSTRAP));
    let globals = match (&global_level, &bootstrap_level) {
        (Some(g), Some(b)) => models::Globals::new(g, b).ok(),
        _ => None,
    };
    send(Event::Models { done: 0, total });
    for (i, load) in loads.iter().enumerate() {
        let path = models::archive_path(&usrdir, load);
        let extracted = models::archive(&path, load)
            .and_then(|bytes| models::level(&bytes, load))
            .and_then(|level| models::read(&level, load, identity.clone(), globals.as_ref()));
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
                let started = Instant::now();
                let rel = ir.palette_path(load);
                let bytes = ir.encode(&m.palette, dir, &rel)?;
                put(dir, &rel, &bytes, &mut stage.palettes.files)?;
                stage.palettes.add(&m.palette);
                stage.palettes.took += m.palette_took + started.elapsed();
                palette_files.push(rel);
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
    // Every model that read has its palette in: the part is.
    dir.record_part(models::PALETTES, palette_files)?;
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
/// then each figure's portrait into `portraits/<figure id>.png`, the right
/// way up, recorded as [`roster::PORTRAITS`] (AE.31).
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
    let written = portraits.iter().map(|p| p.path.clone()).collect();
    dir.record_part(roster::PORTRAITS, written)?;
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
