//! AF.12's checks of the defaults in `guac_oracle::tolerance` (design decision
//! 9's acceptance), over a measurement's records:
//!
//! - [`leave_one_out`]: per group, each replay as a candidate against the
//!   others, the way [`check`] judges one against a reference: on every
//!   compared observable inside the OTHERS' usable horizon at its default
//!   floor (which has to pass) and over the whole run (reported: past a
//!   horizon the envelope is statistical); the RNG's state inside the others'
//!   identical-state horizon; and at every checkpoint the group's replays all
//!   have a shot at, each replay's FLIP against the band of the others plus
//!   [`tolerance::FRAME_FLOOR`], gated inside the others' scene horizon and
//!   reported past it. A horizon read over the whole group, the replay judged
//!   included, would pass every replay by construction (the AF.11-AF.13
//!   review found the first cut doing that);
//! - [`check`]: a candidate replay (a perturbed script's, say) against a
//!   reference group, the same metrics: each trajectory inside the reference's
//!   horizon, where the oracle agrees with itself, and over the whole run, and
//!   the frames at every checkpoint both have shots at, gated the same way.
//!
//! The scene's horizon is the last checkpoint inside both the hero's and the
//! camera's position horizons: a frame shows both, so past it the frames part
//! as the placements do. AF.12's controls found why that gate is needed: of six
//! unperturbed walks one alone took the gate corner's other branch, and its
//! frames from F0 + 180 on failed the band of the other five by 0.11-0.20 past
//! the floor; a band over two branches holds only while each has two replays.

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::path::Path;

use anyhow::{Context, Result, bail};
use guac_oracle::frames::{self, FlipMatrix, FrameBand, FrameVerdict};
use guac_oracle::tolerance;
use guac_oracle::trace::{
    self, Excursion, Horizon, Sample, Trace, TrajectoryTolerance, TrajectoryVerdict,
};
use serde::Serialize;

use super::report::{Spec, common_offsets, frame_at, observable, specs};
use super::{Observation, ReplayRecord, Set};
use crate::actors::angle_from;

/// A replay that failed its leave-one-out somewhere: where first and by how much.
#[derive(Debug, Clone, Serialize)]
pub struct Failure {
    pub label: String,
    pub first_outside: Option<u64>,
    pub worst: Excursion,
}

/// Each replay of a group against the envelope of the others on one
/// observable, at its default tolerance: inside the others' usable horizon
/// read at that floor (gated) and over the whole run (reported).
#[derive(Debug, Clone, Serialize)]
pub struct ObservableLoo {
    pub name: String,
    pub unit: String,
    /// `None`: not compared (host time).
    pub tolerance: Option<TrajectoryTolerance>,
    /// The whole group's, at the default floor; `None` when not compared.
    pub horizon: Option<Horizon>,
    /// Each replay's judge: the others' last agreeing checkpoint at the
    /// default floor, in label order (empty when not compared).
    pub others: Vec<Option<u64>>,
    /// (replay, checkpoint, component) values checked inside the horizons.
    pub checked: usize,
    /// Replays that failed inside the others' horizon: none, or the defaults
    /// are wrong.
    pub failed: Vec<Failure>,
    /// Replays that failed over the whole run, reported, not gated.
    pub past: Vec<Failure>,
}

/// Each replay's shot at one checkpoint FLIPped against every other's, its
/// worst mean against the largest pair among the others plus the frame floor.
#[derive(Debug, Clone, Serialize)]
pub struct FramesLoo {
    pub offset: u64,
    /// Inside the whole group's scene horizon.
    pub inside: bool,
    pub replays: usize,
    /// The largest mean FLIP between two replays' shots.
    pub replay_max: f64,
    /// The smallest floor every replay passes at.
    pub needed_floor: f64,
    /// Replays failing at [`tolerance::FRAME_FLOOR`] inside the others' scene
    /// horizon: gated.
    pub failed: Vec<String>,
    /// Replays failing past the others' scene horizon: reported.
    pub past: Vec<String>,
}

/// A script under one set (idle, set B, say): every compared observable's
/// leave-one-out, the RNG's inside its identical-state horizon and the shots'.
#[derive(Debug, Clone, Serialize)]
pub struct GroupLoo {
    pub script: String,
    pub set: Set,
    pub labels: Vec<String>,
    pub observables: Vec<ObservableLoo>,
    /// The whole group's identical-state horizon for the RNG, and the replays
    /// whose state differs from the others' inside the others' horizon.
    pub rng_horizon: Horizon,
    pub rng_failed: Vec<String>,
    /// The last checkpoint inside the whole group's hero and camera horizons.
    pub scene_horizon: Option<u64>,
    /// Each replay's frames are gated to the others' scene horizon, in label
    /// order.
    pub scene_others: Vec<Option<u64>>,
    pub frames: Vec<FramesLoo>,
}

