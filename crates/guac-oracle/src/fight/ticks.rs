//! The per-frame trace a fight's metrics read, the one shape both sides fill:
//! the oracle's from an O5 recording and O6's chain ([`super::oracle`]),
//! guacamole's headless runner (AG.17) directly, a tick per game frame.
//!
//! An actor carries the cells both sides can name: its kind, the placement
//! it stands for (the hero is [`HERO`]), its pivot, facing and velocity, its
//! behaviour and clip by name (the oracle's cells resolved through the
//! extract, the runner's VM emitting the names) and its script variables by
//! name. Its `id` is the run's own and means nothing across runs, so the
//! trace names every actor the same way on both sides: a placed one by its
//! placement, a spawned one by the actor that spawned it (the nearest of the
//! kinds a [`SpawnRule`] lists, a tick before) and its count among that
//! parent's children of its kind (`Enemy_Chompy(2)>orb#3`), else by its
//! count among its kind (`orb#3`). Spawns in one tick count in order of
//! where they appear, so a run's own ids never decide a name.

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

use super::draws::MtWords;
use crate::MetricError;
use crate::input::Button;

/// The hero's kind and placement name.
pub const HERO: &str = "hero";

/// When something happened: game seconds and game frames since the trace's
/// first tick.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Stamp {
    pub time: f64,
    pub frame: u32,
}

/// What a tick holds of one actor: the cells both sides can fill.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Actor {
    /// The run's own identity for it: the same at every tick it lives
    /// through, never another actor's in the same run.
    pub id: u64,
    /// What it is, as the run names it (`"chompy"`, `"orb"`, [`HERO`]).
    pub kind: String,
    /// The placement it stands for, or `None` for an actor spawned in play.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub placement: Option<String>,
    /// Its pivot, metres.
    pub position: [f64; 3],
    /// Its facing, degrees (θ faces (sin θ, 0, cos θ)).
    pub heading: f64,
    /// m/s.
    pub velocity: [f64; 3],
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub behaviour: Option<String>,
    /// The clip at the top of its blend.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub clip: Option<String>,
    /// Script variables by name (health, an orb's worth, the figure's
    /// experience and gold on the hero), those read at this tick.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub vars: BTreeMap<String, f64>,
}

impl Actor {
    fn finite(&self) -> bool {
        self.position
            .iter()
            .chain(&self.velocity)
            .chain([&self.heading])
            .chain(self.vars.values())
            .all(|v| v.is_finite())
    }
}

/// One game frame: the state as the update after it starts from.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Tick {
    /// Game frames since F0.
    pub frame: u32,
    /// Game seconds since F0: the sum of the steps before it, a hitch one
    /// long step (AG.1's [`crate::recording::Clock::step`] on the oracle's
    /// side).
    pub time: f64,
    /// The hero's pad, the buttons the update after this tick reads; `None`
    /// when the run didn't record it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub buttons: Option<BTreeSet<Button>>,
    /// The generator's watched words (the oracle's only).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mt: Option<MtWords>,
    pub actors: Vec<Actor>,
}

/// How a spawned actor of `kind` finds its parent: the nearest actor of one
/// of `parents`' kinds a tick before it appeared, within `radius` metres.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SpawnRule {
    pub kind: String,
    pub parents: Vec<String>,
    pub radius: f64,
}

/// A condition on an actor's cells, entered at the tick it starts to hold.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Cue {
    Behaviour(String),
    Clip(String),
    /// A script variable at or under a value (health 0, say).
    AtMost {
        var: String,
        value: f64,
    },
    /// The actor leaving the trace: entered at the tick after its last.
    Gone,
}

