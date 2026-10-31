//! The scripts' emitters drawn (openspec AH.87): every clone of an emitter
//! the extract's particle part holds gets a particle system under the
//! level's root, its generators stepped each drawn frame on the TV's virtual
//! clock (so a pause or a hold stops them) and laid out as quads facing the
//! level's camera, one mesh a generator on that generator's material
//! ([`guacamole::particles`] has what they do and how they draw).
//!
//! A system follows its clone (the trail rides the ball the scripts hang it
//! on) while the clone's in play; once the scripts take it off, the
//! system stops (making its last few, the library's rule) and goes when
//! nothing of it is left to draw. It hangs under the level's root, not the
//! clone, so a clone taken off (whose parts go with it) leaves its
//! particles to live out their lives, and a level leaving takes them all.
//!
//! The particles draw their own random numbers ([`ParticleRng`]): nothing
//! here reaches the session's generator, so they move nothing that plays.

use std::sync::Arc;

use bevy::camera::visibility::{NoFrustumCulling, RenderLayers};
use bevy::prelude::*;
use guac_ir::particle::Emitter;
use guacamole::character::static_mesh;
use guacamole::particles::{self, Cosmetic, Frame as Place, GeneratorState, View};
use guacamole::scripts::{ScriptClone, ScriptWorld};
use serde::Serialize;

use crate::level::{LEVEL_LAYER, Level, LevelOutput};

/// Where every TV's particles start: fixed, so a run's input draws the
/// same particles each time.
pub(crate) const SEED: u64 = 0x0041_482E_3837;

/// The particles' random numbers, apart from the session's generator so a
/// particle's draw never moves a script's roll.
#[derive(Resource, Debug)]
pub(crate) struct ParticleRng(pub(crate) Cosmetic);

impl Default for ParticleRng {
    fn default() -> Self {
        Self(Cosmetic::new(SEED))
    }
}

/// A clone [`start_systems`] has looked at.
#[derive(Component, Debug)]
pub(crate) struct Looked;

/// One emitter clone's particles.
#[derive(Component, Debug)]
pub(crate) struct ParticleSystem {
    /// The clone whose emitter it is, by entity and handle.
    clone: Entity,
    name: String,
    emitter: Arc<Emitter>,
    generators: Vec<GeneratorState>,
    /// Each generator's mesh entity and mesh; `None` for one that draws
    /// nothing.
    meshes: Vec<Option<(Entity, Handle<Mesh>)>>,
    /// Where the emitter stood last.
    place: Place,
    on: bool,
}

/// What `/api/_test/tv` reports of the particles.
#[derive(Resource, Debug, Clone, Default, PartialEq, Serialize)]
pub(crate) struct ParticleReport {
    /// Systems drawing now, and every one started since the TV came up.
    pub systems: usize,
    pub started: usize,
    /// Particles alive, and quads drawn this frame.
    pub particles: usize,
    pub quads: usize,
    /// The emitters drawing now, by name, a name once.
    pub emitters: Vec<String>,
    /// Every emitter a system started for, by name, a name once, in the
    /// order first met.
    pub seen: Vec<String>,
}

pub(crate) struct EmittersPlugin;

impl Plugin for EmittersPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<ParticleRng>()
            .init_resource::<ParticleReport>()
            .add_systems(
                PostUpdate,
                (start_systems, step_systems)
                    .chain()
                    .after(TransformSystems::Propagate)
                    .run_if(resource_exists::<ScriptWorld>),
            );
    }
}

/// Where a clone's emitter stands: its entity's place and turn as drawn
/// (`drawn`, eased between steps), else its actor's, and its actor's scale.
fn place_of(entity: Entity, world: &ScriptWorld, drawn: Option<&GlobalTransform>) -> Option<Place> {
    let actor = world.actor(entity.to_bits())?;
    let scale = actor.scale[0] / 100.0;
    Some(match drawn {
        Some(g) => {
            let (_, turn, at) = g.to_scale_rotation_translation();
            Place { at, turn, scale }
        }
        None => {
            let t = crate::hero_scripts::clone_transform(actor);
            Place {
                at: t.translation,
                turn: t.rotation,
                scale,
            }
        }
    })
}

