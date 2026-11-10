//! A hero swapped mid-level (openspec AS.218), headless with
//! guacamole's host on the TV's listing for level 201: the life island's
//! summon, whose three evil Stealth Elves land through entry beams of their
//! own, with a figure swapped in as it comes. Ignored by default; they read
//! `GUAC_GIANTS_DUMP` (process env, else `.env.dev`):
//!
//! `cargo test -p guacamole --profile disc --test giants_swap -- --ignored --nocapture`

use guac_motion::seam::Vec3;
use guac_motion::stick::Stick;
use guac_script::{Handle, Mode};
use guacamole::script_engine::Pad;

mod common;

use common::*;

const LEVEL_201: &str = "level_201";
const EVIL: &str = "026_StealthElf_Evil";
const AI: &str = "026_StealthElf_AI";

/// A run on level 201 with Eruptor joined and its intro skipped, as the
/// ambush tests start one, set down on the life island's landing.
fn island() -> Run {
    let setup = Setup {
        interface: true,
        props: true,
        sounds: true,
        ..Setup::default()
    };
    let (mut run, _) = run_on_level(Mode::Play, LEVEL_201, setup);
    run.step().unwrap();
    run.step().unwrap();
    run.join().unwrap_or_else(|h| panic!("{h}"));
    for k in 0..3000u32 {
        play(
            &mut run,
            Stick::new(0.0, 0.0),
            [false, false, false, k > 120 && k % 20 == 10],
        );
        if k > 300 && cutscene_name(&run).is_none() && !guacamole::anchors::scene_holds(&run.vm) {
            break;
        }
    }
    for _ in 0..60 {
        play(&mut run, Stick::new(0.0, 0.0), [false; 4]);
    }
    run
}

/// One step with `stick` and `actions` held, in play mode; a hero under 60
/// health is topped up, as the Mac's runs do.
fn play(run: &mut Run, stick: Stick, actions: [bool; 4]) {
    run.bench.pads[0] = Pad {
        stick,
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

/// The stick toward `to` across the ground, against the view the step
/// before drew.
fn toward(run: &Run, to: [f32; 2]) -> Stick {
    let at = run.hero().pivot;
    let heading = (to[0] - at.x).atan2(to[1] - at.z).to_degrees();
    let d = (heading - run.bench.view).to_radians();
    Stick::new(-d.sin(), d.cos())
}

/// The live clones named `name`.
fn alive(run: &Run, name: &str) -> Vec<Handle> {
    run.world
        .clones()
        .into_iter()
        .filter(|h| run.world.name(*h) == name)
        .filter(|h| run.world.actor(*h).is_some_and(|a| !a.removed))
        .collect()
}

/// What the summon came to, with a figure swapped in some steps after its
/// beams were made (or none).
#[derive(Debug, Default)]
struct Summoned {
    /// The step the summon's beams were made.
    beams: Option<u32>,
    /// The step his swap began, if one did.
    swapped: Option<u32>,
    /// The step his minion was activated, if it was.
    joined: Option<u32>,
    /// The step each evil elf was first up, in order.
    landed: Vec<u32>,
    /// How many AIs were up when the run ended, and the most at one time.
    ais: usize,
    ais_most: usize,
    /// The hero's health once his join was done.
    health: Option<f32>,
}

/// Level 201's life island from its landing, the hero walked onto the
/// island as the ambush tests do until the three evil elves are up each with
/// its AI, and `swap_after` steps after the summon's beams are made his
/// figure lifted and put back in through the TV's join (the beam).
fn summon(swap_after: Option<u32>) -> Summoned {
    let mut run = island();
    let (_, pan) = run.placed("Pan_Activate");
    run.put(pan + Vec3::Y, 0.0);
    let first = run.placed("Activator_Template(1)").0;
    let start = run.steps;
    while !bridge_grown(&run, first) {
        assert!(run.steps < start + 1500, "the first bridge never grew");
        play(&mut run, Stick::new(0.0, 0.0), [false; 4]);
    }
    for _ in 0..120 {
        play(&mut run, Stick::new(0.0, 0.0), [false; 4]);
    }
    run.put(Vec3::new(70.5, 30.0, -75.0), 0.0);
    for _ in 0..30 {
        play(&mut run, Stick::new(0.0, 0.0), [false; 4]);
    }
    let mut out = Summoned::default();
    let mut seen: Vec<Handle> = Vec::new();
    for k in 0..1500u32 {
        let beams = guacamole::scripts::entry_beam_count(&mut run.vm).unwrap_or(0);
        if out.beams.is_none() && beams > 0 {
            out.beams = Some(run.steps);
        }
        if let (Some(after), Some(made), None) = (swap_after, out.beams, out.swapped)
            && run.steps >= made + after
        {
            out.swapped = Some(run.steps);
            run.deactivate().unwrap_or_else(|h| panic!("{h}"));
            assert!(run.enter().unwrap_or_else(|h| panic!("{h}")), "the beam");
        }
        let stick = match k < 300 {
            true => toward(&run, [70.12, -60.0]),
            false => Stick::new(0.0, 0.0),
        };
        play(&mut run, stick, [false; 4]);
        if out.joined.is_none() && out.swapped.is_some() && run.minion.is_some() {
            out.joined = Some(run.steps);
            out.health = run.health();
        }
        for h in alive(&run, EVIL) {
            if !seen.contains(&h) {
                seen.push(h);
                out.landed.push(run.steps);
            }
        }
        out.ais = alive(&run, AI).len();
        out.ais_most = out.ais_most.max(out.ais);
        if out.landed.len() == 3 && out.ais_most == 3 && run.steps > out.landed[2] + 120 {
            break;
        }
    }
    out
}

/// Whether the vine bridge activator `act` has laid its bridge.
fn bridge_grown(run: &Run, act: Handle) -> bool {
    run.vm
        .placement_of(act)
        .and_then(|id| run.vm.behaviour(id))
        .is_some_and(|b| b.starts_with('5'))
}

/// openspec AS.218: a hero's beam waiting while the life island's summon
/// lands its three beams. Swept over the steps after the summon's beams are
/// made, every evil elf lands with its AI and the hero's join completes,
/// whenever his beam lands among theirs.
#[test]
#[ignore = "reads the Giants dump"]
fn a_swap_as_the_summon_lands_leaves_each_elf_its_ai_and_the_join_whole() {
    let base = summon(None);
    println!("no swap: {base:?}");
    assert_eq!(base.landed.len(), 3, "the three landed: {base:?}");
    assert_eq!(base.ais_most, 3, "each with its AI: {base:?}");
    let beams = base.beams.expect("the summon's beams");
    let (first, last) = (base.landed[0] - beams, base.landed[2] - beams);
    println!("the elves land {first}..{last} steps after the beams are made");
    let mut bad = Vec::new();
    for after in (0..=last + 8).step_by(4) {
        let s = summon(Some(after));
        println!("swap {after} steps after the beams: {s:?}");
        let whole = s.joined.is_some() && s.landed.len() == 3 && s.ais_most == 3;
        if !whole {
            bad.push((after, s));
        }
    }
    assert!(bad.is_empty(), "{bad:#?}");
}
