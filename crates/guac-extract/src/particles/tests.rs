//! The particle part's reader on guac-formats' synthetic emitters (AH.87).

use guac_formats::igz::builder::IgzBuilder;
use guac_formats::particle::builder::{odd_image, sample, seeds};
use guac_ir::Sha256;
use guac_ir::model::{Filter, Wrap};

use super::*;

fn identity() -> DumpIdentity {
    DumpIdentity {
        title_id: "BLUS30968".into(),
        app_ver: "01.00".into(),
        manifest_hash: Sha256([9; 32]),
    }
}

#[test]
fn every_emitter_lands_with_its_generators_and_each_texture_once() {
    let s = sample();
    let igz = Igz::parse(&s.bytes).unwrap();
    let r = emitters(&igz, "character/009_eruptor.bld", identity());
    assert!(r.ir.refused.is_empty(), "{:?}", r.ir.refused);
    let names: Vec<&str> = r.ir.emitters.iter().map(|e| e.name.as_str()).collect();
    assert_eq!(names, ["Trail", "Quiet"]);
    let trail = &r.ir.emitters[0];
    assert_eq!(trail.unread, ["LightningGenerator"]);
    let kinds: Vec<&str> = trail
        .generators
        .iter()
        .map(|g| match g.kind {
            Kind::Sprite(_) => "sprite",
            Kind::Ribbon(_) => "ribbon",
        })
        .collect();
    assert_eq!(kinds, ["sprite", "sprite", "ribbon"]);
    let blends: Vec<Blend> = trail.generators.iter().map(|g| g.blend).collect();
    assert_eq!(blends, [Blend::Add, Blend::Blend, Blend::Blend]);
    assert_eq!(
        trail.generators.iter().map(|g| g.bloom).collect::<Vec<_>>(),
        [0.0, 5.0, 0.0]
    );
    // Two images, the smoke shared by a sprite and the ribbon: two files.
    assert_eq!(r.textures.len(), 2);
    let t: Vec<&Texture> = trail
        .generators
        .iter()
        .map(|g| g.texture.as_ref().unwrap())
        .collect();
    assert_eq!(t[0].texture, t[2].texture);
    assert_ne!(t[0].texture, t[1].texture);
    for x in &t {
        assert!(
            r.textures.iter().any(|(p, _)| *p == x.texture),
            "{}",
            x.texture
        );
        assert!(x.texture.starts_with("textures/") && x.texture.ends_with(".ktx2"));
    }
    assert_eq!(t[1].image, "Particles/textures/spark.png");
    assert_eq!(
        (t[0].sampler.wrap_u, t[0].sampler.wrap_v, t[0].sampler.mip),
        (Wrap::Clamp, Wrap::Repeat, Some(Filter::Linear))
    );
    assert!(r.ir.emitters[1].generators.is_empty());
}

