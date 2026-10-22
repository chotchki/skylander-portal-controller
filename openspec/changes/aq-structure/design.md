# Design

## Context

- **guacamole's library is one compilation unit.** 52,044 lines in 54 files (`5920f56`): the TV's screens, the level and its loading, the hero, the pads, the scripts bridge, the renderer's character path and the test hooks. A one-line change anywhere recompiles the crate and relinks everything that links it: the binary, the `hero_view` example and 11 integration-test files, each its own binary linking Bevy. It depends on `guac-server`, `guac-extract`, `guac-formats`, `guac-ir`, `guac-script`, `guac-motion`, `skylander-core` and `skylander-rpcs3-control` (for the `PortalDriver` trait), Bevy 0.20 and Avian.
- **Lanes are worktrees, two compiling at most.** Each worktree has its own `target/` (14-45 GiB after AG.65); the Mac runs at most two compile lanes at `-j 4` since load 150+ hung it on 2026-10-19. A lane merges into `2.0` when it lands, and three of the six merges on 2026-10-21 conflicted in [`play.rs`](../../../crates/guacamole/src/play.rs), [`crates/guacamole/src/pad.rs`](../../../crates/guacamole/src/pad.rs), [`mirror.rs`](../../../crates/guacamole/src/mirror.rs), [`shots.rs`](../../../crates/guacamole/src/shots.rs) and [`hero_scripts.rs`](../../../crates/guacamole/src/hero_scripts.rs). The mirror is the shared file by construction: every subsystem's `/api/_test/tv` field (`focus`, `level`, `prepare`, `input`, `mode`, `hero`, `gait`) is built there.
- **The tick order is spread over five files.** `FixedUpdate` systems are added in [`hero.rs`](../../../crates/guacamole/src/hero.rs) (`HeroSystems::{Step, Gait, Camera}`, a step counter, the movers after the step), [`hero_scripts.rs`](../../../crates/guacamole/src/hero_scripts.rs) (`ScriptSystems::Step` before the hero's step, `Advance` between it and his gait), [`level.rs`](../../../crates/guacamole/src/level.rs) (the props stood before the step), [`scripts.rs`](../../../crates/guacamole/src/scripts.rs) and [`mirror.rs`](../../../crates/guacamole/src/mirror.rs) (the gait log after `HeroSystems::Gait`), each placing itself by its own before/after edge; inside the scripts' step the VM ticks placements in its own order. AH.59 found four places where that order puts a push a step late.
- **Ids collide at merges.** A lane takes the next free box id in its own tasks.md, so two lanes take the same one and the second merge renumbers: nine renumber commits from 2026-10-20 22:35 to 2026-10-21 18:42.
- **What the tests repeat.** `cargo test --workspace`: 2,106 passed and 196 ignored in 147 binaries, 32.1 s (AG.65). The guacamole e2e, 73 tests in 277 s: every spawn is a `cargo run` into the repo's `target/` ([`crates/e2e-tests/src/lib.rs`](../../../crates/e2e-tests/src/lib.rs)'s `start_server`; AG.78 is its cold first build), a synthetic extract built in the spawn's own temp dir (`IntroFixture::build_in` and its walking and fighting variants: the synthetic dump, roster, model, level and script part) and AE.25's start pass, 144 keys, 616-660 ms on a warm Metal cache and 2,406 ms cold (AG.43's runs); [`guacamole_intro.rs`](../../../crates/e2e-tests/tests/guacamole_intro.rs) also has ffmpeg encode `testsrc` and `sine` intros, three of them 30 s long, per test. The disc tests `giants_identity`, `giants_intros` and [`giants_transcode.rs`](../../../crates/guac-extract/tests/giants_transcode.rs) each make a throwaway extract under `GUAC_EXTRACT_DIR`, the last running the real ffmpeg on one real intro. guac-extract's own [`transcode.rs`](../../../crates/guac-extract/tests/transcode.rs) tests run a fake ffmpeg and encode nothing.
- **Stand-ins live in five kinds of place.** AG's design decision 2 table (15 rows) and AH's (20, plus Slides struck at AH.19), both bound for the design doc at AG.33 and AH.32; open boxes (AH.62's kept health); backlog lines (the swap's script reinstall, Titan feats, particles, combat sound, the HUD); and the code, where 61 lines in 27 files say "stand-in". No list says which box retires each, or what breaks if one is still in when its neighbour lands.

## Goals / Non-Goals

**Goals:**
- Boundaries chosen from measurements, and approved by chotchki before anything moves.
- Splits that land without a lane rebasing a moved file, and change nothing a test can see.
- Lanes that never take the same box id.
- Test runs that build each input once per content and still exercise everything they exercise today.
- Every stand-in with the box that retires it, and the dangerous ones retired before what they'd break.

**Non-Goals:**
- Behaviour, requirements or specs (`skip_specs: true`).
- Item 4, tests that couldn't fail: "not sure we can do anything on this." The reviews' mutants stay the check; the test survey records any it meets.
- Item 6's fix itself: it's AH.59's (decision 8).
- A Bevy, Avian or toolchain upgrade, a new linker, sccache or a shared target dir across lanes. The code survey measures the target dirs and may propose one; it isn't assumed.
- The product's behaviour or release (the product is frozen; a `guac-server` split, if approved, is moves only).

## Decisions

**1. The survey proposes the boundaries and chotchki approves them before any file moves.**
"Worthy of considering a split" is the bar: the 1,000-line mark puts a file on the survey's list, it doesn't split it. A file is proposed for a split when the survey shows a cost: lanes conflicting in it, boxes from several concerns editing it, a rebuild it forces that a boundary would avoid, or concerns mixed in it that a reader has to page past. A 1,000-line parser with its property tests at the foot, or a test file of one subject, can stay. The survey ends in a proposal (crate boundaries, module splits inside them, the order) and chotchki approves, redraws or turns it down (AQ.5) before any file moves. The split boxes below are what the survey is expected to propose; it may redraw them.

**2. A seam becomes a crate only where a measured trial split pays; otherwise it's a module split.**
- A crate buys parallel compilation, a rebuild that stops at the crate's dependents, test binaries that link less (a disc test of the level needn't link the TV's screens) and a boundary a lane can't reach through. It costs `pub(crate)` items turning `pub` across the boundary, `test-hooks` and `dev-tools` threaded through each new crate, test helpers shared across crates (a feature or a support crate), the orphan rule, and maybe little time back: Bevy's systems are generic, so much of the codegen may still land in the crate that builds the `App`.
- So the code survey measures before it proposes: `cargo build --timings` per crate, the rebuild after a one-line change in each hot file, and one trial split of one seam on a throwaway branch, measured the same way. A seam whose trial doesn't pay, or whose boundary too many items cross, becomes a module split.
- A crate boundary can't cut a cycle. The survey's module graph (the `use crate::` edges and the system-set ordering edges) names each cycle; a seam inside one stays a module, or the cycle is broken first in a box of its own.
- The mirror is the main cycle risk: it reads every subsystem. Each subsystem builds its own `/api/_test/tv` report beside its code and the mirror composes them, so the mirror can sit on top and a lane adding a field edits its subsystem, not the shared file.

**3. One split at a time, moves only, with nothing in flight on its files.**
- A split starts when no lane is editing the files it moves, and the session launches none on them until it merges; lanes launched after it start from it. Two splits in flight, or one beside a lane in its files, means rename-against-modify merges, the conflict this change exists to cut.
- A split's commit is moves plus the `mod`, `use`, visibility and crate-manifest lines a move needs, so `git diff --color-moved` and `git log --follow` read it as moves. A fix found while moving is its own commit after.
- Verified by: `.githooks/pre-push` green; `cargo test --workspace`'s passed and ignored counts the same before and after (tests move, none appear or vanish); the guacamole e2e at its count; the disc tests whose code moved rerun with their numbers unchanged; and the rebuild after a one-line change in each moved file against the survey's, which says whether the split paid.
- The level, hero and scripts-bridge splits stand on AH.59 (decision 8). Today their order is edges between those very files, so a system moved before AH.59 can land in another place in the step with nothing failing until a fight runs a step late, item 6's own failure. After AH.59 the order is one list in one module, and a moved system keeps its place.

**4. Per-lane id blocks** (item 3; [`openspec/config.yaml`](../../config.yaml), AQ.1).
- When lanes run at once the session hands each a block of new task ids per change it may add boxes to, written into its prompt at launch (AH.70-AH.79 to one, AH.80-AH.89 to the next), starting past the highest id `2.0` or any block still out holds. A lane numbers new boxes from its block only, in order; it never renumbers a box `2.0` or another lane holds, and one that fills its block lists further boxes unnumbered in its report for the session to number at the merge. The session's own boxes (a merge's findings) take ids past every block out. Unused ids stay gaps, as change ids do. A lone lane still takes the next free id.
- Rejected: (a) renumbering at the merge, today's way (Context): each one rewrites cross-references, while a commit subject or code comment that cited the old id keeps citing it; (b) a lane suffix (`AH.19a`), which breaks `<ID>.<N>` and reads as a split of AH.19; (c) one shared counter file, which lanes in separate worktrees can't see until they merge.

