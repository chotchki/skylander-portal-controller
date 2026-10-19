# Phase AH - M3: the first fight, earned and kept on the kid's profile

## Why

M3 is the first fight: a kid's character attacks, takes hits, beats the enemies the original puts in its way and keeps the experience and gold it earns on that kid's profile, which the phone then shows and edits. It stands on AG (a hero walking chapter 1 on the TV) and AF (an oracle that can be driven frame-locked, its drift measured). The 2026-10-19 research closed where the first fight is, what it computes and how much of it is script, so the rung can be planned from evidence; the synthesis is [`ah-propose/BRIEF.md`](/Users/chotchki/workspace/guacamole-research/ah-propose/BRIEF.md), standing on [`ah-combat`](/Users/chotchki/workspace/guacamole-research/ah-combat/FINDINGS.md), [`ah-progression`](/Users/chotchki/workspace/guacamole-research/ah-progression/FINDINGS.md), [`aj-vm`](/Users/chotchki/workspace/guacamole-research/aj-vm/FINDINGS.md) and [`aj-script`](/Users/chotchki/workspace/guacamole-research/aj-script/FINDINGS.md):

- **The original's first fight is four Chompies bullying Norticus in a pit AG's walk can't reach.** They stand on the pit floor 70-75 m across the ground from the spawn and 21 m below it, past the ledge route's barrels and pot, the upper walkway, a 9 m drop and three push blocks ([`reach3.txt`](/Users/chotchki/workspace/guacamole-research/ah-propose/out/reach3.txt)). AG's 703 m² clearing holds no enemy and no experience; its gold is probably in the breakables (the level's drop rates read as the loot defaults, a hypothesis until the oracle breaks one).
- **Every combat rule is a script body.** The hurt chain, the Chompy's AI, Eruptor's attacks, experience orbs, loot, breakables, defeat and the figure's saves are tfbScript; the native share is comparisons, `random(N)` and movement. chotchki's answer to AG's Open question 2 (`2.0`'s `f772507`, after this branch's base): stand-ins kept thin, and "one that would need a script body's logic rewritten, or that grows past a box of its own, stops there and AJ's VM slice is pulled forward instead". So AH carries the VM's core and the engine members the fight reaches: statically at most 193 of the 414 the disc uses for Chompy and Eruptor together, every branch counted ([`reach_members.txt`](/Users/chotchki/workspace/guacamole-research/ah-propose/out/reach_members.txt)), about 60 of them sprites, particles, sounds, lights and strings. Moving AJ's core into AH is a ladder change, so it's Open question 1.
- **The numbers are small and known.** At medium, a chapter 1 Chompy has 1 health, bites for 11-14 and gives 4 experience as four 1-point orbs and no gold; Eruptor at level 1 has 290 health, and Lava Lob and each Eruption tick do 8. Level 2 is 1,000 experience, so no kid levels up in this fight.
- **The oracle's anchor plays medium** (AF.8's state reads the medium numbers live, [`live_values_l213_af8.tsv`](/Users/chotchki/workspace/guacamole-research/ah-propose/out/live_values_l213_af8.tsv)), and AF left the event and totals floors for the first rung that observes events: this one.

## What Changes

- **The fight is the original's**: the four Chompies around Norticus on the pit floor below the walkway, reached the original's way from the spawn. Three slices, each standing on less than the next: the phone keeps and edits a figure's progress; Eruptor smashes the clearing's breakables for their gold, which opens the ledge route; the fight, first through a dev-only entry at the pit (`test-hooks` and `dev-tools` builds) for chotchki's first look and the oracle comparison, then by the original's route (Open question 2).
- **The original scripts run**: AH pulls the tfbScript VM's core forward from AJ, a ladder change chotchki confirms (Open question 1): a new pure crate interpreting the op graphs `guac-formats` reads from the user's disc, a whitelist of the scripts the fight runs (the level's and the global init, the hero's, the four Chompies and their encounter's controller, Norticus where the fight reaches him, the breakables and coins in reach, and what they spawn), the engine members and world operations they reach written as they're reached and counted against the 193 ceiling, cosmetic members as plain values nothing draws, and one thin native rule for the portal manager's per-player loop. AH writes the random generator down (MT19937 with its 1998 seeding in the design, the original's `random(N)` ranges as a spec), the job the design doc gave AJ.
- **The figure store**: one row per profile per figure in guacamole's own database (experience as a total, gold, playtime), fresh on a profile's first placement, written the moment anything changes, one writer behind the server; the figure's data reaches the scripts as a buffer in the layout the disc's own save-param table describes, never a file.
- **The phone's figure editor moves onto the rows**: guacamole's arms of the stats, edit and reset routes and the portal slot's reset, a check that the path's profile is the phone's unlocked one, a level cap of 15 under guacamole and live updates to every phone.
- **What the kids see on the TV**: the hero's health, level, experience and gold in a thin stand-in HUD in AP's theme, the original's defeat (the figure out for the level, nothing lost) with a one-button restart screen when no character is left, and the portal cards gaining level and gold.
- **The oracle grows**: readers for script variables, the figure's experience and gold, live enemy stats, difficulty, the XP multiplier and the element area; O6, a game-agnostic recorder for actors spawned mid-stretch (projectiles, orbs, coins); a route driver that walks the oracle to savestate anchors at the ledge route and the pit; and the event and totals floors AF left unset, calibrated by AF.12's method.
- **The extract grows by parts**, AG.15's per-part rule: every animation palette slot of the fighting characters and the creatures, the attack, orb and coin models the disc has, the progression tables and the whitelist's script data, with no layout bump.
- Out: upgrades, Magma Ball and Wow Pow (AK), Titan feats (the boulders and their emeralds, a backlog line), the pit's other encounters and the rest of the level's scripts (AJ), particles and combat sound (backlog lines), the original's HUD and menus (AM), co-op (AL), PvP (AI), Windows (the bring-up line).

