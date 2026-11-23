//! A capture asks for its note (openspec AS.805): a hand's capture press
//! ([`crate::flags`]' pad button or key) takes the capture as before, then
//! pauses the game and puts up a prompt over it: the frame just captured, a
//! text box with the keyboard (Bevy's own [`TextInput`]), Save and Cancel.
//!
//! - Save (Enter, the pad's bottom button, or the capture button again)
//!   writes the text into the capture's `capture.json` `note` and its
//!   `note.txt`, as the phone's typed note is kept; an empty one leaves the
//!   capture as it was. Cancel (Esc or the pad's right button) deletes the
//!   capture's own files and its folder ([`delete_capture`]: never a tree),
//!   unless a phone's note joined it meanwhile. Either way the game goes on.
//! - Paused is the level's clock stopped, as Start's pause stops it
//!   ([`crate::pause::hold_time`]): the scripts' step, the physics, the
//!   hero, creatures, the camera, particles and the scripts' sounds run
//!   off it. What plays by the wall clock (an intro, the level end's movie)
//!   stops its steps ([`WallClockPlayback`]) and holds its sound, its
//!   clocks moved on by the time held after ([`Playback::resume`]), so
//!   nothing catches up. The portal's play time and the TV's own UI
//!   animations go on: they run on the real clock, as under Start's pause.
//! - While it's up the router hands it the keys and pads alone
//!   ([`crate::input`]), and the server, on its own thread, answers the
//!   phone as ever.
//! - Never on a run nobody sits at: not under `GUAC_TEST_POSTED_PADS_ONLY`
//!   ([`crate::flags::Flags::prompts`]), not for the posted key or the
//!   phone ([`asks_for_note`]), but for a press the test route asks for as
//!   a hand's (`POST /api/_test/flag/hand`), so a TV run can look at it.
//!
//! In `test-hooks` builds (and the tests) only, as the capture is: a build
//! without the feature compiles none of this.

use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Instant, SystemTime};

use axum::extract::rejection::JsonRejection;
use axum::response::IntoResponse;
use bevy::audio::AudioSink;
use bevy::ecs::system::SystemParam;
use bevy::input::ButtonState;
use bevy::input::gamepad::GamepadButton;
use bevy::input::keyboard::{Key, KeyboardInput};
use bevy::input_focus::InputFocus;
use bevy::prelude::*;
use bevy::render::view::window::screenshot::ScreenshotCaptured;
use bevy::text::{EditableText, LineBreak, TextCursorStyle};
use bevy::ui_widgets::{Activate, Button, TextInput};
use serde::{Deserialize, Serialize};
use tracing::{error, info, warn};

use crate::flags::{FLAG_BUTTON, FlagAsks, FlagState, Source, note_ok};
use crate::input::FocusId;
use crate::intro_player::{IntroPlayer, Playback, WallClockPlayback};
use crate::movies::Movies;
use crate::pad::{Face, FacesPrinted, Labels, LevelPads, labels, prompt};
use crate::theme::{Role, Step, TvTheme};
use crate::tv_state::{Pen, Redrawn, TvAssets};

/// Over everything the TV draws, an intro's overlay (1,000) included.
pub(crate) const NOTE_Z: i32 = 1_100;

/// The captured frame on the prompt, design px (16:9).
const FRAME_W: f32 = 768.0;
const FRAME_H: f32 = 432.0;

/// The prompt's panel, design px.
const PANEL_W: f32 = 1240.0;

/// The text box shows this many lines, then scrolls.
const NOTE_LINES: f32 = 3.0;

/// The most characters it takes: [`crate::flags::NOTE_MAX`] bytes of any text.
const NOTE_CHARS: usize = crate::flags::NOTE_MAX / 4;

/// The text box's focus id: [`crate::input`]'s `keep_focus` leaves the focus
/// on it while the prompt is up.
const NOTE_FOCUS: &str = "capture-note";

/// A cancelled capture's folder is tried again for this many frames when it
/// won't go at once (its frame still being written).
const DELETE_TRIES: u32 = 300;

/// Save keeps the capture with what was typed as its note; Cancel deletes
/// its folder.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Answer {
    Save,
    Cancel,
}

/// An open prompt: its capture's folder, the pad that opened it and the
/// entities it drew.
#[derive(Debug)]
pub(crate) struct Open {
    folder: PathBuf,
    name: String,
    /// The pad that pressed, whose labels the prompt's glyphs take.
    by: Option<Entity>,
    /// Opened this frame: the presses that opened it answer nothing.
    fresh: bool,
    root: Option<Entity>,
    input: Option<Entity>,
    slot: Option<Entity>,
    /// The captured frame, once it lands, till it's on the prompt.
    frame: Option<Handle<Image>>,
    /// The captured frame is on the prompt.
    shown: bool,
    /// Where the focus was, given back after.
    focus_before: Option<Entity>,
    /// Save or Cancel pressed on screen.
    clicked: Option<Answer>,
}

/// The open prompt if any, whether it stopped the level's clock and since
/// when it holds the playbacks, cancelled folders still to delete, and the
/// answers given.
#[derive(Resource, Debug, Default)]
pub(crate) struct NoteOpen {
    open: Option<Open>,
    /// The level's clock is stopped by the prompt (not by Start's pause).
    clock_held: bool,
    /// The wall-clock playbacks held since.
    playback_held: Option<Instant>,
    /// Cancelled folders that didn't go yet, with tries left.
    deleting: Vec<(PathBuf, u32)>,
    /// Every answer given, in order: the folder and what was done.
    pub(crate) answered: Vec<(String, Answer)>,
}

impl NoteOpen {
    pub(crate) fn is_open(&self) -> bool {
        self.open.is_some()
    }
}

/// A capture press by the route asked to be a hand's, this frame.
#[derive(Resource, Debug, Default)]
pub(crate) struct HandNow(bool);

/// The rule: a capture asks for its note when it was a pad's or the key's
/// and either the route asked for it as a hand's (`hand`), or prompts are on
/// for this run (`prompts`: no `GUAC_TEST_POSTED_PADS_ONLY`) and the key
/// wasn't the posted one (`posted`). The phone's capture carries its own
/// typed note: never.
pub(crate) fn asks_for_note(source: &Source, prompts: bool, posted: bool, hand: bool) -> bool {
    match source {
        Source::Phone => false,
        Source::Key | Source::Pad(_) => hand || (prompts && !posted),
    }
}

/// `crate::flags`' `take_flags`' side of the prompt.
#[derive(SystemParam)]
pub(crate) struct NoteTake<'w> {
    note: ResMut<'w, NoteOpen>,
    hand: ResMut<'w, HandNow>,
}

