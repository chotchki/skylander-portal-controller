//! First-run extraction (openspec AB.9, design decision 9). A Giants pick
//! checks the dump setup found through guac-extract and either loads a
//! current extract or runs one on its own thread, progress coming back over a
//! channel. Cards don't wait on it: the portal screen draws [`band`] in its
//! header while the thread runs or after it fails. The stages run in
//! guac-extract's order (AE.3): the roster and its portraits, the models, the
//! level (AG.15), then ffmpeg and the intros. The roster goes on the server's
//! figure list the moment it lands, or at start when the extract is already
//! current (AE.4).

use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use bevy::prelude::*;
use crossbeam_channel::{Receiver, Sender, TryRecvError};
use guac_extract::dump::HashProgress;
use guac_extract::intros::IrFormat;
use guac_extract::known::{KNOWN_DUMPS, KnownDump};
use guac_extract::stages::{self, Event, ModelFailure};
use guac_extract::transcode::{Done, Step, TranscodeEvent};
use guac_extract::{DirState, ExtractError, ExtractRoot, IdentityDir, RootMode};
use guac_ir::{CharacterTable, Roster};
use guac_server::figures::{FigureList, FigureSet, RosterState};
use tracing::{error, info, warn};

use crate::level::{LevelSender, LevelSource};
use crate::screen::TvScreen;
use crate::turntable::{Shelf, ShelfSender};
use crate::tv::{Current, Setup};

/// What a Giants pick extracts with; built once in `main` from the settings.
#[derive(Resource, Clone, Debug)]
pub struct ExtractJob {
    pub root: RootMode,
    /// ffmpeg's configured path (settings, `GUAC_FFMPEG` in dev).
    pub ffmpeg: Option<PathBuf>,
    pub ir: IrFormat,
    /// `test-hooks` only: the e2e harness's synthetic dump, trusted beside
    /// [`KNOWN_DUMPS`] (`GUAC_TEST_KNOWN_DUMP`).
    pub test_known: Option<KnownDump>,
    /// The server's figure list, where the roster goes when it lands (AE.4);
    /// `None` when the server didn't come up.
    pub figures: Option<FigureList>,
    /// The TV's turntables, where the extract's models go once they're in
    /// (AE.13); `None` with no TV to tell.
    pub shelf: Option<ShelfSender>,
    /// The TV's level loader, told where chapter 1 is once a pick finds it
    /// in the extract (AG.19); `None` with no TV to tell.
    pub level: Option<LevelSender>,
}

impl ExtractJob {
    /// Release: `extract/` under the data root, postcard. Dev:
    /// `GUAC_EXTRACT_DIR`, required, and RON.
    pub fn new(guac_root: &Path, extract_dir: Option<String>, ffmpeg: Option<PathBuf>) -> Self {
        let (root, ir) = if cfg!(feature = "dev-tools") {
            let extract_dir = extract_dir.map(Into::into);
            (RootMode::Dev { extract_dir }, IrFormat::Ron)
        } else {
            let data_root = guac_root.to_path_buf();
            (RootMode::Release { data_root }, IrFormat::Postcard)
        };
        Self {
            root,
            ffmpeg,
            ir,
            test_known: None,
            figures: None,
            shelf: None,
            level: None,
        }
    }

    /// What [`spawn`] checks the dump against.
    fn known(&self) -> Vec<KnownDump> {
        KNOWN_DUMPS.iter().copied().chain(self.test_known).collect()
    }
}

/// `GUAC_TEST_KNOWN_DUMP`'s value, `<TITLE_ID> <APP_VER> <manifest hash hex>`.
/// The strings are leaked: one per run, and `KnownDump` holds `&'static str`.
pub fn parse_test_known(value: &str) -> Result<KnownDump, String> {
    let parts: Vec<&str> = value.split_whitespace().collect();
    let [title_id, app_ver, hash] = parts[..] else {
        return Err(format!("expected `TITLE_ID APP_VER HASH`, got {value:?}"));
    };
    let hash = hash.to_ascii_lowercase();
    let plain = |s: &str| s.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'.');
    if !plain(title_id) || !plain(app_ver) || guac_ir::Sha256::from_hex(&hash).is_none() {
        return Err(format!("not a title id, version and SHA-256: {value:?}"));
    }
    let leak = |s: &str| -> &'static str { Box::leak(s.to_owned().into_boxed_str()) };
    Ok(KnownDump {
        title_id: leak(title_id),
        app_ver: leak(app_ver),
        manifest_hash: leak(&hash),
        source: "the e2e harness's synthetic dump (GUAC_TEST_KNOWN_DUMP)",
    })
}

/// Where the extraction stands.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub enum ExtractStatus {
    /// Giants hasn't been picked yet this run.
    #[default]
    Idle,
    /// Picked; the thread hasn't reported yet.
    Starting,
    /// Hashing the dump; cached files count as done.
    Checking {
        bytes_done: u64,
        bytes_total: u64,
    },
    /// Reading the figures off the disc, then `done` of `total` portraits.
    Roster {
        done: usize,
        total: usize,
    },
    /// `done` of `total` character models (AE.6).
    Models {
        done: usize,
        total: usize,
    },
    /// `done` of `total` parts of the level (AG.15).
    Level {
        done: usize,
        total: usize,
    },
    /// Mapping toy types to intro movies.
    Indexing,
    /// `done` of `total` intros converted; `movie` is the one in hand.
    Converting {
        done: usize,
        total: usize,
        movie: String,
    },
    /// Intros are there; `ran` when this start did any work.
    Ready {
        intros: usize,
        ran: bool,
    },
    Failed(Failure),
}

impl ExtractStatus {
    /// The variant's name, for logs and the mirror.
    pub fn kind(&self) -> &'static str {
        match self {
            Self::Idle => "Idle",
            Self::Starting => "Starting",
            Self::Checking { .. } => "Checking",
            Self::Roster { .. } => "Roster",
            Self::Models { .. } => "Models",
            Self::Level { .. } => "Level",
            Self::Indexing => "Indexing",
            Self::Converting { .. } => "Converting",
            Self::Ready { .. } => "Ready",
            Self::Failed(_) => "Failed",
        }
    }
}

/// Why there are no intros, as the TV words it. The four the
/// `guacamole/disc-extraction` spec names have their own variant.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Failure {
    UnknownDump {
        report: PathBuf,
    },
    DumpUnreadable {
        path: PathBuf,
        reason: String,
    },
    NoFfmpeg {
        configured: Option<PathBuf>,
        install: String,
    },
    FfmpegUnusable {
        path: PathBuf,
        reason: String,
    },
    InCheckout {
        root: PathBuf,
        repo: PathBuf,
    },
    /// Anything else, by its `GUAC-E-*` code and message.
    Other {
        code: String,
        message: String,
    },
}

impl Failure {
    pub fn code(&self) -> &str {
        match self {
            Self::UnknownDump { .. } => "GUAC-E-UNKNOWN-DUMP",
            Self::DumpUnreadable { .. } => "GUAC-E-DUMP-UNREADABLE",
            Self::NoFfmpeg { .. } => "GUAC-E-NO-FFMPEG",
            Self::FfmpegUnusable { .. } => "GUAC-E-FFMPEG-UNUSABLE",
            Self::InCheckout { .. } => "GUAC-E-EXTRACT-IN-CHECKOUT",
            Self::Other { code, .. } => code,
        }
    }
}

