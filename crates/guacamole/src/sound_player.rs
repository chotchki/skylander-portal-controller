//! The original's sounds heard on the TV (openspec AR.9): each sound the
//! scripts play (the library's [`guacamole::sounds`], the game's half) gets
//! a voice through Bevy audio while the TV shows the level, its file read
//! off the extract once and shared, its volume and pitch randomness drawn
//! on a cosmetic generator of its own ([`SoundRng`]: nothing here reaches
//! the session's generator, so a sound moves nothing that plays, AH.87's
//! rule), its gain each frame the volume times its falloff at the camera
//! and its pan where its actor stands to the camera (the listener a
//! hypothesis, `guac-sound`'s crate docs; stand-in ledger: `sound-play`).
//!
//! The game's side decides what plays and for how long, on the game clock:
//! a voice follows it, never the other way. A play that ends by a stop, a
//! restart, a limit or its actor's leaving stops its voice at once; one that
//! plays out lets its voice ring to its own end. A paused or held level
//! pauses every voice. A muted dev build (`GUAC_MUTE`) and a TV with no
//! audio output make no voice at all, and everything the scripts read of a
//! sound runs the same.

use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::sync::Arc;

use bevy::audio::{
    AddAudioSource, AudioPlayer, AudioSink, AudioSinkPlayback, PlaybackMode, PlaybackSettings,
    Volume,
};
use bevy::prelude::*;
use bevy::tasks::futures::check_ready;
use bevy::tasks::{IoTaskPool, Task};
use guacamole::particles::Cosmetic;
use guacamole::script_engine::FEET_PER_METRE;
use guacamole::scripts::ScriptWorld;
use guacamole::sounds::SoundPlay;
use guacamole::voice::{Gains, Voice, opens, stereo};
use serde::Serialize;
use tracing::{info, warn};

use crate::level::{Level, LevelCamera};
use crate::pad::LevelPads;
use crate::play::{Mode, Play};

/// Where every TV's sound draws start: fixed, so a run's input draws the
/// same straying each time.
pub(crate) const SEED: u64 = 0x0041_5239_534E_4453;

/// The sounds' cosmetic random numbers, apart from the session's generator.
#[derive(Resource, Debug)]
pub(crate) struct SoundRng(pub(crate) Cosmetic);

impl Default for SoundRng {
    fn default() -> Self {
        Self(Cosmetic::new(SEED))
    }
}

/// A file's bytes as the TV reads them, by the path relative to the extract.
enum File {
    Reading(Task<Result<Arc<[u8]>, String>>),
    Ready(Arc<[u8]>),
    Failed,
}

/// What the player keeps: whether it's muted, the files read or reading
/// for the run it last saw (openspec AR.18: let go when the run changes,
/// [`release`]) and how many it has let go, and the plays it has given a
/// voice (by serial), heard or rung out.
#[derive(Resource)]
pub(crate) struct SoundOut {
    muted: bool,
    files: HashMap<String, File>,
    voiced: HashSet<u64>,
    run: Option<u64>,
    released: usize,
}

impl SoundOut {
    fn new(muted: bool) -> Self {
        Self {
            muted,
            files: HashMap::new(),
            voiced: HashSet::new(),
            run: None,
            released: 0,
        }
    }
}

/// The run changed (a level's load, a reinstall: openspec AR.18): every
/// file let go but those `held` names, which a play of the run now going
/// still sounds; a file reading for no one is dropped with its read, and
/// one that didn't read goes whoever holds it, so a play asking for it
/// tries it again. A voice holds its own bytes, so one still ringing plays
/// on whatever goes.
fn release(out: &mut SoundOut, held: &HashSet<&str>) -> usize {
    let before = out.files.len();
    out.files
        .retain(|rel, f| !matches!(f, File::Failed) && held.contains(rel.as_str()));
    let gone = before - out.files.len();
    out.released += gone;
    gone
}

/// A voice: the play it sounds, its gains, its draws, whether it's paused.
#[derive(Component, Debug)]
pub(crate) struct Heard {
    run: u64,
    serial: u64,
    sequence: u32,
    gains: Arc<Gains>,
    /// Its draws, each in -1..=1: its volume's straying and its pitch's,
    /// laid on its settings as they stand each frame (a fade reaches it).
    volume_draw: f32,
    speed_draw: f32,
    /// Its play ended by playing out: it rings to its end, no longer
    /// followed.
    ringing: bool,
    paused: bool,
}

