use super::builder::{
    self, Built, ChanSpec, ClipSpec, PayloadSpec, QuatSpec, TrackSpec, VecSpec, clamped_knots,
    encode40, payload, skeleton3,
};
use super::qs::{self, QsTransform};
use super::spline::{self, Channel, Curve, QuatTrack, threecomp40};
use super::*;

const H: f64 = std::f64::consts::FRAC_1_SQRT_2;

fn spec(skeleton: bool, clip: Option<ClipSpec>) -> PayloadSpec {
    PayloadSpec {
        skeletons: if skeleton { vec![skeleton3()] } else { vec![] },
        clip,
        ..PayloadSpec::default()
    }
}

fn clip_payload() -> Built {
    payload(&spec(false, Some(builder::clip())))
}

fn close(a: &[f64], b: &[f64], eps: f64) -> bool {
    a.len() == b.len() && a.iter().zip(b).all(|(x, y)| (x - y).abs() <= eps)
}

/// The refusal's field and kind.
fn refused(bytes: &[u8]) -> (&'static str, ErrorKind) {
    let e = Container::parse(bytes).unwrap_err();
    (e.field, e.kind)
}

fn field(bytes: &[u8]) -> &'static str {
    refused(bytes).0
}

/// The data section's start and its local fixup table, absolute.
fn local_table(bytes: &[u8]) -> (usize, usize, usize) {
    let n = u32::from_be_bytes(bytes[0x14..0x18].try_into().unwrap()) as usize;
    let h = HEADER_LEN + SECTION_HEADER_LEN * (n - 1);
    let w =
        |k: usize| u32::from_be_bytes(bytes[h + 20 + 4 * k..h + 24 + 4 * k].try_into().unwrap());
    let start = w(0) as usize;
    (start, start + w(1) as usize, start + w(2) as usize)
}

/// Index of the local fixup whose source is `src`.
fn fixup_from(bytes: &[u8], src: u32) -> usize {
    let (_, from, to) = local_table(bytes);
    (from..to)
        .step_by(8)
        .find(|&e| bytes[e..e + 4] == src.to_be_bytes())
        .unwrap()
}

/// Cox-de Boor straight from the recursive definition, 0/0 taken as 0.
fn naive_basis(i: usize, p: usize, knots: &[u8], u: f64) -> f64 {
    let k = |j: usize| f64::from(knots[j]);
    if p == 0 {
        let last = knots.len() - 1;
        let at_end = u == k(last) && k(i) < k(i + 1) && k(i + 1) == k(last);
        return if (k(i) <= u && u < k(i + 1)) || at_end {
            1.0
        } else {
            0.0
        };
    }
    let a = if k(i + p) > k(i) {
        (u - k(i)) / (k(i + p) - k(i)) * naive_basis(i, p - 1, knots, u)
    } else {
        0.0
    };
    let b = if k(i + p + 1) > k(i + 1) {
        (k(i + p + 1) - u) / (k(i + p + 1) - k(i + 1)) * naive_basis(i + 1, p - 1, knots, u)
    } else {
        0.0
    };
    a + b
}

fn naive(points: &[f64], p: usize, knots: &[u8], u: f64) -> f64 {
    (0..points.len())
        .map(|i| points[i] * naive_basis(i, p, knots, u))
        .sum()
}

#[test]
fn every_seed_parses() {
    for (i, seed) in builder::seeds().iter().enumerate() {
        Container::parse(seed).unwrap_or_else(|e| panic!("seed {i}: {e}"));
    }
    let bytes = payload(&PayloadSpec::default()).bytes;
    let empty = Container::parse(&bytes).unwrap();
    assert!(empty.skeletons.is_empty() && empty.clip.is_none());
    assert_eq!(empty.variants[0].name, Some("Merged Animation Container"));
}

