//! The VM's seam to the engine (design decision 21): members by id, their
//! registered types, the value cells, and the [`Host`] trait guacamole
//! implements over its ECS queries and the tests over plain structs.

use std::collections::HashMap;

use guac_formats::fnv1a32;
use guac_formats::script::Class;

use crate::program::{Program, ScriptId};
use crate::value::Value;
use crate::vm::Handle;

/// An engine member as a script imports it: FNV-1a 32 of the lowercased
/// class namespace (`actorinfo`, the class name less any `tfb`) and of the
/// lowercased member name.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct MemberId {
    pub namespace: u32,
    pub name: u32,
}

impl MemberId {
    pub fn named(namespace: &str, name: &str) -> Self {
        Self {
            namespace: fnv1a32(namespace.to_ascii_lowercase().as_bytes()),
            name: fnv1a32(name.to_ascii_lowercase().as_bytes()),
        }
    }
}

/// How a write into a member converts ([`crate::convert`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// An integer or enum measurement.
    Int,
    Float,
    Color,
    Screen,
    /// An object.
    Reference,
    /// A set the engine keeps (an actor's waypoints): read whole through
    /// [`Host::elements`], never written.
    Set,
    /// Takes whatever it's given (the VM's own cells).
    Any,
}

/// A member's registered type, bounds and reset value.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MemberSpec {
    pub kind: Kind,
    /// Writes clamp to [min, max] only when either is nonzero.
    pub min: f32,
    pub max: f32,
    /// What a value cell reads before anything is written.
    pub default: Value,
}

impl MemberSpec {
    pub fn new(kind: Kind, min: f32, max: f32) -> Self {
        let default = match kind {
            Kind::Int => Value::Int(0),
            Kind::Float => Value::Float(0.0),
            Kind::Color => Value::Color(0),
            Kind::Screen => Value::Screen(0),
            Kind::Reference | Kind::Set | Kind::Any => Value::Null,
        };
        Self {
            kind,
            min,
            max,
            default,
        }
    }

    pub fn with_default(mut self, default: Value) -> Self {
        self.default = default;
        self
    }
}

/// Whether the VM or the host holds a modelled member's state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Place {
    /// Its get, set and reset go to the [`Host`].
    Host,
    /// The VM keeps it per object: it reads the default and keeps what's
    /// written (the cosmetic members each member box lists).
    ValueCell,
}

/// The modelled members: every one the VM may reach that isn't its own
/// ([`Builtin`]). Reaching one that isn't here is unmodelled.
#[derive(Debug, Clone, Default)]
pub struct Members {
    entries: HashMap<MemberId, (MemberSpec, Place)>,
    names: HashMap<MemberId, String>,
}

impl Members {
    pub fn new() -> Self {
        Self::default()
    }

    /// A member whose get, set and reset go to the [`Host`].
    pub fn host(&mut self, namespace: &str, name: &str, spec: MemberSpec) -> &mut Self {
        self.add(namespace, name, spec, Place::Host)
    }

    /// A member the VM keeps per object: it reads `spec`'s default until
    /// written.
    pub fn value_cell(&mut self, namespace: &str, name: &str, spec: MemberSpec) -> &mut Self {
        self.add(namespace, name, spec, Place::ValueCell)
    }

    /// A value cell known only by its hashes (a member the registry names
    /// that no string on the disc spells).
    pub fn value_cell_id(&mut self, id: MemberId, spec: MemberSpec) -> &mut Self {
        self.entries.insert(id, (spec, Place::ValueCell));
        self
    }

    /// A member's name for logs and halts, modelled or not.
    pub fn known(&mut self, namespace: &str, name: &str) -> &mut Self {
        let id = MemberId::named(namespace, name);
        self.names.insert(
            id,
            format!(
                "{}.{}",
                namespace.to_ascii_lowercase(),
                name.to_ascii_lowercase()
            ),
        );
        self
    }

