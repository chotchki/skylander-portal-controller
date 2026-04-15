// Firmware-pack inventory builder.
//
// Walks the Skylanders firmware pack and emits a JSON array of entries.
// Run with:
//   cargo run --manifest-path tools/inventory/Cargo.toml -- \
//       "C:\Users\chris\workspace\Skylanders Characters Pack for RPCS3" \
//       docs/research/firmware-inventory.json
//
// Stable IDs are sha256(game|element|relative_path) truncated to 16 hex chars.

use std::collections::BTreeMap;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};
use walkdir::WalkDir;

#[derive(Debug)]
struct Entry {
    id: String,
    game: String,
    element: Option<String>,
    category: String,
    variant_group: String,
    variant_tag: String,
    name: String,
    relative_path: String,
}

fn main() {
    let args: Vec<String> = env::args().collect();
    if args.len() < 3 {
        eprintln!("usage: firmware-inventory <pack-root> <out.json>");
        std::process::exit(2);
    }
    let pack_root = PathBuf::from(&args[1]);
    let out_path = PathBuf::from(&args[2]);

    let mut entries: Vec<Entry> = Vec::new();
    let mut skipped: Vec<String> = Vec::new();

    for dent in WalkDir::new(&pack_root)
        .into_iter()
        .filter_map(Result::ok)
    {
        if !dent.file_type().is_file() {
            continue;
        }
        let path = dent.path();
        let rel = path.strip_prefix(&pack_root).unwrap();
        let rel_str = rel.to_string_lossy().replace('\\', "/");

        let file_name = path.file_name().unwrap().to_string_lossy().to_string();
        let lower = file_name.to_lowercase();

        // Ignore non-figure files.
        if lower == "desktop.ini" {
            continue;
        }
        if lower == "poster.png" || lower == "poster.jpg" || lower == "poster.jpeg" {
            continue;
        }
        if lower.ends_with(".txt") {
            continue;
        }
        if lower.ends_with("symbolskylanders.png") {
            continue;
        }
        if lower == "skylanders-giants-logo.png" {
            continue;
        }
        if !lower.ends_with(".sky") {
            skipped.push(rel_str.clone());
            continue;
        }

        // Top-level `Sidekicks/` is a known duplicate.
        let segs: Vec<&str> = rel_str.split('/').collect();
        if segs.first().map(|s| *s) == Some("Sidekicks") {
            continue;
        }

        let entry = classify(&segs, &file_name, &rel_str);
        entries.push(entry);
    }

    entries.sort_by(|a, b| a.relative_path.cmp(&b.relative_path));

    // Emit JSON by hand to avoid a serde dep.
    let json = render_json(&entries);
    fs::create_dir_all(out_path.parent().unwrap()).unwrap();
    fs::write(&out_path, json).expect("write json");

    // Print summary to stdout.
    let mut by_game: BTreeMap<String, usize> = BTreeMap::new();
    let mut by_cat: BTreeMap<String, usize> = BTreeMap::new();
    let mut reposes_by_game: BTreeMap<String, usize> = BTreeMap::new();
    for e in &entries {
        *by_game.entry(e.game.clone()).or_default() += 1;
        *by_cat.entry(e.category.clone()).or_default() += 1;
        if e.variant_tag != "base" {
            *reposes_by_game.entry(e.game.clone()).or_default() += 1;
        }
    }
    println!("total: {}", entries.len());
    println!("by game:");
    for (k, v) in &by_game {
        println!("  {:25}{}", k, v);
    }
    println!("by category:");
    for (k, v) in &by_cat {
        println!("  {:20}{}", k, v);
    }
    println!("reposes (variant_tag != base) by game:");
    for (k, v) in &reposes_by_game {
        println!("  {:25}{}", k, v);
    }
    if !skipped.is_empty() {
        println!("\nUnexpected non-.sky files (not in ignore list):");
        for s in &skipped {
            println!("  {}", s);
        }
    }
}

fn classify(segs: &[&str], file_name: &str, rel_str: &str) -> Entry {
    let stem = file_name.strip_suffix(".sky").unwrap_or(file_name).to_string();
    // Some Imaginators files have ".key.sky" double-extension; the inner ".key"
    // is meaningful so we leave it on the displayed name. But strip it for
    // the canonical group/tag derivation.
    let stem_clean = stem.strip_suffix(".key").unwrap_or(&stem).to_string();

    let top = segs[0];
    let (game, element, category) = match top {
        "Items" => {
            let game = normalize_game_name(segs.get(1).copied().unwrap_or(""));
            (game, None, "item".to_string())
        }
        "Adventure Packs" => {
            let game = normalize_game_name(segs.get(1).copied().unwrap_or(""));
            (game, None, "adventure-pack".to_string())
        }
        g if g.starts_with("Skylanders ") => {
            let game = normalize_game_name(g.trim_start_matches("Skylanders ").trim());
            classify_inside_game(&game, segs, &stem_clean)
        }
        _ => (top.to_string(), None, "other".to_string()),
    };

    let (variant_group, variant_tag) = derive_group_and_tag(segs, &stem_clean, &category);

    let id = stable_id(&game, element.as_deref(), rel_str);

    Entry {
        id,
        game,
        element,
        category,
        variant_group,
        variant_tag,
        name: stem,
        relative_path: rel_str.to_string(),
    }
}

