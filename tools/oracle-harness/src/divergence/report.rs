//! A measurement's records turned into its numbers (openspec AF.11): per
//! observable the band the replays give each component at each checkpoint and
//! the usable horizon read at a ladder of floors (AF.10's [`Envelope`] and
//! [`Horizon`]); the RNG's distances; every actor all replays have, matched by
//! its corpus; FLIP between the end shots; set A against set B at the end
//! flip; and how far each replay's end sits from its group's median against
//! the host's load. The floors are a ladder to read horizons off; the
//! tolerances are AF.12's (`guac_oracle::tolerance`), and each observable's
//! horizon at its default floor is reported beside the ladder.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;
use std::path::Path;

use anyhow::{Context, Result};
use guac_oracle::frames::{self, FlipMatrix, Frame};
// The rank-sum and its ranks live in guac-oracle's `stats` (AH.6's
// distribution tests read the same one); observing.rs takes it from here.
pub(super) use guac_oracle::stats::rank_sum_z;
use guac_oracle::stats::ranks;
use guac_oracle::tolerance;
use guac_oracle::trace::{Envelope, Horizon, Sample, Trace, TrajectoryTolerance};
use serde::Serialize;

use super::observing::Choice;
use super::{Observation, ReplayRecord, Set, actors_file};
use crate::actors::{ActorFrame, angle_from};

const POSITION_FLOORS: &[f64] = &[0.0, 1e-4, 1e-3, 1e-2, 1e-1];
const ANGLE_FLOORS: &[f64] = &[0.0, 1e-3, 1e-2, 1e-1, 1.0];
const COUNT_FLOORS: &[f64] = &[0.0, 1.0, 2.0];
const DRAW_FLOORS: &[f64] = &[0.0, 100.0, 1_000.0];
const DELTA_FLOORS: &[f64] = &[0.0, 1e-4, 1e-3];
const WORLD_FLOORS: &[f64] = &[0.0, 1e-3, 1e-2, 1e-1];

pub(super) type Read = fn(&Observation, &Observation) -> Option<Vec<f64>>;

/// One observable: how it's read off an observation (and F0's, for counts
/// since F0), the floors its horizon is read at and the default tolerance a
/// comparison takes (`None`: not compared, as host time isn't).
pub(super) struct Spec {
    pub(super) name: &'static str,
    pub(super) unit: &'static str,
    pub(super) components: &'static [&'static str],
    /// Degrees: unwrapped against the first replay's value before the spread.
    pub(super) angular: bool,
    pub(super) floors: &'static [f64],
    pub(super) tolerance: Option<TrajectoryTolerance>,
    pub(super) read: Read,
}

fn f64s(v: &[f32]) -> Vec<f64> {
    v.iter().map(|&x| f64::from(x)).collect()
}

fn since(now: Option<u64>, then: Option<u64>) -> Option<Vec<f64>> {
    Some(vec![now? as f64 - then? as f64])
}

pub(super) fn specs() -> Vec<Spec> {
    vec![
        Spec {
            name: "hero position",
            unit: "m",
            components: &["x", "y", "z"],
            angular: false,
            floors: POSITION_FLOORS,
            tolerance: Some(tolerance::POSITION),
            read: |o, _| o.hero.as_ref().map(|h| f64s(&h.position)),
        },
        Spec {
            name: "hero heading",
            unit: "deg",
            components: &["heading"],
            angular: true,
            floors: ANGLE_FLOORS,
            tolerance: Some(tolerance::ANGLE),
            read: |o, _| o.hero.as_ref().map(|h| vec![f64::from(h.rotation[1])]),
        },
        Spec {
            name: "camera position",
            unit: "m",
            components: &["x", "y", "z"],
            angular: false,
            floors: POSITION_FLOORS,
            tolerance: Some(tolerance::POSITION),
            read: |o, _| o.camera.as_ref().map(|c| f64s(&c.position)),
        },
        Spec {
            name: "camera angles",
            unit: "deg",
            components: &["tilt", "heading"],
            angular: true,
            floors: ANGLE_FLOORS,
            tolerance: Some(tolerance::ANGLE),
            read: |o, _| o.camera.as_ref().map(|c| f64s(&c.rotation[..2])),
        },
        Spec {
            name: "actors listed",
            unit: "actors",
            components: &["listed"],
            angular: false,
            floors: COUNT_FLOORS,
            tolerance: Some(tolerance::ACTORS),
            read: |o, _| Some(vec![f64::from(o.counts.listed)]),
        },
        Spec {
            name: "skinned actors",
            unit: "actors",
            components: &["skinned"],
            angular: false,
            floors: COUNT_FLOORS,
            tolerance: Some(tolerance::ACTORS),
            read: |o, _| Some(vec![o.counts.skinned as f64]),
        },
        Spec {
            name: "game frames since F0",
            unit: "frames",
            components: &["frames"],
            angular: false,
            floors: COUNT_FLOORS,
            tolerance: Some(tolerance::COUNT),
            read: |o, f0| since(Some(o.game_frame.into()), Some(f0.game_frame.into())),
        },
        Spec {
            name: "frame delta",
            unit: "s",
            components: &["dt"],
            angular: false,
            floors: DELTA_FLOORS,
            tolerance: None,
            read: |o, _| Some(vec![f64::from(o.frame_delta)]),
        },
        Spec {
            name: "MT draws since F0",
            unit: "draws",
            components: &["draws"],
            angular: false,
            floors: DRAW_FLOORS,
            tolerance: Some(tolerance::COUNT),
            read: |o, f0| since(o.rng.drawn, f0.rng.drawn),
        },
        Spec {
            name: "animation LCG steps since F0",
            unit: "steps",
            components: &["steps"],
            angular: false,
            floors: COUNT_FLOORS,
            tolerance: Some(tolerance::ANIMATION_STEPS),
            read: |o, f0| since(o.rng.anim_steps, f0.rng.anim_steps),
        },
    ]
}

