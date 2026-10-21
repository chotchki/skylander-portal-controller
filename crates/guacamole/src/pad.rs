//! The level's input layer (openspec AG.20, design decision 12; pad-input).
//! While the level shows, AP's router ([`crate::input`]) hands this layer the
//! pads, under an intro's skip and over the screens:
//!
//! - The move stick reads raw: Bevy's dead zone and change threshold are off
//!   for both left-stick axes on every pad ([`raw_stick`], set by
//!   [`read_sticks_raw`]), so the value a pad stores is the last one gilrs
//!   sent (bevy_gilrs builds gilrs without its own filters). The original's
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
//!   [`prompt`]): Xbox letters on a pad the table knows as a PC layout,
//!   Nintendo's on a Switch layout, the button's position for any other. The
//!   table is empty until AG.5 reads the 8BitDo ids, so every pad gets the
//!   position glyph meanwhile, which names no label it can't back (design
//!   Open question 5's (b)).
//!
//! Hypothesis, until AG.5 measures the 8BitDo's gate: a full push into a
//! corner reads as the pad sends it, the gate not squared. On a round gate
//! the rim's diagonal reads about 0.707 an axis, which the dead zone turns
//! into 0.862 of top speed; pad-input's Between the axes is that rim read as
//! is (49 and 118 of 128 at 22.55° off forward).

use bevy::input::gamepad::{AxisSettings, Gamepad, GamepadAxis, GamepadButton, GamepadSettings};
use bevy::prelude::*;
use guac_motion::stick::Stick;
use tracing::info;

/// Both left-stick axes as gilrs sends them: the live zone the whole travel,
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
        let was_running = self.begun && self.pause.is_none();
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
        self.last = HeroPad::STILL;
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

    /// The level shows and waits: its clock stops ([`crate::pause`]).
    pub(crate) fn paused(&self) -> bool {
        self.seat.as_ref().is_some_and(|s| s.pause.is_some())
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
    #[cfg_attr(
        not(test),
        expect(dead_code, reason = "the first pads join the table with AG.5 (AG.49)")
    )]
    Xbox,
    /// B at the bottom, A on the right, Y on the left, X at the top.
    #[cfg_attr(
        not(test),
        expect(dead_code, reason = "the first pads join the table with AG.5 (AG.49)")
    )]
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

/// A pad AG.5 identified: the vendor and product ids gilrs reports for it on
/// the Mac, and the labels printed on it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Printed {
    pub(crate) vendor: u16,
    pub(crate) product: u16,
    pub(crate) labels: Labels,
}

/// AG.5's table. Empty until its probe reads the 8BitDo Ultimate 2's ids,
/// wired and wireless, in both variants (AG.49).
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

/// The labels on `pad`, by AG.5's table.
pub(crate) fn labels(pad: &Gamepad) -> Labels {
    labels_in(PRINTED, pad.vendor_id(), pad.product_id())
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
        // The hypothesis: a round gate's rim read as is, its diagonal at
        // 0.862 of top speed.
        let rim = std::f32::consts::FRAC_1_SQRT_2;
        let diagonal = push_of(rim, rim).strength;
        assert!((diagonal - 0.8619).abs() < 1e-4, "{diagonal}");
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
}
