//! Giants' actors read at a pause through PINE (openspec AF.9, design decision
//! 8): where every game actor stands and faces, which ones are skinned, which
//! one a pad drives (the hero), and the camera, in one batched walk down from a
//! static pointer. `oracle actors` prints one; the live tests stamp each with
//! the flip `STATE` read and write it as JSON.
//!
//! Where the transforms come from. Of design decision 8's two leads the actor
//! objects held; the skinned Edge job records carry the hero's transform too
//! but make a poor reader. A job record names a geometry segment, not an actor;
//! its local-to-world block sits in SPU scratch that the next frames' jobs
//! reuse, so the ring mixes this frame's blocks with older and overwritten ones
//! (in AF.8's fresh state 15 of the hero's 80 records pointed at his
//! transform, 1 at an older frame's, 64 at blocks other jobs had since
//! written); and an actor that isn't drawn has no job at all. The game's own
//! actor list has none of that: every simulated actor, drawn or not, named,
//! with the transform the simulation wrote. Where both exist they agree: in
//! two states the hero's current blocks were his corpus's `_xform` times
//! `_scaleVec` to 1.8e-7 and held `_currentMatrix`'s position to 2.1e-5 m.
//!
//! The layout, Giants 01.00 (`BLUS30968`), offsets from each object's start,
//! big-endian; type and field names are the reflection registry's (AC.4):
//! - A static at [`APP`] points at the application singleton
//!   (`tfbPS3Application`). Its `_insight` (+0x2C) is the `igInsightCore`; its
//!   `_curFrame` (+0x5C) counts the game's frames since boot and its
//!   `_frameDelta` (+0x64) is the last frame's measured time in seconds.
//! - The core's `_updaters` (+0x1C) and `_systems` (+0x18) are lists. A list
//!   (`igDataList`) holds `_count` at +0x08, `_capacity` at +0x0C and a memory
//!   ref at +0x10 whose pointer (+0x14) is an array of object pointers.
//! - Among the updaters, the one `tfbActorUpdateGame` keeps `_gameActorList`
//!   (+0x08): every game actor the frame updates, one `tfbCorpus` each.
//! - A corpus is a scene-graph group: `_childList` (+0x20, an `igNodeList`),
//!   `_xform` (+0x24, a memory ref to a 4x4 float matrix, row vectors with the
//!   translation in the last row, the placement's scale not in it),
//!   `_placement` (+0x50), `_desiredFacing` (+0x54), `_velocity` (+0x58, m/s).
//!   A corpus whose child is an `igActor2` (a skeleton instance) is skinned.
//! - The placement is a `tfbActorInfo`: `_name` (+0x08, a C string),
//!   `_currentMatrix` (+0x3C: the position, 3 floats in metres, then 3 Euler
//!   angles in degrees, x the tilt, y the heading, z the roll), `_scaleVec`
//!   (+0xB8, percent), `_controller` (+0xD8) and `_gameActor` (+0xF4, back to
//!   the corpus). A heading θ faces (sin θ, 0, cos θ): 0 is +Z. The angles
//!   make the corpus's rotation as [`euler_matrix`] does: the roll about Z,
//!   then the tilt about X negated (a positive tilt lifts the nose), then the
//!   heading about Y; every actor whose `_xform` is its placement's matched
//!   that to 2.4e-7 in each of five states read, tilted ones included.
//! - The renderer's local-to-world, which the Edge job records carry, is that
//!   matrix with `_scaleVec` / 100 on its diagonal.
//! - A pad drives an actor whose controller is a `tfbPS3Controller`, `_id` at
//!   +0x134. The hero is the one skinned actor a pad drives: two unskinned
//!   ones (the camera's follow actor, the portal manager) have pad controllers
//!   too, and P1's hero's controller reads `_id` -1 in every state read so far.
//! - The camera: among the systems, the `tfbViewportSystem`'s
//!   `_currentViewport` (+0x14), a `tfbViewportInfo` placed the way an actor is
//!   (`_currentMatrix` at +0x3C, the heading its facing, the tilt its pitch).
//!
//! Every pointer is checked against its class's vtable before it's followed,
//! and a corpus that isn't what the list says is skipped with the reason, not
//! guessed at. Every address here is the 01.00 EBOOT's: [`read_frame`] checks
//! PINE's title and version first.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};

use crate::pine::{self, EmuStatus, PineClient, PineError};

/// The static pointer to the application singleton.
pub const APP: u32 = 0x0089_D4F0;

/// Each class's vtable, every instance's first word.
pub mod vtable {
    pub const APPLICATION: u32 = 0x0061_DD60;
    pub const INSIGHT_CORE: u32 = 0x0063_FF88;
    pub const SYSTEM_LIST: u32 = 0x0064_0158;
    pub const UPDATER_LIST: u32 = 0x0064_0028;
    pub const VIEWPORT_SYSTEM: u32 = 0x0061_FA18;
    pub const VIEWPORT: u32 = 0x0061_E720;
    pub const ACTOR_UPDATE: u32 = 0x0062_1790;
    pub const GAME_ACTOR_LIST: u32 = 0x0062_16D0;
    pub const NODE_LIST: u32 = 0x0064_3584;
    pub const CORPUS: u32 = 0x0062_1618;
    pub const ACTOR_INFO: u32 = 0x0062_1048;
    pub const PAD_CONTROLLER: u32 = 0x0062_6300;
    pub const SKELETON_INSTANCE: u32 = 0x0064_5B48;
    /// AG.1's watch list ([`crate::watch`]) follows these too.
    pub const LOGIC_SCALE_MESSAGE: u32 = 0x0063_04F8;
    pub const ACTOR_PARAMETERS: u32 = 0x0062_0CA8;
    pub const ANIMATION_COMPONENT: u32 = 0x0063_2650;
    pub const ANIM_CONTROL: u32 = 0x0063_2608;
    pub const ANIMATION_INFO: u32 = 0x0067_E0A0;
    pub const PHYSICS_ANIMATION: u32 = 0x0063_2790;
    /// AG.9's contacts: the hero's proxy and his ground.
    pub const CHARACTER_PROXY: u32 = 0x0063_4018;
    pub const CONTACT_INFO: u32 = 0x0062_32E0;
}

pub(crate) const APP_LEN: usize = 0x68;
pub(crate) const APP_INSIGHT: usize = 0x2C;
const APP_FRAME: usize = 0x5C;
const APP_FRAME_DELTA: usize = 0x64;
pub(crate) const INSIGHT_LEN: usize = 0x20;
pub(crate) const INSIGHT_SYSTEMS: usize = 0x18;
const INSIGHT_UPDATERS: usize = 0x1C;
const LIST_LEN: usize = 0x18;
const LIST_COUNT: usize = 0x08;
const LIST_CAPACITY: usize = 0x0C;
const LIST_DATA: usize = 0x14;
const ACTOR_UPDATE_LIST: usize = 0x08;
pub(crate) const VIEWPORT_SYSTEM_CURRENT: usize = 0x14;
pub(crate) const NAME: usize = 0x08;
pub(crate) const PLACEMENT_MATRIX: usize = 0x3C;
const VIEWPORT_LEN: usize = 0x54;
pub(crate) const CORPUS_LEN: usize = 0x80;
const CORPUS_CHILDREN: usize = 0x20;
pub(crate) const CORPUS_XFORM_SIZE: usize = 0x24;
pub(crate) const CORPUS_XFORM: usize = 0x28;
const CORPUS_PLACEMENT: usize = 0x50;
pub(crate) const CORPUS_VELOCITY: usize = 0x58;
pub(crate) const ACTOR_LEN: usize = 0xF8;
const ACTOR_SCALE: usize = 0xB8;
pub(crate) const ACTOR_CONTROLLER: usize = 0xD8;
const CONTROLLER_ID: u32 = 0x134;
/// A memory ref's size word: the byte count in the low 27 bits.
pub(crate) const MEMREF_SIZE_MASK: u32 = 0x07FF_FFFF;
pub(crate) const MATRIX_LEN: usize = 64;
/// What's read of a name; a longer one is cut there.
const NAME_LEN: usize = 64;
/// Lists longer than this are taken for garbage (the tutorial's holds 166).
pub const MAX_LIST: u32 = 4096;
/// Children looked at per corpus for a skeleton instance.
const MAX_CHILDREN: u32 = 16;

