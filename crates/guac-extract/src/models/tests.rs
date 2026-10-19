use super::*;
use guac_formats::edge::builder::{cmp_spu_value, skinned};
use guac_formats::havok::builder::{
    ChanSpec, ClipSpec, PayloadSpec, QuatSpec, TrackSpec, VecSpec, clamped_knots,
    clip as havok_clip, encode40, payload, skeleton3,
};
use guac_formats::model::builder::{CharacterSpec, character, inverse_joint, sample};
use guac_formats::roster::builder::{RIG, disc};
use guac_ir::model::TextureRole as R;

fn identity() -> DumpIdentity {
    DumpIdentity {
        title_id: "BLUS30968".into(),
        app_ver: "01.00".into(),
        manifest_hash: Sha256([3; 32]),
    }
}

fn extracted(spec: &CharacterSpec) -> Result<Extracted> {
    read(&character(spec), "009_Eruptor", identity())
}

#[test]
fn the_sample_archive_becomes_a_model() {
    let e = extracted(&sample()).unwrap();
    let m = &e.model;
    assert_eq!(
        (m.load_file.as_str(), m.name.as_str()),
        ("009_Eruptor", "Eruptor.mdl")
    );
    assert_eq!(m.identity, identity());

    // Havok's three joints; Alchemy's unnamed root has none.
    let names: Vec<&str> = m.skeleton.joints.iter().map(|j| j.name.as_str()).collect();
    assert_eq!(names, ["J_root", "J_spine", "J_head"]);
    let parents: Vec<Option<u16>> = m.skeleton.joints.iter().map(|j| j.parent).collect();
    assert_eq!(parents, [None, Some(0), Some(1)]);
    for (i, j) in m.skeleton.joints.iter().enumerate() {
        assert_eq!(j.inverse_bind, inverse_joint(i), "joint {i}");
    }
    assert_eq!(m.skeleton.joints[0].reference.translation, [0.0, 1.0, 0.0]);

    // Two techniques, each drawing the skinned fixture.
    assert_eq!(m.materials.len(), 2);
    assert_eq!((m.vertices(), m.triangles()), (8, 4));
    let f = skinned();
    let s = &m.segments[1];
    assert_eq!(s.material, 1);
    assert_eq!(s.positions, f.expect.positions);
    let normals: Vec<[f32; 3]> = f
        .expect
        .normals
        .iter()
        .map(|&[x, y, z]| cmp_spu_value(x, y, z))
        .collect();
    assert_eq!(s.normals, normals);
    let tangents = s.tangents.as_ref().unwrap();
    assert!(
        tangents.iter().all(|t| t[3] == 1.0),
        "a 3-component tangent's w"
    );
    assert_eq!(s.uvs.len(), 1);
    assert_eq!(s.uvs[0][0], [f.expect.uv0[0][0].1, f.expect.uv0[0][1].1]);
    assert_eq!(s.colors, None);
    let flat: Vec<u16> = f.expect.tris.iter().flatten().copied().collect();
    assert_eq!(s.indices, flat);
    // Palette slots 2, 3 and 10 are joints 0, 1 and 2; a zero weight's
    // joint is 0.
    assert_eq!(
        s.joints,
        [[0, 2, 0, 0], [1, 0, 0, 0], [0, 1, 0, 0], [2, 1, 0, 0]]
    );
    assert_eq!(
        s.weights,
        [
            [200, 55, 0, 0],
            [255, 0, 0, 0],
            [128, 127, 0, 0],
            [100, 100, 55, 0]
        ]
    );

    // The body's textures by role and his parameters.
    let body = &m.materials[0];
    assert_eq!(body.name, "EruptorBody");
    assert_eq!((body.pixel.base, body.pixel.ignore), (0x1C, 0x2FF));
    let roles: Vec<(R, &str)> = body
        .textures
        .iter()
        .map(|t| (t.role, t.image.as_str()))
        .collect();
    assert_eq!(
        roles,
        [
            (R::Layer1BaseNormalMap, "eruptor_n"),
            (R::Layer1BaseSpecMap, "eruptor_sp"),
            (R::Layer1ColorMap, "eruptor_c"),
            (R::Layer4ColorMap, "eruptorrim"),
        ]
    );
    assert_eq!(body.params.layer1_extra, [1.0; 4]);
    assert_eq!(body.params.glow, Some(64));
    assert_eq!(body.params.material_tint, [128, 128, 128, 255]);
    assert_eq!(m.materials[1].params.glow, None);
    let n = &body.textures[0].sampler;
    assert_eq!(
        (n.wrap_u, n.min, n.mip),
        (Wrap::Repeat, Filter::Linear, Some(Filter::Linear))
    );

    // The idle, on the skeleton's joints.
    assert_eq!(m.idle.name, "Eruptor_Idle.ma");
    let joints: Vec<u16> = m.idle.tracks.iter().map(|t| t.joint).collect();
    assert_eq!(joints, [0, 1, 2]);

    // It survives both IR formats.
    let back: Model = guac_ir::from_postcard(&guac_ir::to_postcard(m).unwrap()).unwrap();
    assert_eq!(back, *m);
    let back: Model = guac_ir::from_ron(&guac_ir::to_ron(m).unwrap()).unwrap();
    assert_eq!(back, *m);
}

