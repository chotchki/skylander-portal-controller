//! AH.13's op graphs off the Giants dump: every script in the files AH
//! reads (`level_213_tutotial.bld`, `global.bld`, `init_setup.bld`,
//! `009_eruptor.bld`, and `bootstrap.bld`, whose empty update base AH.12
//! added) read as a graph, each class counted; then every IGZ on the disc,
//! reproducing aj-script's census (14,207 scripts, 44 op classes, 4,617,339
//! positions, the 1,485,888 class words without the index flag).
//! Ignored by default; it reads `GUAC_GIANTS_DUMP` like `giants_igz`:
//!
//! `cargo test -p guac-formats --release --test giants_script_ops -- --ignored --nocapture`

mod common;

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::time::Instant;

use common::usrdir;
use guac_formats::iga::Archive;
use guac_formats::igz::layouts::{Layout, Stack, is_a};
use guac_formats::igz::{Igz, Ref};
use guac_formats::script::{Class, Graph};

/// aj-script's census per class: positions, and op objects at them (once a
/// file).
#[rustfmt::skip]
const CENSUS: &[(&str, u64, u64)] = &[
    ("OpMacroParameter", 1331594, 647218),
    ("OpSetValue", 629118, 492644),
    ("OpFlowBuiltInBehavior", 493438, 595),
    ("OpCheckValue", 307580, 274455),
    ("OpMacroInterface", 243638, 36486),
    ("OpFlowMacro", 243638, 593),
    ("OpSetReference", 225182, 194650),
    ("OpUseMacro", 212525, 60672),
    ("OpAbstractFlow", 193314, 32840),
    ("OpChangeMembership", 61478, 48587),
    ("OpSpawn", 55905, 55905),
    ("OpCreateVariable", 50544, 50544),
    ("OpCheckReference", 45002, 40312),
    ("OpSlideValue", 40739, 40739),
    ("OpRemove", 39442, 21056),
    ("OpFlowLoop", 36880, 223),
    ("OpIncValue", 36752, 13028),
    ("OpDisplace", 33701, 30761),
    ("OpStartSequence", 33202, 33202),
    ("OpFindVariable", 33173, 33173),
    ("OpFlowBehavior", 31106, 224),
    ("OpSetBehavior", 29919, 20804),
    ("OpUserBehavior", 29806, 29806),
    ("OpForEach", 18619, 18619),
    ("OpLoopValue", 18261, 18261),
    ("OpDefineMacro", 17577, 17577),
    ("OpStartUp", 14207, 6243),
    ("OpShutDown", 14207, 3566),
    ("OpPreScript", 14207, 10078),
    ("OpFindSubSet", 13683, 13683),
    ("OpDefineMacroSpecialization", 13536, 13536),
    ("OpControl", 12818, 12818),
    ("OpStopSequence", 8728, 4797),
    ("OpCheckMembership", 8398, 7940),
    ("OpDefineStructure", 7104, 7104),
    ("OpReset", 4610, 2427),
    ("OpMacroSection", 4539, 4539),
    ("OpCheckFOV", 2687, 2687),
    ("OpDecValue", 2433, 1881),
    ("OpStructureSection", 2273, 2273),
    ("OpTurnTo", 779, 715),
    ("OpMoveTo", 663, 626),
    ("OpTeleportTo", 229, 209),
    ("OpMoveFrom", 105, 80),
];

#[derive(Default)]
struct Tally {
    igz: u64,
    with_scripts: u64,
    scripts: u64,
    prescript_first: u64,
    positions: u64,
    op_objects: u64,
    operands: u64,
    unflagged: u64,
    /// Unflagged class words over the objects aj-script's census walked.
    census_unflagged: u64,
    /// Unflagged class words the graph read past that walk, by holder type.
    beyond_census: BTreeMap<String, u64>,
    /// Objects of the 44 classes anywhere in the files, reached or not.
    class_objects: u64,
    /// `OpParameter`s: an op class no list holds.
    parameters: u64,
    classes: BTreeMap<Class, (u64, u64)>,
    failures: Vec<String>,
}

