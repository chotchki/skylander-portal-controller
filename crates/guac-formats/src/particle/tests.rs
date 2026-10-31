use super::builder::{Sample, sample};
use super::*;
use crate::igz::Location;
use crate::igz::builder::IgzBuilder;
use crate::igz::layouts::builder::list;
use crate::igz::layouts::{BlendMode, SpawnVolume, Taper, VelocityDirection};

fn index(igz: &Igz<'_>, at: Location) -> usize {
    igz.object_at(at).expect("an object there").0
}

fn at(base: Location, offset: u32) -> Location {
    base.offset_by(offset).unwrap()
}

fn sprite<'a, 'b>(e: &'b Emitter<'a>, k: usize) -> &'b Sprite<'a> {
    match &e.generators[k] {
        Generator::Sprite(s) => s,
        g => panic!("generator {k} is {g:?}"),
    }
}

#[test]
fn an_emitter_reads_its_generators_in_list_order_and_keeps_the_rest_unread() {
    let s: Sample = sample();
    let igz = Igz::parse(&s.bytes).unwrap();
    let e = emitter(&igz, index(&igz, s.emitter)).unwrap();
    assert_eq!(e.name, "Trail");
    let kinds: Vec<usize> = e.generators.iter().map(Generator::index).collect();
    assert_eq!(
        kinds,
        [s.sprites[0], s.sprites[1], s.ribbon].map(|l| index(&igz, l))
    );
    assert_eq!(
        e.unread,
        vec![(index(&igz, s.lightning), "LightningGenerator")]
    );
    assert!(matches!(e.generators[2], Generator::Ribbon(_)));
}

#[test]
fn a_sprite_generator_reads_every_field_of_its_three_records() {
    let s = sample();
    let igz = Igz::parse(&s.bytes).unwrap();
    let e = emitter(&igz, index(&igz, s.emitter)).unwrap();
    let a = sprite(&e, 0);
    let b = sprite(&e, 1);

    assert_eq!(a.info.vfx.name, None);
    assert_eq!(b.info.vfx.name, Some("sparks"));
    assert_eq!(a.info.vfx.blend_mode(), Some(BlendMode::Add));
    assert_eq!(a.info.vfx.overlay_layer(), 5);
    assert_eq!(b.info.vfx.blend_mode(), Some(BlendMode::Blend));
    assert_eq!(b.info.vfx.overlay_layer(), 0);
    assert_eq!(b.info.vfx.bloom_intensity, 5.0);
    assert_eq!(a.info.vfx.init_matrix.position, [0.25, 0.5, 0.75]);
    assert_eq!(a.info.vfx.init_matrix.rotation, [10.0, 180.0, 30.0]);
    assert_eq!(a.info.vfx.start_delay, 0.125);

    let g = b.generation;
    assert_eq!(
        (g.spawn_rate, g.max_alive, g.currently_alive),
        (41.0, 25, 0)
    );
    assert_eq!(
        (
            g.lifespan,
            g.lifespan_randomness,
            g.max_distance,
            g.midpoint_percent,
            g.next_gen_scale_factor
        ),
        (1.5, 0.125, 25.0, 36.0, 1.0)
    );

    // Both ways round for each of the movement record's bits.
    let m = a.movement;
    assert_eq!(m.spawn_volume, [0.1, 0.2, 0.3]);
    assert_eq!(
        (
            m.initial_speed,
            m.initial_speed_randomness,
            m.initial_direction_spread
        ),
        (1.5, 0.25, 0.0873)
    );
    assert_eq!((m.start_drag, m.mid_drag, m.end_drag), (0.5, 0.625, 0.75));
    assert_eq!(
        (m.start_gravity, m.mid_gravity, m.end_gravity),
        (-6.0, -9.0, -12.0)
    );
    assert!(!m.uniform_distribution() && m.inherits_translation());
    assert_eq!(
        SpawnVolume::of(m.spawn_volume_code()),
        Some(SpawnVolume::Sphere)
    );
    assert_eq!(
        VelocityDirection::of(m.velocity_code()),
        Some(VelocityDirection::Outward)
    );
    let m = b.movement;
    assert!(m.uniform_distribution() && !m.inherits_translation());
    assert_eq!(
        SpawnVolume::of(m.spawn_volume_code()),
        Some(SpawnVolume::Cylinder)
    );
    assert_eq!(
        VelocityDirection::of(m.velocity_code()),
        Some(VelocityDirection::Inward)
    );

    let p = a.appearance;
    assert_eq!(p.start_color, [0x7F, 0x40, 0x00, 0xFF]);
    assert_eq!(p.start_color_randomness, [0x10, 0x20, 0x30, 0x00]);
    assert_eq!(p.end_color, [0x7F, 0x3A, 0x01, 0x00]);
    assert_eq!(p.end_color_randomness, [1, 2, 3, 4]);
    assert_eq!(p.mid_color, [0x7F, 0x39, 0x00, 0xFF]);
    assert_eq!(p.mid_color_randomness, [5, 6, 7, 8]);
    assert_eq!(p.start_size, [0.45, 0.46, 0.47]);
    assert_eq!(p.start_size_randomness, [0.15, 0.16, 0.17]);
    assert_eq!(p.end_size, [0.06, 0.07, 0.08]);
    assert_eq!(p.end_size_randomness, [0.11, 0.12, 0.13]);
    assert_eq!(p.mid_size, [0.30, 0.31, 0.32]);
    assert_eq!(p.mid_size_randomness, [0.01, 0.02, 0.03]);
    assert_eq!(
        (
            p.initial_rotation_angle,
            p.initial_rotation_angle_randomness
        ),
        (1.57, 6.0)
    );
    assert_eq!(
        (
            p.start_rotation_speed,
            p.start_rotation_speed_randomness,
            p.end_rotation_speed,
            p.end_rotation_speed_randomness,
            p.mid_rotation_speed,
            p.mid_rotation_speed_randomness
        ),
        (5.06, 0.5, 13.96, 0.25, 5.24, 0.75)
    );
    assert!(p.random_rotation_direction());
    assert_eq!((p.view_alignment(), p.version()), (0, 1));
    let p = b.appearance;
    assert!(!p.random_rotation_direction());
    assert_eq!((p.view_alignment(), p.version()), (2, 0));
}

