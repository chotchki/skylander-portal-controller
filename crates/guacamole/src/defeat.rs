//! Restart chapter on the TV (openspec AH.26, AH design decision 14;
//! tv-hud's No character left): with the hero beaten and no character left
//! to come in, the portal's alert asks for the beaten figure to come off,
//! then for another ([`crate::alert`]), and from
//! [`RESTART_AFTER`](crate::play::RESTART_AFTER) after his
//! health ran out this one button sits under it: Restart chapter, behind a
//! yes-or-no check that starts on No, as the original's does. Yes ends the
//! level and starts it again ([`RestartLevel`], [`crate::play`]'s
//! `restart`). Ours until AM's menus run the original's defeat screen
//! (stand-in ledger: `no-character`).
//!
//! It's a modal ([`ModalOpen`]): the router hands it the pads and keys as
//! the screen's inputs while it shows, the level held under the alert. South
//! presses the focused choice (the router's Activate), a move left or right
//! goes between Yes and No, Back closes the check.

use bevy::ecs::system::SystemParam;
use bevy::input_focus::{FocusCause, InputFocus};
use bevy::math::CompassOctant;
use bevy::prelude::*;
use bevy::ui_widgets::Activate;
use serde::Serialize;
use tracing::info;

use crate::input::{FocusId, ModalOpen, TvInput};
use crate::pause::ShownAt;
use crate::play::{Play, PlaySystems, RestartLevel};
use crate::theme::{Role, Step, TvTheme};
use crate::tv::{choice_bundle, other_choice_bundle};
use crate::tv_state::{Pen, Redrawn, TvAssets, UiTree};

/// Over the portal's alert (940), under the corner join code (960).
const RESTART_Z: i32 = 950;

/// The choices' names, for the router's focus and the mirror.
pub(crate) const RESTART_CHAPTER: &str = "restart-chapter";
pub(crate) const RESTART_YES: &str = "restart-yes";
pub(crate) const RESTART_NO: &str = "restart-no";

/// Where the choice is: its one button, or the check after it.
#[derive(Resource, Debug, Default, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RestartAsk {
    #[default]
    Offer,
    Confirm,
}

impl RestartAsk {
    pub(crate) fn name(self) -> &'static str {
        match self {
            Self::Offer => "offer",
            Self::Confirm => "confirm",
        }
    }
}

/// The choice drawn, its root.
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct RestartScreen(pub(crate) RestartAsk);

pub(crate) const CONFIRM_LINE: &str = "Restart the chapter?";

pub(crate) struct DefeatPlugin;

impl Plugin for DefeatPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<RestartAsk>()
            .init_resource::<ModalOpen>()
            .add_message::<TvInput>()
            .add_message::<RestartLevel>()
            .add_observer(on_restart_choice)
            .add_systems(
                Update,
                (offer_restart, check_moves, show_restart)
                    .chain()
                    .after(PlaySystems),
            );
    }
}

/// The choice is open while [`Play::restart_offered`] says so, back to its
/// one button whenever it isn't; the modal with it.
fn offer_restart(
    play: Option<Res<Play>>,
    time: Res<Time<Real>>,
    mut ask: ResMut<RestartAsk>,
    mut modal: ResMut<ModalOpen>,
) {
    let offered = play.is_some_and(|p| p.restart_offered(time.elapsed()));
    if modal.0 != offered {
        info!(offered, "Restart chapter");
        modal.0 = offered;
    }
    if !offered && *ask != RestartAsk::Offer {
        *ask = RestartAsk::Offer;
    }
}

