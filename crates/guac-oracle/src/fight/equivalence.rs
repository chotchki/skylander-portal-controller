//! Decision 17's equivalence check (openspec AH.6): before any event is
//! compared, guacamole's state at an anchor is read against AH.4's reads at
//! the oracle's, field by field over a named list, and a mismatch fails the
//! comparison on its own. The hero's pose is held to AF.12's floors
//! ([`crate::tolerance::POSITION`], [`crate::tolerance::ANGLE`]) per
//! component, angles wrapped; everything else is equal or not.

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

use crate::motion::wrap;
use crate::tolerance;

/// A whitelisted placement's state as the check reads it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PlacementState {
    pub behaviour: String,
    pub alerted: bool,
}

/// Decision 17's named list at an anchor.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AnchorState {
    /// Metres.
    pub hero_position: [f64; 3],
    /// Tilt, heading and roll, degrees.
    pub hero_rotation: [f64; 3],
    pub experience: i64,
    pub gold: i64,
    /// Every whitelisted placement, by name.
    pub placements: BTreeMap<String, PlacementState>,
    /// The triggers fired, by name.
    pub triggers: BTreeSet<String>,
    /// The loot accumulator's index, as the game holds it.
    pub loot_index: f32,
}

/// A field the candidate doesn't hold as the oracle does.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub enum Difference {
    /// A position component further off than the floor, metres.
    Position {
        axis: usize,
        by: f64,
    },
    /// A rotation component further off than the floor, degrees.
    Rotation {
        axis: usize,
        by: f64,
    },
    Experience {
        oracle: i64,
        candidate: i64,
    },
    Gold {
        oracle: i64,
        candidate: i64,
    },
    Behaviour {
        placement: String,
        oracle: String,
        candidate: String,
    },
    Alerted {
        placement: String,
        oracle: bool,
    },
    /// A placement on one side's list and not the other's.
    Placement {
        placement: String,
        in_oracle: bool,
    },
    Triggers {
        oracle_only: Vec<String>,
        candidate_only: Vec<String>,
    },
    LootIndex {
        oracle: f32,
        candidate: f32,
    },
}

