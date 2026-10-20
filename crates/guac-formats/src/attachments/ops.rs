//! The three op layouts the attachment stepper reads beyond AE.2's and
//! AH.2's, offsets from the registry (ac4's `registry.json`). AH.13's op
//! graph lays out all 44 classes the disc uses; when the two meet, these
//! give way to its.

use crate::FormatError;
use crate::igz::layouts::{Layout, ObjectRef, OpCode, View};

/// `OpSpawn`: size 0x30, through `OpAbstractControl`: `clone <lhs> at <rhs>
/// facing <facing>`; its block runs with the clone.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OpSpawn<'a> {
    pub op: OpCode<'a>,
    /// Filled at run time: null in files.
    pub last_receiver: ObjectRef<'a>,
    /// An `AbstractPlacementList` stack: the template.
    pub lhs: ObjectRef<'a>,
    pub cached_object: ObjectRef<'a>,
    /// A `PositionStack`: where.
    pub rhs: ObjectRef<'a>,
    pub facing_rhs: ObjectRef<'a>,
}

impl<'a> Layout<'a> for OpSpawn<'a> {
    const TYPE: &'static str = "OpSpawn";

    fn fields(v: &mut View<'_, 'a>) -> Result<Self, FormatError> {
        Ok(Self {
            op: <OpCode<'a> as Layout<'a>>::fields(v)?,
            last_receiver: v.object_ref(0x1C, "_lastReceiver")?,
            lhs: v.object_ref(0x20, "_LHS")?,
            cached_object: v.object_ref(0x24, "_cachedObject")?,
            rhs: v.object_ref(0x28, "_RHS")?,
            facing_rhs: v.object_ref(0x2C, "_facingRHS")?,
        })
    }
}

/// `OpCheckReference`: size 0x2C, `if <lhs> <relation> <rhs>[<index>]` on
/// references.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OpCheckReference<'a> {
    pub op: OpCode<'a>,
    pub lhs: ObjectRef<'a>,
    /// A comparison code ([`crate::progression::RELATIONS`]).
    pub relation: u32,
    pub rhs: ObjectRef<'a>,
    pub index_rhs: ObjectRef<'a>,
}

impl<'a> Layout<'a> for OpCheckReference<'a> {
    const TYPE: &'static str = "OpCheckReference";

    fn fields(v: &mut View<'_, 'a>) -> Result<Self, FormatError> {
        Ok(Self {
            op: <OpCode<'a> as Layout<'a>>::fields(v)?,
            lhs: v.object_ref(0x1C, "_LHS")?,
            relation: v.u32(0x20, "_relOperator")?,
            rhs: v.object_ref(0x24, "_RHS")?,
            index_rhs: v.object_ref(0x28, "_indexRHS")?,
        })
    }
}

/// `OpDecValue`: size 0x20, `dec value <lhs>`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OpDecValue<'a> {
    pub op: OpCode<'a>,
    pub lhs: ObjectRef<'a>,
}

impl<'a> Layout<'a> for OpDecValue<'a> {
    const TYPE: &'static str = "OpDecValue";

    fn fields(v: &mut View<'_, 'a>) -> Result<Self, FormatError> {
        Ok(Self {
            op: <OpCode<'a> as Layout<'a>>::fields(v)?,
            lhs: v.object_ref(0x1C, "_LHS")?,
        })
    }
}
