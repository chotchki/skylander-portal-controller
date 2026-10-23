//! Giants' script scheduler, read at a pause and followed per frame (openspec
//! AH.59): which placements' scripts tick in a frame, in what order, beside
//! the engine's other phases.
//!
//! The layout, Giants 01.00 (`BLUS30968`), offsets from each object's start:
//! - The insight core ([`crate::actors`]) runs a frame as: its frame time,
//!   its input context's update (`igPS3InputContext`, +0x10: the pads read),
//!   then its serial scheduler (+0x20), which runs the `_updaters` list
//!   (+0x1C) in order. [`Updater`] names each by its class.
//! - One updater is `tfbScriptUpdateGame` ([`SCRIPT_UPDATE`]): each frame it
//!   steps the slides, then walks its groups (+0x08, +0x0C, +0x10; a fourth
//!   at +0x14 only teardown walks), each a `PlacementSetList` of one list per
//!   kind of placement (actors, lights, particles, in that order), every
//!   list's placements in list order, then its queue of placements with a
//!   shutdown pending (+0x24). A placement's group is its activity state
//!   (`_activityState`, +0x54): 0 the activators the others' ranges are
//!   measured from, 1 awake, 2 asleep, 3 taken off. Its walk's place is
//!   kept in the updater (+0x18 group, +0x1C kind, +0x20 item), 0 between
//!   frames.
//! - A placement (`tfbActorInfo` or another info class) carries its `_name`
//!   (+0x08), flags (+0x1C), `_activityState` (+0x54), its script (+0xA8),
//!   `_activeTime` (+0xB4, which its tick moves on) and its activation range
//!   and buffer (+0xD0, +0xD4, feet).
//!
//! [`snapshot`] reads all of it at a pause; [`order_chain`] and
//! [`active_chain`] are O6's chains that carry it every frame (slots
//! [`crate::chains::SCHED_ORDER`] and [`crate::chains::SCHED_ACTIVE`] when a
//! drive arms them), [`SchedRecord`] a record's blocks decoded.

use std::collections::BTreeMap;

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};

use crate::actors::{self, Gather, be32, c_string, f32_at, one, vtable};
use crate::verbs::{ChainBlock, ChainSpec, Hop, Key, Probe};

/// `tfbScript::tfbScriptUpdateGame`'s vtable: the scripts' updater.
pub const SCRIPT_UPDATE: u32 = 0x0062_8540;
/// `tfbScript::PlacementSetList`'s: one group, a list per kind.
pub const PLACEMENT_SET_LIST: u32 = 0x0062_84D0;
/// Each kind's list, in the walk's order: actors, lights, particles.
pub const KIND_LISTS: [u32; 3] = [0x0062_1140, 0x0062_C1F0, 0x0062_A4C0];
/// The kinds by name, in [`KIND_LISTS`]' order.
pub const KIND_NAMES: [&str; 3] = ["actors", "lights", "particles"];
/// `tfbScript::PlacementList`'s: the shutdown queue.
pub const PLACEMENT_LIST: u32 = 0x0062_4798;
/// The groups [`snapshot`] reads: the walk's three and teardown's fourth.
pub const GROUPS: usize = 4;

const INSIGHT_INPUT: usize = 0x10;
const INSIGHT_SCHEDULER: usize = 0x20;
const UPDATERS: usize = 0x1C;
const GROUP0: u16 = 0x08;
const CURSOR: u16 = 0x18;
const SHUTDOWN: u16 = 0x24;
const LIST_COUNT: u16 = 0x08;
const LIST_ARRAY: u16 = 0x14;
const NAME: u16 = 0x08;
const FLAGS: u16 = 0x1C;
const STATE: u16 = 0x54;
const SCRIPT: usize = 0xA8;
const ACTIVE: u16 = 0xB4;
const RANGE: usize = 0xD0;
const BUFFER: usize = 0xD4;
/// What [`snapshot`] reads of a placement: through its range and buffer.
const PLACED_LEN: usize = 0xD8;
const NAME_LEN: usize = 64;

