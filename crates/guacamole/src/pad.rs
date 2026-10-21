//! The level's input layer (openspec AG.20, design decision 12; pad-input).
//! While the level shows, AP's router ([`crate::input`]) hands this layer the
//! pads, under an intro's skip and over the screens:
//!
//! - The move stick reads raw: Bevy's dead zone and change threshold are off
//!   for both left-stick axes on every pad ([`raw_stick`], set by
//!   [`read_sticks_raw`]), so the value a pad stores is the last one its
//!   source sent (GameController's float on macOS; elsewhere gilrs's, which
//!   bevy_gilrs builds without its own filters). The original's
//!   per-axis dead zone, rescale and cap, and the direction against the view's
//!   heading, are `guac-motion`'s ([`guac_motion::stick`]), inside the
//!   hero's step.
//! - One pad walks the hero, the first connected ([`Seat`]); another pad's
//!   stick and face buttons move no one. Start on any pad pauses under the
//!   one-button overlay, the bottom face button or Start on any pad resumes.
//!   The walking pad lost, or none when the level starts, pauses under the
//!   controller message until a press on any pad makes that pad the hero's.
//!   Select and Home do nothing.
//! - Face buttons by position: the walking pad's bottom, right, left and top
//!   as the hero's input holds them, whatever is printed on them.
//! - Prompts name the label printed on the pad in hand ([`labels`],
//!   [`prompt`]): Xbox letters on a PC layout, Nintendo's on a Switch
//!   layout, the button's position for any other. On macOS the pads come
//!   from GameController (AG.53, [`guacamole::game_controller`]) without
//!   ids, and the letters are the ones Apple names each position's element
//!   by ([`FacesPrinted`]): AG.5 found the 8BitDo's printing matches them
//!   position for position. Elsewhere the id table decides, empty until the
//!   Windows/HTPC bring-up reads the 8BitDo's ids through gilrs there, so a
//!   pad gets the position glyph meanwhile, which names no label it can't
//!   back (design Open question 5's (b)).
//!
//! The corner, settled for the 8BitDo on macOS (AG.49): GameController
//! reads each axis to exactly 1 at full travel and its rim square-ish (about
//! 0.80 an axis at 45°), so a full diagonal's push caps at 1 and walks at
//! top speed. pad-input's Between the axes is a round gate's rim read as is
//! (49 and 118 of 128 at 22.55° off forward), the value a pad that reads
//! round would send.

use bevy::input::gamepad::{AxisSettings, Gamepad, GamepadAxis, GamepadButton, GamepadSettings};
use bevy::prelude::*;
use guac_motion::stick::Stick;
pub(crate) use guacamole::game_controller::FacesPrinted;
use tracing::info;

/// Both left-stick axes as the pad sends them: the live zone the whole travel,
/// no dead zone, every change kept (Bevy's defaults: a dead zone of 0.05
/// either side and changes under 0.01 dropped).
pub(crate) fn raw_stick() -> AxisSettings {
    AxisSettings::new(-1.0, 0.0, 0.0, 1.0, 0.0)
        .expect("the whole travel, no dead zone, no threshold")
}

/// The axes [`raw_stick`] covers.
pub(crate) const MOVE_STICK: [GamepadAxis; 2] = [GamepadAxis::LeftStickX, GamepadAxis::LeftStickY];

/// Every pad's move stick read raw, from its first axis event (the router
/// runs this between Bevy's connection and event systems; a pad's settings
/// outlive a disconnect, as its entity does).
pub(crate) fn read_sticks_raw(mut pads: Query<&mut GamepadSettings, With<Gamepad>>) {
    let raw = raw_stick();
    for mut settings in &mut pads {
        for axis in MOVE_STICK {
            if settings.axis_settings.get(&axis) != Some(&raw) {
                settings.axis_settings.insert(axis, raw.clone());
            }
        }
    }
}

/// A face button where it sits on the pad, never by the letter printed on
/// it (bevy_gilrs names them the same way).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum Face {
    South,
    East,
    West,
    North,
}

impl Face {
    /// The order the hero's input holds them: bottom, right, left, top.
    pub(crate) const ALL: [Face; 4] = [Face::South, Face::East, Face::West, Face::North];

    pub(crate) fn button(self) -> GamepadButton {
        match self {
            Face::South => GamepadButton::South,
            Face::East => GamepadButton::East,
            Face::West => GamepadButton::West,
            Face::North => GamepadButton::North,
        }
    }

    pub(crate) fn name(self) -> &'static str {
        match self {
            Face::South => "south",
            Face::East => "east",
            Face::West => "west",
            Face::North => "north",
        }
    }
}

/// One pad's frame as the layer reads it.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct PadFrame<Id> {
    pub(crate) id: Id,
    /// The left stick, raw: x right, y forward, each −1 to 1.
    pub(crate) stick: Vec2,
    /// Buttons that went down this frame.
    pub(crate) pressed: Vec<GamepadButton>,
    /// The face buttons held, in [`Face::ALL`]'s order.
    pub(crate) held: [bool; 4],
}

impl PadFrame<Entity> {
    pub(crate) fn read(id: Entity, pad: &Gamepad) -> Self {
        Self {
            id,
            stick: pad.left_stick(),
            pressed: pad.get_just_pressed().copied().collect(),
            held: Face::ALL.map(|f| pad.pressed(f.button())),
        }
    }
}

impl<Id> PadFrame<Id> {
    fn pressed(&self, button: GamepadButton) -> bool {
        self.pressed.contains(&button)
    }
}

/// What the hero's step reads of the walking pad.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct HeroPad {
    pub(crate) stick: Stick,
    /// The face buttons held, in [`Face::ALL`]'s order.
    pub(crate) buttons: [bool; 4],
}

impl HeroPad {
    pub(crate) const STILL: HeroPad = HeroPad {
        stick: Stick::CENTRED,
        buttons: [false; 4],
    };
}

/// Why the level waits.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Pause<Id> {
    /// Start on `by`: the one-button overlay, its prompt as printed on `by`.
    Start { by: Id },
    /// The walking pad went away, or none was connected when the level
    /// started: the controller message.
    NoPad,
}

