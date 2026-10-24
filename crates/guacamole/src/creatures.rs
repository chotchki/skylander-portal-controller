//! The first fight's creatures on the TV (openspec AH.25, design decisions
//! 1 and 6; creatures' Chapter 1's first fight): the four Chompies and
//! Norticus run from their own scripts once the dev entry lists the fight
//! ([`crate::dev_entry`]), the engine walking them ([`ScriptWorld::walk`],
//! AH.18). This is what the TV adds around them:
//!
//! - Bodies ([`place_bodies`]): each hero's solid and each awake creature's
//!   as a kinematic body in Avian's world, on its own entity (the hero's,
//!   the placement's), where the step left it, as the headless runs keep
//!   theirs in parry's world (`Bench::place_bodies`): so the hero's sweep
//!   meets a Chompy, a Chompy's lunge stops on him, a lava ball bursts on
//!   one, and the scripts' engine maps each body to its actor
//!   ([`ScriptWorld::embodied`]). A creature asleep or taken off leaves the
//!   world at the step's end, as a hero's body does with his entity. The
//!   hero's own queries pass his body ([`HeroBody::exclude`]).
//! - Drawing ([`draw_creatures`], [`pose_creatures`]): each awake
//!   creature whose script runs, on its model off the level's props part
//!   ([`guacamole::creature_models`]: the Chompies' and Norticus'), in the
//!   level's lights as the hero is, once every pipeline it draws with is
//!   compiled (the census's creatures table holds them); at its actor's
//!   place, facing and scale after each step, eased between steps; posed
//!   by its idle with the clips its scripts play on it laid over, by the
//!   level's palettes part's slots (stand-in ledger: `creature-pose`).
//!   Taken off or asleep, it goes.
//! - The report (`/api/_test/tv`'s `creatures`): each creature's behaviour,
//!   health and place, what the fight woke past its own (design decision
//!   6, listed once each), what the scripts have spawned that's still on,
//!   and the experience orbs out (AH.20's control) with their worth.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use avian3d::prelude::{Collider, Position, RigidBody, Rotation};
use bevy::camera::visibility::RenderLayers;
use bevy::ecs::system::SystemParam;
use bevy::mesh::skinning::SkinnedMeshInverseBindposes;
use bevy::prelude::*;
use bevy_transform_interpolation::prelude::TransformInterpolation;
use guac_ir::Model;
use guac_ir::palette::Palette;
use guac_motion::seam::{Body, BodyId, BodyKind, Collider as SeamCollider, Pose};
use guac_motion::solid::HavokSolid;
use guac_script::{Handle, Value};
use guacamole::character::{
    CharacterMaterial, ShadowLookup, reference_material, remaster, spawn_character,
};
use guacamole::frame::Frame;
use guacamole::level_scene::hero_scene;
use guacamole::scripts::{ScriptRuntime, ScriptWorld};
use serde::Serialize;
use tracing::warn;

use crate::hero::HeroBody;
use crate::level::{LEVEL_LAYER, Level, LevelSystems, LitAlso};
use crate::physics::{PreparedBody, SeamBody};
use crate::warm::Warm;

/// The body ids the heroes' and the creatures' solids take: this plus a
/// count, clear of the level's meshes' and the props' ids
/// ([`guacamole::props::PROP_BODIES`] up, by object index).
pub(crate) const ACTOR_BODIES: u32 = 1 << 21;

/// The bodies the TV keeps for actors: each one's id, and the solid it was
/// made with (a script changing a creature's solid makes it again).
#[derive(Resource, Debug, Default)]
pub(crate) struct Embodied {
    ids: HashMap<Handle, BodyId>,
    solids: HashMap<Handle, HavokSolid>,
}

impl Embodied {
    fn id(&mut self, h: Handle) -> BodyId {
        let next = BodyId(ACTOR_BODIES + self.ids.len() as u32);
        *self.ids.entry(h).or_insert(next)
    }
}

