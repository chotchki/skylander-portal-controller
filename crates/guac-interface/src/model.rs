//! The model: every sprite, the tree they hang in, the class's two lists, the
//! global converters and every member's get, set and reset over
//! `guac-script`'s values (the crate docs, Members and States).

use std::collections::HashMap;

use guac_ir::interface::{Content, Font, Matrix, Template};
use guac_script::{Obj, Value, convert};

use crate::matrix;
use crate::members::Member;
use crate::sprite::{Source, Sprite, SpriteId, State, flags, kind, placement, props};
use crate::text::{self, Metrics, TextIn};

/// Where the model's own objects sit among the host's entity ids: sprite `n`
/// reads `Value::Entity(sprites + n)`, a string the converter made
/// `strings + n`, the global converters `singletons + 0..3`. The host picks
/// ranges its actors don't use.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Bases {
    pub sprites: u64,
    pub strings: u64,
    pub singletons: u64,
}

impl Default for Bases {
    fn default() -> Self {
        Bases {
            sprites: 1 << 40,
            strings: 2 << 40,
            singletons: 3 << 40,
        }
    }
}

/// The global objects, by offset from [`Bases::singletons`].
pub mod singleton {
    pub const STRING_CONVERTER: u64 = 0;
    pub const TEXT_COLOR_CONVERTER: u64 = 1;
    pub const NAME_EXTRACTOR: u64 = 2;
}

/// What a member reads: one value, a set's elements, or `scale`'s three
/// percents.
#[derive(Debug, Clone, PartialEq)]
pub enum Answer {
    Value(Value),
    /// A set's elements, in order.
    Elements(Vec<Value>),
    /// `scale`'s three percents: the measurement its parts are read and
    /// written through ([`Model::set_scale`]).
    Scale([f32; 3]),
    /// A placement's matrix (`spritematrixmeasurement.origin`): the host
    /// hands it on as its matrices go.
    Matrix(Matrix),
}

/// Why a member gave no answer or took no write.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Refusal {
    /// No setter is registered.
    ReadOnly,
    /// The path goes through nothing (a sprite not there, a value the member
    /// can't take): a write drops and a check skips, quietly.
    Nothing,
    /// Not modelled here yet; the box that models it is named where it's
    /// refused.
    Unmodelled,
}

/// What the model asks of what it doesn't hold: a kind's answer the adapter
/// knows better (a model's, an emitter's), an image still loading, an
/// object's name and its class's numbers. `None` is "not known": the model
/// answers itself where the crate docs give it a rule and refuses as
/// [`Refusal::Unmodelled`] otherwise.
pub trait World {
    /// The kind's computed width, height and depth for a sprite not *sized*.
    fn size(&self, model: &Model, sprite: SpriteId) -> Option<[f32; 3]> {
        let _ = (model, sprite);
        None
    }
    /// What the kind measures for `natural size`, in units.
    fn natural_size(&self, model: &Model, sprite: SpriteId) -> Option<[f32; 2]> {
        let _ = (model, sprite);
        None
    }
    /// Whether an art sprite's texture data is still loading.
    fn loading(&self, model: &Model, sprite: SpriteId) -> bool {
        let _ = (model, sprite);
        false
    }
    /// An object's name, for `nameextractor.Name checksum`.
    fn name_of(&self, value: Value) -> Option<String> {
        let _ = value;
        None
    }
    /// What an object answers at its virtual slots 24 and 25, for
    /// `nameextractor`'s `Instantiation size` and `type count`; `None` reads
    /// 0, as no object does.
    fn class_numbers(&self, value: Value) -> Option<[i32; 2]> {
        let _ = value;
        None
    }
}

/// A world that knows nothing past the model.
pub struct NoWorld;
impl World for NoWorld {}

/// A string: its UTF-16 units and the converter's settings it carries.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Str {
    pub units: Vec<u16>,
    /// `StringInfo._flags`: 0x400 a time, 0x800 has an `@`, 0x2000 commas.
    pub flags: u32,
    pub integer_digits: i32,
    pub decimal_digits: i32,
}

/// String flags (`StringInfo._flags`), the sprite's flags bits 10-13.
pub mod string_flags {
    pub const TIME: u32 = 0x400;
    pub const HAS_AT: u32 = 0x800;
    pub const COMMAS: u32 = 0x2000;
}

#[derive(Debug, Clone, PartialEq)]
struct StringConverter {
    value_set: Option<Vec<Value>>,
    text: Value,
    time: bool,
    integer_digits: i32,
    decimal_digits: i32,
    comma: bool,
}

#[derive(Debug, Clone, PartialEq)]
struct TextColorConverter {
    sprite: Value,
    colors: Option<Vec<Value>>,
}

impl Default for StringConverter {
    fn default() -> Self {
        StringConverter {
            value_set: None,
            text: Value::Null,
            time: false,
            integer_digits: 0,
            decimal_digits: 0,
            comma: false,
        }
    }
}

impl Default for TextColorConverter {
    fn default() -> Self {
        TextColorConverter {
            sprite: Value::Null,
            colors: None,
        }
    }
}

