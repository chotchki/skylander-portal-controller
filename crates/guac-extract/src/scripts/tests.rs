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

const WALKER_ARCHIVES: [&str; 66] = [
    "character/000_airdragon.bld",
    "character/000_airdragon_alt.bld",
    "character/001_gryphon.bld",
    "character/002_skyturtle.bld",
    "character/003_stormgiant.bld",
    "character/004_rock_dragon.bld",
    "character/005_landshark.bld",
    "character/006_dinorang.bld",
    "character/007_gemgolem.bld",
    "character/008_firedragon.bld",
    "character/010_flameknight.bld",
    "character/010_flameknight_legendary.bld",
    "character/011_fireelf.bld",
    "character/012_seadragon.bld",
    "character/013_crab.bld",
    "character/014_gillgrunt.bld",
    "character/015_yeti.bld",
    "character/015_yeti_legendary.bld",
    "character/016_spyro.bld",
    "character/017_skullorc.bld",
    "character/018_wizard.bld",
    "character/018_wizard_alt.bld",
    "character/023_forcefieldgrub.bld",
    "character/016_spyro_dark.bld",
    "character/020_metal_dragon.bld",
    "character/021_drillbot.bld",
    "character/022_bomb_troll.bld",
    "character/019_goldencheat.bld",
    "character/024_lifedragon.bld",
    "character/025_bambazooker.bld",
    "character/026_stealthelf.bld",
    "character/026_stealthelf_legendary.bld",
    "character/027_ent.bld",
    "character/032_cynder.bld",
    "character/029_shadowmaid.bld",
    "character/030_pandoranelite.bld",
    "character/031_ghosteater.bld",
    "character/016_spyro_legendary.bld",
    "character/030_pandoranelite_legendary.bld",
    "character/004_rock_dragon_legendary.bld",
    "character/019_goldencheat_legendary.bld",
    "character/100_eagle.bld",
    "character/100_eagle_legendary.bld",
    "character/101_bugmangiant.bld",
    "character/102_titan_earth.bld",
    "character/102_titan_earth_alt.bld",
    "character/103_earthdragon.bld",
    "character/103_earthdragon_alt.bld",
    "character/104_oilblazer.bld",
    "character/105_firefeet.bld",
    "character/105_firefeet_alt.bld",
    "character/106_icesorc.bld",
    "character/106_icesorc_legendary.bld",
    "character/107_whale.bld",
    "character/108_alchemist.bld",
    "character/108_alchemist_red.bld",
    "character/109_djini.bld",
    "character/109_djini_alt.bld",
    "character/110_robotgiant.bld",
    "character/110_robotgiant_legendary.bld",
    "character/111_turretmaster.bld",
    "character/112_titanlife.bld",
    "character/112_titanlife_alt.bld",
    "character/113_myconman.bld",
    "character/114_mummy.bld",
    "character/115_ostrich.bld",
];

const WALKER_COUNTS: [usize; 66] = [
    4, 4, 5, 4, 4, 3, 4, 3, 3, 3, 4, 4, 3, 3, 4, 3, 4, 4, 3, 4, 3, 3, 3, 3, 3, 3, 3, 3, 4, 5, 5, 5,
    5, 4, 3, 4, 4, 3, 4, 3, 3, 5, 5, 4, 4, 4, 3, 3, 3, 4, 4, 3, 3, 4, 4, 4, 4, 4, 3, 3, 4, 4, 4, 4,
    4, 4,
];

