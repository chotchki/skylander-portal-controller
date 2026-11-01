//! The host's half of a sound's play (openspec AR.9, AR.10): a `start
//! sequence` on a sound info the sounds part gives audio for, its limits,
//! its stop, its end on the game clock, and the members that read it.
//!
//! What a start does, each a reading of the data and the scripts' use of it
//! (`guac-sound`'s crate docs have the settings' meanings), none heard
//! against the original, since the oracle runs silent
//! (stand-in ledger: `sound-play`):
//! - a copy of the sound plays on the actor the op names, a sequence like a
//!   clip's, its length the recording's ([`crate::sounds::PLAY_END`]), looping
//!   when the sound loops, what `time remaining`, `looping` and the rest
//!   read;
//! - a sound with `allow restart` already playing on that actor starts over;
//! - past the sound's actor limit (on that actor) or global limit (on every
//!   actor), the limit's behaviour decides: `just fail` refuses the start
//!   (the op fails, its block skipped, as a refused clip's), any steal stops
//!   the oldest copy and plays the new one (the quietest and the furthest
//!   are the listener's to tell, which the game's side doesn't hold, so
//!   both take the oldest: a hypothesis); a limit of 0 limits nothing;
//! - a `stop sequence` on the sound info stops its copies on the receiver,
//!   one on a copy stops that copy; a copy whose actor leaves play (taken
//!   off, or gone) stops with it.
//!
//! The members (openspec AR.10): a sound's `soundinfo` members read what
//! the sounds part holds of it until a script writes one, then what was
//! written; a write holds for the sound's later plays and the ones playing
//! (a fade's volume reaches what's playing as it slides: a hypothesis, the
//! music macros' fades read so). A copy reads its sound's until a script
//! writes the copy's own, which holds for that copy alone: `Sound - Ultra
//! Sound` sets a copy's volume to 0 and slides it to its sound's, and
//! `Sound - Play With Delay` saves a copy's volume and silences it, which
//! only reads so if the copy's settings are its own (a reading of the
//! scripts: the original's copy isn't read). An
//! object the book doesn't know as a sound keeps what's written and reads
//! 0 until then, as the value cell it was. The deprecated categories read
//! 0. The sound system's read their start until written: the master
//! volumes, music volumes and rates 100, the rest 0 (the listener the
//! camera); its effects, streams and music master volumes scale what plays
//! by whether it streams and its category, `music disabled` silences the
//! music, the master rates scale its speed (each a hypothesis: the oracle
//! runs silent and no script in chapter 1 writes them); reverb, the music
//! volumes, the listener modes and the diagnostic level are carried. Its
//! sets: the copies playing of embedded sounds, of streamed ones, and no
//! movie, rumble or category template.

use guac_ir::sound::{DataHome, DataRef, Sound};
use guac_script::{Access, Answer, Handle, HostFault, Obj, Value};
use guac_sound::{Falloff, Limit, LimitBehavior, Lookup, Params, SoundId};
use tracing::debug;

use crate::script_engine::{Engine, Object, Sequence};
use crate::script_members::Member;
use crate::scripts::{ScriptWorld, fresh_serial};
use crate::sounds::{Ended, PLAY_END, RECENT, SoundEnded, SoundPlay, SoundRefused, play_length};

/// What [`ScriptWorld::sound_lookup`] found for an object.
pub(crate) enum Found {
    /// Not a sound the book knows: the object plays as any other.
    NotASound,
    /// A sound with nothing to play, and why.
    Silent(SoundId, Option<String>, &'static str),
    Playable(SoundId),
}

impl ScriptWorld {
    /// What the book holds for file object `o`.
    pub(crate) fn sound_lookup(&self, o: Obj) -> Found {
        let (Some(book), Some(stem)) = (self.sounds.as_deref(), self.sound_stems.get(&o.unit))
        else {
            return Found::NotASound;
        };
        let id = SoundId {
            stem: stem.clone(),
            object: o.index,
        };
        match book.lookup(stem, o.index) {
            Lookup::Unknown => Found::NotASound,
            Lookup::Silent { sound, why } => Found::Silent(
                id,
                sound.name.clone(),
                match why {
                    guac_sound::Silence::NoData => "no data",
                    guac_sound::Silence::Unresolved => "its data unresolved",
                    guac_sound::Silence::DataMissing => "its data missing",
                    guac_sound::Silence::NoAudio => "no audio",
                },
            ),
            Lookup::Playable(_) => Found::Playable(id),
        }
    }

    /// Whether the actor `h` has left play for a sound on it: taken off, or
    /// no actor of the world's at all; a hero, or an object of the host's
    /// (a pad, the portal), never has.
    fn left_play(&self, h: Handle, engine: &mut dyn Engine) -> bool {
        if engine.hero(h).is_some() || self.objs.get(h).is_some() {
            return false;
        }
        self.actors.get(&h).is_none_or(|a| a.removed)
    }

    fn note_refused(&mut self, id: SoundId, name: Option<String>, why: String) {
        debug!(sound = ?name, stem = %id.stem, object = id.object, %why, "a sound's start played nothing");
        if self.sound_refused.len() == RECENT {
            self.sound_refused.pop_front();
        }
        self.sound_refused.push_back(SoundRefused {
            name,
            id,
            at: self.sound_clock,
            why,
        });
    }

    /// The play of sequence `i` off the playing list, into the ended ones
    /// the test hooks read.
    fn end_play(&mut self, i: u32, how: Ended) {
        let Some(k) = self.plays.iter().position(|p| p.sequence == i) else {
            return;
        };
        let p = self.plays.remove(k);
        if self.sound_ended.len() == RECENT {
            self.sound_ended.pop_front();
        }
        self.sound_ended.push_back(SoundEnded {
            name: p.name,
            id: p.id,
            started: p.started,
            ended: self.sound_clock,
            how,
        });
    }

    /// A play of sequence `i` begun: its sound's, by the book, from now.
    fn begin_play(&mut self, i: u32, id: SoundId) {
        let Some(book) = self.sounds.clone() else {
            return;
        };
        let Lookup::Playable(p) = book.lookup(&id.stem, id.object) else {
            return;
        };
        let length = play_length(PLAY_END, p.audio, p.duration);
        let mut play = SoundPlay::of(fresh_serial(), i, id, &p, length, self.sound_clock);
        if let Some(Value::Object(o)) = self.sequences.get(i as usize).map(|s| s.template) {
            play.params = self.copy_params(i, o, play.params);
        }
        self.plays.push(play);
        self.sounds_started += 1;
    }

    /// Sequence `i` started again from its start (`start_again`): a sound's
    /// is a new play from now.
    pub(crate) fn sound_restarted(&mut self, i: u32) {
        let Some(Value::Object(o)) = self.sequences.get(i as usize).map(|s| s.template) else {
            return;
        };
        if let Found::Playable(id) = self.sound_lookup(o) {
            self.end_play(i, Ended::Stopped);
            self.begin_play(i, id);
        }
    }

