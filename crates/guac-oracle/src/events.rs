//! Event-sequence equality (openspec AF, design decision 9): spawns, hits,
//! pickups and script events, each stamped with the flip offset it happened
//! at. The oracle's sequence is the prefix all N replays agree on, and where
//! they part is reported with it; a candidate passes when its sequence is that
//! prefix and every stamp lies within the replays' stamps for that event, plus
//! a floor. AF observes no events, so the floor has no default until the first
//! rung that does (AH) calibrates it.

use serde::{Deserialize, Serialize};

use crate::MetricError;

/// One event: what happened, as one comparable string (`"hit eruptor
/// chompy#3"`, say), and the flip offset it happened at.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Event {
    pub offset: u64,
    pub kind: String,
}

impl Event {
    pub fn new(offset: u64, kind: impl Into<String>) -> Self {
        Self {
            offset,
            kind: kind.into(),
        }
    }
}

/// One run's events in the order they happened: offsets never decrease
/// (events at one flip keep the order the run recorded them in).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "Vec<Event>", into = "Vec<Event>")]
pub struct EventLog {
    events: Vec<Event>,
}

impl EventLog {
    pub fn new(events: Vec<Event>) -> Result<Self, MetricError> {
        for (index, pair) in events.windows(2).enumerate() {
            if pair[1].offset < pair[0].offset {
                return Err(MetricError::EventOrder {
                    index: index + 1,
                    previous: pair[0].offset,
                    offset: pair[1].offset,
                });
            }
        }
        Ok(Self { events })
    }

    pub fn events(&self) -> &[Event] {
        &self.events
    }
}

impl TryFrom<Vec<Event>> for EventLog {
    type Error = MetricError;

    fn try_from(events: Vec<Event>) -> Result<Self, Self::Error> {
        EventLog::new(events)
    }
}

impl From<EventLog> for Vec<Event> {
    fn from(log: EventLog) -> Self {
        log.events
    }
}

/// One event of the agreed prefix and the earliest and latest flip the
/// replays stamped it with.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct StampBand {
    pub kind: String,
    pub earliest: u64,
    pub latest: u64,
}

/// Where the replays stop agreeing: the first index at which one has a
/// different event or has run out while another goes on.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Parting {
    pub index: usize,
    /// The earliest stamp any replay has at that index.
    pub offset: Option<u64>,
}

/// The oracle's event sequence: the replays' agreed prefix, each event with
/// its stamp band, and where they part.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct EventReference {
    replays: usize,
    prefix: Vec<StampBand>,
    parted: Option<Parting>,
}

impl EventReference {
    pub fn from_logs(logs: &[EventLog]) -> Result<Self, MetricError> {
        if logs.is_empty() {
            return Err(MetricError::NoReplays);
        }
        let mut prefix = Vec::new();
        let mut index = 0;
        let parted = loop {
            let at: Vec<Option<&Event>> = logs.iter().map(|l| l.events.get(index)).collect();
            if at.iter().all(Option::is_none) {
                break None;
            }
            let first = at[0];
            let agree = at.iter().all(|e| match (e, first) {
                (Some(e), Some(first)) => e.kind == first.kind,
                _ => false,
            });
            if !agree {
                break Some(Parting {
                    index,
                    offset: at.iter().flatten().map(|e| e.offset).min(),
                });
            }
            let stamps = at.iter().flatten().map(|e| e.offset);
            prefix.push(StampBand {
                kind: first.expect("all agree, so all are some").kind.clone(),
                earliest: stamps.clone().min().expect("at least one replay"),
                latest: stamps.max().expect("at least one replay"),
            });
            index += 1;
        };
        Ok(Self {
            replays: logs.len(),
            prefix,
            parted,
        })
    }

    pub fn replays(&self) -> usize {
        self.replays
    }

    /// The events every replay has, in order.
    pub fn prefix(&self) -> &[StampBand] {
        &self.prefix
    }

    /// `None` when every replay has exactly the prefix.
    pub fn parted(&self) -> Option<&Parting> {
        self.parted.as_ref()
    }

