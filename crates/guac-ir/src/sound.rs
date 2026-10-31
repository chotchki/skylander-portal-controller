//! An archive's sounds (openspec AR.5): every `tfbSoundInfo` its
//! `level.bld` holds and every `tfbSoundDataInfo` its `level.bld` and its
//! English pack hold, each data info with the audio it plays. A new record
//! in files of its own, `sounds/<stem>.<ron|postcard>`, the audio beside it
//! as `audio/sounds/<sha256 prefix>.mp3`, so [`crate::IR_VERSION`] holds.
//!
//! Numbers are kept as the disc stores them: percentages as 0-100,
//! distances in feet, durations in seconds, the flag and property words
//! raw (guac-formats' sound layouts name their bits, and `guac-sound` says
//! what each means in play).

use serde::{Deserialize, Serialize};

use crate::DumpIdentity;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Sounds {
    pub identity: DumpIdentity,
    /// The archive's path under `PS3_GAME/USRDIR` (`level/level_213_tutotial.bld`).
    pub archive: String,
    /// Each sound info of its `level.bld` that reads, in object order.
    pub sounds: Vec<Sound>,
    /// Each data info of its `level.bld` and English pack that reads,
    /// `level.bld`'s first, each home in object order.
    pub data: Vec<SoundData>,
    /// What didn't read or whose audio didn't, by name or object, with why.
    pub refused: Vec<(String, String)>,
}

/// One `tfbSoundInfo`: what a script names and plays.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Sound {
    /// Its object in the archive's `level.bld`: what a script's reference
    /// to it holds.
    pub object: u32,
    pub name: Option<String>,
    pub data: DataRef,
    /// `Sequence._flags`, raw.
    pub flags: u32,
    /// `_properties`, raw: category, priority and limits.
    pub properties: u32,
    /// 0 one way, 1 ping pong.
    pub playback_mode: u32,
    pub playback_percent: f32,
    pub volume_percent: f32,
    pub volume_randomness_percent: f32,
    pub pitch_randomness_percent: f32,
    pub pan_percent: f32,
    pub attenuation_distance_full: f32,
    pub attenuation_distance_silent: f32,
    pub rumble_duration: f32,
    pub reverb_amount: f32,
    pub reverb_randomness_percent: f32,
    pub filter_percent: f32,
    pub filter_randomness_percent: f32,
    pub filter_attenuation_distance_minimum: f32,
    pub filter_attenuation_distance_maximum: f32,
    pub filter_maximum_percent: f32,
    pub sample_rate_reduction: f32,
}

/// Which data info a sound plays, resolved when the part was written.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum DataRef {
    /// None: the sound plays nothing until a script gives it data.
    None,
    /// A data info of this archive: in its `level.bld` or its pack.
    Here(DataHome, u32),
    /// A data info of `global.bld`, by object there.
    Global(u32),
    /// A ref the part couldn't follow (an import from elsewhere, an id that
    /// lands on nothing); the sound plays nothing.
    Unresolved,
}

/// Which IGZ of an archive a data info sits in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub enum DataHome {
    /// `level.bld`: sound effects, music, ambience, a level's own voices.
    Level,
    /// `ENGLISH.pak`: the voices a sound reaches through the language pack.
    Pack,
}

/// One `tfbSoundDataInfo` and the audio it plays.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SoundData {
    pub home: DataHome,
    /// Its object in that IGZ.
    pub object: u32,
    pub name: Option<String>,
    /// Seconds, as the build measured the recording.
    pub raw_duration: f32,
    /// Streamed from the archive's `.arc` (with the hash the disc keeps), or
    /// embedded in the IGZ.
    pub streamed: Option<i32>,
    /// A voice line's lip-sync stream rides along on the disc; not kept.
    pub lip_sync: bool,
    /// The audio, or `None` when its bank didn't read (listed in
    /// [`Sounds::refused`]).
    pub audio: Option<Audio>,
}

/// A bank's one sample, remuxed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Audio {
    /// `audio/sounds/<sha256 prefix>.mp3`, relative to the extract root.
    pub file: String,
    pub rate: u32,
    pub channels: u32,
    /// Length in samples per channel, the bank's count.
    pub samples: u32,
    /// The bank's loop chunk, in samples, when it has one.
    pub loop_points: Option<(u32, u32)>,
}

