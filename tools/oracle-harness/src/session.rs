//! `oracle session` (openspec AG.6): the original run through what the portal
//! does to it, unattended, from a cold boot in a home with no save (or a
//! state). A plan's steps press through the menus, put figures on the portal
//! and take them off over P1, push either pad's sticks and buttons, take
//! shots while it runs and read the actors at short pauses; between steps and
//! through every wait the driver reads `STATE` (P4's portal events with it)
//! and RPCS3's log, so every guest file the game opens is stamped with the
//! flip the last `STATE` read and the wall time since the session began. An
//! intro movie (`movies/<name>.mov`) opening is the detector design decision
//! 13 names, and the placed figure's own archive opening is the positive
//! control that the log caught the placement at all.
//!
//! Pads go in AHEAD of the counter while the game runs (O4 takes an entry
//! for any flip it hasn't reached), so a session pauses only for its reads:
//! the timings it records are the game's own, not a paused run's.
//!
//! Everything lands in one new dir outside every work tree: `record.json`
//! (the identity line, the hosts' loads before and after, every step with the
//! flips it ran between, the opens, the shots, the reads, the portal events
//! and the portal's copies), `run.txt` (the same as a timeline), each shot
//! and read, the figures as the game left them (`portal/`) and RPCS3's log.

use std::collections::BTreeSet;
use std::fs;
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail};
use guac_oracle::input::{Ds3Pad, PadState};
use serde::{Deserialize, Serialize};
use skylander_core::SlotIndex;
use skylander_rpcs3_control::PortalDriver;
use skylander_rpcs3_control::ipc::proto::EmuState;

use crate::actors::{self, ActorFrame};
use crate::control::{Control, PortalSeen};
use crate::divergence::{Host, pine_connect};
use crate::launch;
use crate::pine::PineClient;
use crate::rpcs3_log::{LogTail, opened_path};
use crate::verbs::{Refusal, Verb};

/// Flips ahead of the counter a pad entry goes in at first; doubled after
/// each `ERR past`, up to [`MAX_LEAD`].
pub const LEAD: u64 = 6;
pub const MAX_LEAD: u64 = 96;
/// Flips ahead of the counter a read's pause is set.
const READ_LEAD: u64 = 3;
/// How often a wait reads `STATE` and the log.
const POLL: Duration = Duration::from_millis(50);
/// The longest a single verb or shot is waited on.
const VERB_WAIT: Duration = Duration::from_secs(120);

/// Steps run in order, each to its end before the next starts; a step that
/// fails ends the session there.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SessionPlan {
    pub steps: Vec<Step>,
}

/// Every step but [`Step::Read`] runs with the game running.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Step {
    /// `port` (0 or 1) reads `pad` for `flips` flips, then rests; the step
    /// ends once the rest is read, or at once with `wait: false`, so the
    /// steps after it run while the pad is held. Scheduled ahead of the
    /// counter, no pause.
    Pad {
        #[serde(default)]
        port: u8,
        pad: PadState,
        flips: u64,
        #[serde(default = "yes")]
        wait: bool,
    },
    /// Until `STATE` reads `flips` past where it read at the step's start.
    Wait { flips: u64 },
    /// `secs` of wall time.
    Secs { secs: f64 },
    /// Until the game opens a file whose guest path holds `contains`, at most
    /// `secs`; past that the session stops (`fail: true`, the default) or
    /// goes on with the miss recorded.
    Opened {
        contains: String,
        secs: f64,
        #[serde(default = "yes")]
        fail: bool,
    },
    /// A `SCREENSHOT` while it runs, named by `label`.
    Shot { label: String },
    /// `count` shots back to back while it runs (RPCS3 names a shot to the
    /// second, so about 1.1 s apart), `label-00`, `label-01`, ...
    Shots { label: String, count: u32 },
    /// The actors read at a pause a few flips on, then resumed, into
    /// `actors-<label>.json`; a read that fails (no level yet) is recorded.
    Read { label: String },
    /// P1's `LOAD` of a fresh, writable copy of `figure` onto the portal
    /// (the emulator picks the slot); `@<n>` puts back this session's n-th
    /// load's copy as the game last wrote it.
    Load { figure: String },
    /// P1's `LOAD` of the file at `copy` as it stands, not copied: a copy an
    /// earlier session against the same run loaded, as the game last wrote it.
    Again { copy: String },
    /// P1's `CLEAR` of a slot.
    Clear { slot: u8 },
    /// P5's `RECONNECT`.
    Reconnect,
    /// A line in the record.
    Note { text: String },
}

