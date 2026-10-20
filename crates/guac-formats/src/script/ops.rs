//! The op classes' layouts beyond AE.2's (openspec AH.13): every field the
//! static registry gives each of the 44 classes the disc uses, at its offset,
//! tagged with the rest of [`crate::igz::layouts`] (`TAG`, `TYPES`). AE.2's
//! layouts (`OpCode`, `OpUseMacro`, `OpMacroInterface`, `OpDefineMacro`,
//! `Variable`, `OpMacroParameter`, `OpChangeMembership`, `OpLoopValue` and
//! the stacks) are reused as they are; the five AH.2 read for the save-param
//! table moved here from `progression`.
//!
//! Operands (`_LHS`, `_RHS`, `_NP` and the like) stay [`ObjectRef`]s here;
//! [`super::Graph`] follows them. Fields the original fills at link or run
//! time (`_cachedObject`, `_lastReceiver`, `_thisPC` on a behaviour) are read
//! as stored: zero or null on the disc.

use super::Field;
use crate::FormatError;
use crate::igz::layouts::{
    Layout, ObjectRef, OpChangeMembership, OpCode, OpDefineMacro, OpLoopValue, OpMacroInterface,
    OpMacroParameter, OpUseMacro, Stack, TypeRef, Variable, View,
};

/// What every op layout gives: its `OpCode` fields, and every field by its
/// registry name, parents' first.
pub trait OpFields<'a> {
    fn code(&self) -> OpCode<'a>;
    fn listed(&self, out: &mut Vec<(&'static str, Field<'a>)>);
}

/// A layout over a base layout plus fields each read by one `View` method.
macro_rules! layout {
    (
        $(#[$doc:meta])*
        $name:ident = $ty:literal, $base:ident: $base_ty:ident {
            $( $(#[$fdoc:meta])* $field:ident: $fty:ty = $read:ident($offset:literal, $fname:literal) ),* $(,)?
        }
    ) => {
        $(#[$doc])*
        #[derive(Debug, Clone, Copy, PartialEq, Eq)]
        pub struct $name<'a> {
            pub $base: $base_ty<'a>,
            $( $(#[$fdoc])* pub $field: $fty, )*
        }

        impl<'a> Layout<'a> for $name<'a> {
            const TYPE: &'static str = $ty;

            fn fields(v: &mut View<'_, 'a>) -> Result<Self, FormatError> {
                Ok(Self {
                    $base: <$base_ty<'a> as Layout<'a>>::fields(v)?,
                    $( $field: v.$read($offset, $fname)?, )*
                })
            }
        }

        impl<'a> OpFields<'a> for $name<'a> {
            fn code(&self) -> OpCode<'a> {
                self.$base.code()
            }

            fn listed(&self, out: &mut Vec<(&'static str, Field<'a>)>) {
                self.$base.listed(out);
                $( out.push(($fname, Field::from(self.$field))); )*
            }
        }
    };
}

impl<'a> OpFields<'a> for OpCode<'a> {
    fn code(&self) -> OpCode<'a> {
        *self
    }

    fn listed(&self, out: &mut Vec<(&'static str, Field<'a>)>) {
        out.push(("_name", Field::Text(self.name)));
        out.push(("_internalFlagsStorage", Field::Word(self.flags.into())));
        out.push(("_branchPC", Field::Word(self.branch.into())));
    }
}

impl<'a> OpFields<'a> for Variable<'a> {
    fn code(&self) -> OpCode<'a> {
        self.op
    }

    fn listed(&self, out: &mut Vec<(&'static str, Field<'a>)>) {
        self.op.listed(out);
        out.push(("_varName", Field::Text(self.name)));
        out.push(("_varContainerType", Field::Type(self.container_type)));
        out.push(("_varContentsType", Field::Type(self.contents_type)));
        out.push(("_varIndex", Field::Int(self.index)));
    }
}

