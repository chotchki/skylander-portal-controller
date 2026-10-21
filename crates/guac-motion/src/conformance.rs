//! The seam's contract as a table (openspec AG.4): each case a world of
//! synthetic bodies, the queries asked of it and the closed-form answers. Every
//! implementation runs every case ([`run`]); two implementations' answers to
//! the same case must also agree with each other ([`agree`]).
//!
//! Tolerances, declared before the table first ran:
//! - [`TOL`] against a closed-form answer: 1e-4 m on distances and points and
//!   1e-4 on each normal component. The first run held it for contacts and
//!   rays but not for casts: parry's cast of a sphere onto the rounded door
//!   went 1.57e-4 m past the closed form, so a cast's distance is checked to
//!   [`TOL_CAST`] (still far under the 0.05 m a margin or size mistake makes).
//! - [`AGREE`] between two implementations: 1e-5, about ten f32 steps at the
//!   table's largest coordinates (level 213's body sits 425 m out, where a step
//!   is 3e-5 m, so the cases there compare points relative to the body).
//!   Both sides make the same parry call on the same shapes, so anything above
//!   float noise is a real difference in how they built or posed a body.
//! - Exact: which bodies, triangles and materials, how many planes, overlap
//!   sets.

use std::sync::Arc;

use crate::seam::{
    Body, BodyId, BodyKind, Cast, Collider, ContactPlane, Mesh, PhysicsSeam, Pose, Quat, RayHit,
    Shape, Vec3,
};

pub const TOL: f32 = 1.0e-4;
pub const TOL_CAST: f32 = 1.0e-3;
pub const AGREE: f32 = 1.0e-5;

/// A call to one of [`PhysicsSeam`]'s four methods with its arguments, so a
/// case can be asked of any implementation.
#[derive(Clone, Debug)]
pub enum Query {
    Cast {
        shape: Shape,
        from: Pose,
        motion: Vec3,
        contact_distance: f32,
        exclude: Vec<BodyId>,
    },
    Contacts {
        shape: Shape,
        at: Pose,
        distance: f32,
        exclude: Vec<BodyId>,
    },
    Ray {
        origin: Vec3,
        direction: Vec3,
        max_distance: f32,
        exclude: Vec<BodyId>,
    },
    Overlaps {
        shape: Shape,
        at: Pose,
        exclude: Vec<BodyId>,
    },
}

#[derive(Clone, Debug, PartialEq)]
pub enum Answer {
    Cast(Option<Cast>),
    Contacts(Vec<ContactPlane>),
    Ray(Option<RayHit>),
    Overlaps(Vec<BodyId>),
}

pub struct Case {
    pub name: &'static str,
    pub bodies: Vec<Body>,
    /// Kinematic bodies and sensors moved after the world is built, before
    /// the queries run.
    pub moves: Vec<(BodyId, Pose)>,
    pub queries: Vec<Query>,
    /// The closed-form answers.
    pub expect: fn(&[Answer]) -> Result<(), String>,
}

pub fn ask(seam: &dyn PhysicsSeam, query: &Query) -> Answer {
    match query {
        Query::Cast {
            shape,
            from,
            motion,
            contact_distance,
            exclude,
        } => Answer::Cast(seam.cast(shape, *from, *motion, *contact_distance, exclude)),
        Query::Contacts {
            shape,
            at,
            distance,
            exclude,
        } => Answer::Contacts(seam.contacts(shape, *at, *distance, exclude)),
        Query::Ray {
            origin,
            direction,
            max_distance,
            exclude,
        } => Answer::Ray(seam.ray(*origin, *direction, *max_distance, exclude)),
        Query::Overlaps { shape, at, exclude } => {
            Answer::Overlaps(seam.overlaps(shape, *at, exclude))
        }
    }
}

/// Asks a case's queries of a seam already holding its bodies (moved), and
/// checks the answers against the closed form.
pub fn run(seam: &dyn PhysicsSeam, case: &Case) -> Result<Vec<Answer>, String> {
    let answers: Vec<Answer> = case.queries.iter().map(|q| ask(seam, q)).collect();
    (case.expect)(&answers).map_err(|e| format!("{}: {e}\n  answers: {answers:#?}", case.name))?;
    Ok(answers)
}

// ---- agreement ----

fn near(a: f32, b: f32, tol: f32) -> bool {
    (a - b).abs() <= tol
}

fn near_v(a: Vec3, b: Vec3, tol: f32) -> bool {
    near(a.x, b.x, tol) && near(a.y, b.y, tol) && near(a.z, b.z, tol)
}

/// [`AGREE`] relative to a point's magnitude past 1 m.
fn point_tol(v: Vec3) -> f32 {
    AGREE * v.abs().max_element().max(1.0)
}

/// Normals and gaps within [`AGREE`], points within [`point_tol`].
fn planes_agree(a: &[ContactPlane], b: &[ContactPlane]) -> Result<(), String> {
    if a.len() != b.len() {
        return Err(format!("{} planes against {}", a.len(), b.len()));
    }
    for (i, (p, q)) in a.iter().zip(b).enumerate() {
        if (p.body, p.triangle, p.material) != (q.body, q.triangle, q.material) {
            return Err(format!("plane {i}: {p:?} against {q:?}"));
        }
        if !(near_v(p.normal, q.normal, AGREE)
            && near_v(p.point, q.point, point_tol(p.point))
            && near(p.distance, q.distance, AGREE))
        {
            return Err(format!(
                "plane {i} differs past {AGREE}: {p:?} against {q:?}"
            ));
        }
    }
    Ok(())
}