impl NoteTake<'_> {
    pub(crate) fn is_open(&self) -> bool {
        self.note.is_open()
    }

    /// Whether this frame's press was the route's hand press (read once).
    pub(crate) fn hand_asked(&mut self) -> bool {
        std::mem::take(&mut self.hand.0)
    }

    /// Opens the prompt on the capture `name` in `folder`, whose frame
    /// lands on `shot`'s observer.
    pub(crate) fn open(
        &mut self,
        commands: &mut Commands,
        folder: PathBuf,
        name: String,
        shot: Entity,
        by: Option<Entity>,
    ) {
        info!(folder = %name, "a capture asks for its note: the game paused");
        commands.entity(shot).observe(frame_landed(folder.clone()));
        self.note.open = Some(Open {
            folder,
            name,
            by,
            fresh: true,
            root: None,
            input: None,
            slot: None,
            frame: None,
            shown: false,
            focus_before: None,
            clicked: None,
        });
    }
}

/// The captured frame onto the prompt that's still up for `folder`, at a
/// third of its size.
fn frame_landed(
    folder: PathBuf,
) -> impl FnMut(On<ScreenshotCaptured>, ResMut<NoteOpen>, Option<ResMut<Assets<Image>>>) {
    move |captured, mut note, images| {
        let (Some(open), Some(mut images)) = (note.open.as_mut(), images) else {
            return;
        };
        if open.folder != folder {
            return;
        }
        match thumbnail(&captured.image) {
            Ok(image) => open.frame = Some(images.add(image)),
            Err(why) => warn!(%why, "the capture's frame won't show on its prompt"),
        }
    }
}

/// `shot` as an RGBA image [`FRAME_W`] wide, for the prompt.
fn thumbnail(shot: &Image) -> Result<Image, String> {
    let image = shot.clone().try_into_dynamic().map_err(|e| e.to_string())?;
    let small = image.resize(
        FRAME_W as u32,
        FRAME_H as u32,
        image::imageops::FilterType::Triangle,
    );
    Ok(Image::from_dynamic(
        image::DynamicImage::ImageRgba8(small.to_rgba8()),
        true,
        bevy::asset::RenderAssetUsages::RENDER_WORLD,
    ))
}

/// Writes `note` into the capture in `folder`: `capture.json`'s `note` and a
/// line of `note.txt`. An empty (or blank) note writes nothing.
pub(crate) fn save_note(folder: &Path, note: &str, at: SystemTime) -> Result<(), String> {
    if note.trim().is_empty() {
        return Ok(());
    }
    let path = folder.join("capture.json");
    let mut json: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&path).map_err(|e| e.to_string())?)
            .map_err(|e| e.to_string())?;
    json["note"] = serde_json::Value::String(note.to_owned());
    let bytes = serde_json::to_vec_pretty(&json).map_err(|e| e.to_string())?;
    std::fs::write(&path, bytes).map_err(|e| e.to_string())?;
    crate::flags::add_note(folder, note, at).map_err(|e| e.to_string())
}

/// What a capture's folder holds ([`crate::flags`]' `capture`): the files
/// Cancel deletes, and the only ones.
const CAPTURE_FILES: [&str; 4] = ["capture.json", "log.txt", "note.txt", "frame.png"];

/// Deletes the capture in `folder`: its own files by name, then the folder,
/// only if that leaves it empty. Never a tree: a folder holding anything
/// else (another capture, a subfolder) stays with that in it, and a symlink
/// is refused, so a Cancel can't reach past the one capture it was asked
/// for. A folder already gone is done.
pub(crate) fn delete_capture(folder: &Path) -> std::io::Result<()> {
    use std::io::ErrorKind;
    let meta = match std::fs::symlink_metadata(folder) {
        Err(e) if e.kind() == ErrorKind::NotFound => return Ok(()),
        meta => meta?,
    };
    if !meta.is_dir() {
        return Err(std::io::Error::other("not a capture's folder"));
    }
    for name in CAPTURE_FILES {
        match std::fs::remove_file(folder.join(name)) {
            Err(e) if e.kind() != ErrorKind::NotFound => return Err(e),
            _ => {}
        }
    }
    std::fs::remove_dir(folder)
}

/// A route's post: characters typed into the box, or a named key
/// (Enter saves, Esc cancels, Backspace deletes).
#[derive(Debug, Clone, PartialEq, Eq)]
enum Posted {
    /// Characters, one key press each, all in one frame.
    Text(String),
    Key(Named),
}

/// The keys the route names.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
enum Named {
    Enter,
    Escape,
    Backspace,
}

/// What `GET /api/_test/flag/note` answers.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
struct NoteShot {
    open: bool,
    /// The capture's folder, by name.
    folder: Option<String>,
    /// What the box holds.
    text: String,
    /// The captured frame is on it.
    frame: bool,
    /// The level's clock is stopped.
    paused: bool,
    /// Every answer given: `[folder, "save" | "cancel"]`.
    answered: Vec<(String, &'static str)>,
}

/// The routes' side: the hand presses and keys posted, and the prompt read.
#[derive(Resource, Clone, Default)]
pub(crate) struct NoteAsks {
    hand: Arc<AtomicUsize>,
    keys: Arc<Mutex<VecDeque<Posted>>>,
    shot: Arc<Mutex<NoteShot>>,
}

impl NoteAsks {
    fn push(&self, posted: Posted) {
        self.keys
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .push_back(posted);
    }

    fn pop(&self) -> Option<Posted> {
        self.keys
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .pop_front()
    }

    fn shot(&self) -> NoteShot {
        self.shot.lock().unwrap_or_else(|e| e.into_inner()).clone()
    }
}

/// `POST /api/_test/flag/note`'s body: `{"type": ".."}` or `{"key":
/// "enter" | "escape" | "backspace"}`.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct NoteBody {
    #[serde(rename = "type")]
    text: Option<String>,
    key: Option<Named>,
}

/// `POST /api/_test/flag/hand` presses the capture key as a hand would, so
/// the capture asks for its note even on an unattended run (202; 409 with
/// flags off). `GET /api/_test/flag/note` reads the prompt ([`NoteShot`]).
/// `POST /api/_test/flag/note` types into it or presses Enter, Esc or
/// Backspace (202; 409 with no prompt up, 400 for any other body or text
/// [`note_ok`] refuses or a line break).
pub(crate) fn routes<S: Clone + Send + Sync + 'static>(flags: &FlagAsks) -> axum::Router<S> {
    use axum::http::StatusCode;
    let (hand, read, post) = (flags.clone(), flags.note.clone(), flags.note.clone());
    axum::Router::new()
        .route(
            "/api/_test/flag/hand",
            axum::routing::post(move || {
                let on = hand.on();
                if on {
                    hand.note.hand.fetch_add(1, Ordering::Relaxed);
                }
                async move {
                    if on {
                        StatusCode::ACCEPTED
                    } else {
                        StatusCode::CONFLICT
                    }
                }
            }),
        )
        .route(
            "/api/_test/flag/note",
            axum::routing::get(move || {
                let shot = read.shot();
                async move { axum::Json(shot) }
            })
            .post(move |body: Result<axum::Json<NoteBody>, JsonRejection>| {
                let asks = post.clone();
                async move {
                    let posted = match body {
                        Ok(axum::Json(NoteBody {
                            text: Some(t),
                            key: None,
                        })) if note_ok(&t) && !t.contains(['\n', '\r']) && !t.is_empty() => {
                            Posted::Text(t)
                        }
                        Ok(axum::Json(NoteBody {
                            text: None,
                            key: Some(k),
                        })) => Posted::Key(k),
                        _ => return StatusCode::BAD_REQUEST.into_response(),
                    };
                    if !asks.shot().open {
                        return StatusCode::CONFLICT.into_response();
                    }
                    asks.push(posted);
                    StatusCode::ACCEPTED.into_response()
                }
            }),
        )
}

