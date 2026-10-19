//! The divergence measurement (openspec AF.11, design decision 7): how far the
//! oracle drifts from itself. Every replay boots one gameplay savestate in a
//! fresh RPCS3 process, holds at Start Paused's pause (F0), gets P5's
//! `RECONNECT`, a fresh copy of the figure on the portal (the game writes to
//! it, so a shared file would carry XP from one replay into the next) and an
//! input script with its offset 0 on F0 + 1, and is observed at the same
//! offsets from F0 as every other replay:
//!
//! - set A only at the end flip, F0 + [`END`], then a `SCREENSHOT` and a
//!   `SAVESTATE` for the region-by-region memory diff;
//! - set B every [`EVERY`] flips up to the end, then the same `SCREENSHOT`.
//!   Set A against set B at the end flip says whether pausing to observe moves
//!   the run.
//!
//! An observation, read through PINE at a `PAUSE_AT` pause: the hero's and the
//! camera's placement and every game actor ([`crate::actors`]), and the RNG
//! ([`crate::rng`]). F0 is observed in both sets, inside the pause every replay
//! takes anyway.
//!
//! [`run_replay`] runs one replay and writes its [`ReplayRecord`] (plus the
//! actor frames, peeks, the end shot and the end state, all guest data, so
//! only outside a work tree); [`Report`] turns the records of one output dir
//! into spreads per checkpoint, usable horizons, RNG distances, FLIP between
//! end shots, set A against set B and how the spreads track the host's load.
//! `oracle divergence run|report` drives both; AF.12's leave-one-out and
//! perturbation runs reuse them.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use guac_oracle::input::InputScript;
use serde::{Deserialize, Serialize};

use crate::actors::Counts;

mod report;
mod run;

pub use report::{
    AcrossObservable, AcrossSets, Band, FlipSummary, Group, LoadLink, Observable, ReplayEnd,
    Report, Rng, RngCheckpoint, World, WorldCheckpoint, spearman,
};
pub use run::{BOOT_WAIT, Inputs, pine_connect, run_replay};

/// The end flip's offset from F0: 30 s at 60 fps (design decision 7).
pub const END: u64 = 1_800;
/// Set B's checkpoint spacing: one pause a second at 60 fps, 30 over the run.
pub const EVERY: u64 = 60;
/// Flips on from a `SAVESTATE` RPCS3 turned down before it's tried again
/// (AF.7's review: an SPU asleep in an unsavable call can't be locked).
pub const SAVE_RETRY_FLIPS: u64 = 30;
/// Tries at the end state before the replay goes on without it.
pub const SAVE_TRIES: u32 = 3;

/// Both pads at rest from F0 + 1 on, so the host's input can't steer a replay.
pub const IDLE: &str = "(steps: [(at: 0, pads: {0: (), 1: ()})])";

/// AF.9's walk from the clearing, a minute in: at rest for 60 flips, the left
/// stick forward for 90 (the move follows the camera, 15 deg), left for 90
/// (the turn: 105 deg, which takes Eruptor into the gate's corner about 30
/// flips in, where AF.9's runs parted), at rest by the rock below the bridge,
/// then a jump (South) and an attack (West) pressed for 6 flips each, then at
/// rest to the end. Port 1 rests throughout.
pub const WALK: &str = "(steps: [
    (at: 0, pads: {0: (), 1: ()}),
    (at: 60, pads: {0: (left: (x: 128, y: 0))}),
    (at: 150, pads: {0: (left: (x: 0, y: 128))}),
    (at: 240, pads: {0: ()}),
    (at: 300, pads: {0: (buttons: [South])}),
    (at: 306, pads: {0: ()}),
    (at: 360, pads: {0: (buttons: [West])}),
    (at: 366, pads: {0: ()}),
])";

/// Which way a replay is observed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum Set {
    /// At the end flip only, then `SCREENSHOT` and `SAVESTATE`.
    A,
    /// Every [`Plan::every`] flips, then `SCREENSHOT`.
    B,
}

