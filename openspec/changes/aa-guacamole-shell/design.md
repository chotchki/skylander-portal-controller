# Design

## Context

The product is one binary, `skylander-portal-controller`, built from `crates/server`. Its `main.rs` does everything between "config loaded" and "eframe runs". It indexes the pack (`:67-188`), builds the phone URL and QR (`:200-241`) and creates the shared handles (`:243-299`). Then it spawns the tokio thread (`:309-645`), which constructs the driver, opens `db.sqlite`, starts the worker and watchdogs, reads `games.yml`, assembles `AppState`, binds the listener (`:461`) and serves. The library half (`crates/server/src/lib.rs`) already exposes every module, but it also exposes the eframe ones (`ui`, `badge`, `vortex`, `wizard`, `compositor`, `gl_fallback`), and a few core modules reach into them:

- `config.rs:246,281` calls `wizard::run_wizard_blocking` and reads the wizard's `PersistedConfig`. `http.rs:960,1010` read it too.
- `paths.rs:22` reads `gl_fallback::APP_DIR_ENV`.
- Everything else points the other way (UI → core), so the cut is clean once those three move.

The driver seam is `PortalDriver` (`crates/rpcs3-control/src/lib.rs:23`): `load(slot, path)` gets the profile's `.sky` working copy, `clear(slot)`, `read_slots()`, plus IPC-only methods with defaults. The HTTP handler forks the working copy (`working_copies::resolve_load_path`, `http.rs:661`) before it queues `DriverJob::LoadFigure { path, … }`, and the worker (`state.rs:1414`) runs each load in `spawn_blocking`, then overwrites the driver's reported name with the catalogue's canonical name and broadcasts `SlotChanged`.

The figure list is the firmware pack. `skylander_indexer::scan` (`main.rs:76-81`) walks it: name from each dump's file name, game, element and category from its folder path, id from the dump's tag header (`<toy type>-<variant>`, `0007d6-0000`). Portraits are `data/images/<figure id>/thumb.png` (`http.rs:433-476`), keyed by that same id, and `release.yml` already stages `data/` next to the exe. Nothing reads `data/figures.json` at runtime.

Motivation is in proposal.md. The track's architecture is in `docs/native/guacamole-design.md` ("Two binaries until G1", Persistence and setup, Figures); this design is the concrete cut.

## Goals / Non-Goals

**Goals:**
- One server library both binaries link, with the product's observable behavior unchanged. The product's e2e suite and the HTPC live smoke are the proof, not inspection.
- A driver kind that means "no emulator and no figure file", with every RPCS3- or file-only branch handled explicitly rather than by accident of `cfg(windows)`.
- A guacamole that starts on a machine with nothing but disc dumps: no controller setup, no RPCS3, no firmware, no figure pack.
- A Bevy app that owns the TV and gets portal changes as data, ready for AB to hang intros on and AD to hang 3D on.

**Non-Goals:**
- Hosting the product's first-launch wizard in Bevy, or any eframe code in guacamole.
- Porting the 3D badge (`BadgeRig`), the vortex shader or the launch animations. The TV here is plain Bevy UI; the launcher surface is rebuilt in Bevy before G1 (design doc, G1: one app).
- Figure progression: guacamole's store, level, XP and gold on the cards and the phone's editor on that store are AG.
- Scanned figures. The ACR122 reader leaves the product on its own track.
- Player pips or Kaos overlays on the guacamole TV beyond each card's owner pip. The phone already shows them.
- Trimming `PortalDriver` or removing anything RPCS3 (G1).
- macOS packaging of guacamole (signing a second binary into the dmg is its own change).
- Choosing the Windows render backend. wgpu's default here; AD picks Vulkan when it adds DLSS.

## Decisions

**1. The server core moves to a new crate; the product keeps `crates/server`.**
`crates/guac-server` (package `guac-server`, lib `guac_server`) takes the non-UI modules, `migrations/` and the `BUILD_TOKEN` stamping from `build.rs`. `crates/server` keeps `main.rs`, the eframe modules, the WiX metadata and the `winresource` icon stamping. `PersistedConfig`/`PersistedDriverKind` move from `wizard.rs` into the library's `config`, and `APP_DIR_ENV` moves into `paths`. `sky-stats` (the sky-parser dependency) and `nfc-import` stay optional features of the library: the product turns them on as it does today and guacamole turns on neither. Alternatives:
- (a) Feature-gate the eframe modules inside `skylander-server`. Rejected: `cargo build --workspace` unifies features, so guacamole would link eframe, glow and the Win32 window code in every workspace build, and the boundary would be a cfg soup that the compiler can't hold anyone to. A crate boundary is a type-checked boundary.
- (b) Keep `skylander-server` as the library and move the product binary to a new crate. Rejected: it churns `release.yml` (`-p skylander-server` at `:105`, `:463`), the WiX step that runs from `crates/server` with `../../wix/*.wxs` includes (`:334-336`), `ci.yml:194` and the e2e harness (`crates/e2e-tests/src/lib.rs:81-87`) for no gain.

