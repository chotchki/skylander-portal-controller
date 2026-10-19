//! Observation traces keyed by flip offset, the envelope N oracle replays make
//! of one observable, the trajectory comparison against it and the usable
//! horizon (openspec AF, design decision 9). Offsets count flips from a run's
//! F0, the flip `Start Paused` holds every replay at, so replays booted from
//! one state line up checkpoint for checkpoint.
//!
//! The tolerance's floor and k are parameters here; AF.12 sets their defaults
//! from AF.11's measurement.

use serde::{Deserialize, Serialize};

use crate::MetricError;

/// One observation: `offset` flips after F0, one value per component (a
/// position's x, y and z, say).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Sample {
    pub offset: u64,
    pub values: Vec<f64>,
}

impl Sample {
    pub fn new(offset: u64, values: impl Into<Vec<f64>>) -> Self {
        Self {
            offset,
            values: values.into(),
        }
    }
}

/// One observable over one run. Offsets strictly increase, every sample has
/// the same number of components (at least one) and every value is finite;
/// [`Trace::new`] refuses anything else, and so does deserializing one (a
/// trace serializes as its plain list of samples).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "Vec<Sample>", into = "Vec<Sample>")]
pub struct Trace {
    samples: Vec<Sample>,
}

impl Trace {
    pub fn new(samples: Vec<Sample>) -> Result<Self, MetricError> {
        let first = samples.first().ok_or(MetricError::EmptyTrace)?;
        let expected = first.values.len();
        if expected == 0 {
            return Err(MetricError::NoComponents {
                offset: first.offset,
            });
        }
        let mut previous: Option<u64> = None;
        for sample in &samples {
            if let Some(previous) = previous
                && sample.offset <= previous
            {
                return Err(MetricError::OffsetOrder {
                    previous,
                    offset: sample.offset,
                });
            }
            previous = Some(sample.offset);
            if sample.values.len() != expected {
                return Err(MetricError::ComponentCount {
                    offset: sample.offset,
                    expected,
                    found: sample.values.len(),
                });
            }
            if let Some(component) = sample.values.iter().position(|v| !v.is_finite()) {
                return Err(MetricError::NotFinite {
                    offset: sample.offset,
                    component,
                });
            }
        }
        Ok(Self { samples })
    }

    pub fn samples(&self) -> &[Sample] {
        &self.samples
    }

    pub fn components(&self) -> usize {
        self.samples[0].values.len()
    }

    /// The sample at `offset`, if the trace has one.
    pub fn at(&self, offset: u64) -> Option<&[f64]> {
        self.samples
            .binary_search_by_key(&offset, |s| s.offset)
            .ok()
            .map(|i| self.samples[i].values.as_slice())
    }
}

impl TryFrom<Vec<Sample>> for Trace {
    type Error = MetricError;

    fn try_from(samples: Vec<Sample>) -> Result<Self, Self::Error> {
        Trace::new(samples)
    }
}

impl From<Trace> for Vec<Sample> {
    fn from(trace: Trace) -> Self {
        trace.samples
    }
}

/// What the replays read for one component at one checkpoint.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct Band {
    pub min: f64,
    pub max: f64,
    /// The middle value, or the mean of the middle two for an even count.
    pub median: f64,
}

impl Band {
    fn of(mut values: Vec<f64>) -> Self {
        values.sort_by(f64::total_cmp);
        let n = values.len();
        let median = if n % 2 == 1 {
            values[n / 2]
        } else {
            (values[n / 2 - 1] + values[n / 2]) / 2.0
        };
        Self {
            min: values[0],
            max: values[n - 1],
            median,
        }
    }

    pub fn spread(&self) -> f64 {
        self.max - self.min
    }
}

/// One checkpoint of an envelope: its offset and a band per component.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Checkpoint {
    pub offset: u64,
    pub bands: Vec<Band>,
}

/// The [min, max] (and median) N replays give each component of one
/// observable at each checkpoint. Built only from traces sampled at the same
/// checkpoints with the same components.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Envelope {
    replays: usize,
    checkpoints: Vec<Checkpoint>,
}

