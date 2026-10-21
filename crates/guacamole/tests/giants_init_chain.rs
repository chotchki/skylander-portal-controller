//! AH.16's disc tests: the init chain on the VM over Giants' own scripts
//! with guacamole's host. `global.bld`'s 17 running placements for their
//! first tick, `init_setup.bld`'s running placements and level 213's
//! definition and enemy system every tick, linked as the TV links the
//! level's script part (its five entries, read here straight off the dump).
//! Ignored by default; they read `GUAC_GIANTS_DUMP` (process env, else
//! `.env.dev`):
//!
//! `cargo test -p guacamole --release --test giants_init_chain -- --ignored --nocapture --test-threads=1`
//!
//! What they hold the run to is the oracle's reads, never the disc's own
//! literals: AH.4's live reads of AF.8's fresh state (level 213 at medium:
//! the enemy stats list, the level's cells), ah-combat's live read of
//! `global.bld`'s boot tuning, and AH.2's static reads of the progression
//! tables.

use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use bevy::prelude::*;
use guac_formats::exports::Exports;
use guac_formats::igz::builder::IgzBuilder;
use guac_formats::igz::{Igz, Location};
use guac_formats::progression::{SaveKind, SaveTable};
use guac_formats::script::builder::{El, Writer};
use guac_formats::script::{Class, Graph};
use guac_script::members::{INIT_CHAIN, Keeper, Registered};
use guac_script::{
    Builtin, Config, Elem, Generator, Halt, Handle, Ins, MemberId, Mode, Operand, OperandId,
    PlacementId, Precision, Program, Role, Unit, Value, Vm, Whitelist,
};
use guacamole::scripts::{
    self, EVERY_TICK, Files, GLOBAL_FIRST_TICKS, ScriptRuntime, ScriptWorld, WorldHost,
    headless_handles, headless_objects, headless_spawner,
};

/// `key` from the environment, else from the repo's `.env.dev`.
fn env_or_dotenv(key: &str) -> Option<String> {
    if let Ok(v) = std::env::var(key) {
        return Some(v);
    }
    let dotenv = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../.env.dev");
    let text = std::fs::read_to_string(dotenv).ok()?;
    text.lines().find_map(|line| {
        let (k, v) = line.trim().split_once('=')?;
        (k.trim() == key).then(|| v.trim().trim_matches('"').to_owned())
    })
}

fn usrdir() -> PathBuf {
    let root = env_or_dotenv("GUAC_GIANTS_DUMP")
        .expect("set GUAC_GIANTS_DUMP (env or .env.dev) to the Giants dump");
    let usr = Path::new(&root).join("PS3_GAME/USRDIR");
    assert!(usr.is_dir(), "no PS3_GAME/USRDIR under {root}");
    usr
}

/// The script part's archives in link order, as `level_scripts` links them.
const ARCHIVES: [(&str, Role); 5] = [
    ("permanent/bootstrap.bld", Role::Bootstrap),
    ("permanent/global.bld", Role::Global),
    ("character/init_setup.bld", Role::Other),
    ("character/009_eruptor.bld", Role::Other),
    ("level/level_213_tutotial.bld", Role::Other),
];

/// The units whose placements run, in the order they run.
const ORDER: [u16; 3] = [1, 2, 4];

/// Each archive's decompressed `level.bld`, read off the dump.
fn part() -> Vec<Vec<u8>> {
    let usr = usrdir();
    ARCHIVES
        .iter()
        .map(|(a, _)| guac_extract::scripts::entry(&usr, a).unwrap_or_else(|e| panic!("{a}: {e}")))
        .collect()
}

