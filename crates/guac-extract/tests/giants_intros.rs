//! The intro index on the real Giants dump (openspec AB.6). Ignored by
//! default: it reads `GUAC_GIANTS_DUMP` and writes the character table into
//! a throwaway identity dir under `GUAC_EXTRACT_DIR`, from the environment or
//! the workspace's `.env.dev`:
//!
//! `cargo test -p guac-extract --release --test giants_intros -- --ignored --nocapture`

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::time::Instant;

use guac_extract::intros::{self, IrFormat, MAX_ENTRY_NAME, MAX_VO_STEM, slug};
use guac_extract::known::KNOWN_DUMPS;
use guac_extract::{ExtractRoot, open};
use guac_formats::iga::Archive;
use guac_ir::{CharacterTable, IntroSource};

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

/// FMVs and logos: every movie on the disc that isn't a character's intro.
fn not_an_intro(movie: &str) -> bool {
    let fmv = movie
        .strip_prefix("fmv")
        .is_some_and(|n| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()));
    fmv || movie.contains("logo")
}

#[test]
#[ignore = "reads the Giants dump at GUAC_GIANTS_DUMP, writes under GUAC_EXTRACT_DIR"]
fn every_giants_intro_maps_to_one_toy_type() {
    let dump = PathBuf::from(env_or_dotenv("GUAC_GIANTS_DUMP").expect("set GUAC_GIANTS_DUMP"));
    let base = PathBuf::from(env_or_dotenv("GUAC_EXTRACT_DIR").expect("set GUAC_EXTRACT_DIR"));
    std::fs::create_dir_all(&base).unwrap();
    let tmp = tempfile::Builder::new()
        .prefix("ab6-intros-")
        .tempdir_in(&base)
        .unwrap();
    let root = ExtractRoot::checked(tmp.path().join("extract")).unwrap();
    let mut opened = open(&dump, &root, KNOWN_DUMPS, &|_| {}).expect("the known dump");

    let data_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../data");
    let catalogue = guac_server::native_catalogue::load(&data_root).unwrap();
    let names = guac_server::native_catalogue::base_names(&catalogue);

    let started = Instant::now();
    let idx = intros::index(&opened.dir, &names).unwrap();
    let took = started.elapsed().as_secs_f64();
    let chars = &idx.table.characters;

    println!(
        "toy  archives                                    vo_name          intro            source        catalogue"
    );
    for (toy, c) in chars {
        let stems: Vec<String> = c
            .archives
            .iter()
            .map(|a| match &a.variant {
                Some(v) => format!("{}[{v}]", a.stem),
                None => a.stem.clone(),
            })
            .collect();
        let (movie, source) = c.intro.as_ref().map_or(("-", "-".to_owned()), |i| {
            (&i.movie, format!("{:?}", i.source))
        });
        let cat = names.get(toy).map_or("(none)".to_owned(), |n| {
            let variant = c
                .intro
                .as_ref()
                .is_some_and(|i| matches!(i.source, IntroSource::Variant { .. }));
            let agrees = variant || slug(n) == movie;
            format!("{n}{}", if agrees { "" } else { " (differs)" })
        });
        println!(
            "{toy:3}  {:42}  {:15}  {movie:15}  {source:12}  {cat}",
            stems.join(" "),
            c.vo_name.as_deref().unwrap_or("-"),
        );
    }

    let disc = idx.count(|s| s == IntroSource::Disc);
    let fallback = idx.count(|s| s == IntroSource::NameFallback);
    let variants: BTreeMap<u32, (u32, &str)> = chars
        .iter()
        .filter_map(|(t, c)| {
            let i = c.intro.as_ref()?;
            match i.source {
                IntroSource::Variant { base } => Some((*t, (base, i.movie.as_str()))),
                _ => None,
            }
        })
        .collect();
    let fallback_toys: Vec<u32> = chars
        .iter()
        .filter(|(_, c)| {
            c.intro
                .as_ref()
                .is_some_and(|i| i.source == IntroSource::NameFallback)
        })
        .map(|(t, _)| *t)
        .collect();
    let no_intro: Vec<u32> = chars
        .iter()
        .filter(|(_, c)| c.intro.is_none())
        .map(|(t, _)| *t)
        .collect();
    println!(
        "split: {disc} by disc, {fallback} by name fallback {fallback_toys:?}; \
         toy types with an archive but no intro {no_intro:?}; {took:.2} s"
    );
    println!("variants sharing a base's intro (toy: base, movie): {variants:?}");
    println!("unclaimed movies: {:?}", idx.unclaimed_movies);

    // The five the catalogue gives a toy type of their own.
    assert_eq!(
        variants,
        BTreeMap::from([
            (28, (16, "spyro")),
            (404, (4, "bash")),
            (416, (16, "spyro")),
            (419, (19, "triggerhappy")),
            (430, (30, "chopchop")),
        ])
    );
    assert!(no_intro.is_empty(), "{no_intro:?}");

    assert!(idx.claimed_twice.is_empty(), "{:?}", idx.claimed_twice);
    let intros: Vec<&String> = idx
        .unclaimed_movies
        .iter()
        .filter(|m| !not_an_intro(m))
        .collect();
    assert!(intros.is_empty(), "intros no toy type claimed: {intros:?}");
    assert_eq!(disc + fallback, 48);
    assert_eq!(idx.unclaimed_movies.len(), 61 - 48);

    // The roster cross-check: both differences.
    let archived: BTreeSet<u32> = chars
        .iter()
        .filter(|(_, c)| !c.archives.is_empty())
        .map(|(t, _)| *t)
        .collect();
    let catalogued: BTreeSet<u32> = names.keys().copied().collect();
    let missing: Vec<u32> = archived.difference(&catalogued).copied().collect();
    let only_catalogue: BTreeMap<u32, String> = catalogued
        .difference(&archived)
        .map(|t| {
            let f = catalogue
                .iter()
                .find(|f| f.tag_identity.is_some_and(|i| i.toy_type.0 == *t))
                .unwrap();
            (*t, format!("{} ({:?})", names[t], f.category))
        })
        .collect();
    println!("archive toy types the catalogue lacks: {missing:?}");
    println!(
        "catalogue toy types with no archive ({}): {only_catalogue:?}",
        only_catalogue.len()
    );
    assert!(
        missing.is_empty(),
        "add these to native_catalogue's SUPPLEMENT: {missing:?}"
    );

    // The caps drop nothing real: the longest voice-over entry name and stem.
    let char_dir = dump.join(intros::CHARACTER_DIR);
    let (mut longest_name, mut longest_stem, mut vo_entries) = (0, 0, 0usize);
    for e in std::fs::read_dir(&char_dir).unwrap() {
        let name = e.unwrap().file_name().into_string().unwrap();
        if intros::parse_archive_file(&name).is_none() {
            continue;
        }
        let bytes = std::fs::read(char_dir.join(&name)).unwrap();
        for entry in Archive::parse(&bytes).unwrap().entries() {
            let n = entry.name();
            let Some(stem) = n.split(['/', '\\']).find_map(|s| {
                s.to_ascii_lowercase()
                    .strip_prefix("vo_")?
                    .strip_suffix(".wav")
                    .map(str::len)
            }) else {
                continue;
            };
            vo_entries += 1;
            longest_name = longest_name.max(n.len());
            longest_stem = longest_stem.max(stem);
        }
    }
    println!(
        "{vo_entries} voice-over entries; longest name {longest_name} bytes (cap {MAX_ENTRY_NAME}), \
         longest stem {longest_stem} (cap {MAX_VO_STEM})"
    );
    assert!(longest_name <= MAX_ENTRY_NAME && longest_stem <= MAX_VO_STEM);

    // What guacamole will load: recorded in the manifest, same table back.
    for format in [IrFormat::Ron, IrFormat::Postcard] {
        let out = intros::write(&mut opened.dir, &idx.table, format).unwrap();
        assert!(opened.dir.output_done(&out.path).unwrap());
        let bytes = std::fs::read(opened.dir.path().join(&out.path)).unwrap();
        let back: CharacterTable = match format {
            IrFormat::Ron => guac_ir::from_ron(std::str::from_utf8(&bytes).unwrap()).unwrap(),
            IrFormat::Postcard => guac_ir::from_postcard(&bytes).unwrap(),
        };
        assert_eq!(back, idx.table);
        println!("{} {} bytes", out.path, out.size);
    }
}