`rust-embed`'s `#[folder = "../../phone/dist"]` (`embedded_assets.rs:30`) stays valid from `crates/guac-server`, and `sqlx::migrate!` moves with `migrations/`. The product's release command line doesn't change. The name follows the codename rule (new crates are `guac-*`, never `skylander-*`): the product links it until G1 and `guacamole` keeps it after, so it's Guacamole's server from day one. The existing `skylander-*` packages keep their names.

This reads "product untouched until G1" as behavior-untouched: 17 modules, the migrations and the bootstrap leave `crates/server`, and four of them (`nfc`, `sky_edit`, `sky_stats`, `working_copies`) move only to stay product-only code behind features and Native arms. If chotchki means the source too, the cheaper cut is (c), and the design doc carries it as Open question 8.
- (c) `guacamole` links today's `skylander-server` lib as it is, eframe and all, and only `boot()`, the Native arms and the interim catalogue land in it. Smallest diff to the product, no release/WiX/e2e churn. Costs: guacamole compiles and links eframe, glow, egui and the Win32/objc2 window code it never runs, and the server stays tangled with the UI modules until G1 untangles it anyway.

**2. Boot is one library call, and the listener binds first.**
`boot(options) -> Result<ServerHandles>` takes four things:
- the loaded config;
- a driver factory, run ON the tokio thread (UIA's `UIAutomation::new()` initializes COM as MTA there, `main.rs:259-262`);
- optional extra routes, merged into the router (guacamole's test hook);
- a catalogue policy: the pack index plus every game `games.yml` lists for the product, or decision 5's interim catalogue plus the games decision 6's setup found for Native.

It returns the handles both UIs read: launcher status, the client counter, the phone and raw-IP URLs, the QR PNG, the event sender and the worker's job sender. The job sender is what PLAN B.2's back-channel (`main.rs:302-308`, `:704-707`) ships across today.

Inside the tokio thread the listener binds before anything with a side effect: before the RPCS3 Net config write (`main.rs:341`), the driver build and the database open. The port is the single-instance lock: it's the resource both apps actually contend for (the phone URL), the OS releases it if the holder crashes and it costs nothing. A lockfile in the runtime dir was the alternative, but it's a second source of truth that can disagree with the port. A bind failure goes to the same `report_fatal` → `LauncherScreen::ServerError` path as today, now before `db.sqlite` is touched.

Config loading splits by binary. The product keeps `config::load` with its wizard. guacamole has its own loader (decision 6) that reads only the keys it uses and never requires `RPCS3_EXE` or `FIRMWARE_PACK_ROOT`.

**3. `DriverKind::Native`, with an explicit arm everywhere RPCS3 or a figure file is assumed.**

| Site | Product today | Native |
|---|---|---|
| `main.rs:76-187` | indexes the pack, merges scans, decorates vehicle terrain | skipped; the interim catalogue (decision 5) |
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

The `BootDirect` row is the dangerous one. The non-IPC arm is gated on `cfg(windows)`, not on the driver, so anything without an IPC socket gets a real RPCS3 launch on the HTPC. The arms key on `DriverKind` (threaded into the job), never on platform. The stats routes answer 409 at run time rather than by leaving the feature off, because `cargo test --workspace` unifies features and turns `sky-stats` on for every test build.

A catalogue record has no file under Native: its file path stays empty, and every arm above that would read it is skipped. Making the path optional in the shared figure type waits for G1 with the rest of the file-shaped API; AA.3's test is what holds it.

**4. The native driver places by identity and sends placements over a channel.**
`DriverJob::LoadFigure` carries `path: Option<PathBuf>`: `Some` for the RPCS3 drivers and the mock, exactly as today, and `None` under Native, where nothing forks a working copy. The worker calls `load(slot, path)` for `Some` and a new trait method `place(slot, &Placement)` for `None`. `place` defaults to an error ("this driver loads figure files"), the way `window_set` defaults to one, so the RPCS3 drivers and the mock don't change. `NativePortalDriver` (in `crates/guacamole`) implements `place`, and its `load` returns an error, so a stray file path under Native fails the placement where it can be seen instead of opening a file.

The load handler builds the `Placement` from the catalogue and the profile store: the figure id plus its toy type and variant (parsed from the id, `ToyTypeId`/`TagVariant` in `crates/core/src/figure.rs:24,50`), the name and element, the portrait path (server-private, never sent to the phone) and the owner (the placing profile's id, initial and colour, or none when no profile is unlocked). Toy type and variant ride along because AB keys the character's intro on the toy type (and AD its model), not on the catalogue's id string.

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

Wrong is worse than missing: a TV that calls Spyro "Cynder" is the failure, a missing repose isn't. AA.4 records what the rules kept and dropped. The list is the same for every profile (no per-kid shelf, design doc Figures). AB's disc test cross-checks it against Giants' character archives and AD replaces it.

Alternatives: reading the product's pack index (rejected, it's the firmware pack); committing a new hand-made roster file (rejected, it's a third copy of data the disc is about to replace).

**6. Setup is one question, and guacamole's settings stay out of the controller's files.**
- Dev: `GUAC_DUMP_DIRS` in `.env.dev`, a list in the OS's path-list form. guacamole's dev loader also reads `BIND_PORT`, `PHONE_DIST`, `DATA_ROOT`, `WINDOW_MODE` and the dev pairing key (`./dev-data/hmac.key`) like the product.
- Release: when no dumps folder is set, the TV's setup screen asks for one and opens a native folder picker on the PC (a grown-up with a keyboard and mouse, once). `guacamole --setup` asks again.
- The scan looks for `PS3_GAME/PARAM.SFO` in each chosen folder and up to two levels below it, reads TITLE_ID with AB.2's parser and reads nothing else from the dump. The TV lists every dump it found by title, and Giants (BLUS30968) is the one guacamole offers. The phone never sees a path.
- Port and pairing key: when the controller's `config.json` exists, guacamole uses its port and key, so the QR and every bookmark are the same with either app, and ignores everything else in it. Without one, guacamole picks the default port, generates a key once and keeps both.
- guacamole's settings (the dumps folders, plus the port and key on a machine without the controller) live in `guacamole/settings.json` under the shared runtime root. Not in `config.json`, because the controller's loader would take the file for its own finished setup. Not in `db.sqlite`, because a new table there stops the frozen controller from starting (design doc, Persistence and setup). Where they live for good follows Open question 1; nothing in the file is precious, and deleting it just means answering setup again.
- Profiles and PINs come from the shared runtime root's `db.sqlite` through the same profile store, with no migration.

**7. Bevy app shape.**
- Bevy 0.20.0 (pinned by `Cargo.lock`; upgrades are their own change at milestone boundaries).
- `LogPlugin` off: the library's `logging::init` owns the global tracing subscriber (two would fail `set_global_default`), and the e2e harness scrapes its "serving on" line.
- One window: borderless fullscreen in TV mode, a resizable window when `.env.dev` says `WINDOW_MODE=desktop` (the product's existing knob). `windows_subsystem = "windows"` outside `dev-tools`, as in the product (`main.rs:7`).
- UI in `bevy_ui`, laid out at 1080p and scaled by `UiScale` from the window height, so the 86" 4K TV at 10 ft gets 2×. Titan One, the same face the phone and launcher use, comes from `crates/server/assets/fonts/`.
- A `ServerBridge` plugin polls `LauncherStatus` each frame (one clone of a sync mutex, as eframe does) and drains the portal channel. A `Tv` plugin owns the setup, join and portal screens.
- Cards sit in two rows of four, so eight always fit at 16:9, inside the same 6% overscan inset the launcher reserves in fullscreen (`TV_OVERSCAN_FRAC`, `crates/server/src/ui/main_screen.rs:125`). The 86" TV crops its edges (R.1), and "each fully visible" in the figure-cards spec means visible on that TV. Each card carries an owner pip in the placing profile's colour with its initial.
- `LauncherScreen::Farewell` sends `AppExit` after the farewell beat. `ServerError` shows its message.
- The guacamole crate allows `clippy::type_complexity` and `clippy::too_many_arguments` crate-wide (Bevy system signatures trip both by design); everything else stays at `-D warnings`.

Release builds each binary in its own `cargo build -p … --no-default-features` step. A combined invocation would unify features across both packages and pull `sky-stats` into guacamole.

**8. e2e against either binary.**
`TestServer::spawn*` reads `E2E_APP` (`product` default, or `guacamole`) and runs `cargo run -p <pkg> --features test-hooks`. For guacamole it skips the firmware-pack lookup and points `GUAC_DUMP_DIRS` at a temp dir holding a synthetic `PS3_GAME/PARAM.SFO` for BLUS30968, built in code at run time with AB.2's test-only builder. AA never opens anything else on a dump, and AB's identity check sees that dir as "no game data", which leaves cards working.

guacamole adds `/api/_test/tv` through the extra-routes hook. A `PostUpdate` system mirrors the TV's screen and cards into an `Arc<Mutex<TvSnapshot>>`, and the hook returns it as JSON, so an e2e test asserts on what the TV shows without screenshots.

## Risks / Trade-offs

- [The split changes the product without anyone noticing] → AA.1 and AA.2 are moves only, with the unit-test count equal before and after, and they land and pass the product's e2e suite on the Mac plus the HTPC live smoke BEFORE any Native code exists.
- [A Native arm is missed and something opens a figure file with an empty path] → AA.3 runs a load, a slot reset, a Kaos swap and an auto-restart restore under Native and asserts nothing was created under the runtime root and `load(slot, path)` was never called. The native driver's `load` errors, so a miss shows up as a failed placement, not a silent one.
- [The interim catalogue shows a figure under the wrong name] → decision 5's rules plus a test over the committed data (no name on two toy types, every entry named). AB's cross-check against the disc finds what the rules can't.
- [The native folder picker opens behind the fullscreen window under Steam Big Picture] → AA.11 checks it on the HTPC. The fallback is setting the dumps folder in `guacamole/settings.json` by hand, which AA.12 documents either way.
- [Adding Bevy to a workspace that already has eframe 0.29 hits a `links` conflict or a duplicate-native-lib clash] → AA.6 starts by adding the dependency and running `cargo build --workspace --all-features` on the Windows CI lane and on the Mac before writing app code. The macOS CI lane has been `if: false` since 2026-05-30 (`ci.yml:150-155`), so the Mac is covered by hand and by `.githooks/pre-push`, not by CI. Duplicate pure-Rust versions (winit, wgpu, raw-window-handle) are fine across two binaries.
- [Bevy makes CI and `.githooks/pre-push` slower] → `Swatinem/rust-cache` already caches the Windows lane (one `shared-key` per OS, `ci.yml:60-64`, not per feature set). AA.10 measures the cold and warm deltas there and on the Mac's pre-push run and records them. If the pre-push hook gets painful, the fix is its own change, not a quiet skip of the guacamole crate.
- [Windows Defender's "allow this app on the network" prompt fires the first time `guacamole.exe` listens, hidden behind a fullscreen window under Big Picture] → the MSI's port-8765 rule (`wix/main.wxs:108-115`) keeps the product from prompting and should cover `guacamole.exe` on the same port. Unverified, so AA.11 records whether a prompt appears; if one does, AA.13 adds a program-scoped exception for it.
- [A second instance on a different port slips past the port lock] → only possible with a dev `BIND_PORT` override; release reads one port.
- [Bevy's borderless fullscreen misbehaves under Steam Big Picture on the 4K TV (focus, overlay, scaling)] → AA.11 is a named HTPC bring-up box; the eframe launcher's always-on-top tricks aren't needed with one window.
- [The phone gate changes the product's phone] → the product's `figure_edit.rs` e2e passes unchanged, and the gate only disables actions whose stats couldn't be read, where an edit couldn't have worked either.

## Migration Plan

AA.1 and AA.2 land first as a pure refactor. Rollback is a revert, and nothing persisted changes shape. The Native work and the guacamole crate land after. guacamole adds one file, `guacamole/settings.json`, under the shared runtime root, and touches nothing else the controller owns; rolling it back is a revert plus deleting that file.

## Open Questions

From the design doc, unchanged here:
- Open question 1: where guacamole's settings, and from AG its tables, live for good. AA keeps them out of both of the controller's files meanwhile.
- Open question 2: how guacamole reaches the HTPC. AA.13 waits on it.
- Open question 8: the wide source cut (decision 1) or the narrow one (c). AA.1 is written for the wide one.
