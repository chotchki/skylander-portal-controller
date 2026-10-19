//! The roster on the real Giants dump (openspec AE.2): IR v2's 123 figures
//! read off the disc, checked against the toy table's counts and sets and
//! against AA's interim catalogue. Ignored by default: it reads
//! `GUAC_GIANTS_DUMP` from the environment or the workspace's `.env.dev`,
//! and AA's catalogue from the workspace's `data/` (until AE.4 retires it):
//!
//! `cargo test -p guac-extract --release --test giants_roster -- --ignored --nocapture`

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::time::Instant;

use guac_extract::known::KNOWN_DUMPS;
use guac_extract::roster;
use guac_ir::roster::ENGLISH;
use guac_ir::{Category, DumpIdentity, Element, FigureKind, Sha256};

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

/// AA's ids the disc names otherwise (design decision 2): toy type, AA's
/// variant, the disc's.
const CHANGED: [(u32, u16, u16); 12] = [
    (0, 0x0802, 0x0402),
    (18, 0x0802, 0x0402),
    (10, 0x0803, 0x0403),
    (15, 0x0803, 0x0403),
    (26, 0x0803, 0x0403),
    (102, 0x0202, 0x0402),
    (109, 0x0202, 0x0402),
    (112, 0x0202, 0x0402),
    (105, 0x0002, 0x0402),
    (108, 0x0002, 0x0402),
    (106, 0x0203, 0x0603),
    (110, 0x0203, 0x0403),
];