impl Cue {
    pub fn holds(&self, actor: &Actor) -> bool {
        match self {
            Cue::Behaviour(b) => actor.behaviour.as_deref() == Some(b.as_str()),
            Cue::Clip(c) => actor.clip.as_deref() == Some(c.as_str()),
            Cue::AtMost { var, value } => actor.vars.get(var).is_some_and(|v| v <= value),
            Cue::Gone => false,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
struct TrackInfo {
    id: u64,
    name: String,
    kind: String,
    parent: Option<String>,
    first: usize,
    /// Its index in each tick's actors, from `first` on.
    slots: Vec<usize>,
}

/// A run's ticks, checked and every actor named.
#[derive(Debug, Clone, PartialEq)]
pub struct FightTrace {
    ticks: Vec<Tick>,
    tracks: Vec<TrackInfo>,
}

/// One actor over the ticks it lives through.
#[derive(Debug, Clone, Copy)]
pub struct Track<'a> {
    info: &'a TrackInfo,
    ticks: &'a [Tick],
}

impl<'a> Track<'a> {
    pub fn name(&self) -> &'a str {
        &self.info.name
    }

    pub fn kind(&self) -> &'a str {
        &self.info.kind
    }

    /// The actor that spawned it, by name.
    pub fn parent(&self) -> Option<&'a str> {
        self.info.parent.as_deref()
    }

    pub fn first(&self) -> usize {
        self.info.first
    }

    pub fn last(&self) -> usize {
        self.info.first + self.info.slots.len() - 1
    }

    /// It appeared after the trace's first tick.
    pub fn spawned(&self) -> bool {
        self.info.first > 0
    }

    /// The tick it's gone at, when it leaves before the trace ends.
    pub fn gone(&self) -> Option<usize> {
        let after = self.last() + 1;
        (after < self.ticks.len()).then_some(after)
    }

    pub fn at(&self, tick: usize) -> Option<&'a Actor> {
        let slot = tick
            .checked_sub(self.info.first)
            .and_then(|k| self.info.slots.get(k))?;
        Some(&self.ticks[tick].actors[*slot])
    }

    /// (tick, state) over its life.
    pub fn states(&self) -> impl Iterator<Item = (usize, &'a Actor)> + 'a {
        let (first, ticks) = (self.info.first, self.ticks);
        self.info
            .slots
            .iter()
            .enumerate()
            .map(move |(k, &slot)| (first + k, &ticks[first + k].actors[slot]))
    }

    /// The ticks `cue` is entered at: where it holds and didn't the tick
    /// before. Holding at the actor's first tick counts only for an actor
    /// spawned then; one standing from F0 was already in it.
    pub fn entries(&self, cue: &Cue) -> Vec<usize> {
        if *cue == Cue::Gone {
            return self.gone().into_iter().collect();
        }
        let mut out = Vec::new();
        let mut before: Option<bool> = None;
        for (t, a) in self.states() {
            let now = cue.holds(a);
            let entered = match before {
                None => now && self.spawned(),
                Some(was) => now && !was,
            };
            if entered {
                out.push(t);
            }
            before = Some(now);
        }
        out
    }

    /// Each unbroken run of ticks `cue` holds over, first and last.
    pub fn spans(&self, cue: &Cue) -> Vec<(usize, usize)> {
        let mut out: Vec<(usize, usize)> = Vec::new();
        for (t, a) in self.states() {
            if !cue.holds(a) {
                continue;
            }
            match out.last_mut() {
                Some(span) if span.1 + 1 == t => span.1 = t,
                _ => out.push((t, t)),
            }
        }
        out
    }
}