/// Guest memory read in batches: each range's bytes, or `None` for a range that
/// isn't mapped. PINE's reads on the oracle (refused unless it's paused), a
/// map of regions in the tests.
pub trait Gather {
    fn gather(&mut self, ranges: &[(u32, usize)]) -> Result<Vec<Option<Vec<u8>>>>;
}

impl Gather for PineClient {
    /// One status ask, then every range in as few messages as PINE's caps
    /// allow. A failed batch (a range not mapped) is read again range by range,
    /// so one bad range costs only itself.
    fn gather(&mut self, ranges: &[(u32, usize)]) -> Result<Vec<Option<Vec<u8>>>> {
        if ranges.is_empty() {
            return Ok(Vec::new());
        }
        match self.status()? {
            EmuStatus::Paused => {}
            other => return Err(PineError::NotPaused(other).into()),
        }
        let mut ops = Vec::new();
        let mut spans = Vec::with_capacity(ranges.len());
        for &(addr, len) in ranges {
            let range = pine::read_ops(addr, len)?;
            spans.push(ops.len()..ops.len() + range.len());
            ops.extend(range);
        }
        match self.run(&ops) {
            Ok(answers) => Ok(spans
                .iter()
                .map(|s| Some(pine::bytes_of(&ops[s.clone()], &answers[s.clone()])))
                .collect()),
            Err(PineError::Failed) => ranges
                .iter()
                .map(|&(addr, len)| match self.read_bytes(addr, len) {
                    Ok(bytes) => Ok(Some(bytes)),
                    Err(PineError::Failed) => Ok(None),
                    Err(e) => Err(e.into()),
                })
                .collect(),
            Err(e) => Err(e.into()),
        }
    }
}

/// One game actor as the frame left it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Actor {
    /// The corpus's guest address, which names the actor for the run.
    pub corpus: u32,
    /// Its `tfbActorInfo`.
    pub placement: u32,
    pub name: Option<String>,
    /// `_currentMatrix`'s position, metres.
    pub position: [f32; 3],
    /// `_currentMatrix`'s Euler angles, degrees: tilt, heading, roll.
    pub rotation: [f32; 3],
    /// `_scaleVec`, percent.
    pub scale: [f32; 3],
    /// The corpus's `_xform`, when it has one: row vectors, translation in
    /// the last row, the scale above not in it.
    pub world: Option<[f32; 16]>,
    /// `_velocity`, m/s.
    pub velocity: [f32; 3],
    /// A skeleton instance (`igActor2`) hangs under the corpus.
    pub skinned: bool,
    /// The `_id` of the pad controller driving it, if one does.
    pub pad: Option<i32>,
}

/// How far an actor went between two frames, in the ground plane.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Step {
    /// Metres in x and z.
    pub distance: f64,
    /// The heading of the move, degrees in (-180, 180].
    pub heading: f64,
    /// Metres in y.
    pub rise: f64,
}

impl Actor {
    /// `_currentMatrix`'s heading, degrees.
    pub fn heading(&self) -> f32 {
        self.rotation[1]
    }

    /// The move from this frame's position to `later`'s: ground distance,
    /// heading and rise.
    pub fn step_to(&self, later: &Actor) -> Step {
        let d = |i: usize| f64::from(later.position[i]) - f64::from(self.position[i]);
        Step {
            distance: d(0).hypot(d(2)),
            heading: heading_deg(d(0), d(2)),
            rise: d(1),
        }
    }

    /// The largest difference in any position component from `other`, metres.
    pub fn position_delta(&self, other: &Actor) -> f64 {
        (0..3)
            .map(|i| (f64::from(self.position[i]) - f64::from(other.position[i])).abs())
            .fold(0.0, f64::max)
    }

    /// How far `world`'s rotation is from [`euler_matrix`] of the angles,
    /// when `world` is this placement's own: its translation the position and
    /// its rows unit length (the game zeroes the matrix of an actor that isn't
    /// placed in the world, and an attached one's is its parent's doing).
    pub fn pose_error(&self) -> Option<f64> {
        let w = self.world?;
        let own = (0..3).all(|i| (w[12 + i] - self.position[i]).abs() <= 1e-4)
            && (0..3).all(|r| {
                let n: f32 = (0..3).map(|c| w[4 * r + c] * w[4 * r + c]).sum();
                (n - 1.0).abs() <= 1e-3
            });
        let m = euler_matrix(self.rotation);
        own.then(|| {
            (0..3)
                .flat_map(|r| (0..3).map(move |c| (r, c)))
                .map(|(r, c)| (m[r][c] - f64::from(w[4 * r + c])).abs())
                .fold(0.0, f64::max)
        })
    }

    /// One line for a log.
    pub fn summary(&self) -> String {
        format!(
            "{} at ({:.5}, {:.5}, {:.5}) heading {:.3} deg (tilt {:.3}, roll {:.3}), scale {}%, \
             velocity ({:.3}, {:.3}, {:.3}){}{}",
            self.name.as_deref().unwrap_or("?"),
            self.position[0],
            self.position[1],
            self.position[2],
            self.rotation[1],
            self.rotation[0],
            self.rotation[2],
            self.scale[0],
            self.velocity[0],
            self.velocity[1],
            self.velocity[2],
            if self.skinned { ", skinned" } else { "" },
            self.pad
                .map_or(String::new(), |id| format!(", pad id {id}")),
        )
    }
}

/// The camera the frame is drawn from.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Camera {
    pub viewport: u32,
    pub name: Option<String>,
    pub position: [f32; 3],
    /// Degrees: tilt (pitch), heading (facing), roll.
    pub rotation: [f32; 3],
}

impl Camera {
    pub fn heading(&self) -> f32 {
        self.rotation[1]
    }
}

/// A corpus on the list that wasn't read as an actor, and why.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Skipped {
    pub corpus: u32,
    pub why: String,
}

/// A frame's actors by kind: listed, read, skinned and players.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Counts {
    /// On the game's actor list.
    pub listed: u32,
    /// Read as actors.
    pub actors: usize,
    pub skinned: usize,
    /// Skinned and driven by a pad.
    pub players: usize,
}

/// Every game actor and the camera, read at one pause.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ActorFrame {
    /// The flip counter P1's `STATE` read at the pause, when the caller had it.
    pub flip: Option<u64>,
    /// `tfbApplication::_curFrame`, the game's own frame count.
    pub game_frame: u32,
    /// `_frameDelta`, seconds.
    pub frame_delta: f32,
    /// The list's count.
    pub listed: u32,
    pub actors: Vec<Actor>,
    pub skipped: Vec<Skipped>,
    pub camera: Option<Camera>,
}

