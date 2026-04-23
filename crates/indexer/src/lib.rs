//! Firmware pack indexer.
//!
//! Walks the Skylanders firmware pack and produces a typed `Vec<Figure>`.
//! Ported from the Phase 1c spike (`tools/inventory`) with the classification
//! rules preserved; changes vs. spike:
//!  - `"vehicle"` and `"trap"` are first-class categories (not bucketed as other/item).
//!  - Element-icon paths are captured per figure when present in the pack.
//!  - Output uses the typed enums from `skylander-core`.
//!
//! Stable IDs are SHA-256 of `"<game>|<element>|<relative_path>"` truncated
//! to 16 hex chars (same scheme as the spike).

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use anyhow::Result;
use sha2::{Digest, Sha256};
use skylander_core::{Category, Element, Figure, FigureId, GameOfOrigin};
use walkdir::WalkDir;

/// Scan a firmware pack root directory and return every `.sky` entry, typed.
pub fn scan(pack_root: &Path) -> Result<Vec<Figure>> {
    let (sky_files, element_icons) = walk(pack_root)?;

    let mut out: Vec<Figure> = Vec::with_capacity(sky_files.len());
    for sky in sky_files {
        let rel_str = sky.rel.replace('\\', "/");
        let segs: Vec<&str> = rel_str.split('/').collect();
        let file_name = sky.file_name.clone();

        let (game, element, category) = classify(&segs, &file_name);
        let stem = file_name
            .strip_suffix(".sky")
            .unwrap_or(&file_name)
            .to_string();
        let stem_clean = stem.strip_suffix(".key").unwrap_or(&stem).to_string();
        let (variant_group, variant_tag) = derive_group_and_tag(&segs, &stem_clean, category);

        let id = stable_id(game, element, &rel_str);

        let element_icon_path = element.and_then(|el| {
            let key = element_icon_key(&segs, el);
            element_icons.get(&key).cloned()
        });

        out.push(Figure {
            id,
            canonical_name: display_name(&stem_clean),
            variant_group,
            variant_tag,
            game,
            element,
            category,
            sky_path: sky.abs,
            element_icon_path,
        });
    }

    out.sort_by(|a, b| a.canonical_name.cmp(&b.canonical_name));
    Ok(out)
}

// ---------------- runtime (scanned) walk ----------------

/// Walk `scanned_dir` (`<data_root>/scanned/`), parse each `<uid>.sky` for
/// its tag-level identity, and emit [`Figure`] entries that can live
/// alongside pack figures in the library. PLAN 6.5.5a.
///
/// Caller is expected to merge with pack results and dedupe: when the same
/// `(figure_id, variant)` shows up in both, **pack wins** (per the UX rule
/// — pack .sky files are reset-to-fresh masters, whereas scanned files
/// carry the physical tag's current state, which isn't what a new profile
/// wants to fork from). That dedup is the consumer's responsibility; this
/// function is purely "what's in the scanned dir".
///
/// Missing directory is not an error — returns an empty vec. Files that
/// fail to read or parse are logged and skipped; one bad dump doesn't
/// torch the whole index.
pub fn scan_runtime(scanned_dir: &Path) -> Result<Vec<Figure>> {
    if !scanned_dir.exists() {
        return Ok(Vec::new());
    }
    let mut out: Vec<Figure> = Vec::new();
    for dent in WalkDir::new(scanned_dir)
        .max_depth(1)
        .into_iter()
        .filter_map(Result::ok)
    {
        if !dent.file_type().is_file() {
            continue;
        }
        let abs = dent.path().to_path_buf();
        let file_name = dent.file_name().to_string_lossy().to_string();
        if !file_name.to_lowercase().ends_with(".sky") {
            continue;
        }

        let bytes = match std::fs::read(&abs) {
            Ok(b) => b,
            Err(e) => {
                tracing::warn!(path = %abs.display(), error = %e, "scan_runtime: read failed");
                continue;
            }
        };
        let stats = match skylander_sky_parser::parse(&bytes) {
            Ok(s) => s,
            Err(e) => {
                tracing::warn!(path = %abs.display(), error = ?e, "scan_runtime: parse failed");
                continue;
            }
        };

        let uid_stem = file_name
            .strip_suffix(".sky")
            .unwrap_or(&file_name)
            .to_string();
        let id = FigureId::new(format!("scan:{}", uid_stem));
        let canonical_name = if stats.nickname.trim().is_empty() {
            format!("Figure 0x{:06X}", stats.figure_id)
        } else {
            stats.nickname.clone()
        };
        let category = figure_kind_to_category(stats.figure_kind);

        out.push(Figure {
            id,
            canonical_name: canonical_name.clone(),
            variant_group: canonical_name,
            variant_tag: "base".to_string(),
            game: GameOfOrigin::Unknown,
            element: None,
            category,
            sky_path: abs,
            element_icon_path: None,
        });
    }
    Ok(out)
}

