//! AC.4's layouts off the Giants dump: Eruptor's hero walk read with the
//! numbers AC.4 recorded, then every gate type read in every character
//! `level.bld` with the value relations checked.
//! Ignored by default; it reads `GUAC_GIANTS_DUMP` like `giants_igz`:
//!
//! `cargo test -p guac-formats --release --test giants_layouts -- --ignored --nocapture`

mod common;

use std::collections::BTreeMap;
use std::path::Path;

use common::usrdir;
use guac_formats::FormatError;
use guac_formats::iga::Archive;
use guac_formats::igz::Igz;
use guac_formats::igz::layouts::{
    self, Actor2, AnimationInfo, AnimationInfoList, BlendMatrixSelect, EdgeGeometry,
    EdgeGeometryAttr, EdgeGeometrySegment, Hero, Image2, IntList, Layout, NodeList, Payload,
    PhysicsAnimation, PhysicsBody, PhysicsCombinerLink, PhysicsModel, RuntimeTechniqueInstance,
    Skeleton2, SkeletonBone, SkeletonBoneList,
};

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

/// The plain mip chain, rounded up to 128 bytes.
fn mip_chain(image: &Image2<'_>) -> Option<usize> {
    let name = image.format_name()?;
    let (block, bytes) = match name {
        n if n.starts_with("dxt1") => (4, 8),
        n if n.starts_with("dxt5") => (4, 16),
        _ => (1, 1),
    };
    let (mut w, mut h, mut total) = (image.width as usize, image.height as usize, 0);
    for _ in 0..image.level_count {
        total += w.div_ceil(block) * h.div_ceil(block) * bytes;
        w = (w / 2).max(1);
        h = (h / 2).max(1);
    }
    Some(total.next_multiple_of(128))
}

fn len(m: Option<guac_formats::igz::MemoryRef<'_>>) -> usize {
    m.map_or(0, |m| m.data.len())
}

/// The relations AC.4 checked between a segment's size fields and buffers.
fn segment_holds(s: &EdgeGeometrySegment<'_>) -> bool {
    let sum = |v: &[u16]| v.iter().map(|&x| usize::from(x)).sum::<usize>();
    len(s.spu_config_info) == 16
        && usize::from(s.indexes_sizes[0]) == len(s.indexes)
        && s.indexes_sizes[1] == 0
        && sum(&s.spu_vertexes_sizes[..3]) == len(s.spu_vertexes0)
        && sum(&s.spu_vertexes_sizes[3..]) == len(s.spu_vertexes1)
        && s.rsx_only_vertexes_size as usize == len(s.rsx_only_vertexes)
        && s.skin_matrices_sizes.iter().all(|x| x % 48 == 0)
        && sum(&s.skin_indexes_and_weights_sizes) == len(s.skin_indexes_and_weights)
        && usize::from(s.spu_output_stream_desc_size) == len(s.spu_output_stream_desc)
        && usize::from(s.rsx_only_stream_desc_size) == len(s.rsx_only_stream_desc)
        && s.counts().is_some_and(|(_, i)| i % 3 == 0)
}

#[derive(Default)]
struct Tally {
    read: BTreeMap<&'static str, u64>,
    failures: Vec<String>,
    counts: BTreeMap<String, u64>,
}

impl Tally {
    fn bump(&mut self, key: impl Into<String>) {
        *self.counts.entry(key.into()).or_default() += 1;
    }

    fn all<'a, T: Layout<'a>>(&mut self, name: &str, igz: &Igz<'a>) -> Vec<T> {
        let mut out = Vec::new();
        for (i, read) in layouts::read_all::<T>(igz) {
            match read {
                Ok(v) => {
                    *self.read.entry(T::TYPE).or_default() += 1;
                    out.push(v);
                }
                Err(e) => self.failures.push(format!("{name}: {} #{i}: {e}", T::TYPE)),
            }
        }
        out
    }

