//! The seam over parry3d alone: what every controller test runs on, without
//! Bevy. Shapes are built as Avian builds them (`Collider::capsule`'s segment
//! order, its `FIX_INTERNAL_EDGES`, which also sets `ORIENTED`), and each query
//! makes the parry call Avian's `SpatialQuery` makes, so the two sides answer
//! the conformance table alike; the broad phase is a plain AABB test per body,
//! enough for one level mesh and its placements.

use std::sync::Arc;

use parry3d::bounding_volume::{Aabb, BoundingVolume};
use parry3d::math::Pose as PPose;
use parry3d::query::{
    self, ContactManifold, DefaultQueryDispatcher, PersistentQueryDispatcher, Ray, RayCast,
    ShapeCastOptions,
};
use parry3d::shape::{SharedShape, TriMeshFlags};

use crate::seam::{
    Body, BodyId, BodyKind, CAST_SLOP, Cast, Collider, ContactPlane, Mesh, PhysicsSeam, Pose,
    RayHit, Shape, Vec3, sort_planes,
};

/// Avian's `TrimeshFlags::FIX_INTERNAL_EDGES` in parry's terms: parry's own
/// flag (internal edges, duplicate vertices merged) plus `ORIENTED`.
pub const MESH_FLAGS: TriMeshFlags = TriMeshFlags::FIX_INTERNAL_EDGES.union(TriMeshFlags::ORIENTED);

/// The parry shape for a seam shape, built the way Avian's constructors build
/// it (a capsule from +Y to −Y, a rounded box or cylinder when it has a
/// margin).
pub fn convex_shape(shape: &Shape) -> SharedShape {
    match *shape {
        Shape::Sphere { radius } => SharedShape::ball(radius),
        Shape::Capsule {
            radius,
            half_segment,
        } => SharedShape::capsule(Vec3::Y * half_segment, Vec3::NEG_Y * half_segment, radius),
        Shape::Box {
            half_extents: h,
            margin,
        } if margin > 0.0 => SharedShape::round_cuboid(h.x, h.y, h.z, margin),
        Shape::Box {
            half_extents: h, ..
        } => SharedShape::cuboid(h.x, h.y, h.z),
        Shape::Cylinder {
            radius,
            half_height,
            margin,
        } if margin > 0.0 => SharedShape::round_cylinder(half_height, radius, margin),
        Shape::Cylinder {
            radius,
            half_height,
            ..
        } => SharedShape::cylinder(half_height, radius),
    }
}

/// The parry mesh, `MESH_FLAGS` set. Merging duplicate vertices rewrites the
/// index buffer but keeps the triangles in order, so a triangle's index (and
/// its material) is the one the mesh was built with.
pub fn mesh_shape(mesh: &Mesh) -> SharedShape {
    SharedShape::trimesh_with_flags(
        mesh.vertices().to_vec(),
        mesh.triangles().to_vec(),
        MESH_FLAGS,
    )
    .expect("Mesh::new refuses what parry can't build")
}

pub fn parry_pose(pose: Pose) -> PPose {
    PPose::from_parts(pose.position, pose.rotation)
}

struct Entry {
    id: BodyId,
    kind: BodyKind,
    pose: PPose,
    shape: SharedShape,
    mesh: Option<Arc<Mesh>>,
    aabb: Aabb,
}

impl Entry {
    fn material(&self, triangle: Option<u32>) -> Option<u8> {
        self.mesh
            .as_ref()
            .zip(triangle)
            .and_then(|(m, t)| m.material(t))
    }

    fn triangle(&self, subshape: u32) -> Option<u32> {
        self.mesh.is_some().then_some(subshape)
    }
}

