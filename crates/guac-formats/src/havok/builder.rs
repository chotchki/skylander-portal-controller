//! Synthetic Havok packfiles for tests and fuzz seeds (`test-fixtures`): a
//! skeleton and a spline clip in the disc's layout, every value known.

use super::qs::{QsTransform, Quat};
use super::spline::{QUANTIZATION, QUAT_BYTES};
use super::{
    AnimationContainer, Binding, CLASS_SEPARATOR, CLASSNAMES, FILE_VERSION, LAYOUT_RULES, MAGIC,
    RootLevelContainer, SIGNATURES, Skeleton, SplineAnimation, VERSION,
};

/// The reflection classes every disc payload also lists.
pub const REFLECTION: [(&str, u32); 4] = [
    ("hkClass", 0x7558_5EF6),
    ("hkClassMember", 0x5C7E_A4C2),
    ("hkClassEnum", 0x8A36_09CF),
    ("hkClassEnumItem", 0xCE6F_8A6C),
];
/// The one other variant class on the disc.
pub const RESOURCE: (&str, u32) = ("hkMemoryResourceContainer", 0x4762_F92A);

/// A section's object data and its fixups, as offsets into the data.
#[derive(Debug, Clone, Default)]
pub struct SectionSpec {
    pub tag: &'static str,
    pub data: Vec<u8>,
    pub local: Vec<(u32, u32)>,
    pub global: Vec<(u32, u32, u32)>,
    pub virtual_: Vec<(u32, u32, u32)>,
}

fn pad(out: &mut Vec<u8>, align: usize, byte: u8) {
    out.resize(out.len().next_multiple_of(align), byte);
}

fn words(out: &mut Vec<u8>, w: &[u32]) {
    for v in w {
        out.extend_from_slice(&v.to_be_bytes());
    }
}

/// A whole packfile; `root` and `root_class` are (section, offset).
pub fn packfile(sections: &[SectionSpec], root: (u32, u32), root_class: (u32, u32)) -> Vec<u8> {
    let mut out = MAGIC.to_vec();
    words(&mut out, &[0, FILE_VERSION as u32]);
    out.extend_from_slice(&LAYOUT_RULES);
    words(
        &mut out,
        &[
            sections.len() as u32,
            root.0,
            root.1,
            root_class.0,
            root_class.1,
        ],
    );
    out.extend_from_slice(VERSION);
    out.push(0xFF);
    words(&mut out, &[0, 0xFFFF_FFFF]);
    let mut bodies = Vec::new();
    let mut start = 0x40 + 0x30 * sections.len();
    for s in sections {
        let mut body = s.data.clone();
        pad(&mut body, 16, 0);
        let local = body.len();
        for (a, b) in &s.local {
            words(&mut body, &[*a, *b]);
        }
        pad(&mut body, 16, 0xFF);
        let global = body.len();
        for (a, b, c) in &s.global {
            words(&mut body, &[*a, *b, *c]);
        }
        pad(&mut body, 16, 0xFF);
        let virtual_ = body.len();
        for (a, b, c) in &s.virtual_ {
            words(&mut body, &[*a, *b, *c]);
        }
        pad(&mut body, 16, 0xFF);
        let end = body.len();
        let mut tag = [0u8; 20];
        tag[..s.tag.len()].copy_from_slice(s.tag.as_bytes());
        tag[19] = 0xFF;
        out.extend_from_slice(&tag);
        let offsets = [start, local, global, virtual_, end, end, end];
        words(&mut out, &offsets.map(|v| v as u32));
        start += end;
        bodies.push(body);
    }
    out.extend(bodies.concat());
    out
}

/// `__classnames__` data and each name's offset, in the order given.
pub fn class_names(names: &[(&str, u32)]) -> (Vec<u8>, Vec<u32>) {
    let mut data = Vec::new();
    let mut offsets = Vec::new();
    for (name, sig) in names {
        data.extend_from_slice(&sig.to_be_bytes());
        data.push(CLASS_SEPARATOR);
        offsets.push(data.len() as u32);
        data.extend_from_slice(name.as_bytes());
        data.push(0);
    }
    pad(&mut data, 16, 0xFF);
    (data, offsets)
}

