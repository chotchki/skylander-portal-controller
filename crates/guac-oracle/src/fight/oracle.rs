//! The oracle's side of a fight trace: what O6 adds to each O5 record and how
//! the two become [`FightTrace`] ticks (openspec AH.6, for AH.5 to fill).
//!
//! O6 doesn't exist yet. AH.5's reader decodes each record's chain (the actor
//! list followed from a base by pointer hops, kept by a filter, each kept
//! item's ranges copied through further hops) into a [`ChainRecord`] riding
//! the O5 record it was copied with, one for one. An item's identity is its
//! address while it stays in the list, its kind and, when the harness names
//! one, its [`ChainItem::serial`]: an address gone for a record and back is a
//! new actor, and so is one listed under another kind or serial. Without a
//! serial, a slot freed and taken by an actor of the same kind between two
//! records reads as one actor that jumped (an orb collected as another
//! spawns, with a pooled allocator), which a pickup or a spawn then miscounts:
//! AH.5 copies a word that changes on reuse where the actor has one. The
//! harness names what the filter matched (`kind`), resolves a placed actor's
//! name checksum to its placement (and leaves a clone's `None`, though it
//! carries its template's checksum, or every clone is named by the template
//! rather than by its parent) and decodes the ranges; the behaviour cell
//! stays raw here and resolves to the behaviour's name through [`Names`],
//! built from the extract, so both sides compare names.
//!
//! The hero is O5's: his body and clip come from the record, and an item of
//! kind [`HERO`] in the chain (his script variables, his behaviour) joins him
//! rather than standing apart. Game time is AG.1's: [`Recording::game_time`]
//! under [`Alignment::Earlier`], records paired by `_curFrame`, a lost or
//! repeated game frame refused.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use super::draws::MtWords;
use super::ticks::{Actor, FightTrace, HERO, SpawnRule, Tick};
use crate::MetricError;
use crate::recording::{Alignment, Body, Recording};

/// An actor O6's filter kept, as AH.5's reader decodes its copied ranges.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ChainItem {
    /// The list's entry: the actor, while it stays listed.
    pub address: u32,
    /// The filter entry it matched, as the harness names it (`"chompy"`,
    /// `"orb"`, [`HERO`]).
    pub kind: String,
    /// The placement whose name checksum it carries; `None` for one spawned
    /// (a clone carries its template's checksum and still reads `None`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub placement: Option<String>,
    /// A word O6 copies that changes when the list's slot is taken by
    /// another actor (the actor's own id or a spawn count), when the harness
    /// names one: the same address under another serial is a new actor.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub serial: Option<u32>,
    /// Transform and velocity off its corpus; `None` when a hop to them was
    /// unreadable.
    pub body: Option<Body>,
    /// The behaviour cell as read; `None` when unreadable.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub behaviour: Option<u32>,
    /// Script variables through the second hop, by name; one unreadable is
    /// left out (and its bit set in `missing`).
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub vars: BTreeMap<String, f32>,
    /// A bit per named range O6 couldn't read, in the harness's order.
    #[serde(default)]
    pub missing: u64,
}

/// What AH.5 adds to one O5 record: the chain's kept items and the
/// generator's watched words.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ChainRecord {
    /// The O5 record it rode in.
    pub flip: u64,
    pub read: u32,
    pub game_frame: u32,
    pub items: Vec<ChainItem>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mt: Option<MtWords>,
}

/// The names behind raw cells, from the extract: each kind's behaviour cells
/// and the hero's clip ids. An empty clip table leaves the hero's clip
/// unnamed; a cell or id missing from a table that isn't empty is refused.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Names {
    pub behaviours: BTreeMap<String, BTreeMap<u32, String>>,
    pub clips: BTreeMap<u32, String>,
}

impl Names {
    fn behaviour(&self, kind: &str, cell: u32, frame: u32) -> Result<String, MetricError> {
        self.behaviours
            .get(kind)
            .and_then(|cells| cells.get(&cell))
            .cloned()
            .ok_or_else(|| MetricError::UnknownBehaviour {
                kind: kind.to_string(),
                cell,
                frame,
            })
    }
}

fn from_body(body: &Body) -> ([f64; 3], f64, [f64; 3]) {
    (
        body.position.map(f64::from),
        f64::from(body.rotation[1]),
        body.velocity.map(f64::from),
    )
}