impl ActorFrame {
    pub fn skinned(&self) -> impl Iterator<Item = &Actor> {
        self.actors.iter().filter(|a| a.skinned)
    }

    /// The skinned actors a pad drives.
    pub fn players(&self) -> impl Iterator<Item = &Actor> {
        self.skinned().filter(|a| a.pad.is_some())
    }

    /// The one player; an error when there's none or more than one.
    pub fn hero(&self) -> Result<&Actor> {
        let players: Vec<&Actor> = self.players().collect();
        match players.as_slice() {
            [one] => Ok(one),
            [] => bail!(
                "no skinned actor a pad drives among {} actors",
                self.actors.len()
            ),
            many => bail!(
                "{} players: {}",
                many.len(),
                many.iter()
                    .map(|a| a.name.as_deref().unwrap_or("?"))
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
        }
    }

    pub fn by_corpus(&self) -> BTreeMap<u32, &Actor> {
        self.actors.iter().map(|a| (a.corpus, a)).collect()
    }

    pub fn counts(&self) -> Counts {
        Counts {
            listed: self.listed,
            actors: self.actors.len(),
            skinned: self.skinned().count(),
            players: self.players().count(),
        }
    }

    /// One line for a log: the counts and the camera.
    pub fn summary(&self) -> String {
        let c = self.counts();
        format!(
            "{}game frame {} (dt {:.4} s): {} listed, {} actors read ({} skipped), {} skinned, {} \
             player(s); camera {}",
            self.flip.map_or(String::new(), |f| format!("flip {f}, ")),
            self.game_frame,
            self.frame_delta,
            c.listed,
            c.actors,
            self.skipped.len(),
            c.skinned,
            c.players,
            self.camera.as_ref().map_or("unread".into(), |cam| format!(
                "{} at ({:.4}, {:.4}, {:.4}) heading {:.3} tilt {:.3}",
                cam.name.as_deref().unwrap_or("?"),
                cam.position[0],
                cam.position[1],
                cam.position[2],
                cam.rotation[1],
                cam.rotation[0]
            )),
        )
    }

    pub fn to_json(&self) -> String {
        serde_json::to_string_pretty(self).expect("an actor frame serialises")
    }

    pub fn from_json(text: &str) -> Result<Self> {
        Ok(serde_json::from_str(text)?)
    }

    /// Writes the frame to `path`, refused inside any work tree: it's read out
    /// of guest memory.
    pub fn write(&self, path: &Path) -> Result<PathBuf> {
        let path = rrc_dump::output::refuse_repo_output(path)?;
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).with_context(|| format!("creating {}", dir.display()))?;
        }
        std::fs::write(&path, self.to_json())
            .with_context(|| format!("writing {}", path.display()))?;
        Ok(path)
    }

    pub fn load(path: &Path) -> Result<Self> {
        let text =
            std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
        Self::from_json(&text).with_context(|| format!("parsing {}", path.display()))
    }
}

/// The rotation `_currentMatrix`'s angles (`[tilt, heading, roll]`, degrees)
/// stand for, as row vectors the way `_xform` stores it (a local vector `v`
/// lands at `v M`): the roll about Z, then the tilt about X negated, then the
/// heading about Y.
pub fn euler_matrix(rotation: [f32; 3]) -> [[f64; 3]; 3] {
    let [tilt, heading, roll] = rotation.map(|d| f64::from(d).to_radians());
    let (sx, cx) = (-tilt).sin_cos();
    let (sy, cy) = heading.sin_cos();
    let (sz, cz) = roll.sin_cos();
    let rz = [[cz, sz, 0.0], [-sz, cz, 0.0], [0.0, 0.0, 1.0]];
    let rx = [[1.0, 0.0, 0.0], [0.0, cx, sx], [0.0, -sx, cx]];
    let ry = [[cy, 0.0, -sy], [0.0, 1.0, 0.0], [sy, 0.0, cy]];
    let mul = |a: [[f64; 3]; 3], b: [[f64; 3]; 3]| {
        std::array::from_fn(|i| std::array::from_fn(|j| (0..3).map(|k| a[i][k] * b[k][j]).sum()))
    };
    mul(mul(rz, rx), ry)
}

/// The game's heading of a move by `dx`, `dz`: degrees in (-180, 180], 0 along
/// +Z, 90 along +X.
pub fn heading_deg(dx: f64, dz: f64) -> f64 {
    wrap_deg(dx.atan2(dz).to_degrees())
}

/// `a` wrapped into (-180, 180].
pub fn wrap_deg(a: f64) -> f64 {
    let r = a.rem_euclid(360.0);
    if r > 180.0 { r - 360.0 } else { r }
}

/// The signed angle from `from` to `to`, degrees in (-180, 180].
pub fn angle_from(from: f64, to: f64) -> f64 {
    wrap_deg(to - from)
}

/// Reads every actor and the camera through PINE, at a pause: Giants 01.00
/// only, and PINE has to report the emulator paused before each batch and after
/// the last.
pub fn read_frame(pine: &mut PineClient) -> Result<ActorFrame> {
    crate::rng::check_title(pine)?;
    let frame = read(pine)?;
    match pine.status()? {
        EmuStatus::Paused => Ok(frame),
        other => {
            bail!("PINE reports the emulator {other:?} after the actor read; it reads at a pause")
        }
    }
}

pub(crate) fn be32(b: &[u8], at: usize) -> u32 {
    u32::from_be_bytes(b[at..at + 4].try_into().unwrap())
}

pub(crate) fn f32_at(b: &[u8], at: usize) -> f32 {
    f32::from_bits(be32(b, at))
}

pub(crate) fn floats<const N: usize>(b: &[u8], at: usize) -> [f32; N] {
    std::array::from_fn(|i| f32_at(b, at + 4 * i))
}

/// Where a PPU object can sit: 4-aligned, in the ELF's data or a heap.
pub(crate) fn object_ptr(p: u32) -> bool {
    p.is_multiple_of(4) && (0x0001_0000..0x5000_0000).contains(&p)
}

pub(crate) fn one(g: &mut impl Gather, addr: u32, len: usize, what: &str) -> Result<Vec<u8>> {
    if !object_ptr(addr) {
        bail!("{what} at {addr:#010x} isn't an object address");
    }
    g.gather(&[(addr, len)])?
        .pop()
        .flatten()
        .with_context(|| format!("{what} at {addr:#010x} isn't mapped"))
}

pub(crate) fn expect_vtable(bytes: &[u8], want: u32, at: u32, class: &str) -> Result<()> {
    let got = be32(bytes, 0);
    if got != want {
        bail!("the object at {at:#010x} has vtable {got:#010x}, not {class}'s {want:#010x}");
    }
    Ok(())
}

/// An `igDataList`'s element pointers, its vtable checked and its count
/// held to its capacity and [`MAX_LIST`].
pub(crate) fn list(g: &mut impl Gather, at: u32, want: u32, class: &str) -> Result<Vec<u32>> {
    let head = one(g, at, LIST_LEN, class)?;
    expect_vtable(&head, want, at, class)?;
    let (count, capacity, data) = (
        be32(&head, LIST_COUNT),
        be32(&head, LIST_CAPACITY),
        be32(&head, LIST_DATA),
    );
    if count > capacity || count > MAX_LIST {
        bail!("{class} at {at:#010x} counts {count} with capacity {capacity}");
    }
    if count == 0 {
        return Ok(Vec::new());
    }
    let words = one(g, data, 4 * count as usize, &format!("{class}'s array"))?;
    Ok((0..count as usize).map(|i| be32(&words, 4 * i)).collect())
}

