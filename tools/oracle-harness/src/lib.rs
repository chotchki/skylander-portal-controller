//! The harness that drives the patched RPCS3 as Guacamole's fidelity oracle
//! (openspec AF): an isolated oracle HOME ([`home`]), the launch wrapper that
//! boots the emulator inside it ([`launch`]), the O-series capture and control
//! verbs ([`verbs`], as run steps in [`control`]), the frame-locked input that
//! loads an input script ([`pad`]), the PINE client that reads and writes
//! guest memory ([`pine`]), a word search over it ([`scan`]), Giants'
//! random number generators read and written through it ([`rng`]), its
//! actors and camera read through it ([`actors`]), the divergence
//! measurement built on all of them ([`divergence`]) and O5's per-frame
//! recorder with Giants' watch list ([`watch`], AG.1).
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
pub mod control;
pub mod divergence;
pub mod env;
pub mod home;
pub mod identity;
pub mod launch;
pub mod outputs;
pub mod pad;
pub mod pine;
pub mod rng;
pub mod rpcs3_log;
pub mod scan;
pub mod snapshot;
pub mod template;
pub mod verbs;
pub mod watch;