/// The model of the original's interface.
#[derive(Debug, Clone)]
pub struct Model {
    bases: Bases,
    sprites: Vec<Sprite>,
    templates: HashMap<Obj, SpriteId>,
    pub(crate) active: Vec<SpriteId>,
    removed: Vec<SpriteId>,
    strings: Vec<Str>,
    file_strings: HashMap<Obj, usize>,
    converter: StringConverter,
    text_colors: TextColorConverter,
    name_object: Value,
    fonts: Vec<Font>,
    /// The frame's sprite list: every sprite drawn, in the order each
    /// started being drawn (the crate docs, Draw order).
    pub(crate) drawing: Vec<SpriteId>,
    /// `viewporttitlesafeinfo`'s margins: left, right, upper, lower. 0 until
    /// AT.10 reads the original's, as the value cells read today.
    pub title_safe: [i32; 4],
    /// The root's width and height, what a sprite with no parent sprite
    /// hangs on (the crate docs, Layout): `RT Viewport 0`'s 1280 by 720 in
    /// the June savestate.
    pub frame: [f32; 2],
}

impl Default for Model {
    fn default() -> Self {
        Model::new(Bases::default())
    }
}

/// A float to an integer toward 0, saturating, NaN 0: `guac-script`'s rule.
fn truncate(f: f32) -> i32 {
    f as i32
}

/// A 12.4 screen value from two halves in units.
fn screen(x: f32, y: f32) -> Value {
    let half = |v: f32| u32::from((v * 16.0) as i32 as i16 as u16);
    Value::Screen(half(x) << 16 | half(y))
}

fn color(c: [u8; 4]) -> Value {
    Value::Color(u32::from_be_bytes(c))
}

impl Model {
    pub fn new(bases: Bases) -> Self {
        Model {
            bases,
            sprites: Vec::new(),
            templates: HashMap::new(),
            active: Vec::new(),
            removed: Vec::new(),
            strings: Vec::new(),
            file_strings: HashMap::new(),
            converter: StringConverter::default(),
            text_colors: TextColorConverter::default(),
            name_object: Value::Null,
            fonts: Vec::new(),
            drawing: Vec::new(),
            title_safe: [0; 4],
            frame: [1280.0, 720.0],
        }
    }

    pub fn bases(&self) -> Bases {
        self.bases
    }

    /// A template's placement, asleep. `obj` is its object in its file, so
    /// a script's reference to the template finds it; `text` the string its
    /// template names, as the host reads it.
    pub fn add_template(&mut self, obj: Option<Obj>, t: &Template, text: Value) -> SpriteId {
        let id = SpriteId(self.sprites.len() as u32);
        self.sprites.push(Sprite::from_template(t, text));
        if let Some(obj) = obj {
            self.templates.insert(obj, id);
        }
        id
    }

    /// A string in a file (a `StringInfo`), so a sprite showing it can be
    /// measured and the converter can take it apart.
    pub fn add_file_string(&mut self, obj: Obj, text: &str, s: Str) -> Value {
        let n = self.strings.len();
        self.strings.push(Str {
            units: text.encode_utf16().collect(),
            ..s
        });
        self.file_strings.insert(obj, n);
        Value::Object(obj)
    }

    /// A font the sprites can draw in; its index is what [`Model::set_font`]
    /// takes.
    pub fn add_font(&mut self, font: Font) -> usize {
        self.fonts.push(font);
        self.fonts.len() - 1
    }

    pub fn font(&self, index: usize) -> Option<&Font> {
        self.fonts.get(index)
    }

    /// A sprite's font (a template's, before it is cloned: clones copy it).
    pub fn set_font(&mut self, id: SpriteId, font: Option<usize>) {
        self.sprite_mut(id).font = font;
    }