impl FightTrace {
    /// Refuses no ticks, a frame that isn't the one after the tick before
    /// (a lost frame), a time that goes back or isn't finite, two actors of
    /// one id in a tick, an id whose kind or placement changes, and an id
    /// that comes back after leaving. Then names every actor.
    pub fn new(ticks: Vec<Tick>, spawns: &[SpawnRule]) -> Result<Self, MetricError> {
        if ticks.is_empty() {
            return Err(MetricError::EmptyTrace);
        }
        for (index, pair) in ticks.windows(2).enumerate() {
            let (a, b) = (&pair[0], &pair[1]);
            if a.frame.checked_add(1) != Some(b.frame) {
                return Err(MetricError::TickGap {
                    index: index + 1,
                    previous: a.frame,
                    frame: b.frame,
                });
            }
            if b.time < a.time {
                return Err(MetricError::TimeOrder {
                    index: index + 1,
                    previous: a.time,
                    time: b.time,
                });
            }
        }
        let mut tracks: Vec<TrackInfo> = Vec::new();
        let mut live: BTreeMap<u64, usize> = BTreeMap::new();
        let mut ended: BTreeSet<u64> = BTreeSet::new();
        for (t, tick) in ticks.iter().enumerate() {
            if !tick.time.is_finite() {
                return Err(MetricError::NotFinite {
                    offset: u64::from(tick.frame),
                    component: 0,
                });
            }
            let mut seen = BTreeSet::new();
            for (slot, actor) in tick.actors.iter().enumerate() {
                if !actor.finite() {
                    return Err(MetricError::NotFinite {
                        offset: u64::from(tick.frame),
                        component: slot,
                    });
                }
                if !seen.insert(actor.id) {
                    return Err(MetricError::DuplicateActor {
                        frame: tick.frame,
                        id: actor.id,
                    });
                }
                if ended.contains(&actor.id) {
                    return Err(MetricError::ActorReturned {
                        frame: tick.frame,
                        id: actor.id,
                    });
                }
                match live.get(&actor.id) {
                    Some(&k) => {
                        let info = &mut tracks[k];
                        let was = &ticks[info.first].actors[info.slots[0]];
                        if was.kind != actor.kind || was.placement != actor.placement {
                            return Err(MetricError::ActorChanged {
                                frame: tick.frame,
                                id: actor.id,
                            });
                        }
                        info.slots.push(slot);
                    }
                    None => {
                        live.insert(actor.id, tracks.len());
                        tracks.push(TrackInfo {
                            id: actor.id,
                            name: String::new(),
                            kind: actor.kind.clone(),
                            parent: None,
                            first: t,
                            slots: vec![slot],
                        });
                    }
                }
            }
            let left: Vec<u64> = live
                .keys()
                .filter(|id| !seen.contains(id))
                .copied()
                .collect();
            for id in left {
                live.remove(&id);
                ended.insert(id);
            }
        }
        name_tracks(&ticks, &mut tracks, spawns);
        tracks.sort_by(|a, b| a.name.cmp(&b.name));
        Ok(Self { ticks, tracks })
    }

    pub fn ticks(&self) -> &[Tick] {
        &self.ticks
    }

    pub fn stamp(&self, tick: usize) -> Stamp {
        Stamp {
            time: self.ticks[tick].time,
            frame: self.ticks[tick].frame,
        }
    }

    /// Every actor, in name order.
    pub fn tracks(&self) -> impl Iterator<Item = Track<'_>> {
        self.tracks.iter().map(|info| Track {
            info,
            ticks: &self.ticks,
        })
    }

    pub fn track(&self, name: &str) -> Option<Track<'_>> {
        self.tracks().find(|t| t.name() == name)
    }

    /// The name the trace gave a run's id.
    pub fn name_of(&self, id: u64) -> Option<&str> {
        self.tracks
            .iter()
            .find(|t| t.id == id)
            .map(|t| t.name.as_str())
    }
}

fn place_order(a: &Actor, b: &Actor) -> std::cmp::Ordering {
    let key = |x: &Actor| {
        [
            x.position[0],
            x.position[2],
            x.position[1],
            x.velocity[0],
            x.velocity[2],
            x.velocity[1],
        ]
    };
    key(a)
        .iter()
        .zip(key(b))
        .map(|(p, q)| p.total_cmp(&q))
        .find(|o| o.is_ne())
        .unwrap_or_else(|| a.id.cmp(&b.id))
}

fn distance(a: [f64; 3], b: [f64; 3]) -> f64 {
    ((a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2) + (a[2] - b[2]).powi(2)).sqrt()
}

fn actor_at<'t>(ticks: &'t [Tick], info: &TrackInfo, t: usize) -> &'t Actor {
    &ticks[t].actors[info.slots[t - info.first]]
}