impl Envelope {
    pub fn from_traces(traces: &[Trace]) -> Result<Self, MetricError> {
        let first = traces.first().ok_or(MetricError::NoReplays)?;
        for (replay, trace) in traces.iter().enumerate().skip(1) {
            let offsets = first.samples.iter().map(|s| s.offset);
            let theirs = trace.samples.iter().map(|s| s.offset);
            if let Some(index) = first_difference(offsets, theirs) {
                return Err(MetricError::CheckpointMismatch { replay, index });
            }
            if trace.components() != first.components() {
                return Err(MetricError::ComponentCount {
                    offset: trace.samples[0].offset,
                    expected: first.components(),
                    found: trace.components(),
                });
            }
        }
        let checkpoints = first
            .samples
            .iter()
            .enumerate()
            .map(|(i, sample)| Checkpoint {
                offset: sample.offset,
                bands: (0..first.components())
                    .map(|c| Band::of(traces.iter().map(|t| t.samples[i].values[c]).collect()))
                    .collect(),
            })
            .collect();
        Ok(Self {
            replays: traces.len(),
            checkpoints,
        })
    }

    /// How many traces it was built from.
    pub fn replays(&self) -> usize {
        self.replays
    }

    pub fn checkpoints(&self) -> &[Checkpoint] {
        &self.checkpoints
    }

    /// The same envelope cut to the checkpoints at or before `last`: inside a
    /// usable horizon, say.
    pub fn until(&self, last: u64) -> Envelope {
        Envelope {
            replays: self.replays,
            checkpoints: self
                .checkpoints
                .iter()
                .filter(|c| c.offset <= last)
                .cloned()
                .collect(),
        }
    }

    /// Checks `candidate` at every checkpoint: each component has to lie in
    /// [min - m, max + m], m = max(floor, k x that component's spread there).
    /// Samples the candidate has at other offsets are ignored; a checkpoint it
    /// lacks is an error, not a failure.
    pub fn check(
        &self,
        candidate: &Trace,
        tolerance: &TrajectoryTolerance,
    ) -> Result<TrajectoryVerdict, MetricError> {
        tolerance.validate()?;
        let components = self
            .checkpoints
            .first()
            .ok_or(MetricError::NoCheckpoints)?
            .bands
            .len();
        if candidate.components() != components {
            return Err(MetricError::ComponentCount {
                offset: candidate.samples[0].offset,
                expected: components,
                found: candidate.components(),
            });
        }
        let mut worst: Option<Excursion> = None;
        let mut outside = 0;
        let mut first_outside = None;
        let mut checked = 0;
        for checkpoint in &self.checkpoints {
            let values = candidate
                .at(checkpoint.offset)
                .ok_or(MetricError::MissingCheckpoint {
                    offset: checkpoint.offset,
                })?;
            for (component, (band, &value)) in checkpoint.bands.iter().zip(values).enumerate() {
                let margin = tolerance.margin(band.spread());
                let excursion = Excursion::of(
                    checkpoint.offset,
                    component,
                    value,
                    band.min - margin,
                    band.max + margin,
                );
                checked += 1;
                if excursion.by > 0.0 {
                    outside += 1;
                    first_outside.get_or_insert(checkpoint.offset);
                }
                if worst.as_ref().is_none_or(|w| excursion.by > w.by) {
                    worst = Some(excursion);
                }
            }
        }
        Ok(TrajectoryVerdict {
            worst: worst.expect("an envelope checkpoint has at least one component"),
            outside,
            first_outside,
            checked,
        })
    }

    /// Where the replays stop agreeing: a checkpoint agrees when every
    /// component's spread there is at most `floor` (0 asks for identical
    /// values, as the RNG's state does).
    pub fn horizon(&self, floor: f64) -> Result<Horizon, MetricError> {
        check_tolerance("the horizon floor", floor)?;
        Horizon::from_agreement(
            self.checkpoints
                .iter()
                .map(|c| (c.offset, c.bands.iter().all(|b| b.spread() <= floor))),
        )
    }
}

fn first_difference(
    mut a: impl Iterator<Item = u64>,
    mut b: impl Iterator<Item = u64>,
) -> Option<usize> {
    let mut index = 0;
    loop {
        match (a.next(), b.next()) {
            (None, None) => return None,
            (Some(x), Some(y)) if x == y => index += 1,
            _ => return Some(index),
        }
    }
}

