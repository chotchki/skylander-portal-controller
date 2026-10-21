//! The TV: `ServerBridge` polls the server's status each frame, `Tv` draws
//! the screen it maps to (`screen::screen_for`) in `bevy_ui`. The screen's
//! kind is a `bevy_state` state (openspec AP, design decision 1): each kind
//! spawns its tree on enter under a root that despawns on exit. Sizes and
//! colours come from [`TvTheme`] (decision 6), through a [`Pen`]; every root
//! sits on the [`starfield`](crate::starfield) and cards and buttons wear
//! §3's [`surfaces`](crate::surfaces) (decision 8); a card's face rises,
//! flashes and floats inside the box the tests measure
//! ([`card_motion`](crate::card_motion), decision 9). The seven screen titles
//! and the portal's game name are drawn through the title shader
//! ([`title`](crate::title), decision 7), each by a camera of its own, so the
//! TV's camera is the one marked [`TvCamera`].

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use bevy::asset::RenderAssetUsages;
use bevy::ecs::system::SystemParam;
use bevy::image::{CompressedImageFormats, ImageSampler, ImageType};
use bevy::input_focus::AutoFocus;
use bevy::input_focus::tab_navigation::TabIndex;
use bevy::prelude::*;
use bevy::text::FontSize;
use bevy::ui::{ComputedNode, UiGlobalTransform};
// Not the prelude's `Button`: that's bevy_ui's deprecated one.
use bevy::ui_widgets::{Activate, Button};
use guac_server::config::WindowMode;
use guac_server::state::{GameList, LauncherStatus};
use guacamole::character::{CharacterShadingPlugin, GlowPlugin};
use skylander_core::{Element, SLOT_COUNT};
use skylander_rpcs3_control::{Placement, PlacementOwner};
use tracing::{error, info, warn};

use crate::card_motion::{self, CardFace, Schedule};
use crate::cards::{PortalCards, drain_portal};
use crate::extraction::{Band, ExtractBand};
use crate::input::FocusId;
use crate::level::LevelPlugin;
use crate::look::{self, ChoiceFace};
use crate::motion::ReducedMotion;
use crate::picker::FolderAsked;
use crate::prepare::{Prepare, PreparePlugin};
use crate::screen::{SetupView, TvScreen, screen_for, while_preparing};
use crate::starfield;
use crate::surfaces::{self, BUTTON_BORDER, BUTTON_RADIUS};
use crate::theme::{Role, Step, TvTheme};
use crate::title;
use crate::turntable::{CardPlate, TurntablePlugin, TurntableSystems, Turntables};

/// How long the farewell shows before the app exits (spec: under 5 s).
pub const FAREWELL_BEAT: Duration = Duration::from_secs(3);

/// How long AlreadyRunning shows before this copy closes itself.
const ALREADY_RUNNING_BEAT: Duration = Duration::from_secs(8);

/// The TV's choices, by the [`FocusId`] the router and the mirror know them by.
pub(crate) const CHOOSE_FOLDER: &str = "choose-folder";
pub(crate) const CONTINUE: &str = "continue";
pub(crate) const CLOSE: &str = "close";

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

/// A screen's title (§3.7's state titles).
const TITLE: Step = Step::Hero;

/// A card's figure name: the larger of 32 and 40 that still fits a long
/// name in two rows of four at 1080p, 4K and the desktop window.
const CARD_NAME: Step = Step::Md;

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
    /// The stars' two sprites (`starfield::Sprites`).
    pub(crate) stars: starfield::Sprites,
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

/// The start's pass's progress line and bar fill (percent) as drawn.
#[derive(Component)]
pub(crate) struct PrepareLine;

#[derive(Component)]
pub(crate) struct PrepareBar(pub(crate) u8);

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
    step: Step,
    role: Role,
    /// Wraps at the column's width instead of running off its sides.
    wrap: bool,
    /// A button with this [`FocusId`], labelled `text`, in place of a line.
    choice: Option<&'static str>,
}

impl Line {
    fn new(text: impl Into<String>, step: Step, role: Role) -> Self {
        Self {
            text: text.into(),
            step,
            role,
            wrap: false,
            choice: None,
        }
    }