/// The C string in `bytes` (cut at [`NAME_LEN`] when it has no NUL).
pub(crate) fn c_string(bytes: &[u8]) -> String {
    let end = bytes.iter().position(|&b| b == 0).unwrap_or(bytes.len());
    String::from_utf8_lossy(&bytes[..end]).into_owned()
}

/// A corpus read so far, on its way to an [`Actor`].
struct Partial {
    corpus: u32,
    placement: u32,
    world_at: Option<u32>,
    children_at: u32,
    velocity: [f32; 3],
    actor: Vec<u8>,
    world: Option<[f32; 16]>,
    name: Option<String>,
    pad: Option<i32>,
    children: Vec<u32>,
    skinned: bool,
}

/// Every game actor and the camera, from [`APP`] down. Fails when the chain to
/// the list breaks (the game isn't in a level yet, or this isn't the 01.00
/// layout); a corpus that doesn't read as an actor is skipped, with why.
pub fn read(g: &mut impl Gather) -> Result<ActorFrame> {
    let app = be32(&one(g, APP, 4, "the application pointer")?, 0);
    let a = one(g, app, APP_LEN, "the application")?;
    expect_vtable(&a, vtable::APPLICATION, app, "tfbPS3Application")?;
    let insight = be32(&a, APP_INSIGHT);
    let core = one(g, insight, INSIGHT_LEN, "the insight core")?;
    expect_vtable(&core, vtable::INSIGHT_CORE, insight, "igInsightCore")?;
    let updaters = list(
        g,
        be32(&core, INSIGHT_UPDATERS),
        vtable::UPDATER_LIST,
        "igInsightUpdaterList",
    )?;
    let systems = list(
        g,
        be32(&core, INSIGHT_SYSTEMS),
        vtable::SYSTEM_LIST,
        "igInsightSystemList",
    )?;

    // The updaters' and systems' heads, for their vtables and one field each.
    // An updater's vtable and its first field; a system's up to the viewport
    // system's `_currentViewport` (objects sit back to back, so no further).
    let heads: Vec<(u32, usize)> = updaters
        .iter()
        .map(|&p| (p, ACTOR_UPDATE_LIST + 4))
        .chain(systems.iter().map(|&p| (p, VIEWPORT_SYSTEM_CURRENT + 4)))
        .filter(|&(p, _)| object_ptr(p))
        .collect();
    let read_heads = g.gather(&heads)?;
    let mut actor_lists = Vec::new();
    let mut viewport = None;
    // Each head is judged by the list it came from, at the length read for it.
    let updater_heads = updaters.iter().filter(|&&p| object_ptr(p)).count();
    for (i, bytes) in read_heads.iter().enumerate() {
        let Some(b) = bytes else { continue };
        let updater = i < updater_heads;
        match be32(b, 0) {
            vtable::ACTOR_UPDATE if updater => actor_lists.push(be32(b, ACTOR_UPDATE_LIST)),
            vtable::VIEWPORT_SYSTEM if !updater => {
                viewport = Some(be32(b, VIEWPORT_SYSTEM_CURRENT)).filter(|&p| p != 0)
            }
            _ => {}
        }
    }
    let [actor_list] = actor_lists[..] else {
        bail!(
            "{} tfbActorUpdateGame among the {} updaters (the game isn't in a level, or this isn't the 01.00 layout)",
            actor_lists.len(),
            updaters.len()
        );
    };
    let corpora = list(g, actor_list, vtable::GAME_ACTOR_LIST, "gameActorList")?;

    // The corpora, and the camera's viewport beside them.
    let mut ranges: Vec<(u32, usize)> = corpora
        .iter()
        .map(|&c| (c, CORPUS_LEN))
        .filter(|&(c, _)| object_ptr(c))
        .collect();
    let viewport = viewport.filter(|&v| object_ptr(v));
    if let Some(v) = viewport {
        ranges.push((v, VIEWPORT_LEN));
    }
    let mut read_corpora: BTreeMap<u32, Option<Vec<u8>>> = BTreeMap::new();
    for (&(at, _), bytes) in ranges.iter().zip(g.gather(&ranges)?) {
        read_corpora.insert(at, bytes);
    }
    let viewport_bytes = viewport
        .and_then(|v| read_corpora.remove(&v).flatten())
        .filter(|b| be32(b, 0) == vtable::VIEWPORT);

    let mut skipped = Vec::new();
    let mut partials = Vec::new();
    for &corpus in &corpora {
        let skip = |why: String| Skipped { corpus, why };
        let Some(Some(c)) = read_corpora.get(&corpus) else {
            skipped.push(skip("not readable".into()));
            continue;
        };
        if be32(c, 0) != vtable::CORPUS {
            skipped.push(skip(format!(
                "vtable {:#010x}, not tfbCorpus's",
                be32(c, 0)
            )));
            continue;
        }
        let placement = be32(c, CORPUS_PLACEMENT);
        if !object_ptr(placement) {
            skipped.push(skip(format!("placement pointer {placement:#010x}")));
            continue;
        }
        let world_at = Some(be32(c, CORPUS_XFORM)).filter(|&p| {
            be32(c, CORPUS_XFORM_SIZE) & MEMREF_SIZE_MASK == MATRIX_LEN as u32 && object_ptr(p)
        });
        partials.push(Partial {
            corpus,
            placement,
            world_at,
            children_at: be32(c, CORPUS_CHILDREN),
            velocity: floats(c, CORPUS_VELOCITY),
            actor: Vec::new(),
            world: None,
            name: None,
            pad: None,
            children: Vec::new(),
            skinned: false,
        });
    }

    // Each corpus's placement, matrix and child list.
    let mut ranges = Vec::new();
    for p in &partials {
        ranges.push((p.placement, ACTOR_LEN));
        ranges.push((p.world_at.unwrap_or(p.placement), MATRIX_LEN));
        ranges.push((
            Some(p.children_at)
                .filter(|&c| object_ptr(c))
                .unwrap_or(p.placement),
            LIST_LEN,
        ));
    }
    let read_b = g.gather(&ranges)?;
    let mut kept = Vec::new();
    let mut lists = Vec::new();
    for (mut p, got) in partials.into_iter().zip(read_b.chunks(3)) {
        let skip = |why: String| Skipped {
            corpus: p.corpus,
            why,
        };
        let Some(actor) = &got[0] else {
            skipped.push(skip(format!(
                "placement {:#010x} not readable",
                p.placement
            )));
            continue;
        };
        if be32(actor, 0) != vtable::ACTOR_INFO {
            skipped.push(skip(format!(
                "placement {:#010x} has vtable {:#010x}, not tfbActorInfo's",
                p.placement,
                be32(actor, 0)
            )));
            continue;
        }
        let transform: [f32; 9] = floats(actor, PLACEMENT_MATRIX);
        let scale: [f32; 3] = floats(actor, ACTOR_SCALE);
        if !transform.iter().chain(&scale).all(|v| v.is_finite()) {
            skipped.push(skip(format!(
                "placement {:#010x} holds a non-finite transform",
                p.placement
            )));
            continue;
        }
        p.actor = actor.clone();
        p.world = p
            .world_at
            .and(got[1].as_ref())
            .map(|m| floats::<16>(m, 0))
            .filter(|m| m.iter().all(|v| v.is_finite()));
        let children = got[2]
            .as_ref()
            .filter(|b| object_ptr(p.children_at) && be32(b, 0) == vtable::NODE_LIST)
            .map(|b| (be32(b, LIST_COUNT).min(MAX_CHILDREN), be32(b, LIST_DATA)))
            .filter(|&(n, data)| n > 0 && object_ptr(data));
        lists.push(children);
        kept.push(p);
    }

    // Every pointer the placements and child lists hold, in one batch.
    let mut ranges = Vec::new();
    let named = |at: u32| Some(at).filter(|&n| object_ptr(n));
    for (p, children) in kept.iter().zip(&lists) {
        let name = named(be32(&p.actor, NAME)).unwrap_or(p.placement);
        let ctl = named(be32(&p.actor, ACTOR_CONTROLLER)).unwrap_or(p.placement);
        ranges.push((name, NAME_LEN));
        ranges.push((ctl, 4));
        ranges.push((ctl + CONTROLLER_ID, 4));
        let (n, data) = children.unwrap_or((1, p.placement));
        ranges.push((data, 4 * n as usize));
    }
    let camera_name = viewport_bytes.as_ref().and_then(|v| named(be32(v, NAME)));
    if let Some(n) = camera_name {
        ranges.push((n, NAME_LEN));
    }
    let read_c = g.gather(&ranges)?;
    for ((p, children), got) in kept.iter_mut().zip(&lists).zip(read_c.chunks(4)) {
        if named(be32(&p.actor, NAME)).is_some() {
            p.name = got[0].as_deref().map(c_string);
        }
        if named(be32(&p.actor, ACTOR_CONTROLLER)).is_some()
            && let (Some(vt), Some(id)) = (&got[1], &got[2])
            && be32(vt, 0) == vtable::PAD_CONTROLLER
        {
            p.pad = Some(be32(id, 0) as i32);
        }
        if let (Some((n, _)), Some(words)) = (children, &got[3]) {
            p.children = (0..*n as usize)
                .map(|i| be32(words, 4 * i))
                .filter(|&c| object_ptr(c))
                .collect();
        }
    }
    let camera = viewport_bytes.map(|v| Camera {
        viewport: viewport.unwrap(),
        name: camera_name
            .and(read_c.get(4 * kept.len()).cloned().flatten())
            .map(|b| c_string(&b)),
        position: floats(&v, PLACEMENT_MATRIX),
        rotation: floats(&v, PLACEMENT_MATRIX + 12),
    });

    // The children's vtables: a skeleton instance makes the actor skinned.
    let ranges: Vec<(u32, usize)> = kept
        .iter()
        .flat_map(|p| p.children.iter().map(|&c| (c, 4)))
        .collect();
    let mut read_d = g.gather(&ranges)?.into_iter();
    for p in &mut kept {
        for _ in 0..p.children.len() {
            if let Some(Some(vt)) = read_d.next() {
                p.skinned |= be32(&vt, 0) == vtable::SKELETON_INSTANCE;
            }
        }
    }

    // In the list's order, whichever stage skipped them.
    skipped.sort_by_key(|s| corpora.iter().position(|&c| c == s.corpus));
    let actors = kept
        .into_iter()
        .map(|p| {
            let m: [f32; 6] = floats(&p.actor, PLACEMENT_MATRIX);
            Actor {
                corpus: p.corpus,
                placement: p.placement,
                name: p.name,
                position: [m[0], m[1], m[2]],
                rotation: [m[3], m[4], m[5]],
                scale: floats(&p.actor, ACTOR_SCALE),
                world: p.world,
                velocity: p.velocity,
                skinned: p.skinned,
                pad: p.pad,
            }
        })
        .collect();
    Ok(ActorFrame {
        flip: None,
        game_frame: be32(&a, APP_FRAME),
        frame_delta: f32_at(&a, APP_FRAME_DELTA),
        listed: corpora.len() as u32,
        actors,
        skipped,
        camera,
    })
}

