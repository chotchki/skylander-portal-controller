//! The interface's keeper (openspec AT.9, design decisions 1 and 10): what a
//! host forwards the interface's members to, over the [`Model`] it holds
//! (the crate docs, The keeper). guacamole's script host and a headless
//! test's host both forward through it, so neither knows a sprite.

use std::collections::{BTreeMap, HashMap, HashSet};

use guac_script::{Access, HostFault, MemberId, Value};

use crate::members::{MEMBERS, Member};
use crate::model::{Answer, Bases, Model, Refusal, World};
use crate::sprite::{SpriteId, State};

/// Where the keeper's objects sit among a host's entity ids: far past any a
/// headless run or Bevy hands out (an entity's generation in the high word),
/// one block each.
const BLOCK: u64 = 1 << 44;
const BASE: u64 = 0xFFE0 << 48;
/// A sprite's `scale`, its measurement object.
const SCALES: u64 = BASE + 3 * BLOCK;
/// A sprite's initial matrix (`spritematrixmeasurement.origin`).
const ORIGINS: u64 = BASE + 4 * BLOCK;
/// An initial matrix's turns (`matrixmeasurement.orientation` on one).
const ORIGIN_TURNS: u64 = BASE + 5 * BLOCK;
const END: u64 = BASE + 6 * BLOCK;

/// The model's own ranges, past every other's.
pub const BASES: Bases = Bases {
    sprites: BASE,
    strings: BASE + BLOCK,
    singletons: BASE + 2 * BLOCK,
};

/// The interface's eight namespaces (`guac-script`'s `INTERFACE`).
const NAMESPACES: [&str; 8] = [
    "spriteinfo",
    "spritematrixmeasurement",
    "spritepositionmeasurement",
    "stringconverter",
    "stringinfo",
    "textcolorconverter",
    "nameextractor",
    "viewporttitlesafeinfo",
];

/// What the keeper answers on its own objects beside the 64: a sprite's
/// scale's parts, its turns, an initial matrix's turns.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Own {
    ScaleAxis(usize),
    ScaleUniform,
    /// 0 tilt, 1 facing, 2 roll: the matrix's turns in its order.
    Turn(usize),
    RelativeTurn(usize),
    Orientation,
    /// `viewportinfo.title safe zone` on the gameplay viewport: the
    /// title-safe info whose margins `viewporttitlesafeinfo` reads.
    TitleSafe,
}

fn own_members() -> HashMap<MemberId, Own> {
    let mut t = HashMap::new();
    for (name, axis) in [("x", 0), ("y", 1), ("z", 2)] {
        t.insert(
            MemberId::named("scalemeasurement", name),
            Own::ScaleAxis(axis),
        );
    }
    t.insert(
        MemberId::named("scalemeasurement", "uniform"),
        Own::ScaleUniform,
    );
    for (name, axis) in [("tilt", 0), ("facing", 1), ("roll", 2)] {
        t.insert(
            MemberId::named("orientationmeasurement", name),
            Own::Turn(axis),
        );
        t.insert(
            MemberId::named("orientationmeasurement", &format!("relative {name}")),
            Own::RelativeTurn(axis),
        );
    }
    t.insert(
        MemberId::named("matrixmeasurement", "orientation"),
        Own::Orientation,
    );
    t.insert(
        MemberId::named("viewportinfo", "title safe zone"),
        Own::TitleSafe,
    );
    t
}

/// One of the keeper's values.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Held {
    Sprite(SpriteId),
    String,
    Singleton,
    Scale(SpriteId),
    Origin(SpriteId),
    OriginTurns(SpriteId),
}

/// The model a host holds and the member access it forwards.
#[derive(Debug, Clone)]
pub struct Keeper {
    model: Model,
    members: HashMap<MemberId, &'static Member>,
    own: HashMap<MemberId, Own>,
    /// A sprite member asked of what holds no sprite (a stray), by member,
    /// each answered as a path through nothing.
    strays: BTreeMap<String, u64>,
    /// A member the keeper doesn't answer asked of one of its objects, by
    /// id, each answered as a path through nothing.
    unknown: BTreeMap<MemberId, u64>,
    /// The two set references whose set the model changed since the VM
    /// last wrote or read it (`value set` by a `text` write, `color set` by
    /// a `text sprite` write or either's reset).
    fresh: HashSet<MemberId>,
    /// The gameplay viewport's container (`viewportinfo.container sprite`).
    container: SpriteId,
}

