use bevy::input::ButtonState;
use bevy::input::gamepad::GamepadButton;
use bevy::input::keyboard::{Key, KeyboardInput};
use bevy::input_focus::InputFocus;
use bevy::ui::Pressed;

use super::*;
use crate::input::FocusId;
use crate::picker::Picking;
use crate::test_support::tv::{
    button, choice, click_beside, click_on, exited, focus, frames, key, pad, portal_app, tap, tv_on,
};

fn found() -> SetupView {
    SetupView {
        giants_found: true,
        show_found: true,
        found_titles: vec!["Skylanders: Giants (BLUS30968)".into()],
        ..Default::default()
    }
}

fn state(app: &App) -> TvState {
    *app.world().resource::<State<TvState>>().get()
}

/// The picker was asked for (`should_open` said open) since the last call.
fn wanted(app: &mut App) -> bool {
    app.world_mut().resource_mut::<Picking>().take_wanted()
}

#[test]
fn south_on_found_lands_on_join() {
    let mut app = tv_on(TvScreen::Found, Duration::ZERO, found());
    assert_eq!(focus(&mut app), Some(CONTINUE));
    let pad = pad(&mut app);
    tap(&mut app, pad, GamepadButton::South);
    frames(&mut app, 2);
    assert_eq!(app.world().resource::<Current>().screen, TvScreen::Join);
    assert_eq!(state(&app), TvState::Join);
}

#[test]
fn a_click_beside_continue_leaves_found_up() {
    let mut app = tv_on(TvScreen::Found, Duration::ZERO, found());
    click_beside(&mut app);
    frames(&mut app, 3);
    assert_eq!(app.world().resource::<Current>().screen, TvScreen::Found);
    assert!(app.world().resource::<Setup>().0.show_found);
    assert_eq!(focus(&mut app), Some(CONTINUE));
    // A click on it does continue (and the beat still would).
    let continue_ = choice(&mut app, CONTINUE);
    click_on(&mut app, continue_);
    frames(&mut app, 2);
    assert_eq!(state(&app), TvState::Join);
}

/// Setup after its own picker was closed (`Picking` already
/// auto-opened, nothing open).
fn setup_after_its_picker_closed() -> App {
    let mut app = tv_on(
        TvScreen::Setup,
        Duration::from_secs(5),
        SetupView {
            needs_setup: true,
            ..Default::default()
        },
    );
    app.insert_resource(Picking::closed_once());
    app.update();
    assert!(!wanted(&mut app), "it opened by itself again");
    app
}

#[test]
fn south_on_setup_after_its_picker_closed_asks_for_it_and_a_click_beside_doesnt() {
    let mut app = setup_after_its_picker_closed();
    assert_eq!(focus(&mut app), Some(CHOOSE_FOLDER));
    click_beside(&mut app);
    assert!(!wanted(&mut app));
    let pad = pad(&mut app);
    button(&mut app, pad, GamepadButton::South, 1.0);
    assert!(wanted(&mut app), "South reached should_open as a press");
    button(&mut app, pad, GamepadButton::South, 0.0);
    assert!(!wanted(&mut app), "one press, one opening");
}

#[test]
fn a_click_beside_the_button_on_no_giants_opens_no_picker() {
    use crate::mirror::{Mirror, TvMirror};
    let setup = SetupView {
        needs_setup: false,
        folders: vec!["/Volumes/dumps".into()],
        found_titles: vec!["Skylanders: SWAP Force (BLUS31076)".into()],
        ..Default::default()
    };
    let mut app = tv_on(TvScreen::NoGiants, Duration::from_secs(5), setup);
    let mirror = TvMirror::default();
    app.insert_resource(mirror.clone()).add_plugins(Mirror);
    click_beside(&mut app);
    frames(&mut app, 2);
    assert!(!wanted(&mut app));
    assert_eq!(app.world().resource::<Current>().screen, TvScreen::NoGiants);
    let shot = mirror.0.lock().unwrap().clone();
    assert_eq!(shot.screen, "NoGiants");
    assert_eq!(shot.focus.as_deref(), Some(CHOOSE_FOLDER));
    // The button itself does ask.
    let choose = choice(&mut app, CHOOSE_FOLDER);
    click_on(&mut app, choose);
    assert!(wanted(&mut app));
}

