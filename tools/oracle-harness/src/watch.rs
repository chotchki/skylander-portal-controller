//! O5's per-frame recorder as run steps, and Giants' watch list (openspec
//! AG.1 and AG.35, design decisions 5 and 6).
//!
//! O5 copies guest ranges into a ring at every read of the pad port it's armed
//! on, on the guest thread making the read, so a game that reads that port once
//! a frame is recorded every frame without a pause, whatever its frames do
//! against the flips (Giants reads port 0 once a game frame: [`GiantsWatch::PORT`]).
//! What it copies is a [`WatchList`]: named fields merged into the fewest
//! ranges (overlapping or touching fields share one), each field found again
//! in a record's payload by name. [`GiantsWatch::resolve`] builds Giants'
//! list at a pause, through PINE, from the same chain [`crate::actors`] walks:
//!
//! - the application's `_logicLoad`, `_GPUWait`, `_curFrame`,
//!   `_presentationRate` (the nominal frame, 1/60 s) and `_frameDelta` (the
//!   last frame's measured time), +0x54 to +0x68;
//! - the time scales. The game's: the application's `_gameLogicScaleMessage`
//!   (+0x10), a `GameLogicScaleMessage` the frame update sets to 1 and lets
//!   the time systems lower each frame, its scale at +0x08 and its stop flag
//!   at +0x0C (`FUN_0010fd90` clamps the frame time to [0.5, 3] x
//!   `_presentationRate` and multiplies it by that scale; a stopped one steps
//!   nothing). The actor's: its `ActorParameters._objectTimeScale` (+0x48),
//!   which the placement's time-scale virtual returns (1 without parameters)
//!   and the per-actor update multiplies in;
//! - the hero (the one skinned actor a pad drives): his placement's
//!   `_currentMatrix` (+0x3C: position, then tilt, heading and roll), his
//!   corpus from `_desiredFacing` (+0x54) through `_animationBlock` (+0x7C),
//!   `_velocity` and the game's `_timeStamp` (+0x6C) among them, the corpus's
//!   `_xform` matrix, his `ActorParameters` from `_groundMaxSpeed` (+0x0C) to
//!   `_objectTimeScale` (which the scripts swap while he walks), and his
//!   controller's button words and its 42 `_buttons` floats (+0x148 to
//!   +0x1F8), whose [23] and [24] (+0x1AC, +0x1B0) are the two floats the
//!   movement code reads for a stick;
//! - pad 0's controller (`_id` 0, in the tutorial the `Generic_FollowActor`'s),
//!   the same span: the hero's own controller (`_id` -1) held 0 in every one
//!   of those floats at every pad read of AG.1's walk, while pad 0's [23] and
//!   [24] carried the stick (x, -1 full left; y, +1 full forward) and [5], [6]
//!   and [16] the face buttons, so the stick a [`Frame`] carries is pad 0's;
//! - the camera: the viewport system's `_currentViewport` (+0x14), so a
//!   switch shows, and that viewport's `_currentMatrix`, `_FOV` (+0xAC) and
//!   `_farClip` with the two flags and `_transitionTime` after it (+0xD0 to
//!   +0xDC);
//! - the hero's clips, which is where his current clip and its time live: the
//!   corpus's `_animationBlock` is an `animationComponent` whose `_nowPlaying`
//!   vector (+0x08: count, a memory ref's size word and its pointer) holds one
//!   `animControl` per clip the blend plays (in AF.8's fresh state five:
//!   Eruptor's idle, walk, run and two leans). Each control's
//!   `_animationSequence` (+0x08, an `AnimationInfo`), `_animID` (+0x0C, the
//!   clip's id, which its `tfbPhysicsAnimation` carries too), its flags,
//!   `_timeRemaining` (+0x14), `_period` (+0x18) and the rest to +0x2C; each
//!   sequence's `_playbackPercent` (+0x20) and its `_blendFactor` (+0x38,
//!   percent: the clip at the top of the blend is what shows). The clip's
//!   time is its period less what remains. The clips' names come once, at the
//!   resolve, from each `tfbPhysicsAnimation`'s name.
//!
//! - the hero's contacts (AG.9): his corpus's `_animationBlock` holds his
//!   `tfbShapePhanton` (+0x1C), whose Havok property 0x2003 (the phantom's
//!   properties, an array of 16-byte key and value pairs at +0x7C, count
//!   +0x80) is his `tfbCharacterProxy`. The proxy keeps his corpus (+0x14),
//!   the velocity its last solve left (+0x20), his phantom (+0x30), its
//!   manifold (an `hkArray` of 0x30-byte `hkpRootCdPoint`s at +0x40: data,
//!   count, capacity), his physics link (+0x4C, which points back at the
//!   proxy from +0x10) and four flag bytes (+0x50). The link holds the
//!   support check's state (+0x20, 2 supported) and the surface normal it
//!   found (+0x30). The step rebuilds the manifold from its start with each
//!   cast (the closest start point, then the cast's first hit) and zeroes its
//!   count once the step is done (`FUN_00138da0`), so at a pad read the count
//!   reads 0 and the watch copies every entry the array's capacity holds. His
//!   corpus's `_ground` (+0x64, a `tfbScript::ContactInfo`) is where the
//!   ground finder leaves the ground under him: `_location` +0x1C,
//!   `_magnitude` +0x28, `_normal` +0x2C, `_contextID` +0x38, `_contactFlags`
//!   +0x3C. Each collidable a contact names is a phantom's (+0x10) or the
//!   level body's (+0x10), named at the resolve from [`crate::level`]'s
//!   solids and the level body.
//! - every `Generic_FollowActor` the frame holds (AG.13): each one's
//!   placement `_currentMatrix`, named `follow.<placement>`. The camera
//!   scripts keep their points on such clones (the magic point the eye keeps
//!   a camera definition's offset from, the player's camera target), so a
//!   record holds the camera's whole chain; they stay out of a [`Frame`] and
//!   [`GiantsWatch::followers`] reads them.
//!
//! The list is fixed when it's armed: a control the game swaps out mid-run
//! leaves its old address in the list, so each record carries the vector's
//! slots too and [`GiantsWatch::frame`] drops the clips from a frame whose
//! slots aren't the resolved ones. What O4's readback says the guest read on
//! the hero's pad is joined per flip afterwards (`PAD_STATE <port> <flip>`,
//! 4,096 flips kept), with its count of the port's reads in each flip, which
//! has to equal the records the ring holds at that flip ([`RingCheck`]).
//!
//! Every address is the 01.00 EBOOT's and every pointer is checked against its
//! class's vtable before it's followed, as in [`crate::actors`].

use std::collections::BTreeMap;
use std::path::Path;

use anyhow::{Context, Result, bail};
use guac_oracle::input::Ds3Pad;
use guac_oracle::recording::{Body, Clip, Clock, Contacts, Frame, Ground, Plane, Recording, View};
use serde::{Deserialize, Serialize};

use crate::actors::{self, Gather, be32, expect_vtable, f32_at, floats, object_ptr, one, vtable};
use crate::control::Control;
use crate::outputs::hex;
use crate::verbs::{
    PadRead, Refusal, Reply, ReplyError, Verb, WATCH_HEADER, WATCH_MAX_BYTES, WATCH_MAX_RANGES,
    WATCH_REPLY_BYTES, Watch, WatchRange, WatchRecord, WatchReply, WatchSummary,
};

/// A named span of guest memory.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Field {
    pub name: String,
    pub addr: u32,
    pub len: u32,
}

impl Field {
    pub fn new(name: impl Into<String>, addr: u32, len: u32) -> Self {
        Self {
            name: name.into(),
            addr,
            len,
        }
    }

    fn end(&self) -> u64 {
        u64::from(self.addr) + u64::from(self.len)
    }
}

/// Where a field lands in a record: its range and its offset in the payload.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Placed {
    pub range: usize,
    pub offset: usize,
    pub len: usize,
}

/// Fields merged into the ranges O5 copies: sorted by address, a field that
/// overlaps or touches the range before it extends that range, so the ranges
/// are disjoint and each byte is copied once.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WatchList {
    fields: Vec<Field>,
    ranges: Vec<WatchRange>,
    placed: BTreeMap<String, Placed>,
}

impl WatchList {
    /// Refused: no fields, an empty or wrapping field, a name twice, or more
    /// ranges or bytes than O5 takes.
    pub fn new(fields: Vec<Field>) -> Result<Self> {
        if fields.is_empty() {
            bail!("a watch needs at least one field");
        }
        let mut order: Vec<usize> = (0..fields.len()).collect();
        order.sort_by_key(|&i| (fields[i].addr, fields[i].len));
        let mut ranges: Vec<(u32, u64)> = Vec::new();
        let mut range_of = vec![0; fields.len()];
        for &i in &order {
            let f = &fields[i];
            if f.len == 0 || f.end() > 1 << 32 {
                bail!(
                    "field {} at {:#010x} + {} isn't a range",
                    f.name,
                    f.addr,
                    f.len
                );
            }
            match ranges.last_mut() {
                Some((_, end)) if u64::from(f.addr) <= *end => *end = (*end).max(f.end()),
                _ => ranges.push((f.addr, f.end())),
            }
            range_of[i] = ranges.len() - 1;
        }
        if ranges.len() > WATCH_MAX_RANGES {
            bail!(
                "{} ranges after merging; O5 takes {WATCH_MAX_RANGES}",
                ranges.len()
            );
        }
        let ranges: Vec<WatchRange> = ranges
            .into_iter()
            .map(|(addr, end)| WatchRange {
                addr,
                len: u32::try_from(end - u64::from(addr)).expect("a range under 4 GiB"),
            })
            .collect();
        let mut starts = Vec::with_capacity(ranges.len());
        let mut at = 0usize;
        for r in &ranges {
            starts.push(at);
            at += r.len as usize;
        }
        if at > WATCH_MAX_BYTES as usize {
            bail!("{at} bytes a record; O5 takes {WATCH_MAX_BYTES}");
        }
        let mut placed = BTreeMap::new();
        for (i, f) in fields.iter().enumerate() {
            let range = range_of[i];
            let p = Placed {
                range,
                offset: starts[range] + (f.addr - ranges[range].addr) as usize,
                len: f.len as usize,
            };
            if placed.insert(f.name.clone(), p).is_some() {
                bail!("the field name {} is used twice", f.name);
            }
        }
        Ok(Self {
            fields,
            ranges,
            placed,
        })
    }

