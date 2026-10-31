# Design

## Context

The original's interface is sprites its scripts clone, hang in a tree and set member by member, drawn as each frame's last 72 draws; the TV runs part of those scripts already and draws none of it. Measured on 2026-10-31 for this proposal, read-only over the Giants 01.00 dump with tools already built (AE's `ae-roster` research tool, Python over AC.4's registry, aj-vm's live member table, aj-script's whole-disc census, AH.59's scheduler records and AE.11's breakdown of AC.13's captures), no RPCS3 and no build. The scripts, renders and outputs are in `~/workspace/guacamole-research/at-interface/` (its `README.txt` says which file holds which number); game data stays there.

### What runs a screen in the original

- **Sprites are drawn, never ticked.** `tfbSpriteInfo` derives from `AbstractPlacement`, not `Placement` (AC.4's registry), so it carries no script and the scheduler never lists one: the frame's sprite updater (`tfbSpriteUpdateGame`, eighth of the twelve updaters AH.59 read, after the scripts, actors, viewport, particles and lights) draws them.
- **Thirteen global placements tick every frame**, the first 13 of the scheduler's second group in every one of AH.59's 900, 706 and 177 records: `Bootstrap_Halt_LoadLevel`, `Macros_Animation`, `Macros_GameSystems`, `Macros_Music`, `Macros_Sound`, `Macros_UI`, `Spyro_PortalManager`, `Pause Controller`, `TRG_Checker`, `Macros_Achievements`, `PointLightManager`, `Spyro_PortalControl`, `zPortalManager`. In chapter 1 the June record's group also holds `213_UI_Controller`. The other interface placements of `global.bld` (`UI_PlayerHUD_Macros`, `UI_Main_Macros`, `Dialogue_UI`, `Macros_MenuSystem`, `Macros_Text`, `Macros_Keyframers`, `UI_Animation_Macros`, `UI_Notification_Macros`, `UI_Menus_ScrollingMenus`, `UI_TheatricalReporter`, `Spyro_PortalUI_Macros`) never tick: they hold macros the ticking ones call.
- **Who calls what.** `Macros_UI`'s tick (`UI.ai`, 2,996 ops) runs the keyframers (`Keyframer - Process Keyframers`), the menus (`Menu - Control Menus`), a queued dialogue (`Dialogue - Dialogue UI Create`) and the streaming sprites, and moves the combat timer. The portal manager's tick fades each player's HUD (`Game - Update Player UI Fade`) and steps its banners (`MainUI - Player HUD - Replace Message - Update Messages`). The pause controller's tick opens the pause menu off Start. The HUD's moving parts are update macros the scripts add to the game systems' update set (`System Update Controls`' `update while unpaused macros`), which the game systems' tick runs: 17 distinct ones across the archives read, 6 of them the interface's (`UI - Follow Actor Sprites System Update`, which moves a bite's number with the hero, `Update Health Bars Resizing`, `Zone Title System Update`, `Elemental Zone UI System Update`, `Notification - Manage Queue`, `Cutscene - Overlay Update Macro`), beside the dialogue's wrapper, added through `System - Add/Remove System Update Macro - Update Constantly` (11 calls in `global.bld`).
- **The HUD is built at the join.** `Game - Activate/Deactivate Minion` calls `UI - Create Player Health UI ` (491 ops) right after equipping the hat, which builds the portrait, health, experience and gold (`Game - Create Player XP UI`, `Money UI - Create Player Money UI`, `Combat - Update Player Health Bar `, `MainUI - Player HUD - Create Player HUD Icon`). Its main box is a clone of `Generic_Container` hung on `_Gameplay_Container`, 237 by 96, its bottom-left corner on its parent's bottom-left (alignment 6 to 6), 20 up (location `0x0000FEC0`: x 0, y -20 in 12.4). The portrait is the character's own `NNN_Health_Icon` template (Eruptor's `009_Health_Icon` in his `level.bld`). A bite's number is combat's (`Combat - Hurt Creature` clones `Damage_HealthIcon` and makes it follow the hero, `UI - Make Sprite Follow Actor`).
- **Menus stream archives.** The pause menu's own page lives in `global.bld` (six choices: `UI_PauseMenu_Image_(1)_MapBG` to `(6)_ReturnToShip`); its sub-screens and the upgrade screen load an archive by name through `Game - Load Streaming Level`'s queue and `Level Stream Loading - System Update`, which sets `system.load level` (13 names: `MiscLoad_Options`, `_Objectives`, `_CollectionScreen`, `_Collection_StoryTablets`, `_Collection_LuckyCharms`, `_Collection_SkyStones`, `_Collection_Movies`, `_Accolades`, `_StatsScreen`, `_LevelSelect`, `_ChallengeSelect`, `_InitSetup`, `_UpgradeUI`). The 16 menu archives (`misc/ui_*.bld`, `misc/upgradeui.bld`) hold 887 templates, 506 strings and 107 scripts; the upgrade screen's is 4 scripts (`UpgradeUI_Init`, `Menu_StoreUI`, `PowersMenu`, `UI_UpgradeMenu`, 2,359 ops) and 9 templates, beside each character's own upgrade images (Eruptor's 16 in his `level.bld`, their textures in his `.arc`).

### What runs on the TV (2.0 at `457a0015`)

`UI.ai` ticks every step ([`scripts.rs`](../../../crates/guacamole/src/scripts.rs)'s `GLOBAL_EVERY_TICK`); the pause controller ticks once and holds (`GLOBAL_FIRST_TICKS`); the portal manager runs as the player loop's stretches ([`player_loop.rs`](../../../crates/guacamole/src/player_loop.rs), the ledger's `player-loop`), which leave out the HUD's fade and step its banners natively ([`hud_messages.rs`](../../../crates/guacamole/src/hud_messages.rs)); the game systems' tick runs the 7 update macros `UPDATES` names (the dialogue's wrapper among them, none of the interface's 6: "the HUD's stay off"); level 213's interface placements are unlisted (`213_UI_Controller`, the 15 `Hints_Passive` placements, `CollectibleDialog`, `StoryTabletPickup` and `StoryTablet_213`, `Macros_Cutscene_EnemyIntroUI`, `UI_Quest_Collection`, the four theatrical reports). The join's activation runs (`scripts::join_activate`, `join_enter`), so the HUD's build already runs, every sprite member a value cell ([`members.rs`](../../../crates/guac-script/src/members.rs)) and every sprite clone a blank actor in the script world.

### The data

- **Templates.** 12,275 `tfbSpriteInfo` in 503 files, each with its own `SpriteInfoList` (12,275 lists: the template's attached objects, empty in the file). `global.bld`'s 409: 262 images, 59 movies (the collection screen's champion intros), 49 models, 27 emitters, 6 texts and 6 containers, by what `_contextDataInfo` holds (`AnimatableTextureDataInfo`, `FontDataInfo`, `tfbPhysicsModel`, `tfbParticleInfo`, none). No template in `global.bld`, level 213, `upgradeui.bld`, `ui_options.bld`, `ui_collection_champions.bld` or `title.bld` is attached to another in its file: every tree is built at run time. The roots `_Gameplay_Container`, `_Screen_Container` and `_UI_Container` are 640 by 480; `UI.ai`'s `UI Containers` structure holds eight more by role (screen, gameplay, player HUD, HUD message, dialogue, announce, portal, menu), each with a `hide state`.
- **A template's fields** (AC.4's registry, AE.1's layout): its place (two signed 12.4 halves), `_properties` (nine bitfields: visual type, blend mode, justification, alignment, align to parent, visible, point size, dirty, string width valid), z depth, priority, width, height, a scale vector in percent (100 is 1), colour and replace colour as RGBA bytes, the context, a string, a numeric value, depth and a game transform. 316 of `global.bld`'s 321 image templates have the colour `0x808080FF` (4 are black, 1 a lighter grey) and all 709 templates decoded across six archives the replace colour `0x80808000`: 0x80 is a channel at full, as AC.13's tint constant of 0.502 shows. The six text templates read 84 out of `_properties` bits 17-23, the height of `contemp_ns` and 2 more than `contemp`'s 82: a hypothesis for the point size until AT.1 reads the shifts.
- **Fonts.** Three `igBitmapFont`: `bootstrap.bld`'s `simple` (42 px high, 177 glyphs, a 512² atlas, kern amount 2), `global.bld`'s `contemp` (82 px, 178 glyphs, 1024²) and `title.bld`'s `contemp_ns` (84 px, 175 glyphs, 1024²). A glyph is its character, advance, offset, size, a box in the atlas and four kern-corner flags; each font covers the 94 printable ASCII characters and 81-84 more (accents, curly quotes up to U+201D), Latin only.
- **Strings.** A `StringInfo`'s text is its archive's language pack's string at its index (AE.1). The scripts build strings at run time through a converter: `Text - String Concatenate` sets `stringinfo.string converter`'s `text` and grows its `value set`, and the converter fills `@`s as a number or a time with its digits and comma settings; a `textcolorconverter` colours a text sprite's letters.
- **The members.** The live registry (aj-vm, a June savestate) names each member's measurement type, range and handlers: `type` art, text, model, container, particles (0-4); `visible` off, on, loading; `blend mode` opaque, crossfade, additive, cuts out, gets (0-7); `alignment` and `align to parent` the nine anchors from top-left (0-8); `justify` left, center, right; `texture flip`; `clipping`; `point size`; `priority` and `Z Depth` within ±10,000; `replace color percentage` 0-100; `natural size` and `screen location` read-only; `active` and `removed` the class's sets. aj-vm read a few: `location` is the sprite's +0xA4 in 12.4, `alignment` bits 8-11 of +0xA8, `width` +0xB4, `priority` +0xB0, `color` +0xC8.

### What the original's frame draws

AE.11's breakdown of AC.13's three captures (Eruptor in play): the interface is each frame's last 72 draws (441-512 of 513, 443-514 of 515, 444-515 of 516), all in one shader (`tfb_simple_PS`) straight into the 1280x720 back buffer after the 960x540 scene, no depth test. 59 are single quads and 13 are text runs of 43 glyph quads in all, from a 1024² DXT5 atlas; 30 blend alpha over, 18 add, 11 subtract in reverse and 13 by the destination's alpha; the 28 drawn plain carry a tint of 0.502 per channel. The counts are the same in all three.

### How much each member blocks

Over the UI's archives' op lists (each op once): reads and writes of each member, and the ops mentioning it inside each screen's macro closure (HUD: `Macros_PlayerHUD.ai`'s 34 macros and its body and what they call, 7,496 ops; dialogue: `Dialogue_UI.ai` and level 213's dialogue, hint and tablet scripts, 14,099; pause: the pause controller's pause-menu macros, 16,651; upgrade: `upgradeui.bld`'s scripts, 8,164). The closures overlap: every screen calls `UI.ai`, `Macros_UI_Main.ai`, `Text.ai` and `Menus.ai`.

| Member (today) | disc refs | HUD | dialogue | pause | upgrade | reads / writes |
|---|---:|---:|---:|---:|---:|---|
| `spriteinfo.width` (cell) | 9,424 | 149 | 140 | 159 | 123 | 142 / 495 |
| `spriteinfo.height` (cell) | 8,152 | 108 | 119 | 122 | 104 | 116 / 419 |
| `spriteinfo.location` (cell) | 16,031 | 114 | 138 | 120 | 68 | 669 / 314 |
| `spritematrixmeasurement.attached objects` (cell) | 424 | 91 | 88 | 91 | 88 | 123 / 0 |
| `spritematrixmeasurement.attach node` (cell) | 14,373 | 84 | 80 | 93 | 44 | 13 / 754 |
| `stringinfo.string converter` (cell) | 461 | 58 | 70 | 69 | 58 | 200 / 0 |
| `spriteinfo.align to parent` (cell) | 10,115 | 65 | 55 | 73 | 33 | 1 / 452 |
| `spriteinfo.alignment` (cell) | 10,295 | 61 | 55 | 68 | 32 | 7 / 450 |
| `stringconverter.value set` (cell) | 312 | 39 | 40 | 44 | 38 | 75 / 37 |
| `spriteinfo.color` (cell) | 8,700 | 59 | 40 | 52 | 9 | 407 / 217 |
| `spriteinfo.natural size` (cell) | 1,858 | 37 | 38 | 43 | 35 | 109 / 0 |
| `spriteinfo.priority` (cell) | 6,664 | 44 | 23 | 42 | 12 | 6 / 376 |
| `spriteinfo.point size` (cell) | 6,118 | 22 | 33 | 29 | 20 | 42 / 189 |
| `stringconverter.text` (cell) | 149 | 19 | 30 | 25 | 20 | 28 / 60 |
| `spriteinfo.visible` (cell) | 4,213 | 18 | 25 | 25 | 21 | 30 / 172 |
| `spriteinfo.text` (cell) | 5,061 | 13 | 22 | 24 | 13 | 12 / 219 |
| `spritematrixmeasurement.orientation` (cell) | 1,725 | 19 | 16 | 15 | 9 | 115 / 0 |
| `spriteinfo.justify` (cell) | 2,126 | 15 | 17 | 20 | 6 | 1 / 125 |
| `spriteinfo.blend mode` (cell) | 413 | 17 | 4 | 17 | 4 | 0 / 42 |
| `spriteinfo.scale` (cell) | 5,297 | 10 | 12 | 17 | 2 | 205 / 0 |
| `spriteinfo.numeric value` (cell) | 2,601 | 12 | 1 | 13 | 0 | 3 / 44 |
| `spriteinfo.text color converter` (absent) | 162 | 0 | 0 | 7 | 7 | 42 / 0 |
| `textcolorconverter.color set` (absent) | 121 | 0 | 0 | 4 | 4 | 19 / 9 |
| `nameextractor.name checksum` (absent) | 181 | 0 | 2 | 4 | 0 | 18 / 0 |
| `spriteinfo.model attach nodes` (absent) | 247 | 0 | 0 | 2 | 0 | 18 / 0 |
| `spriteinfo.type` (absent) | 14 | 3 | 1 | 3 | 1 | 10 / 0 |

A read of `location`, `color` or `scale` is mostly the step before a write of one half (`location . screenmeasurement.y`) or channel. What the scripts read back to decide something is what the model has to compute, not just store: `natural size` (109 reads, never written), `attached objects` (123, the engine's set), `width` and `height` (142 and 116), `point size` (42), `visible` (30), the converter's `value set` and `text`, `model attach nodes` and `type`.

## Goals / Non-Goals

**Goals**
- Every interface screen the original's scripts build in a level draws on the TV from those scripts: the HUD first, then dialogue and hints, the pause menu and AK's upgrade screen.
- The members' semantics written down once, in our own words, in a pure crate that headless tests and component lanes can run without Bevy.
- Each screen deletes the stand-ins it replaces, in the same box.

**Non-Goals**
- The TV's own screens: setup, the portal's alerts, the cards, the join code, the intros (AP's and AB's; Decisions, sixth batch).
- Loading an archive into a running level, script-driven pause and the level's whole placements ticking: the VM's (decision 8 says which screen waits on which).
- The front end (`title.bld`), the level's own menus between levels and the store (AM's), the card game and the arena select (AI's and later).
- Sound: a dialogue's voice and a menu's clicks are AR's and the combat-sound line's.
- Any language but English (AE design decision 3); the fonts are Latin-only anyway.

## Decisions

### 1. Run the scripts, model the members

AT writes the engine's half of the interface and nothing above it: a sprite made by a clone, its fields, its tree, what its members read back, and something that draws it. The script half already runs on the VM: the join builds the HUD today, `UI.ai`'s tick runs every step, and the cutscene system's dialogue wrapper runs since AH.28. A screen rebuilt natively would be one stand-in per screen against the 34 HUD macros, 94 menu macros and 19 dialogue macros the disc already holds (their files 2,124, 3,473 and 1,053 ops), each of them a chance to drift (Method, point 4).

### 2. What a screen is

A script builds a screen by cloning a template (`OpSpawn`), hanging the clone on a parent (`spritematrixmeasurement.attach node`, 754 writes) and setting its members; it animates it by writing members over time (36 keyframer macros in `Keyframers.ai`, one track per member: position, alpha, the three scales and rotations, colour; and `slide value`); it removes it with what's attached (`UI - Remove Sprite and Attached`, which walks `attached objects`). `MainUI - Message Box Text Create` is the pattern whole: clone `Generic_TextBox` at `Generic_Container`, attach it to the box, set location, width and height from the box less its padding, point size, alignment and align to parent 4 (centre), justify 1, text and priority -2, then grow it by `Text - Add Line Height to Text Sprite Until Text Fits` and `Text - Add Width ...`, which read `natural size` against `width` until the text fits.

So the model holds: templates read from the extract; instances, each a copy of its template's fields plus its parent, its attached objects in order, its strings and its hide state; the members as the registry types them; and a draw list derived from them. Kinds: art (an image, or a movie on an image), text (a font and a string), model, container, particles; the draw list carries the first two whole and the other three as entries the adapter draws (decision 6).

### 3. The members' semantics come from their handlers and are checked against the oracle

aj-vm's live table gives each member's getter, setter and reset by address; reading them by aj-vm's method (Ghidra over the decrypted EBOOT, kept in the research dir) says what field or bitfield each touches and what each computes (`natural size`'s text measure, `screen location`'s walk up the tree, `numeric value`, the order of `attached objects`, what clamps). Those readings become `guac-interface`'s crate docs, in our own words, before its code, as `guac-script`'s semantics are its docs. They're hypotheses until the oracle agrees: AC.13's captures for where things land on screen (AT.3), and a sprite reader over PINE for the live fields at AH.10's and AH.11's moments (AT.10).

### 4. The split: pure crates, a thin adapter, scoped gates

As `guac-motion` and Avian's seam are split:

- `guac-formats` (pure, fuzzed): the new layouts and an interface reader over an archive's IGZ: templates, fonts.
- `guac-ir` and `guac-extract`: the interface part and the per-level sprites part, images through AC.6's decoder.
- `guac-interface` (new, pure, `#![forbid(unsafe_code)]`, no Bevy, no I/O; depends on `guac-ir` for the templates and fonts and on `guac-script` for its values and member access): instances, tree, members, strings, the text measure, layout and the draw list. It answers a member get, set or reset the way `guac-script`'s `Host` asks for one, so the TV's host forwards the interface namespaces to it and a headless test does the same with no Bevy in sight.
- `guacamole`: the host's routing (a clone of a sprite template makes an instance) and an `interface` module drawing the draw list through `bevy_ui`.

A pure-crate box's gate is scoped: `cargo fmt --all --check`, `cargo clippy -p <crates> --all-targets -- -D warnings` and `cargo test -p <crates>` for the crates it touches (through [`.githooks/ram-tmpdir.sh`](../../../.githooks/ram-tmpdir.sh)), its `#[ignore]`d disc tests by hand, and `cargo check -p guacamole` only when it changes something guacamole already uses. A wiring box runs [`.githooks/pre-push`](../../../.githooks/pre-push) and the guacamole e2e. The merge into `2.0` runs the full gate either way, so a scoped box can't land anything the full gate would refuse.

### 5. The formats to read

Of the disc's 200 IGZ types, 142 have a field layout in `guac-formats`. The interface touches these:

- With a layout already: `tfbSpriteInfo` and `AnimatableTextureDataInfo` (AE.1), the image chain (`tfbAlphaTextureUnitIDAttr`, `igTextureAttr2`, `igImage2`), `StringInfo` and `LanguagePackInfo` (AE.1), `tfbPhysicsModel`, `tfbParticleInfo`, the list bases `igDataList` and `igObjectList`.
- Without one, all in AC.4's registry, so each is a layout written from the registry plus builder tests, no new registry read: `SpriteInfoList` (12,275 objects; an `igObjectList`), `igBitmapFont` (3; `_kernAmount` 0x34, `_textureBind` 0x38, `_charMetricsList` 0x3C, on `igFont`'s `_height` 0x0C, `_baseLine` 0x10, `_attrList` 0x14, `_color` 0x20, `_charPadding` 0x30), `igCharMetrics` (530, 0x30: `_charNumber` 0x08, `_properties` 0x0A, `_step` 0x0C, `_offset` 0x10, `_size` 0x18, `_texCoordMin` 0x20, `_texCoordMax` 0x28), `igCharMetricsList` (3), `FontDataInfo` (3; `_font` 0x20), and `ScriptScreenInfo` and `ScriptColorInfo` (638 and 939), which the VM reads already off their `_value` and no reader needs more of.
- Needing AC.4's method (its step 5, the field registration function's customisation): the bitfields the registry lists without shifts or widths, `tfbSpriteInfo._properties`' nine and `igCharMetrics._properties`' four kern corners. AT.1 reads them and checks them on the disc (every template's visual type against its context, every alignment within 0-8, the text templates' point size).

### 6. Drawing through Bevy's UI

`bevy_ui`, as AP's TV is, under AP's router and z order:

- One root node standing for the interface's space, mapped onto the window as the original maps it onto its 1280x720 frame. The containers are 640 by 480 in the files while the frame is 1280x720, so the mapping is a measurement, not a guess: AT.3 reads it off AC.13's quads.
- Retained nodes, one per drawn instance, patched when its draw-list entry changes and despawned when it goes (AP's pattern), so a clone made and removed per bite costs a node, not a rebuild.
- Art as `ImageNode`s with their atlas rects; text as glyph image nodes cut from the font's atlas, since Bevy's text stack takes TTF and OTF fonts, not bitmap fonts; colour, replace colour and the blends `ImageNode` can't do (add, reverse subtract, by the destination's alpha: AC.13's 18, 11 and 13 draws) through one `UiMaterial`, its colour math written in `guac-interface` and checked there.
- Over the level and under the TV's own UI: the interface at the HUD's 800 today, under the pause overlays' 900, the portal's alerts' 940, a modal's 950, the join corner's 960 and an intro's 1,000 (The TV's UI).
- Model, emitter and movie sprites draw nothing until AT.13 (a model through a render target its node shows, an emitter through AH.87's particles, a movie through the intro player's video path), each kind logged once.

### 7. The references

- AC.13's three captures (Context): 72 interface draws, 59 quads and 13 text runs, four blends, the tint convention. AT.3 reads each quad's rectangle and texture and each glyph's off them with `rrc-dump` (no RPCS3), which gives the space's mapping, the text's pixel scale and the HUD's static layout to compare the model's draw list with.
- AH.10's and AH.11's shots of the HUD in play (`flash/shot-s183`, `-s189`; `defeat2`, `restart1`, `restart2`) and what AH.223 lists from them: the health as its number alone (`276`), a bite's number by the hero moving with him, the plate and its bars shifting right with LEVEL MAX and LOW HEALTH.
- A sprite reader over PINE in `tools/oracle-harness` (AT.10): each live sprite's name, fields, parent and text at a flip, the oracle's own answer to the model's.

### 8. What each screen needs from the VM

| Screen | Its placements and scripts | Runs on the TV today | Needs beyond today |
|---|---|---|---|
| HUD | built by the join's activation; moved by `UI.ai`'s tick, the portal manager's per-player stretch and the UI's update macros | the build and `UI.ai`'s tick | the portal manager's fade and banner stretches and the UI's registered update macros in the game systems' tick (AT.11), unless the VM's whole placements have landed |
| Dialogue and hints | `Dialogue_UI`'s macros through the cutscene system and `UI.ai`'s tick; level 213's `213_UI_Controller` (ticks in the original), 15 hint placements, the tablet, the collectibles' box, the enemy intro card, the quest collection, four theatrical reports | the cannon's line through the cutscene system | level 213's interface placements ticking (the VM's whole placements, or a listing as AH's); the voices the scenes wait on (AR) |
| Pause menu | the pause controller's tick off Start; `Menus.ai` through `UI.ai`'s tick | the controller's first tick | the controller ticking every step; the game's own pause (`System Tools - Pause Gameplay`: its ignore sets, `placement.script suspended` and `system.pause state`, both absent today); an archive loaded into the running level for its sub-screens |
| Upgrade screen | `Portal - Queue Bring up Upgrade UI` streams `upgradeui.bld`'s 4 scripts | nothing | AK (upgrades and path kept per profile); the archive load; the character's upgrade images from his `.arc` |

The HUD needs nothing from the VM's coming work, so it goes first.

### 9. The order, and the stand-ins each deletes

| Box | Screen | Deletes | Ledger rows |
|---|---|---|---|
| AT.11-AT.12 | HUD and its banners, a bite's number | [`hud.rs`](../../../crates/guacamole/src/hud.rs) but AH.248's arrows, [`hud_messages.rs`](../../../crates/guacamole/src/hud_messages.rs) | `hud` (narrowed to the arrows until AT.13), `hud-messages` (struck) |
| AT.13 | model, emitter and movie sprites | the arrows | `hud` (struck); `cosmetic` narrowed to sounds and particles' members |
| AT.15 | dialogue and hints | none in code (nothing draws them) | `encounter-pans`' text half |
| AT.16 | pause menu | [`pause.rs`](../../../crates/guacamole/src/pause.rs)'s one-button pause | `pause`'s menu half (the lost pad's screen stays) |
| AT.17 | upgrade screen | none (AK's) | AK's line |

### 10. A member moves to the model with everything it needs, never into a stop

`UI.ai`'s tick also moves the combat timer every mover's update reads (`GLOBAL_EVERY_TICK`), so a stop in it would stop more than the interface. A cell never stops a script; an absent member stops it in play mode. So the interface namespaces move from cells to the model in one box (AT.9), with every member the UI's archives reach registered (the 22 absent among them), and a headless run of `UI.ai`'s tick, the join and AH.8's route strict on the dump shows no new stop before it lands.

### 11. AP keeps the TV's own screens

The portal UI's own macros (`PortalUI_Macros.ai`, 3,699 ops: the portal's errors, the join prompt, the restart prompt) stay unrun, the portal's alerts being AP's. The defeat screen's Restart Chapter is the one unclear case: the original draws it from the same file (`Portal - Show/Hide Restart Prompt`, `Portal - Restart Chapter Confirm`), and the TV draws it as AP's modal ([`defeat.rs`](../../../crates/guacamole/src/defeat.rs)); AT.20 asks chotchki.

### 12. The extract

An extract-wide `interface` part: the templates, images, fonts and English strings of `global.bld` (409 templates, 310 images, 906 strings, `contemp`), `bootstrap.bld` (2 templates, `simple`), every character archive (1,970 templates in 75; the upgrade images' textures from each `.arc`) and the 16 menu archives (887 templates, 506 strings). A per-level `sprites` part for each level the extract reads (level 213: 31 templates, 187 strings), so AS's levels get theirs as they come. Both under AG.15's per-part rule. `title.bld`'s 89 templates and `contemp_ns` wait for the front end.

## Risks / Trade-offs

- **The text measure has to match.** The fit loops read `natural size` 109 times and stop on its answer, so a measure a pixel off picks another size or another line count. Mitigation: the handler read (AT.4), AC.13's 43 glyph quads (AT.3) and the sprite reader's `natural size` (AT.10), all before the dialogue box (AT.15), whose boxes fit the most text.
- **A stop in `UI.ai`'s tick stops the combat timer.** Decision 10.
- **The space's mapping.** 640 by 480 containers on a 1280x720 frame and a 16:9 TV; a wrong mapping misplaces every screen at once. Mitigation: AT.3 before AT.12.
- **Nodes per frame.** Text as one node per glyph: AC.13's frame has 59 quads and 43 glyphs; a full pause page or a dialogue box is a few hundred nodes, inside what `bevy_ui` lays out per frame. Measured at AT.12 and AT.16 (the frame times AE.14's hook reads).
- **Clones that never go.** A bite's number is a clone per bite; the VM frees no memory (AH.38). The model frees an instance when the scripts remove it, and AT.9's disc test counts instances after a fight.
- **Sprite blends past four.** The enum has eight values (0-7) and the captures show four; the others wait for a frame that draws them (AT.3 records which value each of the four is).

## Migration Plan

1. The pure half, in any order that keeps each box's inputs: the layouts and bitfields (AT.1), the reader (AT.2), the captures read (AT.3), the crate's semantics (AT.4), sprites and members (AT.5), text (AT.6), layout and the draw list (AT.7), the extract (AT.8). Component lanes, scoped gates.
2. The wiring for the HUD: members on the model (AT.9), the oracle's sprite reader (AT.10, the RPCS3 lane's), the HUD's moving parts (AT.11), the adapter and the HUD on the TV (AT.12), the other sprite kinds (AT.13), chotchki's look (AT.14).
3. Dialogue (AT.15), pause (AT.16) and the upgrade screen (AT.17), each when what decision 8 names has landed.
4. chotchki's call on the defeat screen (AT.20) whenever he gets to it; the docs (AT.22) last.

One-way doors: none. The extract is a cache, and the new parts resume alone.

## Open Questions

1. **The defeat screen's Restart Chapter**: the original's portal UI (AT runs it) or AP's modal (it stays)? Until chotchki answers, it stays AP's (AT.20).
2. **The pause menu's Quit and Return to Village**: the original leaves the level for the front end or the hub, neither of which guacamole has. Recommendation: Quit leaves the level to the portal view (what leaving the level does today), Return to Village stays hidden until the hub exists; asked at AT.16.
3. **A window that isn't 16:9**: the Mac's window can be any shape. Recommendation: the interface's space keeps the original's aspect, letterboxed in the window, as the original's frame is 16:9; decided at AT.12 from AT.3's mapping.