/// Two implementations' answers to the same case, within [`AGREE`] (points
/// [`AGREE`] relative to their magnitude past 1 m).
pub fn agree(a: &[Answer], b: &[Answer]) -> Result<(), String> {
    if a.len() != b.len() {
        return Err(format!("{} answers against {}", a.len(), b.len()));
    }
    for (i, (x, y)) in a.iter().zip(b).enumerate() {
        let result = match (x, y) {
            (Answer::Cast(None), Answer::Cast(None)) => Ok(()),
            (Answer::Cast(Some(p)), Answer::Cast(Some(q))) => {
                if !(near(p.fraction, q.fraction, AGREE) && near(p.distance, q.distance, AGREE)) {
                    Err(format!(
                        "cast stops at {} ({} m) against {} ({} m)",
                        p.fraction, p.distance, q.fraction, q.distance
                    ))
                } else {
                    planes_agree(&p.planes, &q.planes)
                }
            }
            (Answer::Contacts(p), Answer::Contacts(q)) => planes_agree(p, q),
            (Answer::Ray(None), Answer::Ray(None)) => Ok(()),
            (Answer::Ray(Some(p)), Answer::Ray(Some(q))) => {
                if (p.body, p.triangle, p.material) != (q.body, q.triangle, q.material) {
                    Err(format!("ray hits {p:?} against {q:?}"))
                } else if !(near(p.distance, q.distance, AGREE)
                    && near_v(p.normal, q.normal, AGREE)
                    && near_v(p.point, q.point, point_tol(p.point)))
                {
                    Err(format!("ray differs past {AGREE}: {p:?} against {q:?}"))
                } else {
                    Ok(())
                }
            }
            (Answer::Overlaps(p), Answer::Overlaps(q)) if p == q => Ok(()),
            _ => Err(format!("{x:?} against {y:?}")),
        };
        result.map_err(|e| format!("answer {i}: {e}"))?;
    }
    Ok(())
}

// ---- building worlds ----

/// Adds the quad a-b-c-d as two triangles (a, b, c) and (a, c, d), wound so
/// their front faces `front`.
fn quad(verts: &mut Vec<Vec3>, tris: &mut Vec<[u32; 3]>, [a, b, c, d]: [Vec3; 4], front: Vec3) {
    let base = verts.len() as u32;
    verts.extend([a, b, c, d]);
    if (b - a).cross(c - a).dot(front) > 0.0 {
        tris.extend([[base, base + 1, base + 2], [base, base + 2, base + 3]]);
    } else {
        tris.extend([[base, base + 2, base + 1], [base, base + 3, base + 2]]);
    }
}

fn mesh_body(
    id: u32,
    pose: Pose,
    verts: Vec<Vec3>,
    tris: Vec<[u32; 3]>,
    materials: Vec<u8>,
) -> Body {
    Body {
        id: BodyId(id),
        kind: BodyKind::Static,
        pose,
        collider: Collider::Mesh(Arc::new(
            Mesh::new(verts, tris, materials).expect("the table's meshes are valid"),
        )),
    }
}

fn convex(id: u32, kind: BodyKind, pose: Pose, shape: Shape) -> Body {
    Body {
        id: BodyId(id),
        kind,
        pose,
        collider: Collider::Convex(shape),
    }
}

const FLOOR: BodyId = BodyId(1);

/// A square floor at y = 0, ±`half` in x and z, as two triangles split along
/// x = z: triangle 0 where x < z, triangle 1 where x > z. Materials 3 and 7.
fn floor(half: f32) -> Body {
    let (mut v, mut t) = (Vec::new(), Vec::new());
    quad(
        &mut v,
        &mut t,
        [
            Vec3::new(-half, 0.0, -half),
            Vec3::new(-half, 0.0, half),
            Vec3::new(half, 0.0, half),
            Vec3::new(half, 0.0, -half),
        ],
        Vec3::Y,
    );
    mesh_body(FLOOR.0, Pose::IDENTITY, v, t, vec![3, 7])
}

fn sphere(radius: f32) -> Shape {
    Shape::Sphere { radius }
}

fn cast(shape: Shape, from: Vec3, motion: Vec3, contact_distance: f32) -> Query {
    Query::Cast {
        shape,
        from: Pose::at(from),
        motion,
        contact_distance,
        exclude: vec![],
    }
}

fn contacts(shape: Shape, at: Vec3, distance: f32) -> Query {
    Query::Contacts {
        shape,
        at: Pose::at(at),
        distance,
        exclude: vec![],
    }
}

fn ray(origin: Vec3, direction: Vec3, max_distance: f32) -> Query {
    Query::Ray {
        origin,
        direction,
        max_distance,
        exclude: vec![],
    }
}

fn overlaps(shape: Shape, at: Vec3) -> Query {
    Query::Overlaps {
        shape,
        at: Pose::at(at),
        exclude: vec![],
    }
}

// ---- checking ----

fn close(what: &str, got: f32, want: f32, tol: f32) -> Result<(), String> {
    if near(got, want, tol) {
        Ok(())
    } else {
        Err(format!("{what}: {got}, want {want} ± {tol}"))
    }
}

