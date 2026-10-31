//! The sounds a script plays, looked up, and what their settings mean
//! (openspec AR.6). Pure: no Bevy, no I/O; the caller loads the extract's
//! sounds part (`guac_ir::sound::Sounds`, one per archive) and hands it
//! here.
//!
//! # What a script names
//!
//! A sound is a `tfbSoundInfo`, an object of an archive's `level.bld`; a
//! script plays one by starting it as a sequence, as it starts a clip, and
//! names it by reference, which the VM holds as the file it was linked
//! from and the object's index there. [`SoundBook::lookup`] takes that pair
//! as an archive's stem (`global`, `009_eruptor`, `level_213_tutotial`:
//! the archive's file name lower-cased without `.bld`, as every part of the
//! extract names it) and the object. [`SoundBook::named`] finds sounds by
//! their name, case ignored. A level's music is sounds like any other, in
//! the `Music` category ([`SoundBook::music`]); a script plays a track by
//! naming its sound to `global.bld`'s music macros (AR.4).
//!
//! # What the settings mean
//!
//! Each is as the disc stores it ([`Params`]), and what it does is written
//! here in our own words from how the disc's scripts use the `soundinfo`
//! members (AR.4's reading of the music and sound macros, the stock-take's
//! member counts) and from what the values are on the disc. The oracle runs
//! silent, so no meaning here was heard against the original: each one not
//! settled by the data alone says so (**hypothesis**), and the TV's player
//! lists those in the stand-in ledger.
//!
//! - **Volume** (`soundinfo.volume`, the most-used sound member, 1,438
//!   references): percent of the recording's own level, 100 as recorded.
//!   The music macros fade a track by sliding it to 0 and back. Read as a
//!   linear gain, volume / 100 (**hypothesis**: FMOD's volume is linear).
//! - **Volume randomness** and **pitch randomness**: how far each play may
//!   stray from the volume, and from the recording's own pitch, in percent,
//!   either way ([`Params::volume_for`], [`Params::speed_for`]), each play
//!   drawing anew (**hypothesis**: uniform, symmetric, of the value itself);
//!   a sound flagged pitch-negative strays only downward (**hypothesis**,
//!   from the flag's name). The draws are the player's own cosmetic
//!   generator's, never the session's (AH.87's rule for what doesn't play
//!   into the game).
//! - **Falloff**: a sound that attenuates is full within its full distance
//!   and silent past its silent distance, in feet, from where the actor
//!   that plays it stands to the listener ([`Params::gain_at`]); between
//!   them it falls linearly (**hypothesis**: the original's curve isn't
//!   read). The listener is the camera (**hypothesis**: the sound system's
//!   listener modes are camera, player or custom, and nothing in chapter 1's
//!   scripts sets them). A sound that doesn't attenuate plays at its volume
//!   wherever it is.
//! - **3D**: placed left to right by where its actor stands to the listener;
//!   a sound that isn't plays centred, panned by its pan percent
//!   (**hypothesis**: -100 left to 100 right).
//! - **Looping**: it plays again from its start when it ends, until a
//!   script stops it or its actor goes. A recording with a loop chunk loops
//!   between its points; none of chapter 1's has one, so a track loops its
//!   whole recording (AR.4).
//! - **Allow restart**: starting it while it plays starts it over; without
//!   it, a start while it plays is ignored (**hypothesis**, from the name;
//!   `sequence.allow restart` is a clip's switch too).
//! - **Ignores time scales**: plays at its own speed when the game slows
//!   time (**hypothesis**, from the name).
//! - **Category**: the mix group (`Effects/Champion/Combat`, `Voice/NPC/`,
//!   `Music`, ...: [`CATEGORIES`], the EBOOT's own names), which the sound
//!   system's master volumes and the scripts' category settings act on.
//! - **Priority** (0-100) and the **actor and global limits**: how many
//!   plays of this sound one actor, and the whole game, may hold at once,
//!   and what a new play does when they're full: fail, or take the place of
//!   the quietest, the furthest or the oldest ([`LIMIT_BEHAVIORS`]).
//!   `Sound - Play From Set` reads the limits before it plays.
//! - **Rumble duration**: the recording's length as the build measured it,
//!   on every sound read (the music macros read it as a track's length).
//! - **Reverb, filter, sample-rate reduction**: carried, applied by nothing
//!   (AR's non-goals).
//! - **From controller**: carried; what it does isn't read.

