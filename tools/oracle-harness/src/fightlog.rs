//! The fight's metrics over the oracle's own replays (openspec AH.9, design
//! decision 18): each replay's recording and chain records read back into a
//! [`FightTrace`] as AH.6 built it, its per-actor event logs, chain links,
//! breaks, totals and the generator's draws per game frame; then, over a
//! group of pause-free replays of one script from one anchor, the anchor's
//! horizon, each event kind's floor by AF.12's method (the least that passes
//! every replay's leave-one-out, rounded up) and candidates (a press moved,
//! a perturbation) judged against the whole group at those floors, or at an
//! anchor's named ones from `guac_oracle::tolerance` (AH.51).
//!
//! A replay is a dir `oracle goto` or `oracle divergence run` wrote with O5's
//! watch and O6's chains: `recording.json`, `chain.json`,
//! `chain-records.json` and the generator read at F0 (`peek-f0.json` from a
//! drive, `peek-00000.json` from a divergence replay). Each kind's behaviour
//! cells are named from what the end pause named (`chain.json`) and the
//! hero's clips from O6's [`crate::chains::ANIMS`], so the trace compares
//! names, as guacamole's headless runner will.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use guac_oracle::fight::draws::{anchor_horizon, draws};
use guac_oracle::fight::extract::{self, Break, Link, Rules};
use guac_oracle::fight::log::{
    ActorLogs, Gate, Judge, LogReference, LogVerdict, floors_needed, leave_one_out,
};
use guac_oracle::fight::oracle::{ChainRecord, Names, fight_trace};
use guac_oracle::fight::ticks::{FightTrace, SpawnRule};
use guac_oracle::recording::Recording;
use guac_oracle::tolerance::EventFloors;
use guac_oracle::totals::Totals;
use guac_oracle::trace::Horizon;
use serde::{Deserialize, Serialize};

use crate::chains::{CHAIN_FILE, CHAIN_RECORDS, ChainFile};
use crate::divergence::RECORDING_FILE;
use crate::rng::Peek;

/// The F0 peeks a replay dir may hold: a drive's, a divergence replay's.
pub const F0_PEEKS: [&str; 2] = ["peek-f0.json", "peek-00000.json"];

/// What a report reads a group's replays by (RON).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReportRules {
    pub spawns: Vec<SpawnRule>,
    pub rules: Rules,
    /// Event kinds a roll times: by order alone past the horizon, and
    /// everywhere unless `stamps_past_horizon`.
    #[serde(default)]
    pub ordered: BTreeSet<String>,
    /// Game seconds a floor is rounded up to (AF.12: the largest spread
    /// rounded up). A frame at 60 Hz is 1/60.
    #[serde(default = "default_quantum")]
    pub quantum: f64,
    /// Twists searched between two records when counting draws.
    #[serde(default = "default_twists")]
    pub max_twists: u64,
    /// AH.45's (b), the rule since 2026-10-21 ([`Gate::RolledKinds`]):
    /// every kind not in `ordered` stamped wherever it falls, the horizon
    /// gating only `ordered`. Unset is decision 18 as first written
    /// ([`Gate::AllKinds`]): nothing stamped past the horizon and `ordered`
    /// never. At anchor A the generator parts 1 to 4 game frames from F0, by
    /// script (particles drawing on host time), while no event a lob makes is
    /// timed by a roll (AH.9), so the first rule stamps nothing there.
    #[serde(default)]
    pub stamps_past_horizon: bool,
}

fn default_quantum() -> f64 {
    0.005
}

fn default_twists() -> u64 {
    10_000
}

impl ReportRules {
    pub fn from_ron(text: &str) -> Result<Self> {
        let r: Self = ron::from_str(text).context("parsing the report's rules")?;
        if !(r.quantum.is_finite() && r.quantum > 0.0) {
            bail!("a floor's quantum is a positive number of seconds");
        }
        Ok(r)
    }
}

/// A replay dir read back: its dir's name, its trace and the draws its
/// F0 peek counts.
pub struct Replay {
    pub label: String,
    pub trace: FightTrace,
    /// Draws since F0 per game frame since F0.
    pub draws: Vec<(u32, u64)>,
}

