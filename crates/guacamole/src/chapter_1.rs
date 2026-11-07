//! Chapter 1 past its first fight (openspec AU.33): the level's way from the
//! cave to its end, by placement name, as [`crate::scripts::ENCOUNTERS`] and
//! [`crate::scripts::PASSAGES`] name level 214's.
//!
//! The way, as its scripts have it:
//! - **The first cave.** The hero comes in by the pit's interior. `213
//!   gigantus igc` frees the Giant, who stands by the cracked floor. Near him
//!   he says his line, smashes through it and is gone; the hero walks round
//!   by the lower ledge and falls in, and the pit's interior carries him to
//!   the second cave.
//! - **The second cave.** Setup_002's twenty Chompies come out of two mouths
//!   in the arena's walls, then its Arkeyan; the gate opens on the jump pads
//!   up to its exit, which carries him to the vista over the quarry.
//! - **The quarry.** The vista's cutscene, Setup_003's Chompies, three posts
//!   whose chains hold the Arkeyan robot: each hit until it gives, each
//!   giving by a cutscene of its own, the last waking the level's end
//!   (`Temp_Level_Ender`, [`crate::scripts::LEVEL_END_SCRIPTS`]).

use crate::scripts::Encounter;

/// What fights on chapter 1 past the pit's first fight
/// ([`crate::scripts::FIRST_FIGHT`]), walked as creatures.
pub const WAY: Encounter = Encounter {
    level: "level_213_tutotial",
    placements: &[
        "Enemy_Chompy(11)",
        "Enemy_Chompy(12)",
        "Enemy_Chompy(13)",
        "Enemy_Chompy(14)",
        "Enemy_Chompy(15)",
        "Enemy_Chompy(16)",
        "Enemy_Chompy(17)",
        "Enemy_Chompy(18)",
        "Enemy_Chompy(19)",
        "Enemy_Chompy(20)",
        "Enemy_Chompy(21)",
        "Enemy_Chompy(22)",
        "Enemy_Chompy(23)",
        "Enemy_Chompy(24)",
        "Enemy_Chompy(25)",
        "Enemy_Chompy(26)",
        "Enemy_Chompy(27)",
        "Enemy_Chompy(28)",
        "Enemy_Chompy(29)",
        "Enemy_Chompy(30)",
        "Enemy_ChompySpawner(1)",
        "Secret_pit_Chompie",
        "PandoranGruntSimple(1)",
        "Enemy_Chompy(8)",
        "Enemy_Chompy(9)",
        "Enemy_Chompy(10)",
        "PandoranGruntSimple(2)",
        "PandoranGruntSimple(3)",
        "PandoranGruntSimple(5)",
        "PandoranGruntSimple(6)",
        "PandoranGruntSimple(7)",
        "Enemy_Chompy_Undead(1)",
        "Enemy_Chompy_Undead(2)",
        "Enemy_Chompy_Undead(3)",
    ],
    clones: &[],
    steered: &[],
};

/// What takes the way on past them, listed and never walked.
pub const PASSAGE: [&str; 66] = [
    "213_Gigantus_IGC",
    "2.NPC_Norticus_First_Cave",
    "1.NPC_Giant",
    "Titan_Floor(Template)(1)",
    "Titan_Floor(Template)(2)",
    "Titan_Floor_Controller",
    "1.NPC_Snuckels",
    "Longer Tether",
    "Tether_Extend",
    "Transition_35-38",
    "Cave_Dust_Controller",
    "Dummy_Boulder",
    "Setup_002_Controller",
    "Chompie_Monitor",
    "PvP_JumpPad_NORMAL(1)",
    "PvP_JumpPad_NORMAL(2)",
    "PvP_JumpPad_NORMAL(3)",
    "PvP_JumpPad_NORMAL(4)",
    "Extend_Tether_Eon Hint",
    "3.NPC_Norticus",
    "Setup_Life_Controller",
    "3.NPC_Rizzo_Life_Elemental_Area",
    "ElementalWallController(2)",
    "Secret Pit Monitor",
    "213_First_Vista_IGC",
    "213_First_Vista_Laser_Target",
    "213_Vista_P1_WP",
    "213_Vista_P2_WP",
    "Normal_Tether",
    "4.NPC_Norticus at end",
    "213_Mabu_Hammer_Controller",
    "213_Mabu_Hammer_Controller_02",
    "Setup_003_Controller",
    "Setup_003_Trigger(1)",
    "Plank_1",
    "Plank_2",
    "Plank_3_railing",
    "Railing",
    "hand_Gesture_Check",
    "Rope_around_Post",
    "Rope_around_Post(1)",
    "Rope_around_Post(2)",
    "Chain 1 Explode",
    "Chain 2 Explode",
    "Chain 3 Explode",
    "Titan_Chain_1",
    "Titan_Chain_2",
    "Titan_Chain_3",
    "Robot_Collision_Volumer",
    "Exploding_Rocks",
    "Setup_004_Controller",
    "Setup_004_Trigger(1)",
    "Laser_emmitter_0",
    "Laser_emmitter_1",
    "Robot_Platform",
    "Arkeyan_Bot",
    "213_Cheering_Mabu_Controller",
    "PvP_JumpPad_NORMAL(5)",
    "PvP_JumpPad_NORMAL(6)",
    "PvP_JumpPad_NORMAL(11)",
    "Activate_JumpPad",
    "BambooPlatform_Destructible(1)",
    "BambooPlatform_Overhead",
    "Laser at End",
    "Setup_005_Controller",
    "Setup_005_Trigger(1)",
];

/// The placements [`PASSAGE`] lists for the level whose archive is `level`.
pub fn passage(level: &str) -> &'static [&'static str] {
    match level {
        "level_213_tutotial" => &PASSAGE,
        _ => &[],
    }
}
