//! A level asked for by name (openspec AS.13): any level on the disc's list
//! on the TV, not only chapter 1. A `dev-tools` build asks for one at start
//! with `GUAC_LEVEL=<archive>`, a `test-hooks` build over `POST
//! /api/_test/tv/level` (`{"name": "level_200"}`).
//!
//! - When: once the Giants pick's extraction has ended (it owns the extract
//!   while it runs: two writers would each save the manifest over the
//!   other's), and not while the TV is in a level, which leaving for
//!   another is AS.16's.
//! - How: on a thread of its own, the extract opened as a pick opens it, the
//!   name checked against the dump's own level list
//!   (`guac_extract::levels::on_disc`) before anything is read, then
//!   AS.1's on-demand entry (`guac_extract::stages::extract_level`), which
//!   keeps every part already in and reads the rest. While it reads, the
//!   portal's header shows its progress where the first run's shows. A part
//!   the TV's load reads that didn't extract stops it, named.
//! - Then the level's load ([`crate::level`]) takes it as it takes chapter 1,
//!   the level it replaces taken down first. Chapter 1 runs its scripts;
//!   another level is drawn, solid and propped by AG.7's rule with its
//!   scripts unread until AS.15.

use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use bevy::prelude::*;
use crossbeam_channel::{Receiver, TryRecvError};
use guac_extract::ExtractRoot;
use guac_extract::dump::HashProgress;
use guac_extract::levels::{self, DiscLevel};
use guac_extract::stages::{self, Event};
use serde::{Deserialize, Serialize};
use tracing::{info, warn};

use crate::extraction::{Band, ExtractBand, ExtractJob, ExtractStatus, Extraction, QUIET_START};
use crate::level::{Level, LevelSource};
use crate::tv_state::Setup;

/// The parts the TV's load can't do without: a level whose fetch leaves one
/// of these out isn't handed to it. The script, palettes and nodes parts it
/// does without, as chapter 1's load does (a level drawn without them).
pub(crate) const NEEDED: [&str; 6] = [
    levels::SCENE,
    levels::PROPS,
    levels::COLLISION,
    levels::PLACEMENTS,
    levels::LIGHTS,
    levels::ATMOSPHERE,
];

/// Levels asked for over `POST /api/_test/tv/level`, waiting for a frame;
/// the last asked wins.
#[derive(Resource, Clone, Default)]
pub(crate) struct LevelAsks(pub(crate) Arc<Mutex<Option<String>>>);

impl LevelAsks {
    /// Keeps `name` for the next frame when it could name a level file: 1
    /// to 64 characters, lowercase letters, digits and `_`.
    pub(crate) fn ask(&self, name: &str) -> bool {
        let ok = plain_name(name);
        if ok {
            *self.0.lock().unwrap_or_else(|e| e.into_inner()) = Some(name.to_owned());
        }
        ok
    }

    fn take(&self) -> Option<String> {
        self.0.lock().unwrap_or_else(|e| e.into_inner()).take()
    }
}

/// Whether `name` could be a level archive's name on the disc: what the
/// route and the env take before the disc's list is read.
pub(crate) fn plain_name(name: &str) -> bool {
    (1..=64).contains(&name.len())
        && name
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_')
}

/// `name` on the disc's level list, or why not: the check before anything
/// of the level is read.
pub(crate) fn on_the_list<'a>(list: &'a [DiscLevel], name: &str) -> Result<&'a DiscLevel, String> {
    list.iter()
        .find(|l| l.name == name)
        .ok_or_else(|| format!("{name} isn't a level on this disc ({} are)", list.len()))
}

/// What `/api/_test/tv`'s `level.asked` reports of the last level asked.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub(crate) struct AskedReport {
    pub name: String,
    /// `waiting` (for the Giants pick's extraction to end), `opening` (the
    /// extract, its dump checked), `extracting`, `loading` (handed to the
    /// level's load), `refused` or `failed`.
    pub state: String,
    /// Its kind by name (`Story`, `Pvp`, `Hub`, ...), once the list's read.
    pub kind: Option<String>,
    /// Parts done of the parts the on-demand entry walks.
    pub done: usize,
    pub total: usize,
    /// Parts read this time, and parts already in and kept.
    pub parts_read: usize,
    pub parts_kept: usize,
    /// Files written and the bytes of every file the level's parts hold.
    pub written: usize,
    pub bytes: u64,
    /// From the ask to the extract open, and to every part in.
    pub open_ms: Option<u64>,
    pub extract_ms: Option<u64>,
    /// Parts that didn't extract, by part, with why.
    pub failed: Vec<(String, String)>,
    pub reason: Option<String>,
}

