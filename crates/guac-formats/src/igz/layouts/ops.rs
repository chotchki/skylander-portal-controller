//! tfbScript ops and their operand stacks (openspec AE.2): what the toy-table
//! reader walks in `character/init_setup.bld`.
//!
//! A script (`tfbScriptInfo`) is an `OpCodeList` of op objects run in order.
//! Every op is an `OpCode`, whose `_branchPC` says how many ops after it its
//! block holds (aj-vm section 1): a `use macro` block is its INTERFACE block
//! (the parameters, then `flow macro`) and then the caller's after-return
//! ops. Operands are stacks: a path of elements (imports, variables,
//! objects) with, on an RHS stack, a literal for when the path is empty.
//! Only the classes the toy table needs have layouts; the reader names any
//! other op by its type alone.

use super::{DataList, Info, Layout, ObjectRef, TypeRef, View};
use crate::FormatError;

/// `_branchPC` 0xFFFF: `flow end`, which pops every block of its context.
pub const BRANCH_END: u16 = 0xFFFF;

/// `_arithOperator` on a `ValueRHSVariant` with no second operand.
pub const NO_OPERATOR: u32 = u32::MAX;

/// `OpCode`'s fields (size 0x1C): name, flags and the block length.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OpCode<'a> {
    /// The op's display form (`use macro`, `parameter`, a macro's name).
    pub name: Option<&'a str>,
    pub flags: u16,
    /// How many ops after this one its block holds; [`BRANCH_END`] on `flow
    /// end`.
    pub branch: u16,
}

impl<'a> OpCode<'a> {
    pub const NAME: u32 = 0x08;
    pub const FLAGS: u32 = 0x18;
    pub const BRANCH: u32 = 0x1A;

    pub(super) fn fields(v: &mut View<'_, 'a>) -> Result<Self, FormatError> {
        Ok(Self {
            name: v.string(Self::NAME, "_name")?,
            flags: v.u16(Self::FLAGS, "_internalFlagsStorage")?,
            branch: v.u16(Self::BRANCH, "_branchPC")?,
        })
    }

    /// The last position of the block an op at `position` opens, or `None`
    /// when it opens none.
    pub fn block_end(&self, position: usize) -> Option<usize> {
        (self.branch != 0 && self.branch != BRANCH_END).then(|| position + usize::from(self.branch))
    }
}

/// Any op, read as its `OpCode` fields.
impl<'a> Layout<'a> for OpCode<'a> {
    const TYPE: &'static str = "OpCode";

    fn fields(v: &mut View<'_, 'a>) -> Result<Self, FormatError> {
        Self::fields(v)
    }
}

/// `tfbScriptInfo`: size 0x1C, a script (its name is its source path).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ScriptInfo<'a> {
    pub info: Info<'a>,
    /// An `OpCodeList`.
    pub op_list: Option<usize>,
    pub master_var_list: ObjectRef<'a>,
}

impl ScriptInfo<'_> {
    pub const OP_LIST: u32 = 0x14;
    pub const MASTER_VAR_LIST: u32 = 0x18;
}

impl<'a> Layout<'a> for ScriptInfo<'a> {
    const TYPE: &'static str = "tfbScriptInfo";

    fn fields(v: &mut View<'_, 'a>) -> Result<Self, FormatError> {
        Ok(Self {
            info: Info::fields(v)?,
            op_list: v.object(Self::OP_LIST, "_opList", OpCodeList::TYPE)?,
            master_var_list: v.object_ref(Self::MASTER_VAR_LIST, "_masterVarList")?,
        })
    }
}

/// `OpCodeList`: a script's ops in run order, every one a local object
/// (stateless ops, `flow macro` and `flow continue`, are one shared object).
/// Their types aren't checked here: 44 op classes occur on the disc and
/// only the ones a reader follows have layouts.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OpCodeList<'a> {
    pub list: DataList<'a>,
    pub ops: Vec<usize>,
}

impl<'a> Layout<'a> for OpCodeList<'a> {
    const TYPE: &'static str = "OpCodeList";

    fn fields(v: &mut View<'_, 'a>) -> Result<Self, FormatError> {
        let list = DataList::fields(v)?;
        let mut ops = Vec::new();
        for op in v.elements(&list, None)? {
            ops.push(op.ok_or_else(|| v.refused(DataList::DATA, "element"))?);
        }
        Ok(Self { list, ops })
    }
}

/// `OpUseMacro`: size 0x24, a macro call.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OpUseMacro<'a> {
    pub op: OpCode<'a>,
    /// A `ScriptGroupStack` whose element is the macro's `OpDefineMacro`.
    pub np: Option<usize>,
    pub index_rhs: ObjectRef<'a>,
}

