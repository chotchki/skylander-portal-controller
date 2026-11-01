//! AS.11's gap list: what every level's scripts stop on in the VM. Each
//! `level/*.bld` on the Giants dump is linked with `bootstrap.bld`,
//! `global.bld` and `init_setup.bld`, straight from the dump in memory, in
//! guacamole's order and roles, and run strict on a null host:
//!
//! - every member the registry's tables hold is a value cell, so a stop is
//!   never a member guacamole models; every world op finds no target; the
//!   draws come off a generator with a fixed seed;
//! - `global.bld`'s running placements run their first tick and are held,
//!   bar the four guacamole ticks every step in play (the animation macros,
//!   the UI, the mover update and the point lights); `init_setup.bld`'s and
//!   the level's run every tick;
//! - phase A runs those the load wakes (no activation range and no buffer)
//!   for [`TICKS`] ticks, phase B wakes every managed placement, as if a
//!   hero stood by each, for as many more.
//!
//! A stop at a member no table registers makes that member a value cell
//! and the level runs again from its start; any other stop (a shape the VM
//! doesn't model, the budget) leaves that placement out and the level runs
//! again; until the ticks run clean or [`CAP`] runs. Each level logs its
//! link, its first stop and the chain behind it; the table at the end ranks
//! every gap by the levels reaching it, then the earliest tick.
//!
//! Members are hashes in the files. With `GUAC_MEMBER_TABLE` set to a
//! registry table outside the tree (aj-vm's `members_live_table.tsv`: class,
//! kind, name, measurement class, three handlers, units, min, max), gaps
//! print by name and a filled member takes its registered type; without it
//! they print as `<namespace hash>.<name hash>` and take any value.
//!
//! Ignored by default: it reads `GUAC_GIANTS_DUMP` (env, else `.env.dev`).
//! `GUAC_LEVELS` (a comma list of archive names) runs those alone.
//!
//! `cargo test -p guac-script --release --test giants_levels -- --ignored --nocapture`

mod common;

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::sync::Arc;
use std::time::Instant;

use common::{env_or_dotenv, usrdir};
use guac_formats::exports::NAMESPACE;
use guac_formats::fnv1a32;
use guac_formats::iga::Archive;
use guac_formats::igz::Igz;
use guac_formats::igz::layouts::{Layout, ObjectRef, RootList};
use guac_formats::level::Placements;
use guac_formats::script::Operand as Raw;
use guac_formats::script::{Class, Graph};
use guac_script::members::{self, Keeper, Registered};
use guac_script::{
    Access, Answer, Builtin, Config, Elem, Generator, Halt, Handle, Host, HostFault, Ins, Kind,
    MemberId, MemberSpec, Mode, Obj, OpCall, Operand, Program, Reason, Role, Unit, Value, Vm,
    Whitelist,
};

/// Ticks a phase runs: 10 s of the game's 60 a second.
const TICKS: u32 = 600;
/// Runs a level gets before its chain is cut.
const CAP: usize = 300;
/// `_activityState` of a placement the level runs (UNMANAGED).
const RUNNING: u32 = 4;

/// The registry's tables: guacamole's, as it registers them, and AS's
/// every-level table, which the TV's run doesn't register yet (AS.37).
const TABLES: [&[Registered]; 11] = [
    members::INIT_CHAIN,
    members::HERO,
    members::CREATURES,
    members::COMBAT,
    members::LOOP,
    members::FIGURE,
    members::CAMERA,
    members::WALKERS,
    members::ROUTE,
    members::LEVEL_UP,
    members::LEVELS,
];

/// `global.bld`'s placements guacamole ticks every step in play
/// (`scripts::GLOBAL_EVERY_TICK`); the rest of its running ones run their
/// first tick and are held (`scripts::GLOBAL_FIRST_TICKS`).
const GLOBAL_EVERY_TICK: [&str; 4] = [
    "Macros_Animation.ai",
    "/UI.ai",
    "movementUpdate.ai",
    "PointLightManager.ai",
];

