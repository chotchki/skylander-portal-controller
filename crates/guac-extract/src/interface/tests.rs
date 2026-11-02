//! The interface part's reader on guac-formats' synthetic interface (AT.8):
//! the interface builder's `level.bld` and an English pack in a `.bld`, and
//! an `.arc` holding the streamed image one template names.

use guac_formats::iga::builder::IgaBuilder;
use guac_formats::igz::builder::IgzBuilder;
use guac_formats::interface::builder::{image, sample};
use guac_formats::language::builder::pack;
use guac_ir::Sha256;
use guac_ir::interface::TextKey;

use super::*;

fn identity() -> DumpIdentity {
    DumpIdentity {
        title_id: "BLUS30968".into(),
        app_ver: "01.00".into(),
        manifest_hash: Sha256([9; 32]),
    }
}

/// The sample's two text keys, in English.
pub(crate) fn bld() -> Vec<u8> {
    IgaBuilder::new()
        .lzma("level.bld", &sample().bytes)
        .lzma(
            "ENGLISH.pak",
            &pack(
                "ENGLISH",
                guac_formats::language::ENGLISH,
                &["Unused", "Gold"],
            ),
        )
        .build()
}

/// One `igImage2` as its own IGZ, what an `.arc` entry holds.
pub(crate) fn image_igz(fill: u8) -> Vec<u8> {
    let mut b = IgzBuilder::new();
    let o = b.object(0, "igImage2", 0x60);
    let blocks = b.section(0x10);
    image(&mut b, o, blocks, "batteries_low_1.png", fill);
    b.build()
}

/// The `.arc` beside it: the streamed template's image under its source's
/// folder, and another image beside it.
pub(crate) fn arc() -> Vec<u8> {
    IgaBuilder::new()
        .stored(
            "C:/tfb/build/ps3/levels/includes/portalmessaging/batteries_low_1.png/0x3e79592d.png.igb.tex.igz",
            &image_igz(0x77),
        )
        .stored(
            "C:/tfb/build/ps3/models/x.png/0x44444444.png.igb.tex.igz",
            &image_igz(0x11),
        )
        .build()
}

/// The files' names carry the record's shape (AT.27), so an extract holding
/// AT.8's (`interface/global.ron`, `sprites.ron`) reads the parts again.
#[test]
fn the_files_names_carry_the_records_shape() {
    assert_eq!(guac_ir::interface::RECORD, 2);
    assert_eq!(
        path(IrFormat::Ron, "permanent/global.bld"),
        "interface/global.v2.ron"
    );
    assert_eq!(
        path(IrFormat::Postcard, "misc/ui_options.bld"),
        "interface/ui_options.v2.postcard"
    );
    assert_eq!(
        level_path(IrFormat::Ron, "level_213_tutotial"),
        "levels/level_213_tutotial/sprites.v2.ron"
    );
    assert_eq!(
        crate::levels::part_path(
            IrFormat::Postcard,
            "level_213_tutotial",
            crate::levels::SPRITES
        ),
        "levels/level_213_tutotial/sprites.v2.postcard"
    );
}

