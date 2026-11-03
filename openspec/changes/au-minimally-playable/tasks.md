# Tasks

## AU. Giants minimally playable

AU ranks boxes where they live (design decision 1). The boxes it adds are for what no other change holds. Each box names what it stands on and runs once that has landed (design decision 5 has the order). Disc tests are `#[ignore]`d and read `GUAC_GIANTS_DUMP`. A lane's TV runs use its own APFS clone of the dev extract, never chotchki's.

- [ ] AU.0 Phase exit:
  - every box below ticked, split or struck through with its reason;
  - the boxes AU ranks in other changes ticked: AH.29 (the sweep), AS.38 (the save system on the host), AS.46 and AS.47 (the hub), AS.53 (the end-of-level sequencing), and AS.96, AS.97, AS.98, AS.104 and AS.76 (level 214 to its end);
  - [`.githooks/pre-push`](../../../.githooks/pre-push) green (fmt, check, clippy -D warnings, test --workspace);
  - the guacamole e2e and guacamole's disc suites green on the tip;
  - AU.7's playthrough done by chotchki on a release build.
- [ ] AU.1 The scripted heroes' attack buttons, on AH.24 and AG.27: every character that joins on its own scripts has the walking pad's attack buttons reach them as Eruptor's do (the attacks held until a game step reads them, by position as AH.24 has it). A character whose attack script stops in play falls back to the controller and walks on, as AH.151 has it.
  - Unit tests: a walker's press reaching his scripts' attack read; a stopped walker still walking on the stick.
  - Verify: headless, every modelled character pressing each attack, each listed as attacking or fallen back with the member it stopped on (the input AH.29's sweep reads); `giants_walkers` green; on the Mac, one Giant and one non-Giant pressing each attack beside a Chompy, with `run.stopped` naming any stop.
- [ ] AU.2 Every modelled figure fights, on AU.1 and AH.29's sweep: the members the sweep names, modelled largest reach first (most characters unblocked), until every character the roster models attacks on its own scripts. Split by the sweep's member groups as its table says.
  - AH's `guacamole/hero-attacks` delta, "Which characters attack", is edited in place to say every modelled character attacks with its own scripts (design decision 3). M3's Eruptor and Tree Rex checks stand.
  - Unit tests per member, on the host's or the VM's tests as each lives.
  - Verify: the sweep run again listing every modelled character attacking; on the Mac, one figure of each element striking a Chompy in chapter 1's first fight, and a Chompy beaten by each.
- [ ] AU.3 The story save store (design decision 4), on nothing in flight: a migration of guac-server's own (one save per profile per game, the save's layout version with it), the host's save alone written, a save of another layout kept and ignored. The values the save holds are named as the save scripts name them (AS.38 reads which).
  - Unit tests: a save written and read back; a co-op save leaving the second profile's untouched; a save of another layout kept, logged and ignored, with a new save kept beside it.
  - Verify: `cargo test -p guac-server` with the migration on a database from before it (the table gained empty, nothing else changed); the e2e green.
- [ ] AU.4 A later session resumes from the save, on AU.3 and AS.38: a profile with a save starts where the original's load of that save puts it (the hub, with its chapters done); a profile without one starts at chapter 1.
  - Verify: headless, a save made at chapter 1's end and loaded on a fresh run going where the original's load goes; on the Mac, a profile that finished chapter 1 started again resuming there, and a fresh profile starting at chapter 1.
- [ ] AU.5 The level's completion screen on the TV (design decision 2: chotchki, 2026-11-02, "we'll want to draw the level end using the game's art but bevy's ui"; AT.61 struck for it), on AS.53.
  - **The screen:** once the end-of-level sequencing runs, its completion screen (the objectives screen, `misc/ui_objectivesscreen.bld`: `ObjectivesScreen_Streaming.ai` creates it, `ObjectivesScreen.ai` shows the level's objectives and raises `level end seq` to 3 once the player continues) is laid out as a TV screen on AP's stack. It uses the objectives screen's own textures and fonts off the extract (an extract part if none holds them, resumed by an older extract as AS's are), fed by what the sequencing computes on the VM. Its continue takes the pads as the original's does.
  - **Without the art:** an extract without the art still shows the results in the TV's own style, named in `/api/_test/tv`.
  - **Unit tests:** the screen built from a synthetic level's results; continue raising the sequencing on.
  - **Verify:** on the Mac, chapter 1's end showing the completion screen after the outro and before FMV02, a shot of it for chotchki; level 214's end the same.
- [ ] AU.6 The next chapter from the hub, on AS.46, AS.47 and AU.5: where 214's level end goes (the hub or a chapter, design Open Question), the hero there whole, and the way the original's hub offers into the next chapter taken (a walk, a door, or a map screen following design decision 2).
  - Unit tests on any logic of ours.
  - Verify: headless, 214's end reaching the hub and the next chapter's entry; on the Mac, from 214's end through the hub into the next chapter, extracted on its first entry, with `run.stopped` empty.
- [ ] AU.7 chotchki's playthrough, on every box above: a release build with the real dump, a pad and the phone.
  - A fresh profile from chapter 1 through level 214 into the hub and the next chapter.
  - Figures off the roster beyond Eruptor fighting.
  - The game quit at the hub, then resumed from the save in a new session.
  - What he finds becomes boxes.
- [ ] AU.8 Docs:
  - [`docs/native/guacamole-design.md`](../../../docs/native/guacamole-design.md): AU beside the ladder, decisions 2 and 4 in its decisions, and the story saves in its store section;
  - [CLAUDE.md](../../../CLAUDE.md)'s Guacamole section: story saves as they land, every figure fighting, the level end and the hub.
  - No release: no `v*` tag ships guacamole before G1.
- [ ] AU.15 (found at AU.4, 2026-11-03, the au-saves lane) A quit saves as the pause menu's Quit Game does: the phone's quit takes the level down with no save, where the original's `Pause Menu - Quit Game` asks `Save - Save Game` for the slot and the header with the percent complete. What the level set in the save's memory since its last save is lost: chapter 1's adventure done, between its outro's `Game - Level End` and level 214's start (the original saves it there too, but quits only from the pause menu); an objective or an area found. Before the TV takes a level down for a quit, the quit's save asked as the pause menu asks it and the store's write waited for, as the figures' progress is flushed (`ProgressSink::flush_and_wait`). Unit test: a quit asking the save and waiting for the write. Verify: on the Mac, chapter 1's end by the dev entry's `end` anchor, the game quit during FMV02, the next session resuming at level 214.