**5. Caches hold a test's inputs only, keyed by content.**
- What may be cached: an input a test needs and doesn't test (the synthetic intros a TV test plays, the synthetic extract a phone test starts from, the binary the harness spawns, an extract a read-only disc test reads). What may never be: the output of the code a test exists to exercise, so the transcode and extraction tests keep transcoding and extracting on every run; a cache serving their output would turn each into a check of the cache.
- The key is a hash over everything that decides the output: the builder's own source file hashed whole (any edit misses, over-invalidation being the safe direction), the tool's identity (ffmpeg's `-version` lines, the arguments, the lavfi source string) and the versions the output records (`IR_VERSION`, `LAYOUT_VERSION`, the transcode `TARGET`). Never a modification time or a file's presence.
- An entry is built in a temp sibling and renamed into place, so a killed run or two tests racing never leave half of one; readers treat it as read-only, and a test that writes into its extract copies the entry first or builds its own.
- Synthetic entries live under the target dir (`target/guac-test-cache/<kind>/<key>/`), gitignored and dropped by `cargo clean`, holding nothing from a disc. Anything from a disc stays outside the tree under `GUAC_EXTRACT_DIR` (the hard rule), keyed the way guac-extract already keys extract roots (`<TITLE_ID>-<APP_VER>-<hash8>` with each part's completion in the manifest), which makes the real extract a content-keyed cache already.
- Each cached fixture keeps one test that builds it fresh and checks it, so the builder can't rot behind its cache, and `GUAC_TEST_CACHE=off` builds everything fresh for a run that wants it (the phase exits).
- The start pass is the exception to weigh, not a cache to add: macOS already keeps compiled pipelines per user (AE.25), so a warm spawn's pass is 616-660 ms. Skipping it under a test-only switch would put a path in the binary that only tests take. The test survey prices it beside fewer spawns (several checks on one server where they can't interfere); the default is to keep it.

**6. Overtesting is two tests on one path to one assertion.**
Typical shapes: a headless test and an e2e checking the same TV behaviour; e2e tests that each spawn a server to reach the same state; disc tests re-deriving the same reads. A test is deleted or narrowed only when every mutant it fails on fails another test too: the survey writes the mutants, the box runs them and lists them. `cargo test --workspace`'s count then changes only by the removals named in that box.

**7. The stand-in ledger.**
- One table, in the design doc where AG.33 and AH.32 move AG's and AH's stand-in tables (whichever of the three lands first writes it, the others add to it), extending their columns with two: the box that retires it (a box id, or the backlog line until its rung is proposed; a rung's name alone isn't enough) and its hazard (what goes wrong if it's still in when a named box lands, and how it would show; or none, with why).
- Its rows: every row of both tables, every stand-in an open box or a backlog line names, and every code comment that calls something one. Each comment points at its row, so the ledger and the code can be checked against each other with a grep.
- Ordered by hazard: a hazardous row's retiring box goes ahead of the box it endangers, in that box's "on" list. Known so far: (1) the swap's script reinstall (`hero_scripts::reinstall`), which resets Setup_001 and the Chompies on every swap once AH.25 runs the fight on the TV; no box retires it today, so the ledger adds one to AH (the deactivate path: his file's placements and clones off the VM, the next hero joined into the same run) ahead of AH.25; (2) AH.62's kept health, which the reinstall would also reset, with it; (3) the walker a hero falls back to when his scripts aren't in the level's part or his join failed: he walks and can't attack, so a run that fell back passes every walking check, and the ledger names which runs assert his scripts are driving; (4) the host answering every point as on screen (AH.20), harmless while an orb's idle bounce is its only reader, hazardous once a reader gates play on it.

**8. Item 6 is AH.59, ahead of AH.25.**
AH.19's review called the hurt chain a step earlier and the pushes matched AH.10's frame for frame: the root cause is the order, not the pushes. AH.59 measures the original's per-frame order on the oracle once (which of the scheduler's three groups each placement and engine phase ticks in, and in what order, through a frame of the fight, beside the decompile and the June state's live lists) and encodes it as one explicit Bevy schedule of named sets, declared in one module, that every `FixedUpdate` system joins. AH.25 stands on it, so the first fight on the TV isn't tuned around a step-late order, and AQ's gameplay splits stand on it (decision 3). The box lives in [`ah-first-fight/tasks.md`](../ah-first-fight/tasks.md); AQ only orders against it.

## Risks / Trade-offs

- [A crate split doesn't pay: the codegen stays where the `App` is built] → the trial split's numbers decide before chotchki approves; a seam that doesn't pay stays a module split (decision 2).
- [Visibility widens: items go `pub` to cross a boundary] → the survey counts the crossings per seam, and one with many stays a module.
- [A move hides a change] → moves only, one commit per split, `--color-moved` (decision 3).
- [Lanes stall while a split runs] → the splits run one at a time between lanes, in the order chotchki approves; a lane wanting a file mid-split waits for its merge.
- [The test count drifts unnoticed] → the counts before and after each split and each narrowing are recorded in its box.
- [A cache serves stale output] → the key covers the builder's source and the tool's identity, one test builds fresh, `GUAC_TEST_CACHE=off` at phase exits (decision 5).
- [A cache eats the disk the target dirs already crowd] → one entry per key, the kind's older keys pruned when a new one is written, all of it gone with `cargo clean`; the sizes recorded in each cache box.
- [A lane fills its id block] → it lists further boxes unnumbered and the session numbers them at the merge (decision 4).
- [The ledger rots] → every code comment that says "stand-in" points at a row; the docs box checks it with a grep.
- [Features unify across new crates] → the product's release build turns off `guac-server`'s defaults so its features don't unify with guacamole's; each new crate declares its features explicitly, and a split touching `guac-server` builds the product's release command line.

## Migration Plan

Nothing persisted changes shape. The caches live under `target/` and are never read by `guacamole` itself. Each split is a revert away. AQ archives on `2.0` once AQ.0 holds; no release carries it.

## Open Questions

1. Which crate boundaries: answered at AQ.5 from the survey's numbers.
2. Whether `guac-server`'s [`http.rs`](../../../crates/guac-server/src/http.rs) and [`crates/guac-server/src/state.rs`](../../../crates/guac-server/src/state.rs) split before G1, since the frozen product links them: the survey's numbers, then chotchki's call.
3. Whether any e2e test skips the start pass (decision 5): kept unless the survey's price changes chotchki's mind.
