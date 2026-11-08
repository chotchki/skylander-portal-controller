//! The emitters drawn (openspec AH.87, AS.17): every clone the scripts
//! make of an emitter the extract's particle parts hold, and every emitter a
//! level's own file places that stands, gets a particle system under the
//! level's root, its generators stepped each drawn frame on the TV's virtual
//! clock (so a pause or a hold stops them) and laid out as quads facing the
//! level's camera, one mesh a generator on that generator's material
//! ([`guacamole::particles`] has what they do and how they draw).
//!
//! A clone's system follows it (the trail rides the ball the scripts hang it
//! on) while it's in play; once the scripts take it off, the system stops
//! (making its last few, the library's rule) and goes when nothing of it is
//! left to draw. A clone of the level's own emitter (its template in the
//! level's file: a breakable's dust, the chest's flash, the cannon's smoke)
//! draws off the level's particles part, found by its object there, since a
//! level's names repeat; any other off the extract-wide part by name.
//!
//! A placed emitter (openspec AS.17) stands by AG.7's rule
//! ([`guacamole::particles::placed_stands`]): an UNMANAGED one with a range
//! while the hero is within it (its buffer holding it on a little past),
//! measured from his pivot, or from the level's start before he's in, as the
//! props stand; one with no range and no buffer always; one with a buffer
//! and no range is its scripts' to wake, and stands nowhere yet (AS.92); a
//! template never. Its system stands where its file places it and stops
//! when the hero leaves its range, its particles living out their lives, as
//! a clone's do; back in range it goes on.
//!
//! Each system hangs under the level's root, not the clone, so a clone taken
//! off (whose parts go with it) leaves its particles to live out their
//! lives, and a level leaving takes them all.
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

use crate::hero::HeroBody;
use crate::hero_scripts::ScriptSource;
use crate::level::{EmitterDraw, LEVEL_LAYER, Level, LevelOutput};

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

/// What a system's emitter is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Source {
    /// A clone the scripts made, by entity.
    Clone(Entity),
    /// One the level's file places (openspec AS.17), by its place in the
    /// level's particles part.
    Placed(usize),
}

/// One emitter's particles.
#[derive(Component, Debug)]
pub(crate) struct ParticleSystem {
    source: Source,
    name: String,
    emitter: Arc<Emitter>,
    /// Whether it draws off the level's own particles part: a placed one, or
    /// a clone of one of the level's templates.
    level_own: bool,
    generators: Vec<GeneratorState>,
    /// Each generator's mesh entity and mesh; `None` for one that draws
    /// nothing.
    meshes: Vec<Option<(Entity, Handle<Mesh>)>>,
    /// Where the emitter stood last.
    place: Place,
    on: bool,
}

/// Which of the level's own emitters stand and their systems (openspec
/// AS.17), for the level whose root it holds: a new level starts it over.
#[derive(Resource, Debug, Default)]
pub(crate) struct PlacedEmitters {
    root: Option<Entity>,
    /// Each standing now, in the level's particles part's order: the rule's
    /// buffer goes by whether it stood the frame before.
    stands: Vec<bool>,
    /// Each one's system while it has one.
    systems: Vec<Option<Entity>>,
    started: usize,
}

/// What `/api/_test/tv` reports of the particles.
#[derive(Resource, Debug, Clone, Default, PartialEq, Serialize)]
pub(crate) struct ParticleReport {
    /// The scripts' clones' systems drawing now, and every one started
    /// since the TV came up.
    pub systems: usize,
    pub started: usize,
    /// Their particles alive, and quads drawn this frame.
    pub particles: usize,
    pub quads: usize,
    /// The emitters the clones drawing now are of, by name, a name once.
    pub emitters: Vec<String>,
    /// Every emitter a clone's system started for, by name, a name once, in
    /// the order first met.
    pub seen: Vec<String>,
    /// Of the clones' systems drawing now, those drawing off the level's
    /// own particles part (a clone of one of its templates, openspec
    /// AS.17), and every one started since the TV came up.
    pub level_clones: usize,
    pub level_clones_started: usize,
    /// What the level's file places itself (openspec AS.17).
    pub placed: PlacedReport,
}

