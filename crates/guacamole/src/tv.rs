//! The TV: `ServerBridge` polls the server's status each frame, `Tv` draws
//! the screen it maps to (`screen::screen_for`) in `bevy_ui`. The screen's
//! kind is a `bevy_state` state (openspec AP, design decision 1): each kind
//! spawns its tree on enter under a root that despawns on exit.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use bevy::asset::RenderAssetUsages;
use bevy::ecs::system::SystemParam;
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

/// ServerError's message as drawn.
#[derive(Component)]
pub(crate) struct ErrorMessage;

/// The extraction band's lines and bar fill (percent) as drawn.
#[derive(Component)]
pub(crate) struct ExtractTitle;

#[derive(Component)]
pub(crate) struct ExtractDetail;

#[derive(Component)]
pub(crate) struct ExtractBar(pub(crate) u8);

/// The portal header's game name as drawn.
#[derive(Component)]
pub(crate) struct GameName;

/// Found's list of titles as drawn.
#[derive(Component)]
pub(crate) struct TitlesLine;

/// The column of lines Setup and NoGiants draw from `Setup`, as last drawn.
#[derive(Component)]
struct SetupLines(Vec<Line>);

/// The header's slot under the game name, as last drawn.
#[derive(Component)]
struct BandSlot(BandShape);

/// A setup screen's line, kept so a change can be patched over the one drawn.
#[derive(Debug, Clone, PartialEq)]
struct Line {
    text: String,
    size: f32,
    color: Color,
    /// Wraps at the column's width instead of running off its sides.
    wrap: bool,
}

impl Line {
    fn new(text: impl Into<String>, size: f32, color: Color) -> Self {
        Self {
            text: text.into(),
            size,
            color,
            wrap: false,
        }
    }

    fn wrapping(self) -> Self {
        Self { wrap: true, ..self }
    }
}

/// What sits under the portal's game name.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum BandShape {
    /// Cards are up and nothing's extracting.
    Nothing,
    /// An empty portal: where the figures come from.
    Hint,
    /// The extraction's title and detail, and a bar when it has a percent.
    Band { bar: bool },
}

impl BandShape {
    fn of(band: Option<&Band>, empty: bool) -> Self {
        match band {
            Some(band) => Self::Band {
                bar: band.percent.is_some(),
            },
            None if empty => Self::Hint,
            None => Self::Nothing,
        }
    }
}

/// Marks a screen's root node, spawned when its [`TvState`] is entered and
/// despawned with everything under it when it's left.
#[derive(Component)]
pub(crate) struct ScreenRoot;

/// What the TV just redrew: the layout report and the shots wait on these
/// instead of on the resources behind them, which change without a redraw.
#[derive(Message, Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Redrawn {
    /// A screen was entered and spawned.
    Screen(TvState),
    /// A portal cell's content respawned (0-based slot).
    Slot(u8),
    /// The band under the game's name, or the hint it replaces.
    Band,
    /// A drawn line took new text: the players line, a setup line, the message.
    Text,
}

/// The `Update` systems that redraw part of a screen in place.
#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
struct Redraw;

/// Which screen is up, without its data: [`Current`] keeps that. Set from
/// `screen_for(..)`'s kind, so Portal to Portal (another game) is no transition.
#[derive(States, Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub(crate) enum TvState {
    #[default]
    Starting,
    Setup,
    NoGiants,
    Found,
    Join,
    Portal,
    AlreadyRunning,
    ServerError,
    Farewell,
}

impl TvState {
    const ALL: [Self; 9] = [
        Self::Starting,
        Self::Setup,
        Self::NoGiants,
        Self::Found,
        Self::Join,
        Self::Portal,
        Self::AlreadyRunning,
        Self::ServerError,
        Self::Farewell,
    ];

    pub(crate) fn of(screen: &TvScreen) -> Self {
        match screen {
            TvScreen::Starting => Self::Starting,
            TvScreen::Setup => Self::Setup,
            TvScreen::NoGiants => Self::NoGiants,
            TvScreen::Found => Self::Found,
            TvScreen::Join => Self::Join,
            TvScreen::Portal { .. } => Self::Portal,
            TvScreen::AlreadyRunning => Self::AlreadyRunning,
            TvScreen::ServerError { .. } => Self::ServerError,
            TvScreen::Farewell => Self::Farewell,
        }
    }
}

pub struct ServerBridge;

impl Plugin for ServerBridge {
    fn build(&self, app: &mut App) {
        app.init_resource::<PortalCards>()
            .add_message::<crate::cards::PortalChanged>()
            .add_systems(PreUpdate, (poll_status, drain_portal));
    }
}

/// The screens: the state, each one spawned on enter and kept current
/// inside. [`Tv`] adds it; the headless tests add it alone, on `TvAssets`
/// and the resources `main` inserts.
pub(crate) struct Screens;

impl Plugin for Screens {
    fn build(&self, app: &mut App) {
        app.init_state::<TvState>()
            .init_resource::<Portraits>()
            .init_resource::<LayoutReport>()
            .add_message::<Redrawn>()
            .add_systems(PreUpdate, follow_screen.after(poll_status));
        for state in TvState::ALL {
            app.add_systems(OnEnter(state), enter_screen);
        }
        app.add_systems(
            Update,
            (
                (
                    patch_players,
                    patch_address,
                    patch_screen_text,
                    patch_titles,
                    sync_setup_lines,
                    sync_band,
                    sync_cells,
                )
                    .in_set(Redraw),
                arm_layout_report.after(Redraw),
            ),
        );
    }
}

pub struct Tv;

