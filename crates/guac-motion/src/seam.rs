//! The physics seam (openspec AG, design decision 3): the four queries the
//! hero's controller makes of the world, as ag-motion section 1 read them off
//! the original's character proxy (a linear cast with the contact planes where
//! it stops, closest points within a distance, rays, phantom overlaps), and the
//! bodies they run against.
//!
//! The original's world is query-only (no dynamic body in the tutorial's live
//! `hkpWorld`), so a body here is static (the level's one triangle mesh),
//! kinematic (a placement's solid, moved by its owner) or a sensor (overlaps
//! only: casts, contacts and rays pass through it).
//!
//! Two implementations answer the same table ([`crate::conformance`]):
//! [`crate::parry::ParryWorld`] here and guacamole's Avian adapter. Units are
//! meters, positions world space, normals unit length.

use std::sync::Arc;

pub use glam::{Quat, Vec3};

/// Where a body or a query shape sits: its centre and orientation.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Pose {
    pub position: Vec3,
    pub rotation: Quat,
}

impl Pose {
    pub const IDENTITY: Pose = Pose {
        position: Vec3::ZERO,
        rotation: Quat::IDENTITY,
    };

    pub fn at(position: Vec3) -> Pose {
        Pose {
            position,
            rotation: Quat::IDENTITY,
        }
    }

    pub fn new(position: Vec3, rotation: Quat) -> Pose {
        Pose { position, rotation }
    }
}

/// A body's handle, chosen by whoever builds the world (the level is one, each
/// placement another). Ordered, so results sort the same way on every side.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct BodyId(pub u32);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BodyKind {
    /// Never moves: the level's collision mesh.
    Static,
    /// Solid and moved by its owner between queries: a door, a push block.
    Kinematic,
    /// Not solid: only [`PhysicsSeam::overlaps`] sees it. Movable, like a
    /// kinematic body.
    Sensor,
}

impl BodyKind {
    pub fn is_solid(self) -> bool {
        self != BodyKind::Sensor
    }
}

/// The convex shapes `CollisionInfo` names (types 1-4), centred on their pose,
/// long axis along local Y. `margin` is Havok's convex radius: the box or
/// cylinder rounded by it, so the outer size is the inner one plus `margin`
/// all round (the original's 0.05 m on a box, ag-motion section 1).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Shape {
    Sphere {
        radius: f32,
    },
    /// A segment of half length `half_segment` along Y, swept by `radius`.
    Capsule {
        radius: f32,
        half_segment: f32,
    },
    Box {
        half_extents: Vec3,
        margin: f32,
    },
    Cylinder {
        radius: f32,
        half_height: f32,
        margin: f32,
    },
}

impl Shape {
    /// Every size finite and positive (a margin may be 0).
    pub fn is_valid(&self) -> bool {
        let pos = |v: f32| v.is_finite() && v > 0.0;
        let margin = |v: f32| v.is_finite() && v >= 0.0;
        match *self {
            Shape::Sphere { radius } => pos(radius),
            Shape::Capsule {
                radius,
                half_segment,
            } => pos(radius) && pos(half_segment),
            Shape::Box {
                half_extents,
                margin: m,
            } => pos(half_extents.x) && pos(half_extents.y) && pos(half_extents.z) && margin(m),
            Shape::Cylinder {
                radius,
                half_height,
                margin: m,
            } => pos(radius) && pos(half_height) && margin(m),
        }
    }
}

/// A triangle mesh with one material byte per triangle (the level's
/// `tfbSimpleMeshShape`). A triangle's front is its counter-clockwise side
/// seen from outside, (b − a) × (c − a); internal edges are fixed, so a shape
/// sliding across the seam between two triangles gets the faces' normal there,
/// never the edge's. Fixing them makes contacts one-sided: a shape behind a
/// triangle gets no plane from it, while casts and rays see both sides, so
/// the winding decides which side holds a shape up.
#[derive(Clone, Debug, PartialEq)]
pub struct Mesh {
    vertices: Vec<Vec3>,
    triangles: Vec<[u32; 3]>,
    materials: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum MeshError {
    #[error("a mesh needs at least one triangle")]
    Empty,
    #[error("{materials} materials for {triangles} triangles")]
    Materials { triangles: usize, materials: usize },
    #[error("triangle {triangle} names vertex {index} of {vertices}")]
    Index {
        triangle: usize,
        index: u32,
        vertices: usize,
    },
    #[error("vertex {0} isn't finite")]
    NotFinite(usize),
}

impl Mesh {
    pub fn new(
        vertices: Vec<Vec3>,
        triangles: Vec<[u32; 3]>,
        materials: Vec<u8>,
    ) -> Result<Mesh, MeshError> {
        if triangles.is_empty() {
            return Err(MeshError::Empty);
        }
        if materials.len() != triangles.len() {
            return Err(MeshError::Materials {
                triangles: triangles.len(),
                materials: materials.len(),
            });
        }
        if let Some(i) = vertices.iter().position(|v| !v.is_finite()) {
            return Err(MeshError::NotFinite(i));
        }
        for (t, tri) in triangles.iter().enumerate() {
            if let Some(&index) = tri.iter().find(|&&i| i as usize >= vertices.len()) {
                return Err(MeshError::Index {
                    triangle: t,
                    index,
                    vertices: vertices.len(),
                });
            }
        }
        Ok(Mesh {
            vertices,
            triangles,
            materials,
        })
    }

