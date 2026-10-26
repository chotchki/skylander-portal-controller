//! The fight's script data off the dump (openspec AH.22, design decision 7):
//! the whitelist (decision 2) as AH.12 counted it, and two extract-wide
//! parts its scripts need outside a level. Writing them is the script
//! stage's ([`crate::stages`]); a level's own entry and its creatures'
//! palettes are level parts ([`crate::levels`]).
//!
//! - The scripts: the decompressed `level.bld` of `bootstrap.bld`,
//!   `global.bld`, `init_setup.bld` and each fighting character's archive,
//!   copied raw, `scripts/<archive>.igz`. A script refers to placements,
//!   structures and sets in its own file, so a subset of one isn't
//!   self-contained: whole entries it is, parsed at level load by
//!   `guac-formats` (decision 7's departure from "tfbScript graphs → IR").
//!   Each is checked to parse and its op graph to read before it's written.
//! - The spawn models: every model a placement names in `global.bld` and in
//!   each fighting character's archive (his own body aside, which the model
//!   stage writes), read as a level's props are,
//!   `spawns/<archive>.<ron|postcard>`: the lava ball, the level-up's rings,
//!   the death stars. A level's own spawn templates draw from its props.
//!
//! - The walkers' own entries (openspec AG.27): every modelled character's
//!   archive beside [`FIGHTERS`]', so each runs on his own scripts. A
//!   walker's entry that is missing or won't read is left out of the part,
//!   listed with why in [`WALKERS_FILE`] ([`WalkerList`]), and that
//!   character keeps his card; the others and the fighters' land.
//!
//! Each part lands whole or not at all: its files are loaded together. One
//! that doesn't read is the stage's failure for that part alone, and the
//! next start tries it again.

use std::collections::BTreeMap;
use std::path::Path;

use guac_formats::iga::Archive;
use guac_formats::igz::Igz;
use guac_formats::level::Placements;
use guac_formats::model as fmodel;
use guac_formats::script::Graph;
use guac_ir::DumpIdentity;
use guac_ir::level::{Prop, RefusedProp};
use guac_ir::spawn::SpawnModels;

use crate::extract::Manifest;
use crate::levels::Read;
use crate::models::{self, Textures};
use crate::roster::{BOOTSTRAP, GLOBAL, INIT_SETUP, LEVEL};

/// The parts' names in the manifest, which [`complete`] asks for.
pub const SCRIPTS: &str = "scripts";
pub const SPAWNS: &str = "spawns";
/// Where each part's files go.
pub const SCRIPT_DIR: &str = "scripts";
pub const SPAWN_DIR: &str = "spawns";

/// The characters whose attack scripts run, by load file (design decision
/// 15; Open question 3, answered 2026-10-19: Eruptor alone). They key the
/// spawns part; the walkers ([`walker_loads`]) are a longer list that runs
/// movement, never attacks (AH.29).
pub const FIGHTERS: [&str; 1] = ["009_Eruptor"];

/// Which walkers the scripts part holds (openspec AG.27), as a file of the
/// part (`scripts/walkers.json`, [`WalkerList`]): its being recorded is how
/// a start tells a part made before the walkers from a current one.
pub const WALKERS_FILE: &str = "scripts/walkers.json";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Whitelisted {
    /// The archive under `PS3_GAME/USRDIR`.
    pub archive: &'static str,
    /// Each script by its source path's file name, any case.
    pub scripts: &'static [&'static str],
}