#[test]
fn each_template_lands_with_what_it_draws_and_the_text_in_english() {
    let r = read("permanent/global.bld", &bld(), Some(&arc()), identity()).unwrap();
    let ir = &r.ir;
    assert_eq!(ir.archive, "permanent/global.bld");
    assert_eq!(ir.templates.len(), 8);
    // Each string with its text and its settings (the sample's: a time of
    // four digits, a number with an `@`, commas, five digits and one
    // decimal).
    let strings: Vec<(&str, i32, i32, i32)> = ir
        .strings
        .iter()
        .map(|s| (s.text.as_str(), s.flags, s.integer_digits, s.decimal_digits))
        .collect();
    assert_eq!(strings, [("Unused", 0x400, 4, 0), ("Gold", 0x2800, 5, 1)]);
    assert!(ir.refused.is_empty(), "{:?}", ir.refused);
    let t = |n: usize| &ir.templates[n];
    assert_eq!(t(0).content, Content::Container);
    assert_eq!(t(0).properties, 0x2001_660B);
    assert_eq!(t(0).name, "Generic_Container");
    // Its matrices (AT.25), as the builder placed it.
    assert_eq!(
        (t(0).init_matrix.position, t(0).init_matrix.rotation),
        ([1.0, 1.0, 2.0], [0.0, 90.0, 0.0])
    );
    assert_eq!(t(0).matrix.position, [3.0, 0.0, 4.0]);
    let texture = |c: &Content| match c {
        Content::Image(a) => (a.texture.clone(), a.size),
        other => panic!("{other:?}"),
    };
    // The local image, and the streamed one out of the .arc.
    let (local, local_size) = texture(&t(1).content);
    let (streamed, streamed_size) = texture(&t(2).content);
    assert_eq!((local_size, streamed_size), (Some([4, 4]), Some([4, 4])));
    let local = local.unwrap();
    let streamed = streamed.unwrap();
    assert_ne!(local, streamed, "different bytes, different files");
    match &t(3).content {
        Content::Movie(a) => {
            assert_eq!(a.source.as_deref(), Some("Movies/Whirlwind.mov"));
            assert_eq!(a.texture, None);
        }
        other => panic!("{other:?}"),
    }
    assert_eq!(t(4).content, Content::Text(0));
    assert_eq!(t(4).string, Some(TextKey::Local(1)));
    assert_eq!(ir.strings[1].text, "Gold");
    match &t(5).content {
        Content::Imported(Kind::Text, i) => assert_eq!(i.object, 0x33),
        other => panic!("{other:?}"),
    }
    assert!(matches!(t(5).string, Some(TextKey::Imported(i)) if i.object == 0x40));
    assert_eq!(
        t(6).content,
        Content::Model(Some(
            "C:/tfb/Content/Models/UI/Exclamation_Point3.mdl".into()
        ))
    );
    assert_eq!(
        t(7).content,
        Content::Emitter(Some("UI_LevelUp_ArrowParticle_VFX".into()))
    );
    // The font, its atlas a texture of its own.
    let f = &ir.fonts[0];
    assert_eq!((f.height, f.kern_amount, f.glyphs.len()), (82.0, 2.0, 3));
    assert_eq!(f.glyphs[1].kern, [0, 0, 1, 1]);
    assert_eq!(f.atlas_size, [4, 4]);
    // Three textures, each once: the local image, the atlas, the streamed.
    let paths: Vec<&str> = r.textures.iter().map(|(p, _)| p.as_str()).collect();
    assert_eq!(paths.len(), 3, "{paths:?}");
    for p in [&local, &streamed, &f.atlas] {
        assert!(paths.contains(&p.as_str()), "{p}");
        assert!(p.starts_with("textures/") && p.ends_with(".ktx2"));
    }
}

#[test]
fn without_the_arc_a_streamed_image_keeps_no_texture_and_isnt_refused() {
    let r = read("level/level_213_tutotial.bld", &bld(), None, identity()).unwrap();
    match &r.ir.templates[2].content {
        Content::Image(a) => {
            assert_eq!(
                (a.texture.as_deref(), a.stream_archive_hash),
                (None, 0x3E79_592D)
            )
        }
        other => panic!("{other:?}"),
    }
    assert!(r.ir.refused.is_empty());
    assert_eq!(r.textures.len(), 2, "the local image and the atlas");
}

#[test]
fn a_streamed_image_its_arc_lacks_is_refused_by_name() {
    let other = IgaBuilder::new()
        .stored(
            "C:/tfb/build/ps3/models/x.png/0x44444444.png.igb.tex.igz",
            &image_igz(0x11),
        )
        .build();
    let r = read(
        "character/009_eruptor.bld",
        &bld(),
        Some(&other),
        identity(),
    )
    .unwrap();
    assert_eq!(
        r.ir.refused,
        vec![("batteries_are_low".to_owned(), "no .arc entry".to_owned())]
    );
    match &r.ir.templates[2].content {
        Content::Image(a) => assert_eq!(a.texture, None),
        other => panic!("{other:?}"),
    }
}

