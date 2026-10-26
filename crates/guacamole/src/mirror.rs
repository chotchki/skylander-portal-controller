//! `/api/_test/tv` (design decision 8): a `PostUpdate` copy of what the TV
//! draws, so the e2e suite asserts on the TV without screenshots. Mounted
//! only under `test-hooks`. Read off the laid-out UI tree: the players line,
//! join address, join QR and every card's lines, portrait, pip and rect.
//! Copied from the resources that feed the tree: the screen, game, message,
//! Setup's lists, each card's figure and owner ids, and the intro queue. The
//! focused choice is `InputFocus`'s entity's `FocusId`. The turntables'
//! warm (AE.14), the level's load (AG.19), the start's pass (AE.25), the
//! pads as the level's layer reads them (AG.20) and the walk's mode and
//! hero (AG.21) and his locomotion (AG.30) ride along,
//! `/api/_test/tv/frames` hands out the TV's frame times for the stall check,
//! `/api/_test/tv/gaits` his locomotion step by step (AG.30),
//! `/api/_test/tv/drawn` the walk frame by frame as drawn (AG.55),
//! `/api/_test/tv/scripts` the scripts' run step by step (AH.24),
//! `/api/_test/tv/pad` drives pads as a hand would, no hardware needed, and
//! `/api/_test/tv/camera` pins the level's camera where an oracle frame's
//! stood, so a shot of ours lines up with it (AG.79).

use std::collections::VecDeque;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use axum::extract::rejection::JsonRejection;
use bevy::camera::Projection;
use bevy::input::InputSystems;
use bevy::input::gamepad::{
    GamepadButton, GamepadConnection, GamepadConnectionEvent, RawGamepadAxisChangedEvent,
    RawGamepadButtonChangedEvent, RawGamepadEvent,
};
use bevy::prelude::*;
use bevy::ui::UiSystems;
use serde::{Deserialize, Serialize};

use crate::alert::{AlertShot, AlertShots};
use crate::creatures::{CreatureShots, CreaturesReport};
use crate::defeat::{DefeatShot, DefeatShots};
use crate::dev_entry::{DevEntry, EntryShot};
use crate::extraction::{ExtractionShot, ExtractionShots};
use crate::hero::{FollowCamera, HeroBody, HeroGaits, HeroInput};
use crate::hero_scripts::{LootShot, LootShots, ScriptLog, ScriptStep};
use crate::hud::{HudShot, HudShots};
use crate::input::{InputShot, InputShots, PostedPresses, TestPress};
use crate::intro::{IntroEvent, IntroQueue, QueuedShot};
use crate::intro_player::{self, IntroShot, IntroShots};
use crate::level::{Level, LevelReport};
use crate::play::{HeroReport, Play, PlayShots};
use crate::prepare::{Prepare, PrepareReport};
use crate::tv::{CardShot, ScreenShots};
use crate::warm::{PipelineWatch, Warm, WarmReport};
use guacamole::frame::{self, Frame};

/// What the TV shows this frame.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct TvSnapshot {
    /// `TvScreen`'s variant: `Join`, `Portal`, `NoGiants`, ...
    pub screen: String,
    pub game: Option<String>,
    /// ServerError's message.
    pub message: Option<String>,
    /// The count in the drawn players line; none when it isn't drawn.
    pub players: Option<usize>,
    /// The drawn join address.
    pub join_address: Option<String>,
    /// The join QR image is drawn.
    pub join_qr: bool,
    /// Setup's lists (the resource, not the drawn text), for Setup, NoGiants and Found.
    pub found: Vec<String>,
    pub folders: Vec<String>,
    pub problems: Vec<String>,
    /// The focused choice's id (`choose-folder`, `continue`, `close`); none
    /// when no choice has the focus.
    pub focus: Option<String>,
    /// One per card drawn, in slot order.
    pub cards: Vec<CardShot>,
    /// Slots drawn as empty frames, 1-based.
    pub empty_slots: Vec<u8>,
    /// Physical window size the layout below is in.
    pub window: [f32; 2],
    /// Cards not wholly inside the inset (TV) or margin (desktop), 1-based.
    pub cards_outside: Vec<u8>,
    /// Pairs of cards that overlap.
    pub overlapping: usize,
    /// The first-run extraction (AB.9).
    pub extraction: ExtractionShot,
    /// The intro playing over the portal view (AB.10).
    pub intro: Option<IntroShot>,
    /// Intros waiting, in play order.
    pub intro_queue: Vec<QueuedShot>,
    /// The turntables' warm (AE.14); none without turntables.
    pub warm: Option<WarmReport>,
    /// The level's load (AG.19); none without the loader.
    pub level: Option<LevelReport>,
    /// The start's pass (AE.25); none without it.
    pub prepare: Option<PrepareReport>,
    /// The pads and the level's layer (AG.20).
    pub input: InputShot,
    /// Where the TV is (AG.21): `portal` (its own screens), `level`,
    /// `paused` (the level waiting, Start's or for a pad) or `held` (the
    /// portal holding it: an alert, or no hero in it; AG.29).
    pub mode: String,
    /// The hero, once a figure that walks is placed: who, his model's way
    /// to the level and, in it, where he stands and faces.
    pub hero: Option<HeroReport>,
    /// The characters kept on their cards (AG.27), each with why: the
    /// extract left his scripts out, they ask what the TV can't answer, or
    /// they didn't run through in the level (those with their figure's
    /// `slot`, which the e2e harness holds as a hero's scripts gone off,
    /// openspec AH.166).
    pub kept: Vec<guacamole::walkers::Kept>,
    /// The portal's alert as drawn (AG.29, AG.63, AG.73).
    pub alert: Option<AlertShot>,
    /// The small join code drawn in the corner over the level or an alert
    /// (AG.29); `join_qr` counts its QR too.
    pub join_corner: bool,
    /// Where each kind of breakable's loot count stands (openspec AH.27),
    /// once the level's loot data is made; none before the scripts run.
    pub loot: Option<LootShot>,
    /// The creatures the scripts walk, with their health, what the fight
    /// woke past its own and what the scripts spawned that's still on
    /// (openspec AH.25); none before the scripts run.
    pub creatures: Option<CreaturesReport>,
    /// The dev entry (AH.25): where it's got to, F0's step and the
    /// equivalence list read there; none unless one was asked.
    pub entry: Option<EntryShot>,
    /// The HUD (AH.26): the hero's health, level, experience and gold as
    /// shown and drawn, and a bite's numbers; none while no hero is in the
    /// level.
    pub hud: Option<HudShot>,
    /// A defeat (AH.26): the hero dying, the figures beaten in the level
    /// and Restart chapter as drawn; none with nothing to report.
    pub defeat: Option<DefeatShot>,
}

/// Shared with the routes: the last snapshot, skips asked for over HTTP,
/// presses posted for the input router, the frame log, the posted pads,
/// the gait log, the drawn log, the scripts' log, the camera pin and the
/// shots asked for.
#[derive(Resource, Clone, Default)]
pub struct TvMirror(
    pub Arc<Mutex<TvSnapshot>>,
    pub Arc<AtomicUsize>,
    pub(crate) PostedPresses,
    pub(crate) Arc<Mutex<FrameLog>>,
    pub(crate) PostedPads,
    pub(crate) Arc<Mutex<GaitLog>>,
    pub(crate) Arc<Mutex<DrawnLog>>,
    pub(crate) ScriptLog,
    pub(crate) Arc<Mutex<Option<CameraPin>>>,
    pub(crate) AskedShots,
);

/// Shots asked for by label (`/api/_test/tv/shot`), oldest first, taken
/// at once on the next frames, one a frame.
#[derive(Resource, Clone, Default)]
pub(crate) struct AskedShots(pub(crate) Arc<Mutex<Vec<String>>>);

impl AskedShots {
    /// Queues `label` when it names a file safely: 1 to 40 characters,
    /// letters, digits, `-` and `_`.
    pub(crate) fn ask(&self, label: &str) -> bool {
        let ok = (1..=40).contains(&label.len())
            && label
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_');
        if ok {
            self.0
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .push(label.to_owned());
        }
        ok
    }

    #[cfg_attr(not(feature = "dev-tools"), allow(dead_code))]
    pub(crate) fn take(&self) -> Option<String> {
        let mut asked = self.0.lock().unwrap_or_else(|e| e.into_inner());
        (!asked.is_empty()).then(|| asked.remove(0))
    }
}

/// Where `/api/_test/tv/camera` holds the level's camera (AG.79): an oracle
/// frame's viewport as O5 reads it, its eye in metres in the level's frame
/// and its tilt and heading in degrees (a `tfbEulerTransform`'s, as
/// [`guacamole::level_scene::facing`] takes them), and the picture's height in
/// degrees when given (else the follow camera's own).
#[derive(Debug, Clone, Copy, PartialEq, Deserialize)]
pub(crate) struct CameraPin {
    pub(crate) eye: [f32; 3],
    pub(crate) tilt: f32,
    pub(crate) heading: f32,
    #[serde(default)]
    pub(crate) fov_y: Option<f32>,
}

impl CameraPin {
    /// A pin that can be drawn from: every number finite, the picture's
    /// height inside (0, 180).
    fn valid(&self) -> bool {
        self.eye
            .iter()
            .chain([&self.tilt, &self.heading])
            .all(|v| v.is_finite())
            && self.fov_y.is_none_or(|f| f > 0.0 && f < 180.0)
    }

    /// The camera's drawn transform there, unrolled.
    pub(crate) fn transform(&self) -> Transform {
        Transform::from_translation(Vec3::from_array(self.eye)).looking_to(
            guacamole::level_scene::facing([self.tilt, self.heading, 0.0]),
            Vec3::Y,
        )
    }
}

/// `POST /api/_test/tv/shot`'s body.
#[derive(Deserialize)]
struct ShotBody {
    label: String,
}

