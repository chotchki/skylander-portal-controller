//! A dialogue's yes-or-no question on the TV (openspec AU.6, AU design
//! decision 2): Cali's challenge question and Flynn's ready question on the
//! hub, asked on a modal over the level, the answer run back through the
//! original's own buttons ([`guacamole::prompt`]).
//!
//! - **Each step**, after the scripts' step, the question the run waits on
//!   is read ([`guacamole::prompt::up`]) and an answer chosen since the last
//!   step is run on the question it was chosen for, then read again.
//! - **The modal** shows while a question waits: its English text (the
//!   question's string off the interface's model; the TV's own words where
//!   it doesn't read), then Yes and No, the focus on Yes when the original's
//!   prompt starts there. It's a modal as Restart chapter's is: the router
//!   hands it the pads, a move left or right goes between the two, South
//!   presses the focused one. In the disc's art where the extract's
//!   interface part holds it, as AU.5's screen takes the objectives screen's
//!   (openspec AU.29, AU design decision 2): `global.bld`'s yes and no
//!   backdrops, desaturated off the focus, and its bitmap font on a box of
//!   the clean text box's colour; the TV's own look without it
//!   (stand-in ledger: `yes-no`).

use bevy::image::CompressedImageFormats;
use bevy::input_focus::tab_navigation::TabIndex;
use bevy::input_focus::{AutoFocus, FocusCause, InputFocus};
use bevy::math::CompassOctant;
use bevy::prelude::*;
use bevy::render::renderer::RenderDevice;
use bevy::ui_widgets::{Activate, Button};
use guac_script::Value;
use guacamole::prompt::{self, Prompt};
use guacamole::scripts::{ScriptClone, ScriptRuntime, ScriptWorld, WorldHost};
use serde::Serialize;
use tracing::{info, warn};

use crate::completion::{Art, FONT_ARCHIVE, bitmap_text, load_art_of};
use crate::hero_scripts::ScriptSource;
use crate::input::{FocusId, TvInput};
use crate::pause::ShownAt;
use crate::theme::{Role, Step, TvTheme};
use crate::tv::{choice_bundle, other_choice_bundle};
use crate::tv_state::{Pen, Redrawn, TvAssets, UiTree};

/// Over the portal's alert (940) and Restart chapter (950), under the
/// corner join code (960).
pub(crate) const PROMPT_Z: i32 = 955;

/// The choices' names, for the router's focus and the mirror.
pub(crate) const PROMPT_YES: &str = "prompt-yes";
pub(crate) const PROMPT_NO: &str = "prompt-no";

/// The question's line where its string doesn't read.
pub(crate) const UNREAD_LINE: &str = "Yes or no?";

/// `global.bld`'s templates the buttons draw, by name: each focused, and
/// off the focus.
const YES_ON: &str = "UI_ButtonBackdrop_Yes";
const NO_ON: &str = "UI_ButtonBackdrop_No";
const YES_OFF: &str = "UI_ButtonBackdrop_Yes_Desat";
const NO_OFF: &str = "UI_ButtonBackdrop_No_Desat";
const PROMPT_ART: [&str; 4] = [YES_ON, NO_ON, YES_OFF, NO_OFF];

/// The original's screen units to the TV's design pixels, as the completion
/// screen's; the question's and a button's line heights in them; and the
/// box's fill, the clean text box's blur backdrop, its 16-unit corners.
const UNIT: f32 = 2.0;
const QUESTION_SIZE: f32 = 56.0;
const BUTTON_SIZE: f32 = 40.0;
const BOX_FILL: Color = Color::srgba(0.03, 0.05, 0.16, 0.86);
const BOX_CORNER: f32 = 16.0 * UNIT;
const QUESTION_COLOR: Color = Color::WHITE;
const BUTTON_COLOR: Color = Color::srgb(0.03, 0.05, 0.16);

/// A button on the disc's art: its backdrop focused, and off the focus.
#[derive(Component, Debug, Clone, PartialEq)]
pub(crate) struct ArtChoice {
    on: Handle<Image>,
    off: Handle<Image>,
}

/// The question the run waits on, as its last step left it.
#[derive(Resource, Debug, Default, Clone, PartialEq)]
pub(crate) struct Asked(pub(crate) Option<Prompt>);

