//! The experience orbs on the TV (openspec AH.61): each orb the orbs'
//! control holds (`guacamole::scripts::orbs`, AH.20's) drawn where its actor
//! is, from its pop out of a beaten creature, down, and flying in to the
//! hero, until the control lets it go as its worth reaches his figure.
//!
//! An orb is an actor with no model: the original draws it with a particle
//! emitter, which nothing here plays (the backlog's Particles line). So each
//! is drawn on a stand-in (stand-in ledger: `effects`): `global.bld`'s death
//! star ([`ORB_LOOK`], read since AH.39), a glowing star card, small and
//! turned to face the level's camera. Its keys are the spawn models', which
//! the census holds, so drawing one compiles nothing.

use std::collections::HashMap;

use bevy::camera::visibility::RenderLayers;
use bevy::prelude::*;
use guac_script::Handle as ActorHandle;
use guacamole::character::CharacterMaterial;
use guacamole::scripts::{ScriptRuntime, ScriptWorld};

use crate::level::{LEVEL_LAYER, Level, LevelCamera, LevelSystems};

/// The spawn model an orb is drawn on: `global.bld`'s death star, a star
/// card 1.56 m across.
pub(crate) const ORB_LOOK: &str = "C:/tfb/Content/Models/Objects/fx_objects/death_starz.mdl";

/// Its scale on an orb: about 0.4 m across.
pub(crate) const ORB_SCALE: f32 = 0.25;

/// Each orb drawn, by its actor: its root, at the orb's place.
#[derive(Resource, Debug, Default)]
pub(crate) struct DrawnOrbs(pub(crate) HashMap<ActorHandle, Entity>);

/// An orb's card under its root, turned to face the level's camera.
#[derive(Component, Debug)]
pub(crate) struct OrbFace;

/// The parts an orb draws with: the look's meshes on their materials.
type Look = [(Handle<Mesh>, Handle<CharacterMaterial>)];

pub(crate) struct OrbsPlugin;

impl Plugin for OrbsPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<DrawnOrbs>()
            .add_systems(Update, (draw_orbs, face_orbs).chain().after(LevelSystems));
    }
}

/// The orbs out as the scripts' world holds them, drawn; none without a run.
fn draw_orbs(
    mut commands: Commands,
    runtime: Option<Res<ScriptRuntime>>,
    world: Option<Res<ScriptWorld>>,
    level: Res<Level>,
    mut drawn: ResMut<DrawnOrbs>,
    mut roots: Query<&mut Transform, Without<OrbFace>>,
) {
    let out: Vec<(ActorHandle, Vec3)> = match (runtime.as_deref(), world.as_deref()) {
        (Some(rt), Some(w)) => guacamole::scripts::orbs(&rt.vm, w)
            .into_iter()
            .map(|o| (o.actor, o.at))
            .collect(),
        _ => Vec::new(),
    };
    let look = level.spawn_models().and_then(|m| m.get(ORB_LOOK));
    place_orbs(
        &out,
        look.map(Vec::as_slice),
        &mut commands,
        &mut drawn,
        &mut roots,
    );
}

/// `out`'s orbs drawn on `look` at their places: each new one spawned, each
/// drawn one moved, each no longer out let go. Without the look (a spawn
/// part that lacks it) none is drawn.
pub(crate) fn place_orbs(
    out: &[(ActorHandle, Vec3)],
    look: Option<&Look>,
    commands: &mut Commands,
    drawn: &mut DrawnOrbs,
    roots: &mut Query<&mut Transform, Without<OrbFace>>,
) {
    drawn.0.retain(|h, root| {
        let keep = out.iter().any(|(o, _)| o == h);
        if !keep {
            commands.entity(*root).despawn();
        }
        keep
    });
    let Some(look) = look else {
        return;
    };
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
            .spawn((
                Name::new("orb"),
                Transform::from_translation(at),
                Visibility::default(),
            ))
            .id();
        let face = commands
            .spawn((
                OrbFace,
                Transform::from_scale(Vec3::splat(ORB_SCALE)),
                Visibility::default(),
                ChildOf(root),
            ))
            .id();
        for (mesh, material) in look {
            commands.spawn((
                Mesh3d(mesh.clone()),
                MeshMaterial3d(material.clone()),
                Transform::IDENTITY,
                RenderLayers::layer(LEVEL_LAYER),
                ChildOf(face),
            ));
        }
        drawn.0.insert(h, root);
    }
}

/// Each orb's card turned so its face (the model's +Z) looks at the level's
/// camera, upright.
fn face_orbs(
    cameras: Query<&GlobalTransform, With<LevelCamera>>,
    roots: Query<&GlobalTransform, Without<OrbFace>>,
    mut faces: Query<(&ChildOf, &mut Transform), With<OrbFace>>,
) {
    let Some(camera) = cameras.iter().next().map(GlobalTransform::translation) else {
        return;
    };
    for (parent, mut t) in &mut faces {
        let Ok(root) = roots.get(parent.parent()) else {
            continue;
        };
        let to = camera - root.translation();
        if to.length_squared() < 1e-8 {
            continue;
        }
        let rotation = facing(to);
        if t.rotation != rotation {
            t.rotation = rotation;
        }
    }
}