fn yes() -> bool {
    true
}

/// A file label: letters, digits, '_', '-' and '.'.
fn label_ok(label: &str) -> bool {
    !label.is_empty()
        && label
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.'))
}

impl SessionPlan {
    pub fn from_ron(text: &str) -> Result<Self> {
        let plan: SessionPlan = ron::from_str(text).context("parsing the session plan")?;
        plan.check()?;
        Ok(plan)
    }

    pub fn to_ron(&self) -> String {
        ron::ser::to_string_pretty(self, ron::ser::PrettyConfig::default())
            .expect("a plan serializes")
    }

    /// The refusals a plan fails on before anything boots: a port past 1, a
    /// slot past 7, a pad held for no flips, a bad or repeated label, an
    /// `@<n>` naming a load that hasn't happened by then, a figure that
    /// isn't a file, an empty wait.
    pub fn check(&self) -> Result<()> {
        let mut labels = BTreeSet::new();
        let mut loads = 0usize;
        for (i, step) in self.steps.iter().enumerate() {
            let at = || format!("step {i} ({step:?})");
            match step {
                Step::Pad { port, flips, .. } => {
                    if *port > 1 {
                        bail!("{}: port {port} isn't 0 or 1", at());
                    }
                    if *flips == 0 {
                        bail!("{}: a pad held for no flips", at());
                    }
                }
                Step::Wait { flips } if *flips == 0 => bail!("{}: an empty wait", at()),
                Step::Secs { secs } | Step::Opened { secs, .. }
                    if !(secs.is_finite() && *secs > 0.0) =>
                {
                    bail!("{}: {secs} isn't a positive number of seconds", at())
                }
                Step::Opened { contains, .. } if contains.is_empty() => {
                    bail!("{}: an empty path fragment", at())
                }
                Step::Shot { label } | Step::Read { label } => {
                    if !label_ok(label) || !labels.insert(label.clone()) {
                        bail!("{}: label {label:?} is bad or used twice", at());
                    }
                }
                Step::Shots { label, count } => {
                    if *count == 0 || !label_ok(label) || !labels.insert(label.clone()) {
                        bail!(
                            "{}: label {label:?} is bad or used twice, or no shots",
                            at()
                        );
                    }
                }
                Step::Load { figure } => {
                    if let Some(n) = figure.strip_prefix('@') {
                        let n: usize = n
                            .parse()
                            .with_context(|| format!("{}: @<n> takes a load's number", at()))?;
                        if n == 0 || n > loads {
                            bail!("{}: no load {n} before it ({loads} so far)", at());
                        }
                    } else if !Path::new(figure).is_file() {
                        bail!("{}: no figure at {figure}", at());
                    }
                    loads += 1;
                }
                Step::Again { copy } if !Path::new(copy).is_file() => {
                    bail!("{}: no copy at {copy}", at())
                }
                Step::Clear { slot } if *slot > 7 => bail!("{}: slot {slot} isn't 0-7", at()),
                _ => {}
            }
        }
        Ok(())
    }
}