/// The trajectory comparison's two parameters: a floor in the observable's
/// own units and k, the multiple of the replays' spread a candidate may stray
/// beyond their [min, max].
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct TrajectoryTolerance {
    pub floor: f64,
    pub k: f64,
}

impl TrajectoryTolerance {
    /// The floor keeps a checkpoint where the replays agreed exactly from
    /// failing a candidate on float noise; k x spread takes over once they
    /// don't.
    pub fn margin(&self, spread: f64) -> f64 {
        self.floor.max(self.k * spread)
    }

    fn validate(&self) -> Result<(), MetricError> {
        check_tolerance("the trajectory floor", self.floor)?;
        check_tolerance("k", self.k)
    }
}

pub(crate) fn check_tolerance(what: &'static str, value: f64) -> Result<(), MetricError> {
    if value.is_finite() && value >= 0.0 {
        Ok(())
    } else {
        Err(MetricError::BadTolerance { what, value })
    }
}

/// How far one candidate value lies from its allowed band: `by` > 0 is that
/// far outside it, `by` <= 0 inside by |by| from the nearer edge.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct Excursion {
    pub offset: u64,
    pub component: usize,
    pub value: f64,
    /// The band's edges with the margin applied.
    pub low: f64,
    pub high: f64,
    pub by: f64,
}

impl Excursion {
    fn of(offset: u64, component: usize, value: f64, low: f64, high: f64) -> Self {
        let by = if value > high {
            value - high
        } else if value < low {
            low - value
        } else {
            -(value - low).min(high - value)
        };
        Self {
            offset,
            component,
            value,
            low,
            high,
            by,
        }
    }
}

/// Passed when the worst excursion is inside its band (`by` <= 0).
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct TrajectoryVerdict {
    /// The largest excursion over every checkpoint and component, the
    /// earliest one on a tie.
    pub worst: Excursion,
    /// How many (checkpoint, component) values lay outside their band.
    pub outside: usize,
    /// The first checkpoint with a value outside its band.
    pub first_outside: Option<u64>,
    /// The envelope's checkpoints times its components.
    pub checked: usize,
}

impl TrajectoryVerdict {
    pub fn passed(&self) -> bool {
        self.worst.by <= 0.0
    }
}

/// How long the replays of one observable stay together.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Horizon {
    /// The last checkpoint of the unbroken run of agreeing checkpoints from
    /// the first one; `None` when they disagree at the first.
    pub last_agreeing: Option<u64>,
    /// The first checkpoint at which they disagree; `None` when they agree at
    /// every one, so the run ended before the horizon showed.
    pub parted_at: Option<u64>,
}

impl Horizon {
    /// From (checkpoint, agreed there) pairs in increasing checkpoint order:
    /// the reduction for any observable whose agreement isn't a spread, the
    /// RNG's equal peeks for one.
    pub fn from_agreement(
        checks: impl IntoIterator<Item = (u64, bool)>,
    ) -> Result<Self, MetricError> {
        let mut last_agreeing = None;
        let mut parted_at = None;
        let mut previous: Option<u64> = None;
        let mut any = false;
        for (offset, agreed) in checks {
            any = true;
            if let Some(previous) = previous
                && offset <= previous
            {
                return Err(MetricError::OffsetOrder { previous, offset });
            }
            previous = Some(offset);
            if parted_at.is_none() {
                if agreed {
                    last_agreeing = Some(offset);
                } else {
                    parted_at = Some(offset);
                }
            }
        }
        if !any {
            return Err(MetricError::NoCheckpoints);
        }
        Ok(Self {
            last_agreeing,
            parted_at,
        })
    }

    /// The replays never parted: the horizon is past the last checkpoint, so
    /// the measurement runs longer before reporting one.
    pub fn held_throughout(&self) -> bool {
        self.parted_at.is_none()
    }
}

