//! The hero on the TV's side of the seam (openspec AG.16, design decisions
//! 3 and 5): `guac-motion`'s controller stepped in `FixedUpdate` at the
//! original's nominal 1/60 s over Avian's queries ([`AvianSeam`]), his drawn
//! `Transform` eased between steps by `bevy_transform_interpolation`, so the
//! TV draws at any rate and the walk stays the same (hero-movement's The same
//! walk at any frame rate). The gameplay camera steps beside him (AG.17,
//! [`FollowCamera`]): after his step, on his pivot as the step began, its
//! `Transform` the eye looking at the aim, eased the same way; his next step
//! walks him against the view it left (gameplay-camera's The stick held
//! through a switch). Once a hero has joined a run whose camera's scripts
//! place the viewport (openspec AG.28), the view is theirs, one writer: the
//! camera entity takes `viewportinfo.default` as they leave it each step
//! ([`guacamole::viewport`]), and the chain stays only as the fall-back once
//! one of them stops in play, handed the view from where it stood
//! (stand-in ledger: `camera`). AG.21 puts [`HeroPlugin`] on the TV
//! ([`crate::play`]), [`crate::pad`]'s layer feeds [`HeroInput`] and the
//! level's camera is the [`FollowCamera`]'s entity; the tests here run it
//! headless. His locomotion steps after him (AG.30, [`HeroGaits`]): the
//! clips' weights, times and rates from the speed his step's walk read and
//! his fall, which [`crate::play`] poses his model by.
//!
//! A hero whose own scripts run (AH.24, [`ScriptedHero`]; AH design decision
//! 5, one writer) steps on the engine alone: his walk and his facing are
//! what his scripts on the VM left, read before his step
//! ([`crate::hero_scripts`]), and nothing here turns or pushes him. Any
//! other walks on the stick by AG.16's controller
//! (stand-in ledger: `walker`).
//!
//! Ordering ([`guacamole::frame`], openspec AH.59): his step is
//! [`frame::Frame::Hero`], after the scripts, his locomotion
//! [`frame::Frame::Locomotion`] and the camera [`frame::Frame::Camera`], as
//! the original's actors' updater runs after its scripts' and its
//! viewport's after both. Avian refits its trees for a moved kinematic body
//! when its physics schedule runs (`FixedPostUpdate`), so a body moved in
//! `FixedUpdate` before the hero's step would be met where it stood the
//! step before, where `ParryWorld` sees it at once. Movers go in
//! [`frame::Frame::Actors`], after his step: then both sides meet it where
//! it stood when the step began, and record its velocity in
//! [`MovingSolids`] for the next.

use bevy::camera::{PerspectiveProjection, Projection};
use bevy::prelude::*;
use bevy_transform_interpolation::prelude::{TransformInterpolation, TransformInterpolationPlugin};
use guac_motion::camera::{CameraDef, GameCamera, LookFrom, NEAR, TransitionPlane, View};
use guac_motion::hero::{Hero, Input};
use guac_motion::locomotion::{Frame, Locomotion};
use guac_motion::proxy::Surfaces;
use guac_motion::seam::BodyId;
use guac_motion::stick::Stick;
use guacamole::picks::Picked;

use guacamole::frame;
use guacamole::scripts::ScriptWorld;
use guacamole::viewport::Viewport;
use tracing::warn;

use crate::physics::AvianSeam;

/// The game's step: the original's nominal frame (design decision 5).
pub(crate) const STEP_HZ: f64 = 60.0;

pub(crate) struct HeroPlugin;

impl Plugin for HeroPlugin {
    fn build(&self, app: &mut App) {
        frame::ensure(app);
        app.insert_resource(Time::<Fixed>::from_hz(STEP_HZ))
            .add_plugins(TransformInterpolationPlugin::default())
            .init_resource::<HeroInput>()
            .init_resource::<HeroActions>()
            .init_resource::<MovingSolids>()
            .init_resource::<CameraScripts>()
            .add_systems(
                FixedUpdate,
                (
                    step_heroes.in_set(frame::Frame::Hero),
                    step_gaits.in_set(frame::Frame::Locomotion),
                    step_cameras.in_set(frame::Frame::Camera),
                ),
            );
    }
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

/// The attacks the walking pad holds (AH.24, [`crate::pad::actions`]), and
/// those pressed since a game step last read them, so a press shorter than
/// a step (a display frame at over 60 a second, or a press and release
/// inside one frame) still reaches the scripts as a press (pad-input's Every
/// press counts).
#[derive(Resource, Debug, Default, Clone, Copy, PartialEq)]
pub(crate) struct HeroActions {
    /// Held this frame.
    pub(crate) held: [bool; 4],
    /// Held or pressed since a step last read them.
    latched: [bool; 4],
}

impl HeroActions {
    /// A frame of the level's layer: the attacks held now and those whose
    /// buttons went down.
    pub(crate) fn frame(&mut self, held: [bool; 4], tapped: [bool; 4]) {
        self.held = held;
        for ((latched, held), tapped) in self.latched.iter_mut().zip(held).zip(tapped) {
            *latched |= held || tapped;
        }
    }

    /// What a game step reads: each attack held now or pressed since a
    /// step last read them; those kept for it are then let go.
    pub(crate) fn take(&mut self) -> [bool; 4] {
        let out = std::array::from_fn(|i| self.held[i] || self.latched[i]);
        self.latched = [false; 4];
        out
    }

