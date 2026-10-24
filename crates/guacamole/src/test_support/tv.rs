//! The TV laid out headless (no window, no GPU) and driven by pad, key and
//! click, from tv's layout and choice tests; the mirror's, the pads', the
//! intro player's, the titles' and the picker's tests stand on them too.

use std::time::{Duration, Instant};

use bevy::camera::{ComputedCameraValues, RenderTargetInfo};
use bevy::input::ButtonState;
use bevy::input::gamepad::{
    GamepadButton, GamepadConnection, GamepadConnectionEvent, RawGamepadButtonChangedEvent,
    RawGamepadEvent,
};
use bevy::input::keyboard::{Key, KeyboardInput};
use bevy::input::mouse::MouseButtonInput;
use bevy::input_focus::InputFocus;
use bevy::prelude::*;
use bevy::ui::{ComputedNode, UiGlobalTransform};
use guac_server::config::WindowMode;
use guac_server::state::{LauncherScreen, LauncherStatus};
use skylander_core::{Element, FigureId};
use skylander_rpcs3_control::{Placement, PlacementOwner};

use crate::card_motion::CardFace;
use crate::cards::PortalCards;
use crate::input::FocusId;
use crate::look::ChoiceFace;
use crate::picker::PickerChoices;
use crate::screen::{SetupView, TvScreen};
use crate::starfield;
use crate::title;
use crate::tv::*;

pub(crate) fn placement(name: &str, owner: Option<&str>) -> Placement {
    let figure_id = FigureId::new("000009-0000");
    let (toy_type, variant) = figure_id.subtype_parts().unwrap();
    Placement {
        figure_id,
        toy_type,
        variant,
        name: name.into(),
        element: Some(Element::Fire),
        portrait: None,
        owner: owner.map(|c| PlacementOwner {
            profile_id: "p".into(),
            initial: "A".into(),
            color: c.into(),
            progress: Default::default(),
        }),
    }
}

pub(crate) fn portal_app(width: u32, height: u32, mode: WindowMode, cards: PortalCards) -> App {
    let mut app = App::new();
    app.add_plugins((
        MinimalPlugins,
        bevy::asset::AssetPlugin::default(),
        bevy::image::ImagePlugin::default(),
        bevy::image::TextureAtlasPlugin,
        bevy::input::InputPlugin,
        WindowPlugin {
            primary_window: None,
            exit_condition: bevy::window::ExitCondition::DontExit,
            ..default()
        },
        bevy::transform::TransformPlugin,
        bevy::text::TextPlugin,
        // `bevy_ui`'s viewport picking needs its resources; no pointer moves.
        bevy::picking::DefaultPickingPlugins,
        bevy::ui::UiPlugin,
        bevy::state::app::StatesPlugin,
        bevy::input_focus::InputFocusPlugin,
        bevy::ui_widgets::ButtonPlugin,
        crate::input::InputRouter,
    ));
    let font = app
        .world_mut()
        .resource_mut::<Assets<Font>>()
        .add(Font::from_bytes(TITAN_ONE.to_vec()));
    let stars = starfield::Sprites::new(&mut app.world_mut().resource_mut::<Assets<Image>>());
    let assets = TvAssets {
        font,
        qr: None,
        stars,
    };
    app.insert_resource(assets)
        .insert_resource(ServerLink {
            status: Default::default(),
            clients: Default::default(),
            phone_url: format!(
                "http://christophers-macbook-pro.local:8765/?k={}",
                "0123456789abcdef".repeat(4)
            ),
            join_qr_png: Default::default(),
            games: Default::default(),
        })
        .insert_resource(Setup(SetupView::default()))
        .insert_resource(Framing(mode))
        .insert_resource(cards)
        .insert_resource(Current {
            screen: TvScreen::Portal {
                game: "Skylanders: Giants".into(),
            },
            since: Instant::now(),
            clients: 2,
        })
        .insert_resource(UiScale(height as f32 / DESIGN_HEIGHT))
        .add_plugins(Screens);
    app.world_mut().spawn((
        Camera2d,
        TvCamera,
        IsDefaultUiCamera,
        Camera {
            computed: ComputedCameraValues {
                target_info: Some(RenderTargetInfo {
                    physical_size: UVec2::new(width, height),
                    scale_factor: 1.0,
                }),
                ..default()
            },
            ..default()
        },
    ));
    app
}

