//! Pads on macOS through Apple's GameController framework (openspec AG.53,
//! design decision 12; pad-input). AG.5 found gilrs connects chotchki's
//! 8BitDo Ultimate 2 over Bluetooth on the Mac and then reads nothing (and
//! neither its dongle nor its cable has a HID interface there), while
//! GameController reads every element of the Bluetooth pad. So on macOS this
//! is the pad source and gilrs is off, one source per pad (its plugin isn't
//! added; the crate is still built in, through Bevy's `ui` collection);
//! gilrs stays the source everywhere else.
//!
//! It feeds Bevy's own gamepad stream as bevy_gilrs and `/api/_test/tv/pad`
//! do (a `GamepadConnectionEvent` and `RawGamepadEvent`s before Bevy's input
//! systems), so the level's layer, the router, the pause and the hero read
//! these pads unchanged:
//!
//! - Buttons by position: a product category's [`Layout`] says which of
//!   Apple's elements sits at the bottom, right, left and top. AG.5 measured
//!   the Switch Pro (the 8BitDo in Switch mode); every other category takes
//!   Apple's default layout, unmeasured.
//! - Sticks as Apple delivers them, x right and y up: no dead zone or
//!   threshold here (the move stick's are guac-motion's, after AG.20's raw
//!   axis settings).
//! - No press lost between frames: each pad's input-state queue is drained
//!   in order every frame, so a press and its release inside one frame both
//!   reach Bevy and the router sees the press. A pad without the queue (an
//!   OS before it) is read once a frame instead, and says so in the log. The
//!   one loss: what a pad sent in the frame it disconnects, which Bevy drops
//!   for a gone pad anyway (bevy_gilrs's pads the same).
//! - Home does nothing in guacamole (chotchki, 2026-10-20). Its system
//!   gesture is asked off on every pad, a preference Apple says the system
//!   needn't respect, and macOS doesn't: at AG.51 the Games app opened on
//!   Home with the ask in place. Keeping Home from macOS is the backlog's
//!   Home menu line.
//! - The letters Apple names the face elements by, per position
//!   ([`FacesPrinted`]), for the prompt resolver.
//!
//! [`Controllers`] is what this reads of the framework, so the tests run a
//! fake; [`apple`] is the framework itself.

use bevy::input::InputSystems;
use bevy::input::gamepad::{
    GamepadAxis, GamepadButton, GamepadConnection, GamepadConnectionEvent,
    RawGamepadAxisChangedEvent, RawGamepadButtonChangedEvent, RawGamepadEvent,
};
use bevy::prelude::*;
use tracing::{info, warn};

#[cfg(target_os = "macos")]
pub mod apple;

/// An element of Apple's extended gamepad, by Apple's name for it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Element {
    A,
    B,
    X,
    Y,
    LeftShoulder,
    RightShoulder,
    LeftTrigger,
    RightTrigger,
    Menu,
    Options,
    Home,
    Share,
    LeftThumbstickButton,
    RightThumbstickButton,
    DpadUp,
    DpadDown,
    DpadLeft,
    DpadRight,
}

impl Element {
    pub const COUNT: usize = 18;

    pub const ALL: [Element; Element::COUNT] = [
        Element::A,
        Element::B,
        Element::X,
        Element::Y,
        Element::LeftShoulder,
        Element::RightShoulder,
        Element::LeftTrigger,
        Element::RightTrigger,
        Element::Menu,
        Element::Options,
        Element::Home,
        Element::Share,
        Element::LeftThumbstickButton,
        Element::RightThumbstickButton,
        Element::DpadUp,
        Element::DpadDown,
        Element::DpadLeft,
        Element::DpadRight,
    ];

    /// The four face elements, in Apple's letter order.
    pub const FACES: [Element; 4] = [Element::A, Element::B, Element::X, Element::Y];

    fn index(self) -> usize {
        self as usize
    }
}

/// One reading of a pad: every element's pressed state and both sticks,
/// x right and y up, each −1 to 1 as Apple delivers them.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct PadState {
    pub pressed: [bool; Element::COUNT],
    pub left: Vec2,
    pub right: Vec2,
}

impl PadState {
    pub fn is_down(&self, element: Element) -> bool {
        self.pressed[element.index()]
    }

    pub fn set(&mut self, element: Element, down: bool) {
        self.pressed[element.index()] = down;
    }

    /// A copy with `element` down or up, for building readings inline.
    pub fn with(mut self, element: Element, down: bool) -> Self {
        self.set(element, down);
        self
    }
}

/// Which of Apple's elements sit at the face positions on a product
/// category.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Layout {
    /// Bottom, right, left, top.
    pub faces: [Element; 4],
    /// The measurement behind it; none for Apple's default, unmeasured.
    pub measured: Option<&'static str>,
}

/// The Switch Pro, as AG.5 measured it: Apple names the bottom face element
/// B, the right A, the left Y and the top X, as Nintendo prints them (the
/// 8BitDo's printing matches, chotchki).
pub const SWITCH_PRO: Layout = Layout {
    faces: [Element::B, Element::A, Element::Y, Element::X],
    measured: Some("AG.5, 2026-10-20: an 8BitDo Ultimate 2 in Switch mode over Bluetooth"),
};

