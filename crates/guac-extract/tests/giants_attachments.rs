//! What each figure carries, extracted (openspec AE.23), against what the
//! oracle saw (AE.22's sweep): every figure with a character model, its
//! load file's model read through the model stage's reader with
//! `global.bld`, and the models it carries (model file and the bone each
//! hangs on) equal, as a multiset, to the sweep's row for that figure, the
//! body the model the game drew for it; then, off each figure's
//! `figure.json`, each carried model on the same parent (the body or the
//! part it hangs from) at the offset the game's attach point held.
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

/// One carried model as the tree has it: `parent>file@bone` (`body` for
/// the hero, else the parent's model file) and the attach point's offset,
/// position then tilt, heading and roll as the sweep read them, or as the
/// extract's transform.
#[derive(Debug, Clone)]
struct Hang {
    label: String,
    translation: [f32; 3],
    rotation: [f32; 4],
    /// The placement's own `_attachXform` at the sweep's pause, which no
    /// static read takes: the game moved it at run time where it isn't zero.
    runtime_xform: Option<[f64; 6]>,
}

fn file_of(path: &str) -> &str {
    path.rsplit(['/', '\\']).next().unwrap_or_default()
}

/// Each sweep figure's dir, by id, off the run's `summary.json`.
fn sweep_dirs(run: &Path) -> BTreeMap<String, PathBuf> {
    let text = std::fs::read_to_string(run.join("summary.json")).unwrap();
    let summary: serde_json::Value = serde_json::from_str(&text).unwrap();
    summary["rows"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|r| Some((r["id"].as_str()?.to_owned(), run.join(r["dir"].as_str()?))))
        .collect()
}

/// What the sweep's `figure.json` has hanging on the hero that draws a
/// model, nested ones under the attachment they hang on, sorted by label.
fn sweep_hangs(figure: &Path) -> Vec<Hang> {
    let text = std::fs::read_to_string(figure).unwrap();
    let r: serde_json::Value = serde_json::from_str(&text).unwrap();
    fn walk(list: &serde_json::Value, parent: &str, out: &mut Vec<Hang>) {
        for a in list.as_array().into_iter().flatten() {
            let file = a["model"]["name"].as_str().map(file_of);
            if let Some(file) = file {
                let bone = &a["node"]["Bone"];
                let floats = |v: &serde_json::Value| -> Vec<f64> {
                    v.as_array()
                        .into_iter()
                        .flatten()
                        .filter_map(serde_json::Value::as_f64)
                        .collect()
                };
                let t = floats(&bone["transform"]);
                let t = if t.len() == 6 { t } else { vec![0.0; 6] };
                let euler = guac_formats::igz::layouts::EulerTransform {
                    position: [t[0] as f32, t[1] as f32, t[2] as f32],
                    rotation: [t[3] as f32, t[4] as f32, t[5] as f32],
                };
                let offset = models::euler_transform(&euler);
                let x = floats(&a["attach_xform"]);
                out.push(Hang {
                    label: format!(
                        "{parent}>{file}@{}",
                        bone["name"].as_str().unwrap_or("(itself)")
                    ),
                    translation: offset.translation,
                    rotation: offset.rotation,
                    runtime_xform: (x.len() == 6 && x.iter().any(|v| v.abs() > 1e-6))
                        .then(|| [x[0], x[1], x[2], x[3], x[4], x[5]]),
                });
            }
            let under = file.unwrap_or_else(|| a["name"].as_str().unwrap_or("?"));
            walk(&a["attached"], under, out);
        }
    }
    let mut out = Vec::new();
    walk(&r["attachments"]["attached"], "body", &mut out);
    out.sort_by(by_label);
    out
}

/// By label, then offset, so two parts with one label pair the same way.
fn by_label(a: &Hang, b: &Hang) -> std::cmp::Ordering {
    a.label.cmp(&b.label).then_with(|| {
        a.translation
            .iter()
            .zip(&b.translation)
            .map(|(x, y)| x.total_cmp(y))
            .find(|o| o.is_ne())
            .unwrap_or(std::cmp::Ordering::Equal)
    })
}

/// The same off the extracted model.
fn extracted_hangs(model: &Model) -> Vec<Hang> {
    let mut out: Vec<Hang> = model
        .attachments
        .iter()
        .map(|a| {
            let parent = a.parent.map_or("body", |k| {
                file_of(&model.attachments[usize::from(k)].model.path)
            });
            Hang {
                label: format!(
                    "{parent}>{}@{}",
                    file_of(&a.model.path),
                    a.bone.as_deref().unwrap_or("(itself)")
                ),
                translation: a.offset.translation,
                rotation: a.offset.rotation,
                runtime_xform: None,
            }
        })
        .collect();
    out.sort_by(by_label);
    out
}

/// Where two hang lists differ: a label, or an offset past 1e-4 m or
/// 1e-4 in the quaternion (sign free).
fn hang_diffs(got: &[Hang], want: &[Hang]) -> Vec<String> {
    let labels = |h: &[Hang]| h.iter().map(|h| h.label.clone()).collect::<Vec<_>>();
    if labels(got) != labels(want) {
        return vec![format!(
            "{:?} where the game hung {:?}",
            labels(got),
            labels(want)
        )];
    }
    got.iter()
        .zip(want)
        .filter_map(|(g, w)| {
            let moved = g
                .translation
                .iter()
                .zip(&w.translation)
                .any(|(a, b)| (a - b).abs() > 1e-4);
            let dot: f32 = g.rotation.iter().zip(&w.rotation).map(|(a, b)| a * b).sum();
            (moved || (1.0 - dot.abs()) > 1e-4).then(|| {
                format!(
                    "{} at {:?} {:?} where the game's point is {:?} {:?}",
                    g.label, g.translation, g.rotation, w.translation, w.rotation
                )
            })
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

    let run = table_path.parent().unwrap();
    let dirs = sweep_dirs(run);
    let mut figures = 0;
    let mut equal = 0;
    let mut placed = 0;
    let mut offsets = 0;
    let mut moved: BTreeMap<String, [f64; 6]> = BTreeMap::new();
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
        // The tree and the points: each on the same parent, at the offset
        // the game's attach point holds.
        let dir = dirs
            .get(&id)
            .unwrap_or_else(|| panic!("{id} isn't in {}/summary.json", run.display()));
        let want = sweep_hangs(&dir.join("figure.json"));
        let got = extracted_hangs(m);
        let d = hang_diffs(&got, &want);
        if d.is_empty() {
            placed += 1;
            offsets += got.len();
        }
        for w in &want {
            if let Some(x) = w.runtime_xform {
                moved.insert(format!("{} {}", f.load_file, w.label), x);
            }
        }
        diffs.extend(
            d.into_iter()
                .map(|d| format!("{id} ({}): {d}", f.load_file)),
        );
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
        "against {}: {equal} of {figures} carry what the game carried, {bodies} of {figures} the body it drew, {placed} of {figures} each part on the parent and at the offset the game's attach point holds ({offsets} parts)",
        table_path.display()
    );
    println!(
        "{} carried models the game moved off their attach point at run time (a non-zero _attachXform at the sweep's pause), which the static read leaves on it:",
        moved.len()
    );
    for (what, x) in &moved {
        println!("  {what}: {x:?}");
    }
    for d in &diffs {
        println!("  {d}");
    }
    assert_eq!(extracted.len(), 67);
    assert_eq!(figures, 99);
    assert!(diffs.is_empty(), "{} differences", diffs.len());
}