/// An updater as the serial scheduler runs it: its address and its class
/// off its vtable's RTTI.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Updater {
    pub address: u32,
    pub vtable: u32,
    /// Its class as the EBOOT's RTTI names it, when that reads.
    pub class: Option<String>,
}

/// A placement as a pause finds it on a group's list: its name, the state
/// and flags the walk reads, its active time and its activation range.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Placed {
    pub address: u32,
    pub vtable: u32,
    pub name: String,
    pub state: u32,
    pub flags: u32,
    pub active: f32,
    /// Its script pointer (+0xA8) isn't null.
    pub scripted: bool,
    pub range: f32,
    pub buffer: f32,
}

/// The scheduler at a pause: the insight core's phases, the updaters in
/// order, the walk's place and every group's placements.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SchedSnapshot {
    /// What the insight core runs before its updaters: its input context's
    /// class (the pads' read) and its scheduler's.
    pub input: Option<String>,
    pub scheduler_class: Option<String>,
    pub updaters: Vec<Updater>,
    /// The scripts' updater and its index among them.
    pub script_update: u32,
    pub index: u32,
    /// The walk's place: group, kind, item (0 between frames).
    pub cursor: [u32; 3],
    /// Placements with a shutdown pending.
    pub shutdown: Option<u32>,
    /// Per group, per kind: its placements in list order (`None`: no list).
    pub groups: Vec<Vec<Option<Vec<Placed>>>>,
}

impl SchedSnapshot {
    /// Every placement, by address, with its group and kind.
    pub fn by_address(&self) -> BTreeMap<u32, (usize, usize, &Placed)> {
        let mut out = BTreeMap::new();
        for (g, kinds) in self.groups.iter().enumerate() {
            for (k, list) in kinds.iter().enumerate() {
                for p in list.iter().flatten() {
                    out.insert(p.address, (g, k, p));
                }
            }
        }
        out
    }

    /// The vtables group 1's actors carry: what [`active_chain`] keeps.
    pub fn awake_vtables(&self) -> Vec<u32> {
        let mut v: Vec<u32> = self
            .groups
            .get(1)
            .and_then(|k| k.first())
            .and_then(Option::as_ref)
            .into_iter()
            .flatten()
            .map(|p| p.vtable)
            .collect();
        v.push(vtable::ACTOR_INFO);
        v.sort_unstable();
        v.dedup();
        v
    }
}