#[test]
fn the_whitelist_lists_each_archive_once_by_file_name() {
    let archives: Vec<&str> = WHITELIST.iter().map(|w| w.archive).collect();
    let base = [
        "permanent/bootstrap.bld",
        "permanent/global.bld",
        "character/init_setup.bld",
        "character/009_eruptor.bld",
    ];
    assert_eq!(archives[..4], base);
    assert_eq!(archives[70], "level/level_213_tutotial.bld");
    // AG.162: the 66 walkers' archives between them, the roster's order,
    // one entry each (never twice, never a fighter's).
    assert_eq!(archives[4..70], WALKER_ARCHIVES);
    assert_eq!(WHITELIST.len(), 71);
    let mut once = archives.clone();
    once.sort_unstable();
    once.dedup();
    assert_eq!(once.len(), archives.len(), "an archive listed twice");
    // AH.12's roots: 17 running in global.bld and 5 it spawns, Eruptor's
    // eight, the level's 18 and the 15 they spawn there; AG.60's 8 start
    // scripts on the props and the 5 they spawn; AG.28's 7 camera volumes
    // and the sliding rocks' activator; AG.177's goats; AH.42's two from
    // AH.8's route, the cannon's pan and the pit's intro.
    let counts: Vec<usize> = WHITELIST.iter().map(|w| w.scripts.len()).collect();
    assert_eq!(counts[..4], [1, 22, 3, 8]);
    assert_eq!(counts[70], 57);
    for s in ["Pan_Show_Cannon.ai", "Pan_Show_Norticu.ai"] {
        assert!(WHITELIST[70].scripts.contains(&s), "{s}");
    }
    // Off it, by decision 6: the hints, Eon's lines and the zone names
    // (text the TV draws nothing of), the cutscene manager's placement
    // (the cutscene system's macros run without it), the ambience.
    for s in [
        "Hints_Passive.ai",
        "Eon_Line_Activator.ai",
        "Name_Changer.ai",
        "Spyro_CutsceneManager.ai",
        "Goat_Running.ai",
        "Bird_Ambient_new.ai",
    ] {
        assert!(!WHITELIST[70].scripts.contains(&s), "{s}");
    }
    // Each walker's: his definition, his tuning actor, his minion's script
    // and what his file's other running placements run (3 to 5), 244 in all.
    assert_eq!(counts[4..70], WALKER_COUNTS);
    assert_eq!(counts[4..70].iter().sum::<usize>(), 244);
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
fn the_whitelisted_walker_scripts_are_the_walkers_archives_alone() {
    let listed = whitelisted_walker_scripts();
    assert_eq!(listed.len(), 244);
    // His minion's, his definition's and his portal entry's are in; a
    // fighter's, the globals' and the level's are not.
    for s in [
        "000_AirDragon.ai",
        "000_AirDragon_Def.ai",
        "PortalEntry_Air.ai",
    ] {
        assert!(listed.contains(&s), "{s}");
    }
    for s in [
        "009_LavaAttack.ai",
        "Init_Setup.ai",
        "GameSystems.ai",
        "Scale.ai",
    ] {
        assert!(!listed.contains(&s), "{s}");
    }
    // The part's archives are the base's and then these, one each.
    for a in WALKER_ARCHIVES {
        assert!(!archives().iter().any(|b| b == a), "{a}");
    }
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
    let r = spawn_models(&igz, archive, identity(), hero, None).unwrap();
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
    let all = spawn_models(&igz, archive, identity(), None, None).unwrap();
    assert_eq!(all.ir.props.len(), 8);
    let eruptor = &all.ir.props[0];
    assert_eq!(eruptor.path, "Models/Characters/Eruptor/Eruptor.mdl");
    assert!(eruptor.skeleton.is_some() && eruptor.idle.is_some());
    // global.bld's sample places no model; an archive with no portal model
    // has no body.
    let global = disc(&sample()).global;
    let g = Igz::parse(&global).unwrap();
    assert!(body(&g, GLOBAL).is_none());
    let r = spawn_models(&g, GLOBAL, identity(), None, None).unwrap();
    assert!(r.ir.props.is_empty() && r.textures.is_empty());
}

/// AH.39: models whose techniques name an effect of their own file (as
/// `global.bld`'s do) read with that file's own effects exported as
/// tfbmulti2, each as it reads importing it; without them each is refused
/// on `_effect`, the others' reading unchanged. `own_effects` reads
/// `global.bld`'s own through its exports, and none for any other archive.
#[test]
fn spawn_models_naming_their_files_own_effects_read_through_its_exports() {
    let imported_bytes: &'static [u8] = Vec::leak(model::character(&model::attached()));
    let imported = Igz::parse(imported_bytes).unwrap();
    let archive = "character/009_eruptor.bld";
    let want = spawn_models(
        &imported,
        archive,
        identity(),
        body(&imported, archive),
        None,
    )
    .unwrap();
    let own_bytes: &'static [u8] = Vec::leak(model::character(&model::CharacterSpec {
        own_effects: true,
        ..model::attached()
    }));
    let igz = Igz::parse(own_bytes).unwrap();
    let hero = body(&igz, archive);
    let effects: Vec<usize> = (0..igz.objects().len())
        .filter(|&o| igz.type_name(&igz.objects()[o]) == guac_formats::material::EFFECT_TYPE)
        .collect();
    assert_eq!(effects.len(), 7, "one a hung prop");
    // Without the file's own effects: every hung prop refused by name.
    let refused = spawn_models(&igz, archive, identity(), hero, None).unwrap();
    assert!(refused.ir.props.is_empty());
    assert_eq!(refused.ir.refused.len(), 7);
    assert!(
        refused
            .ir
            .refused
            .iter()
            .all(|r| r.reason.contains("_effect")),
        "{:?}",
        refused.ir.refused
    );
    // With them exported as tfbmulti2: as the imports read.
    let own: OwnEffects = effects
        .iter()
        .map(|&o| (o, guac_formats::material::TFBMULTI2))
        .collect();
    let read = spawn_models(&igz, archive, identity(), hero, Some(&own)).unwrap();
    assert!(read.ir.refused.is_empty(), "{:?}", read.ir.refused);
    // Each segment's source object sits one on for every own effect before
    // it; all else is the imports'.
    let unsourced = |props: &[Prop]| -> Vec<Prop> {
        let mut props = props.to_vec();
        for s in props.iter_mut().flat_map(|p| p.segments.iter_mut()) {
            s.source = 0;
        }
        props
    };
    assert!(unsourced(&read.ir.props) == unsourced(&want.ir.props));
    assert_eq!(read.ir.props.len(), 7);
    assert_eq!(read.textures, want.textures);
    // The archives' own effects: global.bld's through its exports alone.
    let global = disc(&sample());
    let g = Igz::parse(&global.global).unwrap();
    let boot = global.bootstrap.clone();
    let exported = guac_formats::exports::Exports::new(&Igz::parse(&boot).unwrap(), &g).unwrap();
    assert_eq!(
        own_effects(GLOBAL, &g, &boot).unwrap(),
        Some(OwnEffects::of_global(&exported))
    );
    assert_eq!(own_effects(archive, &igz, &boot).unwrap(), None);
    assert!(own_effects(GLOBAL, &g, b"not an igz").is_err());
}