impl GroupLoo {
    /// Every replay inside the others' horizon on every compared observable,
    /// on the RNG and on the frames.
    pub fn passed(&self) -> bool {
        self.rng_failed.is_empty()
            && self.observables.iter().all(|o| o.failed.is_empty())
            && self.frames.iter().all(|f| f.failed.is_empty())
    }

    pub fn write_text(&self, out: &mut String) {
        let _ = writeln!(
            out,
            "== {}, set {}: leave-one-out of {} replays: {} ==",
            self.script,
            self.set,
            self.labels.len(),
            if self.passed() { "PASSED" } else { "FAILED" }
        );
        for o in &self.observables {
            let (Some(t), Some(h)) = (o.tolerance, o.horizon) else {
                let _ = writeln!(out, "{}: not compared (host time)", o.name);
                continue;
            };
            let past: Vec<String> = o
                .past
                .iter()
                .map(|f| {
                    format!(
                        "{} from {} ({:+.3e} {} at {})",
                        f.label,
                        f.first_outside.map_or("?".into(), |v| v.to_string()),
                        f.worst.by,
                        o.unit,
                        f.worst.offset
                    )
                })
                .collect();
            let _ = writeln!(
                out,
                "{} (floor {:e} {}, k {}): horizon {}{}, the others' {}; inside those {} values, \
                 {} failed{}; over the whole run {} of {} fail{}",
                o.name,
                t.floor,
                o.unit,
                t.k,
                h.last_agreeing.map_or("none".into(), |v| v.to_string()),
                h.parted_at
                    .map_or(" (held)".into(), |p| format!(" (parts {p})")),
                range_text(&o.others),
                o.checked,
                o.failed.len(),
                if o.failed.is_empty() {
                    String::new()
                } else {
                    format!(
                        ": {}",
                        o.failed
                            .iter()
                            .map(|f| f.label.as_str())
                            .collect::<Vec<_>>()
                            .join(", ")
                    )
                },
                o.past.len(),
                self.labels.len(),
                if past.is_empty() {
                    String::new()
                } else {
                    format!(": {}", past.join("; "))
                },
            );
        }
        let _ = writeln!(
            out,
            "RNG: identical through {} (parts {}); differing inside the others' horizon: {}",
            h_text(self.rng_horizon.last_agreeing),
            h_text(self.rng_horizon.parted_at),
            if self.rng_failed.is_empty() {
                "none".into()
            } else {
                self.rng_failed.join(", ")
            }
        );
        let _ = writeln!(
            out,
            "the scene's horizon (hero and camera): {}, the others' {}",
            h_text(self.scene_horizon),
            range_text(&self.scene_others)
        );
        for f in &self.frames {
            let _ = writeln!(
                out,
                "frames at {} ({}): {} replays, largest pair {:.5}, floor needed {:.5} (default \
                 {}){}{}",
                f.offset,
                if f.inside { "inside" } else { "past" },
                f.replays,
                f.replay_max,
                f.needed_floor,
                tolerance::FRAME_FLOOR,
                if f.failed.is_empty() {
                    String::new()
                } else {
                    format!("; FAILED: {}", f.failed.join(", "))
                },
                if f.past.is_empty() {
                    String::new()
                } else {
                    format!(
                        "; past the others' horizon, reported: {}",
                        f.past.join(", ")
                    )
                }
            );
        }
        out.push('\n');
    }
}

fn h_text(v: Option<u64>) -> String {
    v.map_or("none".into(), |v| v.to_string())
}

/// The spread of the others' horizons over a group's replays: `120`, or
/// `180-240` when they differ (`none` sorts first).
fn range_text(horizons: &[Option<u64>]) -> String {
    let (Some(lo), Some(hi)) = (horizons.iter().min(), horizons.iter().max()) else {
        return "none".into();
    };
    if lo == hi {
        h_text(*lo)
    } else {
        format!("{}-{}", h_text(*lo), h_text(*hi))
    }
}

/// The observables a frame shows the agreement of.
const SCENE: [&str; 2] = ["hero position", "camera position"];

/// The last checkpoint inside every [`SCENE`] observable's horizon; `None`
/// when one parts at its first checkpoint or isn't there.
fn scene_horizon<'a>(horizons: impl Iterator<Item = (&'a str, Option<Horizon>)>) -> Option<u64> {
    scene_last(horizons.map(|(name, h)| (name, h.and_then(|h| h.last_agreeing))))
}

/// [`scene_horizon`] from each observable's last agreeing checkpoint.
fn scene_last<'a>(lasts: impl Iterator<Item = (&'a str, Option<u64>)>) -> Option<u64> {
    let found: Vec<Option<u64>> = lasts
        .filter(|(name, _)| SCENE.contains(name))
        .map(|(_, last)| last)
        .collect();
    if found.len() < SCENE.len() {
        return None;
    }
    found
        .into_iter()
        .try_fold(u64::MAX, |acc, h| Some(acc.min(h?)))
}