impl Default for Keeper {
    fn default() -> Self {
        Keeper::new(Model::new(BASES))
    }
}

fn fault(r: Refusal) -> HostFault {
    match r {
        Refusal::ReadOnly | Refusal::Nothing => HostFault::Nothing,
        Refusal::Unmodelled => HostFault::Unmodelled,
    }
}

/// The gameplay viewport's title-safe margins in frame pixels, left, right,
/// upper and lower (**Hypothesis**, AT.10's to read live): 5 % of the
/// 1280 by 720 frame a side, the margins that make `UI - Init UI
/// Containers` (`Init_Setup.ai`) size `_UI_Container` 768 by 432 units,
/// standing at frame (64, 36), as the June savestate holds it and AC.13
/// draws it (AT.3).
pub const TITLE_SAFE: [i32; 4] = [64, 64, 36, 36];

/// The gameplay viewport's container as the June savestate holds it:
/// `RT Viewport 0`, a container the frame's size at 100 %, standing at the
/// frame's top-left, visible, blend 1 (the crate docs, The keeper).
fn viewport_container(frame: [f32; 2]) -> guac_ir::interface::Template {
    guac_ir::interface::Template {
        name: "RT Viewport 0".into(),
        object: u32::MAX,
        properties: crate::sprite::props::VISIBLE | (1 << 3) | crate::sprite::kind::CONTAINER,
        flags: 0,
        position: [0, 0],
        z_depth: 0.0,
        priority: 0.0,
        width: frame[0],
        height: frame[1],
        scale: [100.0; 3],
        color: crate::model::LETTER,
        replace_color: [0x80, 0x80, 0x80, 0],
        num_value: 0,
        depth: 0.0,
        init_matrix: Default::default(),
        matrix: Default::default(),
        content: guac_ir::interface::Content::Container,
        string: None,
    }
}

impl Keeper {
    /// A keeper over `model`, whose bases should be [`BASES`], with the
    /// gameplay viewport's container added to it, awake.
    pub fn new(mut model: Model) -> Self {
        let container = model.add_template(None, &viewport_container(model.frame), Value::Null);
        model.wake(container);
        model.title_safe = TITLE_SAFE;
        let members = MEMBERS
            .iter()
            .map(|m| {
                let name = m.name.trim_start_matches('-');
                (MemberId::named(m.namespace, name), m)
            })
            .collect();
        Keeper {
            model,
            members,
            own: own_members(),
            strays: BTreeMap::new(),
            unknown: BTreeMap::new(),
            fresh: HashSet::new(),
            container,
        }
    }

    /// The gameplay viewport's container, which `viewportinfo.container
    /// sprite` names on a run holding the model: the sprite the scripts
    /// hang `_Screen_Container` on and size the screen by.
    pub fn container(&self) -> Value {
        self.model.value_of(self.container)
    }

    /// The set reference whose set a write of `m` changes in the model.
    fn changes(m: &Member) -> Option<MemberId> {
        match (m.namespace, m.name) {
            ("stringconverter", "text" | "value set") => {
                Some(MemberId::named("stringconverter", "value set"))
            }
            ("textcolorconverter", "text sprite" | "color set") => {
                Some(MemberId::named("textcolorconverter", "color set"))
            }
            _ => None,
        }
    }

    pub fn model(&self) -> &Model {
        &self.model
    }

    pub fn model_mut(&mut self) -> &mut Model {
        &mut self.model
    }

    /// Whether `id` is one of the interface's 64.
    pub fn is_member(&self, id: MemberId) -> bool {
        self.members.contains_key(&id)
    }

    /// Whether `id` is in one of the interface's namespaces, registered or
    /// not: a host leaves such a member to the keeper.
    pub fn in_namespace(id: MemberId) -> bool {
        NAMESPACES
            .iter()
            .any(|ns| MemberId::named(ns, "").namespace == id.namespace)
    }

    /// Whether `v` is one of the keeper's: a sprite, a template's object, a
    /// string, a converter or a measurement it made.
    pub fn holds(&self, v: Value) -> bool {
        self.held(v).is_some()
    }