/// Each hero's and each awake creature's body where the step left it (the
/// module docs), the scripts' engine told which body is whose.
pub(crate) fn place_bodies(
    mut commands: Commands,
    world: Option<ResMut<ScriptWorld>>,
    mut heroes: Query<(Entity, &mut HeroBody)>,
    mut poses: Query<(&SeamBody, &mut Position, &mut Rotation)>,
    mut kept: ResMut<Embodied>,
    entities: Query<(), With<Transform>>,
) {
    let mut wanted: Vec<(Handle, HavokSolid, Pose)> = Vec::new();
    for (e, mut body) in &mut heroes {
        let h = e.to_bits();
        let solid = *body.hero.solid();
        let pose = solid.pose(Pose::new(body.hero.pivot, body.hero.rotation()));
        let id = kept.id(h);
        if !body.exclude.contains(&id) {
            body.exclude.push(id);
        }
        wanted.push((h, solid, pose));
    }
    if let Some(world) = world.as_deref() {
        wanted.extend(world.walkers());
    }
    let mut placed: Vec<(Handle, BodyId)> = Vec::new();
    for (h, solid, pose) in wanted {
        let Some(e) = Entity::try_from_bits(h) else {
            continue;
        };
        let id = kept.id(h);
        let moved = kept.solids.get(&h) == Some(&solid)
            && poses.get_mut(e).is_ok_and(|(b, mut p, mut r)| {
                if b.id != id {
                    return false;
                }
                if p.0 != pose.position {
                    p.0 = pose.position;
                }
                if r.0 != pose.rotation {
                    r.0 = pose.rotation;
                }
                true
            });
        if !moved {
            let body = Body {
                id,
                kind: BodyKind::Kinematic,
                pose,
                collider: SeamCollider::Convex(solid.in_world()),
            };
            match PreparedBody::new(&body) {
                Ok(prepared) => {
                    let Ok(mut entity) = commands.get_entity(e) else {
                        continue;
                    };
                    // Avian reads a collider's scale off its entity's
                    // transform: a placement's has none, so an unscaled one.
                    if entities.get(e).is_err() {
                        entity.insert(Transform::default());
                    }
                    prepared.insert_on(&mut entity);
                    kept.solids.insert(h, solid);
                }
                Err(why) => {
                    warn!(%why, "an actor's solid can't be a body: it stays out of the world");
                    continue;
                }
            }
        }
        placed.push((h, id));
    }
    // What has no body now (asleep, taken off, gone) leaves the world.
    let gone: Vec<Handle> = kept
        .solids
        .keys()
        .filter(|h| !placed.iter().any(|(p, _)| p == *h))
        .copied()
        .collect();
    for h in gone {
        kept.solids.remove(&h);
        if let Some(e) = Entity::try_from_bits(h)
            && let Ok(mut entity) = commands.get_entity(e)
        {
            entity.try_remove::<(SeamBody, RigidBody, Collider, Position, Rotation)>();
        }
    }
    placed.sort_unstable();
    if let Some(mut world) = world
        && world.embodied != placed
    {
        world.embodied = placed;
    }
}

/// The creatures' plugin: [`place_bodies`] at the step's end, after the
/// hands are posed, as the headless runs place theirs, and each drawn
/// creature placed there too; drawing and posing them once the frame's
/// steps have run.
pub(crate) struct CreaturesPlugin;

impl Plugin for CreaturesPlugin {
    fn build(&self, app: &mut App) {
        guacamole::frame::ensure(app);
        app.init_resource::<Embodied>()
            .init_resource::<DrawnCreatures>()
            .add_systems(
                FixedUpdate,
                (place_bodies, place_creatures)
                    .chain()
                    .in_set(Frame::Hands)
                    .after(crate::hero_scripts::pose_hands),
            )
            .add_systems(
                Update,
                (draw_creatures, pose_creatures)
                    .chain()
                    .after(LevelSystems)
                    .run_if(resource_exists::<ScriptWorld>),
            );
    }
}

/// A creature drawn: its actor, its model and palette, its joints and the
/// materials the level lights for it.
#[derive(Component, Debug)]
pub(crate) struct DrawnCreature {
    pub(crate) actor: Handle,
    model: Arc<Model>,
    palette: Option<Arc<Palette>>,
    joints: Vec<Entity>,
    lit: Vec<bevy::asset::Handle<CharacterMaterial>>,
    /// The TV's clock when it was drawn, for its idle.
    since: Duration,
}

/// Each drawn creature's root, by its actor.
#[derive(Resource, Debug, Default)]
pub(crate) struct DrawnCreatures(pub(crate) HashMap<Handle, Entity>);

/// Where a creature's actor stands, as its drawn root takes it: its pivot,
/// its facing turned about the vertical, its scale.
fn placed(a: &guacamole::script_engine::Actor) -> Transform {
    Transform {
        translation: a.position,
        rotation: Quat::from_rotation_y(a.angles.facing.to_radians()),
        scale: Vec3::from_array(a.scale) / 100.0,
    }
}

