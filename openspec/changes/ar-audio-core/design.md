# Design

## Context

AR stands on `2.0` at `735d9d3c`. What it builds on:

- **The archives** (AB.3): `guac_formats::iga` reads every `.arc` and `.bld` on the disc. A `.bld` holds LZMA-packed IGZ (`level.bld` and kin), a `.arc` stored FSB5 banks and, in the character archives, the upgrade screens' texture IGZ. AB.5's census counted 79,509 FSB5 entries and called them voice lines; the design doc's audio line ("FSB5 v1, one sample per bank, MPEG, 44.1 kHz for 97 %") came from that census's sample.
- **The registry** (AC.4, [`ac4/notes/registry.json`](/Users/chotchki/workspace/guacamole-research/ac4/notes/registry.json)): every type's parent, size and fields. `tfbSoundInfo` (0x80) sits on `AbstractSoundSequence` → `ClonedSequence` → `Sequence` → `tfbScriptObject`, beside `AnimationInfo` on `AbstractAnimSequence`; `tfbSoundDataInfo` (0x28) on `tfbBuildDataInfo`, whose `_data` and `_streamArchiveHash` AE's readers already read for clips and bodies ([`tfb.rs`](../../../crates/guac-formats/src/igz/layouts/tfb.rs)). The members' getters and setters are aj-vm's live table ([`members_live_table.tsv`](/Users/chotchki/workspace/guacamole-research/aj-vm/notes/members_live_table.tsv)): 25 `soundinfo` members on each sound, 22 more on the sound system (master volumes, the three music volumes, reverb settings, the active sets), `sequence.*` on every sequence.
- **The extract's parts** (AG.15, AH.22, AH.87): an extract-wide part is a name in `guac_extract::PARTS` and files recorded under it; a start is current only once every part is, so a new part resumes alone on an older extract, and a record in files of its own leaves `IR_VERSION` where it is.
- **The intros' audio** (AB.10): MP3 under `audio/intro/`, played through Bevy audio with its `mp3` feature. Nothing else in guacamole plays sound.
- **The VM** (AH): a script names an object as `Obj { unit, index }`, the linked file and its index in that file ([`program.rs`](../../../crates/guac-script/src/program.rs)); a `global.bld` import resolves through `guac_formats::exports`. `OpStartSequence` plays a clip today; on a sound info it does nothing, and the `soundinfo` members are value cells or absent ([`script_members.rs`](../../../crates/guacamole/src/script_members.rs) `PlayingSounds` answers an empty set).

The first reads ([`ar-audio/`](/Users/chotchki/workspace/guacamole-research/ar-audio/), 2026-10-31, static) are in the proposal's Why. Every decompile read for this change is a reading aid kept there; the requirements and the code's docs are our words.

## Goals / Non-Goals

**Goals:**

- Every bank on the disc opens, and its codec, channels, rate, length and loop points are counted by family, so the decode path is chosen from the whole disc, not a sample.
- Every sound info chapter 1 can play resolves to its recording, and the extract holds that recording in a form Bevy plays, with the sound's parameters as the data holds them.
- The wiring lane gets one call for "what does this script's sound play, and how", with each parameter's meaning written down and what's still a hypothesis marked.

**Non-Goals:**

- Playing anything, and the `soundinfo` members on the host (the wiring boxes).
- Reverb and filter as effects: their numbers are carried, nothing applies them (FMOD's DSP chain isn't what Bevy audio has).
- The other twelve languages' voices (a language setting is a line of its own when one is wanted).
- A whole-disc extract: only chapter 1's files (decision 5).

## Decisions

**1. One FSB5 reader for both homes of a bank.** A bank embedded in an IGZ and a bank stored in a `.arc` are the same FSB5, so `guac_formats::fsb` reads byte slices and knows nothing of where they came from: the header (version 0's 0x40 bytes or version 1's 0x3C, the codec word), each sample's packed header (rate code, channel code, data offset in 32-byte units, sample count) and its chunk chain (channels, rate, loop start and end, the rest kept by type and length), the name table and the data, each sample's range checked against the data block before anything is sliced. A codec it doesn't decode is still read and named, so the census counts it. Alternative: parse only MPEG banks. Rejected: the census has to see what isn't MPEG to say so.

**2. MPEG to MP3 is a remux in our own code; anything else gets a box.** FMOD stores MPEG frames back to back with zero bytes after some of them; a frame's length is in its header, so walking the frames and dropping the padding between them gives a stream every MP3 decoder plays, sample for sample the original's, with no converter. AR.2's census decides whether that covers the disc. If a family holds PCM or PS-ADPCM, decoding it to WAV in our own code is next; a codec ffmpeg reads (ATRAC3) goes through AB's transcode as Bink does; anything else gets a box of its own and plays nothing until it lands. Alternatives: decode everything to WAV or Ogg (5-10 times the size for WAV, a second lossy pass for Ogg, nothing gained over the original's MP3 frames), or ffmpeg for all (makes sounds wait on a tool the user installs, where the design doc already keeps ffmpeg to the intros).

**3. Sounds are sequences; their switches are `Sequence._flags` bits.** The registry puts `tfbSoundInfo` on `ClonedSequence`, and the members' getters say which word and bits each `soundinfo` member reads: the fields at 0x30-0x74 one to one, the bitfield at 0x54 (pitch negative bit 0, category 1-5, priority 6-17, actor limit 18-22, its behaviour 23-24, global limit 25-29, its behaviour 30-31; the field function's shifts and widths agree), and `Sequence._flags` at 0x18 (looping bit 0, allow restart bit 2, attenuates bit 3, 3D bit 5, filter attenuates bit 6, from controller bit 8, ignores time scales bit 24). The reader keeps the words raw and names the bits, so a bit nobody has read (0x40000000 is set on every sound read so far) is carried, not dropped.

