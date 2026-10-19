//! The figure roster off the dump (openspec AE.2, design decisions 1-3):
//! four archives in, IR v2's [`Roster`] out.
//!
//! `character/init_setup.bld` holds the toy table, `permanent/global.bld`
//! the text keys, sets and values it names plus 12 language packs,
//! `permanent/bootstrap.bld` the 19 roots ahead of global's export ids and
//! `misc/ui_collection_champions.bld` the portraits. `guac-formats` reads
//! them ([`guac_formats::toys`], [`guac_formats::roster`]); this turns the
//! figures into owned IR with every language's text. Each figure's portrait
//! is named `portraits/<id>.png` when the disc has a sprite for it, and
//! [`png`] makes that file out of the sprite's image; writing them is the
//! roster stage's ([`crate::stages`], AE.3).

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use guac_formats::FormatError;
use guac_formats::exports::Exports;
use guac_formats::iga::Archive;
use guac_formats::igz::Igz;
use guac_formats::igz::layouts::{Layout, StringInfo};
use guac_formats::language::{LanguagePack, Texts, packs};
use guac_formats::roster::{self, Category as FCategory, Kind};
use guac_formats::texture::Texture;
use guac_formats::toys::ToyTable;
use guac_ir::roster::ENGLISH;
use guac_ir::{Category, DumpIdentity, Element, FigureKind, Language, Roster, RosterFigure, Text};

use crate::{ExtractError, Result};

pub const INIT_SETUP: &str = "character/init_setup.bld";
pub const GLOBAL: &str = "permanent/global.bld";
pub const BOOTSTRAP: &str = "permanent/bootstrap.bld";
pub const COLLECTION: &str = "misc/ui_collection_champions.bld";
/// The entry each of the four archives keeps its level in.
pub const LEVEL: &str = "level.bld";
/// Where the portraits go in the extract, one `<figure id>.png` each.
pub const PORTRAIT_DIR: &str = "portraits";

/// A figure's portrait as the disc holds it: its file in the extract and the
/// sprite's image, untiled.
#[derive(Debug, Clone)]
pub struct Portrait {
    /// `portraits/<figure id>.png`, the figure's [`RosterFigure::portrait`].
    pub path: String,
    pub texture: Texture,
}

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
        Ok(self.roster_and_portraits(identity)?.0)
    }

    /// [`Self::roster`] plus every figure's portrait image, in roster order.
    pub fn roster_and_portraits(&self, identity: DumpIdentity) -> Result<(Roster, Vec<Portrait>)> {
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
        let mut portraits = Vec::new();
        for f in &figures {
            let portrait = match f.portrait {
                Some(sprite) => {
                    let path = format!("{PORTRAIT_DIR}/{}.png", f.id());
                    let texture = roster::portrait_image(&collection, sprite)
                        .and_then(|image| Texture::read(&collection, image))
                        .map_err(|e| unreadable(COLLECTION, e))?;
                    portraits.push(Portrait {
                        path: path.clone(),
                        texture,
                    });
                    Some(path)
                }
                None => None,
            };
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
                portrait,
            };
            out.push(figure);
        }
        let roster = Roster {
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
        };
        Ok((roster, portraits))
    }
}

/// The roster of the dump at `dump` (the folder holding `PS3_GAME`).
pub fn read(dump: &Path, identity: DumpIdentity) -> Result<Roster> {
    Ok(read_with_portraits(dump, identity)?.0)
}

/// [`read`] plus every figure's portrait image.
pub fn read_with_portraits(dump: &Path, identity: DumpIdentity) -> Result<(Roster, Vec<Portrait>)> {
    Sources::read(&dump.join("PS3_GAME").join("USRDIR"))?.roster_and_portraits(identity)
}

/// Each toy type's English name off its plain figure (variant 0), so 28 is
/// "Dark Spyro" and 100 "Jet-Vac": the intro index's fallback (AE.3).
pub fn base_names(roster: &Roster) -> BTreeMap<u32, String> {
    roster
        .figures
        .iter()
        .filter(|f| f.variant == 0)
        .filter_map(|f| Some((f.toy_type, roster.display_name(f, ENGLISH)?)))
        .collect()
}

