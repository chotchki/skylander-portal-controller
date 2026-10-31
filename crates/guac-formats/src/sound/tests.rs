use super::builder::{
    EMBEDDED_NAME, ENGLISH_NAME, GLOBAL_ID, PACK_ID, STREAM_HASH, SWEDISH_NAME, pack, sample,
};
use super::*;
use crate::igz::layouts::{CATEGORIES, LIMIT_BEHAVIORS};

fn index_of(igz: &Igz<'_>, at: crate::igz::Location) -> usize {
    igz.object_at(at).unwrap().0
}

#[test]
fn a_sound_info_reads_every_field_at_its_offset() {
    let s = sample();
    let igz = Igz::parse(&s.bytes).unwrap();
    let i = index_of(&igz, s.sounds[0]);
    let info = SoundInfo::read(&igz, i).unwrap();
    assert_eq!(info.name, Some("snd_grunt_hit"));
    assert_eq!(
        (info.flags, info.playback_mode, info.playback_percent),
        (0x4000_0129, 1, 75.0)
    );
    // Each scalar is its offset (the builder's seed for the first sound is 0).
    let scalars = [
        (info.volume_percent, 0x30),
        (info.volume_randomness_percent, 0x34),
        (info.pitch_randomness_percent, 0x38),
        (info.pan_percent, 0x3C),
        (info.attenuation_distance_full, 0x40),
        (info.attenuation_distance_silent, 0x44),
        (info.rumble_duration, 0x48),
        (info.reverb_amount, 0x58),
        (info.reverb_randomness_percent, 0x5C),
        (info.filter_percent, 0x60),
        (info.filter_randomness_percent, 0x64),
        (info.filter_attenuation_distance_minimum, 0x68),
        (info.filter_attenuation_distance_maximum, 0x6C),
        (info.filter_maximum_percent, 0x70),
        (info.sample_rate_reduction, 0x74),
    ];
    for (value, offset) in scalars {
        assert_eq!(value, offset as f32, "the field at {offset:#x}");
    }
    assert_eq!(info.data, ObjectRef::Object(index_of(&igz, s.data[0])));
    for r in [
        info.activator,
        info.clones,
        info.parent,
        info.sound_object,
        info.sound_runtime,
        info.tag_list,
    ] {
        assert_eq!(r, ObjectRef::Null);
    }
    let quiet = SoundInfo::read(&igz, index_of(&igz, s.sounds[2])).unwrap();
    assert_eq!(quiet.tag_list, ObjectRef::Object(index_of(&igz, s.data[3])));
    assert_eq!(quiet.volume_percent, 2000.0 + 0x30 as f32);
}

#[test]
fn the_flags_and_the_bitfield_read_their_bits() {
    let s = sample();
    let igz = Igz::parse(&s.bytes).unwrap();
    let a = SoundInfo::read(&igz, index_of(&igz, s.sounds[0])).unwrap();
    assert!(a.looping() && a.attenuates() && a.three_d() && a.from_controller());
    assert!(!a.allow_restart() && !a.filter_attenuates() && !a.ignores_time_scales());
    assert!(a.pitch_is_negative());
    assert_eq!(
        (
            a.category(),
            a.priority(),
            a.actor_limit(),
            a.actor_limit_behavior()
        ),
        (5, 51, 1, 3)
    );
    assert_eq!((a.global_limit(), a.global_limit_behavior()), (3, 3));
    assert_eq!(CATEGORIES[a.category() as usize], "Effects/Enemy/Combat");
    assert_eq!(
        LIMIT_BEHAVIORS[a.actor_limit_behavior() as usize],
        "steal oldest"
    );
    let b = SoundInfo::read(&igz, index_of(&igz, s.sounds[1])).unwrap();
    assert!(b.allow_restart() && b.filter_attenuates() && b.ignores_time_scales());
    assert!(!b.looping() && !b.attenuates() && !b.three_d() && !b.from_controller());
    // Every field at its widest.
    assert!(b.pitch_is_negative());
    assert_eq!(
        (
            b.category(),
            b.priority(),
            b.actor_limit(),
            b.actor_limit_behavior()
        ),
        (31, 4095, 31, 3)
    );
    assert_eq!((b.global_limit(), b.global_limit_behavior()), (31, 3));
}