/// Apple's documented default: A at the bottom, B on the right, X on the
/// left, Y at the top (the Xbox layout, which Apple's own extended gamepad
/// names its buttons by). Unmeasured on any pad here.
pub const APPLE_DEFAULT: Layout = Layout {
    faces: [Element::A, Element::B, Element::X, Element::Y],
    measured: None,
};

/// The layouts by the product category GameController reports; any category
/// not here takes [`APPLE_DEFAULT`].
pub const LAYOUTS: &[(&str, Layout)] = &[("Switch Pro Controller", SWITCH_PRO)];

/// The layout for a pad's `category`; [`APPLE_DEFAULT`] for one not in
/// [`LAYOUTS`].
pub fn layout(category: &str) -> Layout {
    LAYOUTS
        .iter()
        .find(|(c, _)| *c == category)
        .map_or(APPLE_DEFAULT, |(_, l)| *l)
}

/// Bevy's name for `element` on a pad of `layout`: the faces by position,
/// the rest by role (Menu is Start and Options Select, as on every pad
/// gilrs reads; Home is Mode).
pub fn button(layout: &Layout, element: Element) -> GamepadButton {
    const POSITIONS: [GamepadButton; 4] = [
        GamepadButton::South,
        GamepadButton::East,
        GamepadButton::West,
        GamepadButton::North,
    ];
    if let Some(i) = layout.faces.iter().position(|f| *f == element) {
        return POSITIONS[i];
    }
    match element {
        Element::LeftShoulder => GamepadButton::LeftTrigger,
        Element::RightShoulder => GamepadButton::RightTrigger,
        Element::LeftTrigger => GamepadButton::LeftTrigger2,
        Element::RightTrigger => GamepadButton::RightTrigger2,
        Element::Menu => GamepadButton::Start,
        Element::Options => GamepadButton::Select,
        Element::Home => GamepadButton::Mode,
        // No Bevy name: a press, which only "any button" reads.
        Element::Share => GamepadButton::Other(0),
        Element::LeftThumbstickButton => GamepadButton::LeftThumb,
        Element::RightThumbstickButton => GamepadButton::RightThumb,
        Element::DpadUp => GamepadButton::DPadUp,
        Element::DpadDown => GamepadButton::DPadDown,
        Element::DpadLeft => GamepadButton::DPadLeft,
        Element::DpadRight => GamepadButton::DPadRight,
        // Every layout places all four faces.
        Element::A | Element::B | Element::X | Element::Y => unreachable!("a face is placed"),
    }
}

/// What Apple names an element by on the pad.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Glyph {
    /// Its SF Symbol, `b.circle`.
    pub symbol: Option<String>,
    /// Its name, `B Button`.
    pub localized: Option<String>,
}

impl Glyph {
    /// The face letter it names (A, B, X or Y), from the symbol, else the
    /// name; none for anything else (a PlayStation shape, a name Apple
    /// doesn't give), so no prompt claims a letter Apple didn't.
    pub fn letter(&self) -> Option<char> {
        let from_symbol = self
            .symbol
            .as_deref()
            .and_then(|s| s.split_once('.'))
            .map(|(head, _)| head);
        let from_name = self
            .localized
            .as_deref()
            .and_then(|n| n.strip_suffix(" Button"));
        let face = |s: &str| {
            let mut chars = s.chars();
            match (chars.next(), chars.next()) {
                (Some(c), None) if "abxyABXY".contains(c) => Some(c.to_ascii_uppercase()),
                _ => None,
            }
        };
        from_symbol
            .and_then(face)
            .or_else(|| from_name.and_then(face))
    }
}

/// The letters printed on a pad's face buttons, bottom, right, left and top,
/// as GameController names the element at each position: the prompt
/// resolver's input on macOS, where pads come without ids.
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
pub struct FacesPrinted(pub [Option<char>; 4]);

/// What the framework tells about a controller that just connected.
#[derive(Debug, Clone, PartialEq)]
pub struct Opened {
    /// Its vendor name (`Pro Controller` for the 8BitDo in Switch mode).
    pub name: String,
    /// Its product category (`Switch Pro Controller`).
    pub category: String,
    /// A, B, X and Y's glyphs, in [`Element::FACES`]'s order.
    pub glyphs: [Glyph; 4],
    /// Its reading as it connected.
    pub state: PadState,
}

/// GameController as this module reads it: the framework on macOS
/// ([`apple`]), a fake in the tests.
pub trait Controllers {
    /// The controllers connected now, the longest connected first, each by a
    /// key that stays its own while it's connected.
    fn connected(&mut self) -> Vec<u64>;
    /// A controller just connected: what the framework tells about it; none
    /// for one that isn't a gamepad.
    fn open(&mut self, key: u64) -> Option<Opened>;
    /// Buffer its readings, `depth` at most, for [`Controllers::next_state`];
    /// false where the OS has no queue and the reading is taken once a call.
    fn queue(&mut self, key: u64, depth: usize) -> bool;
    /// Ask the system to leave its Home button's gesture off, a preference
    /// macOS may ignore (it did at AG.51). None for a pad without Home, else
    /// whether the system still reports Home bound to a gesture after the
    /// ask.
    fn ask_home_gesture_off(&mut self, key: u64) -> Option<bool>;
    /// The oldest reading buffered since the last call, or none once drained.
    fn next_state(&mut self, key: u64) -> Option<PadState>;
    /// Let go of a controller that's no longer connected (the key may come
    /// back as another).
    fn close(&mut self, key: u64);
}

