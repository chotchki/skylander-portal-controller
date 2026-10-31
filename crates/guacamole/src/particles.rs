//! What an emitter the scripts spawn draws (openspec AH.87): its
//! generators' particles born, aged and laid out as quads each frame, on
//! the extract's particle part (`guac_ir::particle`).
//!
//! A sprite generator makes particles at its rate, at most its most alive
//! at once, each at a point of its spawn volume in its frame on the
//! emitter, leaving along its direction (the generator's turn, out from the
//! volume's centre or into it) within its spread, at its speed; each lives
//! its lifespan, its drag slowing it and its gravity pulling it down (up
//! where negative), its colour, size and spin keyed at the start, the
//! middle (its midpoint percent through its life, none where that's
//! negative) and the end, each with its own draw of the key's randomness.
//! One that moves with its emitter (`inherits_translation`) keeps its place
//! relative to the generator. A rate of [`BURST_RATE`] or more is a burst:
//! its most alive at once, born together, once. A ribbon lays a strip along
//! where its emitter has been, its last `segments` places, tapered and
//! coloured from head to tail.
//!
//! The emitter's clone the scripts take off stops it, and what it made
//! lives out its life: the throw's flash is taken off a step after it
//! comes (AH.87's probe), so the original's particles outlive their clone.
//! One taken off before it made its most alive still makes them, at its
//! rate (the eruption's sparks and smoke go a step after they come, and
//! the disc gives them 25 and 12): our reading, not a measurement.
//!
//! Every random number a particle takes is the particles' own
//! ([`Cosmetic`]), never the session's generator: the original's particles
//! draw on host time from the one generator the scripts roll on (AF.8), so
//! drawing as many as it does would move every later roll with the frame
//! rate; ours move nothing that plays (the design doc's decision).
//!
//! Drawn on the character material (AE's remaster look): unlit, the
//! texture times the vertex colour times two (a sprite generator's 0x7F is
//! full strength; a ribbon's 0xFF, so its colour goes in halved), alpha
//! the texture's times the colour's, added or blended by it, writing no
//! depth, the runtime's fog and point lights owned off; a generator with a
//! bloom glows by its alpha ([`model::Glow::LayerAlpha`]). Quads face the
//! camera (or lie in the generator's plane, a view alignment of 1: a scorch
//! mark on the ground), wound to face it, so one-sided keys draw them.

use bevy::math::{Quat, Vec3};
use guac_ir::model::{self, Glow, Mask, Params, RenderStates, Segment, TextureRole, TextureSlot};
use guac_ir::particle::{
    Blend, Direction, Emitters, Generator, Keyed, Kind, Ribbon, Sprite, Volume,
};

use crate::character::key::bit;
use crate::level_scene::Drawn;

/// A rate at or past this makes a burst: the disc's bursts are 9999 a
/// second, its streams 120 or under.
pub const BURST_RATE: f32 = 1000.0;

/// The shortest life a particle lives, so a lifespan of 0 still draws.
pub const SHORTEST_LIFE: f32 = 1.0 / 60.0;

/// The particles' own random numbers: SplitMix64, seeded once per TV.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Cosmetic(u64);

impl Cosmetic {
    pub fn new(seed: u64) -> Self {
        Self(seed)
    }

    pub fn next_u64(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    /// In [0, 1).
    pub fn unit(&mut self) -> f32 {
        (self.next_u64() >> 40) as f32 / (1u64 << 24) as f32
    }
}

/// Where an emitter stands this frame: its place (metres), its turn and its
/// scale (1 at 100 %).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Frame {
    pub at: Vec3,
    pub turn: Quat,
    pub scale: f32,
}

impl Frame {
    /// A generator's own frame on its emitter: its offset moved and turned
    /// with the emitter.
    pub fn of(&self, g: &Generator) -> Frame {
        let t = Vec3::from_array(g.offset.translation);
        let r = Quat::from_array(g.offset.rotation);
        Frame {
            at: self.at + self.turn * (t * self.scale),
            turn: self.turn * r,
            scale: self.scale,
        }
    }
}

