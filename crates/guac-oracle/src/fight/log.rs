//! Event logs on game time, one per actor (openspec AH.6, design decision
//! 18). AF's [`crate::events`] stamps flips and checks one interleaved
//! prefix; four Chompies whose states interleave differently from replay to
//! replay part that prefix at once while each Chompy does the same thing at
//! the same game time. So here every actor (each Chompy, the hero, each
//! spawned actor by its trace name) keeps its own log, the reference is the
//! prefix that actor's logs agree on across the replays, and each actor is
//! judged on its own.
//!
//! A stamp is game seconds since F0 and the game frame since F0. An event's
//! stamp is judged against the replays' [earliest, latest] widened by its
//! kind's floor (game seconds, calibrated by AF.12's method through
//! [`leave_one_out`] and [`floors_needed`]); which stamps count is the
//! judge's [`Gate`] over the anchor's horizon (the last frame every replay
//! drew alike by, [`super::draws`]) and the kinds a roll times. Decision 18
//! as first written stamps only what no roll times inside the horizon
//! ([`Gate::AllKinds`]); AH.45's (b), the rule since chotchki's call on
//! 2026-10-21, stamps every kind no roll times wherever it falls and lets the
//! horizon gate only the kinds a roll times ([`Gate::RolledKinds`]), so a
//! press moved a frame at anchor A, whose horizon is F0 itself, still fails.
//! An anchor's calibrated floors are named in [`crate::tolerance`]. Where an
//! actor's replays part, the branches are counted (decision 18: a reference
//! holding no replay of a branch claims nothing past it) and nothing after
//! the parting is judged.

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

use crate::MetricError;
use crate::trace::check_tolerance;

/// One thing an actor did (`"state Battle"`, `"bite"`, `"pickup"`), stamped
/// from F0 both ways, so the horizon can be read off the frame.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ActorEvent {
    pub kind: String,
    /// Game seconds since F0.
    pub time: f64,
    /// Game frames since F0.
    pub frame: u32,
}

impl ActorEvent {
    pub fn new(kind: impl Into<String>, time: f64, frame: u32) -> Self {
        Self {
            kind: kind.into(),
            time,
            frame,
        }
    }
}

/// One run's logs by actor name. Each log's frames never go back and its
/// times are finite and never go back (events in one frame keep the order
/// the run logged them in).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "BTreeMap<String, Vec<ActorEvent>>")]
#[serde(into = "BTreeMap<String, Vec<ActorEvent>>")]
pub struct ActorLogs {
    logs: BTreeMap<String, Vec<ActorEvent>>,
}

impl ActorLogs {
    pub fn new(logs: BTreeMap<String, Vec<ActorEvent>>) -> Result<Self, MetricError> {
        for events in logs.values() {
            if let Some(e) = events.iter().find(|e| !e.time.is_finite()) {
                return Err(MetricError::NotFinite {
                    offset: u64::from(e.frame),
                    component: 0,
                });
            }
            for (index, pair) in events.windows(2).enumerate() {
                if pair[1].frame < pair[0].frame || pair[1].time < pair[0].time {
                    return Err(MetricError::TimeOrder {
                        index: index + 1,
                        previous: pair[0].time,
                        time: pair[1].time,
                    });
                }
            }
        }
        Ok(Self { logs })
    }

    /// An actor's log; empty for one the run never logged.
    pub fn log(&self, actor: &str) -> &[ActorEvent] {
        self.logs.get(actor).map_or(&[], Vec::as_slice)
    }

    pub fn actors(&self) -> impl Iterator<Item = &str> {
        self.logs.keys().map(String::as_str)
    }
}

impl TryFrom<BTreeMap<String, Vec<ActorEvent>>> for ActorLogs {
    type Error = MetricError;

    fn try_from(logs: BTreeMap<String, Vec<ActorEvent>>) -> Result<Self, Self::Error> {
        ActorLogs::new(logs)
    }
}

impl From<ActorLogs> for BTreeMap<String, Vec<ActorEvent>> {
    fn from(logs: ActorLogs) -> Self {
        logs.logs
    }
}

