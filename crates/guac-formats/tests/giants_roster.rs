//! AE.1's roster readers off the Giants dump: every language pack joined to
//! its level, every `global.bld` import on the disc resolved through the
//! export ids, the two effects AE's materials name, and every object of the
//! roster's layouts read with 0 failures (the registry check AC.4's types
//! get in `giants_layouts`).
//! Ignored by default; it reads `GUAC_GIANTS_DUMP` like `giants_igz`:
//!
//! `cargo test -p guac-formats --release --test giants_roster -- --ignored --nocapture --test-threads 1`

mod common;

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Instant;

use common::usrdir;
use guac_formats::exports::{Export, Exports, GLOBAL_BASE, NAMESPACE, Root};
use guac_formats::iga::Archive;
use guac_formats::igz::Igz;
use guac_formats::igz::layouts::{
    self, AnimatableTextureDataInfo, DirectionalLightInfo, LanguagePackInfo, Layout, ScriptSet,
    ScriptSetList, SpriteInfo, StringInfo, ValueInfo,
};
use guac_formats::language::{LanguagePack, Texts};
use guac_formats::{FormatError, fnv1a32};

/// Sorted, so the printed tallies and failures come out in the same order
/// on every run.
fn archives(dir: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        for entry in std::fs::read_dir(&d).unwrap() {
            let p = entry.unwrap().path();
            if p.is_dir() {
                stack.push(p);
            } else if matches!(p.extension().and_then(|e| e.to_str()), Some("bld" | "arc")) {
                out.push(p);
            }
        }
    }
    out.sort();
    out
}

/// Runs `f` on every archive, on up to 8 threads (other work shares the
/// machine), and returns the per-archive results in path order.
fn each_archive<T: Send>(paths: &[PathBuf], f: impl Fn(&Path, &[u8]) -> T + Sync) -> Vec<T> {
    let next = AtomicUsize::new(0);
    let out = Mutex::new(Vec::with_capacity(paths.len()));
    let threads = std::thread::available_parallelism().map_or(4, |n| n.get().min(8));
    std::thread::scope(|s| {
        for _ in 0..threads {
            s.spawn(|| {
                loop {
                    let i = next.fetch_add(1, Ordering::Relaxed);
                    let Some(path) = paths.get(i) else { break };
                    let bytes = std::fs::read(path).unwrap();
                    let r = f(path, &bytes);
                    out.lock().unwrap().push((i, r));
                }
            });
        }
    });
    let mut out = out.into_inner().unwrap();
    out.sort_by_key(|(i, _)| *i);
    out.into_iter().map(|(_, r)| r).collect()
}

fn rel(path: &Path, root: &Path) -> String {
    path.strip_prefix(root).unwrap().display().to_string()
}

fn level_bld(usrdir: &Path, archive: &str) -> Vec<u8> {
    let bytes = std::fs::read(usrdir.join(archive)).unwrap();
    let arc = Archive::parse(&bytes).unwrap();
    arc.read(arc.find("level.bld").unwrap())
        .unwrap()
        .into_owned()
}

fn name_at(igz: &Igz<'_>, object: usize) -> String {
    let o = igz.objects()[object];
    o.field(8)
        .and_then(|at| igz.string(at, "_name").ok().flatten())
        .unwrap_or("")
        .to_owned()
}

/// One archive's packs: (languages read, voice packs without a
/// `LanguagePackInfo`, the level's key count, failures).
#[derive(Default)]
struct PackRun {
    archive: String,
    languages: Vec<(Option<String>, i32, usize)>,
    voice_packs: usize,
    keys: usize,
    failures: Vec<String>,
}

