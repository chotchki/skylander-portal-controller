//! A sound and its data (openspec AR.3): `tfbSoundInfo`, what a script
//! plays, and `tfbSoundDataInfo`, the bank it plays.
//!
//! `tfbSoundInfo` is a sequence, as a clip's `AnimationInfo` is: it sits on
//! `AbstractSoundSequence` → `ClonedSequence` → `Sequence`, so its switches
//! are bits of `Sequence._flags`. Every offset is AC.4's registry's; the
//! `_properties` bitfield's shifts and widths are the field function's
//! (each bitfield meta field's `_shift` and `_bits`), and which `_flags` bit
//! each switch is comes from the getter the EBOOT registers for its
//! `soundinfo` or `sequence` member (aj-vm's live member table), each read
//! as an aid in [`ar-audio/notes/`] and written here in our own words.
//! `_categoryInfo` and `_interface` have no storage (offset 0 in the
//! registry), and the runtime refs (`_tfbSoundObject`, `_tfbSoundRuntime`)
//! are null on disc.
//!
//! [`ar-audio/notes/`]: /Users/chotchki/workspace/guacamole-research/ar-audio/notes/

use super::{BuildDataInfo, Layout, ObjectRef, View};
use crate::FormatError;

/// `soundinfo.category`'s names, by value, as the EBOOT spells them.
pub const CATEGORIES: [&str; 32] = [
    "Effects/",
    "Effects/Champion/",
    "Effects/Champion/Combat",
    "Effects/Champion/Movement",
    "Effects/Enemy/",
    "Effects/Enemy/Combat",
    "Effects/Enemy/Movement",
    "Effects/Level/",
    "Effects/Level/Object",
    "Effects/Level/Cutscene",
    "Effects/Level/Ambience",
    "Effects/Level/Environment",
    "Voice/",
    "Voice/Champion/",
    "Voice/Champion/Emote",
    "Voice/Champion/Banter",
    "Voice/Enemy/",
    "Voice/Enemy/Emote",
    "Voice/Enemy/Banter",
    "Voice/NPC/",
    "Voice/NPC/Emote",
    "Voice/NPC/Instruction",
    "Voice/Announcer/",
    "Voice/Announcer/Banter",
    "Voice/Announcer/Instruction",
    "Effects/UI/",
    "Effects/UI/Interface",
    "Effects/UI/Feedback",
    "Music",
    "Video",
    "Other",
    "Voice/Champion/Portal",
];

/// What an actor or global limit does once it's full, by value:
/// `soundinfo.actor limit behavior` and `.global limit behavior`'s names.
pub const LIMIT_BEHAVIORS: [&str; 4] = [
    "just fail",
    "steal quietest",
    "steal furthest",
    "steal oldest",
];

/// `Sequence._flags`' bits a sound's members read.
pub mod flag {
    /// `sequence.looping`.
    pub const LOOPING: u32 = 1 << 0;
    /// `sequence.allow restart`.
    pub const ALLOW_RESTART: u32 = 1 << 2;
    /// `soundinfo.attenuates`: it falls off with distance.
    pub const ATTENUATES: u32 = 1 << 3;
    /// `soundinfo.3D` (read only).
    pub const THREE_D: u32 = 1 << 5;
    /// `soundinfo.filter attenuates`.
    pub const FILTER_ATTENUATES: u32 = 1 << 6;
    /// `soundinfo.from controller`.
    pub const FROM_CONTROLLER: u32 = 1 << 8;
    /// `soundinfo.ignores time scales`.
    pub const IGNORES_TIME_SCALES: u32 = 1 << 24;
}

/// `tfbSoundDataInfo`: size 0x28, a build-data block (the embedded bank, or
/// none and a stream's hash) plus the sound's length and its lip sync.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SoundDataInfo<'a> {
    pub data_info: BuildDataInfo<'a>,
    /// Seconds, as the build measured the recording.
    pub raw_duration: f32,
    /// A voice line's lip-sync stream; null for everything else.
    pub viseme_stream: ObjectRef<'a>,
}

impl SoundDataInfo<'_> {
    pub const RAW_DURATION: u32 = 0x20;
    pub const VISEME_STREAM: u32 = 0x24;
}

impl<'a> Layout<'a> for SoundDataInfo<'a> {
    const TYPE: &'static str = "tfbSoundDataInfo";

    fn fields(v: &mut View<'_, 'a>) -> Result<Self, FormatError> {
        Ok(Self {
            data_info: BuildDataInfo::fields(v)?,
            raw_duration: v.f32(Self::RAW_DURATION, "_rawDuration")?,
            viseme_stream: v.object_ref(Self::VISEME_STREAM, "_visemeStream")?,
        })
    }
}

