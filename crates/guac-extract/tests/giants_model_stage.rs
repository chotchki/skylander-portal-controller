//! The model stage on the real Giants dump (openspec AE.6, AG.14): every
//! character model the roster loads, its textures, its hero side and the
//! 3D-UI rig, written into a throwaway identity dir, again as a resume and
//! once in the install format; Eruptor's model read back from the IR
//! against the decoders' direct output (counts, positions, his materials,
//! his idle skinned at AC.17's three solved times, his walk and run skinned
//! at and between their knots); the rig; and the totals AE.6's and AG.14's
//! ticks record. Ignored by default: it reads `GUAC_GIANTS_DUMP` and writes
//! under `GUAC_EXTRACT_DIR`, from the environment or the workspace's
//! `.env.dev`:
//!
//! `cargo test -p guac-extract --release --test giants_model_stage -- --ignored --nocapture`

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::time::Instant;

use guac_extract::intros::IrFormat;
use guac_extract::known::KNOWN_DUMPS;
use guac_extract::models;
use guac_extract::stages::{self, Files};
use guac_extract::{DirState, ExtractRoot, open};
use guac_formats::edge::{BoneMap, Segment as EdgeSegment};
use guac_formats::igz::Igz;
use guac_formats::igz::layouts::{Mat44, PixelShaderVariantMaskAttr, read_all};
use guac_formats::model::{JointMap, RUN_SLOT, WALK_SLOT, idle, portal_model, skeleton, slot_clip};
use guac_ir::clip::{Clip, RotationTrack};
use guac_ir::model::{Blend, TextureRole};
use guac_ir::{Hero, Model, Rig};

fn env_or_dotenv(key: &str) -> Option<String> {
    if let Ok(value) = std::env::var(key) {
        return Some(value);
    }
    let dotenv = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../.env.dev");
    let text = std::fs::read_to_string(dotenv).ok()?;
    text.lines().find_map(|line| {
        let (k, v) = line.trim().split_once('=')?;
        (k.trim() == key).then(|| v.trim().trim_matches('"').to_owned())
    })
}

/// AC.17's clip times, solved from AC.13's three captures of Eruptor.
const AC17_TIMES: [f64; 3] = [0.23481, 1.09289, 0.62158];
/// Decision 5's clip check: skinned positions within 1e-3 of the diagonal.
const CLIP: f64 = 1e-3;

type M4 = [[f64; 4]; 4];

fn mul(a: &M4, b: &M4) -> M4 {
    std::array::from_fn(|r| std::array::from_fn(|c| (0..4).map(|k| a[r][k] * b[k][c]).sum()))
}

/// Columns stored row by row (the stored inverse joint, the IR's
/// `inverse_bind`) as a column-vector matrix's rows.
fn from_cols(m: &[[f32; 4]; 4]) -> M4 {
    std::array::from_fn(|r| std::array::from_fn(|c| f64::from(m[c][r])))
}

/// A unit-ish quaternion, translation and scale as a column-vector T R S.
fn trs(t: [f64; 3], q: [f64; 4], s: [f64; 3]) -> M4 {
    let n = q.iter().map(|c| c * c).sum::<f64>().sqrt();
    let [x, y, z, w] = q.map(|c| c / n);
    let r = [
        [
            1.0 - 2.0 * (y * y + z * z),
            2.0 * (x * y - z * w),
            2.0 * (x * z + y * w),
        ],
        [
            2.0 * (x * y + z * w),
            1.0 - 2.0 * (x * x + z * z),
            2.0 * (y * z - x * w),
        ],
        [
            2.0 * (x * z - y * w),
            2.0 * (y * z + x * w),
            1.0 - 2.0 * (x * x + y * y),
        ],
    ];
    let row = |i: usize| [r[i][0] * s[0], r[i][1] * s[1], r[i][2] * s[2], t[i]];
    [row(0), row(1), row(2), [0.0, 0.0, 0.0, 1.0]]
}

