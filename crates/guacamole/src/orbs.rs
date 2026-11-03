//! The experience orbs on the TV (openspec AH.61, AH.350): each orb the
//! orbs' control holds (`guacamole::scripts::orbs`, AH.20's) kept as an
//! entity where its actor is, from its pop out of a beaten creature, down,
//! and flying in to the hero, until the control lets it go as its worth
//! reaches his figure.
//!
//! An orb is a clone of an emitter with no model (`global.bld`'s
//! `DeathXP_NewOrbOfficial_EvenSmaller` or `_Double`), and its look is that
//! emitter's: a 0.18 m orange halo and a 0.05 m white core, each one
//! particle of 0.3 s that AS.17's burst rule fills again while the orb's
//! out, as the original's stays lit ([`crate::emitters`]). AH.61's yellow
//! star card over it, drawn while the glow went out after its first burst,
//! is gone at chotchki's call (AH.350: "drop the star in favor of the
//! orbs").

use std::collections::HashMap;

use bevy::prelude::*;
use guac_script::Handle as ActorHandle;
use guacamole::scripts::{ScriptRuntime, ScriptWorld};

use crate::level::LevelSystems;

/// Each orb out, by its actor: an entity kept at its place while the
/// control holds it, despawned as it lets the orb go (`/api/_test/tv`'s
/// `orbs_drawn` counts them).
#[derive(Resource, Debug, Default)]
pub(crate) struct DrawnOrbs(pub(crate) HashMap<ActorHandle, Entity>);

pub(crate) struct OrbsPlugin;

impl Plugin for OrbsPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<DrawnOrbs>()
            .add_systems(Update, keep_orbs.after(LevelSystems));
    }
}

/// The orbs out as the scripts' world holds them, kept; none without a run.
fn keep_orbs(
    mut commands: Commands,
    runtime: Option<Res<ScriptRuntime>>,
    world: Option<Res<ScriptWorld>>,
    mut drawn: ResMut<DrawnOrbs>,
    mut roots: Query<&mut Transform>,
) {
    let out: Vec<(ActorHandle, Vec3)> = match (runtime.as_deref(), world.as_deref()) {
        (Some(rt), Some(w)) => guacamole::scripts::orbs(&rt.vm, w)
            .into_iter()
            .map(|o| (o.actor, o.at))
            .collect(),
        _ => Vec::new(),
    };
    place_orbs(&out, &mut commands, &mut drawn, &mut roots);
}

/// `out`'s orbs kept at their places: each new one an entity of its own,
/// each kept one moved, each no longer out let go.
pub(crate) fn place_orbs(
    out: &[(ActorHandle, Vec3)],
    commands: &mut Commands,
    drawn: &mut DrawnOrbs,
    roots: &mut Query<&mut Transform>,
) {
    drawn.0.retain(|h, root| {
        let keep = out.iter().any(|(o, _)| o == h);
        if !keep {
            commands.entity(*root).despawn();
        }
        keep
    });
    for &(h, at) in out {
        if let Some(&root) = drawn.0.get(&h) {
            if let Ok(mut t) = roots.get_mut(root)
                && t.translation != at
            {
                t.translation = at;
            }
            continue;
        }
        let root = commands
            .spawn((Name::new("orb"), Transform::from_translation(at)))
            .id();
        drawn.0.insert(h, root);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Resource, Default)]
    struct Out(Vec<(ActorHandle, Vec3)>);

    fn place_from_test(
        out: Res<Out>,
        mut commands: Commands,
        mut drawn: ResMut<DrawnOrbs>,
        mut roots: Query<&mut Transform>,
    ) {
        place_orbs(&out.0, &mut commands, &mut drawn, &mut roots);
    }

    fn app() -> App {
        let mut app = App::new();
        app.add_plugins(MinimalPlugins)
            .init_resource::<DrawnOrbs>()
            .init_resource::<Out>()
            .add_systems(Update, place_from_test);
        app
    }

    /// AH.61, AH.350: each orb out kept as an entity where its actor is,
    /// with nothing drawn on it (its look is its own emitter's), moved as
    /// it moves and let go once the control lets it go.
    #[test]
    fn each_orb_out_is_kept_where_its_actor_is_until_it_goes() {
        let mut app = app();
        let (a, b) = (11u64, 12u64);
        let (at_a, at_b) = (Vec3::new(1.0, 2.0, 3.0), Vec3::new(-4.0, 0.5, 6.0));
        app.world_mut().resource_mut::<Out>().0 = vec![(a, at_a), (b, at_b)];
        app.update();
        let drawn = app.world().resource::<DrawnOrbs>().0.clone();
        assert_eq!(drawn.len(), 2);
        for (h, at) in [(a, at_a), (b, at_b)] {
            let root = drawn[&h];
            assert_eq!(app.world().get::<Transform>(root).unwrap().translation, at);
            assert!(
                app.world().get::<Children>(root).is_none(),
                "no star card under it"
            );
            assert!(app.world().get::<Mesh3d>(root).is_none());
        }
        // One flies on, the other is collected.
        let flown = Vec3::new(0.0, 2.0, 2.0);
        app.world_mut().resource_mut::<Out>().0 = vec![(a, flown)];
        app.update();
        let now = app.world().resource::<DrawnOrbs>().0.clone();
        assert_eq!(now.len(), 1);
        assert_eq!(now[&a], drawn[&a], "the same entity");
        assert_eq!(
            app.world().get::<Transform>(now[&a]).unwrap().translation,
            flown
        );
        assert!(app.world().get_entity(drawn[&b]).is_err(), "let go");
        app.world_mut().resource_mut::<Out>().0.clear();
        app.update();
        assert!(app.world().resource::<DrawnOrbs>().0.is_empty());
        assert!(app.world().get_entity(drawn[&a]).is_err());
    }
}
