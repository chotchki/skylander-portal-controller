# Design

## Context

See [proposal.md](proposal.md) (Why) for the three gaps. The state each part starts from:

- **Fighting.** Every modelled character joins by the portal entry and walks on its own scripts (AG.27). Eruptor alone has his attack buttons reach his scripts ([`hero_scripts.rs`](../../../crates/guacamole/src/hero_scripts.rs), `ScriptedHero`). A character whose script stops in play falls back to the controller (AH.151), so a stop costs a figure its attacks, never the session. AH.29 holds the sweep's method (every modelled character headless, its attack scripts in strict mode on a synthetic target), unrun since chotchki's "eruptor is fine" for M3.
- **Saves.** guacamole's store keeps profiles, figure instances and settings ([`crates/guac-server/migrations/`](../../../crates/guac-server/migrations/)), with no story save. AS.38 names what the scripts ask: `system.save state`, which `global.bld`'s checks manager and save scripts set and poll.
- **Level end.** AS.53's notes: the TV's game systems' tick runs only the player loop's update macros, so `System - End of Level Sequencing` never ticks. Listed, it stops on the completion screen's backdrop members (`viewportinfo.filter effect` and its intensities), and the stream loader the objectives screen needs never runs. AT.61, as filed, would draw the objectives screen's own sprite tree through AT's adapter.
- **Hub.** AS.13 loads the hub's stages by name. Hub stage 1 draws no sky (AS.46) and its start stands where `InitializeLevel.ai`'s placement does, with no path (AS.47).
- **214.** It plays to Setup_004's yard. The interiors lane (AS.96) and the cutscenes lane (AS.82, AS.81, AS.78) are running.

## Goals / Non-Goals

**Goals:**
- Each part lands as lanes the Mac's two-lane cap allows. Measure first where the size is unknown (the sweep), then build by reach.
- A kid's session never ends on a gap. A figure whose attacks stop walks on, an unread save starts fresh, and a level end the TV can't sequence still takes him on as AS.15's stand-in does today.