impl Set {
    pub fn parse(text: &str) -> Result<Self> {
        match text {
            "A" | "a" => Ok(Set::A),
            "B" | "b" => Ok(Set::B),
            other => bail!("a set is A or B, not {other:?}"),
        }
    }
}

impl std::fmt::Display for Set {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Set::A => "A",
            Set::B => "B",
        })
    }
}

/// One replay's protocol: the script, the set and where it's observed.
#[derive(Debug, Clone)]
pub struct Plan {
    /// `idle`, `walk` or a script file's stem; it names the replay's group.
    pub script_name: String,
    pub script: InputScript,
    /// The script as RON, recorded with every replay.
    pub script_ron: String,
    pub set: Set,
    /// The end flip's offset from F0.
    pub end: u64,
    /// Flips between set B's checkpoints.
    pub every: u64,
}

impl Plan {
    pub fn new(
        script_name: impl Into<String>,
        script_ron: &str,
        set: Set,
        end: u64,
        every: u64,
    ) -> Result<Self> {
        let script_name = script_name.into();
        if end == 0 || every == 0 {
            bail!("the end ({end}) and the spacing ({every}) are flips after F0, at least 1");
        }
        if script_name.is_empty() || script_name.contains(['/', '-', ' ']) {
            bail!("a script name labels a replay dir: no '/', '-' or space in {script_name:?}");
        }
        let script = InputScript::from_ron(script_ron)
            .with_context(|| format!("parsing the {script_name} script"))?;
        Ok(Self {
            script_name,
            script,
            script_ron: script_ron.to_owned(),
            set,
            end,
            every,
        })
    }

    /// A built-in script ([`IDLE`], [`WALK`]) by name, or a RON file named by
    /// its stem.
    pub fn named(name_or_file: &str, set: Set, end: u64, every: u64) -> Result<Self> {
        match name_or_file {
            "idle" => Self::new("idle", IDLE, set, end, every),
            "walk" => Self::new("walk", WALK, set, end, every),
            file => {
                let path = Path::new(file);
                let text = std::fs::read_to_string(path)
                    .with_context(|| format!("reading the script {file}"))?;
                let stem = path
                    .file_stem()
                    .and_then(|s| s.to_str())
                    .with_context(|| format!("{file} has no name"))?
                    .replace(['-', ' '], "_");
                Self::new(stem, &text, set, end, every)
            }
        }
    }

    /// The offsets from F0 a replay pauses and reads at, F0 itself not
    /// counted: set A the end alone, set B every `every` flips and the end.
    pub fn checkpoints(&self) -> Vec<u64> {
        match self.set {
            Set::A => vec![self.end],
            Set::B => {
                let mut offsets: Vec<u64> = (1..=self.end / self.every)
                    .map(|k| k * self.every)
                    .collect();
                if offsets.last() != Some(&self.end) {
                    offsets.push(self.end);
                }
                offsets
            }
        }
    }

    /// The replay's dir name and label: `<script>-<set>-<nn>`.
    pub fn label(&self, replay: u32) -> String {
        format!("{}-{}-{replay:02}", self.script_name, self.set)
    }

    /// A set A replay ends in a `SAVESTATE`.
    pub fn saves_at_end(&self) -> bool {
        self.set == Set::A
    }
}

/// The host when a replay started or ended: other lanes build and test on the
/// same Mac, and the record says how busy it was.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Host {
    /// `sysctl vm.loadavg`: the 1, 5 and 15 minute load averages, `None` when
    /// it didn't answer.
    pub load_average: Option<[f64; 3]>,
    /// `uptime` as it printed.
    pub uptime: String,
    /// `CGSSessionScreenIsLocked`, `None` when `ioreg` didn't say.
    pub screen_locked: Option<bool>,
}

/// An actor's or the camera's placement: metres, then tilt, heading and roll
/// in degrees, and (an actor's) velocity in m/s.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Pose {
    pub position: [f32; 3],
    pub rotation: [f32; 3],
    pub velocity: Option<[f32; 3]>,
}

