//! The TV's one input router (openspec AP, design decision 2). `route_input`
//! reads every pad, the keys the widgets don't take (the arrows, Esc,
//! NumpadEnter) and the presses posted to `/api/_test/tv/press`, then hands
//! the frame to the top layer only: an intro playing over the portal takes
//! any button or key as its skip and nothing under it sees the press;
//! otherwise the screen gets it as [`TvInput`]s. Mapping is positional (the
//! design doc's Controllers): South activates, East goes back, Mode is Home
//! (nothing yet), the D-pad, the arrows and the left stick move, held ones
//! repeating per [`NavRepeat`].
//!
//! Enter and Space are never mapped: `bevy_ui_widgets`' button takes them
//! from the focused entity, so nothing activates twice.

use std::collections::VecDeque;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use bevy::input::InputSystems;
use bevy::input::gamepad::{Gamepad, GamepadButton};
use bevy::input_focus::InputFocusVisible;
use bevy::math::CompassOctant;
use bevy::prelude::*;
use tracing::{debug, info};

use crate::intro::{IntroEvent, IntroQueue};
use crate::intro_player;
use crate::mirror::TvMirror;

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
    /// An intro over the portal: any button or key skips it.
    Intro,
    // A modal goes here, between the two, once one exists.
    /// The screen's own choices.
    Screen,
}

/// What the top layer did with a frame.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Taken {
    Skip,
    Screen(Vec<TvInput>),
    Nothing,
}

impl Layer {
    pub(crate) fn top(intro_playing: bool) -> Self {
        if intro_playing {
            Self::Intro
        } else {
            Self::Screen
        }
    }

    /// The frame as this layer takes it; nothing passes under it.
    pub(crate) fn take(self, frame: Frame) -> Taken {
        match self {
            // A stick move is no button: it neither skips nor reaches the screen.
            Self::Intro if frame.pressed => Taken::Skip,
            Self::Intro => Taken::Nothing,
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
                self.held = Some((dir, now + REPEAT_EVERY));
                dir
            }
            // A press, or the hold turned to another direction.
            _ => {
                self.held = Some((dir, now + REPEAT_DELAY));
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
            .insert_resource(InputFocusVisible(true))
            .add_systems(PreUpdate, route_input.after(InputSystems));
    }
}

fn route_input(
    time: Res<Time>,
    keys: Option<Res<ButtonInput<KeyCode>>>,
    pads: Query<&Gamepad>,
    mirror: Option<Res<TvMirror>>,
    mut repeat: Local<NavRepeat>,
    mut queue: Option<ResMut<IntroQueue>>,
    mut visible: Option<ResMut<InputFocusVisible>>,
    mut out: MessageWriter<TvInput>,
) {
    let (mut frame, dpad, stick) = read_devices(keys.as_deref(), pads.iter());
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
    let layer = Layer::top(playing);
    if layer == Layer::Intro {
        repeat.swallow();
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
        Taken::Nothing => {}
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

    fn south() -> Frame {
        Frame {
            pressed: true,
            inputs: vec![TvInput::Activate],
        }
    }

    #[test]
    fn the_top_layer_takes_the_press_and_nothing_under_it_sees_it() {
        // South during an intro skips it and reaches no screen.
        assert_eq!(Layer::top(true), Layer::Intro);
        assert_eq!(Layer::top(true).take(south()), Taken::Skip);
        assert_eq!(
            Layer::top(false).take(south()),
            Taken::Screen(vec![TvInput::Activate])
        );
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