    /// The font a text sprite measures and draws in, if it has one (the
    /// default font is null, the crate docs' Text).
    pub fn metrics(&self, id: SpriteId) -> Option<Metrics<'_>> {
        self.sprite(id)
            .font
            .and_then(|f| self.fonts.get(f))
            .map(Metrics::new)
    }

    /// What a text sprite's text shows (the crate docs, Text): its string,
    /// the number filled in when the string has an `@`, or the name of the
    /// object it holds; `None` with no string.
    pub fn text_units(&self, id: SpriteId, world: &dyn World) -> Option<Vec<u16>> {
        let s = self.sprite(id);
        if s.text == Value::Null {
            return None;
        }
        match self.string(s.text) {
            Some(st) if st.flags & (string_flags::HAS_AT | string_flags::TIME) != 0 => {
                Some(text::fill(st, s.numeric_value))
            }
            Some(st) => Some(st.units.clone()),
            None => world.name_of(s.text).map(|n| n.encode_utf16().collect()),
        }
    }

    /// What the text's layout reads of the sprite.
    pub fn text_in(&self, id: SpriteId) -> TextIn {
        let s = self.sprite(id);
        TextIn {
            flags: s.flags,
            point_size: props::POINT_SIZE.get(s.properties),
            width: s.size[0],
            height: s.size[1],
        }
    }

    pub fn sprite(&self, id: SpriteId) -> &Sprite {
        &self.sprites[id.0 as usize]
    }

    pub(crate) fn sprite_mut(&mut self, id: SpriteId) -> &mut Sprite {
        &mut self.sprites[id.0 as usize]
    }

    pub fn len(&self) -> usize {
        self.sprites.len()
    }

    pub fn is_empty(&self) -> bool {
        self.sprites.is_empty()
    }

    pub fn active(&self) -> &[SpriteId] {
        &self.active
    }

    pub fn removed(&self) -> &[SpriteId] {
        &self.removed
    }

    /// The value a script holds for `id`.
    pub fn value_of(&self, id: SpriteId) -> Value {
        Value::Entity(self.bases.sprites + u64::from(id.0))
    }

    /// The sprite a value names: one of the model's, or a template's object.
    pub fn sprite_of(&self, v: Value) -> Option<SpriteId> {
        match v {
            Value::Entity(e) if e >= self.bases.sprites => {
                let n = e - self.bases.sprites;
                (n < self.sprites.len() as u64).then_some(SpriteId(n as u32))
            }
            Value::Object(obj) => self.templates.get(&obj).copied(),
            _ => None,
        }
    }

    /// A global converter's entity: `which` (a [`singleton`] offset) past
    /// [`Bases::singletons`].
    pub fn singleton(&self, which: u64) -> Value {
        Value::Entity(self.bases.singletons + which)
    }

    /// The string a value names, a file's or one the converter made.
    pub fn string(&self, v: Value) -> Option<&Str> {
        match v {
            Value::Entity(e) if e >= self.bases.strings && e < self.bases.singletons => {
                self.strings.get((e - self.bases.strings) as usize)
            }
            Value::Object(obj) => self.file_strings.get(&obj).map(|&n| &self.strings[n]),
            _ => None,
        }
    }

    fn add_string(&mut self, s: Str) -> Value {
        self.strings.push(s);
        Value::Entity(self.bases.strings + (self.strings.len() - 1) as u64)
    }

    /// Wakes a template's placement: into `active`, at its end, its current
    /// matrix its initial one (0x69508).
    pub fn wake(&mut self, id: SpriteId) {
        if self.sprite(id).state == State::Asleep {
            let s = self.sprite_mut(id);
            s.state = State::Awake;
            s.matrix = s.init_matrix;
            self.active.push(id);
        }
    }

    /// A sprite reset as a path element (0x6A020, the crate docs' set
    /// reset): an awake clone taken off and woken again, its source and
    /// activator kept (off its parent, its children off it, last in `active`,
    /// its current matrix its initial one); an awake template not a clone
    /// put to sleep. Anything else as it is (**Hypothesis** for a removed
    /// clone, which the original may wake).
    fn restart(&mut self, id: SpriteId) {
        let s = self.sprite(id);
        if s.state != State::Awake {
            return;
        }
        let (clone, activator) = (s.cloned_from != Source::None, s.activator);
        // The deactivate hook (0x6A620), as `remove` runs it.
        self.attach(id, None);
        while let Some(&child) = self.sprite(id).children.last() {
            self.attach(child, None);
        }
        self.active.retain(|&a| a != id);
        let s = self.sprite_mut(id);
        s.activator = Value::Null;
        if clone {
            s.matrix = s.init_matrix;
            s.activator = activator;
            self.active.push(id);
        } else {
            s.state = State::Asleep;
        }
    }

    /// A clone of `source` (a template's placement or a clone): every field
    /// copied, awake, last in `active` and in `source`'s `clones`.
    pub fn spawn(&mut self, source: SpriteId) -> SpriteId {
        let id = SpriteId(self.sprites.len() as u32);
        let clone = self.sprite(source).clone_of(source);
        self.sprites.push(clone);
        self.sprite_mut(source).clones.push(id);
        self.active.push(id);
        id
    }

    /// `remove`: the crate docs' States.
    pub fn remove(&mut self, id: SpriteId) {
        let was = self.sprite(id).state;
        if was == State::Removed {
            return;
        }
        if was == State::Awake {
            let s = self.sprite_mut(id);
            s.activator = Value::Null;
            self.attach(id, None);
            while let Some(&child) = self.sprite(id).children.last() {
                self.attach(child, None);
            }
            self.active.retain(|&a| a != id);
        }
        if let Source::Sprite(from) = self.sprite(id).cloned_from {
            self.sprite_mut(from).clones.retain(|&c| c != id);
        }
        self.sprite_mut(id).state = State::Removed;
        self.removed.push(id);
    }

    /// An `attach node` write: off its parent first, then onto `parent` when
    /// the sprite and `parent` are awake and `parent` is neither the sprite
    /// nor one of its descendants; last in `parent`'s `attached objects`.
    pub fn attach(&mut self, id: SpriteId, parent: Option<SpriteId>) {
        if let Some(old) = self.sprite(id).parent {
            self.sprite_mut(old).children.retain(|&c| c != id);
            let s = self.sprite_mut(id);
            s.parent = None;
            s.placement &= !placement::ATTACHED;
        }
        let Some(parent) = parent else { return };
        if self.sprite(id).state != State::Awake || self.sprite(parent).state != State::Awake {
            return;
        }
        let mut up = Some(parent);
        while let Some(p) = up {
            if p == id {
                return;
            }
            up = self.sprite(p).parent;
        }
        self.sprite_mut(parent).children.push(id);
        let s = self.sprite_mut(id);
        s.parent = Some(parent);
        s.placement |= placement::ATTACHED;
    }

    /// Whether `id`'s `numeric value` reads as a float: its text is a string
    /// with decimal digits.
    fn reads_float(&self, id: SpriteId) -> bool {
        self.string(self.sprite(id).text)
            .is_some_and(|s| s.decimal_digits >= 1)
    }

    /// The size the layout takes for a sprite that isn't text: what `width`
    /// and `height` read, or its stored numbers where its kind won't say.
    pub(crate) fn size_of(&self, id: SpriteId, world: &dyn World) -> [f32; 3] {
        self.size_read(id, world).unwrap_or(self.sprite(id).size)
    }

    /// The frame's sprite list, in the order each started being drawn.
    pub fn drawing(&self) -> &[SpriteId] {
        &self.drawing
    }

    /// The width, height and depth `width`, `height` and `depth` read.
    fn size_read(&self, id: SpriteId, world: &dyn World) -> Result<[f32; 3], Refusal> {
        let s = self.sprite(id);
        if s.sized() {
            return Ok(s.size);
        }
        self.kind_size(id, world)
    }

    /// The size a sprite reads unwritten, *sized* or not: the world's when
    /// it knows it, else its kind's (the crate docs: art's own size, the
    /// text's size, a container's stored numbers).
    fn kind_size(&self, id: SpriteId, world: &dyn World) -> Result<[f32; 3], Refusal> {
        let s = self.sprite(id);
        if let Some(size) = world.size(self, id) {
            return Ok(size);
        }
        match (s.kind(), &s.content) {
            // Its image's size when in, a bound texture's desired size × 0.5,
            // else its stored size × 0.5 (0xB6950; 0.5 at 0x724360).
            (kind::ART, Content::Image(a) | Content::Movie(a)) => Ok(match (a.size, a.desired) {
                (Some([w, h]), _) => [w as f32, h as f32, s.size[2]],
                (None, [w, h]) if w > 0 && h > 0 => [0.5 * w as f32, 0.5 * h as f32, s.size[2]],
                _ => [0.5 * s.size[0], 0.5 * s.size[1], s.size[2]],
            }),
            (kind::ART, _) => Ok([0.5 * s.size[0], 0.5 * s.size[1], s.size[2]]),
            // 0xB7F08: no font returns what it was handed; no string
            // measures 0 wide.
            (kind::TEXT, _) => Ok(match self.metrics(id) {
                None => s.size,
                Some(m) => {
                    let units = self.text_units(id, world).unwrap_or_default();
                    let [w, h] = text::size(&m, &self.text_in(id), &units);
                    [w, h, s.size[2]]
                }
            }),
            _ => Ok(s.size),
        }
    }

    /// What `natural size` reads: the world's when it knows it, a text's
    /// measure, else what the size reads unwritten.
    fn natural(&self, id: SpriteId, world: &dyn World) -> Result<[f32; 2], Refusal> {
        if let Some(n) = world.natural_size(self, id) {
            return Ok(n);
        }
        let s = self.sprite(id);
        if s.kind() == kind::TEXT {
            // No font or no string: the cached width (0) by the stored
            // height (0xB81E8).
            return Ok(match (self.metrics(id), self.text_units(id, world)) {
                (Some(m), Some(units)) => text::natural_size(&m, &self.text_in(id), &units),
                _ => [0.0, s.size[1]],
            });
        }
        let [w, h, _] = self.kind_size(id, world)?;
        Ok([w, h])
    }

    /// Reads a sprite's member as the crate docs' Members give it; a
    /// computed one the [`World`] doesn't know yet refuses as unmodelled.
    pub fn get(&self, id: SpriteId, m: &Member, world: &dyn World) -> Result<Answer, Refusal> {
        let s = self.sprite(id);
        let int = |i: u32| Ok(Answer::Value(Value::Int(i as i32)));
        let float = |f: f32| Ok(Answer::Value(Value::Float(f)));
        let sprites = |ids: &[SpriteId]| {
            Ok(Answer::Elements(
                ids.iter().map(|&i| self.value_of(i)).collect(),
            ))
        };
        match (m.namespace, m.name) {
            ("spriteinfo", "type") => int(s.kind()),
            ("spriteinfo", "visible") => {
                let on = s.properties & props::VISIBLE != 0;
                let loading = on
                    && s.kind() == kind::ART
                    && matches!(s.content, Content::Image(_) | Content::Movie(_))
                    && world.loading(self, id);
                int(if loading { 2 } else { u32::from(on) })
            }
            // The replace colour's read answers the colour (0xB3358).
            ("spriteinfo", "color") | ("spriteinfo", "replace color") => {
                Ok(Answer::Value(color(s.color)))
            }
            ("spriteinfo", "scale") => Ok(Answer::Scale(s.scale)),
            ("spriteinfo", "location") => Ok(Answer::Value(Value::Screen(
                u32::from(s.location[0] as u16) << 16 | u32::from(s.location[1] as u16),
            ))),
            ("spriteinfo", "screen location") => {
                // Its anchor's offset off its stored size, at its last
                // computed scale, carried by its current matrix (0xB3480).
                let a = props::ALIGNMENT.get(s.properties);
                let mut x = (a % 3) as f32 * 0.5 * s.size[0];
                let mut y = (a / 3) as f32 * 0.5 * s.size[1];
                if let Some(c) = s.computed_scale {
                    x *= c[0];
                    y *= c[2];
                }
                let [x, _, y] = matrix::carry(&matrix::rows(&s.matrix), [x, 0.0, y]);
                Ok(Answer::Value(screen(x, y)))
            }
            ("spriteinfo", "screen width") | ("spriteinfo", "screen height") => {
                let axis = usize::from(m.name == "screen height");
                let mut v = self.size_read(id, world)?[axis];
                let mut at = Some(id);
                while let Some(a) = at {
                    v *= self.sprite(a).scale[axis] * 0.01;
                    at = self.sprite(a).parent;
                }
                float(v)
            }
            ("spriteinfo", "text") => Ok(Answer::Value(s.text)),
            ("spriteinfo", "priority") => float(s.priority),
            ("spriteinfo", "Z Depth") => float(s.z_depth),
            ("spriteinfo", "width") => float(self.size_read(id, world)?[0]),
            ("spriteinfo", "height") => float(self.size_read(id, world)?[1]),
            ("spriteinfo", "depth") => float(self.size_read(id, world)?[2]),
            ("spriteinfo", "natural size") => {
                let [w, h] = self.natural(id, world)?;
                Ok(Answer::Value(screen(w, h)))
            }
            ("spriteinfo", "origin") => Ok(Answer::Value(self.value_of(id))),
            ("spriteinfo", "blend mode") => int(props::BLEND.get(s.properties)),
            ("spriteinfo", "alignment") => int(props::ALIGNMENT.get(s.properties)),
            ("spriteinfo", "align to parent") => int(props::ALIGN_TO_PARENT.get(s.properties)),
            ("spriteinfo", "texture flip") => int(flags::FLIP.get(s.flags)),
            ("spriteinfo", "clipping") => int((s.flags >> 25) & 1),
            ("spriteinfo", "Take a Picture") => int(u32::from(s.flags & flags::PICTURE != 0)),
            ("spriteinfo", "numeric value") => Ok(Answer::Value(if self.reads_float(id) {
                Value::Float(s.numeric_value)
            } else {
                Value::Int(truncate(s.numeric_value))
            })),
            ("spriteinfo", "justify") => int(props::JUSTIFY.get(s.properties)),
            ("spriteinfo", "point size") => int(props::POINT_SIZE.get(s.properties)),
            // A model's animations and attach nodes and an emitter's
            // generators are the adapter's (AT.13); none for art or text.
            ("spriteinfo", "playing animations")
            | ("spriteinfo", "model attach nodes")
            | ("spriteinfo", "generators") => Ok(Answer::Elements(Vec::new())),
            ("spriteinfo", "replace color percentage") => {
                int((u32::from(s.replace[3]) * 100 + 127) / 255)
            }
            ("spriteinfo", "bloom intensity") => int(placement::BLOOM.get(s.placement).min(100)),
            ("spriteinfo", "active") => sprites(&self.active),
            ("spriteinfo", "removed") => sprites(&self.removed),
            ("spriteinfo", "Text Color Converter") => Ok(Answer::Value(
                self.singleton(singleton::TEXT_COLOR_CONVERTER),
            )),
            ("spritematrixmeasurement", "attach node") => Ok(Answer::Value(
                s.parent.map_or(Value::Null, |p| self.value_of(p)),
            )),
            ("spritematrixmeasurement", "attached objects") => sprites(&s.children),
            ("spritematrixmeasurement", "-orientation") => Ok(Answer::Value(self.value_of(id))),
            ("spritematrixmeasurement", "ID") => Ok(Answer::Value(Value::Int(s.id))),
            ("spritematrixmeasurement", "clones") => sprites(&s.clones),
            ("spritematrixmeasurement", "parent") => Ok(Answer::Value(match s.cloned_from {
                Source::Sprite(from) => self.value_of(from),
                Source::None => Value::Null,
            })),
            ("spritematrixmeasurement", "activator") => Ok(Answer::Value(s.activator)),
            // Its initial matrix (0x6A914).
            ("spritematrixmeasurement", "origin") => Ok(Answer::Matrix(s.init_matrix)),
            _ => Err(Refusal::Unmodelled),
        }
    }

    /// A paired `spritepositionmeasurement` read: `id`'s place against
    /// `other`'s, each its current matrix's (the crate docs).
    pub fn get_paired(&self, id: SpriteId, other: SpriteId, m: &Member) -> Result<Answer, Refusal> {
        let a = self.sprite(id).matrix.position;
        let b = self.sprite(other).matrix.position;
        let (dx, dy) = (a[0] - b[0], a[2] - b[2]);
        let v = match m.name {
            "screen distance" => (dx * dx + dy * dy).sqrt(),
            "heading" if dx.abs() > 0.001 || dy.abs() > 0.001 => dx.atan2(dy) * 57.295_78,
            "heading" => 0.0,
            "X offset" => dx,
            "Y offset" => dy,
            "priority offset" => a[1] - b[1],
            _ => return Err(Refusal::Unmodelled),
        };
        Ok(Answer::Value(Value::Float(v)))
    }

    /// A paired `spritepositionmeasurement` write: `id`'s current matrix's
    /// place moved against `other`'s as each setter moves it (the crate
    /// docs). The sprite's next layout writes it again.
    pub fn set_paired(
        &mut self,
        id: SpriteId,
        other: SpriteId,
        m: &Member,
        v: Value,
    ) -> Result<(), Refusal> {
        let v = convert(&m.spec(), v)
            .and_then(|v| v.as_f32())
            .ok_or(Refusal::Nothing)?;
        let b = self.sprite(other).matrix.position;
        let a = &mut self.sprite_mut(id).matrix;
        let p = a.position;
        a.position = match m.name {
            // 0xAF618, 0xAF318, 0xAF820.
            "X offset" => [b[0] - v, p[1], p[2]],
            "Y offset" => [p[0], p[1], b[2] - v],
            "priority offset" => [p[0], b[1] - v, p[2]],
            // 0xAEED0: round the second at the distance in x and y, the
            // depth's difference kept.
            "heading" => {
                let (dx, dd, dy) = (p[0] - b[0], p[1] - b[1], p[2] - b[2]);
                let d = (dy * dy + dx * dx + f32::MIN_POSITIVE).sqrt();
                let a = (f64::from(v * 0.017_453_292)) as f32;
                let (sin, cos) = f64::from(a).sin_cos();
                [
                    b[0] + sin as f32 * d,
                    (f64::from(b[1]) + f64::from(dd)) as f32,
                    b[2] + cos as f32 * d,
                ]
            }
            // 0xAE9D8: the depth the second's, then along the line from it
            // (its length never 0: two that meet stay met).
            "screen distance" => {
                let diff = [p[0] - b[0], 0.0, p[2] - b[2]];
                let len = diff[2].mul_add(diff[2], diff[1].mul_add(diff[1], diff[0] * diff[0]))
                    + f32::MIN_POSITIVE;
                let by = v / len.sqrt();
                let diff = diff.map(|d| d * by);
                [b[0] + diff[0], b[1] + diff[1], b[2] + diff[2]]
            }
            _ => return Err(Refusal::Unmodelled),
        };
        Ok(())
    }

    /// A sprite member's write, converted and clamped as the VM does first
    /// (`guac_script::convert`), then as its setter does (the crate docs,
    /// Members). A value the member can't take is [`Refusal::Nothing`].
    pub fn set(&mut self, id: SpriteId, m: &Member, v: Value) -> Result<(), Refusal> {
        if m.set.is_none() {
            return Err(Refusal::ReadOnly);
        }
        let v = convert(&m.spec(), v).ok_or(Refusal::Nothing)?;
        let int = |v: Value| v.as_i32().ok_or(Refusal::Nothing);
        let float = |v: Value| v.as_f32().ok_or(Refusal::Nothing);
        match (m.namespace, m.name) {
            ("spriteinfo", "visible") => {
                let on = int(v)? != 0;
                let s = self.sprite_mut(id);
                if on != (s.properties & props::VISIBLE != 0) {
                    s.properties ^= props::VISIBLE;
                    s.mark_dirty();
                }
            }
            ("spriteinfo", "color") => {
                let Value::Color(c) = v else {
                    return Err(Refusal::Nothing);
                };
                self.sprite_mut(id).color = c.to_be_bytes();
            }
            ("spriteinfo", "replace color") => {
                let Value::Color(c) = v else {
                    return Err(Refusal::Nothing);
                };
                let [r, g, b, _] = c.to_be_bytes();
                let s = self.sprite_mut(id);
                s.replace[..3].copy_from_slice(&[r, g, b]);
            }
            ("spriteinfo", "location") => {
                let Value::Screen(w) = v else {
                    return Err(Refusal::Nothing);
                };
                self.sprite_mut(id).location = [(w >> 16) as u16 as i16, w as u16 as i16];
            }
            ("spriteinfo", "text") => {
                // A name extractor shows the object it holds.
                let v = if v == self.singleton(singleton::NAME_EXTRACTOR) {
                    self.name_object
                } else {
                    v
                };
                let s = self.sprite_mut(id);
                if s.text != v {
                    s.flags &= !flags::STRING;
                    s.text = v;
                    s.properties = (s.properties & !props::WIDTH_VALID) | props::DIRTY;
                    s.letters.clear();
                }
            }
            ("spriteinfo", "priority") => self.sprite_mut(id).priority = float(v)?,
            ("spriteinfo", "Z Depth") => self.sprite_mut(id).z_depth = float(v)?,
            ("spriteinfo", "width") | ("spriteinfo", "height") | ("spriteinfo", "depth") => {
                let axis = match m.name {
                    "width" => 0,
                    "height" => 1,
                    _ => 2,
                };
                let f = float(v)?;
                let s = self.sprite_mut(id);
                if s.size[axis] != f || !s.sized() {
                    s.size[axis] = f;
                    s.flags |= flags::SIZED;
                    s.mark_dirty();
                }
            }
            ("spriteinfo", "blend mode") => {
                let s = self.sprite_mut(id);
                s.properties = props::BLEND.set(s.properties, int(v)? as u32);
            }
            ("spriteinfo", "alignment") => {
                let s = self.sprite_mut(id);
                s.properties = props::ALIGNMENT.set(s.properties, int(v)? as u32);
            }
            ("spriteinfo", "align to parent") => {
                let s = self.sprite_mut(id);
                s.properties = props::ALIGN_TO_PARENT.set(s.properties, int(v)? as u32);
            }
            ("spriteinfo", "texture flip") => {
                let n = int(v)? as u32;
                let s = self.sprite_mut(id);
                if n != flags::FLIP.get(s.flags) {
                    s.flags = flags::FLIP.set(s.flags, n);
                    s.mark_dirty();
                }
            }
            // Clears *sized*, never clipping itself (0xB3D48).
            ("spriteinfo", "clipping") => {
                let n = int(v)? as u32;
                let s = self.sprite_mut(id);
                if n != (s.flags >> 25) & 1 {
                    s.flags = (s.flags & !flags::SIZED) | ((n & 1) << 25);
                    s.mark_dirty();
                }
            }
            ("spriteinfo", "Take a Picture") => {
                let on = int(v)? != 0;
                let s = self.sprite_mut(id);
                if on != (s.flags & flags::PICTURE != 0) {
                    s.flags ^= flags::PICTURE;
                    s.mark_dirty();
                }
            }
            ("spriteinfo", "numeric value") => {
                let mut f = float(v)?;
                if matches!(v, Value::Float(_) | Value::Double(_)) && !self.reads_float(id) {
                    f = truncate(f) as f32;
                }
                let s = self.sprite_mut(id);
                if s.numeric_value != f {
                    s.numeric_value = f;
                    if s.has_text() {
                        s.properties = (s.properties & !props::WIDTH_VALID) | props::DIRTY;
                        s.letters.clear();
                    }
                }
            }
            ("spriteinfo", "justify") => {
                let n = int(v)? as u32;
                let s = self.sprite_mut(id);
                if n != props::JUSTIFY.get(s.properties) {
                    s.properties = props::JUSTIFY.set(s.properties, n);
                    if s.has_text() {
                        s.mark_dirty();
                    }
                }
            }
            ("spriteinfo", "point size") => {
                let n = int(v)?;
                let s = self.sprite_mut(id);
                if props::POINT_SIZE.get(s.properties) as i32 != n {
                    s.properties = props::POINT_SIZE.set(s.properties, n as u32) | props::DIRTY;
                    if n > 0 {
                        s.flags |= flags::POINT_SIZE_SET;
                    } else {
                        s.flags &= !flags::POINT_SIZE_SET;
                    }
                }
            }
            ("spriteinfo", "replace color percentage") => {
                let p = int(v)?;
                self.sprite_mut(id).replace[3] = ((p * 255 + 50) / 100) as u8;
            }
            ("spriteinfo", "bloom intensity") => {
                // `convert` clamped it to 0-100, as the handler does.
                let b = int(v)? as u32;
                let s = self.sprite_mut(id);
                s.mark_dirty();
                s.placement = placement::BLOOM.set(s.placement, b);
            }
            ("spritematrixmeasurement", "attach node") => {
                let parent = self.sprite_of(v);
                self.attach(id, parent);
            }
            ("spritematrixmeasurement", "ID") => {
                let n = int(v)?;
                // The snapshot every write takes first (0x6A830).
                let s = self.sprite_mut(id);
                s.reset_id.get_or_insert(s.id);
                s.id = n;
            }
            _ => return Err(Refusal::Unmodelled),
        }
        Ok(())
    }

    /// `scale`'s part `axis` (0 x, 1 y, 2 z) written through the
    /// measurement `scale` reads.
    pub fn set_scale(&mut self, id: SpriteId, axis: usize, percent: f32) {
        self.sprite_mut(id).scale[axis] = percent;
    }

    /// A sprite member's `reset` (the op's: its snapshot written back, the
    /// crate docs). A member with no reset handler resets to nothing
    /// (**Hypothesis**: the op finds no handler and does nothing, AT.9's
    /// census); `ID` takes back the one it held before its first write; a
    /// set (0x7BB04) resets each of its sprites, last first, and writes
    /// nothing back.
    pub fn reset(&mut self, id: SpriteId, m: &Member, world: &dyn World) -> Result<(), Refusal> {
        match (m.namespace, m.name, m.reset) {
            (_, _, None) => Ok(()),
            ("spritematrixmeasurement", "ID", _) => {
                let s = self.sprite_mut(id);
                if let Some(n) = s.reset_id {
                    s.id = n;
                }
                Ok(())
            }
            (_, _, Some(SET_RESET)) => {
                let Answer::Elements(elements) = self.get(id, m, world)? else {
                    return Err(Refusal::Unmodelled);
                };
                for e in elements.into_iter().rev() {
                    if let Some(s) = self.sprite_of(e) {
                        self.restart(s);
                    }
                }
                Ok(())
            }
            _ => Err(Refusal::Unmodelled),
        }
    }

    /// A global converter's member's `reset`: `value set`'s and `color
    /// set`'s (0x80E0C) hand the set to its own reset and write it back,
    /// taken as empty (**Hypothesis**, the crate docs); every other global
    /// member has no reset handler and resets to nothing.
    pub fn reset_global(&mut self, m: &Member) -> Result<(), Refusal> {
        match (m.namespace, m.name, m.reset) {
            (_, _, None) => Ok(()),
            ("stringconverter", "value set", _) | ("textcolorconverter", "color set", _) => {
                self.set_global_elements(m, &[])
            }
            _ => Err(Refusal::Unmodelled),
        }
    }

    /// A global converter's member (`stringconverter`, `textcolorconverter`,
    /// `nameextractor`, `viewporttitlesafeinfo`, `stringinfo`).
    pub fn get_global(&self, m: &Member, world: &dyn World) -> Result<Answer, Refusal> {
        let c = &self.converter;
        let int = |i: i32| Ok(Answer::Value(Value::Int(i)));
        let set = |s: &Option<Vec<Value>>| Ok(Answer::Elements(s.clone().unwrap_or_default()));
        match (m.namespace, m.name) {
            ("stringconverter", "value set") => set(&c.value_set),
            ("stringconverter", "text") => Ok(Answer::Value(c.text)),
            ("stringconverter", "'@' representation") => int(i32::from(c.time)),
            ("stringconverter", "integer digits") => int(c.integer_digits),
            ("stringconverter", "decimal digits") => int(c.decimal_digits),
            ("stringconverter", "use comma") => int(i32::from(c.comma)),
            ("stringinfo", "String Converter") => {
                Ok(Answer::Value(self.singleton(singleton::STRING_CONVERTER)))
            }
            ("textcolorconverter", "text sprite") => Ok(Answer::Value(self.text_colors.sprite)),
            ("textcolorconverter", "color set") => set(&self.text_colors.colors),
            ("nameextractor", "Object to name") => Ok(Answer::Value(self.name_object)),
            ("nameextractor", "Instantiation size") | ("nameextractor", "type count") => {
                let n = (self.name_object != Value::Null)
                    .then(|| world.class_numbers(self.name_object))
                    .flatten()
                    .unwrap_or([0; 2]);
                int(n[usize::from(m.name == "type count")])
            }
            ("nameextractor", "Name checksum") => {
                let name = (self.name_object != Value::Null)
                    .then(|| world.name_of(self.name_object))
                    .flatten();
                int(name.map_or(0, |n| fnv1a(n.as_bytes()) as i32))
            }
            ("viewporttitlesafeinfo", "left margin") => int(self.title_safe[0]),
            ("viewporttitlesafeinfo", "right margin") => int(self.title_safe[1]),
            ("viewporttitlesafeinfo", "upper margin") => int(self.title_safe[2]),
            ("viewporttitlesafeinfo", "lower margin") => int(self.title_safe[3]),
            _ => Err(Refusal::Unmodelled),
        }
    }

    /// Writes a global converter's member, converted and clamped as the VM
    /// does first. `world` names an object a text shows.
    pub fn set_global(&mut self, m: &Member, v: Value, world: &dyn World) -> Result<(), Refusal> {
        if m.set.is_none() {
            return Err(Refusal::ReadOnly);
        }
        let v = convert(&m.spec(), v).ok_or(Refusal::Nothing)?;
        let int = |v: Value| v.as_i32().ok_or(Refusal::Nothing);
        match (m.namespace, m.name) {
            ("stringconverter", "text") => {
                self.converter.text = v;
                self.converter.value_set = self
                    .string(v)
                    .map(|s| s.units.iter().map(|&u| Value::Int(i32::from(u))).collect());
            }
            ("stringconverter", "'@' representation") => self.converter.time = int(v)? != 0,
            ("stringconverter", "integer digits") => self.converter.integer_digits = int(v)?,
            ("stringconverter", "decimal digits") => self.converter.decimal_digits = int(v)?,
            ("stringconverter", "use comma") => self.converter.comma = int(v)? != 0,
            ("textcolorconverter", "text sprite") => {
                // Its letters' colours read back, one per glyph drawn
                // (0xBAE20).
                self.text_colors.sprite = v;
                self.text_colors.colors = self.sprite_of(v).map(|id| {
                    let s = self.sprite(id);
                    let n = self.letter_count(id, world);
                    (0..n)
                        .map(|i| color(s.letters.get(i).copied().unwrap_or(LETTER)))
                        .collect()
                });
            }
            ("nameextractor", "Object to name") => self.name_object = v,
            _ => return Err(Refusal::Unmodelled),
        }
        Ok(())
    }

    /// A global set member's write (`stringconverter.value set`,
    /// `textcolorconverter.color set`), with the set's elements as the VM
    /// holds them.
    pub fn set_global_elements(&mut self, m: &Member, elements: &[Value]) -> Result<(), Refusal> {
        match (m.namespace, m.name) {
            ("stringconverter", "value set") => {
                let c = &mut self.converter;
                c.value_set = Some(elements.to_vec());
                c.text = Value::Null;
                if elements.is_empty() {
                    return Ok(());
                }
                let mut s = Str {
                    units: Vec::with_capacity(elements.len()),
                    flags: 0,
                    integer_digits: c.integer_digits,
                    decimal_digits: c.decimal_digits,
                };
                if c.time {
                    s.flags |= string_flags::TIME;
                }
                if c.comma {
                    s.flags |= string_flags::COMMAS;
                }
                for e in elements {
                    let n = match *e {
                        Value::Float(f) => truncate(f + 0.5),
                        other => other.as_i32().unwrap_or(0),
                    };
                    if n == 64 {
                        s.flags |= string_flags::HAS_AT;
                    }
                    s.units.push(n as u16);
                }
                let text = self.add_string(s);
                self.converter.text = text;
            }
            ("textcolorconverter", "color set") => {
                // The kept sprite's letters coloured in order (0xBB5D8).
                self.text_colors.colors = Some(elements.to_vec());
                if let Some(id) = self.sprite_of(self.text_colors.sprite) {
                    let colors = elements.iter().map(|e| match *e {
                        Value::Color(c) => c.to_be_bytes(),
                        _ => LETTER,
                    });
                    let s = self.sprite_mut(id);
                    for (i, c) in colors.enumerate() {
                        if i < s.letters.len() {
                            s.letters[i] = c;
                        } else {
                            s.letters.push(c);
                        }
                    }
                }
            }
            _ => return Err(Refusal::Unmodelled),
        }
        Ok(())
    }
}

