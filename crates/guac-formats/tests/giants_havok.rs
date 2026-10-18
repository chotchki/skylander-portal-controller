//! AC.8's Havok decode off the Giants dump: Eruptor's skeleton against his
//! igSkeleton2 and his idle evaluated, then every Havok payload in every
//! character archive. Ignored by default; it reads `GUAC_GIANTS_DUMP`:
//!
//! `cargo test -p guac-formats --release --test giants_havok -- --ignored --nocapture`

mod common;

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use common::usrdir;
use guac_formats::havok::spline::{Channel, QuatTrack};
use guac_formats::havok::{self, BlendHint, Container, QsTransform, qs};
use guac_formats::iga::Archive;
use guac_formats::igz::Igz;
use guac_formats::igz::layouts::{
    AnimationSystem2, Layout, Payload, PhysicsAnimation, PhysicsBody, Skeleton2, SkeletonBone,
    SkeletonBoneList, read_all,
};

const SKELETON: &str = "Eruptor/Rig/Eruptor.ma";
const IDLE: &str = "Eruptor/Eruptor_Idle.ma";
/// Decision 5's skeleton tolerance: translation relative to the extent, and radians.
const BIND: f64 = 1e-4;

type M4 = [[f64; 4]; 4];

fn level_bld(path: &Path) -> Vec<u8> {
    let bytes = std::fs::read(path).unwrap();
    let archive = Archive::parse(&bytes).unwrap();
    let entry = archive
        .entries()
        .iter()
        .find(|e| e.name().ends_with("level.bld"))
        .unwrap_or_else(|| panic!("no level.bld in {}", path.display()));
    archive.read(entry).unwrap().into_owned()
}

fn inverse(m: M4) -> M4 {
    let mut a: Vec<[f64; 8]> = (0..4)
        .map(|r| {
            std::array::from_fn(|c| {
                if c < 4 {
                    m[r][c]
                } else {
                    f64::from(u8::from(c - 4 == r))
                }
            })
        })
        .collect();
    for c in 0..4 {
        let p = (c..4)
            .max_by(|x, y| a[*x][c].abs().total_cmp(&a[*y][c].abs()))
            .unwrap();
        a.swap(c, p);
        let pv = a[c][c];
        a[c] = a[c].map(|x| x / pv);
        for r in 0..4 {
            if r != c {
                let f = a[r][c];
                let row = a[c];
                for (x, y) in a[r].iter_mut().zip(row) {
                    *x -= f * y;
                }
            }
        }
    }
    std::array::from_fn(|r| std::array::from_fn(|c| a[r][c + 4]))
}

fn transpose(m: M4) -> M4 {
    std::array::from_fn(|r| std::array::from_fn(|c| m[c][r]))
}

/// The angle of `A^T B` for two rotation blocks; atan2 keeps f32 noise
/// near zero from reading as 4e-4 rad.
fn rot_angle(a: M4, b: M4) -> f64 {
    let r: [[f64; 3]; 3] =
        std::array::from_fn(|i| std::array::from_fn(|j| (0..3).map(|k| a[k][i] * b[k][j]).sum()));
    let c = (r[0][0] + r[1][1] + r[2][2] - 1.0) / 2.0;
    let s =
        ((r[2][1] - r[1][2]).powi(2) + (r[0][2] - r[2][0]).powi(2) + (r[1][0] - r[0][1]).powi(2))
            .sqrt()
            / 2.0;
    s.atan2(c)
}

fn dist(a: [f64; 3], b: [f64; 3]) -> f64 {
    (0..3).map(|i| (a[i] - b[i]).powi(2)).sum::<f64>().sqrt()
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

/// The Alchemy skeleton a body skins with.
struct Alchemy<'a> {
    skeleton: Skeleton2<'a>,
    bones: Vec<SkeletonBone<'a>>,
}

impl<'a> Alchemy<'a> {
    fn of(igz: &Igz<'a>, body: &PhysicsBody<'a>) -> Option<Self> {
        let system = AnimationSystem2::read(igz, body.body.combiner_prototype?).ok()?;
        let skeleton = Skeleton2::read(igz, system.skeleton?).ok()?;
        let bones = SkeletonBoneList::read(igz, skeleton.bone_list?)
            .ok()?
            .bones
            .into_iter()
            .map(|b| SkeletonBone::read(igz, b).unwrap())
            .collect();
        Some(Self { skeleton, bones })
    }

