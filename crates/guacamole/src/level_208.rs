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

/// The Ice Chompy's freeze crystal (openspec AS.432), listed and never
/// walked: the freeze hangs a `Chompy_Ice_Prison` clone on its victim, and
/// its own script takes it off him once he's no longer frozen (off his
/// node, faded out, a burst, gone). Unlisted it ran nothing, so the crystal
/// stayed hung on the hero long after the Chompies were gone.
pub const FREEZE: [&str; 1] = ["Chompy_Ice_Prison"];

/// [`FREEZE`] on 208 alone.
pub fn freeze(level: &str) -> impl Iterator<Item = &'static str> {
    let on = level == LEVEL;
    FREEZE.iter().copied().filter(move |_| on)
}

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

/// The way on west from Setup_002's spike gate (openspec AS.346): the
/// brick wall across the river past it (`Wall_Spawner_Template(9)`,
/// `Brick_Wall_Spawner`, the exit's [`EXIT`] wall's family). Unlisted its
/// box stands solid and lays no bricks, an invisible wall no attack beats;
/// listed it lays its bricks once a player comes in range and a few hits
/// blow them apart and take its box off, as the plaza's.
pub const RIVER: [&str; 1] = ["Wall_Spawner_Template(9)"];

/// [`RIVER`] on 208 alone.
pub fn river(level: &str) -> impl Iterator<Item = &'static str> {
    let on = level == LEVEL;
    RIVER.iter().copied().filter(move |_| on)
}

/// Under the river's north edge (openspec AS.757): the level mesh's wall
/// there leaves a 4 m gap (x -30.5, z -31 to -27) where the river's floor
/// runs out to a 20 m drop onto the ground below, which holds loot and the
/// heavy Biclops' start. The level's way back up is the jump pad at the
/// drop's foot (`PvP_JumpPad_NORMAL(3)`, `PR_PvP_jumpPad.ai`, 201's and
/// chapter 1's): unlisted, a hero who went over stayed down there.
pub const UNDER_RIVER: [&str; 1] = ["PvP_JumpPad_NORMAL(3)"];

/// [`UNDER_RIVER`] on 208 alone (201 and chapter 1 name pads of the same
/// name of their own).
pub fn under_river(level: &str) -> impl Iterator<Item = &'static str> {
    let on = level == LEVEL;
    UNDER_RIVER.iter().copied().filter(move |_| on)
}

/// The second beam puzzle (openspec AS.701's set 2), past the river wall,
/// listed by name as [`BEAM`] lists the first (the reflect scripts are
/// shared, and listed by path they'd wake every set on every level that
/// places one, read or not):
/// - **The start** (`Reflect_Start(2)`) powers at once and beams to the
///   pivot (`Reflect_Pivot_Round(2)`), whose switch turns it 45° a press.
/// - **The push block's crystal** (`Snow_Push_Block_Art_Crystal`,
///   `Reflect_Square.ai`): the art `Push_Block_Template(1)` (on the generic
///   listing) spawns on itself, hanging a target, an emitter and a crystal
///   on its three nodes. Pushed twice west to its track's end it stands in
///   the pivot's beam at its second facing, and its emitter beams west at
///   the ice block (`Melting_Ice_blocks_reflect_2`).
pub const BEAM_2: [&str; 4] = [
    "Reflect_Start(2)",
    "Reflect_Pivot_Round(2)",
    "Melting_Ice_blocks_reflect_2",
    "Snow_Push_Block_Art_Crystal",
];

/// [`BEAM_2`] on 208 alone.
pub fn beam_2(level: &str) -> impl Iterator<Item = &'static str> {
    let on = level == LEVEL;
    BEAM_2.iter().copied().filter(move |_| on)
}

/// West of the second ice block, by name (openspec AS.776): each solid to
/// actors and unlisted was an invisible wall, a box standing that no
/// attack beat.
/// - **Two brick walls** (`Wall_Spawner_Template(6)` and `(7)`,
///   `Brick_Wall_Spawner`) across the way west: listed, each lays its
///   bricks as he comes in range and a few hits take it off, as the
///   plaza's and the river's.
/// - **The third beam set** (`Reflect_Start(3)`, `Reflect_Start.ai`, and
///   `Melting_Ice_blocks_reflect_3`, `Ice_Cube.ai`): its start beams east
///   across the push block's track (`Push_Block_Template(9)`, on the
///   generic listing, its crystal art [`BEAM_2`]'s template); pushed south
///   twice into the beam, the crystal relays it south and the block melts.
pub const WEST: [&str; 4] = [
    "Wall_Spawner_Template(6)",
    "Wall_Spawner_Template(7)",
    "Reflect_Start(3)",
    "Melting_Ice_blocks_reflect_3",
];

