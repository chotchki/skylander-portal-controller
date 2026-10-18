//! AC.7's Edge decode off the Giants dump: Eruptor's three segments, then
//! every segment in every character archive, with the findings' checks.
//! Ignored by default; it reads `GUAC_GIANTS_DUMP` like `giants_igz`:
//!
//! `cargo test -p guac-formats --release --test giants_edge -- --ignored --nocapture`

mod common;

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::path::Path;

use common::usrdir;
use guac_formats::FormatError;
use guac_formats::edge::{
    BoneMap, Kind, Segment, Semantic, Stream, x11y11z10n_rsx, x11y11z10n_spu,
};
use guac_formats::iga::Archive;
use guac_formats::igz::Igz;
use guac_formats::igz::layouts::{
    self, Actor2, AnimationSystem2, BlendMatrixSelect, EdgeGeometry, EdgeGeometryAttr,
    EdgeGeometrySegment, Group, Hero, IntList, Layout, Node, NodeList, ObjectRef, PhysicsModel,
    RuntimeTechniqueInstance, Skeleton2, SkeletonBone, SkeletonBoneList, is_a,
};

type Box3 = [[f32; 3]; 2];

/// What a geometry sits under: the nearest node with an `igAABox` bound,
/// the nearest skin palette and the nearest actor.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Default)]
struct Ctx {
    bound: Option<usize>,
    select: Option<usize>,
    actor: Option<usize>,
}

fn aabox(igz: &Igz<'_>, r: ObjectRef<'_>) -> Option<Box3> {
    let ObjectRef::Object(i) = r else { return None };
    let o = igz.objects()[i];
    if igz.type_name(&o) != "igAABox" {
        return None;
    }
    let bytes = igz.bytes(o.at.offset_by(8)?, 24, "igAABox").ok()?;
    let f: Vec<f32> = bytes
        .as_chunks::<4>()
        .0
        .iter()
        .map(|c| f32::from_be_bytes(*c))
        .collect();
    Some([[f[0], f[1], f[2]], [f[3], f[4], f[5]]])
}

/// Every geometry's contexts, walking each scene graph down from its roots.
fn contexts(igz: &Igz<'_>) -> Result<BTreeMap<usize, BTreeSet<Ctx>>, FormatError> {
    let ty = |i: usize| igz.type_name(&igz.objects()[i]);
    let nodes: Vec<usize> = (0..igz.objects().len())
        .filter(|&i| is_a(ty(i), "igNode"))
        .collect();
    let mut children: HashMap<usize, Vec<usize>> = HashMap::new();
    let mut child = HashSet::new();
    for &n in &nodes {
        if is_a(ty(n), "igGroup")
            && let Some(list) = Group::read(igz, n)?.child_list
        {
            let kids = NodeList::read(igz, list)?.nodes;
            child.extend(kids.iter().copied());
            children.insert(n, kids);
        }
    }
    let mut out: BTreeMap<usize, BTreeSet<Ctx>> = BTreeMap::new();
    let mut seen = HashSet::new();
    let mut stack: Vec<(usize, Ctx)> = nodes
        .iter()
        .filter(|n| !child.contains(n))
        .map(|&n| (n, Ctx::default()))
        .collect();
    while let Some((n, mut ctx)) = stack.pop() {
        if !seen.insert((n, ctx)) {
            continue;
        }
        if aabox(igz, Node::read(igz, n)?.bound).is_some() {
            ctx.bound = Some(n);
        }
        if is_a(ty(n), BlendMatrixSelect::TYPE) {
            ctx.select = Some(n);
        }
        if is_a(ty(n), Actor2::TYPE) {
            ctx.actor = Some(n);
        }
        if is_a(ty(n), RuntimeTechniqueInstance::TYPE) {
            if let Some(attr) = RuntimeTechniqueInstance::read(igz, n)?.geom_attr
                && let Some(g) = EdgeGeometryAttr::read(igz, attr)?.geometry
            {
                out.entry(g).or_default().insert(ctx);
            }
            continue;
        }
        for &c in children.get(&n).into_iter().flatten() {
            stack.push((c, ctx));
        }
    }
    Ok(out)
}

