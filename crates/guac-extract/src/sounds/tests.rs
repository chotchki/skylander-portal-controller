//! The sounds part's reader on guac-formats' synthetic sounds (AR.5): the
//! sound builder's `level.bld`, its English pack, an `.arc` with the
//! streams they name, `global.bld`'s exports.

use guac_formats::exports::builder::{bootstrap, names, rooted};
use guac_formats::fsb::builder::{BankSpec, SampleSpec, write};
use guac_formats::iga::builder::IgaBuilder;
use guac_formats::mpeg::builder::stream;
use guac_formats::sound::builder::{GLOBAL_ID, pack, sample};
use guac_ir::Sha256;

use super::*;

fn identity() -> DumpIdentity {
    DumpIdentity {
        title_id: "BLUS30968".into(),
        app_ver: "01.00".into(),
        manifest_hash: Sha256([9; 32]),
    }
}

fn exports() -> Exports {
    let boot = bootstrap();
    let global = rooted(&names("global", 24), &[]);
    Exports::new(&Igz::parse(&boot).unwrap(), &Igz::parse(&global).unwrap()).unwrap()
}

/// A Giants bank of `frames` frames: one MPEG sample.
fn bank(frames: usize, gap: usize) -> Vec<u8> {
    write(&BankSpec::new(vec![SampleSpec::new(
        stream(frames, gap),
        frames as u32 * 1152,
    )]))
    .bytes
}

fn entry(path: &str, hex: &str) -> String {
    format!("c:/tfb/build/ps3/{path}/0x{hex}.wav.hz.wav.enc")
}

/// The archive: the sample's `level.bld` and an English pack in the
/// `.bld`, and an `.arc` streaming the English music, the pack's two lines
/// (the second the same bank as the music) and the Swedish line's own
/// entry.
fn archive() -> (Vec<u8>, Vec<u8>) {
    let bld = IgaBuilder::new()
        .lzma("level.bld", &sample().bytes)
        .lzma("ENGLISH.pak", &pack("ENGLISH"))
        .lzma("SWEDISH.pak", &pack("SWEDISH"))
        .build();
    let music = bank(5, 3);
    let arc = IgaBuilder::new()
        .stored(
            &entry("levels/level_213/sounds/mu_213_explore.wav", "a350e320"),
            &music,
        )
        .stored(
            &entry("levels/npc_audio/eon/vo_eon_lvl213_001.wav", "11111111"),
            &bank(2, 0),
        )
        .stored(
            &entry("levels/npc_audio/eon/vo_eon_lvl213_014.wav", "22222222"),
            &music,
        )
        .stored(
            &entry(
                "levels/npc_audio/eon/swedish/vo_eon_lvl213_014.wav",
                "33333333",
            ),
            &bank(4, 3),
        )
        .stored(
            "C:/tfb/build/ps3/models/x.png/0x44444444.png.igb.tex.igz",
            b"IGZ\x01",
        )
        .build();
    (bld, arc)
}