    /// The portal holds the level past its first frame (AG.29): nothing
    /// held or pressed before it is kept for a step after it, so no attack
    /// reaches the scripts from under an alert, however few steps its first
    /// frame ran.
    pub(crate) fn release(&mut self) {
        *self = Self::default();
    }
}

/// A hero whose own scripts run (AH.24): his step is the engine's alone, his
/// walk and facing his scripts' (AH design decision 5, one writer).
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ScriptedHero;

/// How fast each kinematic solid went over its last move, kept by whoever
/// moves it ([`Surfaces`]).
#[derive(Resource, Debug, Default, Clone, PartialEq)]
pub(crate) struct MovingSolids(pub(crate) Surfaces);

/// A hero: the controller's state and the bodies his queries skip.
#[derive(Component, Debug, Clone)]
pub(crate) struct HeroBody {
    pub(crate) hero: Hero,
    pub(crate) exclude: Vec<BodyId>,
    /// His pivot as his last step began: where the camera's step reads his
    /// feet.
    pub(crate) before: Vec3,
}

/// A hero's locomotion (AG.30; stand-in ledger: `locomotion`): stepped
/// after him, with the weights the step before left, so his pose eases
/// between steps as his transform does. While his scripts pick his clips on
/// the VM (openspec AH.49, [`guacamole::picks`]) the stand-in isn't stepped
/// (one side picks): what they picked is `picked`, its weights the step
/// before left `picked_before`.
#[derive(Component, Debug, Clone)]
pub(crate) struct HeroGaits {
    pub(crate) locomotion: Locomotion,
    pub(crate) before: [f32; 4],
    pub(crate) picked: Option<Picked>,
    pub(crate) picked_before: Vec<(u32, f32)>,
    /// Steps since he came in, whichever side picked: the gait log's key.
    pub(crate) steps: u64,
}

impl HeroGaits {
    pub(crate) fn new(locomotion: Locomotion) -> Self {
        let before = locomotion.weights();
        Self {
            locomotion,
            before,
            picked: None,
            picked_before: Vec::new(),
            steps: 0,
        }
    }
}

/// The gameplay camera following the hero, and the level's transition
/// planes it blends through.
#[derive(Component, Debug, Clone)]
pub(crate) struct FollowCamera {
    pub(crate) camera: GameCamera,
    pub(crate) planes: Vec<TransitionPlane>,
    /// What the TV drew last: the stick reads its heading the step after.
    pub(crate) drawn: View,
    /// Who drew it.
    pub(crate) hold: ViewHold,
}

impl FollowCamera {
    /// `camera` with the level's `planes`, the view its own until the
    /// camera's scripts take it.
    pub(crate) fn new(camera: GameCamera, planes: Vec<TransitionPlane>) -> Self {
        Self {
            drawn: camera.view(),
            camera,
            planes,
            hold: ViewHold::default(),
        }
    }
}

/// Who has the view the TV draws (openspec AG.28; AH design decision 5's one
/// writer): `/api/_test/tv`'s `hero.camera.scripts`.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum ViewHold {
    /// AG.17's chain, the camera's scripts not driving yet (no hero has
    /// joined their run, or the part runs no camera system), with why.
    Chain(String),
    /// The camera's scripts on the VM.
    Scripts,
    /// The camera's scripts stopped in play: the chain has had the view
    /// since, from where it stood, for good (stand-in ledger: `camera`).
    HandedBack(String),
}

impl Default for ViewHold {
    fn default() -> Self {
        Self::Chain(NOT_YET.to_owned())
    }
}

/// Why the chain has the view before a hero joins the camera's scripts.
pub(crate) const NOT_YET: &str = "no hero has joined the camera's scripts";

impl ViewHold {
    /// `on`, `off` or `none`, and why when it isn't `on`.
    pub(crate) fn report(&self) -> (&'static str, Option<String>) {
        match self {
            Self::Scripts => ("on", None),
            Self::HandedBack(why) => ("off", Some(why.clone())),
            Self::Chain(why) => ("none", Some(why.clone())),
        }
    }
}

/// What the camera's scripts on the VM say of the view this step, set by
/// the scripts' step ([`crate::hero_scripts`]) and read by the camera's.
#[derive(Resource, Debug, Clone, PartialEq)]
pub(crate) enum CameraScripts {
    /// Not driving, with why.
    Idle(String),
    /// Driving: the viewport is theirs.
    On,
    /// Stopped in play, with why: the view goes to the chain for good.
    Stopped(String),
}

impl Default for CameraScripts {
    fn default() -> Self {
        Self::Idle(NOT_YET.to_owned())
    }
}

/// The view the camera's scripts left, as the TV draws it: their eye and
/// turn, looking at the point along it nearest his feet (where the chain
/// would aim), their picture and far clip.
pub(crate) fn scripts_view(v: &Viewport, feet: Option<Vec3>) -> View {
    let forward = v.forward();
    let along = feet.map_or(10.0, |f| (f - v.eye).dot(forward)).max(1.0);
    View {
        eye: v.eye,
        aim: v.eye + forward * along,
        fov_y: v.fov_y(),
        far_clip: v.far_clip,
    }
}