/// `portrait` as the PNG the phone and the TV show: level 0 decoded to
/// straight 8-bit RGBA.
pub fn png(portrait: &Portrait) -> Result<Vec<u8>> {
    let refused = |e: &dyn ToString| ExtractError::RosterUnreadable {
        path: PathBuf::from(&portrait.path),
        reason: e.to_string(),
    };
    let t = &portrait.texture;
    let rgba = t.rgba8(0).map_err(|e| refused(&e))?;
    let mut out = Vec::new();
    let mut encoder = ::png::Encoder::new(&mut out, t.width, t.height);
    encoder.set_color(::png::ColorType::Rgba);
    encoder.set_depth(::png::BitDepth::Eight);
    let mut writer = encoder.write_header().map_err(|e| refused(&e))?;
    writer.write_image_data(&rgba).map_err(|e| refused(&e))?;
    writer.finish().map_err(|e| refused(&e))?;
    Ok(out)
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
    fn every_portrait_comes_with_its_image_and_decodes_to_a_png() {
        let (r, portraits) = sources().roster_and_portraits(identity()).unwrap();
        let named: Vec<&str> = r
            .figures
            .iter()
            .filter_map(|f| f.portrait.as_deref())
            .collect();
        let paths: Vec<&str> = portraits.iter().map(|p| p.path.as_str()).collect();
        assert_eq!(paths, named);
        assert_eq!(paths.len(), 24);
        for p in &portraits {
            let bytes = png(p).unwrap();
            let mut reader = ::png::Decoder::new(std::io::Cursor::new(&bytes))
                .read_info()
                .unwrap();
            let mut rgba = vec![0; reader.output_buffer_size().unwrap()];
            let info = reader.next_frame(&mut rgba).unwrap();
            assert_eq!(
                (info.width, info.height, info.color_type, info.bit_depth),
                (4, 4, ::png::ColorType::Rgba, ::png::BitDepth::Eight),
                "{}",
                p.path
            );
            assert_eq!(rgba, p.texture.rgba8(0).unwrap(), "{}", p.path);
        }
        // A level that won't decode names the portrait.
        let mut broken = portraits[0].clone();
        broken.texture.levels[0].truncate(3);
        let e = png(&broken).unwrap_err();
        assert_eq!(e.code(), "GUAC-E-ROSTER-UNREADABLE");
        assert!(e.to_string().contains(&broken.path), "{e}");
    }

    #[test]
    fn base_names_are_each_toy_types_plain_figure_in_english() {
        let names = base_names(&sources().roster(identity()).unwrap());
        assert_eq!(names[&0], "Whirlwind");
        // A toy type of its own keeps its variant word.
        assert_eq!(names[&0x1c], "Dark Spyro");
        assert_eq!(names[&0x64], "Jet-Vac");
        assert_eq!(names[&0xc8], "Anvil Rain");
        // Polar Whirlwind, Series 2 and LightCore rows don't replace a base.
        assert_eq!(names[&7], "Prism Break");
        assert!(!names.contains_key(&999), "DEBUG is dropped");
        assert_eq!(names.len(), 13);
    }

    #[test]
    fn the_builders_archives_read_back_from_a_dump() {
        let tmp = tempfile::tempdir().unwrap();
        let usrdir = tmp.path().join("PS3_GAME/USRDIR");
        for (path, bytes) in disc(&sample()).archives() {
            let path = usrdir.join(path);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, bytes).unwrap();
        }
        let (r, portraits) = read_with_portraits(tmp.path(), identity()).unwrap();
        // Packs come in the archive's entry order, so compare by language.
        let want = sources().roster(identity()).unwrap();
        let names = |r: &Roster, lang| -> Vec<(String, Option<String>)> {
            r.figures
                .iter()
                .map(|f| (f.id(), r.display_name(f, lang)))
                .collect()
        };
        for lang in [ENGLISH, 8] {
            assert_eq!(names(&r, lang), names(&want, lang));
        }
        assert_eq!(portraits.len(), 24);
    }

    #[test]
    fn a_dump_without_the_archives_is_an_io_error() {
        let tmp = tempfile::tempdir().unwrap();
        let e = read(tmp.path(), identity()).unwrap_err();
        assert_eq!(e.code(), "GUAC-E-EXTRACT-IO");
    }
}
