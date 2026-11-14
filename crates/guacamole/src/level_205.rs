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

use guac_script::{Value, Vm};

use crate::scripts::ScriptWorld;

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

/// Level 205's people (openspec AS.286), by placement name, drawn awake on
/// their own models as the hub's are and never walked. Bowers the mechanic
/// stands by the start (his talk on his own trigger, the talk button within
/// his range); the rest stand past the first fight: Cali at the galleon and
/// in the town, T-Bone, the two Mabu at their hammers, Blobbers at his
/// crane, and the card pirates, whose game a yes is told is quit
/// ([`crate::prompt`]), the gate's pirate among them.
pub const PEOPLE: [&str; 12] = [
    "Bowers",
    "Cali",
    "Cali_Town",
    "T-Bone",
    "Slave_1_hammer(5)",
    "Slave_Carry(5)",
    "Blobbers",
    "CardGame_Template(1)_Bandit",
    "CardGame_Template(4)_Dreadbeard",
    "CardGame_Template(2)",
    "CardGame_Chance",
    "Card_Pirate_04_Gate_Woof",
];

/// Level 205's scenes (openspec AS.286), by placement name, listed with
/// [`PEOPLE`]: the galleon's encounter, the card pirates' six, the hints'
/// pans and the camera pops.
pub const SCENES: [&str; 15] = [
    "205_Encounter_At_Galleon_IGC",
    "CardPirate_Bandit_Cine",
    "Card_Pirate_05_Cine",
    "CardPirate_Boathouse_Cine",
    "CardPirate_Boathouse_Cine_B",
    "CardPirate_Casino_Cine",
    "CardPirate_Casino_Cine_B",
    "Hint_Pan_Show_Island",
    "Hint_Pan_Show_Wheel(1)",
    "Hint_Pan_Show_Turntable",
    "SpecialPan",
    "Switch_To_Cam(6.1)",
    "Switch_To_Cam(6.3)",
    "Switch_To_Cam(6)",
    "SwitchToCam7",
];

/// The placements [`PEOPLE`] lists for the level whose archive is `level`.
pub fn people(level: &str) -> &'static [&'static str] {
    match level {
        LEVEL => &PEOPLE,
        _ => &[],
    }
}

/// The placements [`PEOPLE`] and [`SCENES`] list for the level whose
/// archive is `level`, people first.
pub fn listed(level: &str) -> impl Iterator<Item = &'static str> {
    let scenes: &[&str] = if level == LEVEL { &SCENES } else { &[] };
    people(level).iter().chain(scenes).copied()
}

/// Level 205's end (openspec AS.287), by placement name, listed and never
/// walked. The level ends at Dreadbeard's ([`PEOPLE`]): his scene
/// (`Card_Pirate_05_Cine`, [`SCENES`]) pans to him when Setup_005's fight
/// begins and again once its 17 foes are off, then opens his card game;
/// once his game's won theatrics are over his scene plays his last lines
/// and clones the outro (`205_Outro_IGC`), which takes him, the arch's fade
/// and the landing's planks off and sets the cast down back on the start
/// island: the landing pod brings Flynn and Cali in, Blobbers and Bowers
/// see them off, and its `Game - Level End` names the village and the
/// level end's movie. The outro's own actors (`205_Outro_Landing_Pod`,
/// `205_Outro_Blobbers`, `Outro_Mechanic_Clone`) draw as its clones, their
/// placements unrun, as the intro's are.
pub const END: [&str; 1] = ["205_Outro_IGC"];

/// The placements [`END`] lists for the level whose archive is `level`.
pub fn end(level: &str) -> &'static [&'static str] {
    match level {
        LEVEL => &END,
        _ => &[],
    }
}

/// Level 205's legendary treasure, its ship part (in the boathouse), and
/// where Dreadbeard's scene puts the player before him: the dev entry's
/// `finale` set-downs.
pub const TREASURE: &str = "Legendary_Treasure_Bones_Masthead(2)";
pub const BY_DREADBEARD: &str = "Dreadbeard_Player_Waypoints";

/// Dreadbeard's scene, whose state ([`SCENE_STATE`]) runs the end.
pub const FINALE_SCENE: &str = "Card_Pirate_05_Cine";
/// The scene's state variable, and the states it waits in: on Setup_005's
/// first waves (its controller's `start pan` 2), on its 17 foes off, and on
/// Dreadbeard's game won.
pub const SCENE_STATE: &str = "speakerinit";
pub const ON_THE_FIGHT: i32 = 5;
pub const ON_THE_FOES: i32 = 9;
pub const ON_THE_GAME: i32 = 12;
/// Setup_005's controller and what it sets once its first waves are done.
const FIGHT_CONTROLLER: &str = "Setup_005_Controller";
const FIGHT_STARTED: &str = "start pan";
/// The level's set of Setup_005's foes, all of which his scene waits to see
/// off.
const FIGHT_FOES: &str = "Setup_005_Enemies";
/// Dreadbeard, his game's result and the quit the TV's answer leaves in it
/// (`prompt::quit_card_game`).
pub const DREADBEARD: &str = "CardGame_Template(4)_Dreadbeard";
const GAME_RESULT: &str = "win val";
const GAME_QUIT: i32 = -2;
/// What a won game's theatrics set at their end, which his scene waits on.
pub const GAME_WON: &str = "CardTheatricsDone";

