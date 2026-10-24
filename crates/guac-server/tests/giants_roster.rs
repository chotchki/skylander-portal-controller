//! The roster on the real Giants dump (openspec AE.2, AE.4): IR v2's 123
//! figures read off the disc, checked against the toy table's counts and
//! sets, the server's figure list built from them and the 12 ids AA's interim
//! catalogue gave otherwise. Ignored by default: it reads `GUAC_GIANTS_DUMP`
//! from the environment or the workspace's `.env.dev`:
//!
//! `cargo test -p guac-server --release --test giants_roster -- --ignored --nocapture`

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::time::Instant;

use guac_extract::known::KNOWN_DUMPS;
use guac_extract::roster;
use guac_ir::roster::ENGLISH;
use guac_ir::{Category, DumpIdentity, Element, FigureKind, Sha256};
use guac_server::figures::FigureSet;
use skylander_core::{FigureId, GameOfOrigin};

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

#[test]
#[ignore = "reads the Giants dump at GUAC_GIANTS_DUMP"]
fn the_disc_roster_is_123_figures_and_the_servers_list() {
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

    // AE.4's id migration: each old id is off the disc, its new one on it,
    // under the name AA showed (decision 2).
    let names = [
        "Polar Whirlwind",
        "Royal Double Trouble",
        "Legendary Ignitor",
        "Legendary Slam Bam",
        "Legendary Stealth Elf",
        "Granite Crusher",
        "Scarlet Ninjini",
        "Gnarly Tree Rex",
        "Molten Hot Dog",
        "Punch Pop Fizz",
        "Legendary Chill",
        "Legendary Bouncer",
    ];
    for ((old, new), name) in guac_server::figures::AA_IDS_ON_THE_DISC.iter().zip(names) {
        assert!(r.figure(old).is_none(), "{old} is a disc id");
        let f = r
            .figure(new)
            .unwrap_or_else(|| panic!("{new} isn't on the disc"));
        assert_eq!(en(f), name, "{new}");
        assert_eq!(old[..6], new[..6], "{name} keeps its toy type");
    }

    // The server's list (AE.4): every figure, a sky1 toy's base Spyro's
    // Adventure's and the rest Giants'.
    let set = FigureSet::from_roster(&r, Path::new("/extract"));
    assert_eq!(set.figures().len(), 123);
    let by_game = |g: GameOfOrigin| set.figures().iter().filter(|f| f.game == g).count();
    let (sa, giants) = (
        by_game(GameOfOrigin::SpyrosAdventure),
        by_game(GameOfOrigin::Giants),
    );
    println!("server list: {sa} Spyro's Adventure, {giants} Giants");
    assert_eq!((sa, giants), (54, 69));
    let fig = |id: &str| set.get(&FigureId::new(id)).unwrap();
    for (id, name, group, tag, game) in [
        (
            "000010-0000",
            "Spyro",
            "Spyro",
            "base",
            GameOfOrigin::SpyrosAdventure,
        ),
        (
            "00001c-0000",
            "Dark Spyro",
            "Spyro",
            "Dark",
            GameOfOrigin::SpyrosAdventure,
        ),
        (
            "000000-0800",
            "Series 2 Whirlwind",
            "Whirlwind",
            "Series 2",
            GameOfOrigin::Giants,
        ),
        (
            "00001d-0200",
            "LightCore Hex",
            "Hex",
            "LightCore",
            GameOfOrigin::Giants,
        ),
        (
            "00006a-0603",
            "Legendary Chill",
            "Chill",
            "Legendary",
            GameOfOrigin::Giants,
        ),
        (
            "000130-0000",
            "Volcanic Vault",
            "Volcanic Vault",
            "base",
            GameOfOrigin::SpyrosAdventure,
        ),
    ] {
        let f = fig(id);
        assert_eq!(
            (
                f.canonical_name.as_str(),
                f.variant_group.as_str(),
                f.variant_tag.as_str(),
                f.game
            ),
            (name, group, tag, game),
            "{id}"
        );
        assert!(set.portrait(&f.id).is_some(), "{id}");
    }
    assert!(
        r.figures
            .iter()
            .all(|f| f.category != Category::Figure || f.element.is_some())
    );
}
