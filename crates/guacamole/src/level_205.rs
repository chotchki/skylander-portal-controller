//! Level 205's way off its start island (openspec AS.283), as
//! [`crate::level_201`] lists 201's: what of the level's own scripts it
//! needs, by placement name.
//!
//! The way, as its scripts have it:
//! - **The crane's boat.** The island ends in the Giant Pirate Head's stage.
//!   Past its rim a crane's boat platform (`Crane(1)`, its mast
//!   `Crane_Head`) lies swung east (facing 90) across the gap to the far
//!   pier, its deck a box solid to actors and an actor surface (openspec
//!   AS.168), so a hero walks off the stage over it and onto the pier by the
//!   crane's raised deck.
//! - **The wheel's pan.** Down the pier to its south end a hint
//!   (`Hint_Pan_Show_Wheel`, woken within 8 m of its waypoint) pans to the
//!   wheel on the crane's deck and takes itself off: the pier stops short of
//!   the first fight's yard.
//! - **The wheel.** On the deck the wheel (`SpinCrank`) offers its talk
//!   button (△, action 4) within its range; a press tells the crane and its
//!   mast to turn, and both swing 90° over 4 s to facing 180, the boat now
//!   lying south from the pier's end to the yard. Another press swings them
//!   back. The first turn also takes the hint off if it hasn't run.
//! - **The yard.** Down the pier and over the boat to Setup_001's yard,
//!   where the level's first fight waits (openspec AS.284).
//!
//! The fights (openspec AS.284), setup by setup, each a controller that
//! wakes its enemies by its own triggers and takes itself off once they're
//! beaten:
//! - **Setup_001, the first yard.** Once the crane has turned (or the
//!   wheel's hint is gone) its enemies know he's coming; at its trigger by
//!   the controller the yard's gate goes up, a Cyclops Near steps out of the
//!   house by the water (its door swinging open and shut) and its enemy
//!   intro plays, the Cyclops' card the first time; a second walks out of
//!   the house up the slope, and two lobbers wait on the ledges. The first
//!   Cyclops beaten, the gate goes; all five, the controller goes.
//! - **Setup_002** waits for the key by its trigger, its Chompies and the
//!   armored Chompy's intro (`IGC_Armored_Chompy`), its lobbers and its
//!   heavy elf; its helper sits in the far interior.
//! - **Setup_003** by the galleon: its heavy elves, Cyclopes and lobbers in
//!   parts, the house's door and Cali's scene among them.
//! - **Setup_004** behind its gate: a Biclops' intro (`IGC_Cyclops_Heavy`)
//!   once a hero is within 160 ft of it, then its Biclopes, Cyclopes, heavy
//!   elf and lobbers out of its two doors.
//! - **Setup_005** by the town: its Biclopes, Cyclopes, heavy elves,
//!   lobbers and Chompies.
//!
//! Left off: the level controller (`205_Level_Controller.ai`), a debug
//! warp through its own waypoints while two buttons are held near it, no
//! part of the way; and the Giant Pirate Head's script (`GiantPirateHead`),
//! which fades the arch as the camera nears it (openspec AS.288's).

use crate::scripts::Encounter;

/// The placements the way off the island needs, by name, listed and never
/// walked: the crane, its mast, the wheel that turns them and the hint
/// that pans to the wheel. The crane's co-op half (a second player inside
/// its basket's checker as it turns is warped to its waypoint) runs with
/// it.
pub const PASSAGE: [&str; 4] = ["Crane(1)", "Crane_Head", "SpinCrank", "Hint_Pan_Show_Wheel"];

/// The archive the story reaches after 201 (openspec AS.281).
pub const LEVEL: &str = "level_205_piratecards";

