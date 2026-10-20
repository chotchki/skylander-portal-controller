//! The sweep's figure list (openspec AE.22): every roster figure with a
//! character model, and the `.sky` from the dev's pack the game reads as that
//! figure, or none.
//!
//! A `.sky`'s first block is plaintext: the toy type is the 24-bit
//! little-endian word at 0x10 and the subtype word the 16-bit one at 0x1C.
//! Giants reads the subtype's bits 0-11 ("Save - Get Tag Subtype
//! Properties", ae-roster): the deco id in bits 0-7, then bit 11 (Series 2),
//! bit 10 (legendary, an alternate deco) and bit 9 (LightCore), each kept
//! only when the toy is in that flag's set, which the roster's rows for the
//! toy stand for. A tag reads as the toy's alternate deco when bit 10 is set
//! and the roster has that deco (Legendary Stealth Elf's `0x1C03` carries
//! bit 11 too, and Legendary Chill's `0x1603` bit 9), else its LightCore
//! figure on bit 9, else its Series 2 figure on bit 11, else the base. So
//! Volcanic Eruptor (`0x2C02`, deco 2, which no Eruptor alt has) reads as
//! Series 2 Eruptor, and LightCore Warnado (`0x2206`) as Warnado.
//!
//! Where several of the pack's files read as one figure, the one with the
//! lowest year code (bits 12-15: the game it shipped with) wins, then the path;
//! the others are counted. A figure no file reads as is listed with none,
//! never given a near miss.

use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use guac_ir::roster::{Category, FigureKind, Roster, RosterFigure};
use serde::{Deserialize, Serialize};

/// The bytes read of a `.sky`: its toy type and subtype word sit in them.
pub const HEADER_LEN: usize = 0x20;
const TOY_TYPE: usize = 0x10;
const SUBTYPE: usize = 0x1C;
const LEGENDARY: u16 = 0x0400;
const LIGHTCORE: u16 = 0x0200;
const SERIES_2: u16 = 0x0800;

/// A pack file's toy type and subtype word, read off its first block.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PackSky {
    /// Relative to the pack's root.
    pub path: PathBuf,
    pub toy_type: u32,
    /// The subtype word as stored.
    pub word: u16,
}

impl PackSky {
    /// From a `.sky`'s first [`HEADER_LEN`] bytes; `None` when there are
    /// fewer.
    pub fn from_header(path: PathBuf, header: &[u8]) -> Option<Self> {
        let h = header.get(..HEADER_LEN)?;
        Some(Self {
            path,
            toy_type: u32::from(h[TOY_TYPE])
                | u32::from(h[TOY_TYPE + 1]) << 8
                | u32::from(h[TOY_TYPE + 2]) << 16,
            word: u16::from_le_bytes([h[SUBTYPE], h[SUBTYPE + 1]]),
        })
    }

    /// Bits 12-15: the game the tag shipped with.
    pub fn year_code(&self) -> u16 {
        self.word >> 12
    }
}

/// Every `.sky` under `root` (any case), by path; each opened read-only for
/// its first [`HEADER_LEN`] bytes and nothing written. A file too short to
/// carry a header is skipped.
pub fn scan_pack(root: &Path) -> Result<Vec<PackSky>> {
    let mut out = Vec::new();
    let mut dirs = vec![root.to_path_buf()];
    while let Some(dir) = dirs.pop() {
        for entry in fs::read_dir(&dir).with_context(|| format!("listing {}", dir.display()))? {
            let entry = entry?;
            let path = entry.path();
            let kind = entry.file_type()?;
            if kind.is_dir() {
                dirs.push(path);
            } else if kind.is_file()
                && path
                    .extension()
                    .is_some_and(|e| e.eq_ignore_ascii_case("sky"))
            {
                let mut header = Vec::with_capacity(HEADER_LEN);
                fs::File::open(&path)
                    .with_context(|| format!("opening {}", path.display()))?
                    .take(HEADER_LEN as u64)
                    .read_to_end(&mut header)?;
                let rel = path.strip_prefix(root).unwrap_or(&path).to_path_buf();
                if let Some(sky) = PackSky::from_header(rel, &header) {
                    out.push(sky);
                }
            }
        }
    }
    out.sort_by(|a, b| a.path.cmp(&b.path));
    Ok(out)
}

