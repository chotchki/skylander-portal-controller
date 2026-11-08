//! Level 201 from its start to its end (openspec AU.53, with AS.160's and
//! AS.136's halves for it), headless with guacamole's host on the TV's
//! listing for the level: Eruptor joined at its start, its intro skipped as
//! a player skips it, then walked on the stick stretch by stretch along its
//! way, fighting what its triggers wake, to the arena's outro and the level
//! change into the hub. Ignored by default; they read `GUAC_GIANTS_DUMP`
//! (process env, else `.env.dev`):
//!
//! `cargo test -p guacamole --profile disc --test giants_level_201 -- --ignored --nocapture`
//!
//! The walk runs once and every test reads what it recorded. No recording of
//! 201 exists: every step is the level's own scripts on the TV's listing. A
//! hero under 60 health is topped up to 200 where his scripts hold it, as
//! the Mac's run does through `/api/_test/tv/health` (AU.35 keeps the
//! starting difficulty at 1), and each top-up is counted.

use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::sync::OnceLock;

use guac_motion::stick::Stick;
use guac_script::{Handle, Mode};
use guacamole::script_engine::Pad;

mod common;

use common::*;

const LEVEL_201: &str = "level_201";

/// Level 201's way, stretch by stretch, as the Mac's driver walks it:
/// `w x z` walks to within 1 m (his first attack while held up), `h x z n`
/// holds the stick toward a point `n` steps, `push x z n` likewise into a
/// block, `s n` stands, `t n` taps the fourth action every 10 steps (a
/// door's prompt) until a scene starts, `m n` mashes it, `a n` attacks every 7 steps, `f r n`
/// fights the awake foes within `r` m on his storey for at most `n` steps,
/// `cs` waits out a scene. A fight is followed by a walk to where this walk's
/// fight ended, so the Mac's driver, whose chase wanders off it, goes on
/// from the same place.
const WAY: [(&str, &str); 18] = [
    (
        "the landing to Setup_001",
        "w 157 -168; w 156 -171; w 144 -170; f 40 3000; w 135.1 -166.5; w 130 -168; f 40 2000; w 130.3 -167.8; w 124 -150; f 40 2000; w 108.6 -162.0; w 109 -152; w 109.2 -148.2; w 107.8 -145.8; w 103.8 -145.8; w 101.8 -147.8; w 101.8 -151.8; w 101.8 -155.8; w 101.8 -159.8; w 103.2 -162.2; w 104.2 -165.2; w 104.2 -169.2; w 104.2 -173.2; w 104.2 -174.8; f 45 5000; w 98.3 -174.7",
    ),
    (
        "round the plateau to its key",
        "w 99.8 -175.2; w 95.8 -175.2; w 93.8 -177.2; w 93.8 -181.2; w 94.2 -184.8; w 96.2 -186.2; w 96.8 -186.2; w 100.2 -186.8; w 103.8 -187.2; w 107.8 -187.2; w 111.8 -187.2; w 115.8 -187.2; w 116.8 -188.2; s 120; w 119.8 -186.8; w 120.8 -183.8; w 122.2 -181.2; w 122.8 -179.8; w 123.8 -178.8; w 125.2 -176.2; w 125.2 -172.2; w 125.2 -168.2; w 125.2 -166.8; s 200; w 124 -150; f 45 4000; w 126.4 -138.5",
    ),
    (
        "Setup_002's door and its fights",
        "w 121.2 -138.8; w 117.2 -138.8; w 113.2 -138.8; w 109.2 -138.8; w 105.8 -139.2; w 105.2 -142.8; w 101.8 -143.2; w 98.8 -144.2; w 98.8 -148.2; w 98.8 -152.2; w 98.8 -156.2; w 98.8 -160.2; w 98.8 -164.2; w 98.8 -168.2; w 98.8 -172.2; w 96.8 -174.2; w 92.8 -174.2; w 88.8 -174.2; w 88.2 -170.8; w 88.2 -166.8; w 88.2 -162.8; w 88.2 -158.8; w 88.2 -155.8; s 100; f 40 3000; w 88.1 -141.2; w 88.2 -151.2; w 88.2 -147.2; w 88.2 -143.2; w 91 -141; t 40; s 100; f 40 5000; w 89.6 -118.8; s 200",
    ),
    (
        "down to the push blocks",
        "w 90 -132; w 85.8 -131.8; w 81.8 -131.8; w 77.8 -131.8; w 76.2 -134.2; w 76.2 -138.2; w 76.2 -142.2; w 76.2 -146.2; w 76.2 -150.2; w 76.2 -150.8; s 60; f 40 3000; w 76.1 -150.9; w 76.2 -155.2; w 76.2 -159.2; w 76.2 -163.2; w 76.8 -164.8; s 60; f 40 3000; w 69.3 -153.7; w 76.8 -169.2; w 74.2 -170.8; w 72.8 -173.2; w 68.8 -173.2; w 64.8 -173.2; w 60.8 -173.2; w 59.8 -170.2; w 59.8 -166.2; w 59.8 -162.2; w 59.8 -158.2; w 59.8 -154.2; w 59.8 -150.2; w 59.8 -148.8; s 60; f 40 2000; w 53.9 -147.9",
    ),
    (
        "the push blocks, the practice pit and its waves",
        "w 60 -151; push 60 -130 300; w 60 -147.2; w 58.3 -147.2; push 40 -147.2 300; w 55.3 -146.8; push 55.2 -120 300; w 55.3 -142; w 52 -142.8; w 45 -142.5; w 40.8 -142.2; w 36.8 -142.2; w 35.2 -144.8; w 34.2 -147.8; w 33.8 -151.2; w 30.2 -151.8; w 29.8 -149.2; w 29.8 -148.2; w 27.8 -146.2; w 25.2 -144.8; w 21.8 -144.2; w 18.8 -143.2; w 18.8 -139.2; w 17.2 -139.2; w 14.8 -139.2; w 10.8 -139.2; w 6.8 -139.2; w 2.8 -139.2; w 2.8 -136.8; s 60; f 40 3000; w 7.5 -137.7; cs; s 120; f 40 4000; w 6.2 -135.5; cs; s 120; f 40 4000; w 6.2 -135.5; cs; s 120; f 40 4000; w 6.2 -135.5; cs; s 200",
    ),
    (
        "past the first gate to the house on the mound",
        "w 6.7 -135.6; w 2.2 -135.8; w -1.8 -135.8; w -4.8 -136.8; w -4.8 -140.8; w -4.8 -144.8; w -4.8 -148.8; w -6.2 -151.2; w -7.8 -153.8; w -10.8 -154.8; w -13.2 -156.2; w -16.8 -156.8; w -20.8 -156.8; w -24.8 -156.8; w -26.2 -154.2; w -26.2 -150.2; w -26.2 -146.2; w -26.2 -142.2; w -26.2 -138.2; w -22.2 -138.2; w -19.2 -139.2; w -19.2 -143.2; w -17.8 -145.8; w -16.2 -145.8; s 60; w -19.2 -144.2; w -19.2 -140.2; w -21.2 -138.2; w -25.2 -138.2; w -26.2 -141.2; w -26.2 -145.2; w -26.2 -149.2; w -26.2 -153.2; w -26.2 -157.2; w -26.2 -161.2; w -26.2 -165.2; w -26.2 -169.2; w -26.2 -173.2; w -26.2 -177.2; w -24.8 -179.8; w -22.2 -181.2; w -22.2 -185.2; w -20.2 -187.2; w -19.8 -190.8; w -19.8 -194.8; w -20.2 -198.2; w -22.2 -200.2; w -22.2 -203.2; s 60; w -32.5 -210.1; h -40.7 -211 120; s 300",
    ),
    (
        "the house's room and its key",
        "s 60; f 30 4000; w 376.6 206.8; w 385 222; w 374 225; t 40; s 200; w 374 219; w 360 220; w 357 220; s 60; w 372 219; w 392 212; w 405 211.2; h 412 211.2 120; s 300",
    ),
    (
        "Setup_003B and the first door",
        "w -22.2 -203.2; s 60; f 40 3000; w -22.2 -203.2; w -22.2 -200.2; w -20.2 -198.2; w -19.8 -194.8; w -19.8 -190.8; w -20.2 -187.2; w -20.1 -184.4; s 60; f 40 3000; w -16.5 -156.2; w -22.2 -181.8; w -24.2 -179.8; w -25.8 -177.2; w -25.8 -173.2; w -25.8 -169.2; w -25.8 -165.8; s 60; f 40 4000; w -20.7 -155.2; w -30.2 -165.8; w -34.2 -165.8; w -38.2 -165.8; w -42.2 -165.8; w -46.2 -165.8; w -47.8 -163.2; w -47.8 -159.2; t 40; s 300",
    ),
    (
        "Setup_004, the eyeball door and the bridge",
        "w -51.8 -159.8; w -55.8 -159.8; w -59.8 -159.8; w -62.2 -161.2; w -62.2 -162.8; s 60; f 40 3000; w -62.0 -162.8; w -66.8 -162.8; w -70.8 -162.8; w -71.2 -159.2; w -72.2 -156.2; w -72.2 -152.2; w -72.2 -150.8; h -74.4 -150.6 15; a 200; s 60; w -72.2 -155.2; w -71.2 -158.2; w -71.8 -161.8; w -75.8 -161.8; w -79.8 -161.8; w -83.8 -161.8; w -85.8 -159.8; w -85.8 -155.8; w -85.8 -151.8; w -85.8 -147.8; w -85.8 -143.8; w -85.8 -139.8; w -85.2 -136.2; w -86.8 -133.8; w -88.2 -131.2; w -88.8 -127.8; w -88.8 -123.8; w -88.8 -122.8; w -91.8 -122.8; w -93.2 -120.2; w -94.8 -117.8; w -94.8 -113.8; s 60; f 50 5000; w -83.3 -107.7; w -99.2 -113.8; w -103.2 -113.8; w -105.8 -112.2; w -105.8 -108.2; w -105.8 -104.2; w -105.8 -100.2; w -105.8 -96.2; w -105.8 -92.2; w -105.8 -89.2; s 60; cs; s 200",
    ),
    (
        "the channel: the first block",
        "s 600; w -113.2 -75.2; w -113.2 -71.2; w -113.2 -67.2; w -113.2 -63.2; w -113.2 -59.2; w -113.2 -55.2; w -113.2 -51.2; w -113.2 -50.8; push -113.2 -30 300; w -113.2 -45; w -113.2 -38; push -113.2 -10 12; s 200",
    ),
    (
        "the channel: the side blocks",
        "w -113.2 -33; w -117.8 -31; w -117.8 -22.7; w -112 -22.7; push -80 -22.7 40; s 200; w -117.8 -22.7; w -117.8 -31; w -113.2 -33; push -113.2 -10 12; s 200; w -117.8 -31; w -117.8 -22.7; push -80 -22.7 40; s 200; w -117.8 -22.7; push -80 -22.7 40; s 200",
    ),
    (
        "the channel: the last side block",
        "w -110 -22.7; push -80 -22.7 40; s 200",
    ),
    (
        "the second house's room and its key, the fourth door",
        "w -108 -22.7; w -100 -22.7; w -94 -24; w -86 -27; w -84.7 -28; s 60; w -94 -24; w -100.9 -23; w -107.6 -23.4; w -113.2 -26; w -113.2 -33; w -113.2 -38; w -113.2 -46; w -113.2 -50; w -113.2 -54.2; w -113.2 -58.2; w -113.2 -62.2; w -113.2 -66.2; w -113.2 -70.2; w -113.2 -74.2; w -110.2 -75.2; w -106.2 -75.2; w -102.2 -75.2; w -98.2 -75.2; w -96.8 -75.2; h -89.7 -77 120; s 300; w 392 212; w 385 222; w 375 221; s 30; t 40; s 200; w 374 219; w 360 220; w 357 220.4; s 60; w 372 219; w 392 212; w 405 211.2; h 412 211.2 120; s 300; w -97.2 -73.8; w -97.2 -69.8; w -97.2 -65.8; w -97.2 -61.8; w -95.8 -59.8; t 40; s 300",
    ),
    (
        "the channel: the blocks behind the fourth door",
        "w -93.2 -58.2; w -90.8 -56.8; w -90.6 -51.5; push -90.6 -30 60; s 200; w -90.6 -51.5; push -90.6 -30 60; s 200",
    ),
    (
        "the channel: the stack and the special block",
        "w -90.6 -46; w -90.6 -41; push -90.6 0 120; s 200; w -90.6 -30; push -90.6 0 120; s 200; w -94 -28; w -97 -28; w -100 -28; w -99.7 -27; push -99.7 0 200; s 200; w -103.6 -13; w -103.6 -9; push -80 -9 120; s 200",
    ),
    (
        "the second door and Setup_005's fight",
        "w -101 -12; w -100.5 -20; w -100.5 -26; w -101.5 -30.8; w -102.2 -34.8; w -102.2 -38.8; w -102.2 -42.8; w -102.8 -46.2; w -103.8 -49.2; w -104.8 -51.8; w -104.8 -56.2; w -103.2 -58.8; w -99.2 -58.8; w -95.2 -58.8; w -92.2 -57.8; w -89.8 -56.2; w -89.8 -52.2; w -90.8 -50.8; w -90.6 -46; w -90.6 -40; w -90.6 -34; w -90.6 -28; w -90.6 -23.5; push -90.6 0 60; s 200; w -91.5 -21; s 60; f 30 2000; w -91.5 -21.2; w -92 -14; w -92 -9; w -90.6 -5.7; s 60; f 40 5000; w -83.4 36.9; f 60 5000; w -83.4 36.9; cs; s 200",
    ),
    (
        "the fifth gate, the eyeballs and the arena's intro",
        "w -94.2 -4.8; w -96.2 -2.8; w -98.8 -1.2; w -102.2 -0.8; w -105.8 -0.2; w -105.8 3.8; w -105.8 7.8; w -106.2 11.2; w -107.8 13.8; w -109.2 16.2; w -110.8 18.2; w -110.7 24; w -110.7 26; w -115.2 26.2; w -119.2 26.2; w -123.2 26.2; w -127.2 26.2; w -131.2 26.2; w -135.2 26.2; w -139.2 26.2; w -143.2 26.2; w -147.2 26.2; w -151.2 26.2; w -155.2 26.2; w -159.2 26.2; w -163.2 26.2; w -167.2 26.2; w -169.8 24.8; w -172.2 23.2; w -174.2 21.2; w -175.8 22.2; h -180.3 20.7 20; a 150; s 30; w -173.2 22.2; w -171.2 24.2; w -168.2 25.2; w -165.2 26.2; w -162.8 27.8; w -162.8 31.8; w -160.2 32.2; h -158.6 34.3 20; a 150; w -162.8 30.2; w -163.2 26.8; w -166.8 26.2; w -168.2 28.8; w -172.2 28.8; w -175.8 29.2; w -175.8 33.2; w -175.8 37.2; w -175.8 41.2; s 120; cs; s 200; f 60 6000; w -182.4 88.2; cs; s 200; f 60 6000; w -187.6 66.4; cs; s 200; f 60 6000; w -202.9 69.1; cs; s 300",
    ),
    (
        "the arena's battle to its outro",
        "s 200; f 45 3600; cs; s 200; f 45 3600; cs; s 200; f 45 3600; cs; s 200; f 45 3600; cs; s 200; cs; s 600",
    ),
];