/// The groups of `records` (by script and set, three replays or more) and
/// their leave-one-out; frames are read from `dir`'s shots when given.
pub fn leave_one_out(records: &[ReplayRecord], dir: Option<&Path>) -> Result<Vec<GroupLoo>> {
    let mut by: BTreeMap<(String, Set), Vec<&ReplayRecord>> = BTreeMap::new();
    for r in records {
        by.entry((r.script.clone(), r.set)).or_default().push(r);
    }
    let mut out = Vec::new();
    for ((script, set), mut replays) in by {
        if replays.len() < 3 {
            continue;
        }
        replays.sort_by(|a, b| a.label.cmp(&b.label));
        out.push(group_loo(script, set, &replays, dir)?);
    }
    Ok(out)
}

fn group_loo(
    script: String,
    set: Set,
    replays: &[&ReplayRecord],
    dir: Option<&Path>,
) -> Result<GroupLoo> {
    let labels: Vec<String> = replays.iter().map(|r| r.label.clone()).collect();
    let offsets = common_offsets(replays);
    let mut observables = Vec::new();
    for spec in specs() {
        let o = observable(&spec, replays, &offsets)?;
        let (Some(t), Some(h)) = (o.tolerance, o.horizon) else {
            observables.push(ObservableLoo {
                name: o.name,
                unit: o.unit,
                tolerance: None,
                horizon: None,
                others: vec![],
                checked: 0,
                failed: vec![],
                past: vec![],
            });
            continue;
        };
        let (mut checked, mut failed, mut past, mut others) = (0, vec![], vec![], vec![]);
        for (i, v) in trace::leave_one_out(&o.traces, &t)?.into_iter().enumerate() {
            others.push(v.horizon.last_agreeing);
            if let Some(inside) = &v.inside {
                checked += inside.checked;
                if !inside.passed() {
                    failed.push(failure(&labels[i], inside));
                }
            }
            if !v.whole.passed() {
                past.push(failure(&labels[i], &v.whole));
            }
        }
        observables.push(ObservableLoo {
            name: o.name,
            unit: o.unit,
            tolerance: Some(t),
            horizon: Some(h),
            others,
            checked,
            failed,
            past,
        });
    }
    let (rng_horizon, rng_failed) = rng_loo(replays, &offsets)?;
    let scene = scene_horizon(observables.iter().map(|o| (o.name.as_str(), o.horizon)));
    let scene_others: Vec<Option<u64>> = (0..replays.len())
        .map(|i| {
            scene_last(
                observables
                    .iter()
                    .map(|o| (o.name.as_str(), o.others.get(i).copied().flatten())),
            )
        })
        .collect();
    let mut frames = Vec::new();
    if let Some(dir) = dir {
        for &offset in &offsets {
            let Some(stack) = shots_at(dir, replays, offset)? else {
                continue;
            };
            let matrix = FlipMatrix::of(&stack, frames::reference_pixels_per_degree())?;
            let (mut failed, mut past) = (vec![], vec![]);
            for (i, v) in matrix
                .leave_one_out(tolerance::FRAME_FLOOR)?
                .iter()
                .enumerate()
            {
                if v.passed() {
                    continue;
                }
                if scene_others[i].is_some_and(|last| offset <= last) {
                    failed.push(labels[i].clone());
                } else {
                    past.push(labels[i].clone());
                }
            }
            frames.push(FramesLoo {
                offset,
                inside: scene.is_some_and(|last| offset <= last),
                replays: stack.len(),
                replay_max: matrix.band().replay_max,
                needed_floor: matrix.leave_one_out_floor()?,
                failed,
                past,
            });
        }
    }
    Ok(GroupLoo {
        script,
        set,
        labels,
        observables,
        rng_horizon,
        rng_failed,
        scene_horizon: scene,
        scene_others,
        frames,
    })
}

fn failure(label: &str, v: &TrajectoryVerdict) -> Failure {
    Failure {
        label: label.to_owned(),
        first_outside: v.first_outside,
        worst: v.worst,
    }
}

/// The group's RNG horizon (identical MT states and animation LCG steps), and
/// the replays whose state differs from the others' inside the others' own
/// identical-state horizon.
fn rng_loo(replays: &[&ReplayRecord], offsets: &[u64]) -> Result<(Horizon, Vec<String>)> {
    let state = |r: &ReplayRecord, offset: u64| {
        r.at(offset)
            .map(|o| (o.rng.state_sha256.clone(), o.rng.anim_steps))
    };
    let identical_at = |among: &[&&ReplayRecord], offset: u64| -> bool {
        let states: Vec<_> = among.iter().map(|r| state(r, offset)).collect();
        states.iter().all(|s| s.is_some() && *s == states[0])
    };
    let all: Vec<&&ReplayRecord> = replays.iter().collect();
    let horizon = Horizon::from_agreement(offsets.iter().map(|&o| (o, identical_at(&all, o))))?;
    let mut failed = Vec::new();
    for (i, r) in replays.iter().enumerate() {
        let others: Vec<&&ReplayRecord> = replays
            .iter()
            .enumerate()
            .filter(|&(j, _)| j != i)
            .map(|(_, r)| r)
            .collect();
        let theirs =
            Horizon::from_agreement(offsets.iter().map(|&o| (o, identical_at(&others, o))))?;
        let strays = offsets
            .iter()
            .filter(|&&o| theirs.last_agreeing.is_some_and(|last| o <= last))
            .any(|&o| state(r, o) != state(others[0], o));
        if strays {
            failed.push(r.label.clone());
        }
    }
    Ok((horizon, failed))
}