/// Best-effort category inference from the sky-parser's `FigureKind`.
/// Scan-only figures don't come from a folder tree, so we can't derive
/// category from path — the tag's figure_id range is all we have.
fn figure_kind_to_category(kind: skylander_sky_parser::FigureKind) -> Category {
    use skylander_sky_parser::FigureKind as K;
    match kind {
        K::Standard | K::Other => Category::Figure,
        K::Trap => Category::Trap,
        K::Vehicle => Category::Vehicle,
        K::RacingPack => Category::Other,
        K::Cyos => Category::CreationCrystal,
    }
}

// ---------------- walk ----------------

struct SkyFile {
    abs: PathBuf,
    rel: String,
    file_name: String,
}

fn walk(pack_root: &Path) -> Result<(Vec<SkyFile>, HashMap<String, PathBuf>)> {
    let mut sky_files = Vec::new();
    let mut element_icons: HashMap<String, PathBuf> = HashMap::new();

    for dent in WalkDir::new(pack_root).into_iter().filter_map(Result::ok) {
        if !dent.file_type().is_file() {
            continue;
        }
        let abs = dent.path().to_path_buf();
        let rel = abs
            .strip_prefix(pack_root)
            .unwrap_or(&abs)
            .to_string_lossy()
            .to_string();
        let rel_slash = rel.replace('\\', "/");
        let file_name = dent.file_name().to_string_lossy().to_string();
        let lower = file_name.to_lowercase();

        if lower.ends_with("symbolskylanders.png") {
            // Capture element icon; keyed by parent directory.
            let segs: Vec<&str> = rel_slash.split('/').collect();
            if segs.len() >= 2 {
                let parent = segs[..segs.len() - 1].join("/");
                element_icons.insert(parent, abs.clone());
            }
            continue;
        }
        if lower == "desktop.ini"
            || lower == "poster.png"
            || lower == "poster.jpg"
            || lower == "poster.jpeg"
            || lower.ends_with(".txt")
            || lower == "skylanders-giants-logo.png"
        {
            continue;
        }
        if !lower.ends_with(".sky") {
            continue;
        }
        // Top-level `Sidekicks/` is a known duplicate — ignore.
        let segs: Vec<&str> = rel_slash.split('/').collect();
        if segs.first().copied() == Some("Sidekicks") {
            continue;
        }
        sky_files.push(SkyFile {
            abs,
            rel: rel_slash,
            file_name,
        });
    }

    Ok((sky_files, element_icons))
}

// Key a `.sky` file's element-icon lookup to the directory most likely to hold
// the symbol PNG: the file's immediate element directory (e.g.
// "Skylanders Spyros Adventure/Fire" for both base figures and Alternate-types).
fn element_icon_key(segs: &[&str], _el: Element) -> String {
    let mut end = segs.len().saturating_sub(1); // exclude the filename
    // Drop "Alternate types" from the path when present.
    while end > 0 && segs[end - 1] == "Alternate types" {
        end -= 1;
    }
    // Drop "Vehicle" subfolder.
    while end > 0 && segs[end - 1] == "Vehicle" {
        end -= 1;
    }
    // Drop the filename.
    segs[..end].join("/")
}

// ---------------- classify ----------------

fn classify(segs: &[&str], _file_name: &str) -> (GameOfOrigin, Option<Element>, Category) {
    let top = segs.first().copied().unwrap_or("");
    match top {
        // "Items/<game>/..." — attribute to that game.
        "Items" => {
            let game = parse_game(segs.get(1).copied().unwrap_or(""));
            (game, None, Category::Item)
        }
        "Adventure Packs" => {
            let game = parse_game(segs.get(1).copied().unwrap_or(""));
            (game, None, Category::AdventurePack)
        }
        g if g.starts_with("Skylanders ") => {
            let game = parse_game(g);
            classify_inside_game(game, segs)
        }
        _ => (GameOfOrigin::CrossGame, None, Category::Other),
    }
}

