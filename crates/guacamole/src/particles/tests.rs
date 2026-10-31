//! A particle's life on generators built here (AH.87), the quads it draws,
//! the material it draws with and the particles' own random numbers.

use bevy::math::Mat4;
use guac_ir::model::{Filter, Sampler, Transform, Wrap};
use guac_ir::particle::Texture;
use guac_ir::shading::DirectionalLight;

use super::*;
use crate::character::CharacterMaterial;
use crate::character::ShadowLookup;
use crate::level_scene::{LevelFog, LevelLights, view_scene};

const IDENTITY: Transform = Transform {
    translation: [0.0; 3],
    rotation: [0.0, 0.0, 0.0, 1.0],
    scale: [1.0; 3],
};

const STILL: Frame = Frame {
    at: Vec3::ZERO,
    turn: Quat::IDENTITY,
    scale: 1.0,
};

fn keyed<T: Copy>(start: T, mid: T, end: T, zero: T) -> Keyed<T> {
    Keyed {
        start,
        start_random: zero,
        mid,
        mid_random: zero,
        end,
        end_random: zero,
    }
}

/// A stream: 50 a second, at most 24, living 0.4 s, born still at the
/// generator's place, no drag or gravity; its colour and size keyed with a
/// middle a quarter of the way.
fn stream() -> Sprite {
    Sprite {
        spawn_rate: 50.0,
        max_alive: 24,
        lifespan: 0.4,
        lifespan_random: 0.0,
        max_distance: 30.0,
        midpoint_percent: 25.0,
        next_gen_scale: 1.0,
        volume: Some(Volume::Box),
        volume_size: [0.0; 3],
        uniform: false,
        inherits_translation: false,
        direction: Some(Direction::Orientation),
        speed: 0.0,
        speed_random: 0.0,
        spread: 0.0,
        drag: [0.0; 3],
        gravity: [0.0; 3],
        colour: keyed([127, 64, 0, 255], [127, 32, 0, 128], [0, 0, 0, 0], [0; 4]),
        size: keyed([0.4; 3], [0.8; 3], [0.0; 3], [0.0; 3]),
        rotation: 0.0,
        rotation_random: 0.0,
        spin: keyed(0.0, 0.0, 0.0, 0.0),
        random_spin_direction: false,
        view_alignment: 0,
    }
}

fn generator(kind: Kind) -> Generator {
    Generator {
        blend: Blend::Add,
        overlay_layer: 5,
        bloom: 0.0,
        offset: IDENTITY,
        start_delay: 0.0,
        texture: Some(Texture {
            texture: "textures/x.ktx2".into(),
            image: "particles/textures/x.png".into(),
            sampler: Sampler {
                wrap_u: Wrap::Clamp,
                wrap_v: Wrap::Clamp,
                mag: Filter::Linear,
                min: Filter::Linear,
                mip: Some(Filter::Linear),
            },
        }),
        kind,
    }
}

/// `steps` frames of `dt`, on one seed so a run's particles repeat.
fn run(state: &mut GeneratorState, g: &Generator, on: bool, steps: usize, dt: f32) {
    let mut rng = Cosmetic::new(7);
    for _ in 0..steps {
        state.step(g, STILL, on, dt, &mut rng);
    }
}

#[test]
fn a_stream_makes_its_rate_and_each_particle_dies_at_the_end_of_its_life() {
    let g = generator(Kind::Sprite(stream()));
    let mut s = GeneratorState::default();
    // A tenth of a second at 50 a second: 5, the carry kept between frames.
    run(&mut s, &g, true, 10, 0.01);
    assert_eq!((s.particles.len(), s.made), (5, 5));
    // Its first came once a whole one was owed, at the end of the second
    // frame, so it's 0.08 s old.
    let oldest = s.particles.iter().map(|p| p.age).fold(0.0, f32::max);
    assert!((oldest - 0.08).abs() < 1e-4, "{oldest}");
    // Taken off having made 5, it goes on making them to its most, 24,
    // and stops; each dies at 0.4 s.
    let mut off = s.clone();
    run(&mut off, &g, false, 100, 0.01);
    assert_eq!(off.made, 24);
    assert!(off.particles.is_empty(), "every one's life is over");
    assert!(!off.live());
    // On, a full life's worth stays alive: 50 a second for 0.4 s, give or
    // take the one being born as the oldest dies.
    run(&mut s, &g, true, 100, 0.01);
    assert!(
        (20..=21).contains(&s.particles.len()),
        "{}",
        s.particles.len()
    );
    assert!(s.particles.iter().all(|p| p.age < p.life));
}