/// AP.13: South held on the focused Close dips its face by the tap,
/// through `--ease-tap`'s overshoot, and lets it back up to the focus
/// pop when let go, every other node staying put; under `ReducedMotion`
/// each lands at its end the frame it changes.
#[test]
fn the_press_dips_the_focused_face_and_lets_it_back() {
    use bevy::time::TimeUpdateStrategy;

    use crate::look::{FOCUS_SCALE, TAP_SCALE};
    use crate::motion::ReducedMotion;
    use crate::test_support::tv::{outer_nodes, scale_of};
    let pressed = FOCUS_SCALE * TAP_SCALE;
    for reduced in [false, true] {
        let mut app = tv_on(
            error_screens()[1].clone(),
            Duration::ZERO,
            SetupView::default(),
        );
        app.insert_resource(ReducedMotion(reduced)).insert_resource(
            TimeUpdateStrategy::ManualDuration(Duration::from_millis(10)),
        );
        frames(&mut app, 30);
        let close = choice(&mut app, CLOSE);
        assert_eq!(scale_of(&app, close), FOCUS_SCALE, "{reduced}");
        let before = outer_nodes(&mut app);
        let pad = pad(&mut app);
        let run = |app: &mut App, value: f32| {
            button(app, pad, GamepadButton::South, value);
            let mut seen = vec![scale_of(app, close)];
            for _ in 0..20 {
                app.update();
                assert_eq!(outer_nodes(app), before, "{reduced}");
                seen.push(scale_of(app, close));
            }
            seen
        };
        let down = run(&mut app, 1.0);
        assert!(app.world().get::<Pressed>(close).is_some());
        let up = run(&mut app, 0.0);
        assert!(app.world().get::<Pressed>(close).is_none());
        let low = down.iter().copied().fold(f32::MAX, f32::min);
        let high = up.iter().copied().fold(f32::MIN, f32::max);
        assert_eq!(down.last(), Some(&pressed), "{reduced}: {down:?}");
        assert_eq!(up.last(), Some(&FOCUS_SCALE), "{reduced}: {up:?}");
        if reduced {
            assert!(down.iter().all(|s| *s == pressed), "{down:?}");
            assert!(up.iter().all(|s| *s == FOCUS_SCALE), "{up:?}");
        } else {
            assert_eq!(down[0], FOCUS_SCALE, "the press starts from the pop");
            assert!(low < pressed - 0.005, "dips past the tap: {down:?}");
            assert!(high > FOCUS_SCALE + 0.005, "springs past it: {up:?}");
        }
    }
}

fn error_screens() -> [TvScreen; 2] {
    [
        TvScreen::AlreadyRunning,
        TvScreen::ServerError {
            message: "open profile store".into(),
        },
    ]
}

#[test]
fn south_east_esc_and_a_click_anywhere_close_the_error_screens() {
    type Press = fn(&mut App);
    let presses: [(&str, Press); 5] = [
        ("south", |app| {
            let pad = pad(app);
            button(app, pad, GamepadButton::South, 1.0);
            // Held: the close button shows pressed until let go.
            let close = choice(app, CLOSE);
            assert!(app.world().get::<Pressed>(close).is_some());
            button(app, pad, GamepadButton::South, 0.0);
            app.update();
            assert!(app.world().get::<Pressed>(close).is_none());
        }),
        ("east", |app| {
            let pad = pad(app);
            tap(app, pad, GamepadButton::East);
        }),
        ("esc", |app| key(app, KeyCode::Escape, Key::Escape)),
        ("a click anywhere", click_beside),
        ("a click on close", |app| {
            let close = choice(app, CLOSE);
            click_on(app, close);
        }),
    ];
    for screen in error_screens() {
        for (what, press) in presses {
            let mut app = tv_on(screen.clone(), Duration::ZERO, SetupView::default());
            assert_eq!(focus(&mut app), Some(CLOSE), "{screen:?}");
            assert!(!exited(&app), "{screen:?}: closed early");
            press(&mut app);
            assert!(exited(&app), "{screen:?}: {what}");
        }
    }
}

