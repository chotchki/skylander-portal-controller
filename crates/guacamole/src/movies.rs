//! The movie a level end queues, played on the TV (openspec AS.19): the
//! scripts' `Game - Level End - Play Movie` names one of the disc's
//! cinematics (chapter 1's outro, FMV02), which the library's level end
//! hands over before it asks its level ([`guacamole::level_end`],
//! [`guacamole::scripts::LevelEndMovie`]). Here it's converted the first
//! time it's played, by AS.9's `transcode_movie` into the extract, on a
//! thread of its own while the level holds (the portal's header saying so,
//! [`Movies::band`]), then played full screen on AB.10's player with its
//! sound, the level and its sounds held until it's over. The original's
//! skip ends it: △ or Start pressed on any pad, the two buttons the
//! original's movie fader reads off a joined player's controller (`action
//! 4` and `start button`), the fader's skipped branch left to the scripts
//! once the TV runs the end-of-level sequencing (AS.53).
//! (Stand-in ledger: `level-end`.)
//!
//! A movie that won't convert or open is over all the same, with why: the
//! level goes on as if it had played.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Instant;

use bevy::audio::{AudioSink, AudioSource};
use bevy::prelude::*;
use crossbeam_channel::{Receiver, TryRecvError};
use guac_extract::ExtractRoot;
use guac_extract::ffmpeg::Ffmpeg;
use guac_extract::transcode::{self, Done};
use guacamole::scripts::{MovieEnd, MovieState, ScriptWorld};
use serde::Serialize;
use skylander_core::SlotIndex;
use tracing::{info, warn};

use crate::extraction::{Band, ExtractBand, ExtractJob, ExtractStatus, Extraction};
use crate::intro::IntroEntry;
use crate::intro_player::{OutputLatency, Playback};
use crate::tv_state::Setup;

/// The level end's movie as the player has it.
#[derive(Resource, Default)]
pub(crate) struct Movies {
    current: Option<Running>,
    report: MovieReport,
    muted: bool,
}

enum Running {
    Converting {
        name: String,
        feed: Receiver<Converted>,
        started: Instant,
    },
    Playing {
        name: String,
        playback: Playback,
        started: Instant,
    },
}

/// What the conversion's thread hands back: the movie's two files and
/// what converting it did, or why it didn't.
type Converted = Result<(PathBuf, PathBuf, ConvertShot), String>;

/// What converts a movie by its name, on the conversion's thread: AS.9's
/// entry over the extract ([`convert`]) unless one is set (the tests', over
/// clips made in code).
#[derive(Resource, Clone)]
pub(crate) struct Converter(Arc<dyn Fn(&str) -> Converted + Send + Sync>);

/// The level end's movie, as `/api/_test/tv`'s `movie` reports it.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub(crate) struct MovieReport {
    /// The disc's name for it (`fmv02`); none before a level end queued one.
    pub name: Option<String>,
    /// `idle`, `converting`, `playing` or `over`.
    pub state: String,
    pub converted: Option<ConvertShot>,
    /// Frames shown and the video's count, once it opened.
    pub shown: usize,
    pub frames: Option<usize>,
    /// From its first frame's start to its end, milliseconds.
    pub played_ms: Option<u64>,
    /// Its sound as the player had it last: `waiting`, `starting`,
    /// `playing` (an output's sink) or `none`; and whether it was muted
    /// (a dev build's `GUAC_MUTE`).
    pub audio: Option<String>,
    pub muted: bool,
    /// `finished`, `skipped`, or `failed: <why>`.
    pub ended: Option<String>,
    /// How many have played to an end, a skip or a failure.
    pub plays: usize,
}

/// What converting the movie did: AS.9's entry converting it, or finding
/// it in the extract from an earlier play.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub(crate) struct ConvertShot {
    /// False when the extract held it already.
    pub transcoded: bool,
    /// The MP4's and the MP3's bytes together.
    pub bytes: u64,
    /// From the ask to the files in, milliseconds.
    pub ms: u64,
}

impl Movies {
    /// Whether the level holds for it: converting or playing.
    pub(crate) fn holds(&self) -> bool {
        self.current.is_some()
    }

    pub(crate) fn report(&self) -> &MovieReport {
        &self.report
    }

    /// What the portal's header draws while it converts, as a level's
    /// fetch's does; none otherwise.
    pub(crate) fn band(&self) -> Option<Band> {
        match &self.current {
            Some(Running::Converting { name, .. }) => Some(Band {
                title: format!("Getting {} ready", name.to_ascii_uppercase()),
                detail: "Converting the movie off your disc…".into(),
                percent: None,
                code: None,
            }),
            _ => None,
        }
    }
}

pub(crate) struct MoviesPlugin;

