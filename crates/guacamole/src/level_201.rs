//! Level 201 from its landing to its end (openspec AU.53): the level's way,
//! as [`crate::scripts::ENCOUNTERS`] and [`crate::scripts::PASSAGES`] name
//! level 214's and [`crate::chapter_1`] chapter 1's past its first fight:
//! the enemies it places by name, the clones its scripts make that fight,
//! the level's own scripts by their path under its folder (no other level
//! places them) and what of the shared ones it needs by placement name.
//!
//! The way, as its scripts have it:
//! - **The landing.** The intro ([`crate::scripts::INTROS`]) lands the hero
//!   by pod and walks him in. Setup_001's Chompies run down the path ahead
//!   of him; past the green Chompy's enemy intro the first set waits in the
//!   yard below, its armoured Elf Near's intro plays at its second trigger
//!   and he jumps up onto the plateau by the spike gate, where a Chompy pod
//!   waits with him. Both beaten, the gate opens onto the path to the first
//!   key; brought back to the yard, the key wakes the second set.
//! - **The door to the life mage.** Setup_002 waits for the key by its
//!   first trigger; turned in the door above the yard, it opens on the life
//!   mage's intro and his Elf Nears and Chompies.
//! - **The practice pit.** Setup_003's triggers set its Elf Nears in their
//!   places along the cliff path; a row of push blocks walls the way west
//!   until three move (the middle one south, the west one west, the one
//!   behind it south); the pit's third trigger brings the waves, Brock's
//!   practice pit scene with its two elves jumping in, the tower's waves and
//!   the lobbers, then Brock's scene after them, which opens the gate west.
//! - **Two keys for the west door.** The second key lies on the mound by
//!   the tower; the third in the floating house's room (house 2), past its
//!   mage and Elf Near, whose fight lowers the room's gate. Back out, the
//!   house wakes Setup_003B's Chompies in the yard by it, and the two keys
//!   open the double-locked door west.
//! - **The eyeball door and the bridge.** Setup_004's Elf Nears wait in the
//!   round yard below a timed door; hit, the eyeball on the mound opens it
//!   for a while. Past the yard's fight, Brock runs over the rope bridge
//!   and smashes it under the hero (Brock's scene: the bridge's planks are
//!   clones of its controller's, which stand by their own solids, solid to
//!   actors and actor surfaces, `Actor::stands_for_actors`), and he falls
//!   to the grass below.
//! - **The channel's blocks.** Below, a dry channel splits the ground: the
//!   west stack pushed into its side arm makes a way over; the block on the
//!   far side is pushed into the channel first, then the top block along
//!   its way to the activator, which takes the first away and wakes the two
//!   blocks in the channel, a way over to the fourth key. That key opens the
//!   floating house's room again through house 4, its door to the fifth key
//!   and a soul gem; the fifth opens the door to the east stack, whose three
//!   blocks fill the far arm; the stack's top and the channel's block
//!   pushed in by the checker wake Setup_005, whose lobbers and Chompy pods
//!   come at its two triggers. Beaten, the gate south opens.
//! - **The arena.** Two eyeballs hold the arena's door open; Brock's arena
//!   scene, his waves, the half-time scene, Brock himself, the outro and
//!   the level's end, which names the village.

use crate::scripts::Encounter;

