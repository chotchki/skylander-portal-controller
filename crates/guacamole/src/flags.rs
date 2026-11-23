//! A playtest's flags (openspec AS.440): one capture writes what a comment
//! needs to be worked from, into a folder of its own named by the time.
//! `test-hooks` builds only: [`Flags`] is added, the routes mounted and the
//! key and the pad's capture button read only there; anything else answers
//! no route at all, so the phone's field stays off.
//!
//! Three ways to ask: the pad's capture button (Apple's `Button Share`,
//! which [`crate::game_controller`] hands Bevy as [`FLAG_BUTTON`]), the
//! [`FLAG_KEY`] on the TV's window, and the phone's "flag this" field
//! (`POST /api/_test/flag`, `{"note": ".."}`), which takes a capture with
//! its note, or adds the note to the last one by [`joins`]' rule.
//!
//! A capture is a layer over what the mirror already reads: `capture.json`
//! holds `/api/_test/tv`'s snapshot whole plus the scripts' last steps, the
//! drawn log's last frames and the frame log's last seconds, with every
//! actor in reach of the hero or in the camera's view read off the scripts'
//! world beside it; `frame.png` is the frame (the window's, or the
//! offscreen target's under `GUAC_SHOT_DIR`); `log.txt` the log's last
//! lines; `note.txt` the notes. The folder is `GUAC_FLAG_DIR`, else
//! `playtest-flags` beside the extract root, refused inside a git work tree
//! as `GUAC_SHOT_DIR` is: a frame is game output.

use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use axum::extract::rejection::JsonRejection;
use bevy::camera::{Projection, RenderTarget};
use bevy::input::gamepad::GamepadButton;
use bevy::prelude::*;
use bevy::render::view::window::screenshot::{Screenshot, ScreenshotCaptured};
use guacamole::scripts::{ScriptRuntime, ScriptWorld};
use serde::{Deserialize, Serialize};
use tokio::sync::oneshot;
use tracing::{error, info, warn};

use crate::hero::{FollowCamera, HeroBody};
use crate::hero_scripts::{HeroRun, Join, ScriptLog, ScriptSource};
use crate::level::LevelCamera;
use crate::mirror::TvMirror;
use crate::play::Play;
use crate::tv_state::TvCamera;

/// The key on the TV's window that takes a capture.
pub(crate) const FLAG_KEY: KeyCode = KeyCode::KeyF;

/// The pad's capture button as Bevy reads it: Apple's `Button Share`
/// ([`crate::game_controller::button`]), the Switch Pro's capture button,
/// an Xbox pad's Share.
pub(crate) const FLAG_BUTTON: GamepadButton = GamepadButton::Other(0);

/// In the app while flags are taken ([`Flags`] adds it): the router's
/// "press any button" leaves the key and the capture button out then, so
/// flagging an intro doesn't skip it.
#[derive(Resource, Debug, Default)]
pub(crate) struct FlagsTaken;

/// A phone's note joins the last capture when that capture was the pad's
/// or the key's, took no phone note yet and was taken at most this long
/// before the note.
pub(crate) const JOIN_WINDOW: Duration = Duration::from_secs(60);

/// The log's last lines a capture keeps: a few seconds of a busy level's
/// logging, the minute before a press at a quiet one.
const LOG_LINES: usize = 200;

/// How far back the log's tail is read for them.
const LOG_TAIL_BYTES: u64 = 512 * 1024;

/// The scripts' last 2 s of steps, the walk's last second as drawn and the
/// frame log's last 10 s (a freeze reads as one long frame there).
const SCRIPT_STEPS: u64 = 120;
const DRAWN_FRAMES: u32 = 60;
const FRAME_LOG_MS: f64 = 10_000.0;

/// An actor this near the hero is in his reach, m.
const REACH_M: f32 = 15.0;

/// An actor in the camera's picture counts out to this far from it, m.
const VIEW_M: f32 = 80.0;

/// The most actors a capture lists, nearest the hero first.
const ACTORS_MAX: usize = 400;

/// A note's longest, in bytes.
pub(crate) const NOTE_MAX: usize = 4000;

/// The phone asks waiting for the TV at once; one past it is a 429, so a
/// flood (a stuck client, two phones) can't stack captures onto a frame.
pub(crate) const ASKS_MAX: usize = 4;

/// A note is text: a control character other than a line break or a tab
/// is refused (400), so nothing but text reaches `note.txt`.
pub(crate) fn note_ok(note: &str) -> bool {
    note.len() <= NOTE_MAX
        && !note
            .chars()
            .any(|c| c.is_control() && !matches!(c, '\n' | '\r' | '\t'))
}

/// How long the phone's request waits for the TV to take its capture.
const PHONE_WAIT: Duration = Duration::from_secs(5);

/// Who asked for a capture.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Source {
    /// A pad's capture button: the pad's name.
    Pad(String),
    Key,
    Phone,
}

impl Source {
    fn name(&self) -> &'static str {
        match self {
            Self::Pad(_) => "pad",
            Self::Key => "key",
            Self::Phone => "phone",
        }
    }
}

/// Why there's no flag folder: flags are off.
#[derive(Debug, PartialEq)]
pub(crate) enum FlagDirError {
    /// Neither `GUAC_FLAG_DIR` nor an extract root to sit beside.
    Unset,
    /// Inside a git work tree: refused before anything is made.
    InCheckout { dir: PathBuf, repo: PathBuf },
    /// Outside one, but it couldn't be made.
    NotMade { dir: PathBuf, why: String },
}

/// The flag folder: `GUAC_FLAG_DIR`, else `playtest-flags` in the extract
/// root's parent; made absolute, refused inside a git work tree, else made
/// with its parents.
pub(crate) fn flag_dir(
    flag_dir: Option<&str>,
    extract_dir: Option<&str>,
) -> Result<PathBuf, FlagDirError> {
    let dir = match (flag_dir.filter(|d| !d.is_empty()), extract_dir) {
        (Some(d), _) => PathBuf::from(d),
        (None, Some(x)) if !x.is_empty() => {
            let x = std::path::absolute(x).unwrap_or_else(|_| PathBuf::from(x));
            x.parent()
                .ok_or(FlagDirError::Unset)?
                .join("playtest-flags")
        }
        _ => return Err(FlagDirError::Unset),
    };
    let dir = std::path::absolute(&dir).unwrap_or(dir);
    if let Some(repo) = guac_extract::root::enclosing_git_tree(&dir) {
        return Err(FlagDirError::InCheckout { dir, repo });
    }
    match std::fs::create_dir_all(&dir) {
        Ok(()) => Ok(dir),
        Err(e) => Err(FlagDirError::NotMade {
            dir,
            why: e.to_string(),
        }),
    }
}

/// `at` as UTC (the log's clock): `2026-11-15T15-04-05.123Z`, colons left
/// out so the name is a folder on every system.
pub(crate) fn stamp(at: SystemTime) -> String {
    let d = at.duration_since(UNIX_EPOCH).unwrap_or_default();
    let secs = d.as_secs();
    let (y, m, day) = civil(secs.div_euclid(86_400) as i64);
    let s = secs % 86_400;
    format!(
        "{y:04}-{m:02}-{day:02}T{:02}-{:02}-{:02}.{:03}Z",
        s / 3600,
        s / 60 % 60,
        s % 60,
        d.subsec_millis()
    )
}

