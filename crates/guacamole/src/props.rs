//! The props present at the start (openspec AG.25, design decision 10;
//! level-scene's What's there at the start is what the original has and The
//! level is solid where the original's is): which of a level's placements
//! stand, what each draws and what each stops. The TV's loader (the binary's
//! `level.rs`) and the census's disc test read a level's props through this.
//!
//! - Which ([`LevelProps::new`], AG.7's rule as
//!   `guac_formats::level::at_start_of` holds it): a template never stands;
//!   an UNMANAGED placement with an activation range stands while the hero
//!   is within it, measured in 3D from the placement's position, the range in
//!   feet ([`Prop::stands`]); one without a range waits on its script, which
//!   guacamole doesn't run yet, so it doesn't stand. A creature, a skinned
//!   model with a script (enemies, NPCs, critters, the chests), is left out
//!   with its solid until its script runs. What the start's scripts do to the
//!   props that stand (AG.7: they rescale gourds, turn the Titan boulders'
//!   solids into cylinders) waits on them too.
//! - What it draws ([`PropModel`]): its model off the props part, each
//!   tfbmulti2 segment on its material as the level's draw
//!   ([`level_scene::drawn`]), the other effects' left out (AG.24), placed by
//!   its position, its Euler angles (AF.9's convention) and its scale. A
//!   skinned model with no script, or one the stage refused, draws nothing.
//! - What it stops ([`solid`]): its `CollisionInfo` as the original builds it
//!   ([`guac_motion::solid`], AG.7's table), a solid one a kinematic seam
//!   body, a shaped non-solid one a sensor that stops nothing, a shapeless or
//!   mesh-typed one no body at all (AG.7: the shared phantom the world never
//!   holds), each 0.05 m smaller than the original's as the level's mesh
//!   radius moves onto the hero ([`guac_motion::solid::HavokSolid::in_world`]).

use std::collections::HashMap;
use std::ops::Range;

use bevy::math::Vec3;
use bevy::prelude::Transform;
use guac_formats::level::{ActivityState, AtStart, at_start_of};
use guac_ir::actor::{Shape, Solid};
use guac_ir::level::{Placement, Placements, Props, SceneGraph};
use guac_ir::shading::Scene;
use guac_motion::seam::{Body, BodyId, BodyKind, Collider, Pose};
use guac_motion::solid::{SolidInfo, SolidKind, euler};

use crate::character::warm::WarmKey;
use crate::level_scene::{self, Drawn, LevelError};

/// The seam body id a prop's solid takes: this plus its object index in
/// `level.bld`, clear of the level's meshes' ([`level_scene::LEVEL_BODIES`]
/// up) and unique as object indices are.
pub const PROP_BODIES: u32 = 1 << 20;

/// Why a placement stands nowhere in guacamole yet.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum LeftOut {
    /// A template its scripts copy; it never runs itself (AG.7).
    Template,
    /// A skinned model with a script: a creature, left out with its solid
    /// until its script runs.
    Creature,
    /// No activation range: its script wakes it, takes it off or leaves it
    /// (AG.7), and guacamole doesn't run that script yet.
    ByScript,
    /// Ranged, but with no model to draw and no shaped solid.
    Nothing,
    /// An activity state a level file doesn't hold.
    State,
}

/// A placement that stands while the hero is within its range.
#[derive(Debug, Clone, PartialEq)]
pub struct Prop {
    /// Its object index in `level.bld`: unique, as names aren't.
    pub object: u32,
    pub name: String,
    /// Where its range is measured from: the placement's position.
    pub position: [f32; 3],
    /// Its `_activationRange`, feet.
    pub range: f32,
    /// Where it's drawn: its position, Euler angles and scale.
    pub transform: Transform,
    /// Into [`LevelProps::models`]; `None` when it draws nothing.
    pub model: Option<usize>,
    /// Its solid where the level has it; `None` for a shapeless or
    /// mesh-typed one.
    pub body: Option<Body>,
}

impl Prop {
    /// Whether it stands with the hero at `hero` (AG.7's rule: within its
    /// range, the boundary counting as in it).
    pub fn stands(&self, hero: Vec3) -> bool {
        at_start_of(
            ActivityState::Unmanaged.raw(),
            self.range,
            self.position,
            hero.to_array(),
        ) == Some(AtStart::Active)
    }
}

/// A model props draw: its tfbmulti2 segments on their materials, in the
/// model's frame.
#[derive(Debug, Clone, PartialEq)]
pub struct PropModel {
    /// Its path as the level names it.
    pub path: String,
    pub drawn: Drawn,
}

/// A level's props: what can stand, the models they draw, and what's left
/// out with why.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct LevelProps {
    pub models: Vec<PropModel>,
    pub props: Vec<Prop>,
    /// Every other placement, by name, with why it stands nowhere.
    pub left_out: Vec<(String, LeftOut)>,
    /// Ranged placements naming a model that draws nothing, with why: a
    /// prop for its solid, or left out as [`LeftOut::Nothing`] without one.
    pub undrawn: Vec<(String, &'static str)>,
}