#[test]
#[ignore = "reads the Giants dump at GUAC_GIANTS_DUMP"]
fn every_language_pack_joins_its_level() {
    let start = Instant::now();
    let usrdir = usrdir();
    let paths = archives(&usrdir);
    let runs = each_archive(&paths, |path, bytes| {
        let mut run = PackRun {
            archive: rel(path, &usrdir),
            ..Default::default()
        };
        let arc = Archive::parse(bytes).unwrap();
        let paks: Vec<_> = arc
            .entries()
            .iter()
            .filter(|e| e.name().ends_with(".pak"))
            .collect();
        if paks.is_empty() {
            return run;
        }
        let mut packs = Vec::new();
        for e in paks {
            let data = arc.read(e).unwrap();
            let igz = Igz::parse(&data).unwrap();
            if !igz
                .objects()
                .iter()
                .any(|o| igz.type_name(o) == LanguagePackInfo::TYPE)
            {
                run.voice_packs += 1;
                continue;
            }
            match LanguagePack::read(&igz) {
                Ok(p) => packs.push(p),
                Err(err) => run
                    .failures
                    .push(format!("{}: {}: {err}", run.archive, e.name())),
            }
        }
        if packs.is_empty() {
            return run;
        }
        run.languages = packs
            .iter()
            .map(|p| (p.name.clone(), p.lang_id, p.strings.len()))
            .collect();
        let Some(level) = arc.find("level.bld") else {
            run.failures
                .push(format!("{}: packs but no level.bld", run.archive));
            return run;
        };
        let level = arc.read(level).unwrap();
        let level = Igz::parse(&level).unwrap();
        match Texts::new(&level, packs) {
            Ok(texts) => run.keys = texts.key_count(),
            Err(err) => run.failures.push(format!("{}: {err}", run.archive)),
        }
        run
    });

    let with_packs: Vec<&PackRun> = runs.iter().filter(|r| !r.languages.is_empty()).collect();
    let voice: Vec<&str> = runs
        .iter()
        .filter(|r| r.voice_packs > 0)
        .map(|r| r.archive.as_str())
        .collect();
    let failures: Vec<&String> = runs.iter().flat_map(|r| &r.failures).collect();
    let packs: usize = with_packs.iter().map(|r| r.languages.len()).sum();
    let keys: usize = with_packs.iter().map(|r| r.keys).sum();
    let languages: BTreeSet<(Option<String>, i32)> = with_packs
        .iter()
        .flat_map(|r| r.languages.iter().map(|(n, id, _)| (n.clone(), *id)))
        .collect();
    println!(
        "{} archives scanned, {} with language packs, {packs} packs, {keys} keys",
        paths.len(),
        with_packs.len()
    );
    println!("voice-line packs (no LanguagePackInfo): {voice:?}");
    println!("languages: {languages:?}");
    println!("failures: {}", failures.len());
    for f in failures.iter().take(20) {
        println!("  {f}");
    }
    println!("wall {:.1} s", start.elapsed().as_secs_f64());

    assert!(failures.is_empty());
    assert_eq!(with_packs.len(), 304);
    assert!(with_packs.iter().all(|r| r.languages.len() == 12));
    assert_eq!(packs, 3_648);
    assert_eq!(keys, 26_260);
    assert_eq!(voice, ["level/challengevo.bld"]);
    let ids: Vec<i32> = languages.iter().map(|(_, id)| *id).collect();
    assert_eq!(languages.len(), 12);
    assert_eq!(
        BTreeSet::from_iter(ids),
        BTreeSet::from([0, 4, 5, 6, 7, 8, 9, 14, 17, 19, 21, 22])
    );
    assert!(languages.contains(&(Some("ENGLISH".into()), 6)));

    // global.bld: 906 keys, every one in every language.
    let global = with_packs
        .iter()
        .find(|r| r.archive == "permanent/global.bld")
        .unwrap();
    assert_eq!(global.keys, 906);
    assert!(global.languages.iter().all(|(_, _, n)| *n == 906));
}

/// The objects that read, for the caller's own checks; the counts feed the
/// summary line.
fn tally<'a, T: Layout<'a>>(
    igz: &Igz<'a>,
    name: &str,
    read: &mut BTreeMap<&'static str, u64>,
    failures: &mut Vec<String>,
) -> Vec<(usize, T)> {
    let mut out = Vec::new();
    for (i, r) in layouts::read_all::<T>(igz) {
        match r {
            Ok(v) => {
                *read.entry(T::TYPE).or_default() += 1;
                out.push((i, v));
            }
            Err(e) => failures.push(format!("{name}: {} #{i}: {e}", T::TYPE)),
        }
    }
    out
}