fn level_bld(path: &std::path::Path) -> Vec<u8> {
    let bytes = std::fs::read(path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    let arc = Archive::parse(&bytes).unwrap();
    let entry = arc.find("level.bld").expect("level.bld");
    arc.read(entry).unwrap().into_owned()
}

/// A member's registered type off the registry table's row.
fn spec_of(class: &str, min: f32, max: f32) -> MemberSpec {
    let kind = match class {
        "FloatMeasurement" | "PairedFloatMeasurement" => Kind::Float,
        "IntMeasurement" | "EnumMeasurement" | "ButtonMeasurement" | "PairedEnumMeasurement" => {
            Kind::Int
        }
        "ColorMeasurement" => Kind::Color,
        "ScreenMeasurement" => Kind::Screen,
        _ => Kind::Reference,
    };
    MemberSpec::new(kind, min, max)
}

/// The registry table at `GUAC_MEMBER_TABLE`, if set: each member's name and
/// registered type by its id.
fn member_table() -> HashMap<MemberId, (String, MemberSpec)> {
    let Some(path) = std::env::var_os("GUAC_MEMBER_TABLE") else {
        return HashMap::new();
    };
    let text = std::fs::read_to_string(&path).expect("GUAC_MEMBER_TABLE reads");
    let mut out = HashMap::new();
    for line in text.lines().filter(|l| !l.starts_with('#')) {
        let f: Vec<&str> = line.split('\t').collect();
        if f.len() < 10 {
            continue;
        }
        let class = f[0].to_ascii_lowercase();
        let namespace = class.strip_prefix("tfb").unwrap_or(&class);
        let name = f[2].trim_start_matches('-').to_ascii_lowercase();
        let num = |s: &str| s.trim().parse::<f32>().unwrap_or(0.0);
        out.insert(
            MemberId::named(namespace, &name),
            (
                format!("{namespace}.{name}"),
                spec_of(f[3], num(f[8]), num(f[9])),
            ),
        );
    }
    out
}

/// The null host: no member reaches it (every registered one is a value
/// cell), every world op finds no target, draws come off a seeded
/// generator; it counts the world ops asked, by class.
struct NullHost {
    generator: Generator,
    ops: BTreeMap<&'static str, u64>,
}

impl Host for NullHost {
    fn get(&mut self, _: &Access) -> Result<Value, HostFault> {
        Err(HostFault::Unmodelled)
    }
    fn set(&mut self, _: &Access, _: Value) -> Result<(), HostFault> {
        Err(HostFault::Unmodelled)
    }
    fn reset(&mut self, _: &Access) -> Result<(), HostFault> {
        Err(HostFault::Unmodelled)
    }
    fn draw(&mut self) -> u32 {
        self.generator.next_u32()
    }
    fn op(&mut self, call: &OpCall) -> Result<Answer, HostFault> {
        *self.ops.entry(call.class.type_name()).or_default() += 1;
        Ok(Answer::Skip)
    }
}

/// A running placement: `awake` whether the load wakes it (no range, no
/// buffer), `load` its place on its file's root list.
#[derive(Debug, Clone)]
struct Placed {
    obj: Obj,
    name: String,
    script: String,
    awake: bool,
    load: u32,
}

/// Every actor placement's object (for naming) and the running ones, in
/// the order the original's load adds them: awake from the load first,
/// then the rest, each by its place on the file's root list.
fn placements(unit: u16, igz: &Igz<'_>) -> (Vec<Obj>, Vec<Placed>) {
    let p = Placements::read(igz).unwrap_or_default();
    let roots: HashMap<usize, u32> = RootList::read(igz, 0)
        .map(|r| {
            let mut m = HashMap::new();
            for (k, e) in r.elements.iter().enumerate() {
                if let ObjectRef::Object(i) = e {
                    m.entry(*i).or_insert(k as u32);
                }
            }
            m
        })
        .unwrap_or_default();
    let mut objects = Vec::new();
    let mut running = Vec::new();
    for a in &p.actors {
        let obj = Obj {
            unit,
            index: a.index as u32,
        };
        objects.push(obj);
        let at = a.placement();
        let (Some(script), RUNNING) = (a.script, at.placement.activity_state) else {
            continue;
        };
        let range = at.activation_range;
        let buffer = at.activation_range_buffer;
        running.push(Placed {
            obj,
            name: a.name().unwrap_or("").to_owned(),
            script: script.to_owned(),
            awake: range <= 0.0 && buffer <= 0.0,
            load: roots
                .get(&a.index)
                .copied()
                .unwrap_or(roots.len() as u32 + a.index as u32),
        });
    }
    running.sort_by_key(|p| (!p.awake, p.load));
    (objects, running)
}

fn handle(obj: Obj) -> Handle {
    (1 << 48) | (u64::from(obj.unit) << 32) | u64::from(obj.index)
}

/// A stop, where it fell.
#[derive(Debug, Clone)]
struct Stop {
    phase: char,
    tick: u32,
    halt: Halt,
}

/// A run's end: every tick of both phases ran, or a stop and where it fell.
enum Ran {
    Clean,
    Stopped(Stop),
}

struct Level<'p> {
    program: Arc<Program>,
    objects: Vec<Obj>,
    running: Vec<Placed>,
    names: &'p HashMap<MemberId, (String, MemberSpec)>,
}