/// What fights on level 201, walked as creatures: every enemy it places,
/// the clones its scripts make that fight (its pods' Chompies, the arena's
/// waves, the practice pit scene's two elves, whose jump the engine lands).
pub const WAY: Encounter = Encounter {
    level: "level_201",
    placements: &[
        "ChompySpawner(1)",
        "ChompySpawner(2)",
        "ChompySpawner(3)",
        "ChompySpawner(4)",
        "ChompySpawner(5)",
        "ChompySpawner(6)",
        "ChompySpawner_C1(1)",
        "ChompySpawner_C1(2)",
        "ChompySpawner_C1(3)",
        "ChompySpawner_C2(1)",
        "ChompySpawner_C2(2)",
        "ChompySpawner_H3(1)",
        "Enemy_Chompy(1)",
        "Enemy_Chompy(2)",
        "Enemy_Chompy(3)",
        "Enemy_Chompy(4)",
        "Enemy_Chompy(5)",
        "Enemy_Chompy(6)",
        "Enemy_Chompy(7)",
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
        "Enemy_Chompy(IGC)",
        "Enemy_Chompy_H3(1)",
        "Enemy_Chompy_H3(2)",
        "Enemy_Chompy_H3(3)",
        "Enemy_Chompy_H3(4)",
        "Enemy_Chompy_H3(5)",
        "Enemy_Chompy_H3(6)",
        "Enemy_Chompy_H3(7)",
        "Enemy_Chompy_H3(8)",
        "Enemy_Chompy_H3(9)",
        "Enemy_Chompy_H3(10)",
        "Enemy_Chompy_new(1)",
        "Enemy_Chompy_new(2)",
        "Enemy_Chompy_new(3)",
        "Enemy_Chompy_new(5)",
        "Enemy_Chompy_new(6)",
        "Enemy_Chompy_new(7)",
        "Enemy_Chompy_new(8)",
        "Enemy_Chompy_new(9)",
        "Enemy_Chompy_new(12)",
        "Enemy_Chompy_new(41)",
        "Enemy_Chompy_new(42)",
        "Enemy_Chompy_new(43)",
        "Enemy_Chompy_new(44)",
        "Enemy_Chompy_new(45)",
        "Enemy_Elemental_Mage_Life(1)",
        "Enemy_Elemental_Mage_Life(2)",
        "Enemy_Elemental_Mage_Life(3)",
        "Enemy_Elemental_Mage_Life_C1(1)",
        "Enemy_Elemental_Mage_Life_H2(1)",
        "Enemy_Elf_Lobber(1)",
        "Enemy_Elf_Lobber(2)",
        "Enemy_Elf_Lobber(3)",
        "Enemy_Elf_Lobber(4)",
        "Enemy_Elf_Lobber(5)",
        "Enemy_Elf_Lobber(6)",
        "Enemy_Elf_Lobber(7)",
        "Enemy_Elf_Lobber(8)",
        "Enemy_Elf_Lobber(9)",
        "Enemy_Elf_Lobber(10)",
        "Enemy_Elf_Lobber(11)",
        "Enemy_Elf_Lobber(12)",
        "Enemy_Elf_Lobber(13)",
        "Enemy_Elf_Lobber(14)",
        "Enemy_Elf_Lobber(15)",
        "Enemy_Elf_Lobber(16)",
        "Enemy_Elf_Near(1)",
        "Enemy_Elf_Near(2)",
        "Enemy_Elf_Near(3)",
        "Enemy_Elf_Near(4)",
        "Enemy_Elf_Near(5)",
        "Enemy_Elf_Near(6)",
        "Enemy_Elf_Near(8)",
        "Enemy_Elf_Near(9)",
        "Enemy_Elf_Near(14)",
        "Enemy_Elf_Near(15)",
        "Enemy_Elf_Near(16)",
        "Enemy_Elf_Near(19)",
        "Enemy_Elf_Near(23)",
        "Enemy_Elf_Near(24)",
        "Enemy_Elf_Near(25)",
        "Enemy_Elf_Near(26)",
        "Enemy_Elf_Near(28)",
        "Enemy_Elf_Near(29)",
        "Enemy_Elf_Near(66)",
        "Enemy_Elf_Near(77)",
        "Enemy_Elf_Near(88)",
        "Enemy_Elf_Near(99)",
        "Enemy_Elf_Near_H2(1)",
    ],
    clones: &[
        "Enemy_Chompy(0)",
        "Enemy_Chompy",
        "Enemy_Elf_Near_S2",
        "Enemy_Elf_Lobber",
        "Enemy_Elemental_Mage_Life",
        "Enemy_Elf_Heavy_S2",
        "Practice_Pit_Cutscene_Elf_Near",
        "026_StealthElf_Evil",
    ],
    steered: &[],
};