#[test]
fn focus_lands_back_on_the_choose_folder_button_after_setups_lines_respawn() {
    let mut app = setup_after_its_picker_closed();
    let first = choice(&mut app, CHOOSE_FOLDER);
    assert_eq!(app.world().resource::<InputFocus>().get(), Some(first));
    // The sheet opens: the button gives way to a line.
    app.world_mut().resource_mut::<Setup>().0.picking = true;
    app.update();
    let world = app.world_mut();
    assert_eq!(world.query::<&FocusId>().iter(world).count(), 0);
    // It fails: the column respawns with the error and a new button.
    {
        let mut setup = app.world_mut().resource_mut::<Setup>();
        setup.0.picking = false;
        setup.0.error = Some("/Volumes/gone isn't a folder".into());
    }
    app.update();
    let second = choice(&mut app, CHOOSE_FOLDER);
    assert_ne!(second, first);
    app.update();
    assert_eq!(app.world().resource::<InputFocus>().get(), Some(second));
    // A click elsewhere clears the focus (pointer focus); it comes back.
    app.world_mut().resource_mut::<InputFocus>().clear();
    app.update();
    assert_eq!(app.world().resource::<InputFocus>().get(), Some(second));
}

/// Enter goes through the widget (`FocusedInput`, which needs exactly one
/// `PrimaryWindow`: spawned here by hand, no winit), NumpadEnter and
/// South through the router; each activates the button once.
#[test]
fn enter_on_a_focused_button_activates_it_exactly_once() {
    use bevy::ui_widgets::Activate;
    use bevy::window::PrimaryWindow;

    #[derive(Resource, Default)]
    struct Activations(Vec<Entity>);

    let mut app = portal_app(1920, 1080, WindowMode::Tv, PortalCards::default());
    app.add_plugins(bevy::input_focus::InputDispatchPlugin)
        .init_resource::<Activations>()
        .add_observer(|a: On<Activate>, mut seen: ResMut<Activations>| {
            seen.0.push(a.entity);
        });
    app.world_mut().spawn((Window::default(), PrimaryWindow));
    app.update();
    let activations =
        |app: &mut App| std::mem::take(&mut app.world_mut().resource_mut::<Activations>().0);
    let error = TvScreen::ServerError {
        message: "open profile store".into(),
    };
    app.world_mut().resource_mut::<Current>().screen = error;
    frames(&mut app, 2);
    let close = choice(&mut app, CLOSE);
    assert_eq!(app.world().resource::<InputFocus>().get(), Some(close));

    key(&mut app, KeyCode::Enter, Key::Enter);
    frames(&mut app, 2);
    assert_eq!(activations(&mut app), [close], "Enter");
    key(&mut app, KeyCode::Space, Key::Space);
    assert_eq!(activations(&mut app), [close], "Space");
    key(&mut app, KeyCode::NumpadEnter, Key::Enter);
    assert_eq!(activations(&mut app), [close], "NumpadEnter");
    let pad = pad(&mut app);
    tap(&mut app, pad, GamepadButton::South);
    assert_eq!(activations(&mut app), [close], "South");
    // A held key repeating activates nothing more.
    app.world_mut().write_message(KeyboardInput {
        key_code: KeyCode::Enter,
        logical_key: Key::Enter,
        state: ButtonState::Pressed,
        text: None,
        repeat: true,
        window: Entity::PLACEHOLDER,
    });
    app.update();
    assert_eq!(activations(&mut app), [], "a repeat");
}
