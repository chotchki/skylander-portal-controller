//! The TV's one input router (openspec AP, design decision 2). `route_input`
//! reads every pad, the keys the widgets don't take (the arrows, Esc,
//! NumpadEnter) and the presses posted to `/api/_test/tv/press`, then hands
//! the frame to the top layer only: an intro playing over the portal or the
//! level takes any button or key as its skip and nothing under it sees the
//! press; while the level shows, its layer takes the pads ([`crate::pad`],
//! AG.20) and the screen hears nothing; otherwise the screen gets it as
//! [`TvInput`]s. Mapping is positional (the design doc's Controllers): South
//! activates, East goes back, Mode is Home (nothing yet), the D-pad, the
//! arrows and the left stick move, held ones repeating per [`NavRepeat`].
//!
//! Enter and Space are never mapped: `bevy_ui_widgets`' button takes them
//! from the focused entity, so nothing activates twice.
//!
//! Then [`keep_focus`] puts the focus back on one of the screen's choices if
//! it strayed and [`act_on_input`] does what the screen heard: Activate
//! presses the focused button, Back closes the startup-error screens.

use std::collections::VecDeque;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use bevy::input::InputSystems;
use bevy::input::gamepad::{
    Gamepad, GamepadButton, gamepad_connection_system, gamepad_event_processing_system,
};
use bevy::input_focus::{AutoFocus, FocusCause, InputFocus, InputFocusVisible};
use bevy::math::CompassOctant;
use bevy::prelude::*;
use bevy::ui::{InteractionDisabled, Pressed};
// Not the prelude's `Button`: that's bevy_ui's deprecated one.
use bevy::ui_widgets::{Activate, Button};
use tracing::{debug, info};

use crate::hero::{HeroActions, HeroInput};
use crate::intro::{IntroEvent, IntroQueue};
use crate::intro_player::{self, IntroOverlay};
use crate::mirror::TvMirror;
use crate::pad::{self, HeroPad, LevelPads};
use crate::pause;
use crate::tv::Current;

/// Long enough that a tap is one move; a hold starts repeating after it.
pub(crate) const REPEAT_DELAY: Duration = Duration::from_millis(400);

/// A hold's pace, about seven moves a second: quick through a list, slow
/// enough to let go on the one wanted.
pub(crate) const REPEAT_EVERY: Duration = Duration::from_millis(150);

/// The left stick moves only past this deflection (0 to 1), so a stick
/// resting off centre moves nothing.
pub(crate) const STICK_THRESHOLD: f32 = 0.5;

/// Posted presses waiting for the router; more than this and the oldest go.
const POSTED_MAX: usize = 64;

/// A choice's name, for the router's focus and the mirror's `focus`.
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct FocusId(pub(crate) &'static str);

/// What a press means to the screen under it.
#[derive(Message, Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TvInput {
    /// South or NumpadEnter: press the focused choice.
    Activate,
    /// East or Esc.
    Back,
    /// Mode: nothing until the design doc's Open question 1 says what Home does.
    Home,
    /// The D-pad, the arrows or the left stick: a press, then repeats while held.
    Move(CompassOctant),
}

/// A press posted to `/api/_test/tv/press`, by the name the route takes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TestPress {
    South,
    East,
    Mode,
    Up,
    Down,
    Left,
    Right,
}

impl TestPress {
    pub(crate) fn parse(name: &str) -> Option<Self> {
        Some(match name {
            "south" => Self::South,
            "east" => Self::East,
            "mode" => Self::Mode,
            "up" => Self::Up,
            "down" => Self::Down,
            "left" => Self::Left,
            "right" => Self::Right,
            _ => return None,
        })
    }

    /// What the screen makes of it; a posted direction is one move, no hold.
    fn input(self) -> TvInput {
        match self {
            Self::South => TvInput::Activate,
            Self::East => TvInput::Back,
            Self::Mode => TvInput::Home,
            Self::Up => TvInput::Move(CompassOctant::North),
            Self::Down => TvInput::Move(CompassOctant::South),
            Self::Left => TvInput::Move(CompassOctant::West),
            Self::Right => TvInput::Move(CompassOctant::East),
        }
    }
}

