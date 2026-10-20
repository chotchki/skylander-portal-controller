//! The hero on the TV's side of the seam (openspec AG.16, design decisions
//! 3 and 5): `guac-motion`'s controller stepped in `FixedUpdate` at the
//! original's nominal 1/60 s over Avian's queries ([`AvianSeam`]), his drawn
//! `Transform` eased between steps by `bevy_transform_interpolation`, so the
//! TV draws at any rate and the walk stays the same (hero-movement's The same
//! walk at any frame rate). A later box (AG.21) puts [`HeroPlugin`] on the
//! TV and feeds [`HeroInput`] from the pad and the camera; the tests here
//! run it headless.
//!
//! Ordering: Avian refits its trees for a moved kinematic body when its
//! physics schedule runs (`FixedPostUpdate`), so a body moved in `FixedUpdate`
//! before the hero's step would be met where it stood the step before, where
//! `ParryWorld` sees it at once. Movers go after [`HeroSystems::Step`]: then
//! both sides meet it where it stood when the step began, and record its
//! velocity in [`MovingSolids`] for the next.

#![cfg_attr(
    not(test),
    expect(
        dead_code,
        reason = "AG.21 puts the hero on the TV; until then its tests are its callers"
    )
)]

use bevy::prelude::*;
use bevy_transform_interpolation::prelude::{TransformInterpolation, TransformInterpolationPlugin};
use guac_motion::hero::{Hero, Input};
use guac_motion::proxy::Surfaces;
use guac_motion::seam::BodyId;
use guac_motion::stick::Stick;

use crate::physics::AvianSeam;

/// The game's step: the original's nominal frame (design decision 5).
pub(crate) const STEP_HZ: f64 = 60.0;

pub(crate) struct HeroPlugin;

impl Plugin for HeroPlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(Time::<Fixed>::from_hz(STEP_HZ))
            .add_plugins(TransformInterpolationPlugin::default())
            .init_resource::<HeroInput>()
            .init_resource::<MovingSolids>()
            .add_systems(FixedUpdate, step_heroes.in_set(HeroSystems::Step));
    }
}

#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub(crate) enum HeroSystems {
    /// Every hero's frame; kinematic movers run after it.
    Step,
}

/// What the next step reads of the player: the walking stick and the
/// heading the view looked along the step before.
#[derive(Resource, Debug, Clone, Copy, PartialEq)]
pub(crate) struct HeroInput(pub(crate) Input);

impl Default for HeroInput {
    fn default() -> Self {
        Self(Input::stick(Stick::CENTRED, 0.0))
    }
}

/// How fast each kinematic solid went over its last move, kept by whoever
/// moves it ([`Surfaces`]).
#[derive(Resource, Debug, Default, Clone, PartialEq)]
pub(crate) struct MovingSolids(pub(crate) Surfaces);

/// A hero: the controller's state and the bodies his queries skip.
#[derive(Component, Debug, Clone)]
pub(crate) struct HeroBody {
    pub(crate) hero: Hero,
    pub(crate) exclude: Vec<BodyId>,
}

/// His drawn transform: the pivot, his facing turned before his lean.
fn placed(hero: &Hero) -> Transform {
    Transform::from_translation(hero.pivot).with_rotation(hero.rotation())
}

/// Spawns a hero where `hero` stands, eased between steps.
pub(crate) fn spawn_hero(commands: &mut Commands, hero: Hero) -> Entity {
    commands
        .spawn((
            placed(&hero),
            TransformInterpolation,
            HeroBody {
                hero,
                exclude: Vec::new(),
            },
        ))
        .id()
}

