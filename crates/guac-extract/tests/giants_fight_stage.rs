//! The extract for the fight on the real Giants dump (openspec AH.22,
//! design decision 7): a whole first run into a throwaway identity dir,
//! then its parts' resume. Checked: Eruptor's palette holds his throw
//! (slots 20 and 21, partial) and his eruption (32), chapter 1's Chompies
//! their notice, bite and fall (6, 11 and 20-25) with their lengths, every
//! whitelisted script is in the script part and reads through AH.13's
//! graph, and the spawn models hold the lava ball. Printed for AH.22's
//! tick: what each new part adds to the first run's time ahead of the
//! intros and to the extract's size, RON and postcard, with the load
//! average. Ignored by default: it reads `GUAC_GIANTS_DUMP` and writes
//! under `GUAC_EXTRACT_DIR`, from the environment or the workspace's
//! `.env.dev`; the whole run needs ffmpeg (`GUAC_FFMPEG`, else where
//! guac-extract looks):
//!
//! `cargo test -p guac-extract --release --test giants_fight_stage -- --ignored --nocapture`

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::Instant;

use guac_extract::intros::IrFormat;
use guac_extract::known::KNOWN_DUMPS;
use guac_extract::levels::{self, LEVELS};
use guac_extract::scripts::{self, WHITELIST};
use guac_extract::stages::{self, Event};
use guac_extract::{DirState, ExtractRoot, open};
use guac_formats::igz::Igz;
use guac_formats::script::Graph;
use guac_ir::palette::{LevelPalettes, Palette, SlotEntry};
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

fn read<T: serde::de::DeserializeOwned>(dir: &Path, rel: &str) -> T {
    let text = std::fs::read_to_string(dir.join(rel)).unwrap();
    guac_ir::from_ron(&text).unwrap()
}

/// Files under `prefix` in the manifest, and their bytes.
fn recorded(prefix: &str, m: &guac_extract::Manifest) -> (usize, u64) {
    m.outputs
        .iter()
        .filter(|o| o.path.starts_with(prefix))
        .fold((0, 0), |(n, b), o| (n + 1, b + o.size))
}

/// A slot's clip: its file, frames and length; the entry's blend.
fn slot(p: &Palette, slot: u16) -> (String, u32, f32, bool, bool) {
    let e = p
        .get(slot)
        .unwrap_or_else(|| panic!("{}: no slot {slot}", p.owner));
    let SlotEntry::Clip {
        additive,
        partial,
        clip,
        ..
    } = &e.entry
    else {
        panic!("{}: slot {slot} holds no clip", p.owner);
    };
    (
        clip.name.clone(),
        clip.frames,
        clip.duration,
        *additive,
        *partial,
    )
}