/// A sprite one generator made: where it is and how it moves, how old
/// it is, and its own draws of the keys' randomness.
#[derive(Debug, Clone, PartialEq)]
pub struct Particle {
    /// Metres: in the level's frame, or from its generator's place for one
    /// that moves with its emitter.
    pub at: Vec3,
    pub velocity: Vec3,
    pub age: f32,
    pub life: f32,
    /// Radians in the plane it's drawn in.
    pub rotation: f32,
    /// 1 or -1.
    pub spin: f32,
    /// Its own draws, in [0, 1), of the colour's, the size's and the spin's
    /// randomness.
    pub draws: [f32; 3],
}

/// A ribbon's place, newest first, and how long ago it was laid.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Point {
    pub at: Vec3,
    pub age: f32,
}

/// One generator's state on one emitter.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct GeneratorState {
    /// Seconds since the emitter came.
    pub clock: f32,
    /// Particles owed past whole ones, carried to the next frame.
    pub carry: f32,
    /// Particles made since the emitter came, which a burst and a stopped
    /// emitter's last few go by.
    pub made: u32,
    pub particles: Vec<Particle>,
    pub points: Vec<Point>,
    pub since_point: f32,
}

/// The middle key's place in a life, `None` where it has none.
pub fn midpoint(percent: f32) -> Option<f32> {
    (percent > 0.0 && percent < 100.0).then_some(percent / 100.0)
}

/// Start, middle and end at `t` (0-1) of a life, the middle at `mid`.
fn keyed3(v: [f32; 3], t: f32, mid: Option<f32>) -> f32 {
    match mid {
        Some(m) if t < m => v[0] + (v[1] - v[0]) * (t / m),
        Some(m) => v[1] + (v[2] - v[1]) * ((t - m) / (1.0 - m)).min(1.0),
        None => v[0] + (v[2] - v[0]) * t.min(1.0),
    }
}

/// A keyed value at `t`, each key its base plus `draw` of its randomness.
pub fn keyed_f32(k: &Keyed<f32>, t: f32, mid: Option<f32>, draw: f32) -> f32 {
    keyed3(
        [
            k.start + draw * k.start_random,
            k.mid + draw * k.mid_random,
            k.end + draw * k.end_random,
        ],
        t,
        mid,
    )
}

/// A keyed colour at `t`, each channel's code (0-255) as a float.
pub fn keyed_colour(k: &Keyed<[u8; 4]>, t: f32, mid: Option<f32>, draw: f32) -> [f32; 4] {
    std::array::from_fn(|c| {
        let at = |base: [u8; 4], random: [u8; 4]| {
            (f32::from(base[c]) + draw * f32::from(random[c])).min(255.0)
        };
        keyed3(
            [
                at(k.start, k.start_random),
                at(k.mid, k.mid_random),
                at(k.end, k.end_random),
            ],
            t,
            mid,
        )
    })
}

/// A keyed size at `t`, metres.
pub fn keyed_size(k: &Keyed<[f32; 3]>, t: f32, mid: Option<f32>, draw: f32) -> [f32; 3] {
    std::array::from_fn(|c| {
        keyed3(
            [
                k.start[c] + draw * k.start_random[c],
                k.mid[c] + draw * k.mid_random[c],
                k.end[c] + draw * k.end_random[c],
            ],
            t,
            mid,
        )
    })
}

/// Any unit vector `perpendicular` to `d`.
fn perpendicular(d: Vec3) -> Vec3 {
    let other = if d.x.abs() < 0.9 { Vec3::X } else { Vec3::Y };
    d.cross(other).normalize_or_zero()
}