/// Who walks the hero and whether the level waits, from the level's start.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Seat<Id> {
    walking: Option<Id>,
    pause: Option<Pause<Id>>,
    begun: bool,
    /// The portal held the level the frame before ([`Self::hold`]).
    holding: bool,
    /// The walking pad's faces still down from a press a layer took (the
    /// resume, the take-over, an intro's skip): kept from the hero until
    /// each is let go.
    swallowed: [bool; 4],
    /// What the hero read the frame before.
    last: HeroPad,
}

impl<Id: Copy + Eq + std::fmt::Debug> Seat<Id> {
    /// The level starting: the first pad connected walks at its first step.
    pub(crate) fn new() -> Self {
        Self {
            walking: None,
            pause: None,
            begun: false,
            holding: false,
            swallowed: [false; 4],
            last: HeroPad::STILL,
        }
    }

    pub(crate) fn walking(&self) -> Option<Id> {
        self.walking
    }

    pub(crate) fn pause(&self) -> Option<Pause<Id>> {
        self.pause
    }

    /// What the hero read this frame.
    pub(crate) fn last(&self) -> HeroPad {
        self.last
    }

    /// A frame of the pads, connected ones only, in the order they
    /// connected; what the hero reads this frame. The frame the level comes
    /// to wait he reads what he read the frame before: the steps due this
    /// frame were due before the press, and from the next none run.
    pub(crate) fn step(&mut self, pads: &[PadFrame<Id>]) -> HeroPad {
        let was_running = self.begun && self.pause.is_none() && !self.holding;
        self.holding = false;
        if !self.begun {
            self.begun = true;
            match pads.first() {
                Some(first) => {
                    info!(pad = ?first.id, "the level starts with the first pad connected walking");
                    self.take_seat(first);
                }
                None => {
                    info!("the level starts with no pad: waiting for one");
                    self.pause = Some(Pause::NoPad);
                }
            }
        }
        if let Some(walking) = self.walking
            && !pads.iter().any(|p| p.id == walking)
        {
            info!(pad = ?walking, "the walking pad went away: waiting for another");
            self.walking = None;
            self.pause = Some(Pause::NoPad);
        }
        match self.pause {
            Some(Pause::NoPad) => {
                if let Some(pad) = pads.iter().find(|p| !p.pressed.is_empty()) {
                    info!(pad = ?pad.id, "a press takes the hero over");
                    self.take_seat(pad);
                }
            }
            Some(Pause::Start { .. }) => {
                let resume = pads
                    .iter()
                    .find(|p| p.pressed(GamepadButton::South) || p.pressed(GamepadButton::Start));
                if let Some(pad) = resume {
                    info!(pad = ?pad.id, "the level carries on");
                    self.pause = None;
                    if let Some(walking) = self.walking_frame(pads) {
                        self.swallowed = walking.held;
                    }
                }
            }
            None => {
                if let Some(pad) = pads.iter().find(|p| p.pressed(GamepadButton::Start)) {
                    info!(pad = ?pad.id, "Start pauses the level");
                    self.pause = Some(Pause::Start { by: pad.id });
                }
            }
        }
        let walking = self.walking_frame(pads).cloned();
        if let Some(walking) = &walking {
            for (swallowed, held) in self.swallowed.iter_mut().zip(walking.held) {
                *swallowed &= held;
            }
        }
        let out = match (self.pause, walking) {
            (Some(_), _) if was_running => self.last,
            (Some(_), _) | (None, None) => HeroPad::STILL,
            (None, Some(walking)) => HeroPad {
                stick: Stick::new(walking.stick.x, walking.stick.y),
                buttons: std::array::from_fn(|i| walking.held[i] && !self.swallowed[i]),
            },
        };
        self.last = out;
        out
    }

    /// A layer above took the frame (an intro's skip): the hero reads
    /// nothing, and the walking pad's faces held now stay his only once let
    /// go and pressed again.
    pub(crate) fn swallow(&mut self, pads: &[PadFrame<Id>]) {
        if let Some(walking) = self.walking_frame(pads) {
            self.swallowed = walking.held;
        }
        self.holding = false;
        self.last = HeroPad::STILL;
    }

    /// The portal holds the level (AG.29: an alert, or no hero in it): no
    /// pad walks the hero, pauses or takes him over, and the walking pad's
    /// faces held now stay his only once let go and pressed again. The
    /// frame the hold begins he reads what he read the frame before, as a
    /// pause's first frame does: the steps due then were due before it.
    pub(crate) fn hold(&mut self, pads: &[PadFrame<Id>]) -> HeroPad {
        let was_running = self.begun && self.pause.is_none() && !self.holding;
        self.holding = true;
        if let Some(walking) = self.walking_frame(pads) {
            self.swallowed = walking.held;
        }
        let out = if was_running {
            self.last
        } else {
            HeroPad::STILL
        };
        self.last = out;
        out
    }

    fn take_seat(&mut self, pad: &PadFrame<Id>) {
        self.walking = Some(pad.id);
        self.pause = None;
        self.swallowed = pad.held;
    }

    fn walking_frame<'a>(&self, pads: &'a [PadFrame<Id>]) -> Option<&'a PadFrame<Id>> {
        let walking = self.walking?;
        pads.iter().find(|p| p.id == walking)
    }
}

/// The pads in the order they connected, and while the level shows, its
/// [`Seat`].
#[derive(Resource, Debug, Default)]
pub(crate) struct LevelPads {
    order: Vec<Entity>,
    seat: Option<Seat<Entity>>,
    /// The portal holds the level ([`crate::play`]'s alerts, or no hero).
    held: bool,
}

impl LevelPads {
    /// The level takes the TV: this layer takes the pads, the first connected
    /// walking.
    pub(crate) fn show(&mut self) {
        if self.seat.is_none() {
            self.seat = Some(Seat::new());
        }
    }

    /// The level gave the TV back.
    pub(crate) fn hide(&mut self) {
        self.seat = None;
        self.held = false;
    }