    pub fn vertices(&self) -> &[Vec3] {
        &self.vertices
    }

    pub fn triangles(&self) -> &[[u32; 3]] {
        &self.triangles
    }

    pub fn materials(&self) -> &[u8] {
        &self.materials
    }

    /// The material of triangle `index`, in the order the mesh was built.
    pub fn material(&self, index: u32) -> Option<u8> {
        self.materials.get(index as usize).copied()
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum Collider {
    Convex(Shape),
    Mesh(Arc<Mesh>),
}

#[derive(Clone, Debug, PartialEq)]
pub struct Body {
    pub id: BodyId,
    pub kind: BodyKind,
    pub pose: Pose,
    pub collider: Collider,
}

/// One solid's closest point to a query shape: a plane the shape mustn't cross.
/// A convex body gives at most one, a mesh one per triangle within reach (the
/// deepest point of each), as Havok's closest-point collector gives one point
/// per shape pair.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ContactPlane {
    pub body: BodyId,
    /// The mesh triangle, as the mesh was built; `None` on a convex body.
    pub triangle: Option<u32>,
    /// The triangle's material byte; `None` on a convex body.
    pub material: Option<u8>,
    /// Out of the body, towards the query shape.
    pub normal: Vec3,
    /// On the body's surface.
    pub point: Vec3,
    /// The gap along `normal`, negative when the shape is inside.
    pub distance: f32,
}

/// Where a cast stopped.
#[derive(Clone, Debug, PartialEq)]
pub struct Cast {
    /// Of the motion asked for, in [0, 1].
    pub fraction: f32,
    /// The same in meters.
    pub distance: f32,
    /// Every solid within the contact distance asked for at the stopping pose,
    /// sorted by body then triangle. The body the cast hit is among them
    /// unless it hit a mesh triangle from behind, which gives no plane.
    pub planes: Vec<ContactPlane>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RayHit {
    pub body: BodyId,
    pub triangle: Option<u32>,
    pub material: Option<u8>,
    /// Along the normalized direction, in meters.
    pub distance: f32,
    pub point: Vec3,
    pub normal: Vec3,
}

/// The cast's own reach: whatever it hit is at most this far from the shape
/// at the stopping pose (parry's cast converges to well inside it), so the
/// planes there always include it, even when the caller asks for less.
pub const CAST_SLOP: f32 = 1.0e-3;

/// The queries. Each ignores the bodies in `exclude` (the caller's own).
pub trait PhysicsSeam {
    /// Sweeps `shape` from `from` along `motion` without turning it and stops
    /// at the first solid it moves into. A solid it starts touching or inside
    /// stops it at fraction 0 when the motion goes into that solid's surface
    /// and not when it leaves it (the start's contacts are
    /// [`PhysicsSeam::contacts`]'); exactly along that surface, either can
    /// happen, the sign of float noise in the contact normal deciding, so a
    /// controller keeps a gap (the original's 0.01 m snap, Havok's keep
    /// distance). Mesh edges aren't welded here as they are in `contacts`: a
    /// shape inside or touching the floor can stop on the next triangle's edge.
    /// `None` when nothing is in the way, or the motion is zero.
    fn cast(
        &self,
        shape: &Shape,
        from: Pose,
        motion: Vec3,
        contact_distance: f32,
        exclude: &[BodyId],
    ) -> Option<Cast>;

    /// Every solid within `distance` of `shape` at `at` (inside it included),
    /// sorted by body then triangle.
    fn contacts(
        &self,
        shape: &Shape,
        at: Pose,
        distance: f32,
        exclude: &[BodyId],
    ) -> Vec<ContactPlane>;

    /// The first solid surface along the ray, both sides of a triangle. A ray
    /// starting inside a convex body reports where it leaves it.
    fn ray(
        &self,
        origin: Vec3,
        direction: Vec3,
        max_distance: f32,
        exclude: &[BodyId],
    ) -> Option<RayHit>;

    /// Every body, sensors included, that `shape` at `at` touches or overlaps,
    /// sorted.
    fn overlaps(&self, shape: &Shape, at: Pose, exclude: &[BodyId]) -> Vec<BodyId>;
}

/// The order every side reports planes in.
pub fn sort_planes(planes: &mut [ContactPlane]) {
    planes.sort_by_key(|p| (p.body, p.triangle));
}
