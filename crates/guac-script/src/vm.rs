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
    Container, Contents, Decl, DefId, DefKind, Elem, Global, GlobalId, Ins, Obj, OpId, Operand,
    OperandId, Param, Program, ScriptId, Sym,
};
use crate::value::{Value, arith, compare, convert, random_literal};
use crate::whitelist::Whitelist;

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

#[derive(Debug, Clone)]
pub struct Config {
    pub mode: Mode,
    /// Ops one placement may execute in one tick.
    pub budget: u32,
    pub members: Members,
    pub whitelist: Whitelist,
}

impl Config {
    pub fn new(mode: Mode) -> Self {
        Self {
            mode,
            budget: DEFAULT_BUDGET,
            members: Members::new(),
            whitelist: Whitelist::All,
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
    started: bool,
    behaviour: Option<u32>,
    shutdown: bool,
    locals: HashMap<Sym, u32>,
    stopped: Option<Halt>,
    inert: bool,
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
    },
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
        Value::Color(c) => (3, c as u64),
        Value::Screen(s) => (4, s as u64),
        Value::Object(o) => (5, (o.unit as u64) << 32 | o.index as u64),
        Value::Named(a, b) => (6, (a as u64) << 32 | b as u64),
        Value::Entity(e) => (7, e),
        Value::Set(s) => (8, s as u64),
        Value::Struct(s) => (9, s as u64),
        Value::Cell(c) => (10, c as u64),
    };
    Recv::Value(tag, bits)
}

/// The interpreter over one [`Program`].
pub struct Vm {
    program: Arc<Program>,
    config: Config,
    placements: Vec<Placement>,
    cells: Vec<Cell>,
    sets: Vec<SetObj>,
    structs: Vec<StructObj>,
    globals: Vec<Option<u32>>,
    frames: HashMap<DefId, HashMap<Sym, u32>>,
    busy: HashSet<DefId>,
    op_state: HashMap<OpId, OpState>,
    value_cells: HashMap<(Recv, MemberId), u32>,
    value_cell_values: Vec<Value>,
    cast: Value,
    reached: HashSet<MemberId>,
    calls: HashMap<DefId, u64>,
    call_sites: HashMap<(ScriptId, u32), u64>,
    log: Vec<LogLine>,
    executed: u64,
    run: Run,
}

