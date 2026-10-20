//! Giants' O6 chains (openspec AH.5, design decision 16): what the fight's
//! recordings follow besides O5's fixed list, and how a record's blocks
//! become `guac_oracle`'s [`ChainRecord`]s keyed by actor.
//!
//! O6 knows nothing of the game: it follows a chain the harness names. Giants'
//! three, Giants 01.00 (`BLUS30968`), offsets from each object's start:
//! - [`ACTORS`]: the game's actor list, AF.9's layout ([`crate::actors`]): the
//!   static at [`actors::APP`] to the application, its insight core (+0x2C),
//!   the updater list (+0x1C, its array at +0x14), the `tfbActorUpdateGame`
//!   among them (its index found at the resolve) and its `_gameActorList`
//!   (+0x08); each hop's object checked against its vtable. An item is a
//!   corpus, kept when the FNV-1a of its placement's name (`tfbActorInfo`
//!   +0x50, its `_name` +0x08) is one the harness names. Giants' actors carry
//!   no name checksum of their own: a placement's `_name` points into an
//!   interned pool and a clone's is its template's, so the hash names a
//!   placed actor and every clone of a template alike, and a clone tells
//!   itself apart by its `_parent` (+0x5C, its template; 0 for a placed
//!   actor). Each kept corpus's transform (the placement's `_currentMatrix`),
//!   velocity (+0x58), placement, parent, behaviour cell (`_execBehavior`
//!   +0xAC), `_activityState` (+0x54) and `_activeTime` (+0xB4), and through
//!   the placement's `_localVarList` (+0xB0) its script variables by name:
//!   health (`description.creature info.hp`) and the alerted flag
//!   (`description.attention parameters.alerted`), each structure behind a
//!   reference's +0x1C and its fields in an `igObjectList` at +0x18
//!   ([`crate::script`]), each name found by O6's search.
//! - [`ORBS`]: experience orbs aren't actors. `Xp Balls - Create Exp Balls`
//!   keeps each in a structure in `combat control data.xp ball control.all
//!   active balls` (a `ScriptSet`, its list at +0x18), the orb itself a
//!   particle placement it spawns (`DeathXP_NewOrbOfficial_*`) under the
//!   ball's `xp ball.orb`; the ball's `xp ball.xp val` is its worth and
//!   `xp ball.my ID` a number the macro counts up from `Next Ball ID` for
//!   every ball, the serial that tells a reused slot apart. `combat control
//!   data` is a `global.bld` export, found in Alchemy's handle table at the
//!   resolve.
//! - [`LOOT`]: coins, spawned by `Loot - Get Loot` as clones of a currency
//!   art and kept by `Loot - Init Loot` in `Macros_UI`'s `LootInPlay` set
//!   (the placed treasure among them), each a structure whose `lootActor`
//!   is the coin's placement and `monetaryValue` its worth.
//!
//! The breakables in reach, the Chompies, Norticus, the hero and Eruptor's
//! projectiles ride [`ACTORS`] by name ([`fight_kinds`]); the hero's health,
//! experience and gold come from O5's fight cells ([`crate::fight`]), so his
//! item carries them every record. [`evaluate`] is O6's walk over guest
//! memory read at a pause, the reference a recording's last block is checked
//! against.

use std::collections::{BTreeMap, BTreeSet};

use anyhow::{Context, Result, bail};
use guac_oracle::fight::oracle::{ChainItem, ChainRecord, Names};
use guac_oracle::fight::ticks::HERO;
use guac_oracle::recording::Body;
use serde::{Deserialize, Serialize};

use crate::actors::{self, Gather, be32, c_string, f32_at, floats, one, vtable};
use crate::script;
use crate::verbs::{
    CHAIN_MAX_LIST, CHAIN_MAX_NAME, CHAIN_MAX_SCAN, ChainBlock, ChainEntry, ChainSpec, Hop, Key,
    Probe, WatchRecord, name_hash,
};
use crate::watch::{Chains, GiantsWatch, WatchList};

/// One record's chain blocks, by slot.
pub type Blocks = BTreeMap<u8, ChainBlock>;

/// Chain slots, in the order a record carries their blocks.
pub const ACTORS: u8 = 0;
pub const ORBS: u8 = 1;
pub const LOOT: u8 = 2;

/// Items each chain keeps a record. AF.9's list held 162-182 actors in the
/// clearing; the fight keeps the hero, 4 Chompies, Norticus' 2 placements,
/// the 13 breakables in reach and Eruptor's projectiles (a lob every 0.25 s
/// living 2 s is 8 at once), well under 48. A Chompy's 4 experience splits
/// into 4 or 5 orbs (AH.4's 1.1), 16-20 for the fight against 64; the
/// clearing's loot is 3 placed treasures and a coin or two a break.
pub const ACTOR_ITEMS: u32 = 48;
pub const ORB_ITEMS: u32 = 64;
pub const LOOT_ITEMS: u32 = 32;

/// Setup_001's four Chompies (AH.4).
pub const CHOMPIES: [&str; 4] = [
    "Enemy_Chompy(1)",
    "Enemy_Chompy(2)",
    "Enemy_Chompy(3)",
    "Enemy_Chompy(4)",
];
/// Norticus' two placements in the first encounter (AH.4).
pub const NORTICUS: [&str; 2] = [
    "1.NPC_Norticus_First_Encounter",
    "Norticus_First_Encounter(1)",
];
/// The breakables touching the clearing's reach (AG.7's "each solid's
/// absence", `ag7/out/reach2-final.txt`) and the ledge route's three
/// (`barrel_b_intact(1)` and `(4)`, `pottery_1(8)`; design decision 1).
pub const BREAKABLES: [&str; 13] = [
    "barrel_b_intact(1)",
    "barrel_b_intact(4)",
    "barrel_b_intact(9)",
    "pottery_1(7)",
    "pottery_1(8)",
    "pottery_1(11)",
    "pottery_2(4)",
    "pottery_2(5)",
    "pottery_3(3)",
    "gourd_01(1)",
    "gourd_01(2)",
    "gourd_01(10)",
    "Breakable_Stone_Wall_Castle_206(1)",
];
/// The templates in Eruptor's archive whose clones his attacks make (the
/// actors among `009_eruptor.bld`'s placements: the lava ball, its upgrade,
/// the fire blob and its splash, the eruption and its volcano). AH.9 says
/// which ones a throw and an eruption spawn.
pub const ERUPTOR_SPAWNS: [&str; 6] = [
    "009_LavaProjectile",
    "009_LavaProjectileUpgrade",
    "009_FireBlob",
    "009_FireBlobSplash",
    "009_LavaAttack",
    "009_MiniVolcano",
];

/// The kinds [`ACTORS`] keeps for the fight, by placement name, beside the
/// hero.
pub fn fight_kinds() -> Vec<(String, Vec<String>)> {
    let owned = |names: &[&str]| names.iter().map(|n| (*n).to_owned()).collect();
    vec![
        ("chompy".into(), owned(&CHOMPIES)),
        ("norticus".into(), owned(&NORTICUS)),
        ("breakable".into(), owned(&BREAKABLES)),
        ("projectile".into(), owned(&ERUPTOR_SPAWNS)),
    ]
}

