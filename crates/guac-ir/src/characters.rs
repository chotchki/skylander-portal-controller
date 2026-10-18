//! IR v1's character table (openspec AB, design decision 6): per toy type,
//! the character archives the disc carries, the voice-over name they share
//! and the intro movie that name leads to. The first cut of the disc's own
//! roster; names, elements and portraits are AE's.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::DumpIdentity;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CharacterTable {
    pub identity: DumpIdentity,
    /// Keyed by decimal toy type, as the archive names carry it. A toy type
    /// with neither an archive nor an intro isn't here.
    pub characters: BTreeMap<u32, Character>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Character {
    /// `character/<stem>.arc` (and its `.bld`), sorted by stem. Empty for a
    /// toy type whose intro came from the name fallback or a variant's base.
    pub archives: Vec<CharacterArchive>,
    /// The dominant `vo_<name>_…` name in the archives' entry lists.
    pub vo_name: Option<String>,
    pub intro: Option<Intro>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CharacterArchive {
    /// The file stem, `016_spyro_dark`.
    pub stem: String,
    /// What the stem adds to another archive of its toy type (`dark`); `None`
    /// for a base. What a suffix means as a variant is AE's.
    pub variant: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Intro {
    /// `movies/<movie>.mov` on the disc.
    pub movie: String,
    pub source: IntroSource,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub enum IntroSource {
    /// The voice-over name, underscores stripped, is the movie's name.
    Disc,
    /// The interim catalogue's base name, lowercased to letters and digits.
    NameFallback,
    /// A variant on a toy type of its own sharing toy type `base`'s intro:
    /// its catalogue name is a `base` archive's suffix plus the base's name.
    Variant { base: u32 },
}
