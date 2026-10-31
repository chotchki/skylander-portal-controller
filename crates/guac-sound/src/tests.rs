use guac_ir::Sha256;

use super::*;

fn identity() -> guac_ir::DumpIdentity {
    guac_ir::DumpIdentity {
        title_id: "BLUS30968".into(),
        app_ver: "01.00".into(),
        manifest_hash: Sha256([3; 32]),
    }
}

fn audio(file: &str, samples: u32) -> Audio {
    Audio {
        file: file.into(),
        rate: 48_000,
        channels: 2,
        samples,
        loop_points: None,
    }
}

/// A sound with every setting a value of its own: `seed` plus a step per
/// field.
fn sound(object: u32, name: &str, data: DataRef, flags: u32, properties: u32) -> Sound {
    Sound {
        object,
        name: Some(name.into()),
        data,
        flags,
        properties,
        playback_mode: 1,
        playback_percent: 75.0,
        volume_percent: 80.0,
        volume_randomness_percent: 10.0,
        pitch_randomness_percent: 20.0,
        pan_percent: -30.0,
        attenuation_distance_full: 10.0,
        attenuation_distance_silent: 110.0,
        rumble_duration: 1.5,
        reverb_amount: 50.0,
        reverb_randomness_percent: 5.0,
        filter_percent: 40.0,
        filter_randomness_percent: 4.0,
        filter_attenuation_distance_minimum: 2.0,
        filter_attenuation_distance_maximum: 200.0,
        filter_maximum_percent: 90.0,
        sample_rate_reduction: 60.0,
    }
}

fn data(home: DataHome, object: u32, a: Option<Audio>) -> SoundData {
    SoundData {
        home,
        object,
        name: Some(format!("Levels/{home:?}/{object}.wav")),
        raw_duration: 1.5,
        streamed: None,
        lip_sync: false,
        audio: a,
    }
}

/// Global's one sound and data info, and a level's six sounds: data here,
/// in its pack, in global, none, unresolved, and one whose bank didn't
/// read; a music track among them.
fn book() -> SoundBook {
    let global = Sounds {
        identity: identity(),
        archive: "permanent/global.bld".into(),
        sounds: vec![sound(
            3,
            "snd_coin",
            DataRef::Here(DataHome::Level, 9),
            // Attenuates, its filter too.
            0x4000_0048,
            0,
        )],
        data: vec![data(
            DataHome::Level,
            9,
            Some(audio("audio/sounds/c.mp3", 1152)),
        )],
        refused: Vec::new(),
    };
    // Looping, attenuates, 3D; music; priority 50; actor limit 1 (steal
    // furthest); global limit 3 (steal oldest); pitch negative.
    let music_props = 1 | MUSIC << 1 | 50 << 6 | 1 << 18 | 2 << 23 | 3 << 25 | 3 << 30;
    let level = Sounds {
        identity: identity(),
        archive: "level/Level_213_Tutotial.bld".into(),
        sounds: vec![
            sound(
                10,
                "mu_213_explore",
                DataRef::Here(DataHome::Level, 20),
                0x4000_0029,
                music_props,
            ),
            sound(
                11,
                "vo_eon",
                DataRef::Here(DataHome::Pack, 2),
                0x0100_0104,
                5 << 1,
            ),
            sound(12, "SND_Coin", DataRef::Global(9), 0, 0),
            sound(13, "snd_quiet", DataRef::None, 0, 0),
            sound(14, "snd_lost", DataRef::Unresolved, 0, 0),
            sound(15, "snd_broken", DataRef::Here(DataHome::Level, 21), 0, 0),
            sound(16, "snd_away", DataRef::Here(DataHome::Level, 99), 0, 0),
        ],
        data: vec![
            data(
                DataHome::Level,
                20,
                Some(audio("audio/sounds/m.mp3", 6_197_760)),
            ),
            data(DataHome::Level, 21, None),
            data(DataHome::Pack, 2, Some(audio("audio/sounds/v.mp3", 2304))),
        ],
        refused: vec![("Levels/Level/21.wav".into(), "no entry".into())],
    };
    SoundBook::new([global, level])
}