impl Level<'_> {
    fn config(&self, filled: &BTreeSet<MemberId>) -> Config {
        let mut c = Config::new(Mode::Strict);
        for table in TABLES {
            for r in table {
                match r.keeper {
                    Keeper::Vm => {
                        c.members.known(r.namespace, r.name);
                    }
                    Keeper::Host | Keeper::Cell => {
                        let cell = Registered {
                            keeper: Keeper::Cell,
                            ..*r
                        };
                        c.members.value_cell(r.namespace, r.name, cell.spec());
                    }
                }
            }
        }
        for (id, (name, _)) in self.names {
            if c.members.get(*id).is_none() {
                let (ns, n) = name.split_once('.').unwrap_or((name, ""));
                c.members.known(ns, n);
            }
        }
        for id in filled {
            let spec = self
                .names
                .get(id)
                .map_or(MemberSpec::new(Kind::Any, 0.0, 0.0), |(_, s)| *s);
            c.members.value_cell_id(*id, spec);
        }
        let mut every: Vec<String> = GLOBAL_EVERY_TICK.iter().map(|s| (*s).to_owned()).collect();
        let mut first = Vec::new();
        for s in &self.program.scripts {
            match s.unit {
                1 => first.push(s.name.clone()),
                2 | 3 => every.push(s.name.clone()),
                _ => {}
            }
        }
        c.whitelist = Whitelist::Only {
            placements: Vec::new(),
            scripts: every,
            first_tick: first,
        };
        c
    }

    /// One run from the level's start: phase A, then phase B.
    fn run(&self, filled: &BTreeSet<MemberId>, out: &HashSet<Handle>, host: &mut NullHost) -> Ran {
        let mut vm = Vm::new(self.program.clone(), self.config(filled));
        for &obj in &self.objects {
            vm.name_object(obj, handle(obj));
        }
        let scripts: HashMap<(u16, &str), u32> = self
            .program
            .scripts
            .iter()
            .enumerate()
            .map(|(i, s)| ((s.unit, s.name.as_str()), i as u32))
            .collect();
        let mut asleep = Vec::new();
        for p in &self.running {
            let h = handle(p.obj);
            if out.contains(&h) {
                continue;
            }
            let Some(&sid) = scripts.get(&(p.obj.unit, p.script.as_str())) else {
                continue;
            };
            vm.add_at(h, &p.name, sid, p.obj);
            if !p.awake {
                asleep.push((h, false));
            }
        }
        vm.set_activity(&asleep);
        for (phase, wake) in [('A', false), ('B', true)] {
            if wake {
                let all: Vec<(Handle, bool)> = asleep.iter().map(|&(h, _)| (h, true)).collect();
                vm.set_activity(&all);
            }
            for tick in 0..TICKS {
                if let Err(halt) = vm.step(host) {
                    return Ran::Stopped(Stop { phase, tick, halt });
                }
            }
        }
        Ran::Clean
    }
}

