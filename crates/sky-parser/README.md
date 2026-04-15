# skylander-sky-parser

Read-only parser for Skylanders `.sky` firmware dumps. A `.sky` file is a
1024-byte dump of the NFC chip on the back of a Skylander toy (Mifare Classic
1K: 64 × 16-byte blocks). RPCS3 reads/writes this file when emulating the
Portal of Power.

## Format reference

The authoritative format document is committed to this repository at
[`docs/research/sky-format/SkylanderFormat.md`](../../docs/research/sky-format/SkylanderFormat.md),
mirrored from the Runes project:

- <https://github.com/NefariousTechSupport/Runes/blob/master/Docs/SkylanderFormat.md>
  (BSD-3-Clause)

Credits for the reverse-engineering work are preserved in the spec's footer
(Brandon Wilson, Mandar1jn, Winner Nombre, Texthead, Maff).

## Plaintext, no AES

Real physical tags encrypt the per-figure data with AES-128 keyed off the
block contents + figure identity. **RPCS3 writes `.sky` files without that
encryption layer** — the bytes on disk are the decrypted payload directly.
This crate therefore contains **no AES code at all**. If you need to ingest a
dump pulled from a physical tag, decrypt it externally first.

## Scope

**Read-only, always.** This crate will never:

- write back to a `.sky` file,
- regenerate CRC checksums,
- move firmware files on disk (see `CLAUDE.md` — firmware pack stays put).

## Status

### Parsed today

- **Header** (plaintext block 0): serial, figure id (`toy_type` u24 LE), error
  byte, trading-card id (u64), variant bitfield, header CRC16/CCITT-FALSE.
- **Variant decomposition**: deco id, SuperCharger / LightCore / in-game /
  reposed flags, year code → `SkyGeneration`.
- **Web Code**: base-29 derivation from the trading-card ID using the
  `23456789BCDFGHJKLMNPQRSTVWXYZ` alphabet.
- **Standard `tfbSpyroTagData` layout** ("None of the above" section of the
  spec) via the area-sequence algorithm (block `0x08/0x24` offset 0x09 for the
  first data region, block `0x11/0x2D` offset 0x02 for the second):
  - `xp_2011` / `xp_2012` / `xp_2013`
  - derived character `level` via the spec's experience table
  - `gold`, `playtime_secs`, `nickname` (UTF-16 LE)
  - `hero_points`, `trinket`
  - hat history `[2011, 2012, 2013, 2015]` + resolved current hat via the
    per-year lookup algorithm (2015 byte gets +256 per spec)
  - `last_placed` / `last_reset` as `chrono::NaiveDateTime`
  - SSA + Giants Heroic Challenges bitfields
  - Battlegrounds flags, Giants quests (u72), Swap Force quests (u72)
- **Checksum validation** (`checksums_valid: bool`): header CRC, plus the
  region-A "0x30-byte" and 14-byte CRCs on the active data area.

### Stubbed / deferred

- **Trap, Vehicle, Racing Pack, CYOS/Imaginator-crystal** layouts classify as
  `FigureKind::Other` and their payload-specific fields stay at defaults.
  Detection is heuristic (figure-id ranges) and should tighten once the
  `kTfbSpyroTag_ToyType.hpp` enum from Runes is checked in. TODOs in source
  point at the relevant sections.
- **Quest-table decoding**: we expose the raw u72 words; decoding individual
  quest progress (Giants vs Swap Force bit layouts) is a future pass.
- **Usage info** (block `0x10/0x2C` offset 0x00) — the spec itself marks this
  as TODO.

## Testing

```
cargo test -p skylander-sky-parser
```

Tests use a synthetic fixture generator (`src/lib.rs::fixture`) that emits a
well-formed 1024-byte blob with correctly-computed CRCs. The suite covers
header round-trips, variant decomposition, web-code derivation, the
experience table, standard-payload round-trips, hat-lookup ordering for
oldest-first (SSA/Giants/TT) vs newest-first (SF/SC/Imaginators) games,
timestamp encoding, CRC-tamper detection, area-sequence wraparound
(`255 → 0`), and JSON round-trip.

**No real `.sky` files are committed** — redistributing them is a piracy
concern (see `CLAUDE.md`).
