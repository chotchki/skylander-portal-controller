//! The interface on the real Giants dump (openspec AT.8): the extract-wide
//! part and chapter 1's `sprites` part into a throwaway identity dir, in the
//! release build's format, then both parts taken off the manifest (an
//! extract made before them) and read again alone. Checked: `global.bld`'s
//! templates, strings and font, `bootstrap.bld`'s, the character and menu
//! archives' templates, the menus' strings, chapter 1's templates and
//! strings, every texture the IR names in the extract, and every string's
//! settings beside its text (AT.27): each `@` string counted by its flags
//! and digits. Printed: each run's time, files and bytes with the load
//! average. Ignored by default: it
//! reads `GUAC_GIANTS_DUMP` and writes under `GUAC_EXTRACT_DIR` (removed
//! after), from the environment or the workspace's `.env.dev`:
//!
//! `cargo test -p guac-extract --profile disc --test giants_interface -- --ignored --nocapture`

use std::collections::{BTreeMap, BTreeSet};
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
    let sprites = decode(&opened.dir, &levels::part_path(IR, LEVEL, levels::SPRITES));
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
    let s = interface_stage(&mut opened.dir, IR, &|_| {}).unwrap();
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
    // Each template's two matrices (AT.25): the corner shades turn by
    // their initial matrices' depth turn, as the June savestate's do.
    let mut turned = 0usize;
    for t in &global.templates {
        if t.init_matrix.rotation != [0.0; 3] || t.matrix.rotation != [0.0; 3] {
            turned += 1;
        }
        if t.name.starts_with("GameplayOverlay_") {
            println!(
                "    {}: initial {:?} {:?}, current {:?} {:?}",
                t.name,
                t.init_matrix.position,
                t.init_matrix.rotation,
                t.matrix.position,
                t.matrix.rotation
            );
        }
    }
    println!("  global.bld: {turned} templates turned by a matrix");
    assert_eq!(turned, 21);
    let shade = |n: &str| {
        let t = global.templates.iter().find(|t| t.name == n).unwrap();
        assert_eq!(t.matrix, Default::default(), "{n}: current, the file's");
        t.init_matrix.rotation
    };
    assert_eq!(shade("GameplayOverlay_BL"), [0.0, 90.0, 0.0]);
    assert_eq!(shade("GameplayOverlay_BR"), [0.0, 180.00078, 0.0]);
    assert_eq!(shade("GameplayOverlay_UL"), [0.0; 3]);
    assert_eq!(shade("GameplayOverlay_UR"), [0.0, 270.0, 0.0]);
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

    // Every string's settings beside its text (AT.27), over every archive
    // the two parts read: the `@` strings by flags and digits, the strings
    // whose flags say a number or a time, and any flag bits past those.
    let mut irs: Vec<(String, Interface)> = archives.iter().map(|a| (a.clone(), read(a))).collect();
    irs.push((format!("level/{LEVEL}.bld"), sprites.clone()));
    let (mut strings, mut at, mut flagged, mut flagged_without_at) =
        (0usize, 0usize, 0usize, 0usize);
    let mut by_settings: BTreeMap<(i32, i32, i32), usize> = BTreeMap::new();
    let mut flags_seen: BTreeSet<i32> = BTreeSet::new();
    let mut unpacked = Vec::new();
    for (a, ir) in &irs {
        strings += ir.strings.len();
        for st in &ir.strings {
            flags_seen.insert(st.flags);
            let has_at = st.text.contains('@');
            let number = st.flags & (0x800 | 0x400) != 0;
            if has_at {
                at += 1;
                *by_settings
                    .entry((st.flags, st.integer_digits, st.decimal_digits))
                    .or_default() += 1;
            }
            if number {
                flagged += 1;
                if !has_at {
                    flagged_without_at += 1;
                    if flagged_without_at <= 5 {
                        println!(
                            "    flagged without an @: {a}: {:?} {:#x}",
                            st.text, st.flags
                        );
                    }
                }
            }
        }
        if ir.strings.is_empty() && ir.templates.iter().any(|t| t.string.is_some()) {
            unpacked.push(a.clone());
        }
    }
    println!(
        "  strings: {strings} in {} archives, {at} with an @, {flagged} flagged a number or a time ({flagged_without_at} of them without an @); flags seen {:x?}",
        irs.len(),
        flags_seen
    );
    for ((flags, int, dec), n) in &by_settings {
        println!("    @ strings with flags {flags:#06x}, {int} integer digits, {dec} decimal: {n}");
    }
    println!("  archives whose templates name strings and that hold none: {unpacked:?}");

    // The templates holding flags bit 21 (AT.29): no member writes the bit,
    // so a sprite holds it only as its template does; a drawn container
    // with it joins the frame's sprite list.
    let mut bit21: BTreeMap<&str, usize> = BTreeMap::new();
    let mut bit21_containers: BTreeMap<String, usize> = BTreeMap::new();
    for (a, ir) in &irs {
        for t in &ir.templates {
            if t.flags & (1 << 21) == 0 {
                continue;
            }
            let kind = match &t.content {
                Content::Container => "container",
                Content::Image(_) | Content::Movie(_) => "art",
                Content::Text(_) => "text",
                Content::Model(_) => "model",
                Content::Emitter(_) => "particles",
                Content::Empty(_) => "empty",
                Content::Imported(..) => "imported",
            };
            *bit21.entry(kind).or_default() += 1;
            if kind == "container" {
                *bit21_containers
                    .entry(format!("{a}: {}", t.name))
                    .or_default() += 1;
            }
        }
    }
    println!("  flags bit 21 by kind: {bit21:?}");
    for (n, c) in bit21_containers.iter().take(20) {
        println!("    container with bit 21: {n} x{c}");
    }
    assert_eq!(
        strings,
        s.strings + sprites.strings.len(),
        "every string the part counted"
    );
    // The disc's (AT.27): 4,146 strings, the 103 with an `@` each flagged
    // *has an `@`* (0x800), five of them a time too (0x400), none with
    // commas; no string flagged a number or a time without an `@`.
    assert_eq!((strings, irs.len(), at, flagged), (4_146, 177, 103, 103));
    assert_eq!(flagged_without_at, 0);
    assert_eq!(flags_seen, BTreeSet::from([0, 0x800, 0xC00]));
    assert_eq!(
        by_settings,
        BTreeMap::from([
            ((0x800, 0, 0), 96),
            ((0x800, 0, 2), 2),
            ((0x800, 0, 3), 2),
            ((0xC00, 3, 1), 1),
            ((0xC00, 4, 0), 2),
        ])
    );
    assert!(unpacked.is_empty(), "{unpacked:?}");

    // An extract made before the parts: both records gone, everything else
    // as it stands. The next start reads the sprites part and the interface
    // part alone, keeping every file already in.
    drop(opened);
    forget_parts(&dir_path);
    let mut opened = open(&dump, &root, KNOWN_DUMPS, &|_| {}).unwrap();
    println!("load average before the resume: {}", load_average());
    let started = Instant::now();
    let lv = stages::level_stage(&mut opened.dir, IR, &|_| {}).unwrap();
    let again = interface_stage(&mut opened.dir, IR, &|_| {}).unwrap();
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
    let ir = if existing.join("ir/roster.ron").is_file() {
        IrFormat::Ron
    } else {
        IrFormat::Postcard
    };
    // An extract holding the part in this record's shape has nothing to
    // read; one holding AT.8's (its files without the shape in their names)
    // reads it again (AT.27).
    let global = interface::path(ir, "permanent/global.bld");
    if manifest["parts"][PART]
        .as_array()
        .is_some_and(|files| files.iter().any(|f| *f == global.as_str()))
    {
        println!("{} holds the interface part already", existing.display());
        return;
    }
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
    let sp = &x.sprites;
    let s = &x.interface;
    println!(
        "  roster {:.2} s, progression {:.2} s, models {:.2} s, level {:.2} s ({} parts read, {} kept), scripts {:.2} s, particles {:.2} s, then chapter 1's hand-off; sprites {:.2} s ({} read, {:?}), sounds {:.2} s, interface {:.2} s, intros {:?} s",
        x.roster.took.as_secs_f64(),
        x.progression.took.as_secs_f64(),
        x.models.took.as_secs_f64(),
        lv.took.as_secs_f64(),
        lv.parts_read,
        lv.parts_kept,
        x.scripts.took.as_secs_f64(),
        x.particles.took.as_secs_f64(),
        sp.took.as_secs_f64(),
        sp.parts_read,
        sp.files,
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
    // The parts the TV reads all kept; the sprites read after the hand-off
    // (AT.26).
    assert_eq!((lv.parts_read, lv.parts_kept), (0, levels::PLAYED.len()));
    assert_eq!((sp.parts_read, sp.parts_kept), (1, 0));
    assert!(x.sounds.kept && x.particles.kept, "nothing else read again");
    assert!(s.read && s.failed.is_none(), "{:?}", s.failed);
    assert!(s.missing.is_empty(), "{:?}", s.missing);
    assert!(x.intros.is_ok(), "{:?}", x.intros.as_ref().err());
    assert!(opened.dir.manifest().parts.contains_key(PART));
    assert!(opened.dir.manifest().complete);
}
