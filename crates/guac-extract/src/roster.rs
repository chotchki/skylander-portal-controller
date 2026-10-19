//! The figure roster off the dump (openspec AE.2, design decisions 1-3):
//! four archives in, IR v2's [`Roster`] out.
//!
//! `character/init_setup.bld` holds the toy table, `permanent/global.bld`
//! the text keys, sets and values it names plus 12 language packs,
//! `permanent/bootstrap.bld` the 19 roots ahead of global's export ids and
//! `misc/ui_collection_champions.bld` the portraits. `guac-formats` reads
//! them ([`guac_formats::toys`], [`guac_formats::roster`]); this turns the
//! figures into owned IR with every language's text. Each figure's portrait
//! is named `portraits/<id>.png` when the disc has a sprite for it; writing
//! the PNGs is the roster stage's (AE.3).

use std::path::{Path, PathBuf};

use guac_formats::FormatError;
use guac_formats::exports::Exports;
use guac_formats::iga::Archive;
use guac_formats::igz::Igz;
use guac_formats::igz::layouts::{Layout, StringInfo};
use guac_formats::language::{LanguagePack, Texts, packs};
use guac_formats::roster::{self, Category as FCategory, Kind};
use guac_formats::toys::ToyTable;
use guac_ir::{Category, DumpIdentity, Element, FigureKind, Language, Roster, RosterFigure, Text};

use crate::{ExtractError, Result};

pub const INIT_SETUP: &str = "character/init_setup.bld";
pub const GLOBAL: &str = "permanent/global.bld";
pub const BOOTSTRAP: &str = "permanent/bootstrap.bld";
pub const COLLECTION: &str = "misc/ui_collection_champions.bld";
/// The entry each of the four archives keeps its level in.
pub const LEVEL: &str = "level.bld";

/// The roster's inputs, read out of their archives.
#[derive(Debug, Clone)]
pub struct Sources {
    pub init_setup: Vec<u8>,
    pub global: Vec<u8>,
    pub packs: Vec<LanguagePack>,
    pub bootstrap: Vec<u8>,
    pub collection: Vec<u8>,
}

fn unreadable(archive: &str, e: impl ToString) -> ExtractError {
    ExtractError::RosterUnreadable {
        path: PathBuf::from(archive),
        reason: e.to_string(),
    }
}

impl Sources {
    /// Reads the four archives under the dump's `PS3_GAME/USRDIR`.
    pub fn read(usrdir: &Path) -> Result<Self> {
        let level = |archive: &str| -> Result<(Vec<u8>, Vec<LanguagePack>)> {
            let path = usrdir.join(archive);
            let bytes = std::fs::read(&path).map_err(ExtractError::io(&path))?;
            let arc = Archive::parse(&bytes).map_err(|e| unreadable(archive, e))?;
            let entry = arc
                .find(LEVEL)
                .ok_or_else(|| unreadable(archive, "no level.bld"))?;
            let level = arc.read(entry).map_err(|e| unreadable(archive, e))?;
            let packs = if archive == GLOBAL {
                packs(&arc).map_err(|e| unreadable(archive, e))?
            } else {
                Vec::new()
            };
            Ok((level.into_owned(), packs))
        };
        let (global, packs) = level(GLOBAL)?;
        Ok(Self {
            init_setup: level(INIT_SETUP)?.0,
            global,
            packs,
            bootstrap: level(BOOTSTRAP)?.0,
            collection: level(COLLECTION)?.0,
        })
    }

    /// The roster these sources define, under `identity`.
    pub fn roster(&self, identity: DumpIdentity) -> Result<Roster> {
        let parse = |archive, bytes| Igz::parse(bytes).map_err(|e| unreadable(archive, e));
        let init = parse(INIT_SETUP, &self.init_setup)?;
        let global = parse(GLOBAL, &self.global)?;
        let boot = parse(BOOTSTRAP, &self.bootstrap)?;
        let collection = parse(COLLECTION, &self.collection)?;
        let exports = Exports::new(&boot, &global).map_err(|e| unreadable(GLOBAL, e))?;
        let texts = Texts::new(&global, self.packs.clone()).map_err(|e| unreadable(GLOBAL, e))?;
        let table =
            ToyTable::read(&init, &global, &exports).map_err(|e| unreadable(INIT_SETUP, e))?;
        let figures =
            roster::figures(&table, &global, &collection).map_err(|e| unreadable(COLLECTION, e))?;
        let text = |key| -> std::result::Result<Text, FormatError> {
            let name = StringInfo::read(&global, key)?
                .name
                .unwrap_or("")
                .to_owned();
            let texts = texts
                .texts(key)
                .unwrap_or_default()
                .into_iter()
                .map(|(_, t)| t.to_owned())
                .collect();
            Ok(Text { key: name, texts })
        };
        let english = |key| {
            texts
                .text(key, guac_ir::roster::ENGLISH)
                .map(str::to_owned)
                .ok_or_else(|| unreadable(GLOBAL, "no English pack"))
        };
        let mut out = Vec::with_capacity(figures.len());
        for f in &figures {
            let figure = RosterFigure {
                toy_type: f.toy_type,
                variant: f.variant,
                kind: match f.kind {
                    Kind::Base => FigureKind::Base,
                    Kind::Alt => FigureKind::Alt,
                    Kind::Series2 => FigureKind::Series2,
                    Kind::LightCore => FigureKind::LightCore,
                },
                deco: f.deco,
                name: text(f.name).map_err(|e| unreadable(GLOBAL, e))?,
                variant_word: f
                    .word
                    .map(text)
                    .transpose()
                    .map_err(|e| unreadable(GLOBAL, e))?,
                element: f.element.and_then(Element::from_value),
                category: match f.category {
                    FCategory::Figure => Category::Figure,
                    FCategory::Giant => Category::Giant,
                    FCategory::Item => Category::Item,
                    FCategory::AdventurePack => Category::AdventurePack,
                    FCategory::Sidekick => Category::Sidekick,
                },
                load_file: english(f.level_load)?,
                portrait: f.portrait.map(|_| format!("portraits/{}.png", f.id())),
            };
            out.push(figure);
        }
        Ok(Roster {
            identity,
            languages: texts
                .languages()
                .iter()
                .map(|l| Language {
                    id: l.id,
                    name: l.name.clone().unwrap_or_default(),
                })
                .collect(),
            figures: out,
        })
    }
}

