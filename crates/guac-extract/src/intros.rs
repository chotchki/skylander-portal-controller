//! The intro index (AB.6, design decision 6): which `movies/<name>.mov` is
//! each toy type's intro, read off the disc, into IR v1's character table.
//!
//! Character archives are `character/NNN_<codename>[_<variant>].arc`, NNN the
//! decimal toy type. Their entry lists carry the character's voice-over lines
//! as `…/vo_<vo_name>_<line>.wav/…`, and `<vo_name>` with its underscores
//! stripped is the intro's name. A toy type the disc doesn't map falls back
//! to the interim catalogue's base name, lowercased to letters and digits. A
//! fallback never takes a movie the disc gave another toy type.

use std::collections::{BTreeMap, BTreeSet};
use std::fs::{self, File};
use std::io::Read;
use std::path::Path;

use guac_formats::iga::Archive;
use guac_ir::{
    Character, CharacterArchive, CharacterTable, DumpIdentity, Intro, IntroSource, IrError,
};
use serde::Serialize;

use crate::error::{ExtractError, Result};
use crate::extract::{IdentityDir, OutputFile};

pub const CHARACTER_DIR: &str = "PS3_GAME/USRDIR/character";
pub const MOVIE_DIR: &str = "PS3_GAME/USRDIR/movies";

/// One character archive's stem and the entry names that matter here.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArchiveListing {
    pub toy_type: u32,
    pub stem: String,
    pub entry_names: Vec<String>,
}

/// The table plus what the disc test and the CLI report about it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IntroIndex {
    pub table: CharacterTable,
    /// Movies no toy type claimed (FMVs and logos on Giants).
    pub unclaimed_movies: Vec<String>,
    /// Movies two or more toy types claimed, with the claimants.
    pub claimed_twice: BTreeMap<String, Vec<u32>>,
}

impl IntroIndex {
    pub fn count(&self, source: IntroSource) -> usize {
        self.table
            .characters
            .values()
            .filter(|c| c.intro.as_ref().is_some_and(|i| i.source == source))
            .count()
    }
}

/// `009_eruptor.arc` → `(9, "009_eruptor")`; anything not `<digits>_<rest>.arc`
/// (the card-game archives, say) is `None`.
pub fn parse_archive_file(name: &str) -> Option<(u32, &str)> {
    let stem = name.strip_suffix(".arc")?;
    let (digits, rest) = stem.split_once('_')?;
    if digits.is_empty() || rest.is_empty() || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    Some((digits.parse().ok()?, stem))
}

/// The `<vo_name>_<line>` of an entry with a `vo_….wav` path segment,
/// lowercased.
pub fn vo_stem(entry: &str) -> Option<String> {
    entry.split(['/', '\\']).find_map(|seg| {
        let seg = seg.to_ascii_lowercase();
        let stem = seg.strip_prefix("vo_")?.strip_suffix(".wav")?;
        (!stem.is_empty()).then(|| stem.to_owned())
    })
}

/// The most common first token, extended one `_` token at a time by the next
/// token more than half the stems under it carry short of their line number.
/// So `ghost_roaster` beats a minion's `ghost_eater`, `trigger_happy` a
/// misnamed `trigger_character_specific_01`, and ties go to the smaller token.
pub fn dominant_vo_name<S: AsRef<str>>(stems: impl IntoIterator<Item = S>) -> Option<String> {
    let stems: Vec<Vec<String>> = stems
        .into_iter()
        .map(|s| s.as_ref().split('_').map(str::to_owned).collect())
        .filter(|t: &Vec<String>| t.len() >= 2 && t.iter().all(|t| !t.is_empty()))
        .collect();
    let mut first: BTreeMap<&str, usize> = BTreeMap::new();
    for t in &stems {
        *first.entry(&t[0]).or_default() += 1;
    }
    let (&top, _) = first
        .iter()
        .max_by(|a, b| a.1.cmp(b.1).then_with(|| b.0.cmp(a.0)))?;
    let mut prefix = vec![top.to_owned()];
    loop {
        let k = prefix.len();
        let under: Vec<&Vec<String>> = stems.iter().filter(|t| t.starts_with(&prefix)).collect();
        let mut next: BTreeMap<&str, usize> = BTreeMap::new();
        // A stem whose next token is its line number votes to stop.
        for t in under.iter().filter(|t| t.len() > k + 1) {
            *next.entry(&t[k]).or_default() += 1;
        }
        match next
            .iter()
            .max_by(|a, b| a.1.cmp(b.1).then_with(|| b.0.cmp(a.0)))
        {
            Some((&n, &count)) if count * 2 > under.len() => prefix.push(n.to_owned()),
            _ => return Some(prefix.join("_")),
        }
    }
}