fn normalize_game_name(s: &str) -> String {
    // "Spyros Adventure", "Giants", "Swap Force", "Trap Team", "Superchargers",
    // "Imaginators", "Swapforce" all map to a canonical short form.
    match s {
        "Spyros Adventure" | "Skylanders Spyros Adventure" => "Spyros Adventure".to_string(),
        "Giants" | "Skylanders Giants" => "Giants".to_string(),
        "Swap Force" | "Swapforce" | "Skylanders Swapforce" => "Swap Force".to_string(),
        "Trap Team" | "Skylanders Trap Team" => "Trap Team".to_string(),
        "Superchargers" | "Skylanders Superchargers" => "Superchargers".to_string(),
        "Imaginators" | "Skylanders Imaginators" => "Imaginators".to_string(),
        other => other.to_string(),
    }
}

fn classify_inside_game(
    game: &str,
    segs: &[&str],
    stem_clean: &str,
) -> (String, Option<String>, String) {
    // segs[0] = "Skylanders <Game>", segs[1] = first inner folder.
    let inner = segs.get(1).copied().unwrap_or("");
    match inner {
        "Sidekicks" => (game.to_string(), None, "sidekick".to_string()),
        "Minis" => (game.to_string(), None, "sidekick".to_string()),
        "Giants" => (game.to_string(), None, "giant".to_string()),
        "Kaos" => (game.to_string(), None, "kaos".to_string()),
        "Creation Crystals" => {
            let elem = parse_creation_crystal_element(stem_clean);
            (game.to_string(), elem, "creation-crystal".to_string())
        }
        "Traps" => {
            let elem = segs.get(2).copied().unwrap_or("");
            if elem == "Kaos" {
                (game.to_string(), None, "kaos".to_string())
            } else {
                (
                    game.to_string(),
                    Some(elem.to_string()),
                    "item".to_string(),
                )
            }
        }
        elem if is_element(elem) => {
            // Could be a figure under <Element>/ or <Element>/Alternate types/
            // or a vehicle under <Element>/Vehicle/.
            let third = segs.get(2).copied().unwrap_or("");
            let category = if third == "Vehicle" {
                "other".to_string() // vehicles — no enum slot, see writeup
            } else {
                "figure".to_string()
            };
            (game.to_string(), Some(elem.to_string()), category)
        }
        _ => (game.to_string(), None, "other".to_string()),
    }
}

fn is_element(s: &str) -> bool {
    matches!(
        s,
        "Air" | "Earth" | "Fire" | "Life" | "Magic" | "Tech" | "Undead" | "Water" | "Dark" | "Light"
    )
}

fn parse_creation_crystal_element(stem: &str) -> Option<String> {
    // Filenames look like CRYSTAL_-_AIR_Lantern (after .key strip).
    let parts: Vec<&str> = stem.split('_').collect();
    for p in parts {
        let pu = p.trim_matches('-').trim();
        let elem = match pu {
            "AIR" => Some("Air"),
            "EARTH" => Some("Earth"),
            "FIRE" => Some("Fire"),
            "LIFE" => Some("Life"),
            "MAGIC" => Some("Magic"),
            "TECH" => Some("Tech"),
            "UNDEAD" => Some("Undead"),
            "WATER" => Some("Water"),
            "DARK" => Some("Dark"),
            "LIGHT" => Some("Light"),
            _ => None,
        };
        if let Some(e) = elem {
            return Some(e.to_string());
        }
    }
    None
}

// Known variant prefixes and suffixes used to collapse reposes onto their base.
// Order matters for prefixes (longest first when ambiguous).
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