/// The level's own emitters as `/api/_test/tv` reports them (openspec
/// AS.17), apart from what the scripts spawn.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub(crate) struct PlacedReport {
    /// The emitters its particles part holds, those that stand by AG.7's
    /// rule somewhere (UNMANAGED, not a template, and not a script's to
    /// wake), and those standing now.
    pub level: usize,
    pub standing: usize,
    pub awake: usize,
    /// Their systems drawing now (one standing, or one left living out its
    /// particles), and every one started since the level loaded.
    pub systems: usize,
    pub started: usize,
    /// Their particles alive, and quads drawn this frame.
    pub particles: usize,
    pub quads: usize,
    /// The placed emitters drawing now, by name, a name once.
    pub emitters: Vec<String>,
    /// What the level's particles part refused, by name with why; and why
    /// the part didn't read, if it didn't.
    pub refused: Vec<(String, String)>,
    pub unread: Option<String>,
}

pub(crate) struct EmittersPlugin;

impl Plugin for EmittersPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<ParticleRng>()
            .init_resource::<ParticleReport>()
            .init_resource::<PlacedEmitters>()
            .add_systems(
                PostUpdate,
                (
                    start_systems.run_if(resource_exists::<ScriptWorld>),
                    wake_placed,
                    step_systems,
                )
                    .chain()
                    .after(TransformSystems::Propagate),
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

/// A system for `draw` under `root` standing at `place`: its entity and a
/// hidden mesh entity a drawn generator, on the level's layer.
fn spawn_system(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    root: Entity,
    draw: &EmitterDraw,
    place: Place,
    source: Source,
    level_own: bool,
) -> Entity {
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
        source,
        name: draw.emitter.name.clone(),
        emitter: draw.emitter.clone(),
        level_own,
        generators: vec![GeneratorState::default(); draw.emitter.generators.len()],
        meshes: mesh_entities,
        place,
        on: true,
    });
    system
}

/// Each clone of an emitter the particle parts hold gets its system, once
/// the level's loaded: a clone of the level's own template off the level's
/// particles part by its object, any other off the extract-wide part by
/// name.
#[allow(clippy::too_many_arguments)]
pub(crate) fn start_systems(
    mut commands: Commands,
    world: Res<ScriptWorld>,
    source: Option<Res<ScriptSource>>,
    level: Res<Level>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut report: ResMut<ParticleReport>,
    clones: Query<Entity, (With<ScriptClone>, Without<Looked>)>,
    transforms: Query<&GlobalTransform, With<ScriptClone>>,
) {
    let Some((emitters, root)) = level.emitters() else {
        return;
    };
    let unit = source.as_deref().and_then(ScriptSource::level_unit);
    for entity in &clones {
        commands.entity(entity).insert(Looked);
        let own = world
            .emitter_placement(entity.to_bits())
            .filter(|t| Some(t.unit) == unit)
            .and_then(|t| level.level_emitter(t.index));
        let Some((draw, level_own)) = own.map(|d| (d, true)).or_else(|| {
            world
                .emitter_of(entity.to_bits())
                .and_then(|name| emitters.get(name))
                .map(|d| (d, false))
        }) else {
            continue;
        };
        let Some(place) = place_of(entity, &world, transforms.get(entity).ok()) else {
            continue;
        };
        spawn_system(
            &mut commands,
            &mut meshes,
            root,
            draw,
            place,
            Source::Clone(entity),
            level_own,
        );
        report.started += 1;
        if level_own {
            report.level_clones_started += 1;
        }
        if !report.seen.contains(&draw.emitter.name) {
            report.seen.push(draw.emitter.name.clone());
        }
    }
}

