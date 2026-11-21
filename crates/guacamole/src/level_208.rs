//! Level 208's way out of its start plaza (openspec AS.344), as
//! [`crate::level_205`] lists 205's: what of the level's own scripts it
//! needs, by placement name.
//!
//! The plaza's south exit looks open but two scripted solids close it, and
//! the original gets a hero through each by its own script:
//! - **The reveal's trigger** (`Ice_Chompy_IGC_Trigger`, a box 40 by 20 by
//!   8 ft across the exit, solid to actors and the level). Woken, it makes
//!   the first room's Ice Chompies (the level's `Setup_001_Enemies`)
//!   invulnerable, then waits for a player to come into it; one does as he
//!   walks into the box, and it plays the reveal (`IGC_Ice_Chompy_Reveal`:
//!   the camera on one Ice Chompy past the exit, its freeze attack and the
//!   enemy intro's card the first time), waits for the cutscene's end,
//!   makes them vulnerable again and takes itself off, its solid with it.
//!   Its first step reads each of those Chompies' creature info and goes
//!   no further on one that has none, so it waits on all five standing.
//! - **The brick wall** (`Wall_Spawner_Template(5)`, `Brick_Wall_Spawner`,
//!   past the trigger). At its start it lays its bricks (clones of `brick`
//!   and `brick2`) along its box, which it keeps invisible, and makes
//!   itself a 40-hp destructible the hero's attacks hurt; a heavy hit or
//!   its health run out blows the bricks apart (their own script flies
//!   them off and they go), turns its box off for actors and takes itself
//!   off a few seconds on. Nothing else spends it: a hero who doesn't
//!   attack it stays behind it.
//!
//! Past both lies the yard by Setup_002's controller, and its fights
//! (openspec AS.345, [`WAY`]):
//! - **The five Ice Chompies.** Out of the reveal they stand idle until the
//!   combat manager's alert check (every 0.25 s) finds a player within
//!   their 40 ft notice radius with a clear line to him; then they chase,
//!   and within their 6 ft attack range breathe ice on him between
//!   cooldowns, until beaten.
//! - **Setup_002**, down on the frozen river west of the yard: once a
//!   player is in its trigger (`Setup_002_Trigger(1)`) it runs its two
//!   spinning Cyclopes' queues (208's intro turned them and the three adds
//!   off; the queue's `control` wakes them, ledger `control-wakes`); with
//!   both beaten and a player within 60 ft of the snowman by the first gate
//!   it wakes Noodles' scene there (`IGC_Noodles_FirstGate`, a pan), then
//!   runs its heavy Biclops and walks the three adds in along their paths;
//!   with its six (`Setup_002_Enemies`) gone it takes its set off (itself,
//!   its trigger, its paths), Noodles plays his post-fight part and the
//!   spike gate by the spinners (`Spike_Gate_Metal_Template(1)`, on the
//!   generic listing) lowers, opening the river west.

use crate::scripts::Encounter;

/// The archive the story reaches after 205 (openspec AS.340).
pub const LEVEL: &str = "level_208_ice";

/// The exit's two solids and the wall's brick templates (a clone runs under
/// its template's name), by name, listed and never walked: without the
/// bricks' script a beaten wall's bricks stand where they were laid until
/// the wall takes itself off.
pub const EXIT: [&str; 4] = [
    "Ice_Chompy_IGC_Trigger",
    "Wall_Spawner_Template(5)",
    "brick",
    "brick2",
];

/// The five Ice Chompies of the first room's set (`Setup_001_Enemies`),
/// which the reveal's trigger waits on, by name: listed with the exit and
/// walked as [`WAY`]'s (openspec AS.345). The reveal turns its Chompy to
/// its mark and plays its freeze attack; then all five wake to the hero,
/// chase him and breathe ice on him.
pub const REVEAL_FOES: [&str; 5] = [
    "Enemy_Chompy_Ice(2)",
    "Enemy_Chompy_Ice(3)",
    "Enemy_Chompy_Ice(20)",
    "Enemy_Chompy_Ice(21)",
    "Enemy_Chompy_Ice(40)",
];

/// [`EXIT`] and [`REVEAL_FOES`] on 208 alone: another level's `brick`
/// templates are its own.
pub fn exit(level: &str) -> impl Iterator<Item = &'static str> {
    let on = level == LEVEL;
    EXIT.iter().chain(&REVEAL_FOES).copied().filter(move |_| on)
}