impl From<&ExtractError> for Failure {
    fn from(e: &ExtractError) -> Self {
        match e {
            ExtractError::UnknownDump { report, .. } => Self::UnknownDump {
                report: report.clone(),
            },
            ExtractError::DumpUnreadable { path, reason } => Self::DumpUnreadable {
                path: path.clone(),
                reason: reason.clone(),
            },
            ExtractError::NoFfmpeg {
                configured,
                install,
            } => Self::NoFfmpeg {
                configured: configured.clone(),
                install: (*install).to_owned(),
            },
            ExtractError::FfmpegUnusable { path, reason } => Self::FfmpegUnusable {
                path: path.clone(),
                reason: reason.clone(),
            },
            ExtractError::InCheckout { root, repo } => Self::InCheckout {
                root: root.clone(),
                repo: repo.clone(),
            },
            other => Self::Other {
                code: other.code().to_owned(),
                message: other.to_string(),
            },
        }
    }
}

/// What the portal header draws for a status: nothing once intros are ready.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Band {
    pub title: String,
    pub detail: String,
    /// The bar's fill; `None` draws no bar.
    pub percent: Option<u8>,
    pub code: Option<String>,
}

const GETTING_READY: &str = "Getting Skylanders Giants ready";

/// Nothing to do counts as done.
fn percent(done: u64, total: u64) -> u8 {
    (done.min(total) * 100).checked_div(total).unwrap_or(100) as u8
}

/// The status → TV mapping. Pure, so every case is tested without a window.
pub fn band(status: &ExtractStatus) -> Option<Band> {
    let progress = |detail: String, pct: Option<u8>| Band {
        title: GETTING_READY.into(),
        detail,
        percent: pct,
        code: None,
    };
    let failed = |f: &Failure, title: &str, detail: String| Band {
        title: format!("No intros: {title}"),
        detail: format!("{}: {detail}", f.code()),
        percent: None,
        code: Some(f.code().to_owned()),
    };
    Some(match status {
        ExtractStatus::Idle | ExtractStatus::Ready { .. } => return None,
        ExtractStatus::Starting => progress("Checking your Giants dump…".into(), Some(0)),
        ExtractStatus::Checking {
            bytes_done,
            bytes_total,
        } => {
            let pct = percent(*bytes_done, *bytes_total);
            progress(format!("Checking your Giants dump… {pct}%"), Some(pct))
        }
        ExtractStatus::Roster { done, total } => {
            let pct = percent(*done as u64, *total as u64);
            let detail = match total {
                0 => "Reading the figures off your disc…".into(),
                _ => format!("Reading the figures off your disc… {pct}%"),
            };
            progress(detail, (*total > 0).then_some(pct))
        }
        ExtractStatus::Models { done, total } => {
            let pct = percent(*done as u64, *total as u64);
            progress(
                format!("Reading the characters' models off your disc… {pct}%"),
                Some(pct),
            )
        }
        ExtractStatus::Level { done, total } => {
            let pct = percent(*done as u64, *total as u64);
            progress(
                format!("Reading chapter 1 off your disc… {pct}%"),
                Some(pct),
            )
        }
        ExtractStatus::Indexing => progress("Finding each character's intro…".into(), None),
        ExtractStatus::Converting { done, total, .. } => progress(
            format!("Converting intro {} of {total}…", (done + 1).min(*total)),
            Some(percent(*done as u64, *total as u64)),
        ),
        ExtractStatus::Failed(f) => match f {
            Failure::UnknownDump { report } => failed(
                f,
                "this Giants dump isn't one guacamole knows",
                format!(
                    "nothing was extracted. The list of its files is in {}",
                    report.display()
                ),
            ),
            Failure::DumpUnreadable { path, reason } => failed(
                f,
                "couldn't read the Giants dump",
                format!("looked in {}: {reason}", path.display()),
            ),
            Failure::NoFfmpeg {
                configured,
                install,
            } => {
                let found = match configured {
                    Some(p) => format!("{} isn't a program", p.display()),
                    None => "none was found".into(),
                };
                failed(
                    f,
                    "ffmpeg is needed",
                    format!("{found}. Install it with `{install}`, then start guacamole again"),
                )
            }
            Failure::FfmpegUnusable { path, reason } => failed(
                f,
                "this ffmpeg can't convert them",
                format!("{}: {reason}", path.display()),
            ),
            Failure::InCheckout { root, repo } => failed(
                f,
                "the extract folder is inside a source checkout",
                format!(
                    "{} is inside the git working tree at {}; extracted game files may not \
                     live inside a source checkout",
                    root.display(),
                    repo.display()
                ),
            ),
            Failure::Other { code, message } => Band {
                title: "No intros: the extraction stopped".into(),
                detail: message.clone(),
                percent: None,
                code: Some(code.clone()),
            },
        },
    })
}

/// How long progress stays hidden: a current extract checks out in well under
/// this, so a later start shows no progress at all (the spec's second start).
pub const QUIET_START: Duration = Duration::from_secs(1);

/// [`band`], with progress hidden for the first [`QUIET_START`] of a run.
/// Failures show at once.
pub fn band_after(status: &ExtractStatus, running_for: Duration) -> Option<Band> {
    let progress = matches!(
        status,
        ExtractStatus::Starting
            | ExtractStatus::Checking { .. }
            | ExtractStatus::Roster { .. }
            | ExtractStatus::Models { .. }
            | ExtractStatus::Level { .. }
            | ExtractStatus::Indexing
            | ExtractStatus::Converting { .. }
    );
    if progress && running_for < QUIET_START {
        None
    } else {
        band(status)
    }
}

/// What the extraction thread sends.
#[derive(Debug, Clone)]
pub enum ExtractEvent {
    Hashing(HashProgress),
    /// The roster stage: `done` of `total` portraits, none counted yet while
    /// its archives are read.
    Roster {
        done: usize,
        total: usize,
    },
    /// The model stage: `done` of `total` character models (AE.6).
    Models {
        done: usize,
        total: usize,
    },
    /// The level stage: `done` of `total` parts (AG.15).
    Level {
        done: usize,
        total: usize,
    },
    Indexing,
    Transcode(TranscodeEvent),
    Finished(Result<Ready, Failure>),
}

/// A finished extraction: what AB.10 plays from.
#[derive(Debug, Clone, PartialEq)]
pub struct Ready {
    pub library: IntroLibrary,
    /// False when the extract was already current.
    pub ran: bool,
    pub took: Duration,
}

/// The identity dir and its character table.
#[derive(Resource, Debug, Clone, PartialEq)]
pub struct IntroLibrary {
    pub dir: PathBuf,
    pub table: CharacterTable,
}

impl IntroLibrary {
    /// Toy types with an intro, variants sharing their base's counted.
    pub fn intros(&self) -> usize {
        self.table
            .characters
            .values()
            .filter(|c| c.intro.is_some())
            .count()
    }
}

/// The status after `event`; each event carries the whole of it.
pub fn step(event: &ExtractEvent) -> ExtractStatus {
    match event {
        ExtractEvent::Hashing(p) => ExtractStatus::Checking {
            bytes_done: p.bytes_done,
            bytes_total: p.bytes_total,
        },
        ExtractEvent::Roster { done, total } => ExtractStatus::Roster {
            done: *done,
            total: *total,
        },
        ExtractEvent::Models { done, total } => ExtractStatus::Models {
            done: *done,
            total: *total,
        },
        ExtractEvent::Level { done, total } => ExtractStatus::Level {
            done: *done,
            total: *total,
        },
        ExtractEvent::Indexing => ExtractStatus::Indexing,
        ExtractEvent::Transcode(e) => {
            let done = match e.step {
                Step::Started | Step::Encoding(_) => e.index,
                Step::Done(_) => e.index + 1,
            };
            ExtractStatus::Converting {
                done,
                total: e.total,
                movie: e.movie.clone(),
            }
        }
        ExtractEvent::Finished(Ok(ready)) => ExtractStatus::Ready {
            intros: ready.library.intros(),
            ran: ready.ran,
        },
        ExtractEvent::Finished(Err(f)) => ExtractStatus::Failed(f.clone()),
    }
}