/// `axis` turned by up to `spread` radians, every way round equally.
fn spread_about(axis: Vec3, spread: f32, rng: &mut Cosmetic) -> Vec3 {
    let axis = axis.normalize_or(Vec3::Y);
    if spread <= 0.0 {
        return axis;
    }
    let theta = rng.unit() * spread.min(std::f32::consts::PI);
    let phi = rng.unit() * std::f32::consts::TAU;
    let u = perpendicular(axis);
    let v = axis.cross(u);
    (axis * theta.cos() + (u * phi.cos() + v * phi.sin()) * theta.sin()).normalize_or(axis)
}

/// A point of the spawn volume in the generator's own axes (metres, before
/// its turn and scale): a box `size` across, a sphere of radius `size.x`,
/// a cylinder of radius `size.x` in its own x-y plane, `size.z` along z.
fn volume_point(volume: Option<Volume>, size: [f32; 3], rng: &mut Cosmetic) -> Vec3 {
    let [x, y, z] = size;
    match volume.unwrap_or(Volume::Box) {
        Volume::Box => Vec3::new(
            (rng.unit() - 0.5) * x,
            (rng.unit() - 0.5) * y,
            (rng.unit() - 0.5) * z,
        ),
        Volume::Sphere => {
            let d = spread_about(Vec3::Y, std::f32::consts::PI, rng);
            d * x * rng.unit().cbrt()
        }
        Volume::Cylinder => {
            let phi = rng.unit() * std::f32::consts::TAU;
            let r = x * rng.unit().sqrt();
            Vec3::new(r * phi.cos(), r * phi.sin(), (rng.unit() - 0.5) * z)
        }
    }
}

impl GeneratorState {
    /// One frame of `dt` seconds: what's alive ages and moves, then the
    /// generator makes what it owes (only while `on`, the emitter's clone
    /// still in play, or until it's made its most alive), at `frame`, its
    /// emitter's place this frame.
    pub fn step(&mut self, g: &Generator, frame: Frame, on: bool, dt: f32, rng: &mut Cosmetic) {
        self.clock += dt;
        let at = frame.of(g);
        match &g.kind {
            Kind::Sprite(s) => self.step_sprite(s, at, on, dt, g.start_delay, rng),
            Kind::Ribbon(r) => self.step_ribbon(r, at, on, dt, g.start_delay),
        }
    }

    fn step_sprite(
        &mut self,
        s: &Sprite,
        at: Frame,
        on: bool,
        dt: f32,
        delay: f32,
        rng: &mut Cosmetic,
    ) {
        let mid = midpoint(s.midpoint_percent);
        for p in &mut self.particles {
            p.age += dt;
            let t = (p.age / p.life).min(1.0);
            let drag = keyed3(s.drag, t, mid);
            let gravity = keyed3(s.gravity, t, mid);
            p.velocity.y -= gravity * at.scale * dt;
            p.velocity *= (1.0 - drag * dt).max(0.0);
            p.at += p.velocity * dt;
            p.rotation += p.spin * keyed_f32(&s.spin, t, mid, p.draws[2]) * dt;
        }
        self.particles.retain(|p| p.age < p.life);
        if self.clock < delay {
            return;
        }
        let most = u32::from(s.max_alive);
        let owed = if s.spawn_rate >= BURST_RATE {
            if self.made == 0 { most } else { 0 }
        } else if on || self.made < most {
            self.carry += s.spawn_rate * s.next_gen_scale * dt;
            let whole = self.carry.floor();
            self.carry -= whole;
            let mut n = whole as u32;
            if !on {
                n = n.min(most - self.made);
            }
            n
        } else {
            0
        };
        let room = most.saturating_sub(self.particles.len() as u32);
        for _ in 0..owed.min(room) {
            let p = born(s, at, rng);
            self.particles.push(p);
            self.made += 1;
        }
    }

