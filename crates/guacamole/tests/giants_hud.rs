//! AT.9's Verify on the Giants dump: the original's interface scripts
//! writing the interface's model on the TV's run, headless over guacamole's
//! host (the join needs its world: the figure's entry, the portal manager's
//! slot, the hero's template). Eruptor's join builds the HUD (`UI - Create
//! Player Health UI `) with every member on the model and no stop: each
//! instance with its template, parent and rectangle, the main box where
//! the build puts it. The draw list beside AC.13's quads is printed, its
//! check within 1 px AT.39's (split from AT.9).
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
    let mut within = 0;
    let hud: Vec<_> = AC13_QUADS
        .iter()
        .enumerate()
        .filter(|(i, _)| !PARENTLESS.contains(i))
        .collect();
    for (i, (image, want)) in &hud {
        let off = arts
            .iter()
            .filter(|(s, _)| s == image)
            .map(|(_, r)| (0..4).map(|c| (r[c] - want[c]).abs()).fold(0.0, f32::max))
            .fold(f32::INFINITY, f32::min);
        within += usize::from(off <= 1.0);
        println!("  AC.13 quad {i:>2} {image:<26} {want:?}: nearest of its image {off:.2} px off");
    }
    println!(
        "HUD quads within 1 px: {within} of {}; draw list {} entries ({} art); {:.1} s; load after: {}",
        hud.len(),
        list.len(),
        arts.len(),
        started.elapsed().as_secs_f32(),
        load()
    );
}
