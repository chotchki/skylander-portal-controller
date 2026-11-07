//! What every level's materials leave to the scene (openspec AS.56): each
//! material's masks over the whole disc, the bits past those the TV's scene
//! answers. Ignored by default; reads `GUAC_GIANTS_DUMP` (process env, else
//! `.env.dev`):
//!
//! `cargo test -p guacamole --profile disc --test giants_scene_bits -- --ignored --nocapture`

use std::collections::BTreeMap;

use guac_extract::levels;
use guac_formats::igz::Igz;
use guac_ir::level::LevelMaterial;
use guac_ir::model::Mask;
use guacamole::character::key::bit;

mod common;

use common::*;

/// The shader's 14 feature bits on each side.
const BITS: u32 = (1 << 14) - 1;

/// The bits the TV's scene answers: shadows, fog and the point lights.
const PIXEL_SCENE: u32 = 1 << bit::SHADOWS | 1 << bit::FOG | 0b111 << bit::POINT_LIGHTS;
const VERTEX_SCENE: u32 = 1 << bit::SHADOWS | 1 << bit::FOG | 1 << bit::VERTEX_POINT_LIT;

/// The bits of `mask` nothing owns but the scene, past those it answers.
fn left(mask: Mask, answered: u32) -> u32 {
    !mask.ignore & BITS & !answered
}

/// openspec AS.56: every material on the dump leaving the scene a bit the TV
/// doesn't model, by name with the levels it's on. Bit 9, reflections, is
/// the only one, on both masks of 15 of the dump's materials (the ship's
/// deck decal on 214 and the four hub stages among them), and the TV draws
/// them with it off: the original's scene raises it only for a receiver a
/// planar reflection is cast on, which nothing on the dump places (the note
/// on AS.56; stand-in ledger: `reflections`).
#[test]
#[ignore = "reads the Giants dump"]
fn the_only_bit_left_to_the_scene_the_tv_doesnt_model_is_reflections() {
    println!("load at start: {}", load());
    let usrdir = dump().join("PS3_GAME/USRDIR");
    let mut names: Vec<String> = std::fs::read_dir(usrdir.join(levels::LEVEL_FOLDER))
        .unwrap()
        .filter_map(|e| {
            let name = e.unwrap().file_name().into_string().ok()?;
            name.strip_suffix(".bld").map(str::to_owned)
        })
        .collect();
    names.sort();
    let mut found: BTreeMap<(String, u32, u32), Vec<String>> = BTreeMap::new();
    let mut materials = 0usize;
    for level in &names {
        let archive = std::fs::read(levels::archive_path(&usrdir, level)).unwrap();
        let Ok(entry) = levels::level_bld(&archive) else {
            continue;
        };
        let Ok(igz) = Igz::parse(&entry) else {
            continue;
        };
        let mut all = Vec::new();
        if let Ok(scene) = levels::scene(&igz, level, identity()) {
            all.extend(scene.ir.scenes.into_iter().flat_map(|s| s.materials));
        }
        if let Ok(props) = levels::props(&igz, level, identity()) {
            all.extend(props.ir.props.into_iter().flat_map(|p| p.materials));
        }
        for m in all {
            materials += 1;
            let LevelMaterial::Multi2(m) = m else {
                continue;
            };
            let (p, v) = (left(m.pixel, PIXEL_SCENE), left(m.vertex, VERTEX_SCENE));
            if p != 0 || v != 0 {
                let on = found.entry((m.name.clone(), p, v)).or_default();
                if on.last() != Some(level) {
                    on.push(level.clone());
                }
            }
        }
    }
    println!("{} levels, {materials} materials", names.len());
    for ((name, p, v), on) in &found {
        println!(
            "LEFT\t{name}\t{p:#06x}\t{v:#06x}\t{}\t{}",
            on.len(),
            on.join(" ")
        );
    }
    let reflections = 1 << bit::REFLECTIONS;
    assert!(
        found.keys().all(|(_, p, v)| (*p | *v) & !reflections == 0),
        "a bit past reflections: the TV has to answer it or refuse it"
    );
    assert!(
        found.keys().any(|(n, ..)| n == "LIGHT_yellow_DUP_DUP"),
        "the deck's decal among them"
    );
    assert_eq!(found.len(), 15, "the census as AS.56 read it");
    println!("load at end: {}", load());
}