/// Presses posted over HTTP, waiting for the router, which takes one a
/// frame, as a hand would.
#[derive(Clone, Default)]
pub(crate) struct PostedPresses(Arc<Mutex<VecDeque<TestPress>>>);

impl PostedPresses {
    pub(crate) fn push(&self, press: TestPress) {
        let mut queue = self.0.lock().unwrap_or_else(|e| e.into_inner());
        if queue.len() == POSTED_MAX {
            queue.pop_front();
        }
        queue.push_back(press);
    }

    pub(crate) fn pop(&self) -> Option<TestPress> {
        self.0.lock().unwrap_or_else(|e| e.into_inner()).pop_front()
    }

    #[cfg(test)]
    pub(crate) fn len(&self) -> usize {
        self.0.lock().unwrap().len()
    }
}

/// One frame of input as the router read it, before a layer takes it.
#[derive(Debug, Default, Clone, PartialEq)]
pub(crate) struct Frame {
    /// A pad button or key went down, or a press was posted: an intro's skip.
    pub(crate) pressed: bool,
    /// What the screen would make of it, in order.
    pub(crate) inputs: Vec<TvInput>,
}

/// Who takes a frame's input: the top layer, and only it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Layer {
    /// An intro over the portal or the level: any button or key skips it.
    Intro,
    // A modal goes here, under the intro, once one exists.
    /// The level (AG.20): its own read of the pads.
    Level,
    /// The screen's own choices.
    Screen,
}

/// What the top layer did with a frame.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Taken {
    Skip,
    /// The level reads the pads itself ([`crate::pad::Seat`]).
    Level,
    Screen(Vec<TvInput>),
    Nothing,
}

impl Layer {
    pub(crate) fn top(intro_playing: bool, level_shows: bool) -> Self {
        if intro_playing {
            Self::Intro
        } else if level_shows {
            Self::Level
        } else {
            Self::Screen
        }
    }

    pub(crate) fn name(self) -> &'static str {
        match self {
            Self::Intro => "intro",
            Self::Level => "level",
            Self::Screen => "screen",
        }
    }

    /// The frame as this layer takes it; nothing passes under it.
    pub(crate) fn take(self, frame: Frame) -> Taken {
        match self {
            // A stick move is no button: it neither skips nor reaches the screen.
            Self::Intro if frame.pressed => Taken::Skip,
            Self::Intro => Taken::Nothing,
            Self::Level => Taken::Level,
            Self::Screen if frame.inputs.is_empty() => Taken::Nothing,
            Self::Screen => Taken::Screen(frame.inputs),
        }
    }
}

/// A held direction as moves: one on the press, the first repeat
/// [`REPEAT_DELAY`] later, then one every [`REPEAT_EVERY`] until it's let go.
#[derive(Debug, Default)]
pub(crate) struct NavRepeat {
    /// The direction held and when its next move is due.
    held: Option<(CompassOctant, Duration)>,
    /// The hold down now moves nothing until it's let go (a layer above took it).
    swallowed: bool,
}

impl NavRepeat {
    /// `dpad` is the D-pad and arrows (each axis -1, 0 or 1), `stick` the
    /// left stick, `now` any steady clock. The D-pad wins over the stick.
    pub(crate) fn step(&mut self, dpad: Vec2, stick: Vec2, now: Duration) -> Option<CompassOctant> {
        let Some(dir) = held_direction(dpad, stick) else {
            *self = Self::default();
            return None;
        };
        let moved = match self.held {
            Some((was, due)) if was == dir => {
                if now < due {
                    return None;
                }
                // On the cadence, not a frame late each time; after a
                // stall, one move and the cadence again from here.
                let next = due + REPEAT_EVERY;
                self.held = Some((dir, if next > now { next } else { now + REPEAT_EVERY }));
                dir
            }
            // A press, or the hold turned to another direction: a new
            // press, which a layer above can swallow again this frame.
            _ => {
                self.held = Some((dir, now + REPEAT_DELAY));
                self.swallowed = false;
                dir
            }
        };
        (!self.swallowed).then_some(moved)
    }