/// `POST /api/_test/tv/camera`'s body: a pin, or `{"off": true}`.
#[derive(Deserialize)]
#[serde(untagged)]
enum CameraBody {
    Off { off: bool },
    Pin(CameraPin),
}

impl CameraBody {
    /// The pin it asks for (`Some(None)` lets the camera follow again);
    /// `None` for a body that's neither.
    fn pin(self) -> Option<Option<CameraPin>> {
        match self {
            Self::Off { off: true } => Some(None),
            Self::Off { off: false } => None,
            Self::Pin(p) => p.valid().then_some(Some(p)),
        }
    }
}

/// The level's camera where the pin holds it, after its follow eased it
/// and before transforms propagate, so the frame draws from the pin; the
/// follow itself steps on, untouched, and takes over once the pin's off.
fn pin_camera(
    mirror: Res<TvMirror>,
    mut cameras: Query<(&mut Transform, &mut Projection), With<FollowCamera>>,
) {
    let Some(pin) = *mirror.8.lock().unwrap_or_else(|e| e.into_inner()) else {
        return;
    };
    for (mut transform, mut projection) in &mut cameras {
        *transform = pin.transform();
        if let (Some(fov), Projection::Perspective(p)) = (pin.fov_y, &mut *projection) {
            p.fov = fov.to_radians();
        }
    }
}

/// Pads `/api/_test/tv/pad` drives, 0 to 3.
pub(crate) const TEST_PADS: usize = 4;

/// What a posted pad does.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum PadAct {
    Connect {
        name: String,
        vendor: Option<u16>,
        product: Option<u16>,
    },
    Disconnect,
    /// The left stick held there, raw, until the next.
    Stick(Vec2),
    /// The right stick, the same way.
    RightStick(Vec2),
    /// Down for a frame, then up.
    Press(GamepadButton),
    /// Down until released (AH.24: a held attack).
    Hold(GamepadButton),
    Release(GamepadButton),
    /// Down a game step, then up a game step, this many times: a press
    /// every other frame as the oracle's O4 schedules it (AH.9's `lobs`).
    Pulse(GamepadButton, u32),
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct PadPost {
    pub(crate) pad: usize,
    pub(crate) act: PadAct,
}

/// Pad acts posted over HTTP, waiting for [`drive_posted_pads`].
#[derive(Clone, Default)]
pub(crate) struct PostedPads(Arc<Mutex<VecDeque<PadPost>>>);

impl PostedPads {
    fn push(&self, post: PadPost) {
        let mut queue = self.0.lock().unwrap_or_else(|e| e.into_inner());
        if queue.len() == POSTED_PADS_MAX {
            queue.pop_front();
        }
        queue.push_back(post);
    }

    fn pop(&self) -> Option<PadPost> {
        self.0.lock().unwrap_or_else(|e| e.into_inner()).pop_front()
    }
}

/// Pad acts waiting; more than this and the oldest go.
const POSTED_PADS_MAX: usize = 256;

/// A button by the name `/api/_test/tv/pad` takes.
fn pad_button(name: &str) -> Option<GamepadButton> {
    Some(match name {
        "south" => GamepadButton::South,
        "east" => GamepadButton::East,
        "west" => GamepadButton::West,
        "north" => GamepadButton::North,
        "start" => GamepadButton::Start,
        "select" => GamepadButton::Select,
        "mode" => GamepadButton::Mode,
        "up" => GamepadButton::DPadUp,
        "down" => GamepadButton::DPadDown,
        "left" => GamepadButton::DPadLeft,
        "right" => GamepadButton::DPadRight,
        "l1" => GamepadButton::LeftTrigger,
        "r1" => GamepadButton::RightTrigger,
        "l2" => GamepadButton::LeftTrigger2,
        "r2" => GamepadButton::RightTrigger2,
        _ => return None,
    })
}

/// `{"button": .., "presses": ..}`: a pulse's button and how many presses.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PulseBody {
    button: String,
    presses: u32,
}

/// `POST /api/_test/tv/pad`'s body: a pad and one act.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PadBody {
    pad: usize,
    connect: Option<ConnectBody>,
    #[serde(default)]
    disconnect: bool,
    stick: Option<[f32; 2]>,
    /// The right stick, raw, held until the next (AG.28's verify: the
    /// original's does nothing to the view, and the TV's reads none).
    rstick: Option<[f32; 2]>,
    press: Option<String>,
    hold: Option<String>,
    release: Option<String>,
    pulse: Option<PulseBody>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ConnectBody {
    name: Option<String>,
    vendor: Option<u16>,
    product: Option<u16>,
}

impl PadBody {
    /// The act, if the body names exactly one and it's one a pad can do.
    fn post(self) -> Option<PadPost> {
        if self.pad >= TEST_PADS {
            return None;
        }
        let acts = [
            self.connect.is_some(),
            self.disconnect,
            self.stick.is_some(),
            self.rstick.is_some(),
            self.press.is_some(),
            self.hold.is_some(),
            self.release.is_some(),
            self.pulse.is_some(),
        ];
        if acts.iter().filter(|a| **a).count() != 1 {
            return None;
        }
        let act = if let Some(c) = self.connect {
            PadAct::Connect {
                name: c.name.unwrap_or_else(|| "guacamole test pad".into()),
                vendor: c.vendor,
                product: c.product,
            }
        } else if self.disconnect {
            PadAct::Disconnect
        } else if let Some([x, y]) = self.stick {
            let travel = |v: f32| v.is_finite() && (-1.0..=1.0).contains(&v);
            if !(travel(x) && travel(y)) {
                return None;
            }
            PadAct::Stick(Vec2::new(x, y))
        } else if let Some([x, y]) = self.rstick {
            let travel = |v: f32| v.is_finite() && (-1.0..=1.0).contains(&v);
            if !(travel(x) && travel(y)) {
                return None;
            }
            PadAct::RightStick(Vec2::new(x, y))
        } else if let Some(b) = self.hold {
            PadAct::Hold(pad_button(&b)?)
        } else if let Some(b) = self.release {
            PadAct::Release(pad_button(&b)?)
        } else if let Some(p) = self.pulse {
            if p.presses == 0 || p.presses > MAX_PULSES {
                return None;
            }
            PadAct::Pulse(pad_button(&p.button)?, p.presses)
        } else {
            PadAct::Press(pad_button(self.press.as_deref()?)?)
        };
        Some(PadPost { pad: self.pad, act })
    }
}

/// The most presses one pulse takes: a minute of them.
const MAX_PULSES: u32 = 1_800;

/// Game steps run, counted for the pads' pulses.
#[derive(Resource, Debug, Default)]
pub(crate) struct PadSteps(u64);

pub(crate) fn count_pad_steps(mut steps: ResMut<PadSteps>) {
    steps.0 += 1;
}

/// A pulse under way: its pad and button, the presses left, whether the
/// button's down and the game step it last changed at.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Pulse {
    pad: Entity,
    button: GamepadButton,
    left: u32,
    down: bool,
    at: u64,
}

impl Pulse {
    /// This frame's change, if any: a game step has read the last one, so
    /// the button goes the other way; `None` once done and up.
    fn turn(&mut self, steps: u64) -> Option<f32> {
        if steps <= self.at {
            return None;
        }
        if self.down {
            self.down = false;
            self.at = steps;
            return Some(0.0);
        }
        if self.left == 0 {
            return None;
        }
        self.left -= 1;
        self.down = true;
        self.at = steps;
        Some(1.0)
    }

    fn done(&self) -> bool {
        self.left == 0 && !self.down
    }
}

/// The posted pads, as gilrs would send them: a connection, the stick's two
/// axes, a press down for a frame and up the next (nothing else that frame,
/// so the next act waits behind it), through Bevy's own gamepad systems
/// and so through each pad's settings. A pad's entity outlives its
/// disconnection, as bevy_gilrs keeps a real one's.
fn drive_posted_pads(
    mut commands: Commands,
    mirror: Res<TvMirror>,
    steps: Res<PadSteps>,
    mut pads: Local<[Option<Entity>; TEST_PADS]>,
    mut up: Local<Vec<(Entity, GamepadButton)>>,
    mut pulses: Local<Vec<Pulse>>,
    mut connections: MessageWriter<GamepadConnectionEvent>,
    mut raw: MessageWriter<RawGamepadEvent>,
) {
    for pulse in pulses.iter_mut() {
        if let Some(value) = pulse.turn(steps.0) {
            raw.write(RawGamepadButtonChangedEvent::new(pulse.pad, pulse.button, value).into());
        }
    }
    pulses.retain(|p| !p.done());
    if !up.is_empty() {
        for (pad, button) in up.drain(..) {
            raw.write(RawGamepadButtonChangedEvent::new(pad, button, 0.0).into());
        }
        return;
    }
    while let Some(PadPost { pad: slot, act }) = mirror.4.pop() {
        if let PadAct::Connect {
            name,
            vendor,
            product,
        } = act
        {
            let pad = *pads[slot].get_or_insert_with(|| commands.spawn_empty().id());
            info!(slot, ?pad, %name, ?vendor, ?product, "test pad connects");
            let event = GamepadConnectionEvent::new(
                pad,
                GamepadConnection::Connected {
                    name,
                    vendor_id: vendor,
                    product_id: product,
                },
            );
            raw.write(event.clone().into());
            connections.write(event);
            continue;
        }
        let Some(pad) = pads[slot] else {
            warn!(slot, ?act, "test pad never connected: ignored");
            continue;
        };
        match act {
            PadAct::Connect { .. } => unreachable!("handled above"),
            PadAct::Disconnect => {
                info!(slot, ?pad, "test pad disconnects");
                let event = GamepadConnectionEvent::new(pad, GamepadConnection::Disconnected);
                raw.write(event.clone().into());
                connections.write(event);
            }
            PadAct::Stick(v) => {
                for (axis, value) in [
                    (GamepadAxis::LeftStickX, v.x),
                    (GamepadAxis::LeftStickY, v.y),
                ] {
                    raw.write(RawGamepadAxisChangedEvent::new(pad, axis, value).into());
                }
            }
            PadAct::RightStick(v) => {
                for (axis, value) in [
                    (GamepadAxis::RightStickX, v.x),
                    (GamepadAxis::RightStickY, v.y),
                ] {
                    raw.write(RawGamepadAxisChangedEvent::new(pad, axis, value).into());
                }
            }
            PadAct::Press(button) => {
                debug!(slot, ?button, "test pad press");
                raw.write(RawGamepadButtonChangedEvent::new(pad, button, 1.0).into());
                up.push((pad, button));
                return;
            }
            PadAct::Hold(button) => {
                debug!(slot, ?button, "test pad holds");
                raw.write(RawGamepadButtonChangedEvent::new(pad, button, 1.0).into());
            }
            PadAct::Release(button) => {
                debug!(slot, ?button, "test pad lets go");
                raw.write(RawGamepadButtonChangedEvent::new(pad, button, 0.0).into());
            }
            PadAct::Pulse(button, presses) => {
                info!(
                    slot,
                    ?button,
                    presses,
                    "test pad pulses a press every other game step"
                );
                pulses.push(Pulse {
                    pad,
                    button,
                    left: presses,
                    down: false,
                    // Its first press waits for the next step.
                    at: steps.0,
                });
            }
        }
    }
}

