//! The level's completion screen on the TV (openspec AU.5; AU design
//! decision 2: chotchki, 2026-11-02, "we'll want to draw the level end using
//! the game's art but bevy's ui"). Once the end-of-level sequencing reaches
//! its completion screen's state ([`guacamole::level_end`], AS.53), the TV
//! shows over the held level what the objectives screen's own scripts
//! computed there ([`LevelResults`]): the screen's three tabs (its story
//! goals, its dares, its collections), the focused one's objectives each
//! with its checkmark and its count or time, the level's three stars, the
//! screen's title and its continue prompt. Laid out in Bevy's UI as a TV
//! screen (a modal behind AP's router, its tabs `bevy_ui_widgets` buttons),
//! drawn with the objectives screen's own images and `global.bld`'s bitmap
//! font off the extract's interface part; its sprite tree is never built.
//!
//! The layout is the screen's own (openspec AS.446): each piece one of its
//! templates at the size the file gives it, the right way up (the extract's
//! interface images run bottom row first), placed where its scripts put
//! it ([`Places`]: the panel of three tab columns, 509 by 327 of the
//! original's 640 by 480 units; the rows from (20, 95), 32 apart, each its
//! checkmark, its words at 40, a count ending at 455 or a time in its
//! half-alpha recess box from 360; each earned star in its column's well).
//! The tabs come up in turn, as its scripts run them, until the last or the
//! player's first left or right ([`TabCycle`]). Not drawn: its animation (the
//! rows sliding in, the checkmarks slammed in, the fireworks for three
//! stars) and the dots between a line and its count, there being no
//! recording of a completion screen. The focused tab's name joins the
//! screen's title in the scripts' yellow and no title stands under a column
//! (`UI - Objectives Menu - Item Focus`, openspec AS.585).
//!
//! Its pads as the original's menu takes them: left and right move between
//! the tabs, the accept button (South) on any tab is the screen's continue,
//! which the next step runs (the screen's own shutdown raising the
//! sequencing on). An extract without the art (no interface part yet, or a
//! record or texture missing) shows the results in the TV's own style,
//! saying so in `/api/_test/tv`'s `completion.art`.

use std::collections::HashMap;
use std::path::Path;

use bevy::asset::RenderAssetUsages;
use bevy::image::{CompressedImageFormats, ImageSampler, ImageType};
use bevy::input_focus::AutoFocus;
use bevy::math::CompassOctant;
use bevy::prelude::*;
use bevy::render::renderer::RenderDevice;
use bevy::ui::FocusPolicy;
use bevy::ui_widgets::{Activate, Button};
use guac_extract::intros::IrFormat;
use guac_interface::text::{Justify, Metrics, glyphs};
use guac_ir::interface::{Content, Font};
use guacamole::level_end::{LevelResults, Objective, ObjectiveKind, Status};
use guacamole::scripts::ScriptWorld;
use serde::Serialize;
use tracing::{info, warn};

use crate::hero_scripts::ScriptSource;
use crate::input::{FocusId, TvInput};
use crate::pad::{Face, FacesPrinted, Labels, LevelPads, labels, prompt};
use crate::pause::{ShownAt, glyph};
use crate::play::PlaySystems;
use crate::theme::{Role, Step, TvTheme};
use crate::tv_state::{Pen, TvAssets};

/// Over the HUD (800) and the pause overlays (900), under the portal's
/// alerts (940), a modal's (950) and the join corner (960).
pub(crate) const COMPLETION_Z: i32 = 930;

/// The tabs' ids, for the router's focus and the mirror.
pub(crate) const TABS: [&str; 3] = ["completion-tab-0", "completion-tab-1", "completion-tab-2"];

/// The archives the art comes from: the screen's own, and `global.bld`'s
/// bitmap font.
const SCREEN: &str = "misc/ui_objectivesscreen.bld";
pub(crate) const FONT_ARCHIVE: &str = "permanent/global.bld";

/// The original's screen units to the TV's design pixels (its 640 by 480
/// containers on a 1920 by 1080 design: 2.25 a unit on the height, kept at
/// 2 so the panel sits clear of the title and the prompt).
const UNIT: f32 = 2.0;

/// The screen's templates the layout draws, by name.
const TAB_FOCUS: &str = "UI_EOL_Tab_focus";
const TAB_OFF: &str = "UI_EOL_Tab_nonFocus";
const TAB_LEFT: &str = "UI_EOL_Tab_left";
const TAB_RIGHT: &str = "UI_EOL_Tab_right";
const TAB_TILE: &str = "UI_EOL_Tab_tile";
const RECESS_LEFT: &str = "UI_Recess_left";
const RECESS_RIGHT: &str = "UI_Recess_right";
const RECESS_TILE: &str = "UI_Recess_tile";
const RECESS_TIME: &str = "UI_Recess_Time";
const STAR: &str = "UI_Star";
const CHECK_EMPTY: &str = "UI_Checkmark_(00)_empty";
const CHECK_DONE: &str = "UI_Checkmark_(01)";
const ART: [&str; 13] = [
    TAB_FOCUS,
    TAB_OFF,
    TAB_LEFT,
    TAB_RIGHT,
    TAB_TILE,
    RECESS_LEFT,
    RECESS_RIGHT,
    RECESS_TILE,
    RECESS_TIME,
    STAR,
    CHECK_EMPTY,
    CHECK_DONE,
    "UI_Checkmark_(02)_off",
];

/// The tiles beside the tab heads, as the screen's scripts size them.
const TILE_WIDTH: f32 = 75.0;

/// Whether the completion screen holds the TV: shown, its continue not
/// yet asked. The router takes it as a modal's (its tabs the choices), the
/// hero standing still under it while the level's clock runs on: the
/// sequencing waits in its own state and its fanfare plays (the original's
/// pause plays sound through it), where a held level would stop both.
#[derive(Resource, Debug, Default, Clone, Copy, PartialEq, Eq)]
pub(crate) struct CompletionOpen(pub(crate) bool);

/// Whether the level's end holds the players: from its level end recorded
/// until its level is asked (`Sequence::ending`, openspec AS.125), the
/// original's gameplay pause having disabled its pause menu. The router
/// hands the walking pad nothing then (no walk, no attack, no Start) while
/// the level's clock runs on.
#[derive(Resource, Debug, Default, Clone, Copy, PartialEq, Eq)]
pub(crate) struct LevelEnding(pub(crate) bool);

/// Which of the three tab columns has the focus and shows its objectives:
/// 0 the story goals, 1 the dares, 2 the collections.
#[derive(Resource, Debug, Default, Clone, Copy, PartialEq, Eq)]
pub(crate) struct CompletionTab(pub(crate) usize);

/// The screen's root.
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct CompletionScreen;

/// One image of the screen's, with the size its template gives it.
#[derive(Clone)]
pub(crate) struct Piece {
    pub(crate) image: Handle<Image>,
    pub(crate) size: Vec2,
}

/// What the screen draws with, read off the extract's interface part when
/// it opens: the screen's templates' images by name, and `global.bld`'s
/// `contemp` bitmap font with its atlas.
#[derive(Clone)]
pub(crate) struct Art {
    pub(crate) pieces: HashMap<&'static str, Piece>,
    font: Font,
    atlas: Handle<Image>,
}

#[cfg(test)]
impl Art {
    /// `pieces` with an empty font, for a layout test.
    pub(crate) fn for_tests(pieces: HashMap<&'static str, Piece>) -> Self {
        Self {
            pieces,
            font: Font {
                name: None,
                source: None,
                height: 1.0,
                base_line: 0.0,
                char_padding: 0.0,
                kern_amount: 0.0,
                color: [1.0; 4],
                atlas: String::new(),
                atlas_size: [1, 1],
                glyphs: Vec::new(),
            },
            atlas: Handle::default(),
        }
    }
}

/// The art loaded for the screen on show, or why there's none.
#[derive(Resource, Default)]
struct Loaded(Option<Result<Art, String>>);

pub(crate) struct CompletionPlugin;

