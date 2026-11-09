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
    /// Brock in the practice pit's last scene (AS.187), each step from its
    /// start to [`PP_AFTER`] steps past its end: where he stands, whether
    /// the scene still plays.
    brock: Vec<(u32, [f32; 3], bool)>,
}

/// The practice pit's last scene, whose camera's scene jumps Brock off the
/// yard to the landing below (AS.187), and how long past it the walk
/// follows him.
const PP_SCENE: &str = "IGC PP Macro";
const PP_AFTER: u32 = 120;
/// Brock's placement in the practice pit's scenes, and the waypoints of
/// his way through them (`PP Brock WP`).
const BROCK_NPC: &str = "Brock NPC";
const BROCK_WAY: &str = "PP Brock WP";

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
            self.scene = scene.clone().map(|n| (n, steps));
        }
        let in_pp = scene.as_deref() == Some(PP_SCENE);
        let after_pp = self
            .walked
            .scene(PP_SCENE)
            .is_some_and(|(_, end)| steps <= end + PP_AFTER);
        if in_pp || after_pp {
            let h = self.run.placed(BROCK_NPC).0;
            if let Some(a) = self.run.world.actor(h).filter(|a| !a.removed) {
                self.walked
                    .brock
                    .push((steps, a.position.to_array(), in_pp));
            }
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

/// AS.187: the practice pit's last scene jumps Brock off the yard to the
/// landing below on its own arc: he rises well over the yard (the jump's
/// 25 ft reach) and comes down on the ground by his landing, his way's
/// second waypoint, which stands 1.2 m over the ground 3.4 m under the
/// yard (where his walk carried him flat and 6.6 m short before), never
/// under the ground; walked on to its third, he jumps again for its
/// fourth, up off the ground there as the scene ends and takes him off.
#[test]
#[ignore = "reads the Giants dump"]
fn its_practice_pit_scene_jumps_brock_to_his_landing() {
    let w = walked();
    assert!(w.scene(PP_SCENE).is_some(), "{:?}", w.scenes);
    let way = waypoints(BROCK_WAY);
    let grounds: Vec<f32> = way.iter().map(|p| ground_under(*p)).collect();
    let (yard, landing) = (way[0], way[1]);
    println!("Brock's way {way:?}, the ground under it {grounds:?}");
    for b in w.brock.iter().step_by(15) {
        println!("  {} {:?} {}", b.0, b.1, b.2);
    }
    let ys: Vec<f32> = w.brock.iter().map(|b| b.1[1]).collect();
    // The first jump: off the yard, its arc within 2 s (120 steps).
    let off = ys
        .iter()
        .position(|y| *y > yard[1] + 0.5)
        .expect("off the yard");
    let arc = off..(off + 120).min(ys.len());
    let peak = arc.fold(off, |best, k| if ys[k] > ys[best] { k } else { best });
    println!(
        "top {:.2} at step {} over the yard's {:.2}",
        ys[peak], w.brock[peak].0, yard[1]
    );
    assert!(
        ys[peak] > yard[1] + 3.0,
        "rose: {} over {}",
        ys[peak],
        yard[1]
    );
    let down = w.brock[peak..]
        .iter()
        .position(|b| (b.1[1] - grounds[1]).abs() < 0.15)
        .map(|k| peak + k)
        .unwrap_or_else(|| panic!("never down by {landing:?}: {:?}", w.brock.last()));
    let at = w.brock[down].1;
    let off = (at[0] - landing[0]).hypot(at[2] - landing[2]);
    println!(
        "down at step {}: {at:?}, {off:.2} m from his landing",
        w.brock[down].0
    );
    assert!(off < 2.0, "by his landing: {at:?} against {landing:?}");
    let lowest = grounds.iter().copied().fold(f32::MAX, f32::min);
    assert!(
        ys.iter().all(|y| *y > lowest - 0.3),
        "never under the ground: {:?}",
        ys.iter().copied().fold(f32::MAX, f32::min)
    );
    let again = ys[down..].iter().copied().fold(f32::MIN, f32::max);
    assert!(again > grounds[2] + 3.0, "the second jump rose: {again}");
}

/// The ground under `at` on level 201, metres: what its collision meets
/// on a ray down from 3 m over it.
fn ground_under(at: [f32; 3]) -> f32 {
    use guac_motion::seam::Vec3;
    let usr = dump().join("PS3_GAME/USRDIR");
    let bytes = guac_extract::scripts::entry(&usr, &format!("level/{LEVEL_201}.bld")).unwrap();
    let igz = guac_formats::igz::Igz::parse(&bytes).unwrap();
    let (bodies, _) = level_bodies(&igz, LEVEL_201);
    let world = guac_motion::parry::ParryWorld::new(&bodies).unwrap();
    let at = Vec3::from_array(at);
    guac_motion::ground::ray(&world, at + Vec3::Y * 3.0, Vec3::NEG_Y, 10.0, &[])
        .expect("ground")
        .point
        .y
}

/// The waypoints of level 201's placement `name`, metres, off its file.
fn waypoints(name: &str) -> Vec<[f32; 3]> {
    let usr = dump().join("PS3_GAME/USRDIR");
    let bytes = guac_extract::scripts::entry(&usr, &format!("level/{LEVEL_201}.bld")).unwrap();
    let igz = guac_formats::igz::Igz::parse(&bytes).unwrap();
    let p = guac_formats::level::Placements::read(&igz).unwrap();
    let actor = p
        .actors
        .iter()
        .find(|a| a.name() == Some(name))
        .unwrap_or_else(|| panic!("no {name}"));
    p.waypoints(actor)
        .iter()
        .map(|w| w.init_matrix.position)
        .collect()
}

/// A fresh run on level 201 with Eruptor joined and its intro skipped, as
/// [`walked`] starts, for a test of its own.
fn fresh() -> Walker {
    fresh_as(None)
}

/// [`fresh`] with `character` joined in Eruptor's place.
fn fresh_as(character: Option<Character>) -> Walker {
    let setup = Setup {
        interface: true,
        props: true,
        sounds: true,
        character,
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
/// from the bridge's fall to Setup_005's end (the walk's record).
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
    // On the walk: the yard's pad on while he's about the landing, the
    // grass pit's on from the bridge's fall to Setup_005's end (the level's
    // controller takes it off at its start, the bridge's scene puts it back).
    let walked = walked();
    println!("the pads off after each stretch: {:?}", walked.pads);
    let off = |stretch: &str| {
        walked
            .pads
            .iter()
            .find(|(n, _)| n.starts_with(stretch))
            .map(|p| p.1)
    };
    assert_eq!(
        off("the landing").map(|o| o[0]),
        Some(false),
        "{:?}",
        walked.pads
    );
    for stretch in ["Setup_004", "the channel", "the second door"] {
        assert_eq!(off(stretch).map(|o| o[1]), Some(false), "{stretch}");
    }
}

/// openspec AS.175 with AS.168: the life area's first vine bridge. Set down
/// on its pan's trigger past the life wall (a Life hero's way in, the Mac's
/// run walks it), the pan opens the activator, whose roots burst and whose
/// platform grows with no solid, then lays its completed platform, which
/// stands for actors by its own solid, stretched across the gap; the hero
/// walks off the activator's end along it and over to the far side, where
/// before the bridge's own clones ran he fell.
#[test]
#[ignore = "reads the Giants dump"]
fn the_life_areas_first_vine_bridge_carries_him_over() {
    use guac_motion::seam::Vec3;
    let mut w = fresh();
    let (_, pan) = w.run.placed("Pan_Activate");
    let (act, at) = w.run.placed("Activator_Template(1)");
    w.run.put(pan + Vec3::new(0.0, 1.0, 0.0), 0.0);
    let start = w.run.steps;
    let done = |w: &Walker| {
        w.run
            .vm
            .placement_of(act)
            .and_then(|id| w.run.vm.behaviour(id))
            .is_some_and(|b| b.starts_with("5"))
    };
    while !done(&w) {
        assert!(
            w.run.steps < start + 2400,
            "the activator in {:?}",
            w.run
                .vm
                .placement_of(act)
                .and_then(|id| w.run.vm.behaviour(id))
        );
        w.rest(1);
    }
    w.rest(120);
    let bridge: Vec<(
        Handle,
        guac_motion::solid::HavokSolid,
        guac_motion::seam::Pose,
    )> = w
        .run
        .world
        .creature_bodies()
        .into_iter()
        .filter(|(h, _, _)| {
            w.run
                .world
                .actor(*h)
                .and_then(|a| a.template)
                .and_then(|t| w.run.world.template(t))
                .is_some_and(|p| p.name == "NatureBridgePlatformCompleted")
        })
        .collect();
    println!(
        "grown {} steps on: {:?}",
        w.run.steps - start,
        bridge
            .iter()
            .map(|b| (b.2.position, b.1))
            .collect::<Vec<_>>()
    );
    let [(h, _, _)] = bridge[..] else {
        panic!("the completed platform stands: {bridge:?}");
    };
    let a = w.run.world.actor(h).unwrap();
    println!(
        "the activator at {at}, the platform at {} facing {} scale {:?} solid {:?}",
        a.position, a.angles.facing, a.scale, a.solid
    );
    // Off the activator's end, north along the platform, to the far side.
    const FAR: [f32; 2] = [70.9, -81.5];
    w.run.put(at + Vec3::new(0.0, 0.5, 0.0), 0.0);
    w.rest(30);
    let mut low = f32::MAX;
    for _ in 0..900 {
        let me = w.run.hero().pivot;
        if (me.x - FAR[0]).hypot(me.z - FAR[1]) < 1.0 {
            break;
        }
        let stick = w.toward(FAR, 1.0);
        w.step(stick, [false; 4]);
        low = low.min(w.run.hero().pivot.y);
    }
    let end = w.run.hero().pivot;
    println!("over to {end}, lowest {low:.2}");
    assert!(
        (end.x - FAR[0]).hypot(end.z - FAR[1]) < 1.5,
        "never got over: {end}"
    );
    assert!(low > at.y - 2.0, "fell off it: {low}");
    assert!(w.walked.stops.is_empty(), "{:#?}", w.walked.stops);
}

/// A life area bridge's leg: its pan and where he's set down on it, the
/// activator it opens, where its bridge's walk starts and the far side.
type Leg = (&'static str, [f32; 3], &'static str, [f32; 2], [f32; 2]);

/// openspec AS.183: the life area past its island's ambush. Its battle's
/// end is set as it leaves it (the initializer taken off: the TV doesn't
/// run the battle, AS.200); then each later bridge from its pan, as the
/// first: the fight's pan opens the second activator, the second pan (on
/// the west island) the third, the third (north of the ambush's arena) the
/// fourth; each lays its completed platform, which stands for actors by
/// its own solid across its gap, and he walks over it, never falling. Then
/// the area's way out: walked onto its teleporter's start (west, by the hat
/// box), he's carried back to its end by the first bridge (the helper
/// `Teleporter_Helper.ai` its start makes, on 201's listing since AS.183;
/// off it he stood on the start).
#[test]
#[ignore = "reads the Giants dump"]
fn the_life_areas_later_bridges_and_its_teleporter_carry_him() {
    use guac_motion::seam::Vec3;
    let mut w = fresh();
    let (init, _) = w.run.placed("Phase Director Initializer 201 Minion");
    assert!(w.run.vm.remove(init));
    let obj = *w
        .run
        .handles
        .iter()
        .find(|(_, h)| **h == init)
        .expect("its object")
        .0;
    assert!(w.run.world.take_off(obj));
    let platforms = |w: &Walker| -> Vec<Vec3> {
        w.run
            .world
            .creature_bodies()
            .into_iter()
            .filter(|(h, _, _)| {
                w.run
                    .world
                    .actor(*h)
                    .and_then(|a| a.template)
                    .and_then(|t| w.run.world.template(t))
                    .is_some_and(|p| p.name == "NatureBridgePlatformCompleted")
            })
            .map(|(_, _, pose)| pose.position)
            .collect()
    };
    let legs: [Leg; 3] = [
        (
            "Pan_Activate(1)",
            [58.68, 30.0, -57.63],
            "Activator_Template(2)",
            [55.4, -57.3],
            [33.6, -57.2],
        ),
        (
            "Pan_Activate(2)",
            [26.32, 33.6, -43.48],
            "Activator_Template(3)",
            [27.5, -43.0],
            [54.4, -43.3],
        ),
        (
            "Pan_Activate(3)",
            [68.76, 30.0, -22.02],
            "Activator_Template(4)",
            [64.8, -21.96],
            [49.3, -21.1],
        ),
    ];
    for (k, (pan, at, act, foot, far)) in legs.into_iter().enumerate() {
        w.run.put(Vec3::from_array(at), 270.0);
        let act = w.run.placed(act).0;
        let start = w.run.steps;
        let done = |w: &Walker| {
            w.run
                .vm
                .placement_of(act)
                .and_then(|id| w.run.vm.behaviour(id))
                .is_some_and(|b| b.starts_with("5"))
        };
        while !done(&w) {
            assert!(w.run.steps < start + 1500, "{pan}'s bridge never grew");
            w.rest(1);
        }
        let grown = w.run.steps - start;
        w.rest(120);
        let laid = platforms(&w);
        assert_eq!(laid.len(), k + 1, "{pan}: each bridge laid: {laid:?}");
        let mut low = f32::MAX;
        for (to, n) in [(foot, 400u32), (far, 900)] {
            for _ in 0..n {
                let me = w.run.hero().pivot;
                if (me.x - to[0]).hypot(me.z - to[1]) < 0.8 {
                    break;
                }
                let s = w.toward(to, 1.0);
                w.step(s, [false; 4]);
                low = low.min(w.run.hero().pivot.y);
            }
        }
        let end = w.run.hero().pivot;
        println!(
            "{pan}: grown {grown} steps on, its platform at {}, over to {end}, lowest {low:.2}",
            laid[k]
        );
        assert!(
            (end.x - far[0]).hypot(end.z - far[1]) < 1.5,
            "{pan}: never got over: {end}"
        );
        assert!(low > at[1] - 2.0, "{pan}: fell off it: {low}");
    }
    // The way out: onto the teleporter's start, back to its end.
    let (_, start_at) = w.run.placed("TeleporterStart(1)");
    let (_, end_at) = w.run.placed("TeleporterEnd(1)");
    w.run.put(start_at + Vec3::new(0.0, 1.0, 3.0), 0.0);
    w.rest(30);
    let mut taken = None;
    for _ in 0..600 {
        let s = w.toward([start_at.x, start_at.z], 1.0);
        w.step(s, [false; 4]);
        if w.run.hero().pivot.distance(start_at) > 30.0 {
            taken = Some(w.run.steps);
            break;
        }
    }
    w.rest(120);
    let end = w.run.hero().pivot;
    println!("the teleporter: taken at {taken:?}, him at {end}, its end at {end_at}");
    assert!(
        taken.is_some(),
        "the teleporter left him on its start: {end}"
    );
    assert!(
        (end.x - end_at.x).hypot(end.z - end_at.z) < 1.5,
        "by its end: {end}"
    );
    assert!(w.walked.stops.is_empty(), "{:#?}", w.walked.stops);
}

/// Whether `h` runs an enemy action queue now (`EnemySystem_ActionQueues`,
/// one past its start): a queue that runs takes no new start, so the
/// avoider's hold, queued then, lets it run on (openspec AS.190).
fn queue_runs(w: &Walker, h: Handle) -> bool {
    let vm = &w.run.vm;
    vm.program()
        .global_named("EnemySystem_ActionQueues")
        .and_then(|g| vm.global_peek(g))
        .and_then(|s| vm.set(s).map(<[_]>::to_vec))
        .unwrap_or_default()
        .iter()
        .any(|q| {
            vm.field(*q, "^enemy").map(|e| vm.value(e)) == Some(guac_script::Value::Entity(h))
                && vm
                    .field(*q, "executionphase")
                    .and_then(|v| vm.value(v).as_i32())
                    .is_some_and(|p| p > 0)
        })
}

/// openspec AS.177, AS.190: the arena's lemming avoider. In the arena once
/// its battle's first wave is up and its Chompies have ended their entry
/// (each one's action queue, the leap into the arena, done: while one runs
/// the avoider's queue takes no start), the hero dropped off the arena's
/// floor beyond its rim: while he's below it the avoider queues each its
/// clip and idle and holds them, so they slide out of what they were doing
/// and stand; off the listing they run on at him to the rim. None follows him off; the
/// minion's own fall check brings him back up, and the wave comes on again.
#[test]
#[ignore = "reads the Giants dump"]
fn its_arenas_wave_stays_on_the_floor_with_him_below_it() {
    use guac_motion::seam::Vec3;
    const CENTRE: [f32; 3] = [-198.16, 24.05, 76.91];
    let mut w = fresh();
    // Past the fifth gate, the walk's own way through the eyeballs and into
    // the arena's intro.
    w.run.put(Vec3::new(-175.8, 26.0, 22.2), 0.0);
    w.rest(30);
    assert!(w.stretch(
        "h -180.3 20.7 20; a 150; s 30; w -173.2 22.2; w -171.2 24.2; w -168.2 25.2; w -165.2 26.2; w -162.8 27.8; w -162.8 31.8; w -160.2 32.2; h -158.6 34.3 20; a 150; w -162.8 30.2; w -163.2 26.8; w -166.8 26.2; w -168.2 28.8; w -172.2 28.8; w -175.8 29.2; w -175.8 33.2; w -175.8 37.2; w -175.8 41.2; s 120; cs; s 200"
    ));
    let wave = |w: &Walker| -> Vec<(Handle, Vec3)> {
        let run = &w.run;
        run.world
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
                    .map(|a| (h, a.position))
            })
            .filter(|(_, p)| (p.x - CENTRE[0]).hypot(p.z - CENTRE[2]) < 40.0)
            .collect()
    };
    let start = w.run.steps;
    while wave(&w).len() < 2 || w.in_scene() {
        assert!(w.run.steps < start + 6000, "no wave: {:?}", wave(&w));
        w.rest(1);
    }
    w.rest(60);
    let chompies = |w: &Walker| -> Vec<Handle> {
        let mut c: Vec<Handle> = wave(w)
            .into_iter()
            .map(|(h, _)| h)
            .filter(|h| w.run.world.name(*h).contains("Chompy"))
            .collect();
        c.sort_unstable();
        c.dedup();
        c
    };
    let entering = w.run.steps;
    while chompies(&w).iter().any(|h| queue_runs(&w, *h)) {
        assert!(
            w.run.steps < entering + 1800,
            "the Chompies' entry never ended"
        );
        w.rest(1);
    }
    let up: Vec<(Handle, Vec3)> = chompies(&w)
        .into_iter()
        .filter_map(|h| w.run.world.actor(h).map(|a| (h, a.position)))
        .collect();
    println!(
        "the wave {} steps on, its Chompies' entries over {} steps after: {up:?}",
        w.run.steps - start,
        w.run.steps - entering
    );
    let (avoider, _) = w.run.placed("Enemy_LemmingAvoider");
    let holding = |w: &Walker| {
        let id = w.run.vm.placement_of(avoider)?;
        let set = w.run.vm.local(id, "held enemies")?;
        w.run.vm.set(set).map(<[_]>::len)
    };
    println!(
        "the avoider awake {:?}, holding {:?}",
        w.run.world.actor(avoider).map(|a| a.active),
        holding(&w)
    );
    // Off the floor: 20 m out from its middle, 6 m under it, over nothing.
    let below = Vec3::new(CENTRE[0] + 20.0, CENTRE[1] - 6.0, CENTRE[2]);
    w.run.put(below, 0.0);
    let mut lowest = f32::MAX;
    let mut moved = 0.0f32;
    let mut held = 0u32;
    let mut most = 0usize;
    // Where each stands half a second into the hold, once the run each was
    // on (a nip, the chase) has slid out, and how far any goes after.
    let mut settled: Option<Vec<Vec3>> = None;
    let mut after = 0.0f32;
    for _ in 0..90 {
        w.rest(1);
        let me = w.run.hero().pivot;
        let under = me.y < CENTRE[1] - 1.0;
        if under {
            held += 1;
            most = most.max(holding(&w).unwrap_or(0));
        }
        let now: Vec<Vec3> = up
            .iter()
            .map(|(h, at)| w.run.world.actor(*h).map_or(*at, |a| a.position))
            .collect();
        if under && held == 30 {
            settled = Some(now.clone());
        }
        if let Some(from) = settled.as_ref().filter(|_| under) {
            for (a, b) in from.iter().zip(&now) {
                after = after.max((a.x - b.x).hypot(a.z - b.z));
            }
        }
        for (h, at) in &up {
            if let Some(a) = w.run.world.actor(*h).filter(|a| !a.removed) {
                lowest = lowest.min(a.position.y);
                // What the hold holds: how far each goes while he's under.
                if under {
                    moved = moved.max((a.position.x - at.x).hypot(a.position.z - at.z));
                }
            }
        }
    }
    println!(
        "{held} steps under the floor, the avoider holding {most} at most: the wave's lowest {lowest:.2} (the floor {}), moved at most {moved:.2} m while he was, {after:.2} m once it settled",
        CENTRE[1]
    );
    assert!(held > 30, "he never went under");
    assert!(
        settled.is_some() && after < 0.5,
        "the hold let one on at him: {after} m after it settled"
    );
    assert!(most >= up.len(), "the avoider held {most} of {}", up.len());
    assert!(
        lowest > CENTRE[1] - 2.0,
        "an enemy followed him off: {lowest}"
    );
    assert!(w.walked.stops.is_empty(), "{:#?}", w.walked.stops);
}