/// The usable horizon of N traces of one observable at `floor`: the envelope's
/// [`Envelope::horizon`].
pub fn usable_horizon(traces: &[Trace], floor: f64) -> Result<Horizon, MetricError> {
    Envelope::from_traces(traces)?.horizon(floor)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A trace of `components` values every 60 flips from F0 to F0 + 600,
    /// value(offset, component) supplied.
    fn trace(f: impl Fn(u64, usize) -> f64, components: usize) -> Trace {
        Trace::new(
            (0..=10)
                .map(|i| {
                    let offset = i * 60;
                    Sample::new(
                        offset,
                        (0..components).map(|c| f(offset, c)).collect::<Vec<_>>(),
                    )
                })
                .collect(),
        )
        .unwrap()
    }

    /// Ten replays of a hero walking along x at 0.01 a flip, each off the
    /// next by a jitter that grows with time (0.001 x offset/60 x replay).
    fn replays() -> Vec<Trace> {
        (0..10)
            .map(|r| {
                trace(
                    move |offset, c| {
                        let jitter = 0.001 * (offset / 60) as f64 * r as f64;
                        match c {
                            0 => 0.01 * offset as f64 + jitter,
                            1 => 2.0,
                            _ => -jitter,
                        }
                    },
                    3,
                )
            })
            .collect()
    }

    #[test]
    fn envelope_bands_and_spreads() {
        let envelope = Envelope::from_traces(&replays()).unwrap();
        assert_eq!(envelope.replays(), 10);
        assert_eq!(envelope.checkpoints().len(), 11);
        let at_300 = &envelope.checkpoints()[5];
        assert_eq!(at_300.offset, 300);
        // x: 3.0 + 0.005 r for r in 0..10 -> [3.0, 3.045], median 3.0225.
        let x = at_300.bands[0];
        assert!((x.min - 3.0).abs() < 1e-12 && (x.max - 3.045).abs() < 1e-12);
        assert!((x.median - 3.0225).abs() < 1e-12);
        assert!((x.spread() - 0.045).abs() < 1e-12);
        assert_eq!(at_300.bands[1].spread(), 0.0);
        // At F0 every replay reads the same.
        assert!(
            envelope.checkpoints()[0]
                .bands
                .iter()
                .all(|b| b.spread() == 0.0)
        );
        // An odd count's median is its middle value.
        let odd = Envelope::from_traces(&replays()[..3]).unwrap();
        assert!((odd.checkpoints()[5].bands[0].median - 3.005).abs() < 1e-12);
    }

    #[test]
    fn a_trace_inside_the_envelope_passes_with_its_closest_call() {
        let envelope = Envelope::from_traces(&replays()).unwrap();
        let tolerance = TrajectoryTolerance {
            floor: 0.01,
            k: 0.5,
        };
        // The median replay's path, x nudged 0.009 high at 120: inside the
        // floor of 0.01, so the closest call is there, 0.001 from the edge
        // (k x spread = 0.5 x 0.018 = 0.009 < floor, so the floor sets m).
        let candidate = trace(
            |offset, c| match c {
                0 => 0.01 * offset as f64 + if offset == 120 { 0.018 + 0.009 } else { 0.0 },
                1 => 2.0,
                _ => 0.0,
            },
            3,
        );
        let verdict = envelope.check(&candidate, &tolerance).unwrap();
        assert!(verdict.passed(), "{verdict:?}");
        assert_eq!(verdict.outside, 0);
        assert_eq!(verdict.first_outside, None);
        assert_eq!(verdict.checked, 33);
        assert_eq!(verdict.worst.offset, 120);
        assert_eq!(verdict.worst.component, 0);
        assert!(
            (verdict.worst.by + 0.001).abs() < 1e-9,
            "{:?}",
            verdict.worst
        );
    }

    #[test]
    fn a_trace_outside_the_envelope_fails_at_its_worst_excursion() {
        let envelope = Envelope::from_traces(&replays()).unwrap();
        let tolerance = TrajectoryTolerance {
            floor: 0.01,
            k: 2.0,
        };
        // At 480 x's band is [4.8, 4.872], m = 2 x 0.072, high edge 5.016, so
        // 5.116 is 0.1 out; at 360 z's low edge is -0.054 - 0.108, so -0.172
        // is 0.01 out.
        let candidate = trace(
            |offset, c| match c {
                0 => 0.01 * offset as f64 + if offset == 480 { 0.316 } else { 0.0 },
                1 => 2.0,
                _ => {
                    if offset == 360 {
                        -0.172
                    } else {
                        0.0
                    }
                }
            },
            3,
        );
        let verdict = envelope.check(&candidate, &tolerance).unwrap();
        assert!(!verdict.passed());
        assert_eq!(verdict.outside, 2);
        assert_eq!(verdict.first_outside, Some(360));
        let worst = verdict.worst;
        assert_eq!((worst.offset, worst.component), (480, 0));
        assert!((worst.high - 5.016).abs() < 1e-9, "{worst:?}");
        assert!((worst.by - 0.1).abs() < 1e-9, "{worst:?}");
    }

    #[test]
    fn the_margin_is_the_floor_or_k_spreads_whichever_is_wider() {
        let t = TrajectoryTolerance { floor: 0.5, k: 3.0 };
        assert_eq!(t.margin(0.0), 0.5);
        assert_eq!(t.margin(0.1), 0.5);
        assert_eq!(t.margin(1.0), 3.0);
        // On the edge counts as inside.
        let envelope = Envelope::from_traces(&[trace(|_, _| 1.0, 1)]).unwrap();
        let edge = trace(|_, _| 1.5, 1);
        let verdict = envelope.check(&edge, &t).unwrap();
        assert!(verdict.passed());
        assert_eq!(verdict.worst.by, 0.0);
    }

    #[test]
    fn checks_refuse_shapes_that_dont_line_up() {
        let envelope = Envelope::from_traces(&replays()).unwrap();
        let tolerance = TrajectoryTolerance { floor: 0.1, k: 1.0 };
        // Two components where the envelope has three.
        let narrow = trace(|_, _| 0.0, 2);
        assert!(matches!(
            envelope.check(&narrow, &tolerance),
            Err(MetricError::ComponentCount {
                expected: 3,
                found: 2,
                ..
            })
        ));
        // A candidate missing checkpoint 600.
        let short =
            Trace::new((0..10).map(|i| Sample::new(i * 60, vec![0.0; 3])).collect()).unwrap();
        assert_eq!(
            envelope.check(&short, &tolerance),
            Err(MetricError::MissingCheckpoint { offset: 600 })
        );
        // Extra samples between checkpoints are ignored.
        let dense = Trace::new(
            (0..=600)
                .step_by(30)
                .map(|o| Sample::new(o, vec![0.01 * o as f64, 2.0, 0.0]))
                .collect(),
        )
        .unwrap();
        assert!(envelope.check(&dense, &tolerance).unwrap().passed());
        // Bad tolerances.
        for (floor, k) in [(-0.1, 1.0), (0.1, f64::NAN), (f64::INFINITY, 1.0)] {
            assert!(matches!(
                envelope.check(&dense, &TrajectoryTolerance { floor, k }),
                Err(MetricError::BadTolerance { .. })
            ));
        }
        // An envelope cut before its first checkpoint has nothing to check.
        let none = Envelope::from_traces(&[trace(|_, _| 0.0, 3)]).unwrap();
        let none = Envelope {
            checkpoints: Vec::new(),
            ..none
        };
        assert_eq!(
            none.check(&dense, &tolerance),
            Err(MetricError::NoCheckpoints)
        );
        // Replays sampled at different checkpoints or with different
        // components make no envelope.
        let mut mixed = replays();
        mixed.push(short.clone());
        assert_eq!(
            Envelope::from_traces(&mixed),
            Err(MetricError::CheckpointMismatch {
                replay: 10,
                index: 10
            })
        );
        let mut mixed = replays();
        mixed.push(narrow);
        assert!(matches!(
            Envelope::from_traces(&mixed),
            Err(MetricError::ComponentCount { .. })
        ));
        assert_eq!(Envelope::from_traces(&[]), Err(MetricError::NoReplays));
    }

    #[test]
    fn traces_refuse_bad_samples() {
        assert_eq!(Trace::new(vec![]), Err(MetricError::EmptyTrace));
        assert_eq!(
            Trace::new(vec![Sample::new(0, vec![])]),
            Err(MetricError::NoComponents { offset: 0 })
        );
        assert_eq!(
            Trace::new(vec![Sample::new(60, vec![1.0]), Sample::new(60, vec![1.0])]),
            Err(MetricError::OffsetOrder {
                previous: 60,
                offset: 60
            })
        );
        assert_eq!(
            Trace::new(vec![
                Sample::new(0, vec![1.0]),
                Sample::new(60, vec![1.0, 2.0])
            ]),
            Err(MetricError::ComponentCount {
                offset: 60,
                expected: 1,
                found: 2
            })
        );
        assert_eq!(
            Trace::new(vec![Sample::new(0, vec![1.0, f64::NAN])]),
            Err(MetricError::NotFinite {
                offset: 0,
                component: 1
            })
        );
    }

    #[test]
    fn a_trace_round_trips_through_json_and_refuses_a_bad_one() {
        let original = trace(|o, c| o as f64 * 0.25 + c as f64, 2);
        let json = serde_json::to_string(&original).unwrap();
        assert!(
            json.starts_with("[{\"offset\":0,\"values\":[0.0,1.0]}"),
            "{json}"
        );
        let back: Trace = serde_json::from_str(&json).unwrap();
        assert_eq!(back, original);
        let unordered = r#"[{"offset":60,"values":[1.0]},{"offset":0,"values":[1.0]}]"#;
        let err = serde_json::from_str::<Trace>(unordered).unwrap_err();
        assert!(err.to_string().contains("offsets must increase"), "{err}");
    }

    #[test]
    fn the_horizon_of_traces_that_part_at_a_known_checkpoint() {
        // Ten replays identical to F0 + 240, then replay r drifts by
        // 0.002 x r on y from F0 + 300 on.
        let traces: Vec<Trace> = (0..10)
            .map(|r| {
                trace(
                    move |offset, c| {
                        let drift = if offset >= 300 { 0.002 * r as f64 } else { 0.0 };
                        [0.01 * offset as f64, 2.0 + drift][c]
                    },
                    2,
                )
            })
            .collect();
        let horizon = usable_horizon(&traces, 0.001).unwrap();
        assert_eq!(horizon.last_agreeing, Some(240));
        assert_eq!(horizon.parted_at, Some(300));
        assert!(!horizon.held_throughout());
        // A floor wider than the drift's spread (0.018) holds to the end.
        let wide = usable_horizon(&traces, 0.02).unwrap();
        assert_eq!(wide.last_agreeing, Some(600));
        assert!(wide.held_throughout());
        // Identical values only (floor 0): the same parting.
        assert_eq!(usable_horizon(&traces, 0.0).unwrap().parted_at, Some(300));
        // The envelope cut to the horizon passes every replay against the
        // others' envelope at the floor alone; past it, replay 9 fails.
        let envelope = Envelope::from_traces(&traces[..9]).unwrap();
        let tolerance = TrajectoryTolerance {
            floor: 0.001,
            k: 0.0,
        };
        let inside = envelope.until(horizon.last_agreeing.unwrap());
        assert_eq!(inside.checkpoints().len(), 5);
        assert!(inside.check(&traces[9], &tolerance).unwrap().passed());
        let all = envelope.check(&traces[9], &tolerance).unwrap();
        assert_eq!(all.first_outside, Some(300));
    }

    #[test]
    fn a_horizon_from_agreement_flags() {
        let h = Horizon::from_agreement([(0, false), (60, true)]).unwrap();
        assert_eq!(
            h,
            Horizon {
                last_agreeing: None,
                parted_at: Some(0)
            }
        );
        // Agreeing again after parting doesn't extend it.
        let h = Horizon::from_agreement([(0, true), (60, false), (120, true)]).unwrap();
        assert_eq!(h.last_agreeing, Some(0));
        assert_eq!(h.parted_at, Some(60));
        assert_eq!(
            Horizon::from_agreement([(60, true), (0, true)]),
            Err(MetricError::OffsetOrder {
                previous: 60,
                offset: 0
            })
        );
        assert_eq!(
            Horizon::from_agreement(std::iter::empty()),
            Err(MetricError::NoCheckpoints)
        );
        let envelope = Envelope::from_traces(&replays()).unwrap();
        assert!(matches!(
            envelope.horizon(-1.0),
            Err(MetricError::BadTolerance { .. })
        ));
    }
}