/// Frames the log keeps: over half an hour at 60 a second.
const FRAMES_KEPT: usize = 120_000;

/// The TV's frames, oldest first, for the stall check (AE.14): each one's
/// end and length in milliseconds of `Time<Real>` (the interval the render
/// world's frames finish at, so a frame that stalls the TV is a long one)
/// and the render pipelines compiled since the frame before.
#[derive(Debug, Default)]
pub(crate) struct FrameLog {
    frames: VecDeque<[f64; 3]>,
}

impl FrameLog {
    fn push(&mut self, frame: [f64; 3]) {
        if self.frames.len() == FRAMES_KEPT {
            self.frames.pop_front();
        }
        self.frames.push_back(frame);
    }

    /// Frames that ended after `since_ms`, and the last one's end.
    fn since(&self, since_ms: f64) -> FramesBody {
        FramesBody {
            now_ms: self.frames.back().map_or(0.0, |f| f[0]),
            frames: self
                .frames
                .iter()
                .filter(|f| f[0] > since_ms)
                .copied()
                .collect(),
        }
    }
}

/// `GET /api/_test/tv/frames`' body.
#[derive(Debug, Serialize, PartialEq)]
struct FramesBody {
    now_ms: f64,
    /// `[ended_ms, took_ms, pipelines_compiled]` each.
    frames: Vec<[f64; 3]>,
}

#[derive(Deserialize)]
struct FramesQuery {
    since_ms: Option<f64>,
}

/// One game step of the hero's locomotion (AG.30), as the step left it.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub(crate) struct GaitStep {
    /// Steps since he came in, from 1.
    pub step: u64,
    /// The fixed clock at the step, seconds.
    pub time: f64,
    /// The walking pad's stick as the step read it, raw, and its strength
    /// past the dead zone.
    pub stick: [f32; 2],
    pub strength: f32,
    /// The speed the step read (his walk after its push, before its
    /// slowing), ft/s.
    pub speed_ft: f32,
    pub run_share: f32,
    /// Idle, walk, run and fall.
    pub weights: [f32; 4],
    /// The walk's and the run's rates.
    pub rates: [f32; 2],
    pub clip: &'static str,
    pub air: &'static str,
    /// The stand/move switch's side after the step.
    pub moving: bool,
    /// The step's edge probe held him, so the speed it read is his walk's
    /// vertical part alone (AG.58).
    pub held: bool,
    /// The way he faces as the step left him, degrees in [0, 360): his
    /// scripts' heading on his scripts, the controller's on the
    /// stand-in (stand-in ledger: `walker`); what `hero.heading` reports
    /// (openspec AG.188).
    pub facing: f32,
}

/// The hero's locomotion step by step, oldest first; a hero coming in
/// starts it again.
#[derive(Debug, Default)]
pub(crate) struct GaitLog {
    steps: VecDeque<GaitStep>,
}

impl GaitLog {
    fn push(&mut self, step: GaitStep) {
        if step.step == 1 {
            self.steps.clear();
        }
        if self.steps.len() == FRAMES_KEPT {
            self.steps.pop_front();
        }
        self.steps.push_back(step);
    }

    /// Steps after `since`, and the last one's number.
    fn since(&self, since: u64) -> GaitsBody {
        GaitsBody {
            now: self.steps.back().map_or(0, |s| s.step),
            steps: self
                .steps
                .iter()
                .filter(|s| s.step > since)
                .cloned()
                .collect(),
        }
    }
}

/// `GET /api/_test/tv/gaits`' body.
#[derive(Debug, Serialize, PartialEq)]
struct GaitsBody {
    now: u64,
    steps: Vec<GaitStep>,
}

#[derive(Deserialize)]
struct GaitsQuery {
    since: Option<u64>,
}

/// Each hero's locomotion step into the gait log, after it ran.
pub(crate) fn log_gaits(
    time: Res<Time>,
    mirror: Res<TvMirror>,
    input: Option<Res<HeroInput>>,
    heroes: Query<(&HeroGaits, &HeroBody)>,
) {
    let Some((g, body)) = heroes.iter().next() else {
        return;
    };
    // His scripts' picks while they pick his clips (openspec AH.49), else
    // AG.30's (stand-in ledger: `locomotion`).
    let r = crate::play::GaitReport::of_gaits(g);
    let stick = input.map_or(guac_motion::stick::Stick::CENTRED, |i| i.0.stick);
    mirror
        .5
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .push(GaitStep {
            step: g.steps,
            time: time.elapsed_secs_f64(),
            stick: [stick.x, stick.y],
            strength: stick.push(0.0).strength,
            speed_ft: r.speed_ft,
            run_share: r.run_share,
            weights: r.weights,
            rates: r.rates,
            clip: match &g.picked {
                Some(p) => p.top(),
                None => g.locomotion.top().name(),
            },
            air: match &g.picked {
                Some(p) => p.air(),
                None => g.locomotion.air().name(),
            },
            moving: r.moving,
            held: r.held,
            facing: crate::hero::heading(&body.hero),
        });
}

/// One drawn frame of the walk (AG.55), as `Last` finds it: the frame's
/// clocks, the fixed steps run so far and the overstep the drawing eased
/// by, the hero's drawn transform and his step's pivot, the camera's drawn
/// transform, each clip's drawn time and weight and how far his joints
/// moved since the frame before.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub(crate) struct DrawnFrame {
    /// Bevy's frame count.
    pub frame: u32,
    /// `Time<Real>` at the frame, and the virtual clock's delta, ms.
    pub real_ms: f64,
    pub delta_ms: f64,
    /// The main world's work on the frame: from its clock's update to
    /// `Last`, ms.
    pub work_ms: f64,
    /// When `Last` ran, ms on the log's own clock (the presents' clock).
    pub at_ms: f64,
    /// His locomotion's steps since he came in.
    pub steps: u64,
    /// `Time<Fixed>`'s overstep fraction.
    pub overstep: f32,
    /// Translation, then rotation x, y, z, w.
    pub hero: [f32; 7],
    /// His pivot as his last step left it.
    pub pivot: [f32; 3],
    pub camera: [f32; 7],
    /// Idle, walk, run and fall: time in seconds and weight.
    pub clips: [[f64; 2]; 4],
    /// Over every joint, metres moved plus radians turned since the frame
    /// before.
    pub pose_moved: f32,
    /// Cameras drawing this frame.
    pub cameras: usize,
    /// Character materials changed since the frame before (each one is
    /// prepared again for the GPU).
    pub materials_changed: usize,
}

/// The walk's drawn frames, oldest first, and when the render world began
/// and finished each frame (its presents scheduled by the finish), on the
/// log's own clock.
#[derive(Debug)]
pub(crate) struct DrawnLog {
    frames: VecDeque<DrawnFrame>,
    /// The render world's frames: began and finished, ms.
    presents: VecDeque<[f64; 2]>,
    base: std::time::Instant,
}

impl Default for DrawnLog {
    fn default() -> Self {
        Self {
            frames: VecDeque::new(),
            presents: VecDeque::new(),
            base: std::time::Instant::now(),
        }
    }
}

impl DrawnLog {
    fn ms(&self, at: std::time::Instant) -> f64 {
        at.saturating_duration_since(self.base).as_secs_f64() * 1e3
    }

    fn present(&mut self, began: std::time::Instant, at: std::time::Instant) {
        if self.presents.len() == FRAMES_KEPT {
            self.presents.pop_front();
        }
        let span = [self.ms(began), self.ms(at)];
        self.presents.push_back(span);
    }

    fn push(&mut self, frame: DrawnFrame) {
        if self.frames.len() == FRAMES_KEPT {
            self.frames.pop_front();
        }
        self.frames.push_back(frame);
    }

    fn since(&self, since: u32) -> DrawnBody {
        let frames: Vec<DrawnFrame> = self
            .frames
            .iter()
            .filter(|f| f.frame > since)
            .cloned()
            .collect();
        let from = frames.first().map_or(f64::INFINITY, |f| f.at_ms);
        DrawnBody {
            presents: self
                .presents
                .iter()
                .filter(|p| p[1] >= from)
                .copied()
                .collect(),
            frames,
        }
    }
}

/// `GET /api/_test/tv/drawn`' body.
#[derive(Debug, Serialize, PartialEq)]
struct DrawnBody {
    frames: Vec<DrawnFrame>,
    /// From the first frame's `at_ms` on: when each render frame began
    /// and finished.
    presents: Vec<[f64; 2]>,
}

/// The drawn log in the render world, and when its frame began.
#[derive(Resource)]
struct PresentLog(Arc<Mutex<DrawnLog>>, Option<std::time::Instant>);

