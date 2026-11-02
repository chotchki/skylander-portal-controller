//! The sounds chapter 1 can play (openspec AR.5): for `global.bld`, each
//! modelled character's archive and each level the build plays, every
//! `tfbSoundInfo` of its `level.bld` and every `tfbSoundDataInfo` of its
//! `level.bld` and `ENGLISH.pak`, through guac-formats' [`sound`] reader,
//! into `sounds/<stem>.<ron|postcard>`, and each data info's bank remuxed
//! to `audio/sounds/<sha256 prefix>.mp3` (a stream from the archive's
//! `.arc`, English only). Every bank on the disc is MPEG (AR.2), so no
//! ffmpeg is looked for. An extract-wide part of its own ([`PART`]), so an
//! extract made before it resumes for it alone; the audio is named by its
//! bytes, so a bank two archives share is one file.
//!
//! A character archive the dump lacks is listed and the part lands without
//! it (its figure plays no sounds, as a missing model keeps its card); one
//! that's there and won't read refuses the part, and the other stages land.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use guac_formats::exports::Exports;
use guac_formats::fsb::{Bank, Codec};
use guac_formats::iga::Archive;
use guac_formats::igz::Igz;
use guac_formats::mpeg;
use guac_formats::sound::{self, Streams};
use guac_ir::sound::{Audio, DataHome, DataRef, Sound, SoundData, Sounds};
use guac_ir::{DumpIdentity, Roster};
use sha2::Digest;

use crate::intros::IrFormat;
use crate::stages::{Event, Files, count_kept, held, put};
use crate::{ExtractError, IdentityDir, Result, levels, models, roster, scripts};

/// The extract-wide part ([`crate::PARTS`]).
pub const PART: &str = "sounds";
/// The folder each archive's IR file sits in.
pub const DIR: &str = "sounds";
/// The audio, each file named by its bytes' hash.
pub const AUDIO_DIR: &str = "audio/sounds";
/// The English pack's entry in an archive.
pub const ENGLISH_PACK: &str = "ENGLISH.pak";

/// `sounds/<stem>.<ron|postcard>`.
pub fn path(ir: IrFormat, archive: &str) -> String {
    let ext = match ir {
        IrFormat::Ron => "ron",
        IrFormat::Postcard => "postcard",
    };
    format!("{DIR}/{}.{ext}", scripts::stem(archive))
}

/// The archives the part reads, under `PS3_GAME/USRDIR`: `global.bld`,
/// each modelled character's in the roster's order, each level's.
pub fn archives(roster: &Roster) -> Vec<String> {
    std::iter::once(roster::GLOBAL.to_owned())
        .chain(
            models::character_loads(roster)
                .into_iter()
                .map(scripts::character_archive),
        )
        .chain(
            levels::LEVELS
                .iter()
                .map(|l| format!("{}/{l}.bld", levels::LEVEL_FOLDER)),
        )
        .collect()
}

/// `audio/sounds/<first 16 hex of the sha256>.mp3` for `bytes`.
pub fn audio_path(bytes: &[u8]) -> String {
    let sha = sha2::Sha256::digest(bytes);
    let hex: String = sha[..8].iter().map(|b| format!("{b:02x}")).collect();
    format!("{AUDIO_DIR}/{hex}.mp3")
}

/// One archive read: its IR, and its audio by path, each bank once.
#[derive(Debug, Clone, PartialEq)]
pub struct Read {
    pub ir: Sounds,
    pub audio: Vec<(String, Vec<u8>)>,
}

/// A bank's one MPEG sample remuxed, or why it isn't one.
pub fn remux(bank: &[u8]) -> std::result::Result<(Audio, Vec<u8>), String> {
    let bank = Bank::parse(bank).map_err(|e| e.to_string())?;
    if bank.codec != Codec::Mpeg {
        return Err(format!("codec {:?}, not MPEG", bank.codec));
    }
    let [s] = &bank.samples[..] else {
        return Err(format!("{} samples, not one", bank.samples.len()));
    };
    let r = mpeg::remux(s.data, s.samples).map_err(|e| e.to_string())?;
    if r.samples != u64::from(s.samples) {
        return Err(format!(
            "frames hold {} samples, the bank {}",
            r.samples, s.samples
        ));
    }
    let file = audio_path(&r.bytes);
    Ok((
        Audio {
            file,
            rate: s.rate,
            channels: s.channels,
            samples: s.samples,
            loop_points: s.loop_points,
        },
        r.bytes,
    ))
}