#[test]
fn complete_wants_both_parts_and_the_walkers_list() {
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
    // A scripts part made before the walkers has no list (AG.27): the
    // start isn't current, so the part resumes alone.
    assert!(!complete(&m), "the walkers' list missing");
    m.parts
        .insert(SCRIPTS.into(), vec![WALKERS_FILE.to_owned()]);
    assert!(complete(&m));
}

/// AG.27: the walkers are the roster's modelled characters but a fighter
/// (any case), in the roster's order, their entries after the base's.
#[test]
fn the_walkers_are_every_modelled_character_but_a_fighter() {
    let loads = ["016_Spyro", "009_eruptor", "070_TreeRex", "009_Eruptor"];
    let walkers = walker_loads(&loads);
    assert_eq!(walkers, ["016_Spyro", "070_TreeRex"]);
    assert_eq!(
        archives_with(&walkers),
        [
            "permanent/bootstrap.bld",
            "permanent/global.bld",
            "character/init_setup.bld",
            "character/009_eruptor.bld",
            "character/016_spyro.bld",
            "character/070_treerex.bld",
        ]
    );
    // The fighters alone key the spawns part.
    assert_eq!(spawn_archives().len(), 2);
    assert_eq!(archives_with(&[]), archives());
}

/// AG.27: the list a start reads back is the one the stage wrote; an extract
/// made before it has none (no file, not an error), one that won't parse is
/// the error.
#[test]
fn the_walkers_list_reads_back_and_an_old_extract_has_none() {
    let tmp = tempfile::tempdir().unwrap();
    assert_eq!(read_walkers(tmp.path()).unwrap(), WalkerList::default());
    let list = WalkerList {
        listed: vec!["016_Spyro".into()],
        left_out: vec![("070_TreeRex".into(), "no level.bld".into())],
    };
    let path = tmp.path().join(WALKERS_FILE);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(&path, serde_json::to_vec(&list).unwrap()).unwrap();
    assert_eq!(read_walkers(tmp.path()).unwrap(), list);
    std::fs::write(&path, b"not json").unwrap();
    let e = read_walkers(tmp.path()).unwrap_err();
    assert!(e.starts_with(WALKERS_FILE), "{e}");
}
