//! The op and script-object layouts the progression readers follow beyond
//! AE.2's (openspec AH.2): a value set, an increment, a value check, a
//! reference set, a macro specialization and a save param's class. Offsets
//! are the registry's (ah-progression's `reg_fields.tsv`); AH.13's layouts
//! for all 44 op classes take these over.

use crate::FormatError;
use crate::igz::layouts::{Layout, ObjectRef, OpCode, OpDefineMacro, TypeRef, View};

/// `OpSetValue`: size 0x24, `set <lhs> = <rhs>`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OpSetValue<'a> {
    pub op: OpCode<'a>,
    /// A value stack: the cell written.
    pub lhs: ObjectRef<'a>,
    /// An RHS stack or a `ValueRHSVariant`.
    pub rhs: ObjectRef<'a>,
}

impl OpSetValue<'_> {
    pub const LHS: u32 = 0x1C;
    pub const RHS: u32 = 0x20;
}

impl<'a> Layout<'a> for OpSetValue<'a> {
    const TYPE: &'static str = "OpSetValue";

    fn fields(v: &mut View<'_, 'a>) -> Result<Self, FormatError> {
        Ok(Self {
            op: <OpCode<'a> as Layout<'a>>::fields(v)?,
            lhs: v.object_ref(Self::LHS, "_LHS")?,
            rhs: v.object_ref(Self::RHS, "_RHS")?,
        })
    }
}

/// `OpIncValue`: size 0x20, `inc value <lhs>`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OpIncValue<'a> {
    pub op: OpCode<'a>,
    pub lhs: ObjectRef<'a>,
}

impl OpIncValue<'_> {
    pub const LHS: u32 = 0x1C;
}

impl<'a> Layout<'a> for OpIncValue<'a> {
    const TYPE: &'static str = "OpIncValue";

    fn fields(v: &mut View<'_, 'a>) -> Result<Self, FormatError> {
        Ok(Self {
            op: <OpCode<'a> as Layout<'a>>::fields(v)?,
            lhs: v.object_ref(Self::LHS, "_LHS")?,
        })
    }
}

/// `OpCheckValue`: size 0x28, `if <lhs> <relation> <rhs>`: its block runs
/// when the relation holds, else the op after the block does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OpCheckValue<'a> {
    pub op: OpCode<'a>,
    pub lhs: ObjectRef<'a>,
    /// A comparison code ([`super::RELATIONS`]).
    pub relation: u32,
    pub rhs: ObjectRef<'a>,
}

impl OpCheckValue<'_> {
    pub const LHS: u32 = 0x1C;
    pub const RELATION: u32 = 0x20;
    pub const RHS: u32 = 0x24;
}

impl<'a> Layout<'a> for OpCheckValue<'a> {
    const TYPE: &'static str = "OpCheckValue";

    fn fields(v: &mut View<'_, 'a>) -> Result<Self, FormatError> {
        Ok(Self {
            op: <OpCode<'a> as Layout<'a>>::fields(v)?,
            lhs: v.object_ref(Self::LHS, "_LHS")?,
            relation: v.u32(Self::RELATION, "_relOperator")?,
            rhs: v.object_ref(Self::RHS, "_RHS")?,
        })
    }
}

/// `OpSetReference`: size 0x28, `setref <lhs> = <rhs>`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OpSetReference<'a> {
    pub op: OpCode<'a>,
    pub lhs: ObjectRef<'a>,
    pub rhs: ObjectRef<'a>,
    pub index_rhs: ObjectRef<'a>,
}

impl OpSetReference<'_> {
    pub const LHS: u32 = 0x1C;
    pub const RHS: u32 = 0x20;
    pub const INDEX_RHS: u32 = 0x24;
}

impl<'a> Layout<'a> for OpSetReference<'a> {
    const TYPE: &'static str = "OpSetReference";

    fn fields(v: &mut View<'_, 'a>) -> Result<Self, FormatError> {
        Ok(Self {
            op: <OpCode<'a> as Layout<'a>>::fields(v)?,
            lhs: v.object_ref(Self::LHS, "_LHS")?,
            rhs: v.object_ref(Self::RHS, "_RHS")?,
            index_rhs: v.object_ref(Self::INDEX_RHS, "_indexRHS")?,
        })
    }
}

/// `OpDefineMacroSpecialization`: size 0x40, a macro specializing another;
/// `_NP` names the base through an `OpDefineMacroStack`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OpDefineMacroSpecialization<'a> {
    pub def: OpDefineMacro<'a>,
    pub np: Option<usize>,
}

impl OpDefineMacroSpecialization<'_> {
    pub const NP: u32 = 0x38;
}

impl<'a> Layout<'a> for OpDefineMacroSpecialization<'a> {
    const TYPE: &'static str = "OpDefineMacroSpecialization";

    fn fields(v: &mut View<'_, 'a>) -> Result<Self, FormatError> {
        Ok(Self {
            def: <OpDefineMacro<'a> as Layout<'a>>::fields(v)?,
            np: v.object(Self::NP, "_NP", "OpDefineMacroStack")?,
        })
    }
}

/// `ScriptStructure`: size 0x20, a named structure of `global.bld` (a save
/// param, `pp_Exp`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ScriptStructure<'a> {
    pub name: Option<&'a str>,
    pub field_list: ObjectRef<'a>,
    pub contents_type: TypeRef<'a>,
}

impl ScriptStructure<'_> {
    pub const NAME: u32 = 0x08;
    pub const FIELD_LIST: u32 = 0x18;
    pub const CONTENTS_TYPE: u32 = 0x1C;
}

impl<'a> Layout<'a> for ScriptStructure<'a> {
    const TYPE: &'static str = "ScriptStructure";

    fn fields(v: &mut View<'_, 'a>) -> Result<Self, FormatError> {
        Ok(Self {
            name: v.string(Self::NAME, "_name")?,
            field_list: v.object_ref(Self::FIELD_LIST, "_fieldList")?,
            contents_type: v.type_ref(Self::CONTENTS_TYPE, "_varContentsType")?,
        })
    }
}