/// The check's moves: left or right goes between Yes and No; Back closes it.
fn check_moves(
    mut inputs: MessageReader<TvInput>,
    modal: Res<ModalOpen>,
    mut ask: ResMut<RestartAsk>,
    focus: Option<ResMut<InputFocus>>,
    choices: Query<(Entity, &FocusId)>,
) {
    let read: Vec<TvInput> = inputs.read().copied().collect();
    if !modal.0 || *ask != RestartAsk::Confirm {
        return;
    }
    let Some(mut focus) = focus else {
        return;
    };
    let find = |id: &str| choices.iter().find(|(_, f)| f.0 == id).map(|(e, _)| e);
    for input in read {
        match input {
            TvInput::Back => {
                info!("Restart chapter: back from the check");
                *ask = RestartAsk::Offer;
                return;
            }
            TvInput::Move(CompassOctant::West | CompassOctant::East) => {
                let (yes, no) = (find(RESTART_YES), find(RESTART_NO));
                let other = if focus.get() == yes { no } else { yes };
                if let Some(e) = other {
                    focus.set(e, FocusCause::Navigated);
                }
            }
            _ => {}
        }
    }
}

/// What a choice does: Restart chapter opens the check, Yes restarts the
/// level, No closes the check.
fn on_restart_choice(
    activate: On<Activate>,
    ids: Query<&FocusId>,
    modal: Option<Res<ModalOpen>>,
    mut ask: ResMut<RestartAsk>,
    mut restart: MessageWriter<RestartLevel>,
) {
    let Ok(id) = ids.get(activate.entity) else {
        return;
    };
    if !modal.is_some_and(|m| m.0) {
        return;
    }
    match id.0 {
        RESTART_CHAPTER => *ask = RestartAsk::Confirm,
        RESTART_YES => {
            info!("Restart chapter: yes");
            restart.write(RestartLevel);
            *ask = RestartAsk::Offer;
        }
        RESTART_NO => *ask = RestartAsk::Offer,
        _ => {}
    }
}

/// Spawns the choice as it stands and despawns one it no longer is,
/// telling the shots. Needs the TV's font and theme; the headless tests
/// without them draw nothing, the choice working all the same.
fn show_restart(
    mut commands: Commands,
    modal: Res<ModalOpen>,
    ask: Res<RestartAsk>,
    (assets, theme, time): (Option<Res<TvAssets>>, Option<Res<TvTheme>>, Res<Time<Real>>),
    mut redrawn: MessageWriter<Redrawn>,
    mut shown: Local<Option<(RestartAsk, Entity)>>,
) {
    let want = modal.0.then_some(*ask);
    if shown.map(|(a, _)| a) == want {
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
    *shown = Some((want, entity));
    redrawn.write(Redrawn::Alert);
}

/// The one button low on the screen, under the alert's panel; or the check
/// over everything, dimmed, Yes then No, No focused.
fn spawn(commands: &mut Commands, pen: &Pen, ask: RestartAsk) -> Entity {
    let theme = &pen.theme;
    match ask {
        RestartAsk::Offer => commands
            .spawn((
                RestartScreen(ask),
                Node {
                    position_type: PositionType::Absolute,
                    left: px(0),
                    right: px(0),
                    bottom: px(96),
                    justify_content: JustifyContent::Center,
                    ..default()
                },
                GlobalZIndex(RESTART_Z),
            ))
            .with_children(|row| {
                row.spawn(choice_bundle(pen, "Restart chapter", RESTART_CHAPTER));
            })
            .id(),
        RestartAsk::Confirm => commands
            .spawn((
                RestartScreen(ask),
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
                BackgroundColor(theme.shadow.with_alpha(0.6)),
                GlobalZIndex(RESTART_Z),
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
                    panel.spawn(pen.text(CONFIRM_LINE, Step::Lg, Role::Accent));
                    panel
                        .spawn(Node {
                            flex_direction: FlexDirection::Row,
                            column_gap: px(48),
                            ..default()
                        })
                        .with_children(|choices| {
                            // No starts focused, as the original's does.
                            choices.spawn(other_choice_bundle(pen, "Yes", RESTART_YES));
                            choices.spawn(choice_bundle(pen, "No", RESTART_NO));
                        });
                });
            })
            .id(),
    }
}