    fn index(&self, name: &str) -> Option<usize> {
        self.bones.iter().position(|b| b.name == Some(name))
    }
}

#[derive(Debug, Default)]
struct BindCheck {
    passing: usize,
    /// (bone, translation relative to the extent, radians).
    failing: Vec<(String, f64, f64)>,
    worst_passing: (f64, f64),
    names_match: bool,
    parents_match: bool,
    same_order: bool,
    local_translation: f64,
    extent: f64,
}

/// The Havok reference pose composed and inverted, against `_inverseJointArray`.
fn bind_check(sk: &havok::Skeleton<'_>, ig: &Alchemy<'_>) -> BindCheck {
    let names: Vec<&str> = sk.bones.iter().map(|b| b.name.unwrap()).collect();
    let ig_names: Vec<&str> = ig.bones.iter().map(|b| b.name.unwrap()).collect();
    let extra: Vec<&&str> = ig_names.iter().filter(|n| !names.contains(n)).collect();
    let mut out = BindCheck {
        names_match: extra == [&"Unnamed bone"] && names.iter().all(|n| ig_names.contains(n)),
        same_order: names
            == ig_names
                .iter()
                .copied()
                .filter(|n| names.contains(n))
                .collect::<Vec<_>>(),
        parents_match: true,
        ..BindCheck::default()
    };
    if !out.names_match {
        return out;
    }
    let model = sk.bind_pose();
    out.extent = diagonal(model.iter().map(|m| &m.translation));
    let diag = if out.extent > 1e-9 { out.extent } else { 1.0 };
    for (i, n) in names.iter().enumerate() {
        let b = &ig.bones[ig.index(n).unwrap()];
        let hp = usize::try_from(sk.parents[i]).ok().map(|p| names[p]);
        let ip = usize::try_from(b.parent_index)
            .ok()
            .map(|p| ig_names[p])
            .filter(|p| *p != "Unnamed bone");
        out.parents_match &= hp == ip;
        let t = sk.reference_pose[i].translation;
        let local = dist(t, b.translation.map(f64::from));
        out.local_translation = out.local_translation.max(local);
        let j = ig.skeleton.inverse_joints[b.blend_matrix_index as usize].map(|r| r.map(f64::from));
        let w = model[i].matrix();
        let world = inverse(transpose(j));
        let e = dist(
            [world[0][3], world[1][3], world[2][3]],
            model[i].translation,
        ) / diag;
        let a = rot_angle(world, w);
        if e <= BIND && a <= BIND {
            out.passing += 1;
            out.worst_passing = (out.worst_passing.0.max(e), out.worst_passing.1.max(a));
        } else {
            out.failing.push(((*n).to_owned(), e, a));
        }
    }
    out
}

/// Every Havok payload in a level: (body?, object index, name, bytes).
fn payloads<'a>(igz: &Igz<'a>) -> Vec<(bool, usize, &'a str, &'a [u8])> {
    let mut out = Vec::new();
    for (i, b) in read_all::<PhysicsBody>(igz) {
        let b = b.unwrap();
        if let Some(Payload::Havok(bytes)) = b.body.data_info.payload() {
            out.push((true, i, b.body.data_info.info.name.unwrap_or(""), bytes));
        }
    }
    for (i, a) in read_all::<PhysicsAnimation>(igz) {
        let a = a.unwrap();
        if let Some(Payload::Havok(bytes)) = a.info.data_info.payload() {
            out.push((false, i, a.info.data_info.info.name.unwrap_or(""), bytes));
        }
    }
    out
}

fn kinds(clip: &havok::Clip<'_>) -> BTreeMap<String, usize> {
    let mut out = BTreeMap::new();
    let mut bump = |k: String| *out.entry(k).or_insert(0) += 1;
    for t in &clip.animation.tracks {
        for (name, v) in [("position", &t.position), ("scale", &t.scale)] {
            let k = match &v.curve {
                Some(c) => format!("spline degree {}", c.degree),
                None if v.channels.contains(&Channel::Static) => "static".into(),
                None => "identity".into(),
            };
            bump(format!("{name} {k}"));
        }
        bump(match &t.rotation {
            QuatTrack::Identity => "rotation identity".into(),
            QuatTrack::Static(_) => "rotation static".into(),
            QuatTrack::Spline(c) => format!("rotation spline degree {}", c.degree),
        });
    }
    out
}

