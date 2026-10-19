//! The level readers on guest memory laid out in code: the offsets are typed
//! in from AF.8's fresh state, not taken from the reader's constants, so a
//! wrong constant there shows.

use super::*;
use crate::actors::{Actor, ActorFrame, Gather};

const STATICS: u32 = 0x008E_0000;
const HEAP: u32 = 0x3000_0000;
/// Where the synthetic level section sits (level 213's section 0 sat at
/// 0x34304E00).
const SECTION: u32 = 0x3430_4E00;

struct Mem {
    regions: Vec<(u32, Vec<u8>)>,
    next: u32,
}

impl Mem {
    fn new() -> Self {
        Self {
            regions: vec![
                (STATICS, vec![0; 0x2_0000]),
                (HEAP, vec![0; 0x4_0000]),
                (SECTION, vec![0; 0x1_0000]),
            ],
            next: HEAP,
        }
    }

    fn slot(&mut self, addr: u32, len: usize) -> &mut [u8] {
        for (base, bytes) in &mut self.regions {
            if addr >= *base && (addr - *base) as usize + len <= bytes.len() {
                let at = (addr - *base) as usize;
                return &mut bytes[at..at + len];
            }
        }
        panic!("{addr:#x}+{len} is outside the fixture");
    }

    fn put32(&mut self, addr: u32, v: u32) {
        self.slot(addr, 4).copy_from_slice(&v.to_be_bytes());
    }

    fn put8(&mut self, addr: u32, v: u8) {
        self.slot(addr, 1)[0] = v;
    }

    fn putfs(&mut self, addr: u32, vs: &[f32]) {
        for (i, v) in vs.iter().enumerate() {
            self.put32(addr + 4 * i as u32, v.to_bits());
        }
    }

    fn alloc(&mut self, len: usize) -> u32 {
        let at = self.next;
        self.next = (at + len as u32).next_multiple_of(16);
        at
    }

    fn object(&mut self, vtable: u32, len: usize) -> u32 {
        let at = self.alloc(len);
        self.put32(at, vtable);
        self.put32(at + 4, 1);
        at
    }

    fn string(&mut self, s: &str) -> u32 {
        let at = self.alloc(s.len() + 1);
        self.slot(at, s.len()).copy_from_slice(s.as_bytes());
        at
    }

    /// An `hkArray` at `at`: data pointer, count, capacity with the
    /// don't-deallocate flag the game's arrays carry.
    fn hk_array(&mut self, at: u32, data: u32, count: u32) {
        self.put32(at, data);
        self.put32(at + 4, count);
        self.put32(at + 8, 0x8000_0000 | count);
    }
}

impl Gather for Mem {
    fn gather(&mut self, ranges: &[(u32, usize)]) -> anyhow::Result<Vec<Option<Vec<u8>>>> {
        Ok(ranges
            .iter()
            .map(|&(addr, len)| {
                self.regions.iter().find_map(|(base, bytes)| {
                    let at = addr.checked_sub(*base)? as usize;
                    bytes.get(at..at.checked_add(len)?).map(<[u8]>::to_vec)
                })
            })
            .collect())
    }
}

/// Where [`world`] put things.
struct World {
    world: u32,
    island: u32,
    fixed: u32,
    body: u32,
    query: u32,
    mesh: u32,
    entities: u32,
}

/// The vertices, triangles (with welding words) and materials of a ramp.
const VERTICES: [[f32; 3]; 4] = [
    [-511.062_53, 64.358_99, -196.920_38],
    [-510.418_12, 64.358_99, -194.299_13],
    [-509.791_05, 82.120_44, -316.738_65],
    [-509.770_17, 64.358_99, -201.869_98],
];
const TRIANGLES: [[u16; 4]; 2] = [[0, 1, 2, 0x1234], [2, 3, 0, 0x0001]];
const MATERIALS: [u8; 2] = [188, 0];
const TRANSLATION: [f32; 3] = [-425.419_16, -37.632_298, -15.965_546];