#[test]
fn every_sound_and_data_info_lands_with_its_audio() {
    let (bld, arc) = archive();
    let r = read(
        "level/level_213_tutotial.bld",
        &bld,
        Some(&arc),
        &exports(),
        identity(),
    )
    .unwrap();
    let ir = &r.ir;
    // level.bld's four data infos and the pack's two, each home in order.
    let homes: Vec<(DataHome, Option<&str>)> = ir
        .data
        .iter()
        .map(|d| (d.home, d.name.as_deref()))
        .collect();
    assert_eq!(homes.len(), 6);
    assert_eq!(
        &homes[..4].iter().filter(|h| h.0 == DataHome::Level).count(),
        &4
    );
    assert!(
        ir.data
            .windows(2)
            .all(|w| (w[0].home, w[0].object) < (w[1].home, w[1].object))
    );
    let audio = |name: &str| {
        ir.data
            .iter()
            .find(|d| d.name.as_deref() == Some(name))
            .and_then(|d| d.audio.clone())
    };
    let embedded = audio(guac_formats::sound::builder::EMBEDDED_NAME).unwrap();
    assert_eq!(
        (embedded.rate, embedded.channels, embedded.samples),
        (44_100, 2, 3 * 1152)
    );
    let music = audio(guac_formats::sound::builder::ENGLISH_NAME).unwrap();
    assert_eq!(music.samples, 5 * 1152);
    let line = audio("Levels/npc_audio/eon/vo_eon_lvl213_014.wav").unwrap();
    assert_eq!(line.file, music.file, "one bank, one file");
    assert!(music.file.starts_with("audio/sounds/") && music.file.ends_with(".mp3"));
    // Each file's bytes are its bank's frames alone.
    let (_, bytes) = r.audio.iter().find(|(f, _)| *f == music.file).unwrap();
    assert_eq!(
        bytes,
        &guac_formats::mpeg::remux(&stream(5, 3), 5 * 1152)
            .unwrap()
            .bytes
    );
    assert_eq!(audio_path(bytes), music.file);
    // Four distinct banks: the embedded one, the music, the first line and
    // the Swedish line, which its own path finds under its folder.
    assert_eq!(r.audio.len(), 4);
    let swedish = audio(guac_formats::sound::builder::SWEDISH_NAME).unwrap();
    assert_eq!(swedish.samples, 4 * 1152);
    // The streamed ones keep their hash, the embedded one none.
    let english = ir
        .data
        .iter()
        .find(|d| d.name.as_deref() == Some(guac_formats::sound::builder::ENGLISH_NAME))
        .unwrap();
    assert_eq!(
        english.streamed,
        Some(guac_formats::sound::builder::STREAM_HASH as i32)
    );
    // The Swedish line alone carries a lip-sync stream.
    let synced: Vec<_> = ir.data.iter().filter(|d| d.lip_sync).collect();
    assert_eq!(synced.len(), 1);
    assert_eq!(
        synced[0].name.as_deref(),
        Some(guac_formats::sound::builder::SWEDISH_NAME)
    );
    // A file is named by the first 16 hex digits of its bytes' hash.
    assert_eq!(music.file.len(), "audio/sounds/".len() + 16 + ".mp3".len());
}

#[test]
fn each_sounds_data_is_resolved_where_it_lives() {
    let (bld, arc) = archive();
    let r = read(
        "level/level_213_tutotial.bld",
        &bld,
        Some(&arc),
        &exports(),
        identity(),
    )
    .unwrap();
    let ir = &r.ir;
    let refs: Vec<DataRef> = ir.sounds.iter().map(|s| s.data).collect();
    let level = sample();
    let level_igz = Igz::parse(&level.bytes).unwrap();
    let pack_bytes = pack("ENGLISH");
    let pack_igz = Igz::parse(&pack_bytes).unwrap();
    let d0 = level_igz.object_at(level.data[0]).unwrap().0 as u32;
    let pack_d = guac_formats::sound::resolve_pack(&pack_igz, 3).unwrap() as u32;
    let global = guac_formats::sound::resolve_global(&exports(), GLOBAL_ID).unwrap() as u32;
    assert_eq!(
        refs,
        [
            DataRef::Here(DataHome::Level, d0),
            DataRef::Global(global),
            DataRef::None,
            DataRef::Unresolved,
            DataRef::Unresolved,
            DataRef::Here(DataHome::Pack, pack_d),
        ]
    );
    assert!(ir.sounds.windows(2).all(|w| w[0].object < w[1].object));
    // Every number as stored: the builder's first sound's are its offsets.
    let first = &ir.sounds[0];
    assert_eq!(first.name.as_deref(), Some("snd_grunt_hit"));
    assert_eq!((first.flags, first.playback_mode), (0x4000_0129, 1));
    assert_eq!(first.volume_percent, 0x30 as f32);
    assert_eq!(first.attenuation_distance_silent, 0x44 as f32);
    assert_eq!(first.sample_rate_reduction, 0x74 as f32);
    assert_eq!((first.properties >> 1) & 0x1F, 5);
    // The pack's data info found by the sound is one the IR holds.
    assert!(
        ir.data_at(DataHome::Pack, pack_d)
            .is_some_and(|d| d.audio.is_some())
    );
}

