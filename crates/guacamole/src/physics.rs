//! The physics seam's Avian side (openspec AG.4, design decision 3): the
//! seam's bodies as Avian colliders and its four queries over Avian's
//! `SpatialQuery`. Queries only: static, kinematic and sensor colliders, no
//! dynamic body, no solver.
//!
//! [`SeamPlugin`] adds the Avian plugins the queries stand on and no others:
//! the physics schedule (in `FixedPostUpdate`), the collider hierarchy (which
//! tree a body's collider joins), the collider backend (AABBs, the required
//! components), the collider trees `SpatialQuery` walks and the broad phase's
//! system sets they run in. No solver, narrow phase, broad-phase pairs,
//! joints, mass properties, interpolation or transform sync: a seam body is
//! posed through its `Position` and `Rotation`, and nothing else in the app
//! carries either, so the TV's 2D UI never meets a physics system. In a
//! headless app that is 14 more systems, 20 observers, 17 resources and the
//! physics schedule, run once a fixed step.
//!
//! What `SpatialQuery` doesn't hand back, the adapter takes from the
//! collider's own parry shape: a mesh triangle under a ray (Avian's
//! `RayHitData` has no feature) and the contact planes (Avian's
//! `contact_manifolds` drops the manifold's sub-shape, the triangle), the
//! same parry call Avian's wrapper makes, through guac-motion's
//! `planes_against`.

use std::sync::Arc;

use avian3d::parry::bounding_volume::BoundingVolume;
use avian3d::parry::query::Ray;
use avian3d::parry::shape::SharedShape;
use avian3d::prelude::{
    BroadPhaseCorePlugin, Collider, ColliderAabb, ColliderBackendPlugin, ColliderHierarchyPlugin,
    ColliderTreePlugin, PhysicsSchedule, PhysicsSchedulePlugin, PhysicsStepSystems, Position,
    RigidBody, Rotation, Sensor, ShapeCastConfig, SolverSystems, SpatialQuery, SpatialQueryFilter,
    TrimeshFlags,
};
use bevy::ecs::system::SystemParam;
use bevy::prelude::*;
use guac_motion::parry::{
    Solid, WorldError, check_body, check_pose, grown_for_rays, parry_pose, planes_against,
    split_motion,
};
use guac_motion::seam::{
    Body, BodyId, BodyKind, CAST_SLOP, Cast, Collider as SeamCollider, ContactPlane, Mesh,
    PhysicsSeam, Pose, RayHit, Shape, sort_planes,
};

/// Avian's plugins for the seam's queries (see the module docs).
pub(crate) struct SeamPlugin;

impl Plugin for SeamPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins((
            PhysicsSchedulePlugin::default(),
            // Ties a collider to its rigid body (`ColliderOf`), which picks
            // the tree it goes in; without it a body's collider never does.
            ColliderHierarchyPlugin,
            ColliderBackendPlugin::<Collider>::default(),
            ColliderTreePlugin::<Collider>::default(),
            // Only for its system sets (and two empty graphs): the trees'
            // refit and rebuild sit in them.
            BroadPhaseCorePlugin,
        ))
        // The trees finish a rebuild in the solver's last set, which the
        // solver's schedule plugin would place; with no solver, it goes where
        // that plugin puts it, so the physics schedule's order stays
        // unambiguous (Avian builds it with ambiguity detection as an error).
        .configure_sets(
            PhysicsSchedule,
            SolverSystems::Finalize.in_set(PhysicsStepSystems::Solver),
        );
    }
}

/// Tags a collider entity with its seam body, so a query hit (an entity) reads
/// back as a `BodyId`, a sensor stays out of the solid queries and a mesh hit
/// finds its triangle's material.
#[derive(Component, Clone, Debug)]
pub(crate) struct SeamBody {
    pub(crate) id: BodyId,
    pub(crate) kind: BodyKind,
    mesh: Option<Arc<Mesh>>,
}

/// A convex seam shape in Avian's terms. Its constructors take full lengths
/// (a capsule's segment, not its height); the margin rounds the box or
/// cylinder, and Avian has no rounded-cylinder constructor, so that one is
/// parry's.
pub(crate) fn convex_collider(shape: &Shape) -> Collider {
    match *shape {
        Shape::Sphere { radius } => Collider::sphere(radius),
        Shape::Capsule {
            radius,
            half_segment,
        } => Collider::capsule(radius, 2.0 * half_segment),
        Shape::Box {
            half_extents: h,
            margin,
        } if margin > 0.0 => Collider::round_cuboid(2.0 * h.x, 2.0 * h.y, 2.0 * h.z, margin),
        Shape::Box {
            half_extents: h, ..
        } => Collider::cuboid(2.0 * h.x, 2.0 * h.y, 2.0 * h.z),
        Shape::Cylinder {
            radius,
            half_height,
            margin,
        } if margin > 0.0 => SharedShape::round_cylinder(half_height, radius, margin).into(),
        Shape::Cylinder {
            radius,
            half_height,
            ..
        } => Collider::cylinder(radius, 2.0 * half_height),
    }
}