/// When the render world began this frame.
fn log_render_began(mut log: ResMut<PresentLog>) {
    log.1 = Some(std::time::Instant::now());
}

/// When the render world finished this frame, into the drawn log.
fn log_present(log: Res<PresentLog>) {
    let now = std::time::Instant::now();
    let began = log.1.unwrap_or(now);
    log.0
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .present(began, now);
}

#[derive(Deserialize)]
struct DrawnQuery {
    since: Option<u32>,
}

/// A transform as its translation, then its rotation's x, y, z and w.
fn seven(t: &Transform) -> [f32; 7] {
    let [x, y, z] = t.translation.to_array();
    let [a, b, c, d] = t.rotation.to_array();
    [x, y, z, a, b, c, d]
}

/// This frame of the walk into the drawn log, while the level shows.
fn log_drawn(
    count: Res<bevy::diagnostic::FrameCount>,
    real: Res<Time<Real>>,
    time: Res<Time>,
    fixed: Res<Time<Fixed>>,
    play: Option<Res<Play>>,
    reduced: Option<Res<crate::motion::ReducedMotion>>,
    mirror: Res<TvMirror>,
    (bodies, transforms, cameras): (
        Query<(&HeroBody, &HeroGaits)>,
        Query<&Transform>,
        Query<&Camera>,
    ),
    mut before: Local<Vec<Transform>>,
    changed: Option<MessageReader<AssetEvent<guacamole::character::material::CharacterMaterial>>>,
) {
    let materials_changed = changed.map_or(0, |mut c| {
        c.read()
            .filter(|e| matches!(e, AssetEvent::Modified { .. }))
            .count()
    });
    let Some((hero, camera, joints)) = play.as_ref().and_then(|p| p.drawn()) else {
        before.clear();
        return;
    };
    let (Ok((body, gaits)), Ok(drawn), Ok(view)) = (
        bodies.get(hero),
        transforms.get(hero),
        transforms.get(camera),
    ) else {
        return;
    };
    let held = reduced.is_some_and(|r| r.0);
    let clips = crate::play::drawn_clips(
        gaits,
        (fixed.overstep_fraction(), fixed.timestep().as_secs_f64()),
        held,
    );
    let pose: Vec<Transform> = joints
        .iter()
        .filter_map(|j| transforms.get(*j).ok().copied())
        .collect();
    let pose_moved = if before.len() == pose.len() {
        before
            .iter()
            .zip(&pose)
            .map(|(a, b)| {
                a.translation.distance(b.translation) + a.rotation.angle_between(b.rotation)
            })
            .sum()
    } else {
        f32::NAN
    };
    *before = pose;
    let now = std::time::Instant::now();
    let work_ms = real.last_update().map_or(f64::NAN, |at| {
        now.saturating_duration_since(at).as_secs_f64() * 1e3
    });
    let mut log = mirror.6.lock().unwrap_or_else(|e| e.into_inner());
    let at_ms = log.ms(now);
    log.push(DrawnFrame {
        frame: count.0,
        real_ms: real.elapsed().as_secs_f64() * 1e3,
        delta_ms: time.delta().as_secs_f64() * 1e3,
        work_ms,
        at_ms,
        steps: gaits.steps,
        overstep: fixed.overstep_fraction(),
        hero: seven(drawn),
        pivot: body.hero.pivot.to_array(),
        camera: seven(view),
        clips: clips.map(|(at, w)| [at, f64::from(w)]),
        pose_moved,
        cameras: cameras.iter().filter(|c| c.is_active).count(),
        materials_changed,
    });
}

pub struct Mirror;

impl Plugin for Mirror {
    fn build(&self, app: &mut App) {
        frame::ensure(app);
        app.add_systems(PostUpdate, mirror_tv.after(UiSystems::Layout))
            .add_systems(Update, skip_asked.before(intro_player::play_intros))
            .init_resource::<PadSteps>()
            .add_systems(PreUpdate, drive_posted_pads.before(InputSystems))
            .add_systems(FixedUpdate, log_gaits.in_set(Frame::Record))
            .add_systems(FixedUpdate, count_pad_steps.in_set(Frame::Pads))
            .add_systems(
                PostUpdate,
                pin_camera.before(bevy::transform::TransformSystems::Propagate),
            )
            .add_systems(Last, (log_frame, log_drawn));
        // The router takes the presses posted to it (cut 2: it names no mirror),
        // the scripts' run logs where the routes read, and the shots plugin
        // takes the shots they ask for.
        if let Some(presses) = app.world().get_resource::<TvMirror>().map(|m| m.2.clone()) {
            app.insert_resource(presses);
        }
        if let Some(scripts) = app.world().get_resource::<TvMirror>().map(|m| m.7.clone()) {
            app.insert_resource(scripts);
        }
        if let Some(asked) = app.world().get_resource::<TvMirror>().map(|m| m.9.clone()) {
            app.insert_resource(asked);
        }
        let log = app.world().get_resource::<TvMirror>().map(|m| m.6.clone());
        if let (Some(log), Some(render)) = (log, app.get_sub_app_mut(bevy::render::RenderApp)) {
            render
                .insert_resource(PresentLog(log, None))
                .add_systems(
                    bevy::render::Render,
                    log_render_began.in_set(bevy::render::RenderSystems::ExtractCommands),
                )
                .add_systems(
                    bevy::render::Render,
                    log_present.in_set(bevy::render::RenderSystems::Cleanup),
                );
        }
    }
}

/// This frame into the log, with the pipelines compiled since the last.
fn log_frame(
    time: Res<Time<Real>>,
    mirror: Res<TvMirror>,
    watch: Option<Res<PipelineWatch>>,
    mut compiled_before: Local<Option<usize>>,
) {
    let compiled = watch.map(|w| w.ready());
    let new = match (compiled, *compiled_before) {
        (Some(now), Some(before)) => now.saturating_sub(before),
        _ => 0,
    };
    *compiled_before = compiled.or(*compiled_before);
    let ms = |d: std::time::Duration| d.as_secs_f64() * 1e3;
    mirror.3.lock().unwrap_or_else(|e| e.into_inner()).push([
        ms(time.elapsed()),
        ms(time.delta()),
        new as f64,
    ]);
}

#[derive(Deserialize)]
struct ScriptsQuery {
    since: Option<u64>,
}

#[derive(Serialize)]
struct ScriptsBody {
    /// The last step logged, 0 with none.
    last: u64,
    steps: Vec<ScriptStep>,
}

/// `POST /api/_test/tv/press`'s body.
#[derive(Deserialize)]
struct PressBody {
    button: String,
}

/// `/api/_test/tv` serves whatever the last frame wrote; `POST
/// /api/_test/tv/skip` skips the playing intro as a key would, for a run
/// with nobody at the keyboard; `POST /api/_test/tv/press` hands the input
/// router a press (`{"button": "south"}`: south, east, mode, up, down, left
/// or right; anything else is a 400). A test hook, not a phone remote.
/// `POST /api/_test/tv/pad` drives one of four pads as a hand would, one act
/// a body: `{"pad": 0, "connect": {"name": .., "vendor": .., "product":
/// ..}}` (each optional), `{"pad": 0, "disconnect": true}`, `{"pad": 0,
/// "stick": [x, y]}` (the left stick, raw, each −1 to 1, held until the
/// next) or `{"pad": 0, "press": "start"}` (south, east, west, north, start,
/// select, mode or a D-pad direction; down a frame, then up); anything else
/// is a 400. `GET /api/_test/tv/frames?since_ms=<ms>` hands out the frame
/// log after `since_ms` of the TV's real time (all of it without one) and
/// the last frame's end. `GET /api/_test/tv/gaits?since=<step>` hands out
/// the hero's locomotion step by step after step `since` (all of the
/// current hero's without one) and the last step's number (AG.30). `GET
/// /api/_test/tv/drawn?since=<frame>` hands out the walk's drawn frames
/// after Bevy's frame `since` (AG.55). AH.24: the pad also takes `{"pad":
/// 0, "hold": "west"}` (down until released), `{"pad": 0, "release":
/// "west"}` and `{"pad": 0, "pulse": {"button": "south", "presses": 60}}`
/// (down a game step, up a game step, that many times, 1 to 1,800), and the
/// buttons l1, r1, l2 and r2 besides; `GET /api/_test/tv/scripts?since=<step>`
/// hands out the scripts' run step by step after step `since` (all of the
/// run's without one) and the last step's number: the attacks read, the
/// hero's join, the clips started on him, the clones made and taken off.
/// AG.79: `POST /api/_test/tv/camera` pins the level's camera (`{"eye": [x,
/// y, z], "tilt": t, "heading": h}`, optionally `"fov_y"`, degrees: an
/// oracle frame's viewport) until `{"off": true}`; anything else is a 400.
/// `POST /api/_test/tv/shot` (`{"label": "walkway"}`) asks the shots plugin
/// for a shot now, named for the label (letters, digits, `-` and `_`, 40 at
/// most; anything else is a 400), taken only when `GUAC_SHOT_DIR` is set.
/// Generic so a test needs no `AppState`.
pub fn routes<S: Clone + Send + Sync + 'static>(mirror: &TvMirror) -> axum::Router<S> {
    let shared = mirror.0.clone();
    let skips = mirror.1.clone();
    let presses = mirror.2.clone();
    let frames = mirror.3.clone();
    let pads = mirror.4.clone();
    let gaits = mirror.5.clone();
    let drawn = mirror.6.clone();
    let scripts = mirror.7.clone();
    let pin = mirror.8.clone();
    let asked = mirror.9.clone();
    axum::Router::new()
        .route(
            "/api/_test/tv/shot",
            axum::routing::post(move |body: Result<axum::Json<ShotBody>, JsonRejection>| {
                let ok = body.is_ok_and(|axum::Json(b)| asked.ask(&b.label));
                async move {
                    if ok {
                        axum::http::StatusCode::ACCEPTED
                    } else {
                        axum::http::StatusCode::BAD_REQUEST
                    }
                }
            }),
        )
        .route(
            "/api/_test/tv/camera",
            axum::routing::post(move |body: Result<axum::Json<CameraBody>, JsonRejection>| {
                let asked = body.ok().and_then(|axum::Json(b)| b.pin());
                if let Some(asked) = asked {
                    *pin.lock().unwrap_or_else(|e| e.into_inner()) = asked;
                }
                async move {
                    match asked {
                        Some(_) => axum::http::StatusCode::ACCEPTED,
                        None => axum::http::StatusCode::BAD_REQUEST,
                    }
                }
            }),
        )
        .route(
            "/api/_test/tv/scripts",
            axum::routing::get(move |q: axum::extract::Query<ScriptsQuery>| {
                let steps = scripts.since(q.since.unwrap_or(0));
                let last = scripts.last();
                async move { axum::Json(ScriptsBody { last, steps }) }
            }),
        )
        .route(
            "/api/_test/tv/drawn",
            axum::routing::get(move |q: axum::extract::Query<DrawnQuery>| {
                let body = drawn
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .since(q.since.unwrap_or(0));
                async move { axum::Json(body) }
            }),
        )
        .route(
            "/api/_test/tv/pad",
            axum::routing::post(move |body: Result<axum::Json<PadBody>, JsonRejection>| {
                let posted = body
                    .ok()
                    .and_then(|axum::Json(b)| b.post())
                    .map(|post| pads.push(post));
                async move {
                    match posted {
                        Some(()) => axum::http::StatusCode::ACCEPTED,
                        None => axum::http::StatusCode::BAD_REQUEST,
                    }
                }
            }),
        )
        .route(
            "/api/_test/tv/frames",
            axum::routing::get(move |q: axum::extract::Query<FramesQuery>| {
                let body = frames
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .since(q.since_ms.unwrap_or(f64::NEG_INFINITY));
                async move { axum::Json(body) }
            }),
        )
        .route(
            "/api/_test/tv/gaits",
            axum::routing::get(move |q: axum::extract::Query<GaitsQuery>| {
                let body = gaits
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .since(q.since.unwrap_or(0));
                async move { axum::Json(body) }
            }),
        )
        .route(
            "/api/_test/tv",
            axum::routing::get(move || {
                let shared = shared.clone();
                async move { axum::Json(shared.lock().unwrap_or_else(|e| e.into_inner()).clone()) }
            }),
        )
        .route(
            "/api/_test/tv/skip",
            axum::routing::post(move || {
                skips.fetch_add(1, Ordering::Relaxed);
                async { axum::http::StatusCode::ACCEPTED }
            }),
        )
        .route(
            "/api/_test/tv/press",
            axum::routing::post(move |body: Result<axum::Json<PressBody>, JsonRejection>| {
                let pressed = body
                    .ok()
                    .and_then(|axum::Json(b)| TestPress::parse(&b.button))
                    .map(|press| presses.push(press));
                async move {
                    match pressed {
                        Some(()) => axum::http::StatusCode::ACCEPTED,
                        None => axum::http::StatusCode::BAD_REQUEST,
                    }
                }
            }),
        )
}