#[test]
#[ignore = "reads the Giants dump at GUAC_GIANTS_DUMP, writes under GUAC_EXTRACT_DIR"]
fn the_fights_parts_extract_whole_and_resume() {
    let dump = PathBuf::from(env_or_dotenv("GUAC_GIANTS_DUMP").expect("set GUAC_GIANTS_DUMP"));
    let base = PathBuf::from(env_or_dotenv("GUAC_EXTRACT_DIR").expect("set GUAC_EXTRACT_DIR"));
    std::fs::create_dir_all(&base).unwrap();
    let tmp = tempfile::Builder::new()
        .prefix("ah22-fight-")
        .tempdir_in(&base)
        .unwrap();
    let root = ExtractRoot::checked(tmp.path().join("extract")).unwrap();
    let started = Instant::now();
    println!("load average at the start: {}", load_average());
    let mut opened = open(&dump, &root, KNOWN_DUMPS, &|_| {}).expect("the known dump");
    assert_eq!(opened.state, DirState::Fresh);
    let dir = opened.dir.path().to_path_buf();

    // A whole first run, ffmpeg included, each stage's start marked.
    let ffmpeg = env_or_dotenv("GUAC_FFMPEG").map(PathBuf::from);
    let marks = Mutex::new(Vec::new());
    let run_started = Instant::now();
    let done = stages::run(&mut opened.dir, ffmpeg.as_deref(), IrFormat::Ron, &|e| {
        let what = match e {
            Event::RosterReady(_) => "roster ready",
            Event::Models { done: 0, .. } => "models start",
            Event::ModelsReady { .. } => "models ready",
            Event::Level { done: 0, .. } => "level start",
            Event::Scripts { done: 0, .. } => "scripts start",
            Event::Indexing => "indexing",
            _ => return,
        };
        marks
            .lock()
            .unwrap()
            .push((what, run_started.elapsed(), load_average()));
    })
    .unwrap();
    let total = run_started.elapsed();
    for (what, at, load) in marks.into_inner().unwrap() {
        println!(
            "  first run: {what} at {:.2} s; load average {load}",
            at.as_secs_f64()
        );
    }
    let (md, lv, sc) = (&done.models, &done.levels, &done.scripts);
    println!(
        "  first run: roster {:.2} s, models {:.2} s, level {:.2} s, scripts {:.2} s, intros {:?}, {:.1} s in all; load average {}",
        done.roster.took.as_secs_f64(),
        md.took.as_secs_f64(),
        lv.took.as_secs_f64(),
        sc.took.as_secs_f64(),
        done.intros
            .as_ref()
            .map(|i| i.took.as_secs_f64())
            .map_err(|e| e.code()),
        total.as_secs_f64(),
        load_average()
    );
    let p = &md.palettes;
    println!(
        "  character palettes: {} palettes, {} clips, {} networks, {} slots refused; {:?} in {:.2} s of the model stage's {:.2} s",
        p.palettes,
        p.clips,
        p.networks,
        p.refused,
        p.files,
        p.took.as_secs_f64(),
        md.took.as_secs_f64()
    );
    let c = &lv.palettes;
    println!(
        "  creature palettes: {} palettes, {} clips, {} networks, {} refused; {:?} in {:.2} s; level scripts {}, the entry checked and written in {:.2} s",
        c.palettes,
        c.clips,
        c.networks,
        c.refused,
        c.files,
        c.took.as_secs_f64(),
        lv.scripts,
        lv.script_took.as_secs_f64()
    );
    let added = p.took + c.took + lv.script_took + sc.took;
    println!(
        "  AH.22's parts in the first run: {:.2} s (character palettes {:.2}, the level's entry {:.2}, creature palettes {:.2}, the script stage {:.2})",
        added.as_secs_f64(),
        p.took.as_secs_f64(),
        lv.script_took.as_secs_f64(),
        c.took.as_secs_f64(),
        sc.took.as_secs_f64()
    );
    println!(
        "  script stage: {} parts read, {} entries, {} scripts, {:?}; {} spawn models ({} refused), {:?}; {:.2} s",
        sc.parts_read,
        sc.entries,
        sc.scripts,
        sc.script_files,
        sc.props,
        sc.refused_props.len(),
        sc.spawn_files,
        sc.took.as_secs_f64()
    );
    for (p, r) in &sc.refused_props {
        println!("  REFUSED SPAWN MODEL {p}: {r}");
    }
    assert!(md.failed.is_empty() && lv.failed.is_empty() && sc.failed.is_empty());
    // AH.87's emitters, after the scripts (`giants_particles` checks them).
    assert!(done.particles.read && done.particles.failed.is_none());
    assert!(done.intros.is_ok(), "ffmpeg is needed for the whole run");
    assert_eq!((p.palettes, lv.parts_read, sc.parts_read), (67, 8, 2));
    // The fighter's archive and the three base ones, then AG.27's 66 walkers.
    assert_eq!((sc.entries, sc.walkers), (4 + 66, 66));
    let m = opened.dir.manifest().clone();
    for prefix in [
        "palettes/",
        "scripts/",
        "spawns/",
        "levels/",
        "models/",
        "heroes/",
    ] {
        let (n, bytes) = recorded(prefix, &m);
        println!("  {prefix} {n} files, {bytes} bytes");
    }
    let script = levels::script_path(LEVELS[0]);
    let creature_rel = IrFormat::Ron.level_path(LEVELS[0], levels::PALETTES);
    for rel in [&script, &creature_rel] {
        let size = m.outputs.iter().find(|o| o.path == *rel).unwrap().size;
        println!("  {rel}: {size} bytes");
    }
    // The install format's size, the RON put in postcard (no file written).
    let mut postcard = 0u64;
    for rel in &m.parts[guac_extract::models::PALETTES] {
        let p: Palette = read(&dir, rel);
        postcard += guac_ir::to_postcard(&p).unwrap().len() as u64;
    }
    let creatures: LevelPalettes = read(&dir, &creature_rel);
    let creatures_postcard = guac_ir::to_postcard(&creatures).unwrap().len();
    let mut spawns_postcard = 0u64;
    for archive in scripts::spawn_archives() {
        let s: SpawnModels = read(&dir, &IrFormat::Ron.spawn_path(&archive));
        spawns_postcard += guac_ir::to_postcard(&s).unwrap().len() as u64;
    }
    println!(
        "  postcard: character palettes {postcard} bytes, creature palettes {creatures_postcard}, spawn models {spawns_postcard}"
    );

    // Eruptor's throw (partial, both hands) and eruption.
    let eruptor: Palette = read(&dir, &IrFormat::Ron.palette_path("009_Eruptor"));
    assert_eq!(eruptor.owner, "009_Eruptor");
    for s in [0, 20, 21, 32] {
        let (name, frames, len, additive, partial) = slot(&eruptor, s);
        println!(
            "  Eruptor slot {s}: {name}, {frames} frames, {len} s, additive {additive}, partial {partial}"
        );
    }
    let (name, frames, _, additive, partial) = slot(&eruptor, 32);
    assert_eq!(
        (name.as_str(), frames, additive, partial),
        ("Eruptor_Eruption.ma", 61, false, false)
    );
    for s in [20, 21] {
        let (_, _, len, additive, partial) = slot(&eruptor, s);
        assert!(partial && !additive && len > 0.0, "slot {s}");
    }
    println!(
        "  Eruptor: {} slots, {} filled, {} refused: {:?}",
        eruptor.slots,
        eruptor.entries.len(),
        eruptor.refused.len(),
        eruptor.refused
    );

    // Chapter 1's Chompies: the template's model and the placements'.
    let owners: Vec<&str> = creatures
        .palettes
        .iter()
        .map(|p| p.owner.as_str())
        .collect();
    println!("  creatures: {owners:?}");
    let expect: BTreeMap<u16, (&str, u32)> = [
        (6, ("Teeth_KO.ma", 61)),
        (11, ("Teeth_Notice.ma", 24)),
        (20, ("Teeth_JumpAttackWindup.ma", 11)),
        (21, ("Teeth_JumpAttackWindupHold.ma", 25)),
        (22, ("Teeth_JumpAttack.ma", 6)),
        (23, ("Teeth_JumpAttackFlyingLoop.ma", 17)),
        (24, ("Teeth_JumpAttackSuccessLoop.ma", 25)),
        (25, ("Teeth_AttackBasicMiss.ma", 18)),
    ]
    .into();
    for model in ["Chompy.mdl", "ChompyUndead.mdl"] {
        let p = creatures
            .palettes
            .iter()
            .find(|p| p.owner.ends_with(&format!("/{model}")))
            .unwrap_or_else(|| panic!("no {model}"));
        for (&s, &(file, frames)) in &expect {
            let (name, n, len, additive, partial) = slot(p, s);
            println!("  {model} slot {s}: {name}, {n} frames, {len} s");
            assert_eq!(
                (name.as_str(), n, additive, partial),
                (file, frames, false, false)
            );
        }
    }
    assert!(
        owners.iter().any(|o| o.ends_with("/MabuAncient.mdl")),
        "Norticus"
    );
    // AG.60's start scripts add the chest's, by `Chest.ai`.
    assert!(
        owners.iter().any(|o| o.ends_with("/Treasure_Chest.mdl")),
        "the chest"
    );
    // AH.42's two from AH.8's route (the cannon's pan, the pit's intro) name
    // no model: the creatures' palettes stay these eight.
    assert_eq!(owners.len(), 8, "{owners:?}");
    // AG.177's jumping goats add `Sheep_1` and `Sheep_3`'s, by
    // `Goat_Jumping_Pit.ai`: without it the TV's goats have no clip lengths.
    assert!(
        owners.iter().any(|o| o.ends_with("/MountainGoat.mdl")),
        "the jumping goats"
    );

    // Every whitelisted script is in the script part and reads: the base's,
    // Eruptor's, the level's and (AG.162) each of the 66 walkers' own.
    let parse_started = Instant::now();
    let mut held = 0;
    assert_eq!(WHITELIST.len(), 5 + 66);
    for w in WHITELIST {
        let rel = match w.archive.strip_prefix("level/") {
            Some(level) => levels::script_path(level.strip_suffix(".bld").unwrap()),
            None => scripts::script_path(w.archive),
        };
        let bytes = std::fs::read(dir.join(&rel)).unwrap();
        let igz = Igz::parse(&bytes).unwrap();
        let graph = Graph::read(&igz).unwrap();
        let names: Vec<&str> = graph.scripts.iter().filter_map(|s| s.name).collect();
        for s in w.scripts {
            assert!(
                names.iter().any(|n| scripts::listed(&[*s], n)),
                "{s} not in {rel}"
            );
        }
        held += w.scripts.len();
        println!(
            "  {rel}: {} bytes, {} scripts, the {} whitelisted in it",
            bytes.len(),
            names.len(),
            w.scripts.len()
        );
    }
    println!(
        "  {held} whitelisted scripts, all in the script part ({} walkers' of them)",
        scripts::whitelisted_walker_scripts().len()
    );
    // The level's 57 with AG.177's jumping goats and AH.42's two from AH.8's
    // route.
    assert_eq!(held, 1 + 22 + 3 + 8 + 244 + 57);
    println!(
        "  the script part read back and parsed in {:.2} s",
        parse_started.elapsed().as_secs_f64()
    );

    // The lava ball and its splash, the level-up's star and the death stars.
    let fighter: SpawnModels = read(&dir, &IrFormat::Ron.spawn_path("character/009_eruptor.bld"));
    let global: SpawnModels = read(&dir, &IrFormat::Ron.spawn_path("permanent/global.bld"));
    let names = |s: &SpawnModels| -> Vec<String> {
        s.props
            .iter()
            .map(|p| p.path.rsplit('/').next().unwrap().to_owned())
            .collect()
    };
    println!("  Eruptor's spawn models: {:?}", names(&fighter));
    println!("  global.bld's spawn models: {:?}", names(&global));
    for model in ["Fireblob.mdl", "FireBlobSplash.mdl", "LavaBlob.mdl"] {
        assert!(names(&fighter).iter().any(|n| n == model), "{model}");
    }
    // global.bld's own models name its effects as objects of its own, read
    // by the ids its exports give them (AH.39): the level-up's tube, star
    // and warp ring, the death stars and the portal entry's column among
    // them, any refused for another, named reason (before AH.39 all 23
    // refused on `_effect`).
    for model in [
        "lexupblossomPT1b.mdl",
        "lexupblossomPT2starA.mdl",
        "Troll_Warp_Ring.mdl",
        "death_starz.mdl",
        "portal_entry_column.mdl",
    ] {
        assert!(names(&global).iter().any(|n| n == model), "{model}");
    }
    println!(
        "  global.bld: {} read, {} refused",
        global.props.len(),
        global.refused.len()
    );
    for r in &global.refused {
        println!("    refused {}: {}", r.path, r.reason);
        assert!(!r.reason.contains("_effect"), "{}: {}", r.path, r.reason);
    }
    assert!(
        !names(&fighter).iter().any(|n| n == "Eruptor.mdl"),
        "his body is the model stage's"
    );
    drop(opened);

    // Current once; then each new part taken out resumes alone.
    let again = open(&dump, &root, KNOWN_DUMPS, &|_| {}).unwrap();
    assert_eq!(again.state, DirState::Current);
    drop(again);
    let path = dir.join("manifest.json");
    let mut json: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
    for part in [
        scripts::SCRIPTS,
        scripts::SPAWNS,
        guac_extract::models::PALETTES,
        guac_extract::particles::PART,
    ] {
        json["parts"].as_object_mut().unwrap().remove(part);
    }
    for part in [levels::SCRIPT, levels::PALETTES] {
        json["levels"][LEVELS[0]]["parts"]
            .as_object_mut()
            .unwrap()
            .remove(part);
    }
    std::fs::write(&path, serde_json::to_string(&json).unwrap()).unwrap();
    let mut again = open(&dump, &root, KNOWN_DUMPS, &|_| {}).unwrap();
    assert_eq!(again.state, DirState::Resume);
    let resume_started = Instant::now();
    let done = stages::run(&mut again.dir, ffmpeg.as_deref(), IrFormat::Ron, &|_| {}).unwrap();
    let took = resume_started.elapsed();
    let written = done.roster.files.written
        + done.progression.files.written
        + done.models.files.written
        + done.models.palettes.files.written
        + done.levels.files.written
        + done.scripts.files.written;
    println!(
        "  resume of the five parts: levels {} read, {} kept; scripts {} read; palettes kept {}; {written} files written; {:.2} s (models {:.2} s, level {:.2} s, scripts {:.2} s); load average {}",
        done.levels.parts_read,
        done.levels.parts_kept,
        done.scripts.parts_read,
        done.models.palettes.files.kept,
        took.as_secs_f64(),
        done.models.took.as_secs_f64(),
        done.levels.took.as_secs_f64(),
        done.scripts.took.as_secs_f64(),
        load_average()
    );
    assert_eq!(written, 0, "every file was there as recorded");
    assert_eq!((done.levels.parts_read, done.scripts.parts_read), (2, 2));
    assert!(done.particles.read && done.particles.files.written == 0);
    drop(again);
    assert_eq!(
        open(&dump, &root, KNOWN_DUMPS, &|_| {}).unwrap().state,
        DirState::Current
    );
    // A resume of a whole extract reads nothing new.
    let mut again = open(&dump, &root, KNOWN_DUMPS, &|_| {}).unwrap();
    let s = stages::script_stage(&mut again.dir, IrFormat::Ron, &|_| {}).unwrap();
    assert_eq!((s.parts_read, s.parts_kept), (0, 2));
    println!("  script stage kept whole in {:.3} s", s.took.as_secs_f64());
    // The install format: the three stages again in postcard over the same
    // extract (the model stage writes every model's postcard too), each new
    // part's time and size.
    let roster = guac_extract::roster::load(&again.dir, IrFormat::Ron).unwrap();
    let mp = stages::model_stage(&mut again.dir, &roster, IrFormat::Postcard, &|_| {}).unwrap();
    let lp = stages::level_stage(&mut again.dir, IrFormat::Postcard, &|_| {}).unwrap();
    let sp = stages::script_stage(&mut again.dir, IrFormat::Postcard, &|_| {}).unwrap();
    println!(
        "  postcard: model stage {:.2} s, its palettes {:?} in {:.2} s; level stage {:.2} s, creature palettes {:?} in {:.2} s, the entry kept ({} parts read, {} kept); script stage {:.2} s, {} parts read, {} kept, spawns {:?}; load average {}",
        mp.took.as_secs_f64(),
        mp.palettes.files,
        mp.palettes.took.as_secs_f64(),
        lp.took.as_secs_f64(),
        lp.palettes.files,
        lp.palettes.took.as_secs_f64(),
        lp.parts_read,
        lp.parts_kept,
        sp.took.as_secs_f64(),
        sp.parts_read,
        sp.parts_kept,
        sp.spawn_files,
        load_average()
    );
    assert_eq!(mp.palettes.files.written, 67);
    // Every palette's RON, read back and put in postcard, is its postcard
    // file byte for byte; the creatures' and the spawn models' too.
    let mut agreed = 0;
    for load in guac_extract::models::character_loads(&roster) {
        let ron: Palette = read(&dir, &IrFormat::Ron.palette_path(load));
        let post = std::fs::read(dir.join(IrFormat::Postcard.palette_path(load))).unwrap();
        assert!(guac_ir::to_postcard(&ron).unwrap() == post, "{load}");
        agreed += 1;
    }
    assert_eq!(agreed, 67);
    let post = std::fs::read(dir.join(IrFormat::Postcard.level_path(LEVELS[0], levels::PALETTES)))
        .unwrap();
    assert!(guac_ir::to_postcard(&creatures).unwrap() == post);
    for archive in scripts::spawn_archives() {
        let ron: SpawnModels = read(&dir, &IrFormat::Ron.spawn_path(&archive));
        let post = std::fs::read(dir.join(IrFormat::Postcard.spawn_path(&archive))).unwrap();
        assert!(guac_ir::to_postcard(&ron).unwrap() == post, "{archive}");
    }
    println!(
        "  RON and postcard agree byte for byte: {agreed} palettes, the creatures', the spawn models'"
    );
    assert_eq!((lp.parts_read, lp.parts_kept), (7, 1));
    assert_eq!((sp.parts_read, sp.parts_kept), (1, 1));

    println!(
        "took {:.1} s in all; load average {}",
        started.elapsed().as_secs_f64(),
        load_average()
    );
}