/// A seam mesh as a static trimesh collider, its internal edges fixed.
pub(crate) fn mesh_collider(mesh: &Mesh) -> Collider {
    Collider::trimesh_with_config(
        mesh.vertices().to_vec(),
        mesh.triangles().to_vec(),
        TrimeshFlags::FIX_INTERNAL_EDGES,
    )
}

/// A seam body checked and its Avian collider built, ready to spawn: built
/// off the frame when it's a level's mesh ([`PreparedBody::new`] on a task),
/// since a trimesh with its internal edges fixed costs its triangles' worth.
#[derive(Clone)]
pub(crate) struct PreparedBody {
    body: SeamBody,
    pose: Pose,
    collider: Collider,
}

impl PreparedBody {
    /// Refuses what `ParryWorld::new` refuses for one body (a pose or a
    /// convex size that isn't finite), so bad extracted data is an error
    /// here, not a panic or a NaN in Avian's trees; keeping ids unique is the
    /// caller's, as it builds the list.
    pub(crate) fn new(body: &Body) -> Result<Self, WorldError> {
        check_body(body)?;
        let (collider, mesh) = match &body.collider {
            SeamCollider::Convex(shape) => (convex_collider(shape), None),
            SeamCollider::Mesh(mesh) => (mesh_collider(mesh), Some(mesh.clone())),
        };
        Ok(Self {
            body: SeamBody {
                id: body.id,
                kind: body.kind,
                mesh,
            },
            pose: body.pose,
            collider,
        })
    }

    /// Spawns it: a static rigid body, a kinematic one, or a kinematic
    /// sensor. The `Transform` is there for Avian's collider scale (read off
    /// the `GlobalTransform` on insert), never as the body's pose.
    pub(crate) fn spawn(self, commands: &mut Commands) -> Entity {
        let rigid = match self.body.kind {
            BodyKind::Static => RigidBody::Static,
            BodyKind::Kinematic | BodyKind::Sensor => RigidBody::Kinematic,
        };
        let sensor = self.body.kind == BodyKind::Sensor;
        let mut entity = commands.spawn((
            self.body,
            rigid,
            self.collider,
            Position(self.pose.position),
            Rotation(self.pose.rotation),
            Transform::default(),
        ));
        if sensor {
            entity.insert(Sensor);
        }
        entity.id()
    }
}

/// Spawns a seam body at once ([`PreparedBody`]'s checks and kinds): the
/// tests' worlds; the level's are prepared off the frame.
#[cfg(test)]
pub(crate) fn spawn_body(commands: &mut Commands, body: &Body) -> Result<Entity, WorldError> {
    Ok(PreparedBody::new(body)?.spawn(commands))
}

/// Moves a kinematic body or sensor, refusing as `ParryWorld::set_pose` does
/// (no such body, a static one, a pose that isn't finite). Queries see it
/// there once the physics schedule has run (it refits the collider trees),
/// where `ParryWorld` sees it at once.
#[cfg_attr(
    not(test),
    expect(
        dead_code,
        reason = "nothing in the level moves yet: AG.25's props stand still until their scripts run"
    )
)]
pub(crate) fn set_pose(
    bodies: &mut Query<(&SeamBody, &mut Position, &mut Rotation)>,
    id: BodyId,
    pose: Pose,
) -> Result<(), WorldError> {
    let (body, mut position, mut rotation) = bodies
        .iter_mut()
        .find(|(body, ..)| body.id == id)
        .ok_or(WorldError::Missing(id))?;
    if body.kind == BodyKind::Static {
        return Err(WorldError::Static(id));
    }
    check_pose(id, pose)?;
    position.0 = pose.position;
    rotation.0 = pose.rotation;
    Ok(())
}

/// The seam's queries for any system that takes it: `SpatialQuery` for the
/// broad phase, casts and rays, the seam bodies' colliders and poses for
/// what `SpatialQuery` doesn't return.
#[derive(SystemParam)]
pub(crate) struct AvianSeam<'w, 's> {
    spatial: SpatialQuery<'w, 's>,
    bodies: Query<
        'w,
        's,
        (
            &'static SeamBody,
            &'static Position,
            &'static Rotation,
            &'static Collider,
        ),
    >,
}

impl AvianSeam<'_, '_> {
    fn solid(&self, entity: Entity, exclude: &[BodyId]) -> bool {
        self.bodies
            .get(entity)
            .is_ok_and(|(b, ..)| b.kind.is_solid() && !exclude.contains(&b.id))
    }

