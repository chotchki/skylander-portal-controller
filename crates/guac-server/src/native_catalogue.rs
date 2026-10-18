//! guacamole's interim figure catalogue (openspec AA, design decision 5): the
//! committed wiki data (`<data_root>/figures.json`) cut to what Giants plays,
//! checked row by row, plus a hand-written supplement. Wrong is worse than
//! missing, so a doubtful row is dropped. AE replaces this.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde::Deserialize;
use skylander_core::{
    Category, Element, Figure, FigureId, GameOfOrigin, MaskedVariant, TagIdentity, ToyTypeId,
};
use tracing::info;

/// One `figures.json` row; only the fields the rules read.
#[derive(Debug, Clone, Deserialize)]
struct Row {
    figure_id: String,
    alignment: Option<String>,
    #[serde(default)]
    attributes: Attributes,
}

#[derive(Debug, Clone, Default, Deserialize)]
struct Attributes {
    name: Option<String>,
    element: Option<String>,
    firstrelease: Option<String>,
}

/// A base character the data lacks or mislabels, written by hand.
struct Supplement {
    toy_type: u32,
    name: &'static str,
    element: Element,
    game: GameOfOrigin,
}

/// Toy types per RPCS3's `skylander_dialog.cpp` list (which drops the
/// hyphens); names and elements PENDING chotchki's check (AA.4).
const SUPPLEMENT: [Supplement; 4] = [
    // No row data.
    Supplement {
        toy_type: 0x06,
        name: "Dino-Rang",
        element: Element::Earth,
        game: GameOfOrigin::SpyrosAdventure,
    },
    // The data puts the "Crabs" page here.
    Supplement {
        toy_type: 0x0D,
        name: "Wham-Shell",
        element: Element::Water,
        game: GameOfOrigin::SpyrosAdventure,
    },
    // No row data.
    Supplement {
        toy_type: 0x15,
        name: "Drill Sergeant",
        element: Element::Tech,
        game: GameOfOrigin::SpyrosAdventure,
    },
    // No row data.
    Supplement {
        toy_type: 0x64,
        name: "Jet-Vac",
        element: Element::Air,
        game: GameOfOrigin::Giants,
    },
];

/// What the rules kept and dropped, by row kind.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct Tally {
    pub base_kept: usize,
    pub base_dropped: usize,
    pub variant_kept: usize,
    pub variant_dropped: usize,
    pub supplemented: usize,
}

/// A name two candidates claimed: the later toy type's is dropped.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Collision {
    dropped: FigureId,
    kept: FigureId,
}

/// The catalogue from `<data_root>/figures.json`, the same for every profile.
pub fn load(data_root: &Path) -> Result<Vec<Figure>> {
    let (figures, tally, _) = build(rows(data_root)?, &SUPPLEMENT);
    info!(
        base_kept = tally.base_kept,
        base_dropped = tally.base_dropped,
        variant_kept = tally.variant_kept,
        variant_dropped = tally.variant_dropped,
        supplemented = tally.supplemented,
        "built the native catalogue from figures.json",
    );
    Ok(figures)
}

/// Each base figure's name by toy type: the name fallback guac-extract's
/// intro index takes (openspec AB, design decision 6).
pub fn base_names(figures: &[Figure]) -> BTreeMap<u32, String> {
    figures
        .iter()
        .filter_map(|f| match f.id.tag_parts()? {
            (ToyTypeId(toy_type), MaskedVariant(0)) => Some((toy_type, f.canonical_name.clone())),
            _ => None,
        })
        .collect()
}

fn rows(data_root: &Path) -> Result<Vec<Row>> {
    let path = data_root.join("figures.json");
    let text =
        std::fs::read_to_string(&path).with_context(|| format!("read {}", path.display()))?;
    serde_json::from_str(&text).with_context(|| format!("parse {}", path.display()))
}

/// `<data_root>/images/<id>/thumb.png` when it exists, but never a
/// supplemented figure's: its folder holds a mis-keyed page's image.
pub fn portrait(data_root: &Path, id: &FigureId) -> Option<PathBuf> {
    let supplied = id.tag_parts().is_some_and(|(toy_type, variant)| {
        variant == MaskedVariant(0) && SUPPLEMENT.iter().any(|s| ToyTypeId(s.toy_type) == toy_type)
    });
    if supplied {
        return None;
    }
    let thumb = data_root.join("images").join(id.as_str()).join("thumb.png");
    thumb.is_file().then_some(thumb)
}