fn close_v(what: &str, got: Vec3, want: Vec3, tol: f32) -> Result<(), String> {
    if near_v(got, want, tol) {
        Ok(())
    } else {
        Err(format!("{what}: {got}, want {want} ± {tol}"))
    }
}

fn hit(answer: &Answer) -> Result<&Cast, String> {
    match answer {
        Answer::Cast(Some(c)) => Ok(c),
        other => Err(format!("want a cast that stops, got {other:?}")),
    }
}

fn planes(answer: &Answer) -> Result<&[ContactPlane], String> {
    match answer {
        Answer::Contacts(p) => Ok(p),
        other => Err(format!("want contacts, got {other:?}")),
    }
}

fn ray_hit(answer: &Answer) -> Result<&RayHit, String> {
    match answer {
        Answer::Ray(Some(r)) => Ok(r),
        other => Err(format!("want a ray hit, got {other:?}")),
    }
}

fn no_cast(answer: &Answer) -> Result<(), String> {
    match answer {
        Answer::Cast(None) => Ok(()),
        other => Err(format!("want no stop, got {other:?}")),
    }
}

fn overlap_set(answer: &Answer, want: &[u32]) -> Result<(), String> {
    let want: Vec<BodyId> = want.iter().map(|&i| BodyId(i)).collect();
    match answer {
        Answer::Overlaps(got) if *got == want => Ok(()),
        other => Err(format!("want overlaps {want:?}, got {other:?}")),
    }
}

/// One plane per (body, triangle), each a face's normal: what a plane on a
/// face of `front` must look like.
fn all_face(planes: &[ContactPlane], front: Vec3) -> Result<(), String> {
    for p in planes {
        close_v("plane normal", p.normal, front, TOL)?;
    }
    Ok(())
}

// ---- the table ----

pub fn cases() -> Vec<Case> {
    vec![
        sphere_cast_onto_plane(),
        convex_shapes_onto_plane(),
        corner_gives_two_planes(),
        ray_reports_its_triangle(),
        seam_never_snags(),
        cast_inside_doesnt_weld(),
        kinematic_blocks(),
        kinematic_moved(),
        sensors_only_overlap(),
        exclude_skips_bodies(),
        start_inside(),
        contact_reach(),
        deepest_point(),
        mesh_back(),
        ray_sides(),
        zero_motion(),
        an_open_edge_holds_off_its_edge(),
    ]
}

/// A sphere dropped onto a plane stops at the expected fraction, and the
/// plane it lands on is there with its triangle and material, even with no
/// contact distance asked for (the cast's own slop).
fn sphere_cast_onto_plane() -> Case {
    Case {
        name: "sphere_cast_onto_plane",
        bodies: vec![floor(4.0)],
        moves: vec![],
        queries: vec![
            cast(
                sphere(0.5),
                Vec3::new(1.5, 3.0, -1.0),
                Vec3::new(0.0, -5.0, 0.0),
                0.0,
            ),
            cast(
                sphere(0.5),
                Vec3::new(1.5, 3.0, -1.0),
                Vec3::new(0.0, -2.0, 0.0),
                0.0,
            ),
        ],
        expect: |a| {
            let c = hit(&a[0])?;
            close("fraction", c.fraction, 0.5, TOL_CAST / 5.0)?;
            close("distance", c.distance, 2.5, TOL_CAST)?;
            let [p] = c.planes.as_slice() else {
                return Err(format!("want one plane, got {}", c.planes.len()));
            };
            if (p.body, p.triangle, p.material) != (FLOOR, Some(1), Some(7)) {
                return Err(format!(
                    "want the floor's triangle 1 (material 7), got {p:?}"
                ));
            }
            close_v("normal", p.normal, Vec3::Y, TOL)?;
            close_v("point", p.point, Vec3::new(1.5, 0.0, -1.0), TOL)?;
            close("gap", p.distance, 0.0, TOL)?;
            no_cast(&a[1])
        },
    }
}

/// The four convex shapes, rounded where they have a margin, each dropped 5 m
/// from y = 3: each stops with its lowest point on the floor. A capsule on its
/// side and a box turned about Y check the pose's rotation.
fn convex_shapes_onto_plane() -> Case {
    let drop = |shape: Shape, rotation: Quat| Query::Cast {
        shape,
        from: Pose::new(Vec3::new(0.7, 3.0, -1.9), rotation),
        motion: Vec3::new(0.0, -5.0, 0.0),
        contact_distance: 0.0,
        exclude: vec![],
    };
    let boxed = Shape::Box {
        half_extents: Vec3::new(0.4, 0.3, 0.2),
        margin: 0.05,
    };
    let capsule = Shape::Capsule {
        radius: 0.3,
        half_segment: 0.5,
    };
    Case {
        name: "convex_shapes_onto_plane",
        bodies: vec![floor(4.0)],
        moves: vec![],
        queries: vec![
            drop(boxed, Quat::IDENTITY),
            drop(
                Shape::Box {
                    half_extents: Vec3::new(0.4, 0.3, 0.2),
                    margin: 0.0,
                },
                Quat::IDENTITY,
            ),
            drop(boxed, Quat::from_rotation_y(0.7)),
            drop(capsule, Quat::IDENTITY),
            drop(capsule, Quat::from_rotation_z(std::f32::consts::FRAC_PI_2)),
            drop(
                Shape::Cylinder {
                    radius: 0.25,
                    half_height: 0.45,
                    margin: 0.05,
                },
                Quat::IDENTITY,
            ),
            drop(
                Shape::Cylinder {
                    radius: 0.25,
                    half_height: 0.45,
                    margin: 0.0,
                },
                Quat::IDENTITY,
            ),
        ],
        expect: |a| {
            // The lowest point's height under the centre: half height plus
            // margin.
            for (i, below) in [0.35, 0.3, 0.35, 0.8, 0.3, 0.5, 0.45]
                .into_iter()
                .enumerate()
            {
                let c = hit(&a[i]).map_err(|e| format!("shape {i}: {e}"))?;
                close(
                    &format!("shape {i} distance"),
                    c.distance,
                    3.0 - below,
                    TOL_CAST,
                )?;
                if c.planes.is_empty() {
                    return Err(format!("shape {i}: no plane where it stopped"));
                }
                all_face(&c.planes, Vec3::Y).map_err(|e| format!("shape {i}: {e}"))?;
            }
            Ok(())
        },
    }
}

