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
//! The layout rests on the screen's file and our choice: each piece is one
//! of its templates at the size the file gives it (the panel of three tab
//! columns its scripts build, 509 by 327 of the original's 640 by 480 units;
//! a recess under each objective; the stars; the checkmarks), placed where
//! we put them, there being no recording of a completion screen; the
//! scripts' own animation (the tabs shown in turn, the checkmarks slammed
//! in, the fireworks for three stars) isn't drawn.
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
const STAR: &str = "UI_Star";
const CHECK_EMPTY: &str = "UI_Checkmark_(00)_empty";
const CHECK_DONE: &str = "UI_Checkmark_(01)";
const ART: [&str; 12] = [
    TAB_FOCUS,
    TAB_OFF,
    TAB_LEFT,
    TAB_RIGHT,
    TAB_TILE,
    RECESS_LEFT,
    RECESS_RIGHT,
    RECESS_TILE,
    STAR,
    CHECK_EMPTY,
    CHECK_DONE,
    "UI_Checkmark_(02)_off",
];

/// The tiles beside the tab heads, as the screen's scripts size them.
const TILE_WIDTH: f32 = 75.0;

/// Our layout's, in the original's units: the objectives' list inside the
/// panel, under the tab heads and over the columns' star wells, seven rows
/// at most (chapter 1's collections), and each earned star in its well.
const LIST_SIDE: f32 = 40.0;
const LIST_TOP: f32 = 50.0;
const ROW_HEIGHT: f32 = 24.0;
const ROW_GAP: f32 = 3.0;
const WELL_CENTRE: f32 = 283.0;
const STAR_SIZE: f32 = 52.0;

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
    }
    *was_open = sequenced;
    let now = sequenced && !pads.is_some_and(|p| p.held());
    if open.0 != now {
        info!(open = now, "the completion screen");
        open.0 = now;
    }
}

/// Left and right move between the tabs.
fn tab_moves(
    mut inputs: MessageReader<TvInput>,
    open: Res<CompletionOpen>,
    mut tab: ResMut<CompletionTab>,
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
        if next != tab.0 {
            tab.0 = next;
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
            words(root, art, pen, &title, 52.0, gold, Step::Lg, Role::Accent);
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

/// `text` in the bitmap font, one image node a glyph cut from its atlas,
/// laid out by the interface's measure (`guac_interface::text::glyphs`),
/// `size` design pixels for the font's own height.
pub(crate) fn bitmap_text(
    parent: &mut ChildSpawnerCommands,
    art: &Art,
    text: &str,
    size: f32,
    color: Color,
) {
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
                        color,
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

/// One of the screen's pieces at its template's size times `scale` units.
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
        ImageNode {
            color: Color::WHITE.with_alpha(alpha),
            ..ImageNode::new(p.image.clone())
        },
    ));
}

/// The panel: the three tab columns side by side (the focused one's art
/// lit), each a button behind the router with its title on its head, and
/// the focused tab's objectives over them.
fn panel(
    parent: &mut ChildSpawnerCommands,
    art: Option<&Art>,
    pen: &Pen,
    r: &LevelResults,
    tab: usize,
) {
    let theme = &pen.theme;
    let height = art
        .and_then(|a| a.pieces.get(TAB_FOCUS))
        .map_or(327.0, |p| p.size.y)
        * UNIT;
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
                let name = r
                    .tabs
                    .get(i)
                    .cloned()
                    .flatten()
                    .unwrap_or_else(|| ["Story", "Challenges", "Finds"][i].to_owned());
                let mut button = row.spawn((
                    Button,
                    FocusId(id),
                    Node {
                        width: px(art
                            .and_then(|a| a.pieces.get(TAB_FOCUS))
                            .map_or(160.0, |p| p.size.x * UNIT)),
                        height: px(height),
                        flex_direction: FlexDirection::Column,
                        align_items: AlignItems::Center,
                        padding: UiRect::top(px(14.0 * UNIT)),
                        flex_shrink: 0.0,
                        ..default()
                    },
                ));
                if focused {
                    button.insert(AutoFocus);
                }
                match art.and_then(|a| a.pieces.get(if focused { TAB_FOCUS } else { TAB_OFF })) {
                    Some(p) => {
                        button.insert(ImageNode::new(p.image.clone()));
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
                let color = if focused {
                    theme.gold_bright
                } else {
                    Color::WHITE.with_alpha(0.7)
                };
                button.with_children(|head| {
                    words(
                        head,
                        art,
                        pen,
                        &name,
                        26.0,
                        color,
                        Step::Body,
                        if focused { Role::Accent } else { Role::Muted },
                    );
                });
            }
            if let Some(a) = art {
                piece(row, a, TAB_TILE, Some(TILE_WIDTH), 1.0);
                piece(row, a, TAB_RIGHT, None, 1.0);
            }
        })
        .with_children(|row| {
            // The focused tab's objectives over the columns, under their heads.
            row.spawn(Node {
                position_type: PositionType::Absolute,
                left: px(LIST_SIDE * UNIT),
                right: px(LIST_SIDE * UNIT),
                top: px(LIST_TOP * UNIT),
                flex_direction: FlexDirection::Column,
                row_gap: px(ROW_GAP * UNIT),
                ..default()
            })
            .with_children(|list| {
                for o in r.objectives.iter().filter(|o| o.category as usize == tab) {
                    objective(list, art, pen, o);
                }
            });
            // Each tab's star in the well at the foot of its column, an
            // earned one alone (the well's own art shows an empty one).
            if let Some(a) = art
                && let (Some(star), Some(head)) = (a.pieces.get(STAR), a.pieces.get(TAB_FOCUS))
            {
                let first = a.pieces.get(TAB_LEFT).map_or(0.0, |p| p.size.x) + TILE_WIDTH;
                for (i, earned) in r.stars.iter().enumerate().take(TABS.len()) {
                    if !earned {
                        continue;
                    }
                    let centre = first + head.size.x * (i as f32 + 0.5);
                    row.spawn((
                        Node {
                            position_type: PositionType::Absolute,
                            left: px((centre - STAR_SIZE / 2.0) * UNIT),
                            top: px((WELL_CENTRE - STAR_SIZE / 2.0) * UNIT),
                            width: px(STAR_SIZE * UNIT),
                            height: px(STAR_SIZE * UNIT),
                            ..default()
                        },
                        ImageNode::new(star.image.clone()),
                    ));
                }
            }
        });
}