/// A row that passed the per-row rules, before the one-name check.
struct Candidate {
    toy_type: ToyTypeId,
    variant: MaskedVariant,
    name: String,
    group: String,
    tag: String,
    game: GameOfOrigin,
    element: Option<Element>,
}

fn build(rows: Vec<Row>, supplement: &[Supplement]) -> (Vec<Figure>, Tally, Vec<Collision>) {
    let mut tally = Tally::default();
    let mut bases = Vec::new();
    let mut variants = Vec::new();
    for row in rows {
        match FigureId::new(row.figure_id.clone()).tag_parts() {
            Some((toy_type, MaskedVariant(0))) => bases.push((toy_type, row)),
            Some((toy_type, variant)) => variants.push((toy_type, variant, row)),
            // Counted as a variant: a malformed id names no base.
            None => tally.variant_dropped += 1,
        }
    }

    // Toy type -> (base name, category); the supplement wins over the data.
    let mut base_of: HashMap<ToyTypeId, (String, Category)> = HashMap::new();
    let mut candidates = Vec::new();
    for s in supplement {
        let toy_type = ToyTypeId(s.toy_type);
        base_of.insert(toy_type, (s.name.to_string(), Category::Figure));
        candidates.push(Candidate {
            toy_type,
            variant: MaskedVariant(0),
            name: s.name.to_string(),
            group: s.name.to_string(),
            tag: "base".to_string(),
            game: s.game,
            element: Some(s.element),
        });
        tally.supplemented += 1;
    }
    for (toy_type, row) in bases {
        match (game(&row), name(&row)) {
            (Some(game), Some(name)) if !base_of.contains_key(&toy_type) => {
                base_of.insert(toy_type, (name.clone(), category(row.alignment.as_deref())));
                candidates.push(Candidate {
                    toy_type,
                    variant: MaskedVariant(0),
                    group: name.clone(),
                    tag: "base".to_string(),
                    name,
                    game,
                    element: element(&row),
                });
            }
            _ => tally.base_dropped += 1,
        }
    }
    for (toy_type, variant, row) in variants {
        let candidate = (|| {
            let game = game(&row)?;
            let name = name(&row)?;
            let (base, _) = base_of.get(&toy_type)?;
            let tag = variant_tag(&name, base)?;
            Some(Candidate {
                toy_type,
                variant,
                group: base.clone(),
                tag,
                name,
                game,
                element: element(&row),
            })
        })();
        match candidate {
            Some(c) => candidates.push(c),
            None => tally.variant_dropped += 1,
        }
    }

    // First claim on a name wins, since toy types run in release order and the
    // rekey left SA names on later ones (Wrecking Ball on SWAP Force's 0x3EE).
    // Every loss is reported so a test can pin the expected ones.
    candidates.sort_by_key(|c| (c.toy_type, c.variant));
    let supplied: HashSet<ToyTypeId> = supplement.iter().map(|s| ToyTypeId(s.toy_type)).collect();
    let mut claimed: HashMap<String, FigureId> = HashMap::new();
    // Toy types whose base lost its name; their variants were tagged against it.
    let mut lost: HashSet<ToyTypeId> = HashSet::new();
    let mut collisions = Vec::new();
    let mut figures = Vec::new();
    for c in candidates {
        let is_base = c.variant == MaskedVariant(0);
        if !is_base && lost.contains(&c.toy_type) {
            tally.variant_dropped += 1;
            continue;
        }
        let identity = TagIdentity::new(c.toy_type, c.variant);
        let id = FigureId::from_tag_identity(identity);
        if let Some(kept) = claimed.get(&c.name.to_lowercase()) {
            if is_base {
                lost.insert(c.toy_type);
                tally.base_dropped += 1;
            } else {
                tally.variant_dropped += 1;
            }
            collisions.push(Collision {
                dropped: id,
                kept: kept.clone(),
            });
            continue;
        }
        claimed.insert(c.name.to_lowercase(), id.clone());
        if !is_base {
            tally.variant_kept += 1;
        } else if !supplied.contains(&c.toy_type) {
            tally.base_kept += 1;
        }
        let category = base_of
            .get(&c.toy_type)
            .map_or(Category::Other, |(_, category)| *category);
        figures.push(Figure {
            id,
            canonical_name: c.name,
            variant_group: c.group,
            variant_tag: c.tag,
            game: c.game,
            element: c.element,
            category,
            vehicle_terrain: None,
            sky_path: PathBuf::new(),
            element_icon_path: None,
            tag_identity: Some(identity),
        });
    }
    (figures, tally, collisions)
}

