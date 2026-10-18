//! AC.15, AC.16 and AC.17 against the oracle: Eruptor's decode checked
//! against the three RSX captures of AC.13's session (2026-10-18, the
//! `09d602fd5` oracle, FIFO reordering off), read with `tools/rrc-dump`.
//! Textures block for block, his segments' indexes, UVs, positions and CMP
//! normals, the idle clip solved from the posed positions and which bind the
//! game skins with. Ignored by default: it reads `GUAC_GIANTS_DUMP` and
//! `GUAC_CAPTURE_DIR` (the folder holding the `*_capture.rrc.gz` files),
//! each from the environment, else `.env.dev`:
//!
//! `cargo test -p guac-formats --release --test giants_capture -- --ignored --nocapture`
//!
//! Everything read stays in memory; nothing is written.

mod common;

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::hash::{DefaultHasher, Hash, Hasher};
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::time::Instant;

use common::{env_or_dotenv, usrdir};
use guac_formats::edge::{AttributeBlock, BoneMap, Segment, Semantic, StreamDesc, VertexFormat};
use guac_formats::havok;
use guac_formats::iga::Archive;
use guac_formats::igz::Igz;
use guac_formats::igz::layouts::{self, Hero, Image2, Layout, Payload, PhysicsModel};
use guac_formats::texture::{Format, MetaImage, Texture};
use rrc_dump::Capture;
use rrc_dump::replay::{Draw, DrawKind, VertexArray};

const MODEL: &str = "/Eruptor.mdl";
const IDLE: &str = "/Eruptor_Idle.ma";
/// Decision 5: posed positions within 1e-4 of the bounding-box diagonal.
const POSITIONS: f64 = 1e-4;
/// Decision 5's clip check: skinned positions within 1e-3 of the diagonal at the solved time.
const CLIP: f64 = 1e-3;
/// Decision 5's skeleton check: translation relative to the extent, and radians.
const SKELETON: f64 = 1e-4;
/// How closely the game's palette (fitted from the posed positions) must follow ours, per joint
/// in radians and in translation, for a bind to count as the one the game skins with. The worst
/// measured is 2.5e-6 rad and 5.5e-6 (2026-10-18); the other bind misses by 0.36 rad.
const BIND_FIT: f64 = 1e-5;
/// J_wrist_R's subtree: Havok's reference pose and the Alchemy inverse joints disagree on these
/// five by a 20.8 deg (0.3631 rad) turn (AC.8).
const WRIST_SUBTREE: [&str; 5] = [
    "J_hand_B1",
    "J_hand_F1",
    "J_hand_U1",
    "J_palm_R",
    "J_wrist_R",
];
/// Bits per CMP field: x, y, z.
const CMP_BITS: [u32; 3] = [11, 11, 10];
/// RSX's `CELL_GCM_VERTEX_*` codes rrc-dump reports as `type_raw`.
const RSX_F32: u32 = 2;
const RSX_F16: u32 = 3;
const RSX_CMP: u32 = 6;
const TRIANGLES: u32 = 5;

type M4 = [[f64; 4]; 4];

// ---- small f64 linear algebra, column vectors, translation in column 3 ----

fn mul(a: &M4, b: &M4) -> M4 {
    std::array::from_fn(|r| std::array::from_fn(|c| (0..4).map(|k| a[r][k] * b[k][c]).sum()))
}

fn transpose(m: &M4) -> M4 {
    std::array::from_fn(|r| std::array::from_fn(|c| m[c][r]))
}

/// Inverse of an affine matrix (last row 0 0 0 1): the 3x3 by its adjugate, then the translation.
fn inverse_affine(m: &M4) -> M4 {
    let c =
        |r0: usize, c0: usize, r1: usize, c1: usize| m[r0][c0] * m[r1][c1] - m[r0][c1] * m[r1][c0];
    let adj = [
        [c(1, 1, 2, 2), -c(0, 1, 2, 2), c(0, 1, 1, 2)],
        [-c(1, 0, 2, 2), c(0, 0, 2, 2), -c(0, 0, 1, 2)],
        [c(1, 0, 2, 1), -c(0, 0, 2, 1), c(0, 0, 1, 1)],
    ];
    let det = m[0][0] * adj[0][0] + m[0][1] * adj[1][0] + m[0][2] * adj[2][0];
    let mut i = [
        [0.0, 0.0, 0.0, 0.0],
        [0.0; 4],
        [0.0; 4],
        [0.0, 0.0, 0.0, 1.0],
    ];
    for r in 0..3 {
        for k in 0..3 {
            i[r][k] = adj[r][k] / det;
        }
        i[r][3] = -(0..3).map(|k| i[r][k] * m[k][3]).sum::<f64>();
    }
    i
}

fn apply(m: &M4, p: [f64; 3]) -> [f64; 3] {
    std::array::from_fn(|r| m[r][0] * p[0] + m[r][1] * p[1] + m[r][2] * p[2] + m[r][3])
}

fn rotate(m: &M4, v: [f64; 3]) -> [f64; 3] {
    std::array::from_fn(|r| m[r][0] * v[0] + m[r][1] * v[1] + m[r][2] * v[2])
}

fn translation(m: &M4) -> [f64; 3] {
    [m[0][3], m[1][3], m[2][3]]
}

fn dist(a: [f64; 3], b: [f64; 3]) -> f64 {
    (0..3).map(|i| (a[i] - b[i]).powi(2)).sum::<f64>().sqrt()
}

/// The angle of `A^T B` over two rotation blocks; atan2 keeps it accurate near zero.
fn rot_angle(a: &M4, b: &M4) -> f64 {
    let m = |i: usize, j: usize| (0..3).map(|k| a[k][i] * b[k][j]).sum::<f64>();
    let c = (m(0, 0) + m(1, 1) + m(2, 2) - 1.0) / 2.0;
    let s = [(2, 1), (0, 2), (1, 0)]
        .iter()
        .map(|&(i, j)| ((m(i, j) - m(j, i)) / 2.0).powi(2))
        .sum::<f64>()
        .sqrt();
    s.atan2(c)
}

fn angle(a: [f64; 3], b: [f64; 3]) -> f64 {
    let dot: f64 = (0..3).map(|i| a[i] * b[i]).sum();
    let cross = [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ];
    cross.iter().map(|x| x * x).sum::<f64>().sqrt().atan2(dot)
}

fn diagonal<'p>(points: impl IntoIterator<Item = &'p [f64; 3]>) -> f64 {
    let (mut lo, mut hi) = ([f64::MAX; 3], [f64::MIN; 3]);
    for p in points {
        for i in 0..3 {
            lo[i] = lo[i].min(p[i]);
            hi[i] = hi[i].max(p[i]);
        }
    }
    dist(lo, hi)
}

