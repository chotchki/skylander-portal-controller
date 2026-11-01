//! The original's sounds in play, the game's half (openspec AR.9, AR.10):
//! the extract's sounds part loaded into `guac-sound`'s book at the level's
//! load, each VM unit mapped to the archive whose sounds it names, and what a
//! play is to the scripts. The TV's player (the binary's `sound_player`)
//! makes it heard; nothing here touches an output device.
//!
//! A script plays a sound as it plays a clip, by a `start sequence` on the
//! sound info: the host makes a sequence of it on the actor that started it
//! ([`crate::scripts::ScriptWorld`]), whose length is the recording's own
//! ([`SoundEnd`]), on the game clock: it advances with the engine's frame,
//! so it holds while the level is held, and a play's end the scripts wait on
//! is never the output's draining (a closed laptop's default output drains
//! about ten times slower than it plays, AQ.167). Its cosmetic draws (volume
//! and pitch randomness) are the player's, never the session's generator's,
//! and never move the play's length: nothing the scripts read depends on
//! them (AH.87's rule).

use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, Instant};

use guac_extract::intros::IrFormat;
use guac_ir::sound::{Audio, Sounds};
use guac_motion::seam::Vec3;
use guac_sound::{Params, Playable, SoundBook, SoundId};

/// Which length a sound's play lasts on the game clock (openspec AR.12):
/// the recording's frames as remuxed (its samples at its rate), or the
/// length the build measured (`rumble duration`'s number), which the frames
/// outrun by 0 to 3.23 s in chapter 1's part.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SoundEnd {
    Frames,
    Built,
}

/// The end a play keeps (openspec AR.12 reads which).
pub const PLAY_END: SoundEnd = SoundEnd::Frames;

/// A recording's length in seconds by `end`.
pub fn play_length(end: SoundEnd, audio: &Audio, built: f32) -> f32 {
    match end {
        SoundEnd::Frames if audio.rate > 0 => audio.samples as f32 / audio.rate as f32,
        _ => built.max(0.0),
    }
}

/// The archive stem a linked unit's sounds are under, by the unit's file in
/// the extract: `scripts/<stem>.igz` (global, a character, bootstrap,
/// init_setup) or `levels/<level>/script.igz`. `None` for any other name
/// (the headless runs' units, named for logs only).
pub fn unit_stem(unit: &str) -> Option<String> {
    let script_dir = format!("{}/", guac_extract::scripts::SCRIPT_DIR);
    if let Some(rest) = unit.strip_prefix(&script_dir) {
        return rest
            .strip_suffix(".igz")
            .filter(|s| !s.is_empty() && !s.contains('/'))
            .map(str::to_ascii_lowercase);
    }
    let level_dir = format!("{}/", guac_extract::levels::LEVEL_DIR);
    let rest = unit.strip_prefix(&level_dir)?;
    let (level, file) = rest.split_once('/')?;
    (file == format!("{}.igz", guac_extract::levels::SCRIPT) && !level.is_empty())
        .then(|| level.to_ascii_lowercase())
}

/// The sounds part as the level's load reads it: the book, each unit's
/// stem, the units whose stem has no file there (bootstrap's and
/// init_setup's never do) and what reading took.
#[derive(Debug, Clone, Default)]
pub struct SoundPart {
    pub book: Arc<SoundBook>,
    /// Each unit whose archive the book holds sounds of, by unit.
    pub stems: HashMap<u16, String>,
    /// Each unit's stem the part has no file of, or whose file didn't read,
    /// with why.
    pub missing: Vec<(String, String)>,
    pub read: Duration,
}

impl SoundPart {
    /// Sounds in the book, every archive's.
    pub fn sounds(&self) -> usize {
        self.book
            .stems()
            .filter_map(|s| self.book.file(s))
            .map(|f| f.sounds.len())
            .sum()
    }
}

/// The sounds part for `units` (the linked program's, by their files in the
/// extract at `dir`): one IR file a unit whose stem has one, read in `ir`.
pub fn load(dir: &Path, ir: IrFormat, units: &[String]) -> SoundPart {
    let started = Instant::now();
    let mut parts = Vec::new();
    let mut stems = HashMap::new();
    let mut missing = Vec::new();
    for (u, name) in units.iter().enumerate() {
        let Some(stem) = unit_stem(name) else {
            continue;
        };
        let rel = guac_extract::sounds::path(ir, &format!("{stem}.bld"));
        match read_part(dir, ir, &rel) {
            Ok(p) => {
                stems.insert(u as u16, stem);
                parts.push(p);
            }
            Err(why) => missing.push((stem, why)),
        }
    }
    SoundPart {
        book: Arc::new(SoundBook::new(parts)),
        stems,
        missing,
        read: started.elapsed(),
    }
}