#[test]
fn a_ribbon_reads_its_segment_and_ribbon_fields() {
    let s = sample();
    let igz = Igz::parse(&s.bytes).unwrap();
    let e = emitter(&igz, index(&igz, s.emitter)).unwrap();
    let Generator::Ribbon(r) = &e.generators[2] else {
        panic!("{:?}", e.generators[2]);
    };
    let i = r.info;
    assert_eq!(i.vfx.name, Some("ribbon"));
    assert_eq!(i.vfx.blend_mode(), Some(BlendMode::Blend));
    assert_eq!(i.vfx.overlay_layer(), 5);
    assert_eq!(
        (i.taper(), i.version(), i.segment_count()),
        (Taper::End, 0, 7)
    );
    assert_eq!(
        (i.scroll_uv_percent, i.scale_uv_percent, i.segment_width),
        (12.5, 100.0, 1.0)
    );
    assert_eq!(i.tint_color, [0xFF, 0x66, 0x00, 0xFF]);
    assert_eq!(i.end_color, [0xFF, 0x9D, 0x00, 0x2D]);
    assert_eq!(i.segment_granularity, 100.0);
}

#[test]
fn each_generator_names_the_texture_it_draws_through_its_bind() {
    let s = sample();
    let igz = Igz::parse(&s.bytes).unwrap();
    let e = emitter(&igz, index(&igz, s.emitter)).unwrap();
    let t: Vec<_> = e
        .generators
        .iter()
        .map(|g| *g.texture().expect("textured"))
        .collect();
    assert_eq!(t[0].path, Some("Particles/textures/smoke.png"));
    assert_eq!(t[1].path, Some("Particles/textures/spark.png"));
    // The ribbon shares the first sprite's.
    assert_eq!(t[2], t[0]);
    assert_eq!(t[0].data, index(&igz, s.textures[0]));
    assert_eq!(t[0].image, index(&igz, s.images[0]));
    assert_eq!(t[1].image, index(&igz, s.images[1]));
    assert_eq!((t[0].attr.mag_filter, t[0].attr.min_filter), (1, 5));
    assert_eq!((t[0].attr.wrap_s, t[0].attr.wrap_t), (0, 1));
}

