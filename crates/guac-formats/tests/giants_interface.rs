//! The interface's templates and fonts off the Giants dump (openspec AT.1,
//! AT.2): every `.bld`'s `level.bld` walked, each `tfbSpriteInfo` and its
//! `SpriteInfoList` read by the layouts, its bitfields checked against what
//! it draws, and the three bitmap fonts read down to their glyphs.
//! Ignored by default; it reads `GUAC_GIANTS_DUMP`:
//!
//! `cargo test -p guac-formats --profile disc --test giants_interface -- --ignored --nocapture`

mod common;

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use common::usrdir;
use guac_formats::exports::{Export, Exports, Root};
use guac_formats::iga::Archive;
use guac_formats::igz::Igz;
use guac_formats::igz::layouts::{
    BitmapFont, CharMetrics, CharMetricsList, FontDataInfo, Layout, ObjectRef, SpriteInfo,
    SpriteInfoList, VISUAL_TYPES, read_all,
};

/// Sorted, so the census prints in the same order every run.
fn blds(dir: &Path, out: &mut Vec<PathBuf>) {
    let mut entries: Vec<_> = std::fs::read_dir(dir)
        .unwrap()
        .map(|e| e.unwrap().path())
        .collect();
    entries.sort();
    for path in entries {
        if path.is_dir() {
            blds(&path, out);
        } else if path.extension().is_some_and(|e| e == "bld") {
            out.push(path);
        }
    }
}

/// Every `.bld` under `USRDIR`, as paths under it.
fn bld_names() -> (PathBuf, Vec<String>) {
    let root = usrdir();
    let mut paths = Vec::new();
    blds(&root, &mut paths);
    let names = paths
        .iter()
        .map(|p| {
            p.strip_prefix(&root)
                .unwrap()
                .to_string_lossy()
                .into_owned()
        })
        .collect();
    (root, names)
}

/// One `.bld`'s `level.bld`, read on demand so a walk holds one at a time
/// (the disc's 599 together run to gigabytes).
fn level_bld(root: &Path, name: &str) -> Option<Vec<u8>> {
    let bytes = std::fs::read(root.join(name)).unwrap();
    let archive = Archive::parse(&bytes).unwrap();
    let entry = archive
        .entries()
        .iter()
        .find(|e| e.name().ends_with("level.bld"))?;
    Some(archive.read(entry).unwrap().into_owned())
}

/// What a template's context is, by its object's type (an import into
/// `global.bld` by the type of what its export id lands on): what its visual
/// type has to agree with.
fn context_kind(
    igz: &Igz<'_>,
    context: ObjectRef<'_>,
    shared: (&Exports, &Igz<'_>, &Igz<'_>),
) -> String {
    let (exports, bootstrap, global) = shared;
    match context {
        ObjectRef::Null => "none".into(),
        ObjectRef::Object(i) => igz.type_name(&igz.objects()[i]).to_owned(),
        ObjectRef::Import(i) => match exports.resolve_import(&i.id) {
            Some(Ok(Export::Global(Root::Object(o)))) => {
                global.type_name(&global.objects()[o]).to_owned()
            }
            Some(Ok(Export::Bootstrap(Root::Object(o)))) => {
                bootstrap.type_name(&bootstrap.objects()[o]).to_owned()
            }
            other => format!(
                "import {:08x}:{:#x} ({other:?})",
                i.id.namespace, i.id.object
            ),
        },
        ObjectRef::Named { name, .. } => format!("named {}::{}", name.namespace, name.name),
    }
}

/// The visual type a context calls for (AT.1's Verify).
fn expected_kind(context: &str) -> Option<&'static str> {
    Some(match context {
        "none" => "container",
        "AnimatableTextureDataInfo" => "art",
        "FontDataInfo" => "text",
        "tfbPhysicsModel" => "model",
        "tfbParticleInfo" => "particles",
        _ => return None,
    })
}