/// Readings each pad buffers between frames: about two seconds of a
/// Bluetooth pad's reports (AG.5's logs: a change every 15-16 ms while it
/// moves), so a long frame (a level's load) loses nothing. Past it the
/// framework drops the oldest.
pub const QUEUE_DEPTH: usize = 128;

/// The most readings one pad hands over in a frame: past the queue's depth
/// twice over, only a framework that never runs dry would reach it.
const DRAIN_MAX: usize = 2 * QUEUE_DEPTH;

/// What one frame of GameController gives Bevy, in order.
#[derive(Debug, Clone, PartialEq)]
pub enum PadEvent {
    Connected {
        entity: Entity,
        name: String,
        faces: FacesPrinted,
    },
    Disconnected(Entity),
    Button(Entity, GamepadButton, f32),
    Axis(Entity, GamepadAxis, f32),
}

/// What a connected pad stands at.
struct OpenPad {
    key: u64,
    entity: Entity,
    layout: Layout,
    last: PadState,
}

/// The pads GameController reports, as Bevy entities.
pub struct GcPads {
    controllers: Box<dyn Controllers>,
    /// In the order they connected.
    open: Vec<OpenPad>,
    /// Connected controllers that aren't gamepads, not asked again.
    refused: Vec<u64>,
}

impl GcPads {
    pub fn new(controllers: Box<dyn Controllers>) -> Self {
        Self {
            controllers,
            open: Vec::new(),
            refused: Vec::new(),
        }
    }

    /// This frame's events: pads gone, pads new (`spawn` makes each one's
    /// entity) and every reading buffered since the last frame, oldest
    /// first.
    pub fn poll(&mut self, mut spawn: impl FnMut() -> Entity) -> Vec<PadEvent> {
        let mut out = Vec::new();
        let now = self.controllers.connected();
        let controllers = &mut self.controllers;
        self.open.retain(|pad| {
            let still = now.contains(&pad.key);
            if !still {
                info!(pad = ?pad.entity, "GameController pad disconnected");
                controllers.close(pad.key);
                out.push(PadEvent::Disconnected(pad.entity));
            }
            still
        });
        self.refused.retain(|key| {
            let still = now.contains(key);
            if !still {
                controllers.close(*key);
            }
            still
        });
        for key in now {
            if self.open.iter().any(|p| p.key == key) || self.refused.contains(&key) {
                continue;
            }
            let Some(opened) = self.controllers.open(key) else {
                info!(key, "GameController device isn't a gamepad: left alone");
                self.refused.push(key);
                continue;
            };
            let entity = spawn();
            let layout = layout(&opened.category);
            let faces = FacesPrinted(layout.faces.map(|f| {
                let i = Element::FACES.iter().position(|e| *e == f);
                i.and_then(|i| opened.glyphs[i].letter())
            }));
            let queued = self.controllers.queue(key, QUEUE_DEPTH);
            let home = match self.controllers.ask_home_gesture_off(key) {
                None => "no Home",
                Some(false) => "gesture asked off, macOS reports it unbound",
                Some(true) => "gesture asked off, macOS reports it still bound",
            };
            info!(
                pad = ?entity,
                name = %opened.name,
                category = %opened.category,
                layout = layout.measured.unwrap_or("Apple's default, unmeasured"),
                faces = ?faces.0,
                home,
                queued,
                "GameController pad connected"
            );
            if !queued {
                warn!(
                    pad = ?entity,
                    "no input-state queue on this macOS: the pad is read once a frame, so a press \
                     shorter than a frame can be missed"
                );
            }
            out.push(PadEvent::Connected {
                entity,
                name: opened.name,
                faces,
            });
            let mut pad = OpenPad {
                key,
                entity,
                layout,
                last: PadState::default(),
            };
            changes(&mut pad, opened.state, &mut out);
            self.open.push(pad);
        }
        for pad in &mut self.open {
            let mut drained = 0;
            while let Some(state) = self.controllers.next_state(pad.key) {
                changes(pad, state, &mut out);
                drained += 1;
                if drained == DRAIN_MAX {
                    warn!(pad = ?pad.entity, "a frame's readings past {DRAIN_MAX}: the rest wait");
                    break;
                }
            }
        }
        out
    }
}