/// The answer chosen on the modal, for the next step to run: the question
/// it answers, and yes or no.
#[derive(Resource, Debug, Default, Clone, PartialEq)]
pub(crate) struct Answer(pub(crate) Option<(Prompt, bool)>);

/// The modal is up: the router hands it the pads (as [`crate::input::ModalOpen`]
/// does Restart chapter).
#[derive(Resource, Debug, Default, Clone, Copy, PartialEq, Eq)]
pub(crate) struct PromptOpen(pub(crate) bool);

/// The modal's root, for the dialogue UI whose question it asks.
#[derive(Component, Debug, Clone, Copy, PartialEq)]
pub(crate) struct PromptScreen(Value, bool);

/// The answers run, for `/api/_test/tv`: each question's text and the answer.
#[derive(Resource, Debug, Default, Clone, PartialEq)]
pub(crate) struct Answered(pub(crate) Vec<(Option<String>, bool)>);

pub(crate) struct YesNoPlugin;

impl Plugin for YesNoPlugin {
    fn build(&self, app: &mut App) {
        guacamole::frame::ensure(app);
        app.init_resource::<Asked>()
            .init_resource::<Answer>()
            .init_resource::<PromptOpen>()
            .init_resource::<Answered>()
            .add_message::<TvInput>()
            .add_observer(on_choice)
            .add_systems(
                FixedUpdate,
                ask_and_answer
                    .in_set(guacamole::frame::Frame::Scripts)
                    .after(crate::hero_scripts::step_scripts)
                    .run_if(resource_exists::<ScriptRuntime>)
                    .run_if(resource_exists::<ScriptWorld>),
            )
            .add_systems(Update, (open, moves, show, art_focus).chain());
    }
}

/// After the scripts' step: an answer chosen for the question still up run
/// on it ([`prompt::answer`], which checks the run still asks that very
/// question), then the question the run waits on read.
fn ask_and_answer(
    mut commands: Commands,
    mut runtime: ResMut<ScriptRuntime>,
    mut world: ResMut<ScriptWorld>,
    mut asked: ResMut<Asked>,
    mut answer: ResMut<Answer>,
    mut answered: ResMut<Answered>,
) {
    if let Some((chosen, yes)) = answer.0.take()
        && let Some(p) = asked.0.clone().filter(|p| prompt::same(p, &chosen))
    {
        let ScriptRuntime { vm, generator, .. } = &mut *runtime;
        let mut spawn = |template: guac_script::Value| -> guac_script::Handle {
            commands.spawn(ScriptClone(template)).id().to_bits()
        };
        let mut engine = guacamole::script_engine::NoEngine;
        let mut host = WorldHost {
            world: &mut world,
            engine: &mut engine,
            generator,
            spawn: &mut spawn,
        };
        match prompt::answer(vm, &mut host, &p, yes) {
            Ok(true) => {
                info!(question = ?p.text, yes, "the dialogue's question answered");
                answered.0.push((p.text.clone(), yes));
            }
            Ok(false) => warn!(question = ?p.text, "the question was gone before its answer"),
            Err(h) => warn!(%h, "the question's answer stopped"),
        }
    }
    let now = prompt::up(&runtime.vm, &world);
    if asked.0 != now {
        if let Some(p) = &now {
            info!(question = ?p.text, yes_first = p.yes_first, "a dialogue asks yes or no");
        }
        asked.0 = now;
    }
}

/// The modal is up while a question waits, no answer is on its way and the
/// level shows, unheld: while the portal holds the level (its alert, an
/// arrival card) the question waits under it, unanswerable, so the alert
/// reads whole and nothing is answered blind (found at AU.6's review: the
/// modal over "too many Skylanders" hid it, and its Yes waited out the hold
/// unseen). With the level gone from the TV (the portal view in its place)
/// or its run taken down (a level change, Restart chapter) the question and
/// any answer are forgotten: a run still asking is read again at its next
/// step.
fn open(
    mut asked: ResMut<Asked>,
    mut answer: ResMut<Answer>,
    pads: Option<Res<crate::pad::LevelPads>>,
    runtime: Option<Res<ScriptRuntime>>,
    mut ran: Local<bool>,
    mut open: ResMut<PromptOpen>,
) {
    let run_gone = std::mem::replace(&mut *ran, runtime.is_some()) && runtime.is_none();
    let shows = pads.as_ref().is_some_and(|p| p.shows());
    if (!shows || run_gone) && (asked.0.is_some() || answer.0.is_some()) {
        asked.0 = None;
        answer.0 = None;
    }
    let held = pads.is_some_and(|p| p.held());
    let up = shows && !held && asked.0.is_some() && answer.0.is_none();
    if open.0 != up {
        open.0 = up;
    }
}