/// The level's own scripts, by their path under its folder: its setups and
/// their triggers, its scenes, its doors, bridge and arena, its pans and its
/// zones. No other level places them. Its music is
/// [`crate::scripts::MUSIC_SCRIPTS`]' and its camera volumes
/// [`crate::scripts::CAMERA_SCRIPTS`]'.
pub const OWN_SCRIPTS: [&str; 70] = [
    "Level_201/Cutscenes/Scripts/IGC_201_Arena_Brock_Battle.ai",
    "Level_201/Cutscenes/Scripts/IGC_201_Arena_Fight_Controller.ai",
    "Level_201/Cutscenes/Scripts/IGC_201_Arena_Half_Battle.ai",
    "Level_201/Cutscenes/Scripts/IGC_201_Arena_Outro.ai",
    "Level_201/Cutscenes/Scripts/IGC_201_Brock_NPC.ai",
    "Level_201/Cutscenes/Scripts/IGC_201_Grand_Arena.ai",
    "Level_201/Cutscenes/Scripts/IGC_201_Intro.ai",
    "Level_201/Cutscenes/Scripts/IGC_201_Post_Practice_Pitsi.ai",
    "Level_201/Cutscenes/Scripts/IGC_201_Practice_Pits.ai",
    "Level_201/Cutscenes/Scripts/IGC_Elf_Heavy_Reveal.ai",
    "Level_201/Cutscenes/Scripts/IGC_Green_Chompy_Reveal.ai",
    "Level_201/Scripts/201_Arena_FireBlaster.ai",
    "Level_201/Scripts/Activate_Interior_Cave.ai",
    "Level_201/Scripts/Activate_Life_Wall.ai",
    "Level_201/Scripts/Activate_Tower_Interior.ai",
    "Level_201/Scripts/Arena_Camera_Controller.ai",
    "Level_201/Scripts/Arena_lookLeft.ai",
    "Level_201/Scripts/Arena_lookRight.ai",
    "Level_201/Scripts/Arena_net.ai",
    "Level_201/Scripts/Balloon_Scale_up.ai",
    "Level_201/Scripts/Bridge_Debris.ai",
    "Level_201/Scripts/Camera_hotspot_Combat.ai",
    "Level_201/Scripts/Card_Game_Not_yet.ai",
    "Level_201/Scripts/Card_Game_Not_yet2.ai",
    "Level_201/Scripts/Castle_Arena.ai",
    "Level_201/Scripts/Collapsing_Bridge_Controller.ai",
    "Level_201/Scripts/Collapsing_Bridge_Hanging_rope.ai",
    "Level_201/Scripts/Collapsing_Bridge_Support_pole.ai",
    "Level_201/Scripts/Collapsing_Bridge_Suspension_rope.ai",
    "Level_201/Scripts/Cutscene_Chompie.ai",
    "Level_201/Scripts/Elf_Npc_JustSpear.ai",
    "Level_201/Scripts/Elf_Npc.ai",
    "Level_201/Scripts/Enemy_Placements_Setup_001_Controller_Helper.ai",
    "Level_201/Scripts/Enemy_Placements_Setup_001_Controller.ai",
    "Level_201/Scripts/Enemy_Placements_Setup_002_Controller_Helper.ai",
    "Level_201/Scripts/Enemy_Placements_Setup_002_Controller.ai",
    "Level_201/Scripts/Enemy_Placements_Setup_003_Controller_Helper.ai",
    "Level_201/Scripts/Enemy_Placements_Setup_003_Controller.ai",
    "Level_201/Scripts/Enemy_Placements_Setup_003B_Controller.ai",
    "Level_201/Scripts/Enemy_Placements_Setup_004_Controller.ai",
    "Level_201/Scripts/Enemy_Placements_Setup_005_Controller.ai",
    "Level_201/Scripts/Enemy_Placements_Setup_C1_Controller.ai",
    "Level_201/Scripts/Enemy_Placements_Setup_H2_Controller.ai",
    "Level_201/Scripts/Enemy_Placements_Setup_H3_Controller.ai",
    "Level_201/Scripts/Enemy_Placements_Setup_NBA_Controller.ai",
    "Level_201/Scripts/Enemy_Placements_Setup_T1_Controller.ai",
    "Level_201/Scripts/Enemy_Placements_Setup_UBA_Controller.ai",
    "Level_201/Scripts/Flynn_at_start.ai",
    "Level_201/Scripts/IGC_Collapsing_Bridge_Controller.ai",
    "Level_201/Scripts/JumpPad_activator.ai",
    "Level_201/Scripts/level_controller.ai",
    "Level_201/Scripts/Level_Definition_201.ai",
    "Level_201/Scripts/Pan_Show_2_keys.ai",
    "Level_201/Scripts/Pan_Show_Bridge_Extend_2.ai",
    "Level_201/Scripts/Pan_Show_Bridge_Extend_3.ai",
    "Level_201/Scripts/Pan_Show_Bridge_Extend.ai",
    "Level_201/Scripts/Pan_Show_Bridge_post_fight.ai",
    "Level_201/Scripts/Pan_Show_Eye.ai",
    "Level_201/Scripts/Pan_Show_key.ai",
    "Level_201/Scripts/Post_Fight_Camera_Swap.ai",
    "Level_201/Scripts/Pusblock_Special_Activator.ai",
    "Level_201/Scripts/Pushblock_Special_Deactivator.ai",
    "Level_201/Scripts/Quigly_NPC.ai",
    "Level_201/Scripts/Remove_Life_Wall.ai",
    "Level_201/Scripts/Shutdown_Zone_1.ai",
    "Level_201/Scripts/Size_Board.ai",
    "Level_201/Scripts/Size.ai",
    "Level_201/Scripts/Small_prop_Crane.ai",
    "Level_201/Scripts/Spawn_Arrow.ai",
    "Level_201/Scripts/Switch_Quigley.ai",
];