/// [`WEST`] on 208 alone.
pub fn west(level: &str) -> impl Iterator<Item = &'static str> {
    let on = level == LEVEL;
    WEST.iter().copied().filter(move |_| on)
}

/// The fourth beam set, by name (openspec AS.776, AS.796): its ice block
/// (`Melting_Ice_blocks_reflect_4`, `Ice_Cube.ai`) stands across the way
/// to Setup_003's first trigger, unlisted an invisible wall. Listed it
/// stands as an ice block, its start (`Reflect_Start(4)`) beams to the
/// pivot (`Reflect_Pivot_Round(3)`), whose switch turns it 45° a press, and
/// the post (`Reflect_Post(1)`) faces the block. The post is fed by
/// [`WEST`]'s push block taken back: its script snaps its track to 15 ft
/// steps from its own placed spot and starts it two steps west, so after
/// the third block melts it goes back north and east to that spot, where
/// the pivot's first turn lights its crystal and the crystal beams south
/// to the post.
pub const SET_4: [&str; 4] = [
    "Reflect_Start(4)",
    "Reflect_Pivot_Round(3)",
    "Reflect_Post(1)",
    "Melting_Ice_blocks_reflect_4",
];

/// [`SET_4`] on 208 alone.
pub fn set_4(level: &str) -> impl Iterator<Item = &'static str> {
    let on = level == LEVEL;
    SET_4.iter().copied().filter(move |_| on)
}

/// The way down to Setup_004's second trigger, by name (openspec AS.798):
/// the `Blocker` (`Blocker_minions.ai`, a box 35 by 35 ft on the slope
/// below Setup_004's first trigger). Each step its script asks whether a
/// player stands within its 70 ft on its facing side: none, its box is
/// solid to actors; one, it's off. Unlisted its box stood solid always, an
/// invisible wall that held him at (-170.7, 5.9, 39.3) and with him
/// Setup_004's circling Chompies, which wait on that trigger.
pub const WAY_DOWN: [&str; 1] = ["Blocker"];

/// [`WAY_DOWN`] on 208 alone.
pub fn way_down(level: &str) -> impl Iterator<Item = &'static str> {
    let on = level == LEVEL;
    WAY_DOWN.iter().copied().filter(move |_| on)
}

/// Setup_004's jump pads and their activator, by name (openspec AS.1010,
/// AS.1014). The yard's two bomb throwers walk their paths onto ledges 3.8 m
/// and 5.4 m over its floor, out of a walk's reach; a kid beats them with
/// their own bombs (each grenade lands as a `Bomb_Rocket_Template.ai` bomb
/// he walks into and throws with △). `_JumpPad_Activator(1)`
/// (`JumpPad_Activator.ai`) puts both bombers in `Bomb_Targets` at its
/// start, the set a thrown bomb flies to, and waits on the yard's whole set
/// (`Setup_004_Enemies`); then it pans to pad (4) on the floor and pad (5)
/// on bomber (2)'s ledge, turns each on with its appear effect, wakes the
/// post-fight push block and takes itself off. Pad (4) lands a hero on the
/// ledge, pad (5) on the upper level west of it, the way on. The pads are
/// `PR_PvP_jumpPad.ai`'s, as [`UNDER_RIVER`]'s. Unlisted, the pads never
/// came on and a bomb had to be aimed (headless 1,122 steps to beat both
/// against 390 with it, as-208-battle).
pub const YARD_PADS: [&str; 3] = [
    "_JumpPad_Activator(1)",
    "PvP_JumpPad_NORMAL(4)",
    "PvP_JumpPad_NORMAL(5)",
];

/// [`YARD_PADS`] on 208 alone (201 and chapter 1 name pads of the same
/// name of their own).
pub fn yard_pads(level: &str) -> impl Iterator<Item = &'static str> {
    let on = level == LEVEL;
    YARD_PADS.iter().copied().filter(move |_| on)
}