    /// The portal holds the level, or lets it go: while held the router
    /// hands the seat [`Seat::hold`] and the level's clock stops.
    pub(crate) fn hold(&mut self, held: bool) {
        self.held = held && self.seat.is_some();
    }

    pub(crate) fn held(&self) -> bool {
        self.held
    }

    pub(crate) fn shows(&self) -> bool {
        self.seat.is_some()
    }

    pub(crate) fn seat(&self) -> Option<&Seat<Entity>> {
        self.seat.as_ref()
    }

    pub(crate) fn seat_mut(&mut self) -> Option<&mut Seat<Entity>> {
        self.seat.as_mut()
    }

    /// The level shows and waits for Start or a pad.
    pub(crate) fn paused(&self) -> bool {
        self.seat.as_ref().is_some_and(|s| s.pause.is_some())
    }

    /// The level shows and stands still, paused or held: its clock stops
    /// ([`crate::pause::hold_time`]).
    pub(crate) fn waiting(&self) -> bool {
        self.paused() || self.held
    }

    /// This frame's pads, the connected ones in the order they connected
    /// (several in one frame by entity).
    pub(crate) fn frames<'a>(
        &mut self,
        pads: impl Iterator<Item = (Entity, &'a Gamepad)>,
    ) -> Vec<PadFrame<Entity>> {
        let mut connected: Vec<(Entity, &Gamepad)> = pads.collect();
        connected.sort_by_key(|(e, _)| *e);
        self.order.retain(|e| connected.iter().any(|(c, _)| c == e));
        for (entity, _) in &connected {
            if !self.order.contains(entity) {
                self.order.push(*entity);
            }
        }
        self.order
            .iter()
            .filter_map(|e| connected.iter().find(|(c, _)| c == e))
            .map(|(entity, pad)| PadFrame::read(*entity, pad))
            .collect()
    }

    /// The connected pads in the order they connected, as of the last frame.
    pub(crate) fn order(&self) -> &[Entity] {
        &self.order
    }
}

/// The labels printed on a pad's face buttons.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Labels {
    /// A at the bottom, B on the right, X on the left, Y at the top.
    Xbox,
    /// B at the bottom, A on the right, Y on the left, X at the top.
    Nintendo,
    /// A pad the table doesn't place: the button's position, drawn.
    Position,
}

impl Labels {
    pub(crate) fn name(self) -> &'static str {
        match self {
            Labels::Xbox => "xbox",
            Labels::Nintendo => "nintendo",
            Labels::Position => "position",
        }
    }
}

/// A pad identified by the vendor and product ids gilrs reports for it, and
/// the labels printed on it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Printed {
    pub(crate) vendor: u16,
    pub(crate) product: u16,
    pub(crate) labels: Labels,
}

/// The id table, for pads gilrs reads (every platform but macOS). Empty: AG.5
/// found the 8BitDo Ultimate 2 unreadable through gilrs on the Mac (its
/// dongle and cable have no HID interface there, and over Bluetooth gilrs
/// reads nothing), so its rows come from gilrs on XInput at the Windows/HTPC
/// bring-up (openspec/backlog.md).
pub(crate) const PRINTED: &[Printed] = &[];

/// The labels on a pad with these ids, by `table`; [`Labels::Position`] for
/// one it doesn't hold or one without ids.
pub(crate) fn labels_in(table: &[Printed], vendor: Option<u16>, product: Option<u16>) -> Labels {
    let (Some(vendor), Some(product)) = (vendor, product) else {
        return Labels::Position;
    };
    table
        .iter()
        .find(|p| p.vendor == vendor && p.product == product)
        .map_or(Labels::Position, |p| p.labels)
}

/// The labels the letters a platform read off the pad name, bottom, right,
/// left and top: Xbox's or Nintendo's when all four are exactly one of
/// them, else the position (a PlayStation pad's shapes, a pad Apple names
/// no letters on, a mix).
pub(crate) fn labels_printed(faces: [Option<char>; 4]) -> Labels {
    match faces {
        [Some('A'), Some('B'), Some('X'), Some('Y')] => Labels::Xbox,
        [Some('B'), Some('A'), Some('Y'), Some('X')] => Labels::Nintendo,
        _ => Labels::Position,
    }
}

/// The labels on `pad`: by the letters read off it where its source gives
/// them (GameController on macOS), else by the id table.
pub(crate) fn labels(pad: &Gamepad, printed: Option<&FacesPrinted>) -> Labels {
    match printed {
        Some(FacesPrinted(faces)) => labels_printed(*faces),
        None => labels_in(PRINTED, pad.vendor_id(), pad.product_id()),
    }
}

/// What the TV names a button by: the letter printed on it, or its
/// position. Never a PlayStation symbol: there's no way to say one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Prompt {
    Letter(&'static str),
    Position(Face),
}

impl Prompt {
    /// `A`, or `position:south`, for the mirror.
    pub(crate) fn name(self) -> String {
        match self {
            Prompt::Letter(letter) => letter.to_owned(),
            Prompt::Position(face) => format!("position:{}", face.name()),
        }
    }
}

/// The letter `labels` print at `face`'s position, or the position itself
/// when the pad's labels aren't known.
pub(crate) fn prompt(labels: Labels, face: Face) -> Prompt {
    use Face::*;
    match (labels, face) {
        (Labels::Xbox, South) | (Labels::Nintendo, East) => Prompt::Letter("A"),
        (Labels::Xbox, East) | (Labels::Nintendo, South) => Prompt::Letter("B"),
        (Labels::Xbox, West) | (Labels::Nintendo, North) => Prompt::Letter("X"),
        (Labels::Xbox, North) | (Labels::Nintendo, West) => Prompt::Letter("Y"),
        (Labels::Position, face) => Prompt::Position(face),
    }
}

/// Pads driven in tests as gilrs would drive them: a connection, raw axis
/// and button changes, a disconnection, each through Bevy's own gamepad
/// systems and so through each pad's [`GamepadSettings`].
#[cfg(test)]
pub(crate) mod test_pad {
    use bevy::input::gamepad::{
        GamepadConnection, GamepadConnectionEvent, RawGamepadAxisChangedEvent,
        RawGamepadButtonChangedEvent, RawGamepadEvent,
    };