impl Plugin for CompletionPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<CompletionOpen>()
            .init_resource::<LevelEnding>()
            .init_resource::<CompletionTab>()
            .init_resource::<TabCycle>()
            .init_resource::<CompletionReport>()
            .init_resource::<Loaded>()
            .add_message::<TvInput>()
            .add_observer(on_continue)
            .add_systems(
                Update,
                (open_completion, tab_moves, show_completion)
                    .chain()
                    .after(PlaySystems),
            );
    }
}

/// The screen is open while the run's sequencing has reached it, the TV
/// shows it, and its continue isn't asked yet.
pub(crate) fn is_open(world: &ScriptWorld) -> bool {
    world.sequence.shows_completion
        && world
            .sequence
            .completion
            .as_ref()
            .is_some_and(|c| !c.continue_asked && !c.continued)
}

/// The screen up while the run's sequencing holds it open and the portal
/// doesn't hold the level: under the portal's alert or an arrival card it
/// steps aside, as AU.6's question does, so the alert reads whole and no
/// continue goes in blind (found at the au-level-end review: a second
/// figure under the screen raised Too many Skylanders over it, and a South
/// there continued the screen unseen); it comes back on the tab it was on.
fn open_completion(
    world: Option<Res<ScriptWorld>>,
    pads: Option<Res<crate::pad::LevelPads>>,
    mut open: ResMut<CompletionOpen>,
    mut ending: ResMut<LevelEnding>,
    mut tab: ResMut<CompletionTab>,
    mut cycle: ResMut<TabCycle>,
    mut was_open: Local<bool>,
) {
    let holds = world.as_deref().is_some_and(|w| w.sequence.ending);
    if ending.0 != holds {
        info!(holds, "the level's end holds the players");
        ending.0 = holds;
    }
    let sequenced = world.as_deref().is_some_and(is_open);
    if sequenced && !*was_open {
        tab.0 = 0;
        let results = world
            .as_deref()
            .and_then(|w| w.sequence.completion.as_ref())
            .and_then(|c| c.results.as_ref().ok());
        cycle.0 = Some(tab_time(results, 0));
    }
    *was_open = sequenced;
    let now = sequenced && !pads.is_some_and(|p| p.held());
    if open.0 != now {
        info!(open = now, "the completion screen");
        open.0 = now;
    }
}

/// The screen's own run through its tabs (`UI - Objectives Menu - Update`
/// and `Set Show Next Tab Timer`, openspec AS.446): each tab shown, its rows
/// coming in 0.15 s apart, then after the screen's one-second delay the
/// next, until the last; the player's left or right ends it. Seconds of the
/// screen's time left on the shown tab, none once it's over.
#[derive(Resource, Debug, Default, Clone, Copy, PartialEq)]
pub(crate) struct TabCycle(pub(crate) Option<f32>);

/// The screen's delay after a tab's last row before the next tab.
pub(crate) const NEXT_TAB_DELAY: f32 = 1.0;
/// Between one row's coming in and the next's.
pub(crate) const ROW_DELAY: f32 = 0.15;

/// How long tab `tab` of `results` stays before the next: its rows in turn,
/// then the screen's delay (the rows' own slide not counted: no recording).
pub(crate) fn tab_time(results: Option<&LevelResults>, tab: usize) -> f32 {
    let rows = results.map_or(0, |r| {
        r.objectives
            .iter()
            .filter(|o| o.category as usize == tab)
            .count()
    });
    rows as f32 * ROW_DELAY + NEXT_TAB_DELAY
}

/// Left and right move between the tabs, ending the screen's own run
/// through them; while it runs, its time moves it on.
fn tab_moves(
    mut inputs: MessageReader<TvInput>,
    open: Res<CompletionOpen>,
    world: Option<Res<ScriptWorld>>,
    time: Res<Time<Real>>,
    mut tab: ResMut<CompletionTab>,
    mut cycle: ResMut<TabCycle>,
) {
    let read: Vec<TvInput> = inputs.read().copied().collect();
    if !open.0 {
        return;
    }
    for input in read {
        let next = match input {
            TvInput::Move(CompassOctant::West) => tab.0.saturating_sub(1),
            TvInput::Move(CompassOctant::East) => (tab.0 + 1).min(TABS.len() - 1),
            _ => continue,
        };
        cycle.0 = None;
        if next != tab.0 {
            tab.0 = next;
        }
    }
    if let Some(left) = cycle.0 {
        let left = left - time.delta_secs();
        if left > 0.0 {
            cycle.0 = Some(left);
        } else if tab.0 + 1 < TABS.len() {
            tab.0 += 1;
            let results = world
                .as_deref()
                .and_then(|w| w.sequence.completion.as_ref())
                .and_then(|c| c.results.as_ref().ok());
            cycle.0 = (tab.0 + 1 < TABS.len()).then(|| tab_time(results, tab.0));
        } else {
            cycle.0 = None;
        }
    }
}

/// The accept button on a tab: the screen's continue, run by the next step.
fn on_continue(
    activate: On<Activate>,
    ids: Query<&FocusId>,
    open: Res<CompletionOpen>,
    world: Option<ResMut<ScriptWorld>>,
) {
    let Ok(id) = ids.get(activate.entity) else {
        return;
    };
    if !open.0 || !TABS.contains(&id.0) {
        return;
    }
    if let Some(c) = world.and_then(|w| w.into_inner().sequence.completion.as_mut()) {
        info!(tab = id.0, "the completion screen's continue");
        c.continue_asked = true;
    }
}

/// What the screen draws: the run's results while it's open, its art
/// loaded once a screen.
#[allow(clippy::too_many_arguments)]
fn show_completion(
    mut commands: Commands,
    open: Res<CompletionOpen>,
    tab: Res<CompletionTab>,
    world: Option<Res<ScriptWorld>>,
    source: Option<Res<ScriptSource>>,
    (assets, theme, time): (Option<Res<TvAssets>>, Option<Res<TvTheme>>, Res<Time<Real>>),
    (mut images, device): (ResMut<Assets<Image>>, Option<Res<RenderDevice>>),
    (pads, gamepads): (Res<LevelPads>, Query<(&Gamepad, Option<&FacesPrinted>)>),
    mut loaded: ResMut<Loaded>,
    mut report: ResMut<CompletionReport>,
    mut shown: Local<Option<(usize, Entity)>>,
) {
    let completion = world.as_deref().and_then(|w| w.sequence.completion.clone());
    let want = open.0.then_some(tab.0);
    if shown.map(|(t, _)| t) != want {
        if let Some((_, entity)) = shown.take()
            && let Ok(mut root) = commands.get_entity(entity)
        {
            root.despawn();
        }
        if want.is_none() {
            loaded.0 = None;
        }
    }
    if let (Some(t), None) = (want, *shown) {
        if loaded.0.is_none() {
            let formats = device.as_deref().map_or(CompressedImageFormats::BC, |d| {
                CompressedImageFormats::from_features(d.features())
            });
            let art = match source.as_deref().and_then(ScriptSource::extract) {
                Some((dir, ir)) => load_art(dir, *ir, &mut images, formats),
                None => Err("the run's extract isn't known".into()),
            };
            if let Err(why) = &art {
                warn!(reason = %why, "the completion screen's art didn't load: the TV's own style");
            }
            loaded.0 = Some(art);
        }
        if let (Some(assets), Some(theme), Some(c)) = (assets, theme, &completion) {
            let pen = Pen::new(&assets, &theme);
            let label = pads
                .seat()
                .and_then(|s| s.walking())
                .and_then(|e| gamepads.get(e).ok())
                .map_or(Labels::Position, |(pad, printed)| labels(pad, printed));
            let art = loaded.0.as_ref().and_then(|a| a.as_ref().ok());
            let root = spawn(&mut commands, &pen, art, c.results.as_ref().ok(), t, label);
            commands.entity(root).insert(ShownAt(time.elapsed()));
            *shown = Some((t, root));
        }
    }
    let next = CompletionReport::of(
        open.0,
        tab.0,
        completion.as_ref(),
        loaded.0.as_ref(),
        shown.map(|_| time.elapsed().as_secs_f64() * 1e3),
    );
    if *report != next {
        *report = CompletionReport {
            shown_ms: report.shown_ms.filter(|_| next.shown).or(next.shown_ms),
            ..next
        };
    }
}

