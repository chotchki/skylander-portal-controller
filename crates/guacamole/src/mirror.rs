//! `/api/_test/tv` (design decision 8): a `PostUpdate` copy of what the TV
//! draws, so the e2e suite asserts on the TV without screenshots. Mounted
//! only under `test-hooks`. Read off the laid-out UI tree: the players line,
//! join address, join QR and every card's lines, portrait, pip and rect.
//! Copied from the resources that feed the tree: the screen, game, message,
//! Setup's lists, each card's figure and owner ids, and the intro queue. The
//! focused choice is `InputFocus`'s entity's `FocusId`. The turntables'
//! warm (AE.14), the level's load (AG.19), the start's pass (AE.25) and
//! the pads as the level's layer reads them (AG.20) ride along,
//! `/api/_test/tv/frames` hands out the TV's frame times for the stall check
//! and `/api/_test/tv/pad` drives pads as a hand would, no hardware needed.

use std::collections::VecDeque;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use axum::extract::rejection::JsonRejection;
use bevy::input::InputSystems;
use bevy::input::gamepad::{
    GamepadButton, GamepadConnection, GamepadConnectionEvent, RawGamepadAxisChangedEvent,
    RawGamepadButtonChangedEvent, RawGamepadEvent,
};
use bevy::input_focus::InputFocus;
use bevy::prelude::*;
use bevy::ui::{ComputedNode, UiGlobalTransform, UiSystems};
use serde::{Deserialize, Serialize};

