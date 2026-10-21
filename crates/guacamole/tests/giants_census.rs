//! The census tables against the disc (openspec AE.25, AG.43): every key the
//! 01.00 roster's figures draw on a card (each character model with what a
//! fresh figure of it carries, AE.23) and every key chapter 1 draws, read off
//! `GUAC_GIANTS_DUMP` through the extractor's own readers (in memory, no
//! extract written) and keyed by the TV's own derivation (the warm's for a
//! card, the level loader's for chapter 1, its props and what its scripts
//! spawn), equal to
//! `guacamole::census`'s tables. A table that drifts prints the rows the
//! dump gives, in the table's syntax. And every character that walks
//! (AG.21), drawn in the level as the hero, keys nothing the card tables
//! don't hold.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::time::Instant;

use guac_extract::{levels, models, roster};
use guac_formats::igz::Igz;
use guac_ir::shading::Scene;
use guac_ir::{DumpIdentity, Sha256};
use guacamole::census::{self, Row};
use guacamole::character::attach;
use guacamole::character::turntable::rig_scene;
use guacamole::character::warm::{self, WarmKey};
use guacamole::level_scene;
use guacamole::props::LevelProps;
use guacamole::walkers;

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

/// What the card side read: its keys, the models and what they carry.
struct Cards {
    keys: Vec<WarmKey>,
    /// Each walker's keys in the level as the hero.
    heroes: Vec<(String, Vec<WarmKey>)>,
    models: usize,
    /// Attached models over every model, and those the turntable draws
    /// nothing of (helpers whose material it doesn't read).
    carried: usize,
    undrawn: usize,
}

/// Every key the roster's figures draw on a card, as the warm reads them
/// off a shelf: each model, with what a fresh figure carries (read as the
/// model stage reads it, with `global.bld`'s templates), keyed by the
/// warm's own derivation in the rig's scene.
fn card_keys(dump: &Path, hero_scene: &Scene) -> Cards {
    let usrdir = dump.join("PS3_GAME/USRDIR");
    let roster = roster::read(dump, identity()).expect("the roster");
    let level_of = |archive: &str| {
        let bytes =
            std::fs::read(usrdir.join(archive)).unwrap_or_else(|e| panic!("{archive}: {e}"));
        models::level(&bytes, archive).unwrap_or_else(|e| panic!("{archive}: {e}"))
    };
    let (global, bootstrap) = (level_of(roster::GLOBAL), level_of(roster::BOOTSTRAP));
    let globals = models::Globals::new(&global, &bootstrap).expect("global's templates");
    let rig = models::rig(&global, identity()).expect("the sprite rig");
    let scene = rig_scene(&rig).expect("the rig's scene");
    let mut cards = Cards {
        keys: Vec::new(),
        heroes: Vec::new(),
        models: 0,
        carried: 0,
        undrawn: 0,
    };
    for load in &models::character_loads(&roster) {
        let path = models::archive_path(&usrdir, load);
        let archive = models::archive(&path, load).expect("the archive");
        let level = models::level(&archive, load).expect("its level");
        let model = models::read(&level, load, identity(), Some(&globals))
            .unwrap_or_else(|e| panic!("{load}: {e}"))
            .model;
        assert!(
            model.attachments_unsettled.is_none(),
            "{load}: {:?}",
            model.attachments_unsettled
        );
        cards.models += 1;
        cards.carried += model.attachments.len();
        cards.undrawn += attach::segments(&model, &attach::drawn(&model))
            .iter()
            .filter(|s| s.is_empty())
            .count();
        for (k, _) in warm::keys(&model, &scene).unwrap_or_else(|e| panic!("{load}: {e}")) {
            if !cards.keys.contains(&k) {
                cards.keys.push(k);
            }
        }
        if walkers::walker(load).is_some() {
            let keys = warm::keys(&model, hero_scene).unwrap_or_else(|e| panic!("{load}: {e}"));
            cards
                .heroes
                .push((load.to_string(), keys.into_iter().map(|(k, _)| k).collect()));
        }
    }
    cards
}

/// What chapter 1 keys: its own segments', its props', what its scripts
/// spawn, how many materials they draw, and the scene the hero draws in
/// there.
struct Chapter1 {
    keys: Vec<WarmKey>,
    props: Vec<WarmKey>,
    spawns: Vec<WarmKey>,
    materials: usize,
    prop_models: usize,
    spawn_models: usize,
    hero: Scene,
}

/// The spawn part's models (AH.22) off the dump through the extractor's own
/// reader, every spawn archive's, a character's body left out, as the TV
/// draws them (AH.24).
fn spawn_models(dump: &Path) -> Vec<guacamole::props::PropModel> {
    let usrdir = dump.join("PS3_GAME/USRDIR");
    let mut out = Vec::new();
    for archive in guac_extract::scripts::spawn_archives() {
        let entry = guac_extract::scripts::entry(&usrdir, &archive)
            .unwrap_or_else(|e| panic!("{archive}: {e}"));
        let igz = Igz::parse(&entry).unwrap_or_else(|e| panic!("{archive}: {e}"));
        let body = guac_extract::scripts::body(&igz, &archive);
        let read = guac_extract::scripts::spawn_models(&igz, &archive, identity(), body)
            .unwrap_or_else(|e| panic!("{archive}: {e}"));
        out.extend(
            guacamole::props::spawn_models(&read.ir).unwrap_or_else(|e| panic!("{archive}: {e}")),
        );
    }
    out
}