// Offsets, from the registry (AC.4) and AF.9's and AH.4's layouts.
const CORPUS_PLACEMENT: u16 = 0x50;
const CORPUS_VELOCITY: u16 = 0x58;
const PLACEMENT_NAME: u16 = 0x08;
const PLACEMENT_MATRIX: u16 = 0x3C;
const PLACEMENT_STATE: u16 = 0x54;
const PLACEMENT_PARENT: u16 = 0x5C;
const PLACEMENT_BEHAVIOUR: u16 = 0xAC;
const PLACEMENT_LOCALS: u16 = 0xB0;
const PLACEMENT_ACTIVE: u16 = 0xB4;
const ACTOR_GAME_ACTOR: u16 = 0xF4;
const LIST_COUNT: u16 = 0x08;
const LIST_ARRAY: u16 = 0x14;
const OBJECT_NAME: u16 = 0x08;
const REFERENCE_OBJECT: u16 = 0x1C;
const CONTAINER_LIST: u16 = 0x18;
/// A number cell's type word and value.
const CELL_NUMBER: u16 = 0x1C;
const INSIGHT: u16 = 0x2C;
const UPDATERS: u16 = 0x1C;
const ACTOR_UPDATE_LIST: u16 = 0x08;

/// [`ACTORS`]' ranges, in order.
pub mod actor_range {
    pub const MATRIX: usize = 0;
    pub const VELOCITY: usize = 1;
    pub const PLACEMENT: usize = 2;
    pub const PARENT: usize = 3;
    pub const BEHAVIOUR: usize = 4;
    pub const STATE: usize = 5;
    pub const ACTIVE: usize = 6;
    pub const HEALTH: usize = 7;
    pub const ALERTED: usize = 8;
}

/// [`ORBS`]' ranges: each field through a structure, then through a
/// reference (whichever reads is the shape the ball has).
pub mod orb_range {
    pub const WORTH: [usize; 2] = [0, 1];
    pub const ID: [usize; 2] = [2, 3];
    pub const MATRIX: [usize; 2] = [4, 5];
}

/// [`LOOT`]'s ranges: the loot actor's transform and velocity, the entry's
/// `monetaryValue`, then the actor's placement, its parent and its name.
pub mod loot_range {
    pub const MATRIX: usize = 0;
    pub const VELOCITY: usize = 1;
    pub const WORTH: usize = 2;
    pub const ACTOR: usize = 3;
    pub const PARENT: usize = 4;
    pub const NAME: usize = 5;
}

/// The element of the list here named `name`: an `igDataList` of named
/// objects (count +0x08, array +0x14, each name at +0x08).
fn named(name: &str) -> Hop {
    Hop::named(LIST_COUNT, LIST_ARRAY, OBJECT_NAME, name)
}

/// A structure's field `name`, from the structure: its field list, then the
/// search.
fn field(name: &str) -> [Hop; 2] {
    [
        Hop::checked(CONTAINER_LIST, script::vtable::OBJECT_LIST),
        named(name),
    ]
}

/// The structure a reference cell names.
fn through_reference() -> Hop {
    Hop::checked(REFERENCE_OBJECT, script::vtable::SCRIPT_STRUCTURE)
}

/// A placement's script variable `path` (each step a structure behind a
/// reference, the last a number), from the corpus.
fn variable(path: &[&str]) -> Vec<Hop> {
    let mut hops = vec![
        Hop::at(CORPUS_PLACEMENT),
        Hop::checked(PLACEMENT_LOCALS, script::vtable::OBJECT_LIST),
    ];
    for (i, name) in path.iter().enumerate() {
        if i > 0 {
            hops.push(through_reference());
            hops.extend(field(name));
        } else {
            hops.push(named(name));
        }
    }
    hops
}

/// A ball's or a loot entry's field `inner` of its structure field `outer`,
/// `outer` held as a structure (`by_reference` false) or behind a reference.
fn nested(outer: &str, by_reference: bool, inner: &str) -> Vec<Hop> {
    let mut hops = field(outer).to_vec();
    if by_reference {
        hops.push(through_reference());
    }
    hops.extend(field(inner));
    hops
}

/// The actor list's chain: the hero and every kind by name.
fn actor_chain(updater_index: u32, values: Vec<u32>) -> Result<ChainSpec> {
    let at = u16::try_from(4 * updater_index).context("the updater index")?;
    Ok(ChainSpec {
        items: ACTOR_ITEMS,
        base: actors::APP,
        path: vec![
            Hop::checked(0, vtable::APPLICATION),
            Hop::checked(INSIGHT, vtable::INSIGHT_CORE),
            Hop::checked(UPDATERS, vtable::UPDATER_LIST),
            Hop::at(LIST_ARRAY),
            Hop::checked(at, vtable::ACTOR_UPDATE),
            Hop::checked(ACTOR_UPDATE_LIST, vtable::GAME_ACTOR_LIST),
        ],
        count: LIST_COUNT,
        array: LIST_ARRAY,
        key: Key {
            hops: vec![Hop::checked(CORPUS_PLACEMENT, vtable::ACTOR_INFO)],
            off: PLACEMENT_NAME,
            hashed: true,
        },
        values,
        ranges: vec![
            Probe::new(vec![Hop::at(CORPUS_PLACEMENT)], PLACEMENT_MATRIX, 24),
            Probe::new(vec![], CORPUS_VELOCITY, 12),
            Probe::new(vec![], CORPUS_PLACEMENT, 4),
            Probe::new(vec![Hop::at(CORPUS_PLACEMENT)], PLACEMENT_PARENT, 4),
            Probe::new(vec![Hop::at(CORPUS_PLACEMENT)], PLACEMENT_BEHAVIOUR, 4),
            Probe::new(vec![Hop::at(CORPUS_PLACEMENT)], PLACEMENT_STATE, 4),
            Probe::new(vec![Hop::at(CORPUS_PLACEMENT)], PLACEMENT_ACTIVE, 4),
            Probe::new(
                variable(&["description", "creature info", "hp"]),
                CELL_NUMBER,
                8,
            ),
            Probe::new(
                variable(&["description", "attention parameters", "alerted"]),
                CELL_NUMBER,
                8,
            ),
        ],
    })
}

/// A set's elements are structures (or references to them).
fn set_elements() -> (Key, Vec<u32>) {
    (
        Key {
            hops: Vec::new(),
            off: 0,
            hashed: false,
        },
        vec![
            script::vtable::SCRIPT_STRUCTURE,
            script::vtable::SCRIPT_REFERENCE,
        ],
    )
}

/// `combat control data.xp ball control.all active balls`.
fn orb_chain(combat_control: u32) -> ChainSpec {
    let (key, values) = set_elements();
    let mut path = vec![through_reference()];
    path.extend(field("xp ball control"));
    path.extend(field("all active balls"));
    path.push(Hop::checked(CONTAINER_LIST, script::vtable::SET_LIST));
    let mut ranges = Vec::new();
    for inner in ["xp val", "my ID"] {
        for by_ref in [false, true] {
            ranges.push(Probe::new(nested("xp ball", by_ref, inner), CELL_NUMBER, 8));
        }
    }
    for by_ref in [false, true] {
        let mut hops = nested("xp ball", by_ref, "orb");
        hops.push(Hop::at(REFERENCE_OBJECT));
        ranges.push(Probe::new(hops, PLACEMENT_MATRIX, 24));
    }
    ChainSpec {
        items: ORB_ITEMS,
        base: combat_control,
        path,
        count: LIST_COUNT,
        array: LIST_ARRAY,
        key,
        values,
        ranges,
    }
}