    pub fn fields(&self) -> &[Field] {
        &self.fields
    }

    pub fn ranges(&self) -> &[WatchRange] {
        &self.ranges
    }

    /// What O5 copies at each read, the ranges back to back.
    pub fn bytes(&self) -> usize {
        self.ranges.iter().map(|r| r.len as usize).sum()
    }

    pub fn placed(&self, name: &str) -> Option<Placed> {
        self.placed.get(name).copied()
    }

    /// The field's bytes in `record`, or `None` when the list has no such
    /// field, the record's payload is the wrong size or the field's range
    /// wasn't readable that frame.
    pub fn field<'a>(&self, record: &'a WatchRecord, name: &str) -> Option<&'a [u8]> {
        let p = self.placed(name)?;
        if record.payload.len() != self.bytes() || record.missing & (1 << p.range) != 0 {
            return None;
        }
        Some(&record.payload[p.offset..p.offset + p.len])
    }

    /// The `WATCH` that arms a ring of `frames` records of this list, one at
    /// every read of pad `port`.
    pub fn arm(&self, frames: u32, port: u8) -> Watch {
        Watch::Arm {
            frames,
            port,
            ranges: self.ranges.clone(),
        }
    }
}

/// A ring read back: the summary and every record it held, oldest first.
#[derive(Debug, Clone, PartialEq)]
pub struct Ring {
    pub summary: WatchSummary,
    pub records: Vec<WatchRecord>,
}

impl Ring {
    /// The flips the records fell in, each once, in order: two records share a
    /// flip that ran two game frames.
    pub fn flips(&self) -> Vec<u64> {
        let mut flips: Vec<u64> = self.records.iter().map(|r| r.flip).collect();
        flips.dedup();
        flips
    }
}

impl Control {
    /// `WATCH`: a ring of `frames` records of `list`, one at every read of pad
    /// `port`, its first record the port's first read in a whole flip. The
    /// payload and port O5 reports have to be the list's and the one asked for.
    pub fn watch_arm(&self, list: &WatchList, port: u8, frames: u32) -> Result<()> {
        match self.verbs().send_watch(&list.arm(frames, port))? {
            WatchReply::Armed {
                bytes,
                frames: f,
                port: p,
            } if bytes as usize == list.bytes() && f == frames && p == port => Ok(()),
            other => bail!(
                "WATCH answered {other:?} for a {}-byte list of {frames} records on port {port}",
                list.bytes()
            ),
        }
    }

    /// At a pause: the summary, then every record held, as many to a reply as
    /// one carries, all on one connection.
    pub fn watch_read(&self) -> Result<Ring> {
        let WatchReply::Summary(summary) = self.verbs().send_watch(&Watch::Summary)? else {
            bail!("WATCH_READ answered without its summary");
        };
        let stride = (WATCH_HEADER + summary.bytes as usize) as u64;
        let per = (WATCH_REPLY_BYTES / stride).max(1);
        let pages: Vec<Watch> = (0..summary.held)
            .step_by(per as usize)
            .map(|first| Watch::Read {
                first,
                count: per.min(summary.held - first),
            })
            .collect();
        let mut records = Vec::with_capacity(summary.held as usize);
        if !pages.is_empty() {
            for (page, reply) in pages.iter().zip(self.verbs().send_watch_all(&pages)?) {
                match reply
                    .with_context(|| format!("reading the ring: {}", page.encode().trim_end()))?
                {
                    WatchReply::Records { records: r, .. } => records.extend(r),
                    other => bail!("WATCH_READ answered {other:?}"),
                }
            }
        }
        if records
            .iter()
            .any(|r| r.payload.len() != summary.bytes as usize)
        {
            bail!(
                "a record's payload isn't the {} bytes the summary says",
                summary.bytes
            );
        }
        Ok(Ring { summary, records })
    }

    /// `WATCH_CLEAR`: the watch disarmed and its ring freed, so nothing more
    /// is copied at the pad reads.
    pub fn watch_clear(&self) -> Result<()> {
        self.verbs().send_watch(&Watch::Clear)?;
        Ok(())
    }

    /// O4's readback on `port` joined by flip for a ring's flips: what the
    /// guest read there, and how many times it read the port in each. O4 keeps
    /// ports 0 and 1; another port has none.
    pub fn pad_readback(
        &self,
        port: u8,
        flips: &[u64],
    ) -> Result<(BTreeMap<u64, Ds3Pad>, BTreeMap<u64, u32>)> {
        let mut pads = BTreeMap::new();
        let mut reads = BTreeMap::new();
        if port > 1 {
            return Ok((pads, reads));
        }
        for (read, &flip) in self.pad_reads(port, flips)?.into_iter().zip(flips) {
            if let Some(r) = read {
                pads.insert(flip, r.pad);
                reads.insert(flip, r.reads);
            }
        }
        Ok((pads, reads))
    }

    /// O4's readback for each flip on `port`, on one connection; `None` for a
    /// flip the guest made no read in (or past the 4,096 flips O4 keeps). Its
    /// `reads` counts the port's reads in that flip.
    pub fn pad_reads(&self, port: u8, flips: &[u64]) -> Result<Vec<Option<PadRead>>> {
        let verbs: Vec<Verb> = flips
            .iter()
            .map(|&flip| Verb::PadState {
                port,
                flip: Some(flip),
            })
            .collect();
        if verbs.is_empty() {
            return Ok(Vec::new());
        }
        self.verbs()
            .send_all(&verbs)?
            .into_iter()
            .map(|reply| match reply {
                Ok(Reply::PadRead(read)) => Ok(Some(read)),
                Ok(other) => bail!("PAD_STATE answered {other:?}"),
                Err(e) if e.refusal() == Some(Refusal::NoRead) => Ok(None),
                Err(e) => Err(anyhow::Error::from(e)),
            })
            .collect()
    }
}

/// A clip the hero's animation plays: its control, its sequence, its id and
/// the name its `tfbPhysicsAnimation` carries.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WatchedClip {
    pub control: u32,
    pub sequence: u32,
    pub id: u32,
    pub name: Option<String>,
}

/// Giants' watch list, resolved at a pause, with what it was resolved from.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GiantsWatch {
    pub list: WatchList,
    /// The pad port whose reads record ([`GiantsWatch::PORT`]); a list written
    /// before AG.35 has none and was recorded at each flip's first read.
    #[serde(default)]
    pub port: u8,
    pub application: u32,
    pub logic_scale: u32,
    pub hero_corpus: u32,
    pub hero_placement: u32,
    pub hero_name: Option<String>,
    pub controller: u32,
    /// The controller whose `_id` is 0, pad 0's: in AF.8's fresh state the
    /// `Generic_FollowActor`'s (the hero's own reads `_id` -1).
    #[serde(default)]
    pub pad0: Option<u32>,
    pub parameters: u32,
    pub viewport_system: Option<u32>,
    pub viewport: Option<u32>,
    pub animation: u32,
    /// The `_nowPlaying` slots watched (the vector's whole array, live or not).
    pub slots: u32,
    pub clips: Vec<WatchedClip>,
    /// The hero's character proxy and what hangs off it (AG.9): none in a
    /// list written before it, or when his animation block holds no phantom.
    #[serde(default)]
    pub proxy: Option<ProxyWatch>,
    /// Every `Generic_FollowActor` at the resolve (AG.13): none in a list
    /// written before it.
    #[serde(default)]
    pub followers: Vec<Follower>,
}

/// A `Generic_FollowActor` on the watch list (AG.13).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Follower {
    pub placement: u32,
    pub corpus: u32,
}

impl Follower {
    /// The name it takes on the list: `follow.` and its placement in hex.
    pub fn field(&self) -> String {
        format!("follow.{:08x}", self.placement)
    }
}

/// A follower where a record found it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FollowerAt {
    pub follower: Follower,
    pub position: [f32; 3],
    /// Tilt, heading, roll, degrees.
    pub rotation: [f32; 3],
}

/// Where the hero's contacts live, resolved at the pause (AG.9).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProxyWatch {
    pub phantom: u32,
    pub proxy: u32,
    pub link: u32,
    /// The manifold's data at the resolve and the entries watched there (its
    /// capacity then): an array that grows moves, and a record whose proxy
    /// names another data pointer has no planes.
    pub manifold: u32,
    pub capacity: u32,
    /// His corpus's `_ground` `ContactInfo`.
    pub ground: u32,
    /// Each collidable the world held at the resolve, by name: every simulated
    /// actor's phantom by its placement's name (its corpus in hex without
    /// one) and the level's static body as `level`.
    pub colliders: BTreeMap<u32, String>,
}

impl ProxyWatch {
    /// What a contact's collider is called, its address in hex when the
    /// resolve didn't hold it.
    pub fn collider_name(&self, collider: u32) -> String {
        self.colliders
            .get(&collider)
            .cloned()
            .unwrap_or_else(|| format!("{collider:#010x}"))
    }
}

