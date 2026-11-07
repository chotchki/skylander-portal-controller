//! The words the scripts put up over a level, on the TV (openspec AS.131):
//! the enemy intro's card, its name in a band low on the screen where the
//! original's slides up, and the hints, each in a box at the top, read off
//! the run ([`guacamole::announcements`]) and laid out in the TV's own look
//! (stand-in ledger: `level-words`). The scripts own when each shows; a
//! portal alert takes the screen whole and they wait under it, as the held
//! band does.

use bevy::prelude::*;
use guacamole::announcements::{Line, enemy_card, hints};
use guacamole::scripts::{ScriptRuntime, ScriptWorld};
use serde::Serialize;

use crate::pad::LevelPads;
use crate::play::{Play, PlaySystems};
use crate::theme::{Role, Step};
use crate::tv_state::{Pen, TvAssets};

/// Over the level and the HUD's numbers, under everything that takes the
/// screen: the darkness (870), the pause overlay, the completion screen, the
/// portal's alert and the modals.
const ANNOUNCE_Z: i32 = 860;
/// The card's band, up from the screen's foot over the HUD's panel.
const CARD_BOTTOM: f32 = 220.0;
const HINT_TOP: f32 = 40.0;

/// Marks what's drawn, which the shots and the mirror find it by.
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Announcement;

/// What's drawn, its part of `/api/_test/tv`: each line's text, none where
/// the interface's model doesn't read it (nothing's drawn for those).
#[derive(Resource, Debug, Clone, Default, PartialEq, Serialize)]
pub struct AnnounceReport {
    pub card: Option<String>,
    pub hints: Vec<String>,
    /// How many cards and hints have come up since the run began.
    pub cards_shown: u32,
    pub hints_shown: u32,
}

pub(crate) struct AnnouncePlugin;

impl Plugin for AnnouncePlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<AnnounceReport>()
            .add_systems(Update, show.after(PlaySystems));
    }
}

/// The card and the hints the screen shows: nothing while the level doesn't
/// show or a portal alert covers it, and only what reads.
pub(crate) fn wanted(
    level_shows: bool,
    alert: bool,
    card: Option<Line>,
    hints: Vec<Line>,
) -> (Option<String>, Vec<String>) {
    if !level_shows || alert {
        return (None, Vec::new());
    }
    (
        card.and_then(|c| c.text),
        hints.into_iter().filter_map(|h| h.text).collect(),
    )
}

#[allow(clippy::too_many_arguments)]
fn show(
    mut commands: Commands,
    pads: Res<LevelPads>,
    play: Option<Res<Play>>,
    runtime: Option<Res<ScriptRuntime>>,
    world: Option<Res<ScriptWorld>>,
    pen: (Option<Res<TvAssets>>, Option<Res<crate::theme::TvTheme>>),
    mut report: ResMut<AnnounceReport>,
    mut shown: Local<Option<((Option<String>, Vec<String>), Vec<Entity>)>>,
) {
    let (card, lines) = match (runtime.as_deref(), world.as_deref()) {
        (Some(rt), Some(w)) => (enemy_card(&rt.vm, w), hints(&rt.vm, w)),
        _ => (None, Vec::new()),
    };
    let want = wanted(
        pads.shows(),
        play.as_deref().and_then(Play::alert).is_some(),
        card,
        lines,
    );
    let drawn = shown.as_ref().map(|(w, _)| w);
    if drawn == Some(&want) || (drawn.is_none() && want == (None, Vec::new())) {
        return;
    }
    let (before_card, before_hints) = drawn.cloned().unwrap_or_default();
    let mut next = report.clone();
    next.cards_shown += u32::from(want.0.is_some() && want.0 != before_card);
    next.hints_shown += want.1.iter().filter(|h| !before_hints.contains(h)).count() as u32;
    next.card = want.0.clone();
    next.hints = want.1.clone();
    *report = next;
    if let Some((_, entities)) = shown.take() {
        for e in entities {
            if let Ok(mut root) = commands.get_entity(e) {
                root.despawn();
            }
        }
    }
    let (Some(assets), Some(theme)) = pen else {
        return;
    };
    let pen = Pen::new(&assets, &theme);
    let mut entities = Vec::new();
    if let Some(card) = &want.0 {
        entities.push(spawn_card(&mut commands, &pen, card));
    }
    if !want.1.is_empty() {
        entities.push(spawn_hints(&mut commands, &pen, &want.1));
    }
    *shown = Some((want, entities));
}