/// Solves `a x = b` for each right-hand side by Gaussian elimination with partial pivoting.
fn solve_linear(mut a: Vec<Vec<f64>>, mut b: Vec<Vec<f64>>) -> Vec<Vec<f64>> {
    let n = a.len();
    for col in 0..n {
        let pivot = (col..n)
            .max_by(|&x, &y| a[x][col].abs().total_cmp(&a[y][col].abs()))
            .unwrap();
        assert!(a[pivot][col].abs() > 1e-300, "singular at column {col}");
        a.swap(col, pivot);
        for rhs in &mut b {
            rhs.swap(col, pivot);
        }
        for r in col + 1..n {
            let f = a[r][col] / a[col][col];
            if f == 0.0 {
                continue;
            }
            let (above, below) = a.split_at_mut(r);
            for (x, y) in below[0][col..].iter_mut().zip(&above[col][col..]) {
                *x -= f * y;
            }
            for rhs in &mut b {
                rhs[r] -= f * rhs[col];
            }
        }
    }
    b.iter()
        .map(|rhs| {
            let mut x = vec![0.0; n];
            for r in (0..n).rev() {
                let s: f64 = (r + 1..n).map(|c| a[r][c] * x[c]).sum();
                x[r] = (rhs[r] - s) / a[r][r];
            }
            x
        })
        .collect()
}

// ---- CMP (X11Y11Z10N) words ----

/// A CMP word's fields as two's-complement integers.
fn cmp_fields(word: u32) -> [i32; 3] {
    let mut shift = 0;
    CMP_BITS.map(|bits| {
        let raw = (word >> shift) & ((1 << bits) - 1);
        shift += bits;
        ((raw << (32 - bits)) as i32) >> (32 - bits)
    })
}

/// The SPU's reading of a field: (2s + 1) / (2^b - 1), found by brute force over the captures.
fn spu_decode(word: u32) -> [f64; 3] {
    let f = cmp_fields(word);
    std::array::from_fn(|i| (2.0 * f64::from(f[i]) + 1.0) / f64::from((1 << CMP_BITS[i]) - 1))
}

/// The field before the SPU's floor: (x (2^b - 1) - 1) / 2.
fn spu_unrounded(x: f64, bits: u32) -> f64 {
    (x * f64::from((1 << bits) - 1) - 1.0) / 2.0
}

/// The SPU's write: floor of [`spu_unrounded`], clamped to the field.
fn spu_encode(v: [f64; 3]) -> u32 {
    let mut word = 0;
    let mut shift = 0;
    for (x, bits) in v.into_iter().zip(CMP_BITS) {
        let lo = -(1i64 << (bits - 1));
        let s = (spu_unrounded(x, bits).floor() as i64).clamp(lo, -lo - 1);
        word |= ((s as u32) & ((1 << bits) - 1)) << shift;
        shift += bits;
    }
    word
}

/// RPCS3's reading: each field shifted into 16 bits over 32767.
fn rpcs3_decode(word: u32) -> [f64; 3] {
    let f = cmp_fields(word);
    std::array::from_fn(|i| f64::from(f[i]) * f64::from(1 << (16 - CMP_BITS[i])) / 32767.0)
}

/// This crate's reading (`edge::x11y11z10n`): s / (2^(b-1) - 1).
fn crate_decode(word: u32) -> [f64; 3] {
    guac_formats::edge::x11y11z10n(word).map(f64::from)
}

// ---- the disc side ----

fn leak(bytes: Vec<u8>) -> &'static [u8] {
    Vec::leak(bytes)
}

fn level_bld(path: &Path) -> &'static [u8] {
    let bytes = std::fs::read(path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    let archive = Archive::parse(&bytes).unwrap();
    let entry = archive
        .entries()
        .iter()
        .find(|e| e.name().ends_with("level.bld"))
        .unwrap();
    leak(archive.read(entry).unwrap().into_owned())
}

struct Seg {
    object: usize,
    vertex_count: usize,
    triangles: Vec<[u16; 3]>,
    bind: Vec<[f64; 3]>,
    /// Per vertex: (Havok joint, weight / 255) for each nonzero weight.
    influences: Vec<Vec<(usize, f64)>>,
    /// The stored bind CMP words, from SPU input stream 1.
    normal_words: Vec<u32>,
    tangent_words: Vec<u32>,
    rsx_only: &'static [u8],
    rsx_stride: usize,
    uv0: AttributeBlock,
    /// The material's texture units and the `igImage2` each binds, by name.
    units: BTreeMap<u32, String>,
}

struct HeroData {
    segments: Vec<Seg>,
    joints: Vec<&'static str>,
    skeleton: havok::Skeleton<'static>,
    clip: havok::Clip<'static>,
    /// Per Havok joint: the Alchemy inverse joint, column-vector.
    alchemy_inverse: Vec<M4>,
    /// Per Havok joint: the inverse of its reference model pose.
    havok_inverse: Vec<M4>,
    /// Of the bind vertices, which is also the actor's `igAABox` (AC.7).
    diagonal: f64,
    /// Of the bind-pose joint origins (AC.8's extent).
    extent: f64,
}

impl HeroData {
    fn duration(&self) -> f64 {
        f64::from(self.clip.animation.duration)
    }

    /// The clip's model pose at `t`, looped.
    fn pose(&self, t: f64) -> Vec<M4> {
        let local = self
            .clip
            .local_pose(&self.skeleton, t.rem_euclid(self.duration()))
            .unwrap();
        let model = self.skeleton.model_pose(&local).unwrap();
        model.iter().map(havok::QsTransform::matrix).collect()
    }

    /// The skin palette at `t` under an inverse bind: pose x inverse bind, per joint.
    fn palette(&self, t: f64, inverse: &[M4]) -> Vec<M4> {
        self.pose(t)
            .iter()
            .zip(inverse)
            .map(|(w, i)| mul(w, i))
            .collect()
    }
}