/// What moved from `pad`'s last reading to `now`, as Bevy's raw events.
fn changes(pad: &mut OpenPad, now: PadState, out: &mut Vec<PadEvent>) {
    let last = pad.last;
    for (axis, before, after) in [
        (GamepadAxis::LeftStickX, last.left.x, now.left.x),
        (GamepadAxis::LeftStickY, last.left.y, now.left.y),
        (GamepadAxis::RightStickX, last.right.x, now.right.x),
        (GamepadAxis::RightStickY, last.right.y, now.right.y),
    ] {
        if before != after {
            out.push(PadEvent::Axis(pad.entity, axis, after));
        }
    }
    for element in Element::ALL {
        let down = now.is_down(element);
        if last.is_down(element) != down {
            let value = if down { 1.0 } else { 0.0 };
            out.push(PadEvent::Button(
                pad.entity,
                button(&pad.layout, element),
                value,
            ));
        }
    }
    pad.last = now;
}

/// GameController's pads into Bevy's gamepad stream, ahead of Bevy's input
/// systems (as bevy_gilrs's own system runs), on the main thread.
pub fn read_game_controllers(
    mut pads: NonSendMut<GcPads>,
    mut commands: Commands,
    mut connections: MessageWriter<GamepadConnectionEvent>,
    mut raw: MessageWriter<RawGamepadEvent>,
    mut buttons: MessageWriter<RawGamepadButtonChangedEvent>,
    mut axes: MessageWriter<RawGamepadAxisChangedEvent>,
) {
    for event in pads.poll(|| commands.spawn_empty().id()) {
        match event {
            PadEvent::Connected {
                entity,
                name,
                faces,
            } => {
                commands.entity(entity).insert(faces);
                let event = GamepadConnectionEvent::new(
                    entity,
                    GamepadConnection::Connected {
                        name,
                        vendor_id: None,
                        product_id: None,
                    },
                );
                raw.write(event.clone().into());
                connections.write(event);
            }
            PadEvent::Disconnected(entity) => {
                let event = GamepadConnectionEvent::new(entity, GamepadConnection::Disconnected);
                raw.write(event.clone().into());
                connections.write(event);
            }
            PadEvent::Button(entity, button, value) => {
                let event = RawGamepadButtonChangedEvent::new(entity, button, value);
                raw.write(event.into());
                buttons.write(event);
            }
            PadEvent::Axis(entity, axis, value) => {
                let event = RawGamepadAxisChangedEvent::new(entity, axis, value);
                raw.write(event.into());
                axes.write(event);
            }
        }
    }
}

/// `controllers` as the app's pad source.
pub fn install(app: &mut App, controllers: Box<dyn Controllers>) {
    app.insert_non_send(GcPads::new(controllers))
        .add_systems(PreUpdate, read_game_controllers.before(InputSystems));
}

/// Pads from GameController, the framework itself (gilrs off beside it:
/// `main.rs`'s `tv_plugins` disables bevy_gilrs on macOS and adds this,
/// unless `GUAC_TEST_POSTED_PADS_ONLY` leaves both out).
#[cfg(target_os = "macos")]
pub struct GameControllerPads;

#[cfg(target_os = "macos")]
impl Plugin for GameControllerPads {
    fn build(&self, app: &mut App) {
        info!("pads from GameController (gilrs off on macOS)");
        install(app, Box::new(apple::Framework::new()));
    }
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;
    use std::collections::VecDeque;
    use std::rc::Rc;

    use bevy::input::gamepad::Gamepad;

    use super::*;

    /// A controller the fake hands out: what it tells, what it holds and
    /// queues, and what the backend asked of it, for the asserts. Kept once
    /// it disconnects, so its closes can be counted.
    #[derive(Debug, Clone)]
    struct FakePad {
        key: u64,
        connected: bool,
        name: String,
        category: String,
        glyphs: [Glyph; 4],
        gamepad: bool,
        has_home: bool,
        has_queue: bool,
        state: PadState,
        queued: VecDeque<PadState>,
        depth: Option<usize>,
        home_asked: bool,
        /// Without a queue: the reading last handed out, as the framework's
        /// polled source keeps it.
        polled: Option<PadState>,
        opened: usize,
        closed: usize,
    }

    /// GameController as the tests drive it; a clone shares its pads.
    #[derive(Debug, Clone, Default)]
    struct Fake(Rc<RefCell<Vec<FakePad>>>);

    fn glyph(symbol: &str, name: &str) -> Glyph {
        Glyph {
            symbol: Some(symbol.into()),
            localized: Some(name.into()),
        }
    }

    fn apple_glyphs() -> [Glyph; 4] {
        ["a", "b", "x", "y"].map(|l| {
            glyph(
                &format!("{l}.circle"),
                &format!("{} Button", l.to_ascii_uppercase()),
            )
        })
    }

    impl Fake {
        /// A pad of `category` connecting, with Apple's letter glyphs.
        fn connect(&self, key: u64, category: &str) {
            let mut pad = self.pad_template(key);
            pad.category = category.into();
            self.connect_pad(pad);
        }

        /// `pad` connecting; one gone with its key is forgotten.
        fn connect_pad(&self, pad: FakePad) {
            let mut pads = self.0.borrow_mut();
            pads.retain(|p| p.key != pad.key);
            pads.push(pad);
        }

