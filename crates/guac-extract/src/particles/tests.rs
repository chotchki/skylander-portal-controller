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
    // An image of whole blocks keeps them (AH.244 decodes only the rest):
    // each file is its image's blocks as the disc holds them.
    let mut kept = 0;
    for (_, read) in guac_formats::particle::emitters(&igz) {
        let Ok(e) = read else { continue };
        for tex in e.generators.iter().filter_map(|g| g.texture()) {
            let blocks = guac_formats::texture::Texture::read(&igz, tex.image).unwrap();
            assert!(blocks.whole_blocks());
            let as_held = blocks.to_ktx2();
            assert!(
                r.textures.iter().any(|(_, k)| *k == as_held),
                "{:?} stored as its blocks",
                tex.path
            );
            kept += 1;
        }
    }
    assert_eq!(kept, 3, "the trail's three textured generators");
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

    // A good one after it in the file still lands: the refusal stops
    // nothing past it.
    let mut b = IgzBuilder::new();
    let bad = b.object(0, "tfbParticleInfo", 0xE0);
    let good = b.object(0, "tfbParticleInfo", 0xE0);
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

/// AH.244: a block-compressed image a size of no whole blocks, which no
/// GPU takes, is stored decoded: its generator textured, its one KTX2 RGBA8
/// at the image's own size, every level the texels its blocks decode to.
#[test]
fn a_texture_of_no_whole_blocks_is_stored_decoded() {
    let bytes = odd_image();
    let igz = Igz::parse(&bytes).unwrap();
    let r = emitters(&igz, "permanent/global.bld", identity());
    assert!(r.ir.refused.is_empty(), "{:?}", r.ir.refused);
    let g = &r.ir.emitters[0].generators;
    assert_eq!(g.len(), 1);
    let t = g[0].texture.as_ref().expect("textured");
    assert_eq!(t.image, "levelup_arrow.png");
    assert_eq!(r.textures.len(), 1);
    let (path, ktx2) = &r.textures[0];
    assert_eq!(*path, t.texture);
    // The image as the disc holds it: BC blocks of no whole 4 x 4.
    let (_, read) = guac_formats::particle::emitters(&igz).next().unwrap();
    let image = read.unwrap().generators[0].texture().unwrap().image;
    let blocks = guac_formats::texture::Texture::read(&igz, image).unwrap();
    assert!(
        !blocks.whole_blocks(),
        "{} x {}",
        blocks.width,
        blocks.height
    );
    // KTX2's header: VK_FORMAT_R8G8B8A8_UNORM at the image's size and levels.
    let word = |at: usize| u32::from_le_bytes(ktx2[at..at + 4].try_into().unwrap());
    assert_eq!(
        &ktx2[..12],
        &[
            0xAB, 0x4B, 0x54, 0x58, 0x20, 0x32, 0x30, 0xBB, 0x0D, 0x0A, 0x1A, 0x0A
        ]
    );
    assert_eq!(word(12), 37);
    assert_eq!((word(20), word(24)), (blocks.width, blocks.height));
    assert_eq!(word(40) as usize, blocks.levels.len());
    for l in 0..blocks.levels.len() {
        let entry = 80 + 24 * l;
        let at = u64::from_le_bytes(ktx2[entry..entry + 8].try_into().unwrap()) as usize;
        let len = u64::from_le_bytes(ktx2[entry + 8..entry + 16].try_into().unwrap()) as usize;
        assert_eq!(
            &ktx2[at..at + len],
            &blocks.rgba8(l).unwrap()[..],
            "level {l}"
        );
    }
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
    let walkers = vec!["016_Spyro".to_owned()];
    assert_eq!(
        archives_with(&walkers),
        scripts::spawn_archives_with(&walkers)
    );
}

/// AU.1: the part is complete once it holds every walker's emitters the
/// scripts part lists, in either IR; an extract made before it read only
/// the base's resumes for it.
#[test]
fn the_part_wants_every_listed_walkers_emitters() {
    let mut m: crate::extract::Manifest = serde_json::from_value(serde_json::json!({
        "extractor": "0", "layout": 4, "ir_version": 5,
        "identity": identity(), "source_dir": "/d", "complete": true,
        "sources": [], "outputs": []
    }))
    .unwrap();
    assert!(!complete(&m), "no part");
    m.parts.insert(
        PART.into(),
        vec![
            "particles/global.ron".into(),
            "particles/009_eruptor.ron".into(),
        ],
    );
    m.parts.insert(
        scripts::SCRIPTS.into(),
        vec![
            scripts::script_path(crate::roster::GLOBAL),
            scripts::script_path("character/009_eruptor.bld"),
            scripts::WALKERS_FILE.to_owned(),
        ],
    );
    assert!(complete(&m), "no walker listed");
    m.parts
        .get_mut(scripts::SCRIPTS)
        .unwrap()
        .push(scripts::script_path("character/112_titanlife.bld"));
    assert!(!complete(&m), "his emitters missing");
    m.parts
        .get_mut(PART)
        .unwrap()
        .push("particles/112_titanlife.postcard".into());
    assert!(complete(&m));
}

/// AS.17: a level's own emitters land with their generators, each texture
/// once, and where each stands as its file stores it (metres, degrees,
/// percent, its activity state, range and buffer in feet); one that rides
/// a parent is refused by name, and the rest land.
#[test]
fn a_levels_emitters_land_where_they_stand_and_one_riding_is_refused() {
    let bytes = guac_formats::particle::builder::placed();
    let igz = Igz::parse(&bytes).unwrap();
    let r = level_emitters(&igz, "level_213_tutotial", identity());
    assert_eq!(r.ir.level, "level_213_tutotial");
    let names: Vec<&str> =
        r.ir.emitters
            .iter()
            .map(|p| p.emitter.name.as_str())
            .collect();
    assert_eq!(names, ["Standing", "Puff"]);
    let standing = &r.ir.emitters[0];
    assert_eq!(standing.position, [4.0, 5.0, 6.0]);
    assert_eq!(standing.rotation, [-20.0, 45.0, 0.0]);
    assert_eq!(standing.scale, [150.0, 125.0, 100.0]);
    assert_eq!(standing.activity_state, 4);
    assert_eq!(
        (standing.activation_range, standing.activation_range_buffer),
        (70.0, 10.0)
    );
    let puff = &r.ir.emitters[1];
    assert_eq!((puff.activity_state, puff.position), (5, [0.0; 3]));
    // Each with its generator read as a spawn archive's is, the one image
    // stored once.
    for p in &r.ir.emitters {
        assert_eq!(p.emitter.generators.len(), 1);
        let t = p.emitter.generators[0].texture.as_ref().unwrap();
        assert_eq!(t.image, "Particles/textures/dust.png");
        assert_eq!(p.emitter.generators[0].blend, Blend::Add);
    }
    assert_eq!(r.textures.len(), 1);
    assert_eq!(
        r.ir.emitters[0].emitter.generators[0]
            .texture
            .as_ref()
            .unwrap()
            .texture,
        r.textures[0].0
    );
    assert_eq!(r.ir.refused.len(), 1, "{:?}", r.ir.refused);
    assert_eq!(r.ir.refused[0].0, "Riding");
    assert!(
        r.ir.refused[0].1.contains("a parent"),
        "{}",
        r.ir.refused[0].1
    );
}