/// The archive's IGZ: `level.bld` and, if it has one, `ENGLISH.pak`.
fn igz_entries(bytes: &[u8]) -> std::result::Result<(Vec<u8>, Option<Vec<u8>>), String> {
    let arc = Archive::parse(bytes).map_err(|e| e.to_string())?;
    let read = |name: &str| -> std::result::Result<Option<Vec<u8>>, String> {
        arc.find(name)
            .map(|e| {
                arc.read(e)
                    .map(|d| d.into_owned())
                    .map_err(|e| e.to_string())
            })
            .transpose()
    };
    let level = read(roster::LEVEL)?.ok_or("no level.bld")?;
    Ok((level, read(ENGLISH_PACK)?))
}

/// Reads one archive's sounds: `bld` is its `.bld`, `arc` its `.arc` (if
/// the dump has one), `exports` `global.bld`'s. A data info whose bank
/// doesn't read, and a sound that doesn't, are listed in the IR's
/// `refused` and the rest land; the error is an archive that won't read.
pub fn read(
    archive: &str,
    bld: &[u8],
    arc: Option<&[u8]>,
    exports: &Exports,
    identity: DumpIdentity,
) -> std::result::Result<Read, String> {
    let (level, pack) = igz_entries(bld)?;
    let level = Igz::parse(&level).map_err(|e| format!("level.bld: {e}"))?;
    let pack = pack
        .as_deref()
        .map(Igz::parse)
        .transpose()
        .map_err(|e| format!("{ENGLISH_PACK}: {e}"))?;
    let streams = arc
        .map(|a| {
            let parsed = Archive::parse(a).map_err(|e| format!(".arc: {e}"))?;
            let streams = Streams::new(parsed.entries().iter().map(|e| (e.index(), e.name())));
            Ok::<_, String>((parsed, streams))
        })
        .transpose()?;
    let mut out = Sounds {
        identity,
        archive: archive.to_owned(),
        sounds: Vec::new(),
        data: Vec::new(),
        refused: Vec::new(),
    };
    let mut audio: Vec<(String, Vec<u8>)> = Vec::new();
    let mut seen: HashSet<String> = HashSet::new();
    let homes = [
        (DataHome::Level, Some(&level)),
        (DataHome::Pack, pack.as_ref()),
    ];
    for (home, igz) in homes {
        let Some(igz) = igz else { continue };
        for (object, d) in sound::data_infos(igz) {
            let d = match d {
                Ok(d) => d,
                Err(e) => {
                    out.refused
                        .push((format!("{home:?} data info #{object}"), e.to_string()));
                    continue;
                }
            };
            let name = d.name().map(str::to_owned);
            let label = name
                .clone()
                .unwrap_or_else(|| format!("{home:?} data info #{object}"));
            let (streamed, bank): (Option<i32>, std::result::Result<Vec<u8>, String>) =
                match &d.bank {
                    Some(sound::Bank::Embedded(b)) => (None, Ok(b.to_vec())),
                    Some(sound::Bank::Streamed { path, hash }) => (
                        Some(*hash),
                        match &streams {
                            None => Err("no .arc beside the archive".into()),
                            Some((parsed, streams)) => streams
                                .find(path)
                                .map_err(|m| format!("its stream: {m:?}"))
                                .and_then(|i| {
                                    parsed
                                        .read(&parsed.entries()[i])
                                        .map(|b| b.into_owned())
                                        .map_err(|e| e.to_string())
                                }),
                        },
                    ),
                    None => (None, Err("neither data nor a name to stream by".into())),
                };
            let audio_of = bank.and_then(|b| remux(&b));
            let kept = match audio_of {
                Ok((a, bytes)) => {
                    if seen.insert(a.file.clone()) {
                        audio.push((a.file.clone(), bytes));
                    }
                    Some(a)
                }
                Err(why) => {
                    out.refused.push((label, why));
                    None
                }
            };
            out.data.push(SoundData {
                home,
                object: object as u32,
                name,
                raw_duration: d.info.raw_duration,
                streamed,
                lip_sync: d.info.viseme_stream != guac_formats::igz::layouts::ObjectRef::Null,
                audio: kept,
            });
        }
    }
    for (object, s) in sound::sounds(&level) {
        let s = match s {
            Ok(s) => s,
            Err(e) => {
                out.refused
                    .push((format!("sound #{object}"), e.to_string()));
                continue;
            }
        };
        let data = match s.data {
            sound::DataRef::None => DataRef::None,
            sound::DataRef::Local(o) => DataRef::Here(DataHome::Level, o as u32),
            sound::DataRef::Pack(id) => pack
                .as_ref()
                .and_then(|p| sound::resolve_pack(p, id))
                .map_or(DataRef::Unresolved, |o| {
                    DataRef::Here(DataHome::Pack, o as u32)
                }),
            sound::DataRef::Global(id) => sound::resolve_global(exports, id)
                .map_or(DataRef::Unresolved, |o| DataRef::Global(o as u32)),
            sound::DataRef::Other => DataRef::Unresolved,
        };
        let i = &s.info;
        out.sounds.push(Sound {
            object: object as u32,
            name: i.name.map(str::to_owned),
            data,
            flags: i.flags,
            properties: i.properties,
            playback_mode: i.playback_mode,
            playback_percent: i.playback_percent,
            volume_percent: i.volume_percent,
            volume_randomness_percent: i.volume_randomness_percent,
            pitch_randomness_percent: i.pitch_randomness_percent,
            pan_percent: i.pan_percent,
            attenuation_distance_full: i.attenuation_distance_full,
            attenuation_distance_silent: i.attenuation_distance_silent,
            rumble_duration: i.rumble_duration,
            reverb_amount: i.reverb_amount,
            reverb_randomness_percent: i.reverb_randomness_percent,
            filter_percent: i.filter_percent,
            filter_randomness_percent: i.filter_randomness_percent,
            filter_attenuation_distance_minimum: i.filter_attenuation_distance_minimum,
            filter_attenuation_distance_maximum: i.filter_attenuation_distance_maximum,
            filter_maximum_percent: i.filter_maximum_percent,
            sample_rate_reduction: i.sample_rate_reduction,
        });
    }
    // Both lists come out in the order the IR keeps (`level.bld`'s data
    // before the pack's, each in object order): the reader walks objects in
    // order.
    Ok(Read { ir: out, audio })
}