/// A world of seam bodies, queried with parry directly.
pub struct ParryWorld {
    entries: Vec<Entry>,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum WorldError {
    #[error("body {0:?} twice")]
    Duplicate(BodyId),
    #[error("body {0:?} has a shape with a size that isn't finite and positive")]
    BadShape(BodyId),
    #[error("body {0:?} has a pose that isn't finite")]
    BadPose(BodyId),
    #[error("no body {0:?}")]
    Missing(BodyId),
    #[error("body {0:?} is static")]
    Static(BodyId),
}

/// A finite position and a unit rotation, or `BadPose`: what every side
/// refuses before a body is placed or moved.
pub fn check_pose(id: BodyId, pose: Pose) -> Result<(), WorldError> {
    let finite =
        pose.position.is_finite() && pose.rotation.is_finite() && pose.rotation.is_normalized();
    if finite {
        Ok(())
    } else {
        Err(WorldError::BadPose(id))
    }
}

/// One body on its own (its pose, and a convex shape's sizes; `Mesh::new`
/// already refused a bad mesh): what every side refuses before building it.
pub fn check_body(body: &Body) -> Result<(), WorldError> {
    check_pose(body.id, body.pose)?;
    match &body.collider {
        Collider::Convex(shape) if !shape.is_valid() => Err(WorldError::BadShape(body.id)),
        _ => Ok(()),
    }
}

impl ParryWorld {
    pub fn new(bodies: &[Body]) -> Result<ParryWorld, WorldError> {
        let mut entries: Vec<Entry> = Vec::with_capacity(bodies.len());
        for body in bodies {
            if entries.iter().any(|e| e.id == body.id) {
                return Err(WorldError::Duplicate(body.id));
            }
            check_body(body)?;
            let (shape, mesh) = match &body.collider {
                Collider::Convex(shape) => (convex_shape(shape), None),
                Collider::Mesh(mesh) => (mesh_shape(mesh), Some(mesh.clone())),
            };
            let pose = parry_pose(body.pose);
            entries.push(Entry {
                id: body.id,
                kind: body.kind,
                aabb: shape.compute_aabb(&pose),
                pose,
                shape,
                mesh,
            });
        }
        Ok(ParryWorld { entries })
    }

    /// Moves a kinematic body or a sensor; queries see it there at once.
    pub fn set_pose(&mut self, id: BodyId, pose: Pose) -> Result<(), WorldError> {
        let entry = self
            .entries
            .iter_mut()
            .find(|e| e.id == id)
            .ok_or(WorldError::Missing(id))?;
        if entry.kind == BodyKind::Static {
            return Err(WorldError::Static(id));
        }
        check_pose(id, pose)?;
        entry.pose = parry_pose(pose);
        entry.aabb = entry.shape.compute_aabb(&entry.pose);
        Ok(())
    }

    fn solids<'a>(&'a self, exclude: &'a [BodyId]) -> impl Iterator<Item = &'a Entry> + 'a {
        self.entries
            .iter()
            .filter(move |e| e.kind.is_solid() && !exclude.contains(&e.id))
    }
}

/// A solid as [`planes_against`] reads it, from either side's storage.
pub struct Solid<'a> {
    pub body: BodyId,
    pub pose: PPose,
    pub shape: &'a SharedShape,
    /// The seam mesh behind a trimesh shape, for its materials.
    pub mesh: Option<&'a Mesh>,
}

/// The planes one solid gives a shape: one per manifold (per triangle on a
/// mesh), its deepest point. The same parry call Avian's
/// `contact_query::contact_manifolds` makes, keeping the triangle that
/// Avian's wrapper drops.
pub fn planes_against(
    solid: &Solid,
    shape: &SharedShape,
    at: &PPose,
    distance: f32,
    out: &mut Vec<ContactPlane>,
) {
    let Solid {
        body,
        pose: ref body_pose,
        shape: body_shape,
        mesh,
    } = *solid;
    let pos12 = body_pose.inv_mul(at);
    let mut manifolds: Vec<ContactManifold<(), ()>> = Vec::new();
    if DefaultQueryDispatcher
        .contact_manifolds(
            &pos12,
            body_shape.as_ref(),
            shape.as_ref(),
            distance,
            &mut manifolds,
            &mut None,
        )
        .is_err()
    {
        return;
    }
    for manifold in &manifolds {
        let Some(deepest) = manifold
            .contacts()
            .iter()
            .min_by(|a, b| a.dist.total_cmp(&b.dist))
        else {
            continue;
        };
        let sub = manifold.subshape_pos1().copied().unwrap_or(PPose::IDENTITY);
        let local_normal = (sub.rotation * manifold.local_n1).normalize();
        let normal = body_pose.rotation * local_normal;
        if !normal.is_normalized() {
            continue;
        }
        let local_point = sub.transform_point(deepest.local_p1);
        let point = body_pose.transform_point(local_point);
        let triangle = mesh.map(|_| manifold.subshape1);
        if let (Some(mesh), Some(t)) = (mesh, triangle)
            && beyond_a_neighbour(mesh, t, local_point, local_normal)
        {
            continue;
        }
        out.push(ContactPlane {
            body,
            triangle,
            material: mesh.zip(triangle).and_then(|(m, t)| m.material(t)),
            normal,
            point,
            distance: deepest.dist,
        });
    }
}