#[test]
fn a_two_section_packfile_resolves_local_global_and_virtual_fixups() {
    let built = payload(&PayloadSpec {
        skeletons: vec![skeleton3()],
        clip: Some(builder::clip()),
        two_sections: true,
        resource: true,
    });
    let pf = Packfile::parse(&built.bytes).unwrap();
    let tags: Vec<&str> = pf.sections.iter().map(|s| s.tag).collect();
    assert_eq!(tags, ["__classnames__", "__data__"]);
    assert!(pf.fixups.local > 0 && pf.fixups.global > 0);
    // One virtual fixup per object.
    assert_eq!(pf.fixups.virtual_, 6);
    let classes: Vec<&str> = pf.objects().map(|(_, c)| c).collect();
    assert_eq!(
        classes,
        [
            "hkRootLevelContainer",
            "hkaAnimationContainer",
            "hkMemoryResourceContainer",
            "hkaSkeleton",
            "hkaSplineCompressedAnimation",
            "hkaAnimationBinding"
        ]
    );
    assert_eq!(
        pf.root,
        Ptr {
            section: 1,
            offset: 0
        }
    );
    let skeleton = Ptr {
        section: 1,
        offset: built.skeletons[0] as usize,
    };
    // The bone array goes through a global fixup, its names through local.
    let bones = pf
        .pointer(skeleton, Skeleton::BONES, "bones")
        .unwrap()
        .unwrap();
    assert_eq!(pf.string(bones, 0, "name").unwrap(), Some("J_root"));

    let c = Container::parse(&built.bytes).unwrap();
    assert_eq!(c.fixups, pf.fixups);
    let names: Vec<_> = c.variants.iter().map(|v| (v.name, v.class)).collect();
    assert_eq!(
        names,
        [
            (
                Some("Merged Animation Container"),
                Some("hkaAnimationContainer")
            ),
            (Some("Resource Data"), Some("hkMemoryResourceContainer"))
        ]
    );
    assert_eq!(c.skeletons[0].parents, [-1, 0, 1]);
    assert!(c.clip.is_some());
}

#[test]
fn a_skeleton_reads_back_and_composes_its_bind_pose() {
    let bytes = payload(&spec(true, None)).bytes;
    let c = Container::parse(&bytes).unwrap();
    let s = &c.skeletons[0];
    assert_eq!(s.name, Some("J_root"));
    let names: Vec<_> = s.bones.iter().map(|b| b.name.unwrap()).collect();
    assert_eq!(names, ["J_root", "J_spine", "J_head"]);
    assert!(s.bones.iter().all(|b| !b.lock_translation));
    assert_eq!(s.parents, [-1, 0, 1]);
    assert_eq!(s.bone("J_head"), Some(2));
    assert_eq!(s.reference_pose[1].translation, [2.0, 0.0, 0.0]);
    assert!(s.reference_floats.is_empty() && s.float_slots.is_empty());

    // Root at (0,1,0) turned 90 about z: the spine lands 2 up from it, the
    // head 1 further, its x on the model's y and its z on the model's x.
    let bind = s.bind_pose();
    let t: Vec<[f64; 3]> = bind.iter().map(|q| q.translation).collect();
    for (got, want) in t
        .iter()
        .zip([[0.0, 1.0, 0.0], [0.0, 3.0, 0.0], [0.0, 4.0, 0.0]])
    {
        assert!(close(got, &want, 1e-6), "{got:?} vs {want:?}");
    }
    let m = bind[2].matrix();
    let want = [
        [0.0, 0.0, 1.0, 0.0],
        [1.0, 0.0, 0.0, 4.0],
        [0.0, 1.0, 0.0, 0.0],
        [0.0, 0.0, 0.0, 1.0],
    ];
    for (row, w) in m.iter().zip(want) {
        assert!(close(row, &w, 1e-6), "{m:?}");
    }
    assert!(s.model_pose(&s.reference_pose[..2]).is_none());
}

#[test]
fn qs_composition_scales_then_rotates_then_translates() {
    let parent = QsTransform {
        translation: [1.0, 0.0, 0.0],
        rotation: [0.0, 0.0, H, H],
        scale: [2.0; 3],
    };
    let child = QsTransform {
        translation: [1.0, 0.0, 0.0],
        rotation: [H, 0.0, 0.0, H],
        scale: [0.5, 1.0, 3.0],
    };
    let m = parent.mul(&child);
    assert!(close(&m.translation, &[1.0, 2.0, 0.0], 1e-12));
    assert_eq!(m.scale, [1.0, 2.0, 6.0]);
    assert!(close(&m.rotation, &[0.5, 0.5, 0.5, 0.5], 1e-12));
    assert!(qs::angle(m.rotation, [-0.5, -0.5, -0.5, -0.5]) < 1e-6);
}