/// Lowercase, letters and digits only: `Dino-Rang` → `dinorang`.
pub fn slug(name: &str) -> String {
    name.chars()
        .filter(char::is_ascii_alphanumeric)
        .map(|c| c.to_ascii_lowercase())
        .collect()
}

/// Each stem with what it adds to the longest other stem of its toy type it
/// extends (`016_spyro_dark` over `016_spyro` → `dark`), sorted by stem.
pub fn archives(stems: &[&str]) -> Vec<CharacterArchive> {
    let mut out: Vec<CharacterArchive> = stems
        .iter()
        .map(|&stem| {
            let variant = stems
                .iter()
                .filter(|&&base| base != stem)
                .filter_map(|&base| stem.strip_prefix(base)?.strip_prefix('_'))
                .filter(|rest| !rest.is_empty())
                .min_by_key(|rest| rest.len())
                .map(str::to_owned);
            CharacterArchive {
                stem: stem.to_owned(),
                variant,
            }
        })
        .collect();
    out.sort_by(|a, b| a.stem.cmp(&b.stem));
    out.dedup_by(|a, b| a.stem == b.stem);
    out
}

/// The pure index over listings, the disc's movie stems and the catalogue's
/// base names by toy type.
pub fn build(
    identity: DumpIdentity,
    listings: &[ArchiveListing],
    movies: &BTreeSet<String>,
    fallback: &BTreeMap<u32, String>,
) -> IntroIndex {
    let movie_of: BTreeMap<String, &String> = movies.iter().map(|m| (slug(m), m)).collect();
    let mut by_toy: BTreeMap<u32, Vec<&ArchiveListing>> = BTreeMap::new();
    for l in listings {
        by_toy.entry(l.toy_type).or_default().push(l);
    }

    let mut characters = BTreeMap::new();
    let mut claims: BTreeMap<String, Vec<u32>> = BTreeMap::new();
    for (&toy, ls) in &by_toy {
        let stems: Vec<&str> = ls.iter().map(|l| l.stem.as_str()).collect();
        let vo_name = dominant_vo_name(
            ls.iter()
                .flat_map(|l| l.entry_names.iter().filter_map(|e| vo_stem(e))),
        );
        let intro = vo_name
            .as_deref()
            .and_then(|v| movie_of.get(&slug(v)))
            .map(|&m| Intro {
                movie: m.clone(),
                source: IntroSource::Disc,
            });
        if let Some(i) = &intro {
            claims.entry(i.movie.clone()).or_default().push(toy);
        }
        characters.insert(
            toy,
            Character {
                archives: archives(&stems),
                vo_name,
                intro,
            },
        );
    }

    // The fallback, only onto movies the disc left unclaimed.
    let disc_claimed: BTreeSet<String> = claims.keys().cloned().collect();
    for (&toy, name) in fallback {
        if characters.get(&toy).is_some_and(|c| c.intro.is_some()) {
            continue;
        }
        let Some(&m) = movie_of.get(&slug(name)) else {
            continue;
        };
        if disc_claimed.contains(m) {
            continue;
        }
        claims.entry(m.clone()).or_default().push(toy);
        let c = characters.entry(toy).or_insert_with(|| Character {
            archives: Vec::new(),
            vo_name: None,
            intro: None,
        });
        c.intro = Some(Intro {
            movie: m.clone(),
            source: IntroSource::NameFallback,
        });
    }

    let unclaimed_movies = movies
        .iter()
        .filter(|m| !claims.contains_key(*m))
        .cloned()
        .collect();
    claims.retain(|_, toys| toys.len() > 1);
    IntroIndex {
        table: CharacterTable {
            identity,
            characters,
        },
        unclaimed_movies,
        claimed_twice: claims,
    }
}