    fn held(&self, v: Value) -> Option<Held> {
        if let Some(id) = self.model.sprite_of(v) {
            return Some(Held::Sprite(id));
        }
        if self.model.string(v).is_some() {
            return Some(Held::String);
        }
        let Value::Entity(e) = v else { return None };
        if !(BASE..END).contains(&e) {
            return None;
        }
        let n = |base: u64| SpriteId((e - base) as u32);
        let known = |id: SpriteId| (id.0 as usize) < self.model.len();
        Some(match e {
            e if e >= ORIGIN_TURNS => Held::OriginTurns(n(ORIGIN_TURNS)),
            e if e >= ORIGINS => Held::Origin(n(ORIGINS)),
            e if e >= SCALES => Held::Scale(n(SCALES)),
            e if e >= BASES.singletons => Held::Singleton,
            _ => return None,
        })
        .filter(|h| match *h {
            Held::Scale(id) | Held::Origin(id) | Held::OriginTurns(id) => known(id),
            _ => true,
        })
    }

    /// The sprite asks were made of when they hold none, by member, and how
    /// often: each answered as a path through nothing.
    pub fn strays(&self) -> &BTreeMap<String, u64> {
        &self.strays
    }

    /// The members asked of the keeper's objects that it doesn't answer, by
    /// id, and how often: each answered as a path through nothing.
    pub fn unknown(&self) -> &BTreeMap<MemberId, u64> {
        &self.unknown
    }

    fn stray(&mut self, m: &Member) -> HostFault {
        *self.strays.entry(m.path()).or_default() += 1;
        HostFault::Nothing
    }

    fn not_known(&mut self, id: MemberId) -> HostFault {
        *self.unknown.entry(id).or_default() += 1;
        HostFault::Nothing
    }

    /// The sprite a sprite member is read on: its receiver's, or none for
    /// the class's own (`active`, `removed`, the text colour converter),
    /// which any receiver reaches.
    fn sprite_for(&mut self, m: &Member, at: &Access) -> Result<Option<SpriteId>, HostFault> {
        if matches!(m.name, "active" | "removed" | "Text Color Converter") {
            return Ok(None);
        }
        match at.receiver.and_then(|r| self.model.sprite_of(r)) {
            Some(id) => Ok(Some(id)),
            None => Err(self.stray(m)),
        }
    }

    fn value_of(&self, id: SpriteId, a: Answer) -> Result<Value, HostFault> {
        match a {
            Answer::Value(v) => Ok(v),
            Answer::Scale(_) => Ok(Value::Entity(SCALES + u64::from(id.0))),
            Answer::Matrix(_) => Ok(Value::Entity(ORIGINS + u64::from(id.0))),
            // A set read as one value: the VM reads a set through
            // `elements`.
            Answer::Elements(_) => Err(HostFault::Nothing),
        }
    }

    /// A member read, as `guac_script::Host::get` asks it.
    pub fn get(&mut self, at: &Access, world: &dyn World) -> Result<Value, HostFault> {
        let Some(&m) = self.members.get(&at.member) else {
            return self.get_own(at);
        };
        match m.namespace {
            "spriteinfo" | "spritematrixmeasurement" => {
                let id = self.sprite_for(m, at)?;
                // The class's own: any sprite answers as the first does.
                let on = id.unwrap_or(SpriteId(0));
                if id.is_none() && m.name != "Text Color Converter" {
                    return Err(HostFault::Nothing);
                }
                let a = self.model.get(on, m, world).map_err(fault)?;
                self.value_of(on, a)
            }
            "spritepositionmeasurement" => {
                let (first, second) = self.pair(m, at)?;
                match self.model.get_paired(first, second, m).map_err(fault)? {
                    Answer::Value(v) => Ok(v),
                    _ => Err(HostFault::Nothing),
                }
            }
            _ => match self.model.get_global(m, world).map_err(fault)? {
                Answer::Value(v) => Ok(v),
                _ => Err(HostFault::Nothing),
            },
        }
    }

    /// The two sprites a paired measurement compares: the receiver and the
    /// value stack's other point.
    fn pair(&mut self, m: &Member, at: &Access) -> Result<(SpriteId, SpriteId), HostFault> {
        let first = at.receiver.and_then(|r| self.model.sprite_of(r));
        let second = at.other_point.and_then(|r| self.model.sprite_of(r));
        match (first, second) {
            (Some(a), Some(b)) => Ok((a, b)),
            _ => Err(self.stray(m)),
        }
    }

