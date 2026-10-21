# Skylander Portal Controller — Working Notes

Compact reference for this project. SPEC.md is the authoritative long-form requirements + Q&A log. Execution is tracked in **OpenSpec** (since 2026-10-17): each phase is a change at `openspec/changes/<id>-<slug>/` and its `tasks.md` is the plan — tasks are numbered `<ID>.<N>` with a checkbox so they're traceable, and get ticked live as the work lands. Unphased work lives in `openspec/backlog.md`; id allocation, task numbering and the specs policy live in `openspec/config.yaml` (read it before proposing). `PLAN_ARCHIVE.md` is the frozen pre-OpenSpec record.

## What it does

A Windows app that wraps RPCS3 (PS3 emulator) so kids can manage the emulated Skylanders portal from a phone/iPad over Wi-Fi. Launched from Steam Big Picture. Shows a QR code on the TV → phone connects → pick a game → portal-control view with the family's figure collection.

## Non-goals

The product's, carried verbatim from PLAN.md. Guacamole plays the original game's own audio and video from the user's disc, stops reading the committed wiki data once the disc roster lands (M1) and deletes it at G1 with the product code that still reads it (`docs/native/guacamole-design.md`).

- No bundling of `.sky` files or game backups (piracy). The patched RPCS3 *itself* IS bundled — GPL, source public — per the v1 distribution model (16.9.0b): Windows today, macOS via Phase U.
- No Linux support — production targets are Windows + macOS.
- No user-entered figure names.
- No audio (text-only Kaos to dodge copyright).
- No live wiki scraping at runtime — data is committed to the repo.
- **No AXUIElement macOS driver** (ex-Phase 12) — IPC to the patched RPCS3 supersedes it.

## Guacamole (native engine)

