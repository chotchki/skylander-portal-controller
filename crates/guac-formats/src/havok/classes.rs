//! The five classes Giants' payloads hold, at Kovah's 2011.2 offsets:
//! hkReferencedObject is 8 bytes, an hkArray is pointer, size, capacity.

use std::collections::BTreeMap;

use super::qs::QsTransform;
use super::spline::{self, TransformTrack};
use super::{FixupCounts, Packfile, Ptr, err, unsupported};
use crate::{ErrorKind, FormatError};

/// Every array's capacity word on the disc is its size plus this flag.
pub const DONT_DEALLOCATE: u32 = 0x8000_0000;
/// Knots are u8 frame numbers, so a block holds at most 256 frames.
pub const MAX_FRAMES: u32 = 256;

#[derive(Debug, Clone, Copy)]
struct Array {
    at: Ptr,
    len: usize,
}

impl Array {
    fn element(&self, i: usize, size: usize) -> Ptr {
        self.at.plus(i * size)
    }
}

/// Reads objects, refusing two arrays that share bytes (and strings past
/// [`super::MAX_STRING`]) so decode work stays linear in the input.
struct Ctx<'p, 'a> {
    pf: &'p Packfile<'a>,
    /// Array data claimed so far: start -> end offset.
    claims: BTreeMap<Ptr, usize>,
}

impl<'a> Ctx<'_, 'a> {
    fn place(&self, at: Ptr, offset: usize) -> usize {
        self.pf.abs(at.plus(offset))
    }

    fn array(
        &mut self,
        obj: Ptr,
        offset: usize,
        elem: usize,
        field: &'static str,
    ) -> Result<Array, FormatError> {
        let pf = self.pf;
        let data = pf.pointer(obj, offset, field)?;
        let size = pf.i32(obj, offset + 4, field)?;
        let capacity = pf.u32(obj, offset + 8, field)?;
        let place = self.place(obj, offset);
        let len = usize::try_from(size)
            .map_err(|_| unsupported(field, place + 4, u64::from(size as u32)))?;
        if capacity != size as u32 | DONT_DEALLOCATE {
            return Err(unsupported(field, place + 8, u64::from(capacity)));
        }
        match (data, len) {
            (None, 0) => Ok(Array { at: obj, len: 0 }),
            (Some(_), 0) => Err(unsupported(field, place, 0)),
            (None, _) => Err(err(field, place, ErrorKind::Missing)),
            (Some(at), len) => {
                let bytes = len.saturating_mul(elem);
                pf.bytes(at, 0, bytes, field)?;
                let end = at.offset + bytes;
                let before = self.claims.range(..at.plus(bytes)).next_back();
                if let Some((s, e)) = before
                    && s.section == at.section
                    && *e > at.offset
                {
                    return Err(err(field, pf.abs(at), ErrorKind::Duplicate));
                }
                self.claims.insert(at, end);
                Ok(Array { at, len })
            }
        }
    }

    fn empty(&mut self, obj: Ptr, offset: usize, field: &'static str) -> Result<(), FormatError> {
        let a = self.array(obj, offset, 1, field)?;
        if a.len != 0 {
            return Err(unsupported(field, self.place(obj, offset), a.len as u64));
        }
        Ok(())
    }

    /// A pointer to an object of `class`; `None` when null.
    fn object(
        &self,
        at: Ptr,
        offset: usize,
        field: &'static str,
        class: &'static str,
    ) -> Result<Option<Ptr>, FormatError> {
        let Some(p) = self.pf.pointer(at, offset, field)? else {
            return Ok(None);
        };
        match self.pf.class_of(p) {
            Some(c) if c == class => Ok(Some(p)),
            found => {
                let kind = ErrorKind::WrongType {
                    expected: class,
                    found: found.unwrap_or("no object").to_owned(),
                };
                Err(err(field, self.place(at, offset), kind))
            }
        }
    }

    /// An array of non-null pointers to objects of `class`.
    fn objects(
        &mut self,
        obj: Ptr,
        offset: usize,
        field: &'static str,
        class: &'static str,
    ) -> Result<Vec<Ptr>, FormatError> {
        let a = self.array(obj, offset, 4, field)?;
        (0..a.len)
            .map(|i| {
                let p = a.element(i, 4);
                self.object(p, 0, field, class)?
                    .ok_or_else(|| err(field, self.place(p, 0), ErrorKind::Missing))
            })
            .collect()
    }

    fn finite(&self, at: Ptr, offset: usize, field: &'static str) -> Result<f32, FormatError> {
        let v = self.pf.f32(at, offset, field)?;
        if !v.is_finite() {
            return Err(unsupported(
                field,
                self.place(at, offset),
                u64::from(v.to_bits()),
            ));
        }
        Ok(v)
    }
}

