//! Chapter 1 past its first fight (openspec AU.33): the level's way from the
//! cave to its end, by placement name, as [`crate::scripts::ENCOUNTERS`] and
//! [`crate::scripts::PASSAGES`] name level 214's.
//!
//! The way, as its scripts have it:
//! - **The first cave.** The hero comes in by the pit's interior. `213
//!   gigantus igc` plays as he arrives, Norticus and two Mabu at work in it
//!   on their own scripts, and frees the Giant, who stands by the cracked
//!   floor. Near him
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
//! - **Off the way, the elemental areas** (openspec AS.153), each shut by
//!   its wall until a hero of its element comes
//!   ([`crate::scripts::ELEMENTAL_SCRIPTS`]): the air area under the ledge by
//!   the vista, dropped into, whose pads lift a hero to its upper ground and
//!   back to the ledge; the death area by the quarry's fights and its spike
//!   floor; the life area past the second cave and its three Chompies.

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
        "Enemy_Chompy_Undead(4)",
        "Enemy_Chompy_Undead(5)",
        "Enemy_Chompy_Undead(6)",
    ],
    clones: &[],
    steered: &[],
};

/// What takes the way on past them, listed and never walked.
pub const PASSAGE: [&str; 76] = [
    "213_Gigantus_IGC",
    // The first cave's scene's people (openspec AS.105): Norticus in his
    // turban, a Mabu hammering and one carrying a brick, each its own
    // script's (the hat, the hammer, the brick, their clips), and the two
    // at work there the scene takes off at its start and puts back at its
    // end, turning to the hero as he comes by; drawn with `2.NPC_Norticus_
    // First_Cave` as the level's people ([`crate::scripts::LEVEL_PEOPLE`]).
    "213_Gigantus_Norticus",
    "213_Gigantus_Slave_hammer",
    "213_Gigantus_Slave_Carry",
    "Slave_1_hammer(3)",
    "Slave_Carry(3)",
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
    "PvP_JumpPad_NORMAL(7)",
    "PvP_JumpPad_NORMAL(8)",
    "PvP_JumpPad_NORMAL(10)",
    "Spinner",
    "Spike_Floor_Spawner(1)",
];

/// The placements [`PASSAGE`] lists for the level whose archive is `level`.
pub fn passage(level: &str) -> &'static [&'static str] {
    match level {
        "level_213_tutotial" => &PASSAGE,
        _ => &[],
    }
}

#[cfg(test)]
mod tests {
    use guac_script::Mode;

    use super::*;
    use crate::scripts::{Files, encounters, level_config};

    /// Its way is chapter 1's alone: another level's placements so named
    /// (level 214 holds an `Enemy_Chompy(11)`) stay as that level has them.
    #[test]
    fn chapter_1s_way_is_on_its_listing_alone() {
        let files = Files::default();
        let on = level_config(Mode::Play, &files, 5, WAY.level);
        let off = level_config(Mode::Play, &files, 5, "level_200");
        for name in WAY.placements.iter().chain(&PASSAGE) {
            assert!(on.whitelist.lists(name, "C:/l/Unlisted.ai"), "{name}");
            assert!(!off.whitelist.lists(name, "C:/l/Unlisted.ai"), "{name}");
        }
        assert_eq!(encounters(WAY.level).count(), 1);
        assert_eq!(encounters("level_200").count(), 0);
        assert!(passage("level_200").is_empty());
    }

    /// What fights is walked and what takes the way on never is: a jump pad
    /// the engine walked stood asleep on its body (AS.59's rule for 214's).
    #[test]
    fn what_takes_the_way_on_is_never_walked() {
        for name in PASSAGE {
            assert!(!WAY.placements.contains(&name), "{name}");
        }
        for pad in ["PvP_JumpPad_NORMAL(1)", "PvP_JumpPad_NORMAL(6)"] {
            assert!(PASSAGE.contains(&pad), "{pad}");
        }
        for foe in ["Enemy_Chompy(11)", "PandoranGruntSimple(1)"] {
            assert!(WAY.placements.contains(&foe), "{foe}");
        }
    }
}