/// The chain at rest where `view` stands, his feet at `feet` (openspec
/// AG.28: the view handed back from where it stood): its follow point at
/// his feet's height under the point of the view's line nearest them across
/// the ground, a camera of its own whose eye sits at the view's and whose
/// aim is that point, so its first frame draws the view unmoved and its
/// follow point then closes on his feet as the chain's does.
pub(crate) fn chain_from(view: &View, feet: Vec3) -> GameCamera {
    let foot = guac_motion::tuning::FOOT;
    let look = (view.aim - view.eye).normalize_or_zero();
    let across = look.with_y(0.0);
    let along = if across.length_squared() > 1e-8 {
        ((feet - view.eye).dot(across) / across.length_squared()).max(0.1)
    } else {
        (view.aim - view.eye).length()
    };
    let aim = view.eye + look * along;
    let follow = aim.with_y(feet.y);
    let back = view.eye - follow;
    let distance = back.length().max(1e-3);
    let def = CameraDef {
        id: u32::MAX,
        distance: distance / foot,
        pitch: (back.y / distance).clamp(-1.0, 1.0).asin().to_degrees(),
        look_from: LookFrom::Heading(guac_motion::camera::bearing(back)),
        look_at: (aim.y - feet.y) / foot,
        fov_y: view.fov_y,
        far_clip: view.far_clip,
    };
    GameCamera::resting(def, follow, feet, feet)
}

/// The camera's drawn transform: at the eye, looking at the aim.
pub(crate) fn viewed(view: &View) -> Transform {
    Transform::from_translation(view.eye).looking_at(view.aim, Vec3::Y)
}

/// The picture a view draws: its height, the near plane and its far clip;
/// the aspect follows the target.
pub(crate) fn projection(view: &View) -> PerspectiveProjection {
    PerspectiveProjection {
        fov: view.fov_y.to_radians(),
        near: NEAR,
        far: view.far(),
        ..PerspectiveProjection::default()
    }
}

/// The gameplay camera where `camera` stands, eased between steps: what a
/// camera entity takes to follow the hero.
pub(crate) fn following(camera: FollowCamera) -> impl Bundle {
    let view = camera.drawn;
    (
        viewed(&view),
        Projection::Perspective(projection(&view)),
        TransformInterpolation,
        camera,
    )
}

/// Spawns the gameplay camera where `camera` stands, eased between steps.
#[cfg(test)]
pub(crate) fn spawn_camera(commands: &mut Commands, camera: FollowCamera) -> Entity {
    commands.spawn(following(camera)).id()
}

/// The way he faces, degrees in [0, 360): what `/api/_test/tv` reports as
/// `hero.heading` and each step of `/api/_test/tv/gaits` as `facing`
/// (openspec AG.188).
pub(crate) fn heading(hero: &Hero) -> f32 {
    hero.facing.rem_euclid(360.0)
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
                before: hero.pivot,
                hero,
                exclude: Vec::new(),
            },
        ))
        .id()
}

/// One game frame of every hero, the fixed step's length its step, against
/// the gameplay camera's view as the frame before left it when there is one.
pub(crate) fn step_heroes(
    seam: AvianSeam,
    input: Res<HeroInput>,
    moving: Res<MovingSolids>,
    time: Res<Time>,
    cameras: Query<&FollowCamera>,
    mut heroes: Query<(&mut HeroBody, &mut Transform, Has<ScriptedHero>), Without<FollowCamera>>,
) {
    let step = time.delta_secs();
    let mut input = input.0;
    if let Some(follow) = cameras.iter().next() {
        input.view_heading = follow.drawn.heading();
    }
    for (mut body, mut transform, scripted) in &mut heroes {
        let HeroBody {
            hero,
            exclude,
            before,
        } = &mut *body;
        *before = hero.pivot;
        if scripted {
            hero.step_engine(&seam, step, exclude, &moving.0);
        } else {
            hero.step_among(&seam, &input, step, exclude, &moving.0);
        }
        *transform = placed(hero);
    }
}

/// One game frame of each hero's locomotion, on the speed his step's walk
/// read and his move and footing after it; none for a hero whose scripts
/// picked his clips this step (openspec AH.49: AG.30's picker off for him).
pub(crate) fn step_gaits(time: Res<Time>, mut heroes: Query<(&HeroBody, &mut HeroGaits)>) {
    let step = time.delta_secs();
    for (body, mut gaits) in &mut heroes {
        let gaits = &mut *gaits;
        gaits.steps += 1;
        if gaits.picked.is_some() {
            continue;
        }
        gaits.before = gaits.locomotion.weights();
        gaits.locomotion.step(step, Frame::of(&body.hero));
    }
}

