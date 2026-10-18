//! The TV: `ServerBridge` polls the server's status each frame, `Tv` draws
//! the screen it maps to (`screen::screen_for`) in `bevy_ui`.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use bevy::asset::RenderAssetUsages;
use bevy::image::{CompressedImageFormats, ImageSampler, ImageType};
use bevy::prelude::*;
use bevy::text::FontSize;
use bevy::window::PrimaryWindow;
use guac_server::state::LauncherStatus;
use tracing::{error, info};

use crate::screen::{SetupView, TvScreen, screen_for};

/// How long the farewell shows before the app exits (spec: under 5 s).
pub const FAREWELL_BEAT: Duration = Duration::from_secs(3);

/// How long AlreadyRunning shows before this copy closes itself.
const ALREADY_RUNNING_BEAT: Duration = Duration::from_secs(8);

/// The layout's design height; `UiScale` maps it to the window.
const DESIGN_HEIGHT: f32 = 1080.0;

const BACKGROUND: Color = Color::srgb(0.04, 0.08, 0.22);
const GOLD: Color = Color::srgb(0.97, 0.78, 0.25);
const WHITE: Color = Color::srgb(0.95, 0.96, 1.0);
const DIM: Color = Color::srgb(0.65, 0.72, 0.88);

static TITAN_ONE: &[u8] = include_bytes!("../../server/assets/fonts/TitanOne-Regular.ttf");

/// What the TV reads from the server; built once in `main`.
#[derive(Resource, Clone)]
pub struct ServerLink {
    pub status: Arc<Mutex<LauncherStatus>>,
    pub clients: Arc<AtomicUsize>,
    pub phone_url: String,
    pub join_qr_png: Arc<Vec<u8>>,
}

#[derive(Resource, Clone)]
pub struct Setup(pub SetupView);

/// The screen this frame, and since when.
#[derive(Resource)]
struct Current {
    screen: TvScreen,
    since: Instant,
    clients: usize,
}

#[derive(Resource)]
struct TvAssets {
    font: Handle<Font>,
    qr: Option<Handle<Image>>,
}

/// Marks the root node; the whole tree is rebuilt when the screen changes.
#[derive(Component)]
struct ScreenRoot;

pub struct ServerBridge;

impl Plugin for ServerBridge {
    fn build(&self, app: &mut App) {
        app.add_systems(PreUpdate, poll_status);
    }
}

pub struct Tv;

impl Plugin for Tv {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, setup_tv).add_systems(
            Update,
            (scale_ui, rebuild_screen, farewell_exit, escape_quits),
        );
    }
}

fn read_status(link: &ServerLink) -> LauncherStatus {
    link.status
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .clone()
}

fn poll_status(
    mut commands: Commands,
    link: Res<ServerLink>,
    setup: Res<Setup>,
    current: Option<ResMut<Current>>,
) {
    let screen = screen_for(&read_status(&link), &setup.0);
    let clients = link.clients.load(Ordering::Relaxed);
    match current {
        Some(mut current) => {
            if current.screen != screen {
                info!(?screen, "tv screen");
                current.screen = screen;
                current.since = Instant::now();
            }
            if current.clients != clients {
                current.clients = clients;
            }
        }
        None => {
            info!(?screen, "tv screen");
            commands.insert_resource(Current {
                screen,
                since: Instant::now(),
                clients,
            });
        }
    }
}

fn setup_tv(
    mut commands: Commands,
    link: Res<ServerLink>,
    mut fonts: ResMut<Assets<Font>>,
    mut images: ResMut<Assets<Image>>,
) {
    commands.spawn(Camera2d);
    let font = fonts.add(Font::from_bytes(TITAN_ONE.to_vec()));
    // A failed boot has no QR; nothing to decode.
    let qr = match (!link.join_qr_png.is_empty()).then(|| {
        Image::from_buffer(
            &link.join_qr_png,
            ImageType::Extension("png"),
            CompressedImageFormats::NONE,
            true,
            ImageSampler::nearest(),
            RenderAssetUsages::default(),
        )
    }) {
        Some(Ok(image)) => Some(images.add(image)),
        Some(Err(e)) => {
            error!("decode the join QR: {e}");
            None
        }
        None => None,
    };
    commands.insert_resource(TvAssets { font, qr });
}

