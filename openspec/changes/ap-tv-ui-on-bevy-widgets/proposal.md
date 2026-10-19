# Phase AP - The TV on Bevy's own UI stack: retained screens, a controller on the TV's choices and the Skylanders look

## Why

guacamole's TV is hand-rolled `bevy_ui` that despawns and respawns its whole tree on every change (a second phone joining, every extraction tick), takes input by polling keys and the mouse with no focus and looks nothing like Skylanders: a flat background, flat gold and titles with no stroke or glow. Bevy 0.20 already compiles in the pieces a console UI needs (`bevy_ui_widgets`' `Button` and `Activate`, `bevy_input_focus`' focus and directional navigation, `bevy_state`, gradients, shadows and custom UI shaders), but they all assume entities that survive an update and one place that decides who gets a press, and the TV has neither. Every later TV surface (the Home menu, AK's upgrade screen, AG's prompts) stands on fixing that first, and it's cheapest now, while the TV is nine screens and a card grid.

## What Changes

chotchki's answers to the research's six questions (2026-10-18) set the scope:

- **Its own change** ("a, agreed"), not spread over AG and a later rung.
- **Retained screens.** A `bevy_state` state derived from the pure `screen_for` spawns each screen on enter and despawns it on exit; text that changes inside a screen (the players line, the join address, the error message, AB's extraction band) is patched in place; the portal keeps eight slot cells and redraws only the slot that changed. That's what AA's design decision 4 promised ("the UI reconciles entities from" `PortalCards`) and the code never did.
- **One input router** for the pad and the keys, so the top layer takes a press and nothing under it sees it. AB's intro skip moves into it (any button or key still skips, per `guacamole/character-intros`).
- **Real buttons on the TV's choices**: "Choose the folder" on Setup and NoGiants, "Continue" on Found, "Close" on AlreadyRunning and ServerError, each a `bevy_ui_widgets::Button`, focused when the screen appears. A controller's bottom face button presses it, the right face button closes the error screens.
  - **Click-anywhere goes away on Setup, NoGiants and Found** ("b"): only the focused button reacts, so a click lands on the button or does nothing. The error screens KEEP click-anywhere and Esc (AA.6's fix for a mouse-only TV PC), and AlreadyRunning still closes itself.
- **Directional focus moves wait for the Home menu** ("agreed", 2026-10-18): after the buttons no screen has more than one choice, so AP.7 (`bevy_input_focus`' geometry-based navigation, held repeat, moves kept inside an overlay) moved to `openspec/backlog.md` with the Home menu, the first screen with two. AP's router still reads the D-pad and the left stick, so that change adds the navigator (design decision 4) behind moves the router already reads.
- **The Skylanders look** from [`docs/aesthetic/design_language.md`](../../../docs/aesthetic/design_language.md): a theme (palette, the TV type scale with its 24 px floor), the starfield, the gold bezel on portraits, gold gradient card edges, the bezel's picking state as the focus look and motion (card rise, portal impact, idle float) that freezes for offscreen shots.
  - **Titles through a shader** ("Hmmm could we run a shader over it?"): each title's text rendered once into a texture and drawn through a WESL `UiMaterial` that makes the gold stroke, the drop shadows, the glow and the gradient fill from the glyphs' alpha, re-rendered only when the string changes. A spike verifies the APIs first; stacked `Text` copies are the fallback if it doesn't read on a real display.
  - **The owner pip stays top-right** ("b, top right bottom right will interfere with player 2's ui"), recorded as a deliberate departure from the design language's bottom-right ownership pip.
- **Out:** a phone remote for the TV ("b, let's not scope creep"); a TV settings screen, since settings live on the phone ("b, but we'll have to discuss what settings make sense when the time comes"), so `Checkbox`, `Slider` and `RadioGroup` stay unused and which settings exist is a later discussion, not an AP task; BSN, `bevy_feathers` and third-party UI crates (the design says why).

The research lives outside the repo in `~/workspace/guacamole-research/tv-ui/`: a plan standing on a survey of the engine and one of our TV as built, every API it recommends re-opened in the 0.20.0 registry sources.

## Capabilities

### New Capabilities

None.

### Modified Capabilities

- `guacamole/shell`: two ADDED requirements, the TV's choices answering a controller (with a click reaching only the button) and the startup-error screens closing from a controller. ADDED, not MODIFIED: none of AA's shell requirements says how a TV screen is pressed or dismissed (click-anywhere is AA code in [`picker.rs`](../../../crates/guacamole/src/picker.rs), never spec), and the capability lives only in AA's unarchived change, so `openspec/specs/` holds no shell spec to modify yet. `guacamole/figure-cards` doesn't change: cards still appear and leave within 1 second, eight fit and none overlap, whatever animates inside them.

## Impact

- Track: Guacamole, beside the ladder (not a rung). Kid-visible: no ladder milestone; the TV's own screens look like Skylanders, and a pad works the TV's choices. Mac first: checked on the Mac's own display like AA.11; what only the TV at couch distance shows joins the backlog's TV pass ([`openspec/backlog.md`](../../backlog.md)), not this exit.
- Starts when AB is merged to `2.0` (it is, as of 6fff756) and nothing else is editing `crates/guacamole/src/` or the guacamole e2e tests. AB.22's co-op resume fix is editing [`guacamole_tv.rs`](../../../crates/e2e-tests/tests/guacamole_tv.rs) and [`guacamole_intro.rs`](../../../crates/e2e-tests/tests/guacamole_intro.rs) as of this proposal, and AE's first 3D camera would collide in `crates/guacamole/src/`; each goes before or after AP, not beside it.
- Disc serials: none read. guac-extract's output doesn't change (extract layout and IR version stay).
- Code: `crates/guacamole` only (the screens in [`tv.rs`](../../../crates/guacamole/src/tv.rs), the picker's input in [`picker.rs`](../../../crates/guacamole/src/picker.rs), the intro skip in [`intro_player.rs`](../../../crates/guacamole/src/intro_player.rs), the [`mirror`](../../../crates/guacamole/src/mirror.rs) and [`shots`](../../../crates/guacamole/src/shots.rs), plus new router, theme and title-shader modules, one embedded WESL file and a `title_spike` example, which AP.10 deleted once the crate drew the titles itself) and [`crates/e2e-tests`](../../../crates/e2e-tests/README.md) for the focus check and two press tests. The pure logic (`screen_for`, `PortalCards::reduce`, `should_open`, `IntroQueue::apply`, `band`) and its tests stay untouched. No change to `guac-server`, the phone or the product.
- Dependencies: none, and no Bevy feature. Everything used is already compiled by guacamole's `features = ["ui"]`, `Text2d` included (Bevy's `bevy_ui` feature turns on `bevy_sprite`; the design's Context has the `cargo tree`). One dev-only crate feature, `title-spike`, kept AP.8's example out of `cargo test` the way `video-spike` keeps AB's, and went with it at AP.10.
- Test API: `/api/_test/tv` gains a `focus` field (additive; the e2e client reads it as `serde_json::Value`) and `POST /api/_test/tv/press` feeds the router, both under `test-hooks` only. The press route is a test hook, not a phone remote.
- Owning docs: [`docs/native/guacamole-design.md`](../../../docs/native/guacamole-design.md) (a TV UI section: the stack, the router, the title shader, reduced motion, the pip departure), [`docs/aesthetic/design_language.md`](../../../docs/aesthetic/design_language.md) (the look's source; §3.1 gets the TV pip note) and [`CLAUDE.md`](../../../CLAUDE.md)'s guacamole bullet.
- Release reach: none. guacamole isn't in any `v*` release yet, so AP lands and archives on `2.0` with no release task.
- Id: AP, not the AG that [`openspec/config.yaml`](../../config.yaml)'s next-id rule reads. The ladder in the backlog and the design doc already names AE and AG through AO for its rungs, so AP takes the first letter past them, the way AF skipped AE; chotchki on ids, 2026-10-18: "I don't care, the two letter id is just a holdover from claude-plan-bridge."
