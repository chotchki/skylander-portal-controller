//! Synthetic geometry and a frame runner for the controller's scenario tests
//! (openspec AG.16, design decision 17): every world built in code from
//! numbers we chose, on the parry side of the seam.

#![allow(dead_code)]

use std::sync::Arc;

use guac_motion::hero::{Hero, Input};
use guac_motion::parry::ParryWorld;
use guac_motion::seam::{Body, BodyId, BodyKind, Collider, Mesh, Pose, Vec3};
use guac_motion::solid::{HavokSolid, SolidInfo, SolidKind};
use guac_motion::stick::Stick;
use guac_motion::tuning::Tuning;

pub const DT: f32 = 1.0 / 60.0;
/// Eruptor's top speed, 21.5 ft/s.
pub const TOP: f32 = 21.5 * 0.3048;
/// Camera 0's view across the ground at the start.
pub const VIEW: f32 = 15.0;

/// Triangles in code, each quad wound so its front faces `normal`.
#[derive(Default)]
pub struct Soup {
    vertices: Vec<Vec3>,
    triangles: Vec<[u32; 3]>,
}

impl Soup {
    pub fn quad(&mut self, corners: [Vec3; 4], normal: Vec3) -> &mut Self {
        let base = self.vertices.len() as u32;
        self.vertices.extend(corners);
        let front = (corners[1] - corners[0]).cross(corners[2] - corners[0]);
        if front.dot(normal) >= 0.0 {
            self.triangles.push([base, base + 1, base + 2]);
            self.triangles.push([base, base + 2, base + 3]);
        } else {
            self.triangles.push([base, base + 2, base + 1]);
            self.triangles.push([base, base + 3, base + 2]);
        }
        self
    }

    /// Level ground at `y` over [x0, x1] x [z0, z1] in cells of `cell` m, so
    /// he crosses internal edges as he walks.
    pub fn floor(&mut self, y: f32, x: [f32; 2], z: [f32; 2], cell: f32) -> &mut Self {
        let nx = ((x[1] - x[0]) / cell).ceil() as usize;
        let nz = ((z[1] - z[0]) / cell).ceil() as usize;
        for i in 0..nx {
            for j in 0..nz {
                let (a, b) = (
                    x[0] + i as f32 * cell,
                    (x[0] + (i + 1) as f32 * cell).min(x[1]),
                );
                let (c, d) = (
                    z[0] + j as f32 * cell,
                    (z[0] + (j + 1) as f32 * cell).min(z[1]),
                );
                self.quad(
                    [
                        Vec3::new(a, y, c),
                        Vec3::new(a, y, d),
                        Vec3::new(b, y, d),
                        Vec3::new(b, y, c),
                    ],
                    Vec3::Y,
                );
            }
        }
        self
    }

    /// A plane through `origin` rising `degrees` toward +z for `length` m
    /// across the ground, in 2 m cells along x like the floors.
    pub fn ramp(&mut self, origin: Vec3, degrees: f32, x: [f32; 2], length: f32) -> &mut Self {
        let up = Vec3::new(0.0, length * degrees.to_radians().tan(), length);
        let n = up.cross(Vec3::X).normalize();
        for (a, b) in cells(x) {
            let (a, b) = (
                Vec3::new(a, origin.y, origin.z),
                Vec3::new(b, origin.y, origin.z),
            );
            self.quad([a, b, b + up, a + up], n);
        }
        self
    }

    /// A vertical face in the plane z = `z`, facing `facing` (±Z), cut into
    /// the floors' 2 m cells along x so its edges meet theirs vertex for
    /// vertex: parry fixes internal edges only where triangles share
    /// vertices, as the level's mesh does (a T-junction leaves an edge's
    /// contact out).
    pub fn wall_z(&mut self, z: f32, x: [f32; 2], y: [f32; 2], facing: Vec3) -> &mut Self {
        for (a, b) in cells(x) {
            self.quad(
                [
                    Vec3::new(a, y[0], z),
                    Vec3::new(b, y[0], z),
                    Vec3::new(b, y[1], z),
                    Vec3::new(a, y[1], z),
                ],
                facing,
            );
        }
        self
    }

    /// A vertical face in the plane x = `x`, facing ±X, in 2 m cells along z.
    pub fn wall_x(&mut self, x: f32, z: [f32; 2], y: [f32; 2], facing: Vec3) -> &mut Self {
        for (a, b) in cells(z) {
            self.quad(
                [
                    Vec3::new(x, y[0], a),
                    Vec3::new(x, y[0], b),
                    Vec3::new(x, y[1], b),
                    Vec3::new(x, y[1], a),
                ],
                facing,
            );
        }
        self
    }

