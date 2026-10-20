//! What each figure carries, extracted (openspec AE.23), against what the
//! oracle saw (AE.22's sweep): every figure with a character model, its
//! load file's model read through the model stage's reader with
//! `global.bld`, and the models it carries (model file and the bone each
//! hangs on) equal, as a multiset, to the sweep's row for that figure, the
//! body the model the game drew for it.
//!
//! Ignored by default: it reads `GUAC_GIANTS_DUMP` and `GUAC_CAPTURE_DIR`
//! (the sweep's run, the newest `BLUS30968/ae22-full-*` holding a
//! `table.tsv`) from the environment or the workspace's `.env.dev`:
//!
//! `cargo test -p guac-extract --release --test giants_attachments -- --ignored --nocapture`

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::Instant;

use guac_extract::known::KNOWN_DUMPS;
use guac_extract::{models, roster};
use guac_ir::{DumpIdentity, Model, Sha256};

fn env_or_dotenv(key: &str) -> Option<String> {
    if let Ok(value) = std::env::var(key) {
        return Some(value);
    }
    let dotenv = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../.env.dev");
    let text = std::fs::read_to_string(dotenv).ok()?;
    text.lines().find_map(|line| {
        let (k, v) = line.trim().split_once('=')?;
        (k.trim() == key).then(|| v.trim().trim_matches('"').to_owned())
    })
}

/// The newest sweep run's table under `GUAC_CAPTURE_DIR`.
fn sweep_table() -> PathBuf {
    let captures = PathBuf::from(env_or_dotenv("GUAC_CAPTURE_DIR").expect("set GUAC_CAPTURE_DIR"));
    let dir = captures.join("BLUS30968");
    let mut runs: Vec<PathBuf> = std::fs::read_dir(&dir)
        .unwrap_or_else(|e| panic!("{}: {e}", dir.display()))
        .map(|e| e.unwrap().path())
        .filter(|p| {
            p.file_name()
                .is_some_and(|n| n.to_string_lossy().starts_with("ae22-full-"))
                && p.join("table.tsv").is_file()
        })
        .collect();
    runs.sort();
    runs.pop()
        .unwrap_or_else(|| {
            panic!(
                "no ae22-full-* sweep with a table.tsv under {}",
                dir.display()
            )
        })
        .join("table.tsv")
}

/// One figure's row: its load file, the body the game drew and what it
/// carried, `model@bone` each, sorted.
struct Row {
    load: String,
    model: String,
    carried: Vec<String>,
}

fn read_table(path: &Path) -> BTreeMap<String, Row> {
    let text = std::fs::read_to_string(path).unwrap();
    let mut lines = text.lines();
    let header: Vec<&str> = lines.next().unwrap().split('\t').collect();
    let col = |name: &str| {
        header
            .iter()
            .position(|h| *h == name)
            .unwrap_or_else(|| panic!("no {name} column"))
    };
    let (id, load, model, carried) = (
        col("id"),
        col("load file"),
        col("model"),
        col("attached models (model@bone)"),
    );
    lines
        .map(|l| {
            let f: Vec<&str> = l.split('\t').collect();
            let mut c: Vec<String> = f[carried]
                .split(',')
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(str::to_owned)
                .collect();
            c.sort();
            (
                f[id].to_owned(),
                Row {
                    load: f[load].to_owned(),
                    model: f[model].to_owned(),
                    carried: c,
                },
            )
        })
        .collect()
}

/// What `model` carries, `file@bone` each as the sweep writes it, sorted.
fn carried(model: &Model) -> Vec<String> {
    let mut out: Vec<String> = model
        .attachments
        .iter()
        .map(|a| {
            let file = a.model.path.rsplit(['/', '\\']).next().unwrap_or_default();
            format!("{file}@{}", a.bone.as_deref().unwrap_or("(itself)"))
        })
        .collect();
    out.sort();
    out
}

