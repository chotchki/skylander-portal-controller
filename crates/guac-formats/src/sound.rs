//! A file's sounds and where their banks are (openspec AR.3).
//!
//! An IGZ names its sounds as `tfbSoundInfo`s, each pointing at a
//! `tfbSoundDataInfo` in the same file or in the language pack loaded
//! beside it: an EXID import in the namespace `level.bld` whose object word
//! is a position in the pack's root list, 1-based (the pack's root 0 is its
//! `LanguagePackInfo`, so the first data info is id 2). That's how a voice's
//! sound reaches the language played: each `<LANGUAGE>.pak` of an archive
//! holds a data info per voice line under the same name and at the same
//! position. An import from `global.bld` by export id is read too.
//!
//! A data info holds its FSB5 bank in its `_data` (a sound effect's), or
//! holds none and is streamed (a voice's, music's or ambience's): its bank
//! is an entry of the archive's `.arc`, named by the data info's path. The
//! data info's `_name` (`Levels/Level_213/sounds/mu_213_explore.wav`) is the
//! entry's path lower-cased under [`STREAM_PREFIX`], with a leaf
//! (`/0x<8 hex>.wav.hz.wav.enc`) the join doesn't need; a pack other than
//! English's streams from the entry with its language's folder before the
//! file name ([`localized`], [`LANGUAGES`]).
//!
//! `_streamArchiveHash` names the stream too, by a function not found yet
//! (design decision 4), so the join is by path and the hash is only kept.

#[cfg(any(test, feature = "test-fixtures"))]
pub mod builder;

use std::collections::BTreeMap;

use crate::FormatError;
use crate::exports::Exports;
use crate::igz::Igz;
use crate::igz::layouts::{Layout, ObjectRef, RootList, SoundDataInfo, SoundInfo, is_a, read_all};

/// The namespace a file imports its language pack's data infos from.
pub const PACK_NAMESPACE: &str = "level.bld";

/// The build machine's root every stream entry's name starts under; the
/// join compares it case-insensitively.
pub const STREAM_PREFIX: &str = "c:/tfb/build/ps3/";
/// The language folders a localized voice sits in; English has none. The
/// disc's eleven, 6,313-6,320 entries each.
pub const LANGUAGES: [&str; 11] = [
    "danish",
    "dutch",
    "finnish",
    "french",
    "german",
    "hispanic",
    "italian",
    "norwegian",
    "portuguese",
    "spanish",
    "swedish",
];

/// The two homes a sound's FSB5 bank can have.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Bank<'a> {
    /// The FSB5 bank in the data info's `_data`.
    Embedded(&'a [u8]),
    /// The archive's `.arc` entry whose path is this (lower-cased), and the
    /// stream hash the data info carries.
    Streamed { path: String, hash: i32 },
}

/// A `tfbSoundDataInfo` and its bank.
#[derive(Debug, Clone, PartialEq)]
pub struct Data<'a> {
    pub info: SoundDataInfo<'a>,
    /// `None` when the data info holds no data and no name to stream by.
    pub bank: Option<Bank<'a>>,
}

impl<'a> Data<'a> {
    pub fn name(&self) -> Option<&'a str> {
        self.info.data_info.info.name
    }
}

/// A sound's `_tfbSoundDataInfo`, sorted out against its own file's objects
/// and `global.bld`'s export namespace.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DataRef {
    /// Null: the sound plays nothing.
    None,
    /// A data info of the same file, by object index.
    Local(usize),
    /// A `global.bld` export, by export id ([`resolve_global`]).
    Global(u32),
    /// The language pack's root, by its 1-based position ([`resolve_pack`]).
    Pack(u32),
    /// Anything else: an import from another namespace, a named ref, or a
    /// local object that isn't a data info.
    Other,
}

/// A `tfbSoundInfo` and what its data ref points at.
#[derive(Debug, Clone, PartialEq)]
pub struct Sound<'a> {
    pub info: SoundInfo<'a>,
    pub data: DataRef,
}

/// Every `tfbSoundDataInfo` in `igz`, by object index, each read or the
/// reason it wasn't.
pub fn data_infos<'a>(igz: &Igz<'a>) -> Vec<(usize, Result<Data<'a>, FormatError>)> {
    read_all::<SoundDataInfo>(igz)
        .map(|(i, r)| {
            (
                i,
                r.map(|info| {
                    let name = info.data_info.info.name;
                    let bank = match info.data_info.data {
                        Some(m) => Some(Bank::Embedded(m.data)),
                        None => name.map(|n| Bank::Streamed {
                            path: n.to_ascii_lowercase(),
                            hash: info.data_info.stream_archive_hash,
                        }),
                    };
                    Data { info, bank }
                }),
            )
        })
        .collect()
}

