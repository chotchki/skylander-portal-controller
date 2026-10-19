//! The figures the toy table defines (openspec AE.2, design decisions 1-3):
//! one per toy type, Series 2 and LightCore variant and alternate deco, each
//! with the id Giants reads off a tag, its name keys, element, category and
//! collection-screen portrait.
//!
//! - Ids are `<toy type>-<subtype word>`, the word as "Save - Get Tag Subtype
//!   Properties" keeps it: 0 on a base, 0x800 Series 2 (bit 11, a toy in
//!   `sky1 characters with repose`), 0x200 LightCore (bit 9, a toy in
//!   `display as lightcore`), 0x400 | deco id on an alternate deco (bit 10).
//!   An alt whose portrait is in the Legendary-LightCore set (Legendary
//!   Chill) carries bit 9 too: 0x603.
//! - Names are the toy's name key and, on a variant, its word: the disc's
//!   display string, "Legendary" for a null one, "Series 2" and "LightCore"
//!   for those rows, an alt character's prefix (Dark Spyro) on a toy of its
//!   own. The disc has no combined string; the composed name is the word, a
//!   space, then the name.
//! - Category: `giant loads` Giant; `pet level loads` Sidekick;
//!   `Location_Loads`, or an `Item_Loads` toy in the location toys' hundred
//!   (Volcanic Vault, 304, beside 300-303), AdventurePack; `Item_Loads` Item;
//!   the rest Figure. An alt or variant row takes its toy's.
//! - Element: the call's value, 0-7; 0 on an Item or AdventurePack is the
//!   macro's default and reads as none.
//! - Portraits: `misc/ui_collection_champions.bld`'s eight `Collection_*`
//!   sprite sets, each in toy-id order, matched by the toy id in the sprite's
//!   name; the old legendaries' set by position against the alt-character
//!   calls instead, since it follows their order and names Dark Spyro's
//!   sprite under 416.

#[cfg(any(test, feature = "test-fixtures"))]
pub mod builder;

use crate::igz::Igz;
use crate::igz::layouts::{
    AlphaTextureUnitIdAttr, AnimatableTextureDataInfo, Layout, ObjectRef, ScriptSet, ScriptSetList,
    SpriteInfo, StringInfo, TextureAttr2, is_a,
};
use crate::toys::{DEBUG_TOY, Key, Toy, ToyTable, sets};
use crate::{ErrorKind, FormatError};

const FORMAT: &str = "roster";

/// The tag subtype word's bits (design decision 2).
pub mod bits {
    pub const LIGHTCORE: u16 = 0x200;
    pub const LEGENDARY: u16 = 0x400;
    pub const SERIES_2: u16 = 0x800;
}

/// The variant words' keys in `global.bld`.
pub mod words {
    pub const SERIES_2: &str = "ToyType_Series2";
    pub const LIGHTCORE: &str = "ToyType_LightCore";
    /// What a null alt display string reads.
    pub const LEGENDARY: &str = "ToyType_Legendary";
}

/// `global.bld`'s item and location sets, by name.
pub const ITEM_LOADS: &str = "Item_Loads";
pub const LOCATION_LOADS: &str = "Location_Loads";

