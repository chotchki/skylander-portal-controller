# Design

## Context

AA leaves guacamole as a Bevy app that embeds the server, finds the Giants dump through its own setup (the folders the user chose, each dump identified by its PARAM.SFO) and receives `Placed { slot, placement }` from the native driver, carrying the figure's identity (toy type + variant) from the phone's pick. AB adds the first disc reader behind it. Facts measured on chotchki's Giants dump (BLUS30968, APP_VER 01.00; no title update on the Mac, and none on the HTPC by chotchki's account):

- `PS3_GAME/USRDIR` holds `character/`, `level/`, `item/`, `misc/`, `movies/`, `outfit/` and `permanent/`. `character/` has 227 files: 68 `.arc`/`.bld` pairs named `NNN_codename[_variant]` by decimal toy type, the card-game archives (`cardgame_streaming` plus 82 `cardgame_deck_NNN.bld`) and 7 other `.bld` bundles (lock puzzles, a stats screen, setup). Suffixes include `_legendary`, `_alt`, `_dark` and `_red`.
- `movies/` holds 61 Bink 1 (`BIKi`) files at 1280×720: 48 named by character, `fmv01`-`fmv12` without `fmv05` and two logos. Ten of the 61 carry 13 audio tracks, fmv01-fmv11 without fmv05: 12 mono (presumably per-language voice) plus one stereo id-0x0 track (unverified: likely the shared music and effects bed); the rest, every intro among them, carry one (AB.5's census).
- IGA v0x08 (format notes in `docs/native/guacamole-design.md`, cross-checked against [igArchiveLib](https://github.com/LG-RZ/igArchiveLib) and [igArchiveExtractor](https://github.com/NefariousTechSupport/igArchiveExtractor)) is laid out like this:
  - a 0x34-byte little-endian header carrying the TOC size, file count, sector size and the name-table offset and size;
  - a TOC of name hashes then 12-byte file records `{offset, size, blockWord}`, then the block tables;
  - a name table of per-file offsets into NUL-terminated names.
  - A `blockWord` of 0xFFFFFFFF means stored, otherwise its top nibble is the codec (2 = LZMA) and the rest indexes slots in the size-picked block table. Data comes in 0x8000-byte chunks; the chunk layout, raw chunks inside LZMA entries included, is in the guacamole-design.md IGA notes and `iga.rs`'s module docs (AB.3). `.arc` entries are stored and `.bld` entries are LZMA.
- Inside each character `.arc` the entry names include its voice-over lines as `<vo_name>/<language>/vo_<vo_name>_<line>`. `<vo_name>` with its underscores removed equals the intro's file name for 41 of the 48 intros (a read-only probe over the dump; nothing kept). A second probe that took the folder one level up from `<language>` got 44. The misses are characters whose voice-over folder uses a different naming; AB.6's disc test settles the real split.
- ffmpeg 9.0.2 on the Mac decodes `binkvideo` and both Bink audio codecs, and has `libx264`, `libsvtav1`, `libvpx-vp9`, `libmp3lame` and `libopus` encoders but no `libvorbis`.

Motivation is in proposal.md. The track-level extractor plan is in `docs/native/guacamole-design.md` (The extractor).

## Goals / Non-Goals

**Goals:**
- An extractor skeleton every later rung extends: identity, manifest, a parser crate with fuzzing from its first parser, an IR with a version and an output that can't leak into the repo.
- A video path that adds no runtime dependency on ffmpeg and needs no system libraries, checked on the Mac (Windows at the bring-up).
- M0 on the TV, driven by the Mac (Mac first, design doc Decisions, fourth batch). M0 on the HTPC is the Windows/HTPC bring-up's (`openspec/backlog.md`).

**Non-Goals:**
- IGZ, textures, meshes, Havok, FSB5 audio: AC and AE.
- Bink 2 (every later game): that's a later game's M0.
- Matching the original game's rule for when an intro plays (first placement per save, or every time). That's an oracle question for when guacamole runs Giants' own front end. M0 plays it every time, with a skip.
- Upscaling the intros (Enhanced comes later; the transcode keeps 1280×720).
- Bundling ffmpeg, extracting from the EBOOT or any decryption.
- Toy type → variant archive mapping beyond what the intro needs; AE needs it for models.
- Names, elements and portraits from the disc, and the roster replacing AA's interim catalogue on the phone (AE).
- Title updates. The parity target is the 1.00 disc (design doc, Decisions, third batch): nothing looks for an update, and a dump an update changed is an unknown dump (decision 4).

## Decisions

**1. Crate roles.**
- `guac-formats` is `#![forbid(unsafe_code)]` and touches no I/O. Every parser takes `&[u8]` and returns a typed result or a `FormatError` naming the offset and the field. Every length read from a file is bounded before it allocates (an entry can't claim more than the file holds, and a decompressed size can't exceed a per-format cap).
- `guac-ir` is serde types plus `IR_VERSION: u32 = 1`, RON in dev and postcard in an install.
- `guac-extract` owns the filesystem, hashing, the manifest and ffmpeg. guacamole calls it as a library on its own thread and gets progress events over a channel, the same seam AA uses for portal events.

**2. Pure-Rust LZMA.**
The IGA chunk format is raw LZMA1 with externally supplied props and a known output size (≤ 0x8000). AB.3 picks between `lzma-rs` 0.3 and `lzma-rust2` 0.22 by two tests: an API that takes those props plus an unpacked size without a header, and surviving the fuzz target without a panic or an unbounded allocation. C `liblzma` was the alternative; it's rejected because it would put a system library and `unsafe` FFI under the parsers. AB.3 picked `lzma-rust2` 0.23; the reasons are in tasks.md AB.3.

**3. Fuzzing in two layers.**
A stable property test in `cargo test` throws random bytes and mutated synthetic files at every parser and asserts no panic and bounded memory. That's what CI and the pre-push hook run. cargo-fuzz targets live in `crates/guac-formats/fuzz/`, excluded from the workspace because they need nightly, and run locally before AB.0 with the hours recorded. A CI fuzz lane is out of scope until there are more parsers to justify one.

**4. Identity and the known-dump table.**
The identity is (TITLE_ID, disc APP_VER, manifest hash). The manifest hash is SHA-256 over the sorted list of (relative path, size, SHA-256 of contents) for every file under `PS3_GAME/USRDIR` except `EBOOT.BIN`. There's no title-update field. chotchki set the parity target on 2026-10-17: "HTPC's Giants doesn't have the title updates I couldn't figure out how to install them", so Giants is the 1.00 disc and setup only ever finds disc dumps. An update can still reach a dump by being merged into it (files changed under USRDIR, or a PARAM.SFO whose APP_VER is past 01.00), and either one misses the known table, so it's reported as an unknown dump and never handled. Hashing every plaintext file has a first-run cost nobody has measured, so AB.4 measures it on the Mac (the HTPC's at the bring-up). The manifest caches each file's (path, size, mtime, inode, ctime, hash), so later starts re-hash only what changed. Size and mtime alone can't tell two dumps of this disc apart: every Giants file carries the disc's whole-second 2012 mtime, and a re-dump or a `cp -p`, Finder or `rsync -a` copy keeps it. No write can set ctime back, so a same-size change always misses the cache. The inode and ctime come from unix metadata; until the Windows bring-up fills them from the file index and ChangeTime, the cache never hits on Windows and every start re-hashes.

The known table in `guac-extract` holds identities and hashes only, never content: one entry per hash chotchki's own BLUS30968 01.00 dumps produce: the Mac's from AB.4, and the HTPC's added at the bring-up if it hashes differently. An unknown dump fails with `GUAC-E-UNKNOWN-DUMP` and writes `unknown-dump-<timestamp>.json` beside the extract root, so two dumps can be diffed by hand. The design's "unknown dumps get a report, not a guess" stays literal.

**5. Where output goes, and why it can't leak.**
- Release extract root: `extract/` under guacamole's own data root (AA design decision 6), so `%LOCALAPPDATA%\guacamole\extract\` on Windows (the root is machine-local because gigabytes don't belong in a roaming profile) and `~/Library/Application Support/guacamole/extract/` on macOS. Nothing lands under the controller's root.
- Dev: `GUAC_EXTRACT_DIR` in `.env.dev`, required, with no default. guacamole's dev data root (`./dev-data/guacamole/`) is inside the checkout, so the extract can't sit under it there: a default inside `./dev-data/` would be one `git add -f` away from a leak (design doc, No game bytes in the repo).
- Both the app and the CLI walk up from the resolved root and refuse it if any ancestor holds a `.git`.
- The dump is opened read-only and no code path takes a dump path as an output.
- Layout v1:
  - `<root>/<TITLE_ID>-<APP_VER>-<hash8>/manifest.json`, recording the extractor version, `IR_VERSION`, the identity, the source file records and every output with its content hash;
  - `ir/characters.{ron,postcard}`;
  - `video/intro/<name>.<ext>` and `audio/intro/<name>.<ext>`.
- Each output is written to `<file>.partial` and renamed when complete. Resume redoes anything without a finished output or whose recorded hash doesn't match.
- A manifest whose extractor layout or `IR_VERSION` differs from the running code is stale, and the whole identity dir is rebuilt.

**6. The intro index.**
For each intro the index does this:
1. List `character/NNN_*.arc` per decimal toy type.
2. Read each archive's entry names with the IGA reader and take the dominant `<vo_name>` from the `…/vo_<vo_name>_…` pattern.
3. Strip its underscores and match `movies/<name>.mov`.
4. On a miss, fall back to the name AA's interim catalogue gives the toy type's base figure, lowercased with non-alphanumerics removed (that catalogue is the wiki-derived one AE retires; until then it's what the phone shows).
5. A catalogue toy type still without one, whose name is another toy type's archive suffix plus that toy type's name (`dark` + Spyro is Dark Spyro, 28), shares that toy type's intro. It isn't a claim, so the 48 movies still map one to one.
6. A toy type with none of these has no intro and gets a card only.

The result is IR v1's character table: `toy_type → { archives (each with the variant suffix its name carries), vo_name, intro, source: disc | name_fallback | variant { base } }`. A `#[ignore]`d disc test asserts all 48 intros map to some toy type and records the disc-versus-fallback split. A variant on its base's toy type (Legendary Ignitor is toy 10 with variant bits) resolves through that toy type, which the figure's identity carries regardless of variant; a variant the catalogue gives a toy type of its own resolves by step 5 (on Giants, Dark Spyro and four Legendaries).

That table is the roster's groundwork (design doc, Figures): the toy types Giants has characters for, read off its own disc. The same disc test cross-checks it against AA's interim catalogue and records both differences, toy types with a character archive the catalogue lacks and catalogue toy types with no archive (magic items have none, so they're expected there). AB doesn't change what the phone lists. Mapping each archive suffix to a variant word and finding the names, elements and portraits on the disc is AE's, and AE's roster is what replaces the interim catalogue.

**7. Video playback: a spike with fixed criteria (AB.7), and a lean.**
Criteria:
- (a) builds with no system library installed, checked on the Mac by hand (the macOS CI lane is disabled; the Windows build is the bring-up's, so a candidate that would need one there loses on paper too);
- (b) decodes 1280×720 at the intro's frame rate in real time on the M3 Max, with decode ms/frame recorded (the HTPC's number at the bring-up);
- (c) puts frames into a Bevy 0.20 `Image` each frame without stalling the render;
- (d) audio plays through Bevy's own audio, with A/V drift under one frame at the end of the longest intro;
- (e) needs no ffmpeg at runtime.

Candidates:
- H.264 Constrained Baseline through the `openh264` crate (0.9.9, `source` feature builds the decoder with `cc`), encoded at extract time by ffmpeg's `libx264 -profile:v baseline`;
- AV1 through `dav1d` (needs `libdav1d` installed, which fails (a) on Windows unless vendored) or `rav1d` (a Rust port whose Rust-facing API is thin);
- VP9 through `libvpx` (a system library again);
- a runtime ffmpeg subprocess piping raw frames, the fallback only, since it fails (e).

The lean going in was openh264 with MP3 through Bevy's `mp3` feature, encoded with `libmp3lame` (Homebrew's ffmpeg and the common Windows builds both carry it; `libvorbis` is missing from Homebrew's, which rules out Ogg without the experimental native encoder). Spike code lives in an example, not in the shipped crate.

Picked (AB.7, 2026-10-18, numbers in tasks.md AB.7): **openh264 0.9.9 (`source`) for H.264 Constrained Baseline in MP4, MP3 through Bevy's `mp3`**, so AB.8's `transcode::TARGET` stands. crates.io still has no Bevy 0.20 video player: the ones that play files are on Bevy 0.9-0.19 and the newest (`bevy_cutscene`, 0.19) is FFmpeg-backed, and `bevy_gstreamer` (the one on 0.20) is webcam capture over GStreamer. The spike ([`examples/video_spike.rs`](../../../crates/guacamole/examples/video_spike.rs), `--features video-spike`) against AB.8's own extract, the display locked:
- (a) openh264 builds with `cc` alone; the binary links only macOS system frameworks. Off Windows on aarch64 the `source` build assembles OpenH264's NEON `.S` files through `cc` (the spike carries 193 `AArch64_neon` symbols), so the Mac numbers below are NEON. On x86 and x86-64 it gets assembly only with `nasm` on PATH and otherwise falls back to C without a warning, and MSVC never assembles `.S`, so the HTPC build needs `nasm` on PATH (the Windows/HTPC bring-up's). dav1d (`dav1d-sys` 0.8.3) and libvpx (`env-libvpx-sys` 5.1.3) fail with no pkg-config path. rav1d 1.1.0 fails with its default features on macOS (SVE2 flags, then an `asm-offsets.h` the published crate lacks) and builds only with no assembly.
- (b) openh264 decodes in 2.0-2.4 ms per frame on average on one thread over all 48 intros (16,428 frames, p95 3.7 ms), against 33.4 ms per frame, plus 1.1-1.4 ms for `write_rgba8`. rav1d with no assembly took 15.5 ms per frame on one thread, with single packets taking up to 169 ms, and needs `unsafe` calls into its dav1d C ABI. AV1 only buys file size (5.6 MB against 12.3 MB for the longest intro). That compared NEON openh264 with assembly-free rav1d; openh264 in pure C (NEON gated out of a patched `openh264-sys2`, outside the repo) takes 3.4 ms per frame over the same 16,428 frames (p95 5.7), still a tenth of the budget and 4.6x rav1d's speed, so the pick doesn't rest on the assembly.
- (c) Measured headless: a Camera2d renders to a 1920x1080 `Image` and a full-screen `ImageNode` shows a `RENDER_WORLD` `Image` whose `data` is swapped each frame. That's a `Vec` move in the main world and a `write_texture` into the reused texture in the render world, 0.5-0.9 ms per new frame on average, with the worst under 4 ms. In steady state render frames come no further apart with uploads than without. A real window under vsync is AB.16.
- (d) Bevy's MP3 decode (rodio, gapless) gives the source's sample count exactly with zero lag against the Bink audio. Over the longest intro the wall clock and the audio clock (CoreAudio, muted) drift 0.3-1.9 ms apart, under 0.06 of a frame. The audio clock steps in whole device buffers, though: `AudioSink::position()` moves about 42 ms at a time (rodio's 2048-frame buffer at 48 kHz is 42.7 ms), so presenting by it skipped 76-80 of 369 frames. AB.10 presents by the wall clock anchored to the sink's steps (the smallest wall − position seen; the spike's `--wall` does too since the AB.7 review, its first cut anchored to its own first update, which wandered −7 to −36 ms from the steps between runs) and holds video back by the output latency: one device buffer plus the output path's own. That differs by path (CoreAudio reports its own latency for the built-in speakers and for HDMI, and the TV's picture processing adds more), so it isn't one constant in code: AB.10 keeps it as a setting in guacamole's database, defaulting to the lag AB.16 measures on M0's path (HDMI to the TV).
- (e) nothing at runtime but the binary.

Two notes for AB.10: the spike demuxes with `mp4` 0.14 (AVCC to Annex B by hand, SPS/PPS ahead of each sync sample; clean on all 48), and AB.10 picks its own demuxer. Building OpenH264 from source also forgoes Cisco's patent coverage, which covers only Cisco's own binaries (the crate's `libloading` feature loads one instead).

**8. The transcode.**
- ffmpeg discovery order: the path in guacamole's settings (its own database; `GUAC_FFMPEG` in `.env.dev`), then beside the guacamole executable, then PATH.
- One invocation per intro: `-map 0:v:0` plus the chosen voice track, no scaling, the source frame rate and `-progress pipe:1` parsed into progress events.
- Track choice: a file with one audio track uses it. For the multi-track files, AB.5's census records which intros (if any) are multi-track, and the English track's index becomes data in the IR rather than a constant in code. The track language order isn't known yet and the files may not tag it. A multi-track file may need its stereo 0x0 bed mixed with the chosen voice track rather than one track picked alone.
- The ffmpeg command-line builder is a pure function, unit-tested; the process run is the only untested-in-CI part, covered by an `#[ignore]`d disc test and the e2e run.
- As landed (AB.8): the format is one constant, `transcode::TARGET`, H.264 Constrained Baseline in MP4 plus a separate MP3 (decision 7's pick, which AB.7 confirmed); its hash is recorded in the manifest and a different one redoes the intros. The voice choice is `Intro.voice` in the IR (`Only` on all 48 Giants intros); a bed, when there is one, goes first into `amix` so the mix stays stereo. A configured ffmpeg path that isn't a program is the error, not a fall-through to PATH. Each movie's manifest entry holds its recipe (source hash, voice, the path-free command line) and the ffmpeg that encoded it; resume needs the recipe to match, so a data fix to a voice choice reruns that intro.

**9. guacamole integration.**
- On a Giants pick, guacamole takes the Giants dump AA's setup found, checks the identity and either loads a current manifest or starts the extraction thread, with the progress screen on the TV.
- Placements during extraction just make cards.
- The intro player is a small state machine (idle → playing(slot) → idle, with a FIFO queue) driven by `Placed`/`Cleared` and input events. It's pure, so it's unit-tested without a window.
- Skip input comes from Bevy's gamepad and keyboard input. On the HTPC later, Steam Input off for guacamole's shortcut (design doc, Controllers) keeps the pad Bevy sees the real one. Any button skips, so no button prompt is shown and the prompt rule (AG) doesn't come up yet.
- AA's `/api/_test/tv` snapshot gains the playing intro and the queue, so the e2e test asserts "placement starts the intro" without decoding video in the browser.

**10. The game-magic guard is a test, not a hook script.**
[`crates/guac-extract/tests/no_game_bytes.rs`](../../../crates/guac-extract/tests/no_game_bytes.rs) reads git's index (`git ls-files`) plus every blob HEAD or a local branch reaches that no remote-tracking ref does (`git rev-list --objects HEAD --branches --not --remotes`), and fails if any begins with a listed magic. The index alone isn't enough: the guard runs at push time, a push sends commits and a file committed then deleted still ships with its commit. Only each blob's first 64 KiB is read. Two hard-rule items don't, so the guard covers them explicitly: a gzip file is inflated just far enough to check its head again (RPCS3 writes RSX captures as gzipped `.rrc.gz`, `RSXThread.cpp:3401`), and a 64-bit big-endian ELF with machine 21 (a decrypted EBOOT) fails. No tracked file is an ELF or a gzip today. `cargo test --workspace` already runs in `.githooks/pre-push` (on in the dev clone) and the Windows CI lane (the macOS lane is `if: false`, `ci.yml:155`), so the guard needs no new plumbing. Push CI fires only on `main`, so on `2.0` a `--no-verify` push gets past it until a merge or a dispatched run. Synthetic fixtures are always built in code at test time, so they never trip it. The e2e intro video is generated by ffmpeg's `testsrc` at run time and never committed.

## Risks / Trade-offs

- [The known-dump table is too strict: a valid dump that differs in one irrelevant file is refused] → by design for now (no guessing). The report makes the difference visible, and loosening to per-directory hashes is a later, measured change.
- [openh264 can't hold real time, or its frame upload stalls Bevy] → AB.7 measured the M3 Max at about 10× real time for decode plus RGBA on one thread, with an upload under 1 ms per frame on average (decision 7); the HTPC CPU is measured at the bring-up. AV1 via vendored dav1d is the next candidate either way.
- [The voice-over naming heuristic picks the wrong name for an archive that voices two characters] → the dominant-name rule plus the disc test asserting all 48 intros map one-to-one, and a wrong mapping shows up in the census before it reaches a kid.
- [The interim catalogue names a figure the disc has no character for, or lacks one it has] → the roster cross-check records both lists; a missing character goes into AA's supplement, and anything else waits for AE's roster.
- [Entropy flags legitimately compressed data (LZMA chunks, Bink, FSB5 MPEG) as "encrypted"] → the census classifies by magic first and reports entropy only for unknown-magic files, so "high entropy + unknown magic" is the signal, not entropy alone.
- [A first-run extraction takes long enough that the kids give up] → AB extracts only the 48 intros plus the hashing. The progress screen says what it's doing, cards work meanwhile and AB.11 records the measured Mac time so AE knows the real cost before it adds textures (the HTPC's at the bring-up).
- [ffmpeg's output varies across versions (encoder defaults)] → outputs are content-hashed after the fact, not compared to expected hashes, and the manifest records the ffmpeg version used.

## Migration Plan

New crates and a new extract root, nothing existing changes shape. Rollback is a revert plus deleting the extract root. The first IR is v1, and any later layout or IR change re-extracts automatically.