/// `global.bld`'s export ids, off `bootstrap.bld`'s and `global.bld`'s
/// root lists.
pub fn exports(usrdir: &Path) -> std::result::Result<Exports, String> {
    let boot = scripts::entry(usrdir, roster::BOOTSTRAP)?;
    let global = scripts::entry(usrdir, roster::GLOBAL)?;
    let boot = Igz::parse(&boot).map_err(|e| format!("bootstrap.bld: {e}"))?;
    let global = Igz::parse(&global).map_err(|e| format!("global.bld: {e}"))?;
    Exports::new(&boot, &global).map_err(|e| e.to_string())
}

/// What the part did.
#[derive(Debug, Clone, Default)]
pub struct SoundStage {
    /// The part was in already, and kept.
    pub kept: bool,
    /// Read this run.
    pub read: bool,
    /// The archive that stopped the part, and why.
    pub failed: Option<(String, String)>,
    /// Character archives the dump lacks: their figures play no sounds.
    pub missing: Vec<String>,
    /// Archives read, sounds and data infos in them, distinct audio files.
    pub archives: usize,
    pub sounds: usize,
    pub data: usize,
    pub audio: usize,
    /// What didn't read, by archive.
    pub refused: Vec<(String, String)>,
    /// Of [`SoundStage::files`], the audio's.
    pub audio_files: Files,
    pub files: Files,
    pub took: Duration,
}

