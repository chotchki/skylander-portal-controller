//! What of a hero the TV draws through his attacks (openspec AS.444),
//! headless with guacamole's host on the TV's listing for level 201: his
//! body by [`guacamole::props::hero_drawn`] (hidden while his tint is
//! see-through), the parts the extract hangs on a fresh figure (AE.23's,
//! read here off his archive as guac-extract reads them) only while his
//! scripts don't carry him, and every clone his scripts hang on him drawn
//! while it hangs on something live and isn't see-through. Ignored by
//! default; they read `GUAC_GIANTS_DUMP` (process env, else `.env.dev`):
//!
//! `cargo test -p guacamole --profile disc --test giants_hero_parts -- --ignored --nocapture --test-threads=1`
//!
//! The roster check joins a few figures by default; `GUAC_HERO_PARTS_ALL=1`
//! joins every numbered character archive (68 with `113_sniper`, which the
//! roster never loads: 1,119 s at load 2.5-6 on 2026-11-16).

mod common;

use std::collections::BTreeMap;
use std::path::Path;

use common::*;
use guac_formats::attachments::{self, Template};
use guac_formats::exports::Exports;
use guac_formats::iga::Archive;
use guac_formats::igz::Igz;
use guac_formats::model;
use guac_motion::seam::{Quat, Vec3};
use guac_motion::stick::Stick;
use guac_script::{Handle, Mode, Value};
use guacamole::props::{actor_shows, hero_drawn, hero_drawn_scale};
use guacamole::script_engine::Pad;

const LEVEL_201: &str = "level_201";

const NINJINI: Character = Character {
    archive: "character/109_djini.bld",
    load_file: "109_Djini",
    toy: 109,
};
const WARNADO: Character = Character {
    archive: "character/002_skyturtle.bld",
    load_file: "002_SkyTurtle",
    toy: 2,
};
const CHOP_CHOP: Character = Character {
    archive: "character/030_pandoranelite.bld",
    load_file: "030_PandoranElite",
    toy: 30,
};

fn level_bld(path: &Path) -> Vec<u8> {
    let bytes = std::fs::read(path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    let arc = Archive::parse(&bytes).unwrap();
    arc.read(arc.find("level.bld").unwrap())
        .unwrap()
        .into_owned()
}

fn file_name(path: &str) -> String {
    path.rsplit(['/', '\\']).next().unwrap_or(path).to_owned()
}

/// The model files of the parts the extract hangs on `archive`'s fresh
/// figure (AE.23's rule over the dump, as guac-extract's model stage runs
/// it), each with its placement.
fn fresh_parts(archive: &str, load_file: &str) -> Vec<(String, String)> {
    let usr = dump().join("PS3_GAME/USRDIR");
    let boot = level_bld(&usr.join("permanent/bootstrap.bld"));
    let glob = level_bld(&usr.join("permanent/global.bld"));
    let (boot, global) = (Igz::parse(&boot).unwrap(), Igz::parse(&glob).unwrap());
    let exports = Exports::new(&boot, &global).unwrap();
    let bytes = level_bld(&usr.join(archive));
    let igz = Igz::parse(&bytes).unwrap();
    let portal = model::portal_model(&igz).unwrap();
    let hero = model::hero_actor(&igz, &portal, load_file).unwrap();
    let fresh = attachments::fresh(&igz, &hero).unwrap();
    let model_of = |t: &Template| -> Option<(bool, usize)> {
        match t {
            Template::Local(p) => attachments::placement_model(&igz, *p)
                .unwrap()
                .map(|m| (true, m)),
            Template::Global(id) => {
                let o = exports.resolve_import(id)?.ok()?.object()?;
                attachments::placement_model(&global, o)
                    .unwrap()
                    .map(|m| (false, m))
            }
        }
    };
    let name = |igz: &Igz<'_>, model: usize| -> String {
        igz.objects()[model]
            .field(8)
            .and_then(|at| igz.string(at, "_name").ok().flatten())
            .map(file_name)
            .unwrap_or_default()
    };
    let placement = |igz: &Igz<'_>, p: usize| -> String {
        guac_formats::level::Placements::read(igz)
            .ok()
            .and_then(|ps| {
                ps.actors
                    .iter()
                    .find(|a| a.index == p)
                    .and_then(|a| a.name())
                    .map(str::to_owned)
            })
            .unwrap_or_default()
    };
    attachments::hung(&fresh, |t| model_of(t).is_some())
        .unwrap_or_else(|e| panic!("{load_file}: {e}"))
        .iter()
        .map(|h| {
            let (local, m) = model_of(&h.template).unwrap();
            let file = if local {
                name(&igz, m)
            } else {
                name(&global, m)
            };
            let at = match h.template {
                Template::Local(p) => placement(&igz, p),
                Template::Global(_) => String::new(),
            };
            (at, file)
        })
        .collect()
}