/// The whitelist (design decision 2): AH.12's roots and the scripts they
/// spawn, by archive, with AG.60's start scripts on the props and theirs,
/// AG.28's camera volumes and activator, AG.177's jumping goats, AG.162's
/// 66 walkers' own and AH.42's from AH.8's route.
pub const WHITELIST: [Whitelisted; 71] = [
    Whitelisted {
        archive: BOOTSTRAP,
        // The empty base every update callback specializes.
        scripts: &["Boot_Halt_LoadLevel.ai"],
    },
    Whitelisted {
        archive: GLOBAL,
        // The 17 placements running at a level's start (their STARTUP and
        // first behaviour; the portal manager's and the mover update's
        // ticks), then what the fight spawns from here.
        scripts: &[
            "360_PS3_pointer_setup.ai",
            "360PS3GestureManager.ai",
            "GameSystems.ai",
            "Macros_Achievements.ai",
            "Macros_Animation.ai",
            "Minion_Tuning.ai",
            "movementUpdate.ai",
            "MusicController.ai",
            "PauseController.ai",
            "PointLightManager.ai",
            "PortalControl.ai",
            "PortalManager.ai",
            "PS3SpecificSetup.ai",
            "SoundController.ai",
            "Spyro_PortalManager.ai",
            "TRG_Checks_Manager.ai",
            "UI.ai",
            "LevelUp.ai",
            "MinionDeathActor.ai",
            "ShakeSpriteMaster.ai",
            "FX_StunnedStars.ai",
            "UI_Controller_Prompt.ai",
        ],
    },
    Whitelisted {
        archive: INIT_SETUP,
        // The first tick on a PS3.
        scripts: &[
            "Init_Setup_Plat_PS3.ai",
            "Init_Setup.ai",
            "InitSetup_Achievements.ai",
        ],
    },
    Whitelisted {
        archive: "character/009_eruptor.bld",
        scripts: &[
            "009_Def_Eruptor.ai",
            "009_Eruptor_Tuning.ai",
            "009_Eruptor.ai",
            "009_LavaAttack.ai",
            "009_MagmaBall.ai",
            "Accolades_Macros_Fire.ai",
            "PortalEntry_Fire.ai",
            "PortalEntry_LightForce.ai",
        ],
    },
    // AG.162: the 66 walkers' (AG.27), by archive, each the scripts the TV's
    // listing runs for him: his file's running placements (his definition,
    // his tuning actor and, where his file runs one, a portal entry, a
    // shockwave, the combo UI or the ghost include) and his minion's, the
    // template his definition's `Portal - Define Minion` names. Read off the
    // dump by `giants_walkers`' `a_probe_of_the_part`, in the roster's order;
    // `the_whitelist_holds_each_listed_walkers_scripts` there holds them to it.
    Whitelisted {
        archive: "character/000_airdragon.bld",
        scripts: &[
            "000_AirDragon.ai",
            "000_AirDragon_Def.ai",
            "000_AirDragon_Tuning.ai",
            "PortalEntry_Air.ai",
        ],
    },
    Whitelisted {
        archive: "character/000_airdragon_alt.bld",
        scripts: &[
            "000_AirDragon.ai",
            "000_AirDragon_Alt_Def.ai",
            "000_AirDragon_Tuning.ai",
            "PortalEntry_Air.ai",
        ],
    },
    Whitelisted {
        archive: "character/001_gryphon.bld",
        scripts: &[
            "001_Gryphon.ai",
            "001_Gryphon_Def.ai",
            "001_Gryphon_Tuning.ai",
            "FX_Shockwave_Air.ai",
            "PortalEntry_Air.ai",
        ],
    },
    Whitelisted {
        archive: "character/002_skyturtle.bld",
        scripts: &[
            "002_Def_SkyTurtle.ai",
            "002_SkyTurtle.ai",
            "002_SkyTurtle_Tuning.ai",
            "PortalEntry_Air.ai",
        ],
    },
    Whitelisted {
        archive: "character/003_stormgiant.bld",
        scripts: &[
            "003_Def_StormGiant.ai",
            "003_StormGiant.ai",
            "003_StormGiant_Tuning.ai",
            "PortalEntry_Air.ai",
        ],
    },
    Whitelisted {
        archive: "character/004_rock_dragon.bld",
        scripts: &[
            "004_RockDragon.ai",
            "004_RockDragon_Tuning.ai",
            "Def_RockDragon.ai",
        ],
    },
    Whitelisted {
        archive: "character/005_landshark.bld",
        scripts: &[
            "005_Def_LandShark.ai",
            "005_LandShark.ai",
            "005_LandShark_Tuning.ai",
            "FX_Combo_UI.ai",
        ],
    },
    Whitelisted {
        archive: "character/006_dinorang.bld",
        scripts: &[
            "006_Def_Dinorang.ai",
            "006_DinoRang.ai",
            "006_DinoRang_Tuning.ai",
        ],
    },
    Whitelisted {
        archive: "character/007_gemgolem.bld",
        scripts: &[
            "007_GemGolem.ai",
            "007_GemGolem_Def.ai",
            "007_GemGolem_Tuning.ai",
        ],
    },
    Whitelisted {
        archive: "character/008_firedragon.bld",
        scripts: &[
            "008_Def_FireDragon.ai",
            "008_FireDragon.ai",
            "008_FireDragon_Tuning.ai",
        ],
    },
    Whitelisted {
        archive: "character/010_flameknight.bld",
        scripts: &[
            "010_Def_FlameKnight.ai",
            "010_FlameKnight.ai",
            "010_FlameKnight_Tuning.ai",
            "FX_Combo_UI.ai",
        ],
    },
    Whitelisted {
        archive: "character/010_flameknight_legendary.bld",
        scripts: &[
            "010_Def_FlameKnight_Legendary.ai",
            "010_FlameKnight.ai",
            "010_FlameKnight_Tuning.ai",
            "FX_Combo_UI.ai",
        ],
    },
    Whitelisted {
        archive: "character/011_fireelf.bld",
        scripts: &[
            "011_Def_FireElf.ai",
            "011_FireElf.ai",
            "011_FireElf_Tuning.ai",
        ],
    },
    Whitelisted {
        archive: "character/012_seadragon.bld",
        scripts: &[
            "012_Def_WaterDragon.ai",
            "012_SeaDragon_Tuning.ai",
            "012_WaterDragon2.ai",
        ],
    },
    Whitelisted {
        archive: "character/013_crab.bld",
        scripts: &[
            "013_Crab.ai",
            "013_Crab_Def.ai",
            "013_Crab_Tuning.ai",
            "FX_Combo_UI.ai",
        ],
    },
    Whitelisted {
        archive: "character/014_gillgrunt.bld",
        scripts: &[
            "014_GillGrunt.ai",
            "014_GillGrunt_Def.ai",
            "014_GillGrunt_Tuning.ai",
        ],
    },
    Whitelisted {
        archive: "character/015_yeti.bld",
        scripts: &[
            "015_Yeti.ai",
            "015_Yeti_Tuning.ai",
            "Def_Yeti.ai",
            "FX_Combo_UI.ai",
        ],
    },
    Whitelisted {
        archive: "character/015_yeti_legendary.bld",
        scripts: &[
            "015_Yeti.ai",
            "015_Yeti_Tuning.ai",
            "Def_Yeti_Legendary.ai",
            "FX_Combo_UI.ai",
        ],
    },
    Whitelisted {
        archive: "character/016_spyro.bld",
        scripts: &["016_Def_Spryo.ai", "016_Spyro.ai", "016_Spyro_Tuning.ai"],
    },
    Whitelisted {
        archive: "character/017_skullorc.bld",
        scripts: &[
            "017_SkullOrc.ai",
            "017_SkullOrc_Def.ai",
            "017_SkullOrc_Tuning.ai",
            "FX_Combo_UI.ai",
        ],
    },
    Whitelisted {
        archive: "character/018_wizard.bld",
        scripts: &["018_Def_Wizard.ai", "018_Wizard.ai", "018_Wizard_Tuning.ai"],
    },
    Whitelisted {
        archive: "character/018_wizard_alt.bld",
        scripts: &[
            "018_Def_Wizard_Alt.ai",
            "018_Wizard.ai",
            "018_Wizard_Tuning.ai",
        ],
    },
    Whitelisted {
        archive: "character/023_forcefieldgrub.bld",
        scripts: &[
            "023_Def_ForceFieldGrub.ai",
            "023_ForceFieldGrub_main.ai",
            "023_ForceFieldGrub_Tuning.ai",
        ],
    },
    Whitelisted {
        archive: "character/016_spyro_dark.bld",
        scripts: &[
            "016_Def_SpyroDark.ai",
            "016_Spyro.ai",
            "016_Spyro_Tuning.ai",
        ],
    },
    Whitelisted {
        archive: "character/020_metal_dragon.bld",
        scripts: &[
            "020_Def_Metal_Dragon.ai",
            "020_Metal_Dragon.ai",
            "020_MetalDragon_Tuning.ai",
        ],
    },
    Whitelisted {
        archive: "character/021_drillbot.bld",
        scripts: &[
            "021_DrillBot.ai",
            "021_DrillBot_Tuning.ai",
            "Def_DrillBot.ai",
        ],
    },
    Whitelisted {
        archive: "character/022_bomb_troll.bld",
        scripts: &["022_BombTroll_Tuning.ai", "022_Troll.ai", "Def_Troll.ai"],
    },
    Whitelisted {
        archive: "character/019_goldencheat.bld",
        scripts: &[
            "019_Def_GoldenCheat.ai",
            "019_GoldenCheat.ai",
            "019_GoldenCheat_Tuning.ai",
        ],
    },
    Whitelisted {
        archive: "character/024_lifedragon.bld",
        scripts: &[
            "024_LifeDragon.ai",
            "024_LifeDragon_Def.ai",
            "024_LifeDragon_Tuning.ai",
            "PortalEntry_Life.ai",
        ],
    },
    Whitelisted {
        archive: "character/025_bambazooker.bld",
        scripts: &[
            "025_BamBazooker.ai",
            "025_BamBazooker_Tuning.ai",
            "025_Def_BamBazooker.ai",
            "FX_Shockwave.ai",
            "PortalEntry_Life.ai",
        ],
    },
    Whitelisted {
        archive: "character/026_stealthelf.bld",
        scripts: &[
            "026_Def_StealthElf.ai",
            "026_StealthElf.ai",
            "026_StealthElf_Tuning.ai",
            "FX_Combo_UI.ai",
            "PortalEntry_Life.ai",
        ],
    },
    Whitelisted {
        archive: "character/026_stealthelf_legendary.bld",
        scripts: &[
            "026_Def_StealthElf_Legendary.ai",
            "026_StealthElf.ai",
            "026_StealthElf_Tuning.ai",
            "FX_Combo_UI.ai",
            "PortalEntry_Life.ai",
        ],
    },
    Whitelisted {
        archive: "character/027_ent.bld",
        scripts: &[
            "027_Ent.ai",
            "027_Ent_Def_Minion.ai",
            "027_Ent_Tuning.ai",
            "FX_Combo_UI.ai",
            "PortalEntry_Life.ai",
        ],
    },
    Whitelisted {
        archive: "character/032_cynder.bld",
        scripts: &[
            "032_Cynder.ai",
            "032_Cynder_Tuning.ai",
            "032_Def_Cynder.ai",
            "Minion_Include_Ghost.ai",
        ],
    },
    Whitelisted {
        archive: "character/029_shadowmaid.bld",
        scripts: &[
            "029_ShadowMaid.ai",
            "029_ShadowMaid_Def.ai",
            "029_ShadowMaid_Tuning.ai",
        ],
    },
    Whitelisted {
        archive: "character/030_pandoranelite.bld",
        scripts: &[
            "030_Def_PandoranElite.ai",
            "030_PandoranElite.ai",
            "030_PandoranElite_Tuning.ai",
            "FX_Combo_UI.ai",
        ],
    },
    Whitelisted {
        archive: "character/031_ghosteater.bld",
        scripts: &[
            "031_Def_GhostEater.ai",
            "031_GhostEater.ai",
            "031_GhostEater_Tuning.ai",
            "Minion_Include_Ghost.ai",
        ],
    },
    Whitelisted {
        archive: "character/016_spyro_legendary.bld",
        scripts: &[
            "016_Def_SpryoLegendary.ai",
            "016_Spyro.ai",
            "016_Spyro_Tuning.ai",
        ],
    },
    Whitelisted {
        archive: "character/030_pandoranelite_legendary.bld",
        scripts: &[
            "030_Def_PandoranEliteLegendary.ai",
            "030_PandoranElite.ai",
            "030_PandoranElite_Tuning.ai",
            "FX_Combo_UI.ai",
        ],
    },
    Whitelisted {
        archive: "character/004_rock_dragon_legendary.bld",
        scripts: &[
            "004_RockDragon.ai",
            "004_RockDragon_Tuning.ai",
            "Def_RockDragonLegendary.ai",
        ],
    },
    Whitelisted {
        archive: "character/019_goldencheat_legendary.bld",
        scripts: &[
            "019_Def_GoldenCheatLegendary.ai",
            "019_GoldenCheat.ai",
            "019_GoldenCheat_Tuning.ai",
        ],
    },
    Whitelisted {
        archive: "character/100_eagle.bld",
        scripts: &[
            "100_Eagle.ai",
            "100_Eagle_Def.ai",
            "100_Eagle_Tuning.ai",
            "FX_Combo_UI.ai",
            "PortalEntry_Air.ai",
        ],
    },
    Whitelisted {
        archive: "character/100_eagle_legendary.bld",
        scripts: &[
            "100_Eagle.ai",
            "100_Eagle_L_Def.ai",
            "100_Eagle_Tuning.ai",
            "FX_Combo_UI.ai",
            "PortalEntry_Air.ai",
        ],
    },
    Whitelisted {
        archive: "character/101_bugmangiant.bld",
        scripts: &[
            "101_BugmanGiant.ai",
            "101_BugmanGiant_Def.ai",
            "101_BugmanGiant_Tuning.ai",
            "PortalEntry_Air.ai",
        ],
    },
    Whitelisted {
        archive: "character/102_titan_earth.bld",
        scripts: &[
            "102_TitanEarth.ai",
            "102_TitanEarth_Def.ai",
            "102_TitanEarth_Tuning.ai",
            "FX_Combo_UI.ai",
        ],
    },
    Whitelisted {
        archive: "character/102_titan_earth_alt.bld",
        scripts: &[
            "102_TitanEarth.ai",
            "102_TitanEarth_Alt_Def.ai",
            "102_TitanEarth_Tuning.ai",
            "FX_Combo_UI.ai",
        ],
    },
    Whitelisted {
        archive: "character/103_earthdragon.bld",
        scripts: &[
            "103_EarthDragon.ai",
            "103_EarthDragon_Tuning.ai",
            "103_EarthDragonDef.ai",
        ],
    },
    Whitelisted {
        archive: "character/103_earthdragon_alt.bld",
        scripts: &[
            "103_EarthDragon.ai",
            "103_EarthDragon_Alt_Def.ai",
            "103_EarthDragon_Tuning.ai",
        ],
    },
    Whitelisted {
        archive: "character/104_oilblazer.bld",
        scripts: &[
            "104_OilBlazer.ai",
            "104_OilBlazer_Def.ai",
            "104_OilBlazer_Tuning.ai",
        ],
    },
    Whitelisted {
        archive: "character/105_firefeet.bld",
        scripts: &[
            "105_FireFeet.ai",
            "105_FireFeet_Def.ai",
            "105_FireFeet_Tuning.ai",
            "FX_Combo_UI.ai",
        ],
    },
    Whitelisted {
        archive: "character/105_firefeet_alt.bld",
        scripts: &[
            "105_FireFeet.ai",
            "105_FireFeet_Alt_Def.ai",
            "105_FireFeet_Tuning.ai",
            "FX_Combo_UI.ai",
        ],
    },
    Whitelisted {
        archive: "character/106_icesorc.bld",
        scripts: &[
            "106_IceSorc.ai",
            "106_IceSorc_Def.ai",
            "106_IceSorc_Tuning.ai",
        ],
    },
    Whitelisted {
        archive: "character/106_icesorc_legendary.bld",
        scripts: &[
            "106_IceSorc.ai",
            "106_IceSorc_Legendary_Def.ai",
            "106_IceSorc_Tuning.ai",
        ],
    },
    Whitelisted {
        archive: "character/107_whale.bld",
        scripts: &[
            "107_Whale.ai",
            "107_Whale_Def.ai",
            "107_Whale_Tuning.ai",
            "FX_Combo_UI.ai",
        ],
    },
    Whitelisted {
        archive: "character/108_alchemist.bld",
        scripts: &[
            "108_Alchemist.ai",
            "108_Alchemist_Def.ai",
            "108_Alchemist_Tuning.ai",
            "FX_Combo_UI.ai",
        ],
    },
    Whitelisted {
        archive: "character/108_alchemist_red.bld",
        scripts: &[
            "108_Alchemist.ai",
            "108_Alchemist_Legendary_Def.ai",
            "108_Alchemist_Tuning.ai",
            "FX_Combo_UI.ai",
        ],
    },
    Whitelisted {
        archive: "character/109_djini.bld",
        scripts: &[
            "109_Djini.ai",
            "109_Djini_Def.ai",
            "109_Djini_Tuning.ai",
            "FX_Combo_UI.ai",
        ],
    },
    Whitelisted {
        archive: "character/109_djini_alt.bld",
        scripts: &[
            "109_Djini.ai",
            "109_Djini_Alt_Def.ai",
            "109_Djini_Tuning.ai",
            "FX_Combo_UI.ai",
        ],
    },
    Whitelisted {
        archive: "character/110_robotgiant.bld",
        scripts: &[
            "110_RobotGiant.ai",
            "110_RobotGiant_Def.ai",
            "110_RobotGiant_Tuning.ai",
        ],
    },
    Whitelisted {
        archive: "character/110_robotgiant_legendary.bld",
        scripts: &[
            "110_RobotGiant.ai",
            "110_RobotGiant_Legendary_Def.ai",
            "110_RobotGiant_Tuning.ai",
        ],
    },
    Whitelisted {
        archive: "character/111_turretmaster.bld",
        scripts: &[
            "111_Def_TurretMaster.ai",
            "111_TurretMaster.ai",
            "111_TurretMaster_Tuning.ai",
            "FX_Combo_UI.ai",
        ],
    },
    Whitelisted {
        archive: "character/112_titanlife.bld",
        scripts: &[
            "112_TitanLife.ai",
            "112_TitanLife_Def.ai",
            "112_TitanLife_Tuning.ai",
            "PortalEntry_Life.ai",
        ],
    },
    Whitelisted {
        archive: "character/112_titanlife_alt.bld",
        scripts: &[
            "112_TitanLife.ai",
            "112_TitanLife_Alt_Def.ai",
            "112_TitanLife_Tuning.ai",
            "PortalEntry_Life.ai",
        ],
    },
    Whitelisted {
        archive: "character/113_myconman.bld",
        scripts: &[
            "113_Myconman.ai",
            "113_Myconman_Def.ai",
            "113_Myconman_Tuning.ai",
            "PortalEntry_Life.ai",
        ],
    },
    Whitelisted {
        archive: "character/114_mummy.bld",
        scripts: &[
            "114_Mummy.ai",
            "114_Mummy_Def.ai",
            "114_Mummy_Tuning.ai",
            "FX_Combo_UI.ai",
        ],
    },
    Whitelisted {
        archive: "character/115_ostrich.bld",
        scripts: &[
            "115_Def_Ostrich.ai",
            "115_Ostrich.ai",
            "115_Ostrich_Tuning.ai",
            "FX_Combo_UI.ai",
        ],
    },
    Whitelisted {
        archive: "level/level_213_tutotial.bld",
        scripts: &[
            // The level's definition and enemy stats, Setup_001's
            // controller and its Chompies, Norticus, the breakables and
            // coins in reach, the loot, the level's enemy system, the
            // route's triggers, push blocks and spike gate.
            "Level_Definition_213.ai",
            "Setup_001_Controller.ai",
            "S2_Enemy_Chompy.ai",
            "Norticus_First_appearance.ai",
            "Ambient_Chmpie.ai",
            "init_barrel_b.ai",
            "init_Ancient_Pottery.ai",
            "init_gourds_01.ai",
            "Scale.ai",
            "WoodFence_Spawner.ai",
            "Breakable_Wall_213.ai",
            "Placed_Loot_Spinning.ai",
            "LootSystem_Init.ai",
            "_EnemySystem_Functions.ai",
            "Activate_Norticus_Chompies.ai",
            "PushBlock_Template.ai",
            "Push_Block_End.ai",
            "Gate_Spikes.ai",
            // AG.60: the start's scripts on the props (the propeller, the
            // bird nests, the placed gems, the cannon's rubble wall, the
            // sliding rocks, the Titan boulders, the chest and the global
            // reference the chests share).
            "Windmill_Rotate.ai",
            "BirdNest.ai",
            "Placed_Loot.ai",
            "StoneWall_2.ai",
            "Falling_Rocks.ai",
            "Titan_Carriable_Object.ai",
            "Chest.ai",
            "Chest_SetGlobalRef.ai",
            // What those spawn.
            "Art_Generic_Stone.ai",
            "Box_Tortoise.ai",
            "Bouncing_Wood_Pieces.ai",
            "CannonWarpRing.ai",
            "Canon_Ball.ai",
            "Elemental_Symbol_Hints.ai",
            "Fence_Sign.ai",
            "Gate_Spikes_Hole.ai",
            "Lock_EnemyGate.ai",
            "Canon_bottom.ai",
            "Pushblock_canon.ai",
            "Switch_90_Setup.ai",
            "Torus_Effect.ai",
            "stone_wall_Piece_213.ai",
            "Wood_Plank.ai",
            // What AG.60's spawn.
            "StoneWall_Piece.ai",
            "Boulder_Pebbles.ai",
            "Boulder_Chunks.ai",
            "Chest_Lock.ai",
            "Lock_Flash.ai",
            // AG.28: the camera volumes and the sliding rocks' activator.
            "Pop_To_Camera_x.ai",
            "Pop_to_Camera_5.ai",
            "Special_Pop_To_Camera.ai",
            "Jump_Pad_Camera_Pop.ai",
            "Special_Camera_Transition_213.ai",
            "Pop_To_X_Special_213.ai",
            "Pop_to_42_end.ai",
            "Activator_Sliding_Rocks.ai",
            // AG.177: the jumping goats, whose jump opens the third rock.
            "Goat_Jumping_Pit.ai",
            // AH.42, AH.8's route (AH.28): the camera's pan to the cannon
            // and the pit's intro cutscene, on the cutscene system.
            "Pan_Show_Cannon.ai",
            "Pan_Show_Norticu.ai",
        ],
    },
];

