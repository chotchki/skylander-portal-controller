//! Giants' O6 chains (openspec AH.5, design decision 16): what the fight's
//! recordings follow besides O5's fixed list, and how a record's blocks
//! become `guac_oracle`'s [`ChainRecord`]s keyed by actor.
//!
//! O6 knows nothing of the game: it follows a chain the harness names. Giants'
//! four, Giants 01.00 (`BLUS30968`), offsets from each object's start:
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
//! - [`ANIMS`] (AH.9): the hero's clips. O5's list holds the `animControl`s
//!   his `_nowPlaying` vector held at the resolve, and an attack plays a
//!   control made for it (a throw dropped every clip from 20 of AH.7's
//!   records), so the vector itself is a chain: his corpus's
//!   `_animationBlock` (+0x7C, an `animationComponent`), its count at +0x08
//!   and its slots behind +0x10, each control kept by its vtable, with the
//!   control from `_animationSequence` to its end (+0x08 to +0x2C: the id at
//!   +0x0C, `_timeRemaining` +0x14, `_period` +0x18), its sequence's
//!   `_playbackPercent` to `_blendFactor` (+0x20 to +0x3C) and the name
//!   pointer of its `tfbPhysicsAnimation` (sequence +0x3C, then +0x08), each
//!   name read at the end pause. A record's clips are then the vector's,
//!   whatever made them ([`GiantsChains::clips`]).
//!
//! The breakables in reach and their debris, the Chompies, Norticus, the hero
//! and Eruptor's projectiles ride [`ACTORS`] by name ([`fight_kinds`]): a
//! placed actor from F0 while it's listed, a clone from its spawn to its last
//! listing (AH.5's walks kept every body readable to the last; a coin's
//! corpus can go a record before its loot entry, its transform then its
//! body). Giants' actors carry no id or spawn count, so a clone's serial
//! counts its slot being taken again; an orb's is the game's own `my ID`. The
//! hero's health, experience and gold come from O5's fight cells
//! ([`crate::fight`]), so his item carries them every record. [`evaluate`] is
//! O6's walk over guest memory read at a pause, the reference a recording's
//! last block is checked against.

use std::collections::{BTreeMap, BTreeSet};

use anyhow::{Context, Result, bail};
use guac_oracle::fight::oracle::{ChainItem, ChainRecord, Names};
use guac_oracle::fight::ticks::HERO;
use guac_oracle::recording::{Body, Clip};
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
pub const ANIMS: u8 = 3;
/// AH.10: the four Chompies' script state past [`ACTORS`]' health and
/// alerted flag ([`CHOMPY_VARS`]), and the hero's ([`HERO_VARS`]), each
/// through the actor list again, keyed by name, every variable found by
/// O6's search.
pub const CREATURES: u8 = 4;
pub const HERO_STATE: u8 = 5;
/// AH.54: the eruption's lava. The actor list once more, kept by the
/// templates [`LAVA_TEMPLATES`] names: each clone's placement from its
/// `_currentMatrix` to its `_tint` (its place, its hang: `_attachNode`,
/// `_attachNodeOwner` and `_attachXform`, its `_scaleVec`, its tint), its
/// corpus's world transform (`_xform`), its velocity, the head of what it
/// hangs from and of that node's owner, and the eruption's own reach
/// (`$Damage Range`, a local of `009_LavaAttack.ai`).
pub const LAVA: u8 = 6;
/// AH.55: each Chompy's clips. O6 arms 8 chains, so the four Chompies'
/// `_nowPlaying` vectors ride one: the actor list kept by [`CHOMPIES`]' names,
/// each corpus's vector count and its first [`CHOMPY_CLIP_SLOTS`] controls,
/// each as [`ANIMS`] reads one of the hero's (the control's span, its
/// sequence's, its animation's name pointer), a slot that holds no control
/// read as missing.
pub const CHOMPY_ANIMS: u8 = 7;
/// AH.59: a drive that follows the script scheduler (`oracle goto --sched`)
/// arms its two chains in [`LAVA`]'s and [`CHOMPY_ANIMS`]' slots, O6 arming
/// 8 at most: the scripts' updater with each group's placement lists
/// ([`crate::sched::order_chain`]) and group 1's actors with their stamps
/// ([`crate::sched::active_chain`]).
pub const SCHED_ORDER: u8 = LAVA;
pub const SCHED_ACTIVE: u8 = CHOMPY_ANIMS;
/// A Chompy's vector slots [`CHOMPY_ANIMS`] reads.
pub const CHOMPY_CLIP_SLOTS: usize = 6;

/// What [`CREATURES`] follows on each Chompy, by label and path, as
/// `Enemy_Chompy.ai` and the enemy system name them: the state machine's
/// state and substate (the nip's steps: 0 and 1 its windup, 2 its hold, 3
/// its lunge, 10 its recovery), its timer and scratch, the bite's cooldown,
/// its end flag and who it has bitten this lunge (a set: its count), the
/// hurt state, its timer and the immunity after it, the knockback's timer,
/// resistance and last strength, who it's after and who told it.
pub const CHOMPY_VARS: [(&str, &[&str]); 16] = [
    ("nip", &["stateMachine", "state tracking", "state"]),
    ("substate", &["stateMachine", "state tracking", "substate"]),
    ("state timer", &["stateMachine", "state tracking", "timer"]),
    (
        "state scratch",
        &["stateMachine", "state tracking", "scratch"],
    ),
    ("cooldown timer", &["cooldown timer"]),
    ("should end bite now", &["should end bite now"]),
    ("actors hit", &["actors hit"]),
    ("hurt state", &["description", "state"]),
    ("mode", &["description", "state tracking", "mode"]),
    (
        "mode timer",
        &["description", "state tracking", "mode timer"],
    ),
    (
        "immunity timer",
        &["description", "state tracking", "immunity timer"],
    ),
    ("knockback timer", &["description", "knockback timer"]),
    (
        "knockback resist",
        &["description", "state tracking", "knockback current resist"],
    ),
    (
        "knockback strength",
        &[
            "description",
            "state tracking",
            "knockback last knock strength",
        ],
    ),
    (
        "target",
        &["description", "attention parameters", "alerted target"],
    ),
    (
        "communicator",
        &["description", "attention parameters", "communicator"],
    ),
];

/// What [`HERO_STATE`] follows on the hero: his hurt state, its timer and
/// the immunity after it, his knockback's timer, resistance, last strength
/// and heading, and the kills his player is credited with.
pub const HERO_VARS: [(&str, &[&str]); 9] = [
    ("hurt state", &["description", "state"]),
    ("mode", &["description", "state tracking", "mode"]),
    (
        "mode timer",
        &["description", "state tracking", "mode timer"],
    ),
    (
        "immunity timer",
        &["description", "state tracking", "immunity timer"],
    ),
    ("knockback timer", &["description", "knockback timer"]),
    (
        "knockback resist",
        &["description", "state tracking", "knockback current resist"],
    ),
    (
        "knockback strength",
        &[
            "description",
            "state tracking",
            "knockback last knock strength",
        ],
    ),
    (
        "knockback heading",
        &[
            "description",
            "state tracking",
            "knockback last knock heading",
        ],
    ),
    (
        "enemies defeated",
        &[
            "my Minion Description",
            "player joined data",
            "player stat tracking",
            "enemies defeated",
        ],
    ),
];

/// Items each chain keeps a record. AF.9's list held 162-182 actors in the
/// clearing (AH.5's walks 166-199); the fight keeps the hero, 4 Chompies,
/// Norticus' 2 placements, the 16 breakables in reach, the 9 debris
/// templates, Eruptor's projectiles (a lob every 0.25 s living 2 s is 8 at
/// once) and the debris a break spawns, 3 a breakable living 1.5 s, so an
/// eruption breaking five leaves 15: 64. A Chompy's 4 experience splits
/// into 4 or 5 orbs (AH.4's 1.1), 16-20 for the fight against 64; the
/// clearing's loot is 3 placed treasures and a coin or two a break. The
/// hero's blend held 5 clips at AF.8's fresh state in 8 slots.
pub const ACTOR_ITEMS: u32 = 64;
pub const ORB_ITEMS: u32 = 64;
pub const LOOT_ITEMS: u32 = 32;
pub const ANIM_ITEMS: u32 = 16;
/// An eruption spawns 18 blobs and a heat shimmer (AH.24's headless count)
/// beside itself, and the next can't start before it's gone (AH.9: 1 s
/// after the clip ends, the lava gone 0.7 s past it): 32.
pub const LAVA_ITEMS: u32 = 32;

/// The templates [`LAVA`] keeps, each with the kind its clones go by: the
/// three lava blobs `009_LavaAttack.ai` picks from at random, its heat
/// shimmer, and the eruption itself, whose reach rides only this chain (its
/// body and behaviour are [`ACTORS`]').
pub const LAVA_TEMPLATES: [(&str, &str); 5] = [
    ("lava_blob", "009_Lava"),
    ("lava_blob_1", "009_Lava(1)"),
    ("lava_blob_2", "009_Lava(2)"),
    ("lava_heat", "009_LavaHeatDistotion"),
    ("eruption", "009_LavaAttack"),
];

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
/// absence", `ag7/out/reach2-final.txt`), the ledge route's three
/// (`barrel_b_intact(1)` and `(4)`, `pottery_1(8)`; design decision 1) and
/// the three more the ledge route's two eruptions broke in AH.8's route
/// (`barrel_b_intact(3)`, `gourd_01(12)` and `(13)`).
pub const BREAKABLES: [&str; 16] = [
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
    "barrel_b_intact(3)",
    "gourd_01(12)",
    "gourd_01(13)",
];
/// The templates in Eruptor's archive whose clones his attacks make (the
/// actors among `009_eruptor.bld`'s placements: the lava ball, its upgrade,
/// the fire blob and its splash, the eruption and its volcano, and upgrade
/// 4's Magma Ball and its impact, AH.41), each with the kind its clones go
/// by, so a trace tells a ball from the splash under his hand (AH.9); and
/// the collider his join makes and each throw hangs on the hand it throws
/// from (`$primary attack collider`, AH.50), so a trace has that hand's
/// place every frame.
pub const ERUPTOR_SPAWNS: [(&str, &str); 9] = [
    ("lava_ball", "009_LavaProjectile"),
    ("lava_ball_upgrade", "009_LavaProjectileUpgrade"),
    ("fire_blob", "009_FireBlob"),
    ("fire_blob_splash", "009_FireBlobSplash"),
    ("eruption", "009_LavaAttack"),
    ("mini_volcano", "009_MiniVolcano"),
    ("magma_ball", "009_MagmaBall"),
    ("magma_ball_impact", "009_MagmaBall_Impact"),
    ("lava_collider", "009_LavaCollider"),
];