fn eruptor(usrdir: &Path) -> HeroData {
    let level = level_bld(&usrdir.join("character/009_eruptor.bld"));
    let igz: &'static Igz<'static> = Box::leak(Box::new(Igz::parse(level).unwrap()));
    let (model, _) = layouts::read_all::<PhysicsModel>(igz)
        .find(|(_, m)| {
            m.as_ref()
                .is_ok_and(|m| m.model.info.name.is_some_and(|n| n.ends_with(MODEL)))
        })
        .expect("Eruptor.mdl");
    let hero = Hero::read(igz, model).unwrap();
    let skeleton = match hero.body.body.data_info.payload() {
        Some(Payload::Havok(b)) => havok::Container::parse(b)
            .unwrap()
            .skeletons
            .into_iter()
            .next()
            .unwrap(),
        _ => panic!("no Havok skeleton in the body"),
    };
    let idle = hero
        .clips
        .values()
        .filter_map(|c| c.animation.map(|(_, a)| a))
        .find(|a| {
            a.info
                .data_info
                .info
                .name
                .is_some_and(|n| n.ends_with(IDLE))
        })
        .and_then(|a| match a.info.data_info.payload() {
            Some(Payload::Havok(b)) => Some(b),
            _ => None,
        })
        .expect("Eruptor_Idle.ma");
    let clip = havok::Container::parse(idle).unwrap().clip.unwrap();

    // Havok joints pair with Alchemy bones by name (AC.8: no bone mapping table, orders differ).
    let mut joint_of_bone = vec![None; hero.bones.len()];
    let mut joints = Vec::new();
    let mut alchemy_inverse = Vec::new();
    for (j, b) in skeleton.bones.iter().enumerate() {
        let name = b.name.unwrap();
        let mut found = hero
            .bones
            .iter()
            .enumerate()
            .filter(|(_, a)| a.name == Some(name));
        let (i, bone) = found.next().unwrap();
        assert!(found.next().is_none(), "two Alchemy bones named {name}");
        joint_of_bone[i] = Some(j);
        joints.push(name);
        let m = hero.skeleton.inverse_joints[usize::try_from(bone.blend_matrix_index).unwrap()];
        // Stored row-vector (translation in row 3).
        alchemy_inverse.push(transpose(&m.map(|r| r.map(f64::from))));
    }
    let bind_pose = skeleton.bind_pose();
    let havok_inverse = bind_pose
        .iter()
        .map(|q| inverse_affine(&q.matrix()))
        .collect();
    let extent = diagonal(bind_pose.iter().map(|q| &q.translation));

    let mut segments = Vec::new();
    for mesh in &hero.meshes {
        let palette = mesh
            .palette
            .and_then(|p| hero.palettes.get(&p)?.indices)
            .and_then(|i| hero.int_lists.get(&i))
            .unwrap();
        let bone_map = BoneMap::new(&palette.values, &hero.bones);
        let units = (0..16i16)
            .filter_map(|u| {
                let (_, attr) = hero.texture_at(mesh, u)?;
                let name = Image2::read(igz, attr.image?).unwrap().name?;
                Some((u as u32, name.to_owned()))
            })
            .collect::<BTreeMap<_, _>>();
        for (object, layout) in hero.mesh_segments(mesh) {
            let s = Segment::decode(igz, object, layout).unwrap();
            let n = s.vertex_count();
            let skin = s.skin.as_ref().unwrap();
            let influences = skin
                .bones(&bone_map)
                .unwrap()
                .iter()
                .map(|bw| {
                    bw.iter()
                        .filter(|b| b.weight != 0)
                        .map(|b| (joint_of_bone[b.bone].unwrap(), f64::from(b.weight) / 255.0))
                        .collect()
                })
                .collect();
            let desc1 = StreamDesc::parse(layout.spu_input_stream_descs1.unwrap().data).unwrap();
            let spu1 = layout.spu_vertexes1.unwrap().data;
            let words = |sem: Semantic| -> Vec<u32> {
                let b = desc1.blocks.iter().find(|b| b.semantic == sem).unwrap();
                assert_eq!(b.format, VertexFormat::X11Y11Z10N);
                (0..n)
                    .map(|v| {
                        let at = v * usize::from(desc1.stride) + usize::from(b.offset);
                        u32::from_be_bytes(spu1[at..at + 4].try_into().unwrap())
                    })
                    .collect()
            };
            let rsx_desc = StreamDesc::parse(layout.rsx_only_stream_desc.unwrap().data).unwrap();
            let uv0 = *rsx_desc
                .blocks
                .iter()
                .find(|b| b.semantic == Semantic::Uv0)
                .unwrap();
            segments.push(Seg {
                object,
                vertex_count: n,
                triangles: s.triangles().collect(),
                bind: s.positions().map(|p| p.map(f64::from)).collect(),
                influences,
                normal_words: words(Semantic::Normal),
                tangent_words: words(Semantic::Tangent),
                rsx_only: layout.rsx_only_vertexes.unwrap().data,
                rsx_stride: usize::from(rsx_desc.stride),
                uv0,
                units: units.clone(),
            });
        }
    }
    let diagonal = diagonal(segments.iter().flat_map(|s| &s.bind));
    HeroData {
        segments,
        joints,
        skeleton,
        clip,
        alchemy_inverse,
        havok_inverse,
        diagonal,
        extent,
    }
}

// ---- the capture side ----

/// One texture surface as RSX saw it, shared across draws and captures by content.
struct Surface {
    offset: u32,
    /// The format byte, `LN`/`UN` included.
    format_raw: u32,
    width: u32,
    height: u32,
    mipmaps: u32,
    pitch: u32,
    bytes: Vec<u8>,
}

impl Surface {
    fn linear(&self) -> bool {
        self.format_raw & 0x20 != 0
    }

    fn base(&self) -> u32 {
        self.format_raw & !0x60
    }

    fn zero(&self) -> bool {
        self.bytes.iter().all(|&b| b == 0)
    }

    /// The surface's levels after this crate's untile, read by the unit's own description:
    /// RSX's format and `LN` bit pick the metaimage (`LN` clear = `_tile`). `None` for a format
    /// the crate doesn't convert, or bytes that don't fit.
    fn levels(&self) -> Option<Texture> {
        let format = match self.base() {
            0x86 => Format::Bc1RgbaUnorm,
            0x88 => Format::Bc3Unorm,
            0x81 => Format::R8Unorm,
            _ => return None,
        };
        let meta = MetaImage {
            name: "rsx",
            format,
            tiled: !self.linear(),
        };
        // The crate reads linear images as packed rows (one row of blocks is a 1-texel-high
        // level), so a wider pitch can't untile.
        let packed = format.level_size(self.width, 1);
        if self.linear() && u64::from(self.pitch) != packed {
            return None;
        }
        let chain = format.chain_size(self.width, self.height, self.mipmaps) as usize;
        if self.bytes.len() < chain {
            return None;
        }
        let mut data = self.bytes[..self.bytes.len().min(chain.next_multiple_of(128))].to_vec();
        data.resize(chain.next_multiple_of(128), 0);
        let (w, h, n) = (self.width, self.height, self.mipmaps);
        meta.untile(w as u16, h as u16, n as u16, &data).ok()
    }
}

struct Unit {
    unit: u32,
    surface: usize,
    captured: bool,
    border_texels: bool,
    remap: u32,
}

struct HeroDraw {
    draw: usize,
    segment: usize,
    mask: u32,
    primitive: u32,
    index_type: &'static str,
    base_index: u32,
    restart: bool,
    /// The draw's indexes in range order, triangles once grouped by three.
    indexes: Vec<u32>,
    /// Indexes per `DRAW_INDEX_ARRAY` range.
    ranges: Vec<u32>,
    window: (u32, u32),
    /// Where the draw reads UV0 from, less the attribute's offset in the stream.
    uv_base: u32,
    missing_bytes: usize,
    stride: u32,
    positions: Vec<(usize, [u32; 3])>,
    normals: Vec<(usize, u32)>,
    tangents: Vec<(usize, u32)>,
    units: Vec<Unit>,
}

struct CaptureData {
    name: String,
    draws: usize,
    hero: Vec<HeroDraw>,
    /// Distinct surfaces bound by any draw, as indexes into [`Session::surfaces`].
    bound: BTreeSet<usize>,
    unit_states: usize,
    uncaptured: usize,
}

fn array(d: &Draw, slot: u32) -> Option<&VertexArray> {
    d.vertex_arrays.iter().find(|a| a.slot == slot)
}

/// Vertex `v`'s element of an array, when the draw's window holds it.
fn element(a: &VertexArray, v: u32) -> Option<&[u8]> {
    let i = v
        .checked_sub(a.first_vertex)
        .filter(|&i| i < a.vertex_count)?;
    let at = (i * a.stride) as usize;
    a.bytes.get(at..at + a.element_size as usize)
}

/// Which of the hero's segments `d` draws, by its UV0 array: the stream RSX reads in place,
/// byte for byte the segment's RSX-only data over the draw's whole window.
fn hero_segment(d: &Draw, hero: &HeroData) -> Option<usize> {
    if d.kind != DrawKind::Indexed {
        return None;
    }
    let mut found = hero.segments.iter().enumerate().filter(|(_, s)| {
        let Some(a) = array(d, u32::from(s.uv0.slot)) else {
            return false;
        };
        let size = usize::from(s.uv0.size);
        a.type_raw == RSX_F16
            && a.stride as usize == s.rsx_stride
            && a.element_size as usize == size
            && a.vertex_count > 0
            && (a.first_vertex + a.vertex_count) as usize <= s.vertex_count
            && (a.first_vertex..a.first_vertex + a.vertex_count).all(|v| {
                let at = v as usize * s.rsx_stride + usize::from(s.uv0.offset);
                element(a, v) == Some(&s.rsx_only[at..at + size])
            })
    });
    let (i, _) = found.next()?;
    assert!(
        found.next().is_none(),
        "draw {} matches two segments",
        d.draw
    );
    Some(i)
}

