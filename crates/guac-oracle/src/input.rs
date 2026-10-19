//! Input scripts (openspec AF.7, design decision 3): what each pad holds, flip by
//! flip, in one RON file that drives the oracle (the harness turns it into O4's
//! `PAD` lines) and, at a later rung, Guacamole through its positional mapping.
//!
//! A step names a flip offset and the state of port 0, port 1 or both from that
//! offset on; a port holds it until its next step. Offset 0 is the first flip
//! after the pause the script is loaded at, and before a port's first step the
//! oracle gives it the host's input, so a script that owns a port sets it at 0.
//!
//! Buttons are named by POSITION, as Bevy's `GamepadButton` names them (South is
//! the bottom face button whatever the pad prints), because the original's
//! controls are positional (docs/native/guacamole-design.md, Controllers). Sticks
//! are a DualShock 3's bytes: 128 centred, x 0 full left, y 0 full up (forward).
//!
//! ```ron
//! (
//!     steps: [
//!         // Both pads from the start: port 0 pushes forward, port 1 rests.
//!         (at: 0, pads: {0: (left: (x: 128, y: 0)), 1: ()}),
//!         // South for 4 flips, still walking.
//!         (at: 60, pads: {0: (buttons: [South], left: (x: 128, y: 0))}),
//!         (at: 64, pads: {0: (left: (x: 128, y: 0))}),
//!         (at: 120, pads: {0: ()}),
//!     ],
//! )
//! ```

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use serde::de::{self, MapAccess, Visitor};
use serde::{Deserialize, Deserializer, Serialize};

/// The ports a script drives: 0 and 1, the two co-op players.
pub const PORTS: usize = 2;

/// A button by its position on the pad, spelled as Bevy's `GamepadButton`
/// spells it. The trigger pairs follow Bevy too: `LeftTrigger` is the upper
/// shoulder button (L1), `LeftTrigger2` the lower one (L2).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum Button {
    South,
    East,
    North,
    West,
    LeftTrigger,
    LeftTrigger2,
    RightTrigger,
    RightTrigger2,
    Select,
    Start,
    LeftThumb,
    RightThumb,
    DPadUp,
    DPadDown,
    DPadLeft,
    DPadRight,
}

/// Which of a DualShock 3's two digital words a button reports in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Word {
    Digital1,
    Digital2,
}

impl Button {
    pub const ALL: [Button; 16] = [
        Button::South,
        Button::East,
        Button::North,
        Button::West,
        Button::LeftTrigger,
        Button::LeftTrigger2,
        Button::RightTrigger,
        Button::RightTrigger2,
        Button::Select,
        Button::Start,
        Button::LeftThumb,
        Button::RightThumb,
        Button::DPadUp,
        Button::DPadDown,
        Button::DPadLeft,
        Button::DPadRight,
    ];

    /// Where a DualShock 3 reports the button at this position: its word and
    /// `CELL_PAD_CTRL_*` bit (libpad's layout, which RPCS3's `pad_types.h`
    /// spells out). The 16 positions fill both words exactly.
    pub fn ds3_bit(self) -> (Word, u8) {
        use Word::{Digital1, Digital2};
        match self {
            Button::Select => (Digital1, 0x01),
            Button::LeftThumb => (Digital1, 0x02),
            Button::RightThumb => (Digital1, 0x04),
            Button::Start => (Digital1, 0x08),
            Button::DPadUp => (Digital1, 0x10),
            Button::DPadRight => (Digital1, 0x20),
            Button::DPadDown => (Digital1, 0x40),
            Button::DPadLeft => (Digital1, 0x80),
            Button::LeftTrigger2 => (Digital2, 0x01),
            Button::RightTrigger2 => (Digital2, 0x02),
            Button::LeftTrigger => (Digital2, 0x04),
            Button::RightTrigger => (Digital2, 0x08),
            Button::North => (Digital2, 0x10),
            Button::East => (Digital2, 0x20),
            Button::South => (Digital2, 0x40),
            Button::West => (Digital2, 0x80),
        }
    }
}