    fn step_ribbon(&mut self, r: &Ribbon, at: Frame, on: bool, dt: f32, delay: f32) {
        for p in &mut self.points {
            p.age += dt;
        }
        // Laid no faster than a frame's 60th of a second, so the strip's
        // length is its segments' worth of the frames that laid it.
        let every = 1.0 / r.granularity.clamp(1.0, 60.0);
        let lasts = f32::from(r.segments.max(1)) * every;
        self.points.retain(|p| p.age <= lasts);
        if !on || self.clock < delay {
            return;
        }
        self.since_point += dt;
        if self.points.is_empty() || self.since_point >= every {
            self.points.insert(
                0,
                Point {
                    at: at.at,
                    age: 0.0,
                },
            );
            self.since_point = 0.0;
        } else {
            self.points[0].at = at.at;
        }
        self.points.truncate(usize::from(r.segments.max(1)) + 1);
    }

    /// Whether anything of it is left to draw.
    pub fn live(&self) -> bool {
        !self.particles.is_empty() || self.points.len() > 1
    }
}

/// A particle born of `s` at its generator's frame `at`.
fn born(s: &Sprite, at: Frame, rng: &mut Cosmetic) -> Particle {
    let local = volume_point(s.volume, s.volume_size, rng);
    let offset = at.turn * (local * at.scale);
    let axis = at.turn * Vec3::Z;
    let direction = match s.direction.unwrap_or(Direction::Orientation) {
        Direction::Orientation => axis,
        Direction::Outward => offset.normalize_or(axis),
        Direction::Inward => -offset.normalize_or(-axis),
    };
    let speed = (s.speed + rng.unit() * s.speed_random) * at.scale;
    let velocity = spread_about(direction, s.spread, rng) * speed;
    let life = (s.lifespan + rng.unit() * s.lifespan_random).max(SHORTEST_LIFE);
    let rotation = s.rotation + rng.unit() * s.rotation_random;
    let spin = if s.random_spin_direction && rng.unit() < 0.5 {
        -1.0
    } else {
        1.0
    };
    let draws = [rng.unit(), rng.unit(), rng.unit()];
    Particle {
        at: if s.inherits_translation {
            offset
        } else {
            at.at + offset
        },
        velocity,
        age: 0.0,
        life,
        rotation,
        spin,
        draws,
    }
}

/// The view a quad faces: the camera's right and up, and where it is.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct View {
    pub right: Vec3,
    pub up: Vec3,
    pub eye: Vec3,
}

/// Quads as a segment's vertices, built a quad at a time.
#[derive(Debug, Default)]
struct Quads {
    positions: Vec<[f32; 3]>,
    normals: Vec<[f32; 3]>,
    tangents: Vec<[f32; 4]>,
    uvs: Vec<[f32; 2]>,
    colours: Vec<[u8; 4]>,
    indices: Vec<u16>,
}

impl Quads {
    /// Corners in counter-clockwise order as `view` sees them when `facing`
    /// is toward its eye, else the other way round; their UVs alongside.
    fn push(&mut self, corners: [Vec3; 4], uvs: [[f32; 2]; 4], colour: [u8; 4], facing: Vec3) {
        let Ok(base) = u16::try_from(self.positions.len()) else {
            return;
        };
        if base > u16::MAX - 4 {
            return;
        }
        let normal = (corners[1] - corners[0])
            .cross(corners[2] - corners[0])
            .normalize_or_zero();
        let flip = normal.dot(facing) < 0.0;
        let tangent = (corners[1] - corners[0]).normalize_or(Vec3::X);
        let n = if flip { -normal } else { normal };
        for (c, uv) in corners.iter().zip(uvs) {
            self.positions.push(c.to_array());
            self.normals.push(n.to_array());
            self.tangents.push([tangent.x, tangent.y, tangent.z, 1.0]);
            self.uvs.push(uv);
            self.colours.push(colour);
        }
        let order: [u16; 6] = if flip {
            [0, 2, 1, 0, 3, 2]
        } else {
            [0, 1, 2, 0, 2, 3]
        };
        self.indices.extend(order.map(|i| base + i));
    }