/// A class's RTTI name off its vtable (the word before it the typeinfo, its
/// +4 the mangled name), when that reads.
fn class_of(g: &mut impl Gather, vt: u32) -> Option<String> {
    let ti = g.gather(&[(vt.checked_sub(4)?, 4)]).ok()?.pop()??;
    let ti = be32(&ti, 0);
    let at = g.gather(&[(ti.checked_add(4)?, 4)]).ok()?.pop()??;
    let name = g.gather(&[(be32(&at, 0), 96)]).ok()?.pop()??;
    let s = c_string(&name);
    (!s.is_empty() && s.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')).then_some(s)
}

/// An `igDataList`'s elements, unchecked but for its count against its
/// capacity.
fn elements(g: &mut impl Gather, at: u32, what: &str) -> Result<Vec<u32>> {
    let head = one(g, at, 0x18, what)?;
    let (count, capacity, data) = (be32(&head, 0x08), be32(&head, 0x0C), be32(&head, 0x14));
    if count > capacity || count > actors::MAX_LIST {
        bail!("{what} at {at:#010x} counts {count} with capacity {capacity}");
    }
    if count == 0 {
        return Ok(Vec::new());
    }
    let words = one(g, data, 4 * count as usize, what)?;
    Ok((0..count as usize).map(|i| be32(&words, 4 * i)).collect())
}

/// The placements at `addrs`, each named.
fn placed(g: &mut impl Gather, addrs: &[u32]) -> Result<Vec<Placed>> {
    let heads = g.gather(&addrs.iter().map(|&a| (a, PLACED_LEN)).collect::<Vec<_>>())?;
    let mut out = Vec::with_capacity(addrs.len());
    let mut names = Vec::with_capacity(addrs.len());
    for (&a, h) in addrs.iter().zip(&heads) {
        let h = h
            .as_deref()
            .with_context(|| format!("the placement at {a:#010x} isn't mapped"))?;
        names.push((be32(h, NAME as usize), NAME_LEN));
        out.push(Placed {
            address: a,
            vtable: be32(h, 0),
            name: String::new(),
            state: be32(h, STATE as usize),
            flags: be32(h, FLAGS as usize),
            active: f32_at(h, ACTIVE as usize),
            scripted: be32(h, SCRIPT) != 0,
            range: f32_at(h, RANGE),
            buffer: f32_at(h, BUFFER),
        });
    }
    let read = g.gather(&names)?;
    for (p, n) in out.iter_mut().zip(read) {
        p.name = n.map(|b| c_string(&b)).unwrap_or_default();
    }
    Ok(out)
}

/// The scheduler at a pause: the updaters in order, then every group's
/// lists, each placement named, with its state, flags, active time and
/// range.
pub fn snapshot(g: &mut impl Gather) -> Result<SchedSnapshot> {
    let app = be32(&one(g, actors::APP, 4, "the application pointer")?, 0);
    let a = one(g, app, actors::APP_LEN, "the application")?;
    actors::expect_vtable(&a, vtable::APPLICATION, app, "tfbPS3Application")?;
    let insight = be32(&a, actors::APP_INSIGHT);
    let core = one(g, insight, 0x24, "the insight core")?;
    actors::expect_vtable(&core, vtable::INSIGHT_CORE, insight, "igInsightCore")?;
    let input = word_class(g, be32(&core, INSIGHT_INPUT));
    let scheduler_class = word_class(g, be32(&core, INSIGHT_SCHEDULER));
    let list = be32(&core, UPDATERS);
    let addrs = actors::list(g, list, vtable::UPDATER_LIST, "igInsightUpdaterList")?;
    let mut updaters = Vec::with_capacity(addrs.len());
    for &u in &addrs {
        let head = one(g, u, 4, "an updater")?;
        let vt = be32(&head, 0);
        updaters.push(Updater {
            address: u,
            vtable: vt,
            class: class_of(g, vt),
        });
    }
    let found: Vec<usize> = updaters
        .iter()
        .enumerate()
        .filter(|(_, u)| u.vtable == SCRIPT_UPDATE)
        .map(|(i, _)| i)
        .collect();
    let [index] = found[..] else {
        bail!(
            "{} tfbScriptUpdateGame among the {} updaters",
            found.len(),
            updaters.len()
        );
    };
    let script_update = updaters[index].address;
    let s = one(g, script_update, 0x28, "tfbScriptUpdateGame")?;
    let cursor = [
        be32(&s, CURSOR as usize),
        be32(&s, CURSOR as usize + 4),
        be32(&s, CURSOR as usize + 8),
    ];
    let queue = be32(&s, SHUTDOWN as usize);
    let shutdown = (queue != 0)
        .then(|| one(g, queue, 0x0C, "the shutdown queue").map(|q| be32(&q, 0x08)))
        .transpose()?;
    let mut groups = Vec::with_capacity(GROUPS);
    for gi in 0..GROUPS {
        let set = be32(&s, (GROUP0 as usize) + 4 * gi);
        let lists = actors::list(g, set, PLACEMENT_SET_LIST, "tfbScript::PlacementSetList")?;
        let mut kinds = Vec::with_capacity(lists.len());
        for (k, &l) in lists.iter().enumerate() {
            if l == 0 {
                kinds.push(None);
                continue;
            }
            let head = one(g, l, 4, "a placement list")?;
            if KIND_LISTS.get(k).is_some_and(|&v| v != be32(&head, 0)) {
                bail!(
                    "group {gi}'s list {k} at {l:#010x} has vtable {:#010x}",
                    be32(&head, 0)
                );
            }
            let members = elements(g, l, "a placement list")?;
            kinds.push(Some(placed(g, &members)?));
        }
        groups.push(kinds);
    }
    Ok(SchedSnapshot {
        input,
        scheduler_class,
        updaters,
        script_update,
        index: index as u32,
        cursor,
        shutdown,
        groups,
    })
}

/// The class of the object `p` points at, when that reads.
fn word_class(g: &mut impl Gather, p: u32) -> Option<String> {
    if p == 0 {
        return None;
    }
    let head = g.gather(&[(p, 4)]).ok()?.pop()??;
    class_of(g, be32(&head, 0))
}

/// The path to the insight core's updater list.
fn updaters_path() -> Vec<Hop> {
    vec![
        Hop::checked(0, vtable::APPLICATION),
        Hop::checked(actors::APP_INSIGHT as u16, vtable::INSIGHT_CORE),
        Hop::checked(UPDATERS as u16, vtable::UPDATER_LIST),
    ]
}

/// From the scripts' updater to group `g`'s list of kind `k`.
fn kind_list(g: usize, k: usize) -> Vec<Hop> {
    vec![
        Hop::checked(GROUP0 + 4 * g as u16, PLACEMENT_SET_LIST),
        Hop::at(LIST_ARRAY),
        Hop::checked(4 * k as u16, KIND_LISTS[k]),
    ]
}

/// What [`order_chain`] copies of each group's lists: (group, kind, the
/// array's bytes, 1,024 a range).
pub const ORDER_LISTS: [(usize, usize, u16); 5] = [
    (0, 0, 256),
    (1, 0, 2048),
    (1, 2, 512),
    (2, 0, 2048),
    (2, 2, 512),
];

/// O6's chain over the updater list keeping the scripts' updater alone: the
/// walk's place, the shutdown queue's count, and for each of
/// [`ORDER_LISTS`] its count and its array of placement pointers, in the
/// walk's order (a record decodes as [`SchedRecord`]).
pub fn order_chain() -> ChainSpec {
    let mut ranges = vec![
        Probe::new(vec![], CURSOR, 12),
        Probe::new(vec![Hop::at(SHUTDOWN)], LIST_COUNT, 4),
    ];
    for &(g, k, bytes) in &ORDER_LISTS {
        let hops = kind_list(g, k);
        ranges.push(Probe::new(hops.clone(), LIST_COUNT, 4));
        let mut array = hops;
        array.push(Hop::at(LIST_ARRAY));
        let mut at = 0u16;
        while at < bytes {
            let len = (bytes - at).min(1024);
            ranges.push(Probe::new(array.clone(), at, len));
            at += len;
        }
    }
    ChainSpec {
        items: 1,
        base: actors::APP,
        path: updaters_path(),
        count: LIST_COUNT,
        array: LIST_ARRAY,
        key: Key {
            hops: Vec::new(),
            off: 0,
            hashed: false,
        },
        values: vec![SCRIPT_UPDATE],
        ranges,
    }
}

/// The awake actors [`active_chain`] keeps a record, at most.
pub const ACTIVE_ITEMS: u32 = 320;

/// O6's chain over group 1's actors (the awake ones, in the walk's order),
/// each kept when its class is one of `vtables`, with its name pointer,
/// flags, activity state and active time: which ticked in a frame, its
/// active time a tick on.
pub fn active_chain(index: u32, vtables: Vec<u32>) -> Result<ChainSpec> {
    let at = u16::try_from(4 * index).context("the updater index")?;
    let mut path = updaters_path();
    path.push(Hop::at(LIST_ARRAY));
    path.push(Hop::checked(at, SCRIPT_UPDATE));
    path.extend(kind_list(1, 0));
    Ok(ChainSpec {
        items: ACTIVE_ITEMS,
        base: actors::APP,
        path,
        count: LIST_COUNT,
        array: LIST_ARRAY,
        key: Key {
            hops: Vec::new(),
            off: 0,
            hashed: false,
        },
        values: vtables,
        ranges: vec![
            Probe::new(vec![], NAME, 4),
            Probe::new(vec![], FLAGS, 4),
            Probe::new(vec![], STATE, 4),
            Probe::new(vec![], ACTIVE, 4),
        ],
    })
}

/// A group 1 actor in a record: its place on the list and what its tick
/// moves (its active time) beside its state and flags.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AwakeItem {
    pub address: u32,
    /// Its place on group 1's actor list.
    pub index: u32,
    /// Its name's pointer ([`SchedNames`] names it).
    pub name_at: Option<u32>,
    pub flags: Option<u32>,
    pub state: Option<u32>,
    pub active: Option<f32>,
}