fn surface_key(t: &rrc_dump::replay::TextureUnit) -> (u32, u32, u32, u32, u32, u32, u32, u64) {
    let mut h = DefaultHasher::new();
    t.bytes.hash(&mut h);
    (
        t.location,
        t.offset,
        t.format_raw,
        t.width,
        t.height,
        t.mipmaps,
        t.pitch,
        h.finish(),
    )
}

struct Session {
    usrdir: PathBuf,
    hero: HeroData,
    captures: Vec<CaptureData>,
    surfaces: Vec<Surface>,
    /// Per capture, the clip solved from the posed positions with the Alchemy bind.
    solves: Vec<Solve>,
}

fn capture_paths() -> Vec<PathBuf> {
    let dir = env_or_dotenv("GUAC_CAPTURE_DIR")
        .expect("set GUAC_CAPTURE_DIR (env or .env.dev) to the folder holding AC.13's captures");
    let mut paths: Vec<PathBuf> = std::fs::read_dir(&dir)
        .unwrap_or_else(|e| panic!("{dir}: {e}"))
        .map(|e| e.unwrap().path())
        .filter(|p| {
            p.file_name()
                .is_some_and(|n| n.to_string_lossy().ends_with("_capture.rrc.gz"))
        })
        .collect();
    paths.sort();
    assert!(!paths.is_empty(), "no *_capture.rrc.gz in {dir}");
    paths
}

fn session() -> &'static Session {
    static SESSION: OnceLock<Session> = OnceLock::new();
    SESSION.get_or_init(|| {
        let started = Instant::now();
        let paths = capture_paths();
        let usrdir = usrdir();
        let hero = eruptor(&usrdir);
        println!(
            "Eruptor: {} segments {:?}, diagonal {:.4}, skeleton extent {:.4} ({:.2} s)",
            hero.segments.len(),
            hero.segments
                .iter()
                .map(|s| (s.object, s.vertex_count, s.triangles.len()))
                .collect::<Vec<_>>(),
            hero.diagonal,
            hero.extent,
            started.elapsed().as_secs_f64()
        );
        let mut surfaces: Vec<Surface> = Vec::new();
        let mut by_key = HashMap::new();
        let mut captures = Vec::new();
        for path in &paths {
            let t0 = Instant::now();
            let capture = Capture::from_file_bytes(&std::fs::read(path).unwrap()).unwrap();
            let mut data = CaptureData {
                name: path.file_name().unwrap().to_string_lossy().into_owned(),
                draws: 0,
                hero: Vec::new(),
                bound: BTreeSet::new(),
                unit_states: 0,
                uncaptured: 0,
            };
            let stats = rrc_dump::replay(&capture, |d| -> Result<(), ()> {
                let mut units = Vec::new();
                for t in &d.textures {
                    data.unit_states += 1;
                    data.uncaptured += usize::from(!t.captured);
                    let surface = *by_key.entry(surface_key(t)).or_insert_with(|| {
                        surfaces.push(Surface {
                            offset: t.offset,
                            format_raw: t.format_raw,
                            width: t.width,
                            height: t.height,
                            mipmaps: t.mipmaps,
                            pitch: t.pitch,
                            bytes: t.bytes.clone(),
                        });
                        surfaces.len() - 1
                    });
                    data.bound.insert(surface);
                    units.push(Unit {
                        unit: t.unit,
                        surface,
                        captured: t.captured,
                        border_texels: t.border_texels,
                        remap: t.remap,
                    });
                }
                if let Some(seg) = hero_segment(&d, &hero) {
                    data.hero.push(hero_draw(&d, seg, &hero, units));
                }
                Ok(())
            })
            .unwrap();
            data.draws = stats.draws;
            println!(
                "{}: {} commands, {} draws, {} hero draws, {} blobs ({} unreferenced), {} unresolved, {} orphan ({:.2} s)",
                data.name,
                stats.commands,
                stats.draws,
                data.hero.len(),
                capture.data.len(),
                capture.unreferenced_blobs(),
                stats.unresolved_blocks,
                stats.orphan_commands,
                t0.elapsed().as_secs_f64()
            );
            captures.push(data);
        }
        let t0 = Instant::now();
        let solves = captures
            .iter()
            .map(|c| solve(&hero, &posed(&hero, c).0, &hero.alchemy_inverse))
            .collect();
        println!(
            "session ready in {:.2} s (solves {:.2} s); load average {}",
            started.elapsed().as_secs_f64(),
            t0.elapsed().as_secs_f64(),
            load_average()
        );
        Session {
            usrdir,
            hero,
            captures,
            surfaces,
            solves,
        }
    })
}

fn load_average() -> String {
    std::process::Command::new("sysctl")
        .args(["-n", "vm.loadavg"])
        .output()
        .ok()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_owned())
        .unwrap_or_else(|| "unknown".into())
}

fn hero_draw(d: &Draw, segment: usize, hero: &HeroData, units: Vec<Unit>) -> HeroDraw {
    let s = &hero.segments[segment];
    let ia = d.index_array.as_ref().unwrap();
    let mut indexes = Vec::new();
    for r in &d.ranges {
        for slot in r.first..r.first + r.count {
            let at = ((slot - ia.first) * ia.type_size) as usize;
            let raw = match ia.type_size {
                2 => u32::from(u16::from_be_bytes([ia.bytes[at], ia.bytes[at + 1]])),
                _ => u32::from_be_bytes(ia.bytes[at..at + 4].try_into().unwrap()),
            };
            indexes.push(raw.wrapping_add(ia.base_index) & 0x000f_ffff);
        }
    }
    let uv = array(d, u32::from(s.uv0.slot)).unwrap();
    let window = (ia.min_vertex.unwrap(), ia.max_vertex.unwrap());
    let mut out = HeroDraw {
        draw: d.draw,
        segment,
        mask: d.vertex_attrib_input_mask,
        primitive: d.primitive,
        index_type: ia.type_name,
        base_index: ia.base_index,
        restart: ia.restart_index.is_some(),
        indexes,
        ranges: d.ranges.iter().map(|r| r.count).collect(),
        window,
        uv_base: uv.rsx_offset - u32::from(s.uv0.offset),
        missing_bytes: ia.missing_bytes
            + d.vertex_arrays
                .iter()
                .map(|a| a.missing_bytes)
                .sum::<usize>(),
        stride: 0,
        positions: Vec::new(),
        normals: Vec::new(),
        tangents: Vec::new(),
        units,
    };
    let word = |b: &[u8], at: usize| u32::from_be_bytes(b[at..at + 4].try_into().unwrap());
    if let Some(a) = array(d, 0) {
        assert_eq!((a.type_raw, a.components), (RSX_F32, 3), "draw {}", d.draw);
        out.stride = a.stride;
        for v in window.0..=window.1 {
            if let Some(e) = element(a, v) {
                out.positions
                    .push((v as usize, [0, 4, 8].map(|k| word(e, k))));
            }
        }
    }
    for (slot, dst) in [(2, &mut out.normals), (14, &mut out.tangents)] {
        if let Some(a) = array(d, slot).filter(|a| a.type_raw == RSX_CMP) {
            for v in window.0..=window.1 {
                if let Some(e) = element(a, v) {
                    dst.push((v as usize, word(e, 0)));
                }
            }
        }
    }
    out
}

