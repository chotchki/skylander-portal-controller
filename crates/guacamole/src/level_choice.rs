//! A level asked for by name (openspec AS.13): any level on the disc's list
//! on the TV, not only chapter 1. A `dev-tools` build asks for one at start
//! with `GUAC_LEVEL=<archive>`, a `test-hooks` build over `POST
//! /api/_test/tv/level` (`{"name": "level_200"}`).
//!
//! - Or the scripts (openspec AS.15): a level change a script asked for
//!   (`system.load level` naming a level on the disc, which the run holds
//!   while it loads: [`guacamole::scripts::ScriptWorld::level_change`]) is
//!   asked by name the frame it's made.
//! - When: once the Giants pick's extraction has ended (it owns the extract
//!   while it runs: two writers would each save the manifest over the
//!   other's). In a level too (openspec AS.49): the level held while the
//!   next is read ([`LevelChoice::changing`]), then left with the hero in
//!   it for the next as it comes, the portal's figures carried over
//!   ([`crate::play`]).
//! - How: on a thread of its own, the extract opened as a pick opens it, the
//!   name checked against the dump's own level list
//!   (`guac_extract::levels::on_disc`) before anything is read, then
//!   AS.1's on-demand entry (`guac_extract::stages::extract_level`), which
//!   keeps every part already in and reads the rest. While it reads, the
//!   portal's header shows its progress where the first run's shows. A part
//!   the TV's load reads that didn't extract stops it, named.
//! - Then the level's load ([`crate::level`]) takes it as it takes chapter 1,
//!   the level it replaces taken down first, its own scripts run (AS.51).
//! - Its getting-ready screen (openspec AS.14): the header goes on saying
//!   what the load is at ([`loading_band`]) until the level is ready, its
//!   own keys (the census of what it draws, read off its parts at its load)
//!   compiled on the warm's hidden stage first, so the frame it shows in
//!   compiles nothing.

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
    /// Who asked: `scripts` (a script's level change, openspec AS.15), or
    /// none for the route or `GUAC_LEVEL`.
    pub by: Option<String>,
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
    /// The last level change the scripts asked for, by run and level: each
    /// is asked once.
    scripts_asked: Option<(u64, String)>,
}

impl LevelChoice {
    /// A level asked for and not yet handed to the load: what holds a level
    /// the TV is in while the next is read (openspec AS.49).
    pub(crate) fn changing(&self) -> bool {
        self.waiting.is_some() || self.running.is_some()
    }
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

/// What the portal's header draws while the level `name` loads, from its
/// load's report (openspec AS.14): its parts read, handed to the graphics
/// card, then its keys compiled, each with how far; none once it's ready,
/// shown or failed, or while the load is another level's.
pub(crate) fn loading_band(name: &str, r: &crate::level::LevelReport) -> Option<Band> {
    if r.level.as_deref() != Some(name) {
        return None;
    }
    let (detail, pct) = match r.state.as_str() {
        "reading" => ("Reading its parts…".to_owned(), 0),
        "uploading" => ("Handing it to the graphics card…".to_owned(), 0),
        "waiting" => {
            let compiled = r.keys.saturating_sub(r.keys_waiting);
            let pct = (compiled.min(r.keys) * 100)
                .checked_div(r.keys)
                .unwrap_or(100) as u8;
            (
                format!("Getting its graphics ready… {compiled} of {}", r.keys),
                pct,
            )
        }
        _ => return None,
    };
    Some(Band {
        title: format!("Getting {name} ready"),
        detail,
        percent: Some(pct),
        code: None,
    })
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
                ..Default::default()
            })
            .add_systems(
                Update,
                (take_asks, take_script_asks, start_fetch, drain_fetch)
                    .chain()
                    .before(crate::level::LevelSystems),
            )
            .add_systems(Update, follow_load.after(crate::level::LevelSystems));
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

/// A level change the scripts asked for (openspec AS.15): asked by name
/// the frame it's made, once a run, as the route asks, over one waiting.
fn take_script_asks(
    world: Option<Res<guacamole::scripts::ScriptWorld>>,
    mut choice: ResMut<LevelChoice>,
    mut level: ResMut<Level>,
) {
    let Some(world) = world else {
        return;
    };
    let Some(change) = world.level_change() else {
        return;
    };
    let key = (world.run(), change.name.clone());
    if choice.scripts_asked.as_ref() == Some(&key) {
        return;
    }
    info!(name = %change.name, text = %change.text, "a script asked for a level: asked by name");
    level.set_asked(AskedReport {
        name: change.name.clone(),
        by: Some("scripts".into()),
        state: "waiting".into(),
        ..Default::default()
    });
    choice.waiting = Some(change.name.clone());
    choice.scripts_asked = Some(key);
}