impl Sounds {
    /// The sound at `object` in this archive's `level.bld`, by a search
    /// that leans on [`Sounds::sounds`]' object order.
    pub fn sound(&self, object: u32) -> Option<&Sound> {
        self.sounds
            .binary_search_by_key(&object, |s| s.object)
            .ok()
            .map(|i| &self.sounds[i])
    }

    /// The data info at `object` in `home`, `None` for one that didn't read;
    /// the search leans on [`Sounds::data`]'s order.
    pub fn data_at(&self, home: DataHome, object: u32) -> Option<&SoundData> {
        self.data
            .binary_search_by_key(&(home, object), |d| (d.home, d.object))
            .ok()
            .map(|i| &self.data[i])
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{from_postcard, from_ron, to_postcard, to_ron};

    fn sample() -> Sounds {
        let identity = DumpIdentity {
            title_id: "BLUS30968".into(),
            app_ver: "01.00".into(),
            manifest_hash: crate::Sha256([7; 32]),
        };
        let audio = Audio {
            file: "audio/sounds/0011223344556677.mp3".into(),
            rate: 44_100,
            channels: 2,
            samples: 1152 * 3,
            loop_points: Some((10, 3000)),
        };
        let sound = |object, data| Sound {
            object,
            name: Some(format!("snd_{object}")),
            data,
            flags: 0x4000_0029,
            properties: 0x0604_0C8A,
            playback_mode: 0,
            playback_percent: 100.0,
            volume_percent: 45.0,
            volume_randomness_percent: 5.0,
            pitch_randomness_percent: 10.0,
            pan_percent: 0.0,
            attenuation_distance_full: 1.0,
            attenuation_distance_silent: 150.0,
            rumble_duration: 0.44,
            reverb_amount: 50.0,
            reverb_randomness_percent: 0.0,
            filter_percent: 0.0,
            filter_randomness_percent: 0.0,
            filter_attenuation_distance_minimum: 1.0,
            filter_attenuation_distance_maximum: 200.0,
            filter_maximum_percent: 100.0,
            sample_rate_reduction: 60.0,
        };
        Sounds {
            identity,
            archive: "level/level_213_tutotial.bld".into(),
            sounds: vec![
                sound(4, DataRef::Here(DataHome::Level, 9)),
                sound(7, DataRef::Here(DataHome::Pack, 2)),
                sound(8, DataRef::Global(120)),
                sound(11, DataRef::None),
                sound(12, DataRef::Unresolved),
            ],
            data: vec![
                SoundData {
                    home: DataHome::Level,
                    object: 9,
                    name: Some("Levels/x.wav".into()),
                    raw_duration: 0.44,
                    streamed: None,
                    lip_sync: false,
                    audio: Some(audio.clone()),
                },
                SoundData {
                    home: DataHome::Pack,
                    object: 2,
                    name: Some("Levels/npc_audio/eon/vo.wav".into()),
                    raw_duration: 3.1,
                    streamed: Some(-5),
                    lip_sync: true,
                    audio: None,
                },
            ],
            refused: vec![("Levels/npc_audio/eon/vo.wav".into(), "no entry".into())],
        }
    }

    #[test]
    fn sounds_round_trip_in_both_formats() {
        let s = sample();
        assert_eq!(from_ron::<Sounds>(&to_ron(&s).unwrap()).unwrap(), s);
        assert_eq!(
            from_postcard::<Sounds>(&to_postcard(&s).unwrap()).unwrap(),
            s
        );
    }

    #[test]
    fn a_sound_and_a_data_info_are_found_by_object() {
        let s = sample();
        assert_eq!(
            s.sound(7).map(|x| x.data),
            Some(DataRef::Here(DataHome::Pack, 2))
        );
        assert_eq!(s.sound(5), None);
        assert_eq!(
            s.data_at(DataHome::Level, 9)
                .and_then(|d| d.audio.as_ref())
                .map(|a| a.samples),
            Some(3456)
        );
        assert!(
            s.data_at(DataHome::Pack, 2)
                .is_some_and(|d| d.audio.is_none())
        );
        assert_eq!(s.data_at(DataHome::Pack, 9), None, "a home of its own");
        assert_eq!(s.data_at(DataHome::Level, 2), None);
    }
}
