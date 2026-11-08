//! The ship's parts at the hub (openspec AS.157): the legendary treasure a
//! kid collects on a level is a part of the ship, and the hub hangs the
//! ones equipped on it.
//!
//! - **What the original does.** Each part has a collected and an equipped
//!   bit in the story save (`Save - Collection Get/Set - Legendary
//!   Treasure`); a new save reads the ship's three defaults (an engine, a
//!   masthead and a roof, ids 16-18) collected and equipped, by the save's
//!   own default mapping. Going to the village the Pause Controller's switch
//!   appends a stream per part to the hub stage's load
//!   (`level/ship_part_NNN.bld`: `System - Ship Parts - Get Ship Part Loads`
//!   with every part asked, which also sets the ids the streams take). Each
//!   stream's definer (`System - Ship Parts - Define Ship Part`) takes the
//!   next of those ids for its template, and the hub's
//!   `_Utilities_Controller`, once one is defined, clones each equipped one
//!   onto the ship at its `AttachPoint`; the Dragon Engine (its template's
//!   id 2) lifts `Galley_Blocker`, the way down to the galley.
//! - **What the TV does the same.** A hub stage's fetch extracts every ship
//!   part's streamed parts, its load links each stream's file after the
//!   stage's ([`streams`]) and draws its template's model, and once the
//!   run's save is in ([`start`], from `story_save::start_run`) the switch's
//!   loads are asked of the original's own macro and each stream's definer
//!   joins the run, as a hero's file's placements do at his join.
//! - **Equipping** is the treasure menu's (`LegendaryTreasureData.ai`, AK's
//!   line), a screen the TV doesn't have: nothing in the story equips a part
//!   (no level end, intro or cutscene writes `equipped`; the menu and its
//!   debug set-up alone do). Until AK a part is equipped as it's collected,
//!   in its category's place by the menu's own rule, one part a category
//!   ([`equip`], [`on_collection`]), and at a run's start a collected part on
//!   a category still on its default (a save from before this) is equipped
//!   the same way ([`settle`]) (stand-in ledger: `ship-parts`).

use guac_script::{Halt, Obj, PlacementId, Value, Vm};
use serde::Serialize;
use tracing::{info, warn};

use crate::scripts::{Placed, ScriptWorld, WorldHost};

/// The save's accessor for a part's bits: `check id` the part (-1 the
/// current adventure's), each of `collected` and `equipped` read at -1,
/// written at 0 or 1. Its name ends in a space on the disc.
pub const GET_SET: &str = "Save - Collection Get/Set - Legendary Treasure ";
/// The switch's loads: every part's id to load with `get all` (the
/// village's), the equipped ones' without.
pub const LOADS: &str = "System - Ship Parts - Get Ship Part Loads";
/// The parts the save keeps, `level progression data`'s.
const TOTAL: &str = "total ship parts";
/// What the streams define and the controller hangs.
const TRACKING: &str = "Level Stream Tracking";
const HUNG: &str = "Attached_Parts";
const DRAGON_ENGINE: &str = "Dragon_Engine";

/// Parts in a category past its default (the menu's rows 1-4).
pub const CATEGORY_PARTS: i32 = 4;
/// Each category's default, the menu's row 0: the engine's, the
/// masthead's and the roof's (the paint, category 2, has none).
pub const DEFAULTS: [(i32, i32); 3] = [(16, 0), (17, 1), (18, 3)];

/// The menu's category of part `id` (`LegendaryTreasureMenu - Get Ship
/// Part Asset Def Ref From Row and Col`: four categories, the engines,
/// mastheads, paint and roofs, each its four parts `4c` to `4c + 3` under
/// its default); none for an id the menu lays out nowhere.
pub fn category(id: i32) -> Option<i32> {
    match id {
        0..16 => Some(id / CATEGORY_PARTS),
        _ => DEFAULTS.iter().find(|(d, _)| *d == id).map(|&(_, c)| c),
    }
}

/// Whether part `id` is a category's default: never a kid's choice, so a
/// category on it is one [`settle`] fills.
pub fn is_default(id: i32) -> bool {
    DEFAULTS.iter().any(|(d, _)| *d == id)
}

/// A part's bits as the save holds them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct Part {
    pub id: i32,
    pub collected: bool,
    pub equipped: bool,
}

/// What the stand-in equipped, and on what.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Equipped {
    /// Collected on a category still on its default, at a run's start.
    Settled,
    /// As its legendary treasure was collected.
    Collected,
}

