//! The game's progression tables (openspec AH.2, design decisions 2, 9 and
//! 10), read off the disc for good: the experience each level starts at,
//! where the figure's data keeps each value (the save-param table the
//! scripts build, which AH.23's figure buffer lays out) and the most gold a
//! figure holds. A file of its own in the extract, so an extract made before
//! it resumes for it alone.

use serde::{Deserialize, Serialize};

use crate::DumpIdentity;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Progression {
    pub identity: DumpIdentity,
    /// The experience each level starts at, level 1's 0 first: its length is
    /// the highest level, its last entry the most experience a figure holds.
    pub level_xp: Vec<u32>,
    /// The save params in the order the disc sets them up.
    pub save_params: Vec<SaveParam>,
    /// What "Game - Award Money" clamps a figure's gold to.
    pub gold_cap: u32,
}

/// Which setup macro a save param's call makes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum SaveKind {
    Plain,
    /// The second area a value runs on into past the first's limit.
    Spillover,
    MirrorAndClear,
    ExtendedBitfield,
}

/// One setup call of the disc's save-param table.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SaveParam {
    /// The save param (`pp_Exp`), or the set a loop indexes
    /// (`pp_Flags1_UpgradeBits`).
    pub param: String,
    pub kind: SaveKind,
    /// One a pass: a loop's call puts each member at its own bit.
    pub fields: Vec<SaveField>,
    pub width: u32,
    /// 0 for one value, else the width of each value the bits hold.
    pub bits_per_field: u32,
    /// The most the value holds; 0 for no limit.
    pub limit: u32,
}

/// Where one value starts in the figure's data: a 32-bit word and a bit.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SaveField {
    pub word: u32,
    pub bit: u32,
    /// The set member a loop's pass names, when the disc lists one.
    pub member: Option<String>,
}