#[test]
fn two_identical_textures_dedupe() {
    let e = extracted(&sample()).unwrap();
    // Eight binds; the eyes' normal, spec and colour maps are byte-identical
    // copies of the body's images, so five files.
    assert_eq!(e.binds, 8);
    assert_eq!(e.textures.len(), 5);
    let [body, eyes] = [&e.model.materials[0], &e.model.materials[1]];
    for role in [
        R::Layer1BaseNormalMap,
        R::Layer1BaseSpecMap,
        R::Layer1ColorMap,
    ] {
        assert_eq!(
            body.texture(role).unwrap().texture,
            eyes.texture(role).unwrap().texture
        );
    }
    assert_ne!(
        body.texture(R::Layer4ColorMap).unwrap().texture,
        eyes.texture(R::Layer4ColorMap).unwrap().texture
    );
    for (path, ktx2) in &e.textures {
        assert_eq!(*path, texture_path(ktx2));
        assert!(ktx2.starts_with(b"\xABKTX 20\xBB\r\n\x1A\n"), "{path}");
        assert!(path.starts_with("textures/") && path.ends_with(".ktx2"));
    }
}

#[test]
fn a_refused_material_names_its_model_and_why() {
    let mut s = sample();
    s.materials[0].pixel = Some((0x5C, 0x2FF));
    let e = extracted(&s).unwrap_err();
    assert_eq!(e.code(), "GUAC-E-MODEL-UNREADABLE");
    let text = e.to_string();
    assert!(
        text.contains("009_Eruptor") && text.contains("layer4UsesPositions"),
        "{text}"
    );

    // A TFN1 image outside the normal-map slot.
    let mut s = sample();
    s.materials[1].binds[2] = s.materials[1].binds[2].clone().normal_map();
    let text = extracted(&s).unwrap_err().to_string();
    assert!(
        text.contains("TFN1") && text.contains("layer1ColorMap"),
        "{text}"
    );

    // No archive, and an archive without its level.
    let tmp = tempfile::tempdir().unwrap();
    let e = archive(&tmp.path().join("x.bld"), "009_Eruptor").unwrap_err();
    assert_eq!(e.code(), "GUAC-E-MODEL-UNREADABLE");
    let other = guac_formats::iga::builder::IgaBuilder::new()
        .stored("other.bld", b"x")
        .build();
    assert_eq!(
        level(&other, "009_Eruptor").unwrap_err().code(),
        "GUAC-E-MODEL-UNREADABLE"
    );
}

#[test]
fn samplers_map_the_disc_values_and_refuse_the_rest() {
    let attr = |mag, min, s, t| {
        let mut a = guac_formats::model::portal_model(&Igz::parse(&character(&sample())).unwrap())
            .unwrap()
            .hero
            .textures
            .into_values()
            .next()
            .unwrap();
        (a.mag_filter, a.min_filter, a.wrap_s, a.wrap_t) = (mag, min, s, t);
        a
    };
    let s = sampler(&attr(1, 5, 1, 0)).unwrap();
    assert_eq!(
        s,
        Sampler {
            wrap_u: Wrap::Repeat,
            wrap_v: Wrap::Clamp,
            mag: Filter::Linear,
            min: Filter::Linear,
            mip: Some(Filter::Linear),
        }
    );
    let flat = sampler(&attr(0, 0, 0, 0)).unwrap();
    assert_eq!((flat.min, flat.mip), (Filter::Nearest, None));
    assert!(sampler(&attr(2, 5, 1, 1)).is_err());
    assert!(sampler(&attr(1, 3, 1, 1)).is_err());
    assert!(sampler(&attr(1, 5, 4, 1)).is_err());
}

