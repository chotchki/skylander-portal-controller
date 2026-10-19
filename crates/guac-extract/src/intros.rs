//! The intro index (AB.6, design decision 6): which `movies/<name>.mov` is
//! each toy type's intro, read off the disc, into IR v1's character table.
//!
//! Character archives are `character/NNN_<codename>[_<variant>].arc`, NNN the
//! decimal toy type. Their entry lists carry the character's voice-over lines
//! as `…/vo_<vo_name>_<line>.wav/…`, and `<vo_name>` with its underscores
//! stripped is the intro's name. A toy type the disc doesn't map falls back
//! to the roster's English base name (AE.3, [`crate::roster::base_names`]),
//! lowercased to letters and digits. A fallback never takes a movie the disc
//! gave another toy type. A variant the roster gives a toy type of its own
//! (Dark Spyro, 28) shares its base's intro when its name is a base archive's
//! suffix plus the base's name.

use std::collections::{BTreeMap, BTreeSet};
use std::fs::{self, File};
use std::io::Read;
use std::path::{Path, PathBuf};

use guac_formats::iga::Archive;
use guac_ir::{
    Character, CharacterArchive, CharacterTable, DumpIdentity, Intro, IntroSource, IrError,
    VoiceTrack,
};
use serde::Serialize;

use crate::census::MAX_WHOLE_ARCHIVE;
use crate::error::{ExtractError, Result};
use crate::extract::{IdentityDir, OutputFile};

pub const CHARACTER_DIR: &str = "PS3_GAME/USRDIR/character";
pub const MOVIE_DIR: &str = "PS3_GAME/USRDIR/movies";

/// Every Giants intro carries one stereo track (AB.5's census), and only the
/// known dump, whose bytes the table pins, gets this far.
const GIANTS_VOICE: VoiceTrack = VoiceTrack::Only;

/// Entry names past this are skipped unread; Giants' longest is far under.
pub const MAX_ENTRY_NAME: usize = 1024;
/// A `vo_` stem past either cap isn't a voice-over line.
pub const MAX_VO_STEM: usize = 128;
pub const MAX_VO_TOKENS: usize = 16;

/// One character archive's stem and its distinct voice-over stems, counted.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArchiveListing {
    pub toy_type: u32,
    pub stem: String,
    pub vo_stems: BTreeMap<String, usize>,
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
    pub fn count(&self, source: impl Fn(IntroSource) -> bool) -> usize {
        self.table
            .characters
            .values()
            .filter(|c| c.intro.as_ref().is_some_and(|i| source(i.source)))
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
/// lowercased, within [`MAX_VO_STEM`] bytes and [`MAX_VO_TOKENS`] tokens.
pub fn vo_stem(entry: &str) -> Option<String> {
    entry.split(['/', '\\']).find_map(|seg| {
        // Before the copy: an aliased name can be megabytes.
        if seg.len() > MAX_VO_STEM + "vo_.wav".len() {
            return None;
        }
        let seg = seg.to_ascii_lowercase();
        let stem = seg.strip_prefix("vo_")?.strip_suffix(".wav")?;
        (!stem.is_empty() && stem.split('_').count() <= MAX_VO_TOKENS).then(|| stem.to_owned())
    })
}

/// Each distinct [`vo_stem`] and how many entries carry it, skipping names
/// over [`MAX_ENTRY_NAME`].
pub fn vo_stem_counts<'a>(
    entry_names: impl IntoIterator<Item = &'a str>,
) -> BTreeMap<String, usize> {
    let mut out = BTreeMap::new();
    for name in entry_names {
        if name.len() > MAX_ENTRY_NAME {
            continue;
        }
        if let Some(stem) = vo_stem(name) {
            *out.entry(stem).or_default() += 1;
        }
    }
    out
}

