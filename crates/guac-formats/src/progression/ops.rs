//! The script object the progression readers follow beyond AE.2's layouts
//! (openspec AH.2): a save param's class. The five op layouts AH.2 read
//! here moved to [`crate::script::ops`] with AH.13's 44 classes.

use crate::FormatError;
use crate::igz::layouts::{Layout, ObjectRef, TypeRef, View};

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