- **What it is:** a from-scratch Rust/Bevy reimplementation of the Skylanders games WITH original-gameplay parity (minus the design doc's deliberate departures), so the art can be upscaled and relit (started 2026-10-17). Users supply their own disc dumps; `guac-extract` pulls the assets on first run (a game's first pick).
- **Mac first** (2026-10-17, "we're focused on mac for now, windows will come much later"). Guacamole is built, run and validated on the M3 Max Mac, which drives the TV; no rung's exit depends on Windows or the HTPC, and `.githooks/pre-push` (on in the dev clone) is the Mac's automatic gate. The HTPC (Windows, RTX 2080 Ti, Steam Big Picture, 86" 4K TV) is the eventual target, brought up later in one pass: the Windows/HTPC bring-up line in `openspec/backlog.md` (the MSI, Windows CI, the firewall prompt, Steam Input passthrough, DLSS and the 4K floor, the HTPC's own dump).
- **Giants first** (`BLUS30968`) — the later games should come easier after it. The parity target is the 1.00 disc (the HTPC's Giants has no title update): the extractor reads the disc alone and treats a dump an update changed as unknown.
- **The product is frozen** (2026-10-17). The controller installed on the HTPC keeps playing the games and doesn't need touching until Guacamole is ready; a product change happens only when Guacamole needs it or something breaks. The one exception so far: ACR122 support left the product "as soon as it's convenient", on its own track, in change AD (`openspec/changes/ad-remove-acr122/`), before AA.
- **Second binary until G1.** `guacamole` is a separate Bevy app that embeds the same server (Axum + Leptos SPA + driver worker) in-process on a tokio background thread; a native `PortalDriver` feeds the Bevy world over a channel. Only one of the two runs at a time (same port, same mDNS name). The product binary (`crates/server`: eframe launcher + patched RPCS3, P7/P8 window management) stays untouched in behavior and window management. AA.1 moved only what `guacamole` runs into the shared `crates/guac-server` (routes, driver worker + watchdogs, profile store, Kaos, mDNS, QR, paths, logging, firewall) and AA.2 lifted the bootstrap into `guac_server::boot`; `.sky` editing/stats, working copies, the display-mode and RPCS3-config helpers, the product's config loader + wizard + migrations and the eframe UI stay in `crates/server` (reached through four hooks the product passes to `boot` and `guacamole` doesn't) and leave with it at G1 (NFC is already gone: AD landed first). On the Mac `guacamole` runs from `cargo`; from the Windows/HTPC bring-up on, the Windows MSI carries both binaries (the distribution starting point, 2026-10-17). Either way a `v*` tag — only on chotchki's word — ships the product rebuilt from current source, so its behavior has to survive every change made for Guacamole.
- **Its own SQLite, no files to copy around.** `guacamole` keeps profiles, PINs, per-profile figure instances, per-profile story saves, achievements and settings in ONE database of its own under its own data root (release: `~/Library/Application Support/guacamole/` on macOS, `%LOCALAPPDATA%\guacamole\` on Windows; `./dev-data/guacamole/` in dev), with its own migration set (`crates/guac-server/migrations/`: the product's profile tables, same shape, plus `settings`). It never opens the controller's `db.sqlite` (the frozen controller's `sqlx::migrate!` refuses a migration it doesn't know) or `config.json`. Profiles aren't copied — the kids set theirs up again in `guacamole` ("set them up again, it's super simple"), and since the pairing key is `guacamole`'s own too, a phone switching apps scans the TV's join code once. The extract dir is a regenerable cache. Setup asks one thing, where the disc dumps are — no RPCS3 path, PS3 firmware, figure pack or `games.yml` scan.
- **Figures: full reset.** Guacamole never reads a `.sky` — no import, no figure crypto, no firmware pack, no figure dumps, no scanner. A figure is an identity (toy type + variant) from the disc's own roster; the phone offers the whole roster, no per-kid shelf. Progress starts fresh in Guacamole's store, one instance per profile per figure (today's working-copy rule minus the file), and the phone's figure editor edits those rows.
- **Story saves belong to profiles.** One save per profile per game, no slot picker. The profile that starts the game is the host and only its save advances; figure progress (XP, gold, upgrades) goes to the profile that placed the figure, so in co-op both kids' characters advance.
- **Online is out, local PvP is in.** Nothing networked beyond the LAN phone link (no DemonWare, no PSN); trophies become per-profile achievements stored with the profile (names/icons from the disc's `TROPDIR` if it reads, otherwise our own); in co-op a counter both kids feed credits each kid's own share where Guacamole can tell whose it was, otherwise only the host earns it. Couch battles are part of parity for every game.
- **Gates.** **G1** = `guacamole` plays one game end to end → it becomes the only thing this repo builds and ships; the product binary, everything RPCS3 (`rpcs3-patches/` P1–P8 as a product series, `rpcs3-control`'s product drivers, nested-app bundling/signing), `crates/sky-parser` and the wiki data leave. G1 stops building the product, it doesn't uninstall it: the controller on the HTPC keeps the other five games until Guacamole covers them. **G2** = parity → the dev-only RPCS3 oracle (`vendor/rpcs3` + a dev-only patch series) retires.
- **The phone is the portal.** No USB/HID portal, ever.
- **Naming:** binary `guacamole`, crates `guac-*` (`guac-formats` = pure fuzzed parsers, `guac-ir` = versioned serde IR, `guac-extract` = the CLI, `guac-server` = the shared server lib, `guac-oracle` = the oracle comparisons' pure half, `guac-motion` = the hero's motion and the physics seam, `guac-script` = the tfbScript interpreter). Never "Skylanders…" in a crate or binary name.
- **HARD RULE: no game bytes in the repo, ever** — no extracted assets or scripts, decrypted EBOOTs, decompiler/disassembler output, RPCS3 memory dumps, RSX captures or keys. They live outside the tree; tests that need disc data are `#[ignore]`d and read the dump path from `.env.dev`. The **game-magic guard** ([`crates/guac-extract/tests/no_game_bytes.rs`](crates/guac-extract/tests/no_game_bytes.rs), part of `cargo test --workspace`, so `.githooks/pre-push` runs it) fails when any file in git's index or in unpushed history starts with a game magic (`guac_extract::magic::GAME_MAGICS`), is a gzip/zstd whose inflated head does, or is a big-endian PPC64 ELF. That's why test fixtures are always built in code at test time (synthetic SFOs, IGAs, MP4s, the e2e intro from ffmpeg's `testsrc`) and never committed: a committed fixture with a real magic would trip it, and a real disc file can't be one.
- **RPCS3 is the dev-only fidelity oracle**, never a dependency of `guacamole`: observe in the oracle → write the behavior in our own words as an OpenSpec spec → implement from the spec. Original script data runs on a tfbScript VM over the user's IGZ rather than being rewritten. The oracle is the product's pin + P1-P8 + the dev-only **O-series** in `rpcs3-patches/oracle/` (O1-O2 upstream's Mac boot fix, O3 pause/capture/screenshot/savestate, O4 frame-locked pads 0 and 1; every O-verb answers `ERR unknown_cmd` unless `GUAC_ORACLE=1` is in RPCS3's env), built outside the repo by [`rpcs3-patches/oracle/build.sh`](rpcs3-patches/oracle/build.sh), never in a product build. It agrees with itself exactly only on what doesn't integrate host time (a still hero, the frame count, the state at the first pause); a walk agrees for about 60 flips and the RNG parts sooner, so comparisons are envelopes and FLIP bands inside a usable horizon. The design doc's The oracle has the numbers and the tolerances.
- **`docs/native/guacamole-design.md` owns the track** — decisions (incl. the deliberate departures from the original), architecture, the milestone ladder (OpenSpec changes from `AA` up, later rungs in `openspec/backlog.md`), the RE toolchain. Plans sequence by conditions and report measured progress; no time estimates. Read it before proposing a Guacamole change.

## Tech stack (committed)

- **Language:** Rust.
- **HTTP/WS server:** Axum.
- **Phone SPA:** Leptos (WASM). JS fallback acceptable if touch/UX forces it.
- **PC-side launcher window:** egui via `eframe`, fullscreen, sized for 86" TV at 10 ft.
- **DB:** SQLite via `sqlx` (async, compile-time-checked queries).
- **RPCS3 control:** AF_UNIX **IPC to a patched-upstream RPCS3** (Phase 16) — the production path on Windows, no GUI/dialog. The legacy UI Automation driver (Windows accessibility API; image/OCR, raw coordinates as last resorts) is kept only as a fallback for a *stock* RPCS3 (`SKYLANDER_PORTAL_DRIVER=uia`).
- **QR code:** any standard Rust crate.
- **E2E tests:** pure-Rust WebDriver (fantoccini or thirtyfour — no preference).

## Architecture

- Cargo workspace:
  - `crates/core/` — shared types (Figure, SlotState, Command, Event). No I/O. Public/private split enforced via `Figure::to_public()`.
  - `crates/indexer/` — walks the firmware pack, emits `Vec<Figure>` with stable SHA-256-truncated IDs.
  - `crates/rpcs3-control/` — `PortalDriver` trait; `IpcPortalDriver` (Phase 16 — blocking AF_UNIX to the patched RPCS3 via `uds_windows`; the **Windows production driver**), `UiaPortalDriver` (legacy Windows UI Automation; the stock-RPCS3 fallback), `MockPortalDriver` (feature `mock`; the macOS/dev driver). RPCS3 process lifecycle (`RpcsProcess`, Job-Object kill) + Win32 window handling (`SetWindowPos`) live here too.
  - `crates/guac-server/` (lib `guac_server`, openspec AA.1) — the server both binaries link: Axum routes (`http`), driver worker + watchdogs (`state`), profile store, Kaos, mDNS, round QR, `paths`, `logging`, `firewall`, the embedded phone bundle, the shared config types, and `boot(BootOptions) -> Result<ServerHandles>`, the one startup call (binds the listener FIRST, so the port is the single-instance lock). Also guacamole's `setup` (loader + settings), `dumps` (PARAM.SFO scan) and `native_catalogue`, plus guacamole's migration set (`migrations/`). Where it needs product code it calls one of four hook traits in `hooks.rs` (`FigureFiles`, `DisplayModes`, `Rpcs3Config`, `SettingsWriter`); a missing hook is the Native arm.
  - `crates/server/` — the product binary (`skylander-portal-controller`): eframe launcher (`ui/`, badge, vortex, compositor, GL fallback), the product's `config` loader + first-launch wizard, its three frozen `migrations/`, and what stayed product-only behind the hooks (`product_hooks.rs`: `sky_edit` + `sky_stats` under the `sky-stats` feature, `working_copies`, `display_mode`, `rpcs3_config`). Re-exports the moved `guac_server` modules. `dev-tools` feature on by default.
  - `crates/guacamole/` — the second binary (Bevy 0.20; the binary builds the `ui` features plus `bevy_audio` and `mp3`, the `hero_view` example's dev-dependency adds the 3D ones; on macOS its pads come from Apple's GameController.framework, not gilrs: openspec AG.53, the lib's [`game_controller.rs`](crates/guacamole/src/game_controller.rs) on `objc2-game-controller`, a `NonSend` system feeding Bevy's own gamepad stream, bevy_gilrs's plugin left out there and kept elsewhere, Home doing nothing (its system gesture asked off, which macOS ignores: its Games app still opens, the backlog's Home menu line), each position's prompt letter from Apple's element name; its live check is `cargo test -p guacamole --test live_gamecontroller -- --ignored`, a Bluetooth pad in hand): boots `guac_server` with `DriverKind::Native` and `NativePortalDriver` (places by identity via `PortalDriver::place`, sends `Placed`/`Cleared` over a bounded crossbeam channel to the TV; placing what a slot already holds for the same owner sends nothing), the TV screens on Bevy's own UI stack (openspec AP; the design doc's The TV's UI): `bevy_state` screens retained between updates, `bevy_ui_widgets` buttons (import `bevy::ui_widgets::Button` by name: the prelude's deprecated `bevy_ui::Button` fails clippy) with `bevy_input_focus`'s focus behind one input router ([`input.rs`](crates/guacamole/src/input.rs)), titles through a WESL `UiMaterial` embedded in the binary ([`title.rs`](crates/guacamole/src/title.rs), `title.wesl`), each rendered by a `Camera2d` of its own, so anything reading the TV's camera finds it by its `TvCamera` marker, never by `Camera2d`; the `rfd` folder picker and `--setup`, the first-run extraction on a Giants pick ([`extraction.rs`](crates/guacamole/src/extraction.rs)), the intro queue and player ([`intro.rs`](crates/guacamole/src/intro.rs), [`intro_player.rs`](crates/guacamole/src/intro_player.rs), [`video.rs`](crates/guacamole/src/video.rs): openh264 + `mp4`, MP3 through Bevy audio) and the level loader (openspec AG.19: [`level.rs`](crates/guacamole/src/level.rs) reads chapter 1 on a task once the pick finds it whole, hands it to the renderer a share a frame and holds it hidden until every part is resident and every key warm, on the library's [`level_scene.rs`](crates/guacamole/src/level_scene.rs), its fog the `LevelFog` resource; its props, openspec AG.25, by the library's [`props.rs`](crates/guacamole/src/props.rs): AG.7's rule on the placements, a ranged one standing, drawn and solid, while the hero is within its range, templates, creatures and what a script wakes left out), behind the start's pass (openspec AE.25: [`prepare.rs`](crates/guacamole/src/prepare.rs) compiles the library's [`census.rs`](crates/guacamole/src/census.rs) keys, a card's UI and the level camera's output under a "Getting ready" screen before any other screen but an error's, so before any extraction). The level's input layer (openspec AG.20: [`pad.rs`](crates/guacamole/src/pad.rs), [`pause.rs`](crates/guacamole/src/pause.rs)) sits in that router under an intro's skip: the move stick raw (Bevy's dead zone and change threshold off), the first pad connected walking the hero, Start's one-button overlay and the controller message stopping `Time<Virtual>`, prompts by AG.5's table or the button's position. The first walk (openspec AG.21: [`play.rs`](crates/guacamole/src/play.rs)): the first figure placed that walks (the library's [`walkers.rs`](crates/guacamole/src/walkers.rs), Eruptor alone until AG.27) takes the TV into chapter 1 once his intro is over, the level ready with its collision (built into Avian colliders on the load's task, so the frame only spawns them) and his model's keys warm: [`hero.rs`](crates/guacamole/src/hero.rs)'s controller on Avian's side of the seam ([`physics.rs`](crates/guacamole/src/physics.rs)) from `213_Init_Location`'s first waypoint, his card's model drawn on him in the level's light without its point light (so his keys are the census's), camera 0 following him on a camera built through `level::view_camera`, the pads the level's layer's; the TV leaves the level when it leaves the portal view. The portal and the level (openspec AG.29, AG.63, AG.73): `play::judge` rules the portal's alert off the extract's roster (a toy Giants can't use, too many Skylanders, too many magic items, the call for a Skylander once the hero comes off), [`alert.rs`](crates/guacamole/src/alert.rs) draws it full screen over the level with the small join code in the corner, the level held (`Time<Virtual>` stopped, the pads moving no one) while one shows or no hero is in, the next hero coming in where the last stood, and no intro once the level has started. The level's lights and his reach the shader in the world's frame (the character uniform's `misc.w`, which the shader turns by `view.view_from_world`), so the camera's moves rewrite no material; only a fog change relights them. `dev-tools` on by default; `test-hooks` adds `/api/_test/tv` (its `focus` field the focused choice's id, its `level` the level's load, AG.19, and its props standing, AG.25, its `prepare` the start's pass, AE.25, its `input` the pads and the level's layer, AG.20, its `mode` (`portal`, `level`, `paused` or `held`) and `hero` (who walks, his model's state, his position and heading, the level camera as drawn), AG.21, the hero's `gait`, his locomotion's weights, speed and rates, AG.30, and the portal's `alert` and `join_corner`, AG.29; no cards while the level hides the screen), `/api/_test/tv/skip`, `/api/_test/tv/press` (`{"button": "south"}`: a press for the router's screens, not a phone remote), `/api/_test/tv/pad` (`{"pad": 0, "stick": [x, y]}` and the like: a pad driven as a hand would, for the level's layer) `/api/_test/tv/frames?since_ms=<ms>` (the TV's frame times, each with the pipelines compiled in it: AE.14's stall check), `/api/_test/tv/gaits?since=<step>` (the hero's locomotion step by step: AG.30) and `/api/_test/tv/drawn?since=<frame>` (the walk per drawn frame: the clocks and overstep, his drawn transform, pose and clips, the camera's, the render world's frames: AG.55).
  - `crates/guac-formats/`, `crates/guac-ir/`, `crates/guac-extract/` (openspec AB) — Guacamole's extractor. `guac-formats`: pure parsers over byte slices (`#![forbid(unsafe_code)]`, no I/O: PARAM.SFO, IGA, IGZ, textures, Edge, Havok), each under a property test in `cargo test` plus a cargo-fuzz target in `fuzz/` (nightly, outside the workspace). `guac-ir`: the versioned serde IR (`IR_VERSION`; RON in dev, postcard in release). `guac-extract`: dump identity + known-dump table, manifest, extract root, census, intro index and the ffmpeg transcode, as the library guacamole calls and as a dev CLI (`guac-extract identify|census|intros|transcode <dump>`); its [`tests/no_game_bytes.rs`](crates/guac-extract/tests/no_game_bytes.rs) is the game-magic guard. [`docs/native/guacamole-design.md`](docs/native/guacamole-design.md) (The extractor) has the detail.
  - `crates/guac-oracle/` and `tools/oracle-harness/` (openspec AF) — the RPCS3 oracle's two halves, dev-only, never linked by `guacamole`. [`guac-oracle`](crates/guac-oracle/src/lib.rs) is pure (`#![forbid(unsafe_code)]`, no I/O, no RPCS3): the RON input-script format, traces, envelopes and usable horizons, the trajectory, event, totals and frame comparisons with their calibrated defaults (`tolerance`), FLIP through `nv-flip` and an MT19937 model, so a rung's test can dev-depend on it. [`oracle-harness`](tools/oracle-harness/src/lib.rs) (lib `oracle_harness`, bin `oracle`) drives the oracle: the isolated home, the launch, the P1 and O-verb codecs, the PINE client, Giants' RNG and actor readers and the divergence runner, every output through `rrc-dump`'s work-tree refusal.
  - `crates/guac-motion/` (openspec AG.4) — the hero's motion, pure (`#![forbid(unsafe_code)]`, no Bevy); for now the physics seam the controller (AG.16) will query. [`seam.rs`](crates/guac-motion/src/seam.rs): the `PhysicsSeam` trait (a shape cast to the time of impact with the contact planes there, contact planes within a distance, a ray with the triangle it hit and its material, overlaps) over static, kinematic and sensor bodies: the four `CollisionInfo` convex shapes, box and cylinder rounded by a margin (Havok's convex radius), and a triangle mesh with internal edges fixed and a material per triangle. [`parry.rs`](crates/guac-motion/src/parry.rs): `ParryWorld`, the seam over parry3d 0.31 alone, what controller tests run on. [`conformance.rs`](crates/guac-motion/src/conformance.rs) (feature `conformance`): the table every implementation passes, each answer against its closed form and two implementations' answers within 1e-5 of each other. The Avian side is guacamole's [`physics.rs`](crates/guacamole/src/physics.rs): `SeamPlugin` (Avian's physics schedule, collider hierarchy, backend and trees, nothing of its solver; in the TV app) and `AvianSeam`, a `SystemParam` over `SpatialQuery`. avian3d is a git dependency pinned to `main` at `1070759` ("Update to Bevy 0.20") until 0.8 is on crates.io; never #1070's `34c9b008d2f4`.
  - `crates/guac-script/` (openspec AH.14) — the tfbScript interpreter, pure (`#![forbid(unsafe_code)]`, no Bevy, no I/O): the original's scripts run on AH.13's op graphs (`guac_formats::script`). Its semantics are its crate docs, written first, in our own words ([`lib.rs`](crates/guac-script/src/lib.rs)). [`program.rs`](crates/guac-script/src/program.rs): `Program::link`, the files loaded together (`bootstrap.bld`, `global.bld`, a level or character file) turned into ids, `global.bld` imports through the export ids, stubs resolved by name. [`vm.rs`](crates/guac-script/src/vm.rs): `Vm`, a counter per placement over its op list once a tick (blocks, behaviours, static macro frames with the busy guard, structures, sets, single-precision arithmetic, the comparison codes), strict and play mode (`Halt` naming the placement and the member), the per-placement per-tick op budget (`DEFAULT_BUDGET`) and the whitelist. [`host.rs`](crates/guac-script/src/host.rs): members by import hash, `Members` (each modelled member's registered type and bounds, kept by the host or as a value cell) and the `Host` trait (member get, set and reset, the generator's draws, the world ops), which guacamole implements as a `SystemParam` over its ECS queries (design decision 21) and the tests over plain structs. [`generator.rs`](crates/guac-script/src/generator.rs) (AH.15): the session's MT19937, `Generator`, seeded the original's way (`sgenrand`, two `69069x + 1` steps a word) from the clock (`session_seed`), or loaded from an `rng peek`'s state; checked against `guac_oracle::rng`, a dev-dependency only. [`stall.rs`](crates/guac-script/src/stall.rs): the stall detector the headless runs use. Its cargo-fuzz target is in `crates/guac-script/fuzz/` (nightly, outside the workspace); its `#[ignore]`d disc test (`giants_toys`) builds the toy table on the VM.
- Separate from the workspace: `phone/` is a Leptos CSR crate that builds to WASM via trunk. `guac_server::embedded_assets` serves `phone/dist/` through `rust-embed` (read from disk in debug builds, baked into release builds).
- Threading, both binaries: the main OS thread owns the UI (eframe in the product, Bevy in guacamole); `boot` starts a dedicated background thread hosting the tokio multi-thread runtime (Axum + driver worker), and the driver factory runs ON that thread (UIA needs COM as MTA there).
- Driver worker: single tokio task drains `mpsc<DriverJob>`; each load/clear runs inside `spawn_blocking`. Portal state lives in `Arc<Mutex<[SlotState; 8]>>`; changes broadcast through `broadcast::Sender<Event>`.
- Phone is a dumb client. REST for commands (return 202), WS for state (snapshot on connect, `SlotChanged` per event).

## Data & paths

- **First-launch config** (PC keyboard, one time): RPCS3 executable path, firmware-pack root. Games auto-detected by scanning RPCS3's library for known serials; missing games don't delete their per-game settings.
- **Firmware pack** layout: `{Game}/{Element}/[Alternate types/]{Name}.sky`. Top-level `Items`, `Adventure Packs` (per-game subfolders), `Sidekicks` (top-level is a duplicate — ignore it; Giants' internal `Sidekicks` is authoritative). Ignore `desktop.ini`, posters, element-symbol PNGs (reuse as element icons), readme `.txt` files.
- **Runtime state roots** (resolved once at startup, gated by the `dev-tools` Cargo feature — release builds physically can't pick the dev path):
  - Release: `%APPDATA%\skylander-portal-controller\` — `db.sqlite`, `working/<profile_id>/<figure_id>.sky`, `logs/` (daily rotation, 7-day retention).
  - Dev: `./dev-data/` — same layout, plus `./logs/` next to it. Both are gitignored. Never write runtime state outside these roots.
  - A `scanned/` dir an old `nfc-import` dev build left under the runtime root is ignored: nothing reads or deletes it since AD.
  - **guacamole's own root** (`paths::guac_data_root`): release `~/Library/Application Support/guacamole/` on macOS, `%LOCALAPPDATA%\guacamole\` on Windows, separate from the controller's — `db.sqlite` (its own migration set), `logs/`, `extract/` (the extract cache, made on the first Giants pick: one `<TITLE_ID>-<APP_VER>-<hash8>/` per known dump with `manifest.json`, the IR and the transcoded intros; delete it and the next pick re-extracts). Dev: `./dev-data/guacamole/`, a subfolder of the controller's dev root that the product never reads (`db.sqlite`; logs still go to `./logs/`, the same `server.log` the product's dev build writes). Deleting it takes guacamole back to first run. guacamole never opens the controller's `db.sqlite` or `config.json`.
  - **The dev extract root is the one exception** to "never write runtime state outside these roots": `GUAC_EXTRACT_DIR` (from `.env.dev`, required in dev, no default) is a folder OUTSIDE the tree, because an extract under the gitignored `./dev-data/` would be one `git add -f` from a leak. guac-extract refuses a dev extract root inside any git working tree (`GUAC-E-EXTRACT-IN-CHECKOUT`), and a release one only when guacamole's data root or `extract/` holds a `.git` or links into a checkout, so a home folder kept under git doesn't block it (AB.18).
- **`DATA_ROOT`** is the tracked-asset root (`./data`: `images/`, `figures.json`, `games/`), never the runtime root. Setting it to `./dev-data` breaks image lookup, because the server then looks for `dev-data/images/...`.
- **Known dev RPCS3 install:** `C:\emuluators\rpcs3` (note the typo; it's the real path).
- **Known dev firmware pack:** `C:\Users\chris\workspace\Skylanders Characters Pack for RPCS3`.
- **Phone never sees file paths or filenames** — only stable figure IDs.

## Profiles & PINs

- Up to 4 profiles. Each has a 4-digit PIN (anti-sibling, not real security). PINs stored in SQLite.
- Per profile: working copies of `.sky` files (copy-on-first-use from fresh pack), last portal layout.
- Profile picker is the first screen after connect ("welcome, portal master"). Game picker comes after unlock.
- Guest mode = post-MVP.
- Imaginators creation crystals: per-profile, never auto-reset without confirmation.

## Portal behavior

- Portal is an 8-slot screen regardless of game (game will complain if overloaded — we don't pre-validate).
- Portal view is a "drawer" separate from the browse/collection view. Tap figure → Add button. Tap slot's figure → Remove button. No drag-drop.
- On session unlock: prompt "resume last setup?" then auto-drive the GUI.
- Working copy auto-loads when a figure is picked. Fork from fresh on first-ever use. Reset-to-fresh is an explicit user action.
- Figure file shared across games (one working copy per profile+figure).

## Concurrency & takeover

- **Up to 2 concurrent phone sessions** (matches co-op player count). Each unlocks its own profile with its own PIN; unlocks do not propagate between phones.
- Portal is shared free-for-all — either phone can touch any slot. The single driver worker serialises operations naturally; no per-slot arbitration.
- 3rd connection evicts the **oldest** session (FIFO) — evicted phone sees the Kaos "taken over" screen with a "kick back" button.
- 1-minute cooldown applies only to forced eviction (anti-ping-pong). Joining into a free slot has no cooldown.
- Evicted session's kick-back inherits game/portal state but the profile re-locks; PIN re-entry required.
- Any connected phone can display the join QR in-app ("show join code") so existing players can hand it to a new joiner.
- Portal view shows an **ownership indicator** per occupied slot (profile colour/initial) so players can tell whose figure is whose. Ownership = the profile that placed the figure into that slot.
- Post-disconnect figure-cleanup semantics (2-player case) are deliberately deferred — revisit alongside the reconnect-overlay phase.

## Security

- Trusted LAN only, HTTP.
- Phase 1: unsigned commands. Phase 1.5 (immediately after protocol stabilizes): HMAC signing, key embedded in QR.
- Strict input validation. No filesystem paths ever leave the server. Canonical figure names only (no user-entered names).

## Aesthetic

- Match Skylanders game UI: starfield blue backgrounds, circular gold-bezeled figure portraits, bold white titles with gold outline, cartoony feel. Reference: `docs/aesthetic/ui_style_example.png`.
- Implement via CSS (wiki asset resolution isn't enough for high-res phones).
- Phone UI is theme-able (prepping for the Kaos "mind magic" takeover skin — dark purple/pink).
- **Launcher badge is 3D** (PLAN 10.7). The QR card / back-face title cards / orbiting player pips all share one GL pipeline (`crates/server/src/badge.rs::BadgeRig`): cylinder coin geometry (front fan + back fan + side wall + flatter elliptical torus), Lambert diffuse + Blinn-Phong specular, multi-turn intro spin via `LaunchPhase::badge_rotation_y`, 360° flip-on-state-change for back-face text swaps. Each unique front-face texture (QR raster, per-`BackFace` text, per-profile pip face) is rasterised once via `badge_text` (TitanOne via `ab_glyph`, faux-emboss in 3 glyph passes) and cached as a GL texture. Surfaces other than Main (Crashed / Farewell / ServerError) route through `paint_centered_3d_back_card` so the visual language is consistent across screens. The legacy 2D `paint_titled_card` / `paint_bezel` family is gone.
- **Mocks live in `docs/aesthetic/mocks/`** as standalone HTML files — open directly or via a local server. `docs/aesthetic/mocks/index.html` lists every mock grouped by flow.
- **Review mocks on a real iPhone, not just desktop preview.** Safe-area insets, Dynamic Island collisions, mobile Safari viewport behavior (address-bar hiding, pinch-zoom, orientation lock) all differ from desktop devtools. Serve with `python3 -m http.server 8089` from `docs/aesthetic/mocks/` and open `http://<mac-en0-ip>:8089/` on the iPhone (`ipconfig getifaddr en0` to find the IP). Requires the iPhone and Mac to share a network — Mac-as-hotspot (Internet Sharing) works; iPhone-as-hotspot blocks incoming connections to the Mac.
- **Safe-area pattern for top-of-screen padding:** `max(Npx, calc(env(safe-area-inset-top) + 12px))` where N is the desktop-preview value. Preserves desktop look; adapts on devices with a notch/island. Same pattern applies to L/R when content hugs screen edges.
- **Claude-driven iOS repro ([`ios-inspect`](https://github.com/chotchki/ios-inspect)).** Mac-only CLI that drives the iOS Simulator + Safari via `xcrun simctl` and the WebKit Web Inspector protocol (through `ios-webkit-debug-proxy`). Use it for **layout/CSS probes** — `safe-area-inset-*` values, computed styles on specific selectors, DOM-subtree dumps, web-content-only screenshots. Use the chromedriver e2e harness for **functional regressions** that don't depend on iOS-specific rendering. One-shot bringup from the repo root: `cargo install --git https://github.com/chotchki/ios-inspect --tag v0.0.1 && (cd phone && trunk serve --address 0.0.0.0 --port 8090 &) && ios-inspect boot && ios-inspect open http://$(ipconfig getifaddr en0):8090/`. Then iterate with `ios-inspect eval '<js>'`, `ios-inspect computed-style <selector> --filter …`, `ios-inspect screenshot --web-only -o /tmp/x.png`. Proxy lifecycle is self-healing — no manual restart mid-session. **Sim fidelity gap to remember**: non-standalone sim Safari reports `safe-area-inset-bottom = 0` (vs ~34px on real Dynamic Island hardware); bugs that depend on the bottom inset need PWA-standalone mode (Add-to-Home-Screen in the sim) or a real-device fallback. See the [`ios-inspect` repo](https://github.com/chotchki/ios-inspect) README for full usage + limitations.

### CSS escape-hatch policy (PLAN 9.5)

The phone is on Tailwind v4 (PLAN 9.x). When adding or editing styles, default to inline utilities; reach for an `@apply` rule only when readability suffers; reach for raw CSS only for things Tailwind can't shorten. Three tiers:

- **Inline utilities (default).** Layout (`flex`, `grid`, `absolute`), spacing (`p-4`, `gap-3`), typography (`font-display`, `text-body-italic`), colour (`text-gold-bright`, `bg-sf-1`), single-layer effects (`rounded-xl`, `shadow-sm`). Put them on the element in the Leptos `view! {}` macro. This is the home of 95%+ of styling.

- **`@apply` in a per-component CSS file.** Reach for this when an inline class string would exceed roughly **12 utility classes on a single element**, when the same cluster repeats across siblings (an action button group, a row of swatches), or when the element uses pseudo-element trickery (`::before` / `::after`) that needs to share styling vocabulary with the parent's utilities. Files live at `phone/styles/components/<component>.css`, are imported from `phone/styles/input.css`, and are wrapped in `@layer components { … }` so they slot beneath utility specificity.

- **Raw CSS (rare).** Reserve for things Tailwind genuinely can't help with: `@keyframes` (named animations), `@font-face` (font loading), multi-layer pseudo-element decoration with stacked-gradient `background-image`, `:has()` selectors driving cross-component layout, masked SVG silhouettes (`-webkit-mask: url(…)`), `mix-blend-mode` overlays, complex shadow stacks beyond what `shadow-[…]` arbitrary values reasonably express. Co-locate raw CSS with the component that uses it (next to its `@apply` block in the same file), not in a global stylesheet.

The migration's reference exemplars: `gold_bezel.css` (mixes all three tiers — `@apply` for layout primitives, raw `box-shadow` stacks + radial gradients), `kaos_overlay.css` (raw-heavy: six stacked layers, seven keyframes, masked SVG sigil), `framed_panel.css` (a clean two-rule file with one `::before` decoration). When in doubt, look at how a similar component handled it.

Hidden costs to keep in mind: every `@apply` in a component file means class names that don't appear inline on the element, which makes the element harder to grep for visually. Prefer `@apply` only when the readability gain on the markup side outweighs that. The 12-class rule of thumb is just a heuristic — a 6-class string with three repetitions across siblings is more @apply-worthy than an unrepeated 14-class one-off.

## Kaos feature (LAST — post-MVP)

- Wall-clock timer: 20-min warmup, then random within every hour window.
- Text-only overlay (Kaos catchphrases from wiki — text avoids copyright issues).
- 1-for-1 swap of a portal figure with a randomly-chosen compatible figure from the owned collection.
- Compatibility rule (heuristic, can enhance later): figure works in its game of origin and all later games, with known exceptions (vehicles only in SuperChargers, etc.).
- Reposes: collapsed in browse view with a "N variants" badge. Cycle button on card for variant swap.

## Testing

- (1) Unit tests for pure logic (figure indexer, protocol, state machine).
- (2) Integration tests for DB + filesystem.
- (3) E2E: pure-Rust WebDriver drives a headless browser against the phone SPA; test harness reads the QR URL from the log file by pattern. Run locally, not in CI.
- CI deferred until the app works.
- **Every change ships with a test — including the C++ emulator patches
  (`rpcs3-patches/`), tested from Rust.** Two layers, so each side of the IPC
  contract is pinned:
  - **Controller side / wire contract (runs in CI):** `crates/rpcs3-control/src/ipc/proto.rs`
    unit-tests the codec, and `tests/ipc_loopback.rs` runs the real
    `IpcPortalDriver` against an in-process fake AF_UNIX server speaking the P1
    protocol — no RPCS3 needed (Win+mac CI).
  - **Emulator side (HTPC, `#[ignore]`d):** `tests/live_ipc.rs` (attach to a
    running instance) and `tests/live_launch.rs` (the 16.6.1 `launch_no_gui` +
    IPC-readiness + window-handle path) drive the **real patched binary** — proving
    the C++ listener comes up, `g_skyportal` actually loads/clears, the STATE/
    heartbeat frame counter advances, the window handle is published, and a no-GUI
    boot reaches a playable state. This is how we test the C++ without C++ test
    infra: Rust e2e tests against the running emulator (mirror the `live*.rs` UIA
    suites). Both self-manage the RPCS3 lifecycle / clean up the socket + lockfile. If a patch (P1/P2) changes wire behaviour, update the fake
    server in `ipc_loopback.rs` to match — the loopback doubles as the executable
    spec the live binary must satisfy.
  - **The oracle's O-series follows the same rule** in `tools/oracle-harness`: the
    O-verb codec's unit tests and the fakes `verbs_loopback.rs` (O3),
    `pad_loopback.rs` (O4) and `pine_loopback.rs` (PINE) run in `cargo test`; the
    `#[ignore]`d `live_*.rs` there drive the oracle build on the Mac.

## Dev mode (`dev-tools` feature flag)

- Logs to `./logs/`, verbose level.
- Skip first-launch config by reading paths from a `.env.dev`.
- E2E harness can inject a profile and bypass PINs.
- Release builds physically cannot take these shortcuts.
- **guacamole:** `cargo run -p guacamole` from the repo root (add `--features test-hooks` for `/api/_test/*`). It reads the same `.env.dev`, and the keys that matter are `GUAC_DUMP_DIRS` (dump folders in the OS's path-list form; set, it overrides the stored folders and the TV skips setup), `GUAC_EXTRACT_DIR` (the extract root outside the tree; unset, a Giants pick extracts nothing and plays no intros), `GUAC_FFMPEG` (ffmpeg's path, over the stored `ffmpeg_path`; unset, discovery tries beside the executable, PATH, then Homebrew's bin folders on macOS), `GUAC_OUTPUT_LATENCY_MS` (how far intros hold their video back for the sound path, 0-1000, over the stored setting; default a provisional 43), `BIND_PORT`, `DATA_ROOT` and `WINDOW_MODE` (it also reads `PHONE_DIST`, which nothing uses: the phone comes from `rust-embed`) — never `RPCS3_EXE` or `FIRMWARE_PACK_ROOT`. A `test-hooks` build also reads `GUAC_TEST_KNOWN_DUMP` (`<TITLE_ID> <APP_VER> <manifest hash>`: the synthetic dump the e2e harness writes into its own scratch `.env.dev`, trusted beside the known-dump table; never set by hand). Process env, not `.env.dev`: `GUAC_MUTE` (any value mutes intros in a dev build), `GUAC_TEST_POSTED_PADS_ONLY` (any value, `test-hooks` builds only: no gilrs and no GameController pads, so only `/api/_test/tv/pad`'s pads connect and a paired pad can't walk the hero first; the e2e harness sets it with `GUAC_MUTE`), `GUAC_LEVEL_VIEW` (any value: once the Giants pick has loaded chapter 1, a dev build draws it over the TV at AF's fresh-state camera, AG.19's dev entry, with no hero: a level it shows is no longer one the walk can enter) and `GUAC_SHOT_DIR`, which renders the TV offscreen to PNGs; it must be outside any git checkout (intro shots are game frames), and one inside is refused; one that isn't there is made, and a shot that fails to write logs an error (AE.29). With it, `GUAC_SHOT_YAW` (degrees, default 0) is where on its turn each card's model holds still, and `GUAC_SHOT_WALK=<ms>` adds a shot every that many milliseconds the hero has moved while he walks (AG.21; off by default, since a shot's PNG is written on the frame's thread). The extractor's CLI: `cargo run -p guac-extract -- identify|census|intros|transcode <dump>` (reads `GUAC_EXTRACT_DIR`, `GUAC_FFMPEG`, `GUAC_FFPROBE` from the env, else `.env.dev` in the current folder). Release build + run on the Mac: [`docs/native/guacamole-design.md`](docs/native/guacamole-design.md) (Running guacamole on the Mac).
- **Guacamole RE paths** (dev only, both outside the tree): `GUAC_GIANTS_DUMP` is the Giants (BLUS30968) dump root, the folder holding `PS3_GAME`; the `#[ignore]`d disc tests in `crates/guac-formats/tests/` and `crates/guac-extract/tests/` and the `hero_view` example read it from the process env, else `.env.dev`. Of guac-extract's, `giants_identity`, `giants_intros` and `giants_transcode` make a throwaway extract under `GUAC_EXTRACT_DIR` (the transcode also needs ffmpeg and ffprobe), and `census_dumps` writes its report into `GUAC_EXTRACT_DIR`, needs ffprobe and takes every `GUAC_*_DUMP` key plus `GUAC_CENSUS_DUMPS` (a path list of dumps, or of folders holding them up to two levels down). `GUAC_CAPTURE_DIR` is where RSX captures, savestates and `rrc-dump --out` output go. [`crates/guac-formats/tests/giants_capture.rs`](crates/guac-formats/tests/giants_capture.rs) (AC.15-AC.17) reads it as the folder holding AC.13's session and loads its three captures by name, so other captures landing there don't change what it compares; `rrc-dump` refuses any output dir inside a work tree of this repo and its error points at `$GUAC_CAPTURE_DIR` ([`tools/rrc-dump/src/output.rs`](tools/rrc-dump/src/output.rs)). [`crates/guac-formats/tests/giants_capture_texels.rs`](crates/guac-formats/tests/giants_capture_texels.rs) (AC.21) reads its four upgrade-art captures by name from `GUAC_AC21_CAPTURE_DIR`, its own key so AC.13's pins stay put. All four keys are in [`.env.dev.example`](.env.dev.example).
- **The oracle** (dev only, Mac only until the Windows/HTPC bring-up, every path outside the tree): `GUAC_ORACLE_EXE` is the oracle binary (`.../RPCS3.app/Contents/MacOS/rpcs3`, where `rpcs3-patches/oracle/build.sh` installs it) and `GUAC_ORACLE_HOME` its isolated HOME (`oracle home init` builds it from the dev's RPCS3 dir; a savestate boots only in a home holding its trophy files, `dev_hdd0/home/00000001/trophy/NPWR03657_00` from the home it was taken in, which init doesn't copy). `GUAC_ORACLE_SAVESTATE` is a gameplay state this build accepts (what `live_pine` boots, and `live_pad`, `live_rng`, `live_actors` and `oracle divergence run` replay); `GUAC_ORACLE_FIGURE` the `.sky` `live_gameplay`, `live_verbs`, `live_pad`, `live_rng`, `live_actors` and the runner copy onto the portal, fresh each boot; `GUAC_CAPTURE_DIR` also takes the output of the live tests that write any (`live_verbs`, `live_pad`, `live_rng`, `live_actors`: `$GUAC_CAPTURE_DIR/BLUS30968/<af…>-<unix time>/`, `run.txt` opening on the identity line), while the runner writes where `--out` says; `GUAC_ORACLE_DIVERGENCE` and `GUAC_ORACLE_CALIBRATION` are AF.11's and AF.12's output dirs the `#[ignore]`d `calibration` test reads. All of those from the process env, else `.env.dev`, and the live tests also need `GUAC_GIANTS_DUMP`. Process env only: the live tests' `ORACLE_READY_SECS`, `ORACLE_SAMPLE_SECS`, `ORACLE_LEVEL_SECS` and `ORACLE_IDLE_SECS` (waits), `ORACLE_SHADER_MODE` (the home's shader mode) and `ORACLE_AUDIO` (sound on; silent by default), and `GUAC_FLIP_REF`, the NVlabs/flip clone `guac-oracle`'s `#[ignore]`d `flip_reference` reads. `cargo run -p oracle-harness --bin oracle` prints the CLI (`home init`, `boot`, the O-verbs, `pad-load`, `pine`, `rng`, `actors`, `goto`, `session`, `divergence run|report|calibrate|check`); `session --plan <plan.ron>` (AG.6) cold-boots a home (or a state, or attaches to `boot --hold`) and runs menus, figures and both pads with the game running, every guest file it opens stamped with the flip; `goto --route <route.ron>` (AH.7) drives a fresh boot to a point closed loop, pausing every few flips to steer through O4, and can save the state it settles in; `divergence run --perturb <change>` makes AF.12's perturbed walks, and `--gap-secs` (30) waits after each RPCS3 exits, because a boot straight after one hangs on a CoreMIDI client error; the runner stops if a replay changes the home's saves or trophies, which a state's load doesn't put back. Never connect to the oracle's GDB stub (2346): a connection pauses it. Close a PINE connection before any `SAVESTATE` or shutdown: an open one blocks every stop.
- **`E2E_APP=guacamole`** runs the phone e2e suite against guacamole instead of the product (default `product`); product-only tests skip with a message. See `crates/e2e-tests/README.md`.

## macOS support

- **Production target on both Windows and macOS.** Both drive a real
  patched RPCS3 over the cross-platform AF_UNIX **IPC** path
  (`IpcPortalDriver`); UIA is the Windows-only fallback for a *stock*
  RPCS3. The Phase-16 IPC pivot removed the Windows lock-in, so the
  same control path works on macOS — `UnixRpcsProcess` is wired in
  (PLAN 16.11) and `SKYLANDER_PORTAL_DRIVER=ipc` spawns/drives the
  patched Mac binary (build: `.ci-local/build-mac.sh`; run:
  `docs/dev/macos-rpcs3-build.md`). There is **no AXUIElement-based
  driver** — that's an explicit non-goal; IPC supersedes it.
  macOS status (Phase U, shipped v1.9.19+): (1) the **shipped Mac
  release BUNDLES the patched RPCS3** (nested in the signed `.app`,
  pulled from `rpcs3-patched-macos-arm64`) and **defaults to IPC** — the
  wizard persists `Ipc`, so a fresh install drives the real emulator.
  `SKYLANDER_PORTAL_DRIVER=mock` is the explicit **demo-mode** override
  (in-memory portal: demo, family-member play, iOS-Sim e2e, no
  firmware). **Dev `cargo run` mirrors prod** — the compile-time default
  is IPC on both shipping targets (Win + macOS); only Linux/non-target
  builds default to Mock (`config.rs` cfg-gate). `.env.dev` sets `=ipc`,
  `.env.dev.mock` sets `=mock` for a no-emulator demo. (2) **window
  coordination on macOS is surface-embed** (P8 /
  `crates/server/src/compositor.rs` — the game's `CALayer` is hosted
  INSIDE the launcher window via `CALayerHost`, one window, egui frames a
  rect around it), NOT the Win32 `SetWindowPos` sibling-window path. Mac
  users get the same SPA + launcher surface, now driving a real game.
- Dev: `cargo run -p skylander-server` boots cleanly with
  `SKYLANDER_PORTAL_DRIVER=mock` + a sentinel `RPCS3_EXE` path. The QR
  URL uses Bonjour (`<LocalHostName>.local`, read via `scutil --get
  LocalHostName`) so PWA bookmarks survive DHCP changes the same way
  they do on Windows.
- Full dev bringup steps + expected log lines:
  `docs/dev/macos-bringup.md`.
- iOS Simulator iteration: the [`ios-inspect` repo](https://github.com/chotchki/ios-inspect). The CLI
  drives `xcrun simctl` + `ios-webkit-debug-proxy` for layout/CSS
  probes against booted simulators; multi-device (iPad + iPhone)
  in parallel is supported (PLAN 10.2 — `ios-inspect boot
  --device <udid>` per simulator, then `--device` on every
  read/eval/screenshot).
- Release artifact: a **signed + notarized + stapled `.dmg`** (a nested
  `.app` bundling the patched RPCS3), attached to GitHub Releases
  alongside the Windows zip (PLAN 10.9.2/.4 — Apple Developer ID import +
  `xcrun notarytool` + `stapler`, live as of **v1.9.19**). Gatekeeper
  opens it on a normal double-click; no right-click + Open dance.

## Error handling

- GUI-drive failure: silent retry up to N times, then error toast on phone. Start simple, iterate.

## Distribution

- GitHub Releases zip + winget MSI. Do **not** bundle `.sky` files or game/firmware
  content (no piracy).
- **Users supply their own** PS3 firmware + game backups, via their **existing RPCS3
  install** (the v1 model, below).
- **RPCS3 is vendored + patched (Phase 16).** `vendor/rpcs3` is a git submodule pinned
  to a pristine upstream commit; the IPC patches — **P1** (AF_UNIX portal control on the
  emulated Skylander device) and **P2** (borderless, no-focus-steal game window +
  native-handle publishing) — live in `rpcs3-patches/` (apply via
  `rpcs3-patches/apply.sh`; README has the pin + rebase procedure). This makes the repo
  **GPL-2.0-only** (RPCS3 is GPLv2; the patches are a derivative).
- **v1 distribution model (decided 2026-05-30 — PLAN 16.9.0b).** The IPC control path
  needs *our patched* RPCS3 (stock RPCS3 has no IPC listener), which users can't supply
  themselves, so the Windows release **bundles the patched RPCS3 build** next to the app
  at `<app>/rpcs3/rpcs3.exe` (legal under GPL — source is public; the binary comes from
  the gated `rpcs3-patched.yml` Windows build). For **firmware + games**, the user still
  **points the first-launch wizard at their existing RPCS3 install**; that becomes
  `config_dir` (RPCS3's data/config root — installed firmware + `config/games.yml`),
  decoupled from the bundled control binary (the `config_dir` decoupling, 16.9.0). So v1
  ships no firmware/games and writes no RPCS3 config (16.9.1/.2 deferred). **macOS now ships
  the same way** (Phase U, v1.9.19+): the signed/notarized `.dmg` bundles the patched RPCS3
  (nested in the `.app`) and defaults to IPC; firmware + games still come from the user's own
  RPCS3 install. The mock driver stays as the `SKYLANDER_PORTAL_DRIVER=mock` demo-mode override.
- Steam Big Picture shell behavior is a compatibility-pass concern, not a day-1 constraint.

## Git workflow (pre-1.0)

- **`2.0` is the working main** (2026-10-17). Guacamole and all OpenSpec work happen on the `2.0` branch, which acts as main until some milestones are through — chotchki: "Commit freely on a branch, we'll use a 2.0 branch as our 'main' until we've gotten through some milestones." `main` holds the frozen product line.
- **Commit + push directly to the working branch** (`2.0`, above). This is a solo project with no external developer coordination; GitHub PRs are pure friction at this stage. Skip them.
- **Pre-push hook** (PLAN 10.5.5) at `.githooks/pre-push` runs `cargo fmt --check` + `cargo check --workspace` + `cargo clippy --workspace --all-targets -- -D warnings` + `cargo test --workspace` — the full CI lint+test lane mirrored locally (clippy added 2026-06-11 after a clippy-only example regression slipped to CI *after* a release tag was already cut). Activate with `git config core.hooksPath .githooks`; bypass with `git push --no-verify` for WIP branches.
- **CI lanes** documented in `docs/dev/ci.md`. The iOS-Simulator e2e lane is label-gated to `run-ios-sim` on PRs (and runnable via manual `workflow_dispatch`) — running it on every PR would dominate the macOS-minutes budget.
- Reserve PR ceremony for post-1.0 or for cases where a human reviewer genuinely adds value (e.g. first-time CI-bring-up or a dangerous rewrite where the diff view is the point).
- Concurrent subagents modifying overlapping files → spawn with `isolation: "worktree"` so they don't entangle WIP; merge their branches into the working branch (`2.0`) locally when done.

## RPCS3 window/menu gotchas (UIA fallback — see `docs/research/game-launch-window-mgmt.md`)

> These apply to the **legacy UIA driver** (`SKYLANDER_PORTAL_DRIVER=uia`, stock RPCS3).
> The production IPC path (Phase 16, patched RPCS3) has **no Skylanders Manager dialog and
> no menu nav** — it drives `g_skyportal` directly over AF_UNIX and reads liveness from the
> `STATE`/heartbeat, so most of these don't apply there. Kept because UIA is the fallback.

- While a game runs, RPCS3 has **multiple top-level Qt windows that all share the same generic class** `Qt6110QWindowIcon` — main (title prefix `"RPCS3 "`), game viewport (title prefix `"FPS:"`), and the Skylanders Manager dialog (title `"Skylanders Manager"`) are only distinguishable by **title**, never by classname. Anything that grabs windows by class will mis-match. Older RPCS3 builds had distinct classes (`main_window`, `skylander_dialog`); current Qt 6.11 unified them.
- **UIA pattern menu nav works on current Qt 6 RPCS3** (verified 2026-05-03 against `0.0.40-19296` via `tools/uia-probe`). Every `MenuItem` advertises `Invoke`, `ExpandCollapse`, `LegacyIAccessible`, and `Value`. The Manage → Portals and Gates → Skylanders Portal chain reduces to three pattern calls — no keystroke synthesis, no focus dependency, no Steam Overlay competition. Submenus populate lazily on `ExpandCollapse.expand()`, so re-walk the tree (or wait ~100ms) before searching for child items. Older notes claimed Qt 6 didn't honour these patterns — that was true at the time, but no longer.
- **Dialog opens once per RPCS3 session** — `open_dialog` navigates the menu on first call, then keeps the Skylanders Manager off-screen for the rest of the session. Subsequent calls short-circuit. If RPCS3 restarts, first `open_dialog` re-does the nav (brief once-per-session flicker during boot).
- **Modal vs non-modal Qt action invokes.** `UIInvokePattern.invoke()` on a menu item that triggers a *non-modal* dialog (like Skylanders Manager) returns immediately. On an item that triggers a *modal* (like File → Boot Game's QFileDialog), the call **blocks until the modal closes** — Qt's `exec()` event loop runs synchronously. Don't drive modal-popping menu items from the main thread without a worker.
- **Focus thieves are no longer a problem for menu nav** (UIA patterns don't need focus). Still relevant for any remaining keystroke-driven path: the game viewport, RPCS3's **update-check popup** at boot — tell users to disable Settings → Advanced → "Automatically check for updates at startup".
- `RPCS3.buf` singleton lockfile next to `rpcs3.exe` survives forced kill → next launch fails. `RpcsProcess::shutdown_graceful` deletes it after the `Forced` path. Spawned processes are also wrapped in a Win32 Job Object with `KILL_ON_JOB_CLOSE` so RPCS3's re-exec shims and worker children don't leak across test runs.
- **Booting a game programmatically:** pass the game's `EBOOT.BIN` to `rpcs3.exe` as the first CLI argument (`UiaRpcsProcess::launch_with_eboot`). Direct-boot used to break menu-bar nav via synthesised keystrokes — that caveat was keystroke-specific and no longer applies now that the menu nav is pattern-driven. The serial → game directory map lives in `<rpcs3>/config/games.yml` (flat YAML); EBOOT.BIN is at `<game_dir>/PS3_GAME/USRDIR/EBOOT.BIN`. The library-view + select+Enter approach is retired (PLAN 10.8.4) — fragile under load and didn't survive cell-vs-row selection ambiguity.
- **Session isolation:** all UIA + SendInput automation is session-bound. Tests that exercise the real driver must run on the user's interactive desktop — SSH connects in session 0 and cannot see/touch windows in session 2+ at all. `RpcsProcess` launches, `EnumWindows`, UIA tree walk all return empty under SSH. Run RPCS3-live tests on the physical machine.

## Naming gotchas

- Spec originally said "RPS3" — it's **RPCS3**.
- Firmware file extension is `.sky`, not `.dump`.

## See also

- `SPEC.md` — authoritative long-form spec + full Q&A history with decision rationale.
- `openspec/` — execution: `changes/<id>-<slug>/tasks.md` per phase, `backlog.md` for unphased work, `config.yaml` for the conventions.
- `PLAN_ARCHIVE.md` — the frozen pre-OpenSpec plan (phases closed before the 2026-10-17 move).
- `docs/native/guacamole-design.md` — the Guacamole track (decisions, architecture, milestone ladder, RE toolchain).
- `docs/aesthetic/` — UI reference images.
