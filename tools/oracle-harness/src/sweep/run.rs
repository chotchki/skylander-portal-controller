//! One figure placed over the gameplay state and read, as the module doc
//! lays it out.

use std::fs;
use std::path::Path;
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use skylander_core::SlotIndex;
use skylander_rpcs3_control::PortalDriver;
use skylander_rpcs3_control::ipc::proto::PadButton;

use super::attach::{self, HeroAttachments};
use super::list::SweepFigure;
use crate::actors::{self, Actor};
use crate::divergence::{EndShot, Host, pine_connect};
use crate::hero::{self, Hero};
use crate::home::OracleHome;
use crate::identity::Identity;
use crate::launch::{self, BootOptions};
use crate::outputs::{Kind, Rpcs3Outputs, hex};
use crate::rpcs3_log::{LogTail, opened_path};

/// A warm home reaches Start Paused's pause in 4 to 7 s; AF.11's hung
/// restores are what this bounds.
pub const BOOT_WAIT: Duration = Duration::from_secs(120);
/// Flips from the new hero's first read to the one the sweep reads and shoots:
/// 5 s at the game's 60, past the swap-in.
pub const SETTLE: u64 = 300;
/// Each listen for the game's portal queries before the next `RECONNECT`.
const QUERY_WATCH: Duration = Duration::from_secs(3);
/// `RECONNECT`s after F0's before the sweep gives the boot up (AE.22's held
/// boot needed one).
const MAX_RECONNECTS: u32 = 4;
/// From the first query to a new hero (Stealth Elf's took 5 s).
const HERO_WAIT: Duration = Duration::from_secs(90);
/// Flips ahead of the counter a poll pauses at.
const POLL_AHEAD: u64 = 30;
/// Polls in a row with the figure's archive open and no hero at all before
/// the sweep presses CROSS: a prompt is holding the swap (Eye-Brawl's pack
/// figure is "visiting from another adventure", and the game asks whether to
/// show visiting nicknames, NO highlighted, before it brings him in).
const PROMPT_POLLS: u32 = 3;
/// CROSS presses a figure gets before the sweep gives the boot up.
const MAX_PRESSES: u32 = 4;
/// The state's own character (Eruptor and his Series 2 and LightCore
/// figures) may keep its archive and its placement: its hero counts once this
/// many flips have run since the game queried the portal.
const OWN_WAIT: u64 = 300;
const STEP_WAIT: Duration = Duration::from_secs(180);
/// RPCS3's log when the template's silent audio took (AF.3's review).
const SILENT: &[u8] = b"cellAudio initializing. Backend: Null";
const FATAL: &[u8] = b"Thread terminated due to fatal error";
/// The shot's name in the figure's dir.
pub const SHOT_FILE: &str = "hero.png";
/// Where a figure's [`FigureRecord`] lands, beside its shot: a rerun into
/// the same dir keeps a figure that has one.
pub const RECORD_FILE: &str = "figure.json";
/// Every actor at the read pause.
pub const ACTORS_FILE: &str = "actors.json";

/// What every figure's boot shares.
pub struct Inputs<'a> {
    pub exe: &'a Path,
    pub home: &'a OracleHome,
    /// The gameplay savestate each figure boots.
    pub state: &'a Path,
    /// The pack the figures' `.sky` files come from, read only.
    pub pack: &'a Path,
    pub boot_wait: Duration,
    pub settle: u64,
}

/// A character archive the game opened.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Opened {
    pub path: String,
    /// Since the run resumed from F0.
    pub after_secs: f64,
}