/// Left or right goes between Yes and No.
fn moves(
    mut inputs: MessageReader<TvInput>,
    open: Res<PromptOpen>,
    focus: Option<ResMut<InputFocus>>,
    choices: Query<(Entity, &FocusId)>,
) {
    let read: Vec<TvInput> = inputs.read().copied().collect();
    if !open.0 {
        return;
    }
    let Some(mut focus) = focus else {
        return;
    };
    let find = |id: &str| choices.iter().find(|(_, f)| f.0 == id).map(|(e, _)| e);
    for input in read {
        if let TvInput::Move(CompassOctant::West | CompassOctant::East) = input {
            let (yes, no) = (find(PROMPT_YES), find(PROMPT_NO));
            let other = if focus.get() == yes { no } else { yes };
            if let Some(e) = other {
                focus.set(e, FocusCause::Navigated);
            }
        }
    }
}

/// Yes or No pressed: the answer for the next step to run.
fn on_choice(
    activate: On<Activate>,
    ids: Query<&FocusId>,
    open: Res<PromptOpen>,
    asked: Res<Asked>,
    mut answer: ResMut<Answer>,
) {
    let (Ok(id), true) = (ids.get(activate.entity), open.0) else {
        return;
    };
    let yes = match id.0 {
        PROMPT_YES => true,
        PROMPT_NO => false,
        _ => return,
    };
    if let Some(p) = &asked.0 {
        info!(question = ?p.text, yes, "the question's answer chosen");
        answer.0 = Some((p.clone(), yes));
    }
}

/// Each button on the disc's art shows its backdrop for the focus.
fn art_focus(
    focus: Option<Res<InputFocus>>,
    mut choices: Query<(Entity, &ArtChoice, &mut ImageNode)>,
) {
    let focused = focus.and_then(|f| f.get());
    for (e, art, mut node) in &mut choices {
        let want = if focused == Some(e) {
            &art.on
        } else {
            &art.off
        };
        if node.image != *want {
            node.image = want.clone();
        }
    }
}

/// The modal's art off the run's extract, once a session.
fn prompt_art(
    source: Option<&ScriptSource>,
    images: &mut Assets<Image>,
    device: Option<&RenderDevice>,
) -> Result<Art, String> {
    let formats = device.map_or(CompressedImageFormats::BC, |d| {
        CompressedImageFormats::from_features(d.features())
    });
    match source.and_then(ScriptSource::extract) {
        Some((dir, ir)) => load_art_of(dir, *ir, FONT_ARCHIVE, &PROMPT_ART, images, formats),
        None => Err("the run's extract isn't known".into()),
    }
}

/// Spawns the modal for the question up and despawns one no longer up,
/// telling the shots. Needs the TV's font and theme; headless tests without
/// them draw nothing, the question answered all the same.
#[allow(clippy::too_many_arguments)]
fn show(
    mut commands: Commands,
    open: Res<PromptOpen>,
    asked: Res<Asked>,
    (assets, theme, time): (Option<Res<TvAssets>>, Option<Res<TvTheme>>, Res<Time<Real>>),
    (source, mut images, device): (
        Option<Res<ScriptSource>>,
        ResMut<Assets<Image>>,
        Option<Res<RenderDevice>>,
    ),
    mut redrawn: MessageWriter<Redrawn>,
    mut shown: Local<Option<(Value, Entity)>>,
    mut art: Local<Option<Result<Art, String>>>,
) {
    let want = asked.0.as_ref().filter(|_| open.0);
    if shown.map(|(ui, _)| ui) == want.map(|p| p.ui) {
        return;
    }
    if let Some((_, entity)) = shown.take()
        && let Ok(mut root) = commands.get_entity(entity)
    {
        root.despawn();
        redrawn.write(Redrawn::Alert);
    }
    let (Some(want), Some(assets), Some(theme)) = (want, assets, theme) else {
        return;
    };
    let pen = Pen::new(&assets, &theme);
    let loaded = art.get_or_insert_with(|| {
        let a = prompt_art(source.as_deref(), &mut images, device.as_deref());
        if let Err(why) = &a {
            warn!(reason = %why, "the question's art didn't load: the TV's own style");
        }
        a
    });
    let entity = match loaded.as_ref().ok() {
        Some(a) => spawn_art(&mut commands, a, want),
        None => spawn(&mut commands, &pen, want),
    };
    commands.entity(entity).insert(ShownAt(time.elapsed()));
    *shown = Some((want.ui, entity));
    redrawn.write(Redrawn::Alert);
}