/// Level 201's arena's middle (`arenaCenter`), metres.
const ARENA: [f32; 3] = [-198.16, 24.05, 76.91];

/// Set down past the fifth gate and walked through its two eyeballs into
/// the arena, whose intro plays and is waited out, as the walk goes.
fn into_the_arena(w: &mut Walker) {
    w.run
        .put(guac_motion::seam::Vec3::new(-175.8, 26.0, 22.2), 0.0);
    w.rest(30);
    assert!(w.stretch(
        "h -180.3 20.7 20; a 150; s 30; w -173.2 22.2; w -171.2 24.2; w -168.2 25.2; w -165.2 26.2; w -162.8 27.8; w -162.8 31.8; w -160.2 32.2; h -158.6 34.3 20; a 150; w -162.8 30.2; w -163.2 26.8; w -166.8 26.2; w -168.2 28.8; w -172.2 28.8; w -175.8 29.2; w -175.8 33.2; w -175.8 37.2; w -175.8 41.2; s 120; cs; s 200"
    ));
}

/// The arena's foes a kid fights on its floor: awake, within 40 m of its
/// middle and 2 m of his height, their entry over (one still on its leap
/// is on the rim, out of his reach), nearest first.
fn arena_foes(w: &Walker) -> Vec<(Handle, guac_motion::seam::Vec3)> {
    let run = &w.run;
    let me = run.hero().pivot;
    let mut foes: Vec<(Handle, guac_motion::seam::Vec3)> = run
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
                .map(|a| (h, a.position))
        })
        .filter(|(_, p)| (p.x - ARENA[0]).hypot(p.z - ARENA[2]) < 40.0)
        .filter(|(_, p)| (p.y - me.y).abs() < 2.0)
        .filter(|(h, _)| !queue_runs(w, *h))
        .collect();
    foes.sort_by(|a, b| a.1.distance(me).total_cmp(&b.1.distance(me)));
    foes.dedup_by_key(|f| f.0);
    foes
}

