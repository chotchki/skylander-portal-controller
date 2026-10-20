//! `oracle goto`'s live half (openspec AH.7): a fresh RPCS3 boots a gameplay
//! state to Start Paused's pause (F0), gets P5's `RECONNECT` and a fresh copy
//! of the figure, F0 is read (the actors and the generator), O5's watch (and
//! O6's chains) are armed if asked, and the route is driven from there
//! ([`super::drive`]), each pause read through PINE and each order loaded as
//! O4's `PAD` lines with a `PAUSE_AT` after. At the end the watch is read, PINE
//! closed, and, when the plan asks and the drive got there, the state saved.
//!
//! Everything lands in one new dir outside every work tree: `record.json`
//! (the identity line, the hosts' loads and `uptime` before and after, the
//! hashes, the outcome, the shots, the reads, the watch, the end state),
//! `route.json` (every pause and what was done there), `input.ron` (what port
//! 0 held from F0 + 1, AF.7's format), each read's actors, generator and fight
//! state, each shot, the ring and the end state, and RPCS3's log.

use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail};
use guac_oracle::input::InputScript;
use serde::{Deserialize, Serialize};
use skylander_core::SlotIndex;
use skylander_rpcs3_control::PortalDriver;

use super::{Knobs, Outcome, RoutePlan, Session, Trace, drive};
use crate::actors::{self, ActorFrame};
use crate::chains::{self, GiantsChains};
use crate::control::Control;
use crate::divergence::{
    self, EndState, FATAL, Host, SILENT, STEP_WAIT, WatchRun, file_name, log_lines, read_watch,
    save_retrying, saved_in, sha256_file,
};
use crate::fight::{self, Rows};
use crate::home::OracleHome;
use crate::identity::Identity;
use crate::launch::{self, BootOptions};
use crate::outputs::{Kind, Rpcs3Outputs};
use crate::pine::PineClient;
use crate::rng::{self, Peek};
use crate::watch::GiantsWatch;

/// Everything a drive needs: the oracle build and its home, the state it
/// boots, the figure, the rows a read decodes the figure with, and the
/// recorders armed at F0.
pub struct Inputs<'a> {
    pub exe: &'a Path,
    pub home: &'a OracleHome,
    /// The gameplay savestate it boots.
    pub state: &'a Path,
    /// The `.sky` a fresh copy of goes on the portal at F0.
    pub figure: &'a Path,
    pub boot_wait: Duration,
    /// AH.2's rows off the dump: with them a read takes the fight's state
    /// (AH.4) too.
    pub rows: Option<&'a Rows>,
    /// O5's watch on Giants' list armed at F0, and O6's chains with it.
    pub watch: bool,
    pub chains: bool,
    /// The ring's records.
    pub ring: u32,
}

/// A `SCREENSHOT` by its plan label, with the flip it shows, the file it was
/// copied to and its pixels' hash.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ShotTaken {
    pub label: String,
    pub flip: u64,
    pub file: String,
    pub pixels_sha256: String,
}

/// A read at a pause: where it went and the numbers it's taken for.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ReadTaken {
    pub label: String,
    pub flip: u64,
    pub game_frame: u32,
    pub hero: String,
    /// The generator: draws since its `sgenrand` seeding, if found.
    pub seed: Option<u32>,
    pub drawn: Option<u64>,
    /// The fight's state was read too (`fight-<label>.json`).
    pub fight: bool,
}

/// One drive's record (`record.json`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GotoRecord {
    pub identity: String,
    pub route: String,
    pub state: String,
    pub state_sha256: String,
    pub figure_sha256: String,
    pub before: Host,
    pub after: Host,
    pub boot_secs: f64,
    pub f0: u64,
    pub outcome: Outcome,
    /// Flips from F0 to the last pause, and the pauses.
    pub flips: u64,
    pub looks: usize,
    pub shots: Vec<ShotTaken>,
    pub reads: Vec<ReadTaken>,
    pub watch: Option<WatchRun>,
    pub saved: Option<EndState>,
    pub save_refusals: Vec<String>,
    pub save_error: Option<String>,
    pub silent: bool,
    pub fatal: Vec<String>,
    pub wall_secs: f64,
    pub shutdown: String,
}

/// The oracle as a [`Session`].
pub struct Live<'a> {
    control: Control,
    pine: PineClient,
    dir: PathBuf,
    rows: Option<&'a Rows>,
    shots: Vec<ShotTaken>,
    reads: Vec<ReadTaken>,
}