/// Giants' actor layout written out in code, for the tests here and in
/// `tests/`: the statics and one heap, laid out the way AF.8's fresh state has
/// them. The offsets are typed in from the registry, not taken from the
/// reader's constants, so a wrong constant there shows.
pub mod fixture {
    use super::Gather;
    use anyhow::Result;

    /// Where the heap starts; objects are handed out from here, 16-aligned.
    pub const HEAP: u32 = 0x3000_0000;
    const HEAP_LEN: usize = 0x1_0000;
    /// The ELF data pages holding [`super::APP`] and, past it,
    /// [`crate::level::HKP_WORLD`].
    pub const STATICS: u32 = 0x0089_0000;
    const STATICS_LEN: usize = 0x6_0000;
    /// A vtable none of the reader's classes has (another updater, a mesh node).
    pub const OTHER_VTABLE: u32 = 0x0062_0000;

    /// What one actor is made of.
    #[derive(Debug, Clone)]
    pub struct Spec {
        pub name: String,
        pub position: [f32; 3],
        pub rotation: [f32; 3],
        pub scale: [f32; 3],
        pub world: Option<[f32; 16]>,
        pub velocity: [f32; 3],
        pub skinned: bool,
        pub pad: Option<i32>,
    }

    impl Default for Spec {
        fn default() -> Self {
            Self {
                name: String::new(),
                position: [0.0; 3],
                rotation: [0.0; 3],
                scale: [100.0; 3],
                world: None,
                velocity: [0.0; 3],
                skinned: false,
                pad: None,
            }
        }
    }