/// One foe of the arena's battle: its name, the step it's first up and
/// where, whether its entry's action queue has run, and the step that
/// queue ended and where it stood then.
struct Entry {
    name: String,
    first: (u32, guac_motion::seam::Vec3),
    ran: bool,
    end: Option<(u32, guac_motion::seam::Vec3)>,
    hp: Option<f32>,
}

/// A foe's health as its scripts hold it.
fn foe_health(w: &Walker, h: Handle) -> Option<f32> {
    let vm = &w.run.vm;
    let d = vm
        .local(vm.placement_of(h)?, "description")
        .map(|v| vm.value(v))?;
    let c = vm.field(d, "creature info").map(|v| vm.value(v))?;
    vm.field(c, "hp").map(|v| vm.value(v))?.as_f32()
}

/// The commander the arena's script set up (its `commanderactor`).
fn commander(w: &Walker) -> Option<Handle> {
    let castle = w.run.placement("castleArena_script");
    match w
        .run
        .vm
        .local(castle, "commanderactor")
        .map(|v| w.run.vm.value(v))
    {
        Some(guac_script::Value::Entity(h)) => Some(h),
        _ => None,
    }
}

/// The commander's health as the arena's script reads it.
fn commander_health(w: &Walker) -> Option<f32> {
    let vm = &w.run.vm;
    let castle = w.run.placement("castleArena_script");
    let info = vm.local(castle, "commanderinfo").map(|v| vm.value(v))?;
    vm.field(info, "hp").map(|v| vm.value(v))?.as_f32()
}