    /// The copies of `template` a limit counts: a stopped or played-out one
    /// holds no place.
    fn playing_copies(&self, template: Value, actor: Option<Handle>) -> Vec<usize> {
        self.sequences
            .iter()
            .enumerate()
            .filter(|(_, s)| {
                s.template == template && s.plays() && actor.is_none_or(|a| s.actor == a)
            })
            .map(|(i, _)| i)
            .collect()
    }

    /// Room for one more copy under `limit` among `copies` (oldest first):
    /// `Ok` once there is (a steal stopping the oldest), `Err` for a full
    /// limit that fails.
    fn make_room(&mut self, limit: Limit, copies: &[usize]) -> Result<(), &'static str> {
        if limit.count == 0 || copies.len() < limit.count as usize {
            return Ok(());
        }
        if limit.behavior == LimitBehavior::Fail {
            return Err("its limit is full");
        }
        // The oldest copies, enough to leave room for one.
        let over = copies.len() + 1 - limit.count as usize;
        for &i in &copies[..over] {
            self.sequences[i].stopped = true;
            self.end_play(i as u32, Ended::Stopped);
        }
        Ok(())
    }

    /// `template`, file object `o`, played on `actor` as a sound (openspec
    /// AR.9, the module docs), if the book knows it: `None` for an object
    /// that isn't one of its sounds, which plays as any other.
    pub(crate) fn play_sound(&mut self, template: Value, o: Obj, actor: Handle) -> Option<Answer> {
        let id = match self.sound_lookup(o) {
            Found::NotASound => return None,
            Found::Silent(id, name, why) => {
                self.note_refused(id, name, why.to_owned());
                return None;
            }
            Found::Playable(id) => id,
        };
        let book = self.sounds.clone()?;
        let Lookup::Playable(p) = book.lookup(&id.stem, id.object) else {
            return None;
        };
        // As written since, a script's settings over the data's (AR.10).
        let params = self.written_params(o, p.params);
        let name = p.sound.name.clone();
        if params.allow_restart
            && let Some(&i) = self.playing_copies(template, Some(actor)).last()
        {
            return Some(self.start_again(i, actor));
        }
        let on_actor = self.playing_copies(template, Some(actor));
        if let Err(why) = self.make_room(params.actor_limit, &on_actor) {
            self.note_refused(id, name, format!("{why} on its actor"));
            return Some(Answer::Skip);
        }
        let everywhere = self.playing_copies(template, None);
        if let Err(why) = self.make_room(params.global_limit, &everywhere) {
            self.note_refused(id, name, format!("{why} everywhere"));
            return Some(Answer::Skip);
        }
        let length = play_length(PLAY_END, p.audio, p.duration);
        self.sequences.push(Sequence {
            actor,
            template,
            length,
            time: 0.0,
            rate: 100.0,
            looping: params.looping,
            stopped: false,
            done: false,
            blend: 100.0,
            kind: 0,
            layered: false,
            allow_restart: params.allow_restart,
        });
        let i = (self.sequences.len() - 1) as u32;
        self.begin_play(i, id);
        Some(Answer::Run {
            receiver: None,
            result: Some(self.objs.value(Object::Sequence(i))),
        })
    }

    /// What a `start sequence` on file object `sound` does on `actor` (the
    /// module docs), for an embedder or a test that plays one without a
    /// script: the copy started, or `None` for a start refused or an object
    /// the book doesn't know as a sound.
    pub fn play_sound_on(&mut self, sound: Obj, actor: Handle) -> Option<Value> {
        match self.play_sound(Value::Object(sound), sound, actor)? {
            Answer::Run { result, .. } => result,
            _ => None,
        }
    }

    /// A `stop sequence` on sound `template` with `receiver`: its copies on
    /// the receiver stop. False for a template that isn't a sound.
    pub(crate) fn stop_sound(&mut self, template: Value, receiver: Option<Handle>) -> bool {
        let Value::Object(o) = template else {
            return false;
        };
        if !matches!(self.sound_lookup(o), Found::Playable(_) | Found::Silent(..)) {
            return false;
        }
        if let Some(actor) = receiver {
            for i in self.playing_copies(template, Some(actor)) {
                self.sequences[i].stopped = true;
            }
        }
        true
    }

    /// The sounds' frame, at the engine's frame's end: each play whose
    /// sequence played out or was stopped, or whose actor left (its
    /// sequence stopped with it), ended; the rest given their actor's place.
    pub(crate) fn advance_sounds(&mut self, engine: &mut dyn Engine) {
        self.sound_clock += 1;
        let mut ended = Vec::new();
        for k in 0..self.plays.len() {
            let i = self.plays[k].sequence as usize;
            let Some(s) = self.sequences.get(i) else {
                ended.push((i as u32, Ended::Stopped));
                continue;
            };
            let actor = s.actor;
            if s.stopped {
                ended.push((i as u32, Ended::Stopped));
            } else if s.done {
                ended.push((i as u32, Ended::PlayedOut));
            } else if self.left_play(actor, engine) {
                self.sequences[i].stopped = true;
                ended.push((i as u32, Ended::ActorLeft));
            } else {
                let at = self.position(actor, engine);
                self.plays[k].at = at;
            }
        }
        for (i, how) in ended {
            self.end_play(i, how);
        }
    }

    /// The sound copies playing on `actor`, in the order started
    /// (`actorinfo.playing sounds`, openspec AR.10).
    pub(crate) fn sounds_on(&self, actor: Handle) -> Vec<u32> {
        self.plays
            .iter()
            .filter(|p| {
                self.sequences
                    .get(p.sequence as usize)
                    .is_some_and(|s| s.actor == actor && s.plays())
            })
            .map(|p| p.sequence)
            .collect()
    }
}

/// A sound's `soundinfo` members (openspec AR.10).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum SoundField {
    Volume,
    VolumeRandomness,
    PitchRandomness,
    RumbleDuration,
    Pan,
    ThreeD,
    Attenuates,
    AttenuationFull,
    AttenuationSilent,
    IgnoresTimeScales,
    Category,
    ReverbAmount,
    ReverbRandomness,
    FilterPercent,
    FilterRandomness,
    FilterAttenuates,
    FilterMinimum,
    FilterMaximum,
    FilterMaximumPercent,
    Priority,
    ActorLimit,
    ActorLimitBehavior,
    GlobalLimit,
    GlobalLimitBehavior,
    FromController,
    DesignerCategory,
    AuthoredCategory,
}