/// One record of the two chains: the walk's place, each copied list's
/// placements in order (`None` where the list didn't read), and group 1's
/// actors with their stamps.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SchedRecord {
    pub flip: u64,
    pub read: u32,
    pub game_frame: u32,
    pub cursor: Option<[u32; 3]>,
    pub shutdown: Option<u32>,
    /// [`ORDER_LISTS`]' lists, in that order.
    pub lists: Vec<Option<Vec<u32>>>,
    pub awake: Vec<AwakeItem>,
}

/// A record's two blocks decoded: `order` [`order_chain`]'s, `awake`
/// [`active_chain`]'s.
pub fn decode(
    (flip, read, game_frame): (u64, u32, u32),
    order: Option<&ChainBlock>,
    awake: Option<&ChainBlock>,
) -> SchedRecord {
    let mut out = SchedRecord {
        flip,
        read,
        game_frame,
        cursor: None,
        shutdown: None,
        lists: vec![None; ORDER_LISTS.len()],
        awake: Vec::new(),
    };
    if let Some(e) = order
        .filter(|b| b.status == 0)
        .and_then(|b| b.items.first())
    {
        out.cursor = e.range(0).map(|b| [be32(b, 0), be32(b, 4), be32(b, 8)]);
        out.shutdown = e.range(1).map(|b| be32(b, 0));
        let mut r = 2;
        for (slot, &(_, _, bytes)) in ORDER_LISTS.iter().enumerate() {
            let count = e.range(r).map(|b| be32(b, 0));
            r += 1;
            let pieces = usize::from(bytes.div_ceil(1024));
            let mut words = Vec::new();
            let mut whole = true;
            for p in 0..pieces {
                match e.range(r + p) {
                    Some(b) => words.extend((0..b.len() / 4).map(|i| be32(b, 4 * i))),
                    None => whole = false,
                }
            }
            r += pieces;
            out.lists[slot] = match count {
                Some(n) if whole || (n as usize) <= words.len() => {
                    Some(words.into_iter().take(n as usize).collect())
                }
                _ => None,
            };
        }
    }
    if let Some(b) = awake.filter(|b| b.status == 0) {
        out.awake = b
            .items
            .iter()
            .map(|e| AwakeItem {
                address: e.address,
                index: e.index,
                name_at: e.range(0).map(|b| be32(b, 0)),
                flags: e.range(1).map(|b| be32(b, 0)),
                state: e.range(2).map(|b| be32(b, 0)),
                active: e.range(3).map(|b| f32_at(b, 0)),
            })
            .collect();
    }
    out
}