/// Every knot frame of `c`'s splines and the midpoint between each two,
/// as times: the evaluator's hardest and easiest points. The last frame is
/// taken just short of the duration, where time would wrap to 0.
fn knot_times(c: &Clip) -> Vec<f64> {
    let mut knots: Vec<u8> = Vec::new();
    for t in &c.tracks {
        for s in [&t.translation.spline, &t.scale.spline]
            .into_iter()
            .flatten()
        {
            knots.extend(&s.knots);
        }
        if let RotationTrack::Spline { knots: k, .. } = &t.rotation {
            knots.extend(k);
        }
    }
    knots.sort_unstable();
    knots.dedup();
    let fd = f64::from(c.frame_duration);
    let end = f64::from(c.duration) - 1e-6;
    let mut frames: Vec<f64> = knots.iter().map(|&k| f64::from(k)).collect();
    let mids: Vec<f64> = frames.windows(2).map(|w| (w[0] + w[1]) / 2.0).collect();
    frames.extend(mids);
    frames.sort_by(f64::total_cmp);
    frames.into_iter().map(|f| (f * fd).min(end)).collect()
}

/// The angle between two rotations, well conditioned near zero (acos of a
/// dot product near 1 isn't): 2 atan2(|a - b|, |a + b|), signs aligned.
fn angle(a: [f64; 4], b: [f64; 4]) -> f64 {
    let dot: f64 = a.iter().zip(&b).map(|(x, y)| x * y).sum();
    let b = if dot < 0.0 { b.map(|c| -c) } else { b };
    let norm = |f: &dyn Fn(usize) -> f64| (0..4).map(|i| f(i).powi(2)).sum::<f64>().sqrt();
    2.0 * norm(&|i| a[i] - b[i]).atan2(norm(&|i| a[i] + b[i]))
}

fn apply(m: &M4, p: [f32; 3]) -> [f64; 3] {
    let p = p.map(f64::from);
    std::array::from_fn(|r| m[r][0] * p[0] + m[r][1] * p[1] + m[r][2] * p[2] + m[r][3])
}

/// Each vertex skinned: the sum over its influences of weight / 255 times
/// the palette matrix applied to its bind position.
fn skin(
    palette: &[M4],
    positions: &[[f32; 3]],
    joints: &[[u16; 4]],
    weights: &[[u8; 4]],
) -> Vec<[f64; 3]> {
    positions
        .iter()
        .zip(joints.iter().zip(weights))
        .map(|(&p, (j, w))| {
            let mut out = [0.0; 3];
            for k in 0..4 {
                if w[k] == 0 {
                    continue;
                }
                let q = apply(&palette[usize::from(j[k])], p);
                for i in 0..3 {
                    out[i] += f64::from(w[k]) / 255.0 * q[i];
                }
            }
            out
        })
        .collect()
}

/// Files under `prefix` in the manifest, and their bytes.
fn recorded(prefix: &str, m: &guac_extract::Manifest) -> (usize, u64) {
    m.outputs
        .iter()
        .filter(|o| o.path.starts_with(prefix))
        .fold((0, 0), |(n, b), o| (n + 1, b + o.size))
}