/// `tfbSoundInfo`: size 0x80, what a script plays. The percent fields are
/// 0-100 as stored; the distances are feet, as `soundinfo`'s members give
/// their units.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SoundInfo<'a> {
    pub name: Option<&'a str>,
    /// `Sequence._flags`, raw; [`flag`] names the bits read.
    pub flags: u32,
    /// `Sequence._playbackMode`: 0 one way, 1 ping pong.
    pub playback_mode: u32,
    pub playback_percent: f32,
    pub activator: ObjectRef<'a>,
    pub clones: ObjectRef<'a>,
    pub parent: ObjectRef<'a>,
    pub volume_percent: f32,
    pub volume_randomness_percent: f32,
    pub pitch_randomness_percent: f32,
    pub pan_percent: f32,
    pub attenuation_distance_full: f32,
    pub attenuation_distance_silent: f32,
    pub rumble_duration: f32,
    /// The `tfbSoundDataInfo`: local, a `global.bld` import, or null.
    pub data: ObjectRef<'a>,
    pub sound_object: ObjectRef<'a>,
    /// The bitfield, raw; the accessors below read it.
    pub properties: u32,
    pub reverb_amount: f32,
    pub reverb_randomness_percent: f32,
    pub filter_percent: f32,
    pub filter_randomness_percent: f32,
    pub filter_attenuation_distance_minimum: f32,
    pub filter_attenuation_distance_maximum: f32,
    pub filter_maximum_percent: f32,
    pub sample_rate_reduction: f32,
    pub sound_runtime: ObjectRef<'a>,
    pub tag_list: ObjectRef<'a>,
}

impl SoundInfo<'_> {
    pub const NAME: u32 = 0x08;
    pub const FLAGS: u32 = 0x18;
    pub const PLAYBACK_MODE: u32 = 0x1C;
    pub const PLAYBACK_PERCENT: u32 = 0x20;
    pub const ACTIVATOR: u32 = 0x24;
    pub const CLONES: u32 = 0x28;
    pub const PARENT: u32 = 0x2C;
    pub const VOLUME_PERCENT: u32 = 0x30;
    pub const VOLUME_RANDOMNESS_PERCENT: u32 = 0x34;
    pub const PITCH_RANDOMNESS_PERCENT: u32 = 0x38;
    pub const PAN_PERCENT: u32 = 0x3C;
    pub const ATTENUATION_DISTANCE_FULL: u32 = 0x40;
    pub const ATTENUATION_DISTANCE_SILENT: u32 = 0x44;
    pub const RUMBLE_DURATION: u32 = 0x48;
    pub const DATA: u32 = 0x4C;
    pub const SOUND_OBJECT: u32 = 0x50;
    pub const PROPERTIES: u32 = 0x54;
    pub const REVERB_AMOUNT: u32 = 0x58;
    pub const REVERB_RANDOMNESS_PERCENT: u32 = 0x5C;
    pub const FILTER_PERCENT: u32 = 0x60;
    pub const FILTER_RANDOMNESS_PERCENT: u32 = 0x64;
    pub const FILTER_ATTENUATION_DISTANCE_MINIMUM: u32 = 0x68;
    pub const FILTER_ATTENUATION_DISTANCE_MAXIMUM: u32 = 0x6C;
    pub const FILTER_MAXIMUM_PERCENT: u32 = 0x70;
    pub const SAMPLE_RATE_REDUCTION: u32 = 0x74;
    pub const SOUND_RUNTIME: u32 = 0x78;
    pub const TAG_LIST: u32 = 0x7C;

    pub fn looping(&self) -> bool {
        self.flags & flag::LOOPING != 0
    }

    pub fn allow_restart(&self) -> bool {
        self.flags & flag::ALLOW_RESTART != 0
    }

    pub fn attenuates(&self) -> bool {
        self.flags & flag::ATTENUATES != 0
    }

    pub fn three_d(&self) -> bool {
        self.flags & flag::THREE_D != 0
    }

    pub fn filter_attenuates(&self) -> bool {
        self.flags & flag::FILTER_ATTENUATES != 0
    }

    pub fn from_controller(&self) -> bool {
        self.flags & flag::FROM_CONTROLLER != 0
    }

    pub fn ignores_time_scales(&self) -> bool {
        self.flags & flag::IGNORES_TIME_SCALES != 0
    }

    /// `_pitchIsNegative`: bit 0.
    pub fn pitch_is_negative(&self) -> bool {
        self.properties & 1 != 0
    }

    /// `_category`: bits 1-5, an index into [`CATEGORIES`].
    pub fn category(&self) -> u32 {
        (self.properties >> 1) & 0x1F
    }

    /// `_priority`: bits 6-17 (0-100 by its member's range).
    pub fn priority(&self) -> u32 {
        (self.properties >> 6) & 0xFFF
    }

    /// `_actorLimit`: bits 18-22.
    pub fn actor_limit(&self) -> u32 {
        (self.properties >> 18) & 0x1F
    }

    /// `_actorLimitBehavior`: bits 23-24, an index into [`LIMIT_BEHAVIORS`].
    pub fn actor_limit_behavior(&self) -> u32 {
        (self.properties >> 23) & 0x3
    }

    /// `_globalLimit`: bits 25-29.
    pub fn global_limit(&self) -> u32 {
        (self.properties >> 25) & 0x1F
    }

    /// `_globalLimitBehavior`: bits 30-31, an index into [`LIMIT_BEHAVIORS`].
    pub fn global_limit_behavior(&self) -> u32 {
        self.properties >> 30
    }
}