/// Identify, then load the current extract or make it, stage by stage
/// ([`stages::run`]): the roster and its portraits, then ffmpeg, the intro
/// index with the roster's names as the fallback, the IR, the transcode.
/// Blocking.
pub fn run(
    dump: &Path,
    job: &ExtractJob,
    known: &[KnownDump],
    send: &(dyn Fn(ExtractEvent) + Sync),
) -> Result<Ready, ExtractError> {
    let started = Instant::now();
    let root = ExtractRoot::resolve(job.root.clone())?;
    let mut opened = guac_extract::open(dump, &root, known, &|p| send(ExtractEvent::Hashing(p)))?;
    info!(
        dir = %opened.dir.path().display(),
        state = ?opened.state,
        hashed = opened.files_hashed,
        cached = opened.files_cached,
        "giants dump identified"
    );
    if opened.state == DirState::Current {
        let loaded = load_table(&opened.dir, job.ir).and_then(|table| {
            let roster =
                guac_extract::roster::load(&opened.dir, job.ir).map_err(|e| e.to_string())?;
            Ok((table, roster))
        });
        match loaded {
            Ok((table, roster)) => {
                publish(job, &roster, opened.dir.path());
                put_on_shelf(job, &roster, opened.dir.path(), &[]);
                // Current means every part of every level is in.
                send_level(job, &roster, opened.dir.path());
                return Ok(Ready {
                    library: IntroLibrary {
                        dir: opened.dir.path().to_path_buf(),
                        table,
                    },
                    ran: false,
                    took: started.elapsed(),
                });
            }
            Err(why) => warn!("the current extract's IR didn't load, redoing it: {why}"),
        }
    }
    let dir = opened.dir.path().to_path_buf();
    // The roster as it landed, for the shelf once the models are in.
    let landed: Mutex<Option<std::sync::Arc<Roster>>> = Mutex::new(None);
    let extracted = stages::run(
        &mut opened.dir,
        job.ffmpeg.as_deref(),
        job.ir,
        &|e| match e {
            Event::ReadingRoster => send(ExtractEvent::Roster { done: 0, total: 0 }),
            Event::Portraits { done, total } => send(ExtractEvent::Roster { done, total }),
            Event::RosterReady(roster) => {
                publish(job, &roster, &dir);
                *landed.lock().unwrap_or_else(|e| e.into_inner()) = Some(roster);
            }
            Event::Models { done, total } => send(ExtractEvent::Models { done, total }),
            Event::ModelsReady { failed } => {
                if let Some(roster) = &*landed.lock().unwrap_or_else(|e| e.into_inner()) {
                    put_on_shelf(job, roster, &dir, &failed);
                }
            }
            Event::Level { done, total } => send(ExtractEvent::Level { done, total }),
            Event::Indexing => send(ExtractEvent::Indexing),
            Event::Transcode(e) => {
                if let Step::Done(done) = &e.step {
                    let how = match done {
                        Done::Transcoded => "transcoded",
                        Done::Resumed => "already done",
                        Done::Linked { .. } => "linked",
                    };
                    info!(movie = %e.movie, n = e.index + 1, of = e.total, how, "intro");
                }
                if !matches!(e.step, Step::Encoding(_)) {
                    send(ExtractEvent::Transcode(e));
                }
            }
        },
    )?;
    let r = &extracted.roster;
    info!(
        figures = r.roster.figures.len(),
        portraits = r.portraits,
        written = r.files.written,
        kept = r.files.kept,
        bytes = r.files.bytes,
        took_s = r.took.as_secs_f64(),
        "roster written"
    );
    let m = &extracted.models;
    info!(
        models = m.models,
        failed = m.failed.len(),
        heroes = m.heroes,
        unsettled = m.unsettled.len(),
        textures = m.textures,
        binds = m.binds,
        written = m.files.written,
        kept = m.files.kept,
        bytes = m.files.bytes,
        took_s = m.took.as_secs_f64(),
        "models written"
    );
    for f in &m.failed {
        warn!(load_file = %f.load_file, reason = %f.reason, "a model didn't extract; its figures keep their portraits");
    }
    for f in &m.unsettled {
        warn!(load_file = %f.load_file, reason = %f.reason, "a character's walk didn't settle; it keeps its model");
    }
    let l = &extracted.levels;
    info!(
        levels = l.levels,
        read = l.parts_read,
        kept = l.parts_kept,
        failed = l.failed.len(),
        props = l.props,
        refused_props = l.refused_props.len(),
        textures = l.textures,
        shared = l.shared,
        written = l.files.written,
        bytes = l.files.bytes,
        took_s = l.took.as_secs_f64(),
        "level written"
    );
    for f in &l.failed {
        warn!(level = %f.level, part = %f.part, reason = %f.reason, "a level part didn't extract; the next start tries it again");
    }
    // The level is whole before ffmpeg is looked for, so a missing one
    // doesn't keep it from the TV.
    if l.failed.is_empty() && l.levels == guac_extract::levels::LEVELS.len() {
        send_level(job, &extracted.roster.roster, &dir);
    }
    let intros = extracted.intros?;
    let ffmpeg = &intros.ffmpeg;
    info!(ffmpeg = %ffmpeg.path.display(), how = ?ffmpeg.how, version = %ffmpeg.version, "ffmpeg");
    info!(
        transcoded = intros.summary.transcoded,
        resumed = intros.summary.resumed,
        linked = intros.summary.linked,
        took_s = intros.took.as_secs_f64(),
        "intros converted"
    );
    Ok(Ready {
        library: IntroLibrary {
            dir: opened.dir.path().to_path_buf(),
            table: intros.index.table,
        },
        ran: true,
        took: started.elapsed(),
    })
}

/// Puts a landed roster on the server's figure list, its portraits under
/// `dir`, the identity dir (AE.4).
fn publish(job: &ExtractJob, roster: &Roster, dir: &Path) {
    let Some(figures) = &job.figures else {
        return;
    };
    figures.publish(FigureSet::from_roster(roster, dir));
    info!(figures = roster.figures.len(), "roster published");
}

/// Puts the extract at `dir` on the TV's shelf: the turntable draws from it
/// once every model the roster names is in it or in `failed` (AE.13).
fn put_on_shelf(job: &ExtractJob, roster: &Roster, dir: &Path, failed: &[ModelFailure]) {
    let Some(tx) = &job.shelf else {
        return;
    };
    // The app is gone; nothing to tell.
    let _ = tx.send(std::sync::Arc::new(Shelf::new(roster, dir, job.ir, failed)));
}

/// Tells the TV's level loader that chapter 1 is whole in the extract at
/// `dir` (AG.19).
fn send_level(job: &ExtractJob, roster: &Roster, dir: &Path) {
    let Some(tx) = &job.level else {
        return;
    };
    let source = LevelSource::chapter_1(dir.to_path_buf(), job.ir, roster.identity.clone());
    // The app is gone; nothing to tell.
    let _ = tx.send(source);
}

/// At start, the roster of a current extract of `dump`, published on a thread
/// of its own (AE.4): no phone can have picked a game yet. It reads only the
/// dump's listing, the manifest and the roster, writes nothing, and leaves
/// anything short of a current extract to the first Giants pick.
pub fn preload_roster(dump: PathBuf, job: ExtractJob) {
    let spawned = std::thread::Builder::new()
        .name("roster".into())
        .spawn(move || {
            let started = Instant::now();
            let found = ExtractRoot::resolve(job.root.clone())
                .and_then(|root| guac_extract::find_current(&dump, &root, &job.known()));
            match found {
                Ok(Some(dir)) => match guac_extract::roster::load(&dir, job.ir) {
                    Ok(roster) => {
                        publish(&job, &roster, dir.path());
                        put_on_shelf(&job, &roster, dir.path(), &[]);
                        info!(
                            took_ms = started.elapsed().as_millis() as u64,
                            "start-time roster from the current extract"
                        );
                    }
                    Err(e) => warn!("the current extract's roster didn't load: {e}"),
                },
                Ok(None) => info!("no current extract; the roster lands with the Giants pick"),
                Err(e) => info!(code = e.code(), "no start-time roster: {e}"),
            }
        });
    if let Err(e) = spawned {
        warn!("couldn't start the roster thread: {e}");
    }
}