const APP_FIRST: u32 = 0x54;
const APP_LOGIC_SCALE: usize = 0x10;
const SCALE_FIRST: u32 = 0x08;
const CORPUS_FIRST: u32 = 0x54;
const CORPUS_TIME_STAMP: usize = 0x6C;
const CORPUS_ANIMATION: usize = 0x7C;
const ACTOR_PARAMETERS: usize = 0xE0;
const PARAMS_FIRST: u32 = 0x0C;
const PARAMS_LEN: u32 = 0x40;
const PAD_FIRST: u32 = 0x148;
const PAD_LEN: u32 = 0xB0;
/// The stick floats' offsets in the pad field: `_buttons` [23] and [24].
const STICK: [usize; 2] = [0x1AC - PAD_FIRST as usize, 0x1B0 - PAD_FIRST as usize];
const VIEWPORT_FOV: u32 = 0xAC;
const VIEWPORT_FAR: u32 = 0xD0;
const COMPONENT_LEN: usize = 0x28;
const COMPONENT_PLAYING: u32 = 0x08;
const CONTROL_LEN: usize = 0x2C;
const CONTROL_FIRST: u32 = 0x08;
const SEQUENCE_LEN: usize = 0x44;
const SEQUENCE_FIRST: u32 = 0x20;
const SEQUENCE_ANIMATION: usize = 0x3C;
const COMPONENT_PHANTOM: usize = 0x1C;
const PHANTOM_LEN: usize = 0x84;
const PHANTOM_COLLIDABLE: u32 = 0x10;
const PHANTOM_PROPERTIES: usize = 0x7C;
/// Havok's `hkpProperty`: a key word, padding, then a 64-bit value.
const PROPERTY_LEN: usize = 0x10;
/// The property whose value is the phantom's `tfbCharacterProxy`.
const PROPERTY_PROXY: u32 = 0x2003;
const MAX_PROPERTIES: u32 = 64;
const PROXY_LEN: usize = 0x60;
const PROXY_CORPUS: usize = 0x14;
const PROXY_VELOCITY: usize = 0x20;
const PROXY_PHANTOM: usize = 0x30;
const PROXY_MANIFOLD: usize = 0x40;
const PROXY_LINK: usize = 0x4C;
const PROXY_FLAGS: usize = 0x50;
/// The proxy as watched: from its corpus to its end.
const PROXY_FIRST: u32 = 0x14;
const HK_ARRAY_CAPACITY_MASK: u32 = 0x3FFF_FFFF;
/// Manifold entries watched at most: Giants' rebuild keeps the closest start
/// point and the cast's first hit, so the hero's array held 2 in AF.8's state.
const MAX_MANIFOLD: u32 = 16;
const ROOT_CD_POINT_LEN: u32 = 0x30;
const LINK_LEN: usize = 0x40;
const LINK_PROXY: usize = 0x10;
/// The link as watched: the support state to the surface normal's end.
const LINK_FIRST: u32 = 0x20;
const LINK_WATCHED: u32 = 0x20;
const CORPUS_GROUND: usize = 0x64;
const CONTACT_INFO_LEN: usize = 0x44;
/// The ContactInfo as watched: `_location` to `_contactFlags`.
const GROUND_FIRST: u32 = 0x1C;
const GROUND_WATCHED: u32 = 0x24;

/// Slots of `_nowPlaying` watched at most: the fresh state's vector holds 8 for
/// its 5 clips, and a count past this is taken for garbage.
const MAX_SLOTS: u32 = 16;
/// A clip's name is its `.ma` path (`Models/Characters/MinionsMonsters/
/// Eruptor/Eruptor_Idle.ma` is 57 bytes); longer is cut.
const CLIP_NAME_LEN: usize = 128;
/// The clone the camera scripts keep their points on (AG.13); AF.8's fresh
/// state holds six.
const FOLLOW_ACTOR: &str = "Generic_FollowActor";
/// Followers watched at most: past this the list would crowd O5's 64 ranges.
const MAX_FOLLOWERS: usize = 16;

impl GiantsWatch {
    /// The port the watch records on: pad 0, the hero's, which Giants reads
    /// once a game frame (AG.35's runs: as many reads a flip as game frames).
    pub const PORT: u8 = 0;

    /// Builds Giants' list from the frame the run is paused at: the actor
    /// chain to the hero and the camera, then the time scales and the hero's
    /// clips. Fails when there's no single hero, or a pointer on the way isn't
    /// its class's.
    pub fn resolve(g: &mut impl Gather) -> Result<Self> {
        let frame = actors::read(g)?;
        let hero = frame.hero()?.clone();
        let pad0_actor = {
            let on_pad0: Vec<_> = frame.actors.iter().filter(|a| a.pad == Some(0)).collect();
            match on_pad0[..] {
                [one] => Some(one.placement),
                _ => None,
            }
        };
        let viewport = frame.camera.as_ref().map(|c| c.viewport);
        let followers: Vec<Follower> = frame
            .actors
            .iter()
            .filter(|a| a.corpus != hero.corpus && a.name.as_deref() == Some(FOLLOW_ACTOR))
            .map(|a| Follower {
                placement: a.placement,
                corpus: a.corpus,
            })
            .take(MAX_FOLLOWERS)
            .collect();

        let app = be32(&one(g, actors::APP, 4, "the application pointer")?, 0);
        let a = one(g, app, actors::APP_LEN, "the application")?;
        expect_vtable(&a, vtable::APPLICATION, app, "tfbPS3Application")?;
        let logic_scale = be32(&a, APP_LOGIC_SCALE);
        let m = one(g, logic_scale, 0x10, "the game logic scale message")?;
        expect_vtable(
            &m,
            vtable::LOGIC_SCALE_MESSAGE,
            logic_scale,
            "GameLogicScaleMessage",
        )?;
        let viewport_system = viewport_system(g, &a)?;

        let p = one(g, hero.placement, actors::ACTOR_LEN, "the hero's placement")?;
        let controller = be32(&p, actors::ACTOR_CONTROLLER);
        let c = one(g, controller, 4, "the hero's controller")?;
        expect_vtable(&c, vtable::PAD_CONTROLLER, controller, "tfbPS3Controller")?;
        let pad0 = match pad0_actor {
            Some(placement) => {
                let at = one(g, placement, actors::ACTOR_LEN, "pad 0's actor's placement")?;
                let c = be32(&at, actors::ACTOR_CONTROLLER);
                let head = one(g, c, 4, "pad 0's controller")?;
                expect_vtable(&head, vtable::PAD_CONTROLLER, c, "tfbPS3Controller")?;
                Some(c)
            }
            None => None,
        };
        let parameters = be32(&p, ACTOR_PARAMETERS);
        let ap = one(g, parameters, 4, "the hero's ActorParameters")?;
        expect_vtable(&ap, vtable::ACTOR_PARAMETERS, parameters, "ActorParameters")?;

        let corpus = one(g, hero.corpus, actors::CORPUS_LEN, "the hero's corpus")?;
        let xform = be32(&corpus, actors::CORPUS_XFORM);
        if be32(&corpus, actors::CORPUS_XFORM_SIZE) & actors::MEMREF_SIZE_MASK
            != actors::MATRIX_LEN as u32
            || !object_ptr(xform)
        {
            bail!(
                "the hero's corpus at {:#010x} has no 4x4 _xform",
                hero.corpus
            );
        }
        let animation = be32(&corpus, CORPUS_ANIMATION);
        let block = one(g, animation, COMPONENT_LEN, "the hero's animation block")?;
        expect_vtable(
            &block,
            vtable::ANIMATION_COMPONENT,
            animation,
            "animationComponent",
        )?;
        let playing = COMPONENT_PLAYING as usize;
        let (count, size, slots) = (
            be32(&block, playing),
            be32(&block, playing + 4) & actors::MEMREF_SIZE_MASK,
            be32(&block, playing + 8),
        );
        let capacity = (size / 4).min(MAX_SLOTS);
        if count > capacity || !object_ptr(slots) {
            bail!(
                "the hero's _nowPlaying at {animation:#010x} counts {count} in {size} bytes at {slots:#010x}"
            );
        }
        let words = one(
            g,
            slots,
            4 * capacity as usize,
            "the hero's _nowPlaying slots",
        )?;
        let controls: Vec<u32> = (0..count as usize).map(|i| be32(&words, 4 * i)).collect();
        let mut clips = Vec::new();
        for &control in &controls {
            let ctl = one(g, control, CONTROL_LEN, "an animControl")?;
            expect_vtable(&ctl, vtable::ANIM_CONTROL, control, "animControl")?;
            let sequence = be32(&ctl, 0x08);
            let seq = one(g, sequence, SEQUENCE_LEN, "an AnimationInfo")?;
            expect_vtable(&seq, vtable::ANIMATION_INFO, sequence, "AnimationInfo")?;
            let animation = be32(&seq, SEQUENCE_ANIMATION);
            let name = match one(g, animation, 0x0C, "a tfbPhysicsAnimation") {
                Ok(pa) if be32(&pa, 0) == vtable::PHYSICS_ANIMATION => {
                    let at = be32(&pa, actors::NAME);
                    one(g, at, CLIP_NAME_LEN, "a clip's name")
                        .ok()
                        .map(|b| actors::c_string(&b))
                }
                _ => None,
            };
            clips.push(WatchedClip {
                control,
                sequence,
                id: be32(&ctl, 0x0C),
                name,
            });
        }

        let proxy = proxy_watch(g, &frame, hero.corpus, &corpus, &block)?;

        let mut fields = vec![
            Field::new("app", app + APP_FIRST, 0x14),
            Field::new("logic_scale", logic_scale + SCALE_FIRST, 8),
            Field::new(
                "hero.placement",
                hero.placement + actors::PLACEMENT_MATRIX as u32,
                24,
            ),
            Field::new("hero.corpus", hero.corpus + CORPUS_FIRST, 0x2C),
            Field::new("hero.xform", xform, actors::MATRIX_LEN as u32),
            Field::new("hero.parameters", parameters + PARAMS_FIRST, PARAMS_LEN),
            Field::new("hero.pad", controller + PAD_FIRST, PAD_LEN),
            Field::new("anim.playing", animation + COMPONENT_PLAYING, 12),
            Field::new("anim.slots", slots, 4 * capacity.max(1)),
        ];
        if let Some(c) = pad0.filter(|&c| c != controller) {
            fields.push(Field::new("pad0", c + PAD_FIRST, PAD_LEN));
        }
        if let Some(system) = viewport_system {
            fields.push(Field::new(
                "camera.current",
                system + actors::VIEWPORT_SYSTEM_CURRENT as u32,
                4,
            ));
        }
        if let Some(v) = viewport {
            fields.push(Field::new(
                "camera.placement",
                v + actors::PLACEMENT_MATRIX as u32,
                24,
            ));
            fields.push(Field::new("camera.fov", v + VIEWPORT_FOV, 4));
            fields.push(Field::new("camera.far", v + VIEWPORT_FAR, 12));
        }
        if let Some(p) = &proxy {
            fields.push(Field::new(
                "hero.proxy",
                p.proxy + PROXY_FIRST,
                PROXY_LEN as u32 - PROXY_FIRST,
            ));
            if p.capacity > 0 {
                fields.push(Field::new(
                    "hero.manifold",
                    p.manifold,
                    p.capacity * ROOT_CD_POINT_LEN,
                ));
            }
            fields.push(Field::new("hero.link", p.link + LINK_FIRST, LINK_WATCHED));
            fields.push(Field::new(
                "hero.ground",
                p.ground + GROUND_FIRST,
                GROUND_WATCHED,
            ));
        }
        for f in &followers {
            fields.push(Field::new(
                f.field(),
                f.placement + actors::PLACEMENT_MATRIX as u32,
                24,
            ));
        }
        for (i, clip) in clips.iter().enumerate() {
            fields.push(Field::new(
                format!("anim.control.{i}"),
                clip.control + CONTROL_FIRST,
                (CONTROL_LEN as u32) - CONTROL_FIRST,
            ));
            fields.push(Field::new(
                format!("anim.sequence.{i}"),
                clip.sequence + SEQUENCE_FIRST,
                0x1C,
            ));
        }
        Ok(Self {
            list: WatchList::new(fields)?,
            port: Self::PORT,
            application: app,
            logic_scale,
            hero_corpus: hero.corpus,
            hero_placement: hero.placement,
            hero_name: hero.name,
            controller,
            pad0,
            parameters,
            viewport_system,
            viewport,
            animation,
            slots,
            clips,
            proxy,
            followers,
        })
    }

