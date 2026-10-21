//! The fight's state as Giants holds it, read at a pause through PINE
//! (openspec AH.4, design decision 16), on [`crate::script`]'s cells: the hero's
//! health and his figure's experience and gold, the level's enemy stats, the
//! difficulty, the portal-master experience multiplier, the element-area flag,
//! the loot accumulators and which triggers have fired; and the ones that move
//! frame by frame as fields on Giants' watch list ([`FightWatch`]).
//!
//! Where each lives, Giants 01.00 (`BLUS30968`), found in AF.8's fresh state:
//! - The hero (his placement's variables): his health is
//!   `description.creature info.hp` (`invulnerable` beside it), his full
//!   health `description.creature info.character parameters.hp max`. His
//!   player's joined data is `my Minion Description.player joined data`:
//!   its `player` (0 for player 1), `exp target` (a reference to the
//!   experience his next level starts at) and `loaded data.object info.tag
//!   data`, the figure's data as the game keeps it. That structure's `save data
//!   area` is a set of `ValueInfo` words, one per 32-bit word of the figure's
//!   save area in AH.2's numbering (word 0 at its start, the second area from
//!   word 28), and AH.2's rows say where each value sits in them, bit 0 the
//!   first word's lowest and a value running on into the next word as the
//!   scripts' bit-field macros read it ([`crate::script::bit_field`]).
//!   Experience is both areas' `pp_Exp` summed, as the game's own total macro
//!   does; the level is how many of the table's thresholds it has reached.
//! - `global.bld`'s exports ([`crate::script::exports`]): `Macros_UI`, a
//!   placement whose variables hold the level's `EnemyStatsList` (a set of
//!   structures: `Enemy Type`, the template it describes, `hp` and `xp`, the
//!   numbers `Game - Populate Enemy Stats List` computed for the difficulty)
//!   and `LootData`, whose `DestructiblesCommonRef`, `DestructiblesUncommonRef`,
//!   `DestructiblesRareRef` and `PlacedLootCommonRef` and the `XpLoot` set's
//!   entries each keep a `DropRateIndex` (the running count a drop is judged
//!   by) beside its `DropRate`; `console save params`, whose `option -
//!   difficulty` says where the difficulty sits (`bit index`, `num bits`,
//!   `signed`, and `header param`, which picks the `header save val set` over
//!   the `save val set`, each a reference to the console save's words);
//!   `combat control data`, a reference to the structure whose `accolade xp
//!   multiplier` multiplies every kill's experience; and the element-area
//!   flags `ZoneElement_P1_Correct` and `ZoneElement_P2_Correct` (1 while that
//!   player's hero stands in an area of his own element), with
//!   `ZoneElement_Current`.
//! - The level file's own cells (`Enemy_Chompy_DirectDMG`, `PandoranGrunt_DMG`,
//!   `XP_Value_Near`): `ValueInfo`s in the level's data section, in place, so
//!   each sits at the section's base ([`crate::level::section_base`]) plus its
//!   offset on the disc ([`level_values`]).
//! - A trigger is a placement the scripts wait on: a range trigger
//!   (`Setup_001_Trigger`, 20 ft) wakes when the hero comes within its range
//!   and a controller checks it's no longer inactive, and one with a script
//!   (`Activate_Norticus and Chompies`, 32 ft) runs its STARTUP then. One
//!   without a range (`Setup_003_Trigger(1)`) is awake from the level's start
//!   and its controller's `set-ups - check for player triggered` decides.
//!   [`triggers`] reads each one's `_activityState`, started flag,
//!   `_activeTime` and live range; a range trigger has fired once it's been
//!   active.

use std::collections::BTreeMap;

use anyhow::{Context, Result, bail};
use guac_ir::progression::{Progression, SaveKind};
use serde::{Deserialize, Serialize};

use crate::actors::{Gather, be32, f32_at};
use crate::pine::PineClient;
use crate::route::FigureField;
use crate::script::{
    self, Instance, Named, Number, Types, bit_field, exports, fields, number, set_items, the,
    vtable,
};
use crate::verbs::WatchRecord;
use crate::watch::{Field, WatchList};

/// The export whose variables hold the enemy stats and the loot data.
pub const MACROS_UI: &str = "Macros_UI";
pub const CONSOLE_SAVE: &str = "console save params";
pub const COMBAT_CONTROL: &str = "combat control data";
pub const LEVEL_PROGRESSION: &str = "level progression data";
/// Each 1 while that player's hero stands in an area of his own element.
pub const AREA_FLAGS: [&str; 2] = ["ZoneElement_P1_Correct", "ZoneElement_P2_Correct"];
pub const AREA_CURRENT: &str = "ZoneElement_Current";
/// The loot data's accumulators by their references' names.
pub const LOOT: [&str; 4] = [
    "DestructiblesCommonRef",
    "DestructiblesUncommonRef",
    "DestructiblesRareRef",
    "PlacedLootCommonRef",
];
/// A watched block of cells spans at most this many bytes.
pub const MAX_SPAN: u32 = 0x1000;

const HP: [&str; 3] = ["description", "creature info", "hp"];
const INVULNERABLE: [&str; 3] = ["description", "creature info", "invulnerable"];
const HP_MAX: [&str; 4] = [
    "description",
    "creature info",
    "character parameters",
    "hp max",
];
const JOINED: [&str; 2] = ["my Minion Description", "player joined data"];
const TAG_DATA: [&str; 3] = ["loaded data", "object info", "tag data"];
const SAVE_AREA: &str = "save data area";
const VALUE_TYPE: u32 = 0x1C;
const VALUE_END: u32 = 0x24;

/// Where the figure's experience, gold and playtime sit in its save area, from
/// AH.2's rows.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Rows {
    /// Each value's first bit in the area's stream and its width.
    pub experience: (u32, u32),
    /// The second area experience runs on into, when the table has one.
    pub spillover: Option<(u32, u32)>,
    /// What the first area holds before experience runs on into the second
    /// (its row's limit, 33,000 on Giants' disc), when the row has one.
    #[serde(default)]
    pub experience_limit: Option<u32>,
    pub gold: (u32, u32),
    pub playtime: (u32, u32),
    /// The experience each level starts at, level 1's first.
    pub level_xp: Vec<u32>,
}

impl Rows {
    /// The rows from AH.2's tables: refused when `pp_Exp`, `pp_Money` or
    /// `pp_CumulativeTime` has no plain row, or the level table is empty.
    pub fn of(p: &Progression) -> Result<Self> {
        let at = |param: &str, kind: SaveKind| {
            p.save_params
                .iter()
                .find(|s| s.param == param && s.kind == kind)
                .and_then(|s| s.fields.first().map(|f| (32 * f.word + f.bit, s.width)))
        };
        let need = |param: &str| {
            at(param, SaveKind::Plain)
                .with_context(|| format!("AH.2's tables have no plain `{param}` row"))
        };
        if p.level_xp.is_empty() {
            bail!("AH.2's level table is empty");
        }
        let limit = p
            .save_params
            .iter()
            .find(|s| s.param == "pp_Exp" && s.kind == SaveKind::Plain)
            .map(|s| s.limit)
            .filter(|&l| l > 0);
        Ok(Self {
            experience: need("pp_Exp")?,
            spillover: at("pp_Exp", SaveKind::Spillover),
            experience_limit: limit,
            gold: need("pp_Money")?,
            playtime: need("pp_CumulativeTime")?,
            level_xp: p.level_xp.clone(),
        })
    }

    /// The words the rows read: each value's first and, when it runs on,
    /// next word.
    pub fn words(&self) -> Vec<u32> {
        let mut w: Vec<u32> = [
            Some(self.experience),
            self.spillover,
            Some(self.gold),
            Some(self.playtime),
        ]
        .into_iter()
        .flatten()
        .flat_map(|(bit, width)| (bit / 32)..=((bit + width - 1) / 32))
        .collect();
        w.sort_unstable();
        w.dedup();
        w
    }

    /// Experience (both areas), gold and playtime out of the area's words,
    /// `word` giving each word by its index.
    pub fn values(&self, word: impl Fn(u32) -> Option<u32>) -> Result<(u32, u32, u32)> {
        let field = |(bit, width): (u32, u32)| -> Result<u32> {
            let last = (bit + width - 1) / 32;
            let words: Vec<u32> = (0..=last)
                .map(|i| {
                    if i < bit / 32 {
                        Ok(0)
                    } else {
                        word(i).with_context(|| format!("word {i} of the save area isn't read"))
                    }
                })
                .collect::<Result<_>>()?;
            Ok(bit_field(&words, bit, width, false)? as u32)
        };
        let spill = self.spillover.map(field).transpose()?.unwrap_or(0);
        Ok((
            field(self.experience)? + spill,
            field(self.gold)?,
            field(self.playtime)?,
        ))
    }

    /// `words` with one value set (AH.11's figure pokes): experience as a
    /// total, the first area up to [`Rows::experience_limit`] and the rest in
    /// the spillover, as the game's own rows split it; gold or playtime as
    /// they are. Refused for a total the areas can't hold.
    pub fn set(&self, words: &mut [u32], field: FigureField, value: u32) -> Result<()> {
        let put = |words: &mut [u32], (bit, width): (u32, u32), v: u32| {
            script::set_bit_field(words, bit, width, v)
        };
        match field {
            FigureField::Experience => match (self.spillover, self.experience_limit) {
                (Some(spill), Some(limit)) => {
                    let first = value.min(limit);
                    put(words, self.experience, first)?;
                    put(words, spill, value - first)
                }
                _ => put(words, self.experience, value),
            },
            FigureField::Gold => put(words, self.gold, value),
            FigureField::Playtime => put(words, self.playtime, value),
        }
    }

    /// The level `experience` reaches: how many thresholds it's at or past.
    pub fn level(&self, experience: u32) -> u32 {
        self.level_xp.iter().filter(|&&t| t <= experience).count() as u32
    }
}