use std::collections::HashMap;

use guac_ir::sound::{Audio, DataHome, DataRef, Sound, SoundData, Sounds};

/// `soundinfo.category`'s names by value, as the EBOOT spells them.
pub const CATEGORIES: [&str; 32] = [
    "Effects/",
    "Effects/Champion/",
    "Effects/Champion/Combat",
    "Effects/Champion/Movement",
    "Effects/Enemy/",
    "Effects/Enemy/Combat",
    "Effects/Enemy/Movement",
    "Effects/Level/",
    "Effects/Level/Object",
    "Effects/Level/Cutscene",
    "Effects/Level/Ambience",
    "Effects/Level/Environment",
    "Voice/",
    "Voice/Champion/",
    "Voice/Champion/Emote",
    "Voice/Champion/Banter",
    "Voice/Enemy/",
    "Voice/Enemy/Emote",
    "Voice/Enemy/Banter",
    "Voice/NPC/",
    "Voice/NPC/Emote",
    "Voice/NPC/Instruction",
    "Voice/Announcer/",
    "Voice/Announcer/Banter",
    "Voice/Announcer/Instruction",
    "Effects/UI/",
    "Effects/UI/Interface",
    "Effects/UI/Feedback",
    "Music",
    "Video",
    "Other",
    "Voice/Champion/Portal",
];

/// The `Music` category's value.
pub const MUSIC: u32 = 28;

/// What a full limit does with a new play, by value.
pub const LIMIT_BEHAVIORS: [&str; 4] = [
    "just fail",
    "steal quietest",
    "steal furthest",
    "steal oldest",
];

/// `Sequence._flags`' bits the settings read (AR.3).
mod flag {
    pub const LOOPING: u32 = 1 << 0;
    pub const ALLOW_RESTART: u32 = 1 << 2;
    pub const ATTENUATES: u32 = 1 << 3;
    pub const THREE_D: u32 = 1 << 5;
    pub const FILTER_ATTENUATES: u32 = 1 << 6;
    pub const FROM_CONTROLLER: u32 = 1 << 8;
    pub const IGNORES_TIME_SCALES: u32 = 1 << 24;
}

/// An archive's stem, as every part of the extract names its files:
/// `permanent/global.bld` → `global`.
pub fn stem(archive: &str) -> String {
    let file = archive.rsplit('/').next().unwrap_or(archive);
    file.strip_suffix(".bld")
        .unwrap_or(file)
        .to_ascii_lowercase()
}

/// A sound, by the archive it's in and its object there.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct SoundId {
    pub stem: String,
    pub object: u32,
}

/// What a full limit does with a new play, by [`LIMIT_BEHAVIORS`]' value.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LimitBehavior {
    Fail,
    StealQuietest,
    StealFurthest,
    StealOldest,
}

/// How many plays of a sound may sound at once, and what a new one does
/// past that.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Limit {
    pub count: u32,
    pub behavior: LimitBehavior,
}

/// A sound that attenuates: full within `full` feet, silent past `silent`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Falloff {
    pub full: f32,
    pub silent: f32,
}

/// A sound's settings as the disc stores them, and their meanings (the
/// crate docs).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Params {
    pub volume_percent: f32,
    pub volume_randomness_percent: f32,
    pub pitch_randomness_percent: f32,
    pub pitch_is_negative: bool,
    pub pan_percent: f32,
    /// `None` for a sound that plays the same wherever it is.
    pub falloff: Option<Falloff>,
    pub three_d: bool,
    pub looping: bool,
    pub allow_restart: bool,
    pub ignores_time_scales: bool,
    pub from_controller: bool,
    /// An index into [`CATEGORIES`].
    pub category: u32,
    pub priority: u32,
    pub actor_limit: Limit,
    pub global_limit: Limit,
    pub rumble_duration: f32,
    pub reverb_amount: f32,
    pub reverb_randomness_percent: f32,
    pub filter_percent: f32,
    pub filter_randomness_percent: f32,
    pub filter_attenuates: bool,
    pub filter_attenuation_distance_minimum: f32,
    pub filter_attenuation_distance_maximum: f32,
    pub filter_maximum_percent: f32,
    pub sample_rate_reduction: f32,
    /// 0 one way, 1 ping pong, and the percent it plays at.
    pub playback_mode: u32,
    pub playback_percent: f32,
}

