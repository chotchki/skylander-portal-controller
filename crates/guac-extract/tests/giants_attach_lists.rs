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