/// The two `PAD` lines a [`Step::Pad`] sends: `pad` from `base`, the rest at
/// `base + flips`.
pub fn pad_verbs(port: u8, pad: &PadState, base: u64, flips: u64) -> [Verb; 2] {
    [
        Verb::Pad {
            port,
            flip: base,
            pad: pad.ds3(),
        },
        Verb::Pad {
            port,
            flip: base + flips,
            pad: Ds3Pad::REST,
        },
    ]
}

/// What an opened guest path is to a session: an intro or cutscene movie, a
/// character's archive, a level's, or anything else.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum OpenKind {
    Movie,
    Character,
    Level,
    Other,
}

pub fn open_kind(path: &str) -> OpenKind {
    let lower = path.to_ascii_lowercase();
    if lower.contains("/usrdir/movies/") {
        OpenKind::Movie
    } else if lower.contains("/usrdir/character/") {
        OpenKind::Character
    } else if lower.contains("/usrdir/level/") {
        OpenKind::Level
    } else {
        OpenKind::Other
    }
}

/// RPCS3's own clock at the head of a log line (`·W 0:00:54.632944 {...}`),
/// in seconds since its start.
pub fn log_secs(line: &str) -> Option<f64> {
    let stamp = line.split_whitespace().nth(1)?;
    let mut parts = stamp.split(':');
    let (h, m, s) = (parts.next()?, parts.next()?, parts.next()?);
    if parts.next().is_some() {
        return None;
    }
    Some(h.parse::<f64>().ok()? * 3600.0 + m.parse::<f64>().ok()? * 60.0 + s.parse::<f64>().ok()?)
}

/// A `sys_fs_open` RPCS3 logged, placed on the run by the `STATE` read
/// before the driver saw it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Opened {
    pub path: String,
    pub kind: OpenKind,
    /// RPCS3's clock on the line.
    pub log_secs: Option<f64>,
    /// The flip the last `STATE` read before the line was seen.
    pub flip: u64,
    /// Milliseconds since the session began, when the driver saw the line.
    pub wall_ms: u64,
}

/// A step as it ran.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StepRun {
    pub index: usize,
    pub step: String,
    pub from_flip: u64,
    pub to_flip: u64,
    pub from_ms: u64,
    pub to_ms: u64,
    /// What it did: a pad's base flip, a load's slot and copy, a read's
    /// summary, a miss.
    pub detail: serde_json::Value,
}

/// A running shot: the flip `SCREENSHOT` named and when the driver had it.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionShot {
    pub label: String,
    pub flip: u64,
    pub wall_ms: u64,
    pub file: String,
    pub lit_share: f64,
    pub pixels_sha256: String,
}

/// `record.json`: everything [`run`] saw, written once the steps end.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionRecord {
    pub identity: String,
    pub plan: String,
    /// The state it booted, or none (a cold boot, or attached).
    pub state: Option<String>,
    pub attached: bool,
    pub before: Host,
    pub after: Host,
    pub steps: Vec<StepRun>,
    pub opens: Vec<Opened>,
    pub shots: Vec<SessionShot>,
    pub portal_events: Vec<PortalSeen>,
    /// Each load's source and its copy, by load number from 1.
    pub loads: Vec<(String, String)>,
    pub error: Option<String>,
    pub wall_secs: f64,
}

/// What a session runs against: a running oracle's P1 and PINE sockets, its
/// home's RPCS3 config dir and log, and the dir figure copies go into.
pub struct Target<'a> {
    pub p1_socket: &'a Path,
    pub pine_socket: &'a Path,
    pub config_dir: &'a Path,
    pub log: &'a Path,
    pub tmp: &'a Path,
}

struct Driver<'a> {
    control: Control,
    pine: Option<PineClient>,
    pine_socket: PathBuf,
    tail: LogTail,
    dir: PathBuf,
    tmp: PathBuf,
    began: Instant,
    /// The session's start in Unix milliseconds, in its copies' names.
    stamp: u128,
    last: EmuState,
    opens: Vec<Opened>,
    shots: Vec<SessionShot>,
    loads: Vec<(String, String)>,
    timeline: fs::File,
    _target: std::marker::PhantomData<&'a ()>,
}