    use super::*;

    /// A pad connecting; it shows in the world after the next update.
    pub(crate) fn connect(app: &mut App, vendor: Option<u16>, product: Option<u16>) -> Entity {
        let pad = app.world_mut().spawn_empty().id();
        reconnect(app, pad, vendor, product);
        pad
    }

    pub(crate) fn reconnect(app: &mut App, pad: Entity, vendor: Option<u16>, product: Option<u16>) {
        let event = GamepadConnectionEvent::new(
            pad,
            GamepadConnection::Connected {
                name: "test pad".into(),
                vendor_id: vendor,
                product_id: product,
            },
        );
        app.world_mut()
            .write_message(RawGamepadEvent::from(event.clone()));
        app.world_mut().write_message(event);
    }

    pub(crate) fn disconnect(app: &mut App, pad: Entity) {
        let event = GamepadConnectionEvent::new(pad, GamepadConnection::Disconnected);
        app.world_mut()
            .write_message(RawGamepadEvent::from(event.clone()));
        app.world_mut().write_message(event);
    }

    /// The left stick at `x`, `y` as the pad sends it.
    pub(crate) fn stick(app: &mut App, pad: Entity, x: f32, y: f32) {
        for (axis, value) in [(GamepadAxis::LeftStickX, x), (GamepadAxis::LeftStickY, y)] {
            app.world_mut()
                .write_message(RawGamepadEvent::from(RawGamepadAxisChangedEvent::new(
                    pad, axis, value,
                )));
        }
    }

    pub(crate) fn button(app: &mut App, pad: Entity, button: GamepadButton, down: bool) {
        let value = if down { 1.0 } else { 0.0 };
        app.world_mut()
            .write_message(RawGamepadEvent::from(RawGamepadButtonChangedEvent::new(
                pad, button, value,
            )));
    }

    /// Down for one update, so the layer sees it go down and the press ends
    /// before the next.
    pub(crate) fn press(app: &mut App, pad: Entity, b: GamepadButton) {
        button(app, pad, b, true);
        app.update();
        button(app, pad, b, false);
    }
}

#[cfg(test)]
mod tests {
    use guac_motion::stick::{DEAD_ZONE, Push};

    use super::*;

    const A: u8 = 1;
    const B: u8 = 2;

    fn pad(id: u8) -> PadFrame<u8> {
        PadFrame {
            id,
            stick: Vec2::ZERO,
            pressed: Vec::new(),
            held: [false; 4],
        }
    }

    fn pushed(id: u8, x: f32, y: f32) -> PadFrame<u8> {
        PadFrame {
            stick: Vec2::new(x, y),
            ..pad(id)
        }
    }

    fn pressing(mut frame: PadFrame<u8>, button: GamepadButton) -> PadFrame<u8> {
        frame.pressed.push(button);
        if let Some(i) = Face::ALL.iter().position(|f| f.button() == button) {
            frame.held[i] = true;
        }
        frame
    }

    fn holding(mut frame: PadFrame<u8>, face: Face) -> PadFrame<u8> {
        frame.held[Face::ALL.iter().position(|f| *f == face).unwrap()] = true;
        frame
    }

    /// A seat whose level has started with `pads`, A walking.
    fn walking(pads: &[PadFrame<u8>]) -> Seat<u8> {
        let mut seat = Seat::new();
        seat.step(pads);
        seat
    }

    /// The push one pad's raw stick gives the hero, against a view along 15°.
    fn push_of(x: f32, y: f32) -> Push {
        let mut seat = Seat::new();
        seat.step(&[pushed(A, x, y)]).stick.push(15.0)
    }

    /// pad-input's dead zone, rescale and cap, through the layer: the raw
    /// value it hands the hero is the pad's, and his push is 0 up to a
    /// quarter of an axis, (|v| − 0.25) / 0.75 past it, 1 at full travel,
    /// on each axis and either way, and a full corner capped at 1.
    #[test]
    fn the_dead_zone_and_rescale_at_a_quarter_either_side_and_full() {
        for sign in [1.0f32, -1.0] {
            for axis in [Vec2::X, Vec2::Y] {
                for (raw, want) in [(0.24, 0.0), (0.25, 0.0), (0.26, 0.01 / 0.75), (1.0, 1.0)] {
                    let v = axis * raw * sign;
                    let mut seat = Seat::new();
                    let out = seat.step(&[pushed(A, v.x, v.y)]);
                    assert_eq!(out.stick, Stick::new(v.x, v.y), "the raw value, untouched");
                    let push = out.stick.push(15.0);
                    assert!(
                        (push.strength - want).abs() < 1e-6,
                        "{v}: {push:?}, want {want}"
                    );
                    assert_eq!(push.held(), want > 0.0, "{v}");
                }
            }
        }
        assert_eq!(DEAD_ZONE, 0.25);
        for corner in [Vec2::ONE, -Vec2::ONE, Vec2::new(1.0, -1.0)] {
            assert_eq!(push_of(corner.x, corner.y).strength, 1.0, "{corner}");
        }
        // A round gate's rim read as is walks its diagonal at 0.862 of top
        // speed (pad-input's Between the axes); the 8BitDo's square-ish one
        // is the test below.
        let rim = std::f32::consts::FRAC_1_SQRT_2;
        let diagonal = push_of(rim, rim).strength;
        assert!((diagonal - 0.8619).abs() < 1e-4, "{diagonal}");
    }