/// openspec AS.201: the heavy elf over the arena's first wave is its
/// commander, Brock as his own heavy elf in his hat (`Wave Master - Setup
/// Commander`, the castle's `Real Brock`): made on his podium by its intro
/// path's place, 12 m over the floor, and held there under designer
/// control through every wave (his action queue held with it, his own
/// behaviour disabled), out of the waves and their avoider. Once the last
/// wave ends Brock's battle scene, his script clears that control and walks
/// him down his enter-combat path to the floor, where the castle's reveal
/// plays (within 2 ft of the middle's height, the castle's own check) and
/// his fight runs: hurt past 90 % and 50 % he brings a wave of life mages
/// each, beaten the dying Brock, the loot and the outro. Every other foe the
/// waves bring (Chompies, Elf Nears, lobbers, life mages) leaps in off the
/// rim and ends its entry on the floor. A kid's fight on the floor, the
/// nearest whose entry is over, his first attack every 20 steps within
/// 5 m; the hero topped up where his scripts hold his health, counted.
#[test]
#[ignore = "reads the Giants dump"]
fn its_arenas_commander_comes_down_after_the_waves_and_is_beaten() {
    use guac_motion::seam::Vec3;
    println!("load at start: {}", load());
    let mut w = fresh();
    into_the_arena(&mut w);
    let castle = w.run.placement("castleArena_script");
    let (_, podium) = w.run.placed("castleArena_Commander_Intro");
    let way = waypoints("castleArena_Commander_EnterCombatPath");
    let foot = *way.last().unwrap();
    let start = w.run.steps;
    // Each foe the battle brings: its name, the step it's first up and
    // where, whether its entry's queue has run, and the step that queue
    // ended and where it stood then.
    let mut entries: HashMap<Handle, Entry> = HashMap::new();
    let mut brock_scene: Option<(u32, Option<u32>)> = None;
    let mut reveal: Option<u32> = None;
    // Through the waves: his farthest off his podium and lowest over the
    // floor, his health; after Brock's scene: the step he's down, where.
    let (mut off_podium, mut lowest_up) = (0.0f32, f32::MAX);
    let mut full: Option<f32> = None;
    let mut down: Option<(u32, Vec3)> = None;
    let mut least: Option<f32> = None;
    // The step he's beaten, his health under 1 (the castle's own check).
    let mut beaten: Option<u32> = None;
    let mut since_moved = (w.run.hero().pivot, 0u32);
    let mut k = 0u32;
    let ended = |w: &Walker| {
        w.run.vm.removed(castle)
            || w.run
                .vm
                .behaviour(castle)
                .is_some_and(|b| b.eq_ignore_ascii_case("end"))
    };
    while !ended(&w) {
        assert!(w.run.steps < start + 40_000, "the battle never ended");
        k += 1;
        let scene = cutscene_name(&w.run);
        let in_brock = scene.as_deref() == Some("IGC_Arena_BrockBattle");
        match brock_scene {
            None if in_brock => brock_scene = Some((w.run.steps, None)),
            Some((from, None)) if !in_brock => brock_scene = Some((from, Some(w.run.steps))),
            _ => {}
        }
        if scene.as_deref() == Some("IGC_Elf_Heavy_Reveal") && reveal.is_none() {
            reveal = Some(w.run.steps);
        }
        let brock = commander(&w);
        if let Some(c) = brock.and_then(|h| w.run.world.actor(h)).map(|a| a.position) {
            match brock_scene {
                None => {
                    off_podium = off_podium.max((c.x - podium.x).hypot(c.z - podium.z));
                    lowest_up = lowest_up.min(c.y - ARENA[1]);
                    full = commander_health(&w).or(full);
                }
                Some((_, Some(_))) if down.is_none() && (c.y - ARENA[1]).abs() < 0.61 => {
                    down = Some((w.run.steps, c));
                }
                _ => {}
            }
            if let Some(h) = commander_health(&w).filter(|_| down.is_some()) {
                least = Some(least.map_or(h, |l: f32| l.min(h)));
                if h < 1.0 && beaten.is_none() {
                    beaten = Some(w.run.steps);
                }
            }
        }
        for h in w
            .run
            .world
            .creatures()
            .into_iter()
            .chain(w.run.world.clones())
        {
            let name = w.run.world.name(h);
            if !is_foe(&name) || Some(h) == brock {
                continue;
            }
            let Some(a) = w.run.world.actor(h).filter(|a| !a.removed) else {
                continue;
            };
            if (a.position.x - ARENA[0]).hypot(a.position.z - ARENA[2]) > 40.0 {
                continue;
            }
            let e = entries.entry(h).or_insert(Entry {
                name,
                first: (w.run.steps, a.position),
                ran: false,
                end: None,
                hp: None,
            });
            let runs = queue_runs(&w, h);
            e.ran |= runs;
            if e.ran && !runs && e.end.is_none() {
                e.end = Some((w.run.steps, a.position));
                e.hp = foe_health(&w, h);
            }
        }
        // The kid's fight.
        let me = w.run.hero().pivot;
        let target = arena_foes(&w).first().map(|f| f.1);
        let (stick, attack) = match target {
            Some(t) => {
                let far = (t.x - me.x).hypot(t.z - me.z);
                (
                    w.toward([t.x, t.z], if far > 2.5 { 1.0 } else { 0.35 }),
                    far <= 5.0 && k.is_multiple_of(20),
                )
            }
            None => (w.toward([ARENA[0], ARENA[2]], 0.5), false),
        };
        if me.distance(since_moved.0) > 0.5 {
            since_moved = (me, 0);
        } else {
            since_moved.1 += 1;
        }
        if since_moved.1 > 240 {
            // Held up 4 s: a step back toward the middle.
            for _ in 0..30 {
                let s = w.toward([ARENA[0], ARENA[2]], 1.0);
                w.step(s, [false; 4]);
            }
            since_moved = (w.run.hero().pivot, 0);
            continue;
        }
        w.step(stick, [attack, false, false, false]);
    }
    // The castle spawns the outro as it goes to its end.
    for _ in 0..120 {
        if cutscene_name(&w.run).is_some() {
            break;
        }
        w.rest(1);
    }
    let outro = cutscene_name(&w.run).filter(|n| n == "IGC_Arena_Outro");
    println!(
        "the battle {} steps, {} top-ups: on his podium {:.2} m off it at most, {:.2} m over the floor at least, health {full:?}; Brock's scene {brock_scene:?}, down {down:?} ({:.2} m from his path's foot {foot:?}), the reveal at {reveal:?}, his health down to {least:?}, beaten at {beaten:?}, the outro {outro:?}",
        w.run.steps - start,
        w.walked.topups.len(),
        off_podium,
        lowest_up,
        down.map_or(f32::NAN, |(_, c)| (c.x - foot[0]).hypot(c.z - foot[2])),
    );
    // The waves, made before Brock's scene, and the life mages and lobbers
    // his hurts call after it, some of whom the battle's end sends off
    // mid-entry (at his defeat the castle walks every one of the wave's to
    // an exit point).
    let brock_from = brock_scene.map_or(u32::MAX, |b| b.0);
    let (waves, later): (Vec<&Entry>, Vec<&Entry>) =
        entries.values().partition(|e| e.first.0 < brock_from);
    let mut kinds: HashMap<&str, (u32, u32, f32)> = HashMap::new();
    for e in waves.iter().chain(&later) {
        let k = kinds.entry(e.name.as_str()).or_default();
        k.0 += 1;
        if let Some((_, at)) = e.end {
            k.1 += 1;
            k.2 = k.2.max((at.y - ARENA[1]).abs());
        }
    }
    for e in waves.iter().chain(&later) {
        if let Some((step, at)) = e.end.filter(|(_, at)| (at.y - ARENA[1]).abs() >= 0.5) {
            println!(
                "  off the floor: {} from {:?}, ended {step} at {at}, health {:?}",
                e.name, e.first, e.hp
            );
        }
    }
    let cut_short: Vec<String> = later
        .iter()
        .filter(|e| e.end.is_none())
        .map(|e| format!("{} from {} at {}", e.name, e.first.1, e.first.0))
        .collect();
    println!(
        "the battle's foes (made, entries ended, farthest off the floor's height as one ended): {kinds:?}; {} in the waves, {} after Brock's scene, cut short by the battle's end: {cut_short:?}",
        waves.len(),
        later.len()
    );
    assert!(full.is_some(), "no commander through the waves");
    assert!(off_podium < 1.0, "off his podium: {off_podium}");
    assert!(lowest_up > 10.0, "down among the waves: {lowest_up}");
    let (_, Some(scene_end)) = brock_scene.expect("Brock's battle scene") else {
        panic!("Brock's battle scene never ended");
    };
    let (down_at, c) = down.expect("never came down to the floor");
    assert!(
        down_at - scene_end < 600,
        "down {} steps after the scene",
        down_at - scene_end
    );
    assert!(
        (c.x - foot[0]).hypot(c.z - foot[2]) < 2.0,
        "down off his path: {c} against {foot:?}"
    );
    assert!(
        reveal.is_some_and(|r| r >= down_at.saturating_sub(30)),
        "the reveal {reveal:?}, down {down_at}"
    );
    assert!(
        least.is_some_and(|l| l < full.unwrap() * 0.5),
        "never fought: {least:?} of {full:?}"
    );
    assert!(outro.is_some(), "no outro: {:?}", w.walked.scenes);
    assert!(waves.len() >= 30, "the waves' foes: {}", waves.len());
    // An entry ends on the floor, or with the foe beaten on its way in.
    let on_floor = |e: &&Entry| {
        e.end.is_some_and(|(_, at)| (at.y - ARENA[1]).abs() < 0.5)
            || (e.end.is_some() && e.hp.is_some_and(|h| h <= 0.0))
    };
    assert!(
        waves.iter().all(on_floor),
        "a wave's entry off the floor or never over: {kinds:?}"
    );
    let beaten = beaten.expect("never beaten");
    assert!(
        later
            .iter()
            .filter(|e| e.end.is_some_and(|(step, _)| step < beaten))
            .all(on_floor),
        "a later entry off the floor: {kinds:?}"
    );
    assert!(w.walked.stops.is_empty(), "{:#?}", w.walked.stops);
    println!("load at end: {}", load());
}