    /// One record decoded into a [`Frame`], `pad` the readback joined for its
    /// flip. Fails when a field the frame can't go without (the application,
    /// the scale, the hero's placement, corpus or pad) wasn't readable; the
    /// camera and the clips go missing quietly, the clips also when the
    /// vector's slots aren't the resolved controls any more.
    pub fn frame(&self, record: &WatchRecord, pad: Option<Ds3Pad>) -> Result<Frame> {
        let need = |name: &str| {
            self.list
                .field(record, name)
                .with_context(|| format!("{name} wasn't readable at flip {}", record.flip))
        };
        let app = need("app")?;
        let scale = need("logic_scale")?;
        let placement = need("hero.placement")?;
        let corpus = need("hero.corpus")?;
        let params = need("hero.parameters")?;
        // The stick as pad 0's controller holds it, the hero's own without one.
        let pads = match self.list.field(record, "pad0") {
            Some(p) => p,
            None => need("hero.pad")?,
        };
        let clock = Clock {
            frame_delta: f32_at(app, 0x10),
            presentation_rate: f32_at(app, 0x0C),
            logic_scale: f32_at(scale, 0),
            logic_stopped: scale[4] != 0,
            object_scale: f32_at(params, 0x48 - PARAMS_FIRST as usize),
        };
        let velocity_at = actors::CORPUS_VELOCITY - CORPUS_FIRST as usize;
        let camera = match (
            self.list.field(record, "camera.placement"),
            self.list.field(record, "camera.fov"),
            self.list.field(record, "camera.far"),
        ) {
            (Some(p), Some(fov), Some(far)) => Some(View {
                position: floats(p, 0),
                rotation: floats(p, 12),
                fov: f32_at(fov, 0),
                far_clip: f32_at(far, 0),
            }),
            _ => None,
        };
        Ok(Frame {
            flip: record.flip,
            read: u32::try_from(record.read)
                .with_context(|| format!("read {} at flip {}", record.read, record.flip))?,
            host_us: record.host_us,
            game_frame: be32(app, 0x08),
            clock,
            hero: Body {
                position: floats(placement, 0),
                rotation: floats(placement, 12),
                velocity: floats(corpus, velocity_at),
            },
            camera,
            stick: STICK.map(|at| f32_at(pads, at)),
            pad,
            clips: self.clips(record).unwrap_or_default(),
            contacts: self.contacts(record),
        })
    }

    /// The hero's contacts in a record: none without a proxy in the list or
    /// with its range or the link's unreadable (an unread support word would
    /// pass for a frame without support); no planes when the proxy names
    /// another manifold array than the resolved one (it grew and moved).
    pub fn contacts(&self, record: &WatchRecord) -> Option<Contacts> {
        let watch = self.proxy.as_ref()?;
        let p = self.list.field(record, "hero.proxy")?;
        let at = |offset: usize| offset - PROXY_FIRST as usize;
        let link = self.list.field(record, "hero.link")?;
        let planes = match self.list.field(record, "hero.manifold") {
            Some(m) if be32(p, at(PROXY_MANIFOLD)) == watch.manifold => (0..watch.capacity
                as usize)
                .map(|k| {
                    let e = &m[ROOT_CD_POINT_LEN as usize * k..];
                    Plane {
                        position: floats(e, 0),
                        position_w: f32_at(e, 0x0C),
                        normal: floats(e, 0x10),
                        distance: f32_at(e, 0x1C),
                        collider: be32(e, 0x28),
                    }
                })
                .collect(),
            _ => Vec::new(),
        };
        let flags = std::array::from_fn(|k| p[at(PROXY_FLAGS) + k]);
        Some(Contacts {
            velocity: floats(p, at(PROXY_VELOCITY)),
            support: be32(link, 0),
            surface: floats(link, 0x10),
            flags,
            ground: self.list.field(record, "hero.ground").map(|g| Ground {
                location: floats(g, 0),
                magnitude: f32_at(g, 0x0C),
                normal: floats(g, 0x10),
                context: be32(g, 0x1C),
                flags: be32(g, 0x20),
            }),
            planes,
        })
    }

    /// Each follower's place in a record (AG.13), in the resolve's order; one
    /// whose range wasn't readable is left out.
    pub fn followers(&self, record: &WatchRecord) -> Vec<FollowerAt> {
        self.followers
            .iter()
            .filter_map(|&follower| {
                let p = self.list.field(record, &follower.field())?;
                Some(FollowerAt {
                    follower,
                    position: floats(p, 0),
                    rotation: floats(p, 12),
                })
            })
            .collect()
    }

    /// The hero's `_timeStamp` in a record, the game's own clock on him.
    pub fn time_stamp(&self, record: &WatchRecord) -> Option<f32> {
        let corpus = self.list.field(record, "hero.corpus")?;
        Some(f32_at(corpus, CORPUS_TIME_STAMP - CORPUS_FIRST as usize))
    }

    /// The camera viewport the system names in a record, to tell a switch.
    pub fn current_viewport(&self, record: &WatchRecord) -> Option<u32> {
        self.list
            .field(record, "camera.current")
            .map(|b| be32(b, 0))
    }

    fn clips(&self, record: &WatchRecord) -> Option<Vec<Clip>> {
        let playing = self.list.field(record, "anim.playing")?;
        let slots = self.list.field(record, "anim.slots")?;
        let count = be32(playing, 0) as usize;
        if count != self.clips.len()
            || be32(playing, 8) != self.slots
            || (0..count).any(|i| be32(slots, 4 * i) != self.clips[i].control)
        {
            return None;
        }
        self.clips
            .iter()
            .enumerate()
            .map(|(i, _)| {
                let ctl = self.list.field(record, &format!("anim.control.{i}"))?;
                let seq = self.list.field(record, &format!("anim.sequence.{i}"))?;
                Some(Clip {
                    id: be32(ctl, 0x0C - CONTROL_FIRST as usize),
                    weight: f32_at(seq, 0x38 - SEQUENCE_FIRST as usize),
                    rate: f32_at(seq, 0),
                    period: f32_at(ctl, 0x18 - CONTROL_FIRST as usize),
                    remaining: f32_at(ctl, 0x14 - CONTROL_FIRST as usize),
                })
            })
            .collect()
    }

    /// Every record decoded, `pads` the readback by flip (both records of a
    /// flip that ran two game frames read the flip's scheduled pad).
    pub fn recording(
        &self,
        records: &[WatchRecord],
        pads: &BTreeMap<u64, Ds3Pad>,
    ) -> Result<Recording> {
        let frames = records
            .iter()
            .map(|r| self.frame(r, pads.get(&r.flip).copied()))
            .collect::<Result<Vec<_>>>()?;
        Ok(Recording::new(frames)?)
    }
}

/// The hero's proxy chain from his corpus (AG.9): none when his animation
/// block holds no phantom; refused when a pointer on the way isn't its
/// class's, the phantom has no proxy property, or the proxy or link don't
/// point back at him.
fn proxy_watch(
    g: &mut impl Gather,
    frame: &actors::ActorFrame,
    corpus_at: u32,
    corpus: &[u8],
    block: &[u8],
) -> Result<Option<ProxyWatch>> {
    let phantom = be32(block, COMPONENT_PHANTOM);
    if phantom == 0 {
        return Ok(None);
    }
    let ph = one(g, phantom, PHANTOM_LEN, "the hero's phantom")?;
    expect_vtable(
        &ph,
        crate::level::vtable::SHAPE_PHANTOM,
        phantom,
        "tfbShapePhanton",
    )?;
    let (props_at, count) = (
        be32(&ph, PHANTOM_PROPERTIES),
        be32(&ph, PHANTOM_PROPERTIES + 4),
    );
    if count == 0 || count > MAX_PROPERTIES {
        bail!("the hero's phantom at {phantom:#010x} lists {count} properties");
    }
    let props = one(
        g,
        props_at,
        PROPERTY_LEN * count as usize,
        "the hero's phantom's properties",
    )?;
    let Some(proxy) = props
        .chunks(PROPERTY_LEN)
        .find(|p| be32(p, 0) == PROPERTY_PROXY)
        .map(|p| be32(p, 0x0C))
    else {
        bail!("the hero's phantom at {phantom:#010x} has no proxy property ({PROPERTY_PROXY:#x})");
    };
    let px = one(g, proxy, PROXY_LEN, "the hero's character proxy")?;
    expect_vtable(&px, vtable::CHARACTER_PROXY, proxy, "tfbCharacterProxy")?;
    if be32(&px, PROXY_CORPUS) != corpus_at || be32(&px, PROXY_PHANTOM) != phantom {
        bail!("the character proxy at {proxy:#010x} isn't the hero's");
    }
    let manifold = be32(&px, PROXY_MANIFOLD);
    let capacity = be32(&px, PROXY_MANIFOLD + 8) & HK_ARRAY_CAPACITY_MASK;
    if capacity > MAX_MANIFOLD || (capacity > 0 && !object_ptr(manifold)) {
        bail!("the hero's manifold at {manifold:#010x} holds {capacity} entries");
    }
    let link = be32(&px, PROXY_LINK);
    if be32(
        &one(g, link, LINK_LEN, "the hero's physics link")?,
        LINK_PROXY,
    ) != proxy
    {
        bail!("the physics link at {link:#010x} isn't the proxy's");
    }
    let ground = be32(corpus, CORPUS_GROUND);
    let gi = one(g, ground, CONTACT_INFO_LEN, "the hero's ground")?;
    expect_vtable(&gi, vtable::CONTACT_INFO, ground, "ContactInfo")?;
    let mut colliders = BTreeMap::new();
    for s in crate::level::solids(g, frame)? {
        colliders.insert(
            s.phantom + PHANTOM_COLLIDABLE,
            s.name.unwrap_or_else(|| format!("{:#010x}", s.corpus)),
        );
    }
    colliders.insert(
        crate::level::level_body_address(g)? + PHANTOM_COLLIDABLE,
        "level".to_owned(),
    );
    Ok(Some(ProxyWatch {
        phantom,
        proxy,
        link,
        manifold,
        capacity,
        ground,
        colliders,
    }))
}

