//! The host's half of a sound's play (openspec AR.9, AR.10): a `start
//! sequence` on a sound info the sounds part gives audio for, its limits,
//! its stop, its end on the game clock, and the members that read it.
//!
//! What a start does, each a reading of the data and the scripts' use of it
//! (`guac-sound`'s crate docs have the settings' meanings), none heard
//! against the original (the oracle runs silent; stand-in ledger:
//! `sound-play`):
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

use guac_script::{Answer, Handle, Obj, Value};
use guac_sound::{Limit, LimitBehavior, Lookup, SoundId};
use tracing::debug;

use crate::script_engine::{Engine, Object, Sequence};
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
        self.plays.push(SoundPlay::of(
            fresh_serial(),
            i,
            id,
            &p,
            length,
            self.sound_clock,
        ));
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
        let params = p.params;
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
            ],
            data: vec![SoundData {
                home: DataHome::Level,
                object: 90,
                name: Some("Levels/x.wav".into()),
                raw_duration: 0.9,
                streamed: None,
                lip_sync: false,
                audio: Some(Audio {
                    file: "audio/sounds/0011223344556677.mp3".into(),
                    rate: 48_000,
                    channels: 1,
                    samples: 48_000,
                    loop_points: None,
                }),
            }],
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
}
