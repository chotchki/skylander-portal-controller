//! The sounds a level plays, off the dump (openspec AR.9-AR.12): the
//! sounds part read in memory with guac-extract's own reader (nothing
//! written), its music's loop seam decoded as the TV decodes it. Ignored by
//! default: reads `GUAC_GIANTS_DUMP` (process env, else `.env.dev`).

mod common;

use std::sync::Arc;

use common::{dump, identity};

/// Level 213's sounds part as the extract would write it: its IR and its
/// audio by path.
fn level_213() -> guac_extract::sounds::Read {
    let usr = dump().join("PS3_GAME/USRDIR");
    let exports = guac_extract::sounds::exports(&usr).expect("global's exports");
    let bld = std::fs::read(usr.join("level/level_213_tutotial.bld")).expect("the level's .bld");
    let arc = std::fs::read(usr.join("level/level_213_tutotial.arc")).expect("the level's .arc");
    guac_extract::sounds::read(
        "level/level_213_tutotial.bld",
        &bld,
        Some(&arc),
        &exports,
        identity(),
    )
    .expect("the level's sounds")
}

/// The remuxed bytes and the data of the sound named `name`.
fn track(read: &guac_extract::sounds::Read, name: &str) -> (Arc<[u8]>, guac_ir::sound::SoundData) {
    let sound = read
        .ir
        .sounds
        .iter()
        .find(|s| s.name.as_deref() == Some(name))
        .unwrap_or_else(|| panic!("no {name}"));
    let guac_ir::sound::DataRef::Here(home, object) = sound.data else {
        panic!("{name}'s data isn't the level's");
    };
    let data = read.ir.data_at(home, object).expect("its data").clone();
    let file = &data.audio.as_ref().expect("its audio").file;
    let bytes = &read
        .audio
        .iter()
        .find(|(p, _)| p == file)
        .expect("its bytes")
        .1;
    (Arc::from(bytes.as_slice()), data)
}

/// Every sample rodio's decoder makes of `bytes`, interleaved, and the
/// channel count.
fn decode(bytes: &Arc<[u8]>, gapless: bool) -> (Vec<f32>, u16) {
    use rodio::Source;
    let d = rodio::Decoder::builder()
        .with_byte_len(bytes.len() as u64)
        .with_data(std::io::Cursor::new(bytes.clone()))
        .with_gapless(gapless)
        .build()
        .expect("it decodes");
    let channels = d.channels().get();
    (d.map(|s| s as f32).collect(), channels)
}

/// The step across each looping track's seam against its music's own 99th
/// percentile step (openspec AR.11): the TV's loop (the frames decoded on
/// through the wrap, `guacamole::voice::Cycle`) under one, its second pass
/// the first's again once the decoder has settled, a fresh decode's codec
/// silence music on the second pass; a loop of the decoded samples (what a
/// plain decoder's repeat makes) printed beside it.
#[test]
#[ignore = "reads GUAC_GIANTS_DUMP; run on the Mac"]
fn a_looping_bed_loops_its_frames_on_through_the_seam() {
    use guacamole::voice::{Gains, VoiceDecoder};
    let read = level_213();
    let looping: Vec<&str> = read
        .ir
        .sounds
        .iter()
        .filter(|s| s.flags & 1 != 0 && s.properties >> 1 & 0x1F == guac_sound::MUSIC)
        .filter_map(|s| s.name.as_deref())
        .collect();
    assert_eq!(
        looping,
        [
            "mu_213_combat_shorter",
            "mu_213_explore",
            "mu_213_inside",
            "mu_213_inside_combat",
            "mu_213_intro"
        ],
        "chapter 1's looping tracks"
    );
    for name in looping {
        let (bytes, data) = track(&read, name);
        let n = data.audio.as_ref().unwrap().samples as usize;
        let (once, ch) = decode(&bytes, false);
        let ch = ch as usize;
        assert_eq!(once.len(), n * ch, "{name}: every frame decoded");
        // The TV's loop, two passes and a bit, left channel.
        let tv: Vec<f32> = VoiceDecoder::new(bytes.clone(), Gains::new(1.0, 1.0), true)
            .step_by(2)
            .take(2 * n + 8192)
            .map(|s| s as f32)
            .collect();
        assert_eq!(tv.len(), 2 * n + 8192, "{name}: the loop plays on");
        let left: Vec<f32> = once.iter().step_by(ch).copied().collect();
        let mut steps: Vec<f32> = left.windows(2).map(|w| (w[1] - w[0]).abs()).collect();
        steps.sort_by(f32::total_cmp);
        let p99 = steps[steps.len() * 99 / 100];
        let join = (n - 2..n + 3)
            .map(|i| (tv[i] - tv[i - 1]).abs())
            .fold(0.0_f32, f32::max);
        let plain = (left[n - 1] - left[0]).abs();
        let silent = left.iter().take_while(|v| v.abs() <= 1e-4).count();
        let resettled = (4096..8192).all(|i| tv[i] == tv[n + i] && tv[n + i] == tv[2 * n + i - n]);
        println!(
            "{name}: {n} frames; the TV's loop steps {:.2} x p99 ({p99:.4}) across the seam; a loop of the samples steps {:.2} x and then holds {silent} frames under 1e-4 ({:.1} ms); second pass the first's again from frame 4096: {resettled}",
            join / p99,
            plain / p99,
            silent as f32 * 1e3 / data.audio.as_ref().unwrap().rate as f32
        );
        assert!(join < p99, "{name}: the seam steps {join} against {p99}");
        assert!(resettled, "{name}: the second pass isn't the first's again");
        // A fresh decode's codec silence (under 1,000 frames: the intro's
        // 1.27 s is a rest in its music) is music on the second pass.
        assert!(
            silent > 1000 || tv[n..n + silent].iter().any(|v| v.abs() > 1e-4),
            "{name}: the second pass starts silent"
        );
    }
}