#[test]
fn threecomp40_drops_the_largest_component_and_keeps_its_sign() {
    // Hand-assembled: the identity, then x dropped and negative with y at
    // +1447 steps (0.49983).
    let word = |v: u64| {
        let b = v.to_le_bytes();
        [b[0], b[1], b[2], b[3], b[4]]
    };
    let identity = 2047 | 2047 << 12 | 2047 << 24 | 3 << 36;
    assert_eq!(threecomp40(word(identity)), Some([0.0, 0.0, 0.0, 1.0]));
    let y = 1447.0 / (2047.0 * std::f64::consts::SQRT_2);
    let q = threecomp40(word((2047 + 1447) | 2047 << 12 | 2047 << 24 | 1 << 38)).unwrap();
    assert!(
        close(&q, &[-(1.0 - y * y).sqrt(), y, 0.0, 0.0], 1e-12),
        "{q:?}"
    );
    assert_eq!(threecomp40(word(identity | 1 << 39)), None);
    // Kept components bigger than the dropped one: never encoded.
    assert_eq!(threecomp40(word(0xFFF | 0xFFF << 12 | 0xFFF << 24)), None);

    // One per dropped index; y's is negative, so its sign bit is set.
    for (q, index, sign) in [
        ([0.9, 0.3, -0.2, 0.25], 0, 0),
        ([0.1, -0.8, 0.4, 0.3], 1, 1),
        ([0.2, 0.1, 0.9, -0.3], 2, 0),
        ([0.1, 0.2, 0.3, 0.9], 3, 0),
    ] {
        let q = qs::normalize(q);
        let b = encode40(q);
        let v = u64::from_le_bytes([b[0], b[1], b[2], b[3], b[4], 0, 0, 0]);
        assert_eq!(((v >> 36) & 3, (v >> 38) & 1), (index, sign));
        let back = threecomp40(b).unwrap();
        assert!(close(&back, &q, 5e-4), "{back:?} vs {q:?}");
        // Read big-endian, the same bytes give another rotation.
        let mut swapped = b;
        swapped.reverse();
        let wrong = threecomp40(swapped).map_or(1.0, |w| qs::angle(w, q));
        assert!(wrong > 1e-2, "{q:?}");
    }
}

#[test]
fn a_clamped_cubic_hits_its_ends_and_matches_cox_de_boor_inside() {
    let knots = clamped_knots(3, 5, 8);
    assert_eq!(knots, [0, 0, 0, 0, 4, 8, 8, 8, 8]);
    let ys = [1.0, -2.0, 5.0, 0.5, 3.0];
    let curve = Curve::<1> {
        degree: 3,
        knots: knots.clone(),
        points: ys.map(|y| [y]).to_vec(),
    };
    assert_eq!(curve.evaluate(0.0), [1.0]);
    assert_eq!(curve.evaluate(8.0), [3.0]);
    for u in [4.0, 1.5, 6.25] {
        let want = naive(&ys, 3, &knots, u);
        assert!((curve.evaluate(u)[0] - want).abs() < 1e-12, "u {u}");
    }
    // Outside the knots it holds the ends.
    assert_eq!(curve.evaluate(-3.0), [1.0]);
    assert_eq!(curve.evaluate(9.0), [3.0]);

    // A Bezier segment by hand: the middle is (p0 + 3p1 + 3p2 + p3) / 8.
    let bezier = Curve::<1> {
        degree: 3,
        knots: vec![0, 0, 0, 0, 8, 8, 8, 8],
        points: vec![[0.0], [8.0], [-8.0], [16.0]],
    };
    assert!((bezier.evaluate(4.0)[0] - 2.0).abs() < 1e-12);
    let n = spline::basis(3, 3, &bezier.knots, 4.0);
    assert!(close(&n, &[0.125, 0.375, 0.375, 0.125], 1e-12));
    assert_eq!(spline::find_span(3, &knots, 5, 4.0), 4);
    assert_eq!(spline::find_span(3, &knots, 5, 3.99), 3);
}

