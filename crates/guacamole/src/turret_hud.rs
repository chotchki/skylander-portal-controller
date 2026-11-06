//! A turret's pointer over the level (openspec AS.76): the turret's game
//! runs on the VM ([`guacamole::turret`]), whose pointer and calibration
//! circle are sprites the interface doesn't draw on the TV yet, so a kid
//! couldn't see where the stick has put them. This draws both in the TV's
//! own style, each where the scripts hold it, with the circle's line under
//! it (stand-in ledger: `turret-pick`).

use bevy::prelude::*;
use guacamole::scripts::{ScriptRuntime, ScriptWorld};
use guacamole::turret::{self, TurretView};

use crate::pad::LevelPads;
use crate::play::PlaySystems;
use crate::theme::{Role, Step, TvTheme};
use crate::tv_state::{Pen, TvAssets};

/// Over the level and its HUD, under the join corner's (960).
const TURRET_Z: i32 = 950;
/// The pointer's ring, the calibration circle's and their rims, TV pixels.
const POINTER: f32 = 56.0;
const CIRCLE: f32 = 120.0;
const RIM: f32 = 6.0;
/// Interface units across the screen's half: `screen location` reads a
/// pointer's offset at 1.5 screen pixels a unit on the original's 1280 by
/// 720, whose share of the picture is what the TV places by.
const UNITS: [f32; 2] = [1280.0 / 1.5, 720.0 / 1.5];

/// Marks the overlay's root.
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct TurretHud;

/// The ring's left and top as shares of the picture, percent.
pub(crate) fn place(at: [f32; 2]) -> [f32; 2] {
    [
        (0.5 + at[0] / UNITS[0]) * 100.0,
        (0.5 + at[1] / UNITS[1]) * 100.0,
    ]
}

/// The turret's view as the TV last read it, its part of `/api/_test/tv`.
#[derive(Resource, Debug, Clone, Default, PartialEq)]
pub struct TurretReport(pub Option<TurretView>);

pub(crate) struct TurretHudPlugin;

impl Plugin for TurretHudPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<TurretReport>()
            .add_systems(Update, show_turret.after(PlaySystems));
    }
}

fn ring(size: f32, at: [f32; 2], color: Color) -> impl Bundle {
    let [left, top] = place(at);
    (
        Node {
            position_type: PositionType::Absolute,
            left: percent(left),
            top: percent(top),
            width: px(size),
            height: px(size),
            margin: UiRect {
                left: px(-size / 2.0),
                top: px(-size / 2.0),
                ..default()
            },
            border: UiRect::all(px(RIM)),
            border_radius: BorderRadius::all(px(size / 2.0)),
            ..default()
        },
        BorderColor::all(color),
    )
}

#[allow(clippy::too_many_arguments)]
fn show_turret(
    mut commands: Commands,
    pads: Res<LevelPads>,
    runtime: Option<Res<ScriptRuntime>>,
    world: Option<Res<ScriptWorld>>,
    (assets, theme): (Option<Res<TvAssets>>, Option<Res<TvTheme>>),
    mut report: ResMut<TurretReport>,
    mut shown: Local<Option<(TurretView, Entity)>>,
) {
    let view = match (runtime.as_deref(), world.as_deref()) {
        (Some(r), Some(w)) if pads.shows() => turret::view(&r.vm, w),
        _ => None,
    };
    if report.0 != view {
        report.0 = view.clone();
    }
    if shown.as_ref().map(|(v, _)| v) == view.as_ref() {
        return;
    }
    if let Some((_, entity)) = shown.take()
        && let Ok(mut root) = commands.get_entity(entity)
    {
        root.despawn();
    }
    let (Some(view), Some(assets), Some(theme)) = (view, assets, theme) else {
        return;
    };
    if view.pointer.is_none() {
        return;
    }
    let pen = Pen::new(&assets, &theme);
    let entity = commands
        .spawn((
            TurretHud,
            Node {
                position_type: PositionType::Absolute,
                left: px(0),
                top: px(0),
                width: percent(100),
                height: percent(100),
                ..default()
            },
            GlobalZIndex(TURRET_Z),
        ))
        .with_children(|b| {
            if let Some(at) = view.calibration {
                b.spawn(ring(CIRCLE, at, pen.theme.shadow));
                if let Some(line) = &view.message {
                    let [left, top] = place([at[0], at[1] + 70.0]);
                    b.spawn((
                        Node {
                            position_type: PositionType::Absolute,
                            left: percent(left - 18.0),
                            top: percent(top),
                            ..default()
                        },
                        pen.text(line, Step::Body, Role::Accent),
                    ));
                }
            }
            if let Some(at) = view.pointer {
                b.spawn(ring(POINTER, at, pen.theme.gold));
            }
        })
        .id();
    *shown = Some((view, entity));
}

#[cfg(test)]
mod tests {
    use super::*;

    /// AS.76: the middle of the screen is the picture's middle, and a
    /// pointer's offset lands where the scripts' own 1.5 pixels a unit put
    /// it on the original's 1280 by 720.
    #[test]
    fn a_pointer_lands_where_its_offset_puts_it() {
        assert_eq!(place([0.0, 0.0]), [50.0, 50.0]);
        let [left, top] = place([-100.0, 60.0]);
        assert!((left / 100.0 * 1280.0 - (640.0 - 150.0)).abs() < 1e-3);
        assert!((top / 100.0 * 720.0 - (360.0 + 90.0)).abs() < 1e-3);
    }
}