/// The names a recording's sched records need, read at the end pause: each
/// awake item's name pointer, and each listed placement by its address
/// (`_name` read now; a placement freed since reads as what holds its
/// address now, or nothing).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SchedNames {
    pub by_name_at: BTreeMap<u32, String>,
    pub by_address: BTreeMap<u32, String>,
}

/// [`SchedNames`] for `records`, through `g` at the end pause.
pub fn names(g: &mut impl Gather, records: &[SchedRecord]) -> Result<SchedNames> {
    let mut name_ats = std::collections::BTreeSet::new();
    let mut addrs = std::collections::BTreeSet::new();
    for r in records {
        name_ats.extend(r.awake.iter().filter_map(|i| i.name_at).filter(|&a| a != 0));
        for l in r.lists.iter().flatten() {
            addrs.extend(l.iter().copied().filter(|&a| a != 0));
        }
    }
    let mut out = SchedNames::default();
    let name_ats: Vec<u32> = name_ats.into_iter().collect();
    let read = g.gather(&name_ats.iter().map(|&a| (a, NAME_LEN)).collect::<Vec<_>>())?;
    for (a, b) in name_ats.into_iter().zip(read) {
        if let Some(b) = b {
            out.by_name_at.insert(a, c_string(&b));
        }
    }
    let addrs: Vec<u32> = addrs.into_iter().collect();
    let heads = g.gather(
        &addrs
            .iter()
            .map(|&a| (a + u32::from(NAME), 4))
            .collect::<Vec<_>>(),
    )?;
    let ptrs: Vec<(u32, u32)> = addrs
        .iter()
        .zip(heads)
        .filter_map(|(&a, h)| h.map(|h| (a, be32(&h, 0))))
        .filter(|&(_, p)| actors::object_ptr(p))
        .collect();
    let read = g.gather(&ptrs.iter().map(|&(_, p)| (p, NAME_LEN)).collect::<Vec<_>>())?;
    for ((a, _), b) in ptrs.into_iter().zip(read) {
        if let Some(b) = b {
            out.by_address.insert(a, c_string(&b));
        }
    }
    Ok(out)
}

