//! The harness that drives the patched RPCS3 as Guacamole's fidelity oracle
//! (openspec AF): an isolated oracle HOME ([`home`]), the launch wrapper that
//! boots the emulator inside it ([`launch`]), the O-series capture and control
//! verbs ([`verbs`], as run steps in [`control`]), the frame-locked input that
//! loads an input script ([`pad`]), the PINE client that reads and writes
//! guest memory ([`pine`]), a word search over it ([`scan`]), Giants'
//! random number generators read and written through it ([`rng`]), its
//! actors and camera read through it ([`actors`]), the divergence
//! measurement built on all of them ([`divergence`]), O5's per-frame
//! recorder with Giants' watch list ([`watch`], AG.1), a level as the live
//! game holds it: its placements, its collision and its solids ([`level`],
//! AG.7), the hero's own scale, solid and parameters ([`hero`], AG.8), his walk
//! as the movement script keeps it ([`mover`], AG.38), every
//! character placed in turn with what the game attaches to it ([`sweep`],
//! AE.22), the script runtime's cells and `global.bld`'s exports
//! ([`script`], AH.4) and the fight's state on them: the hero's health, his
//! figure's experience and gold, the enemy stats, the difficulty, the
//! multiplier, the element area, the loot counts and the triggers
//! ([`fight`], AH.4), O6's chains over the actors, orbs and loot a fight
//! spawns ([`chains`], AH.5), and the route driver that walks the oracle to a
//! point and leaves it still there, closed loop, for a state to be saved
//! ([`route`], AH.7).
//!
//! It never writes inside a work tree of this repo: every path it creates goes
//! through `rrc_dump::output::refuse_repo_output` first, because what an oracle
//! run produces (states, captures, memory) is game data (the HARD rule in
//! docs/native/guacamole-design.md). It never runs RPCS3 against the dev's own
//! RPCS3 dir either, since a savestate load deletes and rewrites hdd0 and hdd1.
//!
//! The Rust side follows the repo's two-layer rule for emulator patches: codecs
//! unit-tested, a fake server in `tests/` as the executable spec, and
//! `#[ignore]`d live tests against the real oracle on the Mac.

pub mod actors;
pub mod chains;
pub mod control;
pub mod divergence;
pub mod env;
pub mod fight;
pub mod hero;
pub mod home;
pub mod identity;
pub mod launch;
pub mod level;
pub mod mover;
pub mod outputs;
pub mod pad;
pub mod pine;
pub mod rng;
pub mod route;
pub mod rpcs3_log;
pub mod scan;
pub mod script;
pub mod snapshot;
pub mod sweep;
pub mod template;
pub mod verbs;
pub mod watch;