/// What `h` hangs on: the actor of the node it's set at, else the actor
/// itself.
fn owner(run: &Run, h: Handle) -> Option<Handle> {
    let a = run.world.actor(h)?;
    run.world.node_owner(a.attach).or(match a.attach {
        Value::Entity(o) if run.world.actor(o).is_some() => Some(o),
        _ => None,
    })
}

/// Whether `h` hangs on the hero, or on what hangs on him, up the chain.
fn on_hero(run: &Run, h: Handle) -> bool {
    let mut at = h;
    for _ in 0..8 {
        match owner(run, at) {
            Some(o) if o == run.hero => return true,
            Some(o) if o != at => at = o,
            _ => return false,
        }
    }
    false
}

/// What of him draws as the step leaves it: his body, and each drawn
/// model file with how many times it draws (the extract's fresh parts
/// while [`hero_drawn`] lets them, his scripts' clones on him while they
/// hang on something live and aren't see-through).
#[derive(Debug, Default, Clone, PartialEq)]
struct Drawn {
    body: bool,
    models: BTreeMap<String, usize>,
    /// His drawn scale ([`hero_drawn_scale`]), percent per axis.
    scale: [f32; 3],
}

impl Drawn {
    fn of(run: &Run, fresh: &[(String, String)]) -> Drawn {
        let actor = run.world.actor(run.hero);
        let joined = run.minion.is_some();
        let rule = hero_drawn(false, joined, actor);
        let mut out = Drawn {
            body: rule.body,
            scale: (hero_drawn_scale(actor.map_or([100.0; 3], |a| a.scale), 1.0) * 100.0)
                .to_array(),
            ..Drawn::default()
        };
        if rule.fresh_parts {
            for (_, file) in fresh {
                *out.models.entry(file.clone()).or_default() += 1;
            }
        }
        for c in run.world.clones() {
            if run.world.removed(c) || !on_hero(run, c) || run.world.hangs_on_nothing(c) {
                continue;
            }
            let Some(a) = run.world.actor(c) else {
                continue;
            };
            let model = a
                .template
                .and_then(|t| run.world.template(t))
                .and_then(|p| p.model.as_deref())
                .map(file_name);
            if let Some(m) = model
                && actor_shows(a)
            {
                *out.models.entry(m).or_default() += 1;
            }
        }
        out
    }

    fn count(&self, file: &str) -> usize {
        self.models.get(file).copied().unwrap_or(0)
    }
}

/// One step in play with `actions` held and the stick at rest; a hero
/// under 60 health is topped up, as the Mac's runs do.
fn play(run: &mut Run, actions: [bool; 4]) {
    run.bench.pads[0] = Pad {
        stick: Stick::new(0.0, 0.0),
        actions,
        connected: true,
    };
    let stops = run.step_play().unwrap_or_else(|h| panic!("{h}"));
    assert!(stops.is_empty(), "step {}: {stops:?}", run.steps);
    if let (Some(hp), Some(m)) = (run.health(), run.minion)
        && hp < 60.0
    {
        guacamole::scripts::set_health(&mut run.vm, m, 200);
    }
}