/// The sound system's members.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum SystemField {
    EffectsMasterVolume,
    EffectsMasterRate,
    ActiveSoundEffects,
    ActiveMovies,
    StreamsMasterVolume,
    StreamsMasterRate,
    ActiveSoundStreams,
    ActiveControllerRumbles,
    CategoryTemplates,
    MusicDisabled,
    MusicMasterVolume,
    ReverbSetting(u8),
    ReverbAmount(u8),
    MusicVolume(u8),
    PanListenerMode,
    VolumeListenerMode,
    DiagnosticLevel,
}

/// Each sound member by its registered name.
pub(crate) const SOUND_FIELDS: &[(&str, SoundField)] = {
    use SoundField as F;
    &[
        ("volume", F::Volume),
        ("volume randomness", F::VolumeRandomness),
        ("pitch randomness", F::PitchRandomness),
        ("rumble duration", F::RumbleDuration),
        ("pan", F::Pan),
        ("3d", F::ThreeD),
        ("attenuates", F::Attenuates),
        ("attenuation distance full", F::AttenuationFull),
        ("attenuation distance silent", F::AttenuationSilent),
        ("ignores time scales", F::IgnoresTimeScales),
        ("category", F::Category),
        ("reverb amount", F::ReverbAmount),
        ("reverb randomness", F::ReverbRandomness),
        ("filter percent", F::FilterPercent),
        ("filter randomness", F::FilterRandomness),
        ("filter attenuates", F::FilterAttenuates),
        ("filter attenuation distance minimum", F::FilterMinimum),
        ("filter attenuation distance maximum", F::FilterMaximum),
        ("filter maximum percent", F::FilterMaximumPercent),
        ("priority", F::Priority),
        ("actor limit", F::ActorLimit),
        ("actor limit behavior", F::ActorLimitBehavior),
        ("global limit", F::GlobalLimit),
        ("global limit behavior", F::GlobalLimitBehavior),
        ("from controller", F::FromController),
        ("designer category", F::DesignerCategory),
        ("authored category", F::AuthoredCategory),
    ]
};

/// Each sound system member by its registered name.
pub(crate) const SYSTEM_FIELDS: &[(&str, SystemField)] = {
    use SystemField as F;
    &[
        ("effects master volume", F::EffectsMasterVolume),
        ("effects master rate", F::EffectsMasterRate),
        ("active sound effects", F::ActiveSoundEffects),
        ("active movies", F::ActiveMovies),
        ("streams master volume", F::StreamsMasterVolume),
        ("streams master rate", F::StreamsMasterRate),
        ("active sound streams", F::ActiveSoundStreams),
        ("active controller rumbles", F::ActiveControllerRumbles),
        ("category templates", F::CategoryTemplates),
        ("music disabled", F::MusicDisabled),
        ("music master volume", F::MusicMasterVolume),
        ("reverb setting 1", F::ReverbSetting(1)),
        ("reverb setting 2", F::ReverbSetting(2)),
        ("reverb amount 1", F::ReverbAmount(1)),
        ("reverb amount 2", F::ReverbAmount(2)),
        ("music volume 1", F::MusicVolume(1)),
        ("music volume 2", F::MusicVolume(2)),
        ("music volume 3", F::MusicVolume(3)),
        ("pan listener mode", F::PanListenerMode),
        ("volume listener mode", F::VolumeListenerMode),
        ("diagnostic level", F::DiagnosticLevel),
    ]
};

impl SystemField {
    /// What it reads until a script writes it (the module docs).
    pub(crate) fn start(self) -> Value {
        use SystemField as F;
        match self {
            F::EffectsMasterVolume
            | F::EffectsMasterRate
            | F::StreamsMasterVolume
            | F::StreamsMasterRate
            | F::MusicMasterVolume
            | F::MusicVolume(_) => Value::Float(100.0),
            F::ReverbAmount(_) => Value::Float(0.0),
            _ => Value::Int(0),
        }
    }
}

const BIT_ATTENUATES: u32 = 1 << 3;
const BIT_THREE_D: u32 = 1 << 5;
const BIT_FILTER_ATTENUATES: u32 = 1 << 6;
const BIT_FROM_CONTROLLER: u32 = 1 << 8;
const BIT_IGNORES_TIME_SCALES: u32 = 1 << 24;

fn bit(word: u32, b: u32) -> Value {
    Value::Int(i32::from(word & b != 0))
}

fn field(word: u32, shift: u32, mask: u32) -> Value {
    Value::Int(((word >> shift) & mask) as i32)
}

/// What `sound` holds of `f`, as its member reads it.
fn data_value(sound: &Sound, f: SoundField) -> Value {
    use SoundField as F;
    let (fl, p) = (sound.flags, sound.properties);
    match f {
        F::Volume => Value::Float(sound.volume_percent),
        F::VolumeRandomness => Value::Float(sound.volume_randomness_percent),
        F::PitchRandomness => Value::Float(sound.pitch_randomness_percent),
        F::RumbleDuration => Value::Float(sound.rumble_duration),
        F::Pan => Value::Float(sound.pan_percent),
        F::ThreeD => bit(fl, BIT_THREE_D),
        F::Attenuates => bit(fl, BIT_ATTENUATES),
        F::AttenuationFull => Value::Float(sound.attenuation_distance_full),
        F::AttenuationSilent => Value::Float(sound.attenuation_distance_silent),
        F::IgnoresTimeScales => bit(fl, BIT_IGNORES_TIME_SCALES),
        F::Category => field(p, 1, 0x1F),
        F::ReverbAmount => Value::Float(sound.reverb_amount),
        F::ReverbRandomness => Value::Float(sound.reverb_randomness_percent),
        F::FilterPercent => Value::Float(sound.filter_percent),
        F::FilterRandomness => Value::Float(sound.filter_randomness_percent),
        F::FilterAttenuates => bit(fl, BIT_FILTER_ATTENUATES),
        F::FilterMinimum => Value::Float(sound.filter_attenuation_distance_minimum),
        F::FilterMaximum => Value::Float(sound.filter_attenuation_distance_maximum),
        F::FilterMaximumPercent => Value::Float(sound.filter_maximum_percent),
        F::Priority => field(p, 6, 0xFFF),
        F::ActorLimit => field(p, 18, 0x1F),
        F::ActorLimitBehavior => field(p, 23, 3),
        F::GlobalLimit => field(p, 25, 0x1F),
        F::GlobalLimitBehavior => field(p, 30, 3),
        F::FromController => bit(fl, BIT_FROM_CONTROLLER),
        F::DesignerCategory | F::AuthoredCategory => Value::Int(0),
    }
}