/// The currency art `Loot - Get Loot` clones a dropped coin from (level
/// 213's placements of that model set: worth 1, 1 and 2 common, 3 and 7
/// uncommon in AH.9's drops), so a coin's actor rides [`ACTORS`] from its
/// spawn through its slide to the hero, after its loot entry has gone.
pub const COIN_ART: [&str; 5] = ["1_Coin", "2_Coin(1)", "3_Coin(2)", "4_Coin(3)", "5_Coin(1)"];

/// The spinning coins AH.8's route takes, on the walkway's west drop and the
/// six down into the pit (AH.11): each placement rides [`ACTORS`] through the
/// slide its loot entry leaves before.
pub const SPINNING_COINS: [&str; 9] = [
    "Coin_spinning",
    "Coin_spinning_0",
    "Coin_spinning_0(1)",
    "Coin_spinning(1)",
    "Coin_spinning(2)",
    "Coin_spinning(3)",
    "Coin_spinning(4)",
    "Coin_spinning(5)",
    "Coin_spinning(6)",
];

/// The debris templates in level 213 for the breakables in reach (its
/// placements named so beside `barrel_b_intact`, `pottery_*` and `gourd_*`).
/// AH.9 says which ones a break spawns, and what the stone wall leaves.
pub const DEBRIS: [&str; 9] = [
    "barrel_b_debris_01",
    "barrel_b_debris_02",
    "barrel_b_debris_03",
    "pottery_debris_(1)",
    "pottery_debris_(2)",
    "pottery_debris_(3)",
    "gourd_debris_(1)",
    "gourd_debris_(2)",
    "gourd_debris_(3)",
];

/// The kinds [`ACTORS`] keeps for the fight, by placement name, beside the
/// hero.
pub fn fight_kinds() -> Vec<(String, Vec<String>)> {
    let owned = |names: &[&str]| names.iter().map(|n| (*n).to_owned()).collect();
    vec![
        ("chompy".into(), owned(&CHOMPIES)),
        ("norticus".into(), owned(&NORTICUS)),
        ("breakable".into(), owned(&BREAKABLES)),
        ("debris".into(), owned(&DEBRIS)),
        ("coin_art".into(), owned(&COIN_ART)),
        ("spinning_coin".into(), owned(&SPINNING_COINS)),
    ]
    .into_iter()
    .chain(
        ERUPTOR_SPAWNS
            .iter()
            .map(|(kind, name)| ((*kind).to_owned(), vec![(*name).to_owned()])),
    )
    .collect()
}

// Offsets, from the registry (AC.4) and AF.9's and AH.4's layouts.
const CORPUS_PLACEMENT: u16 = 0x50;
const CORPUS_VELOCITY: u16 = 0x58;
const PLACEMENT_NAME: u16 = 0x08;
const PLACEMENT_MATRIX: u16 = 0x3C;
const PLACEMENT_STATE: u16 = 0x54;
const PLACEMENT_PARENT: u16 = 0x5C;
const PLACEMENT_ATTACH_NODE: u16 = 0x6C;
const PLACEMENT_ATTACH_NODE_OWNER: u16 = 0x70;
const PLACEMENT_BEHAVIOUR: u16 = 0xAC;
const CORPUS_XFORM: u16 = actors::CORPUS_XFORM as u16;
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
/// The corpus's `_animationBlock`, its `_nowPlaying` count and slots, a
/// control's watched span and its sequence, the sequence's watched span and
/// its `tfbPhysicsAnimation` (as [`crate::watch`] reads them).
const CORPUS_ANIMATION: u16 = 0x7C;
const COMPONENT_PLAYING: u16 = 0x08;
const COMPONENT_SLOTS: u16 = 0x10;
const CONTROL_SEQUENCE: u16 = 0x08;
const CONTROL_FIRST: u16 = 0x08;
const CONTROL_LEN: u16 = 0x2C - CONTROL_FIRST;
const SEQUENCE_FIRST: u16 = 0x20;
const SEQUENCE_LEN: u16 = 0x3C - SEQUENCE_FIRST;
const SEQUENCE_ANIMATION: u16 = 0x3C;
const ANIMATION_NAME: u16 = actors::NAME as u16;
/// A clip's name is its `.ma` path (57 bytes for Eruptor's idle); longer is
/// cut.
const CLIP_NAME_LEN: usize = 128;

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

/// [`LAVA`]'s ranges, in order.
pub mod lava_range {
    /// The placement from `_currentMatrix` (+0x3C) to `_tint` (+0xCC).
    pub const PLACEMENT: usize = 0;
    /// The corpus's `_xform`, its world transform (row vectors).
    pub const WORLD: usize = 1;
    pub const VELOCITY: usize = 2;
    /// The first 16 bytes of `_attachNode`'s object (its vtable at +0, its
    /// name pointer at +0x08 when it has one), and of `_attachNodeOwner`'s.
    pub const ATTACH: usize = 3;
    pub const ATTACH_OWNER: usize = 4;
    /// `$Damage Range`'s cell (the eruption's alone).
    pub const REACH: usize = 5;

    /// Offsets inside [`PLACEMENT`]'s bytes.
    pub const FIRST: u16 = 0x3C;
    pub const LEN: u16 = 0xCC - FIRST;
    pub const MATRIX: usize = 0;
    pub const ACTIVITY: usize = (0x54 - FIRST) as usize;
    pub const PARENT: usize = (0x5C - FIRST) as usize;
    pub const ATTACH_NODE: usize = (0x6C - FIRST) as usize;
    pub const ATTACH_NODE_OWNER: usize = (0x70 - FIRST) as usize;
    pub const ATTACH_XFORM: usize = (0x78 - FIRST) as usize;
    pub const BEHAVIOUR: usize = (0xAC - FIRST) as usize;
    pub const ACTIVE: usize = (0xB4 - FIRST) as usize;
    pub const SCALE: usize = (0xB8 - FIRST) as usize;
    pub const TINT: usize = (0xC8 - FIRST) as usize;
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

/// [`ANIMS`]' ranges: the control from +0x08, its sequence from +0x20, its
/// animation's name pointer.
pub mod anim_range {
    pub const CONTROL: usize = 0;
    pub const SEQUENCE: usize = 1;
    pub const NAME: usize = 2;
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

/// What a followed variable's last cell is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Leaf {
    /// A number: its type word and value.
    Number,
    /// A reference: the object it names, decoded against
    /// [`GiantsChains::referents`].
    Reference,
    /// A set: its count.
    Set,
}

/// One variable [`CREATURES`] or [`HERO_STATE`] follows, in range order.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Followed {
    pub label: String,
    pub path: Vec<String>,
    pub leaf: Leaf,
}

/// The hops from a corpus to the cell `path` names among `placement`'s
/// variables, each step a structure held in place or one a reference names,
/// as the pause holds them, and what the last cell is. Refused at the step
/// that breaks.
fn variable_hops(g: &mut impl Gather, placement: u32, path: &[&str]) -> Result<(Vec<Hop>, Leaf)> {
    let Some((first, rest)) = path.split_first() else {
        bail!("an empty path");
    };
    let mut cells = script::variables(g, placement)?
        .with_context(|| format!("the placement at {placement:#010x} never started its script"))?;
    let mut hops = vec![
        Hop::at(CORPUS_PLACEMENT),
        Hop::checked(PLACEMENT_LOCALS, script::vtable::OBJECT_LIST),
        named(first),
    ];
    let mut cell = script::the(&cells, first, "the placement")?.clone();
    let mut here = (*first).to_owned();
    for step in rest {
        let structure = match cell.vtable {
            script::vtable::SCRIPT_STRUCTURE => cell.at,
            script::vtable::SCRIPT_REFERENCE => {
                hops.push(through_reference());
                script::referent(g, &cell)?
                    .with_context(|| format!("`{here}` is a null reference"))?
            }
            other => bail!("`{here}` has vtable {other:#010x}, which holds no cells"),
        };
        hops.extend(field(step));
        cells = script::fields(g, structure, &here)?;
        cell = script::the(&cells, step, &here)?.clone();
        here = format!("{here}.{step}");
    }
    let leaf = match cell.vtable {
        script::vtable::VALUE_INFO => Leaf::Number,
        script::vtable::SCRIPT_REFERENCE | script::vtable::PLACEMENT_REFERENCE => Leaf::Reference,
        script::vtable::SCRIPT_SET => Leaf::Set,
        other => bail!("`{here}` has vtable {other:#010x}: no number, reference or set"),
    };
    Ok((hops, leaf))
}

/// The range a followed variable's cell is read through.
fn followed_probe(mut hops: Vec<Hop>, leaf: Leaf) -> Probe {
    match leaf {
        Leaf::Number => Probe::new(hops, CELL_NUMBER, 8),
        Leaf::Reference => Probe::new(hops, REFERENCE_OBJECT, 4),
        Leaf::Set => {
            hops.push(Hop::checked(CONTAINER_LIST, script::vtable::SET_LIST));
            Probe::new(hops, LIST_COUNT, 4)
        }
    }
}

/// `vars` resolved on the first of `placements` whose script holds each
/// path, each with its range; a path none holds is left out.
fn follow_vars(
    g: &mut impl Gather,
    placements: &[u32],
    vars: &[(&str, &[&str])],
) -> Result<(Vec<Followed>, Vec<Probe>)> {
    let mut followed = Vec::new();
    let mut probes = Vec::new();
    for (label, path) in vars {
        let found = placements
            .iter()
            .find_map(|&p| variable_hops(g, p, path).ok());
        if let Some((hops, leaf)) = found {
            followed.push(Followed {
                label: (*label).to_owned(),
                path: path.iter().map(|s| (*s).to_owned()).collect(),
                leaf,
            });
            probes.push(followed_probe(hops, leaf));
        }
    }
    Ok((followed, probes))
}

/// The game's actor list from the application, its updater at
/// `updater_index`.
fn actor_list_path(updater_index: u32) -> Result<Vec<Hop>> {
    let at = u16::try_from(4 * updater_index).context("the updater index")?;
    Ok(vec![
        Hop::checked(0, vtable::APPLICATION),
        Hop::checked(INSIGHT, vtable::INSIGHT_CORE),
        Hop::checked(UPDATERS, vtable::UPDATER_LIST),
        Hop::at(LIST_ARRAY),
        Hop::checked(at, vtable::ACTOR_UPDATE),
        Hop::checked(ACTOR_UPDATE_LIST, vtable::GAME_ACTOR_LIST),
    ])
}

/// A corpus's key: its placement's name, hashed.
fn corpus_key() -> Key {
    Key {
        hops: vec![Hop::checked(CORPUS_PLACEMENT, vtable::ACTOR_INFO)],
        off: PLACEMENT_NAME,
        hashed: true,
    }
}

