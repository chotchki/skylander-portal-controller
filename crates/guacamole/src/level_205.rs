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

use guac_script::{Value, Vm};

use crate::scripts::{Encounter, ScriptWorld};

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

/// The way on from the first yard to Setup_002's end (openspec AS.325), by
/// placement name, listed and never walked; the doors, keys, push blocks,
/// spike gates and rooms on it run by their scripts on every level already.
/// In order:
/// - **The jump pad.** Setup_001's five gone, its activator (woken within
///   its range) lays the pad below the key ledge south of the yard; on it a
///   hero is thrown up onto the ledge and takes the first key.
/// - **The key door.** Off the ledge, the key turned in the door west of
///   the yard (`Door_Template(3)`, △) opens the market, where the armored
///   Chompy's intro plays and its Chompies come at him.
/// - **Blobbers' crane.** Its wheel (`SpinCrank2`, △) swings the crane
///   (`Crane(2)`, its mast `Crane_Head(1)`) between west and south; its
///   boat carries a block between them, a block riding it being drawn by
///   the crate (`FAke_Block`s) and woken again at the other end as that
///   end's push block. Turned south, the block by the boat's north end is
///   pushed onto it; turned west, that block is pushed off its far end
///   into the channel; turned south again, the boat bridges the gap and
///   the block past it is pushed into the channel there, a way into
///   Setup_002's yard.
/// - **The card house.** In the yard Setup_002 wakes; a pirate with a key
///   laughs and runs into the card house, its door (`Interior_Door(3)`)
///   swinging open for him, and the house's room takes the hero in. There
///   Fangs plays his card game; won, his scene wakes the first gate killer
///   (`Gate_Killer`), whose leaving lowers the gate before the key; the key
///   taken, the second gate killer goes and the gate by the door with it,
///   unlisted (the walk is the same to the step with it listed).
///   Carried back to the yard, the key sets Setup_002's fight off; beaten,
///   past its far trigger, its controller goes.
pub const STRETCH_1: [&str; 11] = [
    "SpecialJumpPadActivator_For_JumpPad08",
    "PvP_JumpPad_SUPER(8)",
    "Crane(2)",
    "Crane_Head(1)",
    "SpinCrank2",
    "FAke_Block",
    "FAke_Block(1)",
    "FAke_Block(2)",
    "Fake_Card_Pirate_1",
    "Interior_Door(3)",
    "Gate_Killer",
];

/// The placements [`STRETCH_1`] lists for the level whose archive is
/// `level`: none on another level.
pub fn stretch_1(level: &str) -> &'static [&'static str] {
    match level {
        LEVEL => &STRETCH_1,
        _ => &[],
    }
}

/// The way on from Setup_002's end toward Setup_003's end (openspec
/// AS.326), by placement name, listed and never walked; the key gate, the
/// crane, Setup_003, its house's door and room, Cali and her scene at the
/// galleon were on the listing already. In order:
/// - **The key gate.** The channel west of Blobbers' crane is bridged by
///   its boat turned west, the block pushed off its far end there (AS.325)
///   the way on to the key gate (`Door_Template(6)`, △), which the card
///   house's key opens.
/// - **Setup_003.** Past the gate, a hero inside its combat area's signal
///   face (`CloseDoor`) closes the gate behind him, takes the crane, the
///   last block and the start's people off, gives Cali her range and tells
///   Setup_003's controller its enemies attack: they come out of the house
///   by the galleon (`Interior_Door(4)`) in parts, twelve of them.
/// - **Cali at the galleon.** The twelve off, her scene there plays (she
///   walks onto the dock and points out the gate, the galleon and the chain
///   pull) and hands her back at her place, where she waits on the galleon
///   pulled in to the dock by the chain pull, a Giant's feat the TV doesn't
///   run (AS.371); Setup_003's controller waits on the gate's blowing up
///   after it.
///
/// Left off: the signal face by the gate (`Remove_Part1`), which widens the
/// co-op camera tether to 95 ft once the gate is open and goes, nothing a
/// one-player walk meets; the chain pull's peg is [`CHAIN_PULL`]'s.
pub const STRETCH_2: [&str; 1] = ["CloseDoor"];