impl<'a> OpFields<'a> for OpMacroParameter<'a> {
    fn code(&self) -> OpCode<'a> {
        self.var.op
    }

    fn listed(&self, out: &mut Vec<(&'static str, Field<'a>)>) {
        self.var.listed(out);
        out.push(("_combineOp", Field::Word(self.combine_op)));
        out.push(("_RHS", Field::Ref(self.rhs)));
        out.push(("_indexRHS", Field::Ref(self.index_rhs)));
        out.push(("_macroDef", Field::Ref(self.macro_def)));
        out.push(("_parameterID", Field::Long(self.parameter_id)));
    }
}

impl<'a> OpFields<'a> for OpUseMacro<'a> {
    fn code(&self) -> OpCode<'a> {
        self.op
    }

    fn listed(&self, out: &mut Vec<(&'static str, Field<'a>)>) {
        self.op.listed(out);
        let np = self.np.map_or(ObjectRef::Null, ObjectRef::Object);
        out.push(("_NP", Field::Ref(np)));
        out.push(("_indexRHS", Field::Ref(self.index_rhs)));
    }
}

impl<'a> OpFields<'a> for OpMacroInterface<'a> {
    fn code(&self) -> OpCode<'a> {
        self.op
    }

    fn listed(&self, out: &mut Vec<(&'static str, Field<'a>)>) {
        self.op.listed(out);
        out.push(("_varTemplateList", Field::Ref(self.var_template_list)));
    }
}

impl<'a> OpFields<'a> for OpDefineMacro<'a> {
    fn code(&self) -> OpCode<'a> {
        self.op
    }

    fn listed(&self, out: &mut Vec<(&'static str, Field<'a>)>) {
        self.op.listed(out);
        out.push(("_publicity", Field::Word(self.publicity)));
        out.push(("_creationHandle", Field::Long(self.creation_handle)));
        out.push(("_thisPC", Field::Word(self.this_pc)));
        out.push(("_scriptInfo", Field::Ref(self.script_info)));
        out.push(("_macroVarList", Field::Ref(self.macro_var_list)));
        out.push(("_returnMacro", Field::Ref(self.return_macro)));
    }
}

impl<'a> OpFields<'a> for OpChangeMembership<'a> {
    fn code(&self) -> OpCode<'a> {
        self.op
    }

    fn listed(&self, out: &mut Vec<(&'static str, Field<'a>)>) {
        self.op.listed(out);
        out.push(("_LHS", Field::Ref(self.lhs)));
        out.push(("_combineOp", Field::Word(self.combine_op)));
        out.push(("_RHS", Field::Ref(self.rhs)));
    }
}

impl<'a> OpFields<'a> for OpLoopValue<'a> {
    fn code(&self) -> OpCode<'a> {
        self.op
    }

    fn listed(&self, out: &mut Vec<(&'static str, Field<'a>)>) {
        self.op.listed(out);
        out.push(("_index", Field::Int(self.index)));
        out.push(("_RHS", Field::Ref(self.rhs)));
        out.push(("_numLoops", Field::Int(self.num_loops)));
    }
}

layout! {
    /// `OpSetValue`: size 0x24, writes the value `rhs` evaluates to into the
    /// cell `lhs` names.
    OpSetValue = "OpSetValue", op: OpCode {
        /// A value stack: the cell written.
        lhs: ObjectRef<'a> = object_ref(0x1C, "_LHS"),
        /// An RHS stack or a `ValueRHSVariant`.
        rhs: ObjectRef<'a> = object_ref(0x20, "_RHS"),
    }
}

layout! {
    /// `OpIncValue`: size 0x20, `inc value <lhs>` (adds 1, or 1.0 to a float).
    OpIncValue = "OpIncValue", op: OpCode {
        lhs: ObjectRef<'a> = object_ref(0x1C, "_LHS"),
    }
}