/// The actor list kept to `names`, each corpus with `ranges`.
fn followed_chain(updater_index: u32, names: &[&str], ranges: Vec<Probe>) -> Result<ChainSpec> {
    Ok(ChainSpec {
        items: names.len() as u32,
        base: actors::APP,
        path: actor_list_path(updater_index)?,
        count: LIST_COUNT,
        array: LIST_ARRAY,
        key: corpus_key(),
        values: names.iter().map(|n| name_hash(n)).collect(),
        ranges,
    })
}

/// The actor list's chain: the hero and every kind by name.
fn actor_chain(updater_index: u32, values: Vec<u32>) -> Result<ChainSpec> {
    Ok(ChainSpec {
        items: ACTOR_ITEMS,
        base: actors::APP,
        path: actor_list_path(updater_index)?,
        count: LIST_COUNT,
        array: LIST_ARRAY,
        key: corpus_key(),
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

/// The actor list walked again, kept by [`LAVA_TEMPLATES`]' names, each
/// corpus read through [`lava_range`]'s six ranges.
fn lava_chain(updater_index: u32) -> Result<ChainSpec> {
    let placement = || Hop::at(CORPUS_PLACEMENT);
    Ok(ChainSpec {
        items: LAVA_ITEMS,
        base: actors::APP,
        path: actor_list_path(updater_index)?,
        count: LIST_COUNT,
        array: LIST_ARRAY,
        key: corpus_key(),
        values: LAVA_TEMPLATES.iter().map(|(_, n)| name_hash(n)).collect(),
        ranges: vec![
            Probe::new(vec![placement()], lava_range::FIRST, lava_range::LEN),
            Probe::new(vec![Hop::at(CORPUS_XFORM)], 0, actors::MATRIX_LEN as u16),
            Probe::new(vec![], CORPUS_VELOCITY, 12),
            Probe::new(vec![placement(), Hop::at(PLACEMENT_ATTACH_NODE)], 0, 0x10),
            Probe::new(
                vec![placement(), Hop::at(PLACEMENT_ATTACH_NODE_OWNER)],
                0,
                0x10,
            ),
            Probe::new(variable(&["Damage Range"]), CELL_NUMBER, 8),
        ],
    })
}

/// The hero's `_nowPlaying` controls, from his corpus.
fn anim_chain(hero_corpus: u32) -> ChainSpec {
    let sequence = || Hop::checked(CONTROL_SEQUENCE, vtable::ANIMATION_INFO);
    ChainSpec {
        items: ANIM_ITEMS,
        base: hero_corpus,
        path: vec![Hop::checked(CORPUS_ANIMATION, vtable::ANIMATION_COMPONENT)],
        count: COMPONENT_PLAYING,
        array: COMPONENT_SLOTS,
        key: Key {
            hops: Vec::new(),
            off: 0,
            hashed: false,
        },
        values: vec![vtable::ANIM_CONTROL],
        ranges: vec![
            Probe::new(vec![], CONTROL_FIRST, CONTROL_LEN),
            Probe::new(vec![sequence()], SEQUENCE_FIRST, SEQUENCE_LEN),
            Probe::new(
                vec![
                    sequence(),
                    Hop::checked(SEQUENCE_ANIMATION, vtable::PHYSICS_ANIMATION),
                ],
                ANIMATION_NAME,
                4,
            ),
        ],
    }
}

/// A kept control's clip and its animation's name pointer, when its
/// control and sequence read.
fn anim_clip(e: &ChainEntry) -> Option<(Clip, Option<u32>)> {
    let c = e.range(anim_range::CONTROL)?;
    let s = e.range(anim_range::SEQUENCE)?;
    Some(clip_of(c, s, e.range(anim_range::NAME)))
}

/// [`CHOMPY_ANIMS`]' ranges: the vector's count, then per slot its control,
/// sequence and name pointer, as [`anim_range`] has them for the hero.
pub mod chompy_clip_range {
    pub const COUNT: usize = 0;
    /// Slot `k`'s ranges start here, three a slot.
    pub fn slot(k: usize) -> usize {
        1 + 3 * k
    }
}

/// [`chompy_clip_range`]'s probes, from a Chompy's corpus.
fn chompy_clip_probes() -> Vec<Probe> {
    let block = || Hop::checked(CORPUS_ANIMATION, vtable::ANIMATION_COMPONENT);
    let mut probes = vec![Probe::new(vec![block()], COMPONENT_PLAYING, 4)];
    for k in 0..CHOMPY_CLIP_SLOTS {
        let control = vec![
            block(),
            Hop::at(COMPONENT_SLOTS),
            Hop::checked(4 * k as u16, vtable::ANIM_CONTROL),
        ];
        let mut sequence = control.clone();
        sequence.push(Hop::checked(CONTROL_SEQUENCE, vtable::ANIMATION_INFO));
        let mut animation = sequence.clone();
        animation.push(Hop::checked(SEQUENCE_ANIMATION, vtable::PHYSICS_ANIMATION));
        probes.push(Probe::new(control, CONTROL_FIRST, CONTROL_LEN));
        probes.push(Probe::new(sequence, SEQUENCE_FIRST, SEQUENCE_LEN));
        probes.push(Probe::new(animation, ANIMATION_NAME, 4));
    }
    probes
}

/// A [`CHOMPY_ANIMS`] entry's clips with their name pointers, in its
/// vector's order up to its count; a slot whose control or sequence didn't
/// read ends them.
fn chompy_item_clips(e: &ChainEntry) -> Vec<(Clip, Option<u32>)> {
    let Some(count) = e
        .range(chompy_clip_range::COUNT)
        .map(|b| be32(b, 0) as usize)
    else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for k in 0..count.min(CHOMPY_CLIP_SLOTS) {
        let r = chompy_clip_range::slot(k);
        let (Some(c), Some(s)) = (e.range(r), e.range(r + 1)) else {
            break;
        };
        out.push(clip_of(c, s, e.range(r + 2)));
    }
    out
}

/// A clip from a control's span, its sequence's and its name pointer.
fn clip_of(c: &[u8], s: &[u8], name: Option<&[u8]>) -> (Clip, Option<u32>) {
    let at = |off: u16| usize::from(off - CONTROL_FIRST);
    let clip = Clip {
        id: be32(c, at(0x0C)),
        weight: f32_at(s, usize::from(0x38 - SEQUENCE_FIRST)),
        rate: f32_at(s, 0),
        period: f32_at(c, at(0x18)),
        remaining: f32_at(c, at(0x14)),
    };
    (clip, name.map(|b| be32(b, 0)).filter(|&p| p != 0))
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
    /// What [`CREATURES`] follows, in range order: none when no Chompy's
    /// script had started at the resolve (AH.10).
    #[serde(default)]
    pub creature_vars: Vec<Followed>,
    /// What [`HERO_STATE`] follows, in range order.
    #[serde(default)]
    pub hero_vars: Vec<Followed>,
    /// The placements a followed reference is named by, in order: a
    /// reference to the n-th (from 0) reads n + 1, a null one 0, any other
    /// object -1. The hero's, then each of [`CHOMPIES`]' in order, 0 for one
    /// the actor list didn't hold at the resolve.
    #[serde(default)]
    pub referents: Vec<(String, u32)>,
    /// AH.59: the scheduler's chains in [`SCHED_ORDER`] and [`SCHED_ACTIVE`]
    /// rather than the lava's and the Chompies' clips.
    #[serde(default)]
    pub sched: bool,
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
        Self::resolve_with(g, watch, kinds, None)
    }

    /// [`GiantsChains::resolve`], with `sched` (the scheduler as F0 holds
    /// it, [`crate::sched::snapshot`]) following the script scheduler in
    /// [`SCHED_ORDER`] and [`SCHED_ACTIVE`] instead (AH.59).
    pub fn resolve_with(
        g: &mut impl Gather,
        watch: &GiantsWatch,
        kinds: &[(String, Vec<String>)],
        sched: Option<&crate::sched::SchedSnapshot>,
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
        let actor_spec = actor_chain(updater_index as u32, values)?;
        // AH.10: the Chompies' and the hero's script state, each path
        // resolved on what the pause holds (a Chompy whose script never
        // started holds none).
        let listed = evaluate(&actor_spec, g)?;
        let mut referents = vec![(HERO.to_owned(), watch.hero_placement)];
        let mut chompies = Vec::new();
        for name in CHOMPIES {
            let at = actors.iter().position(|(_, n)| n == name).and_then(|v| {
                listed
                    .items
                    .iter()
                    .find(|i| i.value as usize == v)
                    .and_then(|i| i.range(actor_range::PLACEMENT))
                    .map(|b| be32(b, 0))
            });
            // One the list doesn't hold keeps its place at 0, which no
            // reference read past a null matches, so `Enemy_Chompy(n)` reads
            // n + 1 whichever are missing.
            referents.push((name.to_owned(), at.unwrap_or(0)));
            chompies.extend(at);
        }
        let (creature_vars, creature_probes) = follow_vars(g, &chompies, &CHOMPY_VARS)?;
        let (hero_vars, hero_probes) = follow_vars(g, &[watch.hero_placement], &HERO_VARS)?;
        slots.insert(ACTORS, actor_spec);
        slots.insert(ORBS, orb_chain(combat_control));
        slots.insert(LOOT, loot_chain(macros_ui));
        slots.insert(ANIMS, anim_chain(watch.hero_corpus));
        match sched {
            Some(s) => {
                slots.insert(SCHED_ORDER, crate::sched::order_chain());
                slots.insert(
                    SCHED_ACTIVE,
                    crate::sched::active_chain(s.index, s.awake_vtables())?,
                );
            }
            None => {
                slots.insert(LAVA, lava_chain(updater_index as u32)?);
                // AH.55: the Chompies' clips, whichever the list holds as it
                // runs.
                slots.insert(
                    CHOMPY_ANIMS,
                    followed_chain(updater_index as u32, &CHOMPIES, chompy_clip_probes())?,
                );
            }
        }
        if !creature_vars.is_empty() {
            slots.insert(
                CREATURES,
                followed_chain(updater_index as u32, &CHOMPIES, creature_probes)?,
            );
        }
        if !hero_vars.is_empty() {
            let hero = &actors[0].1;
            slots.insert(
                HERO_STATE,
                followed_chain(updater_index as u32, &[hero.as_str()], hero_probes)?,
            );
        }
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
            creature_vars,
            hero_vars,
            referents,
            sched: sched.is_some(),
        })
    }

    /// What [`CREATURES`] or [`HERO_STATE`] follows.
    fn followed(&self, slot: u8) -> &[Followed] {
        match slot {
            CREATURES => &self.creature_vars,
            HERO_STATE => &self.hero_vars,
            _ => &[],
        }
    }

    /// A followed item's variables by label: a number as its cell holds it,
    /// a set's count, a reference by [`GiantsChains::referents`]; one that
    /// didn't read is left out.
    fn followed_vars(
        &self,
        slot: u8,
        e: &ChainEntry,
        resolved: &Resolved,
    ) -> BTreeMap<String, f32> {
        let mut out = BTreeMap::new();
        for (r, f) in self.followed(slot).iter().enumerate() {
            let Some(b) = e.range(r) else {
                continue;
            };
            let v = match f.leaf {
                Leaf::Number => resolved.number(b),
                Leaf::Set => Some(be32(b, 0) as f32),
                Leaf::Reference => Some(match be32(b, 0) {
                    0 => 0.0,
                    at => self
                        .referents
                        .iter()
                        .position(|(_, p)| *p == at)
                        .map_or(-1.0, |i| (i + 1) as f32),
                }),
            };
            if let Some(v) = v {
                out.insert(f.label.clone(), v);
            }
        }
        out
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
        let mut heads: BTreeMap<u32, Option<Vec<u8>>> = BTreeMap::new();
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
            if let Some(b) = record.get(&LAVA).filter(|_| !self.sched) {
                for i in &b.items {
                    types.extend(i.range(lava_range::REACH).map(word));
                    let Some(p) = i.range(lava_range::PLACEMENT) else {
                        continue;
                    };
                    for (at, head) in [
                        (lava_range::ATTACH_NODE, lava_range::ATTACH),
                        (lava_range::ATTACH_NODE_OWNER, lava_range::ATTACH_OWNER),
                    ] {
                        let node = be32(p, at);
                        if node != 0 {
                            heads
                                .entry(node)
                                .or_insert_with(|| i.range(head).map(<[u8]>::to_vec));
                        }
                    }
                }
            }
            for slot in [CREATURES, HERO_STATE] {
                let Some(b) = record.get(&slot) else {
                    continue;
                };
                for i in &b.items {
                    for (r, f) in self.followed(slot).iter().enumerate() {
                        if f.leaf == Leaf::Number {
                            types.extend(i.range(r).map(word));
                        }
                    }
                }
            }
        }
        let mut clip_names = BTreeMap::new();
        for record in blocks {
            for (&slot, b) in record {
                let named: Vec<(Clip, Option<u32>)> = match slot {
                    ANIMS => b.items.iter().filter_map(anim_clip).collect(),
                    CHOMPY_ANIMS if !self.sched => {
                        b.items.iter().flat_map(chompy_item_clips).collect()
                    }
                    _ => continue,
                };
                for (clip, name) in named {
                    if let Some(at) = name {
                        clip_names.insert(clip.id, at);
                    }
                }
            }
        }
        let mut out = Resolved::default();
        // A name the end pause can't read keeps the clip named by its id, so
        // a recording's every clip is named either way.
        for (id, at) in clip_names {
            let name = one(g, at, CLIP_NAME_LEN, "a clip's name")
                .map(|b| c_string(&b))
                .unwrap_or_else(|_| format!("clip {id:#010x}"));
            out.clip_names.insert(id, name);
        }
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
        // A cell or a name freed since its record (AH.11: a hero beaten, a
        // figure taken off) is named by its address rather than failing the
        // whole ring.
        for cell in cells {
            let name = one(g, cell, 0x0C, "a behaviour cell").and_then(|head| {
                actors::expect_vtable(
                    &head,
                    script::vtable::USER_BEHAVIOR,
                    cell,
                    "OpUserBehavior",
                )?;
                one(g, be32(&head, 0x08), 64, "a behaviour's name").map(|n| c_string(&n))
            });
            out.behaviours.insert(
                cell,
                name.unwrap_or_else(|_| format!("freed behaviour {cell:#010x}")),
            );
        }
        // A hang's node by the head a record read (the node may be gone by
        // the end pause), its name read now from the pointer there.
        for (node, head) in heads {
            let text = match head {
                Some(h) => {
                    let vt = be32(&h, 0);
                    let at = be32(&h, 0x08);
                    let name = (at != 0 && at.is_multiple_of(4))
                        .then(|| one(g, at, 64, "a node's name").ok().map(|n| c_string(&n)))
                        .flatten()
                        .filter(|n| {
                            !n.is_empty() && n.chars().all(|c| c.is_ascii_graphic() || c == ' ')
                        });
                    match name {
                        Some(n) => format!("{vt:#010x} {n}"),
                        None => format!("{vt:#010x}"),
                    }
                }
                None => "unread".to_owned(),
            };
            out.attach_nodes.insert(node, text);
        }
        for at in names {
            let name = one(g, at, 64, "a loot actor's name")
                .map(|n| c_string(&n))
                .unwrap_or_else(|_| format!("freed name {at:#010x}"));
            out.loot_names.insert(at, name);
        }
        Ok(out)
    }

    /// The behaviour names `fight_trace` takes, by kind, from what the blocks
    /// held.
    pub fn names(&self, blocks: &[BTreeMap<u8, ChainBlock>], resolved: &Resolved) -> Names {
        let mut out = Names {
            clips: resolved.clip_names.clone(),
            ..Names::default()
        };
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
        // A blob's slot taken again shows as its active time going back.
        let mut lava_serials = Serials::default();
        let mut out = Vec::with_capacity(records.len());
        let mut all = Vec::with_capacity(records.len());
        for record in records {
            let (o5, blocks) = self.chains.split(&watch.list, record)?;
            let game_frame = game_frame(&watch.list, &o5)
                .with_context(|| format!("the game frame at flip {}", o5.flip))?;
            let mut items = Vec::new();
            let mut followed: Vec<(u32, BTreeMap<String, f32>)> = Vec::new();
            let mut lava: Vec<ChainItem> = Vec::new();
            for (&slot, block) in &blocks {
                // The clips ride beside the actors, not among them
                // ([`GiantsChains::clips`]); a vector that didn't read loses
                // that record's clips alone.
                if slot == ANIMS || slot == CHOMPY_ANIMS {
                    continue;
                }
                // AH.59's scheduler chains decode on their own
                // ([`crate::sched::decode`]).
                if self.sched && slot == SCHED_ORDER {
                    continue;
                }
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
                        // AH.10's followed variables join their actor's item.
                        CREATURES | HERO_STATE => {
                            followed
                                .push((entry.address, self.followed_vars(slot, entry, resolved)));
                            continue;
                        }
                        // AH.54: a blob or the shimmer is an item of its
                        // own; the eruption's reach joins its [`ACTORS`] item.
                        LAVA => {
                            lava.push(lava_item(entry, resolved, &mut lava_serials)?);
                            continue;
                        }
                        other => bail!("no chain in slot {other}"),
                    });
                }
            }
            for item in lava {
                match items.iter_mut().find(|i| i.address == item.address) {
                    Some(i) => i.vars.extend(item.vars),
                    None => items.push(item),
                }
            }
            for (address, vars) in followed {
                if let Some(item) = items
                    .iter_mut()
                    .find(|i| i.address == address && (i.kind == HERO || i.placement.is_some()))
                {
                    item.vars.extend(vars);
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

    /// A record's clips as [`ANIMS`] read them, in the vector's order, the
    /// shape O5's list gives a frame ([`guac_oracle::recording::Frame::clips`]);
    /// `None` when the vector didn't read or a kept control's span didn't.
    pub fn clips(blocks: &Blocks) -> Option<Vec<Clip>> {
        let b = blocks.get(&ANIMS)?;
        if b.status != 0 {
            return None;
        }
        b.items
            .iter()
            .map(|e| anim_clip(e).map(|(c, _)| c))
            .collect()
    }

    /// Each Chompy's clips in a record, by name, as [`CHOMPY_ANIMS`] read
    /// them, in its vector's order: a Chompy the list doesn't hold is left
    /// out, one whose vector didn't read is `None` (AH.55).
    pub fn chompy_clips(blocks: &Blocks) -> BTreeMap<String, Option<Vec<Clip>>> {
        let Some(b) = blocks.get(&CHOMPY_ANIMS).filter(|b| b.status == 0) else {
            return BTreeMap::new();
        };
        b.items
            .iter()
            .filter_map(|e| {
                let name = CHOMPIES.get(e.value as usize)?;
                let read = e.range(chompy_clip_range::COUNT).is_some();
                let clips =
                    read.then(|| chompy_item_clips(e).into_iter().map(|(c, _)| c).collect());
                Some(((*name).to_owned(), clips))
            })
            .collect()
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
/// Each record's Chompy clips ([`CHOMPY_ANIMS`]), when any are followed;
/// their names are [`Resolved::clip_names`].
pub const CREATURE_CLIPS: &str = "creature-clips.json";

/// One record's [`CREATURE_CLIPS`] entry.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CreatureClips {
    pub flip: u64,
    pub read: u32,
    pub game_frame: u32,
    pub clips: BTreeMap<String, Option<Vec<Clip>>>,
}
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
    ) -> Result<Written> {
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
                        k if LAVA_TEMPLATES[..4].iter().any(|(l, _)| *l == k) => LAVA,
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
        if self.sched {
            let sched: Vec<crate::sched::SchedRecord> = blocks
                .iter()
                .zip(&decoded)
                .map(|(b, d)| {
                    crate::sched::decode(
                        (d.flip, d.read, d.game_frame),
                        b.get(&SCHED_ORDER),
                        b.get(&SCHED_ACTIVE),
                    )
                })
                .collect();
            let names = crate::sched::names(g, &sched)?;
            std::fs::write(
                dir.join(crate::sched::SCHED_RECORDS),
                serde_json::to_string(&sched)?,
            )?;
            std::fs::write(
                dir.join(crate::sched::SCHED_NAMES),
                serde_json::to_string_pretty(&names)?,
            )?;
        } else if self.chains.slots.keys().any(|&s| s == CHOMPY_ANIMS) {
            let frames: Vec<CreatureClips> = blocks
                .iter()
                .zip(&decoded)
                .map(|(b, d)| CreatureClips {
                    flip: d.flip,
                    read: d.read,
                    game_frame: d.game_frame,
                    clips: Self::chompy_clips(b),
                })
                .collect();
            std::fs::write(dir.join(CREATURE_CLIPS), serde_json::to_string(&frames)?)?;
        }
        let clips = blocks.iter().map(Self::clips).collect();
        Ok(Written {
            o5,
            run,
            decoded,
            clips,
        })
    }
}