/// A stick's two axes as a DualShock 3 reports them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(try_from = "RawStick")]
pub struct Stick {
    pub x: u8,
    pub y: u8,
}

impl Stick {
    /// Where a DualShock 3's stick rests, mid-range on both axes.
    pub const CENTRED: Stick = Stick { x: 128, y: 128 };

    pub fn is_centred(&self) -> bool {
        *self == Self::CENTRED
    }
}

impl Default for Stick {
    fn default() -> Self {
        Self::CENTRED
    }
}

/// A stick as written, wide enough that an axis past 0-255 is refused by name
/// rather than as a type error.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawStick {
    x: i64,
    y: i64,
}

impl TryFrom<RawStick> for Stick {
    type Error = ScriptError;

    fn try_from(raw: RawStick) -> Result<Self, ScriptError> {
        let axis = |value: i64| u8::try_from(value).map_err(|_| ScriptError::Axis { value });
        Ok(Stick {
            x: axis(raw.x)?,
            y: axis(raw.y)?,
        })
    }
}

/// What one pad holds: the buttons down (each at full pressure) and both
/// sticks. `()` in a script is a pad at rest.
#[derive(Debug, Clone, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct PadState {
    #[serde(skip_serializing_if = "BTreeSet::is_empty")]
    pub buttons: BTreeSet<Button>,
    #[serde(skip_serializing_if = "Stick::is_centred")]
    pub left: Stick,
    #[serde(skip_serializing_if = "Stick::is_centred")]
    pub right: Stick,
}

impl PadState {
    /// Each button down sets its `CELL_PAD_CTRL_*` bit in one of the two words;
    /// the sticks pass through.
    pub fn ds3(&self) -> Ds3Pad {
        let mut pad = Ds3Pad {
            digital1: 0,
            digital2: 0,
            left: self.left,
            right: self.right,
        };
        for button in &self.buttons {
            match button.ds3_bit() {
                (Word::Digital1, bit) => pad.digital1 |= bit,
                (Word::Digital2, bit) => pad.digital2 |= bit,
            }
        }
        pad
    }
}

/// A pad as a DualShock 3 reports it to the game (libpad's `CellPadData`): the
/// two digital words of `CELL_PAD_CTRL_*` bits and the four stick bytes. Every
/// bit has a position, so this and [`PadState`] convert both ways.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Ds3Pad {
    pub digital1: u8,
    pub digital2: u8,
    pub left: Stick,
    pub right: Stick,
}

impl Ds3Pad {
    /// What an untouched pad reports, and what O4 holds before a port's first entry
    /// when nobody is at the host's keyboard.
    pub const REST: Ds3Pad = Ds3Pad {
        digital1: 0,
        digital2: 0,
        left: Stick::CENTRED,
        right: Stick::CENTRED,
    };

    /// The positions behind the bits.
    pub fn state(&self) -> PadState {
        let buttons = Button::ALL
            .into_iter()
            .filter(|b| match b.ds3_bit() {
                (Word::Digital1, bit) => self.digital1 & bit != 0,
                (Word::Digital2, bit) => self.digital2 & bit != 0,
            })
            .collect();
        PadState {
            buttons,
            left: self.left,
            right: self.right,
        }
    }
}

/// From a flip offset on, these ports hold these states.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Step {
    pub at: u64,
    pub pads: Pads,
}

/// A step's ports and their states, ports 0 and 1 only, each at most once.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[serde(transparent)]
pub struct Pads(pub BTreeMap<u8, PadState>);

impl<'de> Deserialize<'de> for Pads {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct PadsVisitor;

        impl<'de> Visitor<'de> for PadsVisitor {
            type Value = Pads;

            fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
                f.write_str("a map from port (0 or 1) to a pad state")
            }

            fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Pads, A::Error> {
                let mut pads = BTreeMap::new();
                while let Some(port) = map.next_key::<i64>()? {
                    let port = check_port(port).map_err(de::Error::custom)?;
                    let state = map.next_value::<PadState>()?;
                    if pads.insert(port, state).is_some() {
                        return Err(de::Error::custom(ScriptError::PortTwice { port }));
                    }
                }
                Ok(Pads(pads))
            }
        }

        deserializer.deserialize_map(PadsVisitor)
    }
}

fn check_port(port: i64) -> Result<u8, ScriptError> {
    u8::try_from(port)
        .ok()
        .filter(|p| usize::from(*p) < PORTS)
        .ok_or(ScriptError::Port { port })
}

/// One port's state from an offset on: what a script compiles to per port.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    pub at: u64,
    pub pad: PadState,
}

/// A whole script. Steps may come in any order; a port may be named at most
/// once per offset, and a step names at least one port.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "RawScript", deny_unknown_fields)]
pub struct InputScript {
    steps: Vec<Step>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawScript {
    steps: Vec<Step>,
}

impl TryFrom<RawScript> for InputScript {
    type Error = ScriptError;

    fn try_from(raw: RawScript) -> Result<Self, ScriptError> {
        Self::new(raw.steps)
    }
}

/// The checks a script fails to load on: ranges, a port named twice for one
/// flip, an empty step, or RON that doesn't parse.
#[derive(Debug, thiserror::Error)]
pub enum ScriptError {
    #[error("axis {value} is outside 0-255")]
    Axis { value: i64 },
    #[error("port {port} is outside 0-{}", PORTS - 1)]
    Port { port: i64 },
    #[error("port {port} is named twice in one step")]
    PortTwice { port: u8 },
    #[error("port {port} has two steps at offset {at}")]
    TwoAtOnce { port: u8, at: u64 },
    #[error("the step at offset {at} names no port")]
    Empty { at: u64 },
    #[error("{0}")]
    Ron(#[from] ron::error::SpannedError),
}

impl InputScript {
    /// Checks `steps` as a parsed script is checked.
    pub fn new(steps: Vec<Step>) -> Result<Self, ScriptError> {
        let mut seen = BTreeSet::new();
        for step in &steps {
            if step.pads.0.is_empty() {
                return Err(ScriptError::Empty { at: step.at });
            }
            for &port in step.pads.0.keys() {
                check_port(i64::from(port))?;
                if !seen.insert((port, step.at)) {
                    return Err(ScriptError::TwoAtOnce { port, at: step.at });
                }
            }
        }
        Ok(Self { steps })
    }

    pub fn from_ron(text: &str) -> Result<Self, ScriptError> {
        Ok(ron::from_str(text)?)
    }

    /// The script as RON, one step a line, which [`InputScript::from_ron`]
    /// reads back to an equal script.
    pub fn to_ron(&self) -> String {
        let config = ron::ser::PrettyConfig::default().depth_limit(2);
        ron::ser::to_string_pretty(self, config).expect("a script always serializes")
    }

    pub fn steps(&self) -> &[Step] {
        &self.steps
    }

    /// Per port, its entries in offset order: what O4 takes, one port's schedule
    /// at a time.
    pub fn entries(&self) -> [Vec<Entry>; PORTS] {
        let mut ports: [Vec<Entry>; PORTS] = Default::default();
        for step in &self.steps {
            for (&port, pad) in &step.pads.0 {
                ports[usize::from(port)].push(Entry {
                    at: step.at,
                    pad: pad.clone(),
                });
            }
        }
        for entries in &mut ports {
            entries.sort_by_key(|e| e.at);
        }
        ports
    }

    /// What `port` holds at `offset`: its last entry at or before it, `None`
    /// before its first (the host's input).
    pub fn state_at(&self, port: usize, offset: u64) -> Option<PadState> {
        self.steps
            .iter()
            .filter(|s| s.at <= offset)
            .filter_map(|s| Some((s.at, s.pads.0.get(&u8::try_from(port).ok()?)?)))
            .max_by_key(|(at, _)| *at)
            .map(|(_, pad)| pad.clone())
    }

