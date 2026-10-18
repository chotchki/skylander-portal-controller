//! IGZ v6 off the Giants dump (openspec AC.5): every IGZ inside the gate
//! hero's archives (`character/009_eruptor.*`), then every character
//! `level.bld` and `permanent/global.bld`, parsed and fully resolved, with the
//! fixup-kind census printed.
//! Ignored by default; it reads `GUAC_GIANTS_DUMP` like `giants_iga`:
//!
//! `cargo test -p guac-formats --release --test giants_igz -- --ignored --nocapture`

mod common;

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use common::usrdir;
use guac_formats::iga::Archive;
use guac_formats::igz::{self, Igz, Location, Marker, Ref};

#[derive(Default)]
struct Census {
    files: u64,
    failures: Vec<String>,
    counts: BTreeMap<String, u64>,
}

impl Census {
    fn bump(&mut self, key: impl Into<String>, by: u64) {
        *self.counts.entry(key.into()).or_default() += by;
    }

    /// Parses one IGZ and resolves every marker, every object's refs and
    /// every memory ref a pointer marker ends, counting what it finds.
    fn take(&mut self, name: &str, bytes: &[u8]) {
        let file = match Igz::parse(bytes) {
            Ok(file) => file,
            Err(e) => {
                self.failures.push(format!("{name}: {e}"));
                return;
            }
        };
        self.files += 1;
        for rec in file.records() {
            let tag = if rec.is_known() { "" } else { " (unknown)" };
            self.bump(format!("kind {:#04x}{tag}: files", rec.kind), 1);
            self.bump(
                format!("kind {:#04x}{tag}: entries", rec.kind),
                rec.count.into(),
            );
        }
        for rec in file.unknown_records() {
            // Is it one big-endian u32, the first object's location?
            let first = file.objects().first().map(|o| o.at.0.to_be_bytes());
            let same = rec.count == 1 && first.is_some_and(|f| rec.payload == f);
            self.bump(
                format!(
                    "kind {:#04x} is the first object's location as a u32: {same}",
                    rec.kind
                ),
                1,
            );
        }
        self.bump("objects", file.objects().len() as u64);
        self.bump("data sections", file.data_sections().len() as u64);
        let placeholders = file
            .data_sections()
            .iter()
            .filter(|s| s.is_placeholder())
            .count();
        self.bump("placeholder sections", placeholders as u64);

        for &(at, marker) in file.markers() {
            match file.reference(at, "census") {
                Ok(r) => self.bump(format!("ref {}", variant(&r)), 1),
                Err(e) => self
                    .failures
                    .push(format!("{name}: {marker:?} at {at:?}: {e}")),
            }
            if marker == Marker::NullMemoryRef {
                let word = file.u32_at(at, "census").unwrap_or(u32::MAX);
                self.bump(format!("null memory ref size word {word:#x}"), 1);
                if let Err(e) = file.memory_ref(at, "census") {
                    self.failures
                        .push(format!("{name}: null memory ref at {at:?}: {e}"));
                }
            }
            if marker == Marker::Pointer
                && let Some(size_at) = at.0.checked_sub(4).map(Location)
                && file.marker_at(size_at).is_none()
                && file
                    .u32_at(size_at, "census")
                    .is_ok_and(|w| w & igz::MEMORY_PRESENT != 0)
            {
                // Without the layouts this can't tell a memory ref from a
                // field that merely has bit 31 set, so it only counts.
                match file.memory_ref(size_at, "census") {
                    Ok(Some(_)) => {
                        self.bump("pointers after a bit-31 word: read as a memory ref", 1)
                    }
                    Ok(None) => self.failures.push(format!("{name}: {size_at:?} read null")),
                    Err(_) => self.bump("pointers after a bit-31 word: not a memory ref", 1),
                }
            }
        }
        for object in file.objects() {
            for (offset, resolved) in file.refs_of(object) {
                if let Err(e) = resolved {
                    self.failures.push(format!(
                        "{name}: {} +{offset:#x}: {e}",
                        file.type_name(object)
                    ));
                }
            }
        }
        for id in file.external_ids() {
            if id.namespace == guac_formats::fnv1a32(b"metaimages") {
                self.bump("EXID in metaimages", 1);
            }
        }
    }

    fn print(&self, title: &str) {
        println!(
            "{title}: {} IGZ parsed, {} failures",
            self.files,
            self.failures.len()
        );
        for (key, value) in &self.counts {
            println!("  {key}: {value}");
        }
    }
}

fn variant(r: &Ref<'_>) -> &'static str {
    match r {
        Ref::Null => "null",
        Ref::Object(_) => "object",
        Ref::Data(_) => "data",
        Ref::Import { .. } => "import",
        Ref::Handle { .. } => "handle",
        Ref::String(_) => "string",
        Ref::NullMemoryRef => "null memory ref",
        Ref::MetaObject { .. } => "metaobject",
        Ref::UnresolvedMeta(_) => "metaobject, unflagged (unresolved)",
    }
}

/// Every IGZ entry of an archive whose name `pick` accepts; the rest are counted.
fn census_archive(census: &mut Census, path: &Path, pick: impl Fn(&str) -> bool) {
    let bytes = std::fs::read(path).unwrap();
    let archive = Archive::parse(&bytes).unwrap();
    let stem = path.file_name().unwrap().to_string_lossy().into_owned();
    for entry in archive.entries() {
        if !pick(entry.name()) {
            continue;
        }
        let data = archive.read(entry).unwrap();
        if !data.starts_with(igz::MAGIC) {
            census.bump("entries that aren't IGZ", 1);
            continue;
        }
        census.take(&format!("{stem}:{}", entry.name()), &data);
    }
}

fn character_archives(usrdir: &Path, ext: &str) -> Vec<PathBuf> {
    let mut paths: Vec<PathBuf> = std::fs::read_dir(usrdir.join("character"))
        .unwrap()
        .map(|e| e.unwrap().path())
        .filter(|p| p.extension().is_some_and(|e| e == ext))
        .collect();
    paths.sort();
    paths
}

#[test]
#[ignore = "reads the Giants dump at GUAC_GIANTS_DUMP"]
fn every_igz_in_the_hero_and_character_archives_parses() {
    let usrdir = usrdir();

    let mut hero = Census::default();
    for ext in ["bld", "arc"] {
        let path = usrdir.join(format!("character/009_eruptor.{ext}"));
        census_archive(&mut hero, &path, |_| true);
    }
    hero.print("Eruptor's archives (009_eruptor.bld, .arc)");

    let mut levels = Census::default();
    for path in character_archives(&usrdir, "bld") {
        census_archive(&mut levels, &path, |name| name.ends_with("level.bld"));
    }
    levels.print("every character level.bld");

    // Where AC.7 found null memory refs with size word 0x7.
    let mut global = Census::default();
    census_archive(&mut global, &usrdir.join("permanent/global.bld"), |_| true);
    global.print("permanent/global.bld");

    assert!(
        hero.files > 0 && levels.files > 0,
        "no IGZ found under {}",
        usrdir.display()
    );
    let failures: Vec<&String> = [&hero, &levels, &global]
        .iter()
        .flat_map(|c| &c.failures)
        .collect();
    assert!(failures.is_empty(), "{failures:#?}");
}