/// The replays' values at one checkpoint: what a spread or a median is taken of.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct Band {
    pub min: f64,
    pub max: f64,
    pub median: f64,
}

impl Band {
    pub fn of(values: &[f64]) -> Option<Self> {
        let mut v = values.to_vec();
        v.sort_by(f64::total_cmp);
        let n = v.len();
        (n > 0).then(|| Band {
            min: v[0],
            max: v[n - 1],
            median: if n % 2 == 1 {
                v[n / 2]
            } else {
                (v[n / 2 - 1] + v[n / 2]) / 2.0
            },
        })
    }

    pub fn spread(&self) -> f64 {
        self.max - self.min
    }
}

/// One observable over one group's replays.
#[derive(Debug, Clone, Serialize)]
pub struct Observable {
    pub name: String,
    pub unit: String,
    pub components: Vec<String>,
    /// The default tolerance a comparison takes (`None`: host time, not
    /// compared), and the usable horizon at its floor.
    pub tolerance: Option<TrajectoryTolerance>,
    pub horizon: Option<Horizon>,
    /// Each replay's trace, in the group's label order.
    pub traces: Vec<Trace>,
    pub envelope: Envelope,
    /// (replay, offset) pairs with no value (no single hero read, say); those
    /// checkpoints are left out of the envelope for every replay.
    pub missing: Vec<(String, u64)>,
    /// The usable horizon at each floor of the ladder.
    pub horizons: Vec<(f64, Horizon)>,
}

impl Observable {
    /// The largest component spread at each checkpoint.
    pub fn spreads(&self) -> Vec<(u64, f64)> {
        self.envelope
            .checkpoints()
            .iter()
            .map(|c| {
                (
                    c.offset,
                    c.bands.iter().map(|b| b.spread()).fold(0.0, f64::max),
                )
            })
            .collect()
    }
}

/// Whether a group's replays hold one MT state at a checkpoint, and how many
/// draws apart they are when they don't.
#[derive(Debug, Clone, Serialize)]
pub struct RngCheckpoint {
    pub offset: u64,
    /// Every replay's MT state the same, word for word.
    pub identical: bool,
    /// How many different MT states the replays hold.
    pub states: usize,
    /// MT numbers drawn since F0; its spread is the largest distance between
    /// two replays, since all draw from one stream.
    pub drawn: Option<Band>,
    pub anim_steps: Option<Band>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Rng {
    pub seeds: BTreeSet<u32>,
    pub instances: BTreeSet<i32>,
    pub checkpoints: Vec<RngCheckpoint>,
    /// Identical MT states and animation LCG steps.
    pub horizon: Horizon,
}

/// Every actor all replays have at one checkpoint (same corpus, same name).
#[derive(Debug, Clone, Serialize)]
pub struct WorldCheckpoint {
    pub offset: u64,
    pub common: usize,
    /// Corpora some replays have and others don't (or under another name).
    pub partial: usize,
    /// The largest position spread of any common actor, and whose.
    pub max_spread: f64,
    pub max_name: Option<String>,
    pub median_spread: f64,
    /// Common actors whose position spread is over 1 mm.
    pub over_1mm: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct World {
    pub checkpoints: Vec<WorldCheckpoint>,
    /// Over `max_spread`, and a partial actor parts the replays too.
    pub horizons: Vec<(f64, Horizon)>,
}

/// FLIP between end shots, at the reference's viewing distance.
#[derive(Debug, Clone, Serialize)]
pub struct FlipSummary {
    pub pairs: Vec<(String, String, f64)>,
    pub mean: Option<Band>,
    /// Pairs whose pixels are equal, byte for byte.
    pub identical: usize,
}

impl FlipSummary {
    fn of(pairs: Vec<(String, String, f64)>, identical: usize) -> Self {
        let means: Vec<f64> = pairs.iter().map(|p| p.2).collect();
        Self {
            mean: Band::of(&means),
            pairs,
            identical,
        }
    }
}

/// One replay's end against its group: the inputs to [`LoadLink`].
#[derive(Debug, Clone, Serialize)]
pub struct ReplayEnd {
    pub label: String,
    pub load_1m: Option<f64>,
    pub run_secs: f64,
    /// Metres from the group's median end position.
    pub hero_off: Option<f64>,
    /// MT draws from the group's median at the end.
    pub draws_off: Option<f64>,
    /// The replay's mean FLIP against the group's other end shots.
    pub flip_mean: Option<f64>,
    /// Fatal guest errors RPCS3 logged (the end state's reboot included).
    pub fatal: usize,
}

/// Spearman's rho of the load (and of the run's wall time) against each
/// replay's distance from its group's median, over one group.
#[derive(Debug, Clone, Serialize)]
pub struct LoadLink {
    pub load: Option<Band>,
    pub run_secs: Option<Band>,
    pub load_vs_run: Option<f64>,
    pub load_vs_hero: Option<f64>,
    pub load_vs_draws: Option<f64>,
    pub load_vs_flip: Option<f64>,
    pub run_vs_hero: Option<f64>,
    pub run_vs_draws: Option<f64>,
    pub run_vs_flip: Option<f64>,
}

/// One script under one set.
#[derive(Debug, Clone, Serialize)]
pub struct Group {
    pub script: String,
    pub set: Set,
    pub labels: Vec<String>,
    pub f0: BTreeSet<u64>,
    pub observables: Vec<Observable>,
    pub rng: Rng,
    pub world: Option<World>,
    pub flip: Option<FlipSummary>,
    pub ends: Vec<ReplayEnd>,
    pub load: LoadLink,
}

/// One observable's end-flip numbers in set A and set B.
#[derive(Debug, Clone, Serialize)]
pub struct AcrossObservable {
    pub name: String,
    pub a: Vec<Band>,
    pub b: Vec<Band>,
    /// B replays inside A's [min, max] on every component, and the reverse.
    pub b_inside_a: usize,
    pub a_inside_b: usize,
    /// The rank-sum (Mann-Whitney) z of B against A, per component.
    pub z: Vec<Option<f64>>,
}

/// Set A against set B at the end flip, for one script: does pausing to
/// observe move the run?
#[derive(Debug, Clone, Serialize)]
pub struct AcrossSets {
    pub script: String,
    pub end: u64,
    pub observables: Vec<AcrossObservable>,
    /// FLIP of every A end shot against every B one.
    pub flip: Option<FlipSummary>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Report {
    pub groups: Vec<Group>,
    pub across: Vec<AcrossSets>,
    /// Set W or O against set A, per script with both, judged against its set
    /// B by the rule asked for (AG.1, AG.34, AH.40).
    pub observing: Vec<super::observing::Discriminators>,
}

impl Report {
    /// Groups `records` by script and set (a group needs two replays). `dir`,
    /// the output dir, is where the actor frames and end shots are read from;
    /// without it the world and FLIP are left out.
    pub fn build(records: &[ReplayRecord], dir: Option<&Path>) -> Result<Self> {
        Self::build_with(records, dir, &[], Choice::default())
    }