#[test]
fn an_archive_without_a_level_bld_or_with_a_broken_one_is_refused() {
    let none = IgaBuilder::new().lzma("ENGLISH.pak", b"IGZ").build();
    assert_eq!(
        read("misc/ui_options.bld", &none, None, identity()).unwrap_err(),
        "no level.bld"
    );
    let broken = IgaBuilder::new().lzma("level.bld", b"not an igz").build();
    let e = read("misc/ui_options.bld", &broken, None, identity()).unwrap_err();
    assert!(e.starts_with("level.bld:"), "{e}");
    // A pack whose count is off the level's keys refuses the archive.
    let off = IgaBuilder::new()
        .lzma("level.bld", &sample().bytes)
        .lzma(
            "ENGLISH.pak",
            &pack("ENGLISH", guac_formats::language::ENGLISH, &["one"]),
        )
        .build();
    assert!(read("misc/ui_options.bld", &off, None, identity()).is_err());
    // A pack with no language pack info refuses an archive naming text,
    // and is left unread by one naming none (the voice pack's).
    let no_info = IgzBuilder::new().build();
    let texts = IgaBuilder::new()
        .lzma("level.bld", &sample().bytes)
        .lzma("ENGLISH.pak", &no_info)
        .build();
    assert!(read("misc/ui_options.bld", &texts, None, identity()).is_err());
    let none = IgaBuilder::new()
        .lzma("level.bld", &IgzBuilder::new().build())
        .lzma("ENGLISH.pak", &no_info)
        .build();
    let r = read("level/challengevo.bld", &none, None, identity()).unwrap();
    assert!(r.ir.strings.is_empty() && r.ir.templates.is_empty());
}

#[test]
fn the_menu_archives_are_the_ui_files_and_the_upgrade_screen() {
    for (p, menu) in [
        ("misc/ui_options.bld", true),
        ("misc/upgradeui.bld", true),
        ("misc/ui_options.arc", false),
        ("misc/other.bld", false),
        ("misc/sub/ui_x.bld", false),
        ("misc/ui_sub/ui_x.bld", false),
        ("level/ui_x.bld", false),
    ] {
        assert_eq!(is_menu(p), menu, "{p}");
    }
    assert_eq!(
        path(IrFormat::Ron, "misc/ui_options.bld"),
        "interface/ui_options.v2.ron"
    );
    assert_eq!(
        path(IrFormat::Postcard, "permanent/global.bld"),
        "interface/global.v2.postcard"
    );
}

#[test]
fn a_streamed_image_is_the_arc_entry_under_its_own_folder_alone() {
    // A folder whose name only ends in the source's isn't it.
    let near = IgaBuilder::new()
        .stored(
            "C:/tfb/build/ps3/old_batteries_low_1.png/0x11111111.png.igb.tex.igz",
            &image_igz(0x11),
        )
        .stored(
            "C:/tfb/build/ps3/levels/Batteries_Low_1.png/0x22222222.png.igb.tex.igz",
            &image_igz(0x22),
        )
        .build();
    let arc = Archive::parse(&near).unwrap();
    assert_eq!(
        arc_entry(&arc, "batteries_low_1.png"),
        Ok(1),
        "case ignored"
    );
    // Two entries under its folder name are refused, by count.
    let two = IgaBuilder::new()
        .stored(
            "C:/a/batteries_low_1.png/0x11111111.png.igb.tex.igz",
            &image_igz(0x11),
        )
        .stored(
            "C:/b/batteries_low_1.png/0x22222222.png.igb.tex.igz",
            &image_igz(0x22),
        )
        .build();
    let arc = Archive::parse(&two).unwrap();
    assert_eq!(
        arc_entry(&arc, "batteries_low_1.png"),
        Err("2 .arc entries".to_owned())
    );
}