/// The roster state a failed extraction leaves while no roster has landed.
pub fn roster_state_after(f: &Failure) -> RosterState {
    match f {
        Failure::UnknownDump { .. } => RosterState::Unrecognized,
        _ => RosterState::Unreadable,
    }
}

/// The table a current extract holds, if it reads and names its dump.
fn load_table(dir: &IdentityDir, ir: IrFormat) -> Result<CharacterTable, String> {
    let path = dir.path().join(ir.characters_path());
    let table: CharacterTable = match ir {
        IrFormat::Ron => {
            let text = std::fs::read_to_string(&path).map_err(|e| e.to_string())?;
            guac_ir::from_ron(&text).map_err(|e| e.to_string())?
        }
        IrFormat::Postcard => {
            let bytes = std::fs::read(&path).map_err(|e| e.to_string())?;
            guac_ir::from_postcard(&bytes).map_err(|e| e.to_string())?
        }
    };
    if table.identity != dir.manifest().identity {
        return Err(format!("{} names another dump", path.display()));
    }
    Ok(table)
}

/// Runs [`run`] on its own thread; events, then one `Finished`, come back.
pub fn spawn(dump: PathBuf, job: ExtractJob) -> Receiver<ExtractEvent> {
    let (tx, rx) = crossbeam_channel::unbounded();
    let known = job.known();
    let thread = std::thread::Builder::new()
        .name("extract".into())
        .spawn(move || finish(&tx, run(&dump, &job, &known, &|e| send(&tx, e))));
    if let Err(e) = thread {
        let (tx, rx) = crossbeam_channel::bounded(1);
        let message = format!("couldn't start the extraction thread: {e}");
        send(
            &tx,
            ExtractEvent::Finished(Err(Failure::Other {
                code: "GUAC-E-EXTRACT-IO".into(),
                message,
            })),
        );
        return rx;
    }
    rx
}

fn send(tx: &Sender<ExtractEvent>, event: ExtractEvent) {
    // The app is gone; the thread just finishes.
    let _ = tx.send(event);
}

fn finish(tx: &Sender<ExtractEvent>, result: Result<Ready, ExtractError>) {
    let result = result.map_err(|e| {
        error!("extraction: {e}");
        Failure::from(&e)
    });
    send(tx, ExtractEvent::Finished(result));
}

/// The status, its band and the thread's channel.
#[derive(Resource, Default)]
pub struct Extraction {
    pub status: ExtractStatus,
    pub(crate) feed: Option<Receiver<ExtractEvent>>,
    /// When the pick started it.
    pub(crate) started: Option<Instant>,
}

/// What the header draws; changes only when the drawn band does.
#[derive(Resource, Default, Debug, PartialEq, Eq)]
pub struct ExtractBand(pub Option<Band>);

pub struct ExtractPlugin;

impl Plugin for ExtractPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Extraction>()
            .init_resource::<ExtractBand>()
            .add_systems(PreUpdate, drain_extraction)
            .add_systems(Update, start_on_pick);
    }
}

/// The first Giants pick starts it, once per run: a failure stays up until
/// guacamole is started again (the spec's missing-ffmpeg rule).
fn start_on_pick(
    current: Option<Res<Current>>,
    setup: Res<Setup>,
    job: Option<Res<ExtractJob>>,
    mut extraction: ResMut<Extraction>,
) {
    let Some(current) = current else { return };
    if !matches!(current.screen, TvScreen::Portal { .. })
        || extraction.status != ExtractStatus::Idle
    {
        return;
    }
    let figures = job.as_ref().and_then(|j| j.figures.clone());
    let (Some(job), Some(dump)) = (job, setup.0.giants_dir.clone()) else {
        extraction.status = ExtractStatus::Failed(Failure::Other {
            code: "GUAC-E-NO-GIANTS-DUMP".into(),
            message: "setup found no Giants dump to extract from".into(),
        });
        if let Some(figures) = figures {
            figures.set_roster_state(RosterState::Unrecognized);
        }
        warn!("giants picked with no dump or no extract settings");
        return;
    };
    // A folder picked after start can bring the dump: the roster's on its way.
    if let Some(figures) = figures {
        figures.set_roster_state(RosterState::Reading);
    }
    info!(dump = %dump.display(), root = ?job.root, "extraction starting");
    extraction.status = ExtractStatus::Starting;
    extraction.started = Some(Instant::now());
    extraction.feed = Some(spawn(dump, job.clone()));
}

/// The next event, or none yet. A thread gone without `Finished` (a panic in
/// [`run`]) is a failure, so the band never freezes on its last progress.
fn next_event(feed: &Receiver<ExtractEvent>) -> Option<ExtractEvent> {
    match feed.try_recv() {
        Ok(event) => Some(event),
        Err(TryRecvError::Empty) => None,
        Err(TryRecvError::Disconnected) => {
            error!("the extraction thread stopped without finishing");
            Some(ExtractEvent::Finished(Err(Failure::Other {
                code: "GUAC-E-EXTRACT-IO".into(),
                message: "the extraction thread stopped without finishing".into(),
            })))
        }
    }
}