/// `at` as RFC 3339 UTC, the JSON's.
fn rfc3339(at: SystemTime) -> String {
    let s = stamp(at);
    let (date, time) = s.split_once('T').unwrap_or((&s, ""));
    format!("{date}T{}", time.replacen('-', ":", 2))
}

/// Days since 1970-01-01 as a proleptic Gregorian year, month and day
/// (Howard Hinnant's `civil_from_days`).
fn civil(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let month = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    let year = yoe + era * 400 + i64::from(month <= 2);
    (year, month, day)
}

/// A capture's folder name: its time and who asked, `-2`, `-3` and on
/// after it while `taken` says the name is in use.
pub(crate) fn folder_name(at: SystemTime, source: &Source, taken: impl Fn(&str) -> bool) -> String {
    let base = format!("{}-{}", stamp(at), source.name());
    if !taken(&base) {
        return base;
    }
    (2..)
        .map(|n| format!("{base}-{n}"))
        .find(|n| !taken(n))
        .expect("a free name")
}

/// The last capture, for [`joins`].
#[derive(Debug, Clone)]
pub(crate) struct LastCapture {
    pub(crate) folder: PathBuf,
    pub(crate) name: String,
    pub(crate) source: Source,
    pub(crate) at: SystemTime,
    /// A phone note is in it already.
    pub(crate) noted: bool,
}

/// The rule: a phone's note (non-empty) joins the last capture when that
/// one was the pad's or the key's, holds no phone note yet, and was taken
/// at most [`JOIN_WINDOW`] before the note (a clock that went back joins
/// nothing). Anything else takes a capture of its own. "The last" is the
/// newest, so a capture taken since ends the one before's window.
pub(crate) fn joins(last: Option<&LastCapture>, note: &str, now: SystemTime) -> bool {
    let Some(last) = last else {
        return false;
    };
    !note.trim().is_empty()
        && last.source != Source::Phone
        && !last.noted
        && now
            .duration_since(last.at)
            .is_ok_and(|since| since <= JOIN_WINDOW)
}

/// What a phone's ask came to.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub(crate) struct Taken {
    /// The capture's folder, by name only (no path leaves the server).
    pub(crate) folder: String,
    /// The note joined a pad's or key's capture.
    pub(crate) joined: bool,
}

/// A phone's ask, waiting for the TV's frame.
struct PhoneAsk {
    note: String,
    reply: oneshot::Sender<Taken>,
}

/// Shared with the routes: whether flags are on, and the phone's asks.
#[derive(Resource, Clone, Default)]
pub(crate) struct FlagAsks {
    on: Arc<AtomicBool>,
    asks: Arc<Mutex<Vec<PhoneAsk>>>,
    /// Key presses posted (`POST /api/_test/flag/key`), not yet fed in.
    keys: Arc<std::sync::atomic::AtomicUsize>,
    /// A capture's note prompt's own asks (openspec AS.805).
    #[cfg(any(feature = "test-hooks", test))]
    pub(crate) note: crate::capture_note::NoteAsks,
}

impl FlagAsks {
    pub(crate) fn set_on(&self, on: bool) {
        self.on.store(on, Ordering::Relaxed);
    }

    pub(crate) fn on(&self) -> bool {
        self.on.load(Ordering::Relaxed)
    }

    /// Queues a phone's ask, or none with [`ASKS_MAX`] already waiting.
    fn push(&self, note: String) -> Option<oneshot::Receiver<Taken>> {
        let mut asks = self.asks.lock().unwrap_or_else(|e| e.into_inner());
        if asks.len() >= ASKS_MAX {
            return None;
        }
        let (reply, rx) = oneshot::channel();
        asks.push(PhoneAsk { note, reply });
        Some(rx)
    }

    /// A phone's ask as the route queues it, for the note prompt's tests.
    #[cfg(test)]
    pub(crate) fn push_for_tests(&self, note: &str) -> oneshot::Receiver<Taken> {
        self.push(note.into()).expect("room")
    }

    fn take(&self) -> Vec<PhoneAsk> {
        std::mem::take(&mut *self.asks.lock().unwrap_or_else(|e| e.into_inner()))
    }
}

/// `POST /api/_test/flag`'s body.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct FlagBody {
    #[serde(default)]
    note: String,
}

/// `POST /api/_test/flag/key` feeds Bevy a press of [`FLAG_KEY`] and its
/// release the frame after, as the window's key would arrive, for a run
/// with nobody at the keyboard (202; 409 with flags off).
/// `GET /api/_test/flag` answers `{"on": bool}`: whether this TV takes
/// flags (its folder made); the phone shows its field only on a yes, and a
/// build without the route (the product, a plain guacamole) answers none.
/// `POST /api/_test/flag` (`{"note": ".."}`, the note optional, at most
/// [`NOTE_MAX`] bytes, no control character but a line break or a tab) takes a capture on the TV's next frame, or joins the
/// last one by [`joins`]' rule, and answers 201 with `{"folder": name,
/// "joined": bool}` once its folder and JSON are written; 409 with flags
/// off, 400 for any other body, 429 with [`ASKS_MAX`] asks already waiting,
/// 503 when the TV took nothing in 5 s (a folder it couldn't make included).
pub(crate) fn routes<S: Clone + Send + Sync + 'static>(asks: &FlagAsks) -> axum::Router<S> {
    use axum::http::StatusCode;
    let (get, post, key) = (asks.clone(), asks.clone(), asks.clone());
    axum::Router::new()
        .route(
            "/api/_test/flag/key",
            axum::routing::post(move || {
                let on = key.on();
                if on {
                    key.keys.fetch_add(1, Ordering::Relaxed);
                }
                async move {
                    if on {
                        StatusCode::ACCEPTED
                    } else {
                        StatusCode::CONFLICT
                    }
                }
            }),
        )
        .route(
            "/api/_test/flag",
            axum::routing::get(move || {
                let on = get.on();
                async move { axum::Json(serde_json::json!({ "on": on })) }
            })
            .post(move |body: Result<axum::Json<FlagBody>, JsonRejection>| {
                let asks = post.clone();
                async move {
                    let Ok(axum::Json(body)) = body else {
                        return StatusCode::BAD_REQUEST.into_response();
                    };
                    if !note_ok(&body.note) {
                        return StatusCode::BAD_REQUEST.into_response();
                    }
                    if !asks.on() {
                        return StatusCode::CONFLICT.into_response();
                    }
                    let Some(rx) = asks.push(body.note) else {
                        return StatusCode::TOO_MANY_REQUESTS.into_response();
                    };
                    match tokio::time::timeout(PHONE_WAIT, rx).await {
                        Ok(Ok(taken)) => (StatusCode::CREATED, axum::Json(taken)).into_response(),
                        _ => StatusCode::SERVICE_UNAVAILABLE.into_response(),
                    }
                }
            }),
        )
}