    /// The last offset any step names, 0 for an empty script.
    pub fn last_offset(&self) -> u64 {
        self.steps.iter().map(|s| s.at).max().unwrap_or(0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const WALK: &str = r#"
        // Both pads from the start: port 0 pushes forward, port 1 rests.
        (
            steps: [
                (at: 0, pads: {0: (left: (x: 128, y: 0)), 1: ()}),
                (at: 60, pads: {0: (buttons: [South], left: (x: 128, y: 0))}),
                (at: 64, pads: {0: (left: (x: 128, y: 0))}),
                (at: 120, pads: {0: (), 1: (buttons: [Start, DPadLeft], right: (x: 255, y: 7))}),
            ],
        )
    "#;

    fn pad(buttons: &[Button], left: (u8, u8), right: (u8, u8)) -> PadState {
        PadState {
            buttons: buttons.iter().copied().collect(),
            left: Stick {
                x: left.0,
                y: left.1,
            },
            right: Stick {
                x: right.0,
                y: right.1,
            },
        }
    }

    fn step(at: u64, pads: &[(u8, PadState)]) -> Step {
        Step {
            at,
            pads: Pads(pads.iter().cloned().collect()),
        }
    }

    #[test]
    fn a_script_parses_with_defaults_for_what_a_pad_leaves_out() {
        let script = InputScript::from_ron(WALK).unwrap();
        let forward = pad(&[], (128, 0), (128, 128));
        assert_eq!(
            script.steps(),
            &[
                step(0, &[(0, forward.clone()), (1, PadState::default())]),
                step(60, &[(0, pad(&[Button::South], (128, 0), (128, 128)))]),
                step(64, &[(0, forward)]),
                step(
                    120,
                    &[
                        (0, PadState::default()),
                        (
                            1,
                            pad(&[Button::Start, Button::DPadLeft], (128, 128), (255, 7))
                        )
                    ]
                ),
            ]
        );
        assert_eq!(PadState::default().ds3(), Ds3Pad::REST);
        assert_eq!(script.last_offset(), 120);
    }

    #[test]
    fn a_script_round_trips_through_ron() {
        let script = InputScript::from_ron(WALK).unwrap();
        let text = script.to_ron();
        assert_eq!(InputScript::from_ron(&text).unwrap(), script, "{text}");
        // What a pad leaves at rest stays out of the text.
        assert!(
            text.contains("(at: 0, pads: {0: (left: (x: 128, y: 0)), 1: ()})"),
            "{text}"
        );

        // Every button and every axis value survive the trip.
        let every = InputScript::new(vec![
            step(0, &[(0, pad(&Button::ALL, (0, 255), (1, 254)))]),
            step(1, &[(1, pad(&[Button::West], (127, 129), (128, 0)))]),
        ])
        .unwrap();
        assert_eq!(InputScript::from_ron(&every.to_ron()).unwrap(), every);
    }

    #[test]
    fn entries_sort_per_port_whatever_order_the_steps_come_in() {
        let a = pad(&[Button::North], (128, 128), (128, 128));
        let b = pad(&[Button::East], (128, 128), (128, 128));
        let c = pad(&[], (0, 128), (128, 128));
        let script = InputScript::new(vec![
            step(90, &[(0, c.clone())]),
            step(10, &[(1, a.clone()), (0, b.clone())]),
            step(30, &[(1, c.clone())]),
            step(5, &[(0, a.clone())]),
        ])
        .unwrap();
        let [p0, p1] = script.entries();
        let offsets = |e: &[Entry]| e.iter().map(|e| e.at).collect::<Vec<_>>();
        assert_eq!(offsets(&p0), [5, 10, 90]);
        assert_eq!(offsets(&p1), [10, 30]);
        assert_eq!(
            (p0[0].pad.clone(), p0[1].pad.clone()),
            (a.clone(), b.clone())
        );
        assert_eq!((p1[0].pad.clone(), p1[1].pad.clone()), (a, c.clone()));

        // A port holds its state until its next entry; before its first, nothing.
        assert_eq!(script.state_at(0, 4), None);
        assert_eq!(script.state_at(0, 9), script.state_at(0, 5));
        assert_eq!(script.state_at(0, 89), Some(b));
        assert_eq!(script.state_at(0, 10_000), Some(c.clone()));
        assert_eq!(script.state_at(1, 9), None);
        assert_eq!(script.state_at(1, 30), Some(c));
        assert_eq!(script.state_at(2, 30), None);
    }

    #[test]
    fn out_of_range_axes_and_ports_are_refused() {
        let refused = |text: &str| InputScript::from_ron(text).unwrap_err().to_string();
        for (text, says) in [
            (
                "(steps: [(at: 0, pads: {0: (left: (x: 256, y: 0))})])",
                "axis 256 is outside 0-255",
            ),
            (
                "(steps: [(at: 0, pads: {0: (right: (x: 0, y: -1))})])",
                "axis -1 is outside 0-255",
            ),
            ("(steps: [(at: 0, pads: {2: ()})])", "port 2 is outside 0-1"),
            (
                "(steps: [(at: 0, pads: {-1: ()})])",
                "port -1 is outside 0-1",
            ),
            (
                "(steps: [(at: 3, pads: {0: (), 0: ()})])",
                "port 0 is named twice in one step",
            ),
            (
                "(steps: [(at: 3, pads: {1: ()}), (at: 3, pads: {0: (), 1: ()})])",
                "port 1 has two steps at offset 3",
            ),
            (
                "(steps: [(at: 7, pads: {})])",
                "the step at offset 7 names no port",
            ),
            ("(steps: [(at: 0, pads: {0: (buttons: [Cross])})])", "Cross"),
            (
                "(steps: [(at: 0, pads: {0: (left: (x: 1, y: 2, z: 3))})])",
                "z",
            ),
            ("(steps: [(at: -1, pads: {0: ()})])", ""),
        ] {
            let err = refused(text);
            assert!(err.contains(says), "{text}: {err}");
        }
        // The same checks hold for a script built in code.
        assert!(matches!(
            InputScript::new(vec![step(0, &[(2, PadState::default())])]),
            Err(ScriptError::Port { port: 2 })
        ));
        assert!(matches!(
            InputScript::new(vec![step(4, &[])]),
            Err(ScriptError::Empty { at: 4 })
        ));
    }

    #[test]
    fn every_position_has_its_own_ds3_bit_and_converts_back() {
        let mut d1 = 0u8;
        let mut d2 = 0u8;
        for button in Button::ALL {
            let (word, bit) = button.ds3_bit();
            assert_eq!(bit.count_ones(), 1, "{button:?}");
            let into = match word {
                Word::Digital1 => &mut d1,
                Word::Digital2 => &mut d2,
            };
            assert_eq!(*into & bit, 0, "{button:?} shares a bit");
            *into |= bit;
        }
        assert_eq!((d1, d2), (0xff, 0xff), "16 positions fill both words");
        // The face buttons by position: South is CROSS's bit, East CIRCLE's.
        assert_eq!(Button::South.ds3_bit(), (Word::Digital2, 0x40));
        assert_eq!(Button::East.ds3_bit(), (Word::Digital2, 0x20));
        assert_eq!(Button::West.ds3_bit(), (Word::Digital2, 0x80));
        assert_eq!(Button::North.ds3_bit(), (Word::Digital2, 0x10));
        assert_eq!(Button::Start.ds3_bit(), (Word::Digital1, 0x08));

        let state = pad(
            &[Button::South, Button::DPadLeft, Button::RightTrigger2],
            (3, 4),
            (5, 6),
        );
        let ds3 = state.ds3();
        assert_eq!((ds3.digital1, ds3.digital2), (0x80, 0x42));
        assert_eq!(ds3.state(), state);
        let all = Ds3Pad {
            digital1: 0xff,
            digital2: 0xff,
            ..Ds3Pad::REST
        };
        assert_eq!(all.state().buttons.len(), 16);
    }
}
