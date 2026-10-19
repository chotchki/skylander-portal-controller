//! The script objects the roster reads (openspec AE.1): `StringInfo` (a text
//! key; its text lives in the archive's language packs), `ValueInfo` (a named
//! number), `ScriptSet` and its `ScriptSetList`, the 3D-UI rig's
//! `DirectionalLightInfo` and a `.pak`'s `LanguagePackInfo`.
//!
//! Every one sits under `tfbScriptObject`, whose `_getFunc`, `_setFunc` and
//! `_resetFunc` (0x0C-0x17) are zero and unmarked in every file: they aren't
//! read, and a fixup on one is refused like any undeclared word.

use super::{DataList, Info, Layout, ObjectRef, TypeRef, View};
use crate::FormatError;
use crate::igz::MemoryRef;

/// An `igVec4ucMetaField`: four bytes, RGBA as stored.
pub type Rgba8 = [u8; 4];

fn rgba(v: &mut View<'_, '_>, offset: u32, field: &'static str) -> Result<Rgba8, FormatError> {
    v.u32(offset, field).map(u32::to_be_bytes)
}

/// `StringInfo`: size 0x28, a text key. The name is the key; the text is
/// the n-th string of each language pack, n its place among the archive's
/// `StringInfo`s in address order ([`crate::language`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StringInfo<'a> {
    pub name: Option<&'a str>,
    /// `igRawRefMetaField`, 0 in files.
    pub unicode_text: u32,
    pub flags: i32,
    pub integer_digits: i32,
    pub decimal_digits: i32,
}

impl StringInfo<'_> {
    pub const NAME: u32 = 0x08;
    pub const UNICODE_TEXT: u32 = 0x18;
    pub const FLAGS: u32 = 0x1C;
    pub const INTEGER_DIGITS: u32 = 0x20;
    pub const DECIMAL_DIGITS: u32 = 0x24;
}

impl<'a> Layout<'a> for StringInfo<'a> {
    const TYPE: &'static str = "StringInfo";

    fn fields(v: &mut View<'_, 'a>) -> Result<Self, FormatError> {
        Ok(Self {
            name: v.string(Self::NAME, "_name")?,
            unicode_text: v.u32(Self::UNICODE_TEXT, "_unicodeText")?,
            flags: v.i32(Self::FLAGS, "_flags")?,
            integer_digits: v.i32(Self::INTEGER_DIGITS, "_integerDigits")?,
            decimal_digits: v.i32(Self::DECIMAL_DIGITS, "_decimalDigits")?,
        })
    }
}

/// `ValueInfo`: size 0x24, a named number (`ElementIndex_Fire` and the like).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ValueInfo<'a> {
    pub name: Option<&'a str>,
    pub reset_value: i32,
    /// The measurement class `_value` is read as (`IntMeasurement`).
    pub value_type: TypeRef<'a>,
    /// The value's bits: an int, or a float's bits for a float class.
    pub value: i32,
}

impl ValueInfo<'_> {
    pub const NAME: u32 = 0x08;
    pub const RESET_VALUE: u32 = 0x18;
    pub const TYPE_FIELD: u32 = 0x1C;
    pub const VALUE: u32 = 0x20;
}

impl<'a> Layout<'a> for ValueInfo<'a> {
    const TYPE: &'static str = "ValueInfo";

    fn fields(v: &mut View<'_, 'a>) -> Result<Self, FormatError> {
        Ok(Self {
            name: v.string(Self::NAME, "_name")?,
            reset_value: v.i32(Self::RESET_VALUE, "_resetValue")?,
            value_type: v.type_ref(Self::TYPE_FIELD, "_type")?,
            value: v.i32(Self::VALUE, "_value")?,
        })
    }
}

/// `ScriptSet`: size 0x1C, a named set. Its members are its list's.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ScriptSet<'a> {
    pub name: Option<&'a str>,
    /// A `ScriptSetList`.
    pub list: Option<usize>,
}

impl ScriptSet<'_> {
    pub const NAME: u32 = 0x08;
    pub const LIST: u32 = 0x18;
}

impl<'a> Layout<'a> for ScriptSet<'a> {
    const TYPE: &'static str = "ScriptSet";

    fn fields(v: &mut View<'_, 'a>) -> Result<Self, FormatError> {
        Ok(Self {
            name: v.string(Self::NAME, "_name")?,
            list: v.object(Self::LIST, "_list", ScriptSetList::TYPE)?,
        })
    }
}

/// `ScriptSetList`: size 0x24, a set's ordered members (an object list
/// whose elements can be local, imported or named).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScriptSetList<'a> {
    pub list: DataList<'a>,
    /// The members' class.
    pub edit_type: TypeRef<'a>,
    /// Storage for the random-choice bitfields.
    pub properties: u32,
    pub next_history: u32,
    pub elements: Vec<ObjectRef<'a>>,
}