/// The way from Setup_002's yard down to its trigger on the frozen river
/// (openspec AS.406), by name, each run by its own script:
/// - **The AI's hint** (`IGC_AI_Intro`, `208_IGC_AI.ai`; the follower it
///   drives, `NPC_AI`, needs no script of its own for the way down, so it
///   stays unlisted). Once the reveal's five Chompies are gone and a
///   player comes within 30 ft of it, the AI talks to him, plays its pan
///   and its two lines, draws its hint into him and takes the hint's
///   blocker (`Pandoran_Hint_Template(1)`, a 90 by 6 by 16 ft box solid to
///   actors across the yard's east side) off the level, opening the way
///   east and down to the lower terrace.
pub const DOWN: [&str; 1] = ["IGC_AI_Intro"];

/// [`DOWN`] on 208 alone.
pub fn down(level: &str) -> impl Iterator<Item = &'static str> {
    let on = level == LEVEL;
    DOWN.iter().copied().filter(move |_| on)
}

/// The first beam puzzle, the terrace's way west to the river (openspec
/// AS.431), by name; the clones' templates listed so their scripts run:
/// - **The first ice block's scene** (`IGC_FirstIceBlock`,
///   `208_FirstIceBlockIGC.ai`). It holds the start's beam off while it
///   pans to the block and the AI speaks, then lets it on.
/// - **The start** (`Reflect_Start(1)`, `Reflect_Start.ai`) hangs an
///   emitter on itself, facing the pivot.
/// - **The pivot** (`Reflect_Pivot_Round(1)`, `Reflect_Round.ai`) makes its
///   switch (a `Reflect_Switchbase_ICE` clone at its first waypoint) and
///   hangs a target, an emitter and a crystal on its three nodes. Each press
///   of the talk button at the switch turns it 45°. Its target lit, it
///   powers its own emitter along its facing.
/// - **An emitter** (`Reflect_Emitter.ai`) casts its ray along its facing and
///   powers the first crystal or ice block in its 6° cone within reach. The
///   pivot's at its fourth facing (264°) reaches the ice block
///   (`Melting_Ice_blocks_reflect_1`, `Ice_Cube.ai`), which melts while it's
///   powered and takes itself off.
pub const BEAM: [&str; 7] = [
    "IGC_FirstIceBlock",
    "Reflect_Start(1)",
    "Reflect_Pivot_Round(1)",
    "Melting_Ice_blocks_reflect_1",
    "Reflect_Switchbase_ICE",
    "Reflect_Target",
    "Reflect_Emitter",
];

/// [`BEAM`] on 208 alone: the other levels' reflect puzzles are their own.
pub fn beam(level: &str) -> impl Iterator<Item = &'static str> {
    let on = level == LEVEL;
    BEAM.iter().copied().filter(move |_| on)
}

/// What fights on level 208 (openspec AS.345), walked as creatures: the
/// reveal's five Ice Chompies, then Setup_002's controller, its six foes
/// (`Setup_002_Enemies`) and Noodles' scene at its first gate.
pub const WAY: Encounter = Encounter {
    level: LEVEL,
    placements: &[
        "Enemy_Chompy_Ice(2)",
        "Enemy_Chompy_Ice(3)",
        "Enemy_Chompy_Ice(20)",
        "Enemy_Chompy_Ice(21)",
        "Enemy_Chompy_Ice(40)",
        "Setup_002_Controller",
        "Enemy_Cyclops_Near(20)",
        "Enemy_Cyclops_Near(21)",
        "Enemy_Cyclops_Near(22)",
        "Enemy_Cyclops_Near(23)",
        "Enemy_Cyclops_Near(24)",
        "Enemy_Biclops_Heavy_S2(1)",
        "IGC_Noodles_FirstGate",
    ],
    clones: &[],
    steered: &[],
};

#[cfg(test)]
mod tests {
    use guac_script::Mode;

    use super::*;
    use crate::scripts::{Files, encounters, level_config};