    fn segment(self, material: u16) -> Option<Segment> {
        if self.indices.is_empty() {
            return None;
        }
        Some(Segment {
            material,
            source: 0,
            positions: self.positions,
            normals: self.normals,
            tangents: Some(self.tangents),
            uvs: vec![self.uvs],
            colors: Some(self.colours),
            joints: Vec::new(),
            weights: Vec::new(),
            indices: self.indices,
        })
    }
}

/// A colour's four codes as the vertex takes them, rounded.
fn codes(c: [f32; 4]) -> [u8; 4] {
    c.map(|v| v.round().clamp(0.0, 255.0) as u8)
}

/// What `state` draws of `g` this frame as one segment on `material`,
/// its places from `origin` (where its entity stands); `None` with nothing
/// alive. `frame` is the emitter's this frame (or where it was last).
pub fn segment(
    state: &GeneratorState,
    g: &Generator,
    frame: Frame,
    view: View,
    origin: Vec3,
    material: u16,
) -> Option<Segment> {
    let at = frame.of(g);
    let mut quads = Quads::default();
    match &g.kind {
        Kind::Sprite(s) => {
            let mid = midpoint(s.midpoint_percent);
            for p in &state.particles {
                let t = (p.age / p.life).min(1.0);
                let world = if s.inherits_translation {
                    at.at + p.at
                } else {
                    p.at
                };
                let c = world - origin;
                let size = keyed_size(&s.size, t, mid, p.draws[1]);
                let (w, h) = (
                    size[0].max(0.0) * at.scale * 0.5,
                    size[1].max(0.0) * at.scale * 0.5,
                );
                let (axis_x, axis_y) = if s.view_alignment == 1 {
                    (at.turn * Vec3::X, at.turn * Vec3::Y)
                } else {
                    (view.right, view.up)
                };
                let (sin, cos) = p.rotation.sin_cos();
                let r = (axis_x * cos + axis_y * sin) * w;
                let u = (axis_y * cos - axis_x * sin) * h;
                let colour = codes(keyed_colour(&s.colour, t, mid, p.draws[0]));
                // The disc stores an image bottom row first, so its foot
                // sits at a v of 0.
                quads.push(
                    [c - r - u, c + r - u, c + r + u, c - r + u],
                    [[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]],
                    colour,
                    view.eye - world,
                );
            }
        }
        Kind::Ribbon(r) => {
            let n = state.points.len();
            let scale_u = r.scale_uv_percent / 100.0;
            let scroll = r.scroll_uv_percent / 100.0 * state.clock;
            let place = |i: usize| {
                let f = i as f32 / (n - 1).max(1) as f32;
                let taper = match r.taper {
                    1 => 1.0 - f,
                    2 => f,
                    3 => 1.0 - (2.0 * f - 1.0).abs(),
                    _ => 1.0,
                };
                let colour: [f32; 4] = std::array::from_fn(|c| {
                    let (a, b) = (f32::from(r.head_colour[c]), f32::from(r.tail_colour[c]));
                    let v = a + (b - a) * f;
                    // A ribbon's 0xFF is full strength: halved for the
                    // shader's doubling, its alpha as stored.
                    if c < 3 { v / 2.0 } else { v }
                });
                (f, taper * r.width * frame.scale * 0.5, codes(colour))
            };
            for i in 0..n.saturating_sub(1) {
                let (a, b) = (state.points[i].at, state.points[i + 1].at);
                let along = (a - b).normalize_or_zero();
                if along == Vec3::ZERO {
                    continue;
                }
                let middle = (a + b) * 0.5;
                let side = along.cross(view.eye - middle).normalize_or_zero();
                let (fa, wa, ca) = place(i);
                let (fb, wb, _) = place(i + 1);
                let (ua, ub) = (fa * scale_u + scroll, fb * scale_u + scroll);
                let (a, b) = (a - origin, b - origin);
                quads.push(
                    [b - side * wb, a - side * wa, a + side * wa, b + side * wb],
                    [[ub, 1.0], [ua, 1.0], [ua, 0.0], [ub, 0.0]],
                    ca,
                    view.eye - middle,
                );
            }
        }
    }
    quads.segment(material)
}