/// The behaviour names `fight_trace` takes, from what the end pause named:
/// each kind's cells as its items carried them.
pub fn names(file: &ChainFile, records: &[ChainRecord]) -> Names {
    let mut out = Names {
        clips: file.resolved.clip_names.clone(),
        ..Names::default()
    };
    for r in records {
        for i in &r.items {
            if let Some(cell) = i.behaviour
                && let Some(name) = file.resolved.behaviours.get(&cell)
            {
                out.behaviours
                    .entry(i.kind.clone())
                    .or_default()
                    .insert(cell, name.clone());
            }
        }
    }
    out
}

/// Every clip the recording's frames carry named: one the end pause named
/// nothing for (AH.10: the hurt reaction a bite lays over the blend reads no
/// `tfbPhysicsAnimation` name) goes by its id, as the end pause names one
/// whose name won't read.
pub fn name_every_clip(names: &mut Names, recording: &Recording) {
    for frame in recording.frames() {
        for c in &frame.clips {
            names
                .clips
                .entry(c.id)
                .or_insert_with(|| format!("clip {:#010x}", c.id));
        }
    }
}

fn read_json<T: serde::de::DeserializeOwned>(path: &Path) -> Result<T> {
    serde_json::from_str(
        &fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?,
    )
    .with_context(|| format!("parsing {}", path.display()))
}

/// A replay dir as a trace, its draws counted from its F0 peek.
pub fn load(dir: &Path, rules: &ReportRules) -> Result<Replay> {
    let recording: Recording = read_json(&dir.join(RECORDING_FILE))?;
    let file: ChainFile = read_json(&dir.join(CHAIN_FILE))?;
    let records: Vec<ChainRecord> = read_json(&dir.join(CHAIN_RECORDS))?;
    let peek_at = F0_PEEKS
        .iter()
        .map(|p| dir.join(p))
        .find(|p| p.exists())
        .with_context(|| format!("no F0 peek in {}", dir.display()))?;
    let peek: Peek = read_json(&peek_at)?;
    let mut named = names(&file, &records);
    name_every_clip(&mut named, &recording);
    let trace = fight_trace(&recording, &records, &named, &rules.spawns)
        .with_context(|| format!("the trace of {}", dir.display()))?;
    let draws = draws(&trace, &peek.mt, rules.max_twists)
        .with_context(|| format!("the draws of {}", dir.display()))?;
    let label = dir
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    Ok(Replay {
        label,
        trace,
        draws,
    })
}

/// What one replay did, by AH.6's extractors.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ReplayReport {
    pub label: String,
    pub ticks: usize,
    pub logs: ActorLogs,
    pub links: Vec<Link>,
    pub breaks: Vec<Break>,
    pub totals: Totals,
    /// Draws since F0 at the last tick.
    pub drawn: u64,
    /// Draws in each game frame since F0, frame by frame.
    pub per_frame: Vec<u64>,
}

impl ReplayReport {
    pub fn of(r: &Replay, rules: &Rules) -> Result<Self> {
        let logs = extract::logs(&r.trace, rules)?;
        let links = rules
            .chain
            .as_ref()
            .map(|c| extract::chain(&r.trace, c))
            .unwrap_or_default();
        let breaks = rules
            .breaks
            .as_ref()
            .map(|b| extract::breaks(&r.trace, b))
            .unwrap_or_default();
        let totals = extract::totals(&r.trace, rules)?;
        let per_frame = r.draws.windows(2).map(|w| w[1].1 - w[0].1).collect();
        Ok(Self {
            label: r.label.clone(),
            ticks: r.trace.ticks().len(),
            logs,
            links,
            breaks,
            totals,
            drawn: r.draws.last().map_or(0, |d| d.1),
            per_frame,
        })
    }
}

/// One replay's leave-one-out, or a candidate's check, at the floors.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Judged {
    pub label: String,
    pub passed: bool,
    pub failed: Vec<String>,
    pub verdict: LogVerdict,
}

impl Judged {
    fn of(label: &str, verdict: LogVerdict) -> Self {
        Self {
            label: label.to_owned(),
            passed: verdict.passed(),
            failed: verdict.failed().into_iter().map(str::to_owned).collect(),
            verdict,
        }
    }
}