    /// Checks `candidate` against the prefix, each stamp allowed `floor` flips
    /// either side of the replays' earliest and latest. Past the prefix
    /// nothing is judged when the replays parted; when they didn't, an event
    /// beyond it is a mismatch. Comparison stops at the first wrong or missing
    /// event, since every event after it is shifted.
    pub fn check(&self, candidate: &EventLog, floor: u64) -> EventVerdict {
        let mut mismatches = Vec::new();
        let mut compared = 0;
        for (index, expected) in self.prefix.iter().enumerate() {
            let Some(found) = candidate.events.get(index) else {
                mismatches.push(EventMismatch::Missing {
                    index,
                    expected: expected.kind.clone(),
                });
                return EventVerdict {
                    compared,
                    mismatches,
                };
            };
            compared += 1;
            if found.kind != expected.kind {
                mismatches.push(EventMismatch::Kind {
                    index,
                    expected: expected.kind.clone(),
                    found: found.kind.clone(),
                    offset: found.offset,
                });
                return EventVerdict {
                    compared,
                    mismatches,
                };
            }
            let low = expected.earliest.saturating_sub(floor);
            let high = expected.latest.saturating_add(floor);
            if !(low..=high).contains(&found.offset) {
                mismatches.push(EventMismatch::Stamp {
                    index,
                    kind: found.kind.clone(),
                    offset: found.offset,
                    low,
                    high,
                });
            }
        }
        if self.parted.is_none()
            && let Some(extra) = candidate.events.get(self.prefix.len())
        {
            mismatches.push(EventMismatch::Extra {
                index: self.prefix.len(),
                found: extra.kind.clone(),
                offset: extra.offset,
            });
        }
        EventVerdict {
            compared,
            mismatches,
        }
    }
}

