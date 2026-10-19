//! The hero's motion, pure (openspec AG, design decisions 3 and 4): for now the
//! physics seam the controller will query ([`seam`]) and its parry3d
//! implementation ([`parry`]). No Bevy: guacamole's Avian side implements the
//! same trait and passes the same table ([`conformance`]).

#![forbid(unsafe_code)]

#[cfg(any(test, feature = "conformance"))]
pub mod conformance;
pub mod parry;
pub mod seam;
