//! AT.9's Verify on the Giants dump: the original's interface scripts
//! writing the interface's model on the TV's run, headless over guacamole's
//! host (the join needs its world: the figure's entry, the portal manager's
//! slot, the hero's template). Eruptor's join builds the HUD (`UI - Create
//! Player Health UI `) with every member on the model and no stop: each
//! instance with its template, parent and rectangle, the main box where
//! the build puts it. The draw list beside AC.13's quads is printed, its
//! check within 1 px AT.39's (split from AT.9). Two probes from AT.9's
//! review beside it: a step's cost with the model and without, and the HUD
//! under other title-safe margins.
//!
//! Ignored by default: it reads `GUAC_GIANTS_DUMP` (env, else `.env.dev`).
//!
//! `cargo test -p guacamole --profile disc --test giants_hud -- --ignored --nocapture`

mod common;

use common::{Setup, joined_with, load};
use guac_interface::NoWorld;
use guac_interface::draw::Entry;
use guac_interface::sprite::{Source, props};
use guac_ir::interface::Content;
use guac_script::Mode;

/// AC.13's 59 quads in capture 112455's draw order (AT.3's reading,
/// `at-lane/out/at3_draws.tsv`): each image's file stem and its rectangle in
/// the 1280x720 frame, numbers only. The 8 parentless (the four flash
/// squares, `gray` at 450-453, and the four corner shades, `black`) aren't
/// the HUD's.
#[allow(clippy::excessive_precision)]
const AC13_QUADS: [(&str, [f32; 4]); 59] = [
    ("gray", [139.00, 609.00, 385.00, 639.00]),
    ("textbox3_cornerfill", [385.00, 585.00, 409.00, 609.00]),
    ("textbox3_cornerfill", [385.00, 639.00, 409.00, 663.00]),
    ("textbox3_cornerfill", [115.00, 585.00, 139.00, 609.00]),
    ("textbox3_cornerfill", [115.00, 639.00, 139.00, 663.00]),
    ("textbox3_tilefill", [115.00, 609.00, 139.00, 639.00]),
    ("textbox3_tilefill", [139.00, 639.00, 385.00, 663.00]),
    ("textbox3_tilefill", [139.00, 585.00, 385.00, 609.00]),
    ("textbox3_tilefill", [385.00, 609.00, 409.00, 639.00]),
    ("gray", [340.00, -600.00, 940.00, -0.00]),
    ("gray", [-600.00, 60.00, 0.00, 660.00]),
    ("gray", [1280.00, 60.00, 1880.00, 660.00]),
    ("gray", [340.00, 720.00, 940.00, 1320.00]),
    ("black", [1080.00, -0.00, 1280.00, 200.00]),
    ("black", [0.00, -0.00, 200.00, 200.00]),
    ("black", [1080.00, 520.00, 1280.00, 720.00]),
    ("portraitbackdrop", [35.50, 553.50, 176.50, 694.50]),
    ("portraitbackdrop", [43.00, 560.25, 170.50, 687.75]),
    ("eruptor", [43.00, 560.25, 170.50, 687.75]),
    ("hpbar_capleft", [146.50, 593.25, 158.50, 630.75]),
    ("hpbar_capleft", [146.50, 593.25, 158.50, 630.75]),
    ("hpbar_capleft", [146.50, 593.25, 158.50, 630.75]),
    ("hpbar_capleft", [388.00, 593.25, 400.00, 630.75]),
    ("hpbar_capleft", [388.00, 593.25, 400.00, 630.75]),
    ("hpbar_capleft", [388.00, 593.25, 400.00, 630.75]),
    ("hpbar_tile", [158.50, 593.25, 388.00, 630.75]),
    ("hpbar_tile", [158.50, 593.25, 388.00, 630.75]),
    ("hpbar_tile", [158.50, 593.25, 388.00, 630.75]),
    ("xpbar", [157.00, 635.25, 163.00, 651.75]),
    ("xpbar", [157.00, 635.25, 163.00, 651.75]),
    ("xpbar", [157.00, 635.25, 163.00, 651.75]),
    ("fire_64", [145.75, 541.50, 205.75, 601.50]),
    ("toyportrait_ring2011", [43.00, 560.25, 170.50, 687.75]),
    ("hpbar_capleft", [388.00, 593.25, 400.00, 630.75]),
    ("hpbar_capleft", [388.00, 593.25, 400.00, 630.75]),
    ("hpbar_capleft", [388.00, 593.25, 400.00, 630.75]),
    ("hpbar_capleft", [146.50, 593.25, 158.50, 630.75]),
    ("hpbar_capleft", [146.50, 593.25, 158.50, 630.75]),
    ("hpbar_capleft", [146.50, 593.25, 158.50, 630.75]),
    ("hpbar_tile", [158.50, 593.25, 388.00, 630.75]),
    ("hpbar_tile", [158.50, 593.25, 388.00, 630.75]),
    ("hpbar_tile", [158.50, 593.25, 388.00, 630.75]),
    ("fire_64", [142.75, 538.50, 202.75, 598.50]),
    ("fire_64stroke", [142.75, 538.50, 202.75, 598.50]),
    ("coin", [358.56, 664.50, 382.56, 688.50]),
    ("hpbarshiny_leftcap", [146.50, 593.25, 158.50, 630.75]),
    ("hpbarshiny_leftcap", [146.50, 593.25, 158.50, 630.75]),
    ("hpbarshiny_leftcap", [146.50, 593.25, 158.50, 630.75]),
    ("hpbarshiny_tile", [158.50, 593.25, 388.00, 630.75]),
    ("hpbarshiny_tile", [158.50, 593.25, 388.00, 630.75]),
    ("hpbarshiny_tile", [158.50, 593.25, 388.00, 630.75]),
    ("hpbarshiny_capright", [388.00, 593.25, 400.00, 630.75]),
    ("hpbarshiny_capright", [388.00, 593.25, 400.00, 630.75]),
    ("hpbarshiny_capright", [388.00, 593.25, 400.00, 630.75]),
    ("xptext", [157.00, 658.50, 218.13, 691.50]),
    ("xptext", [222.63, 654.00, 235.94, 696.00]),
    ("cointext", [382.56, 664.50, 401.50, 688.50]),
    ("portraitbackdrop", [35.50, 553.50, 176.50, 694.50]),
    ("black", [0.00, 520.00, 200.00, 720.00]),
];