    /// AG.49's corner, against AG.5's gate (the 8BitDo over Bluetooth
    /// through GameController, 2026-10-20): each axis reads exactly 1 at
    /// full travel and the rim's diagonals 0.78-0.80 an axis, so a full
    /// diagonal's push caps at 1. The readings are AG.5's guided log's
    /// farthest rim reading in each diagonal 15° bin (centred on 45°, 135°,
    /// 225° and 315°), and the one corner window whose last reading sits in
    /// its corner (the other three, the presses lagging a window, end 18-24°
    /// off the diagonal with an axis at 1, which pushes 1 on any gate); each
    /// walks at top speed.
    #[test]
    fn the_8bitdos_measured_corners_walk_at_top_speed() {
        for (x, y) in [
            // The diagonal bins' farthest rim readings.
            (0.7538, 0.8366),
            (-0.7037, 0.8654),
            (-0.8708, -0.6805),
            (0.8813, -0.6854),
            // The down-left window's last reading, at 219.0°.
            (-0.8573, -0.6953),
        ] {
            assert_eq!(push_of(x, y).strength, 1.0, "({x}, {y})");
        }
        // The rim at 45° itself, about 0.80 an axis: full speed too.
        assert_eq!(push_of(0.80, 0.80).strength, 1.0);
        // Where it stops being full: a 45° push under 0.7803 an axis.
        let edge = 0.25 + 0.75 * std::f32::consts::FRAC_1_SQRT_2;
        assert!(push_of(edge + 1e-4, edge + 1e-4).strength == 1.0);
        assert!(push_of(edge - 1e-3, edge - 1e-3).strength < 1.0);
    }

    /// The stick against the view: forward along it, left at its +90°,
    /// right at −90° and back at 180°, for views along 0°, 15° and 195°;
    /// between the axes bent toward the nearer (pad-input's 49 and 118).
    #[test]
    fn the_stick_against_views_facing_0_15_and_195() {
        let along = |h: f32| h.rem_euclid(360.0);
        for view in [0.0f32, 15.0, 195.0] {
            for (x, y, off) in [
                (0.0, 1.0, 0.0),
                (-1.0, 0.0, 90.0),
                (1.0, 0.0, -90.0),
                (0.0, -1.0, 180.0),
            ] {
                let mut seat = Seat::new();
                let push = seat.step(&[pushed(A, x, y)]).stick.push(view);
                let miss =
                    (along(push.heading) - along(view + off) + 180.0).rem_euclid(360.0) - 180.0;
                assert!(miss.abs() < 1e-4, "view {view}, stick ({x}, {y}): {push:?}");
                assert_eq!(push.strength, 1.0);
            }
            let mut seat = Seat::new();
            let push = seat
                .step(&[pushed(A, 49.0 / 128.0, 118.0 / 128.0)])
                .stick
                .push(view);
            let miss =
                (along(push.heading) - along(view - 11.18) + 180.0).rem_euclid(360.0) - 180.0;
            assert!(miss.abs() < 0.01, "view {view}: {push:?}");
            assert!((push.strength - 0.9132).abs() < 1e-4, "{push:?}");
        }
    }

    /// Face buttons by position: the walking pad's bottom, right, left and
    /// top in that order, whatever its labels; another pad's reach no one.
    #[test]
    fn face_buttons_by_position() {
        let mut seat = walking(&[pad(A), pad(B)]);
        for (i, face) in Face::ALL.into_iter().enumerate() {
            let out = seat.step(&[holding(pad(A), face), pad(B)]);
            let mut want = [false; 4];
            want[i] = true;
            assert_eq!(out.buttons, want, "{face:?}");
        }
        let all = Face::ALL.into_iter().fold(pad(B), holding);
        assert_eq!(seat.step(&[pad(A), all]).buttons, [false; 4]);
        // The bottom one is index 0 on every pad: a Nintendo-labelled pad's
        // B and a PC pad's A are both South.
        assert_eq!(Face::ALL[0].button(), GamepadButton::South);
    }

    /// Another pad's stick and buttons move no one, but its Start pauses,
    /// and its bottom button resumes.
    #[test]
    fn a_second_pad_moves_no_one_but_pauses() {
        let mut seat = walking(&[pad(A), pad(B)]);
        assert_eq!(seat.walking(), Some(A));
        let out = seat.step(&[
            pushed(A, 0.0, 0.5),
            holding(pushed(B, 1.0, 1.0), Face::South),
        ]);
        assert_eq!(out.stick, Stick::new(0.0, 0.5));
        assert_eq!(out.buttons, [false; 4]);
        let walked = out;
        let out = seat.step(&[pushed(A, 0.0, 0.5), pressing(pad(B), GamepadButton::Start)]);
        assert_eq!(seat.pause(), Some(Pause::Start { by: B }));
        assert_eq!(out, walked, "the frame it pauses reads the frame before");
        assert_eq!(seat.step(&[pushed(A, 0.0, 1.0), pad(B)]), HeroPad::STILL);
        seat.step(&[pad(A), pressing(pad(B), GamepadButton::South)]);
        assert_eq!(seat.pause(), None);
        assert_eq!(
            seat.walking(),
            Some(A),
            "resuming doesn't hand the hero over"
        );
        assert_eq!(
            seat.step(&[pushed(A, 0.0, 1.0), pad(B)]).stick,
            Stick::new(0.0, 1.0)
        );
    }

    /// The walking pad lost: the level waits; another pad's stick changes
    /// nothing, its press takes the hero over, and the button it pressed
    /// stays off his input until let go.
    #[test]
    fn the_walking_pad_lost_and_another_taking_over() {
        let mut seat = walking(&[pushed(A, 0.0, 1.0), pad(B)]);
        let before = seat.step(&[pushed(A, 0.0, 1.0), pad(B)]);
        let out = seat.step(&[pushed(B, 1.0, 0.0)]);
        assert_eq!(seat.pause(), Some(Pause::NoPad));
        assert_eq!(seat.walking(), None);
        assert_eq!(out, before, "the frame it's lost reads the frame before");
        assert_eq!(seat.step(&[pushed(B, 1.0, 0.0)]), HeroPad::STILL);
        assert_eq!(seat.pause(), Some(Pause::NoPad), "a stick is no press");
        seat.step(&[pressing(pushed(B, 1.0, 0.0), GamepadButton::East)]);
        assert_eq!((seat.walking(), seat.pause()), (Some(B), None));
        let out = seat.step(&[holding(pushed(B, 1.0, 0.0), Face::East)]);
        assert_eq!(out.stick, Stick::new(1.0, 0.0));
        assert_eq!(
            out.buttons, [false; 4],
            "the take-over's press is swallowed"
        );
        assert_eq!(seat.step(&[pushed(B, 1.0, 0.0)]).buttons, [false; 4]);
        let out = seat.step(&[holding(pushed(B, 1.0, 0.0), Face::East)]);
        assert_eq!(out.buttons, [false, true, false, false], "pressed again");
        // A coming back changes nothing: B walks, second in the order now.
        let out = seat.step(&[
            pressing(pushed(A, 0.0, 1.0), GamepadButton::South),
            pushed(B, 1.0, 0.0),
        ]);
        assert_eq!(seat.walking(), Some(B));
        assert_eq!(out.stick, Stick::new(1.0, 0.0), "B's stick, not A's");
    }