pub(crate) struct NotePlugin;

impl Plugin for NotePlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<NoteOpen>()
            .init_resource::<HandNow>()
            .init_resource::<NoteAsks>()
            .add_message::<Redrawn>()
            .configure_sets(
                Update,
                WallClockPlayback.run_if(|note: Res<NoteOpen>| !note.is_open()),
            )
            .add_systems(
                PreUpdate,
                (
                    feed_posted.before(bevy::input::InputSystems),
                    hold.after(crate::pause::hold_time),
                ),
            )
            .add_systems(Update, show)
            .add_systems(
                Last,
                (answer, hold, publish)
                    .chain()
                    .after(crate::flags::TakeFlags),
            );
    }
}

/// The route's posts into Bevy's keyboard stream as the window's keys would
/// arrive: the hand press ([`crate::flags::FLAG_KEY`]), typed characters
/// and named keys, each released the frame after.
fn feed_posted(
    asks: Res<NoteAsks>,
    window: Query<Entity, With<bevy::window::PrimaryWindow>>,
    mut keys: MessageWriter<KeyboardInput>,
    mut hand: ResMut<HandNow>,
    mut down: Local<Vec<(KeyCode, Key)>>,
) {
    let window = window.iter().next().unwrap_or(Entity::PLACEHOLDER);
    for (key_code, logical_key) in down.drain(..) {
        keys.write(KeyboardInput {
            key_code,
            logical_key,
            state: ButtonState::Released,
            text: None,
            repeat: false,
            window,
        });
    }
    let mut press = |key_code: KeyCode, logical_key: Key, text: Option<&str>| {
        keys.write(KeyboardInput {
            key_code,
            logical_key: logical_key.clone(),
            state: ButtonState::Pressed,
            text: text.map(Into::into),
            repeat: false,
            window,
        });
        down.push((key_code, logical_key));
    };
    if asks.hand.swap(0, Ordering::Relaxed) > 0 {
        hand.0 = true;
        press(
            crate::flags::FLAG_KEY,
            Key::Character("f".into()),
            Some("f"),
        );
    }
    let Some(posted) = asks.pop() else {
        return;
    };
    match posted {
        Posted::Text(text) => {
            for c in text.chars() {
                let s = c.to_string();
                let logical = if c == ' ' {
                    Key::Space
                } else {
                    Key::Character(s.as_str().into())
                };
                press(key_code_of(c), logical, Some(&s));
            }
        }
        Posted::Key(Named::Enter) => press(KeyCode::Enter, Key::Enter, None),
        Posted::Key(Named::Escape) => press(KeyCode::Escape, Key::Escape, None),
        Posted::Key(Named::Backspace) => press(KeyCode::Backspace, Key::Backspace, None),
    }
}

/// A character's key on a US layout, as near as the route needs: letters,
/// digits and space; anything else unidentified.
fn key_code_of(c: char) -> KeyCode {
    use KeyCode::*;
    const LETTERS: [KeyCode; 26] = [
        KeyA, KeyB, KeyC, KeyD, KeyE, KeyF, KeyG, KeyH, KeyI, KeyJ, KeyK, KeyL, KeyM, KeyN, KeyO,
        KeyP, KeyQ, KeyR, KeyS, KeyT, KeyU, KeyV, KeyW, KeyX, KeyY, KeyZ,
    ];
    const DIGITS: [KeyCode; 10] = [
        Digit0, Digit1, Digit2, Digit3, Digit4, Digit5, Digit6, Digit7, Digit8, Digit9,
    ];
    match c.to_ascii_lowercase() {
        l @ 'a'..='z' => LETTERS[(l as u8 - b'a') as usize],
        d @ '0'..='9' => DIGITS[(d as u8 - b'0') as usize],
        ' ' => Space,
        _ => Unidentified(bevy::input::keyboard::NativeKeyCode::Unidentified),
    }
}

/// Keeps the level's clock stopped and the wall-clock playbacks held while
/// the prompt is up, and starts them again after, unless Start's pause or
/// the portal's hold has the level waiting by then ([`LevelPads::waiting`],
/// whose own [`crate::pause::hold_time`] then lets it go). Runs after
/// `hold_time` each frame, which may have let the clock go under it, and in
/// `Last` as the prompt opens and closes, so the frame after a press steps
/// nothing and the frame after an answer steps as ever.
fn hold(
    mut note: ResMut<NoteOpen>,
    pads: Option<Res<LevelPads>>,
    mut time: ResMut<Time<Virtual>>,
    (intro, movies): (Option<ResMut<IntroPlayer>>, Option<ResMut<Movies>>),
    sinks: Query<&AudioSink>,
) {
    let mut intro = intro;
    let mut movies = movies;
    let mut playbacks = |f: &mut dyn FnMut(&mut Playback)| {
        if let Some(p) = intro.as_mut().and_then(|i| i.playback_mut()) {
            f(p);
        }
        if let Some(p) = movies.as_mut().and_then(|m| m.playback_mut()) {
            f(p);
        }
    };
    if note.is_open() {
        if !time.is_paused() {
            time.pause();
            note.clock_held = true;
        }
        playbacks(&mut |p| p.hold(&sinks));
        if note.playback_held.is_none() {
            note.playback_held = Some(Instant::now());
        }
        return;
    }
    if note.clock_held {
        note.clock_held = false;
        if !pads.as_deref().is_some_and(LevelPads::waiting) {
            time.unpause();
        }
    }
    if let Some(since) = note.playback_held.take() {
        let held = since.elapsed();
        playbacks(&mut |p| p.resume(held, &sinks));
        if let Some(m) = movies.as_mut() {
            m.shift(held);
        }
    }
}

/// The prompt drawn once it's open (with the TV's font; a headless app
/// without one draws nothing), the focus on its text box, and the captured
/// frame put on it once it lands.
#[allow(clippy::too_many_arguments)]
fn show(
    mut commands: Commands,
    mut note: ResMut<NoteOpen>,
    (assets, theme): (Option<Res<TvAssets>>, Option<Res<TvTheme>>),
    gamepads: Query<(Entity, &Gamepad, Option<&FacesPrinted>)>,
    focus: Option<ResMut<InputFocus>>,
    mut redrawn: MessageWriter<Redrawn>,
) {
    let Some(open) = note.open.as_mut() else {
        return;
    };
    if open.root.is_none()
        && let (Some(assets), Some(theme)) = (assets, theme)
    {
        let pen = Pen::new(&assets, &theme);
        let pad = open
            .by
            .and_then(|e| gamepads.get(e).ok())
            .or_else(|| gamepads.iter().next());
        let labels = pad.map_or(Labels::Position, |(_, p, printed)| labels(p, printed));
        let (root, input, slot) = spawn(&mut commands, &pen, labels);
        open.root = Some(root);
        open.input = Some(input);
        open.slot = Some(slot);
        if let Some(mut focus) = focus {
            open.focus_before = focus.get();
            focus.set(input, bevy::input_focus::FocusCause::Navigated);
        }
        redrawn.write(Redrawn::Overlay);
    }
    if let Some(slot) = open.slot
        && let Some(frame) = open.frame.take()
    {
        commands.entity(slot).insert(ImageNode::new(frame));
        open.shown = true;
        redrawn.write(Redrawn::Overlay);
    }
}

