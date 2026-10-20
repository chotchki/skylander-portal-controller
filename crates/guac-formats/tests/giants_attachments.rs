//! The fresh-figure attachment rule over every character archive on the
//! Giants disc (openspec AE.23): `#[ignore]`d, reads `GUAC_GIANTS_DUMP`.
//! Run: `cargo test -p guac-formats --release --test giants_attachments --
//! --ignored --nocapture`. The per-figure check against the oracle's sweep
//! (AE.22) is guac-extract's, on the extracted IR.

mod common;

use std::collections::BTreeMap;
use std::path::Path;
use std::time::Instant;

use common::usrdir;
use guac_formats::attachments::{self, Owner, Template};
use guac_formats::exports::Exports;
use guac_formats::iga::Archive;
use guac_formats::igz::Igz;
use guac_formats::igz::layouts::BoneAttachInfo;
use guac_formats::language::{ENGLISH, Texts};
use guac_formats::model;
use guac_formats::roster::{self, Category};
use guac_formats::toys::ToyTable;

fn level_bld(path: &Path) -> Vec<u8> {
    let bytes = std::fs::read(path).unwrap();
    let arc = Archive::parse(&bytes).unwrap();
    arc.read(arc.find("level.bld").unwrap())
        .unwrap()
        .into_owned()
}

fn model_file(igz: &Igz<'_>, model: usize) -> String {
    let o = igz.objects()[model];
    o.field(8)
        .and_then(|at| igz.string(at, "_name").ok().flatten())
        .map(|n| n.rsplit(['/', '\\']).next().unwrap_or(n).to_owned())
        .unwrap_or_default()
}

/// The hero's name, each hung model as (model file, bone), and how many
/// unsure clones hang on the hero.
type FreshSet = (String, Vec<(String, String)>, usize);