        fn disconnect(&self, key: u64) {
            self.with(key, |p| p.connected = false);
        }

        /// A reading the pad sends: buffered for the next frame, the oldest
        /// dropped past the depth asked for (as the framework drops them),
        /// or where the pad has no queue, its state now.
        fn send(&self, key: u64, state: PadState) {
            let mut pads = self.0.borrow_mut();
            let pad = pads.iter_mut().find(|p| p.key == key).unwrap();
            pad.state = state;
            if let (true, Some(depth)) = (pad.has_queue, pad.depth) {
                pad.queued.push_back(state);
                while pad.queued.len() > depth {
                    pad.queued.pop_front();
                }
            }
        }

        fn pad(&self, key: u64) -> FakePad {
            self.0
                .borrow()
                .iter()
                .find(|p| p.key == key)
                .unwrap()
                .clone()
        }

        fn with<T>(&self, key: u64, f: impl FnOnce(&mut FakePad) -> T) -> Option<T> {
            self.0.borrow_mut().iter_mut().find(|p| p.key == key).map(f)
        }
    }

    impl Controllers for Fake {
        fn connected(&mut self) -> Vec<u64> {
            self.0
                .borrow()
                .iter()
                .filter(|p| p.connected)
                .map(|p| p.key)
                .collect()
        }

        fn open(&mut self, key: u64) -> Option<Opened> {
            self.with(key, |p| {
                p.opened += 1;
                p.gamepad.then(|| Opened {
                    name: p.name.clone(),
                    category: p.category.clone(),
                    glyphs: p.glyphs.clone(),
                    state: p.state,
                })
            })
            .flatten()
        }

        fn queue(&mut self, key: u64, depth: usize) -> bool {
            self.with(key, |p| {
                p.depth = Some(depth);
                p.has_queue
            })
            .unwrap_or(false)
        }

        fn ask_home_gesture_off(&mut self, key: u64) -> Option<bool> {
            self.with(key, |p| {
                p.home_asked = p.has_home;
                p.has_home.then_some(false)
            })
            .flatten()
        }

        fn next_state(&mut self, key: u64) -> Option<PadState> {
            self.with(key, |p| {
                if p.has_queue {
                    p.queued.pop_front()
                } else if p.polled != Some(p.state) {
                    p.polled = Some(p.state);
                    Some(p.state)
                } else {
                    None
                }
            })
            .flatten()
        }

        fn close(&mut self, key: u64) {
            self.with(key, |p| p.closed += 1);
        }
    }

    /// Entities 1, 2, ... for [`GcPads::poll`] without a world.
    fn entities() -> impl FnMut() -> Entity {
        let mut next = 0;
        move || {
            next += 1;
            Entity::from_raw_u32(next).unwrap()
        }
    }

    fn polled(pads: &mut GcPads, spawn: &mut impl FnMut() -> Entity) -> Vec<PadEvent> {
        pads.poll(&mut *spawn)
    }

    /// A pad's connection: its entity, name and the letters printed at each
    /// position, by its category's layout; the framework asked to buffer
    /// and to leave Home's gesture off; a disconnection, closed once.
    #[test]
    fn connect_and_disconnect() {
        let fake = Fake::default();
        let mut pads = GcPads::new(Box::new(fake.clone()));
        let mut spawn = entities();
        assert_eq!(polled(&mut pads, &mut spawn), []);
        fake.connect(7, "Switch Pro Controller");
        let pad = Entity::from_raw_u32(1).unwrap();
        assert_eq!(
            polled(&mut pads, &mut spawn),
            [PadEvent::Connected {
                entity: pad,
                name: "pad 7".into(),
                faces: FacesPrinted([Some('B'), Some('A'), Some('Y'), Some('X')]),
            }]
        );
        let opened = fake.pad(7);
        assert_eq!(opened.depth, Some(QUEUE_DEPTH));
        assert!(opened.home_asked, "Home's gesture asked off");
        assert_eq!(polled(&mut pads, &mut spawn), [], "opened once");
        assert_eq!(fake.pad(7).opened, 1);
        assert_eq!(fake.pad(7).closed, 0);
        fake.disconnect(7);
        assert_eq!(polled(&mut pads, &mut spawn), [PadEvent::Disconnected(pad)]);
        assert_eq!(polled(&mut pads, &mut spawn), []);
        assert_eq!(fake.pad(7).closed, 1, "let go of once");
        // It comes back as a new controller: a new entity, at the back.
        fake.connect(7, "Switch Pro Controller");
        assert!(matches!(
            polled(&mut pads, &mut spawn)[..],
            [PadEvent::Connected { entity, .. }] if entity == Entity::from_raw_u32(2).unwrap()
        ));
    }