/// Level 201's push-block puzzle's lower floor (the au-201-elves lane's
/// reading): the dry channel and its basins sit 3.7 m under the puzzle's
/// top, and its one way back up is the ramp at the north basin's middle
/// (x -106 to -98, z -54 to -44), up to the top by the second house's door.
/// A kid off the puzzle lands on that floor: the Mac drivers' two stops, in
/// the north basin's west end after its first block and in the channel by
/// the side blocks' bridge, and the channel's south end and its south basin,
/// each walked north along the floor and up the ramp, never higher than the
/// top's 13.9 m nor out of it. The walk's own way takes the same ramp out of
/// the south basin past Setup_005's blocks.
#[test]
#[ignore = "reads the Giants dump"]
fn its_push_blocks_lower_floor_leads_back_up_its_ramp() {
    use guac_motion::seam::Vec3;
    const RAMP: [[f32; 2]; 5] = [
        [-101.0, -34.0],
        [-102.0, -40.0],
        [-102.0, -45.0],
        [-102.5, -50.0],
        [-103.0, -57.0],
    ];
    println!("load at start: {}", load());
    let mut w = fresh();
    for (from, label) in [
        ([-113.5, 10.4, -35.2], "the north basin's west end"),
        ([-98.2, 10.4, -22.8], "the channel by the side blocks"),
        ([-100.5, 10.4, -18.0], "the channel's south end"),
        ([-95.0, 10.4, -10.0], "the south basin"),
    ] {
        w.run.put(Vec3::from_array(from), 0.0);
        w.rest(30);
        let landed = w.run.hero().pivot;
        let mut legs = Vec::new();
        for to in std::iter::once([-100.5, -26.0]).chain(RAMP) {
            for _ in 0..600 {
                let me = w.run.hero().pivot;
                if (me.x - to[0]).hypot(me.z - to[1]) < 1.0 {
                    break;
                }
                let s = w.toward(to, 1.0);
                w.step(s, [false; 4]);
            }
            legs.push(w.run.hero().pivot);
        }
        let end = w.run.hero().pivot;
        println!("{label}: landed {landed}, up the ramp to {end}, by way of {legs:?}");
        assert!(landed.y < 10.5, "{label}: not on the floor: {landed}");
        assert!(end.y > 13.5, "{label}: never back up: {end}");
        assert!(
            (end.x + 103.0).hypot(end.z + 57.0) < 1.0,
            "{label}: up somewhere else: {end}"
        );
    }
    assert!(w.walked.stops.is_empty(), "{:#?}", w.walked.stops);
    println!("load at end: {}", load());
}

