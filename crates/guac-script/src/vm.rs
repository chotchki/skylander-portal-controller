//! The interpreter: placements, the tick, blocks, macros, paths, cells, sets
//! and structures, as the crate docs set them out. Every failure is a value;
//! nothing here panics on a program, however it's shaped.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use guac_formats::script::{Class, StackClass};

use crate::host::{
    Access, Answer, Builtin, Host, HostFault, Kind, MemberId, Members, OpCall, Place,
};
use crate::program::{
    Container, Contents, Decl, DefId, DefKind, Elem, Global, GlobalId, Ins, LiteralClass, Obj,
    OpId, Operand, OperandId, Param, Program, ScriptId, Sym, Tag,
};
use crate::slide::{between, eased};
use crate::value::{Value, arith_in, compare, convert, random_literal};
use crate::whitelist::{Listed, Whitelist};

/// A placement as the host knows it (an entity's bits).
pub type Handle = u64;
/// A placement's place in the VM, in the order added.
pub type PlacementId = u32;

/// Over twice the largest single tick the disc tests run (`Init_Setup.ai`'s
/// first: 142,149 op executions since the review's parameter-frame fix,
/// aj-vm's tool 57,418), so only a script that loops inside a tick reaches
/// it.
pub const DEFAULT_BUDGET: u32 = 300_000;

/// How deep expressions may nest and structures contain structures: the
/// disc's go a few deep; the bound keeps a graph that loops back on itself
/// from recursing forever.
const MAX_DEPTH: u32 = 32;

/// The counter past every list: a behaviour's end, a shutdown's.
const END: u32 = u32::MAX;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    /// Tests: a stop halts the run and comes back as a [`Halt`].
    Strict,
    /// The game: a stop ends that placement's script for good, logged, and
    /// the rest run on.
    Play,
}

/// How the VM computes with fractions (crate docs, Values and arithmetic).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Precision {
    /// The original's: single precision, every result rounded.
    #[default]
    Single,
    /// A perturbation for the tests that must fail without the original's
    /// rounding (design decision 18's exact layer): fractions are carried
    /// in double precision from the first operation that makes one, in
    /// variables too, and rounded only where a member takes them.
    Double,
}

/// A logic tick's seconds at the game's 60 ticks a second, as the
/// original's single-precision step holds it.
pub const TICK_SECONDS: f32 = 1.0 / 60.0;

/// The original's pool of sliders (aj-vm): a slide begun with every one
/// running lands at once and skips its block (crate docs, Slides).
pub const SLIDERS: usize = 400;

#[derive(Debug, Clone)]
pub struct Config {
    pub mode: Mode,
    /// Ops one placement may execute in one tick.
    pub budget: u32,
    pub members: Members,
    pub whitelist: Whitelist,
    pub precision: Precision,
    /// What each tick adds to a placement's `system.script timer`.
    pub tick_seconds: f32,
}

impl Config {
    pub fn new(mode: Mode) -> Self {
        Self {
            mode,
            budget: DEFAULT_BUDGET,
            members: Members::new(),
            whitelist: Whitelist::All,
            precision: Precision::Single,
            tick_seconds: TICK_SECONDS,
        }
    }
}

/// A [`Halt`]'s cause.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Reason {
    /// A member no table entry models, by name or hashes.
    UnmodelledMember(String),
    /// [`crate::Host::op`] answered unmodelled.
    UnmodelledOp(Class),
    /// What the VM met that it doesn't model, in words.
    Unsupported(String),
    /// More ops in one tick than the budget.
    Budget(u32),
}

/// A stopped script: whose, where and why.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Halt {
    /// `None` for a [`Vm::call`] outside any placement.
    pub placement: Option<Handle>,
    pub placement_name: String,
    pub script: String,
    pub position: u32,
    pub op: Option<Class>,
    pub reason: Reason,
}

impl std::fmt::Display for Halt {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let why = match &self.reason {
            Reason::UnmodelledMember(m) => format!("unmodelled member {m}"),
            Reason::UnmodelledOp(c) => format!("unmodelled op {}", c.type_name()),
            Reason::Unsupported(s) => format!("unsupported: {s}"),
            Reason::Budget(n) => format!("over the budget of {n} ops in one tick"),
        };
        write!(
            f,
            "{} ({}) stopped at {} #{}{}: {why}",
            self.placement_name,
            self.placement
                .map_or("no placement".to_owned(), |h| format!("{h:#x}")),
            self.script,
            self.position,
            self.op
                .map_or(String::new(), |c| format!(" {}", c.type_name())),
        )
    }
}

impl std::error::Error for Halt {}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LogLine {
    pub placement: Option<Handle>,
    pub text: String,
}

/// One [`Vm::step`]: placements ticked, ops run, play-mode stops.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct StepReport {
    pub ticked: u32,
    pub ops: u64,
    /// Play mode: the scripts this step stopped.
    pub stopped: Vec<Halt>,
}

/// A failure inside a tick, before it becomes a [`Reason`].
#[derive(Debug, Clone)]
enum Fault {
    /// A path through nothing: writes drop, branch ops skip.
    Nothing,
    Member(MemberId),
    Op(Class),
    Unsupported(String),
    Budget,
}

fn unsupported<T>(what: impl Into<String>) -> Result<T, Fault> {
    Err(Fault::Unsupported(what.into()))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CellKind {
    Value,
    Reference,
    Set,
    SetReference,
    Structure,
}

#[derive(Debug, Clone)]
struct Cell {
    kind: CellKind,
    value: Value,
    /// The file variable it was made from.
    origin: Option<Obj>,
}

#[derive(Debug, Clone, Default)]
struct SetObj {
    elems: Vec<Value>,
    elem: Option<Contents>,
    /// Where `next` reads from.
    next: usize,
}

#[derive(Debug, Clone)]
struct StructObj {
    fields: Vec<(Sym, u32)>,
}

#[derive(Debug, Clone)]
struct Placement {
    handle: Handle,
    name: String,
    script: ScriptId,
    /// Its object in its file, for a placement a file holds: whose tags
    /// `find variable` reaches.
    obj: Option<Obj>,
    started: bool,
    /// Its first tick, STARTUP's, ran through without stopping.
    set_up: bool,
    behaviour: Option<u32>,
    shutdown: bool,
    locals: HashMap<Sym, u32>,
    stopped: Option<Halt>,
    inert: bool,
    /// Taken off by a `remove`: never ticked again.
    removed: bool,
    /// Whether its variable list exists for `find variable`: from the end of
    /// its STARTUP's tick on, or once another script made a variable on it.
    listed: bool,
    /// Listed for its first tick alone ([`Listed::FirstTick`]).
    first_tick: bool,
    /// Its first tick done: never ticked again, not taken off.
    held: bool,
    /// Inactive (the embedder's activity rule): not ticked until it wakes,
    /// its state kept.
    asleep: bool,
    /// An activator ([`Vm::set_activator`]): its group ticks ahead of every
    /// other placement (crate docs, The tick).
    activator: bool,
    /// Seconds of ticks so far: `system.script timer`.
    active: f32,
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum BlockKind {
    Plain,
    /// A `spawn` or `tell` whose block runs with another receiver: the one to
    /// put back.
    Receiver(Value),
    /// [`Vm::call`]'s caller: the body's return ends the run.
    Root,
}

#[derive(Debug, Clone, Copy)]
struct Block {
    ctx: ScriptId,
    start: u32,
    len: u32,
    op: OpId,
    kind: BlockKind,
}

#[derive(Debug, Clone, Copy)]
struct Call {
    def: DefId,
    /// The context the call was made in, restored on return.
    caller: Option<DefId>,
    /// The body has returned: the use block's after-return code runs.
    returned: bool,
}

#[derive(Debug, Clone, Default)]
struct OpState {
    index: i32,
    count: i32,
    items: Vec<Value>,
    set: Option<u32>,
    cached: Option<Value>,
}

/// The registers of the tick in progress.
#[derive(Debug, Clone)]
struct Run {
    placement: Option<PlacementId>,
    ctx: ScriptId,
    pc: u32,
    blocks: Vec<Block>,
    calls: Vec<Call>,
    /// The running macro's frame, or `None` for the placement's locals.
    frame: Option<DefId>,
    receiver: Value,
    its: Value,
    other_point: Option<Value>,
    ops: u32,
    /// The position of the op executing, for a [`Halt`].
    at: u32,
    at_op: Option<OpId>,
    /// [`Vm::run_span`]'s end: the run stops once the counter reaches it
    /// in the stretch's own list with no call open.
    stop: Option<u32>,
}

/// A path's end: where a read comes from and a write goes.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Spot {
    Cell(u32),
    Const(Value),
    /// A set's `count`: reads its length, a write resizes it.
    Count(u32),
    CastToInteger,
    Undefined,
    Member {
        receiver: Option<Value>,
        member: MemberId,
        /// A value cell's slot.
        cell: Option<u32>,
        /// The path's `_otherPoint`, read as the path was walked: what a
        /// distance or a heading on it is measured from.
        other: Option<Value>,
    },
    /// A part of a packed screen position or colour, read from and written
    /// back through the place that holds it (crate docs, Members).
    Packed {
        base: Base,
        part: Packed,
    },
    /// A slider's sequence member (crate docs, Slides).
    Slider {
        id: u32,
        part: SliderPart,
    },
}

/// What of a slider a `sequence` member reads and writes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SliderPart {
    /// `time remaining`, seconds: what's left of this pass; written, how
    /// much is.
    Left,
    /// `percent remaining`: the same over its time.
    Percent,
    /// `playback rate`, percent: a frame moves it a tick's seconds times
    /// this over 100.
    Rate,
    /// `playback mode`: 0 one way, 1 ping pong.
    Mode,
    /// `looping`: 0 or 1.
    Looping,
    /// `raw duration`, seconds: its time (read only).
    Duration,
    /// `slider.ignores time scales`: kept, and no time scale is modelled to
    /// ignore (a slider runs on the tick's seconds either way).
    IgnoresTimeScales,
}

impl SliderPart {
    fn of(m: MemberId) -> Option<Self> {
        [
            ("time remaining", SliderPart::Left),
            ("percent remaining", SliderPart::Percent),
            ("playback rate", SliderPart::Rate),
            ("playback mode", SliderPart::Mode),
            ("looping", SliderPart::Looping),
            ("raw duration", SliderPart::Duration),
        ]
        .into_iter()
        .find(|(n, _)| MemberId::named("sequence", n) == m)
        .map(|(_, p)| p)
        .or_else(|| {
            (MemberId::named("slider", "ignores time scales") == m)
                .then_some(SliderPart::IgnoresTimeScales)
        })
    }
}

/// Where a packed value lives: a cell, a member or a constant.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Base {
    Cell(u32),
    Const(Value),
    Member {
        receiver: Option<Value>,
        member: MemberId,
        cell: Option<u32>,
        other: Option<Value>,
    },
}