fn behavior(bits: u32) -> LimitBehavior {
    match bits & 3 {
        0 => LimitBehavior::Fail,
        1 => LimitBehavior::StealQuietest,
        2 => LimitBehavior::StealFurthest,
        _ => LimitBehavior::StealOldest,
    }
}

impl Params {
    /// Reads `sound`'s settings: the flag and property words by AR.3's bits.
    pub fn of(sound: &Sound) -> Self {
        let (f, p) = (sound.flags, sound.properties);
        Self {
            volume_percent: sound.volume_percent,
            volume_randomness_percent: sound.volume_randomness_percent,
            pitch_randomness_percent: sound.pitch_randomness_percent,
            pitch_is_negative: p & 1 != 0,
            pan_percent: sound.pan_percent,
            falloff: (f & flag::ATTENUATES != 0).then_some(Falloff {
                full: sound.attenuation_distance_full,
                silent: sound.attenuation_distance_silent,
            }),
            three_d: f & flag::THREE_D != 0,
            looping: f & flag::LOOPING != 0,
            allow_restart: f & flag::ALLOW_RESTART != 0,
            ignores_time_scales: f & flag::IGNORES_TIME_SCALES != 0,
            from_controller: f & flag::FROM_CONTROLLER != 0,
            category: (p >> 1) & 0x1F,
            priority: (p >> 6) & 0xFFF,
            actor_limit: Limit {
                count: (p >> 18) & 0x1F,
                behavior: behavior(p >> 23),
            },
            global_limit: Limit {
                count: (p >> 25) & 0x1F,
                behavior: behavior(p >> 30),
            },
            rumble_duration: sound.rumble_duration,
            reverb_amount: sound.reverb_amount,
            reverb_randomness_percent: sound.reverb_randomness_percent,
            filter_percent: sound.filter_percent,
            filter_randomness_percent: sound.filter_randomness_percent,
            filter_attenuates: f & flag::FILTER_ATTENUATES != 0,
            filter_attenuation_distance_minimum: sound.filter_attenuation_distance_minimum,
            filter_attenuation_distance_maximum: sound.filter_attenuation_distance_maximum,
            filter_maximum_percent: sound.filter_maximum_percent,
            sample_rate_reduction: sound.sample_rate_reduction,
            playback_mode: sound.playback_mode,
            playback_percent: sound.playback_percent,
        }
    }

    /// The category's name in [`CATEGORIES`].
    pub fn category_name(&self) -> &'static str {
        CATEGORIES[self.category as usize & 0x1F]
    }

    /// A play's linear gain for a draw in -1..=1: the volume, strayed by
    /// the draw's share of its randomness (crate docs, **hypothesis**),
    /// never below 0.
    pub fn volume_for(&self, draw: f32) -> f32 {
        let d = draw.clamp(-1.0, 1.0);
        let percent = self.volume_percent * (1.0 + d * self.volume_randomness_percent / 100.0);
        (percent / 100.0).max(0.0)
    }

    /// A play's speed against the recording's own for a draw in -1..=1:
    /// 1 strayed by the draw's share of the pitch randomness, only
    /// downward when the sound is pitch-negative (crate docs,
    /// **hypothesis**), never below 0.
    pub fn speed_for(&self, draw: f32) -> f32 {
        let d = draw.clamp(-1.0, 1.0);
        let d = if self.pitch_is_negative { -d.abs() } else { d };
        (1.0 + d * self.pitch_randomness_percent / 100.0).max(0.0)
    }

    /// The share of its gain a play keeps `feet` from the listener: 1 for a
    /// sound that doesn't attenuate; otherwise 1 within its full distance,
    /// 0 past its silent one and linear between (crate docs,
    /// **hypothesis**).
    pub fn gain_at(&self, feet: f32) -> f32 {
        let Some(Falloff { full, silent }) = self.falloff else {
            return 1.0;
        };
        if feet <= full {
            1.0
        } else if feet >= silent {
            0.0
        } else {
            1.0 - (feet - full) / (silent - full)
        }
    }
}

/// What [`SoundBook::lookup`] hands back for a sound whose data has audio.
#[derive(Debug, Clone, PartialEq)]
pub struct Playable<'a> {
    pub sound: &'a Sound,
    /// The audio's path relative to the extract root.
    pub file: &'a str,
    pub audio: &'a Audio,
    /// The recording's length in seconds, as the build measured it.
    pub duration: f32,
    pub params: Params,
}