impl OpUseMacro<'_> {
    pub const NP: u32 = 0x1C;
    pub const INDEX_RHS: u32 = 0x20;
}

impl<'a> Layout<'a> for OpUseMacro<'a> {
    const TYPE: &'static str = "OpUseMacro";

    fn fields(v: &mut View<'_, 'a>) -> Result<Self, FormatError> {
        Ok(Self {
            op: OpCode::fields(v)?,
            np: v.object(Self::NP, "_NP", "ScriptGroupStack")?,
            index_rhs: v.object_ref(Self::INDEX_RHS, "_indexRHS")?,
        })
    }
}

/// `OpMacroInterface`: size 0x20, the INTERFACE block of a call or a
/// definition.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OpMacroInterface<'a> {
    pub op: OpCode<'a>,
    pub var_template_list: ObjectRef<'a>,
}

impl OpMacroInterface<'_> {
    pub const VAR_TEMPLATE_LIST: u32 = 0x1C;
}

impl<'a> Layout<'a> for OpMacroInterface<'a> {
    const TYPE: &'static str = "OpMacroInterface";

    fn fields(v: &mut View<'_, 'a>) -> Result<Self, FormatError> {
        Ok(Self {
            op: OpCode::fields(v)?,
            var_template_list: v.object_ref(Self::VAR_TEMPLATE_LIST, "_varTemplateList")?,
        })
    }
}

/// `OpDefineMacro`: size 0x38, a macro definition or a stub naming one; its
/// name is the macro's. `_thisPC`, `_scriptInfo` and the frame refs are
/// filled at link, so files hold zeros there. An
/// `OpDefineMacroSpecialization` (0x40) reads through this, its base (`_NP`
/// at 0x38) past the layout and unread.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OpDefineMacro<'a> {
    pub op: OpCode<'a>,
    /// An enum, kept raw.
    pub publicity: u32,
    pub creation_handle: u64,
    pub this_pc: u32,
    pub script_info: ObjectRef<'a>,
    pub macro_var_list: ObjectRef<'a>,
    pub return_macro: ObjectRef<'a>,
}

impl OpDefineMacro<'_> {
    pub const PUBLICITY: u32 = 0x1C;
    pub const CREATION_HANDLE: u32 = 0x20;
    pub const THIS_PC: u32 = 0x28;
    pub const SCRIPT_INFO: u32 = 0x2C;
    pub const MACRO_VAR_LIST: u32 = 0x30;
    pub const RETURN_MACRO: u32 = 0x34;
}

impl<'a> Layout<'a> for OpDefineMacro<'a> {
    const TYPE: &'static str = "OpDefineMacro";

    fn fields(v: &mut View<'_, 'a>) -> Result<Self, FormatError> {
        Ok(Self {
            op: OpCode::fields(v)?,
            publicity: v.u32(Self::PUBLICITY, "_publicity")?,
            creation_handle: u64::from(v.u32(Self::CREATION_HANDLE, "_creationHandle")?) << 32
                | u64::from(v.u32(Self::CREATION_HANDLE + 4, "_creationHandle")?),
            this_pc: v.u32(Self::THIS_PC, "_thisPC")?,
            script_info: v.object_ref(Self::SCRIPT_INFO, "_scriptInfo")?,
            macro_var_list: v.object_ref(Self::MACRO_VAR_LIST, "_macroVarList")?,
            return_macro: v.object_ref(Self::RETURN_MACRO, "_returnMacro")?,
        })
    }
}

/// `OpAbstractCreateVariable`'s fields (size 0x2C): a variable's declaration.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Variable<'a> {
    pub op: OpCode<'a>,
    pub name: Option<&'a str>,
    pub container_type: TypeRef<'a>,
    pub contents_type: TypeRef<'a>,
    /// Filled at link: -1 in files.
    pub index: i32,
}

impl<'a> Variable<'a> {
    pub const VAR_NAME: u32 = 0x1C;
    pub const CONTAINER_TYPE: u32 = 0x20;
    pub const CONTENTS_TYPE: u32 = 0x24;
    pub const VAR_INDEX: u32 = 0x28;

    fn fields(v: &mut View<'_, 'a>) -> Result<Self, FormatError> {
        Ok(Self {
            op: OpCode::fields(v)?,
            name: v.string(Self::VAR_NAME, "_varName")?,
            container_type: v.type_ref(Self::CONTAINER_TYPE, "_varContainerType")?,
            contents_type: v.type_ref(Self::CONTENTS_TYPE, "_varContentsType")?,
            index: v.i32(Self::VAR_INDEX, "_varIndex")?,
        })
    }
}