/// One game frame of the gameplay camera: the view the camera's scripts
/// left this step while they drive (openspec AG.28), else the chain on the
/// hero's pivot as his step began. One hero walks (pad-input's One pad walks
/// the hero): with none, or a second, the chain waits. Their stopping hands
/// the chain the view from where it stood.
pub(crate) fn step_cameras(
    time: Res<Time>,
    heroes: Query<&HeroBody>,
    scripts: Option<Res<CameraScripts>>,
    world: Option<Res<ScriptWorld>>,
    mut cameras: Query<
        (&mut FollowCamera, &mut Transform, Option<&mut Projection>),
        Without<HeroBody>,
    >,
) {
    let hero = heroes.single().ok();
    let feet = hero.map(|h| h.hero.pivot);
    let step = time.delta_secs();
    let state = scripts.as_deref().cloned().unwrap_or_default();
    for (mut follow, mut transform, drawn) in &mut cameras {
        let follow = &mut *follow;
        let place = match (&state, world.as_deref()) {
            (CameraScripts::On, Some(world)) => {
                follow.hold = ViewHold::Scripts;
                let v = world.viewport();
                follow.drawn = scripts_view(v, feet);
                v.transform()
            }
            _ => {
                match &state {
                    CameraScripts::Stopped(why) if follow.hold == ViewHold::Scripts => {
                        follow.camera = chain_from(&follow.drawn, feet.unwrap_or(follow.drawn.aim));
                        follow.hold = ViewHold::HandedBack(why.clone());
                        warn!(
                            reason = %why,
                            "the camera's scripts stopped: the TV's camera follows him from where they left the view"
                        );
                    }
                    CameraScripts::Stopped(why) | CameraScripts::Idle(why)
                        if !matches!(follow.hold, ViewHold::HandedBack(_)) =>
                    {
                        follow.hold = ViewHold::Chain(why.clone());
                    }
                    _ => {}
                }
                let Some(hero) = hero else {
                    continue;
                };
                let FollowCamera { camera, planes, .. } = &mut *follow;
                camera.step(hero.before, step, planes);
                follow.drawn = camera.view();
                viewed(&follow.drawn)
            }
        };
        *transform = place;
        // The far clip moves through a plane's blend and a switch.
        if let Some(mut drawn) = drawn {
            let want = projection(&follow.drawn);
            let same = matches!(&*drawn, Projection::Perspective(p)
                if (p.fov, p.near, p.far) == (want.fov, want.near, want.far));
            if !same {
                *drawn = Projection::Perspective(want);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use bevy::ecs::system::SystemState;
    use bevy::time::TimeUpdateStrategy;
    use guac_motion::camera::{CameraDef, LookFrom};
    use guac_motion::parry::ParryWorld;
    use guac_motion::scenarios::{self, eruptor, stick};
    use guac_motion::seam::{Body, BodyKind, Collider, Pose, Shape, Vec3};

    use super::*;
    use crate::physics::{SeamBody, SeamPlugin, set_pose};
    use crate::test_support::physics::spawn_body;
    use avian3d::prelude::{Position, Rotation};

    /// Fixed steps run, counted in `FixedUpdate`'s [`frame::Frame::Record`].
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
        headless_with(frame, bodies, |_| {})
    }

    /// [`headless`], with `more` added before the app is built.
    fn headless_with(frame: f64, bodies: &[Body], more: impl FnOnce(&mut App)) -> App {
        let mut app = App::new();
        app.add_plugins((MinimalPlugins, SeamPlugin, HeroPlugin));
        more(&mut app);
        app.insert_resource(TimeUpdateStrategy::ManualDuration(Duration::from_secs_f64(
            frame,
        )))
        .init_resource::<Steps>()
        .insert_resource(WalkScript)
        .add_systems(
            FixedUpdate,
            (
                script
                    .run_if(resource_exists::<WalkScript>)
                    .in_set(frame::Frame::Pads),
                count.in_set(frame::Frame::Record),
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
    /// AH.49: while his scripts pick his clips (his gaits' `picked`), the
    /// stand-in isn't stepped, one side picking: its weights, clips and step
    /// count stay where they were as he walks; his gaits' own step count runs
    /// on. Without picks it steps as before.
    #[test]
    fn a_hero_whose_scripts_pick_his_clips_steps_no_stand_in() {
        use bevy::ecs::system::RunSystemOnce;
        use guac_motion::locomotion::{Gaits, Loops};
        let mut app = App::new();
        app.add_plugins(MinimalPlugins);
        let mut hero = Hero::new(
            guac_motion::tuning::eruptor_solid(),
            guac_motion::tuning::Tuning::ERUPTOR,
            Vec3::ZERO,
            0.0,
        );
        hero.gait_speed = 21.5;
        let loops = Loops {
            idle: 4.0 / 3.0,
            walk: Some(1.0667),
            run: Some(0.6667),
            fall: None,
        };
        let mut gaits = HeroGaits::new(Locomotion::new(Gaits::ERUPTOR, loops));
        gaits.picked = Some(guacamole::picks::Picked::default());
        let e = app
            .world_mut()
            .spawn((
                HeroBody {
                    hero,
                    exclude: Vec::new(),
                    before: Vec3::ZERO,
                },
                gaits,
            ))
            .id();
        let stood = app.world().get::<HeroGaits>(e).unwrap().locomotion.clone();
        for _ in 0..30 {
            app.world_mut().run_system_once(step_gaits).unwrap();
        }
        let g = app.world().get::<HeroGaits>(e).unwrap();
        assert_eq!(g.locomotion, stood, "the stand-in stands");
        assert_eq!(g.steps, 30);
        app.world_mut().get_mut::<HeroGaits>(e).unwrap().picked = None;
        for _ in 0..10 {
            app.world_mut().run_system_once(step_gaits).unwrap();
        }
        let g = app.world().get::<HeroGaits>(e).unwrap();
        assert_eq!((g.locomotion.steps(), g.steps), (10, 40));
        assert!(g.locomotion.moving(), "picking for him again");
    }

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

    /// AG.25: the gate's door and a gourd, built as the props build them
    /// off their placements, stop him on Avian's side of the seam where they
    /// do on parry's, step for step: head-on into the door his pivot
    /// 1.4781 m from its middle (AG.7) within the millimetre his 5 mm keep
    /// makes of its 4.1, into the gourd just past touching.
    #[test]
    fn the_door_and_a_gourd_stop_him_through_avian_as_on_parry() {
        use guac_ir::actor::{Shape as IrShape, Solid as IrSolid};
        use guac_ir::level::Placement;
        let solid = |object, at: [f32; 3], heading, shape, size: [f32; 3], turn| {
            let p = Placement {
                object,
                id: 0,
                name: String::new(),
                script: None,
                position: at,
                rotation: [0.0, heading, 0.0],
                scale: [100.0; 3],
                model: None,
                solid: None,
                movement: None,
                waypoints: None,
                activity_state: 4,
                activation_range: 160.0,
            };
            let info = IrSolid {
                shape,
                solid: true,
                world: true,
                actor_surface: false,
                offset: [0.0; 3],
                rotation: [0.0, turn, 0.0],
                width: size[0],
                height: size[1],
                length: size[2],
                weight: 1.0,
                rebound: 0.0,
            };
            guacamole::props::solid(&p, &info).unwrap()
        };
        let mut bodies = open_ground();
        bodies.push(solid(
            1,
            [0.0, 0.0, 5.0],
            180.0,
            IrShape::Box,
            [4.0, 21.0, 34.0],
            90.0,
        ));
        bodies.push(solid(
            2,
            [-8.0, 0.0, 5.0],
            0.0,
            IrShape::Sphere,
            [7.25, 6.5, 3.0],
            0.0,
        ));
        let parry = ParryWorld::new(&bodies).unwrap();
        let input = HeroInput(Input::stick(stick(1.0, -15.0), 15.0));
        for (start, check) in [
            (Vec3::new(2.0, 1.0, 0.0), 0),
            (Vec3::new(-8.0, 1.0, 0.0), 1),
        ] {
            let mut app = headless(1.0 / 60.0, &bodies);
            app.world_mut().remove_resource::<WalkScript>();
            app.insert_resource(input);
            let entity = spawn_eruptor(&mut app, start, 0.0);
            let mut by_hand = eruptor(&parry, start, 0.0);
            let mut worst = 0.0f32;
            for k in 0..90 {
                app.update();
                assert_eq!(steps(&app), k + 1);
                by_hand.step(&parry, &input.0, (1.0f64 / 60.0) as f32, &[]);
                worst = worst.max((hero_of(&app, entity).pivot - by_hand.pivot).length());
            }
            let hero = hero_of(&app, entity);
            eprintln!(
                "Avian against parry into prop {check}: {worst:e} m at worst, at {}",
                hero.pivot
            );
            assert!(worst < 1e-5, "{worst}");
            assert_eq!(hero.velocity, Vec3::ZERO);
            if check == 0 {
                let from_middle = 5.0 - hero.pivot.z;
                assert!((from_middle - 1.4781).abs() < 1.5e-3, "{from_middle}");
            } else {
                let centres =
                    (hero.pivot + Vec3::Y * 0.9144).distance(Vec3::new(-8.0, 1.1049, 5.0));
                let touching = 1.0549 + 0.8644;
                assert!(
                    centres > touching && centres - touching < 0.006,
                    "{centres}"
                );
            }
        }
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
            .add_systems(FixedUpdate, move_block.in_set(frame::Frame::Actors));
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

    /// The gameplay camera on `def` at rest on a hero standing at `feet`.
    fn spawn_follow(app: &mut App, def: CameraDef, feet: Vec3) -> Entity {
        let mut commands = app.world_mut().commands();
        let entity = spawn_camera(
            &mut commands,
            FollowCamera::new(GameCamera::new(def, feet), Vec::new()),
        );
        app.world_mut().flush();
        entity
    }

    fn camera_of(app: &App, entity: Entity) -> &GameCamera {
        &app.world()
            .entity(entity)
            .get::<FollowCamera>()
            .unwrap()
            .camera
    }

    /// AG.17: the gameplay camera steps once a fixed step after the hero, on
    /// his pivot as the step began, and on Avian's side its view is the one
    /// the camera stepped by hand beside the controller on parry gives, the
    /// hero's walk read against it; at 30 and 144 Hz the same after 120
    /// steps; its drawn transform the eye looking at the aim.
    #[test]
    fn the_camera_steps_beside_the_hero_as_on_parry() {
        let bodies = open_ground();
        let dt = (1.0f64 / 60.0) as f32;
        let parry = ParryWorld::new(&bodies).unwrap();
        let mut by_hand = eruptor(&parry, START, 15.0);
        let mut follow = GameCamera::new(CameraDef::LEVEL_213_START, by_hand.pivot);
        for k in 0..120 {
            let mut input = walk_input(k).0;
            input.view_heading = follow.view().heading();
            let feet = by_hand.pivot;
            by_hand.step(&parry, &input, dt, &[]);
            follow.step(feet, dt, &[]);
        }
        let mut ends = Vec::new();
        for rate in [30.0, 60.0, 144.0] {
            let mut app = headless(1.0 / rate, &bodies);
            let hero = spawn_eruptor(&mut app, START, 15.0);
            let pivot = hero_of(&app, hero).pivot;
            let camera = spawn_follow(&mut app, CameraDef::LEVEL_213_START, pivot);
            walk_steps(&mut app, 120);
            let view = camera_of(&app, camera).view();
            ends.push(view.eye);
            let miss = (view.eye - follow.view().eye).length();
            eprintln!("{rate} Hz: the camera's eye {miss:e} m from parry's by hand");
            assert!(miss < 1e-4, "{rate} Hz: {miss}");
            assert!((hero_of(&app, hero).pivot - by_hand.pivot).length() < 1e-4);
            // Drawn eased between the last two steps: within one step's move
            // of the eye (the eye moves at most 0.11 m a step here), looking
            // the way the view does.
            let drawn = app.world().entity(camera).get::<Transform>().unwrap();
            assert!((drawn.translation - view.eye).length() < 0.15, "{rate} Hz");
            let forward = drawn.forward();
            let want = (view.aim - view.eye).normalize();
            assert!(forward.dot(want) > 1.0 - 1e-4, "{forward:?} {want}");
        }
        assert_eq!(ends[0], ends[1]);
        assert_eq!(ends[1], ends[2]);
    }

    /// The stick walks him against the camera's view, not the pad input's:
    /// a camera looking along 195° sends him along 195° with the stick
    /// forward (gameplay-camera's Forward after a switch, decision 12).
    #[test]
    fn the_hero_walks_against_the_cameras_view() {
        let bodies = open_ground();
        let mut app = headless(1.0 / 60.0, &bodies);
        app.world_mut().remove_resource::<WalkScript>();
        app.insert_resource(HeroInput(Input::stick(stick(1.0, 0.0), 0.0)));
        let hero = spawn_eruptor(&mut app, START, 0.0);
        let pivot = hero_of(&app, hero).pivot;
        let def = CameraDef {
            look_from: LookFrom::Heading(15.0),
            ..CameraDef::LEVEL_213_START
        };
        spawn_follow(&mut app, def, pivot);
        walk_steps(&mut app, 30);
        let from = hero_of(&app, hero).pivot;
        walk_steps(&mut app, 60);
        let moved = hero_of(&app, hero).pivot - from;
        let heading = moved.x.atan2(moved.z).to_degrees().rem_euclid(360.0);
        assert!((heading - 195.0).abs() < 1e-2, "{heading}");
    }

    /// AG.20 through AP's router: the first pad connected walks him by its
    /// raw stick; Start stops everything under the overlay (no step runs and
    /// his drawn transform holds, the stick moving nothing) and the bottom
    /// button carries on from where he stopped, at the speed he had; that
    /// pad switched off stops him the same way, another's stick moves no
    /// one, and its press takes him over.
    #[test]
    fn a_pad_walks_him_and_start_or_losing_it_stops_everything() {
        use bevy::input::gamepad::GamepadButton;

        use crate::pad::{LevelPads, Pause};
        use crate::test_support::test_pad;

        let bodies = open_ground();
        let mut app = headless_with(1.0 / 60.0, &bodies, |app| {
            app.add_plugins((bevy::input::InputPlugin, crate::input::InputRouter));
        });
        app.world_mut().remove_resource::<WalkScript>();
        let hero = spawn_eruptor(&mut app, START, 15.0);
        let first = test_pad::connect(&mut app, None, None);
        app.update();
        app.world_mut().resource_mut::<LevelPads>().show();
        test_pad::stick(&mut app, first, 0.0, 1.0);
        let pivot = |app: &App| hero_of(app, hero).pivot;
        let drawn = |app: &App| *app.world().entity(hero).get::<Transform>().unwrap();
        let pause = |app: &App| {
            app.world()
                .resource::<LevelPads>()
                .seat()
                .and_then(|s| s.pause())
        };
        let from = pivot(&app);
        walk_steps(&mut app, 40);
        let walked = pivot(&app) - from;
        assert!(walked.length() > 3.0, "{walked}");
        // Forward along the view; with no camera the input's, the walk
        // script's 15°.
        let heading = walked.x.atan2(walked.z).to_degrees();
        assert!((heading - 15.0).abs() < 1e-2, "{heading}");
        let speed = hero_of(&app, hero).velocity.length();

        // Start: the frame it's read runs the steps already due, then none.
        test_pad::press(&mut app, first, GamepadButton::Start);
        assert_eq!(pause(&app), Some(Pause::Start { by: first }));
        app.update();
        assert!(app.world().resource::<Time<Virtual>>().is_paused());
        let (held, shown, n) = (pivot(&app), drawn(&app), steps(&app));
        test_pad::stick(&mut app, first, 1.0, 0.0);
        for _ in 0..60 {
            app.update();
        }
        assert_eq!((pivot(&app), drawn(&app), steps(&app)), (held, shown, n));

        // The bottom button: on from where he stopped, at the speed he had.
        test_pad::stick(&mut app, first, 0.0, 1.0);
        test_pad::press(&mut app, first, GamepadButton::South);
        assert_eq!(pause(&app), None);
        walk_steps(&mut app, n + 1);
        assert_eq!(steps(&app), n + 1);
        let after = hero_of(&app, hero).velocity.length();
        assert!((after - speed).abs() < 1e-3, "{speed} then {after}");
        assert!(pivot(&app).distance(held) < 0.12, "one step on");

        // Switched off: everything stops until another pad's press.
        test_pad::disconnect(&mut app, first);
        app.update();
        app.update();
        assert_eq!(pause(&app), Some(Pause::NoPad));
        let (held, n) = (pivot(&app), steps(&app));
        let second = test_pad::connect(&mut app, None, None);
        app.update();
        test_pad::stick(&mut app, second, -1.0, 0.0);
        for _ in 0..30 {
            app.update();
        }
        assert_eq!(
            (pivot(&app), steps(&app)),
            (held, n),
            "a stick takes nothing over"
        );
        test_pad::press(&mut app, second, GamepadButton::East);
        assert_eq!(pause(&app), None);
        walk_steps(&mut app, n + 30);
        let turned = pivot(&app) - held;
        let heading = turned.x.atan2(turned.z).to_degrees();
        assert!((heading - 105.0).abs() < 1.0, "left, along 105°: {heading}");

        // The level gone, its pause can't hold the clock.
        test_pad::disconnect(&mut app, second);
        app.update();
        app.update();
        assert!(app.world().resource::<Time<Virtual>>().is_paused());
        app.world_mut().resource_mut::<LevelPads>().hide();
        app.update();
        assert!(!app.world().resource::<Time<Virtual>>().is_paused());
    }

    /// AH.24: a step reads each attack held now or pressed since the step
    /// before read them, so a press no step saw still reaches the next; a
    /// held one every step; one let go before any step read it once.
    #[test]
    fn a_step_reads_each_press_once_and_each_hold_throughout() {
        let mut a = HeroActions::default();
        assert_eq!(a.take(), [false; 4]);
        // Down a frame, up the next, no step between.
        a.frame([true, false, false, false], [true, false, false, false]);
        a.frame([false; 4], [false; 4]);
        assert_eq!(a.take(), [true, false, false, false]);
        assert_eq!(a.take(), [false; 4], "once");
        // Tapped inside a frame: never held.
        a.frame([false; 4], [false, true, false, false]);
        assert_eq!(a.take(), [false, true, false, false]);
        // Held across steps, two steps in one frame included.
        a.frame([false, false, true, false], [false, false, true, false]);
        assert_eq!(a.take(), [false, false, true, false]);
        assert_eq!(a.take(), [false, false, true, false]);
        a.frame([false, false, true, false], [false; 4]);
        assert_eq!(a.take(), [false, false, true, false]);
        a.frame([false; 4], [false; 4]);
        assert_eq!(a.take(), [false; 4], "let go");
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
        assert_eq!(ran, 17);
        assert!(failures.is_empty(), "{}", failures.join("\n"));
    }

    /// AG.48: the overhang's open edge on Avian's side of the seam, headless:
    /// the two open-edge scenarios guac-motion runs on parry.
    #[test]
    fn the_open_edge_cases_through_avian() {
        let mut failures = Vec::new();
        let mut ran = 0;
        for scenario in scenarios::open_edge_scenarios() {
            let mut app = headless(1.0 / 60.0, &scenario.bodies);
            let mut state = SystemState::<AvianSeam>::new(app.world_mut());
            let seam = state.get(app.world()).unwrap();
            if let Err(e) = (scenario.check)(&seam) {
                failures.push(format!("{}: {e}", scenario.name));
            }
            ran += 1;
        }
        assert_eq!(ran, 2);
        assert!(failures.is_empty(), "{}", failures.join("\n"));
    }

    /// A key from the environment, else from the repo's `.env.dev`.
    fn env_or_dotenv(key: &str) -> Option<String> {
        if let Ok(v) = std::env::var(key) {
            return Some(v);
        }
        let dotenv = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../.env.dev");
        let text = std::fs::read_to_string(dotenv).ok()?;
        text.lines().find_map(|line| {
            let (k, v) = line.trim().split_once('=')?;
            (k.trim() == key).then(|| v.trim().trim_matches('"').to_owned())
        })
    }

    /// Level 213's collision mesh read off the dump through the extractor's
    /// own readers, in memory (no extract written), as a seam body.
    fn level_213_mesh(dump: &std::path::Path) -> (Body, std::sync::Arc<guac_motion::seam::Mesh>) {
        use guac_extract::levels;
        use guac_formats::igz::Igz;
        use guac_ir::{DumpIdentity, Sha256};
        let identity = DumpIdentity {
            title_id: "BLUS30968".into(),
            app_ver: "01.00".into(),
            manifest_hash: Sha256([0; 32]),
        };
        let level = levels::LEVELS[0];
        let usrdir = dump.join("PS3_GAME/USRDIR");
        let archive = std::fs::read(levels::archive_path(&usrdir, level)).expect("the archive");
        let bld = levels::level_bld(&archive).expect("its level.bld");
        let igz = Igz::parse(&bld).expect("the level parses");
        let collision = levels::collision(&igz, level, identity)
            .expect("the collision")
            .ir;
        let m = &collision.meshes[0];
        let mesh = std::sync::Arc::new(
            guac_motion::seam::Mesh::new(
                m.vertices.iter().map(|v| Vec3::from_array(*v)).collect(),
                m.triangles.iter().map(|t| t.map(u32::from)).collect(),
                m.materials.clone(),
            )
            .unwrap(),
        );
        let body = Body {
            id: BodyId(1),
            kind: BodyKind::Static,
            pose: Pose::IDENTITY,
            collider: Collider::Mesh(mesh.clone()),
        };
        (body, mesh)
    }

    /// One record of an O5 recording as the replay reads it.
    struct Record {
        game_frame: u64,
        step: f32,
        position: Vec3,
        rotation: [f32; 3],
        velocity: Vec3,
        view: f32,
        stick: [f32; 2],
        supported: bool,
        ground: Option<(Vec3, Vec3)>,
        first_plane: Option<f32>,
    }

    fn record(v: &serde_json::Value) -> Record {
        let f = |v: &serde_json::Value| v.as_f64().unwrap() as f32;
        let v3 = |v: &serde_json::Value| Vec3::new(f(&v[0]), f(&v[1]), f(&v[2]));
        let clock = &v["clock"];
        let rate = f(&clock["presentation_rate"]);
        let clamped = f(&clock["frame_delta"]).clamp(rate * 0.5, rate * 3.0);
        let step = if clock["logic_stopped"].as_bool().unwrap() {
            0.0
        } else {
            (clamped * f(&clock["logic_scale"])) * f(&clock["object_scale"])
        };
        let contacts = &v["contacts"];
        Record {
            game_frame: v["game_frame"].as_u64().unwrap(),
            step,
            position: v3(&v["hero"]["position"]),
            rotation: [0, 1, 2].map(|i| f(&v["hero"]["rotation"][i])),
            velocity: v3(&v["hero"]["velocity"]),
            view: f(&v["camera"]["rotation"][1]),
            stick: [f(&v["stick"][0]), f(&v["stick"][1])],
            supported: contacts["support"].as_u64() == Some(2),
            ground: contacts["ground"].is_object().then(|| {
                (
                    v3(&contacts["ground"]["location"]),
                    v3(&contacts["ground"]["normal"]),
                )
            }),
            first_plane: contacts["planes"]
                .as_array()
                .and_then(|p| p.first())
                .map(|p| f(&p["distance"])),
        }
    }

    fn footing_of(r: &Record) -> guac_motion::walk::Footing {
        use guac_motion::ground::{LEAN_SETTLED, angle};
        let up = guac_motion::solid::euler(r.rotation) * Vec3::Y;
        let normal = r.ground.map(|g| g.1);
        let settled = normal.is_some_and(|n| angle(up, n) <= LEAN_SETTLED);
        guac_motion::walk::Footing {
            supported: r.supported,
            height: r.ground.map(|g| r.position.y - g.0.y),
            slope: if settled {
                up
            } else {
                normal.unwrap_or(Vec3::Y)
            },
            ground: normal,
        }
    }

    /// Free in the air, as guac-motion's replays read it: unsupported 0.4 ft
    /// or more up, his manifold's own closest point 2 cm or more off.
    fn free(r: &Record) -> bool {
        footing_of(r).high() && r.first_plane.is_none_or(|d| d >= 0.02)
    }

    /// AG.48 on Avian's side (`#[ignore]`d: level 213's collision off
    /// `GUAC_GIANTS_DUMP`, the recordings under `GUAC_CAPTURE_DIR`, from the
    /// process env, else the repo's `.env.dev`): AG.39's `air/g39_air_rel`
    /// and `g39_air_rev`, 2 replays each, stepped from their second free
    /// record on each record's own stick, view and step through
    /// `AvianSeam`, land within a record of theirs, every frame before it
    /// moving him (no hanging on the overhang's open edge), and agree with
    /// the parry side to 1 mm at every record.
    #[test]
    #[ignore]
    fn ag48_the_air_replays_through_avian() {
        use guac_motion::tuning::{FOOT, Tuning};
        use guac_motion::walk::Walk;
        let dump = env_or_dotenv("GUAC_GIANTS_DUMP").expect("set GUAC_GIANTS_DUMP");
        let captures = env_or_dotenv("GUAC_CAPTURE_DIR").expect("set GUAC_CAPTURE_DIR");
        let (level, _mesh) = level_213_mesh(std::path::Path::new(&dump));
        let parry = ParryWorld::new(std::slice::from_ref(&level)).unwrap();
        let mut app = headless(1.0 / 60.0, std::slice::from_ref(&level));
        let mut state = SystemState::<AvianSeam>::new(app.world_mut());
        let avian = state.get(app.world()).unwrap();
        let run_dir = std::path::Path::new(&captures).join("BLUS30968/ag39-falls-1792502813/air");
        let mut seen = 0;
        for name in [
            "g39_air_rel-W-00",
            "g39_air_rel-W-01",
            "g39_air_rev-W-00",
            "g39_air_rev-W-01",
        ] {
            let text = std::fs::read_to_string(run_dir.join(name).join("recording.json")).unwrap();
            let json: serde_json::Value = serde_json::from_str(&text).unwrap();
            let frames: Vec<Record> = json.as_array().unwrap().iter().map(record).collect();
            let k = (3..frames.len())
                .find(|&k| free(&frames[k]) && free(&frames[k - 1]) && !free(&frames[k - 2]))
                .unwrap();
            let land = (k..frames.len()).find(|&j| frames[j].supported).unwrap();
            let seed = || {
                let f = &frames[k];
                let moved = (f.position - frames[k - 1].position) / frames[k - 1].step;
                let mut hero = Hero::new(
                    scenarios::eruptor_solid(),
                    Tuning::ERUPTOR,
                    f.position,
                    f.rotation[1],
                );
                hero.up = guac_motion::solid::euler(f.rotation) * Vec3::Y;
                hero.walk = Walk {
                    velocity: moved / FOOT,
                    landing: None,
                };
                hero.footing = footing_of(f);
                hero.velocity = f.velocity;
                hero.set_steps(frames[k - 1].step, frames[k - 2].step);
                hero
            };
            let (mut ours, mut twin) = (seed(), seed());
            let mut landed = None;
            let mut apart = 0.0f32;
            for j in k + 1..=land + 3 {
                let (a, b) = (&frames[j - 1], &frames[j]);
                assert_eq!(b.game_frame, a.game_frame + 1, "{name}");
                let input = Input::stick(Stick::new(b.stick[0], b.stick[1]), a.view);
                let before = ours.pivot;
                ours.step(&avian, &input, a.step, &[]);
                twin.step(&parry, &input, a.step, &[]);
                apart = apart.max((ours.pivot - twin.pivot).length());
                assert!(
                    apart < 1e-3,
                    "{name} {j}: Avian {} parry {}",
                    ours.pivot,
                    twin.pivot
                );
                if landed.is_none() {
                    if ours.footing.supported {
                        landed = Some(j);
                    } else {
                        assert!((ours.pivot - before).length() > 1e-3, "{name}: hung at {j}");
                    }
                }
            }
            eprintln!(
                "{name} from record {k}: lands on Avian at {landed:?}, theirs {land}; Avian and parry {apart:.2e} m apart at worst"
            );
            assert!(
                landed.is_some_and(|l| l.abs_diff(land) <= 1),
                "{name}: {landed:?} against {land}"
            );
            seen += 1;
        }
        assert_eq!(seen, 4);
    }
}
