//! The interface on the real Giants dump (openspec AT.8): the extract-wide
//! part and chapter 1's `sprites` part into a throwaway identity dir, in the
//! release build's format, then both parts taken off the manifest (an
//! extract made before them) and read again alone. Checked: `global.bld`'s
//! templates, strings and font, `bootstrap.bld`'s, the character and menu
//! archives' templates, the menus' strings, chapter 1's templates and
//! strings, every texture the IR names in the extract. Printed: each run's
//! time, files and bytes with the load average. Ignored by default: it
//! reads `GUAC_GIANTS_DUMP` and writes under `GUAC_EXTRACT_DIR` (removed
//! after), from the environment or the workspace's `.env.dev`:
//!
//! `cargo test -p guac-extract --release --test giants_interface -- --ignored --nocapture`

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::Instant;

use guac_extract::interface::{self, PART, interface_stage};
use guac_extract::intros::IrFormat;
use guac_extract::known::KNOWN_DUMPS;
use guac_extract::{DirState, ExtractRoot, IdentityDir, levels, open, stages};
use guac_ir::interface::{Content, Interface};

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

fn load_average() -> String {
    std::process::Command::new("sysctl")
        .args(["-n", "vm.loadavg"])
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_owned())
        .unwrap_or_default()
}

const IR: IrFormat = IrFormat::Postcard;
const LEVEL: &str = levels::FIRST_RUN[0];

/// Every texture an archive's IR names, recorded and on disk.
fn textures_named(ir: &Interface) -> Vec<String> {
    let mut out: Vec<String> = ir.fonts.iter().map(|f| f.atlas.clone()).collect();
    for t in &ir.templates {
        if let Content::Image(a) = &t.content
            && let Some(p) = &a.texture
        {
            out.push(p.clone());
        }
    }
    out.sort();
    out.dedup();
    out
}

fn decode(dir: &IdentityDir, rel: &str) -> Interface {
    IR.decode(dir, rel).unwrap()
}

/// The sprites part's record and file, as an extract made before it lacks
/// them, and the interface part's record.
fn forget_parts(dir: &Path) {
    let path = dir.join("manifest.json");
    let mut json: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
    json["parts"].as_object_mut().unwrap().remove(PART).unwrap();
    json["levels"][LEVEL]["parts"]
        .as_object_mut()
        .unwrap()
        .remove(levels::SPRITES)
        .unwrap();
    std::fs::write(&path, serde_json::to_string(&json).unwrap()).unwrap();
}