/// Each clone of an emitter the particle part holds gets its system, once
/// the level's loaded: a system entity under the level's root and a hidden
/// mesh entity a drawn generator, on the level's layer.
pub(crate) fn start_systems(
    mut commands: Commands,
    world: Res<ScriptWorld>,
    level: Res<Level>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut report: ResMut<ParticleReport>,
    clones: Query<Entity, (With<ScriptClone>, Without<Looked>)>,
    transforms: Query<&GlobalTransform, With<ScriptClone>>,
) {
    let Some((emitters, root)) = level.emitters() else {
        return;
    };
    for entity in &clones {
        commands.entity(entity).insert(Looked);
        let Some(draw) = world
            .emitter_of(entity.to_bits())
            .and_then(|name| emitters.get(name))
        else {
            continue;
        };
        let Some(place) = place_of(entity, &world, transforms.get(entity).ok()) else {
            continue;
        };
        let system = commands
            .spawn((
                Name::new(format!("particles {}", draw.emitter.name)),
                Transform::from_translation(place.at),
                Visibility::Inherited,
                ChildOf(root),
            ))
            .id();
        let mesh_entities = draw
            .materials
            .iter()
            .map(|m| {
                let material = m.clone()?;
                let mesh = meshes.add(placeholder());
                let e = commands
                    .spawn((
                        Mesh3d(mesh.clone()),
                        MeshMaterial3d(material),
                        Transform::IDENTITY,
                        Visibility::Hidden,
                        RenderLayers::layer(LEVEL_LAYER),
                        NoFrustumCulling,
                        ChildOf(system),
                    ))
                    .id();
                Some((e, mesh))
            })
            .collect();
        commands.entity(system).insert(ParticleSystem {
            clone: entity,
            name: draw.emitter.name.clone(),
            emitter: draw.emitter.clone(),
            generators: vec![GeneratorState::default(); draw.emitter.generators.len()],
            meshes: mesh_entities,
            place,
            on: true,
        });
        report.started += 1;
        if !report.seen.contains(&draw.emitter.name) {
            report.seen.push(draw.emitter.name.clone());
        }
    }
}

/// What a generator's mesh holds before it first draws: one triangle of its
/// shape, never shown.
fn placeholder() -> Mesh {
    static_mesh(&particles::shape(0)).expect("the shape builds")
}

/// The view the particles face: the level camera drawing now.
fn view_of<'a>(
    mut cameras: impl Iterator<Item = (&'a Camera, &'a GlobalTransform)>,
) -> Option<View> {
    let (_, g) = cameras.find(|(c, _)| c.is_active)?;
    Some(View {
        right: g.right().as_vec3(),
        up: g.up().as_vec3(),
        eye: g.translation(),
    })
}