/// A button on the disc's art: its backdrop at its template's size with
/// `text` centred on it in the bitmap font.
fn art_button(
    parent: &mut ChildSpawnerCommands,
    art: &Art,
    text: &str,
    id: &'static str,
    (on, off): (&'static str, &'static str),
    focused: bool,
) {
    let (Some(on), Some(off)) = (art.pieces.get(on), art.pieces.get(off)) else {
        return;
    };
    let mut button = parent.spawn((
        Button,
        TabIndex(0),
        FocusId(id),
        Node {
            flex_shrink: 0.0,
            width: px(on.size.x * UNIT),
            height: px(on.size.y * UNIT),
            justify_content: JustifyContent::Center,
            align_items: AlignItems::Center,
            ..default()
        },
        ImageNode::new(if focused {
            on.image.clone()
        } else {
            off.image.clone()
        }),
        ArtChoice {
            on: on.image.clone(),
            off: off.image.clone(),
        },
    ));
    if focused {
        button.insert(AutoFocus);
    }
    button.with_children(|b| bitmap_text(b, art, text, BUTTON_SIZE, BUTTON_COLOR));
}

/// The question in the disc's art (the module docs): its line, then the
/// disc's Yes and No, the focus where the original's prompt starts.
fn spawn_art(commands: &mut Commands, art: &Art, p: &Prompt) -> Entity {
    let line = p.text.as_deref().unwrap_or(UNREAD_LINE).to_owned();
    commands
        .spawn((
            PromptScreen(p.ui, true),
            Node {
                position_type: PositionType::Absolute,
                left: px(0),
                top: px(0),
                width: percent(100),
                height: percent(100),
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                ..default()
            },
            GlobalZIndex(PROMPT_Z),
        ))
        .with_children(|root| {
            root.spawn((
                Node {
                    flex_direction: FlexDirection::Column,
                    align_items: AlignItems::Center,
                    row_gap: px(32),
                    padding: UiRect::axes(px(64), px(40)),
                    border_radius: BorderRadius::all(px(BOX_CORNER)),
                    ..default()
                },
                BackgroundColor(BOX_FILL),
            ))
            .with_children(|panel| {
                bitmap_text(panel, art, &line, QUESTION_SIZE, QUESTION_COLOR);
                panel
                    .spawn(Node {
                        flex_direction: FlexDirection::Row,
                        column_gap: px(32),
                        ..default()
                    })
                    .with_children(|choices| {
                        art_button(
                            choices,
                            art,
                            "Yes",
                            PROMPT_YES,
                            (YES_ON, YES_OFF),
                            p.yes_first,
                        );
                        art_button(choices, art, "No", PROMPT_NO, (NO_ON, NO_OFF), !p.yes_first);
                    });
            });
        })
        .id()
}

