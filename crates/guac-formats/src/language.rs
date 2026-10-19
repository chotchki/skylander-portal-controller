//! Language packs (openspec AE.1): an archive's display text, one `.pak` per
//! language beside its `level.bld`.
//!
//! A `.pak` is an IGZ holding one `LanguagePackInfo`: the language's name,
//! its id and `_stringDict`, UTF-16BE strings each ending in a NUL, back to
//! back with no index. The n-th string is the text of the n-th `StringInfo`
//! in the archive's `level.bld`, taken in address order, so a pack whose
//! string count differs from the level's `StringInfo` count is refused
//! rather than joined off by one. Giants' 01.00 disc holds 304 archives with
//! 12 packs each, every count equal (AE.1's disc test).

#[cfg(any(test, feature = "test-fixtures"))]
pub mod builder;

use std::collections::HashMap;

use crate::iga::Archive;
use crate::igz::Igz;
use crate::igz::layouts::{LanguagePackInfo, Layout, StringInfo, is_a};
use crate::{ErrorKind, FormatError};

const FORMAT: &str = "language pack";

/// Giants' id for English, the language guacamole shows (AE design
/// decision 3). The disc's 12: Hispanic 0, Danish 4, Dutch 5, English 6,
/// Finnish 7, French 8, German 9, Italian 14, Norwegian 17, Portuguese 19,
/// Spanish 21, Swedish 22.
pub const ENGLISH: i32 = 6;

/// A `.pak`'s `LanguagePackInfo`, its strings decoded and owned.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LanguagePack {
    /// `ENGLISH`, `FRENCH`, ...
    pub name: Option<String>,
    pub lang_id: i32,
    pub strings: Vec<String>,
}

impl LanguagePack {
    /// Reads the one `LanguagePackInfo` in `igz`. None, or more than one, is
    /// refused.
    pub fn read(igz: &Igz<'_>) -> Result<Self, FormatError> {
        let mut found = igz
            .objects()
            .iter()
            .enumerate()
            .filter(|(_, o)| is_a(igz.type_name(o), LanguagePackInfo::TYPE));
        let Some((index, _)) = found.next() else {
            return Err(FormatError::new(
                FORMAT,
                LanguagePackInfo::TYPE,
                0,
                ErrorKind::Missing,
            ));
        };
        if let Some((_, second)) = found.next() {
            return Err(igz.err_at(second.at, LanguagePackInfo::TYPE, ErrorKind::Duplicate));
        }
        let info = LanguagePackInfo::read(igz, index)?;
        let strings = match info.string_dict {
            Some(dict) => split(dict.data).map_err(|(at, kind)| {
                let loc = dict.at.offset_by(at as u32).unwrap_or(dict.at);
                igz.err_at(loc, "_stringDict", kind)
            })?,
            None => Vec::new(),
        };
        Ok(Self {
            name: info.info.name.map(str::to_owned),
            lang_id: info.lang_id,
            strings,
        })
    }
}

/// UTF-16BE strings, each ending in a NUL, back to back. Every NUL ends one,
/// so two in a row are an empty string. A dict that ends mid-string, an odd
/// byte count or an unpaired surrogate is refused, with the byte offset.
pub fn split(bytes: &[u8]) -> Result<Vec<String>, (usize, ErrorKind)> {
    let (units, rest) = bytes.as_chunks::<2>();
    if !rest.is_empty() {
        return Err((
            bytes.len() - 1,
            ErrorKind::Truncated {
                needed: bytes.len() + 1,
                available: bytes.len(),
            },
        ));
    }
    let mut out = Vec::new();
    let mut start = 0;
    for (i, unit) in units.iter().enumerate() {
        if u16::from_be_bytes(*unit) != 0 {
            continue;
        }
        let text = char::decode_utf16(units[start..i].iter().map(|u| u16::from_be_bytes(*u)))
            .collect::<Result<String, _>>()
            .map_err(|_| (2 * start, ErrorKind::InvalidUtf16))?;
        out.push(text);
        start = i + 1;
    }
    if start != units.len() {
        return Err((2 * start, ErrorKind::MissingNul { end: bytes.len() }));
    }
    Ok(out)
}

