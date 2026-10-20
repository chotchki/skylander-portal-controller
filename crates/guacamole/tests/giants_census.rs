//! The census tables against the disc (openspec AE.25, AG.43): every key the
//! 01.00 roster's character models draw on a card and every key chapter 1
//! draws, read off `GUAC_GIANTS_DUMP` through the extractor's own readers
//! (in memory, no extract written) and keyed by the TV's own derivation (the
//! warm's for a card, the level loader's for chapter 1), equal to
//! `guacamole::census`'s tables. A table that drifts prints the rows the dump
//! gives, in the table's syntax.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::time::Instant;

use bevy::prelude::GlobalTransform;
use guac_extract::{levels, models, roster};
use guac_formats::igz::Igz;
use guac_ir::{DumpIdentity, Sha256};
use guacamole::census::{self, Row};
use guacamole::character::turntable::rig_scene;
use guacamole::character::warm::{self, WarmKey};
use guacamole::level_scene::{
    self, Drawn, LevelFog, fresh_state_transform, level_lights, sky_fogged, start_zone, view_scene,
};

/// `key` from the environment, else from the repo's `.env.dev`.
fn env_or_dotenv(key: &str) -> Option<String> {
    if let Ok(v) = std::env::var(key) {
        return Some(v);
    }
    let dotenv = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../.env.dev");
    let text = std::fs::read_to_string(dotenv).ok()?;
    text.lines().find_map(|line| {
        let (k, v) = line.trim().split_once('=')?;
        (k.trim() == key).then(|| v.trim().trim_matches('"').to_owned())
    })
}

fn identity() -> DumpIdentity {
    DumpIdentity {
        title_id: "BLUS30968".into(),
        app_ver: "01.00".into(),
        manifest_hash: Sha256([0; 32]),
    }
}

/// Every key the roster's character models draw on a card, as the warm
/// reads them off a shelf: each model in the rig's scene.
fn card_keys(dump: &Path) -> (Vec<WarmKey>, usize) {
    let usrdir = dump.join("PS3_GAME/USRDIR");
    let roster = roster::read(dump, identity()).expect("the roster");
    let global = std::fs::read(usrdir.join(roster::GLOBAL)).expect("global.bld");
    let global = models::level(&global, roster::GLOBAL).expect("global's level");
    let rig = models::rig(&global, identity()).expect("the sprite rig");
    let scene = rig_scene(&rig).expect("the rig's scene");
    let mut keys = Vec::new();
    let loads = models::character_loads(&roster);
    for load in &loads {
        let path = models::archive_path(&usrdir, load);
        let archive = models::archive(&path, load).expect("the archive");
        let level = models::level(&archive, load).expect("its level");
        let model = models::read(&level, load, identity())
            .unwrap_or_else(|e| panic!("{load}: {e}"))
            .model;
        for (k, _) in warm::keys(&model, &scene).unwrap_or_else(|e| panic!("{load}: {e}")) {
            if !keys.contains(&k) {
                keys.push(k);
            }
        }
    }
    (keys, loads.len())
}

/// Every key chapter 1 draws, as the level loader keys it: its drawn
/// segments in its scene at the fresh state's camera, lit and fogged by its
/// start's zone (AG.41), the sky unfogged as that zone has it.
fn level_keys(dump: &Path) -> (Vec<WarmKey>, usize) {
    let usrdir = dump.join("PS3_GAME/USRDIR");
    let level = levels::LEVELS[0];
    let archive = std::fs::read(levels::archive_path(&usrdir, level)).expect("the level archive");
    let bld = levels::level_bld(&archive).expect("its level.bld");
    let igz = Igz::parse(&bld).expect("the level parses");
    let scene = levels::scene(&igz, level, identity())
        .expect("the scene")
        .ir;
    let lights = levels::lights(&igz, level, identity())
        .expect("the lights")
        .ir;
    let atmosphere = levels::atmosphere(&igz, level, identity())
        .expect("the atmosphere")
        .ir;
    let zone = start_zone(&atmosphere).expect("the start's zone");
    let lights = level_lights(&lights, zone).expect("the world lights");
    let fog = LevelFog::from_zone(&zone.fog).expect("the start's fog");
    let sky_fogged = sky_fogged(zone).expect("the sky dome's share");
    let mut drawn = Drawn::default();
    for graph in scene.scenes {
        drawn.append(level_scene::drawn(graph, sky_fogged).expect("a scene draws"));
    }
    let view = GlobalTransform::from(fresh_state_transform())
        .to_matrix()
        .inverse();
    let scene = view_scene(&lights, &fog, view);
    let keys = level_scene::keys(&drawn, &scene)
        .expect("the level's keys")
        .into_iter()
        .map(|(k, _)| k)
        .collect();
    (keys, drawn.materials.len())
}

/// The rows `keys` give, sorted as the tables keep them.
fn rows(keys: &[WarmKey]) -> BTreeSet<Row> {
    keys.iter().map(Row::of).collect()
}

/// Whether the dump's keys are the table's, printing the dump's rows in the
/// table's syntax when they aren't.
fn check(what: &str, from_disc: &[WarmKey], table: &[Row], skinned: bool) -> Result<(), String> {
    let disc = rows(from_disc);
    let kept: BTreeSet<Row> = table.iter().copied().collect();
    if let Some(k) = from_disc.iter().find(|k| k.mesh.skinned != skinned) {
        return Err(format!("{what}: a key skinned the other way: {k:?}"));
    }
    if disc == kept {
        return Ok(());
    }
    eprintln!("{what}: the dump gives {} rows:", disc.len());
    for r in &disc {
        eprintln!("        {}", r.source());
    }
    let missing: Vec<_> = disc.difference(&kept).collect();
    let extra: Vec<_> = kept.difference(&disc).collect();
    Err(format!(
        "{what}: the table differs; {} missing, {} extra: {missing:?} {extra:?}",
        missing.len(),
        extra.len()
    ))
}

#[test]
#[ignore = "disc: reads the 01.00 roster's models and chapter 1 off GUAC_GIANTS_DUMP (AE.25)"]
fn the_census_is_the_discs() {
    let dump = PathBuf::from(
        env_or_dotenv("GUAC_GIANTS_DUMP").expect("set GUAC_GIANTS_DUMP to the Giants dump"),
    );
    let started = Instant::now();
    let (cards, models) = card_keys(&dump);
    let cards_took = started.elapsed();
    let started = Instant::now();
    let (level, materials) = level_keys(&dump);
    let level_took = started.elapsed();
    eprintln!(
        "cards: {} keys off {models} models in {:.1} s; chapter 1: {} keys off {materials} materials in {:.1} s",
        cards.len(),
        cards_took.as_secs_f64(),
        level.len(),
        level_took.as_secs_f64()
    );
    let checks = [
        check("cards", &cards, census::TURNTABLE, true),
        check("chapter 1", &level, census::LEVEL, false),
    ];
    for c in &checks {
        if let Err(why) = c {
            eprintln!("{why}");
        }
    }
    assert!(checks.iter().all(Result::is_ok), "a table isn't the dump's");
    assert_eq!(
        (models, cards.len(), level.len()),
        (67, 33, 55),
        "AE.5's and AG.19's counts"
    );
}