    /// A set member read whole, as `guac_script::Host::elements` asks it.
    pub fn elements(&mut self, at: &Access, world: &dyn World) -> Result<Vec<Value>, HostFault> {
        let Some(&m) = self.members.get(&at.member) else {
            return Err(self.not_known(at.member));
        };
        let a = match m.namespace {
            "spriteinfo" | "spritematrixmeasurement" => {
                let on = self.sprite_for(m, at)?.unwrap_or(SpriteId(0));
                if self.model.is_empty() {
                    return Ok(Vec::new());
                }
                self.model.get(on, m, world).map_err(fault)?
            }
            _ => self.model.get_global(m, world).map_err(fault)?,
        };
        match a {
            Answer::Elements(e) => Ok(e),
            _ => Err(HostFault::Nothing),
        }
    }

    /// A member written, as `guac_script::Host::set` asks it: a write to a
    /// member with no setter goes nowhere, as the original's.
    pub fn set(&mut self, at: &Access, v: Value, world: &dyn World) -> Result<(), HostFault> {
        let Some(&m) = self.members.get(&at.member) else {
            return self.set_own(at, v);
        };
        let done = match m.namespace {
            "spriteinfo" | "spritematrixmeasurement" => {
                let Some(id) = self.sprite_for(m, at)? else {
                    return Ok(());
                };
                self.model.set(id, m, v)
            }
            "spritepositionmeasurement" => {
                let (first, second) = self.pair(m, at)?;
                self.model.set_paired(first, second, m, v)
            }
            _ => {
                let done = self.model.set_global(m, v, world);
                if done.is_ok()
                    && let Some(set) = Self::changes(m)
                {
                    self.fresh.insert(set);
                }
                done
            }
        };
        match done {
            Ok(()) | Err(Refusal::ReadOnly) => Ok(()),
            Err(r) => Err(fault(r)),
        }
    }

    /// A set reference written, as `guac_script::Host::set_elements` hands
    /// it: the string converter's `value set` and the text colour
    /// converter's `color set`, which the VM keeps and the model copies.
    pub fn set_elements(&mut self, at: &Access, elements: &[Value]) -> Result<(), HostFault> {
        let Some(&m) = self.members.get(&at.member) else {
            return Err(self.not_known(at.member));
        };
        self.fresh.remove(&at.member);
        match self.model.set_global_elements(m, elements) {
            Ok(()) => Ok(()),
            Err(_) => Err(HostFault::Nothing),
        }
    }

    /// A set reference about to be read, as `guac_script::Host::set_ref`
    /// asks it: the set the model changed it to since the VM last wrote or
    /// read it, once.
    pub fn set_ref(&mut self, at: &Access, world: &dyn World) -> Option<Vec<Value>> {
        let &m = self.members.get(&at.member)?;
        if !self.fresh.remove(&at.member) {
            return None;
        }
        match self.model.get_global(m, world) {
            Ok(Answer::Elements(e)) => Some(e),
            _ => None,
        }
    }

    /// A member's `reset`, as `guac_script::Host::reset` asks it.
    pub fn reset(&mut self, at: &Access, world: &dyn World) -> Result<(), HostFault> {
        let Some(&m) = self.members.get(&at.member) else {
            return Ok(());
        };
        let done = match m.namespace {
            "spriteinfo" | "spritematrixmeasurement" => {
                let Some(id) = self.sprite_for(m, at)? else {
                    return Ok(());
                };
                self.model.reset(id, m, world)
            }
            "spritepositionmeasurement" => Ok(()),
            _ => {
                let done = self.model.reset_global(m);
                if done.is_ok()
                    && m.reset.is_some()
                    && let Some(set) = Self::changes(m)
                {
                    self.fresh.insert(set);
                }
                done
            }
        };
        done.map_err(fault)
    }