/// The question over the level, dimmed: its line, then Yes and No.
fn spawn(commands: &mut Commands, pen: &Pen, p: &Prompt) -> Entity {
    let theme = &pen.theme;
    let line = p.text.as_deref().unwrap_or(UNREAD_LINE).to_owned();
    commands
        .spawn((
            PromptScreen(p.ui, false),
            Node {
                position_type: PositionType::Absolute,
                left: px(0),
                top: px(0),
                width: percent(100),
                height: percent(100),
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                ..default()
            },
            BackgroundColor(theme.shadow.with_alpha(0.45)),
            GlobalZIndex(PROMPT_Z),
        ))
        .with_children(|root| {
            root.spawn((
                Node {
                    flex_direction: FlexDirection::Column,
                    align_items: AlignItems::Center,
                    row_gap: px(40),
                    padding: UiRect::axes(px(72), px(56)),
                    border: UiRect::all(px(4)),
                    border_radius: BorderRadius::all(px(32)),
                    ..default()
                },
                BackgroundColor(theme.panel[1]),
                BorderColor::all(theme.gold),
            ))
            .with_children(|panel| {
                panel.spawn(pen.text(&line, Step::Lg, Role::Accent));
                panel
                    .spawn(Node {
                        flex_direction: FlexDirection::Row,
                        column_gap: px(48),
                        ..default()
                    })
                    .with_children(|choices| {
                        // The focus where the original's prompt starts.
                        if p.yes_first {
                            choices.spawn(choice_bundle(pen, "Yes", PROMPT_YES));
                            choices.spawn(other_choice_bundle(pen, "No", PROMPT_NO));
                        } else {
                            choices.spawn(other_choice_bundle(pen, "Yes", PROMPT_YES));
                            choices.spawn(choice_bundle(pen, "No", PROMPT_NO));
                        }
                    });
            });
        })
        .id()
}

/// The question as `/api/_test/tv` reports it.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct PromptShot {
    /// The question the run waits on, as the modal reads it.
    pub question: Option<String>,
    pub yes_first: bool,
    /// Every line drawn on the modal, the buttons' included; none drawn.
    pub texts: Vec<String>,
    /// Drawn in the disc's art, not the TV's own look.
    pub art: bool,
    /// When the modal was spawned, on the frame log's clock.
    pub shown_ms: Option<f64>,
}

/// The questions' part of `/api/_test/tv`: the one up, and those answered.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct PromptsShot {
    pub up: Option<PromptShot>,
    /// Each question answered this session, its text and the answer.
    pub answered: Vec<(Option<String>, bool)>,
}

/// What `/api/_test/tv` reads of the questions: the one up, the modal drawn
/// for it and those answered.
#[derive(bevy::ecs::system::SystemParam)]
pub(crate) struct PromptShots<'w, 's> {
    asked: Option<Res<'w, Asked>>,
    answered: Option<Res<'w, Answered>>,
    screens: Query<'w, 's, (Entity, &'static PromptScreen, Option<&'static ShownAt>)>,
    tree: UiTree<'w, 's>,
}