    /// [`Report::build`], with `paused`: another measurement's replays (AF.11's)
    /// whose set B of a script is the paused group a script's set W or O is
    /// judged against (AG.34, AH.40) where `records` hold no set B of it; and
    /// `choice`, the rule judging them.
    pub fn build_with(
        records: &[ReplayRecord],
        dir: Option<&Path>,
        paused: &[ReplayRecord],
        choice: Choice,
    ) -> Result<Self> {
        let mut by: BTreeMap<(String, Set), Vec<&ReplayRecord>> = BTreeMap::new();
        for r in records {
            by.entry((r.script.clone(), r.set)).or_default().push(r);
        }
        let mut paused_by: BTreeMap<String, Vec<&ReplayRecord>> = BTreeMap::new();
        for r in paused.iter().filter(|r| r.set == Set::B) {
            paused_by.entry(r.script.clone()).or_default().push(r);
        }
        let by_set = by.clone();
        let mut groups = Vec::new();
        let mut frames: BTreeMap<String, Frame> = BTreeMap::new();
        for ((script, set), mut replays) in by {
            if replays.len() < 2 {
                continue;
            }
            replays.sort_by(|a, b| a.label.cmp(&b.label));
            if let Some(dir) = dir {
                for r in &replays {
                    if let Some(frame) = end_frame(dir, r)? {
                        frames.insert(r.label.clone(), frame);
                    }
                }
            }
            groups.push(Group::build(script, set, &replays, dir, &frames)?);
        }
        let mut across = Vec::new();
        let scripts: BTreeSet<&str> = groups.iter().map(|g| g.script.as_str()).collect();
        for script in scripts {
            let a = groups
                .iter()
                .find(|g| g.script == script && g.set == Set::A);
            let b = groups
                .iter()
                .find(|g| g.script == script && g.set == Set::B);
            if let (Some(a), Some(b)) = (a, b) {
                across.push(AcrossSets::build(a, b, &frames)?);
            }
        }
        // Set W or O against set A on AF.11's discriminators, at the end flip
        // both share, judged against the script's set B: this measurement's,
        // else `paused`'s.
        let mut observing = Vec::new();
        for ((script, set), replays) in &by_set {
            if !matches!(set, Set::W | Set::O) {
                continue;
            }
            let Some(reference) = by_set.get(&(script.clone(), Set::A)) else {
                continue;
            };
            let paused = by_set
                .get(&(script.clone(), Set::B))
                .or_else(|| paused_by.get(script))
                .map(|rs| (Set::B, rs.as_slice()));
            let both: Vec<&ReplayRecord> = reference.iter().chain(replays).copied().collect();
            if let Some(&end) = common_offsets(&both).last() {
                observing.push(super::observing::Discriminators::build(
                    script,
                    (Set::A, reference),
                    (*set, replays),
                    paused,
                    end,
                    choice,
                ));
            }
        }
        Ok(Self {
            groups,
            across,
            observing,
        })
    }

