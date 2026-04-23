//! Sanity check: index the real firmware pack from this dev machine and
//! confirm the counts match Phase 1c (504 total, specific per-game/category
//! counts). Gated by env var so CI and clean checkouts don't fail.
//!
//! Run with:
//!   SKYLANDER_PACK_ROOT="C:/Users/chris/workspace/Skylanders Characters Pack for RPCS3" \
//!     cargo test -p skylander-indexer --test real_pack -- --ignored --nocapture

use std::collections::BTreeMap;
use std::path::PathBuf;

use skylander_core::{Category, GameOfOrigin};

#[test]
#[ignore = "requires SKYLANDER_PACK_ROOT env var pointing at a firmware pack"]
fn indexes_the_real_pack() {
    let root = match std::env::var("SKYLANDER_PACK_ROOT") {
        Ok(s) => PathBuf::from(s),
        Err(_) => return,
    };
    let figures = skylander_indexer::scan(&root).expect("scan");

    let total = figures.len();
    let mut by_game: BTreeMap<&str, usize> = BTreeMap::new();
    let mut by_cat: BTreeMap<&str, usize> = BTreeMap::new();
    for f in &figures {
        *by_game.entry(game_name(f.game)).or_default() += 1;
        *by_cat.entry(cat_name(f.category)).or_default() += 1;
    }

    eprintln!("total: {total}");
    for (k, v) in &by_game {
        eprintln!("  {k:25} {v}");
    }
    eprintln!("by category:");
    for (k, v) in &by_cat {
        eprintln!("  {k:20} {v}");
    }

    assert_eq!(total, 504, "total figure count should match spike 1c");

    // Game-of-origin buckets — Items / Adventure Packs / top-level stragglers
    // all collapse to CrossGame.
    assert_eq!(by_game["Spyros Adventure"], 50);
    assert_eq!(by_game["Giants"], 74);
    assert_eq!(by_game["Swap Force"], 110);
    assert_eq!(by_game["Trap Team"], 147);
    assert_eq!(by_game["Superchargers"], 67);
    assert_eq!(by_game["Imaginators"], 56);

    // Category buckets. "trap" and "vehicle" are first-class in the library
    // (reclassified out of the spike's "item" / "other" buckets).
    assert!(by_cat.get("figure").copied().unwrap_or(0) >= 300);
    assert!(by_cat.contains_key("trap"));
    assert!(by_cat.contains_key("vehicle"));

    // No accidental top-level Sidekicks leak (that top folder is a dup).
    for f in &figures {
        assert!(!f.sky_path.to_string_lossy().contains("/Sidekicks/"));
    }

    // Every element-tagged figure should have its icon resolved (not 100%
    // guaranteed but overwhelming majority).
    let with_element = figures.iter().filter(|f| f.element.is_some()).count();
    let with_icon = figures
        .iter()
        .filter(|f| f.element.is_some() && f.element_icon_path.is_some())
        .count();
    eprintln!("element-typed: {with_element}, with icon: {with_icon}");
    assert!(
        with_icon as f64 / with_element as f64 > 0.9,
        "most element-typed figures should have a resolved icon"
    );
}

fn game_name(g: GameOfOrigin) -> &'static str {
    match g {
        GameOfOrigin::SpyrosAdventure => "Spyros Adventure",
        GameOfOrigin::Giants => "Giants",
        GameOfOrigin::SwapForce => "Swap Force",
        GameOfOrigin::TrapTeam => "Trap Team",
        GameOfOrigin::Superchargers => "Superchargers",
        GameOfOrigin::Imaginators => "Imaginators",
        GameOfOrigin::CrossGame => "CrossGame",
        GameOfOrigin::Unknown => "Unknown",
    }
}

fn cat_name(c: Category) -> &'static str {
    match c {
        Category::Figure => "figure",
        Category::Sidekick => "sidekick",
        Category::Giant => "giant",
        Category::Item => "item",
        Category::Trap => "trap",
        Category::AdventurePack => "adventure-pack",
        Category::CreationCrystal => "creation-crystal",
        Category::Vehicle => "vehicle",
        Category::Kaos => "kaos",
        Category::Other => "other",
    }
}
