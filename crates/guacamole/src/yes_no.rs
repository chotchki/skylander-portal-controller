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
//!   presses the focused one. Laid out in the TV's own look for now (stand-in
//!   ledger: `yes-no`).

use bevy::input_focus::{FocusCause, InputFocus};
use bevy::math::CompassOctant;
use bevy::prelude::*;
use bevy::ui_widgets::Activate;
use guac_script::Value;
use guacamole::prompt::{self, Prompt};
use guacamole::scripts::{ScriptClone, ScriptRuntime, ScriptWorld, WorldHost};
use serde::Serialize;
use tracing::{info, warn};

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

/// The question the run waits on, as its last step left it.
#[derive(Resource, Debug, Default, Clone, PartialEq)]
pub(crate) struct Asked(pub(crate) Option<Prompt>);

/// The answer chosen on the modal, for the next step to run: the dialogue
/// UI it answers, and yes or no.
#[derive(Resource, Debug, Default, Clone, Copy, PartialEq)]
pub(crate) struct Answer(pub(crate) Option<(Value, bool)>);

/// The modal is up: the router hands it the pads (as [`crate::input::ModalOpen`]
/// does Restart chapter).
#[derive(Resource, Debug, Default, Clone, Copy, PartialEq, Eq)]
pub(crate) struct PromptOpen(pub(crate) bool);

/// The modal's root, for the dialogue UI whose question it asks.
#[derive(Component, Debug, Clone, Copy, PartialEq)]
pub(crate) struct PromptScreen(Value);

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
            .add_systems(Update, (open, moves, show).chain());
    }
}

/// After the scripts' step: an answer chosen for the question still up run
/// on it ([`prompt::answer`]), then the question the run waits on read.
fn ask_and_answer(
    mut commands: Commands,
    mut runtime: ResMut<ScriptRuntime>,
    mut world: ResMut<ScriptWorld>,
    mut asked: ResMut<Asked>,
    mut answer: ResMut<Answer>,
    mut answered: ResMut<Answered>,
) {
    if let Some((ui, yes)) = answer.0.take()
        && let Some(p) = asked.0.clone().filter(|p| p.ui == ui)
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
/// level shows. With the level gone from the TV (its run taken down, or the
/// portal view in its place) the question and any answer are forgotten: a
/// run still asking is read again at its next step.
fn open(
    mut asked: ResMut<Asked>,
    mut answer: ResMut<Answer>,
    pads: Option<Res<crate::pad::LevelPads>>,
    mut open: ResMut<PromptOpen>,
) {
    let shows = pads.is_some_and(|p| p.shows());
    if !shows && (asked.0.is_some() || answer.0.is_some()) {
        asked.0 = None;
        answer.0 = None;
    }
    let up = shows && asked.0.is_some() && answer.0.is_none();
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
        answer.0 = Some((p.ui, yes));
    }
}

/// Spawns the modal for the question up and despawns one no longer up,
/// telling the shots. Needs the TV's font and theme; headless tests without
/// them draw nothing, the question answered all the same.
fn show(
    mut commands: Commands,
    open: Res<PromptOpen>,
    asked: Res<Asked>,
    (assets, theme, time): (Option<Res<TvAssets>>, Option<Res<TvTheme>>, Res<Time<Real>>),
    mut redrawn: MessageWriter<Redrawn>,
    mut shown: Local<Option<(Value, Entity)>>,
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
    let entity = spawn(&mut commands, &pen, want);
    commands.entity(entity).insert(ShownAt(time.elapsed()));
    *shown = Some((want.ui, entity));
    redrawn.write(Redrawn::Alert);
}

/// The question over the level, dimmed: its line, then Yes and No.
fn spawn(commands: &mut Commands, pen: &Pen, p: &Prompt) -> Entity {
    let theme = &pen.theme;
    let line = p.text.as_deref().unwrap_or(UNREAD_LINE).to_owned();
    commands
        .spawn((
            PromptScreen(p.ui),
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
                texts: drawn
                    .map(|(root, _, _)| self.tree.walk(root).0)
                    .unwrap_or_default(),
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

    use super::*;
    use crate::cards::PortalCards;
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
            Some((Value::Entity(0x40), true))
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
            app.world().resource::<Answer>().0,
            Some((Value::Entity(0x50), false))
        );
    }
}