/// What of the shared scripts its way needs, by placement name, listed and
/// never walked: the timed doors and their eyeballs, the life area's
/// teleporters (and the helper its start makes, which carries the hero to
/// its end: openspec AS.183) and vine bridges, the cave's lowering gate, the Giants' floor
/// and log, the arena's wave master, its commander, crowd and drummers and
/// its fountain, Auric's store, and its jump pads (the AU.53 review: a walk
/// drops into the pit below the landing's yard and the one off the grass
/// the bridge drops him on, and each one's pad is the only way out; the
/// third wakes once its crate is broken, by its own activator); and the
/// arena's lemming avoider (openspec AS.177: with every hero below the
/// arena's floor it holds its enemies idle, so none follows him off it, and
/// sends them after him again once one is back up); and the life island's
/// ambush (openspec AS.200): its initializer, woken as a hero comes onto the
/// island past the first vine bridge, defines a minion boss battle the
/// phase director runs beside its camera and the players' invulnerizer;
/// it summons three evil Stealth Elves through the portal's entry beam,
/// each on Stealth Elf's own scripts (walked as [`WAY`]'s clones), driven
/// by a minion controller, its button controllers and its AI
/// (`026_StealthElf_AI`); each beaten, its defeated scene; all three, the
/// victory phase ends the battle and takes the initializer off, which
/// opens the second bridge's pan. Their init (`EnemyMinionInit`) is left
/// off: it scales a player's hurts by its 1.0 and zeroes one player's hits
/// on the other's, co-op's alone.
pub const PASSAGE: [&str; 52] = [
    "EyE_Timer_Door_Template(1)",
    "EyE_Timer_Door_Template(2)",
    "EyE_Timer_Door_Template(3)",
    "Timer_Eyeball_Template(2)",
    "Timer_Eyeball_Template(4)",
    "Timer_Eyeball_Template(5)",
    "Timer_Eyeball_Template_long Range",
    "TeleporterStart(1)",
    "TeleporterEnd(1)",
    "TeleporterHelper",
    "Activator_Template(1)",
    "Activator_Template(2)",
    "Activator_Template(3)",
    "Activator_Template(4)",
    "Temp_Lower_Gate(4)",
    "Titan_Floor(Template)_201(1)",
    "TitanGateLift_Template_201",
    "WaveMaster",
    "commander",
    "castleArena_AudienceController",
    "castleArena_chanting_controller",
    "drummer_watcher",
    "1drummer",
    "2drummer",
    "3drummer",
    "4drummer",
    "5drummer",
    "6drummer",
    "ArenaMaster_lootFountain",
    "Auric-STORE",
    "PvP_JumpPad_NORMAL(1)",
    "PvP_JumpPad_NORMAL(2)",
    "PvP_JumpPad_NORMAL(3)",
    "PvP_JumpPad_NORMAL(4)",
    "PvP_JumpPad_NORMAL(5)",
    "Enemy_LemmingAvoider",
    "Phase Director Initializer 201 Minion",
    "PhaseDirector",
    "BossCamera",
    "PlayerMinionInvulnerizer",
    "Phase Cutscene Intro",
    "Phase Cutscene Victory",
    "MinionController",
    "MinionController_Button",
    "MinionController_Movement",
    "026_StealthElf_AI",
    "026_StealthElf_Tuning Actor",
    "Evil Minion Defeated Cutscene",
    "Delayed Player Minion Vulnerizer",
    "Delayed Xp Ball Collector",
    "Delayed Xp Ball Collector (Defeated Cutscene)",
    "Delayed Food Spawner",
];