/// `bytes` linked with `roles`, and the host's files of `run`'s units.
fn link(bytes: &[&[u8]], roles: &[Role], run: &[u16]) -> (Arc<Program>, Files) {
    let igzs: Vec<Igz<'_>> = bytes.iter().map(|b| Igz::parse(b).unwrap()).collect();
    let graphs: Vec<Graph<'_>> = igzs.iter().map(|i| Graph::read(i).unwrap()).collect();
    let units: Vec<Unit<'_, '_>> = igzs
        .iter()
        .zip(&graphs)
        .zip(roles)
        .map(|((igz, graph), role)| Unit {
            name: "part",
            role: *role,
            igz,
            graph,
        })
        .collect();
    let program = Arc::new(Program::link(&units).unwrap());
    let units: Vec<(u16, &Igz<'_>)> = run.iter().map(|&u| (u, &igzs[u as usize])).collect();
    (program, Files::read(&units).unwrap())
}

fn link_part(part: &[Vec<u8>]) -> (Arc<Program>, Files) {
    let bytes: Vec<&[u8]> = part.iter().map(Vec::as_slice).collect();
    let roles: Vec<Role> = ARCHIVES.iter().map(|(_, r)| *r).collect();
    link(&bytes, &roles, &ORDER)
}

/// A headless run: the VM, the host's world and generator, the handles.
struct Run {
    vm: Vm,
    world: ScriptWorld,
    generator: Generator,
    next: Handle,
    names: HashMap<Handle, String>,
    first: Duration,
    rest: Duration,
    steps: u32,
}

impl Run {
    fn new(program: Arc<Program>, files: &Files, order: &[u16], config: Config) -> Self {
        Self::seeded(program, files, order, config, 5489)
    }

    fn seeded(
        program: Arc<Program>,
        files: &Files,
        order: &[u16],
        config: Config,
        seed: u32,
    ) -> Self {
        let handles = headless_handles(files);
        let objects = headless_objects(&handles);
        let vm = scripts::init_chain(program, files, &handles, order, config);
        Self {
            vm,
            world: ScriptWorld::new(files, &handles, objects),
            generator: Generator::seeded(seed),
            next: objects[6] + 1,
            names: files
                .placed
                .iter()
                .map(|p| (handles[&p.obj], p.name.clone()))
                .collect(),
            first: Duration::ZERO,
            rest: Duration::ZERO,
            steps: 0,
        }
    }

    /// Runs `f` with the host, as a step does.
    fn with_host<T>(&mut self, f: impl FnOnce(&mut Vm, &mut WorldHost<'_, '_>) -> T) -> T {
        let mut spawn = headless_spawner(self.next);
        let t = {
            let mut host = WorldHost {
                world: &mut self.world,
                generator: &mut self.generator,
                spawn: &mut spawn,
            };
            f(&mut self.vm, &mut host)
        };
        self.next = spawn(Value::Null);
        t
    }

    /// `n` steps, each the VM's step then the host's end of step.
    fn step(&mut self, n: u32) -> Result<(), Halt> {
        for _ in 0..n {
            let started = Instant::now();
            let r = self.with_host(|vm, host| vm.step(host));
            self.world.end_step();
            let took = started.elapsed();
            if self.steps == 0 {
                self.first = took;
            } else {
                self.rest += took;
            }
            self.steps += 1;
            assert!(r?.stopped.is_empty(), "strict mode stops by halting");
        }
        Ok(())
    }

    fn placement(&self, name: &str) -> PlacementId {
        let (&h, _) = self
            .names
            .iter()
            .find(|(_, n)| n.as_str() == name)
            .unwrap_or_else(|| panic!("no placement {name}"));
        self.vm
            .placement_of(h)
            .unwrap_or_else(|| panic!("{name} doesn't run"))
    }

    fn name_of(&self, v: Value) -> String {
        match v {
            Value::Entity(h) => self
                .names
                .get(&h)
                .cloned()
                .unwrap_or_else(|| format!("{h:#x}")),
            other => format!("{other:?}"),
        }
    }

    /// A file variable by name, first unit first; `None` when no op of
    /// the part names it.
    fn try_global(&mut self, name: &str) -> Option<Value> {
        let g = self.vm.program().global_named(name)?;
        self.vm.global(g)
    }

    fn global(&mut self, name: &str) -> Value {
        self.try_global(name)
            .unwrap_or_else(|| panic!("no file variable {name}"))
    }

    /// `v`, a file object made into its variable's value.
    fn resolve(&mut self, v: Value) -> Value {
        if let Value::Object(o) = v
            && let Some(g) = self.vm.program().global_of(o)
        {
            return self.vm.global(g).unwrap();
        }
        self.vm.value(v)
    }

    fn field(&mut self, v: Value, name: &str) -> Value {
        let v = self.resolve(v);
        self.vm
            .field(v, name)
            .unwrap_or_else(|| panic!("no field {name} on {v:?}"))
    }

    fn elements(&mut self, v: Value) -> Vec<Value> {
        let v = self.resolve(v);
        self.vm
            .set(v)
            .unwrap_or_else(|| panic!("{v:?} isn't a set"))
            .to_vec()
    }
}

/// The init chain over the part, run strict for `ticks`.
fn run_chain(part: &[Vec<u8>], config: Config, ticks: u32) -> Run {
    let (program, files) = link_part(part);
    let mut run = Run::new(program, &files, &ORDER, config);
    run.step(ticks).unwrap_or_else(|h| panic!("{h}"));
    run
}

fn load() -> String {
    std::process::Command::new("uptime")
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_owned())
        .unwrap_or_default()
}

fn bits(v: Value) -> String {
    match v {
        Value::Float(f) => format!("{f} (0x{:08X})", f.to_bits()),
        Value::Double(d) => format!("{d} (double)"),
        other => format!("{other:?}"),
    }
}

/// A table member's reads and writes over the run.
fn uses(vm: &Vm, r: &Registered) -> (u64, u64) {
    vm.uses()
        .get(&MemberId::named(r.namespace, r.name))
        .copied()
        .unwrap_or((0, 0))
}