/// The world from the static down: its own fixed body, then the level's
/// with the ramp mesh, as the fresh state has them.
fn world(m: &mut Mem) -> World {
    let world = m.object(0x006F_6830, 0x128);
    m.put32(0x008E_1ED0, world);
    m.putfs(world + 0x10, &[0.0, -9.8, 0.0, 0.0]);
    let island = m.object(0x006F_7010, 0x60);
    m.put32(world + 0x20, island);
    m.put32(island + 0x18, world);
    let fixed = m.object(0x006F_6268, 0x130);
    m.put32(fixed + 0x08, world);
    m.put8(fixed + 0xE8, 5);
    let body = m.object(0x006F_6268, 0x130);
    m.put32(body + 0x08, world);
    m.put8(body + 0xE8, 5);
    m.putfs(
        body + 0xF0,
        &[1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0],
    );
    m.putfs(body + 0x120, &TRANSLATION);
    let query = m.object(0x0063_4268, 0x70);
    m.put32(body + 0x10, query);
    let mesh = m.object(0x0063_4178, 0x68);
    m.put32(query + 0x34, mesh);
    m.putfs(mesh + 0x3C, &[0.05]);
    m.put8(mesh + 0x40, 4);
    let verts = m.alloc(48);
    m.putfs(verts, VERTICES.as_flattened());
    m.hk_array(mesh + 0x44, verts, 12);
    let idx = m.alloc(16);
    for (k, w) in TRIANGLES.as_flattened().iter().enumerate() {
        m.slot(idx + 2 * k as u32, 2)
            .copy_from_slice(&w.to_be_bytes());
    }
    m.hk_array(mesh + 0x50, idx, 8);
    let mats = m.alloc(2);
    m.slot(mats, 2).copy_from_slice(&MATERIALS);
    m.hk_array(mesh + 0x5C, mats, 2);
    let entities = m.alloc(16);
    m.put32(entities, fixed);
    m.put32(entities + 4, body);
    m.hk_array(island + 0x4C, entities, 2);
    World {
        world,
        island,
        fixed,
        body,
        query,
        mesh,
        entities,
    }
}

#[test]
fn the_level_collision_reads_from_the_world_down() {
    let mut m = Mem::new();
    let w = world(&mut m);
    let c = collision(&mut m).unwrap();
    assert_eq!(
        c,
        LiveCollision {
            world: w.world,
            body: w.body,
            query: w.query,
            mesh: w.mesh,
            fixed_bodies: 2,
            motion_type: 5,
            rotation: [1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0],
            translation: TRANSLATION,
            radius: 0.05,
            welding_type: 4,
            vertices: VERTICES.to_vec(),
            triangles: TRIANGLES.to_vec(),
            materials: MATERIALS.to_vec(),
        }
    );
}

#[test]
fn a_collision_chain_off_its_layout_is_refused() {
    type Break = fn(&mut Mem, &World);
    let broken: [(&str, Break); 9] = [
        ("isn't the world's", |m, w| {
            m.put32(w.island + 0x18, w.fixed)
        }),
        ("two fixed bodies", |m, w| m.put32(w.entities, w.body)),
        ("no fixed body", |m, w| m.put32(w.body + 0x10, 0)),
        ("not fixed", |m, w| m.put8(w.body + 0xE8, 1)),
        ("isn't the world's", |m, w| m.put32(w.body + 0x08, 0)),
        ("tfbSimpleMeshShape", |m, w| m.put32(w.mesh, 0x0063_4100)),
        ("don't agree", |m, w| m.put32(w.mesh + 0x60, 3)),
        ("don't agree", |m, w| m.put32(w.mesh + 0x48, 11)),
        ("indexes past", |m, w| {
            let idx = u32::from_be_bytes(m.slot(w.mesh + 0x50, 4).try_into().unwrap());
            m.slot(idx, 2).copy_from_slice(&4u16.to_be_bytes());
        }),
    ];
    for (want, f) in broken {
        let mut m = Mem::new();
        let w = world(&mut m);
        f(&mut m, &w);
        let e = collision(&mut m).unwrap_err().to_string();
        assert!(e.contains(want), "wanted {want:?}, got {e}");
    }
    // The world's vtable, and an array past its bound.
    let mut m = Mem::new();
    let w = world(&mut m);
    m.put32(w.world, 0x006F_6800);
    assert!(
        collision(&mut m)
            .unwrap_err()
            .to_string()
            .contains("hkpWorld")
    );
    let mut m = Mem::new();
    let w = world(&mut m);
    m.put32(w.island + 0x50, (1 << 20) + 1);
    assert!(
        collision(&mut m)
            .unwrap_err()
            .to_string()
            .contains("elements")
    );
}