fn skip_asked(mirror: Res<TvMirror>, queue: Option<ResMut<IntroQueue>>) {
    if mirror.1.swap(0, Ordering::Relaxed) == 0 {
        return;
    }
    if let Some(mut queue) = queue.filter(|q| q.playing().is_some()) {
        info!("intro skipped over /api/_test/tv/skip");
        intro_player::apply(&mut queue, IntroEvent::Skip);
    }
}

/// The snapshot, each subsystem's part from its own param (openspec AQ.8).
fn mirror_tv(
    mirror: Res<TvMirror>,
    screen: ScreenShots,
    extraction: ExtractionShots,
    intro: IntroShots,
    input: InputShots,
    play: PlayShots,
    alert: AlertShots,
    loot: LootShots,
    (creatures, entry): (CreatureShots, Option<Res<DevEntry>>),
    (hud, defeat): (HudShots, DefeatShots),
    (warm, level, prepare): (Option<Res<Warm>>, Option<Res<Level>>, Option<Res<Prepare>>),
) {
    let Some(screen) = screen.shot() else {
        return;
    };
    let window = Rect::new(0.0, 0.0, screen.window[0], screen.window[1]);
    let playing = intro.playing(window);
    let input_shot = input.shot(playing.is_some());
    let snapshot = TvSnapshot {
        screen: screen.screen,
        game: screen.game,
        message: screen.message,
        players: screen.players,
        join_address: screen.join_address,
        join_qr: screen.join_qr || screen.qr.as_ref().is_some_and(|qr| alert.corner_qr(qr)),
        found: screen.found,
        folders: screen.folders,
        problems: screen.problems,
        focus: input.focus(),
        cards: screen.cards,
        empty_slots: screen.empty_slots,
        window: screen.window,
        cards_outside: screen.cards_outside,
        overlapping: screen.overlapping,
        extraction: extraction.shot(),
        intro: playing,
        intro_queue: intro.queued(),
        warm: warm.map(|w| w.report().clone()),
        level: level.map(|l| l.snapshot()),
        prepare: prepare.map(|p| p.report().clone()),
        input: input_shot,
        mode: play.mode().name().to_owned(),
        hero: play.hero(),
        kept: play.kept(),
        alert: alert.shot(window),
        join_corner: alert.corner(),
        loot: loot.shot(),
        creatures: creatures.shot(),
        entry: entry.as_deref().filter(|e| e.shown()).map(DevEntry::shot),
        hud: hud.shot(),
        defeat: defeat.shot(),
    };
    let mut shared = mirror.0.lock().unwrap_or_else(|e| e.into_inner());
    if *shared != snapshot {
        *shared = snapshot;
    }
}

#[cfg(test)]
mod tests {
    use bevy::camera::RenderTarget;
    use guac_server::config::WindowMode;
    use skylander_core::SlotIndex;

    use super::*;
    use crate::cards::PortalCards;
    use crate::driver::PortalChange;
    use crate::input::PadShot;
    use crate::level::LevelCamera;
    use crate::pause::OverlayShot;
    use crate::screen::TvScreen;
    use crate::test_support::tv::{placement, portal_app};
    use crate::tv_state::{Current, TvAssets};

    fn snapshot(cards: PortalCards, screen: TvScreen) -> TvSnapshot {
        let mut app = portal_app(1920, 1080, WindowMode::Tv, cards);
        app.world_mut().resource_mut::<Current>().screen = screen;
        let mirror = TvMirror::default();
        app.insert_resource(mirror.clone()).add_plugins(Mirror);
        for _ in 0..3 {
            app.update();
        }
        mirror.0.lock().unwrap().clone()
    }

    fn giants() -> TvScreen {
        TvScreen::Portal {
            game: "Skylanders: Giants".into(),
        }
    }

    /// AG.55: the drawn log hands out the frames after the one asked for,
    /// and the render world's frames from the first of them on.
    #[test]
    fn the_drawn_log_hands_out_frames_after_and_their_renders() {
        let mut log = DrawnLog::default();
        let frame = |n: u32, at_ms: f64| DrawnFrame {
            frame: n,
            real_ms: at_ms,
            delta_ms: 8.0,
            work_ms: 1.0,
            at_ms,
            steps: u64::from(n),
            overstep: 0.5,
            hero: [0.0; 7],
            pivot: [0.0; 3],
            camera: [0.0; 7],
            clips: [[0.0; 2]; 4],
            pose_moved: 0.0,
            cameras: 2,
            materials_changed: 0,
        };
        let base = log.base;
        let at = |ms: u64| base + std::time::Duration::from_millis(ms);
        for (n, ms) in [(1, 10), (2, 18), (3, 26)] {
            log.push(frame(n, ms as f64));
            log.present(at(ms + 1), at(ms + 6));
        }
        let body = log.since(1);
        assert_eq!(
            body.frames.iter().map(|f| f.frame).collect::<Vec<_>>(),
            [2, 3]
        );
        assert_eq!(body.presents, [[19.0, 24.0], [27.0, 32.0]]);
        assert!(log.since(3).frames.is_empty() && log.since(3).presents.is_empty());
    }

    #[test]
    fn a_card_reads_back_as_drawn() {
        let cards = PortalCards::default().reduce([
            PortalChange::Placed {
                slot: SlotIndex::new(0).unwrap(),
                placement: placement("Eruptor", Some("#e8423f")),
            },
            PortalChange::Placed {
                slot: SlotIndex::new(5).unwrap(),
                placement: placement("Tree Rex", None),
            },
        ]);
        let shot = snapshot(cards, giants());
        assert_eq!(shot.screen, "Portal");
        assert_eq!(shot.game.as_deref(), Some("Skylanders: Giants"));
        assert_eq!(shot.players, Some(2));
        assert_eq!(
            shot.join_address.as_deref(),
            Some("http://christophers-macbook-pro.local:8765")
        );
        // The harness has no QR.
        assert!(!shot.join_qr);
        assert_eq!(shot.cards.len(), 2);
        let first = &shot.cards[0];
        assert_eq!(first.slot, 1);
        assert_eq!(first.figure_id, "000009-0000");
        assert_eq!(first.texts, ["Eruptor", "Fire"]);
        assert!(!first.portrait);
        let pip = first.pip.as_ref().unwrap();
        assert_eq!(
            (
                pip.initial.as_str(),
                pip.color.as_str(),
                pip.profile_id.as_str()
            ),
            ("A", "#E8423F", "p")
        );
        assert_eq!(shot.cards[1].slot, 6);
        assert!(shot.cards[1].pip.is_none());
        assert_eq!(shot.empty_slots, [2, 3, 4, 5, 7, 8]);
        assert_eq!(shot.window, [1920.0, 1080.0]);
        assert!(shot.cards_outside.is_empty());
        assert_eq!(shot.overlapping, 0);
        assert!(
            shot.cards
                .iter()
                .all(|c| c.rect[2] > 0.0 && c.rect[3] > 0.0)
        );
    }