impl LevelProps {
    /// `placements`' props, drawn from `props`' models.
    pub fn new(props: &Props, placements: &Placements) -> Result<Self, LevelError> {
        let by_path: HashMap<&str, &guac_ir::level::Prop> =
            props.props.iter().map(|p| (p.path.as_str(), p)).collect();
        let mut out = Self::default();
        let mut model_index: HashMap<&str, usize> = HashMap::new();
        for a in &placements.actors {
            let model = a.model.as_deref().and_then(|m| by_path.get(m).copied());
            let skinned = model.is_some_and(|m| m.skeleton.is_some());
            let why = match ActivityState::from_raw(a.activity_state) {
                Some(ActivityState::Template) => Some(LeftOut::Template),
                Some(ActivityState::Unmanaged) if skinned && a.script.is_some() => {
                    Some(LeftOut::Creature)
                }
                Some(ActivityState::Unmanaged) if a.activation_range <= 0.0 => {
                    Some(LeftOut::ByScript)
                }
                Some(ActivityState::Unmanaged) => None,
                _ => Some(LeftOut::State),
            };
            if let Some(why) = why {
                out.left_out.push((a.name.clone(), why));
                continue;
            }
            let body = a.solid.as_ref().and_then(|s| solid(a, s));
            let drawn = model.filter(|m| m.skeleton.is_none());
            if a.model.is_some() && drawn.is_none() {
                let why = if model.is_some() {
                    "a skinned model with no script"
                } else {
                    "its model didn't extract"
                };
                out.undrawn.push((a.name.clone(), why));
            }
            if body.is_none() && drawn.is_none() {
                out.left_out.push((a.name.clone(), LeftOut::Nothing));
                continue;
            }
            let model = match drawn {
                Some(m) => Some(match model_index.get(m.path.as_str()) {
                    Some(&i) => i,
                    None => {
                        let i = out.models.len();
                        out.models.push(PropModel {
                            path: m.path.clone(),
                            drawn: model_drawn(m)?,
                        });
                        model_index.insert(m.path.as_str(), i);
                        i
                    }
                }),
                None => None,
            };
            out.props.push(Prop {
                object: a.object,
                name: a.name.clone(),
                position: a.position,
                range: a.activation_range,
                transform: placed(a),
                model,
                body,
            });
        }
        Ok(out)
    }

    /// The props standing with the hero at `hero`, by index.
    pub fn standing(&self, hero: Vec3) -> impl Iterator<Item = usize> + '_ {
        self.props
            .iter()
            .enumerate()
            .filter(move |(_, p)| p.stands(hero))
            .map(|(i, _)| i)
    }

    /// The solids of the props standing with the hero at `hero`.
    pub fn bodies(&self, hero: Vec3) -> Vec<Body> {
        self.standing(hero)
            .filter_map(|i| self.props[i].body.clone())
            .collect()
    }

    /// The props standing with the hero at `hero` less those the level's
    /// scripts took off, by object (openspec AH.21: a breakable broken stands
    /// nowhere again, wherever he goes), by index.
    pub fn standing_but<'a>(
        &'a self,
        hero: Vec3,
        taken_off: impl Fn(u32) -> bool + 'a,
    ) -> impl Iterator<Item = usize> + 'a {
        self.standing(hero)
            .filter(move |&i| !taken_off(self.props[i].object))
    }

    /// Every model's drawn part in one list, the models in order, their
    /// materials numbered across them, and each model's segments in it.
    pub fn drawn(&self) -> (Drawn, Vec<Range<usize>>) {
        models_drawn(&self.models)
    }

    /// Every key the props' models draw in `scene`, each once: what the
    /// warm compiles beside the level's.
    pub fn keys(&self, scene: &Scene) -> Result<Vec<WarmKey>, LevelError> {
        models_keys(&self.models, scene)
    }
}

/// The models the extract's spawn part holds for one archive (AH.22), as a
/// prop draws its own (openspec AH.24: what a clone of a template naming
/// one draws): each static model's tfbmulti2 segments on their materials,
/// the other effects' left out as a level's are; a skinned one draws
/// nothing here.
pub fn spawn_models(spawns: &guac_ir::spawn::SpawnModels) -> Result<Vec<PropModel>, LevelError> {
    spawns
        .props
        .iter()
        .filter(|p| p.skeleton.is_none())
        .map(|p| {
            Ok(PropModel {
                path: p.path.clone(),
                drawn: model_drawn(p)?,
            })
        })
        .collect()
}