#[test]
#[ignore = "reads the Giants dump (GUAC_GIANTS_DUMP)"]
fn the_init_chain_runs_600_ticks_strict_on_the_members_it_reaches() {
    println!("load at start: {}", load());
    let wall = Instant::now();
    let part = part();
    let started = Instant::now();
    let (program, files) = link_part(&part);
    println!(
        "read and linked in {:?}: {} scripts, {} ops; {} actor placements ({} running), {} placement ids",
        started.elapsed(),
        program.scripts.len(),
        program.ops.len(),
        files.placed.len(),
        files
            .placed
            .iter()
            .filter(|p| p.activity_state == 4)
            .count(),
        files.ids.len()
    );
    let mut run = Run::new(
        program.clone(),
        &files,
        &ORDER,
        scripts::config(Mode::Strict),
    );
    run.step(600).unwrap_or_else(|h| panic!("{h}"));
    // The generator's seed is the clock's in the TV: other seeds reach the
    // same members.
    for seed in [1, 2, 0x1234_5678, guac_script::session_seed()] {
        let mut other = Run::seeded(
            program.clone(),
            &files,
            &ORDER,
            scripts::config(Mode::Strict),
            seed,
        );
        other
            .step(600)
            .unwrap_or_else(|h| panic!("seed {seed}: {h}"));
        assert_eq!(other.vm.reached(), run.vm.reached(), "seed {seed}");
    }
    println!(
        "600 ticks: the first {:?}, the other 599 {:?} ({:?} a tick); {} ops executed",
        run.first,
        run.rest,
        run.rest / 599,
        run.vm.executed()
    );

    // Every member a path named is a path builtin or in the table, and the
    // table holds nothing the run didn't reach.
    let table: HashMap<MemberId, &Registered> = INIT_CHAIN
        .iter()
        .map(|r| (MemberId::named(r.namespace, r.name), r))
        .collect();
    let mut own = Vec::new();
    let mut by_keeper: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for m in run.vm.reached() {
        if let Some(r) = table.get(m) {
            by_keeper
                .entry(format!("{:?}", r.keeper))
                .or_default()
                .push(r.path());
            continue;
        }
        let b = Builtin::of(*m).unwrap_or_else(|| {
            panic!(
                "{} reached, not in the table",
                run.vm.config().members.name(*m)
            )
        });
        own.push(b.name());
    }
    own.sort_unstable();
    let unreached: Vec<String> = INIT_CHAIN
        .iter()
        .filter(|r| {
            !run.vm
                .reached()
                .contains(&MemberId::named(r.namespace, r.name))
        })
        .map(Registered::path)
        .collect();
    assert!(unreached.is_empty(), "never reached: {unreached:?}");
    let reached = run.vm.reached().len();
    println!(
        "members reached {reached}: {} path builtins {own:?}",
        own.len()
    );
    for (k, v) in &mut by_keeper {
        v.sort();
        println!("  {k} {}: {v:?}", v.len());
    }
    let mut cells_read = 0;
    for r in INIT_CHAIN.iter().filter(|r| r.keeper != Keeper::Vm) {
        let (reads, writes) = uses(&run.vm, r);
        if r.keeper == Keeper::Cell && reads > 0 {
            cells_read += 1;
        }
        println!(
            "  {:?} {}: reads {reads}, writes {writes}",
            r.keeper,
            r.path()
        );
    }
    let cells = INIT_CHAIN
        .iter()
        .filter(|r| r.keeper == Keeper::Cell)
        .count();
    println!("value cells {cells}, {cells_read} read by a whitelisted branch; AH.34's ceiling 324");
    assert!(reached <= 324, "past AH.34's ceiling: {reached}");

    // Who ran: of global.bld's 17, 13 held after their first tick and 4
    // off by their own removes in it; the init setup and the level's
    // definition off by theirs; the enemy system idle.
    let held: Vec<String> = run
        .vm
        .held()
        .iter()
        .map(|h| run.name_of(Value::Entity(*h)))
        .collect();
    assert_eq!(held.len(), 13, "{held:?}");
    for name in [
        "PS3SpecificSetup",
        "360PS3GestureManager",
        "Minion_Tuning",
        "360PS3_PointerSetup",
        "A_Init_Setup_PlatSpecific_360",
        "A_Init_Setup_PlatSpecific_PS3",
        "A_Init_Setup_PlatSpecific_Wii",
        "A_Init_Setup_PlatSpecific_WiiU",
        "Z_Init_Setup_Macros",
        "Z_Init_Setup_Z_Achievements",
        "_Level_Definition",
    ] {
        let id = run.placement(name);
        assert!(run.vm.removed(id), "{name} still runs");
    }
    let enemy = run.placement("EnemySystem_");
    assert_eq!(run.vm.behaviour(enemy), Some("idle"));
    println!(
        "held {}, removed 11, inert {}, the enemy system idle; levels asked for {:?}; clones {}",
        held.len(),
        run.vm.inert().len(),
        run.world.requests,
        run.world.clones().len()
    );
    let mut lines: BTreeMap<&str, u32> = BTreeMap::new();
    for l in run.vm.log() {
        if !l.text.contains("off the whitelist") {
            *lines.entry(l.text.as_str()).or_default() += 1;
        }
    }
    for (l, n) in lines {
        println!("  log {n}x {l}");
    }
    println!("wall {:?}; load at end: {}", wall.elapsed(), load());
}