    /// The hold down now, if any, moves nothing more until it's let go.
    pub(crate) fn swallow(&mut self) {
        if self.held.is_some() {
            self.swallowed = true;
        }
    }
}

/// The direction a frame holds, if any; the stick only past [`STICK_THRESHOLD`].
fn held_direction(dpad: Vec2, stick: Vec2) -> Option<CompassOctant> {
    let v = if dpad != Vec2::ZERO {
        dpad
    } else if stick.length() >= STICK_THRESHOLD {
        stick
    } else {
        return None;
    };
    Dir2::new(v).ok().map(CompassOctant::from)
}

/// The press half of a frame from the keys and pads, with the D-pad and
/// arrows held (each axis clamped to -1..1) and the most deflected stick.
pub(crate) fn read_devices<'a>(
    keys: Option<&ButtonInput<KeyCode>>,
    pads: impl Iterator<Item = &'a Gamepad>,
) -> (Frame, Vec2, Vec2) {
    let mut frame = Frame::default();
    let mut dpad = Vec2::ZERO;
    let mut stick = Vec2::ZERO;
    let mapped = [
        (GamepadButton::South, TvInput::Activate),
        (GamepadButton::East, TvInput::Back),
        (GamepadButton::Mode, TvInput::Home),
    ];
    for pad in pads {
        frame.pressed |= pad.get_just_pressed().next().is_some();
        for (button, input) in mapped {
            if pad.just_pressed(button) {
                frame.inputs.push(input);
            }
        }
        dpad += axes(|b| pad.pressed(b), DPAD);
        if pad.left_stick().length() > stick.length() {
            stick = pad.left_stick();
        }
    }
    if let Some(keys) = keys {
        frame.pressed |= keys.get_just_pressed().next().is_some();
        if keys.just_pressed(KeyCode::NumpadEnter) {
            frame.inputs.push(TvInput::Activate);
        }
        if keys.just_pressed(KeyCode::Escape) {
            frame.inputs.push(TvInput::Back);
        }
        dpad += axes(|k| keys.pressed(k), ARROWS);
    }
    (frame, dpad.clamp(Vec2::NEG_ONE, Vec2::ONE), stick)
}

/// Up, down, left, right.
const DPAD: [GamepadButton; 4] = [
    GamepadButton::DPadUp,
    GamepadButton::DPadDown,
    GamepadButton::DPadLeft,
    GamepadButton::DPadRight,
];

const ARROWS: [KeyCode; 4] = [
    KeyCode::ArrowUp,
    KeyCode::ArrowDown,
    KeyCode::ArrowLeft,
    KeyCode::ArrowRight,
];

fn axes<T: Copy>(held: impl Fn(T) -> bool, [up, down, left, right]: [T; 4]) -> Vec2 {
    let axis =
        |plus: T, minus: T| f32::from(u8::from(held(plus))) - f32::from(u8::from(held(minus)));
    Vec2::new(axis(right, left), axis(up, down))
}

pub(crate) struct InputRouter;

impl Plugin for InputRouter {
    fn build(&self, app: &mut App) {
        // Console-style: the focus ring shows from the start (a mouse press
        // hides it, the next pad or key input brings it back).
        app.add_message::<TvInput>()
            // The overlays tell the shots when they come and go.
            .add_message::<crate::tv::Redrawn>()
            .insert_resource(InputFocusVisible(true))
            .init_resource::<LevelPads>()
            // Between a pad's connection and its first axis event, so its
            // move stick reads raw from that event on.
            .add_systems(
                PreUpdate,
                pad::read_sticks_raw
                    .after(gamepad_connection_system)
                    .before(gamepad_event_processing_system),
            )
            .add_systems(
                PreUpdate,
                (route_input, pause::hold_time, keep_focus, act_on_input)
                    .chain()
                    .after(InputSystems),
            )
            .add_systems(Update, pause::show_overlay);
    }
}