/// What [`finale_stand_in`] made true this step.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct FinaleStep {
    /// Setup_005's first waves done.
    pub fight: bool,
    /// Setup_005's foes taken off, how many.
    pub foes: usize,
    /// Dreadbeard's game won.
    pub game: bool,
}

/// The way to 205's end that isn't run yet, stood in (openspec AS.287; the
/// disc test's and the dev entry's `finale`, stand-in ledger: `dev-entry`),
/// each the step Dreadbeard's scene waits on it: Setup_005's first waves
/// done (its controller's `start pan` 2, which its fight sets: AS.284),
/// its foes taken off as beaten (AS.284), and his card game, which the TV
/// doesn't have (a yes is told it's quit, `prompt::quit_card_game`), won:
/// the quit left as a game still running and the flag his win's theatrics
/// end on set (those theatrics' card and loot screens the TV doesn't
/// have either; AS.315). From there the scene, the outro and the level end
/// are the level's own. On level unit `unit`.
pub fn finale_stand_in(vm: &mut Vm, world: &mut ScriptWorld, unit: u16) -> FinaleStep {
    let mut made = FinaleStep::default();
    let id = |vm: &Vm, world: &ScriptWorld, name: &str| {
        world.placed_in(unit, name).and_then(|h| vm.placement_of(h))
    };
    let Some(scene) = id(vm, world, FINALE_SCENE) else {
        return made;
    };
    let state = vm
        .local(scene, SCENE_STATE)
        .map(|v| vm.value(v))
        .and_then(|v| v.as_f32());
    match state {
        Some(s) if s == ON_THE_FIGHT as f32 => {
            if let Some(c) = id(vm, world, FIGHT_CONTROLLER) {
                made.fight = vm.set_local(c, FIGHT_STARTED, Value::Int(2));
            }
        }
        Some(s) if s == ON_THE_FOES as f32 => {
            made.foes = crate::scripts::take_off_sets(vm, world, &[FIGHT_FOES]);
        }
        Some(s) if s == ON_THE_GAME as f32 => {
            let quit = id(vm, world, DREADBEARD).filter(|d| {
                vm.local(*d, GAME_RESULT).map(|v| vm.value(v)) == Some(Value::Int(GAME_QUIT))
            });
            if let (Some(d), Some(g)) = (quit, vm.program().global_named(GAME_WON)) {
                made.game =
                    vm.set_local(d, GAME_RESULT, Value::Int(0)) && vm.set_global(g, Value::Int(1));
            }
        }
        _ => {}
    }
    made
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

    /// openspec AS.286: its people are on 205's listing and among the TV's
    /// people there, on no other level's.
    #[test]
    fn level_205s_people_are_listed_and_drawn_on_it_alone() {
        let files = Files::default();
        let on = level_config(Mode::Play, &files, 5, LEVEL);
        let off = level_config(Mode::Play, &files, 5, "level_201");
        for name in PEOPLE {
            assert!(on.whitelist.lists(name, "C:/l/Unlisted.ai"), "{name}");
            assert!(
                !off.whitelist.lists(name, "C:/l/Unlisted.ai"),
                "{name} on 201"
            );
            assert!(crate::scripts::is_level_person(LEVEL, name), "{name}");
            assert!(
                !crate::scripts::is_level_person("level_201", name),
                "{name}"
            );
        }
        assert!(people("level_201").is_empty());
    }

    /// openspec AS.287: its outro is on 205's listing and on no other
    /// level's, with Dreadbeard and his scene, which start it.
    #[test]
    fn level_205s_end_is_on_its_listing_alone() {
        let files = Files::default();
        let on = level_config(Mode::Play, &files, 5, LEVEL);
        let off = level_config(Mode::Play, &files, 5, "level_201");
        for name in END {
            assert!(on.whitelist.lists(name, "C:/l/Unlisted.ai"), "{name}");
            assert!(
                !off.whitelist.lists(name, "C:/l/Unlisted.ai"),
                "{name} on 201"
            );
        }
        assert!(on.whitelist.lists(FINALE_SCENE, "C:/l/Unlisted.ai"));
        assert!(on.whitelist.lists(DREADBEARD, "C:/l/Unlisted.ai"));
        assert!(end("level_201").is_empty());
    }
}
