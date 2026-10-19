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

#![cfg_attr(
    not(test),
    expect(
        dead_code,
        reason = "AG.16's controller and AG.21's level are the seam's first callers; until then its tests are"
    )
)]

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
use guac_motion::parry::{Solid, parry_pose, planes_against, split_motion};
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

/// Spawns a seam body: a static rigid body, a kinematic one, or a kinematic
/// sensor. The `Transform` is there for Avian's collider scale (read off the
/// `GlobalTransform` on insert), never as the body's pose.
pub(crate) fn spawn_body(commands: &mut Commands, body: &Body) -> Entity {
    let (collider, mesh) = match &body.collider {
        SeamCollider::Convex(shape) => (convex_collider(shape), None),
        SeamCollider::Mesh(mesh) => (mesh_collider(mesh), Some(mesh.clone())),
    };
    let rigid = match body.kind {
        BodyKind::Static => RigidBody::Static,
        BodyKind::Kinematic | BodyKind::Sensor => RigidBody::Kinematic,
    };
    let mut entity = commands.spawn((
        SeamBody {
            id: body.id,
            kind: body.kind,
            mesh,
        },
        rigid,
        collider,
        Position(body.pose.position),
        Rotation(body.pose.rotation),
        Transform::default(),
    ));
    if body.kind == BodyKind::Sensor {
        entity.insert(Sensor);
    }
    entity.id()
}

/// Moves a kinematic body or sensor. Queries see it there once the physics
/// schedule has run (it refits the collider trees); a static body doesn't
/// move.
pub(crate) fn set_pose(
    bodies: &mut Query<(&SeamBody, &mut Position, &mut Rotation)>,
    id: BodyId,
    pose: Pose,
) -> bool {
    for (body, mut position, mut rotation) in bodies.iter_mut() {
        if body.id == id && body.kind != BodyKind::Static {
            position.0 = pose.position;
            rotation.0 = pose.rotation;
            return true;
        }
    }
    false
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
        let hit = self.spatial.cast_ray_predicate(
            origin,
            dir3(dir),
            max_distance,
            false,
            &SpatialQueryFilter::default(),
            &|entity| self.solid(entity, exclude),
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
            spawn_body(&mut commands, body);
        }
        app.world_mut().flush();
        app.update();
        if !case.moves.is_empty() {
            let mut state = SystemState::<Query<(&SeamBody, &mut Position, &mut Rotation)>>::new(
                app.world_mut(),
            );
            let mut bodies = state.get_mut(app.world_mut()).unwrap();
            for &(id, pose) in &case.moves {
                assert!(set_pose(&mut bodies, id, pose), "{}: {id:?}", case.name);
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
        assert!(set_pose(&mut bodies, door.0, door.1));
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

    /// A static body stays put whatever `set_pose` asks.
    #[test]
    fn a_static_body_doesnt_move() {
        let case = conformance::cases().remove(0);
        let mut app = world_of(&case);
        let mut state =
            SystemState::<Query<(&SeamBody, &mut Position, &mut Rotation)>>::new(app.world_mut());
        let mut bodies = state.get_mut(app.world_mut()).unwrap();
        assert!(!set_pose(&mut bodies, case.bodies[0].id, Pose::at(Vec3::Y)));
        assert!(!set_pose(&mut bodies, BodyId(99), Pose::IDENTITY));
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
}