/// The screen's art off the extract at `dir`: its templates' images by name
/// at the sizes they give, and `global.bld`'s bitmap font with its atlas.
/// Why not when a record, a template or a texture is missing.
pub(crate) fn load_art(
    dir: &Path,
    ir: IrFormat,
    images: &mut Assets<Image>,
    formats: CompressedImageFormats,
) -> Result<Art, String> {
    load_art_of(dir, ir, SCREEN, &ART, images, formats)
}

/// `names`' images off `archive`'s templates at the sizes they give, and
/// `global.bld`'s bitmap font with its atlas: what a screen in the disc's art
/// draws with (openspec AU.5, AU.29).
pub(crate) fn load_art_of(
    dir: &Path,
    ir: IrFormat,
    archive: &str,
    names: &[&'static str],
    images: &mut Assets<Image>,
    formats: CompressedImageFormats,
) -> Result<Art, String> {
    let read = |archive: &str| {
        guacamole::interface_host::read_record(dir, ir, &guac_extract::interface::path(ir, archive))
    };
    let screen = read(archive)?;
    let global = read(FONT_ARCHIVE)?;
    let mut image = |rel: &str| -> Result<Handle<Image>, String> {
        let bytes = std::fs::read(dir.join(rel)).map_err(|e| format!("{rel}: {e}"))?;
        let img = Image::from_buffer(
            &bytes,
            ImageType::Extension("ktx2"),
            formats,
            true,
            ImageSampler::linear(),
            RenderAssetUsages::RENDER_WORLD,
        )
        .map_err(|e| format!("{rel}: {e}"))?;
        Ok(images.add(img))
    };
    let mut pieces = HashMap::new();
    for &name in names {
        let t = screen
            .templates
            .iter()
            .find(|t| t.name == name)
            .ok_or_else(|| format!("{archive} holds no {name}"))?;
        let Content::Image(a) = &t.content else {
            return Err(format!("{name} draws no image"));
        };
        let rel = a
            .texture
            .as_deref()
            .ok_or_else(|| format!("{name}: no texture"))?;
        pieces.insert(
            name,
            Piece {
                image: image(rel)?,
                size: Vec2::new(t.width, t.height),
            },
        );
    }
    let font = global
        .fonts
        .iter()
        .find(|f| f.name.as_deref().is_some_and(|n| n.ends_with("contemp")))
        .cloned()
        .ok_or("global.bld holds no contemp font")?;
    let atlas = image(&font.atlas)?;
    Ok(Art {
        pieces,
        font,
        atlas,
    })
}

/// An objective's line as the screen reads it: its description, a time's
/// `@` filled with the target time (the screen fills it through a string
/// converter).
pub(crate) fn line(o: &Objective) -> String {
    let d = o.description.clone().unwrap_or_default();
    match o.kind {
        ObjectiveKind::Time => d.replace('@', &clock(o.total)),
        _ => d,
    }
}

/// What an objective shows at its right: N/M for a count (a count the
/// screen's scripts left below 0, unread, as 0), the time taken for a time.
pub(crate) fn tally(o: &Objective) -> Option<String> {
    match o.kind {
        ObjectiveKind::Check => None,
        ObjectiveKind::Count => Some(format!("{}/{}", o.count.max(0.0), o.total.max(0.0))),
        ObjectiveKind::Time => Some(clock(o.count)),
    }
}

/// Seconds as minutes and seconds (`4:20`).
pub(crate) fn clock(seconds: f32) -> String {
    let s = seconds.max(0.0) as u32;
    format!("{}:{:02}", s / 60, s % 60)
}

/// The screen's root, over the held level: the title, the panel of three
/// tab columns with the focused tab's objectives in it, the stars and the
/// continue prompt. In the disc's art when `art` holds it, the TV's own
/// style otherwise.
pub(crate) fn spawn(
    commands: &mut Commands,
    pen: &Pen,
    art: Option<&Art>,
    results: Option<&LevelResults>,
    tab: usize,
    labels: Labels,
) -> Entity {
    let theme = &pen.theme;
    let empty = LevelResults::default();
    let r = results.unwrap_or(&empty);
    let gold = theme.gold_bright;
    commands
        .spawn((
            CompletionScreen,
            Node {
                position_type: PositionType::Absolute,
                left: px(0),
                top: px(0),
                width: percent(100),
                height: percent(100),
                flex_direction: FlexDirection::Column,
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                row_gap: px(24),
                ..default()
            },
            BackgroundColor(theme.shadow.with_alpha(0.75)),
            FocusPolicy::Block,
            GlobalZIndex(COMPLETION_Z),
        ))
        .with_children(|root| {
            let title = r.title.clone().unwrap_or_else(|| "Level complete".into());
            // The focused tab's name after the screen's title, a space
            // between, in its own colour (`UI - Objectives Menu - Item
            // Focus`, openspec AS.585).
            root.spawn((
                ScreenTitle,
                Node {
                    flex_direction: FlexDirection::Row,
                    align_items: AlignItems::Center,
                    column_gap: px(16),
                    ..default()
                },
            ))
            .with_children(|line| {
                words(line, art, pen, &title, 52.0, gold, Step::Lg, Role::Accent);
                let mut name = line.spawn((TabTitle, Node::default()));
                name.with_children(|name| match art {
                    Some(a) => bitmap_text(name, a, &tab_name(r, tab), 52.0, TAB_TITLE_COLOUR),
                    None => {
                        name.spawn(pen.inked(
                            &tab_name(r, tab),
                            Step::Lg,
                            TAB_TITLE_COLOUR,
                            bevy::text::Justify::Center,
                        ));
                    }
                });
            });
            panel(root, art, pen, r, tab);
            if art.is_none() {
                stars(root, pen, &r.stars);
            }
            root.spawn(Node {
                flex_direction: FlexDirection::Row,
                align_items: AlignItems::Center,
                column_gap: px(16),
                ..default()
            })
            .with_children(|line| {
                // The disc's prompt draws its own button (the font's glyph
                // for `<`); the TV's own names the button by the pad's
                // labels.
                match (art, &r.prompt) {
                    (Some(_), Some(p)) => {
                        words(
                            line,
                            art,
                            pen,
                            p,
                            34.0,
                            Color::WHITE,
                            Step::Body,
                            Role::Primary,
                        );
                    }
                    _ => {
                        glyph(line, pen, prompt(labels, Face::South));
                        line.spawn(pen.text("Continue", Step::Body, Role::Primary));
                    }
                }
            });
        })
        .id()
}

/// The screen's title line (openspec AS.585).
#[derive(Component)]
pub(crate) struct ScreenTitle;

/// The focused tab's name in the title line (openspec AS.585).
#[derive(Component)]
pub(crate) struct TabTitle;

/// The colour `UI - Objectives Menu - Item Focus` gives the focused tab's
/// name in the title: the file's literal, a yellow.
pub(crate) const TAB_TITLE_COLOUR: Color = Color::srgb_u8(240, 240, 20);

/// Tab `i`'s name: the scripts' own, else ours.
pub(crate) fn tab_name(r: &LevelResults, i: usize) -> String {
    r.tabs
        .get(i)
        .cloned()
        .flatten()
        .unwrap_or_else(|| ["Story", "Challenges", "Finds"][i.min(2)].to_owned())
}

/// A line of text: in the disc's bitmap font at `size` design pixels high
/// and `color` when `art` holds it, else the TV's own at `step` and `role`.
#[allow(clippy::too_many_arguments)]
fn words(
    parent: &mut ChildSpawnerCommands,
    art: Option<&Art>,
    pen: &Pen,
    text: &str,
    size: f32,
    color: Color,
    step: Step,
    role: Role,
) {
    match art {
        Some(a) => bitmap_text(parent, a, text, size, color),
        None => {
            parent.spawn(pen.text(text, step, role));
        }
    }
}

/// What the atlas's glyph bodies read as in the TV's light: the disc's font
/// holds them mid-grey (0x80, in sRGB) and its fragment program doubles a
/// tinted texel (`guac_interface::draw::shade`), so a white tint draws
/// white.
const GLYPH_BODY: f32 = 0.2158;

/// The tint that draws a glyph's body in `color`: Bevy's UI multiplies in
/// linear light and doesn't double, so a tint of `color` itself leaves it at
/// the atlas's grey (openspec AU.29's review).
pub(crate) fn glyph_tint(color: Color) -> Color {
    let c = color.to_linear();
    Color::LinearRgba(LinearRgba::new(
        c.red / GLYPH_BODY,
        c.green / GLYPH_BODY,
        c.blue / GLYPH_BODY,
        c.alpha,
    ))
}

/// `text` in the bitmap font, one image node a glyph cut from its atlas,
/// laid out by the interface's measure (`guac_interface::text::glyphs`),
/// `size` design pixels for the font's own height, its bodies in `color`.
pub(crate) fn bitmap_text(
    parent: &mut ChildSpawnerCommands,
    art: &Art,
    text: &str,
    size: f32,
    color: Color,
) {
    let tint = glyph_tint(color);
    let units: Vec<u16> = text.encode_utf16().collect();
    let m = Metrics::new(&art.font);
    let quads = glyphs(&m, f32::MAX, Justify::Left, &units);
    let scale = size / art.font.height.max(1.0);
    let width = quads.iter().map(|q| q.rect[2]).fold(0.0f32, f32::max) * scale;
    let atlas = Vec2::new(art.font.atlas_size[0] as f32, art.font.atlas_size[1] as f32);
    parent
        .spawn(Node {
            width: px(width),
            height: px(size),
            flex_shrink: 0.0,
            ..default()
        })
        .with_children(|run| {
            for q in quads {
                run.spawn((
                    Node {
                        position_type: PositionType::Absolute,
                        left: px(q.rect[0] * scale),
                        top: px(q.rect[1] * scale),
                        width: px((q.rect[2] - q.rect[0]) * scale),
                        height: px((q.rect[3] - q.rect[1]) * scale),
                        ..default()
                    },
                    ImageNode {
                        color: tint,
                        rect: Some(Rect::new(
                            q.uv[0] * atlas.x,
                            q.uv[1] * atlas.y,
                            q.uv[2] * atlas.x,
                            q.uv[3] * atlas.y,
                        )),
                        ..ImageNode::new(art.atlas.clone())
                    },
                ));
            }
        });
}

/// One of the screen's pieces at its template's size (`w` its width when
/// the scripts set one), upright: the extract's interface images run bottom
/// row first, so drawn as they are a checkmark and a star point down
/// (openspec AS.446, chotchki's shot of 10:45).
fn piece(parent: &mut ChildSpawnerCommands, art: &Art, name: &str, w: Option<f32>, alpha: f32) {
    let Some(p) = art.pieces.get(name) else {
        return;
    };
    parent.spawn((
        Node {
            width: px(w.unwrap_or(p.size.x) * UNIT),
            height: px(p.size.y * UNIT),
            flex_shrink: 0.0,
            ..default()
        },
        upright(p, alpha),
    ));
}

/// `p`'s image the right way up, at `alpha`.
fn upright(p: &Piece, alpha: f32) -> ImageNode {
    ImageNode {
        color: Color::WHITE.with_alpha(alpha),
        flip_y: true,
        ..ImageNode::new(p.image.clone())
    }
}

/// Where the screen's scripts put a tab's pieces, in the original's units
/// off the panel's top-left (`UI - Objectives Menu - Create Tab`, `Create
/// Entry`, `Create Star`, `Create Time Box`, read on the disc for AS.446):
/// the panel the left end, a 75-unit tile, the three columns, a tile and
/// the right end, 509 by 327; the rows from (20, 95), 32 apart, each its
/// checkmark centred on its line, its words from 40, a count ending at 455
/// or a time box from 360; each column's star centred 121 over the panel's
/// middle and 102 apart from 105 left of it.
pub(crate) struct Places;

impl Places {
    pub(crate) const ENTRY: Vec2 = Vec2::new(20.0, 95.0);
    pub(crate) const ROW_STEP: f32 = 32.0;
    pub(crate) const WORDS: f32 = 40.0;
    pub(crate) const COUNT_END: f32 = 455.0;
    pub(crate) const TIME_BOX: f32 = 360.0;
    pub(crate) const TIME_WIDTH: f32 = 75.0;
    pub(crate) const STAR_FIRST: f32 = -105.0;
    pub(crate) const STAR_STEP: f32 = 102.0;
    pub(crate) const STAR_RISE: f32 = 121.0;
    /// A star's size over its image's: halved, then 1.1 times.
    pub(crate) const STAR_SCALE: f32 = 0.55;

    /// Row `i`'s line (its middle), off the panel's top-left.
    pub(crate) fn row(i: usize) -> Vec2 {
        Self::ENTRY + Vec2::new(0.0, Self::ROW_STEP * i as f32)
    }

    /// Tab `i`'s star's middle in a panel `size` units.
    pub(crate) fn star(i: usize, size: Vec2) -> Vec2 {
        size / 2.0
            + Vec2::new(
                Self::STAR_FIRST + Self::STAR_STEP * i as f32,
                -Self::STAR_RISE,
            )
    }
}

/// The panel: the three tab columns side by side (the focused one's art
/// lit), each a button behind the router with its title on its head, the
/// focused tab's objectives over them and each earned star in its well,
/// all where the screen's scripts put them ([`Places`]).
fn panel(
    parent: &mut ChildSpawnerCommands,
    art: Option<&Art>,
    pen: &Pen,
    r: &LevelResults,
    tab: usize,
) {
    let theme = &pen.theme;
    let head = art.and_then(|a| a.pieces.get(TAB_FOCUS));
    let height = head.map_or(327.0, |p| p.size.y) * UNIT;
    let end = art
        .and_then(|a| a.pieces.get(TAB_LEFT))
        .map_or(0.0, |p| p.size.x);
    let size = Vec2::new(
        2.0 * (end + TILE_WIDTH) + 3.0 * head.map_or(80.0, |p| p.size.x),
        height / UNIT,
    );
    parent
        .spawn(Node {
            flex_direction: FlexDirection::Row,
            height: px(height),
            ..default()
        })
        .with_children(|row| {
            if let Some(a) = art {
                piece(row, a, TAB_LEFT, None, 1.0);
                piece(row, a, TAB_TILE, Some(TILE_WIDTH), 1.0);
            }
            for (i, id) in TABS.iter().enumerate() {
                let focused = i == tab;
                let mut button = row.spawn((
                    Button,
                    FocusId(id),
                    Node {
                        width: px(head.map_or(160.0, |p| p.size.x * UNIT)),
                        height: px(height),
                        flex_shrink: 0.0,
                        ..default()
                    },
                ));
                if focused {
                    button.insert(AutoFocus);
                }
                match art.and_then(|a| a.pieces.get(if focused { TAB_FOCUS } else { TAB_OFF })) {
                    Some(p) => {
                        button.insert(upright(p, 1.0));
                    }
                    None => {
                        button.insert((
                            BackgroundColor(if focused {
                                theme.panel[2]
                            } else {
                                theme.panel[0]
                            }),
                            BorderColor::all(if focused { theme.gold } else { theme.panel[1] }),
                        ));
                    }
                }
            }
            if let Some(a) = art {
                piece(row, a, TAB_TILE, Some(TILE_WIDTH), 1.0);
                piece(row, a, TAB_RIGHT, None, 1.0);
            }
        })
        .with_children(|row| {
            // The focused tab's objectives, a line each from the scripts'
            // first entry down.
            for (i, o) in r
                .objectives
                .iter()
                .filter(|o| o.category as usize == tab)
                .enumerate()
            {
                objective(row, art, pen, o, Places::row(i));
            }
            // Each tab's star in its well, an earned one alone (the well's
            // own art shows an empty one).
            if let Some(star) = art.and_then(|a| a.pieces.get(STAR)) {
                let side = star.size.x * Places::STAR_SCALE;
                for (i, earned) in r.stars.iter().enumerate().take(TABS.len()) {
                    if !earned {
                        continue;
                    }
                    let at = Places::star(i, size);
                    row.spawn((
                        Node {
                            position_type: PositionType::Absolute,
                            left: px((at.x - side / 2.0) * UNIT),
                            top: px((at.y - side / 2.0) * UNIT),
                            width: px(side * UNIT),
                            height: px(side * UNIT),
                            ..default()
                        },
                        upright(star, 1.0),
                    ));
                }
            }
        });
}

/// A piece of a row, `left` units from the row's start and centred on its
/// line, `w` units wide.
fn on_line(left: f32, w: f32, h: f32) -> Node {
    Node {
        position_type: PositionType::Absolute,
        left: px(left * UNIT),
        top: px((Places::ROW_STEP - h) / 2.0 * UNIT),
        width: px(w * UNIT),
        height: px(h * UNIT),
        flex_direction: FlexDirection::Row,
        align_items: AlignItems::Center,
        ..default()
    }
}

/// One objective's row with its line at `at` (units off the panel's
/// top-left), as `Create Entry` builds it: its checkmark at its start, its
/// words at 40, a count's N/M ending at 455, a time's box (the recess at
/// half alpha, its clock and the time) from 360.
fn objective(
    parent: &mut ChildSpawnerCommands,
    art: Option<&Art>,
    pen: &Pen,
    o: &Objective,
    at: Vec2,
) {
    let theme = &pen.theme;
    let mut row = parent.spawn(Node {
        position_type: PositionType::Absolute,
        left: px(at.x * UNIT),
        top: px((at.y - Places::ROW_STEP / 2.0) * UNIT),
        width: px((Places::COUNT_END + 10.0) * UNIT),
        height: px(Places::ROW_STEP * UNIT),
        ..default()
    });
    if art.is_none() {
        row.insert(BackgroundColor(theme.panel[0].with_alpha(0.8)));
    }
    row.with_children(|line| {
        let check = if o.status.done() {
            CHECK_DONE
        } else {
            CHECK_EMPTY
        };
        match art.and_then(|a| a.pieces.get(check)) {
            Some(p) => {
                // The container its checkmark sits in is the done one's
                // size; the mark centred in it.
                let side = art
                    .and_then(|a| a.pieces.get(CHECK_DONE))
                    .map_or(p.size.x, |d| d.size.x);
                line.spawn(on_line(0.0, side, side)).with_children(|c| {
                    c.spawn((
                        Node {
                            width: px(p.size.x * UNIT),
                            height: px(p.size.y * UNIT),
                            margin: UiRect::all(Val::Auto),
                            ..default()
                        },
                        upright(p, 1.0),
                    ));
                });
            }
            None => {
                let mark = if o.status.done() { "[x]" } else { "[ ]" };
                line.spawn(on_line(0.0, Places::WORDS, Places::ROW_STEP))
                    .with_children(|c| {
                        c.spawn(pen.text(mark, Step::Caption, Role::Accent));
                    });
            }
        }
        let color = if o.status == Status::NewlyDone {
            theme.gold_bright
        } else {
            Color::WHITE
        };
        let words_end = match o.kind {
            ObjectiveKind::Time => Places::TIME_BOX,
            _ => Places::COUNT_END,
        };
        line.spawn(on_line(
            Places::WORDS,
            words_end - Places::WORDS,
            Places::ROW_STEP,
        ))
        .with_children(|w| {
            words(
                w,
                art,
                pen,
                &self::line(o),
                24.0,
                color,
                Step::Caption,
                Role::Primary,
            );
        });
        match (o.kind, tally(o)) {
            (ObjectiveKind::Time, Some(t)) => time_box(line, art, pen, &t),
            (_, Some(t)) => {
                // Right-justified, its end at 455.
                let mut n = on_line(0.0, Places::COUNT_END, Places::ROW_STEP);
                n.justify_content = JustifyContent::FlexEnd;
                line.spawn(n).with_children(|w| {
                    words(
                        w,
                        art,
                        pen,
                        &t,
                        24.0,
                        theme.gold_bright,
                        Step::Caption,
                        Role::Accent,
                    );
                });
            }
            _ => {}
        }
    });
}

/// A time's box from 360 on its row (`Create Time Box`): the recess's two
/// ends and its 75-unit tile at half alpha, its clock 5 in at 0.7 of its
/// size, the time right-justified 10 in from its end.
fn time_box(line: &mut ChildSpawnerCommands, art: Option<&Art>, pen: &Pen, time: &str) {
    let ends = art.and_then(|a| a.pieces.get(RECESS_LEFT)).map(|p| p.size);
    let h = ends.map_or(Places::ROW_STEP, |s| s.y);
    let end_w = ends.map_or(0.0, |s| s.x);
    let width = 2.0 * end_w + Places::TIME_WIDTH;
    line.spawn(on_line(Places::TIME_BOX, width, h))
        .with_children(|b| {
            if let Some(a) = art {
                piece(b, a, RECESS_LEFT, None, 0.5);
                piece(b, a, RECESS_TILE, Some(Places::TIME_WIDTH), 0.5);
                piece(b, a, RECESS_RIGHT, None, 0.5);
                if let Some(clock) = a.pieces.get(RECESS_TIME) {
                    let side = clock.size * 0.7;
                    b.spawn((
                        Node {
                            position_type: PositionType::Absolute,
                            left: px(5.0 * UNIT),
                            top: px((h - side.y) / 2.0 * UNIT),
                            width: px(side.x * UNIT),
                            height: px(side.y * UNIT),
                            ..default()
                        },
                        upright(clock, 1.0),
                    ));
                }
            }
            b.spawn(Node {
                position_type: PositionType::Absolute,
                left: px(0),
                right: px(10.0 * UNIT),
                top: px(0),
                bottom: px(0),
                flex_direction: FlexDirection::Row,
                justify_content: JustifyContent::FlexEnd,
                align_items: AlignItems::Center,
                ..default()
            })
            .with_children(|w| {
                words(
                    w,
                    art,
                    pen,
                    time,
                    24.0,
                    pen.theme.gold_bright,
                    Step::Caption,
                    Role::Accent,
                );
            });
        });
}

/// The level's three stars in a row, in the TV's own style (the disc's
/// art puts each in its tab's well): an earned one lit, the others dim.
fn stars(parent: &mut ChildSpawnerCommands, pen: &Pen, stars: &[bool]) {
    let theme = &pen.theme;
    parent
        .spawn(Node {
            flex_direction: FlexDirection::Row,
            column_gap: px(24),
            ..default()
        })
        .with_children(|row| {
            for &earned in stars {
                let (mark, ink) = if earned {
                    ("*", theme.gold_bright)
                } else {
                    ("-", theme.role(Role::Muted))
                };
                row.spawn(pen.inked(mark, Step::Lg, ink, bevy::text::Justify::Center));
            }
        });
}

/// One objective as `/api/_test/tv` reports it.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ObjectiveShot {
    pub tab: i32,
    /// `check`, `count` or `time`.
    pub kind: String,
    /// `open`, `newly done` or `done`.
    pub status: String,
    /// The line as drawn (a time's `@` filled).
    pub line: String,
    pub count: f32,
    pub total: f32,
}