/// Whether a script's source path (`C:/tfb/.../S2_Enemy_Chompy.ai`) names
/// one of `scripts`, by its file name, any case.
pub fn listed(scripts: &[&str], path: &str) -> bool {
    let file = path.rsplit(['/', '\\']).next().unwrap_or(path);
    scripts.iter().any(|s| s.eq_ignore_ascii_case(file))
}

/// The whitelisted scripts of the level archive `level`
/// (`level_213_tutotial`); none for a level off the whitelist.
pub fn level_scripts(level: &str) -> &'static [&'static str] {
    let archive = format!("{}/{level}.bld", crate::levels::LEVEL_FOLDER);
    WHITELIST
        .iter()
        .find(|w| w.archive == archive)
        .map_or(&[], |w| w.scripts)
}

/// The whitelisted scripts of the walkers' archives (AG.162): every entry
/// but the bootstrap's, `global.bld`'s, `init_setup.bld`'s, the fighters' and
/// a level's. A creature placed running one of them is followed to its
/// palette ([`crate::levels::palettes`]) as one running its level's is.
pub fn whitelisted_walker_scripts() -> Vec<&'static str> {
    let base = archives();
    WHITELIST
        .iter()
        .filter(|w| {
            w.archive
                .starts_with(&format!("{}/", models::CHARACTER_DIR))
        })
        .filter(|w| !base.iter().any(|b| b == w.archive))
        .flat_map(|w| w.scripts.iter().copied())
        .collect()
}