/// The life area's vine bridges' own clones (openspec AS.175), by their
/// scripts: an activator ([`PASSAGE`]) opened by its pan's trigger or its
/// fight bursts its roots (`StartRoots_cycle.ai`), grows a platform with no
/// solid while it plays (`Root_Path_Grow3.ai`), then lays the completed one
/// (`Root_Final_Mesh.ai`), its solid stretched across the gap to its
/// activator's length, which stands for actors by its own (openspec
/// AS.168), and strews its flowers (`Flower.ai`). No other level the TV
/// plays places a vine bridge.
pub const VINE_BRIDGE: [&str; 4] = [
    "/StartRoots_cycle.ai",
    "/Root_Path_Grow3.ai",
    "/Root_Final_Mesh.ai",
    "/Flower.ai",
];

/// The shared scripts [`VINE_BRIDGE`] lists for the level whose archive is
/// `level`.
pub fn shared_scripts(level: &str) -> &'static [&'static str] {
    match level {
        "level_201" => &VINE_BRIDGE,
        _ => &[],
    }
}

/// The scripts [`OWN_SCRIPTS`] lists for the level whose archive is `level`.
pub fn own_scripts(level: &str) -> &'static [&'static str] {
    match level {
        "level_201" => &OWN_SCRIPTS,
        _ => &[],
    }
}

/// The placements [`PASSAGE`] lists for the level whose archive is `level`.
pub fn passage(level: &str) -> &'static [&'static str] {
    match level {
        "level_201" => &PASSAGE,
        _ => &[],
    }
}

#[cfg(test)]
mod tests {
    use guac_script::Mode;

    use super::*;
    use crate::scripts::{Files, encounters, is_music_script, level_config};

    const OWN: &str = "C:/tfb/Content/Levels/Level_201/Scripts/Castle_Arena.ai";
    const OTHER_LEVELS: &str = "C:/tfb/Content/Levels/Level_202/Scripts/Castle_Arena.ai";