/// The placements [`STRETCH_2`] lists for the level whose archive is
/// `level`: none on another level.
pub fn stretch_2(level: &str) -> &'static [&'static str] {
    match level {
        LEVEL => &STRETCH_2,
        _ => &[],
    }
}

/// The Giant's chain pull at the galleon (openspec AS.371), which Cali
/// waits on: the peg (`TitanChainPeg(1)`, the shared `TitanChainPull.ai`),
/// whose start hangs the chain's template actor (`Titan_Chain`, no script of
/// its own) on the peg and the galleon (`Titan_Chain_Boat`, the level's
/// definition names it the main island) on the chain's node 0, the airship
/// floating out on the chain's idle. Within 40 ft of the talk point by the
/// peg every hero gets the level's need-a-Giant hint; △ there locks a Giant
/// in (a hero who isn't one is refused and keeps the hint), turns him into
/// place, puts the mash prompt up and counts each press of the mash; once
/// the count reaches 300 the pull's own camera (`DefaultAnimatorCamera`)
/// and the chain's pull clip play, the galleon riding node 0 in to 141.7 ft
/// of the peg, and the peg sets the value to set on completion its level
/// hands it (205's hands it none: Cali reads the distance). The pose
/// at the clip's end holds (`hands::posing`, stand-in ledger: `held-clip`).
pub const CHAIN_PULL: [&str; 1] = ["TitanChainPeg(1)"];

/// The placements [`CHAIN_PULL`] lists for the level whose archive is
/// `level`: none on another level.
pub fn chain_pull(level: &str) -> &'static [&'static str] {
    match level {
        LEVEL => &CHAIN_PULL,
        _ => &[],
    }
}

/// The way on from the galleon pulled in to Setup_003's end (openspec
/// AS.372), by placement name, listed and never walked; the galleon's
/// cannon (`Push_Block_Template(6)`, one waypoint: a push block that never
/// moves, the cannon on it), Cali, Setup_003 and its three Cyclopes were on
/// the listing already. In order:
/// - **Aboard.** The galleon in, Cali takes the dock's collision off and
///   wakes the cannon; up onto the deck (the level's own collision where
///   the galleon docks), a hero within 35 ft of her trigger there
///   (`CaliTrigger02`, no script) sets her pan to the gate off.
/// - **The cannon.** △ by it fires its ball along its barrel, south-east
///   over the bluff toward the gate south of Setup_003's yard
///   ([`FLAT_SHOTS`]); Cali reads the fire, wakes the coins past the gate
///   and sets her `pan done`, the gate's blowing up Setup_003's controller
///   waits on.
/// - **The gate.** The ball hits the gate's blocker
///   (`Breakable_Metal_Wall(12)`, `FakeWall.ai`, 55 ft across and unseen,
///   one of the targets a cannon's ball breaks), which goes 3 s on; its
///   controller (`ChainedGate_Controller(1)`, `205_PadLock_Controller.ai`)
///   then blows the gate itself (`Breakable_Metal_Wall(3)`,
///   `205_FakeWall.ai`: its pieces burst out and it goes) and breaks the
///   padlock (`Padlock_Target_03`), whose four chains (`Chain_01` to
///   `_04`) fall with it.
/// - **Setup_003's last three.** Its Cyclopes m to o react to the blast and
///   walk their paths through the gate to their markers in the yard;
///   beaten, the controller goes.
pub const GALLEON: [&str; 8] = [
    "Breakable_Metal_Wall(12)",
    "ChainedGate_Controller(1)",
    "Breakable_Metal_Wall(3)",
    "Padlock_Target_03",
    "Chain_01",
    "Chain_02",
    "Chain_03",
    "Chain_04",
];

/// The placements [`GALLEON`] lists for the level whose archive is
/// `level`: none on another level.
pub fn galleon(level: &str) -> &'static [&'static str] {
    match level {
        LEVEL => &GALLEON,
        _ => &[],
    }
}