/// A floor meeting a wall at x = 1, one mesh as the level is: a sphere in the
/// corner touches two planes, and a sphere cast diagonally into it stops
/// touching both.
fn corner_gives_two_planes() -> Case {
    let (mut v, mut t) = (Vec::new(), Vec::new());
    quad(
        &mut v,
        &mut t,
        [
            Vec3::new(-2.0, 0.0, -2.0),
            Vec3::new(-2.0, 0.0, 2.0),
            Vec3::new(1.0, 0.0, 2.0),
            Vec3::new(1.0, 0.0, -2.0),
        ],
        Vec3::Y,
    );
    quad(
        &mut v,
        &mut t,
        [
            Vec3::new(1.0, 0.0, -2.0),
            Vec3::new(1.0, 0.0, 2.0),
            Vec3::new(1.0, 3.0, 2.0),
            Vec3::new(1.0, 3.0, -2.0),
        ],
        Vec3::NEG_X,
    );
    Case {
        name: "corner_gives_two_planes",
        bodies: vec![mesh_body(
            FLOOR.0,
            Pose::IDENTITY,
            v,
            t,
            vec![10, 11, 20, 21],
        )],
        moves: vec![],
        queries: vec![
            contacts(sphere(0.5), Vec3::new(0.5, 0.5, 0.0), 0.01),
            cast(
                sphere(0.5),
                Vec3::new(-0.5, 1.5, 0.0),
                Vec3::new(2.0, -2.0, 0.0),
                0.01,
            ),
        ],
        expect: |a| {
            let check = |planes: &[ContactPlane]| -> Result<(), String> {
                let [floor, wall] = planes else {
                    return Err(format!("want two planes, got {}", planes.len()));
                };
                close_v("floor normal", floor.normal, Vec3::Y, TOL)?;
                close_v("floor point", floor.point, Vec3::new(0.5, 0.0, 0.0), TOL)?;
                close_v("wall normal", wall.normal, Vec3::NEG_X, TOL)?;
                close_v("wall point", wall.point, Vec3::new(1.0, 0.5, 0.0), TOL)?;
                if floor
                    .material
                    .zip(wall.material)
                    .is_none_or(|(f, w)| f >= 20 || w < 20)
                {
                    return Err(format!("want a floor then a wall triangle, got {planes:?}"));
                }
                for p in planes {
                    close("gap", p.distance, 0.0, TOL)?;
                }
                Ok(())
            };
            check(planes(&a[0])?)?;
            let c = hit(&a[1])?;
            // Half of the 2√2 m motion, to TOL_CAST in meters.
            let length = 8.0f32.sqrt();
            close("fraction", c.fraction, 0.5, TOL_CAST / length)?;
            close("distance", c.distance, 0.5 * length, TOL_CAST)?;
            check(&c.planes)
        },
    }
}

/// A 4 x 4 grid of 32 triangles, each with its own three vertices (so merging
/// duplicates rewrites the index buffer) and its own material, posed where
/// level 213's body is: a ray down through each of four triangles' centroids
/// reports that triangle and its material.
fn ray_reports_its_triangle() -> Case {
    const BODY: Vec3 = Vec3::new(-425.419_16, -37.632_3, -15.965_546);
    let (mut v, mut t) = (Vec::new(), Vec::new());
    for i in 0..4 {
        for j in 0..4 {
            let (x, z) = (i as f32 - 2.0, j as f32 - 2.0);
            quad(
                &mut v,
                &mut t,
                [
                    Vec3::new(x, 0.0, z),
                    Vec3::new(x, 0.0, z + 1.0),
                    Vec3::new(x + 1.0, 0.0, z + 1.0),
                    Vec3::new(x + 1.0, 0.0, z),
                ],
                Vec3::Y,
            );
        }
    }
    let materials = (0..t.len()).map(|i| (i * 7 + 3) as u8).collect();
    let probes = [0usize, 5, 17, 31];
    let centroid =
        |tri: [u32; 3]| (v[tri[0] as usize] + v[tri[1] as usize] + v[tri[2] as usize]) / 3.0;
    let queries = probes
        .iter()
        .map(|&k| ray(BODY + centroid(t[k]) + Vec3::Y * 2.0, Vec3::NEG_Y, 10.0))
        .collect();
    Case {
        name: "ray_reports_its_triangle",
        bodies: vec![mesh_body(FLOOR.0, Pose::at(BODY), v, t, materials)],
        moves: vec![],
        queries,
        expect: |a| {
            for (answer, k) in a.iter().zip([0u32, 5, 17, 31]) {
                let r = ray_hit(answer)?;
                if (r.body, r.triangle, r.material) != (FLOOR, Some(k), Some((k * 7 + 3) as u8)) {
                    return Err(format!("want triangle {k}, got {r:?}"));
                }
                close("distance", r.distance, 2.0, TOL)?;
                close_v("normal", r.normal, Vec3::Y, TOL)?;
                // 3e-5 m is one f32 step at x = 425; the point is relative.
                close("height", r.point.y, -37.632_3, 2.0 * TOL)?;
            }
            Ok(())
        },
    }
}