/// Every field held differently, in the list's order; empty is equivalent.
pub fn equivalence(oracle: &AnchorState, candidate: &AnchorState) -> Vec<Difference> {
    let mut out = Vec::new();
    for axis in 0..3 {
        let by = (candidate.hero_position[axis] - oracle.hero_position[axis]).abs();
        if by.is_nan() || by > tolerance::POSITION.floor {
            out.push(Difference::Position { axis, by });
        }
    }
    for axis in 0..3 {
        let by = wrap(candidate.hero_rotation[axis] - oracle.hero_rotation[axis]).abs();
        if by.is_nan() || by > tolerance::ANGLE.floor {
            out.push(Difference::Rotation { axis, by });
        }
    }
    if candidate.experience != oracle.experience {
        out.push(Difference::Experience {
            oracle: oracle.experience,
            candidate: candidate.experience,
        });
    }
    if candidate.gold != oracle.gold {
        out.push(Difference::Gold {
            oracle: oracle.gold,
            candidate: candidate.gold,
        });
    }
    let names: BTreeSet<&String> = oracle
        .placements
        .keys()
        .chain(candidate.placements.keys())
        .collect();
    for name in names {
        match (oracle.placements.get(name), candidate.placements.get(name)) {
            (Some(o), Some(c)) => {
                if o.behaviour != c.behaviour {
                    out.push(Difference::Behaviour {
                        placement: name.clone(),
                        oracle: o.behaviour.clone(),
                        candidate: c.behaviour.clone(),
                    });
                }
                if o.alerted != c.alerted {
                    out.push(Difference::Alerted {
                        placement: name.clone(),
                        oracle: o.alerted,
                    });
                }
            }
            (o, _) => out.push(Difference::Placement {
                placement: name.clone(),
                in_oracle: o.is_some(),
            }),
        }
    }
    if candidate.triggers != oracle.triggers {
        out.push(Difference::Triggers {
            oracle_only: oracle
                .triggers
                .difference(&candidate.triggers)
                .cloned()
                .collect(),
            candidate_only: candidate
                .triggers
                .difference(&oracle.triggers)
                .cloned()
                .collect(),
        });
    }
    if candidate.loot_index.to_bits() != oracle.loot_index.to_bits() {
        out.push(Difference::LootIndex {
            oracle: oracle.loot_index,
            candidate: candidate.loot_index,
        });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn anchor() -> AnchorState {
        AnchorState {
            hero_position: [-2.1, 22.38, -61.4],
            hero_rotation: [0.0, 179.99996, 0.0],
            experience: 0,
            gold: 3,
            placements: BTreeMap::from([
                (
                    "Enemy_Chompy(1)".into(),
                    PlacementState {
                        behaviour: "Asleep".into(),
                        alerted: false,
                    },
                ),
                (
                    "Norticus".into(),
                    PlacementState {
                        behaviour: "Cower".into(),
                        alerted: false,
                    },
                ),
            ]),
            triggers: BTreeSet::from([
                "Setup_001_Trigger".into(),
                "Activate_Norticus and Chompies".into(),
            ]),
            loot_index: 0.4,
        }
    }

    #[test]
    fn equivalence_passes_and_fails_on_each_field_alone() {
        let oracle = anchor();
        // Inside the floors: 9 cm along x, a heading across the wrap 8e-5 off.
        let mut close = anchor();
        close.hero_position[0] += 0.09;
        close.hero_rotation[1] = -179.99996;
        assert_eq!(equivalence(&oracle, &close), []);
        type Change = Box<dyn Fn(&mut AnchorState)>;
        let off: Vec<(&str, Change)> = vec![
            ("position", Box::new(|a| a.hero_position[2] += 0.11)),
            ("rotation", Box::new(|a| a.hero_rotation[1] += 2e-4)),
            ("experience", Box::new(|a| a.experience += 1)),
            ("gold", Box::new(|a| a.gold -= 1)),
            (
                "behaviour",
                Box::new(|a| a.placements.get_mut("Norticus").unwrap().behaviour = "Run".into()),
            ),
            (
                "alerted",
                Box::new(|a| a.placements.get_mut("Enemy_Chompy(1)").unwrap().alerted = true),
            ),
            (
                "placement",
                Box::new(|a| _ = a.placements.remove("Norticus")),
            ),
            (
                "triggers",
                Box::new(|a| _ = a.triggers.remove("Setup_001_Trigger")),
            ),
            ("loot", Box::new(|a| a.loot_index = 0.4f32.next_up())),
        ];
        for (field, change) in off {
            let mut candidate = anchor();
            change(&mut candidate);
            let found = equivalence(&oracle, &candidate);
            assert_eq!(found.len(), 1, "{field}: {found:?}");
            let named = match &found[0] {
                Difference::Position { axis: 2, .. } => "position",
                Difference::Rotation { axis: 1, .. } => "rotation",
                Difference::Experience { .. } => "experience",
                Difference::Gold { .. } => "gold",
                Difference::Behaviour { .. } => "behaviour",
                Difference::Alerted { .. } => "alerted",
                Difference::Placement {
                    in_oracle: true, ..
                } => "placement",
                Difference::Triggers { .. } => "triggers",
                Difference::LootIndex { .. } => "loot",
                other => panic!("{field}: {other:?}"),
            };
            assert_eq!(named, field);
        }
        // A value that isn't a number is never equivalent.
        let mut nan = anchor();
        nan.hero_position[1] = f64::NAN;
        assert!(matches!(
            equivalence(&oracle, &nan)[..],
            [Difference::Position { axis: 1, .. }]
        ));
    }
}