/// The placements whose behaviour, wake and range the walk records: the
/// setups' controllers, the life wall's controller and its two triggers,
/// what the first trigger takes off at the start, the arena's script and
/// the Hermit's gate.
const WATCHED: [&str; 17] = [
    "Setup_001_Controller",
    "Setup_002_Controller",
    "Setup_002_Controller_Helper",
    "Setup_003_Controller",
    "Setup_003_Controller_Helper",
    "Setup_003B_Controller",
    "Setup_004_Controller",
    "Setup_005_Controller",
    "ElementalWallController_life",
    "Special_Activate_Life_wall",
    "Special_Remove_Life_Wall",
    "Flynn",
    "Cali NPC",
    "Landing_Pod",
    "201_Intro_Arena_Fight_Controller",
    "castleArena_script",
    "hermitGate",
];

/// A watched placement's state at a step it changed.
#[derive(Debug, Clone, PartialEq)]
struct State {
    step: u32,
    name: &'static str,
    /// Its behaviour, `removed` once taken off, `-` with no script.
    behaviour: String,
    awake: bool,
    range: f32,
}

/// An enemy intro's card: the steps it stood, its string's object by name
/// and a hash of its line read at run time (no line is in the tree), and
/// the scene it stood in.
#[derive(Debug, Clone)]
struct Card {
    from: u32,
    to: u32,
    string: Option<String>,
    line: u64,
    scene: Option<String>,
}