#[test]
fn a_data_info_reads_its_bank_duration_and_lip_sync() {
    let s = sample();
    let igz = Igz::parse(&s.bytes).unwrap();
    let d = SoundDataInfo::read(&igz, index_of(&igz, s.data[0])).unwrap();
    assert_eq!(d.data_info.info.name, Some(EMBEDDED_NAME));
    assert_eq!(d.data_info.data.map(|m| m.data), Some(&s.bank[..]));
    assert_eq!((d.raw_duration, d.data_info.stream_archive_hash), (1.25, 0));
    assert_eq!(d.viseme_stream, ObjectRef::Null);
    let sv = SoundDataInfo::read(&igz, index_of(&igz, s.data[2])).unwrap();
    assert_eq!(
        sv.viseme_stream,
        ObjectRef::Object(index_of(&igz, s.data[0]))
    );
    assert_eq!(sv.raw_duration, 2.75);
}

#[test]
fn each_data_info_finds_its_bank() {
    let s = sample();
    let igz = Igz::parse(&s.bytes).unwrap();
    let all: Vec<_> = data_infos(&igz)
        .into_iter()
        .map(|(i, r)| (i, r.unwrap()))
        .collect();
    assert_eq!(all.len(), 4);
    let by = |at| all.iter().find(|(i, _)| *i == index_of(&igz, at)).unwrap();
    let (_, embedded) = by(s.data[0]);
    assert_eq!(embedded.bank, Some(Bank::Embedded(&s.bank[..])));
    assert_eq!(embedded.name(), Some(EMBEDDED_NAME));
    let (_, english) = by(s.data[1]);
    assert_eq!(
        english.bank,
        Some(Bank::Streamed {
            path: ENGLISH_NAME.to_ascii_lowercase(),
            hash: STREAM_HASH as i32
        })
    );
    let (_, swedish) = by(s.data[2]);
    assert!(
        matches!(&swedish.bank, Some(Bank::Streamed { path, .. }) if path == &SWEDISH_NAME.to_ascii_lowercase())
    );
    let (_, empty) = by(s.data[3]);
    assert_eq!((empty.bank.as_ref(), empty.name()), (None, None));
}

#[test]
fn each_sound_sorts_out_its_data_ref() {
    let s = sample();
    let igz = Igz::parse(&s.bytes).unwrap();
    let refs: Vec<DataRef> = sounds(&igz)
        .into_iter()
        .map(|(_, r)| r.unwrap().data)
        .collect();
    assert_eq!(
        refs,
        [
            DataRef::Local(index_of(&igz, s.data[0])),
            DataRef::Global(GLOBAL_ID),
            DataRef::None,
            DataRef::Other,
            DataRef::Other,
            DataRef::Pack(PACK_ID),
        ]
    );
}

#[test]
fn a_pack_id_names_the_packs_root_one_based() {
    let bytes = pack("ENGLISH");
    let igz = Igz::parse(&bytes).unwrap();
    let roots: Vec<_> = (0..igz.objects().len())
        .map(|i| igz.type_name(&igz.objects()[i]).to_owned())
        .collect();
    // Root 0 the pack's info, then two data infos, then a sound info.
    let d = resolve_pack(&igz, PACK_ID).unwrap();
    assert_eq!(roots[d], "tfbSoundDataInfo");
    let name = SoundDataInfo::read(&igz, d).unwrap().data_info.info.name;
    assert_eq!(name, Some(&*SWEDISH_NAME.replace("swedish/", "")));
    assert!(resolve_pack(&igz, 2).is_some_and(|o| o != d));
    // Id 1 is the pack's info, 4 a sound info, 0 and 5 nothing.
    for id in [0, 1, 4, 5, u32::MAX] {
        assert_eq!(resolve_pack(&igz, id), None, "id {id}");
    }
}

#[test]
fn a_pack_streams_from_its_languages_folder() {
    assert_eq!(
        localized(
            "Levels/champion_audio/whirlwind/vo_whirlwind_hat_01.wav",
            "SWEDISH"
        ),
        "levels/champion_audio/whirlwind/swedish/vo_whirlwind_hat_01.wav"
    );
    assert_eq!(localized("vo.wav", "german"), "german/vo.wav");
}