/// The run's ship parts: the streams it links and what's been done with
/// them.
#[derive(Debug, Clone, Default)]
pub struct ShipParts {
    /// The units the streams link as, in their load's order (a hub stage's:
    /// every ship part on the disc, [`streams`]); none on any other level.
    pub units: Vec<u16>,
    /// Each one's part id, its number in its archive's name ([`part_id`]),
    /// in the same order.
    pub ids: Vec<i32>,
    /// Whether the run's start has been taken ([`start`]).
    pub started: bool,
    /// Whether the streams' placements have joined the run.
    pub joined: bool,
    /// The bits read at the run's start.
    pub read: Vec<Part>,
    /// What the stand-in equipped, in order.
    pub equipped: Vec<(i32, Equipped)>,
    /// The legendary treasures whose collection has been taken, each once.
    collections: Vec<PlacementId>,
    /// A stop in the original's macros, kept for the report.
    pub stopped: Option<String>,
}

/// The ship part streams a level loads with, by archive, from `on_disc`
/// (the disc's files by name): a hub stage's every ship part in its id's
/// order, as the village's switch asks them (`get all`: ids 0 to the
/// save's total, `ship_part_NNN` each); none for any other level, whose
/// switch asks the equipped parts' only of the levels that need them (none
/// the TV plays).
pub fn streams<'a>(level: &str, on_disc: impl IntoIterator<Item = &'a str>) -> Vec<String> {
    if !crate::scripts::is_hub(level) {
        return Vec::new();
    }
    let mut parts: Vec<String> = on_disc
        .into_iter()
        .filter(|n| guac_extract::levels::Kind::of(n) == guac_extract::levels::Kind::ShipPart)
        .map(str::to_owned)
        .collect();
    parts.sort();
    parts
}

/// The part a ship part stream defines, by its archive's name: its number,
/// as the switch's loads name each part's stream by its id (`ship_part_`
/// and the id in three digits); none for any other file.
pub fn part_id(stream: &str) -> Option<i32> {
    stream.strip_prefix("ship_part_")?.parse().ok()
}

/// `config`'s listing with every script the streams' placements name (their
/// definers and their templates' own, the engines' turning): what the
/// streams run.
pub fn with_streams(
    mut config: guac_script::Config,
    files: &crate::scripts::Files,
    units: &[u16],
) -> guac_script::Config {
    if let guac_script::Whitelist::Only { scripts, .. } = &mut config.whitelist {
        for p in files.placed.iter().filter(|p| units.contains(&p.obj.unit)) {
            if let Some(s) = &p.script
                && !scripts.contains(s)
            {
                scripts.push(s.clone());
            }
        }
    }
    config
}

fn int(v: Option<Value>) -> Option<i32> {
    v.and_then(|v| v.as_i32())
}

/// A global's value by name; none when the files define no such global.
fn global(vm: &Vm, name: &str) -> Option<Value> {
    let g = vm.program().global_named(name)?;
    vm.global_peek(g).map(|v| vm.value(v))
}

/// How many parts the save keeps; none before the save's set-up has run.
fn total(vm: &Vm) -> Option<i32> {
    let lpd = global(vm, "level progression data")?;
    int(vm.field(lpd, TOTAL).map(|v| vm.value(v))).filter(|t| *t > 0)
}

/// Part `id`'s bits, read through the save's accessor.
pub fn read(vm: &mut Vm, host: &mut WorldHost<'_, '_>, id: i32) -> Result<Part, Halt> {
    let (_, back) = vm.call_with(
        None,
        GET_SET,
        &[
            ("adventure title", Value::Null),
            ("check id", Value::Int(id)),
            ("collected", Value::Int(-1)),
            ("equipped", Value::Int(-1)),
        ],
        &["collected", "equipped"],
        host,
    )?;
    let bit = |i: usize| int(back.get(i).copied().flatten()).is_some_and(|b| b > 0);
    Ok(Part {
        id,
        collected: bit(0),
        equipped: bit(1),
    })
}

/// Every part's bits, ids 0 to the save's total in order; none before the
/// save's set-up has written its total, or with no accessor linked.
pub fn parts(vm: &mut Vm, host: &mut WorldHost<'_, '_>) -> Result<Vec<Part>, Halt> {
    let Some(total) = total(vm) else {
        return Ok(Vec::new());
    };
    if vm.program().macro_named(GET_SET).is_none() {
        return Ok(Vec::new());
    }
    (0..total).map(|id| read(vm, host, id)).collect()
}