impl Card {
    fn seconds(&self) -> f32 {
        (self.to - self.from) as f32 / 60.0
    }
}

/// What the walk did.
#[derive(Debug, Default)]
struct Walked {
    /// Each stretch's name, the step it ended at and whether he got there.
    stretches: Vec<(&'static str, u32, bool)>,
    stops: Vec<String>,
    /// Each top-up's step and the health it found.
    topups: Vec<(u32, f32)>,
    states: Vec<State>,
    /// Each cutscene's name and the steps it played.
    scenes: Vec<(String, u32, u32)>,
    cards: Vec<Card>,
    /// Each fight's way across the ground, every quarter second from where
    /// it began: what the Mac's driver follows after its own fight there.
    fights: Vec<Vec<[f32; 2]>>,
    level_change: Option<String>,
    steps: u32,
    seconds: f64,
    /// The pits' jump pads after each stretch: whether each is off.
    pads: Vec<(&'static str, [bool; 2])>,
}

/// The jump pads the walk records, the one in the pit below the landing's
/// yard and the one in the pit off the grass the bridge drops him on.
const PIT_PADS: [&str; 2] = ["PvP_JumpPad_NORMAL(2)", "PvP_JumpPad_NORMAL(4)"];

impl Walked {
    /// The states `name` went through, in order.
    fn of(&self, name: &str) -> Vec<&State> {
        self.states.iter().filter(|s| s.name == name).collect()
    }

    /// The step `name` was taken off, if it was.
    fn removed(&self, name: &str) -> Option<u32> {
        self.of(name)
            .into_iter()
            .find(|s| s.behaviour == "removed")
            .map(|s| s.step)
    }

    /// The first step the scene named `name` played.
    fn scene(&self, name: &str) -> Option<(u32, u32)> {
        self.scenes
            .iter()
            .find(|(n, _, _)| n == name)
            .map(|&(_, a, b)| (a, b))
    }
}

fn is_foe(name: &str) -> bool {
    (name.starts_with("Enemy_") || name.starts_with("ChompySpawner"))
        && !name.contains("Shoulder")
        && !name.contains("Helmet")
        && !name.contains("Spawner_Pod")
}

fn line_hash(line: &str) -> u64 {
    let mut h = std::collections::hash_map::DefaultHasher::new();
    line.hash(&mut h);
    h.finish()
}

/// The walk's recorder and its stick.
struct Walker {
    run: Run,
    walked: Walked,
    watched: Vec<(&'static str, Handle)>,
    last: HashMap<Handle, State>,
    card: Option<Card>,
    scene: Option<(String, u32)>,
}

impl Walker {
    fn state(&self, name: &'static str, h: Handle) -> State {
        let run = &self.run;
        let behaviour = match run.vm.placement_of(h) {
            None => "-".to_owned(),
            Some(id) if run.vm.removed(id) => "removed".to_owned(),
            Some(id) => run.vm.behaviour(id).unwrap_or("?").to_ascii_lowercase(),
        };
        let actor = run.world.actor(h);
        State {
            step: run.steps,
            name,
            behaviour: if actor.is_some_and(|a| a.removed) {
                "removed".to_owned()
            } else {
                behaviour
            },
            awake: actor.is_some_and(|a| a.active),
            range: actor.map_or(0.0, |a| a.range),
        }
    }

    /// One step with `stick` and `actions` held, recorded.
    fn step(&mut self, stick: Stick, actions: [bool; 4]) {
        self.run.bench.pads[0] = Pad {
            stick,
            actions,
            connected: true,
        };
        let stops = self.run.step_play().unwrap_or_else(|h| panic!("{h}"));
        self.walked.stops.extend(
            stops
                .iter()
                .map(|h| format!("step {}: {h}", self.run.steps)),
        );
        for k in 0..self.watched.len() {
            let (name, h) = self.watched[k];
            let now = self.state(name, h);
            let changed = self.last.get(&h).is_none_or(|l| {
                (l.behaviour.as_str(), l.awake, l.range)
                    != (now.behaviour.as_str(), now.awake, now.range)
            });
            if changed {
                self.walked.states.push(now.clone());
                self.last.insert(h, now);
            }
        }
        let steps = self.run.steps;
        let scene = cutscene_name(&self.run);
        let up = guacamole::announcements::enemy_card(&self.run.vm, &self.run.world);
        let string = up.as_ref().and_then(|c| match c.string {
            guac_script::Value::Object(o) => self.run.vm.program().name_of(o).map(str::to_owned),
            _ => None,
        });
        let line = up.and_then(|c| c.text).map(|t| line_hash(&t));
        match (&mut self.card, line) {
            (Some(c), Some(l)) if c.line == l => c.to = steps,
            (card, line) => {
                if let Some(c) = card.take() {
                    self.walked.cards.push(c);
                }
                *card = line.map(|line| Card {
                    from: steps,
                    to: steps,
                    string,
                    line,
                    scene: scene.clone(),
                });
            }
        }
        if self.scene.as_ref().map(|s| &s.0) != scene.as_ref() {
            if let Some((n, from)) = self.scene.take() {
                self.walked.scenes.push((n, from, steps));
            }
            self.scene = scene.map(|n| (n, steps));
        }
        if let (Some(hp), Some(m)) = (self.run.health(), self.run.minion)
            && hp < 60.0
        {
            guacamole::scripts::set_health(&mut self.run.vm, m, 200);
            self.walked.topups.push((steps, hp));
        }
    }

    fn rest(&mut self, n: u32) {
        for _ in 0..n {
            self.step(Stick::new(0.0, 0.0), [false; 4]);
        }
    }

    /// The stick toward `to` across the ground at `push`, against the view
    /// the step before drew.
    fn toward(&self, to: [f32; 2], push: f32) -> Stick {
        let at = self.run.hero().pivot;
        let heading = (to[0] - at.x).atan2(to[1] - at.z).to_degrees();
        let d = (heading - self.run.bench.view).to_radians();
        Stick::new(-d.sin() * push, d.cos() * push)
    }

    fn in_scene(&self) -> bool {
        cutscene_name(&self.run).is_some() || guacamole::anchors::scene_holds(&self.run.vm)
    }

    /// Walked to within 1 m of `to`, his first attack every 7 steps while
    /// he's been held up half a second; false if 2400 steps don't get him
    /// there.
    fn walk(&mut self, to: [f32; 2]) -> bool {
        let (mut best, mut since) = (f32::MAX, 0u32);
        for k in 0..2400u32 {
            let at = self.run.hero().pivot;
            let d = (to[0] - at.x).hypot(to[1] - at.z);
            if d <= 1.0 {
                return true;
            }
            if d < best - 0.2 {
                (best, since) = (d, 0);
            } else {
                since += 1;
            }
            let stick = self.toward(to, 1.0);
            self.step(stick, [since > 30 && k % 7 == 0, false, false, false]);
        }
        false
    }

    /// The awake foes within `reach` m on his storey fought, nearest first,
    /// for at most `n` steps, until none has stood there for 4 s.
    fn fight(&mut self, reach: f32, n: u32) {
        let at = |w: &Self| {
            let p = w.run.hero().pivot;
            [p.x, p.z]
        };
        let mut trail = vec![at(self)];
        self.fight_for(reach, n, &mut trail);
        trail.push(at(self));
        self.walked.fights.push(trail);
    }

    fn fight_for(&mut self, reach: f32, n: u32, trail: &mut Vec<[f32; 2]>) {
        let (mut pressed, mut idle) = (0u32, 0u32);
        for k in 0..n {
            let me = self.run.hero().pivot;
            if k % 15 == 0 {
                trail.push([me.x, me.z]);
            }
            let run = &self.run;
            let near = run
                .world
                .creatures()
                .into_iter()
                .chain(run.world.clones())
                .filter(|h| is_foe(&run.world.name(*h)))
                .filter(|h| {
                    run.vm
                        .placement_of(*h)
                        .is_some_and(|i| !run.vm.removed(i) && !run.vm.asleep(i))
                })
                .filter_map(|h| {
                    run.world
                        .actor(h)
                        .filter(|a| !a.removed)
                        .map(|a| a.position)
                })
                .filter(|p| (p.y - me.y).abs() < 6.0 && (p.x - me.x).hypot(p.z - me.z) < reach)
                .min_by(|a, b| a.distance(me).total_cmp(&b.distance(me)));
            let Some(near) = near else {
                idle += 1;
                if idle > 240 {
                    return;
                }
                self.rest(1);
                continue;
            };
            idle = 0;
            let far = (near.x - me.x).hypot(near.z - me.z);
            pressed = if far <= 2.5 { pressed + 1 } else { 0 };
            if pressed > 240 {
                // Pressed into him 4 s: stepped back out.
                let away = [me.x + (me.x - near.x) * 4.0, me.z + (me.z - near.z) * 4.0];
                for _ in 0..30 {
                    let stick = self.toward(away, 1.0);
                    self.step(stick, [false; 4]);
                }
                pressed = 0;
                continue;
            }
            let stick = self.toward([near.x, near.z], if far > 2.5 { 1.0 } else { 0.35 });
            self.step(stick, [far <= 5.0 && k % 20 == 0, false, false, false]);
        }
    }

    /// One stretch's commands, as [`WAY`] spells them: false once a walk
    /// doesn't get there.
    fn stretch(&mut self, plan: &str) -> bool {
        for cmd in plan.split(';').map(str::trim).filter(|c| !c.is_empty()) {
            let w: Vec<&str> = cmd.split_whitespace().collect();
            let num = |i: usize| -> f32 { w[i].parse().expect(cmd) };
            match w[0] {
                "w" => {
                    if !self.walk([num(1), num(2)]) {
                        println!("!! {cmd}: stopped at {}", self.run.hero().pivot);
                        return false;
                    }
                }
                "h" | "push" => {
                    for _ in 0..num(3) as u32 {
                        let stick = self.toward([num(1), num(2)], 1.0);
                        self.step(stick, [false; 4]);
                    }
                }
                "s" => self.rest(num(1) as u32),
                "t" => {
                    // A hand stops pressing once a scene starts: a door's
                    // key wakes Setup_002's life mage, whose intro △ skips.
                    for k in 0..num(1) as u32 * 10 {
                        if self.in_scene() {
                            break;
                        }
                        self.step(Stick::new(0.0, 0.0), [false, false, false, k % 10 == 0]);
                    }
                }
                "m" => {
                    for k in 0..num(1) as u32 {
                        self.step(Stick::new(0.0, 0.0), [false, false, false, k % 6 < 3]);
                    }
                }
                "a" => {
                    for k in 0..num(1) as u32 {
                        self.step(Stick::new(0.0, 0.0), [k % 7 == 0, false, false, false]);
                    }
                }
                "f" => self.fight(num(1), num(2) as u32),
                "cs" => {
                    for _ in 0..4000 {
                        if !self.in_scene() {
                            break;
                        }
                        self.rest(1);
                    }
                }
                other => panic!("{other}?"),
            }
        }
        true
    }
}

/// The walk, once for every test.
fn walked() -> &'static Walked {
    static WALKED: OnceLock<Walked> = OnceLock::new();
    WALKED.get_or_init(|| {
        let started = std::time::Instant::now();
        println!("load at start: {}", load());
        let setup = Setup {
            interface: true,
            props: true,
            sounds: true,
            ..Setup::default()
        };
        let (run, _) = run_on_level(Mode::Play, LEVEL_201, setup);
        let watched = WATCHED.iter().map(|n| (*n, run.placed(n).0)).collect();
        let mut w = Walker {
            run,
            walked: Walked::default(),
            watched,
            last: HashMap::new(),
            card: None,
            scene: None,
        };
        w.rest(2);
        w.run.join().unwrap_or_else(|h| panic!("{h}"));
        // Its intro skipped as a player skips it: △ every 20 steps past
        // its second, until it hands him over.
        for k in 0..3000u32 {
            w.step(
                Stick::new(0.0, 0.0),
                [false, false, false, k > 120 && k % 20 == 10],
            );
            if k > 300 && !w.in_scene() {
                break;
            }
        }
        w.rest(60);
        println!("handed over at step {} {}", w.run.steps, w.run.hero().pivot);
        for (name, plan) in WAY {
            let ok = w.stretch(plan);
            println!(
                "{name}: {} at step {} {} health {:?}, {} top-ups so far",
                if ok { "done" } else { "STOPPED" },
                w.run.steps,
                w.run.hero().pivot,
                w.run.health(),
                w.walked.topups.len()
            );
            w.walked.stretches.push((name, w.run.steps, ok));
            let off = PIT_PADS.map(|p| {
                let h = w.run.placed(p).0;
                w.run.world.actor(h).is_none_or(|a| a.removed)
            });
            w.walked.pads.push((name, off));
            if !ok {
                break;
            }
        }
        w.rest(1);
        let mut walked = w.walked;
        walked.level_change = w.run.world.level_change().map(|c| c.name.clone());
        walked.steps = w.run.steps;
        walked.seconds = started.elapsed().as_secs_f64();
        println!(
            "{} steps in {:.1} s, {} top-ups, {} stops, level change {:?}; load at end: {}",
            walked.steps,
            walked.seconds,
            walked.topups.len(),
            walked.stops.len(),
            walked.level_change,
            load()
        );
        for (n, a, b) in &walked.scenes {
            println!("  scene {n} {a}-{b}");
        }
        for (k, f) in walked.fights.iter().enumerate() {
            let way: Vec<String> = f
                .iter()
                .map(|p| format!("{:.1} {:.1}", p[0], p[1]))
                .collect();
            println!("  fight {k}: {}", way.join("; "));
        }
        for c in &walked.cards {
            println!(
                "  card {:?} {}-{} ({:.2} s) in {:?}",
                c.string,
                c.from,
                c.to,
                c.seconds(),
                c.scene
            );
        }
        for s in &walked.states {
            println!(
                "  {} {} {} awake {} range {}",
                s.step, s.name, s.behaviour, s.awake, s.range
            );
        }
        walked
    })
}

/// AU.53: every stretch of the way walked on the stick, nothing stopped in
/// play, and the arena's outro asking for the hub's first stage; the
/// top-ups counted.
#[test]
#[ignore = "reads the Giants dump"]
fn level_201_is_walked_from_its_landing_to_the_hub() {
    let w = walked();
    assert_eq!(w.stretches.len(), WAY.len(), "{:#?}", w.stretches);
    assert!(w.stretches.iter().all(|s| s.2), "{:#?}", w.stretches);
    assert!(w.stops.is_empty(), "{:#?}", w.stops);
    assert_eq!(w.level_change.as_deref(), Some("level_hub_2_stage_1"));
    println!("{} top-ups: {:?}", w.topups.len(), w.topups);
}

/// AU.53: each setup's controller, woken by its own trigger, runs its
/// fights and takes itself off, in the way's order.
#[test]
#[ignore = "reads the Giants dump"]
fn its_setups_end_by_their_own_fights_in_order() {
    let w = walked();
    let setups = [
        "Setup_001_Controller",
        "Setup_002_Controller",
        "Setup_003_Controller",
        "Setup_003B_Controller",
        "Setup_004_Controller",
        "Setup_005_Controller",
    ];
    let ends: Vec<(&str, Option<u32>)> = setups.iter().map(|n| (*n, w.removed(n))).collect();
    assert!(ends.iter().all(|e| e.1.is_some()), "{ends:?}");
    assert!(ends.windows(2).all(|p| p[0].1 < p[1].1), "{ends:?}");
    for n in setups {
        let woke = w.of(n).iter().filter(|s| s.awake).count();
        assert!(woke > 0, "{n} never woke: {:?}", w.of(n));
    }
}

/// AS.160's half on level 201: the life wall's first trigger, past the
/// second setup, takes the landing's people off and wakes the wall's
/// controller for the life area; the second, at the cave's mouth, puts it
/// to sleep while he's in its box and back after.
#[test]
#[ignore = "reads the Giants dump"]
fn its_life_wall_follows_its_two_triggers() {
    let w = walked();
    let flynn = w.removed("Flynn").expect("the landing's Flynn taken off");
    let ranges: Vec<(u32, f32)> = w
        .of("ElementalWallController_life")
        .iter()
        .map(|s| (s.step, s.range))
        .collect();
    println!("Flynn off at {flynn}; the wall's ranges {ranges:?}");
    assert!(
        ranges.iter().any(|&(_, r)| r == 0.0),
        "asleep in the cave's box: {ranges:?}"
    );
    let first_zero = ranges.iter().find(|r| r.1 == 0.0).unwrap().0;
    assert!(
        ranges.iter().any(|&(at, r)| at > first_zero && r == 200.0),
        "awake again after: {ranges:?}"
    );
}

/// AS.136's half on level 201: its four enemy intros each put up their
/// enemy's card in their own scene for their scripts' time, a line of its
/// own each; by name and timing only.
#[test]
#[ignore = "reads the Giants dump"]
fn its_enemy_intros_put_up_their_cards() {
    let w = walked();
    let want = [
        ("S2_Enemy_Chompy_Name", "IGC_Green_Chompy_Reveal", 3.0),
        ("Elf_Armored_Near", "IGC_Armored_Elf_Near", 2.2),
        ("S2_Enemy_LifeMage_Name", "IGC_Life_Mage", 2.5),
        ("S2_Enemy_ElfHeavy_Name", "IGC_Elf_Heavy_Reveal", 2.5),
    ];
    let got: Vec<(Option<&str>, Option<&str>, f32)> = w
        .cards
        .iter()
        .map(|c| (c.string.as_deref(), c.scene.as_deref(), c.seconds()))
        .collect();
    assert_eq!(got.len(), want.len(), "{got:?}");
    for ((string, scene, secs), (n, s, d)) in got.iter().zip(want) {
        assert_eq!((*string, *scene), (Some(n), Some(s)), "{got:?}");
        assert!((secs - d).abs() < 0.1, "{n}: {secs} s against {d}");
    }
    let mut lines: Vec<u64> = w.cards.iter().map(|c| c.line).collect();
    lines.sort_unstable();
    lines.dedup();
    assert_eq!(lines.len(), want.len(), "a line each");
}

/// AU.53: the arena's battle as its castle script runs it, the commander
/// staying to command the waves: the half battle's scene, Brock's, the heavy
/// elf's reveal and his fight, the outro, the script taking itself off and
/// the level change after.
#[test]
#[ignore = "reads the Giants dump"]
fn its_arena_runs_its_battle_to_the_outro() {
    let w = walked();
    let scenes = [
        "IGC_Arena_HalfBattle",
        "IGC_Arena_BrockBattle",
        "IGC_Elf_Heavy_Reveal",
        "IGC_Arena_Outro",
    ];
    let at: Vec<(&str, Option<(u32, u32)>)> = scenes.iter().map(|n| (*n, w.scene(n))).collect();
    assert!(at.iter().all(|a| a.1.is_some()), "{at:?}");
    assert!(at.windows(2).all(|p| p[0].1 < p[1].1), "{at:?}");
    let castle: Vec<&str> = w
        .of("castleArena_script")
        .iter()
        .map(|s| s.behaviour.as_str())
        .collect();
    for b in ["start battle", "waiting", "end", "removed"] {
        assert!(castle.contains(&b), "{b}: {castle:?}");
    }
    let outro = at[3].1.unwrap();
    assert!(
        w.removed("castleArena_script")
            .is_some_and(|s| s >= outro.0),
        "{castle:?}"
    );
}

/// A fresh run on level 201 with Eruptor joined and its intro skipped, as
/// [`walked`] starts, for a test of its own.
fn fresh() -> Walker {
    let setup = Setup {
        interface: true,
        props: true,
        sounds: true,
        ..Setup::default()
    };
    let (run, _) = run_on_level(Mode::Play, LEVEL_201, setup);
    let mut w = Walker {
        run,
        walked: Walked::default(),
        watched: Vec::new(),
        last: HashMap::new(),
        card: None,
        scene: None,
    };
    w.rest(2);
    w.run.join().unwrap_or_else(|h| panic!("{h}"));
    for k in 0..3000u32 {
        w.step(
            Stick::new(0.0, 0.0),
            [false, false, false, k > 120 && k % 20 == 10],
        );
        if k > 300 && !w.in_scene() {
            break;
        }
    }
    w.rest(60);
    w
}

/// AU.53's review: two pits a walk drops into off level 201's way, whose
/// one way out is the jump pad in each (a walk out of either is a step past
/// any the hero climbs), the pads on the TV's listing
/// (`level_201::PASSAGE`). The one below the landing's yard: set down on
/// the yard's edge, he walks off it into the pit, then onto its pad with the
/// stick held back toward the yard and lands up there again; off the
/// listing the pit kept him. The one off the grass the bridge drops him on:
/// its pad, which the level's controller takes off at its start, is back
/// by the time the bridge has dropped him there (the walk's record).
#[test]
#[ignore = "reads the Giants dump"]
fn its_pits_jump_pads_take_him_back_up() {
    let (pad, edge, height) = ("PvP_JumpPad_NORMAL(2)", [109.8, -123.8], 16.5);
    let mut w = fresh();
    let (h, at) = w.run.placed(pad);
    w.run
        .put(guac_motion::seam::Vec3::new(edge[0], height, edge[1]), 0.0);
    w.rest(30);
    // Off the edge toward the pad.
    let mut low = f32::MAX;
    for _ in 0..300 {
        let stick = w.toward([at.x, at.z], 1.0);
        w.step(stick, [false; 4]);
        low = low.min(w.run.hero().pivot.y);
    }
    let fell = w.run.hero().pivot;
    let on = w.run.world.actor(h).map(|a| (a.active, a.removed));
    // Onto the pad (he stands by it now) and on toward the yard.
    let mut top = fell.y;
    for _ in 0..600 {
        let stick = w.toward(edge, 1.0);
        w.step(stick, [false; 4]);
        top = top.max(w.run.hero().pivot.y);
    }
    w.rest(60);
    let end = w.run.hero().pivot;
    println!(
        "{pad}: from the yard at {height} down to {low:.2} ({fell}), the pad (awake, off) {on:?}, up to {top:.2}, at the end {end} ({} stops)",
        w.walked.stops.len()
    );
    assert!(w.walked.stops.is_empty(), "{:#?}", w.walked.stops);
    assert!(low < height - 5.0, "into the pit: {low}");
    assert!(end.y > height - 1.5, "back up in the yard: {end}");
    // The grass pit's pad, on once the bridge has dropped him by it.
    let walked = walked();
    let after_bridge = walked
        .pads
        .iter()
        .find(|(n, _)| n.starts_with("Setup_004"))
        .map(|p| p.1);
    println!("the pads off after each stretch: {:?}", walked.pads);
    assert_eq!(after_bridge, Some([false, false]), "{:?}", walked.pads);
}