/// The name with its whitespace collapsed (one row has a newline in it).
fn name(row: &Row) -> Option<String> {
    let name = row.attributes.name.as_deref()?;
    let name = name.split_whitespace().collect::<Vec<_>>().join(" ");
    (!name.is_empty()).then_some(name)
}

/// Spyro's Adventure or Giants, from `firstrelease`'s first line (a few rows
/// trail an artist's name on a second).
fn game(row: &Row) -> Option<GameOfOrigin> {
    match row
        .attributes
        .firstrelease
        .as_deref()?
        .lines()
        .next()?
        .trim()
    {
        "Skylanders: Spyro's Adventure" => Some(GameOfOrigin::SpyrosAdventure),
        "Skylanders: Giants" => Some(GameOfOrigin::Giants),
        _ => None,
    }
}

/// Case-insensitive; a Dark Edition's `Magic/Dark` is its first element.
fn element(row: &Row) -> Option<Element> {
    let raw = row.attributes.element.as_deref()?;
    let first = raw.split('/').next()?.trim().to_ascii_lowercase();
    Some(match first.as_str() {
        "air" => Element::Air,
        "dark" => Element::Dark,
        "earth" => Element::Earth,
        "fire" => Element::Fire,
        "life" => Element::Life,
        "light" => Element::Light,
        "magic" => Element::Magic,
        "tech" => Element::Tech,
        "undead" => Element::Undead,
        "water" => Element::Water,
        _ => return None,
    })
}

/// From a base row's `alignment`; a variant takes its base's, since Gnarly
/// Tree Rex's own row says "Skylander" for a Giant.
fn category(alignment: Option<&str>) -> Category {
    let a = alignment.unwrap_or_default().to_lowercase();
    if a.contains("giant") {
        Category::Giant
    } else if a.contains("sidekick") || a.contains("mini") {
        Category::Sidekick
    } else if a.contains("magic item") {
        Category::Item
    } else if a.contains("skylander") {
        Category::Figure
    } else {
        Category::Other
    }
}

/// The variant's name minus its base name; `None` when the base isn't in it
/// (a mis-keyed row) or is all of it (the base's page reused).
fn variant_tag(name: &str, base: &str) -> Option<String> {
    let start = name.to_lowercase().find(&base.to_lowercase())?;
    let rest = format!("{} {}", &name[..start], &name[start + base.len()..]);
    let tag = rest.split_whitespace().collect::<Vec<_>>().join(" ");
    (!tag.is_empty()).then_some(tag)
}

#[cfg(test)]
mod tests {
    use super::*;

    const SA: Option<&str> = Some("Skylanders: Spyro's Adventure");
    const GIANTS: Option<&str> = Some("Skylanders: Giants");

    fn row(id: &str, name: Option<&str>, release: Option<&str>, element: &str) -> Row {
        Row {
            figure_id: id.to_string(),
            alignment: Some("Core Skylander".to_string()),
            attributes: Attributes {
                name: name.map(str::to_string),
                element: Some(element.to_string()),
                firstrelease: release.map(str::to_string),
            },
        }
    }

