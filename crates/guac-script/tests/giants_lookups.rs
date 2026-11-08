//! AS.128's two lookups against the scans they stand for, over the whole
//! disc: every `level/*.bld` on the Giants dump linked with
//! `bootstrap.bld`, `global.bld` and `init_setup.bld` as `giants_levels`
//! links them, then
//!
//! - every file variable's name looked up through the link's name index
//!   (`Program::global_named`) gives the global the scan over every global
//!   gave: the lowest object of that name, so of two globals sharing a name
//!   the index answers the one the scan answered first;
//! - every member a path names is the built-in (`Builtin::of`, ids hashed
//!   once) the scan over the built-ins' names finds, and no two built-ins
//!   share an id.
//!
//! Ignored by default: it reads `GUAC_GIANTS_DUMP` (env, else `.env.dev`).
//! `GUAC_LEVELS` (a comma list of archive names) runs those alone.
//! `cargo test -p guac-script --profile disc --test giants_lookups -- --ignored --nocapture`

mod common;

use std::collections::{BTreeMap, HashMap};

use common::{env_or_dotenv, usrdir};
use guac_formats::iga::Archive;
use guac_formats::igz::Igz;
use guac_formats::script::Graph;
use guac_script::{Builtin, Elem, GlobalId, MemberId, Operand, Program, Role, Unit};

/// Every built-in, the match keeping the list whole as one is added.
fn builtins() -> Vec<Builtin> {
    let all = [
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
    for b in all {
        match b {
            Builtin::My
            | Builtin::Null
            | Builtin::Undefined
            | Builtin::Its
            | Builtin::ValueDereferencer
            | Builtin::Count
            | Builtin::First
            | Builtin::Last
            | Builtin::Random
            | Builtin::CastToInteger
            | Builtin::ScriptTimer
            | Builtin::FrameRateScale
            | Builtin::Behaviour
            | Builtin::Next => {}
        }
    }
    all.to_vec()
}

fn level_bld(path: &std::path::Path) -> Vec<u8> {
    let bytes = std::fs::read(path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    let arc = Archive::parse(&bytes).unwrap();
    let entry = arc.find("level.bld").expect("level.bld");
    arc.read(entry).unwrap().into_owned()
}

/// What the scan over every global answered for `name`: the lowest object
/// of that name.
fn scanned(p: &Program, name: &str) -> Option<GlobalId> {
    p.globals
        .iter()
        .enumerate()
        .filter(|(_, (o, _))| p.name_of(*o) == Some(name))
        .map(|(i, (o, _))| (*o, i as GlobalId))
        .min()
        .map(|(_, g)| g)
}

/// The names to look up, each with how many globals carry it: a name
/// carried twice is where the index has to answer the scan's first.
fn global_names(p: &Program) -> BTreeMap<String, usize> {
    let mut out = BTreeMap::new();
    for (o, _) in &p.globals {
        if let Some(n) = p.name_of(*o) {
            *out.entry(n.to_owned()).or_default() += 1;
        }
    }
    out
}

/// The members the paths name, each checked against the built-ins, with
/// how often, to count the built-ins' references the check covered.
fn members(p: &Program) -> HashMap<MemberId, usize> {
    let mut out = HashMap::new();
    for o in &p.operands {
        if let Operand::Path { elems, .. } = o {
            for e in elems {
                if let Elem::Member(m) = e {
                    *out.entry(*m).or_default() += 1;
                }
            }
        }
    }
    out
}

#[test]
#[ignore = "reads the Giants dump at GUAC_GIANTS_DUMP"]
fn every_lookup_by_name_and_by_builtin_id_gives_the_scans_answer() {
    let all = builtins();
    for (i, a) in all.iter().enumerate() {
        for b in &all[i + 1..] {
            assert_ne!(a.id(), b.id(), "{} and {} share an id", a.name(), b.name());
        }
    }
    let by_scan = |m: MemberId| all.iter().copied().find(|b| b.id() == m);

    let usr = usrdir();
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
    let mut files: Vec<String> = std::fs::read_dir(usr.join("level"))
        .unwrap()
        .filter_map(|e| {
            let n = e.unwrap().file_name().into_string().ok()?;
            n.strip_suffix(".bld").map(str::to_owned)
        })
        .collect();
    files.sort();
    if let Some(only) = env_or_dotenv("GUAC_LEVELS").filter(|s| !s.is_empty()) {
        let only: Vec<&str> = only.split(',').collect();
        files.retain(|f| only.contains(&f.as_str()));
    }
    let (mut linked, mut names, mut twins, mut member_ids, mut builtin_refs) = (0, 0, 0, 0, 0);
    for file in &files {
        let bytes = level_bld(&usr.join(format!("level/{file}.bld")));
        let Ok(li) = Igz::parse(&bytes) else {
            println!("SKIP\t{file}\tparse");
            continue;
        };
        let Ok(lg) = Graph::read(&li) else {
            println!("SKIP\t{file}\tgraph");
            continue;
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
        let p = match Program::link(&units) {
            Ok(p) => p,
            Err(e) => {
                println!("SKIP\t{file}\tlink: {e}");
                continue;
            }
        };
        linked += 1;
        for (name, n) in global_names(&p) {
            names += 1;
            let want = scanned(&p, &name);
            assert_eq!(p.global_named(&name), want, "{file}: {name}");
            if n > 1 {
                twins += 1;
                // Where each stands and which the lookup answered: the
                // lowest object, not always the first global (a later
                // global can be the lower object).
                let at: Vec<_> = p
                    .globals
                    .iter()
                    .enumerate()
                    .filter(|(_, (o, _))| p.name_of(*o) == Some(name.as_str()))
                    .map(|(i, (o, _))| format!("{}:{} (#{i})", o.unit, o.index))
                    .collect();
                let (o, _) = p.globals[want.unwrap() as usize];
                println!(
                    "TWIN\t{file}\t{name}\t{}\tanswered {}:{}",
                    at.join(" "),
                    o.unit,
                    o.index
                );
            }
        }
        // A name no global has: neither answers.
        assert_eq!(p.global_named("no global has this name"), None, "{file}");
        for (m, refs) in members(&p) {
            member_ids += 1;
            let b = Builtin::of(m);
            assert_eq!(b, by_scan(m), "{file}: {m:?}");
            if b.is_some() {
                builtin_refs += refs;
            }
        }
    }
    println!(
        "{linked} of {} level files linked; {names} names looked up, {twins} held by more than one global; {member_ids} member ids, {builtin_refs} references to built-ins",
        files.len()
    );
    assert_eq!(linked, files.len(), "every level file links");
    assert!(
        twins > 0,
        "no name held twice: the first-of-its-name rule untested"
    );
}