/// Each drawn frame: which of the level's own emitters stand (openspec
/// AS.17, the library's rule), from the hero's pivot or, before he's in,
/// the level's start; each standing one without a system gets one where its
/// file places it. A new level's starts them over.
pub(crate) fn wake_placed(
    mut commands: Commands,
    level: Res<Level>,
    heroes: Query<&HeroBody>,
    mut placed: ResMut<PlacedEmitters>,
    mut meshes: ResMut<Assets<Mesh>>,
) {
    let Some((emitters, root)) = level.placed_emitters() else {
        if placed.root.is_some() {
            *placed = PlacedEmitters::default();
        }
        return;
    };
    if placed.root != Some(root) {
        *placed = PlacedEmitters {
            root: Some(root),
            stands: vec![false; emitters.len()],
            systems: vec![None; emitters.len()],
            started: 0,
        };
    }
    let Some(start) = level.start() else {
        return;
    };
    let at = heroes.iter().next().map_or(start, |h| h.hero.pivot);
    for (i, p) in emitters.iter().enumerate() {
        let feet = Vec3::from_array(p.placed.position).distance(at) / guacamole::level_scene::FOOT;
        let stands = particles::placed_stands(&p.placed, placed.stands[i], Some(feet));
        placed.stands[i] = stands;
        if stands && placed.systems[i].is_none() {
            let system = spawn_system(
                &mut commands,
                &mut meshes,
                root,
                &p.draw,
                particles::placed_frame(&p.placed),
                Source::Placed(i),
                true,
            );
            placed.systems[i] = Some(system);
            placed.started += 1;
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
/// share, a clone's at its place while it's in play, a placed one's where
/// its file places it while it stands; each generator's mesh takes what it
/// draws now (shown, or hidden with nothing); a system that's off and has
/// nothing left goes.
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
    world: Option<Res<ScriptWorld>>,
    level: Res<Level>,
    mut placed: ResMut<PlacedEmitters>,
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
    let (level_emitters, refused, unread) = match (level.placed_emitters(), level.placed_refused())
    {
        (Some((e, _)), Some((refused, unread))) => (e, refused.to_vec(), unread.map(str::to_owned)),
        _ => (&[][..], Vec::new(), None),
    };
    let mut now = ParticleReport {
        started: report.started,
        seen: std::mem::take(&mut report.seen),
        level_clones_started: report.level_clones_started,
        placed: PlacedReport {
            level: level_emitters.len(),
            standing: level_emitters
                .iter()
                .filter(|p| particles::placed_stands(&p.placed, false, Some(0.0)))
                .count(),
            awake: placed.stands.iter().filter(|s| **s).count(),
            started: placed.started,
            refused,
            unread,
            ..default()
        },
        ..default()
    };
    for (entity, mut system, mut transform) in &mut systems {
        let system = &mut *system;
        match system.source {
            Source::Clone(clone) => {
                let world = world.as_deref();
                // Out of play once taken off or once what it hangs on is
                // (openspec AS.186: an emitter hung on an actor taken off
                // emits no more, as a clone hung so draws nothing).
                let in_play = world.is_some_and(|w| {
                    w.actor(clone.to_bits()).is_some_and(|a| !a.removed)
                        && !w.hangs_on_nothing(clone.to_bits())
                });
                if in_play
                    && let Some(place) =
                        world.and_then(|w| place_of(clone, w, transforms.get(clone).ok()))
                {
                    system.place = place;
                }
                system.on = in_play;
            }
            Source::Placed(i) => {
                system.on = placed.stands.get(i).copied().unwrap_or(false);
            }
        }
        for (state, g) in system.generators.iter_mut().zip(&system.emitter.generators) {
            state.step(g, system.place, system.on, dt, &mut rng.0);
        }
        if !system.on && system.generators.iter().all(|g| !g.live()) {
            commands.entity(entity).despawn();
            if let Source::Placed(i) = system.source
                && let Some(s) = placed.systems.get_mut(i)
            {
                *s = None;
            }
            continue;
        }
        // The system stands where its emitter does, so from the next
        // frame's propagation its blended draws sort by it.
        transform.translation = system.place.at;
        let (count, names) = match system.source {
            Source::Clone(_) => {
                now.systems += 1;
                now.level_clones += usize::from(system.level_own);
                (&mut now.particles, &mut now.emitters)
            }
            Source::Placed(_) => {
                now.placed.systems += 1;
                (&mut now.placed.particles, &mut now.placed.emitters)
            }
        };
        if !names.contains(&system.name) {
            names.push(system.name.clone());
        }
        let mut quads = 0;
        for ((state, g), mesh) in system
            .generators
            .iter()
            .zip(&system.emitter.generators)
            .zip(&system.meshes)
        {
            *count += state.particles.len();
            let Some((mesh_entity, handle)) = mesh else {
                continue;
            };
            let Ok((mut v, placed_at)) = drawn.get_mut(*mesh_entity) else {
                continue;
            };
            // The level's root and the system carry no turn or scale.
            let from = placed_at.translation();
            let laid = view.and_then(|v| particles::segment(state, g, system.place, v, from, 0));
            let shown = match laid.as_ref().map(static_mesh) {
                Some(Ok(m)) => {
                    quads += m.count_vertices() / 4;
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
        match system.source {
            Source::Clone(_) => now.quads += quads,
            Source::Placed(_) => now.placed.quads += quads,
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
        LEVEL, START, lights, placed_emitter, trail, write_level, write_level_particles,
        write_particles_of,
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
        loaded_placing(emitter, Vec::new())
    }

    /// [`loaded_with`], the level's particles part placing `placed`
    /// (openspec AS.17).
    fn loaded_placing(
        emitter: Emitter,
        placed: Vec<guac_ir::particle::Placed>,
    ) -> (App, Entity, Vec<Placed>, tempfile::TempDir) {
        let tmp = tempfile::tempdir().unwrap();
        let source = write_level(tmp.path(), &lights());
        write_particles_of(tmp.path(), emitter);
        write_level_particles(tmp.path(), placed, vec![("Bent".into(), "why".into())]);
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
            assert_eq!(s.source, Source::Clone(clone));
            assert!(!s.level_own, "the fighter's, off the extract-wide part");
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

    /// openspec AS.186: an emitter's clone hung on an actor taken off is
    /// out of play, as a clone hung so draws nothing: it makes nothing past
    /// its most, as one taken off, and goes once its particles have lived
    /// out; while he's on it emits on.
    #[test]
    fn an_emitter_hung_on_an_actor_taken_off_emits_no_more() {
        let (mut app, clone, base, _tmp) = loaded();
        let owner = base[0].obj;
        {
            let mut w = app
                .world_mut()
                .resource_mut::<guacamole::scripts::ScriptWorld>();
            let h = w.handles()[&owner];
            assert!(w.hang(clone.to_bits(), h, 0));
        }
        let on = |app: &mut App| {
            let mut q = app.world_mut().query::<&ParticleSystem>();
            q.iter(app.world())
                .next()
                .map(|s| (s.on, s.generators[0].made))
        };
        // Past its 24 (50 a second for half a second), hung on him.
        for _ in 0..30 {
            app.update();
        }
        let (worn, made) = on(&mut app).unwrap();
        assert!(worn && made >= 24, "hung on him, emitting: {made}");
        assert!(
            app.world_mut()
                .resource_mut::<guacamole::scripts::ScriptWorld>()
                .take_off(owner)
        );
        app.update();
        let (off, at_off) = on(&mut app).unwrap();
        assert!(!off, "him taken off");
        app.update();
        assert_eq!(on(&mut app), Some((false, at_off)), "no more");
        for _ in 0..30 {
            app.update();
        }
        assert_eq!(on(&mut app), None, "its particles' lives over");
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

    /// The system of the level's own emitter at `i` in its particles part,
    /// if it has one.
    fn placed_system(
        app: &mut App,
        i: usize,
    ) -> Option<(Entity, Vec<Option<(Entity, Handle<Mesh>)>>, bool)> {
        let mut q = app.world_mut().query::<(Entity, &ParticleSystem)>();
        q.iter(app.world())
            .find(|(_, s)| s.source == Source::Placed(i))
            .map(|(e, s)| (e, s.meshes.clone(), s.on))
    }

    /// Where each quad a system's first mesh draws is centred, in the
    /// level's frame.
    fn quad_centres(app: &App, meshes: &[Option<(Entity, Handle<Mesh>)>]) -> Vec<Vec3> {
        let (shown, mesh) = meshes[0].clone().unwrap();
        let placed = *app.world().get::<GlobalTransform>(shown).unwrap();
        let m = app.world().resource::<Assets<Mesh>>().get(&mesh).unwrap();
        let positions = m
            .attribute(Mesh::ATTRIBUTE_POSITION)
            .and_then(|a| a.as_float3())
            .unwrap();
        positions
            .chunks(4)
            .map(|q| {
                placed.transform_point(q.iter().map(|v| Vec3::from_array(*v)).sum::<Vec3>() / 4.0)
            })
            .collect()
    }

    /// AS.17: the level's own emitters draw where its file places them, by
    /// AG.7's rule, from the level's start before a hero's in and from his
    /// pivot once he is: an UNMANAGED one with a range while he's within it,
    /// one with neither range nor buffer always, one waiting on its scripts
    /// (a buffer and no range) and a template never. One he leaves stops,
    /// its particles living out their 0.4 s, and goes; the report keeps them
    /// apart from what the scripts spawn.
    #[test]
    fn a_placed_emitter_draws_where_it_stands_while_the_hero_is_in_its_range() {
        let start = Vec3::from_array(START);
        let near = start + Vec3::new(3.0, 0.0, 0.0);
        let far = start + Vec3::new(40.0, 0.0, 0.0);
        let placed = vec![
            // 30 ft is 9.1 m.
            placed_emitter("Near", 10, near, 4, 30.0, 0.0),
            placed_emitter("Far", 11, far, 4, 30.0, 0.0),
            placed_emitter(
                "Always",
                12,
                start + Vec3::new(0.0, 0.0, -60.0),
                4,
                0.0,
                0.0,
            ),
            placed_emitter("Waits", 13, start, 4, 0.0, 1.0),
            placed_emitter("Template", 14, start, 5, 0.0, 0.0),
            // 12 m is 39 ft, past its 30 (AS.17's review: a range read in
            // metres would stand it).
            placed_emitter(
                "Beyond",
                15,
                start + Vec3::new(0.0, 0.0, 12.0),
                4,
                30.0,
                0.0,
            ),
        ];
        let (mut app, _, _, _tmp) = loaded_placing(trail(), placed);
        for _ in 0..30 {
            app.update();
        }
        let r = report(&app).placed;
        assert_eq!((r.level, r.standing, r.awake), (6, 4, 2), "{r:?}");
        assert_eq!((r.systems, r.started), (2, 2), "{r:?}");
        let mut names = r.emitters.clone();
        names.sort();
        assert_eq!(names, ["Always", "Near"]);
        assert!(r.particles >= 10 && r.quads > 0, "{r:?}");
        assert_eq!(r.refused, [("Bent".to_owned(), "why".to_owned())]);
        assert_eq!(r.unread, None);
        // Kept apart from the scripts' clones: the run's clone of the
        // fighter's emitter is its own system.
        assert_eq!(report(&app).systems, 1);
        // Near's quads drawn where it stands: its particles live 0.4 s at
        // about a metre a second from a 0.2 m box.
        let (system, meshes, on) = placed_system(&mut app, 0).expect("Near's system");
        assert!(on);
        assert_eq!(
            app.world().get::<Transform>(system).unwrap().translation,
            near
        );
        let centres = quad_centres(&app, &meshes);
        assert!(!centres.is_empty());
        for c in &centres {
            assert!(c.distance(near) < 1.0, "a quad at {c}, Near at {near}");
        }
        assert!(placed_system(&mut app, 1).is_none(), "Far is out of range");
        assert!(
            placed_system(&mut app, 3).is_none(),
            "Waits is its scripts'"
        );
        assert!(
            placed_system(&mut app, 4).is_none(),
            "a template stands nowhere"
        );
        assert!(
            placed_system(&mut app, 5).is_none(),
            "Beyond is 39 ft off, its range 30 ft"
        );

        // A hero in by Far: it stands, Near stops and its particles live
        // out their lives, then its system goes.
        let ground = guac_motion::scenarios::bodies(&guac_motion::scenarios::open_ground(), &[]);
        let parry = guac_motion::parry::ParryWorld::new(&ground).unwrap();
        let hero = guac_motion::scenarios::eruptor(&parry, far, 0.0);
        app.world_mut().spawn(crate::hero::HeroBody {
            before: hero.pivot,
            hero,
            exclude: Vec::new(),
        });
        app.update();
        let r = report(&app).placed;
        assert_eq!((r.awake, r.started), (2, 3), "{r:?}");
        let (_, _, on) = placed_system(&mut app, 0).expect("Near's, living out");
        assert!(!on);
        for _ in 0..40 {
            app.update();
        }
        assert!(
            placed_system(&mut app, 0).is_none(),
            "Near's particles over"
        );
        let (system, meshes, on) = placed_system(&mut app, 1).expect("Far's system");
        assert!(on);
        assert_eq!(
            app.world().get::<Transform>(system).unwrap().translation,
            far
        );
        for c in quad_centres(&app, &meshes) {
            assert!(c.distance(far) < 1.0, "a quad at {c}, Far at {far}");
        }
        let mut names = report(&app).placed.emitters;
        names.sort();
        assert_eq!(names, ["Always", "Far"]);
    }

    /// AS.17: a clone of one of the level's own templates draws off the
    /// level's particles part, found by its object in the level's file
    /// (names repeat), not off the extract-wide part's emitter of its name.
    #[test]
    fn a_clone_of_the_levels_own_emitter_draws_off_its_particles_part() {
        // The level's own `Trail`, one textured generator to the fighter's
        // two, at object 21 of the level's file.
        let mut own = placed_emitter("Trail", 21, Vec3::ZERO, 5, 0.0, 0.0);
        own.emitter.generators.truncate(1);
        let (mut app, _, _, tmp) = loaded_placing(trail(), vec![own]);
        let unit = app
            .world()
            .resource::<ScriptSource>()
            .level_unit()
            .expect("the level's unit");
        let template = Obj { unit, index: 21 };
        let linked = level_scripts::load(tmp.path(), LEVEL).unwrap();
        let mut files = linked.placements.clone();
        files.placed.push(Placed::named(template, "Trail", None, 5));
        files.emitters.insert(template, "Trail".into());
        install(
            app.world_mut(),
            Arc::new(linked.program.clone()),
            &files,
            &linked.order,
        );
        let clone = app
            .world_mut()
            .query::<(Entity, &ScriptPlacement)>()
            .iter(app.world())
            .find(|(_, p)| p.0 == template)
            .map(|(e, _)| e)
            .unwrap();
        let at = Vec3::from_array(START);
        app.world_mut().entity_mut(clone).insert((
            ScriptClone(Value::Null),
            Transform::from_translation(at),
            GlobalTransform::from_translation(at),
        ));
        for _ in 0..10 {
            app.update();
        }
        let r = report(&app);
        assert_eq!(
            (r.systems, r.level_clones, r.level_clones_started),
            (1, 1, 1),
            "{r:?}"
        );
        let mut q = app.world_mut().query::<&ParticleSystem>();
        let s = q.single(app.world()).unwrap();
        assert_eq!(s.source, Source::Clone(clone));
        assert!(s.level_own);
        assert_eq!(
            s.emitter.generators.len(),
            1,
            "the level's, not the fighter's"
        );
        assert_eq!(s.emitter.object, 21);
    }

    /// AS.17's review: a clone of another file's emitter draws off the
    /// extract-wide part by its name even where the level's file holds an
    /// emitter at the same object: an object is looked up in the level's
    /// part only for the level's own unit.
    #[test]
    fn a_clone_of_another_files_emitter_draws_off_the_extract_wide_part() {
        // The level's own one-generator `Trail` at object 1, the index the
        // fighter's file holds its [`TRAIL`] at.
        let mut own = placed_emitter("Trail", TRAIL.index, Vec3::ZERO, 5, 0.0, 0.0);
        own.emitter.generators.truncate(1);
        let (mut app, clone, _, _tmp) = loaded_placing(trail(), vec![own]);
        assert_ne!(
            app.world().resource::<ScriptSource>().level_unit(),
            Some(TRAIL.unit)
        );
        for _ in 0..10 {
            app.update();
        }
        let r = report(&app);
        assert_eq!(
            (r.systems, r.level_clones, r.level_clones_started),
            (1, 0, 0),
            "{r:?}"
        );
        let mut q = app.world_mut().query::<&ParticleSystem>();
        let s = q.single(app.world()).unwrap();
        assert_eq!(s.source, Source::Clone(clone));
        assert!(!s.level_own);
        assert_eq!(s.emitter.generators.len(), 2, "the fighter's two");
    }
}