/// One `hkRootLevelContainer` named variant, read or not.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Variant<'a> {
    pub name: Option<&'a str>,
    pub class: Option<&'a str>,
}

/// `hkRootLevelContainer`, no vtable: an array of 12-byte variants.
pub struct RootLevelContainer;

impl RootLevelContainer {
    pub const CLASS: &'static str = "hkRootLevelContainer";
    pub const NAMED_VARIANTS: usize = 0x00;
    pub const VARIANT_SIZE: usize = 12;
}

/// `hkaAnimationContainer`; attachments and skins must be empty.
pub struct AnimationContainer;

impl AnimationContainer {
    pub const CLASS: &'static str = "hkaAnimationContainer";
    pub const SKELETONS: usize = 0x08;
    pub const ANIMATIONS: usize = 0x14;
    pub const BINDINGS: usize = 0x20;
    pub const ATTACHMENTS: usize = 0x2C;
    pub const SKINS: usize = 0x38;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Bone<'a> {
    pub name: Option<&'a str>,
    pub lock_translation: bool,
}

/// `hkaSkeleton`. Local frames are refused.
#[derive(Debug, Clone, PartialEq)]
pub struct Skeleton<'a> {
    pub name: Option<&'a str>,
    /// Each before its child; -1 for a root.
    pub parents: Vec<i16>,
    pub bones: Vec<Bone<'a>>,
    pub reference_pose: Vec<QsTransform>,
    pub reference_floats: Vec<f32>,
    pub float_slots: Vec<Option<&'a str>>,
}

impl<'a> Skeleton<'a> {
    pub const CLASS: &'static str = "hkaSkeleton";
    pub const NAME: usize = 0x08;
    pub const PARENT_INDICES: usize = 0x0C;
    pub const BONES: usize = 0x18;
    pub const REFERENCE_POSE: usize = 0x24;
    pub const REFERENCE_FLOATS: usize = 0x30;
    pub const FLOAT_SLOTS: usize = 0x3C;
    pub const LOCAL_FRAMES: usize = 0x48;
    pub const BONE_SIZE: usize = 8;
    /// Translation, rotation and scale, each 4 floats.
    pub const QS_SIZE: usize = 48;

    fn read(ctx: &mut Ctx<'_, 'a>, at: Ptr) -> Result<Self, FormatError> {
        let pf = ctx.pf;
        let name = pf.string(at, Self::NAME, "name")?;
        let a = ctx.array(at, Self::PARENT_INDICES, 2, "parentIndices")?;
        let parents = (0..a.len)
            .map(|i| pf.i16(a.element(i, 2), 0, "parentIndices"))
            .collect::<Result<Vec<_>, _>>()?;
        for (i, p) in parents.iter().enumerate() {
            if !(-1..i as i64).contains(&i64::from(*p)) {
                let place = pf.abs(a.element(i, 2));
                return Err(unsupported("parentIndices", place, u64::from(*p as u16)));
            }
        }
        let a = ctx.array(at, Self::BONES, Self::BONE_SIZE, "bones")?;
        let mut bones = Vec::with_capacity(a.len);
        for i in 0..a.len {
            let b = a.element(i, Self::BONE_SIZE);
            let lock = pf.u8(b, 4, "lockTranslation")?;
            if lock > 1 {
                return Err(unsupported(
                    "lockTranslation",
                    pf.abs(b.plus(4)),
                    u64::from(lock),
                ));
            }
            bones.push(Bone {
                name: pf.string(b, 0, "bone name")?,
                lock_translation: lock == 1,
            });
        }
        let a = ctx.array(at, Self::REFERENCE_POSE, Self::QS_SIZE, "referencePose")?;
        let mut reference_pose = Vec::with_capacity(a.len);
        for i in 0..a.len {
            let q = a.element(i, Self::QS_SIZE);
            let f = |k: usize| ctx.finite(q, 4 * k, "referencePose").map(f64::from);
            // The w lanes of translation and scale are noise on the disc.
            reference_pose.push(QsTransform {
                translation: [f(0)?, f(1)?, f(2)?],
                rotation: [f(4)?, f(5)?, f(6)?, f(7)?],
                scale: [f(8)?, f(9)?, f(10)?],
            });
        }
        if bones.len() != parents.len() || reference_pose.len() != parents.len() {
            let place = ctx.place(at, Self::BONES);
            return Err(unsupported("bones", place, bones.len() as u64));
        }
        let a = ctx.array(at, Self::REFERENCE_FLOATS, 4, "referenceFloats")?;
        let reference_floats = (0..a.len)
            .map(|i| ctx.finite(a.element(i, 4), 0, "referenceFloats"))
            .collect::<Result<Vec<_>, _>>()?;
        let a = ctx.array(at, Self::FLOAT_SLOTS, 4, "floatSlots")?;
        let float_slots = (0..a.len)
            .map(|i| pf.string(a.element(i, 4), 0, "floatSlots"))
            .collect::<Result<Vec<_>, _>>()?;
        if float_slots.len() != reference_floats.len() {
            let place = ctx.place(at, Self::FLOAT_SLOTS);
            return Err(unsupported("floatSlots", place, float_slots.len() as u64));
        }
        ctx.empty(at, Self::LOCAL_FRAMES, "localFrames")?;
        Ok(Self {
            name,
            parents,
            bones,
            reference_pose,
            reference_floats,
            float_slots,
        })
    }