pub(crate) fn tv_camera(world: &mut World) -> Entity {
    let mut q = world.query_filtered::<Entity, With<TvCamera>>();
    q.single(world).unwrap()
}

/// Where every node sits (its size and its whole transform, scale
/// included), but for the choices' and the cards' faces and what's on
/// them, the drifting sky and the titles' ink, which is laid out on each
/// title's own camera, a frame after the screen's.
pub(crate) fn outer_nodes(
    app: &mut App,
) -> std::collections::BTreeMap<Entity, (Vec2, UiGlobalTransform)> {
    let world = app.world_mut();
    let mut skip = Vec::new();
    let mut q = world.query_filtered::<Entity, Or<(
        With<ChoiceFace>,
        With<CardFace>,
        With<starfield::StarLayer>,
        With<title::TitleInk>,
    )>>();
    let mut stack: Vec<Entity> = q.iter(world).collect();
    while let Some(e) = stack.pop() {
        skip.push(e);
        if let Some(kids) = world.get::<Children>(e) {
            stack.extend(kids.iter());
        }
    }
    let mut q = world.query::<(Entity, &ComputedNode, &UiGlobalTransform)>();
    q.iter(world)
        .filter(|(e, _, _)| !skip.contains(e))
        .map(|(e, n, t)| (e, (n.size(), *t)))
        .collect()
}

fn face_of(app: &App, button: Entity) -> Entity {
    app.world().get::<Children>(button).unwrap()[0]
}

pub(crate) fn scale_of(app: &App, button: Entity) -> f32 {
    let scale = app
        .world()
        .get::<UiTransform>(face_of(app, button))
        .unwrap()
        .scale;
    assert_eq!(scale.x, scale.y);
    scale.x
}

/// An `AppExit` was written: messages last two frames, this lasts.
#[derive(Resource, Default)]
pub(crate) struct Exited(bool);

fn note_exit(mut exits: MessageReader<AppExit>, mut exited: ResMut<Exited>) {
    exited.0 |= exits.read().count() > 0;
}

pub(crate) fn exited(app: &App) -> bool {
    app.world().resource::<Exited>().0
}

/// The TV on `screen`, shown for `age`, its status ready and `poll_status`
/// following `Setup` the way the running app does.
pub(crate) fn tv_on(screen: TvScreen, age: Duration, setup: SetupView) -> App {
    let mut app = portal_app(1920, 1080, WindowMode::Tv, PortalCards::default());
    let status = match &screen {
        TvScreen::ServerError { message } => LauncherStatus {
            screen: LauncherScreen::ServerError {
                message: message.clone(),
            },
            ..Default::default()
        },
        TvScreen::AlreadyRunning => LauncherStatus {
            screen: LauncherScreen::ServerError {
                message: "bind".into(),
            },
            port_in_use: true,
            ..Default::default()
        },
        _ => LauncherStatus {
            server_ready: true,
            ..Default::default()
        },
    };
    *app.world().resource::<ServerLink>().status.lock().unwrap() = status;
    app.insert_resource(Setup(setup))
        .init_resource::<Exited>()
        .add_plugins(PickerChoices)
        .add_systems(PreUpdate, poll_status)
        .add_systems(Last, note_exit);
    {
        let mut current = app.world_mut().resource_mut::<Current>();
        current.screen = screen.clone();
        current.since = Instant::now().checked_sub(age).unwrap();
    }
    frames(&mut app, 2);
    assert_eq!(app.world().resource::<Current>().screen, screen);
    app
}

pub(crate) fn frames(app: &mut App, n: usize) {
    for _ in 0..n {
        app.update();
    }
}