/// The oracle's ticks from an O5 recording and the chain records riding it,
/// one per record, each actor named by `spawns`. Refused: a lost or repeated
/// game frame ([`Recording::game_time`]'s refusals), a chain record that
/// isn't its O5 record's ([`MetricError::ChainMismatch`]), an item listed
/// twice, an item whose body didn't read ([`MetricError::Unreadable`]: the
/// whole recording, so AH.5's chain keeps a dying actor's body readable to
/// its last listing or the replay is lost), and a cell the names don't hold.
pub fn fight_trace(
    recording: &Recording,
    chain: &[ChainRecord],
    names: &Names,
    spawns: &[SpawnRule],
) -> Result<FightTrace, MetricError> {
    let times = recording.game_time(Alignment::Earlier)?;
    let frames = recording.frames();
    if chain.len() != frames.len() {
        return Err(MetricError::ChainMismatch {
            index: chain.len().min(frames.len()),
        });
    }
    let origin = frames[0].game_frame;
    // Address -> (id, kind, serial) for the items the record before listed.
    type Listed = (u64, String, Option<u32>);
    let mut listed: BTreeMap<u32, Listed> = BTreeMap::new();
    let mut next_id: u64 = 1;
    let mut ticks = Vec::with_capacity(frames.len());
    for (index, ((frame, record), &time)) in frames.iter().zip(chain).zip(&times).enumerate() {
        if (record.flip, record.read, record.game_frame)
            != (frame.flip, frame.read, frame.game_frame)
        {
            return Err(MetricError::ChainMismatch { index });
        }
        let at = frame.game_frame.wrapping_sub(origin);
        let (position, heading, velocity) = from_body(&frame.hero);
        let clip = match (frame.top_clip(), names.clips.is_empty()) {
            (Some(c), false) => Some(names.clips.get(&c.id).cloned().ok_or(
                MetricError::UnknownClip {
                    id: c.id,
                    frame: at,
                },
            )?),
            _ => None,
        };
        let mut hero = Actor {
            id: 0,
            kind: HERO.into(),
            placement: Some(HERO.into()),
            position,
            heading,
            velocity,
            behaviour: None,
            clip,
            vars: BTreeMap::new(),
        };
        let mut actors = Vec::with_capacity(record.items.len() + 1);
        let mut now: BTreeMap<u32, Listed> = BTreeMap::new();
        for item in &record.items {
            let behaviour = item
                .behaviour
                .map(|cell| names.behaviour(&item.kind, cell, at))
                .transpose()?;
            let vars = item
                .vars
                .iter()
                .map(|(k, v)| (k.clone(), f64::from(*v)))
                .collect();
            if item.kind == HERO {
                hero.behaviour = behaviour;
                hero.vars = vars;
                continue;
            }
            let Some(body) = &item.body else {
                return Err(MetricError::Unreadable {
                    frame: at,
                    address: item.address,
                });
            };
            let id = match listed.get(&item.address) {
                Some((id, kind, serial)) if *kind == item.kind && *serial == item.serial => *id,
                _ => {
                    next_id += 1;
                    next_id - 1
                }
            };
            if now
                .insert(item.address, (id, item.kind.clone(), item.serial))
                .is_some()
            {
                return Err(MetricError::DuplicateActor { frame: at, id });
            }
            let (position, heading, velocity) = from_body(body);
            actors.push(Actor {
                id,
                kind: item.kind.clone(),
                placement: item.placement.clone(),
                position,
                heading,
                velocity,
                behaviour,
                clip: None,
                vars,
            });
        }
        listed = now;
        actors.insert(0, hero);
        ticks.push(Tick {
            frame: at,
            time,
            buttons: frame.pad.map(|p| p.state().buttons),
            mt: record.mt,
            actors,
        });
    }
    FightTrace::new(ticks, spawns)
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::input::{Button, Ds3Pad};
    use crate::recording::{Clock, Frame};

    pub(crate) const NOMINAL: f32 = 1.0 / 60.0;

    /// An O5 record at game frame 6000 + k, the hero at `x`, `delta` its
    /// measured frame time and South held when `south`.
    pub(crate) fn record(k: u64, delta: f32, x: f32, south: bool) -> Frame {
        Frame {
            flip: 100 + k,
            read: 0,
            host_us: k * 16_667,
            game_frame: 6_000 + k as u32,
            clock: Clock {
                frame_delta: delta,
                presentation_rate: NOMINAL,
                logic_scale: 1.0,
                logic_stopped: false,
                object_scale: 1.0,
            },
            hero: Body {
                position: [x, 22.4, -60.0],
                rotation: [0.0, 90.0, 0.0],
                velocity: [0.0; 3],
            },
            camera: None,
            stick: [0.0, 0.0],
            pad: Some(Ds3Pad {
                digital2: if south { 0x40 } else { 0 },
                ..Ds3Pad::REST
            }),
            clips: Vec::new(),
            contacts: None,
            mover: None,
        }
    }

    pub(crate) fn item(address: u32, kind: &str, placement: Option<&str>, x: f32) -> ChainItem {
        ChainItem {
            address,
            kind: kind.into(),
            placement: placement.map(Into::into),
            serial: None,
            body: Some(Body {
                position: [x, 22.4, -60.0],
                rotation: [0.0, 0.0, 0.0],
                velocity: [0.0; 3],
            }),
            behaviour: None,
            vars: BTreeMap::new(),
            missing: 0,
        }
    }

    pub(crate) fn chained(frames: &[Frame], items: Vec<Vec<ChainItem>>) -> Vec<ChainRecord> {
        frames
            .iter()
            .zip(items)
            .map(|(f, items)| ChainRecord {
                flip: f.flip,
                read: f.read,
                game_frame: f.game_frame,
                items,
                mt: None,
            })
            .collect()
    }

    fn names() -> Names {
        Names {
            behaviours: BTreeMap::from([(
                "chompy".into(),
                BTreeMap::from([(0x10, "Asleep".into()), (0x20, "Alerted".into())]),
            )]),
            clips: BTreeMap::new(),
        }
    }

    #[test]
    fn a_recording_and_its_chain_make_named_ticks_on_game_time() {
        // Record 2's frame took 0.2 s: a hitch, one step of three nominal
        // frames.
        let frames: Vec<Frame> = [NOMINAL, NOMINAL, 0.2, NOMINAL]
            .iter()
            .enumerate()
            .map(|(k, &d)| record(k as u64, d, 10.0, k == 1))
            .collect();
        let chompy = |cell| {
            let mut i = item(0x8000_1000, "chompy", Some("Enemy_Chompy(1)"), 12.0);
            i.behaviour = Some(cell);
            i.vars.insert("health".into(), 1.0);
            i
        };
        let mut hero = item(0x8000_2000, HERO, None, 0.0);
        hero.body = None;
        hero.vars.insert("experience".into(), 16.0);
        let items = vec![
            vec![chompy(0x10), hero.clone()],
            vec![chompy(0x10), hero.clone()],
            vec![chompy(0x20), hero.clone()],
            vec![hero],
        ];
        let recording = Recording::new(frames.clone()).unwrap();
        let trace =
            fight_trace(&recording, &chained(&frames, items.clone()), &names(), &[]).unwrap();
        let times: Vec<f64> = trace.ticks().iter().map(|t| t.time).collect();
        let n = f64::from(NOMINAL);
        let hitch = f64::from(NOMINAL * 3.0);
        assert_eq!(times, [0.0, n, 2.0 * n, 2.0 * n + hitch]);
        assert_eq!(trace.ticks()[3].frame, 3);
        assert_eq!(
            trace.ticks()[1]
                .buttons
                .as_ref()
                .map(|b| b.contains(&Button::South)),
            Some(true)
        );
        let c = trace.track("Enemy_Chompy(1)").unwrap();
        assert_eq!(
            c.entries(&super::super::ticks::Cue::Behaviour("Alerted".into())),
            [2]
        );
        assert_eq!(c.gone(), Some(3));
        // The hero's chain item joins O5's hero, body and all.
        let h = trace.track(HERO).unwrap();
        assert_eq!(h.at(0).unwrap().vars["experience"], 16.0);
        assert_eq!(h.at(0).unwrap().position[0], 10.0);
        // A lost game frame, a chain out of step, an unknown cell and an
        // unreadable body are each refused.
        let mut lost = frames.clone();
        lost[3].game_frame += 1;
        let lost_rec = Recording::new(lost.clone()).unwrap();
        assert!(matches!(
            fight_trace(&lost_rec, &chained(&lost, items.clone()), &names(), &[]),
            Err(MetricError::FrameGap { .. })
        ));
        let mut skewed = chained(&frames, items.clone());
        skewed[2].flip += 1;
        assert_eq!(
            fight_trace(&recording, &skewed, &names(), &[]),
            Err(MetricError::ChainMismatch { index: 2 })
        );
        let mut unknown = items.clone();
        unknown[1][0].behaviour = Some(0x30);
        assert!(matches!(
            fight_trace(&recording, &chained(&frames, unknown), &names(), &[]),
            Err(MetricError::UnknownBehaviour {
                cell: 0x30,
                frame: 1,
                ..
            })
        ));
        let mut unread = items;
        unread[2][0].body = None;
        assert_eq!(
            fight_trace(&recording, &chained(&frames, unread), &names(), &[]),
            Err(MetricError::Unreadable {
                frame: 2,
                address: 0x8000_1000
            })
        );
    }

    #[test]
    fn an_address_back_in_the_list_is_a_new_actor() {
        let frames: Vec<Frame> = (0..3).map(|k| record(k, NOMINAL, 0.0, false)).collect();
        let recording = Recording::new(frames.clone()).unwrap();
        // An orb freed at record 1 and its slot reused by another at 2.
        let items = vec![
            vec![item(0x9000, "orb", None, 1.0)],
            vec![],
            vec![item(0x9000, "orb", None, 3.0)],
        ];
        let trace =
            fight_trace(&recording, &chained(&frames, items), &Names::default(), &[]).unwrap();
        let orbs: Vec<(&str, usize, usize)> = trace
            .tracks()
            .filter(|t| t.kind() == "orb")
            .map(|t| (t.name(), t.first(), t.last()))
            .collect();
        assert_eq!(orbs, [("orb#1", 0, 0), ("orb#2", 2, 2)]);
    }

    /// The review's cases: a slot taken between two records, by another kind
    /// or, told by the serial, by another of the same kind, is a new actor;
    /// without a serial a same-kind reuse can't be told from one actor.
    #[test]
    fn a_slot_taken_between_records_is_a_new_actor_by_kind_or_serial() {
        let frames: Vec<Frame> = (0..4).map(|k| record(k, NOMINAL, 0.0, false)).collect();
        let recording = Recording::new(frames.clone()).unwrap();
        let serial = |x: f32, s: u32| {
            let mut i = item(0x9000, "orb", None, x);
            i.serial = Some(s);
            i
        };
        let items = vec![
            vec![serial(1.0, 7)],
            vec![serial(1.5, 7)],
            // Collected, and its slot taken by a new orb in the same frame.
            vec![serial(4.0, 8)],
            // Then by a coin.
            vec![item(0x9000, "coin", None, 6.0)],
        ];
        let trace =
            fight_trace(&recording, &chained(&frames, items), &Names::default(), &[]).unwrap();
        let tracks: Vec<(&str, usize, usize)> = trace
            .tracks()
            .filter(|t| t.kind() != HERO)
            .map(|t| (t.name(), t.first(), t.last()))
            .collect();
        assert_eq!(tracks, [("coin#1", 3, 3), ("orb#1", 0, 1), ("orb#2", 2, 2)]);
        // No serial: the same reuse reads as one orb that jumped; a coin
        // taking the slot is still told by its kind.
        let items = vec![
            vec![item(0x9000, "orb", None, 1.0)],
            vec![item(0x9000, "orb", None, 1.5)],
            vec![item(0x9000, "orb", None, 4.0)],
            vec![item(0x9000, "coin", None, 6.0)],
        ];
        let trace =
            fight_trace(&recording, &chained(&frames, items), &Names::default(), &[]).unwrap();
        let tracks: Vec<(&str, usize, usize)> = trace
            .tracks()
            .filter(|t| t.kind() != HERO)
            .map(|t| (t.name(), t.first(), t.last()))
            .collect();
        assert_eq!(tracks, [("coin#1", 3, 3), ("orb#1", 0, 2)]);
    }
}
