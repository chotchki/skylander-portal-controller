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

use super::census::{self, Census, Disc, RouteRecord, Wake};
use super::{Knobs, Outcome, RoutePlan, Session, Trace, drive};
use crate::actors::{self, ActorFrame};
use crate::chains::{self, GiantsChains};
use crate::control::Control;
use crate::divergence::{
    self, EndState, FATAL, Host, SILENT, STEP_WAIT, WatchRun, file_name, log_lines, read_watch,
    save_retrying, saved_in, sha256_file,
};
use crate::fight::{self, FightWatch, Rows};
use crate::home::OracleHome;
use crate::identity::Identity;
use crate::launch::{self, BootOptions};
use crate::outputs::{Kind, Rpcs3Outputs};
use crate::pine::PineClient;
use crate::rng::{self, Peek};
use crate::watch::{GiantsWatch, WatchFile};

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
    /// Level 213's placements off the dump (AH.8): with them and the rows a
    /// read takes a census too ([`census`]), and a watch carries the route's
    /// placements' wake state per frame ([`census::WATCHED`]).
    pub disc: Option<&'a Disc>,
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
    /// A census was taken too (`census-<label>.json`), in this many ms.
    #[serde(default)]
    pub census_ms: Option<f64>,
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
    disc: Option<&'a Disc>,
    whole: Vec<String>,
    shots: Vec<ShotTaken>,
    reads: Vec<ReadTaken>,
    censuses: Vec<(String, Census)>,
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
        // Appended, not loaded: the paused flip's reads come after the resume,
        // and they're the last run's.
        self.control.append_script_at(script, at + 1)?;
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
        let census_ms = match (self.rows, self.disc) {
            (Some(_), Some(disc)) => {
                let t = Instant::now();
                let c = census::read_paused(&mut self.pine, frame, disc, &self.whole)
                    .with_context(|| format!("the census at {flip}"))?;
                let path = rrc_dump::output::refuse_repo_output(
                    &self.dir.join(format!("census-{label}.json")),
                )?;
                fs::write(&path, serde_json::to_string_pretty(&c)?)?;
                self.censuses.push((label.to_owned(), c));
                Some(t.elapsed().as_secs_f64() * 1e3)
            }
            _ => None,
        };
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
            census_ms,
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
    // The route's placements per frame need the fight's cells to ride on
    // (AH.4's watch) and the disc to find them.
    let route_watch = match (inputs.watch, inputs.rows, inputs.disc) {
        (true, Some(rows), Some(disc)) => Some(route_watch(&mut pine, rows, disc)?),
        _ => None,
    };
    let watched = if inputs.watch {
        let mut watch = GiantsWatch::resolve(&mut pine).context("the watch list at F0")?;
        if let Some(fight) = &route_watch {
            watch = watch
                .with_fight(fight.clone())
                .context("the route's placements on the watch list")?;
        }
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
        disc: inputs.disc,
        whole: inputs.disc.map(census::whole_names).unwrap_or_default(),
        shots: Vec::new(),
        reads: Vec::new(),
        censuses: Vec::new(),
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
        censuses,
        ..
    } = live;
    let watch = match &watched {
        Some((w, c)) => Some(
            read_watch(&control, w, c.as_ref(), &mut pine, identity, dir)
                .context("reading the watch")?,
        ),
        None => None,
    };
    // The route's record: its steps, what fired per frame (named while PINE
    // is still open), what changed between reads, the scripts that started.
    if inputs.disc.is_some() {
        let fired = match (&watched, &route_watch) {
            (Some(_), Some(_)) => {
                let (frames, fired) = route_frames(&dir.join(WatchFile::FILE), &mut pine)
                    .context("the route's frames")?;
                let path = rrc_dump::output::refuse_repo_output(&dir.join(ROUTE_FRAMES_FILE))?;
                fs::write(&path, serde_json::to_string(&frames)?)?;
                fired
            }
            _ => Vec::new(),
        };
        let record = route_record(identity, plan, &trace, &censuses, &reads, fired);
        record.write(dir)?;
    }
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

/// The route's placements' wake state per frame, beside the record
/// (`route-frames.json`).
pub const ROUTE_FRAMES_FILE: &str = "route-frames.json";

/// One record of a route's watch: the game frame, the hero's pivot, the
/// fight's cells and each watched placement's wake state
/// ([`census::WATCHED`]'s order).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FrameLine {
    pub flip: u64,
    pub read: u32,
    pub game_frame: u32,
    pub pivot: [f32; 3],
    pub health: crate::script::Number,
    pub experience: u32,
    pub gold: u32,
    pub loot: crate::script::Number,
    pub wakes: Vec<Wake>,
}