/// The part, unless the extract already holds it
/// ([`IdentityDir::part_holds`]): each archive's sounds into
/// `sounds/<stem>.<ron|postcard>` and their audio into `audio/sounds/`,
/// recorded once every archive's is in. A file that won't write stops the
/// run. Its progress goes out as [`Event::Sounds`]: none of the archives
/// the dump holds done, then each as it lands (all at once when the part is
/// kept), openspec AT.26.
pub fn sound_stage(
    dir: &mut IdentityDir,
    roster: &Roster,
    ir: IrFormat,
    send: &(dyn Fn(Event) + Sync),
) -> Result<SoundStage> {
    let started = Instant::now();
    let usrdir = PathBuf::from(&dir.manifest().source_dir).join("PS3_GAME/USRDIR");
    let identity = dir.manifest().identity.clone();
    let mut stage = SoundStage::default();
    // A character archive the dump lacks isn't read, its figure silent.
    let (present, missing): (Vec<String>, Vec<String>) = archives(roster)
        .into_iter()
        .partition(|a| usrdir.join(a).is_file());
    stage.missing = missing;
    let total = present.len();
    send(Event::Sounds { done: 0, total });
    let irs: Vec<String> = present.iter().map(|a| path(ir, a)).collect();
    if dir.part_holds(PART, &irs)? {
        let recorded = dir.manifest().parts[PART].clone();
        count_kept(dir, &recorded, &mut stage.files);
        stage.kept = true;
        stage.took = started.elapsed();
        send(Event::Sounds { done: total, total });
        return Ok(stage);
    }
    let fail = |stage: &mut SoundStage, archive: &str, reason: String| {
        let e = ExtractError::ScriptsUnreadable {
            part: PART.to_owned(),
            archive: archive.to_owned(),
            reason,
        };
        stage.failed = Some((archive.to_owned(), e.to_string()));
        stage.took = started.elapsed();
    };
    let exports = match exports(&usrdir) {
        Ok(x) => x,
        Err(reason) => {
            fail(&mut stage, roster::GLOBAL, reason);
            return Ok(stage);
        }
    };
    // The audio already in, by path: recorded and on disk with the bytes its
    // record hashes, as `put` keeps any other output. By size alone, a file
    // damaged at its own size would be kept, and the part, never held with
    // it, read again at every start.
    let mut done: HashSet<String> = dir
        .manifest()
        .outputs
        .iter()
        .filter(|o| o.path.starts_with(AUDIO_DIR))
        .filter(|o| {
            std::fs::read(dir.path().join(&o.path)).is_ok_and(|b| {
                b.len() as u64 == o.size && sha2::Sha256::digest(&b)[..] == o.sha256.0[..]
            })
        })
        .map(|o| o.path.clone())
        .collect();
    let mut files: Vec<String> = Vec::new();
    let mut part_audio: HashSet<String> = HashSet::new();
    for (i, (archive, rel)) in present.iter().zip(&irs).enumerate() {
        let bld_path = usrdir.join(archive);
        let bld = std::fs::read(&bld_path).map_err(ExtractError::io(&bld_path))?;
        let arc_path = bld_path.with_extension("arc");
        let arc = match std::fs::read(&arc_path) {
            Ok(b) => Some(b),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
            Err(e) => return Err(ExtractError::io(&arc_path)(e)),
        };
        let r = match read(archive, &bld, arc.as_deref(), &exports, identity.clone()) {
            Ok(r) => r,
            Err(reason) => {
                fail(&mut stage, archive, reason);
                return Ok(stage);
            }
        };
        let mut new: Vec<(String, &[u8])> = Vec::new();
        for (file, bytes) in &r.audio {
            if part_audio.insert(file.clone()) {
                files.push(file.clone());
                stage.audio_files.bytes += bytes.len() as u64;
                if done.contains(file) {
                    stage.audio_files.kept += 1;
                } else {
                    new.push((file.clone(), bytes));
                }
            }
        }
        stage.audio_files.written += new.len();
        dir.write_outputs(&new)?;
        done.extend(new.iter().map(|(f, _)| f.clone()));
        let bytes = ir.encode(&r.ir, dir, rel)?;
        put(dir, rel, &bytes, &mut stage.files)?;
        files.push(rel.clone());
        stage.archives += 1;
        stage.sounds += r.ir.sounds.len();
        stage.data += r.ir.data.len();
        stage.refused.extend(
            r.ir.refused
                .iter()
                .map(|(n, why)| (format!("{archive}: {n}"), why.clone())),
        );
        send(Event::Sounds { done: i + 1, total });
    }
    stage.audio = part_audio.len();
    stage.files.written += stage.audio_files.written;
    stage.files.kept += stage.audio_files.kept;
    stage.files.bytes += stage.audio_files.bytes;
    dir.record_part(PART, files)?;
    stage.read = true;
    stage.took = started.elapsed();
    Ok(stage)
}