impl Driver<'_> {
    fn ms(&self) -> u64 {
        self.began.elapsed().as_millis() as u64
    }

    fn line(&mut self, text: impl AsRef<str>) {
        let text = format!(
            "{:>9.3} s  flip {:>6}  {}",
            self.began.elapsed().as_secs_f64(),
            self.last.frames,
            text.as_ref()
        );
        eprintln!("{text}");
        let _ = writeln!(self.timeline, "{text}");
    }

    /// A frozen or stopped emulator ends the session here, at its first
    /// sight, rather than at the next step's timeout.
    fn poll(&mut self) -> Result<()> {
        if let Ok(state) = self.control.state() {
            if state.status == "frozen" || state.status == "stopped" {
                bail!("RPCS3 reports {} at frame {}", state.status, state.frames);
            }
            self.last = state;
        }
        for line in self.tail.lines() {
            let Some(path) = opened_path(&line) else {
                continue;
            };
            let opened = Opened {
                path: path.to_owned(),
                kind: open_kind(path),
                log_secs: log_secs(&line),
                flip: self.last.frames,
                wall_ms: self.ms(),
            };
            if opened.kind != OpenKind::Other {
                let text = format!("open {:?} {}", opened.kind, opened.path);
                self.line(text);
            }
            self.opens.push(opened);
        }
        Ok(())
    }

    fn wait_until(
        &mut self,
        timeout: Duration,
        mut done: impl FnMut(&Self) -> bool,
    ) -> Result<bool> {
        let deadline = Instant::now() + timeout;
        loop {
            self.poll()?;
            if done(self) {
                return Ok(true);
            }
            if Instant::now() > deadline {
                return Ok(false);
            }
            std::thread::sleep(POLL);
        }
    }

    fn wait_flips(&mut self, flips: u64) -> Result<()> {
        self.poll()?;
        let target = self.last.frames + flips;
        let timeout = Duration::from_secs(60 + flips / 20);
        if !self.wait_until(timeout, |d| d.last.frames >= target)? {
            bail!(
                "waited {timeout:?} for flip {target}: STATE {} frames={}",
                self.last.status,
                self.last.frames
            );
        }
        Ok(())
    }

    /// The pad's two lines ahead of the counter, a further lead after each
    /// `ERR past`; then until the rest's flip has gone by.
    fn pad(
        &mut self,
        port: u8,
        pad: &PadState,
        flips: u64,
        wait: bool,
    ) -> Result<serde_json::Value> {
        let mut lead = LEAD;
        let base = loop {
            self.poll()?;
            if self.last.status == "paused" {
                bail!("a pad step while the run is paused at {}", self.last.frames);
            }
            let base = self.last.frames + lead;
            let verbs = pad_verbs(port, pad, base, flips);
            let replies = self.control.verbs().send_all(&verbs)?;
            let past = replies.iter().any(|r| {
                r.as_ref()
                    .is_err_and(|e| e.refusal() == Some(Refusal::Past))
            });
            if !past {
                for (verb, reply) in verbs.iter().zip(replies) {
                    reply.with_context(|| format!("sending {}", verb.encode().trim_end()))?;
                }
                break base;
            }
            if lead >= MAX_LEAD {
                bail!("the counter outran a {lead}-flip lead");
            }
            lead *= 2;
        };
        let detail =
            serde_json::json!({ "port": port, "base": base, "rest": base + flips, "lead": lead });
        if !wait {
            return Ok(detail);
        }
        let end = base + flips + 1;
        let timeout = Duration::from_secs(60 + (end - self.last.frames) / 20);
        if !self.wait_until(timeout, |d| d.last.frames >= end)? {
            bail!("the pad's rest at {end} never came");
        }
        Ok(detail)
    }

    fn shot(&mut self, label: &str) -> Result<serde_json::Value> {
        let shot = self.control.screenshot(&self.dir, VERB_WAIT)?;
        let named = self.dir.join(format!("shot-{label}.png"));
        fs::rename(&shot.path, &named)?;
        let taken = SessionShot {
            label: label.to_owned(),
            flip: shot.flip,
            wall_ms: self.ms(),
            file: named
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default(),
            lit_share: shot.frame.lit_share(),
            pixels_sha256: shot.frame.pixels_sha256.clone(),
        };
        self.line(format!("shot {label} at flip {}", shot.flip));
        self.shots.push(taken.clone());
        self.poll()?;
        Ok(serde_json::to_value(taken)?)
    }

    fn pine(&mut self) -> Result<&mut PineClient> {
        if self.pine.is_none() {
            self.pine = Some(pine_connect(&self.pine_socket, Duration::from_secs(30))?);
        }
        Ok(self.pine.as_mut().expect("just connected"))
    }

    /// A pause a few flips on, the actors read, the run resumed whatever the
    /// read did.
    fn read(&mut self, label: &str) -> Result<serde_json::Value> {
        // A pause target the counter passes before O3 sees it is refused
        // (`ERR past`): a longer lead each time, as a pad's.
        let mut lead = READ_LEAD;
        let paused = loop {
            self.poll()?;
            match self.control.pause_at(self.last.frames + lead, VERB_WAIT) {
                Ok(p) => break p,
                // O3 refuses a target already passed, or drops one its RSX
                // thread first sees past (`Control::pause_at` says which).
                Err(e)
                    if lead < MAX_LEAD
                        && (e
                            .downcast_ref::<crate::verbs::ReplyError>()
                            .and_then(crate::verbs::ReplyError::refusal)
                            == Some(Refusal::Past)
                            || format!("{e:#}").contains("went by without a pause")) =>
                {
                    lead *= 2;
                }
                Err(e) => return Err(e),
            }
        };
        self.last = paused.clone();
        let read = (|| -> Result<ActorFrame> {
            let mut frame = actors::read_frame(self.pine()?)?;
            frame.flip = Some(paused.frames);
            Ok(frame)
        })();
        self.control.resume()?;
        let detail = match read {
            Ok(frame) => {
                frame.write(&self.dir.join(format!("actors-{label}.json")))?;
                let players: Vec<String> = frame.players().map(actors::Actor::summary).collect();
                let camera = frame.camera.as_ref().map(|c| {
                    format!(
                        "camera {:?} at ({:.3}, {:.3}, {:.3}) heading {:.2}",
                        c.name,
                        c.position[0],
                        c.position[1],
                        c.position[2],
                        c.heading()
                    )
                });
                let text = format!(
                    "read {label}: game frame {}, {} actors, players [{}], {}",
                    frame.game_frame,
                    frame.actors.len(),
                    players.join("; "),
                    camera.clone().unwrap_or_else(|| "no camera".into())
                );
                self.line(text);
                serde_json::json!({
                    "flip": paused.frames,
                    "game_frame": frame.game_frame,
                    "actors": frame.actors.len(),
                    "players": players,
                    "camera": camera,
                })
            }
            Err(e) => {
                self.line(format!("read {label} failed: {e:#}"));
                serde_json::json!({ "flip": paused.frames, "error": format!("{e:#}") })
            }
        };
        Ok(detail)
    }

    fn load(&mut self, figure: &str, as_is: bool) -> Result<serde_json::Value> {
        let copy = match figure.strip_prefix('@') {
            _ if as_is => PathBuf::from(figure),
            Some(n) => {
                let n: usize = n.parse().context("@<n>")?;
                let (_, copy) = self
                    .loads
                    .get(n - 1)
                    .with_context(|| format!("no load {n}"))?;
                PathBuf::from(copy)
            }
            // Named for the session as well as the load, so a later session
            // against the same run never writes over a copy still on the portal.
            None => launch::figure_copy(
                &self.tmp,
                &format!("figure-{}-{}.sky", self.stamp, self.loads.len() + 1),
                Path::new(figure),
            )?,
        };
        // P1's `LOAD` takes the first free slot whatever the hint says.
        self.control
            .driver()
            .load(SlotIndex::new(0).expect("slot 0"), &copy)?;
        self.loads
            .push((figure.to_owned(), copy.display().to_string()));
        let slots = self.control.driver().read_slots()?;
        let slots: Vec<String> = slots.iter().map(|s| format!("{s:?}")).collect();
        self.line(format!(
            "load {} ({figure}); slots {}",
            self.loads.len(),
            slots.join(", ")
        ));
        Ok(serde_json::json!({
            "load": self.loads.len(),
            "figure": figure,
            "copy": copy.display().to_string(),
            "slots": slots,
        }))
    }

    fn run_step(&mut self, step: &Step) -> Result<serde_json::Value> {
        Ok(match step {
            Step::Pad {
                port,
                pad,
                flips,
                wait,
            } => self.pad(*port, pad, *flips, *wait)?,
            Step::Wait { flips } => {
                self.wait_flips(*flips)?;
                serde_json::Value::Null
            }
            Step::Secs { secs } => {
                self.wait_until(Duration::from_secs_f64(*secs), |_| false)?;
                serde_json::Value::Null
            }
            Step::Opened {
                contains,
                secs,
                fail,
            } => {
                let seen = self.opens.len();
                let hit = self.wait_until(Duration::from_secs_f64(*secs), |d| {
                    d.opens[seen..]
                        .iter()
                        .any(|o| o.path.contains(contains.as_str()))
                })?;
                if !hit {
                    self.line(format!("no open of {contains:?} in {secs} s"));
                    if *fail {
                        bail!("no open of {contains:?} in {secs} s");
                    }
                }
                serde_json::json!({ "seen": hit })
            }
            Step::Shot { label } => self.shot(label)?,
            Step::Shots { label, count } => {
                let mut taken = Vec::new();
                for k in 0..*count {
                    taken.push(self.shot(&format!("{label}-{k:02}"))?);
                }
                serde_json::Value::Array(taken)
            }
            Step::Read { label } => self.read(label)?,
            Step::Load { figure } => self.load(figure, false)?,
            Step::Again { copy } => self.load(copy, true)?,
            Step::Clear { slot } => {
                let index = SlotIndex::new(*slot).map_err(|e| anyhow::anyhow!("{e:?}"))?;
                self.control.driver().clear(index)?;
                self.line(format!("clear slot {slot}"));
                serde_json::json!({ "slot": slot })
            }
            Step::Reconnect => {
                self.control.driver().reconnect()?;
                self.line("reconnect");
                serde_json::Value::Null
            }
            Step::Note { text } => {
                self.line(format!("note: {text}"));
                serde_json::Value::Null
            }
        })
    }
}