    fn find<'a>(figures: &'a [Figure], id: &str) -> Option<&'a Figure> {
        figures.iter().find(|f| f.id.as_str() == id)
    }

    fn ids(figures: &[Figure]) -> Vec<&str> {
        figures.iter().map(|f| f.id.as_str()).collect()
    }

    #[test]
    fn a_mis_keyed_variant_is_dropped_and_a_real_one_kept() {
        let rows = vec![
            row("000010-0000", Some("Spyro"), SA, "Magic"),
            row("000010-0801", Some("Cynder"), SA, "Undead"),
            row("00000a-0000", Some("Ignitor"), SA, "Fire"),
            row("00000a-0803", Some("Legendary Ignitor"), GIANTS, "Fire"),
            row("000000-0000", Some("Whirlwind"), SA, "Air"),
            row("000000-0802", Some("Polar Whirlwind"), GIANTS, "Air"),
            // The base's page on a variant row: no tag to tell them apart.
            row("000013-0000", Some("Trigger Happy"), SA, "Tech"),
            row("000013-0801", Some("Trigger Happy"), SA, "Tech"),
        ];
        let (figures, tally, _) = build(rows, &[]);

        assert!(find(&figures, "000010-0801").is_none());
        assert!(find(&figures, "000013-0801").is_none());
        let ignitor = find(&figures, "00000a-0803").expect("Legendary Ignitor");
        assert_eq!(ignitor.variant_group, "Ignitor");
        assert_eq!(ignitor.variant_tag, "Legendary");
        assert_eq!(ignitor.game, GameOfOrigin::Giants);
        let polar = find(&figures, "000000-0802").expect("Polar Whirlwind");
        assert_eq!(polar.variant_group, "Whirlwind");
        assert_eq!(polar.variant_tag, "Polar");
        assert_eq!(
            tally,
            Tally {
                base_kept: 4,
                variant_kept: 2,
                variant_dropped: 2,
                ..Tally::default()
            }
        );
    }

    #[test]
    fn a_later_game_row_is_dropped() {
        let swap_force = Some("Skylanders: Swap Force");
        let rows = vec![
            row("000009-0000", Some("Eruptor"), SA, "Fire"),
            row("000009-0802", Some("Volcanic Eruptor"), swap_force, "Fire"),
            row(
                "0001c2-0000",
                Some("Gusto"),
                Some("Skylanders: Trap Team"),
                "Air",
            ),
        ];
        let (figures, tally, _) = build(rows, &[]);
        assert_eq!(ids(&figures), ["000009-0000"]);
        assert_eq!(tally.base_dropped, 1);
        assert_eq!(tally.variant_dropped, 1);
    }

    #[test]
    fn a_nameless_base_row_is_dropped_unless_supplemented() {
        let rows = vec![
            row("000006-0000", None, None, ""),
            row("000015-0000", None, SA, "Tech"),
            row("00000d-0000", Some("Crabs"), SA, ""),
        ];
        let supplement = [Supplement {
            toy_type: 0x0D,
            name: "Wham-Shell",
            element: Element::Water,
            game: GameOfOrigin::SpyrosAdventure,
        }];
        let (figures, tally, _) = build(rows, &supplement);
        assert_eq!(ids(&figures), ["00000d-0000"]);
        let wham = &figures[0];
        assert_eq!(wham.canonical_name, "Wham-Shell");
        assert_eq!(wham.element, Some(Element::Water));
        assert_eq!(wham.category, Category::Figure);
        assert_eq!(tally.base_dropped, 3);
        assert_eq!(tally.supplemented, 1);
    }

    #[test]
    fn a_name_on_a_later_toy_type_is_dropped() {
        let rows = vec![
            row("0003ee-0000", Some("Wrecking Ball"), SA, "Magic"),
            row("000017-0000", Some("Wrecking Ball"), SA, "Magic"),
        ];
        let (figures, tally, collisions) = build(rows, &[]);
        assert_eq!(ids(&figures), ["000017-0000"]);
        assert_eq!(tally.base_dropped, 1);
        assert_eq!(
            collisions,
            [Collision {
                dropped: FigureId::new("0003ee-0000"),
                kept: FigureId::new("000017-0000"),
            }]
        );
    }

    #[test]
    fn a_variant_whose_base_lost_its_name_is_dropped() {
        let rows = vec![
            row("0003ee-0000", Some("Wrecking Ball"), SA, "Magic"),
            row("0003ee-0801", Some("Dark Wrecking Ball"), SA, "Magic"),
            row("000017-0000", Some("Wrecking Ball"), SA, "Magic"),
        ];
        let (figures, tally, _) = build(rows, &[]);
        assert_eq!(ids(&figures), ["000017-0000"]);
        assert_eq!(tally.base_dropped, 1);
        assert_eq!(tally.variant_dropped, 1);
        assert_eq!(tally.variant_kept, 0);
    }

    #[test]
    fn fields_map_from_the_row() {
        let mut tree_rex = row("000070-0000", Some("Tree Rex"), GIANTS, "life");
        tree_rex.alignment = Some("Giant".to_string());
        let mut gnarly = row("000070-0202", Some("Gnarly Tree Rex"), GIANTS, "Life");
        gnarly.alignment = Some("Skylander".to_string());
        let artist = Some("Skylanders: Spyro's Adventure\nStefano Brusa");
        let mut dark = row("00001c-0000", Some("Dark Spyro"), artist, "Magic/Dark");
        dark.alignment = Some("Dark Edition Skylander".to_string());
        let mut elixir = row("0000ca-0000", Some("Healing Elixir"), SA, "");
        elixir.attributes.element = None;
        elixir.alignment = Some("Magic Item".to_string());
        let mut runt = row("000202-0000", Some("Gill Runt"), SA, "Water");
        runt.alignment = Some("*Sidekick (2011-13) \n*Mini (2014 and onwards)".to_string());
        let stealth = row("00001a-0000", Some("Stealth Elf"), SA, "Life");
        let legendary = row(
            "00001a-0803",
            Some("Legendary\nStealth Elf"),
            GIANTS,
            "Life",
        );

        let rows = vec![tree_rex, gnarly, dark, elixir, runt, stealth, legendary];
        let (figures, _, _) = build(rows, &[]);
        let get = |id| find(&figures, id).unwrap();
        assert_eq!(get("000070-0000").element, Some(Element::Life));
        assert_eq!(get("000070-0000").category, Category::Giant);
        assert_eq!(get("000070-0000").variant_tag, "base");
        assert_eq!(get("000070-0202").category, Category::Giant);
        assert_eq!(get("00001c-0000").element, Some(Element::Magic));
        assert_eq!(get("00001c-0000").game, GameOfOrigin::SpyrosAdventure);
        assert_eq!(get("0000ca-0000").category, Category::Item);
        assert_eq!(get("0000ca-0000").element, None);
        assert_eq!(get("000202-0000").category, Category::Sidekick);
        assert_eq!(get("00001a-0803").canonical_name, "Legendary Stealth Elf");
        assert_eq!(get("00001a-0803").variant_tag, "Legendary");
        assert!(figures.iter().all(|f| f.sky_path.as_os_str().is_empty()));
    }

    #[test]
    fn a_missing_portrait_is_none() {
        let root = tempfile::tempdir().unwrap();
        let thumb = root.path().join("images/000009-0000/thumb.png");
        std::fs::create_dir_all(thumb.parent().unwrap()).unwrap();
        std::fs::write(&thumb, b"\x89PNG").unwrap();
        std::fs::create_dir_all(root.path().join("images/00000a-0000")).unwrap();
        let jet_vac = root.path().join("images/000064-0000/thumb.png");
        std::fs::create_dir_all(jet_vac.parent().unwrap()).unwrap();
        std::fs::write(&jet_vac, b"\x89PNG").unwrap();

        let eruptor = FigureId::new("000009-0000");
        assert_eq!(portrait(root.path(), &eruptor), Some(thumb));
        assert_eq!(portrait(root.path(), &FigureId::new("00000a-0000")), None);
        assert_eq!(portrait(root.path(), &FigureId::new("000008-0000")), None);
        // The data's 0x64 thumb is a building, not Jet-Vac.
        assert_eq!(portrait(root.path(), &FigureId::new("000064-0000")), None);
    }

    #[test]
    fn the_committed_data_builds_a_clean_catalogue() {
        let data_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../data");
        let rows = rows(&data_root).expect("read the committed catalogue");
        let (figures, _, collisions) = build(rows, &SUPPLEMENT);

        // Only the known rekey leftovers; any other name collision is a
        // mis-keyed row the first-claim rule would hide.
        let wrecking_ball = FigureId::new("000017-0000");
        let expected: Vec<Collision> = ["0003ee-0000", "0003f5-0000", "0003f7-0000"]
            .into_iter()
            .map(|id| Collision {
                dropped: FigureId::new(id),
                kept: wrecking_ball.clone(),
            })
            .collect();
        assert_eq!(collisions, expected);

        let mut toy_type_of: HashMap<String, ToyTypeId> = HashMap::new();
        for f in &figures {
            assert!(!f.canonical_name.trim().is_empty(), "{} has no name", f.id);
            let (toy_type, _) = f.id.tag_parts().expect("tag-form id");
            let first = *toy_type_of
                .entry(f.canonical_name.to_lowercase())
                .or_insert(toy_type);
            assert_eq!(
                first, toy_type,
                "{} sits on two toy types",
                f.canonical_name
            );
        }

        let eruptor = find(&figures, "000009-0000").expect("Eruptor");
        assert_eq!(eruptor.canonical_name, "Eruptor");
        assert_eq!(eruptor.element, Some(Element::Fire));
        for s in &SUPPLEMENT {
            let id = format!("{:06x}-0000", s.toy_type);
            let f = find(&figures, &id).unwrap_or_else(|| panic!("{} missing", s.name));
            assert_eq!(f.canonical_name, s.name);
            assert_eq!(f.element, Some(s.element));
            assert_eq!(portrait(&data_root, &f.id), None, "{}", s.name);
        }
        let names = base_names(&figures);
        assert_eq!(names[&9], "Eruptor");
        assert_eq!(names[&0x64], "Jet-Vac");
        // A variant (Legendary Ignitor, 00000a-0803) doesn't replace its base.
        assert_eq!(names[&0x0A], "Ignitor");

        // Cynder on Spyro's toy type; Wrecking Ball on a SWAP Force bottom.
        assert!(find(&figures, "000010-0801").is_none());
        assert!(find(&figures, "0003ee-0000").is_none());
    }
}
