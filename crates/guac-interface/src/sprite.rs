//! A sprite's own fields (the crate docs, The sprite) and the bits of its
//! three words, as AT.1 and AT.4 read them.

use guac_ir::interface::{Content, Matrix, Template};
use guac_script::Value;

/// A sprite in a [`crate::Model`]: its place in the model's list.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct SpriteId(pub u32);

/// A placement's state (the crate docs, States).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum State {
    /// Below 2: in `active`, laid out and drawn.
    Awake,
    /// 2: a template no script has woken.
    Asleep,
    /// 3: taken off by `remove`.
    Removed,
}

/// A field of one of a sprite's words: `width` bits from bit `shift`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Bits {
    pub shift: u32,
    pub width: u32,
}

impl Bits {
    pub const fn new(shift: u32, width: u32) -> Self {
        Self { shift, width }
    }

    pub const fn mask(self) -> u32 {
        ((1u64 << self.width) - 1) as u32
    }

    pub const fn get(self, word: u32) -> u32 {
        (word >> self.shift) & self.mask()
    }

    /// `word` with the field holding `value`'s low bits.
    pub const fn set(self, word: u32, value: u32) -> u32 {
        (word & !(self.mask() << self.shift)) | ((value & self.mask()) << self.shift)
    }
}

/// The properties word (AT.1).
pub mod props {
    use super::Bits;
    pub const TYPE: Bits = Bits::new(0, 3);
    pub const BLEND: Bits = Bits::new(3, 3);
    pub const JUSTIFY: Bits = Bits::new(6, 2);
    pub const ALIGNMENT: Bits = Bits::new(8, 4);
    pub const ALIGN_TO_PARENT: Bits = Bits::new(12, 4);
    pub const VISIBLE: u32 = 1 << 16;
    pub const POINT_SIZE: Bits = Bits::new(17, 12);
    pub const DIRTY: u32 = 1 << 29;
    pub const WIDTH_VALID: u32 = 1 << 30;
}

/// The flags word (AT.4).
pub mod flags {
    use super::Bits;
    pub const SIZED: u32 = 1;
    pub const WRAPS: u32 = 1 << 7;
    pub const FITS: u32 = 1 << 8;
    pub const PICTURE: u32 = 1 << 9;
    /// Bits 10-13, the text's string flags.
    pub const STRING: u32 = 0xF << 10;
    pub const POINT_SIZE_SET: u32 = 1 << 17;
    pub const FLIP: Bits = Bits::new(18, 2);
    pub const CLIPPING: u32 = 1 << 25;
}

/// The placement's own flags word.
pub mod placement {
    use super::Bits;
    pub const ATTACHED: u32 = 1;
    pub const BLOOM: Bits = Bits::new(1, 7);
}

/// The visual types, in the properties' order.
pub mod kind {
    pub const ART: u32 = 0;
    pub const TEXT: u32 = 1;
    pub const MODEL: u32 = 2;
    pub const CONTAINER: u32 = 3;
    pub const PARTICLES: u32 = 4;
}

/// What a clone was made from: its `parent` member.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Source {
    /// A template in a file, itself a placement.
    None,
    Sprite(SpriteId),
}

/// One sprite: a template's placement or a clone.
#[derive(Debug, Clone, PartialEq)]
pub struct Sprite {
    /// The template's name, for logs and the adapter.
    pub name: String,
    /// What it draws, as the extract read its template.
    pub content: Content,
    pub state: State,
    pub cloned_from: Source,
    /// 12.4 fixed point, x then y.
    pub location: [i16; 2],
    pub properties: u32,
    pub flags: u32,
    pub placement: u32,
    pub z_depth: f32,
    pub priority: f32,
    /// `_initMatrix`: where it was placed (the crate docs, The sprite).
    pub init_matrix: Matrix,
    /// `_currentMatrix`: where it stands; each layout writes its top-left
    /// into the place.
    pub matrix: Matrix,
    /// Its computed scale (x, depth, y) as its last layout left it, `None`
    /// before its first (what `screen location` scales its offset by).
    pub computed_scale: Option<[f32; 3]>,
    /// The ID `reset` puts back: the one it held before the first write to
    /// `ID` (its reset template's).
    pub reset_id: Option<i32>,
    /// Width, height, depth.
    pub size: [f32; 3],
    /// Percent per axis.
    pub scale: [f32; 3],
    pub color: [u8; 4],
    /// Red, green, blue, and the replace percentage on 0-255.
    pub replace: [u8; 4],
    /// The string it shows, as the scripts handed it (a file's `StringInfo`
    /// or one the converter made), or `Value::Null`.
    pub text: Value,
    pub numeric_value: f32,
    pub parent: Option<SpriteId>,
    /// `attached objects`, in the order they were hung.
    pub children: Vec<SpriteId>,
    /// The awake clones made from it.
    pub clones: Vec<SpriteId>,
    pub id: i32,
    pub activator: Value,
    /// Its font: an index into the model's fonts (the host resolves a
    /// template's `FontDataInfo`, its own archive's or `global.bld`'s).
    pub font: Option<usize>,
    /// The letters' colours the text colour converter set, one per glyph
    /// drawn, in order (the crate docs, Text).
    pub letters: Vec<[u8; 4]>,
}

impl Sprite {
    /// A template's placement, asleep, its fields as the file holds them.
    pub fn from_template(t: &Template, text: Value) -> Self {
        Sprite {
            name: t.name.clone(),
            content: t.content.clone(),
            state: State::Asleep,
            cloned_from: Source::None,
            location: t.position,
            properties: t.properties,
            flags: t.flags as u32,
            placement: 0,
            z_depth: t.z_depth,
            priority: t.priority,
            init_matrix: t.init_matrix,
            matrix: t.matrix,
            computed_scale: None,
            reset_id: None,
            size: [t.width, t.height, t.depth],
            scale: t.scale,
            color: t.color,
            replace: t.replace_color,
            text,
            numeric_value: t.num_value as f32,
            parent: None,
            children: Vec::new(),
            clones: Vec::new(),
            id: 0,
            activator: Value::Null,
            font: None,
            letters: Vec::new(),
        }
    }

    /// A clone of `self`: every field copied, awake, hung on nothing, holding
    /// nothing; its initial matrix `self`'s current one, and its current one
    /// that, as its waking copies it (0x69D7C, 0x69508; the spawn's `at` and
    /// facing are the host's).
    pub fn clone_of(&self, source: SpriteId) -> Self {
        Sprite {
            state: State::Awake,
            cloned_from: Source::Sprite(source),
            init_matrix: self.matrix,
            computed_scale: None,
            reset_id: None,
            placement: self.placement & !placement::ATTACHED,
            parent: None,
            children: Vec::new(),
            clones: Vec::new(),
            activator: Value::Null,
            letters: Vec::new(),
            ..self.clone()
        }
    }

    pub fn kind(&self) -> u32 {
        props::TYPE.get(self.properties)
    }

    pub fn sized(&self) -> bool {
        self.flags & flags::SIZED != 0
    }

    pub fn has_text(&self) -> bool {
        self.text != Value::Null
    }

    pub fn mark_dirty(&mut self) {
        self.properties |= props::DIRTY;
    }
}
