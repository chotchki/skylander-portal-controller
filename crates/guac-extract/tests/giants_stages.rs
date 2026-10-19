//! The roster stage on the real Giants dump (openspec AE.3): the roster's IR
//! and its 123 portraits written into a throwaway identity dir, again as a
//! resume, and the intro index on the roster's names. Ignored by default: it
//! reads `GUAC_GIANTS_DUMP` and writes under `GUAC_EXTRACT_DIR`, from the
//! environment or the workspace's `.env.dev`:
//!
//! `cargo test -p guac-extract --release --test giants_stages -- --ignored --nocapture`

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::time::Instant;

use guac_extract::intros::{self, IrFormat};
use guac_extract::known::KNOWN_DUMPS;
use guac_extract::stages::{self, Files};
use guac_extract::{DirState, ExtractRoot, open, roster};
use guac_ir::IntroSource;

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
#[ignore = "reads the Giants dump at GUAC_GIANTS_DUMP, writes under GUAC_EXTRACT_DIR"]
fn the_roster_stage_writes_123_figures_and_their_portraits() {
    let dump = PathBuf::from(env_or_dotenv("GUAC_GIANTS_DUMP").expect("set GUAC_GIANTS_DUMP"));
    let base = PathBuf::from(env_or_dotenv("GUAC_EXTRACT_DIR").expect("set GUAC_EXTRACT_DIR"));
    std::fs::create_dir_all(&base).unwrap();
    let tmp = tempfile::Builder::new()
        .prefix("ae3-stages-")
        .tempdir_in(&base)
        .unwrap();
    let root = ExtractRoot::checked(tmp.path().join("extract")).unwrap();
    let started = Instant::now();
    let mut opened = open(&dump, &root, KNOWN_DUMPS, &|_| {}).expect("the known dump");
    assert_eq!(opened.state, DirState::Fresh);
    println!("identity in {:.2} s", started.elapsed().as_secs_f64());

    let stage = stages::roster_stage(&mut opened.dir, IrFormat::Ron, &|_| {}).unwrap();
    let r = &stage.roster;
    println!(
        "roster stage: {} figures, {} portraits, {:?} in {:.2} s",
        r.figures.len(),
        stage.portraits,
        stage.files,
        stage.took.as_secs_f64()
    );
    assert_eq!(r.figures.len(), 123);
    assert_eq!(stage.portraits, 123);
    assert_eq!((stage.files.written, stage.files.kept), (124, 0));

    // Every figure's portrait: a 256x256 RGBA PNG, all different.
    let dir = opened.dir.path().to_path_buf();
    let mut sizes = Vec::new();
    let mut distinct = BTreeSet::new();
    let mut opaque_share = Vec::new();
    for f in &r.figures {
        let rel = f.portrait.as_deref().expect("a portrait");
        assert_eq!(rel, format!("portraits/{}.png", f.id()));
        assert!(opened.dir.output_done(rel).unwrap(), "{rel}");
        let bytes = std::fs::read(dir.join(rel)).unwrap();
        let mut reader = png::Decoder::new(std::io::Cursor::new(&bytes))
            .read_info()
            .unwrap();
        let mut rgba = vec![0; reader.output_buffer_size().unwrap()];
        let info = reader.next_frame(&mut rgba).unwrap();
        assert_eq!(
            (info.width, info.height, info.color_type, info.bit_depth),
            (256, 256, png::ColorType::Rgba, png::BitDepth::Eight),
            "{rel}"
        );
        let opaque = rgba
            .as_chunks::<4>()
            .0
            .iter()
            .filter(|p| p[3] == 255)
            .count();
        opaque_share.push(opaque as f64 / (256.0 * 256.0));
        distinct.insert(rgba);
        sizes.push(bytes.len());
    }
    assert_eq!(distinct.len(), 123, "every portrait is its own image");
    let ir = std::fs::metadata(dir.join(IrFormat::Ron.roster_path()))
        .unwrap()
        .len();
    let (min, max) = (sizes.iter().min().unwrap(), sizes.iter().max().unwrap());
    let total: usize = sizes.iter().sum();
    let (lo, hi) = opaque_share
        .iter()
        .fold((1f64, 0f64), |(lo, hi), &s| (lo.min(s), hi.max(s)));
    println!(
        "ir/roster.ron {ir} bytes; portraits {total} bytes ({min}-{max} each); \
         opaque share {lo:.3}-{hi:.3}"
    );

    // A resume keeps every file; the install format writes only its IR.
    let stage = stages::roster_stage(&mut opened.dir, IrFormat::Ron, &|_| {}).unwrap();
    println!(
        "resumed: {:?} in {:.2} s",
        stage.files,
        stage.took.as_secs_f64()
    );
    assert_eq!((stage.files.written, stage.files.kept), (0, 124));
    let stage = stages::roster_stage(&mut opened.dir, IrFormat::Postcard, &|_| {}).unwrap();
    let postcard = std::fs::metadata(dir.join(IrFormat::Postcard.roster_path()))
        .unwrap()
        .len();
    println!(
        "ir/roster.postcard {postcard} bytes; {:?} in {:.2} s",
        stage.files,
        stage.took.as_secs_f64()
    );
    assert_eq!(
        stage.files,
        Files {
            written: 1,
            kept: 123,
            bytes: stage.files.bytes
        }
    );

    // The intro index on the roster's names maps AB.6's 48 and its five
    // variants, with no catalogue anywhere.
    let started = Instant::now();
    let idx = intros::index(&opened.dir, &roster::base_names(r)).unwrap();
    let chars = &idx.table.characters;
    let by_disc = idx.count(|s| s == IntroSource::Disc);
    let by_name = idx.count(|s| s == IntroSource::NameFallback);
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
    println!(
        "intro index: {by_disc} by disc, {by_name} by name, variants {variants:?}, {:.2} s",
        started.elapsed().as_secs_f64()
    );
    assert_eq!((by_disc, by_name), (48, 0));
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
    assert!(chars.values().all(|c| c.intro.is_some()));
    assert!(idx.claimed_twice.is_empty());
}