/// Every key chapter 1 draws, as the level loader keys it (through its own
/// `level_scene::at_start`): its drawn segments in its scene at the fresh
/// state's camera, lit and fogged by its start's zone (AG.41), the sky
/// unfogged as that zone has it; its props' models in the same scene
/// (`props::LevelProps`, AG.25); and the scene the hero draws in there.
fn level_keys(dump: &Path) -> Chapter1 {
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
    let props = levels::props(&igz, level, identity())
        .expect("the props")
        .ir;
    let placements = levels::placements(&igz, level, identity())
        .expect("the placements")
        .ir;
    let start = level_scene::at_start(scene, &lights, &atmosphere).expect("chapter 1 at its start");
    let view = level_scene::view_scene(&start.lights, &start.fog, level_scene::fresh_state_view());
    let props = LevelProps::new(&props, &placements).expect("chapter 1's props");
    let hero = level_scene::hero_scene(&start.lights, &start.fog, level_scene::fresh_state_view());
    let spawned = spawn_models(dump);
    Chapter1 {
        keys: start.keys.into_iter().map(|(k, _)| k).collect(),
        props: props.keys(&view).expect("the props' keys"),
        spawns: guacamole::props::models_keys(&spawned, &view).expect("the spawn models' keys"),
        materials: start.drawn.materials.len(),
        prop_models: props.models.len(),
        spawn_models: spawned.len(),
        hero,
    }
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
    let Chapter1 {
        keys: level,
        props,
        spawns,
        materials,
        prop_models,
        spawn_models,
        hero: hero_scene,
    } = level_keys(&dump);
    let level_took = started.elapsed();
    // The props' keys the level's table doesn't hold already.
    let props_beyond: Vec<WarmKey> = props
        .iter()
        .filter(|k| !census::LEVEL.contains(&Row::of(k)))
        .copied()
        .collect();
    // What the scripts spawn keys past the level's and its props' (AH.24).
    let spawns_beyond: Vec<WarmKey> = spawns
        .iter()
        .filter(|k| {
            let r = Row::of(k);
            !census::LEVEL.contains(&r) && !census::LEVEL_PROPS.contains(&r)
        })
        .copied()
        .collect();
    eprintln!(
        "what the scripts spawn: {} keys off {spawn_models} models, {} past the level's and its props'",
        spawns.len(),
        spawns_beyond.len()
    );
    let started = Instant::now();
    let cards = card_keys(&dump, &hero_scene);
    let cards_took = started.elapsed();
    let (skinned, unskinned): (Vec<WarmKey>, Vec<WarmKey>) =
        cards.keys.iter().partition(|k| k.mesh.skinned);
    eprintln!(
        "cards: {} keys ({} skinned, {} static) off {} models carrying {} ({} drawing nothing) in {:.1} s; chapter 1: {} keys off {materials} materials, its props {} off {prop_models} models ({} beyond the level's), in {:.1} s",
        cards.keys.len(),
        skinned.len(),
        unskinned.len(),
        cards.models,
        cards.carried,
        cards.undrawn,
        cards_took.as_secs_f64(),
        level.len(),
        props.len(),
        props_beyond.len(),
        level_took.as_secs_f64()
    );
    let checks = [
        check("cards, skinned", &skinned, census::TURNTABLE, true),
        check("cards, static", &unskinned, census::TURNTABLE_STATIC, false),
        check("chapter 1", &level, census::LEVEL, false),
        check(
            "chapter 1's props",
            &props_beyond,
            census::LEVEL_PROPS,
            false,
        ),
        check(
            "what the scripts spawn",
            &spawns_beyond,
            census::SPAWNS,
            false,
        ),
    ];
    for c in &checks {
        if let Err(why) = c {
            eprintln!("{why}");
        }
    }
    assert!(checks.iter().all(Result::is_ok), "a table isn't the dump's");
    // The hero's keys are his card's: the start's pass compiled them.
    assert_eq!(
        cards
            .heroes
            .iter()
            .map(|(l, _)| l.as_str())
            .collect::<Vec<_>>(),
        walkers::WALKERS
            .iter()
            .map(|w| w.load_file)
            .collect::<Vec<_>>(),
        "every walker's model read"
    );
    for (load, keys) in &cards.heroes {
        let outside: Vec<String> = keys
            .iter()
            .filter(|k| {
                let table = if k.mesh.skinned {
                    census::TURNTABLE
                } else {
                    census::TURNTABLE_STATIC
                };
                !table.contains(&Row::of(k))
            })
            .map(|k| Row::of(k).source())
            .collect();
        eprintln!(
            "{load} as the hero: {} keys, {} outside the card tables",
            keys.len(),
            outside.len()
        );
        assert!(outside.is_empty(), "{load} as the hero: {outside:?}");
    }
    assert_eq!(
        (cards.models, cards.carried, cards.undrawn),
        (67, 102, 3),
        "AE.5's models, what AE.23 found them carrying, and the global inviso.mdl's three"
    );
    assert_eq!(
        (skinned.len(), unskinned.len(), level.len()),
        (33, 24, 55),
        "the census's counts"
    );
    assert_eq!(
        (prop_models, props.len(), props_beyond.len()),
        (46, 41, 32),
        "chapter 1's props: the models that can stand, their keys, those past the level's"
    );
    assert_eq!(
        (spawn_models, spawns.len(), spawns_beyond.len()),
        (13, 10, 5),
        "what the scripts spawn: the spawn part's models, their keys, those past the level's and its props'"
    );
}