/// The fight's cells and the route's placements on the watch list, from the
/// pause F0 is read at: refused when a watched placement isn't listed once.
fn route_watch(pine: &mut PineClient, rows: &Rows, disc: &Disc) -> Result<FightWatch> {
    let frame = actors::read_frame(pine).context("the actors for the route's watch")?;
    let hero = frame.hero()?;
    let state = fight::read_paused(pine, hero.placement, rows)?;
    let offsets = disc.offsets();
    let base = crate::level::section_base(&frame, &offsets)?;
    let named = census::WATCHED
        .iter()
        .map(|&n| {
            let off = offsets
                .get(n)
                .with_context(|| format!("`{n}` isn't a placement the disc lists once"))?;
            Ok((n, base.base + off))
        })
        .collect::<Result<Vec<_>>>()?;
    let triggers = fight::triggers(pine, &named)?;
    Ok(FightWatch::of(&state, rows)?.with_triggers(&triggers))
}

/// Every record of the ring at `path` as a [`FrameLine`], and what fired
/// record by record, the behaviours named through PINE at the end pause.
fn route_frames(
    path: &Path,
    pine: &mut PineClient,
) -> Result<(Vec<FrameLine>, Vec<census::Fired>)> {
    let file = WatchFile::load(path)?;
    let fight = file
        .watch
        .fight
        .as_ref()
        .context("the watch carries no fight cells")?;
    let names: Vec<String> = fight.triggers.iter().map(|(n, _)| n.clone()).collect();
    let recording = file.recording()?;
    let mut lines = Vec::with_capacity(file.records.len());
    for (raw, frame) in file.records.iter().zip(recording.frames()) {
        let record = raw.record()?;
        if (record.flip, record.read) != (frame.flip, u64::from(frame.read)) {
            bail!(
                "the recording's frame at flip {} isn't the record at {}",
                frame.flip,
                record.flip
            );
        }
        let f = file
            .watch
            .fight(&record)
            .with_context(|| format!("the fight's cells at flip {} didn't read", record.flip))?;
        lines.push(FrameLine {
            flip: record.flip,
            read: frame.read,
            game_frame: frame.game_frame,
            pivot: frame.hero.position,
            health: f.health,
            experience: f.experience,
            gold: f.gold,
            loot: f.loot,
            wakes: f
                .triggers
                .iter()
                .map(|t| Wake {
                    activity_state: t.activity_state,
                    behaviour: t.behaviour,
                    active_seconds: t.active_seconds,
                })
                .collect(),
        });
    }
    let pointers: std::collections::BTreeSet<u32> = lines
        .iter()
        .flat_map(|l| l.wakes.iter().map(|w| w.behaviour))
        .filter(|&p| p != 0)
        .collect();
    crate::rng::check_title(pine)?;
    let names_of = census::behaviour_names(pine, &pointers)?;
    let wakes: Vec<(u64, u32, Vec<Wake>)> = lines
        .iter()
        .map(|l| (l.flip, l.game_frame, l.wakes.clone()))
        .collect();
    let fired = census::fired(&names, &wakes, &names_of)?;
    Ok((lines, fired))
}

/// A drive's [`RouteRecord`] out of its trace, its censuses in read order,
/// its reads and what fired per frame.
fn route_record(
    identity: &str,
    plan: &RoutePlan,
    trace: &Trace,
    censuses: &[(String, Census)],
    reads: &[ReadTaken],
    fired: Vec<census::Fired>,
) -> RouteRecord {
    let changes = censuses
        .windows(2)
        .map(|w| {
            (
                w[0].0.clone(),
                w[1].0.clone(),
                w[1].1.changes_since(&w[0].1),
            )
        })
        .collect();
    let scripts = match (censuses.first(), censuses.last()) {
        (Some((_, first)), Some((_, last))) => RouteRecord::scripts_between(first, last),
        _ => Default::default(),
    };
    let anchor = reads.last().map(|r| {
        let look = trace.looks.iter().rev().find(|l| l.seen.flip == r.flip);
        let fired = censuses
            .iter()
            .find(|(l, _)| *l == r.label)
            .map(|(_, c)| {
                c.triggers
                    .iter()
                    .filter(|t| t.fired == Some(true))
                    .map(|t| t.name.clone())
                    .collect()
            })
            .unwrap_or_default();
        let mut files = vec![
            format!("actors-{}.json", r.label),
            format!("peek-{}.json", r.label),
        ];
        if r.fight {
            files.push(format!("fight-{}.json", r.label));
        }
        if r.census_ms.is_some() {
            files.push(format!("census-{}.json", r.label));
        }
        census::AnchorReads {
            label: r.label.clone(),
            flip: r.flip,
            game_frame: r.game_frame,
            pivot: look.map_or([0.0; 3], |l| l.seen.pivot),
            facing: look.map_or(0.0, |l| l.seen.facing),
            files,
            seed: r.seed,
            drawn: r.drawn,
            fired,
        }
    });
    RouteRecord {
        identity: identity.to_owned(),
        route: plan.to_ron(),
        steps: RouteRecord::steps(&trace.looks, plan),
        fired,
        changes,
        scripts,
        anchor,
    }
}

