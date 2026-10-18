# Phase AC - Giants decode gate: one hero's mesh, textures, skeleton and clip, checked against the oracle

## Why

Every rung from M1 on stands on four decoders nobody has proven on Giants for PS3:
- IGZ v6 object graphs, whose field layouts are NOT in the files;
- `igImage2` textures;
- Sony Edge compressed geometry;
- Havok 2011.2 animation.

If one of them can't be cracked, Giants stops being the first game, and that's far cheaper to learn now than after the rungs above it are built on it. AC is that gate. It decodes one hero completely, checks each layer against what the original actually draws in the patched RPCS3 and ends in a written verdict: PASS and the ladder continues on Giants, or a layer out of leads and a Trap Team proposal goes to chotchki.

## What Changes

- No timebox. AC reports measured progress per layer as it goes (passed, decoding but outside tolerance with the measured gap, or blocked with what was tried), and the gate closes on a condition: all four layers inside tolerance against the RSX capture (PASS), or a layer that has run out of leads (design decision 9), or chotchki calling it from the progress so far. AC runs in parallel with AA and AB. Every decoder reads its input through AB.3's IGA reader, so AC.1 starts once AB.3 has landed; AB.1-AB.3 need nothing from AA.
- The RE toolchain from `docs/native/guacamole-design.md` (RE toolchain) gets stood up for real:
  - Ghidra 12.1.x with [clienthax/Ps3GhidraScripts](https://github.com/clienthax/Ps3GhidraScripts), plus its `r2`-unaffected cspec patch;
  - the EBOOT decrypted from a COPY by `rpcs3 --decrypt`, which writes next to its input;
  - the Ghidra project kept outside the repo;
  - RPCS3's guest memory dumper and RSX capture.
- The gate hero is chotchki's pick, Eruptor unless he names another. It's the first 3D figure the kids see in AD, so it's his call. Nothing reads a `.sky` or queries the HTPC to choose it.
- Oracle captures of that hero in the patched RPCS3, all kept outside the repo: one RSX capture of a frame with the hero on screen, two more during one looping clip, a guest memory dump and an `sys_fs` open log. The hero gets onto the oracle's emulated portal from the dev's own figure pack, dev-only like the rest of RPCS3; nothing in the `guac-*` crates or `guacamole` reads it. Captures are taken by hand with the game window's capture shortcut, so the gap between the two clip frames isn't known in frames, and the clip check (design decision 5) doesn't need it.
- `tools/rrc-dump`, a dev tool that reads RPCS3's RSX capture format (magic `RRC`, version 6, gzip plus RPCS3's own serializer, `vendor/rpcs3/rpcs3/Emu/RSX/Capture/rsx_replay.h` and `RSXThread.cpp:73-100`). It decodes the subset of RSX methods that locate a draw's vertex arrays, index array, textures and vertex-program constants, and writes them to a directory outside the repo.
- The reflection hunt: find Giants' igMetaObject registry in the memory dump, cross-referenced with the RTTI and `*MetaField` names in Ghidra. The field layouts of just the types the gate needs get written as our own Rust definitions in `guac-formats`.
- `guac-formats` gains these modules, each with synthetic-fixture unit tests, the property test and a fuzz target:
  - IGZ v6: sections, the numbered fixups and the object graph;
  - `igImage2` → KTX2, the original BC1/BC3 blocks passed through (untile and endian fix only, no re-encode);
  - `igPS3EdgeGeometry`: indices, attributes and skin bone maps;
  - a Havok 2011.2 packfile reader with `hkaSkeleton` and one animation clip.
- A dev viewer shows the hero textured at bind pose, playing the clip on a turntable, read straight from the user's dump. That's the only thing anyone sees, and the kids don't.
- The verdict lands in the design doc either way. When a layer is out of leads it comes with a proposed Trap Team change (IGA 0x0B, IGZ v8 with 4CC fixups, Havok 2013.1, the public TT 1.1.0 reflection schema in [ig-workshop-tests](https://github.com/spyrosadventure/ig-workshop-tests), and Bink 2 for its M0). Nothing pivots until chotchki decides.

## Capabilities

### New Capabilities

None. `.openspec.yaml` sets `skip_specs: true`. AC is research plus parsers whose observable result (what the TV shows) is AD's spec. What it learns about the formats is written in our own words into `docs/native/guacamole-design.md`, not into a behavior spec.

### Modified Capabilities

None.

## Impact

- Track: Guacamole, rung AC, the decode gate. No kid-visible milestone (a dev viewer only). PASS unblocks AD (M1); a layer out of leads triggers the Trap Team proposal.
- Disc serial read: BLUS30968 only. Trap Team isn't touched unless a layer runs out of leads, and then only by the proposal.
- guac-extract's output doesn't change: the new parsers aren't wired into extraction until AD, so the layout and IR stay at AB's v1.
- Code:
  - new modules in `crates/guac-formats` (IGZ v6, `igImage2`, Edge, Havok 2011.2, and the recovered type layouts);
  - a new workspace tool `tools/rrc-dump`;
  - the dev viewer as `crates/guacamole/examples/hero_view.rs`.
- Disc-dependent tests are `#[ignore]`d and read `GUAC_GIANTS_DUMP` and the capture directory (`GUAC_CAPTURE_DIR`) from `.env.dev`. Captures, decoded output, the decrypted EBOOT and the Ghidra project all live outside the tree, and AB.1's game-magic guard catches an `RRC` or `IGZ\x01` file that slips into a commit.
- Owning doc: `docs/native/guacamole-design.md`, in Giants in detail, Risks, RE toolchain, The oracle and Why Giants first (the gate's outcome). CLAUDE.md gains the new `.env.dev` keys.
- Release reach: none. Research, parsers and dev tools land on the `2.0` branch (main until some milestones are through, `openspec/config.yaml`), and it archives there once the verdict is written. No `v*` tag carries anything from AC.