/// Runs `plan` against `target` into `dir` (which exists and is outside
/// every work tree). Returns the record and writes it, `record.json`, beside
/// `run.txt`; a step that fails ends the run there, its error in the record.
pub fn run(
    target: &Target,
    plan: &SessionPlan,
    identity: &str,
    state: Option<&Path>,
    attached: bool,
    dir: &Path,
) -> Result<SessionRecord> {
    plan.check()?;
    let began = Instant::now();
    let before = Host::now();
    let control = Control::new(target.p1_socket, target.config_dir)
        .with_log(target.log)
        .with_portal_events();
    let mut tail = LogTail::new(target.log.to_path_buf());
    if attached {
        // An attached session counts opens from its own start.
        let _ = tail.lines();
    }
    let last = control.state()?;
    let timeline = fs::File::create(dir.join("run.txt"))?;
    let mut d = Driver {
        control,
        pine: None,
        pine_socket: target.pine_socket.to_path_buf(),
        tail,
        dir: dir.to_path_buf(),
        tmp: target.tmp.to_path_buf(),
        began,
        stamp: std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_millis()),
        last,
        opens: Vec::new(),
        shots: Vec::new(),
        loads: Vec::new(),
        timeline,
        _target: std::marker::PhantomData,
    };
    d.line(identity);
    d.line(format!("load {:?}; {}", before.load_average, before.uptime));
    let mut steps = Vec::new();
    let mut error = None;
    for (index, step) in plan.steps.iter().enumerate() {
        let (from_flip, from_ms) = (d.last.frames, d.ms());
        d.line(format!("step {index}: {step:?}"));
        match d.run_step(step) {
            Ok(detail) => steps.push(StepRun {
                index,
                step: format!("{step:?}"),
                from_flip,
                to_flip: d.last.frames,
                from_ms,
                to_ms: d.ms(),
                detail,
            }),
            Err(e) => {
                d.line(format!("step {index} failed: {e:#}"));
                error = Some(format!("step {index}: {e:#}"));
                break;
            }
        }
    }
    let _ = d.poll();
    // A held PINE connection hangs a save and the shutdown (AF.8).
    d.pine = None;
    let after = Host::now();
    d.line(format!("load {:?}; {}", after.load_average, after.uptime));
    let record = SessionRecord {
        identity: identity.to_owned(),
        plan: plan.to_ron(),
        state: state.map(|s| s.display().to_string()),
        attached,
        before,
        after,
        steps,
        opens: d.opens,
        shots: d.shots,
        portal_events: d.control.portal_events(),
        loads: d.loads,
        error,
        wall_secs: began.elapsed().as_secs_f64(),
    };
    let path = rrc_dump::output::refuse_repo_output(&dir.join("record.json"))?;
    fs::write(&path, serde_json::to_string_pretty(&record)?)?;
    Ok(record)
}

