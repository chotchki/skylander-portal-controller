//! Shared types for the Skylander Portal Controller.
//!
//! This crate has no I/O. It defines the domain model (figures, slots, portal
//! state) and the wire protocol between server and phone. Phase 2 MVP scope —
//! profiles, PINs, working copies, and game launching come in Phase 3.

pub mod figure;
pub mod portal;
pub mod protocol;

pub use figure::{Category, Element, Figure, FigureId, Game, GameSerial, PublicFigure};
pub use portal::{SlotIndex, SlotState, SLOT_COUNT};
pub use protocol::{Command, Event};