/// A placement in the section at `off`: its vtable, name, flag pool,
/// matrix, state, scale, collision and corpus.
fn place(m: &mut Mem, off: u32, vtable: u32, name: &str, state: u32) -> u32 {
    let at = SECTION + off;
    m.put32(at, vtable);
    let n = m.string(name);
    m.put32(at + 0x08, n);
    m.put32(at + 0x1C, 0x8);
    m.putfs(
        at + 0x3C,
        &[14.830_149, 43.367_107, -82.224_2, 0.0, 180.0, 0.0],
    );
    m.put32(at + 0x54, state);
    m.putfs(at + 0xB8, &[100.0, 100.0, 100.0]);
    at
}

#[test]
fn placements_read_at_their_sections_base() {
    let mut m = Mem::new();
    let door = place(
        &mut m,
        0x7864,
        0x0062_1048,
        "Breakable_Stone_Wall_Castle_206(1)",
        1,
    );
    m.put32(door + 0xC4, SECTION + 0x7000);
    m.put32(door + 0xF4, 0x35A3_9924);
    place(&mut m, 0x5798, 0x0062_A3C8, "Cave_Dust_motes", 2);
    place(&mut m, 0x5220, 0x0062_C0F8, "213_EvilRobot_Beam_light", 5);
    let objects = [
        (0x7864, Kind::Actor),
        (0x5798, Kind::Emitter),
        (0x5220, Kind::Light),
    ];
    let p = placements(&mut m, SECTION, &objects).unwrap();
    assert_eq!(
        p[0],
        LivePlacement {
            offset: 0x7864,
            address: door,
            kind: Kind::Actor,
            name: Some("Breakable_Stone_Wall_Castle_206(1)".into()),
            activity_state: 1,
            flag_pool: 0x8,
            matrix: [14.830_149, 43.367_107, -82.224_2, 0.0, 180.0, 0.0],
            scale: [100.0; 3],
            collision: Some(SECTION + 0x7000),
            corpus: Some(0x35A3_9924),
        }
    );
    let states: Vec<_> = p
        .iter()
        .map(|p| (p.name.clone().unwrap(), p.activity_state))
        .collect();
    assert_eq!(
        states,
        [
            ("Breakable_Stone_Wall_Castle_206(1)".to_owned(), 1),
            ("Cave_Dust_motes".to_owned(), 2),
            ("213_EvilRobot_Beam_light".to_owned(), 5),
        ]
    );
    // An emitter or light keeps no corpus word; a base 4 bytes off reads
    // no vtable where one should be.
    assert_eq!((p[1].corpus, p[2].corpus), (None, None));
    let e = placements(&mut m, SECTION + 4, &objects)
        .unwrap_err()
        .to_string();
    assert!(e.contains("tfbActorInfo"), "{e}");
    let e = placements(&mut m, SECTION, &[(0x7864, Kind::Emitter)])
        .unwrap_err()
        .to_string();
    assert!(e.contains("tfbParticleInfo"), "{e}");
}

fn actor(name: &str, corpus: u32, placement: u32) -> Actor {
    Actor {
        corpus,
        placement,
        name: Some(name.into()),
        position: [0.0; 3],
        rotation: [0.0; 3],
        scale: [100.0; 3],
        world: None,
        velocity: [0.0; 3],
        skinned: false,
        pad: None,
    }
}

fn frame(actors: Vec<Actor>) -> ActorFrame {
    ActorFrame {
        flip: Some(2),
        game_frame: 7403,
        frame_delta: 1.0 / 60.0,
        listed: actors.len() as u32,
        actors,
        skipped: Vec::new(),
        camera: None,
    }
}

#[test]
fn the_section_base_is_the_actors_vote() {
    let offsets: BTreeMap<String, u32> = [
        ("Breakable_Stone_Wall_Castle_206(1)", 0x78_64F8),
        ("Push_Block_Template(1)", 0x74_AD54),
        ("Sheep_1", 0x6C_1000),
        ("2_Push_Block_Art_Sheep", 0x70_0000),
    ]
    .into_iter()
    .map(|(n, o)| (n.to_owned(), o))
    .collect();
    let base = 0x3430_4E00;
    let mut actors = vec![
        actor("Breakable_Stone_Wall_Castle_206(1)", 1, base + 0x78_64F8),
        actor("Push_Block_Template(1)", 2, base + 0x74_AD54),
        actor("Sheep_1", 3, base + 0x6C_1000),
        // A clone keeps its template's name and sits on the heap.
        actor("2_Push_Block_Art_Sheep", 4, 0x35A1_77E4),
        // Not on the disc's list at all.
        actor("009_Eruptor", 5, 0x36AC_366C),
    ];
    assert_eq!(
        section_base(&frame(actors.clone()), &offsets).unwrap(),
        SectionBase {
            base,
            votes: 3,
            dissent: 1
        }
    );
    actors.remove(2);
    let e = section_base(&frame(actors.clone()), &offsets)
        .unwrap_err()
        .to_string();
    assert!(e.contains("2 vote(s)"), "{e}");
    let e = section_base(&frame(Vec::new()), &offsets)
        .unwrap_err()
        .to_string();
    assert!(e.contains("no actor"), "{e}");
}