fn drain_extraction(
    mut commands: Commands,
    job: Option<Res<ExtractJob>>,
    mut extraction: ResMut<Extraction>,
    mut drawn: ResMut<ExtractBand>,
) {
    // A `Finished` clears `feed`, so the loop ends there.
    while let Some(event) = extraction.feed.as_ref().and_then(next_event) {
        let status = step(&event);
        if status.kind() != extraction.status.kind() {
            info!(status = status.kind(), "extraction");
        }
        if extraction.status != status {
            extraction.status = status;
        }
        if let ExtractEvent::Finished(result) = event {
            extraction.feed = None;
            match result {
                Ok(ready) => {
                    info!(
                        dir = %ready.library.dir.display(),
                        intros = ready.library.intros(),
                        ran = ready.ran,
                        took_s = ready.took.as_secs_f64(),
                        "extraction ready"
                    );
                    commands.insert_resource(ready.library);
                }
                Err(f) => {
                    warn!(code = f.code(), "extraction failed: {f:?}");
                    // No-op once the roster has landed (a missing ffmpeg).
                    if let Some(figures) = job.as_ref().and_then(|j| j.figures.as_ref()) {
                        figures.set_roster_state(roster_state_after(&f));
                    }
                }
            }
        }
    }
    // While it runs, every frame: the quiet start ends without an event.
    if extraction.feed.is_some() || extraction.is_changed() {
        let running_for = extraction.started.map_or(Duration::MAX, |t| t.elapsed());
        drawn.set_if_neq(ExtractBand(band_after(&extraction.status, running_for)));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn failed(f: Failure) -> Band {
        band(&ExtractStatus::Failed(f)).expect("a failure draws a band")
    }

    #[test]
    fn idle_and_ready_draw_nothing() {
        assert_eq!(band(&ExtractStatus::Idle), None);
        for ran in [false, true] {
            assert_eq!(band(&ExtractStatus::Ready { intros: 53, ran }), None);
        }
    }

    #[test]
    fn progress_waits_out_the_quiet_start_and_failures_dont() {
        let early = QUIET_START / 2;
        let checking = ExtractStatus::Checking {
            bytes_done: 1,
            bytes_total: 2,
        };
        for status in [
            ExtractStatus::Starting,
            checking.clone(),
            ExtractStatus::Indexing,
        ] {
            assert_eq!(band_after(&status, early), None, "{status:?}");
            assert_eq!(band_after(&status, QUIET_START), band(&status));
        }
        let failed = ExtractStatus::Failed(Failure::DumpUnreadable {
            path: "/d".into(),
            reason: "gone".into(),
        });
        assert_eq!(band_after(&failed, Duration::ZERO), band(&failed));
        let ready = ExtractStatus::Ready {
            intros: 53,
            ran: false,
        };
        assert_eq!(band_after(&ready, Duration::ZERO), None);
        assert!(band_after(&checking, Duration::MAX).is_some());
    }

    #[test]
    fn checking_shows_the_hashed_share() {
        let b = band(&ExtractStatus::Checking {
            bytes_done: 1,
            bytes_total: 4,
        })
        .unwrap();
        assert_eq!(b.title, GETTING_READY);
        assert_eq!(b.detail, "Checking your Giants dump… 25%");
        assert_eq!((b.percent, b.code), (Some(25), None));
        // All cached: nothing left to hash.
        let b = band(&ExtractStatus::Checking {
            bytes_done: 0,
            bytes_total: 0,
        })
        .unwrap();
        assert_eq!(b.percent, Some(100));
    }

    #[test]
    fn converting_counts_intros_from_one() {
        let at = |done| {
            band(&ExtractStatus::Converting {
                done,
                total: 48,
                movie: "eruptor".into(),
            })
            .unwrap()
        };
        assert_eq!(at(0).detail, "Converting intro 1 of 48…");
        assert_eq!(at(0).percent, Some(0));
        assert_eq!(at(12).detail, "Converting intro 13 of 48…");
        assert_eq!(at(12).percent, Some(25));
        assert_eq!(at(48).detail, "Converting intro 48 of 48…");
        assert_eq!(at(48).percent, Some(100));
        assert_eq!(band(&ExtractStatus::Indexing).unwrap().percent, None);
        let b = band(&ExtractStatus::Starting).unwrap();
        assert_eq!((b.title.as_str(), b.percent), (GETTING_READY, Some(0)));
    }

    #[test]
    fn the_roster_stage_names_itself_then_counts_portraits() {
        let reading = band(&ExtractStatus::Roster { done: 0, total: 0 }).unwrap();
        assert_eq!(reading.title, GETTING_READY);
        assert_eq!(reading.detail, "Reading the figures off your disc…");
        assert_eq!(reading.percent, None);
        let b = band(&ExtractStatus::Roster {
            done: 41,
            total: 123,
        })
        .unwrap();
        assert_eq!(b.detail, "Reading the figures off your disc… 33%");
        assert_eq!((b.percent, b.code), (Some(33), None));
        let roster = step(&ExtractEvent::Roster { done: 3, total: 9 });
        assert_eq!(roster, ExtractStatus::Roster { done: 3, total: 9 });
        assert_eq!(roster.kind(), "Roster");
        assert_eq!(band_after(&roster, QUIET_START / 2), None);
    }

    #[test]
    fn the_model_stage_names_itself_and_counts_models() {
        let models = step(&ExtractEvent::Models {
            done: 33,
            total: 67,
        });
        assert_eq!(
            models,
            ExtractStatus::Models {
                done: 33,
                total: 67
            }
        );
        assert_eq!(models.kind(), "Models");
        let b = band(&models).unwrap();
        assert_eq!(b.title, GETTING_READY);
        assert_eq!(
            b.detail,
            "Reading the characters' models off your disc… 49%"
        );
        assert_eq!((b.percent, b.code), (Some(49), None));
        assert_eq!(band_after(&models, QUIET_START / 2), None);
    }

    #[test]
    fn the_level_stage_names_chapter_1_and_counts_its_parts() {
        let level = step(&ExtractEvent::Level { done: 2, total: 5 });
        assert_eq!(level, ExtractStatus::Level { done: 2, total: 5 });
        assert_eq!(level.kind(), "Level");
        let b = band(&level).unwrap();
        assert_eq!(b.title, GETTING_READY);
        assert_eq!(b.detail, "Reading chapter 1 off your disc… 40%");
        assert_eq!((b.percent, b.code.as_deref()), (Some(40), None));
        assert_eq!(band_after(&level, QUIET_START / 2), None);
        assert_eq!(band_after(&level, QUIET_START), Some(b));
    }

    #[test]
    fn an_unknown_dump_says_so_with_its_code_and_the_report() {
        let b = failed(Failure::UnknownDump {
            report: "/x/unknown-dump-1.json".into(),
        });
        assert_eq!(b.code.as_deref(), Some("GUAC-E-UNKNOWN-DUMP"));
        assert_eq!(
            b.title,
            "No intros: this Giants dump isn't one guacamole knows"
        );
        assert!(
            b.detail.starts_with("GUAC-E-UNKNOWN-DUMP: "),
            "{}",
            b.detail
        );
        assert!(b.detail.contains("/x/unknown-dump-1.json"));
        assert_eq!(b.percent, None);
    }

    #[test]
    fn an_unreadable_dump_says_where_it_looked() {
        let b = failed(Failure::DumpUnreadable {
            path: "/discs/Giants".into(),
            reason: "No such file or directory (os error 2)".into(),
        });
        assert_eq!(b.code.as_deref(), Some("GUAC-E-DUMP-UNREADABLE"));
        assert_eq!(b.title, "No intros: couldn't read the Giants dump");
        assert!(b.detail.contains("looked in /discs/Giants: No such file"));
    }

    #[test]
    fn missing_ffmpeg_gives_the_install_line() {
        let b = failed(Failure::NoFfmpeg {
            configured: None,
            install: "brew install ffmpeg".into(),
        });
        assert_eq!(b.code.as_deref(), Some("GUAC-E-NO-FFMPEG"));
        assert_eq!(b.title, "No intros: ffmpeg is needed");
        assert!(b.detail.contains("`brew install ffmpeg`"), "{}", b.detail);
        assert!(b.detail.contains("none was found"));
        let b = failed(Failure::NoFfmpeg {
            configured: Some("/nope/ffmpeg".into()),
            install: "brew install ffmpeg".into(),
        });
        assert!(
            b.detail.contains("/nope/ffmpeg isn't a program"),
            "{}",
            b.detail
        );
    }

    #[test]
    fn an_extract_root_in_a_checkout_names_it_and_says_why() {
        let b = failed(Failure::InCheckout {
            root: "/repo/x/extract".into(),
            repo: "/repo".into(),
        });
        assert_eq!(b.code.as_deref(), Some("GUAC-E-EXTRACT-IN-CHECKOUT"));
        assert!(
            b.detail
                .contains("/repo/x/extract is inside the git working tree at /repo")
        );
        assert!(
            b.detail
                .contains("extracted game files may not live inside a source checkout")
        );
    }

    #[test]
    fn other_errors_keep_their_code_and_message() {
        let e = ExtractError::NoExtractDir;
        let f = Failure::from(&e);
        assert_eq!(f.code(), "GUAC-E-NO-EXTRACT-DIR");
        let b = failed(f);
        assert_eq!(b.code.as_deref(), Some("GUAC-E-NO-EXTRACT-DIR"));
        assert_eq!(b.detail, e.to_string());
        let b = failed(Failure::FfmpegUnusable {
            path: "/bin/ffmpeg".into(),
            reason: "no libx264".into(),
        });
        assert_eq!(b.detail, "GUAC-E-FFMPEG-UNUSABLE: /bin/ffmpeg: no libx264");
    }

    #[test]
    fn errors_map_to_their_failures() {
        use guac_ir::{DumpIdentity, Sha256};
        let unknown = ExtractError::UnknownDump {
            identity: DumpIdentity {
                title_id: "BLUS30968".into(),
                app_ver: "01.01".into(),
                manifest_hash: Sha256([0; 32]),
            },
            report: "/r.json".into(),
        };
        assert_eq!(
            Failure::from(&unknown),
            Failure::UnknownDump {
                report: "/r.json".into()
            }
        );
        let no = ExtractError::NoFfmpeg {
            configured: None,
            install: "brew install ffmpeg",
        };
        assert!(
            matches!(Failure::from(&no), Failure::NoFfmpeg { install, .. } if install == "brew install ffmpeg")
        );
        let checkout = ExtractError::InCheckout {
            root: "/a".into(),
            repo: "/".into(),
        };
        assert_eq!(
            Failure::from(&checkout).code(),
            "GUAC-E-EXTRACT-IN-CHECKOUT"
        );
        let failed = ExtractError::TranscodeFailed {
            movie: "m".into(),
            reason: "x".into(),
        };
        assert_eq!(Failure::from(&failed).code(), "GUAC-E-TRANSCODE-FAILED");
    }

    #[test]
    fn events_step_the_status() {
        let s = step(&ExtractEvent::Hashing(HashProgress {
            files_done: 3,
            files_total: 10,
            bytes_done: 30,
            bytes_total: 100,
        }));
        assert_eq!(
            s,
            ExtractStatus::Checking {
                bytes_done: 30,
                bytes_total: 100
            }
        );
        let s = step(&ExtractEvent::Indexing);
        assert_eq!(s, ExtractStatus::Indexing);
        let ev = |index, step| {
            ExtractEvent::Transcode(TranscodeEvent {
                index,
                total: 2,
                movie: "zap".into(),
                step,
            })
        };
        let s = step(&ev(1, Step::Started));
        assert!(matches!(
            s,
            ExtractStatus::Converting {
                done: 1,
                total: 2,
                ..
            }
        ));
        let s = step(&ev(1, Step::Done(Done::Transcoded)));
        assert!(matches!(
            s,
            ExtractStatus::Converting {
                done: 2,
                total: 2,
                ..
            }
        ));
        let f = Failure::NoFfmpeg {
            configured: None,
            install: "brew install ffmpeg".into(),
        };
        let s = step(&ExtractEvent::Finished(Err(f.clone())));
        assert_eq!(s, ExtractStatus::Failed(f));
    }

    /// An app draining `rx` the way guacamole does.
    fn draining(rx: Receiver<ExtractEvent>) -> App {
        let mut app = App::new();
        app.insert_resource(Extraction {
            feed: Some(rx),
            ..Default::default()
        })
        .insert_resource(ExtractJob {
            figures: Some(FigureList::new(FigureSet::waiting(RosterState::Reading))),
            ..ExtractJob::new(Path::new("/r"), None, None)
        })
        .init_resource::<ExtractBand>()
        .add_systems(Update, drain_extraction);
        app
    }

    fn roster_state(app: &App) -> Option<RosterState> {
        let job = app.world().resource::<ExtractJob>();
        job.figures.as_ref().unwrap().now().roster()
    }

    fn hashing() -> ExtractEvent {
        ExtractEvent::Hashing(HashProgress {
            files_done: 1,
            files_total: 2,
            bytes_done: 1,
            bytes_total: 2,
        })
    }

    #[test]
    fn a_thread_gone_without_finishing_is_a_failure() {
        let (tx, rx) = crossbeam_channel::unbounded();
        let mut app = draining(rx);
        tx.send(hashing()).unwrap();
        app.update();
        let x = app.world().resource::<Extraction>();
        assert_eq!(x.status.kind(), "Checking");
        assert!(x.feed.is_some());
        // The thread panics: its sender drops with no `Finished`.
        drop(tx);
        app.update();
        let x = app.world().resource::<Extraction>();
        assert!(x.feed.is_none());
        let ExtractStatus::Failed(f) = &x.status else {
            panic!("{:?}", x.status)
        };
        assert_eq!(f.code(), "GUAC-E-EXTRACT-IO");
        let drawn = app.world().resource::<ExtractBand>();
        assert!(drawn.0.is_some(), "the failure draws its band");
        // No roster came: the phone's note says it couldn't be read.
        assert_eq!(roster_state(&app), Some(RosterState::Unreadable));
    }

    #[test]
    fn a_real_finish_then_the_drop_stays_as_it_finished() {
        use guac_ir::{DumpIdentity, Sha256};
        use std::collections::BTreeMap;
        let library = IntroLibrary {
            dir: "/x".into(),
            table: CharacterTable {
                identity: DumpIdentity {
                    title_id: "BLUS30968".into(),
                    app_ver: "01.00".into(),
                    manifest_hash: Sha256([0; 32]),
                },
                characters: BTreeMap::new(),
            },
        };
        let ready = Ready {
            library: library.clone(),
            ran: false,
            took: Duration::ZERO,
        };
        let no_ffmpeg = Failure::NoFfmpeg {
            configured: None,
            install: "brew install ffmpeg".into(),
        };
        for (finished, want) in [
            (
                Ok(ready),
                ExtractStatus::Ready {
                    intros: 0,
                    ran: false,
                },
            ),
            (Err(no_ffmpeg.clone()), ExtractStatus::Failed(no_ffmpeg)),
        ] {
            let (tx, rx) = crossbeam_channel::unbounded();
            let mut app = draining(rx);
            tx.send(hashing()).unwrap();
            tx.send(ExtractEvent::Finished(finished)).unwrap();
            drop(tx);
            app.update();
            app.update();
            let x = app.world().resource::<Extraction>();
            let ok = matches!(want, ExtractStatus::Ready { .. });
            assert_eq!(x.status, want);
            assert!(x.feed.is_none());
            // Only the Ok run leaves the library AB.10 plays from.
            let left = app.world().get_resource::<IntroLibrary>();
            assert_eq!(left, ok.then_some(&library));
        }
    }

    #[test]
    fn the_test_known_dump_parses_and_joins_the_table() {
        let hash = "ab".repeat(32);
        let k = parse_test_known(&format!(" BLUS30968  01.00 {} ", hash.to_uppercase())).unwrap();
        assert_eq!(
            (k.title_id, k.app_ver, k.manifest_hash),
            ("BLUS30968", "01.00", hash.as_str())
        );
        for bad in [
            String::new(),
            "BLUS30968 01.00".into(),
            "BLUS30968 01.00 zz".into(),
            format!("BLUS/30968 01.00 {hash}"),
            format!("BLUS30968 01.00 {hash} extra"),
        ] {
            assert!(parse_test_known(&bad).is_err(), "{bad:?}");
        }
        let job = ExtractJob::new(Path::new("/r"), None, None);
        assert_eq!(job.known(), KNOWN_DUMPS);
        let job = ExtractJob {
            test_known: Some(k),
            ..job
        };
        assert_eq!(job.known().len(), KNOWN_DUMPS.len() + 1);
        assert_eq!(job.known().last(), Some(&k));
    }
}

/// [`run`] on synthetic dumps against a fake ffmpeg (a shell script), so
/// unix only; the real dump and ffmpeg are the Mac run's (AB.9's note).
#[cfg(all(test, unix))]
mod run_tests {
    use std::fs;
    use std::os::unix::fs::PermissionsExt;
    use std::sync::Mutex;

    use guac_formats::iga::builder::IgaBuilder;
    use guac_formats::level::builder::{stage_level, stage_sample};
    use guac_formats::roster::builder::{disc, sample};
    use guac_formats::sfo::builder::SfoBuilder;

    use super::*;

    const LISTING: &str = "Encoders:\\n ------\\n V....D libx264  x\\n A....D libmp3lame  y\\n";
    const DECODERS: &str = "Decoders:\\n ------\\n V....D binkvideo  x\\n A....D binkaudio_dct  y\\n A....D binkaudio_rdft  z\\n";

    /// Copies the input into each output; fails on `jetvac` while `<dir>/fail`
    /// exists.
    fn fake_ffmpeg(dir: &Path) -> PathBuf {
        fs::create_dir_all(dir).unwrap();
        let fail = dir.join("fail");
        let script = format!(
            r#"#!/bin/sh
case "$1" in
  -version) echo "ffmpeg version fake"; exit 0;;
  -hide_banner)
    case "$2" in
      -encoders) printf '{LISTING}'; exit 0;;
      -decoders) printf '{DECODERS}'; exit 0;;
    esac;;
