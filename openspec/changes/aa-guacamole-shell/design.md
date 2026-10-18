# Design

## Context

The product is one binary, `skylander-portal-controller`, built from `crates/server`. Its `main.rs` does everything between "config loaded" and "eframe runs". It indexes the pack (`:67-188`), builds the phone URL and QR (`:200-241`) and creates the shared handles (`:243-299`). Then it spawns the tokio thread (`:309-645`), which constructs the driver, opens `db.sqlite`, starts the worker and watchdogs, reads `games.yml`, assembles `AppState`, binds the listener (`:461`) and serves. The library half (`crates/server/src/lib.rs`) already exposes every module, but it also exposes the eframe ones (`ui`, `badge`, `vortex`, `wizard`, `compositor`, `gl_fallback`), and a few core modules reach into them:

- `config.rs:246,281` calls `wizard::run_wizard_blocking` and reads the wizard's `PersistedConfig`. `http.rs:960,1010` read it too.
- `paths.rs:22` reads `gl_fallback::APP_DIR_ENV`.
- Everything else points the other way (UI → core).

A second set of edges matters because only what guacamole runs moves (decision 1): `http.rs` and `state.rs` call `sky_edit`, `sky_stats`, `working_copies`, `display_mode` and `rpcs3_config` (`http.rs:285-299`, `:661`, `:773`, `:1158`, `:1323`; `state.rs:480`, `:1021`, `:1600`), all of which stay with the product.

The driver seam is `PortalDriver` (`crates/rpcs3-control/src/lib.rs:23`): `load(slot, path)` gets the profile's `.sky` working copy, `clear(slot)`, `read_slots()`, plus IPC-only methods with defaults. The HTTP handler forks the working copy (`working_copies::resolve_load_path`, `http.rs:661`) before it queues `DriverJob::LoadFigure { path, … }`, and the worker (`state.rs:1414`) runs each load in `spawn_blocking`, then overwrites the driver's reported name with the catalogue's canonical name and broadcasts `SlotChanged`.

The figure list is the firmware pack. `skylander_indexer::scan` (`main.rs:76-81`) walks it: name from each dump's file name, game, element and category from its folder path, id from the dump's tag header (`<toy type>-<variant>`, `0007d6-0000`). Portraits are `data/images/<figure id>/thumb.png` (`http.rs:433-476`), keyed by that same id, and `release.yml` already stages `data/` next to the exe. Nothing reads `data/figures.json` at runtime.

Motivation is in proposal.md. The track's architecture is in `docs/native/guacamole-design.md` ("Two binaries until G1", Persistence and setup, Figures) and chotchki's 2026-10-17 calls on the database, the distribution and the split are in its Decisions; this design is the concrete cut.

## Goals / Non-Goals

**Goals:**
- One server library both binaries link, with the product's observable behavior unchanged. The product's e2e suite on the Mac is the proof, not inspection (the HTPC live smoke joins at the Windows/HTPC bring-up).
- A driver kind that means "no emulator and no figure file", with every RPCS3- or file-only branch handled explicitly rather than by accident of `cfg(windows)`.
- A guacamole that starts on a machine with nothing but disc dumps: no controller setup, no RPCS3, no firmware, no figure pack, and its own data root and database whether or not the controller is installed.
- A Bevy app that owns the TV and gets portal changes as data, ready for AB to hang intros on and AE to hang 3D on.