/// One archive's fresh set: (model file, bone) per hung model, parents
/// first, and the hero's name.
fn fresh_set(igz: &Igz<'_>, global: &Igz<'_>, exports: &Exports, load: &str) -> FreshSet {
    let portal = model::portal_model(igz).unwrap();
    let hero = attachments::hero_actor(igz, &portal, load).unwrap();
    let fresh = attachments::fresh(igz, &hero).unwrap();
    let model_of = |t: &Template| -> Option<(bool, usize)> {
        match t {
            Template::Local(p) => attachments::placement_model(igz, *p)
                .unwrap()
                .map(|m| (true, m)),
            Template::Global(id) => {
                let o = exports.resolve_import(id)?.ok()?.object()?;
                attachments::placement_model(global, o)
                    .unwrap()
                    .map(|m| (false, m))
            }
        }
    };
    let hung = attachments::hung(&fresh, |t| model_of(t).is_some())
        .unwrap_or_else(|e| panic!("{load}: {e}"));
    let hero_model = hero.info.model.unwrap();
    let mut out: Vec<(String, String)> = Vec::new();
    for h in &hung {
        let (local, m) = model_of(&h.template).unwrap();
        let file = if local {
            model_file(igz, m)
        } else {
            model_file(global, m)
        };
        // The point it hangs on: an entry of the parent's attach list.
        let (owner_igz, owner_model) = match h.parent {
            None => (igz, hero_model),
            Some(k) => {
                let (l, pm) = model_of(&hung[k].template).unwrap();
                (if l { igz } else { global }, pm)
            }
        };
        let points = attachments::attach_points(owner_igz, owner_model).unwrap();
        let bone = match h.slot {
            Some(s) => points
                .get(s)
                .and_then(|p: &BoneAttachInfo<'_>| p.bone_name)
                .unwrap_or("?")
                .to_owned(),
            None => "(owner)".to_owned(),
        };
        out.push((file, bone));
    }
    // Unsure clones on the hero: each one that draws would have refused the
    // set above, so these draw nothing (a LightCore figure's light).
    let unsure = fresh
        .clones
        .iter()
        .filter(|c| c.unsure && c.node.is_some_and(|n| n.owner == Owner::Hero))
        .count();
    (hero.name().unwrap_or_default().to_owned(), out, unsure)
}

#[test]
#[ignore = "reads the Giants disc dump (GUAC_GIANTS_DUMP)"]
fn every_characters_fresh_attachments() {
    let usrdir = usrdir();
    let boot = level_bld(&usrdir.join("permanent/bootstrap.bld"));
    let glob = level_bld(&usrdir.join("permanent/global.bld"));
    let boot = Igz::parse(&boot).unwrap();
    let global = Igz::parse(&glob).unwrap();
    let exports = Exports::new(&boot, &global).unwrap();
    // The roster's character load files (AE.2), each once.
    let init = level_bld(&usrdir.join("character/init_setup.bld"));
    let init = Igz::parse(&init).unwrap();
    let collection = level_bld(&usrdir.join("misc/ui_collection_champions.bld"));
    let collection = Igz::parse(&collection).unwrap();
    let packs = {
        let bytes = std::fs::read(usrdir.join("permanent/global.bld")).unwrap();
        let arc = Archive::parse(&bytes).unwrap();
        guac_formats::language::packs(&arc).unwrap()
    };
    let texts = Texts::new(&global, packs).unwrap();
    let table = ToyTable::read(&init, &global, &exports).unwrap();
    let mut loads: Vec<String> = Vec::new();
    for f in roster::figures(&table, &global, &collection).unwrap() {
        if !matches!(f.category, Category::Figure | Category::Giant) {
            continue;
        }
        let load = texts.text(f.level_load, ENGLISH).unwrap().to_owned();
        if !loads.contains(&load) {
            loads.push(load);
        }
    }
    let start = Instant::now();
    let mut sets: BTreeMap<String, FreshSet> = BTreeMap::new();
    for load in &loads {
        let path = usrdir
            .join("character")
            .join(format!("{}.bld", load.to_ascii_lowercase()));
        let bytes = level_bld(&path);
        let igz = Igz::parse(&bytes).unwrap();
        sets.insert(load.clone(), fresh_set(&igz, &global, &exports, load));
    }
    let took = start.elapsed();
    let mut models = 0;
    let mut per: BTreeMap<usize, usize> = BTreeMap::new();
    let mut helpers = 0;
    let mut unsure = 0;
    for (load, (hero, set, u)) in &sets {
        unsure += u;
        let shown: Vec<String> = set.iter().map(|(m, b)| format!("{m}@{b}")).collect();
        println!("{load}\t{hero}\t{}", shown.join(", "));
        models += set.len();
        helpers += set.iter().filter(|(m, _)| m.contains("inviso")).count();
        *per.entry(set.len()).or_default() += 1;
    }
    println!(
        "{} character archives, {models} attached models ({helpers} invisible helpers), per archive {per:?}, {unsure} unsure clones on a hero drawing nothing, {:.2} s",
        sets.len(),
        took.as_secs_f64()
    );
    assert_eq!(sets.len(), 67);
    let of = |load: &str| -> Vec<String> {
        sets[load]
            .1
            .iter()
            .map(|(m, b)| format!("{m}@{b}"))
            .collect()
    };
    // AE.22's first check: Stealth Elf holds her knives.
    assert_eq!(
        of("026_StealthElf"),
        [
            "StealthElfKnife.mdl@J_attach_R",
            "StealthElfKnife.mdl@J_attach_L"
        ]
    );
    // Eruptor carries nothing: his spikes are an upgrade's.
    assert!(of("009_Eruptor").is_empty());
    // Gill Grunt's spear hangs on his gun, the gun and the pack on him.
    assert_eq!(
        of("014_GillGrunt"),
        [
            "GillGrunt_HarpoonGun.mdl@J_gun",
            "GillGrunt_HarpoonSpear_Prop.mdl@*root*",
            "design_inviso.mdl@*root*",
            "GillGruntBackPack.mdl@J_jetpack"
        ]
    );
    assert_eq!(
        of("112_TitanLife"),
        ["TitanLifeRocket.mdl@Ja_attachRockets"]
    );
}