**4. A streamed bank is found by its path.** A streamed data info's `_name` (`Levels/Level_213/sounds/mu_213_explore.wav`) is its `.arc` entry's path lower-cased, under `c:/tfb/build/ps3/`, with a language folder before the file name for a localized voice and a `/0x<hash>.wav.hz.wav.enc` leaf. `_streamArchiveHash` names the stream too, but by a function not found yet: not the entry's TOC hash, not the leaf's hex, not FNV-1a, FNV-1, CRC-32, djb2 or one-at-a-time over the path variants tried ([`out/`](/Users/chotchki/workspace/guacamole-research/ar-audio/out/)). The join by path is checked whole-disc (AR.3); the hash is kept in the IR so a later reading can switch to it. The archive is the IGZ's own (a level's `.bld` and `.arc` share a stem, a character's too, `global.bld` and `global.arc`); AR.3 measures whether any stream lives elsewhere.

**5. The part is chapter 1's files, English voices.** `global.bld` and `global.arc`, the 67 modelled characters' `.bld` and `.arc` (the roster's load files, as the model stage takes them) and `level_213_tutotial.bld` and `.arc`: every sound info and data info in each IGZ, and each data info's bank, embedded or streamed (the English entry: no language folder). Unreferenced `.arc` entries aren't taken: `global.arc` holds every level's announcer lines, which no chapter 1 file names. The audio lands as `audio/sounds/<sha256 prefix>.mp3`, content-addressed, so a bank two files share is one file (Giants is more than half duplicates). AR.5 measures the part's size and time.

**6. The IR resolves what it can at extract time.** One file per source IGZ, `sounds/<stem>.<ron|postcard>`: its sound infos by object index (name, the parameters, the raw flag and property words, the data ref) and its data infos by object index (name, duration, the stream's hash and path, the audio file, the sample's rate, channels, length and loop). A data ref that's a `global.bld` import is resolved through `guac_formats::exports` when the part is written, so the IR says "global's data info N", never an export id the reader has to resolve again. New records in files of their own, so `IR_VERSION` stays 6 and `PARTS` gains `sounds` (AH.87's precedent); an older extract resumes for the part alone.

**7. The lookup is a crate of its own, `guac-sound`.** Pure (`#![forbid(unsafe_code)]`, no Bevy, no I/O beyond what the caller hands it), on `guac-ir` alone, beside `guac-motion` and `guac-script`: the wiring lane's host asks it, and its tests run without a TV. It takes the sound parts the TV loads and answers, for a sound info named by file stem and object index (what a script's `Obj` maps to) or by name, the playable file and its parameters: volume, its randomness, pitch randomness, pan, whether and from where to where it falls off, looping, allow restart, category, priority, limits, reverb and filter carried, the data's duration and the sample's facts. Its docs say what each means, from how the disc's scripts use the members, each hypothesis marked as one: the oracle runs silent, so no number here is heard against the original (the stand-in ledger gets the rows). Alternative: the lookup in `guac-ir`. Rejected: guac-ir is the records; the meanings and the cross-file resolution are a model, and they grow with the wiring lane.

**8. The census is a disc test.** `#[ignore]`d, in `guac-formats`, it opens every `.arc` entry's bank and every `.bld` IGZ's embedded bank, groups by family off the archive's path (`character/` characters, `level/level_*` and `level_hub_*` story and hub, `arena_*` arena, `pvp_*` PvP, `challenge*` challenge, `permanent/` global, `misc/` and `item/` their own rows) and prints codec, channels, rate, sample counts and loop points, failing on any bank that doesn't open or whose MPEG doesn't walk. Counts, never bytes, go in the tick note.

## Risks / Trade-offs

- [A family holds a codec the remux doesn't cover] → AR.2's census says so before the part is written; that family's sounds get a box and play nothing meanwhile.
- [The path join misses a stream the hash would find] → AR.3 checks every streamed data info on the disc finds exactly one English entry, and lists any that don't.
- [The part is large: 320 kbps voices for 67 characters] → measured at AR.5 (`.arc` English entries alone are under 750 MiB disc-wide); a re-encode to a lower rate is a later trade, never the default, since the remux is exact.
- [FMOD's padding has a shape the walker refuses] → the walker refuses non-zero bytes between frames by name; the whole-disc test runs it over every MPEG bank.
- [The meanings in `guac-sound` are wrong] → each is marked a hypothesis where it is one and listed in the stand-in ledger by the wiring boxes; the oracle can't hear, so scripts' use is the evidence.

## Migration Plan

Nothing to migrate: the part adds files to an extract (a cache), guacamole reads nothing new until the wiring boxes land. AR archives once AR.0 holds; it delivers nothing a release carries.

## Open Questions

- What function `_streamArchiveHash` is (decision 4). The path join is what AR uses meanwhile; finding the hash changes no box.