    fn take(&mut self, name: &str, bytes: &[u8]) {
        let igz = Igz::parse(bytes).unwrap();
        for image in self.all::<Image2>(name, &igz) {
            self.bump(format!(
                "igImage2 format {}",
                image.format_name().unwrap_or("unknown")
            ));
            let data = image.data.map(|h| h.block.data.len());
            if data.is_some() && data == mip_chain(&image) {
                self.bump("igImage2 data == mip chain rounded to 128");
            } else {
                self.failures
                    .push(format!("{name}: {:?} data {data:?}", image.name));
            }
        }
        for s in self.all::<EdgeGeometrySegment>(name, &igz) {
            if segment_holds(&s) {
                self.bump("segment size relations hold");
            } else {
                self.failures
                    .push(format!("{name}: segment relations {s:?}"));
            }
            if s.is_skinned() {
                self.bump("segments skinned");
            }
        }
        self.all::<EdgeGeometry>(name, &igz);
        self.all::<EdgeGeometryAttr>(name, &igz);
        self.all::<RuntimeTechniqueInstance>(name, &igz);
        self.all::<NodeList>(name, &igz);
        self.all::<Actor2>(name, &igz);
        for select in self.all::<BlendMatrixSelect>(name, &igz) {
            if select.blend_matrix_indices.is_some() {
                self.bump("igBlendMatrixSelect with _blendMatrixIndices");
            }
        }
        self.all::<IntList>(name, &igz);
        for skeleton in self.all::<Skeleton2>(name, &igz) {
            let bones = skeleton
                .bone_list
                .map(|l| SkeletonBoneList::read(&igz, l).unwrap().bones.len());
            self.bump(format!(
                "igSkeleton2 bones present: {}",
                bones.is_some() && !skeleton.inverse_joints.is_empty()
            ));
        }
        self.all::<SkeletonBoneList>(name, &igz);
        self.all::<SkeletonBone>(name, &igz);
        self.all::<PhysicsCombinerLink>(name, &igz);
        for body in self.all::<PhysicsBody>(name, &igz) {
            self.bump(payload("tfbPhysicsBody", body.body.data_info.payload()));
        }
        for anim in self.all::<PhysicsAnimation>(name, &igz) {
            self.bump(payload(
                "tfbPhysicsAnimation",
                anim.info.data_info.payload(),
            ));
        }
        self.all::<AnimationInfoList>(name, &igz);
        self.all::<AnimationInfo>(name, &igz);
        for (i, model) in layouts::read_all::<PhysicsModel>(&igz) {
            let Ok(model) = model else { continue };
            // Heroes are the models with a combiner (a skeleton).
            let body = model.model.tfb_body.map(|b| PhysicsBody::read(&igz, b));
            if !matches!(body, Some(Ok(b)) if b.body.combiner_prototype.is_some()) {
                continue;
            }
            match Hero::read(&igz, i) {
                Ok(_) => self.bump("hero walks"),
                Err(e) => self.failures.push(format!("{name}: hero #{i}: {e}")),
            }
        }
    }
}

fn payload(ty: &str, p: Option<Payload<'_>>) -> String {
    let kind = match p {
        Some(Payload::Havok(_)) => "Havok",
        Some(Payload::LipSync(_)) => "lip sync",
        Some(Payload::Other(_)) => "other",
        None => "none",
    };
    format!("{ty} _data: {kind}")
}

fn eruptor<'a>(igz: &Igz<'a>) -> Result<Hero<'a>, FormatError> {
    let (index, _) = layouts::read_all::<PhysicsModel>(igz)
        .find(|(_, m)| {
            m.as_ref().is_ok_and(|m| {
                m.model
                    .info
                    .name
                    .is_some_and(|n| n.ends_with("/Eruptor.mdl"))
            })
        })
        .unwrap_or_else(|| {
            let names: Vec<_> = layouts::read_all::<PhysicsModel>(igz)
                .map(|(i, m)| (i, m.map(|m| m.model.info.name)))
                .collect();
            panic!("no Eruptor.mdl among {names:?}")
        });
    Hero::read(igz, index)
}

