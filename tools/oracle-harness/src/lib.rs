//! The harness that drives the patched RPCS3 as Guacamole's fidelity oracle
//! (openspec AF): an isolated oracle HOME ([`home`]), the launch wrapper that
//! boots the emulator inside it ([`launch`]) and the PINE client that reads and
//! writes guest memory ([`pine`]).
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

pub mod env;
pub mod home;
pub mod launch;
pub mod pine;
pub mod snapshot;
pub mod template;
