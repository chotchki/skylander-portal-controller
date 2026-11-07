//! The extraction's stages in guacamole's order (openspec AE.3, AE.6, AG.14,
//! AG.15, AH.2, AH.22, AT.26, AT.31 and AR.21, design decision 5). Identity is
//! [`crate::open`], the caller's; then the roster with its portraits, then
//! the progression tables (a part of their own: a resume reads them only
//! when they're missing), then the character models with their textures,
//! each with its hero side and its palette, and the 3D-UI rig (a part of
//! their own too since AT.30, with a record of what failed: a start that
//! holds it keeps every model without opening an archive), then the
//! parts the TV reads of each level the build plays ([`levels::PLAYED`]),
//! then the fight's script entries and spawn models and the attacks'
//! emitters, and chapter 1 is handed over ([`Event::LevelReady`]); then
//! what nothing on the TV draws yet: each level's sprites (until AT.9), the
//! sounds (AR.5, which need no ffmpeg, until the TV plays them) and the
//! interface (until AT.9); then ffmpeg is looked for and the intros are
//! indexed and transcoded. The roster lands first so a phone can place
//! figures while the rest runs, the models are announced as they finish
//! ([`Event::ModelsReady`], AE.13) so the turntable draws while the level
//! and the intros run, chapter 1 plays while the parts after it read (its
//! sounds taken up once their part is in, [`Event::SoundsReady`], AR.21),
//! each stage's result goes out as it ends ([`Event::Ended`], AT.31), and
//! the intros are the only thing a missing or unusable ffmpeg costs. A
//! model that won't read is reported and its
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
    /// `done` of `total` archives' emitters read or kept (AH.87).
    Particles {
        done: usize,
        total: usize,
    },
    /// Every part of chapter 1 the TV reads ([`levels::PLAYED`]) is in, and
    /// the script and emitter parts it loads with it have been through
    /// their stages (AT.26): the TV can load the level now, while the parts
    /// nothing draws yet read. Not sent when a part it reads didn't.
    LevelReady,
    /// `done` of `total` levels' sprites parts written, kept or reported
    /// (AT.8).
    Sprites {
        done: usize,
        total: usize,
    },
    /// `done` of `total` archives' sounds read or kept (AR.5).
    Sounds {
        done: usize,
        total: usize,
    },
    /// The sounds part is in, read or kept with nothing failed, and recorded
    /// (openspec AR.21): a level loaded before it can take it up now. Not
    /// sent for a part that didn't read.
    SoundsReady,
    /// `done` of `total` archives' interface read or kept (AT.8).
    Interface {
        done: usize,
        total: usize,
    },
    /// ffmpeg was found; mapping toy types to intro movies.
    Indexing,
    Transcode(TranscodeEvent),
    /// A stage is done, with what it did (openspec AT.31): sent the moment
    /// it ends, after its last progress event and before the next stage's
    /// first, so what it read, kept or failed is said then, not when the run
    /// returns after the intros. The run's [`Extracted`] holds the same.
    Ended(Box<StageEnd>),
}