/// One figure's sweep, as `figure.json` keeps it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FigureRecord {
    pub identity: String,
    pub figure: SweepFigure,
    pub sky_sha256: String,
    pub state: String,
    pub state_sha256: String,
    pub before: Host,
    pub after: Host,
    pub boot_secs: f64,
    pub f0: u64,
    /// The state's hero at F0 (`009_Eruptor` in AF.8's).
    pub f0_hero: Option<String>,
    /// `RECONNECT`s after F0's before the game queried the portal.
    pub reconnects: u32,
    /// `PE cmd=query` pushes seen in the listen that ended them.
    pub queries: u32,
    /// CROSS presses (P6) answering a prompt that held the swap, its
    /// highlighted choice taken.
    #[serde(default)]
    pub presses: u32,
    /// The figure's character archive opening, when it wasn't the state's.
    pub archive: Option<Opened>,
    /// The flip the new hero was first read at, and the one read and shot.
    pub hero_at: u64,
    pub read_at: u64,
    pub game_frame: u64,
    pub hero: Actor,
    /// The hero's placement is named for the figure's load file.
    pub hero_is_load_file: bool,
    /// Scale, `CollisionInfo`, `ActorParameters` and solid (AG.27's check).
    pub tuning: Option<Hero>,
    pub tuning_error: Option<String>,
    pub attachments: Option<HeroAttachments>,
    pub attachments_error: Option<String>,
    pub shot: EndShot,
    pub silent: bool,
    pub fatal: Vec<String>,
    pub wall_secs: f64,
    pub shutdown: String,
}

impl FigureRecord {
    pub fn load(path: &Path) -> Result<Self> {
        let text =
            fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
        serde_json::from_str(&text).with_context(|| format!("parsing {}", path.display()))
    }
}

/// Places `figure` over the state in a fresh RPCS3 and reads it, into
/// `out/<label>/` (refused inside any work tree, never reused). What RPCS3
/// wrote under the home (the shot) is removed once copied out; on an error
/// the dir keeps whatever the run wrote, RPCS3's log included.
pub fn run_figure(inputs: &Inputs, figure: &SweepFigure, out: &Path) -> Result<FigureRecord> {
    let picked = figure
        .sky
        .as_ref()
        .with_context(|| format!("{} has no .sky in the pack", figure.name))?;
    let dir = rrc_dump::output::refuse_repo_output(&out.join(figure.label()))?;
    if dir.exists() {
        bail!("{} exists: a figure's dir is never reused", dir.display());
    }
    fs::create_dir_all(&dir).with_context(|| format!("creating {}", dir.display()))?;
    let started = Instant::now();
    let before = Host::now();
    let identity = Identity::of(inputs.exe, &inputs.home.config_dir())?.line();
    let source = inputs.pack.join(&picked.path);
    let sky_sha256 = sha256_file(&source)?;
    let state_sha256 = sha256_file(inputs.state)?;
    let outputs = Rpcs3Outputs::new(inputs.home.config_dir());
    let had = outputs.list(Kind::Screenshot);

    let driven = drive(inputs, figure, &source, &dir);

    for path in outputs.list(Kind::Screenshot).difference(&had) {
        let _ = fs::remove_file(path);
    }
    let log = inputs.home.cache_dir().join("RPCS3.log");
    let text = fs::read(&log).unwrap_or_default();
    let _ = fs::copy(&log, dir.join("RPCS3.log"));
    let d = driven?;
    if sha256_file(&source)? != sky_sha256 {
        bail!("{} changed during the run", source.display());
    }
    let record = FigureRecord {
        identity,
        figure: figure.clone(),
        sky_sha256,
        state: inputs
            .state
            .file_name()
            .map_or_else(String::new, |n| n.to_string_lossy().into_owned()),
        state_sha256,
        before,
        after: Host::now(),
        boot_secs: d.boot_secs,
        f0: d.f0,
        f0_hero: d.f0_hero,
        reconnects: d.reconnects,
        queries: d.queries,
        presses: d.presses,
        archive: d.archive,
        hero_at: d.hero_at,
        read_at: d.read_at,
        game_frame: d.game_frame,
        hero_is_load_file: d
            .hero
            .name
            .as_deref()
            .is_some_and(|n| n.eq_ignore_ascii_case(&figure.load_file)),
        hero: d.hero,
        tuning: d.tuning,
        tuning_error: d.tuning_error,
        attachments: d.attachments,
        attachments_error: d.attachments_error,
        shot: d.shot,
        silent: !log_lines(&text, SILENT).is_empty(),
        fatal: log_lines(&text, FATAL),
        wall_secs: started.elapsed().as_secs_f64(),
        shutdown: d.shutdown,
    };
    let path = rrc_dump::output::refuse_repo_output(&dir.join(RECORD_FILE))?;
    fs::write(
        &path,
        serde_json::to_string_pretty(&record).expect("a record serialises"),
    )?;
    Ok(record)
}