/// `Macros_UI`'s `LootInPlay`.
fn loot_chain(macros_ui: u32) -> ChainSpec {
    let (key, values) = set_elements();
    let actor = || {
        let mut hops = field("lootActor").to_vec();
        hops.push(Hop::checked(REFERENCE_OBJECT, vtable::ACTOR_INFO));
        hops
    };
    let velocity = {
        let mut hops = actor();
        hops.push(Hop::checked(ACTOR_GAME_ACTOR, vtable::CORPUS));
        hops
    };
    ChainSpec {
        items: LOOT_ITEMS,
        base: macros_ui,
        path: vec![
            Hop::checked(PLACEMENT_LOCALS, script::vtable::OBJECT_LIST),
            named("LootInPlay"),
            Hop::checked(CONTAINER_LIST, script::vtable::SET_LIST),
        ],
        count: LIST_COUNT,
        array: LIST_ARRAY,
        key,
        values,
        ranges: vec![
            Probe::new(actor(), PLACEMENT_MATRIX, 24),
            Probe::new(velocity, CORPUS_VELOCITY, 12),
            Probe::new(field("monetaryValue").to_vec(), CELL_NUMBER, 8),
            Probe::new(field("lootActor").to_vec(), REFERENCE_OBJECT, 4),
            Probe::new(actor(), PLACEMENT_PARENT, 4),
            Probe::new(actor(), PLACEMENT_NAME, 4),
        ],
    }
}

/// Giants' chains as resolved at a pause, with what each [`ACTORS`] value
/// stands for.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GiantsChains {
    pub chains: Chains,
    /// Each [`ACTORS`] value's kind and the placement name it hashes, in
    /// value order; the hero's first.
    pub actors: Vec<(String, String)>,
    pub hero_corpus: u32,
    pub hero_placement: u32,
    pub updater_index: u32,
    pub combat_control: u32,
    pub macros_ui: u32,
}

impl GiantsChains {
    /// Giants' chains from what a pause holds: the actor list's updater, the
    /// hero (as `watch` resolved him) and `kinds` by placement name, the two
    /// exports. Refused: a name in two kinds (one hash can't be two), more
    /// values than O6 matches, an export missing, or a chain past O6's limits.
    pub fn resolve(
        g: &mut impl Gather,
        watch: &GiantsWatch,
        kinds: &[(String, Vec<String>)],
    ) -> Result<Self> {
        let app = be32(&one(g, actors::APP, 4, "the application pointer")?, 0);
        let a = one(g, app, actors::APP_LEN, "the application")?;
        actors::expect_vtable(&a, vtable::APPLICATION, app, "tfbPS3Application")?;
        let insight = be32(&a, actors::APP_INSIGHT);
        let core = one(g, insight, actors::INSIGHT_LEN, "the insight core")?;
        actors::expect_vtable(&core, vtable::INSIGHT_CORE, insight, "igInsightCore")?;
        let updaters = actors::list(
            g,
            be32(&core, UPDATERS as usize),
            vtable::UPDATER_LIST,
            "igInsightUpdaterList",
        )?;
        let heads = g.gather(&updaters.iter().map(|&u| (u, 4)).collect::<Vec<_>>())?;
        let found: Vec<usize> = heads
            .iter()
            .enumerate()
            .filter(|(_, h)| h.as_deref().map(|b| be32(b, 0)) == Some(vtable::ACTOR_UPDATE))
            .map(|(i, _)| i)
            .collect();
        let [updater_index] = found[..] else {
            bail!(
                "{} tfbActorUpdateGame among the {} updaters",
                found.len(),
                updaters.len()
            );
        };
        let hero_name = watch
            .hero_name
            .clone()
            .context("the hero's placement has no name")?;
        let mut actors = vec![(HERO.to_owned(), hero_name)];
        for (kind, names) in kinds {
            if kind == HERO {
                bail!("`{HERO}` is the hero's kind");
            }
            actors.extend(names.iter().map(|n| (kind.clone(), n.clone())));
        }
        let mut seen = BTreeMap::new();
        for (kind, name) in &actors {
            if let Some(other) = seen.insert(name.clone(), kind.clone()) {
                bail!("`{name}` is in two kinds, {other} and {kind}");
            }
        }
        let values = actors.iter().map(|(_, n)| name_hash(n)).collect();
        let exports = script::exports(g, script::GLOBAL, &["combat control data", "Macros_UI"])?;
        let combat_control = exports["combat control data"].at;
        let macros_ui = exports["Macros_UI"].at;
        let mut slots = BTreeMap::new();
        slots.insert(ACTORS, actor_chain(updater_index as u32, values)?);
        slots.insert(ORBS, orb_chain(combat_control));
        slots.insert(LOOT, loot_chain(macros_ui));
        for (slot, spec) in &slots {
            if let Err(r) = spec.check() {
                bail!("chain {slot} breaks O6's limits (ERR {})", r.wire());
            }
        }
        let chains = Chains { slots };
        if chains.bytes() > crate::verbs::CHAIN_BUDGET {
            bail!("the chains take {} bytes a record", chains.bytes());
        }
        Ok(Self {
            chains,
            actors,
            hero_corpus: watch.hero_corpus,
            hero_placement: watch.hero_placement,
            updater_index: updater_index as u32,
            combat_control,
            macros_ui,
        })
    }

    /// The kinds by [`ACTORS`] value.
    pub fn kind(&self, value: u32) -> Option<&str> {
        self.actors.get(value as usize).map(|(k, _)| k.as_str())
    }

    /// Everything a recording's blocks need named at the end pause: the
    /// number cells' type words, the behaviour cells and the loot's names.
    pub fn resolve_names(
        &self,
        g: &mut impl Gather,
        blocks: &[BTreeMap<u8, ChainBlock>],
    ) -> Result<Resolved> {
        let mut types = BTreeSet::new();
        let mut cells = BTreeSet::new();
        let mut names = BTreeSet::new();
        let word = |b: &[u8]| be32(b, 0);
        for record in blocks {
            if let Some(b) = record.get(&ACTORS) {
                for i in &b.items {
                    for r in [actor_range::HEALTH, actor_range::ALERTED] {
                        types.extend(i.range(r).map(word));
                    }
                    cells.extend(
                        i.range(actor_range::BEHAVIOUR)
                            .map(word)
                            .filter(|&c| c != 0),
                    );
                }
            }
            if let Some(b) = record.get(&ORBS) {
                for i in &b.items {
                    for r in orb_range::WORTH.into_iter().chain(orb_range::ID) {
                        types.extend(i.range(r).map(word));
                    }
                }
            }
            if let Some(b) = record.get(&LOOT) {
                for i in &b.items {
                    types.extend(i.range(loot_range::WORTH).map(word));
                    names.extend(i.range(loot_range::NAME).map(word).filter(|&n| n != 0));
                }
            }
        }
        let mut out = Resolved::default();
        for ty in types {
            let kind = if ty == 0 {
                Some(false)
            } else {
                let head = one(g, ty, 0x0C, "a number's type")?;
                let name = one(g, be32(&head, 0x08), 64, "a type's name")?;
                match c_string(&name).as_str() {
                    "FloatMeasurement" => Some(true),
                    "IntMeasurement" => Some(false),
                    _ => None,
                }
            };
            out.float_types.insert(ty, kind);
        }
        for cell in cells {
            let head = one(g, cell, 0x0C, "a behaviour cell")?;
            actors::expect_vtable(&head, script::vtable::USER_BEHAVIOR, cell, "OpUserBehavior")?;
            let name = one(g, be32(&head, 0x08), 64, "a behaviour's name")?;
            out.behaviours.insert(cell, c_string(&name));
        }
        for at in names {
            let name = one(g, at, 64, "a loot actor's name")?;
            out.loot_names.insert(at, c_string(&name));
        }
        Ok(out)
    }

    /// The behaviour names `fight_trace` takes, by kind, from what the blocks
    /// held.
    pub fn names(&self, blocks: &[BTreeMap<u8, ChainBlock>], resolved: &Resolved) -> Names {
        let mut out = Names::default();
        for record in blocks {
            let Some(b) = record.get(&ACTORS) else {
                continue;
            };
            for i in &b.items {
                let (Some(kind), Some(cell)) = (
                    self.kind(i.value),
                    i.range(actor_range::BEHAVIOUR).map(|r| be32(r, 0)),
                ) else {
                    continue;
                };
                if let Some(name) = resolved.behaviours.get(&cell) {
                    out.behaviours
                        .entry(kind.to_owned())
                        .or_default()
                        .insert(cell, name.clone());
                }
            }
        }
        out
    }