/// Any variable declaration (`OpCreateVariable`, a parameter), read as its
/// `OpAbstractCreateVariable` fields.
impl<'a> Layout<'a> for Variable<'a> {
    const TYPE: &'static str = "OpAbstractCreateVariable";

    fn fields(v: &mut View<'_, 'a>) -> Result<Self, FormatError> {
        Self::fields(v)
    }
}

/// `OpMacroParameter`: size 0x48, one argument of a call (in a call's
/// INTERFACE block) or one parameter's default (in a definition's).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OpMacroParameter<'a> {
    pub var: Variable<'a>,
    /// How a set argument combines into the callee's cell.
    pub combine_op: u32,
    /// The value: an RHS stack or a `ValueRHSVariant`.
    pub rhs: ObjectRef<'a>,
    /// An index into a set `rhs` names (`Minion_Names` and its index).
    pub index_rhs: ObjectRef<'a>,
    /// The definition the call binds into.
    pub macro_def: ObjectRef<'a>,
    pub parameter_id: u64,
}

impl OpMacroParameter<'_> {
    pub const COMBINE_OP: u32 = 0x2C;
    pub const RHS: u32 = 0x30;
    pub const INDEX_RHS: u32 = 0x34;
    pub const MACRO_DEF: u32 = 0x38;
    pub const PARAMETER_ID: u32 = 0x40;
}

impl<'a> Layout<'a> for OpMacroParameter<'a> {
    const TYPE: &'static str = "OpMacroParameter";

    fn fields(v: &mut View<'_, 'a>) -> Result<Self, FormatError> {
        Ok(Self {
            var: Variable::fields(v)?,
            combine_op: v.u32(Self::COMBINE_OP, "_combineOp")?,
            rhs: v.object_ref(Self::RHS, "_RHS")?,
            index_rhs: v.object_ref(Self::INDEX_RHS, "_indexRHS")?,
            macro_def: v.object_ref(Self::MACRO_DEF, "_macroDef")?,
            parameter_id: u64::from(v.u32(Self::PARAMETER_ID, "_parameterID")?) << 32
                | u64::from(v.u32(Self::PARAMETER_ID + 4, "_parameterID")?),
        })
    }
}

/// `OpChangeMembership`: size 0x28, `set [set] to`: combines `rhs` into the
/// set `lhs` names (aj-vm: 0 add if absent, 1 remove one, 2 intersect, 3
/// replace, 4 append, 5 remove all).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OpChangeMembership<'a> {
    pub op: OpCode<'a>,
    /// A `SetStack`, as every one on the disc is; unfollowed.
    pub lhs: ObjectRef<'a>,
    pub combine_op: u32,
    /// A `ScriptGroupStack`, or another set operand.
    pub rhs: ObjectRef<'a>,
}

impl OpChangeMembership<'_> {
    pub const LHS: u32 = 0x1C;
    pub const COMBINE_OP: u32 = 0x20;
    pub const RHS: u32 = 0x24;
    /// `_combineOp` 0: add if absent.
    pub const ADD: u32 = 0;
}

impl<'a> Layout<'a> for OpChangeMembership<'a> {
    const TYPE: &'static str = "OpChangeMembership";

    fn fields(v: &mut View<'_, 'a>) -> Result<Self, FormatError> {
        Ok(Self {
            op: OpCode::fields(v)?,
            lhs: v.object_ref(Self::LHS, "_LHS")?,
            combine_op: v.u32(Self::COMBINE_OP, "_combineOp")?,
            rhs: v.object_ref(Self::RHS, "_RHS")?,
        })
    }
}

/// `OpLoopValue`: size 0x28, `loop value`: runs its block `rhs` times.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OpLoopValue<'a> {
    pub op: OpCode<'a>,
    pub index: i32,
    /// The count: an RHS stack or a `ValueRHSVariant`.
    pub rhs: ObjectRef<'a>,
    pub num_loops: i32,
}

impl OpLoopValue<'_> {
    pub const INDEX: u32 = 0x1C;
    pub const RHS: u32 = 0x20;
    pub const NUM_LOOPS: u32 = 0x24;
}

impl<'a> Layout<'a> for OpLoopValue<'a> {
    const TYPE: &'static str = "OpLoopValue";

    fn fields(v: &mut View<'_, 'a>) -> Result<Self, FormatError> {
        Ok(Self {
            op: OpCode::fields(v)?,
            index: v.i32(Self::INDEX, "_index")?,
            rhs: v.object_ref(Self::RHS, "_RHS")?,
            num_loops: v.i32(Self::NUM_LOOPS, "_numLoops")?,
        })
    }
}