#[test]
#[ignore = "reads the Giants dump at GUAC_GIANTS_DUMP, writes under GUAC_EXTRACT_DIR"]
fn the_model_stage_extracts_every_character() {
    let dump = PathBuf::from(env_or_dotenv("GUAC_GIANTS_DUMP").expect("set GUAC_GIANTS_DUMP"));
    let base = PathBuf::from(env_or_dotenv("GUAC_EXTRACT_DIR").expect("set GUAC_EXTRACT_DIR"));
    std::fs::create_dir_all(&base).unwrap();
    let tmp = tempfile::Builder::new()
        .prefix("ae6-models-")
        .tempdir_in(&base)
        .unwrap();
    let root = ExtractRoot::checked(tmp.path().join("extract")).unwrap();
    let started = Instant::now();
    let mut opened = open(&dump, &root, KNOWN_DUMPS, &|_| {}).expect("the known dump");
    assert_eq!(opened.state, DirState::Fresh);
    println!("identity in {:.2} s", started.elapsed().as_secs_f64());
    let roster = stages::roster_stage(&mut opened.dir, IrFormat::Ron, &|_| {}).unwrap();
    println!("roster stage in {:.2} s", roster.took.as_secs_f64());

    // The stage, fresh.
    let stage =
        stages::model_stage(&mut opened.dir, &roster.roster, IrFormat::Ron, &|_| {}).unwrap();
    println!(
        "model stage: {} models, {} failed, {} textures for {} binds (dedupe {:.3}: {:.1}% fewer files), {:?} in {:.2} s",
        stage.models,
        stage.failed.len(),
        stage.textures,
        stage.binds,
        stage.binds as f64 / stage.textures as f64,
        100.0 * (1.0 - stage.textures as f64 / stage.binds as f64),
        stage.files,
        stage.took.as_secs_f64()
    );
    for f in &stage.failed {
        println!("  FAILED {}: {}", f.load_file, f.reason);
    }
    assert!(stage.failed.is_empty(), "every character model extracts");
    assert_eq!(stage.models, 67);
    println!(
        "  heroes: {} settled, {} unsettled, {:.2} s of the stage",
        stage.heroes,
        stage.unsettled.len(),
        stage.heroes_took.as_secs_f64()
    );
    for f in &stage.unsettled {
        println!("  UNSETTLED {}: {}", f.load_file, f.reason);
    }
    assert!(stage.unsettled.is_empty(), "every character's hero settles");
    assert_eq!(stage.heroes, 67);
    let m = opened.dir.manifest().clone();
    let dir = opened.dir.path().to_path_buf();
    let (n_models, model_bytes) = recorded("models/", &m);
    let (n_tex, tex_bytes) = recorded("textures/", &m);
    let (_, rig_bytes) = recorded("ir/rig", &m);
    let (n_heroes, hero_bytes) = recorded("heroes/", &m);
    println!(
        "  RON: models/ {n_models} files {model_bytes} bytes, textures/ {n_tex} files {tex_bytes} bytes, heroes/ {n_heroes} files {hero_bytes} bytes, ir/rig.ron {rig_bytes} bytes"
    );
    assert_eq!((n_models, n_tex, n_heroes), (67, stage.textures, 67));

    // A resume keeps every file.
    let again =
        stages::model_stage(&mut opened.dir, &roster.roster, IrFormat::Ron, &|_| {}).unwrap();
    println!(
        "  resumed: {:?} in {:.2} s",
        again.files,
        again.took.as_secs_f64()
    );
    assert_eq!(
        again.files,
        Files {
            written: 0,
            kept: 67 + 67 + stage.textures + 1,
            bytes: stage.files.bytes
        }
    );
    // The install format writes only its IR, beside.
    let post =
        stages::model_stage(&mut opened.dir, &roster.roster, IrFormat::Postcard, &|_| {}).unwrap();
    let m = opened.dir.manifest().clone();
    let postcard: u64 = m
        .outputs
        .iter()
        .filter(|o| o.path.ends_with(".postcard") && !o.path.contains("roster"))
        .map(|o| o.size)
        .sum();
    let post_heroes: u64 = m
        .outputs
        .iter()
        .filter(|o| o.path.starts_with("heroes/") && o.path.ends_with(".postcard"))
        .map(|o| o.size)
        .sum();
    println!(
        "  postcard: {:?} in {:.2} s; models, heroes and rig {postcard} bytes, the heroes {post_heroes}",
        post.files,
        post.took.as_secs_f64()
    );
    assert_eq!(
        (post.files.written, post.files.kept),
        (67 + 67 + 1, stage.textures)
    );
    // Every hero's RON, read back and put in postcard, is its postcard file
    // byte for byte: the two formats hold the same bits.
    let mut agreed = 0;
    for load in models::character_loads(&roster.roster) {
        let ron: Hero = guac_ir::from_ron(
            &std::fs::read_to_string(dir.join(IrFormat::Ron.hero_path(load))).unwrap(),
        )
        .unwrap();
        let post = std::fs::read(dir.join(IrFormat::Postcard.hero_path(load))).unwrap();
        assert!(guac_ir::to_postcard(&ron).unwrap() == post, "{load}");
        agreed += 1;
    }
    assert_eq!(agreed, 67);
    println!("  heroes: RON and postcard agree byte for byte on all {agreed}");
    // Both formats hold the same model.
    let ron: Model =
        guac_ir::from_ron(&std::fs::read_to_string(dir.join("models/009_Eruptor.ron")).unwrap())
            .unwrap();
    let post: Model =
        guac_ir::from_postcard(&std::fs::read(dir.join("models/009_Eruptor.postcard")).unwrap())
            .unwrap();
    assert_eq!(ron, post);

    // Eruptor, read back from the IR against the decoders' own output.
    let ir: Model =
        guac_ir::from_ron(&std::fs::read_to_string(dir.join("models/009_Eruptor.ron")).unwrap())
            .unwrap();
    let archive = std::fs::read(models::archive_path(
        &dump.join("PS3_GAME/USRDIR"),
        "009_Eruptor",
    ))
    .unwrap();
    let level = models::level(&archive, "009_Eruptor").unwrap();
    let igz = Igz::parse(&level).unwrap();
    let portal = portal_model(&igz).unwrap();
    let hero = &portal.hero;
    let hk = skeleton(&igz, hero).unwrap();
    let joints = JointMap::new(&hk, &hero.bones).unwrap();
    assert_eq!(ir.name, "Eruptor.mdl");
    let sources: Vec<u32> = ir.segments.iter().map(|s| s.source).collect();
    assert_eq!(sources, [122, 123, 135], "AC.13's draws");
    assert_eq!((ir.vertices(), ir.triangles()), (2_861, 4_812));
    assert_eq!(ir.skeleton.joints.len(), 33);
    assert_eq!(
        hero.bones.len(),
        34,
        "33 joints plus igSkeleton2's unnamed root"
    );
    let unmapped: Vec<Option<&str>> = hero
        .bones
        .iter()
        .zip(&joints.joint_of_bone)
        .filter(|(_, j)| j.is_none())
        .map(|(b, _)| b.name)
        .collect();
    println!("  Eruptor's Alchemy bones without a joint: {unmapped:?}");
    assert_eq!(unmapped.len(), 1);
    // Every segment's counts and a few positions as the decoders give them.
    let mut decoded = Vec::new();
    for mesh in &hero.meshes {
        let palette = mesh
            .palette
            .and_then(|p| hero.palettes.get(&p)?.indices)
            .and_then(|i| hero.int_lists.get(&i))
            .unwrap();
        let map = BoneMap::new(&palette.values, &hero.bones);
        for (i, layout) in hero.mesh_segments(mesh) {
            decoded.push((
                i,
                EdgeSegment::decode(&igz, i, layout).unwrap(),
                map.clone(),
            ));
        }
    }
    for ((i, d, _), s) in decoded.iter().zip(&ir.segments) {
        assert_eq!(*i as u32, s.source);
        let positions: Vec<[f32; 3]> = d.positions().collect();
        assert_eq!(positions.len(), s.positions.len());
        assert_eq!(d.triangles().count() * 3, s.indices.len());
        for k in [0, positions.len() / 2, positions.len() - 1] {
            assert_eq!(positions[k], s.positions[k], "segment {i} vertex {k}");
        }
        println!(
            "  segment {i}: {} vertices, {} triangles, first {:?}, last {:?}",
            s.positions.len(),
            s.indices.len() / 3,
            s.positions[0],
            s.positions[s.positions.len() - 1]
        );
    }
    // His two materials at 0x1C, by ae-shader section 2's roles and
    // parameters; the spikes' 0x24 material isn't among them.
    let mats: Vec<(&str, u32, u32, u32, u32)> = ir
        .materials
        .iter()
        .map(|m| {
            (
                m.name.as_str(),
                m.pixel.base,
                m.pixel.ignore,
                m.vertex.base,
                m.vertex.ignore,
            )
        })
        .collect();
    assert_eq!(
        mats,
        [
            ("EruptorBody", 0x1C, 0x2FF, 0x1C, 0x1AFF),
            ("EruptorEyes_Teeth", 0x1C, 0x2FF, 0x1C, 0x1AFF)
        ]
    );
    let roles = |m: &guac_ir::model::Material| -> Vec<(TextureRole, String)> {
        m.textures
            .iter()
            .map(|t| (t.role, t.image.clone()))
            .collect()
    };
    let named = |rim: &str| -> Vec<(TextureRole, String)> {
        vec![
            (TextureRole::Layer1BaseNormalMap, "eruptor_n".into()),
            (TextureRole::Layer1BaseSpecMap, "eruptor_sp".into()),
            (TextureRole::Layer1ColorMap, "eruptor_c".into()),
            (TextureRole::Layer4ColorMap, rim.into()),
        ]
    };
    let (body, eyes) = (&ir.materials[0], &ir.materials[1]);
    let images = |m: &guac_ir::model::Material| -> Vec<String> {
        roles(m)
            .into_iter()
            .map(|(_, n)| n.trim_end_matches(".png").to_ascii_lowercase())
            .collect()
    };
    println!("  body images {:?}, eyes {:?}", roles(body), roles(eyes));
    assert_eq!(
        images(body),
        named("eruptorrim")
            .into_iter()
            .map(|(_, n)| n)
            .collect::<Vec<_>>()
    );
    assert_eq!(
        images(eyes),
        named("specrim_eye6")
            .into_iter()
            .map(|(_, n)| n)
            .collect::<Vec<_>>()
    );
    for m in [body, eyes] {
        let r: Vec<TextureRole> = m.textures.iter().map(|t| t.role).collect();
        assert_eq!(r, named("").into_iter().map(|(r, _)| r).collect::<Vec<_>>());
        assert_eq!(m.params.layer1_extra, [1.0, 1.0, 1.0, 1.0]);
        assert_eq!(m.params.fixed_alpha, [-1.0, -1.0, -1.0, 1.0]);
        assert_eq!(m.params.layer4_blend, [0.0, 1.0, 0.0, 0.0], "the rim added");
        assert_eq!(m.params.layer4_values, [0.0, 1.0, 0.0, 0.0]);
        assert_eq!(m.params.material_tint, [128, 128, 128, 255]);
        assert_eq!(
            (m.states.blend, m.states.depth_write, m.states.two_sided),
            (Blend::Opaque, true, false)
        );
        assert!(m.scrolls.is_empty());
    }
    assert_eq!((body.params.glow, eyes.params.glow), (Some(64), None));
    assert!(
        ir.materials
            .iter()
            .all(|m| m.pixel.base != 0x24 && m.name != "EruptorMatte")
    );
    let spikes = read_all::<PixelShaderVariantMaskAttr>(&igz)
        .filter(|(_, m)| m.as_ref().is_ok_and(|m| m.0.base_mask == 0x24))
        .count();
    println!("  pixel masks at 0x24 in his archive, outside the portal model: {spikes}");
    assert!(spikes > 0);

    // His idle from the IR, skinned at AC.17's times, against the decoders'.
    let decoded_idle = idle(&igz, hero, &hk).unwrap();
    assert_eq!(ir.idle.name, "Eruptor_Idle.ma");
    assert_eq!(decoded_idle.file, Some("Eruptor_Idle.ma"));
    let alchemy_inverse = joints
        .inverse_joints(&hero.bones, &hero.skeleton.inverse_joints)
        .unwrap();
    let all: Vec<[f32; 3]> = ir
        .segments
        .iter()
        .flat_map(|s| s.positions.iter().copied())
        .collect();
    let (lo, hi) = all
        .iter()
        .fold(([f32::MAX; 3], [f32::MIN; 3]), |(lo, hi), p| {
            (
                std::array::from_fn(|i| lo[i].min(p[i])),
                std::array::from_fn(|i| hi[i].max(p[i])),
            )
        });
    let diagonal = (0..3)
        .map(|i| f64::from(hi[i] - lo[i]).powi(2))
        .sum::<f64>()
        .sqrt();
    for t in AC17_TIMES {
        // The decoders: Havok's model pose times Alchemy's inverse joint.
        let local = decoded_idle.clip.local_pose(&hk, t).unwrap();
        let pose = hk.model_pose(&local).unwrap();
        let want_palette: Vec<M4> = pose
            .iter()
            .zip(&alchemy_inverse)
            .map(|(q, inv): (_, &Mat44)| mul(&q.matrix(), &from_cols(inv)))
            .collect();
        // The IR: its evaluator's pose times its inverse binds.
        let ir_pose = ir.idle.model_pose(&ir.skeleton, t);
        let got_palette: Vec<M4> = ir_pose
            .iter()
            .zip(&ir.skeleton.joints)
            .map(|(p, j)| {
                let m = trs(
                    p.translation.map(f64::from),
                    p.rotation.map(f64::from),
                    p.scale.map(f64::from),
                );
                mul(&m, &from_cols(&j.inverse_bind))
            })
            .collect();
        let mut worst = 0f64;
        for ((_, d, map), s) in decoded.iter().zip(&ir.segments) {
            // The decoders' own skin, to Havok joints by name.
            let skin_bones = d.skin.as_ref().unwrap().bones(map).unwrap();
            let (dj, dw): (Vec<[u16; 4]>, Vec<[u8; 4]>) = skin_bones
                .iter()
                .map(|inf| {
                    let mut j = [0u16; 4];
                    let mut w = [0u8; 4];
                    for (k, b) in inf.iter().enumerate() {
                        if b.weight > 0 {
                            j[k] = joints.joint_of_bone[b.bone].unwrap() as u16;
                            w[k] = b.weight;
                        }
                    }
                    (j, w)
                })
                .unzip();
            let positions: Vec<[f32; 3]> = d.positions().collect();
            let want = skin(&want_palette, &positions, &dj, &dw);
            let got = skin(&got_palette, &s.positions, &s.joints, &s.weights);
            for (w, g) in want.iter().zip(&got) {
                let e = (0..3).map(|i| (w[i] - g[i]).powi(2)).sum::<f64>().sqrt();
                worst = worst.max(e);
            }
        }
        println!(
            "  idle at {t} s: worst skinned vertex {worst:.3e} ({:.3e} of the {diagonal:.4} diagonal, tolerance {CLIP:.0e})",
            worst / diagonal
        );
        assert!(worst <= CLIP * diagonal);
    }

    // His walk and run from the IR's hero side, skinned at and between
    // every knot, against the decoders' clips out of the same slots.
    let hero_ir: Hero =
        guac_ir::from_ron(&std::fs::read_to_string(dir.join("heroes/009_Eruptor.ron")).unwrap())
            .unwrap();
    for (name, slot, sc) in [
        ("walk", WALK_SLOT, &hero_ir.locomotion.walk),
        ("run", RUN_SLOT, &hero_ir.locomotion.run),
    ] {
        let sc = sc.as_ref().expect("Eruptor walks and runs");
        assert_eq!(usize::from(sc.slot), slot);
        let ir_clip = &sc.clip;
        let decoded_clip = slot_clip(&igz, hero, &hk, slot).unwrap().unwrap();
        assert_eq!(decoded_clip.file, Some(ir_clip.name.as_str()));
        let times = knot_times(ir_clip);
        let (mut worst, mut worst_joint, mut worst_angle) = (0f64, 0f64, 0f64);
        for &t in &times {
            let local = decoded_clip.clip.local_pose(&hk, t).unwrap();
            let pose = hk.model_pose(&local).unwrap();
            let want_palette: Vec<M4> = pose
                .iter()
                .zip(&alchemy_inverse)
                .map(|(q, inv): (_, &Mat44)| mul(&q.matrix(), &from_cols(inv)))
                .collect();
            let ir_pose = ir_clip.model_pose(&ir.skeleton, t);
            for (w, g) in pose.iter().zip(&ir_pose) {
                let d = (0..3)
                    .map(|i| (w.translation[i] - f64::from(g.translation[i])).powi(2))
                    .sum::<f64>()
                    .sqrt();
                worst_joint = worst_joint.max(d);
                worst_angle = worst_angle.max(angle(w.rotation, g.rotation.map(f64::from)));
            }
            let got_palette: Vec<M4> = ir_pose
                .iter()
                .zip(&ir.skeleton.joints)
                .map(|(p, j)| {
                    let m = trs(
                        p.translation.map(f64::from),
                        p.rotation.map(f64::from),
                        p.scale.map(f64::from),
                    );
                    mul(&m, &from_cols(&j.inverse_bind))
                })
                .collect();
            for ((_, d, map), s) in decoded.iter().zip(&ir.segments) {
                let skin_bones = d.skin.as_ref().unwrap().bones(map).unwrap();
                let (dj, dw): (Vec<[u16; 4]>, Vec<[u8; 4]>) = skin_bones
                    .iter()
                    .map(|inf| {
                        let mut j = [0u16; 4];
                        let mut w = [0u8; 4];
                        for (k, b) in inf.iter().enumerate() {
                            if b.weight > 0 {
                                j[k] = joints.joint_of_bone[b.bone].unwrap() as u16;
                                w[k] = b.weight;
                            }
                        }
                        (j, w)
                    })
                    .unzip();
                let positions: Vec<[f32; 3]> = d.positions().collect();
                let want = skin(&want_palette, &positions, &dj, &dw);
                let got = skin(&got_palette, &s.positions, &s.joints, &s.weights);
                for (w, g) in want.iter().zip(&got) {
                    let e = (0..3).map(|i| (w[i] - g[i]).powi(2)).sum::<f64>().sqrt();
                    worst = worst.max(e);
                }
            }
        }
        println!(
            "  {name} ({}, slot {slot}, {:.4} s, {} frames, label {:?}): {} times at and between knots; worst skinned vertex {worst:.3e} ({:.3e} of the diagonal, tolerance {CLIP:.0e}), worst joint {worst_joint:.3e}, worst turn {worst_angle:.3e} rad",
            ir_clip.name,
            ir_clip.duration,
            ir_clip.frames,
            sc.label,
            times.len(),
            worst / diagonal
        );
        assert!(times.len() >= 3);
        assert!(worst <= CLIP * diagonal, "{name}");
    }

    // The rig: three directional lights, the key pitched -45.
    let rig: Rig =
        guac_ir::from_ron(&std::fs::read_to_string(dir.join("ir/rig.ron")).unwrap()).unwrap();
    for l in &rig.lights {
        println!(
            "  rig {}: diffuse {:?} ambient {:?} specular {:?} x{} at {:?} rotated {:?}",
            l.name, l.diffuse, l.ambient, l.specular, l.multiplier, l.position, l.rotation
        );
    }
    let names: Vec<&str> = rig.lights.iter().map(|l| l.name.as_str()).collect();
    assert_eq!(names, ["_spriteLight1", "_spriteLight2", "_spriteLight3"]);
    assert_eq!(rig.lights[0].rotation[0], -45.0);
    assert_eq!(rig.lights[0].diffuse, [80, 78, 66, 255]);

    // Totals over the 67, for the tick.
    let mut segments = 0;
    let (mut vertices, mut triangles, mut materials) = (0, 0, 0);
    let mut joints_total = 0;
    let mut roles: BTreeMap<String, usize> = BTreeMap::new();
    let mut pixel: BTreeMap<String, usize> = BTreeMap::new();
    let mut states: BTreeMap<String, usize> = BTreeMap::new();
    let mut scrolls = 0;
    let mut uv_sets: BTreeMap<usize, usize> = BTreeMap::new();
    let mut colours = 0;
    let mut tangents = 0;
    let mut low_bits_differ = Vec::new();
    let mut rim_on_default_alpha = Vec::new();
    let mut idles = BTreeSet::new();
    let mut untracked = BTreeMap::new();
    let mut largest = (0, String::new());
    let (mut ron_read, mut postcard_read) = (0.0, 0.0);
    for load in models::character_loads(&roster.roster) {
        let rel = IrFormat::Ron.model_path(load);
        let read = Instant::now();
        let m: Model =
            guac_ir::from_ron(&std::fs::read_to_string(dir.join(&rel)).unwrap()).unwrap();
        ron_read += read.elapsed().as_secs_f64();
        let read = Instant::now();
        let bytes = std::fs::read(dir.join(IrFormat::Postcard.model_path(load))).unwrap();
        let p: Model = guac_ir::from_postcard(&bytes).unwrap();
        postcard_read += read.elapsed().as_secs_f64();
        assert_eq!(p, m, "{load}");
        let size = std::fs::metadata(dir.join(&rel)).unwrap().len();
        if size > largest.0 {
            largest = (size, load.to_owned());
        }
        segments += m.segments.len();
        vertices += m.vertices();
        triangles += m.triangles();
        materials += m.materials.len();
        joints_total += m.skeleton.joints.len();
        for s in &m.segments {
            *uv_sets.entry(s.uvs.len()).or_default() += 1;
            colours += usize::from(s.colors.is_some());
            tangents += usize::from(s.tangents.is_some());
            assert!(
                s.tangents
                    .as_ref()
                    .is_none_or(|t| t.iter().all(|t| t[3] == 1.0))
            );
        }
        for mat in &m.materials {
            for t in &mat.textures {
                *roles.entry(format!("{:?}", t.role)).or_default() += 1;
                assert!(
                    opened.dir.output_done(&t.texture).unwrap(),
                    "{load}: {}",
                    t.texture
                );
            }
            *pixel.entry(format!("{:#06x}", mat.pixel.base)).or_default() += 1;
            *states.entry(format!("{:?}", mat.states)).or_default() += 1;
            scrolls += mat.scrolls.len();
            if mat.pixel.base & 0x7FF != mat.vertex.base & 0x7FF {
                low_bits_differ.push(format!(
                    "{load}: {} ps {:#x} vs {:#x}",
                    mat.name, mat.pixel.base, mat.vertex.base
                ));
            }
            if mat.pixel.base & 0x4 != 0 && mat.params.fixed_alpha[3] < 0.0 {
                rim_on_default_alpha.push(format!("{load}: {}", mat.name));
            }
        }
        idles.insert(m.idle.name.clone());
        let tracked: BTreeSet<u16> = m.idle.tracks.iter().map(|t| t.joint).collect();
        let n = m.skeleton.joints.len() - tracked.len();
        if n > 0 {
            untracked.insert(load.to_owned(), n);
        }
    }
    println!(
        "== 67 models: {segments} segments, {vertices} vertices, {triangles} triangles, {materials} materials, {joints_total} joints; largest {} ({} bytes RON)",
        largest.1, largest.0
    );
    println!("  read back: RON {ron_read:.2} s, postcard {postcard_read:.3} s for all 67");
    println!("  textures by role: {roles:?}");
    println!("  pixel base masks: {pixel:?}");
    println!("  render states: {states:?}");
    println!(
        "  scroll tracks {scrolls}; segments by UV-set count {uv_sets:?}; with tangents {tangents}, vertex colour {colours}"
    );
    println!("  pixel and vertex bits 0-10 differ: {low_bits_differ:?}");
    println!("  layer 4 with no fixed-alpha attr (rim strength -1): {rim_on_default_alpha:?}");
    println!(
        "  {} distinct idles; joints the idle leaves untracked: {untracked:?}",
        idles.len()
    );
    assert!(rim_on_default_alpha.is_empty());
    println!("took {:.2} s in all", started.elapsed().as_secs_f64());
}