/// The palette's blend indices, its bone map and the inverse joint count
/// under `ctx`.
fn bone_map(igz: &Igz<'_>, ctx: &Ctx) -> Result<(Vec<i32>, BoneMap, usize), String> {
    let select = ctx.select.ok_or("no igBlendMatrixSelect above")?;
    let list = BlendMatrixSelect::read(igz, select)
        .map_err(|e| e.to_string())?
        .blend_matrix_indices
        .ok_or("no _blendMatrixIndices")?;
    let indices = IntList::read(igz, list).map_err(|e| e.to_string())?.values;
    let actor = ctx.actor.ok_or("no igActor2 above")?;
    let system = Actor2::read(igz, actor)
        .map_err(|e| e.to_string())?
        .animation_system
        .ok_or("no _animationSystem")?;
    let skeleton = AnimationSystem2::read(igz, system)
        .map_err(|e| e.to_string())?
        .skeleton
        .ok_or("no _skeleton")?;
    let skeleton = Skeleton2::read(igz, skeleton).map_err(|e| e.to_string())?;
    let bones: Vec<SkeletonBone<'_>> = match skeleton.bone_list {
        Some(l) => SkeletonBoneList::read(igz, l)
            .and_then(|l| {
                l.bones
                    .iter()
                    .map(|&b| SkeletonBone::read(igz, b))
                    .collect()
            })
            .map_err(|e| e.to_string())?,
        None => Vec::new(),
    };
    let map = BoneMap::new(&indices, &bones);
    Ok((indices, map, skeleton.inverse_joints.len()))
}

fn union(boxes: impl IntoIterator<Item = Box3>) -> Option<Box3> {
    boxes.into_iter().reduce(|[lo, hi], [a, b]| {
        [
            std::array::from_fn(|k| lo[k].min(a[k])),
            std::array::from_fn(|k| hi[k].max(b[k])),
        ]
    })
}

fn gap(a: Box3, b: Box3) -> f32 {
    (0..2)
        .flat_map(|c| (0..3).map(move |k| (a[c][k] - b[c][k]).abs()))
        .fold(0.0, f32::max)
}

fn length(v: &[f32]) -> f64 {
    v.iter()
        .map(|&x| f64::from(x) * f64::from(x))
        .sum::<f64>()
        .sqrt()
}

#[derive(Default)]
struct Census {
    igz: u64,
    igz_with_edge: u64,
    segments: u64,
    vertexes: u64,
    triangles: u64,
    explicit: u64,
    skinned_vertexes: u64,
    degenerate: u64,
    unreached: u64,
    winding: [u64; 2],
    /// By stream: CMP word 0 decodes to (0, 0, 0) on RSX-only streams and
    /// to (1/2047, 1/2047, 1/1023) on SPU ones.
    zero_normals: BTreeMap<String, u64>,
    /// By stream and semantic.
    worst_unit: BTreeMap<String, f64>,
    uv_extent: f32,
    groups: u64,
    worst_gap: f32,
    counts: BTreeMap<String, u64>,
    failures: Vec<String>,
}

impl Census {
    fn bump(&mut self, key: impl Into<String>) {
        *self.counts.entry(key.into()).or_default() += 1;
    }

    fn take(&mut self, name: &str, bytes: &[u8]) {
        let igz = match Igz::parse(bytes) {
            Ok(igz) => igz,
            Err(e) => return self.failures.push(format!("{name}: {e}")),
        };
        self.igz += 1;
        let mut decoded = BTreeMap::new();
        for (i, layout) in layouts::read_all::<EdgeGeometrySegment>(&igz) {
            let s = layout.and_then(|l| Segment::decode(&igz, i, &l));
            match s {
                Ok(s) => {
                    self.segment(&s);
                    decoded.insert(i, s);
                }
                Err(e) => self.failures.push(format!("{name}: segment #{i}: {e}")),
            }
        }
        if decoded.is_empty() {
            return;
        }
        self.igz_with_edge += 1;
        if let Err(e) = self.scene(name, &igz, &decoded) {
            self.failures.push(format!("{name}: scene: {e}"));
        }
    }

