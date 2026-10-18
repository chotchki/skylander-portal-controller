# rrc-dump: RPCS3's RSX capture, version 6

`rrc-dump` reads one RPCS3 frame capture and writes, per draw, the vertex arrays, index range, texture surfaces and vertex-program constants the decode gate compares against (AC.3, design decision 3 in [the AC design](../../openspec/changes/ac-giants-decode-gate/design.md)). These notes are our reading of how the vendored RPCS3 pin writes the file. They hold for that pin ONLY (a pin bump can change any of it), which is why anything but version 6 is refused.

Sources read: [`rsx_replay.h`](../../vendor/rpcs3/rpcs3/Emu/RSX/Capture/rsx_replay.h) (the structs), [`RSXThread.cpp`](../../vendor/rpcs3/rpcs3/Emu/RSX/RSXThread.cpp) (the field order, around its `serialize<rsx::frame_capture_data>` and the writer in `on_frame_end`), [`serialization.hpp`](../../vendor/rpcs3/rpcs3/util/serialization.hpp) (the encoding), [`serialization_ext.cpp`](../../vendor/rpcs3/rpcs3/util/serialization_ext.cpp) (the gzip wrapper), [`rsx_capture.cpp`](../../vendor/rpcs3/rpcs3/Emu/RSX/Capture/rsx_capture.cpp) and [`RSXFIFO.cpp`](../../vendor/rpcs3/rpcs3/Emu/RSX/RSXFIFO.cpp) (what gets recorded when), [`rsx_replay.cpp`](../../vendor/rpcs3/rpcs3/Emu/RSX/Capture/rsx_replay.cpp) (how RPCS3 itself plays it back) and [`gcm_enums.h`](../../vendor/rpcs3/rpcs3/Emu/RSX/gcm_enums.h) / [`rsx_decode.h`](../../vendor/rpcs3/rpcs3/Emu/RSX/rsx_decode.h) / [`RSXTexture.cpp`](../../vendor/rpcs3/rpcs3/Emu/RSX/RSXTexture.cpp) (register layouts).

## Container

RPCS3 writes `<title>_<time>_capture.rrc.gz`: ONE gzip member (zlib, window bits 16 + 15) around the serialized stream, with no header of its own in front. Its loader also takes the bare stream under any name not ending in `.gz`/`.zst`, so the reader sniffs the gzip magic (`1f 8b`) instead of trusting the extension. zstd captures only come from hand conversion and aren't supported.

## The serializer

RPCS3's serializer is NOT a portable format, it writes in-memory bytes:

- Plain values (integers, enums and structs flagged bit-copyable) go out byte for byte, in the WRITING host's byte order and struct layout. Every struct the capture uses is built from naturally aligned `u32`/`u64` fields, so there is no padding to skip.
- Growable containers (`vector`, `unordered_map`, `unordered_set`) start with their element count as an unsigned LEB128 varint (7 bits per byte, low group first, high bit set = more follows), then the elements. A vector of plain values is its elements back to back; a map entry is key then value; a pair is first then second. Hash-container order is whatever the writer's table iterated in, so nothing can depend on it.
- Fixed-size arrays (`std::array`, C arrays) have NO count, just the elements.

The header word `LE_format` says which byte order the writer had. Any x86-64 or arm64 host writes 1, and that's the only value accepted: the reader is little-endian throughout and refuses a big-endian capture by name.

## `frame_capture_data`, in stream order

| Field | Encoding |
|---|---|
| magic | `u32`, the bytes `R R C 00` |
| version | `u32`, must be 6 |
| LE_format | `u32`, must be 1 |
| tile map | count, then per entry a 432-byte tile state (15 × `{tile, limit, pitch, format}` then 8 × `{region, size, start, offset, status0, status1}`, all `u32`) and its `u64` id |
| memory map | count, then per entry `{u32 offset, u32 location, u64 data_id}` and its `u64` block id |
| memory data map | count, then per entry a byte vector (count + bytes) and its `u64` data id |
| display buffer map | count, then per entry 132 bytes (8 × `{width, height, pitch, offset}` then `u32 count`) and its `u64` id |
| replay commands | count, then per command `u32 header`, `u32 value`, a set of `u64` block ids (count + ids), `u64 tile_id`, `u64 display_buffer_id` |
| register state | 2176 × `u32` vertex-program words, then OPTIONALLY 512 × `[u32; 4]` transform constants, then 16384 × `u32` method registers |

