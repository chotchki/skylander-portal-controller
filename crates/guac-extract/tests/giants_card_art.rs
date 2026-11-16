//! The card game's art on the real Giants dump (openspec AS.365): the part
//! read into a throwaway identity dir, in the release build's format, then
//! taken off the manifest (an extract made before it) and read again alone.
//! Checked: the board's three meshes and two grids read with every texture
//! their materials name in the extract, each stone's deck and asset read
//! (the deck's face model, the asset's sound), the sounds where the TV's
//! book looks for their stems, every refusal named in the record. Printed:
//! each run's time, files and bytes with the load average, and the
//! refusals. Ignored by default: it reads `GUAC_GIANTS_DUMP` and writes
//! under `GUAC_EXTRACT_DIR` (removed after), from the environment or the
//! workspace's `.env.dev`:
//!
//! `cargo test -p guac-extract --profile disc --test giants_card_art -- --ignored --nocapture`

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::Instant;

use guac_extract::intros::IrFormat;
use guac_extract::known::KNOWN_DUMPS;
use guac_extract::screens::{
    CARD_ART_PART, CARD_ART_RECORD, CARD_GAME, CARD_STONE_COUNT, CardArtRecord, card_asset,
    card_deck, card_model_path,
};
use guac_extract::stages::{ScriptStage, card_art_part};
use guac_extract::{DirState, ExtractRoot, open};
use guac_ir::level::LevelMaterial;
use guac_ir::spawn::SpawnModels;

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

fn load_average() -> String {
    std::process::Command::new("sysctl")
        .args(["-n", "vm.loadavg"])
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_owned())
        .unwrap_or_default()
}

const IR: IrFormat = IrFormat::Postcard;

/// The board's pieces the in-game templates name.
const MESHES: [&str; 5] = [
    "boardBlocker",
    "skyStone",
    "skyStoneWinStack",
    "cardGrid3x3",
    "cardGrid4x3",
];

/// Every texture a model's materials name, tfbmulti2's and the other effects' alike.
fn textures(p: &guac_ir::level::Prop) -> Vec<String> {
    p.materials
        .iter()
        .flat_map(|m| match m {
            LevelMaterial::Multi2(m) => m.textures.iter().map(|t| t.texture.clone()).collect(),
            LevelMaterial::Effect(e) => e
                .textures
                .iter()
                .map(|t| t.texture.clone())
                .collect::<Vec<_>>(),
        })
        .collect()
}

fn forget_part(dir: &Path) {
    let path = dir.join("manifest.json");
    let mut json: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
    json["parts"]
        .as_object_mut()
        .unwrap()
        .remove(CARD_ART_PART)
        .unwrap();
    std::fs::write(&path, serde_json::to_string(&json).unwrap()).unwrap();
}

#[test]
#[ignore = "reads GUAC_GIANTS_DUMP; run on the Mac"]
fn the_card_games_art_extracts_and_resumes_alone() {
    let dump = PathBuf::from(env_or_dotenv("GUAC_GIANTS_DUMP").expect("set GUAC_GIANTS_DUMP"));
    let base = PathBuf::from(env_or_dotenv("GUAC_EXTRACT_DIR").expect("set GUAC_EXTRACT_DIR"));
    std::fs::create_dir_all(&base).unwrap();
    let tmp = tempfile::Builder::new()
        .prefix("as365-card-art-")
        .tempdir_in(&base)
        .unwrap();
    let root = ExtractRoot::checked(tmp.path().join("extract")).unwrap();
    let mut opened = open(&dump, &root, KNOWN_DUMPS, &|_| {}).expect("the known dump");
    assert_eq!(opened.state, DirState::Fresh);
    let usr = dump.join("PS3_GAME/USRDIR");
    let usr = if usr.is_dir() { usr } else { dump.clone() };
    let dir_path = opened.dir.path().to_path_buf();

    println!("load average before the part: {}", load_average());
    let started = Instant::now();
    let mut st = ScriptStage::default();
    card_art_part(&mut opened.dir, &usr, IR, &mut st).unwrap();
    let art = st.card_art.clone();
    println!(
        "  first run: {} archives, {} models, {} textures, {} interface records, {} sounds; {:?} in {:.2} s; load average {}",
        art.archives,
        art.models,
        art.textures,
        art.interfaces,
        art.sounds,
        art.files,
        started.elapsed().as_secs_f64(),
        load_average()
    );
    assert!(st.failed.is_empty(), "{:?}", st.failed);
    assert!(art.read && art.missing.is_empty(), "{:?}", art.missing);
    assert_eq!(art.archives, CARD_GAME.len() + 2 * CARD_STONE_COUNT);
    let mut by_why: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for (what, why) in &art.refused {
        by_why.entry(why.clone()).or_default().push(what.clone());
    }
    for (why, whats) in &by_why {
        println!("  refused x{} ({why}): {whats:?}", whats.len());
    }

    // The board's pieces, each read with every texture it names.
    let board: SpawnModels = IR
        .decode(&opened.dir, &card_model_path(IR, CARD_GAME[1]))
        .unwrap();
    for mesh in MESHES {
        let p = board
            .props
            .iter()
            .find(|p| p.path.ends_with(&format!("/{mesh}.mdl")))
            .unwrap_or_else(|| panic!("{mesh} not read: {:?}", board.refused));
        let named = textures(p);
        assert!(!p.segments.is_empty() && !named.is_empty(), "{mesh}");
        for t in &named {
            assert!(dir_path.join(t).is_file(), "{mesh}'s {t}");
        }
        println!(
            "  {mesh}: {} segments, {} materials, {} textures, skinned {}",
            p.segments.len(),
            p.materials.len(),
            named.len(),
            p.skeleton.is_some()
        );
    }

    // Every stone: its deck's face model read, its asset's sound kept
    // where the TV's book looks for the asset's stem.
    let record: CardArtRecord =
        serde_json::from_slice(&std::fs::read(dir_path.join(CARD_ART_RECORD)).unwrap()).unwrap();
    for n in 0..CARD_STONE_COUNT {
        let deck: SpawnModels = IR
            .decode(&opened.dir, &card_model_path(IR, &card_deck(n)))
            .unwrap();
        let [face] = &deck.props[..] else {
            panic!("stone {n}'s deck: {:?}", deck.refused)
        };
        for t in textures(face) {
            assert!(dir_path.join(&t).is_file(), "stone {n}'s face {t}");
        }
        let asset = card_asset(n);
        let line = record.archives.iter().find(|l| l.archive == asset).unwrap();
        assert_eq!(line.sounds.len(), 1, "stone {n}: {:?}", line.sounds);
        assert!(
            dir_path
                .join(guac_extract::sounds::path(IR, &asset))
                .is_file()
        );
    }
    let refused: usize = record.archives.iter().map(|l| l.refused.len()).sum();
    assert_eq!(refused, art.refused.len(), "the record names every refusal");

    // An extract made before the part reads it alone at its next start.
    forget_part(&dir_path);
    let mut reopened = open(&dump, &root, KNOWN_DUMPS, &|_| {}).unwrap();
    let started = Instant::now();
    let mut again = ScriptStage::default();
    card_art_part(&mut reopened.dir, &usr, IR, &mut again).unwrap();
    println!(
        "  caught up: {:?} in {:.2} s; load average {}",
        again.card_art.files,
        started.elapsed().as_secs_f64(),
        load_average()
    );
    assert!(again.card_art.read && again.card_art.files.written == 0);
    let mut third = ScriptStage::default();
    card_art_part(&mut reopened.dir, &usr, IR, &mut third).unwrap();
    assert!(third.card_art.kept, "a second start finds it current");
}