/// openspec AS.200: the life island's ambush. A hero on the island past
/// the first vine bridge wakes its initializer, which defines a minion boss
/// battle: the phase director's summon cutscene brings three evil Stealth
/// Elves in through the portal's entry beam, each on Stealth Elf's own
/// scripts, walked as a creature, with a minion controller, its button
/// controllers and its AI (`026_StealthElf_AI`), which bull-rushes him,
/// presses the elf's attack and smokes away; a kid's fight beats each (the
/// nearest, his first attack every 20 steps within 5 m, topped up where his
/// scripts hold his health, counted), the defeated cutscene after each,
/// then the victory phase ends the battle and takes the initializer off,
/// which opens the second bridge's pan; he walks onto its switch and the
/// bridge grows. The battle's camera is left off (AS.210): the gameplay
/// camera follows him through it.
#[test]
#[ignore = "reads the Giants dump"]
fn its_life_islands_ambush_is_fought_and_opens_the_second_bridge() {
    use guac_motion::seam::Vec3;
    println!("load at start: {}", load());
    let mut w = fresh();
    let (init, _) = w.run.placed("Phase Director Initializer 201 Minion");
    let (bridge, _) = w.run.placed("Activator_Template(2)");
    let (_, switch) = w.run.placed("Pan_Activate(1)");
    let director = w.run.placement("PhaseDirector");
    w.run.put(Vec3::new(70.5, 30.0, -75.0), 0.0);
    w.rest(30);
    let start = w.run.steps;
    // Each evil elf: the step it's first up, its least health, the step
    // it's beaten; the director's behaviours in order; the hero's health
    // drops while one stands.
    let mut evil: HashMap<Handle, (u32, f32, Option<u32>)> = HashMap::new();
    let mut director_went: Vec<String> = Vec::new();
    let mut rushes = 0u32;
    let mut hurts = 0u32;
    let mut last_health = w.run.health();
    let (mut ended, mut grown) = (None, None);
    for k in 0..9000u32 {
        let me = w.run.hero().pivot;
        let up: Vec<(Handle, Vec3)> = w
            .run
            .world
            .clones()
            .into_iter()
            .filter(|h| w.run.world.name(*h) == "026_StealthElf_Evil")
            .filter_map(|h| {
                w.run
                    .world
                    .actor(h)
                    .filter(|a| !a.removed)
                    .map(|a| (h, a.position))
            })
            .collect();
        let target = up
            .iter()
            .filter(|(h, _)| foe_health(&w, *h).is_some_and(|hp| hp > 0.0))
            .min_by(|a, b| a.1.distance(me).total_cmp(&b.1.distance(me)))
            .map(|e| [e.1.x, e.1.z]);
        let (stick, attack) = match (k < 300, target) {
            (true, _) => (w.toward([70.12, -60.0], 1.0), false),
            // The battle over, onto the second bridge's switch by its foot.
            (false, _) if ended.is_some() => (w.toward([switch.x, switch.z], 1.0), false),
            (false, Some(t)) => {
                let far = (t[0] - me.x).hypot(t[1] - me.z);
                (
                    w.toward(t, if far > 2.5 { 1.0 } else { 0.35 }),
                    far <= 5.0 && k.is_multiple_of(20),
                )
            }
            _ => (Stick::new(0.0, 0.0), false),
        };
        w.step(stick, [attack, false, false, false]);
        for (h, _) in &up {
            let hp = foe_health(&w, *h).unwrap_or(0.0);
            let e = evil.entry(*h).or_insert((w.run.steps, hp, None));
            e.1 = e.1.min(hp);
            if hp <= 0.0 && e.2.is_none() {
                e.2 = Some(w.run.steps);
            }
        }
        let health = w.run.health();
        if let (Some(was), Some(now)) = (last_health, health)
            && now < was
            && !up.is_empty()
        {
            hurts += 1;
        }
        last_health = health;
        let vm = &w.run.vm;
        if let Some(b) = vm.behaviour(director)
            && director_went.last().map(String::as_str) != Some(b)
        {
            director_went.push(b.to_owned());
        }
        rushes += w
            .run
            .world
            .clones()
            .into_iter()
            .filter(|h| w.run.world.name(*h) == "026_StealthElf_AI")
            .filter_map(|h| vm.placement_of(h).and_then(|id| vm.behaviour(id)))
            .filter(|b| *b == "bull rush")
            .count() as u32;
        if ended.is_none() && vm.placement_of(init).is_some_and(|id| vm.removed(id)) {
            ended = Some(w.run.steps);
        }
        if grown.is_none()
            && vm
                .placement_of(bridge)
                .and_then(|id| vm.behaviour(id))
                .is_some_and(|b| b.starts_with('5'))
        {
            grown = Some(w.run.steps);
            break;
        }
    }
    let mut elves: Vec<_> = evil.values().copied().collect();
    elves.sort_by_key(|e| e.0);
    println!(
        "{} steps: the elves (up, least health, beaten) {elves:?}; the director {director_went:?}; their AIs' rush steps {rushes}; the hero hurt {hurts} times while one stood, {} top-ups; the battle over at {ended:?}, the second bridge grown at {grown:?}",
        w.run.steps - start,
        w.walked.topups.len()
    );
    assert_eq!(elves.len(), 3, "the three summoned: {elves:?}");
    assert!(
        elves.iter().all(|e| e.2.is_some()),
        "each beaten: {elves:?}"
    );
    for b in [
        "minion summon cutscene",
        "phase update",
        "minion defeated cutscene",
    ] {
        assert!(
            director_went.iter().any(|d| d == b),
            "{b}: {director_went:?}"
        );
    }
    assert!(rushes > 0, "their AIs never rushed him");
    assert!(hurts > 0, "they never hurt him");
    let ended = ended.expect("the battle never ended");
    assert!(ended > elves.iter().filter_map(|e| e.2).max().unwrap());
    assert!(
        grown.is_some_and(|g| g > ended),
        "the second bridge: {grown:?}"
    );
    // Its camera off the listing, the gameplay camera following him
    // through it (stand-in ledger: `boss-camera`).
    let camera = w.run.placement("BossCamera");
    assert_eq!(w.run.vm.behaviour(camera), None, "the boss camera ran");
    assert!(w.walked.stops.is_empty(), "{:#?}", w.walked.stops);
    println!("load at end: {}", load());
}

/// The au-201-elves review (openspec AS.200): a hero taken off before his
/// entry beam activated him has his beam cleared
/// (`scripts::clear_entry_beams`, the deactivate path's), and only his: an
/// enemy minion's beam carries no controller. Cleared the step the life
/// island's summon has made its three beams, all three still land, each
/// evil Stealth Elf made with its AI, and the director goes on past its
/// summon; with every beam cleared none of them lands and the battle never
/// ends, the second bridge with it.
#[test]
#[ignore = "reads the Giants dump"]
fn its_ambushs_beams_outlast_a_heros_cleared_one() {
    use guac_motion::seam::Vec3;
    println!("load at start: {}", load());
    let mut w = fresh();
    let director = w.run.placement("PhaseDirector");
    w.run.put(Vec3::new(70.5, 30.0, -75.0), 0.0);
    w.rest(30);
    let start = w.run.steps;
    let alive = |w: &Walker, name: &str| {
        w.run
            .world
            .clones()
            .into_iter()
            .filter(|h| w.run.world.name(*h) == name)
            .filter(|h| w.run.world.actor(*h).is_some_and(|a| !a.removed))
            .count()
    };
    let mut cleared = None;
    let (mut elves, mut ais) = (0, 0);
    for _ in 0..900u32 {
        let n = guacamole::scripts::entry_beam_count(&mut w.run.vm).unwrap_or(0);
        if cleared.is_none() && n > 0 {
            w.run
                .with_host(guacamole::scripts::clear_entry_beams)
                .unwrap_or_else(|h| panic!("{h}"));
            let left = guacamole::scripts::entry_beam_count(&mut w.run.vm);
            cleared = Some((w.run.steps, n, left));
        }
        let s = w.toward([70.12, -60.0], 1.0);
        w.step(s, [false; 4]);
        elves = alive(&w, "026_StealthElf_Evil");
        ais = alive(&w, "026_StealthElf_AI");
        let summoning = w.run.vm.behaviour(director) == Some("minion summon cutscene");
        if elves == 3 && ais == 3 && !summoning {
            break;
        }
    }
    let past = w.run.vm.behaviour(director).map(str::to_owned);
    println!(
        "the beams cleared at {cleared:?} (step, beams, beams after); {} steps on: {elves} evil elves up, {ais} AIs, the director {past:?}",
        w.run.steps - start
    );
    let (_, made, left) = cleared.expect("the summon's beams");
    assert_eq!(made, 3, "the summon's three beams");
    assert_eq!(
        left,
        Some(3),
        "a hero's clean-up took an enemy minion's beam"
    );
    assert_eq!(elves, 3, "the three landed");
    assert_eq!(ais, 3, "each with its AI");
    assert_ne!(
        past.as_deref(),
        Some("minion summon cutscene"),
        "past the summon"
    );
    assert!(w.walked.stops.is_empty(), "{:#?}", w.walked.stops);
    println!("load at end: {}", load());
}

