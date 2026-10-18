# Phase AE - M1: the placed figure turns on the TV as its own animated model, in two looks, named off the disc

## Why

M1 is the first 3D the kids see: put a figure on the phone and the TV shows that character, textured and idling, turning on its card. It stands on AC's decode gate, which PASSED on 2026-10-18 (AC.20; [`docs/native/guacamole-design.md`](../../../docs/native/guacamole-design.md), Why Giants first, What flips it): decoded from the disc alone, Eruptor matches what the original fed RSX on textures, geometry, skeleton and clip. What was missing for a turntable was the look and the list, and the 2026-10-18 research closed both:

- The look is ONE effect. [`ae-materials`](/Users/chotchki/workspace/guacamole-research/ae-materials/FINDINGS.md) found `tfbmulti2.fx` drawing 2,201 of the 2,243 character materials, its 9,216 RSX variants on the disc; [`ae-shader`](/Users/chotchki/workspace/guacamole-research/ae-shader/FINDINGS.md) decoded it into one function of 14 feature bits that reproduces all 4,096 pixel and 5,120 vertex programs, confirmed on 10,877 patched copies in a savestate. No sRGB anywhere: the game does its arithmetic on stored (gamma-encoded) values.
- RPCS3's Giants looks oversaturated, and that's the game, not the Mac ([`ae-color`](/Users/chotchki/workspace/guacamole-research/ae-color/FINDINGS.md), refuted and corrected in [`VERIFY.md`](/Users/chotchki/workspace/guacamole-research/ae-color/VERIFY.md)): the body variant adds colour map, difference lighting, spec and rim in gamma space, red goes past 1 on 44.9% of Eruptor's pixels, and the 8-bit store clamps each channel on its own, so orange lava flattens to yellow (107° where the unclamped colour is 80.5°). Stock Bevy only looked calmer because it wasn't drawing the game's material.
- The list is on the disc too ([`ae-roster`](/Users/chotchki/workspace/guacamole-research/ae-roster/FINDINGS.md)): 123 figures, names in 12 languages and a 256x256 portrait each, with all 77 rows of AA's interim wiki catalogue among them (12 under the wrong variant id). A research VM runs the same toy table and gets the same 123 ([`aj-vm`](/Users/chotchki/workspace/guacamole-research/aj-vm/FINDINGS.md)).

## What Changes

- **The disc roster replaces AA's interim catalogue.** guac-extract reads the toy table out of `character/init_setup.bld`, the names out of `permanent/global.bld`'s 12 language packs and the portraits out of `misc/ui_collection_champions.bld`; the server's figure list comes from that, and `guacamole` reads no wiki data after it ([`native_catalogue.rs`](../../../crates/guac-server/src/native_catalogue.rs) and its hand-written supplement go; the files themselves leave at G1, [`openspec/backlog.md`](../../backlog.md) Removals). chotchki's two calls (2026-10-18), both carried as is:
  - figure ids take the bits Giants itself reads from the tag's subtype word ("agreed"): alt deco 0x400 | deco id, Series 2 0x800, LightCore 0x200. Today's `0x0BFF` identity mask drops bit 10, Giants' "is legendary", so 12 interim ids name the BASE figure to the original scripts (Polar Whirlwind is 0x0802 there, 0x0402 on the disc). Guacamole's own tables get a migration for the 12; the product's mask stays as it is.
  - Series 2 and LightCore are figures of their own with their own progress, though the disc treats each as its base character with a subtitle ("separate figures, we will now have access to ALL the figures so it will be easier to separate them all"). 123 entries, not 91.