#[test]
fn a_stream_holds_at_its_most_alive() {
    let mut sprite = stream();
    sprite.lifespan = 2.0;
    let g = generator(Kind::Sprite(sprite));
    let mut s = GeneratorState::default();
    run(&mut s, &g, true, 100, 0.01);
    assert_eq!(s.particles.len(), 24, "50 a second for 1 s, held at 24");
}

#[test]
fn a_burst_makes_its_most_at_once_and_only_once() {
    let mut sprite = stream();
    sprite.spawn_rate = 9999.0;
    sprite.max_alive = 3;
    sprite.lifespan = 0.3;
    let g = generator(Kind::Sprite(sprite));
    let mut s = GeneratorState::default();
    run(&mut s, &g, true, 1, 1.0 / 60.0);
    assert_eq!(s.particles.len(), 3);
    // Taken off the next step, its three live out their 0.3 s and none
    // comes after, on or off.
    run(&mut s, &g, true, 30, 1.0 / 60.0);
    assert_eq!((s.particles.len(), s.made), (0, 3));
}

#[test]
fn nothing_comes_before_its_start_delay() {
    let mut g = generator(Kind::Sprite(stream()));
    g.start_delay = 0.2;
    let mut s = GeneratorState::default();
    run(&mut s, &g, true, 19, 0.01);
    assert_eq!(s.made, 0);
    run(&mut s, &g, true, 11, 0.01);
    assert!(s.made > 0);
}

#[test]
fn colour_and_size_run_start_to_middle_to_end_over_a_life() {
    let s = stream();
    let mid = midpoint(s.midpoint_percent);
    assert_eq!(mid, Some(0.25));
    assert_eq!(
        keyed_colour(&s.colour, 0.0, mid, 0.0),
        [127.0, 64.0, 0.0, 255.0]
    );
    assert_eq!(
        keyed_colour(&s.colour, 0.25, mid, 0.0),
        [127.0, 32.0, 0.0, 128.0]
    );
    assert_eq!(keyed_colour(&s.colour, 1.0, mid, 0.0), [0.0; 4]);
    // Halfway from the middle to the end.
    assert_eq!(
        keyed_colour(&s.colour, 0.625, mid, 0.0),
        [63.5, 16.0, 0.0, 64.0]
    );
    assert_eq!(keyed_size(&s.size, 0.125, mid, 0.0), [0.6; 3]);
    assert_eq!(keyed_size(&s.size, 0.625, mid, 0.0), [0.4; 3]);
    // A negative midpoint has none: start to end.
    assert_eq!(midpoint(-50.0), None);
    assert_eq!(keyed_size(&s.size, 0.5, None, 0.0), [0.2; 3]);
    // A draw takes each key that much of its randomness.
    let mut k = s.size;
    k.start_random = [0.2; 3];
    assert_eq!(keyed_size(&k, 0.0, mid, 0.5), [0.5; 3]);
    let mut c = s.colour;
    c.start_random = [200, 0, 0, 0];
    assert_eq!(keyed_colour(&c, 0.0, mid, 1.0)[0], 255.0, "held to a code");
}

#[test]
fn gravity_pulls_down_where_positive_and_drag_slows() {
    let mut sprite = stream();
    sprite.max_alive = 1;
    sprite.spawn_rate = 9999.0;
    sprite.lifespan = 10.0;
    sprite.speed = 2.0;
    // Along the generator's turn, here +Z.
    sprite.gravity = [10.0; 3];
    let g = generator(Kind::Sprite(sprite.clone()));
    let mut s = GeneratorState::default();
    run(&mut s, &g, true, 100, 0.01);
    let p = &s.particles[0];
    assert!(p.velocity.y < -9.0 && p.at.y < -4.0, "{p:?}");
    assert!((p.velocity.z - 2.0).abs() < 1e-4);
    sprite.gravity = [0.0; 3];
    sprite.drag = [1.0; 3];
    let g = generator(Kind::Sprite(sprite));
    let mut s = GeneratorState::default();
    run(&mut s, &g, true, 100, 0.01);
    let v = s.particles[0].velocity.z;
    assert!(v < 2.0 * 0.4 && v > 0.0, "{v}");
}