/// `who` joined on level 201, its intro skipped as the swap tests skip it,
/// then a second of standing.
fn joined_on_201(who: Character) -> Run {
    let setup = Setup {
        interface: true,
        props: true,
        sounds: true,
        character: Some(who),
        ..Setup::default()
    };
    let (mut run, _) = run_on_level(Mode::Play, LEVEL_201, setup);
    run.step().unwrap();
    run.step().unwrap();
    run.join().unwrap_or_else(|h| panic!("{h}"));
    for k in 0..3000u32 {
        play(&mut run, [false, false, false, k > 120 && k % 20 == 10]);
        if k > 300 && cutscene_name(&run).is_none() && !guacamole::anchors::scene_holds(&run.vm) {
            break;
        }
    }
    for _ in 0..60 {
        play(&mut run, [false; 4]);
    }
    run
}

/// His attack: `action` held for `hold` steps of `steps`, what draws
/// recorded each step (before the press first), the run stopping once
/// what draws has been as it was before the press for `settle` steps in a
/// row after `min` steps (a stall ends it too: the cap).
fn attack(
    run: &mut Run,
    fresh: &[(String, String)],
    action: usize,
    hold: u32,
    steps: u32,
) -> Vec<(u32, Drawn)> {
    let mut out = vec![(run.steps, Drawn::of(run, fresh))];
    for k in 0..steps {
        let mut a = [false; 4];
        a[action] = k < hold;
        play(run, a);
        out.push((run.steps, Drawn::of(run, fresh)));
    }
    out
}

fn print(who: &str, seen: &[(u32, Drawn)]) {
    let mut last: Option<&Drawn> = None;
    for (step, d) in seen {
        if last != Some(d) {
            println!(
                "{who} step {step}: body {} scale {:?} {:?}",
                d.body, d.scale, d.models
            );
        }
        last = Some(d);
    }
}

/// Ninjini's bottle (her second action): her scripts squeeze her across,
/// then set her tint's alpha to 0 while the bottle, hung on her armature,
/// stands for her, and bring her back. Drawn: the bottle once on every
/// step, her body hidden while she's in it and back after, her scale drawn
/// per axis while it squeezes.
#[test]
#[ignore = "reads GUAC_GIANTS_DUMP; run on the Mac"]
fn ninjinis_bottle_hides_her_and_draws_it_once() {
    let fresh = fresh_parts(NINJINI.archive, NINJINI.load_file);
    println!("fresh parts {fresh:?}");
    assert_eq!(fresh.len(), 1, "her bottle");
    let bottle = fresh[0].1.clone();
    let mut run = joined_on_201(NINJINI);
    let seen = attack(&mut run, &fresh, 1, 180, 420);
    print("ninjini", &seen);
    for (step, d) in &seen {
        assert_eq!(d.count(&bottle), 1, "step {step}: her bottle once: {d:?}");
    }
    let before = &seen[0].1;
    assert!(before.body, "her body before");
    let hidden: Vec<u32> = seen
        .iter()
        .filter(|(_, d)| !d.body)
        .map(|(s, _)| *s)
        .collect();
    println!("hidden at steps {:?}", hidden.first().zip(hidden.last()));
    assert!(
        hidden.len() > 60,
        "in her bottle a second at least: {}",
        hidden.len()
    );
    let squeezed = seen
        .iter()
        .filter(|(_, d)| d.body && d.scale[0] < 75.0 && d.scale[1] > d.scale[0] + 20.0)
        .count();
    assert!(squeezed > 0, "squeezed across as she's drawn in");
    let back = seen.iter().rposition(|(_, d)| !d.body).unwrap();
    let after = &seen[seen.len() - 1].1;
    assert!(back + 1 < seen.len(), "out of her bottle before the end");
    assert_eq!(after, before, "as she was once she's out");
}

const STEALTH_ELF: Character = Character {
    archive: "character/026_stealthelf.bld",
    load_file: "026_StealthElf",
    toy: 26,
};