impl Tally {
    fn take(&mut self, name: &str, bytes: &[u8]) {
        let igz = match Igz::parse(bytes) {
            Ok(igz) => igz,
            Err(e) => {
                self.failures.push(format!("{name}: {e}"));
                return;
            }
        };
        self.igz += 1;
        for o in igz.objects() {
            let t = igz.type_name(o);
            self.class_objects += u64::from(Class::from_type(t).is_some());
            self.parameters += u64::from(t == "OpParameter");
        }
        if igz.type_index("tfbScriptInfo").is_none() {
            return;
        }
        let g = match Graph::read(&igz) {
            Ok(g) => g,
            Err(e) => {
                self.failures.push(format!("{name}: {e}"));
                return;
            }
        };
        self.with_scripts += 1;
        self.scripts += g.scripts.len() as u64;
        self.operands += g.operands.len() as u64;
        self.unflagged += g.unflagged_classes as u64;
        // The census's walk, and the graph's objects, each word counted once.
        let census = census_walk(&igz, &g);
        let read: BTreeSet<usize> = g
            .ops
            .keys()
            .chain(g.operands.keys())
            .chain(g.values.keys())
            .copied()
            .collect();
        let words = |o: usize| {
            igz.refs_of(&igz.objects()[o])
                .filter(|(_, r)| matches!(r, Ok(Ref::UnresolvedMeta(_))))
                .count() as u64
        };
        self.census_unflagged += census.iter().map(|&o| words(o)).sum::<u64>();
        assert_eq!(
            read.iter().map(|&o| words(o)).sum::<u64>(),
            g.unflagged_classes as u64,
            "{name}: the graph counts each word it read once"
        );
        for &o in read.difference(&census) {
            let n = words(o);
            if n > 0 {
                let t = igz.type_name(&igz.objects()[o]).to_owned();
                *self.beyond_census.entry(t).or_default() += n;
            }
        }
        let missed: u64 = census.difference(&read).map(|&o| words(o)).sum();
        assert_eq!(
            missed, 0,
            "{name}: the graph reads every word the census did"
        );
        let mut seen = BTreeSet::new();
        for s in &g.scripts {
            self.positions += s.ops.len() as u64;
            if g.op(s, 0).map(|o| o.class()) == Some(Class::PreScript) {
                self.prescript_first += 1;
            }
            for o in &s.ops {
                let class = g.ops[o].class();
                let e = self.classes.entry(class).or_default();
                e.0 += 1;
                if seen.insert(*o) {
                    e.1 += 1;
                    self.op_objects += 1;
                }
            }
        }
    }

    fn print(&self, label: &str) {
        println!(
            "{label}: {} IGZ, {} with scripts: {} scripts ({} PRESCRIPT first), {} positions, \
             {} op objects at them, {} operands, {} classes, {} unflagged class words resolved",
            self.igz,
            self.with_scripts,
            self.scripts,
            self.prescript_first,
            self.positions,
            self.op_objects,
            self.operands,
            self.classes.len(),
            self.unflagged,
        );
        for (c, (p, o)) in &self.classes {
            println!("    {:<30} {p:>9} positions {o:>9} objects", c.type_name());
        }
        println!(
            "unflagged class words over aj-script's walk: {}; read past it: {:?}",
            self.census_unflagged, self.beyond_census
        );
        for f in self.failures.iter().take(20) {
            println!("FAIL {f}");
        }
    }
}

/// The objects aj-script's census took words from: every op at a position,
/// then every object their refs reach, descending only through stacks
/// (their refs and elements) and expressions, each once.
fn census_walk(igz: &Igz<'_>, g: &Graph<'_>) -> BTreeSet<usize> {
    let objects = igz.objects();
    let ops: BTreeSet<usize> = g
        .scripts
        .iter()
        .flat_map(|s| s.ops.iter().copied())
        .collect();
    let mut seen = ops.clone();
    let mut out = ops.clone();
    let mut todo: Vec<usize> = Vec::new();
    for &op in &ops {
        for (_, r) in igz.refs_of(&objects[op]) {
            if let Ok(Ref::Object(t)) = r {
                todo.push(t);
            }
        }
    }
    while let Some(o) = todo.pop() {
        if !seen.insert(o) {
            continue;
        }
        let t = igz.type_name(&objects[o]);
        let stack = is_a(t, "ScriptObjectList");
        if !(stack || t == "ValueRHSVariant" || is_a(t, "tfbScriptObject")) {
            continue;
        }
        out.insert(o);
        if stack || t == "ValueRHSVariant" {
            for (_, r) in igz.refs_of(&objects[o]) {
                if let Ok(Ref::Object(x)) = r {
                    todo.push(x);
                }
            }
        }
        if stack && let Ok(s) = Stack::read(igz, o) {
            for e in s.path {
                if let guac_formats::igz::layouts::ObjectRef::Object(x) = e {
                    todo.push(x);
                }
            }
        }
    }
    out
}