/// Any operand stack (`ScriptObjectList` and its subtypes: `SetStack`,
/// `ReferenceStack`, `ScriptGroupStack`, the value stacks): a path, its
/// elements local, imported or named.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Stack<'a> {
    pub list: DataList<'a>,
    pub path: Vec<ObjectRef<'a>>,
}

impl<'a> Stack<'a> {
    fn fields(v: &mut View<'_, 'a>) -> Result<Self, FormatError> {
        let list = DataList::fields(v)?;
        let path = v.element_refs(&list)?;
        Ok(Self { list, path })
    }
}

impl<'a> Layout<'a> for Stack<'a> {
    const TYPE: &'static str = "ScriptObjectList";

    fn fields(v: &mut View<'_, 'a>) -> Result<Self, FormatError> {
        Self::fields(v)
    }
}

/// `RHSValueStack`: size 0x28, a value operand: its path, or with an empty
/// path the literal `value` of class `value_type`. `RHSReferenceStack`
/// (0x2C) is one too, read through this.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RhsValueStack<'a> {
    pub stack: Stack<'a>,
    /// `ValueStack._otherPoint`.
    pub other_point: ObjectRef<'a>,
    pub value_type: TypeRef<'a>,
    /// The literal's bits.
    pub value: i32,
    pub is_random: bool,
    /// Filled at link: 0 in files.
    pub type_is_number: bool,
}

impl RhsValueStack<'_> {
    pub const OTHER_POINT: u32 = 0x18;
    pub const TYPE_FIELD: u32 = 0x1C;
    pub const VALUE: u32 = 0x20;
    pub const IS_RANDOM: u32 = 0x24;
    pub const TYPE_IS_NUMBER: u32 = 0x25;
}

impl<'a> Layout<'a> for RhsValueStack<'a> {
    const TYPE: &'static str = "RHSValueStack";

    fn fields(v: &mut View<'_, 'a>) -> Result<Self, FormatError> {
        Ok(Self {
            stack: Stack::fields(v)?,
            other_point: v.object_ref(Self::OTHER_POINT, "_otherPoint")?,
            value_type: v.type_ref(Self::TYPE_FIELD, "_type")?,
            value: v.i32(Self::VALUE, "_value")?,
            is_random: v.bool(Self::IS_RANDOM, "_isRandom")?,
            type_is_number: v.bool(Self::TYPE_IS_NUMBER, "_typeIsNumber")?,
        })
    }
}

/// `RHSReferenceStack`: registry size 0x2C, a reference operand. Its files'
/// MTSZ is 0x30: the 4 bytes past the layout are unmarked and sit in memory
/// as on disc (AC.14), so they're checked for fixups and not read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RhsReferenceStack<'a> {
    pub value: RhsValueStack<'a>,
    pub type_is_meta: bool,
    pub edit_type: TypeRef<'a>,
}

impl RhsReferenceStack<'_> {
    pub const TYPE_IS_META: u32 = 0x26;
    pub const EDIT_TYPE: u32 = 0x28;
}

impl<'a> Layout<'a> for RhsReferenceStack<'a> {
    const TYPE: &'static str = "RHSReferenceStack";

    fn fields(v: &mut View<'_, 'a>) -> Result<Self, FormatError> {
        Ok(Self {
            value: RhsValueStack::fields(v)?,
            type_is_meta: v.bool(Self::TYPE_IS_META, "_typeIsMeta")?,
            edit_type: v.type_ref(Self::EDIT_TYPE, "_editType")?,
        })
    }
}

/// `ValueRHSVariant`: size 0x1C, `op1 <operator> op2`, or `op1` alone when
/// the operator is [`NO_OPERATOR`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ValueRhsVariant<'a> {
    pub op1: ObjectRef<'a>,
    /// 0 +, 1 -, 2 *, 3 / (aj-vm).
    pub operator: u32,
    pub op2: ObjectRef<'a>,
    pub conditioner: ObjectRef<'a>,
}

impl ValueRhsVariant<'_> {
    pub const OP1: u32 = 0x08;
    pub const OPERATOR: u32 = 0x0C;
    pub const OP2: u32 = 0x10;
    pub const CONDITIONER: u32 = 0x18;
}

impl<'a> Layout<'a> for ValueRhsVariant<'a> {
    const TYPE: &'static str = "ValueRHSVariant";

    fn fields(v: &mut View<'_, 'a>) -> Result<Self, FormatError> {
        Ok(Self {
            op1: v.object_ref(Self::OP1, "_varOp1")?,
            operator: v.u32(Self::OPERATOR, "_arithOperator")?,
            op2: v.object_ref(Self::OP2, "_varOp2")?,
            conditioner: v.object_ref(Self::CONDITIONER, "_conditioner")?,
        })
    }
}