/// Lays out at 1080 logical pixels tall, whatever the window.
fn scale_ui(window: Query<&Window, With<PrimaryWindow>>, mut scale: ResMut<UiScale>) {
    let Ok(window) = window.single() else {
        return;
    };
    let height = window.height();
    if height > 0.0 {
        let wanted = height / DESIGN_HEIGHT;
        if (scale.0 - wanted).abs() > f32::EPSILON {
            scale.0 = wanted;
        }
    }
}

fn rebuild_screen(
    mut commands: Commands,
    current: Option<Res<Current>>,
    assets: Option<Res<TvAssets>>,
    link: Res<ServerLink>,
    setup: Res<Setup>,
    roots: Query<Entity, With<ScreenRoot>>,
) {
    let (Some(current), Some(assets)) = (current, assets) else {
        return;
    };
    if !current.is_changed() && !roots.is_empty() {
        return;
    }
    for root in &roots {
        commands.entity(root).despawn();
    }
    spawn_screen(&mut commands, &assets, &link, &setup.0, &current);
}

fn farewell_exit(current: Option<Res<Current>>, mut exit: MessageWriter<AppExit>) {
    if let Some(current) = current
        && current.screen == TvScreen::Farewell
        && current.since.elapsed() >= FAREWELL_BEAT
    {
        info!("farewell done, exiting");
        exit.write(AppExit::Success);
    }
}

/// Esc or a click closes the error screens (a mouse-only TV PC has no Esc);
/// AlreadyRunning also closes itself so it never hides the running app's TV.
fn escape_quits(
    keys: Res<ButtonInput<KeyCode>>,
    mouse: Res<ButtonInput<MouseButton>>,
    current: Option<Res<Current>>,
    mut exit: MessageWriter<AppExit>,
) {
    let Some(current) = current.filter(|c| c.screen.escape_quits()) else {
        return;
    };
    let pressed = keys.just_pressed(KeyCode::Escape) || mouse.just_pressed(MouseButton::Left);
    let timed_out = current.screen == TvScreen::AlreadyRunning
        && current.since.elapsed() >= ALREADY_RUNNING_BEAT;
    if pressed || timed_out {
        info!(screen = ?current.screen, timed_out, "closing");
        exit.write(AppExit::Success);
    }
}

/// Dev builds say how to set the folder until the picker lands (AA.15).
fn folder_hint() -> Option<&'static str> {
    cfg!(feature = "dev-tools").then_some("Dev: set GUAC_DUMP_DIRS in .env.dev, then restart.")
}