fn read_archive(path: &Path, tally: &mut Tally) -> bool {
    let bytes = std::fs::read(path).unwrap();
    let Ok(arc) = Archive::parse(&bytes) else {
        return false;
    };
    for entry in arc.entries() {
        let name = format!("{}:{}", path.display(), entry.name());
        match arc.read(entry) {
            Ok(data) if data.starts_with(b"IGZ\x01") => tally.take(&name, &data),
            Ok(_) => {}
            Err(e) => tally.failures.push(format!("{name}: {e}")),
        }
    }
    true
}

#[test]
#[ignore = "reads the Giants dump at GUAC_GIANTS_DUMP"]
fn every_script_in_the_files_ah_reads_parses() {
    let started = Instant::now();
    let usrdir = usrdir();
    for (file, scripts, positions) in [
        ("level/level_213_tutotial.bld", 221, 53_497),
        ("permanent/global.bld", 58, 81_783),
        ("character/init_setup.bld", 11, 9_380),
        ("character/009_eruptor.bld", 8, 3_385),
        ("permanent/bootstrap.bld", 1, 27),
    ] {
        let mut tally = Tally::default();
        assert!(read_archive(&usrdir.join(file), &mut tally), "{file}");
        tally.print(file);
        assert!(tally.failures.is_empty(), "{file}");
        assert_eq!(
            (tally.scripts, tally.positions),
            (scripts, positions),
            "{file}"
        );
        assert_eq!(tally.prescript_first, scripts, "{file}");
    }
    println!("wall time {:.1} s", started.elapsed().as_secs_f64());
}

fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
    let mut entries: Vec<PathBuf> = std::fs::read_dir(dir)
        .unwrap()
        .map(|e| e.unwrap().path())
        .collect();
    entries.sort();
    for p in entries {
        if p.is_dir() {
            walk(&p, out);
        } else {
            out.push(p);
        }
    }
}

#[test]
#[ignore = "reads the Giants dump at GUAC_GIANTS_DUMP"]
fn every_script_on_the_disc_parses_and_the_census_holds() {
    let started = Instant::now();
    let mut files = Vec::new();
    walk(&usrdir(), &mut files);
    let mut tally = Tally::default();
    let archives = files.iter().filter(|p| read_archive(p, &mut tally)).count();
    println!("{} files, {archives} archives", files.len());
    tally.print("disc");
    println!(
        "objects of the 44 classes in the files: {}, OpParameter {}",
        tally.class_objects, tally.parameters
    );
    println!("wall time {:.1} s", started.elapsed().as_secs_f64());
    assert!(tally.failures.is_empty());
    assert_eq!(tally.igz, 5_801);
    assert_eq!(tally.with_scripts, 595);
    assert_eq!(tally.scripts, 14_207);
    assert_eq!(tally.prescript_first, 14_207);
    assert_eq!(tally.positions, 4_617_339);
    assert_eq!(tally.op_objects, 2_308_176);
    assert_eq!(tally.census_unflagged, 1_485_888);
    // aj-script's 2,596,478 op objects: the 44 classes' and `OpParameter`'s.
    assert_eq!(tally.class_objects + tally.parameters, 2_596_478);
    assert_eq!(tally.classes.len(), 44);
    for &(name, positions, objects) in CENSUS {
        let class = Class::from_type(name).unwrap();
        assert_eq!(tally.classes[&class], (positions, objects), "{name}");
    }
}