#[test]
#[ignore = "reads the Giants dump at GUAC_GIANTS_DUMP and the sweep under GUAC_CAPTURE_DIR"]
fn every_figure_carries_what_the_oracle_saw() {
    let dump = PathBuf::from(env_or_dotenv("GUAC_GIANTS_DUMP").expect("set GUAC_GIANTS_DUMP"));
    let table_path = sweep_table();
    let table = read_table(&table_path);
    let known = &KNOWN_DUMPS[0];
    let identity = DumpIdentity {
        title_id: known.title_id.into(),
        app_ver: known.app_ver.into(),
        manifest_hash: Sha256([0; 32]),
    };
    let roster = roster::read(&dump, identity.clone()).unwrap();
    let usrdir = dump.join("PS3_GAME/USRDIR");
    let level_of = |archive: &str| {
        let bytes = std::fs::read(usrdir.join(archive)).unwrap();
        models::level(&bytes, archive).unwrap()
    };
    let (global, bootstrap) = (level_of(roster::GLOBAL), level_of(roster::BOOTSTRAP));
    let globals = models::Globals::new(&global, &bootstrap).unwrap();

    let started = Instant::now();
    let mut extracted: BTreeMap<String, Model> = BTreeMap::new();
    for load in models::character_loads(&roster) {
        let path = models::archive_path(&usrdir, load);
        let level = models::archive(&path, load)
            .and_then(|bytes| models::level(&bytes, load))
            .unwrap();
        let m = models::read(&level, load, identity.clone(), Some(&globals)).unwrap();
        extracted.insert(load.to_owned(), m.model);
    }
    let took = started.elapsed();

    let mut figures = 0;
    let mut equal = 0;
    let mut per: BTreeMap<usize, usize> = BTreeMap::new();
    let mut bodies = 0;
    let mut diffs = Vec::new();
    for f in &roster.figures {
        let Some(m) = extracted.get(&f.load_file) else {
            continue;
        };
        figures += 1;
        let id = f.id();
        let row = table
            .get(&id)
            .unwrap_or_else(|| panic!("{id} ({}) isn't in {}", f.load_file, table_path.display()));
        assert_eq!(row.load, f.load_file, "{id}");
        assert_eq!(m.attachments_unsettled, None, "{id}");
        if m.name == row.model {
            bodies += 1;
        } else {
            diffs.push(format!(
                "{id}: body {} where the game drew {}",
                m.name, row.model
            ));
        }
        let got = carried(m);
        *per.entry(got.len()).or_default() += 1;
        if got == row.carried {
            equal += 1;
        } else {
            diffs.push(format!(
                "{id} ({}): {got:?} where the game carried {:?}",
                f.load_file, row.carried
            ));
        }
    }
    let models: usize = extracted.values().map(|m| m.attachments.len()).sum();
    let helpers: usize = extracted
        .values()
        .flat_map(|m| &m.attachments)
        .filter(|a| a.model.segments.is_empty() || a.model.path.contains("inviso"))
        .count();
    let drawn: usize = extracted
        .values()
        .flat_map(|m| &m.attachments)
        .map(|a| a.model.segments.len())
        .sum();
    let skinned = extracted
        .values()
        .flat_map(|m| &m.attachments)
        .filter(|a| a.model.skeleton.is_some())
        .count();
    let nested = extracted
        .values()
        .flat_map(|m| &m.attachments)
        .filter(|a| a.parent.is_some())
        .count();
    println!(
        "{} load files read in {:.2} s: {models} attachments ({helpers} invisible helpers, {skinned} skinned, {nested} on another), {drawn} segments; {figures} figures, per figure {per:?}",
        extracted.len(),
        took.as_secs_f64()
    );
    println!(
        "against {}: {equal} of {figures} carry what the game carried, {bodies} of {figures} the body it drew",
        table_path.display()
    );
    for d in &diffs {
        println!("  {d}");
    }
    assert_eq!(extracted.len(), 67);
    assert_eq!(figures, 99);
    assert!(diffs.is_empty(), "{} figures differ", diffs.len());
}