impl Live<'_> {
    fn paused(&self) -> Result<u64> {
        let s = self.control.state()?;
        if s.status != "paused" {
            bail!(
                "the run isn't paused: STATE {} frames={}",
                s.status,
                s.frames
            );
        }
        Ok(s.frames)
    }
}

impl Session for Live<'_> {
    fn look(&mut self) -> Result<(u64, ActorFrame)> {
        let flip = self.paused()?;
        let mut frame =
            actors::read_frame(&mut self.pine).with_context(|| format!("the actors at {flip}"))?;
        frame.flip = Some(flip);
        Ok((flip, frame))
    }

    fn run(&mut self, script: &InputScript, flips: u64) -> Result<()> {
        let at = self.paused()?;
        self.control.load_script_at(script, at + 1)?;
        // 20 fps, a third of the game's, and a minute.
        let wait = Duration::from_secs(60 + flips / 20);
        self.control
            .pause_at(at + flips, wait)
            .with_context(|| format!("pausing {flips} flips on from {at}"))?;
        Ok(())
    }

    fn shot(&mut self, label: &str) -> Result<()> {
        let shot = self.control.screenshot(&self.dir, STEP_WAIT)?;
        let named = self.dir.join(format!("shot-{label}.png"));
        fs::rename(&shot.path, &named)?;
        self.shots.push(ShotTaken {
            label: label.to_owned(),
            flip: shot.flip,
            file: file_name(&named),
            pixels_sha256: shot.frame.pixels_sha256.clone(),
        });
        Ok(())
    }

    fn read(&mut self, label: &str, frame: &ActorFrame) -> Result<()> {
        let flip = self.paused()?;
        frame.write(&self.dir.join(format!("actors-{label}.json")))?;
        let mut peek = Peek::read(&mut self.pine).with_context(|| format!("the RNG at {flip}"))?;
        peek.flip = Some(flip);
        peek.write(&self.dir.join(format!("peek-{label}.json")))?;
        let origin = peek.origin(rng::ORIGIN_TWISTS);
        let hero = frame.hero()?;
        let fight = match self.rows {
            Some(rows) => {
                let state = fight::read_paused(&mut self.pine, hero.placement, rows)
                    .with_context(|| format!("the fight's state at {flip}"))?;
                let path = rrc_dump::output::refuse_repo_output(
                    &self.dir.join(format!("fight-{label}.json")),
                )?;
                fs::write(&path, serde_json::to_string_pretty(&state)?)?;
                true
            }
            None => false,
        };
        self.reads.push(ReadTaken {
            label: label.to_owned(),
            flip,
            game_frame: frame.game_frame,
            hero: hero.summary(),
            seed: origin.map(|o| o.seed),
            drawn: origin.map(|o| o.draws),
            fight,
        });
        Ok(())
    }
}

/// Drives `plan` on a fresh RPCS3 into `out` (new, outside every work tree)
/// and writes its record there. What RPCS3 wrote under the home (shots, the
/// end state) is removed once copied out.
pub fn run(inputs: &Inputs, plan: &RoutePlan, knobs: Knobs, out: &Path) -> Result<GotoRecord> {
    plan.check()?;
    let dir = rrc_dump::output::refuse_repo_output(out)?;
    if dir.exists() {
        bail!("{} exists: a drive's dir is never reused", dir.display());
    }
    fs::create_dir_all(&dir).with_context(|| format!("creating {}", dir.display()))?;
    fs::write(dir.join("route.ron"), plan.to_ron())?;
    let started = Instant::now();
    let before = Host::now();
    let identity = Identity::of(inputs.exe, &inputs.home.config_dir())?.line();
    let (state_sha256, figure_sha256) = (sha256_file(inputs.state)?, sha256_file(inputs.figure)?);
    let outputs = Rpcs3Outputs::new(inputs.home.config_dir());
    let kinds = [Kind::Screenshot, Kind::SaveState];
    let had = kinds.map(|k| outputs.list(k));

    let driven = drive_live(inputs, plan, knobs, &dir, &identity);

    for (kind, had) in kinds.into_iter().zip(&had) {
        for path in outputs.list(kind).difference(had) {
            let _ = fs::remove_file(path);
        }
    }
    let log = inputs.home.cache_dir().join("RPCS3.log");
    let text = fs::read(&log).unwrap_or_default();
    let silent = !log_lines(&text, SILENT).is_empty();
    let fatal = log_lines(&text, FATAL);
    let _ = fs::copy(&log, dir.join("RPCS3.log"));
    let d = driven?;
    let record = GotoRecord {
        identity,
        route: plan.to_ron(),
        state: file_name(inputs.state),
        state_sha256,
        figure_sha256,
        before,
        after: Host::now(),
        boot_secs: d.boot_secs,
        f0: d.trace.f0,
        outcome: d.trace.outcome.clone(),
        flips: d.trace.flips,
        looks: d.trace.looks.len(),
        shots: d.shots,
        reads: d.reads,
        watch: d.watch,
        saved: d.saved,
        save_refusals: d.save_refusals,
        save_error: d.save_error,
        silent,
        fatal,
        wall_secs: started.elapsed().as_secs_f64(),
        shutdown: d.shutdown,
    };
    let path = rrc_dump::output::refuse_repo_output(&dir.join("record.json"))?;
    fs::write(&path, serde_json::to_string_pretty(&record)?)?;
    Ok(record)
}