/// A member's id back out of a halt's name (`ns.name` by the table, or
/// the two hashes).
fn member_of(name: &str, names: &HashMap<MemberId, (String, MemberSpec)>) -> Option<MemberId> {
    if let Some((id, _)) = names.iter().find(|(_, (n, _))| n == name) {
        return Some(*id);
    }
    let (a, b) = name.split_once('.')?;
    let ns = u32::from_str_radix(a, 16).ok()?;
    let n = u32::from_str_radix(b, 16).ok()?;
    (a.len() == 8 && b.len() == 8).then_some(MemberId {
        namespace: ns,
        name: n,
    })
}

/// A gap's key in the ranking: the member, or the stop's kind and words.
fn gap_key(halt: &Halt) -> String {
    match &halt.reason {
        Reason::UnmodelledMember(m) => format!("member {m}"),
        Reason::UnmodelledOp(c) => format!("op {}", c.type_name()),
        Reason::Unsupported(s) => format!("unsupported {s}"),
        Reason::Budget(_) => "budget".to_owned(),
    }
}

fn op_name(c: Option<Class>) -> &'static str {
    c.map_or("-", Class::type_name)
}

fn one_line(s: &str) -> String {
    s.replace(['\n', '\t'], " ")
}

/// A gap reached in one level: the level, where the run first stopped on
/// it and whose script it was.
struct Hit {
    level: String,
    phase: char,
    tick: u32,
    placement: String,
    script: String,
}

/// One link of a level's chain: the ranking's key, where the run stopped
/// and whose script it was.
struct Found {
    gap: String,
    phase: char,
    tick: u32,
    placement: String,
    script: String,
}

/// A member's name by the table, or its two hashes as a halt names it.
fn member_name(id: MemberId, names: &HashMap<MemberId, (String, MemberSpec)>) -> String {
    names.get(&id).map_or_else(
        || format!("{:08x}.{:08x}", id.namespace, id.name),
        |(n, _)| n.clone(),
    )
}

/// An operand's path as words: members by name, anything else by kind.
fn path_words(
    program: &Program,
    operand: Option<guac_script::OperandId>,
    names: &HashMap<MemberId, (String, MemberSpec)>,
) -> String {
    let Some(o) = operand else {
        return "-".into();
    };
    match &program.operands[o as usize] {
        Operand::Path { elems, .. } => elems
            .iter()
            .map(|e| match e {
                Elem::Null => "_".to_owned(),
                Elem::Op(id) => format!("op{id}"),
                Elem::Global(g) => format!("global{g}"),
                Elem::Object(o) => format!("obj{}:{}", o.unit, o.index),
                Elem::Named(a, b) => format!("{}::{}", program.sym(*a), program.sym(*b)),
                Elem::Member(m) => match Builtin::of(*m) {
                    Some(b) => b.name().to_owned(),
                    None => member_name(*m, names),
                },
                Elem::Unresolved(s) => format!("unresolved({})", program.sym(*s)),
            })
            .collect::<Vec<_>>()
            .join("."),
        other => format!("{other:?}"),
    }
}

/// What a stop's op reads, for an unsupported shape: a loop's set, a
/// check's sides.
fn stop_detail(
    program: &Program,
    halt: &Halt,
    names: &HashMap<MemberId, (String, MemberSpec)>,
) -> String {
    if !matches!(halt.reason, Reason::Unsupported(_)) {
        return String::new();
    }
    let Some(sid) = program.script_named(&halt.script) else {
        return String::new();
    };
    let Some(&op) = program.scripts[sid as usize]
        .ops
        .get(halt.position as usize)
    else {
        return String::new();
    };
    match &program.ops[op as usize] {
        Ins::ForEach { lhs, .. } | Ins::FindSubSet { lhs, .. } => {
            format!("over {}", path_words(program, *lhs, names))
        }
        other => {
            let s = format!("{other:?}");
            s.chars().take(200).collect()
        }
    }
}