/// The way on from Setup_003's end to Setup_004's end (openspec AS.327),
/// by placement name, listed and never walked; the gate's pirate, the
/// jailed pirate and his cell's scene, the push cannon
/// (`Push_Block_Template(9)`, its cannon on the shared push-block and
/// cannon scripts), the gate (`Door_Template(1)`), Setup_004, its foes and
/// the town's push blocks were on the listing already. In order:
/// - **The gate's pirate.** Past the blown gate Cali's script has woken
///   him, the cell's scene, the lever and the walls; a hero within 75 ft
///   plays his intro (he laughs at him from behind his gate), and he walks
///   to his gate and wakes the push cannon on its track.
/// - **The lever's wall.** The cannon pushed west along its track and
///   fired (△) breaks the metal wall (`Breakable_Metal_Wall(1)`,
///   `MetalWall.ai`) between the track and the turntable's lever.
/// - **The turntable.** Until it turns, the turntable
///   (`Switch_Straight_Art_Template(1)`, `205_FakePlate.ai`, a switch in
///   the track's set) stops the cannon short of it; the lever
///   (`Switch_Controller_template(1)`, △) tells it to turn 90°, and the
///   cannon passes over it east.
/// - **The cell.** Pushed east to the track's end and fired, the cannon
///   breaks the jailed pirate's cell's wall (`Breakable_Metal_Wall(5)`);
///   his cell's scene plays his breakout, and he offers his card game,
///   which the TV doesn't have ([`jail_stand_in`] stands it in won for the
///   disc test); won, the scene tells the gate's pirate.
/// - **The gate.** A hero by the gate's pirate's checker plays his outro:
///   he opens his gate and walks the hero through, then shuts it.
/// - **Setup_004**, past the gate within 160 ft of its controller: its
///   Biclops' intro and its fights; it waits on the town's block, a bridge
///   of three push blocks: the east block pushed off its ledge
///   (`Push_Block_Template(3)`) is a way up onto the upper town, whose
///   block (`Push_Block_Template(12)`) pushed off its north ledge lands as
///   the town's block (`Push_Block_Template(14)`), pushed west to its
///   checker; then its left door's fights and the two lobbers on the
///   upper town, and its controller goes.
///
/// Left off: the signal faces past the blown gate and past the gate
/// (`Remove_Part2`, `Remove_Part3`), which take the stretch's people and
/// pieces off behind a hero (the gate's pirate and the cell's scene are
/// woken by Cali's script already), the cannon's blocker
/// (`Cannon_Blocker`, which only joins the set a push checks ahead of it,
/// 4.1 m off the cannon's one straight track, so never inside a push's
/// 29 ft, 20° cone; the walk the same to the step with it listed) and
/// Setup_004's two doors (`Interior_Door(8)`, `(6)`; its controller sets
/// its foes at their places itself), each walked the same without.
pub const STRETCH_3: [&str; 4] = [
    "Switch_Controller_template(1)",
    "Switch_Straight_Art_Template(1)",
    "Breakable_Metal_Wall(1)",
    "Breakable_Metal_Wall(5)",
];

/// The placements [`STRETCH_3`] lists for the level whose archive is
/// `level`: none on another level.
pub fn stretch_3(level: &str) -> &'static [&'static str] {
    match level {
        LEVEL => &STRETCH_3,
        _ => &[],
    }
}

/// What a broken gate or metal wall on level 205 throws (openspec
/// AS.1122), by template name (a clone runs under its template's): the
/// metal walls' pieces (`MetalWall_Piece.ai`: each flung at a random speed
/// under gravity, then faded and taken off) and the padlock gate's padlock,
/// its parts and its chain link, as level 214's padlock gate lists them.
/// Unlisted, the blown gate's pieces stood where they spawned, in the
/// gate's own shape.
pub const BROKEN_PIECES: [&str; 11] = [
    "Metal Wall Piece 1",
    "Metal Wall Piece 2",
    "Metal Wall Piece 3",
    "Metal Wall Piece 4",
    "MetalWall_Brick",
    "Padlock",
    "Padlock_Part_02",
    "Padlock_Part_03",
    "Padlock_Part_04",
    "Padlock_Part_05",
    "Chain_Link",
];

/// The placements [`BROKEN_PIECES`] lists for the level whose archive is
/// `level`: none on another level.
pub fn broken_pieces(level: &str) -> &'static [&'static str] {
    match level {
        LEVEL => &BROKEN_PIECES,
        _ => &[],
    }
}