impl Base {
    fn spot(self) -> Spot {
        match self {
            Base::Cell(c) => Spot::Cell(c),
            Base::Const(v) => Spot::Const(v),
            Base::Member {
                receiver,
                member,
                cell,
                other,
            } => Spot::Member {
                receiver,
                member,
                cell,
                other,
            },
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum Recv {
    Head,
    Value(u8, u64),
    ValueCell(u32),
}

fn recv(v: Value) -> Recv {
    let (tag, bits) = match v {
        Value::Null => (0, 0),
        Value::Int(i) => (1, i as u32 as u64),
        Value::Float(f) => (2, f.to_bits() as u64),
        Value::Double(d) => (11, d.to_bits()),
        Value::Color(c) => (3, c as u64),
        Value::Screen(s) => (4, s as u64),
        Value::Object(o) => (5, (o.unit as u64) << 32 | o.index as u64),
        Value::Named(a, b) => (6, (a as u64) << 32 | b as u64),
        Value::Entity(e) => (7, e),
        Value::Set(s) => (8, s as u64),
        Value::Struct(s) => (9, s as u64),
        Value::Cell(c) => (10, c as u64),
        Value::Bound(b) => (12, b as u64),
        Value::Slider(s) => (13, s as u64),
    };
    Recv::Value(tag, bits)
}

/// Whether an operand reads `its`: a path naming it, or an expression with
/// one on either side (a right side `its.$x` wrapped in an expression is
/// read anew for each member `find subset` tests).
fn reads_its(program: &Program, o: OperandId) -> bool {
    match program.operands.get(o as usize) {
        Some(Operand::Path { names_its, .. }) => *names_its,
        Some(Operand::Expr { a, b, .. }) => {
            a.is_some_and(|a| reads_its(program, a)) || b.is_some_and(|b| reads_its(program, b))
        }
        None => false,
    }
}

/// The kind of cell a declaration's container makes.
fn cell_kind(c: Container) -> Option<CellKind> {
    match c {
        Container::Value => Some(CellKind::Value),
        Container::Reference => Some(CellKind::Reference),
        Container::Set => Some(CellKind::Set),
        Container::SetReference => Some(CellKind::SetReference),
        Container::Structure => Some(CellKind::Structure),
        Container::Unknown(_) => None,
    }
}

/// A tag's value: its literal, through any expression holding it alone.
fn tag_value(program: &Program, o: OperandId) -> Option<Value> {
    match program.operands.get(o as usize)? {
        Operand::Path {
            elems,
            literal: Some(l),
            ..
        } if elems.is_empty() && !l.random => Some(l.value()),
        Operand::Expr {
            a: Some(a),
            op: None,
            b: None,
        } => tag_value(program, *a),
        _ => None,
    }
}

/// The object a tag's value names, a path of that object alone.
fn tag_object(program: &Program, o: OperandId) -> Option<Obj> {
    match program.operands.get(o as usize)? {
        Operand::Path { elems, .. } => match elems.as_slice() {
            [Elem::Object(obj)] => Some(*obj),
            [Elem::Op(op)] => Some(program.op_info.get(*op as usize)?.obj),
            _ => None,
        },
        Operand::Expr {
            a: Some(a),
            op: None,
            b: None,
        } => tag_object(program, *a),
        _ => None,
    }
}

/// The file variable a tag's value names, a path of that variable alone
/// (openspec AG.60: a Titan boulder's `Pebble_Set`, a set reference tag
/// naming the level's set of pebble templates).
fn tag_global(program: &Program, o: OperandId) -> Option<GlobalId> {
    match program.operands.get(o as usize)? {
        Operand::Path { elems, .. } => match elems.as_slice() {
            [Elem::Global(g)] => Some(*g),
            _ => None,
        },
        Operand::Expr {
            a: Some(a),
            op: None,
            b: None,
        } => tag_global(program, *a),
        _ => None,
    }
}

/// What an embedder runs in a held placement's place during a step's walk
/// ([`Vm::step_with`]): the VM, the placement and the host.
pub type Native<'a> = dyn FnMut(&mut Vm, PlacementId, &mut dyn Host) -> Result<(), Halt> + 'a;

/// The interpreter over one [`Program`].
pub struct Vm {
    program: Arc<Program>,
    config: Config,
    placements: Vec<Placement>,
    /// The tick order (crate docs, The tick): the activators' group, then
    /// every other placement's, each in the order added, a placement woken
    /// from sleep moved to its group's end.
    activators: Vec<PlacementId>,
    order: Vec<PlacementId>,
    /// Placements sent round their states ([`Vm::requeue`]), in that order:
    /// off their group's list until the walk has passed every group, then
    /// back on its end and ticked there.
    rejoin: Vec<PlacementId>,
    /// The walk's place during a step: the group (true the activators') and
    /// the index of the next placement in it, stepped back when a placement
    /// at or before it leaves the list, so the walk skips nothing.
    cursor: Option<(bool, usize)>,
    cells: Vec<Cell>,
    sets: Vec<SetObj>,
    structs: Vec<StructObj>,
    globals: Vec<Option<u32>>,
    frames: HashMap<DefId, HashMap<Sym, u32>>,
    busy: HashSet<DefId>,
    op_state: HashMap<OpId, OpState>,
    value_cells: HashMap<(Recv, MemberId), u32>,
    value_cell_values: Vec<Value>,
    /// Each engine member's reads and writes since the VM was made.
    uses: HashMap<MemberId, (u64, u64)>,
    /// A set member's set, per receiver: refilled from the host each read.
    engine_sets: HashMap<(Recv, MemberId), u32>,
    /// The places [`Value::Bound`] names.
    bound: Vec<Spot>,
    /// What a file object naming a placement reads as: its handle.
    objects: HashMap<Obj, Handle>,
    /// The file object each named handle stands for: what a placement with
    /// no script finds its tags by.
    named: HashMap<Handle, Obj>,
    /// The VM's placement running under each handle.
    handles: HashMap<Handle, PlacementId>,
    /// `find variable`'s variables, by owner, name and declaration.
    found: HashMap<(Recv, Sym, Decl), u32>,
    cast: Value,
    reached: HashSet<MemberId>,
    calls: HashMap<DefId, u64>,
    call_sites: HashMap<(ScriptId, u32), u64>,
    log: Vec<LogLine>,
    executed: u64,
    run: Run,
    /// [`Vm::call_with`]'s returns: read from the callee's frame as its
    /// body returns, before the frame resets.
    capture: Option<Capture>,
    /// [`Vm::trace`]: each op executed, up to a cap.
    trace: Option<(usize, Vec<(ScriptId, u32)>)>,
    /// Every slider made, in the order begun ([`Value::Slider`] names one);
    /// those running are stepped once a frame ([`Vm::slide`]).
    sliders: Vec<Slider>,
    running: usize,
    /// The sliders that may still run, by index, in the order begun: what
    /// a frame's slides and a script's write scan, rather than every
    /// slider a session has made (about 25 a second in the fight). One
    /// that ended leaves at the next scan.
    live: Vec<u32>,
    /// Frames [`Vm::slide`] has begun: a slider begun in a frame first moves
    /// in the next.
    frame: u64,
    /// [`Vm::slide`] has run this frame ([`Vm::step`] clears it).
    slid: bool,
    /// A slider's own write, which doesn't end it.
    sliding: bool,
}

/// A `slide value` under way: where it writes, from what to what, over how
/// long and eased how, and whose script began it.
#[derive(Debug, Clone)]
struct Slider {
    spot: Spot,
    placement: Option<PlacementId>,
    ctx: ScriptId,
    receiver: Value,
    from: Value,
    to: Value,
    time: f32,
    out: f32,
    in_: f32,
    elapsed: f32,
    /// `sequence.playback rate`, percent: a frame moves it a tick's seconds
    /// times this over 100.
    rate: f32,
    /// `sequence.looping` and `.playback mode` (ping pong): a looping slide
    /// starts again at its time's end, a ping pong one back the way it
    /// came, and never ends on its own.
    looping: bool,
    ping_pong: bool,
    ignores_time_scales: bool,
    /// Running from its target back to where it began (a ping pong's
    /// return).
    back: bool,
    begun: u64,
    running: bool,
}

#[derive(Debug, Clone)]
struct Capture {
    def: DefId,
    names: Vec<Option<Sym>>,
    values: Vec<Option<Value>>,
}

impl Vm {
    pub fn new(program: Arc<Program>, config: Config) -> Self {
        let globals = vec![None; program.globals.len()];
        Self {
            program,
            config,
            placements: Vec::new(),
            activators: Vec::new(),
            order: Vec::new(),
            rejoin: Vec::new(),
            cursor: None,
            cells: Vec::new(),
            sets: Vec::new(),
            structs: Vec::new(),
            globals,
            frames: HashMap::new(),
            busy: HashSet::new(),
            op_state: HashMap::new(),
            value_cells: HashMap::new(),
            value_cell_values: Vec::new(),
            uses: HashMap::new(),
            engine_sets: HashMap::new(),
            bound: Vec::new(),
            objects: HashMap::new(),
            named: HashMap::new(),
            handles: HashMap::new(),
            found: HashMap::new(),
            cast: Value::Int(0),
            reached: HashSet::new(),
            calls: HashMap::new(),
            call_sites: HashMap::new(),
            log: Vec::new(),
            executed: 0,
            capture: None,
            trace: None,
            sliders: Vec::new(),
            running: 0,
            live: Vec::new(),
            frame: 0,
            slid: false,
            sliding: false,
            run: Run {
                placement: None,
                ctx: 0,
                pc: 0,
                blocks: Vec::new(),
                calls: Vec::new(),
                frame: None,
                receiver: Value::Null,
                its: Value::Null,
                other_point: None,
                ops: 0,
                at: 0,
                at_op: None,
                stop: None,
            },
        }
    }

    pub fn program(&self) -> &Program {
        &self.program
    }

    /// The program, shared: what an embedder holds while it adds to the VM.
    pub fn program_arc(&self) -> Arc<Program> {
        Arc::clone(&self.program)
    }

    pub fn config(&self) -> &Config {
        &self.config
    }

    /// The seconds each tick and slide steps from now on
    /// ([`Config::tick_seconds`]): a model replay steps the oracle's own
    /// frames this way (design decision 18's model replay), where the TV
    /// ticks 1/60 s throughout.
    pub fn set_tick_seconds(&mut self, seconds: f32) {
        self.config.tick_seconds = seconds;
    }

    /// Adds a placement running `script`, ticked after those added before
    /// it. Off the whitelist it stays inert: never ticked, listed.
    pub fn add(&mut self, handle: Handle, name: &str, script: ScriptId) -> PlacementId {
        let script_name = self
            .program
            .scripts
            .get(script as usize)
            .map_or("", |s| s.name.as_str());
        let listing = self.config.whitelist.listing(name, script_name);
        let inert = listing == Listed::Off;
        if inert {
            self.log.push(LogLine {
                placement: Some(handle),
                text: format!("{name}: off the whitelist ({script_name}), inert"),
            });
        }
        self.placements.push(Placement {
            handle,
            name: name.to_owned(),
            script,
            obj: None,
            started: false,
            set_up: false,
            behaviour: None,
            shutdown: false,
            locals: HashMap::new(),
            stopped: None,
            inert,
            removed: false,
            listed: false,
            first_tick: listing == Listed::FirstTick,
            held: false,
            asleep: false,
            activator: false,
            active: 0.0,
        });
        let id = (self.placements.len() - 1) as PlacementId;
        self.order.push(id);
        self.handles.entry(handle).or_insert(id);
        // Variables `find variable` made on it before it ran become its
        // locals.
        let key = recv(Value::Entity(handle));
        let moved: Vec<((Recv, Sym, Decl), u32)> = self
            .found
            .iter()
            .filter(|((owner, ..), _)| *owner == key)
            .map(|(k, c)| (*k, *c))
            .collect();
        for (k, c) in moved {
            self.found.remove(&k);
            let p = &mut self.placements[id as usize];
            p.locals.insert(k.1, c);
            p.listed = true;
        }
        id
    }

    /// [`Vm::add`] for a placement of a loaded file: `obj`, wherever a
    /// script names it, reads as `handle`.
    pub fn add_at(
        &mut self,
        handle: Handle,
        name: &str,
        script: ScriptId,
        obj: Obj,
    ) -> PlacementId {
        self.name_object(obj, handle);
        let id = self.add(handle, name, script);
        self.placements[id as usize].obj = Some(obj);
        id
    }

    /// A file object (a placement the host keeps, scripted or not) that
    /// reads as `handle` wherever a script names it, so a placement is one
    /// value however a script reaches it: by name, as `my`, from a member.
    pub fn name_object(&mut self, obj: Obj, handle: Handle) {
        self.objects.insert(obj, handle);
        self.named.insert(handle, obj);
    }

    /// Whether a `remove` took the placement off.
    pub fn removed(&self, id: PlacementId) -> bool {
        self.placements.get(id as usize).is_some_and(|p| p.removed)
    }

    /// The placements listed for their first tick that have had it.
    /// Whether a placement is held after its first tick (the whitelist
    /// listing its script for that tick alone).
    pub fn is_held(&self, id: PlacementId) -> bool {
        self.placements.get(id as usize).is_some_and(|p| p.held)
    }

    pub fn held(&self) -> Vec<Handle> {
        self.placements
            .iter()
            .filter(|p| p.held)
            .map(|p| p.handle)
            .collect()
    }

    /// The placement running under `handle`, if one does.
    pub fn placement_of(&self, handle: Handle) -> Option<PlacementId> {
        self.handles.get(&handle).copied()
    }

    /// The handle a placement runs under.
    pub fn handle_of(&self, id: PlacementId) -> Option<Handle> {
        self.placements.get(id as usize).map(|p| p.handle)
    }

    /// Sets a placement local by name, made as a number if the script
    /// hasn't used it yet: how an embedder puts a placement in a recorded
    /// state. False when no op names it or the placement doesn't exist.
    pub fn set_local(&mut self, id: PlacementId, name: &str, value: Value) -> bool {
        let Some(sym) = self.program.find_sym(name) else {
            return false;
        };
        if self.placements.get(id as usize).is_none() {
            return false;
        }
        let c = match self.placements[id as usize].locals.get(&sym) {
            Some(&c) => c,
            None => {
                self.cells.push(Cell {
                    kind: CellKind::Value,
                    value: Value::Int(0),
                    origin: None,
                });
                let c = (self.cells.len() - 1) as u32;
                self.placements[id as usize].locals.insert(sym, c);
                c
            }
        };
        self.write_cell(c, value, 0);
        true
    }

    /// Puts the placement running under `handle` to sleep or wakes it
    /// (crate docs, The tick): an inactive placement isn't ticked and keeps
    /// its state, its STARTUP running on its first tick awake; one woken
    /// ticks after every placement already awake. False when no placement
    /// runs under it.
    pub fn set_asleep(&mut self, handle: Handle, asleep: bool) -> bool {
        let Some(&id) = self.handles.get(&handle) else {
            return false;
        };
        let p = &mut self.placements[id as usize];
        let woken = p.asleep && !asleep;
        p.asleep = asleep;
        if woken {
            self.move_to_end(id);
        }
        true
    }

    /// Makes the placement running under `handle` an activator, or not
    /// (crate docs, The tick): the hero's own minion, whose group ticks
    /// ahead of every other placement; it joins its new group's end. False
    /// when no placement runs under it.
    pub fn set_activator(&mut self, handle: Handle, activator: bool) -> bool {
        let Some(&id) = self.handles.get(&handle) else {
            return false;
        };
        if self.placements[id as usize].activator != activator {
            self.unlist(id);
            self.placements[id as usize].activator = activator;
            let list = self.list_mut(activator);
            list.push(id);
        }
        true
    }

    /// Sends the placement running under `handle` round its states, as the
    /// original's `reset` of a placement does (crate docs, The tick): off
    /// its group's list, its `system.script timer` back to 0, and back on
    /// the list's end once the walk has passed every group, ticking there:
    /// after every placement on the lists this step, those that join them
    /// during the walk included. False when no placement runs under it.
    pub fn requeue(&mut self, handle: Handle) -> bool {
        let Some(&id) = self.handles.get(&handle) else {
            return false;
        };
        self.unlist(id);
        self.rejoin.push(id);
        self.placements[id as usize].active = 0.0;
        true
    }

    /// Whether the placement is an activator.
    pub fn is_activator(&self, id: PlacementId) -> bool {
        self.placements
            .get(id as usize)
            .is_some_and(|p| p.activator)
    }

    /// The tick order as it stands: the activators', then the rest, each
    /// by name in its group's order, then those waiting to rejoin
    /// ([`Vm::requeue`]), each with its group.
    pub fn tick_order(&self) -> Vec<(bool, String)> {
        self.activators
            .iter()
            .map(|&id| (true, id))
            .chain(self.order.iter().map(|&id| (false, id)))
            .chain(
                self.rejoin
                    .iter()
                    .map(|&id| (self.placements[id as usize].activator, id)),
            )
            .map(|(g, id)| (g, self.placements[id as usize].name.clone()))
            .collect()
    }

    fn list_mut(&mut self, activators: bool) -> &mut Vec<PlacementId> {
        if activators {
            &mut self.activators
        } else {
            &mut self.order
        }
    }

    /// Takes `id` off its group's list, the walk's cursor stepped back when
    /// it sat before it in the group being walked, or off the rejoin list.
    fn unlist(&mut self, id: PlacementId) {
        if let Some(at) = self.rejoin.iter().position(|&o| o == id) {
            self.rejoin.remove(at);
            return;
        }
        let group = self.placements[id as usize].activator;
        let list = self.list_mut(group);
        let Some(at) = list.iter().position(|&o| o == id) else {
            return;
        };
        list.remove(at);
        if let Some((g, next)) = self.cursor.as_mut()
            && *g == group
            && at < *next
        {
            *next -= 1;
        }
    }

    /// `id` off its group's list and back on at its end.
    fn move_to_end(&mut self, id: PlacementId) {
        self.unlist(id);
        let group = self.placements[id as usize].activator;
        self.list_mut(group).push(id);
    }

    /// Takes the placement running under `handle` off as a script's `remove`
    /// of it does: its script stopped for good, never told again. An
    /// embedder's, for a placement a run carries over as taken off (openspec
    /// AH.27). False when none runs under it or it's already off.
    pub fn remove(&mut self, handle: Handle) -> bool {
        self.handles.contains_key(&handle) && self.take_off(Value::Entity(handle))
    }

    /// Whether the placement is asleep (inactive).
    pub fn asleep(&self, id: PlacementId) -> bool {
        self.placements.get(id as usize).is_some_and(|p| p.asleep)
    }

    /// The placements that are inert, off the whitelist.
    pub fn inert(&self) -> Vec<Handle> {
        self.placements
            .iter()
            .filter(|p| p.inert)
            .map(|p| p.handle)
            .collect()
    }

    /// Asks a placement to run its SHUTDOWN on its next tick.
    pub fn request_shutdown(&mut self, id: PlacementId) {
        if let Some(p) = self.placements.get_mut(id as usize) {
            p.shutdown = true;
        }
    }

    /// The running placement's behaviour as the object `set behavior`
    /// names; nothing outside a placement or before its first.
    fn current_behaviour(&self) -> Value {
        let Some(p) = self
            .run
            .placement
            .and_then(|id| self.placements.get(id as usize))
        else {
            return Value::Null;
        };
        let op = p.behaviour.and_then(|pos| {
            self.program
                .scripts
                .get(p.script as usize)?
                .ops
                .get(pos as usize)
                .copied()
        });
        match op {
            Some(op) => Value::Object(self.program.op_info[op as usize].obj),
            None => Value::Null,
        }
    }

    /// The placement's current behaviour, by name as lowercased.
    pub fn behaviour(&self, id: PlacementId) -> Option<&str> {
        let p = self.placements.get(id as usize)?;
        let pos = p.behaviour?;
        let script = self.program.scripts.get(p.script as usize)?;
        let op = *script.ops.get(pos as usize)?;
        match &self.program.ops[op as usize] {
            Ins::Behaviour { name } => Some(self.program.sym(*name)),
            _ => None,
        }
    }

    /// Why a placement's script stopped, in play mode.
    pub fn stopped(&self, id: PlacementId) -> Option<&Halt> {
        self.placements.get(id as usize)?.stopped.as_ref()
    }

    /// A placement local by name; `None` until the script has used it.
    pub fn local(&self, id: PlacementId, name: &str) -> Option<Value> {
        let sym = self.program.find_sym(name)?;
        let c = *self.placements.get(id as usize)?.locals.get(&sym)?;
        Some(self.cells[c as usize].value)
    }

    /// A macro frame's variable, by name: what a call left there before its
    /// use block closed (nothing, once it has).
    pub fn frame(&self, def: DefId, name: &str) -> Option<Value> {
        let sym = self.program.find_sym(name)?;
        let c = *self.frames.get(&def)?.get(&sym)?;
        Some(self.cells[c as usize].value)
    }

    /// A file variable's value, made from the file if nothing has read it
    /// yet.
    pub fn global(&mut self, g: GlobalId) -> Option<Value> {
        let c = self.global_cell(g).ok()?;
        Some(self.cells[c as usize].value)
    }

    /// A file variable's value if something has made it yet, without making
    /// it: what a reader holding the VM shared asks.
    pub fn global_peek(&self, g: GlobalId) -> Option<Value> {
        let c = self.globals.get(g as usize).copied().flatten()?;
        Some(self.cells[c as usize].value)
    }

    /// Sets a file variable's number, made from the file first if nothing
    /// has read it yet: how an embedder puts the shared state in a recorded
    /// state (a test standing in for a script the whitelist doesn't run).
    /// False when it isn't a number cell.
    pub fn set_global(&mut self, g: GlobalId, value: Value) -> bool {
        let Ok(c) = self.global_cell(g) else {
            return false;
        };
        if self.cells[c as usize].kind != CellKind::Value {
            return false;
        }
        self.write_cell(c, value, 0);
        true
    }

    /// The members of the set `v` holds or names; `None` for a non-set.
    pub fn set(&self, v: Value) -> Option<&[Value]> {
        match self.peek(v) {
            Value::Set(s) => self.sets.get(s as usize).map(|s| s.elems.as_slice()),
            _ => None,
        }
    }

    /// A field of the structure `v` holds or names; `None` when missing.
    pub fn field(&self, v: Value, name: &str) -> Option<Value> {
        let sym = self.program.find_sym(name)?;
        match self.peek(v) {
            Value::Struct(s) => {
                let (_, c) = self
                    .structs
                    .get(s as usize)?
                    .fields
                    .iter()
                    .find(|(n, _)| *n == sym)?;
                Some(self.cells[*c as usize].value)
            }
            _ => None,
        }
    }

    /// Sets a number field of the structure `v` holds or names: how an
    /// embedder puts a recorded state's shared numbers in (the combat
    /// manager's next alert at an anchor). False when the structure has no
    /// number field of that name.
    pub fn set_field(&mut self, v: Value, name: &str, value: Value) -> bool {
        let Some(sym) = self.program.find_sym(name) else {
            return false;
        };
        let Value::Struct(s) = self.peek(v) else {
            return false;
        };
        let Some(c) = self
            .structs
            .get(s as usize)
            .and_then(|st| st.fields.iter().find(|(n, _)| *n == sym))
            .map(|(_, c)| *c)
        else {
            return false;
        };
        if self.cells[c as usize].kind != CellKind::Value {
            return false;
        }
        self.write_cell(c, value, 0);
        true
    }

    /// Points a reference field of the structure `v` holds or names at
    /// `target`, as `setref` does: how an embedder makes a link the
    /// original's own unrun scripts make (the portal manager handing a
    /// slot its tag's data, openspec AH.23). False when the structure has
    /// no reference field of that name.
    pub fn set_reference(&mut self, v: Value, name: &str, target: Value) -> bool {
        let Some(sym) = self.program.find_sym(name) else {
            return false;
        };
        let Value::Struct(s) = self.peek(v) else {
            return false;
        };
        let Some(c) = self
            .structs
            .get(s as usize)
            .and_then(|st| st.fields.iter().find(|(n, _)| *n == sym))
            .map(|(_, c)| *c)
        else {
            return false;
        };
        if !matches!(
            self.cells[c as usize].kind,
            CellKind::Reference | CellKind::SetReference
        ) {
            return false;
        }
        self.write_cell(c, target, 0);
        true
    }

    /// Writes a number into the number variable `v` names (a set's member,
    /// [`Value::Cell`]), converted as a script's write converts it: how an
    /// embedder puts a figure's words in its tag's save area (AH.23). False
    /// when `v` names no number variable.
    pub fn put(&mut self, v: Value, value: Value) -> bool {
        let Value::Cell(c) = v else {
            return false;
        };
        match self.cells.get(c as usize) {
            Some(cell) if cell.kind == CellKind::Value => {
                self.write_cell(c, value, 0);
                true
            }
            _ => false,
        }
    }

    /// Appends one fresh default member to the set `v` holds or names, as
    /// `inc count` does, and returns it: how an embedder makes what the
    /// original's own unrun scripts would (the portal save control's data
    /// for a tag put on the portal, AH.23). `None` for anything but a set,
    /// or a member that can't be made.
    pub fn grow(&mut self, v: Value, host: &mut dyn Host) -> Option<Value> {
        let Value::Set(s) = self.materialize(v) else {
            return None;
        };
        let n = self.sets.get(s as usize)?.elems.len();
        self.resize(s, n + 1, host).ok()?;
        self.sets[s as usize].elems.last().copied()
    }

    /// What `v` holds when it names a cell or a file variable that has one
    /// (a set's number member, a reference's target); `v` itself otherwise.
    pub fn value(&self, v: Value) -> Value {
        self.peek(v)
    }

    /// Every field the structure `v` holds or names, by name as
    /// lowercased, in the order they were made.
    pub fn fields(&self, v: Value) -> Vec<(&str, Value)> {
        match self.peek(v) {
            Value::Struct(s) => self.structs.get(s as usize).map_or(Vec::new(), |st| {
                st.fields
                    .iter()
                    .map(|(n, c)| (self.program.sym(*n), self.cells[*c as usize].value))
                    .collect()
            }),
            _ => Vec::new(),
        }
    }

    /// A value cell's content (`None` until written or read).
    pub fn value_cell(&self, receiver: Option<Value>, member: MemberId) -> Option<Value> {
        let key = (receiver.map_or(Recv::Head, recv), member);
        let slot = *self.value_cells.get(&key)?;
        self.value_cell_values.get(slot as usize).copied()
    }

    /// Every member a path has named since the VM was made, the VM's own
    /// included: the count's "reached".
    pub fn reached(&self) -> &HashSet<MemberId> {
        &self.reached
    }

    /// Each engine member a path read or wrote since the VM was made: its
    /// reads and writes (a value cell's included), the count's evidence
    /// for whether a branch reads one.
    pub fn uses(&self) -> &HashMap<MemberId, (u64, u64)> {
        &self.uses
    }

    /// A placement's `system.script timer`: seconds of ticks so far.
    pub fn active_seconds(&self, id: PlacementId) -> Option<f32> {
        self.placements.get(id as usize).map(|p| p.active)
    }

    /// Calls per macro since the VM was made (a call [`Vm::call`] makes
    /// included).
    pub fn calls(&self) -> &HashMap<DefId, u64> {
        &self.calls
    }

    /// Calls per call site (script, position) since the VM was made.
    pub fn call_sites(&self) -> &HashMap<(ScriptId, u32), u64> {
        &self.call_sites
    }

    /// Ops executed since the VM was made.
    pub fn executed(&self) -> u64 {
        self.executed
    }

    pub fn log(&self) -> &[LogLine] {
        &self.log
    }

    pub fn drain_log(&mut self) -> Vec<LogLine> {
        std::mem::take(&mut self.log)
    }

    /// Records each op executed from here on, as (script, position), up to
    /// `cap` of them (`None` stops recording): a tool's view of what a
    /// tick ran.
    pub fn trace(&mut self, cap: Option<usize>) {
        self.trace = cap.map(|c| (c, Vec::new()));
    }

    /// What [`Vm::trace`] has recorded, taken.
    pub fn traced(&mut self) -> Vec<(ScriptId, u32)> {
        self.trace
            .as_mut()
            .map(|(_, t)| std::mem::take(t))
            .unwrap_or_default()
    }

    /// One logic tick: every placement that isn't inert, asleep or stopped,
    /// in the tick order (crate docs, The tick). Strict mode returns the
    /// first stop; play mode records it and runs the rest.
    pub fn step(&mut self, host: &mut dyn Host) -> Result<StepReport, Halt> {
        self.step_with(host, &mut |_, _, _| Ok(()))
    }

    /// [`Vm::step`], with `native` run for each held placement where the
    /// walk reaches it (crate docs, The tick): what an embedder runs in a
    /// placement's place natively (design decision 2's player loop: the
    /// portal manager's and the game systems' ticks). A native's halt ends
    /// the step there.
    pub fn step_with(
        &mut self,
        host: &mut dyn Host,
        native: &mut Native<'_>,
    ) -> Result<StepReport, Halt> {
        let mut report = StepReport::default();
        if !self.slid {
            match self.slide(host) {
                Ok(slid) => report.stopped = slid.stopped,
                Err(halt) => {
                    self.slid = false;
                    return Err(halt);
                }
            }
        }
        let result = self.tick_all(&mut report, host, native);
        // The frame's over: the next slides begin the next.
        self.slid = false;
        self.cursor = None;
        result.map(|()| report)
    }

    /// Every placement's tick, in the tick order: the activators' group,
    /// then the rest's, each list walked from its head as it stands when the
    /// walk reaches each place; a held placement runs `native` there. What
    /// one spawns with a script of its own joins the order's end and ticks
    /// this tick too ([`Host::adopt`]). Then those sent round their states
    /// ([`Vm::requeue`]), each back on its group's end and ticked there, in
    /// the order they were sent; one sent again after its turn waits for
    /// the next step.
    fn tick_all(
        &mut self,
        report: &mut StepReport,
        host: &mut dyn Host,
        native: &mut Native<'_>,
    ) -> Result<(), Halt> {
        let program = self.program.clone();
        self.adopt(&program, host);
        for group in [true, false] {
            self.cursor = Some((group, 0));
            while let Some((g, at)) = self.cursor {
                let list = if g { &self.activators } else { &self.order };
                let Some(&id) = list.get(at) else {
                    break;
                };
                self.cursor = Some((g, at + 1));
                self.tick_in_place(id, &program, report, host, native)?;
            }
        }
        self.cursor = None;
        let mut rejoined = Vec::new();
        let mut at = 0;
        while let Some(&id) = self.rejoin.get(at) {
            if rejoined.contains(&id) {
                at += 1;
                continue;
            }
            self.rejoin.remove(at);
            rejoined.push(id);
            let group = self.placements[id as usize].activator;
            self.list_mut(group).push(id);
            self.tick_in_place(id, &program, report, host, native)?;
        }
        Ok(())
    }

    /// Placement `id`'s turn in the walk: nothing if it's inert, taken off,
    /// asleep or stopped; `native` if it's held; else its tick. What the
    /// turn spawns is adopted after it.
    fn tick_in_place(
        &mut self,
        id: PlacementId,
        program: &Program,
        report: &mut StepReport,
        host: &mut dyn Host,
        native: &mut Native<'_>,
    ) -> Result<(), Halt> {
        let p = &self.placements[id as usize];
        if p.inert || p.removed || p.asleep || p.stopped.is_some() {
            return Ok(());
        }
        if p.held {
            native(self, id, host)?;
            self.adopt(program, host);
            return Ok(());
        }
        let before = self.executed;
        let result = self.tick(id, host);
        report.ticked += 1;
        report.ops += self.executed - before;
        self.adopt(program, host);
        match result {
            Ok(None) => Ok(()),
            Ok(Some(halt)) => {
                report.stopped.push(halt);
                Ok(())
            }
            Err(halt) => Err(halt),
        }
    }

    /// The frame's slides, once a frame before the scripts (crate docs,
    /// Slides): each running slider a tick's seconds on, its place written
    /// where it now stands, one finished landing on its target and ending.
    /// A slider begun this frame first moves in the next. [`Vm::step`] runs
    /// this itself unless the embedder has this frame, as one that makes
    /// native calls ahead of the step does first. A slider whose write
    /// stops in play mode ends, and its placement's script stops with it.
    pub fn slide(&mut self, host: &mut dyn Host) -> Result<StepReport, Halt> {
        let mut report = StepReport::default();
        self.frame += 1;
        self.slid = true;
        if self.running == 0 {
            self.live.clear();
            return Ok(report);
        }
        let dt = self.config.tick_seconds;
        let live = std::mem::take(&mut self.live);
        let mut failed = None;
        for &id in &live {
            let i = id as usize;
            let s = &mut self.sliders[i];
            if !s.running || s.begun >= self.frame {
                continue;
            }
            s.elapsed += dt * s.rate / 100.0;
            let mut done = s.elapsed + 1e-6 >= s.time;
            if done && s.looping {
                s.elapsed = (s.elapsed - s.time).max(0.0);
                if s.ping_pong {
                    s.back = !s.back;
                }
                done = false;
            }
            let x = s.elapsed / s.time;
            let k = eased(if s.back { 1.0 - x } else { x }, s.out, s.in_);
            let v = if done {
                s.to
            } else {
                between(s.from, s.to, k).unwrap_or(s.to)
            };
            let (spot, placement, ctx, receiver) = (s.spot, s.placement, s.ctx, s.receiver);
            if done {
                s.running = false;
                self.running -= 1;
            }
            report.ticked += 1;
            self.begin(placement, ctx, receiver);
            self.sliding = true;
            let result = self.write(spot, v, host);
            self.sliding = false;
            match self.finish(result) {
                Ok(None) => {}
                Ok(Some(halt)) => {
                    if self.sliders[i].running {
                        self.sliders[i].running = false;
                        self.running -= 1;
                    }
                    report.stopped.push(halt);
                }
                Err(halt) => {
                    failed = Some(halt);
                    break;
                }
            }
        }
        // Any begun meanwhile follow those scanned, in the order begun.
        let begun = std::mem::replace(&mut self.live, live);
        self.live.extend(begun);
        let sliders = &self.sliders;
        self.live.retain(|&i| sliders[i as usize].running);
        match failed {
            Some(halt) => Err(halt),
            None => Ok(report),
        }
    }

    /// A tick the embedder ran natively for a placement (the portal
    /// manager's, decision 2's player loop): its `system.script timer`
    /// grows by a tick's seconds, as a tick of its own would.
    pub fn count_tick(&mut self, id: PlacementId) {
        let dt = self.config.tick_seconds;
        if let Some(p) = self.placements.get_mut(id as usize) {
            p.active += dt;
        }
    }

    /// Sliders running now.
    pub fn sliders_running(&self) -> usize {
        self.running
    }

    /// Sliders a frame's slides and a script's write scan: those running,
    /// and any that ended since the last scan.
    pub fn sliders_scanned(&self) -> usize {
        self.live.len()
    }

    /// What the host has spawned with scripts of their own, added at the
    /// order's end.
    fn adopt(&mut self, program: &Program, host: &mut dyn Host) {
        for (handle, name, script) in host.adopt(program) {
            self.add(handle, &name, script);
        }
    }

    /// One tick of one placement. `Ok(Some)` is a play-mode stop.
    pub fn tick(&mut self, id: PlacementId, host: &mut dyn Host) -> Result<Option<Halt>, Halt> {
        let Some(p) = self.placements.get(id as usize) else {
            return Ok(None);
        };
        if p.inert || p.removed || p.held || p.asleep || p.stopped.is_some() {
            return Ok(None);
        }
        let handle = p.handle;
        let script = p.script;
        self.begin(Some(id), script, Value::Entity(handle));
        let result = self.execute(host);
        let dt = self.config.tick_seconds;
        if let Some(p) = self.placements.get_mut(id as usize) {
            p.active += dt;
            p.listed |= p.started;
            if p.first_tick && p.started {
                p.held = true;
            }
        }
        let out = self.finish(result);
        if let Some(p) = self.placements.get_mut(id as usize) {
            p.set_up |= p.started && p.stopped.is_none() && out.as_ref().is_ok_and(Option::is_none);
        }
        out
    }

    /// Whether a placement's first tick, its STARTUP's, ran through without
    /// stopping: the character whose setup it is has set himself up
    /// (openspec AG.27). False for one never ticked, and for one that
    /// stopped in it.
    pub fn set_up(&self, id: PlacementId) -> bool {
        self.placements.get(id as usize).is_some_and(|p| p.set_up)
    }

    /// Runs the ops at positions `from..to` of a placement's own script as
    /// a stretch of its tick (crate docs, A stretch of a tick): its locals,
    /// `my` and its `system.script timer` as its tick reads them, no tick
    /// counted. What a native rule that schedules part of a placement's
    /// tick runs (design decision 2's player loop: the portal manager's
    /// ops for orbs, the award and loot, only their scheduling ours). The
    /// stretch starts and ends between statements of one list: every block
    /// it opens closes inside it. `Ok(true)` when it ran to `to`,
    /// `Ok(false)` when a `flow end` ended the tick inside it or the
    /// placement doesn't run (taken off, stopped, never added). A stop is
    /// returned in either mode, the placement stopped with it, as
    /// [`Vm::call_with`] returns one.
    pub fn run_span(
        &mut self,
        id: PlacementId,
        from: u32,
        to: u32,
        host: &mut dyn Host,
    ) -> Result<bool, Halt> {
        let Some(p) = self.placements.get(id as usize) else {
            return Ok(false);
        };
        if p.removed || p.stopped.is_some() {
            return Ok(false);
        }
        let len = self
            .program
            .scripts
            .get(p.script as usize)
            .map_or(0, |s| s.ops.len() as u32);
        if from > to || to > len {
            return Err(self.halt_outside(Reason::Unsupported(format!(
                "a stretch {from}..{to} of a list of {len} ops"
            ))));
        }
        let (handle, script) = (p.handle, p.script);
        self.begin(Some(id), script, Value::Entity(handle));
        self.run.pc = from;
        self.run.stop = Some(to);
        let result = if from == to {
            Ok(())
        } else {
            self.execute(host)
        };
        let ran = result.is_ok() && self.run.ctx == script && self.run.pc == to;
        match self.finish(result)? {
            None => Ok(ran),
            Some(halt) => Err(halt),
        }
    }

    /// Runs the macro `name` as a parameterless call from a fresh context,
    /// for a placement or none: what a script's first tick does when it
    /// calls one. Returns the ops it executed.
    pub fn call(
        &mut self,
        placement: Option<PlacementId>,
        name: &str,
        host: &mut dyn Host,
    ) -> Result<u32, Halt> {
        self.call_with(placement, name, &[], &[], host)
            .map(|(ops, _)| ops)
    }

    /// [`Vm::call`] with arguments, as a native caller makes one (one that
    /// only schedules the original's macros, as the join and the player loop
    /// do; stand-in ledger: `join`, `player-loop`): each `(parameter,
    /// value)` written into the callee's frame as a use op's parameter
    /// binds it, by the parameter its own INTERFACE declares under that
    /// name (any case), a set copied whole; a parameter left out reads 0,
    /// nothing or an empty set. After the body returns, each of `returns`
    /// is read from the callee's frame before it resets, as a caller's
    /// after-return code reads what a call returned (`None` for one the
    /// body never made). Returns the ops executed and those values.
    pub fn call_with(
        &mut self,
        placement: Option<PlacementId>,
        name: &str,
        args: &[(&str, Value)],
        returns: &[&str],
        host: &mut dyn Host,
    ) -> Result<(u32, Vec<Option<Value>>), Halt> {
        let Some(def) = self.program.macro_named(name) else {
            return Err(self.halt_outside(Reason::Unsupported(format!("no macro {name:?}"))));
        };
        let arc = Arc::clone(&self.program);
        for &(param, value) in args {
            let Some(p) = own_parameter(&arc, def, param) else {
                return Err(self.halt_outside(Reason::Unsupported(format!(
                    "{name:?} declares no parameter {param:?}"
                ))));
            };
            let cell = match self.frame_cell(def, p.name, p.decl) {
                Ok(c) => c,
                Err(_) => {
                    return Err(self.halt_outside(Reason::Unsupported(format!(
                        "{name:?}'s parameter {param:?} can't hold a value"
                    ))));
                }
            };
            if self.cells[cell as usize].kind != CellKind::Set {
                self.reset_cell(cell, 0);
            }
            self.write_cell(cell, value, 0);
        }
        self.capture = Some(Capture {
            def,
            names: returns
                .iter()
                .map(|r| own_parameter(&arc, def, r).map(|p| p.name))
                .collect(),
            values: Vec::new(),
        });
        let receiver = placement
            .and_then(|p| self.placements.get(p as usize))
            .map_or(Value::Null, |p| Value::Entity(p.handle));
        let (sid, _) = self.program.defs[def as usize].placed.unwrap_or((0, 0));
        self.begin(placement, sid, receiver);
        self.busy.insert(def);
        self.run.calls.push(Call {
            def,
            caller: None,
            returned: false,
        });
        self.run.blocks.push(Block {
            ctx: sid,
            start: 0,
            len: 0,
            op: self.program.defs[def as usize].op,
            kind: BlockKind::Root,
        });
        let result = match self.enter(&arc, def) {
            Ok(()) => self.execute(host),
            Err(f) => Err(f),
        };
        let ops = self.run.ops;
        let finished = self.finish(result);
        let captured = self.capture.take().map(|c| c.values).unwrap_or_default();
        match finished? {
            None => {
                let mut values = captured;
                values.resize(returns.len(), None);
                Ok((ops, values))
            }
            Some(halt) => Err(halt),
        }
    }

    /// An operand's value read outside any placement, as a value read on
    /// the right of a `set` reads it (an expression computed, a path read):
    /// what a test or a tool asks of a script's data. A path through
    /// nothing reads none.
    pub fn eval(&mut self, operand: OperandId, host: &mut dyn Host) -> Result<Value, Halt> {
        let arc = Arc::clone(&self.program);
        self.begin(None, 0, Value::Null);
        match self.rhs(&arc, Some(operand), host, 0) {
            Ok(v) => {
                self.finish(Ok(()))?;
                Ok(v)
            }
            Err(Fault::Nothing) => {
                self.finish(Ok(()))?;
                Ok(Value::Null)
            }
            Err(f) => match self.finish(Err(f)) {
                Err(h) | Ok(Some(h)) => Err(h),
                Ok(None) => Ok(Value::Null),
            },
        }
    }

    fn begin(&mut self, placement: Option<PlacementId>, ctx: ScriptId, receiver: Value) {
        self.run = Run {
            placement,
            ctx,
            pc: 0,
            blocks: Vec::new(),
            calls: Vec::new(),
            frame: None,
            receiver,
            its: Value::Null,
            other_point: None,
            ops: 0,
            at: 0,
            at_op: None,
            stop: None,
        };
    }

    /// Ends a tick: closes any call still open and turns a fault into a stop.
    fn finish(&mut self, result: Result<(), Fault>) -> Result<Option<Halt>, Halt> {
        while let Some(call) = self.run.calls.pop() {
            self.close_call(call.def);
        }
        self.run.blocks.clear();
        let Err(fault) = result else {
            return Ok(None);
        };
        let reason = match fault {
            Fault::Nothing => return Ok(None),
            Fault::Member(m) => Reason::UnmodelledMember(self.config.members.name(m)),
            Fault::Op(c) => Reason::UnmodelledOp(c),
            Fault::Unsupported(s) => Reason::Unsupported(s),
            Fault::Budget => Reason::Budget(self.config.budget),
        };
        let halt = self.halt(reason);
        if let Some(id) = self.run.placement
            && let Some(p) = self.placements.get_mut(id as usize)
        {
            p.stopped = Some(halt.clone());
        }
        self.log.push(LogLine {
            placement: halt.placement,
            text: halt.to_string(),
        });
        match self.config.mode {
            Mode::Strict => Err(halt),
            Mode::Play => Ok(Some(halt)),
        }
    }

    fn halt(&self, reason: Reason) -> Halt {
        let p = self
            .run
            .placement
            .and_then(|id| self.placements.get(id as usize));
        let script = self
            .program
            .scripts
            .get(self.run.ctx as usize)
            .map_or(String::new(), |s| s.name.clone());
        Halt {
            placement: p.map(|p| p.handle),
            placement_name: p.map_or("(no placement)".to_owned(), |p| p.name.clone()),
            script,
            position: self.run.at,
            op: self.run.at_op.and_then(|o| self.program.class(o)),
            reason,
        }
    }

    fn halt_outside(&self, reason: Reason) -> Halt {
        Halt {
            placement: None,
            placement_name: "(no placement)".to_owned(),
            script: String::new(),
            position: 0,
            op: None,
            reason,
        }
    }

    // ---- the loop ----

    fn execute(&mut self, host: &mut dyn Host) -> Result<(), Fault> {
        let arc = Arc::clone(&self.program);
        let program = &*arc;
        loop {
            let Some(script) = program.scripts.get(self.run.ctx as usize) else {
                return Ok(());
            };
            let Some(&op) = script.ops.get(self.run.pc as usize) else {
                return Ok(());
            };
            self.run.ops += 1;
            self.executed += 1;
            if self.run.ops > self.config.budget {
                return Err(Fault::Budget);
            }
            self.run.at = self.run.pc;
            self.run.at_op = Some(op);
            if let Some((cap, ops)) = self.trace.as_mut()
                && ops.len() < *cap
            {
                ops.push((self.run.ctx, self.run.pc));
            }
            self.run.pc += 1;
            match self.exec(program, op, host) {
                Ok(()) => {}
                Err(Fault::Nothing) => {
                    // A path through nothing: a branch op skips its block,
                    // any other did nothing.
                    if program.class(op).is_some_and(Class::opens_block) {
                        self.skip(program, op);
                    }
                }
                Err(f) => return Err(f),
            }
            if self.run.pc == END && self.run.blocks.is_empty() {
                return Ok(());
            }
            if let Some(stop) = self.run.stop
                && self.run.calls.is_empty()
                && self.run.pc >= stop
            {
                // A stretch's end: every block it opened has closed, unless
                // a skip carried the counter past it (a stretch cut through
                // a statement).
                self.close(program)?;
                if self
                    .run
                    .blocks
                    .iter()
                    .any(|b| b.ctx == self.run.ctx && b.kind != BlockKind::Root)
                {
                    return unsupported("a stretch of a tick ended inside a block it opened");
                }
                return Ok(());
            }
        }
    }

    fn skip(&mut self, program: &Program, op: OpId) {
        self.run.pc = self.run.pc.saturating_add(program.branch(op));
    }

    fn push(&mut self, program: &Program, op: OpId, kind: BlockKind) {
        self.run.blocks.push(Block {
            ctx: self.run.ctx,
            start: self.run.pc,
            len: program.branch(op),
            op,
            kind,
        });
    }

    /// Pops every entry of the current list the counter has reached the end
    /// of, newest first, running each end rule.
    fn close(&mut self, program: &Program) -> Result<(), Fault> {
        while let Some(&b) = self.run.blocks.last() {
            if b.ctx != self.run.ctx || b.start.saturating_add(b.len) > self.run.pc {
                break;
            }
            self.run.blocks.pop();
            self.end_rule(program, b)?;
        }
        Ok(())
    }

    fn shutdown_position(&self, program: &Program) -> Option<u32> {
        let id = self.run.placement?;
        let p = self.placements.get(id as usize)?;
        if !p.shutdown {
            return None;
        }
        program.scripts.get(p.script as usize)?.shutdown
    }

    fn end_rule(&mut self, program: &Program, b: Block) -> Result<(), Fault> {
        if let BlockKind::Receiver(saved) = b.kind {
            self.run.receiver = saved;
            return Ok(());
        }
        match &program.ops[b.op as usize] {
            Ins::PreScript => {
                if b.start + b.len == self.run.pc {
                    if let Some(at) = self.shutdown_position(program) {
                        self.run.pc = at;
                    } else if let Some(at) = self
                        .run
                        .placement
                        .and_then(|id| self.placements.get(id as usize))
                        .and_then(|p| p.behaviour)
                    {
                        self.run.pc = at;
                    }
                }
            }
            Ins::Behaviour { .. } => {
                self.run.pc = self.shutdown_position(program).unwrap_or(END);
            }
            Ins::ShutDown => {
                if let Some(id) = self.run.placement
                    && let Some(p) = self.placements.get_mut(id as usize)
                {
                    p.shutdown = false;
                    p.started = false;
                }
                self.run.pc = END;
            }
            Ins::UseMacro { .. } => {
                if let Some(call) = self.run.calls.pop() {
                    self.close_call(call.def);
                    self.run.frame = call.caller;
                }
            }
            Ins::Define(_) => self.ret(program)?,
            _ => {}
        }
        Ok(())
    }

    /// A macro body's end: back to the caller, right after its INTERFACE.
    fn ret(&mut self, program: &Program) -> Result<(), Fault> {
        let Some(&top) = self.run.blocks.last() else {
            return unsupported("a macro body ended with no call open");
        };
        if top.kind == BlockKind::Root {
            self.run.blocks.pop();
            if let Some(call) = self.run.calls.pop() {
                if let Some(c) = self.capture.as_mut()
                    && c.def == call.def
                {
                    let names = c.names.clone();
                    let values = names
                        .iter()
                        .map(|n| {
                            let cell = (*n).and_then(|n| self.frames.get(&call.def)?.get(&n))?;
                            Some(self.cells[*cell as usize].value)
                        })
                        .collect();
                    if let Some(c) = self.capture.as_mut() {
                        c.values = values;
                    }
                }
                self.close_call(call.def);
            }
            self.run.pc = END;
            self.run.ctx = u32::MAX;
            return Ok(());
        }
        if !matches!(program.ops[top.op as usize], Ins::UseMacro { .. }) {
            return unsupported("a macro body ended inside something other than its call");
        }
        self.run.ctx = top.ctx;
        let iface = program
            .scripts
            .get(top.ctx as usize)
            .and_then(|s| s.ops.get(top.start as usize))
            .map_or(0, |&o| program.branch(o));
        self.run.pc = top.start + iface + 1;
        let Some(call) = self.run.calls.last_mut() else {
            return unsupported("a macro body ended with no call open");
        };
        call.returned = true;
        self.run.frame = call.caller;
        Ok(())
    }

    /// The use block closed: the callee's frame back to defaults, no longer
    /// busy.
    fn close_call(&mut self, def: DefId) {
        self.busy.remove(&def);
        let cells: Vec<u32> = self
            .frames
            .get(&def)
            .map(|f| f.values().copied().collect())
            .unwrap_or_default();
        for c in cells {
            self.reset_cell(c, 0);
        }
    }

    /// Enters `def`'s body: its frame current, its INTERFACE skipped.
    fn enter(&mut self, program: &Program, def: DefId) -> Result<(), Fault> {
        let Some(d) = program.defs.get(def as usize) else {
            return unsupported("a call to no definition");
        };
        let Some((sid, pos)) = d.placed else {
            return unsupported(format!("a call to {:?}, which sits in no list", d.display));
        };
        self.run.ctx = sid;
        self.run.frame = Some(def);
        self.run.pc = pos + 1;
        self.run.blocks.push(Block {
            ctx: sid,
            start: pos + 1,
            len: program.branch(d.op),
            op: d.op,
            kind: BlockKind::Plain,
        });
        if let Some(&iface) = program.scripts[sid as usize].ops.get(self.run.pc as usize)
            && program.ops[iface as usize] == Ins::Interface
        {
            self.run.pc += program.branch(iface) + 1;
        }
        Ok(())
    }

    fn placement_mut(&mut self) -> Option<&mut Placement> {
        let id = self.run.placement?;
        self.placements.get_mut(id as usize)
    }

    fn exec(&mut self, program: &Program, op: OpId, host: &mut dyn Host) -> Result<(), Fault> {
        match &program.ops[op as usize] {
            Ins::PreScript => self.push(program, op, BlockKind::Plain),
            Ins::StartUp => {
                let first = program
                    .scripts
                    .get(self.run.ctx as usize)
                    .and_then(|s| s.first_behaviour);
                match self.placement_mut() {
                    // Its declared locals join its list once this first
                    // tick is done ([`Vm::tick`]).
                    Some(p) if !p.started => {
                        p.started = true;
                        p.behaviour = first;
                    }
                    _ => self.skip(program, op),
                }
            }
            Ins::ShutDown => {
                if self.placement_mut().is_some_and(|p| p.shutdown) {
                    self.push(program, op, BlockKind::Plain);
                } else {
                    self.skip(program, op);
                }
            }
            Ins::Section | Ins::Define(_) => self.skip(program, op),
            Ins::Behaviour { .. } => self.push(program, op, BlockKind::Plain),
            Ins::Continue => self.close(program)?,
            Ins::Skip | Ins::End => {
                self.skip(program, op);
                self.close(program)?;
            }
            Ins::Loop => self.flow_loop(program)?,
            Ins::EnterMacro => {
                let Some(call) = self.run.calls.last().copied() else {
                    return unsupported("flow macro with no call open");
                };
                self.enter(program, call.def)?;
            }
            Ins::Interface | Ins::Variable { .. } => {}
            Ins::Parameter(p) => self.bind(program, *p, host)?,
            Ins::UseMacro { np, index } => self.use_macro(program, op, *np, *index, host)?,
            Ins::SetValue { lhs, rhs } => {
                let spot = self.path(program, *lhs, host)?;
                let v = self.rhs(program, *rhs, host, 0)?;
                self.write(spot, v, host)?;
            }
            Ins::SetReference { lhs, rhs, index } => {
                let spot = self.path(program, *lhs, host)?;
                let v = self.rhs_ref(program, *rhs, *index, host)?;
                self.write(spot, v, host)?;
            }
            Ins::Inc { lhs } | Ins::Dec { lhs } => {
                let up = matches!(program.ops[op as usize], Ins::Inc { .. });
                let spot = self.path(program, *lhs, host)?;
                let v = match self.read(spot, host)? {
                    Value::Int(i) => Value::Int(if up {
                        i.wrapping_add(1)
                    } else {
                        i.wrapping_sub(1)
                    }),
                    Value::Float(f) => Value::Float(if up { f + 1.0 } else { f - 1.0 }),
                    Value::Double(d) => Value::Double(if up { d + 1.0 } else { d - 1.0 }),
                    Value::Null => return Err(Fault::Nothing),
                    other => return unsupported(format!("inc or dec of {other:?}")),
                };
                self.write(spot, v, host)?;
            }
            Ins::Reset { np } => {
                let spot = self.path(program, *np, host)?;
                self.reset(spot, host)?;
            }
            Ins::CheckValue { lhs, relation, rhs } => {
                let l = self
                    .path(program, *lhs, host)
                    .and_then(|s| self.read(s, host));
                let l = match l {
                    Ok(v) => self.deref_with(v, host)?,
                    Err(Fault::Nothing) => return Err(Fault::Nothing),
                    Err(f) => return Err(f),
                };
                // As a path element the check reads what its left side read.
                self.op_state.entry(op).or_default().cached = Some(l);
                let r = self.rhs(program, *rhs, host, 0)?;
                if compare(*relation, l, r) != Some(true) {
                    self.skip(program, op);
                }
            }
            Ins::CheckReference {
                lhs,
                relation,
                rhs,
                index,
            } => {
                let spot = self.path(program, *lhs, host)?;
                let l = self.read_ref(spot, host)?;
                let r = self.rhs_ref(program, *rhs, *index, host)?;
                self.op_state.entry(op).or_default().cached = Some(l);
                let holds = match relation {
                    1 => self.same(l, r),
                    5 => !self.same(l, r),
                    _ => false,
                };
                if !holds {
                    self.skip(program, op);
                }
            }
            Ins::CheckMembership { lhs, code, rhs } => {
                let spot = self.path(program, *lhs, host)?;
                // `undefined` holds no set: a check of it skips.
                if spot == Spot::Undefined {
                    return Err(Fault::Nothing);
                }
                let set = self.read(spot, host)?;
                let Value::Set(s) = self.materialize(set) else {
                    return if set == Value::Null {
                        Err(Fault::Nothing)
                    } else {
                        unsupported(format!("membership of {set:?}"))
                    };
                };
                let r = self.rhs_ref(program, *rhs, None, host)?;
                self.materialize(r);
                if !self.membership(s, r, *code) {
                    self.skip(program, op);
                }
            }
            Ins::ChangeMembership { lhs, code, rhs } => {
                let spot = self.path(program, *lhs, host)?;
                // Into `undefined` the change goes nowhere, as its writes do.
                if spot == Spot::Undefined {
                    return Err(Fault::Nothing);
                }
                let set = self.read(spot, host)?;
                let Value::Set(s) = self.materialize(set) else {
                    return if set == Value::Null {
                        Err(Fault::Nothing)
                    } else {
                        unsupported(format!("set combine into {set:?}"))
                    };
                };
                let r = self.rhs_ref(program, *rhs, None, host)?;
                self.combine(s, r, *code)?;
            }
            Ins::LoopValue { rhs } => {
                let n = self.rhs(program, *rhs, host, 0)?;
                let Some(n) = n.as_i32() else {
                    return unsupported(format!("loop count {n:?}"));
                };
                if n <= 0 {
                    self.skip(program, op);
                } else {
                    let st = self.op_state.entry(op).or_default();
                    st.index = 0;
                    st.count = n;
                    self.push(program, op, BlockKind::Plain);
                }
            }
            Ins::ForEach { lhs, dir, rhs } => self.for_each(program, op, *lhs, *dir, *rhs, host)?,
            Ins::FindSubSet { lhs, relation, rhs } => {
                if self.find_subset(program, op, *lhs, *relation, *rhs, host)? {
                    self.push(program, op, BlockKind::Plain);
                } else {
                    self.skip(program, op);
                }
            }
            Ins::SetBehavior { np, index } => self.set_behavior(program, *np, *index, host)?,
            Ins::FindVariable {
                name,
                decl,
                filter,
                owner,
                index,
                skip,
            } => {
                if self.find_variable(*name, *decl, *filter, *owner, *index, *skip, op, host)? {
                    self.push(program, op, BlockKind::Plain);
                } else {
                    self.skip(program, op);
                }
            }
            Ins::World { operands, words } => self.world(program, op, operands, words, host)?,
        }
        Ok(())
    }

    /// `find variable`: whether its block runs. The variable is the op's
    /// result as a path element.
    #[allow(clippy::too_many_arguments)]
    fn find_variable(
        &mut self,
        name: Sym,
        decl: Decl,
        filter: u32,
        owner: Option<OperandId>,
        index: Option<OperandId>,
        skip: Option<OperandId>,
        op: OpId,
        host: &mut dyn Host,
    ) -> Result<bool, Fault> {
        if skip.is_some() {
            return unsupported("find variable with a skip operand");
        }
        let program = Arc::clone(&self.program);
        let owner = match self.rhs_ref(&program, owner, index, host) {
            Ok(v) => v,
            Err(Fault::Nothing) => return Ok(false),
            Err(f) => return Err(f),
        };
        let owner = self.materialize(owner);
        let owner = self.canon(owner);
        if owner == Value::Null {
            return Ok(false);
        }
        if let Value::Entity(h) = owner
            && let Some(&pid) = self.handles.get(&h)
        {
            let cell = self.find_local(pid, name, decl, filter)?;
            let st = self.op_state.entry(op).or_default();
            st.cached = cell.map(Value::Cell);
            return Ok(cell.is_some());
        }
        let key = (recv(owner), name, decl);
        // A placement of a loaded file with no script (a breakable) has its
        // tags from the start too, as a running placement's are its locals:
        // the loot system's init reads each breakable's `archetype ref`.
        let file_obj = match owner {
            Value::Entity(h) => self.named.get(&h).copied(),
            Value::Object(o) => Some(o),
            _ => None,
        };
        if !self.found.contains_key(&key)
            && filter <= 3
            && let Some(obj) = file_obj
            && self.tag(obj, name, decl).is_some()
        {
            let c = self.tag_cell(obj, name, decl)?;
            self.found.insert(key, c);
        }
        let cell = match (filter, self.found.get(&key).copied()) {
            (0 | 1, found) => found,
            (2, Some(c)) => Some(c),
            (2, None) => {
                let c = self.new_cell(decl, None, 0)?;
                self.found.insert(key, c);
                Some(c)
            }
            (3, found) => {
                self.found.remove(&key);
                found
            }
            (f, _) => return unsupported(format!("find variable filter {f}")),
        };
        let st = self.op_state.entry(op).or_default();
        st.cached = cell.map(Value::Cell);
        Ok(cell.is_some())
    }

    fn flow_loop(&mut self, program: &Program) -> Result<(), Fault> {
        let Some(&b) = self.run.blocks.last() else {
            return unsupported("flow with no block open");
        };
        self.run.pc = b.start;
        let past = b.start.saturating_add(b.len);
        match &program.ops[b.op as usize] {
            Ins::LoopValue { .. } => {
                let st = self.op_state.entry(b.op).or_default();
                st.index += 1;
                if st.index >= st.count {
                    self.run.pc = past;
                }
            }
            Ins::ForEach { dir, .. } => {
                let st = self.op_state.entry(b.op).or_default();
                let done = if *dir == 1 {
                    st.index -= 1;
                    st.index < 0
                } else {
                    st.index += 1;
                    st.index as usize >= st.items.len()
                };
                if done {
                    self.run.pc = past;
                }
            }
            _ => {}
        }
        self.close(program)
    }

    fn use_macro(
        &mut self,
        program: &Program,
        op: OpId,
        np: Option<OperandId>,
        index: Option<OperandId>,
        host: &mut dyn Host,
    ) -> Result<(), Fault> {
        let target = match self.rhs_ref(program, np, index, host) {
            Ok(v) => v,
            Err(Fault::Nothing) => Value::Null,
            Err(f) => return Err(f),
        };
        let def = match target {
            Value::Object(o) => program.def_of(o).and_then(|d| program.resolve(d)),
            _ => None,
        };
        let Some(def) = def.filter(|d| program.defs[*d as usize].kind != DefKind::Structure) else {
            let what = match target {
                Value::Object(o) => match program.def_of(o) {
                    Some(d) => format!(
                        "{:?}, a stub no list defines,",
                        program.defs[d as usize].display
                    ),
                    None => format!("{:?}", program.name_of(o).unwrap_or("an object")),
                },
                other => format!("{other:?}"),
            };
            self.note(format!("use macro: {what} names no macro; block skipped"));
            self.skip(program, op);
            return Ok(());
        };
        if !self.busy.insert(def) {
            self.note(format!(
                "use macro: {:?} is running; block skipped",
                program.defs[def as usize].display
            ));
            self.skip(program, op);
            return Ok(());
        }
        self.run.calls.push(Call {
            def,
            caller: self.run.frame,
            returned: false,
        });
        *self.calls.entry(def).or_default() += 1;
        *self
            .call_sites
            .entry((self.run.ctx, self.run.at))
            .or_default() += 1;
        self.push(program, op, BlockKind::Plain);
        Ok(())
    }

    /// A call's parameter: its right side in the caller's context, into the
    /// callee's frame.
    fn bind(&mut self, program: &Program, p: Param, host: &mut dyn Host) -> Result<(), Fault> {
        let Some(call) = self.run.calls.last().copied() else {
            return unsupported("a parameter outside a call");
        };
        let name = param_name(program, &self.frames, call.def, &p);
        let cell = self.frame_cell(call.def, name, p.decl)?;
        let Some(rhs) = p.rhs else {
            return Ok(());
        };
        let kind = self.cells[cell as usize].kind;
        match kind {
            CellKind::Set => {
                let v = self.rhs_ref(program, Some(rhs), None, host)?;
                let Value::Set(dst) = self.cells[cell as usize].value else {
                    return unsupported("a set parameter with no set");
                };
                self.combine(dst, v, p.combine)?;
            }
            CellKind::Value => {
                let v = self.rhs(program, Some(rhs), host, 0)?;
                self.reset_cell(cell, 0);
                self.write_cell(cell, v, 0);
            }
            _ => {
                let v = self.rhs_ref(program, Some(rhs), p.index, host)?;
                self.reset_cell(cell, 0);
                self.write_cell(cell, v, 0);
            }
        }
        Ok(())
    }

    fn set_behavior(
        &mut self,
        program: &Program,
        np: Option<OperandId>,
        index: Option<OperandId>,
        host: &mut dyn Host,
    ) -> Result<(), Fault> {
        let target = self.rhs_ref(program, np, index, host)?;
        let Value::Object(o) = target else {
            return unsupported(format!("set behavior to {target:?}"));
        };
        let name = match program.op_of(o).map(|op| &program.ops[op as usize]) {
            Some(Ins::Behaviour { name }) => *name,
            _ => return unsupported("set behavior to something that isn't a behaviour"),
        };
        let Some(id) = self.run.placement else {
            return unsupported("set behavior outside a placement");
        };
        let script = self.placements[id as usize].script;
        match program.scripts[script as usize].behaviours.get(&name) {
            Some(&pos) => self.placements[id as usize].behaviour = Some(pos),
            None => self.note(format!(
                "set behavior: {:?} isn't in this placement's script",
                program.sym(name)
            )),
        }
        Ok(())
    }

    fn for_each(
        &mut self,
        program: &Program,
        op: OpId,
        lhs: Option<OperandId>,
        dir: u32,
        rhs: Option<OperandId>,
        host: &mut dyn Host,
    ) -> Result<(), Fault> {
        let spot = self.path(program, lhs, host)?;
        let set = self.read(spot, host)?;
        let Value::Set(s) = self.materialize(set) else {
            return if set == Value::Null {
                Err(Fault::Nothing)
            } else {
                unsupported(format!("for each over {set:?}"))
            };
        };
        let elems = self.sets[s as usize].elems.clone();
        if elems.is_empty() {
            self.skip(program, op);
            return Ok(());
        }
        let offset = match rhs {
            Some(_) => self.rhs(program, rhs, host, 0)?.as_i32().unwrap_or(0),
            None => 0,
        };
        let n = elems.len();
        let k = offset.rem_euclid(n as i32) as usize;
        let mut items: Vec<Value> = elems[k..].iter().chain(&elems[..k]).copied().collect();
        if dir == 2 {
            // The original orders by generator keys; the order itself isn't
            // modelled yet, only that every member comes once.
            for i in (1..items.len()).rev() {
                let j = host.draw() as usize % (i + 1);
                items.swap(i, j);
            }
        }
        let st = self.op_state.entry(op).or_default();
        st.index = if dir == 1 { n as i32 - 1 } else { 0 };
        st.items = items;
        self.push(program, op, BlockKind::Plain);
        Ok(())
    }

    /// `find subset`: whether any member matched.
    fn find_subset(
        &mut self,
        program: &Program,
        op: OpId,
        lhs: Option<OperandId>,
        relation: u32,
        rhs: Option<OperandId>,
        host: &mut dyn Host,
    ) -> Result<bool, Fault> {
        let Some(Operand::Path { elems, other, .. }) =
            lhs.and_then(|l| program.operands.get(l as usize))
        else {
            return unsupported("find subset without a path");
        };
        let Some(slot) = elems.iter().position(|e| *e == Elem::Null) else {
            return unsupported("find subset's path has no empty slot");
        };
        let set = self.walk(program, &elems[..slot], host)?;
        // `undefined` holds no set: nothing matches (AH.19: the enemy
        // system's way round obstacles searches it).
        if set == Spot::Undefined {
            return Err(Fault::Nothing);
        }
        let set = self.read(set, host)?;
        let Value::Set(s) = self.materialize(set) else {
            return if set == Value::Null {
                Err(Fault::Nothing)
            } else {
                unsupported(format!("find subset over {set:?}"))
            };
        };
        let names_its = rhs.is_some_and(|r| reads_its(program, r));
        let me = self.run.receiver_placement(&self.placements);
        let members = self.sets[s as usize].elems.clone();
        let saved_its = self.run.its;
        let mut once = None;
        let mut hits: Vec<(f32, usize, Value)> = Vec::new();
        for (i, &m) in members.iter().enumerate() {
            if Some(m) == me {
                continue;
            }
            self.run.its = m;
            let mut spot = Spot::Const(m);
            let mut ok = true;
            // The path's other point, what a measurement on it is taken
            // from, read with this member under test.
            let saved_other = self.run.other_point;
            if other.is_some() {
                match self.rhs_ref(program, *other, None, host) {
                    Ok(p) => self.run.other_point = Some(p),
                    Err(Fault::Nothing) => ok = false,
                    Err(f) => {
                        self.run.its = saved_its;
                        return Err(f);
                    }
                }
            }
            for &e in &elems[slot + 1..] {
                if !ok {
                    break;
                }
                match self.next_spot(program, spot, e, host) {
                    Ok(next) => spot = next,
                    Err(Fault::Nothing) => {
                        ok = false;
                        break;
                    }
                    Err(f) => {
                        self.run.its = saved_its;
                        self.run.other_point = saved_other;
                        return Err(f);
                    }
                }
            }
            self.run.other_point = saved_other;
            if !ok {
                continue;
            }
            let v = match self.read(spot, host).and_then(|v| self.deref_with(v, host)) {
                Ok(v) => v,
                Err(Fault::Nothing) => continue,
                Err(f) => {
                    self.run.its = saved_its;
                    return Err(f);
                }
            };
            let r = match (names_its, once) {
                (false, Some(r)) => r,
                _ => {
                    let r = self.rhs(program, rhs, host, 0)?;
                    once = Some(r);
                    r
                }
            };
            if compare(relation, v, r) == Some(true) {
                hits.push((v.as_f32().unwrap_or(0.0), i, m));
            }
        }
        self.run.its = saved_its;
        hits.sort_by(|a, b| {
            a.0.partial_cmp(&b.0)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then(a.1.cmp(&b.1))
        });
        let st_set = self.op_state.get(&op).and_then(|s| s.set);
        let set = match st_set {
            Some(s) => s,
            None => {
                self.sets.push(SetObj::default());
                (self.sets.len() - 1) as u32
            }
        };
        self.sets[set as usize].elems = hits.iter().map(|h| h.2).collect();
        let st = self.op_state.entry(op).or_default();
        st.set = Some(set);
        st.cached = Some(Value::Set(set));
        Ok(!hits.is_empty())
    }

    fn world(
        &mut self,
        program: &Program,
        op: OpId,
        operands: &[(&'static str, Option<OperandId>)],
        words: &[(&'static str, u32)],
        host: &mut dyn Host,
    ) -> Result<(), Fault> {
        let class = program.class(op).unwrap_or(Class::Remove);
        if class == Class::SlideValue {
            return self.begin_slide(program, op, operands, host);
        }
        let mut read = Vec::with_capacity(operands.len());
        for &(name, o) in operands {
            let v = match o {
                None => None,
                // An empty stack with no literal names nothing: the op does
                // without it (a structure's clone needs no place).
                Some(id)
                    if matches!(
                        program.operands.get(id as usize),
                        Some(Operand::Path { elems, literal: None, .. }) if elems.is_empty()
                    ) =>
                {
                    None
                }
                Some(id) => Some(match program.operands.get(id as usize) {
                    Some(Operand::Path {
                        class: StackClass::RhsValue,
                        ..
                    })
                    | Some(Operand::Expr { .. }) => self.rhs(program, o, host, 0)?,
                    _ => self.rhs_ref(program, o, None, host)?,
                }),
            };
            read.push((name, v));
        }
        match class {
            // Each object a set names goes to the host on its own; a
            // placement already taken off doesn't go again.
            Class::Remove | Class::StopSequence => {
                let Some((name, Some(v))) = read.first().copied() else {
                    return Ok(());
                };
                let targets = self.targets(v);
                for t in targets {
                    if class == Class::Remove && !self.take_off(t) {
                        continue;
                    }
                    // A slider is the VM's own: stopped, its place stays
                    // where it stands.
                    if let Value::Slider(id) = t {
                        if class == Class::StopSequence
                            && let Some(s) = self.sliders.get_mut(id as usize)
                            && s.running
                        {
                            s.running = false;
                            self.running -= 1;
                        }
                        continue;
                    }
                    let call = OpCall {
                        placement: self.my_handle(),
                        receiver: self.run.receiver,
                        class,
                        operands: vec![(name, Some(t))],
                        words: words.to_vec(),
                        sets: Vec::new(),
                    };
                    match host.op(&call) {
                        Ok(_) | Err(HostFault::Nothing) => {}
                        Err(HostFault::Unmodelled) => return Err(Fault::Op(class)),
                    }
                }
                return Ok(());
            }
            // A clone of the VM's own: a structure or a set is copied here,
            // and the block runs with the copy as its receiver.
            Class::Spawn => {
                let template = read.first().and_then(|(_, v)| *v).map(|v| self.peek(v));
                let copy = match template {
                    Some(Value::Struct(s)) => Some(Value::Struct(self.clone_struct(s, 0)?)),
                    Some(Value::Set(s)) => Some(Value::Set(self.clone_set(s, 0)?)),
                    _ => None,
                };
                if let Some(c) = copy {
                    self.op_state.entry(op).or_default().cached = Some(c);
                    let saved = self.run.receiver;
                    self.push(program, op, BlockKind::Receiver(saved));
                    self.run.receiver = c;
                    return Ok(());
                }
            }
            Class::Control => {
                // A placement taken off can't be told again.
                if let Some((_, Some(Value::Entity(h)))) = read.first()
                    && self
                        .handles
                        .get(h)
                        .is_some_and(|&id| self.placements[id as usize].removed)
                {
                    self.skip(program, op);
                    return Ok(());
                }
            }
            _ => {}
        }
        let sets = read
            .iter()
            .filter_map(|&(name, v)| match self.peek(v?) {
                Value::Set(s) => Some((name, self.targets(Value::Set(s)))),
                _ => None,
            })
            .collect();
        let call = OpCall {
            placement: self.my_handle(),
            receiver: self.run.receiver,
            class,
            operands: read,
            words: words.to_vec(),
            sets,
        };
        let answer = match host.op(&call) {
            Ok(a) => a,
            Err(HostFault::Nothing) => return Err(Fault::Nothing),
            Err(HostFault::Unmodelled) => return Err(Fault::Op(class)),
        };
        let branch = class.opens_block();
        match answer {
            Answer::Run { receiver, result } => {
                if let Some(r) = result {
                    self.op_state.entry(op).or_default().cached = Some(r);
                }
                if branch {
                    match receiver {
                        Some(r) => {
                            let saved = self.run.receiver;
                            self.push(program, op, BlockKind::Receiver(saved));
                            self.run.receiver = r;
                        }
                        None => self.push(program, op, BlockKind::Plain),
                    }
                }
            }
            Answer::Done | Answer::Skip => {
                if branch {
                    self.skip(program, op);
                }
            }
            Answer::Found(found) => {
                if found.is_empty() {
                    if branch {
                        self.skip(program, op);
                    }
                } else {
                    self.sets.push(SetObj {
                        elems: found,
                        elem: None,
                        next: 0,
                    });
                    let set = (self.sets.len() - 1) as u32;
                    self.op_state.entry(op).or_default().cached = Some(Value::Set(set));
                    if branch {
                        self.push(program, op, BlockKind::Plain);
                    }
                }
            }
        }
        Ok(())
    }

    /// The tag `name` `obj`'s file gives it, when its declaration holds
    /// what `decl`'s container does.
    fn tag(&self, obj: Obj, name: Sym, decl: Decl) -> Option<Tag> {
        self.program
            .tags
            .get(&obj)?
            .iter()
            .find(|t| t.name == name)
            .copied()
            .filter(|t| cell_kind(t.decl.container) == cell_kind(decl.container))
    }

    /// A new cell holding the tag `name` `obj`'s file gives it ([`Vm::tag`]).
    fn tag_cell(&mut self, obj: Obj, name: Sym, decl: Decl) -> Result<u32, Fault> {
        let Some(tag) = self.tag(obj, name, decl) else {
            return unsupported("no such tag");
        };
        let value = match tag.value {
            None => Value::Int(0),
            Some(o) => match tag_value(&self.program, o) {
                Some(v) => v,
                // A reference tag names an object of its file (a
                // breakable's `archetype ref`, AH.20): that object.
                None => match tag_object(&self.program, o) {
                    Some(obj) => self.canon(Value::Object(obj)),
                    // A tag naming a file variable holds what that variable
                    // does: a set reference tag, the file's set itself
                    // (openspec AG.60: a Titan boulder's pebbles).
                    None => match tag_global(&self.program, o) {
                        Some(g) => {
                            let c = self.global_cell(g)?;
                            self.cells[c as usize].value
                        }
                        None => return unsupported("a tag whose value isn't a literal"),
                    },
                },
            },
        };
        let c = self.new_cell(decl, None, 0)?;
        self.write_cell(c, value, 0);
        Ok(c)
    }

    /// `find variable` on a placement the VM runs: among its locals, the
    /// ones its script declares made as they're first reached.
    fn find_local(
        &mut self,
        pid: PlacementId,
        name: Sym,
        decl: Decl,
        filter: u32,
    ) -> Result<Option<u32>, Fault> {
        let p = &self.placements[pid as usize];
        let mut existing = p
            .locals
            .get(&name)
            .copied()
            .filter(|&c| Some(self.cells[c as usize].kind) == cell_kind(decl.container));
        // A tag its file gives the placement is one of its variables from
        // the start, holding the tag's value.
        let is_tag = p.obj.is_some_and(|o| self.tag(o, name, decl).is_some());
        if existing.is_none()
            && is_tag
            && let Some(obj) = p.obj
        {
            let c = self.tag_cell(obj, name, decl)?;
            self.placements[pid as usize].locals.insert(name, c);
            existing = Some(c);
        }
        let p = &self.placements[pid as usize];
        // A structure type named through a stub is the structure it
        // resolves to.
        let same = |a: &Decl, b: &Decl| {
            let def = |c: Contents| match c {
                Contents::Def(d) => Contents::Def(self.program.resolve(d).unwrap_or(d)),
                other => other,
            };
            a.container == b.container && def(a.contents) == def(b.contents)
        };
        let declared = self
            .program
            .scripts
            .get(p.script as usize)
            .and_then(|s| s.declared.get(&name))
            .is_some_and(|d| same(d, &decl));
        let listed = p.listed;
        let make = |vm: &mut Self| -> Result<u32, Fault> {
            let c = vm.new_cell(decl, None, 0)?;
            let p = &mut vm.placements[pid as usize];
            p.locals.insert(name, c);
            p.listed = true;
            Ok(c)
        };
        Ok(match filter {
            // Filter 0 asks the owner for its variable list, made if it has
            // none yet: a declared local is there before the placement's
            // own STARTUP has run (the portal manager's STARTUP finds the
            // portal control's `portal data` so, ticking before it).
            0 => {
                self.placements[pid as usize].listed = true;
                match existing {
                    Some(c) => Some(c),
                    None if declared => Some(make(self)?),
                    None => None,
                }
            }
            // Filter 1 finds in a list the owner already has: its tags from
            // the start, its declared locals once its STARTUP's tick is done
            // (AH.18: a Chompy's own STARTUP finds `communicate range` so
            // and gets its script's default, 10, as the live game holds it,
            // where its script declares the local).
            1 => match existing {
                Some(c) if listed || is_tag => Some(c),
                None if listed && declared => Some(make(self)?),
                _ => None,
            },
            2 => match existing {
                Some(c) => Some(c),
                None => Some(make(self)?),
            },
            3 => {
                let found = match existing {
                    Some(c) if listed => Some(c),
                    None if listed && declared => Some(make(self)?),
                    _ => None,
                };
                if found.is_some() {
                    self.placements[pid as usize].locals.remove(&name);
                }
                found
            }
            f => return unsupported(format!("find variable filter {f}")),
        })
    }

    /// `slide value` (crate docs, Slides): its place eased from what it
    /// holds now to its target over its seconds by a slider of the pool,
    /// the op's result, its block run; with every slider running, a time
    /// that isn't over 0 or values that don't slide (not numbers, colours
    /// or screen positions alike), it lands on its target at once and
    /// skips its block, as the original does when its pool is full
    /// (aj-vm).
    fn begin_slide(
        &mut self,
        program: &Program,
        op: OpId,
        operands: &[(&'static str, Option<OperandId>)],
        host: &mut dyn Host,
    ) -> Result<(), Fault> {
        let operand = |name: &str| {
            operands
                .iter()
                .find(|(n, _)| *n == name)
                .and_then(|(_, o)| *o)
        };
        let to = self.rhs(program, operand("_RHS"), host, 0)?;
        let mut number = |vm: &mut Self, name: &str| match vm.rhs(program, operand(name), host, 0) {
            Ok(v) => Ok(v.as_f32().unwrap_or(0.0)),
            Err(Fault::Nothing) => Ok(0.0),
            Err(f) => Err(f),
        };
        let time = number(self, "_secondsRHS")?;
        let out = number(self, "_easeOutRHS")? * 0.02;
        let in_ = number(self, "_easeInRHS")? * 0.02;
        let spot = self.path(program, operand("_LHS"), host)?;
        let from = self.read(spot, host)?;
        let from = self.deref_with(from, host)?;
        let slides = time.is_finite()
            && time > 0.0
            && self.running < SLIDERS
            && between(from, to, 0.5).is_some()
            && !matches!(spot, Spot::Const(_) | Spot::Undefined);
        if !slides {
            self.write(spot, to, host)?;
            self.skip(program, op);
            return Ok(());
        }
        // A slide begun on a place one already runs on takes over from it.
        self.end_slides_on(spot);
        self.sliders.push(Slider {
            spot,
            placement: self.run.placement,
            ctx: self.run.ctx,
            receiver: self.run.receiver,
            from,
            to,
            time,
            out,
            in_,
            elapsed: 0.0,
            rate: 100.0,
            looping: false,
            ping_pong: false,
            ignores_time_scales: false,
            back: false,
            // Begun ahead of this frame's slides (a native call before
            // them), it belongs to the frame they're about to begin.
            begun: if self.slid {
                self.frame
            } else {
                self.frame + 1
            },
            running: true,
        });
        self.running += 1;
        let id = (self.sliders.len() - 1) as u32;
        self.live.push(id);
        self.op_state.entry(op).or_default().cached = Some(Value::Slider(id));
        self.push(program, op, BlockKind::Plain);
        Ok(())
    }

    /// Ends every running slide on `spot`: a script's own write to a place
    /// a slide is easing takes it over (AH.19's reading of AH.10's pushes:
    /// his hurt reaction writes his deceleration back to its default while
    /// the push's slide eases it, and the default holds).
    fn end_slides_on(&mut self, spot: Spot) {
        if self.running == 0 {
            return;
        }
        let key = |s: Spot| match s {
            Spot::Member {
                receiver,
                member,
                cell,
                ..
            } => Spot::Member {
                receiver,
                member,
                cell,
                other: None,
            },
            other => other,
        };
        let k = key(spot);
        for &i in &self.live {
            let s = &mut self.sliders[i as usize];
            if s.running && key(s.spot) == k {
                s.running = false;
                self.running -= 1;
            }
        }
        let sliders = &self.sliders;
        self.live.retain(|&i| sliders[i as usize].running);
    }

    /// A slider's `sequence` members and `slider.ignores time scales`
    /// ([`SliderPart`]); any other member of one is unmodelled.
    fn slider_member(&mut self, id: u32, m: MemberId) -> Result<Spot, Fault> {
        if self.sliders.get(id as usize).is_none() {
            return Err(Fault::Nothing);
        }
        match SliderPart::of(m) {
            Some(part) => {
                self.uses.entry(m).or_default().0 += 1;
                Ok(Spot::Slider { id, part })
            }
            None => Err(Fault::Member(m)),
        }
    }

    /// The objects a world op acts on: a set's members one by one, or the
    /// one named, each a placement's handle where it names one.
    fn targets(&self, v: Value) -> Vec<Value> {
        let all = match self.peek(v) {
            Value::Set(s) => self.sets[s as usize].elems.clone(),
            other => vec![other],
        };
        all.into_iter()
            .map(|t| self.canon(self.peek(t)))
            .filter(|t| *t != Value::Null)
            .collect()
    }

    /// `remove` on one object: a placement the VM runs is taken off, its
    /// script stopped for good. False for one already off, which stays as
    /// it is.
    fn take_off(&mut self, t: Value) -> bool {
        if let Value::Entity(h) = t
            && let Some(&id) = self.handles.get(&h)
        {
            let p = &mut self.placements[id as usize];
            if p.removed {
                return false;
            }
            p.removed = true;
            p.started = false;
            p.behaviour = None;
            p.shutdown = false;
        }
        true
    }

    /// A file object that names a placement, as that placement's handle; a
    /// definition stub, as the definition it resolves to (crate docs,
    /// Programs and placements), so two files naming one macro name one
    /// object (AH.19: Eruptor's file names `State - On Fire` through a stub,
    /// and the Chompy's state list holds `global.bld`'s definition).
    fn canon(&self, v: Value) -> Value {
        match v {
            Value::Object(o) => {
                if let Some(&h) = self.objects.get(&o) {
                    return Value::Entity(h);
                }
                match self.program.def_of(o) {
                    Some(d) => match self.program.resolve(d) {
                        Some(r) if r != d => Value::Object(self.program.defs[r as usize].obj),
                        _ => v,
                    },
                    None => v,
                }
            }
            other => other,
        }
    }

    // ---- paths ----

    fn my_handle(&self) -> u64 {
        self.run
            .placement
            .and_then(|id| self.placements.get(id as usize))
            .map_or(0, |p| p.handle)
    }

    fn note(&mut self, text: String) {
        let placement = self
            .run
            .placement
            .and_then(|id| self.placements.get(id as usize))
            .map(|p| p.handle);
        self.log.push(LogLine { placement, text });
    }

    /// The spot an operand's path ends at.
    fn path(
        &mut self,
        program: &Program,
        operand: Option<OperandId>,
        host: &mut dyn Host,
    ) -> Result<Spot, Fault> {
        let Some(o) = operand else {
            return Err(Fault::Nothing);
        };
        match program.operands.get(o as usize) {
            Some(Operand::Path { elems, other, .. }) => {
                let saved = self.run.other_point;
                if other.is_some() {
                    self.run.other_point = Some(self.rhs_ref(program, *other, None, host)?);
                }
                let r = self.walk(program, elems, host);
                self.run.other_point = saved;
                r
            }
            Some(Operand::Expr { .. }) => unsupported("an expression where a place belongs"),
            None => unsupported("an operand past the program"),
        }
    }

    fn walk(
        &mut self,
        program: &Program,
        elems: &[Elem],
        host: &mut dyn Host,
    ) -> Result<Spot, Fault> {
        let Some((&head, rest)) = elems.split_first() else {
            return Err(Fault::Nothing);
        };
        let mut spot = self.head(program, head)?;
        for &e in rest {
            spot = self.next_spot(program, spot, e, host)?;
        }
        Ok(spot)
    }

    fn head(&mut self, program: &Program, e: Elem) -> Result<Spot, Fault> {
        match e {
            Elem::Null => Err(Fault::Nothing),
            Elem::Op(op) => match &program.ops[op as usize] {
                Ins::Variable { name, decl } => {
                    let c = match self.run.frame {
                        Some(def) => self.frame_cell(def, *name, *decl)?,
                        None => self.local_cell(*name, *decl)?,
                    };
                    Ok(Spot::Cell(c))
                }
                Ins::Parameter(p) => {
                    let def = self.param_frame(program, op, p);
                    let c = match def {
                        Some(def) => {
                            let name = param_name(program, &self.frames, def, p);
                            self.frame_cell(def, name, p.decl)?
                        }
                        None => self.local_cell(p.name, p.decl)?,
                    };
                    Ok(Spot::Cell(c))
                }
                Ins::LoopValue { .. } => Ok(Spot::Const(Value::Int(
                    self.op_state.get(&op).map_or(0, |s| s.index),
                ))),
                Ins::ForEach { .. } => Ok(Spot::Const(
                    self.op_state
                        .get(&op)
                        .and_then(|s| s.items.get(s.index.max(0) as usize).copied())
                        .unwrap_or(Value::Null),
                )),
                Ins::FindVariable { .. } => match self.op_state.get(&op).and_then(|s| s.cached) {
                    Some(Value::Cell(c)) => Ok(Spot::Cell(c)),
                    _ => Err(Fault::Nothing),
                },
                Ins::FindSubSet { .. }
                | Ins::CheckValue { .. }
                | Ins::CheckReference { .. }
                | Ins::CheckMembership { .. }
                | Ins::World { .. } => Ok(Spot::Const(
                    self.op_state
                        .get(&op)
                        .and_then(|s| s.cached)
                        .unwrap_or(Value::Null),
                )),
                Ins::Define(_) | Ins::Behaviour { .. } => Ok(Spot::Const(
                    self.canon(Value::Object(program.op_info[op as usize].obj)),
                )),
                _ => unsupported(format!(
                    "{} as a path element",
                    program.class(op).map_or("?", Class::type_name)
                )),
            },
            Elem::Global(g) => Ok(Spot::Cell(self.global_cell(g)?)),
            Elem::Object(o) => Ok(Spot::Const(self.canon(Value::Object(o)))),
            Elem::Named(a, b) => Ok(Spot::Const(Value::Named(a, b))),
            Elem::Member(m) => {
                self.reached.insert(m);
                match Builtin::of(m) {
                    Some(Builtin::My) => Ok(Spot::Const(match self.run.placement {
                        Some(_) => Value::Entity(self.my_handle()),
                        None => Value::Null,
                    })),
                    Some(Builtin::Null) => Ok(Spot::Const(Value::Null)),
                    Some(Builtin::Undefined) => Ok(Spot::Undefined),
                    Some(Builtin::Its) => Ok(Spot::Const(self.run.its)),
                    Some(Builtin::CastToInteger) => Ok(Spot::CastToInteger),
                    Some(Builtin::FrameRateScale) => {
                        Ok(Spot::Const(Value::Float(self.config.tick_seconds * 60.0)))
                    }
                    Some(Builtin::ScriptTimer) => Ok(Spot::Const(Value::Float(
                        self.run
                            .placement
                            .and_then(|id| self.placements.get(id as usize))
                            .map_or(0.0, |p| p.active),
                    ))),
                    Some(Builtin::Behaviour) => Ok(Spot::Const(self.current_behaviour())),
                    Some(b) => unsupported(format!("{} heading a path", b.name())),
                    None => self.member(None, m, Recv::Head),
                }
            }
            Elem::Unresolved(s) => unsupported(program.sym(s).to_owned()),
        }
    }

    /// The frame a parameter on a path reads. In a call's after-return code,
    /// one naming the callee's definition or its base reads the callee's.
    /// Otherwise a definition's own parameter reads the running
    /// definition's frame: in its body, while it binds a nested call's
    /// arguments and in a nested call's after-return code (a
    /// specialization's own parameters name the base). Any other reads the
    /// definition it binds into (a stub followed), or the open call's
    /// specialization of it (an after-return read of what a call returned).
    fn param_frame(&self, program: &Program, op: OpId, p: &Param) -> Option<DefId> {
        let base_of = |d: DefId| {
            program.defs[d as usize]
                .base
                .and_then(|b| program.resolve(b))
        };
        // A family shares its parameter ops, so the caller may declare the
        // one it reads back (AH.60: `Calc - Tangent` reading `Calc - Sin`).
        if let Some(top) = self.run.calls.last()
            && top.returned
            && let Some(def) = p.def.and_then(|d| program.resolve(d))
            && (top.def == def || base_of(top.def) == Some(def))
        {
            return Some(top.def);
        }
        if let Some(running) = self.run.frame
            && declares(program, running, op)
        {
            return Some(running);
        }
        let def = p.def.and_then(|d| program.resolve(d))?;
        if let Some(top) = self.run.calls.last()
            && top.def != def
            && base_of(top.def) == Some(def)
        {
            return Some(top.def);
        }
        Some(def)
    }

    fn next_spot(
        &mut self,
        program: &Program,
        prev: Spot,
        e: Elem,
        host: &mut dyn Host,
    ) -> Result<Spot, Fault> {
        match e {
            Elem::Member(m) => {
                self.reached.insert(m);
                if let Some(part) = Packed::of(m) {
                    let base = match prev {
                        Spot::Cell(c) => Base::Cell(c),
                        Spot::Member {
                            receiver,
                            member,
                            cell,
                            other,
                        } => Base::Member {
                            receiver,
                            member,
                            cell,
                            other,
                        },
                        other => match self.read(other, host)? {
                            Value::Cell(c) => Base::Cell(c),
                            Value::Null => return Err(Fault::Nothing),
                            v => Base::Const(v),
                        },
                    };
                    // A set's element names its cell.
                    let base = match base {
                        Base::Cell(c) => match self.cells[c as usize].value {
                            Value::Cell(inner) => Base::Cell(inner),
                            _ => base,
                        },
                        other => other,
                    };
                    return Ok(Spot::Packed { base, part });
                }
                match Builtin::of(m) {
                    Some(Builtin::ValueDereferencer) => self.dereferencer(prev, host),
                    Some(
                        b @ (Builtin::Count
                        | Builtin::First
                        | Builtin::Last
                        | Builtin::Random
                        | Builtin::Next),
                    ) => {
                        let v = self.read(prev, host)?;
                        let Value::Set(s) = self.materialize(v) else {
                            return if v == Value::Null {
                                Err(Fault::Nothing)
                            } else {
                                unsupported(format!("{} of {v:?}", b.name()))
                            };
                        };
                        let elems = &self.sets[s as usize].elems;
                        Ok(match b {
                            Builtin::Count => Spot::Count(s),
                            Builtin::First => {
                                Spot::Const(elems.first().copied().unwrap_or(Value::Null))
                            }
                            Builtin::Last => {
                                Spot::Const(elems.last().copied().unwrap_or(Value::Null))
                            }
                            Builtin::Next => {
                                let set = &mut self.sets[s as usize];
                                if set.elems.is_empty() {
                                    Spot::Const(Value::Null)
                                } else {
                                    let i = set.next % set.elems.len();
                                    set.next = i + 1;
                                    Spot::Const(set.elems[i])
                                }
                            }
                            _ => {
                                if elems.is_empty() {
                                    Spot::Const(Value::Null)
                                } else {
                                    let n = elems.len();
                                    let i = host.draw() as usize % n;
                                    Spot::Const(self.sets[s as usize].elems[i])
                                }
                            }
                        })
                    }
                    Some(b) => unsupported(format!("{} after a path's head", b.name())),
                    None => {
                        let key = match prev {
                            Spot::Member { cell: Some(c), .. } => Recv::ValueCell(c),
                            _ => Recv::Head,
                        };
                        let v = self.read(prev, host)?;
                        let v = self.materialize(v);
                        if v == Value::Null {
                            // A reference naming nothing still keeps its own
                            // point (crate docs, Variables, structures and
                            // sets): the reference is the receiver.
                            if let Spot::Cell(c) = prev
                                && self.cells[c as usize].kind == CellKind::Reference
                                && m == MemberId::named("placementreference", "remembered")
                            {
                                return self.member(Some(Value::Cell(c)), m, Recv::ValueCell(c));
                            }
                            return Err(Fault::Nothing);
                        }
                        if let Value::Slider(id) = v {
                            return self.slider_member(id, m);
                        }
                        let key = if key == Recv::Head { recv(v) } else { key };
                        self.member(Some(v), m, key)
                    }
                }
            }
            Elem::Op(op) => match &program.ops[op as usize] {
                Ins::Variable { name, decl } => {
                    let v = self.read(prev, host)?;
                    match self.materialize(v) {
                        Value::Struct(s) => Ok(Spot::Cell(self.field_cell(s, *name, *decl, 0)?)),
                        Value::Null => Err(Fault::Nothing),
                        other => {
                            unsupported(format!("field {:?} of {other:?}", program.sym(*name)))
                        }
                    }
                }
                _ => unsupported(format!(
                    "{} after a path's head",
                    program.class(op).map_or("?", Class::type_name)
                )),
            },
            Elem::Null => Err(Fault::Nothing),
            other => unsupported(format!("{other:?} after a path's head")),
        }
    }

    /// `valuedereferencer`: the object a reference names, as the place.
    fn dereferencer(&mut self, prev: Spot, host: &mut dyn Host) -> Result<Spot, Fault> {
        if let Spot::Cell(c) = prev
            && self.cells[c as usize].kind == CellKind::Value
        {
            return Ok(Spot::Cell(c));
        }
        match self.read(prev, host)? {
            Value::Cell(c) => Ok(Spot::Cell(c)),
            Value::Bound(b) => Ok(self.bound[b as usize]),
            Value::Object(o) => match self.program.global_of(o) {
                Some(g) => Ok(Spot::Cell(self.global_cell(g)?)),
                None => unsupported("value dereferencer on an object that holds no value"),
            },
            Value::Null => Err(Fault::Nothing),
            other => unsupported(format!("value dereferencer on {other:?}")),
        }
    }

    /// A member that isn't the VM's own.
    fn member(&mut self, receiver: Option<Value>, m: MemberId, key: Recv) -> Result<Spot, Fault> {
        let Some((spec, place)) = self.config.members.get(m) else {
            return Err(Fault::Member(m));
        };
        let cell = match place {
            Place::Host => None,
            Place::ValueCell => {
                let default = spec.default;
                let slot = *self.value_cells.entry((key, m)).or_insert_with(|| {
                    self.value_cell_values.push(default);
                    (self.value_cell_values.len() - 1) as u32
                });
                Some(slot)
            }
        };
        Ok(Spot::Member {
            receiver,
            member: m,
            cell,
            other: self.run.other_point,
        })
    }

    fn access(&self, receiver: Option<Value>, member: MemberId, other: Option<Value>) -> Access {
        Access {
            placement: self.my_handle(),
            receiver,
            member,
            other_point: other,
        }
    }

    fn read(&mut self, spot: Spot, host: &mut dyn Host) -> Result<Value, Fault> {
        Ok(match spot {
            Spot::Cell(c) => self.cells[c as usize].value,
            Spot::Const(v) => v,
            Spot::Count(s) => Value::Int(self.sets[s as usize].elems.len() as i32),
            Spot::CastToInteger => self.cast,
            Spot::Undefined => Value::Int(0),
            Spot::Slider { id, part } => {
                let s = &self.sliders[id as usize];
                let left = if s.running {
                    (s.time - s.elapsed).max(0.0)
                } else {
                    0.0
                };
                match part {
                    SliderPart::Left => Value::Float(left),
                    SliderPart::Percent => Value::Float(100.0 * left / s.time),
                    SliderPart::Rate => Value::Float(s.rate),
                    SliderPart::Mode => Value::Int(i32::from(s.ping_pong)),
                    SliderPart::Looping => Value::Int(i32::from(s.looping)),
                    SliderPart::Duration => Value::Float(s.time),
                    SliderPart::IgnoresTimeScales => Value::Int(i32::from(s.ignores_time_scales)),
                }
            }
            Spot::Member {
                member,
                cell: Some(slot),
                ..
            } => {
                self.uses.entry(member).or_default().0 += 1;
                self.value_cell_values[slot as usize]
            }
            Spot::Member {
                receiver,
                member,
                cell: None,
                other,
            } => {
                self.uses.entry(member).or_default().0 += 1;
                let at = self.access(receiver, member, other);
                if self
                    .config
                    .members
                    .get(member)
                    .is_some_and(|(s, _)| s.kind == Kind::Set)
                {
                    let elems = match host.elements(&at) {
                        Ok(e) => e,
                        Err(HostFault::Nothing) => return Err(Fault::Nothing),
                        Err(HostFault::Unmodelled) => return Err(Fault::Member(member)),
                    };
                    let key = (receiver.map_or(Recv::Head, recv), member);
                    let set = match self.engine_sets.get(&key) {
                        Some(&s) => s,
                        None => {
                            self.sets.push(SetObj::default());
                            let s = (self.sets.len() - 1) as u32;
                            self.engine_sets.insert(key, s);
                            s
                        }
                    };
                    self.sets[set as usize].elems = elems;
                    return Ok(Value::Set(set));
                }
                match host.get(&at) {
                    Ok(v) => v,
                    Err(HostFault::Nothing) => return Err(Fault::Nothing),
                    Err(HostFault::Unmodelled) => return Err(Fault::Member(member)),
                }
            }
            Spot::Packed { base, part } => {
                let whole = self.read(base.spot(), host)?;
                let whole = self.deref(whole);
                match part.word(whole) {
                    Some(w) => part.get(w),
                    None => return Err(Fault::Nothing),
                }
            }
        })
    }

    /// A read for a reference: a number variable reads as itself (the file's
    /// object, or its cell), anything else as its value.
    fn read_ref(&mut self, spot: Spot, host: &mut dyn Host) -> Result<Value, Fault> {
        // `undefined` reads 0, which as an object is none.
        if spot == Spot::Undefined {
            return Ok(Value::Null);
        }
        if let Spot::Cell(c) = spot
            && self.cells[c as usize].kind == CellKind::Value
        {
            return Ok(match self.cells[c as usize].origin {
                Some(o) => Value::Object(o),
                None => Value::Cell(c),
            });
        }
        // A part of a packed value the VM keeps, or a number a member holds
        // (a value cell's or the host's): the place itself, so a write
        // through it lands there and a read reads it there.
        let bindable = match spot {
            Spot::Packed { base, .. } => !matches!(base, Base::Member { cell: None, .. }),
            Spot::Member { member, .. } => self.config.members.get(member).is_some_and(|(s, _)| {
                matches!(s.kind, Kind::Int | Kind::Float | Kind::Color | Kind::Screen)
            }),
            _ => false,
        };
        if bindable {
            self.bound.push(spot);
            return Ok(Value::Bound((self.bound.len() - 1) as u32));
        }
        self.read(spot, host)
    }

    fn write(&mut self, spot: Spot, v: Value, host: &mut dyn Host) -> Result<(), Fault> {
        if !self.sliding {
            self.end_slides_on(spot);
        }
        match spot {
            Spot::Cell(c) => {
                self.write_cell(c, v, 0);
                Ok(())
            }
            Spot::Count(s) => {
                let Some(n) = self.deref(v).as_i32() else {
                    return unsupported(format!("count set to {v:?}"));
                };
                self.resize(s, n.max(0) as usize, host)
            }
            Spot::CastToInteger => {
                if let Some(i) = self.deref(v).as_i32() {
                    self.cast = Value::Int(i);
                }
                Ok(())
            }
            Spot::Undefined => Ok(()),
            // Time left set on a slider: it carries on from there, and with
            // none left it lands on its target in the next frame's step.
            Spot::Slider { id, part } => {
                let Some(n) = self.deref(v).as_f32() else {
                    return Ok(());
                };
                let s = &mut self.sliders[id as usize];
                match part {
                    SliderPart::Left => s.elapsed = (s.time - n).clamp(0.0, s.time),
                    SliderPart::Percent => {
                        s.elapsed = (s.time - n / 100.0 * s.time).clamp(0.0, s.time);
                    }
                    SliderPart::Rate => s.rate = n,
                    SliderPart::Mode => s.ping_pong = n as i32 == 1,
                    SliderPart::Looping => s.looping = n as i32 == 1,
                    SliderPart::Duration => return unsupported("a slider's raw duration written"),
                    SliderPart::IgnoresTimeScales => s.ignores_time_scales = n as i32 == 1,
                }
                Ok(())
            }
            Spot::Packed { base, part } => {
                let whole = self.read(base.spot(), host)?;
                let whole = self.deref(whole);
                let (Some(w), Some(v)) = (part.word(whole), part.convert(self.deref(v))) else {
                    return Ok(());
                };
                self.write(base.spot(), part.set(w, v), host)
            }
            Spot::Const(_) => unsupported("a write to a constant"),
            Spot::Member {
                receiver,
                member,
                cell,
                other,
            } => {
                let Some((spec, _)) = self.config.members.get(member) else {
                    return Err(Fault::Member(member));
                };
                let spec = *spec;
                self.uses.entry(member).or_default().1 += 1;
                let v = if spec.kind == Kind::Reference {
                    v
                } else {
                    self.deref(v)
                };
                let Some(v) = convert(&spec, v) else {
                    return Ok(());
                };
                match cell {
                    Some(slot) => {
                        self.value_cell_values[slot as usize] = v;
                        Ok(())
                    }
                    None => match host.set(&self.access(receiver, member, other), v) {
                        Ok(()) | Err(HostFault::Nothing) => Ok(()),
                        Err(HostFault::Unmodelled) => Err(Fault::Member(member)),
                    },
                }
            }
        }
    }

    fn reset(&mut self, spot: Spot, host: &mut dyn Host) -> Result<(), Fault> {
        match spot {
            // A reference resets what it names, the reference kept.
            Spot::Cell(c) if self.cells[c as usize].kind == CellKind::Reference => {
                let named = self.cells[c as usize].value;
                // A number it names (a run-time cell, or a file variable's)
                // goes back as a reset of the variable itself puts it.
                let number = match named {
                    Value::Cell(v) if self.cells[v as usize].kind == CellKind::Value => {
                        match self.cells[v as usize].value {
                            // A set's element: the cell it names.
                            Value::Cell(_) => None,
                            _ => Some(v),
                        }
                    }
                    Value::Object(o) => match self.program.global_of(o) {
                        Some(g)
                            if matches!(self.program.globals[g as usize].1, Global::Number(_)) =>
                        {
                            Some(self.global_cell(g)?)
                        }
                        _ => None,
                    },
                    _ => None,
                };
                if let Some(v) = number {
                    self.reset_cell(v, 0);
                    return Ok(());
                }
                match self.peek(named) {
                    Value::Null => Ok(()),
                    Value::Cell(v) => {
                        self.reset_cell(v, 0);
                        Ok(())
                    }
                    other => self.reset(Spot::Const(other), host),
                }
            }
            Spot::Cell(c) => {
                self.reset_cell(c, 0);
                Ok(())
            }
            Spot::Member {
                receiver,
                member,
                cell,
                other,
            } => match cell {
                Some(slot) => {
                    let default = self
                        .config
                        .members
                        .get(member)
                        .map_or(Value::Null, |(s, _)| s.default);
                    self.value_cell_values[slot as usize] = default;
                    Ok(())
                }
                None => match host.reset(&self.access(receiver, member, other)) {
                    Ok(()) | Err(HostFault::Nothing) => Ok(()),
                    Err(HostFault::Unmodelled) => Err(Fault::Member(member)),
                },
            },
            Spot::CastToInteger => {
                self.cast = Value::Int(0);
                Ok(())
            }
            Spot::Undefined | Spot::Slider { .. } => Ok(()),
            // A structure or set a reference names, back to its defaults
            // as a variable holding it would be.
            Spot::Const(Value::Struct(st)) => {
                let fields: Vec<u32> = self.structs[st as usize]
                    .fields
                    .iter()
                    .map(|f| f.1)
                    .collect();
                for f in fields {
                    self.reset_cell(f, 1);
                }
                Ok(())
            }
            Spot::Const(Value::Set(set)) => {
                self.sets[set as usize].elems.clear();
                Ok(())
            }
            // A placement (or any engine object) reset whole: back to how
            // it was loaded, which is the engine's to do.
            Spot::Const(v @ (Value::Object(_) | Value::Entity(_))) => {
                let call = OpCall {
                    placement: self.my_handle(),
                    receiver: self.run.receiver,
                    class: Class::Reset,
                    operands: vec![("_NP", Some(v))],
                    words: Vec::new(),
                    sets: Vec::new(),
                };
                match host.op(&call) {
                    Ok(_) => Ok(()),
                    Err(HostFault::Nothing) => Err(Fault::Nothing),
                    Err(HostFault::Unmodelled) => Err(Fault::Op(Class::Reset)),
                }
            }
            other => unsupported(format!("reset of {other:?}")),
        }
    }

    /// A value read: an expression computed, a literal, or a path read with
    /// a reference to a number read as the number.
    fn rhs(
        &mut self,
        program: &Program,
        operand: Option<OperandId>,
        host: &mut dyn Host,
        depth: u32,
    ) -> Result<Value, Fault> {
        if depth > MAX_DEPTH {
            return unsupported(format!("expressions nested past {MAX_DEPTH}"));
        }
        let Some(o) = operand else {
            return Err(Fault::Nothing);
        };
        match program.operands.get(o as usize) {
            Some(Operand::Expr { a, op, b }) => {
                let x = self.rhs(program, *a, host, depth + 1)?;
                let Some(op) = op else {
                    return Ok(x);
                };
                let y = self.rhs(program, *b, host, depth + 1)?;
                match arith_in(self.config.precision, *op, x, y) {
                    Some(v) => Ok(v),
                    None => unsupported(format!("arithmetic {op} on {x:?} and {y:?}")),
                }
            }
            Some(Operand::Path { elems, literal, .. }) => {
                if elems.is_empty() {
                    let Some(l) = literal else {
                        return Err(Fault::Nothing);
                    };
                    if !l.random {
                        return Ok(l.value());
                    }
                    let word = host.draw();
                    return match random_literal(l.value(), word) {
                        Some(v) => Ok(v),
                        None => unsupported(format!("a random literal of {:?}", l.class)),
                    };
                }
                let spot = self.path(program, Some(o), host)?;
                let v = self.read(spot, host)?;
                let v = self.deref_with(v, host)?;
                match literal {
                    // `random(Enemy_DMG_Rand_Small)`: a roll up to the
                    // path's value, by the literal's class.
                    Some(l) if l.random => {
                        let n = match (l.class, v.as_f32()) {
                            (LiteralClass::Float, Some(f)) => Value::Float(f),
                            (_, Some(_)) => Value::Int(v.as_i32().unwrap_or(0)),
                            (_, None) => {
                                return unsupported(format!("a roll up to {v:?}"));
                            }
                        };
                        let word = host.draw();
                        match random_literal(n, word) {
                            Some(r) => Ok(r),
                            None => unsupported(format!("a roll up to {n:?}")),
                        }
                    }
                    _ => Ok(v),
                }
            }
            None => unsupported("an operand past the program"),
        }
    }

    /// A reference read, at `index` into a set when there is one (modulo its
    /// count, negatives wrapping).
    fn rhs_ref(
        &mut self,
        program: &Program,
        operand: Option<OperandId>,
        index: Option<OperandId>,
        host: &mut dyn Host,
    ) -> Result<Value, Fault> {
        let Some(o) = operand else {
            return Err(Fault::Nothing);
        };
        let v = match program.operands.get(o as usize) {
            Some(Operand::Path { elems, .. }) if !elems.is_empty() => {
                let spot = self.path(program, Some(o), host)?;
                self.read_ref(spot, host)?
            }
            _ => self.rhs(program, Some(o), host, 0)?,
        };
        let Some(ix) = index else {
            return Ok(v);
        };
        let i = self.rhs(program, Some(ix), host, 0)?;
        let Some(i) = i.as_i32() else {
            return unsupported(format!("index {i:?}"));
        };
        match self.materialize(v) {
            Value::Set(s) => {
                let elems = &self.sets[s as usize].elems;
                if elems.is_empty() {
                    Ok(Value::Null)
                } else {
                    Ok(elems[i.rem_euclid(elems.len() as i32) as usize])
                }
            }
            Value::Null => Err(Fault::Nothing),
            other => unsupported(format!("indexing {other:?}")),
        }
    }

    /// A reference to a number (a variable's cell, a file's `ValueInfo`)
    /// read as the number.
    fn deref(&mut self, v: Value) -> Value {
        match v {
            Value::Cell(c) if self.cells[c as usize].kind == CellKind::Value => {
                self.cells[c as usize].value
            }
            Value::Bound(b) => self.peek_bound(b).unwrap_or(Value::Null),
            Value::Object(o) => match self.program.global_of(o) {
                Some(g) if matches!(self.program.globals[g as usize].1, Global::Number(_)) => {
                    match self.global_cell(g) {
                        Ok(c) => self.cells[c as usize].value,
                        Err(_) => v,
                    }
                }
                _ => v,
            },
            other => other,
        }
    }

    /// [`Vm::deref`], a bound host member read through the host.
    fn deref_with(&mut self, v: Value, host: &mut dyn Host) -> Result<Value, Fault> {
        if let Value::Bound(b) = v
            && let Some(&spot @ Spot::Member { cell: None, .. }) = self.bound.get(b as usize)
        {
            let v = self.read(spot, host)?;
            return Ok(self.deref(v));
        }
        Ok(self.deref(v))
    }

    /// What a bound place holds, read without the host (a bound place the
    /// VM keeps; a host member's reads none here, [`Vm::deref_with`]'s).
    fn peek_bound(&self, b: u32) -> Option<Value> {
        let whole = |base: Base| -> Option<Value> {
            let v = match base {
                Base::Cell(c) => self.cells.get(c as usize)?.value,
                Base::Const(v) => v,
                Base::Member { cell: Some(s), .. } => *self.value_cell_values.get(s as usize)?,
                Base::Member { cell: None, .. } => return None,
            };
            Some(match v {
                Value::Cell(c) => self.cells.get(c as usize)?.value,
                other => other,
            })
        };
        match *self.bound.get(b as usize)? {
            Spot::Packed { base, part } => Some(part.get(part.word(whole(base)?)?)),
            Spot::Member { cell: Some(s), .. } => self.value_cell_values.get(s as usize).copied(),
            _ => None,
        }
    }

    /// A reference followed to what it holds: a file's set, structure or
    /// reference by its cell's value (the cell made on first use), a
    /// run-time cell by its value.
    fn materialize(&mut self, v: Value) -> Value {
        if let Value::Object(o) = v
            && let Some(g) = self.program.global_of(o)
        {
            let _ = self.global_cell(g);
        }
        self.peek(v)
    }

    /// [`Vm::materialize`] without making a cell.
    fn peek(&self, v: Value) -> Value {
        match v {
            Value::Cell(c) => self.cells.get(c as usize).map_or(v, |c| c.value),
            Value::Object(o) => match self.program.global_of(o) {
                Some(g) => match self.globals.get(g as usize).copied().flatten() {
                    Some(c) => self.cells[c as usize].value,
                    None => v,
                },
                None => v,
            },
            other => other,
        }
    }

    /// `if reference` codes 1 and 5: the same object, a file variable and
    /// its cell counting as one.
    fn same(&mut self, a: Value, b: Value) -> bool {
        if let (Value::Bound(x), Value::Bound(y)) = (a, b) {
            // Bound places compare by what they name (aj-vm: a bound value
            // compares by its fields).
            return self.bound.get(x as usize) == self.bound.get(y as usize);
        }
        a == b || (!a.is_number() && self.materialize(a) == self.materialize(b))
    }

    // ---- cells, structures and sets ----

    fn new_cell(&mut self, decl: Decl, origin: Option<Obj>, depth: u32) -> Result<u32, Fault> {
        let kind = match decl.container {
            Container::Value => CellKind::Value,
            Container::Reference => CellKind::Reference,
            Container::Set => CellKind::Set,
            Container::SetReference => CellKind::SetReference,
            Container::Structure => CellKind::Structure,
            Container::Unknown(s) => {
                return unsupported(format!("a variable of class {:?}", self.program.sym(s)));
            }
        };
        let value = self.default_value(kind, decl.contents, depth)?;
        self.cells.push(Cell {
            kind,
            value,
            origin,
        });
        Ok((self.cells.len() - 1) as u32)
    }

    fn default_value(
        &mut self,
        kind: CellKind,
        contents: Contents,
        depth: u32,
    ) -> Result<Value, Fault> {
        Ok(match kind {
            CellKind::Value => match contents {
                Contents::Class(c) => match self.program.sym(c) {
                    "screenmeasurement" => Value::Screen(0),
                    "colormeasurement" => Value::Color(0),
                    _ => Value::Int(0),
                },
                _ => Value::Int(0),
            },
            CellKind::Reference | CellKind::SetReference => Value::Null,
            CellKind::Set => {
                self.sets.push(SetObj {
                    elems: Vec::new(),
                    elem: Some(contents),
                    next: 0,
                });
                Value::Set((self.sets.len() - 1) as u32)
            }
            CellKind::Structure => match contents {
                Contents::Def(d) => match self.new_struct(d, depth + 1)? {
                    Some(s) => Value::Struct(s),
                    None => Value::Null,
                },
                _ => Value::Null,
            },
        })
    }

    fn new_struct(&mut self, def: DefId, depth: u32) -> Result<Option<u32>, Fault> {
        if depth > MAX_DEPTH {
            return unsupported(format!("structures nested past {MAX_DEPTH}"));
        }
        // Rare (a structure's first use): the clone isn't on the hot path.
        let program = Arc::clone(&self.program);
        let Some(d) = program.resolve(def) else {
            return Ok(None);
        };
        if program.defs[d as usize].kind != DefKind::Structure {
            return Ok(None);
        }
        let mut fields = Vec::new();
        for &f in &program.defs[d as usize].fields {
            if let Ins::Variable { name, decl } = &program.ops[f as usize] {
                let c = self.new_cell(*decl, None, depth)?;
                fields.push((*name, c));
            }
        }
        self.structs.push(StructObj { fields });
        Ok(Some((self.structs.len() - 1) as u32))
    }

    fn field_cell(&mut self, s: u32, name: Sym, decl: Decl, depth: u32) -> Result<u32, Fault> {
        if let Some((_, c)) = self.structs[s as usize]
            .fields
            .iter()
            .find(|(n, _)| *n == name)
        {
            return Ok(*c);
        }
        let c = self.new_cell(decl, None, depth)?;
        self.structs[s as usize].fields.push((name, c));
        Ok(c)
    }

    fn frame_cell(&mut self, def: DefId, name: Sym, decl: Decl) -> Result<u32, Fault> {
        if let Some(&c) = self.frames.get(&def).and_then(|f| f.get(&name)) {
            return Ok(c);
        }
        let c = self.new_cell(decl, None, 0)?;
        self.frames.entry(def).or_default().insert(name, c);
        Ok(c)
    }

    fn local_cell(&mut self, name: Sym, decl: Decl) -> Result<u32, Fault> {
        let Some(id) = self.run.placement else {
            return unsupported("a placement's variable outside any placement");
        };
        if let Some(&c) = self.placements[id as usize].locals.get(&name) {
            return Ok(c);
        }
        let c = self.new_cell(decl, None, 0)?;
        self.placements[id as usize].locals.insert(name, c);
        Ok(c)
    }

    fn global_cell(&mut self, g: GlobalId) -> Result<u32, Fault> {
        if let Some(Some(c)) = self.globals.get(g as usize) {
            return Ok(*c);
        }
        // Once per file variable: the clone isn't on the hot path.
        let program = Arc::clone(&self.program);
        let Some((obj, global)) = program.globals.get(g as usize) else {
            return unsupported("a file variable past the program");
        };
        let (kind, value) = match global {
            Global::Number(v) => (CellKind::Value, *v),
            Global::Reference(e) => (CellKind::Reference, self.constant(*e)),
            Global::Set { members, elem } => {
                let elems = members.iter().map(|&e| self.constant(e)).collect();
                self.sets.push(SetObj {
                    elems,
                    elem: Some(*elem),
                    next: 0,
                });
                (CellKind::Set, Value::Set((self.sets.len() - 1) as u32))
            }
            Global::Structure { def } => {
                let s = match def {
                    Some(d) => self.new_struct(*d, 0)?,
                    None => None,
                };
                (CellKind::Structure, s.map_or(Value::Null, Value::Struct))
            }
        };
        self.cells.push(Cell {
            kind,
            value,
            origin: Some(*obj),
        });
        let c = (self.cells.len() - 1) as u32;
        self.globals[g as usize] = Some(c);
        Ok(c)
    }

    /// A set member or a reference's target in a file, as a value.
    fn constant(&self, e: Elem) -> Value {
        match e {
            Elem::Op(op) => Value::Object(self.program.op_info[op as usize].obj),
            Elem::Global(g) => Value::Object(self.program.globals[g as usize].0),
            Elem::Object(o) => self.canon(Value::Object(o)),
            Elem::Named(a, b) => Value::Named(a, b),
            Elem::Null | Elem::Member(_) | Elem::Unresolved(_) => Value::Null,
        }
    }

    fn write_cell(&mut self, c: u32, v: Value, depth: u32) {
        let (kind, current) = (self.cells[c as usize].kind, self.cells[c as usize].value);
        match kind {
            CellKind::Value => {
                let v = match v {
                    Value::Cell(_) | Value::Object(_) => self.deref(v),
                    other => other,
                };
                if matches!(
                    v,
                    Value::Int(_)
                        | Value::Float(_)
                        | Value::Double(_)
                        | Value::Color(_)
                        | Value::Screen(_)
                ) {
                    self.cells[c as usize].value = v;
                }
            }
            CellKind::Reference => self.cells[c as usize].value = v,
            CellKind::SetReference => {
                let m = self.materialize(v);
                if matches!(m, Value::Set(_)) {
                    self.cells[c as usize].value = m;
                } else if v == Value::Null {
                    self.cells[c as usize].value = v;
                }
            }
            CellKind::Set => {
                if let (Value::Set(dst), Value::Set(src)) = (current, self.materialize(v))
                    && dst != src
                {
                    self.sets[dst as usize].elems = self.sets[src as usize].elems.clone();
                }
            }
            CellKind::Structure => {
                if let (Value::Struct(dst), Value::Struct(src)) = (current, self.materialize(v)) {
                    self.copy_struct(src, dst, depth);
                }
            }
        }
    }

    fn copy_struct(&mut self, src: u32, dst: u32, depth: u32) {
        if src == dst || depth > MAX_DEPTH {
            return;
        }
        let pairs: Vec<(u32, u32)> = self.structs[src as usize]
            .fields
            .iter()
            .filter_map(|(n, c)| {
                self.structs[dst as usize]
                    .fields
                    .iter()
                    .find(|(m, _)| m == n)
                    .map(|(_, d)| (*c, *d))
            })
            .collect();
        for (from, to) in pairs {
            let v = self.cells[from as usize].value;
            self.write_cell(to, v, depth + 1);
        }
    }

    /// A variable back to its declaration's default, in place.
    fn reset_cell(&mut self, c: u32, depth: u32) {
        if depth > MAX_DEPTH {
            return;
        }
        match (self.cells[c as usize].kind, self.cells[c as usize].value) {
            (CellKind::Set, Value::Set(s)) => self.sets[s as usize].elems.clear(),
            (CellKind::Structure, Value::Struct(s)) => {
                let fields: Vec<u32> = self.structs[s as usize]
                    .fields
                    .iter()
                    .map(|f| f.1)
                    .collect();
                for f in fields {
                    self.reset_cell(f, depth + 1);
                }
            }
            (CellKind::Value, _) => self.cells[c as usize].value = Value::Int(0),
            (CellKind::Reference | CellKind::SetReference, _) => {
                self.cells[c as usize].value = Value::Null
            }
            _ => {}
        }
    }

    /// A structure copied whole: its own cells, its structures and sets
    /// copied too.
    fn clone_struct(&mut self, src: u32, depth: u32) -> Result<u32, Fault> {
        if depth > MAX_DEPTH {
            return unsupported(format!("structures nested past {MAX_DEPTH}"));
        }
        let fields = self.structs[src as usize].fields.clone();
        let mut copied = Vec::with_capacity(fields.len());
        for (name, c) in fields {
            let cell = self.cells[c as usize].clone();
            let value = match (cell.kind, cell.value) {
                (CellKind::Structure, Value::Struct(s)) => {
                    Value::Struct(self.clone_struct(s, depth + 1)?)
                }
                (CellKind::Set, Value::Set(s)) => Value::Set(self.clone_set(s, depth + 1)?),
                (_, v) => v,
            };
            self.cells.push(Cell {
                kind: cell.kind,
                value,
                origin: None,
            });
            copied.push((name, (self.cells.len() - 1) as u32));
        }
        self.structs.push(StructObj { fields: copied });
        Ok((self.structs.len() - 1) as u32)
    }

    /// A set copied: its members as they are (references stay references).
    fn clone_set(&mut self, src: u32, depth: u32) -> Result<u32, Fault> {
        if depth > MAX_DEPTH {
            return unsupported(format!("sets nested past {MAX_DEPTH}"));
        }
        let set = self.sets[src as usize].clone();
        self.sets.push(set);
        Ok((self.sets.len() - 1) as u32)
    }

    /// `count` written: shorter drops the tail, longer appends fresh default
    /// elements of the set's element type.
    fn resize(&mut self, s: u32, n: usize, host: &mut dyn Host) -> Result<(), Fault> {
        let len = self.sets[s as usize].elems.len();
        if n <= len {
            self.sets[s as usize].elems.truncate(n);
            return Ok(());
        }
        if n - len > self.config.budget as usize {
            return unsupported(format!("count grown by {} in one write", n - len));
        }
        let elem = self.sets[s as usize].elem.unwrap_or(Contents::None);
        for _ in len..n {
            let v = match elem {
                Contents::Def(d) => self.new_struct(d, 0)?.map_or(Value::Null, Value::Struct),
                Contents::Class(c) => {
                    let container = match self.program.sym(c) {
                        "valueinfo" | "scriptscreeninfo" | "scriptcolorinfo" => {
                            Some(Container::Value)
                        }
                        "scriptreference" | "placementreference" => Some(Container::Reference),
                        _ => None,
                    };
                    match container {
                        Some(container) => {
                            let cell = self.new_cell(
                                Decl {
                                    container,
                                    contents: Contents::None,
                                },
                                None,
                                0,
                            )?;
                            match self.program.sym(c) {
                                "scriptscreeninfo" => {
                                    self.cells[cell as usize].value = Value::Screen(0)
                                }
                                "scriptcolorinfo" => {
                                    self.cells[cell as usize].value = Value::Color(0)
                                }
                                _ => {}
                            }
                            Value::Cell(cell)
                        }
                        // An engine object: a fresh one from the host.
                        None => {
                            let class = self.program.sym(c).to_owned();
                            host.make(&class).unwrap_or(Value::Null)
                        }
                    }
                }
                _ => Value::Null,
            };
            self.sets[s as usize].elems.push(v);
        }
        Ok(())
    }

    /// Combine codes 0-5 (crate docs, Variables, structures and sets).
    fn combine(&mut self, dst: u32, v: Value, code: u32) -> Result<(), Fault> {
        if code > 5 {
            return unsupported(format!("set combine code {code}"));
        }
        let src = match self.materialize(v) {
            Value::Null => return Ok(()),
            Value::Set(src) => Some(src),
            _ => None,
        };
        match src {
            Some(src) => {
                let from = self.sets[src as usize].elems.clone();
                let to = &mut self.sets[dst as usize].elems;
                match code {
                    0 if to.is_empty() => *to = from,
                    0 => {
                        for e in from {
                            if !to.contains(&e) {
                                to.push(e);
                            }
                        }
                    }
                    1 | 5 if src == dst => to.clear(),
                    1 => {
                        for e in from.iter().rev() {
                            if let Some(p) = to.iter().rposition(|x| x == e) {
                                to.remove(p);
                            }
                        }
                    }
                    5 => to.retain(|x| !from.contains(x)),
                    2 => to.retain(|x| from.contains(x)),
                    3 => *to = from,
                    _ => to.extend(from),
                }
            }
            None => {
                let to = &mut self.sets[dst as usize].elems;
                match code {
                    0 => {
                        if !to.contains(&v) {
                            to.push(v);
                        }
                    }
                    1 => {
                        if let Some(p) = to.iter().position(|x| *x == v) {
                            to.remove(p);
                        }
                    }
                    5 => to.retain(|x| *x != v),
                    2 => {
                        let had = to.contains(&v);
                        to.clear();
                        if had {
                            to.push(v);
                        }
                    }
                    3 => {
                        to.clear();
                        to.push(v);
                    }
                    _ => to.push(v),
                }
            }
        }
        Ok(())
    }

    /// `if member`: one value, codes 0, 2 and 3 a member and 1 not; a set,
    /// 0 all, 1 none, 2 any, 3 all counting repeats.
    fn membership(&self, s: u32, v: Value, code: u32) -> bool {
        let set = &self.sets[s as usize].elems;
        match self.peek(v) {
            Value::Set(r) => {
                let rs = &self.sets[r as usize].elems;
                match code {
                    0 => rs.iter().all(|x| set.contains(x)),
                    1 => !rs.is_empty() && !rs.iter().any(|x| set.contains(x)),
                    2 => rs.iter().any(|x| set.contains(x)),
                    3 => rs.iter().all(|x| {
                        set.iter().filter(|y| *y == x).count()
                            >= rs.iter().filter(|y| *y == x).count()
                    }),
                    _ => false,
                }
            }
            _ => {
                let member = set.contains(&v);
                match code {
                    0 | 2 | 3 => member,
                    1 => !member,
                    _ => false,
                }
            }
        }
    }
}

/// A part of a packed value, the members that read and write one (aj-vm's
/// member table and the EBOOT's accessors as read): a screen position is two
/// 12.4 fixed-point halves, x the high one; a colour is RGBA, red the high
/// byte and alpha the low one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Packed {
    ScreenX,
    ScreenY,
    Red,
    Green,
    Blue,
    Alpha,
}

impl Packed {
    const ALL: [(&'static str, &'static str, Packed); 6] = [
        ("screenmeasurement", "x", Packed::ScreenX),
        ("screenmeasurement", "y", Packed::ScreenY),
        ("colormeasurement", "red", Packed::Red),
        ("colormeasurement", "green", Packed::Green),
        ("colormeasurement", "blue", Packed::Blue),
        ("colormeasurement", "alpha", Packed::Alpha),
    ];

    fn of(m: MemberId) -> Option<Self> {
        Self::ALL
            .iter()
            .find(|(ns, n, _)| MemberId::named(ns, n) == m)
            .map(|(.., p)| *p)
    }

    fn screen(self) -> bool {
        matches!(self, Packed::ScreenX | Packed::ScreenY)
    }

    /// The packed word `whole` holds: its own kind, or a number cell not
    /// yet written (0).
    fn word(self, whole: Value) -> Option<u32> {
        match (self.screen(), whole) {
            (true, Value::Screen(w)) | (false, Value::Color(w)) => Some(w),
            (_, Value::Int(0)) => Some(0),
            _ => None,
        }
    }

    fn shift(self) -> u32 {
        match self {
            Packed::ScreenX => 16,
            Packed::ScreenY => 0,
            Packed::Red => 24,
            Packed::Green => 16,
            Packed::Blue => 8,
            Packed::Alpha => 0,
        }
    }

    fn get(self, w: u32) -> Value {
        if self.screen() {
            let half = (w >> self.shift()) as u16 as i16;
            Value::Float(f32::from(half) * 0.0625)
        } else {
            Value::Int(((w >> self.shift()) & 0xFF) as i32)
        }
    }

    /// A write's number for this part: a screen half in sixteenths,
    /// truncated; a colour byte clamped to 0-255 (its registered range).
    fn convert(self, v: Value) -> Option<u32> {
        if self.screen() {
            let f = v.as_f32()?;
            Some(u32::from(crate::value::truncate(f * 16.0) as i16 as u16))
        } else {
            Some(v.as_i32()?.clamp(0, 255) as u32)
        }
    }

    fn set(self, w: u32, part: u32) -> Value {
        let mask = if self.screen() { 0xFFFF } else { 0xFF } << self.shift();
        let w = (w & !mask) | ((part << self.shift()) & mask);
        if self.screen() {
            Value::Screen(w)
        } else {
            Value::Color(w)
        }
    }
}

/// Whether `op` sits in `def`'s own INTERFACE: the block of the op right
/// after the definition's position, as every definition on the disc lays
/// it out. A stub (no position) declares nothing.
fn declares(program: &Program, def: DefId, op: OpId) -> bool {
    let Some((sid, pos)) = program.defs.get(def as usize).and_then(|d| d.placed) else {
        return false;
    };
    let Some(ops) = program.scripts.get(sid as usize).map(|s| &s.ops) else {
        return false;
    };
    let pos = pos as usize;
    let Some(&iface) = ops.get(pos + 1) else {
        return false;
    };
    if program.ops[iface as usize] != Ins::Interface {
        return false;
    }
    let n = program.branch(iface) as usize;
    ops.get(pos + 2..=pos + 1 + n)
        .is_some_and(|params| params.contains(&op))
}

/// The parameter `def`'s own INTERFACE declares under `name`, any case.
fn own_parameter(program: &Program, def: DefId, name: &str) -> Option<Param> {
    let (sid, pos) = program.defs.get(def as usize)?.placed?;
    let ops = &program.scripts.get(sid as usize)?.ops;
    let pos = pos as usize;
    let &iface = ops.get(pos + 1)?;
    if program.ops[iface as usize] != Ins::Interface {
        return None;
    }
    let n = program.branch(iface) as usize;
    ops.get(pos + 2..=pos + 1 + n)?
        .iter()
        .find_map(|&o| match &program.ops[o as usize] {
            Ins::Parameter(p) if program.sym(p.name).eq_ignore_ascii_case(name) => Some(*p),
            _ => None,
        })
}

/// The callee's name for a parameter: its own, else the one the callee's
/// INTERFACE gives the same id.
fn param_name(
    program: &Program,
    frames: &HashMap<DefId, HashMap<Sym, u32>>,
    def: DefId,
    p: &Param,
) -> Sym {
    if frames.get(&def).is_some_and(|f| f.contains_key(&p.name)) || p.id == 0 {
        return p.name;
    }
    let Some((sid, pos)) = program.defs[def as usize].placed else {
        return p.name;
    };
    let ops = &program.scripts[sid as usize].ops;
    let Some(&iface) = ops.get(pos as usize + 1) else {
        return p.name;
    };
    let n = program.branch(iface) as usize;
    let mut by_id = None;
    for q in pos as usize + 2..=pos as usize + 1 + n {
        if let Some(&o) = ops.get(q)
            && let Ins::Parameter(own) = &program.ops[o as usize]
        {
            if own.name == p.name {
                return p.name;
            }
            if own.id == p.id {
                by_id = Some(own.name);
            }
        }
    }
    by_id.unwrap_or(p.name)
}

impl Run {
    /// The running placement as a value, which `find subset` leaves out.
    fn receiver_placement(&self, placements: &[Placement]) -> Option<Value> {
        let id = self.placement?;
        placements.get(id as usize).map(|p| Value::Entity(p.handle))
    }
}