/// Builds one data section: objects, arrays and strings, with fixups.
#[derive(Debug, Clone, Default)]
pub struct Data {
    pub bytes: Vec<u8>,
    pub local: Vec<(u32, u32)>,
    pub global: Vec<(u32, u32, u32)>,
    pub virtual_: Vec<(u32, u32, u32)>,
    /// This section's index; pointers go through global fixups when set.
    pub global_to: Option<u32>,
    /// The class names section's index.
    pub names: u32,
}

impl Data {
    pub fn alloc(&mut self, len: usize, align: usize) -> u32 {
        pad(&mut self.bytes, align, 0);
        let at = self.bytes.len();
        self.bytes.resize(at + len, 0);
        at as u32
    }

    pub fn object(&mut self, class: u32, size: usize) -> u32 {
        let at = self.alloc(size, 16);
        self.virtual_.push((at, self.names, class));
        at
    }

    pub fn put(&mut self, at: u32, bytes: &[u8]) {
        let at = at as usize;
        self.bytes[at..at + bytes.len()].copy_from_slice(bytes);
    }

    pub fn u32(&mut self, at: u32, v: u32) {
        self.put(at, &v.to_be_bytes());
    }

    pub fn f32(&mut self, at: u32, v: f32) {
        self.put(at, &v.to_be_bytes());
    }

    pub fn ptr(&mut self, at: u32, target: u32) {
        match self.global_to {
            Some(s) => self.global.push((at, s, target)),
            None => self.local.push((at, target)),
        }
    }

    pub fn string(&mut self, s: &str) -> u32 {
        let at = self.alloc(s.len() + 1, 1);
        self.put(at, s.as_bytes());
        at
    }

    /// Strings always take a local fixup, so a global-fixup build has both.
    pub fn string_field(&mut self, at: u32, s: &str) {
        let target = self.string(s);
        self.local.push((at, target));
    }

    /// An hkArray at `at` over `len` elements at `target`.
    pub fn array(&mut self, at: u32, target: Option<u32>, len: u32) {
        if let Some(t) = target {
            self.ptr(at, t);
        }
        self.u32(at + 4, len);
        self.u32(at + 8, len | 0x8000_0000);
    }

    /// An array of `elements`, each `size` bytes, written by `put`.
    pub fn array_of(
        &mut self,
        at: u32,
        size: usize,
        count: usize,
        mut put: impl FnMut(&mut Self, u32, usize),
    ) {
        if count == 0 {
            return self.array(at, None, 0);
        }
        let data = self.alloc(size * count, 16);
        for i in 0..count {
            put(self, data + (size * i) as u32, i);
        }
        self.array(at, Some(data), count as u32);
    }
}

/// THREECOMP40 for a unit quaternion: drops the largest component.
pub fn encode40(q: Quat) -> [u8; QUAT_BYTES] {
    let m = (0..4)
        .max_by(|a, b| q[*a].abs().total_cmp(&q[*b].abs()))
        .unwrap_or(3);
    let k = 2047.0 * std::f64::consts::SQRT_2;
    let mut v = 0u64;
    for (slot, i) in (0..4).filter(|i| *i != m).enumerate() {
        let c = (q[i] * k + 2047.0).round().clamp(0.0, 4095.0) as u64;
        v |= c << (12 * slot);
    }
    v |= (m as u64) << 36;
    v |= u64::from(q[m] < 0.0) << 38;
    let b = v.to_le_bytes();
    [b[0], b[1], b[2], b[3], b[4]]
}

/// A channel of a position or scale track.
#[derive(Debug, Clone, PartialEq)]
pub enum ChanSpec {
    Identity,
    Static(f32),
    Spline { min: f32, max: f32, q: Vec<u16> },
}

/// Spline channels share `degree` and `knots`.
#[derive(Debug, Clone, PartialEq)]
pub struct VecSpec {
    pub degree: u8,
    pub knots: Vec<u8>,
    pub channels: [ChanSpec; 3],
}