impl Vm {
    pub fn new(program: Arc<Program>, config: Config) -> Self {
        let globals = vec![None; program.globals.len()];
        Self {
            program,
            config,
            placements: Vec::new(),
            cells: Vec::new(),
            sets: Vec::new(),
            structs: Vec::new(),
            globals,
            frames: HashMap::new(),
            busy: HashSet::new(),
            op_state: HashMap::new(),
            value_cells: HashMap::new(),
            value_cell_values: Vec::new(),
            cast: Value::Int(0),
            reached: HashSet::new(),
            calls: HashMap::new(),
            call_sites: HashMap::new(),
            log: Vec::new(),
            executed: 0,
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
            },
        }
    }

    pub fn program(&self) -> &Program {
        &self.program
    }

    pub fn config(&self) -> &Config {
        &self.config
    }

    /// Adds a placement running `script`, ticked after those added before
    /// it. Off the whitelist it stays inert: never ticked, listed.
    pub fn add(&mut self, handle: Handle, name: &str, script: ScriptId) -> PlacementId {
        let script_name = self
            .program
            .scripts
            .get(script as usize)
            .map_or("", |s| s.name.as_str());
        let inert = !self.config.whitelist.lists(name, script_name);
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
            started: false,
            behaviour: None,
            shutdown: false,
            locals: HashMap::new(),
            stopped: None,
            inert,
        });
        (self.placements.len() - 1) as PlacementId
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

    /// One logic tick: every placement that isn't inert or stopped, in the
    /// order added. Strict mode returns the first stop; play mode records
    /// it and runs the rest.
    pub fn step(&mut self, host: &mut dyn Host) -> Result<StepReport, Halt> {
        let mut report = StepReport::default();
        for id in 0..self.placements.len() as PlacementId {
            let p = &self.placements[id as usize];
            if p.inert || p.stopped.is_some() {
                continue;
            }
            let before = self.executed;
            let result = self.tick(id, host);
            report.ticked += 1;
            report.ops += self.executed - before;
            match result {
                Ok(None) => {}
                Ok(Some(halt)) => report.stopped.push(halt),
                Err(halt) => return Err(halt),
            }
        }
        Ok(report)
    }

    /// One tick of one placement. `Ok(Some)` is a play-mode stop.
    pub fn tick(&mut self, id: PlacementId, host: &mut dyn Host) -> Result<Option<Halt>, Halt> {
        let Some(p) = self.placements.get(id as usize) else {
            return Ok(None);
        };
        if p.inert || p.stopped.is_some() {
            return Ok(None);
        }
        let handle = p.handle;
        let script = p.script;
        self.begin(Some(id), script, Value::Entity(handle));
        let result = self.execute(host);
        self.finish(result)
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
        let Some(def) = self.program.macro_named(name) else {
            return Err(self.halt_outside(Reason::Unsupported(format!("no macro {name:?}"))));
        };
        let receiver = placement
            .and_then(|p| self.placements.get(p as usize))
            .map_or(Value::Null, |p| Value::Entity(p.handle));
        let (sid, _) = self.program.defs[def as usize].placed.unwrap_or((0, 0));
        self.begin(placement, sid, receiver);
        self.busy.insert(def);
        self.run.calls.push(Call { def, caller: None });
        self.run.blocks.push(Block {
            ctx: sid,
            start: 0,
            len: 0,
            op: self.program.defs[def as usize].op,
            kind: BlockKind::Root,
        });
        let arc = Arc::clone(&self.program);
        let result = match self.enter(&arc, def) {
            Ok(()) => self.execute(host),
            Err(f) => Err(f),
        };
        let ops = self.run.ops;
        match self.finish(result)? {
            None => Ok(ops),
            Some(halt) => Err(halt),
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
        self.run.frame = self.run.calls.last().and_then(|c| c.caller);
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
                    Ok(v) => self.deref(v),
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
            Ins::World { operands, words } => self.world(program, op, operands, words, host)?,
        }
        Ok(())
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
        let Some(Operand::Path { elems, .. }) = lhs.and_then(|l| program.operands.get(l as usize))
        else {
            return unsupported("find subset without a path");
        };
        let Some(slot) = elems.iter().position(|e| *e == Elem::Null) else {
            return unsupported("find subset's path has no empty slot");
        };
        let set = self.walk(program, &elems[..slot], host)?;
        let set = self.read(set, host)?;
        let Value::Set(s) = self.materialize(set) else {
            return if set == Value::Null {
                Err(Fault::Nothing)
            } else {
                unsupported(format!("find subset over {set:?}"))
            };
        };
        let names_its = rhs
            .and_then(|r| program.operands.get(r as usize))
            .is_some_and(|o| {
                matches!(
                    o,
                    Operand::Path {
                        names_its: true,
                        ..
                    }
                )
            });
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
            for &e in &elems[slot + 1..] {
                match self.next_spot(program, spot, e, host) {
                    Ok(next) => spot = next,
                    Err(Fault::Nothing) => {
                        ok = false;
                        break;
                    }
                    Err(f) => {
                        self.run.its = saved_its;
                        return Err(f);
                    }
                }
            }
            if !ok {
                continue;
            }
            let v = match self.read(spot, host) {
                Ok(v) => self.deref(v),
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
        let mut read = Vec::with_capacity(operands.len());
        for &(name, o) in operands {
            let v = match o {
                None => None,
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
        let call = OpCall {
            placement: self.my_handle(),
            receiver: self.run.receiver,
            class,
            operands: read,
            words: words.to_vec(),
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
        }
        Ok(())
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
                Ins::Define(_) | Ins::Behaviour { .. } => {
                    Ok(Spot::Const(Value::Object(program.op_info[op as usize].obj)))
                }
                _ => unsupported(format!(
                    "{} as a path element",
                    program.class(op).map_or("?", Class::type_name)
                )),
            },
            Elem::Global(g) => Ok(Spot::Cell(self.global_cell(g)?)),
            Elem::Object(o) => Ok(Spot::Const(Value::Object(o))),
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
                    Some(b) => unsupported(format!("{} heading a path", b.name())),
                    None => self.member(None, m, Recv::Head),
                }
            }
            Elem::Unresolved(s) => unsupported(program.sym(s).to_owned()),
        }
    }

    /// The frame a parameter on a path reads. A definition's own parameter
    /// reads the running definition's frame: in its body, while it binds a
    /// nested call's arguments and in a nested call's after-return code
    /// (a specialization's own parameters name the base). Any other reads
    /// the definition it binds into (a stub followed), or the open call's
    /// specialization of it (an after-return read of what a call returned).
    fn param_frame(&self, program: &Program, op: OpId, p: &Param) -> Option<DefId> {
        if let Some(running) = self.run.frame
            && declares(program, running, op)
        {
            return Some(running);
        }
        let def = p.def.and_then(|d| program.resolve(d))?;
        if let Some(top) = self.run.calls.last()
            && top.def != def
            && program.defs[top.def as usize]
                .base
                .and_then(|b| program.resolve(b))
                == Some(def)
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
                match Builtin::of(m) {
                    Some(Builtin::ValueDereferencer) => self.dereferencer(prev, host),
                    Some(
                        b @ (Builtin::Count | Builtin::First | Builtin::Last | Builtin::Random),
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
                            return Err(Fault::Nothing);
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
        })
    }

    fn access(&self, receiver: Option<Value>, member: MemberId) -> Access {
        Access {
            placement: self.my_handle(),
            receiver,
            member,
            other_point: self.run.other_point,
        }
    }

    fn read(&mut self, spot: Spot, host: &mut dyn Host) -> Result<Value, Fault> {
        Ok(match spot {
            Spot::Cell(c) => self.cells[c as usize].value,
            Spot::Const(v) => v,
            Spot::Count(s) => Value::Int(self.sets[s as usize].elems.len() as i32),
            Spot::CastToInteger => self.cast,
            Spot::Undefined => Value::Int(0),
            Spot::Member {
                cell: Some(slot), ..
            } => self.value_cell_values[slot as usize],
            Spot::Member {
                receiver,
                member,
                cell: None,
            } => match host.get(&self.access(receiver, member)) {
                Ok(v) => v,
                Err(HostFault::Nothing) => return Err(Fault::Nothing),
                Err(HostFault::Unmodelled) => return Err(Fault::Member(member)),
            },
        })
    }

    /// A read for a reference: a number variable reads as itself (the file's
    /// object, or its cell), anything else as its value.
    fn read_ref(&mut self, spot: Spot, host: &mut dyn Host) -> Result<Value, Fault> {
        if let Spot::Cell(c) = spot
            && self.cells[c as usize].kind == CellKind::Value
        {
            return Ok(match self.cells[c as usize].origin {
                Some(o) => Value::Object(o),
                None => Value::Cell(c),
            });
        }
        self.read(spot, host)
    }

    fn write(&mut self, spot: Spot, v: Value, host: &mut dyn Host) -> Result<(), Fault> {
        match spot {
            Spot::Cell(c) => {
                self.write_cell(c, v, 0);
                Ok(())
            }
            Spot::Count(s) => {
                let Some(n) = self.deref(v).as_i32() else {
                    return unsupported(format!("count set to {v:?}"));
                };
                self.resize(s, n.max(0) as usize)
            }
            Spot::CastToInteger => {
                if let Some(i) = self.deref(v).as_i32() {
                    self.cast = Value::Int(i);
                }
                Ok(())
            }
            Spot::Undefined => Ok(()),
            Spot::Const(_) => unsupported("a write to a constant"),
            Spot::Member {
                receiver,
                member,
                cell,
            } => {
                let Some((spec, _)) = self.config.members.get(member) else {
                    return Err(Fault::Member(member));
                };
                let spec = *spec;
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
                    None => match host.set(&self.access(receiver, member), v) {
                        Ok(()) | Err(HostFault::Nothing) => Ok(()),
                        Err(HostFault::Unmodelled) => Err(Fault::Member(member)),
                    },
                }
            }
        }
    }

    fn reset(&mut self, spot: Spot, host: &mut dyn Host) -> Result<(), Fault> {
        match spot {
            Spot::Cell(c) => {
                self.reset_cell(c, 0);
                Ok(())
            }
            Spot::Member {
                receiver,
                member,
                cell,
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
                None => match host.reset(&self.access(receiver, member)) {
                    Ok(()) | Err(HostFault::Nothing) => Ok(()),
                    Err(HostFault::Unmodelled) => Err(Fault::Member(member)),
                },
            },
            Spot::CastToInteger => {
                self.cast = Value::Int(0);
                Ok(())
            }
            Spot::Undefined => Ok(()),
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
                match arith(*op, x, y) {
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
                Ok(self.deref(v))
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
            CellKind::Value => Value::Int(0),
            CellKind::Reference | CellKind::SetReference => Value::Null,
            CellKind::Set => {
                self.sets.push(SetObj {
                    elems: Vec::new(),
                    elem: Some(contents),
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
            Elem::Object(o) => Value::Object(o),
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
                    Value::Int(_) | Value::Float(_) | Value::Color(_) | Value::Screen(_)
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

    /// `count` written: shorter drops the tail, longer appends fresh default
    /// elements of the set's element type.
    fn resize(&mut self, s: u32, n: usize) -> Result<(), Fault> {
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
                        "valueinfo" => Some(Container::Value),
                        "scriptreference" | "placementreference" => Some(Container::Reference),
                        _ => None,
                    };
                    match container {
                        Some(container) => Value::Cell(self.new_cell(
                            Decl {
                                container,
                                contents: Contents::None,
                            },
                            None,
                            0,
                        )?),
                        None => Value::Null,
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