/// Why [`SoundBook::lookup`] found a sound silent.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Silence {
    /// Its data ref is null: nothing until a script gives it data.
    NoData,
    /// Its data ref couldn't be followed when the part was written.
    Unresolved,
    /// Its data info, or the archive holding it, isn't in the book.
    DataMissing,
    /// Its data info's bank didn't read (the part lists why).
    NoAudio,
}

/// [`SoundBook::lookup`]'s answer: no such sound, a silent one, or one to play.
#[derive(Debug, Clone, PartialEq)]
pub enum Lookup<'a> {
    /// No such sound in the book.
    Unknown,
    Silent {
        sound: &'a Sound,
        why: Silence,
    },
    Playable(Playable<'a>),
}

impl<'a> Lookup<'a> {
    pub fn playable(&self) -> Option<&Playable<'a>> {
        match self {
            Lookup::Playable(p) => Some(p),
            _ => None,
        }
    }
}

/// Every archive's sounds the caller loaded, by stem.
#[derive(Debug, Clone, Default)]
pub struct SoundBook {
    files: HashMap<String, Sounds>,
    names: HashMap<String, Vec<SoundId>>,
}

impl SoundBook {
    /// The book over `parts`; a later part of the same stem replaces an
    /// earlier one.
    pub fn new(parts: impl IntoIterator<Item = Sounds>) -> Self {
        let mut book = Self::default();
        for p in parts {
            book.files.insert(stem(&p.archive), p);
        }
        for (s, file) in &book.files {
            for sound in &file.sounds {
                if let Some(n) = &sound.name {
                    book.names
                        .entry(n.to_ascii_lowercase())
                        .or_default()
                        .push(SoundId {
                            stem: s.clone(),
                            object: sound.object,
                        });
                }
            }
        }
        for ids in book.names.values_mut() {
            ids.sort();
        }
        book
    }

    /// The stems the book holds, in no set order.
    pub fn stems(&self) -> impl Iterator<Item = &str> {
        self.files.keys().map(String::as_str)
    }

    pub fn file(&self, stem: &str) -> Option<&Sounds> {
        self.files.get(stem)
    }

    /// The sound at `object` of `stem`'s `level.bld`.
    pub fn sound(&self, stem: &str, object: u32) -> Option<&Sound> {
        self.files.get(stem)?.sound(object)
    }

    /// Every sound named `name` (case ignored), in stem and object order.
    pub fn named(&self, name: &str) -> &[SoundId] {
        self.names
            .get(&name.to_ascii_lowercase())
            .map_or(&[], Vec::as_slice)
    }

    /// `stem`'s music: its sounds in the `Music` category.
    pub fn music<'b>(&'b self, stem: &str) -> impl Iterator<Item = &'b Sound> + 'b {
        self.files
            .get(stem)
            .into_iter()
            .flat_map(|f| f.sounds.iter())
            .filter(|s| (s.properties >> 1) & 0x1F == MUSIC)
    }

    /// The data info a sound of `stem` plays, following a `global.bld`
    /// ref into the `global` archive's sounds.
    fn data(&self, stem: &str, data: DataRef) -> Result<&SoundData, Silence> {
        let (file, home, object) = match data {
            DataRef::None => return Err(Silence::NoData),
            DataRef::Unresolved => return Err(Silence::Unresolved),
            DataRef::Here(home, o) => (stem, home, o),
            DataRef::Global(o) => ("global", DataHome::Level, o),
        };
        self.files
            .get(file)
            .and_then(|f| f.data_at(home, object))
            .ok_or(Silence::DataMissing)
    }

    /// What the sound at `object` of `stem` plays, and how.
    pub fn lookup(&self, stem: &str, object: u32) -> Lookup<'_> {
        let Some(sound) = self.sound(stem, object) else {
            return Lookup::Unknown;
        };
        let data = match self.data(stem, sound.data) {
            Ok(d) => d,
            Err(why) => return Lookup::Silent { sound, why },
        };
        let Some(audio) = &data.audio else {
            return Lookup::Silent {
                sound,
                why: Silence::NoAudio,
            };
        };
        Lookup::Playable(Playable {
            sound,
            file: &audio.file,
            audio,
            duration: data.raw_duration,
            params: Params::of(sound),
        })
    }

    /// [`SoundBook::lookup`] by id.
    pub fn lookup_id(&self, id: &SoundId) -> Lookup<'_> {
        self.lookup(&id.stem, id.object)
    }
}

#[cfg(test)]
mod tests;