impl Plugin for Tv {
    fn build(&self, app: &mut App) {
        // Before the first state transition, which spawns the first screen
        // ahead of `Startup`.
        let assets = tv_assets(app.world_mut());
        app.insert_resource(assets)
            .add_plugins(Screens)
            .add_systems(Startup, spawn_camera)
            .add_systems(Last, report_layout)
            .add_systems(Update, (scale_ui, farewell_exit, escape_quits));
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

/// The font and the join QR, on the world `main` built (`ServerLink` in it).
fn tv_assets(world: &mut World) -> TvAssets {
    let font = world
        .resource_mut::<Assets<Font>>()
        .add(Font::from_bytes(TITAN_ONE.to_vec()));
    let png = world
        .get_resource::<ServerLink>()
        .expect("main inserts ServerLink before the Tv plugin")
        .join_qr_png
        .clone();
    // A failed boot has no QR; nothing to decode.
    let qr = match (!png.is_empty()).then(|| {
        Image::from_buffer(
            &png,
            ImageType::Extension("png"),
            CompressedImageFormats::NONE,
            true,
            ImageSampler::nearest(),
            RenderAssetUsages::default(),
        )
    }) {
        Some(Ok(image)) => Some(world.resource_mut::<Assets<Image>>().add(image)),
        Some(Err(e)) => {
            error!("decode the join QR: {e}");
            None
        }
        None => None,
    };
    TvAssets { font, qr }
}

fn spawn_camera(mut commands: Commands) {
    commands.spawn(Camera2d);
}

/// The state follows `Current`'s screen, which `poll_status` sets from
/// `screen_for`; the transition lands this frame, right after `PreUpdate`.
fn follow_screen(current: Option<Res<Current>>, mut next: ResMut<NextState<TvState>>) {
    if let Some(current) = current.filter(|c| c.is_changed()) {
        next.set_if_different(TvState::of(&current.screen));
    }
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

/// Every resource a screen reads, as one parameter for the systems that
/// spawn and refresh screens.
#[derive(SystemParam)]
struct Sources<'w> {
    assets: Res<'w, TvAssets>,
    link: Res<'w, ServerLink>,
    setup: Res<'w, Setup>,
    framing: Res<'w, Framing>,
    /// None before the first `poll_status`, while Starting is up.
    current: Option<Res<'w, Current>>,
    cards: Res<'w, PortalCards>,
    band: Option<Res<'w, ExtractBand>>,
    portraits: ResMut<'w, Portraits>,
    images: ResMut<'w, Assets<Image>>,
}

impl Sources<'_> {
    fn clients(&self) -> usize {
        self.current.as_ref().map_or(0, |c| c.clients)
    }

    fn screen(&self) -> Option<&TvScreen> {
        self.current.as_ref().map(|c| &c.screen)
    }
}

/// `OnEnter` every state: the screen's root and everything under it. The
/// root goes on exit (`DespawnOnExit`), its tree with it.
fn enter_screen(
    mut commands: Commands,
    state: Res<State<TvState>>,
    mut sources: Sources,
    mut redrawn: MessageWriter<Redrawn>,
) {
    let state = *state.get();
    commands
        .spawn((
            ScreenRoot,
            DespawnOnExit(state),
            root_node(state, sources.framing.0),
            BackgroundColor(BACKGROUND),
        ))
        .with_children(|ui| spawn_content(ui, state, &mut sources));
    redrawn.write(Redrawn::Screen(state));
}

/// The players line (Join and the portal header) follows `Current.clients`.
fn patch_players(
    current: Option<Res<Current>>,
    mut lines: Query<&mut Text, With<PlayersLine>>,
    redrawn: MessageWriter<Redrawn>,
) {
    let Some(current) = current.filter(|c| c.is_changed()) else {
        return;
    };
    patch(lines.iter_mut(), &players_line(current.clients), redrawn);
}

/// Sets each line to `drawn`, telling [`Redrawn`] once if any of them moved.
fn patch<'a>(
    lines: impl Iterator<Item = Mut<'a, Text>>,
    drawn: &str,
    mut redrawn: MessageWriter<Redrawn>,
) {
    let mut changed = false;
    for mut line in lines {
        changed |= line.set_if_neq(Text(drawn.to_string()));
    }
    if changed {
        redrawn.write(Redrawn::Text);
    }
}

/// The join address (Join and the portal header) follows the phone URL.
fn patch_address(
    link: Res<ServerLink>,
    mut lines: Query<&mut Text, With<JoinAddress>>,
    redrawn: MessageWriter<Redrawn>,
) {
    if link.is_changed() {
        patch(lines.iter_mut(), join_address(&link.phone_url), redrawn);
    }
}

/// The portal's game name and ServerError's message follow `Current`'s
/// screen: another game or another error is no transition.
fn patch_screen_text(
    current: Option<Res<Current>>,
    mut games: Query<&mut Text, (With<GameName>, Without<ErrorMessage>)>,
    mut messages: Query<&mut Text, (With<ErrorMessage>, Without<GameName>)>,
    redrawn: MessageWriter<Redrawn>,
) {
    let Some(current) = current.filter(|c| c.is_changed()) else {
        return;
    };
    match &current.screen {
        TvScreen::Portal { game } => patch(games.iter_mut(), game, redrawn),
        TvScreen::ServerError { message } => patch(messages.iter_mut(), message, redrawn),
        _ => {}
    }
}

fn patch_titles(
    setup: Res<Setup>,
    mut lines: Query<&mut Text, With<TitlesLine>>,
    redrawn: MessageWriter<Redrawn>,
) {
    if setup.is_changed() {
        patch(lines.iter_mut(), &setup.0.titles_line(), redrawn);
    }
}

/// Setup's and NoGiants' lines follow `Setup` (the picker opening or
/// closing, a pick that failed, another folder): patched in place while
/// their count and wrapping hold, respawned when they don't.
fn sync_setup_lines(
    mut commands: Commands,
    setup: Res<Setup>,
    state: Res<State<TvState>>,
    assets: Res<TvAssets>,
    mut columns: Query<(Entity, &mut SetupLines, Option<&Children>)>,
    mut texts: Query<(&mut Text, &mut TextFont, &mut TextColor)>,
    mut redrawn: MessageWriter<Redrawn>,
) {
    if !setup.is_changed() {
        return;
    }
    let lines = setup_lines(*state.get(), &setup.0);
    for (column, mut drawn, children) in &mut columns {
        if drawn.0 == lines {
            continue;
        }
        let same_shape = drawn.0.len() == lines.len()
            && drawn.0.iter().zip(&lines).all(|(a, b)| a.wrap == b.wrap);
        match children.filter(|_| same_shape) {
            Some(children) => {
                for (kid, line) in children.iter().zip(&lines) {
                    let Ok((mut text, mut font, mut color)) = texts.get_mut(kid) else {
                        continue;
                    };
                    text.set_if_neq(Text(line.text.clone()));
                    color.set_if_neq(TextColor(line.color));
                    if font.font_size != FontSize::Px(line.size) {
                        font.font_size = FontSize::Px(line.size);
                    }
                }
            }
            None => {
                commands
                    .entity(column)
                    .despawn_children()
                    .with_children(|column| {
                        for line in &lines {
                            column.spawn(line_bundle(&assets.font, line));
                        }
                    });
            }
        }
        drawn.0 = lines.clone();
        redrawn.write(Redrawn::Text);
    }
}