/// Each drawn creature at its actor's place as the step left it, eased
/// between steps.
pub(crate) fn place_creatures(
    world: Option<Res<ScriptWorld>>,
    mut drawn: Query<(&DrawnCreature, &mut Transform)>,
) {
    let Some(world) = world else {
        return;
    };
    for (c, mut t) in &mut drawn {
        if let Some(a) = world.actor(c.actor) {
            let want = placed(a);
            if *t != want {
                *t = want;
            }
        }
    }
}

/// What [`draw_creatures`] spawns with.
#[derive(SystemParam)]
pub(crate) struct CreatureSpawner<'w, 's> {
    commands: Commands<'w, 's>,
    meshes: ResMut<'w, Assets<Mesh>>,
    bindposes: ResMut<'w, Assets<SkinnedMeshInverseBindposes>>,
    materials: ResMut<'w, Assets<CharacterMaterial>>,
    time: Res<'w, Time>,
}

/// Each awake creature whose script runs drawn on its model, once its
/// keys are compiled (asked for if they aren't); each taken off, asleep or
/// gone from the run let go (the module docs).
fn draw_creatures(
    world: Res<ScriptWorld>,
    runtime: Option<Res<ScriptRuntime>>,
    mut level: ResMut<Level>,
    mut warm: ResMut<Warm>,
    mut drawn: ResMut<DrawnCreatures>,
    roots: Query<&DrawnCreature>,
    mut s: CreatureSpawner,
) {
    let Some(runtime) = runtime else {
        return;
    };
    let vm = &runtime.vm;
    let inert = vm.inert();
    let wanted: Vec<(Handle, String)> = world
        .creatures()
        .into_iter()
        .filter(|h| vm.placement_of(*h).is_some() && !inert.contains(h))
        .filter_map(|h| {
            let a = world.actor(h)?;
            if a.removed || !a.active {
                return None;
            }
            let path = a.obj.and_then(|o| world.template(o))?.model.clone()?;
            Some((h, path))
        })
        .collect();
    // Let go of what's no longer wanted.
    let gone: Vec<Handle> = drawn
        .0
        .keys()
        .filter(|h| !wanted.iter().any(|(w, _)| w == *h))
        .copied()
        .collect();
    for h in gone {
        if let Some(root) = drawn.0.remove(&h) {
            if let Ok(c) = roots.get(root) {
                level.forget(&c.lit);
            }
            s.commands.entity(root).despawn();
        }
    }
    let Some((lights, fog)) = level.lighting().map(|(l, f)| (l.clone(), f)) else {
        return;
    };
    let scene = hero_scene(&lights, &fog, Mat4::IDENTITY);
    for (h, path) in wanted {
        if drawn.0.contains_key(&h) {
            continue;
        }
        let Some((creature, images)) = level.creature(&path) else {
            continue;
        };
        let (model, palette) = (creature.model.clone(), creature.palette.clone());
        let keys = match guacamole::character::warm::level_keys(&model, &crate::play::key_scene()) {
            Ok(k) => k,
            Err(why) => {
                warn!(%path, %why, "a creature's model has no keys: it isn't drawn");
                continue;
            }
        };
        if !warm.covers(keys.iter().map(|(k, _)| k)) {
            warm.ask_hero(&model, &keys, Arc::new(crate::play::key_scene()));
            continue;
        }
        let texture = |t: &guac_ir::model::TextureSlot, m: &guac_ir::model::Material| {
            let key = (
                t.texture.clone(),
                remaster::samples_srgb(m, t.role),
                t.sampler,
            );
            images
                .iter()
                .find(|(k, _)| *k == key)
                .map(|(_, h)| h.clone())
                .unwrap_or_default()
        };
        let mut colour = Vec::with_capacity(model.materials.len());
        let mut lit = Vec::with_capacity(model.materials.len());
        let mut failed = None;
        for m in &model.materials {
            let made = CharacterMaterial::from_ir(m, &scene, ShadowLookup::NONE, |t| texture(t, m))
                .map_err(|e| e.to_string())
                .and_then(|c| {
                    reference_material(m)
                        .map(|r| (c, r))
                        .map_err(|e| e.to_string())
                });
            match made {
                Ok((material, reference)) => {
                    let handle = s.materials.add(material);
                    lit.push(LitAlso {
                        material: handle.clone(),
                        reference,
                        alpha: m.states.alpha_test.unwrap_or(0.0),
                    });
                    colour.push(handle);
                }
                Err(why) => {
                    failed = Some(format!("material {}: {why}", m.name));
                    break;
                }
            }
        }
        if let Some(why) = failed {
            warn!(%path, %why, "a creature's material didn't build: it isn't drawn");
            continue;
        }
        let at = world.actor(h).map(placed).unwrap_or_default();
        let body = match spawn_character(
            &mut s.commands,
            &model,
            Transform::IDENTITY,
            0.0,
            &mut s.meshes,
            &mut s.bindposes,
            &colour,
        ) {
            Ok(b) => b,
            Err(why) => {
                warn!(%path, %why, "a creature's model didn't spawn: it isn't drawn");
                continue;
            }
        };
        let layer = RenderLayers::layer(LEVEL_LAYER);
        for e in &body.segments {
            s.commands.entity(*e).insert(layer.clone());
        }
        let handles: Vec<_> = lit.iter().map(|l| l.material.clone()).collect();
        level.light_also(lit, &mut s.materials);
        let root = s
            .commands
            .spawn((
                Name::new(format!("creature {}", world.name(h))),
                at,
                TransformInterpolation,
                Visibility::default(),
                DrawnCreature {
                    actor: h,
                    model,
                    palette,
                    joints: body.joints,
                    lit: handles,
                    since: s.time.elapsed(),
                },
            ))
            .id();
        s.commands.entity(body.root).insert(ChildOf(root));
        drawn.0.insert(h, root);
    }
}