    fn segment(&mut self, s: &Segment) {
        self.segments += 1;
        self.vertexes += s.vertex_count() as u64;
        self.triangles += s.triangles().count() as u64;
        self.explicit += u64::from(s.indexes.explicit_count);
        self.bump(format!("config {:?}", s.config.kind));
        for a in &s.attributes {
            self.bump(format!(
                "{:?}/{:?}:{:?}x{}@slot{}",
                a.stream, a.semantic, a.format, a.components, a.slot
            ));
        }
        if let Some(skin) = &s.skin {
            self.skinned_vertexes += skin.influences.len() as u64;
        }
        let pos: Vec<[f32; 3]> = s.positions().collect();
        if pos.iter().flatten().any(|x| !x.is_finite()) {
            self.failures.push("non-finite position".into());
        }
        for sem in [Semantic::Normal, Semantic::Tangent] {
            let Some(a) = s.attribute(sem) else { continue };
            let zero = match a.stream {
                Stream::RsxOnly => x11y11z10n_rsx(0),
                Stream::Spu0 | Stream::Spu1 => x11y11z10n_spu(0),
            };
            let key = format!("{:?}/{sem:?}", a.stream);
            for v in a.vertices() {
                if v == zero.as_slice() {
                    if sem == Semantic::Normal {
                        *self.zero_normals.entry(key.clone()).or_default() += 1;
                    }
                    continue;
                }
                let dev = (length(v) - 1.0).abs();
                let worst = self.worst_unit.entry(key.clone()).or_default();
                *worst = worst.max(dev);
            }
        }
        for sem in [Semantic::Uv0, Semantic::Uv1, Semantic::Uv2] {
            if let Some(a) = s.attribute(sem) {
                for &x in &a.values {
                    if !x.is_finite() {
                        self.failures.push("non-finite UV".into());
                    }
                    self.uv_extent = self.uv_extent.max(x.abs());
                }
            }
        }
        let normals: Option<Vec<&[f32]>> = s
            .attribute(Semantic::Normal)
            .map(|a| a.vertices().collect());
        for [a, b, c] in s.triangles() {
            if a == b || b == c || a == c {
                self.degenerate += 1;
            }
            let Some(n) = &normals else { continue };
            let p = |i: u16| pos[usize::from(i)].map(f64::from);
            let (pa, pb, pc) = (p(a), p(b), p(c));
            let u: [f64; 3] = std::array::from_fn(|k| pb[k] - pa[k]);
            let v: [f64; 3] = std::array::from_fn(|k| pc[k] - pa[k]);
            let g = [
                u[1] * v[2] - u[2] * v[1],
                u[2] * v[0] - u[0] * v[2],
                u[0] * v[1] - u[1] * v[0],
            ];
            let m: f64 = (0..3)
                .map(|k| {
                    let sum: f64 = [a, b, c]
                        .iter()
                        .map(|&i| f64::from(n[usize::from(i)][k]))
                        .sum();
                    g[k] * sum
                })
                .sum();
            if m > 0.0 {
                self.winding[0] += 1;
            } else if m < 0.0 {
                self.winding[1] += 1;
            }
        }
    }