/// The most common first token, extended one `_` token at a time by the next
/// token more than half the stems under it carry short of their line number.
/// So `ghost_roaster` beats a minion's `ghost_eater`, `trigger_happy` a
/// misnamed `trigger_character_specific_01`, and ties go to the smaller token.
/// Stems over the caps don't vote, so the cost is bounded by distinct stems.
pub fn dominant_vo_name(stems: &BTreeMap<String, usize>) -> Option<String> {
    let stems: Vec<(Vec<&str>, usize)> = stems
        .iter()
        .filter(|(s, _)| s.len() <= MAX_VO_STEM)
        .map(|(s, &n)| (s.split('_').collect::<Vec<_>>(), n))
        .filter(|(t, _)| (2..=MAX_VO_TOKENS).contains(&t.len()) && t.iter().all(|t| !t.is_empty()))
        .collect();
    let mut first: BTreeMap<&str, usize> = BTreeMap::new();
    for (t, n) in &stems {
        *first.entry(t[0]).or_default() += n;
    }
    let (&top, _) = first
        .iter()
        .max_by(|a, b| a.1.cmp(b.1).then_with(|| b.0.cmp(a.0)))?;
    let mut prefix = vec![top];
    loop {
        let k = prefix.len();
        let mut under = 0;
        let mut next: BTreeMap<&str, usize> = BTreeMap::new();
        for (t, n) in stems.iter().filter(|(t, _)| t.starts_with(&prefix)) {
            under += n;
            // A stem whose next token is its line number votes to stop.
            if t.len() > k + 1 {
                *next.entry(t[k]).or_default() += n;
            }
        }
        match next
            .iter()
            .max_by(|a, b| a.1.cmp(b.1).then_with(|| b.0.cmp(a.0)))
        {
            Some((&n, &count)) if count * 2 > under => prefix.push(n),
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

/// The pure index over listings, the disc's movie stems and the fallback
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
        let mut vo: BTreeMap<String, usize> = BTreeMap::new();
        for (s, n) in ls.iter().flat_map(|l| &l.vo_stems) {
            *vo.entry(s.clone()).or_default() += n;
        }
        let vo_name = dominant_vo_name(&vo);
        let intro = vo_name
            .as_deref()
            .and_then(|v| movie_of.get(&slug(v)))
            .map(|&m| Intro {
                movie: m.clone(),
                source: IntroSource::Disc,
                voice: GIANTS_VOICE,
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
        characters.entry(toy).or_insert_with(no_archive).intro = Some(Intro {
            movie: m.clone(),
            source: IntroSource::NameFallback,
            voice: GIANTS_VOICE,
        });
    }

    // A variant on a toy type of its own: `dark` + `Spyro` is Dark Spyro. It
    // shares the base's movie, so it isn't a claim.
    let mut variants = Vec::new();
    for (&toy, name) in fallback {
        if characters.get(&toy).is_some_and(|c| c.intro.is_some()) {
            continue;
        }
        let want = slug(name);
        let bases: Vec<(u32, &String)> = characters
            .iter()
            .filter_map(|(&base, c)| {
                let intro = c.intro.as_ref()?;
                let names = [
                    fallback.get(&base).map(|n| slug(n)),
                    Some(slug(&intro.movie)),
                ];
                let hit = c
                    .archives
                    .iter()
                    .filter_map(|a| a.variant.as_deref())
                    .any(|v| {
                        let v = slug(v);
                        names.iter().flatten().any(|n| want == format!("{v}{n}"))
                    });
                hit.then_some((base, &intro.movie))
            })
            .collect();
        // Two bases spelling one name: neither.
        if let [(base, movie)] = bases[..] {
            variants.push((toy, base, movie.clone()));
        }
    }
    for (toy, base, movie) in variants {
        characters.entry(toy).or_insert_with(no_archive).intro = Some(Intro {
            movie,
            source: IntroSource::Variant { base },
            voice: GIANTS_VOICE,
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

fn no_archive() -> Character {
    Character {
        archives: Vec::new(),
        vo_name: None,
        intro: None,
    }
}

/// Every `character/NNN_*.arc`'s voice-over stems, one archive in memory at a
/// time and each under the census's whole-read cap, and the stems of
/// `movies/*.mov`. Read-only.
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
            .and_then(|f| f.take(MAX_WHOLE_ARCHIVE + 1).read_to_end(&mut buf))
            .map_err(ExtractError::io(&path))?;
        if buf.len() as u64 > MAX_WHOLE_ARCHIVE {
            return Err(ExtractError::unreadable(
                &path,
                format!("over the {MAX_WHOLE_ARCHIVE}-byte whole-read cap"),
            ));
        }
        let archive = Archive::parse(&buf).map_err(|e| ExtractError::unreadable(&path, e))?;
        listings.push(ArchiveListing {
            toy_type,
            stem: stem.to_owned(),
            vo_stems: vo_stem_counts(archive.entries().iter().map(|e| e.name())),
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

/// Indexes the dump `open` identified, from the identity dir's own record, so
/// the identity is the one it hashed. Fails on an unreadable character or
/// movie folder, or an archive that won't parse or is over the cap; never on
/// a toy type without an intro.
pub fn index(dir: &IdentityDir, fallback: &BTreeMap<u32, String>) -> Result<IntroIndex> {
    let manifest = dir.manifest();
    let dump = PathBuf::from(&manifest.source_dir);
    let (listings, movies) = read_dump(&dump)?;
    Ok(build(
        manifest.identity.clone(),
        &listings,
        &movies,
        fallback,
    ))
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

    /// The roster's file (AE.3).
    pub fn roster_path(self) -> &'static str {
        match self {
            Self::Ron => "ir/roster.ron",
            Self::Postcard => "ir/roster.postcard",
        }
    }

    /// `body` in this format, inside the IR's versioned envelope. A failure
    /// is reported against `rel` in `dir`.
    pub fn encode<T: Serialize>(self, body: &T, dir: &IdentityDir, rel: &str) -> Result<Vec<u8>> {
        match self {
            Self::Ron => guac_ir::to_ron(body).map(String::into_bytes),
            Self::Postcard => guac_ir::to_postcard(body),
        }
        .map_err(|e: IrError| ExtractError::Io {
            path: dir.path().join(rel),
            source: std::io::Error::other(e),
        })
    }
}

/// Writes the table to `ir/characters.<ron|postcard>` through the identity
/// dir (`.partial`, then renamed and recorded in the manifest).
pub fn write(
    dir: &mut IdentityDir,
    table: &CharacterTable,
    format: IrFormat,
) -> Result<OutputFile> {
    let bytes = format.encode(table, dir, format.characters_path())?;
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
            vo_stems: counts(&entry_names),
        }
    }

    fn counts(entry_names: &[String]) -> BTreeMap<String, usize> {
        vo_stem_counts(entry_names.iter().map(String::as_str))
    }

    fn stems(s: &[&str]) -> BTreeMap<String, usize> {
        s.iter().map(|&s| (s.to_owned(), 1)).collect()
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
        // Over a cap: no stem, and the next segment still counts.
        let long = format!("vo_{}_01.wav", "a".repeat(MAX_VO_STEM));
        assert_eq!(vo_stem(&long), None);
        let many = format!("vo_{}01.wav", "a_".repeat(MAX_VO_TOKENS));
        assert_eq!(vo_stem(&many), None);
        assert_eq!(
            vo_stem(&format!("{long}/vo_zap_01.wav")),
            Some("zap_01".into())
        );
    }

    #[test]
    fn aliased_names_count_once_and_long_ones_are_skipped() {
        // A crafted archive's entries can all share one name.
        let name = "c:/a/vo_zap_hat_01.wav".to_owned();
        let huge = format!("c:/{}/vo_zap_hat_02.wav", "x".repeat(MAX_ENTRY_NAME));
        let names = vec![name; 10_000];
        let c = vo_stem_counts(names.iter().map(String::as_str).chain([huge.as_str()]));
        assert_eq!(c, BTreeMap::from([("zap_hat_01".to_owned(), 10_000)]));
        // The prefix walk stops at the token cap, however the map was built.
        let deep = format!("{}01", "a_".repeat(MAX_VO_TOKENS * 4));
        assert_eq!(
            dominant_vo_name(&stems(&[&deep, "zap_01"])),
            Some("zap".into())
        );
    }

    #[test]
    fn the_dominant_vo_name_wins_over_a_stray_one() {
        let mut names = vo("warnado", "warnado", LINES);
        names.extend(vo("skyturtle", "sky_turtle", &["dizzy_03"]));
        assert_eq!(dominant_vo_name(&counts(&names)), Some("warnado".into()));
    }

    #[test]
    fn a_stray_sharing_the_first_token_loses_too() {
        let mut names = vo("ghost_roaster", "ghost_roaster", LINES);
        names.extend(vo("ghosteater", "ghost_eater", &["victory_01"]));
        assert_eq!(
            dominant_vo_name(&counts(&names)),
            Some("ghost_roaster".into())
        );

        let mut names = vo("trigger_happy", "trigger_happy", LINES);
        names.extend(vo("trigger_happy", "trigger", &["character_specific_01"]));
        assert_eq!(
            dominant_vo_name(&counts(&names)),
            Some("trigger_happy".into())
        );
    }

    #[test]
    fn a_multi_token_name_extends_until_the_lines_differ() {
        let chop = counts(&vo("chop_chop", "chop_chop", LINES));
        assert_eq!(dominant_vo_name(&chop), Some("chop_chop".into()));
        // One line type throughout: the line number still stops it.
        assert_eq!(
            dominant_vo_name(&stems(&["zap_hat_01", "zap_hat_02"])),
            Some("zap_hat".into())
        );
        assert_eq!(dominant_vo_name(&stems(&["zap_01"])), Some("zap".into()));
        assert_eq!(dominant_vo_name(&stems(&["zap", ""])), None);
        assert_eq!(dominant_vo_name(&BTreeMap::new()), None);
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
                source: IntroSource::Disc,
                voice: VoiceTrack::Only,
            })
        );
        assert_eq!(idx.unclaimed_movies, ["fmv01"]);
        assert_eq!(idx.count(|s| s == IntroSource::Disc), 1);
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
            (16, "Spyro".to_owned()),
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
                source: IntroSource::NameFallback,
                voice: VoiceTrack::Only,
            })
        );
        // No archive of its own number: the name alone.
        let jet = &idx.table.characters[&100];
        assert!(jet.archives.is_empty());
        assert_eq!(
            jet.intro.as_ref().unwrap().source,
            IntroSource::NameFallback
        );
        // Nor is a bare base name a variant: no suffix spells it.
        assert!(!idx.table.characters.contains_key(&28));
        assert_eq!(idx.count(|s| s == IntroSource::Disc), 1);
        assert_eq!(idx.count(|s| s == IntroSource::NameFallback), 2);
        assert!(idx.claimed_twice.is_empty());
    }

    #[test]
    fn a_variant_on_its_own_toy_type_shares_the_base_intro() {
        let spyro = vo("spyro", "spyro", LINES);
        let bash = vo("bash", "bash", LINES);
        let fallback = BTreeMap::from([
            (16, "Spyro".to_owned()),
            (28, "Dark Spyro".to_owned()),
            (416, "Legendary Spyro".to_owned()),
            (4, "Bash".to_owned()),
            (404, "Legendary Bash".to_owned()),
            // No `_red` archive of Bash's: no intro.
            (405, "Red Bash".to_owned()),
        ]);
        let idx = build(
            identity(),
            &[
                listing(16, "016_spyro", spyro.clone()),
                listing(16, "016_spyro_dark", spyro.clone()),
                listing(16, "016_spyro_legendary", spyro),
                listing(4, "004_rock_dragon", bash.clone()),
                listing(4, "004_rock_dragon_legendary", bash),
            ],
            &movies(&["spyro", "bash", "fmv01"]),
            &fallback,
        );
        let c = &idx.table.characters;
        let variant = |base| {
            Some(Intro {
                movie: if base == 16 { "spyro" } else { "bash" }.into(),
                source: IntroSource::Variant { base },
                voice: VoiceTrack::Only,
            })
        };
        assert_eq!(c[&28].intro, variant(16));
        assert_eq!(c[&416].intro, variant(16));
        assert_eq!(c[&404].intro, variant(4));
        assert!(c[&28].archives.is_empty() && c[&28].vo_name.is_none());
        assert!(!c.contains_key(&405));
        // Shared, not claimed: one claimant each, fmv01 still unclaimed.
        assert!(idx.claimed_twice.is_empty());
        assert_eq!(idx.unclaimed_movies, ["fmv01"]);
        assert_eq!(idx.count(|s| s == IntroSource::Disc), 2);
        assert_eq!(idx.count(|s| matches!(s, IntroSource::Variant { .. })), 3);
    }

    #[test]
    fn a_variant_name_two_bases_spell_gets_neither() {
        let fallback = BTreeMap::from([
            (1, "Zap".to_owned()),
            (2, "Zap".to_owned()),
            (3, "Dark Zap".to_owned()),
        ]);
        let idx = build(
            identity(),
            &[
                listing(1, "001_zap", vo("zap", "zap", LINES)),
                listing(1, "001_zap_dark", vec![]),
                listing(2, "002_eel", vo("zap2", "zap2", LINES)),
                listing(2, "002_eel_dark", vec![]),
            ],
            &movies(&["zap", "zap2"]),
            &fallback,
        );
        assert!(!idx.table.characters.contains_key(&3));
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
        // A fallback name with no archive and no movie isn't in the table.
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