/// openspec AS.168: the rope bridge over the channel holds the hero by its
/// planks' own solids, no way naming them: set down on it halfway over the
/// channel, 10 m above its floor, he stands on its planks and walks on
/// along them toward its far end, each plank a clone of the bridge's
/// controller's that stands for actors (`Actor::stands_for_actors`). The
/// walk's own way reaches it at its head and Brock's scene takes it out
/// from under him (`level_201_is_walked_from_its_landing_to_the_hub`).
#[test]
#[ignore = "reads the Giants dump"]
fn its_bridge_holds_him_by_its_planks_own_solids() {
    use guac_motion::seam::Vec3;
    const ON: [f32; 3] = [-112.23, 25.0, -77.91];
    const ALONG: [f32; 2] = [-118.86, -65.93];
    let mut w = fresh();
    w.run.put(Vec3::from_array(ON), 0.0);
    w.rest(60);
    let planks = w
        .run
        .world
        .creature_bodies()
        .into_iter()
        .filter(|(h, _, _)| w.run.world.actor(*h).is_some_and(|a| a.stands_for_actors()))
        .count();
    let stood = w.run.hero().pivot;
    let mut low = stood.y;
    for _ in 0..600 {
        let at = w.run.hero().pivot;
        if (at.x - ALONG[0]).hypot(at.z - ALONG[1]) < 1.0 {
            break;
        }
        let stick = w.toward(ALONG, 1.0);
        w.step(stick, [false; 4]);
        low = low.min(w.run.hero().pivot.y);
    }
    let end = w.run.hero().pivot;
    let walked = (end.x - stood.x).hypot(end.z - stood.z);
    println!(
        "{planks} clones standing for actors; stood at {stood}, walked {walked:.2} m to {end}, lowest {low:.2}"
    );
    assert!(planks >= 25, "the planks: {planks}");
    assert!(stood.y > 23.5, "fell through it: {stood}");
    assert!(walked > 8.0, "never got along it: {end}");
    assert!(low > 23.0, "fell through it: {low}");
    assert!(w.walked.stops.is_empty(), "{:#?}", w.walked.stops);
}

/// The life area on foot past its second bridge (openspec AS.183), leg by
/// leg as [`WAY`] spells its stretches: from the second bridge's far end
/// up the west island's ramp into the pocket behind its push blocks; the
/// second block east a span, the first north into its end, the second
/// north twice into its end, the two side by side across the gap between
/// the island's ledges; up the ramp, along the west ledge and over them
/// onto the third bridge's switch.
const LIFE_WEST_ISLAND: &str = "w 24 -58.5; w 14 -59; w 12 -57; w 12 -53.6; h 12 -53.5 8; push 30 -53.5 150; w 16.8 -54.4; h 16.8 -54.4 8; push 16.8 -30 150; w 16.8 -48.5; w 21.5 -48.5; w 25.5 -48.5; w 25.5 -57.5; w 21.35 -57.5; push 21.35 -30 150; w 21.35 -53.4; push 21.35 -30 150; w 25.5 -50; w 25.5 -57.5; w 14 -58.5; w 3 -58.5; w 3 -50; w 3 -44.5; w 12 -44.4; w 17 -44.4; w 21.5 -44.4; w 26.3 -43.5";

/// Over the third bridge onto the plateau, the bomb its spawner lays taken
/// and thrown at the long-range eyeball across the gap, which drops the
/// timed door below; off the plateau's edge, through the door and onto the
/// fourth bridge's switch.
const LIFE_PLATEAU: &str = "w 27.5 -43.0; w 54.4 -43.3; w 60 -42; w 58 -40; m 12; w 62 -40; w 69 -39; w 70.6 -32; w 70.6 -25; w 68.76 -22.5";

/// Over the fourth bridge onto the strip, off its west edge into the valley
/// below, onto its jump pad and up to the ground before the wall.
const LIFE_VALLEY: &str = "w 64.8 -21.96; w 49.3 -21.1; w 40 -21; w 37 -10.4; w 30 -10.4; s 60; w 18.4 -16.3; s 120; w 10 -26; w 3 -27.6; h -6 -27.6 40";

/// A Giant's way out: through the wall his attack took, past the hat box
/// (asleep headless, the story save's to wake: the TV's run mashes it
/// open), to the teleporter's start.
const LIFE_GIANT_OUT: &str = "w -6 -28; w -9 -31.5; w -16 -31.5";

/// Anyone's way out on foot: up the ground's south end and off it onto the
/// west island's ledge, over the blocks onto the switch's ledge and off it,
/// back over the second bridge.
const LIFE_ON_FOOT_OUT: &str = "w 5 -30; w 5 -36; w 5 -40; w 5 -43; w 12 -44.4; w 17 -44.4; w 21.5 -44.4; w 26 -44.5; w 27 -48.5; w 30 -55; w 33.6 -57.2; w 55.4 -57.3; w 63 -60; w 70.5 -66";

/// Back over the first bridge from the island's landing, out of the area:
/// from its switch's ground down onto the level's way by Setup_002's door
/// (the life wall's plateau stands 5 m over the switch, a way in alone).
const LIFE_OVER_BRIDGE_1: &str = "w 70.5 -74; w 70.9 -81.5; w 70.96 -103; w 70.84 -111.06; w 72.5 -118; w 73.5 -126; w 76.2 -134.2";

/// Whether the vine bridge activator `act` has laid its bridge.
fn bridge_grown(w: &Walker, act: Handle) -> bool {
    w.run
        .vm
        .placement_of(act)
        .and_then(|id| w.run.vm.behaviour(id))
        .is_some_and(|b| b.starts_with('5'))
}

/// [`LIFE_WEST_ISLAND`] on: the life area from its second bridge's far end
/// to the ground before its wall, each puzzle as a kid plays it; each
/// bridge's activator laid and each leg's end where it should be.
fn life_area_on(w: &mut Walker) {
    let at = |w: &Walker| w.run.hero().pivot;
    let blocks = [
        w.run.placed("Push_Block_Template_1_c(1)").0,
        w.run.placed("Push_Block_Template_1_c(2)").0,
    ];
    assert!(w.stretch(LIFE_WEST_ISLAND), "the west island");
    for b in blocks {
        let a = w.run.world.actor(b).unwrap();
        let beh = w
            .run
            .vm
            .placement_of(b)
            .and_then(|id| w.run.vm.behaviour(id));
        println!("{}: at {} {beh:?}", w.run.world.name(b), a.position);
        assert_eq!(beh, Some("special - push block inactive"), "{}", a.position);
    }
    let act = w.run.placed("Activator_Template(3)").0;
    let start = w.run.steps;
    while !bridge_grown(w, act) {
        assert!(w.run.steps < start + 1500, "the third bridge never grew");
        w.rest(1);
    }
    println!(
        "the third bridge grown {} steps on, him at {}",
        w.run.steps - start,
        at(w)
    );
    w.rest(120);
    let door = w.run.placed("EyE_Timer_Door_Template(3)").0;
    let shut = w.run.world.actor(door).unwrap().position.y;
    assert!(w.stretch(LIFE_PLATEAU), "the plateau to the fourth switch");
    let dropped = w.run.world.actor(door).unwrap().position.y;
    println!("the door from {shut:.2} to {dropped:.2}, him at {}", at(w));
    assert!(dropped < shut - 5.0, "the door never dropped: {dropped}");
    let act = w.run.placed("Activator_Template(4)").0;
    let start = w.run.steps;
    while !bridge_grown(w, act) {
        assert!(w.run.steps < start + 1500, "the fourth bridge never grew");
        w.rest(1);
    }
    println!("the fourth bridge grown {} steps on", w.run.steps - start);
    w.rest(120);
    assert!(w.stretch(LIFE_VALLEY), "the valley and its jump pad");
    let end = at(w);
    println!("before the wall at {end}");
    assert!(end.y > 32.5 && end.x < 3.0, "not up before the wall: {end}");
}