#[test]
fn a_tie_between_two_bases_is_refused() {
    let offsets: BTreeMap<String, u32> = (0..6).map(|i| (format!("p{i}"), 0x100 * i)).collect();
    let actors = (0..6)
        .map(|i| {
            let base = if i < 3 { 0x3430_0000 } else { 0x3530_0000 };
            actor(&format!("p{i}"), i, base + 0x100 * i)
        })
        .collect();
    let e = section_base(&frame(actors), &offsets)
        .unwrap_err()
        .to_string();
    assert!(e.contains("tie"), "{e}");
}

/// A corpus whose block holds a phantom over `shape`, the phantom at
/// heading 180 and at the door's position; `listed` adds it to the
/// world's phantom list.
fn solid(m: &mut Mem, w: &World, shape: u32, in_world: bool, list: &mut Vec<u32>) -> u32 {
    let corpus = m.object(0x0062_1618, 0x80);
    let block = m.object(0x0063_2650, 0x20);
    m.put32(corpus + 0x7C, block);
    let phantom = m.object(0x0063_3F08, 0xE0);
    m.put32(block + 0x1C, phantom);
    m.put32(phantom + 0x10, shape);
    if in_world {
        m.put32(phantom + 0x08, w.world);
        m.put32(phantom + 0x24, 0x35);
        m.put32(phantom + 0x2C, 1);
        list.push(phantom);
    } else {
        m.put32(phantom + 0x2C, 2);
    }
    m.putfs(phantom + 0xA0, &[-1.0, 0.0, 0.0, 0.0]);
    m.putfs(phantom + 0xB0, &[0.0, 1.0, 0.0, 0.0]);
    m.putfs(phantom + 0xC0, &[0.0, 0.0, -1.0, 0.0]);
    m.putfs(phantom + 0xD0, &[14.830_149, 43.367_107, -82.224_2, 1.0]);
    corpus
}

fn close(a: [f32; 3], b: [f32; 3]) -> bool {
    (0..3).all(|i| (a[i] - b[i]).abs() < 1e-6)
}