use crate::cards::PortalCards;
use crate::extraction::{ExtractStatus, Extraction};
use crate::input::Layer;
use crate::input::{FocusId, PostedPresses, TestPress};
use crate::intro::{IntroEntry, IntroEvent, IntroQueue};
use crate::intro_player::{self, IntroOverlay, IntroPlayer, IntroVideo};
use crate::level::{Level, LevelReport};
use crate::pad::{LevelPads, Pause, labels};
use crate::pause::{LevelOverlay, PromptGlyph};
use crate::prepare::{Prepare, PrepareReport};
use crate::screen::TvScreen;
use crate::turntable::{CardPlate, CardState, Turntables};
use crate::tv::{
    CardSlot, Current, EmptySlot, ExtractBar, ExtractDetail, ExtractTitle, Framing, JoinAddress,
    JoinQr, Pip, PlayersLine, Setup, TvAssets, TvCamera, contains, node_rect, safe_area,
};
use crate::warm::{PipelineWatch, Warm, WarmReport};

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
/// presses posted for the input router, the frame log and the posted pads.
#[derive(Resource, Clone, Default)]
pub struct TvMirror(
    pub Arc<Mutex<TvSnapshot>>,
    pub Arc<AtomicUsize>,
    pub(crate) PostedPresses,
    pub(crate) Arc<Mutex<FrameLog>>,
    pub(crate) PostedPads,
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
        _ => return None,
    })
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
        } else {
            PadAct::Press(pad_button(self.press.as_deref()?)?)
        };
        Some(PadPost { pad: self.pad, act })
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
    mut pads: Local<[Option<Entity>; TEST_PADS]>,
    mut up: Local<Vec<(Entity, GamepadButton)>>,
    mut connections: MessageWriter<GamepadConnectionEvent>,
    mut raw: MessageWriter<RawGamepadEvent>,
) {
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

pub struct Mirror;

impl Plugin for Mirror {
    fn build(&self, app: &mut App) {
        app.add_systems(PostUpdate, mirror_tv.after(UiSystems::Layout))
            .add_systems(Update, skip_asked.before(intro_player::play_intros))
            .add_systems(PreUpdate, drive_posted_pads.before(InputSystems))
            .add_systems(Last, log_frame);
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
/// the last frame's end. Generic so a test needs no `AppState`.
pub fn routes<S: Clone + Send + Sync + 'static>(mirror: &TvMirror) -> axum::Router<S> {
    let shared = mirror.0.clone();
    let skips = mirror.1.clone();
    let presses = mirror.2.clone();
    let frames = mirror.3.clone();
    let pads = mirror.4.clone();
    axum::Router::new()
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

/// The pads in the order they connected, the layer that takes them, and
/// while the level shows, who walks, why it waits and its overlay as drawn.
fn input_shot(
    intro_playing: bool,
    (level, gamepads, overlays, glyphs): (
        Option<Res<LevelPads>>,
        Query<(&Gamepad, Option<&Name>)>,
        Query<(Entity, &LevelOverlay)>,
        Query<&PromptGlyph>,
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
        .map(|(entity, (pad, name))| {
            let stick = pad.left_stick();
            PadShot {
                name: name.map(|n| n.as_str().to_owned()).unwrap_or_default(),
                vendor: pad.vendor_id(),
                product: pad.product_id(),
                labels: labels(pad).name().to_owned(),
                stick: [stick.x, stick.y],
                walking: seat.and_then(|s| s.walking()) == Some(entity),
            }
        })
        .collect();
    let overlay = overlays.iter().next().map(|(root, kind)| {
        let mut found = (Vec::new(), false, None);
        walk(root, children, texts, images, pips, &mut found);
        OverlayShot {
            kind: kind.name().to_owned(),
            texts: found.0,
            prompt: glyphs.iter().next().map(|g| g.0.name()),
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
    (players_line, address, qr, input_focus, choices, pads): (
        Query<&Text, With<PlayersLine>>,
        Query<&Text, With<JoinAddress>>,
        Query<&ImageNode, With<JoinQr>>,
        Option<Res<InputFocus>>,
        Query<&FocusId>,
        (
            Option<Res<LevelPads>>,
            Query<(&Gamepad, Option<&Name>)>,
            Query<(Entity, &LevelOverlay)>,
            Query<&PromptGlyph>,
        ),
    ),
    (extraction, band_title, band_detail, bar): (
        Option<Res<Extraction>>,
        Query<&Text, With<ExtractTitle>>,
        Query<&Text, With<ExtractDetail>>,
        Query<&ExtractBar>,
    ),
    (queue, player, overlays, videos, turntables, plates, warm, level, prepare): (
        Option<Res<IntroQueue>>,
        Option<Res<IntroPlayer>>,
        Query<(&ComputedNode, &UiGlobalTransform), With<IntroOverlay>>,
        Query<(&ComputedNode, &UiGlobalTransform), With<IntroVideo>>,
        Option<Res<Turntables>>,
        Query<(&CardPlate, &ComputedNode, &UiGlobalTransform)>,
        Option<Res<Warm>>,
        Option<Res<Level>>,
        Option<Res<Prepare>>,
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
    let mut drawn: Vec<(CardShot, Rect)> = cards
        .iter()
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
    let mut empty_slots: Vec<u8> = empties.iter().map(|e| e.0 + 1).collect();
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
    let input = input_shot(intro.is_some(), pads, (&children, &texts, &images, &pips));
    let snapshot = TvSnapshot {
        screen: current.screen.kind().to_string(),
        game,
        message,
        players: players_line
            .iter()
            .next()
            .and_then(|t| t.0.split_whitespace().next()?.parse().ok()),
        join_address: address.iter().next().map(|t| t.0.clone()),
        join_qr: assets
            .and_then(|a| a.qr.clone())
            .is_some_and(|handle| qr.iter().any(|image| image.image == handle)),
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
        level: level.map(|l| l.report().clone()),
        prepare: prepare.map(|p| p.report().clone()),
        input,
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

    /// AG.20: `/api/_test/tv/pad` takes one act a body for pads 0 to 3 and
    /// refuses the rest; the acts drive a pad through Bevy's own gamepad
    /// systems, so its stick lands raw, and with the level shown that pad
    /// walks, Start pauses under the overlay as drawn, losing the pad brings
    /// the controller message, and another pad's press carries on with it.
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
            (r#"{"pad":0,"hold":"south"}"#, 400),
            ("pad", 400),
        ] {
            assert_eq!(post(body).await, status, "{body}");
        }

        let mut app = portal_app(1920, 1080, WindowMode::Tv, PortalCards::default());
        app.insert_resource(mirror.clone()).add_plugins(Mirror);
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

        app.world_mut()
            .resource_mut::<crate::pad::LevelPads>()
            .show();
        let shot = until(&mut app, "walking", &|s| s.pads[0].walking);
        assert_eq!(shot.layer, "level");
        assert_eq!(shot.hero_stick, Some([0.26, -0.25]));
        assert_eq!((shot.pause, shot.overlay), (None, None));

        assert_eq!(post(r#"{"pad":0,"press":"start"}"#).await, 202);
        let shot = until(&mut app, "paused", &|s| s.overlay.is_some());
        assert_eq!(shot.pause.as_deref(), Some("start"));
        assert_eq!(
            shot.overlay,
            Some(OverlayShot {
                kind: "paused".into(),
                texts: vec!["Paused".into(), "Press".into(), "to play on".into()],
                prompt: Some("position:south".into()),
            })
        );

        assert_eq!(post(r#"{"pad":0,"disconnect":true}"#).await, 202);
        let shot = until(&mut app, "lost", &|s| {
            s.pause.as_deref() == Some("controller")
        });
        assert!(shot.pads.is_empty());
        let shot = until(&mut app, "its message", &|s| {
            s.overlay.as_ref().is_some_and(|o| o.kind == "controller")
        });
        assert_eq!(
            shot.overlay.unwrap().texts,
            ["Connect a controller", "then press any button on it"]
        );

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