impl PromptShots<'_, '_> {
    /// None until a question has been asked.
    pub(crate) fn shot(&self) -> Option<PromptsShot> {
        let answered = self
            .answered
            .as_ref()
            .map(|a| a.0.clone())
            .unwrap_or_default();
        let up = self.asked.as_ref().and_then(|a| a.0.as_ref()).map(|p| {
            let drawn = self.screens.iter().find(|(_, s, _)| s.0 == p.ui);
            PromptShot {
                question: p.text.clone(),
                yes_first: p.yes_first,
                // The disc's art draws its lines glyph by glyph.
                texts: match drawn {
                    Some((_, s, _)) if s.1 => vec![
                        p.text.clone().unwrap_or_else(|| UNREAD_LINE.into()),
                        "Yes".into(),
                        "No".into(),
                    ],
                    Some((root, _, _)) => self.tree.walk(root).0,
                    None => Vec::new(),
                },
                art: drawn.is_some_and(|(_, s, _)| s.1),
                shown_ms: drawn.and_then(|(_, _, at)| at.map(|a| a.0.as_secs_f64() * 1e3)),
            }
        });
        (up.is_some() || !answered.is_empty()).then_some(PromptsShot { up, answered })
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use bevy::time::TimeUpdateStrategy;
    use guac_server::config::WindowMode;

    use std::collections::HashMap;

    use super::*;
    use crate::cards::PortalCards;
    use crate::completion::Piece;
    use crate::input::{PostedPresses, TestPress};
    use crate::test_support::tv::portal_app;

    /// The TV's portal app with the level showing (its layer would take the
    /// pads but for the modal), the questions' plugin and the clock 100 ms
    /// a frame; no run, so the scripts' step never reads or answers.
    fn asking_app() -> (App, PostedPresses) {
        let mut app = portal_app(1920, 1080, WindowMode::Tv, PortalCards::default());
        let presses = PostedPresses::default();
        app.add_plugins(YesNoPlugin)
            .insert_resource(presses.clone())
            .insert_resource(TimeUpdateStrategy::ManualDuration(Duration::from_millis(
                100,
            )));
        app.world_mut()
            .resource_mut::<crate::pad::LevelPads>()
            .show();
        (app, presses)
    }

    /// A question's line in the test's own words (a disc's line is never
    /// in the tree).
    const LINE: &str = "The test's own question";

    fn question(ui: u64, yes_first: bool) -> Prompt {
        Prompt {
            ui: Value::Entity(ui),
            question: Value::Int(7),
            text: Some(LINE.into()),
            yes_first,
            page: Value::Entity(ui + 1),
        }
    }

    /// openspec AU.29: a button on the disc's art draws its backdrop for the
    /// focus: focused its own, off it the desaturated one, moved as the
    /// focus moves.
    #[test]
    fn the_discs_buttons_show_their_backdrop_for_the_focus() {
        let mut app = App::new();
        app.init_resource::<InputFocus>()
            .add_systems(Update, art_focus);
        let (on, off): (Handle<Image>, Handle<Image>) = (
            Handle::default(),
            Handle::Uuid(Default::default(), Default::default()),
        );
        let choice = || ArtChoice {
            on: on.clone(),
            off: off.clone(),
        };
        let a = app
            .world_mut()
            .spawn((choice(), ImageNode::new(off.clone())))
            .id();
        let b = app
            .world_mut()
            .spawn((choice(), ImageNode::new(on.clone())))
            .id();
        app.world_mut()
            .resource_mut::<InputFocus>()
            .set(a, FocusCause::Navigated);
        app.update();
        let image = |app: &App, e| app.world().get::<ImageNode>(e).unwrap().image.clone();
        assert_eq!(image(&app, a), on);
        assert_eq!(image(&app, b), off);
        app.world_mut()
            .resource_mut::<InputFocus>()
            .set(b, FocusCause::Navigated);
        app.update();
        assert_eq!(image(&app, a), off);
        assert_eq!(image(&app, b), on);
    }

    /// openspec AU.29: the modal on the disc's art puts the focus where the
    /// original's prompt starts, each button on its backdrop for it.
    #[test]
    fn the_discs_modal_starts_where_the_originals_prompt_does() {
        for yes_first in [true, false] {
            let mut app = App::new();
            let on = |n: u128| {
                Handle::<Image>::Uuid(bevy::asset::uuid::Uuid::from_u128(n), Default::default())
            };
            let piece = |image| Piece {
                image,
                size: Vec2::new(227.0, 102.0),
            };
            let pieces = HashMap::from([
                (YES_ON, piece(on(1))),
                (NO_ON, piece(on(2))),
                (YES_OFF, piece(on(3))),
                (NO_OFF, piece(on(4))),
            ]);
            let art = Art::for_tests(pieces);
            let p = question(0x70, yes_first);
            let mut commands_queue = bevy::ecs::world::CommandQueue::default();
            let mut commands = Commands::new(&mut commands_queue, app.world());
            spawn_art(&mut commands, &art, &p);
            commands_queue.apply(app.world_mut());
            let mut q = app
                .world_mut()
                .query::<(&FocusId, &ImageNode, Option<&AutoFocus>)>();
            let drawn: Vec<(&'static str, Handle<Image>, bool)> = q
                .iter(app.world())
                .map(|(id, image, auto)| (id.0, image.image.clone(), auto.is_some()))
                .collect();
            let of = |id: &str| drawn.iter().find(|d| d.0 == id).unwrap().clone();
            assert_eq!(of(PROMPT_YES).2, yes_first);
            assert_eq!(of(PROMPT_NO).2, !yes_first);
            assert_eq!(of(PROMPT_YES).1, on(if yes_first { 1 } else { 3 }));
            assert_eq!(of(PROMPT_NO).1, on(if yes_first { 4 } else { 2 }));
        }
    }

    /// The lines the modal draws, as `/api/_test/tv` reads them.
    fn lines(app: &mut App) -> Vec<String> {
        let mut state = bevy::ecs::system::SystemState::<PromptShots>::new(app.world_mut());
        let shots = state.get(app.world()).expect("the shots' params");
        shots
            .shot()
            .and_then(|s| s.up)
            .map(|u| u.texts)
            .unwrap_or_default()
    }

    fn focused(app: &App) -> Option<&'static str> {
        let e = app.world().resource::<InputFocus>().get()?;
        app.world().get::<FocusId>(e).map(|f| f.0)
    }

    fn drawn(app: &mut App) -> Option<Value> {
        let mut q = app.world_mut().query::<&PromptScreen>();
        q.iter(app.world()).next().map(|s| s.0)
    }

    /// Frames until the modal is drawn for `ui` with `focus` focused, up to
    /// 40.
    fn shows(app: &mut App, ui: Value, focus: &str) {
        for _ in 0..40 {
            app.update();
            if drawn(app) == Some(ui) && focused(app) == Some(focus) {
                return;
            }
        }
        panic!(
            "never {ui:?} on {focus}: {:?} on {:?}",
            drawn(app),
            focused(app)
        );
    }

    /// openspec AU.6: a question up opens the modal through the router (the
    /// level's layer under it), drawn with its line (the TV's own where its
    /// string doesn't read), then Yes and No, the focus on Yes where the
    /// original's starts there (on No otherwise); South answers
    /// the focused choice for the next step, the modal closing with it; a
    /// move right reaches No. No question, no modal; the level gone from the
    /// TV, the question forgotten.
    #[test]
    fn a_question_is_a_modal_whose_choice_is_the_next_steps_answer() {
        let (mut app, presses) = asking_app();
        for _ in 0..3 {
            app.update();
        }
        assert!(!app.world().resource::<PromptOpen>().0);
        assert_eq!(drawn(&mut app), None);
        app.world_mut().resource_mut::<Asked>().0 = Some(question(0x40, true));
        shows(&mut app, Value::Entity(0x40), PROMPT_YES);
        assert!(app.world().resource::<PromptOpen>().0);
        assert_eq!(
            lines(&mut app),
            [LINE, "Yes", "No"],
            "its line, then the two"
        );
        presses.push(TestPress::South);
        for _ in 0..3 {
            app.update();
        }
        assert_eq!(
            app.world().resource::<Answer>().0,
            Some((question(0x40, true), true))
        );
        assert!(
            !app.world().resource::<PromptOpen>().0,
            "closed on its answer"
        );
        assert_eq!(drawn(&mut app), None);

        // The level gone from the TV under a question: the modal closes and
        // the question is forgotten, nothing answered.
        let (mut app, _) = asking_app();
        app.world_mut().resource_mut::<Asked>().0 = Some(question(0x60, true));
        shows(&mut app, Value::Entity(0x60), PROMPT_YES);
        app.world_mut()
            .resource_mut::<crate::pad::LevelPads>()
            .hide();
        for _ in 0..3 {
            app.update();
        }
        assert!(!app.world().resource::<PromptOpen>().0);
        assert_eq!(drawn(&mut app), None);
        assert_eq!(app.world().resource::<Asked>().0, None);
        assert_eq!(app.world().resource::<Answer>().0, None);

        // Starting on No where the original's does; right reaches Yes, left
        // back to No, and South answers no.
        let (mut app, presses) = asking_app();
        let unread = Prompt {
            text: None,
            ..question(0x50, false)
        };
        app.world_mut().resource_mut::<Asked>().0 = Some(unread);
        shows(&mut app, Value::Entity(0x50), PROMPT_NO);
        assert_eq!(
            lines(&mut app),
            [UNREAD_LINE, "Yes", "No"],
            "its string unread"
        );
        presses.push(TestPress::Right);
        shows(&mut app, Value::Entity(0x50), PROMPT_YES);
        presses.push(TestPress::Left);
        shows(&mut app, Value::Entity(0x50), PROMPT_NO);
        presses.push(TestPress::South);
        for _ in 0..3 {
            app.update();
        }
        assert_eq!(
            app.world()
                .resource::<Answer>()
                .0
                .as_ref()
                .map(|(p, yes)| (p.ui, *yes)),
            Some((Value::Entity(0x50), false))
        );
    }

    /// A run of no scripts of its own, as the TV installs one: what the
    /// modal reads to forget a question once its run is taken down.
    fn a_run() -> ScriptRuntime {
        use std::sync::Arc;

        use guac_formats::igz::Igz;
        use guac_formats::script::{Graph, builder};
        use guac_script::{Config, Generator, Mode, Program, Role, Unit, Vm};
        let bytes = builder::ui_containers();
        let igz = Igz::parse(&bytes).unwrap();
        let graph = Graph::read(&igz).unwrap();
        let unit = Unit {
            name: "global",
            role: Role::Other,
            igz: &igz,
            graph: &graph,
        };
        let program = Arc::new(Program::link(&[unit]).unwrap());
        ScriptRuntime {
            vm: Vm::new(program, Config::new(Mode::Strict)),
            generator: Generator::seeded(1),
            player_loop: Default::default(),
            steps: 0,
            stopped: Vec::new(),
            entry_stopped: None,
        }
    }

    /// openspec AU.6's review: while the question is up the router gives the
    /// pads to the modal alone. Start on the walking pad pauses nothing; a
    /// second pad's moves and South answer it as the first's would (either
    /// player answers). The portal holding the level hides it, unanswerable,
    /// until the hold ends. The run taken down under a question (a level
    /// change, Restart chapter) closes the modal and forgets it, so no
    /// answer lands on a later run's dialogue.
    #[test]
    fn the_question_holds_the_pads_and_goes_with_its_run() {
        use bevy::input::gamepad::GamepadButton;

        use crate::test_support::tv::{pad, tap};
        let (mut app, _) = asking_app();
        let first = pad(&mut app);
        let second = pad(&mut app);
        for _ in 0..3 {
            app.update();
        }
        let level = app.world().resource::<crate::pad::LevelPads>();
        assert!(!level.paused(), "the first pad walks");
        app.world_mut().resource_mut::<Asked>().0 = Some(question(0x70, true));
        shows(&mut app, Value::Entity(0x70), PROMPT_YES);
        tap(&mut app, first, GamepadButton::Start);
        for _ in 0..3 {
            app.update();
        }
        assert!(
            !app.world().resource::<crate::pad::LevelPads>().paused(),
            "Start under the question pauses nothing"
        );
        assert!(app.world().resource::<PromptOpen>().0);
        tap(&mut app, second, GamepadButton::DPadRight);
        shows(&mut app, Value::Entity(0x70), PROMPT_NO);
        tap(&mut app, second, GamepadButton::South);
        for _ in 0..3 {
            app.update();
        }
        assert_eq!(
            app.world()
                .resource::<Answer>()
                .0
                .as_ref()
                .map(|(p, yes)| (p.ui, *yes)),
            Some((Value::Entity(0x70), false)),
            "the second pad answered no"
        );
        // The portal holding the level (its alert, an arrival card): the
        // question waits under it, the modal gone and nothing answerable,
        // then back with its focus once the hold ends.
        let (mut app, presses) = asking_app();
        app.world_mut().resource_mut::<Asked>().0 = Some(question(0x90, true));
        shows(&mut app, Value::Entity(0x90), PROMPT_YES);
        app.world_mut()
            .resource_mut::<crate::pad::LevelPads>()
            .hold(true);
        for _ in 0..3 {
            app.update();
        }
        assert!(!app.world().resource::<PromptOpen>().0, "under the hold");
        assert_eq!(drawn(&mut app), None);
        presses.push(TestPress::South);
        for _ in 0..3 {
            app.update();
        }
        assert_eq!(
            app.world().resource::<Answer>().0,
            None,
            "nothing answered blind"
        );
        assert_eq!(
            app.world().resource::<Asked>().0,
            Some(question(0x90, true)),
            "the question kept"
        );
        app.world_mut()
            .resource_mut::<crate::pad::LevelPads>()
            .hold(false);
        shows(&mut app, Value::Entity(0x90), PROMPT_YES);
        // A question up under a run, then the run taken down.
        let (mut app, _) = asking_app();
        app.insert_resource(a_run());
        app.world_mut().resource_mut::<Asked>().0 = Some(question(0x80, true));
        shows(&mut app, Value::Entity(0x80), PROMPT_YES);
        app.world_mut().remove_resource::<ScriptRuntime>();
        for _ in 0..3 {
            app.update();
        }
        assert!(
            !app.world().resource::<PromptOpen>().0,
            "closed with its run"
        );
        assert_eq!(drawn(&mut app), None);
        assert_eq!(app.world().resource::<Asked>().0, None);
        assert_eq!(app.world().resource::<Answer>().0, None);
    }
}