/// A sound playing, as `/api/_test/tv`'s `sounds` lists it.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub(crate) struct PlayingShot {
    pub name: Option<String>,
    pub stem: String,
    pub object: u32,
    /// The actor it plays on, by handle.
    pub actor: u64,
    pub file: String,
    /// Seconds in, its length and what the scripts read as left, on the
    /// game clock.
    pub time: f32,
    pub length: f32,
    pub left: f32,
    pub looping: bool,
    /// The engine frame it started on.
    pub started: u64,
    /// Whether a voice sounds it now, and the voice's left and right gains.
    pub heard: bool,
    pub gains: Option<[f32; 2]>,
}

/// A play that ended: the engine frames it spans and why it left.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub(crate) struct EndedShot {
    pub name: Option<String>,
    pub stem: String,
    pub object: u32,
    pub started: u64,
    pub ended: u64,
    pub how: &'static str,
}

/// A start that played nothing: the sound silent or a full limit, and when.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub(crate) struct RefusedShot {
    pub name: Option<String>,
    pub stem: String,
    pub object: u32,
    pub at: u64,
    pub why: String,
}

/// What `/api/_test/tv` reports of the sounds (openspec AR.9).
#[derive(Resource, Debug, Clone, Default, PartialEq, Serialize)]
pub(crate) struct SoundReport {
    /// A muted build, or no audio output: no voice at all.
    pub muted: bool,
    /// Voices are made now: unmuted, an output, the level shown.
    pub audible: bool,
    /// The archives and sounds the run's book holds; 0 with none read.
    pub archives: usize,
    pub sounds: usize,
    /// The run's engine frames, which the plays are stamped by.
    pub clock: u64,
    pub playing: Vec<PlayingShot>,
    /// Plays started since the run began.
    pub started: usize,
    /// The last plays that ended and the last starts that played nothing,
    /// newest last.
    pub ended: Vec<EndedShot>,
    pub refused: Vec<RefusedShot>,
    /// Voices sounding now, rung-out ones included.
    pub voices: usize,
    /// Files the player holds now, read or reading, and how many it has let
    /// go at a run's change (AR.18).
    pub files: usize,
    pub released: usize,
}

/// How many of the last plays ended and starts refused the report lists.
const SHOWN: usize = 32;

/// The last `n` of `items`.
fn last<T>(items: impl ExactSizeIterator<Item = T>, n: usize) -> impl Iterator<Item = T> {
    let skip = items.len().saturating_sub(n);
    items.skip(skip)
}

pub(crate) struct SoundPlayerPlugin;

impl Plugin for SoundPlayerPlugin {
    fn build(&self, app: &mut App) {
        // Dev only: no sound on a machine nobody is listening to.
        let muted = cfg!(feature = "dev-tools") && std::env::var_os("GUAC_MUTE").is_some();
        let output = app.is_plugin_added::<bevy::audio::AudioPlugin>();
        if output {
            app.add_audio_source::<Voice>();
        }
        app.insert_resource(SoundOut::new(muted || !output))
            .init_resource::<SoundRng>()
            .init_resource::<SoundReport>()
            .add_systems(
                PostUpdate,
                hear_sounds
                    .after(TransformSystems::Propagate)
                    .run_if(resource_exists::<ScriptWorld>),
            );
    }
}

/// The file at `rel` under `dir`, read on the IO pool the first time it's
/// asked for; its bytes once they're in.
fn file(out: &mut SoundOut, dir: Option<&Path>, rel: &str) -> Option<Arc<[u8]>> {
    let entry = out.files.entry(rel.to_owned()).or_insert_with(|| {
        let Some(dir) = dir else {
            return File::Failed;
        };
        let path = dir.join(rel);
        File::Reading(IoTaskPool::get().spawn(async move {
            let bytes: Arc<[u8]> = std::fs::read(&path)
                .map_err(|e| format!("{}: {e}", path.display()))?
                .into();
            if opens(&bytes) {
                Ok(bytes)
            } else {
                Err(format!("{}: won't open as audio", path.display()))
            }
        }))
    });
    if let File::Reading(task) = entry
        && let Some(done) = check_ready(task)
    {
        *entry = match done {
            Ok(bytes) => File::Ready(bytes),
            Err(why) => {
                warn!(file = rel, reason = %why, "a sound's audio didn't read: it plays silent");
                File::Failed
            }
        };
    }
    match entry {
        File::Ready(bytes) => Some(bytes.clone()),
        _ => None,
    }
}