/// Every replay's shot at `offset`, decoded a thread a shot (a debug build's
/// PNG decoder is slow); `None` unless all have one.
fn shots_at(
    dir: &Path,
    replays: &[&ReplayRecord],
    offset: u64,
) -> Result<Option<Vec<frames::Frame>>> {
    if replays.iter().any(|r| r.shot_at(offset).is_none()) {
        return Ok(None);
    }
    let decoded: Vec<Result<Option<frames::Frame>>> = std::thread::scope(|scope| {
        let handles: Vec<_> = replays
            .iter()
            .map(|r| scope.spawn(move || frame_at(dir, r, offset)))
            .collect();
        handles
            .into_iter()
            .map(|h| h.join().expect("a decoding thread doesn't panic"))
            .collect()
    });
    decoded.into_iter().collect()
}

/// A reference group to check candidates against: its replays' traces of
/// every compared observable, their horizons and, where they all have shots,
/// each checkpoint's FLIP band.
pub struct Reference {
    pub script: String,
    pub set: Set,
    pub labels: Vec<String>,
    /// Per spec: the reference's traces, horizon and tolerance.
    observables: Vec<(Spec, super::Observable)>,
    /// Per checkpoint with shots: the replays' frames and the band of their
    /// pairs.
    bands: BTreeMap<u64, (Vec<frames::Frame>, FrameBand)>,
    /// The last checkpoint frames are gated at.
    pub scene_horizon: Option<u64>,
}

impl Reference {
    /// The group `script`, `set` of `dir`'s records (three replays or more).
    pub fn load(dir: &Path, script: &str, set: Set) -> Result<Self> {
        let mut records: Vec<ReplayRecord> = super::load_records(dir)?
            .into_iter()
            .filter(|r| r.script == script && r.set == set)
            .collect();
        if records.len() < 3 {
            bail!(
                "{} has {} {script} set {set} replays: a reference needs three",
                dir.display(),
                records.len()
            );
        }
        records.sort_by(|a, b| a.label.cmp(&b.label));
        let refs: Vec<&ReplayRecord> = records.iter().collect();
        let offsets = common_offsets(&refs);
        let mut observables = Vec::new();
        for spec in specs() {
            if spec.tolerance.is_none() {
                continue;
            }
            let o = observable(&spec, &refs, &offsets)?;
            observables.push((spec, o));
        }
        let mut bands = BTreeMap::new();
        for &offset in &offsets {
            if let Some(stack) = shots_at(dir, &refs, offset)? {
                let band = FlipMatrix::of(&stack, frames::reference_pixels_per_degree())?.band();
                bands.insert(offset, (stack, band));
            }
        }
        let scene_horizon = scene_horizon(observables.iter().map(|(s, o)| (s.name, o.horizon)));
        Ok(Self {
            script: script.to_owned(),
            set,
            labels: records.iter().map(|r| r.label.clone()).collect(),
            observables,
            bands,
            scene_horizon,
        })
    }

    /// The checkpoints with a FLIP band.
    pub fn frame_offsets(&self) -> Vec<u64> {
        self.bands.keys().copied().collect()
    }
}

/// One trajectory observable of a candidate against the reference.
#[derive(Debug, Clone, Serialize)]
pub struct ObservableCheck {
    pub name: String,
    pub unit: String,
    pub tolerance: TrajectoryTolerance,
    /// The reference's horizon at the default floor.
    pub horizon: Horizon,
    /// Inside the horizon; `None` when the reference parts at its first
    /// checkpoint.
    pub inside: Option<TrajectoryVerdict>,
    pub whole: TrajectoryVerdict,
}

/// The candidate's frame at one checkpoint against the reference's band.
#[derive(Debug, Clone, Serialize)]
pub struct FrameCheck {
    pub offset: u64,
    /// Inside the reference's scene horizon, so gated.
    pub inside: bool,
    pub verdict: FrameVerdict,
}

/// A candidate replay against a reference group.
#[derive(Debug, Clone, Serialize)]
pub struct CandidateCheck {
    pub label: String,
    pub script: String,
    pub against: String,
    pub observables: Vec<ObservableCheck>,
    pub frames: Vec<FrameCheck>,
}

impl CandidateCheck {
    /// The observables that fail inside the reference's horizon.
    pub fn trajectory_failures(&self) -> Vec<&ObservableCheck> {
        self.observables
            .iter()
            .filter(|o| o.inside.as_ref().is_some_and(|v| !v.passed()))
            .collect()
    }

