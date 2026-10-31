# Phase AR - the audio core: the original's sounds read off the disc, extracted and looked up

## Why

Guacamole plays nothing in a level. The intros have their voice track (AB) and that's all: a throw, a bite, a coin, Eon's lines and the level's music are silent, and a cutscene that waits on its voices ends early (the stand-in ledger's `cosmetic` row: Norticus' scene 183 steps against the original's 300). The 2026-10-31 stock-take ([`stocktake/`](/Users/chotchki/workspace/guacamole-research/stocktake/)) measured the gap: 79,509 FSB5 banks in the stream archives, 147,255 `tfbSoundInfo` and `tfbSoundDataInfo` objects with no layout, no FSB5 parser, the scripts' `soundinfo` namespace at 0 of 25 members real (7 value cells, 18 absent) and `clonedsequence.clones` absent, and the music not located (AG design decision 16). The first reads for this change ([`ar-audio/`](/Users/chotchki/workspace/guacamole-research/ar-audio/), 2026-10-31, static, the dump read only) settle the shape:

- **A bank lives in one of two places.** Sound effects are FSB5 banks embedded in the IGZ that names them, as a `tfbSoundDataInfo`'s `_data` (level 213's `level.bld`: 254 of its 287 data infos); voices, music and ambience are streamed, their data info empty and the bank an entry of the same file's `.arc` (the other 33, among them all eleven `mu_213_*` music streams). The `.arc`s hold every voice line in up to 13 languages, English the one with no language folder.
- **Every bank read so far is one sample of MPEG-1 Layer III at 320 kbps**, mono or stereo at 32, 44.1 or 48 kHz, its frames separated by zero padding, so an MP3 is a remux in our own code, not a transcode, and plays on the path the intros' MP3s already play on.
- **A sound is a sequence.** `tfbSoundInfo` derives from `ClonedSequence` as `AnimationInfo` does, so scripts play a sound with the same start-sequence op that plays a clip; its volume, randomness, pan, falloff, reverb and filter are fields the EBOOT registers, its category, priority and limits a bitfield, and its looping, 3D and falloff switches bits of `Sequence._flags`, each read off the registry and the members' getters.

The TV's sound lane should play from the data and a lookup whose semantics are written down, not from guesses, so this change is the pure half: formats, IR, extract and lookup, no Bevy.

## What Changes

- **guac-formats** reads FSB5 (`#![forbid(unsafe_code)]`, byte slices in, registered for the property test, a fuzz target of its own) and walks an MPEG sample's frames into an MP3; it reads `tfbSoundInfo` and `tfbSoundDataInfo` the AC.4 way and resolves each sound info to its data, local or a `global.bld` import, and each streamed data info to its `.arc` entry.
- **A codec census** over every bank on the disc, embedded and streamed, by family (characters, story and hub, arena, PvP, challenge, global), decides the decode path; a whole-disc test opens every bank and resolves every sound info to a bank and a sample in it.
- **The music is located**: where level 213's tracks live, how its scripts name and switch them, how they stream.
- **The extract grows by a `sounds` part**: for `global.bld`, the 67 modelled characters and level 213, every sound info and data info as IR and each bank's audio as a file under the extract root, English voices only, needing no ffmpeg; an extract made before it resumes for the part alone.
- **A new pure crate, `guac-sound`**, answers what the wiring lane asks: given the sound a script names (a sound info, a music track), the playable file and its parameters as the data holds them, with what each parameter means written in our own words from how the disc's scripts use the `soundinfo` members.
- **The wiring lane's boxes are filed, unticked**: playback through Bevy audio, the `soundinfo` and `clonedsequence` members on the host, music by level, scene timing on voices, chotchki's listen.
- Out: playing anything (the wiring boxes), the other twelve languages, reverb and filter as effects, FMOD's own mixer, the Bink tracks' music bed, the other five games.

## Capabilities

### New Capabilities

- `guacamole/game-sound`: what the kids hear in a level: the original's sound effects, voices and music from their disc, each at its own volume, randomness, falloff and looping, voices holding a scene for their length.

### Modified Capabilities

- `guacamole/disc-extraction`: one ADDED requirement, the sounds chapter 1 can play extracted after the level and without the video converter.

## Impact

- Track: Guacamole, beside the ladder as AP is: AJ's living level and AM's cutscenes stand on it, and AR promotes the backlog's "Combat sound" line, deleting it there. It stands on AB (IGA), AC.4 (the registry), AE (the roster's 67 modelled characters), AG.15 (per-part resume) and AH.22 (the extract's part pattern); none in flight.
- Disc serial read: BLUS30968 01.00 only: every `.arc` and every `.bld`'s IGZ for the census and the whole-disc tests; for the part, `permanent/global.bld` and `global.arc`, the 67 character archives' `.bld` and `.arc`, `level/level_213_tutotial.bld` and `.arc`. Tests that read it are `#[ignore]`d and take `GUAC_GIANTS_DUMP`.
- guac-extract's output: a new extract-wide part, `sounds` (IR files of their own and the audio under `audio/sounds/`); `IR_VERSION` stays 6 and the layout stays 4 unless a record AR touches changes shape (AR.5 records which).
- Kid-visible milestone: none.
- Code: `crates/guac-formats` (the FSB5 reader, the MPEG frame walker, the sound layouts and reader, a fuzz target); `crates/guac-ir` (the sound records); `crates/guac-extract` (the part); new crate `crates/guac-sound` (pure, no Bevy). No guacamole, guac-server or phone change in this half.
- Dependencies: none new.
- Shared with the frozen product: nothing.
- Owning doc: [`docs/native/guacamole-design.md`](../../../docs/native/guacamole-design.md) (Giants in detail's audio line, The extractor, the stand-in ledger's `cosmetic` row, the ladder's neighbours); [`CLAUDE.md`](../../../CLAUDE.md)'s Architecture for the new crate.
- Release reach: none. guacamole runs from `cargo` on the Mac and no `v*` release carries it yet.
- One-way doors: none. The extract is a cache.