/// The roster row Giants reads a tag of `toy_type` with subtype `word` as,
/// among `roster`'s figures; `None` when the toy has no base row either.
pub fn reads_as(roster: &Roster, toy_type: u32, word: u16) -> Option<&RosterFigure> {
    let rows: Vec<&RosterFigure> = roster
        .figures
        .iter()
        .filter(|f| f.toy_type == toy_type)
        .collect();
    let find = |kind: FigureKind, deco: Option<u8>| {
        rows.iter()
            .copied()
            .find(|f| f.kind == kind && (deco.is_none() || f.deco == deco))
    };
    let deco = (word & 0xFF) as u8;
    (word & LEGENDARY != 0)
        .then(|| find(FigureKind::Alt, Some(deco)))
        .flatten()
        .or_else(|| (word & LIGHTCORE != 0).then(|| find(FigureKind::LightCore, None))?)
        .or_else(|| (word & SERIES_2 != 0).then(|| find(FigureKind::Series2, None))?)
        .or_else(|| find(FigureKind::Base, None))
}

/// The file the game reads as a figure with the lowest year code, and how
/// many others read as it too.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Picked {
    pub path: PathBuf,
    pub word: u16,
    /// Other files in the pack that read as the same figure.
    pub others: Vec<PathBuf>,
}

/// A roster figure with a character model, as the sweep places it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SweepFigure {
    /// `<toy type>-<variant>`, the roster's id.
    pub id: String,
    /// The English display name.
    pub name: String,
    pub toy_type: u32,
    pub variant: u16,
    pub kind: FigureKind,
    pub category: Category,
    /// The character file it loads, `026_StealthElf`.
    pub load_file: String,
    /// `None`: no file in the pack reads as this figure.
    pub sky: Option<Picked>,
}

impl SweepFigure {
    /// The figure's dir under a sweep's output: `<id>-<load file>`, plus the
    /// kind where Series 2 and LightCore share their base's file.
    pub fn label(&self) -> String {
        let kind = match self.kind {
            FigureKind::Base | FigureKind::Alt => "",
            FigureKind::Series2 => "-s2",
            FigureKind::LightCore => "-lc",
        };
        format!("{}-{}{kind}", self.id, self.load_file)
    }

    /// Whether `key` names this figure: its id, its name or its load file,
    /// any case.
    pub fn matches(&self, key: &str) -> bool {
        [&self.id, &self.name, &self.load_file]
            .iter()
            .any(|k| k.eq_ignore_ascii_case(key))
    }
}