/// One event of an actor's agreed prefix: the replays' earliest and latest
/// stamps for it.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct EventBand {
    pub kind: String,
    pub earliest: f64,
    pub latest: f64,
    pub first_frame: u32,
    pub last_frame: u32,
}

/// What an actor's replays did where they part: how many took each next
/// event, [`END`] for a log that ended there.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Branches {
    pub index: usize,
    pub outcomes: BTreeMap<String, usize>,
}

/// A log ending where others go on, among [`Branches::outcomes`].
pub const END: &str = "(end)";

/// One actor across the replays: how many logged it, the events they all
/// logged in the same order and where they stop doing so.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ActorReference {
    /// The replays that logged it at all.
    pub present: usize,
    pub prefix: Vec<EventBand>,
    /// `None` when every replay's log is exactly the prefix.
    pub parted: Option<Branches>,
}

/// Which kinds the anchor's horizon gates: past it a gated kind's order is
/// checked, never its stamps.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Gate {
    /// Decision 18 as first written: every kind, and a kind a roll times is
    /// never stamped at all.
    #[default]
    AllKinds,
    /// AH.45's (b): only the kinds a roll times, stamped inside the horizon;
    /// every other kind is stamped wherever it falls.
    RolledKinds,
}

/// Which stamps count and how far they may stray.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Judge {
    /// Game seconds either side of the replays' stamps, by event kind, else
    /// `default`.
    pub default: f64,
    #[serde(default)]
    pub floors: BTreeMap<String, f64>,
    /// The anchor's horizon: the last frame since F0 inside it. `None`
    /// gates nothing.
    #[serde(default)]
    pub horizon: Option<u32>,
    /// Kinds a roll times.
    #[serde(default)]
    pub ordered: BTreeSet<String>,
    #[serde(default)]
    pub gate: Gate,
}

impl Judge {
    pub fn floor(&self, kind: &str) -> f64 {
        self.floors.get(kind).copied().unwrap_or(self.default)
    }

    /// Whether `band`'s stamps are judged. Inside the horizon means every
    /// replay's stamp lies at or before it.
    pub fn stamps(&self, band: &EventBand) -> bool {
        let rolled = self.ordered.contains(&band.kind);
        let inside = self.horizon.is_none_or(|h| band.last_frame <= h);
        match self.gate {
            Gate::AllKinds => !rolled && inside,
            Gate::RolledKinds => !rolled || inside,
        }
    }

    fn validate(&self) -> Result<(), MetricError> {
        check_tolerance("the event floor", self.default)?;
        self.floors
            .values()
            .try_for_each(|&f| check_tolerance("an event kind's floor", f))
    }
}

/// What a candidate did wrong in one actor's log. `Kind`, `Missing` and
/// `Extra` end that actor's comparison (what follows is shifted); `Stamp`
/// doesn't.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub enum LogMismatch {
    Kind {
        index: usize,
        expected: String,
        found: String,
        time: f64,
    },
    Missing {
        index: usize,
        expected: String,
    },
    Extra {
        index: usize,
        found: String,
        time: f64,
    },
    Stamp {
        index: usize,
        kind: String,
        time: f64,
        low: f64,
        high: f64,
    },
}

#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct ActorVerdict {
    /// Prefix events the candidate had an event for.
    pub compared: usize,
    /// Of those, the ones whose stamps were judged.
    pub stamped: usize,
    pub mismatches: Vec<LogMismatch>,
}

impl ActorVerdict {
    pub fn passed(&self) -> bool {
        self.mismatches.is_empty()
    }
}

/// Every actor's verdict, the reference's and the candidate's actors alike.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct LogVerdict {
    pub actors: BTreeMap<String, ActorVerdict>,
}

impl LogVerdict {
    pub fn passed(&self) -> bool {
        self.actors.values().all(ActorVerdict::passed)
    }

    /// The actors that failed, by name.
    pub fn failed(&self) -> Vec<&str> {
        self.actors
            .iter()
            .filter(|(_, v)| !v.passed())
            .map(|(a, _)| a.as_str())
            .collect()
    }
}