/// What fights on level 205 (openspec AS.284), walked as creatures: the
/// five setups' controllers (Setup_002's helper with them), every enemy
/// their sets name (`Setup_001` to `Setup_005_Enemies` and their parts) and
/// the armored Chompy's intro, by name, setup by setup.
pub const WAY: Encounter = Encounter {
    level: LEVEL,
    placements: &[
        // Setup_001, the first yard past the crane.
        "Setup_001_Controller",
        "Enemy_Cyclops_Near(70)",
        "Enemy_Cyclops_Near(20)",
        "Enemy_Cyclops_Near(21)",
        "Enemy_Elf_Lobber(1)",
        "Enemy_Elf_Lobber(2)",
        // Setup_002.
        "Setup_002_Controller",
        "Enemy_Placements_Setup_002_Controller_Helper",
        "IGC_Armored_Chompy",
        "Enemy_Chompy(1)",
        "Enemy_Chompy(2)",
        "Enemy_Chompy(3)",
        "Enemy_Chompy(4)",
        "Enemy_Chompy(5)",
        "Enemy_Chompy(6)",
        "Enemy_Chompy(7)",
        "Enemy_Chompy(8)",
        "Enemy_Chompy(9)",
        "Enemy_Chompy(10)",
        "Enemy_Chompy(11)",
        "Enemy_Chompy(12)",
        "Enemy_Chompy(13)",
        "Enemy_Chompy(14)",
        "Enemy_Chompy(15)",
        "Enemy_Chompy(16)",
        "Enemy_Elf_Heavy_S2(1)",
        "Enemy_Elf_Lobber(3)",
        "Enemy_Elf_Lobber(4)",
        // Setup_003.
        "Setup_003_Controller",
        "Enemy_Elf_Heavy_S2(2)",
        "Enemy_Elf_Heavy_S2(3)",
        "Enemy_Elf_Heavy_S2(4)",
        "Enemy_Cyclops_Near(1)",
        "Enemy_Cyclops_Near(2)",
        "Enemy_Cyclops_Near(3)",
        "Enemy_Cyclops_Near(4)",
        "Enemy_Cyclops_Near(5)",
        "Enemy_Cyclops_Near(6)",
        "Enemy_Cyclops_Near(7)",
        "Enemy_Cyclops_Near(8)",
        "Enemy_Cyclops_Near(9)",
        "Enemy_Elf_Lobber(5)",
        "Enemy_Elf_Lobber(6)",
        "Enemy_Elf_Lobber(7)",
        // Setup_004.
        "Setup_004_Controller",
        "Enemy_Biclops_Heavy_S2(1)",
        "Enemy_Biclops_Heavy_S2(2)",
        "Enemy_Biclops_Heavy_S2(3)",
        "Enemy_Cyclops_Near(13)",
        "Enemy_Cyclops_Near(14)",
        "Enemy_Cyclops_Near(15)",
        "Enemy_Cyclops_Near(16)",
        "Enemy_Cyclops_Near(17)",
        "Enemy_Cyclops_Near(18)",
        "Enemy_Cyclops_Near(19)",
        "Enemy_Cyclops_Near(22)",
        "Enemy_Elf_Heavy_S2(5)",
        "Enemy_Elf_Lobber(8)",
        "Enemy_Elf_Lobber(9)",
        "Enemy_Elf_Lobber(15)",
        "Enemy_Elf_Lobber(16)",
        // Setup_005.
        "Setup_005_Controller",
        "Enemy_Biclops_Heavy_S2(6)",
        "Enemy_Biclops_Heavy_S2(7)",
        "Enemy_Cyclops_Near(10)",
        "Enemy_Cyclops_Near(11)",
        "Enemy_Cyclops_Near(12)",
        "Enemy_Cyclops_Near(23)",
        "Enemy_Cyclops_Near(24)",
        "Enemy_Cyclops_Near(25)",
        "Enemy_Cyclops_Near(26)",
        "Enemy_Elf_Heavy_S2(6)",
        "Enemy_Elf_Heavy_S2(7)",
        "Enemy_Elf_Lobber(11)",
        "Enemy_Elf_Lobber(12)",
        "Enemy_Elf_Lobber(13)",
        "Enemy_Elf_Lobber(14)",
        "Enemy_Elf_Lobber(17)",
        "Enemy_Elf_Lobber(18)",
        "Enemy_Chompy(17)",
        "Enemy_Chompy(18)",
        "Enemy_Chompy(19)",
        "Enemy_Chompy(20)",
        "Enemy_Chompy(21)",
        "Enemy_Chompy(22)",
        "Enemy_Chompy(23)",
        "Enemy_Chompy(24)",
        "Enemy_Chompy(25)",
    ],
    // The Biclops' smoke as its teleport leaves it.
    clones: &["Biclops_smoke_dummy"],
    steered: &[],
};

/// The placements [`PASSAGE`] lists for the level whose archive is `level`.
pub fn passage(level: &str) -> &'static [&'static str] {
    match level {
        LEVEL => &PASSAGE,
        _ => &[],
    }
}

#[cfg(test)]
mod tests {
    use guac_script::Mode;

    use super::*;
    use crate::scripts::{Files, encounters, level_config};

    /// openspec AS.284: its fights are on 205's listing and walked as its
    /// encounter, on no other level's (201 names its own `Enemy_Chompy(1)`
    /// and lobbers, so a level of its own is the one to check against), its
    /// five setups' controllers among them.
    #[test]
    fn level_205s_fights_are_on_its_listing_alone() {
        let files = Files::default();
        let on = level_config(Mode::Play, &files, 5, LEVEL);
        let off = level_config(Mode::Play, &files, 5, "level_202");
        for name in WAY.placements.iter().chain(WAY.clones) {
            assert!(on.whitelist.lists(name, "C:/l/Unlisted.ai"), "{name}");
            assert!(
                !off.whitelist.lists(name, "C:/l/Unlisted.ai"),
                "{name} on 202"
            );
        }
        for n in 1..=5 {
            let c = format!("Setup_00{n}_Controller");
            assert!(WAY.placements.contains(&c.as_str()), "{c}");
        }
        assert!(encounters(LEVEL).any(|e| *e == WAY));
        assert_eq!(encounters("level_202").count(), 0);
        for name in PASSAGE {
            assert!(!WAY.placements.contains(&name), "{name}");
        }
    }

    /// Its way off the island is on 205's listing and on no other level's
    /// (201 places no crane), the level controller and the arch's fade
    /// never.
    #[test]
    fn level_205s_way_off_its_island_is_on_its_listing_alone() {
        let files = Files::default();
        let on = level_config(Mode::Play, &files, 5, LEVEL);
        let off = level_config(Mode::Play, &files, 5, "level_201");
        for name in PASSAGE {
            assert!(on.whitelist.lists(name, "C:/l/Unlisted.ai"), "{name}");
            assert!(
                !off.whitelist.lists(name, "C:/l/Unlisted.ai"),
                "{name} on 201"
            );
        }
        let controller = "C:/tfb/Content/Levels/Level_205/Scripts/205_Level_Controller.ai";
        assert!(!on.whitelist.lists("_LevelController", controller));
        let arch = "C:/tfb/Content/Levels/Level_205/Scripts/GiantPirateHead.ai";
        assert!(!on.whitelist.lists("GiantPirateHead", arch));
        assert!(passage("level_201").is_empty());
    }
}
