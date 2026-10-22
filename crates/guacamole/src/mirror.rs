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
//! `/api/_test/tv/scripts` the scripts' run step by step (AH.24) and
//! `/api/_test/tv/pad` drives pads as a hand would, no hardware needed.

use std::collections::VecDeque;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use axum::extract::rejection::JsonRejection;
use bevy::camera::{Projection, RenderTarget};
use bevy::input::InputSystems;
use bevy::input::gamepad::{
    GamepadButton, GamepadConnection, GamepadConnectionEvent, RawGamepadAxisChangedEvent,
    RawGamepadButtonChangedEvent, RawGamepadEvent,
};
use bevy::input_focus::InputFocus;
use bevy::prelude::*;
use bevy::ui::{ComputedNode, ComputedUiTargetCamera, UiGlobalTransform, UiSystems};
use serde::{Deserialize, Serialize};

use crate::alert::{AlertScreen, CornerJoin, CornerQr};
use crate::cards::PortalCards;
use crate::extraction::{ExtractStatus, Extraction};
use crate::hero::{FollowCamera, HeroActions, HeroBody, HeroGaits, HeroInput};
use crate::hero_scripts::{HeroRun, ScriptLog, ScriptStep};
use crate::input::Layer;
use crate::input::{FocusId, PostedPresses, TestPress};
use crate::intro::{IntroEntry, IntroEvent, IntroQueue};
use crate::intro_player::{self, IntroOverlay, IntroPlayer, IntroVideo};
use crate::level::{Level, LevelCamera, LevelReport, draws_over};
use crate::pad::{FacesPrinted, LevelPads, Pause, labels};
use crate::pause::{LevelOverlay, PromptGlyph, ShownAt};
use crate::play::{HeroReport, Mode, Play};
use crate::prepare::{Prepare, PrepareReport};
use crate::screen::TvScreen;
use crate::turntable::{CardPlate, CardState, Turntables};
use crate::tv::{
    CardSlot, Current, EmptySlot, ExtractBar, ExtractDetail, ExtractTitle, Framing, JoinAddress,
    JoinQr, Pip, PlayersLine, ScreenRoot, Setup, TvAssets, TvCamera, contains, node_rect,
    safe_area,
};
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
    /// The portal's alert as drawn (AG.29, AG.63, AG.73).
    pub alert: Option<AlertShot>,
    /// The small join code drawn in the corner over the level or an alert
    /// (AG.29); `join_qr` counts its QR too.
    pub join_corner: bool,
    /// Where each kind of breakable's loot count stands (openspec AH.27),
    /// once the level's loot data is made; none before the scripts run.
    pub loot: Option<LootShot>,
}

/// The level's loot counts as its scripts hold them (AH.21's running count:
/// a break drops when its kind's count lands at or under the kind's rate).
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct LootShot {
    pub common: Option<f32>,
    pub uncommon: Option<f32>,
    pub rare: Option<f32>,
}

/// The portal's alert as drawn.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct AlertShot {
    /// `wrong_toy`, `too_many_skylanders`, `too_many_items` or
    /// `place_a_skylander`.
    pub kind: String,
    /// Its lines, top to bottom.
    pub texts: Vec<String>,
    /// It covers the whole window.
    pub full_screen: bool,
    /// Over the level's camera, as an overlay is ([`draws_over`]); false
    /// with no level camera.
    pub over_level: bool,
    /// When it was spawned, on the frame log's clock.
    pub shown_ms: Option<f64>,
}

