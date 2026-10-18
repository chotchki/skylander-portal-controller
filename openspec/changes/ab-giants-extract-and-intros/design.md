# Design

## Context

AA leaves guacamole as a Bevy app that embeds the server, finds the Giants dump through its own setup (the folders the user chose, each dump identified by its PARAM.SFO) and receives `Placed { slot, placement }` from the native driver, carrying the figure's identity (toy type + variant) from the phone's pick. AB adds the first disc reader behind it. Facts measured on chotchki's Giants dump (BLUS30968, APP_VER 01.00, no title update on the Mac):

- `PS3_GAME/USRDIR` holds `character/`, `level/`, `item/`, `misc/`, `movies/`, `outfit/` and `permanent/`. `character/` has 227 files: 68 `.arc`/`.bld` pairs named `NNN_codename[_variant]` by decimal toy type, the card-game archives (`cardgame_streaming` plus 82 `cardgame_deck_NNN.bld`) and 7 other `.bld` bundles (lock puzzles, a stats screen, setup). Suffixes include `_legendary`, `_alt`, `_dark` and `_red`.
- `movies/` holds 61 Bink 1 (`BIKi`) files at 1280×720: 48 named by character, `fmv01`-`fmv12` without `fmv05` and two logos. Ten of the 61 carry 13 audio tracks (per-language voice), the rest one; which ten isn't recorded yet.
- IGA v0x08 (format notes in `docs/native/guacamole-design.md`, cross-checked against [igArchiveLib](https://github.com/LG-RZ/igArchiveLib) and [igArchiveExtractor](https://github.com/NefariousTechSupport/igArchiveExtractor)) is laid out like this:
  - a 0x34-byte little-endian header carrying the TOC size, file count, sector size and the name-table offset and size;
  - a TOC of name hashes then 12-byte file records `{offset, size, blockWord}`, then the block tables;
  - a name table of per-file offsets into NUL-terminated names.
  - A `blockWord` of 0xFFFFFFFF means stored, otherwise its top nibble is the codec (2 = LZMA) and the rest indexes the block table. Data comes in 0x8000-byte chunks, each a u16 compressed size + 5 bytes of LZMA props + raw LZMA1. `.arc` entries are stored and `.bld` entries are LZMA.
- Inside each character `.arc` the entry names include its voice-over lines as `<vo_name>/<language>/vo_<vo_name>_<line>`. `<vo_name>` with its underscores removed equals the intro's file name for 41 of the 48 intros (a read-only probe over the dump; nothing kept). A second probe that took the folder one level up from `<language>` got 44. The misses are characters whose voice-over folder uses a different naming; AB.6's disc test settles the real split.
- ffmpeg 9.0.2 on the Mac decodes `binkvideo` and both Bink audio codecs, and has `libx264`, `libsvtav1`, `libvpx-vp9`, `libmp3lame` and `libopus` encoders but no `libvorbis`.

Motivation is in proposal.md. The track-level extractor plan is in `docs/native/guacamole-design.md` (The extractor).

## Goals / Non-Goals

**Goals:**
- An extractor skeleton every later rung extends: identity, manifest, a parser crate with fuzzing from its first parser, an IR with a version and an output that can't leak into the repo.
- A video path that adds no runtime dependency on ffmpeg and needs no system libraries on the Windows CI lane or the Mac.
- M0 on the HTPC.

**Non-Goals:**
- IGZ, textures, meshes, Havok, FSB5 audio: AC and AD.
- Bink 2 (every later game): that's a later game's M0.
- Matching the original game's rule for when an intro plays (first placement per save, or every time). That's an oracle question for when guacamole runs Giants' own front end. M0 plays it every time, with a skip.
- Upscaling the intros (Enhanced comes later; the transcode keeps 1280×720).
- Bundling ffmpeg, extracting from the EBOOT or any decryption.
- Toy type → variant archive mapping beyond what the intro needs; AD needs it for models.
- Names, elements and portraits from the disc, and the roster replacing AA's interim catalogue on the phone (AD).
- Looking for title updates. Setup knows where the dumps are, not where an update lives, and whether the parity target includes one is the design doc's Open question 6.

## Decisions

**1. Crate roles.**
- `guac-formats` is `#![forbid(unsafe_code)]` and touches no I/O. Every parser takes `&[u8]` and returns a typed result or a `FormatError` naming the offset and the field. Every length read from a file is bounded before it allocates (an entry can't claim more than the file holds, and a decompressed size can't exceed a per-format cap).
- `guac-ir` is serde types plus `IR_VERSION: u32 = 1`, RON in dev and postcard in an install.
- `guac-extract` owns the filesystem, hashing, the manifest and ffmpeg. guacamole calls it as a library on its own thread and gets progress events over a channel, the same seam AA uses for portal events.

**2. Pure-Rust LZMA.**
The IGA chunk format is raw LZMA1 with externally supplied props and a known output size (≤ 0x8000). AB.3 picks between `lzma-rs` 0.3 and `lzma-rust2` 0.22 by two tests: an API that takes those props plus an unpacked size without a header, and surviving the fuzz target without a panic or an unbounded allocation. C `liblzma` was the alternative; it's rejected because it would put a system library and `unsafe` FFI under the parsers.

**3. Fuzzing in two layers.**
A stable property test in `cargo test` throws random bytes and mutated synthetic files at every parser and asserts no panic and bounded memory. That's what CI and the pre-push hook run. cargo-fuzz targets live in `crates/guac-formats/fuzz/`, excluded from the workspace because they need nightly, and run locally before AB.0 with the hours recorded. A CI fuzz lane is out of scope until there are more parsers to justify one.

**4. Identity and the known-dump table.**
The identity is (TITLE_ID, disc APP_VER, installed-update APP_VER or none, manifest hash). The manifest hash is SHA-256 over the sorted list of (relative path, size, SHA-256 of contents) for every file under `PS3_GAME/USRDIR` except `EBOOT.BIN`. The update field is always none in AB (Non-Goals); the key keeps it so a later change that adds updates doesn't change the identity's shape. Hashing every plaintext file has a first-run cost nobody has measured, so AB.4 measures it on the Mac and the HTPC. The manifest caches each file's (path, size, mtime, hash), so later starts re-hash only what changed.

The known table in `guac-extract` holds identities and hashes only, never content. An unknown dump fails with `GUAC-E-UNKNOWN-DUMP` and writes `unknown-dump-<timestamp>.json` beside the extract root, so two dumps can be diffed by hand. The design's "unknown dumps get a report, not a guess" stays literal.

**5. Where output goes, and why it can't leak.**
- Release extract root: `%LOCALAPPDATA%\skylander-portal-controller\guacamole\extract\` on Windows (machine-local, since gigabytes don't belong in a roaming profile) and `~/Library/Application Support/skylander-portal-controller/guacamole/extract/` on macOS.
- Dev: `GUAC_EXTRACT_DIR` in `.env.dev`, required, with no default. A default inside `./dev-data/` would be one `git add -f` away from a leak (design doc, No game bytes in the repo).
- Both the app and the CLI walk up from the resolved root and refuse it if any ancestor holds a `.git`.
- The dump is opened read-only and no code path takes a dump path as an output.
- Layout v1:
  - `<root>/<TITLE_ID>-<APP_VER>[-u<UPDATE_VER>]-<hash8>/manifest.json`, recording the extractor version, `IR_VERSION`, the identity, the source file records and every output with its content hash;
  - `ir/characters.{ron,postcard}`;
  - `video/intro/<name>.<ext>` and `audio/intro/<name>.<ext>`.
- Each output is written to `<file>.partial` and renamed when complete. Resume redoes anything without a finished output or whose recorded hash doesn't match.
- A manifest whose extractor layout or `IR_VERSION` differs from the running code is stale, and the whole identity dir is rebuilt.

**6. The intro index.**
For each intro the index does this:
1. List `character/NNN_*.arc` per decimal toy type.
2. Read each archive's entry names with the IGA reader and take the dominant `<vo_name>` from the `…/vo_<vo_name>_…` pattern.
3. Strip its underscores and match `movies/<name>.mov`.
4. On a miss, fall back to the name AA's interim catalogue gives the toy type's base figure, lowercased with non-alphanumerics removed (that catalogue is the wiki-derived one AD retires; until then it's what the phone shows).
5. A toy type with neither has no intro and gets a card only.

The result is IR v1's character table: `toy_type → { archives (each with the variant suffix its name carries), vo_name, intro, source: disc | name_fallback }`. A `#[ignore]`d disc test asserts all 48 intros map to some toy type and records the disc-versus-fallback split. Variant figures resolve through their toy type, which the figure's identity carries regardless of variant.

That table is the roster's groundwork (design doc, Figures): the toy types Giants has characters for, read off its own disc. The same disc test cross-checks it against AA's interim catalogue and records both differences, toy types with a character archive the catalogue lacks and catalogue toy types with no archive (magic items have none, so they're expected there). AB doesn't change what the phone lists. Mapping each archive suffix to a variant word and finding the names, elements and portraits on the disc is AD's, and AD's roster is what replaces the interim catalogue.

**7. Video playback: a spike with fixed criteria (AB.7), and a lean.**
Criteria:
- (a) builds on the Windows CI lane and the Mac with no system library installed (the macOS CI lane is disabled, so the Mac half is checked by hand);
- (b) decodes 1280×720 at the intro's frame rate in real time on the M3 Max and on the HTPC, with decode ms/frame recorded;
- (c) puts frames into a Bevy 0.20 `Image` each frame without stalling the render;
- (d) audio plays through Bevy's own audio, with A/V drift under one frame at the end of the longest intro;
- (e) needs no ffmpeg at runtime.

Candidates:
- H.264 Constrained Baseline through the `openh264` crate (0.9.9, `source` feature builds the decoder with `cc`), encoded at extract time by ffmpeg's `libx264 -profile:v baseline`;
- AV1 through `dav1d` (needs `libdav1d` installed, which fails (a) on Windows unless vendored) or `rav1d` (a Rust port whose Rust-facing API is thin);
- VP9 through `libvpx` (a system library again);
- a runtime ffmpeg subprocess piping raw frames, the fallback only, since it fails (e).

The lean is openh264: it's the only candidate that meets (a) and (e) without vendoring work, and baseline H.264 at 720p is a light decode. AV1 would buy smaller files that nobody needs at 48 short clips. Audio: MP3 through Bevy's `mp3` feature, encoded with `libmp3lame` (Homebrew's ffmpeg and the common Windows builds both carry it; `libvorbis` is missing from Homebrew's, which rules out Ogg without the experimental native encoder). The spike's numbers decide; the lean is what it has to beat. Spike code lives in an example, not in the shipped crate.

**8. The transcode.**
- ffmpeg discovery order: the `GUAC_FFMPEG` config value, then beside the guacamole executable, then PATH.
- One invocation per intro: `-map 0:v:0` plus the chosen voice track, no scaling, the source frame rate and `-progress pipe:1` parsed into progress events.
- Track choice: a file with one audio track uses it. For the multi-track files, AB.5's census records which intros (if any) are multi-track, and the English track's index becomes data in the IR rather than a constant in code. The track language order isn't known yet and the files may not tag it.
- The ffmpeg command-line builder is a pure function, unit-tested; the process run is the only untested-in-CI part, covered by an `#[ignore]`d disc test and the e2e run.

**9. guacamole integration.**
- On a Giants pick, guacamole takes the Giants dump AA's setup found, checks the identity and either loads a current manifest or starts the extraction thread, with the progress screen on the TV.
- Placements during extraction just make cards.
- The intro player is a small state machine (idle → playing(slot) → idle, with a FIFO queue) driven by `Placed`/`Cleared` and input events. It's pure, so it's unit-tested without a window.
- Skip input comes from Bevy's gamepad and keyboard input. With Steam Input off for guacamole's shortcut (design doc, Controllers), the pad Bevy sees is the real one. Any button skips, so no button prompt is shown and the prompt rule (AF) doesn't come up yet.
- AA's `/api/_test/tv` snapshot gains the playing intro and the queue, so the e2e test asserts "placement starts the intro" without decoding video in the browser.

**10. The game-magic guard is a test, not a hook script.**
`crates/guac-extract/tests/no_game_bytes.rs` runs `git ls-files` and fails if any tracked file begins with a listed magic. Two hard-rule items don't, so the guard covers them explicitly: a gzip file is inflated just far enough to check its head again (RPCS3 writes RSX captures as gzipped `.rrc.gz`, `RSXThread.cpp:3401`), and a 64-bit big-endian ELF with machine 21 (a decrypted EBOOT) fails. No tracked file is an ELF or a gzip today. `cargo test --workspace` already runs in `.githooks/pre-push` and the Windows CI lane (the macOS lane is `if: false`, `ci.yml:155`), so the guard needs no new plumbing and can't be bypassed by `--no-verify` without CI catching it. Synthetic fixtures are always built in code at test time, so they never trip it. The e2e intro video is generated by ffmpeg's `testsrc` at run time and never committed.

## Risks / Trade-offs

- [The known-dump table is too strict: a valid dump that differs in one irrelevant file is refused] → by design for now (no guessing). The report makes the difference visible, and loosening to per-directory hashes is a later, measured change.
- [openh264 can't hold real time on the HTPC CPU, or its frame upload stalls Bevy] → the spike measures both machines before the transcode target is fixed. AV1 via vendored dav1d is the next candidate.
- [The voice-over naming heuristic picks the wrong name for an archive that voices two characters] → the dominant-name rule plus the disc test asserting all 48 intros map one-to-one, and a wrong mapping shows up in the census before it reaches a kid.
- [The interim catalogue names a figure the disc has no character for, or lacks one it has] → the roster cross-check records both lists; a missing character goes into AA's supplement, and anything else waits for AD's roster.
- [Entropy flags legitimately compressed data (LZMA chunks, Bink, FSB5 MPEG) as "encrypted"] → the census classifies by magic first and reports entropy only for unknown-magic files, so "high entropy + unknown magic" is the signal, not entropy alone.
- [A first-run extraction on the HTPC takes long enough that the kids give up] → AB extracts only the 48 intros plus the hashing. The progress screen says what it's doing, cards work meanwhile and AB.11 records the measured HTPC time so AD knows the real cost before it adds textures.
- [ffmpeg's output varies across versions (encoder defaults)] → outputs are content-hashed after the fact, not compared to expected hashes, and the manifest records the ffmpeg version used.

## Migration Plan

New crates and a new extract root, nothing existing changes shape. Rollback is a revert plus deleting the extract root. The first IR is v1, and any later layout or IR change re-extracts automatically.
