//! AH.14's disc test: Giants' toy table built by its own scripts on the VM,
//! in run-time order, as aj-vm's research tool built it. Ignored by default;
//! it reads `GUAC_GIANTS_DUMP` like guac-formats' disc tests:
//!
//! `cargo test -p guac-script --release --test giants_toys -- --ignored --nocapture`

mod common;

use std::sync::Arc;
use std::time::Instant;

use common::usrdir;
use guac_formats::exports::Exports;
use guac_formats::iga::Archive;
use guac_formats::igz::Igz;
use guac_formats::script::Graph;
use guac_formats::toys::{DEBUG_TOY, ToyTable};
use guac_script::{
    Access, Answer, Config, Host, HostFault, Kind, MemberSpec, Mode, OpCall, Program, Role, Unit,
    Value, Vm,
};

/// The toy macro `Init_Setup.ai` calls.
const SET_UP: &str = "Portal - Set Up Object Level Loads";

/// The table's run-time order past the item codes' (aj-vm): the old
/// legendaries in alt-lookup order, then DEBUG.
const OLD_LEGENDARIES: [i32; 5] = [404, 28, 416, 419, 430];

fn level_bld(path: &std::path::Path) -> Vec<u8> {
    let bytes = std::fs::read(path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    let arc = Archive::parse(&bytes).unwrap();
    let entry = arc.find("level.bld").expect("level.bld");
    arc.read(entry).unwrap().into_owned()
}

/// A host with no members: every member the run reaches is the VM's own
/// or a listed value cell.
struct NoEngine;

impl Host for NoEngine {
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
        0
    }
}

/// The engine members `Init_Setup.ai`'s first tick reaches past the VM's
/// own, each a value cell of its registered type and bounds (aj-vm's live
/// member table): viewport, sprite and colour members for its UI set-up,
/// sounds, the console's language and pause state, and a text key's
/// converter. The platform reads PS 3, the data being the PS3 build's.
const TICK_VALUE_CELLS: &[(&str, &str, Kind, f32, f32)] = &[
    ("viewportinfo", "default", Kind::Reference, 0.0, 0.0),
    (
        "viewportinfo",
        "container sprite",
        Kind::Reference,
        0.0,
        0.0,
    ),
    ("system", "hardware platform", Kind::Int, 0.0, 4.0),
    ("system", "pause state", Kind::Int, 0.0, 1.0),
    ("system", "save name", Kind::Reference, 0.0, 0.0),
    ("system", "game language", Kind::Int, 0.0, 22.0),
    ("system", "console language", Kind::Int, 0.0, 22.0),
    ("stringinfo", "string converter", Kind::Reference, 0.0, 0.0),
    ("stringconverter", "text", Kind::Reference, 0.0, 0.0),
    ("stringconverter", "value set", Kind::Reference, 0.0, 0.0),
    ("spriteinfo", "visible", Kind::Int, 0.0, 2.0),
    ("spriteinfo", "scale", Kind::Reference, 0.0, 0.0),
    ("spriteinfo", "width", Kind::Float, 0.0, 0.0),
    ("spriteinfo", "height", Kind::Float, 0.0, 0.0),
    ("spriteinfo", "color", Kind::Color, 0.0, 0.0),
    ("spriteinfo", "alignment", Kind::Int, 0.0, 8.0),
    ("spriteinfo", "align to parent", Kind::Int, 0.0, 8.0),
    ("screenmeasurement", "x", Kind::Float, 0.0, 0.0),
    ("screenmeasurement", "y", Kind::Float, 0.0, 0.0),
    (
        "spritematrixmeasurement",
        "attach node",
        Kind::Reference,
        0.0,
        0.0,
    ),
    ("spritematrixmeasurement", "id", Kind::Int, 0.0, 0.0),
    ("colormeasurement", "alpha", Kind::Int, 0.0, 255.0),
    ("colormeasurement", "red", Kind::Int, 0.0, 255.0),
    ("colormeasurement", "green", Kind::Int, 0.0, 255.0),
    ("colormeasurement", "blue", Kind::Int, 0.0, 255.0),
    ("soundinfo", "category templates", Kind::Reference, 0.0, 0.0),
];

/// The first tick's world ops find no target (aj-vm's lenient run did the
/// same): a branch op skips its block.
struct NoTarget;

impl Host for NoTarget {
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
        0
    }
    fn op(&mut self, _: &OpCall) -> Result<Answer, HostFault> {
        Ok(Answer::Skip)
    }
}

fn tick_config() -> Config {
    let mut config = Config::new(Mode::Strict);
    for &(namespace, name, kind, min, max) in TICK_VALUE_CELLS {
        let mut spec = MemberSpec::new(kind, min, max);
        if (namespace, name) == ("system", "hardware platform") {
            spec = spec.with_default(Value::Int(1));
        }
        config.members.value_cell(namespace, name, spec);
    }
    config
}