/// The `tfbViewportSystem` among the insight core's systems, if there's one.
fn viewport_system(g: &mut impl Gather, app: &[u8]) -> Result<Option<u32>> {
    let insight = be32(app, actors::APP_INSIGHT);
    let core = one(g, insight, actors::INSIGHT_LEN, "the insight core")?;
    expect_vtable(&core, vtable::INSIGHT_CORE, insight, "igInsightCore")?;
    let systems = actors::list(
        g,
        be32(&core, actors::INSIGHT_SYSTEMS),
        vtable::SYSTEM_LIST,
        "igInsightSystemList",
    )?;
    let ranges: Vec<(u32, usize)> = systems
        .iter()
        .filter(|&&p| object_ptr(p))
        .map(|&p| (p, 4))
        .collect();
    let heads = g.gather(&ranges)?;
    Ok(ranges
        .iter()
        .zip(heads)
        .find(|(_, b)| {
            b.as_ref()
                .is_some_and(|b| be32(b, 0) == vtable::VIEWPORT_SYSTEM)
        })
        .map(|((p, _), _)| *p))
}

/// What a watched run writes beside its other outputs (`watch.json`): the
/// resolved list and every record raw, so a later box decodes more of it
/// without running the oracle again. Guest bytes: only outside a work tree.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WatchFile {
    pub identity: String,
    pub watch: GiantsWatch,
    pub held: u64,
    pub total: u64,
    pub frames: u32,
    pub armed: u64,
    pub records: Vec<RawRecord>,
    /// O4's readback on the hero's pad by flip, where the guest read one.
    pub pads: BTreeMap<u64, Ds3Pad>,
    /// O4's count of the guest's reads of that pad by flip (since AG.35).
    #[serde(default)]
    pub reads: BTreeMap<u64, u32>,
}

/// A record as it came back, its payload in hex.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RawRecord {
    pub flip: u64,
    pub host_us: u64,
    pub missing: u64,
    /// Which of the port's reads in the flip (0 in a file from before AG.35).
    #[serde(default)]
    pub read: u64,
    pub payload: String,
}

impl RawRecord {
    pub fn of(r: &WatchRecord) -> Self {
        Self {
            flip: r.flip,
            host_us: r.host_us,
            missing: r.missing,
            read: r.read,
            payload: hex(&r.payload),
        }
    }

    pub fn record(&self) -> Result<WatchRecord> {
        let payload = crate::verbs::unhex(&self.payload)
            .with_context(|| format!("the payload at flip {} isn't lower-case hex", self.flip))?;
        Ok(WatchRecord {
            flip: self.flip,
            host_us: self.host_us,
            missing: self.missing,
            read: self.read,
            payload,
        })
    }
}

impl WatchFile {
    pub const FILE: &'static str = "watch.json";

    pub fn write(&self, path: &Path) -> Result<()> {
        let path = rrc_dump::output::refuse_repo_output(path)?;
        std::fs::write(
            &path,
            serde_json::to_string(self).expect("a watch serialises"),
        )
        .with_context(|| format!("writing {}", path.display()))
    }

    pub fn load(path: &Path) -> Result<Self> {
        let text =
            std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
        serde_json::from_str(&text).with_context(|| format!("parsing {}", path.display()))
    }

    /// The recording a later box reads again from the file alone, no oracle.
    pub fn recording(&self) -> Result<Recording> {
        let records = self
            .records
            .iter()
            .map(RawRecord::record)
            .collect::<Result<Vec<_>>>()?;
        self.watch.recording(&records, &self.pads)
    }
}

/// What a ring says about the run it recorded, the checks AG.1's and AG.35's
/// verifies make: every read of the port recorded, every range readable, and
/// `_curFrame` stepping by one a record, so no game frame is lost or counted
/// twice whatever the flips did.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RingCheck {
    pub records: usize,
    pub first_flip: Option<u64>,
    pub last_flip: Option<u64>,
    /// Consecutive records whose flips are more than one apart: a flip the
    /// port wasn't read in, which loses nothing when the game frames go on by
    /// one.
    pub flip_gaps: usize,
    /// Flips holding more than one record: the port read in them twice or
    /// more, a game frame each in Giants.
    #[serde(default)]
    pub shared_flips: usize,
    /// Records past their flip's first read: the game frames a record at each
    /// flip's first read alone (O5 before AG.35) would have lost.
    #[serde(default)]
    pub later_reads: usize,
    /// Consecutive records whose `_curFrame` isn't one apart: a lost game frame
    /// when it jumps, the port read twice in one frame when it repeats.
    pub frame_gaps: usize,
    /// Of those, the ones whose `_curFrame` repeats.
    #[serde(default)]
    pub frame_repeats: usize,
    /// Records with a range that wasn't readable.
    pub missing: usize,
    /// `_curFrame` minus the flip, first and last: constant when the game
    /// counts a frame a flip.
    pub frame_minus_flip: Option<(i64, i64)>,
    /// Flips whose records don't number O4's count of the port's reads in
    /// them, a flip with reads and no record among them; `None` without the
    /// counts. Judged over whole flips (a wrapped ring's first flip may have
    /// lost its earlier reads) inside the [`PAD_HISTORY`] flips O4 keeps.
    #[serde(default)]
    pub reads_mismatch: Option<usize>,
}

/// Flips of reads O4 keeps a port (`PAD_STATE <port> <flip>` answers `no_read`
/// past them).
pub const PAD_HISTORY: u64 = 4_096;

impl RingCheck {
    /// The checks over a decoded recording, its raw records and, when joined,
    /// O4's count of the port's reads by flip.
    pub fn of(recording: &Recording, raw: &[WatchRecord], reads: &BTreeMap<u64, u32>) -> Self {
        let frames = recording.frames();
        let pairs = || frames.windows(2);
        let diff = |f: &Frame| i64::from(f.game_frame) - f.flip as i64;
        let mut per_flip: BTreeMap<u64, u32> = BTreeMap::new();
        for f in frames {
            *per_flip.entry(f.flip).or_default() += 1;
        }
        let reads_mismatch = match (
            frames.iter().find(|f| f.read == 0),
            frames.last(),
            reads.is_empty(),
        ) {
            (Some(first), Some(last), false) => {
                let judged =
                    first.flip.max((last.flip + 1).saturating_sub(PAD_HISTORY))..=last.flip;
                let counted = reads
                    .range(judged.clone())
                    .filter(|&(flip, n)| per_flip.get(flip) != Some(n))
                    .count();
                let uncounted = per_flip
                    .range(judged)
                    .filter(|&(flip, _)| !reads.contains_key(flip))
                    .count();
                Some(counted + uncounted)
            }
            _ => None,
        };
        Self {
            records: frames.len(),
            first_flip: frames.first().map(|f| f.flip),
            last_flip: frames.last().map(|f| f.flip),
            flip_gaps: pairs().filter(|w| w[1].flip > w[0].flip + 1).count(),
            shared_flips: per_flip.values().filter(|&&n| n > 1).count(),
            later_reads: frames.iter().filter(|f| f.read > 0).count(),
            frame_gaps: pairs()
                .filter(|w| w[1].game_frame != w[0].game_frame.wrapping_add(1))
                .count(),
            frame_repeats: pairs()
                .filter(|w| w[1].game_frame == w[0].game_frame)
                .count(),
            missing: raw.iter().filter(|r| r.missing != 0).count(),
            frame_minus_flip: frames
                .first()
                .zip(frames.last())
                .map(|(a, b)| (diff(a), diff(b))),
            reads_mismatch,
        }
    }

    /// Every game frame once, every range readable, and where O4's counts were
    /// joined, every read of the port recorded. Flips may skip or hold two.
    pub fn holds(&self) -> bool {
        self.records > 0
            && self.frame_gaps == 0
            && self.missing == 0
            && self.reads_mismatch.unwrap_or(0) == 0
    }
}