layout! {
    /// `OpDecValue`: size 0x20, subtracts 1 from `lhs` (1.0 from a float).
    OpDecValue = "OpDecValue", op: OpCode {
        lhs: ObjectRef<'a> = object_ref(0x1C, "_LHS"),
    }
}

layout! {
    /// `OpCheckValue`: size 0x28, `if <lhs> <relation> <rhs>`; its block
    /// runs when the relation holds, else the op after the block does.
    OpCheckValue = "OpCheckValue", op: OpCode {
        lhs: ObjectRef<'a> = object_ref(0x1C, "_LHS"),
        /// A comparison code (`crate::progression::RELATIONS`).
        relation: u32 = u32(0x20, "_relOperator"),
        rhs: ObjectRef<'a> = object_ref(0x24, "_RHS"),
    }
}

layout! {
    /// `OpSetReference`: size 0x28, `setref <lhs> = <rhs>`, `index_rhs`
    /// picking a set member.
    OpSetReference = "OpSetReference", op: OpCode {
        lhs: ObjectRef<'a> = object_ref(0x1C, "_LHS"),
        rhs: ObjectRef<'a> = object_ref(0x20, "_RHS"),
        index_rhs: ObjectRef<'a> = object_ref(0x24, "_indexRHS"),
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

impl<'a> OpFields<'a> for OpDefineMacroSpecialization<'a> {
    fn code(&self) -> OpCode<'a> {
        self.def.op
    }

    fn listed(&self, out: &mut Vec<(&'static str, Field<'a>)>) {
        self.def.listed(out);
        let np = self.np.map_or(ObjectRef::Null, ObjectRef::Object);
        out.push(("_NP", Field::Ref(np)));
    }
}

/// `OpAbstractDefinition`'s fields (size 0x30): what `OpDefineStructure`
/// is, a structure type whose block lists its fields' declarations.
/// `_thisPC` and `_scriptInfo` are filled at link (zero in files).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Definition<'a> {
    pub op: OpCode<'a>,
    /// The idle flag at run time (aj-vm); in files, kept raw.
    pub publicity: u32,
    /// The Wii doc's creation id.
    pub creation_handle: u64,
    pub this_pc: u32,
    pub script_info: ObjectRef<'a>,
}

impl<'a> Layout<'a> for Definition<'a> {
    const TYPE: &'static str = "OpAbstractDefinition";

    fn fields(v: &mut View<'_, 'a>) -> Result<Self, FormatError> {
        Ok(Self {
            op: <OpCode<'a> as Layout<'a>>::fields(v)?,
            publicity: v.u32(0x1C, "_publicity")?,
            creation_handle: u64::from(v.u32(0x20, "_creationHandle")?) << 32
                | u64::from(v.u32(0x24, "_creationHandle")?),
            this_pc: v.u32(0x28, "_thisPC")?,
            script_info: v.object_ref(0x2C, "_scriptInfo")?,
        })
    }
}

impl<'a> OpFields<'a> for Definition<'a> {
    fn code(&self) -> OpCode<'a> {
        self.op
    }

    fn listed(&self, out: &mut Vec<(&'static str, Field<'a>)>) {
        self.op.listed(out);
        out.push(("_publicity", Field::Word(self.publicity)));
        out.push(("_creationHandle", Field::Long(self.creation_handle)));
        out.push(("_thisPC", Field::Word(self.this_pc)));
        out.push(("_scriptInfo", Field::Ref(self.script_info)));
    }
}

layout! {
    /// `OpUserBehavior`: size 0x24, a named state (`idle`, `battle`); its
    /// block is the state's ops. `_thisPC` and `_scriptInfo` are filled at
    /// link.
    OpUserBehavior = "OpUserBehavior", op: OpCode {
        this_pc: u32 = u32(0x1C, "_thisPC"),
        script_info: ObjectRef<'a> = object_ref(0x20, "_scriptInfo"),
    }
}