/// What the fetch's thread reports.
#[derive(Debug)]
pub(crate) enum FetchEvent {
    Hashing(HashProgress),
    Opened { kind: levels::Kind },
    Parts { done: usize, total: usize },
    Done(Result<Box<Fetched>, FetchError>),
}

/// Why a fetch gave the load nothing: the ask refused before any of a level
/// was read (a name off the disc's list), or the fetch failed (the extract
/// wouldn't open, a part the load needs didn't extract).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum FetchError {
    Refused(String),
    Failed(String),
}

impl std::fmt::Display for FetchError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Refused(why) | Self::Failed(why) => f.write_str(why),
        }
    }
}

/// A level fetched: its source for the load and what the entry did.
#[derive(Debug)]
pub(crate) struct Fetched {
    pub(crate) source: LevelSource,
    pub(crate) kind: levels::Kind,
    pub(crate) parts_read: usize,
    pub(crate) parts_kept: usize,
    pub(crate) written: usize,
    pub(crate) bytes: u64,
    pub(crate) failed: Vec<(String, String)>,
    pub(crate) opened: Duration,
    pub(crate) extracted: Duration,
}

/// Opens the extract of `dump` as a pick does, checks `name` against the
/// dump's level list and extracts what of it isn't in (AS.1's on-demand
/// entry): the level's source once every part the load needs is in.
/// Blocking.
pub(crate) fn fetch(
    dump: &Path,
    job: &ExtractJob,
    name: &str,
    send: &(dyn Fn(FetchEvent) + Sync),
) -> Result<Fetched, FetchError> {
    let failed = |e: guac_extract::ExtractError| FetchError::Failed(e.to_string());
    let started = Instant::now();
    let root = ExtractRoot::resolve(job.root.clone()).map_err(failed)?;
    let opened = guac_extract::open(dump, &root, &job.known(), &|p| send(FetchEvent::Hashing(p)))
        .map_err(failed)?;
    let mut dir = opened.dir;
    let kind = on_the_list(&levels::on_disc(dir.manifest()), name)
        .map_err(FetchError::Refused)?
        .kind;
    send(FetchEvent::Opened { kind });
    let opened_after = started.elapsed();
    let stage = stages::extract_level(&mut dir, name, job.ir, &|e| {
        if let Event::Level { done, total } = e {
            send(FetchEvent::Parts { done, total });
        }
    })
    .map_err(failed)?;
    let failed: Vec<(String, String)> = stage
        .failed
        .iter()
        .map(|f| (f.part.clone(), f.reason.clone()))
        .collect();
    if let Some((part, why)) = failed.iter().find(|(p, _)| NEEDED.contains(&p.as_str())) {
        return Err(FetchError::Failed(format!(
            "{name}'s {part} didn't extract: {why}"
        )));
    }
    Ok(Fetched {
        source: LevelSource::named(
            dir.path().to_path_buf(),
            job.ir,
            dir.manifest().identity.clone(),
            name,
        ),
        kind,
        parts_read: stage.parts_read,
        parts_kept: stage.parts_kept,
        written: stage.files.written,
        bytes: stage.files.bytes,
        failed,
        opened: opened_after,
        extracted: started.elapsed(),
    })
}

/// The ask waiting and the fetch running.
#[derive(Resource, Default)]
pub(crate) struct LevelChoice {
    waiting: Option<String>,
    running: Option<Running>,
}

struct Running {
    name: String,
    started: Instant,
    feed: Receiver<FetchEvent>,
    status: Progress,
}

/// Where a fetch stands, for the header.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Progress {
    Opening { bytes_done: u64, bytes_total: u64 },
    Parts { done: usize, total: usize },
}