#[cfg(test)]
mod tests {
    use super::super::{Look, Point, Seen, Step};
    use super::*;
    use crate::fight::Trigger;
    use crate::level::SectionBase;

    fn trigger(name: &str, fired: Option<bool>) -> Trigger {
        Trigger {
            name: name.to_owned(),
            placement: 0x3000_0000,
            activity_state: 1,
            started: false,
            active_seconds: 0.0,
            range: 20.0,
            fired,
        }
    }

    fn census(flip: u64, triggers: Vec<Trigger>) -> Census {
        Census {
            flip,
            game_frame: 5_960 + flip as u32,
            base: SectionBase {
                base: 0x3000_0000,
                votes: 3,
                dissent: 0,
            },
            briefs: Vec::new(),
            triggers,
            whole: Vec::new(),
        }
    }

    fn seen(flip: u64, x: f64) -> Seen {
        Seen {
            flip,
            game_frame: 5_960 + flip as u32,
            pivot: [x, 43.4, -93.4],
            facing: 90.0,
            velocity: [0.0; 3],
            view: 15.0,
            eye: [0.0; 3],
        }
    }

    fn read(label: &str, flip: u64) -> ReadTaken {
        ReadTaken {
            label: label.to_owned(),
            flip,
            game_frame: 5_960 + flip as u32,
            hero: "009_Eruptor".to_owned(),
            seed: Some(10_554),
            drawn: Some(133_619),
            fight: true,
            census_ms: Some(40.0),
        }
    }

    #[test]
    fn the_anchors_reads_name_the_range_triggers_fired_there_and_where_he_stood() {
        let plan = RoutePlan {
            steps: vec![
                Step::Via {
                    to: Point { x: 20.0, z: -93.4 },
                    radius: 1.0,
                },
                Step::Read {
                    label: "anchor".into(),
                },
            ],
            save: true,
        };
        let look = |flip, x, step, did: &str| Look {
            seen: seen(flip, x),
            step,
            did: did.to_owned(),
        };
        let trace = Trace {
            f0: 2,
            looks: vec![
                look(2, 10.0, 0, "walk"),
                look(10, 15.0, 0, "walk"),
                look(18, 19.5, 1, "read anchor"),
                look(18, 19.5, 2, "done"),
            ],
            input: None,
            input_ron: String::new(),
            flips: 16,
            outcome: Outcome::Done,
        };
        // Fired at the anchor: the range trigger that has been active, not
        // one asleep and not one without a range (awake from the level's
        // start, decided by its controller).
        let censuses = vec![
            (
                "f0".to_owned(),
                census(
                    2,
                    vec![
                        trigger("Setup_001_Trigger", Some(false)),
                        trigger("Setup_003_Trigger(1)", None),
                    ],
                ),
            ),
            (
                "anchor".to_owned(),
                census(
                    18,
                    vec![
                        trigger("Setup_001_Trigger", Some(true)),
                        trigger("Setup_003_Trigger(1)", None),
                        trigger("Activate_Norticus and Chompies", Some(false)),
                    ],
                ),
            ),
        ];
        let reads = vec![read("f0", 2), read("anchor", 18)];
        let r = route_record("oracle", &plan, &trace, &censuses, &reads, Vec::new());
        let a = r.anchor.as_ref().unwrap();
        assert_eq!(a.fired, vec!["Setup_001_Trigger".to_owned()]);
        assert_eq!(
            (a.label.as_str(), a.flip, a.game_frame, a.pivot[0]),
            ("anchor", 18, 5_978, 19.5)
        );
        assert_eq!(
            a.files,
            [
                "actors-anchor.json",
                "peek-anchor.json",
                "fight-anchor.json",
                "census-anchor.json"
            ]
        );
        assert_eq!((a.seed, a.drawn), (Some(10_554), Some(133_619)));
        assert_eq!(r.changes.len(), 1);
        assert_eq!(
            (r.changes[0].0.as_str(), r.changes[0].1.as_str()),
            ("f0", "anchor")
        );
        assert_eq!(r.steps.len(), 3);
    }
}