/// A posed vertex: where the capture has it, and what skins it.
struct Posed<'h> {
    bind: [f64; 3],
    influences: &'h [(usize, f64)],
    captured: [f64; 3],
}

/// Every vertex a capture's hero draws hold, once, plus how many were seen in more than one
/// pass and how many of those agree bit for bit.
fn posed<'h>(hero: &'h HeroData, c: &CaptureData) -> (Vec<Posed<'h>>, usize, usize) {
    let mut seen: BTreeMap<(usize, usize), Vec<[u32; 3]>> = BTreeMap::new();
    for d in &c.hero {
        for &(v, p) in &d.positions {
            seen.entry((d.segment, v)).or_default().push(p);
        }
    }
    let shared = seen.values().filter(|p| p.len() > 1).count();
    let agree = seen
        .values()
        .filter(|p| p.len() > 1 && p.iter().all(|x| *x == p[0]))
        .count();
    let posed = seen
        .iter()
        .map(|(&(seg, v), p)| {
            let s = &hero.segments[seg];
            Posed {
                bind: s.bind[v],
                influences: &s.influences[v],
                captured: p[0].map(|b| f64::from(f32::from_bits(b))),
            }
        })
        .collect();
    (posed, shared, agree)
}

fn skin(palette: &[M4], p: &Posed<'_>) -> [f64; 3] {
    let mut out = [0.0; 3];
    for &(j, w) in p.influences {
        let q = apply(&palette[j], p.bind);
        for k in 0..3 {
            out[k] += w * q[k];
        }
    }
    out
}