#[test]
fn a_solid_reads_through_its_phantom_and_turns_its_box() {
    let mut m = Mem::new();
    let w = world(&mut m);
    // The door: a box 0.5596 x 3.1504 x 5.1316 under a quarter turn about y,
    // lifted half its height.
    let half = std::f32::consts::FRAC_1_SQRT_2;
    let boxed = m.object(0x006F_22F0, 0x30);
    m.putfs(boxed + 0x10, &[0.05]);
    m.putfs(boxed + 0x20, &[0.5596, 3.1504, 5.1316]);
    let ct = m.object(0x006F_23B0, 0x50);
    m.putfs(ct + 0x10, &[0.05]);
    m.put32(ct + 0x18, boxed);
    m.putfs(ct + 0x20, &[0.0, 3.2004, 0.0, 1.0]);
    m.putfs(ct + 0x30, &[0.0, half, 0.0, half]);
    m.putfs(ct + 0x40, &[1.0, 1.0, 1.0, 1.0]);
    // A shapeless one: the shared 1 m sphere, never added to the world.
    let sphere = m.object(0x006F_24F8, 0x20);
    m.putfs(sphere + 0x10, &[1.0]);
    let ct2 = m.object(0x006F_23B0, 0x50);
    m.put32(ct2 + 0x18, sphere);
    m.putfs(ct2 + 0x30, &[0.0, 0.0, 0.0, 1.0]);
    let mut list = Vec::new();
    let door = solid(&mut m, &w, ct, true, &mut list);
    let leaves = solid(&mut m, &w, ct2, false, &mut list);
    // An actor whose block holds no phantom, and one with no block.
    let bare = m.object(0x0062_1618, 0x80);
    let block = m.object(0x0063_2650, 0x20);
    m.put32(bare + 0x7C, block);
    let macros = m.object(0x0062_1618, 0x80);
    let phantoms = m.alloc(4 * list.len());
    for (k, p) in list.iter().enumerate() {
        m.put32(phantoms + 4 * k as u32, *p);
    }
    m.hk_array(w.world + 0x11C, phantoms, list.len() as u32);
    // The door's placement keeps its CollisionInfo (solid box, actor
    // surface); the leaves' has none.
    let door_at = place(
        &mut m,
        0x7864,
        0x0062_1048,
        "Breakable_Stone_Wall_Castle_206(1)",
        1,
    );
    m.put32(door_at + 0xC4, SECTION + 0x7000);
    m.put32(SECTION + 0x7000 + 0x18, 0x29);
    let leaves_at = m.object(0x0062_1048, 0xF8);
    let f = frame(vec![
        actor("Breakable_Stone_Wall_Castle_206(1)", door, door_at),
        actor(" leaves blowing along ground ", leaves, leaves_at),
        actor("Eon_2_Keep_Going", bare, SECTION + 0x10),
        actor("Macros_UI", macros, 0x3363_47F0),
    ]);
    let s = solids(&mut m, &f).unwrap();
    assert_eq!(s.len(), 2);
    let d = &s[0];
    assert_eq!(
        (
            d.corpus,
            d.in_world,
            d.world,
            d.broadphase_id,
            d.filter_info
        ),
        (door, true, w.world, 0x35, 1)
    );
    assert_eq!(d.translation, [14.830_149, 43.367_107, -82.224_2]);
    assert_eq!(
        (d.collision, d.properties),
        (Some(SECTION + 0x7000), Some(0x29))
    );
    let Shape::Transformed {
        translation,
        rotation,
        scale,
        child,
    } = &d.shape
    else {
        panic!("{:?}", d.shape);
    };
    assert_eq!(
        (*translation, *rotation, *scale),
        ([0.0, 3.2004, 0.0], [0.0, half, 0.0, half], [1.0; 3])
    );
    assert_eq!(
        **child,
        Shape::Box {
            half_extents: [0.5596, 3.1504, 5.1316],
            radius: 0.05
        }
    );
    assert_eq!(d.shape.primitive(), &**child);
    // Its length (z) runs along world x, its width along z.
    let [x, y, z] = d.primitive_axes();
    assert!(close(z, [-1.0, 0.0, 0.0]), "{z:?}");
    assert!(close(x, [0.0, 0.0, 1.0]), "{x:?}");
    assert!(close(y, [0.0, 1.0, 0.0]), "{y:?}");
    let l = &s[1];
    assert_eq!(
        (
            l.corpus,
            l.in_world,
            l.world,
            l.broadphase_id,
            l.filter_info
        ),
        (leaves, false, 0, 0, 2)
    );
    assert_eq!((l.collision, l.properties), (None, None));
    assert_eq!(
        l.shape.primitive(),
        &Shape::Sphere {
            radius: 1.0,
            address: sphere
        }
    );
    // A block that isn't an animation component is refused.
    m.put32(block, 0x0063_2600);
    let e = solids(&mut m, &f).unwrap_err().to_string();
    assert!(e.contains("animationComponent"), "{e}");
}

#[test]
fn rotate_turns_a_vector_by_a_quaternion() {
    let h = std::f32::consts::FRAC_1_SQRT_2;
    // A quarter turn about y takes z to x and x to -z, as a heading of 90 does.
    assert!(close(
        rotate([0.0, h, 0.0, h], [0.0, 0.0, 1.0]),
        [1.0, 0.0, 0.0]
    ));
    assert!(close(
        rotate([0.0, h, 0.0, h], [1.0, 0.0, 0.0]),
        [0.0, 0.0, -1.0]
    ));
    assert!(close(
        rotate([0.0, 0.0, 0.0, 1.0], [1.0, 2.0, 3.0]),
        [1.0, 2.0, 3.0]
    ));
    // Level 213's Setup_003 trigger: -99 deg about y.
    let q = [0.0, -0.760_4, 0.0, 0.649_4];
    let r = rotate(q, [0.0, 0.0, 1.0]);
    assert!((r[0] - (-99.0f32).to_radians().sin()).abs() < 1e-3, "{r:?}");
}