**Non-Goals:**
- Hosting the product's first-launch wizard in Bevy, or any eframe code in guacamole.
- Porting the 3D badge (`BadgeRig`), the vortex shader or the launch animations. The TV here is plain Bevy UI; the launcher surface is rebuilt in Bevy before G1 (design doc, G1: one app).
- Figure progression: guacamole's store, level, XP and gold on the cards and the phone's editor on that store are AH.
- Scanned figures. AD (the ACR122 removal) lands before AA and takes the reader out of the product first.
- Copying the controller's profiles, PINs or figure usage into guacamole. The kids set their profiles up again (decision 6).
- Player pips or Kaos overlays on the guacamole TV beyond each card's owner pip. The phone already shows them.
- Trimming `PortalDriver` or removing anything RPCS3 (G1).
- macOS packaging of guacamole (signing a second binary into the dmg is its own change).
- Anything checked on Windows or the HTPC (Mac first, design doc Decisions, fourth batch). The Windows MSI carrying both binaries, its firewall exception, guacamole under Steam Big Picture, the Windows CI lane's Bevy build and the HTPC live smoke are the Windows/HTPC bring-up's, a line beside the ladder in `openspec/backlog.md`. The code stays cross-platform (decision 3 keys on the driver, never on `cfg(windows)`); it just isn't checked on Windows here.
- Choosing the Windows render backend. wgpu's default here; the Windows/HTPC bring-up picks Vulkan when it adds DLSS.

## Decisions