#[test]
#[ignore = "reads the Giants dump at GUAC_GIANTS_DUMP"]
fn the_disc_roster_is_123_figures_with_aas_77_among_them() {
    let dump = PathBuf::from(env_or_dotenv("GUAC_GIANTS_DUMP").expect("set GUAC_GIANTS_DUMP"));
    let known = KNOWN_DUMPS[0];
    let identity = DumpIdentity {
        title_id: known.title_id.into(),
        app_ver: known.app_ver.into(),
        manifest_hash: Sha256::from_hex(known.manifest_hash).unwrap(),
    };
    let started = Instant::now();
    let r = roster::read(&dump, identity).unwrap();
    let took = started.elapsed();
    let en = |f| r.display_name(f, ENGLISH).unwrap();

    // A base by its category, a variant by its kind: the box's groups.
    let mut tally: BTreeMap<String, usize> = BTreeMap::new();
    for f in &r.figures {
        let key = match f.kind {
            FigureKind::Base => format!("{:?}", f.category),
            k => format!("{k:?}"),
        };
        *tally.entry(key).or_default() += 1;
    }
    println!(
        "{} figures in {:.0} ms (4 archives read and decompressed): {tally:?}",
        r.figures.len(),
        took.as_secs_f64() * 1e3
    );
    assert_eq!(r.figures.len(), 123);
    assert_eq!(
        tally,
        BTreeMap::from([
            ("Figure".to_owned(), 45),
            ("Giant".to_owned(), 8),
            ("Alt".to_owned(), 15),
            ("Series2".to_owned(), 24),
            ("LightCore".to_owned(), 8),
            ("Item".to_owned(), 10),
            ("AdventurePack".to_owned(), 5),
            ("Sidekick".to_owned(), 8),
        ])
    );
    let toys_of = |k: FigureKind| -> Vec<u32> {
        r.figures
            .iter()
            .filter(|f| f.kind == k)
            .map(|f| f.toy_type)
            .collect()
    };
    assert_eq!(
        toys_of(FigureKind::LightCore),
        [7, 9, 20, 29, 100, 106, 108, 113]
    );
    let mut series_2 = toys_of(FigureKind::Series2);
    series_2.sort_unstable();
    assert_eq!(
        series_2,
        [
            0, 1, 3, 4, 5, 7, 9, 10, 11, 12, 14, 15, 16, 18, 19, 20, 21, 23, 25, 26, 27, 29, 30, 32
        ]
    );
    let ids: BTreeSet<String> = r.figures.iter().map(|f| f.id()).collect();
    assert_eq!(ids.len(), 123, "ids are unique");
    let portraits: BTreeSet<&str> = r
        .figures
        .iter()
        .filter_map(|f| f.portrait.as_deref())
        .collect();
    assert_eq!(portraits.len(), 123, "every figure its own portrait");

    // The 12 languages, every name and word in each.
    let languages: Vec<i32> = r.languages.iter().map(|l| l.id).collect();
    assert_eq!(languages, [8, 21, 19, 22, 7, 5, 17, 4, 6, 9, 0, 14]);
    let mut translated = BTreeSet::new();
    for f in &r.figures {
        for text in std::iter::once(&f.name).chain(&f.variant_word) {
            assert_eq!(text.texts.len(), 12, "{}", text.key);
            assert!(text.texts.iter().all(|t| !t.is_empty()), "{}", text.key);
            if text.texts.iter().any(|t| *t != text.texts[0]) {
                translated.insert(text.key.clone());
            }
        }
    }
    let names: BTreeSet<&str> = r
        .figures
        .iter()
        .flat_map(|f| std::iter::once(&f.name).chain(&f.variant_word))
        .map(|t| t.key.as_str())
        .collect();
    println!(
        "{} distinct name and word keys, {} translated: {translated:?}",
        names.len(),
        translated.len()
    );

    // AA.14's four, off the disc.
    for (toy, name, element) in [
        (0x06, "Dino-Rang", Element::Earth),
        (0x0D, "Wham-Shell", Element::Water),
        (0x15, "Drill Sergeant", Element::Tech),
        (0x64, "Jet-Vac", Element::Air),
    ] {
        let f = r.figure(&format!("{toy:06x}-0000")).unwrap();
        assert_eq!((en(f).as_str(), f.element), (name, Some(element)));
        assert!(f.portrait.is_some());
    }

    // AA's interim catalogue: every id on the disc but the 12 decision 2
    // names, each of those under the disc's id with the same name.
    let data_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../data");
    let aa = guac_server::native_catalogue::load(&data_root).unwrap();
    assert_eq!(aa.len(), 77);
    let mut agree = 0;
    let mut moved = Vec::new();
    let mut disagree = Vec::new();
    for row in &aa {
        let id = row.id.as_str();
        match r.figure(id) {
            Some(f) => {
                let same = en(f) == row.canonical_name
                    && format!("{:?}", f.element) == format!("{:?}", row.element)
                    && format!("{:?}", f.category) == format!("{:?}", row.category);
                if same {
                    agree += 1;
                } else {
                    disagree.push(format!(
                        "{id}: disc {} {:?} {:?}, AA {} {:?} {:?}",
                        en(f),
                        f.element,
                        f.category,
                        row.canonical_name,
                        row.element,
                        row.category
                    ));
                }
            }
            None => {
                let (toy, variant) = row.id.tag_parts().unwrap();
                let (_, _, new) = CHANGED
                    .iter()
                    .find(|(t, old, _)| *t == toy.0 && *old == variant.0)
                    .unwrap_or_else(|| panic!("{id} ({}) isn't on the disc", row.canonical_name));
                let f = r.figure(&format!("{:06x}-{new:04x}", toy.0)).unwrap();
                assert_eq!(en(f), row.canonical_name, "{id}");
                moved.push(format!("{id} -> {} {}", f.id(), row.canonical_name));
            }
        }
    }
    println!(
        "AA's 77: {agree} agree on name, element and category; {} moved:",
        moved.len()
    );
    for m in &moved {
        println!("  {m}");
    }
    println!("disagree: {disagree:?}");
    assert_eq!(moved.len(), 12);
    assert!(disagree.is_empty());
    assert_eq!(agree, 65);
    let only_disc = r.figures.len() - agree - moved.len();
    println!("on the disc, not in AA: {only_disc}");
    assert_eq!(only_disc, 46);
    assert!(
        r.figures
            .iter()
            .all(|f| f.category != Category::Figure || f.element.is_some())
    );
}
