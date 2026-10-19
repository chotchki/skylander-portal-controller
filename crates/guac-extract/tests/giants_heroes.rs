//! AG.14's hero census off the real Giants dump (design decisions 8 and
//! 14): for every character load file the roster names, the hero side the
//! model stage writes beside its model (the placement the archive plays
//! the character as, its solid and disc movement parameters, the clips in
//! the locomotion slots), or why it doesn't settle. It asserts that every
//! modelled character resolves a shape and its clips or is listed, and
//! Eruptor's, Tree Rex's and Punch Pop Fizz's facts, and prints the rest
//! for AG.14's tick: which gaits each character lacks, which slots hold a
//! clip named for another gait, the slots' labels, the shapes and the
//! movement numbers. Ignored by default: it reads `GUAC_GIANTS_DUMP` from
//! the environment or the workspace's `.env.dev`:
//!
//! `cargo test -p guac-extract --release --test giants_heroes -- --ignored --nocapture`

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::Instant;

use guac_extract::known::KNOWN_DUMPS;
use guac_extract::{models, roster};
use guac_formats::igz::Igz;
use guac_formats::model::{portal_model, skeleton};
use guac_ir::actor::{Movement, Shape};
use guac_ir::hero::Gait;
use guac_ir::{DumpIdentity, Hero, Sha256};

fn env_or_dotenv(key: &str) -> Option<String> {
    if let Ok(value) = std::env::var(key) {
        return Some(value);
    }
    let dotenv = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../.env.dev");
    let text = std::fs::read_to_string(dotenv).ok()?;
    text.lines().find_map(|line| {
        let (k, v) = line.trim().split_once('=')?;
        (k.trim() == key).then(|| v.trim().trim_matches('"').to_owned())
    })
}

/// The word a gait's clip name carries when it's named for it.
fn word(g: Gait) -> &'static str {
    match g {
        Gait::Walk => "walk",
        Gait::Jog => "jog",
        Gait::Run => "run",
        Gait::Falling => "fall",
        Gait::Landing => "land",
    }
}

/// The 13 speeds, accelerations and turn rates, as ag-levels' table lists
/// them.
fn numbers(m: &Movement) -> [f32; 13] {
    [
        m.ground_max_speed,
        m.ground_acceleration,
        m.ground_deceleration,
        m.ground_turn_rate,
        m.air_max_speed,
        m.air_acceleration,
        m.air_deceleration,
        m.air_turn_rate,
        m.gravity_max_speed,
        m.gravity_acceleration,
        m.jump_max_speed,
        m.jump_acceleration,
        m.jump_initial_speed,
    ]
}

