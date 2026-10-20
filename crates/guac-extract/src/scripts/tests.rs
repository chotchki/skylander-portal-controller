//! The whitelist and the script parts' readers on the builders' synthetic
//! files (AH.22): the roster builder's three global archives, AH.13's
//! writer's creature and the model builder's character with a script that
//! hangs things on it.

use guac_formats::iga::builder::IgaBuilder;
use guac_formats::model::builder as model;
use guac_formats::roster::builder::{disc, sample};
use guac_ir::Sha256;

use super::*;

fn identity() -> DumpIdentity {
    DumpIdentity {
        title_id: "BLUS30968".into(),
        app_ver: "01.00".into(),
        manifest_hash: Sha256([9; 32]),
    }
}

#[test]
fn the_whitelist_lists_each_archive_once_by_file_name() {
    let archives: Vec<&str> = WHITELIST.iter().map(|w| w.archive).collect();
    assert_eq!(
        archives,
        [
            "permanent/bootstrap.bld",
            "permanent/global.bld",
            "character/init_setup.bld",
            "character/009_eruptor.bld",
            "level/level_213_tutotial.bld",
        ]
    );
    // AH.12's roots: 17 running in global.bld and 5 it spawns, Eruptor's
    // eight, the level's 18 and the 15 they spawn there.
    let counts: Vec<usize> = WHITELIST.iter().map(|w| w.scripts.len()).collect();
    assert_eq!(counts, [1, 22, 3, 8, 33]);
    for w in WHITELIST {
        let mut names: Vec<String> = w.scripts.iter().map(|s| s.to_lowercase()).collect();
        names.sort();
        names.dedup();
        assert_eq!(names.len(), w.scripts.len(), "{} twice", w.archive);
        assert!(
            w.scripts.iter().all(|s| s.ends_with(".ai")),
            "{}",
            w.archive
        );
    }
    // By the source path's file name, any case, never a suffix of one.
    let level = level_scripts("level_213_tutotial");
    assert!(listed(
        level,
        "C:/tfb/Content/Levels/Level_213/Scripts/Scale.ai"
    ));
    assert!(listed(level, "c:\\tfb\\scripts\\SCALE.AI"));
    assert!(!listed(
        level,
        "C:/tfb/Content/Levels/Level_213/Scripts/ShakeScale.ai"
    ));
    assert!(!listed(level, "Scale.ai.bak"));
    assert!(level_scripts("level_999_elsewhere").is_empty());
    // The fighters' archives as the disc names them.
    assert_eq!(
        character_archive("009_Eruptor"),
        "character/009_eruptor.bld"
    );
    assert!(
        WHITELIST
            .iter()
            .any(|w| w.archive == character_archive(FIGHTERS[0]))
    );
}

#[test]
fn the_parts_files_are_named_by_their_archives() {
    assert_eq!(
        archives(),
        [
            "permanent/bootstrap.bld",
            "permanent/global.bld",
            "character/init_setup.bld",
            "character/009_eruptor.bld",
        ]
    );
    assert_eq!(
        spawn_archives(),
        ["permanent/global.bld", "character/009_eruptor.bld"]
    );
    let scripts: Vec<String> = archives().iter().map(|a| script_path(a)).collect();
    assert_eq!(
        scripts,
        [
            "scripts/bootstrap.igz",
            "scripts/global.igz",
            "scripts/init_setup.igz",
            "scripts/009_eruptor.igz",
        ]
    );
    use crate::intros::IrFormat;
    assert_eq!(
        IrFormat::Ron.spawn_path("permanent/global.bld"),
        "spawns/global.ron"
    );
    assert_eq!(
        IrFormat::Postcard.spawn_path("character/009_eruptor.bld"),
        "spawns/009_eruptor.postcard"
    );
    assert_eq!(
        IrFormat::Ron.palette_path("009_Eruptor"),
        "palettes/009_Eruptor.ron"
    );
}