#[test]
fn a_particle_that_moves_with_its_emitter_keeps_its_place_on_it() {
    let mut sprite = stream();
    sprite.inherits_translation = true;
    let g = generator(Kind::Sprite(sprite.clone()));
    let mut rng = Cosmetic::new(1);
    let mut s = GeneratorState::default();
    let view = View {
        right: Vec3::X,
        up: Vec3::Y,
        eye: Vec3::new(0.0, 0.0, 10.0),
    };
    s.step(&g, STILL, true, 0.1, &mut rng);
    let moved = Frame {
        at: Vec3::new(5.0, 0.0, 0.0),
        ..STILL
    };
    let seg = segment(&s, &g, moved, view, Vec3::ZERO, 0).unwrap();
    let centre: Vec3 = seg.positions[..4]
        .iter()
        .map(|p| Vec3::from_array(*p))
        .sum::<Vec3>()
        / 4.0;
    assert!((centre - moved.at).length() < 1e-4, "{centre}");
    // One that doesn't stays where it was born.
    sprite.inherits_translation = false;
    let g = generator(Kind::Sprite(sprite));
    let mut s = GeneratorState::default();
    s.step(&g, STILL, true, 0.1, &mut rng);
    let seg = segment(&s, &g, moved, view, Vec3::ZERO, 0).unwrap();
    let centre: Vec3 = seg.positions[..4]
        .iter()
        .map(|p| Vec3::from_array(*p))
        .sum::<Vec3>()
        / 4.0;
    assert!(centre.length() < 1e-4, "{centre}");
}

#[test]
fn its_turn_on_the_emitter_sends_it_up_at_a_tilt_of_90() {
    let mut sprite = stream();
    sprite.spawn_rate = 9999.0;
    sprite.max_alive = 1;
    sprite.speed = 3.0;
    let mut g = generator(Kind::Sprite(sprite));
    // The eruption's head and its column: tilt 90, as the extract turns it.
    g.offset.rotation = Quat::from_rotation_x(-std::f32::consts::FRAC_PI_2).to_array();
    let mut s = GeneratorState::default();
    run(&mut s, &g, true, 1, 0.01);
    let v = s.particles[0].velocity;
    assert!((v - Vec3::new(0.0, 3.0, 0.0)).length() < 1e-4, "{v}");
}

#[test]
fn each_particle_is_a_quad_facing_the_camera_in_its_colour_now() {
    let g = generator(Kind::Sprite(stream()));
    let mut s = GeneratorState::default();
    run(&mut s, &g, true, 10, 0.01);
    let view = View {
        right: Vec3::X,
        up: Vec3::Y,
        eye: Vec3::new(0.0, 0.0, 10.0),
    };
    let seg = segment(&s, &g, STILL, view, Vec3::new(0.0, 0.0, -1.0), 3).unwrap();
    assert_eq!(seg.material, 3);
    assert_eq!(seg.positions.len(), 4 * s.particles.len());
    assert_eq!(seg.indices.len(), 6 * s.particles.len());
    // Wound counter-clockwise toward the eye, so a one-sided key draws it.
    for tri in seg.indices.chunks(3) {
        let [a, b, c] =
            [tri[0], tri[1], tri[2]].map(|i| Vec3::from_array(seg.positions[usize::from(i)]));
        assert!((b - a).cross(c - a).z > 0.0);
    }
    // From its origin: every corner sits a metre nearer the eye than the
    // particles, which stand at the level's origin.
    assert!(seg.positions.iter().all(|p| (p[2] - 1.0).abs() < 1e-5));
    // The youngest, 0.01 s into a 0.4 s life, is nearly its start colour.
    let colours = seg.colors.as_ref().unwrap();
    assert!(colours.iter().any(|c| c[0] == 127 && c[1] >= 60));
    assert!(seg.tangents.is_some() && seg.uvs.len() == 1);
}