- **Two looks, one shader** (chotchki's direction):
  - PARITY: the oracle-comparison look. Textures as stored (`Rgba8Unorm`, no sRGB decode), tfbmulti2's arithmetic in gamma space with the quirks that shape it, the per-channel clamp of an 8-bit target, no tonemapping. It's what gets checked against the oracle.
  - REMASTER: the game's own material and textures without the clip to yellow, in linear light: the disc's gamma-encoded constants go through `bevy_color` (`Srgba::rgb_u8` → `LinearRgba`), colour textures are sRGB-decoded and the result is HDR. Which colour fix sits on top is chotchki's call, with the measured options in design Open question 1.
  - One WESL shader (Bevy 0.20 replaced its WGSL dialect with WESL), its `@if` conditionals mapped onto tfbmulti2's feature bits, so a material's variant is a set of shader defs instead of ported microcode.
- **The turntable.** Each placed character's card shows its model off the disc: skinned with the bind AC.17 settled, posed by its own idle clip, its own materials, turning at a steady rate. Figures without a character model (items, adventure packs and sidekicks in AE) keep their disc portrait. Every material the roster draws is compiled before the portal shows, because macOS compiles a pipeline inside the frame that first needs it.
- **The renderer floor on the Mac, measured**: CSM + SSAO + TAA + bloom at the TV's resolution on the M3 Max, GPU timings per pass, no temporal upscaler, pipelines pre-warmed. That number is the budget the Enhanced line ([`openspec/backlog.md`](../../backlog.md), beside the ladder) has to fit until the Windows/HTPC bring-up.
- **The extract grows**: the roster, portraits, each character model (geometry, skin, skeleton, materials, idle clip) and its KTX2 textures, with the roster as the FIRST stage so the phone's list lands seconds into a first run, and nothing but the intros waiting on ffmpeg.

## Capabilities

### New Capabilities

- `guacamole/figure-roster`: the figures guacamole offers come off the user's own disc. Which figures, how they're named, elements and portraits, Series 2 and LightCore as figures of their own, no wiki data, what the phone shows before the roster is read, and figures saved under the interim ids coming back as the same figures.
- `guacamole/figure-turntable`: what the TV shows for a figure on the portal once it has a model: the character itself, textured, idling and turning on its card, variants as their own models, a fresh figure drawn as fresh, the portrait as the fallback, and never a stall.
- `guacamole/character-shading`: how the original game shades a character, written in our own words as the oracle shows it (the Method's spec for the shading model): the material, the gamma-space arithmetic and per-channel clamp, the lighting, specular, rim, tint, alpha and glow terms, and the quirks the look depends on.
- `guacamole/render-modes`: the two looks, PARITY and REMASTER, what REMASTER changes and keeps, switching between them and which one the TV starts in.

### Modified Capabilities

- `guacamole/figure-cards` (AA's delta, not archived yet): "Each figure on the portal shows as a card" carries the figure's turning model where its portrait was, the portrait staying as the fallback.
- `guacamole/disc-extraction` (AB's delta, not archived yet): "Only the original Giants disc is extracted" and "An unrecognized dump is reported, not guessed at" stop promising figure cards without a recognized dump (no interim catalogue is left to fall back on), and "Extraction runs once and survives interruption" puts the roster first so placing figures works seconds into a first run.

Both bases live in open changes, so `openspec validate` passes now and `openspec archive` refuses AE until AA and AB have archived (design, Migration Plan).

## Impact

- Track: Guacamole, rung AE, milestone M1. Stands on AC's PASS (AC.20) and on AA and AB's landed code. AC.21 (L8's untile and the upgrade-menu images) still holds AC's gate open until chotchki rules on it; AE doesn't wait for that (design Open question 7). The oracle comparison of PARITY (AE.16) waits on AF.5 (`SCREENSHOT` beside `SAVESTATE`) and AF.10 (FLIP); everything else starts now.
- Disc serial read: BLUS30968 01.00 only, from the dump AA's setup found: `character/init_setup.bld`, `permanent/global.bld` (its `level.bld` and 12 `.pak`), `permanent/bootstrap.bld` (the export-id base), `misc/ui_collection_champions.bld` and the character `.bld` archives. Tests that read it are `#[ignore]`d and take the dump from `GUAC_GIANTS_DUMP`.
- guac-extract's output changes: the IR goes from v1 to v2 and the extract layout gains the roster, portraits, models and textures, so every existing extract re-extracts once (by design, AB's manifest rule). Measured size and time get recorded (AE.3, AE.6).
- Kid-visible milestone: M1. On the Mac's own display (the TV pass in [`openspec/backlog.md`](../../backlog.md) covers the couch), every figure on the disc on the phone with its own portrait, and a placed character turning on its card after its intro.
- Code:
  - `crates/guac-formats`: the language-pack reader, the layouts the roster and materials read, the global export-id rule, the toy-table call reader.
  - `crates/guac-ir`: IR v2 (roster, models, materials, clips; the clip evaluator).
  - `crates/guac-extract`: the roster and model stages, ahead of the transcode; ffmpeg only for the intros.
  - `crates/guac-server`: the catalogue from the roster, shared and replaceable after boot (as AA.15 made the game list); `native_catalogue` and the reading of `DATA_ROOT` gone; a migration for the 12 changed ids.
  - `crates/guacamole`: the material and its WESL shader, the PARITY and REMASTER targets, the per-slot turntables, pipeline pre-warm, the look setting, `hero_view` on the new material with AC.18's camera options, the floor bench. Its Bevy features grow the 3D set (`bevy_pbr`, `ktx2`, tonemapping LUTs), its build time and size measured in AE.8.
  - `phone/`: the collection's note while the roster is being read.
  - `crates/e2e-tests`: a synthetic roster and model built in code under `test-hooks`, so the guacamole suite keeps running without a disc.
  - `crates/core`: nothing the product reads changes; the product keeps `VARIANT_IDENTITY_MASK`.
- Dependencies: none new beyond Bevy features already in the lockfile (`hero_view` builds them today as a dev-dependency).
- Owning doc: [`docs/native/guacamole-design.md`](../../../docs/native/guacamole-design.md) (Figures, Renderer, The extractor, Giants in detail, Running guacamole on the Mac, Milestone ladder, Risks). [`CLAUDE.md`](../../../CLAUDE.md)'s Guacamole and Dev-mode sections move with it (no wiki data, no `DATA_ROOT` for guacamole, the look setting).
- Release reach: none. guacamole runs from `cargo` on the Mac and the dmg stays product-only; the first release that carries it is the Windows/HTPC bring-up's MSI. AE changes no product behavior: the product neither links the new material nor reads the roster.
- One-way doors: none. The extract is a cache, and the id migration touches only guacamole's own database (a dev database so far: nothing has shipped).
