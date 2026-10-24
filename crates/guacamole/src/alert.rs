//! The portal's alerts on the TV (openspec AG.29, AG.63, AG.73; level-scene's
//! One hero at a time, Other figures leave the level as it is, Removing the
//! hero pauses the level and A figure Giants can't use holds the level), and
//! the small join code over the level.
//!
//! - One alert at a time, the one [`crate::play::judge`] rules, full screen
//!   and opaque in the TV's own look: the original hides the level behind each
//!   of its portal messages (AG.6, AG.64). Ours in place of its screens
//!   until AM's menus, their words ours too
//!   (stand-in ledger: `portal-alerts`).
//! - TV UI over the level (AG.50, [`crate::level::tv_over_level`]): over the
//!   pause overlays, under the corner join code and an intro.
//! - The small join code (AA's join requirement: it stays on screen once a
//!   game is picked) sits in the portal view's header, which the level and a
//!   full-screen alert cover; so while either shows, the join QR stands in
//!   the corner over them, so another kid can still join.

use bevy::prelude::*;

use crate::pause::ShownAt;
use crate::play::{Alert, Play, PlaySystems};
use crate::screen::TvScreen;
use crate::theme::{Role, Step, TvTheme};
use crate::tv::{Current, Pen, Redrawn, TvAssets};

/// Over the pause overlays (900), under the corner join code.
const ALERT_Z: i32 = 940;

/// Over an alert, under an intro (1,000).
const CORNER_Z: i32 = 960;

/// The corner's QR, design px: the portal header's.
const CORNER_QR: f32 = 176.0;

/// An alert drawn, its root.
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct AlertScreen(pub(crate) Alert);

/// The small join code standing in the corner over the level or an alert.
#[derive(Component, Debug)]
pub(crate) struct CornerJoin;

/// The corner's QR image, kept on the TV's current join code; the mirror
/// counts it as the join QR drawn.
#[derive(Component, Debug)]
pub(crate) struct CornerQr;

/// What an alert says: its title, then what to do.
pub(crate) fn lines(alert: Alert) -> [&'static str; 2] {
    match alert {
        Alert::WrongToy => [
            "A toy on the portal can't be used with this game",
            "Please take it off the portal",
        ],
        Alert::TooManySkylanders => [
            "Too many Skylanders on the portal",
            "Take one off to play on",
        ],
        Alert::TooManyItems => [
            "Too many magic items on the portal",
            "Take one off to play on",
        ],
        Alert::PlaceSkylander => ["Player 1, put a Skylander on the portal", "to play on"],
    }
}

pub(crate) struct AlertPlugin;

impl Plugin for AlertPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Update, (show_alert, show_corner_join).after(PlaySystems));
    }
}

/// Spawns the alert the portal asks for and despawns one it no longer
/// does, telling the shots ([`Redrawn::Alert`]). Needs the TV's font and
/// theme; the headless tests without them draw nothing.
fn show_alert(
    mut commands: Commands,
    play: Option<Res<Play>>,
    (assets, theme, time): (Option<Res<TvAssets>>, Option<Res<TvTheme>>, Res<Time<Real>>),
    mut redrawn: MessageWriter<Redrawn>,
    mut shown: Local<Option<(Alert, Entity)>>,
) {
    let want = play.and_then(|p| p.alert());
    if shown.map(|(a, _)| a) == want {
        return;
    }
    if let Some((_, entity)) = shown.take()
        && let Ok(mut alert) = commands.get_entity(entity)
    {
        alert.despawn();
        redrawn.write(Redrawn::Alert);
    }
    let (Some(want), Some(assets), Some(theme)) = (want, assets, theme) else {
        return;
    };
    let pen = Pen::new(&assets, &theme);
    let entity = spawn_alert(&mut commands, &pen, want);
    commands.entity(entity).insert(ShownAt(time.elapsed()));
    *shown = Some((want, entity));
    redrawn.write(Redrawn::Alert);
}

/// The alert full screen: the TV's night sky, opaque, under a blue panel in
/// a gold edge holding its two lines.
fn spawn_alert(commands: &mut Commands, pen: &Pen, alert: Alert) -> Entity {
    let theme = &pen.theme;
    let [title, then] = lines(alert);
    commands
        .spawn((
            AlertScreen(alert),
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
            BackgroundColor(theme.starfield[1]),
            GlobalZIndex(ALERT_Z),
        ))
        .with_children(|root| {
            root.spawn((
                Node {
                    flex_direction: FlexDirection::Column,
                    align_items: AlignItems::Center,
                    row_gap: px(32),
                    max_width: percent(80),
                    padding: UiRect::axes(px(72), px(56)),
                    border: UiRect::all(px(4)),
                    border_radius: BorderRadius::all(px(32)),
                    ..default()
                },
                BackgroundColor(theme.panel[1]),
                BorderColor::all(theme.gold),
            ))
            .with_children(|panel| {
                panel.spawn(pen.text(title, Step::Lg, Role::Accent));
                panel.spawn(pen.text(then, Step::Body, Role::Primary));
            });
        })
        .id()
}