struct Driven {
    boot_secs: f64,
    f0: u64,
    f0_hero: Option<String>,
    reconnects: u32,
    queries: u32,
    presses: u32,
    archive: Option<Opened>,
    hero_at: u64,
    read_at: u64,
    game_frame: u64,
    hero: Actor,
    tuning: Option<Hero>,
    tuning_error: Option<String>,
    attachments: Option<HeroAttachments>,
    attachments_error: Option<String>,
    shot: EndShot,
    shutdown: String,
}

fn drive(inputs: &Inputs, figure: &SweepFigure, source: &Path, dir: &Path) -> Result<Driven> {
    let launched = Instant::now();
    let mut oracle = launch::boot(
        inputs.exe,
        inputs.home,
        &BootOptions {
            savestate: Some(inputs.state.to_path_buf()),
            borderless: true,
            ..BootOptions::default()
        },
    )?;
    let held = oracle.wait_paused(inputs.boot_wait)?;
    let boot_secs = launched.elapsed().as_secs_f64();
    let f0 = held.frames;
    let control = oracle.control();
    let mut log = LogTail::new(inputs.home.cache_dir().join("RPCS3.log"));
    // Everything up to F0, the restore's own descriptors among it.
    log.lines();
    let mut pine = pine_connect(&oracle.run_paths().pine_socket, Duration::from_secs(30))?;
    crate::rng::check_title(&mut pine)?;
    let f0_frame = actors::read_frame(&mut pine).context("the actors at F0")?;
    let f0_hero = f0_frame.hero().ok().cloned();
    let own = f0_hero
        .as_ref()
        .and_then(|h| h.name.as_deref())
        .is_some_and(|n| n.eq_ignore_ascii_case(&figure.load_file));

    control.driver().reconnect()?;
    let copy = oracle.run_paths().tmp.join("figure.sky");
    fs::copy(source, &copy).with_context(|| format!("copying {}", source.display()))?;
    control
        .driver()
        .load(SlotIndex::new(0).expect("slot 0"), &copy)?;
    control.resume()?;
    let resumed = Instant::now();

    // AE.22's held boots: the guest's USB thread, restored mid-session, takes
    // F0's RECONNECT with its stale device handle (sys_usbd_get_descriptor_size
    // and sys_usbd_transfer_data answer EINVAL) and goes quiet; a RECONNECT once
    // it runs brings the portal's reset, activate and the figure's queries.
    let mut reconnects = 0;
    let queries = loop {
        let mut seen = 0;
        control.driver().watch_events(QUERY_WATCH, |_, line| {
            if line.starts_with("PE cmd=query") {
                seen += 1;
            }
        })?;
        if seen > 0 {
            break seen;
        }
        if reconnects == MAX_RECONNECTS {
            bail!("the game never queried the portal after F0's RECONNECT and {reconnects} more");
        }
        control.driver().reconnect()?;
        reconnects += 1;
    };
    let queried_at = control.state()?.frames;

    let deadline = Instant::now() + HERO_WAIT;
    let mut archive = None;
    let (mut absent, mut presses) = (0, 0);
    let hero_at = loop {
        for line in log.lines() {
            let Some(path) = opened_path(&line) else {
                continue;
            };
            let stem = Path::new(path).file_stem().and_then(|s| s.to_str());
            if archive.is_none()
                && path.contains("/USRDIR/character/")
                && stem.is_some_and(|s| s.eq_ignore_ascii_case(&figure.load_file))
            {
                archive = Some(Opened {
                    path: path.to_owned(),
                    after_secs: resumed.elapsed().as_secs_f64(),
                });
            }
        }
        let now = control.state()?.frames;
        let paused = control.pause_at(now + POLL_AHEAD, STEP_WAIT)?;
        let frame = actors::read_frame(&mut pine)?;
        let ready = match (frame.hero(), &f0_hero) {
            (Ok(_), _) if own => paused.frames >= queried_at + OWN_WAIT,
            (Ok(h), Some(was)) => archive.is_some() && h.placement != was.placement,
            (Ok(_), None) => archive.is_some(),
            (Err(_), _) => false,
        };
        if ready {
            break paused.frames;
        }
        absent = if archive.is_some() && frame.hero().is_err() {
            absent + 1
        } else {
            0
        };
        if Instant::now() > deadline {
            bail!(
                "no new hero {HERO_WAIT:?} after the queries: archive {:?}, hero {:?}",
                archive.as_ref().map(|a| &a.path),
                frame.hero().ok().and_then(|h| h.name.clone())
            );
        }
        control.resume()?;
        if absent >= PROMPT_POLLS && presses < MAX_PRESSES {
            control.driver().press_button(PadButton::Cross, 120)?;
            presses += 1;
            absent = 0;
        }
        std::thread::sleep(Duration::from_millis(500));
    };

    let read = control.pause_at(hero_at + inputs.settle, STEP_WAIT)?;
    let mut frame = actors::read_frame(&mut pine).context("the actors at the read")?;
    frame.flip = Some(read.frames);
    let hero = frame.hero()?.clone();
    let (tuning, tuning_error) = match hero::read_paused(&mut pine, &frame) {
        Ok(h) => (Some(h), None),
        // The solid's walk is the likelier to fail; keep the rest.
        Err(e) => (hero::read(&mut pine, &frame).ok(), Some(format!("{e:#}"))),
    };
    let (attachments, attachments_error) = match attach::read(&mut pine, &frame) {
        Ok(a) => (Some(a), None),
        Err(e) => (None, Some(format!("{e:#}"))),
    };
    crate::rng::check_title(&mut pine)?;
    frame.write(&dir.join(ACTORS_FILE))?;
    let shot = control.screenshot(dir, STEP_WAIT)?;
    let named = dir.join(SHOT_FILE);
    fs::rename(&shot.path, &named)?;
    // An open PINE connection hangs RPCS3's stop (AF.8).
    drop(pine);
    let shutdown = format!("{:?}", oracle.shutdown()?);
    Ok(Driven {
        boot_secs,
        f0,
        f0_hero: f0_hero.and_then(|h| h.name),
        reconnects,
        queries,
        presses,
        archive,
        hero_at,
        read_at: read.frames,
        game_frame: u64::from(frame.game_frame),
        hero,
        tuning,
        tuning_error,
        attachments,
        attachments_error,
        shot: EndShot {
            file: SHOT_FILE.to_owned(),
            flip: shot.flip,
            pixels_sha256: shot.frame.pixels_sha256.clone(),
            lit_share: shot.frame.lit_share(),
        },
        shutdown,
    })
}

fn log_lines(log: &[u8], needle: &[u8]) -> Vec<String> {
    log.split(|&b| b == b'\n')
        .filter(|line| line.windows(needle.len()).any(|w| w == needle))
        .map(|line| String::from_utf8_lossy(line).trim().to_owned())
        .collect()
}

pub(super) fn sha256_file(path: &Path) -> Result<String> {
    let bytes = fs::read(path).with_context(|| format!("reading {}", path.display()))?;
    Ok(hex(&Sha256::digest(bytes)))
}
