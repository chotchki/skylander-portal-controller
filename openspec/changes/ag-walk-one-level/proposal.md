# Phase AG - M2: the figure on the portal walks the opening of chapter 1

## Why

M2 is the first gameplay: put a character on the phone and walk it with a pad through a real Giants level on the TV, moving, turning, sliding and falling the way the original does. It stands on AE (the model, its material and the remaster) and on AF (an oracle that can be driven frame-locked, with its drift from itself measured). The 2026-10-18 research closed what a level is on the disc and how the hero moves, so the rung can be planned from evidence rather than guesses; the synthesis is [`ag-propose/BRIEF.md`](/Users/chotchki/workspace/guacamole-research/ag-propose/BRIEF.md), standing on [`ag-levels`](/Users/chotchki/workspace/guacamole-research/ag-levels/FINDINGS.md) and [`ag-motion`](/Users/chotchki/workspace/guacamole-research/ag-motion/FINDINGS.md):

- A level is one IGZ, its static geometry already in world space (16,912 segments over the disc's 75 scenes, 0 decode errors), its collision ONE explicit triangle mesh per world (68 bodies, 140,603 triangles) and everything else a placement. No trigger, camera-path or navmesh type exists on the disc: that's all script.
- Havok is query-only. The tutorial's live `hkpWorld` holds no dynamic body, and the hero moves by Toys for Bob's own kinematic velocity model over their fork of Havok's character proxy. So AG needs shape casts, contact planes and rays, not a solver.
- The camera, the hero's runtime speed and turn rate, his locomotion clips, the spawn, fog and death falls are tfbScript, and the VM is AJ, three rungs up. AG writes stand-ins for each from oracle observation, as specs AJ's VM must later pass.
- AF.11 (on `af-divergence`, `dd571bf`) says what "feel checked against the oracle" can't mean: trajectory equality. A still hero and camera are bit-exact for 7,200 flips, but a walking hero parts within 60, the gate's corner splits replays into two outcomes 1.24 m apart, and pausing to observe moves the run.

## What Changes

- **The level is chapter 1, `level/level_213_tutotial.bld` ("Time of the Giants"), from its spawn.** Every oracle state so far stands there (AF's fresh state idles Eruptor at the spawn) and AH's first fight is in it. With no scripts running, the static collision joined to the spawn is about 1,660 m², 3,660 m² with one-way drops, 7-15 % of the level's walkable collision, closed off by a breakable door and a rubble wall that are AH's and AJ's. So M2 honestly reads "walk the opening area of Time of the Giants", not the whole level (Open question 3).
- **Who walks**: Eruptor, checked end to end against the oracle; every character with an AE model walks on its own tuning, read statically and checked on Eruptor and Tree Rex (Open question 1).
- **Physics: queries only, our controller.** Avian behind a thin seam (chotchki: "probably avian if I had to guess"): avian3d 0.8 from crates.io, or `main` at `1070759b2145` ("Update to Bevy 0.20 (#1084)") pinned by rev until 0.8 publishes. The design doc's pin, avianphysics/avian#1070 at `34c9b008d2f4`, is stale: #1070 closed unmerged on 2026-10-18 and #1084, built on it, merged to `main` instead. The hero is moved by a controller written from the hero-movement spec, faithful to the original's loop, on Avian's spatial queries.
- **Script stand-ins until AJ**: static readers of the data the scripts pass as macro arguments (camera definitions, volumes and transition planes, tuning links, locomotion states, fog zones) plus native logic written from oracle observation. Each is a requirement in observable terms, listed in one place so AJ's proposal can count what it deletes. Reading macro arguments and replaying their bodies is the narrow rewrite the Method rules out without chotchki's yes, as AE's toy table was ("that's fine"), and this one is several times its size (Open question 2).
- **A per-frame oracle recorder that doesn't pause**: O5, a game-agnostic `WATCH` verb in AF's dev-only O-series, copying address ranges the harness names into a ring once per flip at the guest's own pad read. It feeds the model replay (each frame's measured dt), the movement and camera specs and AH after.
- **Feel checked in four layers, frames not one of them** (remaster only: "parity" means gameplay, never pixels): exact rest poses and collision; a model replay of one oracle run's dt and stick; envelope metrics (top speed, ramp, stop distance, reversal, turn rate, wall slide, camera trail) at AF.12's floors; outcomes at a branch drawn from the oracle's set.
- **Time**: logic in `FixedUpdate` at 1/60 s with interpolated transforms. The original integrates its measured frame time clamped to [1/120, 1/20] s, so "the original tick" in the design doc is a nominal 1/60 s and its long frames are hitches, not design; every comparison keys on game time.
- **The level on the TV in the remaster's look**: AE's material on level 213's 857 tfbmulti2 instances plus the gloss term one of them needs, fog, point lights and shadows from Bevy's cascades of the level's key light; stand-ins for grass (45 instances), warp (4) and an unidentified effect (6); the sky as authored; every material compiled before the level first shows.
- **Pad input**: buttons by position, the original's stick dead zone, the stick read against the camera, Start pausing; prompts by the label printed on the pad in hand, with a position glyph for a pad guacamole can't identify. The Mac half of the backlog's controller hardware checks comes in with it.
- **Portal and level**: the first character placed takes the TV into the level after its intro (Open question 4), a second waits, removing the hero pauses under a "place a Skylander" overlay, and the level replaces the cards on the TV. The backlog's "Character intros in gameplay" line is promoted here: in the two cases measured so far the original played no intro in a level, which the intros delta carries now; AG.4 measures a figure's first-ever placement and the rest.
- **The extract grows**: IR v3 and layout v3 add a level stage after AE's models (world-space segments on AE's records, textures into AE's store, collision, placements, lights, sky, the stand-ins' script data).
- Out: enemies, attacks, NPCs and other scripted creatures, the intro cinematic, the HUD, tutorial prompts, breakables, Titan feats, music and effects (AH, AJ, AM); co-op (AL); Windows (the bring-up line).