/// A stage's result as it ends ([`Event::Ended`]). The intros have none:
/// they end the run, and [`Extracted::intros`] is theirs.
#[derive(Debug, Clone)]
pub enum StageEnd {
    Roster(RosterStage),
    Progression(ProgressionStage),
    Models(ModelStage),
    /// The parts of each level the TV reads ([`levels::PLAYED`]).
    Level(LevelStage),
    Scripts(ScriptStage),
    Particles(crate::particles::ParticleStage),
    /// Each level's sprites part (AT.8).
    Sprites(LevelStage),
    Sounds(crate::sounds::SoundStage),
    Interface(crate::interface::InterfaceStage),
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
    /// The part was in already (its record, the palettes and the nodes),
    /// and kept without opening an archive (openspec AT.30).
    pub kept: bool,
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
                SlotEntry::Clip { .. } | SlotEntry::Blocks { .. } => self.clips += 1,
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
    /// The level's own sounds, read with it on demand (openspec AR.30); none
    /// for a stage over the first run's levels, whose sounds are the
    /// extract-wide part's.
    pub sounds: Option<crate::sounds::SoundStage>,
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
    /// The walkers the scripts part holds (AG.27), and the load files it
    /// left out this run with why: each keeps his card.
    pub walkers: usize,
    pub walkers_left_out: Vec<(String, String)>,
    /// Spawn models read this run, and those refused.
    pub props: usize,
    pub refused_props: Vec<(String, String)>,
    /// Spawn rigs read this run, and those refused (openspec AU.37).
    pub rigs: usize,
    pub refused_rigs: Vec<(String, String)>,
    /// The screens' part (AS.53, [`crate::screens`]), counted apart from
    /// the two above: read this run, or kept; its entries written or kept,
    /// the archives the dump lacks with why, and its files (not in
    /// [`ScriptStage::files`]).
    pub screens_read: bool,
    pub screens_kept: bool,
    pub screens: usize,
    pub screens_missing: Vec<(String, String)>,
    pub screen_files: Files,
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
    /// The parts of each level the TV reads ([`levels::PLAYED`]).
    pub levels: LevelStage,
    pub scripts: ScriptStage,
    /// Each level's sprites part (AT.8), read after chapter 1's hand-off.
    pub sprites: LevelStage,
    /// The attacks' emitters (AH.87).
    pub particles: crate::particles::ParticleStage,
    /// The sounds chapter 1 can play (AR.5).
    pub sounds: crate::sounds::SoundStage,
    /// The shared, character and menu archives' interface (AT.8).
    pub interface: crate::interface::InterfaceStage,
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
    let ended = |end: StageEnd| send(Event::Ended(Box::new(end)));
    let roster = roster_stage(dir, ir, send)?;
    ended(StageEnd::Roster(roster.clone()));
    send(Event::RosterReady(roster.roster.clone()));
    let progression = progression_stage(dir, ir)?;
    ended(StageEnd::Progression(progression.clone()));
    if let Ok(p) = &progression.progression {
        send(Event::ProgressionReady(p.clone()));
    }
    let models = model_stage(dir, &roster.roster, ir, send)?;
    ended(StageEnd::Models(models.clone()));
    send(Event::ModelsReady {
        failed: models.failed.clone(),
    });
    let names = &levels::FIRST_RUN;
    let levels = levels_stage(dir, ir, names, &levels::PLAYED, &level_event, send)?;
    ended(StageEnd::Level(levels.clone()));
    let scripts = script_stage(dir, ir, send)?;
    ended(StageEnd::Scripts(scripts.clone()));
    let particles = crate::particles::particle_stage(dir, ir, send)?;
    ended(StageEnd::Particles(particles.clone()));
    // Whole: every one of its played parts in, for every level.
    if levels.levels == names.len() {
        send(Event::LevelReady);
    }
    let sprites = levels_stage(dir, ir, names, &[levels::SPRITES], &sprite_event, send)?;
    ended(StageEnd::Sprites(sprites.clone()));
    let sounds = crate::sounds::sound_stage(dir, &roster.roster, ir, send)?;
    ended(StageEnd::Sounds(sounds.clone()));
    // Read or kept, and recorded by the stage before it returned (AR.21).
    if sounds.failed.is_none() {
        send(Event::SoundsReady);
    }
    let interface = crate::interface::interface_stage(dir, ir, send)?;
    ended(StageEnd::Interface(interface.clone()));
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
        sprites,
        particles,
        sounds,
        interface,
        intros,
    })
}

/// The level stage's progress, as [`Event::Level`].
fn level_event(done: usize, total: usize) -> Event {
    Event::Level { done, total }
}

/// The sprites' progress, as [`Event::Sprites`].
fn sprite_event(done: usize, total: usize) -> Event {
    Event::Sprites { done, total }
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

/// Each level a first run extracts ([`levels::FIRST_RUN`]), part by part, into
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
    levels_stage(
        dir,
        ir,
        &levels::FIRST_RUN,
        &levels::PARTS,
        &level_event,
        send,
    )
}