    pub fn mesh(&self) -> Mesh {
        Mesh::new(
            self.vertices.clone(),
            self.triangles.clone(),
            vec![1; self.triangles.len()],
        )
        .unwrap()
    }
}

/// [lo, hi] in 2 m cells.
fn cells(range: [f32; 2]) -> Vec<(f32, f32)> {
    let n = ((range[1] - range[0]) / 2.0).ceil() as usize;
    (0..n)
        .map(|i| {
            (
                range[0] + i as f32 * 2.0,
                (range[0] + (i + 1) as f32 * 2.0).min(range[1]),
            )
        })
        .collect()
}

pub const LEVEL: BodyId = BodyId(1);

pub fn level(soup: &Soup) -> Body {
    Body {
        id: LEVEL,
        kind: BodyKind::Static,
        pose: Pose::IDENTITY,
        collider: Collider::Mesh(Arc::new(soup.mesh())),
    }
}

/// The level and any placements' solids.
pub fn world(soup: &Soup, solids: &[(BodyId, HavokSolid, Pose)]) -> ParryWorld {
    let mut bodies = vec![level(soup)];
    for (id, solid, at) in solids {
        bodies.push(Body {
            id: *id,
            kind: BodyKind::Kinematic,
            pose: solid.pose(*at),
            collider: Collider::Convex(solid.in_world()),
        });
    }
    ParryWorld::new(&bodies).unwrap()
}

/// Eruptor's solid: a 4 x 6 x 3 ft sphere at 150 %.
pub fn eruptor_solid() -> HavokSolid {
    SolidInfo {
        kind: SolidKind::Sphere,
        offset: [0.0; 3],
        rotation: [0.0; 3],
        width: 4.0,
        height: 6.0,
        length: 3.0,
    }
    .havok([150.0; 3])
    .unwrap()
}

/// Eruptor settled at `at` (x and z; y is where he's dropped from) facing
/// `facing`.
pub fn eruptor(world: &ParryWorld, at: Vec3, facing: f32) -> Hero {
    let mut hero = Hero::new(eruptor_solid(), Tuning::ERUPTOR, at, facing);
    hero.settle(world, &[]);
    hero
}

/// A stick pushed `strength` of the way toward `degrees` off the view's
/// forward (positive to the left, as headings turn).
pub fn stick(strength: f32, degrees: f32) -> Stick {
    // The dead zone's inverse: an axis value a gives (|a| - 0.25)/0.75.
    let (s, c) = degrees.to_radians().sin_cos();
    let raw = |v: f32| {
        if v.abs() < 1e-6 {
            0.0
        } else {
            v.signum() * (v.abs() * 0.75 + 0.25)
        }
    };
    Stick::new(raw(-s * strength), raw(c * strength))
}

/// What a frame left him at.
#[derive(Debug, Clone, Copy)]
pub struct State {
    pub pivot: Vec3,
    pub velocity: Vec3,
    pub facing: f32,
    pub supported: bool,
    pub height: Option<f32>,
    pub walk: Vec3,
    pub held_off: bool,
}

pub fn state(hero: &Hero) -> State {
    State {
        pivot: hero.pivot,
        velocity: hero.velocity,
        facing: hero.facing,
        supported: hero.footing.supported,
        height: hero.footing.height,
        walk: hero.walk.velocity,
        held_off: hero.held_off,
    }
}

/// Runs `frames` frames of [`DT`], the stick each frame from `stick_at`
/// (frame index from 0), the view at [`VIEW`]; the states after each.
pub fn run(
    world: &ParryWorld,
    hero: &mut Hero,
    frames: usize,
    stick_at: impl Fn(usize) -> Stick,
) -> Vec<State> {
    (0..frames)
        .map(|k| {
            hero.step(world, &Input::stick(stick_at(k), VIEW), DT, &[]);
            state(hero)
        })
        .collect()
}

pub fn ground_speed(v: Vec3) -> f32 {
    v.x.hypot(v.z)
}

/// The bearing of a move across the ground, degrees.
pub fn bearing(v: Vec3) -> f32 {
    v.x.atan2(v.z).to_degrees().rem_euclid(360.0)
}

pub fn angle_between(a: f32, b: f32) -> f32 {
    let d = (a - b).rem_euclid(360.0);
    d.min(360.0 - d)
}

/// A big floor of 2 m cells around the origin at y = 0.
pub fn open_ground() -> Soup {
    let mut soup = Soup::default();
    soup.floor(0.0, [-40.0, 40.0], [-40.0, 40.0], 2.0);
    soup
}