/// Each drawn creature posed: its idle on the TV's clock since it was
/// drawn, with the clips its scripts play on it laid over, oldest first,
/// each at its time out of its palette.
fn pose_creatures(
    world: Res<ScriptWorld>,
    time: Res<Time>,
    creatures: Query<&DrawnCreature>,
    mut transforms: Query<&mut Transform, Without<DrawnCreature>>,
) {
    for c in &creatures {
        let seconds = time.elapsed().saturating_sub(c.since).as_secs_f64();
        let over: Vec<(&guac_ir::clip::Clip, f64)> = match &c.palette {
            Some(p) => crate::hero_scripts::playing_on(&world, c.actor)
                .into_iter()
                .filter_map(|(slot, at, _)| {
                    Some((p.clip(u16::try_from(slot).ok()?)?, f64::from(at)))
                })
                .collect(),
            None => Vec::new(),
        };
        let pose = guacamole::character::mesh::blended_over(
            &c.model,
            &[(&c.model.idle, seconds, 1.0)],
            &over,
        );
        for (joint, p) in c.joints.iter().zip(pose) {
            if let Ok(mut t) = transforms.get_mut(*joint)
                && *t != p
            {
                *t = p;
            }
        }
    }
}

/// A creature as `/api/_test/tv` reports it.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub(crate) struct CreatureShot {
    pub name: String,
    /// The behaviour its script is in; none before it starts.
    pub behaviour: Option<String>,
    /// `description.creature info.hp` as its scripts hold it (a synthetic
    /// creature's own `health` where it has no description).
    pub health: Option<f32>,
    /// Its pivot, metres, and its facing, degrees.
    pub position: [f32; 3],
    pub facing: f32,
    /// Awake by the activity rule, and taken off.
    pub awake: bool,
    pub gone: bool,
    /// Its body in Avian's world, while it has one.
    pub body: bool,
}

/// What the scripts spawned that's still on: its template's name and where.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub(crate) struct SpawnShot {
    pub name: String,
    pub at: [f32; 3],
}

/// A placement off the whitelist a whitelisted script woke (design
/// decision 6): it stays inert.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub(crate) struct WokenShot {
    pub name: String,
    pub by: String,
}

/// An experience orb out in the level: where, its worth, whether it's
/// still coming down.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub(crate) struct OrbShot {
    pub at: [f32; 3],
    pub worth: i32,
    pub falling: bool,
}

/// The creatures' part of `/api/_test/tv`.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub(crate) struct CreaturesReport {
    pub creatures: Vec<CreatureShot>,
    pub woken: Vec<WokenShot>,
    pub spawned: Vec<SpawnShot>,
    /// The orbs out (openspec AH.20's control), for a driver to walk to.
    pub orbs: Vec<OrbShot>,
}