fn model_at(sk: &havok::Skeleton<'_>, clip: &havok::Clip<'_>, t: f64) -> Vec<QsTransform> {
    sk.model_pose(&clip.local_pose(sk, t).unwrap()).unwrap()
}

#[test]
#[ignore = "reads the Giants dump from GUAC_GIANTS_DUMP"]
fn eruptor_skeleton_and_idle() {
    let bytes = level_bld(&usrdir().join("character/009_eruptor.bld"));
    let igz = Igz::parse(&bytes).unwrap();
    let all = payloads(&igz);
    let (_, body_index, _, skel_bytes) = *all.iter().find(|p| p.2.ends_with(SKELETON)).unwrap();
    let (_, _, _, idle_bytes) = *all.iter().find(|p| p.2.ends_with(IDLE)).unwrap();
    assert_eq!((skel_bytes.len(), idle_bytes.len()), (3_520, 6_880));

    let sc = Container::parse(skel_bytes).unwrap();
    println!(
        "skeleton payload: fixups {:?}, variants {:?}",
        sc.fixups, sc.variants
    );
    assert!(sc.clip.is_none());
    let sk = &sc.skeletons[0];
    assert_eq!((sk.name, sk.bones.len()), (Some("J_root"), 33));
    assert!(sk.bones.iter().all(|b| !b.lock_translation));
    let scale = sk
        .reference_pose
        .iter()
        .flat_map(|q| q.scale)
        .map(|s| (s - 1.0).abs())
        .fold(0.0, f64::max);
    assert!(scale < 1e-6, "{scale}");
    let rot_len = sk
        .reference_pose
        .iter()
        .map(|q| (qs::length(q.rotation) - 1.0).abs())
        .fold(0.0, f64::max);
    assert!(rot_len < 1e-7, "{rot_len}");

    let body = PhysicsBody::read(&igz, body_index).unwrap();
    let ig = Alchemy::of(&igz, &body).unwrap();
    assert_eq!((ig.bones.len(), ig.skeleton.inverse_joints.len()), (34, 33));
    let check = bind_check(sk, &ig);
    println!(
        "bind: {} of 33 pass, extent {:.4}, worst passing {:.2e} rel / {:.2e} rad, local translation vs igSkeletonBone {:.2e}, same order {}",
        check.passing,
        check.extent,
        check.worst_passing.0,
        check.worst_passing.1,
        check.local_translation,
        check.same_order
    );
    for f in &check.failing {
        println!("  fails: {} {:.4} rel, {:.4} rad", f.0, f.1, f.2);
    }
    assert!(check.names_match && check.parents_match && !check.same_order);
    assert_eq!(check.passing, 28);
    let failing: BTreeSet<&str> = check.failing.iter().map(|f| f.0.as_str()).collect();
    let want = [
        "J_hand_B1",
        "J_hand_F1",
        "J_hand_U1",
        "J_palm_R",
        "J_wrist_R",
    ];
    assert_eq!(failing, want.into_iter().collect());
    assert!(check.failing.iter().all(|f| (f.2 - 0.3631).abs() < 1e-3));
    assert!(check.worst_passing.0 < 1e-6 && check.worst_passing.1 < 1e-6);
    assert!(check.local_translation < 2e-4);

    let cc = Container::parse(idle_bytes).unwrap();
    println!("clip payload: fixups {:?}", cc.fixups);
    let clip = cc.clip.as_ref().unwrap();
    let a = &clip.animation;
    let fps = 1.0 / f64::from(a.frame_duration);
    println!(
        "idle: {:.4} s, {} frames at {fps:.2} fps, {} tracks, data {} bytes ending at {}, blend {:?}",
        a.duration,
        a.num_frames,
        a.tracks.len(),
        a.data_len,
        a.data_end,
        clip.binding.blend_hint
    );
    assert_eq!(
        (a.num_frames, a.tracks.len(), a.data_len, a.data_end),
        (41, 33, 5_776, 5_764)
    );
    assert!((f64::from(a.duration) - 4.0 / 3.0).abs() < 1e-5 && (fps - 30.0).abs() < 1e-3);
    assert_eq!(clip.binding.track_to_bone, (0..33).collect::<Vec<u16>>());
    assert_eq!(clip.binding.blend_hint, BlendHint::Normal);
    assert!(a.tracks.iter().all(|t| t.mask[0] == 0x45));
    let k = kinds(clip);
    println!("idle tracks: {k:?}");
    let get = |s: &str| k.get(s).copied().unwrap_or(0);
    assert_eq!(
        (
            get("position spline degree 3"),
            get("position spline degree 1")
        ),
        (10, 5)
    );
    assert_eq!(get("position static") + get("position identity"), 18);
    assert_eq!(get("rotation spline degree 3"), 20);
    assert_eq!((get("rotation static"), get("rotation identity")), (6, 7));
    assert_eq!(get("scale identity"), 33);

    let end = f64::from(a.duration);
    let names: Vec<&str> = sk.bones.iter().map(|b| b.name.unwrap()).collect();
    let (m0, m1) = (model_at(sk, clip, 0.0), model_at(sk, clip, end));
    let loop_t = m0
        .iter()
        .zip(&m1)
        .map(|(x, y)| dist(x.translation, y.translation))
        .fold(0.0, f64::max);
    let loop_r = m0
        .iter()
        .zip(&m1)
        .map(|(x, y)| qs::angle(x.rotation, y.rotation))
        .fold(0.0, f64::max);
    println!("loop: model translation {loop_t:.2e}, rotation {loop_r:.2e} rad");
    assert!(loop_t < 1e-7 && loop_r < 1e-6);

    let steps = 4 * (a.num_frames as usize - 1);
    let samples: Vec<Vec<QsTransform>> = (0..=steps)
        .map(|i| model_at(sk, clip, end * i as f64 / steps as f64))
        .collect();
    let track = |n: &str| {
        let i = names.iter().position(|x| *x == n).unwrap();
        samples.iter().map(|s| s[i].translation).collect::<Vec<_>>()
    };
    let (left, right, root) = (track("J_ankle_L"), track("J_ankle_R"), track("J_root"));
    let (fl, fr) = (diagonal(&left), diagonal(&right));
    let bob = root.iter().map(|p| p[1]).fold(f64::MIN, f64::max)
        - root.iter().map(|p| p[1]).fold(f64::MAX, f64::min);
    println!(
        "feet over {} samples: ankle L {fl:.4}, R {fr:.4}; root bobs {bob:.4}",
        steps + 1
    );
    assert!(fl < 0.005 && fr < 0.005 && bob > 0.1);
    let mid = model_at(sk, clip, end / 2.0);
    println!(
        "root height: t0 {:.3}, mid {:.3}, bind {:.3}",
        m0[0].translation[1],
        mid[0].translation[1],
        sk.bind_pose()[0].translation[1]
    );
}