## Capabilities

### New Capabilities

- `guacamole/figure-progress`: a figure's progress per profile in guacamole's own store: fresh on first placement, earned in play, levels by the original's table, shown, edited and reset on the phone.
- `guacamole/combat`: how a hit lands for heroes and creatures alike: damage and criticals, armour deflects, no grace after a hit, stun and knockback, health, defeat.
- `guacamole/hero-attacks`: what each hero's attacks do under the buttons (Eruptor's two first) and which characters attack.
- `guacamole/creatures`: the creatures a level's scripts move and fight with, chapter 1's first fight first: waking, warning each other, closing in, biting and dying.
- `guacamole/pickups`: experience orbs and coins, how a hero collects them, what each adds and levelling up.
- `guacamole/breakables`: what breaks under attacks, what it drops and leaves, the ways it opens, and what stays put for a Giant's feat.
- `guacamole/tv-hud`: the hero's numbers over the level and the screen when no character is left, guacamole's own until the original's interface runs.
- `guacamole/script-runtime`: the game's own computed values and random rolls coming out as the original's, and the difficulty played.

### Modified Capabilities

- `guacamole/figure-cards` (AA's requirements, modified by AE and AG, all unarchived): "Each figure on the portal shows as a card" (AG's text) loses its "no level, experience or gold" and one ADDED requirement puts the placing profile's level and gold on the card; "Figure progress is not shown or edited yet" is REMOVED, its place taken by figure-progress.
- `guacamole/pad-input` (AG's new capability, unarchived): one ADDED requirement, the attack buttons.

The figure-cards and pad-input deltas sit on unarchived changes, so AH archives after AA, AE and AG, its MODIFIED block rebuilt from their archived text first (design, Migration Plan).

## Impact

- Track: Guacamole, rung AH, milestone M3. No change-wide gate: each box names what it stands on, AG's boxes by id (tasks). The store and the phone's editor (AH.1-AH.3) stand on AA alone; the oracle work (AH.4-AH.9) on AF and AG.1 and AG.7, every trace's use on AG.34 and AG.35; the VM (AH.10-AH.13) on AG.15, AG.16, AG.25 and AG.30; the fight on the TV on AG.19 and AG.21; the route on AG.10, AG.28 and AG.36.
- Disc serial read: BLUS30968 01.00 only: `level/level_213_tutotial.bld`, `permanent/global.bld`, `character/init_setup.bld`, `character/009_eruptor.bld` and, for who fights, every character archive AE already reads. Tests that read it are `#[ignore]`d and take `GUAC_GIANTS_DUMP`; oracle lanes use AF's keys and write only under `GUAC_CAPTURE_DIR`.
- guac-extract's output changes by new parts under AG.15's per-part rule (the progression tables, the script data, every palette slot of the fighting characters and creatures, the attack, orb and coin models), so an extract resumes for them and the layout stays at 4; `IR_VERSION` stays 3 unless a record AH touches changes shape, which AH.12 records (a bump re-extracts once).
- Kid-visible milestone: M3, on the Mac's own display (the TV pass takes the couch): place Eruptor, smash the clearing for gold, walk the original's route down to the pit, beat the four Chompies, and see the experience and gold on the TV and on the phone, kept on the kid's own profile; edit a figure's level and gold from the phone.
- Code:
  - `crates/guac-formats`: the 44 op classes' layouts and the metaobject-index resolution, the progression readers, each with a property test and a fuzz target;
  - new crate `crates/guac-script` (pure, no Bevy): the interpreter, typed member cells, the generator, the whitelist;
  - `crates/guac-ir`, `crates/guac-extract`: the new parts;
  - `crates/guac-motion`: AG's controller driven by a destination for creatures;
  - `crates/guacamole`: the members and world ops, the fight's actors, the HUD and defeat screen, the dev entry, the channel carrying progress to the store; `/api/_test/tv` gains health, experience and gold under `test-hooks`;
  - `crates/guac-server`: the `figure_instances` migration in guacamole's own set, the store's queries, guacamole's arms of the figure routes and the slot reset;
  - `crates/rpcs3-control`: the native placement carrying the instance's progress (the product's drivers never build one);
  - `phone/`: the level cap and the Unavailable state under guacamole; the product's path unchanged;
  - `rpcs3-patches/oracle/`: O6, dev-only behind `GUAC_ORACLE=1`; `tools/oracle-harness` and `crates/guac-oracle`: the readers, O6's codec, fake and loopback, the route driver, the event and totals floors.
- Dependencies: none new.
- Shared with the frozen product: guac-server's routes, the phone and the placement type. guacamole's arms key on the `FigureFiles` hook's absence, the new table lives only in guacamole's migration set, and AH.0 runs the product's phone e2e at no fewer passes than AH's base.
- Owning doc: [`docs/native/guacamole-design.md`](../../../docs/native/guacamole-design.md) (Persistence and setup, Figures, Method, Giants in detail, The oracle, The extractor, Risks, Milestone ladder); [`CLAUDE.md`](../../../CLAUDE.md)'s Architecture and Data & paths for the new crate and table; [`rpcs3-patches/README.md`](../../../rpcs3-patches/README.md) for O6.
- Release reach: none. guacamole runs from `cargo` on the Mac and no `v*` release carries it yet.
- One-way doors: none outside guacamole's own dev database, which takes one migration. The extract is a cache and the oracle series never enters a product build.
- Backlog: the AH line is promoted and deleted; the docs box adds Titan feats, particles, combat sound and the original's HUD, and rewrites AJ's line once Open question 1 is answered.