/// Each drawn frame: every system's generators step by the virtual clock's
/// share, at its clone's place while it's in play; each generator's mesh
/// takes what it draws now (shown, or hidden with nothing); a system whose
/// clone has gone and that has nothing left goes.
///
/// It runs after the transforms propagate, so a mesh stands this frame
/// where the last frame's system put it (and on a system's first frame at
/// the world's origin): its quads are laid from where it stands, not from
/// the emitter's place now, or a moving emitter's would draw a frame's move
/// behind its particles.
#[allow(clippy::too_many_arguments)]
pub(crate) fn step_systems(
    mut commands: Commands,
    time: Res<Time>,
    world: Res<ScriptWorld>,
    mut rng: ResMut<ParticleRng>,
    mut report: ResMut<ParticleReport>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut systems: Query<(Entity, &mut ParticleSystem, &mut Transform)>,
    mut drawn: Query<(&mut Visibility, &GlobalTransform), Without<ParticleSystem>>,
    transforms: Query<&GlobalTransform, (With<ScriptClone>, Without<ParticleSystem>)>,
    cameras: Query<(&Camera, &GlobalTransform), (With<LevelOutput>, Without<ParticleSystem>)>,
) {
    let dt = time.delta_secs();
    let view = view_of(cameras.iter());
    let mut now = ParticleReport {
        started: report.started,
        seen: std::mem::take(&mut report.seen),
        ..default()
    };
    for (entity, mut system, mut transform) in &mut systems {
        let system = &mut *system;
        let in_play = world
            .actor(system.clone.to_bits())
            .is_some_and(|a| !a.removed);
        if in_play
            && let Some(place) = place_of(system.clone, &world, transforms.get(system.clone).ok())
        {
            system.place = place;
        }
        system.on = in_play;
        for (state, g) in system.generators.iter_mut().zip(&system.emitter.generators) {
            state.step(g, system.place, system.on, dt, &mut rng.0);
        }
        if !system.on && system.generators.iter().all(|g| !g.live()) {
            commands.entity(entity).despawn();
            continue;
        }
        // The system stands where its emitter does, so from the next
        // frame's propagation its blended draws sort by it.
        transform.translation = system.place.at;
        now.systems += 1;
        if !now.emitters.contains(&system.name) {
            now.emitters.push(system.name.clone());
        }
        for ((state, g), mesh) in system
            .generators
            .iter()
            .zip(&system.emitter.generators)
            .zip(&system.meshes)
        {
            now.particles += state.particles.len();
            let Some((mesh_entity, handle)) = mesh else {
                continue;
            };
            let Ok((mut v, placed)) = drawn.get_mut(*mesh_entity) else {
                continue;
            };
            // The level's root and the system carry no turn or scale.
            let from = placed.translation();
            let quads = view.and_then(|v| particles::segment(state, g, system.place, v, from, 0));
            let shown = match quads.as_ref().map(static_mesh) {
                Some(Ok(m)) => {
                    now.quads += m.count_vertices() / 4;
                    let _ = meshes.insert(handle.id(), m);
                    true
                }
                _ => false,
            };
            let want = if shown {
                Visibility::Inherited
            } else {
                Visibility::Hidden
            };
            if *v != want {
                *v = want;
            }
        }
    }
    *report = now;
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use bevy::time::TimeUpdateStrategy;
    use guac_script::{Obj, Value};
    use guacamole::level_scripts;
    use guacamole::scripts::{Placed, ScriptPlacement, ScriptRuntime, install};

    use super::*;
    use crate::level::level_channel;
    use crate::test_support::hero_scripts::write_attack;
    use crate::test_support::level::{
        LEVEL, START, lights, trail, write_level, write_particles_of,
    };
    use crate::test_support::turntable::{app, extract};

    /// The emitter the run takes as a placement (and the test, as a clone).
    const TRAIL: Obj = Obj { unit: 99, index: 1 };

    /// A level loaded with its particle part, the attack's script part
    /// installed with [`TRAIL`] among its placements answering for an
    /// entity marked a clone at the level's start, a level camera looking
    /// at it, the clock a 60th of a second a frame.
    fn loaded() -> (App, Entity, Vec<Placed>, tempfile::TempDir) {
        loaded_with(trail())
    }

    /// [`loaded`] with `emitter` (named `Trail`) in the particle part.
    fn loaded_with(emitter: Emitter) -> (App, Entity, Vec<Placed>, tempfile::TempDir) {
        let tmp = tempfile::tempdir().unwrap();
        let source = write_level(tmp.path(), &lights());
        write_particles_of(tmp.path(), emitter);
        write_attack(tmp.path());
        let mut app = app(extract(tmp.path()));
        app.add_message::<crate::tv_state::Redrawn>()
            .add_plugins(crate::level::LevelPlugin)
            .add_plugins(EmittersPlugin)
            .insert_resource(TimeUpdateStrategy::ManualDuration(Duration::from_secs_f64(
                1.0 / 60.0,
            )));
        let (tx, feed) = level_channel();
        tx.send(source).unwrap();
        app.insert_resource(feed);
        // The level loaded and its own script part's run installed (it
        // links beside the load), so the run below is the last one in.
        let ready = |app: &App| {
            let level = app.world().resource::<Level>();
            level.emitters().is_some() && level.report().scripts.is_some()
        };
        for _ in 0..400 {
            app.update();
            if ready(&app) {
                break;
            }
        }
        assert!(ready(&app));
        app.update();
        let linked = level_scripts::load(tmp.path(), LEVEL).unwrap();
        let mut files = linked.placements.clone();
        let base = files.placed.clone();
        files.placed.push(Placed::named(TRAIL, "Trail", None, 4));
        files.emitters.insert(TRAIL, "Trail".into());
        install(
            app.world_mut(),
            Arc::new(linked.program.clone()),
            &files,
            &linked.order,
        );
        let at = Vec3::from_array(START);
        let clone = app
            .world_mut()
            .query::<(Entity, &ScriptPlacement)>()
            .iter(app.world())
            .find(|(_, p)| p.0 == TRAIL)
            .map(|(e, _)| e)
            .unwrap();
        app.world_mut().entity_mut(clone).insert((
            ScriptClone(Value::Null),
            Transform::from_translation(at),
            GlobalTransform::from_translation(at),
        ));
        app.world_mut().spawn((
            Camera::default(),
            LevelOutput,
            Transform::from_translation(at + Vec3::new(0.0, 2.0, 8.0)).looking_at(at, Vec3::Y),
        ));
        (app, clone, base, tmp)
    }

    fn report(app: &App) -> ParticleReport {
        app.world().resource::<ParticleReport>().clone()
    }

    /// An emitter's clone draws its particles on their own numbers: the
    /// session's generator stands as the run left it, the particles'
    /// moves. Once its clone goes it makes nothing more and the system
    /// goes when its particles have lived out their 0.4 s.
    #[test]
    fn an_emitters_clone_draws_on_its_own_numbers_until_it_goes() {
        let (mut app, clone, base, tmp) = loaded();
        let session = app.world().resource::<ScriptRuntime>().generator.clone();
        for _ in 0..30 {
            app.update();
        }
        let r = report(&app);
        assert_eq!((r.systems, r.started), (1, 1));
        assert_eq!(r.emitters, ["Trail"]);
        assert!(r.particles >= 10, "{r:?}");
        // Its textured generator's quads; the other draws none.
        assert!(r.quads > 0 && r.quads <= r.particles, "{r:?}");
        let (system, meshes) = {
            let mut q = app.world_mut().query::<(Entity, &ParticleSystem)>();
            let (e, s) = q.single(app.world()).unwrap();
            assert_eq!(s.clone, clone);
            (e, s.meshes.clone())
        };
        assert!(meshes[1].is_none());
        let (shown, mesh) = meshes[0].clone().unwrap();
        assert_eq!(
            app.world().get::<Visibility>(shown),
            Some(&Visibility::Inherited)
        );
        let m = app.world().resource::<Assets<Mesh>>().get(&mesh).unwrap();
        assert_eq!(m.count_vertices(), 4 * r.quads);
        // Under the level's root, standing where its emitter does.
        let (_, root) = app.world().resource::<Level>().emitters().unwrap();
        assert_eq!(
            app.world().get::<ChildOf>(system).map(ChildOf::parent),
            Some(root)
        );
        assert!(
            app.world().resource::<ScriptRuntime>().generator == session,
            "the session's generator untouched"
        );
        assert_ne!(app.world().resource::<ParticleRng>().0, Cosmetic::new(SEED));

        // The run installed again without it: its clone's gone.
        let linked = level_scripts::load(tmp.path(), LEVEL).unwrap();
        let mut files = linked.placements.clone();
        files.placed = base;
        install(
            app.world_mut(),
            Arc::new(linked.program.clone()),
            &files,
            &linked.order,
        );
        let made = |app: &mut App| {
            let mut q = app.world_mut().query::<&ParticleSystem>();
            let s = q.single(app.world()).unwrap();
            (s.on, s.generators[0].made)
        };
        app.update();
        let (on, at_off) = made(&mut app);
        assert!(!on);
        // Past its 24 already (50 a second for half a second): no more.
        assert!(at_off >= 24, "{at_off}");
        app.update();
        assert_eq!(made(&mut app), (false, at_off));
        for _ in 0..30 {
            app.update();
        }
        assert_eq!(report(&app).systems, 0, "its particles' lives over");
        let mut q = app.world_mut().query::<&ParticleSystem>();
        assert_eq!(q.iter(app.world()).count(), 0);
        assert!(app.world().get_entity(system).is_err());
    }

    /// The virtual clock stopped (a pause, a hold behind an alert) holds
    /// every particle where it is: none ages, moves or is born until it
    /// runs again.
    #[test]
    fn a_stopped_clock_holds_every_particle() {
        let (mut app, _, _, _tmp) = loaded();
        for _ in 0..20 {
            app.update();
        }
        let state = |app: &mut App| {
            let mut q = app.world_mut().query::<&ParticleSystem>();
            q.single(app.world()).unwrap().generators.clone()
        };
        let before = state(&mut app);
        assert!(!before[0].particles.is_empty());
        app.world_mut().resource_mut::<Time<Virtual>>().pause();
        for _ in 0..10 {
            app.update();
        }
        assert!(state(&mut app) == before, "nothing moved while stopped");
        app.world_mut().resource_mut::<Time<Virtual>>().unpause();
        app.update();
        app.update();
        assert!(state(&mut app) != before);
    }

    /// A moving emitter's quads draw where its particles are, every frame
    /// from its first: the step runs after the transforms propagate, so its
    /// mesh's place this frame is the one it was given, not where its
    /// emitter stands now (a first frame's none at all), and its quads are
    /// laid from that.
    #[test]
    fn a_moving_emitters_quads_draw_where_its_particles_are() {
        // A stream, and a burst, whose particles all come on its first
        // frame.
        let mut burst = trail();
        if let guac_ir::particle::Kind::Sprite(s) = &mut burst.generators[0].kind {
            s.spawn_rate = 9999.0;
        }
        for (emitter, first) in [(trail(), false), (burst, true)] {
            moving_quads_draw_where_their_particles_are(emitter, first);
        }
    }

    fn moving_quads_draw_where_their_particles_are(emitter: Emitter, first: bool) {
        let (mut app, clone, _, _tmp) = loaded_with(emitter);
        let at = Vec3::from_array(START);
        let mut checked = 0;
        for k in 1..=20 {
            // A quarter metre a frame, as a thrown ball goes.
            let to = at + Vec3::new(0.25 * k as f32, 0.0, 0.0);
            app.world_mut()
                .entity_mut(clone)
                .insert(Transform::from_translation(to));
            app.update();
            let (meshes, particles) = {
                let mut q = app.world_mut().query::<&ParticleSystem>();
                let s = q.single(app.world()).unwrap();
                (s.meshes.clone(), s.generators[0].particles.clone())
            };
            let (shown, mesh) = meshes[0].clone().unwrap();
            if app.world().get::<Visibility>(shown) != Some(&Visibility::Inherited) {
                assert!(!(first && k == 1), "a burst draws on its first frame");
                continue;
            }
            let placed = *app.world().get::<GlobalTransform>(shown).unwrap();
            let m = app.world().resource::<Assets<Mesh>>().get(&mesh).unwrap();
            let positions = m
                .attribute(Mesh::ATTRIBUTE_POSITION)
                .and_then(|a| a.as_float3())
                .unwrap();
            assert_eq!(positions.len(), 4 * particles.len(), "frame {k}");
            for (quad, p) in positions.chunks(4).zip(&particles) {
                let centre = quad.iter().map(|v| Vec3::from_array(*v)).sum::<Vec3>() / 4.0;
                let drawn = placed.transform_point(centre);
                assert!(
                    drawn.distance(p.at) < 1e-3,
                    "frame {k}: drawn at {drawn}, the particle at {}",
                    p.at
                );
                checked += 1;
            }
        }
        assert!(checked > 100, "{checked}");
    }
}