/// The engine sets' shared reset handler (the crate docs, A set's reset).
const SET_RESET: u32 = 0x7BB04;

/// A letter's colour until the text colour converter sets it: the font's
/// colour at run time (0.5, 0.5, 0.5, 1).
pub const LETTER: [u8; 4] = [0x80, 0x80, 0x80, 0xFF];

impl Model {
    /// How many glyphs a text sprite draws (its letters for the text colour
    /// converter); 0 for anything else.
    pub fn letter_count(&self, id: SpriteId, world: &dyn World) -> usize {
        self.glyph_layout(id, world).map_or(0, |(g, _)| g.len())
    }

    /// A text sprite's glyphs, from its top-left in font pixels, and its
    /// layout (the crate docs, Text); `None` for one that draws no text.
    pub fn glyph_layout(
        &self,
        id: SpriteId,
        world: &dyn World,
    ) -> Option<(Vec<text::GlyphQuad>, text::TextOut)> {
        let s = self.sprite(id);
        if s.kind() != kind::TEXT {
            return None;
        }
        let m = self.metrics(id)?;
        let units = self.text_units(id, world)?;
        if units.is_empty() {
            return None;
        }
        let out = text::layout(&m, &self.text_in(id), &units);
        let (width, justify) = text::break_width(
            s.sized(),
            out.size[0],
            out.scale[0],
            props::JUSTIFY.get(s.properties),
        );
        // The font breaks the string itself at that width (0x3D5698).
        Some((text::glyphs(&m, width, justify, &units), out))
    }
}

/// 32-bit FNV-1a.
pub fn fnv1a(bytes: &[u8]) -> u32 {
    bytes.iter().fold(0x811C_9DC5u32, |h, &b| {
        (h ^ u32::from(b)).wrapping_mul(0x0100_0193)
    })
}

#[cfg(test)]
mod tests;