#[test]
fn every_emitter_in_a_file_is_listed_in_object_order() {
    let s = sample();
    let igz = Igz::parse(&s.bytes).unwrap();
    let all = emitters(&igz);
    let names: Vec<(usize, &str)> = all
        .iter()
        .map(|(i, e)| (*i, e.as_ref().unwrap().name))
        .collect();
    assert_eq!(
        names,
        vec![
            (index(&igz, s.emitter), "Trail"),
            (index(&igz, s.quiet), "Quiet")
        ]
    );
    let quiet = all[1].1.as_ref().unwrap();
    assert!(quiet.generators.is_empty() && quiet.unread.is_empty());
}

#[test]
fn a_generator_list_that_is_a_record_is_refused() {
    let bent = &builder::seeds()[1];
    let igz = Igz::parse(bent).unwrap();
    let e = emitter(&igz, 0).unwrap_err();
    assert!(
        matches!(e.kind, ErrorKind::WrongType { expected, .. } if expected == "GeneratorInfoList"),
        "{e}"
    );
}

/// An emitter whose list holds one sprite generator, its records as
/// `records` says (generation, movement, appearance).
fn one_sprite(records: [bool; 3], extra: usize) -> Vec<u8> {
    let mut b = IgzBuilder::new();
    let emitter = b.object(0, "tfbParticleInfo", 0xE0);
    let gil = b.object(0, "GeneratorInfoList", 0x18);
    let g = b.object(0, "GeneratorInfo", 0x54);
    let r = [
        b.object(0, "GeneratorGenerationFields", 0x34),
        b.object(0, "GeneratorMovementFields", 0x4C),
        b.object(0, "GeneratorAppearanceFields", 0x9C),
    ];
    let blocks = b.section(0x10);
    crate::level::builder::placement(&mut b, emitter, "One", 1, &[0.0; 6], 5);
    b.pointer(at(emitter, 0xD8), gil);
    let elements = vec![Some(g); 1 + extra];
    list(&mut b, gil, blocks, &elements);
    for (k, (&on, offset)) in records.iter().zip([0x44, 0x48, 0x50]).enumerate() {
        if on {
            b.pointer(at(g, offset), r[k]);
        }
    }
    b.build()
}

#[test]
fn a_sprite_generator_missing_a_record_is_refused_by_name() {
    for (records, field) in [
        ([false, true, true], "_generationFields"),
        ([true, false, true], "_movementFields"),
        ([true, true, false], "_appearanceFields"),
    ] {
        let bytes = one_sprite(records, 0);
        let igz = Igz::parse(&bytes).unwrap();
        let e = emitter(&igz, 0).unwrap_err();
        assert_eq!(
            (e.field, e.kind),
            (field, ErrorKind::Missing),
            "{records:?}"
        );
    }
    let bytes = one_sprite([true; 3], 0);
    let igz = Igz::parse(&bytes).unwrap();
    let e = emitter(&igz, 0).unwrap();
    // Untextured: no texture, the rest read.
    assert_eq!(e.generators.len(), 1);
    assert!(e.generators[0].texture().is_none());
}

#[test]
fn a_list_past_the_most_generators_is_refused() {
    let bytes = one_sprite([true; 3], MAX_GENERATORS - 1);
    let igz = Igz::parse(&bytes).unwrap();
    assert_eq!(emitter(&igz, 0).unwrap().generators.len(), MAX_GENERATORS);
    let bytes = one_sprite([true; 3], MAX_GENERATORS);
    let igz = Igz::parse(&bytes).unwrap();
    let e = emitter(&igz, 0).unwrap_err();
    assert_eq!(e.field, "_generatorInfoList");
    assert!(matches!(e.kind, ErrorKind::OutOfRange { .. }), "{e}");
}

#[test]
fn every_seed_reads_or_refuses_without_panicking() {
    for seed in builder::seeds() {
        let igz = Igz::parse(&seed).unwrap();
        let _ = emitters(&igz);
    }
}