/// One objective's row: its recess, its checkmark, its line and its count
/// or time.
fn objective(parent: &mut ChildSpawnerCommands, art: Option<&Art>, pen: &Pen, o: &Objective) {
    let theme = &pen.theme;
    let h = ROW_HEIGHT * UNIT;
    let mut row = parent.spawn(Node {
        height: px(h),
        flex_direction: FlexDirection::Row,
        align_items: AlignItems::Center,
        column_gap: px(8.0 * UNIT),
        padding: UiRect::horizontal(px(6.0 * UNIT)),
        ..default()
    });
    if art.is_none() {
        row.insert(BackgroundColor(theme.panel[0].with_alpha(0.8)));
    }
    row.with_children(|line| {
        if let Some(a) = art {
            // The recess behind the row: its two ends and its tile between.
            line.spawn(Node {
                position_type: PositionType::Absolute,
                left: px(0),
                right: px(0),
                top: px(0),
                bottom: px(0),
                flex_direction: FlexDirection::Row,
                ..default()
            })
            .with_children(|recess| {
                for (name, grow) in [
                    (RECESS_LEFT, false),
                    (RECESS_TILE, true),
                    (RECESS_RIGHT, false),
                ] {
                    if let Some(p) = a.pieces.get(name) {
                        let mut node = Node {
                            height: px(h),
                            flex_shrink: 0.0,
                            ..default()
                        };
                        if grow {
                            node.flex_grow = 1.0;
                        } else {
                            node.width = px(p.size.x * h / p.size.y.max(1.0));
                        }
                        recess.spawn((node, ImageNode::new(p.image.clone())));
                    }
                }
            });
            let check = if o.status.done() {
                CHECK_DONE
            } else {
                CHECK_EMPTY
            };
            if let Some(p) = a.pieces.get(check) {
                line.spawn((
                    Node {
                        width: px(20.0 * UNIT),
                        height: px(20.0 * UNIT),
                        flex_shrink: 0.0,
                        ..default()
                    },
                    ImageNode::new(p.image.clone()),
                ));
            }
        } else {
            let mark = if o.status.done() { "[x]" } else { "[ ]" };
            line.spawn(pen.text(mark, Step::Caption, Role::Accent));
        }
        let color = if o.status == Status::NewlyDone {
            theme.gold_bright
        } else {
            Color::WHITE
        };
        words(
            line,
            art,
            pen,
            &self::line(o),
            24.0,
            color,
            Step::Caption,
            Role::Primary,
        );
        line.spawn(Node {
            flex_grow: 1.0,
            ..default()
        });
        if let Some(t) = tally(o) {
            words(
                line,
                art,
                pen,
                &t,
                24.0,
                theme.gold_bright,
                Step::Caption,
                Role::Accent,
            );
        }
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
        // What's drawn: the title, the tabs' names, the focused tab's
        // objectives alone, each with its tally.
        let texts = drawn_texts(&mut app);
        for want in ["All done", "One", "Two", "Three", "Meet him"] {
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
}