## Capabilities

### New Capabilities

- `guacamole/level-scene`: the level guacamole plays and how it gets there: chapter 1's opening off the user's disc, what's in it at the start, what's solid, its look, never stalling, and how the portal takes the TV into it and keeps it there.
- `guacamole/hero-movement`: how a hero moves under the stick, as the oracle shows the original: camera-relative walking, top speed, speeding up and stopping, turning and hard reversals, walls, slopes, steps and ledges, falling, no jumping, his locomotion animation and frame-rate independence.
- `guacamole/gameplay-camera`: where the camera starts, how it follows, how the level's camera zones switch it and that nothing pushes it in.
- `guacamole/pad-input`: buttons by position, the move stick's dead zone, prompts that name the button printed on the pad in hand, and Start's pause.

### Modified Capabilities

- `guacamole/character-intros` (archived base in [`openspec/specs/guacamole/`](../../specs/guacamole/character-intros/spec.md)): "Placing a figure plays its character's intro" gets the level case above; AG.4 rewrites it from the full measurement.
- `guacamole/disc-extraction` (archived base): one ADDED requirement, levels extracted after the figures. ADDED, not MODIFIED, since AE's delta already modifies "Extraction runs once and survives interruption".
- `guacamole/figure-cards` (AE's MODIFIED version of AA's requirement, both unarchived): "Each figure on the portal shows as a card" holds on the portal view, not over a level.
- `guacamole/character-shading` (AE's new capability, unarchived): one ADDED requirement, the gloss map.

The figure-cards and character-shading deltas sit on unarchived changes, so `openspec validate` passes now and AG archives only after AA and AE (design, Migration Plan).

## Impact

- Track: Guacamole, rung AG, milestone M2. Starts once AE's model and material path (AE.5 the model census, AE.6 the model stage, AE.7 the reference model, AE.8 the WESL shader and material) and AF.11-AF.13 (the divergence measurement, its calibrated floors, AF's docs) have landed on `2.0`; AG.12, the level on the TV, also waits for AE.12 (the remaster's decode and HDR camera) and AE.14 (the pre-warm). Inside, each box names what it stands on (tasks).
- Disc serial read: BLUS30968 01.00 only: `level/level_213_tutotial.bld`, `permanent/global.bld` (the camera system and effect names), `character/init_setup.bld` (boot tuning) and the character `.bld` archives AE already reads (tuning and locomotion states). Tests that read it are `#[ignore]`d and take `GUAC_GIANTS_DUMP`; oracle lanes use AF's keys and write only under `GUAC_CAPTURE_DIR`.
- guac-extract's output changes: IR v3 and `LAYOUT_VERSION` 3, so every extract re-extracts once (AB's manifest rule). Time and size get measured (AG.9).
- Kid-visible milestone: M2. On the Mac's own display (the TV pass in [`openspec/backlog.md`](../../backlog.md) takes the couch): place a character, its intro plays, then it stands at chapter 1's spawn and an 8BitDo pad walks it around the opening area with the original's speed, turns, walls, ledges and camera.
- Code:
  - `rpcs3-patches/oracle/`: O5 (`WATCH`), dev-only and behind `GUAC_ORACLE=1` like O1-O4; `tools/oracle-harness` and `crates/guac-oracle`: its codec, fake server, loopback and the per-frame trace;
  - `crates/guac-formats`: the collision, scene-graph, placement and light layouts and the script-data readers, each with a property test and a fuzz target;
  - `crates/guac-ir`, `crates/guac-extract`: IR v3, the level stage;
  - new crate `crates/guac-motion` (pure, no Bevy): the controller, the physics seam and its parry3d implementation;
  - `crates/guacamole`: the Avian seam, the level scene, the camera, the level input layer and prompt resolver, the portal-to-level flow, the locomotion and fall stand-ins; `/api/_test/tv` gains the mode and the hero's position under `test-hooks`;
  - `crates/guac-server`: only if AG.4 finds intros tracked per save (a per-profile "seen" table in guacamole's own migrations);
  - `crates/e2e-tests`: a synthetic level in the guacamole fixture, built in code.
- Dependencies: avian3d (with parry3d 0.31 and `bevy_transform_interpolation` 0.6 under it); parry3d directly in `guac-motion`. The pre-push cost is measured cold and warm as AA.10 did for Bevy (AG.3).
- Owning doc: [`docs/native/guacamole-design.md`](../../../docs/native/guacamole-design.md) (Decisions' physics line, The oracle, Renderer's frame rate, Controllers, Giants in detail, Risks, Milestone ladder); [`CLAUDE.md`](../../../CLAUDE.md)'s Guacamole and Architecture sections for the new crate and modules; [`rpcs3-patches/README.md`](../../../rpcs3-patches/README.md) for O5.
- Release reach: none. guacamole runs from `cargo` on the Mac and no `v*` release carries it yet; the product neither links the new crate nor changes behavior (AG.0 runs its e2e to prove it).
- One-way doors: none. The extract is a cache, the oracle series never enters a product build, and a per-profile intro table would touch only guacamole's own dev database.