/// `character/<load file>.bld`, lower-cased as the disc names them.
pub fn character_archive(load_file: &str) -> String {
    format!(
        "{}/{}.bld",
        models::CHARACTER_DIR,
        load_file.to_ascii_lowercase()
    )
}

/// The walkers' own load files out of `loads` (the roster's modelled
/// characters, [`crate::models::character_loads`]): each but a fighter,
/// whose entry is the fighters' already, in the roster's order.
pub fn walker_loads(loads: &[&str]) -> Vec<String> {
    loads
        .iter()
        .filter(|l| !FIGHTERS.iter().any(|f| f.eq_ignore_ascii_case(l)))
        .map(|l| (*l).to_owned())
        .collect()
}

/// Which walkers the scripts part holds and which it left out and why: what
/// [`WALKERS_FILE`] keeps, so the TV links the walkers' entries it was made
/// with without the roster.
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct WalkerList {
    /// Load files whose entries are in the part, in link order.
    pub listed: Vec<String>,
    /// Load files whose entries aren't, with the reason: each keeps his card.
    pub left_out: Vec<(String, String)>,
}

/// The walkers a start's scripts part holds, read from the extract at `dir`;
/// none for an extract made before them (no file), the error for one that
/// won't read.
pub fn read_walkers(dir: &Path) -> Result<WalkerList, String> {
    let path = dir.join(WALKERS_FILE);
    match std::fs::read(&path) {
        Ok(bytes) => serde_json::from_slice(&bytes).map_err(|e| format!("{WALKERS_FILE}: {e}")),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(WalkerList::default()),
        Err(e) => Err(format!("{WALKERS_FILE}: {e}")),
    }
}