layout! {
    /// `OpSetBehavior`: size 0x24, switches the placement to the behaviour
    /// `np` names.
    OpSetBehavior = "OpSetBehavior", op: OpCode {
        np: ObjectRef<'a> = object_ref(0x1C, "_NP"),
        index_rhs: ObjectRef<'a> = object_ref(0x20, "_indexRHS"),
    }
}

layout! {
    /// `OpAbstractControl`'s fields (size 0x28): an op whose block runs with
    /// another placement as the receiver.
    Control = "OpAbstractControl", op: OpCode {
        last_receiver: ObjectRef<'a> = object_ref(0x1C, "_lastReceiver"),
        lhs: ObjectRef<'a> = object_ref(0x20, "_LHS"),
        cached_object: ObjectRef<'a> = object_ref(0x24, "_cachedObject"),
    }
}

layout! {
    /// `OpSpawn`: size 0x30, clones the template `lhs` names at `rhs`,
    /// facing `facing_rhs`; its block runs with the clone as the receiver.
    OpSpawn = "OpSpawn", control: Control {
        rhs: ObjectRef<'a> = object_ref(0x28, "_RHS"),
        facing_rhs: ObjectRef<'a> = object_ref(0x2C, "_facingRHS"),
    }
}

layout! {
    /// `OpControl`: size 0x2C, `tell <lhs>`: its block runs with that
    /// placement as the receiver.
    OpControl = "OpControl", control: Control {
        index_rhs: ObjectRef<'a> = object_ref(0x28, "_indexRHS"),
    }
}

layout! {
    /// `OpCheckReference`: size 0x2C, `if <lhs> is <rhs>` over references;
    /// only codes 1 (`==`) and 5 (`!=`) on the disc.
    OpCheckReference = "OpCheckReference", op: OpCode {
        lhs: ObjectRef<'a> = object_ref(0x1C, "_LHS"),
        relation: u32 = u32(0x20, "_relOperator"),
        rhs: ObjectRef<'a> = object_ref(0x24, "_RHS"),
        index_rhs: ObjectRef<'a> = object_ref(0x28, "_indexRHS"),
    }
}

layout! {
    /// `OpSlideValue`: size 0x34, eases `lhs` to `rhs` over `seconds_rhs`;
    /// its block runs at once.
    OpSlideValue = "OpSlideValue", op: OpCode {
        lhs: ObjectRef<'a> = object_ref(0x1C, "_LHS"),
        rhs: ObjectRef<'a> = object_ref(0x20, "_RHS"),
        seconds_rhs: ObjectRef<'a> = object_ref(0x24, "_secondsRHS"),
        ease_out_rhs: ObjectRef<'a> = object_ref(0x28, "_easeOutRHS"),
        ease_in_rhs: ObjectRef<'a> = object_ref(0x2C, "_easeInRHS"),
        cached_slider: ObjectRef<'a> = object_ref(0x30, "_cachedSlider"),
    }
}

layout! {
    /// `OpRemove`: size 0x20, removes the placements `np` names.
    OpRemove = "OpRemove", op: OpCode {
        np: ObjectRef<'a> = object_ref(0x1C, "_NP"),
    }
}

layout! {
    /// `OpStopSequence`: size 0x20, stops the sequences `np` names.
    OpStopSequence = "OpStopSequence", op: OpCode {
        np: ObjectRef<'a> = object_ref(0x1C, "_NP"),
    }
}

layout! {
    /// `OpReset`: size 0x20, puts what `np` names back to its reset value.
    OpReset = "OpReset", op: OpCode {
        np: ObjectRef<'a> = object_ref(0x1C, "_NP"),
    }
}

layout! {
    /// `OpDisplace`: size 0x30, moves `np` by `len` feet at `head` and
    /// `pitch` degrees; `combine_mode` 0 world axes, 1 absolute, 2 the
    /// target's frame (aj-vm).
    OpDisplace = "OpDisplace", op: OpCode {
        np: ObjectRef<'a> = object_ref(0x1C, "_NP"),
        combine_mode: u32 = u32(0x20, "_combineMode"),
        len_np: ObjectRef<'a> = object_ref(0x24, "_lenNP"),
        head_np: ObjectRef<'a> = object_ref(0x28, "_headNP"),
        pitch_np: ObjectRef<'a> = object_ref(0x2C, "_pitchNP"),
    }
}

