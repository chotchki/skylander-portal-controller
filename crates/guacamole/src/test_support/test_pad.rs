//! Pads driven in tests as gilrs would drive them: a connection, raw axis
//! and button changes, a disconnection, each through Bevy's own gamepad
//! systems and so through each pad's [`GamepadSettings`].

use bevy::input::gamepad::{
    GamepadAxis, GamepadButton, GamepadConnection, GamepadConnectionEvent,
    RawGamepadAxisChangedEvent, RawGamepadButtonChangedEvent, RawGamepadEvent,
};
use bevy::prelude::*;

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