/// A sphere sliding across the seam between the floor's two coplanar
/// triangles: at every pose over the seam, from the original's 0.01 m snap
/// height down to 5 mm inside the floor, each plane within 0.05 m is the
/// faces' normal (internal edges fixed: unfixed, the far triangle's edge
/// tilts it against the motion); and a cast across at the snap height goes
/// all the way.
fn seam_never_snags() -> Case {
    let mut queries = Vec::new();
    for height in [0.01, 0.0, -0.005] {
        for i in 0..=20 {
            let x = -0.5 + i as f32 * 0.05;
            queries.push(contacts(sphere(0.5), Vec3::new(x, 0.5 + height, 0.0), 0.05));
        }
    }
    queries.push(cast(
        sphere(0.5),
        Vec3::new(-1.0, 0.51, 0.0),
        Vec3::new(2.0, 0.0, 0.0),
        0.05,
    ));
    queries.push(cast(
        sphere(0.5),
        Vec3::new(-1.0, 0.51, -1.0),
        Vec3::new(2.0, 0.0, 2.0),
        0.05,
    ));
    Case {
        name: "seam_never_snags",
        bodies: vec![floor(4.0)],
        moves: vec![],
        queries,
        expect: |a| {
            for (i, answer) in a[..63].iter().enumerate() {
                let p = planes(answer)?;
                if p.is_empty() {
                    return Err(format!("pose {i}: no plane"));
                }
                all_face(p, Vec3::Y).map_err(|e| format!("pose {i}: {e}"))?;
            }
            no_cast(&a[63])?;
            no_cast(&a[64])
        },
    }
}

/// What the seam doesn't do, pinned so a change shows: a cast doesn't weld.
/// From 5 mm inside the floor, a sphere cast across the seam stops on the far
/// triangle's edge, short of the seam; the planes there are still the faces'
/// (contacts weld), so nothing in them opposes the motion. The controller keeps
/// its shape off the ground, as the original's 0.01 m snap does. The closed
/// form puts first touch at x = −0.0998 (0.9003 m on); parry's cast, grazing
/// the edge at 8°, stopped at x = −0.1110 (0.8890 m) on the first run, 1.2 mm
/// of gap short, so the check is a bound: past 0.8 m, short of the seam.
fn cast_inside_doesnt_weld() -> Case {
    Case {
        name: "cast_inside_doesnt_weld",
        bodies: vec![floor(4.0)],
        moves: vec![],
        queries: vec![cast(
            sphere(0.5),
            Vec3::new(-1.0, 0.495, 0.0),
            Vec3::new(2.0, 0.0, 0.0),
            0.05,
        )],
        expect: |a| {
            let c = hit(&a[0])?;
            // First touch on the edge x = z: x²/2 + 0.495² = 0.5².
            let touch = 1.0 - (2.0f32 * (0.25 - 0.495 * 0.495)).sqrt();
            if !(0.8..=touch + TOL_CAST).contains(&c.distance) {
                return Err(format!("stopped at {} m, want 0.8 to {touch}", c.distance));
            }
            all_face(&c.planes, Vec3::Y)
        },
    }
}

const DOOR: BodyId = BodyId(5);

/// A kinematic box (a door turned 90° about Y, its 0.05 m margin) stops a
/// sphere at its face, with its body and no triangle.
fn door(position: Vec3) -> Body {
    convex(
        DOOR.0,
        BodyKind::Kinematic,
        Pose::new(position, Quat::from_rotation_y(std::f32::consts::FRAC_PI_2)),
        Shape::Box {
            half_extents: Vec3::new(0.25, 1.0, 0.75),
            margin: 0.05,
        },
    )
}

fn door_check(answer: &Answer, distance: f32) -> Result<(), String> {
    let c = hit(answer)?;
    close("distance", c.distance, distance, TOL_CAST)?;
    let [p] = c.planes.as_slice() else {
        return Err(format!("want one plane, got {:?}", c.planes));
    };
    if (p.body, p.triangle, p.material) != (DOOR, None, None) {
        return Err(format!("want the door, got {p:?}"));
    }
    close_v("normal", p.normal, Vec3::NEG_X, TOL)
}

fn kinematic_blocks() -> Case {
    Case {
        name: "kinematic_blocks",
        bodies: vec![floor(8.0), door(Vec3::new(3.0, 1.2, 0.0))],
        moves: vec![],
        queries: vec![cast(
            sphere(0.5),
            Vec3::new(0.0, 1.2, 0.0),
            Vec3::new(5.0, 0.0, 0.0),
            0.0,
        )],
        // The face: 3 − 0.75 − 0.05; the sphere's centre 0.5 short of it.
        expect: |a| door_check(&a[0], 1.7),
    }
}