/// What [`GiantsChains::write_outputs`] split off and decoded.
pub struct Written {
    /// Each record as O5's list alone, for `watch.json` and the recording.
    pub o5: Vec<WatchRecord>,
    pub run: ChainRun,
    pub decoded: Vec<ChainRecord>,
    /// Each record's clips as [`ANIMS`] read them ([`GiantsChains::clips`]).
    pub clips: Vec<Option<Vec<Clip>>>,
}

/// `recording` with each frame whose clips O5's list dropped (an attack's
/// control made after the resolve, AH.9) given the clips [`ANIMS`] read in
/// the same record; frames O5 read keep theirs. Returns how many it filled.
pub fn fill_clips(
    recording: guac_oracle::recording::Recording,
    clips: &[Option<Vec<Clip>>],
) -> Result<(guac_oracle::recording::Recording, usize)> {
    let mut frames = recording.frames().to_vec();
    if frames.len() != clips.len() {
        bail!(
            "{} frames against {} records of clips",
            frames.len(),
            clips.len()
        );
    }
    let mut filled = 0;
    for (frame, c) in frames.iter_mut().zip(clips) {
        if let (true, Some(c)) = (frame.clips.is_empty(), c)
            && !c.is_empty()
        {
            frame.clips = c.clone();
            filled += 1;
        }
    }
    Ok((guac_oracle::recording::Recording::new(frames)?, filled))
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
    /// A clip's name (its `.ma` path), by its id, from [`ANIMS`].
    #[serde(default)]
    pub clip_names: BTreeMap<u32, String>,
    /// What a [`LAVA`] item hangs from, and that node's owner, by address:
    /// its vtable and, where its head holds a readable name pointer at +0x08,
    /// the name (AH.54).
    #[serde(default)]
    pub attach_nodes: BTreeMap<u32, String>,
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

/// A [`LAVA`] entry as an item: its kind by template, its place and velocity
/// as its body, and as numbers its scale (percent per axis), its tint (0 to
/// 255 a channel), whether it hangs (`attached`, and `attach.node`, the
/// node's place among [`Resolved::attach_nodes`] counted from 1), the hang's
/// own transform, its world place, its activity and active time and, on the
/// eruption, its reach (feet).
fn lava_item(e: &ChainEntry, resolved: &Resolved, serials: &mut Serials) -> Result<ChainItem> {
    use lava_range as r;
    let (kind, name) = LAVA_TEMPLATES
        .get(e.value as usize)
        .with_context(|| format!("no lava value {}", e.value))?;
    let p = e.range(r::PLACEMENT);
    let word = |at: usize| p.map(|b| be32(b, at));
    let parent = word(r::PARENT);
    let clone = parent.is_some_and(|w| w != 0);
    let active = p.map(|b| f32_at(b, r::ACTIVE));
    let mut vars = BTreeMap::new();
    if let Some(b) = p {
        let s: [f32; 3] = floats(b, r::SCALE);
        for (axis, v) in ["x", "y", "z"].iter().zip(s) {
            vars.insert(format!("scale.{axis}"), v);
        }
        for (i, ch) in ["r", "g", "b", "a"].iter().enumerate() {
            vars.insert(format!("tint.{ch}"), f32::from(b[r::TINT + i]));
        }
        let node = be32(b, r::ATTACH_NODE);
        vars.insert("attached".into(), f32::from(u8::from(node != 0)));
        if let Some(i) = resolved.attach_nodes.keys().position(|&a| a == node) {
            vars.insert("attach.node".into(), (i + 1) as f32);
        }
        let owner = be32(b, r::ATTACH_NODE_OWNER);
        if let Some(i) = resolved.attach_nodes.keys().position(|&a| a == owner) {
            vars.insert("attach.owner".into(), (i + 1) as f32);
        }
        let x: [f32; 6] = floats(b, r::ATTACH_XFORM);
        for (label, v) in ["x", "y", "z", "tilt", "heading", "roll"].iter().zip(x) {
            vars.insert(format!("attach.{label}"), v);
        }
        vars.insert("placement.activity".into(), be32(b, r::ACTIVITY) as f32);
        vars.insert("placement.active".into(), f32_at(b, r::ACTIVE));
    }
    if let Some(w) = e.range(r::WORLD) {
        let m: [f32; 16] = floats(w, 0);
        for (axis, v) in ["x", "y", "z"].iter().zip(&m[12..15]) {
            vars.insert(format!("world.{axis}"), *v);
        }
    }
    if let Some(v) = e.range(r::REACH).and_then(|b| resolved.number(b)) {
        vars.insert("reach".into(), v);
    }
    let matrix = p.map(|b| &b[r::MATRIX..r::MATRIX + 24]);
    Ok(ChainItem {
        address: e.address,
        kind: (*kind).into(),
        placement: (!clone).then(|| (*name).to_owned()),
        serial: clone.then(|| serials.of(e.address, None, active)),
        body: body(matrix, e.range(r::VELOCITY).map(|b| floats(b, 0))),
        behaviour: word(r::BEHAVIOUR).filter(|&c| c != 0),
        vars,
        missing: u64::from(e.missing),
    })
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
        /// The hero's `_nowPlaying` slots and the controls in them.
        slots: u32,
        controls: Vec<u32>,
    }

    /// Eruptor's idle and walk as AF.8's fresh state plays them, then the
    /// throw an attack adds: (name, id, weight, rate, period, remaining).
    const CLIPS: [(&str, u32, f32, f32, f32, f32); 3] = [
        (
            "Models/Characters/MinionsMonsters/Eruptor/Eruptor_Idle.ma",
            0x3090_C890,
            100.0,
            100.0,
            1.333_333_4,
            0.788_604,
        ),
        (
            "Models/Characters/MinionsMonsters/Eruptor/Eruptor_Walk.ma",
            0x3090_B960,
            0.0,
            5.817_651,
            1.066_666_7,
            0.288_884,
        ),
        (
            "Models/Characters/MinionsMonsters/Eruptor/Eruptor_Attack1.ma",
            0x3090_D000,
            100.0,
            150.0,
            0.5,
            0.42,
        ),
    ];

    /// A control playing `clip`, its sequence and animation behind it.
    fn anim_control(w: &mut World, clip: (&str, u32, f32, f32, f32, f32)) -> u32 {
        let (name, id, weight, rate, period, remaining) = clip;
        let animation = w.object(vtable::PHYSICS_ANIMATION, 0x2C);
        let at = w.string(name);
        w.put32(animation + 0x08, at);
        w.put32(animation + 0x28, id);
        let sequence = w.object(vtable::ANIMATION_INFO, 0x44);
        w.put32(sequence + 0x3C, animation);
        w.putf(sequence + 0x20, rate);
        w.putf(sequence + 0x38, weight);
        let control = w.object(vtable::ANIM_CONTROL, 0x2C);
        w.put32(control + 0x08, sequence);
        w.put32(control + 0x0C, id);
        w.putf(control + 0x14, remaining);
        w.putf(control + 0x18, period);
        control
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
        // The hero's blend: the idle and the walk in 8 slots, and a word in
        // the third that isn't a control (the filter passes it over).
        let block = w.object(vtable::ANIMATION_COMPONENT, 0x28);
        w.put32(hero + 0x7C, block);
        let slots = w.alloc(32);
        w.put32(block + 0x08, 3);
        w.put32(block + 0x0C, 0x1000_0020);
        w.put32(block + 0x10, slots);
        let controls: Vec<u32> = CLIPS[..2]
            .iter()
            .map(|&c| anim_control(&mut w, c))
            .collect();
        let stranger = w.object(vtable::ANIMATION_INFO, 0x44);
        for (i, &c) in controls.iter().chain([&stranger]).enumerate() {
            w.put32(slots + 4 * i as u32, c);
        }
        Fixture {
            w,
            hero,
            chompy,
            leaf: leaf.corpus,
            watch,
            types: t,
            walk,
            slots,
            controls,
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

    /// AH.59: with the scheduler asked for, its two chains take the lava's
    /// and the Chompies' clips' slots, the rest as before, within O6's
    /// budget.
    #[test]
    fn sched_mode_arms_the_schedulers_chains_in_the_lava_and_clip_slots() {
        let mut f = fixture();
        let plain = GiantsChains::resolve(&mut f.w, &f.watch, &kinds()).unwrap();
        let snapshot = crate::sched::SchedSnapshot {
            input: None,
            scheduler_class: None,
            updaters: Vec::new(),
            script_update: 0,
            index: 2,
            cursor: [0; 3],
            shutdown: None,
            groups: Vec::new(),
        };
        let g = GiantsChains::resolve_with(&mut f.w, &f.watch, &kinds(), Some(&snapshot)).unwrap();
        assert!(g.sched && !plain.sched);
        assert_eq!(
            g.chains.slots[&SCHED_ORDER],
            crate::sched::order_chain(),
            "the order chain in the lava's slot"
        );
        assert_eq!(
            g.chains.slots[&SCHED_ACTIVE],
            crate::sched::active_chain(2, vec![vtable::ACTOR_INFO]).unwrap()
        );
        for slot in [ACTORS, ORBS, LOOT, ANIMS, CREATURES, HERO_STATE] {
            assert_eq!(
                g.chains.slots.get(&slot),
                plain.chains.slots.get(&slot),
                "{slot}"
            );
        }
        assert!(
            g.chains.bytes() <= crate::verbs::CHAIN_BUDGET,
            "{}",
            g.chains.bytes()
        );
    }

    #[test]
    fn giants_chains_resolve_and_walk_the_fixture_as_o6_would() {
        let mut f = fixture();
        let g = GiantsChains::resolve(&mut f.w, &f.watch, &kinds()).unwrap();
        assert_eq!(g.updater_index, 1);
        assert_eq!(g.actors[0], (HERO.to_owned(), "009_Eruptor".to_owned()));
        assert_eq!(g.kind(1), Some("chompy"));
        assert_eq!(g.actors.len(), 1 + 4 + 2 + 16 + 9 + 5 + 9 + 9 + 1);
        assert!(g.chains.bytes() <= crate::verbs::CHAIN_BUDGET);
        let actors = evaluate(&g.chains.slots[&ACTORS], &mut f.w).unwrap();
        // The hero, the Chompy, the barrel, the leaf and the dropped coin's
        // clone of its currency art (AH.9); the rock and the placed treasure
        // aren't asked for.
        assert_eq!((actors.status, actors.unread, actors.matched), (0, 0, 5));
        let kept: Vec<&str> = actors
            .items
            .iter()
            .map(|i| g.kind(i.value).unwrap())
            .collect();
        assert_eq!(kept, [HERO, "chompy", "breakable", "leaf", "coin_art"]);
        // Each of Eruptor's spawns its own kind.
        assert_eq!(
            g.actors
                .iter()
                .filter(|(k, n)| n.starts_with("009_") && k != HERO)
                .map(|(k, _)| k.as_str())
                .collect::<Vec<_>>(),
            [
                "lava_ball",
                "lava_ball_upgrade",
                "fire_blob",
                "fire_blob_splash",
                "eruption",
                "mini_volcano",
                "magma_ball",
                "magma_ball_impact",
                "lava_collider"
            ]
        );
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
        // The hero's clips: his two controls kept in slot order, each range
        // read; the slot holding something else listed and passed over.
        let anims = evaluate(&g.chains.slots[&ANIMS], &mut f.w).unwrap();
        assert_eq!(
            (anims.status, anims.listed, anims.unread, anims.matched),
            (0, 3, 0, 2)
        );
        assert_eq!(
            anims
                .items
                .iter()
                .map(|i| (i.address, i.index, i.missing))
                .collect::<Vec<_>>(),
            [(f.controls[0], 0, 0), (f.controls[1], 1, 0)]
        );
        let clips: Vec<Clip> = anims
            .items
            .iter()
            .map(|e| anim_clip(e).unwrap().0)
            .collect();
        assert_eq!(
            clips[0],
            Clip {
                id: 0x3090_C890,
                weight: 100.0,
                rate: 100.0,
                period: 1.333_333_4,
                remaining: 0.788_604,
            }
        );
        assert_eq!(
            (clips[1].id, clips[1].weight, clips[1].rate),
            (0x3090_B960, 0.0, 5.817_651)
        );
        // A hero with no animation block: the list's link breaks, no items.
        f.w.put32(f.hero + 0x7C, 0);
        let none = evaluate(&g.chains.slots[&ANIMS], &mut f.w).unwrap();
        assert_eq!((none.status, none.items.len()), (1, 0));
        let _ = (f.leaf, f.types.int, f.walk, f.slots);
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
        // Then the slot taken by a leaf with a placement of its own whose
        // clock is past the last one's: the placement changing alone is a new
        // serial.
        let other = f.w.object(0x0062_1048, 0xF8);
        for off in (0x08..0xF8).step_by(4) {
            let v = f.w.get32(leaf_placement + off);
            f.w.put32(other + off, v);
        }
        f.w.putf(other + 0xB4, 0.9);
        f.w.put32(f.leaf + 0x50, other);
        f.w.put32(f.w.app + 0x5C, 5962);
        // And an attack: its throw's control made now, in the third slot.
        let throw = anim_control(&mut f.w, CLIPS[2]);
        f.w.put32(f.slots + 8, throw);
        let third = record(&mut f);
        let records: Vec<WatchRecord> = [first, second, third]
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
        // A behaviour cell freed since its record (AH.11: a hero beaten, its
        // vtable cleared) is named by its address and the rest still resolve.
        let freed = *resolved.behaviours.keys().next().unwrap();
        let vtable_was = f.w.get32(freed);
        f.w.put32(freed, 0);
        let again = g.resolve_names(&mut f.w, &blocks).unwrap();
        assert_eq!(
            again.behaviours[&freed],
            format!("freed behaviour {freed:#010x}")
        );
        assert_eq!(again.behaviours.len(), resolved.behaviours.len());
        assert!(
            again
                .behaviours
                .iter()
                .all(|(c, n)| *c == freed || resolved.behaviours[c] == *n)
        );
        f.w.put32(freed, vtable_was);
        let (chain, _) = g.records(&watch, &records, &resolved).unwrap();
        assert_eq!(chain.len(), 3);
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
        let leaf3 = chain[2].items.iter().find(|i| i.kind == "leaf").unwrap();
        assert_eq!((leaf3.address, leaf3.serial), (leaf.address, Some(2)));
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
        // The clips ride beside the actors: the idle and the walk in the
        // first two records, the throw joining in the third, every id named
        // from its animation, the names fight_trace takes the same.
        let clips: Vec<Vec<Clip>> = blocks
            .iter()
            .map(|b| GiantsChains::clips(b).unwrap())
            .collect();
        let ids = |c: &[Clip]| c.iter().map(|c| c.id).collect::<Vec<_>>();
        assert_eq!(ids(&clips[0]), [0x3090_C890, 0x3090_B960]);
        assert_eq!(ids(&clips[1]), ids(&clips[0]));
        assert_eq!(ids(&clips[2]), [0x3090_C890, 0x3090_B960, 0x3090_D000]);
        assert_eq!(
            (clips[2][2].weight, clips[2][2].rate, clips[2][2].remaining),
            (100.0, 150.0, 0.42)
        );
        assert_eq!(
            resolved.clip_names.values().cloned().collect::<Vec<_>>(),
            [CLIPS[1].0, CLIPS[0].0, CLIPS[2].0]
        );
        assert_eq!(names.clips, resolved.clip_names);
        assert!(
            chain
                .iter()
                .all(|r| r.items.iter().all(|i| i.kind != "clip"))
        );
        // A vector that didn't read: that record's clips alone are lost.
        let mut broken = blocks[0].clone();
        broken.get_mut(&ANIMS).unwrap().status = 1;
        assert_eq!(GiantsChains::clips(&broken), None);
    }

    #[test]
    fn the_chains_clips_fill_the_frames_o5_dropped_them_from() {
        use guac_oracle::recording::{Clock, Frame, Recording};
        let clip = |id: u32, remaining: f32| Clip {
            id,
            weight: 100.0,
            rate: 150.0,
            period: 0.5,
            remaining,
        };
        let frame = |k: u32, clips: Vec<Clip>| Frame {
            flip: 30 + u64::from(k),
            read: 0,
            host_us: 16_667 * u64::from(k),
            game_frame: 6_662 + k,
            clock: Clock {
                frame_delta: 1.0 / 60.0,
                presentation_rate: 1.0 / 60.0,
                logic_scale: 1.0,
                logic_stopped: false,
                object_scale: 1.0,
            },
            hero: Body {
                position: [31.1, 51.5, -73.0],
                rotation: [0.0, 253.7, 0.0],
                velocity: [0.0; 3],
            },
            camera: None,
            stick: [0.0, 0.0],
            pad: None,
            clips,
            contacts: None,
            mover: None,
        };
        let idle = clip(0x3090_C890, 0.8);
        let rec = Recording::new(vec![
            frame(0, vec![idle]),
            frame(1, Vec::new()),
            frame(2, Vec::new()),
            frame(3, vec![idle]),
            frame(4, Vec::new()),
        ])
        .unwrap();
        let chain = vec![
            Some(vec![clip(0x3090_C890, 0.1)]),
            Some(vec![idle, clip(0x3090_D000, 0.42)]),
            None,
            Some(Vec::new()),
            Some(Vec::new()),
        ];
        let (filled, n) = fill_clips(rec.clone(), &chain).unwrap();
        assert_eq!(n, 1);
        let f = filled.frames();
        // O5's own clips stay; a dropped frame takes the chain's, or stays
        // empty (and uncounted) where the chain didn't read or read none.
        assert_eq!(f[0].clips, [idle]);
        assert_eq!(f[1].clips, chain[1].clone().unwrap());
        assert!(f[2].clips.is_empty());
        assert_eq!(f[3].clips, [idle]);
        assert!(f[4].clips.is_empty());
        // The counts have to line up.
        assert!(fill_clips(rec, &chain[..3]).is_err());
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

    /// Guest memory as separate regions, so a test can end one a word short.
    struct Regions(BTreeMap<u32, Vec<u8>>);

    impl Gather for Regions {
        fn gather(&mut self, ranges: &[(u32, usize)]) -> Result<Vec<Option<Vec<u8>>>> {
            Ok(ranges
                .iter()
                .map(|&(addr, len)| {
                    let (&base, bytes) = self.0.range(..=addr).next_back()?;
                    let at = (addr - base) as usize;
                    bytes.get(at..at.checked_add(len)?).map(<[u8]>::to_vec)
                })
                .collect())
        }
    }

    impl Regions {
        fn put(&mut self, addr: u32, v: u32) {
            let (&base, bytes) = self.0.range_mut(..=addr).next_back().unwrap();
            let at = (addr - base) as usize;
            bytes[at..at + 4].copy_from_slice(&v.to_be_bytes());
        }

        fn region(&mut self, at: u32, len: usize) {
            self.0.insert(at, vec![0; len]);
        }

        /// A C string at the start of a page of its own: guest memory is
        /// mapped by the page, and the walk reads a name to its page's end.
        fn string(&mut self, at: u32, s: &str) {
            let mut b = vec![0; 0x1000];
            b[..s.len()].copy_from_slice(s.as_bytes());
            self.0.insert(at, b);
        }
    }

    #[test]
    fn the_walks_bounds_and_broken_links_are_o6s() {
        const BASE: u32 = 0x0010_0000;
        const LIST: u32 = 0x0020_0000;
        const ARRAY: u32 = 0x0030_0000;
        const SUB: u32 = 0x0060_0000;
        let item = |k: u32| 0x0040_0000 + 0x100 * k;
        let (n255, n256) = ("n".repeat(255), "m".repeat(256));
        let mut m = Regions(BTreeMap::new());
        m.region(BASE, 0x10);
        m.put(BASE, LIST);
        m.region(LIST, 0x20);
        m.put(LIST + 0x08, 3);
        m.put(LIST + 0x14, ARRAY);
        m.region(ARRAY, 4 * 4_096);
        // Three items, each a name pointer at +0 and a list at +0x18 (its
        // array +0x24) whose element named `hp` holds 7 at +4.
        m.region(item(0), 0x300);
        for (k, name) in ["chompy", n255.as_str(), n256.as_str()]
            .into_iter()
            .enumerate()
        {
            let k = k as u32;
            m.put(ARRAY + 4 * k, item(k));
            m.string(0x0050_0000 + 0x1000 * k, name);
            m.put(item(k), 0x0050_0000 + 0x1000 * k);
            m.put(item(k) + 0x24, SUB);
        }
        m.region(SUB, 4 * 1_025);
        m.put(SUB, 0x0070_0000);
        m.region(0x0070_0000, 0x10);
        m.string(0x0070_1000, "hp");
        m.put(0x0070_0000, 0x0070_1000);
        m.put(0x0070_0004, 7);
        // chompy's search runs over 1,024 elements, the 255-byte name's over
        // 1,025; the 256-byte name has no NUL in O6's 256 bytes.
        m.put(item(0) + 0x18, 1_024);
        m.put(item(1) + 0x18, 1_025);
        let spec = ChainSpec {
            items: 8,
            base: BASE,
            path: vec![Hop::at(0)],
            count: 0x08,
            array: 0x14,
            key: Key {
                hops: Vec::new(),
                off: 0,
                hashed: true,
            },
            values: vec![name_hash("chompy"), name_hash(&n255), name_hash(&n256)],
            ranges: vec![Probe::new(vec![Hop::named(0x18, 0x24, 0, "hp")], 4, 4)],
        };
        let b = evaluate(&spec, &mut m).unwrap();
        assert_eq!((b.status, b.listed, b.unread, b.matched), (0, 3, 1, 2));
        let kept: Vec<(u32, u32, u32)> = b
            .items
            .iter()
            .map(|i| (i.index, i.value, i.missing))
            .collect();
        assert_eq!(kept, [(0, 0, 0), (1, 1, 1)]);
        assert_eq!(b.items[0].range(0), Some(&7u32.to_be_bytes()[..]));
        // 4,096 is a list (the entries past the three null, unread); 4,097
        // isn't, the path's one hop then 3.
        m.put(LIST + 0x08, 4_096);
        let b = evaluate(&spec, &mut m).unwrap();
        assert_eq!(
            (b.status, b.listed, b.unread, b.matched),
            (0, 4_096, 4_094, 2)
        );
        m.put(LIST + 0x08, 4_097);
        let b = evaluate(&spec, &mut m).unwrap();
        assert_eq!((b.status, b.listed, b.items.len()), (2 + 2, 0, 0));
        // The count word, then the array pointer, past the end of the list's
        // memory; then the path's own hop null.
        m.region(0x0080_0000, 8);
        m.put(BASE, 0x0080_0000);
        assert_eq!(evaluate(&spec, &mut m).unwrap().status, 2);
        m.region(0x0090_0000, 0x10);
        m.put(0x0090_0008, 3);
        m.put(BASE, 0x0090_0000);
        assert_eq!(evaluate(&spec, &mut m).unwrap().status, 3);
        m.put(BASE, 0);
        assert_eq!(evaluate(&spec, &mut m).unwrap().status, 1);
    }

    /// A block's bytes as O6 writes them.
    /// AH.10's two chains on the fixture grown with a second Chompy running
    /// the enemy system's state machine mid-lunge (its state machine a
    /// structure held in place, its description behind a reference, who it
    /// bit a set, who it's after and who told it references), a third whose
    /// script never started, and the hero's description holding his hurt and
    /// knockback state and his player's kill count.
    #[test]
    fn the_chompies_and_the_heros_script_state_ride_chains_of_their_own() {
        let mut f = fixture();
        let t = fx::Types {
            int: f.types.int,
            float: f.types.float,
        };
        let w = &mut f.w;
        let hero_placement = w.get32(f.hero + 0x50);
        let chompy1 = w.get32(f.chompy + 0x50);
        // The hero: his description and his player's stats, in a list of
        // his own.
        let hp = fx::int(w, &t, "hp", 278);
        let max = fx::int(w, &t, "hp max", 290);
        let info = fx::structure(w, "", &[max, hp]);
        let alerted = fx::int(w, &t, "alerted", 2);
        let attention = fx::structure(w, "", &[alerted]);
        let tracking = [
            fx::int(w, &t, "mode", 1),
            fx::float(w, &t, "mode timer", 145.7316),
            fx::float(w, &t, "immunity timer", 146.3316),
            fx::float(w, &t, "knockback current resist", 12.5),
            fx::float(w, &t, "knockback last knock strength", 25.0),
            fx::float(w, &t, "knockback last knock heading", 20.672),
        ];
        let tracking = fx::structure(w, "state tracking", &tracking);
        let state = fx::int(w, &t, "state", 1);
        let timer = fx::float(w, &t, "knockback timer", 227.537);
        let refs = [
            fx::reference(w, script::vtable::SCRIPT_REFERENCE, "creature info", info),
            fx::reference(
                w,
                script::vtable::SCRIPT_REFERENCE,
                "attention parameters",
                attention,
            ),
        ];
        let desc = fx::structure(w, "", &[state, tracking, timer, refs[0], refs[1]]);
        let d = fx::reference(w, script::vtable::SCRIPT_REFERENCE, "description", desc);
        let defeated = fx::int(w, &t, "enemies defeated", 3);
        let stats = fx::structure(w, "player stat tracking", &[defeated]);
        let joined = fx::structure(w, "", &[stats]);
        let joined = fx::reference(
            w,
            script::vtable::SCRIPT_REFERENCE,
            "player joined data",
            joined,
        );
        let mine = fx::structure(w, "", &[joined]);
        let mine = fx::reference(
            w,
            script::vtable::SCRIPT_REFERENCE,
            "my Minion Description",
            mine,
        );
        let locals = w.list(script::vtable::OBJECT_LIST, &[d, mine], 2);
        w.put32(hero_placement + 0xB0, locals);
        // Chompy(2) mid-lunge, its immunity timer left out.
        let machine = [
            fx::int(w, &t, "state", 3),
            fx::int(w, &t, "substate", 0),
            fx::float(w, &t, "timer", 33.9),
            fx::int(w, &t, "scratch", 0),
        ];
        let machine = fx::structure(w, "state tracking", &machine);
        let machine = fx::structure(w, "stateMachine", &[machine]);
        let cooldown = fx::float(w, &t, "cooldown timer", 34.88);
        let end = fx::int(w, &t, "should end bite now", 1);
        // Who it bit: one in a list with room for four, so the count is
        // the count and not the room.
        let hit = fx::named(w, script::vtable::SCRIPT_SET, 0x20, "actors hit");
        let bitten = w.list(script::vtable::SET_LIST, &[hero_placement], 4);
        w.put32(hit + 0x18, bitten);
        let tracking = [
            fx::int(w, &t, "mode", 0),
            fx::float(w, &t, "mode timer", 0.0),
            fx::float(w, &t, "knockback current resist", 0.0),
            fx::float(w, &t, "knockback last knock strength", 0.0),
        ];
        let tracking = fx::structure(w, "state tracking", &tracking);
        let target = fx::reference(
            w,
            script::vtable::PLACEMENT_REFERENCE,
            "alerted target",
            hero_placement,
        );
        let told = fx::reference(
            w,
            script::vtable::PLACEMENT_REFERENCE,
            "communicator",
            chompy1,
        );
        let alerted = fx::int(w, &t, "alerted", 2);
        let attention = fx::structure(w, "", &[alerted, target, told]);
        let hp = fx::int(w, &t, "hp", 1);
        let info = fx::structure(w, "", &[hp]);
        let state = fx::int(w, &t, "state", 0);
        let timer = fx::float(w, &t, "knockback timer", 0.0);
        let refs = [
            fx::reference(w, script::vtable::SCRIPT_REFERENCE, "creature info", info),
            fx::reference(
                w,
                script::vtable::SCRIPT_REFERENCE,
                "attention parameters",
                attention,
            ),
        ];
        let desc = fx::structure(w, "", &[state, tracking, timer, refs[0], refs[1]]);
        let d = fx::reference(w, script::vtable::SCRIPT_REFERENCE, "description", desc);
        let (chompy2, _) = scripted(
            w,
            "Enemy_Chompy(2)",
            14.0,
            "attack - nip",
            &[machine, cooldown, end, hit, d],
        );
        let chompy3 = w
            .actor(&Spec {
                name: "Enemy_Chompy(3)".into(),
                ..Spec::default()
            })
            .corpus;

        let g = GiantsChains::resolve(&mut f.w, &f.watch, &kinds()).unwrap();
        // Every path the second Chompy holds, its immunity timer left out;
        // the first's paths broke at their first step, so each fell to the
        // second.
        let labels = |v: &[Followed]| v.iter().map(|f| f.label.clone()).collect::<Vec<_>>();
        let want: Vec<String> = CHOMPY_VARS
            .iter()
            .map(|(l, _)| (*l).to_owned())
            .filter(|l| l != "immunity timer")
            .collect();
        assert_eq!(labels(&g.creature_vars), want);
        let leaf = |l: &str| g.creature_vars.iter().find(|f| f.label == l).unwrap().leaf;
        assert_eq!(
            [
                leaf("nip"),
                leaf("actors hit"),
                leaf("target"),
                leaf("communicator")
            ],
            [Leaf::Number, Leaf::Set, Leaf::Reference, Leaf::Reference]
        );
        let all: Vec<String> = HERO_VARS.iter().map(|(l, _)| (*l).to_owned()).collect();
        assert_eq!(labels(&g.hero_vars), all);
        assert_eq!(
            g.referents
                .iter()
                .map(|(n, _)| n.as_str())
                .collect::<Vec<_>>(),
            [
                HERO,
                "Enemy_Chompy(1)",
                "Enemy_Chompy(2)",
                "Enemy_Chompy(3)",
                "Enemy_Chompy(4)"
            ]
        );
        // The fixture has no fourth Chompy: it keeps its place, at 0, so the
        // others read the same whichever is missing.
        assert_eq!(g.referents[4].1, 0);
        assert!(g.referents[..4].iter().all(|(_, p)| *p != 0));
        assert!(g.chains.bytes() <= crate::verbs::CHAIN_BUDGET);
        for slot in [CREATURES, HERO_STATE] {
            assert_eq!(g.chains.slots[&slot].check(), Ok(()));
        }
        // O6's walk: the first Chompy holds none of them, the second every
        // one, the third no variables at all.
        let creatures = evaluate(&g.chains.slots[&CREATURES], &mut f.w).unwrap();
        let every = (1u32 << want.len()) - 1;
        assert_eq!(
            creatures
                .items
                .iter()
                .map(|i| (i.address, i.missing))
                .collect::<Vec<_>>(),
            [(f.chompy, every), (chompy2, 0), (chompy3, every)]
        );
        let hero = evaluate(&g.chains.slots[&HERO_STATE], &mut f.w).unwrap();
        assert_eq!((hero.items.len(), hero.items[0].missing), (1, 0));

        // Two records: then the second's communicator null, its target a
        // stranger.
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
        f.w.put32(told + 0x1C, 0);
        f.w.put32(target + 0x1C, f.leaf);
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
        let (chain, _) = g.records(&watch, &records, &resolved).unwrap();
        let item = |k: usize, name: &str| {
            chain[k]
                .items
                .iter()
                .find(|i| i.placement.as_deref() == Some(name))
                .unwrap()
                .vars
                .clone()
        };
        let c2 = item(0, "Enemy_Chompy(2)");
        assert_eq!(
            [
                c2["nip"],
                c2["substate"],
                c2["state timer"],
                c2["cooldown timer"],
                c2["should end bite now"],
                c2["actors hit"],
                c2["target"],
                c2["communicator"],
                c2["health"],
            ],
            [3.0, 0.0, 33.9, 34.88, 1.0, 1.0, 1.0, 2.0, 1.0]
        );
        let c2 = item(1, "Enemy_Chompy(2)");
        assert_eq!((c2["target"], c2["communicator"]), (-1.0, 0.0));
        // The first Chompy and the third carry ACTORS' cells alone.
        assert!(!item(0, "Enemy_Chompy(1)").contains_key("nip"));
        assert!(!item(0, "Enemy_Chompy(3)").contains_key("nip"));
        let h = item(0, "009_Eruptor");
        assert_eq!(
            [
                h["hurt state"],
                h["mode"],
                h["mode timer"],
                h["immunity timer"],
                h["knockback timer"],
                h["knockback resist"],
                h["knockback strength"],
                h["knockback heading"],
                h["enemies defeated"],
                h["health"],
            ],
            [
                1.0, 1.0, 145.7316, 146.3316, 227.537, 12.5, 25.0, 20.672, 3.0, 278.0
            ]
        );
    }

    /// A fixture whose Chompies' scripts hold none of AH.10's paths (AF.8's
    /// fresh state: their scripts never started) arms neither chain; the
    /// lava's is armed whether any lava is listed or not (AH.54), and a clip
    /// chain for each Chompy the list holds (AH.55: the fixture's one).
    #[test]
    fn no_chompy_or_hero_state_to_follow_arms_neither_chain() {
        let mut f = fixture();
        let g = GiantsChains::resolve(&mut f.w, &f.watch, &kinds()).unwrap();
        assert!(g.creature_vars.is_empty() && g.hero_vars.is_empty());
        assert_eq!(
            g.chains.slots.keys().copied().collect::<Vec<_>>(),
            [ACTORS, ORBS, LOOT, ANIMS, LAVA, CHOMPY_ANIMS]
        );
    }

    /// AH.54: the eruption's lava on a chain of its own: a blob hung on a
    /// ground placement with its scale, tint, hang and world place, one
    /// hanging from nothing, and the eruption's reach joining its
    /// [`ACTORS`] item.
    #[test]
    fn the_eruptions_lava_rides_a_chain_of_its_own() {
        let mut f = fixture();
        let t = &f.types;
        let reach = fx::float(&mut f.w, t, "Damage Range", 12.5);
        let template = fx::placement(&mut f.w, "009_LavaAttack", None, 0.0, None);
        let (eruption, eruption_placement) =
            scripted(&mut f.w, "009_LavaAttack", 11.0, "LavaBlobbing", &[reach]);
        f.w.put32(eruption_placement + 0x5C, template);
        let ground = f.w.actor(&Spec {
            name: "rock_ground".into(),
            ..Spec::default()
        });
        let blob_template = fx::placement(&mut f.w, "009_Lava", None, 0.0, None);
        let mut world = [0.0f32; 16];
        world[0] = 2.6;
        world[5] = 3.0;
        world[10] = 0.5;
        world[12..15].copy_from_slice(&[10.6, 43.4, -92.9]);
        world[15] = 1.0;
        let blob = f.w.actor(&Spec {
            name: "009_Lava".into(),
            position: [10.5, 43.4, -93.0],
            rotation: [0.0, 36.0, 0.0],
            scale: [260.0, 300.0, 50.0],
            world: Some(world),
            velocity: [0.0, -1.0, 0.0],
            ..Spec::default()
        });
        f.w.put32(blob.placement + 0x5C, blob_template);
        f.w.put32(blob.placement + 0x6C, ground.placement);
        f.w.put_floats(blob.placement + 0x78, &[0.1, 0.2, 0.3, 0.0, 36.0, 0.0]);
        f.w.put32(blob.placement + 0xC8, 0xFF80_40C0);
        let blob2_template = fx::placement(&mut f.w, "009_Lava(2)", None, 0.0, None);
        let blob2 = f.w.actor(&Spec {
            name: "009_Lava(2)".into(),
            scale: [100.0, 100.0, 50.0],
            ..Spec::default()
        });
        f.w.put32(blob2.placement + 0x5C, blob2_template);
        let g = GiantsChains::resolve(&mut f.w, &f.watch, &kinds()).unwrap();
        assert!(g.chains.bytes() <= crate::verbs::CHAIN_BUDGET);
        let spec = &g.chains.slots[&LAVA];
        assert_eq!(spec.items, LAVA_ITEMS);
        let lava = evaluate(spec, &mut f.w).unwrap();
        assert_eq!((lava.status, lava.unread, lava.matched), (0, 0, 3));
        assert_eq!(
            lava.items.iter().map(|i| i.address).collect::<Vec<_>>(),
            [eruption, blob.corpus, blob2.corpus]
        );
        use lava_range as r;
        // The eruption: no world transform, no hang; its reach reads. A blob
        // runs no script: no reach; the second hangs from nothing.
        let hang = (1 << r::ATTACH) | (1 << r::ATTACH_OWNER);
        assert_eq!(lava.items[0].missing, (1 << r::WORLD) | hang);
        assert_eq!(
            lava.items[1].missing,
            (1 << r::ATTACH_OWNER) | (1 << r::REACH)
        );
        assert_eq!(
            lava.items[2].missing,
            (1 << r::WORLD) | hang | (1 << r::REACH)
        );
        // Decoded from a record as O6 writes it.
        let app = f.w.app;
        let mut payload = f.w.gather(&[(app + 0x54, 0x14)]).unwrap()[0]
            .clone()
            .unwrap();
        for spec in g.chains.slots.values() {
            payload.extend(block_bytes(spec, &evaluate(spec, &mut f.w).unwrap()));
        }
        let records = [WatchRecord {
            flip: 100,
            host_us: 0,
            missing: 0,
            read: 0,
            payload,
        }];
        let watch = GiantsWatch {
            list: WatchList::new(vec![crate::watch::Field::new("app", app + 0x54, 0x14)]).unwrap(),
            ..f.watch.clone()
        };
        let blocks: Vec<Blocks> = records
            .iter()
            .map(|r| g.chains.split(&watch.list, r).unwrap().1)
            .collect();
        let resolved = g.resolve_names(&mut f.w, &blocks).unwrap();
        assert_eq!(
            resolved.attach_nodes,
            [(ground.placement, "0x00621048 rock_ground".to_owned())].into()
        );
        let (chain, _) = g.records(&watch, &records, &resolved).unwrap();
        let items = &chain[0].items;
        let at = |a: u32| {
            let found: Vec<&ChainItem> = items.iter().filter(|i| i.address == a).collect();
            assert_eq!(found.len(), 1, "one item at {a:#x}");
            found[0]
        };
        let e = at(eruption);
        assert_eq!((e.kind.as_str(), e.vars["reach"]), ("eruption", 12.5));
        assert!(e.body.is_some(), "its body is the actor chain's");
        let b = at(blob.corpus);
        assert_eq!(b.kind, "lava_blob");
        assert_eq!(b.placement, None, "a clone");
        assert_eq!(b.serial, Some(0));
        let body = b.body.as_ref().unwrap();
        assert_eq!(
            (body.position, body.rotation, body.velocity),
            ([10.5, 43.4, -93.0], [0.0, 36.0, 0.0], [0.0, -1.0, 0.0])
        );
        let v = |k: &str| b.vars[k];
        assert_eq!(
            [v("scale.x"), v("scale.y"), v("scale.z")],
            [260.0, 300.0, 50.0]
        );
        assert_eq!(
            [v("tint.r"), v("tint.g"), v("tint.b"), v("tint.a")],
            [255.0, 128.0, 64.0, 192.0]
        );
        assert_eq!((v("attached"), v("attach.node")), (1.0, 1.0));
        assert!(!b.vars.contains_key("attach.owner"));
        assert_eq!(
            [
                v("attach.x"),
                v("attach.y"),
                v("attach.z"),
                v("attach.heading")
            ],
            [0.1, 0.2, 0.3, 36.0]
        );
        assert_eq!(
            [v("world.x"), v("world.y"), v("world.z")],
            [10.6, 43.4, -92.9]
        );
        assert!(!b.vars.contains_key("reach"));
        let b2 = at(blob2.corpus);
        assert_eq!(b2.kind, "lava_blob_2");
        assert_eq!(b2.vars["attached"], 0.0);
        assert!(!b2.vars.contains_key("world.x"));
    }

    /// AH.55: the Chompies' clips ride one chain, each Chompy's vector read
    /// slot by slot up to its count, by name per record; a slot holding
    /// something other than a control ends its clips, one whose vector
    /// doesn't read has none, and the hero's clips stay [`ANIMS`]'.
    #[test]
    fn the_chompies_clips_ride_a_chain_of_their_own() {
        let mut f = fixture();
        let notice = (
            "Models/Characters/Enemies/Chompy/Teeth_Notice.ma",
            0x3091_1000,
            100.0,
            100.0,
            0.766_666_7,
            0.5,
        );
        let idle = (
            "Models/Characters/Enemies/Chompy/Teeth_Idle.ma",
            0x3091_2000,
            0.0,
            100.0,
            1.0,
            0.25,
        );
        let control = anim_control(&mut f.w, notice);
        let under = anim_control(&mut f.w, idle);
        let stranger = f.w.object(vtable::ANIMATION_INFO, 0x44);
        let block = f.w.object(vtable::ANIMATION_COMPONENT, 0x28);
        let slots = f.w.alloc(32);
        f.w.put32(block + 0x08, 3);
        f.w.put32(block + 0x10, slots);
        for (k, c) in [control, under, stranger].into_iter().enumerate() {
            f.w.put32(slots + 4 * k as u32, c);
        }
        f.w.put32(f.chompy + 0x7C, block);
        let g = GiantsChains::resolve(&mut f.w, &f.watch, &kinds()).unwrap();
        let spec = &g.chains.slots[&CHOMPY_ANIMS];
        assert_eq!(spec.ranges.len(), 1 + 3 * CHOMPY_CLIP_SLOTS);
        assert!(g.chains.bytes() <= crate::verbs::CHAIN_BUDGET);
        let app = f.w.app;
        let record = |w: &mut World| {
            let mut payload = w.gather(&[(app + 0x54, 0x14)]).unwrap()[0].clone().unwrap();
            for spec in g.chains.slots.values() {
                payload.extend(block_bytes(spec, &evaluate(spec, w).unwrap()));
            }
            payload
        };
        let first = record(&mut f.w);
        f.w.putf(control + 0x14, 0.25);
        f.w.put32(block + 0x08, 1);
        let second = record(&mut f.w);
        // Its vector gone: the Chompy is listed with no clips to read.
        f.w.put32(f.chompy + 0x7C, 0);
        let third = record(&mut f.w);
        let watch = GiantsWatch {
            list: WatchList::new(vec![crate::watch::Field::new("app", app + 0x54, 0x14)]).unwrap(),
            ..f.watch.clone()
        };
        let blocks: Vec<Blocks> = [first, second, third]
            .into_iter()
            .enumerate()
            .map(|(k, payload)| {
                let r = WatchRecord {
                    flip: 10 + k as u64,
                    host_us: 0,
                    missing: 0,
                    read: 0,
                    payload,
                };
                g.chains.split(&watch.list, &r).unwrap().1
            })
            .collect();
        let resolved = g.resolve_names(&mut f.w, &blocks).unwrap();
        assert_eq!(resolved.clip_names[&0x3091_1000], notice.0);
        assert_eq!(resolved.clip_names[&0x3091_2000], idle.0);
        let read: Vec<Option<Vec<(u32, f32)>>> = blocks
            .iter()
            .map(|b| {
                let by_name = GiantsChains::chompy_clips(b);
                assert_eq!(by_name.keys().collect::<Vec<_>>(), ["Enemy_Chompy(1)"]);
                by_name["Enemy_Chompy(1)"]
                    .as_ref()
                    .map(|c| c.iter().map(|c| (c.id, c.remaining)).collect())
            })
            .collect();
        assert_eq!(
            read,
            [
                Some(vec![(0x3091_1000, 0.5), (0x3091_2000, 0.25)]),
                Some(vec![(0x3091_1000, 0.25)]),
                None,
            ]
        );
        // The hero's own clips are untouched by the Chompy's.
        assert_eq!(GiantsChains::clips(&blocks[0]).unwrap().len(), 2);
    }

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