struct Driven {
    boot_secs: f64,
    trace: Trace,
    shots: Vec<ShotTaken>,
    reads: Vec<ReadTaken>,
    watch: Option<WatchRun>,
    saved: Option<EndState>,
    save_refusals: Vec<String>,
    save_error: Option<String>,
    shutdown: String,
}

fn drive_live(
    inputs: &Inputs,
    plan: &RoutePlan,
    knobs: Knobs,
    dir: &Path,
    identity: &str,
) -> Result<Driven> {
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
    let control = oracle.control();
    control.driver().reconnect()?;
    let copy = oracle.run_paths().tmp.join("figure.sky");
    fs::copy(inputs.figure, &copy)
        .with_context(|| format!("copying {}", inputs.figure.display()))?;
    control
        .driver()
        .load(SlotIndex::new(0).expect("slot 0"), &copy)?;
    let mut pine =
        divergence::pine_connect(&oracle.run_paths().pine_socket, Duration::from_secs(30))?;
    rng::check_title(&mut pine)?;
    if control.state()?.frames != held.frames {
        bail!("the run moved off Start Paused's pause before F0 was read");
    }
    let watched = if inputs.watch {
        let watch = GiantsWatch::resolve(&mut pine).context("the watch list at F0")?;
        let chains = if inputs.chains {
            let chains = GiantsChains::resolve(&mut pine, &watch, &chains::fight_kinds())
                .context("Giants' chains at F0")?;
            control.chains_arm(&chains.chains)?;
            Some(chains)
        } else {
            None
        };
        let armed = chains
            .as_ref()
            .map(|c| c.chains.clone())
            .unwrap_or_default();
        control.watch_arm_chained(&watch.list, &armed, watch.port, inputs.ring)?;
        Some((watch, chains))
    } else {
        None
    };
    let mut live = Live {
        control,
        pine,
        dir: dir.to_path_buf(),
        rows: inputs.rows,
        shots: Vec::new(),
        reads: Vec::new(),
    };
    // F0 is read before anything runs, as every replay's is.
    let (_, f0_frame) = live.look()?;
    live.read("f0", &f0_frame)?;
    let trace = drive(&mut live, plan, knobs)?;
    let path = rrc_dump::output::refuse_repo_output(&dir.join("route.json"))?;
    fs::write(&path, serde_json::to_string_pretty(&trace)?)?;
    fs::write(dir.join("input.ron"), &trace.input_ron)?;
    let Live {
        control,
        mut pine,
        shots,
        reads,
        ..
    } = live;
    let watch = match &watched {
        Some((w, c)) => Some(
            read_watch(&control, w, c.as_ref(), &mut pine, identity, dir)
                .context("reading the watch")?,
        ),
        None => None,
    };
    // A held PINE connection hangs a save and the shutdown (AF.8).
    drop(pine);
    let (saved, save_refusals, save_error) = if plan.save && trace.outcome == Outcome::Done {
        let tried = save_retrying(&control, dir);
        let written = match (&tried.saved, tried.asked_at) {
            (Some(saved), _) => Some((saved.path.clone(), saved.saved_at.frames)),
            (None, Some(asked_at)) => saved_in(dir).map(|p| (p, asked_at)),
            (None, None) => None,
        };
        let saved = match written {
            Some((path, saved_at)) => Some(EndState {
                file: file_name(&path),
                saved_at,
                bytes: fs::metadata(&path)?.len(),
                sha256: sha256_file(&path)?,
            }),
            None => None,
        };
        (saved, tried.refusals, tried.error)
    } else {
        (None, Vec::new(), None)
    };
    let shutdown = format!("{:?}", oracle.shutdown()?);
    Ok(Driven {
        boot_secs,
        trace,
        shots,
        reads,
        watch,
        saved,
        save_refusals,
        save_error,
        shutdown,
    })
}