/// What the portal's header draws while `name` is fetched: its dump checked,
/// then its parts read off the disc.
fn band(name: &str, p: Progress) -> Band {
    let percent =
        |done: u64, total: u64| (done.min(total) * 100).checked_div(total).unwrap_or(100) as u8;
    let (detail, pct) = match p {
        Progress::Opening {
            bytes_done,
            bytes_total,
        } => {
            let pct = percent(bytes_done, bytes_total);
            (format!("Checking your Giants dump… {pct}%"), pct)
        }
        Progress::Parts { done, total } => {
            let pct = percent(done as u64, total as u64);
            (format!("Reading {name} off your disc… {pct}%"), pct)
        }
    };
    Band {
        title: format!("Getting {name} ready"),
        detail,
        percent: Some(pct),
        code: None,
    }
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

/// `GUAC_LEVEL`'s level, asked at start in a `dev-tools` build.
fn asked_at_start(dev_build: bool, value: Option<&str>) -> Option<String> {
    let v = value?.trim();
    (dev_build && plain_name(v)).then(|| v.to_owned())
}

/// The plugin: [`LevelChoice`], the asks and the systems, after the
/// extraction's and before the level's load takes a source.
pub(crate) struct LevelChoicePlugin {
    pub(crate) asks: LevelAsks,
}

impl Plugin for LevelChoicePlugin {
    fn build(&self, app: &mut App) {
        let env = std::env::var("GUAC_LEVEL").ok();
        let at_start = asked_at_start(cfg!(feature = "dev-tools"), env.as_deref());
        if env.is_some() && at_start.is_none() {
            warn!(value = ?env, "GUAC_LEVEL isn't a level's name (or this isn't a dev build): no level asked");
        }
        if let Some(name) = &at_start {
            info!(%name, "GUAC_LEVEL: the level asked at start, loaded once the Giants pick's extraction ends");
        }
        app.insert_resource(self.asks.clone())
            .insert_resource(LevelChoice {
                waiting: at_start,
                running: None,
            })
            .add_systems(
                Update,
                (take_asks, start_fetch, drain_fetch)
                    .chain()
                    .before(crate::level::LevelSystems),
            );
    }
}

/// An ask over the route replaces one waiting.
fn take_asks(asks: Res<LevelAsks>, mut choice: ResMut<LevelChoice>, mut level: ResMut<Level>) {
    let Some(name) = asks.take() else {
        return;
    };
    info!(%name, "a level asked for by name");
    level.set_asked(AskedReport {
        name: name.clone(),
        state: "waiting".into(),
        ..Default::default()
    });
    choice.waiting = Some(name);
}

/// The waiting ask's fetch, once the extraction's ended and no other runs.
fn start_fetch(
    mut choice: ResMut<LevelChoice>,
    mut level: ResMut<Level>,
    extraction: Res<Extraction>,
    play: Option<Res<crate::play::Play>>,
    job: Option<Res<ExtractJob>>,
    setup: Res<Setup>,
) {
    if choice.running.is_some() || choice.waiting.is_none() {
        return;
    }
    let name = choice.waiting.clone().unwrap_or_default();
    if level.asked().is_none_or(|a| a.name != name) {
        level.set_asked(AskedReport {
            name: name.clone(),
            state: "waiting".into(),
            ..Default::default()
        });
    }
    let refuse = |level: &mut Level, why: String| {
        warn!(%name, reason = %why, "the level asked for isn't loaded");
        level.update_asked(|a| {
            a.state = "refused".into();
            a.reason = Some(why);
        });
    };
    if play.as_deref().is_some_and(crate::play::Play::started) {
        choice.waiting = None;
        refuse(
            &mut level,
            "the TV is in a level: leaving it for another is AS.16's".into(),
        );
        return;
    }
    if !extraction_ended(&extraction) {
        return;
    }
    choice.waiting = None;
    let (Some(job), Some(dump)) = (job, setup.0.giants_dir.clone()) else {
        refuse(
            &mut level,
            "no Giants dump or extract to read it from".into(),
        );
        return;
    };
    let job = job.clone();
    let (tx, rx) = crossbeam_channel::unbounded();
    let thread_name = name.clone();
    let spawned = std::thread::Builder::new()
        .name("level fetch".into())
        .spawn(move || {
            let result = fetch(&dump, &job, &thread_name, &|e| {
                let _ = tx.send(e);
            });
            let _ = tx.send(FetchEvent::Done(result.map(Box::new)));
        });
    if let Err(e) = spawned {
        refuse(&mut level, format!("its thread didn't start: {e}"));
        return;
    }
    info!(%name, "the level asked for: opening the extract");
    level.update_asked(|a| a.state = "opening".into());
    choice.running = Some(Running {
        name,
        started: Instant::now(),
        feed: rx,
        status: Progress::Opening {
            bytes_done: 0,
            bytes_total: 0,
        },
    });
}

/// The fetch's events into the report and the header; its level handed to
/// the load once every part it needs is in.
fn drain_fetch(
    mut choice: ResMut<LevelChoice>,
    mut level: ResMut<Level>,
    extraction: Res<Extraction>,
    job: Option<Res<ExtractJob>>,
    mut drawn: ResMut<ExtractBand>,
) {
    let Some(running) = choice.running.as_mut() else {
        return;
    };
    let ms = |t: Duration| Some(t.as_millis() as u64);
    let mut done = None;
    loop {
        let event = match running.feed.try_recv() {
            Ok(e) => e,
            Err(TryRecvError::Empty) => break,
            Err(TryRecvError::Disconnected) => FetchEvent::Done(Err(FetchError::Failed(
                "the fetch's thread stopped without finishing".into(),
            ))),
        };
        match event {
            FetchEvent::Hashing(p) => {
                running.status = Progress::Opening {
                    bytes_done: p.bytes_done,
                    bytes_total: p.bytes_total,
                }
            }
            FetchEvent::Opened { kind } => {
                let took = running.started.elapsed();
                level.update_asked(|a| {
                    a.state = "extracting".into();
                    a.kind = Some(format!("{kind:?}"));
                    a.open_ms = ms(took);
                });
                running.status = Progress::Parts { done: 0, total: 0 };
            }
            FetchEvent::Parts { done, total } => {
                level.update_asked(|a| (a.done, a.total) = (done, total));
                running.status = Progress::Parts { done, total };
            }
            FetchEvent::Done(result) => {
                done = Some(result);
                break;
            }
        }
    }
    let name = running.name.clone();
    let Some(result) = done else {
        let shown = (running.started.elapsed() >= QUIET_START).then(|| band(&name, running.status));
        drawn.set_if_neq(ExtractBand(shown));
        return;
    };
    choice.running = None;
    // The header goes back to what the extraction leaves it (nothing, or
    // its failure).
    drawn.set_if_neq(ExtractBand(crate::extraction::band(&extraction.status)));
    match result {
        Ok(f) => {
            info!(
                level = %name,
                kind = ?f.kind,
                read = f.parts_read,
                kept = f.parts_kept,
                written = f.written,
                bytes = f.bytes,
                failed = f.failed.len(),
                open_s = f.opened.as_secs_f64(),
                took_s = f.extracted.as_secs_f64(),
                "the level asked for is in the extract: loading it"
            );
            for (part, why) in &f.failed {
                warn!(level = %name, %part, reason = %why, "a part of the level asked for didn't extract; it loads without it");
            }
            level.update_asked(|a| {
                a.state = "loading".into();
                a.kind = Some(format!("{:?}", f.kind));
                a.parts_read = f.parts_read;
                a.parts_kept = f.parts_kept;
                a.written = f.written;
                a.bytes = f.bytes;
                a.open_ms = ms(f.opened);
                a.extract_ms = ms(f.extracted);
                a.failed = f.failed.clone();
            });
            if let Some(tx) = job.as_ref().and_then(|j| j.level.as_ref()) {
                let _ = tx.send(f.source);
            }
        }
        Err(e) => {
            warn!(level = %name, reason = %e, "the level asked for didn't load");
            level.update_asked(|a| {
                a.state = match e {
                    FetchError::Refused(_) => "refused",
                    FetchError::Failed(_) => "failed",
                }
                .into();
                a.reason = Some(e.to_string());
            });
        }
    }
}

/// `POST /api/_test/tv/level`'s body.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct LevelBody {
    name: String,
}