#[test]
fn the_sample_clip_reads_and_evaluates_to_its_values() {
    let bytes = clip_payload().bytes;
    let c = Container::parse(&bytes).unwrap();
    let clip = c.clip.unwrap();
    let a = &clip.animation;
    assert_eq!(
        (a.num_frames, a.tracks.len(), a.max_frames_per_block),
        (9, 3, 256)
    );
    assert_eq!(a.data_end, a.data_len);
    assert_eq!(clip.binding.track_to_bone, [0, 1, 2]);
    assert_eq!(clip.binding.blend_hint, BlendHint::Normal);
    assert_eq!(
        clip.binding.original_skeleton_name,
        Some("Synthetic:J_root")
    );
    let t0 = &a.tracks[0];
    assert_eq!(t0.mask, [0x45, 0x52, 0xF0, 0x07]);
    assert_eq!(
        t0.position.channels,
        [Channel::Spline, Channel::Static, Channel::Spline]
    );
    assert_eq!(a.tracks[1].rotation, QuatTrack::Identity);

    let end = f64::from(a.duration);
    assert!((end - 8.0 / 30.0).abs() < 1e-6);
    let x = [
        -1.0,
        4.0 * 16384.0 / 65535.0 - 1.0,
        4.0 * 32768.0 / 65535.0 - 1.0,
    ];
    let x = [x[0], x[1], x[2], 4.0 * 49152.0 / 65535.0 - 1.0, 3.0];
    let z = [1.0, 0.0, 1.0, 0.0, 1.0];
    let knots = clamped_knots(3, 5, 8);
    for t in [0.0, end / 2.0, end, 0.1] {
        let f = a.frame_at(t);
        let s = a.sample(t);
        let want = [naive(&x, 3, &knots, f), 0.5, naive(&z, 3, &knots, f)];
        assert!(close(&s[0].translation, &want, 1e-6), "t {t}: {:?}", s[0]);
        // Linear from the identity to 90 about z, normalized.
        let half = (f / 8.0) * std::f64::consts::FRAC_PI_2;
        let blend = qs::normalize([0.0, 0.0, (f / 8.0) * H, 1.0 - f / 8.0 + (f / 8.0) * H]);
        assert!(qs::angle(s[0].rotation, blend) < 1e-3, "t {t}");
        if t == end / 2.0 {
            assert!(
                qs::angle(
                    s[0].rotation,
                    [0.0, 0.0, (half / 2.0).sin(), (half / 2.0).cos()]
                ) < 1e-3
            );
        }
        assert_eq!(s[0].scale, [2.0; 3]);
        assert_eq!(s[1], QsTransform::IDENTITY);
        assert_eq!(s[2].translation, [0.0, 0.25, 0.0]);
        assert!(qs::angle(s[2].rotation, [H, 0.0, 0.0, -H]) < 1e-3);
    }
    assert_eq!(a.sample(-1.0), a.sample(0.0));
    assert_eq!(a.sample(f64::NAN), a.sample(0.0));
    assert_eq!(a.sample(99.0), a.sample(end));

    let bytes = payload(&spec(true, Some(builder::clip()))).bytes;
    let c = Container::parse(&bytes).unwrap();
    let clip = c.clip.as_ref().unwrap();
    let skeleton = &c.skeletons[0];
    let pose = clip.local_pose(skeleton, end).unwrap();
    assert_eq!(pose, clip.animation.sample(end));
    let mut additive = clip.clone();
    additive.binding.blend_hint = BlendHint::Additive;
    assert_eq!(additive.local_pose(skeleton, 0.0), None);
    let mut far = clip.clone();
    far.binding.track_to_bone[1] = 3;
    assert_eq!(far.local_pose(skeleton, 0.0), None);
    // A partial binding keeps the reference pose on the bones it skips.
    let mut partial = clip.clone();
    partial.animation.tracks.truncate(1);
    let pose = partial.local_pose(skeleton, 0.0).unwrap();
    assert_eq!(pose[1..], skeleton.reference_pose[1..]);
}

#[test]
fn big_endian_spline_data_is_refused() {
    let mut c = builder::clip();
    c.big_endian = true;
    assert!(Container::parse(&payload(&spec(false, Some(c))).bytes).is_err());
}