/// The figure's data from its tag data's save area, through AH.2's rows.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Figure {
    /// Its `tag data` structure.
    pub tag_data: u32,
    /// The save area's words, in order.
    pub words: Vec<u32>,
    /// Each word's `ValueInfo`.
    pub cells: Vec<u32>,
    pub experience: u32,
    pub gold: u32,
    pub playtime_secs: u32,
    pub level: u32,
}

/// The figure's data from its `tag data` structure through `rows`: refused
/// when the structure has no save area, a word isn't an integer cell, or a row
/// reads past the words.
pub fn figure(
    g: &mut impl Gather,
    types: &mut Types,
    tag_data: u32,
    rows: &Rows,
) -> Result<Figure> {
    let cells = fields(g, tag_data, "the figure's tag data")?;
    let area = the(&cells, SAVE_AREA, "the figure's tag data")?;
    if area.vtable != vtable::SCRIPT_SET {
        bail!(
            "the figure's `{SAVE_AREA}` at {:#010x} has vtable {:#010x}, not a ScriptSet's",
            area.at,
            area.vtable
        );
    }
    let items = set_items(g, area.at, SAVE_AREA)?;
    let mut words = Vec::with_capacity(items.len());
    for (i, &at) in items.iter().enumerate() {
        match number(g, types, at, &format!("word {i} of the save area"))? {
            Number::Int(v) => words.push(v as u32),
            Number::Float(v) => bail!("word {i} of the save area at {at:#010x} holds a float, {v}"),
        }
    }
    let (experience, gold, playtime_secs) = rows.values(|i| words.get(i as usize).copied())?;
    Ok(Figure {
        tag_data,
        words,
        cells: items,
        experience,
        gold,
        playtime_secs,
        level: rows.level(experience),
    })
}

/// The hero from his placement's variables and his player's joined data.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Hero {
    pub placement: u32,
    pub name: Option<String>,
    pub behaviour: Option<String>,
    pub health: Number,
    pub health_max: Number,
    pub invulnerable: Number,
    /// His joined data's `player`: 0 for player 1.
    pub player: i64,
    /// The experience his next level starts at, as his joined data holds it.
    pub exp_target: Number,
    pub figure: Figure,
    /// The cells: `hp`, `hp max`, `invulnerable`.
    pub cells: [u32; 3],
}

/// The hero from his placement: refused when his script never started or a
/// cell's chain breaks.
pub fn hero(g: &mut impl Gather, types: &mut Types, placement: u32, rows: &Rows) -> Result<Hero> {
    let i = Instance::read(g, placement)?;
    let what = i
        .name
        .clone()
        .unwrap_or_else(|| format!("{placement:#010x}"));
    let vars = i
        .variables
        .clone()
        .with_context(|| format!("{what}'s script never started"))?;
    let num = |g: &mut _, types: &mut Types, path: &[&str]| {
        script::number_at(g, types, &vars, path, &what)
    };
    let (hp_cell, health) = num(g, types, &HP)?;
    let (max_cell, health_max) = num(g, types, &HP_MAX)?;
    let (inv_cell, invulnerable) = num(g, types, &INVULNERABLE)?;
    let joined_at = script::structure_at(g, &vars, &JOINED, &what)?;
    let joined_what = format!("{what}.{}", JOINED.join("."));
    let joined = fields(g, joined_at, &joined_what)?;
    let player = number(g, types, the(&joined, "player", &joined_what)?.at, "player")?.as_int();
    let target = the(&joined, "exp target", &joined_what)?;
    let target_at = script::referent(g, target)?
        .with_context(|| format!("{joined_what}.exp target is a null reference"))?;
    let exp_target = number(g, types, target_at, "the exp target")?;
    let tag = script::structure_at(g, &joined, &TAG_DATA, &joined_what)?;
    Ok(Hero {
        placement,
        name: i.name,
        behaviour: i.behaviour,
        health,
        health_max,
        invulnerable,
        player,
        exp_target,
        figure: figure(g, types, tag, rows)?,
        cells: [hp_cell.at, max_cell.at, inv_cell.at],
    })
}

/// One `EnemyStatsList` row: the template's numbers for the level's
/// difficulty, as `Game - Populate Enemy Stats List` left them.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EnemyStat {
    /// The template it describes, by name.
    pub kind: Option<String>,
    pub hp: Number,
    pub xp: Number,
}

/// The level's `EnemyStatsList` as `Macros_UI` holds it, in its order:
/// refused when the list isn't among its variables or a row's chain breaks.
pub fn enemy_stats(
    g: &mut impl Gather,
    types: &mut Types,
    macros_ui: u32,
) -> Result<Vec<EnemyStat>> {
    let vars = script::variables(g, macros_ui)?.context("Macros_UI's script never started")?;
    let list = the(&vars, "EnemyStatsList", MACROS_UI)?;
    let rows = set_items(g, list.at, "EnemyStatsList")?;
    let mut out = Vec::with_capacity(rows.len());
    for (i, &row) in rows.iter().enumerate() {
        let what = format!("EnemyStatsList[{i}]");
        let cells = fields(g, row, &what)?;
        let kind_ref = the(&cells, "Enemy Type", &what)?;
        let kind = match script::referent(g, kind_ref)? {
            Some(p) => script::named(g, &[p])?.pop().map(|n| n.name),
            None => None,
        };
        out.push(EnemyStat {
            kind,
            hp: number(
                g,
                types,
                the(&cells, "hp", &what)?.at,
                &format!("{what}.hp"),
            )?,
            xp: number(
                g,
                types,
                the(&cells, "xp", &what)?.at,
                &format!("{what}.xp"),
            )?,
        });
    }
    Ok(out)
}

/// A level-file cell read in place in the level's data section.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LevelValue {
    pub name: String,
    pub at: u32,
    pub value: Number,
}

/// Each named `ValueInfo` at `base` plus its offset in the level's section:
/// refused when what's there isn't a `ValueInfo` carrying that name.
pub fn level_values(
    g: &mut impl Gather,
    types: &mut Types,
    base: u32,
    cells: &[(&str, u32)],
) -> Result<Vec<LevelValue>> {
    let at: Vec<u32> = cells.iter().map(|&(_, off)| base + off).collect();
    let found = script::named(g, &at)?;
    cells
        .iter()
        .zip(at)
        .map(|(&(name, _), at)| {
            let cell = found
                .iter()
                .find(|c| c.at == at)
                .with_context(|| format!("`{name}` at {at:#010x} isn't readable or has no name"))?;
            if cell.name != name {
                bail!("the cell at {at:#010x} is `{}`, not `{name}`", cell.name);
            }
            Ok(LevelValue {
                name: name.to_owned(),
                at,
                value: number(g, types, at, &format!("`{name}`"))?,
            })
        })
        .collect()
}

/// A value out of the console save's words, through its row in `console save
/// params`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConsoleValue {
    /// For the difficulty: 0 easy, 1 medium, 2 hard, 3 nightmare, as the
    /// enemy stats read it.
    pub value: i64,
    /// Its first bit in the words' stream (the row's `bit index` plus `num
    /// bits` for each field before it) and its width.
    pub bit: u32,
    pub bits: u32,
    pub signed: bool,
    /// Read from the header's words rather than the save's.
    pub header: bool,
    /// The words it was read from.
    pub words: usize,
}

/// The `field`-th value of the row `row` names among the console save's
/// cells: refused when the row or its words' chain breaks.
fn console_value(
    g: &mut impl Gather,
    types: &mut Types,
    cells: &[Named],
    row: &str,
    field: u32,
) -> Result<ConsoleValue> {
    let row_at = script::structure_at(g, cells, &[row], CONSOLE_SAVE)?;
    let row_cells = fields(g, row_at, row)?;
    let mut get = |name: &str| -> Result<i64> {
        Ok(number(g, types, the(&row_cells, name, row)?.at, name)?.as_int())
    };
    let (first, bits, signed, header) = (
        get("bit index")?,
        get("num bits")?,
        get("signed")? != 0,
        get("header param")? != 0,
    );
    let (Ok(first), Ok(bits)) = (u32::try_from(first), u32::try_from(bits)) else {
        bail!("`{row}` reads bit {first}, {bits} bits");
    };
    let bit = first + bits * field;
    let set_name = if header {
        "header save val set"
    } else {
        "save val set"
    };
    let set_ref = the(cells, set_name, CONSOLE_SAVE)?;
    if set_ref.vtable != vtable::SET_REFERENCE {
        bail!(
            "`{set_name}` at {:#010x} has vtable {:#010x}, not a set reference's",
            set_ref.at,
            set_ref.vtable
        );
    }
    let set = script::referent(g, set_ref)?
        .with_context(|| format!("`{set_name}` is a null reference"))?;
    let items = set_items(g, set, set_name)?;
    let mut words = Vec::with_capacity(items.len());
    for (i, &at) in items.iter().enumerate() {
        words.push(number(g, types, at, &format!("{set_name}[{i}]"))?.as_int() as u32);
    }
    Ok(ConsoleValue {
        value: bit_field(&words, bit, bits, signed)
            .with_context(|| format!("`{row}` field {field}"))?,
        bit,
        bits,
        signed,
        header,
        words: words.len(),
    })
}

/// The difficulty out of `console save params`' `option - difficulty`.
pub fn difficulty(
    g: &mut impl Gather,
    types: &mut Types,
    console_save: u32,
) -> Result<ConsoleValue> {
    let cells = fields(g, console_save, CONSOLE_SAVE)?;
    console_value(g, types, &cells, "option - difficulty", 0)
}