fn classify_inside_game(
    game: GameOfOrigin,
    segs: &[&str],
) -> (GameOfOrigin, Option<Element>, Category) {
    let inner = segs.get(1).copied().unwrap_or("");
    match inner {
        "Sidekicks" | "Minis" => (game, None, Category::Sidekick),
        "Giants" => (game, None, Category::Giant),
        "Kaos" => (game, None, Category::Kaos),
        "Creation Crystals" => {
            let stem = segs
                .last()
                .and_then(|f| f.strip_suffix(".sky"))
                .map(|s| s.strip_suffix(".key").unwrap_or(s))
                .unwrap_or("");
            (
                game,
                parse_creation_crystal_element(stem),
                Category::CreationCrystal,
            )
        }
        "Traps" => {
            let elem_seg = segs.get(2).copied().unwrap_or("");
            if elem_seg == "Kaos" {
                (game, None, Category::Kaos)
            } else {
                (game, parse_element(elem_seg), Category::Trap)
            }
        }
        elem if parse_element(elem).is_some() => {
            let element = parse_element(elem);
            let third = segs.get(2).copied().unwrap_or("");
            let category = if third == "Vehicle" {
                Category::Vehicle
            } else {
                Category::Figure
            };
            (game, element, category)
        }
        _ => (game, None, Category::Other),
    }
}

fn parse_game(s: &str) -> GameOfOrigin {
    match s.trim_start_matches("Skylanders ").trim() {
        "Spyros Adventure" => GameOfOrigin::SpyrosAdventure,
        "Giants" => GameOfOrigin::Giants,
        "Swap Force" | "Swapforce" => GameOfOrigin::SwapForce,
        "Trap Team" => GameOfOrigin::TrapTeam,
        "Superchargers" => GameOfOrigin::Superchargers,
        "Imaginators" => GameOfOrigin::Imaginators,
        _ => GameOfOrigin::CrossGame,
    }
}

fn parse_element(s: &str) -> Option<Element> {
    Some(match s {
        "Air" => Element::Air,
        "Dark" => Element::Dark,
        "Earth" => Element::Earth,
        "Fire" => Element::Fire,
        "Life" => Element::Life,
        "Light" => Element::Light,
        "Magic" => Element::Magic,
        "Tech" => Element::Tech,
        "Undead" => Element::Undead,
        "Water" => Element::Water,
        _ => return None,
    })
}

fn parse_creation_crystal_element(stem: &str) -> Option<Element> {
    for p in stem.split('_') {
        let pu = p.trim_matches('-').trim();
        let elem = match pu {
            "AIR" => Some(Element::Air),
            "EARTH" => Some(Element::Earth),
            "FIRE" => Some(Element::Fire),
            "LIFE" => Some(Element::Life),
            "MAGIC" => Some(Element::Magic),
            "TECH" => Some(Element::Tech),
            "UNDEAD" => Some(Element::Undead),
            "WATER" => Some(Element::Water),
            "DARK" => Some(Element::Dark),
            "LIGHT" => Some(Element::Light),
            _ => None,
        };
        if elem.is_some() {
            return elem;
        }
    }
    None
}

// ---------------- display name / variant grouping ----------------

fn display_name(stem_clean: &str) -> String {
    // Imaginators uses underscores for spaces.
    stem_clean.replace('_', " ")
}

const VARIANT_PREFIXES: &[&str] = &[
    "Series 2",
    "LightCore",
    "Eon's Elite",
    "Power Punch",
    "Power Blue",
    "Big Bubble",
    "Bone Bash",
    "Birthday Bash",
    "Super Shot",
    "Shark Shooter",
    "Deep Dive",
    "Double Dare",
    "Hurricane",
    "Lava Lance",
    "Lava Barf",
    "Steel Plated",
    "Missile-Tow",
    "Hyper Beam",
    "Horn Blast",
    "Fire Bone",
    "Knockout",
    "Eggcellent",
    "Eggcited",
    "Frightful",
    "Jolly",
    "Polar",
    "Molten",
    "Jade",
    "Royal",
    "Punch",
    "Scarlet",
    "Gnarly",
    "Granite",
    "Volcanic",
    "Legendary",
    "Dark",
    "Turbo",
    "E3",
    "Golden",
    "Nitro",
    "Gold",
    "Platinum",
    "Ultimate",
];