#[test]
fn clip_refusals_name_their_field() {
    let built = clip_payload();
    let anim = built.animation.unwrap();
    let binding = built.binding.unwrap();
    let set = |offset: u32, v: u32| {
        let mut b = built.clone();
        b.put_u32(offset, v);
        field(&b.bytes)
    };
    assert_eq!(set(anim + 0x08, 1), "type");
    assert_eq!(set(anim + 0x14, 1), "numberOfFloatTracks");
    assert_eq!(set(anim + 0x18, 0x40), "extractedMotion");
    assert_eq!(set(anim + 0x2C, 2), "numBlocks");
    assert_eq!(set(anim + 0x80, 1), "endian");
    assert_eq!(set(anim + 0x28, 300), "numFrames");
    assert_eq!(set(anim + 0x30, 8), "maxFramesPerBlock");
    assert_eq!(set(anim + 0x34, 8), "maskAndQuantizationSize");
    assert_eq!(set(anim + 0x0C, 1.0f32.to_bits()), "duration");
    assert_eq!(set(anim + 0x40, f32::NAN.to_bits()), "frameDuration");
    assert_eq!(set(anim + 0x38, 1.0f32.to_bits()), "blockDuration");
    assert_eq!(set(anim + 0x3C, 1.0f32.to_bits()), "blockInverseDuration");
    assert_eq!(set(binding + 0x28, 0x0200_0000), "blendHint");
    assert_eq!(set(binding + 0x1C + 4, 1), "floatTrackToFloatSlotIndices");
    assert_eq!(set(anim + 0x1C + 8, 0), "annotationTracks");

    let spline = built.at(built.spline.unwrap());
    let byte = |at: usize, v: u8| {
        let mut b = built.bytes.clone();
        b[at] = v;
        refused(&b)
    };
    // Track 0's quantization byte: 8-bit positions, then POLAR32 rotations.
    assert_eq!(byte(spline, 0x44).0, "quantization");
    assert_eq!(byte(spline, 0x41).0, "quantization");
    // Track 2's position mask (static xyz) gains a w flag.
    assert_eq!(byte(spline + 9, 0x0F).0, "vector mask");
    // Quartic: track 0's degree byte, after 12 mask bytes and its count.
    assert_eq!(byte(spline + 14, 4).0, "degree");

    // Knots that aren't clamped, an extra tail, a dirty tail, bit 39.
    let mut c = builder::clip();
    if let Some(t) = c.tracks.first_mut() {
        t.position.knots = vec![0, 0, 0, 1, 4, 8, 8, 8, 8];
    }
    assert_eq!(field(&payload(&spec(false, Some(c))).bytes), "knots");
    let mut c = builder::clip();
    c.tail = 16;
    assert_eq!(field(&payload(&spec(false, Some(c))).bytes), "data padding");
    let mut c = builder::clip();
    if let Some(t) = c.tracks.last_mut() {
        t.rotation = QuatSpec::Static([0, 0, 0, 0, 0x80]);
    }
    assert_eq!(field(&payload(&spec(false, Some(c))).bytes), "rotation");
    let mut c = builder::clip();
    c.track_to_bone.pop();
    assert_eq!(
        field(&payload(&spec(false, Some(c))).bytes),
        "transformTrackToBoneIndices"
    );
    let mut c = builder::clip();
    c.track_to_bone[0] = -1;
    assert_eq!(
        field(&payload(&spec(false, Some(c))).bytes),
        "transformTrackToBoneIndices"
    );
    let mut c = builder::clip();
    c.tracks.push(TrackSpec {
        position: VecSpec {
            degree: 1,
            knots: vec![0, 0, 8, 8],
            channels: [
                ChanSpec::Spline {
                    min: 0.0,
                    max: f32::INFINITY,
                    q: vec![0, 1],
                },
                ChanSpec::Identity,
                ChanSpec::Identity,
            ],
        },
        rotation: QuatSpec::Identity,
        scale: VecSpec::identity(),
    });
    c.track_to_bone.push(0);
    assert_eq!(field(&payload(&spec(false, Some(c))).bytes), "spline max");
}