/// One game frame of every hero, the fixed step's length its step.
fn step_heroes(
    seam: AvianSeam,
    input: Res<HeroInput>,
    moving: Res<MovingSolids>,
    time: Res<Time>,
    mut heroes: Query<(&mut HeroBody, &mut Transform)>,
) {
    let step = time.delta_secs();
    for (mut body, mut transform) in &mut heroes {
        let HeroBody { hero, exclude } = &mut *body;
        hero.step_among(&seam, &input.0, step, exclude, &moving.0);
        *transform = placed(hero);
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use bevy::ecs::system::SystemState;
    use bevy::time::TimeUpdateStrategy;
    use guac_motion::parry::ParryWorld;
    use guac_motion::scenarios::{self, eruptor, stick};
    use guac_motion::seam::{Body, BodyKind, Collider, Pose, Shape, Vec3};

    use super::*;
    use crate::physics::{SeamBody, SeamPlugin, set_pose, spawn_body};
    use avian3d::prelude::{Position, Rotation};

    /// Fixed steps run, counted in `FixedUpdate` after the heroes'.
    #[derive(Resource, Default)]
    struct Steps(u32);

    fn count(mut steps: ResMut<Steps>) {
        steps.0 += 1;
    }

    /// The pad follows the walk script while this is in the world.
    #[derive(Resource)]
    struct WalkScript;

    /// The pad as a step reads it: the walk script by the step's count, set
    /// in the fixed step itself, so every display rate feeds a step the same.
    fn script(steps: Res<Steps>, mut input: ResMut<HeroInput>) {
        *input = walk_input(steps.0);
    }

    /// A headless app on the hero's plugins, its display frame `frame`
    /// seconds, with `bodies` in Avian's world and the walk script on the
    /// pad, its step count at 0.
    fn headless(frame: f64, bodies: &[Body]) -> App {
        let mut app = App::new();
        app.add_plugins((MinimalPlugins, SeamPlugin, HeroPlugin))
            .insert_resource(TimeUpdateStrategy::ManualDuration(Duration::from_secs_f64(
                frame,
            )))
            .init_resource::<Steps>()
            .insert_resource(WalkScript)
            .add_systems(
                FixedUpdate,
                (
                    script
                        .run_if(resource_exists::<WalkScript>)
                        .before(HeroSystems::Step),
                    count.after(HeroSystems::Step),
                ),
            );
        app.finish();
        app.cleanup();
        let mut commands = app.world_mut().commands();
        for body in bodies {
            spawn_body(&mut commands, body).unwrap();
        }
        app.world_mut().flush();
        // A fixed step's physics schedule builds the trees the queries walk.
        while app.world().resource::<Steps>().0 == 0 {
            app.update();
        }
        app.world_mut().resource_mut::<Steps>().0 = 0;
        app
    }

    /// Eruptor settled on Avian's world at `at`, facing `facing`.
    fn spawn_eruptor(app: &mut App, at: Vec3, facing: f32) -> Entity {
        let mut state = SystemState::<AvianSeam>::new(app.world_mut());
        let hero = eruptor(&state.get(app.world()).unwrap(), at, facing);
        let mut commands = app.world_mut().commands();
        let entity = spawn_hero(&mut commands, hero);
        app.world_mut().flush();
        entity
    }

    fn hero_of(app: &App, entity: Entity) -> &Hero {
        &app.world().entity(entity).get::<HeroBody>().unwrap().hero
    }

    fn steps(app: &App) -> u32 {
        app.world().resource::<Steps>().0
    }

    fn open_ground() -> Vec<Body> {
        scenarios::bodies(&scenarios::open_ground(), &[])
    }

    const START: Vec3 = Vec3::new(0.0, 1.0, 0.0);

    /// The stick held for the first 60 steps, at 30° off a view along 15°,
    /// then let go.
    fn walk_input(step: u32) -> HeroInput {
        let s = if step < 60 {
            stick(1.0, 30.0)
        } else {
            Stick::CENTRED
        };
        HeroInput(Input::stick(s, 15.0))
    }

    /// Runs display frames until `n` fixed steps have run.
    fn walk_steps(app: &mut App, n: u32) {
        while steps(app) < n {
            app.update();
        }
    }

    /// The hero steps once a fixed step at 1/60 s, and on Avian's side of the
    /// seam walks where the controller on parry walks, step for step, to
    /// float noise.
    #[test]
    fn the_hero_steps_at_60_hz_as_on_parry() {
        let bodies = open_ground();
        let mut app = headless(1.0 / 60.0, &bodies);
        assert_eq!(
            app.world().resource::<Time<Fixed>>().timestep(),
            Duration::from_secs_f64(1.0 / 60.0)
        );
        let entity = spawn_eruptor(&mut app, START, 15.0);
        let parry = ParryWorld::new(&bodies).unwrap();
        let mut by_hand = eruptor(&parry, START, 15.0);
        let mut worst = 0.0f32;
        for k in 0..120 {
            app.update();
            assert_eq!(steps(&app), k + 1, "one step a 1/60 s frame");
            by_hand.step(&parry, &walk_input(k).0, (1.0f64 / 60.0) as f32, &[]);
            worst = worst.max((hero_of(&app, entity).pivot - by_hand.pivot).length());
        }
        eprintln!("Avian against parry over 120 steps: {worst:e} m at worst");
        assert!(worst < 1e-5, "{worst}");
        // 60 steps at full stick and 16 slowing: well off the start.
        assert!((hero_of(&app, entity).pivot - by_hand.pivot).length() < 1e-5);
        assert!(by_hand.pivot.distance(START) > 5.0);
    }

    /// The same path at any display rate: at 30, 60 and 144 Hz the hero is
    /// where the same 120 steps leave him, to the bit, and his drawn
    /// transform eases between the last two steps by how far the display is
    /// into the next.
    #[test]
    fn the_same_path_at_30_60_and_144_hz() {
        let bodies = open_ground();
        let mut ends = Vec::new();
        for rate in [30.0, 60.0, 144.0] {
            let mut app = headless(1.0 / rate, &bodies);
            let entity = spawn_eruptor(&mut app, START, 15.0);
            walk_steps(&mut app, 120);
            assert_eq!(steps(&app), 120, "{rate} Hz");
            ends.push(hero_of(&app, entity).pivot);
        }
        eprintln!("after 120 steps at 30, 60 and 144 Hz: {ends:?}");
        assert_eq!(ends[0], ends[1]);
        assert_eq!(ends[1], ends[2]);
    }

    /// Between steps the drawn transform is the interpolation's: at 144 Hz,
    /// on the segment from the step before's pivot to this one's, as far
    /// along it as the display is into the next step.
    #[test]
    fn the_drawn_transform_eases_between_steps() {
        let bodies = open_ground();
        let mut app = headless(1.0 / 144.0, &bodies);
        let entity = spawn_eruptor(&mut app, START, 15.0);
        walk_steps(&mut app, 30);
        let mut checked = 0;
        for _ in 0..12 {
            let before = hero_of(&app, entity).pivot;
            let n = steps(&app);
            app.update();
            let after = hero_of(&app, entity).pivot;
            let alpha = app.world().resource::<Time<Fixed>>().overstep_fraction();
            let drawn = app
                .world()
                .entity(entity)
                .get::<Transform>()
                .unwrap()
                .translation;
            if steps(&app) == n + 1 {
                let want = before.lerp(after, alpha);
                assert!((drawn - want).length() < 1e-4, "{drawn} {want} {alpha}");
                checked += 1;
            }
        }
        eprintln!("{checked} display frames that ran a step checked against the lerp");
        assert!(checked >= 4, "{checked}");
        // Walking at 6.553 m/s, a step moves him 0.109 m: the drawn
        // transform isn't the stepped pivot mid-step.
        let pivot = hero_of(&app, entity).pivot;
        let drawn = app
            .world()
            .entity(entity)
            .get::<Transform>()
            .unwrap()
            .translation;
        let alpha = app.world().resource::<Time<Fixed>>().overstep_fraction();
        assert!(alpha > 0.0 && alpha < 1.0, "{alpha}");
        assert!((drawn - pivot).length() > 1e-3, "{drawn} {pivot}");
    }

    /// A kinematic block moved after the hero's step, its velocity kept in
    /// `MovingSolids`, pushes him on Avian's side as on parry's: 1 m/s, 5 mm
    /// off its face once it has moved. Its move shows to Avian's queries once
    /// the physics schedule has refit the trees, the next step, which is when
    /// the hero meets it.
    #[test]
    fn a_block_moved_after_his_step_pushes_him_as_on_parry() {
        let block = BodyId(2);
        let half = 1.0;
        let start_at = Vec3::new(0.0, half, 3.0);
        let mut bodies = open_ground();
        bodies.push(Body {
            id: block,
            kind: BodyKind::Kinematic,
            pose: Pose::at(start_at),
            collider: Collider::Convex(Shape::Box {
                half_extents: Vec3::splat(half),
                margin: 0.0,
            }),
        });
        let towards = Vec3::new(0.0, 0.0, -1.0);
        let dt = (1.0f64 / 60.0) as f32;

        #[derive(Resource)]
        struct Block(Vec3);
        fn move_block(
            mut at: ResMut<Block>,
            mut moving: ResMut<MovingSolids>,
            mut bodies: Query<(&SeamBody, &mut Position, &mut Rotation)>,
        ) {
            let towards = Vec3::new(0.0, 0.0, -1.0);
            at.0 += towards * (1.0f64 / 60.0) as f32;
            set_pose(&mut bodies, BodyId(2), Pose::at(at.0)).unwrap();
            moving.0.set(BodyId(2), towards);
        }

        let mut app = headless(1.0 / 60.0, &bodies);
        app.world_mut().remove_resource::<WalkScript>();
        app.insert_resource(Block(start_at))
            .insert_resource(HeroInput::default())
            .add_systems(FixedUpdate, move_block.after(HeroSystems::Step));
        let entity = spawn_eruptor(&mut app, START, 0.0);

        let mut parry = ParryWorld::new(&bodies).unwrap();
        let mut by_hand = eruptor(&parry, START, 0.0);
        let mut surfaces = Surfaces::default();
        let mut at = start_at;
        let mut worst = 0.0f32;
        for _ in 0..150 {
            app.update();
            by_hand.step_among(
                &parry,
                &Input::stick(Stick::CENTRED, 0.0),
                dt,
                &[],
                &surfaces,
            );
            at += towards * dt;
            parry.set_pose(block, Pose::at(at)).unwrap();
            surfaces.set(block, towards);
            worst = worst.max((hero_of(&app, entity).pivot - by_hand.pivot).length());
        }
        eprintln!("Avian against parry, the block's push: {worst:e} m at worst");
        assert!(worst < 1e-4, "{worst}");
        let hero = hero_of(&app, entity);
        assert!((hero.velocity.z + 1.0).abs() < 1e-3, "{:?}", hero.velocity);
        let gap = (at.z - half) - (hero.pivot.z + 0.9144);
        assert!((gap - 0.005).abs() < 1e-4, "{gap}");
    }

    /// AG.16's Verify: the controller's open-ground and wall cases, each in
    /// its world on Avian's side of the seam, headless.
    #[test]
    fn the_controllers_open_ground_and_wall_cases_through_avian() {
        let mut failures = Vec::new();
        let all = scenarios::open_ground_scenarios()
            .into_iter()
            .chain(scenarios::wall_scenarios());
        let mut ran = 0;
        for scenario in all {
            let mut app = headless(1.0 / 60.0, &scenario.bodies);
            let mut state = SystemState::<AvianSeam>::new(app.world_mut());
            let seam = state.get(app.world()).unwrap();
            if let Err(e) = (scenario.check)(&seam) {
                failures.push(format!("{}: {e}", scenario.name));
            }
            ran += 1;
        }
        assert_eq!(ran, 16);
        assert!(failures.is_empty(), "{}", failures.join("\n"));
    }
}