/// The decoded records beside a chained drive's other outputs.
pub const SCHED_RECORDS: &str = "sched-records.json";
/// Their names, read at the end pause.
pub const SCHED_NAMES: &str = "sched-names.json";

#[cfg(test)]
mod tests {
    use super::*;
    use crate::actors::fixture::{OTHER_VTABLE, World};
    use crate::chains::evaluate;
    use crate::verbs::CHAIN_BUDGET;

    /// The scheduler as a frame of the fight holds it, laid out in code: the
    /// updaters (the actors' and the scripts' among others), group 0 the
    /// hero, group 1 the global scripts then the fight's, an orb on group 1's
    /// particles, group 2 two asleep, the fourth a taken-off one.
    struct Fixture {
        w: World,
        hero: u32,
        awake: Vec<u32>,
        orb: u32,
        asleep: Vec<u32>,
    }

    fn placement(w: &mut World, vt: u32, name: &str, state: u32, flags: u32, t: f32) -> u32 {
        let p = w.object(vt, PLACED_LEN);
        let n = w.string(name);
        w.put32(p + u32::from(NAME), n);
        w.put32(p + u32::from(FLAGS), flags);
        w.put32(p + u32::from(STATE), state);
        w.putf(p + u32::from(ACTIVE), t);
        w.put32(p + SCRIPT as u32, 0x3100_0000);
        w.putf(p + RANGE as u32, 30.0);
        p
    }