/// The oracle's logs: each actor's prefix over N replays.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct LogReference {
    replays: usize,
    actors: BTreeMap<String, ActorReference>,
}

impl LogReference {
    /// An actor a replay never logged has an empty log there, so a spawn
    /// only some replays make parts at its first event.
    pub fn from_runs(runs: &[ActorLogs]) -> Result<Self, MetricError> {
        if runs.is_empty() {
            return Err(MetricError::NoReplays);
        }
        let names: BTreeSet<&str> = runs.iter().flat_map(ActorLogs::actors).collect();
        let actors = names
            .into_iter()
            .map(|name| {
                let logs: Vec<&[ActorEvent]> = runs.iter().map(|r| r.log(name)).collect();
                (name.to_string(), actor_reference(&logs))
            })
            .collect();
        Ok(Self {
            replays: runs.len(),
            actors,
        })
    }

    pub fn replays(&self) -> usize {
        self.replays
    }

    pub fn actors(&self) -> &BTreeMap<String, ActorReference> {
        &self.actors
    }

    /// Each actor of the reference or the candidate judged on its own.
    pub fn check(&self, candidate: &ActorLogs, judge: &Judge) -> Result<LogVerdict, MetricError> {
        judge.validate()?;
        let empty = ActorReference {
            present: 0,
            prefix: Vec::new(),
            parted: None,
        };
        let names: BTreeSet<&str> = self
            .actors
            .keys()
            .map(String::as_str)
            .chain(candidate.actors())
            .collect();
        let actors = names
            .into_iter()
            .map(|name| {
                let reference = self.actors.get(name).unwrap_or(&empty);
                (
                    name.to_string(),
                    check_actor(reference, candidate.log(name), judge),
                )
            })
            .collect();
        Ok(LogVerdict { actors })
    }
}

fn actor_reference(logs: &[&[ActorEvent]]) -> ActorReference {
    let mut prefix = Vec::new();
    let mut index = 0;
    let parted = loop {
        let at: Vec<Option<&ActorEvent>> = logs.iter().map(|l| l.get(index)).collect();
        if at.iter().all(Option::is_none) {
            break None;
        }
        let first = at[0];
        let agree = at.iter().all(|e| match (e, first) {
            (Some(e), Some(first)) => e.kind == first.kind,
            _ => false,
        });
        if !agree {
            let mut outcomes = BTreeMap::new();
            for e in &at {
                *outcomes
                    .entry(e.map_or(END.to_string(), |e| e.kind.clone()))
                    .or_insert(0) += 1;
            }
            break Some(Branches { index, outcomes });
        }
        let events: Vec<&ActorEvent> = at.into_iter().flatten().collect();
        let time = |f: fn(f64, f64) -> f64| events.iter().map(|e| e.time).reduce(f).unwrap();
        prefix.push(EventBand {
            kind: events[0].kind.clone(),
            earliest: time(f64::min),
            latest: time(f64::max),
            first_frame: events.iter().map(|e| e.frame).min().unwrap(),
            last_frame: events.iter().map(|e| e.frame).max().unwrap(),
        });
        index += 1;
    };
    ActorReference {
        present: logs.iter().filter(|l| !l.is_empty()).count(),
        prefix,
        parted,
    }
}

fn check_actor(reference: &ActorReference, log: &[ActorEvent], judge: &Judge) -> ActorVerdict {
    let mut verdict = ActorVerdict::default();
    for (index, band) in reference.prefix.iter().enumerate() {
        let Some(found) = log.get(index) else {
            verdict.mismatches.push(LogMismatch::Missing {
                index,
                expected: band.kind.clone(),
            });
            return verdict;
        };
        verdict.compared += 1;
        if found.kind != band.kind {
            verdict.mismatches.push(LogMismatch::Kind {
                index,
                expected: band.kind.clone(),
                found: found.kind.clone(),
                time: found.time,
            });
            return verdict;
        }
        if judge.stamps(band) {
            verdict.stamped += 1;
            let floor = judge.floor(&band.kind);
            let (low, high) = (band.earliest - floor, band.latest + floor);
            if !(low..=high).contains(&found.time) {
                verdict.mismatches.push(LogMismatch::Stamp {
                    index,
                    kind: found.kind.clone(),
                    time: found.time,
                    low,
                    high,
                });
            }
        }
    }
    if reference.parted.is_none()
        && let Some(extra) = log.get(reference.prefix.len())
    {
        verdict.mismatches.push(LogMismatch::Extra {
            index: reference.prefix.len(),
            found: extra.kind.clone(),
            time: extra.time,
        });
    }
    verdict
}