use axum::response::IntoResponse;

/// Takes the captures: the key, the pads' capture button and the phone's
/// asks, read in `Last` (the mirror's snapshot written this frame).
pub(crate) struct Flags {
    pub(crate) dir: PathBuf,
    pub(crate) log_dir: PathBuf,
    /// The routes' side: the phone's asks land here.
    pub(crate) asks: FlagAsks,
    /// A hand's capture asks for its note ([`crate::capture_note`],
    /// openspec AS.805): off on a run nobody sits at
    /// (`GUAC_TEST_POSTED_PADS_ONLY`).
    pub(crate) prompts: bool,
}

/// Where captures go and the last one taken.
#[derive(Resource)]
pub(crate) struct FlagState {
    dir: PathBuf,
    log_dir: PathBuf,
    pub(crate) last: Option<LastCapture>,
    #[cfg(any(feature = "test-hooks", test))]
    prompts: bool,
}

/// Where the captures are taken in `Last`, for what answers after them.
#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub(crate) struct TakeFlags;

/// [`FLAG_KEY`]'s press this frame was posted (`/api/_test/flag/key`),
/// not a hand's.
#[derive(Resource, Debug, Default)]
struct PostedKeyNow(bool);

impl Plugin for Flags {
    fn build(&self, app: &mut App) {
        info!(dir = %self.dir.display(), key = ?FLAG_KEY, prompts = self.prompts, "playtest flags on");
        app.insert_resource(FlagState {
            dir: self.dir.clone(),
            log_dir: self.log_dir.clone(),
            last: None,
            #[cfg(any(feature = "test-hooks", test))]
            prompts: self.prompts,
        })
        .insert_resource(self.asks.clone())
        .init_resource::<FlagsTaken>()
        .init_resource::<PostedKeyNow>()
        .add_systems(
            PreUpdate,
            feed_posted_keys.before(bevy::input::InputSystems),
        )
        .add_systems(Last, take_flags.in_set(TakeFlags));
        #[cfg(any(feature = "test-hooks", test))]
        app.insert_resource(self.asks.note.clone())
            .add_plugins(crate::capture_note::NotePlugin);
    }
}

/// An actor off the scripts' world, with what the TV drew of it.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub(crate) struct ActorShot {
    pub handle: u64,
    /// Its placement's name (a clone's template's).
    pub name: String,
    pub clone: bool,
    /// The script its placement names.
    pub script: Option<String>,
    /// `awake`, `asleep` or `gone`.
    pub state: &'static str,
    /// The behaviour its script is in, lowercased as the VM spells it.
    pub behaviour: Option<String>,
    /// Where it stands, metres, and its facing, degrees.
    pub at: [f32; 3],
    pub facing: f32,
    /// From the hero, m.
    pub distance: Option<f32>,
    pub in_reach: bool,
    pub in_view: bool,
    /// Something of it is shown (not hidden: its model's on, wherever the
    /// camera looks), and something of it drew this frame (the renderer's
    /// own read, so off-screen is false); none for an actor with no entity
    /// and nothing drawing it.
    pub shown: Option<bool>,
    pub drawn: Option<bool>,
    /// The clips playing on it: palette slot, seconds in, length (s at
    /// 100 %), rate (percent) and looping.
    pub clips: Vec<ClipShot>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub(crate) struct ClipShot {
    /// Its name in its model's palette (openspec AS.461); none where the TV
    /// holds no names for it.
    pub name: Option<String>,
    pub slot: u32,
    pub time: f32,
    pub length: f32,
    pub rate: f32,
    pub looping: bool,
}

/// The camera a capture reads: its eye, its look and up as drawn, the
/// picture's height in degrees.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub(crate) struct CameraShot {
    pub which: &'static str,
    pub eye: [f32; 3],
    pub forward: [f32; 3],
    pub up: [f32; 3],
    pub fov_y: Option<f32>,
}

/// Every actor in reach of `hero` or inside `view` (a point's place in the
/// picture as `[x, y]` in -1..1 and its distance from the eye, none behind
/// it), nearest the hero first, [`ACTORS_MAX`] at most.
pub(crate) fn actors_near(
    world: &ScriptWorld,
    vm: Option<&guac_script::Vm>,
    hero: Option<Vec3>,
    view: impl Fn(Vec3) -> Option<([f32; 2], f32)>,
    drawn: impl Fn(u64) -> Option<(bool, bool)>,
) -> Vec<ActorShot> {
    let mut out: Vec<ActorShot> = world
        .actors()
        .filter_map(|(h, a)| {
            let distance = hero.map(|p| p.distance(a.position));
            let in_reach = distance.is_some_and(|d| d <= REACH_M);
            let in_view = view(a.position).is_some_and(|([x, y], d)| {
                x.abs() <= 1.0 && y.abs() <= 1.0 && (0.0..=VIEW_M).contains(&d)
            });
            if !(in_reach || in_view) {
                return None;
            }
            let id = vm.and_then(|vm| vm.placement_of(h));
            Some(ActorShot {
                handle: h,
                name: world.name(h),
                clone: a.obj.is_none(),
                script: a
                    .obj
                    .or(a.template)
                    .and_then(|o| world.template(o))
                    .and_then(|p| p.script.clone()),
                state: if a.removed {
                    "gone"
                } else if a.active {
                    "awake"
                } else {
                    "asleep"
                },
                behaviour: vm
                    .zip(id)
                    .and_then(|(vm, id)| vm.behaviour(id))
                    .map(str::to_ascii_lowercase),
                at: a.position.to_array(),
                facing: a.angles.facing.rem_euclid(360.0),
                distance,
                in_reach,
                in_view,
                shown: drawn(h).map(|d| d.0),
                drawn: drawn(h).map(|d| d.1),
                clips: world
                    .sequences()
                    .iter()
                    .filter(|s| s.actor == h && s.plays())
                    .filter_map(|s| {
                        world.clip_of(s).map(|(template, slot)| ClipShot {
                            name: world.clip_name(template, slot).map(str::to_owned),
                            slot,
                            time: s.time,
                            length: s.length,
                            rate: s.rate,
                            looping: s.looping,
                        })
                    })
                    .collect(),
            })
        })
        .collect();
    out.sort_by(|a, b| {
        let d = |s: &ActorShot| s.distance.unwrap_or(f32::INFINITY);
        d(a).total_cmp(&d(b)).then(a.handle.cmp(&b.handle))
    });
    out.truncate(ACTORS_MAX);
    out
}