    fn fixture() -> Fixture {
        let mut w = World::new();
        let sched = w.object(SCRIPT_UPDATE, 0x2C);
        // The scripts' updater after the actors' (World::new has physics,
        // actors, viewport): rebuild the list with it third.
        let old = w.updaters;
        let data = w.get32(old + 0x14);
        let physics = w.get32(data);
        let actor_update = w.actor_update;
        let viewport = w.get32(data + 8);
        w.updaters = w.list(0x0064_0028, &[physics, sched, actor_update, viewport], 16);
        let updaters = w.updaters;
        w.put32(w.insight + 0x1C, updaters);
        let hero = placement(&mut w, vtable::ACTOR_INFO, "009_Eruptor", 0, 0x8, 3.5);
        let names = [
            "Macros_GameSystems",
            "Macros_UI",
            "Spyro_PortalManager",
            "Enemy_Chompy(2)",
            "movementUpdate",
        ];
        let awake: Vec<u32> = names
            .iter()
            .enumerate()
            .map(|(i, n)| placement(&mut w, vtable::ACTOR_INFO, n, 1, 0x8, i as f32))
            .collect();
        let orb = placement(
            &mut w,
            OTHER_VTABLE + 32,
            "DeathXP_NewOrbOfficial",
            1,
            0x8,
            0.25,
        );
        let asleep: Vec<u32> = ["Setup_002_Controller", "Enemy_Chompy(11)"]
            .iter()
            .map(|n| placement(&mut w, vtable::ACTOR_INFO, n, 2, 0, 0.0))
            .collect();
        let gone = placement(&mut w, vtable::ACTOR_INFO, "Setup_001_Trigger", 3, 0, 9.0);
        let k =
            |w: &mut World, k: usize, items: &[u32], cap: u32| w.list(KIND_LISTS[k], items, cap);
        let g0 = [k(&mut w, 0, &[hero], 8), 0, 0];
        let g1 = [
            k(&mut w, 0, &awake, 256),
            k(&mut w, 1, &[], 4),
            k(&mut w, 2, &[orb], 50),
        ];
        let g2 = [
            k(&mut w, 0, &asleep, 1024),
            k(&mut w, 1, &[], 0),
            k(&mut w, 2, &[], 100),
        ];
        let g3 = [
            k(&mut w, 0, &[gone], 100),
            k(&mut w, 1, &[], 0),
            k(&mut w, 2, &[], 50),
        ];
        for (i, lists) in [g0, g1, g2, g3].iter().enumerate() {
            let set = w.list(PLACEMENT_SET_LIST, lists, 3);
            w.put32(sched + 0x08 + 4 * i as u32, set);
        }
        let queue = w.list(PLACEMENT_LIST, &[], 8);
        w.put32(sched + 0x24, queue);
        Fixture {
            w,
            hero,
            awake,
            orb,
            asleep,
        }
    }

    #[test]
    fn a_snapshot_reads_the_updaters_and_every_group_in_the_walks_order() {
        let mut f = fixture();
        let s = snapshot(&mut f.w).unwrap();
        assert_eq!(s.index, 1, "the scripts' updater second");
        assert_eq!(s.updaters.len(), 4);
        assert_eq!(s.updaters[2].vtable, vtable::ACTOR_UPDATE);
        assert_eq!(s.cursor, [0, 0, 0]);
        assert_eq!(s.shutdown, Some(0));
        assert_eq!(s.groups.len(), GROUPS);
        let names = |g: usize, k: usize| -> Vec<String> {
            s.groups[g][k]
                .iter()
                .flatten()
                .map(|p| p.name.clone())
                .collect()
        };
        assert_eq!(names(0, 0), ["009_Eruptor"]);
        assert!(
            s.groups[0][1].is_none() && s.groups[0][2].is_none(),
            "no list"
        );
        assert_eq!(
            names(1, 0),
            [
                "Macros_GameSystems",
                "Macros_UI",
                "Spyro_PortalManager",
                "Enemy_Chompy(2)",
                "movementUpdate"
            ]
        );
        assert_eq!(names(1, 2), ["DeathXP_NewOrbOfficial"]);
        assert_eq!(names(2, 0), ["Setup_002_Controller", "Enemy_Chompy(11)"]);
        assert_eq!(names(3, 0), ["Setup_001_Trigger"]);
        let ui = &s.groups[1][0].as_ref().unwrap()[1];
        assert_eq!(
            (ui.state, ui.flags, ui.active, ui.scripted, ui.range),
            (1, 8, 1.0, true, 30.0)
        );
        assert_eq!(s.by_address()[&f.hero].0, 0);
        assert_eq!(s.awake_vtables(), vec![vtable::ACTOR_INFO]);
    }

