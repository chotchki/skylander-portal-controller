//! The census (AB.5) on a synthetic dump built in a tempdir: every file and
//! IGA entry gets a kind, unknowns get an entropy, the report lands in the
//! extract root and the dump is left as it was.

use std::fs;
use std::path::Path;

use guac_extract::census::{Class, HIGH_ENTROPY, MAX_WHOLE_ARCHIVE, census};
use guac_extract::magic::GAME_MAGICS;
use guac_extract::{ExtractError, ExtractRoot};
use guac_formats::iga::builder::IgaBuilder;
use guac_formats::sfo::builder::SfoBuilder;

fn noise(len: usize) -> Vec<u8> {
    let mut x = 0x2545_F491_4F6C_DD1Du64;
    (0..len)
        .map(|_| {
            x ^= x << 13;
            x ^= x >> 7;
            x ^= x << 17;
            (x >> 24) as u8
        })
        .collect()
}

fn put(dump: &Path, rel: &str, bytes: &[u8]) {
    let p = dump.join(rel);
    fs::create_dir_all(p.parent().unwrap()).unwrap();
    fs::write(p, bytes).unwrap();
}

fn magic(what: &str) -> &'static [u8] {
    GAME_MAGICS
        .iter()
        .find(|(_, w)| w.contains(what))
        .unwrap()
        .0
}

fn starting_with(m: &[u8]) -> Vec<u8> {
    let mut b = m.to_vec();
    b.resize(256, 0);
    b
}

#[test]
fn a_synthetic_dump_gets_every_file_and_entry_classified() {
    let tmp = tempfile::tempdir().unwrap();
    let dump = tmp.path().join("dump");
    put(
        &dump,
        "PS3_GAME/PARAM.SFO",
        &SfoBuilder::disc_game("BLUS30968", "01.00", "01.00").build(),
    );
    put(
        &dump,
        "PS3_GAME/ICON0.PNG",
        &starting_with(b"\x89PNG\r\n\x1a\n"),
    );
    put(&dump, "PS3_GAME/USRDIR/EBOOT.BIN", &starting_with(b"SCE\0"));
    put(&dump, "PS3_GAME/USRDIR/alchemy.xml", b"<root>\n</root>\n");
    put(
        &dump,
        "PS3_GAME/USRDIR/movies/a.mov",
        &starting_with(b"BIKi"),
    );
    put(&dump, "PS3_GAME/USRDIR/mystery.bin", &noise(1 << 16));
    put(&dump, "PS3_GAME/USRDIR/zeros.bin", &[0u8; 1000]);
    let iga = IgaBuilder::new()
        .stored("a.igz", &starting_with(magic("IGZ object file")))
        .lzma("b.raw", &noise(40_000))
        .stored("c.txt", b"plain words\n")
        .build();
    put(&dump, "PS3_GAME/USRDIR/character/001_a.arc", &iga);
    put(
        &dump,
        "PS3_GAME/USRDIR/big.pak",
        &starting_with(b"\x1aAGI\0\0\0\x0b"),
    );
    // Trap Team's: little-endian, version 0x0B.
    put(
        &dump,
        "PS3_GAME/USRDIR/tt.arc",
        &starting_with(b"IGA\x1a\x0b\0\0\0"),
    );

    let before: Vec<_> = walk(&dump);
    let root = ExtractRoot::checked(tmp.path().join("extract")).unwrap();
    // No ffprobe: the Bink is counted but unprobed.
    let (c, report) = census(&dump, &root, None).unwrap();
    assert_eq!(walk(&dump), before, "the dump is untouched");

    assert_eq!(report.parent().unwrap(), root.path());
    let name = report.file_name().unwrap().to_str().unwrap();
    assert!(name.starts_with("census-BLUS30968-01.00-") && name.ends_with(".json"));
    let json: serde_json::Value = serde_json::from_slice(&fs::read(&report).unwrap()).unwrap();
    assert_eq!(json["summary"]["files"], 10);

    let class = |p: &str| c.files.iter().find(|f| f.path == p).unwrap();
    assert_eq!(class("PS3_GAME/USRDIR/EBOOT.BIN").class, Class::Game);
    assert_eq!(class("PS3_GAME/PARAM.SFO").class, Class::Game);
    assert_eq!(class("PS3_GAME/ICON0.PNG").class, Class::Allowed);
    assert_eq!(class("PS3_GAME/USRDIR/alchemy.xml").class, Class::Allowed);
    assert_eq!(class("PS3_GAME/USRDIR/EBOOT.BIN").entropy, None);

    let s = &c.summary;
    let unknown: Vec<_> = s.unknown_files.iter().map(|(p, _, _)| p.as_str()).collect();
    assert_eq!(
        unknown,
        ["PS3_GAME/USRDIR/mystery.bin", "PS3_GAME/USRDIR/zeros.bin"]
    );
    assert!(s.unknown_files[0].2 >= HIGH_ENTROPY);
    assert_eq!(s.unknown_files[1].2, 0.0);
    assert_eq!(
        s.high_entropy_unknown_files,
        ["PS3_GAME/USRDIR/mystery.bin"]
    );

    assert_eq!(s.iga_archives, 3);
    assert_eq!(s.iga_entries, 3);
    let unread: Vec<_> = s.iga_unread.iter().map(|(p, _)| p.as_str()).collect();
    assert_eq!(
        unread,
        ["PS3_GAME/USRDIR/big.pak", "PS3_GAME/USRDIR/tt.arc"]
    );
    assert!(
        s.iga_unread[1].1.contains("version 0xb"),
        "{:?}",
        s.iga_unread
    );
    assert_eq!(s.entries_by_what["IGZ object file"], 1);
    assert_eq!(s.entries_by_what["text"], 1);
    assert_eq!(s.unknown_entries, 1);
    assert_eq!(
        s.high_entropy_unknown_entries,
        ["PS3_GAME/USRDIR/character/001_a.arc: b.raw"]
    );

    assert_eq!(s.bink_files, 1);
    assert_eq!(s.bink_unprobed[0].1, "no ffprobe");
    assert!(c.ffprobe.is_none());
}