#[test]
fn a_sound_is_found_by_its_archives_stem_and_object() {
    let b = book();
    let stems: Vec<&str> = b.stems().collect();
    assert_eq!(stems, ["global", "level_213_tutotial"]);
    assert_eq!(stem("character/009_Eruptor.bld"), "009_eruptor");
    assert_eq!(stem("global"), "global");
    let p = b.lookup("level_213_tutotial", 10);
    let p = p.playable().expect("the track plays");
    assert_eq!((p.file, p.audio.samples), ("audio/sounds/m.mp3", 6_197_760));
    assert_eq!(p.sound.name.as_deref(), Some("mu_213_explore"));
    assert_eq!(p.duration, 1.5);
    assert_eq!(b.lookup("level_213_tutotial", 99), Lookup::Unknown);
    assert_eq!(b.lookup("nowhere", 10), Lookup::Unknown);
}

#[test]
fn data_in_the_pack_and_in_global_is_followed() {
    let b = book();
    let vo = b.lookup("level_213_tutotial", 11);
    assert_eq!(vo.playable().map(|p| p.file), Some("audio/sounds/v.mp3"));
    let coin = b.lookup("level_213_tutotial", 12);
    assert_eq!(coin.playable().map(|p| p.file), Some("audio/sounds/c.mp3"));
    assert_eq!(
        b.lookup("global", 3).playable().map(|p| p.file),
        Some("audio/sounds/c.mp3")
    );
}

#[test]
fn a_sound_that_plays_nothing_says_why() {
    let b = book();
    let why = |o| match b.lookup("level_213_tutotial", o) {
        Lookup::Silent { why, .. } => Some(why),
        _ => None,
    };
    assert_eq!(why(13), Some(Silence::NoData));
    assert_eq!(why(14), Some(Silence::Unresolved));
    assert_eq!(why(15), Some(Silence::NoAudio));
    assert_eq!(why(16), Some(Silence::DataMissing));
    // A global ref with no global archive loaded.
    let alone = SoundBook::new([b.file("level_213_tutotial").unwrap().clone()]);
    assert!(matches!(
        alone.lookup("level_213_tutotial", 12),
        Lookup::Silent {
            why: Silence::DataMissing,
            ..
        }
    ));
}

#[test]
fn sounds_are_found_by_name_case_ignored() {
    let b = book();
    assert_eq!(
        b.named("snd_coin"),
        [
            SoundId {
                stem: "global".into(),
                object: 3
            },
            SoundId {
                stem: "level_213_tutotial".into(),
                object: 12
            },
        ]
    );
    assert_eq!(b.named("MU_213_EXPLORE").len(), 1);
    assert!(b.named("nothing").is_empty());
    let id = &b.named("vo_eon")[0];
    assert!(b.lookup_id(id).playable().is_some());
}

#[test]
fn a_levels_music_is_its_music_category() {
    let b = book();
    let names: Vec<_> = b
        .music("level_213_tutotial")
        .map(|s| s.name.as_deref().unwrap())
        .collect();
    assert_eq!(names, ["mu_213_explore"]);
    assert_eq!(b.music("global").count(), 0);
    assert_eq!(b.music("nowhere").count(), 0);
}