    /// Every element and both sticks reach Bevy by position on each layout:
    /// on the Switch Pro, Apple's B is South, A East, Y West and X North;
    /// on Apple's default (an Xbox pad, a DualSense, anything not in the
    /// table) A is South, B East, X West and Y North; the rest by role on
    /// both. Sticks as sent, either way, with nothing of ours in between.
    #[test]
    fn every_button_and_axis_by_position_per_category() {
        use GamepadButton as G;
        let rest = [
            (Element::LeftShoulder, G::LeftTrigger),
            (Element::RightShoulder, G::RightTrigger),
            (Element::LeftTrigger, G::LeftTrigger2),
            (Element::RightTrigger, G::RightTrigger2),
            (Element::Menu, G::Start),
            (Element::Options, G::Select),
            (Element::Home, G::Mode),
            (Element::Share, G::Other(0)),
            (Element::LeftThumbstickButton, G::LeftThumb),
            (Element::RightThumbstickButton, G::RightThumb),
            (Element::DpadUp, G::DPadUp),
            (Element::DpadDown, G::DPadDown),
            (Element::DpadLeft, G::DPadLeft),
            (Element::DpadRight, G::DPadRight),
        ];
        for (category, faces) in [
            (
                "Switch Pro Controller",
                [
                    (Element::B, G::South),
                    (Element::A, G::East),
                    (Element::Y, G::West),
                    (Element::X, G::North),
                ],
            ),
            (
                "Xbox One",
                [
                    (Element::A, G::South),
                    (Element::B, G::East),
                    (Element::X, G::West),
                    (Element::Y, G::North),
                ],
            ),
            (
                "DualSense",
                [
                    (Element::A, G::South),
                    (Element::B, G::East),
                    (Element::X, G::West),
                    (Element::Y, G::North),
                ],
            ),
        ] {
            let fake = Fake::default();
            let mut pads = GcPads::new(Box::new(fake.clone()));
            let mut spawn = entities();
            fake.connect(1, category);
            polled(&mut pads, &mut spawn);
            let pad = Entity::from_raw_u32(1).unwrap();
            let mut covered = Vec::new();
            for (element, want) in faces.into_iter().chain(rest) {
                covered.push(element);
                fake.send(1, PadState::default().with(element, true));
                fake.send(1, PadState::default());
                assert_eq!(
                    polled(&mut pads, &mut spawn),
                    [
                        PadEvent::Button(pad, want, 1.0),
                        PadEvent::Button(pad, want, 0.0)
                    ],
                    "{category}: {element:?}"
                );
            }
            assert_eq!(covered.len(), Element::COUNT, "every element");
            let state = PadState {
                left: Vec2::new(0.8027, -0.004),
                right: Vec2::new(-1.0, 0.25),
                ..default()
            };
            fake.send(1, state);
            assert_eq!(
                polled(&mut pads, &mut spawn),
                [
                    PadEvent::Axis(pad, GamepadAxis::LeftStickX, 0.8027),
                    PadEvent::Axis(pad, GamepadAxis::LeftStickY, -0.004),
                    PadEvent::Axis(pad, GamepadAxis::RightStickX, -1.0),
                    PadEvent::Axis(pad, GamepadAxis::RightStickY, 0.25),
                ],
                "{category}"
            );
            // Only what changed: the left stick's x alone.
            fake.send(
                1,
                PadState {
                    left: Vec2::new(0.8, -0.004),
                    ..state
                },
            );
            assert_eq!(
                polled(&mut pads, &mut spawn),
                [PadEvent::Axis(pad, GamepadAxis::LeftStickX, 0.8)]
            );
        }
        assert_eq!(layout("Switch Pro Controller"), SWITCH_PRO);
        assert!(SWITCH_PRO.measured.is_some());
        for unmeasured in ["Xbox One", "DualSense", "DualShock 4", "MFi", ""] {
            assert_eq!(layout(unmeasured), APPLE_DEFAULT, "{unmeasured}");
        }
        assert_eq!(APPLE_DEFAULT.measured, None);
    }

    /// The letters per position for the resolver: from each face's SF
    /// Symbol, else its name; never a letter Apple didn't give (a
    /// DualSense's shapes give none).
    #[test]
    fn the_letters_apple_prints_per_position() {
        assert_eq!(glyph("b.circle", "B Button").letter(), Some('B'));
        assert_eq!(glyph("y.circle.fill", "").letter(), Some('Y'));
        let named = Glyph {
            symbol: None,
            localized: Some("X Button".into()),
        };
        assert_eq!(named.letter(), Some('X'));
        for shape in [
            glyph("xmark.circle", "Cross Button"),
            glyph("circle.circle", "Circle Button"),
            glyph("square.circle", "Square Button"),
            glyph("triangle.circle", "Triangle Button"),
            glyph("l.rectangle.roundedbottom", "L Button"),
            Glyph::default(),
        ] {
            assert_eq!(shape.letter(), None, "{shape:?}");
        }
        let fake = Fake::default();
        let mut pads = GcPads::new(Box::new(fake.clone()));
        let mut spawn = entities();
        fake.connect(1, "Xbox One");
        let mut sense = fake.pad(1);
        sense.key = 2;
        sense.category = "DualSense".into();
        sense.glyphs = [
            glyph("xmark.circle", "Cross Button"),
            glyph("circle.circle", "Circle Button"),
            glyph("square.circle", "Square Button"),
            glyph("triangle.circle", "Triangle Button"),
        ];
        fake.connect_pad(sense);
        let faces: Vec<_> = polled(&mut pads, &mut spawn)
            .into_iter()
            .filter_map(|e| match e {
                PadEvent::Connected { faces, .. } => Some(faces),
                _ => None,
            })
            .collect();
        assert_eq!(
            faces,
            [
                FacesPrinted([Some('A'), Some('B'), Some('X'), Some('Y')]),
                FacesPrinted([None; 4])
            ]
        );
    }