#[test]
fn a_root_inside_the_dump_is_refused() {
    let tmp = tempfile::tempdir().unwrap();
    let dump = tmp.path().join("dump");
    put(
        &dump,
        "PS3_GAME/PARAM.SFO",
        &SfoBuilder::disc_game("BLUS30968", "01.00", "01.00").build(),
    );
    let root = ExtractRoot::checked(dump.join("out")).unwrap();
    assert!(matches!(
        census(&dump, &root, None),
        Err(ExtractError::OverlapsDump { .. })
    ));
    assert!(!dump.join("out").exists());
}

#[test]
fn an_unsafe_title_names_the_report_unsafe_inside_the_root() {
    let tmp = tempfile::tempdir().unwrap();
    let dump = tmp.path().join("dump");
    put(
        &dump,
        "PS3_GAME/PARAM.SFO",
        &SfoBuilder::disc_game("x/../../y", "01.00", "01.00").build(),
    );
    let root = ExtractRoot::checked(tmp.path().join("extract")).unwrap();
    fs::create_dir_all(root.path().join("census-x")).unwrap();
    let (c, report) = census(&dump, &root, None).unwrap();
    assert_eq!(report.parent().unwrap(), root.path());
    let name = report.file_name().unwrap().to_str().unwrap();
    assert!(name.starts_with("census-unsafe-01.00-"), "{name}");
    assert_eq!(c.title_id, "x/../../y");
    assert!(Path::new(&c.dump_dir).is_absolute());
}

#[test]
fn a_root_that_became_a_checkout_is_refused_at_use() {
    let tmp = tempfile::tempdir().unwrap();
    let dump = tmp.path().join("dump");
    put(
        &dump,
        "PS3_GAME/PARAM.SFO",
        &SfoBuilder::disc_game("BLUS30968", "01.00", "01.00").build(),
    );
    let root = ExtractRoot::checked(tmp.path().join("extract")).unwrap();
    fs::create_dir(tmp.path().join(".git")).unwrap();
    assert!(matches!(
        census(&dump, &root, None),
        Err(ExtractError::InCheckout { .. })
    ));
    assert!(!root.path().exists());
}

#[test]
fn an_archive_over_the_whole_read_cap_is_unread() {
    let tmp = tempfile::tempdir().unwrap();
    let dump = tmp.path().join("dump");
    put(
        &dump,
        "PS3_GAME/PARAM.SFO",
        &SfoBuilder::disc_game("BLUS30968", "01.00", "01.00").build(),
    );
    put(
        &dump,
        "PS3_GAME/USRDIR/huge.arc",
        &starting_with(b"IGA\x1a\x08\0\0\0"),
    );
    // Sparse on APFS and ext4: nothing past the head is written.
    fs::OpenOptions::new()
        .write(true)
        .open(dump.join("PS3_GAME/USRDIR/huge.arc"))
        .unwrap()
        .set_len(MAX_WHOLE_ARCHIVE + 1)
        .unwrap();
    let root = ExtractRoot::checked(tmp.path().join("extract")).unwrap();
    let (c, _) = census(&dump, &root, None).unwrap();
    let unread = &c.summary.iga_unread;
    assert_eq!(unread.len(), 1);
    assert!(unread[0].1.contains("whole-read cap"), "{unread:?}");
}

fn walk(dir: &Path) -> Vec<(String, Vec<u8>)> {
    let mut out = Vec::new();
    for e in fs::read_dir(dir).unwrap() {
        let p = e.unwrap().path();
        if p.is_dir() {
            out.extend(walk(&p));
        } else {
            out.push((p.display().to_string(), fs::read(&p).unwrap()));
        }
    }
    out.sort();
    out
}