/// A group of replays of one script from one anchor, and candidates against
/// it.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct GroupReport {
    pub replays: Vec<ReplayReport>,
    /// The anchor's horizon on the draws: the last game frame since F0 every
    /// replay drew alike by, and the first they parted at.
    pub horizon: Horizon,
    /// The least floor each kind needs for every leave-one-out to pass.
    pub needed: BTreeMap<String, f64>,
    /// Those rounded up to the quantum: the floors.
    pub floors: BTreeMap<String, f64>,
    /// What the leave-one-out and the candidates were judged by: `floors`,
    /// or an anchor's named floors when `named`.
    pub judge: Judge,
    pub named: bool,
    pub reference: LogReference,
    pub leave_one_out: Vec<Judged>,
    pub candidates: Vec<(ReplayReport, Judged)>,
}

/// `x` rounded up to a whole number of `quantum`s, 0 kept 0.
pub fn round_up(x: f64, quantum: f64) -> f64 {
    let n = (x / quantum - 1e-9).ceil().max(0.0);
    n * quantum
}

impl GroupReport {
    /// Three or more replays, AF.12's method: the horizon, the floors each
    /// kind needs, each replay against the others at them; then each
    /// candidate against all of them.
    pub fn build(group: &[Replay], candidates: &[Replay], rules: &ReportRules) -> Result<Self> {
        Self::build_at(group, candidates, rules, None)
    }

    /// As [`GroupReport::build`], the group's floors still calibrated, but
    /// the leave-one-out and the candidates judged at `named`'s floors under
    /// AH.45's (b), its rolled kinds joining `rules.ordered`: whether an
    /// anchor's named defaults hold on this group.
    pub fn build_at(
        group: &[Replay],
        candidates: &[Replay],
        rules: &ReportRules,
        named: Option<&EventFloors>,
    ) -> Result<Self> {
        let replays = group
            .iter()
            .map(|r| ReplayReport::of(r, &rules.rules))
            .collect::<Result<Vec<_>>>()?;
        let draws: Vec<Vec<(u32, u64)>> = group.iter().map(|r| r.draws.clone()).collect();
        let horizon = anchor_horizon(&draws)?;
        let runs: Vec<ActorLogs> = replays.iter().map(|r| r.logs.clone()).collect();
        let open = Judge {
            default: 0.0,
            floors: BTreeMap::new(),
            horizon: Some(horizon.last_agreeing.map_or(0, |h| h as u32)),
            ordered: rules.ordered.clone(),
            gate: if rules.stamps_past_horizon {
                Gate::RolledKinds
            } else {
                Gate::AllKinds
            },
        };
        // Every kind the replays logged, 0 where none strayed.
        let mut needed = floors_needed(&runs, &open)?;
        for run in &runs {
            for a in run.actors() {
                for e in run.log(a) {
                    needed.entry(e.kind.clone()).or_insert(0.0);
                }
            }
        }
        let floors: BTreeMap<String, f64> = needed
            .iter()
            .map(|(k, &v)| (k.clone(), round_up(v, rules.quantum)))
            .collect();
        let judge = match named {
            Some(named) => {
                let mut judge = named.judge(open.horizon);
                judge.ordered.extend(rules.ordered.iter().cloned());
                judge
            }
            None => Judge {
                floors: floors.clone(),
                ..open
            },
        };
        let leave_one_out = leave_one_out(&runs, &judge)?
            .into_iter()
            .zip(&replays)
            .map(|(v, r)| Judged::of(&r.label, v))
            .collect();
        let reference = LogReference::from_runs(&runs)?;
        let candidates = candidates
            .iter()
            .map(|c| {
                let report = ReplayReport::of(c, &rules.rules)?;
                let verdict = reference.check(&report.logs, &judge)?;
                let judged = Judged::of(&c.label, verdict);
                Ok((report, judged))
            })
            .collect::<Result<Vec<_>>>()?;
        Ok(Self {
            replays,
            horizon,
            needed,
            floors,
            judge,
            named: named.is_some(),
            reference,
            leave_one_out,
            candidates,
        })
    }