fn write_equipped(
    vm: &mut Vm,
    host: &mut WorldHost<'_, '_>,
    id: i32,
    on: bool,
) -> Result<(), Halt> {
    vm.call_with(
        None,
        GET_SET,
        &[
            ("adventure title", Value::Null),
            ("check id", Value::Int(id)),
            ("collected", Value::Int(-1)),
            ("equipped", Value::Int(i32::from(on))),
        ],
        &[],
        host,
    )?;
    Ok(())
}

/// Part `id` equipped in its category's place, as the menu's equip does: the
/// category's equipped part off, `id` on. What changed: false when it was
/// already the category's alone. A part no category holds is left.
pub fn equip(vm: &mut Vm, host: &mut WorldHost<'_, '_>, id: i32) -> Result<bool, Halt> {
    let Some(cat) = category(id) else {
        return Ok(false);
    };
    let Some(total) = total(vm) else {
        return Ok(false);
    };
    let mut changed = false;
    for other in (0..total).filter(|&o| category(o) == Some(cat)) {
        let part = read(vm, host, other)?;
        let want = other == id;
        if part.equipped != want {
            write_equipped(vm, host, other, want)?;
            changed = true;
        }
    }
    Ok(changed)
}

/// The parts `parts` says to equip at a run's start: in each category whose
/// equipped part isn't one of its collected parts past the default, its
/// collected part past the default of the highest id. A save made before
/// the TV equipped as it collected holds such a part.
pub fn to_settle(parts: &[Part]) -> Vec<i32> {
    let mut out = Vec::new();
    for cat in 0..CATEGORY_PARTS {
        let own: Vec<&Part> = parts
            .iter()
            .filter(|p| category(p.id) == Some(cat) && !is_default(p.id))
            .collect();
        if own.iter().any(|p| p.equipped && p.collected) {
            continue;
        }
        if let Some(p) = own.iter().filter(|p| p.collected).max_by_key(|p| p.id) {
            out.push(p.id);
        }
    }
    out
}

/// [`to_settle`]'s parts equipped (stand-in ledger: `ship-parts`): the bits
/// read first, kept for the report.
pub fn settle(vm: &mut Vm, host: &mut WorldHost<'_, '_>) -> Result<Vec<i32>, Halt> {
    let read = parts(vm, host)?;
    let ids = to_settle(&read);
    host.world.ship_parts.read = read;
    for &id in &ids {
        equip(vm, host, id)?;
        host.world.ship_parts.equipped.push((id, Equipped::Settled));
        info!(
            part = id,
            "a collected ship part on a category still on its default: equipped"
        );
    }
    Ok(ids)
}

/// A legendary treasure showing its report (its `show player`) whose
/// collection hasn't been taken yet: its level's part is in the collection
/// then (`Loot_Specials.ai` sets it collected first), and it's equipped in
/// its category's place (stand-in ledger: `ship-parts`). The part, if one
/// was.
pub fn on_collection(vm: &mut Vm, host: &mut WorldHost<'_, '_>) -> Result<Option<i32>, Halt> {
    let showing = crate::treasure::legendary_showing(vm, host.world);
    let Some(&treasure) = showing
        .iter()
        .find(|t| !host.world.ship_parts.collections.contains(t))
    else {
        return Ok(None);
    };
    host.world.ship_parts.collections.push(treasure);
    let Some(title) = global(vm, "Current Adventure Title").filter(|t| *t != Value::Null) else {
        return Ok(None);
    };
    if vm.program().macro_named(GET_SET).is_none() {
        return Ok(None);
    }
    let (_, back) = vm.call_with(
        None,
        GET_SET,
        &[
            ("adventure title", title),
            ("check id", Value::Int(-1)),
            ("collected", Value::Int(-1)),
            ("equipped", Value::Int(-1)),
        ],
        &["check id", "collected"],
        host,
    )?;
    let (Some(id), Some(1)) = (
        int(back.first().copied().flatten()),
        int(back.get(1).copied().flatten()),
    ) else {
        return Ok(None);
    };
    equip(vm, host, id)?;
    host.world
        .ship_parts
        .equipped
        .push((id, Equipped::Collected));
    info!(
        part = id,
        "a legendary treasure collected: its ship part equipped"
    );
    Ok(Some(id))
}