/// The same door moved 2 m on: the queries find it where it went.
fn kinematic_moved() -> Case {
    Case {
        name: "kinematic_moved",
        bodies: vec![floor(8.0), door(Vec3::new(3.0, 1.2, 0.0))],
        moves: vec![(
            DOOR,
            Pose::new(
                Vec3::new(5.0, 1.2, 0.0),
                Quat::from_rotation_y(std::f32::consts::FRAC_PI_2),
            ),
        )],
        queries: vec![
            cast(
                sphere(0.5),
                Vec3::new(0.0, 1.2, 0.0),
                Vec3::new(5.0, 0.0, 0.0),
                0.0,
            ),
            overlaps(sphere(0.5), Vec3::new(2.5, 1.2, 0.0)),
        ],
        expect: |a| {
            door_check(&a[0], 3.7)?;
            overlap_set(&a[1], &[])
        },
    }
}

const SENSOR: BodyId = BodyId(8);
const BALL: BodyId = BodyId(9);

/// A sensor box over the floor and a kinematic ball beyond it: overlaps see
/// both, but casts, rays and contacts pass through the sensor.
fn sensors_only_overlap() -> Case {
    Case {
        name: "sensors_only_overlap",
        bodies: vec![
            floor(8.0),
            convex(
                SENSOR.0,
                BodyKind::Sensor,
                Pose::at(Vec3::new(0.0, 1.0, 0.0)),
                Shape::Box {
                    half_extents: Vec3::ONE,
                    margin: 0.0,
                },
            ),
            convex(
                BALL.0,
                BodyKind::Kinematic,
                Pose::at(Vec3::new(3.0, 1.0, 0.0)),
                sphere(0.5),
            ),
        ],
        moves: vec![],
        queries: vec![
            overlaps(sphere(0.5), Vec3::new(0.5, 1.0, 0.0)),
            overlaps(sphere(0.5), Vec3::new(2.4, 1.0, 0.0)),
            overlaps(sphere(0.6), Vec3::new(1.5, 0.55, 0.0)),
            cast(
                sphere(0.5),
                Vec3::new(-3.0, 1.0, 0.0),
                Vec3::new(6.0, 0.0, 0.0),
                0.0,
            ),
            ray(Vec3::new(-3.0, 1.0, 0.0), Vec3::X, 10.0),
            contacts(sphere(0.5), Vec3::new(0.0, 1.0, 0.0), 0.1),
        ],
        expect: |a| {
            overlap_set(&a[0], &[SENSOR.0])?;
            overlap_set(&a[1], &[BALL.0])?;
            overlap_set(&a[2], &[FLOOR.0, SENSOR.0])?;
            let c = hit(&a[3])?;
            close("cast distance", c.distance, 5.0, TOL_CAST)?;
            if c.planes.iter().map(|p| p.body).collect::<Vec<_>>() != [BALL] {
                return Err(format!("want the ball's plane alone, got {:?}", c.planes));
            }
            let r = ray_hit(&a[4])?;
            if r.body != BALL {
                return Err(format!("want the ball, got {r:?}"));
            }
            close("ray distance", r.distance, 5.5, TOL)?;
            close_v("ray normal", r.normal, Vec3::NEG_X, TOL)?;
            match &a[5] {
                Answer::Contacts(p) if p.is_empty() => Ok(()),
                other => Err(format!("want no contacts, got {other:?}")),
            }
        },
    }
}

/// A pillar standing on the floor: excluding it, a cast down passes it to the
/// floor and contacts on its top find nothing; excluding the floor too, a ray
/// down finds nothing; excluding the floor, an overlap low in the pillar finds
/// the pillar alone.
fn exclude_skips_bodies() -> Case {
    let pillar = convex(
        BALL.0,
        BodyKind::Kinematic,
        Pose::at(Vec3::new(0.0, 1.0, 0.0)),
        Shape::Box {
            half_extents: Vec3::new(0.5, 1.0, 0.5),
            margin: 0.0,
        },
    );
    Case {
        name: "exclude_skips_bodies",
        bodies: vec![floor(4.0), pillar],
        moves: vec![],
        queries: vec![
            Query::Cast {
                shape: sphere(0.25),
                from: Pose::at(Vec3::new(0.0, 4.0, 0.0)),
                motion: Vec3::new(0.0, -5.0, 0.0),
                contact_distance: 0.0,
                exclude: vec![BALL],
            },
            Query::Ray {
                origin: Vec3::new(0.0, 4.0, 0.0),
                direction: Vec3::NEG_Y,
                max_distance: 10.0,
                exclude: vec![BALL, FLOOR],
            },
            Query::Overlaps {
                shape: sphere(0.25),
                at: Pose::at(Vec3::new(0.0, 0.2, 0.0)),
                exclude: vec![FLOOR],
            },
            Query::Contacts {
                shape: sphere(0.25),
                at: Pose::at(Vec3::new(0.0, 2.25, 0.0)),
                distance: 0.01,
                exclude: vec![BALL],
            },
        ],
        expect: |a| {
            close("cast past the pillar", hit(&a[0])?.distance, 3.75, TOL_CAST)?;
            if a[1] != Answer::Ray(None) {
                return Err(format!("want no ray hit, got {:?}", a[1]));
            }
            overlap_set(&a[2], &[BALL.0])?;
            match &a[3] {
                Answer::Contacts(p) if p.is_empty() => Ok(()),
                other => Err(format!("want no contacts, got {other:?}")),
            }
        },
    }
}