/// The parentless 8 among [`AC13_QUADS`], by place in it.
const PARENTLESS: [usize; 8] = [9, 10, 11, 12, 13, 14, 15, 58];

/// An image's file stem, lowercased, as AT.3's table names it.
fn stem(source: &str) -> String {
    let file = source.rsplit('/').next().unwrap_or(source);
    file.split('.').next().unwrap_or(file).to_ascii_lowercase()
}

/// Each of AC.13's HUD quads (all but [`PARENTLESS`]) with how far its
/// nearest draw-list entry of its image is, the worst corner coordinate.
fn hud_offsets(
    m: &guac_interface::Model,
    frame: &guac_interface::layout::Frame,
) -> Vec<(usize, f32)> {
    let list = m.draw_list(frame);
    let arts: Vec<(String, [f32; 4])> = list
        .iter()
        .filter_map(|e| match e {
            Entry::Art { sprite, rect, .. } => match &m.sprite(*sprite).content {
                Content::Image(a) => Some((stem(a.source.as_deref().unwrap_or("")), *rect)),
                _ => None,
            },
            _ => None,
        })
        .collect();
    AC13_QUADS
        .iter()
        .enumerate()
        .filter(|(i, _)| !PARENTLESS.contains(i))
        .map(|(i, (image, want))| {
            let off = arts
                .iter()
                .filter(|(s, _)| s == image)
                .map(|(_, r)| (0..4).map(|c| (r[c] - want[c]).abs()).fold(0.0, f32::max))
                .fold(f32::INFINITY, f32::min);
            (i, off)
        })
        .collect()
}