/// The RNG at one pause, reduced from a [`crate::rng::Peek`] (written beside
/// the record whole).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RngObservation {
    /// The class meta's live count (the census found 1).
    pub instances: i32,
    pub mti: u32,
    /// The MT's `sgenrand` seed and numbers drawn since, when within reach.
    pub seed: Option<u32>,
    pub drawn: Option<u64>,
    /// The animation LCG's steps since power-on's 0x12345.
    pub anim_steps: Option<u64>,
    pub fmod: Option<u32>,
    /// SHA-256 of `_mti` and the 624 words, big-endian: equal states, equal hash.
    pub state_sha256: String,
}

/// What one pause read: the counters, the actor counts, the hero, the camera
/// and the RNG.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Observation {
    /// Flips after F0.
    pub offset: u64,
    /// `STATE`'s counter at the pause.
    pub flip: u64,
    /// `tfbApplication::_curFrame` and `_frameDelta` (seconds).
    pub game_frame: u32,
    pub frame_delta: f32,
    pub counts: Counts,
    /// `None` when no single skinned actor a pad drives was there.
    pub hero: Option<Pose>,
    pub camera: Option<Pose>,
    pub rng: RngObservation,
    /// The actor frame and the peek, in milliseconds.
    pub read_ms: f64,
}

/// The shot at the end flip, copied into the replay's dir.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EndShot {
    pub file: String,
    pub flip: u64,
    pub pixels_sha256: String,
    pub lit_share: f64,
}

/// The end state, copied into the replay's dir.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EndState {
    pub file: String,
    /// The flip it was taken at: the end flip, or a later one after a refusal.
    pub saved_at: u64,
    pub bytes: u64,
    pub sha256: String,
}

/// Everything one replay recorded, as `record.json` in its dir.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ReplayRecord {
    pub label: String,
    pub script: String,
    pub set: Set,
    pub replay: u32,
    /// The oracle's identity line ([`crate::identity::Identity::line`]).
    pub identity: String,
    pub state: String,
    pub state_sha256: String,
    pub figure_sha256: String,
    pub before: Host,
    pub after: Host,
    /// From the spawn to Start Paused's pause.
    pub boot_secs: f64,
    pub f0: u64,
    /// The flip the script's offset 0 landed on.
    pub base: u64,
    /// From the `RESUME` at F0 to the end pause: how fast the run went.
    pub run_secs: f64,
    /// F0 first, then each checkpoint in order; the last is the end flip.
    pub observations: Vec<Observation>,
    pub shot: Option<EndShot>,
    pub saved: Option<EndState>,
    /// RPCS3's reason each time it turned the end `SAVESTATE` down.
    pub save_refusals: Vec<String>,
    /// Why the replay has no end state, when it was asked for one and hasn't.
    pub save_error: Option<String>,
    /// RPCS3's log shows `cellAudio` on the `Null` backend.
    pub silent: bool,
    /// RPCS3's log lines for a guest thread that died, the end state's reboot
    /// included.
    #[serde(default)]
    pub fatal: Vec<String>,
    pub wall_secs: f64,
    pub shutdown: String,
}

impl ReplayRecord {
    pub const FILE: &'static str = "record.json";

    /// The observation at `offset`, if the replay paused there.
    pub fn at(&self, offset: u64) -> Option<&Observation> {
        self.observations.iter().find(|o| o.offset == offset)
    }

    /// The last observation's offset (the end flip).
    pub fn end(&self) -> Option<u64> {
        self.observations.last().map(|o| o.offset)
    }

    pub fn write(&self, dir: &Path) -> Result<PathBuf> {
        let path = rrc_dump::output::refuse_repo_output(&dir.join(Self::FILE))?;
        std::fs::write(
            &path,
            serde_json::to_string_pretty(self).expect("a record serialises"),
        )
        .with_context(|| format!("writing {}", path.display()))?;
        Ok(path)
    }

    pub fn load(path: &Path) -> Result<Self> {
        let text =
            std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
        serde_json::from_str(&text).with_context(|| format!("parsing {}", path.display()))
    }
}