    /// openspec AS.344: the exit's solids, the wall's bricks and the
    /// reveal's five Chompies are on 208's listing and on no other level's
    /// (205 holds walls and Chompies of its own, 202 none); since AS.345
    /// the Chompies are walked as [`WAY`]'s, no longer drawn as people.
    #[test]
    fn level_208s_exit_is_on_its_listing_alone() {
        let files = Files::default();
        let on = level_config(Mode::Play, &files, 5, LEVEL);
        assert_eq!(exit(LEVEL).count(), 9);
        for other in ["level_202", "level_205_piratecards"] {
            let off = level_config(Mode::Play, &files, 5, other);
            for name in EXIT.iter().chain(&REVEAL_FOES) {
                assert!(on.whitelist.lists(name, "C:/l/Unlisted.ai"), "{name}");
                assert!(
                    !off.whitelist.lists(name, "C:/l/Unlisted.ai"),
                    "{name} on {other}"
                );
            }
            assert_eq!(exit(other).count(), 0);
        }
        // Walked, not drawn as its people (openspec AS.345).
        for name in REVEAL_FOES {
            assert!(WAY.placements.contains(&name), "{name}");
            assert!(!crate::scripts::is_level_person(LEVEL, name), "{name}");
        }
        // The Ice Chompies' own script stays off for the level's others
        // (AS.345's).
        let ice = "C:/tfb/Content/Levels/_Enemies/Chompy_Ice/scripts/S2_Enemy_IceChompy.ai";
        assert!(!on.whitelist.lists("Enemy_Chompy_Ice(1)", ice));
    }

    /// openspec AS.406: the AI's hint is on 208's listing and on no other
    /// level's.
    #[test]
    fn level_208s_way_down_is_on_its_listing_alone() {
        let files = Files::default();
        let on = level_config(Mode::Play, &files, 5, LEVEL);
        assert_eq!(down(LEVEL).collect::<Vec<_>>(), DOWN);
        for other in ["level_202", "level_205_piratecards"] {
            let off = level_config(Mode::Play, &files, 5, other);
            for name in DOWN {
                assert!(on.whitelist.lists(name, "C:/l/Unlisted.ai"), "{name}");
                assert!(
                    !off.whitelist.lists(name, "C:/l/Unlisted.ai"),
                    "{name} on {other}"
                );
            }
            assert_eq!(down(other).count(), 0);
        }
    }

    /// openspec AS.431: the first beam puzzle's placements and clone
    /// templates are on 208's listing and on no other level's.
    #[test]
    fn level_208s_beam_puzzle_is_on_its_listing_alone() {
        let files = Files::default();
        let on = level_config(Mode::Play, &files, 5, LEVEL);
        assert_eq!(beam(LEVEL).collect::<Vec<_>>(), BEAM);
        for other in ["level_202", "level_205_piratecards"] {
            let off = level_config(Mode::Play, &files, 5, other);
            for name in BEAM {
                assert!(on.whitelist.lists(name, "C:/l/Unlisted.ai"), "{name}");
                assert!(
                    !off.whitelist.lists(name, "C:/l/Unlisted.ai"),
                    "{name} on {other}"
                );
            }
            assert_eq!(beam(other).count(), 0);
        }
    }

    /// openspec AS.345: 208's fights are its one encounter, walked there
    /// alone: every placement of [`WAY`] on its listing and on no other
    /// level's but where that level names its own (205's Setup_002 shares
    /// names with 208's).
    #[test]
    fn level_208s_fights_are_on_its_listing_alone() {
        let files = Files::default();
        let on = level_config(Mode::Play, &files, 5, LEVEL);
        assert_eq!(encounters(LEVEL).collect::<Vec<_>>(), vec![&WAY]);
        for other in ["level_202", "level_205_piratecards"] {
            assert!(encounters(other).all(|e| *e != WAY), "{other}");
            let off = level_config(Mode::Play, &files, 5, other);
            for name in WAY.placements {
                assert!(on.whitelist.lists(name, "C:/l/Unlisted.ai"), "{name}");
                if crate::level_205::WAY.placements.contains(name) && other != "level_202" {
                    continue;
                }
                assert!(
                    !off.whitelist.lists(name, "C:/l/Unlisted.ai"),
                    "{name} on {other}"
                );
            }
        }
        for name in ["Setup_002_Controller", "Enemy_Biclops_Heavy_S2(1)"] {
            assert!(WAY.placements.contains(&name), "{name}");
        }
    }
}
