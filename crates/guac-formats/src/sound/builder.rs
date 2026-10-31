//! Synthetic sounds for tests and fuzz seeds (`test-fixtures`): six sound
//! infos (data local, imported from `global.bld`, null, a local object that
//! isn't a data info, an import from elsewhere, one from the language pack)
//! and four data infos (an embedded bank, an English stream, a Swedish one
//! with a lip-sync ref, one with neither data nor name), and a language
//! pack ([`pack`]). Every field holds a value of its own,
//! written at the offsets AR.3's layouts give as literals, so a wrong reader
//! constant shows.

use crate::fsb::builder::{BankSpec, SampleSpec, write};
use crate::igz::Location;
use crate::igz::builder::IgzBuilder;
use crate::mpeg::builder::stream;

/// Where [`sample`] put each object; indexes follow creation order.
#[derive(Debug, Clone)]
pub struct Sample {
    pub bytes: Vec<u8>,
    /// Data infos: embedded, English stream, Swedish stream, empty.
    pub data: [Location; 4],
    /// Sound infos: local, global, null, a sound info as data, elsewhere,
    /// the pack.
    pub sounds: [Location; 6],
    /// The FSB5 bank the first data info holds, as written.
    pub bank: Vec<u8>,
}

/// The export id the global sound imports.
pub const GLOBAL_ID: u32 = 0x21;
/// The pack position the last sound imports: the pack's second data info.
pub const PACK_ID: u32 = 3;
pub const EMBEDDED_NAME: &str = "Levels/_Enemies/Grunt/sounds/fx_grunt_hit.wav";
pub const ENGLISH_NAME: &str = "Levels/Level_213/sounds/mu_213_Explore.wav";
pub const SWEDISH_NAME: &str = "Levels/npc_audio/eon/swedish/vo_eon_lvl213_014.wav";
pub const STREAM_HASH: u32 = 0x9D5D_28AB;

fn at(base: Location, offset: u32) -> Location {
    base.offset_by(offset).expect("inside the section")
}

fn float(b: &mut IgzBuilder, base: Location, offset: u32, value: f32) {
    b.put_bytes(at(base, offset), &value.to_be_bytes());
}

/// A sound info's scalars, each `seed` plus its offset, so no two fields
/// (and no two sounds) hold the same value.
fn sound_fields(b: &mut IgzBuilder, s: Location, seed: f32) {
    for offset in (0x30..=0x48).step_by(4).chain((0x58..=0x74).step_by(4)) {
        float(b, s, offset, seed + offset as f32);
    }
}