/// Whether a verb failed because O5 (or O3, O4) refused it for that reason,
/// rather than on the socket.
pub fn is_refusal(err: &anyhow::Error, refusal: Refusal) -> bool {
    err.downcast_ref::<ReplyError>()
        .and_then(ReplyError::refusal)
        == Some(refusal)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn record(flip: u64, missing: u64, payload: Vec<u8>) -> WatchRecord {
        WatchRecord {
            flip,
            host_us: flip * 16_667,
            missing,
            read: 0,
            payload,
        }
    }

    #[test]
    fn overlapping_and_touching_fields_share_a_range() {
        let list = WatchList::new(vec![
            Field::new("b", 0x1010, 8),
            Field::new("a", 0x1000, 0x14),
            Field::new("inside", 0x1004, 4),
            Field::new("touch", 0x1018, 4),
            Field::new("apart", 0x2000, 2),
            Field::new("tail", 0x1FFE, 2),
        ])
        .unwrap();
        // 0x1000..0x1018 (a, b, inside), touched by 0x1018..0x101C, then 0x1FFE..0x2002.
        assert_eq!(
            list.ranges(),
            [
                WatchRange {
                    addr: 0x1000,
                    len: 0x1C
                },
                WatchRange {
                    addr: 0x1FFE,
                    len: 4
                }
            ]
        );
        assert_eq!(list.bytes(), 0x20);
        let at = |n: &str| list.placed(n).unwrap();
        assert_eq!((at("a").range, at("a").offset), (0, 0));
        assert_eq!(at("b").offset, 0x10);
        assert_eq!(at("inside").offset, 4);
        assert_eq!(at("touch").offset, 0x18);
        assert_eq!((at("tail").range, at("tail").offset), (1, 0x1C));
        assert_eq!((at("apart").range, at("apart").offset), (1, 0x1E));
        // A field's bytes come back from where its range put it.
        let payload: Vec<u8> = (0..0x20).collect();
        let r = record(5, 0, payload);
        assert_eq!(
            list.field(&r, "b"),
            Some(&[16u8, 17, 18, 19, 20, 21, 22, 23][..])
        );
        assert_eq!(list.field(&r, "apart"), Some(&[30u8, 31][..]));
        // A range O5 couldn't read leaves its fields out, the others in.
        let unread = record(6, 0b10, (0..0x20).collect());
        assert!(list.field(&unread, "tail").is_none());
        assert!(list.field(&unread, "a").is_some());
        assert!(list.field(&r, "nothing").is_none());
        assert!(list.field(&record(7, 0, vec![0; 3]), "a").is_none());
        assert_eq!(list.arm(600, 0).encode(), "WATCH 600 0 1000 28 1ffe 4\n");
    }

    #[test]
    fn a_list_o5_wouldnt_take_is_refused() {
        assert!(WatchList::new(Vec::new()).is_err());
        assert!(WatchList::new(vec![Field::new("z", 0x1000, 0)]).is_err());
        assert!(WatchList::new(vec![Field::new("w", 0xFFFF_FFF0, 0x20)]).is_err());
        assert!(
            WatchList::new(vec![Field::new("x", 0x1000, 4), Field::new("x", 0x2000, 4)]).is_err()
        );
        let many: Vec<Field> = (0..65)
            .map(|i| Field::new(format!("f{i}"), 0x1000 + 0x100 * i, 4))
            .collect();
        assert!(WatchList::new(many[..64].to_vec()).is_ok());
        assert!(WatchList::new(many).is_err());
        assert!(WatchList::new(vec![Field::new("big", 0x1000, WATCH_MAX_BYTES)]).is_ok());
        assert!(WatchList::new(vec![Field::new("big", 0x1000, WATCH_MAX_BYTES + 1)]).is_err());
        // The last byte of the address space is a range; one past it isn't.
        assert!(WatchList::new(vec![Field::new("end", 0xFFFF_FFFF, 1)]).is_ok());
    }
    use crate::actors::fixture::{Added, Spec, World};
    use guac_oracle::recording::{Alignment, Order};

    /// AF.8's fresh state as far as the watch goes: Eruptor (skinned, a pad's)
    /// and a sheep, the camera, the time scales, his parameters and three
    /// clips, idle at the top of the blend.
    struct Giants {
        w: World,
        hero: Added,
        /// Pad 0's controller, the follow actor's.
        pad0: u32,
        controls: Vec<u32>,
    }

    const CLIPS: [(&str, u32, f32, f32); 3] = [
        (
            "Models/Characters/MinionsMonsters/Eruptor/Eruptor_Idle.ma",
            0x3090_C890,
            100.0,
            1.333_333_4,
        ),
        (
            "Models/Characters/MinionsMonsters/Eruptor/Eruptor_Walk.ma",
            0x3090_B960,
            0.0,
            1.066_666_7,
        ),
        (
            "Models/Characters/MinionsMonsters/Eruptor/Eruptor_Run.ma",
            0x3090_B980,
            0.0,
            0.666_666_7,
        ),
    ];

    fn giants() -> Giants {
        let mut w = World::new();
        w.camera([6.5107, 52.2663, -107.1263], [-23.3597, 14.9997, 0.0]);
        let mut world = [0.0f32; 16];
        for i in 0..4 {
            world[5 * i] = 1.0;
        }
        let hero = w.actor(&Spec {
            name: "009_Eruptor".into(),
            position: [10.19027, 43.38267, -93.39406],
            scale: [150.0; 3],
            world: Some(world),
            skinned: true,
            pad: Some(-1),
            ..Spec::default()
        });
        w.actor(&Spec {
            name: "Sheep_1".into(),
            skinned: true,
            ..Spec::default()
        });
        let follow = w.actor(&Spec {
            name: "Generic_FollowActor".into(),
            pad: Some(0),
            ..Spec::default()
        });
        let pad0 = w.get32(follow.placement + 0xD8);
        w.putf(w.app + 0x60, 1.0 / 60.0);
        let message = w.object(vtable::LOGIC_SCALE_MESSAGE, 0x10);
        w.putf(message + 0x08, 1.0);
        let app = w.app;
        w.put32(app + 0x10, message);
        let params = w.object(vtable::ACTOR_PARAMETERS, 0x4C);
        w.put_floats(params + 0x0C, &[21.5, 160.0, 80.0, 1440.0]);
        w.putf(params + 0x48, 1.0);
        w.put32(hero.placement + 0xE0, params);
        let block = w.object(vtable::ANIMATION_COMPONENT, 0x28);
        w.put32(hero.corpus + 0x7C, block);
        let slots = w.alloc(32);
        w.put32(block + 0x08, CLIPS.len() as u32);
        w.put32(block + 0x0C, 0x1000_0020);
        w.put32(block + 0x10, slots);
        let mut controls = Vec::new();
        for (i, (name, id, weight, period)) in CLIPS.iter().enumerate() {
            let animation = w.object(vtable::PHYSICS_ANIMATION, 0x2C);
            let at = w.string(name);
            w.put32(animation + 0x08, at);
            w.put32(animation + 0x28, *id);
            let sequence = w.object(vtable::ANIMATION_INFO, 0x44);
            w.put32(sequence + 0x3C, animation);
            w.putf(sequence + 0x20, 100.0);
            w.putf(sequence + 0x38, *weight);
            let control = w.object(vtable::ANIM_CONTROL, 0x2C);
            w.put32(control + 0x08, sequence);
            w.put32(control + 0x0C, *id);
            w.putf(control + 0x14, *period);
            w.putf(control + 0x18, *period);
            w.put32(slots + 4 * i as u32, control);
            controls.push(control);
        }
        Giants {
            w,
            hero,
            pad0,
            controls,
        }
    }

    /// Where [`with_proxy`] put the hero's contacts.
    struct ProxyFixture {
        phantom: u32,
        proxy: u32,
        link: u32,
        manifold: u32,
        ground: u32,
        body: u32,
        door: u32,
    }

    /// The hero's proxy chain as AF.8's fresh state holds it, pressed against
    /// the gate's door: his phantom in his animation block, its properties
    /// naming his proxy, the proxy's manifold (two entries, the door's face
    /// as a start point and as the cast's hit), its link, his ground, and the
    /// world holding his phantom, the door's and the level's static body.
    fn with_proxy(g: &mut Giants) -> ProxyFixture {
        let w = &mut g.w;
        let sphere = w.object(crate::level::vtable::SPHERE_SHAPE, 0x50);
        w.putf(sphere + 0x10, 0.8144);
        let phantom = |w: &mut World, corpus: u32| {
            let block = w.get32(corpus + 0x7C);
            let block = if block == 0 {
                let b = w.object(vtable::ANIMATION_COMPONENT, 0x28);
                w.put32(corpus + 0x7C, b);
                b
            } else {
                block
            };
            let ph = w.object(crate::level::vtable::SHAPE_PHANTOM, 0xE0);
            w.put32(block + 0x1C, ph);
            w.put32(ph + 0x10, sphere);
            ph
        };
        let hero_phantom = phantom(w, g.hero.corpus);
        let door = w.actor(&Spec {
            name: "Breakable_Stone_Wall_Castle_206(1)".into(),
            ..Spec::default()
        });
        let door_phantom = phantom(w, door.corpus);
        // The world: its fixed island holding the level's body, and the two
        // phantoms listed.
        let world = w.object(crate::level::vtable::WORLD, 0x128);
        w.put32(crate::level::HKP_WORLD, world);
        let island = w.object(crate::level::vtable::SIMULATION_ISLAND, 0x58);
        w.put32(world + 0x20, island);
        w.put32(island + 0x18, world);
        let body = w.object(crate::level::vtable::RIGID_BODY, 0x130);
        w.put32(body + 0x08, world);
        let query = w.object(crate::level::vtable::TRIANGLE_COLLISION_QUERY, 0x38);
        w.put32(body + 0x10, query);
        let entities = w.alloc(4);
        w.put32(entities, body);
        w.put32(island + 0x4C, entities);
        w.put32(island + 0x50, 1);
        let listed = w.alloc(8);
        w.put32(listed, hero_phantom);
        w.put32(listed + 4, door_phantom);
        w.put32(world + 0x11C, listed);
        w.put32(world + 0x120, 2);
        for ph in [hero_phantom, door_phantom] {
            w.put32(ph + 0x08, world);
        }
        // The hero's phantom's properties, the proxy among them.
        let proxy = w.object(vtable::CHARACTER_PROXY, 0x60);
        let props = w.alloc(0x30);
        for (k, (key, value)) in [(0x200D, 0x3000_0040), (0x2003, proxy), (0x2004, 0)]
            .into_iter()
            .enumerate()
        {
            w.put32(props + 0x10 * k as u32, key);
            w.put32(props + 0x10 * k as u32 + 0x0C, value);
        }
        w.put32(hero_phantom + 0x7C, props);
        w.put32(hero_phantom + 0x80, 3);
        // The proxy: his corpus, a velocity along the door, his phantom, the
        // manifold, the link, a flag.
        w.put32(proxy + 0x14, g.hero.corpus);
        w.put_floats(proxy + 0x20, &[-6.4850, 0.0, 0.0]);
        w.put32(proxy + 0x30, hero_phantom);
        let manifold = w.alloc(0x60);
        w.put32(proxy + 0x40, manifold);
        w.put32(proxy + 0x48, 2);
        let link = w.alloc(0x40);
        w.put32(link + 0x10, proxy);
        w.put32(proxy + 0x4C, link);
        w.put32(proxy + 0x50, 0x0000_0001);
        w.put32(link + 0x20, 2);
        w.put_floats(link + 0x30, &[0.0179, 0.9998, -0.0010]);
        for (k, w_) in [(0u32, 1.0f32), (1, 0.0)] {
            let e = manifold + 0x30 * k;
            w.put_floats(e, &[14.1131, 44.3155, -82.8338, w_]);
            w.put_floats(
                e + 0x10,
                &[0.0, 0.0, -1.0, if k == 0 { 0.0018 } else { 0.0 }],
            );
            w.put32(e + 0x20, hero_phantom + 0x10);
            w.put32(e + 0x28, door_phantom + 0x10);
        }
        // His ground.
        let ground = w.object(vtable::CONTACT_INFO, 0x44);
        w.put32(g.hero.corpus + 0x64, ground);
        w.put_floats(ground + 0x1C, &[13.7756, 43.3971, -83.7026, 43.4071]);
        w.put_floats(ground + 0x2C, &[0.0179, 0.9998, -0.0010]);
        w.put32(ground + 0x3C, 1);
        ProxyFixture {
            phantom: hero_phantom,
            proxy,
            link,
            manifold,
            ground,
            body,
            door: door_phantom,
        }
    }

    #[test]
    fn the_heros_contacts_resolve_through_his_phantom_and_name_their_colliders() {
        let mut g = giants();
        let fx = with_proxy(&mut g);
        let watch = GiantsWatch::resolve(&mut g.w).unwrap();
        let p = watch.proxy.as_ref().unwrap();
        assert_eq!(
            (p.phantom, p.proxy, p.link, p.manifold, p.capacity, p.ground),
            (fx.phantom, fx.proxy, fx.link, fx.manifold, 2, fx.ground)
        );
        assert_eq!(
            p.collider_name(fx.door + 0x10),
            "Breakable_Stone_Wall_Castle_206(1)"
        );
        assert_eq!(p.collider_name(fx.phantom + 0x10), "009_Eruptor");
        assert_eq!(p.collider_name(fx.body + 0x10), "level");
        assert_eq!(p.collider_name(0x3000_0004), "0x30000004");
        assert!(watch.list.bytes() < 1_200, "{}", watch.list.bytes());
        // A record decodes the proxy's state as the step left it: the count
        // word is 0 (the game zeroes it), and both entries come back.
        let full = sample(&mut g.w, &watch.list, 278);
        let c = watch.frame(&full, None).unwrap().contacts.unwrap();
        assert_eq!(c.velocity, [-6.4850, 0.0, 0.0]);
        assert_eq!((c.support, c.flags), (2, [0, 0, 0, 1]));
        assert_eq!(c.surface, [0.0179, 0.9998, -0.0010]);
        let ground = c.ground.unwrap();
        assert_eq!(
            (ground.location, ground.magnitude, ground.flags),
            ([13.7756, 43.3971, -83.7026], 43.4071, 1)
        );
        assert_eq!(c.planes.len(), 2);
        assert_eq!(c.planes[0].normal, [0.0, 0.0, -1.0]);
        assert_eq!(
            (c.planes[0].distance, c.planes[0].position_w),
            (0.0018, 1.0)
        );
        assert_eq!(c.planes[1].position_w, 0.0);
        assert!(c.planes.iter().all(|e| e.collider == fx.door + 0x10));
        // The array grown and moved: the proxy names other data, so no planes.
        g.w.put32(fx.proxy + 0x40, 0x3000_0000);
        let moved = sample(&mut g.w, &watch.list, 279);
        let c = watch.frame(&moved, None).unwrap().contacts.unwrap();
        assert!(c.planes.is_empty());
        assert_eq!(c.support, 2);
        // A list without the proxy (one written before AG.9) has no contacts.
        let mut old = watch.clone();
        old.proxy = None;
        assert!(old.frame(&full, None).unwrap().contacts.is_none());
        // The proxy's range unreadable: no contacts, the frame still decodes.
        let mut unread = full.clone();
        unread.missing = 1 << watch.list.placed("hero.proxy").unwrap().range;
        assert!(watch.frame(&unread, None).unwrap().contacts.is_none());
        // The link's alone: no contacts either, rather than a support word of
        // 0 that reads as a frame without support.
        let link_range = watch.list.placed("hero.link").unwrap().range;
        assert_ne!(link_range, watch.list.placed("hero.proxy").unwrap().range);
        let mut unread = full.clone();
        unread.missing = 1 << link_range;
        assert!(watch.frame(&unread, None).unwrap().contacts.is_none());
    }

    #[test]
    fn a_proxy_chain_off_its_layout_is_refused() {
        let msg = |g: &mut Giants| format!("{:#}", GiantsWatch::resolve(&mut g.w).unwrap_err());
        // The proxy's vtable isn't its class's.
        let mut g = giants();
        let fx = with_proxy(&mut g);
        g.w.put32(fx.proxy, vtable::ANIMATION_INFO);
        assert!(msg(&mut g).contains("tfbCharacterProxy"), "{}", msg(&mut g));
        // The proxy isn't the hero's.
        let mut g = giants();
        let fx = with_proxy(&mut g);
        g.w.put32(fx.proxy + 0x14, 0x3000_0000);
        assert!(msg(&mut g).contains("isn't the hero's"), "{}", msg(&mut g));
        // The phantom names no proxy.
        let mut g = giants();
        let fx = with_proxy(&mut g);
        let props = g.w.get32(fx.phantom + 0x7C);
        g.w.put32(props + 0x10, 0x2005);
        assert!(msg(&mut g).contains("no proxy property"), "{}", msg(&mut g));
        // The link doesn't point back at the proxy.
        let mut g = giants();
        let fx = with_proxy(&mut g);
        g.w.put32(fx.link + 0x10, 0);
        assert!(msg(&mut g).contains("physics link"), "{}", msg(&mut g));
        // The ground isn't a ContactInfo.
        let mut g = giants();
        let fx = with_proxy(&mut g);
        g.w.put32(fx.ground, vtable::ANIMATION_INFO);
        assert!(msg(&mut g).contains("ContactInfo"), "{}", msg(&mut g));
        // A manifold past what the watch takes.
        let mut g = giants();
        let fx = with_proxy(&mut g);
        g.w.put32(fx.proxy + 0x48, 0x8000_0000 | 17);
        assert!(msg(&mut g).contains("17 entries"), "{}", msg(&mut g));
        // No proxy chain at all: a hero whose block holds no phantom.
        let mut g = giants();
        let fx = with_proxy(&mut g);
        let block = g.w.get32(g.hero.corpus + 0x7C);
        g.w.put32(block + 0x1C, 0);
        let _ = fx;
        assert!(GiantsWatch::resolve(&mut g.w).unwrap().proxy.is_none());
    }

    /// What O5 copies at a pad read: each range's bytes back to back.
    fn sample(w: &mut World, list: &WatchList, flip: u64) -> WatchRecord {
        let ranges: Vec<(u32, usize)> = list
            .ranges()
            .iter()
            .map(|r| (r.addr, r.len as usize))
            .collect();
        let payload = w
            .gather(&ranges)
            .unwrap()
            .into_iter()
            .flat_map(|b| b.unwrap())
            .collect();
        record(flip, 0, payload)
    }

    #[test]
    fn giants_list_resolves_from_the_frame_and_names_each_clip() {
        let mut g = giants();
        let watch = GiantsWatch::resolve(&mut g.w).unwrap();
        assert_eq!(watch.hero_corpus, g.hero.corpus);
        assert_eq!(watch.hero_placement, g.hero.placement);
        assert_eq!(watch.hero_name.as_deref(), Some("009_Eruptor"));
        assert!(watch.viewport.is_some() && watch.viewport_system.is_some());
        let names: Vec<_> = watch
            .clips
            .iter()
            .map(|c| c.name.clone().unwrap())
            .collect();
        assert_eq!(names, CLIPS.map(|c| c.0.to_owned()));
        assert_eq!(watch.clips[1].id, 0x3090_B960);
        assert_eq!(
            watch.clips.iter().map(|c| c.control).collect::<Vec<_>>(),
            g.controls
        );
        // Every field is placed, and the list fits O5.
        for f in watch.list.fields() {
            assert!(watch.list.placed(&f.name).is_some(), "{}", f.name);
        }
        assert!(watch.list.ranges().len() <= WATCH_MAX_RANGES);
        assert!(watch.list.bytes() < 1_000, "{}", watch.list.bytes());
        // A pointer that isn't its class's stops the resolve.
        let params = g.w.get32(g.hero.placement + 0xE0);
        g.w.put32(params, vtable::ANIMATION_INFO);
        let err = GiantsWatch::resolve(&mut g.w).unwrap_err();
        assert!(format!("{err:#}").contains("ActorParameters"), "{err:#}");
    }

    #[test]
    fn every_follow_actor_is_watched_and_read_back_by_its_placement() {
        let mut g = giants();
        // The camera's magic point, a second follower beside pad 0's, just
        // off the hero as the fresh state has it (AG.13).
        let magic = g.w.actor(&Spec {
            name: "Generic_FollowActor".into(),
            position: [10.190276, 43.382694, -93.39401],
            ..Spec::default()
        });
        let watch = GiantsWatch::resolve(&mut g.w).unwrap();
        assert_eq!(watch.followers.len(), 2, "{:?}", watch.followers);
        assert!(watch.followers.contains(&Follower {
            placement: magic.placement,
            corpus: magic.corpus,
        }));
        assert!(watch.followers.iter().all(|f| f.corpus != g.hero.corpus));
        for f in &watch.followers {
            assert_eq!(f.field(), format!("follow.{:08x}", f.placement));
            assert!(watch.list.placed(&f.field()).is_some(), "{}", f.field());
        }
        assert!(watch.list.ranges().len() <= WATCH_MAX_RANGES);
        let app = g.w.app;
        g.w.put32(app + 0x5C, 5_962);
        let before = sample(&mut g.w, &watch.list, 100);
        g.w.put32(app + 0x5C, 5_963);
        g.w.put_floats(
            magic.placement + actors::PLACEMENT_MATRIX as u32,
            &[11.0, 43.5, -94.0, -1.0, 15.0, 0.5],
        );
        let after = sample(&mut g.w, &watch.list, 101);
        let at = |r: &WatchRecord| {
            watch
                .followers(r)
                .into_iter()
                .find(|f| f.follower.placement == magic.placement)
                .unwrap()
        };
        assert_eq!(at(&before).position, [10.190276, 43.382694, -93.39401]);
        assert_eq!(at(&after).position, [11.0, 43.5, -94.0]);
        assert_eq!(at(&after).rotation, [-1.0, 15.0, 0.5]);
        // Followers stay out of the frame the recording decodes.
        let pads = BTreeMap::new();
        let r = watch.recording(&[before, after.clone()], &pads).unwrap();
        assert_eq!(r.frames()[1].hero.position, [10.19027, 43.38267, -93.39406]);
        // A list written before AG.13 reads as having none.
        let mut json = serde_json::to_value(&watch).unwrap();
        json.as_object_mut().unwrap().remove("followers");
        let old: GiantsWatch = serde_json::from_value(json).unwrap();
        assert!(old.followers.is_empty() && old.followers(&after).is_empty());
    }

    #[test]
    fn a_synthetic_ring_decodes_to_game_time_summed_from_clamped_steps_a_hitch_included() {
        let mut g = giants();
        let watch = GiantsWatch::resolve(&mut g.w).unwrap();
        let (app, placement, corpus) = (g.w.app, g.hero.placement, g.hero.corpus);
        let controller = g.w.get32(placement + 0xD8);
        // The stick held forward on pad 0's controller, the hero's own at rest.
        g.w.put_floats(g.pad0 + 0x1AC, &[0.0, 1.0]);
        g.w.put_floats(controller + 0x1AC, &[0.0, 0.0]);
        let deltas = [
            0.0167f32, 0.0158, 0.0175, 0.0166, 0.0171, 0.2, 0.0161, 0.0170, 0.0164, 0.0169,
        ];
        let (mut x, mut speed, mut remaining) = (10.19027f32, 0.0f32, CLIPS[0].3);
        let mut records = Vec::new();
        for (n, &delta) in deltas.iter().enumerate() {
            // The frame measures its time, the pads are read (O5 copies),
            // then the update moves the hero by his new velocity.
            g.w.put32(app + 0x5C, 5_960 + n as u32);
            g.w.putf(app + 0x64, delta);
            records.push(sample(&mut g.w, &watch.list, 100 + n as u64));
            let step = Clock {
                frame_delta: delta,
                presentation_rate: 1.0 / 60.0,
                logic_scale: 1.0,
                logic_stopped: false,
                object_scale: 1.0,
            }
            .step();
            speed = (speed + 48.768 * step).min(6.553);
            x += speed * step;
            remaining -= step;
            if remaining <= 0.0 {
                remaining += CLIPS[0].3;
            }
            g.w.putf(placement + 0x3C, x);
            g.w.putf(corpus + 0x58, speed);
            g.w.putf(g.controls[0] + 0x14, remaining);
        }
        let pads = BTreeMap::from([(101, Ds3Pad::REST)]);
        let r = watch.recording(&records, &pads).unwrap();
        let check = RingCheck::of(&r, &records, &BTreeMap::new());
        assert!(check.holds(), "{check:?}");
        assert_eq!(check.reads_mismatch, None);
        assert_eq!(watch.port, GiantsWatch::PORT);
        assert_eq!((check.first_flip, check.last_flip), (Some(100), Some(109)));
        assert_eq!(check.frame_minus_flip, Some((5_860, 5_860)));
        let f = &r.frames()[3];
        assert_eq!(f.stick, [0.0, 1.0]);
        assert_eq!(watch.pad0, Some(g.pad0));
        assert_eq!(f.clock.presentation_rate, 1.0 / 60.0);
        assert_eq!((f.clock.logic_scale, f.clock.object_scale), (1.0, 1.0));
        assert_eq!(f.camera.unwrap().rotation[1], 14.9997);
        assert_eq!(r.frames()[1].pad, Some(Ds3Pad::REST));
        assert_eq!(r.frames()[2].pad, None);
        // The clip at the top is the idle, its time counting the steps.
        let top = f.top_clip().unwrap();
        assert_eq!(top.id, CLIPS[0].1);
        let steps = r.steps();
        let into: f64 = steps[..3].iter().sum();
        assert!(
            (f64::from(top.time()) - into).abs() < 1e-6,
            "{} {into}",
            top.time()
        );
        // Game time sums the clamped steps: the 0.2 s hitch counts 0.05 s.
        let t = r.game_time(Alignment::Earlier).unwrap();
        assert!((t[6] - t[5] - f64::from(1.0f32 / 60.0 * 3.0)).abs() < 1e-12);
        let want: f64 = deltas[..9]
            .iter()
            .map(|&d| f64::from(if d > 0.05 { 1.0f32 / 60.0 * 3.0 } else { d }))
            .sum();
        assert!((t[9] - want).abs() < 1e-9, "{} {want}", t[9]);
        // The walk was made with the earlier record's step and the later one's
        // velocity, and only that pairing fits it.
        let fit = r.residuals(Alignment::Earlier, Order::VelocityFirst, |_| true);
        assert_eq!(fit.pairs, 9);
        assert!(fit.max_ground < 2e-6, "{fit:?}");
        for (a, o) in [
            (Alignment::Later, Order::VelocityFirst),
            (Alignment::Earlier, Order::PositionFirst),
            (Alignment::Later, Order::PositionFirst),
        ] {
            assert!(r.residuals(a, o, |_| true).max_ground > 1e-3, "{a:?} {o:?}");
        }
        // The game's own clock on him reads back, and the camera's viewport.
        assert_eq!(watch.time_stamp(&records[0]), Some(0.0));
        assert_eq!(watch.current_viewport(&records[0]), watch.viewport);
    }

    #[test]
    fn a_flip_running_two_game_frames_holds_both_and_the_check_counts_what_one_a_flip_lost() {
        let mut g = giants();
        let watch = GiantsWatch::resolve(&mut g.w).unwrap();
        let app = g.w.app;
        // Eight game frames read on port 0: flip 103 ran none (a long frame)
        // and flip 104 ran two, its reads 0 and 1 (AG.1's walk-W-03, AG.35).
        let reads_at = [
            (100, 0),
            (101, 0),
            (102, 0),
            (104, 0),
            (104, 1),
            (105, 0),
            (106, 0),
            (107, 0),
        ];
        let mut records = Vec::new();
        for (n, &(flip, read)) in reads_at.iter().enumerate() {
            g.w.put32(app + 0x5C, 5_960 + n as u32);
            g.w.putf(
                app + 0x64,
                if flip == 104 && read == 0 {
                    0.031
                } else {
                    1.0 / 60.0
                },
            );
            let mut r = sample(&mut g.w, &watch.list, flip);
            r.read = read;
            records.push(r);
        }
        let r = watch.recording(&records, &BTreeMap::new()).unwrap();
        let o4: BTreeMap<u64, u32> = [
            (100, 1),
            (101, 1),
            (102, 1),
            (104, 2),
            (105, 1),
            (106, 1),
            (107, 1),
        ]
        .into_iter()
        .collect();
        let check = RingCheck::of(&r, &records, &o4);
        assert!(check.holds(), "{check:?}");
        assert_eq!(
            (
                check.records,
                check.flip_gaps,
                check.shared_flips,
                check.later_reads
            ),
            (8, 1, 1, 1)
        );
        assert_eq!((check.frame_gaps, check.frame_repeats), (0, 0));
        assert_eq!(check.frame_minus_flip, Some((5_860, 5_860)));
        assert_eq!(check.reads_mismatch, Some(0));
        assert_eq!(r.frames()[4].read, 1);
        assert_eq!(r.game_time(Alignment::Earlier).unwrap().len(), 8);
        // O4 counting a read the ring doesn't hold, or a flip with reads and no
        // record, is a mismatch.
        let mut short = o4.clone();
        short.insert(104, 3);
        assert_eq!(RingCheck::of(&r, &records, &short).reads_mismatch, Some(1));
        let mut unrecorded = o4.clone();
        unrecorded.insert(103, 1);
        let check = RingCheck::of(&r, &records, &unrecorded);
        assert_eq!(check.reads_mismatch, Some(1));
        assert!(!check.holds());
        // One record a flip, as O5 took them before AG.35: game frame 5964 lost.
        let firsts: Vec<WatchRecord> = records.iter().filter(|r| r.read == 0).cloned().collect();
        let old = watch.recording(&firsts, &BTreeMap::new()).unwrap();
        let check = RingCheck::of(&old, &firsts, &BTreeMap::new());
        assert!(!check.holds());
        assert_eq!((check.frame_gaps, check.later_reads), (1, 0));
        assert!(matches!(
            old.game_time(Alignment::Earlier),
            Err(guac_oracle::MetricError::FrameGap { flip: 105, .. })
        ));
        // The port read twice in one game frame shows as a repeat.
        let mut twice = records.clone();
        twice[4].payload = twice[3].payload.clone();
        let r = watch.recording(&twice, &BTreeMap::new()).unwrap();
        let check = RingCheck::of(&r, &twice, &o4);
        assert_eq!((check.frame_gaps, check.frame_repeats), (2, 1));
        assert!(!check.holds());
    }

    #[test]
    fn a_record_missing_a_range_keeps_what_it_can() {
        let mut g = giants();
        let watch = GiantsWatch::resolve(&mut g.w).unwrap();
        let hero_controller = g.w.get32(g.hero.placement + 0xD8);
        g.w.put_floats(g.pad0 + 0x1AC, &[-1.0, 0.0]);
        g.w.put_floats(hero_controller + 0x1AC, &[0.25, 0.5]);
        let full = sample(&mut g.w, &watch.list, 7);
        let range_of = |name: &str| watch.list.placed(name).unwrap().range;
        // Pad 0's stick, or the hero's controller's when pad 0's wasn't readable.
        assert_eq!(watch.frame(&full, None).unwrap().stick, [-1.0, 0.0]);
        let mut no_pad0 = full.clone();
        no_pad0.missing = 1 << range_of("pad0");
        assert_eq!(watch.frame(&no_pad0, None).unwrap().stick, [0.25, 0.5]);
        // The clips' range unreadable: the frame decodes, without clips.
        let mut no_clips = full.clone();
        no_clips.missing = 1 << range_of("anim.control.1");
        let f = watch.frame(&no_clips, None).unwrap();
        assert!(f.clips.is_empty());
        assert_eq!(watch.frame(&full, None).unwrap().clips.len(), 3);
        // The hero's placement unreadable: no frame.
        let mut no_hero = full.clone();
        no_hero.missing = 1 << range_of("hero.placement");
        let err = watch.frame(&no_hero, None).unwrap_err();
        assert!(format!("{err:#}").contains("hero.placement"), "{err:#}");
        // A control swapped out under the list: the slot no longer names it,
        // so the clips are dropped rather than read off a stale address.
        let slots = g.w.get32(watch.animation + 0x10);
        g.w.put32(slots + 4, 0x3000_0000);
        let swapped = sample(&mut g.w, &watch.list, 8);
        assert!(watch.frame(&swapped, None).unwrap().clips.is_empty());
        // The raw record round-trips through the file's hex.
        let raw = RawRecord::of(&full);
        assert_eq!(raw.record().unwrap(), full);
    }
}