/// The collection screen's portrait sets.
pub mod sprite_sets {
    pub const MINIONS: &str = "Collection_MinionIcons";
    pub const SERIES_2: &str = "Collection_MinionIcons_Series2";
    pub const LIGHTCORE: &str = "Collection_MinionIcons_LightCore";
    pub const LEGENDARY: &str = "Collection_MinionIcons_Legendary";
    pub const LEGENDARY_LIGHTCORE: &str = "Collection_MinionIcons_LegendaryLightcore";
    pub const OLD_LEGENDARIES: &str = "Collection_MinionIcons_OldLegendaries";
    pub const ITEMS: &str = "Collection_ItemIcons";
    pub const ITEMS_LEGENDARY: &str = "Collection_ItemIcons_Legendary";
    pub const ALL: [&str; 8] = [
        MINIONS,
        SERIES_2,
        LIGHTCORE,
        LEGENDARY,
        LEGENDARY_LIGHTCORE,
        OLD_LEGENDARIES,
        ITEMS,
        ITEMS_LEGENDARY,
    ];
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Kind {
    Base,
    Alt,
    Series2,
    LightCore,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Category {
    Figure,
    Giant,
    Item,
    AdventurePack,
    Sidekick,
}

/// One figure of the roster. Keys are `StringInfo` objects of `global.bld`;
/// the portrait is a `tfbSpriteInfo` of the collection file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Figure {
    pub toy_type: u32,
    /// The tag subtype word Giants keeps ([`bits`]).
    pub variant: u16,
    pub kind: Kind,
    /// The deco id an alt's tag carries in subtype bits 0-7; `None` on the
    /// other kinds.
    pub deco: Option<u8>,
    /// The toy's own name key, the same on all its rows.
    pub name: Key,
    /// The variant word, on everything but a plain base.
    pub word: Option<Key>,
    /// 0 Magic, 1 Water, 2 Air, 3 Undead, 4 Tech, 5 Fire, 6 Earth, 7 Life.
    pub element: Option<u8>,
    pub category: Category,
    /// The level load key; its text is the file the figure loads.
    pub level_load: Key,
    pub portrait: Option<usize>,
}

impl Figure {
    /// `<toy type>-<variant>`, six and four hex digits, as the server's ids.
    pub fn id(&self) -> String {
        format!("{:06x}-{:04x}", self.toy_type, self.variant)
    }
}

/// A `StringInfo` of `global` by key name.
fn key(global: &Igz<'_>, name: &str) -> Result<Key, FormatError> {
    let mut found = None;
    for (i, o) in global.objects().iter().enumerate() {
        if global.type_name(o) != StringInfo::TYPE {
            continue;
        }
        if StringInfo::read(global, i)?.name == Some(name) {
            if found.is_some() {
                return Err(FormatError::new(
                    FORMAT,
                    "StringInfo",
                    0,
                    ErrorKind::Duplicate,
                ));
            }
            found = Some(i);
        }
    }
    found.ok_or_else(|| FormatError::new(FORMAT, "StringInfo", 0, ErrorKind::Missing))
}

/// The members of the one `ScriptSet` of `igz` named `name`, every one a
/// local object.
fn set_members(igz: &Igz<'_>, name: &str) -> Result<Vec<usize>, FormatError> {
    let mut found = None;
    for (i, o) in igz.objects().iter().enumerate() {
        if !is_a(igz.type_name(o), ScriptSet::TYPE) {
            continue;
        }
        let set = ScriptSet::read(igz, i)?;
        if set.name == Some(name) {
            if found.is_some() {
                return Err(FormatError::new(
                    FORMAT,
                    "ScriptSet",
                    0,
                    ErrorKind::Duplicate,
                ));
            }
            found = Some(set.list);
        }
    }
    let list = found
        .ok_or_else(|| FormatError::new(FORMAT, "ScriptSet", 0, ErrorKind::Missing))?
        .ok_or_else(|| FormatError::new(FORMAT, "_list", 0, ErrorKind::Missing))?;
    ScriptSetList::read(igz, list)?
        .elements
        .into_iter()
        .map(|e| match e {
            ObjectRef::Object(o) => Ok(o),
            _ => Err(FormatError::new(
                FORMAT,
                "element",
                0,
                ErrorKind::Unsupported { value: 0 },
            )),
        })
        .collect()
}

/// A sprite set: (sprite object, the toy id its name carries).
struct SpriteSet {
    sprites: Vec<(usize, Option<u32>)>,
}

impl SpriteSet {
    fn read(collection: &Igz<'_>, name: &str) -> Result<Self, FormatError> {
        let mut sprites = Vec::new();
        for o in set_members(collection, name)? {
            let sprite = SpriteInfo::read(collection, o)?;
            sprites.push((o, sprite.placement.name.and_then(toy_id_in)));
        }
        Ok(Self { sprites })
    }