/// The pads as the router reads them, and the level's layer (AG.20).
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct InputShot {
    /// Who takes the pads: `intro`, `level` or `screen`.
    pub layer: String,
    /// Connected, in the order they connected.
    pub pads: Vec<PadShot>,
    /// While the level shows and waits: `start` or `controller`.
    pub pause: Option<String>,
    /// The waiting level's overlay as drawn.
    pub overlay: Option<OverlayShot>,
    /// The walking pad's stick as the hero read it this frame, raw; none
    /// while no level shows.
    pub hero_stick: Option<[f32; 2]>,
    /// The attacks the walking pad holds this frame (AH.24, pad-input's The
    /// face buttons attack: the first, second, third and a fourth no
    /// position raises yet); none while no level shows.
    pub hero_actions: Option<[bool; 4]>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct PadShot {
    pub name: String,
    pub vendor: Option<u16>,
    pub product: Option<u16>,
    /// The resolver's pick: `xbox`, `nintendo` or `position`.
    pub labels: String,
    /// The left stick as stored, raw.
    pub stick: [f32; 2],
    /// This pad walks the hero.
    pub walking: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct OverlayShot {
    /// `paused` or `controller`.
    pub kind: String,
    /// Every line drawn on it, top to bottom.
    pub texts: Vec<String>,
    /// The drawn prompt: `A`, `B` ... or `position:south`.
    pub prompt: Option<String>,
    /// It draws over the level's camera (AG.50): its camera after the
    /// level's on one target, cleared to no colour and laid over
    /// ([`crate::level::draws_over`]); false with no level camera.
    pub over_level: bool,
    /// When it was spawned, on the frame log's clock (`/api/_test/tv/frames`).
    pub shown_ms: Option<f64>,
}

/// A queued intro, from `IntroQueue`.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct QueuedShot {
    /// 1-based.
    pub slot: u8,
    pub figure_id: String,
    pub movie: String,
}

impl From<&IntroEntry> for QueuedShot {
    fn from(e: &IntroEntry) -> Self {
        Self {
            slot: e.slot.as_u8() + 1,
            figure_id: e.figure_id.clone(),
            movie: e.movie.clone(),
        }
    }
}

/// The playing intro: the queue's entry, the player's counts, the overlay as drawn.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct IntroShot {
    pub slot: u8,
    pub figure_id: String,
    pub movie: String,
    /// The video's frame count once open; frames presented and passed over.
    pub frames: Option<usize>,
    pub shown: usize,
    pub skipped: usize,
    /// `waiting`, `starting`, `playing` (a sink) or `none` (wall clock only).
    pub audio: String,
    /// The overlay is drawn and covers the whole window.
    pub full_screen: bool,
    /// The picture's x, y, width, height in physical pixels, once open.
    pub video_rect: Option<[f32; 4]>,
}

/// The extraction's state from the resource; the band's lines and bar as drawn.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct ExtractionShot {
    /// `ExtractStatus`'s variant: `Idle`, `Checking`, `Converting`, `Ready`, `Failed`, ...
    pub state: String,
    /// A failure's `GUAC-E-*` code.
    pub code: Option<String>,
    /// Converting: intros done, of how many.
    pub converted: Option<[usize; 2]>,
    /// Ready: toy types with an intro, and whether this start extracted.
    pub intros: Option<usize>,
    pub ran: Option<bool>,
    pub title: Option<String>,
    pub detail: Option<String>,
    /// The drawn bar's fill.
    pub percent: Option<u8>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct CardShot {
    /// 1-based, as the phone numbers slots.
    pub slot: u8,
    /// From `PortalCards`, not drawn.
    pub figure_id: String,
    /// Every line drawn on the card outside the pip, top to bottom.
    pub texts: Vec<String>,
    /// An image drawn in the portrait frame: the portrait, or the model.
    pub portrait: bool,
    /// The card's turntable (AE.13): `model` (its model is on the card),
    /// `portrait` (no model to show, or one that couldn't be) or `loading`
    /// (the portrait while the model is read or the extract makes it).
    pub state: String,
    /// Why a `portrait` card has no model, as the log has it.
    pub reason: Option<String>,
    /// The bezel plate the portrait or the model draws in: x, y, width,
    /// height in physical pixels.
    pub plate: Option<[f32; 4]>,
    pub pip: Option<PipShot>,
    /// x, y, width, height in physical pixels.
    pub rect: [f32; 4],
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct PipShot {
    pub initial: String,
    /// The pip's fill as drawn, `#RRGGBB`.
    pub color: String,
    /// From `PortalCards`, not drawn.
    pub profile_id: String,
}

/// Shared with the routes: the last snapshot, skips asked for over HTTP,
/// presses posted for the input router, the frame log, the posted pads,
/// the gait log and the drawn log.
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
);

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