/// The completion screen, its part of `/api/_test/tv` (openspec AU.5).
#[derive(Resource, Debug, Clone, Default, PartialEq, Serialize)]
pub struct CompletionReport {
    /// Drawn this frame: open, its root spawned (false once the continue
    /// is asked, the report kept).
    pub shown: bool,
    /// `disc` for the screen's own art, `missing: <why>` for the TV's style;
    /// empty before a screen.
    pub art: String,
    /// `read`, or `unread: <why>` (the objectives screen's scripts not
    /// linked, their collection stopped); empty before a screen.
    pub results: String,
    pub tab: usize,
    pub tabs: Vec<String>,
    pub title: Option<String>,
    pub prompt: Option<String>,
    pub level: Option<String>,
    pub objectives: Vec<ObjectiveShot>,
    pub stars: Vec<bool>,
    /// The continue taken, and the sequencing raised past the screen.
    pub continue_asked: bool,
    pub continued: bool,
    /// When it was drawn, on the frame log's clock.
    pub shown_ms: Option<f64>,
}

impl CompletionReport {
    fn of(
        open: bool,
        tab: usize,
        c: Option<&guacamole::level_end::Completion>,
        art: Option<&Result<Art, String>>,
        shown_ms: Option<f64>,
    ) -> Self {
        let Some(c) = c else {
            return Self::default();
        };
        let r = c.results.as_ref().ok();
        Self {
            shown: open && shown_ms.is_some(),
            art: match art {
                Some(Ok(_)) => "disc".into(),
                Some(Err(why)) => format!("missing: {why}"),
                None => String::new(),
            },
            results: match &c.results {
                Ok(_) => "read".into(),
                Err(why) => format!("unread: {why}"),
            },
            tab,
            tabs: r.map_or_else(Vec::new, |r| {
                r.tabs
                    .iter()
                    .map(|t| t.clone().unwrap_or_default())
                    .collect()
            }),
            title: r.and_then(|r| r.title.clone()),
            prompt: r.and_then(|r| r.prompt.clone()),
            level: r.and_then(|r| r.level.clone()),
            objectives: r.map_or_else(Vec::new, |r| {
                r.objectives
                    .iter()
                    .map(|o| ObjectiveShot {
                        tab: o.category,
                        kind: match o.kind {
                            ObjectiveKind::Check => "check",
                            ObjectiveKind::Count => "count",
                            ObjectiveKind::Time => "time",
                        }
                        .into(),
                        status: match o.status {
                            Status::Open => "open",
                            Status::NewlyDone => "newly done",
                            Status::Done => "done",
                        }
                        .into(),
                        line: line(o),
                        count: o.count,
                        total: o.total,
                    })
                    .collect()
            }),
            stars: r.map_or_else(Vec::new, |r| r.stars.clone()),
            continue_asked: c.continue_asked,
            continued: c.continued,
            shown_ms: shown_ms.filter(|_| open),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use bevy::input_focus::InputFocus;
    use bevy::time::TimeUpdateStrategy;
    use guac_ir::interface::{Glyph, Interface, Matrix, Template};
    use guac_server::config::WindowMode;
    use guacamole::level_end::Completion;
    use guacamole::scripts::Files;

    use super::*;
    use crate::cards::PortalCards;
    use crate::input::{PostedPresses, TestPress};
    use crate::test_support::tv::portal_app;

    /// A level's results as the objectives screen's scripts would leave
    /// them, made up: a story goal done this time, a time dare done before
    /// (1:15 against 4:20), a count the scripts left below 0, and the level
    /// clear star.
    fn results() -> LevelResults {
        let o = |category, kind, status, d: &str, count, total| Objective {
            category,
            kind,
            status,
            description: Some(d.into()),
            count,
            total,
        };
        LevelResults {
            title: Some("All done".into()),
            prompt: Some("Onward".into()),
            level: Some("Level".into()),
            tabs: vec![Some("One".into()), Some("Two".into()), Some("Three".into())],
            objectives: vec![
                o(
                    0,
                    ObjectiveKind::Check,
                    Status::NewlyDone,
                    "Meet him",
                    0.0,
                    0.0,
                ),
                o(1, ObjectiveKind::Time, Status::Done, "Under @", 75.0, 260.0),
                o(2, ObjectiveKind::Count, Status::Open, "Hats", -1.0, 3.0),
            ],
            stars: vec![true, false, false],
        }
    }

    /// The TV's portal app with the completion screen's plugin, a run whose
    /// sequencing reached its completion screen with [`results`], the level
    /// showing, its clock running (100 ms a frame).
    fn screen_app() -> (App, PostedPresses) {
        let mut app = portal_app(1920, 1080, WindowMode::Tv, PortalCards::default());
        let presses = PostedPresses::default();
        app.add_plugins(CompletionPlugin)
            .insert_resource(presses.clone())
            .insert_resource(TimeUpdateStrategy::ManualDuration(Duration::from_millis(
                100,
            )));
        let mut world = ScriptWorld::new(&Files::default(), &HashMap::new(), [1, 2, 3, 4, 5, 6, 7]);
        world.sequence.shows_completion = true;
        world.sequence.ending = true;
        world.sequence.completion = Some(Completion {
            results: Ok(results()),
            continue_asked: false,
            continued: false,
        });
        app.insert_resource(world);
        app.world_mut().resource_mut::<LevelPads>().show();
        (app, presses)
    }

    fn drawn(app: &mut App) -> usize {
        let mut q = app.world_mut().query::<&CompletionScreen>();
        q.iter(app.world()).count()
    }

    /// Every line of text drawn under the screen's root, in tree order.
    fn drawn_texts(app: &mut App) -> Vec<String> {
        let mut roots = app
            .world_mut()
            .query_filtered::<Entity, With<CompletionScreen>>();
        let mut stack: Vec<Entity> = roots.iter(app.world()).collect();
        let mut out = Vec::new();
        while let Some(e) = stack.pop() {
            if let Some(t) = app.world().get::<Text>(e) {
                out.push(t.0.clone());
            }
            if let Some(kids) = app.world().get::<Children>(e) {
                stack.extend(kids.iter().rev());
            }
        }
        out
    }

    fn focused(app: &App) -> Option<&'static str> {
        let e = app.world().resource::<InputFocus>().get()?;
        app.world().get::<FocusId>(e).map(|f| f.0)
    }

    fn report(app: &App) -> CompletionReport {
        app.world().resource::<CompletionReport>().clone()
    }

    /// AU.5: the screen comes up over the held level with the run's
    /// results (its title, the three tabs, each objective's line as drawn,
    /// the stars), the first tab focused behind the router; a move right
    /// focuses and shows the second; the accept button (South, the router's
    /// Activate) on it asks the continue, which closes the screen and lets
    /// the router's modal go. With no extract known, the TV's own style,
    /// named. The level's end holds the players under it and after it, as
    /// the run's sequence says (AS.125), until its level is asked.
    #[test]
    fn the_screen_shows_the_runs_results_moves_its_tabs_and_its_accept_continues() {
        let (mut app, presses) = screen_app();
        for _ in 0..3 {
            app.update();
        }
        assert!(app.world().resource::<CompletionOpen>().0);
        assert!(app.world().resource::<LevelEnding>().0, "the players held");
        assert_eq!(drawn(&mut app), 1);
        assert_eq!(focused(&app), Some(TABS[0]));
        let r = report(&app);
        assert!(r.shown);
        assert_eq!(r.results, "read");
        assert!(r.art.starts_with("missing: "), "{}", r.art);
        assert_eq!(r.title.as_deref(), Some("All done"));
        assert_eq!(r.tabs, ["One", "Two", "Three"]);
        assert_eq!(
            r.objectives
                .iter()
                .map(|o| (o.tab, o.kind.as_str(), o.status.as_str(), o.line.as_str()))
                .collect::<Vec<_>>(),
            [
                (0, "check", "newly done", "Meet him"),
                (1, "time", "done", "Under 4:20"),
                (2, "count", "open", "Hats"),
            ]
        );
        assert_eq!(r.stars, [true, false, false]);
        // What's drawn: the title with the focused tab's name, the focused
        // tab's objectives alone, each with its tally.
        let texts = drawn_texts(&mut app);
        for want in ["All done", "One", "Meet him"] {
            assert!(texts.iter().any(|t| t == want), "{want} drawn: {texts:?}");
        }
        assert!(
            !texts.iter().any(|t| t == "Under 4:20" || t == "Hats"),
            "{texts:?}"
        );
        presses.push(TestPress::Right);
        for _ in 0..3 {
            app.update();
        }
        assert_eq!(report(&app).tab, 1);
        assert_eq!(focused(&app), Some(TABS[1]));
        assert_eq!(drawn(&mut app), 1, "redrawn, not doubled");
        let texts = drawn_texts(&mut app);
        for want in ["Under 4:20", "1:15"] {
            assert!(texts.iter().any(|t| t == want), "{want} drawn: {texts:?}");
        }
        assert!(!texts.iter().any(|t| t == "Meet him"), "{texts:?}");
        // The portal holds the level (its alert): the screen steps aside,
        // and a South under the alert continues nothing; let go, it's back
        // on the tab it was on.
        app.world_mut().resource_mut::<LevelPads>().hold(true);
        for _ in 0..3 {
            app.update();
        }
        assert!(!app.world().resource::<CompletionOpen>().0, "aside");
        assert_eq!(drawn(&mut app), 0);
        presses.push(TestPress::South);
        for _ in 0..3 {
            app.update();
        }
        let world = app.world().resource::<ScriptWorld>();
        assert!(!world.sequence.completion.as_ref().unwrap().continue_asked);
        app.world_mut().resource_mut::<LevelPads>().hold(false);
        for _ in 0..3 {
            app.update();
        }
        assert!(app.world().resource::<CompletionOpen>().0, "back");
        assert_eq!((report(&app).tab, drawn(&mut app)), (1, 1));
        assert_eq!(focused(&app), Some(TABS[1]));
        presses.push(TestPress::South);
        for _ in 0..3 {
            app.update();
        }
        let world = app.world().resource::<ScriptWorld>();
        assert!(world.sequence.completion.as_ref().unwrap().continue_asked);
        assert!(!app.world().resource::<CompletionOpen>().0);
        assert_eq!(drawn(&mut app), 0);
        let r = report(&app);
        assert!(r.continue_asked && !r.shown);
        assert!(
            app.world().resource::<LevelEnding>().0,
            "held past the screen"
        );
        app.world_mut()
            .resource_mut::<ScriptWorld>()
            .sequence
            .ending = false;
        app.update();
        assert!(
            !app.world().resource::<LevelEnding>().0,
            "let go at the ask"
        );
    }

    /// The texts under every entity `of` matches, in tree order.
    fn texts_under<F: bevy::ecs::query::QueryFilter>(app: &mut App) -> Vec<String> {
        let mut roots = app.world_mut().query_filtered::<Entity, F>();
        let mut stack: Vec<Entity> = roots.iter(app.world()).collect();
        let mut out = Vec::new();
        while let Some(e) = stack.pop() {
            if let Some(t) = app.world().get::<Text>(e) {
                out.push(t.0.clone());
            }
            if let Some(kids) = app.world().get::<Children>(e) {
                stack.extend(kids.iter());
            }
        }
        out
    }

    /// openspec AS.585: `UI - Objectives Menu - Item Focus` writes the
    /// focused tab's name into the screen's title after its own, in the
    /// file's yellow; no title stands under a column. A move to the next
    /// tab moves its name into the title.
    #[test]
    fn the_focused_tabs_name_is_in_the_title_and_none_under_the_columns() {
        let (mut app, presses) = screen_app();
        for _ in 0..3 {
            app.update();
        }
        let mut title = texts_under::<With<ScreenTitle>>(&mut app);
        title.sort();
        assert_eq!(title, ["All done", "One"]);
        let named = texts_under::<With<TabTitle>>(&mut app);
        assert_eq!(named, ["One"]);
        let mut q = app.world_mut().query_filtered::<&TextColor, With<Text>>();
        assert!(
            q.iter(app.world()).any(|c| c.0 == TAB_TITLE_COLOUR),
            "in its own colour"
        );
        assert!(
            texts_under::<With<FocusId>>(&mut app).is_empty(),
            "no title under a column"
        );
        let all = drawn_texts(&mut app);
        assert!(!all.iter().any(|t| t == "Two" || t == "Three"), "{all:?}");
        presses.push(TestPress::Right);
        for _ in 0..3 {
            app.update();
        }
        assert_eq!(texts_under::<With<TabTitle>>(&mut app), ["Two"]);
        assert!(texts_under::<With<FocusId>>(&mut app).is_empty());
    }

    /// The screen's lines: a time's `@` filled with its target, its tally
    /// the time taken; a count's tally N/M, one the scripts left below 0
    /// reading 0; a check's none.
    #[test]
    fn a_times_at_is_its_target_and_a_count_below_0_reads_0() {
        let r = results();
        assert_eq!(line(&r.objectives[1]), "Under 4:20");
        assert_eq!(tally(&r.objectives[1]).as_deref(), Some("1:15"));
        assert_eq!(tally(&r.objectives[2]).as_deref(), Some("0/3"));
        assert_eq!(tally(&r.objectives[0]), None);
        assert_eq!(clock(3601.0), "60:01");
    }

    /// openspec AU.29's review: the font's bodies are the disc's 0x80 grey,
    /// and a glyph's tint draws them in the colour asked for, its alpha kept.
    #[test]
    fn a_glyphs_body_draws_in_the_colour_asked_for() {
        let grey = Color::srgb_u8(0x80, 0x80, 0x80).to_linear();
        assert!((grey.red - GLYPH_BODY).abs() < 1e-3);
        for c in [
            Color::WHITE,
            Color::srgb(0.03, 0.05, 0.16),
            Color::srgba(1.0, 0.8, 0.2, 0.7),
        ] {
            let (want, tint) = (c.to_linear(), glyph_tint(c).to_linear());
            assert!((tint.red * GLYPH_BODY - want.red).abs() < 1e-5, "{c:?}");
            assert!((tint.green * GLYPH_BODY - want.green).abs() < 1e-5);
            assert!((tint.blue * GLYPH_BODY - want.blue).abs() < 1e-5);
            assert_eq!(tint.alpha, want.alpha);
        }
    }

    fn template(name: &str, tex: &str) -> Template {
        Template {
            name: name.into(),
            object: 1,
            properties: 0,
            flags: 0,
            position: [0, 0],
            z_depth: 0.0,
            priority: 0.0,
            width: 25.0,
            height: 30.0,
            scale: [100.0; 3],
            color: [0x80, 0x80, 0x80, 0xFF],
            replace_color: [0x80, 0x80, 0x80, 0],
            num_value: 0,
            depth: 100.0,
            init_matrix: Matrix::default(),
            matrix: Matrix::default(),
            content: Content::Image(guac_ir::interface::Art {
                source: None,
                texture: Some(tex.into()),
                size: Some([4, 4]),
                stream_archive_hash: 0,
                desired: [4, 4],
            }),
            string: None,
        }
    }

    fn record(archive: &str, templates: Vec<Template>, fonts: Vec<Font>) -> Interface {
        Interface {
            identity: crate::test_support::turntable::identity(),
            archive: archive.into(),
            templates,
            fonts,
            strings: Vec::new(),
            refused: Vec::new(),
        }
    }

    /// AU.5's art: the screen's twelve pieces read off its interface record
    /// at the sizes their templates give and `global.bld`'s `contemp` font
    /// with its atlas, each texture off the extract; a record missing a
    /// piece, or the font's record missing, is named.
    #[test]
    fn the_art_is_the_screens_pieces_and_globals_bitmap_font() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path();
        std::fs::create_dir_all(dir.join("textures")).unwrap();
        std::fs::create_dir_all(dir.join("interface")).unwrap();
        let tex = "textures/piece.ktx2";
        std::fs::write(dir.join(tex), crate::test_support::turntable::ktx2()).unwrap();
        let font = Font {
            name: Some("hdd:/tfb/content/fonts/contemp".into()),
            source: None,
            height: 82.0,
            base_line: 64.0,
            char_padding: 0.0,
            kern_amount: 2.0,
            color: [1.0; 4],
            atlas: tex.into(),
            atlas_size: [4, 4],
            glyphs: vec![Glyph {
                char: u16::from(b'A'),
                step: 40.0,
                offset: [0.0, 0.0],
                size: [40.0, 82.0],
                uv_min: [0.0, 0.0],
                uv_max: [1.0, 1.0],
                kern: [0; 4],
            }],
        };
        let write = |archive: &str, r: &Interface| {
            let rel = guac_extract::interface::path(IrFormat::Ron, archive);
            std::fs::write(dir.join(rel), guac_ir::to_ron(r).unwrap()).unwrap();
        };
        let all: Vec<Template> = ART.iter().map(|n| template(n, tex)).collect();
        write(SCREEN, &record(SCREEN, all.clone(), Vec::new()));
        let mut images = Assets::<Image>::default();
        assert_eq!(
            load_art(dir, IrFormat::Ron, &mut images, CompressedImageFormats::BC).err(),
            Some(format!(
                "interface/global.v{}.ron: No such file or directory (os error 2)",
                guac_ir::interface::RECORD
            )),
            "the font's record named"
        );
        write(FONT_ARCHIVE, &record(FONT_ARCHIVE, Vec::new(), vec![font]));
        let art = load_art(dir, IrFormat::Ron, &mut images, CompressedImageFormats::BC).unwrap();
        assert_eq!(art.pieces.len(), ART.len());
        assert_eq!(art.pieces[STAR].size, Vec2::new(25.0, 30.0));
        assert!(art.font.name.as_deref().unwrap().ends_with("contemp"));
        write(SCREEN, &record(SCREEN, all[1..].to_vec(), Vec::new()));
        assert_eq!(
            load_art(dir, IrFormat::Ron, &mut images, CompressedImageFormats::BC).err(),
            Some(format!("{SCREEN} holds no {TAB_FOCUS}"))
        );
    }