#[test]
#[ignore = "reads GUAC_GIANTS_DUMP; run on the Mac"]
fn the_interface_extracts_and_resumes_alone() {
    let dump = PathBuf::from(env_or_dotenv("GUAC_GIANTS_DUMP").expect("set GUAC_GIANTS_DUMP"));
    let base = PathBuf::from(env_or_dotenv("GUAC_EXTRACT_DIR").expect("set GUAC_EXTRACT_DIR"));
    std::fs::create_dir_all(&base).unwrap();
    let tmp = tempfile::Builder::new()
        .prefix("at8-interface-")
        .tempdir_in(&base)
        .unwrap();
    let root = ExtractRoot::checked(tmp.path().join("extract")).unwrap();
    let mut opened = open(&dump, &root, KNOWN_DUMPS, &|_| {}).expect("the known dump");
    assert_eq!(opened.state, DirState::Fresh);
    let dir_path = opened.dir.path().to_path_buf();

    // Chapter 1's parts, the sprites among them.
    println!("load average before the level: {}", load_average());
    let lv = stages::level_stage(&mut opened.dir, IR, &|_| {}).unwrap();
    assert!(lv.failed.is_empty(), "{:?}", lv.failed);
    assert_eq!(lv.parts_read, levels::PARTS.len());
    println!(
        "  chapter 1's level stage: {} parts, {:?} in {:.2} s; load average {}",
        lv.parts_read,
        lv.files,
        lv.took.as_secs_f64(),
        load_average()
    );
    let sprites = decode(&opened.dir, &IR.level_path(LEVEL, levels::SPRITES));
    println!(
        "  level 213: {} templates, {} strings, {} fonts, {} textures, refused {:?}",
        sprites.templates.len(),
        sprites.strings.len(),
        sprites.fonts.len(),
        textures_named(&sprites).len(),
        sprites.refused
    );
    assert_eq!((sprites.templates.len(), sprites.strings.len()), (31, 187));

    println!("load average before the part: {}", load_average());
    let s = interface_stage(&mut opened.dir, IR).unwrap();
    println!(
        "  first run: {} archives, {} templates, {} fonts, {} strings, {} textures ({:?}); {:?} in {:.2} s; load average {}",
        s.archives,
        s.templates,
        s.fonts,
        s.strings,
        s.textures,
        s.texture_files,
        s.files,
        s.took.as_secs_f64(),
        load_average()
    );
    assert!(s.failed.is_none(), "{:?}", s.failed);
    assert!(s.missing.is_empty(), "{:?}", s.missing);
    let mut refused: BTreeMap<String, usize> = BTreeMap::new();
    for (what, why) in &s.refused {
        *refused.entry(why.clone()).or_default() += 1;
        let _ = what;
    }
    println!("  refused by reason: {refused:?}");
    for (what, why) in s.refused.iter().take(10) {
        println!("    {what}: {why}");
    }

    let archives = interface::archives(opened.dir.manifest());
    let read = |a: &str| decode(&opened.dir, &interface::path(IR, a));
    let global = read("permanent/global.bld");
    let global_textures = textures_named(&global);
    println!(
        "  global.bld: {} templates, {} strings, {} textures named",
        global.templates.len(),
        global.strings.len(),
        global_textures.len()
    );
    assert_eq!(global.templates.len(), 409);
    // Of the file's 310 images, the 190 its art templates draw and
    // `contemp`'s atlas; no template or font reaches the other 119.
    assert_eq!(global_textures.len(), 191);
    assert_eq!(global.strings.len(), 906);
    assert_eq!(global.fonts.len(), 1);
    assert_eq!(global.fonts[0].glyphs.len(), 178);
    let bootstrap = read("permanent/bootstrap.bld");
    assert_eq!(bootstrap.templates.len(), 2);
    assert_eq!(bootstrap.fonts.len(), 1);
    assert_eq!(
        bootstrap.fonts[0].name.as_deref(),
        Some("hdd:/tfb/content/fonts/simple")
    );
    let (mut characters, mut with) = (0usize, 0usize);
    let (mut menus, mut menu_strings, mut menu_archives) = (0usize, 0usize, 0usize);
    let mut streamed = 0usize;
    let mut part_textures = Vec::new();
    for a in &archives {
        let ir = read(a);
        part_textures.extend(textures_named(&ir));
        if a.starts_with("character/") {
            characters += ir.templates.len();
            with += usize::from(!ir.templates.is_empty());
            streamed += ir
                .templates
                .iter()
                .filter(|t| {
                    matches!(&t.content, Content::Image(a)
                    if a.stream_archive_hash != 0 && a.texture.is_some())
                })
                .count();
        }
        if interface::is_menu(a) {
            menu_archives += 1;
            menus += ir.templates.len();
            menu_strings += ir.strings.len();
        }
    }
    println!(
        "  characters: {characters} templates in {with} of {} archives, {streamed} upgrade images out of their .arc; menus: {menus} templates and {menu_strings} strings in {menu_archives}",
        archives
            .iter()
            .filter(|a| a.starts_with("character/"))
            .count()
    );
    assert_eq!((characters, with), (1_970, 75));
    assert_eq!((menus, menu_strings, menu_archives), (887, 506, 16));
    part_textures.sort();
    part_textures.dedup();
    for t in &part_textures {
        assert!(opened.dir.output_done(t).unwrap(), "{t}");
    }

    // An extract made before the parts: both records gone, everything else
    // as it stands. The next start reads the sprites part and the interface
    // part alone, keeping every file already in.
    drop(opened);
    forget_parts(&dir_path);
    let mut opened = open(&dump, &root, KNOWN_DUMPS, &|_| {}).unwrap();
    println!("load average before the resume: {}", load_average());
    let started = Instant::now();
    let lv = stages::level_stage(&mut opened.dir, IR, &|_| {}).unwrap();
    let again = interface_stage(&mut opened.dir, IR).unwrap();
    println!(
        "  resumed: level {} read, {} kept in {:.2} s; interface read {} ({:?}) in {:.2} s; {:.2} s together; load average {}",
        lv.parts_read,
        lv.parts_kept,
        lv.took.as_secs_f64(),
        again.read,
        again.files,
        again.took.as_secs_f64(),
        started.elapsed().as_secs_f64(),
        load_average()
    );
    assert_eq!((lv.parts_read, lv.parts_kept), (1, levels::PARTS.len() - 1));
    assert!(again.read && again.failed.is_none());
    assert_eq!(again.files.written, 0, "every file already in is kept");
    assert!(levels::complete(opened.dir.manifest()));
    assert!(opened.dir.manifest().parts.contains_key(PART));
}