    /// Two pads that connect in one frame keep the framework's order; one
    /// that connects later comes after both; the first going away leaves
    /// the others in their order.
    #[test]
    fn two_pads_in_connection_order() {
        let fake = Fake::default();
        let mut pads = GcPads::new(Box::new(fake.clone()));
        let mut spawn = entities();
        fake.connect(30, "Switch Pro Controller");
        fake.connect(10, "Xbox One");
        let connected = |events: Vec<PadEvent>| -> Vec<String> {
            events
                .into_iter()
                .filter_map(|e| match e {
                    PadEvent::Connected { name, .. } => Some(name),
                    _ => None,
                })
                .collect()
        };
        assert_eq!(
            connected(polled(&mut pads, &mut spawn)),
            ["pad 30", "pad 10"]
        );
        fake.connect(20, "DualSense");
        assert_eq!(connected(polled(&mut pads, &mut spawn)), ["pad 20"]);
        fake.disconnect(30);
        polled(&mut pads, &mut spawn);
        let order: Vec<u64> = pads.open.iter().map(|p| p.key).collect();
        assert_eq!(order, [10, 20]);
    }

    /// A press and its release buffered inside one frame both reach Bevy in
    /// order, so the pad reads the button just pressed that frame (what
    /// the router reads) and not held.
    #[test]
    fn a_sub_frame_tap_still_reads_as_a_press() {
        let fake = Fake::default();
        let mut app = App::new();
        app.add_plugins((MinimalPlugins, bevy::input::InputPlugin));
        install(&mut app, Box::new(fake.clone()));
        fake.connect(1, "Switch Pro Controller");
        app.update();
        let pad = app
            .world_mut()
            .query_filtered::<Entity, With<Gamepad>>()
            .single(app.world())
            .unwrap();
        fake.send(1, PadState::default().with(Element::B, true));
        fake.send(1, PadState::default());
        app.update();
        let gamepad = app.world().get::<Gamepad>(pad).unwrap();
        assert!(gamepad.just_pressed(GamepadButton::South), "the tap");
        assert!(gamepad.just_released(GamepadButton::South));
        assert!(!gamepad.pressed(GamepadButton::South));
        app.update();
        let gamepad = app.world().get::<Gamepad>(pad).unwrap();
        assert!(!gamepad.just_pressed(GamepadButton::South), "once");
    }

    /// A long frame (a level's load, a hitch) loses nothing the queue holds:
    /// 1.8 s of a Bluetooth pad's reports at AG.5's 15 ms, a tap of the
    /// bottom button each two of them, all reach Bevy in order at the next
    /// frame; past the queue's depth the oldest go, as the framework drops
    /// them.
    #[test]
    fn a_long_frame_keeps_every_reading_the_queue_holds() {
        let fake = Fake::default();
        let mut pads = GcPads::new(Box::new(fake.clone()));
        let mut spawn = entities();
        fake.connect(1, "Switch Pro Controller");
        polled(&mut pads, &mut spawn);
        let pad = Entity::from_raw_u32(1).unwrap();
        let taps = 60;
        for _ in 0..taps {
            fake.send(1, PadState::default().with(Element::B, true));
            fake.send(1, PadState::default());
        }
        let events = polled(&mut pads, &mut spawn);
        let want: Vec<PadEvent> = (0..taps)
            .flat_map(|_| {
                [
                    PadEvent::Button(pad, GamepadButton::South, 1.0),
                    PadEvent::Button(pad, GamepadButton::South, 0.0),
                ]
            })
            .collect();
        assert_eq!(events, want, "every tap, in order");
        // Past the depth: the two oldest readings go, the newest stay.
        for i in 0..QUEUE_DEPTH + 2 {
            fake.send(1, PadState::default().with(Element::B, i % 2 == 0));
        }
        let events = polled(&mut pads, &mut spawn);
        assert_eq!(events.len(), QUEUE_DEPTH);
        assert_eq!(
            (events[0].clone(), events[QUEUE_DEPTH - 1].clone()),
            (
                PadEvent::Button(pad, GamepadButton::South, 1.0),
                PadEvent::Button(pad, GamepadButton::South, 0.0)
            )
        );
    }