/// AH.4's enemy stats list, level 213 at medium: each float as the live
/// single printed, which parses back to its bits.
const LIVE_STATS: [(&str, i32, &str); 4] = [
    ("PandoranGrunt", 59, "128.33334"),
    ("PandoranGruntSimple", 33, "128.33334"),
    ("Enemy_ChompySpawner_Legs", 61, "20"),
    ("Enemy_Chompy", 1, "4"),
];

/// AH.4's level cells (`level-cells-f0.json`).
const LIVE_CELLS: [(&str, &str); 10] = [
    ("Enemy_Chompy_DirectDMG", "11"),
    ("Enemy_Chompy_DoTDMG", "5"),
    ("Enemy_Chompy_HP", "3"),
    ("PandoranGrunt_DMG", "36"),
    ("PandoranGrunt_DMG_Simple", "14.400001"),
    ("PandoranGrunt_HP", "80"),
    ("XP_Value_Near", "128.33334"),
    ("XP_Value_Far", "106.94444"),
    ("XP_Value_HeavyNear", "427.77777"),
    ("Difficulty_Rating_Current_Level", "1"),
];

/// A live print as the cell holds it: an integer prints without a point.
fn live(s: &str) -> Value {
    if s.contains('.') {
        Value::Float(s.parse().unwrap())
    } else {
        Value::Int(s.parse().unwrap())
    }
}

/// Bit-equal: the same kind holding the same bits. A double isn't a
/// single, whatever it rounds to.
fn same(vm: Value, live: Value) -> bool {
    match (vm, live) {
        (Value::Float(a), Value::Float(b)) => a.to_bits() == b.to_bits(),
        (Value::Int(a), Value::Int(b)) => a == b,
        _ => false,
    }
}

/// The level's computed numbers against AH.4's: those that differ.
fn level_numbers(run: &mut Run) -> Vec<String> {
    let mut off = Vec::new();
    let ui = run.placement("Macros_UI");
    let list = run.vm.local(ui, "EnemyStatsList").expect("EnemyStatsList");
    let mut got = Vec::new();
    for e in run.elements(list) {
        let kind = run.field(e, "Enemy Type");
        let kind = run.name_of(kind);
        let (hp, xp) = (run.field(e, "hp"), run.field(e, "xp"));
        println!("  stats {kind}: hp {} xp {}", bits(hp), bits(xp));
        got.push((kind, hp, xp));
    }
    if got.len() != LIVE_STATS.len() {
        off.push(format!("{} stats entries", got.len()));
    }
    for ((kind, hp, xp), (lk, lhp, lxp)) in got.iter().zip(LIVE_STATS) {
        if kind != lk || !same(*hp, Value::Int(lhp)) || !same(*xp, live(lxp)) {
            off.push(format!("{kind} {hp:?} {xp:?}"));
        }
    }
    for (name, l) in LIVE_CELLS {
        // A cell only the enemies' own archives name isn't the chain's:
        // the level file holds it as the oracle read it.
        let Some(v) = run.try_global(name) else {
            println!("  {name}: unchecked, no op of the part names it (live {l})");
            continue;
        };
        println!("  {name}: {} (live {l})", bits(v));
        if !same(v, live(l)) {
            off.push(format!("{name} {v:?}"));
        }
    }
    off
}