/// `models` in one list, the models in order, their materials numbered
/// across them, and each model's segments in it: as [`LevelProps::drawn`].
pub fn models_drawn(models: &[PropModel]) -> (Drawn, Vec<Range<usize>>) {
    let mut all = Drawn::default();
    let mut ranges = Vec::with_capacity(models.len());
    for m in models {
        let from = all.segments.len();
        all.append(m.drawn.clone());
        ranges.push(from..all.segments.len());
    }
    (all, ranges)
}

/// Every key `models` draw in `scene`, each once.
pub fn models_keys(models: &[PropModel], scene: &Scene) -> Result<Vec<WarmKey>, LevelError> {
    let (drawn, _) = models_drawn(models);
    Ok(level_scene::keys(&drawn, scene)?
        .into_iter()
        .map(|(k, _)| k)
        .collect())
}

/// A static prop's drawn part, as a scene's: its tfbmulti2 segments on
/// their materials (a layer's unset values at the level's defaults), the
/// other effects' left out.
fn model_drawn(model: &guac_ir::level::Prop) -> Result<Drawn, LevelError> {
    level_scene::drawn(
        SceneGraph {
            world: model.path.clone(),
            materials: model.materials.clone(),
            segments: model.segments.clone(),
            sky: Vec::new(),
        },
        false,
    )
}

/// Where a placement draws its model: at its position, turned by its Euler
/// angles (AF.9's convention, [`euler`]) and scaled by its percentages.
pub fn placed(placement: &Placement) -> Transform {
    Transform {
        translation: Vec3::from_array(placement.position),
        rotation: euler(placement.rotation),
        scale: Vec3::from_array(placement.scale) / 100.0,
    }
}

/// `CollisionInfo`'s primitive as the solid builder names it.
pub fn solid_kind(shape: Shape) -> SolidKind {
    match shape {
        Shape::None => SolidKind::None,
        Shape::Box => SolidKind::Box,
        Shape::Capsule => SolidKind::Capsule,
        Shape::Sphere => SolidKind::Sphere,
        Shape::Cylinder => SolidKind::Cylinder,
        Shape::Mesh => SolidKind::Mesh,
    }
}

