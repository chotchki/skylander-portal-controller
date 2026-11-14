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
//! Left off: the level controller (`205_Level_Controller.ai`), a debug
//! warp through its own waypoints while two buttons are held near it, no
//! part of the way; and the Giant Pirate Head's script (`GiantPirateHead`),
//! which fades the arch as the camera nears it (openspec AS.288's).

/// The placements the way off the island needs, by name, listed and never
/// walked: the crane, its mast, the wheel that turns them and the hint
/// that pans to the wheel. The crane's co-op half (a second player inside
/// its basket's checker as it turns is warped to its waypoint) runs with
/// it.
pub const PASSAGE: [&str; 4] = ["Crane(1)", "Crane_Head", "SpinCrank", "Hint_Pan_Show_Wheel"];

/// The archive the story reaches after 201 (openspec AS.281).
pub const LEVEL: &str = "level_205_piratecards";

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
    use crate::scripts::{Files, level_config};

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