    fn choice(label: &str, id: &'static str) -> Self {
        Self {
            choice: Some(id),
            ..Self::new(label, Step::Md, Role::Accent)
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

/// The TV's own camera. Every title renders through a `Camera2d` of its own
/// ([`title`]), so whatever reads the TV's camera (the UI's scale, the layout
/// report, the mirror, the shots) finds it by this, never by `Camera2d`.
#[derive(Component, Debug)]
pub(crate) struct TvCamera;

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
    /// A choice's focus look turned on or off (`look`).
    Focus,
    /// The level came up over the TV (the dev entry, AG.19).
    Level,
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
    Preparing,
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
    const ALL: [Self; 10] = [
        Self::Starting,
        Self::Preparing,
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
            TvScreen::Preparing => Self::Preparing,
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
            .init_resource::<TvTheme>()
            .init_resource::<Portraits>()
            .init_resource::<LayoutReport>()
            .init_resource::<FolderAsked>()
            .init_resource::<ReducedMotion>()
            .init_resource::<Turntables>()
            .add_message::<Redrawn>()
            .add_plugins(title::Titles)
            .add_observer(on_choice)
            .add_systems(PreUpdate, follow_screen.after(poll_status))
            .add_systems(Update, (click_or_beat_closes, starfield::drift));
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
                    patch_prepare,
                    sync_setup_lines,
                    sync_band,
                    sync_cells,
                )
                    .in_set(Redraw),
                arm_layout_report.after(Redraw),
                show_turntables.after(Redraw).after(TurntableSystems),
                look::ease_faces.after(Redraw),
                card_motion::move_cards.after(Redraw),
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
            // Before `Screens`, whose `Titles` needs only what this registers.
            .add_plugins(title::TitleShader)
            .add_plugins(Screens)
            // The cards' turntables (AE.13): the character material, the
            // game's glow and the per-slot scenes.
            .add_plugins((CharacterShadingPlugin, GlowPlugin, TurntablePlugin))
            // The level, loaded behind the portal from the Giants pick
            // (AG.19); the warm compiles its keys.
            .add_plugins(LevelPlugin)
            // The start's pass (AE.25): every pipeline the TV draws Giants
            // with, before the first screen a phone or the setup acts on.
            .add_plugins(PreparePlugin)
            .add_systems(Startup, spawn_camera)
            .add_systems(Last, report_layout)
            .add_systems(Update, (scale_ui, farewell_exit));
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
    prepare: Option<Res<Prepare>>,
    current: Option<ResMut<Current>>,
) {
    let preparing = prepare.is_some_and(|p| p.preparing());
    let screen = while_preparing(screen_for(&read_status(&link), &setup.0), preparing);
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

/// The font, the join QR and the stars, on the world `main` built
/// (`ServerLink` in it).
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
    let stars = starfield::Sprites::new(&mut world.resource_mut::<Assets<Image>>());
    TvAssets { font, qr, stars }
}

/// The default UI camera by name, so no camera added later (a title's, AE's
/// 3D one) can take the TV's UI.
fn spawn_camera(mut commands: Commands) {
    commands.spawn((Camera2d, TvCamera, IsDefaultUiCamera));
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
pub(crate) fn scale_ui(camera: Query<&Camera, With<TvCamera>>, mut scale: ResMut<UiScale>) {
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
    theme: Res<'w, TvTheme>,
    link: Res<'w, ServerLink>,
    setup: Res<'w, Setup>,
    framing: Res<'w, Framing>,
    /// None before the first `poll_status`, while Starting is up.
    current: Option<Res<'w, Current>>,
    cards: Res<'w, PortalCards>,
    band: Option<Res<'w, ExtractBand>>,
    portraits: ResMut<'w, Portraits>,
    images: ResMut<'w, Assets<Image>>,
    /// When a card was drawn, for its rise (`card_motion`).
    time: Res<'w, Time>,
    /// A card's model, once its turntable has one up (AE.13).
    turntables: Res<'w, Turntables>,
    /// The start's pass, for its screen's progress (AE.25).
    prepare: Option<Res<'w, Prepare>>,
}

impl Sources<'_> {
    fn clients(&self) -> usize {
        self.current.as_ref().map_or(0, |c| c.clients)
    }

    fn screen(&self) -> Option<&TvScreen> {
        self.current.as_ref().map(|c| &c.screen)
    }

    fn pen(&self) -> Pen {
        Pen::new(&self.assets, &self.theme)
    }
}

/// `OnEnter` every state: the screen's root on the sky, and everything under
/// it. The root goes on exit (`DespawnOnExit`), its tree with it.
fn enter_screen(
    mut commands: Commands,
    state: Res<State<TvState>>,
    mut sources: Sources,
    mut redrawn: MessageWriter<Redrawn>,
) {
    let state = *state.get();
    let stars = sources.assets.stars.clone();
    commands
        .spawn((
            ScreenRoot,
            DespawnOnExit(state),
            root_node(state, sources.framing.0),
            starfield::sky(&sources.theme),
        ))
        .with_children(|ui| {
            starfield::spawn_stars(ui, &stars, &sources.theme);
            spawn_content(ui, state, &mut sources);
        });
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
        // A title, so in caps (`title::spawn`).
        TvScreen::Portal { game } => patch(games.iter_mut(), &game.to_uppercase(), redrawn),
        TvScreen::ServerError { message } => patch(messages.iter_mut(), message, redrawn),
        _ => {}
    }
}

/// The start's pass's line and bar follow its count of keys compiled.
fn patch_prepare(
    prepare: Option<Res<Prepare>>,
    mut lines: Query<&mut Text, With<PrepareLine>>,
    mut bars: Query<(&mut PrepareBar, &mut Node)>,
    redrawn: MessageWriter<Redrawn>,
) {
    let Some(prepare) = prepare.filter(|p| p.is_changed()) else {
        return;
    };
    let r = prepare.report();
    let pct = prepare_percent(r.compiled, r.keys);
    for (mut bar, mut node) in &mut bars {
        if bar.0 != pct {
            bar.0 = pct;
            node.width = percent(f32::from(pct));
        }
    }
    patch(lines.iter_mut(), &prepare_line(r.compiled, r.keys), redrawn);
}

/// The pass's progress as drawn: how many of its keys are ready.
fn prepare_line(compiled: usize, keys: usize) -> String {
    if keys == 0 {
        String::new()
    } else {
        format!("{compiled} of {keys} ready")
    }
}

fn prepare_percent(compiled: usize, keys: usize) -> u8 {
    (compiled.min(keys) * 100).checked_div(keys).unwrap_or(0) as u8
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
    theme: Res<TvTheme>,
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
        // A button's label isn't patched: another label is another button.
        let same_shape = drawn.0.len() == lines.len()
            && drawn.0.iter().zip(&lines).all(|(a, b)| {
                a.wrap == b.wrap && a.choice == b.choice && (a.choice.is_none() || a.text == b.text)
            });
        match children.filter(|_| same_shape) {
            Some(children) => {
                for (kid, line) in children.iter().zip(&lines) {
                    if line.choice.is_some() {
                        continue;
                    }
                    let Ok((mut text, mut font, mut color)) = texts.get_mut(kid) else {
                        continue;
                    };
                    text.set_if_neq(Text(line.text.clone()));
                    color.set_if_neq(TextColor(theme.role(line.role)));
                    let size = FontSize::Px(theme.size(line.step));
                    if font.font_size != size {
                        font.font_size = size;
                    }
                }
            }
            None => {
                let pen = Pen::new(&assets, &theme);
                commands
                    .entity(column)
                    .despawn_children()
                    .with_children(|column| {
                        for line in &lines {
                            spawn_line(column, &pen, line);
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
    theme: Res<TvTheme>,
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
            let pen = Pen::new(&assets, &theme);
            commands
                .entity(slot)
                .despawn_children()
                .with_children(|ui| {
                    spawn_band_slot(ui, &pen, shape, band);
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
            color.set_if_neq(TextColor(theme.role(detail_role(band))));
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
    theme: Res<TvTheme>,
    time: Res<Time>,
    turntables: Res<Turntables>,
    mut portraits: ResMut<Portraits>,
    mut images: ResMut<Assets<Image>>,
    mut cells: Query<(Entity, &Cell, &mut Drew)>,
    mut redrawn: MessageWriter<Redrawn>,
) {
    if !cards.is_changed() {
        return;
    }
    let pen = Pen::new(&assets, &theme);
    for (entity, cell, mut drew) in &mut cells {
        let now = &cards.slots[usize::from(cell.0)];
        if drew.0 == *now {
            continue;
        }
        let portrait = portrait_of(now.as_ref(), &mut portraits, &mut images);
        let shown = CardImages {
            portrait,
            model: turntables.image(cell.0),
        };
        // A card placed while the portal is up rises alone and flashes.
        let motion = Schedule::placement(cell.0, time.elapsed());
        commands
            .entity(entity)
            .despawn_children()
            .with_children(|content| {
                spawn_slot(content, &pen, cell.0, now.as_ref(), shown, motion);
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
            Redrawn::Text | Redrawn::Focus | Redrawn::Level => {}
        }
    }
}

/// What a choice does when it's activated: by a click on it, Enter or Space
/// while it's focused (the widget's own), South or NumpadEnter (the router).
fn on_choice(
    activate: On<Activate>,
    ids: Query<&FocusId>,
    mut setup: ResMut<Setup>,
    mut asked: ResMut<FolderAsked>,
    mut exit: MessageWriter<AppExit>,
) {
    let Ok(id) = ids.get(activate.entity) else {
        return;
    };
    info!(choice = id.0, "tv choice");
    match id.0 {
        CHOOSE_FOLDER => asked.0 = true,
        CONTINUE if setup.0.show_found => setup.0.show_found = false,
        CLOSE => {
            exit.write(AppExit::Success);
        }
        _ => {}
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

/// A click anywhere closes the startup-error screens (AA.6: a mouse-only TV
/// PC), beside their Close button and the router's Back (East, Esc);
/// AlreadyRunning also closes itself so it never hides the running app's TV.
fn click_or_beat_closes(
    mouse: Res<ButtonInput<MouseButton>>,
    current: Option<Res<Current>>,
    mut exit: MessageWriter<AppExit>,
) {
    let Some(current) = current.filter(|c| c.screen.escape_quits()) else {
        return;
    };
    let clicked = mouse.just_pressed(MouseButton::Left);
    let timed_out = current.screen == TvScreen::AlreadyRunning
        && current.since.elapsed() >= ALREADY_RUNNING_BEAT;
    if clicked || timed_out {
        info!(screen = ?current.screen, timed_out, "closing");
        exit.write(AppExit::Success);
    }
}

/// The folder picker under Setup and NoGiants: its button, or while the
/// sheet is up a line saying where it is.
fn picker_lines(setup: &SetupView) -> Vec<Line> {
    let mut lines = Vec::new();
    if let Some(error) = &setup.error {
        lines.push(Line::new(
            format!("Couldn't use that folder: {error}"),
            Step::Body,
            Role::Primary,
        ));
    }
    lines.push(if setup.picking {
        Line::new(
            "Choose a folder in the window that opened.",
            Step::Md,
            Role::Primary,
        )
    } else {
        Line::choice("Choose the folder", CHOOSE_FOLDER)
    });
    lines.push(Line::new(
        "Or run: guacamole --setup <folder>",
        Step::Caption,
        Role::Muted,
    ));
    lines
}

/// The lines under Setup's and NoGiants' headings, all from `Setup`.
fn setup_lines(state: TvState, setup: &SetupView) -> Vec<Line> {
    match state {
        TvState::Setup => picker_lines(setup),
        TvState::NoGiants => {
            let mut lines = vec![Line::new(
                format!("Looked in: {}", setup.folders.join(", ")),
                Step::Body,
                Role::Muted,
            )];
            for problem in &setup.problems {
                lines.push(Line::new(problem.clone(), Step::Body, Role::Primary));
            }
            if !setup.found_titles.is_empty() {
                lines.push(Line::new("Found:", Step::Md, Role::Primary));
                lines.push(Line::new(setup.titles_line(), Step::Body, Role::Primary).wrapping());
            } else if setup.problems.is_empty() {
                lines.push(Line::new("No game dumps there.", Step::Md, Role::Primary));
            }
            lines.extend(picker_lines(setup));
            lines
        }
        _ => Vec::new(),
    }
}

/// A centred line; a wrapping one is held to the column's width.
fn line_bundle(pen: &Pen, line: &Line) -> impl Bundle + use<> {
    (
        pen.text(&line.text, line.step, line.role),
        Node {
            max_width: if line.wrap { percent(100) } else { Val::Auto },
            ..default()
        },
    )
}

/// A setup column's line: centred text, or a choice's button.
fn spawn_line(column: &mut ChildSpawnerCommands, pen: &Pen, line: &Line) {
    match line.choice {
        Some(id) => column.spawn(choice_bundle(pen, &line.text, id)),
        None => column.spawn(line_bundle(pen, line)),
    };
}

/// A choice: a `bevy_ui_widgets` button, focused when it appears
/// (`AutoFocus`), taking the focus from a click on it (`TabIndex`; without one
/// a click clears it) and named for the router and the mirror (`FocusId`).
/// Its look, §3.2's blue card, sits on a face inside it, which the focus
/// look and the press scale (`look`) without moving a rect the tests read.
fn choice_bundle(pen: &Pen, text: &str, id: &'static str) -> impl Bundle + use<> {
    (
        Button,
        TabIndex(0),
        AutoFocus,
        FocusId(id),
        Node {
            flex_shrink: 0.0,
            ..default()
        },
        children![(
            ChoiceFace::default(),
            Node {
                padding: UiRect::axes(px(48), px(16)),
                border: UiRect::all(px(BUTTON_BORDER)),
                border_radius: BorderRadius::all(px(BUTTON_RADIUS)),
                ..default()
            },
            surfaces::blue_card(&pen.theme),
            children![pen.text(text, Step::Md, Role::Accent)],
        )],
    )
}

/// What every line is drawn with: the font and the theme, so a size comes
/// only from a [`Step`] and a colour from a [`Role`].
pub(crate) struct Pen {
    font: Handle<Font>,
    pub(crate) theme: TvTheme,
}

impl Pen {
    pub(crate) fn new(assets: &TvAssets, theme: &TvTheme) -> Self {
        Self {
            font: assets.font.clone(),
            theme: theme.clone(),
        }
    }

    fn font(&self, step: Step) -> TextFont {
        TextFont {
            font: self.font.clone().into(),
            font_size: FontSize::Px(self.theme.size(step)),
            ..default()
        }
    }

    /// Centred.
    pub(crate) fn text(&self, s: &str, step: Step, role: Role) -> impl Bundle + use<> {
        self.text_at(s, step, role, Justify::Center)
    }

    fn text_at(&self, s: &str, step: Step, role: Role, justify: Justify) -> impl Bundle + use<> {
        self.inked(s, step, self.theme.role(role), justify)
    }

    /// A title in caps through the title shader (decision 7), `marker` on its
    /// hidden layout `Text`; a screen's own title gets §2's halo behind it.
    fn title(
        &self,
        parent: &mut ChildSpawnerCommands,
        s: &str,
        step: Step,
        justify: Justify,
        halo: bool,
        marker: impl Bundle,
    ) {
        title::spawn(
            parent,
            &self.theme,
            self.font.clone(),
            s,
            step,
            justify,
            halo,
            marker,
        );
    }

    /// A screen's own title, centred, with its halo.
    fn screen_title(&self, parent: &mut ChildSpawnerCommands, s: &str, step: Step) {
        self.title(parent, s, step, Justify::Center, true, ());
    }

    /// In a colour no role names: a pip's ink, picked against its fill.
    fn inked(&self, s: &str, step: Step, color: Color, justify: Justify) -> impl Bundle + use<> {
        (
            Text::new(s),
            self.font(step),
            TextColor(color),
            TextLayout::justify(justify),
        )
    }
}

/// The band's detail: primary when it's a failure to act on, muted for progress.
fn detail_role(band: &Band) -> Role {
    if band.code.is_some() {
        Role::Primary
    } else {
        Role::Muted
    }
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
    pen: &Pen,
    shape: BandShape,
    band: Option<&Band>,
) {
    match (shape, band) {
        (BandShape::Hint, _) => {
            slot.spawn(pen.text_at(
                "Put a figure on the portal from your phone.",
                Step::Body,
                Role::Primary,
                Justify::Left,
            ));
        }
        (BandShape::Band { .. }, Some(band)) => spawn_band(slot, pen, band),
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
fn spawn_setup_lines(ui: &mut ChildSpawnerCommands, pen: &Pen, lines: Vec<Line>) {
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
            spawn_line(column, pen, line);
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
    let pen = sources.pen();

    match state {
        TvState::Starting => {
            ui.spawn(pen.text("Starting…", Step::Lg, Role::Muted));
        }
        TvState::Preparing => {
            let (compiled, keys) = sources
                .prepare
                .as_ref()
                .map_or((0, 0), |p| (p.report().compiled, p.report().keys));
            pen.screen_title(ui, "Getting ready", TITLE);
            ui.spawn(pen.text(
                "Setting up the graphics on this computer.",
                Step::Md,
                Role::Primary,
            ));
            let pct = prepare_percent(compiled, keys);
            spawn_bar(ui, &pen, pct, PrepareBar(pct));
            ui.spawn((
                PrepareLine,
                pen.text(&prepare_line(compiled, keys), Step::Body, Role::Muted),
            ));
            ui.spawn(pen.text(
                "The first start takes a while. Later ones don't.",
                Step::Caption,
                Role::Muted,
            ));
        }
        TvState::Setup => {
            pen.screen_title(ui, "Where are your disc dumps?", TITLE);
            ui.spawn(pen.text(
                "Choose the folder that holds your Skylanders Giants dump.",
                Step::Md,
                Role::Primary,
            ));
            spawn_setup_lines(ui, &pen, setup_lines(state, setup));
        }
        TvState::NoGiants => {
            // At 96 it takes two rows of the 1080p inset; 64 keeps it to one.
            pen.screen_title(ui, "No Skylanders Giants dump found", Step::Lg);
            spawn_setup_lines(ui, &pen, setup_lines(state, setup));
        }
        TvState::Found => {
            pen.screen_title(ui, "Found your games", TITLE);
            let titles = Line::new(setup.titles_line(), Step::Body, Role::Primary).wrapping();
            ui.spawn((TitlesLine, line_bundle(&pen, &titles)));
            ui.spawn(pen.text("Skylanders: Giants is ready!", Step::Lg, Role::Accent));
            ui.spawn(choice_bundle(&pen, "Continue", CONTINUE));
        }
        TvState::Join => {
            // navigation.md §3.7 names the launcher's "SCAN TO CONNECT" as its 64.
            pen.screen_title(ui, "Scan to play", Step::Lg);
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
            ui.spawn((JoinAddress, pen.text(address, Step::Body, Role::Muted)));
            let players = players_line(sources.clients());
            ui.spawn((PlayersLine, pen.text(&players, Step::Md, Role::Primary)));
        }
        // Handled above.
        TvState::Portal => {}
        TvState::AlreadyRunning => {
            pen.screen_title(ui, "Already running", TITLE);
            ui.spawn(pen.text(
                "guacamole or the Skylander Portal Controller is already serving phones on this computer.",
                Step::Md,
                Role::Primary,
            ));
            ui.spawn(choice_bundle(&pen, "Close", CLOSE));
            ui.spawn(pen.text(
                "Press Esc or click to close this one (it closes itself in a few seconds).",
                Step::Body,
                Role::Muted,
            ));
        }
        TvState::ServerError => {
            let message = match sources.screen() {
                Some(TvScreen::ServerError { message }) => message.as_str(),
                _ => "",
            };
            pen.screen_title(ui, "Something went wrong", TITLE);
            // The whole cause chain, wrapped inside the column; a path with
            // no space to break at breaks between characters.
            ui.spawn((
                ErrorMessage,
                Text::new(message),
                pen.font(Step::Body),
                TextColor(pen.theme.role(Role::Primary)),
                TextLayout::new(Justify::Center, LineBreak::WordOrCharacter),
                Node {
                    max_width: percent(100),
                    ..default()
                },
            ));
            ui.spawn(choice_bundle(&pen, "Close", CLOSE));
            ui.spawn(pen.text("Press Esc or click to close.", Step::Body, Role::Muted));
        }
        TvState::Farewell => {
            pen.screen_title(ui, "Goodbye, Portal Master!", TITLE);
        }
    }
}

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

/// The profile's colour as the store keeps it (`#rgb` or `#rrggbb`); gold if
/// it won't parse, so a pip never goes invisible.
pub fn owner_color(owner: &PlacementOwner, theme: &TvTheme) -> Srgba {
    Srgba::hex(&owner.color).unwrap_or(theme.gold.to_srgba())
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
    camera: Query<&Camera, With<TvCamera>>,
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
    let pen = sources.pen();
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
                // A title, without the halo a screen's own gets: it heads
                // the cards from the corner rather than centring a screen.
                pen.title(left, &game, Step::Lg, Justify::Left, false, GameName);
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
                .with_children(|slot| spawn_band_slot(slot, &pen, shape, band.as_ref()));
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
                        let right =
                            |s: &str, step, role| pen.text_at(s, step, role, Justify::Right);
                        join.spawn(right("Scan to join", Step::Body, Role::Primary));
                        join.spawn((PlayersLine, right(&players, Step::Caption, Role::Muted)));
                        join.spawn((JoinAddress, right(&address, Step::Caption, Role::Muted)));
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
    .with_children(|grid| spawn_rows(grid, &pen, sources));
}

/// The grid's rows of cells, each holding its slot's card or empty frame.
fn spawn_rows(grid: &mut ChildSpawnerCommands, pen: &Pen, sources: &mut Sources) {
    let Sources {
        cards,
        portraits,
        images,
        time,
        turntables,
        ..
    } = sources;
    // The entrance: each card rises 80 ms after the one before it.
    let mut order = 0;
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
                let shown = CardImages {
                    portrait: portrait_of(placement, portraits, images),
                    model: turntables.image(slot as u8),
                };
                let motion = Schedule::entrance(slot as u8, order, time.elapsed());
                order += usize::from(placement.is_some());
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
                        spawn_slot(cell, pen, slot as u8, placement, shown, motion);
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

/// What a card's plate can show: its portrait, and its turntable's image
/// once the model is up, which it shows instead.
struct CardImages {
    portrait: Option<Handle<Image>>,
    model: Option<Handle<Image>>,
}

/// A cell's content: the card, moving as `motion` says, or the numbered
/// empty frame.
fn spawn_slot(
    cell: &mut ChildSpawnerCommands,
    pen: &Pen,
    slot: u8,
    placement: Option<&Placement>,
    images: CardImages,
    motion: Schedule,
) {
    match placement {
        Some(placement) => spawn_card(cell, pen, slot, placement, images, motion),
        None => spawn_empty(cell, pen, slot),
    }
}

/// Each card's plate shows its turntable's image once the model is up, and
/// its portrait until then or without one (the figure-turntable spec),
/// patched in place so the card neither respawns nor rises again.
fn show_turntables(
    mut commands: Commands,
    turntables: Res<Turntables>,
    plates: Query<(Entity, &CardPlate, Option<&ImageNode>)>,
    mut redrawn: MessageWriter<Redrawn>,
) {
    for (entity, plate, node) in &plates {
        let wanted = turntables
            .image(plate.slot)
            .or_else(|| plate.portrait.clone());
        if node.map(|n| n.image.id()) == wanted.as_ref().map(Handle::id) {
            continue;
        }
        match wanted {
            Some(image) => commands.entity(entity).insert(ImageNode::new(image)),
            None => commands.entity(entity).remove::<ImageNode>(),
        };
        redrawn.write(Redrawn::Slot(plate.slot));
    }
}

/// The extraction's title, detail and bar, under the game's name.
fn spawn_band(left: &mut ChildSpawnerCommands, pen: &Pen, band: &Band) {
    left.spawn((
        ExtractTitle,
        pen.text_at(&band.title, Step::Body, Role::Accent, Justify::Left),
    ));
    left.spawn((
        ExtractDetail,
        pen.text_at(
            &band.detail,
            Step::Caption,
            detail_role(band),
            Justify::Left,
        ),
    ));
    if let Some(pct) = band.percent {
        spawn_bar(left, pen, pct, ExtractBar(pct));
    }
}

/// A progress bar: `pct` of a faint track in gold, its fill marked with
/// `fill` (the extraction's or the start's pass's) for the patch to find.
fn spawn_bar(parent: &mut ChildSpawnerCommands, pen: &Pen, pct: u8, fill: impl Component) {
    parent
        .spawn((
            Node {
                width: percent(100),
                max_width: px(640),
                height: px(12),
                flex_shrink: 0.0,
                border_radius: BorderRadius::MAX,
                overflow: Overflow::clip(),
                ..default()
            },
            BackgroundColor(pen.theme.faint),
        ))
        .with_children(|bar| {
            bar.spawn((
                fill,
                Node {
                    width: percent(f32::from(pct)),
                    height: percent(100),
                    ..default()
                },
                BackgroundColor(pen.theme.gold),
            ));
        });
}

/// A cell's content box: fills the cell.
fn slot_box() -> Node {
    Node {
        flex_grow: 1.0,
        flex_basis: px(0),
        min_width: px(0),
        min_height: px(0),
        flex_direction: FlexDirection::Column,
        ..default()
    }
}

/// What a slot draws in its box: the edge and corners, the padding and the
/// lines centred, clipping anything that doesn't fit.
fn slot_frame() -> Node {
    Node {
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

/// An empty frame's node: the box and the frame in one, since it never moves.
fn slot_content() -> Node {
    Node {
        flex_grow: 1.0,
        flex_basis: px(0),
        min_width: px(0),
        min_height: px(0),
        ..slot_frame()
    }
}

/// Portrait, name, element and owner pip; no level, XP or gold (AH). The card
/// is §3.3's framed panel edged in gold, the portrait (or the figure's
/// turning model, AE.13) sits in §3.1's bezel ringed in the figure's element,
/// and the pip stays top-right. `CardSlot` is
/// the box the mirror and the tests measure; the card itself is its face, laid
/// over the box, which rises, and the bezel on it floats and flashes
/// (`card_motion`), so neither moves the box.
fn spawn_card(
    cell: &mut ChildSpawnerCommands,
    pen: &Pen,
    slot: u8,
    placement: &Placement,
    images: CardImages,
    motion: Schedule,
) {
    spawn_card_as(cell, pen, Some(slot), placement, images, motion);
}

/// [`spawn_card`], marked as `slot`'s card (`CardSlot`, `CardPlate`, `Pip`)
/// or, with none, marked as nothing: the start's pass's offscreen card.
fn spawn_card_as(
    cell: &mut ChildSpawnerCommands,
    pen: &Pen,
    slot: Option<u8>,
    placement: &Placement,
    images: CardImages,
    motion: Schedule,
) {
    let theme = &pen.theme;
    let mut card = cell.spawn(slot_box());
    if let Some(slot) = slot {
        card.insert(CardSlot(slot));
    }
    card.with_children(|card| {
        card.spawn((
            CardFace(motion),
            Node {
                position_type: PositionType::Absolute,
                left: px(0),
                right: px(0),
                top: px(0),
                bottom: px(0),
                ..slot_frame()
            },
            surfaces::card(theme),
        ))
        .with_children(|face| {
            let shown = images.model.or_else(|| images.portrait.clone());
            let (bezel, plate) = surfaces::spawn_bezel(face, theme, placement.element, shown);
            if let Some(slot) = slot {
                face.commands().entity(plate).insert(CardPlate {
                    slot,
                    portrait: images.portrait,
                });
            }
            card_motion::on_bezel(face.commands().entity(bezel), motion, theme);
            face.spawn((
                pen.text(&placement.name, CARD_NAME, Role::Primary),
                Node {
                    max_width: percent(100),
                    ..default()
                },
            ));
            if let Some(element) = placement.element {
                face.spawn(pen.text(element_label(element), Step::Caption, Role::Muted));
            }
            if let Some(owner) = &placement.owner {
                let color = owner_color(owner, theme);
                let mut pip = face.spawn(surfaces::pip(theme, color.into()));
                if let Some(slot) = slot {
                    pip.insert(Pip(slot));
                }
                pip.with_children(|pip| {
                    let ink = theme.ink_on(color);
                    pip.spawn(pen.inked(&owner.initial, Step::Body, ink, Justify::Center));
                });
            }
        });
    });
}

/// A card for no figure on the portal, drawn by `camera` (offscreen, `size`
/// pixels) and nowhere else: the start's pass (AE.25) draws one so the UI
/// pipelines a card's face, bezel, shadows, flash, name and pip draw with
/// are compiled before the first placement. It carries none of the markers
/// the mirror, the layout report and the turntables find a card by.
pub(crate) fn spawn_sample_card(
    commands: &mut Commands,
    assets: &TvAssets,
    theme: &TvTheme,
    camera: Entity,
    size: UVec2,
    plate: Handle<Image>,
    now: Duration,
) -> Entity {
    let pen = Pen::new(assets, theme);
    let figure_id = skylander_core::FigureId::new("000009-0000");
    let (toy_type, variant) = figure_id.subtype_parts().expect("a well-formed figure id");
    let placement = Placement {
        figure_id,
        toy_type,
        variant,
        name: "Getting ready".into(),
        element: Some(Element::Fire),
        portrait: None,
        owner: Some(PlacementOwner {
            profile_id: String::new(),
            initial: "G".into(),
            color: "#da5ad6".into(),
            progress: Default::default(),
        }),
    };
    let images = CardImages {
        portrait: Some(plate),
        model: None,
    };
    commands
        .spawn((
            Node {
                width: px(size.x as f32),
                height: px(size.y as f32),
                flex_direction: FlexDirection::Column,
                ..default()
            },
            bevy::ui::UiTargetCamera(camera),
            Name::new("prepare: a card"),
        ))
        .with_children(|cell| {
            spawn_card_as(
                cell,
                &pen,
                None,
                &placement,
                images,
                Schedule::placement(0, now),
            );
        })
        .id()
}

fn spawn_empty(cell: &mut ChildSpawnerCommands, pen: &Pen, slot: u8) {
    cell.spawn((
        EmptySlot(slot),
        slot_content(),
        BorderColor::all(pen.theme.faint),
    ))
    .with_children(|frame| {
        let number = (slot + 1).to_string();
        frame.spawn(pen.text(&number, Step::Md, Role::Muted));
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Runs `click_or_beat_closes` once on `screen`, shown for `age`, with a
    /// left click or not.
    fn closes(screen: TvScreen, age: Duration, click: bool) -> bool {
        let mut app = App::new();
        let mut mouse = ButtonInput::<MouseButton>::default();
        if click {
            mouse.press(MouseButton::Left);
        }
        app.insert_resource(mouse)
            .insert_resource(Current {
                screen,
                since: Instant::now().checked_sub(age).unwrap(),
                clients: 0,
            })
            .add_systems(Update, click_or_beat_closes);
        app.update();
        app.should_exit().is_some()
    }

    /// AE.31: the extract's portrait through the card's loader keeps its
    /// rows, so a portrait the disc stores bottom row first reaches the card
    /// with its head on top.
    #[test]
    fn a_cards_portrait_is_the_right_way_up() {
        use guac_extract::roster::{Portrait, png};
        use guac_formats::texture::{Format, Texture};
        // One opaque BC3 block, every texel the RGB565 colour `c0`.
        let block = |c0: u16| {
            let mut b = vec![255, 255, 0, 0, 0, 0, 0, 0];
            b.extend(c0.to_le_bytes());
            b.extend([0; 6]);
            b
        };
        // Stored as the disc stores it: red, first, is the foot; blue the head.
        let portrait = Portrait {
            path: "portraits/000009-0000.png".into(),
            texture: Texture {
                format: Format::Bc3Unorm,
                width: 4,
                height: 8,
                levels: vec![[block(0xF800), block(0x001F)].concat()],
            },
        };
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("000009-0000.png");
        std::fs::write(&path, png(&portrait).unwrap()).unwrap();
        let mut images = Assets::<Image>::default();
        let handle = Portraits::default()
            .get(&path, &mut images)
            .expect("the portrait loads");
        let data = images.get(&handle).unwrap().data.as_ref().unwrap();
        assert_eq!(data.len(), 4 * 8 * 4);
        for x in 0..4 {
            assert_eq!(data[x * 4..][..4], [0, 0, 255, 255], "top row, x {x}");
            assert_eq!(data[(7 * 4 + x) * 4..][..4], [255, 0, 0, 255], "bottom row");
        }
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
            progress: Default::default(),
        };
        let theme = TvTheme::default();
        assert_eq!(
            owner_color(&owner("#ff0000"), &theme),
            Srgba::rgb(1.0, 0.0, 0.0)
        );
        assert_eq!(
            owner_color(&owner("#00f"), &theme),
            Srgba::rgb(0.0, 0.0, 1.0)
        );
        assert_eq!(owner_color(&owner("teal"), &theme), theme.gold.to_srgba());
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
        let theme = TvTheme::default();
        assert_ne!(theme.element(Some(Fire)), theme.element(Some(Water)));
    }

    /// The shell delta's "Nothing on the join code reacts" (AP.6): a pad
    /// button, a key and a click change nothing on Join.
    #[test]
    fn a_click_never_closes_the_join_screen() {
        use bevy::input::gamepad::GamepadButton;
        use bevy::input::keyboard::Key;

        use super::choice_tests::{click_beside, exited, focus, key, pad, tap, tv_on};
        assert!(!closes(TvScreen::Join, Duration::ZERO, true));
        assert!(!closes(TvScreen::Join, ALREADY_RUNNING_BEAT * 4, false));

        let joined = SetupView {
            giants_found: true,
            ..Default::default()
        };
        let mut app = tv_on(TvScreen::Join, Duration::ZERO, joined.clone());
        let pad = pad(&mut app);
        for button in [
            GamepadButton::South,
            GamepadButton::East,
            GamepadButton::Mode,
            GamepadButton::Start,
            GamepadButton::DPadDown,
        ] {
            tap(&mut app, pad, button);
        }
        for (code, logical) in [
            (KeyCode::Escape, Key::Escape),
            (KeyCode::Enter, Key::Enter),
            (KeyCode::NumpadEnter, Key::Enter),
            (KeyCode::Space, Key::Space),
            (KeyCode::ArrowUp, Key::ArrowUp),
        ] {
            key(&mut app, code, logical);
        }
        click_beside(&mut app);
        app.update();
        assert!(!exited(&app));
        assert_eq!(app.world().resource::<Current>().screen, TvScreen::Join);
        assert_eq!(
            *app.world().resource::<State<TvState>>().get(),
            TvState::Join
        );
        assert_eq!(app.world().resource::<Setup>().0, joined);
        assert_eq!(focus(&mut app), None);
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

    /// The layout at rest, as a shot sees it: the cards' motion at its end
    /// (`ReducedMotion`), so a card mid-rise doesn't carry its lines across
    /// its box. The motion's own tests measure it moving.
    fn measure(app: &mut App) -> Laid {
        app.insert_resource(ReducedMotion(true));
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
        // The TV's text. A title's ink lays out on the title's own camera,
        // and nothing else may: a line that lost the TV's camera would slip
        // past every check that reads these.
        let tv = tv_camera(world);
        let mut q = world.query_filtered::<(
            &ComputedNode,
            &UiGlobalTransform,
            &bevy::ui::ComputedUiTargetCamera,
            Option<&ChildOf>,
        ), With<Text>>();
        let mut texts = Vec::new();
        let mut off_tv = Vec::new();
        for (node, at, camera, parent) in q.iter(world) {
            if camera.get() == Some(tv) {
                texts.push(node_rect(node, at));
            } else {
                off_tv.push(parent.map(ChildOf::parent));
            }
        }
        for parent in off_tv {
            assert!(
                parent.is_some_and(|p| world.get::<title::TitleInk>(p).is_some()),
                "a text off the TV's camera that isn't a title's ink"
            );
        }
        Laid {
            cards,
            empties,
            pips,
            texts,
        }
    }

    pub(crate) fn tv_camera(world: &mut World) -> Entity {
        let mut q = world.query_filtered::<Entity, With<TvCamera>>();
        q.single(world).unwrap()
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
            let line = TvTheme::default().size(Step::Body) * h as f32 / DESIGN_HEIGHT;
            assert!(rect.height() > 2.0 * line, "{w}x{h}: one line, {rect:?}");
            assert!(contains(safe, rect), "{w}x{h}: {rect:?} leaves {safe:?}");
            // The title, the message, the Close button's label and the hint.
            assert_eq!(laid.texts.len(), 4);
            for (i, a) in laid.texts.iter().enumerate() {
                assert!(contains(safe, *a), "{w}x{h}: {a:?} leaves {safe:?}");
                for b in &laid.texts[i + 1..] {
                    assert!(a.intersect(*b).is_empty(), "{a:?} overlaps {b:?}");
                }
            }
        }
    }

    /// Every drawn `Text` under the screen's root: its size, colour and rect.
    fn drawn_text(app: &mut App) -> Vec<(String, FontSize, Color, Rect)> {
        let world = app.world_mut();
        let mut q = world.query::<(
            Entity,
            &Text,
            &TextFont,
            &TextColor,
            &ComputedNode,
            &UiGlobalTransform,
        )>();
        let mut parents = world.query::<&ChildOf>();
        let mut drawn = Vec::new();
        for (entity, text, font, color, node, at) in q.iter(world) {
            let mut top = entity;
            while let Ok(parent) = parents.get(world, top) {
                top = parent.parent();
            }
            if world.get::<ScreenRoot>(top).is_some() {
                drawn.push((text.0.clone(), font.font_size, color.0, node_rect(node, at)));
            }
        }
        drawn
    }

    /// AP.9: every screen, with the most lines each can draw, and the portal
    /// full under a band and empty with its hint, draws its text on the TV
    /// scale (nothing under 24 design px, nothing between steps) in a role's
    /// colour or a pip's ink, inside the inset with no two lines overlapping,
    /// at 1080p and 4K on the TV and in the desktop window.
    #[test]
    fn every_screen_draws_on_the_type_scale_inside_the_inset() {
        use crate::extraction::{ExtractStatus, band};
        let theme = TvTheme::default();
        let folders = vec![
            "/Volumes/dumps".to_string(),
            "/Users/someone/ps3".to_string(),
        ];
        let titles = vec![
            "Skylanders: SWAP Force (BLUS31076)".to_string(),
            "Skylanders: Trap Team (BLUS31442)".to_string(),
            "Skylanders: SuperChargers (BLUS31545)".to_string(),
        ];
        let error = Some("/Volumes/gone isn't a folder".to_string());
        let setup = |f: fn(&mut SetupView)| {
            let mut view = SetupView::default();
            f(&mut view);
            view
        };
        let no_giants = SetupView {
            folders: folders.clone(),
            found_titles: titles.clone(),
            problems: vec!["Couldn't read /Volumes/dumps/BLUS31442: permission denied".into()],
            error: error.clone(),
            ..Default::default()
        };
        let giants = || TvScreen::Portal {
            game: "Skylanders: Giants".into(),
        };
        let progress = band(&ExtractStatus::Converting {
            done: 12,
            total: 48,
            movie: "eruptor".into(),
        });
        let server_error = TvScreen::ServerError {
            message: "open profile store: open sqlite at /Users/someone/Library/Application Support/guacamole/db.sqlite: unable to open database file".into(),
        };
        let cases: Vec<(&str, TvScreen, SetupView, PortalCards, Option<Band>)> = vec![
            (
                "starting",
                TvScreen::Starting,
                SetupView::default(),
                PortalCards::default(),
                None,
            ),
            (
                "setup",
                TvScreen::Setup,
                setup(|v| v.needs_setup = true),
                PortalCards::default(),
                None,
            ),
            (
                "setup, a failed pick",
                TvScreen::Setup,
                SetupView {
                    error: error.clone(),
                    ..Default::default()
                },
                PortalCards::default(),
                None,
            ),
            (
                "setup, picking",
                TvScreen::Setup,
                setup(|v| v.picking = true),
                PortalCards::default(),
                None,
            ),
            (
                "no giants",
                TvScreen::NoGiants,
                no_giants.clone(),
                PortalCards::default(),
                None,
            ),
            (
                "no giants, nothing there",
                TvScreen::NoGiants,
                SetupView {
                    folders: folders.clone(),
                    ..Default::default()
                },
                PortalCards::default(),
                None,
            ),
            (
                "found",
                TvScreen::Found,
                SetupView {
                    found_titles: titles.clone(),
                    show_found: true,
                    giants_found: true,
                    ..Default::default()
                },
                PortalCards::default(),
                None,
            ),
            (
                "join",
                TvScreen::Join,
                SetupView::default(),
                PortalCards::default(),
                None,
            ),
            (
                "portal, empty",
                giants(),
                SetupView::default(),
                PortalCards::default(),
                None,
            ),
            (
                "portal, eight under a band",
                giants(),
                SetupView::default(),
                full("Legendary Chop Chop the Undead"),
                progress,
            ),
            (
                "portal, eight under a failure",
                giants(),
                SetupView::default(),
                full("Eruptor"),
                Some(long_failure()),
            ),
            (
                "already running",
                TvScreen::AlreadyRunning,
                SetupView::default(),
                PortalCards::default(),
                None,
            ),
            (
                "server error",
                server_error,
                SetupView::default(),
                PortalCards::default(),
                None,
            ),
            (
                "farewell",
                TvScreen::Farewell,
                SetupView::default(),
                PortalCards::default(),
                None,
            ),
        ];
        let roles = theme.text_colors();
        for (w, h, mode) in [
            (1920, 1080, WindowMode::Tv),
            (3840, 2160, WindowMode::Tv),
            (1280, 720, WindowMode::Desktop),
        ] {
            let safe = safe_area(w, h, mode);
            for (what, screen, view, cards, band) in &cases {
                let mut app = portal_app(w, h, mode, cards.clone());
                app.insert_resource(Setup(view.clone()))
                    .insert_resource(ExtractBand(band.clone()));
                app.world_mut().resource_mut::<Current>().screen = screen.clone();
                measure(&mut app);
                let drawn = drawn_text(&mut app);
                let at = format!("{what} at {w}x{h}");
                assert!(!drawn.is_empty(), "{at}: no text");
                for (text, size, color, rect) in &drawn {
                    let FontSize::Px(px) = *size else {
                        panic!("{at}: {text:?} is {size:?}, not a step");
                    };
                    assert!(px >= 24.0 && theme.on_scale(px), "{at}: {text:?} at {px}");
                    assert!(roles.contains(color), "{at}: {text:?} in {color:?}");
                    assert!(
                        contains(safe, *rect),
                        "{at}: {text:?} {rect:?} leaves {safe:?}"
                    );
                }
                for (i, (a, _, _, ra)) in drawn.iter().enumerate() {
                    for (b, _, _, rb) in &drawn[i + 1..] {
                        assert!(ra.intersect(*rb).is_empty(), "{at}: {a:?} overlaps {b:?}");
                    }
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
        // Titles in caps (AP.10); "Starting…" isn't one.
        let steps = [
            (TvScreen::Starting, TvState::Starting, "Starting…"),
            (TvScreen::Preparing, TvState::Preparing, "GETTING READY"),
            (
                TvScreen::Setup,
                TvState::Setup,
                "WHERE ARE YOUR DISC DUMPS?",
            ),
            (TvScreen::Found, TvState::Found, "FOUND YOUR GAMES"),
            (TvScreen::Join, TvState::Join, "SCAN TO PLAY"),
            (giants(), TvState::Portal, "SKYLANDERS: GIANTS"),
            (
                TvScreen::Farewell,
                TvState::Farewell,
                "GOODBYE, PORTAL MASTER!",
            ),
        ];
        for (screen, state, line) in steps {
            app.world_mut().resource_mut::<Current>().screen = screen;
            app.update();
            let found = roots(&mut app);
            assert_eq!(found.len(), 1, "{state:?}: {found:?}");
            assert_eq!(found[0].1, state);
            assert_eq!(*app.world().resource::<State<TvState>>().get(), state);
            let drawn = if state == TvState::Starting {
                texts(&mut app)
            } else {
                titles(&mut app)
            };
            assert!(drawn.contains(&line.to_string()), "{state:?}: {drawn:?}");
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
            drawn.contains(&"SKYLANDERS: SWAP FORCE".to_string()),
            "{drawn:?}"
        );
        assert!(!drawn.contains(&"SKYLANDERS: GIANTS".to_string()));
        assert_eq!(titles(&mut app), ["SKYLANDERS: SWAP FORCE"]);
    }

    /// The titles' layout `Text`s: what each title draws.
    fn titles(app: &mut App) -> Vec<String> {
        let world = app.world_mut();
        let mut q = world.query_filtered::<&Text, With<title::TitleText>>();
        q.iter(world).map(|t| t.0.clone()).collect()
    }

    fn every_screen() -> [TvScreen; 10] {
        [
            TvScreen::Starting,
            TvScreen::Preparing,
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

    /// One root carrying `state` and no other parentless node but its
    /// titles' inks, one per title with a camera each; the node count.
    fn one_root(app: &mut App, state: TvState, step: &str) -> usize {
        let found = roots(app);
        assert_eq!(found.len(), 1, "{step}: {found:?}");
        assert_eq!(found[0].1, state, "{step}");
        assert_eq!(*app.world().resource::<State<TvState>>().get(), state);
        let (all, parentless) = nodes(app);
        let world = app.world_mut();
        let inks: Vec<Entity> = world
            .query_filtered::<Entity, With<title::TitleInk>>()
            .iter(world)
            .collect();
        let surfaces = world.query::<&title::TitleSurface>().iter(world).count();
        let cameras = world
            .query_filtered::<(), With<title::TitleCamera>>()
            .iter(world)
            .count();
        assert_eq!(
            (inks.len(), cameras),
            (surfaces, surfaces),
            "{step}: a title's ink or camera left behind"
        );
        let strays: Vec<Entity> = parentless
            .into_iter()
            .filter(|e| *e != found[0].0 && !inks.contains(e))
            .collect();
        assert!(strays.is_empty(), "{step}: a stray node {strays:?}");
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
        // Each round ends on the same screen with its title drawn: the
        // cameras, the titles' materials and the images there.
        let mut ends = Vec::new();
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
            frames(&mut app, 3);
            let world = app.world_mut();
            let cameras = world.query::<&Camera>().iter(world).count();
            let materials = world.resource::<Assets<title::TitleMaterial>>().len();
            ends.push((cameras, materials, world.resource::<Assets<Image>>().len()));
        }
        // The TV's and the title's cameras, one material, and no image left
        // behind by a round's 162 steps (the first round grows the font
        // atlases, so it's the baseline's warm-up).
        assert_eq!((ends[1].0, ends[1].1), (2, 1), "{ends:?}");
        assert_eq!(ends[1], ends[2], "{ends:?}");

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

    /// AP.2, with AP.6's button: another error's text is patched in place;
    /// the sheet opening swaps the button for a line and a failed pick adds
    /// the error, so the column respawns for both.
    #[test]
    fn the_setup_lines_follow_the_picker() {
        let mut app = portal_app(1920, 1080, WindowMode::Tv, PortalCards::default());
        app.world_mut().resource_mut::<Current>().screen = TvScreen::Setup;
        frames(&mut app, 2);
        // Each line's entity and text; a button reads as `<its id>`.
        let lines = |app: &mut App| {
            let world = app.world_mut();
            let mut columns = world.query_filtered::<&Children, With<SetupLines>>();
            let kids: Vec<Entity> = columns.single(world).unwrap().iter().collect();
            let mut texts = world.query::<&Text>();
            let mut ids = world.query::<&FocusId>();
            kids.into_iter()
                .map(|kid| match ids.get(world, kid) {
                    Ok(id) => (kid, format!("<{}>", id.0)),
                    Err(_) => (kid, texts.get(world, kid).unwrap().0.clone()),
                })
                .collect::<Vec<_>>()
        };
        let drawn = |lines: &[(Entity, String)]| -> Vec<String> {
            lines.iter().map(|(_, t)| t.clone()).collect()
        };
        let or_run = "Or run: guacamole --setup <folder>";
        let before = lines(&mut app);
        assert_eq!(drawn(&before), ["<choose-folder>", or_run]);

        app.world_mut().resource_mut::<Setup>().0.picking = true;
        app.update();
        let picking = lines(&mut app);
        assert_eq!(
            drawn(&picking),
            ["Choose a folder in the window that opened.", or_run]
        );
        assert_ne!(picking[0].0, before[0].0);

        {
            let mut setup = app.world_mut().resource_mut::<Setup>();
            setup.0.picking = false;
            setup.0.error = Some("/Volumes/gone isn't a folder".into());
        }
        app.update();
        let failed = lines(&mut app);
        assert_eq!(
            drawn(&failed),
            [
                "Couldn't use that folder: /Volumes/gone isn't a folder",
                "<choose-folder>",
                or_run,
            ]
        );

        app.world_mut().resource_mut::<Setup>().0.error =
            Some("/Volumes/other isn't a folder".into());
        app.update();
        let again = lines(&mut app);
        let entities = |l: &[(Entity, String)]| l.iter().map(|(e, _)| *e).collect::<Vec<_>>();
        assert_eq!(
            entities(&again),
            entities(&failed),
            "patched, not respawned"
        );
        assert_eq!(
            again[0].1,
            "Couldn't use that folder: /Volumes/other isn't a folder"
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

    /// One star drawn on the sky: what it is, its node's translation in
    /// design px, its centre in target px and what it draws.
    type DrawnStar = (starfield::SkyStar, Val2, Vec2, ImageNode);

    /// The one star layer up: its rect in target px and its stars, in the
    /// order they draw.
    fn star_layer(app: &mut App) -> (Entity, Rect, Vec<DrawnStar>) {
        let world = app.world_mut();
        let mut q = world.query_filtered::<(
            Entity,
            &ComputedNode,
            &UiGlobalTransform,
            &Children,
        ), With<starfield::StarLayer>>();
        let (layer, node, at, kids) = q.single(world).unwrap();
        let (rect, kids) = (node_rect(node, at), kids.to_vec());
        let mut q = world.query::<(
            &starfield::SkyStar,
            &UiTransform,
            &UiGlobalTransform,
            &ImageNode,
            Has<Children>,
        )>();
        let stars = kids
            .iter()
            .map(|kid| {
                let (star, moved, drawn, image, parent) = q.get(world, *kid).unwrap();
                assert!(!parent, "a star is one node");
                (*star, moved.translation, drawn.translation, image.clone())
            })
            .collect();
        (layer, rect, stars)
    }

    /// AP.11: every screen's root sits on the sky, at 1080p, 4K and in the
    /// desktop window: §1's three gradients on the root and a layer of stars
    /// as its first child, over the whole root from the window's corner (not
    /// the inset's), with no text in it and no line moved by it. Since the
    /// 2026-10-18 follow-up the layer holds a node a star, the small ones
    /// first, each drawing its size's sprite in its tint at its alpha where
    /// its copy in the window is with no drift.
    #[test]
    fn every_screen_sits_on_the_starfield() {
        let theme = TvTheme::default();
        let sky = starfield::stars(starfield::SEED);
        for (width, height, mode) in [
            (1920, 1080, WindowMode::Tv),
            (3840, 2160, WindowMode::Tv),
            (1280, 720, WindowMode::Desktop),
        ] {
            let mut app = portal_app(width, height, mode, full("Eruptor"));
            app.insert_resource(ReducedMotion(true));
            let scale = height as f32 / DESIGN_HEIGHT;
            let sprites = app.world().resource::<TvAssets>().stars.clone();
            for screen in every_screen() {
                let case = format!("{width}x{height} {screen:?}");
                app.world_mut().resource_mut::<Current>().screen = screen.clone();
                frames(&mut app, 3);
                let first = {
                    let world = app.world_mut();
                    let mut q = world
                        .query_filtered::<(&BackgroundGradient, &Children), With<ScreenRoot>>();
                    let (sky, children) = q.single(world).unwrap();
                    assert_eq!(*sky, starfield::sky(&theme), "{case}");
                    assert_eq!(sky.0.len(), 3, "{case}");
                    children[0]
                };
                let (layer, rect, stars) = star_layer(&mut app);
                assert_eq!(layer, first, "{case}: the stars draw under the screen");
                let window = Rect::new(0.0, 0.0, width as f32, height as f32);
                assert!(rect.min.abs_diff_eq(window.min, 0.01), "{case}: {rect:?}");
                assert!(rect.max.abs_diff_eq(window.max, 0.01), "{case}: {rect:?}");
                assert_eq!(stars.len(), sky.len(), "{case}");
                let big: Vec<bool> = stars.iter().map(|s| s.3.image == sprites.big).collect();
                assert!(big.is_sorted(), "{case}: the small stars first");
                for star in &sky {
                    let (placed, moved, centre, image) =
                        stars.iter().find(|s| s.0.at == star.at).unwrap();
                    assert_eq!(*moved, placed.placed(Vec2::ZERO).translation, "{case}");
                    let home = starfield::wrap(star.at) * scale;
                    assert!(centre.abs_diff_eq(home, 0.01), "{case}: {centre} {home}");
                    let sprite = if star.big() {
                        &sprites.big
                    } else {
                        &sprites.small
                    };
                    assert_eq!(image.image, *sprite, "{case}");
                    let tint = theme.star_tints[star.tint as usize].with_alpha(star.alpha);
                    assert_eq!(image.color, tint, "{case}");
                }
                assert!(app.world().get::<Text>(layer).is_none(), "{case}");

                // Without the layer every line lands where it did.
                let lines = |app: &mut App| {
                    let mut drawn = drawn_text(app);
                    drawn.sort_by(|a, b| a.0.cmp(&b.0));
                    drawn
                        .into_iter()
                        .map(|(text, _, _, rect)| (text, rect))
                        .collect::<Vec<_>>()
                };
                let with = lines(&mut app);
                app.world_mut().entity_mut(layer).despawn();
                frames(&mut app, 2);
                assert_eq!(lines(&mut app), with, "{case}");
            }
        }
    }

    /// AP.11: the stars drift with the clock, a screen entered later picking
    /// the drift up where the last one was, and hold at their start under
    /// `ReducedMotion`.
    #[test]
    fn the_stars_drift_unless_motion_is_reduced() {
        use bevy::time::TimeUpdateStrategy;
        for reduced in [false, true] {
            let mut app = portal_app(1920, 1080, WindowMode::Tv, PortalCards::default());
            app.insert_resource(ReducedMotion(reduced))
                .insert_resource(TimeUpdateStrategy::ManualDuration(Duration::from_secs(25)));
            app.world_mut()
                .resource_mut::<Time<Virtual>>()
                .set_max_delta(Duration::from_secs(60));
            for screen in [TvScreen::Join, TvScreen::Farewell] {
                app.world_mut().resource_mut::<Current>().screen = screen.clone();
                frames(&mut app, 2);
                let elapsed = app.world().resource::<Time>().elapsed();
                assert!(elapsed >= Duration::from_secs(25), "{elapsed:?}");
                let at = if reduced {
                    Vec2::ZERO
                } else {
                    starfield::drift_at(elapsed)
                };
                assert_ne!(at == Vec2::ZERO, !reduced, "{screen:?} at {elapsed:?}");
                let (_, _, stars) = star_layer(&mut app);
                let mut moved = 0;
                for (placed, translation, centre, _) in &stars {
                    let case = format!("{reduced} {screen:?} {placed:?}");
                    assert_eq!(*translation, placed.placed(at).translation, "{case}");
                    let drawn = placed.centre(at);
                    assert!(centre.abs_diff_eq(drawn, 0.01), "{case}: {centre}");
                    moved += usize::from(!drawn.abs_diff_eq(placed.centre(Vec2::ZERO), 0.01));
                }
                assert_eq!(moved == stars.len(), !reduced, "{reduced} {screen:?}");
            }
        }
    }

    /// The most pairs of `stars` one shift carries one onto the other, to
    /// within about half a px: a pattern that repeats shows as a shift many
    /// pairs share (AP.11's tile at 16:9: 960 px across, 32 pairs), where
    /// scattered stars share one by chance at most twice.
    fn most_pairs_one_shift(stars: &[Vec2]) -> usize {
        let mut shifts = std::collections::HashMap::<IVec2, usize>::new();
        for (i, a) in stars.iter().enumerate() {
            for b in &stars[i + 1..] {
                let d = *b - *a;
                let d = if (d.x, d.y) < (0.0, 0.0) { -d } else { d };
                *shifts.entry((d * 2.0).round().as_ivec2()).or_default() += 1;
            }
        }
        shifts
            .keys()
            .map(|at| {
                (-1..=1)
                    .flat_map(|x| (-1..=1).map(move |y| *at + IVec2::new(x, y)))
                    .filter_map(|near| shifts.get(&near))
                    .sum()
            })
            .max()
            .unwrap_or(0)
    }

    #[test]
    fn a_repeating_pattern_shares_a_shift_and_scattered_stars_dont() {
        let tile: Vec<Vec2> = starfield::stars(starfield::SEED)
            .iter()
            .map(|s| s.at)
            .filter(|at| at.cmplt(Vec2::new(960.0, 540.0)).all())
            .collect();
        assert_eq!(tile.len(), 16);
        let two_by_two: Vec<Vec2> = [0.0, 960.0]
            .iter()
            .flat_map(|x| [0.0, 540.0].map(|y| Vec2::new(*x, y)))
            .flat_map(|corner| tile.iter().map(move |at| *at + corner))
            .collect();
        assert_eq!(most_pairs_one_shift(&two_by_two), 32);
        let scattered: Vec<Vec2> = starfield::stars(starfield::SEED)
            .iter()
            .map(|s| s.at)
            .filter(|at| at.cmplt(Vec2::new(1920.0, 1080.0)).all())
            .collect();
        assert_eq!(scattered.len(), 64);
        assert!(most_pairs_one_shift(&scattered) <= 2);
    }

    /// The AP.11 follow-up (2026-10-18): AP.11's 960×540 tile showed 2×2 on
    /// a 16:9 screen, the same 16 stars in each quarter. On a 16:9 screen at
    /// 1080p and 4K and a 32:9 one at both, from the drift's start and then a
    /// frame every 7 s through a loop and past it, every star on the screen,
    /// a sliver at an edge included, is another star of the sky (no two at
    /// the same offset within its period), each node draws its own star, no
    /// shift carries more than two of them onto others (no pattern repeats,
    /// whatever its period) and every 2×2 cells' patch of the screen holds
    /// one.
    #[test]
    fn no_star_shows_twice_on_a_screen() {
        use bevy::time::TimeUpdateStrategy;
        use starfield::tests::offset_in_period;
        for (width, height) in [(1920, 1080), (3840, 2160), (3840, 1080), (7680, 2160)] {
            let mut app = portal_app(width, height, WindowMode::Tv, PortalCards::default());
            app.insert_resource(TimeUpdateStrategy::ManualDuration(Duration::from_secs(7)));
            app.world_mut()
                .resource_mut::<Time<Virtual>>()
                .set_max_delta(Duration::from_secs(60));
            app.world_mut().resource_mut::<Current>().screen = TvScreen::Join;
            let scale = height as f32 / DESIGN_HEIGHT;
            let screen = Vec2::new(width as f32, height as f32) / scale;
            let patch = 2.0 * starfield::CELL;
            let patches = (screen / patch).as_uvec2();
            let mut shown = 0;
            for frame in 0..22 {
                app.update();
                let elapsed = app.world().resource::<Time>().elapsed();
                let drift = starfield::drift_at(elapsed);
                let case = format!("{width}x{height} frame {frame} at {elapsed:?}");
                let (_, _, stars) = star_layer(&mut app);
                let mut seen: Vec<(Vec2, Vec2)> = Vec::new();
                let mut held = std::collections::BTreeSet::new();
                for (placed, _, centre, _) in &stars {
                    let c = *centre / scale;
                    let reach = Vec2::splat(placed.half);
                    if !(c.cmpgt(-reach).all() && c.cmplt(screen + reach).all()) {
                        continue;
                    }
                    let offset = offset_in_period(c + drift);
                    let own = offset_in_period(placed.at);
                    assert!(offset.abs_diff_eq(own, 0.01), "{case}: {c} draws {own}");
                    for (other, at) in &seen {
                        assert!(
                            !offset.abs_diff_eq(*other, 0.5),
                            "{case}: {c} and {at} are one star"
                        );
                    }
                    seen.push((offset, c));
                    if c.cmpge(Vec2::ZERO).all() && c.cmplt(screen).all() {
                        held.insert((c / patch).as_uvec2().to_array());
                    }
                }
                assert_eq!(held.len() as u32, patches.x * patches.y, "{case}");
                let on: Vec<Vec2> = seen.iter().map(|(_, at)| *at).collect();
                assert!(most_pairs_one_shift(&on) <= 2, "{case}");
                shown = shown.max(seen.len());
            }
            // About one star a cell: 64 cells on 16:9, 128 on 32:9.
            let cells = (screen / starfield::CELL).element_product() as usize;
            assert!((cells..cells + cells / 2).contains(&shown), "{shown}");
        }
    }

    /// AP.12: the owner pip stays in its card's top-right corner (design
    /// decision 8), on eight cards at 1080p, 4K and in the desktop window,
    /// with short names and with long ones under the longest band.
    #[test]
    fn each_pip_sits_in_its_cards_top_right_quadrant() {
        for (w, h, mode) in [
            (1920, 1080, WindowMode::Tv),
            (3840, 2160, WindowMode::Tv),
            (1280, 720, WindowMode::Desktop),
        ] {
            for (name, band) in [
                ("Eruptor", None),
                ("Legendary Chop Chop the Undead", Some(long_failure())),
            ] {
                let mut app = portal_app(w, h, mode, full(name));
                app.insert_resource(ExtractBand(band));
                let laid = measure(&mut app);
                assert_eq!(laid.pips.len(), 8, "{w}x{h} {name}");
                for (slot, pip) in &laid.pips {
                    let card = laid.cards[usize::from(*slot)].1;
                    let quadrant = Rect::from_corners(
                        Vec2::new(card.center().x, card.min.y),
                        Vec2::new(card.max.x, card.center().y),
                    );
                    assert!(
                        contains(quadrant, *pip),
                        "{w}x{h} {name}: pip {slot} {pip:?} leaves {quadrant:?}"
                    );
                }
            }
        }
    }

    /// The card's box, its face, and the bezel, plate and pip `spawn_card`
    /// drew for `slot`.
    fn card_parts(app: &mut App, slot: u8) -> [Entity; 5] {
        let world = app.world_mut();
        let mut cards = world.query::<(Entity, &CardSlot)>();
        let card = cards
            .iter(world)
            .find(|(_, s)| s.0 == slot)
            .map(|(e, _)| e)
            .unwrap();
        let mut found = [
            card,
            Entity::PLACEHOLDER,
            Entity::PLACEHOLDER,
            Entity::PLACEHOLDER,
            Entity::PLACEHOLDER,
        ];
        let mut stack = vec![card];
        while let Some(e) = stack.pop() {
            if world.get::<CardFace>(e).is_some() {
                found[1] = e;
            }
            if world.get::<surfaces::Bezel>(e).is_some() {
                found[2] = e;
            }
            if world.get::<surfaces::Plate>(e).is_some() {
                found[3] = e;
            }
            if world.get::<Pip>(e).is_some() {
                found[4] = e;
            }
            if let Some(kids) = world.get::<Children>(e) {
                stack.extend(kids.iter());
            }
        }
        found
    }

    fn rect_of(app: &App, e: Entity) -> Rect {
        node_rect(
            app.world().get::<ComputedNode>(e).unwrap(),
            app.world().get::<UiGlobalTransform>(e).unwrap(),
        )
    }

    /// AP.12: a figure card is §3.3's framed panel edged in the gold run, its
    /// portrait in §3.1's bezel ringed in the figure's element (the phone's
    /// rule: the portrait covers the plate) or in gold for a figure with no
    /// element, the plate concentric at 80% of the bezel holding the portrait,
    /// and the pip a gold ring round the owner's colour. Since AP.14 the card
    /// is the face over the `CardSlot` box, which wears nothing and keeps its
    /// rect, the face filling it exactly at rest.
    #[test]
    fn a_card_wears_its_materials() {
        let theme = TvTheme::default();
        let dir = tempfile::tempdir().unwrap();
        let thumb = dir.path().join("thumb.png");
        image::RgbaImage::from_pixel(8, 8, image::Rgba([200, 60, 20, 255]))
            .save(&thumb)
            .unwrap();
        let mut eruptor = placement("Eruptor", Some("#e8423f"));
        eruptor.portrait = Some(thumb);
        let mut rex = placement("Tree Rex", None);
        rex.element = None;
        let cards = PortalCards::default().reduce([
            PortalChange::Placed {
                slot: SlotIndex::new(0).unwrap(),
                placement: eruptor,
            },
            PortalChange::Placed {
                slot: SlotIndex::new(1).unwrap(),
                placement: rex,
            },
        ]);
        for (w, h, mode) in [(1920, 1080, WindowMode::Tv), (3840, 2160, WindowMode::Tv)] {
            let mut app = portal_app(w, h, mode, cards.clone());
            measure(&mut app);
            let scale = h as f32 / DESIGN_HEIGHT;
            for (slot, element, portrait) in [(0, Some(Element::Fire), true), (1, None, false)] {
                let at = format!("{w}x{h} slot {slot}");
                let [card, face, bezel, plate, pip] = card_parts(&mut app, slot);
                let world = app.world();
                assert!(world.get::<BorderGradient>(card).is_none(), "{at}");
                assert!(world.get::<BackgroundGradient>(card).is_none(), "{at}");
                assert_eq!(rect_of(&app, face), rect_of(&app, card), "{at}");
                let (edge, fill) = (
                    world.get::<BorderGradient>(face).unwrap(),
                    world.get::<BackgroundGradient>(face).unwrap(),
                );
                let (expected_fill, expected_edge) = {
                    let mut probe = World::new();
                    let e = probe.spawn(surfaces::card(&theme)).id();
                    (
                        probe.get::<BackgroundGradient>(e).unwrap().clone(),
                        probe.get::<BorderGradient>(e).unwrap().clone(),
                    )
                };
                assert_eq!(*edge, expected_edge, "{at}: the gold edge");
                assert_eq!(*fill, expected_fill, "{at}: the panel");
                assert!(
                    world
                        .get::<BorderColor>(face)
                        .unwrap()
                        .top
                        .is_fully_transparent(),
                    "{at}: no flat edge over the gold"
                );
                let ring = &world.get::<BackgroundGradient>(bezel).unwrap().0[0];
                assert_eq!(*ring, surfaces::ring(&theme, element), "{at}");
                assert_eq!(
                    *ring == surfaces::gold_ring(&theme),
                    element.is_none(),
                    "{at}: gold only for a figure with no element"
                );
                assert_eq!(
                    world.get::<ImageNode>(plate).is_some(),
                    portrait,
                    "{at}: the portrait sits on the plate"
                );
                let (outer, inner) = (rect_of(&app, bezel), rect_of(&app, plate));
                assert!(
                    (outer.width() - surfaces::BEZEL * scale).abs() < 1.0,
                    "{at}"
                );
                assert!(
                    inner.center().abs_diff_eq(outer.center(), 1.0),
                    "{at}: {inner:?} in {outer:?}"
                );
                assert!(
                    (inner.width() / outer.width() - 0.8).abs() < 0.01,
                    "{at}: {inner:?} in {outer:?}"
                );
                if slot == 0 {
                    let world = app.world();
                    assert_eq!(
                        Srgba::from(world.get::<BackgroundColor>(pip).unwrap().0).to_hex(),
                        "#E8423F",
                        "{at}: the pip's plate is the owner's"
                    );
                    let ring = world.get::<BorderGradient>(pip).unwrap();
                    assert_eq!(ring.0[0], surfaces::gold_ring(&theme), "{at}");
                    let size = rect_of(&app, pip).width();
                    assert!((size - surfaces::PIP * scale).abs() < 1.0, "{at}: {size}");
                } else {
                    assert_eq!(pip, Entity::PLACEHOLDER, "{at}: no owner, no pip");
                }
            }
        }
    }

    /// AP.12, AP.13: every choice's face is §3.2's blue card: at rest (the
    /// focus not showing) the three-stop fill, a solid `--gb` edge and the two
    /// drops under it; focused, the brightened fill with §3.1's picking glow
    /// between the drops and the face at 1.05, the button's own node unscaled.
    #[test]
    fn every_choice_wears_the_blue_card() {
        use bevy::input_focus::InputFocusVisible;
        let theme = TvTheme::default();
        let mut app = portal_app(1920, 1080, WindowMode::Tv, PortalCards::default());
        app.insert_resource(ReducedMotion(true));
        let mut seen = std::collections::BTreeSet::new();
        for screen in every_screen() {
            app.world_mut().resource_mut::<Current>().screen = screen.clone();
            for showing in [false, true] {
                app.insert_resource(InputFocusVisible(showing));
                frames(&mut app, 2);
                let world = app.world_mut();
                let mut q = world.query::<(Entity, &FocusId, &Children)>();
                let faces: Vec<(Entity, &'static str, Entity)> = q
                    .iter(world)
                    .map(|(button, id, kids)| (button, id.0, kids[0]))
                    .collect();
                for (button, id, face) in faces {
                    let at = format!("{screen:?} {id} showing {showing}");
                    assert!(world.get::<ChoiceFace>(face).is_some(), "{at}");
                    let fill = world.get::<BackgroundGradient>(face).unwrap();
                    let Gradient::Linear(base) = &fill.0[0] else {
                        panic!("{at}: {fill:?}");
                    };
                    let colours: Vec<Color> = base.stops.iter().map(|s| s.color).collect();
                    let shadows = &world.get::<BoxShadow>(face).unwrap().0;
                    let scale = world.get::<UiTransform>(face).unwrap().scale;
                    if showing {
                        assert_eq!(colours, theme.blue_card_lit, "{at}");
                        assert_eq!(shadows.len(), 3, "{at}");
                        assert_eq!(shadows[1].color, theme.focus_glow, "{at}");
                        assert_eq!(scale, Vec2::splat(look::FOCUS_SCALE), "{at}");
                    } else {
                        assert_eq!(colours, theme.blue_card, "{at}");
                        assert_eq!(shadows.len(), 2, "{at}");
                        assert_eq!(scale, Vec2::ONE, "{at}");
                    }
                    let edge = world.get::<BorderColor>(face).unwrap();
                    assert_eq!(edge.top, theme.gold_bright, "{at}");
                    let outer = world.get::<UiTransform>(button).unwrap();
                    assert_eq!(*outer, UiTransform::IDENTITY, "{at}: the button's own node");
                    seen.insert(id);
                }
            }
        }
        assert_eq!(
            seen,
            [CHOOSE_FOLDER, CONTINUE, CLOSE].into(),
            "every choice was drawn"
        );
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

    /// AP.13: the focus moving from one choice to another eases one face down
    /// and the other up through the spring's overshoot, and every other node,
    /// the buttons and the marked lines included, stays exactly where it was
    /// before, during and after; the change writes `Redrawn::Focus`, which
    /// arms a shot.
    #[test]
    fn a_focus_change_moves_no_marked_rect() {
        use bevy::input_focus::{FocusCause, InputFocus};
        use bevy::time::TimeUpdateStrategy;
        let mut app = portal_app(1920, 1080, WindowMode::Tv, PortalCards::default());
        app.init_resource::<Seen>()
            .add_systems(PostUpdate, see)
            .insert_resource(TimeUpdateStrategy::ManualDuration(Duration::from_millis(
                20,
            )));
        app.world_mut().resource_mut::<Current>().screen = TvScreen::ServerError {
            message: "bind 0.0.0.0:8765: address in use".into(),
        };
        frames(&mut app, 2);
        // A second choice beside Close, as the Home menu will have.
        let root = {
            let world = app.world_mut();
            let mut q = world.query_filtered::<Entity, With<ScreenRoot>>();
            q.single(world).unwrap()
        };
        let pen = Pen::new(
            app.world().resource::<TvAssets>(),
            app.world().resource::<TvTheme>(),
        );
        let other = app
            .world_mut()
            .spawn(choice_bundle(&pen, "Try again", "try-again"))
            .id();
        app.world_mut().entity_mut(root).add_child(other);
        let close = choice_tests::choice(&mut app, CLOSE);
        app.world_mut()
            .resource_mut::<InputFocus>()
            .set(close, FocusCause::Navigated);
        frames(&mut app, 20);
        assert_eq!(scale_of(&app, close), look::FOCUS_SCALE);
        assert_eq!(scale_of(&app, other), 1.0);
        let before = outer_nodes(&mut app);
        assert!(before.len() > 4, "{before:?}");
        for marked in [close, other] {
            assert!(before.contains_key(&marked), "the buttons are measured");
        }
        seen(&mut app);

        app.world_mut()
            .resource_mut::<InputFocus>()
            .set(other, FocusCause::Navigated);
        let mut ups = Vec::new();
        let mut downs = Vec::new();
        for frame in 0..20 {
            app.update();
            assert_eq!(outer_nodes(&mut app), before, "frame {frame}");
            ups.push(scale_of(&app, other));
            downs.push(scale_of(&app, close));
        }
        let top = ups.iter().copied().fold(f32::MIN, f32::max);
        assert!(top > look::FOCUS_SCALE, "the pop overshoots: {ups:?}");
        assert!(ups[1] > 1.0 && ups[1] < top, "mid-way: {ups:?}");
        assert!(
            downs.iter().any(|s| *s < 1.0),
            "and back past rest: {downs:?}"
        );
        assert_eq!(ups.last(), Some(&look::FOCUS_SCALE), "{ups:?}");
        assert_eq!(downs.last(), Some(&1.0), "{downs:?}");
        assert_eq!(outer_nodes(&mut app), before, "after");
        let (redrawn, armed) = seen(&mut app);
        assert_eq!(
            redrawn,
            [Redrawn::Focus],
            "a shot is armed, no layout report"
        );
        assert_eq!(armed, 0);
    }

    /// A node's rect as drawn, its transform's scale included (`node_rect`
    /// takes only where its centre is).
    fn drawn_rect(app: &App, e: Entity) -> Rect {
        let size = app.world().get::<ComputedNode>(e).unwrap().size();
        let at = app.world().get::<UiGlobalTransform>(e).unwrap();
        let [a, b] = [-0.5 * size, 0.5 * size].map(|p| at.transform_point2(p));
        Rect::from_corners(a, b)
    }

    /// Each card's face by slot: the entity, its rect as drawn, its
    /// transform and whether it's shown.
    fn faces(app: &mut App) -> std::collections::BTreeMap<u8, (Entity, Rect, UiTransform, bool)> {
        let world = app.world_mut();
        let mut q =
            world.query_filtered::<(Entity, &ChildOf, &UiTransform, &Visibility), With<CardFace>>();
        let found: Vec<_> = q
            .iter(world)
            .map(|(e, box_, t, v)| (e, box_.parent(), *t, *v != Visibility::Hidden))
            .collect();
        found
            .into_iter()
            .map(|(e, box_, t, shown)| {
                let slot = app.world().get::<CardSlot>(box_).unwrap().0;
                (slot, (e, drawn_rect(app, e), t, shown))
            })
            .collect()
    }

    /// The flash over `bezel`'s portrait, if its card was placed.
    fn flash_on(app: &App, bezel: Entity) -> Option<Entity> {
        let kids = app.world().get::<Children>(bezel)?;
        kids.iter()
            .find(|k| app.world().get::<card_motion::ImpactFlash>(*k).is_some())
    }

    /// AP.14, the box's check: the portal's entrance rises its eight cards
    /// in turn, and at every 20 ms frame along the way the mirror reads each
    /// card's box where it rests, none outside the inset and no two
    /// overlapping, every node off the faces keeps its rect, every pip (on
    /// a face, so it moves) stays on its card's box, and the faces as drawn
    /// never overlap each other or leave the window.
    #[test]
    fn the_entrance_rises_without_moving_a_marked_rect() {
        use bevy::time::TimeUpdateStrategy;

        use crate::mirror::{Mirror, TvMirror};
        let (w, h) = (1920, 1080);
        let window = Rect::new(0.0, 0.0, w as f32, h as f32);
        let mirrored = |reduced: bool| {
            let mut app = portal_app(w, h, WindowMode::Tv, full("Eruptor"));
            let mirror = TvMirror::default();
            app.insert_resource(mirror.clone())
                .insert_resource(ReducedMotion(reduced))
                .insert_resource(TimeUpdateStrategy::ManualDuration(Duration::from_millis(
                    20,
                )))
                .add_plugins(Mirror);
            (app, mirror)
        };
        let boxes = |mirror: &crate::mirror::TvMirror| {
            let shot = mirror.0.lock().unwrap().clone();
            let rects: Vec<[f32; 4]> = shot.cards.iter().map(|c| c.rect).collect();
            (shot, rects)
        };
        let rest = {
            let (mut app, mirror) = mirrored(true);
            frames(&mut app, 3);
            boxes(&mirror).1
        };
        assert_eq!(rest.len(), 8);

        let (mut app, mirror) = mirrored(false);
        app.update();
        let outer = outer_nodes(&mut app);
        let (mut waiting, mut rising) = (0, 0);
        for frame in 0..60 {
            let (shot, rects) = boxes(&mirror);
            assert!(shot.cards_outside.is_empty(), "frame {frame}: {shot:?}");
            assert_eq!(shot.overlapping, 0, "frame {frame}");
            assert_eq!(rects, rest, "frame {frame}: a card's box moved");
            assert_eq!(outer_nodes(&mut app), outer, "frame {frame}");
            // The pips ride the faces, and `report_layout` measures them
            // (`pips_off_card`): each stays on its card's box as drawn.
            let pips: Vec<(u8, Entity)> = {
                let world = app.world_mut();
                let mut q = world.query::<(Entity, &Pip)>();
                q.iter(world).map(|(e, p)| (p.0, e)).collect()
            };
            assert_eq!(pips.len(), 8, "frame {frame}");
            for (slot, pip) in pips {
                let [card, ..] = card_parts(&mut app, slot);
                let (card, pip) = (rect_of(&app, card), drawn_rect(&app, pip));
                assert!(
                    contains(card, pip),
                    "frame {frame}: pip {slot} {pip:?} off {card:?}"
                );
            }
            let faces = faces(&mut app);
            assert_eq!(faces.len(), 8);
            let shown: Vec<Rect> = faces.values().filter(|f| f.3).map(|f| f.1).collect();
            for (i, a) in shown.iter().enumerate() {
                assert!(
                    contains(window, *a),
                    "frame {frame}: {a:?} leaves the window"
                );
                for b in &shown[i + 1..] {
                    assert!(a.intersect(*b).is_empty(), "frame {frame}: {a:?} on {b:?}");
                }
            }
            waiting += faces.values().filter(|f| !f.3).count();
            rising += faces
                .values()
                .filter(|f| {
                    let Val::Px(drop) = f.2.translation.y else {
                        panic!("{:?}", f.2)
                    };
                    f.3 && drop > 0.0 && drop < card_motion::RISE_DROP
                })
                .count();
            app.update();
        }
        assert!(waiting > 0, "cards waited their turn, hidden");
        assert!(rising > 8, "and were caught mid-rise");
        for (slot, (_, rect, transform, shown)) in faces(&mut app) {
            assert_eq!(transform, UiTransform::IDENTITY, "slot {slot} at rest");
            assert!(shown, "slot {slot}");
            let r = rest[usize::from(slot)];
            assert!(
                rect.min.abs_diff_eq(Vec2::new(r[0], r[1]), 0.01),
                "slot {slot}: its face over its box at rest, {rect:?} on {r:?}"
            );
        }
    }

    /// AP.14: a placement while the portal is up rises alone, at once, under
    /// its flash; the flash grows past its card's edge and the face clips it
    /// there, so it never reaches a neighbour; its bezel holds still through
    /// the impact while the others breathe, and breathes after.
    #[test]
    fn a_placement_rises_alone_and_its_flash_stays_on_its_card() {
        use bevy::time::TimeUpdateStrategy;
        use bevy::ui::{CalculatedClip, Val2};
        let two = PortalCards::default().reduce([
            PortalChange::Placed {
                slot: SlotIndex::new(0).unwrap(),
                placement: placement("Eruptor", Some("#33cc66")),
            },
            PortalChange::Placed {
                slot: SlotIndex::new(2).unwrap(),
                placement: placement("Spyro", None),
            },
        ]);
        let mut app = portal_app(1920, 1080, WindowMode::Tv, two.clone());
        app.insert_resource(TimeUpdateStrategy::ManualDuration(Duration::from_millis(
            20,
        )));
        // The entrance is over by 1.1 s.
        frames(&mut app, 60);
        assert!(
            faces(&mut app)
                .values()
                .all(|f| f.2 == UiTransform::IDENTITY && f.3)
        );
        *app.world_mut().resource_mut::<PortalCards>() = two.reduce([PortalChange::Placed {
            slot: SlotIndex::new(1).unwrap(),
            placement: placement("Tree Rex", Some("#33cc66")),
        }]);
        app.update();
        let outer = outer_nodes(&mut app);
        let [_, face, bezel, _, _] = card_parts(&mut app, 1);
        let flash = flash_on(&app, bezel).expect("a placement flashes");
        let others = [0, 2].map(|slot| card_parts(&mut app, slot));
        for [_, _, bezel, _, _] in others {
            assert_eq!(flash_on(&app, bezel), None, "the entrance doesn't flash");
        }
        let (mut past_its_edge, mut others_breathe, mut floats_after) = (false, false, false);
        for frame in 0..45u32 {
            let age = frame * 20;
            let world = app.world();
            let lift = |e: Entity| world.get::<UiTransform>(e).unwrap().translation;
            let rise = *world.get::<UiTransform>(face).unwrap();
            assert_ne!(world.get::<Visibility>(face), Some(&Visibility::Hidden));
            if frame == 0 {
                assert_eq!(rise.translation, Val2::px(0.0, card_motion::RISE_DROP));
                assert_eq!(rise.scale, Vec2::splat(card_motion::RISE_SCALE));
            }
            if age >= 500 {
                assert_eq!(rise, UiTransform::IDENTITY, "risen at {age} ms");
            }
            for [_, face, bezel, _, _] in others {
                assert_eq!(
                    *world.get::<UiTransform>(face).unwrap(),
                    UiTransform::IDENTITY,
                    "it rises alone"
                );
                others_breathe |= lift(bezel) != Val2::ZERO;
            }
            if age <= 600 {
                assert_eq!(lift(bezel), Val2::ZERO, "held through the impact, {age} ms");
            } else {
                floats_after |= lift(bezel) != Val2::ZERO;
            }
            let shown = world.get::<Visibility>(flash) != Some(&Visibility::Hidden);
            assert_eq!(shown, age > 0 && age < 600, "the flash at {age} ms");
            if shown {
                let drawn = drawn_rect(&app, flash);
                let card = drawn_rect(&app, face);
                let clip = app.world().get::<CalculatedClip>(flash).unwrap();
                assert!(clip.contains_point(drawn.center()), "{age} ms");
                let corners = [
                    drawn.min,
                    drawn.max,
                    Vec2::new(drawn.min.x, drawn.max.y),
                    Vec2::new(drawn.max.x, drawn.min.y),
                    Vec2::new(drawn.center().x, drawn.min.y),
                ];
                for corner in corners.into_iter().filter(|c| !card.contains(*c)) {
                    past_its_edge = true;
                    assert!(!clip.contains_point(corner), "{age} ms: {corner} drawn");
                }
                for [neighbour, ..] in others {
                    let r = rect_of(&app, neighbour);
                    for p in [r.center(), r.min, r.max] {
                        assert!(!clip.contains_point(p), "{age} ms: on a neighbour");
                    }
                }
            }
            assert_eq!(outer_nodes(&mut app), outer, "{age} ms");
            app.update();
        }
        assert!(
            past_its_edge,
            "at 2× the flash reaches past its card's edge"
        );
        assert!(others_breathe, "the cards at rest float");
        assert!(
            floats_after,
            "and the placed one does once its impact is over"
        );
    }

    /// AP.14: under `ReducedMotion` every card is at rest from the frame it's
    /// drawn, through the entrance and a placement: risen, shown, its bezel
    /// still and no flash.
    #[test]
    fn reduced_motion_holds_every_card_at_rest() {
        use bevy::time::TimeUpdateStrategy;
        let seven = full("Eruptor").reduce([PortalChange::Cleared {
            slot: SlotIndex::new(7).unwrap(),
        }]);
        let mut app = portal_app(1920, 1080, WindowMode::Tv, seven.clone());
        app.insert_resource(ReducedMotion(true)).insert_resource(
            TimeUpdateStrategy::ManualDuration(Duration::from_millis(20)),
        );
        let at_rest = |app: &mut App, step: &str| {
            let faces = faces(app);
            for (slot, (_, _, transform, shown)) in &faces {
                assert_eq!(*transform, UiTransform::IDENTITY, "{step}: slot {slot}");
                assert!(shown, "{step}: slot {slot}");
            }
            let world = app.world_mut();
            let mut bezels = world.query_filtered::<&UiTransform, With<card_motion::Floats>>();
            assert!(
                bezels.iter(world).all(|t| *t == UiTransform::IDENTITY),
                "{step}"
            );
            let mut flashes = world.query_filtered::<&Visibility, With<card_motion::ImpactFlash>>();
            assert!(
                flashes.iter(world).all(|v| *v == Visibility::Hidden),
                "{step}"
            );
            (faces.len(), flashes.iter(world).count())
        };
        for frame in 0..10 {
            app.update();
            assert_eq!(at_rest(&mut app, &format!("entrance {frame}")), (7, 0));
        }
        *app.world_mut().resource_mut::<PortalCards>() = seven.reduce([PortalChange::Placed {
            slot: SlotIndex::new(7).unwrap(),
            placement: placement("Jet-Vac", None),
        }]);
        for frame in 0..10 {
            app.update();
            assert_eq!(at_rest(&mut app, &format!("placement {frame}")), (8, 1));
        }
    }

    /// The portal with the turntables (AE.13) over
    /// `turntable::tests::extract`'s shelf, `slots` placed with its modelled
    /// figure 1; the extract's folder, which the loads read, beside it.
    fn turntable_app(
        width: u32,
        height: u32,
        mode: WindowMode,
        slots: &[u8],
    ) -> (App, tempfile::TempDir) {
        use crate::turntable::tests::{add_turntable, extract, placed};
        let tmp = tempfile::tempdir().unwrap();
        let shelf = extract(tmp.path());
        let cards = PortalCards::default().reduce(slots.iter().map(|s| placed(*s, 1, "One")));
        let mut app = portal_app(width, height, mode, cards);
        add_turntable(&mut app);
        let (tx, feed) = crate::turntable::shelf_channel();
        tx.send(std::sync::Arc::new(shelf)).unwrap();
        app.insert_resource(feed)
            .insert_resource(ReducedMotion(true));
        crate::turntable::tests::settle(&mut app);
        (app, tmp)
    }

    /// Each plate's slot, rect, the image it shows and its card's rect.
    fn plates(app: &mut App) -> Vec<(u8, Rect, Option<AssetId<Image>>)> {
        let world = app.world_mut();
        let mut q = world.query::<(
            &CardPlate,
            &ComputedNode,
            &UiGlobalTransform,
            Option<&ImageNode>,
        )>();
        let mut out: Vec<_> = q
            .iter(world)
            .map(|(p, n, t, i)| (p.slot, node_rect(n, t), i.map(|i| i.image.id())))
            .collect();
        out.sort_by_key(|p| p.0);
        out
    }

    /// AE.13: a full portal's turning models each draw inside their own card
    /// and the TV's inset, at 1080p, 4K and the desktop window.
    #[test]
    fn eight_turntable_images_sit_inside_their_cards_and_the_inset() {
        for (w, h, mode) in [
            (1920, 1080, WindowMode::Tv),
            (3840, 2160, WindowMode::Tv),
            (1280, 720, WindowMode::Desktop),
        ] {
            let (mut app, _extract) = turntable_app(w, h, mode, &[0, 1, 2, 3, 4, 5, 6, 7]);
            let laid = measure(&mut app);
            let safe = safe_area(w, h, mode);
            let turntables = app.world().resource::<Turntables>();
            let images: Vec<_> = (0..8).map(|s| turntables.image(s).unwrap().id()).collect();
            let plates = plates(&mut app);
            assert_eq!(plates.len(), 8, "{w}x{h}");
            for (slot, rect, image) in &plates {
                assert_eq!(
                    *image,
                    Some(images[usize::from(*slot)]),
                    "slot {slot} shows its model"
                );
                let card = laid.cards[usize::from(*slot)].1;
                assert!(
                    rect.width() > 0.0 && contains(card, *rect),
                    "slot {slot}: {rect:?} in {card:?}"
                );
                assert!(
                    contains(safe, *rect),
                    "slot {slot}: {rect:?} leaves {safe:?}"
                );
                for (other, r, _) in &plates {
                    assert!(
                        other == slot || r.intersect(*rect).is_empty(),
                        "{slot} and {other}"
                    );
                }
            }
            // Drawn at the plate's own pixels, so the UI never scales it.
            let size = app.world().resource::<crate::turntable::PlateSize>().0;
            assert_eq!(size.x as f32, plates[0].1.width().ceil(), "{w}x{h}");
            let assets = app.world().resource::<Assets<Image>>();
            for id in &images {
                assert_eq!(assets.get(*id).unwrap().size(), size);
            }
        }
    }

    /// AE.13: leaving the portal and coming back respawns every card, but no
    /// turntable: the new card shows the scene that was already turning.
    #[test]
    fn a_ui_rebuild_keeps_the_turntable_and_shows_it_again() {
        let (mut app, _extract) = turntable_app(1920, 1080, WindowMode::Tv, &[0, 5]);
        let scenes = |app: &App| {
            let t = app.world().resource::<Turntables>();
            (t.stage_entities(0).unwrap(), t.stage_entities(5).unwrap())
        };
        let before = scenes(&app);
        let shown = plates(&mut app);
        let card = cells(&mut app)[&0].1;

        app.world_mut().resource_mut::<Current>().screen = TvScreen::Join;
        frames(&mut app, 3);
        assert!(plates(&mut app).is_empty(), "the portal's gone");
        app.world_mut().resource_mut::<Current>().screen = TvScreen::Portal {
            game: "Skylanders: Giants".into(),
        };
        frames(&mut app, 3);
        assert_ne!(cells(&mut app)[&0].1, card, "the card was respawned");
        assert_eq!(scenes(&app), before, "the scenes weren't");
        let again = plates(&mut app);
        let images = |p: &[(u8, Rect, Option<AssetId<Image>>)]| {
            p.iter().map(|(s, _, i)| (*s, *i)).collect::<Vec<_>>()
        };
        assert_eq!(images(&again), images(&shown));
        assert!(again.iter().all(|(_, _, i)| i.is_some()));

        // A placement in another slot respawns that cell, not slot 1's model.
        let cards = app
            .world()
            .resource::<PortalCards>()
            .clone()
            .reduce([crate::turntable::tests::placed(2, 3, "Three")]);
        app.insert_resource(cards);
        crate::turntable::tests::settle(&mut app);
        assert_eq!(scenes(&app), before);
        let t = app.world().resource::<Turntables>();
        assert_eq!(
            t.card_state(2),
            Some(crate::turntable::CardState::Portrait),
            "the damaged one keeps its portrait"
        );
        let third = plates(&mut app).into_iter().find(|p| p.0 == 2).unwrap();
        assert_eq!(third.2, None, "no portrait in the made-up roster, no model");
    }

    /// AE.25: the start's pass's screen, its title, lines and bar inside
    /// the inset at 1080p and 4K on the TV and in the desktop window, the
    /// line and the bar following the count of keys ready.
    #[test]
    fn the_preparing_screen_lays_out_inside_the_inset_and_counts_up() {
        for (width, height, mode) in [
            (1920, 1080, WindowMode::Tv),
            (3840, 2160, WindowMode::Tv),
            (1280, 720, WindowMode::Desktop),
        ] {
            let mut app = portal_app(width, height, mode, PortalCards::default());
            let mut prepare = Prepare::new(true);
            prepare.set_progress(44, 88);
            app.insert_resource(prepare);
            app.world_mut().resource_mut::<Current>().screen = TvScreen::Preparing;
            let laid = measure(&mut app);
            let safe = safe_area(width, height, mode);
            assert!(laid.texts.len() >= 3, "{width}x{height}: {:?}", laid.texts);
            for t in &laid.texts {
                assert!(
                    contains(safe, *t),
                    "{width}x{height} {mode:?}: {t:?} outside {safe:?}"
                );
            }
            assert!(laid.cards.is_empty() && laid.empties.is_empty());
            let drawn = texts(&mut app);
            assert!(drawn.contains(&"44 of 88 ready".to_string()), "{drawn:?}");
            assert_eq!(titles(&mut app), ["GETTING READY"]);
            let world = app.world_mut();
            let bars: Vec<u8> = world
                .query::<&PrepareBar>()
                .iter(world)
                .map(|b| b.0)
                .collect();
            assert_eq!(bars, [50]);
            // The count moves on: patched in place, no new screen.
            let root = roots(&mut app);
            app.world_mut()
                .resource_mut::<Prepare>()
                .set_progress(88, 88);
            app.update();
            assert!(texts(&mut app).contains(&"88 of 88 ready".to_string()));
            let world = app.world_mut();
            let bars: Vec<u8> = world
                .query::<&PrepareBar>()
                .iter(world)
                .map(|b| b.0)
                .collect();
            assert_eq!(bars, [100]);
            assert_eq!(roots(&mut app), root);
        }
        assert_eq!(prepare_line(0, 0), "");
        assert_eq!(prepare_percent(3, 0), 0);
        assert_eq!(prepare_percent(1, 3), 33);
    }

    /// AE.25: the pass's card is drawn by its own camera and counted by
    /// nothing that counts cards: no card, plate, pip or empty frame on the
    /// TV's camera, the portal's own cards untouched beside it.
    #[test]
    fn the_sample_card_draws_offscreen_and_is_counted_nowhere() {
        let mut app = portal_app(1920, 1080, WindowMode::Tv, full("Eruptor"));
        let laid = measure(&mut app);
        let before = (laid.cards.len(), laid.pips.len());
        let camera = app.world_mut().spawn(Camera2d).id();
        let plate = app
            .world_mut()
            .resource_mut::<Assets<Image>>()
            .add(Image::default());
        let world = app.world_mut();
        let assets = world.remove_resource::<TvAssets>().unwrap();
        let theme = world.resource::<TvTheme>().clone();
        let root = {
            let mut commands = world.commands();
            spawn_sample_card(
                &mut commands,
                &assets,
                &theme,
                camera,
                UVec2::splat(448),
                plate.clone(),
                Duration::ZERO,
            )
        };
        world.flush();
        world.insert_resource(assets);
        frames(&mut app, 3);
        let world = app.world_mut();
        let counted = (
            world.query::<&CardSlot>().iter(world).count(),
            world.query::<&Pip>().iter(world).count(),
        );
        assert_eq!(counted, before);
        assert_eq!(world.query::<&EmptySlot>().iter(world).count(), 0);
        assert_eq!(
            world
                .get::<bevy::ui::UiTargetCamera>(root)
                .map(|c| c.entity()),
            Some(camera)
        );
        assert_eq!(world.query::<&CardPlate>().iter(world).count(), 8);
        // Its face, bezel and plate are there, on its camera alone.
        let mut faces = world.query_filtered::<&bevy::ui::ComputedUiTargetCamera, With<CardFace>>();
        let on: Vec<Option<Entity>> = faces.iter(world).map(|c| c.get()).collect();
        assert_eq!(
            on.iter().filter(|c| **c == Some(camera)).count(),
            1,
            "{on:?}"
        );
        let mut images = world.query::<&ImageNode>();
        assert!(images.iter(world).any(|i| i.image == plate));
    }
}

/// AP.6: the TV's choices as `bevy_ui_widgets` buttons, one test per
/// scenario of the shell delta. Headless has no pointer: a click on a button
/// is the widget's own pointer press and click triggered on its entity (with
/// the mouse button down and up around them), and a click elsewhere is a
/// left press with no pointer event.
#[cfg(test)]
pub(crate) mod choice_tests {
    use bevy::input::ButtonState;
    use bevy::input::gamepad::{
        GamepadButton, GamepadConnection, GamepadConnectionEvent, RawGamepadButtonChangedEvent,
        RawGamepadEvent,
    };
    use bevy::input::keyboard::{Key, KeyboardInput};
    use bevy::input::mouse::MouseButtonInput;
    use bevy::input_focus::InputFocus;
    use bevy::ui::Pressed;
    use guac_server::state::LauncherScreen;

    use super::layout_tests::portal_app;
    use super::*;
    use crate::input::FocusId;
    use crate::picker::{PickerChoices, Picking};

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

    fn frames(app: &mut App, n: usize) {
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
    fn mouse(app: &mut App, state: ButtonState) {
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

        use super::layout_tests::{outer_nodes, scale_of};
        use crate::look::{FOCUS_SCALE, TAP_SCALE};
        use crate::motion::ReducedMotion;
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
}