/// The portal-master experience multiplier and what sets it: `Save - Accolade
/// Set XP Multiplier` counts the collection accolades `level progression
/// data.accolade checks.xp multiplier accolades` lists that the console save
/// marks done (each its `save bit index`-th field of `collection -
/// accolades`) and takes the count's entry of `xp multiplier table` (the
/// first for one), 1 for none.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct XpMultiplier {
    /// `combat control data.accolade xp multiplier`.
    pub value: Number,
    /// Each accolade that counts, by name, and whether the save marks it done.
    pub accolades: Vec<(String, bool)>,
    pub table: Vec<Number>,
}

impl XpMultiplier {
    pub fn completed(&self) -> usize {
        self.accolades.iter().filter(|(_, done)| *done).count()
    }

    /// What the game's rule gives for the accolades done: none past the table.
    pub fn by_rule(&self) -> Option<f64> {
        match self.completed() {
            0 => Some(1.0),
            n => self.table.get(n - 1).map(|v| v.as_f64()),
        }
    }
}

/// The multiplier, the accolades and the table: refused when a chain breaks.
pub fn xp_multiplier(
    g: &mut impl Gather,
    types: &mut Types,
    combat: &Named,
    level_progression: u32,
    console_save: u32,
) -> Result<XpMultiplier> {
    let (_, value) = script::number_at(
        g,
        types,
        std::slice::from_ref(combat),
        &[COMBAT_CONTROL, "accolade xp multiplier"],
        "global.bld",
    )?;
    let progression = fields(g, level_progression, LEVEL_PROGRESSION)?;
    let checks_at = script::structure_at(g, &progression, &["accolade checks"], LEVEL_PROGRESSION)?;
    let checks = fields(g, checks_at, "accolade checks")?;
    let table_set = the(&checks, "xp multiplier table", "accolade checks")?;
    let table = set_items(g, table_set.at, "xp multiplier table")?
        .iter()
        .enumerate()
        .map(|(i, &at)| number(g, types, at, &format!("xp multiplier table[{i}]")))
        .collect::<Result<Vec<_>>>()?;
    let listed = the(&checks, "xp multiplier accolades", "accolade checks")?;
    let console = fields(g, console_save, CONSOLE_SAVE)?;
    let mut accolades = Vec::new();
    for (i, &at) in set_items(g, listed.at, "xp multiplier accolades")?
        .iter()
        .enumerate()
    {
        let what = format!("xp multiplier accolades[{i}]");
        let name = script::named(g, &[at])?
            .pop()
            .with_context(|| format!("{what} has no name"))?
            .name;
        let cells = fields(g, at, &what)?;
        let index = number(
            g,
            types,
            the(&cells, "save bit index", &what)?.at,
            "save bit index",
        )?
        .as_int();
        let Ok(index) = u32::try_from(index) else {
            bail!("{what} names save bit {index}");
        };
        let done = console_value(g, types, &console, "collection - accolades", index)?;
        accolades.push((name, done.value != 0));
    }
    Ok(XpMultiplier {
        value,
        accolades,
        table,
    })
}

/// A loot running count: a drop happens when it's at or under the rate after
/// the rate is added (and 1 taken off past 1).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Accumulator {
    /// The reference's name ([`LOOT`]), or `XpLoot` and the enemy type.
    pub name: String,
    pub index: Number,
    pub rate: Number,
    /// The `DropRateIndex` cell.
    pub index_cell: u32,
}

/// The loot data's running counts as `Macros_UI` holds them: [`LOOT`]'s, then
/// each `XpLoot` entry's. Refused when one of [`LOOT`] or a cell's chain breaks.
pub fn loot(g: &mut impl Gather, types: &mut Types, macros_ui: u32) -> Result<Vec<Accumulator>> {
    let vars = script::variables(g, macros_ui)?.context("Macros_UI's script never started")?;
    let data_at = script::structure_at(g, &vars, &["LootData"], MACROS_UI)?;
    let data = fields(g, data_at, "LootData")?;
    let mut out = Vec::new();
    let mut read = |g: &mut _, types: &mut Types, cells: &[Named], name: String, what: &str| {
        let (index, i) = script::number_at(g, types, cells, &["DropRateIndex"], what)?;
        let (_, rate) = script::number_at(g, types, cells, &["DropRate"], what)?;
        out.push(Accumulator {
            name,
            index: i,
            rate,
            index_cell: index.at,
        });
        anyhow::Ok(())
    };
    for name in LOOT {
        let what = format!("LootData.{name}");
        let at = script::structure_at(g, &data, &[name], "LootData")?;
        let cells = fields(g, at, &what)?;
        read(g, types, &cells, name.to_owned(), &what)?;
    }
    let xp = the(&data, "XpLoot", "LootData")?;
    for (i, &entry) in set_items(g, xp.at, "LootData.XpLoot")?.iter().enumerate() {
        let what = format!("LootData.XpLoot[{i}]");
        let cells = fields(g, entry, &what)?;
        let kind = match script::referent(g, the(&cells, "EnemyType", &what)?)? {
            Some(p) => script::named(g, &[p])?.pop().map(|n| n.name),
            None => None,
        };
        let name = format!("XpLoot {}", kind.as_deref().unwrap_or("(none)"));
        read(g, types, &cells, name, &what)?;
    }
    Ok(out)
}

/// The element-area exports: each player's flag and `ZoneElement_Current`
/// (-1 at the spawn in AF.8's fresh state).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Area {
    pub p1: Number,
    pub p2: Number,
    pub current: Number,
    /// The flags' cells, players 1 and 2.
    pub cells: [u32; 2],
}

/// A trigger placement's wake state.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Trigger {
    pub name: String,
    pub placement: u32,
    pub activity_state: u32,
    pub started: bool,
    pub active_seconds: f32,
    /// Its live activation range, feet.
    pub range: f32,
    /// For a range trigger, whether it has been active: it is (state 1), was
    /// and has been taken off (3), or its clock or started script shows it.
    /// None for one without a range, which is awake from the level's start
    /// and fires when its controller's own check finds a player in it.
    pub fired: Option<bool>,
}

/// Each named placement's trigger state: refused when the placement at an
/// address isn't a `tfbActorInfo` carrying that name.
pub fn triggers(g: &mut impl Gather, placements: &[(&str, u32)]) -> Result<Vec<Trigger>> {
    placements
        .iter()
        .map(|&(name, placement)| {
            let i = Instance::read(g, placement)?;
            if i.name.as_deref() != Some(name) {
                bail!(
                    "the placement at {placement:#010x} is {:?}, not `{name}`",
                    i.name
                );
            }
            Ok(Trigger {
                name: name.to_owned(),
                placement,
                activity_state: i.activity_state,
                started: i.started,
                active_seconds: i.active_seconds,
                range: i.activation_range,
                fired: (i.activation_range > 0.0).then_some(
                    matches!(i.activity_state, 1 | 3) || i.started || i.active_seconds > 0.0,
                ),
            })
        })
        .collect()
}

/// Everything AH.4 reads but the level's own cells and the triggers, which
/// need the disc's offsets.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FightState {
    pub hero: Hero,
    pub difficulty: ConsoleValue,
    pub xp_multiplier: XpMultiplier,
    pub area: Area,
    /// The flag of the hero's own player.
    pub area_at_hero: Number,
    pub enemy_stats: Vec<EnemyStat>,
    pub loot: Vec<Accumulator>,
    /// The `FloatMeasurement` metaobject, as the cells name it.
    pub float_type: Option<u32>,
}

/// The hero's state, then `global.bld`'s exports and what `Macros_UI` holds,
/// in one pass over the handle table.
pub fn read(g: &mut impl Gather, hero_placement: u32, rows: &Rows) -> Result<FightState> {
    let mut types = Types::default();
    let mut names = vec![
        MACROS_UI,
        CONSOLE_SAVE,
        COMBAT_CONTROL,
        LEVEL_PROGRESSION,
        AREA_CURRENT,
    ];
    names.extend(AREA_FLAGS);
    let ex = exports(g, script::GLOBAL, &names)?;
    let class = |name: &str, want: u32, class: &str| -> Result<u32> {
        let e = ex[name];
        if e.vtable != want {
            bail!(
                "the export `{name}` at {:#010x} has vtable {:#010x}, not {class}'s",
                e.at,
                e.vtable
            );
        }
        Ok(e.at)
    };
    let macros_ui = class(MACROS_UI, vtable::ACTOR_INFO, "tfbActorInfo")?;
    let console = class(CONSOLE_SAVE, vtable::SCRIPT_STRUCTURE, "ScriptStructure")?;
    let combat = Named {
        at: class(COMBAT_CONTROL, vtable::SCRIPT_REFERENCE, "ScriptReference")?,
        vtable: vtable::SCRIPT_REFERENCE,
        name: COMBAT_CONTROL.to_owned(),
    };
    let flags = [
        class(AREA_FLAGS[0], vtable::VALUE_INFO, "ValueInfo")?,
        class(AREA_FLAGS[1], vtable::VALUE_INFO, "ValueInfo")?,
    ];
    let current = class(AREA_CURRENT, vtable::VALUE_INFO, "ValueInfo")?;
    let progression = class(
        LEVEL_PROGRESSION,
        vtable::SCRIPT_STRUCTURE,
        "ScriptStructure",
    )?;

    let hero = hero(g, &mut types, hero_placement, rows)?;
    let xp_multiplier = xp_multiplier(g, &mut types, &combat, progression, console)?;
    let area = Area {
        p1: number(g, &mut types, flags[0], AREA_FLAGS[0])?,
        p2: number(g, &mut types, flags[1], AREA_FLAGS[1])?,
        current: number(g, &mut types, current, AREA_CURRENT)?,
        cells: flags,
    };
    let area_at_hero = match hero.player {
        0 => area.p1,
        1 => area.p2,
        p => bail!("the hero's joined data says player {p}"),
    };
    let state = FightState {
        difficulty: difficulty(g, &mut types, console)?,
        xp_multiplier,
        area,
        area_at_hero,
        enemy_stats: enemy_stats(g, &mut types, macros_ui)?,
        loot: loot(g, &mut types, macros_ui)?,
        hero,
        float_type: None,
    };
    Ok(FightState {
        float_type: types.float_type(),
        ..state
    })
}