/// Stealth Elf's stealth (her second action, the AS.444 review): her
/// scripts hide her and her knives through `Minion - Set Minion Tint`, wait
/// on her go-invisible clip by its palette entry, then on a 2 s timer, and
/// bring her back. With that wait read as nothing she stayed see-through
/// for good. Hidden a second at least, back within 420 steps of a tap; a
/// second press mid-stealth ends it too; taken off mid-stealth, the next
/// figure stands drawn.
#[test]
#[ignore = "reads GUAC_GIANTS_DUMP; run on the Mac"]
fn stealth_elfs_stealth_ends_and_draws_her_again() {
    let fresh = fresh_parts(STEALTH_ELF.archive, STEALTH_ELF.load_file);
    let mut run = joined_on_201(STEALTH_ELF);
    let seen = attack(&mut run, &fresh, 1, 10, 420);
    print("stealth elf", &seen);
    let hidden = seen.iter().filter(|(_, d)| !d.body).count();
    assert!(hidden > 60, "unseen a second at least: {hidden}");
    let last = seen.iter().rposition(|(_, d)| !d.body).unwrap();
    assert!(
        last + 60 < seen.len(),
        "drawn again a second before the end: {last}"
    );
    assert!(seen[seen.len() - 1].1.body);
    // A second press while she's unseen.
    let seen = attack(&mut run, &fresh, 1, 10, 30);
    assert!(seen.iter().any(|(_, d)| !d.body), "unseen again");
    let again = attack(&mut run, &fresh, 1, 10, 420);
    assert!(again[again.len() - 1].1.body, "drawn after a second press");
    // Taken off mid-stealth and put back: drawn.
    let seen = attack(&mut run, &fresh, 1, 10, 30);
    assert!(
        seen.iter().any(|(_, d)| !d.body),
        "unseen before the take-off"
    );
    run.deactivate().unwrap_or_else(|h| panic!("{h}"));
    for _ in 0..30 {
        play(&mut run, [false; 4]);
    }
    run.join().unwrap_or_else(|h| panic!("{h}"));
    let after = attack(&mut run, &fresh, 1, 0, 60);
    assert!(after.iter().all(|(_, d)| d.body), "the next figure drawn");
}

/// Chop Chop's sword (his first action): his scripts hang his sword and
/// shield as clones of their own, which the extract lists on a fresh
/// figure too. Each draws once on every step of his swing, his body all
/// through.
#[test]
#[ignore = "reads GUAC_GIANTS_DUMP; run on the Mac"]
fn chop_chops_swing_draws_one_sword() {
    let fresh = fresh_parts(CHOP_CHOP.archive, CHOP_CHOP.load_file);
    println!("fresh parts {fresh:?}");
    assert_eq!(fresh.len(), 2, "his shield and sword");
    let mut run = joined_on_201(CHOP_CHOP);
    let seen = attack(&mut run, &fresh, 0, 10, 120);
    print("chop chop", &seen);
    for (step, d) in &seen {
        assert!(d.body, "step {step}: his body");
        for (_, file) in &fresh {
            assert_eq!(d.count(file), 1, "step {step}: {file} once: {d:?}");
        }
    }
}

/// Warnado (openspec AS.444's third): his spin (first action) hangs him on
/// a dummy, his club feet on him, and plays his shell clip on his own body;
/// his tornado (second) throws a tornado off him. His scripts hide nothing
/// of him in either, so his body draws all through: what the TV draws, as
/// asked. His feet draw once mid-spin and are gone after; nothing the
/// extract lists for him.
#[test]
#[ignore = "reads GUAC_GIANTS_DUMP; run on the Mac"]
fn warnados_spin_and_tornado_keep_his_body() {
    let fresh = fresh_parts(WARNADO.archive, WARNADO.load_file);
    assert!(fresh.is_empty(), "{fresh:?}");
    let mut run = joined_on_201(WARNADO);
    let spin = attack(&mut run, &fresh, 0, 180, 240);
    print("warnado spin", &spin);
    let before = spin[0].1.clone();
    let feet: Vec<&Drawn> = spin
        .iter()
        .map(|(_, d)| d)
        .filter(|d| d.models.len() > before.models.len())
        .collect();
    assert!(!feet.is_empty(), "his feet hung on him mid-spin");
    assert!(feet.iter().all(|d| d.models.values().all(|&n| n == 1)));
    assert!(spin.iter().all(|(_, d)| d.body), "his body all through");
    assert_eq!(spin.last().unwrap().1, before, "as he was after");
    let tornado = attack(&mut run, &fresh, 1, 120, 300);
    print("warnado tornado", &tornado);
    assert!(tornado.iter().all(|(_, d)| d.body), "his body all through");
    assert!(
        tornado
            .iter()
            .any(|(_, d)| d.models.len() > before.models.len()),
        "his tornado hung on him as it starts"
    );
}