    /// Each record's chain record: its actors by kind (the hero joined by
    /// O5's fight cells: his health, experience and gold), its orbs and its
    /// loot, with the generator's words. `records` are the ring's, whose
    /// payloads carry `list`'s ranges then these chains' blocks; `game_frame`
    /// gives each one's `_curFrame`.
    pub fn records(
        &self,
        watch: &GiantsWatch,
        records: &[WatchRecord],
        resolved: &Resolved,
    ) -> Result<(Vec<ChainRecord>, Vec<Blocks>)> {
        let mut serials = Serials::default();
        let mut out = Vec::with_capacity(records.len());
        let mut all = Vec::with_capacity(records.len());
        for record in records {
            let (o5, blocks) = self.chains.split(&watch.list, record)?;
            let game_frame = game_frame(&watch.list, &o5)
                .with_context(|| format!("the game frame at flip {}", o5.flip))?;
            let mut items = Vec::new();
            for (&slot, block) in &blocks {
                if block.status != 0 {
                    bail!(
                        "chain {slot}'s list broke at link {} in the record of flip {} read {}",
                        block.status,
                        o5.flip,
                        o5.read
                    );
                }
                for entry in &block.items {
                    items.push(match slot {
                        ACTORS => self.actor(entry, resolved, &mut serials)?,
                        ORBS => orb(entry, resolved),
                        LOOT => loot(entry, resolved),
                        other => bail!("no chain in slot {other}"),
                    });
                }
            }
            let fight = watch.fight(&o5);
            let hero = items.iter_mut().find(|i| i.kind == HERO);
            let hero = match hero {
                Some(h) => h,
                None => {
                    items.insert(
                        0,
                        ChainItem {
                            address: self.hero_corpus,
                            kind: HERO.into(),
                            placement: None,
                            serial: None,
                            body: None,
                            behaviour: None,
                            vars: BTreeMap::new(),
                            missing: 0,
                        },
                    );
                    &mut items[0]
                }
            };
            if let Some(f) = fight {
                hero.vars.insert("health".into(), f.health.as_f64() as f32);
                hero.vars.insert("experience".into(), f.experience as f32);
                hero.vars.insert("gold".into(), f.gold as f32);
            }
            out.push(ChainRecord {
                flip: o5.flip,
                read: u32::try_from(o5.read).context("a record's read")?,
                game_frame,
                items,
                mt: watch.mt_words(&o5),
            });
            all.push(blocks);
        }
        Ok((out, all))
    }

    fn actor(
        &self,
        e: &ChainEntry,
        resolved: &Resolved,
        serials: &mut Serials,
    ) -> Result<ChainItem> {
        use actor_range as r;
        let (kind, name) = self
            .actors
            .get(e.value as usize)
            .with_context(|| format!("no actor value {}", e.value))?;
        let parent = e.range(r::PARENT).map(|b| be32(b, 0));
        let clone = parent.is_some_and(|p| p != 0) && kind != HERO;
        let placement = e.range(r::PLACEMENT).map(|b| be32(b, 0));
        let active = e.range(r::ACTIVE).map(|b| f32_at(b, 0));
        let serial = clone.then(|| serials.of(e.address, placement, active));
        let mut vars = BTreeMap::new();
        for (label, range) in [("health", r::HEALTH), ("alerted", r::ALERTED)] {
            if let Some(v) = e.range(range).and_then(|b| resolved.number(b)) {
                vars.insert(label.to_owned(), v);
            }
        }
        if let Some(b) = e.range(r::STATE) {
            vars.insert("placement.activity".to_owned(), be32(b, 0) as f32);
        }
        if let Some(t) = active {
            vars.insert("placement.active".to_owned(), t);
        }
        Ok(ChainItem {
            address: e.address,
            kind: kind.clone(),
            placement: (!clone).then(|| name.clone()),
            serial,
            body: body(
                e.range(r::MATRIX),
                e.range(r::VELOCITY).map(|b| floats(b, 0)),
            ),
            behaviour: e
                .range(r::BEHAVIOUR)
                .map(|b| be32(b, 0))
                .filter(|&c| c != 0),
            vars,
            missing: u64::from(e.missing),
        })
    }
}

/// Beside a chained recording's `watch.json` (whose records hold O5's
/// ranges alone): the chains and what the end pause named.
pub const CHAIN_FILE: &str = "chain.json";
/// Each record's chain bytes, back to back in the ring's order.
pub const CHAIN_BLOCKS: &str = "chain.bin";
/// The records decoded: `guac_oracle`'s [`ChainRecord`]s, one per O5 record.
pub const CHAIN_RECORDS: &str = "chain-records.json";

/// [`CHAIN_FILE`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ChainFile {
    pub identity: String,
    pub chains: GiantsChains,
    pub resolved: Resolved,
    /// Each record's flip and read, in [`CHAIN_BLOCKS`]' order.
    pub keys: Vec<(u64, u64)>,
    /// Each record's bytes in [`CHAIN_BLOCKS`].
    pub stride: usize,
}

/// What one chain held across a recording.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SlotRun {
    /// Records whose list didn't read (a link broke).
    pub broken: usize,
    pub max_listed: u32,
    pub max_unread: u32,
    pub max_matched: u32,
    pub max_kept: u32,
    /// Records where the filter matched more than the chain keeps.
    pub dropped: usize,
    /// Kept items whose transform or velocity didn't read (a coin's
    /// velocity aside, counted next).
    pub bodies_missing: usize,
    /// Loot whose actor had no corpus to read a velocity off.
    #[serde(default)]
    pub velocities_missing: usize,
}

/// A chained recording's summary, for a replay's record.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ChainRun {
    pub bytes: usize,
    pub records: usize,
    pub slots: BTreeMap<u8, SlotRun>,
    /// Actors told apart, by kind, over the whole recording.
    pub actors: BTreeMap<String, usize>,
    pub names_ms: f64,
}