/// The engine members a file's own operands name, with how often: every
/// import that isn't one of `global.bld`'s or `level.bld`'s exports.
fn members_named(graph: &Graph<'_>) -> BTreeMap<MemberId, u64> {
    let (global, level) = (fnv1a32(NAMESPACE.as_bytes()), fnv1a32(b"level.bld"));
    let mut out = BTreeMap::new();
    for operand in graph.operands.values() {
        let Raw::Stack(path) = operand else {
            continue;
        };
        for e in &path.elements {
            if let ObjectRef::Import(i) = e
                && i.id.namespace != global
                && i.id.namespace != level
            {
                let id = MemberId {
                    namespace: i.id.namespace,
                    name: i.id.object,
                };
                *out.entry(id).or_default() += 1;
            }
        }
    }
    out
}

fn load_average() -> String {
    std::process::Command::new("sysctl")
        .args(["-n", "vm.loadavg"])
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_owned())
        .unwrap_or_default()
}

#[test]
#[ignore = "reads the Giants dump (GUAC_GIANTS_DUMP)"]
fn every_levels_scripts_run_on_a_null_host() {
    let usr = usrdir();
    let names = member_table();
    let only: Option<Vec<String>> = env_or_dotenv("GUAC_LEVELS")
        .filter(|s| !s.is_empty())
        .map(|s| s.split(',').map(str::to_owned).collect());
    let boot = level_bld(&usr.join("permanent/bootstrap.bld"));
    let global = level_bld(&usr.join("permanent/global.bld"));
    let init = level_bld(&usr.join("character/init_setup.bld"));
    let (bi, gi, ii) = (
        Igz::parse(&boot).unwrap(),
        Igz::parse(&global).unwrap(),
        Igz::parse(&init).unwrap(),
    );
    let (bg, gg, ig) = (
        Graph::read(&bi).unwrap(),
        Graph::read(&gi).unwrap(),
        Graph::read(&ii).unwrap(),
    );
    let (gobjs, grun) = placements(1, &gi);
    let (iobjs, irun) = placements(2, &ii);
    let mut files: Vec<String> = std::fs::read_dir(usr.join("level"))
        .unwrap()
        .filter_map(|e| {
            let n = e.unwrap().file_name().into_string().ok()?;
            n.strip_suffix(".bld").map(str::to_owned)
        })
        .collect();
    files.sort();
    let all = files.len();
    if let Some(only) = &only {
        files.retain(|f| only.contains(f));
    }
    println!(
        "{} level files, {} run; {} members named by the table; load average {}",
        all,
        files.len(),
        names.len(),
        load_average()
    );
    // What the registry holds, and what the shared files name past it.
    let registry: HashSet<MemberId> = TABLES
        .iter()
        .flat_map(|t| t.iter())
        .map(|r| MemberId::named(r.namespace, r.name))
        .collect();
    let absent = |named: &BTreeMap<MemberId, u64>| -> BTreeMap<MemberId, u64> {
        named
            .iter()
            .filter(|(id, _)| !registry.contains(id) && Builtin::of(**id).is_none())
            .map(|(id, n)| (*id, *n))
            .collect()
    };
    for (unit, graph) in [("global.bld", &gg), ("init_setup.bld", &ig)] {
        let a = absent(&members_named(graph));
        println!(
            "SHARED\t{unit}\t{} absent members named\t{} references",
            a.len(),
            a.values().sum::<u64>()
        );
    }
    // Absent member -> (levels naming it, references).
    let mut statics: BTreeMap<MemberId, (usize, u64)> = BTreeMap::new();
    let started = Instant::now();
    // Gap -> per level (first phase and tick, placement, script).
    let mut ranking: BTreeMap<String, Vec<Hit>> = BTreeMap::new();
    let mut first_stops: BTreeMap<String, usize> = BTreeMap::new();
    let mut op_levels: BTreeMap<&'static str, (usize, u64)> = BTreeMap::new();
    let (mut linked, mut clean, mut capped, mut no_scripts) = (0, 0, 0, 0);
    let mut level_213_clean = None;
    for file in &files {
        let t = Instant::now();
        let bytes = level_bld(&usr.join(format!("level/{file}.bld")));
        let li = match Igz::parse(&bytes) {
            Ok(i) => i,
            Err(e) => {
                println!("LINK\t{file}\tFAIL\t-\t-\t-\t-\tparse: {e}");
                continue;
            }
        };
        let lg = match Graph::read(&li) {
            Ok(g) => g,
            Err(e) => {
                println!("LINK\t{file}\tFAIL\t-\t-\t-\t-\tgraph: {e}");
                continue;
            }
        };
        let units = [
            Unit {
                name: "permanent/bootstrap.bld",
                role: Role::Bootstrap,
                igz: &bi,
                graph: &bg,
            },
            Unit {
                name: "permanent/global.bld",
                role: Role::Global,
                igz: &gi,
                graph: &gg,
            },
            Unit {
                name: "character/init_setup.bld",
                role: Role::Other,
                igz: &ii,
                graph: &ig,
            },
            Unit {
                name: file,
                role: Role::Other,
                igz: &li,
                graph: &lg,
            },
        ];
        let program = match Program::link(&units) {
            Ok(p) => Arc::new(p),
            Err(e) => {
                println!(
                    "LINK\t{file}\tFAIL\t-\t-\t-\t-\t{}",
                    one_line(&e.to_string())
                );
                continue;
            }
        };
        linked += 1;
        let named = absent(&members_named(&lg));
        for (id, n) in &named {
            let e = statics.entry(*id).or_default();
            e.0 += 1;
            e.1 += n;
        }
        let (lobjs, lrun) = placements(3, &li);
        let level_scripts = program.scripts.iter().filter(|s| s.unit == 3).count();
        let level = Level {
            program,
            objects: gobjs.iter().chain(&iobjs).chain(&lobjs).copied().collect(),
            running: grun.iter().chain(&irun).chain(&lrun).cloned().collect(),
            names: &names,
        };
        println!(
            "LINK\t{file}\tok\t{level_scripts}\t{}\t{}\t{:.2}\t",
            level.program.ops.len(),
            lrun.len(),
            t.elapsed().as_secs_f64()
        );
        if level_scripts == 0 {
            no_scripts += 1;
        }
        let mut filled = BTreeSet::new();
        let mut out = HashSet::new();
        let mut found: Vec<Found> = Vec::new();
        let mut host_ops = BTreeMap::new();
        let mut ended = "cap";
        for run in 0..CAP {
            let mut host = NullHost {
                generator: Generator::seeded(5489),
                ops: BTreeMap::new(),
            };
            let ran = level.run(&filled, &out, &mut host);
            host_ops = host.ops;
            match ran {
                Ran::Clean => {
                    ended = "clean";
                    break;
                }
                Ran::Stopped(stop) => {
                    let h = &stop.halt;
                    println!(
                        "STOP\t{file}\t{run}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}",
                        stop.phase,
                        stop.tick,
                        h.placement_name,
                        h.script,
                        h.position,
                        op_name(h.op),
                        one_line(&gap_key(h)),
                        one_line(&stop_detail(&level.program, h, &names))
                    );
                    found.push(Found {
                        gap: gap_key(h),
                        phase: stop.phase,
                        tick: stop.tick,
                        placement: h.placement_name.clone(),
                        script: h.script.clone(),
                    });
                    let member = match &h.reason {
                        Reason::UnmodelledMember(m) => member_of(m, &names),
                        _ => None,
                    };
                    match (member, h.placement) {
                        (Some(id), _) if filled.insert(id) => {}
                        (_, Some(p)) if out.insert(p) => {}
                        _ => {
                            ended = "stuck";
                            break;
                        }
                    }
                }
            }
        }
        match ended {
            "clean" => clean += 1,
            _ => capped += 1,
        }
        if file == "level_213_tutotial" {
            level_213_clean = Some(ended == "clean");
        }
        if let Some(f) = found.first() {
            *first_stops.entry(f.gap.clone()).or_default() += 1;
        }
        let mut seen = HashSet::new();
        for f in &found {
            if seen.insert(f.gap.clone()) {
                ranking.entry(f.gap.clone()).or_default().push(Hit {
                    level: file.clone(),
                    phase: f.phase,
                    tick: f.tick,
                    placement: f.placement.clone(),
                    script: f.script.clone(),
                });
            }
        }
        for (class, n) in &host_ops {
            let e = op_levels.entry(class).or_default();
            e.0 += 1;
            e.1 += n;
        }
        println!(
            "LEVEL\t{file}\t{}\t{}\t{}\t{ended}\t{:.2}",
            filled.len(),
            out.len(),
            found.len(),
            t.elapsed().as_secs_f64()
        );
        println!(
            "OPS\t{file}\t{}",
            host_ops
                .iter()
                .map(|(c, n)| format!("{c}={n}"))
                .collect::<Vec<_>>()
                .join(",")
        );
    }
    // Ranked: levels reaching it, then the earliest (phase, tick).
    let mut rows: Vec<(&String, &Vec<Hit>)> = ranking.iter().collect();
    rows.sort_by(|a, b| {
        let early = |v: &Vec<Hit>| {
            v.iter()
                .map(|r| (r.phase, r.tick))
                .min()
                .unwrap_or(('Z', 0))
        };
        b.1.len()
            .cmp(&a.1.len())
            .then_with(|| early(a.1).cmp(&early(b.1)))
            .then_with(|| a.0.cmp(b.0))
    });
    println!("RANK\tgap\tlevels\tfirst stop in\tearliest\tmedian first\texample");
    for (gap, levels) in &rows {
        let mut ticks: Vec<(char, u32)> = levels.iter().map(|r| (r.phase, r.tick)).collect();
        ticks.sort();
        let (ep, et) = ticks[0];
        let (mp, mt) = ticks[ticks.len() / 2];
        let ex = &levels[0];
        println!(
            "RANK\t{}\t{}\t{}\t{ep}{et}\t{mp}{mt}\t{} ({}, {})",
            one_line(gap),
            levels.len(),
            first_stops.get(*gap).copied().unwrap_or(0),
            ex.level,
            ex.placement,
            ex.script
        );
    }
    // Every absent member the levels' own scripts name, whether or not a
    // run reached it: by levels naming it, then references.
    let mut srows: Vec<(&MemberId, &(usize, u64))> = statics.iter().collect();
    srows.sort_by(|a, b| b.1.0.cmp(&a.1.0).then(b.1.1.cmp(&a.1.1)));
    println!("STATIC\tmember\tlevels naming\treferences\tlevels a run reached it in");
    for (id, (levels, refs)) in &srows {
        let name = member_name(**id, &names);
        let reached = ranking.get(&format!("member {name}")).map_or(0, Vec::len);
        println!("STATIC\t{name}\t{levels}\t{refs}\t{reached}");
    }
    for (class, (levels, n)) in &op_levels {
        println!("WORLDOP\t{class}\t{levels} levels\t{n} asks");
    }
    println!(
        "TOTAL\t{} run\t{linked} linked\t{clean} clean\t{capped} cut\t{no_scripts} without scripts\t{} gaps\t{:.1} s\tload average {}",
        files.len(),
        ranking.len(),
        started.elapsed().as_secs_f64(),
        load_average()
    );
    if only.is_none() {
        assert_eq!(all, 143, "the disc's level files");
        assert_eq!(linked, all, "every level links");
    }
    if let Some(clean) = level_213_clean {
        println!("level 213's chain ended clean: {clean}");
    }
}