**1. Only what guacamole runs moves to a new crate; the product keeps `crates/server` and the rest.**
chotchki's call on 2026-10-17 ("I'm good with it"): `crates/guac-server` (package `guac-server`, lib `guac_server`) takes the modules guacamole runs, and nothing guacamole doesn't run moves just to sit in a shared crate.
- Moves: `embedded_assets`, `http`, `kaos`, `logging`, `mdns`, `paths`, `profiles`, `round_qr`, `state`, the config types both loaders fill (`DriverKind`, `WindowMode` and the loaded config itself), the `BUILD_TOKEN` stamping from `build.rs` and a new `migrations/` set for guacamole's own database (decision 6).
- Stays in `crates/server`: `sky_edit`, `sky_stats`, `working_copies`, `display_mode`, `rpcs3_config`, the product's `config` loader, `wizard.rs` with its `PersistedConfig`, the product's `migrations/` (frozen at its three), `main.rs`, the eframe modules, the WiX metadata and the `winresource` icon stamping. `sky-stats` (the sky-parser dependency) stays a feature of `crates/server` the product turns on as it does today; guacamole can't reach it. AD (the ACR122 removal) lands before AA, so `nfc`, `nfc-import` and the `scan:` arms are already gone and the split has none to leave behind.
- `APP_DIR_ENV` moves into `paths`. `firewall` (the launcher's connection diagnostic, read by `state.rs:1387`) goes wherever the cut compiles cleanest, recorded in AA.1.
- Hooks: where moved code calls a module that stays, it calls a trait instead, and the product passes the implementation to `boot()` (decision 2). Four of them: figure files (the stats, edit and reset routes, the slot reset and the working-copy fork or rewrite on a load, Kaos swap or auto-restart restore), the display mode (pre-set and capture around a launch), the RPCS3 config writes (the Net config at boot, the savestate config at `state.rs:1600`) and the product's settings writer (the window-mode and 2× toggles, `http.rs:948`, `:998`). guacamole passes none, which is its Native arm at those sites (decision 3).

The split is by module. `http` and `state` still carry RPCS3-shaped branches both binaries compile (the IPC spawn and poller, UIA's `BootDirect` arm), with Native arms, until G1 removes them.

Alternatives:
- (a) Feature-gate the eframe modules inside `skylander-server`. Rejected: `cargo build --workspace` unifies features, so guacamole would link eframe, glow and the Win32 window code in every workspace build, and the boundary would be a cfg soup that the compiler can't hold anyone to. A crate boundary is a type-checked boundary.
- (b) Keep `skylander-server` as the library and move the product binary to a new crate. Rejected: it churns `release.yml` (`-p skylander-server` at `:105`, `:463`), the WiX step that runs from `crates/server` with `../../wix/*.wxs` includes (`:334-336`), `ci.yml:194` and the e2e harness (`crates/e2e-tests/src/lib.rs:81-87`) for no gain.
- (c) Move every non-UI module, product-only ones included, and guard those with features and Native arms. Rejected by chotchki's call: `sky_edit`, `sky_stats` and `working_copies` would move only to be product-only code in a shared crate.
- (d) `guacamole` links today's `skylander-server` lib as it is, eframe and all. Rejected with (c): guacamole would compile and link eframe, glow, egui and the Win32/objc2 window code it never runs.

`rust-embed`'s `#[folder = "../../phone/dist"]` (`embedded_assets.rs:30`) stays valid from `crates/guac-server`. The product's release command line doesn't change. The name follows the codename rule (new crates are `guac-*`, never `skylander-*`): the product links it until G1 and `guacamole` keeps it after, so it's Guacamole's server from day one. The existing `skylander-*` packages keep their names.

"Product untouched until G1" reads as behavior-untouched, and it carries more weight than it did: the next tag ships this source's product, and the bring-up's MSI installs it on the HTPC (proposal, Release reach). The moves and the hook seams land before any Native code and pass the product's e2e suite on the Mac at its pre-AA count (AA.1, AA.2).

**2. Boot is one library call, and the listener binds first.**
`boot(options) -> Result<ServerHandles>` takes six things:
- the loaded config;
- the database: its path under the binary's own root and the migration set to run (the product's `db.sqlite` with its own three, or guacamole's, decision 6);
- a driver factory, run ON the tokio thread (UIA's `UIAutomation::new()` initializes COM as MTA there, `main.rs:259-262`);
- the product hooks (decision 1), all present for the product and all absent for guacamole;
- optional extra routes, merged into the router (guacamole's test hook);
- a catalogue policy: the pack index plus every game `games.yml` lists for the product, or decision 5's interim catalogue plus the games decision 6's setup found for Native.

It returns the handles both UIs read: launcher status, the client counter, the phone and raw-IP URLs, the QR PNG, the event sender and the worker's job sender. The job sender is what PLAN B.2's back-channel (`main.rs:302-308`, `:704-707`) ships across today.

Inside the tokio thread the listener binds before anything with a side effect: before the RPCS3 Net config write (`main.rs:341`), the driver build and the database open. The port is the single-instance lock: it's the resource both apps actually contend for (the join address), the OS releases it if the holder crashes and it costs nothing. It works across the two apps because both use 8765, the port the controller's setup always writes (`wizard.rs:278`) and guacamole's default. A lockfile was the alternative, but the two apps no longer share a runtime root to put one in, and it's a second source of truth that can disagree with the port. A bind failure goes to the same `report_fatal` → `LauncherScreen::ServerError` path as today, now before the database is touched.

Config loading splits by binary. The product keeps `config::load` with its wizard. guacamole has its own loader (decision 6) that reads only the keys it uses and never requires `RPCS3_EXE` or `FIRMWARE_PACK_ROOT`.

**3. `DriverKind::Native`, with an explicit arm everywhere RPCS3 or a figure file is assumed.**

| Site | Product today | Native |
|---|---|---|
| `main.rs:76-187` | indexes the pack, decorates vehicle terrain | skipped; the interim catalogue (decision 5) |
| `main.rs:341` | writes RPCS3's Net config unless Mock | skipped |
| `main.rs:419-430` | state poller for IPC only | skipped (already) |
| `main.rs:479-487` | Mock installs `RpcsProcess::mock()` | nothing installed; no process is ever tracked |
| `main.rs:521-540` | catalogue from `games.yml` (Mock: all six) | Giants only, when setup found a BLUS30968 dump (decision 6) |
| `http.rs:661` (load), `:773` (slot reset), `state.rs:480` (Kaos swap), `:1021` (restore) | fork or rewrite the profile's `.sky` working copy, then load it | no file; the job carries the placement (decision 4); slot reset answers 409 |
| `http.rs:285-299` (figure stats, edit, reset) | read, edit or reset the working copy (`sky-stats` feature) | 409 whether or not the feature is compiled in |
| `http.rs:1156-1177`, `:1323` | pre-set and capture the per-game Windows display mode | skipped both ways |
| `http.rs:1191` | EBOOT resolution unless Mock | skipped |
| `state.rs:1563-1742` (`BootDirect`) | IPC spawn, else on Windows a UIA spawn for any non-IPC driver | no spawn on any OS; reveal at once, as Mock does off Windows |
| `http.rs:948`, `:998`, `:1629` | rewrite `config.json` (window mode, 2×) / open RPCS3 settings (409 for Mock) | 409 |

The `BootDirect` row is the dangerous one. The non-IPC arm is gated on `cfg(windows)`, not on the driver, so anything without an IPC socket gets a real RPCS3 launch on Windows (the HTPC, once it's brought up). The arms key on `DriverKind` (threaded into the job), never on platform. The rows whose code stays in `crates/server` (the working-copy, figure-stats, display-mode, RPCS3-config and `config.json` rows) are hooks (decision 1): guacamole passes none, so the step is skipped or the route answers 409. The routes stay in the shared router and answer 409 without the hook, so the phone gets the same "unavailable" answer from guacamole whatever features a workspace build unified.

A catalogue record has no file under Native: its file path stays empty, and every arm above that would read it is skipped. Making the path optional in the shared figure type waits for G1 with the rest of the file-shaped API; AA.3's test is what holds it.

**4. The native driver places by identity and sends placements over a channel.**
`DriverJob::LoadFigure` carries `path: Option<PathBuf>`: `Some` for the RPCS3 drivers and the mock, exactly as today, and `None` under Native, where nothing forks a working copy. The worker calls `load(slot, path)` for `Some` and a new trait method `place(slot, &Placement)` for `None`. `place` defaults to an error ("this driver loads figure files"), the way `window_set` defaults to one, so the RPCS3 drivers and the mock don't change. `NativePortalDriver` (in `crates/guacamole`) implements `place`, and its `load` returns an error, so a stray file path under Native fails the placement where it can be seen instead of opening a file.

The load handler builds the `Placement` from the catalogue and the profile store: the figure id plus its toy type and variant (parsed from the id, `ToyTypeId`/`TagVariant` in `crates/core/src/figure.rs:24,50`), the name and element, the portrait path (server-private, never sent to the phone) and the owner (the placing profile's id, initial and colour, or none when no profile is unlocked). Toy type and variant ride along because AB keys the character's intro on the toy type (and AE its model), not on the catalogue's id string.

The driver sends `Placed { slot, placement }`; `clear` sends `Cleared { slot }`. It keeps its own 8-slot array for `read_slots`, like the mock. `open_dialog` is a required trait method, so it gets a no-op; the IPC-only methods keep the trait's defaults, as `MockPortalDriver` does (`mock.rs:92`).

Rejected: encoding the id as a fake path (`load(slot, "0007d6-0000")`). It's a lie the type system can't catch, and the mock or an RPCS3 driver handed one would try to open it.

The channel is a bounded `crossbeam-channel` (`Receiver` is `Sync`, so it sits in a plain Bevy `Resource`). A `PreUpdate` system drains it into a `PortalCards` resource; the UI reconciles entities from that. The channel only carries data, so a Bevy-side bug can't stall the worker. A full channel drops the message and logs, which costs at most a stale card until the next change, never a hung load.

Test hooks: `AppState.test_mock: Option<Arc<MockPortalDriver>>` (`state.rs:112`) becomes an injector trait that both the mock and the native driver implement. `/api/_test/inject_load` then works against guacamole and the existing suite runs unchanged.

**5. The interim catalogue: the committed wiki data, validated.**
Measured on the committed data on 2026-10-17. `data/figures.json` has 489 rows keyed by the tag-form id; 331 carry a name and 303 an element. Across the toy types it gives Spyro's Adventure and Giants characters (49 of them), 45 base rows (variant 0000) carry a name, an element and a Spyro's Adventure or Giants `firstrelease`. Of the other four, 0x06, 0x15 and 0x64 carry nothing and 0x0D carries another page ("Crabs"). 19 names sit on more than one toy type, because the rekey from wiki pages to tag ids put some pages on the wrong rows: "Elite Voodood" on 15 toy types, "Bumble Blast" on 24, "Cynder" on Spyro's (0x10) 0x0801 row. Every figures.json row in those character toy types has a `thumb.png`.

So guacamole builds its catalogue at boot from `data/figures.json` and `data/images/` with these rules:
- a row is in when its `firstrelease` is Spyro's Adventure or Giants (Giants plays both, the origin-game-and-later rule in `crates/core/src/compat.rs:48`);
- a base row needs a name; a variant row's name must also contain its own toy type's base name, which drops the mis-keyed rows (a "Cynder" on Spyro's toy type) and keeps real variants ("Legendary Ignitor", "Polar Whirlwind");
- the phone's existing fields come from the row: game from `firstrelease`, category from `alignment`, element read case-insensitively (one row says `life`), variant group from the base name and variant tag from the rest of the name;
- a short supplement in guacamole's own code fills the base characters the data lacks or mislabels (the four above), with names and elements written by hand and checked by chotchki;
- the portrait is `data/images/<id>/thumb.png` when it exists.

Wrong is worse than missing: a TV that calls Spyro "Cynder" is the failure, a missing repose isn't. AA.4 records what the rules kept and dropped. The list is the same for every profile (no per-kid shelf, design doc Figures). AB's disc test cross-checks it against Giants' character archives and AE replaces it.

Alternatives: reading the product's pack index (rejected, it's the firmware pack); committing a new hand-made roster file (rejected, it's a third copy of data the disc is about to replace).

**6. Setup is one question, and guacamole keeps its own data root and database.**
chotchki's call on 2026-10-17: guacamole gets its own SQLite, and the profiles aren't copied ("set them up again, it's super simple"). guacamole never opens the controller's `db.sqlite` or `config.json`.
- Data root (the runtime-state directory; not the product's `DATA_ROOT` key, which points at the committed wiki data and which guacamole also reads for decision 5's catalogue): `%LOCALAPPDATA%\guacamole\` on Windows, machine-local because AB's gigabyte extract dir lives under it, and `~/Library/Application Support/guacamole/` on macOS. Dev uses `./dev-data/guacamole/`, gitignored with the rest of `./dev-data/` and separate from the product's files there. `paths` takes the root as a boot option, so the product's root (`%APPDATA%\skylander-portal-controller\`, dev `./dev-data/`) doesn't move.
- Database: `<root>/db.sqlite`, opened by the same profile store with guacamole's own migration set (`crates/guac-server/migrations/`). It starts with the product's profile tables (profiles, sessions, figure usage, the Kaos toggle; same shape, so the store's checked queries serve both files) and a settings table, and grows from AH on. The product passes its own three migrations from `crates/server/migrations/`, so nothing guacamole adds can reach `db.sqlite`, whose migrator would refuse it (design doc, Persistence and setup).
- Profiles: a first start has none. The phone's existing create-profile flow (name, colour, PIN) makes them and the existing PIN unlock and lockout apply, now against guacamole's database. Nothing is copied from the controller, and nothing guacamole does changes the controller's profiles.
- Settings, in that database: the dumps folders, the port (8765 by default, the controller's own, so decision 2's port lock covers both apps) and a pairing key generated on the first start and reused after. Dev reads `GUAC_DUMP_DIRS` (a list in the OS's path-list form), `BIND_PORT`, `PHONE_DIST`, `DATA_ROOT` and `WINDOW_MODE` from `.env.dev` over them; the dev key lives in the dev database like the release one.
- Pairing: the key is guacamole's own, so the join address (host name and port) is the same as the controller's but the key isn't. A phone that last joined the controller sends a key guacamole rejects, gets the phone's existing pairing screen (`phone/src/components/pairing_required.rs`) and scans guacamole's join code once.
- Release setup: when no dumps folder is set, the TV's setup screen asks for one and opens a native folder picker on the machine running it (a grown-up with a keyboard and mouse, once). `guacamole --setup` asks again, and `guacamole --setup <folder>` stores a folder without the picker.
- The scan looks for `PS3_GAME/PARAM.SFO` in each chosen folder and up to two levels below it, reads TITLE_ID with AB.2's parser and reads nothing else from the dump. The TV lists every dump it found by title, and Giants (BLUS30968) is the one guacamole offers. The phone never sees a path.

**7. Bevy app shape.**
- Bevy 0.20.0 (pinned by `Cargo.lock`; upgrades are their own change at milestone boundaries).
- `LogPlugin` off: the library's `logging::init` owns the global tracing subscriber (two would fail `set_global_default`), and the e2e harness scrapes its "serving on" line.
- One window: borderless fullscreen in TV mode, a resizable window when `.env.dev` says `WINDOW_MODE=desktop` (the product's existing knob). `windows_subsystem = "windows"` outside `dev-tools`, as in the product (`main.rs:7`).
- UI in `bevy_ui`, laid out at 1080p and scaled by `UiScale` from the window height, so the 86" 4K TV at 10 ft gets 2×. Titan One, the same face the phone and launcher use, comes from `crates/server/assets/fonts/`.
- A `ServerBridge` plugin polls `LauncherStatus` each frame (one clone of a sync mutex, as eframe does) and drains the portal channel. A `Tv` plugin owns the setup, join and portal screens.
- Cards sit in two rows of four, so eight always fit at 16:9, inside the same 6% overscan inset the launcher reserves in fullscreen (`TV_OVERSCAN_FRAC`, `crates/server/src/ui/main_screen.rs:125`). The 86" TV crops its edges (R.1), and "each fully visible" in the figure-cards spec means visible on that TV. Each card carries an owner pip in the placing profile's colour with its initial.
- `LauncherScreen::Farewell` sends `AppExit` after the farewell beat. `ServerError` shows its message.
- The guacamole crate allows `clippy::type_complexity` and `clippy::too_many_arguments` crate-wide (Bevy system signatures trip both by design); everything else stays at `-D warnings`.

Release builds each binary in its own `cargo build -p … --no-default-features` step. A combined invocation would unify `guac-server`'s features (`dev-tools`, `test-hooks`, the mock driver) across both packages.

**8. e2e against either binary.**
`TestServer::spawn*` reads `E2E_APP` (`product` default, or `guacamole`) and runs `cargo run -p <pkg> --features test-hooks`. For guacamole it skips the firmware-pack lookup and points `GUAC_DUMP_DIRS` at a temp dir holding a synthetic `PS3_GAME/PARAM.SFO` for BLUS30968, built in code at run time with AB.2's test-only builder. AA never opens anything else on a dump, and AB's identity check sees that dir as "no game data", which leaves cards working.

guacamole adds `/api/_test/tv` through the extra-routes hook. A `PostUpdate` system mirrors the TV's screen and cards into an `Arc<Mutex<TvSnapshot>>`, and the hook returns it as JSON, so an e2e test asserts on what the TV shows without screenshots.

## Risks / Trade-offs

- [The split changes the product without anyone noticing, and the next tag ships that product (the bring-up's MSI installs it over the frozen controller on the HTPC)] → AA.1 and AA.2 are moves plus hook seams with the product's hooks wired to the code that was there, with the unit-test count equal before and after, and they land and pass the product's e2e suite on the Mac at its pre-AA count BEFORE any Native code exists. That suite runs the mock driver, so the product's IPC spawn and poller, which move with `state`, aren't exercised by AA; the bring-up's HTPC live smoke and its check of the installed product from the MSI cover them before anyone plays. A bad product release is fixed forward with a patch release (tags are never deleted).
- [A Native arm is missed and something opens a figure file with an empty path] → AA.3 runs a load, a slot reset, a Kaos swap and an auto-restart restore under Native and asserts nothing was created under the runtime root and `load(slot, path)` was never called. The native driver's `load` errors, so a miss shows up as a failed placement, not a silent one.
- [The interim catalogue shows a figure under the wrong name] → decision 5's rules plus a test over the committed data (no name on two toy types, every entry named). AB's cross-check against the disc finds what the rules can't.
- [The native folder picker opens behind the fullscreen window] → AA.11 checks it on the Mac driving the TV, and the bring-up checks it again under Steam Big Picture. The fallback is `guacamole --setup <folder>`, which AA.12 documents either way.
- [Adding Bevy to a workspace that already has eframe 0.29 hits a `links` conflict or a duplicate-native-lib clash] → AA.6 starts by adding the dependency and running `cargo build --workspace --all-features` on the Mac before writing app code. The macOS CI lane has been `if: false` since 2026-05-30 (`ci.yml:150-155`), so the Mac is covered by hand and by `.githooks/pre-push`, not by CI. A conflict that only shows on Windows surfaces at the bring-up's first Windows CI run. Duplicate pure-Rust versions (winit, wgpu, raw-window-handle) are fine across two binaries.
- [Bevy makes `.githooks/pre-push` (and later CI) slower] → AA.10 measures the cold and warm deltas on the Mac's pre-push run and records them. The Windows lane's delta is the bring-up's (`Swatinem/rust-cache` already caches it, one `shared-key` per OS, `ci.yml:60-64`, not per feature set). If the pre-push hook gets painful, the fix is its own change, not a quiet skip of the guacamole crate.
- [The OS firewall asks to allow `guacamole` on the network the first time it listens, hidden behind the fullscreen window] → on the Mac, AA.11 records whether macOS asks. On Windows it's the bring-up's: the MSI's port-8765 rule (`wix/main.wxs:108-115`) keeps the product from prompting and should cover `guacamole.exe` on the same port; unverified, so the bring-up records whether Defender prompts and, if it does, adds a program-scoped exception to the MSI.
- [A second instance on a different port slips past the port lock] → only possible with a dev `BIND_PORT` override or a controller whose `config.json` was hand-edited off 8765. AA.11 starts guacamole on the Mac with the product running and expects the already-running screen.
- [Kids lose their profiles, or mix them up, across the two apps] → nothing is lost: the controller keeps its profiles untouched, and guacamole's are new ones the kids set up once (chotchki's call). The phone's profile picker shows only the running app's profiles, and a profile created in one never shows in the other (shell spec).
- [Bevy's borderless fullscreen misbehaves on the TV (focus, scaling)] → AA.11 runs it on the Mac driving the TV; Steam Big Picture's overlay on the HTPC is the bring-up's. The eframe launcher's always-on-top tricks aren't needed with one window.
- [The phone gate changes the product's phone] → the product's `figure_edit.rs` e2e passes unchanged, and the gate only disables actions whose stats couldn't be read, where an edit couldn't have worked either.

## Migration Plan

AA.1 and AA.2 land first as a pure refactor. Rollback is a revert, and nothing persisted changes shape. The Native work and the guacamole crate land after. guacamole adds its own data root and touches nothing the controller owns; rolling it back is a revert plus deleting that root (the guacamole profiles go with it). Once a tag has shipped the product built from this source, rollback for the product is a patch release, not a revert.

## Open Questions

None of the design doc's open questions block AA. The ones AA waited on (which database, how guacamole reaches the HTPC, how much of `crates/server` moves) were settled on 2026-10-17 (design doc, Decisions, third batch), and the fourth batch moved reaching the HTPC to the Windows/HTPC bring-up.