    /// Bounds per group and the skin's bone map, through the scene graph.
    fn scene(
        &mut self,
        name: &str,
        igz: &Igz<'_>,
        decoded: &BTreeMap<usize, Segment>,
    ) -> Result<(), FormatError> {
        let ctx = contexts(igz)?;
        let mut reached = HashSet::new();
        let mut groups: BTreeMap<usize, BTreeSet<usize>> = BTreeMap::new();
        for (&g, ctxs) in &ctx {
            let segments = EdgeGeometry::read(igz, g)?.segments;
            reached.extend(segments.iter().copied());
            for c in ctxs {
                if let Some(b) = c.bound {
                    groups
                        .entry(b)
                        .or_default()
                        .extend(segments.iter().copied());
                }
                for s in &segments {
                    let Some(skin) = decoded.get(s).and_then(|s| s.skin.as_ref()) else {
                        continue;
                    };
                    let mapped = bone_map(igz, c).and_then(|(indices, map, joints)| {
                        skin.bones(&map).map_err(|e| e.to_string())?;
                        // Every weighted influence's blend matrix has an inverse joint.
                        let fits = skin
                            .influences
                            .iter()
                            .flatten()
                            .filter(|i| i.weight > 0)
                            .all(|i| {
                                let slot = skin.palette_slots[usize::from(i.local)];
                                usize::try_from(indices[usize::from(slot)])
                                    .is_ok_and(|b| b < joints)
                            });
                        fits.then_some(())
                            .ok_or_else(|| "a blend matrix past the inverse joints".to_owned())
                    });
                    match mapped {
                        Ok(_) => self.bump("skinned segment contexts mapped to bones"),
                        Err(e) => self.failures.push(format!("{name}: segment #{s}: {e}")),
                    }
                }
            }
        }
        self.unreached += decoded.keys().filter(|s| !reached.contains(s)).count() as u64;
        for (node, segs) in groups {
            self.groups += 1;
            let bound = aabox(igz, Node::read(igz, node)?.bound).expect("a bound");
            let got = union(segs.iter().filter_map(|s| decoded.get(s)?.bounds()));
            match got {
                Some(got) => {
                    let g = gap(got, bound);
                    self.worst_gap = self.worst_gap.max(g);
                    if g != 0.0 {
                        self.failures.push(format!(
                            "{name}: node #{node}: union {got:?} vs igAABox {bound:?}"
                        ));
                    }
                }
                None => self
                    .failures
                    .push(format!("{name}: node #{node}: no vertices")),
            }
        }
        Ok(())
    }

    fn take_archive(&mut self, path: &Path) {
        let name = path.file_name().unwrap().to_string_lossy().into_owned();
        let bytes = std::fs::read(path).unwrap();
        let archive = Archive::parse(&bytes).unwrap();
        for entry in archive.entries() {
            let data = archive.read(entry).unwrap();
            if data.starts_with(b"IGZ\x01") {
                self.take(&format!("{name}/{}", entry.name()), &data);
            }
        }
    }

    fn print(&self, title: &str) {
        println!(
            "{title}: {} IGZ, {} with Edge; {} segments, {} vertices, {} triangles",
            self.igz, self.igz_with_edge, self.segments, self.vertexes, self.triangles
        );
        println!(
            "  explicit indexes {}, skinned vertices {}, degenerate triangles {}",
            self.explicit, self.skinned_vertexes, self.degenerate
        );
        println!(
            "  winding agrees with normals {}, disagrees {}",
            self.winding[0], self.winding[1]
        );
        println!(
            "  all-zero normal words {:?}, worst unit-length deviation {:?}, |uv| max {}",
            self.zero_normals, self.worst_unit, self.uv_extent
        );
        println!(
            "  bound groups {}, worst union gap {}, segments under no scene graph {}",
            self.groups, self.worst_gap, self.unreached
        );
        for (k, n) in &self.counts {
            println!("  {k}: {n}");
        }
        println!("  failures: {}", self.failures.len());
        for f in self.failures.iter().take(40) {
            println!("    {f}");
        }
    }
}