/// Leave-one-out, AF.12's acceptance on game time: each run judged against
/// the others' reference. Needs three runs.
pub fn leave_one_out(runs: &[ActorLogs], judge: &Judge) -> Result<Vec<LogVerdict>, MetricError> {
    if runs.len() < 3 {
        return Err(MetricError::TooFewReplays {
            needed: 3,
            found: runs.len(),
        });
    }
    (0..runs.len())
        .map(|i| {
            let others: Vec<ActorLogs> = runs
                .iter()
                .enumerate()
                .filter(|&(j, _)| j != i)
                .map(|(_, r)| r.clone())
                .collect();
            LogReference::from_runs(&others)?.check(&runs[i], judge)
        })
        .collect()
}

/// The least floor each event kind needs for every run's leave-one-out to
/// pass on stamps (0 for a kind always inside the others' spread): AF.12's
/// calibration, before the perturbations check the floors can still fail.
/// The judge's floors are ignored; its horizon, ordered kinds and gate hold.
pub fn floors_needed(
    runs: &[ActorLogs],
    judge: &Judge,
) -> Result<BTreeMap<String, f64>, MetricError> {
    let open = Judge {
        default: 0.0,
        floors: BTreeMap::new(),
        ..judge.clone()
    };
    let mut needed: BTreeMap<String, f64> = BTreeMap::new();
    for verdict in leave_one_out(runs, &open)? {
        for v in verdict.actors.values() {
            for m in &v.mismatches {
                if let LogMismatch::Stamp {
                    kind,
                    time,
                    low,
                    high,
                    ..
                } = m
                {
                    let by = (low - time).max(time - high);
                    let slot = needed.entry(kind.clone()).or_insert(0.0);
                    *slot = slot.max(by);
                }
            }
        }
    }
    Ok(needed)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::events::{Event, EventLog, EventReference};

    const F: f64 = 1.0 / 64.0;

    fn logs(actors: &[(&str, &[(&str, u32)])]) -> ActorLogs {
        ActorLogs::new(
            actors
                .iter()
                .map(|(name, events)| {
                    (
                        name.to_string(),
                        events
                            .iter()
                            .map(|&(k, frame)| ActorEvent::new(k, f64::from(frame) * F, frame))
                            .collect(),
                    )
                })
                .collect(),
        )
        .unwrap()
    }

    fn judge(default: f64) -> Judge {
        Judge {
            default,
            floors: BTreeMap::new(),
            horizon: None,
            ordered: BTreeSet::new(),
            gate: Gate::AllKinds,
        }
    }

    /// Two Chompies whose states interleave one way in the first replay and
    /// the other in the second, each Chompy's own log within a frame.
    fn interleaved() -> Vec<ActorLogs> {
        vec![
            logs(&[
                (
                    "Enemy_Chompy(1)",
                    &[("state Alerted", 60), ("state Battle", 80)],
                ),
                (
                    "Enemy_Chompy(2)",
                    &[("state Alerted", 61), ("state Battle", 79)],
                ),
            ]),
            logs(&[
                (
                    "Enemy_Chompy(1)",
                    &[("state Alerted", 61), ("state Battle", 79)],
                ),
                (
                    "Enemy_Chompy(2)",
                    &[("state Alerted", 60), ("state Battle", 80)],
                ),
            ]),
        ]
    }

    #[test]
    fn chompies_interleaving_differently_pass_while_each_log_agrees() {
        let runs = interleaved();
        let reference = LogReference::from_runs(&runs).unwrap();
        for actor in reference.actors().values() {
            assert_eq!(actor.parted, None);
            assert_eq!(actor.prefix.len(), 2);
        }
        // Either replay, or a candidate in a third order, passes at floor 0.
        for candidate in runs.iter().cloned().chain([logs(&[
            (
                "Enemy_Chompy(1)",
                &[("state Alerted", 60), ("state Battle", 79)],
            ),
            (
                "Enemy_Chompy(2)",
                &[("state Alerted", 61), ("state Battle", 80)],
            ),
        ])]) {
            let verdict = reference.check(&candidate, &judge(0.0)).unwrap();
            assert!(verdict.passed(), "{verdict:?}");
            assert_eq!(verdict.actors["Enemy_Chompy(1)"].stamped, 2);
        }
        // AF's one interleaved prefix parts at the first event, so it could
        // judge nothing of the same two replays.
        let merged = |l: &ActorLogs| {
            let mut all: Vec<(u32, String)> = l
                .actors()
                .flat_map(|a| {
                    l.log(a)
                        .iter()
                        .map(move |e| (e.frame, format!("{a} {}", e.kind)))
                })
                .collect();
            all.sort();
            EventLog::new(
                all.into_iter()
                    .map(|(f, k)| Event::new(u64::from(f), k))
                    .collect(),
            )
            .unwrap()
        };
        let af = EventReference::from_logs(&runs.iter().map(merged).collect::<Vec<_>>()).unwrap();
        assert_eq!(af.prefix().len(), 0);
        assert_eq!(af.parted().map(|p| p.index), Some(0));
    }

    #[test]
    fn one_actors_log_off_fails_that_actor_alone() {
        let reference = LogReference::from_runs(&interleaved()).unwrap();
        // Chompy 2 battles 0.25 s late; Chompy 1 is fine.
        let late = logs(&[
            (
                "Enemy_Chompy(1)",
                &[("state Alerted", 60), ("state Battle", 80)],
            ),
            (
                "Enemy_Chompy(2)",
                &[("state Alerted", 60), ("state Battle", 96)],
            ),
        ]);
        let verdict = reference.check(&late, &judge(F)).unwrap();
        assert_eq!(verdict.failed(), ["Enemy_Chompy(2)"]);
        assert_eq!(
            verdict.actors["Enemy_Chompy(2)"].mismatches,
            [LogMismatch::Stamp {
                index: 1,
                kind: "state Battle".into(),
                time: 96.0 * F,
                low: 78.0 * F,
                high: 81.0 * F
            }]
        );
        // A state skipped, a state too many, an actor missing or extra.
        let skipped = logs(&[
            ("Enemy_Chompy(1)", &[("state Battle", 80)]),
            (
                "Enemy_Chompy(2)",
                &[("state Alerted", 60), ("state Battle", 80)],
            ),
        ]);
        let v = reference.check(&skipped, &judge(F)).unwrap();
        assert!(matches!(
            v.actors["Enemy_Chompy(1)"].mismatches[..],
            [LogMismatch::Kind { index: 0, .. }]
        ));
        let extra = logs(&[
            (
                "Enemy_Chompy(1)",
                &[
                    ("state Alerted", 60),
                    ("state Battle", 80),
                    ("state Nip", 90),
                ],
            ),
            (
                "Enemy_Chompy(2)",
                &[("state Alerted", 60), ("state Battle", 80)],
            ),
            ("Enemy_Chompy(3)", &[("state Alerted", 70)]),
        ]);
        let v = reference.check(&extra, &judge(F)).unwrap();
        assert_eq!(v.failed(), ["Enemy_Chompy(1)", "Enemy_Chompy(3)"]);
        let missing = logs(&[(
            "Enemy_Chompy(1)",
            &[("state Alerted", 60), ("state Battle", 80)],
        )]);
        let v = reference.check(&missing, &judge(F)).unwrap();
        assert_eq!(v.failed(), ["Enemy_Chompy(2)"]);
    }

    #[test]
    fn a_reference_passes_a_candidate_inside_it_and_fails_one_a_floor_outside() {
        let reference = LogReference::from_runs(&interleaved()).unwrap();
        // Battle's band is frames 79 to 80; a floor of 2 frames lets 82 in and
        // not 83 (exact in binary: the stamps are multiples of 1/64).
        let at = |frame: u32| {
            logs(&[
                (
                    "Enemy_Chompy(1)",
                    &[("state Alerted", 60), ("state Battle", frame)],
                ),
                (
                    "Enemy_Chompy(2)",
                    &[("state Alerted", 60), ("state Battle", 80)],
                ),
            ])
        };
        let floor = 2.0 * F;
        for frame in [77, 79, 80, 82] {
            assert!(
                reference.check(&at(frame), &judge(floor)).unwrap().passed(),
                "{frame}"
            );
        }
        for frame in [76, 83] {
            assert!(
                !reference.check(&at(frame), &judge(floor)).unwrap().passed(),
                "{frame}"
            );
        }
        // A kind's own floor wins over the default.
        let mut j = judge(0.0);
        j.floors.insert("state Battle".into(), floor);
        assert!(reference.check(&at(82), &j).unwrap().passed());
        assert!(reference.check(&at(82), &judge(-1.0)).is_err());
    }

    #[test]
    fn past_the_horizon_and_for_rolled_kinds_only_order_counts() {
        let reference = LogReference::from_runs(&interleaved()).unwrap();
        let way_late = logs(&[
            (
                "Enemy_Chompy(1)",
                &[("state Alerted", 60), ("state Battle", 200)],
            ),
            (
                "Enemy_Chompy(2)",
                &[("state Alerted", 60), ("state Battle", 80)],
            ),
        ]);
        assert!(!reference.check(&way_late, &judge(F)).unwrap().passed());
        // A horizon at frame 70: Battle (79-80) lies past it.
        let mut j = judge(F);
        j.horizon = Some(70);
        let v = reference.check(&way_late, &j).unwrap();
        assert!(v.passed(), "{v:?}");
        assert_eq!(v.actors["Enemy_Chompy(1)"].stamped, 1);
        // The horizon at the band's last frame still judges it; one frame
        // short of it doesn't, though a replay's stamp (79) lies inside:
        // every replay's must.
        j.horizon = Some(80);
        assert!(!reference.check(&way_late, &j).unwrap().passed());
        j.horizon = Some(79);
        assert!(reference.check(&way_late, &j).unwrap().passed());
        // A kind a roll times: order only, wherever it falls.
        let mut j = judge(F);
        j.ordered.insert("state Battle".into());
        assert!(reference.check(&way_late, &j).unwrap().passed());
        // Order still counts past the horizon.
        j.horizon = Some(10);
        let swapped = logs(&[
            (
                "Enemy_Chompy(1)",
                &[("state Battle", 60), ("state Alerted", 80)],
            ),
            (
                "Enemy_Chompy(2)",
                &[("state Alerted", 60), ("state Battle", 80)],
            ),
        ]);
        assert!(!reference.check(&swapped, &j).unwrap().passed());
    }

    #[test]
    fn under_b_the_horizon_gates_only_the_rolled_kinds() {
        let reference = LogReference::from_runs(&interleaved()).unwrap();
        // Battle (frames 79-80) 120 frames late, Alerted (60-61) in place.
        let way_late = logs(&[
            (
                "Enemy_Chompy(1)",
                &[("state Alerted", 60), ("state Battle", 200)],
            ),
            (
                "Enemy_Chompy(2)",
                &[("state Alerted", 60), ("state Battle", 80)],
            ),
        ]);
        let mut j = judge(F);
        j.gate = Gate::RolledKinds;
        j.horizon = Some(70);
        // Unrolled, Battle is stamped past the horizon and fails.
        let v = reference.check(&way_late, &j).unwrap();
        assert_eq!(v.failed(), ["Enemy_Chompy(1)"]);
        assert_eq!(v.actors["Enemy_Chompy(1)"].stamped, 2);
        // Rolled, it's stamped inside the horizon and skipped past it.
        j.ordered.insert("state Battle".into());
        let v = reference.check(&way_late, &j).unwrap();
        assert!(v.passed(), "{v:?}");
        assert_eq!(v.actors["Enemy_Chompy(1)"].stamped, 1);
        j.horizon = Some(80);
        assert!(!reference.check(&way_late, &j).unwrap().passed());
        // No horizon gates nothing: the rolled kind is stamped too.
        j.horizon = None;
        assert!(!reference.check(&way_late, &j).unwrap().passed());
        // A judge written before the gate existed reads as decision 18's
        // first rule.
        let old = r#"{"default":0.1,"horizon":3,"ordered":["bite"]}"#;
        assert_eq!(
            serde_json::from_str::<Judge>(old).unwrap().gate,
            Gate::AllKinds
        );
    }

    #[test]
    fn replays_that_part_count_their_branches_and_judge_only_before() {
        let mut runs = interleaved();
        runs.push(logs(&[
            (
                "Enemy_Chompy(1)",
                &[("state Alerted", 60), ("state Fled", 85)],
            ),
            ("Enemy_Chompy(2)", &[("state Alerted", 61)]),
        ]));
        let reference = LogReference::from_runs(&runs).unwrap();
        let c1 = &reference.actors()["Enemy_Chompy(1)"];
        assert_eq!(c1.prefix.len(), 1);
        assert_eq!(
            c1.parted,
            Some(Branches {
                index: 1,
                outcomes: BTreeMap::from([("state Battle".into(), 2), ("state Fled".into(), 1)])
            })
        );
        let c2 = &reference.actors()["Enemy_Chompy(2)"].parted;
        assert_eq!(c2.as_ref().unwrap().outcomes[END], 1);
        // Past the parting nothing is judged.
        let either = logs(&[
            (
                "Enemy_Chompy(1)",
                &[("state Alerted", 60), ("anything", 300)],
            ),
            ("Enemy_Chompy(2)", &[("state Alerted", 60)]),
        ]);
        assert!(reference.check(&either, &judge(F)).unwrap().passed());
    }

    #[test]
    fn leave_one_out_and_the_floors_it_needs() {
        // Three replays agreeing within 2 frames on Battle, a fourth 6 frames
        // past the others.
        let run = |battle: u32| {
            logs(&[(
                "Enemy_Chompy(1)",
                &[("state Alerted", 60), ("state Battle", battle)],
            )])
        };
        let runs = vec![run(80), run(81), run(82), run(88)];
        let verdicts = leave_one_out(&runs, &judge(F)).unwrap();
        let failed: Vec<bool> = verdicts.iter().map(|v| !v.passed()).collect();
        assert_eq!(failed, [false, false, false, true]);
        // Run 3 needs 6 frames past the others' 82, so that's the floor.
        let needed = floors_needed(&runs, &judge(F)).unwrap();
        assert_eq!(needed["state Battle"], 6.0 * F);
        assert!(!needed.contains_key("state Alerted"));
        let mut j = judge(0.0);
        j.floors = needed;
        assert!(
            leave_one_out(&runs, &j)
                .unwrap()
                .iter()
                .all(LogVerdict::passed)
        );
        assert!(leave_one_out(&runs[..2], &j).is_err());
    }

    #[test]
    fn a_log_refuses_time_going_back() {
        let back = BTreeMap::from([(
            "hero".to_string(),
            vec![
                ActorEvent::new("press South", 1.0, 60),
                ActorEvent::new("throw", 0.5, 61),
            ],
        )]);
        assert!(matches!(
            ActorLogs::new(back),
            Err(MetricError::TimeOrder { index: 1, .. })
        ));
        let json = serde_json::to_string(&interleaved()[0]).unwrap();
        assert_eq!(
            serde_json::from_str::<ActorLogs>(&json).unwrap(),
            interleaved()[0]
        );
    }
}
