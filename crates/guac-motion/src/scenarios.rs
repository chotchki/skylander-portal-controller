//! The controller's open-ground and wall scenarios on synthetic geometry,
//! for any side of the seam (openspec AG.16; its Verify runs them through
//! guacamole's Avian side headless as well as on [`crate::parry`]). Each
//! [`Scenario`] is a world of bodies built in code and a check that steps
//! Eruptor through it on whatever seam holds those bodies, from AG.8's and
//! AG.9's numbers: hero-movement's The stick walks the hero as seen from the
//! camera, Top speed, Speeding up and slowing down, Turning and hard
//! reversals, No jumping, The same walk at any frame rate and Walls; and,
//! since AG.48, Falling and landing's open edge.
//!
//! Synthetic walls share the floors' vertices, as level 213's indexed mesh
//! does: parry gives an edge two triangles don't share by vertex (a
//! T-junction) no contact, while the seam's cast still stops on it.

use std::sync::Arc;

use crate::clock::FixedClock;
use crate::hero::{Hero, Input};
use crate::proxy::KEEP;
use crate::seam::{Body, BodyId, BodyKind, Collider, Mesh, PhysicsSeam, Pose, Quat, Shape, Vec3};
use crate::solid::{HavokSolid, SolidInfo, SolidKind, euler};
use crate::stick::Stick;
use crate::tuning::{FOOT, Tuning};

pub const DT: f32 = 1.0 / 60.0;
/// Eruptor's top speed, 21.5 ft/s.
pub const TOP: f32 = 21.5 * FOOT;
/// Camera 0's view across the ground at the start.
pub const VIEW: f32 = 15.0;

/// A world and a check run in it.
pub struct Scenario {
    pub name: &'static str,
    pub bodies: Vec<Body>,
    pub check: fn(&dyn PhysicsSeam) -> Result<(), String>,
}

// A check returns its first failure instead of panicking, so the harness on
// the other side of the seam names the scenario it failed in.
macro_rules! ensure {
    ($cond:expr, $($msg:tt)+) => {
        let held: bool = $cond;
        if !held {
            return Err(format!($($msg)+));
        }
    };
}

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
    /// vertex.
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
        .expect("a soup's quads index their own vertices")
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

/// The soup as the level's static body.
pub fn level(soup: &Soup) -> Body {
    Body {
        id: LEVEL,
        kind: BodyKind::Static,
        pose: Pose::IDENTITY,
        collider: Collider::Mesh(Arc::new(soup.mesh())),
    }
}

/// The level and any placements' solids, each 0.05 m smaller in the world
/// as a hero's world holds them ([`HavokSolid::in_world`]).
pub fn bodies(soup: &Soup, solids: &[(BodyId, HavokSolid, Pose)]) -> Vec<Body> {
    let mut bodies = vec![level(soup)];
    for (id, solid, at) in solids {
        bodies.push(Body {
            id: *id,
            kind: BodyKind::Kinematic,
            pose: solid.pose(*at),
            collider: Collider::Convex(solid.in_world()),
        });
    }
    bodies
}

/// Eruptor's solid: a 4 x 6 x 3 ft sphere at 150 % ([`crate::tuning`]'s).
pub fn eruptor_solid() -> HavokSolid {
    crate::tuning::eruptor_solid()
}