/// [`read`] over PINE: Giants 01.00 only, paused before and after.
pub fn read_paused(pine: &mut PineClient, hero_placement: u32, rows: &Rows) -> Result<FightState> {
    crate::rng::check_title(pine)?;
    let s = read(pine, hero_placement, rows)?;
    crate::level::still_paused(pine, "the fight's read")?;
    Ok(s)
}

/// The fight's cells that move frame by frame, on Giants' watch list: the
/// hero's health, full health and invulnerability, the figure's words AH.2's
/// rows read experience, gold and playtime from, both players' element-area
/// flags and the destructibles' common loot count, each cell's type word and
/// value, in blocks of cells near each other; and, when named, triggers'
/// wake state.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FightWatch {
    /// Each cell by label (`hp`, `hp max`, `invulnerable`, `word.<n>`,
    /// `area.p1`, `area.p2`, `loot`), with its `ValueInfo`.
    pub cells: Vec<(String, u32)>,
    /// The `FloatMeasurement` metaobject: a cell whose type word is this reads
    /// as a float, anything else as an integer.
    pub float_type: u32,
    pub rows: Rows,
    /// Trigger placements by name ([`FightWatch::with_triggers`]): each
    /// one's `_activityState` to its `_activeTime` (+0x54 to +0xB8).
    #[serde(default)]
    pub triggers: Vec<(String, u32)>,
}

/// A watched trigger in a record.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TriggerAt {
    pub name: String,
    pub activity_state: u32,
    pub active_seconds: f32,
    /// `_execBehavior` (+0xAC): the `OpUserBehavior` its script runs, 0
    /// before its STARTUP picks one (named at a pause, AH.8).
    #[serde(default)]
    pub behaviour: u32,
}

/// The fight's cells decoded from one O5 record.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FightFrame {
    pub health: Number,
    pub health_max: Number,
    pub invulnerable: Number,
    pub experience: u32,
    pub gold: u32,
    pub playtime_secs: u32,
    pub level: u32,
    pub area_p1: Number,
    pub area_p2: Number,
    pub loot: Number,
    /// The destructibles' uncommon and rare counts, when the watch holds
    /// them (AH.9).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub loot_uncommon: Option<Number>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub loot_rare: Option<Number>,
    pub triggers: Vec<TriggerAt>,
}

const TRIGGER_FIRST: u32 = 0x54;
const TRIGGER_LEN: u32 = 0xB8 - TRIGGER_FIRST;

impl FightWatch {
    /// The cells `state` was read from: refused when the state carries no
    /// float type or no destructibles' common count, or the rows read a word
    /// past the figure's save area.
    pub fn of(state: &FightState, rows: &Rows) -> Result<Self> {
        let float_type = state
            .float_type
            .context("none of the fight's cells is a FloatMeasurement")?;
        let common = state
            .loot
            .iter()
            .find(|a| a.name == LOOT[0])
            .context("no destructibles' common loot count")?;
        let h = &state.hero;
        let mut cells = vec![
            ("hp".to_owned(), h.cells[0]),
            ("hp max".to_owned(), h.cells[1]),
            ("invulnerable".to_owned(), h.cells[2]),
            ("area.p1".to_owned(), state.area.cells[0]),
            ("area.p2".to_owned(), state.area.cells[1]),
            ("loot".to_owned(), common.index_cell),
        ];
        // The uncommon and rare counts too (AH.9: which breakables step
        // which), where the loot data holds them.
        for (label, name) in [("loot.uncommon", LOOT[1]), ("loot.rare", LOOT[2])] {
            if let Some(a) = state.loot.iter().find(|a| a.name == name) {
                cells.push((label.to_owned(), a.index_cell));
            }
        }
        for w in rows.words() {
            let at = h
                .figure
                .cells
                .get(w as usize)
                .with_context(|| format!("the rows read word {w}, past the save area"))?;
            cells.push((format!("word.{w}"), *at));
        }
        Ok(Self {
            cells,
            float_type,
            rows: rows.clone(),
            triggers: Vec::new(),
        })
    }

    /// The watch with these triggers' wake state on it too, `trigger.<name>`
    /// each (AH.8 times the route's from them).
    pub fn with_triggers(mut self, triggers: &[Trigger]) -> Self {
        self.triggers = triggers
            .iter()
            .map(|t| (t.name.clone(), t.placement))
            .collect();
        self
    }

    /// The cells grouped into blocks by address, each block from its first
    /// cell's type word to its last's value, no wider than [`MAX_SPAN`].
    fn blocks(&self) -> Vec<(u32, u32, Vec<usize>)> {
        let mut order: Vec<usize> = (0..self.cells.len()).collect();
        order.sort_by_key(|&i| self.cells[i].1);
        let mut out: Vec<(u32, u32, Vec<usize>)> = Vec::new();
        for i in order {
            let at = self.cells[i].1;
            match out.last_mut() {
                Some((lo, hi, members)) if at + VALUE_END - *lo <= MAX_SPAN => {
                    *hi = (*hi).max(at + VALUE_END);
                    members.push(i);
                }
                _ => out.push((at + VALUE_TYPE, at + VALUE_END, vec![i])),
            }
        }
        out
    }

    /// The watch-list fields, `fight.<n>` per block and `trigger.<name>` per
    /// trigger.
    pub fn list_fields(&self) -> Vec<Field> {
        self.blocks()
            .iter()
            .enumerate()
            .map(|(n, (lo, hi, _))| Field::new(format!("fight.{n}"), *lo, hi - lo))
            .chain(self.triggers.iter().map(|(name, at)| {
                Field::new(format!("trigger.{name}"), at + TRIGGER_FIRST, TRIGGER_LEN)
            }))
            .collect()
    }

    /// The fight's cells in a record: none when a block wasn't readable.
    pub fn read(&self, list: &WatchList, record: &WatchRecord) -> Option<FightFrame> {
        let mut values = BTreeMap::new();
        for (n, (lo, _, members)) in self.blocks().iter().enumerate() {
            let bytes = list.field(record, &format!("fight.{n}"))?;
            for &i in members {
                let (label, at) = &self.cells[i];
                let off = (at + VALUE_TYPE - lo) as usize;
                let value = if be32(bytes, off) == self.float_type {
                    Number::Float(f32_at(bytes, off + 4))
                } else {
                    Number::Int(be32(bytes, off + 4) as i32)
                };
                values.insert(label.clone(), value);
            }
        }
        let word = |i: u32| match values.get(&format!("word.{i}")) {
            Some(Number::Int(v)) => Some(*v as u32),
            _ => None,
        };
        let (experience, gold, playtime_secs) = self.rows.values(word).ok()?;
        let triggers = self
            .triggers
            .iter()
            .map(|(name, _)| {
                let b = list.field(record, &format!("trigger.{name}"))?;
                Some(TriggerAt {
                    name: name.clone(),
                    activity_state: be32(b, 0),
                    active_seconds: f32_at(b, (0xB4 - TRIGGER_FIRST) as usize),
                    behaviour: be32(b, (0xAC - TRIGGER_FIRST) as usize),
                })
            })
            .collect::<Option<Vec<_>>>()?;
        Some(FightFrame {
            health: values["hp"],
            health_max: values["hp max"],
            invulnerable: values["invulnerable"],
            experience,
            gold,
            playtime_secs,
            level: self.rows.level(experience),
            area_p1: values["area.p1"],
            area_p2: values["area.p2"],
            loot: values["loot"],
            loot_uncommon: values.get("loot.uncommon").copied(),
            loot_rare: values.get("loot.rare").copied(),
            triggers,
        })
    }
}

/// The fight's state laid out in a fixture's memory, as AF.8's fresh state
/// holds it for Eruptor (the numbers his and the level's there).
#[cfg(test)]
pub(crate) mod fixture {
    use super::*;
    use crate::actors::fixture::{Spec, World};
    use crate::script::fixture::{self as fx, Types as FxTypes};
    use guac_ir::progression::{SaveField, SaveParam};
    use guac_ir::{DumpIdentity, Sha256};

    /// Where [`fight`] put things.
    pub struct Fixture {
        pub w: World,
        pub t: FxTypes,
        pub hero: u32,
        pub hp: u32,
        pub joined: u32,
        pub tag: u32,
        pub words: Vec<u32>,
        pub macros_ui: u32,
        pub console: u32,
        pub combat_ref: u32,
        pub save_words: Vec<u32>,
        pub listed: u32,
        pub flags: [u32; 2],
        pub enemy_list: u32,
        pub common_index: u32,
        pub xp_loot: u32,
        pub level_cells: Vec<(String, u32)>,
        pub level_base: u32,
    }

    fn row(param: &str, kind: SaveKind, word: u32, bit: u32, width: u32, limit: u32) -> SaveParam {
        SaveParam {
            param: param.into(),
            kind,
            fields: vec![SaveField {
                word,
                bit,
                member: None,
            }],
            width,
            bits_per_field: 0,
            limit,
        }
    }

    /// Giants' tables as AH.2 reads them, the rows the readers use.
    pub fn progression() -> Progression {
        Progression {
            identity: DumpIdentity {
                title_id: "BLUS30968".into(),
                app_ver: "01.00".into(),
                manifest_hash: Sha256([0; 32]),
            },
            level_xp: vec![
                0, 1000, 2200, 3800, 6000, 9000, 13000, 18200, 24800, 33000, 42700, 53900, 66600,
                80800, 96500,
            ],
            save_params: vec![
                row("pp_Exp", SaveKind::Plain, 0, 0, 24, 33000),
                row("pp_Money", SaveKind::Plain, 0, 24, 16, 65535),
                row("pp_CumulativeTime", SaveKind::Plain, 1, 8, 32, 0),
                row("pp_Hat", SaveKind::Plain, 5, 0, 16, 45),
                row("pp_Exp", SaveKind::Spillover, 28, 24, 16, 63500),
            ],
            gold_cap: 65000,
        }
    }