    pub fn bone(&self, name: &str) -> Option<usize> {
        self.bones.iter().position(|b| b.name == Some(name))
    }

    /// Model space from a local pose; `None` when the lengths differ.
    pub fn model_pose(&self, local: &[QsTransform]) -> Option<Vec<QsTransform>> {
        if local.len() != self.parents.len() {
            return None;
        }
        let mut out: Vec<QsTransform> = Vec::with_capacity(local.len());
        for (l, p) in local.iter().zip(&self.parents) {
            let m = match usize::try_from(*p) {
                Ok(p) => out[p].mul(l),
                Err(_) => *l,
            };
            out.push(m);
        }
        Some(out)
    }

    pub fn bind_pose(&self) -> Vec<QsTransform> {
        self.model_pose(&self.reference_pose).unwrap_or_default()
    }
}

/// `hkaSplineCompressedAnimation` (through `hkaAnimation`), one block.
#[derive(Debug, Clone, PartialEq)]
pub struct SplineAnimation {
    pub duration: f32,
    pub num_frames: u32,
    pub max_frames_per_block: u32,
    pub block_duration: f32,
    pub frame_duration: f32,
    pub data_len: usize,
    /// Where the transform data ends; zero padding follows.
    pub data_end: usize,
    pub tracks: Vec<TransformTrack>,
}

impl SplineAnimation {
    pub const CLASS: &'static str = "hkaSplineCompressedAnimation";
    pub const TYPE: usize = 0x08;
    pub const DURATION: usize = 0x0C;
    pub const NUMBER_OF_TRANSFORM_TRACKS: usize = 0x10;
    pub const NUMBER_OF_FLOAT_TRACKS: usize = 0x14;
    pub const EXTRACTED_MOTION: usize = 0x18;
    pub const ANNOTATION_TRACKS: usize = 0x1C;
    pub const NUM_FRAMES: usize = 0x28;
    pub const NUM_BLOCKS: usize = 0x2C;
    pub const MAX_FRAMES_PER_BLOCK: usize = 0x30;
    pub const MASK_AND_QUANTIZATION_SIZE: usize = 0x34;
    pub const BLOCK_DURATION: usize = 0x38;
    pub const BLOCK_INVERSE_DURATION: usize = 0x3C;
    pub const FRAME_DURATION: usize = 0x40;
    pub const BLOCK_OFFSETS: usize = 0x44;
    pub const FLOAT_BLOCK_OFFSETS: usize = 0x50;
    pub const TRANSFORM_OFFSETS: usize = 0x5C;
    pub const FLOAT_OFFSETS: usize = 0x68;
    pub const DATA: usize = 0x74;
    pub const ENDIAN: usize = 0x80;
    /// `hkaAnimation`'s type for spline compression in 2011.2.
    pub const SPLINE: i32 = 3;
    /// Tolerance on the timing relations, which hold on every disc clip.
    pub const TIMING: f64 = 1e-4;