fn derive_group_and_tag(
    segs: &[&str],
    stem_clean: &str,
    category: &str,
) -> (String, String) {
    let in_alternate = segs.iter().any(|s| *s == "Alternate types");

    // Imaginators uses underscores in base names ("Bad_Juju") and dashes to mark
    // variant suffixes ("Hoodsickle-Steelplated", "Golden_Queen-Dark").
    // Two cases:
    //   1. Stem contains an underscore — definitely an Imaginators-style name.
    //   2. Stem is in `Alternate types/` and contains a dash — treat as variant.
    if stem_clean.contains('_') {
        if let Some((base, suffix)) = imaginators_split(stem_clean) {
            let group = base.replace('_', " ");
            let tag = if in_alternate {
                if suffix.is_empty() { "Alternate".to_string() } else { suffix }
            } else {
                "base".to_string()
            };
            return (group, tag);
        }
    } else if in_alternate {
        if let Some(dash_pos) = stem_clean.rfind('-') {
            let (base, tail) = stem_clean.split_at(dash_pos);
            let tail = tail[1..].to_string();
            if is_known_imaginator_tag(&tail) {
                return (base.to_string(), tail);
            }
        }
    }

    // Strip parenthetical halves (Top)/(Bottom) etc. for grouping.
    let (without_parens, paren) = split_parens(stem_clean);

    // Try to peel a known variant prefix.
    let trimmed = without_parens.trim();
    let (prefix, base) = peel_variant_prefix(trimmed);

    let group = base.trim().to_string();

    // Build tag.
    let tag = if !in_alternate {
        // Base file lives in element folder. Swap-Force figures come as Top/Bottom
        // halves with neither half being "the base"; keep them grouped but tag both
        // as "base (Top)" / "base (Bottom)" so they don't get counted as reposes.
        if let Some(p) = paren.clone() {
            format!("base ({})", p)
        } else if !prefix.is_empty() {
            // A variant prefix on a file in the *base* element folder
            // (Giants did this with "Series 2 X", "LightCore X", "Legendary Chill",
            // and Superchargers does it with "Eon's Elite X", "Dark X", etc.)
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
    } else {
        // Alternate but no recognized prefix — fall back to the literal stem.
        if let Some(p) = paren.clone() {
            format!("Alternate ({})", p)
        } else {
            "Alternate".to_string()
        }
    };

    // Special-case categories where grouping is trivially the name.
    if category == "creation-crystal"
        || category == "adventure-pack"
        || category == "item"
        || category == "kaos"
    {
        return (group.clone(), if in_alternate { tag } else { "base".to_string() });
    }

    (group, tag)
}

fn imaginators_split(stem: &str) -> Option<(String, String)> {
    // Imaginators bases use underscores ("Bad_Juju", "Golden_Queen").
    // Imaginators reposes use a dash AFTER the (possibly underscored) base
    // ("Golden_Queen-Dark", "Hoodsickle-Steelplated", "Tri-Tip-Legendary").
    // Heuristic: if the stem contains an underscore OR ends in a -<Word> that
    // we recognize as a variant tag and the rest looks like a base, split.
    if !stem.contains('_') {
        return None;
    }
    if let Some(dash_pos) = stem.rfind('-') {
        // Don't split "Tri-Tip" alone; require a recognizable variant tail
        // OR the base portion contains an underscore.
        let (base, tail) = stem.split_at(dash_pos);
        let tail = &tail[1..]; // drop the dash
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
    if let Some(open) = s.find('(') {
        if let Some(close) = s.rfind(')') {
            if close > open {
                let inner = s[open + 1..close].trim().to_string();
                let outer = format!("{}{}", &s[..open], &s[close + 1..]);
                let cleaned = outer.trim().to_string();
                return (cleaned, Some(inner));
            }
        }
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

fn stable_id(game: &str, element: Option<&str>, rel_path: &str) -> String {
    let key = format!("{}|{}|{}", game, element.unwrap_or(""), rel_path);
    let mut hasher = Sha256::new();
    hasher.update(key.as_bytes());
    let digest = hasher.finalize();
    let hex: String = digest.iter().map(|b| format!("{:02x}", b)).collect();
    hex[..16].to_string()
}

fn render_json(entries: &[Entry]) -> String {
    let mut s = String::new();
    s.push_str("[\n");
    for (i, e) in entries.iter().enumerate() {
        s.push_str("  {\n");
        s.push_str(&format!("    \"id\": \"{}\",\n", e.id));
        s.push_str(&format!("    \"game\": {},\n", json_str(&e.game)));
        match &e.element {
            Some(el) => s.push_str(&format!("    \"element\": {},\n", json_str(el))),
            None => s.push_str("    \"element\": null,\n"),
        }
        s.push_str(&format!("    \"category\": {},\n", json_str(&e.category)));
        s.push_str(&format!(
            "    \"variant_group\": {},\n",
            json_str(&e.variant_group)
        ));
        s.push_str(&format!(
            "    \"variant_tag\": {},\n",
            json_str(&e.variant_tag)
        ));
        s.push_str(&format!("    \"name\": {},\n", json_str(&e.name)));
        s.push_str(&format!(
            "    \"relative_path\": {}\n",
            json_str(&e.relative_path)
        ));
        if i + 1 == entries.len() {
            s.push_str("  }\n");
        } else {
            s.push_str("  },\n");
        }
    }
    s.push_str("]\n");
    s
}

fn json_str(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

fn _path_to_string(p: &Path) -> String {
    p.to_string_lossy().to_string()
}