/// One archive's IGZ entries: the `global.bld` imports they hold (id ->
/// references), the roster layouts read, the `_texAttr` targets seen and
/// failures.
#[derive(Default)]
struct ImportRun {
    files: u64,
    imports: BTreeMap<u32, u64>,
    read: BTreeMap<&'static str, u64>,
    tex_attr_targets: BTreeMap<String, u64>,
    failures: Vec<String>,
}

#[test]
#[ignore = "reads the Giants dump at GUAC_GIANTS_DUMP"]
fn every_global_import_resolves_and_every_roster_layout_reads() {
    let start = Instant::now();
    let usrdir = usrdir();
    let boot_bytes = level_bld(&usrdir, "permanent/bootstrap.bld");
    let global_bytes = level_bld(&usrdir, "permanent/global.bld");
    let boot = Igz::parse(&boot_bytes).unwrap();
    let global = Igz::parse(&global_bytes).unwrap();
    let exports = Exports::new(&boot, &global).unwrap();
    assert_eq!(exports.global_base(), GLOBAL_BASE);
    assert_eq!(exports.global_roots(), 2_710);

    let namespace = fnv1a32(NAMESPACE.as_bytes());
    let paths = archives(&usrdir);
    let runs = each_archive(&paths, |path, bytes| {
        let mut run = ImportRun::default();
        let arc_name = rel(path, &usrdir);
        let arc = Archive::parse(bytes).unwrap();
        for e in arc.entries() {
            let Ok(head) = arc.read_chunk(e, 0) else {
                continue;
            };
            if !head.starts_with(guac_formats::igz::MAGIC) {
                continue;
            }
            let name = format!("{arc_name}: {}", e.name());
            let data = match arc.read(e) {
                Ok(d) => d,
                Err(err) => {
                    run.failures.push(format!("{name}: {err}"));
                    continue;
                }
            };
            let igz = match Igz::parse(&data) {
                Ok(i) => i,
                Err(err) => {
                    run.failures.push(format!("{name}: {err}"));
                    continue;
                }
            };
            run.files += 1;
            for id in igz.external_ids() {
                if id.namespace == namespace {
                    *run.imports.entry(id.object).or_default() += 1;
                }
            }
            let (r, f) = (&mut run.read, &mut run.failures);
            tally::<StringInfo>(&igz, &name, r, f);
            tally::<ValueInfo>(&igz, &name, r, f);
            tally::<ScriptSet>(&igz, &name, r, f);
            tally::<ScriptSetList>(&igz, &name, r, f);
            tally::<DirectionalLightInfo>(&igz, &name, r, f);
            tally::<LanguagePackInfo>(&igz, &name, r, f);
            tally::<SpriteInfo>(&igz, &name, r, f);
            for (_, d) in tally::<AnimatableTextureDataInfo>(&igz, &name, r, f) {
                let target = d.tex_attr.map_or("null".to_owned(), |t| {
                    igz.type_name(&igz.objects()[t]).to_owned()
                });
                *run.tex_attr_targets.entry(target).or_default() += 1;
            }
        }
        run
    });

    let mut imports: BTreeMap<u32, u64> = BTreeMap::new();
    let mut read: BTreeMap<&str, u64> = BTreeMap::new();
    let mut targets: BTreeMap<String, u64> = BTreeMap::new();
    let mut failures = Vec::new();
    let mut files = 0;
    for run in runs {
        files += run.files;
        for (id, n) in run.imports {
            *imports.entry(id).or_default() += n;
        }
        for (t, n) in run.read {
            *read.entry(t).or_default() += n;
        }
        for (t, n) in run.tex_attr_targets {
            *targets.entry(t).or_default() += n;
        }
        failures.extend(run.failures);
    }
    let mut resolved = BTreeMap::<&str, u64>::new();
    let mut unresolved = Vec::new();
    let mut below = BTreeMap::new();
    for (&id, &refs) in &imports {
        match exports.resolve(id) {
            Ok(Export::Bootstrap(Root::Object(o))) => {
                *resolved.entry("bootstrap").or_default() += 1;
                below.insert(id, (name_at(&boot, o), refs));
            }
            Ok(Export::Global(Root::Object(_))) => *resolved.entry("global").or_default() += 1,
            Ok(other) => unresolved.push(format!("{id:#x}: lands on {other:?}")),
            Err(e) => unresolved.push(format!("{id:#x}: {e}")),
        }
    }
    println!("{} archives, {files} IGZ", paths.len());
    println!(
        "{} distinct global.bld export ids, {} references; resolved {resolved:?}; unresolved {}",
        imports.len(),
        imports.values().sum::<u64>(),
        unresolved.len()
    );
    println!("under {GLOBAL_BASE:#x} (bootstrap roots): {below:?}");
    println!("roster layouts read: {read:?}");
    println!("_texAttr targets: {targets:?}");
    println!("failures: {}", failures.len());
    for f in failures.iter().take(20) {
        println!("  {f}");
    }
    println!("wall {:.1} s", start.elapsed().as_secs_f64());

    assert!(failures.is_empty());
    assert!(unresolved.is_empty(), "{unresolved:?}");
    assert_eq!(imports.len(), 1_208);
    let below_names: Vec<(u32, &str)> =
        below.iter().map(|(id, (n, _))| (*id, n.as_str())).collect();
    assert_eq!(
        below_names,
        [
            (3, "Opaque_Background"),
            (4, "fonts/simple.png"),
            (11, "System Monitors"),
            (13, "System Monitor Controls"),
            (15, "LoadLevel_OnLaunch"),
            (19, "Home Button Disabled"),
        ]
    );
    assert_eq!(below.values().map(|(_, n)| n).sum::<u64>(), 27);
}