/// The archives the scripts part copies, in load order: `bootstrap.bld`
/// (its roots take the first export ids), `global.bld`, `init_setup.bld`,
/// then each fighter's. The walkers' follow ([`archives_with`]).
pub fn archives() -> Vec<String> {
    [BOOTSTRAP, GLOBAL, INIT_SETUP]
        .into_iter()
        .map(str::to_owned)
        .chain(FIGHTERS.iter().map(|f| character_archive(f)))
        .collect()
}

/// [`archives`] and then each of `walkers`' (load files).
pub fn archives_with(walkers: &[String]) -> Vec<String> {
    archives()
        .into_iter()
        .chain(walkers.iter().map(|w| character_archive(w)))
        .collect()
}

/// The archives the spawn models come from: `global.bld` and each
/// fighter's.
pub fn spawn_archives() -> Vec<String> {
    std::iter::once(GLOBAL.to_owned())
        .chain(FIGHTERS.iter().map(|f| character_archive(f)))
        .collect()
}

/// An archive's stem (`permanent/global.bld` → `global`): its files' name
/// in each part.
pub fn stem(archive: &str) -> &str {
    let file = archive.rsplit('/').next().unwrap_or(archive);
    file.strip_suffix(".bld").unwrap_or(file)
}

/// `scripts/<stem>.igz`: the entry as the disc holds it, in either IR
/// format.
pub fn script_path(archive: &str) -> String {
    format!("{SCRIPT_DIR}/{}.igz", stem(archive))
}