/// The streams' running placements, in the order a file's load adds them
/// (awake ones first, each in its root list's order), as `(placement,
/// handle)`.
fn stream_placements<'w>(
    world: &'w ScriptWorld,
    unit: u16,
) -> Vec<(&'w Placed, guac_script::Handle)> {
    let mut placed: Vec<&Placed> = world
        .templates
        .values()
        .filter(|p| p.obj.unit == unit && p.activity_state == 4 && p.script.is_some())
        .collect();
    placed.sort_by_key(|p| {
        (
            !crate::scripts::awake(p.range, p.buffer, false, None),
            p.load,
            p.obj.index,
        )
    });
    placed
        .into_iter()
        .filter_map(|p| world.objects.get(&p.obj).map(|&h| (p, h)))
        .collect()
}

/// The streams joined (the module docs): the village's loads asked of the
/// switch's own macro, every part's id to load set, then each stream's
/// running placements added to the run. How many joined; a stop in the
/// loads is kept and the streams join on it, their definers numbering on
/// from the last defined.
pub fn join(vm: &mut Vm, host: &mut WorldHost<'_, '_>) -> Result<usize, Halt> {
    if host.world.ship_parts.units.is_empty() || host.world.ship_parts.joined {
        return Ok(0);
    }
    host.world.ship_parts.joined = true;
    if vm.program().macro_named(LOADS).is_some()
        && let Err(h) = vm.call_with(
            None,
            LOADS,
            &[("get all", Value::Int(1))],
            &["return full string"],
            host,
        )
    {
        warn!(%h, "the ship parts' loads stopped: the streams number their parts on from the last");
        host.world.ship_parts.stopped = Some(h.to_string());
    }
    // The loads list every part, the definers taking them in turn: one left
    // out of the run (its parts didn't extract) would shift each after it to
    // the next's id, so the run's streams' own ids go first, in their order.
    let ids = host.world.ship_parts.ids.clone();
    if !put_loading(vm, &ids) {
        warn!(
            streams = ids.len(),
            "the ship parts' loads list too few ids: the streams number theirs on from the last"
        );
    }
    let program = vm.program_arc();
    let units = host.world.ship_parts.units.clone();
    let mut added = 0;
    for unit in units {
        let placed: Vec<(Obj, String, String, guac_script::Handle)> =
            stream_placements(host.world, unit)
                .into_iter()
                .filter_map(|(p, h)| Some((p.obj, p.name.clone(), p.script.clone()?, h)))
                .collect();
        for (obj, name, script, h) in placed {
            let Some(sid) = program
                .scripts
                .iter()
                .position(|s| s.unit == unit && s.name == script)
            else {
                continue;
            };
            vm.add_at(h, &name, sid as u32, obj);
            added += 1;
        }
    }
    info!(added, "the ship part streams joined the run");
    Ok(added)
}

/// `ids` over the head of the loads' list of ids to take (`Level Stream
/// Tracking`'s `ship part ids loading`), in order. False when there's no
/// such list or it holds fewer.
fn put_loading(vm: &mut Vm, ids: &[i32]) -> bool {
    let Some(members) = global(vm, TRACKING)
        .and_then(|t| vm.field(t, "ship part ids loading"))
        .and_then(|set| vm.set(set).map(<[Value]>::to_vec))
    else {
        return false;
    };
    if members.len() < ids.len() {
        return false;
    }
    members
        .iter()
        .zip(ids)
        .all(|(m, &id)| vm.put(*m, Value::Int(id)))
}

/// The run's start for its ship parts, once its save is in: the stand-in's
/// settling, then the streams joined. Once a run.
pub fn start(vm: &mut Vm, host: &mut WorldHost<'_, '_>) -> Result<(), Halt> {
    if host.world.ship_parts.started {
        return Ok(());
    }
    host.world.ship_parts.started = true;
    settle(vm, host)?;
    join(vm, host)?;
    Ok(())
}

/// What `/api/_test/tv` says of the ship's parts.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct ShipReport {
    /// The streams the run links, and whether they've joined it.
    pub streams: usize,
    pub joined: bool,
    /// The parts the streams defined, by id, in order.
    pub defined: Vec<i32>,
    /// The parts read collected and equipped at the run's start.
    pub collected: Vec<i32>,
    pub equipped: Vec<i32>,
    /// What the stand-in equipped since, in order.
    pub stand_in: Vec<(i32, Equipped)>,
    /// What hangs on the ship (the controller's clones), by their
    /// templates' names, and the Dragon Engine's flag.
    pub hung: Vec<String>,
    pub dragon_engine: Option<i32>,
    pub stopped: Option<String>,
}