#[test]
#[ignore = "reads the Giants dump (GUAC_GIANTS_DUMP)"]
fn level_213s_numbers_at_medium_are_the_oracles_to_the_bit() {
    println!("load at start: {}", load());
    let wall = Instant::now();
    let mut run = run_chain(&part(), scripts::config(Mode::Strict), 10);
    let off = level_numbers(&mut run);
    assert!(off.is_empty(), "not the oracle's: {off:?}");

    // Medium, from the console save the chain clears to its defaults: the
    // difficulty option's 4 bits at bit 2,549 hold 1, as AH.4 read it.
    let save = run.global("console save params");
    let option = run.field(save, "option - difficulty");
    assert_eq!(run.field(option, "bit index"), Value::Int(2549));
    assert_eq!(run.field(option, "num bits"), Value::Int(4));
    let words = run.field(save, "save val set");
    let words = run.elements(words);
    let Value::Int(word) = run.resolve(words[2549 / 32]) else {
        panic!("{:?}", words[2549 / 32])
    };
    assert_eq!((word >> (2549 % 32)) & 0xF, 1, "medium");

    // ah-combat's live read of global.bld's boot tuning: to the bit where
    // its print is exact (whole numbers, halves, quarters).
    for (name, l) in [
        ("HP_Minion_VeryLow", 200.0),
        ("HP_Minion_Low", 240.0),
        ("HP_Minion_Medium", 270.0),
        ("HP_Minion_High", 300.0),
        ("HP_Minion_VeryHigh", 340.0),
        ("HP_Minion_NudgeAmount", 10.0),
        ("HP_Minion_Titan_Low", 400.0),
        ("HP_Minion_Titan_Medium", 430.0),
        ("HP_Minion_Titan_High", 460.0),
        ("Speed_Slow", 21.5),
        ("Speed_Med", 22.25),
        ("Average_Minion_HP", 285.0),
        ("LuckyCharm_SlayerDamBonus", 1.25),
    ] {
        let v = run.global(name);
        assert!(same(v, Value::Float(l)), "{name}: {v:?}");
    }
    for (name, l) in [
        ("Speed_VerySlow", 20),
        ("Speed_Fast", 23),
        ("Speed_VeryFast", 25),
        ("Armor_Buff_Flight", 20),
        ("Tuning_UpgradeDam_GlobalModifier", 2),
        ("HP_Minion_MaxMultiplier", 2),
        ("Tuning_PowerStat_MaxMultiplier", 1),
        ("PvP_Duration_Invulnerability", 8),
    ] {
        let v = run.global(name);
        assert!(same(v, Value::Int(l)), "{name}: {v:?}");
    }
    // No live read to the bit: within the print (6 significant digits, or
    // AH.4's 3 decimals), named unchecked.
    for (name, l, within) in [
        ("Average_Minion_RoF", 2.7, 5e-6),
        ("Armor_Buff_MinionUpgrade", 13.3, 5e-5),
        ("Average_Minion_Damage", 10.1786, 5e-5),
        ("Average_Minion_DPS", 27.4821, 5e-5),
        ("XP_Value_HeavyFar", 427.778, 5e-4),
        ("XP_Value_Wizard", 427.778, 5e-4),
        ("Hazard_DMG_VeryLow", 2.0, 5e-4),
        ("Hazard_DMG_Low", 10.0, 5e-4),
        ("Hazard_DMG_Medium", 25.0, 5e-4),
        ("Hazard_DMG_High", 60.0, 5e-4),
        ("Hazard_DMG_VeryHigh", 96.0, 5e-4),
    ] {
        let v = run.global(name);
        let f = v.as_f32().unwrap_or_else(|| panic!("{name}: {v:?}"));
        assert!((f - l).abs() <= within, "{name}: {}", bits(v));
        println!(
            "  unchecked to the bit: {name} {} (live print {l})",
            bits(v)
        );
    }
    let combat = run.global("combat control data");
    let multiplier = run.field(combat, "accolade xp multiplier");
    assert_eq!(multiplier, Value::Float(1.0));
    println!(
        "  the accolade experience multiplier {} on a fresh save (AF.8's save, one accolade done, reads 1.1)",
        bits(multiplier)
    );
    println!("wall {:?}; load at end: {}", wall.elapsed(), load());
}

#[test]
#[ignore = "reads the Giants dump (GUAC_GIANTS_DUMP)"]
fn computed_in_double_precision_the_numbers_arent_the_oracles() {
    let mut config = scripts::config(Mode::Strict);
    config.precision = Precision::Double;
    let mut run = run_chain(&part(), config, 10);
    let off = level_numbers(&mut run);
    println!("in double precision, off: {off:?}");
    assert!(
        off.iter().any(|o| o.starts_with("XP_Value_Near")),
        "the bit-equal check catches it: {off:?}"
    );
    let near = run.global("XP_Value_Near");
    let Value::Double(d) = near else {
        panic!("{near:?}")
    };
    // Rounded it's the oracle's single: only the bits held tell them apart.
    assert_eq!((d as f32).to_bits(), 0x4300_5556);
    // So with every one of them (AH.16's review): each value off is held
    // as a double that rounds once to the oracle's single bits. On level
    // 213's numbers the check catches double precision by the value held,
    // not by a different rounding (the unit test's (1e8 + 1) − 1e8 is the
    // case where the bits differ).
    let mut doubles = Vec::new();
    for (name, l) in LIVE_CELLS {
        if let Some(Value::Double(d)) = run.try_global(name) {
            let Value::Float(f) = live(l) else {
                panic!("{name}: live {l}")
            };
            assert_eq!((d as f32).to_bits(), f.to_bits(), "{name}: {d}");
            doubles.push(name.to_owned());
        }
    }
    let ui = run.placement("Macros_UI");
    let list = run.vm.local(ui, "EnemyStatsList").expect("EnemyStatsList");
    for (e, (kind, _, lxp)) in run.elements(list).into_iter().zip(LIVE_STATS) {
        if let Value::Double(d) = run.field(e, "xp") {
            assert_eq!((d as f32).to_bits(), lxp.parse::<f32>().unwrap().to_bits());
            doubles.push(format!("{kind} xp"));
        }
    }
    assert_eq!(doubles.len(), off.len(), "{doubles:?} {off:?}");
    println!("each off value a double rounding to the oracle's bits: {doubles:?}");
}