/// Parts the extract hangs on a fresh figure that his own scripts keep
/// see-through while he stands, so they draw nowhere once he's joined:
/// Eye-Brawl's optic nerve (alpha 0 standing, stretched to his eye). Every
/// other named part on the roster draws as a clone of his scripts.
const SEE_THROUGH_STANDING: &[&str] = &["114_Optic_Nerve"];

/// The roster check under AS.444's rule: every named part the extract
/// hangs on a fresh figure is a clone his own scripts made once he's
/// joined and the TV draws (hung on something live, a model, not
/// see-through), so the extract's draws give way to theirs without a part
/// going missing; [`SEE_THROUGH_STANDING`]'s are the ones his scripts hide.
/// A few figures by default, `GUAC_HERO_PARTS_ALL=1` every load file.
#[test]
#[ignore = "reads GUAC_GIANTS_DUMP; run on the Mac"]
fn every_fresh_part_is_one_his_scripts_hang() {
    let usr = dump().join("PS3_GAME/USRDIR/character");
    let all = std::env::var_os("GUAC_HERO_PARTS_ALL").is_some();
    let mut loads: Vec<String> = std::fs::read_dir(&usr)
        .unwrap()
        .filter_map(|e| e.ok()?.file_name().into_string().ok())
        .filter_map(|n| n.strip_suffix(".bld").map(str::to_owned))
        .filter(|n| n.as_bytes().first().is_some_and(u8::is_ascii_digit))
        .collect();
    loads.sort();
    if !all {
        let few = [
            "014_gillgrunt",
            "026_stealthelf",
            "030_pandoranelite",
            "109_djini",
            "114_mummy",
        ];
        loads.retain(|l| few.contains(&l.as_str()));
    }
    let start = std::time::Instant::now();
    let (mut checked, mut carrying, mut parts) = (0, 0, 0);
    for archive in &loads {
        let path: &'static str = Box::leak(format!("character/{archive}.bld").into_boxed_str());
        // The load file as the archive's own hero names it (any case).
        let bytes = level_bld(&usr.join(format!("{archive}.bld")));
        let igz = Igz::parse(&bytes).unwrap();
        let Ok(portal) = model::portal_model(&igz) else {
            println!("{archive}: no portal model, not a figure's");
            continue;
        };
        let Ok(hero) = model::hero_actor(&igz, &portal, archive) else {
            println!("{archive}: no hero actor, not one the roster loads");
            continue;
        };
        let load_file: &'static str =
            Box::leak(hero.name().unwrap_or(archive).to_owned().into_boxed_str());
        let toy: u16 = archive[..3].parse().unwrap();
        let fresh = fresh_parts(path, load_file);
        let setup = Setup {
            interface: true,
            props: true,
            sounds: true,
            character: Some(Character {
                archive: path,
                load_file,
                toy,
            }),
            ..Setup::default()
        };
        let (mut run, _) = run_on_level(Mode::Play, LEVEL_201, setup);
        run.step().unwrap();
        run.step().unwrap();
        run.join().unwrap_or_else(|h| panic!("{load_file}: {h}"));
        for _ in 0..30 {
            play(&mut run, [false; 4]);
        }
        // His scripts' live clones by placement that the TV draws: hung on
        // something live, a model, not see-through (the AS.444 review: a
        // clone at alpha 0 counted as carried and drew nothing).
        let live: Vec<(String, Option<String>, String)> = run
            .world
            .clones()
            .into_iter()
            .filter(|&c| !run.world.removed(c) && !run.world.hangs_on_nothing(c))
            .filter_map(|c| {
                let a = run.world.actor(c)?;
                let model = a
                    .template
                    .and_then(|t| run.world.template(t))
                    .and_then(|p| p.model.as_deref())
                    .map(file_name)?;
                actor_shows(a).then(|| {
                    (
                        run.world.name(c),
                        owner(&run, c).map(|o| run.world.name(o)),
                        model,
                    )
                })
            })
            .collect();
        println!("{load_file}: fresh {fresh:?}");
        for (placement, file) in &fresh {
            // A global template's part has no placement name to find its
            // clone by (three `inviso.mdl`s on the roster).
            if placement.is_empty() {
                println!("   ({file}): a global template's, not matched");
                continue;
            }
            let want = fresh.iter().filter(|(p, _)| p == placement).count();
            let mine: Vec<_> = live.iter().filter(|(n, ..)| n == placement).collect();
            println!("   {placement} ({file}): {mine:?}");
            if SEE_THROUGH_STANDING.contains(&placement.as_str()) {
                assert!(mine.is_empty(), "{placement} drawn standing now: {mine:?}");
                continue;
            }
            assert!(
                mine.len() >= want,
                "{load_file}: {placement} ({file}) a drawn clone of his scripts: {mine:?}"
            );
        }
        checked += 1;
        carrying += usize::from(!fresh.is_empty());
        parts += fresh.len();
    }
    println!(
        "{checked} figures, {carrying} carrying, {parts} parts, each a live clone of his scripts, {:.0} s, load {}",
        start.elapsed().as_secs_f32(),
        load()
    );
    assert!(checked >= 5);
}