/// Eruptor settled at `at` (x and z; y is where he's dropped from) facing
/// `facing`.
pub fn eruptor(seam: &dyn PhysicsSeam, at: Vec3, facing: f32) -> Hero {
    let mut hero = Hero::new(eruptor_solid(), Tuning::ERUPTOR, at, facing);
    hero.settle(seam, &[]);
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
/// (frame index from 0), the view at `view`; the states after each.
pub fn run_viewed(
    seam: &dyn PhysicsSeam,
    hero: &mut Hero,
    frames: usize,
    view: f32,
    stick_at: impl Fn(usize) -> Stick,
) -> Vec<State> {
    (0..frames)
        .map(|k| {
            hero.step(seam, &Input::stick(stick_at(k), view), DT, &[]);
            state(hero)
        })
        .collect()
}

/// [`run_viewed`] with the view at [`VIEW`].
pub fn run(
    seam: &dyn PhysicsSeam,
    hero: &mut Hero,
    frames: usize,
    stick_at: impl Fn(usize) -> Stick,
) -> Vec<State> {
    run_viewed(seam, hero, frames, VIEW, stick_at)
}

pub fn ground_speed(v: Vec3) -> f32 {
    v.x.hypot(v.z)
}

/// Degrees in [0, 360), as headings run: 0 along +z, 90 along +x.
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

/// The wall behind the start, square to the view's forward: open ground with
/// a face at z = 6 looking back at -z.
pub fn walled() -> Soup {
    let mut soup = open_ground();
    soup.wall_z(6.0, [-40.0, 40.0], [-1.0, 5.0], Vec3::NEG_Z);
    soup
}

/// Where he's dropped onto open ground.
const START: Vec3 = Vec3::new(0.0, 1.0, 0.0);

fn on_open_ground(
    name: &'static str,
    check: fn(&dyn PhysicsSeam) -> Result<(), String>,
) -> Scenario {
    Scenario {
        name,
        bodies: bodies(&open_ground(), &[]),
        check,
    }
}

fn on_the_wall(name: &'static str, check: fn(&dyn PhysicsSeam) -> Result<(), String>) -> Scenario {
    Scenario {
        name,
        bodies: bodies(&walled(), &[]),
        check,
    }
}

/// What AG.16's Verify runs on each side of the seam: the open-ground
/// cases, each in a world of level cells only.
pub fn open_ground_scenarios() -> Vec<Scenario> {
    vec![
        settled_he_rides_5_mm_up(),
        top_speed_is_21_5_ft_s_times_the_strength(),
        from_rest_and_letting_go(),
        the_stick_walks_him_against_the_view(),
        no_gate_either_side_of_15_degrees(),
        a_full_reversal_goes_straight_back(),
        half_stick_turns_at_720(),
        let_go_mid_turn_freezes_his_facing(),
        the_push_and_the_slowing_take_the_steps_before_their_own(),
        no_button_lifts_him(),
        the_same_path_at_30_60_and_144_hz(),
    ]
}

/// The wall cases: [`walled`]'s made-up copy of the wall behind the start,
/// and the gate's door as a kinematic solid built from its `CollisionInfo`.
pub fn wall_scenarios() -> Vec<Scenario> {
    vec![
        along_a_wall(),
        half_stick_along_a_wall(),
        head_on_he_stops_dead(),
        let_go_along_a_wall(),
        the_gate_door_stops_him(),
        put_down_under_an_overhang_he_stays(),
    ]
}

/// Open ground with an overhang's underside 1.85 m up, its edge 0.5 m along
/// +z from the origin: clear of a hero standing at the origin, in reach of
/// one dropped from a metre above it.
pub fn overhung() -> Soup {
    let mut soup = open_ground();
    soup.quad(
        [
            Vec3::new(-2.0, 1.85, 0.5),
            Vec3::new(2.0, 1.85, 0.5),
            Vec3::new(2.0, 1.85, 3.0),
            Vec3::new(-2.0, 1.85, 3.0),
        ],
        Vec3::NEG_Y,
    );
    soup
}

/// AG.29: a hero put down where another stood stays there, facing his way,
/// supported, and stands still let go, under an overhang a settle's cast
/// from a metre up meets first.
pub fn put_down_under_an_overhang_he_stays() -> Scenario {
    Scenario {
        name: "put_down_under_an_overhang_he_stays",
        bodies: bodies(&overhung(), &[]),
        check: |seam| {
            let at = Vec3::new(0.0, 0.005, 0.0);
            let mut hero = Hero::new(eruptor_solid(), Tuning::ERUPTOR, at, 30.0);
            hero.stand(seam, &[]);
            ensure!((hero.pivot - at).length() < 1e-4, "moved: {}", hero.pivot);
            ensure!(hero.facing == 30.0, "{}", hero.facing);
            ensure!(hero.footing.supported, "unsupported: {:?}", hero.footing);
            let after = run(seam, &mut hero, 60, |_| Stick::CENTRED);
            ensure!(
                after.iter().all(|s| (s.pivot - at).length() < 1e-4),
                "drifted: {:?}",
                after.last().map(|s| s.pivot)
            );
            let mut dropped = Hero::new(eruptor_solid(), Tuning::ERUPTOR, at, 30.0);
            dropped.settle(seam, &[]);
            ensure!(
                (dropped.pivot - at).length() > 0.1,
                "a settle from a metre up met no overhang: {}",
                dropped.pivot
            );
            Ok(())
        },
    }
}

/// Settled, he rides 5 mm over level ground, supported (AF.9's idle pivot
/// 5.0 mm over the ground at the spawn), and stays put.
pub fn settled_he_rides_5_mm_up() -> Scenario {
    on_open_ground("settled_he_rides_5_mm_up", |seam| {
        let mut hero = eruptor(seam, START, 0.0);
        ensure!((hero.pivot.y - 0.005).abs() < 1e-4, "{}", hero.pivot.y);
        ensure!(hero.footing.supported, "unsupported");
        let after = run(seam, &mut hero, 60, |_| Stick::CENTRED);
        ensure!(after.iter().all(|s| s.pivot == after[0].pivot), "moved");
        Ok(())
    })
}

/// Top speed: 6.553 m each game second within 1 mm at full stick, 21.5 ft/s x
/// the strength at part stick, on level ground.
pub fn top_speed_is_21_5_ft_s_times_the_strength() -> Scenario {
    on_open_ground("top_speed_is_21_5_ft_s_times_the_strength", |seam| {
        for s in [1.0, 0.5, 0.2] {
            let mut hero = eruptor(seam, START, 0.0);
            let states = run(seam, &mut hero, 120, |_| stick(s, 0.0));
            let (a, b) = (states[59].pivot, states[119].pivot);
            let metres = ground_speed(b - a);
            ensure!((metres - TOP * s).abs() < 1e-3, "S {s}: {metres}");
            ensure!(
                states[60..]
                    .iter()
                    .all(|st| st.pivot.y == states[59].pivot.y),
                "S {s}: rose or sank"
            );
        }
        Ok(())
    })
}

/// Speeding up and slowing down, from rest: 90 % of top from the step that
/// starts 0.1167 s in, 2.8905 m in the first half second (AG.8's 1/60 s
/// predictions, inside its 0.115-0.119 s and 2.88-2.90 m); let go at top
/// speed, 0.826 m in 0.267 s, then still.
pub fn from_rest_and_letting_go() -> Scenario {
    on_open_ground("from_rest_and_letting_go", |seam| {
        let mut hero = eruptor(seam, START, 15.0);
        let start = hero.pivot;
        let states = run(seam, &mut hero, 30, |_| stick(1.0, 0.0));
        let first = states
            .iter()
            .position(|s| ground_speed(s.velocity) >= 0.9 * TOP)
            .ok_or("never at 90 %")?;
        ensure!((first as f32 * DT - 0.1167).abs() < 1e-3, "{first}");
        let half = ground_speed(states[29].pivot - start);
        ensure!((half - 2.8905).abs() < 1e-3, "{half}");

        let released = hero.pivot;
        let stop = run(seam, &mut hero, 90, |_| Stick::CENTRED);
        let moving = stop.iter().filter(|s| s.velocity != Vec3::ZERO).count();
        ensure!(moving == 16, "{moving} frames, not 0.2667 s at 1/60");
        let slid = ground_speed(stop[89].pivot - released);
        ensure!((slid - 0.8263).abs() < 1e-3, "{slid}");
        ensure!((0.81..=0.85).contains(&slid), "{slid}");
        ensure!(
            stop[16..].iter().all(|s| s.pivot == stop[16].pivot),
            "moved after"
        );
        Ok(())
    })
}

/// The stick walks the hero: forward at the start walks him along the view's
/// 15° from his first frame, facing it; left along 105°, his facing reaching
/// it in the fifth frame.
pub fn the_stick_walks_him_against_the_view() -> Scenario {
    on_open_ground("the_stick_walks_him_against_the_view", |seam| {
        for (degrees, want, facing_by) in [(0.0, 15.0, 1), (90.0, 105.0, 5)] {
            let mut hero = eruptor(seam, START, 0.0);
            let states = run(seam, &mut hero, 6, |_| stick(1.0, degrees));
            for s in &states {
                ensure!(
                    angle_between(bearing(s.velocity), want) < 0.01,
                    "{degrees}: {s:?}"
                );
            }
            let reached = states
                .iter()
                .position(|s| s.facing == want)
                .ok_or(format!("{degrees}: never faced {want}"))?
                + 1;
            ensure!(reached == facing_by, "{degrees}: {reached}");
        }
        Ok(())
    })
}

/// From rest at the start: no gate at 15° from his facing. Either side of it
/// by 0.01° the first frame runs the same law: the stick's way at 160 ft/s² x
/// the step, his facing on it.
pub fn no_gate_either_side_of_15_degrees() -> Scenario {
    on_open_ground("no_gate_either_side_of_15_degrees", |seam| {
        let first = |off: f32| {
            let mut hero = eruptor(seam, START, VIEW - off);
            run(seam, &mut hero, 1, |_| stick(1.0, 0.0))[0]
        };
        let (under, over) = (first(15.0 - 0.01), first(15.0 + 0.01));
        for s in [under, over] {
            ensure!(
                (ground_speed(s.velocity) - 160.0 * FOOT * DT).abs() < 1e-4,
                "{s:?}"
            );
            ensure!(angle_between(bearing(s.velocity), VIEW) < 0.01, "{s:?}");
            ensure!((s.facing - VIEW).abs() < 1e-3, "{s:?}");
        }
        ensure!(
            (ground_speed(under.velocity) - ground_speed(over.velocity)).abs() < 1e-6,
            "{under:?} {over:?}"
        );
        Ok(())
    })
}

/// A full reversal: from the next frame he walks back at top speed, never
/// slower, his facing round in 0.13-0.15 s.
pub fn a_full_reversal_goes_straight_back() -> Scenario {
    on_open_ground("a_full_reversal_goes_straight_back", |seam| {
        let mut hero = eruptor(seam, START, VIEW);
        run(seam, &mut hero, 30, |_| stick(1.0, 0.0));
        let before = hero.velocity;
        let back = run(seam, &mut hero, 30, |_| stick(1.0, 180.0));
        ensure!((back[0].velocity + before).length() < 1e-3, "{:?}", back[0]);
        ensure!(
            back.iter()
                .all(|s| (ground_speed(s.velocity) - TOP).abs() < 1e-3),
            "slowed"
        );
        let round = back
            .iter()
            .position(|s| angle_between(s.facing, VIEW + 180.0) < 1e-3)
            .ok_or("never round")?
            + 1;
        let seconds = round as f32 * DT;
        ensure!((0.13..=0.15).contains(&seconds), "{seconds}");
        Ok(())
    })
}

/// Part stick: his facing turns at 720°/s at half stick, while he walks the
/// stick's way at once.
pub fn half_stick_turns_at_720() -> Scenario {
    on_open_ground("half_stick_turns_at_720", |seam| {
        let mut hero = eruptor(seam, START, VIEW);
        run(seam, &mut hero, 30, |_| stick(0.5, 0.0));
        let turn = run(seam, &mut hero, 3, |_| stick(0.5, 90.0));
        ensure!(
            (turn[0].facing - (VIEW + 12.0)).abs() < 1e-3,
            "{:?}",
            turn[0]
        );
        ensure!(
            (turn[1].facing - (VIEW + 24.0)).abs() < 1e-3,
            "{:?}",
            turn[1]
        );
        ensure!(
            angle_between(bearing(turn[0].velocity), VIEW + 90.0) < 0.01,
            "{:?}",
            turn[0]
        );
        Ok(())
    })
}

/// Let go mid-turn: swung 90° at top speed and let go a frame later, he slows
/// along that one frame's stick and his facing stays where it left it.
pub fn let_go_mid_turn_freezes_his_facing() -> Scenario {
    on_open_ground("let_go_mid_turn_freezes_his_facing", |seam| {
        let mut hero = eruptor(seam, START, VIEW);
        run(seam, &mut hero, 30, |_| stick(1.0, 0.0));
        let swung = run(seam, &mut hero, 1, |_| stick(1.0, 90.0))[0];
        ensure!((swung.facing - (VIEW + 24.0)).abs() < 1e-3, "{swung:?}");
        let slowing = run(seam, &mut hero, 40, |_| Stick::CENTRED);
        for s in slowing.iter().filter(|s| s.velocity != Vec3::ZERO) {
            ensure!(
                angle_between(bearing(s.velocity), VIEW + 90.0) < 0.01,
                "{s:?}"
            );
        }
        ensure!(slowing.iter().all(|s| s.facing == swung.facing), "turned");
        ensure!(slowing[39].velocity == Vec3::ZERO, "{:?}", slowing[39]);
        Ok(())
    })
}

/// The lags (AG.8, AG.38): the push and the turn integrate the step two
/// frames before the frame's own, the slowing and gravity the step one
/// before, the move its own. With a 0.05 s hitch in the third frame, the
/// push and his facing's turn it gives show in the fifth, the slowing and,
/// up in the air, gravity in the fourth.
pub fn the_push_and_the_slowing_take_the_steps_before_their_own() -> Scenario {
    on_open_ground(
        "the_push_and_the_slowing_take_the_steps_before_their_own",
        |seam| {
            let mut hero = eruptor(seam, START, VIEW);
            let steps = [DT, DT, 0.05, DT, DT, DT];
            let mut lengths = Vec::new();
            for &step in &steps {
                hero.step(seam, &Input::stick(stick(1.0, 0.0), VIEW), step, &[]);
                lengths.push(hero.walk.velocity.length());
            }
            // Two nominal frames of history before the first.
            let lagged = [DT, DT, DT, DT, 0.05, DT];
            let mut want = 0.0f32;
            for (k, &lag) in lagged.iter().enumerate() {
                want = (want + 160.0 * lag).min(21.5);
                ensure!((lengths[k] - want).abs() < 1e-4, "frame {k}: {lengths:?}");
            }
            // The move takes its own step: the hitch frame moves him 0.05 s
            // worth.
            let mut hero = eruptor(seam, START, VIEW);
            run(seam, &mut hero, 30, |_| stick(1.0, 0.0));
            let before = hero.pivot;
            hero.step(seam, &Input::stick(stick(1.0, 0.0), VIEW), 0.05, &[]);
            let moved = ground_speed(hero.pivot - before);
            ensure!((moved - TOP * 0.05).abs() < 1e-4, "{moved}");
            // Let go: the slowing takes the step before, so the hitch shows a
            // frame after it.
            let mut slowed = Vec::new();
            for &step in &[0.05, DT, DT] {
                hero.step(seam, &Input::stick(Stick::CENTRED, VIEW), step, &[]);
                slowed.push(hero.walk.velocity.length());
            }
            ensure!(
                (slowed[0] - (21.5 - 80.0 * 0.05)).abs() < 1e-4,
                "{slowed:?}"
            );
            ensure!(
                (slowed[1] - (slowed[0] - 80.0 * 0.05)).abs() < 1e-4,
                "{slowed:?}"
            );
            ensure!(
                (slowed[2] - (slowed[1] - 80.0 * DT)).abs() < 1e-4,
                "{slowed:?}"
            );
            // His facing: the stick 90° left of the view from facing 0, so
            // 24° a nominal frame to 105°, the hitch's 72° in the fifth.
            let mut hero = eruptor(seam, START, 0.0);
            let mut facings = Vec::new();
            for &step in &steps {
                hero.step(seam, &Input::stick(stick(1.0, 90.0), VIEW), step, &[]);
                facings.push(hero.facing);
            }
            for (k, want) in [24.0, 48.0, 72.0, 96.0, 105.0, 105.0].iter().enumerate() {
                ensure!(
                    (facings[k] - want).abs() < 1e-3,
                    "facing, frame {k}: {facings:?}"
                );
            }
            // Gravity, 30 m up: 136.21 ft/s² x the step before, the hitch's
            // in the fourth frame.
            let mut hero = Hero::new(
                eruptor_solid(),
                Tuning::ERUPTOR,
                Vec3::new(0.0, 30.0, 0.0),
                VIEW,
            );
            let mut was = 0.0f32;
            let mut gained = Vec::new();
            for &step in &steps {
                hero.step(seam, &Input::stick(Stick::CENTRED, VIEW), step, &[]);
                gained.push(was - hero.walk.velocity.y);
                was = hero.walk.velocity.y;
            }
            for (k, &lag) in [DT, DT, DT, 0.05, DT, DT].iter().enumerate() {
                ensure!(
                    (gained[k] - 136.21 * lag).abs() < 1e-3,
                    "gravity, frame {k}: {gained:?}"
                );
            }
            Ok(())
        },
    )
}

/// No jumping: the face buttons change nothing, frame for frame.
pub fn no_button_lifts_him() -> Scenario {
    on_open_ground("no_button_lifts_him", |seam| {
        let walk = |buttons: [bool; 4]| {
            let mut hero = eruptor(seam, START, VIEW);
            (0..60)
                .map(|k| {
                    let input = Input {
                        stick: stick(1.0, if k < 30 { 0.0 } else { 45.0 }),
                        view_heading: VIEW,
                        buttons,
                    };
                    hero.step(seam, &input, DT, &[]);
                    (hero.pivot, hero.velocity)
                })
                .collect::<Vec<_>>()
        };
        let none = walk([false; 4]);
        ensure!(
            none == walk([true, false, false, false]),
            "the bottom button"
        );
        ensure!(none == walk([true; 4]), "all four");
        ensure!(
            none.iter().all(|(p, v)| p.y == none[0].0.y && v.y == 0.0),
            "left the ground"
        );
        Ok(())
    })
}

/// The same walk at any frame rate: the same stick, held for the first
/// second of game time and let go, leaves him in the same place after 2 s
/// whether the display runs at 30, 60 or 144 Hz. This drives
/// [`FixedClock`]; guacamole's own test of the requirement runs Bevy's fixed
/// schedule under each display rate.
pub fn the_same_path_at_30_60_and_144_hz() -> Scenario {
    on_open_ground("the_same_path_at_30_60_and_144_hz", |seam| {
        let mut ends = Vec::new();
        for rate in [30.0, 60.0, 144.0] {
            let mut hero = eruptor(seam, START, VIEW);
            let mut clock = FixedClock::default();
            let mut steps = 0;
            for _ in 0..(rate as usize * 2) {
                for _ in 0..clock.advance(1.0 / rate) {
                    let s = if steps < 60 {
                        stick(1.0, 30.0)
                    } else {
                        Stick::CENTRED
                    };
                    hero.step(seam, &Input::stick(s, VIEW), clock.step(), &[]);
                    steps += 1;
                }
            }
            ensure!(steps == 120, "{rate} Hz: {steps} steps");
            ends.push(hero.pivot);
        }
        ensure!(ends[0] == ends[1] && ends[1] == ends[2], "{ends:?}");
        Ok(())
    })
}

/// Walks him from 3 m short of the wall at `degrees` from head-on, `s` of
/// the stick, for `frames` frames, the view facing +z so forward is
/// head-on.
fn into_the_wall(seam: &dyn PhysicsSeam, degrees: f32, s: f32, frames: usize) -> Vec<State> {
    let mut hero = eruptor(seam, Vec3::new(0.0, 1.0, 3.0), 0.0);
    run_viewed(seam, &mut hero, frames, 0.0, |_| stick(s, -degrees))
}

/// Walls: along the wall at his speed x 1 − 0.1235 (1 − sin θ), θ from
/// head-on, within 6 mm/s of the medians AG.9 measured at 1.99°, 5.04°,
/// 10.1°, 20°, 45° and 69.83° (5.7669, 5.8103, 5.8820, 6.0183, 6.3164 and
/// 6.5030-6.5037 m/s: w ran 0.1218-0.1250 per recording, 0.1235 their
/// middle), held 5 mm off it and never more than 1.1 mm higher than where he
/// met it.
pub fn along_a_wall() -> Scenario {
    on_the_wall("along_a_wall", |seam| {
        for (degrees, median) in [
            (1.99, 5.7669),
            (5.04, 5.8103),
            (10.1, 5.8820),
            (20.0, 6.0183),
            (45.0, 6.3164),
            (69.83, 6.5034),
        ] {
            let states = into_the_wall(seam, degrees, 1.0, 90);
            let met = states
                .iter()
                .position(|s| s.pivot.z > 6.0 - 0.9144 - 0.0051)
                .ok_or(format!("{degrees}°: never met the wall"))?;
            let sliding = &states[met + 2..];
            ensure!(!sliding.is_empty(), "{degrees}°");
            for s in sliding {
                let speed = ground_speed(s.velocity);
                ensure!((speed - median).abs() < 6e-3, "{degrees}°: {speed}");
                ensure!(s.velocity.z.abs() < 1e-4, "{degrees}°: {s:?}");
                let gap = 6.0 - (s.pivot.z + 0.9144);
                ensure!((gap - 0.005).abs() < 2e-4, "{degrees}°: gap {gap}");
                ensure!(
                    s.pivot.y - states[met].pivot.y <= 1.1e-3,
                    "{degrees}°: rose"
                );
            }
        }
        Ok(())
    })
}

/// Half stick along a wall: 45.7° from head-on, 0.965 of his speed.
pub fn half_stick_along_a_wall() -> Scenario {
    on_the_wall("half_stick_along_a_wall", |seam| {
        let states = into_the_wall(seam, 45.7, 0.5, 120);
        let s = states[119];
        let share = ground_speed(s.velocity) / (TOP * 0.5);
        ensure!((share - 0.965).abs() < 1e-3, "{share}");
        Ok(())
    })
}

/// Head-on: he stops dead the frame he reaches the wall, from top speed,
/// 5 mm off its face, and stays while the stick holds; within 0.15° of
/// head-on too; at 1.06° he creeps along at the push's share, 0.12 m/s.
pub fn head_on_he_stops_dead() -> Scenario {
    on_the_wall("head_on_he_stops_dead", |seam| {
        let stop = 6.0 - 0.9144 - 0.005;
        let states = into_the_wall(seam, 0.0, 1.0, 90);
        let met = states
            .iter()
            .position(|s| s.pivot.z > stop - 1e-4)
            .ok_or("never met the wall")?;
        ensure!(
            (states[met].pivot.z - stop).abs() < 1e-4,
            "{:?}",
            states[met]
        );
        ensure!(
            (ground_speed(states[met - 1].velocity) - TOP).abs() < 1e-3,
            "{:?}",
            states[met - 1]
        );
        ensure!(
            states[met + 1..]
                .iter()
                .all(|s| s.pivot == states[met].pivot),
            "moved after"
        );
        for angle in [0.04, 0.15] {
            let states = into_the_wall(seam, angle, 1.0, 90);
            ensure!(
                states[89].velocity == Vec3::ZERO,
                "{angle}°: {:?}",
                states[89]
            );
        }
        let creep = into_the_wall(seam, 1.06, 1.0, 90);
        let speed = ground_speed(creep[89].velocity);
        ensure!(
            (speed - TOP * 1.06f32.to_radians().sin()).abs() < 2e-3,
            "{speed}"
        );
        Ok(())
    })
}

/// Let go along a wall at 45°, he slows along it as on open ground, his
/// ground speed 0.9639 of his walk's (AG.9: stopped 0.81-0.84 m on).
pub fn let_go_along_a_wall() -> Scenario {
    on_the_wall("let_go_along_a_wall", |seam| {
        let mut hero = eruptor(seam, Vec3::new(0.0, 1.0, 4.5), 0.0);
        run_viewed(seam, &mut hero, 30, 0.0, |_| stick(1.0, -45.0));
        let released = hero.pivot;
        for _ in 0..40 {
            hero.step(seam, &Input::stick(Stick::CENTRED, 0.0), DT, &[]);
            if hero.velocity != Vec3::ZERO {
                let share = ground_speed(hero.velocity) / (hero.walk.velocity.length() * FOOT);
                ensure!((share - 0.9639).abs() < 1e-3, "{share}");
            }
        }
        let slid = ground_speed(hero.pivot - released);
        ensure!((0.78..=0.82).contains(&slid), "{slid}");
        Ok(())
    })
}

/// The gate's door's solid as AG.7 read it: 4 x 21 x 34 ft with a quarter
/// turn of its own.
pub fn gate_door() -> HavokSolid {
    SolidInfo {
        kind: SolidKind::Box,
        offset: [0.0; 3],
        rotation: [0.0, 90.0, 0.0],
        width: 4.0,
        height: 21.0,
        length: 34.0,
    }
    .havok([100.0; 3])
    .expect("a box has a shape")
}

/// The gate's door, built from its CollisionInfo (4 x 21 x 34 ft, its own
/// quarter turn, heading 180): walked into head-on he stops with his pivot
/// 1.4781 m from its middle (AG.7) within the millimetre his 5 mm keep makes
/// of its 4.1 mm; along x it's 10.36 m wide, so 4 m off its middle he walks
/// past it.
pub fn the_gate_door_stops_him() -> Scenario {
    let at = Pose::new(Vec3::new(0.0, 0.0, 5.0), euler([0.0, 180.0, 0.0]));
    Scenario {
        name: "the_gate_door_stops_him",
        bodies: bodies(&open_ground(), &[(BodyId(2), gate_door(), at)]),
        check: |seam| {
            let mut hero = eruptor(seam, Vec3::new(2.0, 1.0, 0.0), 0.0);
            let states = run(seam, &mut hero, 90, |_| stick(1.0, -VIEW));
            let s = states[89];
            ensure!(s.velocity == Vec3::ZERO, "{s:?}");
            let from_middle = 5.0 - s.pivot.z;
            ensure!((from_middle - 1.4781).abs() < 1.5e-3, "{from_middle}");
            let mut past = eruptor(seam, Vec3::new(-4.0, 1.0, 3.5), 0.0);
            let along = run(seam, &mut past, 60, |_| stick(1.0, -VIEW - 90.0));
            ensure!(
                along
                    .iter()
                    .all(|s| (ground_speed(s.velocity) - TOP).abs() < 1e-3 || s.velocity.x < 0.0),
                "held up past the door"
            );
            Ok(())
        },
    }
}

/// A face 3 m high leaning out 4.8° over level ground at y = -3, as the
/// overhang under the ledge route's west end leans over the corridor, its
/// top edge open (nothing beyond it, as at that overhang: AG.48).
pub fn overhang() -> Soup {
    let mut soup = Soup::default();
    soup.floor(-3.0, [-0.25, 20.0], [-10.0, 10.0], 2.0);
    soup.quad(
        [
            Vec3::new(0.0, 0.0, -10.0),
            Vec3::new(0.0, 0.0, 10.0),
            Vec3::new(-0.25, -3.0, 10.0),
            Vec3::new(-0.25, -3.0, -10.0),
        ],
        Vec3::X,
    );
    soup
}

fn over_the_overhang(
    name: &'static str,
    check: fn(&dyn PhysicsSeam) -> Result<(), String>,
) -> Scenario {
    Scenario {
        name,
        bodies: bodies(&overhang(), &[]),
        check,
    }
}

/// What AG.48 runs on each side of the seam: a hero falling onto the
/// overhang's open edge.
pub fn open_edge_scenarios() -> Vec<Scenario> {
    vec![
        round_an_open_edge_he_falls_on(),
        an_open_edge_within_45_holds_him_up(),
    ]
}

/// Eruptor dropped, the stick let go, from `at` (his pivot), unsettled.
fn dropped(at: Vec3) -> Hero {
    Hero::new(eruptor_solid(), Tuning::ERUPTOR, at, 0.0)
}

/// Falling and landing (AG.48: AG.39's `g39_air_rel` and `g39_air_rev`):
/// let go, he falls onto the overhang's open edge 0.8 m out from it, where
/// it lies 61° below him, is held off it (his keep, 5-10 mm against a
/// rounded edge) and slides round it, down past the face and onto the
/// ground below, moving every frame until he lands there, never behind the
/// face.
pub fn round_an_open_edge_he_falls_on() -> Scenario {
    over_the_overhang("round_an_open_edge_he_falls_on", |seam| {
        let mut hero = dropped(Vec3::new(0.8, 1.0, 0.0));
        let mut touched = 0;
        for k in 0..60 {
            let before = hero.pivot;
            hero.step(seam, &Input::stick(Stick::CENTRED, VIEW), DT, &[]);
            ensure!(
                hero.pivot.x >= 0.8 - 1e-4,
                "frame {k}: behind the edge at {}",
                hero.pivot
            );
            if hero.footing.supported {
                ensure!(
                    hero.pivot.y < -2.9 && touched > 0,
                    "frame {k}: held at {} after {touched} frames on the edge",
                    hero.pivot
                );
                ensure!((hero.pivot.y + 3.0 - 0.005).abs() < 6e-3, "{}", hero.pivot);
                return Ok(());
            }
            ensure!(
                (hero.pivot - before).length() > 1e-3,
                "frame {k}: hung in the air at {}",
                hero.pivot
            );
            let centre = hero.pivot + Vec3::Y * 0.9144;
            let gap = centre.x.hypot(centre.y.max(0.0)) - 0.9144;
            if centre.y > 0.0 && gap < 0.02 {
                ensure!(gap > 0.004, "frame {k}: {gap} m off the edge");
                touched += 1;
            }
        }
        Err(format!("never landed: {}", hero.pivot))
    })
}

/// Falling and landing (AG.48: AG.39's let-gos at the ledge route's west
/// end, held on the overhang's open edge at 41.6-45.0° below them): let go
/// 0.45 m out from the open edge, where it lies 29° below him, he lands on
/// it, slides round it a few centimetres as he lands and stays there, held
/// up, the edge still within 45° below him.
pub fn an_open_edge_within_45_holds_him_up() -> Scenario {
    over_the_overhang("an_open_edge_within_45_holds_him_up", |seam| {
        let mut hero = dropped(Vec3::new(0.45, 1.0, 0.0));
        for _ in 0..90 {
            hero.step(seam, &Input::stick(Stick::CENTRED, VIEW), DT, &[]);
        }
        ensure!(hero.footing.supported, "unsupported at {}", hero.pivot);
        ensure!(hero.velocity == Vec3::ZERO, "moving {}", hero.velocity);
        let centre = hero.pivot + hero.up * 0.9144;
        let from_edge = Vec3::new(centre.x, centre.y, 0.0);
        let gap = from_edge.length() - 0.9144;
        let below = from_edge.angle_between(Vec3::Y).to_degrees();
        ensure!(
            (0.004..=0.012).contains(&gap) && below < 45.0 && (hero.pivot.x - 0.45).abs() < 0.1,
            "{}: {gap} m off the edge, {below}° below him",
            hero.pivot
        );
        Ok(())
    })
}

/// What AG.70 and AG.71 run on each side of the seam: the engine's
/// post-step as read (his lean off the ground and with his facing, the snap
/// only while his lean turns, a frame begun on the ground ending supported
/// within the snap's reach), a solid he's inside letting him out slowly and
/// sliding him plain, and the edge probe under a solid hanging over an edge
/// and its search through a solid's radius.
pub fn post_step_scenarios() -> Vec<Scenario> {
    vec![
        off_the_ground_his_lean_turns_upright(),
        off_the_ground_his_lean_turns_with_his_facing(),
        near_ground_he_can_stand_on_he_leans_to_it(),
        off_a_step_he_floats_until_his_lean_turns(),
        a_solid_he_is_inside_lets_him_out_slowly(),
        a_still_solid_slides_him_plain_inside_it_and_the_frame_he_meets_it(),
        the_probe_passes_under_a_solid_over_an_edge(),
        a_search_from_inside_a_solids_radius_goes_on_through_it(),
    ]
}

/// Up `degrees` off straight up toward +x.
fn tilted(degrees: f32) -> Vec3 {
    let a = degrees.to_radians();
    Vec3::new(a.sin(), a.cos(), 0.0)
}

/// He leans with the ground, in the air (AG.71: AG.39's falls, 351 of 390
/// records): dropped from 3 m leaning 10°, his up axis turns back toward
/// straight up at 120°/s, 2° a frame, the last frame taking what's left.
pub fn off_the_ground_his_lean_turns_upright() -> Scenario {
    fn check(seam: &dyn PhysicsSeam) -> Result<(), String> {
        let mut hero = dropped(Vec3::new(0.0, 3.0, 0.0));
        hero.up = tilted(10.0);
        let mut before = crate::ground::angle(hero.up, Vec3::Y);
        for k in 0..5 {
            hero.step(seam, &Input::stick(Stick::CENTRED, VIEW), DT, &[]);
            ensure!(!hero.footing.supported, "frame {k}: landed from 3 m");
            let now = crate::ground::angle(hero.up, Vec3::Y);
            let want = (before - 2.0).max(0.0);
            ensure!(
                (now - want).abs() < 1e-3,
                "frame {k}: {now}° off up, want {want}°"
            );
            before = now;
        }
        Ok(())
    }
    Scenario {
        name: "off_the_ground_his_lean_turns_upright",
        bodies: bodies(&open_ground(), &[]),
        check,
    }
}

/// He leans with the ground, turning with his facing (AG.71: his lean is
/// his transform's; over AG.39's falls it predicts his lean in the air to
/// 0.11°, 2.3° without): dropped from 3 m leaning 10° toward +x with the
/// stick turning him, each frame his up axis turns about the vertical by
/// what his facing turned, and 2° back toward straight up.
pub fn off_the_ground_his_lean_turns_with_his_facing() -> Scenario {
    fn check(seam: &dyn PhysicsSeam) -> Result<(), String> {
        let mut hero = dropped(Vec3::new(0.0, 3.0, 0.0));
        hero.up = tilted(10.0);
        let side = |up: Vec3| bearing(Vec3::new(up.x, 0.0, up.z));
        for k in 0..4 {
            let (facing, toward) = (hero.facing, side(hero.up));
            let tilt = crate::ground::angle(hero.up, Vec3::Y);
            hero.step(seam, &Input::stick(stick(1.0, -90.0), 0.0), DT, &[]);
            ensure!(!hero.footing.supported, "frame {k}: landed from 3 m");
            let swung = (hero.facing - facing + 180.0).rem_euclid(360.0) - 180.0;
            ensure!(swung.abs() > 5.0, "frame {k}: his facing turned {swung}°");
            let now = side(hero.up);
            let want = (toward + swung).rem_euclid(360.0);
            ensure!(
                angle_between(now, want) < 1e-2,
                "frame {k}: his lean faces {now}°, want {want}° (turned {swung}°)"
            );
            let left = crate::ground::angle(hero.up, Vec3::Y);
            ensure!(
                (left - (tilt - 2.0)).abs() < 1e-3,
                "frame {k}: {left}° off up, want {}°",
                tilt - 2.0
            );
        }
        Ok(())
    }
    Scenario {
        name: "off_the_ground_his_lean_turns_with_his_facing",
        bodies: bodies(&open_ground(), &[]),
        check,
    }
}

/// He leans with the ground, within the snap's reach of ground he can stand
/// on (AG.71: the 8 records within 9 cm of the ground finder's point): 0.3 m
/// over a 20° slope and not on it, his up axis turns toward its normal, not
/// toward straight up.
pub fn near_ground_he_can_stand_on_he_leans_to_it() -> Scenario {
    fn check(seam: &dyn PhysicsSeam) -> Result<(), String> {
        let n = Vec3::new(0.0, 20f32.to_radians().cos(), -20f32.to_radians().sin());
        let over = Vec3::new(0.0, 6.0 * 20f32.to_radians().tan() + 0.3, 4.0);
        let mut hero = dropped(over);
        hero.step(seam, &Input::stick(Stick::CENTRED, VIEW), DT, &[]);
        ensure!(
            !hero.footing.supported,
            "on the slope at once: {}",
            hero.pivot
        );
        let turned = crate::ground::angle(hero.up, Vec3::Y);
        ensure!(
            (turned - 2.0).abs() < 1e-3,
            "turned {turned}°, want 2° a frame"
        );
        ensure!(
            (crate::ground::angle(hero.up, n) - 18.0).abs() < 1e-3,
            "turned away from the slope's normal: {}",
            hero.up
        );
        Ok(())
    }
    let mut soup = Soup::default();
    soup.ramp(Vec3::new(0.0, 0.0, -2.0), 20.0, [-6.0, 6.0], 12.0);
    Scenario {
        name: "near_ground_he_can_stand_on_he_leans_to_it",
        bodies: bodies(&soup, &[]),
        check,
    }
}

/// Steps and drops, the post-step as read (AG.70: along the gate's door the
/// original's Eruptor rode up to 0.288 m over ground falling away under him,
/// supported, until a new slope turned his lean and the snap set him 10 mm
/// over it): walked off a 0.1 m step onto level ground he ends every frame
/// supported, no band pulling his walk down, riding the step's height over
/// the lower ground; where it falls away at 3° his lean turns and the snap
/// sets him 10 mm over it.
pub fn off_a_step_he_floats_until_his_lean_turns() -> Scenario {
    fn check(seam: &dyn PhysicsSeam) -> Result<(), String> {
        let mut hero = eruptor(seam, Vec3::new(0.0, 1.0, 0.0), 0.0);
        let states = run_viewed(seam, &mut hero, 150, 0.0, |_| stick(1.0, 0.0));
        let (mut floated, mut snapped) = (0, None);
        for (k, s) in states.iter().enumerate() {
            let h = s.height.unwrap_or(f32::NAN);
            if s.pivot.z > 3.0 && s.pivot.z < 9.5 {
                ensure!(s.supported, "frame {k}: unsupported at {}", s.pivot);
                ensure!(s.walk.y.abs() < 1e-4, "frame {k}: walk {}", s.walk);
                ensure!((h - 0.105).abs() < 2e-3, "frame {k}: {h} m up");
                floated += 1;
            }
            if s.pivot.z > 10.0 && snapped.is_none() {
                ensure!(
                    (h - 0.01).abs() < 1e-3 || h > 0.1,
                    "frame {k}: {h} m over the slope at {}",
                    s.pivot
                );
                if (h - 0.01).abs() < 1e-3 {
                    snapped = Some(k);
                }
            }
        }
        ensure!(floated > 30, "floated {floated} frames");
        let last = states.last().unwrap();
        ensure!(
            snapped.is_some() && last.supported,
            "never set down over the slope: {:?}",
            last
        );
        Ok(())
    }
    let mut soup = Soup::default();
    soup.floor(0.1, [-6.0, 6.0], [-6.0, 2.0], 2.0)
        .floor(0.0, [-6.0, 6.0], [2.0, 10.0], 2.0)
        .ramp(Vec3::new(0.0, 0.0, 10.0), -3.0, [-6.0, 6.0], 30.0);
    Scenario {
        name: "off_a_step_he_floats_until_his_lean_turns",
        bodies: bodies(&soup, &[]),
        check,
    }
}

/// Walls (AG.70: AG.58's door, 8-9 cm inside it, let him out at 0.983 of
/// his depth a second over every step): standing 4 cm inside a solid's face
/// he moves out of it at his depth a second, 0.67 mm the first frame and
/// 2.5 cm in a second, where the level's mesh still sets him off at once.
pub fn a_solid_he_is_inside_lets_him_out_slowly() -> Scenario {
    fn check(seam: &dyn PhysicsSeam) -> Result<(), String> {
        let mut hero = Hero::new(eruptor_solid(), Tuning::ERUPTOR, Vec3::Y * KEEP, 0.0);
        hero.footing = crate::walk::Footing::level(KEEP);
        let start = hero.pivot;
        let states = run(seam, &mut hero, 60, |_| Stick::CENTRED);
        let first = (states[0].pivot - start).x;
        ensure!(
            (first + 0.04 / 60.0).abs() < 1e-4,
            "first frame {first} m along x"
        );
        let out = -(states[59].pivot - start).x;
        let want = 0.04 * (1.0 - (1.0f32 - 1.0 / 60.0).powi(60));
        ensure!(
            (out - want).abs() < 5e-4,
            "out {out} m in a second, want {want}"
        );
        Ok(())
    }
    // A box whose face stands 4 cm inside his keep: his query sphere (his
    // own grown by the radius the world takes off the box) reaches x 0.9144
    // and the keep 5 mm past that.
    let face = 0.9144 + KEEP - 0.04;
    let solid = HavokSolid {
        shape: Shape::Box {
            half_extents: Vec3::new(1.0, 1.0, 1.0),
            margin: crate::solid::CONVEX_RADIUS,
        },
        centre: Vec3::ZERO,
        rotation: Quat::IDENTITY,
    };
    Scenario {
        name: "a_solid_he_is_inside_lets_him_out_slowly",
        bodies: bodies(
            &open_ground(),
            &[(BodyId(2), solid, Pose::at(Vec3::new(face + 1.0, 1.0, 0.0)))],
        ),
        check,
    }
}

/// Inside a solid (AG.70: along AG.58's gate's door, his lean tipping him
/// 2-9 cm into its box, theirs slid at the walk's own share along it, and
/// the frame he met it the same): walking at top speed 45° into a still
/// solid's face, more than 2 cm inside it he slides plain, at the push's
/// share along it, where 1 cm inside the wall law takes him; met halfway
/// through a frame he slides plain for the rest of it and from the next
/// frame by the law.
pub fn a_still_solid_slides_him_plain_inside_it_and_the_frame_he_meets_it() -> Scenario {
    fn walking(at: f32) -> Hero {
        let mut hero = Hero::new(
            eruptor_solid(),
            Tuning::ERUPTOR,
            Vec3::new(at, KEEP, 0.0),
            45.0,
        );
        hero.footing = crate::walk::Footing::level(KEEP);
        hero.walk.velocity = Vec3::new(1.0, 0.0, 1.0).normalize() * 21.5;
        hero
    }
    fn check(seam: &dyn PhysicsSeam) -> Result<(), String> {
        let push = Input::stick(stick(1.0, 45.0), 0.0);
        let plain = TOP * 45f32.to_radians().sin();
        let law = TOP * (1.0 - crate::proxy::WALL_SLOWING * (1.0 - 45f32.to_radians().sin()));
        let along = |hero: &mut Hero| {
            let z = hero.pivot.z;
            hero.step(seam, &push, DT, &[]);
            (hero.pivot.z - z) / DT
        };
        // 4 cm inside, then 1 cm: plain, then the law.
        for (at, want, what) in [(0.0, plain, "4 cm in"), (-0.03, law, "1 cm in")] {
            let speed = along(&mut walking(at));
            ensure!(
                (speed - want).abs() < 5e-3,
                "{what}: along it at {speed} m/s, want {want}"
            );
        }
        // Touching at x -0.04: met halfway through the frame.
        let mut hero = walking(-0.04 - 0.5 * plain * DT);
        let first = along(&mut hero);
        ensure!(
            (hero.pivot.x + 0.04).abs() < 1e-3,
            "never met it: {}",
            hero.pivot
        );
        ensure!(
            (first - plain).abs() < 5e-3,
            "the frame he meets it: {first} m/s along it, want {plain}"
        );
        let next = along(&mut hero);
        ensure!(
            (next - law).abs() < 5e-3,
            "the frame after: {next} m/s along it, want {law}"
        );
        Ok(())
    }
    // A box whose face stands 4 cm inside his keep with him at x 0, as
    // a_solid_he_is_inside_lets_him_out_slowly's.
    let face = 0.9144 + KEEP - 0.04;
    let solid = HavokSolid {
        shape: Shape::Box {
            half_extents: Vec3::new(1.0, 1.0, 1.0),
            margin: crate::solid::CONVEX_RADIUS,
        },
        centre: Vec3::ZERO,
        rotation: Quat::IDENTITY,
    };
    Scenario {
        name: "a_still_solid_slides_him_plain_inside_it_and_the_frame_he_meets_it",
        bodies: bodies(
            &open_ground(),
            &[(BodyId(2), solid, Pose::at(Vec3::new(face + 1.0, 1.0, 0.0)))],
        ),
        check,
    }
}

/// An edge with nothing below (AG.70: past the gate's door's end the
/// original's slow probe passed while its point lay under the door's box
/// with its radius, 3-7 cm under its bottom, and refused within 1.7 mm past
/// its face): a solid hanging over an open edge, its bottom 0.07 m over the
/// floor's level, lets the probe pass anywhere under it past the edge, and
/// a millimetre past its side the probe refuses.
pub fn the_probe_passes_under_a_solid_over_an_edge() -> Scenario {
    fn check(seam: &dyn PhysicsSeam) -> Result<(), String> {
        let pivot = Vec3::new(0.0, 0.005, 1.0);
        for (x, z, want) in [(0.5, 3.0, true), (0.999, 3.0, true), (1.001, 3.0, false)] {
            let point = Vec3::new(x, 0.005, z);
            let got = crate::edge::walkable(seam, pivot, point, &[]);
            ensure!(got == want, "at ({x}, {z}): {got}, want {want}");
        }
        Ok(())
    }
    let mut soup = Soup::default();
    soup.floor(0.0, [-6.0, 6.0], [-6.0, 2.0], 2.0);
    // Its box with its radius: x -1..1, y 0.07..2.07, z 1.5..5.5.
    let solid = HavokSolid {
        shape: Shape::Box {
            half_extents: Vec3::new(0.95, 0.95, 1.95),
            margin: crate::solid::CONVEX_RADIUS,
        },
        centre: Vec3::ZERO,
        rotation: Quat::IDENTITY,
    };
    Scenario {
        name: "the_probe_passes_under_a_solid_over_an_edge",
        bodies: bodies(
            &soup,
            &[(BodyId(2), solid, Pose::at(Vec3::new(0.0, 1.07, 3.5)))],
        ),
        check,
    }
}

/// An edge with nothing below, the probe's search (AG.70): seeing a solid
/// with its radius, a ray starting inside that radius goes on through it as
/// one starting inside the solid does. Under a box whose radius takes in the
/// down search's start (0.1 m over the point; the box's bottom with its
/// radius at 0.07, as the world holds it at 0.12), the search finds the
/// floor 1 m below, not the radius's face.
pub fn a_search_from_inside_a_solids_radius_goes_on_through_it() -> Scenario {
    fn check(seam: &dyn PhysicsSeam) -> Result<(), String> {
        let found = crate::ground::find_seeing(
            seam,
            Vec3::new(0.5, 0.0, 0.0),
            crate::solid::MESH_RADIUS,
            &[],
        )
        .ok_or("found nothing")?;
        ensure!(
            (found.point.y + 1.0).abs() < 1e-4,
            "found {found:?}, want the floor at -1"
        );
        Ok(())
    }
    let mut soup = Soup::default();
    soup.floor(-1.0, [-6.0, 6.0], [-6.0, 6.0], 2.0);
    // Its box with its radius: x and z -1..1, y 0.07..2.07.
    let solid = HavokSolid {
        shape: Shape::Box {
            half_extents: Vec3::splat(0.95),
            margin: crate::solid::CONVEX_RADIUS,
        },
        centre: Vec3::ZERO,
        rotation: Quat::IDENTITY,
    };
    Scenario {
        name: "a_search_from_inside_a_solids_radius_goes_on_through_it",
        bodies: bodies(
            &soup,
            &[(BodyId(2), solid, Pose::at(Vec3::new(0.0, 1.07, 0.0)))],
        ),
        check,
    }
}
