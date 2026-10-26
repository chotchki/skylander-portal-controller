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
use std::sync::atomic::Ordering;
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
use guac_server::state::LauncherStatus;
use serde::Serialize;
use skylander_core::SLOT_COUNT;
use skylander_rpcs3_control::Placement;
use tracing::{error, info, warn};

use crate::card_motion::{self, Schedule};
use crate::card_view::{CardImages, slot_frame, spawn_card};
use crate::cards::{PortalCards, drain_portal};
use crate::extraction::{Band, ExtractBand};
use crate::input::FocusId;
use crate::look::{self, ChoiceFace};
use crate::motion::ReducedMotion;
use crate::picker::FolderAsked;
use crate::prepare::Prepare;
use crate::screen::{SetupView, TvScreen, screen_for, while_preparing};
use crate::starfield;
use crate::surfaces::{self, BUTTON_BORDER, BUTTON_RADIUS};
use crate::theme::{Role, Step, TvTheme};
use crate::title;
use crate::turntable::{CardPlate, CardState, TurntableSystems, Turntables};
use crate::tv_state::{
    CardSlot, Current, ExtractBar, ExtractDetail, ExtractTitle, Framing, Pen, Pip, Redrawn,
    ScreenRoot, ServerLink, Setup, TV_ORDER, TvAssets, TvCamera, TvState, UiTree, node_rect,
};

/// How long the farewell shows before the app exits (spec: under 5 s).
pub const FAREWELL_BEAT: Duration = Duration::from_secs(3);

/// How long AlreadyRunning shows before this copy closes itself.
const ALREADY_RUNNING_BEAT: Duration = Duration::from_secs(8);

/// The TV's choices, by the [`FocusId`] the router and the mirror know them by.
pub(crate) const CHOOSE_FOLDER: &str = "choose-folder";
pub(crate) const CONTINUE: &str = "continue";
pub(crate) const CLOSE: &str = "close";

/// The layout's design height; `UiScale` maps it to the window.
pub(crate) const DESIGN_HEIGHT: f32 = 1080.0;

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

pub(crate) static TITAN_ONE: &[u8] =
    include_bytes!("../../server/assets/fonts/TitanOne-Regular.ttf");

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

/// The `Update` systems that redraw part of a screen in place.
#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
struct Redraw;

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
            .add_plugins(Screens)
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