impl Plugin for MoviesPlugin {
    fn build(&self, app: &mut App) {
        // Dev only: muted on a machine nobody is listening to, as intros.
        let muted = cfg!(feature = "dev-tools") && std::env::var_os("GUAC_MUTE").is_some();
        app.insert_resource(Movies {
            muted,
            ..Movies::default()
        })
        .add_systems(Update, (play_movies, show_band).chain());
    }
}

/// The header's band while the movie converts ([`Movies::band`]), then
/// what the extraction leaves it, as a level's fetch hands it back.
fn show_band(
    movies: Res<Movies>,
    extraction: Res<Extraction>,
    mut drawn: ResMut<ExtractBand>,
    mut held: Local<bool>,
) {
    if let Some(band) = movies.band() {
        drawn.set_if_neq(ExtractBand(Some(band)));
        *held = true;
    } else if *held {
        drawn.set_if_neq(ExtractBand(crate::extraction::band(&extraction.status)));
        *held = false;
    }
}

/// The level end's movie taken up once it's queued and the extract is free
/// (the pick's extraction ended), converted on a thread, played, and handed
/// back over when it ends, is skipped or fails (the module docs).
#[allow(clippy::too_many_arguments)]
fn play_movies(
    mut commands: Commands,
    mut movies: ResMut<Movies>,
    world: Option<ResMut<ScriptWorld>>,
    (job, setup, extraction, converter): (
        Option<Res<ExtractJob>>,
        Res<Setup>,
        Res<Extraction>,
        Option<Res<Converter>>,
    ),
    latency: Res<OutputLatency>,
    mut images: ResMut<Assets<Image>>,
    mut sounds: Option<ResMut<Assets<AudioSource>>>,
    sinks: Query<&AudioSink>,
    pads: Query<&Gamepad>,
) {
    let Some(mut world) = world else {
        return;
    };
    let now = Instant::now();
    if movies.current.is_none() {
        let Some(m) = world
            .level_end_movie()
            .filter(|m| m.state == MovieState::Queued)
        else {
            return;
        };
        let name = m.name.clone();
        if !extraction_ended(&extraction) {
            return;
        }
        world.movie_playing();
        let converter: Converter = match (
            converter.as_deref(),
            job.as_deref(),
            setup.0.giants_dir.clone(),
        ) {
            (Some(c), _, _) => c.clone(),
            (None, Some(job), Some(dump)) => {
                let job = job.clone();
                Converter(Arc::new(move |name: &str| convert(&dump, &job, name)))
            }
            (None, _, _) => {
                over(
                    &mut movies,
                    &mut world,
                    MovieEnd::Failed("no Giants dump or extract to read it from".into()),
                );
                return;
            }
        };
        let (tx, feed) = crossbeam_channel::bounded(1);
        let thread_name = name.clone();
        let spawned = std::thread::Builder::new()
            .name("movie convert".into())
            .spawn(move || {
                let _ = tx.send((converter.0)(&thread_name));
            });
        if let Err(e) = spawned {
            over(
                &mut movies,
                &mut world,
                MovieEnd::Failed(format!("its thread didn't start: {e}")),
            );
            return;
        }
        info!(movie = %name, "the level end's movie: converting it if it isn't in");
        movies.report = MovieReport {
            name: Some(name.clone()),
            state: "converting".into(),
            plays: movies.report.plays,
            ..MovieReport::default()
        };
        movies.current = Some(Running::Converting {
            name,
            feed,
            started: now,
        });
        return;
    }
    let muted = movies.muted;
    match movies.current.take() {
        Some(Running::Converting {
            name,
            feed,
            started,
        }) => match feed.try_recv() {
            Ok(Ok((mp4, mp3, shot))) => {
                info!(movie = %name, ?shot, "the level end's movie: playing it");
                movies.report.converted = Some(shot);
                movies.report.state = "playing".into();
                let entry = IntroEntry {
                    id: u64::MAX - movies.report.plays as u64,
                    slot: SlotIndex::new(0).expect("slot 0"),
                    figure_id: String::new(),
                    movie: name.clone(),
                };
                let playback = Playback::open(&mut commands, entry, Some((mp4, mp3)), now);
                movies.current = Some(Running::Playing {
                    name,
                    playback,
                    started: now,
                });
            }
            Ok(Err(why)) => {
                warn!(movie = %name, reason = %why, "the level end's movie wouldn't convert");
                over(&mut movies, &mut world, MovieEnd::Failed(why));
            }
            Err(TryRecvError::Empty) => {
                movies.current = Some(Running::Converting {
                    name,
                    feed,
                    started,
                });
            }
            Err(TryRecvError::Disconnected) => over(
                &mut movies,
                &mut world,
                MovieEnd::Failed("its conversion's thread ended".into()),
            ),
        },
        Some(Running::Playing {
            name,
            mut playback,
            started,
        }) => {
            let skip = pads.iter().any(|p| {
                p.just_pressed(GamepadButton::North) || p.just_pressed(GamepadButton::Start)
            });
            let over_now = playback.step_movie(
                now,
                latency.0,
                muted,
                &mut commands,
                &mut images,
                sounds.as_deref_mut(),
                &sinks,
            );
            let progress = playback.progress();
            movies.report.shown = progress.0;
            movies.report.frames = progress.1;
            movies.report.audio = Some(playback.audio().to_owned());
            movies.report.muted = muted;
            let end = match (skip, over_now) {
                (true, _) => Some(MovieEnd::Skipped),
                (false, Some(Ok(()))) => Some(MovieEnd::Finished),
                (false, Some(Err(why))) => Some(MovieEnd::Failed(why)),
                (false, None) => None,
            };
            let Some(end) = end else {
                movies.current = Some(Running::Playing {
                    name,
                    playback,
                    started,
                });
                return;
            };
            movies.report.played_ms = Some(started.elapsed().as_millis() as u64);
            playback.stop(&mut commands, &mut images, &sinks, &format!("{end:?}"));
            info!(movie = %name, ?end, "the level end's movie over");
            over(&mut movies, &mut world, end);
        }
        None => {}
    }
}