#[test]
#[ignore = "reads the Giants dump (GUAC_GIANTS_DUMP)"]
fn the_experience_table_and_save_params_are_ah2s_reads() {
    let part = part();
    let mut run = run_chain(&part, scripts::config(Mode::Strict), 10);
    // AH.2's static reads of the same files.
    let boot = Igz::parse(&part[0]).unwrap();
    let global = Igz::parse(&part[1]).unwrap();
    let init = Igz::parse(&part[2]).unwrap();
    let exports = Exports::new(&boot, &global).unwrap();
    let table = SaveTable::read(&init, &global, &exports).unwrap();

    let portal = run.global("portal control data");
    let xp = run.field(portal, "level xp table");
    let vm_xp: Vec<u32> = run
        .elements(xp)
        .into_iter()
        .map(|e| match run.resolve(e) {
            Value::Int(i) => i as u32,
            other => panic!("{other:?}"),
        })
        .collect();
    assert_eq!(vm_xp, table.level_xp, "the experience table");
    assert_eq!(
        run.field(portal, "level hp mult per level"),
        Value::Float(0.111)
    );

    // Each row's fields pass by pass: a plain param's own structure (in a
    // loop, the set's element of that pass), a special row's second
    // structure, the one `portal save types` records for its param.
    let types = run.global("portal save types");
    let mut passes_checked = 0;
    for p in &table.params {
        let param = run.global(&p.param);
        let passes: Vec<Value> = match p.kind {
            SaveKind::Plain => match run.vm.set(param) {
                Some(_) => run.elements(param),
                None => vec![param],
            },
            kind => {
                let list = match kind {
                    SaveKind::Spillover => "spillover params",
                    SaveKind::MirrorAndClear => "mirror and clear if over limit params",
                    _ => "bit field extended area params",
                };
                let entries = run.field(types, list);
                let mut mine = Vec::new();
                for e in run.elements(entries) {
                    let of = run.field(e, "for param");
                    if run.resolve(of) == param {
                        mine.push(e);
                    }
                }
                assert_eq!(mine.len(), 1, "{} {kind:?}", p.param);
                vec![run.field(mine[0], "save param")]
            }
        };
        assert_eq!(passes.len(), p.fields.len(), "{} {:?}", p.param, p.kind);
        for (pass, f) in passes.into_iter().zip(&p.fields) {
            let got = [
                "val index",
                "bit index",
                "num bits",
                "bits per field",
                "limit to val",
            ]
            .map(|n| run.field(pass, n));
            let want =
                [f.word, f.bit, p.width, p.bits_per_field, p.limit].map(|n| Value::Int(n as i32));
            assert_eq!(got, want, "{} {:?}", p.param, p.kind);
            passes_checked += 1;
        }
    }
    println!(
        "the experience table {vm_xp:?} and {} save-param rows ({passes_checked} passes) as AH.2 read them",
        table.params.len()
    );
}

/// The full-health call sites' chain (the HUD's, `Macros_PlayerHUD.ai`)
/// around the disc's two macros: `Minion - Get HP Mult for Level` on
/// `level`, `base` times what it returns, then `Round Number Up` on the
/// product through a reference, as the disc's call passes it. The two
/// definitions are stubs that linking resolves by name to `global.bld`'s.
fn full_health_driver() -> Vec<u8> {
    let mut b = IgzBuilder::new();
    let mut w = Writer::new(&mut b);
    let mult = w.op(Class::DefineMacro, "Minion - Get HP Mult for Level");
    let round = w.op(Class::DefineMacro, "Round Number Up");
    let level = w.variable("level", "ValueInfo", Some("IntMeasurement"));
    let base = w.variable("base", "ValueInfo", Some("FloatMeasurement"));
    let factor = w.variable("factor", "ValueInfo", Some("FloatMeasurement"));
    let health = w.variable("health", "ValueInfo", Some("IntMeasurement"));
    let parameter =
        |w: &mut Writer<'_>, def: Location, name: &str, container: &str, rhs: Location| {
            let o = w.op(Class::MacroParameter, "parameter");
            w.text(o, 0x1C, name);
            w.class_word(o, 0x20, container);
            w.word(o, 0x28, u32::MAX);
            w.refer(o, 0x30, rhs);
            w.refer(o, 0x38, def);
            o
        };
    // `use macro <def>` with its INTERFACE: its block left open.
    let call = |w: &mut Writer<'_>, def: Location, params: &[Location]| {
        let np = w.group(&[El::Local(def)], None);
        let (at, o) = w.open(Class::UseMacro, "use macro");
        w.refer(o, 0x1C, np);
        let (iface, _) = w.open(Class::MacroInterface, "INTERFACE");
        for &p in params {
            w.push(p);
        }
        let enter = w.flow(Class::FlowMacro);
        w.push(enter);
        w.close(iface);
        at
    };
    w.script(
        "C:/guac/AH16/FullHealth.ai",
        |_| {},
        |_| {},
        |w| {
            let b = w.behaviour("compute");
            let lv = w.rhs("IntMeasurement", &[El::Local(level)]);
            let input = parameter(w, mult, "input level", "ValueInfo", lv);
            let one = w.float(1.0);
            let ret = parameter(w, mult, "return hp mult", "ValueInfo", one);
            let at = call(w, mult, &[input, ret]);
            let got = w.rhs("FloatMeasurement", &[El::Local(ret)]);
            w.set(&[El::Local(factor)], got);
            w.end_block(at);
            let hp = w.rhs("FloatMeasurement", &[El::Local(base)]);
            let f = w.rhs("FloatMeasurement", &[El::Local(factor)]);
            let product = w.expr(hp, Some((2, f)));
            w.set(&[El::Local(health)], product);
            let r = w.reference(&[El::Local(health)], None);
            let val = parameter(w, round, "round val", "ScriptReference", r);
            let at = call(w, round, &[val]);
            w.end_block(at);
            w.end_block(b);
        },
    );
    b.build()
}