struct Files {
    init_setup: Vec<u8>,
    global: Vec<u8>,
    bootstrap: Vec<u8>,
}

impl Files {
    fn read() -> Self {
        let usr = usrdir();
        Self {
            init_setup: level_bld(&usr.join("character/init_setup.bld")),
            global: level_bld(&usr.join("permanent/global.bld")),
            bootstrap: level_bld(&usr.join("permanent/bootstrap.bld")),
        }
    }
}

/// What a row of `toy level loads` holds, by the static reader's keys.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Row {
    item: i32,
    level: Option<u32>,
    name: Option<u32>,
    element: i32,
}

fn row(vm: &Vm, v: Value) -> Row {
    let key = |f: &str| match vm.field(v, f) {
        Some(Value::Object(o)) => Some(o.index),
        _ => None,
    };
    let int = |f: &str| match vm.field(v, f) {
        Some(Value::Int(i)) => i,
        other => panic!("{f}: {other:?}"),
    };
    Row {
        item: int("item code"),
        level: key("level"),
        name: key("name"),
        element: int("element"),
    }
}

/// Checks the VM's `toy data` against the static reader (AE.2) and returns
/// the figure count by the roster's rule.
fn check_table(vm: &mut Vm, table: &ToyTable) -> usize {
    let g = vm.program().global_named("toy data").expect("toy data");
    let data = vm.global(g).expect("toy data's cell");
    let set = |vm: &Vm, name: &str| -> Vec<Value> {
        let v = vm
            .field(data, name)
            .unwrap_or_else(|| panic!("no field {name:?}"));
        vm.set(v)
            .unwrap_or_else(|| panic!("{name:?} isn't a set"))
            .to_vec()
    };
    // Run-time order: item codes ascending, the old legendaries, DEBUG.
    let rows: Vec<Row> = set(vm, "toy level loads")
        .into_iter()
        .map(|v| row(vm, v))
        .collect();
    let mut want: Vec<i32> = table
        .toys
        .iter()
        .map(|t| t.item_code)
        .filter(|c| *c != DEBUG_TOY && !OLD_LEGENDARIES.contains(c))
        .collect();
    want.sort();
    want.extend(OLD_LEGENDARIES);
    want.push(DEBUG_TOY);
    let got: Vec<i32> = rows.iter().map(|r| r.item).collect();
    assert_eq!(got, want, "toy level loads in run-time order");
    for r in &rows {
        let t = table.toys.iter().find(|t| t.item_code == r.item).unwrap();
        assert_eq!(r.level, Some(t.level_load as u32), "{} level load", r.item);
        assert_eq!(r.name, Some(t.name as u32), "{} name", r.item);
        assert_eq!(r.element, t.element, "{} element", r.item);
    }
    // Each set the flags and the after-return code fill.
    for name in [
        "character level loads",
        "magic item level loads",
        "pet level loads",
        "sky1 character loads",
        "sky1 characters with repose",
        "giant loads",
    ] {
        let mut got: Vec<i32> = set(vm, name).into_iter().map(|v| row(vm, v).item).collect();
        let mut want: Vec<i32> = table
            .toys
            .iter()
            .filter(|t| t.in_set(name))
            .map(|t| t.item_code)
            .collect();
        got.sort();
        want.sort();
        assert_eq!(got, want, "{name}");
    }
    let lightcore: Vec<Option<u32>> = set(vm, "display as lightcore")
        .into_iter()
        .map(|v| row(vm, v).level)
        .collect();
    let want: Vec<Option<u32>> = table
        .lightcore_loads
        .iter()
        .map(|k| Some(*k as u32))
        .collect();
    assert_eq!(lightcore, want, "display as lightcore");
    // The legendary alts: each one's toy, deco and level load.
    let alts = set(vm, "subtype lookup - legendary");
    assert_eq!(alts.len(), table.alts.len(), "subtype lookup - legendary");
    for (v, a) in alts.iter().zip(&table.alts) {
        let normal = vm.field(*v, "normal").unwrap();
        assert_eq!(row(vm, normal).item, table.toys[a.toy].item_code);
        assert_eq!(vm.field(*v, "match deco id"), Some(Value::Int(a.deco)));
        assert_eq!(
            vm.field(*v, "alt level load"),
            Some(Value::Object(guac_script::Obj {
                unit: 1,
                index: a.level_load as u32
            }))
        );
    }
    let alt_chars = set(vm, "alt character lookups");
    assert_eq!(
        alt_chars.len(),
        table.alt_characters.len(),
        "alt character lookups"
    );
    for (v, a) in alt_chars.iter().zip(&table.alt_characters) {
        let n = vm.field(*v, "normal").unwrap();
        let alt = vm.field(*v, "alt").unwrap();
        assert_eq!(row(vm, n).item, table.toys[a.normal].item_code);
        assert_eq!(row(vm, alt).item, table.toys[a.alt].item_code);
    }
    // The roster's rule: each toy but DEBUG, its Series 2 row, its LightCore
    // row and each legendary alt.
    (rows.len() - 1) + set(vm, "sky1 characters with repose").len() + lightcore.len() + alts.len()
}

