//! Where the dev entry and the headless fight take the hero (openspec AH.25,
//! AH design decision 17): AH.8's route from the strip down to anchor B as
//! a scripted hero's set-downs, the state at anchor B that the set-downs
//! don't make, set from AH.4's reads there, and decision 17's equivalence
//! list read off a run, to hold against the oracle's anchor before anything
//! is compared. Every number here is a reading off the oracle's recordings
//! (`ah8-route-1792539821/route_b`), not game bytes.
//!
//! The set-downs fire the route's triggers in AH.8's order, one each, so
//! each wakes what it woke there: the camera volumes on the way to the
//! strip ([`TO_STRIP`]); within 16 ft of `Setup_001_Trigger`
//! (Setup_001's controller sets the four Chompies' ranges the next frame);
//! 122 steps later within 23 ft of `Activate_Norticus and Chompies` (it
//! takes the far-off Norticus and Chompies off and wakes the spike gate and
//! Norticus); on the scaffold at the pit's rim, 26 ft above
//! `Enemy_Chompy(1)` (nothing changes); landed on the floor, inside the
//! controller's 7 ft rule (the intro, the four hurtable); then anchor B.
//! What the original's route left that they don't (the intro cutscene
//! walked `Enemy_Chompy(4)` round its path; the route broke nine
//! breakables and took 17 gold) is set from AH.4's reads at anchor B
//! ([`ANCHOR_B`], [`ANCHOR_B_LOOT`], the combat manager's phase
//! [`FIRST_ALERT`]) or left for the equivalence list to record (the
//! figure's gold, the save's 1.1 multiplier: design decision 12).

use bevy::math::Vec3;
use guac_script::{Value, Vm};
use serde::Serialize;

use crate::scripts::{FIRST_FIGHT, Rarity, ScriptWorld, loot_index};

/// AH.8's route from the strip to anchor B as a scripted hero's set-downs
/// (`route-record.json`, its steps' end points): the step each is made at
/// (from the route's start), where his pivot goes (metres) and his facing
/// (degrees). 16 ft from `Setup_001_Trigger` and 36 ft from the Norticus
/// trigger where the trigger fired (step 106's end, `s001`); 23 ft from the
/// Norticus trigger 122 steps later (`activate`, where it fired 122 game
/// frames after); on the scaffold at the pit's rim, 26 ft above
/// `Enemy_Chompy(1)` (`scaffold`); landed on the pit floor (`cutscene`,
/// where the 7 ft rule had passed); anchor B (`anchor`, 31.7 ft from
/// `Enemy_Chompy(2)`), as the headless runs calibrated against AH.10 set
/// him down (the dev entry sets him at [`ANCHOR_B_POSE`] instead).
pub const ROUTE: [(u32, [f32; 3], f32); 5] = [
    (0, [31.75, 44.06, -5.55], 270.0),
    (122, [27.76, 43.06, -5.04], 250.0),
    (180, [10.67, 30.35, -1.93], 200.0),
    (300, [7.27, 22.39, -7.56], 200.0),
    (360, [5.445, 22.393, -12.023], 202.28),
];

/// Anchor B's own pivot and facing as its read holds them
/// (`route-record.json`'s anchor): the dev entry's last set-down, so
/// decision 17's pose holds to AF.12's floors (0.1 m, 1e-4°). [`ROUTE`]'s
/// last, rounded, stays the headless runs' (half a millimetre and 0.001°
/// off), which their AH.10 numbers were taken on.
pub const ANCHOR_B_POSE: ([f32; 3], f32) = ([5.4454904, 22.392666, -12.022571], 202.27884);

/// Norticus' scene (`cutscene go`) ends 5 s on in the original (AH.10's 300
/// frames), set off the whitelist; until AH.56 runs what ends it, the
/// headless runs and the dev entry set `Cutscene State` back to 0 that many
/// steps into it, which frees the hero and walks Norticus into the cave.
pub const SCENE_STEPS: u32 = 300;
/// The global Norticus' scene waits on.
pub const CUTSCENE_STATE: &str = "Cutscene State";
/// Norticus, whose scene that is.
pub const NORTICUS: &str = "1.NPC_Norticus_First_Encounter";

/// The triggers AH.8's route fired on its way to the strip, as set-downs a
/// step apart in the order it fired them, ahead of [`ROUTE`] in the dev
/// entry: where the route's hero stood when he first came within each one's
/// range (`route.json`'s looks against the placements' ranges, AH.4's rule
/// for a trigger's name): `Transition 7-1` (flip 50, 59 ft of its 60),
/// `Transition 2-8` (flip 1,277, on the walkway), `Pop_to_Camera_5` (flip
/// 2,355, by its west drop), `Transition 5-3` (flip 2,379) and `Transition
/// 3-6` (flip 4,883, on the strip). Camera volumes all: nothing the fight
/// runs reads them, the equivalence list does. `Transition 1-2` fires at
/// the level's start as the route's did; `trg_lvl_213_lush_peak` had fired
/// before the route began and its hero never came within its 300 ft, so no
/// set-down fires it.
pub const TO_STRIP: [(u32, [f32; 3], f32); 5] = [
    (0, [15.510332, 43.489338, -91.62676], 71.64656),
    (10, [14.3095, 55.581364, -77.74613], 267.51846),
    (20, [-5.9015956, 56.90614, -62.63886], 326.5737),
    (30, [-7.5078025, 56.087265, -60.21129], 326.69798),
    (40, [28.015396, 45.927345, -25.662092], 135.61743),
];

/// The step the dev entry makes [`ROUTE`]'s step 0 at, past [`TO_STRIP`].
pub const STRIP_AT: u32 = 60;