fn route_input(
    time: Res<Time>,
    keys: Option<Res<ButtonInput<KeyCode>>>,
    pads: Query<(Entity, &Gamepad)>,
    mirror: Option<Res<TvMirror>>,
    mut repeat: Local<NavRepeat>,
    mut queue: Option<ResMut<IntroQueue>>,
    mut visible: Option<ResMut<InputFocusVisible>>,
    mut level: ResMut<LevelPads>,
    hero: Option<ResMut<HeroInput>>,
    actions: Option<ResMut<HeroActions>>,
    mut out: MessageWriter<TvInput>,
) {
    let (mut frame, dpad, stick) = read_devices(keys.as_deref(), pads.iter().map(|(_, p)| p));
    let level_pads = level.frames(pads.iter());
    let moved = repeat.step(dpad, stick, time.elapsed());
    frame.inputs.extend(moved.map(TvInput::Move));
    if let Some(press) = mirror.and_then(|m| m.2.pop()) {
        info!(?press, "tv press posted");
        frame.pressed = true;
        frame.inputs.push(press.input());
    }
    if (frame.pressed || moved.is_some())
        && let Some(visible) = visible.as_mut()
        && !visible.0
    {
        visible.0 = true;
    }

    let playing = queue.as_ref().is_some_and(|q| q.playing().is_some());
    let layer = Layer::top(playing, level.shows());
    if layer != Layer::Screen {
        repeat.swallow();
    }
    // The hero reads the walking pad only while the level's layer is on top
    // and the portal doesn't hold the level (AG.29); under an intro or a
    // hold, nothing, and a face held through it stays his only once let go.
    let held = level.held();
    let mut release = false;
    let walked = match (layer, level.seat_mut()) {
        (Layer::Level, Some(seat)) if held => {
            // Past the hold's first frame no attack is kept for a step
            // after it (AH.24's attacks under AG.29's hold).
            release = seat.holding();
            Some(seat.hold(&level_pads))
        }
        (Layer::Level, Some(seat)) => Some(seat.step(&level_pads)),
        (_, Some(seat)) => {
            seat.swallow(&level_pads);
            Some(HeroPad::STILL)
        }
        (_, None) => None,
    };
    if let (Some(walked), Some(mut actions)) = (walked, actions) {
        let next = {
            let mut a = *actions;
            if release {
                a.release();
            } else {
                a.frame(walked.actions(), walked.tapped());
            }
            a
        };
        if *actions != next {
            *actions = next;
        }
    }
    if let (Some(walked), Some(mut hero)) = (walked, hero)
        && (hero.0.stick, hero.0.buttons) != (walked.stick, walked.buttons)
    {
        hero.0.stick = walked.stick;
        hero.0.buttons = walked.buttons;
    }
    match layer.take(frame) {
        Taken::Skip => {
            if let Some(queue) = queue.as_mut() {
                info!("intro skipped by a key or button");
                intro_player::apply(queue, IntroEvent::Skip);
            }
        }
        Taken::Screen(inputs) => {
            for input in inputs {
                debug!(?input, "tv input");
                out.write(input);
            }
        }
        Taken::Level | Taken::Nothing => {}
    }
}

