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
- **Its own SQLite, no files to copy around.** `guacamole` keeps profiles, PINs, per-profile figure instances, per-profile story saves, achievements and settings in ONE database of its own under its own data root (release: `~/Library/Application Support/guacamole/` on macOS, `%LOCALAPPDATA%\guacamole\` on Windows; `./dev-data/guacamole/` in dev), with its own migration set (`crates/guac-server/migrations/`, five: the product's profile tables, same shape, plus `settings`, the disc roster's ids, the figure store and `story_saves`, AU.3; a migration file that has run on a family's database is frozen, since sqlx refuses one whose checksum moved). It never opens the controller's `db.sqlite` (the frozen controller's `sqlx::migrate!` refuses a migration it doesn't know) or `config.json`. Profiles aren't copied — the kids set theirs up again in `guacamole` ("set them up again, it's super simple"), and since the pairing key is `guacamole`'s own too, a phone switching apps scans the TV's join code once. The extract dir is a regenerable cache. Setup asks one thing, where the disc dumps are — no RPCS3 path, PS3 firmware, figure pack or `games.yml` scan.
- **Figures: full reset.** Guacamole never reads a `.sky` — no import, no figure crypto, no firmware pack, no figure dumps, no scanner. A figure is an identity (toy type + variant) from the disc's own roster; the phone offers the whole roster, no per-kid shelf. Progress starts fresh in Guacamole's store, one instance per profile per figure (today's working-copy rule minus the file), and the phone's figure editor edits those rows.
- **Story saves belong to profiles.** One save per profile per game, no slot picker. The profile that starts the game is the host and only its save advances; figure progress (XP, gold, upgrades) goes to the profile that placed the figure, so in co-op both kids' characters advance. Built (openspec AS.38, AU.3, AU.4, AU.15): the original's save scripts run on the VM, each save kept as JSON in `story_saves`, and a later session resumes where the title's load goes. How it's built, file by file: [`docs/native/guacamole-crate-map.md`](docs/native/guacamole-crate-map.md) (Story saves, as built).
- **Online is out, local PvP is in.** Nothing networked beyond the LAN phone link (no DemonWare, no PSN); trophies become per-profile achievements stored with the profile (names/icons from the disc's `TROPDIR` if it reads, otherwise our own); in co-op a counter both kids feed credits each kid's own share where Guacamole can tell whose it was, otherwise only the host earns it. Couch battles are part of parity for every game.
- **Gates.** **G1** = `guacamole` plays one game end to end → it becomes the only thing this repo builds and ships; the product binary, everything RPCS3 (`rpcs3-patches/` P1–P8 as a product series, `rpcs3-control`'s product drivers, nested-app bundling/signing), `crates/sky-parser` and the wiki data leave. G1 stops building the product, it doesn't uninstall it: the controller on the HTPC keeps the other five games until Guacamole covers them. **G2** = parity → the dev-only RPCS3 oracle (`vendor/rpcs3` + a dev-only patch series) retires.
- **The phone is the portal.** No USB/HID portal, ever.
- **Naming:** binary `guacamole`, crates `guac-*` (`guac-formats` = pure fuzzed parsers, `guac-ir` = versioned serde IR, `guac-extract` = the CLI, `guac-server` = the shared server lib, `guac-oracle` = the oracle comparisons' pure half, `guac-motion` = the hero's motion and the physics seam, `guac-script` = the tfbScript interpreter, `guac-sound` = the sounds a script plays, looked up). Never "Skylanders…" in a crate or binary name.
- **HARD RULE: no game bytes in the repo, ever** — no extracted assets or scripts, decrypted EBOOTs, decompiler/disassembler output, RPCS3 memory dumps, RSX captures or keys. They live outside the tree; tests that need disc data are `#[ignore]`d and read the dump path from `.env.dev`. The **game-magic guard** ([`crates/guac-extract/tests/no_game_bytes.rs`](crates/guac-extract/tests/no_game_bytes.rs), part of `cargo test --workspace`, so `.githooks/pre-push` runs it) fails when any file in git's index or in unpushed history starts with a game magic (`guac_extract::magic::GAME_MAGICS`), is a gzip/zstd whose inflated head does, is a big-endian PPC64 ELF, or is bare MPEG Layer III frames (the `sounds` part's audio, AR.5). That's why test fixtures are always built in code at test time (synthetic SFOs, IGAs, MP4s, the e2e intro from ffmpeg's `testsrc`) and never committed: a committed fixture with a real magic would trip it, and a real disc file can't be one.
- **chotchki's own data comes through every run unchanged.** The disc dumps under `~/Library/Application Support/rpcs3/game_discs/` and the `.sky` pack are read-only (copy a figure out before loading it anywhere). No RPCS3 binary ever runs with the real `HOME`, not even `--version`: the oracle runs in a home of its own from `oracle home init`, and `~/Library/Application Support/rpcs3/` and `~/Library/Caches/rpcs3/` are hashed (entry count and listing) before an oracle session and after it. An existing oracle build or home is never modified or deleted, and git never runs in an oracle-src checkout.
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
  - `crates/guac-server/` (lib `guac_server`, openspec AA.1) — the server both binaries link: Axum routes (`http`), driver worker + watchdogs (`state`), profile store, Kaos, mDNS, round QR, `paths`, `logging`, `firewall`, the embedded phone bundle, the shared config types, and `boot(BootOptions) -> Result<ServerHandles>`, the one startup call (binds the listener FIRST, so the port is the single-instance lock). Also guacamole's `setup` (loader + settings), `dumps` (PARAM.SFO scan) and `native_catalogue`, plus guacamole's migration set (`migrations/`) and its story saves (`saves`: the store's queries, `GameHost` and the TV's keeper `StorySink`, AU.3). Where it needs product code it calls one of four hook traits in `hooks.rs` (`FigureFiles`, `DisplayModes`, `Rpcs3Config`, `SettingsWriter`); a missing hook is the Native arm.
  - `crates/server/` — the product binary (`skylander-portal-controller`): eframe launcher (`ui/`, badge, vortex, compositor, GL fallback), the product's `config` loader + first-launch wizard, its three frozen `migrations/`, and what stayed product-only behind the hooks (`product_hooks.rs`: `sky_edit` + `sky_stats` under the `sky-stats` feature, `working_copies`, `display_mode`, `rpcs3_config`). Re-exports the moved `guac_server` modules. `dev-tools` feature on by default.
  - `crates/guacamole/` — the second binary (Bevy 0.20): boots `guac_server` with `DriverKind::Native`, the TV's screens on Bevy's own UI stack behind one input router, the first-run extraction, intros, the level loader and scene, the hero on Avian's side of the physics seam, and the original's scripts on the tfbScript VM driving the hero, creatures, camera, cutscenes, HUD, sounds, story saves and the level end. `dev-tools` on by default; `test-hooks` adds the `/api/_test/tv*` endpoints the e2e and the Mac drivers read.
  - `crates/guac-formats/`, `crates/guac-ir/`, `crates/guac-extract/` (openspec AB) — the extractor: pure fuzzed parsers over byte slices (no I/O), the versioned serde IR, and the library plus dev CLI that identifies a dump and extracts levels, cinematics and sounds on demand. Its [`tests/no_game_bytes.rs`](crates/guac-extract/tests/no_game_bytes.rs) is the game-magic guard.
  - `crates/guac-oracle/` and `tools/oracle-harness/` (openspec AF) — the RPCS3 oracle's two halves, dev-only, never linked by `guacamole`: the pure comparisons (traces, envelopes, FLIP, the RNG model) and the harness that drives the oracle build.
  - `crates/guac-motion/` (openspec AG.4) — the hero's motion and the `PhysicsSeam` trait, pure (no Bevy), with `ParryWorld` for tests and a conformance table every implementation passes; the Avian side is guacamole's [`physics.rs`](crates/guacamole/src/physics.rs).
  - `crates/guac-script/` (openspec AH.14) — the tfbScript interpreter, pure (no Bevy, no I/O): `Program::link`, the `Vm`, the `Host` trait guacamole implements, the session's MT19937. Its semantics are its crate docs ([`lib.rs`](crates/guac-script/src/lib.rs)), written first, in our own words.
  - `crates/guac-sound/` (openspec AR.6) — the sounds a script plays, looked up, pure: `SoundBook` over the extract's `sounds` part.
  - **The detail lives in [`docs/native/guacamole-crate-map.md`](docs/native/guacamole-crate-map.md)**: every file, test hook, env switch and stand-in ledger key of these crates, by openspec box. A lane that changes what a crate does updates its entry THERE; this file stays one line a crate.
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
- The phone is on Tailwind v4: inline utilities by default, `@apply` in a per-component CSS file past about 12 classes or on repetition, raw CSS only for what Tailwind can't shorten.
- The rest (the 3D launcher badge, mocks and reviewing them on a real iPhone, the safe-area pattern, `ios-inspect`, the full CSS escape-hatch policy, the post-MVP Kaos feature) is in [`docs/product-reference.md`](docs/product-reference.md). Read its Aesthetic section before any phone work.

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
- **The dev profile is Bevy's** (openspec AG.65, the root [`Cargo.toml`](Cargo.toml)): the workspace's crates at opt-level 1, every dependency at 3, debug assertions on. A cold target dir costs about 4 times an opt-level-0 one (11 minutes for `cargo build --workspace --all-targets` at 4 jobs) and a one-line rebuild about the same, while the tests and the e2e run 2-6 times faster, so reuse a warm target dir rather than starting a fresh one. The release-built disc tests run under `--profile disc` (release with incremental on, AQ.159): a one-line change rebuilds in 5-9 s there against release's 30-49 s.
- **The Mac's load cap** (a load past 150 hung it on 2026-10-19, a hard reboot): at most two compile lanes at once, each with `CARGO_BUILD_JOBS=4` and cargo under `nice`; hold a heavy run while the 1-minute load is over 32 or `/` has under 40 GiB free; no fuzzing beside a build, nothing started only to make load ("under load" means what the other lane already puts on the Mac), and one RPCS3 at a time. While chotchki plays a build (a playtest), no lanes run, or their builds and disc runs go under `taskpolicy -b` (background QoS, the E-cores on Apple silicon) as well as `nice`: the TV stuttered at loads of 19-27, far under the cap (openspec AG.288), and its slow-frame log (`slow frame` lines with the 1-minute load) says which stalls were the Mac's. Never guacamole's binary tests under it: their 5 s wall-clock waits time out there (AQ.153).
- **guacamole:** `cargo run -p guacamole` from the repo root (add `--features test-hooks` for `/api/_test/*`). It reads the same `.env.dev`; the keys that matter are `GUAC_DUMP_DIRS`, `GUAC_EXTRACT_DIR` (the extract root, outside the tree), `GUAC_FFMPEG` and `BIND_PORT`, never `RPCS3_EXE` or `FIRMWARE_PACK_ROOT`. Process env: `GUAC_MUTE`, `GUAC_TEST_POSTED_PADS_ONLY`, `GUAC_LEVEL`, `GUAC_DEV_ENTRY` and `GUAC_SHOT_DIR` (offscreen PNGs, outside any git checkout).
- **Guacamole RE paths and the oracle** (dev only, every path outside the tree): `GUAC_GIANTS_DUMP` is the Giants dump root the `#[ignore]`d disc tests read, `GUAC_CAPTURE_DIR` the captures root, `GUAC_ORACLE_EXE` and `GUAC_ORACLE_HOME` the oracle build and its isolated HOME. Never connect to the oracle's GDB stub (2346); close a PINE connection before any `SAVESTATE` or shutdown.
- **Every key, CLI verb and live-test knob in full:** [`docs/native/guacamole-dev-env.md`](docs/native/guacamole-dev-env.md).
- **`E2E_APP=guacamole`** runs the phone e2e suite against guacamole instead of the product (default `product`); product-only tests skip with a message. On the Mac run it through `.githooks/ram-tmpdir.sh` (`TMPDIR` on a RAM disk, AQ.35), as the pre-push runs `cargo test --workspace`. See `crates/e2e-tests/README.md`.

## macOS support

- The product ships on Windows and macOS, both driving the patched RPCS3 over the AF_UNIX IPC path; `SKYLANDER_PORTAL_DRIVER=mock` is the demo-mode override, and there is no AXUIElement driver. The Mac release is a signed, notarized `.dmg` bundling the patched RPCS3, with the game's surface embedded in the launcher window (P8).
- Bring-up steps, the surface embed, the iOS Simulator loop and the release artifact: [`docs/product-reference.md`](docs/product-reference.md) (macOS support) and [`docs/dev/macos-bringup.md`](docs/dev/macos-bringup.md).

## Error handling

- GUI-drive failure: silent retry up to N times, then error toast on phone. Start simple, iterate.

## Distribution

- GitHub Releases zip + winget MSI on Windows, the `.dmg` on macOS. Never bundle `.sky` files or game/firmware content (no piracy); users supply their own firmware and game backups through their existing RPCS3 install.
- RPCS3 is vendored and patched: `vendor/rpcs3` is a submodule pinned to a pristine upstream commit, the patches live in `rpcs3-patches/` (apply via `rpcs3-patches/apply.sh`), which makes the repo **GPL-2.0-only**. The release bundles the patched build.
- The v1 distribution model's reasoning and the `config_dir` decoupling: [`docs/product-reference.md`](docs/product-reference.md) (Distribution).

## Git workflow (pre-1.0)

- **`2.0` is the working main** (2026-10-17). Guacamole and all OpenSpec work happen on the `2.0` branch, which acts as main until some milestones are through — chotchki: "Commit freely on a branch, we'll use a 2.0 branch as our 'main' until we've gotten through some milestones." `main` holds the frozen product line.
- **Commit + push directly to the working branch** (`2.0`, above). This is a solo project with no external developer coordination; GitHub PRs are pure friction at this stage. Skip them.
- **Pre-push hook** (PLAN 10.5.5) at `.githooks/pre-push` runs `cargo fmt --check` + `cargo check --workspace` + `cargo clippy --workspace --all-targets -- -D warnings` + `cargo test --workspace` (through [`.githooks/ram-tmpdir.sh`](.githooks/ram-tmpdir.sh): `TMPDIR` on a 2 GiB APFS RAM disk made for the run and detached after, an interrupted run included, so the tests' temp-dir flushes don't wait on the SSD; openspec AQ.35, Mac only) — the full CI lint+test lane mirrored locally (clippy added 2026-06-11 after a clippy-only example regression slipped to CI *after* a release tag was already cut). Activate with `git config core.hooksPath .githooks`; bypass with `git push --no-verify` for WIP branches.
- **CI lanes** documented in `docs/dev/ci.md`. The iOS-Simulator e2e lane is label-gated to `run-ios-sim` on PRs (and runnable via manual `workflow_dispatch`) — running it on every PR would dominate the macOS-minutes budget.
- Reserve PR ceremony for post-1.0 or for cases where a human reviewer genuinely adds value (e.g. first-time CI-bring-up or a dangerous rewrite where the diff view is the point).
- **Parallel lanes work in persistent worktrees under `~/workspace/guac-lanes/`**, each keeping its own warm target dir, and merge into the working branch (`2.0`) locally when done. Not `isolation: "worktree"` per agent: a fresh worktree pays the cold build (11 minutes at 4 jobs, AG.65) every time. Not under `/private/tmp` either: macOS's `tmp_cleaner` purges it at midnight, which took an integration worktree's files on 2026-10-24.

## RPCS3 window/menu gotchas (UIA fallback)

These bind only the legacy UIA driver (`SKYLANDER_PORTAL_DRIVER=uia`, stock RPCS3); the production IPC path has no dialog and no menu nav. They're in [`docs/product-reference.md`](docs/product-reference.md) (RPCS3 window/menu gotchas) and `docs/research/game-launch-window-mgmt.md`. The one that bites outside UIA: all UIA and SendInput automation is session-bound, so RPCS3-live tests run on the physical machine, never over SSH.

## Naming gotchas

- Spec originally said "RPS3" — it's **RPCS3**.
- Firmware file extension is `.sky`, not `.dump`.

## See also

- `SPEC.md` — authoritative long-form spec + full Q&A history with decision rationale.
- `openspec/` — execution: `changes/<id>-<slug>/tasks.md` per phase, `backlog.md` for unphased work, `config.yaml` for the conventions.
- `PLAN_ARCHIVE.md` — the frozen pre-OpenSpec plan (phases closed before the 2026-10-17 move).
- `docs/native/guacamole-design.md` — the Guacamole track (decisions, architecture, milestone ladder, RE toolchain).
- [`docs/native/guacamole-crate-map.md`](docs/native/guacamole-crate-map.md) — what lives where in the Guacamole crates, by openspec box (the detail this file used to carry).
- [`docs/native/guacamole-dev-env.md`](docs/native/guacamole-dev-env.md) — every Guacamole env key, RE path and oracle knob.
- [`docs/product-reference.md`](docs/product-reference.md) — the frozen product's look, Kaos, macOS support, distribution and UIA gotchas.
- `docs/aesthetic/` — UI reference images.

**Keep this file small.** Every agent session loads it whole before doing any work (it was 98 KB on 2026-11-13, a third of that after the trim). What every session needs goes here in a line or two; what one feature needs goes in the doc that owns it.