/// What an unwritten member of an object the book doesn't know reads: its
/// value cell's start.
fn unknown_value(f: SoundField) -> Value {
    use SoundField as F;
    match f {
        F::ThreeD
        | F::Attenuates
        | F::IgnoresTimeScales
        | F::Category
        | F::FilterAttenuates
        | F::Priority
        | F::ActorLimit
        | F::ActorLimitBehavior
        | F::GlobalLimit
        | F::GlobalLimitBehavior
        | F::FromController
        | F::DesignerCategory
        | F::AuthoredCategory => Value::Int(0),
        _ => Value::Float(0.0),
    }
}

fn behavior_of(v: Option<&Value>, was: LimitBehavior) -> LimitBehavior {
    match v.and_then(Value::as_i32) {
        Some(0) => LimitBehavior::Fail,
        Some(1) => LimitBehavior::StealQuietest,
        Some(2) => LimitBehavior::StealFurthest,
        Some(3) => LimitBehavior::StealOldest,
        _ => was,
    }
}

impl ScriptWorld {
    /// The sound info a receiver names, and the copy when it's one: the
    /// sound itself, or a copy and its sound.
    fn sound_of(&self, recv: Option<Value>) -> Option<(Obj, Option<u32>)> {
        match recv? {
            Value::Object(o) => Some((o, None)),
            v => match self.handle(v).and_then(|h| self.objs.get(h)) {
                Some(Object::Sequence(i)) => match self.sequences.get(i as usize)?.template {
                    Value::Object(o) => Some((o, Some(i))),
                    _ => None,
                },
                _ => None,
            },
        }
    }

    /// The book's sound at `o`, if it knows one there.
    fn book_sound(&self, o: Obj) -> Option<&Sound> {
        let stem = self.sound_stems.get(&o.unit)?;
        self.sounds.as_deref()?.sound(stem, o.index)
    }

    /// `f` of sound info `o` as read now: written, else the data's, else
    /// its cell's start.
    pub(crate) fn sound_value(&self, o: Obj, f: SoundField) -> Value {
        if let Some(v) = self.sound_written.get(&(o, f)) {
            return *v;
        }
        match self.book_sound(o) {
            Some(s) => data_value(s, f),
            None => unknown_value(f),
        }
    }

    /// `params` with what scripts wrote to sound info `o` laid over them.
    pub(crate) fn written_params(&self, o: Obj, params: Params) -> Params {
        self.laid_over(o, params, &|f| self.sound_written.get(&(o, f)))
    }

    /// `f` of copy `i` of sound info `o` as read now: what was written to
    /// the copy, else its sound's ([`ScriptWorld::sound_value`]).
    pub(crate) fn copy_value(&self, i: u32, o: Obj, f: SoundField) -> Value {
        match self.copy_written.get(&(i, f)) {
            Some(v) => *v,
            None => self.sound_value(o, f),
        }
    }

    /// `params` with what scripts wrote to sound info `o`, then to its copy
    /// `i`, laid over them: a copy's settings are its own once written
    /// (`Sound - Ultra Sound` fades a copy in from 0 to its sound's volume,
    /// `Sound - Play With Delay` saves a copy's volume and silences it).
    pub(crate) fn copy_params(&self, i: u32, o: Obj, params: Params) -> Params {
        let p = self.written_params(o, params);
        self.laid_over(o, p, &|f| self.copy_written.get(&(i, f)))
    }