/// His first attack every 7 steps at `wall` before him for at most 600
/// steps, as a kid mashes it: the step it was taken off, if it was.
fn strike(w: &mut Walker, wall: Handle) -> Option<u32> {
    for k in 0..600 {
        if w.run.world.removed(wall) {
            return Some(k);
        }
        w.step(Stick::new(0.0, 0.0), [k % 7 == 0, false, false, false]);
    }
    None
}

/// openspec AS.183: the life area on foot past its second bridge, a hero who
/// isn't a Giant (Eruptor in a Life hero's place: the area's own scripts
/// ask nothing of his element past its wall). Through the ambush: the
/// west island's push blocks pushed into their ends, the third bridge from
/// its switch on the ledge they reach, the plateau's bomb thrown at the
/// eyeball across the gap, the timed door below, the fourth bridge, the
/// valley and its jump pad; the wall before the hat box and the teleporter
/// a Giant's (it stands to his attacks), so his way out is on foot, off the
/// ground's south end onto the west island and back over the second and
/// first bridges.
#[test]
#[ignore = "reads the Giants dump"]
fn the_life_area_is_walked_on_past_its_second_bridge_and_out() {
    let mut w = past_the_ambush(None);
    life_area_on(&mut w);
    let wall = w.run.placed("Breakable_Stone_Wall_Castle_206(4)").0;
    assert_eq!(
        strike(&mut w, wall),
        None,
        "his attacks took a Giant's wall"
    );
    assert!(
        w.run.hero().pivot.x > 0.0,
        "past the wall: {}",
        w.run.hero().pivot
    );
    assert!(w.stretch(LIFE_ON_FOOT_OUT), "back to the second bridge");
    assert!(w.stretch(LIFE_OVER_BRIDGE_1), "over the first bridge");
    let end = w.run.hero().pivot;
    println!("out at {end}, {} top-ups", w.walked.topups.len());
    assert!(end.z < -133.0 && end.y < 29.0, "out on the way: {end}");
    assert!(w.walked.stops.is_empty(), "{:#?}", w.walked.stops);
}

/// openspec AS.183: the life area past its second bridge with Tree Rex,
/// the life Giant: as [`the_life_area_is_walked_on_past_its_second_bridge_and_out`]
/// to the wall, which his attacks take; past the hat box behind it onto the
/// teleporter's start, carried to its end by the first bridge and back
/// over it, out of the area.
#[test]
#[ignore = "reads the Giants dump"]
fn the_life_area_is_walked_on_past_its_second_bridge_by_a_giant() {
    let mut w = past_the_ambush(Some(TREE_REX));
    life_area_on(&mut w);
    let wall = w.run.placed("Breakable_Stone_Wall_Castle_206(4)").0;
    let struck = strike(&mut w, wall);
    println!("the wall taken {struck:?} steps into his attacks");
    assert!(struck.is_some(), "his attacks left the wall");
    assert!(w.stretch(LIFE_GIANT_OUT), "past the wall");
    let (_, start) = w.run.placed("TeleporterStart(1)");
    let (_, end_at) = w.run.placed("TeleporterEnd(1)");
    let mut taken = None;
    for _ in 0..600 {
        let s = w.toward([start.x, start.z], 1.0);
        w.step(s, [false; 4]);
        if w.run.hero().pivot.distance(start) > 30.0 {
            taken = Some(w.run.steps);
            break;
        }
    }
    w.rest(120);
    let landed = w.run.hero().pivot;
    println!("the teleporter: taken at {taken:?}, him at {landed}, its end at {end_at}");
    assert!(taken.is_some(), "left on its start: {landed}");
    assert!(
        (landed.x - end_at.x).hypot(landed.z - end_at.z) < 1.5,
        "by its end: {landed}"
    );
    assert!(w.stretch(LIFE_OVER_BRIDGE_1), "over the first bridge");
    let end = w.run.hero().pivot;
    println!("out at {end}, {} top-ups", w.walked.topups.len());
    assert!(end.z < -133.0 && end.y < 29.0, "out on the way: {end}");
    assert!(w.walked.stops.is_empty(), "{:#?}", w.walked.stops);
}

/// A run on level 201 with `character` joined in Eruptor's place, the life
/// island's ambush fought through and its second bridge crossed, him at its
/// far end: the first bridge grown from its switch past the life wall (his
/// way in), then onto the island, where its initializer wakes the battle,
/// and the three evil Stealth Elves fought as a kid fights them (the
/// nearest, his first attack every 20 steps within 5 m, as
/// [`its_life_islands_ambush_is_fought_and_opens_the_second_bridge`]); the
/// victory takes the initializer off and he walks onto the second switch,
/// whose bridge grows, and over it.
fn past_the_ambush(character: Option<Character>) -> Walker {
    use guac_motion::seam::Vec3;
    let mut w = fresh_as(character);
    let (_, pan) = w.run.placed("Pan_Activate");
    w.run.put(pan + Vec3::Y, 0.0);
    let first = w.run.placed("Activator_Template(1)").0;
    let start = w.run.steps;
    while !bridge_grown(&w, first) {
        assert!(w.run.steps < start + 1500, "the first bridge never grew");
        w.rest(1);
    }
    w.rest(120);
    let init = w.run.placement("Phase Director Initializer 201 Minion");
    let second = w.run.placed("Activator_Template(2)").0;
    let (_, switch) = w.run.placed("Pan_Activate(1)");
    w.run.put(Vec3::new(70.5, 30.0, -75.0), 0.0);
    w.rest(30);
    let (mut beaten, mut ended) = (0usize, None);
    for k in 0..9000u32 {
        let me = w.run.hero().pivot;
        let up: Vec<(Handle, Vec3)> = w
            .run
            .world
            .clones()
            .into_iter()
            .filter(|h| w.run.world.name(*h) == "026_StealthElf_Evil")
            .filter_map(|h| {
                w.run
                    .world
                    .actor(h)
                    .filter(|a| !a.removed)
                    .map(|a| (h, a.position))
            })
            .collect();
        beaten = beaten.max(
            up.iter()
                .filter(|(h, _)| foe_health(&w, *h).is_some_and(|hp| hp <= 0.0))
                .count(),
        );
        let target = up
            .iter()
            .filter(|(h, _)| foe_health(&w, *h).is_some_and(|hp| hp > 0.0))
            .min_by(|a, b| a.1.distance(me).total_cmp(&b.1.distance(me)))
            .map(|e| [e.1.x, e.1.z]);
        let (stick, attack) = match (k < 300, target) {
            (true, _) => (w.toward([70.12, -60.0], 1.0), false),
            (false, _) if ended.is_some() => (w.toward([switch.x, switch.z], 1.0), false),
            (false, Some(t)) => {
                let far = (t[0] - me.x).hypot(t[1] - me.z);
                (
                    w.toward(t, if far > 2.5 { 1.0 } else { 0.35 }),
                    far <= 5.0 && k.is_multiple_of(20),
                )
            }
            _ => (Stick::new(0.0, 0.0), false),
        };
        w.step(stick, [attack, false, false, false]);
        if ended.is_none() && w.run.vm.removed(init) {
            ended = Some(w.run.steps);
        }
        if bridge_grown(&w, second) {
            break;
        }
    }
    println!(
        "the ambush over at {ended:?}, the second bridge grown at {}, {} top-ups so far",
        w.run.steps,
        w.walked.topups.len()
    );
    assert!(ended.is_some(), "the ambush never ended");
    assert!(bridge_grown(&w, second), "the second bridge never grew");
    w.rest(120);
    assert!(
        w.stretch("w 61.5 -57.4; w 55.4 -57.3; w 33.6 -57.2"),
        "over the second bridge"
    );
    w
}