/// The header's slot follows `ExtractBand`, and the hint the cards: a tick
/// patches the band's title, detail and bar; a change of shape (the band
/// coming or going, a bar appearing) respawns the slot's content.
fn sync_band(
    mut commands: Commands,
    band: Option<Res<ExtractBand>>,
    cards: Res<PortalCards>,
    assets: Res<TvAssets>,
    mut slots: Query<(Entity, &mut BandSlot, &mut Node)>,
    mut titles: Query<&mut Text, (With<ExtractTitle>, Without<ExtractDetail>)>,
    mut details: Query<(&mut Text, &mut TextColor), (With<ExtractDetail>, Without<ExtractTitle>)>,
    mut bars: Query<(&mut ExtractBar, &mut Node), Without<BandSlot>>,
    mut redrawn: MessageWriter<Redrawn>,
) {
    let band_changed = band.as_ref().is_some_and(|b| b.is_changed());
    if !band_changed && !cards.is_changed() {
        return;
    }
    let band = band.as_ref().and_then(|b| b.0.as_ref());
    let shape = BandShape::of(band, cards.cards().next().is_none());
    for (slot, mut drawn, mut node) in &mut slots {
        if drawn.0 != shape {
            node.display = band_slot_display(shape);
            let font = assets.font.clone();
            commands
                .entity(slot)
                .despawn_children()
                .with_children(|ui| {
                    spawn_band_slot(ui, &font, shape, band);
                });
            drawn.0 = shape;
            redrawn.write(Redrawn::Band);
            continue;
        }
        let Some(band) = band.filter(|_| band_changed) else {
            continue;
        };
        redrawn.write(Redrawn::Band);
        for mut title in &mut titles {
            title.set_if_neq(Text(band.title.clone()));
        }
        for (mut detail, mut color) in &mut details {
            detail.set_if_neq(Text(band.detail.clone()));
            color.set_if_neq(TextColor(detail_color(band)));
        }
        if let Some(pct) = band.percent {
            for (mut bar, mut node) in &mut bars {
                if bar.0 != pct {
                    bar.0 = pct;
                    node.width = percent(f32::from(pct));
                }
            }
        }
    }
}

/// Each of the portal's eight cells follows its slot in `PortalCards`: one
/// whose placement changed respawns its own content, the other seven and the
/// header keep theirs.
fn sync_cells(
    mut commands: Commands,
    cards: Res<PortalCards>,
    assets: Res<TvAssets>,
    mut portraits: ResMut<Portraits>,
    mut images: ResMut<Assets<Image>>,
    mut cells: Query<(Entity, &Cell, &mut Drew)>,
    mut redrawn: MessageWriter<Redrawn>,
) {
    if !cards.is_changed() {
        return;
    }
    let text =
        |s: &str, size: f32, color: Color| label(&assets.font, s, size, color, Justify::Center);
    for (entity, cell, mut drew) in &mut cells {
        let now = &cards.slots[usize::from(cell.0)];
        if drew.0 == *now {
            continue;
        }
        let portrait = portrait_of(now.as_ref(), &mut portraits, &mut images);
        commands
            .entity(entity)
            .despawn_children()
            .with_children(|content| {
                spawn_slot(content, &text, cell.0, now.as_ref(), portrait);
            });
        drew.0 = now.clone();
        redrawn.write(Redrawn::Slot(cell.0));
    }
}