/// A play's left and right gains now: its drawn volume, its falloff at the
/// listener and its pan from where its actor stands to the listener (a 3D
/// sound's) or its own pan percent (any other's).
fn gains_of(
    play: &SoundPlay,
    p: &guac_sound::Params,
    volume: f32,
    listener: Option<&GlobalTransform>,
) -> (f32, f32) {
    let local = match (play.at, listener) {
        (Some(at), Some(l)) => Some(l.affine().inverse().transform_point3(at)),
        _ => None,
    };
    let falloff = local.map_or(1.0, |v| p.gain_at(v.length() * FEET_PER_METRE));
    let pan = match local {
        Some(v) if p.three_d && v.length() > 1e-4 => v.x / v.length(),
        _ => p.pan_percent / 100.0,
    };
    stereo(volume * falloff, pan)
}

#[allow(clippy::too_many_arguments)]
fn hear_sounds(
    mut commands: Commands,
    world: Res<ScriptWorld>,
    mut out: ResMut<SoundOut>,
    mut rng: ResMut<SoundRng>,
    mut report: ResMut<SoundReport>,
    voices: Option<ResMut<Assets<Voice>>>,
    mut heard: Query<(Entity, &mut Heard, Option<&AudioSink>)>,
    (level, play, pads, time, cameras): (
        Res<Level>,
        Option<Res<Play>>,
        Option<Res<LevelPads>>,
        Res<Time<Virtual>>,
        Query<&GlobalTransform, With<LevelCamera>>,
    ),
) {
    let in_level = match (play.as_deref(), pads.as_deref()) {
        (Some(play), Some(pads)) => play.mode(pads) != Mode::Portal,
        _ => false,
    };
    let audible = !out.muted && voices.is_some() && in_level;
    let paused = time.is_paused();
    let listener = cameras.iter().next();
    let playing: HashMap<u64, &SoundPlay> = world.plays().iter().map(|p| (p.serial, p)).collect();
    // A new run: the files its plays don't hold go (AR.18).
    if out.run != Some(world.run()) {
        let held: HashSet<&str> = world.plays().iter().map(|p| p.file.as_str()).collect();
        let kept = held.len();
        let gone = release(&mut out, &held);
        if gone > 0 {
            info!(
                gone,
                kept, "a new run: the sounds' files no play holds let go"
            );
        }
        out.run = Some(world.run());
    }
    let mut gains_now: HashMap<u64, [f32; 2]> = HashMap::new();
    // The voices: followed, rung out, or stopped.
    for (e, mut h, sink) in &mut heard {
        let follows = h.run == world.run() && in_level;
        if let (true, Some(p)) = (follows, playing.get(&h.serial)) {
            let params = world.play_params(p);
            let volume = params.volume_for(h.volume_draw) * world.master_gain(p);
            let (l, r) = gains_of(p, &params, volume, listener);
            h.gains.set(l, r);
            gains_now.insert(h.serial, [l, r]);
            if let Some(s) = world.sequences().get(h.sequence as usize)
                && let Some(sink) = sink
            {
                let rate = (s.rate / 100.0).max(0.0) * world.master_rate(p);
                sink.set_speed(params.speed_for(h.speed_draw) * rate);
            }
        } else if follows
            && !h.ringing
            && world
                .sequences()
                .get(h.sequence as usize)
                .is_some_and(|s| s.done && !s.stopped)
        {
            h.ringing = true;
        } else if !h.ringing || !follows {
            if let Some(sink) = sink {
                sink.stop();
            }
            commands.entity(e).despawn();
            continue;
        }
        if h.paused != paused {
            if let Some(sink) = sink {
                if paused {
                    sink.pause();
                } else {
                    sink.play();
                }
            }
            h.paused = paused;
        }
    }
    // A voice for each play that has none, once its file's in.
    out.voiced.retain(|s| playing.contains_key(s));
    if audible && let Some(mut voices) = voices {
        let dir = level.extract_dir().map(Path::to_path_buf);
        for p in world.plays() {
            if out.voiced.contains(&p.serial) {
                continue;
            }
            let Some(bytes) = file(&mut out, dir.as_deref(), &p.file) else {
                continue;
            };
            let (volume_draw, speed_draw) = (rng.0.unit() * 2.0 - 1.0, rng.0.unit() * 2.0 - 1.0);
            let params = world.play_params(p);
            let volume = params.volume_for(volume_draw) * world.master_gain(p);
            // Its sequence's rate from the start, as each frame after sets
            // it: a play held at rate 0 (`Sound - Play With Delay`) starts
            // held, not a frame at full speed.
            let rate = world
                .sequences()
                .get(p.sequence as usize)
                .map_or(1.0, |s| (s.rate / 100.0).max(0.0));
            let speed = params.speed_for(speed_draw) * rate * world.master_rate(p);
            let (l, r) = gains_of(p, &params, volume, listener);
            let gains = Gains::new(l, r);
            let voice = voices.add(Voice {
                bytes,
                gains: gains.clone(),
                looped: p.params.looping,
            });
            commands.spawn((
                AudioPlayer(voice),
                PlaybackSettings {
                    mode: PlaybackMode::Despawn,
                    volume: Volume::Linear(1.0),
                    speed,
                    paused,
                    ..PlaybackSettings::ONCE
                },
                Heard {
                    run: world.run(),
                    serial: p.serial,
                    sequence: p.sequence,
                    gains,
                    volume_draw,
                    speed_draw,
                    ringing: false,
                    paused,
                },
                Name::new(format!("sound {}", p.name.as_deref().unwrap_or("?"))),
            ));
            gains_now.insert(p.serial, [l, r]);
            out.voiced.insert(p.serial);
        }
    }
    if report.started == 0 && world.sounds_started() > 0 {
        info!(audible, muted = out.muted, "the first sound plays");
    }
    *report = SoundReport {
        muted: out.muted,
        audible,
        archives: world.sound_book().map_or(0, |b| b.stems().count()),
        sounds: world.sound_book().map_or(0, |b| {
            b.stems()
                .filter_map(|s| b.file(s))
                .map(|f| f.sounds.len())
                .sum()
        }),
        clock: world.sound_clock(),
        playing: world
            .plays()
            .iter()
            .map(|p| {
                let s = world.sequences().get(p.sequence as usize);
                PlayingShot {
                    name: p.name.clone(),
                    stem: p.id.stem.clone(),
                    object: p.id.object,
                    actor: s.map_or(0, |s| s.actor),
                    file: p.file.clone(),
                    time: s.map_or(0.0, |s| s.time),
                    length: p.length,
                    left: s.map_or(0.0, |s| s.time_remaining()),
                    looping: s.is_some_and(|s| s.looping),
                    started: p.started,
                    heard: gains_now.contains_key(&p.serial),
                    gains: gains_now.get(&p.serial).copied(),
                }
            })
            .collect(),
        started: world.sounds_started(),
        ended: last(world.sounds_ended(), SHOWN)
            .map(|e| EndedShot {
                name: e.name.clone(),
                stem: e.id.stem.clone(),
                object: e.id.object,
                started: e.started,
                ended: e.ended,
                how: e.how.name(),
            })
            .collect(),
        refused: last(world.sounds_refused(), SHOWN)
            .map(|r| RefusedShot {
                name: r.name.clone(),
                stem: r.id.stem.clone(),
                object: r.id.object,
                at: r.at,
                why: r.why.clone(),
            })
            .collect(),
        voices: heard.iter().count(),
        files: out.files.len(),
        released: out.released,
    };
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use guac_ir::sound::{Audio, DataHome, DataRef, Sound, SoundData, Sounds};
    use guac_script::{Handle, Obj};
    use guac_sound::{Params, SoundBook, SoundId};
    use guacamole::script_engine::NoEngine;
    use guacamole::scripts::{Files, Placed, headless_handles, headless_objects};

    use super::*;

    const LEVEL: u16 = 4;
    const ONCE: u32 = 10;
    const LOOP: u32 = 11;
    /// A one-shot of a recording of its own (AR.18).
    const OTHER: u32 = 12;
    const FILE: &str = "audio/sounds/0011223344556677.mp3";
    const OTHER_FILE: &str = "audio/sounds/8899aabbccddeeff.mp3";
    const STEP: f32 = 1.0 / 60.0;

    fn sound(object: u32, flags: u32) -> Sound {
        Sound {
            object,
            name: Some(format!("snd_{object}")),
            data: DataRef::Here(DataHome::Level, 90),
            flags: 0x4000_0000 | flags,
            properties: 0,
            playback_mode: 0,
            playback_percent: 100.0,
            volume_percent: 50.0,
            volume_randomness_percent: 20.0,
            pitch_randomness_percent: 10.0,
            pan_percent: 0.0,
            attenuation_distance_full: 1.0,
            attenuation_distance_silent: 101.0,
            rumble_duration: 1.0,
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
        SoundBook::new([Sounds {
            identity: guac_ir::DumpIdentity {
                title_id: "BLUS30968".into(),
                app_ver: "01.00".into(),
                manifest_hash: guac_ir::Sha256([0; 32]),
            },
            archive: "level/level_213_tutotial.bld".into(),
            // A one-shot and a loop, each 3D and falling off, and a one-shot
            // of another recording.
            sounds: vec![
                sound(ONCE, 0x28),
                sound(LOOP, 0x29),
                Sound {
                    data: DataRef::Here(DataHome::Level, 91),
                    ..sound(OTHER, 0x28)
                },
            ],
            data: [(90, FILE), (91, OTHER_FILE)]
                .into_iter()
                .map(|(object, file)| SoundData {
                    home: DataHome::Level,
                    object,
                    name: None,
                    raw_duration: 1.0,
                    streamed: None,
                    lip_sync: false,
                    audio: Some(Audio {
                        file: file.into(),
                        rate: 48_000,
                        channels: 1,
                        samples: 48_000,
                        loop_points: None,
                    }),
                })
                .collect(),
            refused: Vec::new(),
        }])
    }

    /// The world the tests' TV runs: one actor, the book above.
    fn world() -> (ScriptWorld, Handle) {
        let obj = Obj {
            unit: LEVEL,
            index: 500,
        };
        let files = Files {
            placed: vec![Placed::named(obj, "thrower", None, 4)],
            ..Files::default()
        };
        let handles = headless_handles(&files);
        let mut world = ScriptWorld::new(&files, &handles, headless_objects(&handles));
        world.set_sounds(
            Arc::new(book()),
            HashMap::from([(LEVEL, "level_213_tutotial".to_owned())]),
        );
        (world, handles[&obj])
    }

    /// The TV's player over a world with one actor and the book above, its
    /// file's audio in an extract at `dir`; in the level or on the portal,
    /// muted or not.
    fn tv(dir: PathBuf, muted: bool, in_level: bool) -> (App, Handle) {
        let mut app = App::new();
        app.add_plugins((MinimalPlugins, AssetPlugin::default()));
        app.init_asset::<Voice>();
        for file in [FILE, OTHER_FILE] {
            let audio = dir.join(file);
            std::fs::create_dir_all(audio.parent().unwrap()).unwrap();
            std::fs::write(&audio, guacamole::voice::tests_support::silent_mp3(40)).unwrap();
        }
        let (world, actor) = world();
        let camera = app
            .world_mut()
            .spawn((LevelCamera, GlobalTransform::default()))
            .id();
        if in_level {
            app.insert_resource(Play::started_for_tests(camera))
                .insert_resource(LevelPads::default());
        }
        app.insert_resource(world)
            .insert_resource(Level::from_extract_for_tests(dir))
            .insert_resource(SoundOut::new(muted))
            .init_resource::<SoundRng>()
            .init_resource::<SoundReport>()
            .add_systems(PostUpdate, hear_sounds);
        (app, actor)
    }

    fn start(app: &mut App, actor: Handle, object: u32) -> u32 {
        let mut w = app.world_mut().resource_mut::<ScriptWorld>();
        w.play_sound_on(
            Obj {
                unit: LEVEL,
                index: object,
            },
            actor,
        )
        .expect("it plays");
        w.plays().last().unwrap().sequence
    }

    fn steps(app: &mut App, n: u32) {
        let mut w = app.world_mut().resource_mut::<ScriptWorld>();
        for _ in 0..n {
            w.advance(STEP, &mut NoEngine);
        }
    }

    fn voices(app: &mut App) -> Vec<(u64, bool, bool)> {
        let world = app.world_mut();
        world
            .query::<(&Heard, &AudioPlayer<Voice>)>()
            .iter(world)
            .map(|(h, _)| (h.serial, h.ringing, h.paused))
            .collect()
    }

    /// Updates until a voice is there (its file read on the IO pool).
    fn until_voiced(app: &mut App, n: usize) {
        for _ in 0..2000 {
            app.update();
            if voices(app).len() == n {
                return;
            }
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
        panic!("no voice");
    }

    /// A play in the level gets a voice once its file's in, on the
    /// sounds' own draws (the session's generator nowhere in reach), listed
    /// heard; a hold pauses it; a stop takes it away at the next frame.
    #[test]
    fn a_play_gets_a_voice_and_loses_it_when_stopped() {
        let tmp = tempfile::tempdir().unwrap();
        let (mut app, actor) = tv(tmp.path().to_path_buf(), false, true);
        let seq = start(&mut app, actor, LOOP);
        until_voiced(&mut app, 1);
        assert_eq!(app.world().resource::<Assets<Voice>>().len(), 1, "its own");
        assert_ne!(app.world().resource::<SoundRng>().0, Cosmetic::new(SEED));
        let report = app.world().resource::<SoundReport>().clone();
        assert!(report.audible && !report.muted);
        assert_eq!((report.archives, report.sounds, report.started), (1, 3, 1));
        assert!(report.playing[0].heard && report.playing[0].looping);
        app.world_mut().resource_mut::<Time<Virtual>>().pause();
        app.update();
        assert!(voices(&mut app)[0].2, "held, paused");
        app.world_mut().resource_mut::<Time<Virtual>>().unpause();
        app.update();
        assert!(!voices(&mut app)[0].2);
        app.world_mut()
            .resource_mut::<ScriptWorld>()
            .sequences_mut()[seq as usize]
            .stopped = true;
        steps(&mut app, 1);
        app.update();
        assert!(voices(&mut app).is_empty());
        let report = app.world().resource::<SoundReport>().clone();
        assert_eq!(report.ended[0].how, "stopped");
        assert!(report.playing.is_empty());
        // Its asset freed with it, nothing kept per play.
        app.update();
        assert_eq!(app.world().resource::<Assets<Voice>>().len(), 0);
    }

    /// The level held (the portal's alert or an arrival card): its voices
    /// pause and stay, the same voice going on once it's let go.
    #[test]
    fn a_held_level_pauses_its_voices_and_keeps_them() {
        let tmp = tempfile::tempdir().unwrap();
        let (mut app, actor) = tv(tmp.path().to_path_buf(), false, true);
        start(&mut app, actor, LOOP);
        until_voiced(&mut app, 1);
        let serial = voices(&mut app)[0].0;
        let hold = |app: &mut App, held: bool| {
            let mut pads = app.world_mut().resource_mut::<LevelPads>();
            pads.show();
            pads.hold(held);
            let mut time = app.world_mut().resource_mut::<Time<Virtual>>();
            if held {
                time.pause();
            } else {
                time.unpause();
            }
            app.update();
        };
        hold(&mut app, true);
        assert_eq!(voices(&mut app), [(serial, false, true)], "held, paused");
        hold(&mut app, false);
        assert_eq!(voices(&mut app), [(serial, false, false)], "the same voice");
    }

    /// A play held at rate 0 (`Sound - Play With Delay`) gets a voice held
    /// from the start, its speed its sequence's rate.
    #[test]
    fn a_held_play_starts_its_voice_held() {
        let tmp = tempfile::tempdir().unwrap();
        let (mut app, actor) = tv(tmp.path().to_path_buf(), false, true);
        let seq = start(&mut app, actor, LOOP);
        app.world_mut()
            .resource_mut::<ScriptWorld>()
            .sequences_mut()[seq as usize]
            .rate = 0.0;
        until_voiced(&mut app, 1);
        let world = app.world_mut();
        let speeds: Vec<f32> = world
            .query::<(&Heard, &PlaybackSettings)>()
            .iter(world)
            .map(|(_, s)| s.speed)
            .collect();
        assert_eq!(speeds, [0.0]);
    }

    /// One that plays out rings to its own end, followed no more; the
    /// TV leaving the level stops it.
    #[test]
    fn a_play_that_plays_out_rings_until_the_level_goes() {
        let tmp = tempfile::tempdir().unwrap();
        let (mut app, actor) = tv(tmp.path().to_path_buf(), false, true);
        start(&mut app, actor, ONCE);
        until_voiced(&mut app, 1);
        steps(&mut app, 61);
        app.update();
        assert_eq!(voices(&mut app).len(), 1);
        assert!(voices(&mut app)[0].1, "ringing");
        // Frames on, past the play's end on the game's side, it rings on.
        for _ in 0..3 {
            steps(&mut app, 1);
            app.update();
        }
        assert_eq!(voices(&mut app).len(), 1, "it rings on");
        assert_eq!(
            app.world().resource::<SoundReport>().ended[0].how,
            "played out"
        );
        app.world_mut().remove_resource::<Play>();
        app.update();
        assert!(voices(&mut app).is_empty());
    }

    /// Muted, or on the portal: no voice, the play listed as unheard and
    /// its game side running all the same.
    #[test]
    fn a_muted_or_portal_tv_makes_no_voice() {
        for (muted, in_level) in [(true, true), (false, false)] {
            let tmp = tempfile::tempdir().unwrap();
            let (mut app, actor) = tv(tmp.path().to_path_buf(), muted, in_level);
            start(&mut app, actor, ONCE);
            for _ in 0..50 {
                app.update();
                std::thread::sleep(std::time::Duration::from_millis(1));
            }
            assert!(
                voices(&mut app).is_empty(),
                "muted {muted}, in the level {in_level}"
            );
            let report = app.world().resource::<SoundReport>().clone();
            assert_eq!((report.muted, report.audible), (muted, false));
            assert!(!report.playing[0].heard);
            steps(&mut app, 61);
            app.update();
            let report = app.world().resource::<SoundReport>().clone();
            assert_eq!(report.ended[0].how, "played out");
            assert_eq!(app.world().resource::<SoundRng>().0, Cosmetic::new(SEED));
        }
    }

    /// The bytes the player holds for `rel`, once read.
    fn held_bytes(app: &App, rel: &str) -> Option<Arc<[u8]>> {
        match app.world().resource::<SoundOut>().files.get(rel)? {
            File::Ready(b) => Some(b.clone()),
            _ => None,
        }
    }

    fn held_files(app: &App) -> Vec<String> {
        let mut f: Vec<String> = app
            .world()
            .resource::<SoundOut>()
            .files
            .keys()
            .cloned()
            .collect();
        f.sort();
        f
    }

    /// AR.18: the files a run's plays read stay for the run, a play's
    /// ending letting none go; once the run changes, each one no play of
    /// the new run holds goes, one a new play holds stays (the same bytes,
    /// not read again), and a run with no plays keeps none. The report
    /// counts what's held and what went.
    #[test]
    fn a_new_run_lets_go_of_the_files_no_play_holds() {
        let tmp = tempfile::tempdir().unwrap();
        let (mut app, actor) = tv(tmp.path().to_path_buf(), false, true);
        start(&mut app, actor, LOOP);
        let other = start(&mut app, actor, OTHER);
        until_voiced(&mut app, 2);
        app.world_mut()
            .resource_mut::<ScriptWorld>()
            .sequences_mut()[other as usize]
            .stopped = true;
        steps(&mut app, 1);
        app.update();
        assert_eq!(voices(&mut app).len(), 1);
        assert_eq!(held_files(&app), [FILE, OTHER_FILE], "kept through the run");
        let report = app.world().resource::<SoundReport>().clone();
        assert_eq!((report.files, report.released), (2, 0));
        let loop_bytes = held_bytes(&app, FILE).unwrap();

        // A new run (a reinstall) whose first play is the loop's.
        let (mut next, actor) = world();
        next.play_sound_on(
            Obj {
                unit: LEVEL,
                index: LOOP,
            },
            actor,
        )
        .expect("it plays");
        app.insert_resource(next);
        app.update();
        assert_eq!(held_files(&app), [FILE], "the new run's play holds it");
        assert!(Arc::ptr_eq(&held_bytes(&app, FILE).unwrap(), &loop_bytes));
        let report = app.world().resource::<SoundReport>().clone();
        assert_eq!((report.files, report.released), (1, 1));
        until_voiced(&mut app, 1);

        // Another with nothing playing: every file goes.
        app.insert_resource(world().0);
        app.update();
        assert!(held_files(&app).is_empty());
        let report = app.world().resource::<SoundReport>().clone();
        assert_eq!((report.files, report.released), (0, 2));
    }

    /// AR.18: a file that didn't read is read again in the next run, even
    /// when a play of that run asks for it from its first frame (the first
    /// run's failure isn't carried into it).
    #[test]
    fn a_file_that_didnt_read_is_tried_again_in_the_next_run() {
        let tmp = tempfile::tempdir().unwrap();
        let (mut app, actor) = tv(tmp.path().to_path_buf(), false, true);
        let other = tmp.path().join(OTHER_FILE);
        let bytes = std::fs::read(&other).unwrap();
        std::fs::remove_file(&other).unwrap();
        start(&mut app, actor, OTHER);
        let failed = |app: &App| {
            matches!(
                app.world().resource::<SoundOut>().files.get(OTHER_FILE),
                Some(File::Failed)
            )
        };
        for _ in 0..2000 {
            app.update();
            if failed(&app) {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
        assert!(failed(&app), "the missing file's read failed");
        assert!(voices(&mut app).is_empty());

        // The file there again, a new run whose first play is its sound's.
        std::fs::write(&other, bytes).unwrap();
        let (mut next, actor) = world();
        next.play_sound_on(
            Obj {
                unit: LEVEL,
                index: OTHER,
            },
            actor,
        )
        .expect("it plays");
        app.insert_resource(next);
        until_voiced(&mut app, 1);
        assert!(held_bytes(&app, OTHER_FILE).is_some());
    }

    fn play_at(at: Option<Vec3>, flags: u32, pan: f32) -> SoundPlay {
        let mut s = sound(ONCE, flags);
        s.pan_percent = pan;
        SoundPlay {
            serial: 1,
            sequence: 0,
            id: SoundId {
                stem: "level_213_tutotial".into(),
                object: ONCE,
            },
            name: None,
            file: FILE.into(),
            params: Params::of(&s),
            length: 1.0,
            started: 0,
            at,
        }
    }

    /// The gains: full within the full distance, half way to silent at the
    /// middle, nothing past it; a 3D sound panned toward its side of the
    /// camera (the camera looks down -Z, so +X is its right); one that
    /// isn't by its pan percent; no place or no camera, centred and whole.
    #[test]
    fn a_plays_gains_are_its_falloff_and_its_side_of_the_camera() {
        let feet = |f: f32| f / FEET_PER_METRE;
        let camera = GlobalTransform::default();
        let right = play_at(Some(Vec3::new(feet(0.5), 0.0, 0.0)), 0x28, 0.0);
        assert_eq!(
            gains_of(&right, &right.params, 0.8, Some(&camera)),
            stereo(0.8, 1.0)
        );
        let middle = play_at(Some(Vec3::new(0.0, 0.0, -feet(51.0))), 0x28, 0.0);
        let (l, r) = gains_of(&middle, &middle.params, 1.0, Some(&camera));
        assert!((l - 0.5).abs() < 1e-4 && (r - 0.5).abs() < 1e-4, "{l} {r}");
        let gone = play_at(Some(Vec3::new(-feet(200.0), 0.0, 0.0)), 0x28, 0.0);
        assert_eq!(
            gains_of(&gone, &gone.params, 1.0, Some(&camera)),
            (0.0, 0.0)
        );
        let flat = play_at(Some(Vec3::new(feet(0.5), 0.0, 0.0)), 0x08, -50.0);
        assert_eq!(
            gains_of(&flat, &flat.params, 1.0, Some(&camera)),
            stereo(1.0, -0.5)
        );
        let turned = GlobalTransform::from(Transform::from_rotation(Quat::from_rotation_y(
            std::f32::consts::PI,
        )));
        assert_eq!(
            gains_of(&right, &right.params, 1.0, Some(&turned)),
            stereo(1.0, -1.0)
        );
        assert_eq!(
            gains_of(
                &play_at(None, 0x28, 0.0),
                &play_at(None, 0x28, 0.0).params,
                0.7,
                Some(&camera)
            ),
            (0.7, 0.7)
        );
        assert_eq!(gains_of(&right, &right.params, 0.7, None), (0.7, 0.7));
    }
}
