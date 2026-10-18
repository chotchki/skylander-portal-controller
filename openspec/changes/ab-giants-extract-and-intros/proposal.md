# Phase AB - Giants extractor skeleton + M0: place a figure, its original intro plays

## Why

M0 is the cheapest proof that guacamole can read the user's own disc and play something from the original game: place a figure on the phone, that character's intro video plays on the TV. Getting there means standing up the extractor every later rung uses (disc identity, the container reader, the manifest, the transcode step) and the first cut of the disc's own roster, which the full reset makes the figure catalogue's real source. It also means answering a question the plan has carried since day one, because Bevy has no video playback.

## What Changes

- Three new crates, named per the codename rule:
  - `guac-formats`: pure parsers over byte slices, no I/O. In AB that's PARAM.SFO and the IGA v0x08 container with its chunked LZMA entries. Every parser runs under a property test in `cargo test` and a cargo-fuzz target. AA's setup reads title ids with the PARAM.SFO parser, so AB.1-AB.2 land before AA.5.
  - `guac-ir`: the versioned serde IR (RON in dev for diffing, postcard shipped). IR v1 holds the dump identity and the character table: toy type → archives → intro.
  - `guac-extract`: the library guacamole calls on first run, plus a `guac-extract` CLI over the same code. It covers identity, manifest, census, intro index and transcode.
- The dump comes from guacamole's own setup: the Giants dump AA's scan found in the folders the user chose. Nothing reads RPCS3's `games.yml` or needs an RPCS3 install.
- Identity before anything else. The key is (TITLE_ID, disc APP_VER, a hash over every plaintext USRDIR file but the EBOOT), with no title-update field: the parity target is the 1.00 disc (chotchki, 2026-10-17: "HTPC's Giants doesn't have the title updates I couldn't figure out how to install them"), so a dump a title update changed is reported as an unknown dump, never handled. The one known dump is chotchki's BLUS30968 at 01.00. Anything else gets its own error code and a report of names, sizes and hashes (never content), not a guess.
- Extracted files can't land in a checkout, by construction. In release the extract root is `extract/` under guacamole's own data root (AA design decision 6); in dev it's `GUAC_EXTRACT_DIR`, outside the tree. Either way it's refused when it sits inside any git working tree. The dump is opened read-only and nothing is ever written under it. A `cargo test` check fails when any file in git's index or in unpushed history starts with a game magic (IGA and IGZ in both byte orders, `BIKi`, `KB2`, `FSB5`, `SCE\0`, `NPD\0`, `\x7fPKG`, `RRC`, `\0PSF` and the disc's TROPHY.TRP, LIC.DAT, PS3_DISC.SFB and PS3UPDAT.PUP heads), checks a gzip file again after inflating its head (RPCS3 writes RSX captures as `.rrc.gz`) and fails on a big-endian PPC64 ELF (a decrypted EBOOT). That guard is the one [`docs/native/guacamole-design.md`](../../../docs/native/guacamole-design.md) proposed; AB builds it because AB is the first change that touches disc bytes.
- A census of the dump. Magic sniffing plus entropy over every USRDIR file and every IGA entry tests the design's "only the EBOOT is encrypted" assumption, and counts the audio tracks in each Bink file.
- The intro index, built from what the disc says, with a measured fallback:
  - `movies/` holds 61 Bink 1 files: 48 character intros, 11 FMVs (`fmv05` is absent) and 2 logos. 48 is also Giants' playable roster, and the intro names line up with it one for one, so every playable character has an intro.
  - Intros are named by character (`eruptor.mov`), while character archives are named by decimal toy type and an internal codename (`character/009_eruptor.arc`, but `000_*` for Whirlwind).
  - The bridge is the voice-over folder each character archive names in its entry list (`<name>/<language>/vo_<name>_…`). Read-only probes of the Giants dump mapped 41 to 44 of the 48 intros that way, depending on how the folder is picked.
  - The rest use a different voice-over naming, and some toy types have no archive of their own number (Dark Spyro rides toy 16's `_dark` archive). Those fall back to the name in AA's interim catalogue, and the census records how many came each way.
- The roster's groundwork. IR v1's character table is the first cut of the disc's own roster: per toy type, its archives (with the variant suffixes their names carry), voice-over name and intro. A disc test cross-checks it against AA's interim catalogue and records the toy types each side has that the other lacks. Names, elements and portraits stay with the interim catalogue until AE finds them on the disc and the roster replaces it.
- A transcode at extract time through an external ffmpeg. FFmpeg decodes Bink 1 (probed on a Giants file). guacamole looks for ffmpeg at the path in its own settings (`GUAC_FFMPEG` in dev), then next to its own executable, then on PATH. A missing ffmpeg is a named error with the install one-liner, and bundling it waits until guacamole ships alone.
- A spike picks how Bevy plays video. Bevy 0.20 has none: its `mp4` and `aac` features are rodio audio decoders. No third-party player targets 0.20 either (as of 2026-10-17: `bevy_av1` 0.3 is on Bevy 0.17, `bevy_cutscene` 0.1 on 0.19, `bevy_movie_player` 0.7 on 0.18, `bevy_video` 0.9 on 0.9). The spike measures candidate decoders against fixed criteria, and the winner fixes the transcode target.
- guacamole runs the extraction on the first Giants pick, with progress on the TV. After that, placing a figure plays its intro full screen with sound, then the portal view comes back with the cards. Intros queue, removal drops one, any controller button or key skips. A figure with no intro just gets its AA card.

## Capabilities

### New Capabilities

- `guacamole/disc-extraction`: how guacamole turns the user's own Giants dump into what it plays. Which dumps it accepts, that the dump is never changed and the output never lands in a checkout, extracting once and resuming after an interruption, and what the TV says when ffmpeg or the dump is missing.
- `guacamole/character-intros`: placing a figure plays that character's original intro on the TV. Which figures have one, queueing, skipping and removal and what happens while extraction is still running.

### Modified Capabilities

None. AA's specs aren't archived yet, and the cards' own behavior doesn't change: an intro plays over the portal view and the cards are there when it ends.

## Impact

- Track: Guacamole, rung AB, milestone M0. AB.1-AB.8 (the crates, parsers, identity, census, intro index, video spike and transcode) need nothing from AA and can start now; AA.5 needs AB.2's PARAM.SFO parser, and AB.9-AB.11 need AA's binary, setup and native driver. AC's decoders read through AB.3's IGA reader, so AB.1-AB.3 land first and AC starts after them.
- Disc serial read: BLUS30968 only, from the dump AA's setup found. Tests that read it are `#[ignore]`d and take the dump path from `GUAC_GIANTS_DUMP` in `.env.dev`.
- guac-extract's output is introduced here. The extract layout and IR version both start at 1, so every later change that bumps either forces a re-extraction (by design).
- Kid-visible milestone: M0. On the TV the Mac drives, a kid places a figure and that character's original intro plays with sound (Mac first, design doc Decisions, fourth batch; M0 on the HTPC under Steam Big Picture is the Windows/HTPC bring-up's, `openspec/backlog.md`).
- Code:
  - New crates `crates/guac-formats` (plus a `fuzz/` dir outside the workspace), `crates/guac-ir` and `crates/guac-extract`.
  - `crates/guacamole` gains the extraction screen and the intro player.
  - `crates/e2e-tests` gains an intro test, which builds its video with ffmpeg's test source at run time.