/// One level off the disc's list ([`levels::on_disc`]), by its archive's
/// name, extracted as the first run's are (openspec AS.1, design decision
/// 1): its parts under `levels/<archive>/`, recorded under its name, each
/// kept when already in, its textures into the shared store; then, for a
/// level past the first run's, its own sounds (openspec AR.30,
/// [`crate::sounds::level_sound_stage`]), so its music plays from its load;
/// sounds that won't write either (a full disk) are failed as ones that
/// won't read are, nothing recorded, so the level, its parts in, plays
/// silent rather than being refused. A name the dump holds no level file of
/// is refused before anything is read.
pub fn extract_level(
    dir: &mut IdentityDir,
    level: &str,
    ir: IrFormat,
    send: &(dyn Fn(Event) + Sync),
) -> Result<LevelStage> {
    if !levels::on_disc(dir.manifest())
        .iter()
        .any(|l| l.name == level)
    {
        return Err(ExtractError::NoSuchLevel {
            level: level.to_owned(),
        });
    }
    let mut stage = levels_stage(dir, ir, &[level], &levels::PARTS, &level_event, send)?;
    if !levels::FIRST_RUN.contains(&level) {
        let sounds = crate::sounds::level_sound_stage(dir, level, ir, send).unwrap_or_else(|e| {
            crate::sounds::SoundStage {
                failed: Some((crate::sounds::level_archive(level), e.to_string())),
                ..Default::default()
            }
        });
        stage.sounds = Some(sounds);
    }
    Ok(stage)
}