impl GiantsChains {
    /// At the end pause: each record split into O5's own (returned, for
    /// `watch.json` and the recording) and its chain bytes, the names read
    /// through `g`, and [`CHAIN_FILE`], [`CHAIN_BLOCKS`] and
    /// [`CHAIN_RECORDS`] written into `dir`.
    pub fn write_outputs(
        &self,
        watch: &GiantsWatch,
        records: &[WatchRecord],
        g: &mut impl Gather,
        identity: &str,
        dir: &std::path::Path,
    ) -> Result<(Vec<WatchRecord>, ChainRun, Vec<ChainRecord>)> {
        let lb = watch.list.bytes();
        let stride = self.chains.bytes();
        let mut o5 = Vec::with_capacity(records.len());
        let mut raw = Vec::with_capacity(records.len() * stride);
        let mut blocks = Vec::with_capacity(records.len());
        for r in records {
            let (own, b) = self.chains.split(&watch.list, r)?;
            raw.extend_from_slice(&r.payload[lb..]);
            o5.push(own);
            blocks.push(b);
        }
        let t = std::time::Instant::now();
        let resolved = self.resolve_names(g, &blocks)?;
        let names_ms = t.elapsed().as_secs_f64() * 1e3;
        let (decoded, _) = self.records(watch, records, &resolved)?;
        let mut slots: BTreeMap<u8, SlotRun> = BTreeMap::new();
        for record in &blocks {
            for (&slot, b) in record {
                let s = slots.entry(slot).or_default();
                s.broken += usize::from(b.status != 0);
                s.max_listed = s.max_listed.max(b.listed);
                s.max_unread = s.max_unread.max(b.unread);
                s.max_matched = s.max_matched.max(b.matched);
                s.max_kept = s.max_kept.max(b.items.len() as u32);
                s.dropped += usize::from(b.dropped() > 0);
            }
        }
        let mut seen: BTreeMap<String, BTreeSet<(u32, Option<u32>)>> = BTreeMap::new();
        for r in &decoded {
            for i in &r.items {
                seen.entry(i.kind.clone())
                    .or_default()
                    .insert((i.address, i.serial));
            }
        }
        for record in &blocks {
            if let Some(b) = record.get(&LOOT) {
                slots.entry(LOOT).or_default().velocities_missing += b
                    .items
                    .iter()
                    .filter(|i| i.range(loot_range::VELOCITY).is_none())
                    .count();
            }
        }
        for d in &decoded {
            for i in &d.items {
                if i.kind != HERO && i.body.is_none() {
                    let slot = match i.kind.as_str() {
                        "orb" => ORBS,
                        "coin" => LOOT,
                        _ => ACTORS,
                    };
                    slots.entry(slot).or_default().bodies_missing += 1;
                }
            }
        }
        let run = ChainRun {
            bytes: stride,
            records: records.len(),
            slots,
            actors: seen.into_iter().map(|(k, v)| (k, v.len())).collect(),
            names_ms,
        };
        let dir = rrc_dump::output::refuse_repo_output(dir)?;
        std::fs::create_dir_all(&dir)?;
        let file = ChainFile {
            identity: identity.to_owned(),
            chains: self.clone(),
            resolved,
            keys: records.iter().map(|r| (r.flip, r.read)).collect(),
            stride,
        };
        std::fs::write(dir.join(CHAIN_FILE), serde_json::to_string_pretty(&file)?)?;
        std::fs::write(dir.join(CHAIN_BLOCKS), &raw)?;
        std::fs::write(dir.join(CHAIN_RECORDS), serde_json::to_string(&decoded)?)?;
        Ok((o5, run, decoded))
    }
}

/// What the end pause named for a recording's blocks.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Resolved {
    /// A number cell's type word: `FloatMeasurement` (true), `IntMeasurement`
    /// (false) or neither.
    pub float_types: BTreeMap<u32, Option<bool>>,
    /// A behaviour cell's `OpUserBehavior` name.
    pub behaviours: BTreeMap<u32, String>,
    /// A loot actor's name, by its name pointer.
    pub loot_names: BTreeMap<u32, String>,
}

impl Resolved {
    /// A cell's type word and value as a float, its type one of the two
    /// number types.
    pub fn number(&self, b: &[u8]) -> Option<f32> {
        match self.float_types.get(&be32(b, 0))? {
            Some(true) => Some(f32_at(b, 4)),
            Some(false) => Some(be32(b, 4) as i32 as f32),
            None => None,
        }
    }
}

/// A clone's serial: Giants' actors carry no id or spawn count (`_ID` is
/// the template's, the corpus's `_timeStamp` the frame's game time, a node's
/// `_handle` 20 for all), so a slot taken by another actor shows as its
/// placement pointer changing or its `_activeTime` going back, and the
/// serial counts those per list address.
#[derive(Debug, Default)]
struct Serials {
    by_address: BTreeMap<u32, (Option<u32>, Option<f32>, u32)>,
}

impl Serials {
    fn of(&mut self, address: u32, placement: Option<u32>, active: Option<f32>) -> u32 {
        let entry = self
            .by_address
            .entry(address)
            .or_insert((placement, active, 0));
        let went_back = matches!((entry.1, active), (Some(was), Some(now)) if now < was);
        if entry.0 != placement || went_back {
            entry.2 += 1;
        }
        entry.0 = placement;
        entry.1 = active;
        entry.2
    }
}

fn body(matrix: Option<&[u8]>, velocity: Option<[f32; 3]>) -> Option<Body> {
    let m = matrix?;
    Some(Body {
        position: floats(m, 0),
        rotation: floats(m, 12),
        velocity: velocity?,
    })
}

/// The first of `ranges` that read.
fn either(e: &ChainEntry, ranges: [usize; 2]) -> Option<&[u8]> {
    ranges.into_iter().find_map(|r| e.range(r))
}

fn orb(e: &ChainEntry, resolved: &Resolved) -> ChainItem {
    let mut vars = BTreeMap::new();
    if let Some(v) = either(e, orb_range::WORTH).and_then(|b| resolved.number(b)) {
        vars.insert("worth".to_owned(), v);
    }
    ChainItem {
        address: e.address,
        kind: "orb".into(),
        placement: None,
        serial: either(e, orb_range::ID).map(|b| be32(b, 4)),
        // A particle placement: its transform, and no velocity to read.
        body: body(either(e, orb_range::MATRIX), Some([0.0; 3])),
        behaviour: None,
        vars,
        missing: u64::from(e.missing),
    }
}

fn loot(e: &ChainEntry, resolved: &Resolved) -> ChainItem {
    use loot_range as r;
    let mut vars = BTreeMap::new();
    if let Some(v) = e.range(r::WORTH).and_then(|b| resolved.number(b)) {
        vars.insert("worth".to_owned(), v);
    }
    let parent = e.range(r::PARENT).map(|b| be32(b, 0));
    let name = e
        .range(r::NAME)
        .and_then(|b| resolved.loot_names.get(&be32(b, 0)))
        .cloned();
    ChainItem {
        address: e.address,
        kind: "coin".into(),
        placement: if parent == Some(0) { name } else { None },
        serial: e.range(r::ACTOR).map(|b| be32(b, 0)),
        // A placed treasure out of range has no corpus (and so no velocity:
        // its bit stays set in `missing`), and a coin's corpus goes in the
        // record before its entry leaves the set: its transform is its body.
        body: body(
            e.range(r::MATRIX),
            Some(e.range(r::VELOCITY).map_or([0.0; 3], |b| floats(b, 0))),
        ),
        behaviour: None,
        vars,
        missing: u64::from(e.missing),
    }
}

/// `_curFrame` in a record of Giants' list (its `app` field, +0x5C).
fn game_frame(list: &WatchList, record: &WatchRecord) -> Option<u32> {
    list.field(record, "app").map(|b| be32(b, 0x08))
}

// --- O6's walk, for checking a block against guest memory at a pause --------

fn word(g: &mut impl Gather, addr: u64) -> Result<Option<u32>> {
    if !addr.is_multiple_of(4) || addr + 4 > 1 << 32 {
        return Ok(None);
    }
    Ok(g.gather(&[(addr as u32, 4)])?
        .pop()
        .flatten()
        .map(|b| be32(&b, 0)))
}

/// FNV-1a over the NUL-terminated string at `at`, up to [`CHAIN_MAX_NAME`]
/// bytes, read a page at a time as O6 checks them.
fn name_hash_at(g: &mut impl Gather, at: u32) -> Result<Option<u32>> {
    let mut h: u32 = 0x811C_9DC5;
    let mut done = 0usize;
    while done < CHAIN_MAX_NAME {
        let a = u64::from(at) + done as u64;
        if a >= 1 << 32 {
            return Ok(None);
        }
        let page_end = (a | 0xFFF) + 1;
        let len = ((page_end - a) as usize).min(CHAIN_MAX_NAME - done);
        let Some(bytes) = g.gather(&[(a as u32, len)])?.pop().flatten() else {
            return Ok(None);
        };
        for &c in &bytes {
            if c == 0 {
                return Ok(Some(h));
            }
            h = (h ^ u32::from(c)).wrapping_mul(0x0100_0193);
        }
        done += len;
    }
    Ok(None)
}