    /// No pad when the level starts: it waits for a press, not a connection.
    #[test]
    fn no_pad() {
        let mut seat = Seat::<u8>::new();
        assert_eq!(seat.step(&[]), HeroPad::STILL);
        assert_eq!(seat.pause(), Some(Pause::NoPad));
        seat.step(&[pushed(A, 0.0, 1.0)]);
        assert_eq!((seat.walking(), seat.pause()), (None, Some(Pause::NoPad)));
        seat.step(&[pressing(pad(A), GamepadButton::Start)]);
        assert_eq!((seat.walking(), seat.pause()), (Some(A), None));
        // The press that took over doesn't also pause.
        assert_eq!(
            seat.step(&[pushed(A, 0.0, 1.0)]).stick,
            Stick::new(0.0, 1.0)
        );
    }

    /// Start pauses and the frame it does reads the frame before; while
    /// paused the hero reads nothing and only the bottom button or Start, on
    /// any pad, resumes; the resume's press stays off his buttons.
    #[test]
    fn pause_and_resume() {
        let mut seat = walking(&[pushed(A, 0.0, 1.0), pad(B)]);
        let before = seat.step(&[pushed(A, 0.0, 1.0), pad(B)]);
        let out = seat.step(&[pressing(pushed(A, 0.0, 1.0), GamepadButton::Start), pad(B)]);
        assert_eq!(seat.pause(), Some(Pause::Start { by: A }));
        assert_eq!(out, before);
        for other in [
            GamepadButton::East,
            GamepadButton::West,
            GamepadButton::North,
            GamepadButton::Select,
            GamepadButton::Mode,
            GamepadButton::DPadUp,
        ] {
            let out = seat.step(&[pressing(pushed(A, 0.0, 1.0), other), pad(B)]);
            assert_eq!(out, HeroPad::STILL, "{other:?}");
            assert!(seat.pause().is_some(), "{other:?} doesn't resume");
        }
        seat.step(&[pressing(pushed(A, 0.0, 1.0), GamepadButton::South), pad(B)]);
        assert_eq!(seat.pause(), None);
        let out = seat.step(&[holding(pushed(A, 0.0, 1.0), Face::South), pad(B)]);
        assert_eq!(out.stick, Stick::new(0.0, 1.0), "carries on");
        assert_eq!(out.buttons, [false; 4], "the resume's press is swallowed");
        // Start on the other pad pauses; Start on the other resumes.
        seat.step(&[pad(A), pressing(pad(B), GamepadButton::Start)]);
        assert_eq!(seat.pause(), Some(Pause::Start { by: B }));
        seat.step(&[pressing(pad(A), GamepadButton::Start), pad(B)]);
        assert_eq!(seat.pause(), None);
    }

    /// Select and Home do nothing while he walks.
    #[test]
    fn select_and_home_do_nothing() {
        let mut seat = walking(&[pushed(A, 0.3, 0.9)]);
        let before = seat.step(&[pushed(A, 0.3, 0.9)]);
        for button in [GamepadButton::Select, GamepadButton::Mode] {
            let out = seat.step(&[pressing(pushed(A, 0.3, 0.9), button)]);
            assert_eq!((out, seat.pause()), (before, None), "{button:?}");
        }
    }

    /// A press taken by a layer above stays off the hero until let go.
    #[test]
    fn a_swallowed_press_stays_off_until_let_go() {
        let mut seat = walking(&[pad(A)]);
        seat.swallow(&[holding(pad(A), Face::South)]);
        assert_eq!(
            seat.step(&[holding(pad(A), Face::South)]).buttons,
            [false; 4]
        );
        seat.step(&[pad(A)]);
        assert_eq!(
            seat.step(&[holding(pad(A), Face::South)]).buttons,
            [true, false, false, false]
        );
    }

    /// The resolver: a pad in the table by its two ids gets its labels, and
    /// a pad it doesn't place, or one without ids, the position; the
    /// letters by position on each label set; nothing else to say.
    #[test]
    fn the_resolver_table_and_its_fallback() {
        let table = [
            Printed {
                vendor: 0x2dc8,
                product: 0x0001,
                labels: Labels::Xbox,
            },
            Printed {
                vendor: 0x2dc8,
                product: 0x0002,
                labels: Labels::Nintendo,
            },
        ];
        let at = |v, p| labels_in(&table, v, p);
        assert_eq!(at(Some(0x2dc8), Some(0x0001)), Labels::Xbox);
        assert_eq!(at(Some(0x2dc8), Some(0x0002)), Labels::Nintendo);
        assert_eq!(at(Some(0x2dc8), Some(0x0003)), Labels::Position);
        assert_eq!(
            at(Some(0x0001), Some(0x0001)),
            Labels::Position,
            "product alone"
        );
        assert_eq!(at(None, Some(0x0001)), Labels::Position);
        assert_eq!(at(Some(0x2dc8), None), Labels::Position);
        // A DualShock 3 isn't in it: its position, never its symbols.
        assert_eq!(at(Some(0x054c), Some(0x0268)), Labels::Position);

        let letters = |labels| Face::ALL.map(|f| prompt(labels, f));
        use Prompt::Letter;
        assert_eq!(
            letters(Labels::Xbox),
            [Letter("A"), Letter("B"), Letter("X"), Letter("Y")]
        );
        assert_eq!(
            letters(Labels::Nintendo),
            [Letter("B"), Letter("A"), Letter("Y"), Letter("X")]
        );
        assert_eq!(letters(Labels::Position), Face::ALL.map(Prompt::Position));
        assert_eq!(prompt(Labels::Xbox, Face::South).name(), "A");
        assert_eq!(
            prompt(Labels::Position, Face::South).name(),
            "position:south"
        );

        // The shipped table holds nothing until AG.5: every pad its position.
        assert!(PRINTED.is_empty());
        assert_eq!(
            labels_in(PRINTED, Some(0x2dc8), Some(0x0001)),
            Labels::Position
        );
    }