    fn read(ctx: &mut Ctx<'_, '_>, at: Ptr) -> Result<Self, FormatError> {
        let pf = ctx.pf;
        let int = |offset, field, ok: &dyn Fn(i32) -> bool| {
            let v = pf.i32(at, offset, field)?;
            if !ok(v) {
                return Err(unsupported(
                    field,
                    pf.abs(at.plus(offset)),
                    u64::from(v as u32),
                ));
            }
            Ok(v)
        };
        int(Self::TYPE, "type", &|v| v == Self::SPLINE)?;
        let tracks = int(
            Self::NUMBER_OF_TRANSFORM_TRACKS,
            "numberOfTransformTracks",
            &|v| v >= 0,
        )?;
        int(Self::NUMBER_OF_FLOAT_TRACKS, "numberOfFloatTracks", &|v| {
            v == 0
        })?;
        if pf
            .pointer(at, Self::EXTRACTED_MOTION, "extractedMotion")?
            .is_some()
        {
            let place = pf.abs(at.plus(Self::EXTRACTED_MOTION));
            return Err(unsupported("extractedMotion", place, 1));
        }
        ctx.empty(at, Self::ANNOTATION_TRACKS, "annotationTracks")?;
        let frames = int(Self::NUM_FRAMES, "numFrames", &|v| {
            (1..=MAX_FRAMES as i32).contains(&v)
        })?;
        int(Self::NUM_BLOCKS, "numBlocks", &|v| v == 1)?;
        let mfpb = int(Self::MAX_FRAMES_PER_BLOCK, "maxFramesPerBlock", &|v| {
            v >= frames
        })?;
        int(
            Self::MASK_AND_QUANTIZATION_SIZE,
            "maskAndQuantizationSize",
            &|v| i64::from(v) == 4 * i64::from(tracks),
        )?;
        let duration = ctx.finite(at, Self::DURATION, "duration")?;
        let block_duration = ctx.finite(at, Self::BLOCK_DURATION, "blockDuration")?;
        let block_inverse = ctx.finite(at, Self::BLOCK_INVERSE_DURATION, "blockInverseDuration")?;
        let frame_duration = ctx.finite(at, Self::FRAME_DURATION, "frameDuration")?;
        let fd = f64::from(frame_duration);
        let near = |a: f64, b: f64| (a - b).abs() <= Self::TIMING * b.abs().max(1.0);
        for (field, offset, ok) in [
            ("frameDuration", Self::FRAME_DURATION, fd > 0.0),
            (
                "duration",
                Self::DURATION,
                near(f64::from(duration), f64::from(frames - 1) * fd),
            ),
            (
                "blockDuration",
                Self::BLOCK_DURATION,
                near(f64::from(block_duration), f64::from(mfpb - 1) * fd),
            ),
            (
                "blockInverseDuration",
                Self::BLOCK_INVERSE_DURATION,
                near(f64::from(block_inverse) * f64::from(block_duration), 1.0),
            ),
        ] {
            if !ok {
                let place = pf.abs(at.plus(offset));
                return Err(unsupported(
                    field,
                    place,
                    u64::from(pf.u32(at, offset, field)?),
                ));
            }
        }
        let offsets = |ctx: &mut Ctx<'_, '_>, offset, field| {
            let a = ctx.array(at, offset, 4, field)?;
            (0..a.len)
                .map(|i| pf.u32(a.element(i, 4), 0, field))
                .collect::<Result<Vec<_>, _>>()
        };
        let blocks = offsets(ctx, Self::BLOCK_OFFSETS, "blockOffsets")?;
        let float_blocks = offsets(ctx, Self::FLOAT_BLOCK_OFFSETS, "floatBlockOffsets")?;
        ctx.empty(at, Self::TRANSFORM_OFFSETS, "transformOffsets")?;
        ctx.empty(at, Self::FLOAT_OFFSETS, "floatOffsets")?;
        let a = ctx.array(at, Self::DATA, 1, "data")?;
        let data = if a.len == 0 {
            &[][..]
        } else {
            pf.bytes(a.at, 0, a.len, "data")?
        };
        int(Self::ENDIAN, "endian", &|v| v == 0)?;
        if blocks != [0] {
            let place = pf.abs(at.plus(Self::BLOCK_OFFSETS));
            return Err(unsupported("blockOffsets", place, blocks.len() as u64));
        }
        let base = if a.len == 0 {
            pf.abs(at.plus(Self::DATA))
        } else {
            pf.abs(a.at)
        };
        let block = spline::decode(data, base, tracks as usize, (frames - 1) as u8)?;
        if float_blocks != [block.end as u32] {
            let place = pf.abs(at.plus(Self::FLOAT_BLOCK_OFFSETS));
            return Err(unsupported("floatBlockOffsets", place, block.end as u64));
        }
        let tail = &data[block.end..];
        if tail.len() >= 16 || tail.iter().any(|b| *b != 0) {
            return Err(unsupported(
                "data padding",
                base + block.end,
                tail.len() as u64,
            ));
        }
        Ok(Self {
            duration,
            num_frames: frames as u32,
            max_frames_per_block: mfpb as u32,
            block_duration,
            frame_duration,
            data_len: data.len(),
            data_end: block.end,
            tracks: block.tracks,
        })
    }