    /// The whole report as text, for `report.txt` and the terminal.
    pub fn text(&self) -> String {
        let mut out = String::new();
        for g in &self.groups {
            g.write_text(&mut out);
        }
        for a in &self.across {
            a.write_text(&mut out);
        }
        for o in &self.observing {
            o.write_text(&mut out);
        }
        out
    }
}

fn end_frame(dir: &Path, record: &ReplayRecord) -> Result<Option<Frame>> {
    match record.end() {
        Some(end) => frame_at(dir, record, end),
        None => Ok(None),
    }
}

/// The replay's shot at `offset` ([`ReplayRecord::shot_at`]), decoded; `None`
/// when it has none or the file isn't there.
pub(super) fn frame_at(dir: &Path, record: &ReplayRecord, offset: u64) -> Result<Option<Frame>> {
    let Some(shot) = record.shot_at(offset) else {
        return Ok(None);
    };
    let path = dir.join(&record.label).join(&shot.file);
    if !path.is_file() {
        return Ok(None);
    }
    let image = image::open(&path)
        .with_context(|| format!("decoding {}", path.display()))?
        .to_rgba8();
    Ok(Some(Frame::from_rgba8(
        image.width(),
        image.height(),
        image.as_raw(),
    )?))
}

/// The offsets every replay observed, in order.
pub(super) fn common_offsets(replays: &[&ReplayRecord]) -> Vec<u64> {
    let mut sets = replays.iter().map(|r| {
        r.observations
            .iter()
            .map(|o| o.offset)
            .collect::<BTreeSet<_>>()
    });
    let first = sets.next().unwrap_or_default();
    sets.fold(first, |acc, s| acc.intersection(&s).copied().collect())
        .into_iter()
        .collect()
}

pub(super) fn observable(
    spec: &Spec,
    replays: &[&ReplayRecord],
    offsets: &[u64],
) -> Result<Observable> {
    let mut missing = Vec::new();
    // values[offset][replay]
    let mut rows: Vec<(u64, Vec<Vec<f64>>)> = Vec::new();
    for &offset in offsets {
        let mut row = Vec::new();
        for r in replays {
            let value = match (r.at(offset), r.at(0)) {
                (Some(o), Some(f0)) => (spec.read)(o, f0),
                _ => None,
            };
            match value {
                Some(v) => row.push(v),
                None => missing.push((r.label.clone(), offset)),
            }
        }
        if row.len() == replays.len() {
            if spec.angular {
                let reference = row[0].clone();
                for values in &mut row {
                    for (v, r) in values.iter_mut().zip(&reference) {
                        *v = r + angle_from(*r, *v);
                    }
                }
            }
            rows.push((offset, row));
        }
    }
    let traces = (0..replays.len())
        .map(|i| {
            Trace::new(
                rows.iter()
                    .map(|(offset, row)| Sample::new(*offset, row[i].clone()))
                    .collect(),
            )
        })
        .collect::<Result<Vec<_>, _>>()
        .with_context(|| format!("{}: no checkpoint every replay has", spec.name))?;
    let envelope = Envelope::from_traces(&traces)?;
    let horizons = spec
        .floors
        .iter()
        .map(|&floor| Ok((floor, envelope.horizon(floor)?)))
        .collect::<Result<Vec<_>>>()?;
    let horizon = spec
        .tolerance
        .map(|t| envelope.horizon(t.floor))
        .transpose()?;
    Ok(Observable {
        name: spec.name.to_owned(),
        unit: spec.unit.to_owned(),
        components: spec.components.iter().map(|c| (*c).to_owned()).collect(),
        tolerance: spec.tolerance,
        horizon,
        traces,
        envelope,
        missing,
        horizons,
    })
}

fn rng(replays: &[&ReplayRecord], offsets: &[u64]) -> Result<Rng> {
    let mut checkpoints = Vec::new();
    let (mut seeds, mut instances) = (BTreeSet::new(), BTreeSet::new());
    for &offset in offsets {
        let obs: Vec<(&Observation, &Observation)> = replays
            .iter()
            .filter_map(|r| Some((r.at(offset)?, r.at(0)?)))
            .collect();
        let states: BTreeSet<&str> = obs
            .iter()
            .map(|(o, _)| o.rng.state_sha256.as_str())
            .collect();
        let drawn: Option<Vec<f64>> = obs
            .iter()
            .map(|(o, f0)| since(o.rng.drawn, f0.rng.drawn).map(|v| v[0]))
            .collect();
        let anim: Option<Vec<f64>> = obs
            .iter()
            .map(|(o, f0)| since(o.rng.anim_steps, f0.rng.anim_steps).map(|v| v[0]))
            .collect();
        for (o, _) in &obs {
            seeds.extend(o.rng.seed);
            instances.insert(o.rng.instances);
        }
        let anim = anim.as_deref().and_then(Band::of);
        checkpoints.push(RngCheckpoint {
            offset,
            identical: states.len() == 1 && anim.is_none_or(|b| b.spread() == 0.0),
            states: states.len(),
            drawn: drawn.as_deref().and_then(Band::of),
            anim_steps: anim,
        });
    }
    let horizon = Horizon::from_agreement(checkpoints.iter().map(|c| (c.offset, c.identical)))?;
    Ok(Rng {
        seeds,
        instances,
        checkpoints,
        horizon,
    })
}

fn world(replays: &[&ReplayRecord], offsets: &[u64], dir: &Path) -> Result<Option<World>> {
    let mut checkpoints = Vec::new();
    for &offset in offsets {
        let mut frames = Vec::new();
        for r in replays {
            let path = dir.join(&r.label).join(actors_file(offset));
            if !path.is_file() {
                return Ok(None);
            }
            frames.push(ActorFrame::load(&path)?);
        }
        let maps: Vec<BTreeMap<u32, &crate::actors::Actor>> =
            frames.iter().map(ActorFrame::by_corpus).collect();
        let all: BTreeSet<u32> = maps.iter().flat_map(|m| m.keys().copied()).collect();
        let mut spreads: Vec<(f64, Option<String>)> = Vec::new();
        let mut partial = 0;
        for corpus in &all {
            let actors: Option<Vec<_>> = maps.iter().map(|m| m.get(corpus)).collect();
            let same = actors
                .as_ref()
                .is_some_and(|a| a.iter().all(|x| x.name == a[0].name));
            if !same {
                partial += 1;
                continue;
            }
            let actors = actors.expect("checked");
            let spread = (0..3)
                .map(|i| {
                    let v: Vec<f64> = actors.iter().map(|a| f64::from(a.position[i])).collect();
                    Band::of(&v).map_or(0.0, |b| b.spread())
                })
                .fold(0.0, f64::max);
            spreads.push((spread, actors[0].name.clone()));
        }
        let max = spreads
            .iter()
            .max_by(|a, b| a.0.total_cmp(&b.0))
            .cloned()
            .unwrap_or((0.0, None));
        let values: Vec<f64> = spreads.iter().map(|s| s.0).collect();
        checkpoints.push(WorldCheckpoint {
            offset,
            common: spreads.len(),
            partial,
            max_spread: max.0,
            max_name: max.1,
            median_spread: Band::of(&values).map_or(0.0, |b| b.median),
            over_1mm: values.iter().filter(|&&s| s > 1e-3).count(),
        });
    }
    let horizons = WORLD_FLOORS
        .iter()
        .map(|&floor| {
            Ok((
                floor,
                Horizon::from_agreement(
                    checkpoints
                        .iter()
                        .map(|c| (c.offset, c.partial == 0 && c.max_spread <= floor)),
                )?,
            ))
        })
        .collect::<Result<Vec<_>>>()?;
    Ok(Some(World {
        checkpoints,
        horizons,
    }))
}

fn flip_summary(
    left: &[&str],
    right: Option<&[&str]>,
    frames: &BTreeMap<String, Frame>,
    hashes: &BTreeMap<&str, &str>,
) -> Result<Option<FlipSummary>> {
    let ppd = frames::reference_pixels_per_degree();
    let mut pairs = Vec::new();
    let mut identical = 0;
    let mut add = |a: &str, b: &str| -> Result<()> {
        if let (Some(fa), Some(fb)) = (frames.get(a), frames.get(b)) {
            pairs.push((
                a.to_owned(),
                b.to_owned(),
                frames::flip(fa, fb, ppd)?.mean(),
            ));
            if hashes.get(a).is_some_and(|h| hashes.get(b) == Some(h)) {
                identical += 1;
            }
        }
        Ok(())
    };
    match right {
        None => {
            if left.iter().filter(|l| frames.contains_key(**l)).count() < 2 {
                return Ok(None);
            }
            let present: Vec<&str> = left
                .iter()
                .copied()
                .filter(|l| frames.contains_key(*l))
                .collect();
            let stack: Vec<Frame> = present.iter().map(|l| frames[*l].clone()).collect();
            for &(i, j, mean) in FlipMatrix::of(&stack, ppd)?.pairs() {
                pairs.push((present[i].to_owned(), present[j].to_owned(), mean));
                if hashes
                    .get(present[i])
                    .is_some_and(|h| hashes.get(present[j]) == Some(h))
                {
                    identical += 1;
                }
            }
        }
        Some(right) => {
            for a in left {
                for b in right {
                    add(a, b)?;
                }
            }
        }
    }
    Ok((!pairs.is_empty()).then(|| FlipSummary::of(pairs, identical)))
}

impl Group {
    pub(super) fn build(
        script: String,
        set: Set,
        replays: &[&ReplayRecord],
        dir: Option<&Path>,
        frames: &BTreeMap<String, Frame>,
    ) -> Result<Self> {
        let offsets = common_offsets(replays);
        let observables = specs()
            .iter()
            .map(|spec| observable(spec, replays, &offsets))
            .collect::<Result<Vec<_>>>()?;
        let rng = rng(replays, &offsets)?;
        let world = match dir {
            Some(dir) => world(replays, &offsets, dir)?,
            None => None,
        };
        let labels: Vec<&str> = replays.iter().map(|r| r.label.as_str()).collect();
        let hashes = shot_hashes(replays);
        let flip = flip_summary(&labels, None, frames, &hashes)?;
        let ends = ends(replays, &observables, flip.as_ref());
        let load = load_link(&ends);
        Ok(Self {
            script,
            set,
            labels: labels.iter().map(|l| (*l).to_owned()).collect(),
            f0: replays.iter().map(|r| r.f0).collect(),
            observables,
            rng,
            world,
            flip,
            ends,
            load,
        })
    }

