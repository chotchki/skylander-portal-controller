//! The TV: `ServerBridge` polls the server's status each frame, `Tv` draws
//! the screen it maps to (`screen::screen_for`) in `bevy_ui`.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use bevy::asset::RenderAssetUsages;
use bevy::image::{CompressedImageFormats, ImageSampler, ImageType};
use bevy::prelude::*;
use bevy::text::FontSize;
use bevy::ui::{ComputedNode, UiGlobalTransform};
use guac_server::config::WindowMode;
use guac_server::state::{GameList, LauncherStatus};
use skylander_core::{Element, SLOT_COUNT};
use skylander_rpcs3_control::{Placement, PlacementOwner};
use tracing::{error, info, warn};

use crate::cards::{PortalCards, drain_portal};
use crate::extraction::{Band, ExtractBand};
use crate::screen::{SetupView, TvScreen, screen_for};

/// How long the farewell shows before the app exits (spec: under 5 s).
pub const FAREWELL_BEAT: Duration = Duration::from_secs(3);

/// How long AlreadyRunning shows before this copy closes itself.
const ALREADY_RUNNING_BEAT: Duration = Duration::from_secs(8);

/// The layout's design height; `UiScale` maps it to the window.
const DESIGN_HEIGHT: f32 = 1080.0;

/// The product launcher's TV title-safe inset (`TV_OVERSCAN_FRAC`,
/// `crates/server/src/ui/main_screen.rs:125`), copied: guacamole can't link
/// the eframe crate. The 86" TV crops its edges (R.1).
pub const TV_OVERSCAN_FRAC: f32 = 0.06;

/// Desktop windows don't crop; this is just breathing room.
const DESKTOP_MARGIN: f32 = 48.0;

/// Cards per portal row; two rows hold all eight slots at 16:9.
const CARDS_PER_ROW: usize = 4;

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
    /// The phone's game list; a pick replaces it.
    pub games: GameList,
}

#[derive(Resource, Clone)]
pub struct Setup(pub SetupView);

/// The window mode: TV mode keeps everything inside the overscan inset.
#[derive(Resource, Clone, Copy)]
pub struct Framing(pub WindowMode);

/// Portraits decoded so far, by path; `None` for one that wouldn't load.
#[derive(Resource, Default)]
struct Portraits(HashMap<PathBuf, Option<Handle<Image>>>);

impl Portraits {
    /// Reads and decodes on first use; a 128 px thumb costs well under a frame.
    fn get(&mut self, path: &Path, images: &mut Assets<Image>) -> Option<Handle<Image>> {
        self.0
            .entry(path.to_path_buf())
            .or_insert_with(|| {
                let decoded = std::fs::read(path)
                    .map_err(|e| e.to_string())
                    .and_then(|bytes| {
                        Image::from_buffer(
                            &bytes,
                            ImageType::Extension("png"),
                            CompressedImageFormats::NONE,
                            true,
                            ImageSampler::linear(),
                            RenderAssetUsages::default(),
                        )
                        .map_err(|e| e.to_string())
                    });
                match decoded {
                    Ok(image) => Some(images.add(image)),
                    Err(e) => {
                        warn!(path = %path.display(), "portrait didn't load: {e}");
                        None
                    }
                }
            })
            .clone()
    }
}

/// The screen this frame, and since when.
#[derive(Resource)]
pub(crate) struct Current {
    pub(crate) screen: TvScreen,
    pub(crate) since: Instant,
    pub(crate) clients: usize,
}

#[derive(Resource)]
pub(crate) struct TvAssets {
    font: Handle<Font>,
    pub(crate) qr: Option<Handle<Image>>,
}

/// The join QR as drawn (Join and the portal header), for the mirror.
#[derive(Component)]
pub(crate) struct JoinQr;

/// The join address line as drawn.
#[derive(Component)]
pub(crate) struct JoinAddress;

/// The "N phones connected" line as drawn.
#[derive(Component)]
pub(crate) struct PlayersLine;

/// The extraction band's lines and bar fill (percent) as drawn.
#[derive(Component)]
pub(crate) struct ExtractTitle;

#[derive(Component)]
pub(crate) struct ExtractDetail;

#[derive(Component)]
pub(crate) struct ExtractBar(pub(crate) u8);

/// Marks the root node; the whole tree is rebuilt when the screen changes.
#[derive(Component)]
struct ScreenRoot;

pub struct ServerBridge;

impl Plugin for ServerBridge {
    fn build(&self, app: &mut App) {
        app.init_resource::<PortalCards>()
            .add_message::<crate::cards::PortalChanged>()
            .add_systems(PreUpdate, (poll_status, drain_portal));
    }
}

pub struct Tv;