/// The creatures' part of `/api/_test/tv` (openspec AQ.8).
#[derive(SystemParam)]
pub(crate) struct CreatureShots<'w> {
    runtime: Option<Res<'w, ScriptRuntime>>,
    world: Option<Res<'w, ScriptWorld>>,
}

impl CreatureShots<'_> {
    /// None until the run is made.
    pub(crate) fn shot(&self) -> Option<CreaturesReport> {
        let (rt, world) = (self.runtime.as_deref()?, self.world.as_deref()?);
        Some(report(&rt.vm, world))
    }
}

/// Every creature the engine walks (taken off ones too), the placements
/// the fight woke past its own, and each clone still on, off `vm` and
/// `world` as the step left them.
pub(crate) fn report(vm: &guac_script::Vm, world: &ScriptWorld) -> CreaturesReport {
    let creatures = world
        .creatures()
        .into_iter()
        .filter_map(|h| {
            let a = world.actor(h)?;
            let id = vm.placement_of(h);
            Some(CreatureShot {
                name: world.name(h),
                behaviour: id.and_then(|id| vm.behaviour(id)).map(str::to_owned),
                health: id.and_then(|id| health(vm, id)),
                position: a.position.to_array(),
                facing: a.angles.facing.rem_euclid(360.0),
                awake: a.active,
                gone: a.removed,
                body: world.embodied.iter().any(|(e, _)| *e == h),
            })
        })
        .collect();
    let woken = world
        .woken
        .iter()
        .map(|w| WokenShot {
            name: w.name.clone(),
            by: w.by.clone(),
        })
        .collect();
    let spawned = world
        .clones()
        .into_iter()
        .filter(|&c| !world.removed(c))
        .filter_map(|c| {
            Some(SpawnShot {
                name: world.name(c),
                at: world.actor(c)?.position.to_array(),
            })
        })
        .collect();
    let orbs = guacamole::scripts::orbs(vm, world)
        .into_iter()
        .map(|o| OrbShot {
            at: o.at.to_array(),
            worth: o.worth,
            falling: o.falling,
        })
        .collect();
    CreaturesReport {
        creatures,
        woken,
        spawned,
        orbs,
    }
}

/// A creature's health as its scripts hold it: `description.creature
/// info.hp`, else a local `health` (a synthetic creature's).
fn health(vm: &guac_script::Vm, id: guac_script::PlacementId) -> Option<f32> {
    let number = |v: Value| match v {
        Value::Int(i) => Some(i as f32),
        Value::Float(f) => Some(f),
        _ => None,
    };
    let described = vm
        .local(id, "description")
        .map(|v| vm.value(v))
        .and_then(|d| vm.field(d, "creature info").map(|v| vm.value(v)))
        .and_then(|c| vm.field(c, "hp").map(|v| vm.value(v)))
        .and_then(number);
    described.or_else(|| vm.local(id, "health").map(|v| vm.value(v)).and_then(number))
}

#[cfg(test)]
mod tests {
    use bevy::input::gamepad::GamepadButton;
    use guac_formats::script::builder::{
        ATTACK_BALL, FIGHT_BATTLE, FIGHT_CREATURE, FIGHT_HEALTH, FIGHT_XP,
    };
    use guacamole::scripts::ScriptWorld;

    use super::*;
    use crate::dev_entry::{Anchor, Asked, Plan};
    use crate::hero_scripts::ScriptSource;
    use crate::test_support::play::{CREATURE_AT, fight_app, until};
    use crate::test_support::test_pad;

    fn start() -> Asked {
        Asked {
            plan: Plan::of(Anchor::Start),
            seed: None,
        }
    }

    fn creature(app: &App) -> Handle {
        let w = app.world();
        let level = w.resource::<ScriptSource>().level_unit().unwrap();
        w.resource::<ScriptWorld>()
            .placed_in(level, FIGHT_CREATURE)
            .expect("the creature's placement")
    }

    fn shot(app: &App) -> CreaturesReport {
        let w = app.world();
        report(
            &w.resource::<ScriptRuntime>().vm,
            w.resource::<ScriptWorld>(),
        )
    }

    /// The seam body on `h`'s entity, if it has one.
    fn body(app: &mut App, h: Handle) -> Option<BodyId> {
        let e = Entity::try_from_bits(h).unwrap();
        app.world_mut()
            .query::<&SeamBody>()
            .get(app.world(), e)
            .ok()
            .map(|b| b.id)
    }