/// Level 208's snowmen, by name (openspec AS.1021): the nine placed
/// `Snowman_Whole.ai`s, each a capsule 8 ft wide and 12 ft high solid to
/// actors. Its script waits until a player's minion runs into it or hurts
/// it by more than 5, then bursts into its parts and loot and takes itself
/// off. Unlisted each stood for good: `Snowman(7)` beside Setup_004's
/// second trigger walled the yard off from a Giant at the 14 ft his own
/// scripts set his solid to, and `Snowman(9)` stands in Setup_002's fight.
pub const SNOWMEN: [&str; 9] = [
    "Snowman(1)",
    "Snowman(2)",
    "Snowman(3)",
    "Snowman(4)",
    "Snowman(5)",
    "Snowman(6)",
    "Snowman(7)",
    "Snowman(8)",
    "Snowman(9)",
];

/// [`SNOWMEN`] on 208 alone.
pub fn snowmen(level: &str) -> impl Iterator<Item = &'static str> {
    let on = level == LEVEL;
    SNOWMEN.iter().copied().filter(move |_| on)
}

/// The scripts a burst snowman's parts run (openspec AS.1021): its carrot
/// and pipe, its arms and its hat bounce off and go.
pub const SNOWMAN_PARTS: [&str; 3] = [
    "/Level_208/Scripts/Snowman_carrot_bounce.ai",
    "/Level_208/Scripts/Snowman_arm_bounce.ai",
    "/Level_208/Scripts/Snowman_hat.ai",
];

/// [`SNOWMAN_PARTS`] on 208 alone.
pub fn snowman_parts(level: &str) -> impl Iterator<Item = &'static str> {
    let on = level == LEVEL;
    SNOWMAN_PARTS.iter().copied().filter(move |_| on)
}