impl VecSpec {
    pub fn identity() -> Self {
        Self {
            degree: 0,
            knots: Vec::new(),
            channels: [ChanSpec::Identity, ChanSpec::Identity, ChanSpec::Identity],
        }
    }

    pub fn constant(v: [f32; 3]) -> Self {
        Self {
            channels: v.map(ChanSpec::Static),
            ..Self::identity()
        }
    }

    fn mask(&self) -> u8 {
        (0..3).fold(0, |m, c| match self.channels[c] {
            ChanSpec::Identity => m,
            ChanSpec::Static(_) => m | 1 << c,
            ChanSpec::Spline { .. } => m | 0x10 << c,
        })
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum QuatSpec {
    Identity,
    Static([u8; QUAT_BYTES]),
    Spline {
        degree: u8,
        knots: Vec<u8>,
        points: Vec<[u8; QUAT_BYTES]>,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub struct TrackSpec {
    pub position: VecSpec,
    pub rotation: QuatSpec,
    pub scale: VecSpec,
}

/// Writes spline data little-endian, or big-endian to show it's refused.
struct Spline {
    out: Vec<u8>,
    big_endian: bool,
}

impl Spline {
    fn u16(&mut self, v: u16) {
        let b = if self.big_endian {
            v.to_be_bytes()
        } else {
            v.to_le_bytes()
        };
        self.out.extend_from_slice(&b);
    }

    fn f32(&mut self, v: f32) {
        let b = if self.big_endian {
            v.to_be_bytes()
        } else {
            v.to_le_bytes()
        };
        self.out.extend_from_slice(&b);
    }

    fn quat(&mut self, q: [u8; QUAT_BYTES]) {
        let mut q = q;
        if self.big_endian {
            q.reverse();
        }
        self.out.extend_from_slice(&q);
    }

    fn knots(&mut self, degree: u8, knots: &[u8]) {
        self.u16((knots.len() - usize::from(degree) - 2) as u16);
        self.out.push(degree);
        self.out.extend_from_slice(knots);
    }

    fn vector(&mut self, v: &VecSpec) {
        let splines: Vec<&Vec<u16>> = v
            .channels
            .iter()
            .filter_map(|c| match c {
                ChanSpec::Spline { q, .. } => Some(q),
                _ => None,
            })
            .collect();
        if splines.is_empty() {
            for c in &v.channels {
                if let ChanSpec::Static(x) = c {
                    self.f32(*x);
                }
            }
            return;
        }
        self.knots(v.degree, &v.knots);
        pad(&mut self.out, 4, 0);
        for c in &v.channels {
            match c {
                ChanSpec::Identity => {}
                ChanSpec::Static(x) => self.f32(*x),
                ChanSpec::Spline { min, max, .. } => {
                    self.f32(*min);
                    self.f32(*max);
                }
            }
        }
        for i in 0..splines[0].len() {
            for q in &splines {
                self.u16(q[i]);
            }
        }
        pad(&mut self.out, 4, 0);
    }

    fn rotation(&mut self, r: &QuatSpec) {
        match r {
            QuatSpec::Identity => {}
            QuatSpec::Static(q) => self.quat(*q),
            QuatSpec::Spline {
                degree,
                knots,
                points,
            } => {
                self.knots(*degree, knots);
                for q in points {
                    self.quat(*q);
                }
            }
        }
        pad(&mut self.out, 4, 0);
    }
}

/// One block's data: masks, then each track's position, rotation, scale.
pub fn spline_data(tracks: &[TrackSpec], big_endian: bool) -> Vec<u8> {
    let mut w = Spline {
        out: Vec::new(),
        big_endian,
    };
    for t in tracks {
        let rot = match t.rotation {
            QuatSpec::Identity => 0,
            QuatSpec::Static(_) => 0x0F,
            QuatSpec::Spline { .. } => 0xF0,
        };
        w.out
            .extend_from_slice(&[QUANTIZATION, t.position.mask(), rot, t.scale.mask()]);
    }
    for t in tracks {
        w.vector(&t.position);
        w.rotation(&t.rotation);
        w.vector(&t.scale);
    }
    w.out
}

#[derive(Debug, Clone, PartialEq)]
pub struct SkeletonSpec {
    pub name: &'static str,
    /// Name, parent and reference transform.
    pub bones: Vec<(&'static str, i16, QsTransform)>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ClipSpec {
    pub frames: u32,
    pub frame_duration: f32,
    pub tracks: Vec<TrackSpec>,
    pub track_to_bone: Vec<i16>,
    pub blend_hint: u8,
    pub big_endian: bool,
    /// Zero bytes after the transform data.
    pub tail: usize,
}

impl ClipSpec {
    pub fn data(&self) -> Vec<u8> {
        spline_data(&self.tracks, self.big_endian)
    }
}

/// What goes in one payload.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct PayloadSpec {
    pub skeletons: Vec<SkeletonSpec>,
    pub clip: Option<ClipSpec>,
    /// Two sections (no `__types__`) with global fixups, instead of the
    /// disc's three with local ones.
    pub two_sections: bool,
    /// Adds a "Resource Data" variant, as one disc payload has.
    pub resource: bool,
}

/// Where the builder put things, for tests that mutate them.
#[derive(Debug, Clone, Default)]
pub struct Built {
    pub bytes: Vec<u8>,
    /// Absolute offset of the data section.
    pub data_start: usize,
    pub skeletons: Vec<u32>,
    pub animation: Option<u32>,
    pub binding: Option<u32>,
    /// The spline data, from the data section's start.
    pub spline: Option<u32>,
}

impl Built {
    pub fn at(&self, offset: u32) -> usize {
        self.data_start + offset as usize
    }

    pub fn put_u32(&mut self, offset: u32, v: u32) {
        let at = self.at(offset);
        self.bytes[at..at + 4].copy_from_slice(&v.to_be_bytes());
    }
}

pub fn payload(spec: &PayloadSpec) -> Built {
    let mut names: Vec<(&str, u32)> = REFLECTION.to_vec();
    names.extend(SIGNATURES);
    if spec.resource {
        names.push(RESOURCE);
    }
    let (cn_data, cn) = class_names(&names);
    let class = |name: &str| cn[names.iter().position(|(n, _)| *n == name).unwrap_or(0)];
    let data_index = if spec.two_sections { 1 } else { 2 };
    let mut d = Data {
        global_to: spec.two_sections.then_some(data_index),
        ..Data::default()
    };
    let mut built = Built::default();

    let root = d.object(class(RootLevelContainer::CLASS), 12);
    let variants = 1 + usize::from(spec.resource);
    let container = d.object(class(AnimationContainer::CLASS), 0x44);
    let resource = spec.resource.then(|| d.object(class(RESOURCE.0), 0x20));
    d.array_of(root, 12, variants, |d, v, i| {
        let (name, cls, obj) = match i {
            0 => (
                "Merged Animation Container",
                AnimationContainer::CLASS,
                container,
            ),
            _ => ("Resource Data", RESOURCE.0, resource.unwrap_or(0)),
        };
        d.string_field(v, name);
        d.string_field(v + 4, cls);
        d.ptr(v + 8, obj);
    });

    for s in &spec.skeletons {
        let at = d.object(class(Skeleton::CLASS), 0x54);
        built.skeletons.push(at);
        d.string_field(at + 0x08, s.name);
        let n = s.bones.len();
        d.array_of(at + 0x0C, 2, n, |d, e, i| {
            d.put(e, &s.bones[i].1.to_be_bytes())
        });
        d.array_of(at + 0x18, 8, n, |d, e, i| d.string_field(e, s.bones[i].0));
        d.array_of(at + 0x24, 48, n, |d, e, i| {
            let q = s.bones[i].2;
            let f = [
                q.translation[0],
                q.translation[1],
                q.translation[2],
                0.0,
                q.rotation[0],
                q.rotation[1],
                q.rotation[2],
                q.rotation[3],
                q.scale[0],
                q.scale[1],
                q.scale[2],
                1.0,
            ];
            for (k, v) in f.iter().enumerate() {
                d.f32(e + 4 * k as u32, *v as f32);
            }
        });
        for empty in [0x30, 0x3C, 0x48] {
            d.array(at + empty, None, 0);
        }
    }
    let skeletons = built.skeletons.clone();
    d.array_of(container + 0x08, 4, skeletons.len(), |d, e, i| {
        d.ptr(e, skeletons[i])
    });

    if let Some(c) = &spec.clip {
        let anim = d.object(class(SplineAnimation::CLASS), 0x84);
        let binding = d.object(class(Binding::CLASS), 0x2C);
        built.animation = Some(anim);
        built.binding = Some(binding);
        let data = c.data();
        let nt = c.tracks.len() as u32;
        d.u32(anim + 0x08, 3);
        let fd = c.frame_duration;
        d.f32(anim + 0x0C, (c.frames - 1) as f32 * fd);
        d.u32(anim + 0x10, nt);
        d.array(anim + 0x1C, None, 0);
        d.u32(anim + 0x28, c.frames);
        d.u32(anim + 0x2C, 1);
        d.u32(anim + 0x30, 256);
        d.u32(anim + 0x34, 4 * nt);
        d.f32(anim + 0x38, 255.0 * fd);
        d.f32(anim + 0x3C, 1.0 / (255.0 * fd));
        d.f32(anim + 0x40, fd);
        d.array_of(anim + 0x44, 4, 1, |d, e, _| d.u32(e, 0));
        let end = data.len() as u32;
        d.array_of(anim + 0x50, 4, 1, |d, e, _| d.u32(e, end));
        d.array(anim + 0x5C, None, 0);
        d.array(anim + 0x68, None, 0);
        let mut bytes = data;
        bytes.resize(bytes.len() + c.tail, 0);
        d.array_of(anim + 0x74, 1, bytes.len(), |d, e, i| {
            d.bytes[e as usize] = bytes[i]
        });
        built.spline = Some(d.bytes.len() as u32 - bytes.len() as u32);
        d.string_field(binding + 0x08, "Synthetic:J_root");
        d.ptr(binding + 0x0C, anim);
        let map = &c.track_to_bone;
        d.array_of(binding + 0x10, 2, map.len(), |d, e, i| {
            d.put(e, &map[i].to_be_bytes())
        });
        d.array(binding + 0x1C, None, 0);
        d.bytes[binding as usize + 0x28] = c.blend_hint;
        d.array_of(container + 0x14, 4, 1, |d, e, _| d.ptr(e, anim));
        d.array_of(container + 0x20, 4, 1, |d, e, _| d.ptr(e, binding));
    } else {
        d.array(container + 0x14, None, 0);
        d.array(container + 0x20, None, 0);
    }
    d.array(container + 0x2C, None, 0);
    d.array(container + 0x38, None, 0);

    let names_section = SectionSpec {
        tag: CLASSNAMES,
        data: cn_data,
        ..SectionSpec::default()
    };
    let data_section = SectionSpec {
        tag: "__data__",
        data: d.bytes,
        local: d.local,
        global: d.global,
        virtual_: d.virtual_,
    };
    let mut sections = vec![names_section];
    if !spec.two_sections {
        sections.push(SectionSpec {
            tag: "__types__",
            ..SectionSpec::default()
        });
    }
    sections.push(data_section);
    let root_class = class(RootLevelContainer::CLASS);
    built.bytes = packfile(&sections, (data_index, root), (0, root_class));
    let cn_len = sections[0].data.len().next_multiple_of(16);
    built.data_start = 0x40 + 0x30 * sections.len() + cn_len;
    built
}

fn qs(t: [f64; 3], r: Quat) -> QsTransform {
    QsTransform {
        translation: t,
        rotation: r,
        scale: [1.0; 3],
    }
}

/// Three bones: a root at (0, 1, 0) turned 90 degrees about +Z, a child 2
/// along its x and a grandchild 1 along x, turned 90 degrees about +X.
pub fn skeleton3() -> SkeletonSpec {
    let h = std::f64::consts::FRAC_1_SQRT_2;
    SkeletonSpec {
        name: "J_root",
        bones: vec![
            ("J_root", -1, qs([0.0, 1.0, 0.0], [0.0, 0.0, h, h])),
            ("J_spine", 0, qs([2.0, 0.0, 0.0], [0.0, 0.0, 0.0, 1.0])),
            ("J_head", 1, qs([1.0, 0.0, 0.0], [h, 0.0, 0.0, h])),
        ],
    }
}

/// Knots for `points` control points of `degree` over frames `0..=last`,
/// interior knots spread evenly.
pub fn clamped_knots(degree: u8, points: usize, last: u8) -> Vec<u8> {
    let d = usize::from(degree);
    let inner = points - d - 1;
    let mut k = vec![0u8; d + 1];
    k.extend((1..=inner).map(|i| (i * usize::from(last) / (inner + 1)) as u8));
    k.extend(std::iter::repeat_n(last, d + 1));
    k
}

/// Four tracks over 9 frames at 30 fps: a cubic position on x and z with
/// static y, a linear rotation, a static scale, identity and static tracks.
pub fn clip() -> ClipSpec {
    let h = std::f64::consts::FRAC_1_SQRT_2;
    let cubic = clamped_knots(3, 5, 8);
    let position = VecSpec {
        degree: 3,
        knots: cubic,
        channels: [
            ChanSpec::Spline {
                min: -1.0,
                max: 3.0,
                q: vec![0, 16384, 32768, 49152, 65535],
            },
            ChanSpec::Static(0.5),
            ChanSpec::Spline {
                min: 0.0,
                max: 1.0,
                q: vec![65535, 0, 65535, 0, 65535],
            },
        ],
    };
    let rotation = QuatSpec::Spline {
        degree: 1,
        knots: vec![0, 0, 8, 8],
        points: vec![encode40([0.0, 0.0, 0.0, 1.0]), encode40([0.0, 0.0, h, h])],
    };
    ClipSpec {
        frames: 9,
        frame_duration: 1.0 / 30.0,
        tracks: vec![
            TrackSpec {
                position,
                rotation,
                scale: VecSpec::constant([2.0, 2.0, 2.0]),
            },
            TrackSpec {
                position: VecSpec::identity(),
                rotation: QuatSpec::Identity,
                scale: VecSpec::identity(),
            },
            TrackSpec {
                position: VecSpec::constant([0.0, 0.25, 0.0]),
                rotation: QuatSpec::Static(encode40([h, 0.0, 0.0, -h])),
                scale: VecSpec::identity(),
            },
        ],
        track_to_bone: vec![0, 1, 2],
        blend_hint: 0,
        big_endian: false,
        tail: 0,
    }
}

pub fn seeds() -> Vec<Vec<u8>> {
    let skeleton = PayloadSpec {
        skeletons: vec![skeleton3()],
        ..PayloadSpec::default()
    };
    let clip_only = PayloadSpec {
        clip: Some(clip()),
        ..PayloadSpec::default()
    };
    let both = PayloadSpec {
        skeletons: vec![skeleton3()],
        clip: Some(clip()),
        two_sections: true,
        resource: true,
    };
    let mut padded = clip_only.clone();
    if let Some(c) = &mut padded.clip {
        c.tail = 12;
    }
    // The cheapest track in bytes, many times: the heap budget's worst case.
    let mut wide = clip_only.clone();
    if let Some(c) = &mut wide.clip {
        let identity = c.tracks[1].clone();
        c.tracks = vec![identity; 2000];
        c.track_to_bone = vec![0; 2000];
    }
    [
        skeleton,
        clip_only,
        both,
        padded,
        wide,
        PayloadSpec::default(),
    ]
    .iter()
    .map(|s| payload(s).bytes)
    .collect()
}