/// Every `<replay>/record.json` under `dir`, in label order.
pub fn load_records(dir: &Path) -> Result<Vec<ReplayRecord>> {
    let mut paths: Vec<PathBuf> = std::fs::read_dir(dir)
        .with_context(|| format!("listing {}", dir.display()))?
        .flatten()
        .map(|e| e.path().join(ReplayRecord::FILE))
        .filter(|p| p.is_file())
        .collect();
    paths.sort();
    paths.iter().map(|p| ReplayRecord::load(p)).collect()
}

/// The actor frame and peek file names for an offset, in a replay's dir.
pub fn actors_file(offset: u64) -> String {
    format!("actors-{offset:05}.json")
}

pub fn peek_file(offset: u64) -> String {
    format!("peek-{offset:05}.json")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_plan_pauses_at_the_end_or_every_spacing_and_the_end() {
        let a = Plan::named("idle", Set::A, END, EVERY).unwrap();
        assert_eq!(a.checkpoints(), [1_800]);
        assert!(a.saves_at_end());
        assert_eq!(a.label(3), "idle-A-03");
        let b = Plan::named("walk", Set::B, END, EVERY).unwrap();
        let offsets = b.checkpoints();
        assert_eq!(offsets.len(), 30);
        assert_eq!((offsets[0], offsets[29]), (60, 1_800));
        assert!(offsets.windows(2).all(|w| w[1] - w[0] == 60));
        assert!(!b.saves_at_end());
        // An end off the spacing is still a checkpoint.
        let odd = Plan::named("idle", Set::B, 130, 60).unwrap();
        assert_eq!(odd.checkpoints(), [60, 120, 130]);
        assert!(Plan::named("idle", Set::B, 0, 60).is_err());
        assert!(Plan::named("idle", Set::B, 60, 0).is_err());
        assert!(Plan::new("a-b", IDLE, Set::A, 60, 60).is_err());
        assert_eq!(Set::parse("b").unwrap(), Set::B);
        assert!(Set::parse("C").is_err());
    }

    #[test]
    fn the_walk_is_forward_then_left_then_a_jump_and_an_attack_on_port_0() {
        use guac_oracle::input::{Button, Stick};
        let walk = Plan::named("walk", Set::B, END, EVERY).unwrap().script;
        let at = |offset| walk.state_at(0, offset).unwrap();
        assert!(at(0).buttons.is_empty() && at(59).left.is_centred());
        assert_eq!(at(60).left, Stick { x: 128, y: 0 });
        assert_eq!(at(149).left, Stick { x: 128, y: 0 });
        assert_eq!(at(150).left, Stick { x: 0, y: 128 });
        assert!(at(240).left.is_centred());
        assert_eq!(at(300).buttons.iter().collect::<Vec<_>>(), [&Button::South]);
        assert!(at(306).buttons.is_empty());
        assert_eq!(at(365).buttons.iter().collect::<Vec<_>>(), [&Button::West]);
        assert!(at(366).buttons.is_empty() && at(1_800).left.is_centred());
        // Port 1 rests from offset 0 on, as in the idle script.
        assert!(walk.state_at(1, 1_800).unwrap().left.is_centred());
        let idle = Plan::named("idle", Set::A, END, EVERY).unwrap().script;
        assert_eq!(idle.state_at(0, 1_800), idle.state_at(0, 0));
    }

    #[test]
    fn a_script_file_is_named_by_its_stem() {
        let tmp = tempfile::tempdir().unwrap();
        let file = tmp.path().join("walk-late press.ron");
        std::fs::write(&file, WALK).unwrap();
        let plan = Plan::named(file.to_str().unwrap(), Set::B, 120, 60).unwrap();
        assert_eq!(plan.script_name, "walk_late_press");
        assert_eq!(plan.label(0), "walk_late_press-B-00");
        std::fs::write(&file, "(steps: [(at: 0, pads: {0: (left: (x: 256))})])").unwrap();
        assert!(Plan::named(file.to_str().unwrap(), Set::B, 120, 60).is_err());
    }
}
