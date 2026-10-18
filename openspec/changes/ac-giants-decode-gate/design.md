# Design

## Context

What's already measured, from `docs/native/guacamole-design.md` (Giants in detail):
- IGZ v6 is `IGZ\x01` + a big-endian version, with section descriptors from 0x10 and the fixups in section 0. The fixups are numbered: 0 TMET, 1 TSTR, 2 EXID, 3 EXNM, 5 RVTB, 0xA TMHN, 0xC MTSZ and 0xE RSTR are known; 6, 7, 8, 0xB, 0xF and 0x10 aren't. TMET names the object types, but the field layouts live in the EBOOT's reflection registry. External refs are FNV-1a32 hashes.
- Textures are `igImage2` in `dxt1`/`dxt5` (tiled and linear) and some `l8_tile`. What `_tile` means on PS3 is unknown.
- Meshes are `igPS3EdgeGeometry` + segments + `igEdgeGeometryAttr`. Reading aids are the v6 Edge path in [NoesisPlugins](https://github.com/NefariousTechSupport/NoesisPlugins) `fmt_alchemy_igz.py`, chrrox's index decompression (credited in [SkydraHeavy](https://github.com/spyrosadventure/SkydraHeavy)) and igCauldron's `EdgeStuff.cs`, which targets later IGA.
- Havok `hk_2011.2.0-r1` packfiles ride inside `level.bld` and the character bundles. [Kovah](https://github.com/NefariousTechSupport/Kovah) has 2011.2 class metadata dumped from Giants on Wii (same SDK version, also big-endian with 32-bit pointers), and [HavokLib](https://github.com/PredatorCZ/HavokLib) reads spline-compressed animation in that range.
- The design doc lists Giants' 20 on-disc SPU images as all FMOD. Where Edge decompression and skinning actually run (an SPU job embedded in the EBOOT, the PPU or the RSX vertex program) is NOT known, and it decides what an RSX capture holds.

AB.3 supplies the IGA reader. The oracle is the product's patched RPCS3 running Giants on the Mac, on whichever pin boots it: the in-tree `accfecd2d` has never booted a game, while `09d602fd5` (the v1.9.19 build) ran Giants at ~60 fps SPU-LLVM. AC.2 settles which first; a bump past that is chotchki's call. The hero reaches its emulated portal from the dev's own figure pack, which only the oracle reads (design doc, The oracle).

## Goals / Non-Goals

**Goals:**
- Prove all four layers on one hero against the original's own rendered data, with numeric tolerances fixed before the comparison runs.
- Leave a method AE can repeat across the roster: the capture recipe, the RRC reader, the layout-recovery recipe.
- A verdict on a condition, never on a date: PASS, or a named layer out of leads, never "mostly".

**Non-Goals:**
- The other 47 characters, levels, collision, materials and shading, tfbScript.
- Wiring the parsers into guac-extract or the IR (AE).
- A faithful renderer. The dev viewer uses plain Bevy materials, and its frame comparison is recorded, not gated.
- Any Trap Team work beyond writing the fallback proposal.
- Choosing the hero from the kids' play data. It's chotchki's pick (Eruptor unless he names another), and nothing reads a `.sky` or the HTPC for it.

## Decisions

**1. The capture is the ground truth; other tools are reading aids.**
NoesisPlugins, igCauldron, HavokLib and Kovah target other builds (Wii, later IGA, other Havok point releases). Their output tells us where to look, never what's right. The RSX capture is what the original fed the GPU on this disc, so every pass/fail compares against it.

**2. The first capture also answers where skinning runs.**
The capture's draw for the hero shows one of two things:
- float positions already posed, which means Edge decompression and skinning happened before RSX, on SPU or PPU;
- a bind-pose stream plus bone matrices in vertex-program constants, which means RSX skins.

AC.2 records which one, since it decides the comparisons in decisions 5 and 7. If it's the posed case, bind pose is checked through the skeleton (decision 7) rather than straight from the vertex stream.

**3. `rrc-dump` decodes a subset of RSX, not all of it.**
An RRC file is a gzip of RPCS3's serializer over `frame_capture_data`: memory blocks keyed by RSX offset and location, plus the FIFO command list with per-command memory state. The capture also carries the register state at its start (`reg_state`, `RSXThread.cpp:73-83`). `rrc-dump` starts from that and replays the command list only far enough to track the NV4097 registers that locate:
- the vertex data arrays (offset, format, stride);
- the index array and the draw ranges;
- the texture units (offset, format, size, the swizzle flag);
- the vertex-program constants.

At each draw it writes those buffers out raw, plus a JSON description. Register names come from RPCS3's `rsx_methods.h`. The reader pins RRC version 6 and errors on any other, because a capture is only meaningful against the RPCS3 pin that wrote it (recorded alongside it), and a pin bump can change the format. Unit tests use synthetic captures built in code.

**4. Field layouts come from the running game's registry, written as our own code.**
1. Locate igMetaObject instances in the guest memory dump by the TMET type names the IGZ files carry, and follow each to its field list. The `*MetaField` RTTI names in Ghidra say which field kind each entry is.
2. Read the offsets and sizes.
3. Confirm one or two types against their constructors in Ghidra.

The output is a Rust definition per type, tagged "IGZ v6 / BLUS30968 01.00", for the gate's types only: `igImage2`, `igPS3EdgeGeometry` and its segments, `igEdgeGeometryAttr` and whatever wraps the skeleton and clip. The design doc's rule holds: layouts recovered by RE live in `guac-formats` as code, so an install never needs the EBOOT.

The fallback if the registry won't surface: infer a type's layout by diffing many instances across the 1,542 texture IGZ and the character bundles. That's statistical and weaker evidence, but it doesn't need the EBOOT at all.

**5. Tolerances, fixed now.**
- Indices: each draw's triangles are an ordered subset of ours, in our order and with the same vertex rotation (chotchki, 2026-10-18: the SPU job culls, `igPS3EdgeManager._cullingState` 1, so a draw holds only the triangles that survive; this replaces "the triangle set is equal to ours").
- Positions: equal after the same dequantization, or within 1e-4 of the mesh's bounding-box diagonal if the capture's floats come from a different dequantize order.
- UVs: within 1/65536. Normals: within 1e-3 after normalization.
- Textures: BC blocks byte-identical to the capture's surface after our untile. If RSX holds them re-swizzled, the comparison is after the same swizzle, and a FLIP number on the decoded RGBA is recorded but not gated.
- Skeleton: bind-pose world transforms within 1e-4 (translation, relative to the skeleton's extent) and 1e-4 rad.
- Clip: for each of the two clip captures, solve for the clip time by searching over the clip's duration for our pose that best matches it; at the solved time the match must hold within 1e-3 of the bounding-box diagonal for skinned positions, or 1e-3 for bone transforms if RSX skins. Two independent solves, both inside tolerance at two different clip times, avoid needing the game's clock or the frame gap between hand-taken captures. A clip that holds the same pose for long stretches can match anywhere in them, so the two captures go on visibly different poses.

**6. Unknown fixups get characterized only as far as the gate needs.**
The IGZ parser handles the known fixups, characterizes any unknown one an AC type depends on, and counts the rest. Where a needed object depends on an unknown fixup, the parser refuses rather than guesses. The census of fixup kinds across the hero's archives goes in the verdict.

**7. Havok: our own reader, with Kovah's metadata as the map.**
A 2011.2 packfile reader covering header, sections, class names and the local, global and virtual fixups, enough to materialize `hkaSkeleton` and the clip's `hkaAnimation` subclass. If that's spline-compressed, the decoder is ours, with HavokLib as the reading aid. Output for the viewer is bone transforms; glTF export waits for AE.

**8. The dev viewer is an example, not a product surface.**
`crates/guacamole/examples/hero_view.rs` reads the dump at `GUAC_GIANTS_DUMP`, decodes the hero in memory, builds a skinned Bevy mesh with the KTX2 textures and plays the clip on a turntable. It writes nothing to disk. A screenshot beside the oracle frame gets a FLIP number in the verdict for the record.

**9. Progress is measured; the gate closes on a condition.**
- Each layer carries a status in AC.9's ledger, updated whenever a check runs: passed (with its numbers), decoding but outside tolerance (with the measured gap) or blocked (with every lead tried).
- PASS is all four layers inside decision 5's tolerances.
- A layer is out of leads when every lead this design names for it has been tried and failed: for layouts, the registry hunt and its statistical fallback (decision 4); for Edge and Havok, the public references plus a stage-by-stage diff against the capture that no longer points at a stage to fix (decisions 1, 7 and Risks); for textures, the capture has ruled out every untile order tried and points at no other. Out of leads goes straight to the verdict (AC.20, split from AC.11), written as which layers passed and the exact gap on the rest.
- chotchki can call the verdict from the ledger at any point, and a layer still making progress keeps going until he does or it passes. No date, week count or effort figure decides it (design doc, Measuring progress).

**10. Out of leads is a proposal, not a pivot.**
When a layer runs out of leads, AC.20 writes the verdict and a proposed Trap Team change: Trap Team plus the ladder re-ordered, Bink 2 for its M0 and which of AC's work carries (the RRC reader, the method, IGA beyond 0x08). AC proposes nothing past that. chotchki decides.

## Risks / Trade-offs

- [The reflection registry isn't in a findable shape in memory] → decision 4's statistical fallback, plus Ghidra on the registration calls. Only when both fail is the layer out of leads.
- [Edge on v6 differs from every public reference] → the capture is ground truth, so a mismatch is a finding, not a dead end. The diff says which stage (index decompression, attribute unpack, bone map) is wrong.
- [The GDB stub needs the PPU interpreter for breakpoints (LLVM refuses them)] → memory dumps and Ghidra over live stepping, with the interpreter only for one targeted breakpoint if a registration has to be watched.
- [An RPCS3 pin bump mid-AC changes the RRC layout] → captures carry their pin and `rrc-dump` refuses other versions. The pin moves only on chotchki's call, and if it does, AC re-captures.
- [Three layers pass and the fourth stalls without running out of leads] → the ledger shows it, so chotchki sees the exact gap and decides between waiting, a scoped re-plan and the Trap Team proposal. AC doesn't pick for him.
- [Decoded output or a capture leaks into the repo] → everything decoded lives in `GUAC_CAPTURE_DIR` or memory, and AB.1's guard fails `cargo test` on a committed `RRC`, `IGZ\x01`, `IGA\x1a` or `SCE\0` file.