/// The roster's figures with a character model (its Figures and Giants, AE.5's
/// 99), in roster order, each with the `.sky` it takes from `pack`.
pub fn figures(roster: &Roster, pack: &[PackSky]) -> Vec<SweepFigure> {
    roster
        .figures
        .iter()
        .filter(|f| matches!(f.category, Category::Figure | Category::Giant))
        .map(|f| {
            let mut reading: Vec<&PackSky> = pack
                .iter()
                .filter(|s| {
                    s.toy_type == f.toy_type
                        && reads_as(roster, s.toy_type, s.word).is_some_and(|r| r == f)
                })
                .collect();
            reading.sort_by(|a, b| (a.year_code(), &a.path).cmp(&(b.year_code(), &b.path)));
            let sky = reading.split_first().map(|(first, rest)| Picked {
                path: first.path.clone(),
                word: first.word,
                others: rest.iter().map(|s| s.path.clone()).collect(),
            });
            SweepFigure {
                id: f.id(),
                name: roster
                    .display_name(f, guac_ir::roster::ENGLISH)
                    .unwrap_or_else(|| f.name.key.clone()),
                toy_type: f.toy_type,
                variant: f.variant,
                kind: f.kind,
                category: f.category,
                load_file: f.load_file.clone(),
                sky,
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use guac_ir::roster::{Language, Text};
    use guac_ir::{DumpIdentity, Sha256};

    fn text(s: &str) -> Text {
        Text {
            key: s.into(),
            texts: vec![s.into()],
        }
    }

    fn row(
        toy_type: u32,
        variant: u16,
        kind: FigureKind,
        category: Category,
        name: &str,
        word: Option<&str>,
        load_file: &str,
    ) -> RosterFigure {
        RosterFigure {
            toy_type,
            variant,
            kind,
            deco: (kind == FigureKind::Alt).then_some((variant & 0xFF) as u8),
            name: text(name),
            variant_word: word.map(text),
            element: None,
            category,
            load_file: load_file.into(),
            portrait: None,
            sky1: toy_type < 100,
        }
    }

    /// A slice of Giants' roster: Stealth Elf with her Series 2 and legendary
    /// figures, Chill with LightCore and legendary LightCore, Tree Rex, Eye-Brawl
    /// (no file in the pack), an item and a sidekick.
    fn roster() -> Roster {
        use Category::*;
        use FigureKind::*;
        Roster {
            identity: DumpIdentity {
                title_id: "BLUS30968".into(),
                app_ver: "01.00".into(),
                manifest_hash: Sha256([0; 32]),
            },
            languages: vec![Language {
                id: guac_ir::roster::ENGLISH,
                name: "ENGLISH".into(),
            }],
            figures: vec![
                row(26, 0, Base, Figure, "Stealth Elf", None, "026_StealthElf"),
                row(
                    26,
                    0x800,
                    Series2,
                    Figure,
                    "Stealth Elf",
                    Some("Series 2"),
                    "026_StealthElf",
                ),
                row(
                    26,
                    0x403,
                    Alt,
                    Figure,
                    "Stealth Elf",
                    Some("Legendary"),
                    "026_StealthElf_Legendary",
                ),
                row(106, 0, Base, Figure, "Chill", None, "106_Chill"),
                row(
                    106,
                    0x200,
                    LightCore,
                    Figure,
                    "Chill",
                    Some("LightCore"),
                    "106_Chill",
                ),
                row(
                    106,
                    0x603,
                    Alt,
                    Figure,
                    "Chill",
                    Some("Legendary"),
                    "106_Chill_Legendary",
                ),
                row(112, 0, Base, Giant, "Tree Rex", None, "112_TitanLife"),
                row(114, 0, Base, Giant, "Eye-Brawl", None, "114_Eyebrawl"),
                row(200, 0, Base, Item, "Anvil Rain", None, "item_anvil"),
                row(502, 0, Base, Sidekick, "Barkley", None, "item_pet_02"),
            ],
        }
    }

    fn sky(path: &str, toy_type: u32, word: u16) -> PackSky {
        PackSky {
            path: path.into(),
            toy_type,
            word,
        }
    }

    fn pack() -> Vec<PackSky> {
        vec![
            sky("Swapforce/Life/Ninja Stealth Elf.sky", 26, 0x2805),
            sky("Spyros Adventure/Life/Stealth Elf.sky", 26, 0x0000),
            sky("Giants/Life/Series 2 Stealth Elf.sky", 26, 0x1801),
            sky(
                "Giants/Life/Alternate types/Legendary Stealth Elf.sky",
                26,
                0x1C03,
            ),
            sky(
                "Swapforce/Life/Alternate types/Dark Stealth Elf.sky",
                26,
                0x2C02,
            ),
            sky("Giants/Water/Chill.sky", 106, 0x0000),
            sky("Giants/Water/LightCore Chill.sky", 106, 0x1206),
            sky("Giants/Water/Legendary Chill.sky", 106, 0x1603),
            sky("Giants/Giants/Tree Rex.sky", 112, 0x0000),
            sky("Items/Anvil Rain.sky", 200, 0x0000),
            sky(
                "Superchargers/Life/Super Shot Stealth Elf.sky",
                3415,
                0x0000,
            ),
        ]
    }

    #[test]
    fn a_tag_reads_as_legendary_then_lightcore_then_series_2_then_base() {
        let r = roster();
        let id = |toy, word| reads_as(&r, toy, word).map(RosterFigure::id);
        assert_eq!(id(26, 0x0000).as_deref(), Some("00001a-0000"));
        assert_eq!(id(26, 0x1801).as_deref(), Some("00001a-0800"));
        // Bit 11 as well, and the legendary wins.
        assert_eq!(id(26, 0x1C03).as_deref(), Some("00001a-0403"));
        // Deco 2 isn't one of her alts: bit 10 drops and bit 11 holds.
        assert_eq!(id(26, 0x2C02).as_deref(), Some("00001a-0800"));
        assert_eq!(id(106, 0x1206).as_deref(), Some("00006a-0200"));
        assert_eq!(id(106, 0x1603).as_deref(), Some("00006a-0603"));
        // Tree Rex has no LightCore or Series 2 row: the bits drop.
        assert_eq!(id(112, 0x1A00).as_deref(), Some("000070-0000"));
        assert_eq!(id(3415, 0), None, "a toy the roster doesn't have");
    }

    #[test]
    fn the_list_takes_the_characters_and_lists_a_figure_with_no_sky() {
        let r = roster();
        let list = figures(&r, &pack());
        let ids: Vec<&str> = list.iter().map(|f| f.id.as_str()).collect();
        assert_eq!(
            ids,
            [
                "00001a-0000",
                "00001a-0800",
                "00001a-0403",
                "00006a-0000",
                "00006a-0200",
                "00006a-0603",
                "000070-0000",
                "000072-0000",
            ],
            "the item and the sidekick stay out"
        );
        let by = |id: &str| list.iter().find(|f| f.id == id).unwrap();
        // The base: the Spyro's Adventure file, year code 0, over Swap Force's.
        let base = by("00001a-0000").sky.as_ref().unwrap();
        assert_eq!(
            base.path,
            Path::new("Spyros Adventure/Life/Stealth Elf.sky")
        );
        assert!(base.others.is_empty());
        // Series 2: Giants' own over the two Swap Force tags that read as it.
        let s2 = by("00001a-0800").sky.as_ref().unwrap();
        assert_eq!(s2.path, Path::new("Giants/Life/Series 2 Stealth Elf.sky"));
        assert_eq!(s2.others.len(), 2);
        assert_eq!(by("00001a-0403").sky.as_ref().unwrap().word, 0x1C03);
        assert_eq!(by("00001a-0403").name, "Legendary Stealth Elf");
        assert_eq!(
            by("00006a-0603").sky.as_ref().unwrap().path,
            Path::new("Giants/Water/Legendary Chill.sky")
        );
        // Eye-Brawl: nothing in the pack reads as him, so none, not a guess.
        let eye = by("000072-0000");
        assert!(eye.sky.is_none());
        assert_eq!(eye.category, Category::Giant);
        assert_eq!(by("00001a-0800").label(), "00001a-0800-026_StealthElf-s2");
        assert_eq!(by("00006a-0200").label(), "00006a-0200-106_Chill-lc");
        assert!(by("00001a-0000").matches("stealth elf"));
        assert!(by("00001a-0000").matches("026_stealthelf"));
        assert!(!by("00001a-0000").matches("026"));
    }

    #[test]
    fn the_pack_scan_reads_headers_and_writes_nothing() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("Giants/Life");
        fs::create_dir_all(&dir).unwrap();
        let mut block = vec![0u8; 1024];
        block[0x10..0x13].copy_from_slice(&[26, 0, 0]);
        block[0x1C..0x1E].copy_from_slice(&0x1C03u16.to_le_bytes());
        let file = dir.join("Legendary Stealth Elf.SKY");
        fs::write(&file, &block).unwrap();
        fs::write(dir.join("short.sky"), [0u8; 8]).unwrap();
        fs::write(dir.join("desktop.ini"), b"x").unwrap();
        let before = fs::metadata(&file).unwrap().modified().unwrap();
        let scanned = scan_pack(tmp.path()).unwrap();
        assert_eq!(
            scanned,
            [sky("Giants/Life/Legendary Stealth Elf.SKY", 26, 0x1C03)]
        );
        assert_eq!(scanned[0].year_code(), 1);
        assert_eq!(fs::read(&file).unwrap(), block);
        assert_eq!(fs::metadata(&file).unwrap().modified().unwrap(), before);
    }
}