/// The runtime's bits a particle material owns off: shadows, fog and the
/// point lights, on either side.
const RUNTIME_PIXEL: u32 = 1 << bit::SHADOWS | 1 << bit::FOG | 0b111 << bit::POINT_LIGHTS;
const RUNTIME_VERTEX: u32 = 1 << bit::SHADOWS | 1 << bit::FOG | 1 << bit::VERTEX_POINT_LIT;

/// The material generator `k` of `emitter` draws with (the module docs);
/// `None` for one with no texture, which draws nothing.
pub fn material(emitter: &str, k: usize, g: &Generator) -> Option<model::Material> {
    let t = g.texture.as_ref()?;
    let (blend, alpha_test) = match g.blend {
        Blend::Add => (model::Blend::Additive, None),
        Blend::Blend => (model::Blend::Alpha, None),
        Blend::Knockout => (model::Blend::Alpha, Some(0.5)),
    };
    Some(model::Material {
        name: format!("particle {emitter}#{k}"),
        pixel: Mask {
            base: 0,
            ignore: RUNTIME_PIXEL,
        },
        vertex: Mask {
            base: 1 << bit::VERTEX_COLOUR,
            ignore: RUNTIME_VERTEX,
        },
        textures: vec![TextureSlot {
            role: TextureRole::Layer1ColorMap,
            texture: t.texture.clone(),
            image: t.image.clone(),
            sampler: t.sampler,
        }],
        params: Params {
            // Below zero: the texture's own alpha.
            fixed_alpha: [-1.0, -1.0, 0.0, 0.0],
            layer1_extra: [0.0; 4],
            layer2_extra: None,
            layer2_blend: None,
            layer3_blend: None,
            layer4_blend: [0.0; 4],
            layer4_values: [0.0; 4],
            material_tint: [128, 128, 128, 255],
            glow: if g.bloom > 0.0 {
                Glow::LayerAlpha
            } else {
                Glow::None
            },
        },
        states: RenderStates {
            blend,
            depth_write: false,
            two_sided: false,
            alpha_test,
        },
        scrolls: Vec::new(),
        tracks: Vec::new(),
    })
}

/// One triangle with the shape every particle segment has (one UV set,
/// tangents, colours): what keys a particle material.
pub fn shape(material: u16) -> Segment {
    let mut q = Quads::default();
    q.push(
        [Vec3::ZERO, Vec3::X, Vec3::new(1.0, 1.0, 0.0), Vec3::Y],
        [[0.0; 2]; 4],
        [0; 4],
        Vec3::Z,
    );
    let mut s = q.segment(material).expect("a quad");
    s.indices.truncate(3);
    s
}

/// Each emitter's generators by name, each with its material in a drawn
/// part of their own, where it has one: what keys them and what the level
/// draws them with.
pub type Index = Vec<(String, Vec<(usize, Option<u16>)>)>;

/// `emitters`' materials as a drawn part, one shape segment a material, and
/// the index into it. A name a second archive holds again keeps the first.
pub fn drawn(emitters: &[Emitters]) -> (Drawn, Index) {
    let mut out = Drawn::default();
    let mut index: Index = Vec::new();
    for e in emitters.iter().flat_map(|a| &a.emitters) {
        if index.iter().any(|(n, _)| *n == e.name) {
            continue;
        }
        let mut generators = Vec::with_capacity(e.generators.len());
        for (k, g) in e.generators.iter().enumerate() {
            let m = material(&e.name, k, g).and_then(|m| {
                let i = u16::try_from(out.materials.len()).ok()?;
                out.materials.push(m);
                out.segments.push(shape(i));
                out.sources.push(0);
                Some(i)
            });
            generators.push((k, m));
        }
        index.push((e.name.clone(), generators));
    }
    (out, index)
}

#[cfg(test)]
mod tests;
