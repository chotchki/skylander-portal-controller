//! IR v2's roster (openspec AE.2, design decisions 2, 3 and 6): every figure
//! the user's disc defines, with the id Giants reads off a tag, its names in
//! each of the disc's languages, element, category, load file and portrait.

use serde::{Deserialize, Serialize};

use crate::DumpIdentity;

/// Giants' id for English, the language the phone and TV show.
pub const ENGLISH: i32 = 6;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Roster {
    pub identity: DumpIdentity,
    /// The disc's languages; every [`Text`] holds one string per entry, in
    /// this order.
    pub languages: Vec<Language>,
    /// In the toy table's order: a toy, its Series 2 and LightCore figures,
    /// then its alternate decos.
    pub figures: Vec<RosterFigure>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Language {
    /// Giants' language id (English 6).
    pub id: i32,
    /// As the pack names it: `ENGLISH`, `FRENCH`, ...
    pub name: String,
}

/// A text key and its text in each of [`Roster::languages`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Text {
    pub key: String,
    pub texts: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub enum FigureKind {
    Base,
    /// An alternate deco (Polar Whirlwind, Legendary Chill).
    Alt,
    Series2,
    LightCore,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub enum Category {
    Figure,
    Giant,
    Item,
    AdventurePack,
    Sidekick,
}

/// In Giants' element order: its value 0 is Magic, 7 Life.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub enum Element {
    Magic,
    Water,
    Air,
    Undead,
    Tech,
    Fire,
    Earth,
    Life,
}

impl Element {
    pub const ALL: [Element; 8] = [
        Element::Magic,
        Element::Water,
        Element::Air,
        Element::Undead,
        Element::Tech,
        Element::Fire,
        Element::Earth,
        Element::Life,
    ];

    /// Giants' element value, 0-7.
    pub fn from_value(v: u8) -> Option<Self> {
        Self::ALL.get(usize::from(v)).copied()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RosterFigure {
    pub toy_type: u32,
    /// The tag's subtype word as Giants keeps it: 0 on a base, 0x800 Series
    /// 2, 0x200 LightCore, 0x400 | deco on an alternate deco (Legendary
    /// Chill 0x603).
    pub variant: u16,
    pub kind: FigureKind,
    /// The deco id in the subtype word's low byte; set on alternate decos
    /// only.
    pub deco: Option<u8>,
    /// The toy's own name: a character's, shared by its variants.
    pub name: Text,
    /// The disc's variant word, on every figure but a plain base.
    pub variant_word: Option<Text>,
    /// `None` on items and adventure-pack pieces the disc gives its default.
    pub element: Option<Element>,
    pub category: Category,
    /// The file the figure loads, `009_Eruptor`.
    pub load_file: String,
    /// The extract-relative PNG, `portraits/<id>.png`.
    pub portrait: Option<String>,
}

impl RosterFigure {
    /// `<toy type>-<variant>`, six and four hex digits: the id the server
    /// and guacamole's database key a figure by.
    pub fn id(&self) -> String {
        format!("{:06x}-{:04x}", self.toy_type, self.variant)
    }
}

impl Roster {
    fn lang(&self, lang_id: i32) -> Option<usize> {
        self.languages.iter().position(|l| l.id == lang_id)
    }

    /// `None` when the disc has no pack for `lang_id`.
    pub fn text<'s>(&self, text: &'s Text, lang_id: i32) -> Option<&'s str> {
        text.texts.get(self.lang(lang_id)?).map(String::as_str)
    }

    /// A figure's display name: its variant word, a space, then the toy's
    /// name ("Polar Whirlwind", "Series 2 Whirlwind", "Dark Spyro"), or the
    /// name alone on a plain base. The disc has no combined string.
    pub fn display_name(&self, figure: &RosterFigure, lang_id: i32) -> Option<String> {
        let name = self.text(&figure.name, lang_id)?;
        Some(match &figure.variant_word {
            Some(word) => format!("{} {name}", self.text(word, lang_id)?),
            None => name.to_owned(),
        })
    }

    pub fn figure(&self, id: &str) -> Option<&RosterFigure> {
        self.figures.iter().find(|f| f.id() == id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Sha256, from_postcard, from_ron, to_postcard, to_ron};

    fn text(key: &str, en: &str, fr: &str) -> Text {
        Text {
            key: key.into(),
            texts: vec![en.into(), fr.into()],
        }
    }

    fn figure(variant: u16, kind: FigureKind, word: Option<Text>) -> RosterFigure {
        RosterFigure {
            toy_type: 0,
            variant,
            kind,
            deco: (kind == FigureKind::Alt).then_some(2),
            name: text("000_Toy_Name", "Whirlwind", "Whirlwind"),
            variant_word: word,
            element: Element::from_value(2),
            category: Category::Figure,
            load_file: "000_AirDragon".into(),
            portrait: Some(format!("portraits/{:06x}-{variant:04x}.png", 0)),
        }
    }

    fn roster() -> Roster {
        Roster {
            identity: DumpIdentity {
                title_id: "BLUS30968".into(),
                app_ver: "01.00".into(),
                manifest_hash: Sha256([7; 32]),
            },
            languages: vec![
                Language {
                    id: ENGLISH,
                    name: "ENGLISH".into(),
                },
                Language {
                    id: 8,
                    name: "FRENCH".into(),
                },
            ],
            figures: vec![
                figure(0, FigureKind::Base, None),
                figure(
                    0x800,
                    FigureKind::Series2,
                    Some(text("ToyType_Series2", "Series 2", "Série 2")),
                ),
                figure(
                    0x402,
                    FigureKind::Alt,
                    Some(text("ToyType_Polar", "Polar", "Polaire")),
                ),
            ],
        }
    }

    #[test]
    fn names_compose_the_variant_word_ahead_of_the_name() {
        let r = roster();
        let names: Vec<String> = r
            .figures
            .iter()
            .map(|f| r.display_name(f, ENGLISH).unwrap())
            .collect();
        assert_eq!(
            names,
            ["Whirlwind", "Series 2 Whirlwind", "Polar Whirlwind"]
        );
        assert_eq!(
            r.display_name(&r.figures[2], 8).as_deref(),
            Some("Polaire Whirlwind")
        );
        assert_eq!(r.display_name(&r.figures[2], 21), None);
    }

    #[test]
    fn ids_and_elements() {
        let r = roster();
        let ids: Vec<String> = r.figures.iter().map(RosterFigure::id).collect();
        assert_eq!(ids, ["000000-0000", "000000-0800", "000000-0402"]);
        assert_eq!(r.figure("000000-0402").unwrap().deco, Some(2));
        assert!(r.figure("000000-0002").is_none());
        assert_eq!(Element::from_value(0), Some(Element::Magic));
        assert_eq!(Element::from_value(7), Some(Element::Life));
        assert_eq!(Element::from_value(8), None);
    }

    #[test]
    fn the_roster_round_trips_through_both() {
        let r = roster();
        assert_eq!(from_ron::<Roster>(&to_ron(&r).unwrap()).unwrap(), r);
        assert_eq!(
            from_postcard::<Roster>(&to_postcard(&r).unwrap()).unwrap(),
            r
        );
    }
}