/// The focused choice's id, if any.
pub(crate) fn focus(app: &mut App) -> Option<&'static str> {
    let entity = app.world().resource::<InputFocus>().get()?;
    app.world().get::<FocusId>(entity).map(|id| id.0)
}

/// The one choice marked `id`.
pub(crate) fn choice(app: &mut App, id: &str) -> Entity {
    let world = app.world_mut();
    let mut q = world.query::<(Entity, &FocusId)>();
    let found: Vec<Entity> = q
        .iter(world)
        .filter(|(_, f)| f.0 == id)
        .map(|(e, _)| e)
        .collect();
    assert_eq!(found.len(), 1, "{id}: {found:?}");
    found[0]
}

pub(crate) fn pad(app: &mut App) -> Entity {
    let pad = app.world_mut().spawn_empty().id();
    app.world_mut().write_message(GamepadConnectionEvent::new(
        pad,
        GamepadConnection::Connected {
            name: "pad".into(),
            vendor_id: None,
            product_id: None,
        },
    ));
    app.update();
    pad
}

/// `button` down (1) or up (0) for a frame.
pub(crate) fn button(app: &mut App, pad: Entity, button: GamepadButton, value: f32) {
    app.world_mut()
        .write_message(RawGamepadEvent::Button(RawGamepadButtonChangedEvent::new(
            pad, button, value,
        )));
    app.update();
}

/// A press and release of `button`, a frame each.
pub(crate) fn tap(app: &mut App, pad: Entity, which: GamepadButton) {
    button(app, pad, which, 1.0);
    button(app, pad, which, 0.0);
}

/// A key down then up, a frame each.
pub(crate) fn key(app: &mut App, code: KeyCode, logical: Key) {
    for state in [ButtonState::Pressed, ButtonState::Released] {
        app.world_mut().write_message(KeyboardInput {
            key_code: code,
            logical_key: logical.clone(),
            state,
            text: None,
            repeat: false,
            window: Entity::PLACEHOLDER,
        });
        app.update();
    }
}

/// Lands on no button.
pub(crate) fn click_beside(app: &mut App) {
    for state in [ButtonState::Pressed, ButtonState::Released] {
        mouse(app, state);
        app.update();
    }
}

/// The left mouse button, as winit reports it (`InputPlugin` clears a
/// `ButtonInput` written by hand before anything reads it).
pub(crate) fn mouse(app: &mut App, state: ButtonState) {
    app.world_mut().write_message(MouseButtonInput {
        button: MouseButton::Left,
        state,
        window: Entity::PLACEHOLDER,
    });
}

/// A click on `entity` as picking reports one over it: the mouse button
/// and the pointer's press, then its click and release.
pub(crate) fn click_on(app: &mut App, entity: Entity) {
    use bevy::camera::NormalizedRenderTarget;
    use bevy::picking::backend::HitData;
    use bevy::picking::events::{Pointer, PointerClick, PointerPress, PointerRelease};
    use bevy::picking::pointer::{Location, PointerButton, PointerId};
    let pointer = Pointer::new(
        PointerId::Mouse,
        Location {
            target: NormalizedRenderTarget::None {
                width: 1920,
                height: 1080,
            },
            position: Vec2::ZERO,
        },
    );
    let hit = HitData::new(entity, 0.0, None, None);
    let primary = PointerButton::Primary;
    mouse(app, ButtonState::Pressed);
    let world = app.world_mut();
    world.trigger(PointerPress {
        entity,
        pointer: pointer.clone(),
        button: primary,
        hit: hit.clone(),
        count: 1,
    });
    app.update();
    mouse(app, ButtonState::Released);
    let world = app.world_mut();
    world.trigger(PointerClick {
        entity,
        pointer: pointer.clone(),
        button: primary,
        hit: hit.clone(),
        duration: Duration::from_millis(80),
        count: 1,
    });
    world.trigger(PointerRelease {
        entity,
        pointer,
        button: primary,
        hit,
    });
    app.update();
}