    /// AS.446: the screen runs through its tabs by itself, as its scripts
    /// do: each tab's rows 0.15 s apart and then its one-second delay
    /// (100 ms a frame here: the first tab's one row 1.15 s, the second's
    /// the same), on to the last and no further; a left or right ends it
    /// where the player put it.
    #[test]
    fn the_tabs_come_up_in_turn_until_the_last_or_a_press() {
        let (mut app, _) = screen_app();
        let frames_to = |app: &mut App, want: usize| {
            (0..40).find(|_| {
                app.update();
                report(app).tab == want
            })
        };
        let first = frames_to(&mut app, 1).expect("the second tab by itself");
        assert!((10..=14).contains(&first), "{first} frames");
        let second = frames_to(&mut app, 2).expect("the third tab by itself");
        assert!((10..=14).contains(&second), "{second} frames");
        for _ in 0..30 {
            app.update();
        }
        assert_eq!(report(&app).tab, 2, "the last stays");
        assert_eq!(app.world().resource::<TabCycle>().0, None);

        let (mut app, presses) = screen_app();
        for _ in 0..3 {
            app.update();
        }
        presses.push(TestPress::Right);
        for _ in 0..40 {
            app.update();
        }
        assert_eq!(report(&app).tab, 1, "the press ends the run through them");
        assert_eq!(app.world().resource::<TabCycle>().0, None);
    }