const SPROCKET: Character = Character {
    archive: "character/111_turretmaster.bld",
    load_file: "111_TurretMaster",
    toy: 111,
};

/// Sprocket's turret fades in (openspec AS.690): her piece macro clones
/// each piece at alpha 0 and hands its `placement.tint` alpha by reference
/// to her own slide macro, which writes it up to 255 each step. Each of the
/// four pieces is drawn (its alpha over 0) and reaches 255 within a second
/// of its spawn; before AS.690 the reference held a number of its own and
/// every piece stayed at alpha 0 to its expiry.
#[test]
#[ignore = "reads GUAC_GIANTS_DUMP; run on the Mac"]
fn sprockets_turret_fades_in_through_her_slide() {
    let mut run = joined_on_201(SPROCKET);
    // Per piece: the step it was first seen, the first step its alpha read
    // over 0, the first it read 255.
    let mut seen: BTreeMap<String, (u32, Option<u32>, Option<u32>)> = BTreeMap::new();
    for k in 0..400u32 {
        play(&mut run, [false, k < 200, false, false]);
        for c in run.world.clones() {
            let name = run.world.name(c);
            if !name.contains("Turret_00") || run.world.removed(c) {
                continue;
            }
            let alpha = run.world.actor(c).unwrap().tint & 0xFF;
            let e = seen.entry(name).or_insert((k, None, None));
            if alpha > 0 && e.1.is_none() {
                e.1 = Some(k);
            }
            if alpha == 0xFF && e.2.is_none() {
                e.2 = Some(k);
            }
        }
    }
    println!("pieces (spawned, drawn, whole): {seen:?}");
    for piece in ["Bottom", "Top", "Body", "Seat"] {
        let (spawned, drawn, whole) = *seen
            .get(&format!("111_Turret_00_{piece}"))
            .unwrap_or_else(|| panic!("her turret's {piece} spawned: {seen:?}"));
        let drawn = drawn.unwrap_or_else(|| panic!("{piece} drawn: {seen:?}"));
        assert!(
            drawn - spawned < 30,
            "{piece} drawn within a second: {seen:?}"
        );
        let whole = whole.unwrap_or_else(|| panic!("{piece} reaches 255: {seen:?}"));
        assert!(
            whole - spawned < 60,
            "{piece} whole within a second of its spawn: {seen:?}"
        );
    }
}

/// Each live clone hung on one of `run`'s hero's posed nodes, by its
/// template's model file: how far it stands from that node as his body is
/// drawn now (his pivot, turned by his facing and his lean).
fn off_his_nodes(run: &Run) -> Vec<(String, f32)> {
    let hero = run.hero();
    let drawn = hero.rotation();
    let pivot = hero.pivot;
    let Some(nodes) = run.world.nodes(run.hero) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for c in run.world.clones() {
        if run.world.removed(c) {
            continue;
        }
        let Some(a) = run.world.actor(c) else {
            continue;
        };
        let Some((owner, i)) = run.world.node_of(a.attach) else {
            continue;
        };
        if owner != run.hero {
            continue;
        }
        let Some(node) = nodes.get(i as usize) else {
            continue;
        };
        let model = a
            .template
            .and_then(|t| run.world.template(t))
            .and_then(|p| p.model.as_deref())
            .map_or_else(|| format!("clone {c:#x}"), file_name);
        out.push((model, a.position.distance(pivot + drawn * node.at)));
    }
    out.sort_by(|a, b| a.0.cmp(&b.0));
    out
}