    pub fn last_frame(&self) -> f64 {
        f64::from(self.num_frames - 1)
    }

    /// The frame time at `t` seconds, clamped to the clip.
    pub fn frame_at(&self, t: f64) -> f64 {
        let f = t / f64::from(self.frame_duration);
        if f.is_nan() {
            0.0
        } else {
            f.clamp(0.0, self.last_frame())
        }
    }

    /// Every track's transform at `t` seconds.
    pub fn sample(&self, t: f64) -> Vec<QsTransform> {
        let f = self.frame_at(t);
        self.tracks.iter().map(|tr| tr.value(f)).collect()
    }
}

/// `hkaAnimationBinding`'s blend hint, 2011.2 numbering.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum BlendHint {
    Normal,
    Additive,
}

/// `hkaAnimationBinding`; float slots must be empty.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Binding<'a> {
    pub original_skeleton_name: Option<&'a str>,
    /// One bone index per transform track.
    pub track_to_bone: Vec<u16>,
    pub blend_hint: BlendHint,
}

impl<'a> Binding<'a> {
    pub const CLASS: &'static str = "hkaAnimationBinding";
    pub const ORIGINAL_SKELETON_NAME: usize = 0x08;
    pub const ANIMATION: usize = 0x0C;
    pub const TRANSFORM_TRACK_TO_BONE_INDICES: usize = 0x10;
    pub const FLOAT_TRACK_TO_FLOAT_SLOT_INDICES: usize = 0x1C;
    pub const BLEND_HINT: usize = 0x28;

    /// The binding and the animation it names.
    fn read(ctx: &mut Ctx<'_, 'a>, at: Ptr) -> Result<(Self, Ptr), FormatError> {
        let pf = ctx.pf;
        let original_skeleton_name =
            pf.string(at, Self::ORIGINAL_SKELETON_NAME, "originalSkeletonName")?;
        let animation = ctx
            .object(at, Self::ANIMATION, "animation", SplineAnimation::CLASS)?
            .ok_or_else(|| {
                err(
                    "animation",
                    ctx.place(at, Self::ANIMATION),
                    ErrorKind::Missing,
                )
            })?;
        let field = "transformTrackToBoneIndices";
        let a = ctx.array(at, Self::TRANSFORM_TRACK_TO_BONE_INDICES, 2, field)?;
        let track_to_bone = (0..a.len)
            .map(|i| {
                let b = pf.i16(a.element(i, 2), 0, field)?;
                u16::try_from(b)
                    .map_err(|_| unsupported(field, pf.abs(a.element(i, 2)), u64::from(b as u16)))
            })
            .collect::<Result<Vec<_>, _>>()?;
        ctx.empty(
            at,
            Self::FLOAT_TRACK_TO_FLOAT_SLOT_INDICES,
            "floatTrackToFloatSlotIndices",
        )?;
        let blend_hint = match pf.u8(at, Self::BLEND_HINT, "blendHint")? {
            0 => BlendHint::Normal,
            1 => BlendHint::Additive,
            v => {
                return Err(unsupported(
                    "blendHint",
                    ctx.place(at, Self::BLEND_HINT),
                    u64::from(v),
                ));
            }
        };
        let binding = Self {
            original_skeleton_name,
            track_to_bone,
            blend_hint,
        };
        Ok((binding, animation))
    }
}

/// A clip and its binding.
#[derive(Debug, Clone, PartialEq)]
pub struct Clip<'a> {
    pub animation: SplineAnimation,
    pub binding: Binding<'a>,
}