    /// The first solid surface along a normalized ray, meshes alone when
    /// `meshes_only`.
    fn first_hit(
        &self,
        origin: Vec3,
        dir: Vec3,
        max_distance: f32,
        exclude: &[BodyId],
        meshes_only: bool,
    ) -> Option<RayHit> {
        let hit = self.spatial.cast_ray_predicate(
            origin,
            dir3(dir),
            max_distance,
            false,
            &SpatialQueryFilter::default(),
            &|entity| {
                self.solid(entity, exclude)
                    && (!meshes_only
                        || self
                            .bodies
                            .get(entity)
                            .is_ok_and(|(b, ..)| b.mesh.is_some()))
            },
        )?;
        let (body, position, rotation, collider) = self.bodies.get(hit.entity).ok()?;
        // The same ray against the one collider it hit, for the triangle.
        let triangle = body.mesh.as_ref().and_then(|_| {
            collider
                .shape_scaled()
                .cast_ray_and_get_normal(
                    &parry_pose(Pose::new(position.0, rotation.0)),
                    &Ray::new(origin, dir),
                    max_distance,
                    false,
                )
                .map(|h| h.subshape)
        });
        Some(RayHit {
            body: body.id,
            triangle,
            material: body
                .mesh
                .as_ref()
                .zip(triangle)
                .and_then(|(m, t)| m.material(t)),
            distance: hit.distance,
            point: origin + dir * hit.distance,
            normal: hit.normal,
        })
    }
}

/// The unit direction a seam query already normalized, as Avian's `Dir3`
/// without normalizing it again, so both sides cast along the same vector.
fn dir3(dir: Vec3) -> Dir3 {
    Dir3::new_unchecked(dir)
}

impl PhysicsSeam for AvianSeam<'_, '_> {
    fn cast(
        &self,
        shape: &Shape,
        from: Pose,
        motion: Vec3,
        contact_distance: f32,
        exclude: &[BodyId],
    ) -> Option<Cast> {
        let (dir, length) = split_motion(motion)?;
        let config = ShapeCastConfig {
            max_distance: length,
            target_distance: 0.0,
            compute_contact_on_penetration: true,
            ignore_origin_penetration: true,
        };
        let hit = self.spatial.cast_shape_predicate(
            &convex_collider(shape),
            from.position,
            from.rotation,
            dir3(dir),
            &config,
            &SpatialQueryFilter::default(),
            &|entity| self.solid(entity, exclude),
        )?;
        let at = Pose::new(from.position + dir * hit.distance, from.rotation);
        Some(Cast {
            fraction: (hit.distance / length).clamp(0.0, 1.0),
            distance: hit.distance,
            planes: self.contacts(shape, at, contact_distance.max(CAST_SLOP), exclude),
        })
    }

    fn contacts(
        &self,
        shape: &Shape,
        at: Pose,
        distance: f32,
        exclude: &[BodyId],
    ) -> Vec<ContactPlane> {
        let distance = distance.max(0.0);
        let query = convex_collider(shape);
        let pose = parry_pose(at);
        let reach = query.shape_scaled().compute_aabb(&pose).loosened(distance);
        let mut planes = Vec::new();
        for entity in self
            .spatial
            .aabb_intersections_with_aabb(ColliderAabb::from_min_max(reach.mins, reach.maxs))
        {
            let Ok((body, position, rotation, collider)) = self.bodies.get(entity) else {
                continue;
            };
            if !body.kind.is_solid() || exclude.contains(&body.id) {
                continue;
            }
            let solid = Solid {
                body: body.id,
                pose: parry_pose(Pose::new(position.0, rotation.0)),
                shape: collider.shape_scaled(),
                mesh: body.mesh.as_deref(),
            };
            planes_against(&solid, query.shape_scaled(), &pose, distance, &mut planes);
        }
        sort_planes(&mut planes);
        planes
    }

    fn ray(
        &self,
        origin: Vec3,
        direction: Vec3,
        max_distance: f32,
        exclude: &[BodyId],
    ) -> Option<RayHit> {
        let (dir, _) = split_motion(direction)?;
        self.first_hit(origin, dir, max_distance, exclude, false)
    }

    /// The mesh through `SpatialQuery` as [`PhysicsSeam::ray`] has it, each
    /// convex body's grown shape (guac-motion's `grown_for_rays`, the parry
    /// side's own) against the ray directly: a level's handful of solids.
    fn ray_grown(
        &self,
        origin: Vec3,
        direction: Vec3,
        max_distance: f32,
        grow: f32,
        exclude: &[BodyId],
    ) -> Option<RayHit> {
        let (dir, _) = split_motion(direction)?;
        let mut best = self.first_hit(origin, dir, max_distance, exclude, true);
        let ray = Ray::new(origin, dir);
        for (body, position, rotation, collider) in &self.bodies {
            if body.mesh.is_some() || !body.kind.is_solid() || exclude.contains(&body.id) {
                continue;
            }
            let reach = best.map_or(max_distance, |b| b.distance);
            let shape = grown_for_rays(collider.shape_scaled(), grow);
            let Some(found) = shape.cast_ray_and_get_normal(
                &parry_pose(Pose::new(position.0, rotation.0)),
                &ray,
                reach,
                false,
            ) else {
                continue;
            };
            if best.is_none_or(|b| found.time_of_impact < b.distance) {
                best = Some(RayHit {
                    body: body.id,
                    triangle: None,
                    material: None,
                    distance: found.time_of_impact,
                    point: origin + dir * found.time_of_impact,
                    normal: found.normal,
                });
            }
        }
        best
    }