    fn hero_entity(app: &App) -> Entity {
        app.world().resource::<crate::play::Play>().hero().unwrap()
    }

    /// AH.25: with the entry asked, the synthetic encounter's creature (a
    /// first fight's placement) runs from its script on the TV, `idle`
    /// until its 30th tick and `battle` after, its health its script's; the
    /// engine walks it, standing on the floor where it was placed; it and
    /// the hero are bodies in Avian's world on their own entities, the
    /// scripts' engine maps each body to its actor, and the hero's own
    /// queries pass his.
    #[test]
    fn the_encounters_creature_runs_walks_and_is_a_body_with_the_entry() {
        let tmp = tempfile::tempdir().unwrap();
        let (mut app, _) = fight_app(tmp.path(), Some(start()));
        let c = creature(&app);
        let ticked = |app: &App| {
            let vm = &app.world().resource::<ScriptRuntime>().vm;
            vm.placement_of(c)
                .and_then(|id| vm.behaviour(id))
                .map(str::to_owned)
        };
        until(&mut app, "the creature in battle", |a| {
            ticked(a).as_deref() == Some("battle")
        });
        assert!(app.world().resource::<ScriptRuntime>().steps >= FIGHT_BATTLE as u64);
        let report = shot(&app);
        let mine: Vec<_> = report
            .creatures
            .iter()
            .filter(|s| s.name == FIGHT_CREATURE)
            .collect();
        assert_eq!(mine.len(), 1, "{report:?}");
        let s = mine[0];
        assert_eq!(s.behaviour.as_deref(), Some("battle"));
        assert_eq!(s.health, Some(FIGHT_HEALTH as f32));
        assert!(s.awake && !s.gone && s.body, "{s:?}");
        let at = Vec3::from_array(s.position);
        assert!(
            at.with_y(0.0)
                .distance(Vec3::from_array(CREATURE_AT).with_y(0.0))
                < 0.01
        );
        assert!((at.y - CREATURE_AT[1]).abs() < 0.1, "on the floor: {at}");
        let creature_body = body(&mut app, c).expect("its body");
        let hero = hero_entity(&app);
        let hero_body = body(&mut app, hero.to_bits()).expect("his body");
        assert!(creature_body.0 >= ACTOR_BODIES && hero_body.0 >= ACTOR_BODIES);
        assert_ne!(creature_body, hero_body);
        let embodied = app.world().resource::<ScriptWorld>().embodied.clone();
        assert!(embodied.contains(&(c, creature_body)), "{embodied:?}");
        assert!(
            embodied.contains(&(hero.to_bits(), hero_body)),
            "{embodied:?}"
        );
        assert!(
            app.world()
                .entity(hero)
                .get::<HeroBody>()
                .unwrap()
                .exclude
                .contains(&hero_body),
            "his own sweeps pass his body"
        );
    }

    /// Without an entry the TV's listing leaves the first fight's
    /// placements inert (AH.27's): the creature never ticks, no engine
    /// walks it and it has no body; the hero's is still there.
    #[test]
    fn without_the_entry_the_creature_stays_inert() {
        let tmp = tempfile::tempdir().unwrap();
        let (mut app, _) = fight_app(tmp.path(), None);
        for _ in 0..40 {
            app.update();
        }
        let c = creature(&app);
        let w = app.world();
        let vm = &w.resource::<ScriptRuntime>().vm;
        assert!(vm.inert().contains(&c));
        assert_eq!(vm.placement_of(c).and_then(|id| vm.behaviour(id)), None);
        assert!(w.resource::<ScriptWorld>().creatures().is_empty());
        assert!(shot(&app).creatures.is_empty());
        assert_eq!(body(&mut app, c), None);
        let hero = hero_entity(&app).to_bits();
        assert!(body(&mut app, hero).is_some());
    }

