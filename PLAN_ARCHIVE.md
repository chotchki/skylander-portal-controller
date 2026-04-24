# Skylander Portal Controller — Archived Plan (Phases 0–3)

Frozen record of Phases 0–3. Every checkbox here is resolved — either done, partial-but-promoted into a later-phase item, or explicitly deferred into the active plan ([PLAN.md](PLAN.md)).

Conventions match PLAN.md: `[ ]` pending, `[x]` done, `[~]` partial/deferred, `[?]` needs discussion.

---

## Phase 0 — Scaffolding (tiny, just enough to start building)

- [x] `cargo init` a workspace with one binary crate (for now). Commit `Cargo.lock`.
- [x] Add `.gitignore` for `target/`, `logs/`, `dev-data/`, `.env.dev`, `node_modules/` (just in case).
- [x] Create `docs/research/` folder. Research writeups land here.
- [x] Create `docs/aesthetic/` README referencing `ui_style_example.png`.
- [x] Verify local build runs with `cargo run`.

**Review checkpoint:** nothing to review; just ensures we have a skeleton.

---

## Phase 1 — Research spikes (PARALLELIZABLE)

These are the unknowns that block everything else. Each produces a short writeup in `docs/research/`.

### 1a. RPCS3 portal control — DONE
- [x] Read RPCS3 source. Dialog at `rpcs3/rpcs3qt/skylander_dialog.{h,cpp}`. 8 slots, each with Label/LineEdit/Clear/Create/Load buttons. `UI_SKY_NUM = 8`. Dialog is a singleton triggered by `actionManage_Skylanders_Portal`.
- [x] Enumerated widget tree via UIA (`tools/uia-probe/`). Every widget addressable by class/name; Invoke+Value patterns available.
- [x] Drove a full load end-to-end (`tools/uia-drive/`). Eruptor → slot 1 in **861 ms total** (file dialog 554ms, path-set 2ms, value-change 71ms). Spinner signal: poll `ValuePattern::get_value()` on the row's `QLineEdit`.
- [x] Slot confirmation comes from the `QLineEdit` value changing away from "None" — reliable and fast.
- [x] Dialog is a **nested window** under the main RPCS3 window, not top-level. UIA finds it fine.
- [x] Off-screen move: `TransformPattern.move_to` is insufficient (UIA reports success but window doesn't move). **Phase 2 must use Win32 `SetWindowPos` via `NativeWindowHandle`**. UIA accessibility continues to work either way.
- [x] Writeup: `docs/research/rpcs3-control.md`.

### 1b. RPCS3 launch & version detection — DONE (folded into 1a)
- [x] CLI boots a game via `rpcs3.exe "<game_root>/PS3_GAME/USRDIR/EBOOT.BIN"`. Do NOT use `--no-gui` (kills the Manage menu we depend on).
- [x] Game catalogue: read `<install>/config/games.yml` — flat YAML of `serial: path/`. Users' installed titles: BLUS30968 (Giants), BLUS31076 (Swap Force), BLUS31442 (Trap Team), BLUS31545 (Superchargers), BLUS31600 (Imaginators). SSA (BLUS30906) not installed yet.
- [x] Version from UIA window title: `RPCS3 0.0.40-19203-0b9c53e2 Alpha | master`. Regex-parseable.
- Writeup merged into `docs/research/rpcs3-control.md`.

### 1c. Firmware pack indexer (first pass) — DONE
- [x] Walk `C:\Users\chris\workspace\Skylanders Characters Pack for RPCS3`, produce a JSON inventory: `{game, element, variant_group, file_name}`.
- [x] Classify each entry (figure / sidekick / item / adventure-pack / creation-crystal).
- [x] Stable figure IDs: hash of `(game, element, relative path)` — verify stability across scans.
- [x] Writeup: `docs/research/firmware-inventory.md` with counts per game/element/type and any odd entries.
- **Findings:** 504 entries. `.sky` (not `.dump`). Spec-unknown sub-trees: Trap Team `Traps/` (64), Trap Team `Minis/` (18), Superchargers `Vehicle/` (27). Imaginators has 3 naming conventions. Tool lives at `tools/inventory/`.

### 1d. Wiki scrape feasibility — DONE
- [x] Confirm Fandom MediaWiki search API is reachable. Probe it with 20 figure names from the indexer: measure hit rate.
- [x] From a hit page, extract: canonical name, hero image URL, thumbnail URL, element, type, game of origin, notable attributes.
- [x] Decide scrape tool (PowerShell is fine per user; also OK to use pure Rust with `reqwest` + `scraper`).
- [x] Writeup: `docs/research/wiki-scrape.md` with the hit rate, extraction plan, image-size decisions (thumbnail + hero), and attribution block for the app's About page.
- **Findings:** 20/20 hit via `opensearch`. Metadata extractable via `prop=pageimages|categories|revisions`. Use Rust (`reqwest`) for the Phase 2 scraper; output to `data/figures.json` + bundled images.

### 1e. Stack smoke test (Axum + Leptos + egui) — DONE
- [x] Axum serves a "hello" JSON and a WebSocket echo.
- [x] Leptos (trunk-built WASM) SPA says "hello from the phone" and round-trips one WS message.
- [x] `eframe` window shows a QR code pointing at the Axum bind address and the current WS client count.
- [x] Confirm all three coexist in one binary and build on Windows from the HTPC dev environment.
- [x] Writeup: `docs/research/stack-smoke.md`.
- **Findings:** stack works. Axum on tokio on a background thread + eframe on the main thread coexist cleanly. Leptos 0.7 compiles to WASM via trunk (768KB debug; smaller in release). Phase 2 will split into a real Cargo workspace; SPA bundles into server via `include_dir!`.

**Review checkpoint (end of Phase 1): ALL SPIKES COMPLETE. UIA is viable.** Proceed to Phase 2.

---

## Phase 2 — Minimal end-to-end slice

**Target:** a phone connects → sees the figure collection (names + element icons only, no wiki scrape yet) → taps a figure → taps a portal slot → figure loads into RPCS3's emulated portal → slot UI updates when RPCS3 confirms.

Deliberately deferred to Phase 3: PIN-gated profiles, multi-profile, working-copies, session resume, game launching, wiki scrape, aesthetic pass, Kaos, takeover, security signing. We want the smallest possible end-to-end slice first.

**Pre-conditions the MVP assumes** (will be relaxed in Phase 3):
- RPCS3 is already running with the Skylanders Manager dialog open (no game launching yet).
- Paths to RPCS3 install and firmware pack come from `.env.dev`.
- No profiles. Phone is a single, unauthed controller.
- No working copies — we load the user's fresh `.sky` files directly (read-only use; Phase 3 introduces the copy-on-first-use).

---

### 2.1 Workspace restructure — DONE

- [x] 2.1.1 Convert root `Cargo.toml` into a `[workspace]` with no root package.
- [x] 2.1.2 Create `crates/core/` — shared types (Figure, SlotState, Command, Event), no I/O. (scaffolded; real contents land in 2.2)
- [x] 2.1.3 Create `crates/indexer/` — library form of `tools/inventory`. Depends on `core`. (scaffolded; 2.4 ports the logic)
- [x] 2.1.4 Create `crates/rpcs3-control/` — `PortalDriver` trait + `UiaPortalDriver` impl + `MockPortalDriver` (feature-flagged). Depends on `core`. (trait + mock stub in place; UIA impl in 2.3)
- [x] 2.1.5 Create `crates/server/` — the binary. Axum + eframe + plumbing. Depends on `core`, `indexer`, `rpcs3-control`.
- [x] 2.1.6 Phone SPA stays at `tools/phone-smoke/` for now; promoted to `phone/` in 2.7.1.
- [x] 2.1.7 Move the Phase 1 spike's `src/main.rs` and `assets/spike_index.html` into `crates/server/` via `git mv` (history preserved).
- [x] 2.1.8 `cargo check --workspace` passes; `cd tools/phone-smoke && trunk build` still passes.

### 2.2 Core types (`crates/core/`) — DONE

- [x] 2.2.1 Define `FigureId` (newtype over the SHA-256/64-bit hex the indexer produces).
- [x] 2.2.2 Define `Figure { id, canonical_name, variant_group, variant_tag, game, element, category, sky_path, element_icon_path }`. `sky_path` and `element_icon_path` are server-side only — **never serialized to the phone**.
- [x] 2.2.3 Define `PublicFigure` — the phone-safe subset of `Figure` (no filesystem paths).
- [x] 2.2.4 Define `Game { id: GameSerial, display_name, sky_root: Option<PathBuf> }`. `sky_root` is `#[serde(skip)]`.
- [x] 2.2.5 Define `SlotIndex(u8)` newtype with `0..=7`, 1-indexed on the phone via `from_display` / `display`.
- [x] 2.2.6 Define `SlotState { Empty, Loading { figure_id }, Loaded { figure_id, display_name }, Error { message } }`.
- [x] 2.2.7 Define `Command { LoadFigure { slot, figure_id }, ClearSlot { slot }, RefreshPortal }`.
- [x] 2.2.8 Define `Event { PortalSnapshot { slots }, SlotChanged { slot, state }, Error { message } }`.
- [x] 2.2.9 Serde-derive everything with `#[serde(tag = "kind", rename_all = "snake_case")]` for enums.
- [x] 2.2.10 Unit tests for serde round-trip, slot-index bounds, Figure→PublicFigure path scrub (5 tests, all green).

### 2.3 RPCS3 control (`crates/rpcs3-control/`) — DONE (live test pending hands-on)

- [x] 2.3.1 UIA helpers ported to `src/uia.rs`.
- [x] 2.3.2 `PortalDriver` trait defined.
- [x] 2.3.3 `UiaPortalDriver` constructed via `::new()`; re-resolves widgets per call. `unsafe impl Send+Sync` added with reasoning documented (server serialises all access).
- [x] 2.3.4 `open_dialog()` auto-triggers via the Manage menu → Manage Skylanders Portal submenu when the dialog isn't already visible.
- [x] 2.3.5 `read_slots()` returns `[SlotState; 8]`; "None"/empty → `Empty`, anything else → `Loaded { display_name, figure_id: None }`.
- [x] 2.3.6 `load()` clears if occupied, invokes Load, waits for the Select Skylander File dialog, sets path via Value pattern, invokes Open, polls until value changes.
- [x] 2.3.7 `clear()` invokes Clear, polls until "None".
- [x] 2.3.8 Error-modal detection after `load` — dismisses the OK button and surfaces the message.
- [x] 2.3.9 `hide_dialog_offscreen()` implemented via Win32 `SetWindowPos` in `src/hide.rs`. (Visual verification still requires an interactive RPCS3 session — covered by the live test when run.)
- [x] 2.3.10 `tracing::instrument` spans on every `PortalDriver` method plus key event logs inside.
- [x] 2.3.11 `MockPortalDriver` (feature `mock`) with configurable latency.
- [x] 2.3.12 Live integration test `tests/live.rs`; gated by `RPCS3_SKY_TEST_PATH` env var. Run with `cargo test -p skylander-rpcs3-control --test live -- --ignored`.
- [x] 2.3.13 Mock unit tests (2 tests green).
- [x] 2.3.14 Trait docs call out the server-side serialisation requirement.

### 2.4 Indexer (`crates/indexer/`) — DONE (mostly)

- [x] 2.4.1 Port `tools/inventory/src/main.rs` into a library crate. Public API: `pub fn scan(root: &Path) -> Result<Vec<Figure>>`.
- [x] 2.4.2 Preserve the classification rules and the variant-prefix peel list from the spike.
- [x] 2.4.3 `category = "vehicle"` first-class (was "other" in the spike). 27 entries.
- [x] 2.4.4 `category = "trap"` first-class (was "item" with element). 57 entries.
- [x] 2.4.5 Element icon path resolved per figure (~97.6% coverage on real pack).
- [~] 2.4.6 Snapshot test deferred. Replaced with a count-check integration test (`tests/real_pack.rs`, gated by `SKYLANDER_PACK_ROOT` env var) that verifies per-game + per-category totals match Phase 1c exactly. A full JSON snapshot is overkill for now.
- [x] 2.4.7 `tools/inventory/` left as-is — it's the historical Phase 1c builder. Future regeneration should use the library.

### 2.5 Dev config + bootstrap — DONE

- [x] 2.5.1 `.env.dev` loader in `config.rs` (RPCS3_EXE, FIRMWARE_PACK_ROOT, GAMES_YAML, BIND_PORT, SKYLANDER_PORTAL_DRIVER).
- [x] 2.5.2 `dev-tools` feature on `skylander-server`, default ON.
- [x] 2.5.3 `.env.dev.example` committed, `.env.dev` gitignored.
- [x] 2.5.4 `tracing-appender` daily rolling log files in `./logs/` (dev). Release-mode APPDATA path is a Phase 3 concern.
- [x] 2.5.5 Startup config logged to stdout + file — "serving on http://…" is the URL-scrape anchor the e2e harness needs.

### 2.6 Server (`crates/server/`) — DONE (mostly)

- [x] 2.6.1 `AppState` in `state.rs` (figures, figure_index, driver_tx, portal mutex, broadcast, client counter).
- [x] 2.6.2 Startup sequence in `main.rs`: config → index → driver → spawn tokio thread → eframe on main.
- [x] 2.6.3 `GET /api/figures` → `Vec<PublicFigure>`.
- [x] 2.6.4 `GET /api/portal` → current slot snapshot.
- [x] 2.6.5 `POST /api/portal/slot/:n/load { figure_id }` validates + enqueues; returns 202.
- [x] 2.6.6 `POST /api/portal/slot/:n/clear` same pattern. (Also added `POST /api/portal/refresh`.)
- [x] 2.6.7 `/ws` sends initial `PortalSnapshot` then forwards broadcast events.
- [x] 2.6.8 Driver worker in `state::spawn_driver_worker`: single tokio task, `spawn_blocking` for each driver call, broadcasts `SlotChanged` before + after.
- [x] 2.6.9 `ServeDir` for the phone SPA — currently `tools/phone-smoke/dist/`; updated to `phone/dist/` in 2.7.1. Falls back gracefully if dist doesn't exist yet.
- [~] 2.6.10 Element icon endpoint deferred — phone SPA doesn't need it for the immediate MVP wireframe. Will land alongside 2.7 if needed.
- [x] 2.6.11 First-non-loopback IP pick via `local_ip_address`; URL logged as `serving on http://…`.
- [~] 2.6.12 Route unit tests deferred. Manual e2e check against the mock driver confirmed: load → portal reflects Loaded → clear → empty.

Verified on `http://192.168.1.162:8765` with mock driver:
`POST /slot/1/load` → portal shows `Loaded{Eruptor}` → `POST /slot/1/clear` → empty.

### 2.7 Phone SPA (`phone/`) — DONE

- [x] 2.7.1 `git mv tools/phone-smoke → phone`. Crate renamed to `skylander-portal-phone`.
- [x] 2.7.2 WS client in `src/ws.rs`: connects to `/ws`, exponential-backoff reconnect (500ms → 8s cap), dispatches `PortalSnapshot` / `SlotChanged` / `Error` into signals.
- [x] 2.7.3 REST helpers in `src/api.rs`: `fetch_figures`, `post_load`, `post_clear`.
- [x] 2.7.4 `<Portal />` renders all 8 slots. Each slot shows Empty / Loading / Loaded / Error with a Pick button (empty) or Remove button (loaded).
- [x] 2.7.5 `<Browser />` grid with element-chip filter + text search. Element-themed card icons (CSS gradient per element).
- [x] 2.7.6 Interaction: tap slot → banner "Pick a Skylander for slot N" → tap figure → optimistic Loading → WS flips to Loaded. Also a tap-figure-without-picking fallback that uses the first empty slot.
- [x] 2.7.7 Minimal but coherent CSS (`phone/assets/app.css`): dark blue background, per-element gradient badges on figure cards, responsive grid, touch-sized buttons. Phase 3 will redo this for the Skylanders aesthetic.
- [x] 2.7.8 Toast stack, auto-dismiss at 4s.
- [x] 2.7.9 `trunk build` pipeline stable; outputs to `phone/dist/`.
- [x] 2.7.10 Verified: server now serves the SPA's index.html at `/`, `/api/figures` returns 200, full stack is one command (`cargo run`) away.

### 2.8 eframe launcher window — DONE

- [x] 2.8.1 `LauncherApp` in `crates/server/src/ui.rs`. Fullscreen viewport, big fonts (64pt heading, 32pt URL, 40pt status).
- [x] 2.8.2 Shows QR code (10x upsampled), URL, client count, figure count.
- [x] 2.8.3 Shares the `AtomicUsize` client counter directly with the server; full AppState sharing across the OS-thread boundary was overkill for MVP.
- [~] 2.8.4 No buttons — Phase 3.
- [~] RPCS3 connection status indicator deferred; the first failed driver job already surfaces as an Event::Error on the phone side.

### 2.9 End-to-end wire-up + manual smoke

- [ ] 2.9.1 On a fresh dev environment, start RPCS3, open Skylanders Manager, `cargo run`. QR window appears. Scan from phone. See figures. Tap slot 1 → tap Eruptor → Eruptor appears on the emulated portal. Full cycle.
- [ ] 2.9.2 Test "already loaded": slot 1 already has Eruptor, pick another figure — driver should clear then load, UI should reflect Loading → Loaded.
- [ ] 2.9.3 Test "missing file" error path: delete a `.sky` file, try to load it, confirm error toast appears on phone.
- [ ] 2.9.4 Test "dialog not open": close the Manage dialog, try to load — driver auto-opens it, then proceeds.
- [ ] 2.9.5 Confirm the off-screen helper actually hides the dialog (resolves the 1a open item). If Win32 `SetWindowPos` still doesn't hide it, document and defer.
- [ ] 2.9.6 Snapshot memory/CPU under idle + during a load cycle. No strict target; just sanity.

### 2.10 E2E test harness

- [ ] 2.10.1 Add `tests/e2e/` with a Rust integration test that shells out to launch the server, waits for the "serving on http://…" log line, scrapes the URL.
- [ ] 2.10.2 Use `fantoccini` (pure Rust) against a locally-running ChromeDriver. Document the one-time `chromedriver` setup in `tests/e2e/README.md`.
- [ ] 2.10.3 Happy path: load the URL, assert the figure grid renders, click slot 1, click first figure, assert the slot shows Loading then Loaded (WS driven).
- [ ] 2.10.4 Use `MockPortalDriver` by default. Extend the mock so specific scenarios can be simulated by config / env var so we don't need RPCS3.
- [ ] 2.10.5 Keep this suite manually run locally; no CI.
- [ ] 2.10.6 **Regression scenarios from Phase 2 bug-bashing** — each must be a named test case:
      - **Spam-click same slot**: rapid-fire clicks on one figure → only one load fires; subsequent requests return 429 → phone shows one toast max, no stuck Loading.
      - **Dup-figure across slots**: load Airstrike into slot 1 (success), attempt to load Airstrike into slot 2. Driver must surface a "file in use" style error; phone shows toast; slot 2 ends in Empty (NOT Error-as-slot-text); slot 1 stays Loaded.
      - **Clear-then-load sequence**: loaded slot → Remove → Pick another figure → slot ends Loaded with the new figure.
      - **Error toast never populates the slot**: any driver failure path → slot state in the phone is Empty or the pre-failure value; `Event::Error` is surfaced as a toast.
      - **Reconnect**: disconnect WS mid-load → phone reconnects → receives `PortalSnapshot` → UI matches the real portal state.

### 2.11 Cleanup + commit hygiene — DONE

- [x] 2.11.1 Phase 1 `src/main.rs` / `assets/spike_index.html` moved via `git mv` in 2.1.7; the stale HTML dropped once `phone/dist/` came online.
- [x] 2.11.2 `tools/phone-smoke/` promoted to `phone/` via `git mv`.
- [x] 2.11.3 CLAUDE.md architecture section rewritten for the workspace layout.
- [x] 2.11.4 README.md has a "Running in dev" quickstart + layout map.

---

**Review checkpoint (end of Phase 2):** demo the end-to-end slice on the HTPC. Identify the three biggest pain points. Plan Phase 3 (profiles + PINs + session resume + game launching) accordingly.

---

## Phase 3 — Testing infrastructure first, then features

**Strategy**: Phase 2 surfaced bugs through manual tapping. That's slow and non-repeating. Phase 3 puts test infrastructure in place *before* feature work so every subsequent bug lands as a named regression test. The process/window-management pieces (RPCS3 lifecycle + off-screen hide + game launching) are prerequisites for the automated e2e harness AND for shipping game-launch as a user feature, so they come first.

**Milestone 1 (3.1 – 3.6):** a green `cargo test --test e2e` that exercises the regression scenarios from 2.10.6 against the mock driver, with injection points for the failure modes we chased this phase.

**Milestone 2 (3.7):** optional heavier suite against a real RPCS3 the harness starts and stops.

Everything after 3.7 is feature work, safe to pick off in any order once tests are watching the door.

---

### 3.1 RPCS3 process management (`crates/rpcs3-control`) — DONE

- [x] 3.1.1 `RpcsProcess::launch(exe, eboot)` — spawns `rpcs3.exe <EBOOT.BIN>`, owns the `Child`, stores PID.
- [x] 3.1.2 `wait_ready(timeout)` — polls UIA for a top-level window whose name starts with `"RPCS3 "`; detects early child exit and surfaces the status.
- [x] 3.1.3 `shutdown_graceful(timeout)` — posts `WM_CLOSE` to the main HWND; on timeout, falls back to `Child::kill` (spawned) or `TerminateProcess` (attached). Returns `ShutdownPath::{Graceful, Forced, AlreadyExited}`.
- [x] 3.1.4 `is_alive()` — `Child::try_wait` for spawned, `GetExitCodeProcess` for attached. Crashed-event hook deferred to the server integration in 3.3.
- [x] 3.1.5 `RpcsProcess::attach()` — UIA-resolve the first RPCS3 window, get its PID via `GetWindowThreadProcessId`, attach. Drop is a no-op for attached processes.
- [x] 3.1.t Live integration tests in `crates/rpcs3-control/tests/process.rs` (both `#[ignore]`).

### 3.2 Window management (`crates/rpcs3-control`) — DONE

- [x] 3.2.1 `hide_dialog_offscreen()` is public and idempotent (skips the SetWindowPos when the dialog's bounding rect is within 100px of the target).
- [x] 3.2.2 `restore_dialog_visible(x, y)` brings the dialog back, cycling SW_HIDE/SW_SHOW + `InvalidateRect` + `RedrawWindow` so Qt actually repaints after a long off-screen sojourn (verified visually).
- [x] 3.2.3 `WindowKind` enum classifies UIElements as `Main`, `SkylanderDialog`, `FileDialog`, or `Other`. Game-output window class still TBD — will capture first time we launch a game in 3.3 and refine.
- [x] 3.2.4 **Confirmed end-to-end**: off-screen hide makes the dialog visually disappear; `read_slots()` still reports all 8 slots correctly from the off-screen dialog (UIA accessibility survives). Restore brings it back repainted. Verified interactively on RPCS3 0.0.40.

### 3.3 Game launching — DONE

- [x] 3.3.1 `crates/server/src/games.rs` parses games.yml; `SKYLANDERS_SERIALS` whitelist filters to the six titles; missing EBOOT.BIN causes that game to be skipped with a warn log.
- [x] 3.3.2 `InstalledGame::eboot_path()` resolves `<sky_root>/PS3_GAME/USRDIR/EBOOT.BIN`.
- [x] 3.3.3 `RpcsLifecycle { process, current }` lives in `AppState::rpcs3` (Arc<Mutex>). `current` holds a `GameLaunched { serial, display_name }`.
- [x] 3.3.4 `GET /api/games`, `GET /api/status`, `POST /api/launch { serial }`, `POST /api/quit[?force=true]`. launch + quit are serialized by the lifecycle mutex; launch does wait_ready on a spawn_blocking; quit does shutdown_graceful on a spawn_blocking with a 30s timeout (500ms when `force=true`).
- [x] 3.3.5 Phone `<GamePicker />` renders when `current_game` is None. Tapping a card posts /launch; the WS `GameChanged` event flips the UI into the portal view once RPCS3 is ready.
- [x] 3.3.6 Header shows "Quit game" button when a game is active. Phase 3 can add the 30s countdown + force-kick UI on top (currently force=true is available programmatically only).

### 3.4 Mock driver failure injection (`crates/rpcs3-control`) — DONE

- [x] 3.4.1 `MockPortalDriver::queue_load_outcomes(Vec<MockOutcome>)`. `MockOutcome` variants: `Ok`, `FileInUse { message }`, `QtModal { message }`, `Timeout`. (Simpler than per-figure targeting; FIFO across all loads is enough for the 2.10.6 scenarios.)
- [x] 3.4.2 Queue lives in `Mutex<VecDeque<MockOutcome>>`, pop-front per load.
- [x] 3.4.3 `POST /api/_test/inject_load` endpoint behind `#[cfg(feature = "test-hooks")]` on `crates/server`. Body: `{ "outcomes": [{ "kind": "file_in_use", "message": "…" }, …] }`. Returns 409 if the mock driver isn't active. Disabled at the Router level when the feature is off.
- [x] 3.4.4 Three new unit tests exercise the Ok-fallthrough, FileInUse, and QtModal paths through `MockPortalDriver::load`.

### 3.5 E2E harness scaffolding (`crates/e2e-tests/`) — DONE

- [x] 3.5.1 Workspace crate `crates/e2e-tests/`. Runs via `cargo test -p skylander-e2e-tests`.
- [x] 3.5.2 `Phone::new` connects fantoccini to `http://localhost:4444`. One-time chromedriver install steps documented in `crates/e2e-tests/README.md`.
- [x] 3.5.3 `TestServer::spawn()` writes a temp `.env.dev`, runs `cargo run -p skylander-server --features test-hooks`, multiplexes stdout + stderr into a channel, scrapes `serving on http://…`.
- [x] 3.5.4 `Phone` helpers: `wait_for_portal`, `tap_slot`, `tap_figure_named`, `slot_text`, `search`, `toast_count`, `last_toast_text`, `wait_until`. `launch_giants` + `inject_load_outcomes` + `set_game` REST helpers exported at crate root.
- [x] 3.5.5 `ChildGuard::Drop` kills the cargo-run child; fantoccini clients torn down via `Phone::close` (or their own Drop).
- [x] 3.5.6 Harness owns chromedriver: `TestServer::spawn` locates `chromedriver.exe` via `$CHROMEDRIVER` → PATH → winget fallback (`%LOCALAPPDATA%/Microsoft/WinGet/Packages/Chromium.ChromeDriver_*/chromedriver-win64/`), spawns on a dynamic free port, kills it on drop. No need to run `chromedriver --port=4444` manually.

### 3.6 Phase 2 regression scenarios as named tests — GREEN

All seven tests pass (`cargo test -p skylander-e2e-tests -- --ignored --test-threads=1`), ~17s end-to-end with the dev firmware pack. `--test-threads=1` recommended: each test spawns its own `cargo run` build of the server which contends on the artifact dir if parallelised.

- [x] 3.6.1 `spam_click_same_slot` — five rapid clicks, expect ≤1 toast, slot eventually Loaded. ✅
- [x] 3.6.2 `dup_figure_across_slots` — first load injected OK, second injected FileInUse, slot 2 stays Empty. ✅
- [x] 3.6.3 `clear_then_load_sequence` — load → Remove → load a different card → slot shows new figure. ✅
- [x] 3.6.4 `error_toast_never_populates_slot` — parameterised over FileInUse and QtModal variants. ✅
- [x] 3.6.5 `ws_reconnect` — page reload variant (fantoccini can't easily reach into a JS `WebSocket` handle; a lower-level WS-drop approach is a future refinement). ✅
- [x] 3.6.6 `on_portal_figures_disabled` — loads, asserts `.card.on-portal` class + "Already" toast on tap. ✅
- [x] 3.6.7 Product fix surfaced by 3.6.6: removed the redundant `disabled` attribute on the figure `<button>` in `phone/src/lib.rs` — the click handler already gates internally, and `disabled` was preventing the "Already" toast from ever firing. SPA rebuild required (`cd phone && trunk build`).

### 3.6b Game-launch + mid-game menu-driving research spike (interactive, HTPC)

**Why this exists:** Phase 1a/3.2 validated UIA driving against an *already-running* RPCS3 with the Skylanders Manager dialog *already opened by hand*. The first attempt at 3.7 (live lifecycle) revealed two unvalidated assumptions that need a research spike, not more test code:

1. We've never opened the Manage menu via UIA *while a game is actually running* — the menu may be disabled, may need the emulator paused, or may behave differently from when the game isn't booted.
2. We've never tested off-screen window hiding with a real game window present (only with the standalone Manager dialog).

Plus a concrete bug to fix: `RpcsProcess::shutdown_graceful`'s `Forced` path leaves `RPCS3.buf` orphaned, which blocks subsequent launches with "Another instance is running."

**Driver:** Chris at the HTPC. Output: `docs/research/game-launch-window-mgmt.md`, plus possibly a small fix to `process.rs` for the lockfile.

**Working assumptions** (per Chris):
- The portal dialog can be opened at any time after RPCS3 has finished initialising — including during shader compile and active gameplay. Shader compile being visible to the user is fine.
- **Critical:** when a game runs, RPCS3 has *two* top-level windows: the original main window (menu bar, no game viewport) and a separate game-viewport window. They likely both have `"RPCS3 "` in the title (the viewport adds FPS + game name). Our `main_window()` in `crates/rpcs3-control/src/uia.rs:78–92` does a "first child whose name starts with `RPCS3 `" walk — which probably grabs the *viewport* window post-launch and then can't find the Manage menu inside it. This is the most likely root cause of the 3.7 failure.

So the spike needs to (a) catalogue the two windows and how to distinguish them, then (b) make `main_window()` always pick the menu-bar one.

Findings: `docs/research/game-launch-window-mgmt.md`. Major revision: the two-window theory was partly right (viewport title starts with `"FPS:"`, not `"RPCS3 "`, so it doesn't collide with the main-window title match), but the actual blocker was that UIA `Invoke`/`ExpandCollapse` are no-ops on Qt 6 menus. Keyboard navigation (Alt → arrows → Enter) with UIA focus verification is the mechanism that works.

- [x] 3.6b.1 `crates/rpcs3-control/examples/rpcs3_windows.rs` — enumerates all rpcs3.exe top-level windows with title/class/HWND/rect. Confirmed two-window architecture (main at small rect + `"RPCS3 "` title; viewport fullscreen + `"FPS: ... | Skylanders ..."` title, same `Qt6110QWindowIcon` class).
- [x] 3.6b.2 Manually confirmed: Manage → Manage Skylanders Portal works at every game phase (boot, shader compile, in-game). Gamepad input stays with the game; only keyboard focus shifts during our nav.
- [x] 3.6b.3 `dump_menu_tree.rs` probe revealed the real blocker: the Manage MenuItem has **zero children** in the UIA tree until the menu is visually opened. UIA `Invoke`/`ExpandCollapse` return success but don't actually open it. Pivoted to keyboard navigation.
- [x] 3.6b.4 `open_skylanders_dialog.rs` probe: synthesised `Alt` tap → `Right`×3 → `Down` → `Down`×3 → `Right` → `Enter` with per-step UIA `has_keyboard_focus` verification. Full nav ≈ 2s. Works with the game viewport minimised and the main window moved to `(-4000, -4000)` during navigation. Rewrote `UiaPortalDriver::trigger_dialog_via_menu` (crates/rpcs3-control/src/uia.rs) to use this mechanism; bumped `DIALOG_OPEN_TIMEOUT` from 3s to 5s.
- [x] 3.6b.5 Dialog + main window + viewport are all restored to sensible state after nav via an RAII guard. Skylanders Manager dialog is slung to `(-4000, -4000)` the instant it appears. Dialog is opened once per RPCS3 session — subsequent `open_dialog` calls short-circuit.
- [x] 3.6b.6 Lockfile cleanup: `RpcsProcess::shutdown_graceful` now remembers the install dir from `launch`, deletes `<install_dir>/RPCS3.buf` after the `Forced` path.
- [x] 3.6b.7 Findings documented; CLAUDE.md "RPCS3 window/menu gotchas" section added.

**Known residual UX**: Qt clamps menu popup windows and the Skylanders Manager dialog to visible screen coords even when the parent is off-screen, so during the once-per-session open the user sees (a) menu popup items flash in the upper-left for ~2s, (b) the dialog briefly appear centre-screen before we move it off. Acceptable for MVP — happens once during RPCS3 boot. Logged as PLAN 5.1 for post-Kaos polish.

### 3.7 Optional: real-RPCS3 e2e (heavier, manual trigger) — PARKED, needs direct-desktop session

Home: `crates/rpcs3-control/tests/live_lifecycle.rs`. All `#[ignore]`-gated. Driver code is correct (proven by 3.6b probes in Chris's interactive desktop session), but **tests cannot be verified over SSH/RDP** because of Windows session isolation: SSH runs in session 0, the user's desktop is session 2, and Win32 windows don't cross sessions. The tests need Chris at the physical keyboard/console, not in a remote session.

- [x] 3.7.1 `tests/live_lifecycle.rs` scaffolded — three `#[ignore]` tests with panic-safe teardown.
- [x] 3.7.2 `lifecycle_launch_load_clear_quit` — green on HTPC. **Summary:** flipped the test flow to dialog-first / boot-second (matches real-app order and avoids the post-boot Alt-menu focus mess). Rewrote `boot_game_by_serial` to synthesise a single `SendInput` mouse click at the cell's centre — UIA `select()` / `set_focus()` on a `DataItem` don't update Qt's `currentIndex` on this build, so keyboard activation kept booting the alphabetically-first game (Digimon) regardless of target serial. `expect_focused_menu_item` (and the File→Exit submenu walker) now collect every focused `MenuItem` across the main-window tree AND the desktop popup tree, because Qt keeps the menubar header keyboard-focused *in addition to* the dropdown item. Added `UiaPortalDriver::running_viewport_title` and a serial→title map in the test so "wrong game booted" fails loudly instead of accidentally passing on a cross-compatible figure. Teardown uses Down + Up wrap (Exit is always last in File menu) instead of walking Down×N. Ran with `RPCS3_TEST_SERIAL=BLUS31442` (Trap Team, not the default) + Eruptor.sky; ~29s end-to-end.
- [x] 3.7.3 `offscreen_hide_really_hides` — green. **Summary:** `open_dialog` now auto-slings the Skylanders Manager off-screen the instant it appears, so the test must explicitly `restore_dialog_visible` as setup before it can exercise the hide/restore round-trip.
- [x] 3.7.4 `file_dialog_hidden_while_manager_hidden` — green. **Summary:** when the Skylanders Manager is off-screen, Qt clamps its child `#32770` common-dialog frame (the native "Select Skylander File" picker) to visible coords — by default popping up at (0, 0, 648, 480). `load()` now `EnumWindows`-polls for visible `#32770` frames after the Load button Invoke and `SetWindowPos`'es them off-screen in a tight loop until none remain on-screen. `file_dlg.get_native_window_handle()` returns an inner child HWND, not the frame — moving that had no visual effect, which is why the first attempt failed silently. Added `find_visible_file_dialogs` helper. Individual test runs clean; suite-sequential runs can still lose to RPCS3-lockfile races between teardown and the next test's launch — acceptable since these are interactive-desktop tests, not CI candidates.
- [x] 3.7.5 Replace the existing EBOOT-based launch contract in tests with launch-then-UIA-boot. Shutdown via `File → Exit` menu nav (mirror the Manage menu approach) to get clean exits and let RPCS3 release its lockfile normally. **Summary:** Added `RpcsProcess::launch_library` (no EBOOT arg), extracted `wait_for_exit_or_force` out of `shutdown_graceful`, and added `UiaPortalDriver::boot_game_by_serial` + `UiaPortalDriver::quit_via_file_menu` (mirrors `trigger_dialog_via_menu`'s RAII guard + `expect_focused_menu_item` verification pattern; walks File submenu by name until it finds "Exit"). Tests switched env `RPCS3_TEST_EBOOT` → `RPCS3_TEST_SERIAL`. Server's own EBOOT-direct launch path left untouched (known-broken per CLAUDE.md; separate refactor).
- [x] 3.7.7 Integrated live e2e: phone SPA + server + **real RPCS3** + real UIA driver, end-to-end. New `TestServer::spawn_live()` variant in `crates/e2e-tests/src/lib.rs` that writes `SKYLANDER_PORTAL_DRIVER=uia` (instead of `mock`), accepts the same `RPCS3_EXE` / `RPCS3_TEST_SERIAL` / firmware-pack env vars as the 3.7 live-lifecycle tests, and keeps `--features test-hooks` for the HMAC-key hook the phone auth flow needs (but no `inject_load` on the real driver). First scenario in `crates/e2e-tests/tests/live_integration.rs`: mirror the 3.7.2 lifecycle test but drive it from the phone — boot the test serial, tap slot 1, pick Eruptor by name, assert slot label matches canonical name, Remove, assert empty. `#[ignore]`-gated + env-var-opt-in so `cargo test --workspace` stays CI-safe; intended to run on the HTPC only (per the local-e2e-uses-real-RPCS3 norm). Mock-driver e2e stays — it's what CI exercises and it still catches protocol/UI regressions the real driver doesn't surface (injected failures). **Summary:** landed green on the HTPC over RDP in 30.91s first try (BLUS31076 + Lava Barf Eruptor). Full stack exercised: phone clicks `.game-card` → signed `POST /api/launch` → server `launch_library` + `wait_ready` → `DriverJob::BootGame` via oneshot → worker `open_dialog` + mouse-click boot → viewport detected → phone's `.screen-portal` appears → tap slot → click `.fig-card-p4` → `.detail-btn-primary` → `POST /api/slots/1/load` → working-copy fork + `DriverJob::LoadFigure` → slot label "Lava Barf Eruptor" → `.p4-slot-action--remove` → slot back to empty. Teardown via `ChildGuard` drops the server child; Job Object takes RPCS3 down with it; next spawn's defensive `remove_file(RPCS3.buf)` clears any lockfile.
  - Also in this item: replaced the server's `/api/launch` EBOOT-direct code path with `RpcsProcess::launch_library` + `driver.boot_game_by_serial` via a new `DriverJob::BootGame { serial, timeout, done: oneshot }`. The old EBOOT-direct path is the one CLAUDE.md calls out as menu-un-drivable; integrated e2e surfaced that it still needed fixing server-side. `boot_game_by_serial` was promoted from an inherent `UiaPortalDriver` method to the `PortalDriver` trait (mock impl is a no-op — the mock has no RPCS3 to boot; existing mock-driver tests still use `/api/_test/set_game` for the fake-launch shortcut).
- [ ] 3.7.8 (**post-4.15.15**) Game-list hardening: source the `/api/games` list from the actual running RPCS3 instance instead of the `games.yml` config file, to eliminate the drift-between-config-and-reality failure mode. Current failure mode: phone picks a serial that `games.yml` advertises but RPCS3's library doesn't have, and `boot_game_by_serial` fails with `"no DataItem named {serial} in RPCS3 library"` — catchable but user-hostile. Implementation options to evaluate: (a) **Command-line investigation first** — does `rpcs3.exe` have a CLI flag that dumps the installed-game list (JSON/text)? Check `rpcs3 --help`, the RPCS3 source for arg parsing, and the wiki. If yes, run it at server startup + cache; this avoids needing RPCS3 to be running for the phone picker to populate. (b) UIA enumeration: extend `UiaPortalDriver` with `enumerate_games() -> Result<Vec<(serial, display_name)>>` that walks the `game_list_table` for all `DataItem`s (boot_game_by_serial already does the single-item version). Requires RPCS3 to be running at library view — couples nicely with the 4.15.15 egui-shell-at-startup story (server boots RPCS3 behind the shell as part of cold-boot). (c) Hybrid: `games.yml` as fast-path at startup; refresh from UIA (or the CLI dump) whenever RPCS3 is running and cache. Decision + implementation deferred until 4.15.15 lands the egui-shell-at-startup flow so we know whether RPCS3 is expected to be running at game-picker time.
- [x] 3.7.9 Expand `tests/live_integration.rs` with three more scenarios so live-driver coverage catches up with the mock-driver regression suite. Same `#[ignore]`-gating and env-var opt-in as 3.7.7; each runs ~20-30s against real RPCS3 on the HTPC. **Summary:** refactored the 3.7.7 inline flow into shared helpers (`spawn_and_land_on_portal`, `place_figure`, `wait_slot_label`, `wait_slot_empty`, `remove_slot`, `dismiss_figure_detail`) so each new scenario is ~10 lines. Added `RPCS3_SKY_TEST_PATH_2` env var for the 2-figure scenarios — missing is a silent skip, same pattern as the existing live env. All 4 tests (3.7.7 + 3.7.9.1/.2/.3) pass individually on the HTPC; full-suite `--test-threads=1` run deferred (the RPCS3-lockfile race between teardown and next launch documented in 3.7.4 still applies to sequential runs, and these are interactive-desktop tests, not CI candidates).
  - [x] 3.7.9.1 `live_clear_then_load_different` — load figure A, REMOVE, load figure B → slot shows B. Green in 20.85s. Caught one product bug during bring-up: `FigureDetail` doesn't auto-close after a successful PLACE, so the test explicitly dismisses it via `BACK TO BOX` between loads — `dismiss_figure_detail` helper added for this.
  - [x] 3.7.9.2 `live_spam_click_same_slot` — 5× rapid click on PLACE ON PORTAL → slot ends up Loaded with the correct figure, no error detail. Green in 17.91s. Interesting finding: the client-side `DetailState::Loading` guard leaks 4 of 5 clicks through to `post_load`, but the server's per-slot `SlotState::Loading` 429 back-pressure catches them (log shows 1 driver load + 4 `reusing existing working copy` lines from the 429-rejected ones). Noted as "server-side back-pressure works" — client-side guard tightening is a follow-up if the 4 wasted HTTP round-trips become a perf concern.
  - [x] 3.7.9.3 `live_resume_after_reload` — load 2 figures, `location.reload()`, ResumeModal appears, click RESUME → both slots re-materialize. Green in 30.55s. Validates the full 3.12 resume path end-to-end through the real driver: reload drops the WS session → `unlock_default_profile` pre-seeds the new session's profile → server emits `Event::ResumePrompt` → phone renders `.resume-panel` → RESUME click fires per-slot `post_load` → real driver clears the already-occupied slot and re-loads each figure. Caught + documented a minor selector race: the game-picker's staggered `gp-card-rise` animation leaves most `.game-name` texts at `opacity: 0` for ~820ms after mount, during which `webdriver.text()` returns empty — `spawn_and_land_on_portal` now polls until the target card's text is readable instead of doing a single-shot `find_all` + match.

**Review checkpoint:** 3.1 – 3.6 are green; 3.6b and 3.7 run on the HTPC at Chris's pace and don't block the rest of Phase 3 fanning out.

---

### 3.8 Name reconciliation (carryover from Phase 2)

- [x] 3.8.1 Driver worker knows the `figure_id` it asked to load; it overrides RPCS3's `display_name` with `figures[figure_id].canonical_name` before broadcasting `SlotChanged` so unknowns don't show as `"Unknown (Id:N Var:M)"`. **Summary:** landed alongside 3.10b's `placed_by` threading — `DriverJob::LoadFigure` already carries `canonical_name` from the pack index (passed in by the HTTP handler from `state.lookup_figure`), and `handle_job` broadcasts it verbatim into `SlotState::Loaded.display_name` regardless of what the driver's post-load `read_value()` returned (`crates/server/src/state.rs:165-178`). The driver's observational read is deliberately ignored — comment on `DriverJob::LoadFigure.canonical_name` documents the rationale (working copies are named with figure-id hashes, so UIA's filename-derived fallback wouldn't be useful anyway).
- [x] 3.8.2 On `RefreshPortal` (no figure_id context), attempt a name-to-id reverse match against the indexed figures; fall back to the raw display name with a visual "?" badge if unmatched. **Summary:** added `find_figure_by_display_name(figures, name)` + `reconcile_slot_names(snap, figures)` helpers in `crates/server/src/state.rs`. Match is case- and surrounding-whitespace-insensitive against `canonical_name`; empty input always returns `None`. Threaded `figures: Arc<Vec<Figure>>` into `spawn_driver_worker` (cloned once from `figures_for_task` at startup in `main.rs`) and propagated into `refresh()` + `restore_after_failure()` so both the `RefreshPortal` path and the post-failure re-read apply the same reconciliation. Matched slots get their `figure_id` populated and their `display_name` canonicalised; unmatched slots keep the raw RPCS3 string and `figure_id: None`. Phone-side: new `.p4-slot-badge--unmatched` amber circle with `?` glyph in the top-right of any slot whose `SlotState::Loaded { figure_id: None }` renders, with a tooltip explaining "This figure isn't in your collection" (`phone/src/screens/portal.rs` + `phone/assets/app.css`). 6 new unit tests cover the helper + reconcile in exact, case/whitespace, rejected-empty, unmatched-leave-alone, and no-clobber-existing-figure-id paths.

### 3.9 Profile system + PINs (covers SPEC.md Q20-Q24)

- [x] 3.9.1 `crates/server`: SQLite via `sqlx`. Schema: `profiles(id, display_name, pin_hash, color, created_at)`, `sessions(profile_id, last_portal_layout_json, updated_at)`, `figure_usage(profile_id, figure_id, last_used_at)`. Migrations under `crates/server/migrations/`; DB path is `./dev-data/db.sqlite` (dev) or `%APPDATA%/skylander-portal-controller/db.sqlite` (release).
- [x] 3.9.2 Profile-picker screen — placeholder "Welcome, portal master" visuals; aesthetic pass deferred to 3.15.
- [x] 3.9.3 PIN keypad: 4-digit, 64px+ tap targets. Three-strikes backoff (5s lockout) enforced server-side via the in-memory `Lockouts` map + `Retry-After`.
- [x] 3.9.4 Profile admin UI: create/delete profile, reset PIN. Delete + reset both gated by the existing PIN per SPEC Q21.
- [x] 3.9.5 Guest mode deferred to 3.15 or later.

### 3.10 Two-session concurrency + FIFO eviction

Per SPEC Round 4 (Q99–Q105). Supersedes the original single-session takeover model. **Reordered ahead of working-copies/session-resume because it mutates `SlotState`, session plumbing, and the WS protocol — cheaper to land before downstream work freezes those shapes.**

Rolled out in six sub-milestones for tracking. Each is a meaningful checkpoint you can run tests against before moving on.

| Milestone | Covers | Status |
|-----------|--------|--------|
| 3.10a | 3.10.1, 3.10.2, 3.10.3 — `SessionRegistry` with 2-slot FIFO + forced-evict cooldown, `Event::Welcome`/`TakenOver`, per-session profile setters, 5 new unit tests | ✅ done |
| 3.10b | 3.10.5, 3.10.6 — `SlotState::{Loading,Loaded}.placed_by: Option<String>` threaded through `DriverJob` → `handle_job` → `SlotChanged` broadcast; `http::load_slot` resolves it from the caller's session profile; phone `model.rs` mirrors | ✅ done |
| 3.10c | 3.10.4 (server half) — retired `MaybeSession` bridge. `CurrentSession(SessionId)` is a required extractor on `unlock_profile` / `lock_profile` / `load_slot`. Clear/refresh/launch/quit/profile-CRUD remain session-agnostic (global or PIN-gated). | ✅ done |
| 3.10d | 3.10.4 (phone half) — `api::SESSION_ID` thread-local captured from `Event::Welcome`; `X-Session-Id` attached on every fetch in `do_fetch`; `TakenOver` → `TakeoverScreen` with "kick back" → page reload. `ProfileChanged` + `TakenOver` filtered client-side by session id. | ✅ done |
| 3.10e | 3.10.7, 3.10.8 — ownership badge on portal slots, "show join code" affordance | **deferred** until after Phase 4 (aesthetic + UX pass); would otherwise be re-styled twice |
| 3.10f | multi-phone scenarios against the **live** UIA driver (mirrors of 3.10e.2 / .5 but with real RPCS3 + real working copies) | ✅ done |
| 3.10f (.6) | 3.10e.6 — ownership-badge visual test, needs 3.10.7 first | deferred with 3.10e |

- [x] 3.10.1 Server session registry: `[Option<Session>; 2]` keyed by connection id + join timestamp. Admit freely while a slot is `None`. Implemented as `HashMap<SessionId, SessionState>` capped at `MAX_SESSIONS=2` with `created_at` for FIFO ordering.
- [x] 3.10.2 On 3rd connection, evict the **oldest** session (FIFO); evicted client receives `TakenOver { session_id, by_kaos }` and shows the existing Kaos screen. `RegistrationOutcome::AdmittedByEvicting { session, evicted }` returned from `SessionRegistry::register()`.
- [x] 3.10.3 1-minute cooldown applies only to forced eviction. Tracked as a single `last_forced_evict_at` on the registry (per-slot refinement deferred — one timestamp handles ping-pong correctly in practice). `RegistrationOutcome::RejectedByCooldown { retry_after }` when within the window; WS handshake sends an `Event::Error` and closes.
- [x] 3.10.4 Profile unlock is **per-session** (not global). Server: `CurrentSession(SessionId)` extractor required on `unlock_profile` / `lock_profile` / `load_slot`. `Event::ProfileChanged { session_id, .. }` fan-outs; the phone filters by its own session id. Phone: captures session id from `Event::Welcome`, stores in `api::SESSION_ID` thread-local, attaches as `X-Session-Id` on every fetch. Evicted-then-kicked-back sessions re-lock because page reload mints a fresh session.
- [x] 3.10.5 Portal state remains a single shared `[SlotState; 8]`. Both phones see the same `SlotChanged` stream; last writer wins (driver worker serialises).
- [x] 3.10.6 Extend `SlotState` with `placed_by: Option<String>` (set on successful load, cleared on clear). Included in `SlotChanged` so both phones can render ownership.
- [ ] 3.10.7 Phone: ownership indicator on each occupied slot (profile colour + initial). Owning phone's own figures get a highlighted treatment. **Deferred: land after Phase 4** so the styling matches the Skylanders aesthetic pass first time.
- [ ] 3.10.8 Phone: "show join code" action — the same QR the launcher shows, rendered inside the menu overlay (Phase 4, 4.12.4b) so an existing player can hand the join URL to a new joiner. **Moved into the Phase 4 menu overlay**; no separate sheet needed.
- [ ] 3.10.9 Defer: 2-player disconnect-cleanup semantics (what happens to P2's figures when P1 drops, how kick-back restores layout). Revisit with 3.17 reconnect overlay once real failure modes are visible.

### 3.10e E2E harness — 2-session support

- [x] 3.10e.1 `Phone::new` already supports N concurrent clients — fantoccini opens a fresh browser session per call, chromedriver fans them out. Tests just call `Phone::new` twice/thrice. New `Phone::session_id()` reads `<body data-session-id>` (populated on `Event::Welcome`) so tests can target a specific session server-side.
- [x] 3.10e.2 `concurrent_edits_both_phones` — P1 and P2 each load a different figure into a different slot; both phones see both slots Loaded via the shared `SlotChanged` broadcast.
- [x] 3.10e.3 `third_connection_evicts_oldest` — P1, P2 connected; P3 connects; P1 flips to `.takeover` (Kaos screen), P2 is undisturbed (no `.takeover`), P3 lands on the portal view.
- [x] 3.10e.4 `forced_eviction_cooldown` — after 3.10e.3 eviction, a raw WS reconnect gets `Event::Error` mentioning "taken over"/"full" and is closed. `/api/_test/clear_eviction_cooldown` fast-forwards the server clock; a subsequent connect lands `Event::Welcome` (admitted). Avoids a real 60s sleep.
- [x] 3.10e.5 `independent_profile_unlock` — two distinct profiles injected, `set_session_profile(s2, profile_B)` pins P2 to a different profile from P1's; each phone's `.profile-chip` header shows its own profile name.
- [ ] 3.10e.6 `ownership_badge_reflects_placer` — deferred with 3.10.7 (ownership badge UI) since the test asserts on the badge. Lands after Phase 4.

### 3.10f Multi-phone scenarios against the **live** driver

The mock-backed 3.10e.2–5 tests validate the session registry + WS plumbing against `MockPortalDriver`. Those miss the interactions that only the real driver surfaces: the driver worker serialising concurrent load jobs through a single UIA dialog, and per-profile working copies actually forking on disk when two different phones (two different profiles) each place a figure. These scenarios mirror the mock set but run against real RPCS3 + real UIA; same env vars and `#[ignore]`-gating as the 3.7.x live tests.

- [x] 3.10f.1 `live_concurrent_edits_both_phones` — 2 phones connected to the same profile, each taps a different slot + places a different figure. Assert both phones see both slots Loaded via the shared `SlotChanged` broadcast. Validates: the driver worker serialises the two concurrent `DriverJob::LoadFigure` jobs through one Skylanders Manager dialog without either racing into a bad state. **Green in 22.39s.** Phone 1 places Eruptor in slot 1, Phone 2 places Fryno in slot 2, worker processes them sequentially through the one dialog, both phones see both slots Loaded via the cross-broadcast.
- [x] 3.10f.2 `live_independent_profiles_loads` — 2 phones registered under 2 distinct profiles (inject + `set_session_profile`). Each places a figure into a different slot; assert the on-disk working copies fork into `dev-data/working/<profile_id>/` under the respective profiles (not the same directory). Validates the per-profile `resolve_load_path` through the real driver — the cross-profile collision case mock can't actually produce because it doesn't touch the filesystem. **Green in 23.04s.** Added `TestServer::dev_data_dir()` accessor so tests can inspect the harness's per-run temp dir. Post-load assertion walks `dev-data/working/{pid_a,pid_b}/` and confirms each profile has its own non-empty dir. Two distinct ULID profile IDs → two distinct `working/` subdirs on disk → distinct forked working copies.

### 3.11 Working copies + reset-to-fresh

- [x] 3.11.1 Working copy location via `paths::working_copy_path(profile_id, figure_id)` — `<runtime_root>/working/<profile_id>/<figure_id>.sky`. Release `<runtime_root>` = `%APPDATA%/skylander-portal-controller/`; dev = `./dev-data/`. Single resolver, no per-call branching.
- [x] 3.11.2 `crates/server/src/working_copies.rs`: `resolve_load_path(profile_id, figure)` forks on first use, returns the existing working path thereafter. Driver job consumes the resolved path; `figure_usage.last_used_at` bumped via new `ProfileStore::record_figure_usage`. 4 unit tests (fork, reuse, isolation, reset).
- [x] 3.11.3 `POST /api/portal/slot/:n/reset` signed endpoint + `working_copies::reset_to_fresh`. Phone slot UI gains a "Reset" button with a plain `window.confirm()` gate.
- [ ] 3.11.4 Creation Crystals extra-confirm (type "RESET" to confirm) — deferred with Phase 4's polish pass, since the current `window.confirm` already carries meaningful "All progress will be lost" wording. Track in Phase 4's scope.

### 3.12 Session resume + layout memory

- [x] 3.12.1 Layout persistence threaded into `spawn_driver_worker`: after every successful `LoadFigure`/`ClearSlot`, call `persist_layout` which iterates unlocked sessions and writes the current 8-slot snapshot to each profile's `sessions.last_portal_layout_json` row. New `ProfileStore::save_portal_layout` / `load_portal_layout` + SQLite upserts.
- [x] 3.12.2 `Event::ResumePrompt { session_id, slots }` emitted on profile unlock. Three entry points, all routing through `build_resume_prompt`:
  1. `unlock_profile` REST handler (explicit PIN entry).
  2. `handle_ws` handshake (when the session was pre-seeded with a profile via `pending_unlock`, typical post-reload).
  3. Test-hook `unlock_session` (the `set_profile` fallback path for races where the new WS registered before the hook fired).
  Phone `ResumeModal` component shows "Resume" / "Start fresh"; Resume issues per-slot `/api/portal/slot/:n/load` calls.
- [x] 3.12.3 Skip if the saved layout is all-Empty or no row exists.
- [ ] 3.12.4 2-phone modal — if the *other* phone already has figures on the portal when this profile unlocks, the current modal still shows "Resume"/"Start fresh" with no special handling. Server-side back-pressure on `load_slot` (the existing 429 on already-loading slot) swallows collisions but doesn't communicate intent well. Proper 3-option modal (clear + resume / alongside / fresh) lands as a follow-up after Phase 4 when we can style it cleanly.
- [x] 3.12.5 Supporting work: new `ProfileStore::record_figure_usage` (updates `figure_usage.last_used_at` on every load). Canonical name threaded through `DriverJob::LoadFigure` so per-profile working copies (figure_id-hash filenames) don't leak into the displayed slot text. `unlock_default_profile` e2e helper now idempotent (reuses existing "Player 1" on repeat calls) so the reload-test flow keeps the same profile id. `/api/_test/layout/:profile_id` + `/api/_test/set_session_profile` hooks for multi-phone/resume tests.

### 3.13 HMAC command signing

- [x] 3.13.1 Server holds a 32-byte HMAC-SHA256 key in `Config.hmac_key`, persisted across restarts. Dev: `./dev-data/hmac.key` (hex). Release: inside `config.json`, generated by the first-launch wizard. QR payload becomes `http://<ip>:<port>/#k=<hex>` — the phone reads `window.location.hash` on boot, stores the key in a thread-local, wipes the fragment from the URL bar.
- [x] 3.13.2 Phone's `do_fetch` signs every request: `HMAC-SHA256(key, "{ts_ms}.{method}.{path}.{body_len}.{body}")` → hex into `X-Skyportal-Sig`, plus `X-Skyportal-Timestamp: <ms>`. Body-length included in the framing to prevent path-confusion on suffix-matching body bodies.
- [x] 3.13.3 Server's `Signed` extractor validates timestamp skew (±30s) + constant-time HMAC compare (`subtle::ConstantTimeEq`). Rejects with 401 on missing/bad/stale sig. Applied to every mutating endpoint: `load_slot`, `clear_slot`, `refresh_portal`, `launch_game`, `quit_game`, `unlock_profile`, `lock_profile`, `reset_pin`, `create_profile`, `delete_profile`.
- [x] 3.13.4 READ endpoints (`list_figures`, `get_portal`, `list_games`, `get_status`, `list_profiles`, `figure_image`, `/ws`) stay unauthenticated per the trusted-LAN threat model.
- [x] 3.13.5 Dev bypass: `dev-tools` builds accept unsigned requests with a warning so the test harness works unchanged. Present signatures are still validated (catches typos / key drift). Release rejects outright.
- [x] 3.13.6 E2E coverage: `/api/_test/hmac_key` hook returns the server's key; `TestServer::phone_url()` bakes it into a `#k=<hex>` fragment so every phone-driven test exercises the signed path end-to-end (not the dev bypass). Three new `tests/hmac.rs` scenarios: `signed_unlock_succeeds`, `tampered_signature_rejected` (401 with "bad signature"), `stale_timestamp_rejected` (61s old → 401 with "skew").

### 3.14 Reposes: collapse + variant cycling

- [ ] 3.14.1 Browser collapses figures sharing a `variant_group` into a single card showing the base figure + "N variants" badge.
- [ ] 3.14.2 Tap the variant badge → cycle between variants in place (per SPEC Q76).
- [ ] 3.14.3 Loaded variant reflected on the slot's display_name.

### 3.15 Aesthetic pass — **MOVED to Phase 4**

The Skylanders-style CSS + UX reorganization grew big enough to earn its own phase. See Phase 4.

### 3.16 First-launch config wizard (egui) — DRAFTED (needs live-desktop verify)

Implementation lives in `crates/server/src/wizard.rs` + `crates/server/src/paths.rs`. Runs inside `config::load()` so callers don't need to know setup happened. 11 unit tests for the validators + config JSON round-trip.

- [x] 3.16.1 4-page egui wizard (Welcome → RPCS3 → Firmware pack → Done) gated behind the release `load()` path when `config.json` doesn't exist. Heuristic pre-fill: `%APPDATA%\..\..\emuluators\rpcs3\rpcs3.exe` + `%PROGRAMFILES%\RPCS3\rpcs3.exe` for RPCS3, common dev-pack paths for firmware. `rfd::FileDialog` Browse buttons. Live validity indicator per page (green/red).
- [x] 3.16.2 Validators: `validate_rpcs3_path` (file must exist + be named `rpcs3.exe`), `validate_firmware_pack` (dir must exist + contain at least one `.sky` file, recursive via walkdir). Green/red UI feedback.
- [x] 3.16.3 Writes `%APPDATA%/skylander-portal-controller/config.json` via `directories::ProjectDirs`. `load()` returns the fresh config after the wizard completes. No hot-restart needed.
- [x] 3.16.4 Shared path resolver `crates/server/src/paths.rs` (`config_json_path`, `db_path`, `log_dir`, `runtime_dir_unchecked`) so every runtime-state consumer uses the same dev-vs-release split. `profiles::resolve_db_path` now delegates here.
- [ ] 3.16.5 Live-desktop verification (blocked on SSH's session isolation per CLAUDE.md): run `cargo run -p skylander-server --no-default-features --release` from a clean APPDATA, walk through all 4 pages, confirm `config.json` lands in `%APPDATA%\skylander-portal-controller\` and subsequent launches skip the wizard.
- [ ] 3.16.6 Future: a "re-run wizard" affordance once we have a general app-settings area. For now "delete `config.json` and relaunch" is the documented escape hatch.

### 3.17 Reconnect overlay + network fallback

- [ ] 3.17.1 When the authorised phone disconnects and doesn't return within ~10s, render a small always-on-top overlay window (separate eframe viewport) in the lower-right showing a reconnect QR.
- [ ] 3.17.2 "Can't connect" button on the launcher opens a network-interface picker (Q49 fallback).

### 3.19 Wiki scrape — partial first pass

- [x] 3.19.1 `tools/wiki-scrape/` — Rust one-shot binary. Reads `docs/research/firmware-inventory.json`, hits Fandom's MediaWiki API (opensearch + query with `prop=pageimages|categories|revisions`), downloads thumb + hero PNGs, emits `data/figures.json`.
- [x] 3.19.2 Scrape output committed at `data/figures.json` + `data/images/<figure_id>/thumb.png`. `data/figures.manual.json` exists as an empty curation overlay. **Partial**: 100/504 figures scraped in the first run (roughly the whole SSA + Giants sets). Remaining ~400 figures need a re-run — the agent appears to have hit a rate-limit or early termination. Hero.pngs are gitignored (20MB+); re-run `cargo run -p skylander-wiki-scrape` locally if you need them.
- [x] 3.19.3 Server serves `GET /api/figures/:id/image?size={thumb,hero}` with `Cache-Control: public, max-age=86400`. Fallback: element-icon from the firmware pack. Input validated to 16 hex chars.
- [x] 3.19.4 Phone card icon renders `<img class="card-thumb" src="/api/figures/{id}/image?size=thumb">` over the element-short label; label shows through on 404.
- [x] 3.19.5 Scraper hardened (Retry-After handling, per-figure incremental writes, 5-retry skip-on-failure, `merge_for_save` preserves prior entries, `--resume` implicit) and re-run: **504/504 figures** (100% coverage), **488 thumbs** (~97% image hit rate — 16 figures had no wiki infobox image, which is fine).
- [ ] 3.19.6 **Attribution (pre-release blocker).** Fandom content is CC BY-SA — the license requires prominent attribution + license identification + indication of modifications. Surface in the phone app: either (a) a footer link "Data & images from the Skylanders Wiki (CC BY-SA)" visible on the figure browser, (b) an About screen reachable from the header, or (c) a per-figure credit on the figure-detail screen (blocks on old PLAN 5.3 → 6.3). The exact placement depends on how Phase 4 reshapes the layout — decide when that lands. `data/LICENSE.md` already cites the source for the repo; this covers the user-visible runtime requirement. Must land before any public release (old 3.18 → covered by Phase 7).

---

## Phase 3 carryover — newly resolved (2026-04-22/23)

- [x] **3.7.8 Phase 1 verify-at-launch.** `PortalDriver::enumerate_games` + `DriverJob::EnumerateGames`; `/api/launch` checks between `wait_ready` and `BootGame`, returns 404 on miss. Empty enumeration + UIA errors fall through.
- [x] **3.7.8 Phase 2 truth-from-UIA at picker time.** 2026-04-23. Game catalogue now sourced from `driver.enumerate_games()` at server startup (after RPCS3 `wait_ready`), then filtered + display-name-mapped through `SKYLANDERS_SERIALS` via a new `serials_to_catalogue` helper in main.rs. Drops the `games.yml` dependency entirely: `InstalledGame` loses its `sky_root` + `eboot_path()` fields (modern boot path is UIA-based, never EBOOT-direct), `PortalDriver::list_installed_games` retires, `games_yaml.rs` module deleted, `UiaPortalDriver::new()` becomes zero-arg, `RpcsProcess::launch(exe, eboot)` legacy wrapper removed, `Config.games_yaml` + `PersistedConfig.games_yaml` + `GAMES_YAML` env var all dropped. Old config.json entries with `games_yaml` deserialize fine (serde ignores unknown fields). Tokio task startup restructured: spawn RPCS3 → enumerate via `DriverJob::EnumerateGames` → build AppState → serve. If enumeration fails at startup, catalogue starts empty and user sees "no games" in the picker — acceptable, same shape as RPCS3 library actually being empty. 152/152 workspace tests green; release build with `--no-default-features --features nfc-import` compiles clean.
- [x] **3.10.8 Show-join-code.** 2026-04-22. Pre-rendered PNG at `GET /api/join-qr.png` backed by `crates/server/src/round_qr.rs` (shared renderer with launcher). Phone `menu_overlay.rs` renders `<img>` inside gold-ringed `.menu-qr-frame`.
- [x] **3.16.5 First-launch wizard live-desktop verify.** 2026-04-23 alongside 6.5.4. Release-mode wizard clicks through end-to-end; `config.json` persists; server boots from it. Reader-detection side stays gated on physical console (RDP redirects PC/SC — see auto-memory).
- [x] **3.19.6 Fandom CC BY-SA attribution surface on the phone.** 2026-04-23. Profile-picker footer's old inline tagline ("data & images from the skylanders wiki · cc by-sa") replaced with a pill-shaped "CREDITS" button. New `phone/src/components/credits_overlay.rs` renders a framed-panel modal that leads with the Activision trademark + unaffiliated-fan-tool disclaimer (per Chris's framing), followed by Fandom CC BY-SA 3.0, SIL OFL 1.1 for Titan One + Fraunces, and MIT attribution for Marijn Kneppers' reverse-engineering write-up. Scrim click + CLOSE both dismiss; `prefers-reduced-motion` suppresses the rise animation. Satisfies the pre-release gate (Phase 7).
- [-] **3.16.6 "Re-run wizard" affordance.** Won't-do for v1 (2026-04-23). The documented escape hatch — delete `config.json` and relaunch — covers the need. A proper in-app settings area would carry more UI scope than the one-time-correction use case warrants; revisit if/when a general settings area shows up for other reasons.
- [-] **3.11.4 Creation Crystals extra-confirm.** Won't-do (2026-04-23). Predates 4.2.14's hold-to-activate pattern, which already provides a deliberate ~1.2s press-and-hold before any destructive action fires. Typing "RESET" as belt-and-suspenders on top of the hold pattern adds friction without preventing a meaningfully larger class of mistakes.
- [-] **3.17.2 "Can't connect" button → network-interface picker.** Won't-do (2026-04-23). `first_non_loopback_ipv4()` + mDNS hostname advertisement (`<computername>.local`) covers the common case; a UI picker is worth it only if users actually hit a multi-interface setup where the auto-pick lands wrong. Revisit if that user surfaces.
- [x] **3.10.7 Ownership badge — final aesthetic pass.** 2026-04-23. `.p4-slot-owner` upgraded from a flat 20px coloured dot to a 24px mini-heraldic bezel: gold radial-gradient ring (same recipe as `.bezel-ring`) wrapping a `--profile-color`-tinted inner plate, Titan One initial on top with dual-layer text-shadow. Markup split into `.p4-slot-owner-ring` + `.p4-slot-owner-plate` so the ring is universal and only the plate varies by owner. `.p4-slot-owner--pending` desaturates + dims during Loading→Loaded. Entry `@keyframes p4-slot-owner-in` spring-scales from 0.5 → 1.0 over 260ms; `prefers-reduced-motion` suppresses. `pointer-events: none` so the pip can't steal slot taps.
- [x] **3.10e.6 Ownership-badge e2e test.** 2026-04-23. `ownership_pip_shows_correct_owner_per_slot` in `crates/e2e-tests/tests/multi_phone.rs` — two profiles (Alice `#ff6b2a` + Bob `#5ac96b`) place figures in separate slots; both connected phones verify each slot's pip plate carries the correct initial and the wrapper's inline `--profile-color` carries the placing profile's swatch. Uses current `.p4-slot-*` selectors directly (broader selector migration is 4.16.1).
- [-] **3.12.4 Three-option resume modal** (clear+resume / alongside / fresh) for the 2-phone case. Won't-do (2026-04-23). Under the 3.10.9 simple-drop policy, Alice's figures are already off the portal when she reconnects; she can re-place from the toy box directly. A modal with three options is premature UI for a flow that works as-is. Revisit if real-world use surfaces a case where manual re-placement is painful enough to warrant the modal.
- [x] **3.14 Reposes — variant cycling.** 2026-04-23.
  - [x] **3.14.1 Browser collapses figures sharing `variant_group`.** New `GroupedCard` shape + `group_variants` helper in `phone/src/screens/browser.rs` sorts "base" first then alphabetical by `variant_tag`; outer order preserves first-seen. `<For>` keys by `variant_group` so `current_idx` state persists across filter changes. Six unit tests cover singletons, multi-variant merge, base-first sort, first-seen outer order, empty input, and scanned-CC grouping by kind.
  - [x] **3.14.2 Tap variant badge → cycle in place.** New `.fig-variant-badge` top-right gold pill; `variant_count` text; click cycles `current_idx` through variants (modular). Outer card switched `<button>` → `<div role="button" tabindex="0">` so the inner badge can be a legit `<button>` without nested buttons-in-buttons. Badge click stops propagation + card's click handler checks `.closest(".fig-variant-badge")` as a belt-and-suspenders guard. Card fields (initial, portrait, variant tag, on-portal ribbon, loading state) all read through a derived `current` Signal so cycling advances the whole card visual. Search extended to match `variant_group` so "spyro" hits all Spyro reposes.
  - [x] **3.14.3 Loaded variant reflected on the slot's display_name.** No code change — already satisfied by the existing pack-index canonical-name flow: `state.rs::handle_job(LoadFigure)` stamps the slot's `display_name` from the placed Figure's `canonical_name` (variant-specific, e.g. "Legendary Spyro"), explicitly overriding whatever generic name the driver reads back.
- [x] **3.10.9 Two-player disconnect-cleanup semantics.** 2026-04-23. Simple MVP policy: when a phone disconnects, every slot whose `placed_by` matches the departing session's profile flips to `Loading { placed_by: None }`, broadcasts `SlotChanged`, and enqueues a `DriverJob::ClearSlot` so RPCS3 drops the `.sky` file. New `AppState::clear_slots_for_profile` method; pure per-slot logic factored into `state::flip_loaded_owned_to_loading` free fn with 3 unit tests (owner match, no-match no-op, emitted Loading carries `placed_by: None`). Locked + Loading + Empty + Error slots deliberately untouched. Hooked into `http.rs::ws_handler`'s WS-close path (before `sessions.remove` so `profile_of` still resolves). E2e `disconnect_clears_departing_profile_slots` in `multi_phone.rs` exercises the full 2-phone path: Alice places slot 1, Bob places slot 2, Alice disconnects, witness Bob sees slot 1 empty + slot 2 still loaded. Future 3-option resume modal (3.12.4) will build on this — the simple-drop is correct as a default; "alongside" + "clear + resume" live in the modal.

---

## Phase 4 — Aesthetic + UX pass

Two intertwined workstreams: visual reskin (Option A Heraldic — gold bezels, Titan One + Fraunces, starfield) + IA reorganization (portal-primary + toy-box drawer). Milestones A–D mostly shipped; residuals (4.15 remainders, 4.16–17 review, 4.18–19 drift reconciliation, 4.20.14 re-audit) stay in active PLAN.md.

### 4.1 Design tokens + CSS architecture
- [x] 4.1.1 Palette (starfield + gold + element gradients + Kaos variants up-front).
- [x] 4.1.2 Titan One display + Fraunces body, self-hosted OFL under `phone/assets/fonts/`.
- [x] 4.1.3 Motion tokens (spring, sweep, tap, impact, shudder, halo, idle-float).
- [x] 4.1.4 `.skin-kaos` body class repalettes app-wide without touching components.
- [x] 4.1.5 `prefers-reduced-motion` disables ambient drift + halo rotations.

### 4.2 Static mockups (4.2.12 open in active PLAN)
- [x] 4.2.1 Portal view all states — `option_a_heraldic.html`.
- [x] 4.2.2 Slot state-transitions — `transitions.html`.
- [x] 4.2.3 Profile picker — `profile_picker.html`.
- [x] 4.2.4 PIN keypad — `pin_keypad.html`.
- [x] 4.2.5 Game picker — `game_picker.html`.
- [x] 4.2.6 Browser / toy-box lid — `portal_with_box.html`.
- [x] 4.2.7 Picking-mode integrated into box-open state.
- [x] 4.2.8 Profile creation flow — `profile_create.html`.
- [x] 4.2.8b Konami-gated profile management — `profile_manage.html`.
- [x] 4.2.8b Figure detail view — `figure_detail.html` (numbering dupe preserved for back-refs).
- [x] 4.2.9 Takeover / Kaos — `kaos_takeover.html` + `kaos_swap.html`.
- [x] 4.2.10 Resume modal / reset-to-fresh / show-join-code — folded into `menu_overlay.html`.
- [x] 4.2.11 Screen-transition demo — `screen_transitions.html` (420ms ease-spring default).
- [x] 4.2.13 Contrast/readability cleanup — opaque-text contract in design_language.md §2; 43 alpha→solid edits across 10 mocks.
- [x] 4.2.14 Hold-to-activate destructive confirmation (~1.2s). Applied to RESET + SHUT DOWN. REMOVE stays single-tap.
- [x] 4.2.15 Portal slot loaded-selection REMOVE overlay — auto-dismiss 5s.
- [x] 4.2.16 Connection-lost overlay — `connection_lost.html`.
- [x] 4.2.17 Empty states — `empty_states.html`.

### 4.3 UX reorganization
- [x] 4.3.1 Portal vs browser: toy box lid (portal primary 2×4, collection in wood-textured lid).
- [x] 4.3.2 Header: kebab → profile swatch → profile name+game → connection pip.
- [x] 4.3.3 Picking-mode flow: empty slot → toy box; loaded slot → Remove/Reset.
- [x] 4.3.4 Modal stack semantics (3 categories, no stacking, ConnectionLost always wins).
- [x] 4.3.5 Navigation map in `navigation.md` §1.
- [x] 4.3.6 Collection default sort — game-compatible, then last-used (`crates/core/src/compat.rs`).
- [x] 4.3.7 Figure detail view flow — lift in, dim others to 25%.

### 4.4 Shared Leptos components
- [x] 4.4.1 `<GoldBezel>`.
- [x] 4.4.2 `<FramedPanel>`.
- [x] 4.4.3 `<DisplayHeading>`.
- [x] 4.4.4 `<RayHalo>`.
- [x] 4.4.5 `<FigureHero>` (reused by Kaos swap).

### 4.5 Starfield + ambient motion
- [x] 4.5.1 Layered starfield on body (gradients + SVG stars + 40s parallax).
- [x] 4.5.2 `<MagicDust>` sparse particle layer (24 particles; reduced-motion hides).

### 4.6 Portal view reskin
- [x] 4.6.1 Slots via `<GoldBezel>`; empty = dimmed bezel with "+".
- [x] 4.6.2 Empty → Picking: scale 1.05 spring + glow + RayHalo.
- [x] 4.6.3 Pick → Loading: halo speeds up + gold sweep + plate dims.
- [x] 4.6.4 Loading → Loaded: impact flash + brightness spike + 4s idle float.
- [x] 4.6.5 Loaded → Cleared: desaturate + shrink + fade.
- [x] 4.6.6 Errored: red-tinted + shake + subdued red glow.
- [x] 4.6.7 Slot tap feedback: plate dent + 0.96 spring-back.

### 4.6b Figure detail view
- [x] 4.6b.1 `<FigureDetail>` with idle/loading/errored states.
- [x] 4.6b.2 Entrance: others crossfade to 25%; FLIP-style lift.
- [x] 4.6b.3 Action-icon row (stubs until 3.14, 6.3, 3.11.3 wire handlers).
- [x] 4.6b.4 Stats preview strip (placeholder).
- [x] 4.6b.5 PLACE ON PORTAL → loading ring → box closes + impact.
- [x] 4.6b.6 BACK TO BOX stays enabled in loading/errored.
- [x] 4.6b.7 Server contract unchanged.

### 4.7 Browser view reskin
- [x] 4.7.1 Figure cards with smaller `<GoldBezel>`.
- [x] 4.7.2 Element chips as gold-bordered pills.
- [x] 4.7.3 on-portal: desaturated + "ON PORTAL" ribbon.
- [x] 4.7.4 Search shimmer on focus.
- [x] 4.7.5 Empty/filtered-out illustration.

### 4.8 Profile picker reskin
- [x] 4.8.1 `<DisplayHeading>` "WELCOME, PORTAL MASTER".
- [x] 4.8.2 Oversized gold-bezeled swatches tinted by profile color.
- [x] 4.8.3 "+ Add profile" bezel card.
- [x] 4.8.4 Entry bloom, 80ms stagger.

### 4.9 PIN keypad reskin
- [x] 4.9.1 `<FramedPanel>` + mini gold bezel dots.
- [x] 4.9.2 Inset dent + bounce (<100ms).
- [x] 4.9.3 Unlock: shockwave + gold L→R streak.
- [x] 4.9.4 Lockout: red panel + countdown display font.

### 4.10 Profile admin reskin
- [x] 4.10.1 `<FramedPanel>` + themed inputs.
- [x] 4.10.2 Color-picker swatches as mini bezels.
- [x] 4.10.3 Destructive actions red-tinted.

### 4.11 Game picker reskin
- [x] 4.11.1 Game cards with per-game art slot.
- [x] 4.11.2 Stagger-rise entry.
- [x] 4.11.3 Selected-card flash before WS → portal.

### 4.12 Modals + takeover
- [x] 4.12.1 Resume-last-setup modal.
- [x] 4.12.2 Reset-to-fresh confirm (hold, gold-flake fall + desaturation).
- [x] 4.12.3 Takeover/Kaos polish (stays blue; skin ships 5.4).
- [x] 4.12.4 Show-join-code sheet → folded into menu QR card.
- [x] 4.12.4b Menu overlay (kebab surface): join QR + profile chip + 3 actions.

### 4.13 Toasts redesign
- [x] 4.13.1 Color-coded left strip (error/warn/success/info).
- [x] 4.13.2 Top-right slide-in non-blocking; bottom-center for critical.

### 4.14 Ambient polish
- [x] 4.14.1 Screen-to-screen cross-fade + direction motion (`NavDir` signal).
- [x] 4.14.2 Connection pip: breathe / steady green / soft red.

### 4.15 egui TV launcher (residuals in active PLAN)
Design source: `docs/aesthetic/mocks/tv_launcher_v3.html`. Open: 4.15a.6 (review), 4.15a.7 (WGSL port), 4.15.9 (game-switching), 4.15.12 (shader detection), 4.15.13 (shader ring).

- [x] 4.15a.1 Initial HTML mock (superseded, removed).
- [x] 4.15a.2 State machine in `navigation.md` §3.
- [x] 4.15a.3 Procedural cloud WebGL shader (10 arms, cylindrical, 3 knobs).
- [x] 4.15a.4 QR + player orbit (folded into v3).
- [x] 4.15a.5 In-game transparency + shutdown (folded into v3).
- [-] 4.2.12 Mockups review round — overcome by events (2026-04-23). The Leptos reskin was built iteratively against the mocks, and subsequent rounds (4.18.x drift reconciliation) already served as the user-facing review. A separate pre-Leptos review beat would be redundant at this point.
- [-] 4.15a.6 Launcher review round — overcome by events (2026-04-23). 4.15.5-4.15.16 landed with live HTPC iteration; 4.19.x captures the remaining drift explicitly. No benefit to a separate stop-and-review checkpoint.
- [x] 4.15.9 **Game-switching transition.** 2026-04-23. New `LauncherStatus.switching: bool` flag; phone's HOLD TO SWITCH GAMES now calls `/api/quit?switch=true` (via new `api::post_quit_for_switch`) which sets the flag before stopping emulation. Launcher dispatcher: when `switching=true` and screen=Main, force `LaunchPhase::ClosingToInGame { progress: 1.0 }` so iris pins at fully-closed DarkHole, and render a new `render_switching_heading` → "SWITCHING GAMES" (Titan One, HEADING_LG, embossed via `paint_heraldic_title`) over the void. Transparent-in-game render gated on `current_game.is_some()` so RPCS3's library view doesn't peek through during the switch gap (previously it did — that's what Chris was seeing as "no iris close when switching"). `/api/launch` clears `switching=false` on entry so the flag resolves the moment the next boot fires. PLAN's note about `quit_via_file_menu` SendInput→PostMessage migration is stale — under 4.15.16 game quit uses `stop_emulation` (UIA Invoke + PostMessage fallback), never the keyboard-nav file-menu path. 152/152 workspace tests green.
- [-] 4.15a.7 WGSL port via `egui_wgpu` — overcome by events (2026-04-23). The PLAN entry was written when 4.15.5's polar-mesh approximation was the shipped renderer. Since then, `crates/server/src/vortex.rs` landed a real GLSL fragment shader (domain-warped FBM, spiral + log-radial coords, streak overlay, iris mask) via `egui_glow` with `vortex_shader_spike.rs` as the live-tuning playground + `vortex_presets/idle.json` as the saved look. Chris is happy with the current render, so the remaining work in 4.15a.7's text — backend migration from `glow` → `wgpu` — no longer has a user-visible payoff. Reopen if a specific feature ever needs wgpu-only GPU paths.
- [x] 4.15.1 Palette in `crates/server/src/palette.rs`.
- [x] 4.15.2 Titan One as named font family.
- [x] 4.15.3 QR in gold bezel.
- [x] 4.15.4 Status strip + `LauncherStatus`.
- [~] 4.15.5 Polar-mesh vortex approximation shipped; full WGSL port at 4.15a.7.
- [x] 4.15.6 QR card-flip on max-players.
- [x] 4.15.7 Player-orbit indicators (`SessionPip`).
- [x] 4.15.8 In-game transparency via eframe transparent + `PostMessage` input routing.
- [x] 4.15.10 Crash recovery (`LauncherScreen::Crashed`).
- [x] 4.15.11 Shutdown farewell (3s countdown + `ViewportCommand::Close`).
- [x] 4.15.14 Phone-side game-crash overlay.
- [x] 4.15.15 Cover + input routing research spike (chose opaque `WS_EX_TOPMOST` cover + `PostMessage`).
- [x] 4.15.16 RPCS3 lifecycle — launch at server startup; `/api/launch` uses the already-running instance; `/api/quit` uses `StopEmulation` not full shutdown.

### 4.18 Phone UI drift reconciliation (partial)
Done pieces below. Remaining items in active PLAN.

- [x] 4.18.1a mDNS/Bonjour (`crates/server/src/mdns.rs` via `GetComputerNameExW`).
- [x] 4.18.1b "Add to Home Screen" PWA hint (10 unit tests).
- [x] 4.18.1d iOS dark-band diagnosed (html flat-bg seam against body::before gradient).
- [x] 4.18.1e Gradient moved from `body::before` to `html`; `min-height: 100vh` follow-up. Kaos skin class moved to `<html>`.
- [x] 4.18.2 Drop "Skylander Portal" brand text.
- [x] 4.18.3 Profile swatch beside kebab.
- [x] 4.18.4 Pulsing pip only (text label removed).
- [x] 4.18.5 Single `Header` reactive via `Option::map`.
- [x] 4.18.5a MANAGE PROFILES into kebab menu.
- [x] 4.18.5b MenuOverlay context-aware.
- [x] 4.18.6a CreateProfileForm visual parity with PinEntry (heraldic gold-bezel keypad).
- [x] 4.18.7 Prefilled random Skylander name + reroll button.
- [x] 4.18.8 PIN confirm mismatch feedback (shake + banner + wipe).
- [x] 4.18.13 "PORTAL" `<DisplayHeading>` above slot grid.
- [x] 4.18.17 Ownership pip per loaded slot + `resolve_owner` helper + 4 unit tests.
- [x] 4.18.18 Action button labels visible.
- [x] 4.18.21 ConnectionLost overlay (pulsing pip, spinner, manual TRY AGAIN after 3 retries).

### 4.19 egui TV-launcher drift reconciliation (partial)
Done pieces below; extensive residuals in active PLAN.

- [x] 4.19.1 Inventory drift. Headline: spec 8 states, code 3. Itemised as 4.19.2–4.19.22.
- [x] 4.19.2a Startup surface + launch-phase infra (`ui/launch_phase.rs`).
- [x] 4.19.10b QR URL mDNS-based (alongside 4.18.1a).
- [x] 4.19.23 Server-error launcher state (`LauncherScreen::ServerError`) replacing `expect("bind")` panic.

### 4.20 Design system consolidation (4.20.14 residual in active PLAN)

- [~] 4.20.1 `<ActionButton>` extracted for menu-action call sites; ResetConfirmModal + figure_detail BezelButton stay inline. 1 unit test.
- [x] 4.20.2 `<ToyBoxLid>` / `<ToyBoxInterior>` extracted; browser.rs 392 → 241 lines. 1 unit test.
- [x] 4.20.3 `MenuOverlay` extracted to `screens/menu_overlay.rs`.
- [x] 4.20.4 Relocate `Header` to `components/`.
- [~] 4.20.5 `<KaosOverlay>` scaffolded for takeover variant; swap variant lands with 5.3.
- [x] 4.20.5a `ConnectionLost` / `GameCrashScreen` / `PwaHint` as `components/`.
- [x] 4.20.6 Typography scale declared in `:root` (8 tokens).
- [~] 4.20.7 Font-size migration: 88 sites to tokens; 58 off-token kept as literals by design.
- [x] 4.20.8 Motion tokens (`--dur-impact/shudder/sky-drift/hold-confirm`).
- [~] 4.20.9 Duration migration: 4 semantic sites; ambiguous/off-token literals left alone.
- [x] 4.20.10 Launcher typography constants in `palette.rs` (8 pt-size consts).
- [x] 4.20.11 §6.1 mock reference fixed.
- [x] 4.20.12 §3.1 bezel states extended with overlay-badge corner table.
- [x] 4.20.13 §10 egui vortex open-question updated (three paths: shipped polar-mesh / deferred WGSL / rejected frame atlas).

### 4.21 iOS inspector tool (`tools/ios-inspect/`)
Mac-only CLI driving iOS Simulator + Safari via WebKit Web Inspector protocol (`ios-webkit-debug-proxy`). Routes layout/CSS probes; e2e harness handles functional regressions.

- [x] 4.21.1 Spike — WebKit protocol + proxy + simulator + screenshot all verified end-to-end on iPhone 17 Pro / iOS 26.2 sim.
- [x] 4.21.2 `tools/ios-inspect/` built: 8 subcommands (`boot`/`open`/`eval`/`computed-style`/`dump-dom`/`screenshot`/`tabs`/`shutdown`), self-healing proxy lifecycle via `lsof`-based socket discovery.
- [x] 4.21.3 Documented in CLAUDE.md Aesthetic section.
- [x] 4.21.4 4.18.1d root-caused via the tool — html flat-bg color-seam against fixed `body::before` gradient. Recommended fix landed as 4.18.1e.

---

## Phase 6 — Post-Kaos polish (partial)

6.1/6.3/6.4 stay open. 6.2 parent + 6.5 parent have mixed completion; residuals in active PLAN.

### 6.2 Parse `.sky` firmware for per-figure stats (partial)
Encryption discovered + decryption landed. FigureKind range-table split. Remaining: per-kind UI + payloads (Trap/Vehicle/CYOS), investigate 10 CRC-fails, pin Vehicle/CYOS ranges.

- [x] 6.2.0 Validate plaintext assumption — 0/151 CRC-valid on real dumps; encryption confirmed (2026-04-22).
- [x] 6.2.0b AES-128-ECB + MD5-derived per-block keys per blog Appendix B. 141/151 CRC-valid post-landing. `Fixture::build()` encrypts synthetic plaintext so 22 tests run full decrypt→parse roundtrip.
- [x] 6.2.2 `FigureKind::Other` split into `Trap`/`Vehicle`/`RacingPack`/`Cyos`. Trap range `0x0D2..=0x0DC` from real dumps (community-sourced ranges didn't match reality).
- [x] 6.2.7 Superseded by 6.5.5 — scanning proves folder-walk vestigial.

### 6.5 NFC-scan import via ACS122U (partial)
Scan pipeline landed. Remaining sub-items in active PLAN (timeout live-test).

- [x] 6.5.0 SPIKE — `pcsc` crate + `ShareMode::Direct` + `SCardControl(IOCTL 3500)`; 1024 bytes in ~1.4s; Key-A CRC48 per blog Appendix A. macOS needs ACS Unified CCID Driver (Apple inbox driver rejects SCardControl escape).
- [x] 6.5.1 Scanner worker + event stream (`crates/nfc-reader/` lib crate; `nfc-import` feature off by default). `Event::FigureScanned` carries identity only, not raw bytes.
- [x] 6.5.3 Shared-pack storage layout: `<data_root>/scanned/<uid>.sky` (uid-keyed — preserves per-physical-tag state across household duplicates).
- [x] 6.5.4 First-launch onboarding — `probe_reader()` + optional pack. Wizard re-themed with fullscreen + palette + Titan One + starfield (2026-04-23). Live-verified on HTPC; reader-detect path gated on physical console (RDP redirects PC/SC — see auto-memory).
- [x] 6.5.5a Scanned-figure indexer + pack-wins merge. `VARIANT_IDENTITY_MASK = 0x0BFF`. Nickname-promotion escape-hatch keeps pack master identity while surfacing user customization.
- [-] 6.5.5b Superseded by 6.6.

### 6.6 ID-scheme rekey: SHA → tag identity
Pack figures now keyed by `{toy_type:06x}-{variant:04x}` (was SHA-of-path); scans stay `scan:{uid}`; parse-failure fallback `sha:{hex}`. `data/figures.json` rekeyed 504 → 489 (15 collisions collapsed; winners in `data/rekey-log.json`). Newtypes `ToyTypeId` / `TagVariant` / `MaskedVariant` / `TagIdentity` / `MifareNuid` in `skylander_core::figure`. Five phases all landed 2026-04-23; 153/153 workspace tests green.

- [x] 6.6.1 Phase 1 — Newtype foundation + `Figure.tag_identity` prep. Indexer populates via `parse_tag_identity()` helper. `MifareNuid` nfc-reader migration was initially deferred (6.6.1f) and closed in Phase 4 (6.6.4f).
- [x] 6.6.2 Phase 2 — Migration tool (`tools/rekey-figure-ids/`) + dev DB wipe. Idempotent rename-planning loop tracks already-claimed destinations in-memory; 488 image dirs renamed, 14 orphan SHA dirs cleaned, `data/rekey-log.json` committed.
- [x] 6.6.3 Phase 3 — Indexer + core `stable_id` switch. `FigureId::from_tag_identity` produces hyphen-separated lowercase hex. Image-endpoint validator updated to accept 3 canonical forms.
- [x] 6.6.4 Phase 4 — Consumer sweep. FigureId opacity held up; concrete fix was image-endpoint validator + nfc-reader `Uid` → `MifareNuid` newtype. Profile DB schema takes TEXT opaque; no migration needed after wipe.
- [x] 6.6.5 Phase 5 — Live verification. Boot numbers identical pre/post (pack=504, identity-map=489, nicknames_promoted=1). Non-6.6 fallout logged as 4.18.27/4.18.28/4.18.29 in active PLAN.

**Risk callouts from planning (historical):**
- 15 pack `(fid, variant_masked)` collisions collapsed to one entry each; losers effectively dropped from library.
- Any phone bookmark pointing at `/api/figures/<old-sha>/…` 404s post-migration; `boot_id` change triggers auto-reload so live sessions don't notice.
- DB wipe lost dev-data profile state. Acceptable (no real users).

---