/// `POST /api/_test/tv/level` (`{"name": "level_200"}`): 202 with the ask
/// kept for the next frame, 400 for a body that isn't one or a name no
/// level file could have. A name off the disc's list, or an ask the TV
/// can't take now, shows in `/api/_test/tv`'s `level.asked`. Generic so a
/// test needs no `AppState`.
pub(crate) fn routes<S: Clone + Send + Sync + 'static>(asks: &LevelAsks) -> axum::Router<S> {
    use axum::extract::rejection::JsonRejection;
    let asks = asks.clone();
    axum::Router::new().route(
        "/api/_test/tv/level",
        axum::routing::post(move |body: Result<axum::Json<LevelBody>, JsonRejection>| {
            let kept = body.is_ok_and(|axum::Json(b)| asks.ask(&b.name));
            async move {
                if kept {
                    axum::http::StatusCode::ACCEPTED
                } else {
                    axum::http::StatusCode::BAD_REQUEST
                }
            }
        }),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn listed(names: &[&str]) -> Vec<DiscLevel> {
        names
            .iter()
            .map(|n| DiscLevel {
                name: (*n).into(),
                kind: levels::Kind::of(n),
                bytes: 1,
            })
            .collect()
    }

    /// The ask's name is checked twice before anything of a level is read:
    /// that it could be a level file's name at all (the route's 400), and
    /// that the disc's list holds it.
    #[test]
    fn a_level_off_the_discs_list_is_refused_and_one_on_it_named() {
        let list = listed(&[
            "level_213_tutotial",
            "level_200",
            "pvp_level_001_cubedungeon",
        ]);
        let pvp = on_the_list(&list, "pvp_level_001_cubedungeon").unwrap();
        assert_eq!(
            (pvp.name.as_str(), pvp.kind),
            ("pvp_level_001_cubedungeon", levels::Kind::Pvp)
        );
        assert_eq!(
            on_the_list(&list, "level_200").unwrap().kind,
            levels::Kind::Story
        );
        let why = on_the_list(&list, "level_999").unwrap_err();
        assert!(
            why.contains("level_999") && why.contains("isn't a level on this disc"),
            "{why}"
        );
        // A prefix of a name on the list isn't it.
        assert!(on_the_list(&list, "level_2").is_err());
        for bad in [
            "",
            "Level_200",
            "../level_200",
            "level 200",
            "level/200",
            &"x".repeat(65),
        ] {
            assert!(!plain_name(bad), "{bad:?}");
        }
        assert!(plain_name("pvp_level_001_cubedungeon") && plain_name(&"x".repeat(64)));
        let asks = LevelAsks::default();
        assert!(!asks.ask("../etc"));
        assert_eq!(asks.take(), None);
        assert!(asks.ask("level_200") && asks.ask("level_201"));
        assert_eq!(
            asks.take().as_deref(),
            Some("level_201"),
            "the last asked wins"
        );
        assert_eq!(asks.take(), None);
    }

    /// `GUAC_LEVEL` asks only in a dev build, and only for a name a level
    /// file could have.
    #[test]
    fn guac_level_asks_in_a_dev_build_alone() {
        assert_eq!(
            asked_at_start(true, Some("level_200")).as_deref(),
            Some("level_200")
        );
        assert_eq!(
            asked_at_start(true, Some(" level_200\n")).as_deref(),
            Some("level_200")
        );
        assert_eq!(asked_at_start(false, Some("level_200")), None);
        assert_eq!(asked_at_start(true, Some("../x")), None);
        assert_eq!(asked_at_start(true, None), None);
    }

    /// The header while a level is fetched: the dump's check, then its
    /// parts, each with its bar.
    #[test]
    fn the_header_says_which_level_is_read_and_how_far() {
        let b = band("level_200", Progress::Parts { done: 4, total: 10 });
        assert_eq!(b.title, "Getting level_200 ready");
        assert_eq!(b.detail, "Reading level_200 off your disc… 40%");
        assert_eq!((b.percent, b.code), (Some(40), None));
        let b = band(
            "level_200",
            Progress::Opening {
                bytes_done: 1,
                bytes_total: 4,
            },
        );
        assert_eq!(
            (b.detail.as_str(), b.percent),
            ("Checking your Giants dump… 25%", Some(25))
        );
        // Nothing to do counts as done.
        assert_eq!(
            band("x", Progress::Parts { done: 0, total: 0 }).percent,
            Some(100)
        );
    }
}