/// The waiting ask's fetch, once the extraction's ended and no other runs.
fn start_fetch(
    mut choice: ResMut<LevelChoice>,
    mut level: ResMut<Level>,
    extraction: Res<Extraction>,
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
/// the load once every part it needs is in, the TV in a level or not (a
/// figure placed while its parts were read included: the level it went
/// into is left for this one, openspec AS.49).
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

/// The asked level's load into its report and the header (openspec AS.14):
/// [`loading_band`] while it loads, then the extraction's band again, the
/// ask `ready` (or `shown`, the dev view's) or `failed` with the load's
/// reason.
fn follow_load(
    choice: Res<LevelChoice>,
    mut level: ResMut<Level>,
    extraction: Res<Extraction>,
    mut drawn: ResMut<ExtractBand>,
    mut held: Local<bool>,
) {
    let Some(name) = level
        .asked()
        .filter(|a| a.state == "loading" && choice.running.is_none())
        .map(|a| a.name.clone())
    else {
        return;
    };
    let r = level.report();
    if r.level.as_deref() != Some(name.as_str()) {
        return;
    }
    if let Some(band) = loading_band(&name, r) {
        drawn.set_if_neq(ExtractBand(Some(band)));
        *held = true;
        return;
    }
    let (state, reason) = match r.state.as_str() {
        "failed" => ("failed", r.reason.clone()),
        "shown" => ("shown", None),
        _ => ("ready", None),
    };
    if *held {
        drawn.set_if_neq(ExtractBand(crate::extraction::band(&extraction.status)));
        *held = false;
    }
    level.update_asked(|a| {
        a.state = state.into();
        if reason.is_some() {
            a.reason = reason;
        }
    });
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

    /// AS.14: the header while the asked level loads, off its load's
    /// report: reading, handing over, then its keys compiled of all it
    /// draws; nothing once it's ready, shown or failed, or for another
    /// level's load.
    #[test]
    fn the_header_follows_the_asked_levels_load_until_its_keys_are_compiled() {
        let mut r = crate::level::LevelReport {
            state: "reading".into(),
            level: Some("level_200".into()),
            keys: 40,
            ..Default::default()
        };
        let detail = |r: &crate::level::LevelReport| {
            loading_band("level_200", r).map(|b| (b.detail, b.percent))
        };
        assert_eq!(
            loading_band("level_200", &r).map(|b| b.title).as_deref(),
            Some("Getting level_200 ready")
        );
        assert_eq!(detail(&r), Some(("Reading its parts…".into(), Some(0))));
        r.state = "uploading".into();
        assert_eq!(
            detail(&r),
            Some(("Handing it to the graphics card…".into(), Some(0)))
        );
        r.state = "waiting".into();
        r.keys_waiting = 30;
        assert_eq!(
            detail(&r),
            Some(("Getting its graphics ready… 10 of 40".into(), Some(25)))
        );
        r.keys_waiting = 0;
        assert_eq!(detail(&r).and_then(|d| d.1), Some(100));
        for done in ["ready", "shown", "failed", "idle"] {
            r.state = done.into();
            assert_eq!(detail(&r), None, "{done}");
        }
        r.state = "waiting".into();
        assert_eq!(loading_band("pvp_level_001_cubedungeon", &r), None);
    }

    /// AS.14: the asked level's load drives the header and the ask's state:
    /// its band while it loads, the extraction's again (here its failure,
    /// which stays up) once it shows, the ask `shown`; a failed load `failed`
    /// with its reason; nothing moves while the fetch still runs or the load
    /// is another level's.
    #[test]
    fn the_asked_levels_load_moves_its_report_and_gives_the_header_back() {
        let mut app = App::new();
        let failed = ExtractStatus::Failed(crate::extraction::Failure::Other {
            code: "GUAC-E-X".into(),
            message: "made up".into(),
        });
        app.insert_resource(Level::new(false))
            .insert_resource(LevelChoice::default())
            .insert_resource(Extraction {
                status: failed.clone(),
                ..Default::default()
            })
            .insert_resource(ExtractBand(None))
            .add_systems(Update, follow_load);
        let asked = |app: &App| app.world().resource::<Level>().asked().cloned().unwrap();
        let band = |app: &App| app.world().resource::<ExtractBand>().0.clone();
        let set = |app: &mut App, level: &str, state: &str| {
            let mut l = app.world_mut().resource_mut::<Level>();
            let r = l.report_mut();
            (r.level, r.state, r.keys, r.keys_waiting) = (Some(level.into()), state.into(), 4, 1);
        };
        app.world_mut()
            .resource_mut::<Level>()
            .set_asked(AskedReport {
                name: "level_200".into(),
                state: "loading".into(),
                ..Default::default()
            });
        // Another level's load moves nothing.
        set(&mut app, "level_213_tutotial", "waiting");
        app.update();
        assert_eq!((asked(&app).state.as_str(), band(&app)), ("loading", None));
        set(&mut app, "level_200", "waiting");
        app.update();
        assert_eq!(
            band(&app).map(|b| b.detail),
            Some("Getting its graphics ready… 3 of 4".into())
        );
        assert_eq!(asked(&app).state, "loading");
        set(&mut app, "level_200", "shown");
        app.update();
        assert_eq!(asked(&app).state, "shown");
        assert_eq!(band(&app), crate::extraction::band(&failed));
        // Once shown it's done: a later state of the load moves nothing.
        set(&mut app, "level_200", "waiting");
        app.update();
        assert_eq!(asked(&app).state, "shown");

        // A load that fails, its reason the ask's.
        app.world_mut()
            .resource_mut::<Level>()
            .set_asked(AskedReport {
                name: "level_200".into(),
                state: "loading".into(),
                ..Default::default()
            });
        {
            let mut l = app.world_mut().resource_mut::<Level>();
            let r = l.report_mut();
            r.state = "failed".into();
            r.reason = Some("the level has no start".into());
        }
        app.update();
        let a = asked(&app);
        assert_eq!(
            (a.state.as_str(), a.reason.as_deref()),
            ("failed", Some("the level has no start"))
        );
    }

    /// The ask waits while the pick's extraction runs (it owns the
    /// manifest: two writers would each save it over the other's), Ready
    /// included until its thread's channel is drained, and its fetch starts
    /// once it has ended, done or failed; an ask while a hero is in a level,
    /// or with no dump or extract to read, is refused without a fetch.
    #[test]
    fn the_ask_waits_for_the_picks_extraction_to_end() {
        let tmp = tempfile::tempdir().unwrap();
        let app_with = |status: ExtractStatus, running: bool, job: bool| {
            let mut app = App::new();
            let (keep, feed) = crossbeam_channel::unbounded();
            app.insert_resource(Level::new(false))
                .insert_resource(LevelChoice {
                    waiting: Some("level_200".into()),
                    ..Default::default()
                })
                .insert_resource(Extraction {
                    status,
                    feed: running.then_some(feed),
                    ..Default::default()
                })
                .insert_resource(Setup(crate::screen::SetupView {
                    giants_dir: Some(tmp.path().join("dump")),
                    ..Default::default()
                }))
                .add_systems(Update, start_fetch);
            if job {
                app.insert_resource(ExtractJob::new(
                    tmp.path(),
                    Some(tmp.path().join("extract").display().to_string()),
                    None,
                ));
            }
            (app, keep)
        };
        let state = |app: &App| {
            let choice = app.world().resource::<LevelChoice>();
            let asked = app.world().resource::<Level>().asked().cloned().unwrap();
            (
                asked.state,
                choice.running.is_some(),
                choice.waiting.is_some(),
            )
        };
        let sprites = ExtractStatus::Sprites { done: 3, total: 9 };
        let ready = ExtractStatus::Ready {
            intros: 0,
            ran: true,
        };
        // Running, and Ready with its channel not yet drained: waiting.
        for (status, running) in [(sprites.clone(), true), (ready.clone(), true)] {
            let (mut app, _keep) = app_with(status, running, true);
            for _ in 0..3 {
                app.update();
            }
            assert_eq!(state(&app), ("waiting".into(), false, true));
        }
        // Ended, done or failed: the fetch starts.
        let failed = ExtractStatus::Failed(crate::extraction::Failure::Other {
            code: "GUAC-E-X".into(),
            message: "made up".into(),
        });
        for status in [ready.clone(), failed] {
            let (mut app, _keep) = app_with(status, false, true);
            app.update();
            assert_eq!(state(&app), ("opening".into(), true, false));
        }
        // Ended with nothing to read from: refused, no fetch.
        let (mut app, _keep) = app_with(ready.clone(), false, false);
        app.update();
        assert_eq!(state(&app), ("refused".into(), false, false));
        // A hero in a level (openspec AS.49): the ask waits on the
        // extraction as any does, then its fetch starts.
        for (status, running, after) in [(sprites, true, "waiting"), (ready, false, "opening")] {
            let (mut app, _keep) = app_with(status, running, true);
            let camera = app.world_mut().spawn_empty().id();
            app.insert_resource(crate::play::Play::started_for_tests(camera));
            app.update();
            let asked = app.world().resource::<Level>().asked().cloned().unwrap();
            assert_eq!((asked.state.as_str(), asked.reason), (after, None));
        }
    }

    /// A fetch that ends while the TV is in a level (asked there, or a
    /// figure placed while its parts were read) hands its level to the load
    /// as one with no level started does (openspec AS.49: the level the TV
    /// is in is left for it), the header given back either way.
    #[test]
    fn a_fetch_ending_while_the_tv_is_in_a_level_hands_the_load_its_level() {
        let run = |started: bool| {
            let (to_load, loads) = crossbeam_channel::unbounded();
            let (tx, feed) = crossbeam_channel::unbounded();
            let mut app = App::new();
            app.insert_resource(Level::new(false))
                .insert_resource(LevelChoice {
                    waiting: None,
                    running: Some(Running {
                        name: "level_208_ice".into(),
                        started: Instant::now(),
                        feed,
                        status: Progress::Parts { done: 4, total: 10 },
                    }),
                    scripts_asked: None,
                })
                .insert_resource(Extraction {
                    status: ExtractStatus::Ready {
                        intros: 0,
                        ran: false,
                    },
                    ..Default::default()
                })
                .insert_resource(ExtractBand(Some(band(
                    "level_208_ice",
                    Progress::Parts { done: 4, total: 10 },
                ))))
                .insert_resource(ExtractJob {
                    level: Some(to_load),
                    ..ExtractJob::new(Path::new("."), None, None)
                })
                .add_systems(Update, drain_fetch);
            if started {
                let camera = app.world_mut().spawn_empty().id();
                app.insert_resource(crate::play::Play::started_for_tests(camera));
            }
            app.world_mut()
                .resource_mut::<Level>()
                .set_asked(AskedReport {
                    name: "level_208_ice".into(),
                    state: "extracting".into(),
                    ..Default::default()
                });
            let source = LevelSource::named(
                "extract".into(),
                guac_extract::intros::IrFormat::Ron,
                crate::test_support::turntable::identity(),
                "level_208_ice",
            );
            tx.send(FetchEvent::Done(Ok(Box::new(Fetched {
                source,
                kind: levels::Kind::Story,
                parts_read: 10,
                parts_kept: 0,
                written: 1,
                bytes: 1,
                failed: Vec::new(),
                opened: Duration::ZERO,
                extracted: Duration::ZERO,
            }))))
            .unwrap();
            app.update();
            let asked = app.world().resource::<Level>().asked().cloned().unwrap();
            let sent: Vec<String> = loads.try_iter().map(|s| s.level).collect();
            let band = app.world().resource::<ExtractBand>().0.clone();
            (asked, sent, band)
        };
        for started in [true, false] {
            let (asked, sent, band) = run(started);
            assert_eq!(
                (asked.state.as_str(), sent, asked.parts_read, asked.reason),
                ("loading", vec!["level_208_ice".to_owned()], 10, None),
                "started {started}"
            );
            assert_eq!(band, None, "the header given back");
        }
    }

    /// A level change a script asked for (openspec AS.15) is asked by name
    /// the frame it's made, by the scripts, over one waiting, and once a
    /// run: the same run's held request asks nothing more, and a request
    /// naming no level (a stream) asks nothing.
    #[test]
    fn a_scripts_level_change_is_asked_by_name_once_a_run() {
        use guacamole::scripts::ScriptWorld;
        let mut app = App::new();
        app.insert_resource(Level::new(false))
            .insert_resource(LevelChoice {
                waiting: Some("level_200".into()),
                ..Default::default()
            })
            .add_systems(Update, take_script_asks);
        let (mut world, text) =
            crate::test_support::hero_scripts::world_with_text("Level_214_Tutotial_2");
        world.set_levels(std::sync::Arc::new(
            ["level_214_tutotial_2".to_owned(), "level_200".to_owned()].into(),
        ));
        app.insert_resource(world);
        app.update();
        let waiting = |app: &App| app.world().resource::<LevelChoice>().waiting.clone();
        assert_eq!(
            waiting(&app).as_deref(),
            Some("level_200"),
            "no request yet"
        );
        app.world_mut()
            .resource_mut::<ScriptWorld>()
            .ask_level(text);
        app.world_mut().resource_mut::<ScriptWorld>().end_step();
        app.update();
        assert_eq!(waiting(&app).as_deref(), Some("level_214_tutotial_2"));
        let asked = app.world().resource::<Level>().asked().cloned().unwrap();
        assert_eq!(
            (
                asked.name.as_str(),
                asked.by.as_deref(),
                asked.state.as_str()
            ),
            ("level_214_tutotial_2", Some("scripts"), "waiting")
        );
        // Held, and asked once: a fetch taking the ask isn't asked again.
        app.world_mut().resource_mut::<LevelChoice>().waiting = None;
        app.world_mut().resource_mut::<ScriptWorld>().end_step();
        app.update();
        assert_eq!(waiting(&app), None);
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
