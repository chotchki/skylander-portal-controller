# Phase AU - Giants minimally playable

## Why

Chapter 1 plays start to finish on the TV now (its fights, its outro, FMV02) and level 214 plays about half way, so chotchki called it on 2026-11-02: "I feel like we're getting close to a minimally playable game". Three gaps still stop a kid in the first session, and he agreed they're what minimal means ("yes that's what I mean, co-op isn't part of it"):

- **Only Eruptor fights.** Every other figure joins and walks on its own scripts, its attack buttons reaching no script (AH.29's answer for M3, "eruptor is fine").
- **Nothing keeps story progress.** A figure's experience and gold are kept on the profile that placed it, but every session starts over at chapter 1 (AS.38: the host answers no `system.save state`).
- **There's no way from one level to the next past 214.** The end-of-level sequencing doesn't run (AS.53), so no completion screen shows. The hub the original goes to between chapters barely draws (no sky, AS.46; a wrong start, AS.47), and nothing there takes a kid into another chapter. 214 itself stops at its second island (AS.96-AS.98, AS.76).

None of this is new work in kind. Every part has boxes in flight in AH, AS and AT. What's missing is the target that ranks them, so the lanes go there first rather than deeper into one level's fights.

## What Changes

- **Every modelled figure fights,** not just Eruptor. First AH.29's sweep, measured: every modelled character placed headless with its attack scripts run, and each listed as attacking, walking only or stopped on a named member. Then the members the sweep names are modelled, largest reach first, until every character the roster models attacks on its own scripts. This supersedes AH.29's "eruptor is fine" for this target and nothing else: M3's checks on Eruptor and Tree Rex stand.
- **Story saves per profile, resumed.** One save per profile per game, no slot picker. The profile that starts the game is the host, and only its save advances ([CLAUDE.md](../../../CLAUDE.md), Story saves belong to profiles). The original's own save system runs on the host (AS.38) against guacamole's store. A later session resumes from the profile's save where the original's load goes: chapter 1 or level 214, whichever comes first undone, else the hub at the stage its done chapters open (AU.4's reading).
- **Level to level.**
  - **The level end:** the end-of-level sequencing runs (AS.53) and its completion screen draws. On chotchki's word, "we'll want to draw the level end using the game's art but bevy's ui": the screen is laid out in Bevy's UI with the disc's own art (design decision 2), which settles how AT.61's screen gets drawn.
  - **The hub:** it draws (its sky, AS.46; its start, AS.47), and a kid picks the next chapter there as the original's hub offers it.
- **Level 214 to its end:** the interiors (AS.96), Setup_004B and Setup_005 (AS.97, AS.98) and the third island to its outro (AS.76).
- **Out:**
  - co-op (AL, chotchki: "co-op isn't part of it");
  - upgrades and the upgrade screen (AK);
  - couch battles (AI);
  - achievements;
  - chapters past the first the hub offers, beyond reaching them;
  - Windows.

## Capabilities

### New Capabilities

- `guacamole/story-saves`: a profile's story progress kept in guacamole's store and resumed on the next session, the host's save alone advancing.
- `guacamole/level-flow`: what follows a level's end on the TV: the completion screen, the movie the level end queues, and the next level or the hub, where the next chapter is picked.

### Modified Capabilities

None in `openspec/specs/`. AH's in-flight delta `guacamole/hero-attacks` ("Which characters attack") gets edited in place when every figure attacks, since AH hasn't archived (design decision 3).

## Impact

- **Track:** Guacamole. AU is a target beside the ladder, adding boxes only where a part has none. On the ladder it reaches across M5 (AJ, the level alive), M8 (AM, a run of levels with saves) and AS's wiring, and delivers none of those rungs whole.
- **Release reach:** none. No `v*` tag ships guacamole before G1, and the product is untouched.
- **Disc serial read:** BLUS30968 01.00 only: the character files and `global.bld` for the sweep, `global.bld`'s save scripts, the level files AS already reads, the hub's stages and the interface parts the completion screen's art comes from.
- **guac-extract's output:** possibly a part for the completion screen's art, if the interface part doesn't already hold it (design decision 2). Any such part is resumed by an older extract as AS's parts are. No IR version change is planned.
- **Kid-visible milestone:** Giants played from a fresh profile, any figure fighting, chapter 1 into 214 and on to the hub, picking up where you left off next time. Nothing in the ladder names it. It's the first build chotchki can hand the kids as a game.
- **Code:**
  - `crates/guac-script` (the members the sweep names);
  - `crates/guacamole`: [`hero_scripts.rs`](../../../crates/guacamole/src/hero_scripts.rs), [`script_members.rs`](../../../crates/guacamole/src/script_members.rs), [`scripts.rs`](../../../crates/guacamole/src/scripts.rs), [`level_end.rs`](../../../crates/guacamole/src/level_end.rs), [`level.rs`](../../../crates/guacamole/src/level.rs), a level-end screen on the TV's UI stack, the store's save rows;
  - `crates/guac-server` (a migration for the save rows, if `settings` and the profile tables don't hold them);
  - `crates/guac-extract` (the screen's art, if needed).
- **Owning doc:** [`docs/native/guacamole-design.md`](../../../docs/native/guacamole-design.md), for the ladder, the decisions and the TV's UI. [CLAUDE.md](../../../CLAUDE.md)'s Guacamole section, for story saves and profiles.
- **Dependencies:** none new.