#[test]
#[ignore = "reads the Giants dump (GUAC_GIANTS_DUMP)"]
fn eruptors_join_builds_the_hud() {
    println!("load before: {}", load());
    let started = std::time::Instant::now();
    let setup = Setup {
        interface: true,
        ..Setup::default()
    };
    // Strict: any stop panics.
    let (run, _) = joined_with(Mode::Strict, setup);
    let k = run.world.interface().expect("the run holds the model");
    let mut m = k.model().clone();
    let frame = m.lay_out(&NoWorld);
    let name = |id: guac_interface::SpriteId| m.sprite(id).name.clone();
    let instances = k.instances();
    println!("instances: {}", instances.len());
    for &id in &instances {
        let s = m.sprite(id);
        let from = match s.cloned_from {
            Source::Sprite(t) => name(t),
            Source::None => "-".into(),
        };
        let parent = s.parent.map_or("-".into(), name);
        let r = frame.get(id).map(|p| p.rect());
        println!(
            "  {:<30} from {:<28} on {:<26} rect {:?}",
            s.name,
            from,
            parent,
            r.map(|r| r.map(|v| (v * 100.0).round() / 100.0))
        );
    }
    // The templates awake from the load, by their files' activity states.
    let woken: Vec<String> = m
        .active()
        .iter()
        .filter(|&&id| m.sprite(id).cloned_from == Source::None)
        .map(|&id| m.sprite(id).name.clone())
        .collect();
    println!("templates awake: {} {woken:?}", woken.len());
    // Each one the June savestate's live sprite list holds (AT.3's read of
    // it, `at-lane/notes/live_sprites.txt`), the viewport's container first
    // there and last here.
    assert_eq!(
        woken,
        [
            "PortalUI_Container",
            "GameplayOverlay_BL",
            "GameplayOverlay_BR",
            "GameplayOverlay_UL",
            "GameplayOverlay_UR",
            "darkness_Fade_out",
            "CSMetrics_CameraSpr",
            "CSMetrics_CameraVal",
            "CSMetrics_CSTotalSpr",
            "CSMetrics_CSTotalVal",
            "CSMetrics_TimeSpr",
            "CSMetrics_TimeVal",
            "RT Viewport 0",
            "_Screen_Container",
            "_UI_Container",
            "_UI_PauseContainer",
            "_Gameplay_Container",
        ]
    );
    assert!(k.strays().is_empty(), "strays: {:?}", k.strays());
    assert!(k.unknown().is_empty(), "unknown: {:?}", k.unknown());
    // The main box: a clone of `Generic_Container` hung on a clone of
    // `_Gameplay_Container`, 237 by 96, its bottom-left on its parent's
    // bottom-left, 20 up (location 0x0000FEC0).
    let boxes: Vec<_> = instances
        .iter()
        .copied()
        .filter(|&id| {
            let s = m.sprite(id);
            let on_gameplay = s.parent.is_some_and(|p| {
                m.sprite(p).name == "_Gameplay_Container" && m.sprite(p).cloned_from != Source::None
            });
            s.name == "Generic_Container"
                && on_gameplay
                && s.size[..2] == [237.0, 96.0]
                && props::ALIGNMENT.get(s.properties) == 6
                && props::ALIGN_TO_PARENT.get(s.properties) == 6
                && s.location == [0, -320]
        })
        .collect();
    assert_eq!(boxes.len(), 1, "the main box, once");
    let main = frame.get(boxes[0]).unwrap();
    println!("main box at {:?} on screen", main.rect());
    assert_eq!(instances.len(), 60, "the HUD's instances at the join");
    // The draw list beside AC.13's quads: each HUD quad's nearest entry of
    // its image, how far off (AT.39 holds them within 1 px).
    let list = m.draw_list(&frame);
    let arts = list
        .iter()
        .filter(|e| {
            matches!(e, Entry::Art { sprite, .. }
                if matches!(m.sprite(*sprite).content, Content::Image(_)))
        })
        .count();
    let offsets = hud_offsets(&m, &frame);
    let within = offsets.iter().filter(|(_, off)| *off <= 1.0).count();
    for &(i, off) in &offsets {
        let (image, want) = AC13_QUADS[i];
        println!("  AC.13 quad {i:>2} {image:<26} {want:?}: nearest of its image {off:.2} px off");
    }
    println!(
        "HUD quads within 1 px: {within} of {}; draw list {} entries ({arts} art); {:.1} s; load after: {}",
        offsets.len(),
        list.len(),
        started.elapsed().as_secs_f32(),
        load()
    );
}

/// The strings the model holds: the converter makes one at each `value set`
/// write, and none is ever let go within a run.
fn strings_held(m: &guac_interface::Model) -> u64 {
    let base = m.bases().strings;
    let held = |n: u64| m.string(guac_script::Value::Entity(base + n)).is_some();
    if !held(0) {
        return 0;
    }
    let mut hi = 1;
    while held(hi) {
        hi *= 2;
    }
    let mut lo = hi / 2;
    while lo + 1 < hi {
        let mid = (lo + hi) / 2;
        if held(mid) {
            lo = mid;
        } else {
            hi = mid;
        }
    }
    lo + 1
}