impl Clip<'_> {
    /// The skeleton's local pose at `t` seconds: its reference pose with each
    /// track written over its bone. `None` for an additive clip (how the game
    /// applies those is open) or a bone the skeleton lacks.
    pub fn local_pose(&self, skeleton: &Skeleton<'_>, t: f64) -> Option<Vec<QsTransform>> {
        if self.binding.blend_hint != BlendHint::Normal {
            return None;
        }
        let mut pose = skeleton.reference_pose.clone();
        for (q, b) in self
            .animation
            .sample(t)
            .into_iter()
            .zip(&self.binding.track_to_bone)
        {
            *pose.get_mut(usize::from(*b))? = q;
        }
        Some(pose)
    }
}

/// A packfile's contents: its variants, skeletons and at most one clip.
#[derive(Debug, Clone, PartialEq)]
pub struct Container<'a> {
    pub fixups: FixupCounts,
    pub variants: Vec<Variant<'a>>,
    pub skeletons: Vec<Skeleton<'a>>,
    pub clip: Option<Clip<'a>>,
}

impl<'a> Container<'a> {
    /// Reads the one hkaAnimationContainer variant; others are listed only.
    pub fn parse(bytes: &'a [u8]) -> Result<Self, FormatError> {
        let pf = Packfile::parse(bytes)?;
        let mut ctx = Ctx {
            pf: &pf,
            claims: BTreeMap::new(),
        };
        let size = RootLevelContainer::VARIANT_SIZE;
        let a = ctx.array(
            pf.root,
            RootLevelContainer::NAMED_VARIANTS,
            size,
            "namedVariants",
        )?;
        let mut variants = Vec::with_capacity(a.len);
        let mut container = None;
        for i in 0..a.len {
            let v = a.element(i, size);
            let class = pf.string(v, 4, "variant className")?;
            let variant = Variant {
                name: pf.string(v, 0, "variant name")?,
                class,
            };
            if let Some(obj) = pf.pointer(v, 8, "variant")? {
                let found = pf.class_of(obj);
                if found.is_none() || found != class {
                    let kind = ErrorKind::WrongType {
                        expected: "the variant's className",
                        found: found.unwrap_or("no object").to_owned(),
                    };
                    return Err(err("variant", ctx.place(v, 8), kind));
                }
                if class == Some(AnimationContainer::CLASS) && container.replace(obj).is_some() {
                    return Err(err("variant", ctx.place(v, 8), ErrorKind::Duplicate));
                }
            }
            variants.push(variant);
        }
        let place = pf.abs(pf.root);
        let at = container.ok_or_else(|| err("namedVariants", place, ErrorKind::Missing))?;

        type C = AnimationContainer;
        let skeletons = ctx
            .objects(at, C::SKELETONS, "skeletons", Skeleton::CLASS)?
            .into_iter()
            .map(|p| Skeleton::read(&mut ctx, p))
            .collect::<Result<Vec<_>, _>>()?;
        let animations = ctx.objects(at, C::ANIMATIONS, "animations", SplineAnimation::CLASS)?;
        let bindings = ctx.objects(at, C::BINDINGS, "bindings", Binding::CLASS)?;
        ctx.empty(at, C::ATTACHMENTS, "attachments")?;
        ctx.empty(at, C::SKINS, "skins")?;
        if animations.len() > 1 {
            let place = ctx.place(at, C::ANIMATIONS);
            return Err(unsupported("animations", place, animations.len() as u64));
        }
        if bindings.len() != animations.len() {
            let place = ctx.place(at, C::BINDINGS);
            return Err(unsupported("bindings", place, bindings.len() as u64));
        }
        let clip = match (animations.first(), bindings.first()) {
            (Some(&anim), Some(&b)) => {
                let animation = SplineAnimation::read(&mut ctx, anim)?;
                let (binding, target) = Binding::read(&mut ctx, b)?;
                if target != anim {
                    let place = ctx.place(b, Binding::ANIMATION);
                    return Err(unsupported("animation", place, pf.abs(target) as u64));
                }
                if binding.track_to_bone.len() != animation.tracks.len() {
                    let place = ctx.place(b, Binding::TRANSFORM_TRACK_TO_BONE_INDICES);
                    let n = binding.track_to_bone.len() as u64;
                    return Err(unsupported("transformTrackToBoneIndices", place, n));
                }
                Some(Clip { animation, binding })
            }
            _ => None,
        };
        Ok(Self {
            fixups: pf.fixups,
            variants,
            skeletons,
            clip,
        })
    }
}