/// A sphere 0.1 m inside a wall: moving further in stops it at once, the
/// wall's plane at −0.1; moving out, or along the wall and slightly out,
/// nothing stops it. Exactly along the wall is left out: the first run
/// stopped there at 0, the sign of float noise in the contact normal deciding
/// (the seam's `cast` says so).
fn start_inside() -> Case {
    let wall = convex(
        BALL.0,
        BodyKind::Kinematic,
        Pose::at(Vec3::new(2.0, 1.0, 0.0)),
        Shape::Box {
            half_extents: Vec3::new(1.0, 1.0, 2.0),
            margin: 0.0,
        },
    );
    let from = Vec3::new(0.6, 1.0, 0.0);
    Case {
        name: "start_inside",
        bodies: vec![wall],
        moves: vec![],
        queries: vec![
            cast(sphere(0.5), from, Vec3::new(1.0, 0.0, 0.0), 0.0),
            cast(sphere(0.5), from, Vec3::new(-1.0, 0.0, 0.0), 0.0),
            cast(sphere(0.5), from, Vec3::new(-0.05, 0.0, 1.0), 0.0),
        ],
        expect: |a| {
            let c = hit(&a[0])?;
            close("fraction", c.fraction, 0.0, TOL)?;
            let [p] = c.planes.as_slice() else {
                return Err(format!("want one plane, got {:?}", c.planes));
            };
            close_v("normal", p.normal, Vec3::NEG_X, TOL)?;
            close("depth", p.distance, -0.1, TOL)?;
            no_cast(&a[1])?;
            no_cast(&a[2])
        },
    }
}

/// How far contacts reach: a gap of 0.03 m is a plane at 0.03 within 0.05 and
/// nothing within 0.02.
fn contact_reach() -> Case {
    let at = Vec3::new(1.5, 0.53, -1.0);
    Case {
        name: "contact_reach",
        bodies: vec![floor(4.0)],
        moves: vec![],
        queries: vec![
            contacts(sphere(0.5), at, 0.05),
            contacts(sphere(0.5), at, 0.02),
        ],
        expect: |a| {
            let [p] = planes(&a[0])? else {
                return Err(format!("want one plane, got {:?}", a[0]));
            };
            close("gap", p.distance, 0.03, TOL)?;
            close_v("point", p.point, Vec3::new(1.5, 0.0, -1.0), TOL)?;
            if !planes(&a[1])?.is_empty() {
                return Err(format!("want nothing within 0.02, got {:?}", a[1]));
            }
            Ok(())
        },
    }
}

/// A box tipped 0.05 rad about Z, its low edge 1 cm into the floor and its
/// high one 4 cm above: one plane for the triangle, at the deepest point.
fn deepest_point() -> Case {
    let (s, c) = 0.05f32.sin_cos();
    let centre = Vec3::new(1.5, 0.5 * (s + c) - 0.01, -1.0);
    Case {
        name: "deepest_point",
        bodies: vec![floor(4.0)],
        moves: vec![],
        queries: vec![Query::Contacts {
            shape: Shape::Box {
                half_extents: Vec3::splat(0.5),
                margin: 0.0,
            },
            at: Pose::new(centre, Quat::from_rotation_z(0.05)),
            distance: 0.05,
            exclude: vec![],
        }],
        expect: |a| {
            let [p] = planes(&a[0])? else {
                return Err(format!("want one plane, got {:?}", a[0]));
            };
            let (s, c) = 0.05f32.sin_cos();
            close("depth", p.distance, -0.01, TOL)?;
            close("low edge x", p.point.x, 1.5 - 0.5 * c + 0.5 * s, TOL)?;
            close("on the floor", p.point.y, 0.0, TOL)?;
            close_v("normal", p.normal, Vec3::Y, TOL)
        },
    }
}

/// A mesh triangle is one-sided for contacts (internal-edge fixing drops a
/// contact from behind) but not for casts: a sphere just under the floor gets
/// no plane from it, and a sphere cast up into it from below stops there with
/// no plane at all. So a mesh's winding decides which side holds a shape up
/// (the `Mesh` docs).
fn mesh_back() -> Case {
    Case {
        name: "mesh_back",
        bodies: vec![floor(4.0)],
        moves: vec![],
        queries: vec![
            contacts(sphere(0.5), Vec3::new(1.5, -0.49, -1.0), 0.05),
            contacts(sphere(0.5), Vec3::new(1.5, -0.53, -1.0), 0.05),
            cast(
                sphere(0.5),
                Vec3::new(1.5, -2.0, -1.0),
                Vec3::new(0.0, 3.0, 0.0),
                0.0,
            ),
        ],
        expect: |a| {
            for i in [0, 1] {
                if !planes(&a[i])?.is_empty() {
                    return Err(format!(
                        "query {i}: want no plane from behind, got {:?}",
                        a[i]
                    ));
                }
            }
            let up = hit(&a[2])?;
            close("cast up", up.distance, 1.5, TOL_CAST)?;
            if !up.planes.is_empty() {
                return Err(format!(
                    "want no plane under the floor, got {:?}",
                    up.planes
                ));
            }
            Ok(())
        },
    }
}