/// The log's last `lines` lines: the newest `server.log*` in `dir`, its
/// last [`LOG_TAIL_BYTES`] at most.
pub(crate) fn log_tail(dir: &Path, lines: usize) -> Result<Vec<String>, String> {
    let newest = std::fs::read_dir(dir)
        .map_err(|e| format!("{}: {e}", dir.display()))?
        .flatten()
        .filter(|e| e.file_name().to_string_lossy().starts_with("server.log"))
        .filter_map(|e| Some((e.metadata().ok()?.modified().ok()?, e.path())))
        .max()
        .map(|(_, p)| p)
        .ok_or_else(|| format!("no server.log in {}", dir.display()))?;
    let mut file = std::fs::File::open(&newest).map_err(|e| e.to_string())?;
    let len = file.metadata().map_err(|e| e.to_string())?.len();
    let from = len.saturating_sub(LOG_TAIL_BYTES);
    file.seek(SeekFrom::Start(from))
        .map_err(|e| e.to_string())?;
    let mut bytes = Vec::new();
    file.read_to_end(&mut bytes).map_err(|e| e.to_string())?;
    let text = String::from_utf8_lossy(&bytes);
    let mut all: Vec<&str> = text.lines().collect();
    // A tail read from mid-file starts mid-line.
    if from > 0 && !all.is_empty() {
        all.remove(0);
    }
    let start = all.len().saturating_sub(lines);
    Ok(all[start..].iter().map(|l| (*l).to_owned()).collect())
}

/// Appends a note, with its time, to the capture's `note.txt`.
pub(crate) fn add_note(folder: &Path, note: &str, at: SystemTime) -> std::io::Result<()> {
    use std::io::Write;
    let mut f = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(folder.join("note.txt"))?;
    writeln!(f, "[{}] {}", rfc3339(at), note.trim_end())
}

/// What a capture reads beside the mirror.
#[derive(bevy::ecs::system::SystemParam)]
struct CaptureReads<'w, 's> {
    mirror: Option<Res<'w, TvMirror>>,
    scripts: Option<Res<'w, ScriptLog>>,
    count: Res<'w, bevy::diagnostic::FrameCount>,
    world: Option<Res<'w, ScriptWorld>>,
    runtime: Option<Res<'w, ScriptRuntime>>,
    source: Option<Res<'w, ScriptSource>>,
    play: Option<Res<'w, Play>>,
    heroes: Query<'w, 's, (&'static HeroRun, &'static HeroBody)>,
    cameras: Query<
        'w,
        's,
        (
            &'static Camera,
            &'static GlobalTransform,
            &'static Projection,
            Has<FollowCamera>,
        ),
        Or<(With<FollowCamera>, With<LevelCamera>)>,
    >,
    visible: Query<
        'w,
        's,
        (
            &'static InheritedVisibility,
            &'static ViewVisibility,
            Option<&'static Children>,
        ),
    >,
    tv_target: Query<'w, 's, &'static RenderTarget, With<TvCamera>>,
    level: Option<Res<'w, crate::level::Level>>,
    creatures: Query<'w, 's, (Entity, &'static crate::creatures::DrawnCreature)>,
}

impl CaptureReads<'_, '_> {
    fn hero(&self) -> Option<Vec3> {
        let e = self.play.as_deref().and_then(Play::hero)?;
        self.heroes.get(e).ok().map(|(_, b)| b.hero.pivot)
    }

    fn camera(&self) -> Option<(&Camera, &GlobalTransform, &Projection, bool)> {
        self.cameras
            .iter()
            .filter(|(c, ..)| c.is_active)
            .max_by_key(|(c, ..)| c.order)
    }

    /// What draws each actor apart from its own entity: a creature's model
    /// (its own root), a file prop's (under the level's root), by handle.
    fn draws(&self) -> std::collections::HashMap<u64, Vec<Entity>> {
        let mut out: std::collections::HashMap<u64, Vec<Entity>> = Default::default();
        for (e, c) in &self.creatures {
            out.entry(c.actor).or_default().push(e);
        }
        let unit = self.source.as_deref().and_then(|s| s.level_unit());
        if let (Some(level), Some(world), Some(unit)) =
            (self.level.as_deref(), self.world.as_deref(), unit)
        {
            for (object, e) in level.prop_draws() {
                let obj = guac_script::Obj {
                    unit,
                    index: object,
                };
                if let Some(h) = world.handles().get(&obj) {
                    out.entry(*h).or_default().push(e);
                }
            }
        }
        out
    }

    /// Whether something of `h` is shown and whether it drew this frame:
    /// its entity, what [`Self::draws`] lists for it, or anything under
    /// them; none when it has no entity and nothing draws it.
    fn drawn(
        &self,
        h: u64,
        draws: &std::collections::HashMap<u64, Vec<Entity>>,
    ) -> Option<(bool, bool)> {
        let own = Entity::try_from_bits(h).filter(|e| self.visible.get(*e).is_ok());
        let mut stack: Vec<(Entity, u8)> = own
            .into_iter()
            .chain(draws.get(&h).into_iter().flatten().copied())
            .map(|e| (e, 0))
            .collect();
        if stack.is_empty() {
            return None;
        }
        let (mut shown, mut drawn) = (false, false);
        while let Some((e, depth)) = stack.pop() {
            let Ok((inherited, view, children)) = self.visible.get(e) else {
                continue;
            };
            shown |= inherited.get();
            drawn |= view.get();
            if shown && drawn {
                break;
            }
            if depth < 6
                && let Some(children) = children
            {
                stack.extend(children.iter().map(|c| (c, depth + 1)));
            }
        }
        Some((shown, drawn))
    }