/// A level's archive under `PS3_GAME/USRDIR`: `level/<level>.bld`.
pub fn level_archive(level: &str) -> String {
    format!("{}/{level}.bld", levels::LEVEL_FOLDER)
}

/// One level's own sounds, read as the part reads chapter 1's (openspec
/// AR.30), into `sounds/<level>.<ron|postcard>` and `audio/sounds/`,
/// recorded as the level's `sounds` part in the manifest's `levels` map, so
/// the TV's book finds them by the level's stem at its load; kept when in.
/// Its `global.bld` refs lean on the extract-wide part, and a first-run
/// level's sounds are that part's, never this stage's. An archive that
/// won't read is [`SoundStage::failed`], nothing recorded: the level plays
/// silent and the next ask tries again.
pub fn level_sound_stage(
    dir: &mut IdentityDir,
    level: &str,
    ir: IrFormat,
    send: &(dyn Fn(Event) + Sync),
) -> Result<SoundStage> {
    let started = Instant::now();
    let archive = level_archive(level);
    let rel = path(ir, &archive);
    let mut stage = SoundStage::default();
    send(Event::Sounds { done: 0, total: 1 });
    if dir.level_part_done(level, PART, &rel)? {
        let recorded = dir.manifest().levels[level].parts[PART].clone();
        count_kept(dir, &recorded, &mut stage.files);
        stage.audio = recorded.iter().filter(|f| f.starts_with(AUDIO_DIR)).count();
        stage.kept = true;
        stage.archives = 1;
        stage.took = started.elapsed();
        send(Event::Sounds { done: 1, total: 1 });
        return Ok(stage);
    }
    let usrdir = PathBuf::from(&dir.manifest().source_dir).join("PS3_GAME/USRDIR");
    let identity = dir.manifest().identity.clone();
    let failed = |stage: &mut SoundStage, archive: &str, reason: String| {
        let e = ExtractError::ScriptsUnreadable {
            part: PART.to_owned(),
            archive: archive.to_owned(),
            reason,
        };
        stage.failed = Some((archive.to_owned(), e.to_string()));
        stage.took = started.elapsed();
    };
    let exports = match exports(&usrdir) {
        Ok(x) => x,
        Err(reason) => {
            failed(&mut stage, roster::GLOBAL, reason);
            return Ok(stage);
        }
    };
    let bld_path = usrdir.join(&archive);
    let bld = std::fs::read(&bld_path).map_err(ExtractError::io(&bld_path))?;
    let arc_path = bld_path.with_extension("arc");
    let arc = match std::fs::read(&arc_path) {
        Ok(b) => Some(b),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
        Err(e) => return Err(ExtractError::io(&arc_path)(e)),
    };
    let r = match read(&archive, &bld, arc.as_deref(), &exports, identity) {
        Ok(r) => r,
        Err(reason) => {
            failed(&mut stage, &archive, reason);
            return Ok(stage);
        }
    };
    let mut files: Vec<String> = Vec::with_capacity(r.audio.len() + 1);
    let mut new: Vec<(String, &[u8])> = Vec::new();
    for (file, bytes) in &r.audio {
        files.push(file.clone());
        if !held(dir, file, bytes, &mut stage.audio_files)? {
            new.push((file.clone(), bytes));
        }
    }
    // The level's new audio under one manifest save, as the part's.
    dir.write_outputs(&new)?;
    let bytes = ir.encode(&r.ir, dir, &rel)?;
    put(dir, &rel, &bytes, &mut stage.files)?;
    files.push(rel);
    dir.record_level_part(level, PART, files)?;
    stage.archives = 1;
    stage.sounds = r.ir.sounds.len();
    stage.data = r.ir.data.len();
    stage.audio = r.audio.len();
    stage.refused =
        r.ir.refused
            .iter()
            .map(|(n, why)| (format!("{archive}: {n}"), why.clone()))
            .collect();
    stage.files.written += stage.audio_files.written;
    stage.files.kept += stage.audio_files.kept;
    stage.files.bytes += stage.audio_files.bytes;
    stage.read = true;
    stage.took = started.elapsed();
    send(Event::Sounds { done: 1, total: 1 });
    Ok(stage)
}

#[cfg(test)]
mod tests;
