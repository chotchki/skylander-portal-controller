//! openspec AS.562 on the dump: an attach list with an empty slot reads,
//! the points after it keeping their index. Before the fix the typed list
//! reader refused the whole list (96 lists over 477 archives, every one
//! this), so a model lost every node. Three archives that held one: level
//! 208 (the cyclops bomber), Sprocket's (her turret's models) and a card
//! deck's asset (a presented creature's weapon). A few seconds.
//!
//! `cargo test -p guac-extract --profile disc --test giants_attach_lists -- --ignored --nocapture`

use std::path::{Path, PathBuf};

use guac_formats::igz::Igz;
use guac_ir::{DumpIdentity, Sha256};

fn usrdir() -> PathBuf {
    let dotenv = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../.env.dev");
    let root = std::env::var("GUAC_GIANTS_DUMP")
        .ok()
        .or_else(|| {
            std::fs::read_to_string(dotenv).ok()?.lines().find_map(|l| {
                let (k, v) = l.trim().split_once('=')?;
                (k.trim() == "GUAC_GIANTS_DUMP").then(|| v.trim().trim_matches('"').to_owned())
            })
        })
        .expect("GUAC_GIANTS_DUMP in the environment or .env.dev");
    Path::new(&root).join("PS3_GAME/USRDIR")
}

fn identity() -> DumpIdentity {
    DumpIdentity {
        title_id: "BLUS30968".into(),
        app_ver: "01.00".into(),
        manifest_hash: Sha256([0; 32]),
    }
}

#[test]
#[ignore = "reads the Giants dump"]
fn attach_lists_with_an_empty_slot_read_with_the_others_at_their_index() {
    let usr = usrdir();
    // Each archive and a model in it whose list holds an empty slot.
    let cases = [
        ("level/level_208_ice.bld", "CyclopsBomber.mdl"),
        (
            "character/111_turretmaster.bld",
            "SteamPunkGirl_Turret_01_Base.mdl",
        ),
        ("misc/cardgame_card_034.bld", "SeaDogHeavyGun.mdl"),
    ];
    for (archive, model) in cases {
        let entry = guac_extract::scripts::entry(&usr, archive).expect(archive);
        let igz = Igz::parse(&entry).expect(archive);
        let read = guac_extract::levels::nodes(&igz, archive, identity()).expect(archive);
        assert!(
            read.ir.refused.is_empty(),
            "{archive}: {:?}",
            read.ir.refused
        );
        let list = read
            .ir
            .models
            .iter()
            .find(|m| m.load_file.ends_with(model))
            .unwrap_or_else(|| panic!("{archive}: {model}'s list"));
        let empty: Vec<usize> = (0..list.nodes.len())
            .filter(|&i| list.nodes[i].is_empty())
            .collect();
        println!(
            "{archive} {model}: {} slots, empty at {empty:?}",
            list.nodes.len()
        );
        assert!(!empty.is_empty(), "{archive}: {model} holds an empty slot");
        // A real point after an empty slot: the list kept its order rather
        // than closing up.
        let first = empty[0];
        assert!(
            list.nodes[first + 1..].iter().any(|n| !n.is_empty()),
            "{archive}: {model} has a point after its empty slot {first}"
        );
    }
}

/// openspec AS.661 on the dump: a character archive's static models' attach
/// lists, the part the TV hangs his spawns on. Before it no part carried
/// them, so Sprocket's turret pieces and Crusher's earth crack stood at their
/// pivots. Every character archive reads with none refused; Sprocket's
/// turret base and Crusher's crack keep their empty slots at their index.
#[test]
#[ignore = "reads the Giants dump"]
fn every_character_archives_static_lists_read_for_the_spawn_nodes_part() {
    let usr = usrdir();
    let mut names: Vec<String> = std::fs::read_dir(usr.join("character"))
        .expect("character/")
        .filter_map(|e| e.ok()?.file_name().into_string().ok())
        .filter(|n| n.ends_with(".bld"))
        .collect();
    names.sort();
    let (mut archives, mut with, mut lists) = (0, 0, 0);
    let mut picked = Vec::new();
    for n in &names {
        let archive = format!("character/{n}");
        let Ok(entry) = guac_extract::scripts::entry(&usr, &archive) else {
            continue;
        };
        let igz = Igz::parse(&entry).expect(&archive);
        let body = guac_extract::scripts::body(&igz, &archive);
        let r = guac_extract::scripts::spawn_nodes(&igz, &archive, identity(), body)
            .unwrap_or_else(|e| panic!("{archive}: {e}"));
        assert!(r.refused.is_empty(), "{archive}: {:?}", r.refused);
        assert!(
            r.models
                .iter()
                .all(|m| m.nodes.iter().all(|n| n.joint.is_none())),
            "{archive}: a static model's node on a joint"
        );
        archives += 1;
        with += usize::from(!r.models.is_empty());
        lists += r.models.len();
        if matches!(n.as_str(), "111_turretmaster.bld" | "102_titan_earth.bld") {
            picked.push(r);
        }
    }
    println!("{archives} character archives, {with} with static lists, {lists} lists");
    assert!(archives > 100, "{archives}");
    assert!(with > 50 && lists > 250, "{with} archives, {lists} lists");
    let find = |archive: &str, model: &str| {
        picked
            .iter()
            .find(|r| r.archive == archive)
            .and_then(|r| r.models.iter().find(|m| m.load_file.ends_with(model)))
            .unwrap_or_else(|| panic!("{archive}: {model}"))
            .clone()
    };
    let base = find(
        "character/111_turretmaster.bld",
        "SteamPunkGirl_Turret_01_Base.mdl",
    );
    assert_eq!(base.nodes.len(), 3);
    assert!(base.nodes[0].is_empty() && !base.nodes[2].is_empty());
    let top = find(
        "character/111_turretmaster.bld",
        "SteamPunkGirl_Turret_00_Unified_Top.mdl",
    );
    assert_eq!(top.nodes.len(), 6);
    assert!(top.nodes.iter().all(|n| !n.is_empty()));
    let crack = find("character/102_titan_earth.bld", "earthcrack_crack.mdl");
    assert_eq!(crack.nodes.len(), 2);
    assert!(crack.nodes[0].is_empty() && !crack.nodes[1].is_empty());
}