fn derive_group_and_tag(segs: &[&str], stem_clean: &str, category: Category) -> (String, String) {
    let in_alternate = segs.contains(&"Alternate types");

    if stem_clean.contains('_') {
        if let Some((base, suffix)) = imaginators_split(stem_clean) {
            let group = base.replace('_', " ");
            let tag = if in_alternate {
                if suffix.is_empty() {
                    "Alternate".to_string()
                } else {
                    suffix
                }
            } else {
                "base".to_string()
            };
            return (group, tag);
        }
    } else if in_alternate && let Some(dash_pos) = stem_clean.rfind('-') {
        let (base, tail) = stem_clean.split_at(dash_pos);
        let tail = tail[1..].to_string();
        if is_known_imaginator_tag(&tail) {
            return (base.to_string(), tail);
        }
    }

    let (without_parens, paren) = split_parens(stem_clean);
    let trimmed = without_parens.trim();
    let (prefix, base) = peel_variant_prefix(trimmed);
    let group = base.trim().to_string();

    let tag = if !in_alternate {
        if let Some(p) = paren.clone() {
            format!("base ({})", p)
        } else if !prefix.is_empty() {
            prefix.to_string()
        } else {
            "base".to_string()
        }
    } else if !prefix.is_empty() {
        if let Some(p) = paren.clone() {
            format!("{} ({})", prefix, p)
        } else {
            prefix.to_string()
        }
    } else if let Some(p) = paren.clone() {
        format!("Alternate ({})", p)
    } else {
        "Alternate".to_string()
    };

    match category {
        Category::CreationCrystal
        | Category::AdventurePack
        | Category::Item
        | Category::Trap
        | Category::Kaos => (
            group.clone(),
            if in_alternate {
                tag
            } else {
                "base".to_string()
            },
        ),
        _ => (group, tag),
    }
}

fn imaginators_split(stem: &str) -> Option<(String, String)> {
    if !stem.contains('_') {
        return None;
    }
    if let Some(dash_pos) = stem.rfind('-') {
        let (base, tail) = stem.split_at(dash_pos);
        let tail = &tail[1..];
        if base.contains('_') || is_known_imaginator_tag(tail) {
            return Some((base.to_string(), tail.to_string()));
        }
    }
    Some((stem.to_string(), String::new()))
}

fn is_known_imaginator_tag(s: &str) -> bool {
    matches!(
        s,
        "Dark"
            | "Legendary"
            | "Mystical"
            | "Eggbomber"
            | "Steelplated"
            | "Hardboiled"
            | "Jinglebell"
            | "Solarflare"
            | "Candy-Coated"
    )
}

fn split_parens(s: &str) -> (String, Option<String>) {
    if let Some(open) = s.find('(')
        && let Some(close) = s.rfind(')')
        && close > open
    {
        let inner = s[open + 1..close].trim().to_string();
        let outer = format!("{}{}", &s[..open], &s[close + 1..]);
        let cleaned = outer.trim().to_string();
        return (cleaned, Some(inner));
    }
    (s.to_string(), None)
}

fn peel_variant_prefix(name: &str) -> (&'static str, String) {
    for &p in VARIANT_PREFIXES {
        let with_space = format!("{} ", p);
        if name.starts_with(&with_space) {
            return (p, name[with_space.len()..].to_string());
        }
    }
    ("", name.to_string())
}

fn stable_id(game: GameOfOrigin, element: Option<Element>, rel_path: &str) -> FigureId {
    let elem_str = element.map(element_str).unwrap_or("");
    let game_str = game_str(game);
    let key = format!("{}|{}|{}", game_str, elem_str, rel_path);
    let digest = Sha256::digest(key.as_bytes());
    let hex: String = digest.iter().map(|b| format!("{:02x}", b)).collect();
    FigureId::new(hex[..16].to_string())
}

fn game_str(g: GameOfOrigin) -> &'static str {
    match g {
        GameOfOrigin::SpyrosAdventure => "Spyros Adventure",
        GameOfOrigin::Giants => "Giants",
        GameOfOrigin::SwapForce => "Swap Force",
        GameOfOrigin::TrapTeam => "Trap Team",
        GameOfOrigin::Superchargers => "Superchargers",
        GameOfOrigin::Imaginators => "Imaginators",
        GameOfOrigin::CrossGame => "",
        GameOfOrigin::Unknown => "",
    }
}

fn element_str(e: Element) -> &'static str {
    match e {
        Element::Air => "Air",
        Element::Dark => "Dark",
        Element::Earth => "Earth",
        Element::Fire => "Fire",
        Element::Life => "Life",
        Element::Light => "Light",
        Element::Magic => "Magic",
        Element::Tech => "Tech",
        Element::Undead => "Undead",
        Element::Water => "Water",
    }
}