/// A placement's solid as a seam body where the level has it: the
/// original's shape for its `CollisionInfo` at its scale, turned by its own
/// angles then the placement's and set on the placement's pivot, 0.05 m
/// smaller for the world (the hero's query shape takes it); a kinematic body
/// when solid, a sensor when not, nothing for a shapeless or mesh-typed one.
pub fn solid(placement: &Placement, solid: &Solid) -> Option<Body> {
    let havok = SolidInfo {
        kind: solid_kind(solid.shape),
        offset: solid.offset,
        rotation: solid.rotation,
        width: solid.width,
        height: solid.height,
        length: solid.length,
    }
    .havok(placement.scale)?;
    let at = Pose::new(
        Vec3::from_array(placement.position),
        euler(placement.rotation),
    );
    Some(Body {
        id: BodyId(PROP_BODIES + placement.object),
        kind: if solid.solid {
            BodyKind::Kinematic
        } else {
            BodyKind::Sensor
        },
        pose: havok.pose(at),
        collider: Collider::Convex(havok.in_world()),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::census;
    use guac_ir::level::{EffectMaterial, LevelMaterial, RefusedProp};
    use guac_ir::model::{Joint, Skeleton};
    use guac_ir::{DumpIdentity, Sha256};
    use guac_motion::parry::ParryWorld;
    use guac_motion::scenarios::{self, Soup, eruptor, run, stick};
    use guac_motion::seam::{PhysicsSeam, Shape as Convex};
    use guac_motion::solid::{CONVEX_RADIUS, MESH_RADIUS};
    use guac_motion::tuning::FOOT;

    const LEVEL: &str = "level_213_tutotial";

    fn identity() -> DumpIdentity {
        DumpIdentity {
            title_id: "BLUS30968".into(),
            app_ver: "01.00".into(),
            manifest_hash: Sha256([0; 32]),
        }
    }

    /// A `CollisionInfo` of `shape`, `w` x `h` x `l` feet, its own angles
    /// `rotation`, its offset `offset` metres.
    fn info(shape: Shape, solid: bool, [w, h, l]: [f32; 3], rotation: [f32; 3]) -> Solid {
        Solid {
            shape,
            solid,
            world: true,
            actor_surface: false,
            offset: [0.0; 3],
            rotation,
            width: w,
            height: h,
            length: l,
            weight: 10.0,
            rebound: 0.0,
        }
    }

    /// An actor as the placements part keeps one: UNMANAGED (4) or another
    /// state, its range in feet, at `at` facing `heading`.
    #[allow(clippy::too_many_arguments)]
    fn actor(
        object: u32,
        name: &str,
        state: u32,
        range: f32,
        at: [f32; 3],
        heading: f32,
        model: Option<&str>,
        solid: Option<Solid>,
        script: Option<&str>,
    ) -> Placement {
        Placement {
            object,
            id: 0,
            name: name.into(),
            script: script.map(str::to_owned),
            position: at,
            rotation: [0.0, heading, 0.0],
            scale: [100.0; 3],
            model: model.map(str::to_owned),
            solid,
            movement: None,
            waypoints: None,
            activity_state: state,
            activation_range: range,
        }
    }

    /// A made-up model: one segment on a material that draws chapter 1's
    /// first census key, and one on the water's warp, which isn't drawn
    /// (AG.24); skinned when `skinned`.
    fn model(path: &str, skinned: bool) -> guac_ir::level::Prop {
        let key = census::LEVEL[0].key(false).unwrap();
        let material = census::material_for(&key);
        let warp = LevelMaterial::Effect(EffectMaterial {
            name: "warp".into(),
            effect: 0xAA9,
            pixel: None,
            vertex: None,
            textures: Vec::new(),
            states: material.states,
            scrolls: Vec::new(),
            tracks: Vec::new(),
            unread: Vec::new(),
        });
        let mut segment = key.mesh.triangle(0);
        segment.positions = vec![[0.0, 0.0, 1.0], [1.0, 0.0, 1.0], [0.0, 1.0, 1.0]];
        let mut water = key.mesh.triangle(1);
        water.source = 1;
        guac_ir::level::Prop {
            path: path.into(),
            skeleton: skinned.then(|| Skeleton {
                joints: vec![Joint {
                    name: "root".into(),
                    parent: None,
                    reference: guac_ir::model::Transform::IDENTITY,
                    inverse_bind: bevy::math::Mat4::IDENTITY.to_cols_array_2d(),
                }],
            }),
            materials: vec![LevelMaterial::Multi2(material), warp],
            segments: vec![segment, water],
            idle: None,
        }
    }

    /// The hero at AF's fresh state (AF.9's idle pivot).
    const HERO: [f32; 3] = [10.190_27, 43.382_67, -93.394_06];

    fn at_x(dx: f32) -> [f32; 3] {
        [HERO[0] + dx, HERO[1], HERO[2]]
    }

    /// A made-up chapter 1's placements and props, one of each kind AG.7
    /// found: a gourd 4.5 m off with its 160 ft range, a coin 49.25 m off
    /// with its 160 ft (48.768 m), a template, a creature (a skinned model
    /// with a script), one its script wakes, a trigger (a non-solid box), a
    /// bug spawner (a range and nothing else), moths on a skinned model with
    /// no script and no solid, a gem whose model the stage refused, and the
    /// hero's own placement.
    fn level() -> (Props, Placements) {
        let sphere = info(Shape::Sphere, true, [7.25, 6.5, 3.0], [0.0; 3]);
        let none = info(Shape::None, true, [3.0, 6.0, 3.0], [0.0; 3]);
        let actors = vec![
            actor(
                10,
                "gourd",
                4,
                160.0,
                at_x(4.5),
                0.0,
                Some("gourd.mdl"),
                Some(sphere),
                None,
            ),
            actor(
                11,
                "coin",
                4,
                160.0,
                at_x(49.25),
                0.0,
                Some("coin.mdl"),
                Some(none),
                Some("Placed_Loot_Spinning.ai"),
            ),
            actor(
                12,
                "template",
                5,
                200.0,
                at_x(1.0),
                0.0,
                Some("gourd.mdl"),
                Some(sphere),
                None,
            ),
            actor(
                13,
                "chompy",
                4,
                260.0,
                at_x(3.0),
                0.0,
                Some("chompy.mdl"),
                Some(info(Shape::Capsule, true, [3.0, 6.0, 3.0], [0.0; 3])),
                Some("S2_Enemy_Chompy.ai"),
            ),
            actor(
                14,
                "fence",
                4,
                0.0,
                at_x(2.0),
                0.0,
                Some("gourd.mdl"),
                Some(info(Shape::Box, true, [23.0, 18.0, 3.0], [0.0; 3])),
                Some("WoodFence_Spawner.ai"),
            ),
            actor(
                15,
                "trigger",
                4,
                60.0,
                at_x(-5.0),
                0.0,
                None,
                Some(info(Shape::Box, false, [41.0, 14.0, 35.0], [0.0; 3])),
                None,
            ),
            actor(
                16,
                "bugs",
                4,
                60.0,
                at_x(6.0),
                0.0,
                None,
                Some(none),
                Some("Bug_Spawner.ai"),
            ),
            actor(
                17,
                "moths",
                4,
                80.0,
                at_x(7.0),
                0.0,
                Some("moths.mdl"),
                Some(none),
                None,
            ),
            actor(
                18,
                "gem",
                4,
                160.0,
                at_x(-3.0),
                0.0,
                Some("gem.mdl"),
                Some(info(Shape::Sphere, true, [3.0, 3.0, 3.0], [0.0; 3])),
                Some("Placed_Loot.ai"),
            ),
            actor(19, "hero", 0, 0.0, HERO, 0.0, None, Some(sphere), None),
        ];
        let props = Props {
            identity: identity(),
            level: LEVEL.into(),
            props: vec![
                model("gourd.mdl", false),
                model("coin.mdl", false),
                model("chompy.mdl", true),
                model("moths.mdl", true),
            ],
            refused: vec![RefusedProp {
                path: "gem.mdl".into(),
                reason: "sits under a transform".into(),
            }],
        };
        let placements = Placements {
            identity: identity(),
            level: LEVEL.into(),
            actors,
            waypoints: Vec::new(),
        };
        (props, placements)
    }

    fn names(props: &LevelProps, standing: impl Iterator<Item = usize>) -> Vec<&str> {
        standing.map(|i| props.props[i].name.as_str()).collect()
    }

    /// AG.7's rule as the props' set: templates, creatures and the
    /// placements a script wakes stand nowhere; a ranged one stands while
    /// the hero is within its range, the coin only once he's walked within
    /// its 160 ft; a solid one is a kinematic body, a non-solid one a sensor,
    /// a shapeless one none.
    #[test]
    fn the_rules_set_is_the_props_that_stand() {
        let (props, placements) = level();
        let p = LevelProps::new(&props, &placements).unwrap();
        let left: Vec<(&str, LeftOut)> = p.left_out.iter().map(|(n, w)| (n.as_str(), *w)).collect();
        assert_eq!(
            left,
            [
                ("template", LeftOut::Template),
                ("chompy", LeftOut::Creature),
                ("fence", LeftOut::ByScript),
                ("bugs", LeftOut::Nothing),
                ("moths", LeftOut::Nothing),
                ("hero", LeftOut::State),
            ]
        );
        assert_eq!(
            p.undrawn,
            [
                ("moths".to_owned(), "a skinned model with no script"),
                ("gem".to_owned(), "its model didn't extract"),
            ]
        );
        assert_eq!(
            names(&p, 0..p.props.len()),
            ["gourd", "coin", "trigger", "gem"]
        );
        let models: Vec<&str> = p.models.iter().map(|m| m.path.as_str()).collect();
        assert_eq!(models, ["gourd.mdl", "coin.mdl"]);
        assert_eq!(
            p.props.iter().map(|p| p.model).collect::<Vec<_>>(),
            [Some(0), Some(1), None, None]
        );

        // At the start the coin is 49.25 m off, past its 48.768 m.
        let start = Vec3::from_array(HERO);
        assert_eq!(names(&p, p.standing(start)), ["gourd", "trigger", "gem"]);
        let bodies = p.bodies(start);
        let kinds: Vec<(u32, BodyKind)> = bodies
            .iter()
            .map(|b| (b.id.0 - PROP_BODIES, b.kind))
            .collect();
        assert_eq!(
            kinds,
            [
                (10, BodyKind::Kinematic),
                (15, BodyKind::Sensor),
                (18, BodyKind::Kinematic)
            ]
        );
        // 0.5 m on toward it, 48.75 m: there; the trigger, 5.5 m behind with
        // its 60 ft (18.288 m), still.
        let on = Vec3::from_array(at_x(0.5));
        assert_eq!(
            names(&p, p.standing(on)),
            ["gourd", "coin", "trigger", "gem"]
        );
        // Far up the coin's way the gourd and the gem are out of their 160 ft.
        let far = Vec3::from_array(at_x(60.0));
        assert_eq!(names(&p, p.standing(far)), ["coin"]);
        // The coin's solid is shapeless: it stands, drawn, stopping nothing.
        assert!(p.bodies(far).is_empty());
        // Exactly at the range counts as in it, as AG.7's rule has it.
        let edge = Vec3::from_array(at_x(49.25 - 160.0 * FOOT));
        assert!(p.props[1].stands(edge));
    }

    /// Each `CollisionInfo` shape AG.7 settled, as a body where the level
    /// has it, against the live phantoms' sizes (AG.7's read, less the
    /// 0.05 m the hero's query shape takes): the gate door's box with its
    /// own quarter turn running its 34 ft across the gateway at heading 180,
    /// a pot's capsule, a gourd's sphere, a barrel's cylinder; a mesh-typed
    /// and a shapeless one none. Each answers a ray on parry's side of the
    /// seam where its surface is.
    #[test]
    fn a_collider_from_each_collision_info_shape() {
        let door_at = [14.830_149, 43.367_107, -82.224_2];
        let shaped = |object, shape, size, rotation, at, heading| {
            let mut a = actor(
                object,
                "x",
                4,
                200.0,
                at,
                heading,
                None,
                Some(info(shape, true, size, rotation)),
                None,
            );
            a.solid = Some(info(shape, true, size, rotation));
            solid(&a, a.solid.as_ref().unwrap())
        };
        let door = shaped(
            1,
            Shape::Box,
            [4.0, 21.0, 34.0],
            [0.0, 90.0, 0.0],
            door_at,
            180.000_27,
        )
        .unwrap();
        let Collider::Convex(Convex::Box {
            half_extents,
            margin,
        }) = door.collider
        else {
            panic!("{door:?}")
        };
        assert!(
            (half_extents - Vec3::new(0.5596, 3.1504, 5.1316)).length() < 1e-5,
            "{half_extents}"
        );
        assert_eq!(margin, CONVEX_RADIUS - MESH_RADIUS);
        // Half its height up from the pivot, its 34 ft along x.
        assert!(
            (door.pose.position - Vec3::new(door_at[0], door_at[1] + 3.2004, door_at[2])).length()
                < 1e-4
        );
        let along_x = (door.pose.rotation * Vec3::Z).x.abs();
        assert!(
            (along_x - 1.0).abs() < 1e-5,
            "its length along x: {along_x}"
        );
        assert_eq!(
            (door.id, door.kind),
            (BodyId(PROP_BODIES + 1), BodyKind::Kinematic)
        );

        let pot = shaped(2, Shape::Capsule, [6.0, 9.0, 3.0], [0.0; 3], [0.0; 3], 0.0).unwrap();
        let Collider::Convex(Convex::Capsule {
            radius,
            half_segment,
        }) = pot.collider
        else {
            panic!("{pot:?}")
        };
        assert!(
            (radius - (0.8644 - MESH_RADIUS)).abs() < 1e-5 && (half_segment - 0.4572).abs() < 1e-5
        );
        let gourd = shaped(
            3,
            Shape::Sphere,
            [7.25, 6.5, 3.0],
            [0.0; 3],
            [20.0, 0.0, 0.0],
            0.0,
        )
        .unwrap();
        let Collider::Convex(Convex::Sphere { radius }) = gourd.collider else {
            panic!("{gourd:?}")
        };
        assert!((radius - (1.0549 - MESH_RADIUS)).abs() < 1e-5, "{radius}");
        assert!((gourd.pose.position - Vec3::new(20.0, 1.1049, 0.0)).length() < 1e-4);
        let barrel = shaped(
            4,
            Shape::Cylinder,
            [6.0, 7.0, 3.0],
            [0.0; 3],
            [40.0, 0.0, 0.0],
            0.0,
        )
        .unwrap();
        let Collider::Convex(Convex::Cylinder {
            radius,
            half_height,
            margin,
        }) = barrel.collider
        else {
            panic!("{barrel:?}")
        };
        assert!((radius - 0.8644).abs() < 1e-5 && (half_height - 1.0168).abs() < 1e-5);
        assert_eq!(margin, CONVEX_RADIUS - MESH_RADIUS);
        assert!((barrel.pose.position.y - 1.0668).abs() < 1e-4);
        assert_eq!(
            shaped(5, Shape::Mesh, [3.0, 6.0, 3.0], [0.0; 3], [0.0; 3], 0.0),
            None
        );
        assert_eq!(
            shaped(6, Shape::None, [3.0, 6.0, 3.0], [0.0; 3], [0.0; 3], 0.0),
            None
        );

        // A non-solid one senses: casts and rays pass it.
        let mut trigger = actor(7, "t", 4, 60.0, [60.0, 0.0, 0.0], 0.0, None, None, None);
        let sensor = solid(
            &trigger,
            &info(Shape::Box, false, [41.0, 14.0, 35.0], [0.0; 3]),
        )
        .unwrap();
        assert_eq!(sensor.kind, BodyKind::Sensor);
        trigger.scale = [50.0; 3];
        let half = solid(
            &trigger,
            &info(Shape::Box, false, [41.0, 14.0, 35.0], [0.0; 3]),
        )
        .unwrap();
        let Collider::Convex(Convex::Box { half_extents, .. }) = half.collider else {
            panic!()
        };
        assert!(
            (half_extents.x - (41.0 * FOOT * 0.5 - 0.1) / 2.0).abs() < 1e-5,
            "its scale"
        );

        let world = ParryWorld::new(&[door, pot, gourd, barrel, sensor]).unwrap();
        // The door's face toward the start, 0.6096 m off its middle less the
        // 0.05 m the hero takes.
        let hit = world
            .ray(Vec3::new(14.83, 45.0, -95.0), Vec3::Z, 30.0, &[])
            .unwrap();
        assert_eq!(hit.body, BodyId(PROP_BODIES + 1));
        assert!(
            (hit.point.z - (door_at[2] - 0.5596)).abs() < 1e-4,
            "{hit:?}"
        );
        // Its end along x, 5.18 m off its middle.
        let hit = world
            .ray(Vec3::new(30.0, 45.0, door_at[2]), -Vec3::X, 30.0, &[])
            .unwrap();
        assert!(
            (hit.point.x - (door_at[0] + 5.1316)).abs() < 1e-4,
            "{hit:?}"
        );
        let hit = world
            .ray(Vec3::new(20.0, 5.0, 0.0), -Vec3::Y, 10.0, &[])
            .unwrap();
        assert_eq!(hit.body, BodyId(PROP_BODIES + 3));
        assert!((hit.point.y - (1.1049 + 1.0049)).abs() < 1e-4, "{hit:?}");
        let hit = world
            .ray(Vec3::new(40.0, 5.0, 0.0), -Vec3::Y, 10.0, &[])
            .unwrap();
        assert!(
            (hit.point.y - (1.0668 + 1.0668 - MESH_RADIUS)).abs() < 1e-4,
            "{hit:?}"
        );
        assert!(
            world
                .ray(Vec3::new(60.0, 5.0, 0.0), -Vec3::Y, 10.0, &[])
                .is_none()
        );
        assert_eq!(
            world.overlaps(
                &Convex::Sphere { radius: 0.1 },
                Pose::at(Vec3::new(60.0, 1.0, 0.0)),
                &[]
            ),
            [BodyId(PROP_BODIES + 7)]
        );
    }

    /// A prop draws its model where its placement puts it: turned by its
    /// heading (90° takes the model's +z to +x) and scaled, its tfbmulti2
    /// segment on its material and the warp's left out (AG.24); its keys
    /// the census's.
    #[test]
    fn a_prop_draws_its_model_where_its_placement_puts_it() {
        let mut a = actor(
            1,
            "gourd",
            4,
            160.0,
            [1.0, 2.0, 3.0],
            90.0,
            Some("gourd.mdl"),
            None,
            None,
        );
        a.scale = [150.0; 3];
        let t = placed(&a);
        let corner = t.transform_point(Vec3::new(0.0, 0.0, 1.0));
        assert!(
            (corner - Vec3::new(2.5, 2.0, 3.0)).length() < 1e-5,
            "{corner}"
        );
        let props = Props {
            identity: identity(),
            level: LEVEL.into(),
            props: vec![model("gourd.mdl", false)],
            refused: Vec::new(),
        };
        let placements = Placements {
            identity: identity(),
            level: LEVEL.into(),
            actors: vec![a],
            waypoints: Vec::new(),
        };
        let p = LevelProps::new(&props, &placements).unwrap();
        let (drawn, ranges) = p.drawn();
        assert_eq!((drawn.materials.len(), drawn.segments.len()), (1, 1));
        assert_eq!((drawn.left_out_materials, drawn.left_out_segments), (1, 1));
        assert_eq!((ranges.len(), ranges[0].clone()), (1, 0..1));
        assert_eq!(p.props[0].transform, t);
        let scene = Scene {
            fog: true,
            ..Scene::new()
        };
        assert_eq!(
            p.keys(&scene).unwrap(),
            [census::LEVEL[0].key(false).unwrap()]
        );
    }

    /// What only a Giant moves stays put (openspec AH.27; breakables' Tree
    /// Rex at a boulder, a stand-in until the Titan feats run): a Titan
    /// boulder built as the props build it off `Titan_CarriableObject_
    /// Boulder_v3_(4)`'s placement (AG.7: an 18 x 16 x 3 ft sphere 2.74 m
    /// down, its feat script `Titan_Carriable_Object.ai` off the TV's
    /// listing), walked into head-on for 3 s by a Giant-sized capsule (8 x
    /// 14 x 3 ft at 100 %, ours: Tree Rex's own read is AG.27's) on Tree
    /// Rex's live 20 ft/s and 360°/s (AG.12): he stops against it, the
    /// boulder stands where it stood with its solid, and taken-off reads
    /// false for it, so nothing takes it down.
    #[test]
    fn a_titan_boulder_a_giant_walks_into_stays_put() {
        let feat = "C:/tfb/Content/Levels/Includes/Titan_Includes/Titan_CarribleObject/Scripts/Titan_Carriable_Object.ai";
        let boulder_at = [0.0, 0.0, 6.0];
        let mut solid = info(Shape::Sphere, true, [18.0, 16.0, 3.0], [0.0; 3]);
        solid.offset = [0.0, -2.7432, 0.0];
        let boulder = actor(
            32886,
            "Titan_CarriableObject_Boulder_v3_(4)",
            4,
            220.0,
            boulder_at,
            0.0,
            Some("boulder.mdl"),
            Some(solid),
            Some(feat),
        );
        let p = LevelProps::new(
            &Props {
                identity: identity(),
                level: LEVEL.into(),
                props: vec![model("boulder.mdl", false)],
                refused: Vec::new(),
            },
            &Placements {
                identity: identity(),
                level: LEVEL.into(),
                actors: vec![boulder],
                waypoints: Vec::new(),
            },
        )
        .unwrap();
        assert_eq!(p.props.len(), 1, "a prop, its feat script not run");
        assert!(
            !crate::scripts::smash_config(guac_script::Mode::Play)
                .whitelist
                .lists("Titan_CarriableObject_Boulder_v3_(4)", feat)
        );
        let mut bodies = vec![scenarios::level(Soup::default().floor(
            0.0,
            [-20.0, 20.0],
            [-20.0, 20.0],
            4.0,
        ))];
        let before = p.bodies(Vec3::ZERO);
        bodies.extend(before.clone());
        let world = ParryWorld::new(&bodies).unwrap();
        let giant = guac_motion::solid::SolidInfo {
            kind: guac_motion::solid::SolidKind::Capsule,
            offset: [0.0; 3],
            rotation: [0.0; 3],
            width: 8.0,
            height: 14.0,
            length: 3.0,
        }
        .havok([100.0; 3])
        .unwrap();
        let tree_rex = guac_motion::tuning::Tuning {
            max_speed: 20.0,
            turn_rate: 360.0,
            width: 8.0,
            ..guac_motion::tuning::Tuning::ERUPTOR
        };
        let mut hero = guac_motion::hero::Hero::new(giant, tree_rex, Vec3::new(0.0, 1.0, 0.0), 0.0);
        hero.settle(&world, &[]);
        let states = run(&world, &mut hero, 180, |_| stick(1.0, -scenarios::VIEW));
        let s = states[179];
        assert_eq!(s.velocity, Vec3::ZERO, "stopped against it: {s:?}");
        assert!(s.pivot.z > 0.5 && s.pivot.z < boulder_at[2], "{s:?}");
        let standing: Vec<usize> = p.standing_but(s.pivot, |_| false).collect();
        assert_eq!(standing, [0], "it stands where it stood");
        assert_eq!(p.bodies(s.pivot), before, "its solid unmoved");
    }

    /// The gate's door and a gourd, built as the props build them off their
    /// placements, stop Eruptor where the original's do on parry's side of
    /// the seam: walked into head-on, his pivot 1.4781 m from the door's
    /// middle (AG.7) within the millimetre his 5 mm keep makes of its 4.1;
    /// the gourd (a 7.25 ft sphere, AG.7's walk back) with their centres
    /// just past touching, AG.7's 2.5 mm short and his 5 mm keep.
    #[test]
    fn the_door_and_a_gourd_stop_him_where_the_originals_do() {
        let door_at = [0.0, 0.0, 5.0];
        let door = actor(
            1,
            "door",
            4,
            150.0,
            door_at,
            180.0,
            Some("door.mdl"),
            Some(info(Shape::Box, true, [4.0, 21.0, 34.0], [0.0, 90.0, 0.0])),
            Some("Breakable_Wall_213.ai"),
        );
        let gourd = actor(
            2,
            "gourd",
            4,
            160.0,
            [-8.0, 0.0, 5.0],
            0.0,
            Some("gourd.mdl"),
            Some(info(Shape::Sphere, true, [7.25, 6.5, 3.0], [0.0; 3])),
            None,
        );
        let p = LevelProps::new(
            &Props {
                identity: identity(),
                level: LEVEL.into(),
                props: vec![model("door.mdl", false), model("gourd.mdl", false)],
                refused: Vec::new(),
            },
            &Placements {
                identity: identity(),
                level: LEVEL.into(),
                actors: vec![door, gourd],
                waypoints: Vec::new(),
            },
        )
        .unwrap();
        let mut bodies = vec![scenarios::level(Soup::default().floor(
            0.0,
            [-20.0, 20.0],
            [-20.0, 20.0],
            4.0,
        ))];
        bodies.extend(p.bodies(Vec3::ZERO));
        assert_eq!(bodies.len(), 3);
        let world = ParryWorld::new(&bodies).unwrap();

        let mut hero = eruptor(&world, Vec3::new(2.0, 1.0, 0.0), 0.0);
        let states = run(&world, &mut hero, 90, |_| stick(1.0, -scenarios::VIEW));
        let s = states[89];
        assert_eq!(s.velocity, Vec3::ZERO, "{s:?}");
        let from_middle = door_at[2] - s.pivot.z;
        assert!((from_middle - 1.4781).abs() < 1.5e-3, "{from_middle}");

        let mut hero = eruptor(&world, Vec3::new(-8.0, 1.0, 0.0), 0.0);
        let states = run(&world, &mut hero, 90, |_| stick(1.0, -scenarios::VIEW));
        let s = states[89];
        assert_eq!(s.velocity, Vec3::ZERO, "{s:?}");
        let centres = (s.pivot + Vec3::Y * 0.9144).distance(Vec3::new(-8.0, 1.1049, 5.0));
        let touching = 1.0549 + 0.8644;
        assert!(
            centres > touching && centres - touching < 0.006,
            "{centres}"
        );
    }
}