    /// Bevy's own systems keep the move stick raw from a pad's first axis
    /// event, on the router's schedule: a first value inside Bevy's 0.05 dead
    /// zone lands, as does a change of 0.004 (its 0.01 threshold drops
    /// both); the other stick keeps Bevy's defaults.
    #[test]
    fn the_move_stick_reads_raw() {
        let mut app = App::new();
        app.add_plugins((
            MinimalPlugins,
            bevy::input::InputPlugin,
            crate::input::InputRouter,
        ));
        let id = test_pad::connect(&mut app, None, None);
        test_pad::stick(&mut app, id, 0.004, 0.0);
        app.update();
        let stick = |app: &App| app.world().get::<Gamepad>(id).unwrap().left_stick();
        assert_eq!(stick(&app), Vec2::new(0.004, 0.0), "the first event");
        let settings = app.world().get::<GamepadSettings>(id).unwrap();
        for axis in MOVE_STICK {
            let raw = settings.get_axis_settings(axis);
            assert_eq!(
                (
                    raw.livezone_lowerbound(),
                    raw.deadzone_lowerbound(),
                    raw.deadzone_upperbound(),
                    raw.livezone_upperbound(),
                    raw.threshold()
                ),
                (-1.0, 0.0, 0.0, 1.0, 0.0),
                "{axis:?}"
            );
        }
        assert_eq!(
            settings.get_axis_settings(GamepadAxis::RightStickX),
            &AxisSettings::default()
        );
        for (x, y) in [
            (0.3, 0.9),
            (0.304, 0.9),
            (0.304, 0.904),
            (0.02, 0.0),
            (0.03, 0.0),
        ] {
            test_pad::stick(&mut app, id, x, y);
            app.update();
            assert_eq!(stick(&app), Vec2::new(x, y));
        }
    }

    /// "The first pad connected" by when it connected, through the router:
    /// the pad whose entity sorts first goes away and comes back (bevy_gilrs
    /// keeps its entity), so it's behind the one that stayed, which walks.
    #[test]
    fn the_pad_connected_longest_walks_whatever_its_entity() {
        let mut app = App::new();
        app.add_plugins((
            MinimalPlugins,
            bevy::input::InputPlugin,
            crate::input::InputRouter,
        ));
        let one = test_pad::connect(&mut app, None, None);
        app.update();
        let two = test_pad::connect(&mut app, None, None);
        app.update();
        let (low, high) = (one.min(two), one.max(two));
        test_pad::disconnect(&mut app, low);
        app.update();
        assert_eq!(app.world().resource::<LevelPads>().order(), [high]);
        test_pad::reconnect(&mut app, low, None, None);
        app.update();
        app.update();
        assert_eq!(app.world().resource::<LevelPads>().order(), [high, low]);
        app.world_mut().resource_mut::<LevelPads>().show();
        app.update();
        let pads = app.world().resource::<LevelPads>();
        assert_eq!(pads.seat().and_then(Seat::walking), Some(high));
    }

    /// The labels from the letters a platform read off the pad: Xbox's or
    /// Nintendo's only when all four are exactly one of them; and they win
    /// over the id table, which stays the source without them.
    #[test]
    fn labels_from_the_letters_read_off_the_pad() {
        let at = |s: &str| {
            let mut c = s.chars().map(|c| (c != '-').then_some(c));
            [(); 4].map(|_| c.next().flatten())
        };
        assert_eq!(labels_printed(at("ABXY")), Labels::Xbox);
        assert_eq!(labels_printed(at("BAYX")), Labels::Nintendo);
        for other in ["----", "ABYX", "BAXY", "AB-Y", "XYAB"] {
            assert_eq!(labels_printed(at(other)), Labels::Position, "{other}");
        }
        let mut world = World::new();
        let pad = world.spawn(Gamepad::default()).id();
        let gamepad = world.get::<Gamepad>(pad).unwrap();
        assert_eq!(labels(gamepad, None), Labels::Position, "no ids");
        assert_eq!(
            labels(gamepad, Some(&FacesPrinted(at("BAYX")))),
            Labels::Nintendo
        );
        assert_eq!(
            prompt(
                labels(gamepad, Some(&FacesPrinted(at("BAYX")))),
                Face::South
            ),
            Prompt::Letter("B")
        );
    }

    /// GameController's pads (AG.53) through the router, on a fake of the
    /// framework: the first connected walks; its letters name the overlay's
    /// prompt (a Switch Pro's bottom button is B); and a press and release
    /// of its bottom button inside one frame resumes the level as a press.
    mod game_controller {
        use std::cell::RefCell;
        use std::collections::VecDeque;
        use std::rc::Rc;

        use guacamole::game_controller::{Controllers, Element, Glyph, Opened, PadState, install};

        use super::super::*;
        use crate::pause;