/// A Save or Cancel on screen, clicked.
#[derive(Component, Debug, Clone, Copy)]
struct NoteButton(Answer);

/// The prompt: dimmed over everything, a gold-edged panel with its title,
/// the frame, the text box, Save and Cancel with the pad's buttons, and a
/// line saying how. The root, the text box and the frame's slot.
fn spawn(commands: &mut Commands, pen: &Pen, labels: Labels) -> (Entity, Entity, Entity) {
    let theme = &pen.theme;
    let root = commands
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                left: px(0),
                top: px(0),
                width: percent(100),
                height: percent(100),
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                ..default()
            },
            BackgroundColor(theme.shadow.with_alpha(0.7)),
            GlobalZIndex(NOTE_Z),
            Name::new("capture note"),
        ))
        .id();
    let panel = commands
        .spawn((
            Node {
                width: px(PANEL_W),
                flex_direction: FlexDirection::Column,
                align_items: AlignItems::Center,
                row_gap: px(24),
                padding: UiRect::axes(px(56), px(40)),
                border: UiRect::all(px(4)),
                border_radius: BorderRadius::all(px(32)),
                ..default()
            },
            BackgroundColor(theme.panel[1]),
            BorderColor::all(theme.gold),
            ChildOf(root),
        ))
        .id();
    commands.spawn((
        pen.text("What did you see?", Step::Lg, Role::Accent),
        ChildOf(panel),
    ));
    let slot = commands
        .spawn((
            Node {
                width: px(FRAME_W),
                height: px(FRAME_H),
                border: UiRect::all(px(3)),
                ..default()
            },
            BackgroundColor(Color::BLACK),
            BorderColor::all(theme.gold),
            ChildOf(panel),
        ))
        .id();
    let field = commands
        .spawn((
            Node {
                width: percent(100),
                padding: UiRect::axes(px(20), px(12)),
                border: UiRect::all(px(3)),
                border_radius: BorderRadius::all(px(16)),
                ..default()
            },
            BackgroundColor(theme.panel[0]),
            BorderColor::all(theme.gold),
            ChildOf(panel),
        ))
        .id();
    let input = commands
        .spawn((
            TextInput,
            EditableText {
                max_characters: Some(NOTE_CHARS),
                visible_lines: Some(NOTE_LINES),
                allow_newlines: false,
                cursor_width: 0.12,
                ..default()
            },
            pen.font(Step::Body),
            TextColor(theme.role(Role::Primary)),
            TextLayout::linebreak(LineBreak::WordBoundary),
            TextCursorStyle {
                color: theme.gold,
                ..default()
            },
            Node {
                width: percent(100),
                ..default()
            },
            FocusId(NOTE_FOCUS),
            ChildOf(field),
        ))
        .id();
    let row = commands
        .spawn((
            Node {
                flex_direction: FlexDirection::Row,
                align_items: AlignItems::Center,
                column_gap: px(64),
                ..default()
            },
            ChildOf(panel),
        ))
        .id();
    for (answer, face, label) in [
        (Answer::Save, Face::South, "Save"),
        (Answer::Cancel, Face::East, "Cancel"),
    ] {
        let button = commands
            .spawn((
                Button,
                NoteButton(answer),
                Node {
                    flex_direction: FlexDirection::Row,
                    align_items: AlignItems::Center,
                    column_gap: px(16),
                    padding: UiRect::axes(px(28), px(10)),
                    border: UiRect::all(px(3)),
                    border_radius: BorderRadius::all(px(40)),
                    ..default()
                },
                BackgroundColor(theme.panel[2]),
                BorderColor::all(theme.gold),
                ChildOf(row),
            ))
            .observe(click)
            .id();
        commands.entity(button).with_children(|b| {
            crate::pause::glyph(b, pen, prompt(labels, face));
            b.spawn(pen.text(label, Step::Md, Role::Primary));
        });
    }
    commands.spawn((
        pen.text(
            "Enter saves your note, Esc throws the capture away. The game waits.",
            Step::Caption,
            Role::Muted,
        ),
        ChildOf(panel),
    ));
    (root, input, slot)
}

/// Save or Cancel clicked.
fn click(on: On<Activate>, buttons: Query<&NoteButton>, mut note: ResMut<NoteOpen>) {
    if let (Ok(b), Some(open)) = (buttons.get(on.entity), note.open.as_mut()) {
        open.clicked = Some(b.0);
    }
}