    #[test]
    fn a_snapshot_refuses_a_list_of_another_kind_and_a_missing_updater() {
        let mut f = fixture();
        let s = snapshot(&mut f.w).unwrap();
        let set1 = f.w.get32(s.script_update + 0x0C);
        let kinds = f.w.get32(set1 + 0x14);
        let lights = f.w.get32(kinds + 4);
        f.w.put32(lights, KIND_LISTS[2]);
        assert!(
            snapshot(&mut f.w)
                .unwrap_err()
                .to_string()
                .contains("list 1")
        );
        let mut f = fixture();
        let script_update = snapshot(&mut f.w).unwrap().script_update;
        f.w.put32(script_update, OTHER_VTABLE);
        assert!(
            snapshot(&mut f.w)
                .unwrap_err()
                .to_string()
                .contains("0 tfbScriptUpdateGame")
        );
    }

    #[test]
    fn the_chains_carry_each_groups_order_and_the_awakes_stamps() {
        let mut f = fixture();
        let s = snapshot(&mut f.w).unwrap();
        let order = order_chain();
        let awake = active_chain(s.index, s.awake_vtables()).unwrap();
        assert!(order.check().is_ok() && awake.check().is_ok());
        assert!(order.bytes() + awake.bytes() < CHAIN_BUDGET / 2);
        let ob = evaluate(&order, &mut f.w).unwrap();
        let ab = evaluate(&awake, &mut f.w).unwrap();
        assert_eq!(
            (ob.status, ob.listed, ob.items.len()),
            (0, 4, 1),
            "the one updater kept"
        );
        assert_eq!((ab.status, ab.listed, ab.items.len()), (0, 5, 5));
        let r = decode((40, 0, 6000), Some(&ob), Some(&ab));
        assert_eq!(r.cursor, Some([0, 0, 0]));
        assert_eq!(r.shutdown, Some(0));
        assert_eq!(r.lists[0], Some(vec![f.hero]));
        assert_eq!(r.lists[1], Some(f.awake.clone()));
        assert_eq!(r.lists[2], Some(vec![f.orb]));
        assert_eq!(r.lists[3], Some(f.asleep.clone()));
        assert_eq!(r.lists[4], Some(vec![]));
        let order_of: Vec<u32> = r.awake.iter().map(|i| i.address).collect();
        assert_eq!(order_of, f.awake);
        assert_eq!(r.awake[2].active, Some(2.0));
        assert_eq!(r.awake[4].state, Some(1));
        let n = names(&mut f.w, std::slice::from_ref(&r)).unwrap();
        assert_eq!(n.by_address[&f.hero], "009_Eruptor");
        let ui = r.awake[1].name_at.unwrap();
        assert_eq!(n.by_name_at[&ui], "Macros_UI");
        // A placement woken mid-run joins the awake list's end and the next
        // record lists it there; one put to sleep leaves it.
        let set1 = f.w.get32(s.script_update + 0x0C);
        let kinds = f.w.get32(set1 + 0x14);
        let actors1 = f.w.get32(kinds);
        let data = f.w.get32(actors1 + 0x14);
        f.w.put32(data + 4 * 5, f.asleep[0]);
        f.w.put32(actors1 + 0x08, 6);
        let ab = evaluate(&awake, &mut f.w).unwrap();
        let r = decode((41, 0, 6001), None, Some(&ab));
        assert_eq!(r.awake.last().map(|i| i.address), Some(f.asleep[0]));
        assert!(
            r.lists.iter().all(Option::is_none),
            "no order block, no lists"
        );
    }

    #[test]
    fn a_list_the_chain_cant_read_whole_decodes_as_none() {
        let mut f = fixture();
        let order = order_chain();
        let mut ob = evaluate(&order, &mut f.w).unwrap();
        // The awake actors' array's second 1,024 bytes missing: the count (5)
        // still fits the first, so it reads; the hero's array missing does not.
        ob.items[0].missing |= 1 << 6;
        ob.items[0].missing |= 1 << 3;
        let r = decode((1, 0, 1), Some(&ob), None);
        assert_eq!(r.lists[0], None);
        assert_eq!(r.lists[1].as_ref().map(Vec::len), Some(5));
    }
}