        /// Pads by key, each with its category and queued readings.
        #[derive(Clone, Default)]
        struct Fake(Rc<RefCell<Vec<(u64, &'static str, VecDeque<PadState>)>>>);

        impl Controllers for Fake {
            fn connected(&mut self) -> Vec<u64> {
                self.0.borrow().iter().map(|p| p.0).collect()
            }
            fn open(&mut self, key: u64) -> Option<Opened> {
                let category = self.0.borrow().iter().find(|p| p.0 == key)?.1;
                Some(Opened {
                    name: format!("pad {key}"),
                    category: category.into(),
                    glyphs: ["a", "b", "x", "y"].map(|l| Glyph {
                        symbol: Some(format!("{l}.circle")),
                        localized: None,
                    }),
                    state: PadState::default(),
                })
            }
            fn queue(&mut self, _: u64, _: usize) -> bool {
                true
            }
            fn ask_home_gesture_off(&mut self, _: u64) -> Option<bool> {
                Some(false)
            }
            fn next_state(&mut self, key: u64) -> Option<PadState> {
                let mut pads = self.0.borrow_mut();
                pads.iter_mut().find(|p| p.0 == key)?.2.pop_front()
            }
            fn close(&mut self, _: u64) {}
        }

        impl Fake {
            fn connect(&self, key: u64, category: &'static str) {
                self.0.borrow_mut().push((key, category, VecDeque::new()));
            }
            fn send(&self, key: u64, state: PadState) {
                let mut pads = self.0.borrow_mut();
                pads.iter_mut()
                    .find(|p| p.0 == key)
                    .unwrap()
                    .2
                    .push_back(state);
            }
        }

        fn app(fake: &Fake) -> App {
            let mut app = App::new();
            app.add_plugins((
                MinimalPlugins,
                bevy::input::InputPlugin,
                crate::input::InputRouter,
            ));
            install(&mut app, Box::new(fake.clone()));
            app
        }

        fn entity_of(app: &mut App, name: &str) -> Entity {
            app.world_mut()
                .query::<(Entity, &Name)>()
                .iter(app.world())
                .find(|(_, n)| n.as_str() == name)
                .map(|(e, _)| e)
                .unwrap()
        }

        #[test]
        fn game_controller_pads_walk_pause_and_resume_through_the_router() {
            let fake = Fake::default();
            let mut app = app(&fake);
            fake.connect(1, "Switch Pro Controller");
            app.update();
            fake.connect(2, "Xbox One");
            app.update();
            let (switch, xbox) = (entity_of(&mut app, "pad 1"), entity_of(&mut app, "pad 2"));
            assert_eq!(app.world().resource::<LevelPads>().order(), [switch, xbox]);
            app.world_mut().resource_mut::<LevelPads>().show();
            app.update();
            let walking = |app: &App| {
                let pads = app.world().resource::<LevelPads>();
                pads.seat().and_then(Seat::walking)
            };
            assert_eq!(walking(&app), Some(switch), "the first connected walks");

            // The stick lands raw, as Apple sends it.
            fake.send(
                1,
                PadState {
                    left: Vec2::new(0.004, 0.8027),
                    ..default()
                },
            );
            app.update();
            let seat = |app: &App| app.world().resource::<LevelPads>().seat().unwrap().clone();
            assert_eq!(seat(&app).last().stick, Stick::new(0.004, 0.8027));

            // Start (Apple's Menu) on the Xbox pad pauses; the prompt is the
            // bottom button as printed on it.
            fake.send(2, PadState::default().with(Element::Menu, true));
            fake.send(2, PadState::default());
            app.update();
            assert_eq!(seat(&app).pause(), Some(Pause::Start { by: xbox }));
            let labels_of = |app: &mut App| {
                let mut q = app
                    .world_mut()
                    .query::<(Entity, &Gamepad, Option<&FacesPrinted>)>();
                let found: Vec<(Entity, Labels)> = q
                    .iter(app.world())
                    .map(|(e, g, p)| (e, labels(g, p)))
                    .collect();
                move |pad: Entity| found.iter().find(|(e, _)| *e == pad).unwrap().1
            };
            let of = labels_of(&mut app);
            let level = app.world().resource::<LevelPads>();
            assert_eq!(
                pause::wanted(level, &of),
                Some(pause::LevelOverlay::Paused(Prompt::Letter("A")))
            );
            assert_eq!(of(switch), Labels::Nintendo);

            // A tap of the Switch Pro's bottom button (Apple's B) inside one
            // frame resumes.
            fake.send(1, PadState::default().with(Element::B, true));
            fake.send(1, PadState::default());
            app.update();
            assert_eq!(seat(&app).pause(), None, "the sub-frame tap resumed");

            // Paused from the Switch Pro: B.
            fake.send(1, PadState::default().with(Element::Menu, true));
            app.update();
            let of = labels_of(&mut app);
            let level = app.world().resource::<LevelPads>();
            assert_eq!(
                pause::wanted(level, &of),
                Some(pause::LevelOverlay::Paused(Prompt::Letter("B")))
            );
        }

        /// The letters reach what the TV draws and what `/api/_test/tv`
        /// reads, not only `pause::wanted`: on the TV's own app with the
        /// mirror, a Switch Pro over GameController lists as Nintendo and
        /// Start's overlay as drawn names B.
        #[test]
        fn the_letters_reach_the_drawn_overlay_and_the_mirror() {
            use guac_server::config::WindowMode;

            use crate::cards::PortalCards;
            use crate::mirror::{InputShot, Mirror, TvMirror};
            use crate::tv::layout_tests::portal_app;

            let fake = Fake::default();
            let mut app = portal_app(1920, 1080, WindowMode::Tv, PortalCards::default());
            install(&mut app, Box::new(fake.clone()));
            let mirror = TvMirror::default();
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
            fake.connect(1, "Switch Pro Controller");
            let shot = until(&mut app, "connected", &|s| !s.pads.is_empty());
            assert_eq!(
                (shot.pads[0].name.as_str(), shot.pads[0].labels.as_str()),
                ("pad 1", "nintendo")
            );
            app.world_mut().resource_mut::<LevelPads>().show();
            until(&mut app, "walking", &|s| s.pads[0].walking);
            fake.send(1, PadState::default().with(Element::Menu, true));
            fake.send(1, PadState::default());
            let shot = until(&mut app, "paused", &|s| s.overlay.is_some());
            let overlay = shot.overlay.unwrap();
            assert_eq!(overlay.kind, "paused");
            assert_eq!(overlay.prompt.as_deref(), Some("B"));
        }
    }
}