    fn add(&mut self, namespace: &str, name: &str, spec: MemberSpec, place: Place) -> &mut Self {
        let id = MemberId::named(namespace, name);
        self.entries.insert(id, (spec, place));
        self.names.insert(
            id,
            format!(
                "{}.{}",
                namespace.to_ascii_lowercase(),
                name.to_ascii_lowercase()
            ),
        );
        self
    }

    pub fn get(&self, id: MemberId) -> Option<(&MemberSpec, Place)> {
        self.entries.get(&id).map(|(s, p)| (s, *p))
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// `namespace.name` for a member added by name or one of the VM's own,
    /// else its two hashes.
    pub fn name(&self, id: MemberId) -> String {
        if let Some(n) = self.names.get(&id) {
            return n.clone();
        }
        if let Some(b) = Builtin::of(id) {
            return b.name().to_owned();
        }
        format!("{:08x}.{:08x}", id.namespace, id.name)
    }
}

/// The members that are the VM's own (crate docs, Members).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Builtin {
    My,
    Null,
    Undefined,
    Its,
    ValueDereferencer,
    Count,
    First,
    Last,
    Random,
    CastToInteger,
    /// `system.script timer`: the running placement's seconds of logic
    /// ticks so far (its `_activeTime`).
    ScriptTimer,
    /// `system.frame rate scale`: the tick's seconds times 60, 1 at the
    /// game's full rate.
    FrameRateScale,
    /// `optoplevelbehavior.behavior`: the running placement's behaviour, as
    /// the object a `set behavior` names.
    Behaviour,
    /// `setvariant.next`: the set's elements in turn, from the first,
    /// round again after the last.
    Next,
}

impl Builtin {
    const ALL: [Builtin; 14] = [
        Builtin::My,
        Builtin::Null,
        Builtin::Undefined,
        Builtin::Its,
        Builtin::ValueDereferencer,
        Builtin::Count,
        Builtin::First,
        Builtin::Last,
        Builtin::Random,
        Builtin::CastToInteger,
        Builtin::ScriptTimer,
        Builtin::FrameRateScale,
        Builtin::Behaviour,
        Builtin::Next,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Builtin::My => "optoplevelbehavior.my",
            Builtin::Null => "abstractscriptgroup.null",
            Builtin::Undefined => "abstractscriptgroup.undefined",
            Builtin::Its => "abstractscriptgroup.its",
            Builtin::ValueDereferencer => "abstractscriptgroup.valuedereferencer",
            Builtin::Count => "setvariant.count",
            Builtin::First => "setvariant.first",
            Builtin::Last => "setvariant.last",
            Builtin::Random => "setvariant.random",
            Builtin::CastToInteger => "system.cast to integer",
            Builtin::ScriptTimer => "system.script timer",
            Builtin::FrameRateScale => "system.frame rate scale",
            Builtin::Behaviour => "optoplevelbehavior.behavior",
            Builtin::Next => "setvariant.next",
        }
    }

    pub fn id(self) -> MemberId {
        let (namespace, name) = self.name().split_once('.').unwrap_or_default();
        MemberId::named(namespace, name)
    }

    pub fn of(id: MemberId) -> Option<Self> {
        Self::ALL.into_iter().find(|b| b.id() == id)
    }
}