#[test]
fn what_doesnt_read_is_listed_and_the_rest_lands() {
    let (bld, arc) = archive();
    let r = read(
        "level/level_213_tutotial.bld",
        &bld,
        Some(&arc),
        &exports(),
        identity(),
    )
    .unwrap();
    let why: Vec<(&str, &str)> =
        r.ir.refused
            .iter()
            .map(|(n, w)| (n.as_str(), w.as_str()))
            .collect();
    // The empty data info names nothing to play.
    assert_eq!(
        why,
        [("Level data info #3", "neither data nor a name to stream by")]
    );
    // A stream whose entry the .arc lacks is listed by its name.
    let no_swedish = IgaBuilder::new()
        .stored(
            &entry("levels/level_213/sounds/mu_213_explore.wav", "a350e320"),
            &bank(5, 3),
        )
        .build();
    let r = read(
        "level/level_213_tutotial.bld",
        &bld,
        Some(&no_swedish),
        &exports(),
        identity(),
    )
    .unwrap();
    assert!(
        r.ir.refused
            .iter()
            .any(|(n, w)| n.contains("swedish") && w.contains("None"))
    );
    assert!(
        r.ir.data
            .iter()
            .any(|d| d.audio.is_some() && d.streamed.is_some())
    );
    // Without the .arc every stream is refused and the embedded bank lands.
    let r = read(
        "level/level_213_tutotial.bld",
        &bld,
        None,
        &exports(),
        identity(),
    )
    .unwrap();
    let with_audio = r.ir.data.iter().filter(|d| d.audio.is_some()).count();
    assert_eq!(with_audio, 1);
    assert_eq!(r.audio.len(), 1);
    assert!(r.ir.refused.iter().any(|(_, w)| w.contains("no .arc")));
}

#[test]
fn an_archive_that_wont_read_is_the_error() {
    let (bld, arc) = archive();
    let ex = exports();
    // No level.bld.
    let empty = IgaBuilder::new()
        .stored("ENGLISH.pak", &pack("ENGLISH"))
        .build();
    assert!(
        read("a.bld", &empty, Some(&arc), &ex, identity())
            .unwrap_err()
            .contains("no level.bld")
    );
    // Not an archive, a level that isn't an IGZ, an .arc that isn't one.
    assert!(read("a.bld", b"nope", Some(&arc), &ex, identity()).is_err());
    let not_igz = IgaBuilder::new().stored("level.bld", b"not an igz").build();
    assert!(
        read("a.bld", &not_igz, None, &ex, identity())
            .unwrap_err()
            .starts_with("level.bld")
    );
    assert!(
        read("a.bld", &bld, Some(b"nope"), &ex, identity())
            .unwrap_err()
            .starts_with(".arc")
    );
}

#[test]
fn a_bank_that_isnt_one_mpeg_sample_is_refused_by_name() {
    let pcm = write(&BankSpec {
        codec: 0x02,
        ..BankSpec::new(vec![SampleSpec::new(vec![1; 64], 32)])
    })
    .bytes;
    assert!(remux(&pcm).unwrap_err().contains("Pcm16"));
    let two = write(&BankSpec::new(vec![
        SampleSpec::new(stream(1, 0), 1152),
        SampleSpec::new(stream(1, 0), 1152),
    ]))
    .bytes;
    assert!(remux(&two).unwrap_err().contains("2 samples"));
    // A count the frames don't reach.
    let short = write(&BankSpec::new(vec![SampleSpec::new(
        stream(2, 0),
        3 * 1152,
    )]))
    .bytes;
    assert!(remux(&short).unwrap_err().contains("frames hold 2304"));
    let (a, bytes) = remux(&bank(3, 3)).unwrap();
    assert_eq!((a.samples, a.loop_points), (3456, None));
    assert_eq!(a.file, audio_path(&bytes));
}

#[test]
fn the_part_reads_global_each_modelled_character_and_each_level() {
    use guac_ir::{Category, FigureKind, RosterFigure, Text};
    let figure = |load: &str, category| RosterFigure {
        toy_type: 0,
        variant: 0,
        kind: FigureKind::Base,
        deco: None,
        name: Text {
            key: String::new(),
            texts: Vec::new(),
        },
        variant_word: None,
        element: None,
        category,
        load_file: load.into(),
        portrait: None,
        sky1: false,
    };
    let roster = Roster {
        identity: identity(),
        languages: Vec::new(),
        figures: vec![
            figure("MinionLoad_009_Eruptor", Category::Figure),
            figure("MinionLoad_009_Eruptor", Category::Figure),
            figure("MinionLoad_110_RobotGiant", Category::Giant),
            figure("Item_Anvil", Category::Item),
        ],
    };
    assert_eq!(
        archives(&roster),
        [
            "permanent/global.bld",
            "character/minionload_009_eruptor.bld",
            "character/minionload_110_robotgiant.bld",
            "level/level_213_tutotial.bld",
        ]
    );
    assert_eq!(
        path(IrFormat::Ron, "permanent/global.bld"),
        "sounds/global.ron"
    );
    assert_eq!(
        path(IrFormat::Postcard, "level/level_213_tutotial.bld"),
        "sounds/level_213_tutotial.postcard"
    );
}