#[test]
#[ignore = "reads the Giants dump at GUAC_GIANTS_DUMP"]
fn eruptor_reads_with_the_recorded_numbers() {
    let bytes = level_bld(&usrdir().join("character/009_eruptor.bld"));
    let igz = Igz::parse(&bytes).unwrap();
    let hero = eruptor(&igz).unwrap();

    let body = &hero.body.body;
    let segments: Vec<&EdgeGeometrySegment<'_>> = hero.segments.values().collect();
    let (vertexes, triangles) = segments.iter().fold((0, 0), |(v, t), s| {
        let (nv, ni) = s.counts().unwrap();
        (v + usize::from(nv), t + usize::from(ni) / 3)
    });
    let havok = |p: Option<Payload<'_>>| match p {
        Some(Payload::Havok(b)) => b.len(),
        other => panic!("not Havok: {other:?}"),
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
                .is_some_and(|n| n.ends_with("Eruptor_Idle.ma"))
        })
        .expect("Eruptor_Idle.ma");
    let palette_sizes: Vec<usize> = hero
        .palettes
        .values()
        .map(|p| {
            p.indices
                .and_then(|i| hero.int_lists.get(&i))
                .map_or(0, |l| l.values.len())
        })
        .collect();
    let images = layouts::read_all::<Image2>(&igz).count();
    let slots = &hero.clip_slots;
    let wrapped = hero
        .clips
        .values()
        .filter(|c| c.animation.is_some())
        .count();
    let animations = layouts::read_all::<PhysicsAnimation>(&igz).count();
    let infos = layouts::read_all::<AnimationInfo>(&igz).count();

    println!(
        "Eruptor (009_eruptor.bld level.bld, {} objects):",
        igz.objects().len()
    );
    println!(
        "  model {:?}, body {:?}",
        hero.model.model.info.name, body.data_info.info.name
    );
    println!(
        "  actor {:?}, skeleton {:?}",
        hero.actor.group.node.name, hero.skeleton.name
    );
    println!(
        "  {} bones, {} inverse joints, root parent {}",
        hero.bones.len(),
        hero.skeleton.inverse_joints.len(),
        hero.bones[0].parent_index
    );
    println!(
        "  {} meshes, {} geometries, {} segments ({} skinned), {vertexes} vertices, {triangles} triangles",
        hero.meshes.len(),
        hero.geometries.len(),
        segments.len(),
        segments.iter().filter(|s| s.is_skinned()).count()
    );
    for m in &hero.meshes {
        let segs: Vec<(u16, u16)> = hero
            .mesh_segments(m)
            .map(|(_, s)| s.counts().unwrap())
            .collect();
        println!(
            "    mesh {:?}: palette {:?}, segments (vertices, indexes) {segs:?}",
            m.technique.node.name, m.palette
        );
    }
    println!("  palettes: {palette_sizes:?} slots");
    println!(
        "  clip table: {} slots, {} filled, {wrapped} wrap a tfbPhysicsAnimation; file: {infos} AnimationInfo, {animations} tfbPhysicsAnimation",
        slots.len(),
        hero.clips.len()
    );
    println!(
        "  Havok skeleton {} bytes, {} clips, idle {} bytes, {} igImage2",
        havok(body.data_info.payload()),
        hero.clips.len(),
        havok(idle.info.data_info.payload()),
        images
    );

    assert_eq!(hero.bones.len(), 34);
    assert_eq!(hero.skeleton.inverse_joints.len(), 33);
    assert_eq!(hero.meshes.len(), 2);
    assert_eq!(segments.len(), 3);
    assert!(segments.iter().all(|s| s.is_skinned() && segment_holds(s)));
    assert_eq!((vertexes, triangles), (2_861, 4_812));
    assert_eq!(havok(body.data_info.payload()), 3_520);
    assert_eq!(havok(idle.info.data_info.payload()), 6_880);
    assert_eq!((slots.len(), hero.clips.len(), wrapped), (185, 32, 31));
    assert_eq!(images, 44);
    // Every skinned bone's blend index has an inverse joint.
    let max_blend = hero
        .bones
        .iter()
        .map(|b| b.blend_matrix_index)
        .max()
        .unwrap();
    assert_eq!(max_blend + 1, hero.skeleton.inverse_joints.len() as i32);
}

#[test]
#[ignore = "reads the Giants dump at GUAC_GIANTS_DUMP"]
fn every_character_level_reads_by_the_layouts() {
    let usrdir = usrdir();
    let mut paths: Vec<_> = std::fs::read_dir(usrdir.join("character"))
        .unwrap()
        .map(|e| e.unwrap().path())
        .filter(|p| p.extension().is_some_and(|e| e == "bld"))
        .collect();
    paths.sort();
    let mut tally = Tally::default();
    for path in &paths {
        let name = path.file_name().unwrap().to_string_lossy().into_owned();
        tally.take(&name, &level_bld(path));
    }
    println!("{} character level.bld files:", paths.len());
    for (ty, n) in &tally.read {
        println!("  {ty}: {n} read");
    }
    for (key, n) in &tally.counts {
        println!("  {key}: {n}");
    }
    println!("  failures: {}", tally.failures.len());
    for f in tally.failures.iter().take(40) {
        println!("    {f}");
    }
    assert!(!paths.is_empty());
    assert!(tally.failures.is_empty());
}