/// `hops` from `cur`: 0 once each held, else the 1-based hop that didn't.
fn follow(g: &mut impl Gather, cur: &mut u32, hops: &[Hop]) -> Result<usize> {
    for (i, h) in hops.iter().enumerate() {
        match *h {
            Hop::Deref { off, check } => {
                let Some(next) = word(g, u64::from(*cur) + u64::from(off))? else {
                    return Ok(i + 1);
                };
                if next == 0 {
                    return Ok(i + 1);
                }
                if let Some(w) = check
                    && word(g, u64::from(next))? != Some(w)
                {
                    return Ok(i + 1);
                }
                *cur = next;
            }
            Hop::Search {
                count,
                array,
                name,
                hash,
            } => {
                let (Some(n), Some(arr)) = (
                    word(g, u64::from(*cur) + u64::from(count))?,
                    word(g, u64::from(*cur) + u64::from(array))?,
                ) else {
                    return Ok(i + 1);
                };
                if n > CHAIN_MAX_SCAN {
                    return Ok(i + 1);
                }
                let mut found = None;
                for k in 0..n {
                    let Some(e) = word(g, u64::from(arr) + 4 * u64::from(k))?.filter(|&e| e != 0)
                    else {
                        continue;
                    };
                    let Some(at) = word(g, u64::from(e) + u64::from(name))?.filter(|&p| p != 0)
                    else {
                        continue;
                    };
                    if name_hash_at(g, at)? == Some(hash) {
                        found = Some(e);
                        break;
                    }
                }
                match found {
                    Some(e) => *cur = e,
                    None => return Ok(i + 1),
                }
            }
        }
    }
    Ok(0)
}