    /// Everything but the frame, as `capture.json` holds it.
    fn json(&self, source: &Source, at: SystemTime, note: &str) -> serde_json::Value {
        let tv = self
            .mirror
            .as_deref()
            .map(|m| m.0.lock().unwrap_or_else(|e| e.into_inner()).clone());
        let step = self.scripts.as_deref().map_or(0, ScriptLog::last);
        let steps = self
            .scripts
            .as_deref()
            .map(|l| l.since(step.saturating_sub(SCRIPT_STEPS)))
            .unwrap_or_default();
        let (drawn, frames) = match self.mirror.as_deref() {
            Some(m) => {
                let drawn =
                    m.6.lock()
                        .unwrap_or_else(|e| e.into_inner())
                        .since(self.count.0.saturating_sub(DRAWN_FRAMES));
                let log = m.3.lock().unwrap_or_else(|e| e.into_inner());
                let frames = log.since(log.last_ms() - FRAME_LOG_MS);
                (
                    serde_json::to_value(drawn).ok(),
                    serde_json::to_value(frames).ok(),
                )
            }
            None => (None, None),
        };
        let camera = self.camera().map(|(_, t, p, follow)| CameraShot {
            which: if follow { "follow" } else { "level" },
            eye: t.translation().to_array(),
            forward: t.forward().to_array(),
            up: t.up().to_array(),
            fov_y: match p {
                Projection::Perspective(p) => Some(p.fov.to_degrees()),
                _ => None,
            },
        });
        let hero = self.hero();
        let draws = self.draws();
        let actors = self.world.as_deref().map(|w| {
            let vm = self.runtime.as_deref().map(|r| &r.vm);
            let cam = self.camera();
            actors_near(
                w,
                vm,
                hero,
                |p| {
                    let (c, t, ..) = cam?;
                    let ndc = c.world_to_ndc(t, p)?;
                    // Reversed depth: in front of the eye is z in (0, 1].
                    (ndc.z > 0.0).then(|| ([ndc.x, ndc.y], t.translation().distance(p)))
                },
                |h| self.drawn(h, &draws),
            )
        });
        let scene = match (self.runtime.as_deref(), self.world.as_deref()) {
            (Some(rt), Some(w)) => {
                self.source
                    .as_deref()
                    .and_then(|s| s.level_unit())
                    .map(|level| {
                        let seated = self
                            .play
                            .as_deref()
                            .and_then(Play::hero)
                            .and_then(|e| self.heroes.get(e).ok());
                        let pose = seated.map(|(_, b)| (b.hero.pivot, b.hero.facing));
                        // The joined hero by his pivot, not his actor's
                        // place, which stays where he joined.
                        let handle = seated.and_then(|(r, _)| match r.join {
                            Join::Joined(id) => rt.vm.handle_of(id),
                            _ => None,
                        });
                        let read = crate::watch::read(&rt.vm, w, level, &[], pose, handle, None);
                        serde_json::json!({
                            "cutscene_state": read.cutscene_state,
                            "minions": read.minions,
                        })
                    })
            }
            _ => None,
        };
        let summary = tv.as_ref().map(|tv| {
            serde_json::json!({
                "screen": tv.screen,
                "mode": tv.mode,
                "level": tv.level.as_ref().and_then(|l| l.level.clone()),
                "run_level": tv.run.as_ref().map(|r| r.level.clone()),
                "zone": tv.run.as_ref().map(|r| serde_json::json!({
                    "streams": r.streams,
                    "interiors": r.interiors,
                })),
                "hero": tv.hero.as_ref().map(|h| serde_json::json!({
                    "slot": h.slot,
                    "figure_id": h.figure_id,
                    "position": h.position,
                    "heading": h.heading,
                    "health": h.health,
                    "health_max": h.health_max,
                    "behaviour": h.behaviour,
                    "scripts": h.scripts,
                })),
                "stopped": tv.run.as_ref().map(|r| r.stopped.clone()),
                "level_end": tv.run.as_ref().map(|r| r.level_end.clone()),
                "up": {
                    "intro": tv.intro,
                    "overlay": tv.input.overlay,
                    "pause": tv.input.pause,
                    "alert": tv.alert,
                    "prompt": tv.prompt,
                    "announce": tv.announce,
                    "held_band": tv.held_band,
                    "movie": tv.movie,
                    "completion": tv.completion.is_some(),
                    "defeat": tv.defeat,
                },
            })
        });
        serde_json::json!({
            "taken": rfc3339(at),
            "taken_unix_ms": at.duration_since(UNIX_EPOCH).unwrap_or_default().as_millis() as u64,
            "source": source.name(),
            "pad": match source { Source::Pad(n) => Some(n.clone()), _ => None },
            "build": guac_server::http::BUILD_TOKEN,
            "frame": self.count.0,
            "note": (!note.trim().is_empty()).then(|| note.to_owned()),
            "summary": summary,
            "script_step": step,
            "camera": camera,
            "scene": scene,
            "actors": actors,
            "tv": tv,
            "scripts_recent": steps,
            "drawn_recent": drawn,
            "frames_recent": frames,
        })
    }