/// Level 205's Giants' walls (openspec AS.1120, AS.1160): every metal wall
/// (`MetalWall.ai`) placed on the level but the two the push cannon breaks
/// ([`STRETCH_3`]). A Giant's own attacks break one (its hurt check takes a
/// Giant's titan hurt or a thrown boulder's) and nothing else does, and at
/// its STARTUP it raises its icons at its waypoints (`Elemental_Icon_Hint`,
/// which `Elemental_Symbol_Hints.ai` makes a `Giant_icon`, beside a
/// `Bomb_icon` or `Canon_Icon` by its waypoints' count): what tells a kid
/// without a Giant why it won't open. Interior 2's (`(6)`, which its
/// house's controller wakes with the room) bars the room's far side before
/// `Chest(1)`; `(4)` stands before `Chest(4)`, `(2)` before a loot bag and
/// coin piles. Unlisted, each stood inert with no icon and not even a
/// Giant got through.
pub const GIANT_WALLS: [&str; 3] = [
    "Breakable_Metal_Wall(6)",
    "Breakable_Metal_Wall(4)",
    "Breakable_Metal_Wall(2)",
];

/// The placements [`GIANT_WALLS`] lists for the level whose archive is
/// `level`: none on another level.
pub fn giant_walls(level: &str) -> &'static [&'static str] {
    match level {
        LEVEL => &GIANT_WALLS,
        _ => &[],
    }
}

/// The cannon's ball (openspec AS.372), whose clones fly without falling
/// until their script sets their gravity (stand-in ledger: `cannon-shot`):
/// the ball's STARTUP takes its fall off under a check of a variable the VM
/// reads switched off (guac-script's crate docs, Checks and flows), so it
/// falls from its 200 ft/s launch onto the bluff between the galleon and
/// the gate, 3.4 m under its muzzle, where flat it clears the bluff and
/// meets the gate's blocker. The ball's own later behaviours set its
/// gravity again once it hits.
pub const FLAT_SHOTS: [&str; 1] = ["Canon_Ball"];

/// The templates [`FLAT_SHOTS`] lists for the level whose archive is
/// `level`: none on another level.
pub fn flat_shots(level: &str) -> &'static [&'static str] {
    match level {
        LEVEL => &FLAT_SHOTS,
        _ => &[],
    }
}

/// The clones of [`STRETCH_2`]'s way a scene walks standing on the ground
/// under each step, as the level's people stand (`ScriptWorld::people`):
/// the galleon's encounter's Cali, whose body walk stops at the 0.25 m step
/// onto the dock (stand-in ledger: `scene-walk`). It stands the intro's
/// Cali too: her walk there within 0.68 m of her body walk's, at the same
/// place at its end.
pub const STRETCH_2_GROUNDED: [&str; 1] = ["Cali_NPC"];

/// The templates [`STRETCH_2_GROUNDED`] lists for the level whose archive
/// is `level`: none on another level.
pub fn stretch_2_grounded(level: &str) -> &'static [&'static str] {
    match level {
        LEVEL => &STRETCH_2_GROUNDED,
        _ => &[],
    }
}

/// Who of [`STRETCH_1`] the TV draws as one of the level's people, awake
/// on his own model: the pirate who runs into the card house.
pub const STRETCH_1_PEOPLE: [&str; 1] = ["Fake_Card_Pirate_1"];

/// The placements [`STRETCH_1_PEOPLE`] lists for the level whose archive
/// is `level`: none on another level.
pub fn stretch_1_people(level: &str) -> &'static [&'static str] {
    match level {
        LEVEL => &STRETCH_1_PEOPLE,
        _ => &[],
    }
}

/// The card house's pirate (Fangs), whose game the key behind the room's
/// gate waits on.
pub const CARD_HOUSE_PIRATE: &str = "CardGame_Template(2)";

/// Fangs' card game won where the TV has no card game (openspec AS.329;
/// the disc test's, stand-in ledger: `dev-entry`): a quit the TV's answer
/// left in his result turned to a game still running and the flag a won
/// game's theatrics end on set, as [`finale_stand_in`] does for
/// Dreadbeard's. From there his scene's outro, the gate killers and the
/// key are the level's own. Whether it stood in this step, on level unit
/// `unit`.
pub fn card_house_stand_in(vm: &mut Vm, world: &mut ScriptWorld, unit: u16) -> bool {
    won_where_quit(vm, world, unit, CARD_HOUSE_PIRATE)
}