/// The choice as drawn, its part of `/api/_test/tv`.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct RestartShot {
    /// `offer` (its one button) or `confirm` (the check).
    pub stage: String,
    /// Every line drawn on it, top to bottom, the buttons' included.
    pub texts: Vec<String>,
    /// When it was spawned, on the frame log's clock.
    pub shown_ms: Option<f64>,
}

/// A defeat as the TV holds it (AH.26), its part of `/api/_test/tv`.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct DefeatShot {
    /// The hero's health ran out and he isn't out yet.
    pub dying: bool,
    /// The figures beaten in this level, each `<profile>/<figure>` (`-`
    /// for no profile).
    pub beaten: Vec<String>,
    /// Restart chapter, while it's offered.
    pub restart: Option<RestartShot>,
}

/// The defeat's state and its choice as drawn.
#[derive(SystemParam)]
pub(crate) struct DefeatShots<'w, 's> {
    play: Option<Res<'w, Play>>,
    screens: Query<'w, 's, (Entity, &'static RestartScreen, Option<&'static ShownAt>)>,
    tree: UiTree<'w, 's>,
}

impl DefeatShots<'_, '_> {
    /// None outside a level with nothing to report.
    pub(crate) fn shot(&self) -> Option<DefeatShot> {
        let play = self.play.as_deref()?;
        let restart = self.screens.iter().next().map(|(root, s, at)| RestartShot {
            stage: s.0.name().to_owned(),
            texts: self.tree.walk(root).0,
            shown_ms: at.map(|a| a.0.as_secs_f64() * 1e3),
        });
        let shot = DefeatShot {
            dying: play.dying(),
            beaten: play
                .beaten()
                .iter()
                .map(|(p, f)| format!("{}/{f}", p.as_deref().unwrap_or("-")))
                .collect(),
            restart,
        };
        (shot != DefeatShot::default()).then_some(shot)
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
    use crate::play::Alert;
    use crate::test_support::tv::portal_app;

    #[derive(Resource, Default)]
    struct Restarts(usize);

    fn count_restarts(mut asked: MessageReader<RestartLevel>, mut n: ResMut<Restarts>) {
        n.0 += asked.read().count();
    }

    /// The TV's portal app with the walk held under `alert` after a defeat
    /// at 1 s on the real clock, the choice's plugin and a counter of the
    /// restarts it asks for; the clock 100 ms a frame.
    fn beaten_app(alert: Alert) -> (App, PostedPresses) {
        let mut app = portal_app(1920, 1080, WindowMode::Tv, PortalCards::default());
        let presses = PostedPresses::default();
        app.add_plugins(DefeatPlugin)
            .insert_resource(presses.clone())
            .init_resource::<Restarts>()
            .add_systems(Update, count_restarts)
            .insert_resource(TimeUpdateStrategy::ManualDuration(Duration::from_millis(
                100,
            )));
        let camera = app.world_mut().spawn_empty().id();
        app.insert_resource(Play::beaten_for_tests(
            camera,
            alert,
            Duration::from_secs(1),
        ));
        // The level shows, held: its layer would take the pads but for the
        // modal.
        let mut pads = app.world_mut().resource_mut::<crate::pad::LevelPads>();
        pads.show();
        pads.hold(true);
        (app, presses)
    }

    fn drawn(app: &mut App) -> Option<RestartAsk> {
        let mut q = app.world_mut().query::<&RestartScreen>();
        q.iter(app.world()).next().map(|s| s.0)
    }

    fn focused(app: &App) -> Option<&'static str> {
        let e = app.world().resource::<InputFocus>().get()?;
        app.world().get::<FocusId>(e).map(|f| f.0)
    }