/// The creatures where anchor B has them (`actors-anchor.json`): AH.8's
/// route ran the intro cutscene, which guacamole doesn't, so the fourth
/// Chompy's walk round its path is somewhere else when the hero lands.
pub const ANCHOR_B: [(&str, [f32; 3], f32); 5] = [
    ("Enemy_Chompy(1)", [4.5213137, 22.374975, -23.117332], 243.0),
    (
        "Enemy_Chompy(2)",
        [1.8390902, 22.380882, -20.97726],
        178.00002,
    ),
    (
        "Enemy_Chompy(3)",
        [-0.13646065, 22.384933, -21.690493],
        146.0,
    ),
    (
        "Enemy_Chompy(4)",
        [3.6175919, 22.28899, -27.261147],
        258.4939,
    ),
    (
        "1.NPC_Norticus_First_Encounter",
        [2.0715888, 22.338335, -23.759197],
        0.0,
    ),
];

/// The step AH.8's opening starts at, AH.10's F0: 120 steps after the hero
/// lands at anchor B's pose (the oracle's eye settled 120 flips there), the
/// creatures set there again at it.
pub const OPENING: u32 = 480;

/// The combat manager's first alert from F0, steps: anchor B's state holds
/// the 0.25 s check's phase, AH.10's first alert landing at F0 + 24 (or 25,
/// by F0's own frame time).
pub const FIRST_ALERT: u32 = 24;

/// The level's loot counts at anchor B (`fight-anchor.json`, AH.4's read
/// of `LootData`): the destructibles' common, uncommon and rare running
/// counts after the nine breakables AH.8's route broke.
pub const ANCHOR_B_LOOT: [f32; 3] = [0.6242796, 0.09749699, 0.5707676];

/// The way the oracle's view looked at anchor B (AH.8's last look:
/// `route.json`, degrees, as a heading), from camera 5, which
/// `Pop_to_Camera_5` brought in on the way down: the dev entry's follow
/// camera rests on camera 0's definition looking along it there, until the
/// level's camera triggers run (AG.28; stand-in ledger: `camera`).
pub const ANCHOR_B_VIEW: f32 = 113.79292;

/// AH.8's reads at anchor B for the equivalence list's numbers
/// (`fight-anchor.json`): the oracle's figure's experience and gold.
pub const ANCHOR_B_EXPERIENCE: u32 = 0;
pub const ANCHOR_B_GOLD: u32 = 17;

/// The triggers fired at anchor B, in their names' order
/// (`route-record.json`'s anchor, AH.4's rule: a range trigger has fired
/// once it has been active).
pub const ANCHOR_B_FIRED: [&str; 9] = [
    "Activate_Norticus and Chompies",
    "Pop_to_Camera_5",
    "Setup_001_Trigger",
    "Transition 1-2",
    "Transition 2-8",
    "Transition 3-6",
    "Transition 5-3",
    "Transition 7-1",
    "trg_lvl_213_lush_peak",
];

/// A placement's behaviour and alerted flag as decision 17's list holds
/// them.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct PlacementState {
    pub name: String,
    /// None for one whose script hasn't started or doesn't run.
    pub behaviour: Option<String>,
    /// `description.attention parameters.alerted`, where it has one.
    pub alerted: Option<i32>,
}

/// Design decision 17's equivalence list, read off a run: what's held
/// against the oracle's reads at an anchor before any event is compared.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Equivalence {
    /// His pivot (metres) and facing (degrees).
    pub hero: Option<([f32; 3], f32)>,
    /// His figure's experience and gold as its words hold them.
    pub experience: Option<u32>,
    pub gold: Option<u32>,
    /// Each of the first fight's placements ([`FIRST_FIGHT`]), in its order.
    pub placements: Vec<PlacementState>,
    /// The level's triggers that have fired, by name in order: each a level
    /// placement with no script and no model and a range, woken by the
    /// activity rule at least once ([`ScriptWorld::fired`]).
    pub triggers_fired: Vec<String>,
    /// The destructibles' loot counts: common, uncommon, rare.
    pub loot: [Option<f32>; 3],
}

/// The list off `vm` and `world`, the level's file linked as `level`: the
/// hero's pose and his figure's experience and gold as the caller reads
/// them (the engine's, the figure's words).
pub fn equivalence(
    vm: &Vm,
    world: &ScriptWorld,
    level: u16,
    hero: Option<(Vec3, f32)>,
    figure: Option<(u32, u32)>,
) -> Equivalence {
    let placements = FIRST_FIGHT
        .iter()
        .map(|name| {
            let id = world
                .placed_in(level, name)
                .and_then(|h| vm.placement_of(h));
            let path = |fields: &[&str]| -> Option<Value> {
                let mut v = vm.local(id?, "description").map(|v| vm.value(v))?;
                for f in fields {
                    v = vm.field(v, f).map(|x| vm.value(x))?;
                }
                Some(v)
            };
            let alerted = match path(&["attention parameters", "alerted"]) {
                Some(Value::Int(i)) => Some(i),
                Some(Value::Float(f)) => Some(f as i32),
                _ => None,
            };
            PlacementState {
                name: (*name).to_owned(),
                behaviour: id.and_then(|id| vm.behaviour(id)).map(str::to_owned),
                alerted,
            }
        })
        .collect();
    let mut triggers_fired = world.fired_triggers(level);
    triggers_fired.sort();
    Equivalence {
        hero: hero.map(|(p, f)| (p.to_array(), f.rem_euclid(360.0))),
        experience: figure.map(|f| f.0),
        gold: figure.map(|f| f.1),
        placements,
        triggers_fired,
        loot: [Rarity::Common, Rarity::Uncommon, Rarity::Rare].map(|r| loot_index(vm, world, r)),
    }
}