#[test]
#[ignore = "reads the Giants dump at GUAC_GIANTS_DUMP"]
fn every_template_and_font_reads_by_the_layouts() {
    let mut templates = 0usize;
    let mut files_with = 0usize;
    let mut failures = Vec::new();
    let mut pairs: BTreeMap<(String, String), usize> = BTreeMap::new();
    let mut by_area: BTreeMap<String, usize> = BTreeMap::new();
    let mut point_sizes: BTreeMap<(String, u16), usize> = BTreeMap::new();
    let mut fonts = Vec::new();
    let mut lists_nonempty = 0usize;
    let mut contextless: BTreeMap<&str, usize> = BTreeMap::new();
    let mut elsewhere: BTreeMap<&str, usize> = BTreeMap::new();

    let (root, names) = bld_names();
    let bootstrap_bytes = level_bld(&root, "permanent/bootstrap.bld").unwrap();
    let global_bytes = level_bld(&root, "permanent/global.bld").unwrap();
    let bootstrap = Igz::parse(&bootstrap_bytes).unwrap();
    let global = Igz::parse(&global_bytes).unwrap();
    let exports = Exports::new(&bootstrap, &global).unwrap();
    let mut imported_contexts = 0usize;
    for name in names {
        let Some(bytes) = level_bld(&root, &name) else {
            continue;
        };
        let igz = match Igz::parse(&bytes) {
            Ok(igz) => igz,
            Err(e) => {
                failures.push(format!("{name}: {e}"));
                continue;
            }
        };
        let mut here = 0usize;
        for (i, sprite) in read_all::<SpriteInfo>(&igz) {
            let sprite = match sprite {
                Ok(s) => s,
                Err(e) => {
                    failures.push(format!("{name} #{i}: {e}"));
                    continue;
                }
            };
            here += 1;
            let bits = sprite.bits();
            let kind = VISUAL_TYPES
                .get(usize::from(bits.visual_type))
                .copied()
                .unwrap_or("past the enum");
            if matches!(sprite.context_data_info, ObjectRef::Import(_)) {
                imported_contexts += 1;
            }
            let context = context_kind(
                &igz,
                sprite.context_data_info,
                (&exports, &bootstrap, &global),
            );
            if context == "none" && kind != "container" {
                // A visual type with nothing in the file to draw from.
                *contextless.entry(kind).or_default() += 1;
            } else if context.starts_with("import ") {
                // From a namespace other than global.bld's: not resolvable
                // from the walk.
                *elsewhere.entry(kind).or_default() += 1;
            } else if expected_kind(&context) != Some(kind) {
                failures.push(format!(
                    "{name} #{i} {:?}: {kind} on a {context}",
                    sprite.placement.name
                ));
            }
            *pairs.entry((kind.to_owned(), context)).or_default() += 1;
            for (anchor, value) in [
                ("alignment", bits.alignment),
                ("align to parent", bits.align_to_parent),
            ] {
                if value > 8 {
                    failures.push(format!("{name} #{i}: {anchor} {value}"));
                }
            }
            if kind == "text" {
                *point_sizes
                    .entry((name.clone(), bits.point_size))
                    .or_default() += 1;
            }
            match sprite.placement.attached_objects {
                ObjectRef::Object(list) => match SpriteInfoList::read(&igz, list) {
                    Ok(l) if l.elements.is_empty() => {}
                    Ok(_) => lists_nonempty += 1,
                    Err(e) => failures.push(format!("{name} #{i}'s list: {e}")),
                },
                other => failures.push(format!("{name} #{i}: attached objects {other:?}")),
            }
        }
        if here > 0 {
            files_with += 1;
            templates += here;
            let area = name.split('/').next().unwrap_or("").to_owned();
            *by_area.entry(area).or_default() += here;
        }

        for (i, data) in read_all::<FontDataInfo>(&igz) {
            let data = match data {
                Ok(d) => d,
                Err(e) => {
                    failures.push(format!("{name} font data #{i}: {e}"));
                    continue;
                }
            };
            let font = match data.font.map(|f| BitmapFont::read(&igz, f)) {
                Some(Ok(f)) => f,
                Some(Err(e)) => {
                    failures.push(format!("{name} font #{i}: {e}"));
                    continue;
                }
                None => {
                    failures.push(format!("{name} font data #{i}: no font"));
                    continue;
                }
            };
            let glyphs = match font
                .char_metrics_list
                .map(|l| CharMetricsList::read(&igz, l))
            {
                Some(Ok(l)) => l.glyphs,
                other => {
                    failures.push(format!("{name} font #{i}'s glyphs: {other:?}"));
                    continue;
                }
            };
            let mut read = 0usize;
            let mut kerned = 0usize;
            for g in &glyphs {
                match CharMetrics::read(&igz, *g) {
                    Ok(c) => {
                        read += 1;
                        let k = c.kern();
                        if (k.top_left, k.top_right, k.bottom_left, k.bottom_right) != (0, 0, 0, 0)
                        {
                            kerned += 1;
                        }
                    }
                    Err(e) => failures.push(format!("{name} glyph #{g}: {e}")),
                }
            }
            println!(
                "font {name}: {:?} height {} baseline {} kern {} padding {} glyphs {read} ({kerned} with a kern corner)",
                font.font.name,
                font.font.height,
                font.font.base_line,
                font.kern_amount,
                font.font.char_padding
            );
            fonts.push((name.clone(), font.font.height, read));
        }
    }

    println!("templates {templates} in {files_with} files; by area {by_area:?}");
    for ((kind, context), n) in &pairs {
        println!("  {kind:>10} on {context}: {n}");
    }
    let global_text: Vec<(u16, usize)> = point_sizes
        .iter()
        .filter(|((f, _), _)| f == "permanent/global.bld")
        .map(|((_, size), n)| (*size, *n))
        .collect();
    println!("global.bld's text templates' point sizes: {global_text:?}");
    let mut sizes: BTreeMap<u16, usize> = BTreeMap::new();
    for ((_, size), n) in &point_sizes {
        *sizes.entry(*size).or_default() += n;
    }
    println!("every text template's point size: {sizes:?}");
    println!(
        "non-empty sprite lists: {lists_nonempty}; contexts imported: {imported_contexts}; no context but not a container: {contextless:?}; imported from another namespace: {elsewhere:?}"
    );
    for f in failures.iter().take(40) {
        println!("FAIL {f}");
    }

    assert!(failures.is_empty(), "{} failures", failures.len());
    assert_eq!((templates, files_with), (12_275, 503));
    assert_eq!(lists_nonempty, 0);
    // The disc's own exceptions to "a visual type agrees with its context"
    // (AT.1, 2026-10-31): 20 templates of a drawing type with no context,
    // and 36 emitters whose context another namespace exports.
    assert_eq!(
        contextless,
        BTreeMap::from([("art", 9), ("particles", 9), ("text", 2)])
    );
    assert_eq!(elsewhere, BTreeMap::from([("particles", 36)]));
    let agreeing: usize = pairs
        .iter()
        .filter(|((k, c), _)| expected_kind(c) == Some(k.as_str()))
        .map(|(_, n)| n)
        .sum();
    assert_eq!(agreeing, 12_219);
    assert_eq!(
        global_text,
        vec![(84, 6)],
        "global.bld's six text templates at 84 points"
    );
    fonts.sort_by(|a, b| a.0.cmp(&b.0));
    assert_eq!(
        fonts
            .iter()
            .map(|(f, h, n)| (f.as_str(), *h, *n))
            .collect::<Vec<_>>(),
        vec![
            ("level/title.bld", 84.0, 175),
            ("permanent/bootstrap.bld", 42.0, 177),
            ("permanent/global.bld", 82.0, 178),
        ]
    );
    assert_eq!(fonts.iter().map(|f| f.2).sum::<usize>(), 530);
}