#[test]
#[ignore = "reads the Giants dump from GUAC_GIANTS_DUMP"]
fn every_character_havok_payload() {
    let dir = usrdir().join("character");
    let mut paths: Vec<_> = std::fs::read_dir(&dir)
        .unwrap()
        .map(|e| e.unwrap().path())
        .collect();
    paths.sort();
    let magic = havok::MAGIC;
    let mut counts: BTreeMap<String, u64> = BTreeMap::new();
    let bump = |c: &mut BTreeMap<String, u64>, k: String, n: u64| *c.entry(k).or_default() += n;
    let mut failures = Vec::new();
    let (mut skeletons, mut clips, mut empty, mut raw, mut levels) = (0, 0, 0, 0, 0);
    let mut bind_all_pass = 0;
    let mut bind_failing = Vec::new();
    let mut worst_prenorm: (f64, String) = (0.0, String::new());
    for path in &paths {
        let name = path.file_name().unwrap().to_string_lossy().into_owned();
        if !(name.ends_with(".bld") || name.ends_with(".arc")) {
            continue;
        }
        let bytes = std::fs::read(path).unwrap();
        let archive = Archive::parse(&bytes).unwrap();
        for entry in archive.entries() {
            let data = archive.read(entry).unwrap();
            raw += data.windows(8).filter(|w| *w == magic).count();
            if !(name.ends_with(".bld") && entry.name().ends_with("level.bld")) {
                continue;
            }
            levels += 1;
            let igz = Igz::parse(&data).unwrap();
            for (is_body, index, obj, payload) in payloads(&igz) {
                let c = match Container::parse(payload) {
                    Ok(c) => c,
                    Err(e) => {
                        failures.push(format!("{name} {obj}: {e}"));
                        continue;
                    }
                };
                for v in &c.variants {
                    bump(&mut counts, format!("variant {:?}", v.class), 1);
                }
                bump(&mut counts, "fixups local".into(), c.fixups.local as u64);
                bump(&mut counts, "fixups global".into(), c.fixups.global as u64);
                bump(
                    &mut counts,
                    "fixups virtual".into(),
                    c.fixups.virtual_ as u64,
                );
                if is_body {
                    if c.skeletons.len() != 1 || c.clip.is_some() {
                        failures.push(format!(
                            "{name} {obj}: body holds {} skeletons",
                            c.skeletons.len()
                        ));
                        continue;
                    }
                    skeletons += 1;
                    let body = PhysicsBody::read(&igz, index).unwrap();
                    let Some(ig) = Alchemy::of(&igz, &body) else {
                        failures.push(format!("{name} {obj}: no igSkeleton2"));
                        continue;
                    };
                    let check = bind_check(&c.skeletons[0], &ig);
                    bump(
                        &mut counts,
                        format!("bind same order {}", check.same_order),
                        1,
                    );
                    if !(check.names_match && check.parents_match) {
                        failures.push(format!("{name} {obj}: bones don't pair by name"));
                    } else if check.failing.is_empty() {
                        bind_all_pass += 1;
                    } else {
                        let n = c.skeletons[0].bones.len();
                        bind_failing.push(format!("{name} {obj}: {} of {n}", check.passing));
                    }
                    continue;
                }
                let Some(clip) = &c.clip else {
                    if c.skeletons.is_empty() {
                        empty += 1;
                    } else {
                        failures.push(format!("{name} {obj}: skeleton in a clip slot"));
                    }
                    continue;
                };
                clips += 1;
                let a = &clip.animation;
                for (k, n) in kinds(clip) {
                    bump(&mut counts, k, n as u64);
                }
                bump(
                    &mut counts,
                    "tracks 0x45".into(),
                    a.tracks.iter().filter(|t| t.mask[0] == 0x45).count() as u64,
                );
                bump(&mut counts, format!("fps {:.2}", 1.0 / a.frame_duration), 1);
                bump(
                    &mut counts,
                    format!("blend {:?}", clip.binding.blend_hint),
                    1,
                );
                let identity = clip
                    .binding
                    .track_to_bone
                    .iter()
                    .enumerate()
                    .all(|(i, b)| usize::from(*b) == i);
                bump(&mut counts, format!("binding identity {identity}"), 1);
                let end = f64::from(a.duration);
                for t in [0.0, end / 2.0, end] {
                    let f = a.frame_at(t);
                    let s = a.sample(t);
                    let finite = s.iter().all(|q| {
                        q.translation
                            .iter()
                            .chain(&q.rotation)
                            .chain(&q.scale)
                            .all(|x| x.is_finite())
                    });
                    if !finite {
                        failures.push(format!("{name} {obj}: non-finite sample at {t}"));
                    }
                    for tr in &a.tracks {
                        if let QuatTrack::Spline(c) = &tr.rotation {
                            let dev = (1.0 - qs::length(c.evaluate(f))).abs();
                            if dev > worst_prenorm.0 {
                                worst_prenorm = (dev, format!("{name} {obj}"));
                            }
                        }
                    }
                }
            }
        }
    }
    println!("{levels} level.bld, {raw} packfile magics in every entry");
    println!(
        "skeletons {skeletons}, clips {clips}, empty {empty}, failures {}",
        failures.len()
    );
    println!("bind: {bind_all_pass} of {skeletons} pass on every bone");
    for b in &bind_failing {
        println!("  bind {b}");
    }
    println!(
        "worst pre-normalize quaternion length deviation {:.3} ({})",
        worst_prenorm.0, worst_prenorm.1
    );
    for (k, v) in &counts {
        println!("  {k}: {v}");
    }
    for f in failures.iter().take(40) {
        println!("FAIL {f}");
    }
    assert!(failures.is_empty());
    assert_eq!((levels, skeletons, clips, empty), (158, 120, 3_920, 12));
    assert_eq!(raw, 4_057);
    assert_eq!(counts["tracks 0x45"], 108_527);
    assert_eq!(bind_all_pass, 107);
}