/// The layout report waits a few frames after the portal's cards or band
/// were redrawn; any other screen has no cards to report.
fn arm_layout_report(mut redrawn: MessageReader<Redrawn>, mut report: ResMut<LayoutReport>) {
    for redrawn in redrawn.read() {
        match redrawn {
            Redrawn::Screen(TvState::Portal) | Redrawn::Slot(_) | Redrawn::Band => {
                report.0 = REPORT_AFTER_FRAMES;
            }
            Redrawn::Screen(_) => report.0 = 0,
            Redrawn::Text => {}
        }
    }
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
fn picker_lines(setup: &SetupView, choose: &str) -> Vec<Line> {
    let mut lines = Vec::new();
    if let Some(error) = &setup.error {
        lines.push(Line::new(
            format!("Couldn't use that folder: {error}"),
            32.0,
            GOLD,
        ));
    }
    if setup.picking {
        lines.push(Line::new(
            "Choose a folder in the window that opened.",
            40.0,
            WHITE,
        ));
    } else {
        lines.push(Line::new(
            format!("Press Enter or click to {choose}."),
            40.0,
            WHITE,
        ));
    }
    lines.push(Line::new("Or run: guacamole --setup <folder>", 28.0, DIM));
    lines
}

/// The lines under Setup's and NoGiants' headings, all from `Setup`.
fn setup_lines(state: TvState, setup: &SetupView) -> Vec<Line> {
    match state {
        TvState::Setup => picker_lines(setup, "choose it"),
        TvState::NoGiants => {
            let mut lines = vec![Line::new(
                format!("Looked in: {}", setup.folders.join(", ")),
                32.0,
                DIM,
            )];
            for problem in &setup.problems {
                lines.push(Line::new(problem.clone(), 32.0, GOLD));
            }
            if !setup.found_titles.is_empty() {
                lines.push(Line::new("Found:", 40.0, WHITE));
                lines.push(Line::new(setup.titles_line(), 36.0, WHITE).wrapping());
            } else if setup.problems.is_empty() {
                lines.push(Line::new("No game dumps there.", 40.0, WHITE));
            }
            lines.extend(picker_lines(setup, "choose another folder"));
            lines
        }
        _ => Vec::new(),
    }
}

/// A centred line; a wrapping one is held to the column's width.
fn line_bundle(font: &Handle<Font>, line: &Line) -> impl Bundle + use<> {
    (
        label(font, &line.text, line.size, line.color, Justify::Center),
        Node {
            max_width: if line.wrap { percent(100) } else { Val::Auto },
            ..default()
        },
    )
}

fn label(
    font: &Handle<Font>,
    s: &str,
    size: f32,
    color: Color,
    justify: Justify,
) -> impl Bundle + use<> {
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
}

/// The band's detail: white when it's a failure to act on, dim for progress.
fn detail_color(band: &Band) -> Color {
    if band.code.is_some() { WHITE } else { DIM }
}

/// An empty slot leaves the layout, so its row gap doesn't move the name.
fn band_slot_display(shape: BandShape) -> Display {
    if shape == BandShape::Nothing {
        Display::None
    } else {
        Display::Flex
    }
}

/// The hint or the band, under the game's name.
fn spawn_band_slot(
    slot: &mut ChildSpawnerCommands,
    font: &Handle<Font>,
    shape: BandShape,
    band: Option<&Band>,
) {
    let text_at =
        |s: &str, size: f32, color: Color, justify: Justify| label(font, s, size, color, justify);
    match (shape, band) {
        (BandShape::Hint, _) => {
            slot.spawn(text_at(
                "Put a figure on the portal from your phone.",
                32.0,
                WHITE,
                Justify::Left,
            ));
        }
        (BandShape::Band { .. }, Some(band)) => spawn_band(slot, &text_at, band),
        _ => {}
    }
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

/// A screen's root: a centred column, or the portal's header over its grid.
fn root_node(state: TvState, mode: WindowMode) -> Node {
    if state == TvState::Portal {
        return Node {
            width: percent(100),
            height: percent(100),
            flex_direction: FlexDirection::Column,
            row_gap: px(24),
            padding: inset(mode),
            ..default()
        };
    }
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
    }
}

/// Setup's and NoGiants' lines in a full-width column with the screen's own
/// gap, so they lay out as the screen's own children would.
fn spawn_setup_lines(ui: &mut ChildSpawnerCommands, font: &Handle<Font>, lines: Vec<Line>) {
    ui.spawn((
        Node {
            width: percent(100),
            flex_direction: FlexDirection::Column,
            align_items: AlignItems::Center,
            row_gap: px(28),
            ..default()
        },
        SetupLines(lines.clone()),
    ))
    .with_children(|column| {
        for line in &lines {
            column.spawn(line_bundle(font, line));
        }
    });
}

/// Every screen's lines; the portal's header and grid come from [`spawn_portal`].
fn spawn_content(ui: &mut ChildSpawnerCommands, state: TvState, sources: &mut Sources) {
    if state == TvState::Portal {
        spawn_portal(ui, sources);
        return;
    }
    let setup = &sources.setup.0;
    let font = sources.assets.font.clone();
    let text = |s: &str, size: f32, color: Color| label(&font, s, size, color, Justify::Center);

    match state {
        TvState::Starting => {
            ui.spawn(text("Starting…", 64.0, DIM));
        }
        TvState::Setup => {
            ui.spawn(text("Where are your disc dumps?", 88.0, GOLD));
            ui.spawn(text(
                "Choose the folder that holds your Skylanders Giants dump.",
                40.0,
                WHITE,
            ));
            spawn_setup_lines(ui, &font, setup_lines(state, setup));
        }
        TvState::NoGiants => {
            ui.spawn(text("No Skylanders Giants dump found", 80.0, GOLD));
            spawn_setup_lines(ui, &font, setup_lines(state, setup));
        }
        TvState::Found => {
            ui.spawn(text("Found your games", 88.0, GOLD));
            let titles = Line::new(setup.titles_line(), 36.0, WHITE).wrapping();
            ui.spawn((TitlesLine, line_bundle(&font, &titles)));
            ui.spawn(text("Skylanders: Giants is ready!", 56.0, GOLD));
            ui.spawn(text("Click to continue.", 32.0, DIM));
        }
        TvState::Join => {
            ui.spawn(text("Scan to play", 88.0, GOLD));
            if let Some(qr) = &sources.assets.qr {
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
            let address = join_address(&sources.link.phone_url);
            ui.spawn((JoinAddress, text(address, 32.0, DIM)));
            let players = players_line(sources.clients());
            ui.spawn((PlayersLine, text(&players, 40.0, WHITE)));
        }
        // Handled above.
        TvState::Portal => {}
        TvState::AlreadyRunning => {
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
        TvState::ServerError => {
            let message = match sources.screen() {
                Some(TvScreen::ServerError { message }) => message.as_str(),
                _ => "",
            };
            ui.spawn(text("Something went wrong", 88.0, GOLD));
            // The whole cause chain, wrapped inside the column; a path with
            // no space to break at breaks between characters.
            ui.spawn((
                ErrorMessage,
                Text::new(message),
                TextFont {
                    font: font.clone().into(),
                    font_size: FontSize::Px(36.0),
                    ..default()
                },
                TextColor(WHITE),
                TextLayout::new(Justify::Center, LineBreak::WordOrCharacter),
                Node {
                    max_width: percent(100),
                    ..default()
                },
            ));
            ui.spawn(text("Press Esc or click to close.", 32.0, DIM));
        }
        TvState::Farewell => {
            ui.spawn(text("Goodbye, Portal Master!", 96.0, GOLD));
        }
    }
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

/// A slot's place in the grid (0-based), kept as long as the portal is up;
/// its one child is the card or the empty frame.
#[derive(Component, Debug)]
pub(crate) struct Cell(pub(crate) u8);

/// The placement a cell drew, none for its empty frame.
#[derive(Component)]
struct Drew(Option<Placement>);

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

/// Frames until `report_layout` logs a redrawn portal: layout and text
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
fn spawn_portal(ui: &mut ChildSpawnerCommands, sources: &mut Sources) {
    let font = sources.assets.font.clone();
    let text_at =
        |s: &str, size: f32, color: Color, justify: Justify| label(&font, s, size, color, justify);
    let game = match sources.screen() {
        Some(TvScreen::Portal { game }) => game.clone(),
        _ => String::new(),
    };
    let players = players_line(sources.clients());
    let address = join_address(&sources.link.phone_url).to_string();
    let qr = sources.assets.qr.clone();
    let band = sources.band.as_ref().and_then(|b| b.0.clone());
    let shape = BandShape::of(band.as_ref(), sources.cards.cards().next().is_none());

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
                left.spawn((GameName, text_at(&game, 64.0, GOLD, Justify::Left)));
                left.spawn((
                    BandSlot(shape),
                    Node {
                        display: band_slot_display(shape),
                        width: percent(100),
                        flex_direction: FlexDirection::Column,
                        align_items: AlignItems::FlexStart,
                        row_gap: px(8),
                        ..default()
                    },
                ))
                .with_children(|slot| spawn_band_slot(slot, &font, shape, band.as_ref()));
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
                        join.spawn((PlayersLine, text_at(&players, 28.0, DIM, Justify::Right)));
                        join.spawn((JoinAddress, text_at(&address, 24.0, DIM, Justify::Right)));
                    });
                if let Some(qr) = &qr {
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
    .with_children(|grid| spawn_rows(grid, &font, sources));
}

/// The grid's rows of cells, each holding its slot's card or empty frame.
fn spawn_rows(grid: &mut ChildSpawnerCommands, font: &Handle<Font>, sources: &mut Sources) {
    let text = |s: &str, size: f32, color: Color| label(font, s, size, color, Justify::Center);
    let Sources {
        cards,
        portraits,
        images,
        ..
    } = sources;
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
                let placement = cards.slots[slot].as_ref();
                let portrait = portrait_of(placement, portraits, images);
                cells
                    .spawn((
                        Cell(slot as u8),
                        Drew(placement.cloned()),
                        // An equal share of the row; the content fills it.
                        Node {
                            flex_grow: 1.0,
                            flex_basis: px(0),
                            min_width: px(0),
                            min_height: px(0),
                            flex_direction: FlexDirection::Column,
                            ..default()
                        },
                    ))
                    .with_children(|cell| {
                        spawn_slot(cell, &text, slot as u8, placement, portrait);
                    });
            }
        });
    }
}