- Dependencies: a pure-Rust LZMA crate (`lzma-rs` 0.3 or `lzma-rust2` 0.22, AB.3 picks), `serde`, `ron`, `postcard`, `sha2` and the decoder the spike picks. ffmpeg is an external tool, not a crate dependency.
- Owning doc: `docs/native/guacamole-design.md` (The extractor, Giants in detail, Figures). CLAUDE.md gains the new `.env.dev` keys (`GUAC_EXTRACT_DIR`, `GUAC_GIANTS_DUMP`, `GUAC_FFMPEG`), the game-magic guard and, in Data & paths, the extract root: `extract/` under guacamole's data root in release (`~/Library/Application Support/guacamole/` on macOS, `%LOCALAPPDATA%\guacamole\` on Windows, machine-local), and in dev a path outside the tree, the one exception to "never write runtime state outside these roots".
- Release reach: none (AB.13). guacamole runs from `cargo` on the Mac and the first release that carries it is the Windows/HTPC bring-up's MSI; guac-extract ships inside guacamole then, not as its own artifact. ffmpeg isn't bundled: the Mac uses Homebrew's (`brew install ffmpeg`), and the HTPC's install (`winget install Gyan.FFmpeg`) is the bring-up's. AB changes no product code, and nothing reaches the dmg.
- One-way doors: none. An extract dir is a cache, since deleting it just re-extracts.