#[test]
fn a_global_id_resolves_to_global_bld_objects_only() {
    use crate::exports::builder::{bootstrap, names, rooted};
    let boot = bootstrap();
    let global = rooted(&names("global", 24), &[0x03]);
    let (boot, global) = (Igz::parse(&boot).unwrap(), Igz::parse(&global).unwrap());
    let exports = Exports::new(&boot, &global).unwrap();
    let base = exports.global_base();
    // The first global root is a local object of global.bld.
    assert!(resolve_global(&exports, base).is_some());
    // A bootstrap id and an id past the roots land on none.
    assert_eq!(resolve_global(&exports, 1), None);
    assert_eq!(resolve_global(&exports, base + 1000), None);
}

#[test]
fn a_stream_entry_name_gives_its_path_and_language() {
    assert_eq!(
        stream_path(
            "c:/tfb/build/ps3/levels/level_213/sounds/mu_213_explore.wav/0xa350e320.wav.hz.wav.enc"
        ),
        Some(("levels/level_213/sounds/mu_213_explore.wav", None))
    );
    assert_eq!(
        stream_path(
            "C:/tfb/build/ps3/levels/npc_audio/eon/swedish/vo_eon_lvl213_014.wav/0x7b8e3f75.wav.hz.wav.enc"
        ),
        Some((
            "levels/npc_audio/eon/swedish/vo_eon_lvl213_014.wav",
            Some("swedish")
        ))
    );
    // A texture IGZ, a wrong leaf, a short hex and no prefix are no streams.
    for other in [
        "C:/tfb/build/ps3/models/x/final/eruptor_up5b.png/0x4a5c6eb6.png.igb.tex.igz",
        "c:/tfb/build/ps3/levels/a.wav/0x4a5c6eb6.wav.hz",
        "c:/tfb/build/ps3/levels/a.wav/0x4a5c6e.wav.hz.wav.enc",
        "d:/x/levels/a.wav/0x4a5c6eb6.wav.hz.wav.enc",
        "c:/tfb/build/ps3/0x4a5c6eb6.wav.hz.wav.enc",
    ] {
        assert_eq!(stream_path(other), None, "{other}");
    }
    assert_eq!(
        language_of("Levels/npc_audio/eon/German/vo.wav"),
        Some("german")
    );
    assert_eq!(language_of("Levels/npc_audio/eon/vo.wav"), None);
    assert_eq!(language_of("german"), None);
}

#[test]
fn streams_find_one_entry_by_path_or_say_why_not() {
    let names = [
        "c:/tfb/build/ps3/levels/level_213/sounds/mu_213_explore.wav/0xa350e320.wav.hz.wav.enc",
        "c:/tfb/build/ps3/levels/npc_audio/eon/swedish/vo_eon_lvl213_014.wav/0x7b8e3f75.wav.hz.wav.enc",
        "c:/tfb/build/ps3/levels/npc_audio/eon/vo_eon_lvl213_014.wav/0x11111111.wav.hz.wav.enc",
        "c:/tfb/build/ps3/levels/dup.wav/0x22222222.wav.hz.wav.enc",
        "c:/tfb/build/ps3/levels/dup.wav/0x33333333.wav.hz.wav.enc",
        "C:/tfb/build/ps3/models/x.png/0x44444444.png.igb.tex.igz",
        "c:/tfb/build/ps3/levels/Upper/Case.wav/0x55555555.wav.hz.wav.enc",
    ];
    let streams = Streams::new(names.iter().enumerate().map(|(i, n)| (i, *n)));
    assert_eq!(streams.len(), 5, "the texture isn't a stream");
    assert_eq!(
        streams.find("levels/upper/case.wav"),
        Ok(6),
        "kept lower-cased"
    );
    assert_eq!(streams.find(&ENGLISH_NAME.to_ascii_lowercase()), Ok(0));
    assert_eq!(streams.find(ENGLISH_NAME), Ok(0), "case is ignored");
    assert_eq!(streams.find(SWEDISH_NAME), Ok(1));
    assert_eq!(
        streams.find("Levels/npc_audio/eon/vo_eon_lvl213_014.wav"),
        Ok(2),
        "English and Swedish are entries of their own"
    );
    assert_eq!(streams.find("levels/dup.wav"), Err(Miss::Many(vec![3, 4])));
    assert_eq!(streams.find("levels/none.wav"), Err(Miss::None));
}