/// The small join code in the corner while the level or an alert covers the
/// portal view's header; gone with them.
fn show_corner_join(
    mut commands: Commands,
    play: Option<Res<Play>>,
    current: Option<Res<Current>>,
    (assets, theme): (Option<Res<TvAssets>>, Option<Res<TvTheme>>),
    mut qrs: Query<&mut ImageNode, With<CornerQr>>,
    mut redrawn: MessageWriter<Redrawn>,
    mut shown: Local<Option<Entity>>,
) {
    let on_portal = current.is_some_and(|c| matches!(c.screen, TvScreen::Portal { .. }));
    let covered = play.is_some_and(|p| p.started() || p.alert().is_some());
    let want = on_portal && covered;
    let qr = assets.as_ref().and_then(|a| a.qr.clone());
    if let (true, Some(qr)) = (shown.is_some(), &qr) {
        for mut image in &mut qrs {
            if image.image != *qr {
                image.image = qr.clone();
            }
        }
    }
    if shown.is_some() == want {
        return;
    }
    if let Some(entity) = shown.take() {
        if let Ok(mut corner) = commands.get_entity(entity) {
            corner.despawn();
        }
        redrawn.write(Redrawn::Alert);
        return;
    }
    let (Some(assets), Some(theme)) = (assets, theme) else {
        return;
    };
    let pen = Pen::new(&assets, &theme);
    *shown = Some(spawn_corner(&mut commands, &pen, qr));
    redrawn.write(Redrawn::Alert);
}

/// The corner: "Scan to join" over the QR, on a dimmed plate so it reads
/// over any part of the level.
fn spawn_corner(commands: &mut Commands, pen: &Pen, qr: Option<Handle<Image>>) -> Entity {
    commands
        .spawn((
            CornerJoin,
            Node {
                position_type: PositionType::Absolute,
                top: px(32),
                right: px(32),
                flex_direction: FlexDirection::Column,
                align_items: AlignItems::Center,
                row_gap: px(8),
                padding: UiRect::all(px(12)),
                border_radius: BorderRadius::all(px(16)),
                ..default()
            },
            BackgroundColor(pen.theme.shadow.with_alpha(0.6)),
            GlobalZIndex(CORNER_Z),
        ))
        .with_children(|corner| {
            corner.spawn(pen.text("Scan to join", Step::Caption, Role::Primary));
            if let Some(qr) = qr {
                corner.spawn((
                    CornerQr,
                    ImageNode::new(qr),
                    Node {
                        width: px(CORNER_QR),
                        height: px(CORNER_QR),
                        ..default()
                    },
                ));
            }
        })
        .id()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Each alert's two lines say what level-scene's requirements have it
    /// say: the toy can't be used and to take it off; too many of a kind
    /// and to take one off; player 1 asked for a Skylander.
    #[test]
    fn each_alert_says_what_is_wrong_and_what_to_do() {
        let [title, then] = lines(Alert::WrongToy);
        assert!(title.contains("can't be used with this game"), "{title}");
        assert!(then.contains("take it off"), "{then}");
        assert!(lines(Alert::TooManySkylanders)[0].contains("Too many Skylanders"));
        assert!(lines(Alert::TooManyItems)[0].contains("Too many magic items"));
        for alert in [Alert::TooManySkylanders, Alert::TooManyItems] {
            assert!(lines(alert)[1].contains("Take one off"), "{alert:?}");
        }
        let [call, _] = lines(Alert::PlaceSkylander);
        assert!(
            call.starts_with("Player 1") && call.contains("put a Skylander"),
            "{call}"
        );
    }

    /// The stack over the level: the pause overlays, then an alert over
    /// them, the small join code over the alert so another kid can still
    /// join, and an intro over all of it.
    #[test]
    fn an_alert_covers_the_pause_overlays_and_the_join_code_stays_on_top() {
        let stack = [
            crate::pause::OVERLAY_Z,
            ALERT_Z,
            CORNER_Z,
            crate::intro_player::OVERLAY_Z,
        ];
        assert!(stack.windows(2).all(|w| w[0] < w[1]), "{stack:?}");
    }
}