/// The answer, from a frame after the prompt opened: Enter, the pad's
/// bottom button or the capture button saves; Esc or the pad's right
/// button cancels; a click on either does its own. In `Last`, so a key
/// typed in the frame its Enter lands in is in the box already.
#[allow(clippy::too_many_arguments)]
fn answer(
    mut commands: Commands,
    mut note: ResMut<NoteOpen>,
    keys: Option<Res<ButtonInput<KeyCode>>>,
    pads: Query<&Gamepad>,
    texts: Query<&EditableText>,
    mut state: Option<ResMut<FlagState>>,
    focus: Option<ResMut<InputFocus>>,
) {
    note.deleting.retain_mut(|(folder, tries)| {
        *tries -= 1;
        !(delete_capture(folder).is_ok() || *tries == 0)
    });
    let Some(open) = note.open.as_mut() else {
        return;
    };
    if std::mem::take(&mut open.fresh) {
        return;
    }
    let key = |k: KeyCode| keys.as_deref().is_some_and(|keys| keys.just_pressed(k));
    let pad = |b: GamepadButton| pads.iter().any(|p| p.just_pressed(b));
    let chosen = if key(KeyCode::Enter)
        || key(KeyCode::NumpadEnter)
        || pad(GamepadButton::South)
        || pad(FLAG_BUTTON)
    {
        Some(Answer::Save)
    } else if key(KeyCode::Escape) || pad(GamepadButton::East) {
        Some(Answer::Cancel)
    } else {
        open.clicked.take()
    };
    let Some(chosen) = chosen else {
        return;
    };
    let open = note.open.take().expect("open above");
    let text = open
        .input
        .and_then(|e| texts.get(e).ok())
        .map(|t| t.value().to_string())
        .unwrap_or_default();
    match chosen {
        Answer::Save => {
            let text: String = text
                .chars()
                .filter(|c| !c.is_control() || *c == '\t')
                .collect();
            match save_note(&open.folder, &text, SystemTime::now()) {
                Ok(()) => {
                    info!(folder = %open.name, chars = text.chars().count(), "the capture's note saved: the game goes on")
                }
                Err(why) => error!(folder = %open.name, %why, "the capture's note wasn't written"),
            }
            if !text.trim().is_empty()
                && let Some(last) = state
                    .as_mut()
                    .and_then(|s| s.last.as_mut())
                    .filter(|l| l.folder == open.folder)
            {
                last.noted = true;
            }
        }
        // A phone's note joined it while the prompt was up (the join rule,
        // and the phone was told so): it stays.
        Answer::Cancel
            if state.as_ref().is_some_and(|s| {
                s.last
                    .as_ref()
                    .is_some_and(|l| l.folder == open.folder && l.noted)
            }) =>
        {
            info!(folder = %open.name, "the capture cancelled but kept: a phone's note is in it");
        }
        Answer::Cancel => {
            if let Some(state) = state.as_mut()
                && state.last.as_ref().is_some_and(|l| l.folder == open.folder)
            {
                state.last = None;
            }
            match delete_capture(&open.folder) {
                Ok(()) => {
                    info!(folder = %open.name, "the capture cancelled and deleted: the game goes on")
                }
                Err(e) => {
                    warn!(folder = %open.name, %e, "the cancelled capture didn't go at once; trying again");
                    note.deleting.push((open.folder.clone(), DELETE_TRIES));
                }
            }
        }
    }
    note.answered.push((open.name.clone(), chosen));
    if let Some(root) = open.root
        && let Ok(mut root) = commands.get_entity(root)
    {
        root.despawn();
    }
    if let Some(mut focus) = focus {
        match open.focus_before {
            Some(before) => focus.set(before, bevy::input_focus::FocusCause::Navigated),
            None => focus.clear(),
        }
    }
}