/// Every `character/NNN_*.arc`'s voice-over entry names, one archive in
/// memory at a time, and the stems of `movies/*.mov`. Read-only.
pub fn read_dump(dump: &Path) -> Result<(Vec<ArchiveListing>, BTreeSet<String>)> {
    let char_dir = dump.join(CHARACTER_DIR);
    let mut listings = Vec::new();
    let mut buf = Vec::new();
    for name in sorted_names(&char_dir)? {
        let Some((toy_type, stem)) = parse_archive_file(&name) else {
            continue;
        };
        let path = char_dir.join(&name);
        buf.clear();
        File::open(&path)
            .and_then(|mut f| f.read_to_end(&mut buf))
            .map_err(ExtractError::io(&path))?;
        let archive = Archive::parse(&buf).map_err(|e| ExtractError::unreadable(&path, e))?;
        let entry_names = archive
            .entries()
            .iter()
            .map(|e| e.name())
            .filter(|n| vo_stem(n).is_some())
            .map(str::to_owned)
            .collect();
        listings.push(ArchiveListing {
            toy_type,
            stem: stem.to_owned(),
            entry_names,
        });
    }
    let movies = sorted_names(&dump.join(MOVIE_DIR))?
        .into_iter()
        .filter_map(|n| n.strip_suffix(".mov").map(str::to_owned))
        .collect();
    Ok((listings, movies))
}

fn sorted_names(dir: &Path) -> Result<Vec<String>> {
    let mut names = Vec::new();
    for e in fs::read_dir(dir).map_err(ExtractError::io(dir))? {
        let e = e.map_err(ExtractError::io(dir))?;
        if e.file_type().map_err(ExtractError::io(e.path()))?.is_file() {
            let name = e.file_name();
            let name = name
                .to_str()
                .ok_or_else(|| ExtractError::unreadable(e.path(), "the name isn't UTF-8"))?;
            names.push(name.to_owned());
        }
    }
    names.sort();
    Ok(names)
}

/// Fails on an unreadable character or movie folder, or an archive that
/// won't parse; never on a toy type without an intro.
pub fn index(
    dump: &Path,
    identity: DumpIdentity,
    fallback: &BTreeMap<u32, String>,
) -> Result<IntroIndex> {
    let (listings, movies) = read_dump(dump)?;
    Ok(build(identity, &listings, &movies, fallback))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum IrFormat {
    /// Dev: diffs by eye.
    Ron,
    /// An install.
    Postcard,
}

impl IrFormat {
    pub fn characters_path(self) -> &'static str {
        match self {
            Self::Ron => "ir/characters.ron",
            Self::Postcard => "ir/characters.postcard",
        }
    }
}

