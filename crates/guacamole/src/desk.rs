//! An unattended test run's TV keeps off the desk (openspec AS.740): a
//! `test-hooks` build with `GUAC_TEST_POSTED_PADS_ONLY` set is a run nobody
//! sits at (the e2e, a lane's or a review's TV driver, a landing's token
//! check) on a Mac someone may be using. Its window opens hidden and never
//! active (`main.rs`'s `window`), [`DeskDeaf`] takes it out of the Dock and
//! the app switcher, and the desk's keyboard, mouse, touch and gestures are
//! dropped at the frame's start, so the posted pads, the posted keys
//! (`flags`' capture key, written in `PreUpdate`) and the test routes are the
//! only way in. A build without `test-hooks`, or one without the switch,
//! never adds it.

use bevy::ecs::message::{MessageUpdateSystems, Messages};
use bevy::input::gestures::{DoubleTapGesture, PanGesture, PinchGesture, RotationGesture};
use bevy::input::keyboard::KeyboardInput;
use bevy::input::mouse::{MouseButtonInput, MouseMotion, MouseWheel};
use bevy::input::touch::TouchInput;
use bevy::picking::PickingSystems;
use bevy::picking::input::PointerInputSettings;
use bevy::prelude::*;
use bevy::window::{CursorEntered, CursorLeft, CursorMoved, Ime};

/// The TV deaf to the desk: picking's mouse and touch pointers off, and every
/// keyboard, mouse, touch, gesture and IME message winit wrote for the frame
/// dropped in `First`, before picking and `InputSystems` read them.
pub(crate) struct DeskDeaf;

impl Plugin for DeskDeaf {
    fn build(&self, app: &mut App) {
        app.insert_resource(PointerInputSettings {
            is_touch_enabled: false,
            is_mouse_enabled: false,
        })
        .add_systems(
            First,
            drop_the_desks_input
                .after(MessageUpdateSystems)
                .before(PickingSystems::Input),
        );
        #[cfg(target_os = "macos")]
        app.add_systems(Startup, off_the_dock);
    }
}

/// Clears one message type's queue, if the app has it.
fn clear<M: Message>(world: &mut World) {
    if let Some(mut messages) = world.get_resource_mut::<Messages<M>>() {
        messages.clear();
    }
}

/// Every message the desk's hands reach the TV through, gone.
fn drop_the_desks_input(world: &mut World) {
    clear::<KeyboardInput>(world);
    clear::<Ime>(world);
    clear::<MouseButtonInput>(world);
    clear::<MouseMotion>(world);
    clear::<MouseWheel>(world);
    clear::<CursorMoved>(world);
    clear::<CursorEntered>(world);
    clear::<CursorLeft>(world);
    clear::<TouchInput>(world);
    clear::<PinchGesture>(world);
    clear::<RotationGesture>(world);
    clear::<DoubleTapGesture>(world);
    clear::<PanGesture>(world);
}