impl<'a> Layout<'a> for SoundInfo<'a> {
    const TYPE: &'static str = "tfbSoundInfo";

    fn fields(v: &mut View<'_, 'a>) -> Result<Self, FormatError> {
        Ok(Self {
            name: v.string(Self::NAME, "_name")?,
            flags: v.u32(Self::FLAGS, "_flags")?,
            playback_mode: v.u32(Self::PLAYBACK_MODE, "_playbackMode")?,
            playback_percent: v.f32(Self::PLAYBACK_PERCENT, "_playbackPercent")?,
            activator: v.object_ref(Self::ACTIVATOR, "_activator")?,
            clones: v.object_ref(Self::CLONES, "_clones")?,
            parent: v.object_ref(Self::PARENT, "_parent")?,
            volume_percent: v.f32(Self::VOLUME_PERCENT, "_volumePercent")?,
            volume_randomness_percent: v
                .f32(Self::VOLUME_RANDOMNESS_PERCENT, "_volumeRandomnessPercent")?,
            pitch_randomness_percent: v
                .f32(Self::PITCH_RANDOMNESS_PERCENT, "_pitchRandomnessPercent")?,
            pan_percent: v.f32(Self::PAN_PERCENT, "_panPercent")?,
            attenuation_distance_full: v
                .f32(Self::ATTENUATION_DISTANCE_FULL, "_attenuationDistanceFull")?,
            attenuation_distance_silent: v.f32(
                Self::ATTENUATION_DISTANCE_SILENT,
                "_attenuationDistanceSilent",
            )?,
            rumble_duration: v.f32(Self::RUMBLE_DURATION, "_rumbleDuration")?,
            data: v.object_ref(Self::DATA, "_tfbSoundDataInfo")?,
            sound_object: v.object_ref(Self::SOUND_OBJECT, "_tfbSoundObject")?,
            properties: v.u32(Self::PROPERTIES, "_properties")?,
            reverb_amount: v.f32(Self::REVERB_AMOUNT, "_reverbAmount")?,
            reverb_randomness_percent: v
                .f32(Self::REVERB_RANDOMNESS_PERCENT, "_reverbRandomnessPercent")?,
            filter_percent: v.f32(Self::FILTER_PERCENT, "_filterPercent")?,
            filter_randomness_percent: v
                .f32(Self::FILTER_RANDOMNESS_PERCENT, "_filterRandomnessPercent")?,
            filter_attenuation_distance_minimum: v.f32(
                Self::FILTER_ATTENUATION_DISTANCE_MINIMUM,
                "_filterAttenuationDistanceMinimum",
            )?,
            filter_attenuation_distance_maximum: v.f32(
                Self::FILTER_ATTENUATION_DISTANCE_MAXIMUM,
                "_filterAttenuationDistanceMaximum",
            )?,
            filter_maximum_percent: v.f32(Self::FILTER_MAXIMUM_PERCENT, "_filterMaximumPercent")?,
            sample_rate_reduction: v.f32(Self::SAMPLE_RATE_REDUCTION, "_sampleRateReduction")?,
            sound_runtime: v.object_ref(Self::SOUND_RUNTIME, "_tfbSoundRuntime")?,
            tag_list: v.object_ref(Self::TAG_LIST, "_tagList")?,
        })
    }
}