#[test]
#[ignore = "reads the Giants dump at GUAC_GIANTS_DUMP"]
fn the_effects_ae_names_resolve_and_the_sprite_rig_reads() {
    let usrdir = usrdir();
    let boot_bytes = level_bld(&usrdir, "permanent/bootstrap.bld");
    let global_bytes = level_bld(&usrdir, "permanent/global.bld");
    let boot = Igz::parse(&boot_bytes).unwrap();
    let global = Igz::parse(&global_bytes).unwrap();
    let exports = Exports::new(&boot, &global).unwrap();
    let effect = |id| -> Result<(String, String), FormatError> {
        let Export::Global(Root::Object(o)) = exports.resolve(id)? else {
            panic!("{id:#x} isn't global's")
        };
        Ok((
            global.type_name(&global.objects()[o]).to_owned(),
            name_at(&global, o),
        ))
    };
    // Named by their source path, c:/tfb/bin/shaders/<file>.
    for (id, file) in [(0xAA7, "tfbmulti2.fx"), (0xAA9, "tfbwarp.fx")] {
        let (ty, name) = effect(id).unwrap();
        println!("{id:#x}: {ty} {name}");
        assert_eq!(ty, "igEffect");
        assert_eq!(name.rsplit('/').next(), Some(file));
    }

    let rig: Vec<DirectionalLightInfo> = layouts::read_all::<DirectionalLightInfo>(&global)
        .map(|(_, r)| r.unwrap())
        .collect();
    for l in &rig {
        println!(
            "{:?}: diffuse {:?} ambient {:?} specular {:?} x{} at {:?} rotated {:?}",
            l.name,
            l.diffuse,
            l.ambient,
            l.specular,
            l.multiplier,
            l.init_matrix.position,
            l.init_matrix.rotation
        );
    }
    let names: Vec<_> = rig.iter().map(|l| l.name.unwrap()).collect();
    assert_eq!(names, ["_spriteLight1", "_spriteLight2", "_spriteLight3"]);
    assert_eq!(rig[0].diffuse, [80, 78, 66, 255]);
    assert_eq!(rig[0].init_matrix.rotation[0], -45.0);
}