The optional constants are the one shape-shifter. RPCS3 writes them only when the session was booted FROM A SAVESTATE (it gates them on the savestate serialization version, which is zero on a normal boot), and its own loader expects them absent. So a normal capture carries NO starting constants, only what the frame itself uploads. The two shapes differ by exactly 8192 bytes at the end of the stream, which is how the reader tells them apart (anything else left over is an error).

## Memory blocks

All four maps draw ids from one counter (it starts at 0x1234 and is pre-incremented, so the first id is 0x1235 in the first capture an RPCS3 process takes). RPCS3's `reset()` clears only the tile map, memory map and command list between captures, never the data map, the display-buffer map or the counter. So a later capture in the same process starts at a higher id and also carries every earlier capture's data blobs, which no block references. Decoding is unaffected, but the "data blobs" count and the file size aren't per-frame numbers. A memory-map entry names an RSX location and an offset into it plus the data id of its bytes; identical bytes are stored once and shared. Location is 0 (local, VRAM) or 1 (main, through the IO map); the blit paths can also record the DMA handles `0xFEED0000`/`0xFEED0001`, which alias local and main, and the reader folds them together.

A command's block-id set lists the blocks to write into RSX memory BEFORE that command runs. RPCS3 snapshots a draw's inputs when the draw ends, so the blocks for a draw (fragment program, each enabled texture, the vertex ranges it read and its index range) hang off its `SET_BEGIN_END(0)`. Texture block lengths come from RPCS3's own subresource layout (all mips, all faces), which is why `rrc-dump` takes a texture's size from its block rather than recomputing it.

## The command list

Each command is a FIFO method header plus one argument, with the argument already byte-swapped to a host `u32`. Normally the header is `method_offset | 1 << 18` (count 1). Three method ranges keep their batch shape instead: `NV308A_COLOR`, the vertex-program upload `SET_TRANSFORM_PROGRAM` and the constant upload `SET_TRANSFORM_CONSTANT`. Their leading command's header carries the batch count in bits 18..29 (plus the non-increment flag in bit 30 if the game used one) and the next `count - 1` commands carry the remaining arguments with a zero header. RPCS3's replay consumes those by count and never looks at their headers; so does this reader. Semaphore methods (`NV406E_SET_REFERENCE`, `SEMAPHORE_ACQUIRE`/`RELEASE`) and the FIFO's own jumps, calls and returns are never recorded.

The first command is the odd one out: the capture-start NOP that carries the first tile and display state is stored as the bare register index 0x40, not `0x100 | 1 << 18`, so its count is 0. RPCS3's replay writes its header word and no argument; this reader counts it as an orphan. Exactly one orphan per capture is expected, more than one is an anomaly.

## What the replay tracks

The replay starts from the captured method registers and walks the commands in order, memory first and then the register write. The registers are a flat `u32` array indexed by `method_offset >> 2`, as on the RSX, so vertex array, index array and texture state is just read back at draw time:

- Vertex array `i`: `SET_VERTEX_DATA_ARRAY_OFFSET + 4i` (bit 31 = location, the rest the offset, added to `SET_VERTEX_DATA_BASE_OFFSET` and masked to 28 bits) and `SET_VERTEX_DATA_ARRAY_FORMAT + 4i` (type in bits 0..3, component count 4..8, stride 8..16, frequency 16..32). An array feeds the draw when its bit in `SET_VERTEX_ATTRIB_INPUT_MASK` is set and its component count isn't zero.
- Index array: `SET_INDEX_ARRAY_ADDRESS` (29 bits of offset) plus `SET_INDEX_ARRAY_DMA` (location in bits 0..4, type in 4..8: 0 = `u32`, 1 = `u16`). Each index gets `SET_VERTEX_DATA_BASE_INDEX` added and is masked to 20 bits; `SET_RESTART_INDEX` is skipped when `SET_RESTART_INDEX_ENABLE` is set (and ignored for `u16` indices when it doesn't fit 16 bits).
- Draws: `SET_BEGIN_END(primitive)` .. `SET_BEGIN_END(0)` (only the low byte counts, so `0x100` also ends a draw), with the `DRAW_ARRAYS` / `DRAW_INDEX_ARRAY` ranges in between (first in bits 0..24, count - 1 in bits 24..32).
- Texture unit `n` (16 fragment units, stride 0x20): `SET_TEXTURE_OFFSET` (31-bit offset), `SET_TEXTURE_FORMAT` (location + 1 in bits 0..2, cubemap bit 2, border bit 3, dimension 4..8, format 8..16, mip count 16..32), `SET_TEXTURE_CONTROL0` (bit 31 = enabled), `SET_TEXTURE_IMAGE_RECT` (width high 16, height low 16) and `SET_TEXTURE_CONTROL3 + 4n` (pitch in bits 0..20, depth 20..32). The format byte's 0x20 bit (`LN`) marks a linear layout and its absence a swizzled (Morton-ordered) one; 0x40 (`UN`) is unnormalized coordinates. EXCEPT for DXT1/23/45 and B8R8_G8R8/R8B8_R8G8, which RPCS3 never Morton-orders: for them `LN` clear only means packed rows (pitch ignored, layers 128-byte aligned), and 3D power-of-two DXT uses NVIDIA's VTC tiling instead. `draw.json` keeps the raw bit as `linear` and reports the real layout as `swizzled`.
- Constants: uploads go to a separate 468 × vec4 table at `SET_TRANSFORM_CONSTANT_LOAD + word / 4`, the load register isn't auto-incremented, and a batch writes consecutive words (capped at the 32-word method window) even when flagged non-increment. A word counts as known once the frame (or a savestate-booted capture's starting state) has written it.

Vertex and index data are big-endian in RSX memory and are written out raw; constants are host values and are written as little-endian `f32`.

`missing_bytes` counts bytes a draw reads that no captured block covered, per range: RPCS3 records one block per `DRAW_ARRAYS` range (and per index range), so the gap between disjoint ranges is zero-filled in the dumped span but not counted. An indexed draw's vertex window is min..max referenced vertex, the same window RPCS3 records. One known disagreement: for `u16` indices RPCS3's capture truncates `index + base` to 16 bits when it computes that window, so a base index pushing them past 0xFFFF leaves real missing bytes in the capture; the dump's 20-bit window is the correct one.

## What only a real capture settles

- That Giants' draws reach the capture un-flattened. RPCS3 can merge draws when a frame has more than 2000 of them (FIFO reordering), and the capture code warns its memory snapshots assume it's off. Turn on "Disable FIFO reordering" before capturing to be safe.
- Constants written between two ranges of one draw. RPCS3 defers those to the matching range; the dump snapshots constants at the draw's end and flags the draw (`constants_changed_mid_draw`) instead of splitting it.
- Vertex bindings changed between two ranges of one draw (`SET_VERTEX_DATA_BASE_OFFSET`, `SET_VERTEX_DATA_BASE_INDEX` or an array offset). RPCS3 draws each range with the binding it was issued under, but its capture snapshots every range at the FINAL binding, so the earlier ranges' true bytes were never captured and no decoder can recover them. The dump flags the draw (`vertex_bindings_changed_mid_draw`).
- What the hero's textures really held. RPCS3 snapshots guest memory, not GPU-resident data: every enabled texture is copied out of guest memory at the draw's end, render targets included, so a texture comes out whole (`captured: true`) even when its bytes are stale because RPCS3 never wrote the surface back. `captured: false` means RPCS3 recorded nothing at that address (its layout came out empty).