    /// The numbers a box records, as text.
    pub fn text(&self) -> String {
        let mut out = String::new();
        let mut line = |s: String| {
            out.push_str(&s);
            out.push('\n');
        };
        line(format!(
            "{} replays; horizon: agreeing through game frame {:?} since F0, parted at {:?}; stamps judged {}",
            self.replays.len(),
            self.horizon.last_agreeing,
            self.horizon.parted_at,
            match (self.judge.gate, self.judge.horizon) {
                (_, None) => "everywhere".to_owned(),
                (Gate::AllKinds, Some(h)) => format!("through frame {h}"),
                (Gate::RolledKinds, Some(h)) => {
                    format!("everywhere, the rolled kinds through frame {h}")
                }
            }
        ));
        for r in &self.replays {
            line(format!(
                "  {}: {} ticks, {} drawn, {} events in {} logs, {} throws, {} breaks, totals {:?}",
                r.label,
                r.ticks,
                r.drawn,
                r.logs.actors().map(|a| r.logs.log(a).len()).sum::<usize>(),
                r.logs.actors().count(),
                r.links.len(),
                r.breaks.len(),
                r.totals
            ));
        }
        line("floors (needed -> rounded up), game seconds:".into());
        for (k, v) in &self.needed {
            line(format!("  {k}: {v:.6} -> {:.3}", self.floors[k]));
        }
        if self.named {
            line(format!(
                "judged at the named floors (default {:.3}), rolled {:?}:",
                self.judge.default, self.judge.ordered
            ));
            for k in self.needed.keys() {
                line(format!("  {k}: {:.3}", self.judge.floor(k)));
            }
        }
        line("reference, per actor (present, prefix, parted):".into());
        for (name, a) in self.reference.actors() {
            let bands: Vec<String> = a
                .prefix
                .iter()
                .map(|b| {
                    format!(
                        "{} {:.4}-{:.4} (f{}-{})",
                        b.kind, b.earliest, b.latest, b.first_frame, b.last_frame
                    )
                })
                .collect();
            line(format!(
                "  {name}: {} of {}; {}; {}",
                a.present,
                self.replays.len(),
                bands.join(", "),
                a.parted.as_ref().map_or("agreed".to_owned(), |p| format!(
                    "parted at {}: {:?}",
                    p.index, p.outcomes
                ))
            ));
        }
        for j in &self.leave_one_out {
            line(format!(
                "leave-one-out {}: {}{}",
                j.label,
                if j.passed { "passes" } else { "FAILS" },
                if j.failed.is_empty() {
                    String::new()
                } else {
                    format!(" ({})", j.failed.join(", "))
                }
            ));
        }
        for (r, j) in &self.candidates {
            let first = j
                .verdict
                .actors
                .iter()
                .filter(|(_, v)| !v.passed())
                .map(|(a, v)| format!("{a}: {:?}", v.mismatches.first()))
                .take(3)
                .collect::<Vec<_>>();
            line(format!(
                "candidate {}: {} ({} breaks, totals {:?}){}",
                r.label,
                if j.passed { "passes" } else { "fails" },
                r.breaks.len(),
                r.totals,
                if first.is_empty() {
                    String::new()
                } else {
                    format!("; {}", first.join("; "))
                }
            ));
        }
        out
    }
}

/// `oracle fight report`: reads `dirs` and `candidates`, judged at the
/// group's floors or `named`'s, and writes `report.json` and `report.txt`
/// into `out` (outside every work tree).
pub fn report(
    rules_at: &Path,
    dirs: &[PathBuf],
    candidates: &[PathBuf],
    named: Option<&EventFloors>,
    out: &Path,
) -> Result<GroupReport> {
    let rules = ReportRules::from_ron(
        &fs::read_to_string(rules_at).with_context(|| format!("reading {}", rules_at.display()))?,
    )?;
    let group = dirs
        .iter()
        .map(|d| load(d, &rules))
        .collect::<Result<Vec<_>>>()?;
    let cands = candidates
        .iter()
        .map(|d| load(d, &rules))
        .collect::<Result<Vec<_>>>()?;
    let report = GroupReport::build_at(&group, &cands, &rules, named)?;
    let dir = rrc_dump::output::refuse_repo_output(out)?;
    fs::create_dir_all(&dir)?;
    fs::write(
        dir.join("report.json"),
        serde_json::to_string_pretty(&report)?,
    )?;
    fs::write(dir.join("report.txt"), report.text())?;
    Ok(report)
}

#[cfg(test)]
mod tests;