#[cfg(test)]
mod tests {
    use super::*;
    use guac_oracle::input::{Button, Stick};

    fn plan(text: &str) -> Result<SessionPlan> {
        SessionPlan::from_ron(text)
    }

    #[test]
    fn a_plan_reads_its_steps_and_defaults() {
        let p = plan(
            r#"(steps: [
                Pad(pad: (buttons: [South]), flips: 6),
                Pad(port: 1, pad: (left: (x: 128, y: 0)), flips: 120, wait: false),
                Wait(flips: 60),
                Secs(secs: 1.5),
                Opened(contains: "/movies/", secs: 30.0),
                Opened(contains: "/character/", secs: 5.0, fail: false),
                Shot(label: "prompt"),
                Shots(label: "after", count: 3),
                Read(label: "r1"),
                Clear(slot: 1),
                Reconnect,
                Note(text: "hi"),
            ])"#,
        )
        .unwrap();
        assert_eq!(p.steps.len(), 12);
        match &p.steps[0] {
            Step::Pad {
                port,
                pad,
                flips,
                wait,
            } => {
                assert_eq!((*port, *flips, *wait), (0, 6, true));
                assert!(pad.buttons.contains(&Button::South));
                assert!(pad.left.is_centred());
            }
            other => panic!("{other:?}"),
        }
        match &p.steps[1] {
            Step::Pad {
                port, pad, wait, ..
            } => {
                assert_eq!((*port, *wait), (1, false));
                assert_eq!(pad.left, Stick { x: 128, y: 0 });
            }
            other => panic!("{other:?}"),
        }
        assert!(matches!(&p.steps[4], Step::Opened { fail: true, .. }));
        assert!(matches!(&p.steps[5], Step::Opened { fail: false, .. }));
        // It writes back as it reads.
        assert_eq!(SessionPlan::from_ron(&p.to_ron()).unwrap(), p);
    }

    #[test]
    fn a_plan_refuses_what_cant_run() {
        let bad = [
            "(steps: [Pad(port: 2, pad: (), flips: 6)])",
            "(steps: [Pad(pad: (), flips: 0)])",
            "(steps: [Wait(flips: 0)])",
            "(steps: [Secs(secs: 0.0)])",
            "(steps: [Opened(contains: \"\", secs: 1.0)])",
            "(steps: [Shot(label: \"a b\")])",
            "(steps: [Shot(label: \"a\"), Read(label: \"a\")])",
            "(steps: [Shots(label: \"a\", count: 0)])",
            "(steps: [Clear(slot: 8)])",
            "(steps: [Load(figure: \"@1\")])",
            "(steps: [Load(figure: \"/no/such/figure.sky\")])",
            "(steps: [Jump])",
        ];
        for text in bad {
            assert!(plan(text).is_err(), "{text} should be refused");
        }
    }

    #[test]
    fn a_put_back_names_an_earlier_load() {
        let tmp = tempfile::tempdir().unwrap();
        let fig = tmp.path().join("f.sky");
        fs::write(&fig, [0u8; 16]).unwrap();
        let text = format!(
            "(steps: [Load(figure: {:?}), Clear(slot: 0), Load(figure: \"@1\")])",
            fig.display().to_string()
        );
        plan(&text).unwrap();
        let again = format!(
            "(steps: [Again(copy: {:?}), Again(copy: \"/no/such/copy.sky\")])",
            fig.display().to_string()
        );
        let refused = plan(&again).unwrap_err();
        assert!(format!("{refused:#}").contains("step 1"), "{refused:#}");
        let ahead = format!(
            "(steps: [Load(figure: \"@2\"), Load(figure: {:?})])",
            fig.display().to_string()
        );
        assert!(plan(&ahead).is_err(), "@2 before the second load");
    }

    #[test]
    fn a_pad_step_is_its_state_then_the_rest() {
        let pad = PadState {
            buttons: [Button::South].into(),
            ..PadState::default()
        };
        let [on, off] = pad_verbs(1, &pad, 100, 6);
        assert_eq!(
            on,
            Verb::Pad {
                port: 1,
                flip: 100,
                pad: pad.ds3()
            }
        );
        assert_eq!(
            off,
            Verb::Pad {
                port: 1,
                flip: 106,
                pad: Ds3Pad::REST
            }
        );
        assert_eq!(on.encode().trim_end(), "PAD 1 100 00 40 128 128 128 128");
    }

    #[test]
    fn opens_sort_into_movies_characters_and_levels() {
        assert_eq!(
            open_kind("/dev_bdvd/PS3_GAME/USRDIR/movies/eruptor.mov"),
            OpenKind::Movie
        );
        assert_eq!(
            open_kind("/dev_bdvd/PS3_GAME/USRDIR/character/009_eruptor.arc"),
            OpenKind::Character
        );
        assert_eq!(
            open_kind("/dev_bdvd/PS3_GAME/USRDIR/level/level_213_tutotial.bld"),
            OpenKind::Level
        );
        assert_eq!(
            open_kind("/dev_hdd0/home/00000001/savedata/BLUS30968/E_SLOT_1"),
            OpenKind::Other
        );
    }

    #[test]
    fn a_log_lines_clock_reads_as_seconds() {
        let line = "\u{b7}W 0:01:51.125000 {PPU[0x1000025] Thread (load) [libfs: 0x00bf2090]} \
                    sys_fs: sys_fs_open(path=\u{201c}/x\u{201d}, flags=00, fd=*0x1)";
        let secs = log_secs(line).unwrap();
        assert!((secs - 111.125).abs() < 1e-9, "{secs}");
        assert_eq!(log_secs("no clock here"), None);
        assert_eq!(log_secs("\u{b7}W 1:2:3:4 {x}"), None);
    }
}