/// Where the focus belongs on a screen whose choices are `(entity, id,
/// default)`: where it is if that's one of them, else the one with the id it
/// last had (a button respawned under it), else the default; nowhere on a
/// screen without choices.
pub(crate) fn reseed<'a>(
    now: Option<Entity>,
    choices: &[(Entity, &'a str, bool)],
    last: Option<&str>,
) -> Option<(Entity, &'a str)> {
    now.and_then(|e| choices.iter().find(|c| c.0 == e))
        .or_else(|| last.and_then(|id| choices.iter().find(|c| c.1 == id)))
        .or_else(|| choices.iter().find(|c| c.2))
        .map(|c| (c.0, c.1))
}

/// Focus never strays from the screen's choices (a click off a button clears
/// it, a respawned button takes its entity along): it goes back per
/// [`reseed`], the default being the `AutoFocus` one. An intro's overlay
/// keeps it while it plays.
fn keep_focus(
    focus: Option<ResMut<InputFocus>>,
    choices: Query<(Entity, &FocusId, Has<AutoFocus>)>,
    overlays: Query<(), With<IntroOverlay>>,
    mut last: Local<Option<&'static str>>,
) {
    let Some(mut focus) = focus else {
        return;
    };
    let now = focus.get();
    if now.is_some_and(|e| overlays.contains(e)) {
        return;
    }
    let choices: Vec<_> = choices
        .iter()
        .map(|(e, id, auto)| (e, id.0, auto))
        .collect();
    let Some((entity, id)) = reseed(now, &choices, *last) else {
        return;
    };
    *last = Some(id);
    if now != Some(entity) {
        debug!(choice = id, "focus back on the screen's choice");
        focus.set(entity, FocusCause::Navigated);
    }
}

/// The screen's inputs. Activate presses the focused button, which shows
/// `Pressed` until South and NumpadEnter are both up; Back closes the
/// startup-error screens; Home does nothing yet; Move waits for AP.7's
/// navigator.
fn act_on_input(
    mut commands: Commands,
    mut inputs: MessageReader<TvInput>,
    focus: Option<Res<InputFocus>>,
    buttons: Query<(), (With<Button>, Without<InteractionDisabled>)>,
    current: Option<Res<Current>>,
    keys: Option<Res<ButtonInput<KeyCode>>>,
    pads: Query<&Gamepad>,
    mut held: Local<Option<Entity>>,
    mut exit: MessageWriter<AppExit>,
) {
    let down = pads.iter().any(|p| p.pressed(GamepadButton::South))
        || keys.is_some_and(|k| k.pressed(KeyCode::NumpadEnter));
    if !down
        && let Some(was) = held.take()
        && let Ok(mut button) = commands.get_entity(was)
    {
        button.try_remove::<Pressed>();
    }
    for input in inputs.read() {
        match *input {
            TvInput::Activate => {
                let focused = focus.as_ref().and_then(|f| f.get());
                let Some(button) = focused.filter(|e| buttons.contains(*e)) else {
                    continue;
                };
                commands.entity(button).insert(Pressed);
                *held = Some(button);
                commands.trigger(Activate { entity: button });
            }
            TvInput::Back => {
                if let Some(current) = current.as_ref().filter(|c| c.screen.escape_quits()) {
                    info!(screen = ?current.screen, "closing on back");
                    exit.write(AppExit::Success);
                }
            }
            TvInput::Home | TvInput::Move(_) => {}
        }
    }
}

/// What the screen layer heard, for the headless tests.
#[cfg(test)]
pub(crate) mod heard {
    use super::*;

    #[derive(Resource, Default)]
    pub(crate) struct Heard(pub(crate) Vec<TvInput>);

    pub(crate) fn listen(app: &mut App) {
        app.init_resource::<Heard>().add_systems(Update, record);
    }

    fn record(mut inputs: MessageReader<TvInput>, mut heard: ResMut<Heard>) {
        heard.0.extend(inputs.read().copied());
    }

    /// Everything heard since the last call.
    pub(crate) fn take(app: &mut App) -> Vec<TvInput> {
        std::mem::take(&mut app.world_mut().resource_mut::<Heard>().0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const MS: Duration = Duration::from_millis(1);

    fn at(ms: u64) -> Duration {
        Duration::from_millis(ms)
    }

    const UP: Vec2 = Vec2::Y;

    /// The moves a hold of `dpad` makes, sampled every 10 ms from 0 to `until_ms`.
    fn moves(dpad: Vec2, until_ms: u64) -> Vec<u64> {
        let mut repeat = NavRepeat::default();
        (0..=until_ms)
            .step_by(10)
            .filter(|ms| repeat.step(dpad, Vec2::ZERO, at(*ms)).is_some())
            .collect()
    }

    #[test]
    fn a_press_moves_once_then_repeats_after_the_delay_at_the_interval() {
        let delay = REPEAT_DELAY.as_millis() as u64;
        let every = REPEAT_EVERY.as_millis() as u64;
        // One on the press, none before the delay, then one per interval.
        let want: Vec<u64> = std::iter::once(0)
            .chain((0..4).map(|n| delay + n * every))
            .collect();
        assert_eq!(moves(UP, delay + 3 * every + 9), want);
        let mut repeat = NavRepeat::default();
        assert_eq!(
            repeat.step(UP, Vec2::ZERO, Duration::ZERO),
            Some(CompassOctant::North)
        );
        assert_eq!(repeat.step(UP, Vec2::ZERO, REPEAT_DELAY - MS), None);
        assert_eq!(
            repeat.step(UP, Vec2::ZERO, REPEAT_DELAY),
            Some(CompassOctant::North)
        );
    }

    #[test]
    fn a_release_stops_the_repeats_and_the_next_press_moves_at_once() {
        let mut repeat = NavRepeat::default();
        assert!(repeat.step(UP, Vec2::ZERO, at(0)).is_some());
        assert_eq!(repeat.step(Vec2::ZERO, Vec2::ZERO, at(100)), None);
        // Released past when the repeat was due: nothing.
        assert_eq!(repeat.step(Vec2::ZERO, Vec2::ZERO, at(1000)), None);
        assert_eq!(
            repeat.step(-Vec2::X, Vec2::ZERO, at(1010)),
            Some(CompassOctant::West)
        );
        // Turning while held is a new press.
        assert_eq!(
            repeat.step(Vec2::X, Vec2::ZERO, at(1020)),
            Some(CompassOctant::East)
        );
        assert_eq!(repeat.step(Vec2::X, Vec2::ZERO, at(1030)), None);
    }

    #[test]
    fn the_stick_moves_only_past_its_threshold() {
        let mut repeat = NavRepeat::default();
        let under = Vec2::new(0.0, -(STICK_THRESHOLD - 0.01));
        for ms in (0..2000).step_by(10) {
            assert_eq!(repeat.step(Vec2::ZERO, under, at(ms)), None, "{ms} ms");
        }
        let past = Vec2::new(0.0, -0.9);
        assert_eq!(
            repeat.step(Vec2::ZERO, past, at(2000)),
            Some(CompassOctant::South)
        );
        // The D-pad wins over a stick held another way.
        let mut repeat = NavRepeat::default();
        assert_eq!(repeat.step(Vec2::X, past, at(0)), Some(CompassOctant::East));
        // The navigator takes all eight octants, so a diagonal stays one.
        let mut repeat = NavRepeat::default();
        assert_eq!(
            repeat.step(Vec2::ZERO, Vec2::new(0.7, 0.7), at(0)),
            Some(CompassOctant::NorthEast)
        );
    }

    #[test]
    fn a_swallowed_hold_moves_nothing_until_let_go() {
        let mut repeat = NavRepeat::default();
        // Nothing held: swallowing keeps nothing for later.
        repeat.swallow();
        assert!(repeat.step(UP, Vec2::ZERO, at(0)).is_some());
        repeat.swallow();
        for ms in (10..2000).step_by(10) {
            assert_eq!(repeat.step(UP, Vec2::ZERO, at(ms)), None, "{ms} ms");
        }
        assert_eq!(repeat.step(Vec2::ZERO, Vec2::ZERO, at(2000)), None);
        assert!(repeat.step(UP, Vec2::ZERO, at(2010)).is_some());
    }

    /// AP.5 review: once the layer that swallowed a hold is gone, turning
    /// it is a new press and moves; the old direction still doesn't.
    #[test]
    fn a_turn_after_a_swallowed_hold_is_a_new_press() {
        let mut repeat = NavRepeat::default();
        assert!(repeat.step(-Vec2::X, Vec2::ZERO, at(0)).is_some());
        repeat.swallow();
        assert_eq!(repeat.step(-Vec2::X, Vec2::ZERO, at(1000)), None);
        // Up joins the held Left: north-west, a direction never swallowed.
        assert_eq!(
            repeat.step(Vec2::new(-1.0, 1.0), Vec2::ZERO, at(1010)),
            Some(CompassOctant::NorthWest)
        );
        // Swallowed again in the frame it turned (the layer still up): nothing
        // until the next turn or release.
        let mut repeat = NavRepeat::default();
        assert!(repeat.step(-Vec2::X, Vec2::ZERO, at(0)).is_some());
        repeat.swallow();
        assert!(repeat.step(UP, Vec2::ZERO, at(10)).is_some());
        repeat.swallow();
        for ms in (20..1000).step_by(10) {
            assert_eq!(repeat.step(UP, Vec2::ZERO, at(ms)), None, "{ms} ms");
        }
    }

    /// AP.5 review: the repeats keep their cadence at a frame rate that
    /// doesn't divide it (each one due on the last one's schedule, not on the
    /// late frame that made it: at 30 Hz that drifts to one every 167 ms), and
    /// a stall gives one move, not a burst.
    #[test]
    fn a_hold_keeps_its_cadence_at_30_hz_and_after_a_stall() {
        let frame = Duration::from_nanos(33_333_333);
        let mut repeat = NavRepeat::default();
        let mut moves = Vec::new();
        for n in 0..=90u32 {
            let now = frame * n;
            if repeat.step(UP, Vec2::ZERO, now).is_some() {
                moves.push(now);
            }
        }
        // 3 s: the press, then one at 400 ms and every 150 ms after,
        // 400 + 17 × 150 = 2950 being the last inside 3000.
        assert_eq!(moves.len(), 1 + 1 + 17, "{moves:?}");
        for (i, at) in moves.iter().enumerate().skip(1) {
            let due = REPEAT_DELAY + REPEAT_EVERY * (i as u32 - 1);
            assert!(
                *at >= due && *at < due + frame,
                "move {i} at {at:?}, due {due:?}"
            );
        }
        // A one-second stall mid-hold: one move when it ends, then on the
        // cadence again from there.
        let mut repeat = NavRepeat::default();
        assert!(repeat.step(UP, Vec2::ZERO, at(0)).is_some());
        assert!(repeat.step(UP, Vec2::ZERO, at(400)).is_some());
        assert!(repeat.step(UP, Vec2::ZERO, at(1550)).is_some());
        assert_eq!(repeat.step(UP, Vec2::ZERO, at(1560)), None);
        assert_eq!(repeat.step(UP, Vec2::ZERO, at(1699)), None);
        assert!(repeat.step(UP, Vec2::ZERO, at(1700)).is_some());
    }

    fn south() -> Frame {
        Frame {
            pressed: true,
            inputs: vec![TvInput::Activate],
        }
    }

    #[test]
    fn the_top_layer_takes_the_press_and_nothing_under_it_sees_it() {
        // South during an intro skips it and reaches no screen, over the
        // level too.
        assert_eq!(Layer::top(true, false), Layer::Intro);
        assert_eq!(Layer::top(true, true), Layer::Intro);
        assert_eq!(Layer::top(true, false).take(south()), Taken::Skip);
        assert_eq!(
            Layer::top(false, false).take(south()),
            Taken::Screen(vec![TvInput::Activate])
        );
        // The level reads the pads itself: the screen hears nothing.
        assert_eq!(Layer::top(false, true), Layer::Level);
        assert_eq!(Layer::Level.take(south()), Taken::Level);
        // A stick move is no button: it doesn't skip, and doesn't move under it.
        let stick = Frame {
            pressed: false,
            inputs: vec![TvInput::Move(CompassOctant::East)],
        };
        assert_eq!(Layer::Intro.take(stick.clone()), Taken::Nothing);
        assert_eq!(
            Layer::Screen.take(stick),
            Taken::Screen(vec![TvInput::Move(CompassOctant::East)])
        );
        // Any other key skips an intro and means nothing to a screen.
        let other = Frame {
            pressed: true,
            inputs: Vec::new(),
        };
        assert_eq!(Layer::Intro.take(other.clone()), Taken::Skip);
        assert_eq!(Layer::Screen.take(other), Taken::Nothing);
    }

    /// AB.10's skip rule, moved in: any key or pad button just pressed.
    #[test]
    fn any_key_or_controller_button_is_a_press() {
        let none: [Gamepad; 0] = [];
        let mut keys = ButtonInput::<KeyCode>::default();
        assert!(!read_devices(Some(&keys), none.iter()).0.pressed);
        keys.press(KeyCode::KeyQ);
        assert!(read_devices(Some(&keys), none.iter()).0.pressed);
        keys.clear();
        assert!(
            !read_devices(Some(&keys), none.iter()).0.pressed,
            "held, not new"
        );
        let mut pad = Gamepad::default();
        assert!(!read_devices(None, std::iter::once(&pad)).0.pressed);
        pad.digital_mut().press(GamepadButton::Start);
        let (frame, _, _) = read_devices(None, std::iter::once(&pad));
        assert!(frame.pressed && frame.inputs.is_empty(), "{frame:?}");
    }

    #[test]
    fn buttons_and_keys_map_by_position_and_enter_stays_the_widgets() {
        let mut pad = Gamepad::default();
        for button in [
            GamepadButton::South,
            GamepadButton::East,
            GamepadButton::Mode,
        ] {
            pad.digital_mut().press(button);
        }
        let (frame, _, _) = read_devices(None, std::iter::once(&pad));
        assert_eq!(
            frame.inputs,
            [TvInput::Activate, TvInput::Back, TvInput::Home]
        );

        let mut keys = ButtonInput::<KeyCode>::default();
        keys.press(KeyCode::NumpadEnter);
        keys.press(KeyCode::Escape);
        keys.press(KeyCode::Enter);
        keys.press(KeyCode::Space);
        let none: [Gamepad; 0] = [];
        let (frame, _, _) = read_devices(Some(&keys), none.iter());
        assert_eq!(frame.inputs, [TvInput::Activate, TvInput::Back]);
    }

    #[test]
    fn the_dpad_and_arrows_hold_a_direction_and_the_widest_stick_wins() {
        let mut keys = ButtonInput::<KeyCode>::default();
        keys.press(KeyCode::ArrowLeft);
        let mut pad = Gamepad::default();
        pad.digital_mut().press(GamepadButton::DPadLeft);
        pad.digital_mut().press(GamepadButton::DPadUp);
        let (_, dpad, _) = read_devices(Some(&keys), std::iter::once(&pad));
        assert_eq!(dpad, Vec2::new(-1.0, 1.0), "clamped");

        let mut calm = Gamepad::default();
        let mut pushed = Gamepad::default();
        use bevy::input::gamepad::{GamepadAxis, GamepadInput};
        calm.analog_mut()
            .set(GamepadInput::Axis(GamepadAxis::LeftStickX), 0.2);
        pushed
            .analog_mut()
            .set(GamepadInput::Axis(GamepadAxis::LeftStickY), -0.8);
        let (_, _, stick) = read_devices(None, [&calm, &pushed].into_iter());
        assert_eq!(stick, Vec2::new(0.0, -0.8));
    }

    #[test]
    fn focus_goes_back_to_the_choice_it_had_else_the_default() {
        let mut world = World::new();
        let [first, second, other] = [(); 3].map(|_| world.spawn_empty().id());
        let choices = [(first, "first", true), (second, "second", false)];
        assert_eq!(
            reseed(Some(second), &choices, Some("first")),
            Some((second, "second")),
            "where it is"
        );
        // Off the choices (a click elsewhere, a respawned button): the one
        // it last had.
        for now in [Some(other), None] {
            assert_eq!(
                reseed(now, &choices, Some("second")),
                Some((second, "second"))
            );
        }
        assert_eq!(
            reseed(None, &choices, Some("gone")),
            Some((first, "first")),
            "the default"
        );
        assert_eq!(reseed(None, &choices, None), Some((first, "first")));
        assert_eq!(reseed(Some(other), &[], Some("first")), None);
    }

    #[test]
    fn posted_names_parse_and_unknown_ones_dont() {
        for (name, press) in [
            ("south", TestPress::South),
            ("east", TestPress::East),
            ("mode", TestPress::Mode),
            ("up", TestPress::Up),
            ("down", TestPress::Down),
            ("left", TestPress::Left),
            ("right", TestPress::Right),
        ] {
            assert_eq!(TestPress::parse(name), Some(press));
        }
        for name in ["jump", "South", "", "start"] {
            assert_eq!(TestPress::parse(name), None, "{name}");
        }
        let posted = PostedPresses::default();
        for _ in 0..POSTED_MAX + 3 {
            posted.push(TestPress::Up);
        }
        assert_eq!(posted.len(), POSTED_MAX);
    }
}