/// How near an edge a contact point is on it, metres (f32 at the level's
/// hundred-metre coordinates resolves about 1e-5).
const ON_EDGE: f32 = 1.0e-4;

/// A contact on triangle `t` at an edge it shares, whose normal lies outside
/// the arc between its face's normal and its neighbour's, past the
/// neighbour's: the shape is over the neighbour's face, which gives the plane
/// itself, so this one is dropped, as the original's welding snaps it to the
/// neighbour's (and the dedupe would then merge the two). Parry's own fix
/// tests the normal against the cone of whichever edge pseudo-normal is
/// nearest, not the touched edge's, so it passes some: on level 213 at the
/// start's convex 1.4° crease (triangles 2672 and 2428), a plane 2° past
/// the flat side's that lifted the hero 3 mm (AG.16's replays). Points in
/// the mesh's frame. A vertex contact meets the test once per edge through it.
fn beyond_a_neighbour(mesh: &Mesh, t: u32, point: Vec3, normal: Vec3) -> bool {
    let (Some(face), Some(corners)) = (mesh.face_normal(t), mesh.corners(t)) else {
        return false;
    };
    if normal.dot(face) > 1.0 - 1.0e-7 {
        return false;
    }
    (0..3).any(|e| {
        let (a, b) = (corners[e], corners[(e + 1) % 3]);
        let ab = b - a;
        let along = ((point - a).dot(ab) / ab.length_squared()).clamp(0.0, 1.0);
        if (a + ab * along - point).length() > ON_EDGE {
            return false;
        }
        let Some(other) = mesh.neighbour(t, e).and_then(|u| mesh.face_normal(u)) else {
            return false;
        };
        let axis = ab.normalize();
        let r = normal - axis * normal.dot(axis);
        let c = face.dot(other);
        let det = 1.0 - c * c;
        if c <= 0.0 || det < 1.0e-10 {
            return false;
        }
        // r = α face + β other; α < 0 is past the neighbour's normal.
        let alpha = (r.dot(face) - c * r.dot(other)) / det;
        alpha < -1.0e-4
    })
}

/// The cast options both sides use: Avian's `ShapeCastConfig` with
/// `ignore_origin_penetration` (a start contact the motion leaves or slides
/// along is no hit) and `compute_contact_on_penetration`.
pub fn cast_options(max_distance: f32) -> ShapeCastOptions {
    ShapeCastOptions {
        max_time_of_impact: max_distance,
        target_distance: 0.0,
        stop_at_penetration: false,
        compute_impact_geometry_on_penetration: true,
    }
}

/// Unit direction and length of a motion, `None` for a zero or non-finite one.
pub fn split_motion(motion: Vec3) -> Option<(Vec3, f32)> {
    let length = motion.length();
    (length.is_finite() && length > 0.0).then(|| (motion / length, length))
}