    /// A member on one of the keeper's own measurement objects.
    fn get_own(&mut self, at: &Access) -> Result<Value, HostFault> {
        let held = at.receiver.and_then(|r| self.held(r));
        let float = |f: f32| Ok(Value::Float(f));
        match (self.own.get(&at.member).copied(), held) {
            (Some(Own::ScaleAxis(axis)), Some(Held::Scale(id))) => {
                float(self.model.sprite(id).scale[axis])
            }
            // As an actor's: it answers only when the three are one.
            (Some(Own::ScaleUniform), Some(Held::Scale(id))) => {
                let s = self.model.sprite(id).scale;
                if s[0] == s[1] && s[1] == s[2] {
                    float(s[0])
                } else {
                    Err(HostFault::Nothing)
                }
            }
            (Some(Own::Turn(axis)), Some(Held::Sprite(id))) => {
                float(self.model.sprite(id).matrix.rotation[axis])
            }
            (Some(Own::Turn(axis)), Some(Held::OriginTurns(id))) => {
                float(self.model.sprite(id).init_matrix.rotation[axis])
            }
            (Some(Own::RelativeTurn(_)), Some(Held::Sprite(_))) => float(0.0),
            (Some(Own::Orientation), Some(Held::Origin(id))) => {
                Ok(Value::Entity(ORIGIN_TURNS + u64::from(id.0)))
            }
            (Some(Own::Orientation), Some(Held::Sprite(id))) => Ok(self.model.value_of(id)),
            // The host routes it here from the viewport, which isn't ours.
            (Some(Own::TitleSafe), _) => {
                Ok(self.model.singleton(crate::model::singleton::TITLE_SAFE))
            }
            _ => Err(self.not_known(at.member)),
        }
    }

    fn set_own(&mut self, at: &Access, v: Value) -> Result<(), HostFault> {
        let held = at.receiver.and_then(|r| self.held(r));
        let own = self.own.get(&at.member).copied();
        let Some(f) = v.as_f32() else {
            return Err(HostFault::Nothing);
        };
        match (own, held) {
            (Some(Own::ScaleAxis(axis)), Some(Held::Scale(id))) => {
                self.model.set_scale(id, axis, f);
            }
            (Some(Own::ScaleUniform), Some(Held::Scale(id))) => {
                for axis in 0..3 {
                    self.model.set_scale(id, axis, f);
                }
            }
            (Some(Own::Turn(axis)), Some(Held::Sprite(id))) => {
                self.model.sprite_mut(id).matrix.rotation[axis] = f;
            }
            (Some(Own::RelativeTurn(axis)), Some(Held::Sprite(id))) => {
                self.model.sprite_mut(id).matrix.rotation[axis] += f;
            }
            // An initial matrix is read-only.
            (Some(Own::Turn(_) | Own::RelativeTurn(_)), Some(Held::OriginTurns(_))) => {}
            _ => return Err(self.not_known(at.member)),
        }
        Ok(())
    }

    /// A clone of `template` (a template's object or a sprite) if it's the
    /// keeper's: awake, last in `active`, its activator `spawner`; its
    /// initial matrix its source's current one with the spawn's place,
    /// where `at` names one of the keeper's (a sprite's current place, an
    /// initial matrix's), and the spawn's facing as its facing turn
    /// (**Hypothesis**: facing is the turn about depth, as an actor's is
    /// about its up axis); its current matrix that, as waking copies it.
    /// `None` when the template isn't a sprite.
    pub fn spawn(
        &mut self,
        template: Value,
        at: Option<Value>,
        facing: Option<f32>,
        spawner: Value,
    ) -> Option<Value> {
        let source = self.model.sprite_of(template)?;
        let place = match at.and_then(|v| self.held(v)) {
            Some(Held::Sprite(s)) => Some(self.model.sprite(s).matrix.position),
            Some(Held::Origin(s)) => Some(self.model.sprite(s).init_matrix.position),
            _ => None,
        };
        let id = self.model.spawn(source);
        let s = self.model.sprite_mut(id);
        s.activator = spawner;
        if let Some(p) = place {
            s.init_matrix.position = p;
        }
        if let Some(f) = facing {
            s.init_matrix.rotation[1] = f;
        }
        s.matrix = s.init_matrix;
        Some(self.model.value_of(id))
    }

    /// `remove` on `v`: true when it's one of the keeper's sprites.
    pub fn remove(&mut self, v: Value) -> bool {
        match self.model.sprite_of(v) {
            Some(id) => {
                self.model.remove(id);
                true
            }
            None => false,
        }
    }

    /// Whether `v` is one of the keeper's sprites and taken off: false for
    /// anything else, so a host's tell on it runs.
    pub fn removed(&self, v: Value) -> bool {
        self.model
            .sprite_of(v)
            .is_some_and(|id| self.model.sprite(id).state == State::Removed)
    }

    /// Every awake clone, in `active`'s order.
    pub fn instances(&self) -> Vec<SpriteId> {
        self.model
            .active()
            .iter()
            .copied()
            .filter(|&id| self.model.sprite(id).cloned_from != crate::sprite::Source::None)
            .collect()
    }
}

#[cfg(test)]
mod tests;