/// openspec AS.1124: what a hero's scripts hang on his nodes (Chill's
/// helmet, shield and javelin) stays on his body as it's drawn leaned on a
/// slope as well as upright: chotchki's 2026-11-27 playtest saw her helmet
/// and shield off her on the hub's stairs, where she leaned 31°, each
/// 0.8 m off the node it hangs on. Each hero joined on 201 stands upright,
/// then leaned that much (his controller's `up`, as a slope leaves it,
/// set for one posing), and every clone on his nodes is held within 2 cm
/// of its upright distance from where its node is drawn (on it, or at the
/// offset his scripts hang it at). `GUAC_HERO_PARTS_ALL=1` takes
/// every numbered character archive in place of the few.
#[test]
#[ignore = "reads GUAC_GIANTS_DUMP; run on the Mac"]
fn what_hangs_on_a_hero_stays_on_him_leaned() {
    let usr = dump().join("PS3_GAME/USRDIR/character");
    let all = std::env::var_os("GUAC_HERO_PARTS_ALL").is_some();
    let mut loads: Vec<String> = std::fs::read_dir(&usr)
        .unwrap()
        .filter_map(|e| e.ok()?.file_name().into_string().ok())
        .filter_map(|n| n.strip_suffix(".bld").map(str::to_owned))
        .filter(|n| n.as_bytes().first().is_some_and(u8::is_ascii_digit))
        .collect();
    loads.sort();
    if !all {
        loads.retain(|l| ["106_icesorc", "030_pandoranelite"].contains(&l.as_str()));
    }
    let start = std::time::Instant::now();
    let (mut with_props, mut props) = (0, 0);
    for archive in &loads {
        let path: &'static str = Box::leak(format!("character/{archive}.bld").into_boxed_str());
        let bytes = level_bld(&usr.join(format!("{archive}.bld")));
        let igz = Igz::parse(&bytes).unwrap();
        let Ok(portal) = model::portal_model(&igz) else {
            continue;
        };
        let Ok(hero) = model::hero_actor(&igz, &portal, archive) else {
            continue;
        };
        let load_file: &'static str =
            Box::leak(hero.name().unwrap_or(archive).to_owned().into_boxed_str());
        let toy: u16 = archive[..3].parse().unwrap();
        let mut run = joined_on_201(Character {
            archive: path,
            load_file,
            toy,
        });
        run.pose_hands();
        let upright = off_his_nodes(&run);
        let up = run.hero().up;
        run.bench.heroes.get_mut(&run.hero).unwrap().up =
            Quat::from_rotation_x(31f32.to_radians()) * Vec3::Y;
        run.pose_hands();
        let leaned = off_his_nodes(&run);
        run.bench.heroes.get_mut(&run.hero).unwrap().up = up;
        println!("{load_file}: upright {upright:?}, leaned {leaned:?}");
        assert_eq!(
            upright.iter().map(|p| &p.0).collect::<Vec<_>>(),
            leaned.iter().map(|p| &p.0).collect::<Vec<_>>(),
            "{load_file}: the same clones hang"
        );
        // Each stands as far from its node leaned as upright: on it, or at
        // the offset his scripts hang it at (Camo's `inviso.mdl` 1 ft off).
        for ((model, up), (_, lean)) in upright.iter().zip(&leaned) {
            assert!(
                (lean - up).abs() < 0.02,
                "{load_file}: {model} {lean} m off its node leaned, {up} upright"
            );
        }
        if !leaned.is_empty() {
            with_props += 1;
            props += leaned.len();
        }
    }
    println!(
        "{} heroes, {with_props} with clones on their nodes ({props}), {:.0} s",
        loads.len(),
        start.elapsed().as_secs_f64()
    );
    if !all {
        assert!(with_props >= 1, "Chill hangs her props on her nodes");
    }
}