fn read_part(dir: &Path, ir: IrFormat, rel: &str) -> Result<Sounds, String> {
    let bytes = std::fs::read(dir.join(rel)).map_err(|e| format!("{rel}: {e}"))?;
    match ir {
        IrFormat::Ron => std::str::from_utf8(&bytes)
            .map_err(|e| e.to_string())
            .and_then(|t| guac_ir::from_ron(t).map_err(|e| e.to_string())),
        IrFormat::Postcard => guac_ir::from_postcard(&bytes).map_err(|e| e.to_string()),
    }
    .map_err(|e| format!("{rel}: {e}"))
}

/// A sound playing on the game's side: what started it and on what, the
/// file and settings the book gave, and where its actor stood at the last
/// engine frame. Its sequence is the scripts' (`time remaining` and the
/// rest read it); this is what the TV's player plays it by.
#[derive(Debug, Clone, PartialEq)]
pub struct SoundPlay {
    /// Unique across every run on this TV: a play started again is a new
    /// play.
    pub serial: u64,
    /// Its sequence, by index in the world's list.
    pub sequence: u32,
    pub id: SoundId,
    pub name: Option<String>,
    /// The audio file, relative to the extract root.
    pub file: String,
    /// Its settings as the data holds them when it started.
    pub params: Params,
    /// Its length on the game clock, seconds ([`PLAY_END`]).
    pub length: f32,
    /// The engine frame it started on ([`crate::scripts::ScriptWorld`]'s
    /// sound clock).
    pub started: u64,
    /// Its actor's pivot at the last engine frame, metres; `None` before
    /// the first.
    pub at: Option<Vec3>,
}

impl SoundPlay {
    pub(crate) fn of(
        serial: u64,
        sequence: u32,
        id: SoundId,
        p: &Playable<'_>,
        length: f32,
        started: u64,
    ) -> Self {
        Self {
            serial,
            sequence,
            id,
            name: p.sound.name.clone(),
            file: p.file.to_owned(),
            params: p.params,
            length,
            started,
            at: None,
        }
    }
}

/// Why a play left the playing list.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Ended {
    /// Its length ran out on the game clock with nothing stopping it.
    PlayedOut,
    /// A script stopped it, a limit took its place, or a start played it
    /// again from its start.
    Stopped,
    /// Its actor left play (taken off, or gone from the world).
    ActorLeft,
}

impl Ended {
    pub fn name(self) -> &'static str {
        match self {
            Ended::PlayedOut => "played out",
            Ended::Stopped => "stopped",
            Ended::ActorLeft => "actor left",
        }
    }
}

/// A play that ended, as the test hooks list it.
#[derive(Debug, Clone, PartialEq)]
pub struct SoundEnded {
    pub name: Option<String>,
    pub id: SoundId,
    pub started: u64,
    pub ended: u64,
    pub how: Ended,
}

/// How many ended plays and refused starts the world keeps for the test
/// hooks: a fight's worth.
pub const RECENT: usize = 256;

/// A play that couldn't be made: the sound's silent or unknown, or a limit
/// refused it.
#[derive(Debug, Clone, PartialEq)]
pub struct SoundRefused {
    pub name: Option<String>,
    pub id: SoundId,
    pub at: u64,
    pub why: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_units_stem_is_its_archives() {
        assert_eq!(unit_stem("scripts/global.igz").as_deref(), Some("global"));
        assert_eq!(
            unit_stem("scripts/009_Eruptor.igz").as_deref(),
            Some("009_eruptor")
        );
        assert_eq!(
            unit_stem("levels/level_213_tutotial/script.igz").as_deref(),
            Some("level_213_tutotial")
        );
        for other in [
            "part",
            "scripts/.igz",
            "scripts/a/b.igz",
            "scripts/global.ron",
            "levels/level_213_tutotial/props.ron",
            "levels//script.igz",
        ] {
            assert_eq!(unit_stem(other), None, "{other}");
        }
    }

    #[test]
    fn a_plays_length_is_its_frames_or_the_builds() {
        let audio = Audio {
            file: "audio/sounds/x.mp3".into(),
            rate: 48_000,
            channels: 2,
            samples: 96_000,
            loop_points: None,
        };
        assert_eq!(play_length(SoundEnd::Frames, &audio, 1.5), 2.0);
        assert_eq!(play_length(SoundEnd::Built, &audio, 1.5), 1.5);
        let no_rate = Audio { rate: 0, ..audio };
        assert_eq!(play_length(SoundEnd::Frames, &no_rate, 1.5), 1.5);
        assert_eq!(play_length(SoundEnd::Built, &no_rate, -1.0), 0.0);
    }
}