#[test]
fn a_quad_in_its_generators_plane_still_faces_the_eye() {
    let mut sprite = stream();
    sprite.view_alignment = 1;
    let g = generator(Kind::Sprite(sprite));
    let mut s = GeneratorState::default();
    run(&mut s, &g, true, 5, 0.01);
    for eye in [Vec3::new(0.0, 0.0, 10.0), Vec3::new(0.0, 0.0, -10.0)] {
        let view = View {
            right: Vec3::X,
            up: Vec3::Y,
            eye,
        };
        let seg = segment(&s, &g, STILL, view, Vec3::ZERO, 0).unwrap();
        let [a, b, c] =
            [0, 1, 2].map(|i| Vec3::from_array(seg.positions[usize::from(seg.indices[i])]));
        assert!((b - a).cross(c - a).dot(eye) > 0.0, "{eye}");
    }
}

#[test]
fn a_ribbon_follows_its_emitter_tapering_to_its_tail() {
    let ribbon = Ribbon {
        taper: 1,
        segments: 4,
        scroll_uv_percent: 0.0,
        scale_uv_percent: 100.0,
        width: 1.0,
        head_colour: [255, 102, 0, 255],
        tail_colour: [255, 157, 0, 45],
        granularity: 100.0,
    };
    let g = generator(Kind::Ribbon(ribbon));
    let mut s = GeneratorState::default();
    let mut rng = Cosmetic::new(3);
    for k in 0..10 {
        let at = Frame {
            at: Vec3::new(k as f32, 0.0, 0.0),
            ..STILL
        };
        s.step(&g, at, true, 1.0 / 60.0, &mut rng);
    }
    // Its four segments' places, the head where the emitter is.
    assert_eq!(s.points.len(), 5);
    assert_eq!(s.points[0].at, Vec3::new(9.0, 0.0, 0.0));
    let view = View {
        right: Vec3::X,
        up: Vec3::Y,
        eye: Vec3::new(5.0, 0.0, 10.0),
    };
    let seg = segment(&s, &g, STILL, view, Vec3::ZERO, 0).unwrap();
    assert_eq!(seg.positions.len(), 4 * 4);
    // The head half a width either side, the tail a point; its colour
    // halved for the shader's doubling, its alpha as stored.
    let mut head: Vec<f32> = seg.positions[1..3].iter().map(|p| p[1]).collect();
    head.sort_by(f32::total_cmp);
    assert_eq!(head, [-0.5, 0.5]);
    let tail = &seg.positions[seg.positions.len() - 4];
    assert!(tail[1].abs() < 1e-5);
    assert_eq!(seg.colors.as_ref().unwrap()[1], [128, 51, 0, 255]);
    // Taken off, its places age out and it's gone.
    for _ in 0..20 {
        s.step(&g, STILL, false, 1.0 / 60.0, &mut rng);
    }
    assert!(!s.live());
}

#[test]
fn the_particles_draw_their_own_numbers_and_never_the_sessions() {
    let mut session = guac_script::Generator::seeded(5150);
    let before = session.clone();
    let mut sprite = stream();
    sprite.volume_size = [1.0; 3];
    sprite.spread = 1.0;
    sprite.speed_random = 1.0;
    let g = generator(Kind::Sprite(sprite));
    let mut a = GeneratorState::default();
    let mut rng = Cosmetic::new(42);
    for _ in 0..60 {
        a.step(&g, STILL, true, 1.0 / 60.0, &mut rng);
        // The scripts roll as they would with no particles at all.
        session.next_u32();
    }
    let mut alone = before.clone();
    for _ in 0..60 {
        alone.next_u32();
    }
    assert!(session == alone, "the session's draws are its own");
    assert_ne!(rng, Cosmetic::new(42), "the particles drew theirs");
    // Their numbers are the cosmetic generator's: the same seed gives the
    // same particles, another seed others.
    let mut b = GeneratorState::default();
    let mut again = Cosmetic::new(42);
    for _ in 0..60 {
        b.step(&g, STILL, true, 1.0 / 60.0, &mut again);
    }
    assert_eq!(a, b);
    let mut c = GeneratorState::default();
    let mut other = Cosmetic::new(43);
    for _ in 0..60 {
        c.step(&g, STILL, true, 1.0 / 60.0, &mut other);
    }
    assert_ne!(a.particles, c.particles);
}