#[test]
fn a_sprite_keeps_its_records_as_stored_with_their_bits_named() {
    let s = sample();
    let igz = Igz::parse(&s.bytes).unwrap();
    let r = emitters(&igz, "character/009_eruptor.bld", identity());
    let g = &r.ir.emitters[0].generators;
    let (Kind::Sprite(a), Kind::Sprite(b)) = (&g[0].kind, &g[1].kind) else {
        panic!("{g:?}");
    };
    assert_eq!(
        (a.spawn_rate, a.max_alive, a.lifespan, a.midpoint_percent),
        (40.0, 24, 0.5, 35.0)
    );
    assert_eq!(
        (a.volume, a.direction, a.inherits_translation, a.uniform),
        (Some(Volume::Sphere), Some(Direction::Outward), true, false)
    );
    assert_eq!(
        (b.volume, b.direction, b.inherits_translation, b.uniform),
        (Some(Volume::Cylinder), Some(Direction::Inward), false, true)
    );
    assert_eq!(a.drag, [0.5, 0.625, 0.75]);
    assert_eq!(a.gravity, [-6.0, -9.0, -12.0]);
    assert_eq!(a.colour.start, [0x7F, 0x40, 0x00, 0xFF]);
    assert_eq!(a.colour.mid, [0x7F, 0x39, 0x00, 0xFF]);
    assert_eq!(a.colour.end, [0x7F, 0x3A, 0x01, 0x00]);
    assert_eq!(a.size.mid, [0.30, 0.31, 0.32]);
    assert_eq!((a.spin.start, a.spin.mid, a.spin.end), (5.06, 5.24, 13.96));
    assert!(a.random_spin_direction && !b.random_spin_direction);
    assert_eq!((a.view_alignment, b.view_alignment), (0, 2));
    // Where it sits on its emitter: the position as stored.
    assert_eq!(g[0].offset.translation, [0.25, 0.5, 0.75]);
    assert_eq!(g[0].start_delay, 0.125);
    let Kind::Ribbon(ribbon) = &g[2].kind else {
        panic!("{:?}", g[2]);
    };
    assert_eq!((ribbon.taper, ribbon.segments, ribbon.width), (1, 7, 1.0));
    assert_eq!(ribbon.head_colour, [0xFF, 0x66, 0x00, 0xFF]);
    assert_eq!(ribbon.tail_colour, [0xFF, 0x9D, 0x00, 0x2D]);
}

#[test]
fn an_emitter_that_does_not_read_is_listed_by_name_and_the_rest_land() {
    let bent = &seeds()[1];
    let igz = Igz::parse(bent).unwrap();
    let r = emitters(&igz, "permanent/global.bld", identity());
    assert!(r.ir.emitters.is_empty());
    assert_eq!(r.ir.refused.len(), 1);
    assert_eq!(r.ir.refused[0].0, "Bent");
    assert!(
        r.ir.refused[0].1.contains("GeneratorInfoList"),
        "{}",
        r.ir.refused[0].1
    );

    // A good one beside it still lands.
    let mut b = IgzBuilder::new();
    let good = b.object(0, "tfbParticleInfo", 0xE0);
    let bad = b.object(0, "tfbParticleInfo", 0xE0);
    let record = b.object(0, "GeneratorGenerationFields", 0x34);
    let at = |l: guac_formats::igz::Location, o| l.offset_by(o).unwrap();
    guac_formats::level::builder::placement(&mut b, good, "Good", 1, &[0.0; 6], 5);
    guac_formats::level::builder::placement(&mut b, bad, "Bad", 2, &[0.0; 6], 5);
    b.pointer(at(bad, 0xD8), record);
    let bytes = b.build();
    let igz = Igz::parse(&bytes).unwrap();
    let r = emitters(&igz, "permanent/global.bld", identity());
    assert_eq!(r.ir.emitters.len(), 1);
    assert_eq!(r.ir.emitters[0].name, "Good");
    assert_eq!(
        r.ir.refused
            .iter()
            .map(|(n, _)| n.as_str())
            .collect::<Vec<_>>(),
        ["Bad"]
    );
}

/// A block-compressed image a size of no whole blocks no GPU takes: its
/// generator lands untextured, so it draws nothing, and nothing is stored.
#[test]
fn a_texture_of_no_whole_blocks_leaves_its_generator_untextured() {
    let bytes = odd_image();
    let igz = Igz::parse(&bytes).unwrap();
    let r = emitters(&igz, "permanent/global.bld", identity());
    assert!(r.ir.refused.is_empty(), "{:?}", r.ir.refused);
    let g = &r.ir.emitters[0].generators;
    assert_eq!(g.len(), 1);
    assert!(g[0].texture.is_none());
    assert!(r.textures.is_empty());
}

#[test]
fn each_archives_file_is_named_by_its_stem_in_either_format() {
    assert_eq!(
        path(IrFormat::Ron, "character/009_eruptor.bld"),
        "particles/009_eruptor.ron"
    );
    assert_eq!(
        path(IrFormat::Postcard, "permanent/global.bld"),
        "particles/global.postcard"
    );
    assert_eq!(archives(), scripts::spawn_archives());
}