    /// The checkpoints inside the scene's horizon whose frame fails.
    pub fn frame_failures(&self) -> Vec<u64> {
        self.frames
            .iter()
            .filter(|f| f.inside && !f.verdict.passed())
            .map(|f| f.offset)
            .collect()
    }

    /// The checkpoints past the scene's horizon whose frame fails: reported.
    pub fn frame_failures_past(&self) -> Vec<u64> {
        self.frames
            .iter()
            .filter(|f| !f.inside && !f.verdict.passed())
            .map(|f| f.offset)
            .collect()
    }

    /// Fails when any trajectory fails inside its horizon or any frame inside
    /// the scene's.
    pub fn passed(&self) -> bool {
        self.trajectory_failures().is_empty() && self.frame_failures().is_empty()
    }

    pub fn write_text(&self, out: &mut String) {
        let _ = writeln!(
            out,
            "== {} against {}: {} ==",
            self.label,
            self.against,
            if self.passed() { "passed" } else { "FAILED" }
        );
        for o in &self.observables {
            let inside = o
                .inside
                .as_ref()
                .map_or("nothing inside".into(), |v| verdict_text(v, &o.unit));
            let _ = writeln!(
                out,
                "{}: inside the horizon ({}) {}; whole run {}",
                o.name,
                h_text(o.horizon.last_agreeing),
                inside,
                verdict_text(&o.whole, &o.unit)
            );
        }
        let (failed, past) = (self.frame_failures(), self.frame_failures_past());
        let worst = self.frames.iter().filter(|f| f.inside).max_by(|a, b| {
            (a.verdict.worst - a.verdict.limit).total_cmp(&(b.verdict.worst - b.verdict.limit))
        });
        let _ = writeln!(
            out,
            "frames inside the scene's horizon ({}), {} checkpoints: {} failed{}{}; past it, \
             reported: {} of {} failed{}\n",
            h_text(
                self.frames
                    .iter()
                    .filter(|f| f.inside)
                    .map(|f| f.offset)
                    .max()
            ),
            self.frames.iter().filter(|f| f.inside).count(),
            failed.len(),
            if failed.is_empty() {
                String::new()
            } else {
                format!(" (first at {})", failed[0])
            },
            worst.map_or(String::new(), |w| format!(
                "; closest call at {}: worst mean {:.5} against a limit of {:.5}",
                w.offset, w.verdict.worst, w.verdict.limit
            )),
            past.len(),
            self.frames.iter().filter(|f| !f.inside).count(),
            if past.is_empty() {
                String::new()
            } else {
                format!(" ({past:?})")
            }
        );
    }
}

fn verdict_text(v: &TrajectoryVerdict, unit: &str) -> String {
    format!(
        "{} (worst {:+.4e} {unit} at {} c{}, {} of {} outside{})",
        if v.passed() { "passed" } else { "FAILED" },
        v.worst.by,
        v.worst.offset,
        v.worst.component,
        v.outside,
        v.checked,
        v.first_outside
            .map_or(String::new(), |f| format!(", first at {f}"))
    )
}

/// `candidate` (its files under `candidate_dir`) against `reference`, on every
/// checkpoint the reference has.
pub fn check(
    reference: &Reference,
    candidate: &ReplayRecord,
    candidate_dir: &Path,
) -> Result<CandidateCheck> {
    let mut observables = Vec::new();
    for (spec, o) in &reference.observables {
        let tolerance = o.tolerance.expect("compared observables only");
        let horizon = o.horizon.expect("a compared observable has a horizon");
        let trace = candidate_trace(spec, &o.traces[0], candidate)
            .with_context(|| format!("{}'s {}", candidate.label, spec.name))?;
        let whole = o.envelope.check(&trace, &tolerance)?;
        let inside = horizon
            .last_agreeing
            .map(|last| o.envelope.until(last).check(&trace, &tolerance))
            .transpose()?;
        observables.push(ObservableCheck {
            name: o.name.clone(),
            unit: o.unit.clone(),
            tolerance,
            horizon,
            inside,
            whole,
        });
    }
    let mut frames_checked = Vec::new();
    for (&offset, (stack, band)) in &reference.bands {
        let Some(frame) = frame_at(candidate_dir, candidate, offset)? else {
            continue;
        };
        let means = frames::candidate_means(&frame, stack, frames::reference_pixels_per_degree())?;
        frames_checked.push(FrameCheck {
            offset,
            inside: reference.scene_horizon.is_some_and(|last| offset <= last),
            verdict: band.check(&means, tolerance::FRAME_FLOOR)?,
        });
    }
    Ok(CandidateCheck {
        label: candidate.label.clone(),
        script: candidate.script.clone(),
        against: format!(
            "{} set {} ({} replays)",
            reference.script,
            reference.set,
            reference.labels.len()
        ),
        observables,
        frames: frames_checked,
    })
}

