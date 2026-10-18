//! First-run extraction (openspec AB.9, design decision 9). A Giants pick
//! checks the dump setup found through guac-extract and either loads a
//! current extract or runs one on its own thread, progress coming back over a
//! channel. Cards don't wait on it: the portal screen draws [`band`] in its
//! header while the thread runs or after it fails.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use bevy::prelude::*;
use crossbeam_channel::{Receiver, Sender, TryRecvError};
use guac_extract::dump::HashProgress;
use guac_extract::ffmpeg::Ffmpeg;
use guac_extract::intros::{self, IrFormat};
use guac_extract::known::{KNOWN_DUMPS, KnownDump};
use guac_extract::transcode::{self, Done, Step, TranscodeEvent};
use guac_extract::{DirState, ExtractError, ExtractRoot, IdentityDir, RootMode};
use guac_ir::CharacterTable;
use tracing::{error, info, warn};

use crate::screen::TvScreen;
use crate::tv::{Current, Setup};

/// What a Giants pick extracts with; built once in `main` from the settings.
#[derive(Resource, Clone, Debug)]
pub struct ExtractJob {
    pub root: RootMode,
    /// ffmpeg's configured path (settings, `GUAC_FFMPEG` in dev).
    pub ffmpeg: Option<PathBuf>,
    /// The interim catalogue's folder: the intro index's name fallback.
    pub data_root: PathBuf,
    pub ir: IrFormat,
}

impl ExtractJob {
    /// Release: `extract/` under the data root, postcard. Dev:
    /// `GUAC_EXTRACT_DIR`, required, and RON.
    pub fn new(
        guac_root: &Path,
        extract_dir: Option<String>,
        ffmpeg: Option<PathBuf>,
        catalogue: PathBuf,
    ) -> Self {
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
            data_root: catalogue,
            ir,
        }
    }
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

const GETTING_READY: &str = "Getting the Skylanders Giants intros ready";

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