#[test]
fn the_cosmetic_generator_spreads_its_units_over_zero_to_one() {
    let mut rng = Cosmetic::new(9);
    let draws: Vec<f32> = (0..10_000).map(|_| rng.unit()).collect();
    assert!(draws.iter().all(|u| (0.0..1.0).contains(u)));
    let mean = draws.iter().sum::<f32>() / draws.len() as f32;
    assert!((mean - 0.5).abs() < 0.02, "{mean}");
}

fn level_like_scene() -> guac_ir::shading::Scene {
    let lights = LevelLights {
        directional: [DirectionalLight::OFF; 3],
        points: Vec::new(),
        left_out: Vec::new(),
    };
    view_scene(&lights, &LevelFog::NONE, Mat4::IDENTITY)
}

#[test]
fn a_generator_draws_unlit_by_its_blend_and_glows_by_its_alpha_with_a_bloom() {
    let scene = level_like_scene();
    assert!(
        scene.fog && scene.point_lights.len() == 3,
        "a level's scene"
    );
    let mut g = generator(Kind::Sprite(stream()));
    let m = material("009_LavaBall_Trail", 1, &g).unwrap();
    assert_eq!(m.name, "particle 009_LavaBall_Trail#1");
    assert_eq!(m.states.blend, model::Blend::Additive);
    assert!(!m.states.depth_write && !m.states.two_sided);
    assert_eq!(m.params.glow, Glow::None);
    let c =
        CharacterMaterial::from_ir(&m, &scene, ShadowLookup::NONE, |_| Default::default()).unwrap();
    // The level's fog and point lights owned off: texture times colour.
    assert_eq!(
        (c.key.shader.pixel(), c.key.shader.vertex()),
        (0x0000, 0x1000)
    );
    g.blend = Blend::Blend;
    g.bloom = 5.0;
    let m = material("x", 0, &g).unwrap();
    assert_eq!(m.states.blend, model::Blend::Alpha);
    assert_eq!(m.params.glow, Glow::LayerAlpha);
    g.blend = Blend::Knockout;
    let m = material("x", 0, &g).unwrap();
    assert_eq!(
        (m.states.blend, m.states.alpha_test),
        (model::Blend::Alpha, Some(0.5))
    );
    g.texture = None;
    assert!(material("x", 0, &g).is_none(), "untextured draws nothing");
}

#[test]
fn the_emitters_draw_one_material_a_textured_generator_keyed_on_their_shape() {
    let mut glowing = generator(Kind::Sprite(stream()));
    glowing.bloom = 10.0;
    let mut bare = generator(Kind::Sprite(stream()));
    bare.texture = None;
    let emitter = |name: &str, gs: Vec<Generator>| guac_ir::particle::Emitter {
        name: name.into(),
        object: 1,
        generators: gs,
        unread: Vec::new(),
    };
    let archive = |es| Emitters {
        identity: guac_ir::DumpIdentity {
            title_id: "BLUS30968".into(),
            app_ver: "01.00".into(),
            manifest_hash: guac_ir::Sha256([0; 32]),
        },
        archive: "a".into(),
        emitters: es,
        refused: Vec::new(),
    };
    let parts = [
        archive(vec![emitter(
            "Trail",
            vec![glowing.clone(), bare, generator(Kind::Sprite(stream()))],
        )]),
        archive(vec![
            emitter("Trail", vec![glowing]),
            emitter("Steam", vec![]),
        ]),
    ];
    let (d, index) = drawn(&parts);
    assert_eq!(
        index,
        vec![
            (
                "Trail".to_owned(),
                vec![(0, Some(0)), (1, None), (2, Some(1))]
            ),
            ("Steam".to_owned(), vec![])
        ]
    );
    assert_eq!((d.materials.len(), d.segments.len()), (2, 2));
    let keys = crate::level_scene::keys(&d, &level_like_scene()).unwrap();
    // One colour key for both, and the glowing one's mask.
    assert_eq!(keys.len(), 2);
    let k = keys[0].0;
    assert_eq!(
        (
            k.mesh.uv_sets,
            k.mesh.tangents,
            k.mesh.colours,
            k.mesh.skinned
        ),
        (1, true, true, false)
    );
    assert_eq!(keys[1].0.material.pass, crate::character::Pass::LevelGlow);
}