#[test]
#[ignore = "reads the Giants dump at GUAC_GIANTS_DUMP"]
fn every_modelled_character_settles_a_hero_or_is_listed() {
    let dump = PathBuf::from(env_or_dotenv("GUAC_GIANTS_DUMP").expect("set GUAC_GIANTS_DUMP"));
    let started = Instant::now();
    let known = &KNOWN_DUMPS[0];
    let identity = DumpIdentity {
        title_id: known.title_id.into(),
        app_ver: known.app_ver.into(),
        manifest_hash: Sha256([0; 32]),
    };
    let roster = roster::read(&dump, identity.clone()).unwrap();
    let usrdir = dump.join("PS3_GAME/USRDIR");
    let loads = models::character_loads(&roster);
    assert_eq!(loads.len(), 67);
    // Each load file's first figure, by name, so the lists read.
    let named = |load: &str| -> String {
        let name = roster
            .figures
            .iter()
            .find(|f| f.load_file == load)
            .and_then(|f| roster.display_name(f, guac_ir::roster::ENGLISH))
            .unwrap_or_default();
        format!("{load} ({name})")
    };

    let mut heroes: BTreeMap<String, Hero> = BTreeMap::new();
    let mut listed: Vec<(String, String)> = Vec::new();
    let mut by_name = Vec::new();
    let mut hero_time = 0.0;
    for load in &loads {
        let archive = models::archive(&models::archive_path(&usrdir, load), load).unwrap();
        let level = models::level(&archive, load).unwrap();
        let igz = Igz::parse(&level).unwrap();
        let portal = portal_model(&igz).unwrap();
        let sk = skeleton(&igz, &portal.hero).unwrap();
        let t = Instant::now();
        let hero = models::hero(&igz, &portal, &sk, load, identity.clone());
        hero_time += t.elapsed().as_secs_f64();
        match hero {
            Ok(h) => {
                let drawn = guac_formats::model::hero_actor(&igz, &portal, load)
                    .unwrap()
                    .info
                    .model;
                if drawn != Some(portal.model) {
                    by_name.push(format!("{load} ({}, model {:?})", h.actor, drawn));
                }
                heroes.insert((*load).to_owned(), h);
            }
            Err(e) => listed.push(((*load).to_owned(), e.to_string())),
        }
    }

    println!(
        "== {} of {} characters settle a hero, {} listed; the hero side read in {hero_time:.2} s of {:.2} s",
        heroes.len(),
        loads.len(),
        listed.len(),
        started.elapsed().as_secs_f64()
    );
    for (load, why) in &listed {
        println!("  LISTED {load}: {why}");
    }
    println!("  found by the load file's name: {by_name:?}");
    let shapes_by: Vec<String> = heroes
        .iter()
        .filter(|(_, h)| h.solid.shape == Shape::Capsule)
        .map(|(l, _)| named(l))
        .collect();
    println!("  capsules ({}): {shapes_by:?}", shapes_by.len());

    // Per character: the actor, its solid, its movement and its gaits.
    let mut shapes: BTreeMap<String, usize> = BTreeMap::new();
    let mut lacking: BTreeMap<Gait, Vec<String>> = BTreeMap::new();
    let mut misnamed: Vec<String> = Vec::new();
    let mut labels: Vec<String> = Vec::new();
    let mut scales: BTreeMap<String, usize> = BTreeMap::new();
    for (load, h) in &heroes {
        let s = &h.solid;
        *shapes
            .entry(format!(
                "{:?} solid={} world={} surface={}",
                s.shape, s.solid, s.world, s.actor_surface
            ))
            .or_default() += 1;
        *scales.entry(format!("{:?}", h.scale)).or_default() += 1;
        let gaits: Vec<String> = Gait::ALL
            .into_iter()
            .map(|g| match h.locomotion.get(g) {
                Some(c) => format!("{g:?}={}@{}({:.4}s)", c.clip.name, c.slot, c.clip.duration),
                None => format!("{g:?}=-"),
            })
            .collect();
        println!(
            "  {load}\t{}\t{:?} {} x {} x {} ft {} lb offset {:?}\t{:?} motion {} tilt {} ts {}\t{}",
            h.actor,
            s.shape,
            s.width,
            s.height,
            s.length,
            s.weight,
            s.offset,
            numbers(&h.movement),
            h.movement.motion_type,
            h.movement.tilts_with_ground,
            h.movement.time_scale,
            gaits.join(" ")
        );
        for g in Gait::ALL {
            match h.locomotion.get(g) {
                None => lacking.entry(g).or_default().push(named(load)),
                Some(c) => {
                    if !c.clip.name.to_ascii_lowercase().contains(word(g)) {
                        misnamed.push(format!("{} {g:?}: {}", named(load), c.clip.name));
                    }
                    if let Some(l) = &c.label {
                        labels.push(format!("{load} {g:?}: {l}"));
                    }
                }
            }
        }
    }
    println!("  shapes: {shapes:?}");
    println!("  scales: {scales:?}");
    for (g, loads) in &lacking {
        println!("  without a {g:?} ({}): {loads:?}", loads.len());
    }
    println!(
        "  named for another gait ({}): {misnamed:#?}",
        misnamed.len()
    );
    println!("  labelled ({}): {labels:#?}", labels.len());

    // Every modelled character resolves a shape and its clips.
    assert!(listed.is_empty(), "{listed:#?}");
    assert_eq!(heroes.len(), 67);
    assert!(heroes.values().all(|h| h.solid.solid && h.solid.world));
    assert!(
        heroes
            .values()
            .all(|h| h.locomotion.walk.is_some() || h.locomotion.run.is_some())
    );
    assert!(heroes.values().all(|h| h.locomotion.falling.is_some()));

    // Eruptor: a sphere 4 ft wide on his disc movement (18 ft/s and 360 a
    // second, against the 21.5 and 1440 his scripts set), walking from slot 1
    // and running from slot 3 at the speeds their labels give, no jog or
    // landing.
    let e = &heroes["009_Eruptor"];
    assert_eq!(e.actor, "009_Eruptor");
    assert_eq!(e.scale, [100.0; 3]);
    assert_eq!(
        (
            e.solid.shape,
            e.solid.width,
            e.solid.height,
            e.solid.length,
            e.solid.weight
        ),
        (Shape::Sphere, 4.0, 6.0, 3.0, 10.0)
    );
    assert_eq!(
        numbers(&e.movement),
        [
            18.0, 160.0, 80.0, 360.0, 30.0, 160.0, 20.0, 360.0, 120.0, 128.0, 0.0, 0.0, 0.0
        ]
    );
    assert!(e.movement.tilts_with_ground && e.movement.motion_type == 0);
    let clip = |h: &Hero, g: Gait| {
        h.locomotion
            .get(g)
            .map(|c| (c.slot, c.clip.name.clone(), c.label.clone()))
    };
    assert_eq!(
        clip(e, Gait::Walk),
        Some((1, "Eruptor_Walk.ma".into(), Some("3.658 m/sec".into())))
    );
    assert_eq!(
        clip(e, Gait::Run),
        Some((3, "Eruptor_Run.ma".into(), Some("6.401 m/sec".into())))
    );
    assert_eq!(
        clip(e, Gait::Falling),
        Some((57, "Eruptor_Falling.ma".into(), None))
    );
    assert_eq!((clip(e, Gait::Jog), clip(e, Gait::Landing)), (None, None));

    // Tree Rex: a Giant's capsule, 14 x 21.25 ft, 25 ft/s turning 1440 on
    // the disc (his scripts make it 20 and 360), and a landing.
    let t = &heroes["112_TitanLife"];
    assert_eq!(t.actor, "112_TitanLife");
    assert_eq!(
        (t.solid.shape, t.solid.width, t.solid.height),
        (Shape::Capsule, 14.0, 21.25)
    );
    assert_eq!(
        (t.movement.ground_max_speed, t.movement.ground_turn_rate),
        (25.0, 1440.0)
    );
    assert_eq!(
        clip(t, Gait::Landing),
        Some((61, "TitanLife_Landing.ma".into(), None))
    );

    // Punch Pop Fizz: no actor draws her portal model, so her hero goes by
    // her load file's name; the only one.
    assert_eq!(by_name.len(), 1, "{by_name:?}");
    assert!(by_name[0].starts_with("108_Alchemist_Red"), "{by_name:?}");
    println!("took {:.2} s in all", started.elapsed().as_secs_f64());
}