/// The enemy's name, centred in a band low on the screen.
fn spawn_card(commands: &mut Commands, pen: &Pen, text: &str) -> Entity {
    commands
        .spawn((
            Announcement,
            Node {
                position_type: PositionType::Absolute,
                bottom: px(CARD_BOTTOM),
                left: percent(20),
                width: percent(60),
                justify_content: JustifyContent::Center,
                padding: UiRect::all(px(20)),
                border_radius: BorderRadius::all(px(20)),
                ..default()
            },
            BackgroundColor(pen.theme.shadow.with_alpha(0.75)),
            GlobalZIndex(ANNOUNCE_Z),
        ))
        .with_child(pen.text(text, Step::Lg, Role::Accent))
        .id()
}

/// The hints, one under another, in a box at the screen's top.
fn spawn_hints(commands: &mut Commands, pen: &Pen, hints: &[String]) -> Entity {
    commands
        .spawn((
            Announcement,
            Node {
                position_type: PositionType::Absolute,
                top: px(HINT_TOP),
                left: percent(25),
                width: percent(50),
                flex_direction: FlexDirection::Column,
                align_items: AlignItems::Center,
                row_gap: px(8),
                padding: UiRect::all(px(16)),
                border_radius: BorderRadius::all(px(16)),
                ..default()
            },
            BackgroundColor(pen.theme.shadow.with_alpha(0.7)),
            GlobalZIndex(ANNOUNCE_Z),
        ))
        .with_children(|b| {
            for hint in hints {
                b.spawn(pen.text(hint, Step::Body, Role::Primary));
            }
        })
        .id()
}

#[cfg(test)]
mod tests {
    use guac_script::Value;

    use super::*;

    fn line(text: Option<&str>) -> Line {
        Line {
            string: Value::Null,
            text: text.map(str::to_owned),
        }
    }

    /// openspec AS.131: the words sit over the HUD and under what takes the
    /// screen, so a hint left up never covers a pause, a completion screen
    /// or a modal.
    #[test]
    fn the_words_sit_under_what_takes_the_screen() {
        for under in [crate::hud::HUD_Z, crate::hud::DAMAGE_Z] {
            assert!(ANNOUNCE_Z > under, "{under}");
        }
        for over in [
            crate::pause::OVERLAY_Z,
            crate::completion::COMPLETION_Z,
            crate::alert::ALERT_Z,
            crate::yes_no::PROMPT_Z,
            crate::defeat::RESTART_Z,
            crate::arrival::ARRIVAL_Z,
        ] {
            assert!(ANNOUNCE_Z < over, "{over}");
        }
    }

    /// openspec AS.131: over the level the card and hints that read; a line
    /// the interface doesn't read draws nothing; none with the level hidden
    /// or a portal alert over it, until it goes.
    #[test]
    fn the_level_shows_what_reads_and_nothing_under_an_alert() {
        let (card, hints) = (
            Some(line(Some("name"))),
            vec![line(Some("hint")), line(None)],
        );
        assert_eq!(
            wanted(true, false, card.clone(), hints.clone()),
            (Some("name".to_owned()), vec!["hint".to_owned()])
        );
        assert_eq!(
            wanted(true, false, Some(line(None)), Vec::new()),
            (None, Vec::new())
        );
        assert_eq!(
            wanted(false, false, card.clone(), hints.clone()),
            (None, Vec::new())
        );
        assert_eq!(wanted(true, true, card, hints), (None, Vec::new()));
    }
}