    /// AG.29: an alert reads back as drawn, its lines, full screen, the
    /// corner join code beside it; the screen's tree hidden under the level
    /// (AG.50) draws none of its cards or join lines, while the corner's
    /// join code stands over it. The alert gone, the corner stays over the
    /// level and goes with its end.
    #[test]
    fn an_alert_and_the_corner_join_code_read_back_and_a_hidden_screen_draws_no_card() {
        use crate::play::{Alert, Play};
        let cards = PortalCards::default().reduce([
            PortalChange::Placed {
                slot: SlotIndex::new(0).unwrap(),
                placement: placement("Eruptor", Some("#e8423f")),
            },
            PortalChange::Placed {
                slot: SlotIndex::new(5).unwrap(),
                placement: placement("Tree Rex", None),
            },
        ]);
        let mut app = portal_app(1920, 1080, WindowMode::Tv, cards);
        app.world_mut().resource_mut::<Current>().screen = giants();
        let mirror = TvMirror::default();
        app.insert_resource(mirror.clone())
            .insert_resource(Play::alerting_for_tests(Alert::TooManySkylanders, None))
            .add_plugins((Mirror, crate::alert::AlertPlugin));
        for _ in 0..3 {
            app.update();
        }
        let shot = mirror.0.lock().unwrap().clone();
        let alert = shot.alert.as_ref().expect("the alert");
        assert_eq!(alert.kind, "too_many_skylanders");
        assert_eq!(
            alert.texts,
            crate::alert::lines(Alert::TooManySkylanders).map(str::to_owned)
        );
        assert!(alert.full_screen, "{alert:?}");
        assert!(!alert.over_level, "no level camera");
        assert!(alert.shown_ms.is_some());
        assert!(shot.join_corner, "the join code over the alert");
        assert_eq!(shot.cards.len(), 2, "under it, the portal view's cards");
        assert_eq!(shot.players, Some(2));

        // The level over the screen: its tree hidden, the hero off.
        let mut roots = app
            .world_mut()
            .query_filtered::<&mut Visibility, With<crate::tv_state::ScreenRoot>>();
        for mut v in roots.iter_mut(app.world_mut()) {
            *v = Visibility::Hidden;
        }
        app.insert_resource(Play::alerting_for_tests(
            Alert::PlaceSkylander,
            Some(Entity::PLACEHOLDER),
        ));
        for _ in 0..3 {
            app.update();
        }
        let shot = mirror.0.lock().unwrap().clone();
        assert_eq!(shot.alert.as_ref().unwrap().kind, "place_a_skylander");
        assert!(
            shot.cards.is_empty() && shot.empty_slots.is_empty(),
            "{:?}",
            shot.cards
        );
        assert_eq!((shot.players, shot.join_address.as_deref()), (None, None));
        assert!(shot.join_corner);

        // The alert gone, the level still shown: the corner alone.
        app.insert_resource(Play::started_for_tests(Entity::PLACEHOLDER));
        for _ in 0..3 {
            app.update();
        }
        let shot = mirror.0.lock().unwrap().clone();
        assert!(shot.alert.is_none(), "{:?}", shot.alert);
        assert!(shot.join_corner, "the join code over the level alone");

        // No alert and no level: neither drawn.
        app.insert_resource(Play::default());
        for _ in 0..3 {
            app.update();
        }
        let shot = mirror.0.lock().unwrap().clone();
        assert!(shot.alert.is_none() && !shot.join_corner);
    }

    #[test]
    fn off_the_portal_screen_no_card_is_drawn() {
        let cards = PortalCards::default().reduce([PortalChange::Placed {
            slot: SlotIndex::new(0).unwrap(),
            placement: placement("Eruptor", Some("#e8423f")),
        }]);
        let shot = snapshot(cards, TvScreen::Join);
        assert_eq!(shot.screen, "Join");
        assert!(shot.game.is_none());
        assert!(shot.cards.is_empty() && shot.empty_slots.is_empty());
    }

    #[test]
    fn the_join_screen_reads_back_its_qr_and_a_new_phone() {
        let mut app = portal_app(1920, 1080, WindowMode::Tv, PortalCards::default());
        let qr = app
            .world_mut()
            .resource_mut::<Assets<Image>>()
            .add(Image::default());
        app.world_mut().resource_mut::<TvAssets>().qr = Some(qr);
        {
            let mut current = app.world_mut().resource_mut::<Current>();
            current.screen = TvScreen::Join;
            current.clients = 0;
        }
        let mirror = TvMirror::default();
        app.insert_resource(mirror.clone()).add_plugins(Mirror);
        for _ in 0..3 {
            app.update();
        }
        let shot = mirror.0.lock().unwrap().clone();
        assert!(shot.join_qr);
        assert_eq!(shot.players, Some(0));
        assert_eq!(
            shot.join_address.as_deref(),
            Some("http://christophers-macbook-pro.local:8765")
        );

        // Read off the redrawn line, so a tree that missed the change shows 0.
        app.world_mut().resource_mut::<Current>().clients = 1;
        for _ in 0..3 {
            app.update();
        }
        assert_eq!(mirror.0.lock().unwrap().players, Some(1));

        // Screens without the join block draw none of it.
        app.world_mut().resource_mut::<Current>().screen = TvScreen::Farewell;
        for _ in 0..3 {
            app.update();
        }
        let shot = mirror.0.lock().unwrap().clone();
        assert_eq!(
            (shot.players, shot.join_address, shot.join_qr),
            (None, None, false)
        );
    }

    #[test]
    fn the_extraction_reads_back_its_state_and_drawn_band() {
        use crate::extraction::{ExtractBand, ExtractStatus, Extraction, Failure, band};
        let mut app = portal_app(1920, 1080, WindowMode::Tv, PortalCards::default());
        let mirror = TvMirror::default();
        app.insert_resource(mirror.clone()).add_plugins(Mirror);
        app.update();
        let idle = mirror.0.lock().unwrap().extraction.clone();
        assert_eq!(idle.state, "Idle");
        assert_eq!((idle.title, idle.percent), (None, None));

        let converting = ExtractStatus::Converting {
            done: 12,
            total: 48,
            movie: "eruptor".into(),
        };
        app.insert_resource(ExtractBand(band(&converting)));
        app.insert_resource(Extraction {
            status: converting,
            ..Default::default()
        });
        for _ in 0..3 {
            app.update();
        }
        let shot = mirror.0.lock().unwrap().extraction.clone();
        assert_eq!(shot.state, "Converting");
        assert_eq!(shot.converted, Some([12, 48]));
        assert_eq!(shot.detail.as_deref(), Some("Converting intro 13 of 48…"));
        assert_eq!(shot.percent, Some(25));
        assert_eq!(shot.code, None);

        let failed = ExtractStatus::Failed(Failure::NoFfmpeg {
            configured: None,
            install: "brew install ffmpeg".into(),
        });
        app.insert_resource(ExtractBand(band(&failed)));
        app.insert_resource(Extraction {
            status: failed,
            ..Default::default()
        });
        for _ in 0..3 {
            app.update();
        }
        let shot = mirror.0.lock().unwrap().extraction.clone();
        assert_eq!(shot.state, "Failed");
        assert_eq!(shot.code.as_deref(), Some("GUAC-E-NO-FFMPEG"));
        assert_eq!(shot.title.as_deref(), Some("No intros: ffmpeg is needed"));
        assert!(shot.detail.unwrap().contains("brew install ffmpeg"));
        assert_eq!(shot.percent, None);

        app.insert_resource(ExtractBand(None));
        app.insert_resource(Extraction {
            status: ExtractStatus::Ready {
                intros: 53,
                ran: false,
            },
            ..Default::default()
        });
        for _ in 0..3 {
            app.update();
        }
        let shot = mirror.0.lock().unwrap().extraction.clone();
        assert_eq!(
            (shot.state.as_str(), shot.intros, shot.ran),
            ("Ready", Some(53), Some(false))
        );
        assert_eq!(shot.title, None);
    }

    #[tokio::test]
    async fn the_route_serves_the_last_snapshot() {
        use tower::ServiceExt;
        let mirror = TvMirror::default();
        mirror.0.lock().unwrap().screen = "NoGiants".into();
        let app = routes::<()>(&mirror);
        let response = app
            .clone()
            .oneshot(
                axum::http::Request::get("/api/_test/tv")
                    .body(axum::body::Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), 200);
        let body = http_body_util::BodyExt::collect(response.into_body())
            .await
            .unwrap()
            .to_bytes();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(json["screen"], "NoGiants");
        assert_eq!(json["cards"], serde_json::json!([]));
        assert_eq!(json["intro"], serde_json::Value::Null);
        assert_eq!(json["intro_queue"], serde_json::json!([]));

        let response = app
            .oneshot(
                axum::http::Request::post("/api/_test/tv/skip")
                    .body(axum::body::Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), 202);
        assert_eq!(mirror.1.load(Ordering::Relaxed), 1);
    }