/// The candidate's values of `spec` at `first`'s checkpoints (the reference's
/// first replay's trace), angles unwrapped against it as the reference's were.
fn candidate_trace(spec: &Spec, first: &Trace, candidate: &ReplayRecord) -> Result<Trace> {
    let f0: &Observation = candidate.at(0).context("no observation at F0")?;
    let samples = first
        .samples()
        .iter()
        .map(|reference| {
            let o = candidate
                .at(reference.offset)
                .with_context(|| format!("no observation at F0 + {}", reference.offset))?;
            let mut values = (spec.read)(o, f0)
                .with_context(|| format!("no value at F0 + {}", reference.offset))?;
            if spec.angular {
                for (v, r) in values.iter_mut().zip(&reference.values) {
                    *v = r + angle_from(*r, *v);
                }
            }
            Ok(Sample::new(reference.offset, values))
        })
        .collect::<Result<Vec<_>>>()?;
    Ok(Trace::new(samples)?)
}

/// For the unit tests: an envelope's check, so a test can compare with
/// [`check`]'s.
#[cfg(test)]
fn envelope_of(reference: &Reference, name: &str) -> trace::Envelope {
    reference
        .observables
        .iter()
        .find(|(s, _)| s.name == name)
        .map(|(_, o)| o.envelope.clone())
        .expect("a compared observable")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::actors::Counts;
    use crate::divergence::{CheckpointShot, EndShot, Host, Pose, RngObservation};

    fn obs(offset: u64, z: f32, heading: f32, drawn: u64, state: &str) -> Observation {
        Observation {
            offset,
            flip: offset + 2,
            game_frame: 6_000 + offset as u32,
            frame_delta: 1.0 / 60.0,
            counts: Counts {
                listed: 166,
                actors: 166,
                skinned: 35,
                players: 1,
            },
            hero: Some(Pose {
                position: [10.0, 43.0, z],
                rotation: [0.0, heading, 0.0],
                velocity: Some([0.0; 3]),
            }),
            camera: Some(Pose {
                position: [6.5, 52.0, z - 14.0],
                rotation: [-23.0, 15.0, 0.0],
                velocity: None,
            }),
            rng: RngObservation {
                instances: 1,
                mti: 1,
                seed: Some(10554),
                drawn: Some(116_862 + drawn),
                anim_steps: Some(17),
                fmod: Some(0),
                state_sha256: state.to_owned(),
            },
            read_ms: 4.0,
        }
    }

    fn record(script: &str, replay: u32, observations: Vec<Observation>) -> ReplayRecord {
        ReplayRecord {
            label: format!("{script}-C-{replay:02}"),
            script: script.into(),
            set: Set::C,
            replay,
            identity: "oracle".into(),
            state: "s".into(),
            state_sha256: "s".into(),
            figure_sha256: "f".into(),
            before: Host {
                load_average: Some([1.0, 1.0, 1.0]),
                uptime: String::new(),
                screen_locked: Some(true),
            },
            after: Host {
                load_average: None,
                uptime: String::new(),
                screen_locked: None,
            },
            boot_secs: 5.0,
            f0: 2,
            base: 3,
            run_secs: 30.0,
            observations,
            shot: None,
            shots: vec![],
            saved: None,
            save_refusals: vec![],
            save_error: None,
            silent: true,
            fatal: vec![],
            wall_secs: 60.0,
            shutdown: "Graceful".into(),
        }
    }

    /// A walk observed every 60 flips to 240: at rest to 60, then 0.109 m a
    /// flip forward on z (a frame of Eruptor's walk), replay r `jitter(r)`
    /// off from 120 on and parted by a metre at 240 for odd replays.
    fn walk(script: &str, r: u32, late: u64, heading: f32) -> ReplayRecord {
        let jitter = |offset: u64| {
            if offset < 120 {
                0.0
            } else if offset < 240 {
                0.002 * r as f32
            } else {
                (r % 2) as f32
            }
        };
        let observations = (0..=4u64)
            .map(|k| {
                let offset = k * 60;
                let walked = offset.saturating_sub(60 + late) as f32 * 0.109;
                let state = if offset == 0 {
                    "s0".to_owned()
                } else {
                    format!("s{offset}-{r}")
                };
                obs(
                    offset,
                    -93.0 + walked + jitter(offset),
                    heading,
                    offset * 18 + u64::from(r) * offset / 60,
                    &state,
                )
            })
            .collect();
        record(script, r, observations)
    }

    fn reference_of(records: Vec<ReplayRecord>) -> (tempfile::TempDir, Reference) {
        let tmp = tempfile::tempdir().unwrap();
        for r in &records {
            r.write(&{
                let d = tmp.path().join(&r.label);
                std::fs::create_dir_all(&d).unwrap();
                d
            })
            .unwrap();
        }
        let reference = Reference::load(tmp.path(), "walk", Set::C).unwrap();
        (tmp, reference)
    }

    #[test]
    fn leave_one_out_passes_inside_each_horizon_and_reports_past_it() {
        let records: Vec<ReplayRecord> = (0..10).map(|r| walk("walk", r, 0, 15.0)).collect();
        let groups = leave_one_out(&records, None).unwrap();
        assert_eq!(groups.len(), 1);
        let g = &groups[0];
        assert!(g.passed(), "{g:?}");
        let hero = g
            .observables
            .iter()
            .find(|o| o.name == "hero position")
            .unwrap();
        // 0.018 m of jitter at 120 and 180 sits under the 0.1 m floor; the
        // metre at 240 parts them.
        assert_eq!(
            hero.horizon,
            Some(Horizon {
                last_agreeing: Some(180),
                parted_at: Some(240)
            })
        );
        // Without any one replay the other nine still hold both branches, so
        // each is judged to 180 too.
        assert_eq!(hero.others, vec![Some(180); 10]);
        assert_eq!(hero.checked, 10 * 4 * 3);
        // Over the whole run nobody strays: two branches of five each, every
        // replay inside the other nine's range.
        assert!(hero.past.is_empty(), "{:?}", hero.past);
        // The MT parts at 60 (a draw apart a replay), so only F0 is inside;
        // past it k = 1 keeps every replay within one spread.
        let draws = g
            .observables
            .iter()
            .find(|o| o.name == "MT draws since F0")
            .unwrap();
        assert_eq!(draws.horizon.unwrap().last_agreeing, Some(0));
        assert!(draws.past.is_empty());
        assert_eq!(g.rng_horizon.last_agreeing, Some(0));
        assert!(g.rng_failed.is_empty());
        let delta = g
            .observables
            .iter()
            .find(|o| o.name == "frame delta")
            .unwrap();
        assert!(delta.tolerance.is_none() && delta.checked == 0);
        let mut text = String::new();
        g.write_text(&mut text);
        assert!(text.contains("PASSED"), "{text}");
    }

    #[test]
    fn leave_one_out_fails_a_replay_that_strays_alone_inside_the_others_horizon() {
        // Nine replays walk alike to the end; the tenth alone takes the other
        // branch at 240 and, from 120 on, holds another RNG state. Horizons
        // over all ten end at 180 (positions) and 60 (the RNG) and would have
        // passed it by construction; the nine's run to 240 on both, and the
        // tenth fails both inside them.
        let mut records: Vec<ReplayRecord> = (0..10)
            .map(|r| {
                let mut rec = walk("walk", 0, 0, 15.0);
                rec.label = format!("walk-C-{r:02}");
                rec.replay = r;
                rec
            })
            .collect();
        let lone = &mut records[9];
        lone.observations[4].hero.as_mut().unwrap().position[2] += 1.0;
        for o in &mut lone.observations[2..] {
            o.rng.state_sha256 = format!("other-{}", o.offset);
        }
        let groups = leave_one_out(&records, None).unwrap();
        let g = &groups[0];
        assert!(!g.passed());
        let hero = g
            .observables
            .iter()
            .find(|o| o.name == "hero position")
            .unwrap();
        assert_eq!(hero.horizon.unwrap().last_agreeing, Some(180));
        assert_eq!(&hero.others[..9], &[Some(180); 9]);
        assert_eq!(hero.others[9], Some(240));
        let failed: Vec<&str> = hero.failed.iter().map(|f| f.label.as_str()).collect();
        assert_eq!(failed, ["walk-C-09"]);
        assert_eq!(hero.failed[0].first_outside, Some(240));
        assert_eq!(g.rng_failed, ["walk-C-09"]);
        let mut text = String::new();
        g.write_text(&mut text);
        assert!(
            text.contains("FAILED") && text.contains("the others' 180-240"),
            "{text}"
        );
    }

    #[test]
    fn a_late_walk_fails_inside_the_horizon_and_an_on_time_one_passes() {
        let (_tmp, reference) = reference_of((0..10).map(|r| walk("walk", r, 0, 15.0)).collect());
        let dir = tempfile::tempdir().unwrap();
        let on_time = check(&reference, &walk("walk", 10, 0, 15.0), dir.path()).unwrap();
        assert!(on_time.passed(), "{on_time:?}");
        // Two flips late is 0.218 m short at 120, past the 0.1 m floor.
        let late = check(&reference, &walk("walk_late", 0, 2, 15.0), dir.path()).unwrap();
        assert!(!late.passed());
        let names: Vec<&str> = late
            .trajectory_failures()
            .iter()
            .map(|o| o.name.as_str())
            .collect();
        assert_eq!(names, ["hero position", "camera position"]);
        let hero = &late.observables[0];
        assert_eq!(hero.inside.as_ref().unwrap().first_outside, Some(120));
        // One flip late is 0.109 m short: from the top of the band (replay 9's
        // 0.018 of jitter) that's still inside the floor.
        let one = check(&reference, &walk("walk_late", 9, 1, 15.0), dir.path()).unwrap();
        assert!(one.passed(), "{:?}", one.trajectory_failures());
        // A heading 0.001 deg off fails the angle floor of 1e-4.
        let turned = check(&reference, &walk("walk_turned", 0, 0, 15.001), dir.path()).unwrap();
        let names: Vec<&str> = turned
            .trajectory_failures()
            .iter()
            .map(|o| o.name.as_str())
            .collect();
        assert_eq!(names, ["hero heading"]);
        // The envelope check is the one the reference's own observable makes.
        let env = envelope_of(&reference, "hero position");
        assert_eq!(env.replays(), 10);
        let mut text = String::new();
        late.write_text(&mut text);
        assert!(text.contains("FAILED"), "{text}");
    }

    #[test]
    fn a_reference_needs_three_replays_and_candidates_need_its_checkpoints() {
        let tmp = tempfile::tempdir().unwrap();
        for r in 0..2 {
            let rec = walk("walk", r, 0, 15.0);
            let d = tmp.path().join(&rec.label);
            std::fs::create_dir_all(&d).unwrap();
            rec.write(&d).unwrap();
        }
        assert!(Reference::load(tmp.path(), "walk", Set::C).is_err());
        let (_tmp, reference) = reference_of((0..5).map(|r| walk("walk", r, 0, 15.0)).collect());
        let mut short = walk("walk", 9, 0, 15.0);
        short.observations.pop();
        let err = check(&reference, &short, tmp.path()).unwrap_err();
        assert!(format!("{err:#}").contains("F0 + 240"), "{err:#}");
    }

    #[test]
    fn frames_compare_at_every_checkpoint_with_shots() {
        // Shots of three synthetic scenes: replays alike up to a little noise,
        // the candidate's frame at 120 shifted.
        let tmp = tempfile::tempdir().unwrap();
        let png = |dir: &Path, name: &str, shift: u32, noise: u32| {
            let img = image::RgbImage::from_fn(64, 48, |x, y| {
                let v = ((x + shift) * 4 + y * 3 + (x * y + noise) % 3) as u8;
                image::Rgb([v, v.wrapping_mul(3), 255 - v])
            });
            img.save(dir.join(name)).unwrap();
        };
        // Shots at 0, 60 and 120, inside the scene's horizon (180), and at 240,
        // past it.
        let shots = |dir: &Path, r: u32, shifted_at: u64| -> Vec<CheckpointShot> {
            [0u64, 60, 120, 240]
                .iter()
                .map(|&offset| {
                    let name = format!("shot-{offset}.png");
                    let shift = if offset == shifted_at { 9 } else { 0 };
                    png(dir, &name, shift, r);
                    CheckpointShot {
                        offset,
                        shot: EndShot {
                            file: name,
                            flip: offset + 2,
                            pixels_sha256: String::new(),
                            lit_share: 1.0,
                        },
                    }
                })
                .collect()
        };
        for r in 0..5 {
            let mut rec = walk("walk", r, 0, 15.0);
            let d = tmp.path().join(&rec.label);
            std::fs::create_dir_all(&d).unwrap();
            rec.shots = shots(&d, r, u64::MAX);
            rec.write(&d).unwrap();
        }
        let reference = Reference::load(tmp.path(), "walk", Set::C).unwrap();
        assert_eq!(reference.frame_offsets(), [0, 60, 120, 240]);
        assert_eq!(reference.scene_horizon, Some(180));
        let cand_dir = tempfile::tempdir().unwrap();
        let candidate = |name: &str, shifted_at: u64| {
            let mut rec = walk(name, 0, 0, 15.0);
            let d = cand_dir.path().join(&rec.label);
            std::fs::create_dir_all(&d).unwrap();
            rec.shots = shots(&d, 7, shifted_at);
            check(&reference, &rec, cand_dir.path()).unwrap()
        };
        // Moved at 120, inside the horizon: fails.
        let inside = candidate("walk_inside", 120);
        assert_eq!(inside.frames.len(), 4);
        assert_eq!(inside.frame_failures(), [120]);
        assert!(!inside.passed());
        // Moved at 240, past it: reported, and the candidate passes.
        let past = candidate("walk_past", 240);
        assert!(past.frame_failures().is_empty());
        assert_eq!(past.frame_failures_past(), [240]);
        assert!(past.passed());
        let mut text = String::new();
        past.write_text(&mut text);
        assert!(text.contains("1 of 1 failed ([240])"), "{text}");
        // The reference's leave-one-out gates the three shots inside the
        // horizon, and each needs a floor of its own.
        let records = crate::divergence::load_records(tmp.path()).unwrap();
        let groups = leave_one_out(&records, Some(tmp.path())).unwrap();
        assert_eq!(groups[0].scene_horizon, Some(180));
        let inside: Vec<bool> = groups[0].frames.iter().map(|f| f.inside).collect();
        assert_eq!(inside, [true, true, true, false]);
        assert!(groups[0].frames.iter().all(|f| f.needed_floor >= 0.0));
    }
}