    /// The addresses an added actor got.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub struct Added {
        pub corpus: u32,
        pub placement: u32,
    }

    pub struct World {
        statics: Vec<u8>,
        heap: Vec<u8>,
        next: u32,
        pub app: u32,
        pub insight: u32,
        pub updaters: u32,
        pub actor_update: u32,
        pub actor_list: u32,
        pub viewport: u32,
        list_data: u32,
        count: u32,
    }

    impl Default for World {
        fn default() -> Self {
            Self::new()
        }
    }

    impl World {
        /// The chain down to an empty actor list, a camera named "default" and
        /// the game at frame 5960, 1/60 s a frame.
        pub fn new() -> Self {
            let mut w = Self {
                statics: vec![0; STATICS_LEN],
                heap: vec![0; HEAP_LEN],
                next: HEAP,
                app: 0,
                insight: 0,
                updaters: 0,
                actor_update: 0,
                actor_list: 0,
                viewport: 0,
                list_data: 0,
                count: 0,
            };
            w.app = w.object(0x0061_DD60, 0x70);
            w.put32(super::APP, w.app);
            w.put32(w.app + 0x5C, 5960);
            w.putf(w.app + 0x64, 1.0 / 60.0);
            w.insight = w.object(0x0063_FF88, 0x38);
            w.put32(w.app + 0x2C, w.insight);
            // Updaters: physics, actors, viewport (only the middle one matters).
            w.actor_update = w.object(0x0062_1790, 0x14);
            let physics = w.object(OTHER_VTABLE, 0x0C);
            let viewport_update = w.object(OTHER_VTABLE + 8, 0x08);
            w.updaters = w.list(0x0064_0028, &[physics, w.actor_update, viewport_update], 16);
            w.put32(w.insight + 0x1C, w.updaters);
            w.actor_list = w.list(0x0062_16D0, &[], 256);
            w.list_data = w.get32(w.actor_list + 0x14);
            w.put32(w.actor_update + 0x08, w.actor_list);
            // Systems: a sprite system, then the viewport system and its viewport.
            w.viewport = w.object(0x0061_E720, 0x114);
            let name = w.string("default");
            w.put32(w.viewport + 0x08, name);
            let viewport_system = w.object(0x0061_FA18, 0x20);
            w.put32(viewport_system + 0x0C, w.viewport);
            w.put32(viewport_system + 0x14, w.viewport);
            let sprites = w.object(OTHER_VTABLE + 16, 0x0C);
            let systems = w.list(0x0064_0158, &[sprites, viewport_system], 16);
            w.put32(w.insight + 0x18, systems);
            w
        }

        fn slot(&mut self, addr: u32, len: usize) -> &mut [u8] {
            let (base, bytes) = if addr >= HEAP {
                (HEAP, &mut self.heap)
            } else {
                (STATICS, &mut self.statics)
            };
            let at = (addr - base) as usize;
            &mut bytes[at..at + len]
        }

        pub fn put32(&mut self, addr: u32, v: u32) {
            self.slot(addr, 4).copy_from_slice(&v.to_be_bytes());
        }

        pub fn get32(&mut self, addr: u32) -> u32 {
            u32::from_be_bytes(self.slot(addr, 4).try_into().unwrap())
        }

        pub fn putf(&mut self, addr: u32, v: f32) {
            self.put32(addr, v.to_bits());
        }

        fn putfs(&mut self, addr: u32, vs: &[f32]) {
            for (i, v) in vs.iter().enumerate() {
                self.putf(addr + 4 * i as u32, *v);
            }
        }

        /// `len` zeroed bytes off the heap, 16-aligned.
        pub fn alloc(&mut self, len: usize) -> u32 {
            let at = self.next;
            self.next = (at + len as u32).next_multiple_of(16);
            assert!(
                (self.next - HEAP) as usize <= HEAP_LEN,
                "the fixture's heap is full"
            );
            at
        }

        /// An object of `len` bytes whose first word is `vtable`, refcount 1.
        pub fn object(&mut self, vtable: u32, len: usize) -> u32 {
            let at = self.alloc(len);
            self.put32(at, vtable);
            self.put32(at + 4, 1);
            at
        }

        /// A C string off the heap.
        pub fn string(&mut self, s: &str) -> u32 {
            let at = self.alloc(s.len() + 1);
            self.slot(at, s.len()).copy_from_slice(s.as_bytes());
            at
        }

        /// Floats from `addr` on, big-endian as the guest keeps them.
        pub fn put_floats(&mut self, addr: u32, vs: &[f32]) {
            self.putfs(addr, vs);
        }

        /// An `igDataList` with `vtable`, holding `items`, room for `capacity`.
        pub fn list(&mut self, vtable: u32, items: &[u32], capacity: u32) -> u32 {
            let at = self.object(vtable, 0x18);
            let data = self.alloc(4 * capacity as usize);
            self.put32(at + 0x08, items.len() as u32);
            self.put32(at + 0x0C, capacity);
            self.put32(at + 0x10, 0x1000_0000 | (4 * capacity));
            self.put32(at + 0x14, data);
            for (i, &item) in items.iter().enumerate() {
                self.put32(data + 4 * i as u32, item);
            }
            at
        }

        /// Appends `corpus` (any word) to the game's actor list.
        pub fn push_corpus(&mut self, corpus: u32) {
            let at = self.list_data + 4 * self.count;
            self.put32(at, corpus);
            self.count += 1;
            self.put32(self.actor_list + 0x08, self.count);
        }

        /// The viewport's `_currentMatrix`: position, then tilt, heading, roll.
        pub fn camera(&mut self, position: [f32; 3], rotation: [f32; 3]) {
            self.putfs(self.viewport + 0x3C, &position);
            self.putfs(self.viewport + 0x48, &rotation);
        }

        /// A corpus and its `tfbActorInfo` per `spec`, on the actor list.
        pub fn actor(&mut self, spec: &Spec) -> Added {
            let corpus = self.object(0x0062_1618, 0x80);
            let placement = self.object(0x0062_1048, 0xF8);
            let name = self.string(&spec.name);
            self.put32(corpus + 0x08, name);
            self.put32(placement + 0x08, name);
            // A child: a skeleton instance, or a mesh node.
            let child = self.object(
                if spec.skinned {
                    0x0064_5B48
                } else {
                    OTHER_VTABLE + 24
                },
                0x28,
            );
            let children = self.list(0x0064_3584, &[child], 4);
            self.put32(corpus + 0x20, children);
            if let Some(m) = spec.world {
                let at = self.alloc(64);
                self.putfs(at, &m);
                self.put32(corpus + 0x24, 0x1000_0040);
                self.put32(corpus + 0x28, at);
            }
            self.put32(corpus + 0x50, placement);
            self.putfs(corpus + 0x58, &spec.velocity);
            self.putfs(placement + 0x24, &spec.position);
            self.putfs(placement + 0x3C, &spec.position);
            self.putfs(placement + 0x48, &spec.rotation);
            self.putfs(placement + 0xB8, &spec.scale);
            if let Some(id) = spec.pad {
                let pad = self.object(0x0062_6300, 0x20C);
                self.put32(pad + 0x134, id as u32);
                self.put32(placement + 0xD8, pad);
            }
            self.put32(placement + 0xF4, corpus);
            self.push_corpus(corpus);
            Added { corpus, placement }
        }

        /// The memory as `(base, bytes)` regions: the statics page and the heap.
        pub fn regions(&self) -> Vec<(u32, Vec<u8>)> {
            vec![(STATICS, self.statics.clone()), (HEAP, self.heap.clone())]
        }
    }

    impl Gather for World {
        fn gather(&mut self, ranges: &[(u32, usize)]) -> Result<Vec<Option<Vec<u8>>>> {
            Ok(ranges
                .iter()
                .map(|&(addr, len)| {
                    let (base, bytes) = if addr >= HEAP {
                        (HEAP, &self.heap)
                    } else if addr >= STATICS {
                        (STATICS, &self.statics)
                    } else {
                        return None;
                    };
                    let at = (addr - base) as usize;
                    bytes.get(at..at.checked_add(len)?).map(<[u8]>::to_vec)
                })
                .collect())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::fixture::{Added, OTHER_VTABLE, Spec, World};
    use super::*;

    fn translation(t: [f32; 3]) -> [f32; 16] {
        [
            1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, t[0], t[1], t[2], 1.0,
        ]
    }

    /// AF.8's fresh state in miniature: the hero, the camera's follow actor (a
    /// pad's, not skinned), a sheep turned and walking, and a macro with no
    /// matrix of its own.
    fn tutorial() -> (World, [Added; 4]) {
        let mut w = World::new();
        w.camera([6.5107, 52.2663, -107.1263], [-23.3597, 14.9997, 0.0]);
        let hero = w.actor(&Spec {
            name: "009_Eruptor".into(),
            position: [10.190_27, 43.382_67, -93.394_06],
            scale: [150.0; 3],
            world: Some(translation([10.190_27, 43.382_67, -93.394_06])),
            skinned: true,
            pad: Some(-1),
            ..Spec::default()
        });
        let follow = w.actor(&Spec {
            name: "Generic_FollowActor".into(),
            position: [10.190_27, 43.8395, -93.394_06],
            world: Some(translation([10.190_27, 43.8395, -93.394_06])),
            pad: Some(0),
            ..Spec::default()
        });
        let sheep = w.actor(&Spec {
            name: "Sheep_2".into(),
            position: [-1.5, 44.25, -88.0],
            rotation: [8.5, 91.13, -15.12],
            scale: [140.0; 3],
            velocity: [1.25, 0.0, -0.5],
            skinned: true,
            ..Spec::default()
        });
        let macros = w.actor(&Spec {
            name: "Macros_GameSystems".into(),
            position: [-20.284, 0.0, -78.138],
            ..Spec::default()
        });
        (w, [hero, follow, sheep, macros])
    }

    #[test]
    fn a_frame_reads_every_listed_actor_and_the_camera_out_of_giants_layout() {
        let (mut w, [hero, follow, sheep, macros]) = tutorial();
        let frame = read(&mut w).unwrap();
        assert_eq!(
            frame,
            ActorFrame {
                flip: None,
                game_frame: 5960,
                frame_delta: 1.0 / 60.0,
                listed: 4,
                actors: vec![
                    Actor {
                        corpus: hero.corpus,
                        placement: hero.placement,
                        name: Some("009_Eruptor".into()),
                        position: [10.190_27, 43.382_67, -93.394_06],
                        rotation: [0.0; 3],
                        scale: [150.0; 3],
                        world: Some(translation([10.190_27, 43.382_67, -93.394_06])),
                        velocity: [0.0; 3],
                        skinned: true,
                        pad: Some(-1),
                    },
                    Actor {
                        corpus: follow.corpus,
                        placement: follow.placement,
                        name: Some("Generic_FollowActor".into()),
                        position: [10.190_27, 43.8395, -93.394_06],
                        rotation: [0.0; 3],
                        scale: [100.0; 3],
                        world: Some(translation([10.190_27, 43.8395, -93.394_06])),
                        velocity: [0.0; 3],
                        skinned: false,
                        pad: Some(0),
                    },
                    Actor {
                        corpus: sheep.corpus,
                        placement: sheep.placement,
                        name: Some("Sheep_2".into()),
                        position: [-1.5, 44.25, -88.0],
                        rotation: [8.5, 91.13, -15.12],
                        scale: [140.0; 3],
                        world: None,
                        velocity: [1.25, 0.0, -0.5],
                        skinned: true,
                        pad: None,
                    },
                    Actor {
                        corpus: macros.corpus,
                        placement: macros.placement,
                        name: Some("Macros_GameSystems".into()),
                        position: [-20.284, 0.0, -78.138],
                        rotation: [0.0; 3],
                        scale: [100.0; 3],
                        world: None,
                        velocity: [0.0; 3],
                        skinned: false,
                        pad: None,
                    },
                ],
                skipped: vec![],
                camera: Some(Camera {
                    viewport: w.viewport,
                    name: Some("default".into()),
                    position: [6.5107, 52.2663, -107.1263],
                    rotation: [-23.3597, 14.9997, 0.0],
                }),
            }
        );
        assert_eq!(
            frame.counts(),
            Counts {
                listed: 4,
                actors: 4,
                skinned: 2,
                players: 1
            }
        );
        assert_eq!(frame.hero().unwrap().corpus, hero.corpus);
        assert_eq!(frame.by_corpus()[&sheep.corpus].heading(), 91.13);
        // Two reads of one memory are one frame.
        assert_eq!(read(&mut w).unwrap(), frame);
    }

    #[test]
    fn the_hero_is_the_one_skinned_actor_a_pad_drives() {
        // The follow actor has a pad and no skeleton: not a player.
        let mut w = World::new();
        w.actor(&Spec {
            name: "Generic_FollowActor".into(),
            pad: Some(0),
            ..Spec::default()
        });
        w.actor(&Spec {
            name: "Sheep_1".into(),
            skinned: true,
            ..Spec::default()
        });
        let err = read(&mut w).unwrap().hero().unwrap_err().to_string();
        assert!(
            err.contains("no skinned actor a pad drives among 2"),
            "{err}"
        );
        // A controller that isn't a pad's doesn't make a player either.
        let other = w.actor(&Spec {
            name: "Scripted".into(),
            skinned: true,
            ..Spec::default()
        });
        let ctl = w.object(OTHER_VTABLE, 0x200);
        w.put32(other.placement + 0xD8, ctl);
        assert_eq!(read(&mut w).unwrap().players().count(), 0);
        assert_eq!(read(&mut w).unwrap().actors[2].pad, None);
        // Two players, as in co-op, aren't one hero.
        for (name, id) in [("009_Eruptor", -1), ("112_TreeRex", 1)] {
            w.actor(&Spec {
                name: name.into(),
                skinned: true,
                pad: Some(id),
                ..Spec::default()
            });
        }
        let frame = read(&mut w).unwrap();
        assert_eq!(frame.counts().players, 2);
        let err = frame.hero().unwrap_err().to_string();
        assert_eq!(err, "2 players: 009_Eruptor, 112_TreeRex");
    }

    #[test]
    fn a_corpus_that_isnt_an_actor_is_skipped_with_why() {
        let (mut w, [hero, ..]) = tutorial();
        // Another class on the list, a null placement, a placement of another
        // class, a pointer to nothing, one past the heap's end, garbage floats.
        let stranger = w.object(OTHER_VTABLE, 0x80);
        w.push_corpus(stranger);
        let orphan = w.actor(&Spec::default());
        w.put32(orphan.corpus + 0x50, 0);
        let odd = w.actor(&Spec::default());
        w.put32(odd.placement, OTHER_VTABLE);
        w.push_corpus(0x4F00_0000);
        w.push_corpus(0x0000_0001);
        let nan = w.actor(&Spec::default());
        w.put32(nan.placement + 0x40, f32::NAN.to_bits());
        // A matrix ref that isn't 64 bytes and a child list of another class
        // read as no matrix and no skeleton, not as a skip.
        let partial = w.actor(&Spec {
            skinned: true,
            world: Some(translation([1.0, 2.0, 3.0])),
            ..Spec::default()
        });
        w.put32(partial.corpus + 0x24, 0x1000_0030);
        let children = w.get32(partial.corpus + 0x20);
        w.put32(children, OTHER_VTABLE);

        let frame = read(&mut w).unwrap();
        assert_eq!(frame.listed, 11);
        assert_eq!(
            frame.skipped,
            [
                Skipped {
                    corpus: stranger,
                    why: format!("vtable {OTHER_VTABLE:#010x}, not tfbCorpus's")
                },
                Skipped {
                    corpus: orphan.corpus,
                    why: "placement pointer 0x00000000".into()
                },
                Skipped {
                    corpus: odd.corpus,
                    why: format!(
                        "placement {:#010x} has vtable {OTHER_VTABLE:#010x}, not tfbActorInfo's",
                        odd.placement
                    )
                },
                Skipped {
                    corpus: 0x4F00_0000,
                    why: "not readable".into()
                },
                Skipped {
                    corpus: 0x0000_0001,
                    why: "not readable".into()
                },
                Skipped {
                    corpus: nan.corpus,
                    why: format!(
                        "placement {:#010x} holds a non-finite transform",
                        nan.placement
                    )
                },
            ]
        );
        let p = frame.by_corpus()[&partial.corpus].clone();
        assert_eq!((p.world, p.skinned), (None, false));
        // The rest read as before.
        assert_eq!(frame.counts().actors, 5);
        assert_eq!(frame.hero().unwrap().corpus, hero.corpus);
        // No name, no controller: read as such.
        w.put32(hero.placement + 0x08, 0);
        w.put32(hero.placement + 0xD8, 0);
        let bare = read(&mut w).unwrap();
        let h = bare.by_corpus()[&hero.corpus].clone();
        assert_eq!((h.name, h.pad), (None, None));
    }

    #[test]
    fn a_broken_chain_to_the_list_is_refused() {
        let check = |poke: &dyn Fn(&mut World), want: &str| {
            let (mut w, _) = tutorial();
            poke(&mut w);
            let err = format!("{:#}", read(&mut w).unwrap_err());
            assert!(err.contains(want), "{want:?} not in {err:?}");
        };
        check(
            &|w| w.put32(APP, 0),
            "the application at 0x00000000 isn't an object address",
        );
        check(
            &|w| {
                let app = w.app;
                w.put32(app, OTHER_VTABLE)
            },
            "not tfbPS3Application's 0x0061dd60",
        );
        check(
            &|w| {
                let i = w.insight;
                w.put32(i, OTHER_VTABLE)
            },
            "not igInsightCore's",
        );
        check(
            &|w| {
                let u = w.actor_update;
                w.put32(u, OTHER_VTABLE)
            },
            "0 tfbActorUpdateGame among the 3 updaters",
        );
        check(
            &|w| {
                let l = w.actor_list;
                w.put32(l, OTHER_VTABLE)
            },
            "not gameActorList's",
        );
        check(
            &|w| {
                let l = w.actor_list;
                w.put32(l + 0x08, 300)
            },
            "gameActorList at 0x",
        );
        check(
            &|w| {
                let l = w.updaters;
                w.put32(l + 0x08, MAX_LIST + 1);
                w.put32(l + 0x0C, MAX_LIST + 1)
            },
            "counts 4097",
        );
        // No viewport system, or its viewport not a viewport: no camera, the
        // actors still read.
        let (mut w, _) = tutorial();
        let v = w.viewport;
        w.put32(v, OTHER_VTABLE);
        let frame = read(&mut w).unwrap();
        assert_eq!((frame.camera.is_none(), frame.counts().actors), (true, 4));
    }

    #[test]
    fn headings_follow_the_games_convention() {
        // 0 faces +Z, 90 faces +X (sin, 0, cos).
        assert_eq!(heading_deg(0.0, 1.0), 0.0);
        assert_eq!(heading_deg(1.0, 0.0), 90.0);
        assert_eq!(heading_deg(0.0, -1.0), 180.0);
        assert_eq!(heading_deg(-1.0, 0.0), -90.0);
        let h = heading_deg(15f64.to_radians().sin(), 15f64.to_radians().cos());
        assert!((h - 15.0).abs() < 1e-12, "{h}");
        assert_eq!(wrap_deg(195.0), -165.0);
        assert_eq!(wrap_deg(-180.0), 180.0);
        assert_eq!(wrap_deg(720.0 + 15.0), 15.0);
        assert_eq!(angle_from(170.0, -170.0), 20.0);
        assert_eq!(angle_from(-170.0, 170.0), -20.0);
        assert_eq!(angle_from(15.0, 105.0), 90.0);

        let at = |position: [f32; 3]| Actor {
            corpus: 0,
            placement: 0,
            name: None,
            position,
            rotation: [0.0; 3],
            scale: [100.0; 3],
            world: None,
            velocity: [0.0; 3],
            skinned: true,
            pad: Some(-1),
        };
        // Three metres along heading 15 (the start camera's facing), dropping 0.5.
        let (s, c) = (15f32.to_radians().sin(), 15f32.to_radians().cos());
        let a = at([10.0, 43.5, -93.0]);
        let b = at([10.0 + 3.0 * s, 43.0, -93.0 + 3.0 * c]);
        let step = a.step_to(&b);
        assert!((step.distance - 3.0).abs() < 1e-5, "{step:?}");
        assert!((step.heading - 15.0).abs() < 1e-3, "{step:?}");
        assert_eq!(step.rise, -0.5);
        assert!((a.position_delta(&b) - f64::from(3.0 * c)).abs() < 1e-5);
        assert_eq!(a.position_delta(&a), 0.0);
    }

    #[test]
    fn the_angles_make_the_matrix_the_corpus_holds() {
        let close = |m: [[f64; 3]; 3], want: [[f64; 3]; 3], tol: f64| {
            for r in 0..3 {
                for c in 0..3 {
                    assert!((m[r][c] - want[r][c]).abs() <= tol, "{m:?} vs {want:?}");
                }
            }
        };
        // Heading 90: local +Z (forward) lands on +X, local +X on -Z.
        close(
            euler_matrix([0.0, 90.0, 0.0]),
            [[0.0, 0.0, -1.0], [0.0, 1.0, 0.0], [1.0, 0.0, 0.0]],
            1e-12,
        );
        // A positive tilt lifts the nose: forward gains +Y.
        let forward = euler_matrix([30.0, 0.0, 0.0])[2];
        close(
            [forward, [0.0; 3], [0.0; 3]],
            [[0.0, 0.5, 0.75f64.sqrt()], [0.0; 3], [0.0; 3]],
            1e-12,
        );
        // A positive roll lifts local +X.
        let m = euler_matrix([0.0, 0.0, 30.0]);
        assert!((m[0][1] - 0.5).abs() < 1e-12, "{m:?}");
        // AF.9's walk end: the hero turned to 105 deg on a slope, his angles
        // as `_currentMatrix` held them and his corpus's `_xform` rows.
        let mut hero = Actor {
            corpus: 0,
            placement: 0,
            name: None,
            position: [20.506_144, 43.498_642, -85.1342],
            rotation: [7.060_408_6, 104.999_66, 2.886_932_6],
            scale: [150.0; 3],
            world: Some([
                -0.264_464_38,
                0.049_983_24,
                -0.963_099_1,
                0.0,
                -0.105_541_82,
                0.991_157_6,
                0.080_420_94,
                0.0,
                0.958_602_67,
                0.122_915_73,
                -0.256_850_6,
                0.0,
                20.506_144,
                43.498_642,
                -85.1342,
                1.0,
            ]),
            velocity: [0.0; 3],
            skinned: true,
            pad: Some(-1),
        };
        let err = hero.pose_error().unwrap();
        assert!(err < 3e-7, "{err:e}");
        // Read the other way round (tilt not negated), it's off by 0.06.
        let wrong = euler_matrix([-7.060_408_6, 104.999_66, 2.886_932_6]);
        assert!((wrong[2][1] - f64::from(hero.world.unwrap()[9])).abs() > 0.05);
        // Not its own matrix: moved off the position, or zeroed.
        hero.position[0] += 1.0;
        assert_eq!(hero.pose_error(), None);
        hero.position[0] -= 1.0;
        hero.world = Some([0.0; 16]);
        hero.position = [0.0; 3];
        assert_eq!(hero.pose_error(), None);
        hero.world = None;
        assert_eq!(hero.pose_error(), None);
    }

    #[test]
    fn a_frame_round_trips_through_its_file() {
        let (mut w, _) = tutorial();
        let mut frame = read(&mut w).unwrap();
        frame.flip = Some(662);
        frame.skipped.push(Skipped {
            corpus: 0x3000_0040,
            why: "not readable".into(),
        });
        let text = frame.to_json();
        assert!(text.contains("\"flip\": 662"), "{text}");
        assert!(text.contains("\"name\": \"009_Eruptor\""), "{text}");
        assert_eq!(ActorFrame::from_json(&text).unwrap(), frame);
        assert!(frame.summary().contains(
            "flip 662, game frame 5960 (dt 0.0167 s): 4 listed, 4 actors read (1 skipped), \
             2 skinned, 1 player(s); camera default at (6.5107, 52.2663, -107.1263) heading \
             15.000 tilt -23.360"
        ));
        assert!(
            frame
                .hero()
                .unwrap()
                .summary()
                .starts_with("009_Eruptor at (10.19027, 43.38267, -93.39406) heading 0.000")
        );
    }
}
