# Phase AT - The original's interface: its scripts on the VM, its sprites and text drawn through Bevy's UI

## Why

chotchki, 2026-10-31, at the stock-take: "ui, run the originals" (design doc, Decisions, sixth batch). The HUD, the menus, dialogue and hints and AK's upgrade screen come from the original's interface scripts on the VM, their sprite and text members drawn through Bevy's UI with the disc's own sprites and fonts, not rebuilt screen by screen. The TV's own screens (setup, the portal's alerts, the cards) stay AP's.

The disc already holds every screen as script data, and the VM already runs part of it (design, Context, has each measurement):

- **The scripts already run; nothing draws what they write.** Eruptor's join on the TV runs `Game - Activate/Deactivate Minion`, which calls `UI - Create Player Health UI ` (491 ops) straight after equipping his hat, so the HUD's build runs today, into value cells: `spriteinfo` is the disc's biggest member namespace (98,415 references, 17.8 %), 2 of its 30 members are real (the screen's size), 19 are cells and 9 absent, and 97,463 of its references land in cells nothing draws (the stock-take). The UI as a whole is 54 members and 21.3 % of the disc's references: 4 real, 28 cells, 22 absent.
- **A screen is templates, clones and a tree.** Every one of the disc's 12,275 sprite templates (`tfbSpriteInfo`, in 503 files) is a placement the scheduler never ticks; the scripts clone one, hang it on a parent with `spritematrixmeasurement.attach node` (754 writes over the UI's archives, none of the 409 templates in `global.bld` attached in the file) and set its place, size, alignment, colour, order, text and visibility. Text sizes itself: the scripts' fit loops read `spriteinfo.natural size` 109 times and write it never, so the text measure is the engine's and has to match.
- **The stand-ins say what's missing.** The TV draws its own HUD ([`hud.rs`](../../../crates/guacamole/src/hud.rs), the stand-in ledger's `hud`), steps the HUD's banners with nothing drawn ([`hud_messages.rs`](../../../crates/guacamole/src/hud_messages.rs), `hud-messages`), puts a one-button overlay where the pause menu goes ([`pause.rs`](../../../crates/guacamole/src/pause.rs), `pause`) and draws no dialogue or hint (`encounter-pans`); the sprites and strings are the `cosmetic` row's cells.

## What Changes