impl Plugin for Tv {
    fn build(&self, app: &mut App) {
        app.init_resource::<Portraits>()
            .init_resource::<LayoutReport>()
            .add_systems(Startup, setup_tv)
            .add_systems(Last, report_layout)
            .add_systems(
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
                info!(clients, "tv players");
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
/// Reads the camera's target, not the window, so a dev offscreen render
/// (`shots`) scales the same way.
fn scale_ui(camera: Query<&Camera, With<Camera2d>>, mut scale: ResMut<UiScale>) {
    let Some(size) = camera.single().ok().and_then(Camera::logical_target_size) else {
        return;
    };
    let height = size.y;
    if height > 0.0 {
        let wanted = height / DESIGN_HEIGHT;
        if (scale.0 - wanted).abs() > f32::EPSILON {
            scale.0 = wanted;
        }
    }
}

/// The inset every screen sits inside, as fractions of the window.
pub fn inset(mode: WindowMode) -> UiRect {
    match mode {
        WindowMode::Tv => {
            let pct = TV_OVERSCAN_FRAC * 100.0;
            UiRect::axes(vw(pct), vh(pct))
        }
        WindowMode::Desktop => UiRect::all(px(DESKTOP_MARGIN)),
    }
}

/// Rebuilds the whole tree on any change: at most eight cards, so a
/// rebuild costs less than diffing would.
fn rebuild_screen(
    mut commands: Commands,
    current: Option<Res<Current>>,
    assets: Option<Res<TvAssets>>,
    link: Res<ServerLink>,
    setup: Res<Setup>,
    framing: Res<Framing>,
    cards: Res<PortalCards>,
    band: Option<Res<ExtractBand>>,
    mut portraits: ResMut<Portraits>,
    mut images: ResMut<Assets<Image>>,
    mut report: ResMut<LayoutReport>,
    roots: Query<Entity, With<ScreenRoot>>,
) {
    let (Some(current), Some(assets)) = (current, assets) else {
        return;
    };
    let on_portal = matches!(current.screen, TvScreen::Portal { .. });
    let band_changed = band.as_ref().is_some_and(|b| b.is_changed());
    let portal_changed = on_portal && (cards.is_changed() || band_changed);
    if !current.is_changed() && !setup.is_changed() && !portal_changed && !roots.is_empty() {
        return;
    }
    for root in &roots {
        commands.entity(root).despawn();
    }
    report.0 = if matches!(current.screen, TvScreen::Portal { .. }) {
        REPORT_AFTER_FRAMES
    } else {
        0
    };
    let portal = PortalView {
        cards: &cards,
        band: band.as_ref().and_then(|b| b.0.as_ref()),
        portraits: &mut portraits,
        images: &mut images,
    };
    spawn_screen(
        &mut commands,
        &assets,
        &link,
        &setup.0,
        &current,
        framing.0,
        portal,
    );
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

/// What the folder picker is doing, under Setup and NoGiants.
fn picker_lines(setup: &SetupView, choose: &str) -> Vec<(String, f32, Color)> {
    let mut lines = Vec::new();
    if let Some(error) = &setup.error {
        lines.push((format!("Couldn't use that folder: {error}"), 32.0, GOLD));
    }
    if setup.picking {
        lines.push((
            "Choose a folder in the window that opened.".into(),
            40.0,
            WHITE,
        ));
    } else {
        lines.push((format!("Press Enter or click to {choose}."), 40.0, WHITE));
    }
    lines.push(("Or run: guacamole --setup <folder>".into(), 28.0, DIM));
    lines
}

/// What the portal screen draws from.
struct PortalView<'a> {
    cards: &'a PortalCards,
    /// The extraction's state, drawn in the header.
    band: Option<&'a Band>,
    portraits: &'a mut Portraits,
    images: &'a mut Assets<Image>,
}

/// The join URL without its pairing key: the key rides in the QR, and the
/// TV's text is for reading, not typing.
pub fn join_address(url: &str) -> &str {
    url.split(['?', '#'])
        .next()
        .unwrap_or(url)
        .trim_end_matches('/')
}

fn players_line(clients: usize) -> String {
    match clients {
        1 => "1 phone connected".to_string(),
        n => format!("{n} phones connected"),
    }
}

fn spawn_screen(
    commands: &mut Commands,
    assets: &TvAssets,
    link: &ServerLink,
    setup: &SetupView,
    current: &Current,
    mode: WindowMode,
    portal: PortalView,
) {
    if let TvScreen::Portal { game } = &current.screen {
        spawn_portal(commands, assets, link, current, mode, game, portal);
        return;
    }
    let font = assets.font.clone();
    // Wraps within the column rather than running off its sides.
    let titles = (
        Text::new(setup.titles_line()),
        TextFont {
            font: font.clone().into(),
            font_size: FontSize::Px(36.0),
            ..default()
        },
        TextColor(WHITE),
        TextLayout::justify(Justify::Center),
        Node {
            max_width: percent(100),
            ..default()
        },
    );
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
            // Safe: a column taller than the screen starts at the top, not off it.
            justify_content: JustifyContent::CenterSafe,
            align_items: AlignItems::Center,
            row_gap: px(28),
            padding: inset(mode),
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
            for (line, size, color) in picker_lines(setup, "choose it") {
                ui.spawn(text(&line, size, color));
            }
        }
        TvScreen::NoGiants => {
            ui.spawn(text("No Skylanders Giants dump found", 80.0, GOLD));
            ui.spawn(text(
                &format!("Looked in: {}", setup.folders.join(", ")),
                32.0,
                DIM,
            ));
            for problem in &setup.problems {
                ui.spawn(text(problem, 32.0, GOLD));
            }
            if !setup.found_titles.is_empty() {
                ui.spawn(text("Found:", 40.0, WHITE));
                ui.spawn(titles.clone());
            } else if setup.problems.is_empty() {
                ui.spawn(text("No game dumps there.", 40.0, WHITE));
            }
            for (line, size, color) in picker_lines(setup, "choose another folder") {
                ui.spawn(text(&line, size, color));
            }
        }
        TvScreen::Found => {
            ui.spawn(text("Found your games", 88.0, GOLD));
            ui.spawn(titles.clone());
            ui.spawn(text("Skylanders: Giants is ready!", 56.0, GOLD));
            ui.spawn(text("Click to continue.", 32.0, DIM));
        }
        TvScreen::Join => {
            ui.spawn(text("Scan to play", 88.0, GOLD));
            if let Some(qr) = &assets.qr {
                ui.spawn((
                    JoinQr,
                    ImageNode::new(qr.clone()),
                    Node {
                        width: px(420),
                        height: px(420),
                        ..default()
                    },
                ));
            }
            ui.spawn((JoinAddress, text(join_address(&link.phone_url), 32.0, DIM)));
            ui.spawn((PlayersLine, text(&players_line(current.clients), 40.0, WHITE)));
        }
        // Drawn by `spawn_portal`.
        TvScreen::Portal { .. } => {}
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

const CARD_BG: Color = Color::srgb(0.08, 0.14, 0.34);
const EMPTY_EDGE: Color = Color::srgba(0.65, 0.72, 0.88, 0.22);
const PORTRAIT_BG: Color = Color::srgb(0.03, 0.05, 0.14);

/// A card on the portal screen, by slot (0-based).
#[derive(Component, Debug)]
pub(crate) struct CardSlot(pub(crate) u8);

/// An empty slot's frame, so the cards around it keep their places.
#[derive(Component, Debug)]
pub(crate) struct EmptySlot(pub(crate) u8);

/// A card's owner pip, by slot.
#[derive(Component, Debug)]
pub(crate) struct Pip(pub(crate) u8);

/// The element as a word ("Fire").
pub fn element_label(element: Element) -> &'static str {
    match element {
        Element::Air => "Air",
        Element::Dark => "Dark",
        Element::Earth => "Earth",
        Element::Fire => "Fire",
        Element::Life => "Life",
        Element::Light => "Light",
        Element::Magic => "Magic",
        Element::Tech => "Tech",
        Element::Undead => "Undead",
        Element::Water => "Water",
    }
}

/// The card's edge and element word; dim for a figure with no element.
pub fn element_color(element: Option<Element>) -> Color {
    match element {
        Some(Element::Air) => Color::srgb(0.55, 0.85, 1.0),
        Some(Element::Dark) => Color::srgb(0.62, 0.45, 0.85),
        Some(Element::Earth) => Color::srgb(0.78, 0.55, 0.30),
        Some(Element::Fire) => Color::srgb(1.0, 0.42, 0.18),
        Some(Element::Life) => Color::srgb(0.45, 0.85, 0.30),
        Some(Element::Light) => Color::srgb(1.0, 0.95, 0.60),
        Some(Element::Magic) => Color::srgb(0.85, 0.40, 1.0),
        Some(Element::Tech) => Color::srgb(1.0, 0.70, 0.15),
        Some(Element::Undead) => Color::srgb(0.70, 0.72, 0.80),
        Some(Element::Water) => Color::srgb(0.25, 0.55, 1.0),
        None => DIM,
    }
}

/// The profile's colour as the store keeps it (`#rgb` or `#rrggbb`); gold if
/// it won't parse, so a pip never goes invisible.
pub fn owner_color(owner: &PlacementOwner) -> Srgba {
    Srgba::hex(&owner.color).unwrap_or(GOLD.to_srgba())
}

/// Dark ink on a light pip, white on a dark one.
pub fn ink_on(background: Srgba) -> Color {
    let luma = 0.2126 * background.red + 0.7152 * background.green + 0.0722 * background.blue;
    if luma > 0.6 {
        Color::srgb(0.05, 0.05, 0.1)
    } else {
        Color::WHITE
    }
}

/// The window shrunk by the inset (TV) or the margin (desktop), in physical
/// pixels, with a pixel of slack because layout snaps edges to whole pixels.
pub fn safe_area(width: u32, height: u32, mode: WindowMode) -> Rect {
    let (w, h) = (width as f32, height as f32);
    let (dx, dy) = match mode {
        WindowMode::Tv => (w * TV_OVERSCAN_FRAC, h * TV_OVERSCAN_FRAC),
        WindowMode::Desktop => {
            let m = DESKTOP_MARGIN * h / DESIGN_HEIGHT;
            (m, m)
        }
    };
    Rect::new(dx - 1.0, dy - 1.0, w - dx + 1.0, h - dy + 1.0)
}

pub(crate) fn contains(outer: Rect, inner: Rect) -> bool {
    outer.contains(inner.min) && outer.contains(inner.max)
}

pub(crate) fn node_rect(node: &ComputedNode, at: &UiGlobalTransform) -> Rect {
    Rect::from_center_size(at.translation, node.size())
}

/// Frames until `report_layout` logs a rebuilt portal: layout and text
/// measuring settle over a couple of frames.
#[derive(Resource, Default)]
struct LayoutReport(u8);

const REPORT_AFTER_FRAMES: u8 = 3;

/// Logs where the portal's cards landed in the real window (`tv layout`), so a
/// run shows whether every card fits inside the inset without watching it.
fn report_layout(
    mut pending: ResMut<LayoutReport>,
    framing: Res<Framing>,
    camera: Query<&Camera, With<Camera2d>>,
    cards: Query<(&CardSlot, &ComputedNode, &UiGlobalTransform)>,
    empties: Query<&EmptySlot>,
    pips: Query<(&Pip, &ComputedNode, &UiGlobalTransform)>,
) {
    match pending.0 {
        0 => return,
        1 => pending.0 = 0,
        _ => {
            pending.0 -= 1;
            return;
        }
    }
    let Some(target) = camera.single().ok().and_then(Camera::physical_target_size) else {
        return;
    };
    let mut rects: Vec<(u8, Rect)> = cards
        .iter()
        .map(|(s, n, t)| (s.0, node_rect(n, t)))
        .collect();
    if rects.is_empty() && empties.is_empty() {
        return;
    }
    rects.sort_by_key(|(s, _)| *s);
    let (w, h) = (target.x, target.y);
    let safe = safe_area(w, h, framing.0);
    let outside: Vec<u8> = rects
        .iter()
        .filter(|(_, r)| !contains(safe, *r))
        .map(|(s, _)| s + 1)
        .collect();
    let overlapping = rects
        .iter()
        .enumerate()
        .flat_map(|(i, (_, a))| rects[i + 1..].iter().map(move |(_, b)| (*a, *b)))
        .filter(|(a, b)| !a.intersect(*b).is_empty())
        .count();
    let pips_off_card = pips
        .iter()
        .filter(|(p, n, t)| {
            let card = rects.iter().find(|(s, _)| s == &p.0).map(|(_, r)| *r);
            card.is_none_or(|c| !contains(c, node_rect(n, t)))
        })
        .count();
    let mut empty: Vec<u8> = empties.iter().map(|e| e.0 + 1).collect();
    empty.sort();
    let card_size = rects
        .first()
        .map(|(_, r)| format!("{:.0}x{:.0}", r.width(), r.height()))
        .unwrap_or_default();
    info!(
        window = %format!("{w}x{h}"),
        mode = ?framing.0,
        cards = rects.len(),
        ?empty,
        card_size,
        ?outside,
        overlapping,
        pips_off_card,
        "tv layout"
    );
}

/// The portal: the game and a small join code on top, then the slots in two
/// rows of four, each holding its card or an empty frame.
fn spawn_portal(
    commands: &mut Commands,
    assets: &TvAssets,
    link: &ServerLink,
    current: &Current,
    mode: WindowMode,
    game: &str,
    portal: PortalView,
) {
    let font = assets.font.clone();
    let text_at = |s: &str, size: f32, color: Color, justify: Justify| {
        (
            Text::new(s),
            TextFont {
                font: font.clone().into(),
                font_size: FontSize::Px(size),
                ..default()
            },
            TextColor(color),
            TextLayout::justify(justify),
        )
    };
    let text = |s: &str, size: f32, color: Color| text_at(s, size, color, Justify::Center);
    let PortalView {
        cards,
        band,
        portraits,
        images,
    } = portal;
    let empty = cards.cards().next().is_none();

    let mut root = commands.spawn((
        ScreenRoot,
        Node {
            width: percent(100),
            height: percent(100),
            flex_direction: FlexDirection::Column,
            row_gap: px(24),
            padding: inset(mode),
            ..default()
        },
        BackgroundColor(BACKGROUND),
    ));
    root.with_children(|ui| {
        // Header: the game on the left, the small join code on the right.
        ui.spawn(Node {
            flex_direction: FlexDirection::Row,
            justify_content: JustifyContent::SpaceBetween,
            align_items: AlignItems::Center,
            column_gap: px(24),
            // Grows only for a band whose detail wraps.
            min_height: px(184),
            flex_shrink: 0.0,
            ..default()
        })
        .with_children(|header| {
            header
                .spawn(Node {
                    flex_direction: FlexDirection::Column,
                    align_items: AlignItems::FlexStart,
                    row_gap: px(8),
                    min_width: px(0),
                    flex_grow: 1.0,
                    flex_shrink: 1.0,
                    ..default()
                })
                .with_children(|left| {
                    left.spawn(text_at(game, 64.0, GOLD, Justify::Left));
                    if let Some(band) = band {
                        spawn_band(left, &text_at, band);
                    } else if empty {
                        left.spawn(text_at(
                            "Put a figure on the portal from your phone.",
                            32.0,
                            WHITE,
                            Justify::Left,
                        ));
                    }
                });
            header
                .spawn(Node {
                    flex_direction: FlexDirection::Row,
                    align_items: AlignItems::Center,
                    column_gap: px(20),
                    flex_shrink: 0.0,
                    ..default()
                })
                .with_children(|right| {
                    right
                        .spawn(Node {
                            flex_direction: FlexDirection::Column,
                            align_items: AlignItems::FlexEnd,
                            row_gap: px(6),
                            ..default()
                        })
                        .with_children(|join| {
                            join.spawn(text_at("Scan to join", 32.0, WHITE, Justify::Right));
                            join.spawn((
                                PlayersLine,
                                text_at(&players_line(current.clients), 28.0, DIM, Justify::Right),
                            ));
                            join.spawn((
                                JoinAddress,
                                text_at(join_address(&link.phone_url), 24.0, DIM, Justify::Right),
                            ));
                        });
                    if let Some(qr) = &assets.qr {
                        right.spawn((
                            JoinQr,
                            ImageNode::new(qr.clone()),
                            Node {
                                width: px(176),
                                height: px(176),
                                flex_shrink: 0.0,
                                ..default()
                            },
                        ));
                    }
                });
        });

        // Two rows of four; every cell grows equally, so eight always fit.
        ui.spawn(Node {
            flex_direction: FlexDirection::Column,
            row_gap: px(24),
            flex_grow: 1.0,
            flex_basis: px(0),
            min_height: px(0),
            ..default()
        })
        .with_children(|grid| {
            for row in 0..SLOT_COUNT / CARDS_PER_ROW {
                grid.spawn(Node {
                    flex_direction: FlexDirection::Row,
                    column_gap: px(24),
                    flex_grow: 1.0,
                    flex_basis: px(0),
                    min_height: px(0),
                    ..default()
                })
                .with_children(|cells| {
                    for col in 0..CARDS_PER_ROW {
                        let slot = row * CARDS_PER_ROW + col;
                        match &cards.slots[slot] {
                            Some(placement) => {
                                let portrait = placement
                                    .portrait
                                    .as_deref()
                                    .and_then(|p| portraits.get(p, images));
                                spawn_card(cells, &text, slot as u8, placement, portrait);
                            }
                            None => spawn_empty(cells, &text, slot as u8),
                        }
                    }
                });
            }
        });
    });
}

/// The extraction's title, detail and bar, under the game's name.
fn spawn_band<T: Bundle>(
    left: &mut ChildSpawnerCommands,
    text_at: &impl Fn(&str, f32, Color, Justify) -> T,
    band: &Band,
) {
    left.spawn((
        ExtractTitle,
        text_at(&band.title, 30.0, GOLD, Justify::Left),
    ));
    let color = if band.code.is_some() { WHITE } else { DIM };
    left.spawn((
        ExtractDetail,
        text_at(&band.detail, 22.0, color, Justify::Left),
    ));
    if let Some(pct) = band.percent {
        left.spawn((
            Node {
                width: percent(100),
                max_width: px(640),
                height: px(12),
                flex_shrink: 0.0,
                border_radius: BorderRadius::MAX,
                overflow: Overflow::clip(),
                ..default()
            },
            BackgroundColor(EMPTY_EDGE),
        ))
        .with_children(|bar| {
            bar.spawn((
                ExtractBar(pct),
                Node {
                    width: percent(f32::from(pct)),
                    height: percent(100),
                    ..default()
                },
                BackgroundColor(GOLD),
            ));
        });
    }
}

/// A grid cell: grows to an equal share and clips anything that doesn't fit.
fn cell() -> Node {
    Node {
        flex_grow: 1.0,
        flex_basis: px(0),
        min_width: px(0),
        min_height: px(0),
        flex_direction: FlexDirection::Column,
        align_items: AlignItems::Center,
        justify_content: JustifyContent::Center,
        row_gap: px(10),
        padding: UiRect::all(px(16)),
        border: UiRect::all(px(4)),
        border_radius: BorderRadius::all(px(24)),
        overflow: Overflow::clip(),
        ..default()
    }
}

/// Portrait, name, element and owner pip; no level, XP or gold (AH).
fn spawn_card<T: Bundle>(
    cells: &mut ChildSpawnerCommands,
    text: &impl Fn(&str, f32, Color) -> T,
    slot: u8,
    placement: &Placement,
    portrait: Option<Handle<Image>>,
) {
    let edge = element_color(placement.element);
    cells
        .spawn((
            CardSlot(slot),
            cell(),
            BackgroundColor(CARD_BG),
            BorderColor::all(edge),
        ))
        .with_children(|card| {
            let frame = Node {
                width: px(160),
                height: px(160),
                flex_shrink: 0.0,
                border: UiRect::all(px(6)),
                border_radius: BorderRadius::MAX,
                ..default()
            };
            match portrait {
                Some(image) => {
                    card.spawn((
                        frame,
                        ImageNode::new(image),
                        BorderColor::all(GOLD),
                        BackgroundColor(PORTRAIT_BG),
                    ));
                }
                None => {
                    card.spawn((frame, BorderColor::all(GOLD), BackgroundColor(PORTRAIT_BG)));
                }
            }
            card.spawn((
                text(&placement.name, 36.0, WHITE),
                Node {
                    max_width: percent(100),
                    ..default()
                },
            ));
            if let Some(element) = placement.element {
                card.spawn(text(element_label(element), 28.0, edge));
            }
            if let Some(owner) = &placement.owner {
                let color = owner_color(owner);
                card.spawn((
                    Pip(slot),
                    Node {
                        position_type: PositionType::Absolute,
                        top: px(10),
                        right: px(10),
                        width: px(60),
                        height: px(60),
                        border: UiRect::all(px(4)),
                        border_radius: BorderRadius::MAX,
                        align_items: AlignItems::Center,
                        justify_content: JustifyContent::Center,
                        ..default()
                    },
                    BackgroundColor(color.into()),
                    BorderColor::all(WHITE),
                ))
                .with_children(|pip| {
                    pip.spawn(text(&owner.initial, 34.0, ink_on(color)));
                });
            }
        });
}

fn spawn_empty<T: Bundle>(
    cells: &mut ChildSpawnerCommands,
    text: &impl Fn(&str, f32, Color) -> T,
    slot: u8,
) {
    cells
        .spawn((EmptySlot(slot), cell(), BorderColor::all(EMPTY_EDGE)))
        .with_children(|frame| {
            frame.spawn(text(&(slot + 1).to_string(), 56.0, EMPTY_EDGE));
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
    fn tv_mode_insets_by_the_overscan_fraction_and_desktop_by_a_margin() {
        assert_eq!(inset(WindowMode::Tv), UiRect::axes(vw(6.0), vh(6.0)));
        assert_eq!(inset(WindowMode::Desktop), UiRect::all(px(DESKTOP_MARGIN)));
        let safe = safe_area(3840, 2160, WindowMode::Tv);
        assert_eq!(
            (safe.min.x, safe.min.y),
            (3840.0 * 0.06 - 1.0, 2160.0 * 0.06 - 1.0)
        );
    }

    #[test]
    fn the_join_address_drops_the_pairing_key() {
        assert_eq!(
            join_address("http://mac.local:8765/?k=abcdef"),
            "http://mac.local:8765"
        );
        assert_eq!(
            join_address("http://mac.local:8765/#k=ab"),
            "http://mac.local:8765"
        );
        assert_eq!(join_address("http://10.0.0.2:8765"), "http://10.0.0.2:8765");
        assert_eq!(join_address(""), "");
    }

    #[test]
    fn owner_colours_parse_both_store_forms_and_fall_back_to_gold() {
        let owner = |c: &str| PlacementOwner {
            profile_id: "p".into(),
            initial: "A".into(),
            color: c.into(),
        };
        assert_eq!(owner_color(&owner("#ff0000")), Srgba::rgb(1.0, 0.0, 0.0));
        assert_eq!(owner_color(&owner("#00f")), Srgba::rgb(0.0, 0.0, 1.0));
        assert_eq!(owner_color(&owner("teal")), GOLD.to_srgba());
        // Light pips get dark ink, dark ones white.
        assert_eq!(
            ink_on(Srgba::rgb(1.0, 1.0, 0.6)),
            Color::srgb(0.05, 0.05, 0.1)
        );
        assert_eq!(ink_on(Srgba::rgb(0.1, 0.1, 0.5)), Color::WHITE);
    }

    #[test]
    fn every_element_has_a_word() {
        use Element::*;
        let all = [
            Air, Dark, Earth, Fire, Life, Light, Magic, Tech, Undead, Water,
        ];
        let words: Vec<&str> = all.iter().map(|e| element_label(*e)).collect();
        assert_eq!(
            words,
            [
                "Air", "Dark", "Earth", "Fire", "Life", "Light", "Magic", "Tech", "Undead", "Water"
            ]
        );
        assert_ne!(element_color(Some(Fire)), element_color(Some(Water)));
    }

    #[test]
    fn a_click_never_closes_the_join_screen() {
        assert!(!closes(TvScreen::Join, Duration::ZERO, true));
        assert!(!closes(TvScreen::Join, ALREADY_RUNNING_BEAT * 4, false));
    }
}

/// The portal screen laid out headless by `bevy_ui` itself (no window, no
/// GPU): where each card lands at a given window size and mode.
#[cfg(test)]
pub(crate) mod layout_tests {
    use bevy::camera::{ComputedCameraValues, RenderTargetInfo};
    use skylander_core::{FigureId, SlotIndex};

    use super::*;
    use crate::driver::PortalChange;

    struct Laid {
        cards: Vec<(u8, Rect)>,
        empties: Vec<(u8, Rect)>,
        pips: Vec<(u8, Rect)>,
        texts: Vec<Rect>,
    }

    pub(crate) fn placement(name: &str, owner: Option<&str>) -> Placement {
        let figure_id = FigureId::new("000009-0000");
        let (toy_type, variant) = figure_id.tag_parts().unwrap();
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
            }),
        }
    }

    fn lay_out(width: u32, height: u32, mode: WindowMode, cards: PortalCards) -> Laid {
        let mut app = portal_app(width, height, mode, cards);
        measure(&mut app)
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
        ));
        let font = app
            .world_mut()
            .resource_mut::<Assets<Font>>()
            .add(Font::from_bytes(TITAN_ONE.to_vec()));
        app.insert_resource(TvAssets { font, qr: None })
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
            .init_resource::<Portraits>()
            .init_resource::<LayoutReport>()
            .add_systems(Update, rebuild_screen);
        app.world_mut().spawn((
            Camera2d,
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

    fn measure(app: &mut App) -> Laid {
        for _ in 0..3 {
            app.update();
        }
        let world = app.world_mut();
        let mut q = world.query::<(&CardSlot, &ComputedNode, &UiGlobalTransform)>();
        let mut cards: Vec<_> = q
            .iter(world)
            .map(|(s, n, t)| (s.0, node_rect(n, t)))
            .collect();
        cards.sort_by_key(|c| c.0);
        let mut q = world.query::<(&EmptySlot, &ComputedNode, &UiGlobalTransform)>();
        let mut empties: Vec<_> = q
            .iter(world)
            .map(|(s, n, t)| (s.0, node_rect(n, t)))
            .collect();
        empties.sort_by_key(|c| c.0);
        let mut q = world.query::<(&Pip, &ComputedNode, &UiGlobalTransform)>();
        let pips = q
            .iter(world)
            .map(|(s, n, t)| (s.0, node_rect(n, t)))
            .collect();
        let mut q = world.query_filtered::<(&ComputedNode, &UiGlobalTransform), With<Text>>();
        let texts = q.iter(world).map(|(n, t)| node_rect(n, t)).collect();
        Laid {
            cards,
            empties,
            pips,
            texts,
        }
    }

    fn full(name: &str) -> PortalCards {
        PortalCards::default().reduce((0..8).map(|n| PortalChange::Placed {
            slot: SlotIndex::new(n).unwrap(),
            placement: placement(name, Some("#33cc66")),
        }))
    }

    fn assert_eight_visible(width: u32, height: u32, mode: WindowMode, name: &str) {
        assert_eight_visible_under(width, height, mode, name, None);
    }

    fn assert_eight_visible_under(
        width: u32,
        height: u32,
        mode: WindowMode,
        name: &str,
        band: Option<Band>,
    ) {
        let mut app = portal_app(width, height, mode, full(name));
        app.insert_resource(ExtractBand(band));
        let laid = measure(&mut app);
        let safe = safe_area(width, height, mode);
        assert_eq!(laid.cards.len(), 8, "{width}x{height} {mode:?}");
        assert!(laid.empties.is_empty());
        for (slot, r) in &laid.cards {
            assert!(
                r.width() > 0.0 && r.height() > 0.0,
                "slot {slot} has no size"
            );
            assert!(contains(safe, *r), "slot {slot} {r:?} leaves {safe:?}");
        }
        for (i, (_, a)) in laid.cards.iter().enumerate() {
            for (_, b) in &laid.cards[i + 1..] {
                assert!(a.intersect(*b).is_empty(), "{a:?} overlaps {b:?}");
            }
        }
        // Slot order: two rows of four, left to right, top to bottom.
        for (slot, r) in &laid.cards {
            let (row, col) = (
                *slot as usize / CARDS_PER_ROW,
                *slot as usize % CARDS_PER_ROW,
            );
            if col > 0 {
                let left = laid.cards[*slot as usize - 1].1;
                assert!(left.max.x <= r.min.x && (left.center().y - r.center().y).abs() < 1.0);
            }
            if row > 0 {
                let above = laid.cards[*slot as usize - CARDS_PER_ROW].1;
                assert!(above.max.y <= r.min.y);
            }
        }
        // Every pip sits on its own card; all text stays inside the safe area.
        assert_eq!(laid.pips.len(), 8);
        for (slot, pip) in &laid.pips {
            assert!(contains(laid.cards[*slot as usize].1, *pip), "pip {slot}");
        }
        // No two lines of text overlap (the header's title and join address).
        for (i, a) in laid.texts.iter().enumerate() {
            for b in &laid.texts[i + 1..] {
                assert!(a.intersect(*b).is_empty(), "text {a:?} overlaps {b:?}");
            }
        }
        // Text is inside the safe area, and a card's text is inside its card
        // (cards clip, so text crossing an edge would be cut off).
        for t in &laid.texts {
            assert!(contains(safe, *t), "text {t:?} leaves {safe:?}");
            for (slot, card) in &laid.cards {
                let crosses = !card.intersect(*t).is_empty() && !contains(*card, *t);
                assert!(!crosses, "text {t:?} crosses card {slot}'s edge {card:?}");
            }
        }
    }

    #[test]
    fn eight_cards_fit_inside_the_tv_inset_at_1080p_and_4k() {
        assert_eight_visible(1920, 1080, WindowMode::Tv, "Eruptor");
        assert_eight_visible(3840, 2160, WindowMode::Tv, "Eruptor");
    }

    #[test]
    fn eight_cards_fit_the_desktop_window() {
        assert_eight_visible(1280, 720, WindowMode::Desktop, "Eruptor");
    }

    /// The longest band the TV draws: a failure naming two deep folders.
    fn long_failure() -> Band {
        use crate::extraction::{ExtractStatus, Failure, band};
        let deep = "/Users/someone/Library/Application Support/guacamole/extract";
        band(&ExtractStatus::Failed(Failure::InCheckout {
            root: format!("{deep}/and/a/much/deeper/folder/than/anyone/would/use").into(),
            repo: format!("{deep}/and/a/much").into(),
        }))
        .unwrap()
    }

    #[test]
    fn eight_cards_fit_under_the_extraction_band() {
        use crate::extraction::{ExtractStatus, band};
        let progress = band(&ExtractStatus::Converting {
            done: 47,
            total: 48,
            movie: "wreckingball".into(),
        });
        for b in [progress, Some(long_failure())] {
            assert_eight_visible_under(1920, 1080, WindowMode::Tv, "Eruptor", b.clone());
            assert_eight_visible_under(3840, 2160, WindowMode::Tv, "Eruptor", b.clone());
            assert_eight_visible_under(1280, 720, WindowMode::Desktop, "Eruptor", b);
        }
    }

    #[test]
    fn the_band_replaces_the_empty_portal_hint_and_draws_its_bar() {
        use crate::extraction::{ExtractStatus, band};
        let lines = |app: &mut App| {
            let world = app.world_mut();
            let mut q = world.query::<&Text>();
            q.iter(world).map(|t| t.0.clone()).collect::<Vec<_>>()
        };
        let hint = "Put a figure on the portal from your phone.".to_string();
        let mut app = portal_app(1920, 1080, WindowMode::Tv, PortalCards::default());
        measure(&mut app);
        assert!(lines(&mut app).contains(&hint));

        let checking = band(&ExtractStatus::Checking {
            bytes_done: 1,
            bytes_total: 2,
        });
        app.insert_resource(ExtractBand(checking));
        measure(&mut app);
        let drawn = lines(&mut app);
        assert!(!drawn.contains(&hint), "{drawn:?}");
        assert!(drawn.contains(&"Checking your Giants dump… 50%".to_string()));
        let world = app.world_mut();
        let mut bars = world.query::<(&ExtractBar, &ComputedNode)>();
        let (bar, node) = bars.single(world).unwrap();
        assert_eq!(bar.0, 50);
        assert!(node.size().x > 0.0);

        // Ready draws nothing: the hint is back.
        app.insert_resource(ExtractBand(None));
        measure(&mut app);
        assert!(lines(&mut app).contains(&hint));
    }

    #[test]
    fn long_names_wrap_inside_their_cards() {
        assert_eight_visible(1920, 1080, WindowMode::Tv, "Legendary Chop Chop the Undead");
    }

    #[test]
    fn a_removed_card_leaves_the_others_where_they_were() {
        let before = lay_out(1920, 1080, WindowMode::Tv, full("Eruptor"));
        let cleared = full("Eruptor").reduce([PortalChange::Cleared {
            slot: SlotIndex::new(2).unwrap(),
        }]);
        let after = lay_out(1920, 1080, WindowMode::Tv, cleared);
        assert_eq!(after.cards.len(), 7);
        assert_eq!(after.empties.len(), 1);
        assert_eq!(after.empties[0].0, 2);
        for (slot, r) in &after.cards {
            assert_eq!(before.cards[*slot as usize].1, *r, "slot {slot} moved");
        }
        assert_eq!(before.cards[2].1, after.empties[0].1);
    }

    #[test]
    fn the_screen_follows_portal_cards() {
        let one = PortalCards::default().reduce([PortalChange::Placed {
            slot: SlotIndex::new(0).unwrap(),
            placement: placement("Eruptor", Some("#33cc66")),
        }]);
        let mut app = portal_app(1920, 1080, WindowMode::Tv, one.clone());
        assert_eq!(measure(&mut app).cards.len(), 1);
        // A Kaos swap in slot 1 and a placement in slot 6.
        let next = one.reduce([
            PortalChange::Cleared {
                slot: SlotIndex::new(0).unwrap(),
            },
            PortalChange::Placed {
                slot: SlotIndex::new(0).unwrap(),
                placement: placement("Spyro", Some("#33cc66")),
            },
            PortalChange::Placed {
                slot: SlotIndex::new(5).unwrap(),
                placement: placement("Tree Rex", None),
            },
        ]);
        *app.world_mut().resource_mut::<PortalCards>() = next;
        let laid = measure(&mut app);
        let slots: Vec<u8> = laid.cards.iter().map(|c| c.0).collect();
        assert_eq!(slots, [0, 5]);
        assert_eq!(laid.pips.len(), 1);
        let world = app.world_mut();
        let mut names = world.query::<&Text>();
        let names: Vec<String> = names.iter(world).map(|t| t.0.clone()).collect();
        assert!(names.contains(&"Spyro".to_string()) && names.contains(&"Tree Rex".to_string()));
        assert!(!names.contains(&"Eruptor".to_string()));
        // A quit clears the game: back to the join code, no cards.
        app.world_mut().resource_mut::<Current>().screen = TvScreen::Join;
        let laid = measure(&mut app);
        assert!(laid.cards.is_empty() && laid.empties.is_empty());
        assert_eq!(app.world().resource::<LayoutReport>().0, 0);
    }

    #[test]
    fn the_join_screen_stays_inside_the_tv_inset() {
        for (w, h) in [(1920, 1080), (3840, 2160)] {
            let mut app = portal_app(w, h, WindowMode::Tv, PortalCards::default());
            {
                let mut current = app.world_mut().resource_mut::<Current>();
                current.screen = TvScreen::Join;
                current.clients = 1;
            }
            let laid = measure(&mut app);
            let safe = safe_area(w, h, WindowMode::Tv);
            assert!(laid.cards.is_empty());
            for t in &laid.texts {
                assert!(contains(safe, *t), "text {t:?} leaves {safe:?}");
            }
            let world = app.world_mut();
            let mut q = world.query::<&Text>();
            let lines: Vec<String> = q.iter(world).map(|t| t.0.clone()).collect();
            assert!(
                lines.contains(&"1 phone connected".to_string()),
                "{lines:?}"
            );
            assert!(lines.contains(&"http://christophers-macbook-pro.local:8765".to_string()));
        }
    }

    #[test]
    fn a_card_without_an_owner_has_no_pip() {
        let cards = PortalCards::default().reduce([PortalChange::Placed {
            slot: SlotIndex::new(0).unwrap(),
            placement: placement("Eruptor", None),
        }]);
        let laid = lay_out(1920, 1080, WindowMode::Tv, cards);
        assert_eq!(laid.cards.len(), 1);
        assert_eq!(laid.empties.len(), 7);
        assert!(laid.pips.is_empty());
    }
}