/// [`ShipReport`] off the run.
pub fn report(vm: &Vm, world: &ScriptWorld) -> ShipReport {
    let s = &world.ship_parts;
    let defined = global(vm, TRACKING)
        .and_then(|t| vm.field(t, "ship parts"))
        .and_then(|set| vm.set(set).map(<[Value]>::to_vec))
        .unwrap_or_default()
        .into_iter()
        .filter_map(|p| int(vm.field(p, "id").map(|v| vm.value(v))))
        .collect();
    let hung = global(vm, HUNG)
        .and_then(|set| vm.set(set).map(<[Value]>::to_vec))
        .unwrap_or_default()
        .into_iter()
        .filter_map(|v| match v {
            Value::Entity(h) => world.actor(h).and_then(|a| a.template.or(a.obj)),
            _ => None,
        })
        .filter_map(|o| world.template(o).map(|p| p.name.clone()))
        .collect();
    ShipReport {
        streams: s.units.len(),
        joined: s.joined,
        defined,
        collected: s
            .read
            .iter()
            .filter(|p| p.collected)
            .map(|p| p.id)
            .collect(),
        equipped: s.read.iter().filter(|p| p.equipped).map(|p| p.id).collect(),
        stand_in: s.equipped.clone(),
        hung,
        dragon_engine: int(global(vm, DRAGON_ENGINE)),
        stopped: s.stopped.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The menu's layout: four parts a category over its default, the paint
    /// with none; nothing past the save's 19.
    #[test]
    fn each_part_sits_in_the_menus_category() {
        let cats: Vec<Option<i32>> = (0..20).map(category).collect();
        let mut want: Vec<Option<i32>> = (0..16).map(|i| Some(i / 4)).collect();
        want.extend([Some(0), Some(1), Some(3), None]);
        assert_eq!(cats, want);
        assert_eq!(category(-1), None);
        assert!((16..19).all(is_default) && !(0..16).any(is_default));
    }

    fn part(id: i32, collected: bool, equipped: bool) -> Part {
        Part {
            id,
            collected,
            equipped,
        }
    }

    /// A new save's defaults settle nothing; a collected roof on the roof's
    /// default is equipped, the highest of two; a category whose own part is
    /// equipped is left, even with another collected; the paint, with no
    /// default, takes its collected part.
    #[test]
    fn a_collected_part_on_its_categorys_default_is_settled() {
        let fresh = [
            part(16, true, true),
            part(17, true, true),
            part(18, true, true),
        ];
        assert!(to_settle(&fresh).is_empty());
        let mut old = fresh.to_vec();
        old.extend([
            part(12, true, false),
            part(15, true, false),
            part(8, true, false),
        ]);
        assert_eq!(to_settle(&old), [8, 15]);
        let mut chosen = fresh.to_vec();
        chosen.extend([
            part(12, true, false),
            part(15, true, true),
            part(1, false, false),
        ]);
        assert!(to_settle(&chosen).is_empty(), "the roof's own part stays");
        // An equipped bit on a part not collected isn't a choice.
        let odd = [
            part(18, true, true),
            part(13, false, true),
            part(14, true, false),
        ];
        assert_eq!(to_settle(&odd), [14]);
    }

    /// A stream's part id is its archive's number; nothing else has one.
    #[test]
    fn a_streams_part_is_its_number() {
        assert_eq!(part_id("ship_part_000"), Some(0));
        assert_eq!(part_id("ship_part_018"), Some(18));
        assert_eq!(part_id("level_hub_2_stage_1"), None);
        assert_eq!(part_id("ship_part_x"), None);
    }

    /// A hub stage loads every ship part on the disc in id order; no other
    /// level any.
    #[test]
    fn a_hub_stage_streams_every_ship_part() {
        let disc = [
            "ship_part_002",
            "level_201",
            "ship_part_000",
            "ship_part_001",
            "level_hub_2_stage_1",
            "challengevo",
        ];
        assert_eq!(
            streams("level_hub_2_stage_3", disc),
            ["ship_part_000", "ship_part_001", "ship_part_002"]
        );
        assert!(streams("level_201", disc).is_empty());
        assert!(streams("level_214_tutotial_2", disc).is_empty());
    }
}