- **A pure interface model**, new crate `crates/guac-interface` (no Bevy, no I/O, `#![forbid(unsafe_code)]`, as `guac-motion` and `guac-script` are): sprite instances made from the disc's templates, their tree, every interface member's get, set and reset (34 `spriteinfo`, 8 `spritematrixmeasurement`, 5 `spritepositionmeasurement`, `stringinfo`'s converter, 6 `stringconverter`, 2 `textcolorconverter`, 4 `nameextractor`, 4 `viewporttitlesafeinfo`: 64 in the live registry), the strings and the bitmap font's text measure, the layout and a draw list. Its semantics are its crate docs, written first in our own words from each member's handler in the EBOOT (the live registry names them) and checked against the oracle.
- **The formats and the extract**: `guac-formats` gains the font and sprite-list layouts from AC.4's registry, the sprite bitfields the registry doesn't hold (read by AC.4's method) and an interface reader; `guac-ir` and `guac-extract` gain an extract-wide interface part (the templates, images, three fonts and English strings of `global.bld`, `bootstrap.bld`, every character archive and the 16 menu archives) and a per-level sprites part.
- **The wiring**: the interface members move from value cells to the model, a clone of a sprite template makes a sprite, and a new `interface` module in `guacamole` draws the model through `bevy_ui` (retained nodes as AP's are, art and glyphs from the disc's textures, a UI material for the blends and colour replacement `ImageNode` lacks), over the level and under the TV's own screens.
- **Screens in this order**, each deleting its stand-ins: the HUD (the join already builds it: [`hud.rs`](../../../crates/guacamole/src/hud.rs) and [`hud_messages.rs`](../../../crates/guacamole/src/hud_messages.rs) go), dialogue and hints (the text half of `encounter-pans`), the pause menu ([`pause.rs`](../../../crates/guacamole/src/pause.rs)'s menu half), AK's upgrade screen and the defeat screen's Restart Chapter ([`defeat.rs`](../../../crates/guacamole/src/defeat.rs)'s screen; chotchki's call at AT.20).
- **Out**: the TV's own screens, the join code and the intros (AP's and AB's); languages other than English (AE design decision 3); level audio (AR); loading an archive into a running level (the VM's, design decision 8); the front end (`title.bld`) and the menus of a level run (AM's).

## Capabilities

### New Capabilities

- `guacamole/interface`: what the TV draws of the original's interface: the sprites and text its scripts make, where and how they say, in the disc's own art and fonts; the dialogue, hints and pause menu as the original's.

### Modified Capabilities

- `guacamole/tv-hud`: two ADDED requirements, the original's HUD with its banners and a bite's number by the hero, which replace AH's "The hero's numbers on the TV" and its level-up arrows once both changes archive (AT archives after AH; AT.21 says what changes then). ADDED, not MODIFIED: the capability lives only in AH's unarchived change, so `openspec/specs/` holds no tv-hud spec to modify yet (AP's reasoning for `guacamole/shell`).
- `guacamole/disc-extraction`: one ADDED requirement, the interface extracted with the rest and resumed alone.

## Impact

- Track: Guacamole, beside the ladder (not a rung), one of the sixth batch's foundations. Its pure boxes stand on nothing in flight; the HUD on AH's VM, landed; dialogue on the VM's whole placements and AR's voices; the pause menu on the VM's script pause and archive loading; the upgrade screen on AK. Each box names what it stands on (tasks).
- Disc serial read: BLUS30968 01.00 only: `permanent/global.bld`, `permanent/bootstrap.bld`, every `character/*.bld` (75 hold sprites, 1,970 of them) and the `.arc` beside it for the upgrade images, the 16 menu archives (`misc/ui_*.bld` and `misc/upgradeui.bld`: 887 sprites, 506 strings, 107 scripts) and each level the extract reads. Tests that read it are `#[ignore]`d and take `GUAC_GIANTS_DUMP`.
- guac-extract's output changes by new parts under AG.15's per-part rule, so an extract resumes for them, the layout stays at 4 and `IR_VERSION` at 6 unless a record AT touches changes shape, which AT.8 records.
- Kid-visible: the original's screens over the level, in the order above. No ladder milestone.
- Code: `crates/guac-formats` (layouts, the interface reader, property tests and a fuzz target); `crates/guac-ir`, `crates/guac-extract` (the parts); new `crates/guac-interface`; `crates/guac-script` (the interface members' keeper); `crates/guacamole` (the host's routing, the `interface` module, the stand-ins deleted, `/api/_test/tv`'s `interface` field under `test-hooks`); `tools/oracle-harness` (a sprite reader over PINE); `crates/e2e-tests` (the HUD's cases on the model).
- Gates: a pure-crate box runs scoped gates (fmt, clippy and tests of the crates it touches, `cargo check -p guacamole` when it changes something guacamole uses), so component lanes skip the multi-hour runs; a wiring box runs the full gate and the guacamole e2e (design decision 4). Every merge into `2.0` runs the full gate either way.
- Dependencies: none new. Everything drawn uses Bevy features guacamole compiles already (`bevy_ui`, `UiMaterial`, images).
- Owning doc: [`docs/native/guacamole-design.md`](../../../docs/native/guacamole-design.md) (Decisions, the stand-in ledger, The TV's UI, The extractor, Milestone ladder); [`CLAUDE.md`](../../../CLAUDE.md)'s Architecture for the new crate.
- Release reach: none. guacamole runs from `cargo` on the Mac and no `v*` release carries it yet, so AT lands and archives on `2.0` with no release task.
- Backlog: the original's HUD line is promoted and deleted; AH.223 stays parked as a line of its own, its HUD items going with AT.12; AK's line points at AT for its screen; the ledger rows that named the HUD line (`hud`, `hud-messages`) name AT's boxes, and `cosmetic`, `pause` and `encounter-pans` name AT for their interface halves.
- Id: AT, the decision's own (AR and AS are the batch's other two foundations).