/// `level`'s `StringInfo` objects in address order: the order a pack's
/// strings follow.
pub fn string_infos(level: &Igz<'_>) -> Vec<usize> {
    let mut keys: Vec<(crate::igz::Location, usize)> = level
        .objects()
        .iter()
        .enumerate()
        .filter(|(_, o)| level.type_name(o) == StringInfo::TYPE)
        .map(|(i, o)| (o.at, i))
        .collect();
    keys.sort_unstable();
    keys.into_iter().map(|(_, i)| i).collect()
}

/// Pairs each of `level`'s `StringInfo`s with its string in `pack`, in
/// address order. A pack with more or fewer strings than the level has keys
/// is refused.
pub fn join<'p>(
    level: &Igz<'_>,
    pack: &'p LanguagePack,
) -> Result<Vec<(usize, &'p str)>, FormatError> {
    let keys = string_infos(level);
    if keys.len() != pack.strings.len() {
        return Err(FormatError::new(
            FORMAT,
            "_stringDict",
            0,
            ErrorKind::CountMismatch {
                found: pack.strings.len() as u64,
                expected: keys.len() as u64,
            },
        ));
    }
    Ok(keys
        .into_iter()
        .zip(pack.strings.iter().map(String::as_str))
        .collect())
}

/// Every `.pak` in an archive, read. A `.pak` that isn't an IGZ with one
/// `LanguagePackInfo` is refused (`level/challengevo.bld`'s voice-line packs
/// are the disc's only such, and no roster archive has one).
pub fn packs(archive: &Archive<'_>) -> Result<Vec<LanguagePack>, FormatError> {
    let mut out = Vec::new();
    for entry in archive.entries() {
        if !entry.name().ends_with(".pak") {
            continue;
        }
        let bytes = archive.read(entry)?;
        out.push(LanguagePack::read(&Igz::parse(&bytes)?)?);
    }
    Ok(out)
}

/// A pack's language: `id` is what [`Texts::text`] looks up by.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Language {
    pub name: Option<String>,
    pub id: i32,
}

/// A level's text in every language its packs carry, by `StringInfo` object.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Texts {
    languages: Vec<Language>,
    /// `StringInfo` object index -> its place in address order.
    keys: HashMap<usize, usize>,
    /// Per language, per key.
    strings: Vec<Vec<String>>,
}

impl Texts {
    /// Joins every pack to `level` ([`join`]); two packs with one language
    /// id are refused.
    pub fn new(level: &Igz<'_>, packs: Vec<LanguagePack>) -> Result<Self, FormatError> {
        let order = string_infos(level);
        let mut languages: Vec<Language> = Vec::with_capacity(packs.len());
        let mut strings = Vec::with_capacity(packs.len());
        for pack in packs {
            join(level, &pack)?;
            if languages.iter().any(|l| l.id == pack.lang_id) {
                return Err(FormatError::new(FORMAT, "_langID", 0, ErrorKind::Duplicate));
            }
            languages.push(Language {
                name: pack.name,
                id: pack.lang_id,
            });
            strings.push(pack.strings);
        }
        let keys = order.into_iter().enumerate().map(|(n, o)| (o, n)).collect();
        Ok(Self {
            languages,
            keys,
            strings,
        })
    }

    /// The languages in pack order.
    pub fn languages(&self) -> &[Language] {
        &self.languages
    }

    /// Every pack's string count, the same for all of them.
    pub fn key_count(&self) -> usize {
        self.keys.len()
    }

    /// `None` when `key` isn't one of the level's `StringInfo` objects or
    /// no pack has `lang_id`.
    pub fn text(&self, key: usize, lang_id: i32) -> Option<&str> {
        let n = *self.keys.get(&key)?;
        let lang = self.languages.iter().position(|l| l.id == lang_id)?;
        self.strings[lang].get(n).map(String::as_str)
    }

    /// In pack order; `None` when `key` isn't one of the level's
    /// `StringInfo` objects.
    pub fn texts(&self, key: usize) -> Option<Vec<(&Language, &str)>> {
        let n = *self.keys.get(&key)?;
        Some(
            self.languages
                .iter()
                .zip(&self.strings)
                .map(|(l, s)| (l, s[n].as_str()))
                .collect(),
        )
    }
}

#[cfg(test)]
mod tests;