    /// The frame's target: the offscreen image under `GUAC_SHOT_DIR`, else
    /// the window.
    fn screenshot(&self) -> Screenshot {
        match self.tv_target.iter().next() {
            Some(RenderTarget::Image(t)) => Screenshot::image(t.handle.clone()),
            _ => Screenshot::primary_window(),
        }
    }
}

/// A posted key press into Bevy's keyboard stream, its release the frame
/// after: what winit hands Bevy for the window's key.
fn feed_posted_keys(
    asks: Option<Res<FlagAsks>>,
    window: Query<Entity, With<bevy::window::PrimaryWindow>>,
    mut keys: MessageWriter<bevy::input::keyboard::KeyboardInput>,
    mut down: Local<bool>,
    mut posted: ResMut<PostedKeyNow>,
) {
    use bevy::input::ButtonState;
    let Some(asks) = asks else {
        return;
    };
    let state = if *down {
        ButtonState::Released
    } else if asks.keys.swap(0, Ordering::Relaxed) > 0 {
        ButtonState::Pressed
    } else {
        return;
    };
    *down = state == ButtonState::Pressed;
    posted.0 = *down;
    keys.write(bevy::input::keyboard::KeyboardInput {
        key_code: FLAG_KEY,
        logical_key: bevy::input::keyboard::Key::Character("f".into()),
        state,
        text: None,
        repeat: false,
        window: window.iter().next().unwrap_or(Entity::PLACEHOLDER),
    });
}

/// The key, the capture buttons and the phone's asks, this frame. A
/// hand's press asks for its note ([`crate::capture_note`], openspec
/// AS.805); while that prompt is up the key types into it and the button
/// is its Save, so neither takes a capture.
#[allow(clippy::too_many_arguments)]
fn take_flags(
    mut commands: Commands,
    mut state: ResMut<FlagState>,
    keys: Option<Res<ButtonInput<KeyCode>>>,
    pads: Query<(Entity, &Gamepad, Option<&Name>)>,
    asks: Option<Res<FlagAsks>>,
    reads: CaptureReads,
    mut posted: ResMut<PostedKeyNow>,
    #[cfg(any(feature = "test-hooks", test))] mut note: crate::capture_note::NoteTake,
) {
    let posted = std::mem::take(&mut posted.0);
    let pressed = if keys.as_deref().is_some_and(|k| k.just_pressed(FLAG_KEY)) {
        Some((Source::Key, None))
    } else {
        pads.iter()
            .find(|(_, p, _)| p.just_pressed(FLAG_BUTTON))
            .map(|(e, _, n)| {
                (
                    Source::Pad(n.map_or_else(|| "pad".into(), |n| n.as_str().to_owned())),
                    Some(e),
                )
            })
    };
    #[cfg(any(feature = "test-hooks", test))]
    let pressed = pressed.filter(|_| !note.is_open());
    #[cfg(not(any(feature = "test-hooks", test)))]
    let _ = posted;
    if let Some((source, _by)) = pressed {
        let taken = capture(&mut commands, &mut state, &reads, source.clone(), "");
        #[cfg(any(feature = "test-hooks", test))]
        if let Some((name, shot)) = taken {
            let hand = note.hand_asked();
            if crate::capture_note::asks_for_note(&source, state.prompts, posted, hand) {
                let folder = state.dir.join(&name);
                note.open(&mut commands, folder, name, shot, _by);
            }
        }
        #[cfg(not(any(feature = "test-hooks", test)))]
        let _ = taken;
    }
    for ask in asks.map(|a| a.take()).unwrap_or_default() {
        let now = SystemTime::now();
        let taken = if joins(state.last.as_ref(), &ask.note, now) {
            let last = state.last.as_mut().expect("joins needs one");
            match add_note(&last.folder, &ask.note, now) {
                Ok(()) => info!(folder = %last.name, "a phone's note joined the capture"),
                Err(e) => error!(folder = %last.name, %e, "a phone's note wasn't written"),
            }
            last.noted = true;
            Some(Taken {
                folder: last.name.clone(),
                joined: true,
            })
        } else {
            capture(&mut commands, &mut state, &reads, Source::Phone, &ask.note).map(
                |(folder, _)| Taken {
                    folder,
                    joined: false,
                },
            )
        };
        if let Some(taken) = taken {
            let _ = ask.reply.send(taken);
        }
    }
}

/// Writes one capture's folder: `capture.json` and `log.txt` now, the note
/// if any, `frame.png` once the frame lands. Its name and the frame's
/// screenshot entity, or none when the folder couldn't be made.
fn capture(
    commands: &mut Commands,
    state: &mut FlagState,
    reads: &CaptureReads,
    source: Source,
    note: &str,
) -> Option<(String, Entity)> {
    let at = SystemTime::now();
    let name = folder_name(at, &source, |n| state.dir.join(n).exists());
    let folder = state.dir.join(&name);
    if let Err(e) = std::fs::create_dir_all(&folder) {
        error!(folder = %folder.display(), %e, "a flag's folder couldn't be made");
        return None;
    }
    let json = reads.json(&source, at, note);
    let written = serde_json::to_vec_pretty(&json)
        .map_err(|e| e.to_string())
        .and_then(|b| std::fs::write(folder.join("capture.json"), b).map_err(|e| e.to_string()));
    if let Err(e) = written {
        error!(folder = %folder.display(), %e, "a flag's capture.json wasn't written");
    }
    let log = match log_tail(&state.log_dir, LOG_LINES) {
        Ok(lines) => lines.join("\n") + "\n",
        Err(e) => format!("(no log: {e})\n"),
    };
    if let Err(e) = std::fs::write(folder.join("log.txt"), log) {
        warn!(folder = %folder.display(), %e, "a flag's log.txt wasn't written");
    }
    if !note.trim().is_empty()
        && let Err(e) = add_note(&folder, note, at)
    {
        error!(folder = %folder.display(), %e, "a flag's note wasn't written");
    }
    let png = folder.join("frame.png");
    let shot = commands
        .spawn(reads.screenshot())
        .observe(move |captured: On<ScreenshotCaptured>| {
            let (image, path) = (captured.image.clone(), png.clone());
            std::thread::spawn(move || match save_png(&image, &path) {
                Ok(()) => info!(path = %path.display(), "a flag's frame saved"),
                Err(why) => error!(path = %path.display(), %why, "a flag's frame not saved"),
            });
        })
        .id();
    info!(folder = %folder.display(), source = source.name(), "flag captured");
    state.last = Some(LastCapture {
        folder,
        name: name.clone(),
        noted: source == Source::Phone && !note.trim().is_empty(),
        source,
        at,
    });
    Some((name, shot))
}

/// `shot` as an 8-bit RGB PNG at `path`.
fn save_png(shot: &Image, path: &Path) -> Result<(), String> {
    let image = shot.clone().try_into_dynamic().map_err(|e| e.to_string())?;
    image
        .to_rgb8()
        .save_with_format(path, image::ImageFormat::Png)
        .map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(secs: u64, ms: u64) -> SystemTime {
        UNIX_EPOCH + Duration::from_secs(secs) + Duration::from_millis(ms)
    }

    /// A folder is named by its UTC time to the millisecond and who asked,
    /// with `-2` on, past every name in use; the JSON's time is the same
    /// instant as RFC 3339.
    #[test]
    fn a_folder_is_named_by_its_time_and_source() {
        // 2026-11-15 15:04:05.123 UTC.
        let t = at(1_794_755_045, 123);
        assert_eq!(stamp(t), "2026-11-15T15-04-05.123Z");
        assert_eq!(rfc3339(t), "2026-11-15T15:04:05.123Z");
        assert_eq!(stamp(UNIX_EPOCH), "1970-01-01T00-00-00.000Z");
        // A leap day and the last moment of a year.
        assert_eq!(stamp(at(951_782_400, 0)), "2000-02-29T00-00-00.000Z");
        assert_eq!(stamp(at(1_798_761_599, 999)), "2026-12-31T23-59-59.999Z");
        let none = |_: &str| false;
        assert_eq!(
            folder_name(t, &Source::Key, none),
            "2026-11-15T15-04-05.123Z-key"
        );
        assert_eq!(
            folder_name(t, &Source::Pad("Pro Controller".into()), none),
            "2026-11-15T15-04-05.123Z-pad"
        );
        let used = [
            "2026-11-15T15-04-05.123Z-phone",
            "2026-11-15T15-04-05.123Z-phone-2",
        ];
        assert_eq!(
            folder_name(t, &Source::Phone, |n| used.contains(&n)),
            "2026-11-15T15-04-05.123Z-phone-3"
        );
    }

    fn last(source: Source, at: SystemTime, noted: bool) -> LastCapture {
        LastCapture {
            folder: PathBuf::from("/x"),
            name: "x".into(),
            source,
            at,
            noted,
        }
    }

    /// The note-joins rule: a note joins a pad's or key's capture up to a
    /// minute old with no phone note in it; not a phone's own, not one
    /// noted already, not past the minute, not an empty note, not with none
    /// taken, not across a clock that went back.
    #[test]
    fn a_note_joins_a_pad_or_key_capture_inside_a_minute() {
        let t = at(1_000_000, 0);
        let pad = Source::Pad("Pro Controller".into());
        assert!(joins(Some(&last(Source::Key, t, false)), "a", t));
        assert!(joins(
            Some(&last(pad.clone(), t, false)),
            "a",
            t + JOIN_WINDOW
        ));
        assert!(!joins(
            Some(&last(pad.clone(), t, false)),
            "a",
            t + JOIN_WINDOW + Duration::from_millis(1)
        ));
        assert!(!joins(Some(&last(Source::Phone, t, false)), "a", t));
        assert!(!joins(Some(&last(Source::Key, t, true)), "a", t));
        assert!(!joins(Some(&last(Source::Key, t, false)), "  ", t));
        assert!(!joins(None, "a", t));
        assert!(!joins(
            Some(&last(Source::Key, t, false)),
            "a",
            t - Duration::from_secs(1)
        ));
    }

    /// The folder: `GUAC_FLAG_DIR` first, else `playtest-flags` beside the
    /// extract root; refused in a git work tree with nothing made, as
    /// `GUAC_SHOT_DIR` is; nothing set is off; one that can't be made says
    /// so.
    #[test]
    fn the_flag_folder_is_refused_in_a_checkout() {
        let tmp = tempfile::tempdir().unwrap();
        let own = tmp.path().join("flags/mine");
        let s = |p: &Path| p.to_str().unwrap().to_owned();
        let extract = tmp.path().join("research/extract");
        assert_eq!(
            flag_dir(Some(&s(&own)), Some(&s(&extract))),
            Ok(own.clone())
        );
        assert!(own.is_dir());
        let beside = tmp.path().join("research/playtest-flags");
        assert_eq!(flag_dir(None, Some(&s(&extract))), Ok(beside.clone()));
        assert!(beside.is_dir());
        assert_eq!(flag_dir(Some(""), Some(&s(&extract))), Ok(beside));
        assert_eq!(flag_dir(None, None), Err(FlagDirError::Unset));
        let repo = tmp.path().join("repo");
        std::fs::create_dir_all(repo.join(".git")).unwrap();
        let inside = repo.join("flags");
        assert_eq!(
            flag_dir(Some(&s(&inside)), None),
            Err(FlagDirError::InCheckout {
                dir: inside.clone(),
                repo: repo.clone()
            })
        );
        assert!(!inside.exists(), "nothing made in a checkout");
        // An extract root in a checkout puts its flags in it too: refused.
        let in_repo = repo.join("extract");
        assert!(matches!(
            flag_dir(None, Some(&s(&in_repo))),
            Err(FlagDirError::InCheckout { .. })
        ));
        let file = tmp.path().join("a-file");
        std::fs::write(&file, b"x").unwrap();
        assert!(matches!(
            flag_dir(Some(&s(&file.join("flags"))), None),
            Err(FlagDirError::NotMade { .. })
        ));
    }

    /// The log's tail: the newest `server.log*`'s last lines, a tail read
    /// from mid-file dropping its first, cut line.
    #[test]
    fn the_logs_last_lines_come_from_the_newest_file() {
        let tmp = tempfile::tempdir().unwrap();
        let old = tmp.path().join("server.log.2026-11-14");
        std::fs::write(&old, "old\n").unwrap();
        std::thread::sleep(Duration::from_millis(20));
        let lines: Vec<String> = (0..300).map(|n| format!("line {n}")).collect();
        std::fs::write(
            tmp.path().join("server.log.2026-11-15"),
            lines.join("\n") + "\n",
        )
        .unwrap();
        std::fs::write(tmp.path().join("other.txt"), "no\n").unwrap();
        let tail = log_tail(tmp.path(), 200).unwrap();
        assert_eq!(tail.len(), 200);
        assert_eq!(tail[0], "line 100");
        assert_eq!(tail[199], "line 299");
        let big: String = (0..50_000).map(|n| format!("entry {n:06}\n")).collect();
        assert!(big.len() as u64 > LOG_TAIL_BYTES);
        std::thread::sleep(Duration::from_millis(20));
        std::fs::write(tmp.path().join("server.log.2026-11-16"), &big).unwrap();
        let tail = log_tail(tmp.path(), 5).unwrap();
        assert_eq!(tail.last().unwrap(), "entry 049999");
        assert!(
            tail.iter()
                .all(|l| l.starts_with("entry ") && l.len() == 12),
            "{tail:?}"
        );
        assert!(log_tail(&tmp.path().join("none"), 5).is_err());
    }

    /// The TV's side: the key writes a capture's folder (its JSON from the
    /// mirror's snapshot, the build and the log's tail); a phone's note
    /// inside the minute joins it, a second note makes its own capture, and
    /// a note after the minute does too.
    #[test]
    fn the_key_captures_and_a_phone_note_joins_it_inside_the_minute() {
        use bevy::input::ButtonState;
        use bevy::input::keyboard::{Key, KeyboardInput};
        let press = |app: &mut App, down: bool| {
            app.world_mut().write_message(KeyboardInput {
                key_code: FLAG_KEY,
                logical_key: Key::Character("f".into()),
                state: if down {
                    ButtonState::Pressed
                } else {
                    ButtonState::Released
                },
                text: None,
                repeat: false,
                window: Entity::PLACEHOLDER,
            });
        };
        let tmp = tempfile::tempdir().unwrap();
        let (dir, logs) = (tmp.path().join("flags"), tmp.path().join("logs"));
        std::fs::create_dir_all(&logs).unwrap();
        std::fs::write(logs.join("server.log.2026-11-15"), "one\ntwo\n").unwrap();
        let asks = FlagAsks::default();
        let mirror = TvMirror::default();
        mirror.0.lock().unwrap().screen = "Portal".into();
        let mut app = App::new();
        app.add_plugins((MinimalPlugins, bevy::input::InputPlugin))
            .insert_resource(mirror)
            .add_plugins(Flags {
                dir: dir.clone(),
                log_dir: logs,
                asks: asks.clone(),
                prompts: false,
            });
        app.update();
        assert!(app.world().contains_resource::<FlagsTaken>());
        let folders = |dir: &Path| {
            let mut v: Vec<String> = std::fs::read_dir(dir)
                .map(|d| {
                    d.flatten()
                        .map(|e| e.file_name().into_string().unwrap())
                        .collect()
                })
                .unwrap_or_default();
            v.sort();
            v
        };
        assert!(folders(&dir).is_empty(), "nothing before a press");
        press(&mut app, true);
        app.update();
        let made = folders(&dir);
        assert_eq!(made.len(), 1, "{made:?}");
        assert!(made[0].ends_with("Z-key"), "{made:?}");
        let key = dir.join(&made[0]);
        let json: serde_json::Value =
            serde_json::from_slice(&std::fs::read(key.join("capture.json")).unwrap()).unwrap();
        assert_eq!(json["source"], "key");
        assert_eq!(json["build"], guac_server::http::BUILD_TOKEN);
        assert_eq!(json["tv"]["screen"], "Portal");
        assert_eq!(json["summary"]["screen"], "Portal");
        assert!(json["taken"].as_str().unwrap().ends_with('Z'));
        assert_eq!(
            std::fs::read_to_string(key.join("log.txt")).unwrap(),
            "one\ntwo\n"
        );
        assert!(!key.join("note.txt").exists());
        // Held, not pressed again: no second capture.
        app.update();
        assert_eq!(folders(&dir).len(), 1);

        let mut joined = asks.push("the ship is missing".into()).unwrap();
        app.update();
        assert_eq!(
            joined.try_recv().unwrap(),
            Taken {
                folder: made[0].clone(),
                joined: true
            }
        );
        let note = std::fs::read_to_string(key.join("note.txt")).unwrap();
        assert!(note.ends_with("] the ship is missing\n"), "{note}");
        assert_eq!(folders(&dir).len(), 1, "joined, no new folder");

        let mut own = asks.push("double swords".into()).unwrap();
        app.update();
        let taken = own.try_recv().unwrap();
        assert!(
            !taken.joined && taken.folder.ends_with("Z-phone"),
            "{taken:?}"
        );
        let phone = dir.join(&taken.folder);
        let json: serde_json::Value =
            serde_json::from_slice(&std::fs::read(phone.join("capture.json")).unwrap()).unwrap();
        assert_eq!(
            (json["source"].as_str(), json["note"].as_str()),
            (Some("phone"), Some("double swords"))
        );
        assert!(
            std::fs::read_to_string(phone.join("note.txt"))
                .unwrap()
                .ends_with("] double swords\n")
        );

        // A key capture a minute and more ago: the note takes its own.
        press(&mut app, false);
        app.update();
        press(&mut app, true);
        app.update();
        assert_eq!(folders(&dir).len(), 3);
        app.world_mut()
            .resource_mut::<FlagState>()
            .last
            .as_mut()
            .unwrap()
            .at -= JOIN_WINDOW + Duration::from_secs(1);
        let mut late = asks.push("late".into()).unwrap();
        app.update();
        assert!(!late.try_recv().unwrap().joined);
        assert_eq!(folders(&dir).len(), 4);

        // The posted key: Bevy's keyboard stream takes it as the window's
        // key, one capture, and its release the frame after.
        press(&mut app, false);
        app.update();
        asks.keys.fetch_add(1, Ordering::Relaxed);
        app.update();
        let now = folders(&dir);
        assert_eq!(now.len(), 5, "{now:?}");
        assert_eq!(now.iter().filter(|f| f.ends_with("Z-key")).count(), 3);
        app.update();
        assert!(
            !app.world()
                .resource::<ButtonInput<KeyCode>>()
                .pressed(FLAG_KEY),
            "released the frame after"
        );
        assert_eq!(folders(&dir).len(), 5);
    }

    /// A folder that can't be written (a full disk, a folder gone read-only)
    /// costs the capture and nothing else: no panic, the phone's ask let go
    /// (its route answers 503), the TV's frames going on.
    #[cfg(unix)]
    #[test]
    fn a_capture_that_cant_be_written_plays_on() {
        use std::os::unix::fs::PermissionsExt;
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("flags");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o555)).unwrap();
        let asks = FlagAsks::default();
        let mut app = App::new();
        app.add_plugins((MinimalPlugins, bevy::input::InputPlugin))
            .insert_resource(TvMirror::default())
            .add_plugins(Flags {
                dir: dir.clone(),
                log_dir: tmp.path().join("no-logs"),
                asks: asks.clone(),
                prompts: false,
            });
        app.update();
        asks.keys.fetch_add(1, Ordering::Relaxed);
        let mut ask = asks.push("a note".into()).unwrap();
        app.update();
        assert!(
            matches!(ask.try_recv(), Err(oneshot::error::TryRecvError::Closed)),
            "the ask let go"
        );
        assert!(app.world().resource::<FlagState>().last.is_none());
        for _ in 0..3 {
            app.update();
        }
        std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o755)).unwrap();
        assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 0);
    }

    /// A capture's actors: the fight's creature in reach, read off the
    /// scripts' world with its script's behaviour, and shown by its own
    /// model's entity (a creature draws on a root of its own, not under its
    /// actor), not shown once that model is hidden.
    #[test]
    fn a_creature_in_reach_reads_shown_by_its_own_model() {
        use bevy::ecs::system::RunSystemOnce;
        use guac_formats::script::builder::FIGHT_CREATURE;
        let tmp = tempfile::tempdir().unwrap();
        let (mut app, _) = crate::test_support::play::fight_app(tmp.path(), None);
        for _ in 0..10 {
            app.update();
        }
        let creature = |app: &mut App| {
            let j = app
                .world_mut()
                .run_system_once(|r: CaptureReads| r.json(&Source::Key, SystemTime::now(), ""))
                .unwrap();
            j["actors"]
                .as_array()
                .unwrap()
                .iter()
                .find(|a| a["name"] == FIGHT_CREATURE)
                .cloned()
                .unwrap_or_else(|| panic!("no creature in {}", j["actors"]))
        };
        let model = {
            let w = app.world_mut();
            let mut q = w.query_filtered::<Entity, With<crate::creatures::DrawnCreature>>();
            q.single(w).unwrap()
        };
        // No renderer here to propagate it: the model's root set as the
        // visibility pass would leave it, shown and then hidden.
        app.world_mut()
            .entity_mut(model)
            .insert(InheritedVisibility::VISIBLE);
        let c = creature(&mut app);
        assert_eq!(c["in_reach"], true, "{c}");
        assert_eq!(c["state"], "awake", "{c}");
        assert!(c["behaviour"].is_string(), "{c}");
        assert_eq!(
            (c["shown"].as_bool(), c["drawn"].as_bool()),
            (Some(true), Some(false)),
            "{c}"
        );
        app.world_mut()
            .entity_mut(model)
            .insert(InheritedVisibility::HIDDEN);
        assert_eq!(creature(&mut app)["shown"], false);
    }

    /// The route: on or off as set; a post with flags off is a 409 and
    /// queues nothing; a bad or long body a 400; a good one waits for the
    /// TV's answer and hands back the folder's name.
    #[tokio::test]
    async fn the_route_answers_on_and_hands_back_the_folder() {
        use tower::ServiceExt;
        let asks = FlagAsks::default();
        let app = routes::<()>(&asks);
        let get = || {
            let app = app.clone();
            async move {
                let r = app
                    .oneshot(
                        axum::http::Request::get("/api/_test/flag")
                            .body(axum::body::Body::empty())
                            .unwrap(),
                    )
                    .await
                    .unwrap();
                let b = http_body_util::BodyExt::collect(r.into_body())
                    .await
                    .unwrap()
                    .to_bytes();
                serde_json::from_slice::<serde_json::Value>(&b).unwrap()
            }
        };
        let post = |body: String| {
            let app = app.clone();
            async move {
                let r = app
                    .oneshot(
                        axum::http::Request::post("/api/_test/flag")
                            .header("content-type", "application/json")
                            .body(axum::body::Body::from(body))
                            .unwrap(),
                    )
                    .await
                    .unwrap();
                let status = r.status();
                let b = http_body_util::BodyExt::collect(r.into_body())
                    .await
                    .unwrap()
                    .to_bytes();
                (status, b)
            }
        };
        assert_eq!(get().await, serde_json::json!({"on": false}));
        assert_eq!(post(r#"{"note":"x"}"#.into()).await.0, 409);
        assert!(asks.take().is_empty());
        asks.set_on(true);
        assert_eq!(get().await, serde_json::json!({"on": true}));
        let long = format!(r#"{{"note":"{}"}}"#, "x".repeat(NOTE_MAX + 1));
        for bad in [
            long.as_str(),
            r#"{"note":3}"#,
            r#"{"text":"x"}"#,
            "flag",
            r#"{"note":"a\u0000b"}"#,
            r#"{"note":"\u001b[2J"}"#,
            r#"{"note":"a\u0085b"}"#,
        ] {
            assert_eq!(post(bad.into()).await.0, 400, "{bad}");
        }
        assert!(asks.take().is_empty(), "a refused body queues nothing");
        assert!(note_ok("two lines\r\nand\ta tab, ünïcode ✓"));
        // A flood: ASKS_MAX wait for the TV, the next is refused at once.
        let held: Vec<_> = (0..ASKS_MAX)
            .map(|_| asks.push("x".into()).unwrap())
            .collect();
        assert_eq!(post(r#"{"note":"one more"}"#.into()).await.0, 429);
        assert_eq!(asks.take().len(), ASKS_MAX);
        drop(held);
        let answer = asks.clone();
        let tv = tokio::spawn(async move {
            loop {
                if let Some(ask) = answer.take().pop() {
                    assert_eq!(ask.note, "the ship is gone");
                    let _ = ask.reply.send(Taken {
                        folder: "f".into(),
                        joined: true,
                    });
                    return;
                }
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        });
        let (status, body) = post(r#"{"note":"the ship is gone"}"#.into()).await;
        tv.await.unwrap();
        assert_eq!(status, 201);
        assert_eq!(
            serde_json::from_slice::<serde_json::Value>(&body).unwrap(),
            serde_json::json!({"folder": "f", "joined": true})
        );
    }
}