    /// The one sprite carrying `toy`, if exactly one does.
    fn by_toy(&self, toy: u32) -> Option<usize> {
        let mut hits = self.sprites.iter().filter(|(_, t)| *t == Some(toy));
        match (hits.next(), hits.next()) {
            (Some((o, _)), None) => Some(*o),
            _ => None,
        }
    }
}

/// The toy id a portrait sprite's name carries: the first three-digit part
/// after `Collection_MinionIcon_` or `Collection_ItemIcon_`
/// (`Collection_MinionIcon_Leg_000_AirDragon` is toy 0).
pub fn toy_id_in(sprite: &str) -> Option<u32> {
    let rest = sprite
        .strip_prefix("Collection_MinionIcon_")
        .or_else(|| sprite.strip_prefix("Collection_ItemIcon_"))?;
    rest.split('_')
        .find(|t| t.len() == 3 && t.bytes().all(|b| b.is_ascii_digit()))
        .and_then(|t| t.parse().ok())
}

/// The `igImage2` a portrait sprite draws: sprite -> its
/// `AnimatableTextureDataInfo` -> `tfbAlphaTextureUnitIDAttr` ->
/// `igTextureAttr2` -> image.
pub fn portrait_image(collection: &Igz<'_>, sprite: usize) -> Result<usize, FormatError> {
    let missing = |field| FormatError::new(FORMAT, field, 0, ErrorKind::Missing);
    let s = SpriteInfo::read(collection, sprite)?;
    let data = s
        .texture_data(collection)
        .ok_or_else(|| missing("_contextDataInfo"))?;
    let bind = AnimatableTextureDataInfo::read(collection, data)?
        .tex_attr
        .ok_or_else(|| missing("_texAttr"))?;
    let texture = AlphaTextureUnitIdAttr::read(collection, bind)?
        .bind
        .texture
        .ok_or_else(|| missing("_texture"))?;
    TextureAttr2::read(collection, texture)?
        .image
        .ok_or_else(|| missing("_image"))
}

/// The roster: every figure `table` defines but DEBUG, in table order (a
/// toy, its Series 2 and LightCore rows, then its alts).
pub fn figures(
    table: &ToyTable,
    global: &Igz<'_>,
    collection: &Igz<'_>,
) -> Result<Vec<Figure>, FormatError> {
    let series_2 = key(global, words::SERIES_2)?;
    let lightcore = key(global, words::LIGHTCORE)?;
    let legendary = key(global, words::LEGENDARY)?;
    let items = set_members(global, ITEM_LOADS)?;
    let locations = set_members(global, LOCATION_LOADS)?;
    let location_hundreds: Vec<i32> = table
        .toys
        .iter()
        .filter(|t| locations.contains(&t.level_load))
        .map(|t| t.item_code / 100)
        .collect();
    let category = |toy: &Toy| {
        if toy.in_set(sets::GIANT) {
            Category::Giant
        } else if toy.in_set(sets::PET) {
            Category::Sidekick
        } else if locations.contains(&toy.level_load)
            || (items.contains(&toy.level_load)
                && location_hundreds.contains(&(toy.item_code / 100)))
        {
            Category::AdventurePack
        } else if items.contains(&toy.level_load) {
            Category::Item
        } else {
            Category::Figure
        }
    };
    let mut sprite_sets = Vec::new();
    for name in sprite_sets::ALL {
        sprite_sets.push((name, SpriteSet::read(collection, name)?));
    }
    let set = |name| {
        &sprite_sets
            .iter()
            .find(|(n, _)| *n == name)
            .expect("read above")
            .1
    };

    let mut out = Vec::new();
    for (i, toy) in table.toys.iter().enumerate() {
        if toy.item_code == DEBUG_TOY {
            continue;
        }
        let toy_type = u32::try_from(toy.item_code).map_err(|_| {
            FormatError::new(
                FORMAT,
                "item code",
                0,
                ErrorKind::OutOfRange {
                    value: toy.item_code as i64 as u64,
                    limit: u32::MAX.into(),
                },
            )
        })?;
        let cat = category(toy);
        let element = match (u8::try_from(toy.element), cat) {
            (Ok(0), Category::Item | Category::AdventurePack) => None,
            (Ok(e @ 0..=7), _) => Some(e),
            _ => {
                return Err(FormatError::new(
                    FORMAT,
                    "element",
                    0,
                    ErrorKind::OutOfRange {
                        value: toy.element as i64 as u64,
                        limit: 8,
                    },
                ));
            }
        };
        let row = |variant, kind, deco, word, level_load, portrait| Figure {
            toy_type,
            variant,
            kind,
            deco,
            name: toy.name,
            word,
            element,
            category: cat,
            level_load,
            portrait,
        };
        let item = matches!(
            cat,
            Category::Item | Category::AdventurePack | Category::Sidekick
        );

        // The base: a toy of its own may be another's variant.
        let alt_character = table.alt_characters.iter().position(|a| a.alt == i);
        let portrait = match alt_character {
            Some(n) => set(sprite_sets::OLD_LEGENDARIES)
                .sprites
                .get(n)
                .map(|(o, _)| *o),
            None if item => set(sprite_sets::ITEMS).by_toy(toy_type),
            None => set(sprite_sets::MINIONS).by_toy(toy_type),
        };
        let word = alt_character.map(|n| table.alt_characters[n].prefix);
        out.push(row(0, Kind::Base, None, word, toy.level_load, portrait));
        if toy.in_set(sets::REPOSE) {
            let p = set(sprite_sets::SERIES_2).by_toy(toy_type);
            out.push(row(
                bits::SERIES_2,
                Kind::Series2,
                None,
                Some(series_2),
                toy.level_load,
                p,
            ));
        }
        if table.is_lightcore(toy) {
            let p = set(sprite_sets::LIGHTCORE).by_toy(toy_type);
            out.push(row(
                bits::LIGHTCORE,
                Kind::LightCore,
                None,
                Some(lightcore),
                toy.level_load,
                p,
            ));
        }
        for alt in table.alts.iter().filter(|a| a.toy == i) {
            let deco = u8::try_from(alt.deco).map_err(|_| {
                FormatError::new(
                    FORMAT,
                    "check against deco id",
                    0,
                    ErrorKind::OutOfRange {
                        value: alt.deco as i64 as u64,
                        limit: 0x100,
                    },
                )
            })?;
            let mut variant = bits::LEGENDARY | u16::from(deco);
            let portrait = if item {
                set(sprite_sets::ITEMS_LEGENDARY).by_toy(toy_type)
            } else if let Some(p) = set(sprite_sets::LEGENDARY_LIGHTCORE).by_toy(toy_type) {
                variant |= bits::LIGHTCORE;
                Some(p)
            } else {
                set(sprite_sets::LEGENDARY).by_toy(toy_type)
            };
            let word = alt.display.unwrap_or(legendary);
            out.push(row(
                variant,
                Kind::Alt,
                Some(deco),
                Some(word),
                alt.level_load,
                portrait,
            ));
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests;