/// O6's walk of `spec` over guest memory as `g` reads it now, the block it
/// would write: the reference a recording's blocks are checked against at a
/// pause (and the tests' own).
pub fn evaluate(spec: &ChainSpec, g: &mut impl Gather) -> Result<ChainBlock> {
    let mut list = spec.base;
    let hops = spec.path.len() as u32;
    let mut status = follow(g, &mut list, &spec.path)? as u32;
    let mut count = 0;
    let mut arr = 0;
    if status == 0 {
        match (
            word(g, u64::from(list) + u64::from(spec.count))?,
            word(g, u64::from(list) + u64::from(spec.array))?,
        ) {
            (None, _) => status = hops + 1,
            (_, None) => status = hops + 2,
            (Some(n), _) if n > CHAIN_MAX_LIST => status = hops + 3,
            (Some(n), Some(a)) => (count, arr) = (n, a),
        }
    }
    let mut block = ChainBlock {
        status,
        listed: count,
        unread: 0,
        matched: 0,
        items: Vec::new(),
    };
    for k in 0..count {
        let Some(e) = word(g, u64::from(arr) + 4 * u64::from(k))?.filter(|&e| e != 0) else {
            block.unread += 1;
            continue;
        };
        let mut cur = e;
        let key = if follow(g, &mut cur, &spec.key.hops)? != 0 {
            None
        } else {
            match word(g, u64::from(cur) + u64::from(spec.key.off))? {
                Some(w) if !spec.key.hashed => Some(w),
                Some(w) if w != 0 => name_hash_at(g, w)?,
                _ => None,
            }
        };
        let Some(key) = key else {
            block.unread += 1;
            continue;
        };
        let Some(v) = spec.values.iter().position(|&v| v == key) else {
            continue;
        };
        block.matched += 1;
        if block.items.len() as u32 == spec.items {
            continue;
        }
        let mut missing = 0u32;
        let mut ranges = Vec::with_capacity(spec.ranges.len());
        for (r, p) in spec.ranges.iter().enumerate() {
            let mut cur = e;
            let read = if follow(g, &mut cur, &p.hops)? == 0
                && u64::from(cur) + u64::from(p.off) + u64::from(p.len) <= 1 << 32
            {
                g.gather(&[(cur + u32::from(p.off), p.len as usize)])?
                    .pop()
                    .flatten()
            } else {
                None
            };
            match read {
                Some(bytes) => ranges.push(bytes),
                None => {
                    missing |= 1 << r;
                    ranges.push(vec![0; p.len as usize]);
                }
            }
        }
        block.items.push(ChainEntry {
            address: e,
            index: k,
            value: v as u32,
            missing,
            ranges,
        });
    }
    if block.status != 0 {
        block.listed = 0;
    }
    Ok(block)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::actors::fixture::{Spec, World};
    use crate::script::fixture as fx;
    use crate::verbs::{CHAIN_HEADER, CHAIN_ITEM_HEADER, ChainBlock, Hop};

    /// AF.8's fresh state as the chains go: Eruptor (whose description holds
    /// his health and the alerted flag behind references, as AF.8's state
    /// has them), two Chompies awake, a barrel, a clone of a leaf template, a
    /// rock nobody asks for, `combat control data` with two balls (one
    /// structure, one reference) and `Macros_UI` with a placed treasure and
    /// a coin's clone in `LootInPlay`.
    struct Fixture {
        w: World,
        hero: u32,
        chompy: u32,
        leaf: u32,
        watch: GiantsWatch,
        types: fx::Types,
        walk: u32,
    }

    /// A creature's description: `description` a reference to a structure
    /// whose `creature info` and `attention parameters` are references to
    /// structures holding `hp` and `alerted`.
    fn description(w: &mut World, t: &fx::Types, hp: i32, alerted: i32) -> u32 {
        let h = fx::int(w, t, "hp", hp);
        let max = fx::int(w, t, "hp max", hp);
        let info = fx::structure(w, "", &[max, h]);
        let a = fx::int(w, t, "alerted", alerted);
        let attention = fx::structure(w, "", &[a]);
        let info_ref = fx::reference(w, script::vtable::SCRIPT_REFERENCE, "creature info", info);
        let att_ref = fx::reference(
            w,
            script::vtable::SCRIPT_REFERENCE,
            "attention parameters",
            attention,
        );
        let d = fx::structure(w, "", &[att_ref, info_ref]);
        fx::reference(w, script::vtable::SCRIPT_REFERENCE, "description", d)
    }

    /// An actor whose placement runs `behaviour` and holds `vars`.
    fn scripted(w: &mut World, name: &str, x: f32, behaviour: &str, vars: &[u32]) -> (u32, u32) {
        let a = w.actor(&Spec {
            name: name.into(),
            position: [x, 22.4, -60.0],
            velocity: [x / 10.0, 0.0, 0.0],
            ..Spec::default()
        });
        let p = fx::placement(w, "unused", Some(behaviour), 2.5, Some(vars));
        // The actor's own placement takes the script instance's words.
        for off in [0x1Cu32, 0x54, 0xA8, 0xAC, 0xB0, 0xB4] {
            let v = w.get32(p + off);
            w.put32(a.placement + off, v);
        }
        (a.corpus, a.placement)
    }

    fn fixture() -> Fixture {
        let mut w = World::new();
        let t = fx::types(&mut w);
        let d = description(&mut w, &t, 290, 0);
        let (hero, hero_placement) = scripted(&mut w, "009_Eruptor", 10.0, "default", &[d]);
        let pad = w.object(vtable::PAD_CONTROLLER, 0x20C);
        w.put32(pad + 0x134, (-1i32) as u32);
        w.put32(hero_placement + 0xD8, pad);
        let d1 = description(&mut w, &t, 1, 1);
        let (chompy, _) = scripted(&mut w, "Enemy_Chompy(1)", 12.0, "Battle", &[d1]);
        let rock = w.actor(&Spec {
            name: "rock".into(),
            ..Spec::default()
        });
        let barrel = w.actor(&Spec {
            name: "barrel_b_intact(1)".into(),
            position: [25.7, 53.3, -74.6],
            ..Spec::default()
        });
        // A clone of a leaf template: its own placement, the template's
        // name, the template as its parent.
        let template = fx::placement(&mut w, "leaves Blowing in air ", None, 0.0, None);
        let leaf = w.actor(&Spec {
            name: "leaves Blowing in air ".into(),
            position: [1.0, 2.0, 3.0],
            ..Spec::default()
        });
        w.put32(leaf.placement + 0x5C, template);
        w.putf(leaf.placement + 0xB4, 4.0);
        let _ = (rock, barrel);
        // combat control data -> xp ball control -> all active balls.
        let ball = |w: &mut World, t: &fx::Types, worth: i32, id: i32, x: f32, by_ref: bool| {
            let orb = fx::placement(w, "DeathXP_NewOrbOfficial_EvenSmaller", None, 0.0, None);
            w.put_floats(orb + 0x3C, &[x, 23.0, -61.0, 0.0, 0.0, 0.0]);
            let v = fx::int(w, t, "xp val", worth);
            let i = fx::int(w, t, "my ID", id);
            let o = fx::reference(w, script::vtable::PLACEMENT_REFERENCE, "orb", orb);
            let xb = if by_ref {
                let inner = fx::structure(w, "", &[v, i, o]);
                fx::reference(w, script::vtable::SCRIPT_REFERENCE, "xp ball", inner)
            } else {
                fx::structure(w, "xp ball", &[v, i, o])
            };
            fx::structure(w, "", &[xb])
        };
        let b1 = ball(&mut w, &t, 1, 7, 13.0, false);
        let b2 = ball(&mut w, &t, 1, 8, 14.0, true);
        let balls = fx::set(&mut w, "all active balls", &[b1, b2]);
        let next = fx::int(&mut w, &t, "Next Ball ID", 9);
        let control = fx::structure(&mut w, "xp ball control", &[next, balls]);
        let accolade = fx::float(&mut w, &t, "accolade xp multiplier", 1.1);
        let data = fx::structure(&mut w, "", &[accolade, control]);
        let combat = fx::reference(
            &mut w,
            script::vtable::SCRIPT_REFERENCE,
            "combat control data",
            data,
        );
        // Macros_UI -> LootInPlay: a placed treasure and a coin's clone.
        let treasure = w.actor(&Spec {
            name: "Copper_Coins".into(),
            position: [30.0, 60.0, -70.0],
            ..Spec::default()
        });
        let coin_template = fx::placement(&mut w, "1_Coin", None, 0.0, None);
        let coin = w.actor(&Spec {
            name: "1_Coin".into(),
            position: [26.0, 53.0, -74.0],
            velocity: [0.0, 5.0, 0.0],
            ..Spec::default()
        });
        w.put32(coin.placement + 0x5C, coin_template);
        let entry = |w: &mut World, t: &fx::Types, at: u32, worth: i32| {
            let a = fx::reference(w, script::vtable::PLACEMENT_REFERENCE, "lootActor", at);
            let m = fx::int(w, t, "monetaryValue", worth);
            fx::structure(w, "", &[m, a])
        };
        let e1 = entry(&mut w, &t, treasure.placement, 5);
        let e2 = entry(&mut w, &t, coin.placement, 2);
        // A spinning coin out of range: a placement with no corpus.
        let far = fx::placement(&mut w, "Coin_spinning", None, 0.0, None);
        w.put_floats(far + 0x3C, &[40.0, 49.0, -66.0, 0.0, 30.0, 0.0]);
        let e3 = entry(&mut w, &t, far, 1);
        let lip = fx::set(&mut w, "LootInPlay", &[e1, e2, e3]);
        let ui = fx::placement(&mut w, "Macros_UI", None, 1.0, Some(&[lip]));
        fx::handles(&mut w, script::GLOBAL, &[(0x1111, combat), (0x2222, ui)]);
        let watch = GiantsWatch {
            hero_corpus: hero,
            hero_placement,
            hero_name: Some("009_Eruptor".into()),
            ..watch_stub()
        };
        let walk = w.get32(chompy + 0x50);
        Fixture {
            w,
            hero,
            chompy,
            leaf: leaf.corpus,
            watch,
            types: t,
            walk,
        }
    }

    /// The watch fields the chains read: the hero and a list with the
    /// application's frame field.
    fn watch_stub() -> GiantsWatch {
        GiantsWatch {
            list: WatchList::new(vec![crate::watch::Field::new(
                "app",
                actors::APP + 0x54,
                0x14,
            )])
            .unwrap(),
            port: 0,
            application: 0,
            logic_scale: 0,
            hero_corpus: 0,
            hero_placement: 0,
            hero_name: None,
            controller: 0,
            pad0: None,
            parameters: 0,
            viewport_system: None,
            viewport: None,
            animation: 0,
            slots: 0,
            clips: Vec::new(),
            proxy: None,
            followers: Vec::new(),
            mover: None,
            fight: None,
            mt: None,
        }
    }

    fn kinds() -> Vec<(String, Vec<String>)> {
        let mut k = fight_kinds();
        k.push(("leaf".into(), vec!["leaves Blowing in air ".into()]));
        k
    }

    #[test]
    fn giants_chains_resolve_and_walk_the_fixture_as_o6_would() {
        let mut f = fixture();
        let g = GiantsChains::resolve(&mut f.w, &f.watch, &kinds()).unwrap();
        assert_eq!(g.updater_index, 1);
        assert_eq!(g.actors[0], (HERO.to_owned(), "009_Eruptor".to_owned()));
        assert_eq!(g.kind(1), Some("chompy"));
        assert_eq!(g.actors.len(), 1 + 4 + 2 + 13 + 6 + 1);
        assert!(g.chains.bytes() <= crate::verbs::CHAIN_BUDGET);
        let actors = evaluate(&g.chains.slots[&ACTORS], &mut f.w).unwrap();
        // The hero, the Chompy, the barrel and the leaf; the rock and the two
        // loot actors aren't asked for.
        assert_eq!((actors.status, actors.unread, actors.matched), (0, 0, 4));
        let kept: Vec<&str> = actors
            .items
            .iter()
            .map(|i| g.kind(i.value).unwrap())
            .collect();
        assert_eq!(kept, [HERO, "chompy", "breakable", "leaf"]);
        let hero = &actors.items[0];
        assert_eq!(hero.address, f.hero);
        assert_eq!(hero.missing, 0, "every range of the hero reads");
        let chompy = &actors.items[1];
        assert_eq!(chompy.address, f.chompy);
        assert_eq!(be32(chompy.range(actor_range::HEALTH).unwrap(), 4), 1);
        assert_eq!(be32(chompy.range(actor_range::ALERTED).unwrap(), 4), 1);
        // The barrel has no script: its variables don't read, the rest does.
        let barrel = &actors.items[2];
        assert_eq!(
            barrel.missing,
            (1 << actor_range::HEALTH) | (1 << actor_range::ALERTED)
        );
        let orbs = evaluate(&g.chains.slots[&ORBS], &mut f.w).unwrap();
        assert_eq!((orbs.status, orbs.listed, orbs.matched), (0, 2, 2));
        // A ball's fields read through a structure in the first, through a
        // reference in the second, never both.
        assert_eq!(
            orbs.items[0].missing,
            (1 << orb_range::WORTH[1]) | (1 << orb_range::ID[1]) | (1 << orb_range::MATRIX[1])
        );
        assert_eq!(
            orbs.items[1].missing,
            (1 << orb_range::WORTH[0]) | (1 << orb_range::ID[0]) | (1 << orb_range::MATRIX[0])
        );
        let loot = evaluate(&g.chains.slots[&LOOT], &mut f.w).unwrap();
        assert_eq!((loot.status, loot.listed, loot.matched), (0, 3, 3));
        assert_eq!(
            loot.items.iter().map(|i| i.missing).collect::<Vec<_>>(),
            [0, 0, 1 << loot_range::VELOCITY]
        );
        let _ = (f.leaf, f.types.int, f.walk);
    }

    #[test]
    fn a_records_blocks_become_chain_records_keyed_by_actor() {
        let mut f = fixture();
        let g = GiantsChains::resolve(&mut f.w, &f.watch, &kinds()).unwrap();
        // A record: the app field, then each chain's block as O6 would write
        // it, the leaf's slot then taken by another leaf (its active time
        // back to 0.5) in a second record.
        let record = |f: &mut Fixture| {
            let app = f.w.app;
            let mut payload = f.w.gather(&[(app + 0x54, 0x14)]).unwrap()[0]
                .clone()
                .unwrap();
            for spec in g.chains.slots.values() {
                payload.extend(block_bytes(spec, &evaluate(spec, &mut f.w).unwrap()));
            }
            payload
        };
        let first = record(&mut f);
        let leaf_placement = f.w.get32(f.leaf + 0x50);
        f.w.putf(leaf_placement + 0xB4, 0.5);
        f.w.put32(f.w.app + 0x5C, 5961);
        let second = record(&mut f);
        let records: Vec<WatchRecord> = [first, second]
            .into_iter()
            .enumerate()
            .map(|(k, payload)| WatchRecord {
                flip: 100 + k as u64,
                host_us: 0,
                missing: 0,
                read: 0,
                payload,
            })
            .collect();
        // The watch's list: the application's frame field alone.
        let watch = GiantsWatch {
            list: WatchList::new(vec![crate::watch::Field::new("app", f.w.app + 0x54, 0x14)])
                .unwrap(),
            ..f.watch.clone()
        };
        let blocks: Vec<BTreeMap<u8, ChainBlock>> = records
            .iter()
            .map(|r| g.chains.split(&watch.list, r).unwrap().1)
            .collect();
        let resolved = g.resolve_names(&mut f.w, &blocks).unwrap();
        assert_eq!(
            resolved
                .float_types
                .values()
                .filter(|k| **k == Some(false))
                .count(),
            1
        );
        assert_eq!(
            resolved
                .behaviours
                .values()
                .cloned()
                .collect::<BTreeSet<_>>(),
            ["Battle".to_owned(), "default".to_owned()].into()
        );
        let (chain, _) = g.records(&watch, &records, &resolved).unwrap();
        assert_eq!(chain.len(), 2);
        let first = &chain[0];
        let by_kind = |k: &str| {
            first
                .items
                .iter()
                .filter(|i| i.kind == k)
                .collect::<Vec<_>>()
        };
        let hero = by_kind(HERO)[0];
        assert_eq!(hero.vars["health"], 290.0);
        assert_eq!(hero.placement.as_deref(), Some("009_Eruptor"));
        let chompy = by_kind("chompy")[0];
        assert_eq!(chompy.placement.as_deref(), Some("Enemy_Chompy(1)"));
        assert_eq!((chompy.vars["health"], chompy.vars["alerted"]), (1.0, 1.0));
        assert_eq!(chompy.body.as_ref().unwrap().position, [12.0, 22.4, -60.0]);
        assert_eq!(chompy.body.as_ref().unwrap().velocity, [1.2, 0.0, 0.0]);
        assert!(chompy.behaviour.is_some() && chompy.serial.is_none());
        // A clone: no placement, a serial; its slot taken by another leaf
        // (its active time back) is a new serial.
        let leaf = by_kind("leaf")[0];
        assert_eq!((leaf.placement.as_deref(), leaf.serial), (None, Some(0)));
        let leaf2 = chain[1].items.iter().find(|i| i.kind == "leaf").unwrap();
        assert_eq!((leaf2.address, leaf2.serial), (leaf.address, Some(1)));
        // Orbs and coins: their worth and serial, the clone's placement none,
        // the placed treasure named.
        let orbs = by_kind("orb");
        assert_eq!(
            orbs.iter()
                .map(|o| (o.serial, o.vars["worth"]))
                .collect::<Vec<_>>(),
            [(Some(7), 1.0), (Some(8), 1.0)]
        );
        assert_eq!(orbs[1].body.as_ref().unwrap().position, [14.0, 23.0, -61.0]);
        let coins = by_kind("coin");
        assert_eq!(
            coins
                .iter()
                .map(|c| (c.placement.clone(), c.vars["worth"]))
                .collect::<Vec<_>>(),
            [
                (Some("Copper_Coins".to_owned()), 5.0),
                (None, 2.0),
                (Some("Coin_spinning".to_owned()), 1.0)
            ]
        );
        assert_eq!(coins[1].body.as_ref().unwrap().velocity, [0.0, 5.0, 0.0]);
        // No corpus: the transform is its body, the velocity's bit set.
        let far = coins[2].body.as_ref().unwrap();
        assert_eq!(
            (far.position, far.velocity),
            ([40.0, 49.0, -66.0], [0.0; 3])
        );
        assert_eq!(coins[2].missing, 1 << loot_range::VELOCITY);
        assert_eq!((first.game_frame, chain[1].game_frame), (5960, 5961));
        // And the names fight_trace takes: each kind's behaviour cells.
        let names = g.names(&blocks, &resolved);
        assert_eq!(
            names.behaviours["chompy"].values().next().unwrap(),
            "Battle"
        );
    }

    #[test]
    fn a_name_in_two_kinds_or_a_missing_export_is_refused() {
        let mut f = fixture();
        let mut k = kinds();
        k.push(("again".into(), vec!["Enemy_Chompy(1)".into()]));
        let err = GiantsChains::resolve(&mut f.w, &f.watch, &k).unwrap_err();
        assert!(format!("{err:#}").contains("in two kinds"), "{err:#}");
        let mut k = kinds();
        k.push((HERO.into(), vec!["x".into()]));
        assert!(GiantsChains::resolve(&mut f.w, &f.watch, &k).is_err());
        // The orbs' path broken under O6's walk: the block says which link.
        let g = GiantsChains::resolve(&mut f.w, &f.watch, &kinds()).unwrap();
        let mut spec = g.chains.slots[&ORBS].clone();
        spec.path.push(Hop::checked(0, 0xDEAD_BEEF));
        let b = evaluate(&spec, &mut f.w).unwrap();
        assert_eq!((b.status, b.listed), (spec.path.len() as u32, 0));
    }

    /// A block's bytes as O6 writes them.
    fn block_bytes(spec: &ChainSpec, b: &ChainBlock) -> Vec<u8> {
        let mut out = vec![0u8; spec.bytes()];
        let kept = b.items.len() as u32;
        for (i, w) in [b.status, b.listed, b.unread, b.matched, kept]
            .iter()
            .enumerate()
        {
            out[4 * i..4 * i + 4].copy_from_slice(&w.to_be_bytes());
        }
        for (k, item) in b.items.iter().enumerate() {
            let at = CHAIN_HEADER + k * spec.item_bytes();
            for (i, w) in [item.address, item.index, item.value, item.missing]
                .iter()
                .enumerate()
            {
                out[at + 4 * i..at + 4 * i + 4].copy_from_slice(&w.to_be_bytes());
            }
            let mut r = at + CHAIN_ITEM_HEADER;
            for bytes in &item.ranges {
                out[r..r + bytes.len()].copy_from_slice(bytes);
                r += bytes.len();
            }
        }
        out
    }
}