    pub fn observable(&self, name: &str) -> Option<&Observable> {
        self.observables.iter().find(|o| o.name == name)
    }

    fn write_text(&self, out: &mut String) {
        let _ = writeln!(
            out,
            "== {}, set {}: {} replays ({} .. {}), F0 {:?} ==",
            self.script,
            self.set,
            self.labels.len(),
            self.labels[0],
            self.labels[self.labels.len() - 1],
            self.f0
        );
        let _ = writeln!(
            out,
            "MT seeds {:?}, live instances {:?}; RNG identical through {} (parted at {})",
            self.rng.seeds,
            self.rng.instances,
            fmt_opt(self.rng.horizon.last_agreeing),
            fmt_opt(self.rng.horizon.parted_at)
        );
        for o in &self.observables {
            let horizons: Vec<String> = o
                .horizons
                .iter()
                .map(|(floor, h)| {
                    format!(
                        "{floor:e}: {}{}",
                        fmt_opt(h.last_agreeing),
                        h.parted_at
                            .map_or(" (held)".into(), |p| format!(" (parts {p})"))
                    )
                })
                .collect();
            let default = match (o.tolerance, o.horizon) {
                (Some(t), Some(h)) => format!(
                    "; at the default floor {:e}: {}{}",
                    t.floor,
                    fmt_opt(h.last_agreeing),
                    h.parted_at
                        .map_or(" (held)".into(), |p| format!(" (parts {p})"))
                ),
                _ => "; not compared".into(),
            };
            let _ = writeln!(
                out,
                "{} ({}): horizon by floor [{}]{default}{}",
                o.name,
                o.unit,
                horizons.join("; "),
                if o.missing.is_empty() {
                    String::new()
                } else {
                    format!("; {} missing values", o.missing.len())
                }
            );
            let spreads: Vec<String> = o
                .spreads()
                .iter()
                .map(|(off, s)| format!("{off}:{s:.3e}"))
                .collect();
            let _ = writeln!(out, "  spread by checkpoint: {}", spreads.join(" "));
            if let Some(end) = o.envelope.checkpoints().last() {
                let bands: Vec<String> = end
                    .bands
                    .iter()
                    .zip(&o.components)
                    .map(|(b, c)| {
                        format!("{c} [{:.5} .. {:.5}] median {:.5}", b.min, b.max, b.median)
                    })
                    .collect();
                let _ = writeln!(out, "  at {}: {}", end.offset, bands.join("; "));
            }
        }
        let drawn: Vec<String> = self
            .rng
            .checkpoints
            .iter()
            .map(|c| {
                format!(
                    "{}:{}{}",
                    c.offset,
                    c.drawn
                        .map_or("?".into(), |b| format!("{}..{}", b.min, b.max)),
                    if c.identical {
                        "=".to_owned()
                    } else {
                        format!("({} states)", c.states)
                    }
                )
            })
            .collect();
        let _ = writeln!(out, "MT draws since F0 by checkpoint: {}", drawn.join(" "));
        if let Some(w) = &self.world {
            let horizons: Vec<String> = w
                .horizons
                .iter()
                .map(|(floor, h)| format!("{floor:e}: {}", fmt_opt(h.last_agreeing)))
                .collect();
            let _ = writeln!(
                out,
                "every actor (by corpus): horizon by floor [{}]",
                horizons.join("; ")
            );
            let rows: Vec<String> = w
                .checkpoints
                .iter()
                .map(|c| {
                    format!(
                        "{}:{}c/{}p max {:.3e} ({}) >1mm {}",
                        c.offset,
                        c.common,
                        c.partial,
                        c.max_spread,
                        c.max_name.as_deref().unwrap_or("-"),
                        c.over_1mm
                    )
                })
                .collect();
            let _ = writeln!(out, "  {}", rows.join(" | "));
        }
        if let Some(f) = &self.flip {
            write_flip(out, "FLIP between end shots", f);
        }
        for e in &self.ends {
            let _ = writeln!(
                out,
                "  {}: load {} run {:.1} s; hero {} m, MT {} draws off the median; FLIP mean {}{}",
                e.label,
                e.load_1m.map_or("?".into(), |l| format!("{l:.2}")),
                e.run_secs,
                e.hero_off.map_or("?".into(), |v| format!("{v:.4}")),
                e.draws_off.map_or("?".into(), |v| format!("{v:.0}")),
                e.flip_mean.map_or("-".into(), |v| format!("{v:.4}")),
                if e.fatal > 0 {
                    format!("; {} fatal error(s) in RPCS3's log", e.fatal)
                } else {
                    String::new()
                },
            );
        }
        let l = &self.load;
        let _ = writeln!(
            out,
            "load {} (1 min), run {} s; Spearman rho: load~run {}, load~hero {}, load~MT {}, \
             load~FLIP {}, run~hero {}, run~MT {}, run~FLIP {}\n",
            fmt_band(l.load),
            fmt_band(l.run_secs),
            fmt_rho(l.load_vs_run),
            fmt_rho(l.load_vs_hero),
            fmt_rho(l.load_vs_draws),
            fmt_rho(l.load_vs_flip),
            fmt_rho(l.run_vs_hero),
            fmt_rho(l.run_vs_draws),
            fmt_rho(l.run_vs_flip),
        );
    }
}

fn shot_hashes<'a>(replays: &[&'a ReplayRecord]) -> BTreeMap<&'a str, &'a str> {
    replays
        .iter()
        .filter_map(|r| Some((r.label.as_str(), r.shot.as_ref()?.pixels_sha256.as_str())))
        .collect()
}

fn ends(
    replays: &[&ReplayRecord],
    observables: &[Observable],
    flip: Option<&FlipSummary>,
) -> Vec<ReplayEnd> {
    let at_end = |name: &str| -> Option<(Vec<Vec<f64>>, Vec<f64>)> {
        let o = observables.iter().find(|o| o.name == name)?;
        let end = o.envelope.checkpoints().last()?;
        let values: Vec<Vec<f64>> = o
            .traces
            .iter()
            .map(|t| t.at(end.offset).map(<[f64]>::to_vec))
            .collect::<Option<_>>()?;
        Some((values, end.bands.iter().map(|b| b.median).collect()))
    };
    let hero = at_end("hero position");
    let draws = at_end("MT draws since F0");
    replays
        .iter()
        .enumerate()
        .map(|(i, r)| ReplayEnd {
            label: r.label.clone(),
            load_1m: r.before.load_average.map(|l| l[0]),
            run_secs: r.run_secs,
            hero_off: hero.as_ref().map(|(v, m)| {
                v[i].iter()
                    .zip(m)
                    .map(|(a, b)| (a - b).powi(2))
                    .sum::<f64>()
                    .sqrt()
            }),
            draws_off: draws.as_ref().map(|(v, m)| (v[i][0] - m[0]).abs()),
            flip_mean: flip.and_then(|f| {
                let mine: Vec<f64> = f
                    .pairs
                    .iter()
                    .filter(|p| p.0 == r.label || p.1 == r.label)
                    .map(|p| p.2)
                    .collect();
                (!mine.is_empty()).then(|| mine.iter().sum::<f64>() / mine.len() as f64)
            }),
            fatal: r.fatal.len(),
        })
        .collect()
}

fn load_link(ends: &[ReplayEnd]) -> LoadLink {
    let pairs = |x: fn(&ReplayEnd) -> Option<f64>, y: fn(&ReplayEnd) -> Option<f64>| {
        let (a, b): (Vec<f64>, Vec<f64>) = ends.iter().filter_map(|e| Some((x(e)?, y(e)?))).unzip();
        spearman(&a, &b)
    };
    let loads: Vec<f64> = ends.iter().filter_map(|e| e.load_1m).collect();
    let runs: Vec<f64> = ends.iter().map(|e| e.run_secs).collect();
    LoadLink {
        load: Band::of(&loads),
        run_secs: Band::of(&runs),
        load_vs_run: pairs(|e| e.load_1m, |e| Some(e.run_secs)),
        load_vs_hero: pairs(|e| e.load_1m, |e| e.hero_off),
        load_vs_draws: pairs(|e| e.load_1m, |e| e.draws_off),
        load_vs_flip: pairs(|e| e.load_1m, |e| e.flip_mean),
        run_vs_hero: pairs(|e| Some(e.run_secs), |e| e.hero_off),
        run_vs_draws: pairs(|e| Some(e.run_secs), |e| e.draws_off),
        run_vs_flip: pairs(|e| Some(e.run_secs), |e| e.flip_mean),
    }
}

impl AcrossSets {
    fn build(a: &Group, b: &Group, frames: &BTreeMap<String, Frame>) -> Result<Self> {
        let end = a
            .observables
            .first()
            .and_then(|o| o.envelope.checkpoints().last())
            .map_or(0, |c| c.offset);
        let mut observables = Vec::new();
        for oa in &a.observables {
            let Some(ob) = b.observable(&oa.name) else {
                continue;
            };
            let values = |o: &Observable| -> Option<Vec<Vec<f64>>> {
                o.traces
                    .iter()
                    .map(|t| t.at(end).map(<[f64]>::to_vec))
                    .collect()
            };
            let (Some(va), Some(vb)) = (values(oa), values(ob)) else {
                continue;
            };
            let comps = oa.components.len();
            let column = |v: &[Vec<f64>], c: usize| v.iter().map(|x| x[c]).collect::<Vec<_>>();
            let bands = |v: &[Vec<f64>]| -> Vec<Band> {
                (0..comps).filter_map(|c| Band::of(&column(v, c))).collect()
            };
            let (ba, bb) = (bands(&va), bands(&vb));
            let inside = |v: &[Vec<f64>], band: &[Band]| {
                v.iter()
                    .filter(|x| x.iter().zip(band).all(|(&x, b)| x >= b.min && x <= b.max))
                    .count()
            };
            observables.push(AcrossObservable {
                name: oa.name.clone(),
                b_inside_a: inside(&vb, &ba),
                a_inside_b: inside(&va, &bb),
                z: (0..comps)
                    .map(|c| rank_sum_z(&column(&va, c), &column(&vb, c)))
                    .collect(),
                a: ba,
                b: bb,
            });
        }
        let la: Vec<&str> = a.labels.iter().map(String::as_str).collect();
        let lb: Vec<&str> = b.labels.iter().map(String::as_str).collect();
        let flip = flip_summary(&la, Some(&lb), frames, &BTreeMap::new())?;
        Ok(Self {
            script: a.script.clone(),
            end,
            observables,
            flip,
        })
    }