/// A clip off the Havok builder, decoded, with the skeleton it poses: the
/// decoders' answer and the IR's, side by side.
fn decoded(spec: ClipSpec) -> (havok::Skeleton<'static>, havok::Clip<'static>) {
    let p = PayloadSpec {
        skeletons: vec![skeleton3()],
        clip: Some(spec),
        ..PayloadSpec::default()
    };
    let bytes: &'static [u8] = Vec::leak(payload(&p).bytes);
    let c = havok::Container::parse(bytes).unwrap();
    (c.skeletons.into_iter().next().unwrap(), c.clip.unwrap())
}

fn ir_skeleton(s: &havok::Skeleton<'_>) -> Skeleton {
    skeleton(s, &vec![[[0.0; 4]; 4]; s.bones.len()]).unwrap()
}

/// The angle between two rotations, well conditioned near zero (acos of a
/// dot product near 1 isn't): 2 atan2(|a - b|, |a + b|), signs aligned.
fn angle(a: [f64; 4], b: [f64; 4]) -> f64 {
    let dot: f64 = a.iter().zip(&b).map(|(x, y)| x * y).sum();
    let b = if dot < 0.0 { b.map(|c| -c) } else { b };
    let norm = |f: &dyn Fn(usize) -> f64| (0..4).map(|i| f(i).powi(2)).sum::<f64>().sqrt();
    2.0 * norm(&|i| a[i] - b[i]).atan2(norm(&|i| a[i] + b[i]))
}

/// The largest gap between the decoders' model pose and the IR's at `t`
/// (the decoders' unlooped): translation, then rotation angle.
fn gap(s: &havok::Skeleton<'_>, c: &havok::Clip<'_>, ir: &Clip, t: f64, at: f64) -> (f64, f64) {
    let want = s.model_pose(&c.local_pose(s, t).unwrap()).unwrap();
    let got = ir.model_pose(&ir_skeleton(s), at);
    want.iter().zip(&got).fold((0.0, 0.0), |(dt, dr), (w, g)| {
        let d = (0..3)
            .map(|i| (w.translation[i] - f64::from(g.translation[i])).powi(2))
            .sum::<f64>()
            .sqrt();
        let r = angle(w.rotation, g.rotation.map(f64::from));
        (dt.max(d), dr.max(r))
    })
}

#[test]
fn the_evaluator_matches_the_decoders_at_and_between_knots() {
    let (s, c) = decoded(havok_clip());
    let ir = clip(&c, "test");
    assert_eq!((ir.frames, ir.tracks.len()), (9, 3));
    let fd = f64::from(c.animation.frame_duration);
    let d = f64::from(ir.duration);
    // The cubic's knots are frames 0, 4 and 8, the rotation's 0 and 8.
    for frame in [0.0, 4.0, 8.0 - 1e-9, 0.5, 1.0, 2.75, 3.3, 5.9, 7.99] {
        let t = frame * fd;
        let (dt, dr) = gap(&s, &c, &ir, t, t);
        assert!(dt < 1e-5 && dr < 1e-5, "frame {frame}: {dt:e}, {dr:e}");
        // A turntable runs the idle for minutes: whole loops either way change nothing.
        for k in [-2.0, 1.0, 3.0] {
            let (dt, dr) = gap(&s, &c, &ir, t, t + k * d);
            assert!(dt < 1e-5 && dr < 1e-5, "frame {frame} + {k} loops: {dt:e}");
        }
    }
}