layout! {
    /// `OpStartSequence`: size 0x28, starts the sequence `lhs` names on the
    /// receiver; its block runs at once when it starts.
    OpStartSequence = "OpStartSequence", op: OpCode {
        lhs: ObjectRef<'a> = object_ref(0x1C, "_LHS"),
        index_rhs: ObjectRef<'a> = object_ref(0x20, "_indexRHS"),
        cached_object: ObjectRef<'a> = object_ref(0x24, "_cachedObject"),
    }
}

layout! {
    /// `OpFindVariable`: size 0x3C, a named variable on `owner`, found,
    /// made or removed as `filter` says; its block runs when there is one.
    OpFindVariable = "OpFindVariable", op: OpCode {
        container_type: TypeRef<'a> = type_ref(0x1C, "_varContainerType"),
        contents_type: TypeRef<'a> = type_ref(0x20, "_varContentsType"),
        name: Option<&'a str> = string(0x24, "_varName"),
        filter: u32 = u32(0x28, "_filter"),
        skip_rhs: ObjectRef<'a> = object_ref(0x2C, "_skipRHS"),
        owner: ObjectRef<'a> = object_ref(0x30, "_owner"),
        index_rhs: ObjectRef<'a> = object_ref(0x34, "_indexRHS"),
        cached_object: ObjectRef<'a> = object_ref(0x38, "_cachedObject"),
    }
}

layout! {
    /// `OpForEach`: size 0x34, runs its block once per member of the set
    /// `lhs` names, rotated by `rhs`, in the order `dir` says.
    OpForEach = "OpForEach", op: OpCode {
        index: i32 = i32(0x1C, "_index"),
        lhs: ObjectRef<'a> = object_ref(0x20, "_LHS"),
        dir: u32 = u32(0x24, "_dir"),
        rhs: ObjectRef<'a> = object_ref(0x28, "_RHS"),
        cached_set: ObjectRef<'a> = object_ref(0x2C, "_cachedSet"),
        cached_object: ObjectRef<'a> = object_ref(0x30, "_cachedObject"),
    }
}

layout! {
    /// `OpFindSubSet`: size 0x30, the members of a set whose path value
    /// passes `relation` against `rhs`; its block runs when any do.
    /// `_setRemainder` is filled at link.
    OpFindSubSet = "OpFindSubSet", op: OpCode {
        lhs: ObjectRef<'a> = object_ref(0x1C, "_LHS"),
        relation: u32 = u32(0x20, "_relOperator"),
        rhs: ObjectRef<'a> = object_ref(0x24, "_RHS"),
        set_remainder: i32 = i32(0x28, "_setRemainder"),
        cached_object: ObjectRef<'a> = object_ref(0x2C, "_cachedObject"),
    }
}

layout! {
    /// `OpCheckMembership`: size 0x30, `if <rhs> in <lhs>` as
    /// `membership_op` says (0 all, 1 none, 2 any, 3 all with multiplicity).
    OpCheckMembership = "OpCheckMembership", op: OpCode {
        lhs: ObjectRef<'a> = object_ref(0x1C, "_LHS"),
        membership_op: u32 = u32(0x20, "_membershipOp"),
        rhs: ObjectRef<'a> = object_ref(0x24, "_RHS"),
        cached_type: ObjectRef<'a> = object_ref(0x28, "_cachedType"),
        cached_obj: ObjectRef<'a> = object_ref(0x2C, "_cachedObj"),
    }
}