/// The accessory activation policy: no Dock tile, not in Cmd-Tab, so nobody
/// switches to the test's app by accident. winit sets the regular policy as
/// its loop launches (an unbundled binary), so this runs after, at Startup,
/// on the main thread AppKit needs.
#[cfg(target_os = "macos")]
fn off_the_dock(_main_thread: bevy::ecs::system::NonSendMarker) {
    use objc2::MainThreadMarker;
    use objc2_app_kit::{NSApplication, NSApplicationActivationPolicy};
    let Some(mtm) = MainThreadMarker::new() else {
        warn!("off the Dock: not on the main thread, the app keeps its Dock tile");
        return;
    };
    let app = NSApplication::sharedApplication(mtm);
    if !app.setActivationPolicy(NSApplicationActivationPolicy::Accessory) {
        warn!("off the Dock: AppKit refused the accessory policy");
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use bevy::input::ButtonState;
    use bevy::input::InputSystems;
    use bevy::input::gamepad::GamepadButton;
    use bevy::input::keyboard::Key;
    use bevy::input::touch::TouchPhase;
    use bevy::window::PrimaryWindow;

    use super::*;
    use crate::screen::{SetupView, TvScreen};
    use crate::test_support::tv::{click_beside, exited, focus, key, pad, tap, tv_on};

    /// One of each message the desk writes, then a frame: with [`DeskDeaf`]
    /// none is left for a reader; without it every one is (a message lasts
    /// two frames), so the check reads something.
    #[test]
    fn every_message_of_the_desk_is_gone_before_anything_reads_it() {
        for deaf in [false, true] {
            let mut app = App::new();
            app.add_plugins((MinimalPlugins, bevy::input::InputPlugin))
                .add_message::<Ime>()
                .add_message::<CursorMoved>()
                .add_message::<CursorEntered>()
                .add_message::<CursorLeft>();
            if deaf {
                app.add_plugins(DeskDeaf);
            }
            let window = Entity::PLACEHOLDER;
            let world = app.world_mut();
            world.write_message(KeyboardInput {
                key_code: KeyCode::Escape,
                logical_key: Key::Escape,
                state: ButtonState::Pressed,
                text: None,
                repeat: false,
                window,
            });
            world.write_message(Ime::Disabled { window });
            world.write_message(MouseButtonInput {
                button: MouseButton::Left,
                state: ButtonState::Pressed,
                window,
            });
            world.write_message(MouseMotion { delta: Vec2::X });
            world.write_message(MouseWheel {
                unit: bevy::input::mouse::MouseScrollUnit::Line,
                x: 0.0,
                y: 1.0,
                window,
                phase: bevy::input::touch::TouchPhase::Moved,
            });
            world.write_message(CursorMoved {
                window,
                position: Vec2::ONE,
                delta: None,
            });
            world.write_message(CursorEntered { window });
            world.write_message(CursorLeft { window });
            world.write_message(TouchInput {
                phase: TouchPhase::Started,
                position: Vec2::ONE,
                window,
                force: None,
                id: 0,
            });
            world.write_message(PinchGesture(1.0));
            world.write_message(RotationGesture(1.0));
            world.write_message(DoubleTapGesture);
            world.write_message(PanGesture(Vec2::X));
            app.update();
            let world = app.world();
            let left = [
                ("key", world.resource::<Messages<KeyboardInput>>().len()),
                ("ime", world.resource::<Messages<Ime>>().len()),
                (
                    "button",
                    world.resource::<Messages<MouseButtonInput>>().len(),
                ),
                ("motion", world.resource::<Messages<MouseMotion>>().len()),
                ("wheel", world.resource::<Messages<MouseWheel>>().len()),
                ("moved", world.resource::<Messages<CursorMoved>>().len()),
                ("entered", world.resource::<Messages<CursorEntered>>().len()),
                ("left", world.resource::<Messages<CursorLeft>>().len()),
                ("touch", world.resource::<Messages<TouchInput>>().len()),
                ("pinch", world.resource::<Messages<PinchGesture>>().len()),
                (
                    "rotation",
                    world.resource::<Messages<RotationGesture>>().len(),
                ),
                (
                    "double tap",
                    world.resource::<Messages<DoubleTapGesture>>().len(),
                ),
                ("pan", world.resource::<Messages<PanGesture>>().len()),
            ];
            for (what, n) in left {
                assert_eq!(n, if deaf { 0 } else { 1 }, "{what}, deaf {deaf}");
            }
            assert!(
                !world
                    .resource::<ButtonInput<KeyCode>>()
                    .pressed(KeyCode::Escape)
                    || !deaf
            );
            assert!(
                !world
                    .resource::<ButtonInput<MouseButton>>()
                    .pressed(MouseButton::Left)
                    || !deaf
            );
        }
    }

    /// Picking's mouse and touch pointers are off, so a hand at the desk
    /// hovers and clicks nothing.
    #[test]
    fn picking_takes_no_mouse_and_no_touch() {
        let mut app = App::new();
        app.add_plugins((MinimalPlugins, DeskDeaf));
        let settings = app.world().resource::<PointerInputSettings>();
        assert!(!settings.is_mouse_enabled);
        assert!(!settings.is_touch_enabled);
    }

    /// A key written in `PreUpdate` before `InputSystems`, as `flags`' posted
    /// capture key is, still reaches the keys: only what winit wrote before
    /// the frame goes.
    #[test]
    fn a_posted_key_still_reaches_the_keys() {
        fn post(mut keys: MessageWriter<KeyboardInput>, mut once: Local<bool>) {
            if !std::mem::replace(&mut *once, true) {
                keys.write(KeyboardInput {
                    key_code: KeyCode::KeyF,
                    logical_key: Key::Character("f".into()),
                    state: ButtonState::Pressed,
                    text: None,
                    repeat: false,
                    window: Entity::PLACEHOLDER,
                });
            }
        }
        let mut app = App::new();
        app.add_plugins((MinimalPlugins, bevy::input::InputPlugin, DeskDeaf))
            .add_systems(PreUpdate, post.before(InputSystems));
        app.update();
        assert!(
            app.world()
                .resource::<ButtonInput<KeyCode>>()
                .just_pressed(KeyCode::KeyF)
        );
    }

    /// On the server error screen Esc, Enter on its focused Close and a click
    /// anywhere each close the TV (`tv/choice_tests.rs`); with [`DeskDeaf`]
    /// none moves anything, while a posted pad's East still closes it.
    #[test]
    fn a_key_and_a_click_at_the_desk_move_nothing_in_the_router() {
        type Press = fn(&mut App);
        let presses: [(&str, Press); 4] = [
            ("esc", |app| key(app, KeyCode::Escape, Key::Escape)),
            ("enter", |app| key(app, KeyCode::Enter, Key::Enter)),
            ("numpad enter", |app| {
                key(app, KeyCode::NumpadEnter, Key::Enter)
            }),
            ("a click anywhere", click_beside),
        ];
        let screen = TvScreen::ServerError {
            message: "open profile store".into(),
        };
        for (what, press) in presses {
            for deaf in [false, true] {
                let mut app = tv_on(screen.clone(), Duration::ZERO, SetupView::default());
                // Enter reaches the focused button through the widgets'
                // dispatch, which needs the one primary window.
                app.add_plugins(bevy::input_focus::InputDispatchPlugin);
                app.world_mut().spawn((Window::default(), PrimaryWindow));
                if deaf {
                    app.add_plugins(DeskDeaf);
                }
                app.update();
                let before = focus(&mut app);
                assert!(before.is_some(), "{what}: the close button has the focus");
                press(&mut app);
                app.update();
                assert_eq!(exited(&app), !deaf, "{what}, deaf {deaf}");
                if deaf {
                    assert_eq!(focus(&mut app), before, "{what}");
                    let pad = pad(&mut app);
                    tap(&mut app, pad, GamepadButton::East);
                    assert!(exited(&app), "{what}: a posted pad's East still closes it");
                }
            }
        }
    }
}
