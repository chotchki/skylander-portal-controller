//! The darkness an interior's controller puts over the screen (openspec
//! AS.96, [`guacamole::interiors`]): while one is up, a black screen over
//! the level and its HUD at the opacity its mask leaves
//! ([`guacamole::interiors::darkness`]), under the pause and the portal's
//! alerts. Drawn only while the level has started; the scripts' slide of
//! the mask's alpha is all its timing, so it holds with `Time<Virtual>` as
//! they do. The mask's own art (a grey square stretched over the screen,
//! cutting the level's darkness sprite out of it) isn't drawn: the black's
//! the darkness sprite's colour as a hypothesis, no oracle frame catching
//! one mid-fade.

use bevy::prelude::*;
use guacamole::scripts::{ScriptRuntime, ScriptWorld};

use crate::play::{Play, PlaySystems};

/// Over the HUD and its damage numbers (800, 850), under the pause (900)
/// and the alerts (940 up).
const DARKNESS_Z: i32 = 870;

/// Reads the darkness off the run after the level's step, then draws it.
pub(crate) struct DarknessPlugin;

impl Plugin for DarknessPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Darkness>().add_systems(
            Update,
            (read_darkness, draw_darkness).chain().after(PlaySystems),
        );
    }
}

/// How dark the screen is, 0 clear to 1 black, as the TV draws it this
/// frame: none outside a level.
#[derive(Resource, Debug, Clone, Copy, Default, PartialEq)]
pub(crate) struct Darkness(pub f32);

/// The one node [`draw_darkness`] keeps and recolours, spawned as a darkness
/// comes up and despawned once it lifts.
#[derive(Component)]
struct DarknessScreen;

fn read_darkness(
    play: Option<Res<Play>>,
    runtime: Option<Res<ScriptRuntime>>,
    world: Option<Res<ScriptWorld>>,
    mut darkness: ResMut<Darkness>,
) {
    let now = match (play.as_deref(), runtime.as_deref(), world.as_deref()) {
        (Some(p), Some(rt), Some(w)) if p.started() => guacamole::interiors::darkness(&rt.vm, w),
        _ => 0.0,
    };
    if darkness.0 != now {
        darkness.0 = now;
    }
}

fn draw_darkness(
    mut commands: Commands,
    darkness: Res<Darkness>,
    mut screens: Query<(Entity, &mut BackgroundColor), With<DarknessScreen>>,
) {
    let alpha = darkness.0.clamp(0.0, 1.0);
    if alpha <= 0.0 {
        for (e, _) in &screens {
            commands.entity(e).despawn();
        }
        return;
    }
    let colour = Color::srgba(0.0, 0.0, 0.0, alpha);
    if let Some((_, mut bg)) = screens.iter_mut().next() {
        if bg.0 != colour {
            bg.0 = colour;
        }
        return;
    }
    commands.spawn((
        DarknessScreen,
        Node {
            position_type: PositionType::Absolute,
            left: px(0),
            top: px(0),
            width: percent(100),
            height: percent(100),
            ..default()
        },
        BackgroundColor(colour),
        GlobalZIndex(DARKNESS_Z),
        Pickable::IGNORE,
    ));
}

#[cfg(test)]
mod tests {
    use super::*;

    fn app(dark: f32) -> App {
        let mut app = App::new();
        app.insert_resource(Darkness(dark));
        app.add_systems(Update, draw_darkness);
        app
    }

    fn screens(app: &mut App) -> Vec<Color> {
        let mut q = app
            .world_mut()
            .query_filtered::<&BackgroundColor, With<DarknessScreen>>();
        q.iter(app.world()).map(|b| b.0).collect()
    }

    /// openspec AS.96: a darkness up draws one black screen at its opacity,
    /// follows it as it changes and goes once it's lifted.
    #[test]
    fn the_darkness_draws_one_black_screen_at_its_opacity_and_goes_once_lifted() {
        let mut app = app(0.0);
        app.update();
        assert!(screens(&mut app).is_empty(), "nothing while clear");
        app.world_mut().resource_mut::<Darkness>().0 = 0.4;
        app.update();
        app.update();
        assert_eq!(screens(&mut app), vec![Color::srgba(0.0, 0.0, 0.0, 0.4)]);
        app.world_mut().resource_mut::<Darkness>().0 = 1.0;
        app.update();
        assert_eq!(screens(&mut app), vec![Color::srgba(0.0, 0.0, 0.0, 1.0)]);
        app.world_mut().resource_mut::<Darkness>().0 = 0.0;
        app.update();
        assert!(screens(&mut app).is_empty(), "lifted");
    }

    /// No level started, no darkness, whatever the run holds.
    #[test]
    fn no_level_no_darkness() {
        let mut app = App::new();
        app.insert_resource(Darkness(0.7));
        app.add_systems(Update, read_darkness);
        app.update();
        assert_eq!(app.world().resource::<Darkness>().0, 0.0);
    }
}