layout! {
    /// `OpCheckFOV`: size 0x3C, the targets `lhs` names inside `facing` ±
    /// `fov`/2 degrees from `from_lhs` whose distance passes `relation`
    /// against `rhs`; its block runs when any do.
    OpCheckFov = "OpCheckFOV", op: OpCode {
        rhs_facing: ObjectRef<'a> = object_ref(0x1C, "_RHSfacing"),
        rhs_fov: ObjectRef<'a> = object_ref(0x20, "_RHSfov"),
        mode: u32 = u32(0x24, "_mode"),
        cached_object: ObjectRef<'a> = object_ref(0x28, "_cachedObject"),
        from_lhs: ObjectRef<'a> = object_ref(0x2C, "_fromLHS"),
        lhs: ObjectRef<'a> = object_ref(0x30, "_LHS"),
        relation: u32 = u32(0x34, "_relOperator"),
        rhs: ObjectRef<'a> = object_ref(0x38, "_RHS"),
    }
}

layout! {
    /// `OpTurnTo`: size 0x28, turns the receiver to `facing_rhs` or toward
    /// what `np` names; its block runs once the facing is reached.
    OpTurnTo = "OpTurnTo", op: OpCode {
        facing_rhs: ObjectRef<'a> = object_ref(0x1C, "_facingRHS"),
        np: ObjectRef<'a> = object_ref(0x20, "_NP"),
        index_rhs: ObjectRef<'a> = object_ref(0x24, "_indexRHS"),
    }
}

layout! {
    /// `OpAbstractMove`'s fields (size 0x30), what `OpMoveTo` and
    /// `OpMoveFrom` are: steer toward (or away from) what `np` names until
    /// the distance passes `rhs`, along waypoints as `dir` says.
    OpMove = "OpAbstractMove", op: OpCode {
        index_rhs: ObjectRef<'a> = object_ref(0x1C, "_indexRHS"),
        lhs: ObjectRef<'a> = object_ref(0x20, "_LHS"),
        dir: u32 = u32(0x24, "_dir"),
        rhs: ObjectRef<'a> = object_ref(0x28, "_RHS"),
        np: ObjectRef<'a> = object_ref(0x2C, "_NP"),
    }
}

layout! {
    /// `OpTeleportTo`: size 0x28, puts the receiver at what `lhs` names (or
    /// its next waypoint, as `dir` says), facing `facing_rhs`.
    OpTeleportTo = "OpTeleportTo", op: OpCode {
        lhs: ObjectRef<'a> = object_ref(0x1C, "_LHS"),
        dir: u32 = u32(0x20, "_dir"),
        facing_rhs: ObjectRef<'a> = object_ref(0x24, "_facingRHS"),
    }
}

/// `ValueStack`: size 0x1C, a path read as a value (an op's LHS, a check's
/// left side), `_otherPoint` the point a distance or heading on it is
/// measured from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ValueStack<'a> {
    pub stack: Stack<'a>,
    /// A `PositionStack`, or null.
    pub other_point: ObjectRef<'a>,
}

impl<'a> Layout<'a> for ValueStack<'a> {
    const TYPE: &'static str = "ValueStack";

    fn fields(v: &mut View<'_, 'a>) -> Result<Self, FormatError> {
        Ok(Self {
            stack: <Stack<'a> as Layout<'a>>::fields(v)?,
            other_point: v.object_ref(0x18, "_otherPoint")?,
        })
    }
}

/// `ScriptGroupStack`: size 0x1C, a path to a script object (a macro to
/// call, a placement, a behaviour), `_editType` what it must name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GroupStack<'a> {
    pub stack: Stack<'a>,
    pub edit_type: TypeRef<'a>,
}

impl<'a> Layout<'a> for GroupStack<'a> {
    const TYPE: &'static str = "ScriptGroupStack";

    fn fields(v: &mut View<'_, 'a>) -> Result<Self, FormatError> {
        Ok(Self {
            stack: <Stack<'a> as Layout<'a>>::fields(v)?,
            edit_type: v.type_ref(0x18, "_editType")?,
        })
    }
}