/// What fights on level 208 (openspec AS.345), walked as creatures: the
/// reveal's five Ice Chompies, then Setup_002's controller, its six foes
/// (`Setup_002_Enemies`) and Noodles' scene at its first gate; then
/// Setup_003's controller (openspec AS.402), the nine foes its script
/// names (`enemy a` to `enemy i`: the bomb thrower and eight Cyclopes) and
/// the bomber's reveal by the fourth ice block; then Setup_004's
/// controller (openspec AS.403, AS.798), the fifteen its script names
/// (`enemy a` to `enemy o`: seven Ice Chompies, two heavy Biclopes, two
/// bomb throwers, four Cyclopes) and the two Ice Chompies it doesn't name,
/// (8) and (9): the level's `Setup_004_Enemies` holds all seventeen, its
/// `First Chompies` holds (8) and (9) with (11) to (13) (the count its
/// circling Chompies wait on), and the controller's group comes off only
/// once every one of `Setup_004_Enemies` is (openspec AS.1020). Woken on its first trigger, it circles
/// four of the Chompies round `Setup_004_Anchor` until a player reaches
/// its second trigger down the slope ([`WAY_DOWN`]), then sends them, the
/// Cyclopes, the Biclopes and the bombers in.
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
        "Setup_003_Controller",
        "Enemy_BombThrower(3)",
        "Enemy_Cyclops_Near(4)",
        "Enemy_Cyclops_Near(5)",
        "Enemy_Cyclops_Near(16)",
        "Enemy_Cyclops_Near(17)",
        "Enemy_Cyclops_Near(18)",
        "Enemy_Cyclops_Near(19)",
        "Enemy_Cyclops_Near(25)",
        "Enemy_Cyclops_Near(26)",
        "Cyclops_Bomber_IGC_Trigger",
        "Setup_004_Controller",
        "Enemy_Chompy_Ice(1)",
        "Enemy_Chompy_Ice(11)",
        "Enemy_Chompy_Ice(12)",
        "Enemy_Chompy_Ice(13)",
        "Enemy_Chompy_Ice(14)",
        "Enemy_Chompy_Ice(15)",
        "Enemy_Chompy_Ice(16)",
        "Enemy_Biclops_Heavy_S2(3)",
        "Enemy_Biclops_Heavy_S2(4)",
        "Enemy_BombThrower(1)",
        "Enemy_BombThrower(2)",
        "Enemy_Cyclops_Near(1)",
        "Enemy_Cyclops_Near(2)",
        "Enemy_Cyclops_Near(3)",
        "Enemy_Cyclops_Near(6)",
        "Enemy_Chompy_Ice(8)",
        "Enemy_Chompy_Ice(9)",
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
        // (AS.345's): the big battle's, still unlisted, on its ledge;
        // Setup_004's (AS.403, AS.1020) are walked by name.
        let ice = "C:/tfb/Content/Levels/_Enemies/Chompy_Ice/scripts/S2_Enemy_IceChompy.ai";
        assert!(!on.whitelist.lists("Enemy_Chompy_Ice(25)", ice));
        for n in [
            "Enemy_Chompy_Ice(1)",
            "Enemy_Chompy_Ice(8)",
            "Enemy_Chompy_Ice(9)",
        ] {
            assert!(WAY.placements.contains(&n), "{n}");
        }
    }

    /// openspec AS.1010: the yard's jump pads and their activator are on
    /// 208's listing and on no other level's that doesn't name pads of its
    /// own (202 and 205 have no `PvP_JumpPad_NORMAL`).
    #[test]
    fn level_208s_yard_pads_are_on_its_listing_alone() {
        let files = Files::default();
        let on = level_config(Mode::Play, &files, 5, LEVEL);
        assert_eq!(yard_pads(LEVEL).collect::<Vec<_>>(), YARD_PADS);
        for other in ["level_202", "level_205_piratecards"] {
            let off = level_config(Mode::Play, &files, 5, other);
            for name in YARD_PADS {
                assert!(on.whitelist.lists(name, "C:/l/Unlisted.ai"), "{name}");
                assert!(
                    !off.whitelist.lists(name, "C:/l/Unlisted.ai"),
                    "{name} on {other}"
                );
            }
            assert_eq!(yard_pads(other).count(), 0);
        }
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

    /// openspec AS.432: the freeze crystal's template is on 208's listing
    /// and on no other level's.
    #[test]
    fn level_208s_freeze_crystal_is_on_its_listing_alone() {
        let files = Files::default();
        let on = level_config(Mode::Play, &files, 5, LEVEL);
        assert_eq!(freeze(LEVEL).collect::<Vec<_>>(), FREEZE);
        for other in ["level_202", "level_205_piratecards"] {
            let off = level_config(Mode::Play, &files, 5, other);
            for name in FREEZE {
                assert!(on.whitelist.lists(name, "C:/l/Unlisted.ai"), "{name}");
                assert!(
                    !off.whitelist.lists(name, "C:/l/Unlisted.ai"),
                    "{name} on {other}"
                );
            }
            assert_eq!(freeze(other).count(), 0);
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

    /// openspec AS.346, AS.701: the river wall and the second beam puzzle
    /// are on 208's listing and on no other level's.
    #[test]
    fn level_208s_river_wall_and_second_beam_are_on_its_listing_alone() {
        let files = Files::default();
        let on = level_config(Mode::Play, &files, 5, LEVEL);
        assert_eq!(river(LEVEL).collect::<Vec<_>>(), RIVER);
        assert_eq!(beam_2(LEVEL).collect::<Vec<_>>(), BEAM_2);
        for other in ["level_202", "level_205_piratecards"] {
            let off = level_config(Mode::Play, &files, 5, other);
            for name in RIVER.iter().chain(&BEAM_2) {
                assert!(on.whitelist.lists(name, "C:/l/Unlisted.ai"), "{name}");
                assert!(
                    !off.whitelist.lists(name, "C:/l/Unlisted.ai"),
                    "{name} on {other}"
                );
            }
            assert_eq!(river(other).count() + beam_2(other).count(), 0);
        }
        // The river's wall and the push block's crystal by their own names.
        for name in ["Wall_Spawner_Template(9)", "Snow_Push_Block_Art_Crystal"] {
            assert!(on.whitelist.lists(name, "C:/l/Unlisted.ai"), "{name}");
        }
        // 208's other walls and sets stay off until a kid's way reaches
        // them.
        for name in ["Wall_Spawner_Template(8)", "Reflect_Start(5)"] {
            assert!(!on.whitelist.lists(name, "C:/l/Unlisted.ai"), "{name}");
        }
    }

    /// openspec AS.776: the way west of the second ice block is on 208's
    /// listing and on no other level's; the fourth set, the wall past it
    /// and the ice traps stay off.
    #[test]
    fn level_208s_way_west_and_its_traps_are_on_its_listing_alone() {
        let files = Files::default();
        let on = level_config(Mode::Play, &files, 5, LEVEL);
        assert_eq!(west(LEVEL).collect::<Vec<_>>(), WEST);
        for other in ["level_202", "level_205_piratecards"] {
            let off = level_config(Mode::Play, &files, 5, other);
            for name in WEST {
                assert!(on.whitelist.lists(name, "C:/l/Unlisted.ai"), "{name}");
                assert!(
                    !off.whitelist.lists(name, "C:/l/Unlisted.ai"),
                    "{name} on {other}"
                );
            }
            assert_eq!(west(other).count(), 0);
        }
        for name in [
            "Wall_Spawner_Template(6)",
            "Wall_Spawner_Template(7)",
            "Melting_Ice_blocks_reflect_3",
            "Reflect_Start(3)",
        ] {
            assert!(on.whitelist.lists(name, "C:/l/Unlisted.ai"), "{name}");
        }
        for name in [
            "Wall_Spawner_Template(8)",
            "Reflect_Start(5)",
            "Melting_Ice_blocks_reflect_5",
            "Ice_Trap_Large(1)",
        ] {
            assert!(!on.whitelist.lists(name, "C:/l/Unlisted.ai"), "{name}");
        }
    }

    /// openspec AS.757: the jump pad under the river's north edge is on
    /// 208's listing and on no other level's (201 and chapter 1 name their
    /// own pads by the same name).
    #[test]
    fn level_208s_pad_under_the_river_is_on_its_listing_alone() {
        let files = Files::default();
        let on = level_config(Mode::Play, &files, 5, LEVEL);
        assert_eq!(under_river(LEVEL).collect::<Vec<_>>(), UNDER_RIVER);
        for name in UNDER_RIVER {
            assert!(on.whitelist.lists(name, "C:/l/Unlisted.ai"), "{name}");
        }
        for other in ["level_202", "level_205_piratecards"] {
            let off = level_config(Mode::Play, &files, 5, other);
            for name in UNDER_RIVER {
                assert!(
                    !off.whitelist.lists(name, "C:/l/Unlisted.ai"),
                    "{name} on {other}"
                );
            }
            assert_eq!(under_river(other).count(), 0);
        }
    }

    /// openspec AS.776: the fourth beam set is on 208's listing and on no
    /// other level's.
    #[test]
    fn level_208s_fourth_set_is_on_its_listing_alone() {
        let files = Files::default();
        let on = level_config(Mode::Play, &files, 5, LEVEL);
        assert_eq!(set_4(LEVEL).collect::<Vec<_>>(), SET_4);
        for name in SET_4 {
            assert!(on.whitelist.lists(name, "C:/l/Unlisted.ai"), "{name}");
        }
        for other in ["level_202", "level_205_piratecards"] {
            let off = level_config(Mode::Play, &files, 5, other);
            for name in SET_4 {
                assert!(
                    !off.whitelist.lists(name, "C:/l/Unlisted.ai"),
                    "{name} on {other}"
                );
            }
            assert_eq!(set_4(other).count(), 0);
        }
    }

    /// openspec AS.798: the way down to Setup_004's second trigger is on
    /// 208's listing and on no other level's.
    #[test]
    fn level_208s_way_down_to_setup_004_is_on_its_listing_alone() {
        let files = Files::default();
        let on = level_config(Mode::Play, &files, 5, LEVEL);
        assert_eq!(way_down(LEVEL).collect::<Vec<_>>(), WAY_DOWN);
        assert!(on.whitelist.lists("Blocker", "C:/l/Unlisted.ai"));
        for other in ["level_202", "level_205_piratecards"] {
            let off = level_config(Mode::Play, &files, 5, other);
            assert!(
                !off.whitelist.lists("Blocker", "C:/l/Unlisted.ai"),
                "{other}"
            );
            assert_eq!(way_down(other).count(), 0);
        }
    }

    /// openspec AS.1021: 208's snowmen and their parts' scripts are on its
    /// listing and on no other level's.
    #[test]
    fn level_208s_snowmen_are_on_its_listing_alone() {
        let files = Files::default();
        let on = level_config(Mode::Play, &files, 5, LEVEL);
        let whole = "C:/tfb/Content/Levels/Level_208/Scripts/Snowman_Whole.ai";
        for n in SNOWMEN {
            assert!(on.whitelist.lists(n, whole), "{n}");
        }
        assert!(!on.whitelist.lists("Snowman", whole), "the template");
        for part in ["Snowman_carrot_bounce", "Snowman_arm_bounce", "Snowman_hat"] {
            let path = format!("C:/tfb/Content/Levels/Level_208/Scripts/{part}.ai");
            assert!(on.whitelist.lists("Snowman_Carrot", &path), "{part}");
        }
        for other in ["level_202", "level_205_piratecards"] {
            let off = level_config(Mode::Play, &files, 5, other);
            assert!(!off.whitelist.lists("Snowman(7)", whole), "{other}");
            assert_eq!(snowmen(other).count() + snowman_parts(other).count(), 0);
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