impl PhysicsSeam for ParryWorld {
    fn cast(
        &self,
        shape: &Shape,
        from: Pose,
        motion: Vec3,
        contact_distance: f32,
        exclude: &[BodyId],
    ) -> Option<Cast> {
        let (dir, length) = split_motion(motion)?;
        let query = convex_shape(shape);
        let start = parry_pose(from);
        let end = parry_pose(Pose::new(from.position + motion, from.rotation));
        let swept = query.compute_aabb(&start).merged(&query.compute_aabb(&end));
        let mut best = length;
        let mut hit = false;
        for entry in self.solids(exclude) {
            if !entry.aabb.intersects(&swept) {
                continue;
            }
            let Ok(Some(found)) = query::cast_shapes(
                &entry.pose,
                Vec3::ZERO,
                entry.shape.as_ref(),
                &start,
                dir,
                query.as_ref(),
                cast_options(length),
            ) else {
                continue;
            };
            if found.time_of_impact < best || !hit {
                best = found.time_of_impact;
                hit = true;
            }
        }
        if !hit {
            return None;
        }
        let at = Pose::new(from.position + dir * best, from.rotation);
        Some(Cast {
            fraction: (best / length).clamp(0.0, 1.0),
            distance: best,
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
        let query = convex_shape(shape);
        let pose = parry_pose(at);
        let reach = query.compute_aabb(&pose).loosened(distance.max(0.0));
        let mut planes = Vec::new();
        for entry in self.solids(exclude) {
            if entry.aabb.intersects(&reach) {
                let solid = Solid {
                    body: entry.id,
                    pose: entry.pose,
                    shape: &entry.shape,
                    mesh: entry.mesh.as_deref(),
                };
                planes_against(&solid, &query, &pose, distance.max(0.0), &mut planes);
            }
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
        let ray = Ray::new(origin, dir);
        let mut best: Option<RayHit> = None;
        for entry in self.solids(exclude) {
            let reach = best.map_or(max_distance, |b| b.distance);
            if !entry.aabb.intersects_local_ray(&ray, reach) {
                continue;
            }
            let Some(found) = entry
                .shape
                .cast_ray_and_get_normal(&entry.pose, &ray, reach, false)
            else {
                continue;
            };
            if best.is_none_or(|b| found.time_of_impact < b.distance) {
                let triangle = entry.triangle(found.subshape);
                best = Some(RayHit {
                    body: entry.id,
                    triangle,
                    material: entry.material(triangle),
                    distance: found.time_of_impact,
                    point: origin + dir * found.time_of_impact,
                    normal: found.normal,
                });
            }
        }
        best
    }

    fn overlaps(&self, shape: &Shape, at: Pose, exclude: &[BodyId]) -> Vec<BodyId> {
        let query = convex_shape(shape);
        let pose = parry_pose(at);
        let aabb = query.compute_aabb(&pose);
        let mut ids: Vec<BodyId> = self
            .entries
            .iter()
            .filter(|e| !exclude.contains(&e.id) && e.aabb.intersects(&aabb))
            .filter(|e| {
                query::intersection_test(&e.pose, e.shape.as_ref(), &pose, query.as_ref())
                    .is_ok_and(|i| i.intersecting)
            })
            .map(|e| e.id)
            .collect();
        ids.sort();
        ids
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::conformance::{self, Case};
    use crate::seam::{Mesh, MeshError, Quat};

    fn world(case: &Case) -> ParryWorld {
        let mut world = ParryWorld::new(&case.bodies).expect(case.name);
        for &(id, pose) in &case.moves {
            world.set_pose(id, pose).expect(case.name);
        }
        world
    }

    #[test]
    fn the_table() {
        let mut failures = Vec::new();
        for case in conformance::cases() {
            if let Err(e) = conformance::run(&world(&case), &case) {
                failures.push(e);
            }
        }
        assert!(failures.is_empty(), "{}", failures.join("\n\n"));
    }

    #[test]
    fn same_world_twice_agrees_bit_for_bit() {
        for case in conformance::cases() {
            let a = conformance::run(&world(&case), &case).unwrap();
            let b = conformance::run(&world(&case), &case).unwrap();
            assert_eq!(a, b, "{}", case.name);
        }
    }

    /// The seam case sees the fix: the same floor built without internal-edge
    /// fixing gives a sphere 5 mm inside it, just short of the seam, the far
    /// triangle's edge normal, tilted against its motion along +x.
    #[test]
    fn unfixed_edges_tilt_the_plane() {
        let (v, t) = (
            vec![
                Vec3::new(-4.0, 0.0, -4.0),
                Vec3::new(-4.0, 0.0, 4.0),
                Vec3::new(4.0, 0.0, 4.0),
                Vec3::new(4.0, 0.0, -4.0),
            ],
            vec![[0, 1, 2], [0, 2, 3]],
        );
        let sphere = convex_shape(&Shape::Sphere { radius: 0.5 });
        let at = parry_pose(Pose::at(Vec3::new(-0.05, 0.495, 0.0)));
        let tilt = |flags| {
            let mesh = SharedShape::trimesh_with_flags(v.clone(), t.clone(), flags).unwrap();
            let mut planes = Vec::new();
            let solid = Solid {
                body: BodyId(1),
                pose: PPose::IDENTITY,
                shape: &mesh,
                mesh: None,
            };
            planes_against(&solid, &sphere, &at, 0.05, &mut planes);
            planes.iter().map(|p| p.normal.x).fold(0.0f32, f32::min)
        };
        assert!(
            tilt(TriMeshFlags::empty()) < -0.05,
            "{}",
            tilt(TriMeshFlags::empty())
        );
        assert!(tilt(MESH_FLAGS).abs() < 1e-6, "{}", tilt(MESH_FLAGS));
    }

    fn ball(id: u32, kind: BodyKind) -> Body {
        Body {
            id: BodyId(id),
            kind,
            pose: Pose::IDENTITY,
            collider: Collider::Convex(Shape::Sphere { radius: 1.0 }),
        }
    }

    #[test]
    fn world_refuses_by_name() {
        let two = [ball(1, BodyKind::Static), ball(1, BodyKind::Kinematic)];
        assert!(matches!(
            ParryWorld::new(&two),
            Err(WorldError::Duplicate(BodyId(1)))
        ));
        let mut bad = ball(2, BodyKind::Static);
        bad.collider = Collider::Convex(Shape::Capsule {
            radius: 0.5,
            half_segment: f32::NAN,
        });
        assert!(matches!(
            ParryWorld::new(&[bad]),
            Err(WorldError::BadShape(BodyId(2)))
        ));
        let mut far = ball(3, BodyKind::Static);
        far.pose.position.x = f32::INFINITY;
        assert!(matches!(
            ParryWorld::new(&[far]),
            Err(WorldError::BadPose(BodyId(3)))
        ));
        let mut world =
            ParryWorld::new(&[ball(1, BodyKind::Static), ball(2, BodyKind::Sensor)]).unwrap();
        assert_eq!(
            world.set_pose(BodyId(1), Pose::IDENTITY),
            Err(WorldError::Static(BodyId(1)))
        );
        assert_eq!(
            world.set_pose(BodyId(9), Pose::IDENTITY),
            Err(WorldError::Missing(BodyId(9)))
        );
        let skewed = Pose::new(Vec3::ZERO, Quat::from_xyzw(0.0, 0.0, 0.0, 2.0));
        assert_eq!(
            world.set_pose(BodyId(2), skewed),
            Err(WorldError::BadPose(BodyId(2)))
        );
        assert_eq!(world.set_pose(BodyId(2), Pose::at(Vec3::X)), Ok(()));
    }

    #[test]
    fn mesh_refuses_by_name() {
        let v = vec![Vec3::ZERO, Vec3::X, Vec3::Z];
        assert_eq!(Mesh::new(v.clone(), vec![], vec![]), Err(MeshError::Empty));
        assert_eq!(
            Mesh::new(v.clone(), vec![[0, 1, 2]], vec![]),
            Err(MeshError::Materials {
                triangles: 1,
                materials: 0
            })
        );
        assert_eq!(
            Mesh::new(v.clone(), vec![[0, 1, 3]], vec![0]),
            Err(MeshError::Index {
                triangle: 0,
                index: 3,
                vertices: 3
            })
        );
        let mut nan = v.clone();
        nan[1].y = f32::NAN;
        assert_eq!(
            Mesh::new(nan, vec![[0, 1, 2]], vec![0]),
            Err(MeshError::NotFinite(1))
        );
        assert!(Mesh::new(v, vec![[0, 2, 1]], vec![9]).is_ok());
    }
}
