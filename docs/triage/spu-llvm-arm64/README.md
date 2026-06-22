# SPU-LLVM **Giga** block-size crash — isolation notes (Skylanders Giants, Apple Silicon)

Working notes toward a **precise upstream patch** for RPCS3 (not a bug report —
the maintainers reject drive-by AI reports). Found 2026-06-22 while validating
the pin bump (PLAN 16.12 → 16.13).

## One-line

`SPU Decoder = Recompiler (LLVM)` **+** `SPU Block Size = Giga` crashes the
*emulated game* (not the emulator) on **Skylanders Giants (BLUS30968)**: the
**FMOD** audio SPU program drives the shared SPU analyser to throw a bounds
check. **Mega block size runs the same game at ~60 fps**; the interpreter is also
fine. So the defect is **Giga-block-size-specific**, in the shared recompiler
analyser/megafunction builder (`SPUCommonRecompiler.cpp`).

## Decoder / block-size matrix (Giants, this M3 Max)

| SPU Decoder | SPU Block Size | Result |
|---|---|---|
| Recompiler (LLVM) | **Giga** | **CRASH** — `Range check failed (index: 372, container_size: 992)`, SPU thread frozen, ~30 s later a cascade RSX segfault |
| Recompiler (LLVM) | **Mega** | **~59–60 fps sustained**, boots clean (validated via `live_launch.rs`) |
| Interpreter (dynamic) | (n/a) | works (slow) — no static analysis, so the analyser bug isn't reached |

ASMJIT not yet tested empirically, but the throwing code is in
`SPUCommonRecompiler.cpp` (shared by LLVM **and** ASMJIT), so ASMJIT+Giga is
expected to crash too — to be confirmed.

## Environment

- **Host:** Apple M3 Max, macOS 26.5.1 (25F80). `hw.optional.arm.FEAT_I8MM = 1`.
- **RPCS3:** local build of pin `927e2492e` (`v0.0.40-637`) + this repo's P1/P2/P3
  patches → reports `RPCS3 0.0.41-local_build Alpha`. Built via `.ci-local/build-mac.sh`
  (Homebrew Qt 6.11, **LLVM 21.1.8**, MoltenVK). **Release build — DWARF stripped.**
- **Game:** Skylanders Giants, serial **BLUS30968**. Crashing SPU thread runs the
  **FMOD** audio middleware program.

## Crash signature (from `~/Library/Caches/rpcs3/RPCS3.log`)

1. Analyser flags **1190** functions malformed in a tight burst at emu-time
   `00:13.998`: `bad fallthrough to 0x…`, `bad stack frame`, `calls bad function`.
2. Immediately: `·F {SPU[…] Thread (FMOD)} SIG: Thread terminated due to fatal
   error: Range check failed (index: 372, container_size: 992)` →
   "Emulation has been frozen!"
3. ~30 s later (cascade, secondary): `·F {RSX […]} SIG: Thread terminated due to
   fatal error: Segfault reading location c6fc8cab9d00e40b …` — the RSX command
   stream reads garbage (`nop x4096` runs) because the game state is already dead.

The RSX segfault is **downstream**; the **root cause is the SPU analyser throw**.

## Root cause (pinned — no debug build needed)

The `ensure`/`::at32` machinery carries a compile-time `std::source_location`, so
even this **stripped Release build prints the exact line** in the fatal:

```
Range check failed (index: 372, container_size: 992)
(in file …/SPUCommonRecompiler.cpp:5261[:25], in function 'auto spu_recompiler_base::analyse()')
```

- **`SPUCommonRecompiler.cpp:5261`** — `const auto& bb_body = ::at32(m_bbs, bpc);`
  inside the `initiate_patterns` lambda of `spu_recompiler_base::analyse()`.
- `::at32(map, key)` throws when `key ∉ map` (`util/types.hpp:1054`). Here
  `bpc = 0x174` (= 372) is **not a key in `m_bbs`** (992 registered blocks).
- **The defect:** `initiate_patterns` dereferences `m_bbs[bpc]` (5261) and
  `m_bbs[first_pred_of_loop|bpc]` (5279) **unguarded**, while the sibling lambdas
  `get_block_targets` / `get_block_preds` (5238–5256) *do* guard
  (`m_block_info[pc/4] && m_bbs.count(pc)` → `{}`). In **Giga** mode the reg-state
  walk (callers at 5988 / 6037 / 6047) hands `initiate_patterns` a `bpc` that
  FMOD's malformed control flow left out of `m_bbs` → range error → SPU thread
  frozen → cascade RSX segfault.

## Fix (candidate — `0001-candidate-fix-guard-initiate_patterns.patch`)

Make `initiate_patterns` guard `bpc` exactly like its sibling lambdas, skipping
pattern detection for a non-block (`reduced_loop`/`atomic16`/`rchcnt_loop` are
optimizations — safe to skip):

```cpp
const auto initiate_patterns = [&](block_reg_state_iterator& block_state_it, u32 bpc, bool is_multi_block)
{
    if (!m_block_info[bpc / 4] || !m_bbs.count(bpc))
        return;                       // <-- added; matches get_block_targets/preds
    const auto& bb_body = ::at32(m_bbs, bpc);
    ...
```

+10 / −0 lines. **Verified:** rebuilt → Giga+LLVM boots Giants with **zero fatals**
(`boot-watch` OK 60s; the FMOD SPU thread compiles its blocks successfully past
0x174). See PLAN 16.13.4 for the fps + Mega/interpreter-regression confirmation.

## Upstream PR notes

Patch is against the pinned commit `927e2492e`; it applies to current master
(the seam hasn't churned). Frame for the PR as: *"`initiate_patterns` accesses
`m_bbs` unguarded where its sibling lambdas guard — crashes giga-mode SPU
recompilation of a real game (Skylanders Giants, FMOD program)"* with the repro
recipe above. **No AI-generated prose in the PR.**

## Repro / iteration

The crash repro config is saved at
`~/Library/Application Support/rpcs3/config.yml.repro-giga-llvm` (SPU LLVM + Giga).
Run via the lifted harness (auto-detects the freeze and kills the emulator so a
crash bails in ~20 s, not the full deadline):

```sh
RPCS3_EXE=…/rpcs3.app/Contents/MacOS/rpcs3 \
RPCS3_EBOOT="$HOME/Games/ps3/Skylanders Giants/PS3_GAME/USRDIR/EBOOT.BIN" \
RPCS3_CONFIG_DIR="$HOME/Library/Application Support/rpcs3" \
RPCS3_READY_SECS=180 RPCS3_SAMPLE_SECS=20 \
  cargo test -p skylander-rpcs3-control --test live_launch -- --ignored --nocapture
```

Evidence files alongside this note: `evidence-fatal-summary.txt`,
`raw-spu-fatal-block.txt`.