    /// AS.446: the panel's pieces where the screen's scripts put them:
    /// each column's star over its column (102 apart against the columns'
    /// 103, within 4 units of each middle), the rows inside the panel from
    /// (20, 95) 32 apart; and on the TV, every image of the screen's the
    /// right way up, the first row's checkmark on its line and its earned
    /// star in its well 121 over the panel's middle.
    #[test]
    fn the_layouts_columns_are_where_the_screens_scripts_put_them() {
        let (end, tile, column) = (25.0, TILE_WIDTH, 103.0);
        let panel = Vec2::new(2.0 * (end + tile) + 3.0 * column, 327.0);
        assert_eq!(panel.x, 509.0);
        for i in 0..3 {
            let middle = end + tile + column * (i as f32 + 0.5);
            let star = Places::star(i, panel);
            assert!(
                (star.x - middle).abs() <= 4.0,
                "tab {i}: {star} against {middle}"
            );
            assert_eq!(star.y, 327.0 / 2.0 - 121.0);
        }
        assert_eq!(Places::row(0), Vec2::new(20.0, 95.0));
        assert_eq!(Places::row(6), Vec2::new(20.0, 95.0 + 6.0 * 32.0));
        assert!(Places::row(6).y + 16.0 < panel.y, "seven rows inside");
        assert!(Places::ENTRY.x + Places::COUNT_END < panel.x - end);

        let (mut app, _) = screen_app();
        let sizes = [
            (TAB_FOCUS, (103.0, 327.0)),
            (TAB_OFF, (103.0, 327.0)),
            (TAB_LEFT, (25.0, 327.0)),
            (TAB_RIGHT, (25.0, 327.0)),
            (TAB_TILE, (1.0, 327.0)),
            (RECESS_LEFT, (16.0, 30.0)),
            (RECESS_RIGHT, (16.0, 30.0)),
            (RECESS_TILE, (1.0, 30.0)),
            (RECESS_TIME, (32.0, 32.0)),
            (STAR, (128.0, 128.0)),
            (CHECK_EMPTY, (25.0, 25.0)),
            (CHECK_DONE, (32.0, 32.0)),
        ];
        let mut pieces = HashMap::new();
        {
            let mut images = app.world_mut().resource_mut::<Assets<Image>>();
            for (name, (w, h)) in sizes {
                pieces.insert(
                    name,
                    Piece {
                        image: images.add(Image::default()),
                        size: Vec2::new(w, h),
                    },
                );
            }
        }
        let ours: Vec<Handle<Image>> = pieces.values().map(|p| p.image.clone()).collect();
        let star = pieces[STAR].image.clone();
        let check = pieces[CHECK_DONE].image.clone();
        let recess = pieces[RECESS_TILE].image.clone();
        app.insert_resource(Loaded(Some(Ok(Art::for_tests(pieces)))));
        for _ in 0..3 {
            app.update();
        }
        let mut q = app.world_mut().query::<(&Node, &ImageNode)>();
        let drawn: Vec<(Node, ImageNode)> = q
            .iter(app.world())
            .map(|(n, i)| (n.clone(), i.clone()))
            .collect();
        let art: Vec<&(Node, ImageNode)> = drawn
            .iter()
            .filter(|(_, i)| ours.contains(&i.image))
            .collect();
        assert!(art.len() >= 7, "{} pieces", art.len());
        assert!(art.iter().all(|(_, i)| i.flip_y), "every piece upright");
        let (n, _) = art.iter().find(|(_, i)| i.image == star).expect("the star");
        let side = 128.0 * Places::STAR_SCALE;
        let at = Places::star(0, panel);
        assert_eq!(n.left, px((at.x - side / 2.0) * UNIT));
        assert_eq!(n.top, px((at.y - side / 2.0) * UNIT));
        assert!(
            art.iter().any(|(_, i)| i.image == check),
            "the first row's check"
        );
        assert!(
            !art.iter().any(|(_, i)| i.image == recess),
            "no recess behind a check row"
        );
        // The dares: the time's box, its recess and its time; the
        // collections: a count's N/M with no box.
        let pieces_now = |app: &mut App| -> Vec<Handle<Image>> {
            let mut q = app.world_mut().query::<&ImageNode>();
            q.iter(app.world()).map(|i| i.image.clone()).collect()
        };
        app.world_mut().resource_mut::<CompletionTab>().0 = 1;
        app.world_mut().resource_mut::<TabCycle>().0 = None;
        for _ in 0..3 {
            app.update();
        }
        assert!(pieces_now(&mut app).contains(&recess), "the time's box");
        app.world_mut().resource_mut::<CompletionTab>().0 = 2;
        for _ in 0..3 {
            app.update();
        }
        assert!(
            !pieces_now(&mut app).contains(&recess),
            "no box for a count"
        );
        assert_eq!(report(&app).tab, 2);
        // The count's place: right-justified, its end at 455.
        let counts = {
            let mut q = app.world_mut().query::<&Node>();
            q.iter(app.world())
                .filter(|n| {
                    n.justify_content == JustifyContent::FlexEnd
                        && n.width == px(Places::COUNT_END * UNIT)
                })
                .count()
        };
        assert_eq!(counts, 1, "the count drawn at its place");
    }
}