/// The portrait drawn for a placement, decoded on first use.
fn portrait_of(
    placement: Option<&Placement>,
    portraits: &mut Portraits,
    images: &mut Assets<Image>,
) -> Option<Handle<Image>> {
    placement?
        .portrait
        .as_deref()
        .and_then(|p| portraits.get(p, images))
}

/// A cell's content: the card, or the numbered empty frame.
fn spawn_slot<T: Bundle>(
    cell: &mut ChildSpawnerCommands,
    text: &impl Fn(&str, f32, Color) -> T,
    slot: u8,
    placement: Option<&Placement>,
    portrait: Option<Handle<Image>>,
) {
    match placement {
        Some(placement) => spawn_card(cell, text, slot, placement, portrait),
        None => spawn_empty(cell, text, slot),
    }
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
    left.spawn((
        ExtractDetail,
        text_at(&band.detail, 22.0, detail_color(band), Justify::Left),
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

/// A cell's content: fills the cell and clips anything that doesn't fit.
fn slot_content() -> Node {
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
    cell: &mut ChildSpawnerCommands,
    text: &impl Fn(&str, f32, Color) -> T,
    slot: u8,
    placement: &Placement,
    portrait: Option<Handle<Image>>,
) {
    let edge = element_color(placement.element);
    cell.spawn((
        CardSlot(slot),
        slot_content(),
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
    cell: &mut ChildSpawnerCommands,
    text: &impl Fn(&str, f32, Color) -> T,
    slot: u8,
) {
    cell.spawn((
        EmptySlot(slot),
        slot_content(),
        BorderColor::all(EMPTY_EDGE),
    ))
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
            bevy::state::app::StatesPlugin,
            bevy::input_focus::InputFocusPlugin,
            crate::input::InputRouter,
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
            .add_plugins(Screens);
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

    /// AA.21: a nested startup error reaches the screen whole, wrapped
    /// inside the inset, including a deep path and a SHA-384 checksum
    /// (96 hex digits, nowhere to break between words).
    #[test]
    fn a_server_error_draws_every_cause_wrapped_inside_the_inset() {
        let deep = "/Users/someone/Library/Application Support/guacamole/and/a/much/deeper/folder/than/anyone/would/use/db.sqlite";
        let causes = [
            "open profile store".to_string(),
            format!("open sqlite at {deep}"),
            format!(
                "migration 20261017000000 checksum {} does not match",
                "0123456789abcdef".repeat(6)
            ),
        ];
        // Formatted as boot formats a fatal error: the scope, then `{:#}`.
        let chain = anyhow::anyhow!("{}", causes[2]).context(causes[1].clone());
        let message = format!("{}: {chain:#}", causes[0]);
        for (w, h, mode) in [
            (1920, 1080, WindowMode::Tv),
            (3840, 2160, WindowMode::Tv),
            (1280, 720, WindowMode::Desktop),
        ] {
            let mut app = portal_app(w, h, mode, PortalCards::default());
            let status = LauncherStatus {
                screen: guac_server::state::LauncherScreen::ServerError {
                    message: message.clone(),
                },
                ..Default::default()
            };
            app.world_mut().resource_mut::<Current>().screen =
                screen_for(&status, &SetupView::default());
            let laid = measure(&mut app);
            let safe = safe_area(w, h, mode);
            let world = app.world_mut();
            let mut q = world
                .query_filtered::<(&Text, &ComputedNode, &UiGlobalTransform), With<ErrorMessage>>();
            let (drawn, node, t) = q.single(world).unwrap();
            for cause in &causes {
                assert!(
                    drawn.0.contains(cause.as_str()),
                    "{cause} not in {:?}",
                    drawn.0
                );
            }
            let rect = node_rect(node, t);
            let line = 36.0 * h as f32 / DESIGN_HEIGHT;
            assert!(rect.height() > 2.0 * line, "{w}x{h}: one line, {rect:?}");
            assert!(contains(safe, rect), "{w}x{h}: {rect:?} leaves {safe:?}");
            assert_eq!(laid.texts.len(), 3);
            for (i, a) in laid.texts.iter().enumerate() {
                assert!(contains(safe, *a), "{w}x{h}: {a:?} leaves {safe:?}");
                for b in &laid.texts[i + 1..] {
                    assert!(a.intersect(*b).is_empty(), "{a:?} overlaps {b:?}");
                }
            }
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

    /// The screen's roots: (entity, the state it leaves with).
    fn roots(app: &mut App) -> Vec<(Entity, TvState)> {
        let world = app.world_mut();
        let mut q = world.query_filtered::<(Entity, &DespawnOnExit<TvState>), With<ScreenRoot>>();
        q.iter(world).map(|(e, s)| (e, s.0)).collect()
    }

    fn texts(app: &mut App) -> Vec<String> {
        let world = app.world_mut();
        let mut q = world.query::<&Text>();
        q.iter(world).map(|t| t.0.clone()).collect()
    }

    /// AP.1: each step enters its screen and leaves the last one, tree and
    /// all; a new game on the portal is no transition.
    #[test]
    fn each_screen_step_leaves_exactly_one_root() {
        let giants = || TvScreen::Portal {
            game: "Skylanders: Giants".into(),
        };
        let mut app = portal_app(1920, 1080, WindowMode::Tv, full("Eruptor"));
        let steps = [
            (TvScreen::Starting, TvState::Starting, "Starting…"),
            (
                TvScreen::Setup,
                TvState::Setup,
                "Where are your disc dumps?",
            ),
            (TvScreen::Found, TvState::Found, "Found your games"),
            (TvScreen::Join, TvState::Join, "Scan to play"),
            (giants(), TvState::Portal, "Skylanders: Giants"),
            (
                TvScreen::Farewell,
                TvState::Farewell,
                "Goodbye, Portal Master!",
            ),
        ];
        for (screen, state, line) in steps {
            app.world_mut().resource_mut::<Current>().screen = screen;
            app.update();
            let found = roots(&mut app);
            assert_eq!(found.len(), 1, "{state:?}: {found:?}");
            assert_eq!(found[0].1, state);
            assert_eq!(*app.world().resource::<State<TvState>>().get(), state);
            assert!(texts(&mut app).contains(&line.to_string()), "{state:?}");
            let cards = {
                let world = app.world_mut();
                world.query::<&CardSlot>().iter(world).count()
            };
            assert_eq!(cards, if state == TvState::Portal { 8 } else { 0 });
        }

        // Portal to Portal keeps the root and draws the new game.
        app.world_mut().resource_mut::<Current>().screen = giants();
        app.update();
        let before = roots(&mut app);
        app.world_mut().resource_mut::<Current>().screen = TvScreen::Portal {
            game: "Skylanders: Swap Force".into(),
        };
        app.update();
        assert_eq!(roots(&mut app), before);
        let drawn = texts(&mut app);
        assert!(
            drawn.contains(&"Skylanders: Swap Force".to_string()),
            "{drawn:?}"
        );
        assert!(!drawn.contains(&"Skylanders: Giants".to_string()));
    }

    fn every_screen() -> [TvScreen; 9] {
        [
            TvScreen::Starting,
            TvScreen::Setup,
            TvScreen::NoGiants,
            TvScreen::Found,
            TvScreen::Join,
            TvScreen::Portal {
                game: "Skylanders: Giants".into(),
            },
            TvScreen::AlreadyRunning,
            TvScreen::ServerError {
                message: "open profile store".into(),
            },
            TvScreen::Farewell,
        ]
    }

    /// Every UI node, and the parentless ones: a screen root or a stray.
    fn nodes(app: &mut App) -> (std::collections::BTreeSet<Entity>, Vec<Entity>) {
        let world = app.world_mut();
        let mut q = world.query_filtered::<(Entity, Has<ChildOf>), With<Node>>();
        let mut all = std::collections::BTreeSet::new();
        let mut parentless = Vec::new();
        for (entity, parented) in q.iter(world) {
            all.insert(entity);
            if !parented {
                parentless.push(entity);
            }
        }
        (all, parentless)
    }

    /// One root carrying `state` and no other parentless node; the node count.
    fn one_root(app: &mut App, state: TvState, step: &str) -> usize {
        let found = roots(app);
        assert_eq!(found.len(), 1, "{step}: {found:?}");
        assert_eq!(found[0].1, state, "{step}");
        assert_eq!(*app.world().resource::<State<TvState>>().get(), state);
        let (all, parentless) = nodes(app);
        assert_eq!(parentless, [found[0].0], "{step}: a stray node");
        all.len()
    }

    /// AP.1 review: every ordered pair of the nine screens, three times over,
    /// leaves exactly the new screen's root and nothing stray beside it, and a
    /// screen comes back with the node count it had; `Current` going away and
    /// back between two frames is no transition.
    #[test]
    fn every_pair_of_screens_leaves_one_root_and_no_strays() {
        let mut app = portal_app(1920, 1080, WindowMode::Tv, full("Eruptor"));
        app.world_mut().resource_mut::<Setup>().0.error = Some("gone".into());
        let mut counts: HashMap<TvState, usize> = HashMap::new();
        for round in 0..3 {
            for from in every_screen() {
                for to in every_screen() {
                    for screen in [from.clone(), to.clone()] {
                        let state = TvState::of(&screen);
                        app.world_mut().resource_mut::<Current>().screen = screen;
                        app.update();
                        let step = format!("round {round}, {from:?} to {to:?}");
                        let count = one_root(&mut app, state, &step);
                        assert_eq!(*counts.entry(state).or_insert(count), count, "{step}");
                    }
                }
            }
        }

        let before = (roots(&mut app), nodes(&mut app).0);
        let farewell = app.world().resource::<Current>().screen.clone();
        app.world_mut().resource_mut::<Current>().screen = TvScreen::Join;
        app.world_mut().resource_mut::<Current>().screen = farewell;
        app.update();
        assert_eq!((roots(&mut app), nodes(&mut app).0), before);
    }

    /// AP.2 review: a source written with nothing new respawns nothing, on
    /// any screen.
    #[test]
    fn a_touched_source_with_nothing_new_respawns_nothing() {
        use crate::extraction::{ExtractStatus, band};
        let mut app = portal_app(1920, 1080, WindowMode::Tv, full("Eruptor"));
        app.insert_resource(ExtractBand(band(&ExtractStatus::Converting {
            done: 3,
            total: 48,
            movie: "eruptor".into(),
        })));
        for screen in every_screen() {
            app.world_mut().resource_mut::<Current>().screen = screen.clone();
            frames(&mut app, 3);
            let (before, _) = nodes(&mut app);
            let world = app.world_mut();
            world.resource_mut::<Current>().set_changed();
            world.resource_mut::<Setup>().set_changed();
            world.resource_mut::<PortalCards>().set_changed();
            world.resource_mut::<ExtractBand>().set_changed();
            frames(&mut app, 3);
            assert_eq!(nodes(&mut app).0, before, "{screen:?}");
        }
    }

    /// The one entity carrying `M`, and its text.
    fn marked<M: Component>(app: &mut App) -> (Entity, String) {
        let world = app.world_mut();
        let mut q = world.query_filtered::<(Entity, &Text), With<M>>();
        let (entity, text) = q.single(world).unwrap();
        (entity, text.0.clone())
    }

    fn frames(app: &mut App, n: usize) {
        for _ in 0..n {
            app.update();
        }
    }

    /// AP.2: a phone joining patches the drawn line, on Join and on the portal.
    #[test]
    fn a_new_phone_patches_the_players_line_in_place() {
        let mut app = portal_app(1920, 1080, WindowMode::Tv, PortalCards::default());
        {
            let mut current = app.world_mut().resource_mut::<Current>();
            current.screen = TvScreen::Join;
            current.clients = 1;
        }
        frames(&mut app, 3);
        for screen in [
            TvScreen::Join,
            TvScreen::Portal {
                game: "Skylanders: Giants".into(),
            },
        ] {
            app.world_mut().resource_mut::<Current>().screen = screen.clone();
            app.world_mut().resource_mut::<Current>().clients = 1;
            frames(&mut app, 2);
            let (line, text) = marked::<PlayersLine>(&mut app);
            let (address, _) = marked::<JoinAddress>(&mut app);
            assert_eq!(text, "1 phone connected", "{screen:?}");
            app.world_mut().resource_mut::<Current>().clients = 2;
            app.update();
            assert_eq!(
                marked::<PlayersLine>(&mut app),
                (line, "2 phones connected".to_string()),
                "{screen:?}"
            );
            assert_eq!(marked::<JoinAddress>(&mut app).0, address);
        }
    }

    /// AP.2: a tick of the extraction patches the band; a failure, which
    /// has no bar, respawns it.
    #[test]
    fn a_band_tick_keeps_its_lines_and_bar() {
        use crate::extraction::{ExtractStatus, Failure, band};
        let converting = |done| {
            band(&ExtractStatus::Converting {
                done,
                total: 48,
                movie: "eruptor".into(),
            })
        };
        let mut app = portal_app(1920, 1080, WindowMode::Tv, PortalCards::default());
        app.insert_resource(ExtractBand(converting(12)));
        frames(&mut app, 3);
        let bar = |app: &mut App| {
            let world = app.world_mut();
            let mut q = world.query::<(Entity, &ExtractBar, &Node)>();
            let (entity, bar, node) = q.single(world).unwrap();
            (entity, bar.0, node.width)
        };
        let (title, _) = marked::<ExtractTitle>(&mut app);
        let (detail, text) = marked::<ExtractDetail>(&mut app);
        let (fill, pct, width) = bar(&mut app);
        assert_eq!(
            (text.as_str(), pct, width),
            ("Converting intro 13 of 48…", 25, percent(25))
        );

        app.insert_resource(ExtractBand(converting(13)));
        app.update();
        assert_eq!(marked::<ExtractTitle>(&mut app).0, title);
        assert_eq!(
            marked::<ExtractDetail>(&mut app),
            (detail, "Converting intro 14 of 48…".to_string())
        );
        assert_eq!(bar(&mut app), (fill, 27, percent(27)));

        app.insert_resource(ExtractBand(band(&ExtractStatus::Failed(
            Failure::NoFfmpeg {
                configured: None,
                install: "brew install ffmpeg".into(),
            },
        ))));
        app.update();
        let (failed, text) = marked::<ExtractTitle>(&mut app);
        assert_ne!(failed, title);
        assert_eq!(text, "No intros: ffmpeg is needed");
        let world = app.world_mut();
        assert_eq!(world.query::<&ExtractBar>().iter(world).count(), 0);
    }

    /// AP.2: the picker opening patches Setup's lines; a failed pick adds a
    /// line, so the column respawns with it.
    #[test]
    fn the_setup_lines_follow_the_picker() {
        let mut app = portal_app(1920, 1080, WindowMode::Tv, PortalCards::default());
        app.world_mut().resource_mut::<Current>().screen = TvScreen::Setup;
        frames(&mut app, 2);
        let lines = |app: &mut App| {
            let world = app.world_mut();
            let mut columns = world.query_filtered::<&Children, With<SetupLines>>();
            let kids: Vec<Entity> = columns.single(world).unwrap().iter().collect();
            let mut texts = world.query::<&Text>();
            kids.into_iter()
                .map(|kid| (kid, texts.get(world, kid).unwrap().0.clone()))
                .collect::<Vec<_>>()
        };
        let before = lines(&mut app);
        assert_eq!(before[0].1, "Press Enter or click to choose it.");

        app.world_mut().resource_mut::<Setup>().0.picking = true;
        app.update();
        let picking = lines(&mut app);
        assert_eq!(picking.len(), before.len());
        assert_eq!(
            picking[0],
            (
                before[0].0,
                "Choose a folder in the window that opened.".into()
            )
        );
        assert_eq!(picking[1], before[1]);

        {
            let mut setup = app.world_mut().resource_mut::<Setup>();
            setup.0.picking = false;
            setup.0.error = Some("/Volumes/gone isn't a folder".into());
        }
        app.update();
        let failed: Vec<String> = lines(&mut app).into_iter().map(|(_, t)| t).collect();
        assert_eq!(
            failed,
            [
                "Couldn't use that folder: /Volumes/gone isn't a folder",
                "Press Enter or click to choose it.",
                "Or run: guacamole --setup <folder>",
            ]
        );
    }

    /// AP.2: another error's message lands on the drawn line.
    #[test]
    fn another_error_patches_the_drawn_message() {
        let mut app = portal_app(1920, 1080, WindowMode::Tv, PortalCards::default());
        let error = |message: &str| TvScreen::ServerError {
            message: message.into(),
        };
        app.world_mut().resource_mut::<Current>().screen = error("open profile store");
        frames(&mut app, 2);
        let (line, text) = marked::<ErrorMessage>(&mut app);
        assert_eq!(text, "open profile store");
        app.world_mut().resource_mut::<Current>().screen = error("bind 0.0.0.0:8765");
        app.update();
        assert_eq!(
            marked::<ErrorMessage>(&mut app),
            (line, "bind 0.0.0.0:8765".to_string())
        );
    }

    /// Each cell's entity and its one child, the card or the empty frame.
    fn cells(app: &mut App) -> std::collections::BTreeMap<u8, (Entity, Entity)> {
        let world = app.world_mut();
        let mut q = world.query::<(Entity, &Cell, &Children)>();
        q.iter(world)
            .map(|(cell, slot, kids)| {
                assert_eq!(kids.len(), 1, "cell {}", slot.0);
                (slot.0, (cell, kids[0]))
            })
            .collect()
    }

    /// AP.3: a placement in slot 3 respawns that cell's content and nothing
    /// else: the other seven cells' content and the header stay.
    #[test]
    fn a_placement_respawns_only_its_own_cell() {
        let two = PortalCards::default().reduce([
            PortalChange::Placed {
                slot: SlotIndex::new(0).unwrap(),
                placement: placement("Eruptor", Some("#33cc66")),
            },
            PortalChange::Placed {
                slot: SlotIndex::new(5).unwrap(),
                placement: placement("Tree Rex", None),
            },
        ]);
        let mut app = portal_app(1920, 1080, WindowMode::Tv, two.clone());
        frames(&mut app, 3);
        let before = cells(&mut app);
        assert_eq!(before.len(), 8);
        let header = (
            marked::<GameName>(&mut app).0,
            marked::<PlayersLine>(&mut app).0,
        );

        let slot3 = SlotIndex::from_display(3).unwrap();
        *app.world_mut().resource_mut::<PortalCards>() =
            two.clone().reduce([PortalChange::Placed {
                slot: slot3,
                placement: placement("Spyro", Some("#33cc66")),
            }]);
        app.update();
        let after = cells(&mut app);
        for (slot, (cell, content)) in &after {
            assert_eq!(*cell, before[slot].0, "cell {slot} respawned");
            if *slot == slot3.as_u8() {
                assert_ne!(*content, before[slot].1);
            } else {
                assert_eq!(*content, before[slot].1, "slot {slot}'s content respawned");
            }
        }
        let world = app.world_mut();
        let card = world.get::<CardSlot>(after[&slot3.as_u8()].1).unwrap();
        assert_eq!(card.0, slot3.as_u8());
        assert_eq!(
            (
                marked::<GameName>(&mut app).0,
                marked::<PlayersLine>(&mut app).0
            ),
            header
        );

        // The same cards again (a resume replaying them) respawn nothing.
        let same = app.world().resource::<PortalCards>().clone();
        *app.world_mut().resource_mut::<PortalCards>() = same;
        app.update();
        assert_eq!(cells(&mut app), after);
    }

    /// Each frame's `Redrawn` messages, and the frames the layout report was
    /// armed in (`report_layout` takes one off every `Last`, so it reads
    /// `REPORT_AFTER_FRAMES` in `PostUpdate` only in a frame that armed it).
    #[derive(Resource, Default)]
    struct Seen {
        redrawn: Vec<Redrawn>,
        armed: usize,
    }

    fn see(mut seen: ResMut<Seen>, mut redrawn: MessageReader<Redrawn>, report: Res<LayoutReport>) {
        seen.redrawn.extend(redrawn.read().copied());
        if report.0 == REPORT_AFTER_FRAMES {
            seen.armed += 1;
        }
    }

    /// What was redrawn and how often the report was armed since last asked.
    fn seen(app: &mut App) -> (Vec<Redrawn>, usize) {
        let mut seen = app.world_mut().resource_mut::<Seen>();
        (
            std::mem::take(&mut seen.redrawn),
            std::mem::take(&mut seen.armed),
        )
    }

    /// AP.4: one slot's change arms the layout report once, a band tick arms
    /// it once, a patched line and an unchanged frame never do, and leaving
    /// the portal disarms it.
    #[test]
    fn a_slot_or_a_band_tick_arms_the_layout_report_once() {
        use crate::extraction::{ExtractStatus, band};
        let converting = |done| {
            ExtractBand(band(&ExtractStatus::Converting {
                done,
                total: 48,
                movie: "eruptor".into(),
            }))
        };
        let mut app = portal_app(1920, 1080, WindowMode::Tv, full("Eruptor"));
        app.init_resource::<Seen>()
            .add_systems(PostUpdate, see)
            .add_systems(Last, report_layout);
        frames(&mut app, 6);
        let (entered, armed) = seen(&mut app);
        assert_eq!(
            entered,
            [
                Redrawn::Screen(TvState::Starting),
                Redrawn::Screen(TvState::Portal)
            ]
        );
        assert_eq!(armed, 1);

        let slot3 = SlotIndex::from_display(3).unwrap();
        let spyro = app
            .world()
            .resource::<PortalCards>()
            .clone()
            .reduce([PortalChange::Placed {
                slot: slot3,
                placement: placement("Spyro", Some("#33cc66")),
            }]);
        *app.world_mut().resource_mut::<PortalCards>() = spyro;
        frames(&mut app, 6);
        assert_eq!(seen(&mut app), (vec![Redrawn::Slot(slot3.as_u8())], 1));
        assert_eq!(app.world().resource::<LayoutReport>().0, 0, "never fired");

        // The band appearing, then a tick.
        for done in [12, 13] {
            app.insert_resource(converting(done));
            frames(&mut app, 6);
            assert_eq!(seen(&mut app), (vec![Redrawn::Band], 1), "intro {done}");
        }

        frames(&mut app, 6);
        assert_eq!(seen(&mut app), (vec![], 0));
        app.world_mut().resource_mut::<Current>().clients = 3;
        frames(&mut app, 6);
        assert_eq!(seen(&mut app), (vec![Redrawn::Text], 0));
        app.world_mut().resource_mut::<Current>().screen = TvScreen::Join;
        frames(&mut app, 6);
        assert_eq!(seen(&mut app), (vec![Redrawn::Screen(TvState::Join)], 0));
    }
}