impl ScriptSetList<'_> {
    pub const EDIT_TYPE: u32 = 0x18;
    pub const PROPERTIES: u32 = 0x1C;
    pub const NEXT_HISTORY: u32 = 0x20;
}

impl<'a> Layout<'a> for ScriptSetList<'a> {
    const TYPE: &'static str = "ScriptSetList";

    fn fields(v: &mut View<'_, 'a>) -> Result<Self, FormatError> {
        let list = DataList::fields(v)?;
        let elements = v.element_refs(&list)?;
        Ok(Self {
            list,
            edit_type: v.type_ref(Self::EDIT_TYPE, "_editType")?,
            properties: v.u32(Self::PROPERTIES, "_properties")?,
            next_history: v.u32(Self::NEXT_HISTORY, "_nextHistory")?,
            elements,
        })
    }
}

/// A file's root list (its first object, an `igObjectList`) with every
/// element kept: `global.bld`'s holds an import among its objects.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RootList<'a> {
    pub list: DataList<'a>,
    pub elements: Vec<ObjectRef<'a>>,
}

impl<'a> Layout<'a> for RootList<'a> {
    const TYPE: &'static str = "igObjectList";

    fn fields(v: &mut View<'_, 'a>) -> Result<Self, FormatError> {
        let list = DataList::fields(v)?;
        let elements = v.element_refs(&list)?;
        Ok(Self { list, elements })
    }
}

/// `tfbEulerTransformMetaField`: a position, then Euler angles in degrees
/// (ag-levels; the 3D-UI key light's pitch is the first angle, -45).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct EulerTransform {
    pub position: [f32; 3],
    pub rotation: [f32; 3],
}

impl EulerTransform {
    pub const LEN: u32 = 0x18;

    pub(super) fn read(
        v: &mut View<'_, '_>,
        offset: u32,
        field: &'static str,
    ) -> Result<Self, FormatError> {
        Ok(Self {
            position: v.floats(offset, field)?,
            rotation: v.floats(offset + 0x0C, field)?,
        })
    }
}

/// `DirectionalLightInfo`: size 0x40, one light of a rig (`global.bld`'s
/// `_spriteLight1-3` light 3D drawn in the UI).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DirectionalLightInfo<'a> {
    pub name: Option<&'a str>,
    pub diffuse: Rgba8,
    pub ambient: Rgba8,
    pub specular: Rgba8,
    pub multiplier: f32,
    pub init_matrix: EulerTransform,
}

impl DirectionalLightInfo<'_> {
    pub const NAME: u32 = 0x08;
    pub const DIFFUSE: u32 = 0x18;
    pub const AMBIENT: u32 = 0x1C;
    pub const SPECULAR: u32 = 0x20;
    pub const MULTIPLIER: u32 = 0x24;
    pub const INIT_MATRIX: u32 = 0x28;
}

impl<'a> Layout<'a> for DirectionalLightInfo<'a> {
    const TYPE: &'static str = "DirectionalLightInfo";

    fn fields(v: &mut View<'_, 'a>) -> Result<Self, FormatError> {
        Ok(Self {
            name: v.string(Self::NAME, "_name")?,
            diffuse: rgba(v, Self::DIFFUSE, "_diffuseColor")?,
            ambient: rgba(v, Self::AMBIENT, "_ambientColor")?,
            specular: rgba(v, Self::SPECULAR, "_specularColor")?,
            multiplier: v.f32(Self::MULTIPLIER, "_multiplier")?,
            init_matrix: EulerTransform::read(v, Self::INIT_MATRIX, "_initMatrix")?,
        })
    }
}

/// `LanguagePackInfo`: size 0x20, through `igInfo`. One per `.pak`; its
/// `_stringDict` is the pack's text, UTF-16BE strings each ending in a NUL,
/// back to back with no index ([`crate::language`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LanguagePackInfo<'a> {
    /// The name is the language's (`ENGLISH`).
    pub info: Info<'a>,
    pub lang_id: i32,
    pub string_dict: Option<MemoryRef<'a>>,
}

impl LanguagePackInfo<'_> {
    pub const LANG_ID: u32 = 0x14;
    pub const STRING_DICT: u32 = 0x18;
}

impl<'a> Layout<'a> for LanguagePackInfo<'a> {
    const TYPE: &'static str = "LanguagePackInfo";

    fn fields(v: &mut View<'_, 'a>) -> Result<Self, FormatError> {
        Ok(Self {
            info: Info::fields(v)?,
            lang_id: v.i32(Self::LANG_ID, "_langID")?,
            string_dict: v.memory_ref(Self::STRING_DICT, "_stringDict")?,
        })
    }
}