#[test]
#[ignore = "reads the Giants dump (GUAC_GIANTS_DUMP)"]
fn the_toy_table_runs_on_the_vm_in_run_time_order() {
    let files = Files::read();
    let parsed = Instant::now();
    let init = Igz::parse(&files.init_setup).unwrap();
    let global = Igz::parse(&files.global).unwrap();
    let boot = Igz::parse(&files.bootstrap).unwrap();
    let (gi, gg, gb) = (
        Graph::read(&init).unwrap(),
        Graph::read(&global).unwrap(),
        Graph::read(&boot).unwrap(),
    );
    let parse_time = parsed.elapsed();
    let linked = Instant::now();
    let program = Arc::new(
        Program::link(&[
            Unit {
                name: "character/init_setup.bld",
                role: Role::Other,
                igz: &init,
                graph: &gi,
            },
            Unit {
                name: "permanent/global.bld",
                role: Role::Global,
                igz: &global,
                graph: &gg,
            },
            Unit {
                name: "permanent/bootstrap.bld",
                role: Role::Bootstrap,
                igz: &boot,
                graph: &gb,
            },
        ])
        .unwrap(),
    );
    let link_time = linked.elapsed();
    println!(
        "parsed in {parse_time:?}, linked in {link_time:?}: {} scripts, {} ops, {} operands, {} definitions, {} file variables",
        program.scripts.len(),
        program.ops.len(),
        program.operands.len(),
        program.defs.len(),
        program.globals.len()
    );
    let exports = Exports::new(&boot, &global).unwrap();
    let table = ToyTable::read(&init, &global, &exports).unwrap();

    // The macro alone, strict, nothing but the VM's own members.
    let mut vm = Vm::new(program.clone(), Config::new(Mode::Strict));
    let t = Instant::now();
    let ops = vm
        .call(None, SET_UP, &mut NoEngine)
        .unwrap_or_else(|h| panic!("{h}"));
    let took = t.elapsed();
    println!("{SET_UP}: {ops} op executions in {took:?}");
    let figures = check_table(&mut vm, &table);
    assert_eq!(figures, 123, "the roster's figures");
    let mut reached: Vec<String> = vm
        .reached()
        .iter()
        .map(|m| vm.config().members.name(*m))
        .collect();
    reached.sort();
    println!("members reached: {reached:?}");

    // Init_Setup.ai's whole first tick, strict, every member it reaches past
    // the VM's own a listed value cell; timed on fresh VMs.
    let script = program
        .script_named("Init_Setup.ai")
        .expect("Init_Setup.ai");
    let mut times = Vec::new();
    let mut first = None;
    for _ in 0..21 {
        let mut vm = Vm::new(program.clone(), tick_config());
        let id = vm.add(1, "Init_Setup", script);
        let t = Instant::now();
        vm.tick(id, &mut NoTarget).unwrap_or_else(|h| panic!("{h}"));
        times.push(t.elapsed());
        if first.is_none() {
            first = Some((vm, id));
        }
    }
    let (mut vm, id) = first.unwrap();
    let ops = vm.executed();
    times.sort();
    let median = times[times.len() / 2];
    println!(
        "Init_Setup.ai's first tick: {ops} op executions, {} macros called {} times; min {:?}, median {:?}, max {:?} over {} runs; {:.1} M op executions a second at the median",
        vm.calls().len(),
        vm.calls().values().sum::<u64>(),
        times[0],
        median,
        times[times.len() - 1],
        times.len(),
        ops as f64 / median.as_secs_f64() / 1e6
    );
    assert!(
        ops < guac_script::DEFAULT_BUDGET as u64 / 2,
        "the budget sits well above the largest tick"
    );
    for line in vm.log() {
        println!("log: {}", line.text);
    }
    let figures = check_table(&mut vm, &table);
    assert_eq!(figures, 123, "the roster's figures, from the tick");
    // The save offsets the tick computes: 6 bits a toy, in table order.
    let g = vm.program().global_named("toy data").unwrap();
    let data = vm.global(g).unwrap();
    let loads = vm.field(data, "toy level loads").unwrap();
    let rows: Vec<Value> = vm.set(loads).unwrap().to_vec();
    for (i, r) in rows.iter().enumerate() {
        assert_eq!(
            vm.field(*r, "save offset"),
            Some(Value::Int(6 * i as i32)),
            "toy {i}"
        );
    }
    // Ticks 2 and 3 run PRESCRIPT and the behaviour only.
    let before = vm.executed();
    vm.tick(id, &mut NoTarget).unwrap_or_else(|h| panic!("{h}"));
    println!("tick 2: {} op executions", vm.executed() - before);
}