/// A development probe for AT.9's cost (the review's): the run from the
/// level's start with its props and sounds, joined and standing, in play
/// mode, `AT9_STEPS` steps (1,800) with the interface's model and without,
/// `AT9_ROUNDS` times (2) alternately; each round's ops and wall time a
/// step, and the model's sprites and strings as it grows.
#[test]
#[ignore = "reads GUAC_GIANTS_DUMP; a probe"]
fn probe_a_steps_cost_with_and_without_the_model() {
    let steps: u32 = std::env::var("AT9_STEPS")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(1_800);
    let rounds: u32 = std::env::var("AT9_ROUNDS")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(2);
    for round in 0..rounds {
        for interface in [false, true] {
            let (mut run, _) = joined_with(
                Mode::Play,
                Setup {
                    props: true,
                    sounds: true,
                    interface,
                    ..Setup::default()
                },
            );
            let before = load();
            let ops = run.vm.executed();
            let started = std::time::Instant::now();
            for k in 0..steps {
                let stopped = run.step_play().unwrap_or_else(|h| panic!("{h}"));
                assert!(stopped.is_empty(), "step {k}: {stopped:?}");
                if interface && k % 600 == 0 {
                    let m = run.world.interface().unwrap().model();
                    println!(
                        "  step {k}: {} sprites, {} strings, {} awake",
                        m.len(),
                        strings_held(m),
                        m.active().len()
                    );
                }
            }
            let took = started.elapsed();
            let ops = run.vm.executed() - ops;
            let grown = run.world.interface().map(|k| {
                let m = k.model();
                (m.len(), strings_held(m), m.active().len())
            });
            println!(
                "round {round} model {interface}: {} ops a step, {:.0} us a step ({steps} steps, {:.2?}); model {grown:?}; load {before} -> {}",
                ops / u64::from(steps),
                took.as_secs_f64() * 1e6 / f64::from(steps),
                took,
                load()
            );
        }
    }
}

/// A development probe for AT.9's title-safe Hypothesis (the review's): the
/// join's HUD with the margins `AT9_MARGINS` gives (`left,right,upper,lower`
/// in frame pixels, else the keeper's), `_UI_Container`'s and the main
/// box's rectangles on screen.
#[test]
#[ignore = "reads GUAC_GIANTS_DUMP; a probe"]
fn probe_the_hud_under_other_title_safe_margins() {
    let margins: Option<[i32; 4]> = std::env::var("AT9_MARGINS")
        .ok()
        .filter(|s| !s.is_empty())
        .map(|s| {
            let v: Vec<i32> = s.split(',').map(|n| n.trim().parse().unwrap()).collect();
            [v[0], v[1], v[2], v[3]]
        });
    let (mut run, _) = common::run_on_with(
        Mode::Strict,
        Setup {
            interface: true,
            ..Setup::default()
        },
    );
    if let Some(m) = margins {
        let mut k = run.world.interface().unwrap().clone();
        k.model_mut().title_safe = m;
        run.world.set_interface(k);
    }
    run.step().unwrap_or_else(|h| panic!("{h}"));
    run.step().unwrap_or_else(|h| panic!("{h}"));
    run.join().unwrap_or_else(|h| panic!("{h}"));
    for _ in 0..120 {
        run.step().unwrap_or_else(|h| panic!("{h}"));
    }
    let k = run.world.interface().unwrap();
    let mut m = k.model().clone();
    println!("margins {:?}", m.title_safe);
    let frame = m.lay_out(&NoWorld);
    let offsets = hud_offsets(&m, &frame);
    let within = offsets.iter().filter(|(_, o)| *o <= 1.0).count();
    let finite: Vec<f32> = offsets
        .iter()
        .map(|(_, o)| *o)
        .filter(|o| o.is_finite())
        .collect();
    println!(
        "HUD quads within 1 px: {within} of {}; finite {} with median {:?}; {offsets:?}",
        offsets.len(),
        finite.len(),
        {
            let mut f = finite.clone();
            f.sort_by(f32::total_cmp);
            f.get(f.len() / 2).copied()
        }
    );
    for &id in m.active() {
        let s = m.sprite(id);
        let named = matches!(
            s.name.as_str(),
            "_UI_Container" | "_Gameplay_Container" | "Generic_Container" | "_Screen_Container"
        );
        if named {
            println!(
                "  {:<22} clone {:<5} size {:?} at {:?}",
                s.name,
                s.cloned_from != Source::None,
                &s.size[..2],
                frame.get(id).map(|p| p.rect())
            );
        }
        if s.cloned_from != Source::None
            && let Some(u) = m.text_units(id, &NoWorld)
        {
            println!(
                "  text {:<20} {:?} value {:?}",
                s.name,
                String::from_utf16_lossy(&u),
                s.numeric_value
            );
        }
    }
}