/// The movie over: reported, the level end told.
fn over(movies: &mut Movies, world: &mut ScriptWorld, end: MovieEnd) {
    movies.current = None;
    movies.report.state = "over".into();
    movies.report.plays += 1;
    movies.report.ended = Some(match &end {
        MovieEnd::Finished => "finished".into(),
        MovieEnd::Skipped => "skipped".into(),
        MovieEnd::Unplayed => "unplayed".into(),
        MovieEnd::Failed(why) => format!("failed: {why}"),
    });
    world.movie_over(end);
}

/// Whether the Giants pick's extraction has ended, done or not: it owns
/// the extract while it runs.
fn extraction_ended(e: &Extraction) -> bool {
    e.feed.is_none()
        && matches!(
            e.status,
            ExtractStatus::Ready { .. } | ExtractStatus::Failed(_)
        )
}

/// Opens the extract of `dump` as a pick does and converts the cinematic
/// `name` if it isn't in (AS.9's entry): its two files and what it did.
/// Blocking.
fn convert(dump: &Path, job: &ExtractJob, name: &str) -> Converted {
    let started = Instant::now();
    let root = ExtractRoot::resolve(job.root.clone()).map_err(|e| e.to_string())?;
    let opened =
        guac_extract::open(dump, &root, &job.known(), &|_| {}).map_err(|e| e.to_string())?;
    let mut dir = opened.dir;
    let ffmpeg = Ffmpeg::locate(job.ffmpeg.as_deref()).map_err(|e| e.to_string())?;
    let done =
        transcode::transcode_movie(&mut dir, name, &ffmpeg, &|_| {}).map_err(|e| e.to_string())?;
    let shot = ConvertShot {
        transcoded: done.done == Done::Transcoded,
        bytes: done.bytes,
        ms: started.elapsed().as_millis() as u64,
    };
    Ok((
        dir.path().join(&done.video),
        dir.path().join(&done.audio),
        shot,
    ))
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::time::Duration;

    use guac_server::config::WindowMode;
    use guacamole::scripts::{Files, headless_objects};

    use super::*;
    use crate::cards::PortalCards;
    use crate::test_support::tv::portal_app;
    use crate::test_support::video::test_clip;

    /// A TV with the movie player, the extraction over, a script world, and
    /// `converter` in AS.9's entry's place.
    fn tv(converter: Converter) -> App {
        let mut app = portal_app(1920, 1080, WindowMode::Tv, PortalCards::default());
        let handles = HashMap::new();
        app.insert_resource(Extraction {
            status: ExtractStatus::Ready {
                intros: 0,
                ran: false,
            },
            ..Default::default()
        })
        .init_resource::<ExtractBand>()
        .init_resource::<OutputLatency>()
        .insert_resource(converter)
        .insert_resource(ScriptWorld::new(
            &Files::default(),
            &handles,
            headless_objects(&handles),
        ))
        .add_plugins(MoviesPlugin);
        app.update();
        app
    }

    /// A converter over a clip made in code (`frames` at 25 fps, no
    /// sound), counting its asks, or one that refuses with `why`.
    fn clip(dir: &Path, frames: usize) -> Converter {
        let mp4 = dir.join("fmv02.mp4");
        std::fs::write(&mp4, test_clip(64, 36, frames, 25)).unwrap();
        let mp3 = dir.join("fmv02.mp3");
        Converter(Arc::new(move |name: &str| {
            assert_eq!(name, "fmv02");
            Ok((
                mp4.clone(),
                mp3.clone(),
                ConvertShot {
                    transcoded: true,
                    bytes: 64,
                    ms: 1,
                },
            ))
        }))
    }

    fn state(app: &App) -> Option<MovieState> {
        app.world()
            .resource::<ScriptWorld>()
            .level_end_movie()
            .map(|m| m.state.clone())
    }

    /// Frames until `done` holds, up to 5 s.
    fn until(app: &mut App, what: &str, done: impl Fn(&App) -> bool) {
        let start = Instant::now();
        while !done(app) {
            assert!(start.elapsed() < Duration::from_secs(5), "{what}");
            app.update();
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    /// AS.19: a movie the level end queued is taken up, held for (the
    /// level holds and the header says so while it converts), played full
    /// screen to its last frame and handed back over finished, the report
    /// saying what converting it did; nothing is taken up before one is
    /// queued.
    #[test]
    fn a_queued_movie_converts_plays_to_its_end_and_hands_back() {
        let tmp = tempfile::tempdir().unwrap();
        let mut app = tv(clip(tmp.path(), 10));
        for _ in 0..3 {
            app.update();
        }
        assert!(!app.world().resource::<Movies>().holds(), "none queued");
        app.world_mut()
            .resource_mut::<ScriptWorld>()
            .queue_movie("fmv02");
        app.update();
        assert_eq!(state(&app), Some(MovieState::Playing));
        assert!(app.world().resource::<Movies>().holds());
        until(&mut app, "the movie over", |app| {
            matches!(state(app), Some(MovieState::Over(_)))
        });
        assert_eq!(state(&app), Some(MovieState::Over(MovieEnd::Finished)));
        let movies = app.world().resource::<Movies>();
        assert!(!movies.holds());
        let r = movies.report();
        assert_eq!(
            (r.name.as_deref(), r.state.as_str(), r.ended.as_deref()),
            (Some("fmv02"), "over", Some("finished"))
        );
        assert_eq!((r.shown, r.frames, r.plays), (10, Some(10), 1));
        assert!(r.converted.as_ref().is_some_and(|c| c.transcoded));
    }

    /// AS.19: the original's skip ends it: △ (or Start) pressed on a pad
    /// while it plays hands it back skipped, its overlay gone.
    #[test]
    fn a_press_of_the_skip_ends_the_movie() {
        let tmp = tempfile::tempdir().unwrap();
        let mut app = tv(clip(tmp.path(), 250));
        app.world_mut()
            .resource_mut::<ScriptWorld>()
            .queue_movie("fmv02");
        until(&mut app, "the movie playing", |app| {
            app.world().resource::<Movies>().report().shown > 0
        });
        let pad = crate::test_support::test_pad::connect(&mut app, None, None);
        app.update();
        assert_eq!(
            state(&app),
            Some(MovieState::Playing),
            "connecting skips nothing"
        );
        crate::test_support::test_pad::press(&mut app, pad, GamepadButton::North);
        assert_eq!(state(&app), Some(MovieState::Over(MovieEnd::Skipped)));
        let r = app.world().resource::<Movies>().report();
        assert!(r.shown < 250, "ended early: {r:?}");
        assert_eq!(r.ended.as_deref(), Some("skipped"));
        let overlays = app
            .world_mut()
            .query_filtered::<(), With<crate::intro_player::IntroOverlay>>()
            .iter(app.world())
            .count();
        assert_eq!(overlays, 0, "its overlay taken down");
    }

    /// AS.19: one that won't convert is over all the same, with why, so the
    /// level goes on; the header said so while it converted.
    #[test]
    fn a_movie_that_wont_convert_is_over_with_why() {
        let (gate, open) = crossbeam_channel::bounded::<()>(1);
        let mut app = tv(Converter(Arc::new(move |_: &str| {
            let _ = open.recv();
            Err("no ffmpeg".into())
        })));
        app.world_mut()
            .resource_mut::<ScriptWorld>()
            .queue_movie("fmv02");
        app.update();
        app.update();
        let band = app.world().resource::<ExtractBand>().0.clone();
        assert!(
            band.as_ref()
                .is_some_and(|b| b.title == "Getting FMV02 ready"),
            "{band:?}"
        );
        gate.send(()).unwrap();
        until(&mut app, "the movie over", |app| {
            matches!(state(app), Some(MovieState::Over(_)))
        });
        assert_eq!(
            state(&app),
            Some(MovieState::Over(MovieEnd::Failed("no ffmpeg".into())))
        );
        app.update();
        assert_eq!(
            app.world().resource::<ExtractBand>().0,
            None,
            "the header back"
        );
    }
}