    /// Eruptor, his player's joined data and figure (`xp`, `gold`, `secs` in
    /// his save area), the exports and what they hold.
    pub fn fight(xp: u32, gold: u32, secs: u32) -> Fixture {
        let mut w = World::new();
        let hero = w
            .actor(&Spec {
                name: "009_Eruptor".into(),
                skinned: true,
                pad: Some(-1),
                ..Spec::default()
            })
            .placement;
        fight_on(w, hero, xp, gold, secs)
    }

    /// [`fight`]'s state for the hero already at `hero` in `w`.
    pub fn fight_on(mut w: World, hero: u32, xp: u32, gold: u32, secs: u32) -> Fixture {
        let t = fx::types(&mut w);
        // The hero's cells.
        let hp_max = fx::int(&mut w, &t, "hp max", 290);
        let exp_value = fx::float(&mut w, &t, "exp value", 1.0);
        let params = fx::structure(&mut w, "char vars base", &[exp_value, hp_max]);
        let params_ref = fx::reference(
            &mut w,
            vtable::SCRIPT_REFERENCE,
            "character parameters",
            params,
        );
        let invulnerable = fx::int(&mut w, &t, "invulnerable", 0);
        let armor = fx::float(&mut w, &t, "armor", 6.0);
        let hp = fx::int(&mut w, &t, "hp", 290);
        let info = fx::structure(&mut w, "", &[params_ref, invulnerable, armor, hp]);
        let info_ref = fx::reference(&mut w, vtable::SCRIPT_REFERENCE, "creature info", info);
        let description = fx::structure(&mut w, "", &[info_ref]);
        let description_ref =
            fx::reference(&mut w, vtable::SCRIPT_REFERENCE, "description", description);
        // His figure: 44 words, experience across both areas.
        let mut words = [0u32; 44];
        let first = xp.min(33000);
        let spill = xp - first;
        words[0] = first | (gold & 0xFF) << 24;
        words[1] = (gold >> 8) & 0xFF | (secs & 0x00FF_FFFF) << 8;
        words[2] = secs >> 24;
        words[28] = (spill & 0xFF) << 24;
        words[29] = spill >> 8;
        let cells: Vec<u32> = words
            .iter()
            .map(|&v| fx::int(&mut w, &t, "", v as i32))
            .collect();
        let area = fx::set(&mut w, "save data area", &cells);
        let serial = fx::int(&mut w, &t, "toy serial", 10);
        let tag = fx::structure(&mut w, "", &[serial, area]);
        let tag_ref = fx::reference(&mut w, vtable::SCRIPT_REFERENCE, "tag data", tag);
        let object_info = fx::structure(&mut w, "portal object base", &[tag_ref]);
        let oi_ref = fx::reference(&mut w, vtable::SCRIPT_REFERENCE, "object info", object_info);
        let loaded = fx::structure(&mut w, "", &[oi_ref]);
        let loaded_ref = fx::reference(&mut w, vtable::SCRIPT_REFERENCE, "loaded data", loaded);
        let target = fx::int(&mut w, &t, "", 1000);
        let target_ref = fx::reference(&mut w, vtable::SCRIPT_REFERENCE, "exp target", target);
        let player = fx::int(&mut w, &t, "player", 0);
        let joined = fx::structure(&mut w, "", &[loaded_ref, target_ref, player]);
        let joined_ref = fx::reference(
            &mut w,
            vtable::SCRIPT_REFERENCE,
            "player joined data",
            joined,
        );
        let in_level = fx::int(&mut w, &t, "in level up", 0);
        let mine = fx::structure(&mut w, "my Minion Description", &[in_level, joined_ref]);
        let timer = fx::int(&mut w, &t, "Timer", 0);
        let list = w.list(vtable::OBJECT_LIST, &[timer, description_ref, mine], 4);
        w.put32(hero + 0x1C, 0x8);
        w.put32(hero + 0xB0, list);
        let op = fx::named(&mut w, vtable::USER_BEHAVIOR, 0x28, "default");
        w.put32(hero + 0xAC, op);

        // Macros_UI: the enemy stats and the loot data.
        let row = |w: &mut World, kind: &str, hp: i32, xp: Option<i32>| {
            let template = fx::placement(w, kind, None, 0.0, None);
            let kind_ref = fx::reference(w, vtable::PLACEMENT_REFERENCE, "Enemy Type", template);
            let hp = fx::int(w, &t, "hp", hp);
            let xp = match xp {
                Some(v) => fx::int(w, &t, "xp", v),
                None => fx::float(w, &t, "xp", 128.333_34),
            };
            fx::structure(w, "", &[hp, xp, kind_ref])
        };
        let rows = [
            row(&mut w, "PandoranGrunt", 59, None),
            row(&mut w, "PandoranGruntSimple", 33, None),
            row(&mut w, "Enemy_Chompy", 1, Some(4)),
        ];
        let enemy_list = fx::set(&mut w, "EnemyStatsList", &rows);
        let accumulator = |w: &mut World, index: f32, rate: f32| {
            let i = fx::float(w, &t, "DropRateIndex", index);
            let r = fx::float(w, &t, "DropRate", rate);
            (fx::structure(w, "", &[i, r]), i)
        };
        let (common, common_index) = accumulator(&mut w, 0.824_279_5, 0.6);
        let (uncommon, _) = accumulator(&mut w, 0.097_497, 1.0);
        let (rare, _) = accumulator(&mut w, 0.570_767_6, 1.0);
        let (placed, _) = accumulator(&mut w, 0.0, 0.0);
        let refs: Vec<u32> = [
            ("DestructiblesCommonRef", common),
            ("DestructiblesUncommonRef", uncommon),
            ("DestructiblesRareRef", rare),
            ("PlacedLootCommonRef", placed),
        ]
        .into_iter()
        .map(|(n, s)| fx::reference(&mut w, vtable::SCRIPT_REFERENCE, n, s))
        .collect();
        let grunt = fx::placement(&mut w, "PandoranGrunt", None, 0.0, None);
        let grunt_ref = fx::reference(&mut w, vtable::PLACEMENT_REFERENCE, "EnemyType", grunt);
        let (grunt_loot, _) = accumulator(&mut w, 0.25, 0.6);
        let gl_list = w.get32(grunt_loot + 0x18);
        let gl_data = w.get32(gl_list + 0x14);
        let (i0, i1) = (w.get32(gl_data), w.get32(gl_data + 4));
        let grunt_loot = fx::structure(&mut w, "", &[grunt_ref, i0, i1]);
        let xp_loot = fx::set(&mut w, "XpLoot", &[grunt_loot]);
        let mut data_cells = refs.clone();
        data_cells.push(xp_loot);
        let loot_data = fx::structure(&mut w, "LootData", &data_cells);
        let loot_index = fx::int(&mut w, &t, "LootIndex", 0);
        let macros_ui = fx::placement(
            &mut w,
            MACROS_UI,
            None,
            99.484,
            Some(&[enemy_list, loot_data, loot_index]),
        );

        // The console save: 178 words, medium (1) at bit 2,549.
        let mut save: Vec<u32> = (0..178).map(|_| 0).collect();
        save[79] = 1 << 21;
        // Captain's save bit (1) of the collection accolades from bit 8.
        save[0] = 1 << 9;
        let save_cells: Vec<u32> = save
            .iter()
            .map(|&v| fx::int(&mut w, &t, "", v as i32))
            .collect();
        let save_set = fx::set(&mut w, "SaveGame", &save_cells);
        let save_ref = fx::reference(&mut w, vtable::SET_REFERENCE, "save val set", save_set);
        let header_set = fx::set(&mut w, "SaveGame_Header", &[]);
        let header_ref = fx::reference(
            &mut w,
            vtable::SET_REFERENCE,
            "header save val set",
            header_set,
        );
        let opt = [
            fx::int(&mut w, &t, "limit to val", 0),
            fx::int(&mut w, &t, "fields total", 1),
            fx::int(&mut w, &t, "num bits", 4),
            fx::int(&mut w, &t, "bit index", 2549),
            fx::int(&mut w, &t, "signed", 0),
            fx::int(&mut w, &t, "header param", 0),
        ];
        let option = fx::structure(&mut w, "option - difficulty", &opt);
        let acc = [
            fx::int(&mut w, &t, "fields total", 32),
            fx::int(&mut w, &t, "num bits", 1),
            fx::int(&mut w, &t, "bit index", 8),
            fx::int(&mut w, &t, "signed", 0),
            fx::int(&mut w, &t, "header param", 0),
        ];
        let collection = fx::structure(&mut w, "collection - accolades", &acc);
        let console = fx::structure(
            &mut w,
            CONSOLE_SAVE,
            &[option, collection, save_ref, header_ref],
        );
        // The accolades that count toward the multiplier, and its table.
        let accolade = |w: &mut World, name: &str, bit: i32| {
            let b = fx::int(w, &t, "save bit index", bit);
            fx::structure(w, name, &[b])
        };
        let listed = [
            accolade(&mut w, "ambassador - 1 toy of every element", 0),
            accolade(&mut w, "captain - non-starter pack toy", 1),
        ];
        let listed = fx::set(&mut w, "xp multiplier accolades", &listed);
        let table: Vec<u32> = [1.1f32, 1.2, 1.35]
            .iter()
            .map(|&v| fx::float(&mut w, &t, "", v))
            .collect();
        let table = fx::set(&mut w, "xp multiplier table", &table);
        let checks = fx::structure(&mut w, "accolade checks", &[listed, table]);
        let progression_data = fx::structure(&mut w, LEVEL_PROGRESSION, &[checks]);

        // Combat control, the area flags.
        let multiplier = fx::float(&mut w, &t, "accolade xp multiplier", 1.1);
        let combat = fx::structure(&mut w, "", &[multiplier]);
        let combat_ref = fx::reference(&mut w, vtable::SCRIPT_REFERENCE, COMBAT_CONTROL, combat);
        let p1 = fx::int(&mut w, &t, AREA_FLAGS[0], 0);
        let p2 = fx::int(&mut w, &t, AREA_FLAGS[1], 0);
        let current = fx::int(&mut w, &t, AREA_CURRENT, -1);
        fx::handles(
            &mut w,
            script::GLOBAL,
            &[
                (0x4B9, macros_ui),
                (0x520, console),
                (0x508, combat_ref),
                (0x518, progression_data),
                (0xA21, p1),
                (0xA22, p2),
                (0xA20, current),
            ],
        );

        // The level file's cells: a section of their own.
        let level_base = w.alloc(0);
        let level_cells: Vec<(String, u32)> = [
            ("Enemy_Chompy_DirectDMG", Some(11), None),
            ("PandoranGrunt_DMG", Some(36), None),
            ("XP_Value_Near", None, Some(128.333_34f32)),
        ]
        .into_iter()
        .map(|(n, i, f)| {
            let at = match (i, f) {
                (Some(v), _) => fx::int(&mut w, &t, n, v),
                (_, Some(v)) => fx::float(&mut w, &t, n, v),
                _ => unreachable!(),
            };
            (n.to_owned(), at - level_base)
        })
        .collect();
        Fixture {
            w,
            t,
            hero,
            hp,
            joined,
            tag,
            words: cells,
            macros_ui,
            console,
            combat_ref,
            save_words: save_cells,
            listed,
            flags: [p1, p2],
            enemy_list,
            common_index,
            xp_loot,
            level_cells,
            level_base,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::fixture::{fight, progression};
    use super::*;
    use crate::script::fixture as fx;

    fn rows() -> Rows {
        Rows::of(&progression()).unwrap()
    }

    fn err(r: Result<impl std::fmt::Debug>) -> String {
        format!("{:#}", r.unwrap_err())
    }

    #[test]
    fn rows_come_from_ah2s_tables_and_read_both_areas() {
        let r = rows();
        assert_eq!(r.experience, (0, 24));
        assert_eq!(r.spillover, Some((28 * 32 + 24, 16)));
        assert_eq!((r.gold, r.playtime), ((24, 16), (40, 32)));
        assert_eq!(r.words(), [0, 1, 2, 28, 29]);
        for (xp, level) in [(0, 1), (999, 1), (1000, 2), (96_499, 14), (96_500, 15)] {
            assert_eq!(r.level(xp), level, "{xp}");
        }
        // A table without a plain gold row, or an empty level table.
        let mut p = progression();
        p.save_params.retain(|s| s.param != "pp_Money");
        assert!(err(Rows::of(&p)).contains("pp_Money"));
        let mut p = progression();
        p.level_xp.clear();
        assert!(err(Rows::of(&p)).contains("level table"));
        // No spillover row: experience is the first area's alone.
        let mut p = progression();
        p.save_params.retain(|s| s.kind != SaveKind::Spillover);
        let r = Rows::of(&p).unwrap();
        assert_eq!(r.spillover, None);
        assert_eq!(r.words(), [0, 1, 2]);
    }

    /// AH.11's figure pokes: experience as a total over both areas as the
    /// first area's limit splits it, gold and playtime as they are, each
    /// read back through the rows and nothing else moved.
    #[test]
    fn a_figure_value_set_reads_back_through_the_rows() {
        let r = rows();
        assert_eq!(r.experience_limit, Some(33_000));
        let mut words = vec![0u32; 44];
        let read = |w: &[u32]| r.values(|i| w.get(i as usize).copied()).unwrap();
        r.set(&mut words, FigureField::Gold, 17).unwrap();
        r.set(&mut words, FigureField::Playtime, 34).unwrap();
        r.set(&mut words, FigureField::Experience, 999).unwrap();
        assert_eq!(read(&words), (999, 17, 34));
        assert_eq!(words[28] >> 24, 0);
        // 96,499: 33,000 in the first area, 63,499 spilled over.
        r.set(&mut words, FigureField::Experience, 96_499).unwrap();
        assert_eq!(read(&words), (96_499, 17, 34));
        assert_eq!(bit_field(&words, 0, 24, false).unwrap(), 33_000);
        assert_eq!(bit_field(&words, 28 * 32 + 24, 16, false).unwrap(), 63_499);
        assert_eq!(r.level(96_499), 14);
        // Back under the limit: the spillover emptied.
        r.set(&mut words, FigureField::Experience, 40).unwrap();
        assert_eq!(bit_field(&words, 28 * 32 + 24, 16, false).unwrap(), 0);
        r.set(&mut words, FigureField::Gold, 64_999).unwrap();
        assert_eq!(read(&words), (40, 64_999, 34));
        // More than the areas hold is refused.
        assert!(
            r.set(&mut words, FigureField::Experience, 33_000 + 65_536)
                .is_err()
        );
        // Without a spillover row the first area takes it all.
        let mut p = progression();
        p.save_params.retain(|s| s.kind != SaveKind::Spillover);
        let r1 = Rows::of(&p).unwrap();
        let mut w1 = vec![0u32; 44];
        r1.set(&mut w1, FigureField::Experience, 40_000).unwrap();
        assert_eq!(bit_field(&w1, 0, 24, false).unwrap(), 40_000);
    }

    #[test]
    fn the_whole_state_reads_field_for_field() {
        // 40,000 experience: 33,000 in the first area and 7,000 spilled over.
        let mut f = fight(40_000, 298, 0x0100_0004);
        let s = read(&mut f.w, f.hero, &rows()).unwrap();
        let h = &s.hero;
        assert_eq!(h.name.as_deref(), Some("009_Eruptor"));
        assert_eq!(h.behaviour.as_deref(), Some("default"));
        assert_eq!(
            (h.health, h.health_max, h.invulnerable),
            (Number::Int(290), Number::Int(290), Number::Int(0))
        );
        assert_eq!((h.player, h.exp_target), (0, Number::Int(1000)));
        assert_eq!(h.cells[0], f.hp);
        let fig = &h.figure;
        assert_eq!(fig.tag_data, f.tag);
        assert_eq!(fig.words.len(), 44);
        assert_eq!(fig.cells, f.words);
        assert_eq!(
            (fig.experience, fig.gold, fig.playtime_secs, fig.level),
            (40_000, 298, 0x0100_0004, 10)
        );
        assert_eq!(
            s.difficulty,
            ConsoleValue {
                value: 1,
                bit: 2549,
                bits: 4,
                signed: false,
                header: false,
                words: 178
            }
        );
        let m = &s.xp_multiplier;
        assert_eq!(m.value, Number::Float(1.1));
        assert_eq!(
            m.accolades,
            [
                ("ambassador - 1 toy of every element".to_owned(), false),
                ("captain - non-starter pack toy".to_owned(), true),
            ]
        );
        assert_eq!(m.table.len(), 3);
        assert_eq!((m.completed(), m.by_rule()), (1, Some(f64::from(1.1f32))));
        assert_eq!(
            (s.area.p1, s.area.p2, s.area.current, s.area_at_hero),
            (
                Number::Int(0),
                Number::Int(0),
                Number::Int(-1),
                Number::Int(0)
            )
        );
        let stats: Vec<(Option<&str>, Number, Number)> = s
            .enemy_stats
            .iter()
            .map(|e| (e.kind.as_deref(), e.hp, e.xp))
            .collect();
        assert_eq!(
            stats,
            [
                (
                    Some("PandoranGrunt"),
                    Number::Int(59),
                    Number::Float(128.333_34)
                ),
                (
                    Some("PandoranGruntSimple"),
                    Number::Int(33),
                    Number::Float(128.333_34)
                ),
                (Some("Enemy_Chompy"), Number::Int(1), Number::Int(4)),
            ]
        );
        let loot: Vec<(&str, Number, Number)> = s
            .loot
            .iter()
            .map(|a| (a.name.as_str(), a.index, a.rate))
            .collect();
        assert_eq!(
            loot,
            [
                (
                    "DestructiblesCommonRef",
                    Number::Float(0.824_279_5),
                    Number::Float(0.6)
                ),
                (
                    "DestructiblesUncommonRef",
                    Number::Float(0.097_497),
                    Number::Float(1.0)
                ),
                (
                    "DestructiblesRareRef",
                    Number::Float(0.570_767_6),
                    Number::Float(1.0)
                ),
                (
                    "PlacedLootCommonRef",
                    Number::Float(0.0),
                    Number::Float(0.0)
                ),
                (
                    "XpLoot PandoranGrunt",
                    Number::Float(0.25),
                    Number::Float(0.6)
                ),
            ]
        );
        assert_eq!(s.loot[0].index_cell, f.common_index);
        assert_eq!(s.float_type, Some(f.t.float));
        // Player 2's flag is the hero's when he's player 2.
        let p = f.w.get32(f.joined + 0x18);
        let data = f.w.get32(p + 0x14);
        let player = f.w.get32(data + 8);
        f.w.put32(player + 0x20, 1);
        f.w.put32(f.flags[1] + 0x20, 1);
        let s = read(&mut f.w, f.hero, &rows()).unwrap();
        assert_eq!((s.hero.player, s.area_at_hero), (1, Number::Int(1)));
        f.w.put32(player + 0x20, 2);
        assert!(err(read(&mut f.w, f.hero, &rows())).contains("player 2"));
    }

    #[test]
    fn the_difficulty_reads_its_row_from_the_set_the_row_names() {
        // A row at bit 4, 3 bits signed: the save's words hold 2 there, the
        // header's -1.
        let build = |header: i32| {
            let mut w = crate::actors::fixture::World::new();
            let t = fx::types(&mut w);
            // Field 2 of the row sits 2 x 3 bits on, at bit 10: 3 there.
            let save = fx::int(&mut w, &t, "", (2 << 4) | (3 << 10));
            let header_word = fx::int(&mut w, &t, "", 0b111 << 4);
            let save_set = fx::set(&mut w, "SaveGame", &[save]);
            let header_set = fx::set(&mut w, "SaveGame_Header", &[header_word]);
            let refs = [
                fx::reference(&mut w, vtable::SET_REFERENCE, "save val set", save_set),
                fx::reference(
                    &mut w,
                    vtable::SET_REFERENCE,
                    "header save val set",
                    header_set,
                ),
            ];
            let opt = [
                fx::int(&mut w, &t, "num bits", 3),
                fx::int(&mut w, &t, "bit index", 4),
                fx::int(&mut w, &t, "signed", 1),
                fx::int(&mut w, &t, "header param", header),
            ];
            let option = fx::structure(&mut w, "option - difficulty", &opt);
            let console = fx::structure(&mut w, CONSOLE_SAVE, &[option, refs[0], refs[1]]);
            (w, console)
        };
        let (mut w, console) = build(0);
        let d = difficulty(&mut w, &mut Types::default(), console).unwrap();
        assert_eq!((d.value, d.header, d.signed, d.words), (2, false, true, 1));
        let cells = fields(&mut w, console, CONSOLE_SAVE).unwrap();
        let third = console_value(
            &mut w,
            &mut Types::default(),
            &cells,
            "option - difficulty",
            2,
        )
        .unwrap();
        assert_eq!((third.bit, third.value), (10, 3));
        let (mut w, console) = build(1);
        let d = difficulty(&mut w, &mut Types::default(), console).unwrap();
        assert_eq!((d.value, d.header), (-1, true));
    }

    #[test]
    fn the_multiplier_counts_the_accolades_the_save_marks_done() {
        let read_m = |f: &mut super::fixture::Fixture| {
            read(&mut f.w, f.hero, &rows()).map(|s| s.xp_multiplier)
        };
        // None done: the rule gives 1; both: the table's second entry.
        let mut f = fight(0, 0, 4);
        f.w.put32(f.save_words[0] + 0x20, 0);
        let m = read_m(&mut f).unwrap();
        assert_eq!((m.completed(), m.by_rule()), (0, Some(1.0)));
        f.w.put32(f.save_words[0] + 0x20, 0b11 << 8);
        let m = read_m(&mut f).unwrap();
        assert_eq!((m.completed(), m.by_rule()), (2, Some(f64::from(1.2f32))));
        // More done than the table holds: no rule.
        let m = XpMultiplier {
            accolades: vec![("a".into(), true); 4],
            ..m
        };
        assert_eq!(m.by_rule(), None);
        // An accolade's bit past the save's words, the list not a set.
        let mut f = fight(0, 0, 4);
        let list = f.w.get32(f.listed + 0x18);
        let data = f.w.get32(list + 0x14);
        let captain = f.w.get32(data + 4);
        let cl = f.w.get32(captain + 0x18);
        let cd = f.w.get32(cl + 0x14);
        let bit = f.w.get32(cd);
        f.w.put32(bit + 0x20, 178 * 32);
        assert!(err(read_m(&mut f)).contains("collection - accolades"));
        let mut f = fight(0, 0, 4);
        f.w.put32(f.listed, vtable::SCRIPT_STRUCTURE);
        assert!(err(read_m(&mut f)).contains("xp multiplier accolades"));
    }

    #[test]
    fn the_level_cells_read_at_their_offsets() {
        let mut f = fight(0, 0, 4);
        let cells: Vec<(&str, u32)> = f
            .level_cells
            .iter()
            .map(|(n, o)| (n.as_str(), *o))
            .collect();
        let got = level_values(&mut f.w, &mut Types::default(), f.level_base, &cells).unwrap();
        let values: Vec<(&str, Number)> = got.iter().map(|v| (v.name.as_str(), v.value)).collect();
        assert_eq!(
            values,
            [
                ("Enemy_Chompy_DirectDMG", Number::Int(11)),
                ("PandoranGrunt_DMG", Number::Int(36)),
                ("XP_Value_Near", Number::Float(128.333_34)),
            ]
        );
        // A base 4 bytes off, a name that isn't the cell's, a cell that isn't
        // a number.
        let e = err(level_values(
            &mut f.w,
            &mut Types::default(),
            f.level_base + 4,
            &cells,
        ));
        assert!(e.contains("Enemy_Chompy_DirectDMG"), "{e}");
        let swapped = [(cells[1].0, cells[0].1)];
        let e = err(level_values(
            &mut f.w,
            &mut Types::default(),
            f.level_base,
            &swapped,
        ));
        assert!(
            e.contains("is `Enemy_Chompy_DirectDMG`, not `PandoranGrunt_DMG`"),
            "{e}"
        );
        let at = f.level_base + cells[0].1;
        f.w.put32(at, vtable::SCRIPT_REFERENCE);
        let e = err(level_values(
            &mut f.w,
            &mut Types::default(),
            f.level_base,
            &cells,
        ));
        assert!(e.contains("ValueInfo"), "{e}");
    }

    #[test]
    fn each_break_of_the_states_chains_is_refused_naming_it() {
        let read_it = |f: &mut super::fixture::Fixture| read(&mut f.w, f.hero, &rows());
        // An export missing.
        let mut f = fight(0, 0, 4);
        let name = f.w.get32(f.console + 0x08);
        f.w.put32(name, u32::from_be_bytes(*b"xxxx"));
        assert!(err(read_it(&mut f)).contains("no export named `console save params`"));
        // An export of the wrong class.
        let mut f = fight(0, 0, 4);
        f.w.put32(f.combat_ref, vtable::PLACEMENT_REFERENCE);
        assert!(err(read_it(&mut f)).contains("`combat control data`"));
        // The hero's script never started.
        let mut f = fight(0, 0, 4);
        f.w.put32(f.hero + 0xB0, 0);
        assert!(err(read_it(&mut f)).contains("never started"));
        // His joined data's `exp target` null.
        let mut f = fight(0, 0, 4);
        let cells = f.w.get32(f.joined + 0x18);
        let data = f.w.get32(cells + 0x14);
        let target = f.w.get32(data + 4);
        f.w.put32(target + 0x1C, 0);
        assert!(err(read_it(&mut f)).contains("exp target is a null reference"));
        // His tag data without a save area.
        let mut f = fight(0, 0, 4);
        let cells = f.w.get32(f.tag + 0x18);
        let data = f.w.get32(cells + 0x14);
        let area = f.w.get32(data + 4);
        let name = f.w.get32(area + 0x08);
        f.w.put32(name, u32::from_be_bytes(*b"xxxx"));
        assert!(err(read_it(&mut f)).contains("no `save data area`"));
        // A save word holding a float.
        let mut f = fight(0, 0, 4);
        f.w.put32(f.words[3] + 0x1C, f.t.float);
        assert!(err(read_it(&mut f)).contains("word 3 of the save area"));
        // A save area too short for the rows.
        let mut f = fight(0, 0, 4);
        let cells = f.w.get32(f.tag + 0x18);
        let data = f.w.get32(cells + 0x14);
        let area = f.w.get32(data + 4);
        let list = f.w.get32(area + 0x18);
        f.w.put32(list + 0x08, 20);
        assert!(err(read_it(&mut f)).contains("word 28"));
        // The enemy stats list missing, a row's type reference not one.
        let mut f = fight(0, 0, 4);
        let name = f.w.get32(f.enemy_list + 0x08);
        f.w.put32(name, u32::from_be_bytes(*b"xxxx"));
        assert!(err(read_it(&mut f)).contains("no `EnemyStatsList`"));
        let mut f = fight(0, 0, 4);
        let list = f.w.get32(f.enemy_list + 0x18);
        let data = f.w.get32(list + 0x14);
        let row = f.w.get32(data + 8);
        let rl = f.w.get32(row + 0x18);
        let rd = f.w.get32(rl + 0x14);
        let kind = f.w.get32(rd + 8);
        f.w.put32(kind, vtable::VALUE_INFO);
        assert!(err(read_it(&mut f)).contains("not a reference"));
        // A loot accumulator without its index.
        let mut f = fight(0, 0, 4);
        let name = f.w.get32(f.common_index + 0x08);
        f.w.put32(name, u32::from_be_bytes(*b"xxxx"));
        assert!(err(read_it(&mut f)).contains("DropRateIndex"));
        // The XpLoot set not a set.
        let mut f = fight(0, 0, 4);
        f.w.put32(f.xp_loot, vtable::SCRIPT_STRUCTURE);
        assert!(err(read_it(&mut f)).contains("XpLoot"));
        // The console save's words not a set reference.
        let mut f = fight(0, 0, 4);
        let cells = f.w.get32(f.console + 0x18);
        let data = f.w.get32(cells + 0x14);
        let svs = f.w.get32(data + 8);
        f.w.put32(svs, vtable::SCRIPT_REFERENCE);
        assert!(err(read_it(&mut f)).contains("not a set reference's"));
        // The difficulty past the console save's words.
        let mut f = fight(0, 0, 4);
        let cells = f.w.get32(f.console + 0x18);
        let data = f.w.get32(cells + 0x14);
        let option = f.w.get32(data);
        let ol = f.w.get32(option + 0x18);
        let od = f.w.get32(ol + 0x14);
        let bit = f.w.get32(od + 12);
        f.w.put32(bit + 0x20, 178 * 32);
        assert!(err(read_it(&mut f)).contains("past the 178 words"));
        // The multiplier missing.
        let mut f = fight(0, 0, 4);
        let combat = f.w.get32(f.combat_ref + 0x1C);
        let cl = f.w.get32(combat + 0x18);
        let cd = f.w.get32(cl + 0x14);
        let m = f.w.get32(cd);
        let name = f.w.get32(m + 0x08);
        f.w.put32(name, u32::from_be_bytes(*b"xxxx"));
        assert!(err(read_it(&mut f)).contains("accolade xp multiplier"));
        // Macros_UI's script never started.
        let mut f = fight(0, 0, 4);
        f.w.put32(f.macros_ui + 0xB0, 0);
        assert!(err(read_it(&mut f)).contains("Macros_UI's script never started"));
    }

    #[test]
    fn triggers_read_their_state_and_whether_they_fired() {
        let mut w = crate::actors::fixture::World::new();
        let ranged = |w: &mut crate::actors::fixture::World,
                      name: &str,
                      state: u32,
                      active: f32,
                      range: f32,
                      script: bool| {
            let p = fx::placement(w, name, None, active, script.then_some(&[][..]));
            w.put32(p + 0x54, state);
            w.putf(p + 0xD0, range);
            w.putf(p + 0xD4, 1.0);
            p
        };
        let asleep = ranged(&mut w, "Setup_001_Trigger", 2, 0.0, 20.0, false);
        let awake = ranged(&mut w, "Transition 7-1", 1, 0.75, 55.0, false);
        let started = ranged(&mut w, "Activate_Norticus and Chompies", 3, 0.0, 32.0, true);
        // Woke once and gone back to sleep: its clock shows it.
        let slept = ranged(&mut w, "Pop_to_Camera_5", 2, 1.5, 10.0, false);
        // Taken off (state 3) with its clock back at 0 and no script: the
        // state alone says it fired.
        let off = ranged(&mut w, "Transition 1-2", 3, 0.0, 100.0, false);
        // No range: awake from the start, its controller's check decides.
        let volume = ranged(&mut w, "Setup_003_Trigger(1)", 1, 55.65, 0.0, false);
        let got = triggers(
            &mut w,
            &[
                ("Setup_001_Trigger", asleep),
                ("Transition 7-1", awake),
                ("Activate_Norticus and Chompies", started),
                ("Pop_to_Camera_5", slept),
                ("Transition 1-2", off),
                ("Setup_003_Trigger(1)", volume),
            ],
        )
        .unwrap();
        let summary: Vec<(&str, u32, bool, f32, Option<bool>)> = got
            .iter()
            .map(|t| {
                (
                    t.name.as_str(),
                    t.activity_state,
                    t.started,
                    t.range,
                    t.fired,
                )
            })
            .collect();
        assert_eq!(
            summary,
            [
                ("Setup_001_Trigger", 2, false, 20.0, Some(false)),
                ("Transition 7-1", 1, false, 55.0, Some(true)),
                ("Activate_Norticus and Chompies", 3, true, 32.0, Some(true)),
                ("Pop_to_Camera_5", 2, false, 10.0, Some(true)),
                ("Transition 1-2", 3, false, 100.0, Some(true)),
                ("Setup_003_Trigger(1)", 1, false, 0.0, None),
            ]
        );
        assert_eq!(got[1].active_seconds, 0.75);
        let i = Instance::read(&mut w, asleep).unwrap();
        assert_eq!((i.activation_range, i.deactivation_buffer), (20.0, 1.0));
        // An address whose placement carries another name.
        assert!(
            err(triggers(&mut w, &[("Setup_001_Trigger", awake)]))
                .contains("not `Setup_001_Trigger`")
        );
    }

    /// What O5 copies at a pad read: each range's bytes back to back.
    fn sample(w: &mut crate::actors::fixture::World, list: &WatchList) -> WatchRecord {
        let ranges: Vec<(u32, usize)> = list
            .ranges()
            .iter()
            .map(|r| (r.addr, r.len as usize))
            .collect();
        let payload: Vec<u8> = w
            .gather(&ranges)
            .unwrap()
            .into_iter()
            .flat_map(|b| b.unwrap())
            .collect();
        WatchRecord {
            flip: 30,
            host_us: 0,
            missing: 0,
            read: 0,
            payload,
        }
    }

    #[test]
    fn the_watch_carries_the_cells_that_move_frame_by_frame() {
        let mut f = fight(999, 64_999, 120);
        let r = rows();
        let s = read(&mut f.w, f.hero, &r).unwrap();
        let watch = FightWatch::of(&s, &r).unwrap();
        let labels: Vec<&str> = watch.cells.iter().map(|(l, _)| l.as_str()).collect();
        assert_eq!(
            labels,
            [
                "hp",
                "hp max",
                "invulnerable",
                "area.p1",
                "area.p2",
                "loot",
                "loot.uncommon",
                "loot.rare",
                "word.0",
                "word.1",
                "word.2",
                "word.28",
                "word.29"
            ]
        );
        let fields = watch.list_fields();
        assert!(fields.iter().all(|f| f.len <= MAX_SPAN));
        let list = WatchList::new(fields).unwrap();
        let got = watch.read(&list, &sample(&mut f.w, &list)).unwrap();
        assert_eq!(
            got,
            FightFrame {
                health: Number::Int(290),
                health_max: Number::Int(290),
                invulnerable: Number::Int(0),
                experience: 999,
                gold: 64_999,
                playtime_secs: 120,
                level: 1,
                area_p1: Number::Int(0),
                area_p2: Number::Int(0),
                loot: Number::Float(0.824_279_5),
                loot_uncommon: Some(Number::Float(0.097_497)),
                loot_rare: Some(Number::Float(0.570_767_6)),
                triggers: Vec::new(),
            }
        );
        // A bite, an orb into level 2, the hero erupting: the next record has
        // them. Experience's first word: 1,000; health 278, now a float.
        f.w.put32(f.words[0], 1000 | (64_999 & 0xFF) << 24);
        f.w.put32(f.words[0] + 0x20, 1000 | (64_999 & 0xFF) << 24);
        f.w.put32(f.hp + 0x1C, f.t.float);
        f.w.putf(f.hp + 0x20, 278.0);
        let inv = s.hero.cells[2];
        f.w.put32(inv + 0x20, 1);
        let got = watch.read(&list, &sample(&mut f.w, &list)).unwrap();
        assert_eq!(
            (got.health, got.invulnerable, got.experience, got.level),
            (Number::Float(278.0), Number::Int(1), 1000, 2)
        );
        // A block unreadable: no fight frame.
        let mut record = sample(&mut f.w, &list);
        record.missing = 1 << list.placed("fight.0").unwrap().range;
        assert!(watch.read(&list, &record).is_none());
        // A state without a float type or a common count can't be watched.
        let mut bare = s.clone();
        bare.float_type = None;
        assert!(err(FightWatch::of(&bare, &r)).contains("FloatMeasurement"));
        let mut bare = s.clone();
        bare.loot.remove(0);
        assert!(err(FightWatch::of(&bare, &r)).contains("common loot"));
    }

    #[test]
    fn the_watch_carries_named_triggers_wake_state() {
        let mut f = fight(0, 0, 4);
        let r = rows();
        let s = read(&mut f.w, f.hero, &r).unwrap();
        let trigger = fx::placement(&mut f.w, "Setup_001_Trigger", None, 0.0, None);
        f.w.put32(trigger + 0x54, 2);
        f.w.putf(trigger + 0xD0, 20.0);
        let named = triggers(&mut f.w, &[("Setup_001_Trigger", trigger)]).unwrap();
        let watch = FightWatch::of(&s, &r).unwrap().with_triggers(&named);
        let fields = watch.list_fields();
        let t = fields
            .iter()
            .find(|f| f.name == "trigger.Setup_001_Trigger")
            .unwrap();
        assert_eq!((t.addr, t.len), (trigger + 0x54, 0x64));
        let list = WatchList::new(fields).unwrap();
        let at = |w: &mut crate::actors::fixture::World| {
            watch.read(&list, &sample(w, &list)).unwrap().triggers
        };
        assert_eq!(
            at(&mut f.w),
            [TriggerAt {
                name: "Setup_001_Trigger".into(),
                activity_state: 2,
                active_seconds: 0.0,
                behaviour: 0,
            }]
        );
        // The hero comes within its range: awake, its clock running, its
        // script on a behaviour.
        f.w.put32(trigger + 0x54, 1);
        f.w.putf(trigger + 0xB4, 0.05);
        f.w.put32(trigger + 0xAC, 0x3012_3450);
        let now = at(&mut f.w);
        assert_eq!(
            (
                now[0].activity_state,
                now[0].active_seconds,
                now[0].behaviour
            ),
            (1, 0.05, 0x3012_3450)
        );
        // Its range unreadable: no frame.
        let mut record = sample(&mut f.w, &list);
        record.missing = 1 << list.placed("trigger.Setup_001_Trigger").unwrap().range;
        assert!(watch.read(&list, &record).is_none());
    }

    #[test]
    fn cells_far_apart_go_in_blocks_of_their_own() {
        let watch = FightWatch {
            cells: vec![
                ("a".into(), 0x3000_0000),
                ("b".into(), 0x3000_0028),
                // A block runs from the first cell's type word to the last's
                // value's end: c ends it at exactly MAX_SPAN, d would pass it.
                ("c".into(), 0x3000_0000 + MAX_SPAN - 8),
                ("d".into(), 0x3000_0000 + MAX_SPAN - 4),
            ],
            float_type: 1,
            rows: rows(),
            triggers: Vec::new(),
        };
        let blocks = watch.blocks();
        assert_eq!(blocks.len(), 2);
        assert_eq!(blocks[0].2, [0, 1, 2]);
        assert_eq!(
            (blocks[0].0, blocks[0].1),
            (0x3000_001C, 0x3000_001C + MAX_SPAN)
        );
        assert_eq!(blocks[1].2, [3]);
    }
}