esac
prev=''; in=''
for a in "$@"; do
  if [ "$prev" = "-i" ]; then in="${{a#file:}}"; fi
  prev="$a"
done
case "$in" in *jetvac*) if [ -e '{fail}' ]; then echo "jetvac: broken" >&2; exit 1; fi;; esac
prev=''
for a in "$@"; do
  case "$a" in file:*) if [ "$prev" != "-i" ]; then cat "$in" > "${{a#file:}}"; fi;; esac
  prev="$a"
done
printf 'frame=10\nout_time_us=1000000\nprogress=end\n'
"#,
            fail = fail.display(),
        );
        let p = dir.join("ffmpeg");
        fs::write(&p, script).unwrap();
        fs::set_permissions(&p, fs::Permissions::from_mode(0o755)).unwrap();
        p
    }

    /// The roster builder's sample disc, plus two intros: Eruptor maps by
    /// his voice-over name, Jet-Vac (toy 100) by the roster's; and the level
    /// builder's chapter 1 (AG.15), which a current extract holds.
    fn make_dump(tmp: &Path) -> PathBuf {
        let dump = tmp.join("discs/Giants");
        let usr = dump.join("PS3_GAME/USRDIR");
        fs::create_dir_all(usr.join("movies")).unwrap();
        fs::create_dir_all(usr.join("character")).unwrap();
        fs::write(
            dump.join("PS3_GAME/PARAM.SFO"),
            SfoBuilder::disc_game("BLUS30968", "01.00", "01.00").build(),
        )
        .unwrap();
        for (path, bytes) in disc(&sample()).archives() {
            let path = usr.join(path);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, bytes).unwrap();
        }
        let vo = "champion_audio/eruptor/vo_eruptor_01.wav/0.wav.hz.wav.enc";
        let eruptor = IgaBuilder::new().stored(vo, b"wav").build();
        fs::write(usr.join("character/009_eruptor.arc"), eruptor).unwrap();
        let eagle = IgaBuilder::new().stored("model.igz", b"igz").build();
        fs::write(usr.join("character/100_eagle.arc"), eagle).unwrap();
        fs::write(usr.join("movies/eruptor.mov"), b"eruptor bink").unwrap();
        fs::write(usr.join("movies/jetvac.mov"), b"jetvac bink").unwrap();
        let level = guac_extract::levels::archive_path(&usr, guac_extract::levels::LEVELS[0]);
        fs::create_dir_all(level.parent().unwrap()).unwrap();
        let bld = stage_level(&stage_sample());
        fs::write(level, IgaBuilder::new().stored("level.bld", &bld).build()).unwrap();
        dump
    }

    fn known_for(dump: &Path) -> Vec<KnownDump> {
        let scan = guac_extract::dump::scan(dump, &Default::default(), &|_| {}).unwrap();
        vec![KnownDump {
            title_id: "BLUS30968",
            app_ver: "01.00",
            manifest_hash: Box::leak(scan.identity.manifest_hash.to_string().into_boxed_str()),
            source: "test",
        }]
    }

    fn job(extract: &Path, ffmpeg: Option<PathBuf>) -> ExtractJob {
        ExtractJob {
            root: RootMode::Dev {
                extract_dir: Some(extract.as_os_str().to_owned()),
            },
            ffmpeg,
            ir: IrFormat::Ron,
            test_known: None,
            figures: Some(FigureList::new(FigureSet::waiting(RosterState::Reading))),
            shelf: None,
            level: None,
        }
    }

    /// The figure list `job` publishes to, as it is now.
    fn listed(job: &ExtractJob) -> std::sync::Arc<FigureSet> {
        job.figures.as_ref().unwrap().now()
    }

    fn run_all(
        dump: &Path,
        job: &ExtractJob,
        known: &[KnownDump],
    ) -> (Result<Ready, ExtractError>, Vec<ExtractStatus>) {
        let seen = Mutex::new(Vec::new());
        let result = run(dump, job, known, &|e| {
            seen.lock().unwrap().push(step(&e));
        });
        (result, seen.into_inner().unwrap())
    }

    #[test]
    fn a_known_dump_extracts_then_a_restart_loads_it_and_a_failure_resumes() {
        let tmp = tempfile::tempdir().unwrap();
        let dump = make_dump(tmp.path());
        let known = known_for(&dump);
        let ffmpeg = fake_ffmpeg(&tmp.path().join("bin"));
        let extract = tmp.path().join("extract");
        let (shelf, shelves) = crate::turntable::shelf_channel();
        let job = ExtractJob {
            shelf: Some(shelf),
            ..job(&extract, Some(ffmpeg.clone()))
        };

        // The first run stops at jetvac, as a kill mid-run would.
        fs::write(ffmpeg.with_file_name("fail"), "").unwrap();
        let (result, seen) = run_all(&dump, &job, &known);
        let err = result.unwrap_err();
        assert_eq!(err.code(), "GUAC-E-TRANSCODE-FAILED", "{err}");
        // The models went on the TV's shelf before the intros failed (AE.13):
        // this dump has no character archives, so each of the roster's 11
        // load files is there as reported, the rig beside them.
        let landed: Vec<_> = shelves.0.try_iter().collect();
        assert_eq!(landed.len(), 1);
        assert_eq!(landed[0].failed.len(), 11, "{:?}", landed[0].failed);
        assert!(
            landed[0]
                .failed
                .values()
                .all(|why| why.contains("no archive")),
            "{:?}",
            landed[0].failed
        );
        assert!(landed[0].rig.is_ok(), "{:?}", landed[0].rig);
        assert!(!landed[0].loads.is_empty());
        for (id, load) in &landed[0].loads {
            let model = landed[0].dir.join(job.ir.model_path(load));
            assert!(
                model.is_file() || landed[0].failed.contains_key(load),
                "{id}: {load}"
            );
        }
        let mut kinds: Vec<&str> = seen.iter().map(ExtractStatus::kind).collect();
        kinds.dedup();
        assert_eq!(
            kinds,
            [
                "Checking",
                "Roster",
                "Models",
                "Level",
                "Indexing",
                "Converting"
            ]
        );
        // The roster went on the list before the intros failed (AE.4).
        assert_eq!(listed(&job).roster(), Some(RosterState::Ready));
        assert_eq!(listed(&job).figures().len(), 24);
        assert!(
            seen.contains(&ExtractStatus::Roster {
                done: 24,
                total: 24
            }),
            "{seen:?}"
        );

        // Next start: eruptor is kept, jetvac is done now.
        fs::remove_file(ffmpeg.with_file_name("fail")).unwrap();
        let (result, seen) = run_all(&dump, &job, &known);
        let ready = result.unwrap();
        assert!(ready.ran);
        assert_eq!(ready.library.intros(), 2);
        let table = &ready.library.table;
        assert_eq!(
            table.characters[&9].intro.as_ref().unwrap().source,
            guac_ir::IntroSource::Disc
        );
        // By the roster's English name, not the wiki's.
        assert_eq!(
            table.characters[&100].intro.as_ref().unwrap().source,
            guac_ir::IntroSource::NameFallback
        );
        assert!(matches!(
            seen.last(),
            Some(ExtractStatus::Converting {
                done: 2,
                total: 2,
                ..
            })
        ));
        let dir = &ready.library.dir;
        assert_eq!(
            fs::read(dir.join("video/intro/jetvac.mp4")).unwrap(),
            b"jetvac bink"
        );
        assert!(dir.join("ir/characters.ron").is_file());
        assert!(dir.join("ir/roster.ron").is_file());
        assert!(dir.join("portraits/000064-0000.png").is_file());

        // A restart: current, loaded from the IR, nothing converted, and
        // the roster published off the extract.
        let restarted = ExtractJob {
            figures: Some(FigureList::new(FigureSet::waiting(RosterState::Reading))),
            ..job.clone()
        };
        let (result, seen) = run_all(&dump, &restarted, &known);
        let again = result.unwrap();
        assert!(!again.ran);
        // A current extract goes on the shelf as it's loaded.
        assert_eq!(shelves.0.try_iter().count(), 2, "the second run's and this");
        assert_eq!(again.library, ready.library);
        assert!(seen.iter().all(|s| s.kind() == "Checking"), "{seen:?}");
        let set = listed(&restarted);
        assert_eq!(set.figures().len(), 24);
        let jet_vac = skylander_core::FigureId::new("000064-0000");
        assert_eq!(
            set.portrait(&jet_vac),
            Some(dir.join("portraits/000064-0000.png").as_path())
        );
    }

    /// AE.4: at start, a current extract's roster lands with no pick; one
    /// that isn't complete leaves the list waiting for the pick.
    #[test]
    fn a_start_preloads_only_a_current_extracts_roster() {
        let tmp = tempfile::tempdir().unwrap();
        let dump = make_dump(tmp.path());
        let known = known_for(&dump);
        let extract = tmp.path().join("extract");
        let k = known[0];
        let trusted = |job: ExtractJob| ExtractJob {
            test_known: Some(k),
            ..job
        };
        let wait = |job: &ExtractJob, want: usize| {
            let deadline = Instant::now() + Duration::from_secs(10);
            while listed(job).figures().len() != want {
                assert!(Instant::now() < deadline, "{:?}", job.figures);
                std::thread::sleep(Duration::from_millis(10));
            }
        };

        // Nothing extracted, then the roster stage alone: no ffmpeg.
        let missing = tmp.path().join("nope/ffmpeg");
        let early = trusted(job(&extract, None));
        preload_roster(dump.clone(), early.clone());
        std::thread::sleep(Duration::from_millis(200));
        assert_eq!(listed(&early).roster(), Some(RosterState::Reading));
        let (result, _) = run_all(&dump, &job(&extract, Some(missing)), &known);
        assert!(result.is_err());
        let partial = trusted(job(&extract, None));
        preload_roster(dump.clone(), partial.clone());
        std::thread::sleep(Duration::from_millis(200));
        assert!(listed(&partial).is_waiting(), "an incomplete extract");

        let ffmpeg = fake_ffmpeg(&tmp.path().join("bin"));
        run_all(&dump, &job(&extract, Some(ffmpeg)), &known)
            .0
            .unwrap();
        let start = trusted(job(&extract, None));
        preload_roster(dump.clone(), start.clone());
        wait(&start, 24);
        assert_eq!(listed(&start).roster(), Some(RosterState::Ready));
    }

    #[test]
    fn a_failure_before_the_roster_says_why_on_the_list() {
        let report = PathBuf::from("/x/report.txt");
        assert_eq!(
            roster_state_after(&Failure::UnknownDump { report }),
            RosterState::Unrecognized
        );
        for f in [
            Failure::DumpUnreadable {
                path: "/d".into(),
                reason: "gone".into(),
            },
            Failure::Other {
                code: "GUAC-E-ROSTER-UNREADABLE".into(),
                message: "init_setup".into(),
            },
            Failure::Other {
                code: "GUAC-E-NO-EXTRACT-DIR".into(),
                message: "unset".into(),
            },
        ] {
            assert_eq!(roster_state_after(&f), RosterState::Unreadable, "{f:?}");
        }
    }

    #[test]
    fn a_known_dump_without_ffmpeg_gets_its_roster_and_no_intros() {
        let tmp = tempfile::tempdir().unwrap();
        let dump = make_dump(tmp.path());
        let known = known_for(&dump);
        let missing = tmp.path().join("nope/ffmpeg");
        let extract = tmp.path().join("extract");
        let job = job(&extract, Some(missing.clone()));
        let (result, seen) = run_all(&dump, &job, &known);
        let f = Failure::from(&result.unwrap_err());
        assert_eq!(
            f,
            Failure::NoFfmpeg {
                configured: Some(missing),
                install: guac_extract::ffmpeg::INSTALL_LINE.into(),
            }
        );
        assert!(
            seen.iter()
                .all(|s| ["Checking", "Roster", "Models", "Level"].contains(&s.kind())),
            "{seen:?}"
        );
        // The roster and its portraits are there; the intros aren't.
        let dir = fs::read_dir(&extract)
            .unwrap()
            .next()
            .unwrap()
            .unwrap()
            .path();
        assert!(dir.join("ir/roster.ron").is_file());
        let portraits = fs::read_dir(dir.join("portraits")).unwrap().count();
        assert_eq!(portraits, 24);
        assert!(!dir.join("ir/characters.ron").exists());
        assert!(!dir.join("video").exists());
    }

    #[test]
    fn an_unknown_dump_is_reported_beside_the_root() {
        let tmp = tempfile::tempdir().unwrap();
        let dump = make_dump(tmp.path());
        let job = job(&tmp.path().join("x/extract"), None);
        let (result, _) = run_all(&dump, &job, KNOWN_DUMPS);
        let Failure::UnknownDump { report } = Failure::from(&result.unwrap_err()) else {
            panic!("not an unknown dump");
        };
        assert_eq!(report.parent(), Some(tmp.path().join("x").as_path()));
        assert!(report.is_file());
        assert!(!tmp.path().join("x/extract").exists());
    }

    #[test]
    fn a_missing_dump_is_unreadable_and_names_it() {
        let tmp = tempfile::tempdir().unwrap();
        let gone = tmp.path().join("discs/Giants");
        let job = job(&tmp.path().join("extract"), None);
        let (result, _) = run_all(&gone, &job, KNOWN_DUMPS);
        let Failure::DumpUnreadable { path, .. } = Failure::from(&result.unwrap_err()) else {
            panic!("not unreadable");
        };
        assert_eq!(path, gone);
    }

    #[test]
    fn a_root_in_a_checkout_and_no_root_are_refused() {
        let tmp = tempfile::tempdir().unwrap();
        let dump = make_dump(tmp.path());
        let repo = tmp.path().join("repo");
        fs::create_dir_all(repo.join(".git")).unwrap();
        let job_in = job(&repo.join("extract"), None);
        let (result, _) = run_all(&dump, &job_in, KNOWN_DUMPS);
        let Failure::InCheckout { root, repo: found } = Failure::from(&result.unwrap_err()) else {
            panic!("not refused");
        };
        assert_eq!((root, found), (repo.join("extract"), repo.clone()));

        let unset = ExtractJob {
            root: RootMode::Dev { extract_dir: None },
            ..job_in
        };
        let (result, seen) = run_all(&dump, &unset, KNOWN_DUMPS);
        assert_eq!(result.unwrap_err().code(), "GUAC-E-NO-EXTRACT-DIR");
        assert!(seen.is_empty());
    }

    #[test]
    fn a_synthetic_dump_is_known_only_through_test_known() {
        let tmp = tempfile::tempdir().unwrap();
        let dump = make_dump(tmp.path());
        let ffmpeg = fake_ffmpeg(&tmp.path().join("bin"));
        let mut job = job(&tmp.path().join("extract"), Some(ffmpeg));
        let (result, _) = run_all(&dump, &job, &job.known());
        assert_eq!(result.unwrap_err().code(), "GUAC-E-UNKNOWN-DUMP");
        let k = known_for(&dump)[0];
        let line = format!("{} {} {}", k.title_id, k.app_ver, k.manifest_hash);
        job.test_known = Some(parse_test_known(&line).unwrap());
        let (result, _) = run_all(&dump, &job, &job.known());
        assert_eq!(result.unwrap().library.intros(), 2);
    }
}