/// Every `tfbSoundInfo` in `igz`, by object index, its data ref sorted out
/// against `igz`'s own objects.
pub fn sounds<'a>(igz: &Igz<'a>) -> Vec<(usize, Result<Sound<'a>, FormatError>)> {
    read_all::<SoundInfo>(igz)
        .map(|(i, r)| {
            (
                i,
                r.map(|info| {
                    let data = match info.data {
                        ObjectRef::Null => DataRef::None,
                        ObjectRef::Object(o)
                            if igz
                                .objects()
                                .get(o)
                                .is_some_and(|ob| is_a(igz.type_name(ob), SoundDataInfo::TYPE)) =>
                        {
                            DataRef::Local(o)
                        }
                        ObjectRef::Import(imp)
                            if imp.id.namespace
                                == crate::fnv1a32(crate::exports::NAMESPACE.as_bytes()) =>
                        {
                            DataRef::Global(imp.id.object)
                        }
                        ObjectRef::Import(imp)
                            if imp.id.namespace == crate::fnv1a32(PACK_NAMESPACE.as_bytes()) =>
                        {
                            DataRef::Pack(imp.id.object)
                        }
                        _ => DataRef::Other,
                    };
                    Sound { info, data }
                }),
            )
        })
        .collect()
}

/// The `global.bld` object a [`DataRef::Global`] id lands on, if it lands
/// on one of that file's own objects (not a bootstrap one, not an import
/// it re-exports).
pub fn resolve_global(exports: &Exports, id: u32) -> Option<usize> {
    match exports.resolve(id).ok()? {
        crate::exports::Export::Global(crate::exports::Root::Object(o)) => Some(o),
        _ => None,
    }
}

/// The data info a [`DataRef::Pack`] id names in `pack` (a `.pak`'s IGZ):
/// its root at position `id - 1`, if that's a local data info.
pub fn resolve_pack(pack: &Igz<'_>, id: u32) -> Option<usize> {
    let roots = RootList::read(pack, 0).ok()?;
    match roots.elements.get(id.checked_sub(1)? as usize)? {
        ObjectRef::Object(o)
            if pack
                .objects()
                .get(*o)
                .is_some_and(|ob| is_a(pack.type_name(ob), SoundDataInfo::TYPE)) =>
        {
            Some(*o)
        }
        _ => None,
    }
}

/// The stream path a pack in `language` plays for a data info named
/// `path`: its language's folder before the file name, lower-cased.
pub fn localized(path: &str, language: &str) -> String {
    let path = path.to_ascii_lowercase();
    let language = language.to_ascii_lowercase();
    match path.rsplit_once('/') {
        Some((dir, file)) => format!("{dir}/{language}/{file}"),
        None => format!("{language}/{path}"),
    }
}

/// The language folder `path` names: its file's parent folder, if that's
/// one of [`LANGUAGES`] (case ignored).
pub fn language_of(path: &str) -> Option<&'static str> {
    let mut parts = path.rsplit('/');
    parts.next()?;
    let folder = parts.next()?;
    LANGUAGES
        .iter()
        .copied()
        .find(|l| l.eq_ignore_ascii_case(folder))
}

/// A stream entry's path and language, from its name: the name under
/// [`STREAM_PREFIX`] up to its leaf. `None` for a name of another shape.
pub fn stream_path(entry: &str) -> Option<(&str, Option<&'static str>)> {
    let rest = entry.get(..STREAM_PREFIX.len())?;
    if !rest.eq_ignore_ascii_case(STREAM_PREFIX) {
        return None;
    }
    let rest = &entry[STREAM_PREFIX.len()..];
    let (path, leaf) = rest.rsplit_once('/')?;
    let hex = leaf.strip_prefix("0x")?.strip_suffix(".wav.hz.wav.enc")?;
    if hex.len() != 8 || !hex.bytes().all(|b| b.is_ascii_hexdigit()) || path.is_empty() {
        return None;
    }
    Some((path, language_of(path)))
}

/// Why a path found no single stream.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Miss {
    /// No entry has the path.
    None,
    /// Several do: their entry indexes.
    Many(Vec<usize>),
}

/// An `.arc`'s stream entries by path (lower-cased).
#[derive(Debug, Clone, Default)]
pub struct Streams {
    paths: BTreeMap<String, Vec<usize>>,
}

impl Streams {
    /// Indexes every entry name of the right shape ([`stream_path`]).
    pub fn new<'n>(entries: impl IntoIterator<Item = (usize, &'n str)>) -> Self {
        let mut paths: BTreeMap<String, Vec<usize>> = BTreeMap::new();
        for (i, name) in entries {
            if let Some((path, _)) = stream_path(name) {
                paths.entry(path.to_ascii_lowercase()).or_default().push(i);
            }
        }
        Self { paths }
    }

    /// The one entry whose path is `path`, case ignored.
    pub fn find(&self, path: &str) -> Result<usize, Miss> {
        match self
            .paths
            .get(&path.to_ascii_lowercase())
            .map(Vec::as_slice)
        {
            Some([one]) => Ok(*one),
            Some(many) if !many.is_empty() => Err(Miss::Many(many.to_vec())),
            _ => Err(Miss::None),
        }
    }

    pub fn len(&self) -> usize {
        self.paths.len()
    }

    pub fn is_empty(&self) -> bool {
        self.paths.is_empty()
    }
}

#[cfg(test)]
mod tests;