/// What an extract made before the parts pays on its next start: the dev
/// extract under `GUAC_EXTRACT_DIR` as it stands, cloned file by file (APFS,
/// `cp -c`) into a throwaway root, then every stage guacamole's Giants pick
/// runs on an extract that isn't current (`stages::run`, ffmpeg from
/// `GUAC_FFMPEG` or found), in the format the extract holds: each kept part
/// re-hashed, chapter 1's `sprites` part and the interface part read, the
/// intros resumed. Printed: each stage's time and the whole. The dev extract
/// itself is never opened. Skips when there's no extract of this dump there,
/// or one already holding the interface part.
#[test]
#[ignore = "reads GUAC_GIANTS_DUMP and the dev extract; run on the Mac"]
fn the_parts_first_run_on_an_extract_made_before_them() {
    let dump = PathBuf::from(env_or_dotenv("GUAC_GIANTS_DUMP").expect("set GUAC_GIANTS_DUMP"));
    let base = PathBuf::from(env_or_dotenv("GUAC_EXTRACT_DIR").expect("set GUAC_EXTRACT_DIR"));
    let source_dir = dump.canonicalize().unwrap().to_string_lossy().into_owned();
    let existing = std::fs::read_dir(&base).unwrap().find_map(|e| {
        let path = e.ok()?.path();
        let text = std::fs::read_to_string(path.join("manifest.json")).ok()?;
        let m: serde_json::Value = serde_json::from_str(&text).ok()?;
        (m["source_dir"] == source_dir.as_str() && m["complete"] == true).then_some((path, m))
    });
    let Some((existing, manifest)) = existing else {
        println!(
            "no complete extract of {source_dir} under {}",
            base.display()
        );
        return;
    };
    if manifest["parts"].get(PART).is_some() {
        println!("{} holds the interface part already", existing.display());
        return;
    }
    let ir = if existing.join("ir/roster.ron").is_file() {
        IrFormat::Ron
    } else {
        IrFormat::Postcard
    };
    let outputs = manifest["outputs"].as_array().map_or(0, Vec::len);
    let tmp = tempfile::Builder::new()
        .prefix("at8-existing-")
        .tempdir_in(&base)
        .unwrap();
    let clone_root = tmp.path().join("extract");
    std::fs::create_dir_all(&clone_root).unwrap();
    let started = Instant::now();
    let cp = std::process::Command::new("cp")
        .arg("-cR")
        .arg(&existing)
        .arg(&clone_root)
        .status()
        .unwrap();
    assert!(cp.success(), "cp -cR {}", existing.display());
    println!(
        "  cloned {} ({outputs} outputs, {ir:?}) in {:.2} s",
        existing.display(),
        started.elapsed().as_secs_f64()
    );

    let root = ExtractRoot::checked(clone_root).unwrap();
    let opened_at = Instant::now();
    let mut opened = open(&dump, &root, KNOWN_DUMPS, &|_| {}).expect("the known dump");
    assert_eq!(opened.state, DirState::Resume, "the parts missing");
    println!(
        "  opened ({} hashed, {} cached) in {:.2} s",
        opened.files_hashed,
        opened.files_cached,
        opened_at.elapsed().as_secs_f64()
    );
    let ffmpeg = env_or_dotenv("GUAC_FFMPEG").map(PathBuf::from);
    println!("load average before the run: {}", load_average());
    let started = Instant::now();
    let x = stages::run(&mut opened.dir, ffmpeg.as_deref(), ir, &|_| {}).unwrap();
    let took = started.elapsed();
    let lv = &x.levels;
    let s = &x.interface;
    println!(
        "  roster {:.2} s, progression {:.2} s, models {:.2} s, level {:.2} s ({} part read, {} kept, {:?}), scripts {:.2} s, particles {:.2} s, sounds {:.2} s, interface {:.2} s, intros {:?} s",
        x.roster.took.as_secs_f64(),
        x.progression.took.as_secs_f64(),
        x.models.took.as_secs_f64(),
        lv.took.as_secs_f64(),
        lv.parts_read,
        lv.parts_kept,
        lv.files,
        x.scripts.took.as_secs_f64(),
        x.particles.took.as_secs_f64(),
        x.sounds.took.as_secs_f64(),
        s.took.as_secs_f64(),
        x.intros
            .as_ref()
            .map(|i| i.took.as_secs_f64())
            .map_err(|e| e.to_string()),
    );
    println!(
        "  interface: {} archives, {} textures ({:?}); {:?}",
        s.archives, s.textures, s.texture_files, s.files
    );
    let after = opened.dir.manifest().outputs.len();
    println!(
        "  the whole start: {:.2} s, outputs {outputs} -> {after}; load average {}",
        took.as_secs_f64(),
        load_average()
    );
    assert_eq!((lv.parts_read, lv.parts_kept), (1, levels::PARTS.len() - 1));
    assert!(x.sounds.kept && x.particles.kept, "nothing else read again");
    assert!(s.read && s.failed.is_none(), "{:?}", s.failed);
    assert!(s.missing.is_empty(), "{:?}", s.missing);
    assert!(x.intros.is_ok(), "{:?}", x.intros.as_ref().err());
    assert!(opened.dir.manifest().parts.contains_key(PART));
    assert!(opened.dir.manifest().complete);
}