pub fn sample() -> Sample {
    let mut b = IgzBuilder::new();
    let blocks = b.section(0x10);
    let data = [
        b.object(0, "tfbSoundDataInfo", 0x28),
        b.object(0, "tfbSoundDataInfo", 0x28),
        b.object(0, "tfbSoundDataInfo", 0x28),
        b.object(0, "tfbSoundDataInfo", 0x28),
    ];
    let sounds = [
        b.object(0, "tfbSoundInfo", 0x80),
        b.object(0, "tfbSoundInfo", 0x80),
        b.object(0, "tfbSoundInfo", 0x80),
        b.object(0, "tfbSoundInfo", 0x80),
        b.object(0, "tfbSoundInfo", 0x80),
        b.object(0, "tfbSoundInfo", 0x80),
    ];
    let bank = write(&BankSpec::new(vec![
        SampleSpec::new(stream(3, 3), 3 * 1152).named("0x06838641.wav.hz"),
    ]))
    .bytes;

    // The embedded bank's data info.
    b.string(at(data[0], 0x08), EMBEDDED_NAME);
    b.memory_ref(at(data[0], 0x14), blocks, &bank, 2);
    float(&mut b, data[0], 0x20, 1.25);
    // The English stream's: no data, a hash, a duration.
    b.string(at(data[1], 0x08), ENGLISH_NAME);
    b.null_memory_ref(at(data[1], 0x14), 0);
    b.put_u32(at(data[1], 0x1C), STREAM_HASH);
    float(&mut b, data[1], 0x20, 129.103);
    // The Swedish line's, with its lip sync.
    b.string(at(data[2], 0x08), SWEDISH_NAME);
    b.null_memory_ref(at(data[2], 0x14), 0);
    b.put_u32(at(data[2], 0x1C), 0x0102_0304);
    float(&mut b, data[2], 0x20, 2.75);
    b.pointer(at(data[2], 0x24), data[0]);
    // Neither data nor a name.
    b.null_memory_ref(at(data[3], 0x14), 0);

    let names = [
        "snd_grunt_hit",
        "snd_global",
        "snd_quiet",
        "snd_odd",
        "snd_far",
    ];
    for (i, (&s, name)) in sounds.iter().zip(names).enumerate() {
        b.string(at(s, 0x08), name);
        sound_fields(&mut b, s, 1000.0 * i as f32);
    }
    // Looping, attenuates, 3D, from controller and the bit every disc sound
    // carries; playback ping pong at 75 %.
    b.put_u32(at(sounds[0], 0x18), 0x4000_0129);
    b.put_u32(at(sounds[0], 0x1C), 1);
    float(&mut b, sounds[0], 0x20, 75.0);
    // Pitch negative, category 5, priority 51, actor limit 1 (steal
    // oldest), global limit 3 (steal oldest): each field's lowest bit set
    // where its neighbour below would read it if a width grew.
    b.put_u32(
        at(sounds[0], 0x54),
        1 | 5 << 1 | 51 << 6 | 1 << 18 | 3 << 23 | 3 << 25 | 3 << 30,
    );
    b.pointer(at(sounds[0], 0x4C), data[0]);
    // Allow restart, filter attenuates, ignores time scales; every bitfield
    // at its widest.
    b.put_u32(at(sounds[1], 0x18), 0x0100_0044);
    b.put_u32(at(sounds[1], 0x54), u32::MAX);
    b.import_raw(
        at(sounds[1], 0x4C),
        GLOBAL_ID,
        crate::fnv1a32(crate::exports::NAMESPACE.as_bytes()),
    );
    b.pointer(at(sounds[2], 0x7C), data[3]);
    b.pointer(at(sounds[3], 0x4C), sounds[0]);
    b.import(at(sounds[4], 0x4C), "global.arc", "snd_far_data");
    b.import_raw(
        at(sounds[5], 0x4C),
        PACK_ID,
        crate::fnv1a32(super::PACK_NAMESPACE.as_bytes()),
    );
    Sample {
        bytes: b.build(),
        data,
        sounds,
        bank,
    }
}

/// A language pack in `language` (`ENGLISH`): its root list a
/// `LanguagePackInfo` and two streamed data infos (the second named
/// [`SWEDISH_NAME`] without its folder, as every pack names it), then a
/// sound info, which no id may land on.
pub fn pack(language: &str) -> Vec<u8> {
    use crate::igz::layouts::builder::list;
    let mut b = IgzBuilder::new();
    let blocks = b.section(0x10);
    let root = b.object(0, "igObjectList", 0x18);
    let info = b.object(0, "LanguagePackInfo", 0x20);
    b.string(at(info, 0x08), language);
    b.null_memory_ref(at(info, 0x18), 0);
    let data = [
        b.object(0, "tfbSoundDataInfo", 0x28),
        b.object(0, "tfbSoundDataInfo", 0x28),
    ];
    for (d, name) in data.iter().zip([
        "Levels/npc_audio/eon/vo_eon_lvl213_001.wav",
        &SWEDISH_NAME.replace("swedish/", ""),
    ]) {
        b.string(at(*d, 0x08), name);
        b.null_memory_ref(at(*d, 0x14), 0);
    }
    let sound = b.object(0, "tfbSoundInfo", 0x80);
    list(
        &mut b,
        root,
        blocks,
        &[Some(info), Some(data[0]), Some(data[1]), Some(sound)],
    );
    b.build()
}

/// The property test's and the fuzz targets' starting points: the sample,
/// a pack, and a file of one embedded data info alone.
pub fn seeds() -> Vec<Vec<u8>> {
    let mut b = IgzBuilder::new();
    let blocks = b.section(0x10);
    let d = b.object(0, "tfbSoundDataInfo", 0x28);
    let bank = write(&BankSpec::new(vec![SampleSpec::new(stream(1, 0), 1152)])).bytes;
    b.memory_ref(at(d, 0x14), blocks, &bank, 2);
    vec![sample().bytes, pack("ENGLISH"), b.build()]
}