#[test]
fn the_settings_read_as_the_disc_stores_them() {
    let b = book();
    let p = b
        .lookup("level_213_tutotial", 10)
        .playable()
        .unwrap()
        .params;
    assert!(p.looping && p.three_d && !p.allow_restart && !p.ignores_time_scales);
    assert!(!p.from_controller && !p.filter_attenuates);
    assert!(p.pitch_is_negative);
    assert_eq!(
        p.falloff,
        Some(Falloff {
            full: 10.0,
            silent: 110.0
        })
    );
    assert_eq!(
        (p.category, p.category_name(), p.priority),
        (MUSIC, "Music", 50)
    );
    assert_eq!(
        p.actor_limit,
        Limit {
            count: 1,
            behavior: LimitBehavior::StealFurthest
        }
    );
    assert_eq!(
        p.global_limit,
        Limit {
            count: 3,
            behavior: LimitBehavior::StealOldest
        }
    );
    assert_eq!(
        (
            p.volume_percent,
            p.volume_randomness_percent,
            p.pitch_randomness_percent,
            p.pan_percent
        ),
        (80.0, 10.0, 20.0, -30.0)
    );
    assert_eq!(
        (p.rumble_duration, p.playback_mode, p.playback_percent),
        (1.5, 1, 75.0)
    );
    assert_eq!(
        (
            p.reverb_amount,
            p.reverb_randomness_percent,
            p.sample_rate_reduction
        ),
        (50.0, 5.0, 60.0)
    );
    assert_eq!(
        (
            p.filter_percent,
            p.filter_randomness_percent,
            p.filter_attenuation_distance_minimum,
            p.filter_attenuation_distance_maximum,
            p.filter_maximum_percent
        ),
        (40.0, 4.0, 2.0, 200.0, 90.0)
    );
    // Allow restart, from controller, ignores time scales; no falloff.
    let vo = b
        .lookup("level_213_tutotial", 11)
        .playable()
        .unwrap()
        .params;
    assert!(vo.allow_restart && vo.from_controller && vo.ignores_time_scales);
    assert!(!vo.looping && !vo.three_d && !vo.pitch_is_negative);
    assert_eq!(vo.falloff, None);
    assert_eq!(vo.category_name(), "Effects/Enemy/Combat");
    assert_eq!(vo.actor_limit.behavior, LimitBehavior::Fail);
    assert!(!vo.filter_attenuates);
    let coin = b.lookup("global", 3).playable().unwrap().params;
    assert!(coin.filter_attenuates && coin.falloff.is_some());
    assert!(!coin.looping && !coin.three_d && !coin.allow_restart);
    assert_eq!(LIMIT_BEHAVIORS[3], "steal oldest");
}

#[test]
fn a_play_strays_within_its_randomness() {
    let b = book();
    let vo = b
        .lookup("level_213_tutotial", 11)
        .playable()
        .unwrap()
        .params;
    // 80 % with 10 % randomness: 0.72 to 0.88.
    assert!((vo.volume_for(0.0) - 0.8).abs() < 1e-6);
    assert!((vo.volume_for(-1.0) - 0.72).abs() < 1e-6);
    assert!((vo.volume_for(1.0) - 0.88).abs() < 1e-6);
    assert!(
        (vo.volume_for(5.0) - 0.88).abs() < 1e-6,
        "a draw past 1 is 1"
    );
    // 20 % pitch randomness either way.
    assert!((vo.speed_for(-1.0) - 0.8).abs() < 1e-6);
    assert!((vo.speed_for(0.5) - 1.1).abs() < 1e-6);
    // Pitch-negative strays only down.
    let track = b
        .lookup("level_213_tutotial", 10)
        .playable()
        .unwrap()
        .params;
    assert!((track.speed_for(0.5) - 0.9).abs() < 1e-6);
    assert!((track.speed_for(-0.5) - 0.9).abs() < 1e-6);
    // Never below silence or a stop.
    let mut wild = vo;
    wild.volume_randomness_percent = 300.0;
    wild.pitch_randomness_percent = 300.0;
    assert_eq!(wild.volume_for(-1.0), 0.0);
    assert_eq!(wild.speed_for(-1.0), 0.0);
}

#[test]
fn a_sound_that_attenuates_falls_off_between_its_distances() {
    let b = book();
    let track = b
        .lookup("level_213_tutotial", 10)
        .playable()
        .unwrap()
        .params;
    assert_eq!(track.gain_at(0.0), 1.0);
    assert_eq!(track.gain_at(10.0), 1.0);
    assert!((track.gain_at(60.0) - 0.5).abs() < 1e-6);
    assert!((track.gain_at(85.0) - 0.25).abs() < 1e-6);
    assert_eq!(track.gain_at(110.0), 0.0);
    assert_eq!(track.gain_at(1e6), 0.0);
    // One that doesn't plays the same anywhere.
    let vo = b
        .lookup("level_213_tutotial", 11)
        .playable()
        .unwrap()
        .params;
    assert_eq!(vo.gain_at(1e6), 1.0);
}