/// Whether `m` holds both parts, the scripts one with its walkers' list
/// ([`WALKERS_FILE`]): a start is current only then, so an extract made
/// before the walkers resumes for the scripts part alone.
pub fn complete(m: &Manifest) -> bool {
    m.parts
        .get(SCRIPTS)
        .is_some_and(|files| files.iter().any(|f| f == WALKERS_FILE))
        && m.parts.contains_key(SPAWNS)
}

/// The `level.bld` `archive` keeps, decompressed, under `usrdir`. Why it
/// can't is the error: no archive, no entry, or one that won't unpack.
pub fn entry(usrdir: &Path, archive: &str) -> Result<Vec<u8>, String> {
    let path = usrdir.join(archive);
    let bytes = std::fs::read(&path).map_err(|e| format!("{}: {e}", path.display()))?;
    let arc = Archive::parse(&bytes).map_err(|e| e.to_string())?;
    let e = arc.find(LEVEL).ok_or("no level.bld")?;
    Ok(arc.read(e).map_err(|e| e.to_string())?.into_owned())
}

/// The scripts an entry holds, by source path, once it parses and its op
/// graph reads (AH.13's readers): what's checked before it's written.
pub fn check(entry: &[u8]) -> Result<Vec<String>, String> {
    let igz = Igz::parse(entry).map_err(|e| e.to_string())?;
    let graph = Graph::read(&igz).map_err(|e| e.to_string())?;
    Ok(graph
        .scripts
        .iter()
        .map(|s| s.name.unwrap_or_default().to_owned())
        .collect())
}