pub(crate) fn poll_status(
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
    commands.spawn((
        Camera2d,
        Camera {
            order: TV_ORDER,
            ..default()
        },
        TvCamera,
        IsDefaultUiCamera,
    ));
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
            Redrawn::Text | Redrawn::Focus | Redrawn::Level | Redrawn::Overlay | Redrawn::Alert => {
            }
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
pub(crate) fn choice_bundle(pen: &Pen, text: &str, id: &'static str) -> impl Bundle + use<> {
    (AutoFocus, other_choice_bundle(pen, text, id))
}

/// [`choice_bundle`]'s choice that isn't focused when it appears: one of
/// several, beside the one that is (AH.26's check, on No).
pub(crate) fn other_choice_bundle(pen: &Pen, text: &str, id: &'static str) -> impl Bundle + use<> {
    (
        Button,
        TabIndex(0),
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

/// The screens' titles: [`Pen`]'s own lines are in `tv_state`, a title
/// goes through the title shader, which sits above it.
impl Pen {
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

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct CardShot {
    /// 1-based, as the phone numbers slots.
    pub slot: u8,
    /// From `PortalCards`, not drawn.
    pub figure_id: String,
    /// Every line drawn on the card outside the pip, top to bottom.
    pub texts: Vec<String>,
    /// An image drawn in the portrait frame: the portrait, or the model.
    pub portrait: bool,
    /// The card's turntable (AE.13): `model` (its model is on the card),
    /// `portrait` (no model to show, or one that couldn't be) or `loading`
    /// (the portrait while the model is read or the extract makes it).
    pub state: String,
    /// Why a `portrait` card has no model, as the log has it.
    pub reason: Option<String>,
    /// The bezel plate the portrait or the model draws in: x, y, width,
    /// height in physical pixels.
    pub plate: Option<[f32; 4]>,
    pub pip: Option<PipShot>,
    /// x, y, width, height in physical pixels.
    pub rect: [f32; 4],
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct PipShot {
    pub initial: String,
    /// The pip's fill as drawn, `#RRGGBB`.
    pub color: String,
    /// From `PortalCards`, not drawn.
    pub profile_id: String,
}

/// The screen as drawn, its part of `/api/_test/tv` (openspec AQ.8): the
/// mirror composes it with the other subsystems' parts.
#[derive(SystemParam)]
pub(crate) struct ScreenShots<'w, 's> {
    current: Option<Res<'w, Current>>,
    assets: Option<Res<'w, TvAssets>>,
    setup: Res<'w, Setup>,
    framing: Res<'w, Framing>,
    portal: Res<'w, PortalCards>,
    camera: Query<'w, 's, &'static Camera, With<TvCamera>>,
    cards: Query<
        'w,
        's,
        (
            Entity,
            &'static CardSlot,
            &'static ComputedNode,
            &'static UiGlobalTransform,
        ),
    >,
    empties: Query<'w, 's, &'static EmptySlot>,
    players_line: Query<'w, 's, &'static Text, With<PlayersLine>>,
    address: Query<'w, 's, &'static Text, With<JoinAddress>>,
    qr: Query<'w, 's, &'static ImageNode, With<JoinQr>>,
    screens: Query<'w, 's, &'static Visibility, With<ScreenRoot>>,
    turntables: Option<Res<'w, Turntables>>,
    plates: Query<
        'w,
        's,
        (
            &'static CardPlate,
            &'static ComputedNode,
            &'static UiGlobalTransform,
        ),
    >,
    tree: UiTree<'w, 's>,
}

/// The screen's fields of the snapshot, and the join QR's image for the
/// corner's code to match.
pub(crate) struct ScreenShot {
    pub(crate) screen: String,
    pub(crate) game: Option<String>,
    pub(crate) message: Option<String>,
    pub(crate) players: Option<usize>,
    pub(crate) join_address: Option<String>,
    /// The join QR drawn on the screen itself.
    pub(crate) join_qr: bool,
    pub(crate) qr: Option<Handle<Image>>,
    pub(crate) found: Vec<String>,
    pub(crate) folders: Vec<String>,
    pub(crate) problems: Vec<String>,
    pub(crate) cards: Vec<CardShot>,
    pub(crate) empty_slots: Vec<u8>,
    pub(crate) window: [f32; 2],
    pub(crate) cards_outside: Vec<u8>,
    pub(crate) overlapping: usize,
}

impl ScreenShots<'_, '_> {
    /// None before the first screen.
    pub(crate) fn shot(&self) -> Option<ScreenShot> {
        let current = self.current.as_ref()?;
        let size = self
            .camera
            .single()
            .ok()
            .and_then(Camera::physical_target_size)
            .unwrap_or_default();
        let safe = safe_area(size.x, size.y, self.framing.0);
        // The screen's tree hidden (the level over it, AG.50) draws none of its
        // cards, lines or QR.
        let screen_shown = self.screens.iter().all(|v| *v != Visibility::Hidden);
        let mut drawn: Vec<(CardShot, Rect)> =
            self.cards
                .iter()
                .filter(|_| screen_shown)
                .map(|(entity, slot, node, at)| {
                    let found = self.tree.walk(entity);
                    let placement = self
                        .portal
                        .slots
                        .get(slot.0 as usize)
                        .and_then(Option::as_ref);
                    let rect = node_rect(node, at);
                    let shot = CardShot {
                        slot: slot.0 + 1,
                        figure_id: placement
                            .map(|p| p.figure_id.as_str().to_string())
                            .unwrap_or_default(),
                        texts: found.0,
                        portrait: found.1,
                        state: self
                            .turntables
                            .as_ref()
                            .and_then(|t| t.card_state(slot.0))
                            .unwrap_or(CardState::Portrait)
                            .name()
                            .into(),
                        reason: self
                            .turntables
                            .as_ref()
                            .and_then(|t| t.portrait_reason(slot.0))
                            .map(str::to_owned),
                        plate: self.plates.iter().find(|(p, ..)| p.slot == slot.0).map(
                            |(_, n, t)| {
                                let r = node_rect(n, t);
                                [r.min.x, r.min.y, r.width(), r.height()]
                            },
                        ),
                        pip: found.2.map(|(initial, fill)| PipShot {
                            initial,
                            color: Srgba::from(fill).to_hex(),
                            profile_id: placement
                                .and_then(|p| p.owner.as_ref())
                                .map(|o| o.profile_id.clone())
                                .unwrap_or_default(),
                        }),
                        rect: [rect.min.x, rect.min.y, rect.width(), rect.height()],
                    };
                    (shot, rect)
                })
                .collect();
        drawn.sort_by_key(|(c, _)| c.slot);
        let cards_outside = drawn
            .iter()
            .filter(|(_, r)| !contains(safe, *r))
            .map(|(c, _)| c.slot)
            .collect();
        let overlapping = drawn
            .iter()
            .enumerate()
            .flat_map(|(i, (_, a))| drawn[i + 1..].iter().map(move |(_, b)| (*a, *b)))
            .filter(|(a, b)| !a.intersect(*b).is_empty())
            .count();
        let mut empty_slots: Vec<u8> = self
            .empties
            .iter()
            .filter(|_| screen_shown)
            .map(|e| e.0 + 1)
            .collect();
        empty_slots.sort();
        let (game, message) = match &current.screen {
            TvScreen::Portal { game } => (Some(game.clone()), None),
            TvScreen::ServerError { message } => (None, Some(message.clone())),
            _ => (None, None),
        };
        let qr = self.assets.as_ref().and_then(|a| a.qr.clone());
        let join_qr = qr.as_ref().is_some_and(|handle| {
            screen_shown && self.qr.iter().any(|image| image.image == *handle)
        });
        Some(ScreenShot {
            screen: current.screen.kind().to_string(),
            game,
            message,
            players: self
                .players_line
                .iter()
                .next()
                .filter(|_| screen_shown)
                .and_then(|t| t.0.split_whitespace().next()?.parse().ok()),
            join_address: self
                .address
                .iter()
                .next()
                .filter(|_| screen_shown)
                .map(|t| t.0.clone()),
            join_qr,
            qr,
            found: self.setup.0.found_titles.clone(),
            folders: self.setup.0.folders.clone(),
            problems: self.setup.0.problems.clone(),
            cards: drawn.into_iter().map(|(c, _)| c).collect(),
            empty_slots,
            window: [size.x as f32, size.y as f32],
            cards_outside,
            overlapping,
        })
    }
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

    /// The shell delta's "Nothing on the join code reacts" (AP.6): a pad
    /// button, a key and a click change nothing on Join.
    #[test]
    fn a_click_never_closes_the_join_screen() {
        use bevy::input::gamepad::GamepadButton;
        use bevy::input::keyboard::Key;

        use crate::test_support::tv::{click_beside, exited, focus, key, pad, tap, tv_on};
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
mod layout_tests;

/// AP.6: the TV's choices as `bevy_ui_widgets` buttons, one test per
/// scenario of the shell delta. Headless has no pointer: a click on a button
/// is the widget's own pointer press and click triggered on its entity (with
/// the mouse button down and up around them), and a click elsewhere is a
/// left press with no pointer event.
#[cfg(test)]
mod choice_tests;