    fn overlaps(&self, shape: &Shape, at: Pose, exclude: &[BodyId]) -> Vec<BodyId> {
        let mut ids: Vec<BodyId> = self
            .spatial
            .shape_intersections(
                &convex_collider(shape),
                at.position,
                at.rotation,
                &SpatialQueryFilter::default(),
            )
            .into_iter()
            .filter_map(|entity| self.bodies.get(entity).ok())
            .map(|(body, ..)| body.id)
            .filter(|id| !exclude.contains(id))
            .collect();
        ids.sort();
        ids
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use bevy::ecs::system::SystemState;
    use bevy::time::TimeUpdateStrategy;
    use guac_motion::conformance::{self, Answer, Case};
    use guac_motion::parry::ParryWorld;

    use super::*;

    /// A headless app with Avian's seam plugins and no renderer, each update
    /// one 1/64 s fixed step, so the physics schedule runs once per update.
    fn headless() -> App {
        let mut app = App::new();
        app.add_plugins((MinimalPlugins, SeamPlugin))
            .insert_resource(TimeUpdateStrategy::ManualDuration(Duration::from_secs_f64(
                1.0 / 64.0,
            )));
        app.finish();
        app.cleanup();
        app
    }

    fn world_of(case: &Case) -> App {
        let mut app = headless();
        let mut commands = app.world_mut().commands();
        for body in &case.bodies {
            spawn_body(&mut commands, body).expect(case.name);
        }
        app.world_mut().flush();
        app.update();
        if !case.moves.is_empty() {
            let mut state = SystemState::<Query<(&SeamBody, &mut Position, &mut Rotation)>>::new(
                app.world_mut(),
            );
            let mut bodies = state.get_mut(app.world_mut()).unwrap();
            for &(id, pose) in &case.moves {
                set_pose(&mut bodies, id, pose).expect(case.name);
            }
            app.update();
        }
        app
    }

    fn avian_answers(case: &Case) -> Result<Vec<Answer>, String> {
        let mut app = world_of(case);
        let mut state = SystemState::<AvianSeam>::new(app.world_mut());
        let seam = state.get(app.world()).unwrap();
        conformance::run(&seam, case)
    }

    fn parry_answers(case: &Case) -> Vec<Answer> {
        let mut world = ParryWorld::new(&case.bodies).unwrap();
        for &(id, pose) in &case.moves {
            world.set_pose(id, pose).unwrap();
        }
        conformance::run(&world, case).unwrap()
    }

    /// Every case of the seam's table through Avian, and Avian's answers
    /// within `conformance::AGREE` of parry's.
    #[test]
    fn the_table_through_avian_agrees_with_parry() {
        let mut failures = Vec::new();
        for case in conformance::cases() {
            match avian_answers(&case) {
                Err(e) => failures.push(format!("avian: {e}")),
                Ok(avian) => {
                    if let Err(e) = conformance::agree(&avian, &parry_answers(&case)) {
                        failures.push(format!("{}: avian against parry: {e}", case.name));
                    }
                }
            }
        }
        assert!(failures.is_empty(), "{}", failures.join("\n\n"));
    }

    /// AG.25: a prop's solid off each `CollisionInfo` shape AG.7 settled,
    /// built as the props build them (the gate door's box on its own quarter
    /// turn at heading 180, a pot's capsule, a gourd's sphere, a barrel's
    /// cylinder, a trigger's non-solid box), answers through Avian as
    /// through parry: rays and the hero's grown sphere cast at each, his
    /// contacts touching each, overlaps in the trigger, within
    /// `conformance::AGREE`.
    #[test]
    fn the_props_solids_answer_through_avian_as_through_parry() {
        use guac_ir::actor::{Shape as IrShape, Solid as IrSolid};
        use guac_ir::level::Placement;
        use guac_motion::conformance::Query;
        use guacamole::props;
        let place = |object, at: [f32; 3], heading, shape, solid, size: [f32; 3], turn| {
            let p = Placement {
                object,
                id: 0,
                name: format!("prop {object}"),
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
                solid,
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
            props::solid(&p, &info).unwrap()
        };
        let bodies = vec![
            place(
                1,
                [0.0, 0.0, 5.0],
                180.0,
                IrShape::Box,
                true,
                [4.0, 21.0, 34.0],
                90.0,
            ),
            place(
                2,
                [10.0, 0.0, 0.0],
                30.0,
                IrShape::Capsule,
                true,
                [6.0, 9.0, 3.0],
                0.0,
            ),
            place(
                3,
                [20.0, 0.0, 0.0],
                0.0,
                IrShape::Sphere,
                true,
                [7.25, 6.5, 3.0],
                0.0,
            ),
            place(
                4,
                [30.0, 0.0, 0.0],
                45.0,
                IrShape::Cylinder,
                true,
                [6.0, 7.0, 3.0],
                0.0,
            ),
            place(
                5,
                [40.0, 0.0, 0.0],
                0.0,
                IrShape::Box,
                false,
                [41.0, 14.0, 35.0],
                0.0,
            ),
        ];
        let hero = Shape::Sphere { radius: 0.9144 };
        let mut queries = Vec::new();
        // From in front of each, and from over its middle (the door's at
        // z 5, the rest at 0).
        for (x, z, middle) in [
            (0.0, -2.0, 5.0),
            (10.0, -3.0, 0.0),
            (20.0, -3.0, 0.0),
            (30.0, -3.0, 0.0),
            (40.0, -10.0, 0.0),
        ] {
            let from = Vec3::new(x, 0.9144, z);
            queries.push(Query::Ray {
                origin: from,
                direction: Vec3::Z,
                max_distance: 20.0,
                exclude: Vec::new(),
            });
            queries.push(Query::Cast {
                shape: hero,
                from: Pose::at(from),
                motion: Vec3::Z * 10.0,
                contact_distance: 0.05,
                exclude: Vec::new(),
            });
            queries.push(Query::Ray {
                origin: Vec3::new(x + 0.3, 12.0, middle),
                direction: -Vec3::Y,
                max_distance: 20.0,
                exclude: Vec::new(),
            });
        }
        // Touching the door's face 5 mm off, and the gourd.
        queries.push(Query::Contacts {
            shape: hero,
            at: Pose::at(Vec3::new(0.0, 0.9144, 5.0 - 0.5596 - 0.9144 - 0.005)),
            distance: 0.05,
            exclude: Vec::new(),
        });
        queries.push(Query::Contacts {
            shape: hero,
            at: Pose::at(Vec3::new(20.0, 1.1049, -(1.0049 + 0.9144 + 0.002))),
            distance: 0.05,
            exclude: Vec::new(),
        });
        queries.push(Query::Overlaps {
            shape: hero,
            at: Pose::at(Vec3::new(40.0, 1.0, 0.0)),
            exclude: Vec::new(),
        });
        let case = Case {
            name: "the props' solids",
            bodies,
            moves: Vec::new(),
            queries,
            expect: |answers| {
                // Every solid one met by its ray and its cast, the trigger by
                // neither; the two touching poses one plane each; the
                // trigger overlapped.
                let hits = answers
                    .iter()
                    .filter(|a| matches!(a, Answer::Ray(Some(_)) | Answer::Cast(Some(_))))
                    .count();
                let planes: Vec<usize> = answers
                    .iter()
                    .filter_map(|a| match a {
                        Answer::Contacts(p) => Some(p.len()),
                        _ => None,
                    })
                    .collect();
                if hits != 12 || planes != [1, 1] {
                    return Err(format!("{hits} hits, planes {planes:?}"));
                }
                match answers.last() {
                    Some(Answer::Overlaps(ids)) if ids == &[BodyId(props::PROP_BODIES + 5)] => {
                        Ok(())
                    }
                    other => Err(format!("the trigger: {other:?}")),
                }
            },
        };
        let parry = parry_answers(&case);
        let avian = avian_answers(&case).unwrap();
        conformance::agree(&avian, &parry).unwrap();
    }

    /// A moved kinematic body isn't where queries look until the physics
    /// schedule refits the trees: the update in between is the contract.
    #[test]
    fn a_move_shows_after_the_physics_step() {
        let case = conformance::cases()
            .into_iter()
            .find(|c| c.name == "kinematic_moved")
            .unwrap();
        let mut app = world_of(&Case {
            moves: vec![],
            ..case
        });
        let door = case_door();
        let mut state =
            SystemState::<Query<(&SeamBody, &mut Position, &mut Rotation)>>::new(app.world_mut());
        let mut bodies = state.get_mut(app.world_mut()).unwrap();
        set_pose(&mut bodies, door.0, door.1).unwrap();
        let probe = |app: &mut App| {
            let mut state = SystemState::<AvianSeam>::new(app.world_mut());
            let seam = state.get(app.world()).unwrap();
            seam.overlaps(
                &Shape::Sphere { radius: 0.5 },
                Pose::at(door.1.position),
                &[],
            )
        };
        assert_eq!(probe(&mut app), Vec::<BodyId>::new(), "before the step");
        app.update();
        assert_eq!(probe(&mut app), vec![door.0], "after the step");
    }

    fn case_door() -> (BodyId, Pose) {
        conformance::cases()
            .into_iter()
            .find(|c| c.name == "kinematic_moved")
            .unwrap()
            .moves[0]
    }

    /// What Avian's own systems read off a seam body: the level mesh a static
    /// rigid body with its internal edges fixed, a placement kinematic, a
    /// sensor kinematic and marked `Sensor` (no mass, a sensor in Avian's
    /// collision events), none dynamic.
    #[test]
    fn spawned_bodies_carry_avians_markers() {
        let case = conformance::cases()
            .into_iter()
            .find(|c| c.name == "sensors_only_overlap")
            .unwrap();
        let mut app = world_of(&case);
        let mut query = app
            .world_mut()
            .query::<(&SeamBody, &RigidBody, Has<Sensor>, &Collider)>();
        let mut seen = 0;
        for (body, rigid, sensor, collider) in query.iter(app.world()) {
            seen += 1;
            let want = match body.kind {
                BodyKind::Static => (RigidBody::Static, false),
                BodyKind::Kinematic => (RigidBody::Kinematic, false),
                BodyKind::Sensor => (RigidBody::Kinematic, true),
            };
            assert_eq!((*rigid, sensor), want, "{:?}", body.id);
            if let Some(mesh) = collider.shape_scaled().as_trimesh() {
                assert!(
                    mesh.flags().contains(
                        avian3d::parry::shape::TriMeshFlags::FIX_INTERNAL_EDGES
                            | avian3d::parry::shape::TriMeshFlags::ORIENTED
                    ),
                    "{:?}",
                    mesh.flags()
                );
                assert_eq!(body.kind, BodyKind::Static);
            }
        }
        assert_eq!(seen, 3);
    }

    /// `set_pose` refuses what `ParryWorld::set_pose` refuses, by the same
    /// name: a static body, a missing one, a pose that isn't finite; then a
    /// sensor moves.
    #[test]
    fn set_pose_refuses_as_parry_does() {
        let case = conformance::cases()
            .into_iter()
            .find(|c| c.name == "sensors_only_overlap")
            .unwrap();
        let (floor, sensor) = (case.bodies[0].id, case.bodies[1].id);
        let mut app = world_of(&case);
        let mut parry = ParryWorld::new(&case.bodies).unwrap();
        let mut state =
            SystemState::<Query<(&SeamBody, &mut Position, &mut Rotation)>>::new(app.world_mut());
        let mut bodies = state.get_mut(app.world_mut()).unwrap();
        let skewed = Pose::new(Vec3::ZERO, Quat::from_xyzw(0.0, 0.0, 0.0, 2.0));
        let far = Pose::at(Vec3::new(f32::INFINITY, 0.0, 0.0));
        for (id, pose, want) in [
            (floor, Pose::at(Vec3::Y), WorldError::Static(floor)),
            (BodyId(99), Pose::IDENTITY, WorldError::Missing(BodyId(99))),
            (sensor, skewed, WorldError::BadPose(sensor)),
            (sensor, far, WorldError::BadPose(sensor)),
        ] {
            assert_eq!(set_pose(&mut bodies, id, pose), Err(want.clone()));
            assert_eq!(parry.set_pose(id, pose), Err(want));
        }
        assert_eq!(set_pose(&mut bodies, sensor, Pose::at(Vec3::X)), Ok(()));
    }

    /// `spawn_body` refuses a body `ParryWorld::new` refuses, by the same
    /// name, and spawns nothing for it.
    #[test]
    fn spawn_refuses_as_parry_does() {
        let ball = |pose: Pose, shape: Shape| Body {
            id: BodyId(4),
            kind: BodyKind::Kinematic,
            pose,
            collider: SeamCollider::Convex(shape),
        };
        let sphere = Shape::Sphere { radius: 0.5 };
        let bad = [
            ball(Pose::at(Vec3::new(0.0, f32::NAN, 0.0)), sphere),
            ball(
                Pose::new(Vec3::ZERO, Quat::from_xyzw(0.0, 0.0, 0.0, 0.5)),
                sphere,
            ),
            ball(
                Pose::IDENTITY,
                Shape::Capsule {
                    radius: 0.3,
                    half_segment: 0.0,
                },
            ),
            ball(
                Pose::IDENTITY,
                Shape::Box {
                    half_extents: Vec3::new(0.4, -0.3, 0.2),
                    margin: 0.05,
                },
            ),
            ball(
                Pose::IDENTITY,
                Shape::Cylinder {
                    radius: 0.25,
                    half_height: 0.45,
                    margin: f32::INFINITY,
                },
            ),
        ];
        let mut app = headless();
        for body in &bad {
            let want = ParryWorld::new(std::slice::from_ref(body)).err();
            assert!(want.is_some(), "{body:?}");
            let mut commands = app.world_mut().commands();
            assert_eq!(spawn_body(&mut commands, body).err(), want, "{body:?}");
            app.world_mut().flush();
        }
        app.update();
        let mut seam = app.world_mut().query::<&SeamBody>();
        assert_eq!(seam.iter(app.world()).count(), 0);
    }

    /// The plugin leaves everything but seam bodies alone: a UI-like entity
    /// with a `Transform` gets no physics component and keeps its transform
    /// through physics steps.
    #[test]
    fn other_entities_are_untouched() {
        let mut app = headless();
        let transform = Transform::from_xyz(3.0, 4.0, 0.0).with_scale(Vec3::splat(2.0));
        let other = app.world_mut().spawn(transform).id();
        for _ in 0..4 {
            app.update();
        }
        let entity = app.world().entity(other);
        assert_eq!(entity.get::<Transform>(), Some(&transform));
        assert!(entity.get::<Position>().is_none());
        assert!(entity.get::<Rotation>().is_none());
        assert!(entity.get::<ColliderAabb>().is_none());
    }

    /// Avian's constructors and guac-motion's parry shapes are the same
    /// shapes: the same AABB for each convex shape the table uses.
    #[test]
    fn colliders_match_parry_shapes() {
        use guac_motion::parry::convex_shape;
        let shapes = [
            Shape::Sphere { radius: 0.5 },
            Shape::Capsule {
                radius: 0.3,
                half_segment: 0.5,
            },
            Shape::Box {
                half_extents: Vec3::new(0.4, 0.3, 0.2),
                margin: 0.05,
            },
            Shape::Box {
                half_extents: Vec3::new(0.4, 0.3, 0.2),
                margin: 0.0,
            },
            Shape::Cylinder {
                radius: 0.25,
                half_height: 0.45,
                margin: 0.05,
            },
            Shape::Cylinder {
                radius: 0.25,
                half_height: 0.45,
                margin: 0.0,
            },
        ];
        let pose = parry_pose(Pose::new(
            Vec3::new(1.0, 2.0, 3.0),
            Quat::from_rotation_z(0.3) * Quat::from_rotation_y(1.1),
        ));
        for shape in shapes {
            let avian = convex_collider(&shape).shape_scaled().compute_aabb(&pose);
            let parry = convex_shape(&shape).compute_aabb(&pose);
            assert_eq!(
                (avian.mins, avian.maxs),
                (parry.mins, parry.maxs),
                "{shape:?}"
            );
        }
    }

    /// AG.70 and AG.71: the post-step's scenarios guac-motion runs on parry
    /// (his lean in the air, with his facing and near the ground, the float
    /// off a step until his lean turns, a still solid letting him out slowly
    /// and sliding him plain, the probe under a solid over an edge and its
    /// search through a solid's radius), each in its world on Avian's side,
    /// headless.
    #[test]
    fn the_post_step_cases_through_avian() {
        let mut failures = Vec::new();
        let mut ran = 0;
        for scenario in guac_motion::scenarios::post_step_scenarios() {
            let mut app = headless();
            let mut commands = app.world_mut().commands();
            for body in &scenario.bodies {
                spawn_body(&mut commands, body).expect(scenario.name);
            }
            app.world_mut().flush();
            app.update();
            let mut state = SystemState::<AvianSeam>::new(app.world_mut());
            let seam = state.get(app.world()).unwrap();
            if let Err(e) = (scenario.check)(&seam) {
                failures.push(format!("{}: {e}", scenario.name));
            }
            ran += 1;
        }
        assert_eq!(ran, 8);
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

    /// Level 213's mesh and the gate's door off the dump, as seam bodies:
    /// the mesh through guac-extract in memory, the door's solid from its
    /// `CollisionInfo` as the original builds it.
    fn level_and_door(dump: &std::path::Path) -> Vec<Body> {
        use guac_extract::levels;
        use guac_formats::igz::Igz;
        use guac_formats::igz::layouts::Shape as Kind;
        use guac_formats::level::Placements;
        use guac_ir::{DumpIdentity, Sha256};
        use guac_motion::solid::{SolidInfo, SolidKind, euler};
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
        let mesh = Mesh::new(
            m.vertices.iter().map(|v| Vec3::from_array(*v)).collect(),
            m.triangles.iter().map(|t| t.map(u32::from)).collect(),
            m.materials.clone(),
        )
        .unwrap();
        let placements = Placements::read(&igz).unwrap();
        let door = placements
            .actor("Breakable_Stone_Wall_Castle_206(1)")
            .expect("the gate's door");
        let c = door.collision.expect("its CollisionInfo");
        let kind = match c.shape() {
            Kind::Box => SolidKind::Box,
            other => panic!("the door is a box, not {other:?}"),
        };
        let placement = door.placement();
        let solid = SolidInfo {
            kind,
            offset: c.transform.position,
            rotation: c.transform.rotation,
            width: c.width,
            height: c.height,
            length: c.length,
        }
        .havok(placement.scale)
        .expect("a solid door");
        let at = placement.placement.init_matrix;
        vec![
            Body {
                id: BodyId(1),
                kind: BodyKind::Static,
                pose: Pose::IDENTITY,
                collider: SeamCollider::Mesh(Arc::new(mesh)),
            },
            Body {
                id: BodyId(2),
                kind: BodyKind::Kinematic,
                pose: solid.pose(Pose::new(Vec3::from(at.position), euler(at.rotation))),
                collider: SeamCollider::Convex(solid.in_world()),
            },
        ]
    }

    /// AG.70 on Avian's side (`#[ignore]`d: level 213 and the gate's door
    /// off `GUAC_GIANTS_DUMP`, AG.58's recordings under `GUAC_CAPTURE_DIR`,
    /// from the process env, else the repo's `.env.dev`): AG.58's three
    /// `w58_door` replays, from the record after the slide's frame without
    /// support, on each record's own stick, view and step through
    /// `AvianSeam` and `ParryWorld` alike: the two within 1 mm at every
    /// record, the probe stopping him within a record of theirs past the
    /// door's end and the stick held, passing as many times as theirs to 1
    /// while its slow point lies under the door's box with its radius.
    #[test]
    #[ignore]
    fn ag70_past_the_gate_doors_end_through_avian() {
        use guac_motion::hero::{Hero, Input};
        use guac_motion::parry::ParryWorld;
        use guac_motion::stick::Stick;
        use guac_motion::tuning::Tuning;
        use guac_motion::walk::{Footing, Walk};
        let dump = env_or_dotenv("GUAC_GIANTS_DUMP").expect("set GUAC_GIANTS_DUMP");
        let captures = env_or_dotenv("GUAC_CAPTURE_DIR").expect("set GUAC_CAPTURE_DIR");
        let bodies = level_and_door(std::path::Path::new(&dump));
        let parry = ParryWorld::new(&bodies).unwrap();
        let mut app = headless();
        let mut commands = app.world_mut().commands();
        for body in &bodies {
            spawn_body(&mut commands, body).unwrap();
        }
        app.world_mut().flush();
        app.update();
        let mut state = SystemState::<AvianSeam>::new(app.world_mut());
        let avian = state.get(app.world()).unwrap();
        let run = std::path::Path::new(&captures).join("BLUS30968/ag58-wall-1792597650");
        let f = |v: &serde_json::Value| v.as_f64().unwrap() as f32;
        let v3 = |v: &serde_json::Value| Vec3::new(f(&v[0]), f(&v[1]), f(&v[2]));
        let step = |r: &serde_json::Value| {
            let c = &r["clock"];
            let rate = f(&c["presentation_rate"]);
            f(&c["frame_delta"]).clamp(rate * 0.5, rate * 3.0)
                * f(&c["logic_scale"])
                * f(&c["object_scale"])
        };
        let held = |r: &serde_json::Value| f(&r["mover"]["fields"]["controller inactive"]) != 0.0;
        let supported = |r: &serde_json::Value| r["contacts"]["support"].as_u64() == Some(2);
        let mut seen = 0;
        for name in ["a/w58_door-O-00", "b/w58_door-O-00", "b/w58_door-O-01"] {
            let text = std::fs::read_to_string(run.join(name).join("recording.json")).unwrap();
            let json: serde_json::Value = serde_json::from_str(&text).unwrap();
            let r = json.as_array().unwrap();
            let k = (200..r.len()).find(|&k| !supported(&r[k])).unwrap() + 1;
            let stop = (k..r.len()).find(|&j| held(&r[j])).unwrap();
            let last = (stop..r.len()).rfind(|&j| !held(&r[j])).unwrap();
            let theirs = (stop..=last).filter(|&j| !held(&r[j])).count();
            let seed = || {
                let s = &r[k];
                let rotation = [0, 1, 2].map(|i| f(&s["hero"]["rotation"][i]));
                let mut hero = Hero::new(
                    guac_motion::scenarios::eruptor_solid(),
                    Tuning::ERUPTOR,
                    v3(&s["hero"]["position"]),
                    rotation[1],
                );
                hero.up = guac_motion::solid::euler(rotation) * Vec3::Y;
                let m = &s["mover"];
                let (h, p) = (f(&m["heading"]).to_radians(), f(&m["pitch"]).to_radians());
                hero.walk = Walk {
                    velocity: Vec3::new(p.cos() * h.sin(), p.sin(), p.cos() * h.cos())
                        * f(&m["length"]),
                    landing: None,
                };
                let g = &s["contacts"]["ground"];
                let normal = v3(&g["normal"]);
                let settled = guac_motion::ground::angle(hero.up, normal)
                    <= guac_motion::ground::LEAN_SETTLED;
                hero.footing = Footing {
                    supported: supported(s),
                    height: Some(f(&s["hero"]["position"][1]) - f(&g["location"][1])),
                    slope: if settled { hero.up } else { normal },
                    ground: Some(normal),
                };
                hero.velocity = v3(&s["hero"]["velocity"]);
                hero.held_off = held(s);
                hero.set_steps(step(&r[k - 1]), step(&r[k - 2]));
                hero
            };
            let (mut ours, mut twin) = (seed(), seed());
            let (mut ours_stop, mut flickers, mut apart) = (None, 0usize, 0.0f32);
            for j in k + 1..=last + 30 {
                let (a, b) = (&r[j - 1], &r[j]);
                let stick = Stick::new(f(&b["stick"][0]), f(&b["stick"][1]));
                let input = Input::stick(stick, f(&a["camera"]["rotation"][1]));
                ours.step(&avian, &input, step(a), &[]);
                twin.step(&parry, &input, step(a), &[]);
                apart = apart.max((ours.pivot - twin.pivot).length());
                match (ours_stop, ours.held_off) {
                    (None, true) => ours_stop = Some(j),
                    (Some(_), false) => flickers += 1,
                    _ => {}
                }
            }
            eprintln!(
                "{name} from record {k}: stops on Avian at {ours_stop:?}, theirs {stop}; {flickers} passes, theirs {theirs}; Avian and parry {apart:.2e} m apart at worst"
            );
            assert!(apart < 1e-3, "{name}: Avian and parry {apart} m apart");
            assert!(
                ours_stop.is_some_and(|s| s.abs_diff(stop) <= 1),
                "{name}: {ours_stop:?} against {stop}"
            );
            assert!(
                flickers.abs_diff(theirs) <= 1,
                "{name}: {flickers} against {theirs}"
            );
            seen += 1;
        }
        assert_eq!(seen, 3);
    }
}