/// The roster of the dump at `dump` (the folder holding `PS3_GAME`).
pub fn read(dump: &Path, identity: DumpIdentity) -> Result<Roster> {
    Sources::read(&dump.join("PS3_GAME").join("USRDIR"))?.roster(identity)
}

#[cfg(test)]
mod tests {
    use super::*;
    use guac_formats::roster::builder::{disc, sample};
    use guac_ir::roster::ENGLISH;
    use guac_ir::{Sha256, from_postcard, to_postcard};

    fn identity() -> DumpIdentity {
        DumpIdentity {
            title_id: "BLUS30968".into(),
            app_ver: "01.00".into(),
            manifest_hash: Sha256([1; 32]),
        }
    }

    fn sources() -> Sources {
        let d = disc(&sample());
        Sources {
            init_setup: d.init_setup,
            global: d.global,
            packs: d
                .packs
                .iter()
                .map(|p| LanguagePack::read(&Igz::parse(p).unwrap()).unwrap())
                .collect(),
            bootstrap: d.bootstrap,
            collection: d.collection,
        }
    }

    #[test]
    fn the_sample_becomes_ir_with_every_language() {
        let r = sources().roster(identity()).unwrap();
        assert_eq!(r.figures.len(), 24);
        let names: Vec<(i32, &str)> = r
            .languages
            .iter()
            .map(|l| (l.id, l.name.as_str()))
            .collect();
        assert_eq!(names, [(6, "ENGLISH"), (8, "FRENCH")]);
        let polar = r.figure("000000-0402").unwrap();
        assert_eq!(polar.kind, FigureKind::Alt);
        assert_eq!(polar.name.key, "000_Toy_Name");
        assert_eq!(polar.variant_word.as_ref().unwrap().key, "ToyType_Polar");
        assert_eq!(
            r.display_name(polar, ENGLISH).as_deref(),
            Some("Polar Whirlwind")
        );
        assert_eq!(
            r.display_name(polar, 8).as_deref(),
            Some("fr:Polar fr:Whirlwind")
        );
        assert_eq!(polar.load_file, "MinionLoad_000_AirDragonLegendary");
        assert_eq!(polar.portrait.as_deref(), Some("portraits/000000-0402.png"));
        assert_eq!(polar.element, Some(Element::Air));
        let anvil = r.figure("0000c8-0000").unwrap();
        assert_eq!((anvil.category, anvil.element), (Category::Item, None));
        let dark = r.figure("00001c-0000").unwrap();
        assert_eq!(r.display_name(dark, ENGLISH).as_deref(), Some("Dark Spyro"));
        assert_eq!(dark.kind, FigureKind::Base);
        // It survives the install format.
        assert_eq!(
            from_postcard::<Roster>(&to_postcard(&r).unwrap()).unwrap(),
            r
        );
    }

    #[test]
    fn a_broken_table_names_its_archive() {
        let mut s = sources();
        s.init_setup = s.global.clone();
        let e = s.roster(identity()).unwrap_err();
        assert_eq!(e.code(), "GUAC-E-ROSTER-UNREADABLE");
        assert!(e.to_string().contains(INIT_SETUP), "{e}");
        let mut s = sources();
        s.packs.clear();
        let e = s.roster(identity()).unwrap_err();
        assert!(e.to_string().contains("no English pack"), "{e}");
    }

    #[test]
    fn a_dump_without_the_archives_is_an_io_error() {
        let tmp = tempfile::tempdir().unwrap();
        let e = read(tmp.path(), identity()).unwrap_err();
        assert_eq!(e.code(), "GUAC-E-EXTRACT-IO");
    }
}