/// Writes the table to `ir/characters.<ron|postcard>` through the identity
/// dir (`.partial`, then renamed and recorded in the manifest).
pub fn write(
    dir: &mut IdentityDir,
    table: &CharacterTable,
    format: IrFormat,
) -> Result<OutputFile> {
    let bytes = match format {
        IrFormat::Ron => guac_ir::to_ron(table).map(String::into_bytes),
        IrFormat::Postcard => guac_ir::to_postcard(table),
    }
    .map_err(|e: IrError| ExtractError::Io {
        path: dir.path().join(format.characters_path()),
        source: std::io::Error::other(e),
    })?;
    dir.write_output(format.characters_path(), &bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn identity() -> DumpIdentity {
        DumpIdentity {
            title_id: "BLUS30968".into(),
            app_ver: "01.00".into(),
            manifest_hash: guac_ir::Sha256([7; 32]),
        }
    }

    fn vo(folder: &str, name: &str, lines: &[&str]) -> Vec<String> {
        let mut out = Vec::new();
        for lang in ["", "french/", "german/"] {
            for line in lines {
                out.push(format!(
                    "c:/tfb/build/ps3/levels/champion_audio/{folder}/{lang}vo_{name}_{line}.wav/0x1.wav.hz.wav.enc"
                ));
            }
        }
        out
    }

    fn listing(toy_type: u32, stem: &str, entry_names: Vec<String>) -> ArchiveListing {
        ArchiveListing {
            toy_type,
            stem: stem.into(),
            entry_names,
        }
    }

    fn movies(names: &[&str]) -> BTreeSet<String> {
        names.iter().map(|&n| n.to_owned()).collect()
    }

    const LINES: &[&str] = &["hat_01", "level_up_02", "bounce_pad_03", "victory_01"];

    #[test]
    fn archive_file_names_parse() {
        assert_eq!(
            parse_archive_file("009_eruptor.arc"),
            Some((9, "009_eruptor"))
        );
        assert_eq!(
            parse_archive_file("113_sniper.arc"),
            Some((113, "113_sniper"))
        );
        assert_eq!(parse_archive_file("009_eruptor.bld"), None);
        assert_eq!(parse_archive_file("cardgame_streaming.arc"), None);
        assert_eq!(parse_archive_file("009_.arc"), None);
        assert_eq!(parse_archive_file("_x.arc"), None);
    }

    #[test]
    fn the_vo_stem_is_the_wav_segment() {
        assert_eq!(
            vo_stem("c:/a/sonic_boom/french/vo_sonic_boom_hat_01.wav/0x1.wav.hz.wav.enc"),
            Some("sonic_boom_hat_01".into())
        );
        assert_eq!(
            vo_stem("c:\\a\\VO_Zap_Hat_01.WAV"),
            Some("zap_hat_01".into())
        );
        assert_eq!(vo_stem("c:/a/zap/hat_01.wav"), None);
        assert_eq!(vo_stem("c:/a/vo_.wav"), None);
    }

    #[test]
    fn the_dominant_vo_name_wins_over_a_stray_one() {
        let mut names = vo("warnado", "warnado", LINES);
        names.extend(vo("skyturtle", "sky_turtle", &["dizzy_03"]));
        let stems: Vec<String> = names.iter().filter_map(|n| vo_stem(n)).collect();
        assert_eq!(dominant_vo_name(&stems), Some("warnado".into()));
    }

    #[test]
    fn a_stray_sharing_the_first_token_loses_too() {
        let mut names = vo("ghost_roaster", "ghost_roaster", LINES);
        names.extend(vo("ghosteater", "ghost_eater", &["victory_01"]));
        let stems: Vec<String> = names.iter().filter_map(|n| vo_stem(n)).collect();
        assert_eq!(dominant_vo_name(&stems), Some("ghost_roaster".into()));

        let mut names = vo("trigger_happy", "trigger_happy", LINES);
        names.extend(vo("trigger_happy", "trigger", &["character_specific_01"]));
        let stems: Vec<String> = names.iter().filter_map(|n| vo_stem(n)).collect();
        assert_eq!(dominant_vo_name(&stems), Some("trigger_happy".into()));
    }

    #[test]
    fn a_multi_token_name_extends_until_the_lines_differ() {
        let stems: Vec<String> = vo("chop_chop", "chop_chop", LINES)
            .iter()
            .filter_map(|n| vo_stem(n))
            .collect();
        assert_eq!(dominant_vo_name(&stems), Some("chop_chop".into()));
        // One line type throughout: the line number still stops it.
        assert_eq!(
            dominant_vo_name(["zap_hat_01", "zap_hat_02"]),
            Some("zap_hat".into())
        );
        assert_eq!(dominant_vo_name(["zap_01"]), Some("zap".into()));
        assert_eq!(dominant_vo_name(["zap", ""]), None);
        assert_eq!(dominant_vo_name(Vec::<String>::new()), None);
    }

    #[test]
    fn underscores_strip_to_the_movie_name() {
        let idx = build(
            identity(),
            &[listing(
                1,
                "001_gryphon",
                vo("sonic_boom", "sonic_boom", LINES),
            )],
            &movies(&["sonicboom", "fmv01"]),
            &BTreeMap::new(),
        );
        let c = &idx.table.characters[&1];
        assert_eq!(c.vo_name.as_deref(), Some("sonic_boom"));
        assert_eq!(
            c.intro,
            Some(Intro {
                movie: "sonicboom".into(),
                source: IntroSource::Disc
            })
        );
        assert_eq!(idx.unclaimed_movies, ["fmv01"]);
        assert_eq!(idx.count(IntroSource::Disc), 1);
    }

    #[test]
    fn a_variant_suffix_is_kept_with_its_archive() {
        let spyro = vo("spyro", "spyro", LINES);
        let idx = build(
            identity(),
            &[
                listing(16, "016_spyro_legendary", spyro.clone()),
                listing(16, "016_spyro", spyro.clone()),
                listing(16, "016_spyro_dark", spyro),
                listing(4, "004_rock_dragon", vo("bash", "bash", LINES)),
                listing(113, "113_myconman", vo("shroom_boom", "shroomboom", LINES)),
                listing(113, "113_sniper", vec![]),
            ],
            &movies(&["spyro", "bash", "shroomboom"]),
            &BTreeMap::new(),
        );
        let a = |stem: &str, v: Option<&str>| CharacterArchive {
            stem: stem.into(),
            variant: v.map(str::to_owned),
        };
        assert_eq!(
            idx.table.characters[&16].archives,
            [
                a("016_spyro", None),
                a("016_spyro_dark", Some("dark")),
                a("016_spyro_legendary", Some("legendary")),
            ]
        );
        // Underscores in a lone codename aren't a variant.
        assert_eq!(
            idx.table.characters[&4].archives,
            [a("004_rock_dragon", None)]
        );
        // Two codenames on one toy type: two bases, one voice.
        let shroom = &idx.table.characters[&113];
        assert_eq!(
            shroom.archives,
            [a("113_myconman", None), a("113_sniper", None)]
        );
        assert_eq!(shroom.intro.as_ref().unwrap().movie, "shroomboom");
        assert!(idx.unclaimed_movies.is_empty());
    }

    #[test]
    fn a_miss_falls_back_to_the_name_slug() {
        let fallback = BTreeMap::from([
            (6, "Dino-Rang".to_owned()),
            (100, "Jet-Vac".to_owned()),
            // The disc already gave spyro to 16: a fallback can't take it.
            (28, "Spyro".to_owned()),
        ]);
        let idx = build(
            identity(),
            &[
                listing(6, "006_dinorang", vo("dino", "rang_dino", LINES)),
                listing(16, "016_spyro", vo("spyro", "spyro", LINES)),
            ],
            &movies(&["dinorang", "jetvac", "spyro"]),
            &fallback,
        );
        let dino = &idx.table.characters[&6];
        assert_eq!(dino.vo_name.as_deref(), Some("rang_dino"));
        assert_eq!(
            dino.intro,
            Some(Intro {
                movie: "dinorang".into(),
                source: IntroSource::NameFallback
            })
        );
        // No archive of its own number: the name alone.
        let jet = &idx.table.characters[&100];
        assert!(jet.archives.is_empty());
        assert_eq!(
            jet.intro.as_ref().unwrap().source,
            IntroSource::NameFallback
        );
        assert!(!idx.table.characters.contains_key(&28));
        assert_eq!(idx.count(IntroSource::Disc), 1);
        assert_eq!(idx.count(IntroSource::NameFallback), 2);
        assert!(idx.claimed_twice.is_empty());
    }

    #[test]
    fn a_toy_type_with_neither_has_no_intro() {
        let fallback = BTreeMap::from([
            (113, "Shroomboom".to_owned()),
            (200, "Sky-Iron Shield".to_owned()),
        ]);
        let idx = build(
            identity(),
            &[listing(113, "113_sniper", vec!["c:/a/model.igz".into()])],
            &movies(&["fmv01"]),
            &fallback,
        );
        let c = &idx.table.characters[&113];
        assert_eq!(c.vo_name, None);
        assert_eq!(c.intro, None);
        // A catalogue name with no archive and no movie isn't in the table.
        assert!(!idx.table.characters.contains_key(&200));
        assert_eq!(idx.unclaimed_movies, ["fmv01"]);
    }

    #[test]
    fn two_toy_types_on_one_movie_are_reported() {
        let idx = build(
            identity(),
            &[
                listing(9, "009_eruptor", vo("eruptor", "eruptor", LINES)),
                listing(99, "099_copy", vo("eruptor", "eruptor", LINES)),
            ],
            &movies(&["eruptor"]),
            &BTreeMap::new(),
        );
        assert_eq!(
            idx.claimed_twice,
            BTreeMap::from([("eruptor".to_owned(), vec![9, 99])])
        );
    }

    #[test]
    fn slugs_keep_letters_and_digits() {
        assert_eq!(slug("Dino-Rang"), "dinorang");
        assert_eq!(slug("Eye-Brawl"), "eyebrawl");
        assert_eq!(slug("Pop Fizz"), "popfizz");
        assert_eq!(slug("Sky-Iron Shield"), "skyironshield");
    }
}
