//! The header over a held level (openspec AS.88): while the level end's
//! movie converts on its first play, or the next level is read, the TV
//! holds the level's last frame, and the header's band saying why (`Getting
//! FMV02 ready`, `Getting level_214_tutotial_2 ready` and its bar) is drawn
//! on the portal view the level covers. This draws the same band over the
//! held level, top right under the join corner's card, where the TV's own
//! words over a level already sit; the portal view's header is unchanged.

use bevy::prelude::*;
use serde::Serialize;

use crate::extraction::{Band, ExtractBand};
use crate::level_choice::LevelChoice;
use crate::movies::Movies;
use crate::pad::LevelPads;
use crate::play::PlaySystems;
use crate::theme::{Role, Step, TvTheme};
use crate::tv_state::{Pen, TvAssets};

/// The join corner's (960): over the level, its alerts and its modals.
const HELD_BAND_Z: i32 = 960;
/// Under the join corner's card (32 down, its caption and 176 of QR).
const TOP: f32 = 300.0;
const WIDTH: f32 = 460.0;

/// Marks the band's root, which the shots and the mirror find it by.
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct HeldBand;

/// The band over the held level as drawn, its part of `/api/_test/tv`.
#[derive(Resource, Debug, Clone, Default, PartialEq, Serialize)]
pub struct HeldBandReport {
    pub title: Option<String>,
    pub detail: Option<String>,
    pub percent: Option<u8>,
}

pub(crate) struct HeldBandPlugin;

impl Plugin for HeldBandPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<HeldBandReport>()
            .add_systems(Update, show_held_band.after(PlaySystems));
    }
}

/// The band the held level shows: the movie's while it converts, else the
/// header's while the next level is read; none while the level doesn't
/// show.
pub(crate) fn wanted(
    level_shows: bool,
    movie: Option<Band>,
    changing: bool,
    header: Option<&Band>,
) -> Option<Band> {
    if !level_shows {
        return None;
    }
    movie.or_else(|| changing.then(|| header.cloned()).flatten())
}

#[allow(clippy::too_many_arguments)]
fn show_held_band(
    mut commands: Commands,
    pads: Res<LevelPads>,
    movies: Option<Res<Movies>>,
    choice: Option<Res<LevelChoice>>,
    header: Option<Res<ExtractBand>>,
    (assets, theme): (Option<Res<TvAssets>>, Option<Res<TvTheme>>),
    mut report: ResMut<HeldBandReport>,
    mut shown: Local<Option<(Band, Entity)>>,
) {
    let want = wanted(
        pads.shows(),
        movies.as_deref().and_then(Movies::band),
        choice.as_deref().is_some_and(LevelChoice::changing),
        header.as_deref().and_then(|h| h.0.as_ref()),
    );
    if shown.as_ref().map(|(b, _)| b) == want.as_ref() {
        return;
    }
    if let Some((_, entity)) = shown.take()
        && let Ok(mut root) = commands.get_entity(entity)
    {
        root.despawn();
    }
    let next = HeldBandReport {
        title: want.as_ref().map(|b| b.title.clone()),
        detail: want.as_ref().map(|b| b.detail.clone()),
        percent: want.as_ref().and_then(|b| b.percent),
    };
    if *report != next {
        *report = next;
    }
    let (Some(band), Some(assets), Some(theme)) = (want, assets, theme) else {
        return;
    };
    let pen = Pen::new(&assets, &theme);
    let entity = spawn(&mut commands, &pen, &band);
    *shown = Some((band, entity));
}

/// The band as the header draws it, a bar only for a read with a share
/// done (a level's fetch; a movie's conversion has none).
fn spawn(commands: &mut Commands, pen: &Pen, band: &Band) -> Entity {
    let theme = &pen.theme;
    commands
        .spawn((
            HeldBand,
            Node {
                position_type: PositionType::Absolute,
                top: px(TOP),
                right: px(32),
                width: px(WIDTH),
                flex_direction: FlexDirection::Column,
                row_gap: px(8),
                padding: UiRect::all(px(16)),
                border_radius: BorderRadius::all(px(16)),
                ..default()
            },
            BackgroundColor(theme.shadow.with_alpha(0.7)),
            GlobalZIndex(HELD_BAND_Z),
        ))
        .with_children(|b| {
            b.spawn(pen.text(&band.title, Step::Body, Role::Accent));
            if !band.detail.is_empty() {
                b.spawn(pen.text(&band.detail, Step::Caption, Role::Primary));
            }
            if let Some(p) = band.percent {
                b.spawn((
                    Node {
                        width: percent(100),
                        height: px(12),
                        border_radius: BorderRadius::all(px(6)),
                        ..default()
                    },
                    BackgroundColor(theme.panel[0]),
                ))
                .with_child((
                    Node {
                        width: percent(f32::from(p.min(100))),
                        height: percent(100),
                        border_radius: BorderRadius::all(px(6)),
                        ..default()
                    },
                    BackgroundColor(theme.gold),
                ));
            }
        })
        .id()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn band(title: &str, percent: Option<u8>) -> Band {
        Band {
            title: title.into(),
            detail: "detail".into(),
            percent,
            code: None,
        }
    }

    /// AS.88: over the level, the movie's band while it converts, else the
    /// header's while the next level is read; neither held, none; the level
    /// not showing (the portal view draws its own header), none.
    #[test]
    fn the_held_level_shows_the_movies_band_or_the_next_levels() {
        let movie = band("Getting FMV02 ready", None);
        let level = band("Getting level_214_tutotial_2 ready", Some(40));
        assert_eq!(
            wanted(true, Some(movie.clone()), true, Some(&level)),
            Some(movie.clone())
        );
        assert_eq!(wanted(true, None, true, Some(&level)), Some(level.clone()));
        assert_eq!(wanted(true, None, false, Some(&level)), None, "no read");
        assert_eq!(wanted(true, None, true, None), None);
        assert_eq!(wanted(false, Some(movie), true, Some(&level)), None);
    }
}