/// The jailed pirate (the bandit), who challenges the hero to his card game
/// once the push cannon's ball breaks his cell's wall; his cell's scene
/// (`CardPirate_Bandit_Cine`) waits on that game won before it tells the
/// gate's pirate to let the hero by.
pub const JAIL_PIRATE: &str = "CardGame_Template(1)_Bandit";

/// The jailed pirate's card game won where the TV has no card game
/// (openspec AS.327; the disc test's, stand-in ledger: `dev-entry`), as
/// [`card_house_stand_in`] stands in Fangs': a quit turned to a game still
/// running and `CardTheatricsDone` set. From there his cell's scene, the
/// gate's pirate and the gate are the level's own. Whether it stood in
/// this step, on level unit `unit`.
pub fn jail_stand_in(vm: &mut Vm, world: &mut ScriptWorld, unit: u16) -> bool {
    won_where_quit(vm, world, unit, JAIL_PIRATE)
}

/// `pirate`'s quit card game turned to a game still running with
/// [`GAME_WON`] set: whether there was one to turn.
fn won_where_quit(vm: &mut Vm, world: &mut ScriptWorld, unit: u16, pirate: &str) -> bool {
    let Some(id) = world
        .placed_in(unit, pirate)
        .and_then(|h| vm.placement_of(h))
    else {
        return false;
    };
    if vm.local(id, GAME_RESULT).map(|v| vm.value(v)) != Some(Value::Int(GAME_QUIT)) {
        return false;
    }
    let Some(g) = vm.program().global_named(GAME_WON) else {
        return false;
    };
    vm.set_local(id, GAME_RESULT, Value::Int(0)) && vm.set_global(g, Value::Int(1))
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

/// Level 205's music (openspec AS.289), by their path under its folder, as
/// [`crate::scripts::MUSIC_SCRIPTS`] lists 201's; the level's ambience
/// (`snd_level_audio_environment.ai`) and the music's mute mix are already
/// that list's by file name. Each starts a track through the music macro:
/// - **The dynamic sound controller** (one): at its start the intro scene's
///   track, then on the music events the level's scripts raise its tracks
///   through the level (the gameplay intro once the intro hands over, the
///   thin explore bed after Cali and after the gate's blow-up, the snare,
///   the town's combat in the arena, the final battle's two and its
///   stingers, the outro's). It holds the flags the watchers read: in
///   combat, the intro's switch, the key picked up, the interior switch.
/// - **The combat watchers** (five, by their ids): woken in range, the
///   first marks the controller in combat, the third starts the
///   percussive combat bed; once every enemy each watches is off it says a
///   victory line on a random minion and takes itself off, the first and
///   second going back to the thin explore bed, the third to the stinger.
///   The first's range is the controller's to give (150) as the yard's
///   fight starts, the fourth's the gate's blow-up (80), the second's the
///   key's.
/// - **The interior watchers** (five, one round each interior below the
///   level): stepping in starts the inside bed; stepping out the gameplay
///   intro, the thin explore bed, the percussive bed while the key is held,
///   or the town's combat for the town's two.
/// - **The undead watchers** (two): the undead area's minimal explore bed
///   inside it, the town's combat once out of both.
/// - **The air watcher**: the air area's explore bed inside it, the thin
///   explore bed out of it.
/// - **The key watchers** (two): the one by the key gives the other a range
///   and goes; that one, reached, marks the key picked up and gives the
///   second combat watcher its range.
pub const MUSIC: [&str; 6] = [
    "Level_205/Scripts/snd_dynamic_sound_controller_205.ai",
    "Level_205/Scripts/snd_combat_watcher_205.ai",
    "Level_205/Scripts/snd_interior_watcher_205.ai",
    "Level_205/Scripts/snd_undead_watcher_205.ai",
    "Level_205/Scripts/snd_elemental_air_watcher_205.ai",
    "Level_205/Scripts/snd_key_watcher.ai",
];

/// The scripts [`MUSIC`] lists for the level whose archive is `level`.
pub fn music(level: &str) -> &'static [&'static str] {
    match level {
        LEVEL => &MUSIC,
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

    /// openspec AS.325: its way on from the first yard to Setup_002's end
    /// is on 205's listing alone, the runaway pirate drawn among its people
    /// there, and none of it walked as a fight or listed twice over.
    #[test]
    fn level_205s_way_to_setup_002s_end_is_on_its_listing_alone() {
        let files = Files::default();
        let on = level_config(Mode::Play, &files, 5, LEVEL);
        let off = level_config(Mode::Play, &files, 5, "level_201");
        for name in STRETCH_1 {
            assert!(on.whitelist.lists(name, "C:/l/Unlisted.ai"), "{name}");
            assert!(
                !off.whitelist.lists(name, "C:/l/Unlisted.ai"),
                "{name} on 201"
            );
            assert!(!WAY.placements.contains(&name), "{name} fights");
            assert!(!PASSAGE.contains(&name), "{name}");
            assert!(!listed(LEVEL).any(|n| n == name), "{name}");
        }
        for name in STRETCH_1_PEOPLE {
            assert!(STRETCH_1.contains(&name), "{name}");
            assert!(crate::scripts::is_level_person(LEVEL, name), "{name}");
            assert!(!crate::scripts::is_level_person("level_201", name));
        }
        assert!(stretch_1("level_201").is_empty());
        assert!(stretch_1_people("level_201").is_empty());
    }

    /// openspec AS.326: its way on from Setup_002's end is on 205's listing
    /// alone, none of it walked as a fight or listed twice over, and its
    /// grounded clones are 205's alone.
    #[test]
    fn level_205s_way_on_from_setup_002s_end_is_on_its_listing_alone() {
        let files = Files::default();
        let on = level_config(Mode::Play, &files, 5, LEVEL);
        let off = level_config(Mode::Play, &files, 5, "level_201");
        for name in STRETCH_2 {
            assert!(on.whitelist.lists(name, "C:/l/Unlisted.ai"), "{name}");
            assert!(
                !off.whitelist.lists(name, "C:/l/Unlisted.ai"),
                "{name} on 201"
            );
            assert!(!WAY.placements.contains(&name), "{name} fights");
            assert!(!PASSAGE.contains(&name), "{name}");
            assert!(!STRETCH_1.contains(&name), "{name}");
            assert!(!listed(LEVEL).any(|n| n == name), "{name}");
        }
        // Setup_003's controller waits on its combat area's signal face.
        assert!(on.whitelist.lists("CloseDoor", "C:/l/Unlisted.ai"));
        assert!(!on.whitelist.lists("Remove_Part1", "C:/l/Unlisted.ai"));
        assert!(stretch_2("level_201").is_empty());
        assert_eq!(stretch_2_grounded(LEVEL), ["Cali_NPC"]);
        assert!(stretch_2_grounded("level_201").is_empty());
    }

    /// openspec AS.371: the chain pull's peg is on 205's listing alone,
    /// listed once.
    #[test]
    fn level_205s_chain_pull_is_on_its_listing_alone() {
        let files = Files::default();
        let on = level_config(Mode::Play, &files, 5, LEVEL);
        let off = level_config(Mode::Play, &files, 5, "level_201");
        assert_eq!(CHAIN_PULL, ["TitanChainPeg(1)"]);
        for name in CHAIN_PULL {
            assert!(on.whitelist.lists(name, "C:/l/Unlisted.ai"), "{name}");
            assert!(
                !off.whitelist.lists(name, "C:/l/Unlisted.ai"),
                "{name} on 201"
            );
            assert!(!WAY.placements.contains(&name), "{name} fights");
            assert!(!PASSAGE.contains(&name), "{name}");
            assert!(!STRETCH_1.contains(&name), "{name}");
            assert!(!STRETCH_2.contains(&name), "{name}");
            assert!(!listed(LEVEL).any(|n| n == name), "{name}");
        }
        assert!(chain_pull("level_201").is_empty());
    }

    /// openspec AS.372: the galleon's gate and its padlock are on 205's
    /// listing alone, listed once, none of them on an earlier stretch's;
    /// the cannon's ball flies flat on 205 alone.
    #[test]
    fn level_205s_galleon_gate_is_on_its_listing_alone() {
        let files = Files::default();
        let on = level_config(Mode::Play, &files, 5, LEVEL);
        let off = level_config(Mode::Play, &files, 5, "level_201");
        for name in GALLEON {
            assert!(on.whitelist.lists(name, "C:/l/Unlisted.ai"), "{name}");
            assert!(
                !off.whitelist.lists(name, "C:/l/Unlisted.ai"),
                "{name} on 201"
            );
            assert!(!WAY.placements.contains(&name), "{name} fights");
            assert!(!PASSAGE.contains(&name), "{name}");
            assert!(!STRETCH_1.contains(&name), "{name}");
            assert!(!STRETCH_2.contains(&name), "{name}");
            assert!(!CHAIN_PULL.contains(&name), "{name}");
            assert!(!listed(LEVEL).any(|n| n == name), "{name}");
        }
        assert!(galleon("level_201").is_empty());
        assert_eq!(flat_shots(LEVEL), ["Canon_Ball"]);
        assert!(flat_shots("level_201").is_empty());
    }

    /// openspec AS.327: the way on from Setup_003's end is on 205's listing
    /// alone, listed once, none of it on an earlier stretch's; the
    /// jailed pirate is one of the level's people.
    #[test]
    fn level_205s_way_on_from_setup_003s_end_is_on_its_listing_alone() {
        let files = Files::default();
        let on = level_config(Mode::Play, &files, 5, LEVEL);
        let off = level_config(Mode::Play, &files, 5, "level_201");
        for name in STRETCH_3 {
            assert!(on.whitelist.lists(name, "C:/l/Unlisted.ai"), "{name}");
            assert!(
                !off.whitelist.lists(name, "C:/l/Unlisted.ai"),
                "{name} on 201"
            );
            assert!(!WAY.placements.contains(&name), "{name} fights");
            assert!(!PASSAGE.contains(&name), "{name}");
            assert!(!STRETCH_1.contains(&name), "{name}");
            assert!(!STRETCH_2.contains(&name), "{name}");
            assert!(!CHAIN_PULL.contains(&name), "{name}");
            assert!(!GALLEON.contains(&name), "{name}");
            assert!(!listed(LEVEL).any(|n| n == name), "{name}");
        }
        // The turntable's lever and the cell's wall.
        assert!(
            on.whitelist
                .lists("Switch_Controller_template(1)", "C:/l/Unlisted.ai")
        );
        assert!(
            on.whitelist
                .lists("Breakable_Metal_Wall(5)", "C:/l/Unlisted.ai")
        );
        assert!(!on.whitelist.lists("Remove_Part2", "C:/l/Unlisted.ai"));
        assert!(PEOPLE.contains(&JAIL_PIRATE));
        assert!(stretch_3("level_201").is_empty());
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

    /// openspec AS.289: its sound scripts are on 205's listing, held with
    /// the music while the level's sounds are on their way and set apart
    /// with it, and on no other level's listing (each by its path under
    /// 205's folder, so another level's copy of a file by the same name
    /// stays off).
    #[test]
    fn level_205s_music_is_on_its_listing_alone() {
        let files = Files::default();
        let on = level_config(Mode::Play, &files, 5, LEVEL);
        let off = level_config(Mode::Play, &files, 5, "level_201");
        for s in MUSIC {
            let path = format!("C:/tfb/Content/Levels/{s}");
            assert!(on.whitelist.lists("watcher", &path), "{s}");
            assert!(!off.whitelist.lists("watcher", &path), "{s} on 201");
            assert!(crate::scripts::is_music_script(&path), "{s}");
            let apart = crate::scripts::without_music(on.clone());
            assert!(!apart.whitelist.lists("watcher", &path), "{s} set apart");
        }
        let elsewhere = "C:/tfb/Content/Levels/Level_208/Scripts/snd_key_watcher.ai";
        assert!(!on.whitelist.lists("watcher", elsewhere));
        assert!(!crate::scripts::is_music_script(elsewhere));
        assert!(music("level_201").is_empty());
    }
}