/// The `max hp` a character's scripts pass `Portal - Define Minion`: the
/// operand to evaluate for it (what the script set the file variable the
/// argument names to, when it set it before the call) and the script.
fn base_operand(p: &Program, unit: u16) -> Option<(String, OperandId)> {
    let define = p.find_sym("portal - define minion")?;
    let max_hp = p.find_sym("max hp")?;
    let path_of = |o: OperandId| -> Option<&Vec<Elem>> {
        match &p.operands[o as usize] {
            Operand::Path { elems, .. } => Some(elems),
            Operand::Expr {
                a: Some(a),
                op: None,
                b: None,
            } => match &p.operands[*a as usize] {
                Operand::Path { elems, .. } => Some(elems),
                _ => None,
            },
            _ => None,
        }
    };
    for s in p.scripts.iter().filter(|s| s.unit == unit) {
        for (pos, &op) in s.ops.iter().enumerate() {
            let Ins::UseMacro { np: Some(np), .. } = &p.ops[op as usize] else {
                continue;
            };
            let Some(Elem::Op(d)) = path_of(*np).and_then(|e| e.first()) else {
                continue;
            };
            let def = p
                .def_of(p.op_info[*d as usize].obj)
                .and_then(|d| p.resolve(d));
            if def.is_none_or(|d| p.defs[d as usize].name != define) {
                continue;
            }
            let arg = s.ops[pos + 1..]
                .iter()
                .find_map(|&q| match &p.ops[q as usize] {
                    Ins::Parameter(param) if param.name == max_hp => param.rhs,
                    _ => None,
                })?;
            let set = path_of(arg).and_then(|named| {
                s.ops[..pos]
                    .iter()
                    .rev()
                    .find_map(|&q| match &p.ops[q as usize] {
                        Ins::SetValue {
                            lhs: Some(l),
                            rhs: Some(r),
                        } if path_of(*l) == Some(named) => Some(*r),
                        _ => None,
                    })
            });
            return Some((s.name.clone(), set.unwrap_or(arg)));
        }
    }
    None
}

/// Combat's rule in single precision, as the combat spec writes it.
fn rule(base: f32, level: i32) -> i32 {
    let multiplier = (level - 1) as f32 * 0.111 + 1.0;
    (base * multiplier + 0.99) as i32
}