    /// AH.25: the awake creature is drawn on its model off the level's
    /// props part in the level's layer, at its actor's place, posed by its
    /// reference pose until its script starts its battle slot, then by
    /// that clip out of the level's palettes part; beaten, it goes.
    #[test]
    fn an_awake_creature_is_drawn_and_posed_by_the_clips_its_scripts_play() {
        use crate::test_support::play::BATTLE_TOP;
        let tmp = tempfile::tempdir().unwrap();
        let (mut app, pad) = fight_app(tmp.path(), Some(start()));
        let c = creature(&app);
        until(&mut app, "the creature drawn", |a| {
            a.world().resource::<DrawnCreatures>().0.contains_key(&c)
        });
        let root = app.world().resource::<DrawnCreatures>().0[&c];
        let joints = app
            .world()
            .get::<DrawnCreature>(root)
            .unwrap()
            .joints
            .clone();
        assert_eq!(joints.len(), 2);
        let top = |app: &App| *app.world().get::<Transform>(joints[1]).unwrap();
        app.update();
        let at = app.world().get::<Transform>(root).unwrap().translation;
        let actor = app
            .world()
            .resource::<ScriptWorld>()
            .actor(c)
            .unwrap()
            .position;
        assert!(at.distance(actor) < 0.05, "{at} against {actor}");
        let segments: Vec<Entity> = app
            .world_mut()
            .query_filtered::<Entity, With<Mesh3d>>()
            .iter(app.world())
            .filter(|e| {
                app.world()
                    .get::<ChildOf>(*e)
                    .and_then(|p| app.world().get::<ChildOf>(p.parent()))
                    .is_some_and(|p| p.parent() == root)
            })
            .collect();
        assert_eq!(segments.len(), 1, "its one segment");
        assert_eq!(
            app.world().get::<RenderLayers>(segments[0]),
            Some(&RenderLayers::layer(LEVEL_LAYER))
        );
        let before = top(&app);
        if ticked_to_battle(&app) {
            panic!("in battle before it was looked at");
        }
        assert!(before.translation.distance(Vec3::Y) < 1e-4, "{before:?}");
        until(&mut app, "its battle clip", |a| {
            top(a).translation.distance(Vec3::from_array(BATTLE_TOP)) < 1e-4
        });
        test_pad::press(&mut app, pad, GamepadButton::South);
        until(&mut app, "it beaten and let go", |a| {
            !a.world().resource::<DrawnCreatures>().0.contains_key(&c)
        });
        assert!(app.world().get_entity(root).is_err(), "its root despawned");
    }

    fn ticked_to_battle(app: &App) -> bool {
        let c = creature(app);
        let vm = &app.world().resource::<ScriptRuntime>().vm;
        vm.placement_of(c).and_then(|id| vm.behaviour(id)) == Some("battle")
    }

    /// AH.25 on the TV: Eruptor's synthetic throw comes down on the
    /// creature 3.2 m ahead, stopping on its body, and the creature's
    /// script (the break the ball asks for) awards its experience through
    /// `Game - Award Experience` into his figure's words, read back onto his
    /// slot; the creature and the ball are taken off, and its body leaves
    /// the world.
    #[test]
    fn a_throw_beats_the_synthetic_creature_and_its_experience_reaches_his_figure() {
        let tmp = tempfile::tempdir().unwrap();
        let (mut app, pad) = fight_app(tmp.path(), Some(start()));
        let c = creature(&app);
        assert!(body(&mut app, c).is_some());
        let xp = |app: &App| {
            app.world().resource::<crate::progress::Earning>().slots[0]
                .as_ref()
                .unwrap()
                .progress
                .xp
        };
        assert_eq!(xp(&app), 0);
        test_pad::press(&mut app, pad, GamepadButton::South);
        until(&mut app, "the creature beaten", |a| {
            a.world().resource::<ScriptWorld>().removed(c)
        });
        let world = app.world().resource::<ScriptWorld>();
        let ball = world
            .clones()
            .into_iter()
            .find(|&b| world.name(b) == ATTACK_BALL)
            .expect("the ball");
        assert!(world.removed(ball), "the ball went with it");
        let met = world.actor(ball).unwrap().position;
        let at = Vec3::from_array(CREATURE_AT);
        assert!(
            met.with_y(0.0).distance(at.with_y(0.0)) < 1.0,
            "on its body, short of the floor past it: {met}"
        );
        assert!(met.y > CREATURE_AT[1] + 0.1, "over the floor: {met}");
        app.update();
        assert_eq!(xp(&app), FIGHT_XP as u32);
        let report = shot(&app);
        let s = report
            .creatures
            .iter()
            .find(|s| s.name == FIGHT_CREATURE)
            .unwrap();
        assert!(s.gone && !s.body, "{s:?}");
        for _ in 0..3 {
            app.update();
        }
        assert_eq!(body(&mut app, c), None, "its body left the world");
    }
}