#[test]
fn an_entry_is_checked_by_ah13s_readers() {
    let d = disc(&sample());
    let names = check(&d.global).unwrap();
    assert_eq!(
        names,
        ["C:/tfb/Content/Levels/Includes/Spyro_GenericStuff/scripts/Spyro_PortalManager.ai"]
    );
    assert_eq!(check(&d.init_setup).unwrap().len(), 3);
    assert_eq!(check(&d.bootstrap).unwrap(), Vec::<String>::new());
    let (creature, _) = guac_formats::script::builder::creature();
    assert_eq!(
        check(&creature).unwrap(),
        ["C:/tfb/Content/Levels/Synthetic/Creature.ai"]
    );
    // Not an IGZ; a script with no op list (AG.3's sample).
    assert!(check(b"not an igz").is_err());
    let bare = guac_formats::level::builder::sample().bytes;
    assert!(check(&bare).unwrap_err().contains("_opList"));
}

#[test]
fn an_entry_is_the_archives_level_decompressed() {
    let tmp = tempfile::tempdir().unwrap();
    let d = disc(&sample());
    for (path, bytes) in d.archives() {
        let path = tmp.path().join(path);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, bytes).unwrap();
    }
    assert_eq!(entry(tmp.path(), GLOBAL).unwrap(), d.global);
    // No archive; an archive with no level in it.
    let e = entry(tmp.path(), "character/009_eruptor.bld").unwrap_err();
    assert!(e.contains("009_eruptor.bld"), "{e}");
    std::fs::write(
        tmp.path().join(INIT_SETUP),
        IgaBuilder::new().stored("other.igz", b"x").build(),
    )
    .unwrap();
    assert_eq!(entry(tmp.path(), INIT_SETUP).unwrap_err(), "no level.bld");
}

#[test]
fn spawn_models_are_every_placements_but_the_fighters_body() {
    let bytes: &'static [u8] = Vec::leak(model::character(&model::attached()));
    let igz = Igz::parse(bytes).unwrap();
    let archive = "character/009_eruptor.bld";
    let hero = body(&igz, archive);
    assert!(hero.is_some());
    let r = spawn_models(&igz, archive, identity(), hero).unwrap();
    assert_eq!(
        (r.ir.archive.as_str(), r.ir.identity.clone()),
        (archive, identity())
    );
    let paths: Vec<&str> = r.ir.props.iter().map(|p| p.path.as_str()).collect();
    assert_eq!(
        paths,
        [
            "Models/Characters/Knife.mdl",
            "Models/Characters/KnifeUpgrade1.mdl",
            "Models/Characters/Armor.mdl",
            "Models/Characters/Gun.mdl",
            "Models/Characters/Spear.mdl",
            "Models/Characters/Boomerang.mdl",
            "Models/Characters/Hat.mdl",
        ]
    );
    assert!(r.ir.refused.is_empty(), "{:?}", r.ir.refused);
    // Each binds the attachment colour: one texture file.
    assert_eq!(r.textures.len(), 1);
    // Without the body left out, the body is a skinned prop among them.
    let all = spawn_models(&igz, archive, identity(), None).unwrap();
    assert_eq!(all.ir.props.len(), 8);
    let eruptor = &all.ir.props[0];
    assert_eq!(eruptor.path, "Models/Characters/Eruptor/Eruptor.mdl");
    assert!(eruptor.skeleton.is_some() && eruptor.idle.is_some());
    // global.bld's sample places no model; an archive with no portal model
    // has no body.
    let global = disc(&sample()).global;
    let g = Igz::parse(&global).unwrap();
    assert!(body(&g, GLOBAL).is_none());
    let r = spawn_models(&g, GLOBAL, identity(), None).unwrap();
    assert!(r.ir.props.is_empty() && r.textures.is_empty());
}

#[test]
fn complete_wants_both_parts() {
    let mut m: Manifest = serde_json::from_value(serde_json::json!({
        "extractor": "0", "layout": 4, "ir_version": 5,
        "identity": identity(), "source_dir": "/d", "complete": true,
        "sources": [], "outputs": []
    }))
    .unwrap();
    assert!(!complete(&m));
    m.parts.insert(SCRIPTS.into(), Vec::new());
    assert!(!complete(&m), "the spawn models missing");
    m.parts.insert(SPAWNS.into(), Vec::new());
    assert!(complete(&m));
}