**Non-Goals:**
- Fidelity past striking a target, for any character but Eruptor and Tree Rex (AH's requirement already says so). Each character's damage and timing against the original is later work.
- The save's layout on a PS3. guacamole keeps its own store and reads no PS3 save data.
- The rest of the hub's menus (upgrades, the bestiary, the trophy room), beyond reaching a chapter.

## Decisions

1. **AU ranks boxes where they live and adds boxes only where a part has none.**
   - **Where they stay:** AH.29, AS.38, AS.46, AS.47, AS.53, AS.76 and AS.96-AS.98 stay in their changes. AU.0's exit names them, and AU's own boxes cover what no change holds: every figure attacking past the sweep, the save store and the resume, the level-end screen on chotchki's word (AT.61 struck for it), the hub's chapter pick and the playthrough.
   - **Alternative, moving the boxes into AU:** renumbers what commits and code comments cite. The repo never renumbers a box ([`openspec/config.yaml`](../../config.yaml)), so this is rejected.
2. **The level end is laid out in Bevy's UI with the disc's own art** (chotchki, 2026-11-02: "we'll want to draw the level end using the game's art but bevy's ui").
   - **How it works:** the screen is a TV screen on AP's stack (a `bevy_state` screen retained between updates, `bevy_ui_widgets` buttons behind the input router), drawn with the objectives screen's own textures and fonts off the extract.
   - **What drives it:** the values the original's sequencing computes on the VM: the level's objectives, its collectibles and the `level end seq` state. The sequencing's other halves still run on the VM: the fanfare, the movie the level end queues and the level switch.
   - **Rejected, AT.61's adapter:** drawing the objectives screen's script-built sprite tree, which would need the stream loader, the backdrop's filter members and the screen's whole interface script before anything showed. AU.5 is this decision's box, and AT.61 is struck for it with what it found carried over.
   - **Rejected, a screen of our own with our art:** chotchki asked for the game's.
   - **Reach:** if the hub's chapter pick needs a menu screen, it follows this decision unless he says otherwise.
3. **Every figure fights: the sweep, then members by reach.**
   - **The sweep:** AH.29's sweep runs as written, headless on the dump, and records each modelled character as attacking, walking only or stopped on a named member, with the members each needs.
   - **The members:** modelled in guac-script or guacamole's host, most characters unblocked first.
   - **The spec:** AH's in-flight `guacamole/hero-attacks` delta, "Which characters attack", is edited in place to read that every modelled character attacks with its own scripts. AH is still a change, so its delta is the requirement's one home, and no second delta in AU says the opposite until both archive.
   - **The button routing:** a walker's buttons reach his scripts as Eruptor's do. One bit of routing, `HeroActions` for every scripted hero, comes first and is pinned by a test that a stopped character still walks.
4. **Story saves are the original's save system run on the host, against rows of guacamole's own.**
   - **What the host does:** the save scripts run on the VM. The host answers `system.save state` as AS.38 says, and keeps what the scripts save (the story's variables, the chapters done, the collectibles) as named values in one row per profile per game.
   - **The store:** a migration of guac-server's own, so a database from before it gains the table empty. The host's row alone advances.
   - **Resuming:** loading a save puts a kid where the original's load does: the title's load goes to chapter 1 unless it's done, then level 214 unless it's done, then the hub's stage one past the hub's unlocking adventures done (levels 210, 202 and 211), not straight to the hub (AU.4's reading of the title's scripts).
   - **Versioning:** a row from a build whose save layout differs is kept, logged and ignored, never deleted. That profile starts fresh, so a pre-G1 change to what's saved can't crash a start or lose an older save.
   - **Rejected, a PS3 save file's layout:** guacamole reads no RPCS3 data and keeps one store.
5. **The order.**
   - **The two lanes running now** already serve AU: interiors (AS.96, 214) and the cutscenes (AS.82, AS.81, AS.78, which the level flow's movies and scenes stand on).
   - **When a lane frees:**
     1. AH.29's sweep, a measurement lane;
     2. AS.38 and the resume;
     3. the members the sweep ranks, then AU.1's routing;
     4. the level end and the hub, together, since the hub is where the level end goes;
     5. 214's rest, between them where a lane has room.

## Risks / Trade-offs

- **[The sweep finds a long tail of members]** → Rank by how many characters each unblocks and land in that order. A character still short walks with his card, as today. AU.0 asks for every modelled character attacking, and the tail is visible in the sweep's table.
- **[What's saved changes as more of the save system is read]** → Versioned rows (decision 4). An older save is ignored with a log line, never deleted, so a rebuild can read it again once the layout settles.
- **[The level end's art isn't in any part the extract holds]** → An extract part for it, resumed by an older extract as AS's parts are. Until it lands, the screen draws with the TV's own title style and says so in `/api/_test/tv`.
- **[The hub's chapter pick needs systems AT hasn't built]** → It follows decision 2, a screen of our own over the original's state. The original's own map scripts drive which chapters are open.
- **[Two lanes edit [`scripts.rs`](../../../crates/guacamole/src/scripts.rs) and the docs at once]** → As now: each lane merges 2.0 when the other lands, and the reviewer runs the merged tree's gates.

## Migration Plan

- The save table is a new migration in [`crates/guac-server/migrations/`](../../../crates/guac-server/migrations/). A database from before it gains the table empty; nothing is backfilled, and no product database is touched.
- An extract part for the level end's art, if needed, resumes on an older extract the way AS's parts do.
- Rollback is the previous build. An older build ignores the save table it doesn't know.

## Open Questions

- Which chapter the hub offers first after 214, and whether 214's own level end names the hub or a chapter. Read off 214's level end and the hub's scripts when the level-flow lane starts. Either answer leaves the boxes as they are. Answered at AU.6 (2026-11-03): 214's level end names the hub (`999_Level_Village`, stage 1 after 214), and the hub offers level 201 first (Flynn's, once Cali's intro has run).