    /// AG.30: `/api/_test/tv/gaits` hands out the hero's locomotion steps
    /// after the one asked for, and a hero coming in starts the log again.
    #[tokio::test]
    async fn the_gait_log_hands_out_steps_after_one() {
        use tower::ServiceExt;
        let mirror = TvMirror::default();
        let step = |n: u64, clip: &'static str| GaitStep {
            step: n,
            time: n as f64 / 60.0,
            stick: [0.0, 1.0],
            strength: 1.0,
            speed_ft: 2.6667 * n as f32,
            run_share: 0.0,
            weights: [1.0, 0.0, 0.0, 0.0],
            rates: [1.0, 1.0],
            clip,
            air: "ground",
            moving: n > 1,
            held: false,
            facing: 0.0,
        };
        let get = |since: &'static str| {
            let routes = routes::<()>(&mirror);
            async move {
                let response = routes
                    .oneshot(
                        axum::http::Request::get(format!("/api/_test/tv/gaits{since}"))
                            .body(axum::body::Body::empty())
                            .unwrap(),
                    )
                    .await
                    .unwrap();
                assert_eq!(response.status(), 200);
                let body = http_body_util::BodyExt::collect(response.into_body())
                    .await
                    .unwrap()
                    .to_bytes();
                serde_json::from_slice::<serde_json::Value>(&body).unwrap()
            }
        };
        assert_eq!(get("").await, serde_json::json!({"now": 0, "steps": []}));
        for n in 1..=3 {
            mirror.5.lock().unwrap().push(step(n, "idle"));
        }
        let json = get("?since=1").await;
        assert_eq!(json["now"], 3);
        let steps = json["steps"].as_array().unwrap();
        assert_eq!(steps.len(), 2);
        assert_eq!(steps[0]["step"], 2);
        assert_eq!(steps[0]["clip"], "idle");
        assert_eq!(steps[1]["weights"], serde_json::json!([1.0, 0.0, 0.0, 0.0]));
        assert_eq!(get("").await["steps"].as_array().unwrap().len(), 3);
        mirror.5.lock().unwrap().push(step(1, "walk"));
        let json = get("").await;
        assert_eq!(
            (json["now"].clone(), json["steps"][0]["clip"].clone()),
            (1.into(), "walk".into())
        );
        assert_eq!(json["steps"].as_array().unwrap().len(), 1);
    }

    /// AG.188: a step of the gait log carries the way he faces as the step
    /// left him, the heading `hero.heading` reports: two steps into a turn
    /// to his right (24° a step at Eruptor's 1440°/s), 312°, short of the
    /// stick's 270°; in the log and in `/api/_test/tv/gaits`' JSON.
    #[tokio::test]
    async fn a_gait_step_carries_his_facing_after_a_turn() {
        use bevy::ecs::system::RunSystemOnce;
        use guac_motion::hero::Input;
        use guac_motion::locomotion::{Frame, Gaits, Locomotion, Loops};
        use guac_motion::parry::ParryWorld;
        use guac_motion::scenarios;
        use tower::ServiceExt;
        let ground = scenarios::bodies(&scenarios::open_ground(), &[]);
        let parry = ParryWorld::new(&ground).unwrap();
        let mut hero = scenarios::eruptor(&parry, Vec3::new(0.0, 1.0, 0.0), 0.0);
        let right = Input::stick(scenarios::stick(1.0, -90.0), 0.0);
        let mirror = TvMirror::default();
        let mut app = App::new();
        app.insert_resource(mirror.clone())
            .init_resource::<Time>()
            .insert_resource(HeroInput(right));
        let loops = Loops {
            idle: 2.0,
            walk: Some(1.0),
            run: Some(0.8),
            fall: None,
        };
        let mut gaits = HeroGaits::new(Locomotion::new(Gaits::ERUPTOR, loops));
        let e = app.world_mut().spawn_empty().id();
        for _ in 0..2 {
            hero.step(&parry, &right, 1.0 / 60.0, &[]);
            gaits.locomotion.step(1.0 / 60.0, Frame::of(&hero));
            gaits.steps += 1;
            app.world_mut().entity_mut(e).insert((
                HeroBody {
                    before: hero.pivot,
                    hero: hero.clone(),
                    exclude: Vec::new(),
                },
                gaits.clone(),
            ));
            app.world_mut().run_system_once(log_gaits).unwrap();
        }
        let want = crate::hero::heading(&hero);
        assert!((want - 312.0).abs() < 1e-3, "two steps in: {want}");
        let steps = mirror.5.lock().unwrap().since(0).steps;
        assert_eq!(steps.len(), 2);
        assert_eq!(steps[1].facing, want);
        assert!(
            (steps[0].facing - 336.0).abs() < 1e-3,
            "{}",
            steps[0].facing
        );
        let response = routes::<()>(&mirror)
            .oneshot(
                axum::http::Request::get("/api/_test/tv/gaits?since=0")
                    .body(axum::body::Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        let body = http_body_util::BodyExt::collect(response.into_body())
            .await
            .unwrap()
            .to_bytes();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(json["steps"][1]["facing"], serde_json::json!(want));
    }

    /// AG.20: `/api/_test/tv/pad` takes one act a body for pads 0 to 3 and
    /// refuses the rest; the acts drive a pad through Bevy's own gamepad
    /// systems, so its stick lands raw, and with the level shown that pad
    /// walks, Start pauses under the overlay as drawn, losing the pad brings
    /// the controller message, and another pad's press carries on with it.
    /// AG.50: both overlays draw over the level's camera, and at AG.21's
    /// order (the level's camera after the TV's) the mirror says they don't.
    #[tokio::test]
    async fn posted_pads_drive_the_level_as_a_hand_would() {
        use tower::ServiceExt;
        let mirror = TvMirror::default();
        let routes = routes::<()>(&mirror);
        let post = |body: &'static str| {
            let routes = routes.clone();
            async move {
                routes
                    .oneshot(
                        axum::http::Request::post("/api/_test/tv/pad")
                            .header("content-type", "application/json")
                            .body(axum::body::Body::from(body))
                            .unwrap(),
                    )
                    .await
                    .unwrap()
                    .status()
            }
        };
        for (body, status) in [
            (r#"{"pad":0,"connect":{}}"#, 202),
            (r#"{"pad":0,"stick":[0.26,-0.25]}"#, 202),
            (r#"{"pad":4,"connect":{}}"#, 400),
            (r#"{"pad":0,"stick":[1.5,0]}"#, 400),
            // AG.28's verify: the right stick, raw, held until the next.
            (r#"{"pad":0,"rstick":[1,-1]}"#, 202),
            (r#"{"pad":0,"rstick":[0,1.5]}"#, 400),
            (r#"{"pad":0,"rstick":[0,0],"stick":[0,0]}"#, 400),
            (r#"{"pad":0,"press":"jump"}"#, 400),
            (r#"{"pad":0,"press":"start","stick":[0,0]}"#, 400),
            (r#"{"pad":0}"#, 400),
            (r#"{"pad":0,"jump":"south"}"#, 400),
            // AH.24's: a held button, its release and a pulse (on a pad
            // never connected, so the run below never sees them).
            (r#"{"pad":3,"hold":"south"}"#, 202),
            (r#"{"pad":3,"release":"r2"}"#, 202),
            (r#"{"pad":3,"hold":"jump"}"#, 400),
            (r#"{"pad":3,"hold":"west","release":"west"}"#, 400),
            (r#"{"pad":3,"pulse":{"button":"south","presses":60}}"#, 202),
            (r#"{"pad":3,"pulse":{"button":"south","presses":0}}"#, 400),
            (
                r#"{"pad":3,"pulse":{"button":"south","presses":1801}}"#,
                400,
            ),
            (r#"{"pad":3,"pulse":{"button":"jump","presses":2}}"#, 400),
            ("pad", 400),
        ] {
            assert_eq!(post(body).await, status, "{body}");
        }

        let mut app = portal_app(1920, 1080, WindowMode::Tv, PortalCards::default());
        app.insert_resource(mirror.clone())
            .add_plugins(Mirror)
            .add_systems(PostUpdate, crate::level::tv_over_level);
        let until = |app: &mut App, what: &str, done: &dyn Fn(&InputShot) -> bool| {
            for _ in 0..30 {
                app.update();
                let shot = mirror.0.lock().unwrap().input.clone();
                if done(&shot) {
                    return shot;
                }
            }
            panic!("{what}: {:#?}", mirror.0.lock().unwrap().input);
        };
        let shot = until(&mut app, "connected", &|s| !s.pads.is_empty());
        assert_eq!(shot.layer, "screen");
        assert_eq!(
            shot.pads,
            [PadShot {
                name: "guacamole test pad".into(),
                vendor: None,
                product: None,
                labels: "position".into(),
                stick: [0.26, -0.25],
                walking: false,
            }]
        );
        assert_eq!(shot.hero_stick, None);

        // The level shows, its camera on the TV's target as the walk's is.
        app.world_mut()
            .resource_mut::<crate::pad::LevelPads>()
            .show();
        let level_camera = app
            .world_mut()
            .spawn((
                crate::level::view_camera(RenderTarget::default(), crate::level::VIEW_ORDER),
                LevelCamera,
            ))
            .id();
        let shot = until(&mut app, "walking", &|s| s.pads[0].walking);
        assert_eq!(shot.layer, "level");
        assert_eq!(shot.hero_stick, Some([0.26, -0.25]));
        assert_eq!((shot.pause, shot.overlay), (None, None));

        assert_eq!(post(r#"{"pad":0,"press":"start"}"#).await, 202);
        let shot = until(&mut app, "paused", &|s| s.overlay.is_some());
        assert_eq!(shot.pause.as_deref(), Some("start"));
        let overlay = shot.overlay.unwrap();
        assert!(overlay.shown_ms.is_some(), "{overlay:?}");
        assert_eq!(
            overlay,
            OverlayShot {
                kind: "paused".into(),
                texts: vec!["Paused".into(), "Press".into(), "to play on".into()],
                prompt: Some("position:south".into()),
                over_level: true,
                shown_ms: overlay.shown_ms,
            }
        );

        assert_eq!(post(r#"{"pad":0,"disconnect":true}"#).await, 202);
        let shot = until(&mut app, "lost", &|s| {
            s.pause.as_deref() == Some("controller")
        });
        assert!(shot.pads.is_empty());
        let shot = until(&mut app, "its message", &|s| {
            s.overlay.as_ref().is_some_and(|o| o.kind == "controller")
        });
        let overlay = shot.overlay.unwrap();
        assert_eq!(
            overlay.texts,
            ["Connect a controller", "then press any button on it"]
        );
        assert!(overlay.over_level, "the message over the level");
        // AG.21's order, the level's camera after the TV's replacing it,
        // covered both; the mirror says so.
        app.world_mut()
            .get_mut::<Camera>(level_camera)
            .unwrap()
            .order = 1;
        let shot = until(&mut app, "covered", &|s| {
            s.overlay.as_ref().is_some_and(|o| !o.over_level)
        });
        assert_eq!(shot.overlay.unwrap().kind, "controller");
        app.world_mut()
            .get_mut::<Camera>(level_camera)
            .unwrap()
            .order = crate::level::VIEW_ORDER;

        for body in [
            r#"{"pad":1,"connect":{"name":"another","vendor":11720,"product":1}}"#,
            r#"{"pad":1,"stick":[0,1]}"#,
            r#"{"pad":1,"press":"east"}"#,
        ] {
            assert_eq!(post(body).await, 202, "{body}");
        }
        let shot = until(&mut app, "taken over", &|s| {
            s.pause.is_none() && s.overlay.is_none()
        });
        assert_eq!(shot.pads.len(), 1);
        assert!(shot.pads[0].walking);
        assert_eq!(
            (shot.pads[0].name.as_str(), shot.pads[0].vendor),
            ("another", Some(11720))
        );
        assert_eq!(shot.hero_stick, Some([0.0, 1.0]));
    }

    /// AH.24: a posted pad's button held until released, read as held by
    /// Bevy's own systems; a pulse down a game step and up the next, each
    /// press read by one step as the hero's attack and no more, that many
    /// times.
    #[test]
    fn a_posted_pad_holds_and_pulses_by_game_steps() {
        use crate::hero::HeroActions;
        let mirror = TvMirror::default();
        let mut app = App::new();
        app.add_plugins((
            MinimalPlugins,
            bevy::input::InputPlugin,
            crate::input::InputRouter,
        ))
        .init_resource::<HeroActions>()
        .insert_resource(Time::<Fixed>::from_hz(60.0))
        .insert_resource(bevy::time::TimeUpdateStrategy::ManualDuration(
            std::time::Duration::from_secs_f64(1.0 / 60.0),
        ))
        .init_resource::<PadSteps>()
        .add_systems(PreUpdate, drive_posted_pads.before(InputSystems))
        .add_systems(FixedUpdate, count_pad_steps.in_set(Frame::Pads));
        app.insert_resource(mirror.clone());
        // Each game step reads the attacks as the scripts' step does.
        #[derive(Resource, Default)]
        struct Read(Vec<[bool; 4]>);
        app.init_resource::<Read>().add_systems(
            FixedUpdate,
            (|mut a: ResMut<HeroActions>, mut read: ResMut<Read>| {
                let now = a.take();
                read.0.push(now);
            })
            .in_set(Frame::Scripts),
        );
        let push = |act: PadAct| mirror.4.push(PadPost { pad: 0, act });
        push(PadAct::Connect {
            name: "pulse pad".into(),
            vendor: None,
            product: None,
        });
        for _ in 0..3 {
            app.update();
        }
        app.world_mut()
            .resource_mut::<crate::pad::LevelPads>()
            .show();
        for _ in 0..3 {
            app.update();
        }
        let pad = {
            let mut q = app.world_mut().query::<(Entity, &Gamepad)>();
            q.iter(app.world()).next().unwrap().0
        };
        let held = |app: &App, b| app.world().get::<Gamepad>(pad).unwrap().pressed(b);
        push(PadAct::Hold(GamepadButton::West));
        for _ in 0..5 {
            app.update();
            assert!(held(&app, GamepadButton::West));
        }
        push(PadAct::Release(GamepadButton::West));
        app.update();
        assert!(!held(&app, GamepadButton::West));
        let from = app.world().resource::<Read>().0.len();
        push(PadAct::Pulse(GamepadButton::South, 3));
        for _ in 0..12 {
            app.update();
        }
        let read = &app.world().resource::<Read>().0[from..];
        let south: Vec<bool> = read.iter().map(|a| a[0]).collect();
        assert_eq!(
            south.iter().filter(|s| **s).count(),
            3,
            "three presses: {south:?}"
        );
        let first = south.iter().position(|s| *s).unwrap();
        assert_eq!(
            &south[first..first + 5],
            [true, false, true, false, true],
            "every other step: {south:?}"
        );
    }

    /// AP.5: `south` is queued and the router takes it as the screen's
    /// Activate; a name it doesn't know, or no name, is a 400 and queues nothing.
    #[tokio::test]
    async fn a_posted_press_reaches_the_router_and_an_unknown_one_is_refused() {
        use crate::input::TvInput;
        use crate::test_support::heard::{listen, take};
        use tower::ServiceExt;
        let mirror = TvMirror::default();
        let routes = routes::<()>(&mirror);
        for (body, status) in [
            (r#"{"button":"south"}"#, 202),
            (r#"{"button":"jump"}"#, 400),
            (r#"{"press":"south"}"#, 400),
            ("south", 400),
        ] {
            let response = routes
                .clone()
                .oneshot(
                    axum::http::Request::post("/api/_test/tv/press")
                        .header("content-type", "application/json")
                        .body(axum::body::Body::from(body))
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(response.status(), status, "{body}");
        }
        assert_eq!(mirror.2.len(), 1);

        let mut app = portal_app(1920, 1080, WindowMode::Tv, PortalCards::default());
        app.world_mut().resource_mut::<Current>().screen = TvScreen::Join;
        // The router reads the posted presses the mirror's plugin hands it.
        app.insert_resource(mirror.2.clone());
        listen(&mut app);
        app.update();
        assert_eq!(take(&mut app), [TvInput::Activate]);
        assert_eq!(mirror.2.len(), 0);
        app.update();
        assert_eq!(take(&mut app), []);
    }

    /// AG.79: `/api/_test/tv/camera` holds the level's camera where a pin
    /// puts it (an oracle frame's eye, tilt and heading, and its picture's
    /// height when given), the follow stepping on underneath, until `{"off":
    /// true}`; a body that's neither, or a pin that isn't finite, is a 400.
    #[tokio::test]
    async fn a_pinned_camera_draws_from_the_pin_until_its_off() {
        use bevy::camera::PerspectiveProjection;
        use guac_motion::camera::{CameraDef, GameCamera};
        use tower::ServiceExt;
        let mirror = TvMirror::default();
        let routes = routes::<()>(&mirror);
        let post = |body: &'static str| {
            let routes = routes.clone();
            async move {
                routes
                    .oneshot(
                        axum::http::Request::post("/api/_test/tv/camera")
                            .header("content-type", "application/json")
                            .body(axum::body::Body::from(body))
                            .unwrap(),
                    )
                    .await
                    .unwrap()
                    .status()
            }
        };
        for (body, status) in [
            (r#"{"eye":[1,2,3],"tilt":0,"heading":"x"}"#, 400),
            (r#"{"eye":[1,2],"tilt":0,"heading":0}"#, 400),
            (r#"{"eye":[1,2,3],"tilt":0,"heading":0,"fov_y":180}"#, 400),
            (r#"{"off":false}"#, 400),
            ("pin", 400),
        ] {
            assert_eq!(post(body).await, status, "{body}");
        }
        assert_eq!(*mirror.8.lock().unwrap(), None);

        let mut app = App::new();
        app.add_plugins(MinimalPlugins)
            .insert_resource(mirror.clone())
            .add_systems(PostUpdate, pin_camera);
        let feet = Vec3::new(10.19, 43.38, -93.39);
        let follow = FollowCamera::new(
            GameCamera::new(CameraDef::LEVEL_213_START, feet),
            Vec::new(),
        );
        let followed = crate::hero::viewed(&follow.camera.view());
        let camera = app
            .world_mut()
            .spawn((
                followed,
                Projection::Perspective(PerspectiveProjection::default()),
                follow,
            ))
            .id();
        let drawn = |app: &App| {
            let e = app.world().entity(camera);
            let fov = match e.get::<Projection>().unwrap() {
                Projection::Perspective(p) => p.fov,
                _ => unreachable!(),
            };
            (*e.get::<Transform>().unwrap(), fov)
        };
        app.update();
        assert_eq!(drawn(&app).0, followed, "no pin: the follow's");

        // The walkway's viewport (AH.8's route_b, `actors-walkway.json`).
        assert_eq!(
            post(r#"{"eye":[-17.509413,65.969376,-67.233185],"tilt":-37.472958,"heading":60.542427,"fov_y":53.96}"#).await,
            202
        );
        app.update();
        let (t, fov) = drawn(&app);
        assert!(
            t.translation
                .distance(Vec3::new(-17.509413, 65.969376, -67.233185))
                < 1e-6
        );
        let ahead = t.forward().as_vec3();
        let want = guacamole::level_scene::facing([-37.472958, 60.542427, 0.0]);
        assert!(ahead.distance(want) < 1e-6, "{ahead} against {want}");
        assert!(t.right().y.abs() < 1e-6, "unrolled");
        assert!((fov - 53.96f32.to_radians()).abs() < 1e-6);
        // The follow steps on underneath: a step's write is drawn over.
        app.world_mut().entity_mut(camera).insert(followed);
        app.update();
        assert_eq!(drawn(&app).0, t);

        assert_eq!(post(r#"{"off":true}"#).await, 202);
        assert_eq!(*mirror.8.lock().unwrap(), None);
        // A shot asked for over the route lands in the shots plugin's queue.
        for (body, status) in [
            (r#"{"label":"walkway"}"#, 202),
            (r#"{"label":"../walkway"}"#, 400),
            (r#"{"shot":"walkway"}"#, 400),
        ] {
            let response = routes
                .clone()
                .oneshot(
                    axum::http::Request::post("/api/_test/tv/shot")
                        .header("content-type", "application/json")
                        .body(axum::body::Body::from(body))
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(response.status(), status, "{body}");
        }
        assert_eq!(*mirror.9.0.lock().unwrap(), ["walkway"]);
        app.world_mut().entity_mut(camera).insert(followed);
        app.update();
        assert_eq!(drawn(&app).0, followed, "off: the follow's again");
    }
}