/// The turn that points a card's +Z along `to`, its up kept toward +Y.
pub(crate) fn facing(to: Vec3) -> Quat {
    Transform::IDENTITY.looking_to(-to, Vec3::Y).rotation
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Resource, Default)]
    struct Out(Vec<(ActorHandle, Vec3)>);

    #[derive(Resource, Default)]
    struct TestLook(Option<Vec<(Handle<Mesh>, Handle<CharacterMaterial>)>>);

    fn place_from_test(
        out: Res<Out>,
        look: Res<TestLook>,
        mut commands: Commands,
        mut drawn: ResMut<DrawnOrbs>,
        mut roots: Query<&mut Transform, Without<OrbFace>>,
    ) {
        place_orbs(
            &out.0,
            look.0.as_deref(),
            &mut commands,
            &mut drawn,
            &mut roots,
        );
    }

    fn app() -> App {
        let mut app = App::new();
        app.add_plugins(MinimalPlugins)
            .init_resource::<DrawnOrbs>()
            .init_resource::<Out>()
            .init_resource::<TestLook>()
            .init_resource::<Assets<Mesh>>()
            .add_systems(Update, place_from_test);
        app
    }

    /// The parts under an orb's root: its card, then the look's meshes.
    fn parts(app: &App, root: Entity) -> (Entity, Vec<Entity>) {
        let face = app.world().get::<Children>(root).unwrap()[0];
        let parts = app.world().get::<Children>(face).unwrap().to_vec();
        (face, parts)
    }

    /// AH.61: each orb out drawn on the look where its actor is (its card
    /// small, its parts in the level's layer on the look's materials), moved
    /// as it moves and let go once the control lets it go; with no look in
    /// the spawn part nothing is drawn, and the orbs out are drawn once it
    /// comes.
    #[test]
    fn each_orb_out_is_drawn_where_its_actor_is_until_it_goes() {
        let mut app = app();
        let (a, b) = (11u64, 12u64);
        let (at_a, at_b) = (Vec3::new(1.0, 2.0, 3.0), Vec3::new(-4.0, 0.5, 6.0));
        app.world_mut().resource_mut::<Out>().0 = vec![(a, at_a), (b, at_b)];
        app.update();
        assert!(app.world().resource::<DrawnOrbs>().0.is_empty(), "no look");
        let mesh = app
            .world_mut()
            .resource_mut::<Assets<Mesh>>()
            .add(Mesh::from(Cuboid::default()));
        let material = Handle::<CharacterMaterial>::default();
        app.world_mut().resource_mut::<TestLook>().0 = Some(vec![(mesh.clone(), material.clone())]);
        app.update();
        let drawn = app.world().resource::<DrawnOrbs>().0.clone();
        assert_eq!(drawn.len(), 2);
        for (h, at) in [(a, at_a), (b, at_b)] {
            let root = drawn[&h];
            assert_eq!(app.world().get::<Transform>(root).unwrap().translation, at);
            let (face, parts) = parts(&app, root);
            assert!(app.world().get::<OrbFace>(face).is_some());
            assert_eq!(
                app.world().get::<Transform>(face).unwrap().scale,
                Vec3::splat(ORB_SCALE)
            );
            assert_eq!(parts.len(), 1);
            assert_eq!(app.world().get::<Mesh3d>(parts[0]).unwrap().0, mesh);
            assert_eq!(
                app.world()
                    .get::<MeshMaterial3d<CharacterMaterial>>(parts[0])
                    .unwrap()
                    .0,
                material
            );
            assert_eq!(
                app.world().get::<RenderLayers>(parts[0]),
                Some(&RenderLayers::layer(LEVEL_LAYER))
            );
        }
        // One flies on, the other is collected.
        let flown = Vec3::new(0.0, 2.0, 2.0);
        app.world_mut().resource_mut::<Out>().0 = vec![(a, flown)];
        app.update();
        let now = app.world().resource::<DrawnOrbs>().0.clone();
        assert_eq!(now.len(), 1);
        assert_eq!(now[&a], drawn[&a], "the same root");
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

    /// The card's face looks at the camera from any side, and it doesn't
    /// roll: a star card tipped sideways reads as a smear, not an orb.
    #[test]
    fn an_orbs_card_faces_the_camera() {
        for to in [
            Vec3::new(0.0, 0.0, 5.0),
            Vec3::new(3.0, 1.0, -4.0),
            Vec3::new(-2.0, 6.0, 1.0),
        ] {
            let q = facing(to);
            assert!((q * Vec3::Z).distance(to.normalize()) < 1e-5, "{to}");
            assert!((q * Vec3::X).y.abs() < 1e-5, "{to}");
        }
    }
}