fn spawn_screen(
    commands: &mut Commands,
    assets: &TvAssets,
    link: &ServerLink,
    setup: &SetupView,
    current: &Current,
) {
    let font = assets.font.clone();
    let text = move |s: &str, size: f32, color: Color| {
        (
            Text::new(s),
            TextFont {
                font: font.clone().into(),
                font_size: FontSize::Px(size),
                ..default()
            },
            TextColor(color),
            TextLayout::justify(Justify::Center),
        )
    };

    let mut root = commands.spawn((
        ScreenRoot,
        Node {
            width: percent(100),
            height: percent(100),
            flex_direction: FlexDirection::Column,
            justify_content: JustifyContent::Center,
            align_items: AlignItems::Center,
            row_gap: px(28),
            padding: UiRect::all(px(64)),
            ..default()
        },
        BackgroundColor(BACKGROUND),
    ));

    root.with_children(|ui| match &current.screen {
        TvScreen::Starting => {
            ui.spawn(text("Starting…", 64.0, DIM));
        }
        TvScreen::Setup => {
            ui.spawn(text("Where are your disc dumps?", 88.0, GOLD));
            ui.spawn(text(
                "Choose the folder that holds your Skylanders Giants dump.",
                40.0,
                WHITE,
            ));
            if let Some(hint) = folder_hint() {
                ui.spawn(text(hint, 30.0, DIM));
            }
        }
        TvScreen::NoGiants => {
            ui.spawn(text("No Skylanders Giants dump found", 80.0, GOLD));
            ui.spawn(text(
                &format!("Looked in: {}", setup.folders.join(", ")),
                32.0,
                DIM,
            ));
            if setup.found_titles.is_empty() {
                ui.spawn(text("No game dumps there.", 40.0, WHITE));
            } else {
                ui.spawn(text("Found:", 40.0, WHITE));
                for title in &setup.found_titles {
                    ui.spawn(text(title, 36.0, WHITE));
                }
            }
            ui.spawn(text("Choose another folder.", 40.0, WHITE));
            if let Some(hint) = folder_hint() {
                ui.spawn(text(hint, 30.0, DIM));
            }
        }
        TvScreen::Join => {
            ui.spawn(text("Scan to play", 88.0, GOLD));
            if let Some(qr) = &assets.qr {
                ui.spawn((
                    ImageNode::new(qr.clone()),
                    Node {
                        width: px(420),
                        height: px(420),
                        ..default()
                    },
                ));
            }
            ui.spawn(text(&link.phone_url, 28.0, DIM));
            let players = match current.clients {
                1 => "1 phone connected".to_string(),
                n => format!("{n} phones connected"),
            };
            ui.spawn(text(&players, 40.0, WHITE));
        }
        TvScreen::AlreadyRunning => {
            ui.spawn(text("Already running", 88.0, GOLD));
            ui.spawn(text(
                "guacamole or the Skylander Portal Controller is already serving phones on this computer.",
                40.0,
                WHITE,
            ));
            ui.spawn(text(
                "Press Esc or click to close this one (it closes itself in a few seconds).",
                32.0,
                DIM,
            ));
        }
        TvScreen::ServerError { message } => {
            ui.spawn(text("Something went wrong", 88.0, GOLD));
            ui.spawn(text(message, 36.0, WHITE));
            ui.spawn(text("Press Esc or click to close.", 32.0, DIM));
        }
        TvScreen::Farewell => {
            ui.spawn(text("Goodbye, Portal Master!", 96.0, GOLD));
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Runs `escape_quits` once on `screen`, shown for `age`, with a left click or not.
    fn closes(screen: TvScreen, age: Duration, click: bool) -> bool {
        let mut app = App::new();
        let mut mouse = ButtonInput::<MouseButton>::default();
        if click {
            mouse.press(MouseButton::Left);
        }
        app.insert_resource(ButtonInput::<KeyCode>::default())
            .insert_resource(mouse)
            .insert_resource(Current {
                screen,
                since: Instant::now().checked_sub(age).unwrap(),
                clients: 0,
            })
            .add_systems(Update, escape_quits);
        app.update();
        app.should_exit().is_some()
    }

    #[test]
    fn already_running_closes_itself_after_the_beat() {
        let late = ALREADY_RUNNING_BEAT + Duration::from_secs(1);
        assert!(closes(TvScreen::AlreadyRunning, late, false));
        assert!(!closes(TvScreen::AlreadyRunning, Duration::ZERO, false));
    }

    #[test]
    fn server_error_waits_for_a_click() {
        let error = || TvScreen::ServerError {
            message: "open profile store".into(),
        };
        let late = ALREADY_RUNNING_BEAT * 4;
        assert!(!closes(error(), late, false));
        assert!(closes(error(), Duration::ZERO, true));
        assert!(closes(TvScreen::AlreadyRunning, Duration::ZERO, true));
    }

    #[test]
    fn a_click_never_closes_the_join_screen() {
        assert!(!closes(TvScreen::Join, Duration::ZERO, true));
        assert!(!closes(TvScreen::Join, ALREADY_RUNNING_BEAT * 4, false));
    }
}