/// What the host needs to answer a member: whose script asks, on what,
/// and from where a distance is measured.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Access {
    /// The running placement's handle (`my`).
    pub placement: u64,
    /// What the member is read on; `None` when it heads the path.
    pub receiver: Option<Value>,
    pub member: MemberId,
    /// A value stack's `_otherPoint`, read: what a distance or a heading on
    /// the path is measured from.
    pub other_point: Option<Value>,
    /// The reference cell the receiver was read through, for a reference's
    /// own members: `placementreference.remembered` is a point each
    /// reference keeps, two references to one actor keeping two (openspec
    /// AG.28: the camera's look-from and look-at points name one follower
    /// clone and sit 55 ft apart). `None` otherwise.
    pub via: Option<u32>,
    /// The op whose read or write this is (its id in the program), so a
    /// host can answer one asker apart from another (openspec AH.150: the
    /// on-screen asks it answers from the gameplay camera). `None` outside
    /// an op's run.
    pub op: Option<crate::program::OpId>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HostFault {
    /// The path goes through nothing (no such object, a getter that fails):
    /// a write drops and a check skips, quietly, as in the original.
    Nothing,
    /// The host doesn't model it: strict mode stops, play mode stops the
    /// placement.
    Unmodelled,
}

/// An op the VM leaves to the host (crate docs, Left to the host), with its
/// operands read.
#[derive(Debug, Clone, PartialEq)]
pub struct OpCall {
    pub placement: u64,
    /// What the op acts on when no operand says: the placement, or the target
    /// of an enclosing `spawn` or `tell`.
    pub receiver: Value,
    pub class: Class,
    /// Each operand field by registry name, read (`None` for a null field).
    pub operands: Vec<(&'static str, Option<Value>)>,
    /// Each enum field by registry name (`_combineMode`, `_dir`, `_filter`).
    pub words: Vec<(&'static str, u32)>,
    /// Each operand that reads as a set, its members in order (a path of
    /// waypoints, the targets a field of view looks for).
    pub sets: Vec<(&'static str, Vec<Value>)>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Answer {
    /// The op ran; a branch op's block is skipped.
    Done,
    /// The op's block runs (a branch op only), with `receiver` as the
    /// receiver if given; `result` is what the op reads as on a path (the
    /// spawned clone, the sequence started, the targets found).
    Run {
        receiver: Option<Value>,
        result: Option<Value>,
    },
    /// The op found no target: a branch op skips its block, any other does
    /// nothing.
    Skip,
    /// A branch op's targets (a field-of-view check's, nearest first): with
    /// any, the block runs and the op reads as a set of them on a path; with
    /// none, it skips.
    Found(Vec<Value>),
}

/// The engine, as the VM reaches it.
pub trait Host {
    /// A member's value, or why there's none.
    fn get(&mut self, at: &Access) -> Result<Value, HostFault>;

    /// A write, already converted and clamped to the member's type.
    fn set(&mut self, at: &Access, value: Value) -> Result<(), HostFault>;

    /// `reset`: the member back to its reset value.
    fn reset(&mut self, at: &Access) -> Result<(), HostFault>;

    /// The next 32-bit number of the session's [`crate::Generator`]
    /// (decision 11).
    fn draw(&mut self) -> u32;

    /// A world op. The default leaves it unmodelled.
    fn op(&mut self, call: &OpCall) -> Result<Answer, HostFault> {
        let _ = call;
        Err(HostFault::Unmodelled)
    }

    /// A [`Kind::Set`] member's members, as the engine holds them now.
    /// The default leaves it unmodelled.
    fn elements(&mut self, at: &Access) -> Result<Vec<Value>, HostFault> {
        let _ = at;
        Err(HostFault::Unmodelled)
    }

    /// A fresh engine object of `class` (lowercased, `actorwaypoint`): what
    /// a set of them grows by when a script raises its count (crate docs,
    /// Variables, structures and sets). The default makes none, and the
    /// new element reads nothing.
    fn make(&mut self, class: &str) -> Option<Value> {
        let _ = class;
        None
    }

    /// The scripted placements world ops have spawned since it was last
    /// asked, each with its name and its script in `program`: the VM adds
    /// them and ticks each this tick, after every placement in the order
    /// (crate docs, The tick). The default spawns none; an embedder that
    /// leaves them listed adds them itself, and they tick from the next.
    fn adopt(&mut self, program: &Program) -> Vec<(Handle, String, ScriptId)> {
        let _ = program;
        Vec::new()
    }
}