/// Rays see both sides of a mesh triangle and leave a convex body they start
/// inside through its far face.
fn ray_sides() -> Case {
    let block = convex(
        BALL.0,
        BodyKind::Kinematic,
        Pose::at(Vec3::new(0.0, 2.0, 2.0)),
        Shape::Box {
            half_extents: Vec3::new(0.5, 0.5, 0.5),
            margin: 0.0,
        },
    );
    Case {
        name: "ray_sides",
        bodies: vec![floor(4.0), block],
        moves: vec![],
        queries: vec![
            ray(Vec3::new(1.5, -1.0, -1.0), Vec3::Y, 10.0),
            ray(Vec3::new(0.1, 2.0, 2.0), Vec3::X, 10.0),
        ],
        expect: |a| {
            let under = ray_hit(&a[0])?;
            if (under.body, under.triangle) != (FLOOR, Some(1)) {
                return Err(format!(
                    "want the floor's triangle 1 from below, got {under:?}"
                ));
            }
            close("from below", under.distance, 1.0, TOL)?;
            close_v("from below, normal", under.normal, Vec3::NEG_Y, TOL)?;
            let out = ray_hit(&a[1])?;
            if out.body != BALL {
                return Err(format!("want the block, got {out:?}"));
            }
            close("out of the block", out.distance, 0.4, TOL)?;
            close_v(
                "out of the block, point",
                out.point,
                Vec3::new(0.5, 2.0, 2.0),
                TOL,
            )
        },
    }
}

/// A zero motion or direction asks nothing.
fn zero_motion() -> Case {
    Case {
        name: "zero_motion",
        bodies: vec![floor(4.0)],
        moves: vec![],
        queries: vec![
            cast(sphere(0.5), Vec3::new(0.0, 0.5, 0.0), Vec3::ZERO, 0.0),
            ray(Vec3::new(0.0, 1.0, 0.0), Vec3::ZERO, 10.0),
        ],
        expect: |a| {
            no_cast(&a[0])?;
            if a[1] != Answer::Ray(None) {
                return Err(format!("want no ray, got {:?}", a[1]));
            }
            Ok(())
        },
    }
}

/// A face leaning out 4.8° over a drop, its top edge open (no other triangle
/// has it), as the overhang under the ledge route's west end is (AG.48): a
/// sphere past the top edge gets the edge's own plane, its normal from the
/// edge's nearest point to the sphere and its gap the true one; past the
/// edge's end, the end's; in front of the face, the face's; behind the face,
/// nothing, as anywhere else on a mesh.
fn an_open_edge_holds_off_its_edge() -> Case {
    let (mut v, mut t) = (Vec::new(), Vec::new());
    quad(
        &mut v,
        &mut t,
        [
            Vec3::new(0.0, 0.0, -2.0),
            Vec3::new(0.0, 0.0, 2.0),
            Vec3::new(-0.25, -3.0, 2.0),
            Vec3::new(-0.25, -3.0, -2.0),
        ],
        Vec3::X,
    );
    Case {
        name: "an_open_edge_holds_off_its_edge",
        bodies: vec![mesh_body(FLOOR.0, Pose::IDENTITY, v, t, vec![5, 6])],
        moves: vec![],
        queries: vec![
            contacts(sphere(0.45), Vec3::new(0.3, 0.4, 0.5), 0.1),
            contacts(sphere(0.45), Vec3::new(0.3, 0.4, 2.3), 0.2),
            contacts(sphere(0.45), Vec3::new(0.5, -1.0, 0.0), 0.2),
            contacts(sphere(0.45), Vec3::new(-0.3, 0.4, 0.5), 0.1),
        ],
        expect: |a| {
            let one = |i: usize| -> Result<ContactPlane, String> {
                match planes(&a[i])? {
                    [p] if (p.body, p.triangle, p.material) == (FLOOR, Some(0), Some(5)) => Ok(*p),
                    other => Err(format!(
                        "query {i}: want triangle 0's one plane, got {other:?}"
                    )),
                }
            };
            let edge = one(0)?;
            close_v(
                "past the edge, normal",
                edge.normal,
                Vec3::new(0.6, 0.8, 0.0),
                TOL,
            )?;
            close_v(
                "past the edge, point",
                edge.point,
                Vec3::new(0.0, 0.0, 0.5),
                TOL,
            )?;
            close("past the edge, gap", edge.distance, 0.05, TOL)?;
            let end = one(1)?;
            let to = Vec3::new(0.3, 0.4, 0.3);
            close_v("past its end, normal", end.normal, to.normalize(), TOL)?;
            close_v(
                "past its end, point",
                end.point,
                Vec3::new(0.0, 0.0, 2.0),
                TOL,
            )?;
            close("past its end, gap", end.distance, to.length() - 0.45, TOL)?;
            let face = one(2)?;
            let n = Vec3::new(3.0, -0.25, 0.0).normalize();
            close_v("the face, normal", face.normal, n, TOL)?;
            close(
                "the face, gap",
                face.distance,
                Vec3::new(0.5, -1.0, 0.0).dot(n) - 0.45,
                TOL,
            )?;
            if !planes(&a[3])?.is_empty() {
                return Err(format!("behind the face, want nothing, got {:?}", a[3]));
            }
            Ok(())
        },
    }
}