    /// Its way is level 201's alone: its enemies, passage and own scripts on
    /// its listing and on no level's that lists nothing of its own (level
    /// 214 lists an `Enemy_Chompy(4)` of its own), and a script of the same
    /// name under another level's folder never.
    #[test]
    fn level_201s_way_is_on_its_listing_alone() {
        let files = Files::default();
        let on = level_config(Mode::Play, &files, 5, WAY.level);
        let others = ["level_202", "level_205_piratecards"];
        for name in WAY.placements.iter().chain(WAY.clones).chain(&PASSAGE) {
            assert!(on.whitelist.lists(name, "C:/l/Unlisted.ai"), "{name}");
            for level in others {
                let off = level_config(Mode::Play, &files, 5, level);
                assert!(
                    !off.whitelist.lists(name, "C:/l/Unlisted.ai"),
                    "{name} on {level}"
                );
            }
        }
        assert!(on.whitelist.lists("", OWN));
        assert!(!on.whitelist.lists("", OTHER_LEVELS));
        let off = level_config(Mode::Play, &files, 5, "level_202");
        assert!(!off.whitelist.lists("", OWN));
        assert_eq!(encounters(WAY.level).count(), 1);
        assert!(own_scripts("level_202").is_empty());
        assert!(passage("level_202").is_empty());
    }

    /// openspec AS.175: the vine bridges' clones' scripts run on 201's
    /// listing, as the shared scripts they are, and on no other level's,
    /// whatever folder places them.
    #[test]
    fn its_vine_bridges_clones_are_listed() {
        let files = Files::default();
        let on = level_config(Mode::Play, &files, 5, WAY.level);
        for script in VINE_BRIDGE {
            let path =
                format!("C:/tfb/Content/Levels/Includes/GameElement_VineBridge/Scripts{script}");
            assert!(on.whitelist.lists("a clone", &path), "{script}");
            for level in ["level_202", "level_213_tutotial", "level_214_tutotial_2"] {
                let off = level_config(Mode::Play, &files, 5, level);
                assert!(
                    !off.whitelist.lists("a clone", &path),
                    "{script} on {level}"
                );
            }
        }
        assert!(shared_scripts("level_202").is_empty());
    }

    /// openspec AS.183: the helper the life area's teleporter start makes
    /// to carry the hero to its end runs on 201's listing, as the start's
    /// own clone, and on no other level's.
    #[test]
    fn its_teleporters_helper_is_listed() {
        let files = Files::default();
        let on = level_config(Mode::Play, &files, 5, WAY.level);
        let off = level_config(Mode::Play, &files, 5, "level_202");
        let path = "C:/tfb/Content/Levels/Includes/Teleporter/scripts/Teleporter_Helper.ai";
        assert!(on.whitelist.lists("TeleporterHelper", path));
        assert!(!off.whitelist.lists("TeleporterHelper", path));
    }

    /// AS.160's half on level 201: the life wall's two triggers, which write
    /// its controller's range as the hero passes them, run on its listing
    /// and on no other level's.
    #[test]
    fn its_life_wall_triggers_are_listed() {
        let files = Files::default();
        let on = level_config(Mode::Play, &files, 5, WAY.level);
        let off = level_config(Mode::Play, &files, 5, "level_202");
        for (name, script) in [
            ("Special_Activate_Life_wall", "Activate_Life_Wall.ai"),
            ("Special_Remove_Life_Wall", "Remove_Life_Wall.ai"),
        ] {
            let path = format!("C:/tfb/Content/Levels/Level_201/Scripts/{script}");
            assert!(on.whitelist.lists(name, &path), "{name}");
            assert!(!off.whitelist.lists(name, &path), "{name} on 202");
        }
    }

    /// Its own scripts are its folder's (each a path under it, none a bare
    /// file name another level's copy would match), its music the music
    /// list's and its walked enemies never what takes the way on.
    #[test]
    fn its_own_scripts_are_its_folders() {
        for s in OWN_SCRIPTS {
            assert!(s.starts_with("Level_201/"), "{s}");
            assert!(!s.contains("/snd_"), "{s} is music");
        }
        assert!(is_music_script(
            "C:/tfb/Content/Levels/Level_201/Scripts/snd_combat_watcher_201.ai"
        ));
        assert!(!is_music_script(OWN));
        for name in PASSAGE {
            assert!(!WAY.placements.contains(&name), "{name}");
        }
    }
}