#[test]
fn a_looping_clip_has_no_seam() {
    // Position and rotation return to where they start, as an idle does.
    let h = std::f64::consts::FRAC_1_SQRT_2;
    let back = ClipSpec {
        tracks: vec![TrackSpec {
            position: VecSpec {
                degree: 3,
                knots: clamped_knots(3, 5, 8),
                channels: [
                    ChanSpec::Spline {
                        min: -1.0,
                        max: 1.0,
                        q: vec![0, 30000, 65535, 30000, 0],
                    },
                    ChanSpec::Static(0.5),
                    ChanSpec::Identity,
                ],
            },
            rotation: QuatSpec::Spline {
                degree: 1,
                knots: vec![0, 0, 4, 8, 8],
                points: vec![
                    encode40([0.0, 0.0, 0.0, 1.0]),
                    encode40([0.0, h, 0.0, h]),
                    encode40([0.0, 0.0, 0.0, 1.0]),
                ],
            },
            scale: VecSpec::identity(),
        }],
        track_to_bone: vec![0],
        ..havok_clip()
    };
    let (s, c) = decoded(back);
    let ir = clip(&c, "loop");
    let sk = ir_skeleton(&s);
    let d = f64::from(ir.duration);
    let eps = 1e-7;
    let before = ir.model_pose(&sk, d - eps);
    let after = ir.model_pose(&sk, d + eps);
    for (b, a) in before.iter().zip(&after) {
        for i in 0..3 {
            assert!(
                (b.translation[i] - a.translation[i]).abs() < 1e-4,
                "{b:?} {a:?}"
            );
        }
        let r = angle(b.rotation.map(f64::from), a.rotation.map(f64::from));
        assert!(r < 1e-4, "{r}");
    }
    // At the duration itself time wraps to 0, so the frame there is the first.
    assert_eq!(ir.model_pose(&sk, d), ir.model_pose(&sk, 0.0));
    // The last frame before the wrap is the decoders' too, not an artefact
    // of the evaluator's own clamping.
    let (dt, dr) = gap(&s, &c, &ir, d - eps, d - eps);
    assert!(dt < 1e-5 && dr < 1e-5);
}

#[test]
fn the_rig_is_globals_three_sprite_lights_in_order() {
    let global = disc(&guac_formats::roster::builder::sample()).global;
    let r = rig(&global, identity()).unwrap();
    let names: Vec<&str> = r.lights.iter().map(|l| l.name.as_str()).collect();
    assert_eq!(names, RIG_LIGHTS);
    for (l, (_, diffuse, ambient, specular, rotation)) in r.lights.iter().zip(RIG) {
        assert_eq!(
            (l.diffuse, l.ambient, l.specular),
            (diffuse, ambient, specular)
        );
        assert_eq!((l.rotation, l.multiplier), (rotation, 1.0));
    }
    assert_eq!(r.lights[0].rotation[0], -45.0, "the key's pitch");
    // A level without them.
    let e = rig(&guac_formats::exports::builder::bootstrap(), identity()).unwrap_err();
    assert!(e.to_string().contains("no _spriteLight1"), "{e}");
}

#[test]
fn character_loads_are_each_figure_and_giants_once_in_roster_order() {
    let d = disc(&guac_formats::roster::builder::sample());
    let tmp = tempfile::tempdir().unwrap();
    let usrdir = tmp.path().join("PS3_GAME/USRDIR");
    for (path, bytes) in d.archives() {
        let path = usrdir.join(path);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, bytes).unwrap();
    }
    let roster = crate::roster::read(tmp.path(), identity()).unwrap();
    let loads = character_loads(&roster);
    assert_eq!(
        loads,
        [
            "MinionLoad_000_AirDragon",
            "MinionLoad_000_AirDragonLegendary",
            "MinionLoad_007_GemGolem",
            "MinionLoad_016_Spyro",
            "MinionLoad_016_SpyroDark",
            "MinionLoad_100_Eagle",
            "MinionLoad_100_EagleLegendary",
            "MinionLoad_106_IceSorc",
            "MinionLoad_106_IceSorcLegendary",
            "MinionLoad_112_TitanLife",
            "MinionLoad_112_TitanLifeAlt",
        ]
    );
    assert_eq!(
        archive_path(Path::new("u"), "MinionLoad_000_AirDragon"),
        Path::new("u/character/minionload_000_airdragon.bld")
    );
}