#[test]
#[ignore = "reads the Giants dump at GUAC_GIANTS_DUMP"]
fn every_archives_interface_reads() {
    use guac_formats::interface::{Content, Interface, TextKey};

    let (root, names) = bld_names();
    let mut templates = 0usize;
    let mut files_with = 0usize;
    let mut failures = Vec::new();
    let mut character = (0usize, 0usize);
    let mut menus = (0usize, 0usize);
    let mut global_kinds: BTreeMap<&'static str, usize> = BTreeMap::new();
    let mut imported: BTreeMap<String, usize> = BTreeMap::new();
    let mut empty: BTreeMap<String, usize> = BTreeMap::new();
    let mut keys: BTreeMap<&'static str, usize> = BTreeMap::new();
    let mut fonts = Vec::new();
    // Every template holding flags bit 21, by kind (AT.29: a drawn
    // container with it joins the frame's sprite list), and every string's
    // settings read (AT.27).
    let mut bit21: BTreeMap<String, usize> = BTreeMap::new();
    let mut strings = 0usize;
    for name in names {
        let Some(bytes) = level_bld(&root, &name) else {
            continue;
        };
        let igz = match Igz::parse(&bytes) {
            Ok(igz) => igz,
            Err(e) => {
                failures.push(format!("{name}: {e}"));
                continue;
            }
        };
        let ui = match Interface::read(&igz) {
            Ok(ui) => ui,
            Err(e) => {
                failures.push(format!("{name}: {e}"));
                continue;
            }
        };
        for f in &ui.fonts {
            fonts.push((name.clone(), f.height, f.glyphs.len()));
        }
        strings += ui.strings.len();
        for t in ui.templates.iter().filter(|t| t.flags & (1 << 21) != 0) {
            *bit21.entry(format!("{:?}", t.bits.kind())).or_default() += 1;
        }
        if ui.templates.is_empty() {
            continue;
        }
        files_with += 1;
        templates += ui.templates.len();
        for t in &ui.templates {
            let k = match t.string {
                None => "none",
                Some(TextKey::Local(_)) => "local",
                Some(TextKey::Imported(_)) => "imported",
            };
            *keys.entry(k).or_default() += 1;
        }
        let n = ui.templates.len();
        if name.starts_with("character/") {
            character.0 += 1;
            character.1 += n;
        }
        let file = name.rsplit('/').next().unwrap_or("");
        if name.starts_with("misc/") && (file.starts_with("ui_") || file == "upgradeui.bld") {
            menus.0 += 1;
            menus.1 += n;
        }
        for t in &ui.templates {
            if let Content::Empty { kind } = &t.content {
                *empty.entry(format!("{kind:?}")).or_default() += 1;
            }
            if let Content::Imported { kind, id } = &t.content {
                *imported
                    .entry(format!("{kind:?} {:08x}:{:#x}", id.namespace, id.object))
                    .or_default() += 1;
            }
            if name == "permanent/global.bld" {
                let k = match &t.content {
                    Content::Container => "container",
                    Content::Empty { .. } => "empty",
                    Content::Image(_) => "image",
                    Content::Movie(_) => "movie",
                    Content::Text { .. } => "text",
                    Content::Model { .. } => "model",
                    Content::Emitter { .. } => "emitter",
                    Content::Imported { .. } => "imported",
                };
                *global_kinds.entry(k).or_default() += 1;
            }
        }
    }
    println!("templates {templates} in {files_with} files; text keys {keys:?}");
    println!("character archives {character:?}, menu archives {menus:?}");
    println!("global.bld {global_kinds:?}");
    println!("imported contexts {imported:?}; empty by kind {empty:?}");
    println!("fonts {fonts:?}");
    println!("strings' settings read: {strings}; templates with flags bit 21 by kind: {bit21:?}");
    for f in failures.iter().take(40) {
        println!("FAIL {f}");
    }
    assert!(failures.is_empty(), "{} failures", failures.len());
    assert_eq!((templates, files_with), (12_275, 503));
    // AT.29: no template holds flags bit 21, so no container joins the
    // frame's sprite list and its quad stays unmodelled; every string's
    // settings read (AT.27).
    assert!(bit21.is_empty(), "{bit21:?}");
    assert_eq!(strings, 26_260);
    assert_eq!(character, (75, 1_970));
    assert_eq!(menus, (16, 887));
    assert_eq!(
        global_kinds,
        BTreeMap::from([
            ("container", 6),
            ("emitter", 27),
            ("image", 262),
            ("model", 49),
            ("movie", 59),
            ("text", 6),
        ])
    );
    fonts.sort_by(|a, b| a.0.cmp(&b.0));
    assert_eq!(
        fonts
            .iter()
            .map(|(f, h, n)| (f.as_str(), *h, *n))
            .collect::<Vec<_>>(),
        vec![
            ("level/title.bld", 84.0, 175),
            ("permanent/bootstrap.bld", 42.0, 177),
            ("permanent/global.bld", 82.0, 178),
        ]
    );
}