fn max_error(palette: &[M4], posed: &[Posed<'_>]) -> f64 {
    posed
        .iter()
        .map(|p| dist(skin(palette, p), p.captured))
        .fold(0.0, f64::max)
}

#[derive(Debug, Clone, Copy)]
struct Solve {
    t: f64,
    /// Worst vertex at `t`, absolute.
    error: f64,
    /// The best worst-vertex error more than 0.1 s (looped) from `t`.
    far: f64,
}

/// AC.17's clip solve: the clip time whose skinned bind vertices best match the posed ones,
/// worst vertex as the metric, over the loop on a grid and then refined around the best.
fn solve(hero: &HeroData, posed: &[Posed<'_>], inverse: &[M4]) -> Solve {
    let d = hero.duration();
    let err = |t: f64| max_error(&hero.palette(t, inverse), posed);
    let n = 2000;
    let grid: Vec<(f64, f64)> = (0..n)
        .map(|i| {
            let t = d * f64::from(i) / f64::from(n);
            (t, err(t))
        })
        .collect();
    let (mut t, mut error) = grid
        .iter()
        .copied()
        .min_by(|a, b| a.1.total_cmp(&b.1))
        .unwrap();
    let mut step = d / f64::from(n);
    for _ in 0..8 {
        let k = 40;
        let lo = t - 2.0 * step;
        for i in 0..=k {
            let x = lo + 4.0 * step * f64::from(i) / f64::from(k);
            let e = err(x);
            if e < error {
                (t, error) = (x, e);
            }
        }
        step = 4.0 * step / f64::from(k);
    }
    let t = t.rem_euclid(d);
    let far = grid
        .iter()
        .filter(|(x, _)| {
            let dt = (x - t).abs();
            dt.min(d - dt) > 0.1
        })
        .map(|x| x.1)
        .fold(f64::MAX, f64::min);
    Solve { t, error, far }
}

/// The game's palette, fitted by least squares from the posed vertices alone: per joint the
/// 3x4 that, weighted and summed over each vertex's influences, best maps bind to captured.
/// Returns the palette (`None` for a joint no vertex weights) and the worst vertex residual.
fn fit_palette(joints: usize, posed: &[Posed<'_>]) -> (Vec<Option<M4>>, f64) {
    let n = 4 * joints;
    let mut a = vec![vec![0.0; n]; n];
    let mut b = vec![vec![0.0; n]; 3];
    for p in posed {
        let h = [p.bind[0], p.bind[1], p.bind[2], 1.0];
        let row: Vec<(usize, f64)> = p
            .influences
            .iter()
            .flat_map(|&(j, w)| (0..4).map(move |k| (4 * j + k, w * h[k])))
            .collect();
        for &(i, vi) in &row {
            for &(k, vk) in &row {
                a[i][k] += vi * vk;
            }
            for (rhs, x) in b.iter_mut().zip(p.captured) {
                rhs[i] += vi * x;
            }
        }
    }
    let active: Vec<usize> = (0..joints).filter(|&j| a[4 * j][4 * j] > 0.0).collect();
    let idx: Vec<usize> = active
        .iter()
        .flat_map(|&j| (0..4).map(move |k| 4 * j + k))
        .collect();
    let sub_a = idx
        .iter()
        .map(|&i| idx.iter().map(|&k| a[i][k]).collect())
        .collect();
    let sub_b = (0..3)
        .map(|c| idx.iter().map(|&i| b[c][i]).collect())
        .collect();
    let x = solve_linear(sub_a, sub_b);
    let mut palette = vec![None; joints];
    for (ai, &j) in active.iter().enumerate() {
        let mut m = [
            [0.0, 0.0, 0.0, 0.0],
            [0.0; 4],
            [0.0; 4],
            [0.0, 0.0, 0.0, 1.0],
        ];
        for (c, row) in m.iter_mut().enumerate().take(3) {
            for (k, v) in row.iter_mut().enumerate() {
                *v = x[c][4 * ai + k];
            }
        }
        palette[j] = Some(m);
    }
    let full: Vec<M4> = palette.iter().map(|m| m.unwrap_or([[0.0; 4]; 4])).collect();
    (palette, max_error(&full, posed))
}

/// The ordered-subset check (chotchki, 2026-10-18; SPU culling is on): every captured triangle
/// is one of ours, same vertex rotation, in our order. Returns (in order, outside our decode,
/// ours but rotated, ours but out of order).
fn ordered_subset(captured: &[[u32; 3]], decoded: &[[u16; 3]]) -> (bool, usize, usize, usize) {
    let ours: Vec<[u32; 3]> = decoded.iter().map(|t| t.map(u32::from)).collect();
    let set: BTreeSet<[u32; 3]> = ours.iter().copied().collect();
    let rotations: BTreeSet<[u32; 3]> = ours
        .iter()
        .flat_map(|t| [[t[1], t[2], t[0]], [t[2], t[0], t[1]]])
        .collect();
    let (mut next, mut ok) = (0, true);
    let (mut outside, mut rotated, mut out_of_order) = (0, 0, 0);
    for t in captured {
        match (next..ours.len()).find(|&q| ours[q] == *t) {
            Some(q) => next = q + 1,
            None => {
                ok = false;
                if set.contains(t) {
                    out_of_order += 1;
                } else if rotations.contains(t) {
                    rotated += 1;
                } else {
                    outside += 1;
                }
            }
        }
    }
    (ok, outside, rotated, out_of_order)
}

/// Which pass a hero draw is, by its vertex input mask (AC.12's table).
fn pass(mask: u32) -> char {
    match mask {
        0x101 => 'A',
        0x4105 => 'B',
        0x18B => 'C',
        0x105 => 'D',
        0x38F => 'E',
        _ => '?',
    }
}

// ---- AC.15 ----

struct DiscImage {
    name: String,
    tiled: bool,
    texture: Texture,
}

/// Every `igImage2` in `archives` (paths under USRDIR) the crate converts, and how many it
/// refused (formats the character files don't use).
fn disc_images(usrdir: &Path, archives: &[&str]) -> (Vec<DiscImage>, usize) {
    let (mut out, mut refused) = (Vec::new(), 0);
    for file in archives {
        let bytes = std::fs::read(usrdir.join(file)).unwrap();
        let archive = Archive::parse(&bytes).unwrap();
        for entry in archive.entries() {
            let data = archive.read(entry).unwrap();
            if !data.starts_with(b"IGZ\x01") {
                continue;
            }
            let Ok(igz) = Igz::parse(&data) else {
                refused += 1;
                continue;
            };
            for (i, image) in layouts::read_all::<Image2>(&igz) {
                let Ok(image) = image else {
                    refused += 1;
                    continue;
                };
                let tiled = image
                    .format_name()
                    .and_then(MetaImage::by_name)
                    .map(|m| m.tiled);
                match (Texture::read(&igz, i), tiled) {
                    (Ok(texture), Some(tiled)) => out.push(DiscImage {
                        name: image.name.unwrap_or("?").to_owned(),
                        tiled,
                        texture,
                    }),
                    _ => refused += 1,
                }
            }
        }
    }
    (out, refused)
}

fn ktx2_levels(t: &Texture) -> Vec<Vec<u8>> {
    let file = t.to_ktx2();
    let r = ktx2::Reader::new(&file[..]).unwrap();
    assert_eq!(r.header().level_count as usize, t.levels.len());
    r.levels().map(|l| l.data.to_vec()).collect()
}

#[test]
#[ignore = "reads the Giants dump at GUAC_GIANTS_DUMP and AC.13's captures at GUAC_CAPTURE_DIR"]
fn eruptor_textures_match_the_captures() {
    let s = session();
    let t0 = Instant::now();
    let (eruptor, refused) = disc_images(
        &s.usrdir,
        &["character/009_eruptor.bld", "character/009_eruptor.arc"],
    );
    assert_eq!((eruptor.len(), refused), (60, 0));
    // What else the frames bind: the level drawn before him leaves its units enabled (AC.12).
    let (others, others_refused) = disc_images(
        &s.usrdir,
        &[
            "level/level_213_tutotial.bld",
            "level/level_213_tutotial.arc",
            "permanent/global.bld",
        ],
    );
    println!(
        "disc: Eruptor's {} images, {} more from the tutorial and global.bld ({} refused) in {:.2} s, load average {}",
        eruptor.len(),
        others.len(),
        others_refused,
        t0.elapsed().as_secs_f64(),
        load_average()
    );
    let ktx: Vec<Vec<Vec<u8>>> = eruptor.iter().map(|i| ktx2_levels(&i.texture)).collect();

    // Each bound surface after the same untile, against every disc image of its shape.
    let all: Vec<&DiscImage> = eruptor.iter().chain(&others).collect();
    let mut matches: HashMap<usize, Vec<usize>> = HashMap::new();
    let mut levels: HashMap<usize, Texture> = HashMap::new();
    let bound: BTreeSet<usize> = s
        .captures
        .iter()
        .flat_map(|c| c.bound.iter().copied())
        .collect();
    for &i in &bound {
        let Some(t) = s.surfaces[i].levels() else {
            continue;
        };
        let m: Vec<usize> = all
            .iter()
            .enumerate()
            .filter(|(_, d)| d.texture == t)
            .map(|(k, _)| k)
            .collect();
        matches.insert(i, m);
        levels.insert(i, t);
    }

    // Every unit of every hero draw. Enabled isn't sampled (FORMAT.md): a unit holds his
    // material's image only on the passes that bind it, and what the draw before left elsewhere.
    let mut by_image: BTreeMap<&str, (u32, bool, BTreeSet<u32>, usize)> = BTreeMap::new();
    let mut his_units: BTreeMap<char, BTreeSet<u32>> = BTreeMap::new();
    let mut leftovers: BTreeSet<&str> = BTreeSet::new();
    let mut targets: BTreeSet<(u32, u32, u32, u32, u32)> = BTreeSet::new();
    let mut hero_units = 0;
    for c in &s.captures {
        for d in &c.hero {
            let seg = &s.hero.segments[d.segment];
            for u in &d.units {
                hero_units += 1;
                let at = format!("{} draw {} u{}", c.name, d.draw, u.unit);
                let surface = &s.surfaces[u.surface];
                assert!(u.captured && !u.border_texels, "{at}");
                let found = matches.get(&u.surface).map_or(&[][..], Vec::as_slice);
                let his = found.iter().find(|&&k| k < eruptor.len());
                if let Some(&k) = his {
                    // One of his: exactly what his material binds on this unit, equal level for
                    // level to the KTX2 we write after the same untile.
                    let name = eruptor[k].name.as_str();
                    assert_eq!(
                        seg.units.get(&u.unit).map(String::as_str),
                        Some(name),
                        "{at}"
                    );
                    assert_eq!(
                        levels[&u.surface].levels, ktx[k],
                        "{at}: KTX2 levels of {name}"
                    );
                    assert_eq!(
                        surface.linear(),
                        !eruptor[k].tiled,
                        "{at}: LN against _tile"
                    );
                    assert_eq!(u.remap, 0xaae4, "{at}");
                    let e = by_image.entry(name).or_insert((
                        u.unit,
                        surface.linear(),
                        BTreeSet::new(),
                        0,
                    ));
                    e.2.insert(surface.mipmaps);
                    e.3 += 1;
                    his_units.entry(pass(d.mask)).or_default().insert(u.unit);
                } else if let Some(&k) = found.first() {
                    leftovers.insert(all[k].name.as_str());
                } else {
                    assert!(
                        surface.zero(),
                        "{at}: a nonzero surface matching no disc image"
                    );
                    targets.insert((
                        u.unit,
                        surface.offset,
                        surface.base(),
                        surface.width,
                        surface.pitch,
                    ));
                }
            }
        }
    }
    let draws: usize = s.captures.iter().map(|c| c.hero.len()).sum();
    println!(
        "hero units: {hero_units} unit states over {draws} draws, every one captured, none with border texels; his own images by pass {his_units:?}"
    );
    for (name, (unit, linear, mips, n)) in &by_image {
        let e = eruptor.iter().find(|e| e.name == *name).unwrap();
        println!(
            "  u{unit} {name}: {:?} {}x{}, {} levels equal to the KTX2 after the untile, LN {linear} (_tile {}), {n} unit states, RSX mips {mips:?}",
            e.texture.format,
            e.texture.width,
            e.texture.height,
            e.texture.levels.len(),
            e.tiled
        );
    }
    println!("  left bound by earlier draws, each equal to a disc image: {leftovers:?}");
    for (unit, offset, base, width, pitch) in &targets {
        println!(
            "  render target u{unit} at {offset:#010x}: format {base:#x}, width {width}, pitch {pitch}, every byte zero"
        );
    }
    let material: BTreeSet<&str> = s
        .hero
        .segments
        .iter()
        .flat_map(|g| g.units.values().map(String::as_str))
        .collect();
    assert_eq!(
        by_image.keys().copied().collect::<BTreeSet<_>>(),
        material,
        "every image his material binds is bound and checked"
    );
    // The main lit pass binds his whole material; every pass binds his colour map.
    assert_eq!(his_units[&'B'], [0, 3, 4, 6].into());
    assert!(his_units.values().all(|u| u.contains(&0)) && his_units.len() == 5);
    assert!(
        targets
            .iter()
            .all(|t| t.2 == 0x85 && t.3 == 960 && t.4 == 4096)
    );

    // The whole frame: every distinct surface any draw binds.
    for c in &s.captures {
        let (mut matched, mut zero, mut other) = (0, 0, Vec::new());
        let mut swizzle: BTreeMap<(bool, bool), usize> = BTreeMap::new();
        let mut bigs = BTreeSet::new();
        for &i in &c.bound {
            let surface = &s.surfaces[i];
            match matches.get(&i).filter(|m| !m.is_empty()) {
                Some(m) => {
                    matched += 1;
                    *swizzle
                        .entry((surface.linear(), all[m[0]].tiled))
                        .or_default() += 1;
                    if surface.linear() {
                        // Matching proves the crate's packed-row reading of a linear pitch.
                        bigs.insert(format!(
                            "{} {}x{} pitch {}",
                            all[m[0]].name, surface.width, surface.height, surface.pitch
                        ));
                    }
                }
                None if surface.zero() => zero += 1,
                None => other.push(format!(
                    "{:#x} format {:#x} {}x{} mips {} pitch {}",
                    surface.offset,
                    surface.format_raw,
                    surface.width,
                    surface.height,
                    surface.mipmaps,
                    surface.pitch
                )),
            }
        }
        println!(
            "{}: {} draws bind {} distinct surfaces over {} unit states ({} not captured): {matched} equal a disc image level for level, {zero} all zero, {} other; (LN, _tile) {swizzle:?}",
            c.name,
            c.draws,
            c.bound.len(),
            c.unit_states,
            c.uncaptured,
            other.len()
        );
        println!("    linear (_big): {bigs:?}");
        for o in &other {
            println!("    unmatched {o}");
        }
        assert_eq!(c.uncaptured, 0);
        assert!(other.is_empty());
        assert!(
            swizzle.keys().all(|(linear, tiled)| linear != tiled),
            "LN set exactly on _big"
        );
    }

    // Coverage: which of his 60 any draw binds.
    let covered: BTreeSet<&str> = matches
        .values()
        .flatten()
        .filter(|&&k| k < eruptor.len())
        .map(|&k| eruptor[k].name.as_str())
        .collect();
    println!(
        "Eruptor's images bound anywhere in the three frames: {} of {}: {covered:?}; the other {} aren't drawn in these frames, so they aren't compared here, which isn't a failure (AC.15's note has the savestate's resident ones)",
        covered.len(),
        eruptor.len(),
        eruptor.len() - covered.len()
    );
    assert!(material.iter().all(|m| covered.contains(m)));
    println!("AC.15 done in {:.2} s", t0.elapsed().as_secs_f64());
}

// ---- AC.16 ----

#[test]
#[ignore = "reads the Giants dump at GUAC_GIANTS_DUMP and AC.13's captures at GUAC_CAPTURE_DIR"]
fn eruptor_segments_match_the_captures() {
    let s = session();
    let hero = &s.hero;
    let mut uv_bases: BTreeMap<usize, BTreeSet<u32>> = BTreeMap::new();
    let (mut words, mut exact) = (0usize, 0usize);
    let mut worst_lsb = 0;
    let mut worst_tie: f64 = 0.0;
    let mut angles = [0.0f64; 3];
    let mut crate_vs_spu: f64 = 0.0;
    for seg in &hero.segments {
        for &w in seg.normal_words.iter().chain(&seg.tangent_words) {
            crate_vs_spu = crate_vs_spu.max(angle(crate_decode(w), spu_decode(w)));
        }
    }
    for (c, solved) in s.captures.iter().zip(&s.solves) {
        // The draws: 5 passes, the 503-vertex segment skipped by pass C.
        let mut passes: BTreeMap<usize, String> = BTreeMap::new();
        let mut kept = Vec::new();
        for d in &c.hero {
            let seg = &hero.segments[d.segment];
            passes.entry(d.segment).or_default().push(pass(d.mask));
            assert_eq!(d.primitive, TRIANGLES);
            assert_eq!((d.index_type, d.base_index, d.restart), ("u16", 0, false));
            assert_eq!(d.missing_bytes, 0, "draw {}", d.draw);
            assert_eq!(d.indexes.len() % 3, 0);
            // One draw per segment, cut into batches of at most 256 indexes that triangles cross.
            assert!(d.ranges.iter().all(|&n| n <= 256), "draw {}", d.draw);
            assert!((d.window.1 as usize) < seg.vertex_count);
            let triangles = d.indexes.as_chunks::<3>().0;
            let (ok, outside, rotated, out_of_order) = ordered_subset(triangles, &seg.triangles);
            assert!(
                ok,
                "{} draw {}: {outside} outside our decode, {rotated} rotated, {out_of_order} out of order",
                c.name, d.draw
            );
            kept.push(triangles.len() as f64 / seg.triangles.len() as f64);
            uv_bases.entry(d.segment).or_default().insert(d.uv_base);
            let normals_expected = matches!(pass(d.mask), 'B' | 'D' | 'E');
            assert_eq!(d.stride, if normals_expected { 20 } else { 12 });
            assert_eq!(!d.normals.is_empty(), normals_expected);
            assert_eq!(!d.tangents.is_empty(), pass(d.mask) == 'B');
        }
        let passes: Vec<String> = passes.into_values().collect();
        assert_eq!(passes, ["ABCDE", "ABCDE", "ABDE"], "{}", c.name);
        let lo = kept.iter().copied().fold(1.0, f64::min);
        let hi = kept.iter().copied().fold(0.0, f64::max);

        // Positions: one skin per frame, ours at the solved time.
        let (vertices, shared, agree) = posed(hero, c);
        assert_eq!(shared, agree, "a vertex differs between passes");
        let palette = hero.palette(solved.t, &hero.alchemy_inverse);
        let error = max_error(&palette, &vertices);
        let moved = vertices
            .iter()
            .map(|p| dist(p.captured, p.bind))
            .fold(0.0, f64::max);
        assert!(error <= POSITIONS * hero.diagonal);

        // Normals and tangents: the SPU's own re-encode of our bind words, against its words.
        let mut seen: BTreeMap<(usize, usize, bool), Vec<u32>> = BTreeMap::new();
        for d in &c.hero {
            for (list, tangent) in [(&d.normals, false), (&d.tangents, true)] {
                for &(v, w) in list {
                    seen.entry((d.segment, v, tangent)).or_default().push(w);
                }
            }
        }
        let (mut c_words, mut c_exact) = (0, 0);
        for (&(seg, v, tangent), ws) in &seen {
            assert!(
                ws.iter().all(|w| *w == ws[0]),
                "a CMP word differs between passes"
            );
            let g = &hero.segments[seg];
            let bind = if tangent {
                g.tangent_words[v]
            } else {
                g.normal_words[v]
            };
            let blend = |decode: fn(u32) -> [f64; 3]| {
                let b = decode(bind);
                let mut n = [0.0; 3];
                for &(j, w) in &g.influences[v] {
                    let r = rotate(&palette[j], b);
                    for k in 0..3 {
                        n[k] += w * r[k];
                    }
                }
                n
            };
            let ours = blend(spu_decode);
            let predicted = spu_encode(ours);
            let got = ws[0];
            c_words += 1;
            if predicted == got {
                c_exact += 1;
            } else {
                let (p, q) = (cmp_fields(predicted), cmp_fields(got));
                for k in 0..3 {
                    let lsb = (p[k] - q[k]).abs();
                    worst_lsb = worst_lsb.max(lsb);
                    if lsb > 0 {
                        let u = spu_unrounded(ours[k], CMP_BITS[k]);
                        worst_tie = worst_tie.max((u - u.floor()).min(u.ceil() - u));
                    }
                }
            }
            let decodes: [fn(u32) -> [f64; 3]; 3] = [spu_decode, crate_decode, rpcs3_decode];
            for (a, decode) in angles.iter_mut().zip(decodes) {
                *a = a.max(angle(decode(got), blend(decode)));
            }
        }
        words += c_words;
        exact += c_exact;
        println!(
            "{}: {} hero draws, every one an ordered subset (kept {:.1}%..{:.1}% of our triangles); {} posed vertices ({shared} in more than one pass, all bit-identical), moved up to {moved:.3} from bind; at t {:.5} s our skin misses by {error:.3e} ({:.2e} of the diagonal, tolerance {POSITIONS:.0e}); CMP {c_exact} of {c_words} words exact",
            c.name,
            c.hero.len(),
            100.0 * lo,
            100.0 * hi,
            vertices.len(),
            solved.t,
            error / hero.diagonal
        );
    }
    println!(
        "UV0 read in place, byte-identical over every window, at one base per segment: {:?}",
        uv_bases
            .values()
            .map(|b| b.iter().map(|x| format!("{x:#x}")).collect::<Vec<_>>())
            .collect::<Vec<_>>()
    );
    assert!(uv_bases.values().all(|b| b.len() == 1));
    println!(
        "CMP under the SPU's rule (decode (2s+1)/(2^b-1), blend without renormalizing, encode floor((x(2^b-1)-1)/2)): {exact} of {words} words exact, every miss within {worst_lsb} LSB per component and {worst_tie:.1e} of a step from the rounding boundary"
    );
    println!(
        "  worst angle against our float blend: SPU decode {:.2e} rad, crate decode {:.2e}, RPCS3 decode {:.2e}; one z step is {:.2e} rad, so decision 5's 1e-3 rad can't hold by construction (chotchki's call); the crate and SPU decodes differ by up to {crate_vs_spu:.3e} rad on his bind words",
        angles[0],
        angles[1],
        angles[2],
        2.0 / 1023.0
    );
    // The measured bound, asserted: misses are single-LSB rounding ties.
    assert_eq!(worst_lsb, 1);
    assert!(worst_tie < 1e-3);
    assert!(exact * 1000 > words * 998, "{exact} of {words}");
}

// ---- AC.17 ----

#[test]
#[ignore = "reads the Giants dump at GUAC_GIANTS_DUMP and AC.13's captures at GUAC_CAPTURE_DIR"]
fn eruptor_idle_and_bind_match_the_captures() {
    let s = session();
    let hero = &s.hero;
    let d = hero.duration();
    assert!((d - 4.0 / 3.0).abs() < 1e-5);
    let subtree: BTreeSet<usize> = WRIST_SUBTREE
        .iter()
        .map(|n| hero.joints.iter().position(|j| j == n).unwrap())
        .collect();
    for (c, solved) in s.captures.iter().zip(&s.solves) {
        let (vertices, _, _) = posed(hero, c);
        // The clip: one sharp minimum, inside decision 5's 1e-3 of the diagonal.
        assert!(solved.error <= CLIP * hero.diagonal);
        assert!(solved.far > CLIP * hero.diagonal, "a second minimum");

        // The game's palette, fitted from his posed vertices alone.
        let (fit, residual) = fit_palette(hero.joints.len(), &vertices);
        assert!(
            fit.iter().all(Option::is_some),
            "a joint no captured vertex weights"
        );
        let fit: Vec<M4> = fit.into_iter().flatten().collect();
        assert!(residual < BIND_FIT);

        // Alchemy's inverse joints at the solved time: the palette, and the skeleton through it.
        let alchemy = hero.palette(solved.t, &hero.alchemy_inverse);
        let pose = hero.pose(solved.t);
        let (mut rot, mut trans, mut w_rot, mut w_trans) = (0.0f64, 0.0f64, 0.0f64, 0.0f64);
        for j in 0..fit.len() {
            rot = rot.max(rot_angle(&fit[j], &alchemy[j]));
            trans = trans.max(dist(translation(&fit[j]), translation(&alchemy[j])));
            let world = mul(&fit[j], &inverse_affine(&hero.alchemy_inverse[j]));
            w_rot = w_rot.max(rot_angle(&world, &pose[j]));
            w_trans = w_trans.max(dist(translation(&world), translation(&pose[j])));
        }
        assert!(rot < BIND_FIT && trans < BIND_FIT);
        assert!(w_rot < SKELETON && w_trans / hero.extent < SKELETON);

        // Havok's reference pose as the bind: the same palette misses J_wrist_R's subtree by the
        // 20.8 deg disagreement and only there, and no clip time absorbs it.
        let havok = hero.palette(solved.t, &hero.havok_inverse);
        let misses: BTreeMap<usize, f64> = (0..fit.len())
            .map(|j| (j, rot_angle(&fit[j], &havok[j])))
            .filter(|&(_, a)| a > 1e-3)
            .collect();
        let havok_solve = solve(hero, &vertices, &hero.havok_inverse);
        println!(
            "{}: Eruptor_Idle at t {:.5} s (of {d:.4}), worst vertex {:.3e} ({:.2e} of the diagonal), next best more than 0.1 s away {:.3e}; fitted palette residual {residual:.2e}, against Alchemy's bind {rot:.2e} rad / {trans:.2e}, skeleton through it {w_rot:.2e} rad / {:.2e} of the extent; Havok's bind misses {:?} by {:.4}..{:.4} rad and solves at best to {:.3} ({:.1}% of the diagonal) at t {:.5}",
            c.name,
            solved.t,
            solved.error,
            solved.error / hero.diagonal,
            solved.far,
            w_trans / hero.extent,
            misses.keys().map(|&j| hero.joints[j]).collect::<Vec<_>>(),
            misses.values().copied().fold(f64::MAX, f64::min),
            misses.values().copied().fold(0.0, f64::max),
            havok_solve.error,
            100.0 * havok_solve.error / hero.diagonal,
            havok_solve.t
        );
        assert_eq!(misses.keys().copied().collect::<BTreeSet<_>>(), subtree);
        assert!(misses.values().all(|a| (a - 0.3631).abs() < 5e-3));
        assert!(havok_solve.error > CLIP * hero.diagonal);
    }
    // Visibly different poses: the solved times sit apart on the loop.
    for (i, a) in s.solves.iter().enumerate() {
        for b in &s.solves[i + 1..] {
            let dt = (a.t - b.t).abs();
            assert!(dt.min(d - dt) > 0.1, "{} and {}", a.t, b.t);
        }
    }
}