/// What [`EventReference::check`] found: a `Kind`, `Missing` or `Extra` ends
/// the comparison, a `Stamp` doesn't.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub enum EventMismatch {
    /// A different event where the prefix has `expected` (one reordered, say).
    Kind {
        index: usize,
        expected: String,
        found: String,
        offset: u64,
    },
    /// The candidate ran out before the prefix did.
    Missing { index: usize, expected: String },
    /// An event past a prefix every replay ended on.
    Extra {
        index: usize,
        found: String,
        offset: u64,
    },
    /// The right event at a flip outside [low, high], the replays' stamps
    /// widened by the floor.
    Stamp {
        index: usize,
        kind: String,
        offset: u64,
        low: u64,
        high: u64,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct EventVerdict {
    /// Prefix events the candidate had an event for.
    pub compared: usize,
    pub mismatches: Vec<EventMismatch>,
}

impl EventVerdict {
    pub fn passed(&self) -> bool {
        self.mismatches.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn log(events: &[(u64, &str)]) -> EventLog {
        EventLog::new(events.iter().map(|&(o, k)| Event::new(o, k)).collect()).unwrap()
    }

    /// Three replays that agree on four events, stamped a flip or two apart.
    fn replays() -> Vec<EventLog> {
        vec![
            log(&[
                (10, "spawn chompy"),
                (20, "hit chompy"),
                (30, "pickup gold"),
                (45, "die chompy"),
            ]),
            log(&[
                (10, "spawn chompy"),
                (21, "hit chompy"),
                (31, "pickup gold"),
                (44, "die chompy"),
            ]),
            log(&[
                (10, "spawn chompy"),
                (22, "hit chompy"),
                (30, "pickup gold"),
                (46, "die chompy"),
            ]),
        ]
    }

    #[test]
    fn the_reference_is_the_agreed_prefix_with_its_stamp_bands() {
        let reference = EventReference::from_logs(&replays()).unwrap();
        assert_eq!(reference.replays(), 3);
        assert_eq!(reference.parted(), None);
        let bands: Vec<_> = reference
            .prefix()
            .iter()
            .map(|b| (b.kind.as_str(), b.earliest, b.latest))
            .collect();
        assert_eq!(
            bands,
            [
                ("spawn chompy", 10, 10),
                ("hit chompy", 20, 22),
                ("pickup gold", 30, 31),
                ("die chompy", 44, 46)
            ]
        );
    }

    #[test]
    fn a_replay_like_candidate_passes() {
        let reference = EventReference::from_logs(&replays()).unwrap();
        let candidate = log(&[
            (10, "spawn chompy"),
            (21, "hit chompy"),
            (30, "pickup gold"),
            (45, "die chompy"),
        ]);
        let verdict = reference.check(&candidate, 0);
        assert!(verdict.passed(), "{verdict:?}");
        assert_eq!(verdict.compared, 4);
    }

    #[test]
    fn an_event_reordered_fails_where_the_order_breaks() {
        let reference = EventReference::from_logs(&replays()).unwrap();
        let candidate = log(&[
            (10, "spawn chompy"),
            (20, "pickup gold"),
            (30, "hit chompy"),
            (45, "die chompy"),
        ]);
        let verdict = reference.check(&candidate, 5);
        assert!(!verdict.passed());
        assert_eq!(
            verdict.mismatches,
            [EventMismatch::Kind {
                index: 1,
                expected: "hit chompy".into(),
                found: "pickup gold".into(),
                offset: 20
            }]
        );
        assert_eq!(verdict.compared, 2);
    }

    #[test]
    fn a_stamp_moved_past_the_spread_fails_and_one_inside_the_floor_passes() {
        let reference = EventReference::from_logs(&replays()).unwrap();
        // The hit's replays stamped 20 to 22; with a floor of 1, 23 passes
        // and 24 doesn't.
        let at = |hit: u64| {
            log(&[
                (10, "spawn chompy"),
                (hit, "hit chompy"),
                (30, "pickup gold"),
                (45, "die chompy"),
            ])
        };
        assert!(reference.check(&at(23), 1).passed());
        assert!(reference.check(&at(19), 1).passed());
        let verdict = reference.check(&at(24), 1);
        assert_eq!(
            verdict.mismatches,
            [EventMismatch::Stamp {
                index: 1,
                kind: "hit chompy".into(),
                offset: 24,
                low: 19,
                high: 23
            }]
        );
        // Every event is still compared past a stamp miss.
        assert_eq!(verdict.compared, 4);
        // Floor 0 holds a stamp to the replays' own spread.
        assert!(!reference.check(&at(23), 0).passed());
    }

    #[test]
    fn a_candidate_short_or_long_fails() {
        let reference = EventReference::from_logs(&replays()).unwrap();
        let short = log(&[(10, "spawn chompy"), (20, "hit chompy")]);
        assert_eq!(
            reference.check(&short, 0).mismatches,
            [EventMismatch::Missing {
                index: 2,
                expected: "pickup gold".into()
            }]
        );
        let long = log(&[
            (10, "spawn chompy"),
            (20, "hit chompy"),
            (30, "pickup gold"),
            (45, "die chompy"),
            (50, "spawn chompy"),
        ]);
        assert_eq!(
            reference.check(&long, 0).mismatches,
            [EventMismatch::Extra {
                index: 4,
                found: "spawn chompy".into(),
                offset: 50
            }]
        );
    }

    #[test]
    fn replays_that_part_report_where_and_judge_only_the_prefix() {
        let mut logs = replays();
        // The third replay's chompy dies before the pickup.
        logs[2] = log(&[
            (10, "spawn chompy"),
            (22, "hit chompy"),
            (29, "die chompy"),
            (33, "pickup gold"),
        ]);
        let reference = EventReference::from_logs(&logs).unwrap();
        assert_eq!(reference.prefix().len(), 2);
        assert_eq!(
            reference.parted(),
            Some(&Parting {
                index: 2,
                offset: Some(29)
            })
        );
        // Either continuation passes: past the parting nothing is judged.
        let candidate = log(&[(10, "spawn chompy"), (21, "hit chompy"), (40, "anything")]);
        assert!(reference.check(&candidate, 0).passed());
        // One replay running out where the others go on is a parting too.
        let mut logs = replays();
        logs[1] = log(&[
            (10, "spawn chompy"),
            (21, "hit chompy"),
            (31, "pickup gold"),
        ]);
        let reference = EventReference::from_logs(&logs).unwrap();
        assert_eq!(
            reference.parted(),
            Some(&Parting {
                index: 3,
                offset: Some(45)
            })
        );
        assert_eq!(EventReference::from_logs(&[]), Err(MetricError::NoReplays));
    }

    #[test]
    fn a_log_refuses_events_out_of_order_and_round_trips() {
        assert_eq!(
            EventLog::new(vec![
                Event::new(20, "a"),
                Event::new(20, "b"),
                Event::new(19, "c")
            ]),
            Err(MetricError::EventOrder {
                index: 2,
                previous: 20,
                offset: 19
            })
        );
        let original = replays().remove(0);
        let json = serde_json::to_string(&original).unwrap();
        assert_eq!(serde_json::from_str::<EventLog>(&json).unwrap(), original);
        assert!(
            serde_json::from_str::<EventLog>(
                r#"[{"offset":5,"kind":"a"},{"offset":4,"kind":"b"}]"#
            )
            .is_err()
        );
    }
}