/// Identify, then load the current extract or make it: the intro index with
/// the catalogue's names as the fallback, the IR, the transcode. Blocking.
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
        match load_table(&opened.dir, job.ir) {
            Ok(table) => {
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
    let ffmpeg = Ffmpeg::locate(job.ffmpeg.as_deref())?;
    info!(ffmpeg = %ffmpeg.path.display(), how = ?ffmpeg.how, version = %ffmpeg.version, "ffmpeg");
    send(ExtractEvent::Indexing);
    let index = intros::index(&opened.dir, &fallback_names(&job.data_root))?;
    intros::write(&mut opened.dir, &index.table, job.ir)?;
    let summary = transcode::transcode_intros(&mut opened.dir, &index.table, &ffmpeg, &|e| {
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
    })?;
    opened.dir.mark_complete()?;
    info!(
        transcoded = summary.transcoded,
        resumed = summary.resumed,
        linked = summary.linked,
        "intros converted"
    );
    Ok(Ready {
        library: IntroLibrary {
            dir: opened.dir.path().to_path_buf(),
            table: index.table,
        },
        ran: true,
        took: started.elapsed(),
    })
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

/// Base names by toy type; none if the catalogue won't load (the disc's own
/// names still map every Giants intro).
fn fallback_names(data_root: &Path) -> BTreeMap<u32, String> {
    match guac_server::native_catalogue::load(data_root) {
        Ok(figures) => guac_server::native_catalogue::base_names(&figures),
        Err(e) => {
            warn!("no name fallback for the intro index: {e:#}");
            BTreeMap::new()
        }
    }
}

/// Runs [`run`] on its own thread; events, then one `Finished`, come back.
pub fn spawn(dump: PathBuf, job: ExtractJob) -> Receiver<ExtractEvent> {
    let (tx, rx) = crossbeam_channel::unbounded();
    let thread = std::thread::Builder::new()
        .name("extract".into())
        .spawn(move || finish(&tx, run(&dump, &job, KNOWN_DUMPS, &|e| send(&tx, e))));
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
    let (Some(job), Some(dump)) = (job, setup.0.giants_dir.clone()) else {
        extraction.status = ExtractStatus::Failed(Failure::Other {
            code: "GUAC-E-NO-GIANTS-DUMP".into(),
            message: "setup found no Giants dump to extract from".into(),
        });
        warn!("giants picked with no dump or no extract settings");
        return;
    };
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
                Err(f) => warn!(code = f.code(), "extraction failed: {f:?}"),
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
        .init_resource::<ExtractBand>()
        .add_systems(Update, drain_extraction);
        app
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
    }

    #[test]
    fn a_real_finish_then_the_drop_stays_as_it_finished() {
        use guac_ir::{DumpIdentity, Sha256};
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
}

/// [`run`] on synthetic dumps against a fake ffmpeg (a shell script), so
/// unix only; the real dump and ffmpeg are the Mac run's (AB.9's note).
#[cfg(all(test, unix))]
mod run_tests {
    use std::fs;
    use std::os::unix::fs::PermissionsExt;
    use std::sync::Mutex;

    use guac_formats::iga::builder::IgaBuilder;
    use guac_formats::sfo::builder::SfoBuilder;

    use super::*;

    const LISTING: &str = "Encoders:\\n ------\\n V....D libx264  x\\n A....D libmp3lame  y\\n";
    const DECODERS: &str = "Decoders:\\n ------\\n V....D binkvideo  x\\n A....D binkaudio_dct  y\\n A....D binkaudio_rdft  z\\n";

    /// Copies the input into each output; fails on `zap` while `<dir>/fail` exists.
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
case "$in" in *zap*) if [ -e '{fail}' ]; then echo "zap: broken" >&2; exit 1; fi;; esac
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

    /// Eruptor maps by its voice-over name, Zap (toy 12) by the catalogue's.
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
        let vo = "champion_audio/eruptor/vo_eruptor_01.wav/0.wav.hz.wav.enc";
        let eruptor = IgaBuilder::new().stored(vo, b"wav").build();
        fs::write(usr.join("character/009_eruptor.arc"), eruptor).unwrap();
        let zap = IgaBuilder::new().stored("model.igz", b"igz").build();
        fs::write(usr.join("character/012_zap.arc"), zap).unwrap();
        fs::write(usr.join("movies/eruptor.mov"), b"eruptor bink").unwrap();
        fs::write(usr.join("movies/zap.mov"), b"zap bink").unwrap();
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

    /// The committed catalogue, whose name for toy 12 the fallback takes.
    fn catalogue(tmp: &Path) -> PathBuf {
        let data = tmp.join("data");
        fs::create_dir_all(&data).unwrap();
        let real = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../data/figures.json");
        fs::copy(real, data.join("figures.json")).unwrap();
        data
    }

    fn job(tmp: &Path, extract: &Path, ffmpeg: Option<PathBuf>) -> ExtractJob {
        ExtractJob {
            root: RootMode::Dev {
                extract_dir: Some(extract.as_os_str().to_owned()),
            },
            ffmpeg,
            data_root: catalogue(tmp),
            ir: IrFormat::Ron,
        }
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
        let job = job(tmp.path(), &extract, Some(ffmpeg.clone()));

        // The first run stops at zap, as a kill mid-run would.
        fs::write(ffmpeg.with_file_name("fail"), "").unwrap();
        let (result, seen) = run_all(&dump, &job, &known);
        let err = result.unwrap_err();
        assert_eq!(err.code(), "GUAC-E-TRANSCODE-FAILED", "{err}");
        let kinds: Vec<&str> = seen.iter().map(ExtractStatus::kind).collect();
        assert_eq!(kinds.first(), Some(&"Checking"));
        assert!(
            kinds.contains(&"Indexing") && kinds.contains(&"Converting"),
            "{kinds:?}"
        );

        // Next start: eruptor is kept, zap is done now.
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
        assert_eq!(
            table.characters[&12].intro.as_ref().unwrap().source,
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
            fs::read(dir.join("video/intro/zap.mp4")).unwrap(),
            b"zap bink"
        );
        assert!(dir.join("ir/characters.ron").is_file());

        // A restart: current, loaded from the IR, nothing converted.
        let (result, seen) = run_all(&dump, &job, &known);
        let again = result.unwrap();
        assert!(!again.ran);
        assert_eq!(again.library, ready.library);
        assert!(seen.iter().all(|s| s.kind() == "Checking"), "{seen:?}");
    }

    #[test]
    fn a_known_dump_without_ffmpeg_extracts_nothing() {
        let tmp = tempfile::tempdir().unwrap();
        let dump = make_dump(tmp.path());
        let known = known_for(&dump);
        let missing = tmp.path().join("nope/ffmpeg");
        let job = job(
            tmp.path(),
            &tmp.path().join("extract"),
            Some(missing.clone()),
        );
        let (result, seen) = run_all(&dump, &job, &known);
        let f = Failure::from(&result.unwrap_err());
        assert_eq!(
            f,
            Failure::NoFfmpeg {
                configured: Some(missing),
                install: guac_extract::ffmpeg::INSTALL_LINE.into(),
            }
        );
        assert!(seen.iter().all(|s| s.kind() == "Checking"));
    }

    #[test]
    fn an_unknown_dump_is_reported_beside_the_root() {
        let tmp = tempfile::tempdir().unwrap();
        let dump = make_dump(tmp.path());
        let job = job(tmp.path(), &tmp.path().join("x/extract"), None);
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
        let job = job(tmp.path(), &tmp.path().join("extract"), None);
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
        let job_in = job(tmp.path(), &repo.join("extract"), None);
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
}