    fn write_text(&self, out: &mut String) {
        let _ = writeln!(
            out,
            "== {}: set A against set B at F0 + {} ==",
            self.script, self.end
        );
        for o in &self.observables {
            let fmt = |bands: &[Band]| {
                bands
                    .iter()
                    .map(|b| format!("[{:.5} .. {:.5}] median {:.5}", b.min, b.max, b.median))
                    .collect::<Vec<_>>()
                    .join("; ")
            };
            let z: Vec<String> = o.z.iter().map(|z| fmt_rho(*z)).collect();
            let _ = writeln!(
                out,
                "{}: A {}; B {}; B inside A {}, A inside B {}; rank-sum z {}",
                o.name,
                fmt(&o.a),
                fmt(&o.b),
                o.b_inside_a,
                o.a_inside_b,
                z.join(" ")
            );
        }
        if let Some(f) = &self.flip {
            write_flip(out, "FLIP of A's end shots against B's", f);
        }
        out.push('\n');
    }
}

fn write_flip(out: &mut String, what: &str, f: &FlipSummary) {
    let _ = writeln!(
        out,
        "{what}: {} pairs, mean FLIP {} ({} pixel-identical)",
        f.pairs.len(),
        f.mean.map_or("?".into(), |b| format!(
            "min {:.5} median {:.5} max {:.5}",
            b.min, b.median, b.max
        )),
        f.identical
    );
}

fn fmt_opt(v: Option<u64>) -> String {
    v.map_or("none".into(), |v| v.to_string())
}

fn fmt_rho(v: Option<f64>) -> String {
    v.map_or("n/a".into(), |v| format!("{v:+.2}"))
}

fn fmt_band(b: Option<Band>) -> String {
    b.map_or("?".into(), |b| format!("{:.2}-{:.2}", b.min, b.max))
}

/// Spearman's rank correlation, ties at their mean rank; `None` under three
/// pairs or when either side doesn't vary.
pub fn spearman(x: &[f64], y: &[f64]) -> Option<f64> {
    if x.len() != y.len() || x.len() < 3 {
        return None;
    }
    let (rx, ry) = (ranks(x), ranks(y));
    let n = rx.len() as f64;
    let (mx, my) = (rx.iter().sum::<f64>() / n, ry.iter().sum::<f64>() / n);
    let cov: f64 = rx.iter().zip(&ry).map(|(a, b)| (a - mx) * (b - my)).sum();
    let vx: f64 = rx.iter().map(|a| (a - mx).powi(2)).sum();
    let vy: f64 = ry.iter().map(|b| (b - my).powi(2)).sum();
    (vx > 0.0 && vy > 0.0).then(|| cov / (vx * vy).sqrt())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::actors::Counts;
    use crate::divergence::{Host, Pose, RngObservation};

    fn obs(offset: u64, x: f32, heading: f32, drawn: u64, state: &str) -> Observation {
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
                position: [x, 43.0, -93.0],
                rotation: [0.0, heading, 0.0],
                velocity: Some([0.0; 3]),
            }),
            camera: Some(Pose {
                position: [6.5, 52.0, -107.0],
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

    fn record(set: Set, replay: u32, load: f64, observations: Vec<Observation>) -> ReplayRecord {
        ReplayRecord {
            label: format!("walk-{set}-{replay:02}"),
            script: "walk".into(),
            set,
            replay,
            identity: "oracle".into(),
            state: "BLUS30968_1_1.SAVESTAT.zst".into(),
            state_sha256: "bd1d".into(),
            figure_sha256: "512d".into(),
            before: Host {
                load_average: Some([load, 0.0, 0.0]),
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
            run_secs: 30.0 + load,
            observations,
            shot: None,
            shots: vec![],
            watch: None,
            saved: None,
            save_refusals: vec![],
            save_error: None,
            silent: true,
            fatal: vec![],
            wall_secs: 60.0,
            shutdown: "Graceful".into(),
        }
    }

    /// Ten set B replays identical through offset 120, parting from 180 by
    /// `i` mm in x and `i` draws; heading near +-180 so it wraps.
    fn set_b() -> Vec<ReplayRecord> {
        (0..10)
            .map(|i| {
                let observations = (0..=5)
                    .map(|k| {
                        let offset = k * 60;
                        let part = offset >= 180;
                        let dx = if part { i as f32 * 1e-3 } else { 0.0 };
                        let heading = if part && i % 2 == 1 { -179.9 } else { 179.9 };
                        let drawn = if part { 1_000 + i as u64 } else { offset * 10 };
                        let state = if part {
                            format!("s{i}")
                        } else {
                            format!("t{k}")
                        };
                        obs(offset, 10.0 + dx, heading, drawn, &state)
                    })
                    .collect();
                record(Set::B, i, f64::from(i), observations)
            })
            .collect()
    }

    #[test]
    fn horizons_and_rng_distances_come_off_the_records() {
        let records = set_b();
        let report = Report::build(&records, None).unwrap();
        assert_eq!(report.groups.len(), 1);
        let g = &report.groups[0];
        let hero = g.observable("hero position").unwrap();
        // At floor 0 the replays agree through 120 and part at 180; 1 cm holds.
        assert_eq!(
            hero.horizons[0].1,
            Horizon {
                last_agreeing: Some(120),
                parted_at: Some(180)
            }
        );
        let cm = hero.horizons.iter().find(|(f, _)| *f == 1e-2).unwrap().1;
        assert!(cm.held_throughout());
        let spreads = hero.spreads();
        assert!((spreads[5].1 - 9e-3).abs() < 1e-5, "{spreads:?}");
        // The heading crosses +-180: unwrapped, 0.2 deg apart, not 359.8.
        let heading = g.observable("hero heading").unwrap();
        assert!((heading.spreads()[5].1 - 0.2).abs() < 1e-3);
        // The RNG: identical through 120, then 10 states 9 draws apart at most.
        assert_eq!(g.rng.horizon.last_agreeing, Some(120));
        assert_eq!(g.rng.horizon.parted_at, Some(180));
        let end = g.rng.checkpoints.last().unwrap();
        assert_eq!((end.states, end.identical), (10, false));
        assert_eq!(end.drawn.unwrap().spread(), 9.0);
        assert_eq!(end.drawn.unwrap().min, 1_000.0);
        assert_eq!(g.rng.seeds, BTreeSet::from([10554]));
        // Counts never part.
        assert!(
            g.observable("actors listed").unwrap().horizons[0]
                .1
                .held_throughout()
        );
        // The load rises with the index, and so does the run's wall time; the
        // ends sit 4.5, 3.5 .. 0.5, 0.5 .. 4.5 draws off their median, a V
        // with no rank correlation to it.
        assert_eq!(g.load.load_vs_run, Some(1.0));
        assert_eq!(g.load.load_vs_draws, Some(0.0));
        let ends: Vec<f64> = g.ends.iter().map(|e| e.draws_off.unwrap()).collect();
        assert_eq!(ends[..3], [4.5, 3.5, 2.5]);
        assert!(
            report
                .text()
                .contains("hero position (m): horizon by floor")
        );
    }

    #[test]
    fn set_a_against_set_b_compares_the_end_flip() {
        let mut records = set_b();
        for i in 0..10 {
            // Set A: F0 and the end only, ends shifted 5 cm in x.
            let full = &records[i as usize].observations;
            let mut end = full.last().unwrap().clone();
            end.hero.as_mut().unwrap().position[0] += 0.05;
            records.push(record(Set::A, i, 1.0, vec![full[0].clone(), end]));
        }
        let report = Report::build(&records, None).unwrap();
        assert_eq!(report.groups.len(), 2);
        assert_eq!(report.across.len(), 1);
        let across = &report.across[0];
        assert_eq!(across.end, 300);
        let hero = across
            .observables
            .iter()
            .find(|o| o.name == "hero position")
            .unwrap();
        assert_eq!((hero.b_inside_a, hero.a_inside_b), (0, 0));
        // Every B sits below every A in x: the rank-sum z is as low as 10 and
        // 10 go.
        assert!(hero.z[0].unwrap() < -3.7, "{:?}", hero.z);
        assert_eq!(hero.z[1], None, "y never varies");
        let draws = across
            .observables
            .iter()
            .find(|o| o.name == "MT draws since F0")
            .unwrap();
        assert_eq!((draws.b_inside_a, draws.a_inside_b), (10, 10));
    }

    #[test]
    fn spearman_reads_ranks() {
        let x = [1.0, 2.0, 3.0, 4.0, 5.0];
        assert_eq!(spearman(&x, &[2.0, 4.0, 8.0, 16.0, 32.0]), Some(1.0));
        assert_eq!(spearman(&x, &[5.0, 4.0, 3.0, 2.0, 1.0]), Some(-1.0));
        assert_eq!(spearman(&x, &[1.0; 5]), None);
        assert_eq!(spearman(&x[..2], &x[..2]), None);
        // Ties share a mean rank.
        let tied = spearman(&[1.0, 2.0, 2.0, 3.0], &[1.0, 2.0, 3.0, 4.0]).unwrap();
        assert!((tied - 0.9487).abs() < 1e-4, "{tied}");
    }

    #[test]
    fn a_replay_without_a_hero_drops_that_checkpoint_from_the_hero_observables() {
        let mut records = set_b();
        records[3].observations[2].hero = None;
        let report = Report::build(&records, None).unwrap();
        let hero = report.groups[0].observable("hero position").unwrap();
        assert_eq!(hero.missing, [("walk-B-03".to_owned(), 120)]);
        assert!(hero.envelope.checkpoints().iter().all(|c| c.offset != 120));
        // The camera still has every checkpoint.
        let camera = report.groups[0].observable("camera position").unwrap();
        assert_eq!(camera.envelope.checkpoints().len(), 6);
    }

    #[test]
    fn a_group_of_one_is_left_out() {
        let records = vec![set_b().remove(0)];
        assert!(Report::build(&records, None).unwrap().groups.is_empty());
    }
}
