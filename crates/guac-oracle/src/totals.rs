//! Totals (XP, gold, damage) at a checkpoint (openspec AF, design decision 9):
//! equal when the N replays agree, otherwise within their [min, max], either
//! widened by a floor. AF observes no totals, so the floor has no default until
//! the first rung that does (AH) calibrates it.

use std::collections::BTreeMap;

use serde::Serialize;

use crate::MetricError;

/// One run's totals at one checkpoint, by name (`"xp"`, `"gold"`).
pub type Totals = BTreeMap<String, i64>;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct TotalBand {
    pub min: i64,
    pub max: i64,
}

/// Each total's [min, max] over N replays that all report the same names.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct TotalsReference {
    replays: usize,
    bands: BTreeMap<String, TotalBand>,
}

impl TotalsReference {
    pub fn from_runs(runs: &[Totals]) -> Result<Self, MetricError> {
        let first = runs.first().ok_or(MetricError::NoReplays)?;
        for (replay, run) in runs.iter().enumerate().skip(1) {
            if let Some(name) = first
                .keys()
                .find(|k| !run.contains_key(*k))
                .or_else(|| run.keys().find(|k| !first.contains_key(*k)))
            {
                return Err(MetricError::TotalsMismatch {
                    replay,
                    name: name.clone(),
                });
            }
        }
        let bands = first
            .keys()
            .map(|name| {
                let values = runs.iter().map(|r| r[name]);
                let band = TotalBand {
                    min: values.clone().min().expect("at least one replay"),
                    max: values.max().expect("at least one replay"),
                };
                (name.clone(), band)
            })
            .collect();
        Ok(Self {
            replays: runs.len(),
            bands,
        })
    }

    pub fn replays(&self) -> usize {
        self.replays
    }

    pub fn bands(&self) -> &BTreeMap<String, TotalBand> {
        &self.bands
    }

    /// Every total the replays report has to lie in [min - floor, max +
    /// floor]; one missing from the candidate, or one the replays don't
    /// report, is a mismatch too.
    pub fn check(&self, candidate: &Totals, floor: u64) -> TotalsVerdict {
        let floor = i64::try_from(floor).unwrap_or(i64::MAX);
        let mut mismatches = Vec::new();
        for (name, band) in &self.bands {
            match candidate.get(name) {
                None => mismatches.push(TotalMismatch::Missing { name: name.clone() }),
                Some(&value) => {
                    let low = band.min.saturating_sub(floor);
                    let high = band.max.saturating_add(floor);
                    if !(low..=high).contains(&value) {
                        mismatches.push(TotalMismatch::Outside {
                            name: name.clone(),
                            value,
                            low,
                            high,
                        });
                    }
                }
            }
        }
        for (name, &value) in candidate {
            if !self.bands.contains_key(name) {
                mismatches.push(TotalMismatch::Unknown {
                    name: name.clone(),
                    value,
                });
            }
        }
        TotalsVerdict { mismatches }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub enum TotalMismatch {
    /// Outside [low, high], the replays' band widened by the floor.
    Outside {
        name: String,
        value: i64,
        low: i64,
        high: i64,
    },
    Missing {
        name: String,
    },
    /// The candidate reports a name no replay does: a counter the oracle
    /// side never read, so nothing to compare it with.
    Unknown {
        name: String,
        value: i64,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct TotalsVerdict {
    pub mismatches: Vec<TotalMismatch>,
}

impl TotalsVerdict {
    pub fn passed(&self) -> bool {
        self.mismatches.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn totals(pairs: &[(&str, i64)]) -> Totals {
        pairs.iter().map(|&(k, v)| (k.to_string(), v)).collect()
    }

    #[test]
    fn a_total_off_by_one_from_agreeing_replays_fails() {
        let runs = vec![totals(&[("xp", 120), ("gold", 35)]); 10];
        let reference = TotalsReference::from_runs(&runs).unwrap();
        assert_eq!(reference.replays(), 10);
        assert!(
            reference
                .check(&totals(&[("xp", 120), ("gold", 35)]), 0)
                .passed()
        );
        for xp in [119, 121] {
            let verdict = reference.check(&totals(&[("xp", xp), ("gold", 35)]), 0);
            assert_eq!(
                verdict.mismatches,
                [TotalMismatch::Outside {
                    name: "xp".into(),
                    value: xp,
                    low: 120,
                    high: 120
                }]
            );
        }
        // A floor of 1 lets it through.
        assert!(
            reference
                .check(&totals(&[("xp", 121), ("gold", 35)]), 1)
                .passed()
        );
    }

    #[test]
    fn disagreeing_replays_allow_their_range() {
        let runs = vec![
            totals(&[("damage", 300)]),
            totals(&[("damage", 312)]),
            totals(&[("damage", 305)]),
        ];
        let reference = TotalsReference::from_runs(&runs).unwrap();
        assert_eq!(
            reference.bands()["damage"],
            TotalBand { min: 300, max: 312 }
        );
        assert!(reference.check(&totals(&[("damage", 300)]), 0).passed());
        assert!(reference.check(&totals(&[("damage", 312)]), 0).passed());
        assert!(!reference.check(&totals(&[("damage", 313)]), 0).passed());
        assert!(!reference.check(&totals(&[("damage", 299)]), 0).passed());
    }

    #[test]
    fn names_have_to_line_up() {
        let runs = vec![totals(&[("xp", 1), ("gold", 2)]), totals(&[("xp", 1)])];
        assert_eq!(
            TotalsReference::from_runs(&runs),
            Err(MetricError::TotalsMismatch {
                replay: 1,
                name: "gold".into()
            })
        );
        let runs = vec![totals(&[("xp", 1)]), totals(&[("xp", 1), ("gold", 2)])];
        assert_eq!(
            TotalsReference::from_runs(&runs),
            Err(MetricError::TotalsMismatch {
                replay: 1,
                name: "gold".into()
            })
        );
        assert_eq!(TotalsReference::from_runs(&[]), Err(MetricError::NoReplays));
        let reference = TotalsReference::from_runs(&[totals(&[("xp", 1)])]).unwrap();
        assert_eq!(
            reference.check(&totals(&[("gold", 2)]), 0).mismatches,
            [
                TotalMismatch::Missing { name: "xp".into() },
                TotalMismatch::Unknown {
                    name: "gold".into(),
                    value: 2
                }
            ]
        );
        // A huge floor saturates rather than wrapping.
        for xp in [i64::MAX, -(1 << 62)] {
            assert!(reference.check(&totals(&[("xp", xp)]), u64::MAX).passed());
        }
    }
}