#[test]
fn packfile_refusals_name_their_field() {
    let built = payload(&spec(true, None));
    let at = |o: usize, v: &[u8]| {
        let mut b = built.bytes.clone();
        b[o..o + v.len()].copy_from_slice(v);
        refused(&b)
    };
    assert!(matches!(
        at(0, &[0x57, 0xE0, 0xE0, 0x58]),
        ("magic", ErrorKind::BadMagic { .. })
    ));
    assert_eq!(at(0x0C, &[0, 0, 0, 8]).0, "fileVersion");
    // Little-endian, 8-byte pointers, no padding reuse.
    assert_eq!(at(0x11, &[1]).0, "layoutRules");
    assert_eq!(at(0x10, &[8]).0, "layoutRules");
    assert_eq!(at(0x12, &[0]).0, "layoutRules");
    assert_eq!(at(0x28, b"hk_2012").0, "contentsVersion");
    assert_eq!(at(0x38, &[0, 0, 0, 1]).0, "flags");
    assert_eq!(at(0x3C, &[0, 1]).0, "maxPredicate");
    assert_eq!(at(0x14, &[0x7F, 0, 0, 0]).0, "numSections");
    assert_eq!(at(0x18, &[0, 0, 0, 1]).0, "contentsSectionOffset");
    assert!(matches!(
        at(0x24, &[0, 0, 0, 5]),
        (
            "contentsClassNameSectionOffset",
            ErrorKind::WrongType { .. }
        )
    ));
    // The data section's header: imports before the end, its start inside
    // the class names.
    let h = HEADER_LEN + 2 * SECTION_HEADER_LEN;
    assert_eq!(at(h + 40, &[0, 0, 0, 0]).0, "exportsOffset");
    assert_eq!(at(h + 20, &[0, 0, 0, 0x90]).0, "absoluteDataStart");
    assert_eq!(
        at(HEADER_LEN + SECTION_HEADER_LEN, b"__classnames__").1,
        ErrorKind::Duplicate
    );
    assert_eq!(at(HEADER_LEN, b"__nothing__").1, ErrorKind::Missing);

    // hkaSkeleton's signature, then its separator byte.
    let sk = built
        .bytes
        .windows(11)
        .position(|w| w == b"hkaSkeleton")
        .unwrap();
    assert_eq!(at(sk - 5, &[0x2F, 0x13, 0x03, 0xFB]).0, "class signature");
    assert_eq!(at(sk - 1, &[0x0A]).0, "class separator");

    // Fixups: a dst past the data, a duplicate source, -1 ending the table
    // early, a source off the word grid.
    let (start, from, _) = local_table(&built.bytes);
    let data_len = (from - start) as u32;
    assert_eq!(at(from + 4, &data_len.to_be_bytes()).0, "fixup dst");
    let first: Vec<u8> = built.bytes[from..from + 4].to_vec();
    assert_eq!(at(from + 8, &first).1, ErrorKind::Duplicate);
    assert_eq!(at(from, &[0xFF; 4]).0, "fixup padding");
    assert_eq!(at(from, &[0, 0, 0, 2]).0, "fixup src");

    let skeleton = built.skeletons[0];
    let set = |offset: u32, v: u32| {
        let mut b = built.clone();
        b.put_u32(offset, v);
        refused(&b.bytes)
    };
    // A pointer word that isn't zero, a capacity without its flag, a
    // negative size, a size past the data, local frames.
    assert_eq!(set(skeleton + 0x08, 4).0, "name");
    assert_eq!(set(skeleton + 0x0C + 8, 3).0, "parentIndices");
    assert_eq!(set(skeleton + 0x0C + 4, u32::MAX).0, "parentIndices");
    let past = set(skeleton + 0x24 + 4, 1000);
    assert!(
        matches!(past, ("referencePose", ErrorKind::Unsupported { .. })),
        "{past:?}"
    );
    assert_eq!(set(skeleton + 0x48 + 4, 1).0, "localFrames");

    // Two arrays over the same bytes: bones pointed at the parent indices.
    let parents = fixup_from(&built.bytes, skeleton + 0x0C);
    let bones = fixup_from(&built.bytes, skeleton + 0x18);
    let dst: Vec<u8> = built.bytes[parents + 4..parents + 8].to_vec();
    let mut b = built.bytes.clone();
    b[bones + 4..bones + 8].copy_from_slice(&dst);
    assert_eq!(refused(&b), ("bones", ErrorKind::Duplicate));

    // A parent after its child.
    let mut s = skeleton3();
    s.bones[1].1 = 2;
    let bad = payload(&PayloadSpec {
        skeletons: vec![s],
        ..PayloadSpec::default()
    });
    assert_eq!(field(&bad.bytes), "parentIndices");
}

#[test]
fn a_string_past_max_string_is_refused() {
    let named = |n: usize| {
        let mut s = skeleton3();
        s.name = "A".repeat(n).leak();
        payload(&PayloadSpec {
            skeletons: vec![s],
            ..PayloadSpec::default()
        })
    };
    let longest = named(MAX_STRING);
    let ok = Container::parse(&longest.bytes).unwrap();
    assert_eq!(ok.skeletons[0].name.map(str::len), Some(MAX_STRING));
    let (field, kind) = refused(&named(MAX_STRING + 1).bytes);
    assert_eq!(field, "name");
    assert!(matches!(kind, ErrorKind::MissingNul { .. }), "{kind:?}");
}
