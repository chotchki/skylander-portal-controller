//! The hero's motion, pure (openspec AG, design decisions 3, 4 and 5): the
//! physics seam ([`seam`]) and its parry3d implementation ([`parry`]), and the
//! hero controller written from the hero-movement spec on top of them. No
//! Bevy: guacamole's Avian side implements the same seam and passes the same
//! table ([`conformance`]).
//!
//! The controller, one file per family of mechanisms:
//! - [`hero`]: a game frame in the original's order, and the step lags;
//! - [`walk`]: the walk the movement script stores (AG.8, AG.10, AG.38);
//! - [`edge`]: the edge probe's stand-in (AG.36, AG.39);
//! - [`proxy`]: the engine's cast-and-slide move and the wall law (AG.9);
//! - [`solve`]: the frictionless plane solve under it;
//! - [`ground`]: the ground finder, support, the snap and the lean (AG.9,
//!   AG.39);
//! - [`solid`]: shapes from a `CollisionInfo` (AG.7, AG.8);
//! - [`stick`]: the dead zone and the stick against the view (AG.8);
//! - [`tuning`]: Eruptor's numbers, in the script's feet;
//! - [`clock`]: the fixed 1/60 s step against the display's.
//!
//! The engine's own steering beside it ([`steer`], AH.18): the velocity
//! model an actor no stick drives runs, fed a destination by the scripts'
//! `move to`, `move from` and `turn to` ([`hero::Hero::step_steered`]).
//!
//! The locomotion stand-in beside it ([`locomotion`], AG.30): the clips'
//! weights, times and rates from the speed the controller's walk reads and
//! his fall (AG.11).
//!
//! The gameplay camera beside it ([`camera`], AG.17): the follow point, the
//! camera target, the views and a transition plane's blend; and, behind the
//! `runner` feature, the headless runner that steps both from an AF.7 input
//! script into the oracle's trace shape (`runner`).
//!
//! What the oracle didn't show is labelled a hypothesis in the module that
//! builds it, with the reading behind it; AG.16's box keeps the list.
//! [`scenarios`] (with `conformance`) holds the controller's open-ground and
//! wall cases for any side of the seam.

#![forbid(unsafe_code)]

pub mod camera;
pub mod clock;
#[cfg(any(test, feature = "conformance"))]
pub mod conformance;
pub mod edge;
pub mod ground;
pub mod hero;
pub mod locomotion;
pub mod parry;
pub mod proxy;
#[cfg(feature = "runner")]
pub mod runner;
#[cfg(any(test, feature = "conformance"))]
pub mod scenarios;
pub mod seam;
pub mod solid;
pub mod solve;
pub mod steer;
pub mod stick;
pub mod tuning;
pub mod walk;