fn count_pad_steps(mut steps: ResMut<PadSteps>) {
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
fn log_gaits(
    time: Res<Time>,
    mirror: Res<TvMirror>,
    input: Option<Res<HeroInput>>,
    heroes: Query<&HeroGaits>,
) {
    let Some(g) = heroes.iter().next() else {
        return;
    };
    let l = &g.locomotion;
    let stick = input.map_or(guac_motion::stick::Stick::CENTRED, |i| i.0.stick);
    mirror
        .5
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .push(GaitStep {
            step: l.steps(),
            time: time.elapsed_secs_f64(),
            stick: [stick.x, stick.y],
            strength: stick.push(0.0).strength,
            speed_ft: l.speed(),
            run_share: l.run_share(),
            weights: l.weights(),
            rates: [
                l.play(guac_motion::locomotion::Clip::Walk).rate,
                l.play(guac_motion::locomotion::Clip::Run).rate,
            ],
            clip: l.top().name(),
            air: l.air().name(),
            moving: l.moving(),
            held: l.held(),
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
        steps: gaits.locomotion.steps(),
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
            .add_systems(Last, (log_frame, log_drawn));
        // The scripts' run logs where the routes read.
        if let Some(scripts) = app.world().get_resource::<TvMirror>().map(|m| m.7.clone()) {
            app.insert_resource(scripts);
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
    axum::Router::new()
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

/// Depth-first text and images under `root`, the pip's subtree apart.
fn walk(
    root: Entity,
    children: &Query<&Children>,
    texts: &Query<&Text>,
    images: &Query<(), With<ImageNode>>,
    pips: &Query<(&Pip, &BackgroundColor)>,
    out: &mut (Vec<String>, bool, Option<(String, Color)>),
) {
    let Ok(kids) = children.get(root) else {
        return;
    };
    for kid in kids.iter() {
        if let Ok((_, fill)) = pips.get(kid) {
            let mut inner = (Vec::new(), false, None);
            walk(kid, children, texts, images, pips, &mut inner);
            out.2 = Some((inner.0.join(""), fill.0));
            continue;
        }
        if let Ok(text) = texts.get(kid) {
            out.0.push(text.0.clone());
        }
        if images.contains(kid) {
            out.1 = true;
        }
        walk(kid, children, texts, images, pips, out);
    }
}

/// The overlays as the mirror reads them: each one's root, when it was
/// spawned and the camera its UI draws through.
type Overlays<'w, 's> = Query<
    'w,
    's,
    (
        Entity,
        &'static LevelOverlay,
        Option<&'static ShownAt>,
        Option<&'static ComputedUiTargetCamera>,
    ),
>;

/// What the portal draws over the screen's tree (AG.29): the screen roots'
/// visibility, the alert with its lines' root, when it came and its camera,
/// and the corner join code and its QR.
type PortalDrawn<'w, 's> = (
    Query<'w, 's, &'static Visibility, With<ScreenRoot>>,
    Query<
        'w,
        's,
        (
            Entity,
            &'static AlertScreen,
            Option<&'static ShownAt>,
            &'static ComputedNode,
            &'static UiGlobalTransform,
            Option<&'static ComputedUiTargetCamera>,
        ),
    >,
    Query<'w, 's, &'static ImageNode, With<CornerQr>>,
    Query<'w, 's, (), With<CornerJoin>>,
);

/// Every camera with its target, and the level's.
type OverlayCameras<'w, 's> = (
    Query<'w, 's, (&'static Camera, Option<&'static RenderTarget>)>,
    Query<'w, 's, Entity, With<LevelCamera>>,
);

/// The pads in the order they connected, the layer that takes them, and
/// while the level shows, who walks, why it waits and its overlay as drawn,
/// over the level's camera or not.
fn input_shot(
    intro_playing: bool,
    (level, gamepads, overlays, glyphs, (cameras, level_cameras), actions): (
        Option<Res<LevelPads>>,
        Query<(&Gamepad, Option<&Name>, Option<&FacesPrinted>)>,
        Overlays,
        Query<&PromptGlyph>,
        OverlayCameras,
        Option<Res<HeroActions>>,
    ),
    (children, texts, images, pips): (
        &Query<&Children>,
        &Query<&Text>,
        &Query<(), With<ImageNode>>,
        &Query<(&Pip, &BackgroundColor)>,
    ),
) -> InputShot {
    let Some(level) = level else {
        return InputShot::default();
    };
    let seat = level.seat();
    let pads = level
        .order()
        .iter()
        .filter_map(|e| gamepads.get(*e).ok().map(|g| (*e, g)))
        .map(|(entity, (pad, name, printed))| {
            let stick = pad.left_stick();
            PadShot {
                name: name.map(|n| n.as_str().to_owned()).unwrap_or_default(),
                vendor: pad.vendor_id(),
                product: pad.product_id(),
                labels: labels(pad, printed).name().to_owned(),
                stick: [stick.x, stick.y],
                walking: seat.and_then(|s| s.walking()) == Some(entity),
            }
        })
        .collect();
    let overlay = overlays.iter().next().map(|(root, kind, at, target)| {
        let mut found = (Vec::new(), false, None);
        walk(root, children, texts, images, pips, &mut found);
        let ui = target
            .and_then(ComputedUiTargetCamera::get)
            .and_then(|c| cameras.get(c).ok());
        let over_level = ui.is_some_and(|ui| {
            level_cameras
                .iter()
                .filter_map(|c| cameras.get(c).ok())
                .any(|level| draws_over(ui, level))
        });
        OverlayShot {
            kind: kind.name().to_owned(),
            texts: found.0,
            prompt: glyphs.iter().next().map(|g| g.0.name()),
            over_level,
            shown_ms: at.map(|a| a.0.as_secs_f64() * 1e3),
        }
    });
    InputShot {
        layer: Layer::top(intro_playing, level.shows()).name().to_owned(),
        pads,
        pause: seat.and_then(|s| s.pause()).map(|p| {
            match p {
                Pause::Start { .. } => "start",
                Pause::NoPad => "controller",
            }
            .to_owned()
        }),
        overlay,
        hero_stick: seat.map(|s| [s.last().stick.x, s.last().stick.y]),
        hero_actions: seat.and(actions.map(|a| a.held)),
    }
}

fn mirror_tv(
    mirror: Res<TvMirror>,
    current: Option<Res<Current>>,
    assets: Option<Res<TvAssets>>,
    setup: Res<Setup>,
    framing: Res<Framing>,
    portal: Res<PortalCards>,
    camera: Query<&Camera, With<TvCamera>>,
    cards: Query<(Entity, &CardSlot, &ComputedNode, &UiGlobalTransform)>,
    empties: Query<&EmptySlot>,
    children: Query<&Children>,
    texts: Query<&Text>,
    images: Query<(), With<ImageNode>>,
    pips: Query<(&Pip, &BackgroundColor)>,
    (players_line, address, qr, input_focus, choices, pads, portal_drawn): (
        Query<&Text, With<PlayersLine>>,
        Query<&Text, With<JoinAddress>>,
        Query<&ImageNode, With<JoinQr>>,
        Option<Res<InputFocus>>,
        Query<&FocusId>,
        (
            Option<Res<LevelPads>>,
            Query<(&Gamepad, Option<&Name>, Option<&FacesPrinted>)>,
            Overlays,
            Query<&PromptGlyph>,
            OverlayCameras,
            Option<Res<HeroActions>>,
        ),
        PortalDrawn,
    ),
    (extraction, band_title, band_detail, bar): (
        Option<Res<Extraction>>,
        Query<&Text, With<ExtractTitle>>,
        Query<&Text, With<ExtractDetail>>,
        Query<&ExtractBar>,
    ),
    (
        queue,
        player,
        overlays,
        videos,
        turntables,
        plates,
        warm,
        level,
        prepare,
        play,
        heroes,
        cameras,
        (runtime, earning, script_world),
    ): (
        Option<Res<IntroQueue>>,
        Option<Res<IntroPlayer>>,
        Query<(&ComputedNode, &UiGlobalTransform), With<IntroOverlay>>,
        Query<(&ComputedNode, &UiGlobalTransform), With<IntroVideo>>,
        Option<Res<Turntables>>,
        Query<(&CardPlate, &ComputedNode, &UiGlobalTransform)>,
        Option<Res<Warm>>,
        Option<Res<Level>>,
        Option<Res<Prepare>>,
        Option<Res<Play>>,
        Query<(&HeroBody, Option<&HeroGaits>, Option<&HeroRun>)>,
        Query<(&Transform, &Projection), With<FollowCamera>>,
        (
            Option<Res<guacamole::scripts::ScriptRuntime>>,
            Option<Res<crate::progress::Earning>>,
            Option<Res<guacamole::scripts::ScriptWorld>>,
        ),
    ),
) {
    let Some(current) = current else {
        return;
    };
    let size = camera
        .single()
        .ok()
        .and_then(Camera::physical_target_size)
        .unwrap_or_default();
    let safe = safe_area(size.x, size.y, framing.0);
    let (screens, alerts, corner_qrs, corners) = portal_drawn;
    // The screen's tree hidden (the level over it, AG.50) draws none of its
    // cards, lines or QR.
    let screen_shown = screens.iter().all(|v| *v != Visibility::Hidden);
    let mut drawn: Vec<(CardShot, Rect)> = cards
        .iter()
        .filter(|_| screen_shown)
        .map(|(entity, slot, node, at)| {
            let mut found = (Vec::new(), false, None);
            walk(entity, &children, &texts, &images, &pips, &mut found);
            let placement = portal.slots.get(slot.0 as usize).and_then(Option::as_ref);
            let rect = node_rect(node, at);
            let shot = CardShot {
                slot: slot.0 + 1,
                figure_id: placement
                    .map(|p| p.figure_id.as_str().to_string())
                    .unwrap_or_default(),
                texts: found.0,
                portrait: found.1,
                state: turntables
                    .as_ref()
                    .and_then(|t| t.card_state(slot.0))
                    .unwrap_or(CardState::Portrait)
                    .name()
                    .into(),
                reason: turntables
                    .as_ref()
                    .and_then(|t| t.portrait_reason(slot.0))
                    .map(str::to_owned),
                plate: plates
                    .iter()
                    .find(|(p, ..)| p.slot == slot.0)
                    .map(|(_, n, t)| {
                        let r = node_rect(n, t);
                        [r.min.x, r.min.y, r.width(), r.height()]
                    }),
                pip: found.2.map(|(initial, fill)| PipShot {
                    initial,
                    color: Srgba::from(fill).to_hex(),
                    profile_id: placement
                        .and_then(|p| p.owner.as_ref())
                        .map(|o| o.profile_id.clone())
                        .unwrap_or_default(),
                }),
                rect: [rect.min.x, rect.min.y, rect.width(), rect.height()],
            };
            (shot, rect)
        })
        .collect();
    drawn.sort_by_key(|(c, _)| c.slot);
    let cards_outside = drawn
        .iter()
        .filter(|(_, r)| !contains(safe, *r))
        .map(|(c, _)| c.slot)
        .collect();
    let overlapping = drawn
        .iter()
        .enumerate()
        .flat_map(|(i, (_, a))| drawn[i + 1..].iter().map(move |(_, b)| (*a, *b)))
        .filter(|(a, b)| !a.intersect(*b).is_empty())
        .count();
    let mut empty_slots: Vec<u8> = empties
        .iter()
        .filter(|_| screen_shown)
        .map(|e| e.0 + 1)
        .collect();
    empty_slots.sort();
    let (game, message) = match &current.screen {
        TvScreen::Portal { game } => (Some(game.clone()), None),
        TvScreen::ServerError { message } => (None, Some(message.clone())),
        _ => (None, None),
    };
    let status = extraction.map(|e| e.status.clone()).unwrap_or_default();
    let extraction = ExtractionShot {
        state: status.kind().to_string(),
        code: match &status {
            ExtractStatus::Failed(f) => Some(f.code().to_string()),
            _ => None,
        },
        converted: match &status {
            ExtractStatus::Converting { done, total, .. } => Some([*done, *total]),
            _ => None,
        },
        intros: match &status {
            ExtractStatus::Ready { intros, .. } => Some(*intros),
            _ => None,
        },
        ran: match &status {
            ExtractStatus::Ready { ran, .. } => Some(*ran),
            _ => None,
        },
        title: band_title.iter().next().map(|t| t.0.clone()),
        detail: band_detail.iter().next().map(|t| t.0.clone()),
        percent: bar.iter().next().map(|b| b.0),
    };
    let window = Rect::new(0.0, 0.0, size.x as f32, size.y as f32);
    let intro = queue.as_ref().and_then(|q| q.playing()).map(|entry| {
        let now = player
            .as_ref()
            .and_then(|p| p.now())
            .filter(|n| n.entry.id == entry.id);
        let full_screen = overlays.iter().any(|(node, at)| {
            let r = node_rect(node, at);
            // A pixel of slack, as for the cards.
            (r.min - window.min).abs().max_element() <= 1.0
                && (r.max - window.max).abs().max_element() <= 1.0
        });
        IntroShot {
            slot: entry.slot.as_u8() + 1,
            figure_id: entry.figure_id.clone(),
            movie: entry.movie.clone(),
            frames: now.as_ref().and_then(|n| n.frames),
            shown: now.as_ref().map_or(0, |n| n.shown),
            skipped: now.as_ref().map_or(0, |n| n.skipped),
            audio: now.as_ref().map_or("waiting", |n| n.audio).to_string(),
            full_screen,
            video_rect: videos.iter().next().map(|(node, at)| {
                let r = node_rect(node, at);
                [r.min.x, r.min.y, r.width(), r.height()]
            }),
        }
    });
    let intro_queue = queue
        .as_ref()
        .map(|q| q.queued().map(QueuedShot::from).collect())
        .unwrap_or_default();
    let mode = match (play.as_deref(), pads.0.as_deref()) {
        (Some(play), Some(level_pads)) => play.mode(level_pads),
        _ => Mode::Portal,
    };
    let hero = play
        .as_deref()
        .and_then(|p| p.report(&heroes, &cameras, runtime.as_deref(), earning.as_deref()));
    let alert = alerts
        .iter()
        .next()
        .map(|(root, kind, at, node, place, target)| {
            let mut found = (Vec::new(), false, None);
            walk(root, &children, &texts, &images, &pips, &mut found);
            let r = node_rect(node, place);
            let (overlay_cameras, level_cameras) = &pads.4;
            let ui = target
                .and_then(ComputedUiTargetCamera::get)
                .and_then(|c| overlay_cameras.get(c).ok());
            AlertShot {
                kind: kind.0.name().to_owned(),
                texts: found.0,
                full_screen: (r.min - window.min).abs().max_element() <= 1.0
                    && (r.max - window.max).abs().max_element() <= 1.0,
                over_level: ui.is_some_and(|ui| {
                    level_cameras
                        .iter()
                        .filter_map(|c| overlay_cameras.get(c).ok())
                        .any(|level| draws_over(ui, level))
                }),
                shown_ms: at.map(|a| a.0.as_secs_f64() * 1e3),
            }
        });
    let qr_handle = assets.and_then(|a| a.qr.clone());
    let join_qr = qr_handle.is_some_and(|handle| {
        (screen_shown && qr.iter().any(|image| image.image == handle))
            || corner_qrs.iter().any(|image| image.image == handle)
    });
    let input = input_shot(intro.is_some(), pads, (&children, &texts, &images, &pips));
    let loot = match (runtime.as_deref(), script_world.as_deref()) {
        (Some(rt), Some(w)) => {
            use guacamole::scripts::{Rarity, loot_index};
            let shot = LootShot {
                common: loot_index(&rt.vm, w, Rarity::Common),
                uncommon: loot_index(&rt.vm, w, Rarity::Uncommon),
                rare: loot_index(&rt.vm, w, Rarity::Rare),
            };
            (shot != LootShot::default()).then_some(shot)
        }
        _ => None,
    };
    let snapshot = TvSnapshot {
        screen: current.screen.kind().to_string(),
        game,
        message,
        players: players_line
            .iter()
            .next()
            .filter(|_| screen_shown)
            .and_then(|t| t.0.split_whitespace().next()?.parse().ok()),
        join_address: address
            .iter()
            .next()
            .filter(|_| screen_shown)
            .map(|t| t.0.clone()),
        join_qr,
        found: setup.0.found_titles.clone(),
        folders: setup.0.folders.clone(),
        problems: setup.0.problems.clone(),
        focus: input_focus
            .and_then(|f| f.get())
            .and_then(|e| choices.get(e).ok())
            .map(|id| id.0.to_string()),
        cards: drawn.into_iter().map(|(c, _)| c).collect(),
        empty_slots,
        window: [size.x as f32, size.y as f32],
        cards_outside,
        overlapping,
        extraction,
        intro,
        intro_queue,
        warm: warm.map(|w| w.report().clone()),
        level: level.map(|l| l.snapshot()),
        prepare: prepare.map(|p| p.report().clone()),
        input,
        mode: mode.name().to_owned(),
        hero,
        alert,
        join_corner: !corners.is_empty(),
        loot,
    };
    let mut shared = mirror.0.lock().unwrap_or_else(|e| e.into_inner());
    if *shared != snapshot {
        *shared = snapshot;
    }
}

#[cfg(test)]
mod tests {
    use guac_server::config::WindowMode;
    use skylander_core::SlotIndex;

    use super::*;
    use crate::driver::PortalChange;
    use crate::tv::layout_tests::{placement, portal_app};

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
            .query_filtered::<&mut Visibility, With<crate::tv::ScreenRoot>>();
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
        use crate::input::heard::{listen, take};
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
        app.insert_resource(mirror.clone());
        listen(&mut app);
        app.update();
        assert_eq!(take(&mut app), [TvInput::Activate]);
        assert_eq!(mirror.2.len(), 0);
        app.update();
        assert_eq!(take(&mut app), []);
    }
}