fn level_bld(path: &Path) -> Vec<u8> {
    let bytes = std::fs::read(path).unwrap();
    let archive = Archive::parse(&bytes).unwrap();
    let entry = archive
        .entries()
        .iter()
        .find(|e| e.name().ends_with("level.bld"))
        .unwrap();
    archive.read(entry).unwrap().into_owned()
}

#[test]
#[ignore = "reads the Giants dump at GUAC_GIANTS_DUMP"]
fn eruptor_decodes_to_the_recorded_mesh() {
    let bytes = level_bld(&usrdir().join("character/009_eruptor.bld"));
    let igz = Igz::parse(&bytes).unwrap();
    let (model, _) = layouts::read_all::<PhysicsModel>(&igz)
        .find(|(_, m)| {
            m.as_ref().is_ok_and(|m| {
                m.model
                    .info
                    .name
                    .is_some_and(|n| n.ends_with("/Eruptor.mdl"))
            })
        })
        .expect("Eruptor.mdl");
    let hero = Hero::read(&igz, model).unwrap();
    let segments: Vec<Segment> = hero
        .meshes
        .iter()
        .flat_map(|m| hero.mesh_segments(m))
        .map(|(i, s)| Segment::decode(&igz, i, s).unwrap())
        .collect();
    let vertexes: usize = segments.iter().map(Segment::vertex_count).sum();
    let triangles: usize = segments.iter().map(|s| s.triangles().count()).sum();

    // Each mesh's skin through its palette to the hero's bones.
    let mut mapped = 0;
    for m in &hero.meshes {
        let palette = &hero.palettes[&m.palette.unwrap()];
        let indices = &hero.int_lists[&palette.indices.unwrap()].values;
        let map = BoneMap::new(indices, &hero.bones);
        for (i, s) in hero.mesh_segments(m) {
            let s = Segment::decode(&igz, i, s).unwrap();
            let bones = s.skin.as_ref().unwrap().bones(&map).unwrap();
            mapped += bones.len();
        }
    }
    let got = union(segments.iter().filter_map(Segment::bounds)).unwrap();
    let bound = aabox(&igz, hero.actor.group.node.bound).expect("the actor's igAABox");
    println!(
        "Eruptor: {} segments ({:?}), {vertexes} vertices, {triangles} triangles, {mapped} mapped to {} bones",
        segments.len(),
        segments.iter().map(|s| s.config.kind).collect::<Vec<_>>(),
        hero.bones.len()
    );
    println!("  union {got:?}\n  igAABox {bound:?}");
    assert_eq!(segments.len(), 3);
    assert!(segments.iter().all(|s| s.config.kind == Kind::Skinned));
    assert_eq!((vertexes, triangles), (2_861, 4_812));
    assert_eq!(mapped, vertexes);
    assert_eq!(got, bound);
}

#[test]
#[ignore = "reads the Giants dump at GUAC_GIANTS_DUMP"]
fn every_character_segment_decodes() {
    let mut paths: Vec<_> = std::fs::read_dir(usrdir().join("character"))
        .unwrap()
        .map(|e| e.unwrap().path())
        .filter(|p| p.extension().is_some_and(|e| e == "bld" || e == "arc"))
        .collect();
    paths.sort();
    let mut c = Census::default();
    for path in &paths {
        c.take_archive(path);
    }
    c.print(&format!("{} character archives", paths.len()));
    assert!(c.failures.is_empty());
    assert_eq!(c.segments, 2_321);
    assert_eq!(c.counts["config Skinned"], 424);
    assert_eq!((c.vertexes, c.triangles), (624_950, 698_924));
    assert_eq!(c.explicit, 368_718);
    assert_eq!(c.skinned_vertexes, 262_899);
    assert_eq!(c.degenerate, 0);
    assert_eq!(c.unreached, 0);
    assert_eq!(c.groups, 2_005);
    assert!(c.worst_unit.values().all(|&d| d < 0.01));
    assert!(c.uv_extent <= 64.0);
    assert!(c.winding[0] > 99 * c.winding[1]);
}