    /// `params` with the writes `w` finds laid over them, for sound info
    /// `o`.
    fn laid_over<'a>(
        &'a self,
        o: Obj,
        params: Params,
        w: &dyn Fn(SoundField) -> Option<&'a Value>,
    ) -> Params {
        use SoundField as F;
        let float = |f, was: f32| w(f).and_then(Value::as_f32).unwrap_or(was);
        let int = |f, was: u32| {
            w(f).and_then(Value::as_i32)
                .map_or(was, |i| i.max(0) as u32)
        };
        let flag = |f, was: bool| w(f).and_then(Value::as_i32).map_or(was, |i| i != 0);
        let mut p = params;
        p.volume_percent = float(F::Volume, p.volume_percent);
        p.volume_randomness_percent = float(F::VolumeRandomness, p.volume_randomness_percent);
        p.pitch_randomness_percent = float(F::PitchRandomness, p.pitch_randomness_percent);
        p.rumble_duration = float(F::RumbleDuration, p.rumble_duration);
        p.pan_percent = float(F::Pan, p.pan_percent);
        p.three_d = flag(F::ThreeD, p.three_d);
        // A falloff turned off keeps its distances: its sound's as read.
        let (full, silent) = p.falloff.map_or_else(
            || {
                let d = |f| self.sound_value(o, f).as_f32().unwrap_or(0.0);
                (d(F::AttenuationFull), d(F::AttenuationSilent))
            },
            |f| (f.full, f.silent),
        );
        let attenuates = flag(F::Attenuates, p.falloff.is_some());
        p.falloff = attenuates.then_some(Falloff {
            full: float(F::AttenuationFull, full),
            silent: float(F::AttenuationSilent, silent),
        });
        p.ignores_time_scales = flag(F::IgnoresTimeScales, p.ignores_time_scales);
        p.category = int(F::Category, p.category);
        p.reverb_amount = float(F::ReverbAmount, p.reverb_amount);
        p.reverb_randomness_percent = float(F::ReverbRandomness, p.reverb_randomness_percent);
        p.filter_percent = float(F::FilterPercent, p.filter_percent);
        p.filter_randomness_percent = float(F::FilterRandomness, p.filter_randomness_percent);
        p.filter_attenuates = flag(F::FilterAttenuates, p.filter_attenuates);
        p.filter_attenuation_distance_minimum =
            float(F::FilterMinimum, p.filter_attenuation_distance_minimum);
        p.filter_attenuation_distance_maximum =
            float(F::FilterMaximum, p.filter_attenuation_distance_maximum);
        p.filter_maximum_percent = float(F::FilterMaximumPercent, p.filter_maximum_percent);
        p.priority = int(F::Priority, p.priority);
        p.actor_limit = Limit {
            count: int(F::ActorLimit, p.actor_limit.count),
            behavior: behavior_of(w(F::ActorLimitBehavior), p.actor_limit.behavior),
        };
        p.global_limit = Limit {
            count: int(F::GlobalLimit, p.global_limit.count),
            behavior: behavior_of(w(F::GlobalLimitBehavior), p.global_limit.behavior),
        };
        p.from_controller = flag(F::FromController, p.from_controller);
        p
    }

    /// A play's settings now: its sound's as the data holds them with what
    /// scripts have written to the sound since, then to its copy (the
    /// player reads these each frame, so a fade reaches what's playing).
    pub fn play_params(&self, play: &crate::sounds::SoundPlay) -> Params {
        match self
            .sequences
            .get(play.sequence as usize)
            .map(|s| s.template)
        {
            Some(Value::Object(o)) => {
                let base = self.book_sound(o).map_or(play.params, Params::of);
                self.copy_params(play.sequence, o, base)
            }
            _ => play.params,
        }
    }

    /// The sound system's member `f` as read now.
    pub(crate) fn system_value(&self, f: SystemField) -> Value {
        self.sound_system.get(&f).copied().unwrap_or(f.start())
    }

    fn system_percent(&self, f: SystemField) -> f32 {
        self.system_value(f).as_f32().unwrap_or(100.0).max(0.0) / 100.0
    }

    /// Whether the play's recording streams (a voice, music, ambience) or
    /// sits in its file (an effect).
    pub(crate) fn play_streams(&self, play: &crate::sounds::SoundPlay) -> bool {
        let Some(book) = self.sounds.as_deref() else {
            return false;
        };
        let Some(sound) = book.sound(&play.id.stem, play.id.object) else {
            return false;
        };
        let (stem, home, object) = match sound.data {
            DataRef::Here(home, o) => (play.id.stem.as_str(), home, o),
            DataRef::Global(o) => ("global", DataHome::Level, o),
            _ => return false,
        };
        book.file(stem)
            .and_then(|f| f.data_at(home, object))
            .is_some_and(|d| d.streamed.is_some())
    }

    /// The sound system's share of a play's gain (the module docs): the
    /// music master volume for music (none while it's disabled), else the
    /// streams' or the effects' master volume.
    pub fn master_gain(&self, play: &crate::sounds::SoundPlay) -> f32 {
        use SystemField as F;
        if self.play_params(play).category == guac_sound::MUSIC {
            if self.system_value(F::MusicDisabled).as_i32().unwrap_or(0) != 0 {
                return 0.0;
            }
            return self.system_percent(F::MusicMasterVolume);
        }
        if self.play_streams(play) {
            self.system_percent(F::StreamsMasterVolume)
        } else {
            self.system_percent(F::EffectsMasterVolume)
        }
    }

    /// The sound system's share of a play's speed: its streams' or effects'
    /// master rate.
    pub fn master_rate(&self, play: &crate::sounds::SoundPlay) -> f32 {
        if self.play_streams(play) {
            self.system_percent(SystemField::StreamsMasterRate)
        } else {
            self.system_percent(SystemField::EffectsMasterRate)
        }
    }

    /// A read of a sound member, or `None` for any other member.
    pub(crate) fn sound_member(
        &mut self,
        at: &Access,
        m: Member,
    ) -> Option<Result<Value, HostFault>> {
        Some(match m {
            Member::Sound(f) => match self.sound_of(at.receiver) {
                Some((o, Some(i))) => Ok(self.copy_value(i, o, f)),
                Some((o, None)) => Ok(self.sound_value(o, f)),
                None => Err(HostFault::Nothing),
            },
            Member::SoundSystem(f) => Ok(self.system_value(f)),
            Member::SeqActivator => {
                match self.handle(at.receiver?).and_then(|h| self.objs.get(h)) {
                    Some(Object::Sequence(i)) => self
                        .sequences
                        .get(i as usize)
                        .map(|s| Value::Entity(s.actor))
                        .ok_or(HostFault::Nothing),
                    _ => Err(HostFault::Nothing),
                }
            }
            _ => return None,
        })
    }

    /// A write of a sound member, or `None` for any other member. The
    /// activator and the system's sets read only: a write drops.
    pub(crate) fn set_sound_member(
        &mut self,
        at: &Access,
        m: Member,
        v: Value,
    ) -> Option<Result<(), HostFault>> {
        use SystemField as F;
        Some(match m {
            Member::Sound(f) => match self.sound_of(at.receiver) {
                Some((_, Some(i))) => {
                    self.copy_written.insert((i, f), v);
                    Ok(())
                }
                Some((o, None)) => {
                    self.sound_written.insert((o, f), v);
                    Ok(())
                }
                None => Err(HostFault::Nothing),
            },
            Member::SoundSystem(
                F::ActiveSoundEffects
                | F::ActiveMovies
                | F::ActiveSoundStreams
                | F::ActiveControllerRumbles
                | F::CategoryTemplates,
            ) => Err(HostFault::Nothing),
            Member::SoundSystem(f) => {
                self.sound_system.insert(f, v);
                Ok(())
            }
            Member::SeqActivator => Err(HostFault::Nothing),
            _ => return None,
        })
    }

    /// One of the sound system's sets (the module docs).
    pub(crate) fn sound_set(&mut self, f: SystemField) -> Result<Vec<Value>, HostFault> {
        use SystemField as F;
        let streams = match f {
            F::ActiveSoundEffects => false,
            F::ActiveSoundStreams => true,
            F::ActiveMovies | F::ActiveControllerRumbles | F::CategoryTemplates => {
                return Ok(Vec::new());
            }
            _ => return Err(HostFault::Nothing),
        };
        let copies: Vec<u32> = self
            .plays
            .iter()
            .filter(|p| self.play_streams(p) == streams)
            .filter(|p| {
                self.sequences
                    .get(p.sequence as usize)
                    .is_some_and(Sequence::plays)
            })
            .map(|p| p.sequence)
            .collect();
        Ok(copies
            .into_iter()
            .map(|i| self.objs.value(Object::Sequence(i)))
            .collect())
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::sync::Arc;

    use guac_formats::script::Class;
    use guac_ir::sound::{Audio, DataHome, DataRef, Sound, SoundData, Sounds};
    use guac_motion::seam::Vec3;
    use guac_script::{Access, MemberId, OpCall};
    use guac_sound::SoundBook;

    use super::*;
    use crate::script_engine::{Actor, NoEngine};
    use crate::scripts::{Files, headless_objects};

    const STEP: f32 = 1.0 / 60.0;
    const LEVEL: u16 = 4;
    const A: Handle = 0x6200_0001;
    const B: Handle = 0x6200_0002;
    /// The level's sound infos by object: a one-shot (1 s at 48 kHz, its
    /// build-measured length 0.9 s), a loop, a one-shot that may restart, a
    /// one-shot whose actor limit fails, one that steals, one with a global
    /// limit of 2, one with no data.
    const ONCE: u32 = 10;
    const LOOP: u32 = 11;
    const RESTARTS: u32 = 12;
    const FAILS: u32 = 13;
    const STEALS: u32 = 14;
    const GLOBAL: u32 = 15;
    const SILENT: u32 = 16;
    /// A streamed one, and a music track (streamed too).
    const STREAMED: u32 = 17;
    const MUSIC: u32 = 18;

    fn props(actor: u32, actor_how: u32, global: u32, global_how: u32) -> u32 {
        actor << 18 | actor_how << 23 | global << 25 | global_how << 30
    }

    fn sound(object: u32, flags: u32, properties: u32, data: DataRef) -> Sound {
        Sound {
            object,
            name: Some(format!("snd_{object}")),
            data,
            flags: 0x4000_0000 | flags,
            properties,
            playback_mode: 0,
            playback_percent: 100.0,
            volume_percent: 80.0,
            volume_randomness_percent: 0.0,
            pitch_randomness_percent: 0.0,
            pan_percent: 0.0,
            attenuation_distance_full: 1.0,
            attenuation_distance_silent: 150.0,
            rumble_duration: 0.9,
            reverb_amount: 0.0,
            reverb_randomness_percent: 0.0,
            filter_percent: 0.0,
            filter_randomness_percent: 0.0,
            filter_attenuation_distance_minimum: 1.0,
            filter_attenuation_distance_maximum: 200.0,
            filter_maximum_percent: 100.0,
            sample_rate_reduction: 0.0,
        }
    }

    fn book() -> SoundBook {
        let here = DataRef::Here(DataHome::Level, 90);
        let no_limit = props(0, 0, 0, 0);
        let level = Sounds {
            identity: guac_ir::DumpIdentity {
                title_id: "BLUS30968".into(),
                app_ver: "01.00".into(),
                manifest_hash: guac_ir::Sha256([1; 32]),
            },
            archive: "level/level_213_tutotial.bld".into(),
            sounds: vec![
                sound(ONCE, 0, no_limit, here),
                sound(LOOP, 1, no_limit, here),
                sound(RESTARTS, 1 << 2, props(1, 0, 0, 0), here),
                sound(FAILS, 0, props(1, 0, 0, 0), here),
                sound(STEALS, 0, props(1, 3, 0, 0), here),
                sound(GLOBAL, 0, props(0, 0, 2, 0), here),
                sound(SILENT, 0, no_limit, DataRef::None),
                sound(STREAMED, 0, no_limit, DataRef::Here(DataHome::Level, 91)),
                sound(
                    MUSIC,
                    1,
                    no_limit | guac_sound::MUSIC << 1,
                    DataRef::Here(DataHome::Level, 91),
                ),
            ],
            data: [(90, None), (91, Some(-7))]
                .into_iter()
                .map(|(object, streamed)| SoundData {
                    home: DataHome::Level,
                    object,
                    name: Some("Levels/x.wav".into()),
                    raw_duration: 0.9,
                    streamed,
                    lip_sync: false,
                    audio: Some(Audio {
                        file: "audio/sounds/0011223344556677.mp3".into(),
                        rate: 48_000,
                        channels: 1,
                        samples: 48_000,
                        loop_points: None,
                    }),
                })
                .collect(),
            refused: Vec::new(),
        };
        SoundBook::new([level])
    }

    fn world() -> ScriptWorld {
        let handles = HashMap::new();
        let mut w = ScriptWorld::new(&Files::default(), &handles, headless_objects(&handles));
        for (h, x) in [(A, 0.0), (B, 3.0)] {
            let mut a = Actor::blank();
            a.position = Vec3::new(x, 0.0, 0.0);
            w.actors.insert(h, a);
        }
        w.set_sounds(
            Arc::new(book()),
            HashMap::from([(LEVEL, "level_213_tutotial".to_owned())]),
        );
        w
    }

    fn sound_value(object: u32) -> Value {
        Value::Object(Obj {
            unit: LEVEL,
            index: object,
        })
    }

    fn call(class: Class, on: Handle, what: Value) -> OpCall {
        OpCall {
            placement: on,
            receiver: Value::Entity(on),
            class,
            operands: vec![("_LHS", Some(what))],
            words: vec![("_dir", 0), ("_relOperator", 0)],
            sets: Vec::new(),
        }
    }

    fn start(w: &mut ScriptWorld, on: Handle, object: u32) -> Answer {
        w.op(
            &call(Class::StartSequence, on, sound_value(object)),
            &mut |_| 0,
            &mut NoEngine,
        )
        .unwrap()
    }

    fn started(answer: Answer) -> Value {
        match answer {
            Answer::Run { result, .. } => result.unwrap(),
            other => panic!("not started: {other:?}"),
        }
    }

    fn read(w: &mut ScriptWorld, ns: &str, name: &str, on: Value) -> Value {
        let at = Access {
            placement: 1,
            receiver: Some(on),
            member: MemberId::named(ns, name),
            other_point: None,
            via: None,
            op: None,
        };
        w.get(&at, &mut NoEngine).unwrap()
    }

    fn set(w: &mut ScriptWorld, ns: &str, name: &str, on: Value) -> Vec<Value> {
        let at = Access {
            placement: 1,
            receiver: Some(on),
            member: MemberId::named(ns, name),
            other_point: None,
            via: None,
            op: None,
        };
        w.elements(&at, &mut NoEngine).unwrap()
    }

    fn write(w: &mut ScriptWorld, ns: &str, name: &str, on: Option<Value>, v: Value) -> bool {
        let at = Access {
            placement: 1,
            receiver: on,
            member: MemberId::named(ns, name),
            other_point: None,
            via: None,
            op: None,
        };
        w.set(&at, v, &mut NoEngine).is_ok()
    }

    fn steps(w: &mut ScriptWorld, n: u32) {
        for _ in 0..n {
            w.advance(STEP, &mut NoEngine);
        }
    }

    fn ended(w: &ScriptWorld) -> Vec<(u32, &'static str)> {
        w.sounds_ended()
            .map(|e| (e.id.object, e.how.name()))
            .collect()
    }

    /// A start of a sound the book plays: a sequence on the actor of the
    /// recording's frames' length (1 s, not its build's 0.9), the play
    /// listed with its file and settings and its actor's place once the
    /// engine's frame has run; `time remaining` and `raw duration` read it
    /// on the game clock; it plays out after 60 steps, a loop never does.
    #[test]
    fn a_sound_plays_on_its_actor_for_its_recordings_length() {
        let mut w = world();
        let once = started(start(&mut w, A, ONCE));
        let looped = started(start(&mut w, B, LOOP));
        assert_eq!(w.plays().len(), 2);
        let p = &w.plays()[0];
        assert_eq!(p.file, "audio/sounds/0011223344556677.mp3");
        assert_eq!(
            (p.id.stem.as_str(), p.id.object),
            ("level_213_tutotial", ONCE)
        );
        assert_eq!((p.length, p.params.volume_percent, p.at), (1.0, 80.0, None));
        assert_eq!(
            read(&mut w, "sequence", "raw duration", once),
            Value::Float(1.0)
        );
        assert_eq!(read(&mut w, "sequence", "looping", looped), Value::Int(1));
        steps(&mut w, 30);
        assert_eq!(w.plays()[1].at, Some(Vec3::new(3.0, 0.0, 0.0)));
        let left = read(&mut w, "sequence", "time remaining", once)
            .as_f32()
            .unwrap();
        assert!((left - 0.5).abs() < 1e-4, "{left}");
        steps(&mut w, 31);
        assert_eq!(ended(&w), [(ONCE, "played out")]);
        assert_eq!(
            read(&mut w, "sequence", "time remaining", once),
            Value::Float(0.0)
        );
        steps(&mut w, 600);
        assert_eq!(w.plays().len(), 1, "the loop plays on");
        assert_eq!(w.sounds_started(), 2);
    }

    /// A stop of the sound info stops its copies on the receiver alone; a
    /// stop of a copy stops that copy; a copy whose actor is taken off
    /// stops with it. `playing sounds` holds each actor's copies, and the
    /// sound's `clones` every copy, until each ends.
    #[test]
    fn a_sound_stops_by_a_stop_or_its_actor_leaving() {
        let mut w = world();
        let on_a = started(start(&mut w, A, LOOP));
        let on_b = started(start(&mut w, B, LOOP));
        let once_b = started(start(&mut w, B, ONCE));
        assert_eq!(
            set(&mut w, "actorinfo", "playing sounds", Value::Entity(A)),
            [on_a]
        );
        assert_eq!(
            set(&mut w, "actorinfo", "playing sounds", Value::Entity(B)),
            [on_b, once_b]
        );
        assert_eq!(
            set(&mut w, "clonedsequence", "clones", sound_value(LOOP)),
            [on_a, on_b]
        );
        // A stop of the sound with no actor receiving it stops none.
        assert!(w.stop_sound(sound_value(LOOP), None));
        steps(&mut w, 1);
        assert!(w.sounds_ended().next().is_none(), "none stopped");
        w.op(
            &call(Class::StopSequence, A, sound_value(LOOP)),
            &mut |_| 0,
            &mut NoEngine,
        )
        .unwrap();
        steps(&mut w, 1);
        assert_eq!(ended(&w), [(LOOP, "stopped")]);
        assert_eq!(
            set(&mut w, "clonedsequence", "clones", sound_value(LOOP)),
            [on_b]
        );
        w.op(
            &call(Class::StopSequence, B, once_b),
            &mut |_| 0,
            &mut NoEngine,
        )
        .unwrap();
        steps(&mut w, 1);
        assert_eq!(ended(&w)[1], (ONCE, "stopped"));
        w.actors.get_mut(&B).unwrap().removed = true;
        steps(&mut w, 1);
        assert_eq!(ended(&w)[2], (LOOP, "actor left"));
        assert!(w.plays().is_empty());
        assert!(set(&mut w, "actorinfo", "playing sounds", Value::Entity(B)).is_empty());
        assert_eq!(
            read(&mut w, "sequence", "time remaining", on_b),
            Value::Float(0.0)
        );
    }

    /// The limits: a sound with `allow restart` playing on the actor starts
    /// over as a new play; one past an actor limit that fails is refused
    /// (the op skips its block), listed with why, and one on another actor
    /// plays; one that steals stops the oldest; a global limit of 2 holds
    /// across actors. A sound with no data is listed and plays as any other
    /// object (a sequence with no length); an object the book doesn't know
    /// isn't a sound.
    #[test]
    fn a_sounds_limits_refuse_steal_or_restart() {
        let mut w = world();
        let first = started(start(&mut w, A, RESTARTS));
        let serial = w.plays()[0].serial;
        steps(&mut w, 10);
        assert_eq!(started(start(&mut w, A, RESTARTS)), first, "the same copy");
        assert_ne!(w.plays()[0].serial, serial, "a new play");
        assert_eq!(ended(&w), [(RESTARTS, "stopped")]);

        started(start(&mut w, A, FAILS));
        assert_eq!(start(&mut w, A, FAILS), Answer::Skip);
        assert_eq!(
            w.sounds_refused()
                .last()
                .map(|r| (r.id.object, r.why.as_str())),
            Some((FAILS, "its limit is full on its actor"))
        );
        started(start(&mut w, B, FAILS));

        let old = started(start(&mut w, A, STEALS));
        let new = started(start(&mut w, A, STEALS));
        assert_ne!(old, new);
        assert_eq!(
            set(&mut w, "clonedsequence", "clones", sound_value(STEALS)),
            [new]
        );

        started(start(&mut w, A, GLOBAL));
        started(start(&mut w, B, GLOBAL));
        assert_eq!(start(&mut w, A, GLOBAL), Answer::Skip);
        assert_eq!(
            w.sounds_refused().last().map(|r| r.why.as_str()),
            Some("its limit is full everywhere")
        );

        let before = w.plays().len();
        let silent = started(start(&mut w, A, SILENT));
        assert_eq!(w.plays().len(), before, "no play");
        assert_eq!(
            w.sounds_refused()
                .last()
                .map(|r| (r.id.object, r.why.as_str())),
            Some((SILENT, "no data"))
        );
        assert_eq!(
            read(&mut w, "sequence", "raw duration", silent),
            Value::Float(0.0)
        );
        let refused = w.sounds_refused().count();
        started(start(&mut w, A, 999));
        assert_eq!(w.sounds_refused().count(), refused, "not a sound");
    }

    /// AR.10: a sound's members read the data until written (its volume,
    /// category, limits and flags as the part holds them), then what was
    /// written, for its next play and the one playing (a fade); a copy
    /// reads its sound's until its own is written, which is the copy's
    /// alone (`Sound - Ultra Sound`'s fade in from 0 to its sound's volume);
    /// the deprecated two read 0; an object the book doesn't know keeps a
    /// write and reads 0 before one; no object, no value.
    #[test]
    fn a_sounds_members_read_its_data_until_written() {
        let mut w = world();
        let once = sound_value(ONCE);
        assert_eq!(
            read(&mut w, "soundinfo", "volume", once),
            Value::Float(80.0)
        );
        assert_eq!(
            read(&mut w, "soundinfo", "rumble duration", once),
            Value::Float(0.9)
        );
        assert_eq!(
            read(&mut w, "soundinfo", "actor limit", sound_value(STEALS)),
            Value::Int(1)
        );
        assert_eq!(
            read(
                &mut w,
                "soundinfo",
                "actor limit behavior",
                sound_value(STEALS)
            ),
            Value::Int(3)
        );
        assert_eq!(
            read(&mut w, "soundinfo", "global limit", sound_value(GLOBAL)),
            Value::Int(2)
        );
        assert_eq!(
            read(&mut w, "soundinfo", "category", sound_value(MUSIC)),
            Value::Int(guac_sound::MUSIC as i32)
        );
        assert_eq!(
            read(&mut w, "soundinfo", "designer category", once),
            Value::Int(0)
        );
        let playing = started(start(&mut w, A, ONCE));
        assert_eq!(
            read(&mut w, "soundinfo", "volume", playing),
            Value::Float(80.0)
        );
        // The copy's volume to 0, as `Sound - Ultra Sound` starts a fade in:
        // the copy's alone, its sound still reading its own (the fade's
        // goal).
        assert!(write(
            &mut w,
            "soundinfo",
            "volume",
            Some(playing),
            Value::Float(0.0)
        ));
        assert_eq!(
            read(&mut w, "soundinfo", "volume", playing),
            Value::Float(0.0)
        );
        assert_eq!(
            read(&mut w, "soundinfo", "volume", once),
            Value::Float(80.0),
            "its sound's untouched"
        );
        let p = w.plays()[0].clone();
        assert_eq!(w.play_params(&p).volume_percent, 0.0, "the copy silent");
        // Its sound's written: the one playing that wrote its own keeps it.
        assert!(write(
            &mut w,
            "soundinfo",
            "volume",
            Some(once),
            Value::Float(20.0)
        ));
        assert_eq!(w.play_params(&p).volume_percent, 0.0, "the copy's own");
        let second = started(start(&mut w, A, ONCE));
        assert_eq!(
            read(&mut w, "soundinfo", "volume", second),
            Value::Float(20.0)
        );
        let p2 = w.plays()[1].clone();
        assert_eq!(
            w.play_params(&p2).volume_percent,
            20.0,
            "one playing with none of its own follows its sound's (a fade)"
        );
        assert!(write(
            &mut w,
            "soundinfo",
            "volume",
            Some(playing),
            Value::Float(20.0)
        ));
        assert!(write(
            &mut w,
            "soundinfo",
            "attenuates",
            Some(once),
            Value::Int(1)
        ));
        assert!(write(
            &mut w,
            "soundinfo",
            "attenuation distance full",
            Some(once),
            Value::Float(4.0)
        ));
        let falloff = w.play_params(&p).falloff.unwrap();
        assert_eq!((falloff.full, falloff.silent), (4.0, 150.0));
        started(start(&mut w, B, ONCE));
        assert_eq!(
            w.plays()[2].params.volume_percent,
            20.0,
            "the next starts so"
        );
        // Its limit written down to 1 refusing: the next on B is refused.
        assert!(write(
            &mut w,
            "soundinfo",
            "actor limit",
            Some(once),
            Value::Int(1)
        ));
        assert_eq!(start(&mut w, B, ONCE), Answer::Skip);
        let other = Value::Object(Obj { unit: 9, index: 1 });
        assert_eq!(read(&mut w, "soundinfo", "pan", other), Value::Float(0.0));
        assert!(write(
            &mut w,
            "soundinfo",
            "pan",
            Some(other),
            Value::Float(-40.0)
        ));
        assert_eq!(read(&mut w, "soundinfo", "pan", other), Value::Float(-40.0));
        assert!(!write(&mut w, "soundinfo", "pan", None, Value::Float(1.0)));
    }

    /// A sound's copy is no animation: `actorinfo.playing animations` holds
    /// an actor's clips alone and `playing sounds` his sound copies alone
    /// (a walker's locomotion reads the first, and took his footsteps for
    /// animations).
    #[test]
    fn a_sound_copy_is_no_animation() {
        let mut w = world();
        let template = Obj {
            unit: LEVEL,
            index: 77,
        };
        w.set_palette(template, vec![0.5]);
        let clip = w.objs.value(Object::Clip { template, slot: 0 });
        let clip_copy = started(
            w.op(
                &call(Class::StartSequence, A, clip),
                &mut |_| 0,
                &mut NoEngine,
            )
            .unwrap(),
        );
        let sound_copy = started(start(&mut w, A, LOOP));
        assert_eq!(
            set(&mut w, "actorinfo", "playing animations", Value::Entity(A)),
            [clip_copy]
        );
        assert_eq!(
            set(&mut w, "actorinfo", "playing sounds", Value::Entity(A)),
            [sound_copy]
        );
    }

    /// AR.10: the sound system's members read their start until written;
    /// its sets hold the copies playing, embedded and streamed apart, none
    /// of a movie, and take no write; its master volumes scale a play's
    /// gain by kind, music silenced while disabled; a copy's activator is
    /// its actor.
    #[test]
    fn the_sound_systems_members_scale_what_plays() {
        let mut w = world();
        let sys = |w: &mut ScriptWorld, name: &str| {
            let at = Access {
                placement: 1,
                receiver: None,
                member: MemberId::named("soundinfo", name),
                other_point: None,
                via: None,
                op: None,
            };
            w.get(&at, &mut NoEngine).unwrap()
        };
        assert_eq!(sys(&mut w, "music master volume"), Value::Float(100.0));
        assert_eq!(sys(&mut w, "reverb setting 1"), Value::Int(0));
        let effect = started(start(&mut w, A, ONCE));
        let stream = started(start(&mut w, A, STREAMED));
        let music = started(start(&mut w, B, MUSIC));
        let set = |w: &mut ScriptWorld, name: &str| {
            let at = Access {
                placement: 1,
                receiver: None,
                member: MemberId::named("soundinfo", name),
                other_point: None,
                via: None,
                op: None,
            };
            w.elements(&at, &mut NoEngine).unwrap()
        };
        assert_eq!(set(&mut w, "active sound effects"), [effect]);
        assert_eq!(set(&mut w, "active sound streams"), [stream, music]);
        assert!(set(&mut w, "active movies").is_empty());
        assert!(!write(
            &mut w,
            "soundinfo",
            "active sound streams",
            None,
            Value::Null
        ));
        let plays = w.plays().to_vec();
        assert_eq!(
            plays.iter().map(|p| w.master_gain(p)).collect::<Vec<_>>(),
            [1.0; 3]
        );
        for (name, v) in [
            ("effects master volume", 50.0),
            ("streams master volume", 25.0),
            ("music master volume", 80.0),
            ("streams master rate", 50.0),
        ] {
            assert!(write(&mut w, "soundinfo", name, None, Value::Float(v)));
        }
        assert_eq!(sys(&mut w, "music master volume"), Value::Float(80.0));
        assert_eq!(
            plays.iter().map(|p| w.master_gain(p)).collect::<Vec<_>>(),
            [0.5, 0.25, 0.8]
        );
        assert_eq!(w.master_rate(&plays[1]), 0.5);
        assert_eq!(w.master_rate(&plays[0]), 1.0);
        assert!(write(
            &mut w,
            "soundinfo",
            "music disabled",
            None,
            Value::Int(1)
        ));
        assert_eq!(w.master_gain(&plays[2]), 0.0);
        assert_eq!(
            read(&mut w, "clonedsequence", "activator", music),
            Value::Entity(B)
        );
        let at = Access {
            placement: 1,
            receiver: Some(sound_value(MUSIC)),
            member: MemberId::named("clonedsequence", "activator"),
            other_point: None,
            via: None,
            op: None,
        };
        assert_eq!(w.get(&at, &mut NoEngine), Err(HostFault::Nothing));
    }
}