fn name_tracks(ticks: &[Tick], tracks: &mut [TrackInfo], spawns: &[SpawnRule]) {
    let actor = |info: &TrackInfo, t: usize| actor_at(ticks, info, t);
    let mut taken: BTreeMap<String, usize> = BTreeMap::new();
    let mut children: BTreeMap<(String, String), usize> = BTreeMap::new();
    let mut orphans: BTreeMap<String, usize> = BTreeMap::new();
    for t in 0..ticks.len() {
        let mut new: Vec<usize> = (0..tracks.len())
            .filter(|&k| tracks[k].first == t)
            .collect();
        new.sort_by(|&a, &b| place_order(actor(&tracks[a], t), actor(&tracks[b], t)));
        for k in new {
            let a = actor(&tracks[k], t);
            let (name, parent) = if let Some(p) = &a.placement {
                let n = taken.entry(p.clone()).or_default();
                *n += 1;
                let name = if *n == 1 {
                    p.clone()
                } else {
                    format!("{p}~{n}")
                };
                (name, None)
            } else {
                let rule = spawns.iter().find(|r| r.kind == a.kind);
                let parent = rule.filter(|_| t > 0).and_then(|rule| {
                    tracks
                        .iter()
                        .filter(|c| {
                            c.first < t
                                && c.first + c.slots.len() > t - 1
                                && rule.parents.contains(&c.kind)
                        })
                        .map(|c| (distance(actor(c, t - 1).position, a.position), &c.name))
                        .filter(|(d, _)| *d <= rule.radius)
                        .min_by(|x, y| x.0.total_cmp(&y.0).then_with(|| x.1.cmp(y.1)))
                        .map(|(_, name)| name.clone())
                });
                match parent {
                    Some(p) => {
                        let n = children.entry((p.clone(), a.kind.clone())).or_default();
                        *n += 1;
                        (format!("{p}>{}#{n}", a.kind), Some(p))
                    }
                    None => {
                        let n = orphans.entry(a.kind.clone()).or_default();
                        *n += 1;
                        (format!("{}#{n}", a.kind), None)
                    }
                }
            };
            tracks[k].name = name;
            tracks[k].parent = parent;
        }
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// An actor at `x` along the ground, nothing else set.
    pub(crate) fn actor(id: u64, kind: &str, placement: Option<&str>, x: f64) -> Actor {
        Actor {
            id,
            kind: kind.into(),
            placement: placement.map(Into::into),
            position: [x, 22.4, -60.0],
            heading: 0.0,
            velocity: [0.0; 3],
            behaviour: None,
            clip: None,
            vars: BTreeMap::new(),
        }
    }

    /// Ticks one nominal frame apart, frame k at k / 60 s.
    pub(crate) fn ticks(actors: Vec<Vec<Actor>>) -> Vec<Tick> {
        actors
            .into_iter()
            .enumerate()
            .map(|(k, actors)| Tick {
                frame: k as u32,
                time: k as f64 / 60.0,
                buttons: None,
                mt: None,
                actors,
            })
            .collect()
    }

    fn orb_rule() -> Vec<SpawnRule> {
        vec![SpawnRule {
            kind: "orb".into(),
            parents: vec!["chompy".into()],
            radius: 2.0,
        }]
    }

    #[test]
    fn spawned_actors_are_named_by_parent_and_place_never_by_id() {
        let c1 = || actor(7, "chompy", Some("Enemy_Chompy(1)"), 0.0);
        let c2 = || actor(8, "chompy", Some("Enemy_Chompy(2)"), 5.0);
        // A pot nearer the orbs than Chompy 2, of no kind an orb comes from.
        let pot = || actor(9, "pot", Some("pottery_1(8)"), 5.35);
        // Two runs: Chompy 2 spawns two orbs and Chompy 1 one, every id
        // different, the orbs in the opposite order in each tick's list.
        let run = |ids: [u64; 3]| {
            let orbs = vec![
                actor(ids[0], "orb", None, 5.4),
                actor(ids[1], "orb", None, 0.3),
                actor(ids[2], "orb", None, 5.2),
            ];
            let mut reversed = orbs.clone();
            reversed.reverse();
            ticks(vec![
                vec![c1(), c2(), pot()],
                [vec![c1(), c2(), pot()], orbs].concat(),
                [vec![c1(), c2(), pot()], reversed].concat(),
            ])
        };
        for ids in [[30, 31, 32], [92, 91, 90]] {
            let trace = FightTrace::new(run(ids), &orb_rule()).unwrap();
            let names: Vec<&str> = trace.tracks().map(|t| t.name()).collect();
            assert_eq!(
                names,
                [
                    "Enemy_Chompy(1)",
                    "Enemy_Chompy(1)>orb#1",
                    "Enemy_Chompy(2)",
                    "Enemy_Chompy(2)>orb#1",
                    "Enemy_Chompy(2)>orb#2",
                    "pottery_1(8)"
                ]
            );
            // Numbered by where they appeared: 5.2 before 5.4.
            assert_eq!(trace.name_of(ids[2]), Some("Enemy_Chompy(2)>orb#1"));
            assert_eq!(trace.name_of(ids[0]), Some("Enemy_Chompy(2)>orb#2"));
            let orb = trace.track("Enemy_Chompy(1)>orb#1").unwrap();
            assert_eq!(
                (orb.parent(), orb.first(), orb.last()),
                (Some("Enemy_Chompy(1)"), 1, 2)
            );
            assert!(orb.spawned() && orb.gone().is_none());
        }
        // Out of every parent's reach, or with no rule: counted by kind.
        let far = ticks(vec![vec![c1()], vec![c1(), actor(3, "orb", None, 9.0)]]);
        let trace = FightTrace::new(far, &orb_rule()).unwrap();
        assert!(trace.track("orb#1").is_some());
        let trace = FightTrace::new(run([1, 2, 3]), &[]).unwrap();
        assert!(trace.track("orb#3").is_some());
    }

    #[test]
    fn a_trace_refuses_what_it_cant_key() {
        let one = || vec![actor(1, "chompy", Some("C"), 0.0)];
        // A lost frame.
        let mut lost = ticks(vec![one(), one(), one()]);
        lost[2].frame = 3;
        assert_eq!(
            FightTrace::new(lost, &[]),
            Err(MetricError::TickGap {
                index: 2,
                previous: 1,
                frame: 3
            })
        );
        // Time going back; a stopped logic (no step) is fine.
        let mut back = ticks(vec![one(), one(), one()]);
        back[2].time = 0.0;
        assert!(matches!(
            FightTrace::new(back, &[]),
            Err(MetricError::TimeOrder { index: 2, .. })
        ));
        let mut stopped = ticks(vec![one(), one(), one()]);
        stopped[2].time = stopped[1].time;
        assert!(FightTrace::new(stopped, &[]).is_ok());
        // An id twice in a tick, changing kind, or back after leaving.
        let twice = ticks(vec![vec![
            actor(1, "a", None, 0.0),
            actor(1, "a", None, 1.0),
        ]]);
        assert_eq!(
            FightTrace::new(twice, &[]),
            Err(MetricError::DuplicateActor { frame: 0, id: 1 })
        );
        let changed = ticks(vec![one(), vec![actor(1, "orb", Some("C"), 0.0)]]);
        assert_eq!(
            FightTrace::new(changed, &[]),
            Err(MetricError::ActorChanged { frame: 1, id: 1 })
        );
        let back_again = ticks(vec![one(), vec![], one()]);
        assert_eq!(
            FightTrace::new(back_again, &[]),
            Err(MetricError::ActorReturned { frame: 2, id: 1 })
        );
        let mut nan = ticks(vec![one()]);
        nan[0].actors[0].vars.insert("health".into(), f64::NAN);
        assert!(FightTrace::new(nan, &[]).is_err());
        assert_eq!(FightTrace::new(vec![], &[]), Err(MetricError::EmptyTrace));
    }

    #[test]
    fn cues_are_entered_where_they_start_to_hold() {
        let with = |id, b: &str| {
            let mut a = actor(id, "chompy", Some("C"), 0.0);
            a.behaviour = Some(b.into());
            a
        };
        let trace = FightTrace::new(
            ticks(vec![
                vec![with(1, "Lunge")],
                vec![with(1, "Lunge")],
                vec![with(1, "Recover")],
                vec![with(1, "Lunge")],
                vec![],
            ]),
            &[],
        )
        .unwrap();
        let c = trace.track("C").unwrap();
        let lunge = Cue::Behaviour("Lunge".into());
        // Already lunging at F0 isn't an entry; the second lunge is.
        assert_eq!(c.entries(&lunge), [3]);
        assert_eq!(c.spans(&lunge), [(0, 1), (3, 3)]);
        assert_eq!(c.entries(&Cue::Gone), [4]);
        assert_eq!(c.gone(), Some(4));
    }
}