    /// Frames until the choice is drawn as `want` with `focus` focused, up
    /// to 80 (8 s on the test's clock).
    fn shows(app: &mut App, want: RestartAsk, focus: &str) {
        for _ in 0..80 {
            app.update();
            if drawn(app) == Some(want) && focused(app) == Some(focus) {
                return;
            }
        }
        panic!(
            "never {want:?} on {focus}: {:?} on {:?}",
            drawn(app),
            focused(app)
        );
    }

    fn restarts(app: &App) -> usize {
        app.world().resource::<Restarts>().0
    }

    /// tv-hud's No character left through AP's router: nothing until 5 s
    /// after his health ran out, then one button, Restart chapter, focused;
    /// South (a posted press, the router's Activate) opens the check on No;
    /// Back closes it; South again and a move right to Yes, then South,
    /// asks for the restart once. The same over the call for a Skylander;
    /// never with a hero in.
    #[test]
    fn restart_chapter_is_one_button_through_the_router_behind_a_check_on_no() {
        for alert in [Alert::NeedsRest, Alert::PlaceSkylander] {
            let (mut app, presses) = beaten_app(alert);
            // 1 s to 5.9 s on the real clock: not yet.
            for _ in 0..49 {
                app.update();
            }
            assert_eq!(drawn(&mut app), None, "{alert:?}: not before 5 s");
            assert!(!app.world().resource::<ModalOpen>().0);
            shows(&mut app, RestartAsk::Offer, RESTART_CHAPTER);
            assert!(app.world().resource::<ModalOpen>().0);
            presses.push(TestPress::South);
            shows(&mut app, RestartAsk::Confirm, RESTART_NO);
            presses.push(TestPress::East);
            shows(&mut app, RestartAsk::Offer, RESTART_CHAPTER);
            assert_eq!(restarts(&app), 0, "{alert:?}");
            presses.push(TestPress::South);
            shows(&mut app, RestartAsk::Confirm, RESTART_NO);
            presses.push(TestPress::Right);
            shows(&mut app, RestartAsk::Confirm, RESTART_YES);
            presses.push(TestPress::South);
            for _ in 0..5 {
                app.update();
            }
            assert_eq!(restarts(&app), 1, "{alert:?}: once");
            assert_eq!(drawn(&mut app), Some(RestartAsk::Offer));
        }
        // No on the check: back to the button, nothing asked.
        let (mut app, presses) = beaten_app(Alert::NeedsRest);
        shows(&mut app, RestartAsk::Offer, RESTART_CHAPTER);
        presses.push(TestPress::South);
        shows(&mut app, RestartAsk::Confirm, RESTART_NO);
        presses.push(TestPress::South);
        shows(&mut app, RestartAsk::Offer, RESTART_CHAPTER);
        assert_eq!(restarts(&app), 0);
        // The check open, a hero in (the walk's alert gone): no choice, no
        // modal; the next defeat's choice opens on its button again.
        presses.push(TestPress::South);
        shows(&mut app, RestartAsk::Confirm, RESTART_NO);
        let camera = app.world_mut().spawn_empty().id();
        app.insert_resource(Play::started_for_tests(camera));
        for _ in 0..3 {
            app.update();
        }
        assert_eq!(drawn(&mut app), None);
        assert!(!app.world().resource::<ModalOpen>().0);
        app.insert_resource(Play::beaten_for_tests(
            camera,
            Alert::NeedsRest,
            Duration::ZERO,
        ));
        shows(&mut app, RestartAsk::Offer, RESTART_CHAPTER);
        // Beaten with characters on the portal (too many of them, or a toy
        // Giants can't use beside him): no choice, however long it holds.
        for alert in [Alert::TooManySkylanders, Alert::WrongToy] {
            let (mut app, _) = beaten_app(alert);
            for _ in 0..80 {
                app.update();
            }
            assert_eq!(drawn(&mut app), None, "{alert:?}");
            assert!(!app.world().resource::<ModalOpen>().0, "{alert:?}");
        }
    }
}