#[test]
#[ignore = "reads the Giants dump (GUAC_GIANTS_DUMP)"]
fn full_health_at_levels_1_to_15_for_every_characters_base_is_combats_rule() {
    println!("load at start: {}", load());
    let wall = Instant::now();
    let usr = usrdir();
    let part = part();
    let driver = full_health_driver();
    let mut archives: Vec<String> = std::fs::read_dir(usr.join("character"))
        .unwrap()
        .filter_map(Result::ok)
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|n| n.ends_with(".bld") && n != "init_setup.bld")
        .collect();
    archives.sort();
    let roles = [
        Role::Bootstrap,
        Role::Global,
        Role::Other,
        Role::Other,
        Role::Other,
    ];
    let mut bases: BTreeMap<String, Vec<String>> = BTreeMap::new();
    let mut by_order = Vec::new();
    let mut unreadable = Vec::new();
    let mut characters = 0;
    for archive in &archives {
        let character = match guac_extract::scripts::entry(&usr, &format!("character/{archive}")) {
            Ok(c) => c,
            Err(e) => {
                unreadable.push(format!("{archive}: {e}"));
                continue;
            }
        };
        let bytes = [&part[0][..], &part[1], &part[2], &character, &driver];
        let (program, files) = link(&bytes, &roles, &[1, 2]);
        let Some((script, base_at)) = base_operand(&program, 3) else {
            continue;
        };
        characters += 1;
        let mut config = scripts::config(Mode::Strict);
        config.whitelist =
            Whitelist::with_first_ticks(&["Driver"], &EVERY_TICK, &GLOBAL_FIRST_TICKS);
        let driver_script = program.script_named("AH16/FullHealth.ai").unwrap();
        let mut run = Run::new(program, &files, &[1, 2], config);
        let d = run.vm.add(1 << 40, "Driver", driver_script);
        // The chain's first tick tunes the hit points a base names.
        run.step(1).unwrap_or_else(|h| panic!("{archive}: {h}"));
        let base = run
            .with_host(|vm, host| vm.eval(base_at, host))
            .unwrap_or_else(|h| panic!("{archive}: {h}"));
        let base = base
            .as_f32()
            .unwrap_or_else(|| panic!("{archive}: base {base:?}"));
        let mut healths = Vec::new();
        for level in 1..=15 {
            run.vm.set_local(d, "base", Value::Float(base));
            run.vm.set_local(d, "level", Value::Int(level));
            run.with_host(|vm, host| vm.tick(d, host))
                .unwrap_or_else(|h| panic!("{archive}: {h}"));
            let Some(Value::Int(h)) = run.vm.local(d, "health") else {
                panic!("{archive} level {level}: {:?}", run.vm.local(d, "health"))
            };
            assert_eq!(h, rule(base, level), "{archive} at level {level}");
            // The two ways a compiler could have computed it otherwise:
            // fused, and the base distributed over the sum.
            let m = (level - 1) as f32 * 0.111 + 1.0;
            let fused = base.mul_add(m, 0.99) as i32;
            let distributed = (base + base * ((level - 1) as f32 * 0.111) + 0.99) as i32;
            if fused != h || distributed != h {
                by_order.push(format!(
                    "{archive} level {level}: {h}, fused {fused}, distributed {distributed}"
                ));
            }
            healths.push(h);
        }
        let from = script.rsplit('/').next().unwrap_or(&script).to_owned();
        bases
            .entry(format!("{base} {healths:?}"))
            .or_default()
            .push(format!("{archive} ({from})"));
        if archive == "009_eruptor.bld" {
            assert_eq!(base, 290.0);
            for (level, want) in [(1, 290), (2, 323), (6, 451), (10, 580), (15, 741)] {
                assert_eq!(healths[level - 1], want, "Eruptor at level {level}");
            }
        }
    }
    for (healths, who) in &bases {
        println!("base {healths}: {}", who.join(", "));
    }
    println!(
        "{characters} characters of {} archives define a minion; unreadable {unreadable:?}",
        archives.len()
    );
    println!("off by order of computation or a fused multiply-add: {by_order:?}");
    assert!(characters > 0);
    println!("wall {:?}; load at end: {}", wall.elapsed(), load());
}

#[test]
#[ignore = "reads the Giants dump (GUAC_GIANTS_DUMP)"]
fn a_headless_app_steps_the_init_chain_600_fixed_steps_in_play_mode() {
    println!("load at start: {}", load());
    let (program, files) = link_part(&part());
    let mut app = App::new();
    app.add_plugins((MinimalPlugins, scripts::ScriptPlugin));
    scripts::install(app.world_mut(), program, &files, &ORDER);
    let mut first = Duration::ZERO;
    let mut worst = Duration::ZERO;
    let started = Instant::now();
    for i in 0..600 {
        let t = Instant::now();
        app.world_mut().run_schedule(FixedUpdate);
        let took = t.elapsed();
        if i == 0 {
            first = took;
        } else {
            worst = worst.max(took);
        }
    }
    let all = started.elapsed();
    let clones = app
        .world_mut()
        .query::<&scripts::ScriptClone>()
        .iter(app.world())
        .count();
    let placements = app
        .world_mut()
        .query::<&scripts::ScriptPlacement>()
        .iter(app.world())
        .count();
    let runtime = app.world().resource::<ScriptRuntime>();
    assert_eq!(runtime.steps, 600);
    let stopped: Vec<String> = runtime.stopped.iter().map(ToString::to_string).collect();
    assert!(stopped.is_empty(), "stopped: {stopped:?}");
    println!(
        "600 fixed steps in {all:?}: the first {first:?}, the worst after it {worst:?}; {} ops; {placements} placement entities, {clones} clone entities; nothing stopped",
        runtime.vm.executed()
    );
    println!("load at end: {}", load());
}

#[test]
#[ignore = "reads the Giants dump (GUAC_GIANTS_DUMP)"]
fn the_host_reads_each_placements_id_and_buffer_off_its_file() {
    let (_, files) = link_part(&part());
    let listing = scripts::config(Mode::Strict).whitelist;
    let running: Vec<&str> = files
        .placed
        .iter()
        .filter(|p| p.activity_state == 4)
        .filter_map(|p| p.script.as_deref().map(|s| (p.name.as_str(), s)))
        .filter(|(name, script)| listing.lists(name, script))
        .map(|(name, _)| name)
        .collect();
    assert_eq!(
        running.len(),
        GLOBAL_FIRST_TICKS.len() + EVERY_TICK.len(),
        "{running:?}"
    );
    for p in &files.placed {
        assert_eq!(files.ids.get(&p.obj), Some(&p.id), "{}", p.name);
        assert!(p.buffer >= 0.0, "{}", p.name);
    }
    let with_waypoints = files
        .placed
        .iter()
        .filter(|p| !p.waypoints.is_empty())
        .count();
    println!(
        "{} actor placements, {} running on the whitelist, {with_waypoints} with waypoints, {} ids",
        files.placed.len(),
        running.len(),
        files.ids.len()
    );
}