/// Every model a placement in `igz` names, as a level's props are read,
/// but `skip` (a character's own body), its textures into the read. One
/// that doesn't read is listed with why.
pub fn spawn_models(
    igz: &Igz<'_>,
    archive: &str,
    identity: DumpIdentity,
    skip: Option<usize>,
) -> Result<Read<SpawnModels>, String> {
    let placements = Placements::read(igz).map_err(|e| e.to_string())?;
    let mut textures = Textures::default();
    let mut by_path: BTreeMap<&str, ()> = BTreeMap::new();
    let (mut props, mut refused): (Vec<Prop>, Vec<RefusedProp>) = (Vec::new(), Vec::new());
    for (&model, info) in &placements.models {
        if Some(model) == skip {
            continue;
        }
        let path = info
            .info
            .name
            .ok_or_else(|| format!("model #{model} has no name"))?;
        if by_path.insert(path, ()).is_some() {
            return Err(format!("two models named {path}"));
        }
        let mut own = textures.clone();
        match crate::levels::prop(igz, model, path, &mut own) {
            Ok(p) => {
                textures = own;
                props.push(p);
            }
            Err(reason) => refused.push(RefusedProp {
                path: path.to_owned(),
                reason,
            }),
        }
    }
    Ok(Read {
        ir: SpawnModels {
            identity,
            archive: archive.to_owned(),
            props,
            refused,
        },
        textures: textures.files,
    })
}

/// The model a character archive draws its figure with in play
/// ([`fmodel::in_play`]): the model stage's, left out of its spawns.
/// `None` for an archive that isn't a character's.
pub fn body(igz: &Igz<'_>, archive: &str) -> Option<usize> {
    let load = stem(archive);
    let portal = fmodel::portal_model(igz).ok()?;
    Some(fmodel::in_play(igz, portal, load).ok()?.model)
}

#[cfg(test)]
mod tests;
