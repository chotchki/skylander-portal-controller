//! The intro index on the real Giants dump (openspec AB.6). Ignored by
//! default: it reads `GUAC_GIANTS_DUMP` and writes the character table into
//! a throwaway identity dir under `GUAC_EXTRACT_DIR`, from the environment or
//! the workspace's `.env.dev`:
//!
//! `cargo test -p guac-extract --release --test giants_intros -- --ignored --nocapture`

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::time::Instant;

use guac_extract::intros::{self, IrFormat, slug};
use guac_extract::known::KNOWN_DUMPS;
use guac_extract::{ExtractRoot, open};
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
    let identity = opened.dir.manifest().identity.clone();

    let data_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../data");
    let catalogue = guac_server::native_catalogue::load(&data_root).unwrap();
    let names = guac_server::native_catalogue::base_names(&catalogue);

    let started = Instant::now();
    let idx = intros::index(&dump, identity, &names).unwrap();
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
            let agrees = slug(n) == movie;
            format!("{n}{}", if agrees { "" } else { " (differs)" })
        });
        println!(
            "{toy:3}  {:42}  {:15}  {movie:15}  {source:12}  {cat}",
            stems.join(" "),
            c.vo_name.as_deref().unwrap_or("-"),
        );
    }

    let disc = idx.count(IntroSource::Disc);
    let fallback = idx.count(IntroSource::NameFallback);
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
    println!("unclaimed movies: {:?}", idx.unclaimed_movies);

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