    /// Through Bevy: a connection makes a `Gamepad` with Apple's vendor
    /// name, no ids and the letters printed per position beside it; its
    /// sticks and buttons land; a disconnection takes the `Gamepad` off and
    /// leaves the entity; a non-gamepad is left alone and asked once.
    #[test]
    fn pads_reach_bevy_and_leave_it() {
        let fake = Fake::default();
        let mut app = App::new();
        app.add_plugins((MinimalPlugins, bevy::input::InputPlugin));
        install(&mut app, Box::new(fake.clone()));
        let mut remote = fake.pad_template(5);
        remote.gamepad = false;
        fake.connect_pad(remote);
        fake.connect(1, "Switch Pro Controller");
        app.update();
        let found: Vec<(Entity, String)> = app
            .world_mut()
            .query::<(Entity, &Gamepad, &Name)>()
            .iter(app.world())
            .map(|(e, _, n)| (e, n.as_str().to_owned()))
            .collect();
        assert_eq!(found.len(), 1, "the remote isn't a pad");
        let (pad, name) = found[0].clone();
        assert_eq!(name, "pad 1");
        let gamepad = app.world().get::<Gamepad>(pad).unwrap();
        assert_eq!((gamepad.vendor_id(), gamepad.product_id()), (None, None));
        assert_eq!(
            app.world().get::<FacesPrinted>(pad),
            Some(&FacesPrinted([Some('B'), Some('A'), Some('Y'), Some('X')]))
        );
        fake.send(
            1,
            PadState {
                left: Vec2::new(0.0, 1.0),
                ..default()
            }
            .with(Element::A, true),
        );
        app.update();
        let gamepad = app.world().get::<Gamepad>(pad).unwrap();
        assert_eq!(gamepad.left_stick(), Vec2::new(0.0, 1.0));
        assert!(
            gamepad.pressed(GamepadButton::East),
            "Apple's A, on the right"
        );
        app.update();
        assert_eq!(fake.pad(5).opened, 1, "asked once");
        fake.disconnect(1);
        fake.disconnect(5);
        app.update();
        assert!(app.world().get::<Gamepad>(pad).is_none());
        assert!(app.world().get_entity(pad).is_ok(), "the entity stays");
        assert_eq!(
            (fake.pad(1).closed, fake.pad(5).closed),
            (1, 1),
            "both let go of"
        );
    }

    /// Home's system gesture is asked off on every pad that has a Home
    /// button, as it connects (whether macOS honours it is its own: AG.51
    /// saw it not); a pad without one is left as it is.
    #[test]
    fn home_gesture_asked_off_on_each_pad() {
        let fake = Fake::default();
        let mut pads = GcPads::new(Box::new(fake.clone()));
        let mut spawn = entities();
        fake.connect(1, "Switch Pro Controller");
        let mut no_home = fake.pad_template(2);
        no_home.has_home = false;
        fake.connect_pad(no_home);
        polled(&mut pads, &mut spawn);
        assert!(fake.pad(1).home_asked);
        assert!(!fake.pad(2).home_asked);
        // Home then does what any button does: a press, Mode.
        fake.send(1, PadState::default().with(Element::Home, true));
        assert_eq!(
            polled(&mut pads, &mut spawn),
            [PadEvent::Button(
                Entity::from_raw_u32(1).unwrap(),
                GamepadButton::Mode,
                1.0
            )]
        );
    }

    /// A pad on an OS without the queue is read once a frame: what it holds
    /// as the frame reads it, nothing repeated.
    #[test]
    fn a_pad_without_the_queue_is_read_once_a_frame() {
        let fake = Fake::default();
        let mut pads = GcPads::new(Box::new(fake.clone()));
        let mut spawn = entities();
        let mut old = fake.pad_template(1);
        old.has_queue = false;
        fake.connect_pad(old);
        polled(&mut pads, &mut spawn);
        let pad = Entity::from_raw_u32(1).unwrap();
        fake.send(1, PadState::default().with(Element::Menu, true));
        assert_eq!(
            polled(&mut pads, &mut spawn),
            [PadEvent::Button(pad, GamepadButton::Start, 1.0)]
        );
        assert_eq!(polled(&mut pads, &mut spawn), []);
    }

    /// A pad held as it connects: its reading then reaches Bevy at once.
    #[test]
    fn a_pad_connecting_held_sends_what_it_holds() {
        let fake = Fake::default();
        let mut pads = GcPads::new(Box::new(fake.clone()));
        let mut spawn = entities();
        let mut held = fake.pad_template(1);
        held.state = PadState {
            left: Vec2::new(0.5, 0.0),
            ..default()
        }
        .with(Element::Menu, true);
        fake.connect_pad(held);
        let events = polled(&mut pads, &mut spawn);
        let pad = Entity::from_raw_u32(1).unwrap();
        assert_eq!(
            events[1..],
            [
                PadEvent::Axis(pad, GamepadAxis::LeftStickX, 0.5),
                PadEvent::Button(pad, GamepadButton::Start, 1.0)
            ]
        );
    }

    impl Fake {
        /// A Switch Pro to change before connecting it.
        fn pad_template(&self, key: u64) -> FakePad {
            FakePad {
                key,
                connected: true,
                name: format!("pad {key}"),
                category: "Switch Pro Controller".into(),
                glyphs: apple_glyphs(),
                gamepad: true,
                has_home: true,
                has_queue: true,
                state: PadState::default(),
                queued: VecDeque::new(),
                depth: None,
                home_asked: false,
                polled: None,
                opened: 0,
                closed: 0,
            }
        }
    }
}