/// [`level_stage`] over `names`, each level's `parts` alone (in
/// [`levels::PARTS`]' order), its progress sent through `event`;
/// [`LevelStage::levels`] counts the levels with every one of `parts` in.
fn levels_stage(
    dir: &mut IdentityDir,
    ir: IrFormat,
    names: &[&str],
    parts: &[&str],
    event: &dyn Fn(usize, usize) -> Event,
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
        sounds: None,
    };
    let total = names.len() * parts.len();
    let mut done = 0;
    send(event(done, total));
    // Every file counted, kept or written, and the textures the parts read
    // this run bind.
    let mut counted: HashSet<String> = HashSet::new();
    let mut read_textures: HashSet<String> = HashSet::new();
    for &level in names {
        let mut needed = Vec::new();
        for &part in parts {
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
                send(event(done, total));
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
        let unpacked = bytes
            .as_ref()
            .map_err(Clone::clone)
            .and_then(|b| levels::level_bld(b));
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
                send(event(done, total));
                continue;
            }
            let mut props = None;
            let palettes_started = Instant::now();
            let mut palettes = None;
            let read = igz.as_ref().map_err(Clone::clone).and_then(|igz| {
                let (id, rel) = (identity.clone(), levels::part_path(ir, level, part));
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
                    levels::COLLISION => {
                        levels::collision(igz, level, id).and_then(|r| encoded(ir, dir, &rel, r))
                    }
                    levels::PLACEMENTS => {
                        levels::placements(igz, level, id).and_then(|r| encoded(ir, dir, &rel, r))
                    }
                    levels::LIGHTS => {
                        levels::lights(igz, level, id).and_then(|r| encoded(ir, dir, &rel, r))
                    }
                    levels::ATMOSPHERE => {
                        levels::atmosphere(igz, level, id).and_then(|r| encoded(ir, dir, &rel, r))
                    }
                    levels::PALETTES => levels::palettes(igz, level, id).and_then(|r| {
                        let mut count = PaletteCount::default();
                        r.ir.palettes.iter().for_each(|p| count.add(p));
                        palettes = Some(count);
                        encoded(ir, dir, &rel, r)
                    }),
                    levels::NODES => {
                        levels::nodes(igz, level, id).and_then(|r| encoded(ir, dir, &rel, r))
                    }
                    levels::PARTICLES => {
                        levels::particles(igz, level, id).and_then(|r| encoded(ir, dir, &rel, r))
                    }
                    levels::SPRITES => {
                        let archive = format!("{}/{level}.bld", levels::LEVEL_FOLDER);
                        bytes
                            .as_ref()
                            .map_err(Clone::clone)
                            .and_then(|b| crate::interface::english_pack(b))
                            .and_then(|pack| {
                                crate::interface::read_igz(&archive, igz, pack.as_deref(), None, id)
                            })
                            .and_then(|r| {
                                let bytes =
                                    ir.encode(&r.ir, dir, &rel).map_err(|e| e.to_string())?;
                                Ok((bytes, r.textures))
                            })
                    }
                    other => Err(format!("no reader for a part named {other}")),
                }
            });
            match read {
                Ok((bytes, part_textures)) => {
                    let mut files = Vec::with_capacity(part_textures.len() + 1);
                    let mut new: Vec<(String, &[u8])> = Vec::new();
                    for (rel, ktx2) in &part_textures {
                        if read_textures.insert(rel.clone()) && store.contains(rel) {
                            stage.shared += 1;
                        }
                        if counted.insert(rel.clone()) && !held(dir, rel, ktx2, &mut stage.files)? {
                            new.push((rel.clone(), ktx2));
                        }
                        files.push(rel.clone());
                    }
                    // The part's new textures under one manifest save, not one
                    // each (openspec AS.26).
                    dir.write_outputs(&new)?;
                    let rel = levels::part_path(ir, level, part);
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
            send(event(done, total));
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
/// `spawns/<stem>.<ron|postcard>` and their textures into the store, then
/// the screens' entries (AS.53, [`crate::screens`]). A part
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

    // The walkers (AG.27): every modelled character but a fighter, from the
    // roster the stage before wrote; none with no roster to read.
    let loads: Vec<String> = match crate::roster::load(dir, ir) {
        Ok(roster) => scripts::walker_loads(&models::character_loads(&roster)),
        Err(_) => Vec::new(),
    };
    let base = scripts::archives().len();
    let archives = scripts::archives_with(&loads);
    // The part is held once it records every one of these walkers as
    // listed (its file in) or left out (its reason kept): a left-out one
    // isn't read again each start, a new one is.
    let prior = scripts::read_walkers(dir.path()).unwrap_or_default();
    let known = loads
        .iter()
        .all(|l| prior.listed.contains(l) || prior.left_out.iter().any(|(o, _)| o == l));
    let mut held: Vec<String> = scripts::archives_with(&prior.listed)
        .iter()
        .map(|a| scripts::script_path(a))
        .collect();
    held.push(scripts::WALKERS_FILE.to_owned());
    if known && dir.part_holds(scripts::SCRIPTS, &held)? {
        count_kept(dir, &held, &mut stage.script_files);
        stage.walkers = prior.listed.len();
        stage.walkers_left_out = prior.left_out;
        stage.parts_kept += 1;
    } else {
        // Whole or not at all: the entries load together. The base's
        // failing fails the part; a walker's is left out and listed.
        let mut checked = Vec::with_capacity(archives.len());
        let mut list = scripts::WalkerList::default();
        let mut failed = None;
        for (i, archive) in archives.iter().enumerate() {
            let walker = i.checked_sub(base).map(|w| &loads[w]);
            match entry(archive).and_then(|bytes| Ok((scripts::check(&bytes)?, bytes))) {
                Ok((names, bytes)) => {
                    list.listed.extend(walker.cloned());
                    checked.push((archive.clone(), names.len(), bytes));
                }
                Err(reason) if walker.is_some() => {
                    list.left_out.extend(walker.map(|w| (w.clone(), reason)));
                }
                Err(reason) => {
                    failed = Some((archive.clone(), reason));
                    break;
                }
            }
        }
        match failed {
            Some((archive, reason)) => fail(&mut stage, scripts::SCRIPTS, &archive, reason),
            None => {
                let mut recorded = Vec::with_capacity(checked.len() + 1);
                for (archive, n, bytes) in &checked {
                    let rel = scripts::script_path(archive);
                    put(dir, &rel, bytes, &mut stage.script_files)?;
                    recorded.push(rel);
                    stage.entries += 1;
                    stage.scripts += n;
                }
                let json = serde_json::to_vec_pretty(&list).map_err(|e| {
                    ExtractError::ScriptsUnreadable {
                        part: scripts::SCRIPTS.to_owned(),
                        archive: scripts::WALKERS_FILE.to_owned(),
                        reason: e.to_string(),
                    }
                })?;
                put(dir, scripts::WALKERS_FILE, &json, &mut stage.script_files)?;
                recorded.push(scripts::WALKERS_FILE.to_owned());
                dir.record_part(scripts::SCRIPTS, recorded)?;
                stage.walkers = list.listed.len();
                stage.walkers_left_out = list.left_out;
                stage.parts_read += 1;
            }
        }
    }
    send(Event::Scripts { done: 1, total });

    // Every walker the scripts part holds (openspec AU.1): what his attacks
    // spawn is in his own file.
    let listed = scripts::read_walkers(dir.path()).unwrap_or_default().listed;
    let archives = scripts::spawn_archives_with(&listed);
    let irs: Vec<String> = archives.iter().map(|a| ir.spawn_path(a)).collect();
    if dir.part_holds(scripts::SPAWNS, &irs)? {
        let recorded = dir.manifest().parts[scripts::SPAWNS].clone();
        count_kept(dir, &recorded, &mut stage.spawn_files);
        stage.parts_kept += 1;
    } else {
        let mut read = Vec::with_capacity(archives.len());
        let mut failed = None;
        for (archive, rel) in archives.iter().zip(&irs) {
            // `global.bld`'s models name its effects by object, read through
            // its and bootstrap's exports (AH.39).
            let boot = (archive.as_str() == crate::roster::GLOBAL)
                .then(|| entry(crate::roster::BOOTSTRAP));
            let one = entry(archive).and_then(|bytes| {
                let igz = guac_formats::igz::Igz::parse(&bytes).map_err(|e| e.to_string())?;
                let body = scripts::body(&igz, archive);
                let effects = match boot {
                    Some(b) => scripts::own_effects(archive, &igz, &b?)?,
                    None => None,
                };
                let r =
                    scripts::spawn_models(&igz, archive, identity.clone(), body, effects.as_ref())?;
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
    // What each character's skinned spawns play (openspec AU.37).
    let archives = scripts::rig_archives_with(&listed);
    let irs: Vec<String> = archives.iter().map(|a| ir.spawn_rig_path(a)).collect();
    if dir.part_holds(scripts::SPAWN_RIGS, &irs)? {
        let recorded = dir.manifest().parts[scripts::SPAWN_RIGS].clone();
        count_kept(dir, &recorded, &mut stage.spawn_files);
        stage.parts_kept += 1;
    } else {
        let mut read = Vec::with_capacity(archives.len());
        let mut failed = None;
        for (archive, rel) in archives.iter().zip(&irs) {
            let one = entry(archive).and_then(|bytes| {
                let igz = guac_formats::igz::Igz::parse(&bytes).map_err(|e| e.to_string())?;
                let body = scripts::body(&igz, archive);
                let r = scripts::spawn_rigs(&igz, archive, identity.clone(), body)?;
                let refused: Vec<(String, String)> = r
                    .refused
                    .iter()
                    .map(|p| (format!("{archive}: {}", p.path), p.reason.clone()))
                    .collect();
                let rigs = r.rigs.len();
                let bytes = ir.encode(&r, dir, rel).map_err(|e| e.to_string())?;
                Ok((rigs, refused, bytes))
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
            Some((archive, reason)) => fail(&mut stage, scripts::SPAWN_RIGS, &archive, reason),
            None => {
                let mut files = Vec::with_capacity(irs.len());
                for ((rigs, refused, bytes), rel) in read.into_iter().zip(&irs) {
                    put(dir, rel, &bytes, &mut stage.spawn_files)?;
                    files.push(rel.clone());
                    stage.rigs += rigs;
                    stage.refused_rigs.extend(refused);
                }
                dir.record_part(scripts::SPAWN_RIGS, files)?;
                stage.parts_read += 1;
            }
        }
    }
    screen_part(dir, &usrdir, &mut stage)?;
    send(Event::Scripts { done: 2, total });
    for f in [stage.script_files, stage.spawn_files] {
        stage.files.written += f.written;
        stage.files.kept += f.kept;
        stage.files.bytes += f.bytes;
    }
    stage.took = started.elapsed();
    Ok(stage)
}

/// The screens' part (AS.53, [`crate::screens`]): kept while it's recorded
/// with every file it names intact (an archive the dump lacks recorded as
/// none), else each archive's entry read, checked and written, and the part
/// recorded; one that won't read is the part's failure alone.
pub fn screen_part(dir: &mut IdentityDir, usrdir: &Path, stage: &mut ScriptStage) -> Result<()> {
    let recorded = dir.manifest().parts.get(crate::screens::PART).cloned();
    if let Some(files) = recorded
        && dir.part_holds(crate::screens::PART, &files)?
    {
        count_kept(dir, &files, &mut stage.screen_files);
        stage.screens = files.len();
        stage.screens_kept = true;
        return Ok(());
    }
    match crate::screens::read(usrdir) {
        Ok(read) => {
            let mut files = Vec::with_capacity(read.entries.len());
            for (archive, bytes, _) in &read.entries {
                let rel = crate::screens::path(archive);
                put(dir, &rel, bytes, &mut stage.screen_files)?;
                files.push(rel);
            }
            stage.screens = files.len();
            stage.screens_missing = read.missing;
            dir.record_part(crate::screens::PART, files)?;
            stage.screens_read = true;
        }
        Err((archive, reason)) => {
            let e = ExtractError::ScriptsUnreadable {
                part: crate::screens::PART.to_owned(),
                archive: archive.clone(),
                reason,
            };
            stage.failed.push(PartFailure {
                part: crate::screens::PART.to_owned(),
                archive,
                reason: e.to_string(),
            });
        }
    }
    Ok(())
}

/// `files`, each already in, counted as kept with its recorded size.
pub(crate) fn count_kept(dir: &IdentityDir, files: &[String], into: &mut Files) {
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
/// `heroes/<load file>.<ron|postcard>`, its palette into
/// `palettes/<load file>.<ron|postcard>` and its attach nodes into
/// `nodes/<load file>.<ron|postcard>`, each model's new files under one
/// manifest save (AS.26's batching, openspec AT.30), then `global.bld`'s
/// rig into `ir/rig.<ron|postcard>` and the stage's record into
/// `ir/models.<ron|postcard>` ([`models::StageRecord`]), the palettes',
/// nodes' and models' parts recorded once every model has been through. A
/// start whose extract holds all three parts as recorded, its record naming
/// the roster's load files, keeps them without opening an archive and
/// reports the record's models and failures. A model or rig that won't read
/// is reported in [`ModelStage::failed`], a hero that doesn't settle in
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
        kept: false,
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
    send(Event::Models { done: 0, total });
    let record_rel = ir.model_record_path();
    if dir.part_holds(models::PART, &[record_rel.to_owned()])?
        && dir.part_holds(models::PALETTES, &[])?
        && dir.part_holds(models::NODES, &[])?
        && let Ok(record) = ir.decode::<models::StageRecord>(dir, record_rel)
        && record.loads == loads
    {
        let failure = |(load_file, reason): (String, String)| ModelFailure { load_file, reason };
        for part in [models::PART, models::NODES] {
            let recorded = dir.manifest().parts[part].clone();
            count_kept(dir, &recorded, &mut stage.files);
        }
        let recorded = dir.manifest().parts[models::PALETTES].clone();
        count_kept(dir, &recorded, &mut stage.palettes.files);
        let [palettes, clips, networks, refused] = record.palettes;
        (stage.palettes.palettes, stage.palettes.clips) = (palettes, clips);
        (stage.palettes.networks, stage.palettes.refused) = (networks, refused);
        stage.kept = true;
        stage.models = record.models;
        stage.failed = record.failed.into_iter().map(failure).collect();
        stage.heroes = record.heroes;
        stage.unsettled = record.unsettled.into_iter().map(failure).collect();
        (stage.textures, stage.binds) = (record.textures, record.binds);
        stage.took = started.elapsed();
        send(Event::Models { done: total, total });
        return Ok(stage);
    }
    let mut written: HashSet<String> = HashSet::new();
    let mut files: Vec<String> = Vec::new();
    let mut palette_files = Vec::new();
    let mut node_files = Vec::new();
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
    for (i, load) in loads.iter().enumerate() {
        let path = models::archive_path(&usrdir, load);
        let extracted = models::archive(&path, load)
            .and_then(|bytes| models::level(&bytes, load))
            .and_then(|level| models::read(&level, load, identity.clone(), globals.as_ref()));
        match extracted {
            Ok(m) => {
                stage.binds += m.binds;
                // The model's files not already in, under one manifest
                // save (AS.26's batching).
                let mut new: Vec<(String, &[u8])> = Vec::new();
                for (rel, ktx2) in &m.textures {
                    if written.insert(rel.clone()) {
                        files.push(rel.clone());
                        if !held(dir, rel, ktx2, &mut stage.files)? {
                            new.push((rel.clone(), ktx2));
                        }
                    }
                }
                let model_rel = ir.model_path(load);
                let model = ir.encode(&m.model, dir, &model_rel)?;
                if !held(dir, &model_rel, &model, &mut stage.files)? {
                    new.push((model_rel.clone(), &model));
                }
                files.push(model_rel);
                stage.models += 1;
                let started = Instant::now();
                let hero = match &m.hero {
                    Ok(hero) => {
                        let rel = ir.hero_path(load);
                        let bytes = ir.encode(hero, dir, &rel)?;
                        stage.heroes += 1;
                        Some((rel, bytes))
                    }
                    Err(reason) => {
                        stage.unsettled.push(ModelFailure {
                            load_file: (*load).to_owned(),
                            reason: reason.clone(),
                        });
                        None
                    }
                };
                if let Some((rel, bytes)) = &hero {
                    if !held(dir, rel, bytes, &mut stage.files)? {
                        new.push((rel.clone(), bytes));
                    }
                    files.push(rel.clone());
                }
                stage.heroes_took += m.hero_took + started.elapsed();
                let started = Instant::now();
                let palette_rel = ir.palette_path(load);
                let palette = ir.encode(&m.palette, dir, &palette_rel)?;
                if !held(dir, &palette_rel, &palette, &mut stage.palettes.files)? {
                    new.push((palette_rel.clone(), &palette));
                }
                stage.palettes.add(&m.palette);
                stage.palettes.took += m.palette_took + started.elapsed();
                palette_files.push(palette_rel);
                let nodes_rel = ir.nodes_path(load);
                let nodes = ir.encode(&m.nodes, dir, &nodes_rel)?;
                if !held(dir, &nodes_rel, &nodes, &mut stage.files)? {
                    new.push((nodes_rel.clone(), &nodes));
                }
                node_files.push(nodes_rel);
                dir.write_outputs(&new)?;
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
    dir.record_part(models::NODES, node_files)?;
    let rig = std::fs::read(usrdir.join(roster::GLOBAL))
        .map_err(ExtractError::io(usrdir.join(roster::GLOBAL)))
        .and_then(|bytes| models::level(&bytes, roster::GLOBAL))
        .and_then(|level| models::rig(&level, identity));
    match rig {
        Ok(rig) => {
            let bytes = ir.encode(&rig, dir, ir.rig_path())?;
            put(dir, ir.rig_path(), &bytes, &mut stage.files)?;
            files.push(ir.rig_path().to_owned());
        }
        Err(e @ ExtractError::ModelUnreadable { .. }) => stage.failed.push(ModelFailure {
            load_file: "ir/rig".into(),
            reason: e.to_string(),
        }),
        Err(e) => return Err(e),
    }
    // The record last, then the part: a stage that stopped before it is
    // read again whole by the next start, every file it wrote kept.
    let failures = |f: &[ModelFailure]| {
        f.iter()
            .map(|f| (f.load_file.clone(), f.reason.clone()))
            .collect()
    };
    let p = &stage.palettes;
    let record = models::StageRecord {
        loads: loads.iter().map(|l| (*l).to_owned()).collect(),
        models: stage.models,
        failed: failures(&stage.failed),
        heroes: stage.heroes,
        unsettled: failures(&stage.unsettled),
        textures: stage.textures,
        binds: stage.binds,
        palettes: [p.palettes, p.clips, p.networks, p.refused],
    };
    let bytes = ir.encode(&record, dir, record_rel)?;
    put(dir, record_rel, &bytes, &mut stage.files)?;
    files.push(record_rel.to_owned());
    dir.record_part(models::PART, files)?;
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
pub(crate) fn put(dir: &mut IdentityDir, rel: &str, bytes: &[u8], files: &mut Files) -> Result<()> {
    if !held(dir, rel, bytes, files)? {
        dir.write_output(rel, bytes)?;
    }
    Ok(())
}

/// Whether the extract already holds exactly `bytes` at `rel` (recorded, and
/// the file still hashes to the record), counted in `files` as kept or as
/// one to write; writing it is the caller's.
pub(crate) fn held(dir: &IdentityDir, rel: &str, bytes: &[u8], files: &mut Files) -> Result<bool> {
    let sha = Sha256(sha2::Sha256::digest(bytes).into());
    let size = bytes.len() as u64;
    let recorded = dir
        .manifest()
        .outputs
        .iter()
        .any(|o| o.path == rel && (o.size, o.sha256) == (size, sha));
    let held = recorded && dir.output_done(rel)?;
    if held {
        files.kept += 1;
    } else {
        files.written += 1;
    }
    files.bytes += size;
    Ok(held)
}