/// The prompt as the route reads it.
fn publish(
    note: Res<NoteOpen>,
    asks: Res<NoteAsks>,
    texts: Query<&EditableText>,
    time: Res<Time<Virtual>>,
) {
    let open = note.open.as_ref();
    let shot = NoteShot {
        open: open.is_some(),
        folder: open.map(|o| o.name.clone()),
        text: open
            .and_then(|o| o.input)
            .and_then(|e| texts.get(e).ok())
            .map(|t| t.value().to_string())
            .unwrap_or_default(),
        frame: open.is_some_and(|o| o.shown),
        paused: time.is_paused(),
        answered: note
            .answered
            .iter()
            .map(|(f, a)| {
                (
                    f.clone(),
                    match a {
                        Answer::Save => "save",
                        Answer::Cancel => "cancel",
                    },
                )
            })
            .collect(),
    };
    let mut held = asks.shot.lock().unwrap_or_else(|e| e.into_inner());
    if *held != shot {
        *held = shot;
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use bevy::input::keyboard::Key;

    use super::*;
    use crate::flags::Flags;
    use crate::test_support::play::{fight_app_with, hero, log_last};
    use crate::test_support::test_pad;

    /// The rule, each way: a hand's press asks, the posted key and the phone
    /// never do, and an unattended run only on the route's hand press.
    #[test]
    fn only_a_hands_press_asks_for_a_note() {
        let pad = Source::Pad("Pro Controller".into());
        assert!(asks_for_note(&pad, true, false, false));
        assert!(asks_for_note(&Source::Key, true, false, false));
        assert!(
            !asks_for_note(&Source::Key, true, true, false),
            "posted key"
        );
        assert!(!asks_for_note(&pad, false, false, false), "unattended");
        assert!(
            !asks_for_note(&Source::Key, false, true, false),
            "unattended"
        );
        assert!(
            asks_for_note(&Source::Key, false, false, true),
            "the route's hand press"
        );
        assert!(
            !asks_for_note(&Source::Phone, true, false, true),
            "the phone"
        );
    }

    #[test]
    fn a_key_for_each_character() {
        assert_eq!(key_code_of('a'), KeyCode::KeyA);
        assert_eq!(key_code_of('Z'), KeyCode::KeyZ);
        assert_eq!(key_code_of('7'), KeyCode::Digit7);
        assert_eq!(key_code_of(' '), KeyCode::Space);
        assert!(matches!(key_code_of('!'), KeyCode::Unidentified(_)));
    }

    /// AH.25's fight with the flags on (`prompts` as asked), Eruptor joined;
    /// the app, its pad and the flag folder.
    fn fight(tmp: &Path, prompts: bool) -> (App, Entity, PathBuf, FlagAsks) {
        let dir = tmp.join("flags");
        let logs = tmp.join("logs");
        std::fs::create_dir_all(&logs).unwrap();
        let asks = FlagAsks::default();
        asks.set_on(true);
        let flags = Flags {
            dir: dir.clone(),
            log_dir: logs,
            asks: asks.clone(),
            prompts,
        };
        let (app, pad) = fight_app_with(tmp, |app| {
            app.add_plugins(flags);
        });
        (app, pad, dir, asks)
    }

    fn folders(dir: &Path) -> Vec<PathBuf> {
        let mut v: Vec<PathBuf> = std::fs::read_dir(dir)
            .map(|d| d.flatten().map(|e| e.path()).collect())
            .unwrap_or_default();
        v.sort();
        v
    }

    fn open(app: &App) -> bool {
        app.world().resource::<NoteOpen>().is_open()
    }

    fn json(folder: &Path) -> serde_json::Value {
        serde_json::from_slice(&std::fs::read(folder.join("capture.json")).unwrap()).unwrap()
    }

    /// The scripts' steps each of `frames` frames.
    fn steps(app: &mut App, frames: usize) -> Vec<u64> {
        (0..frames)
            .map(|_| {
                let before = log_last(app);
                app.update();
                log_last(app) - before
            })
            .collect()
    }

    /// A hand's capture press mid-fight: the capture as ever, then the
    /// level waits under the prompt (no step, the hero where he stood, the
    /// level's clock stopped) and the pad's faces reach him not at all;
    /// the pad's bottom button saves, an empty note leaving the capture as
    /// taken, and the level goes on at its own pace, no step made up.
    #[test]
    fn the_level_waits_under_the_prompt_and_goes_on_after_without_catching_up() {
        let tmp = tempfile::tempdir().unwrap();
        let (mut app, pad, dir, _) = fight(tmp.path(), true);
        let before = steps(&mut app, 10);
        let per_frame = *before.iter().max().unwrap();
        assert!(per_frame > 0, "the fight steps: {before:?}");
        test_pad::press(&mut app, pad, FLAG_BUTTON);
        app.update();
        assert!(open(&app), "the press asks for its note");
        assert_eq!(folders(&dir).len(), 1, "the capture taken as ever");
        let (at, step) = (hero(&app).pivot, log_last(&app));
        assert_eq!(steps(&mut app, 30), vec![0; 30], "no step under the prompt");
        assert!(app.world().resource::<Time<Virtual>>().is_paused());
        assert_eq!(hero(&app).pivot, at, "he stands");
        // The faces go to the prompt, never the hero: the bottom one saves.
        test_pad::button(&mut app, pad, GamepadButton::South, true);
        app.update();
        assert_eq!(
            app.world().resource::<crate::hero::HeroActions>().held,
            [false; 4],
            "the press that saved reached no attack"
        );
        assert!(!open(&app), "the bottom button saved");
        assert_eq!(log_last(&app), step, "nothing stepped as it closed");
        test_pad::button(&mut app, pad, GamepadButton::South, false);
        let after = steps(&mut app, 10);
        assert!(
            after[0] <= per_frame,
            "the first frame after makes up nothing: {after:?}"
        );
        assert!(
            after.iter().all(|s| *s <= per_frame),
            "{after:?} against {per_frame}"
        );
        assert!(after.iter().sum::<u64>() > 0, "and it goes on: {after:?}");
        let made = folders(&dir);
        assert_eq!(made.len(), 1);
        assert_eq!(
            json(&made[0])["note"],
            serde_json::Value::Null,
            "an empty note"
        );
        assert!(!made[0].join("note.txt").exists());
        assert_eq!(
            app.world()
                .resource::<NoteOpen>()
                .answered
                .last()
                .map(|a| a.1),
            Some(Answer::Save)
        );
    }

    /// The pad's right button cancels: the capture's folder is gone and the
    /// level goes on; a second capture press while it's up saves rather
    /// than taking another.
    #[test]
    fn back_deletes_the_capture_and_the_capture_button_saves() {
        let tmp = tempfile::tempdir().unwrap();
        let (mut app, pad, dir, _) = fight(tmp.path(), true);
        test_pad::press(&mut app, pad, FLAG_BUTTON);
        app.update();
        assert!(open(&app));
        assert_eq!(folders(&dir).len(), 1);
        test_pad::press(&mut app, pad, GamepadButton::East);
        app.update();
        assert!(!open(&app), "back cancelled");
        assert!(folders(&dir).is_empty(), "and the capture went");
        assert!(!app.world().resource::<Time<Virtual>>().is_paused());
        assert!(app.world().resource::<FlagState>().last.is_none());

        test_pad::press(&mut app, pad, FLAG_BUTTON);
        app.update();
        assert!(open(&app));
        test_pad::press(&mut app, pad, FLAG_BUTTON);
        app.update();
        assert!(!open(&app), "the capture button again saved");
        assert_eq!(folders(&dir).len(), 1, "no second capture");
        assert!(!app.world().resource::<Time<Virtual>>().is_paused());
    }

    /// A run nobody sits at (`GUAC_TEST_POSTED_PADS_ONLY`: prompts off) and
    /// the phone take their captures as before, the level never waiting;
    /// the route's hand press is the one that asks there.
    #[test]
    fn no_prompt_unattended_or_for_the_phone() {
        let tmp = tempfile::tempdir().unwrap();
        let (mut app, pad, dir, asks) = fight(tmp.path(), false);
        test_pad::press(&mut app, pad, FLAG_BUTTON);
        app.update();
        assert!(!open(&app), "unattended: no prompt");
        assert_eq!(folders(&dir).len(), 1);
        assert!(
            steps(&mut app, 5).iter().sum::<u64>() > 0,
            "the level goes on"
        );
        // The phone's note joins that capture (the join rule), asking nothing.
        let mut ask = asks.push_for_tests("the gate");
        app.update();
        assert!(ask.try_recv().unwrap().joined);
        assert!(!open(&app), "the phone's carries its note");
        asks.note.hand.fetch_add(1, Ordering::Relaxed);
        app.update();
        assert!(open(&app), "the route asked for a hand's");
        assert_eq!(folders(&dir).len(), 2);
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .release(crate::flags::FLAG_KEY);

        let tmp = tempfile::tempdir().unwrap();
        let (mut app, _, dir, asks) = fight(tmp.path(), true);
        let mut ask = asks.push_for_tests("double swords");
        app.update();
        assert!(ask.try_recv().is_ok());
        assert!(
            !open(&app),
            "a phone's capture asks nothing, prompts on or not"
        );
        assert_eq!(folders(&dir).len(), 1);
        assert!(steps(&mut app, 5).iter().sum::<u64>() > 0);
    }

    /// The TV laid out with the prompt's widgets: the portal screen with
    /// Bevy's text input and the focused input's dispatch, the flags on.
    fn tv(tmp: &Path) -> (App, PathBuf, FlagAsks) {
        use crate::cards::PortalCards;
        let mut app = crate::test_support::tv::portal_app(
            1920,
            1080,
            guac_server::config::WindowMode::Tv,
            PortalCards::default(),
        );
        let dir = tmp.join("flags");
        let asks = FlagAsks::default();
        asks.set_on(true);
        app.add_plugins((
            bevy::input_focus::InputDispatchPlugin,
            bevy::ui_widgets::TextInputPlugin,
        ))
        .add_plugins(Flags {
            dir: dir.clone(),
            log_dir: tmp.join("logs"),
            asks: asks.clone(),
            prompts: true,
        });
        app.world_mut()
            .spawn((Window::default(), bevy::window::PrimaryWindow));
        for _ in 0..3 {
            app.update();
        }
        (app, dir, asks)
    }

    fn typed(app: &App) -> Option<String> {
        let open = app.world().resource::<NoteOpen>().open.as_ref()?;
        let input = open.input?;
        Some(
            app.world()
                .get::<EditableText>(input)
                .unwrap()
                .value()
                .to_string(),
        )
    }

    /// The key on the TV's window opens the prompt drawn over the screen,
    /// the focus on its box; what's typed (an `f` and the arrows among it)
    /// goes into the box and nowhere else: no second capture, no move for
    /// the screen; Backspace takes one off; Enter saves it into the
    /// capture's `note` and `note.txt`, the prompt gone and the focus back.
    #[test]
    fn typed_keys_go_into_the_note_and_enter_saves_it() {
        let tmp = tempfile::tempdir().unwrap();
        let (mut app, dir, asks) = tv(tmp.path());
        let focus_before = app.world().resource::<InputFocus>().get();
        crate::test_support::tv::key(&mut app, crate::flags::FLAG_KEY, Key::Character("f".into()));
        app.update();
        assert!(open(&app));
        let input = app
            .world()
            .resource::<NoteOpen>()
            .open
            .as_ref()
            .unwrap()
            .input;
        assert!(input.is_some(), "drawn with the TV's font");
        assert_eq!(
            app.world().resource::<InputFocus>().get(),
            input,
            "the box has the keys"
        );
        app.world_mut()
            .resource_mut::<Messages<crate::input::TvInput>>()
            .clear();
        asks.note.push(Posted::Text("the f gatex".into()));
        app.update();
        for code in [KeyCode::ArrowLeft, KeyCode::ArrowRight] {
            let logical = if code == KeyCode::ArrowLeft {
                Key::ArrowLeft
            } else {
                Key::ArrowRight
            };
            crate::test_support::tv::key(&mut app, code, logical);
        }
        asks.note.push(Posted::Key(Named::Backspace));
        app.update();
        app.update();
        assert_eq!(typed(&app).as_deref(), Some("the f gate"));
        assert_eq!(folders(&dir).len(), 1, "the f typed took no capture");
        let heard: Vec<_> = app
            .world_mut()
            .resource_mut::<Messages<crate::input::TvInput>>()
            .drain()
            .collect();
        assert!(heard.is_empty(), "the screen heard nothing: {heard:?}");
        asks.note.push(Posted::Key(Named::Enter));
        app.update();
        app.update();
        assert!(!open(&app), "Enter saved");
        let folder = &folders(&dir)[0];
        assert_eq!(json(folder)["note"], "the f gate");
        let kept = std::fs::read_to_string(folder.join("note.txt")).unwrap();
        assert!(kept.ends_with("] the f gate\n"), "{kept}");
        assert_eq!(
            app.world().resource::<InputFocus>().get(),
            focus_before,
            "the focus back"
        );
        let mut q = app.world_mut().query_filtered::<(), With<TextInput>>();
        assert_eq!(q.iter(app.world()).count(), 0, "the prompt gone");
        assert!(
            app.world()
                .resource::<FlagState>()
                .last
                .as_ref()
                .unwrap()
                .noted
        );
    }

    /// Esc cancels from the keyboard: the folder gone, the prompt down.
    #[test]
    fn escape_cancels() {
        let tmp = tempfile::tempdir().unwrap();
        let (mut app, dir, asks) = tv(tmp.path());
        crate::test_support::tv::key(&mut app, crate::flags::FLAG_KEY, Key::Character("f".into()));
        app.update();
        assert!(open(&app));
        asks.note.push(Posted::Text("fine after all".into()));
        app.update();
        asks.note.push(Posted::Key(Named::Escape));
        app.update();
        app.update();
        assert!(!open(&app));
        assert!(folders(&dir).is_empty());
    }

    /// Cancel reaches the one capture and nothing past it (openspec AS.808):
    /// an older capture beside it stays; a second Esc does nothing; and the
    /// delete itself takes only a capture's own files, so a folder holding
    /// anything else, the flags root among them, or a symlink, keeps it all.
    #[test]
    fn cancel_deletes_its_own_capture_and_nothing_else() {
        let tmp = tempfile::tempdir().unwrap();
        let (mut app, dir, asks) = tv(tmp.path());
        let older = dir.join("2026-11-22T18-50-00.000Z-pad");
        std::fs::create_dir_all(&older).unwrap();
        std::fs::write(older.join("capture.json"), "{}").unwrap();
        std::fs::write(older.join("frame.png"), "png").unwrap();
        crate::test_support::tv::key(&mut app, crate::flags::FLAG_KEY, Key::Character("f".into()));
        app.update();
        assert!(open(&app));
        assert_eq!(folders(&dir).len(), 2);
        for _ in 0..2 {
            asks.note.push(Posted::Key(Named::Escape));
            app.update();
            app.update();
        }
        assert!(!open(&app));
        assert_eq!(
            folders(&dir),
            vec![older.clone()],
            "the older capture stays"
        );
        assert!(older.join("capture.json").exists() && older.join("frame.png").exists());
        assert_eq!(app.world().resource::<NoteOpen>().answered.len(), 1);

        // The delete, asked of what isn't one capture.
        assert!(
            delete_capture(&dir).is_err(),
            "the root, with a capture in it"
        );
        assert!(older.join("capture.json").exists());
        let nested = tmp.path().join("nested");
        std::fs::create_dir_all(nested.join("inner")).unwrap();
        std::fs::write(nested.join("capture.json"), "{}").unwrap();
        std::fs::write(nested.join("keep.txt"), "x").unwrap();
        assert!(delete_capture(&nested).is_err());
        assert!(nested.join("inner").is_dir() && nested.join("keep.txt").exists());
        let link = tmp.path().join("link");
        std::os::unix::fs::symlink(&older, &link).unwrap();
        assert!(delete_capture(&link).is_err(), "a symlink");
        assert!(older.join("frame.png").exists(), "its target kept");
        assert!(
            delete_capture(&tmp.path().join("gone")).is_ok(),
            "gone is done"
        );
    }

    /// A phone's note that joins the capture while its prompt is up (the
    /// join rule; the phone told it joined) keeps the capture through a
    /// Cancel (openspec AS.808).
    #[test]
    fn cancel_keeps_a_capture_the_phone_noted() {
        let tmp = tempfile::tempdir().unwrap();
        let (mut app, dir, asks) = tv(tmp.path());
        crate::test_support::tv::key(&mut app, crate::flags::FLAG_KEY, Key::Character("f".into()));
        app.update();
        assert!(open(&app));
        let mut ask = asks.push_for_tests("the gate");
        app.update();
        assert!(ask.try_recv().unwrap().joined);
        asks.note.push(Posted::Key(Named::Escape));
        app.update();
        app.update();
        assert!(!open(&app));
        let made = folders(&dir);
        assert_eq!(made.len(), 1, "kept");
        assert!(made[0].join("note.txt").exists());
    }

    /// Esc and the arrows under the prompt reach no screen (no Back, no
    /// quit), and the route reads the frame on the prompt only once it's
    /// there (openspec AS.808).
    #[test]
    fn escape_under_the_prompt_reaches_no_screen() {
        let tmp = tempfile::tempdir().unwrap();
        let (mut app, _, asks) = tv(tmp.path());
        crate::test_support::tv::key(&mut app, crate::flags::FLAG_KEY, Key::Character("f".into()));
        app.update();
        assert!(open(&app));
        assert!(!asks.note.shot().frame, "no frame lands headless");
        app.world_mut()
            .resource_mut::<Messages<crate::input::TvInput>>()
            .clear();
        app.world_mut().resource_mut::<Messages<AppExit>>().clear();
        crate::test_support::tv::key(&mut app, KeyCode::Escape, Key::Escape);
        app.update();
        let heard: Vec<_> = app
            .world_mut()
            .resource_mut::<Messages<crate::input::TvInput>>()
            .drain()
            .collect();
        assert!(heard.is_empty(), "the screen heard {heard:?}");
        assert!(
            app.world_mut()
                .resource_mut::<Messages<AppExit>>()
                .drain()
                .next()
                .is_none(),
            "nothing quits"
        );
        assert!(!open(&app), "Esc cancelled");
    }

    /// Start under the prompt pauses nothing, and the game after is as it
    /// was before the press; a prompt opened under Start's pause leaves the
    /// level paused under Start after (openspec AS.808).
    #[test]
    fn the_prompt_leaves_starts_pause_as_it_found_it() {
        let tmp = tempfile::tempdir().unwrap();
        let (mut app, pad, _, _) = fight(tmp.path(), true);
        let paused = |app: &App| {
            (
                app.world().resource::<Time<Virtual>>().is_paused(),
                app.world()
                    .resource::<LevelPads>()
                    .seat()
                    .and_then(|s| s.pause())
                    .is_some(),
            )
        };
        app.update();
        assert_eq!(paused(&app), (false, false));
        test_pad::press(&mut app, pad, FLAG_BUTTON);
        app.update();
        assert!(open(&app));
        test_pad::press(&mut app, pad, GamepadButton::Start);
        app.update();
        assert!(open(&app), "Start answers nothing");
        assert_eq!(
            paused(&app),
            (true, false),
            "the prompt's hold, not Start's"
        );
        test_pad::press(&mut app, pad, GamepadButton::South);
        app.update();
        assert!(!open(&app));
        assert_eq!(paused(&app), (false, false), "running, as before");
        assert!(steps(&mut app, 5).iter().sum::<u64>() > 0);

        test_pad::press(&mut app, pad, GamepadButton::Start);
        app.update();
        assert_eq!(paused(&app), (true, true), "Start's pause");
        test_pad::press(&mut app, pad, FLAG_BUTTON);
        app.update();
        assert!(open(&app), "a capture under Start's pause asks too");
        test_pad::press(&mut app, pad, GamepadButton::East);
        app.update();
        assert!(!open(&app), "cancelled");
        assert_eq!(paused(&app), (true, true), "still Start's pause");
        assert_eq!(steps(&mut app, 5), vec![0; 5], "nothing steps under it");
    }

    /// The captured frame onto the prompt and off it again, ten times over
    /// (openspec AS.808): the route says it's there only once it is, the
    /// frame on disk is the shot's own pixels, and after each answer the
    /// thumbnail and its node are gone, the image count back where it was.
    #[test]
    fn the_frame_shows_and_is_freed_when_the_prompt_closes() {
        use bevy::render::view::window::screenshot::Screenshot;
        let tmp = tempfile::tempdir().unwrap();
        let (mut app, dir, asks) = tv(tmp.path());
        let count = |app: &mut App| {
            let nodes = app
                .world_mut()
                .query::<&ImageNode>()
                .iter(app.world())
                .count();
            (app.world().resource::<Assets<Image>>().len(), nodes)
        };
        let before = count(&mut app);
        for i in 0..10u8 {
            crate::test_support::tv::key(
                &mut app,
                crate::flags::FLAG_KEY,
                Key::Character("f".into()),
            );
            app.update();
            assert!(open(&app), "press {i}");
            assert!(!asks.note.shot().frame, "no frame before it lands");
            let shot = app
                .world_mut()
                .query_filtered::<Entity, With<Screenshot>>()
                .iter(app.world())
                .next()
                .expect("the capture's screenshot");
            let image = Image::new_fill(
                bevy::render::render_resource::Extent3d {
                    width: 64,
                    height: 36,
                    depth_or_array_layers: 1,
                },
                bevy::render::render_resource::TextureDimension::D2,
                &[i * 20, 80, 160, 255],
                bevy::render::render_resource::TextureFormat::Rgba8UnormSrgb,
                bevy::asset::RenderAssetUsages::all(),
            );
            app.world_mut().trigger(ScreenshotCaptured {
                entity: shot,
                image,
            });
            app.world_mut().entity_mut(shot).despawn();
            app.update();
            assert!(asks.note.shot().frame, "press {i}: the frame on it");
            asks.note.push(Posted::Key(Named::Enter));
            app.update();
            app.update();
            assert!(!open(&app));
        }
        for _ in 0..3 {
            app.update();
        }
        assert_eq!(count(&mut app), before, "(images, image nodes)");
        // The frame saved is the shot's, written off the frame's thread.
        let made = folders(&dir);
        assert_eq!(made.len(), 10);
        let png = made[9].join("frame.png");
        let until = Instant::now() + Duration::from_secs(5);
        while !png.exists() && Instant::now() < until {
            std::thread::sleep(Duration::from_millis(20));
        }
        std::thread::sleep(Duration::from_millis(100));
        let saved = image::open(&png).unwrap().to_rgb8();
        assert_eq!(saved.dimensions(), (64, 36));
        assert_eq!(saved.get_pixel(10, 10).0, [180, 80, 160]);
    }

    /// The clock a playback keeps reads the same after a hold as it did at
    /// its start, the time held not counted.
    #[test]
    fn a_held_playback_clock_skips_nothing() {
        let mut clock = crate::intro_player::AnchoredClock::default();
        let t0 = Instant::now();
        clock.tick(t0, Some(Duration::ZERO));
        let before = clock.tick(t0 + Duration::from_secs(2), Some(Duration::from_secs(2)));
        let held = Duration::from_secs(30);
        clock.shift(held);
        let after = clock.tick(
            t0 + Duration::from_secs(2) + held,
            Some(Duration::from_secs(2)),
        );
        assert!((after - before).abs() < 1e-9, "{before} then {after}");
    }

    /// The routes: the hand press and the typing refused with flags off or
    /// no prompt up, a bad body a 400, the read as published.
    #[tokio::test]
    async fn the_routes_answer() {
        use tower::ServiceExt;
        let flags = FlagAsks::default();
        let app = routes::<()>(&flags);
        let call = |req: axum::http::Request<axum::body::Body>| {
            let app = app.clone();
            async move { app.oneshot(req).await.unwrap().status() }
        };
        let hand = || {
            axum::http::Request::post("/api/_test/flag/hand")
                .body(axum::body::Body::empty())
                .unwrap()
        };
        let note = |body: &str| {
            axum::http::Request::post("/api/_test/flag/note")
                .header("content-type", "application/json")
                .body(axum::body::Body::from(body.to_owned()))
                .unwrap()
        };
        assert_eq!(call(hand()).await, 409);
        flags.set_on(true);
        assert_eq!(call(hand()).await, 202);
        assert_eq!(flags.note.hand.load(Ordering::Relaxed), 1);
        assert_eq!(call(note(r#"{"type":"x"}"#)).await, 409, "no prompt up");
        flags.note.shot.lock().unwrap().open = true;
        for bad in [
            r#"{"type":"a\nb"}"#,
            r#"{"type":""}"#,
            r#"{"key":"tab"}"#,
            r#"{"type":"a","key":"enter"}"#,
            "{}",
        ] {
            assert_eq!(call(note(bad)).await, 400, "{bad}");
        }
        assert_eq!(call(note(r#"{"type":"the gate"}"#)).await, 202);
        assert_eq!(call(note(r#"{"key":"enter"}"#)).await, 202);
        assert_eq!(flags.note.pop(), Some(Posted::Text("the gate".into())));
        assert_eq!(flags.note.pop(), Some(Posted::Key(Named::Enter)));
        let r = app
            .clone()
            .oneshot(
                axum::http::Request::get("/api/_test/flag/note")
                    .body(axum::body::Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        let b = http_body_util::BodyExt::collect(r.into_body())
            .await
            .unwrap()
            .to_bytes();
        let shot: serde_json::Value = serde_json::from_slice(&b).unwrap();
        assert_eq!(shot["open"], true);
    }
}
