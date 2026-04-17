# Skylander Portal Controller — Execution Plan

This plan is deliberately **research-first**. We don't know enough to plan end-to-end yet. Each phase ends with a review checkpoint where we decide what to plan next. The north star is: **get to a minimal end-to-end testable slice as fast as possible, then grow it.**

Conventions:
- `[ ]` pending, `[x]` done, `[~]` in progress, `[?]` blocked / needs discussion.
- Each research step should end with findings written to `docs/research/<topic>.md` so future phases can reference them.
- Don't skip a review checkpoint. The whole point is to re-plan with new information.

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

## Phase 4 — Aesthetic + UX pass

Phase 3 got the app *working*. Phase 4 makes it feel like Skylanders. Two intertwined workstreams:

- **Visual / aesthetic** — gold bezels, starfield, typography, ambient + state-transition animations. References: `docs/aesthetic/ui_style_example.png`, `Screenshot 2026-04-15 17161?.png`, `kaos_lair_feel.png`.
- **UX / information architecture** — the portal-vs-browser drawer model (SPEC says they're separate, current implementation has them co-mounted), header composition, picking-mode flow, modal stack, navigation between screens. The placeholder CSS has been papering over decisions we never actually made.

Remaining Phase 3 deferrals (3.10.7 ownership, 3.10.8 show-join-code, 3.10e.6 ownership-test, 3.11.4 crystal extra-confirm, 3.12.4 3-option resume modal, 3.19.6 attribution) are **deliberately not folded in here** — they'll land as small follow-ups after Phase 4 using the design system Phase 4 establishes. The Kaos skin (5.4) rides free on Phase 4's CSS-var architecture.

**Direction chosen: Option A — Heraldic** (thick embossed gold bezels, Titan One display + Fraunces body, starfield, filigree plaques). Portal slot state-transition animations confirmed. Screen-level transition animations mock after 4.3 settles the IA. Mocks: `docs/aesthetic/mocks/option_a_heraldic.html` (portal), `transitions.html` (slot state machine). Options B (arcane hex) and C (modernized thin-ring) filed for reference but not pursued.

**Milestone A (4.1 – 4.3):** design tokens + per-screen mockups + IA agreed + screen-transition animations mocked.
**Milestone B (4.4 – 4.12):** reskin + transitions landed screen-by-screen.
**Milestone C (4.13 – 4.17):** egui launcher parity, e2e selector fix-ups, demo.

### 4.1 Design tokens + CSS architecture

- [x] 4.1.1 Palette: starfield blues (`--sf-1/2/3`), gold bezel stops (`--gold-bright/gold/gold-mid/gold-shadow/gold-inner`), per-element gradients (`--el-*`), status colors (loading/error/success). Kaos (5.4) variants defined as sibling vars upfront.
- [x] 4.1.2 Typography: **Titan One** (display), **Fraunces** (body). Self-host under `phone/assets/fonts/`. Both are OFL-licensed; note in `data/LICENSE.md`.
- [x] 4.1.3 Easing + timing tokens — `--ease-spring`, `--ease-sweep`, `--dur-tap`, `--dur-loading-sweep` (0.9s), `--dur-impact` (600ms), `--dur-shudder` (400ms), `--dur-halo-slow` (3.4s), `--dur-idle-float` (4.5s).
- [x] 4.1.4 CSS vars restructured so a body-class swap (`.skin-kaos`) repalettes the whole app without touching component CSS.
- [x] 4.1.5 `prefers-reduced-motion` kill-switch disables ambient drift + halo rotations app-wide.

### 4.2 Static mockups

Direction locked to Option A. Standalone HTML/CSS in `docs/aesthetic/mocks/`, viewable by opening directly. One screen per file.

- [x] 4.2.1 Portal view with all slot states — `option_a_heraldic.html`.
- [x] 4.2.2 Slot state-transition demo — `transitions.html`.
- [x] 4.2.3 Profile picker — "WELCOME, PORTAL MASTER" heading, gold-bezeled profile swatches, "+ Add" card. Mock: `profile_picker.html`.
- [x] 4.2.4 PIN keypad — framed panel, gold-bezel dot display, oversized keys, lockout state. Mock: `pin_keypad.html`.
- [x] 4.2.5 Game picker — names only, big and simple. No serial numbers or figure counts (those are dev/filter concerns, not pick-a-game concerns). Stagger-rise animation. Box art as faded background behind each card title (production: `/api/games/{serial}/boxart` endpoint, bundled images; mock uses gradient placeholders evoking each game's palette). "Launching" state dims other cards, pulses the selected title.
- [x] 4.2.6 Browser / collection view — **resolved: toy box lid** overlay with collapsible search/filter lid + figure grid. Mock: `portal_with_box.html` (checkbox toggles open/closed; click SEARCH to expand lid, scroll to collapse).
- [x] 4.2.7 Picking-mode — integrated into box-open state. "PICKING FOR SLOT N" label at top of collection. No separate banner mock needed.
- [x] 4.2.8 Profile creation flow — name entry → color pick → PIN set → PIN confirm → done. Mocked in `docs/aesthetic/mocks/profile_create.html` as a single-file 4-step stepper (demo controls jump between steps). **Name**: native `<input type="text">` prefilled with a random Skylanders character name (Spyro / Eruptor / Stealth Elf / etc.), with a ↻ roll button to reroll — kids can tap through with the default or type their own, no whitelist ("kid will name themselves poop and that's okay"). **Color**: 4×2 grid of gold-bezeled element swatches (Fire / Water / Life / Magic / Tech / Undead / Earth / Air), selection ringed in gold with a ✓. **PIN set + confirm**: reuses the `pin_keypad.html` layout (framed keypad panel + gold-bezel PIN dots + profile identity row with live name/initial) with label swap "CHOOSE YOUR PIN" → "TYPE IT AGAIN"; mismatch on confirm shows an inline error banner, not a screen change.
- [x] 4.2.8b **Konami-gated profile management.** User idea: the "manage" entry from the PIN keypad screen leads to a hidden admin area gated by the Konami code (↑↑↓↓←→←→BA). "If you're old enough to figure that out you're old enough to manage the profiles." Mocked in `docs/aesthetic/mocks/profile_manage.html` — 3 screens in one file:
    1. **Konami gate** — retro on-screen gamepad (D-pad + A/B) over a 10-dot gold progress indicator. Inputs flash gold on correct; wrong input resets **the whole sequence** (not just the mis-hit slot — brute-force protection), with filled dots visibly fading gold → red → empty during a shake so the wipe is unambiguous. Also accepts physical arrow keys + B/A so desktop review works. On complete: gold flash + auto-advance to manage screen. Hint reads "Contra was such an easy game" — veiled reference for people who grew up with the 30-lives cheat; no code spelled out.
    2. **Profile management** — list of up-to-4 profile rows (bezel + name + last-used/figure-count + 3 actions: rename ✎, reset PIN 🔑, delete 🗑). Dashed "ADD PROFILE" row tucked below. Lock button (back-to-gate) in the header.
    3. **PIN reset** — identity-row + "TYPE A NEW PIN" + 4 PIN dots + the standard keypad. Confirm step (TYPE IT AGAIN) deferred to implementation — the pattern is identical to profile_create step 4 with the same mismatch error slot rule.
    - **Delete is hold-to-confirm** per §4 hold-to-activate — tapping the trash reveals an inline red bar "HOLD TO DELETE SPYRO" inside the row, with a ✕ cancel. Completing the hold animates the row collapsing out. Aligns with reset_confirm + menu_overlay danger pattern.
    - Demo controls include a "▸ auto-type code" button that fires the 10-key sequence at ~220ms/key so the gate unlock animation can be reviewed without memorising anything.
- [x] 4.2.8b Figure detail view — mock: `docs/aesthetic/mocks/figure_detail.html`. Default / loading / errored states (demo controls in the corner toggle between them).
- [x] 4.2.9 Takeover / Kaos screen — mocked in full Kaos palette directly (purple/pink/magenta, not blue placeholder). `kaos_takeover.html` (eviction + KICK BACK IN) + `kaos_swap.html` (mid-game figure swap + BACK TO THE BATTLE). Kaos skin swap for the *rest* of the app deferred to 5.4.
- [x] 4.2.10 Resume-last-setup modal, reset-to-fresh confirm, show-join-code — all use `<FramedPanel>` treatment. Mocks: `resume_prompt.html` (resume vs. start-fresh after a session unlock), `reset_confirm.html` (hold-to-confirm with the gold-flake reset animation per 4.2.14.a). Show-join-code is folded into `menu_overlay.html`'s prominent QR card per 4.12.4b — no separate sheet needed since the menu is the always-available "how does my friend join?" surface.
- [x] 4.2.16 **Connection-lost overlay.** What the phone shows when the WebSocket drops mid-session. Mocked in `connection_lost.html`: faded portal underlay + red pulsing disconnected pip (✕) + "LOST CONNECTION" heading + "reconnecting…" spinner bar (auto-retry). Demo toggle reveals a manual "TRY AGAIN" gold button for when auto-retry gives up. Hint: "make sure the TV is on and you're on the same Wi-Fi."
- [x] 4.2.17 **Empty states.** Three variants in `empty_states.html` (demo controls toggle): (a) no profiles / first launch — "NO PORTAL MASTERS" + CREATE PROFILE button, (b) no filter matches — inside the toy-box interior with active filter chips visible, "NO HEROES FOUND" + CLEAR FILTERS, (c) no games detected — "NO GAMES FOUND" + guidance to check RPCS3's library. All use a shared dimmed gold-bezel emblem with `?` glyph + the heraldic heading/body/hint stack.
- [x] 4.2.11 Screen-transition animation demo — `screen_transitions.html`. Auto-plays the golden-path sequence (ProfilePicker → PIN → GamePicker → Portal) on loop. Each screen is a simplified silhouette — headings + placeholder shapes — enough to identify what's coming/going. Deeper = slide up, back = slide down, per `navigation.md` direction convention. 420ms `ease-spring` default. Controls: prev/next step, auto-play toggle (2s hold between slides), speed knob (0.5×/1×/2×). Info bar shows the active edge + timing. **This locks the skeleton** (which direction, how long, what easing). Custom polish effects (particle bursts, gold streaks, cloud-part reveals) land in 4.14.
- [ ] 4.2.12 Review round with user. Iterate before touching Leptos.
- [ ] 4.2.13 **Contrast + readability cleanup pass.** After the first mock batch, user review caught systemic `rgba(..., <1)` text dimming across most screens. §2 Typography of `design_language.md` gained a "Contrast & readability contract" (opaque text only, role table: primary / accent / muted). First-pass audit staged to `docs/aesthetic/mocks/_contrast_pass/`. Follow-up items below finish the pass before promoting it back to `mocks/`.
    - [x] 4.2.13.1 Stage mocks to `_contrast_pass/` folder.
    - [x] 4.2.13.2 Agent-assisted audit: 43 alpha→solid edits across 10 files (role table applied).
    - [x] 4.2.13.3 **Archive commit first** — PLAN update + `_contrast_pass/` staging committed while `option_a_heraldic.html` / `option_b_arcane.html` / `option_c_modernized.html` still live in `mocks/`. Preserves the three direction options in git history for reference.
    - [x] 4.2.13.4 `figure_detail.html` — fix remaining gold-on-gold contrast spot the rule missed (user flagged post-audit). *Fixed: `.action-btn` darkened inner disk so the gold glyph/border have real contrast.*
    - [x] 4.2.13.5 Subtitle audit — `resume_prompt.html` "pick up where you left off?" is the gold-subtitle reference; `game_picker.html` "choose your adventure" is the blue-surface reference. Align the other mocks' subtitles to one of these two. *Verified all mocks use one of two canonical variants: sentence-case (Fraunces italic 600, 15px, `#fff4e0`, 0.08em, small gold glow) for intimate tone; uppercase (Fraunces italic 600, 13px, `#fff4e0`, 0.20–0.25em, dark shadow) for directive/section. Both use primary warm-white color.*
    - [x] 4.2.13.6 `portal_with_box.html` — restore swipe-gesture affordance visuals lost in the pass; fix filter-category buttons being clipped/cut off by adjacent elements. *Fixed: added `.lid-grabber` pill (iOS-style) animated to hint at swipe gesture; `.search-expanded` now scrolls internally with a soft bottom fade so chips can never be clipped; lid max-height bumped 320→380px.*
    - [x] 4.2.13.7 `reset_confirm.html` — keep `KEEP SPYRO` as tertiary outline (NOT blue secondary — it's not an alternative action, it's a back-out), remove the alpha'd border, apply the readability rules. *Fixed: solid muted-gold `#c9a84a` border, inset highlight for physicality.*
    - [x] 4.2.13.8 Delete `option_a_heraldic.html` / `option_b_arcane.html` / `option_c_modernized.html` from `mocks/` (archived at commit 4.2.13.3). Direction is locked, old options are clutter.
    - [x] 4.2.13.9 `design_language.md` §2 — carve-out rule: non-actionable **flavor text** (Kaos quote glyphs, taunt attribution, decorative insult copy) may relax the opaque-text rule moderately — readability of the insult must still hold, but some ambient dim is allowed since these aren't interactive controls.
    - [x] 4.2.13.10 `design_language.md` §4 Layout — new rule: **never clip interactive buttons / tap targets** behind other layers. Applies to filter chips, drawers, modals. *Added as "Tap-target reachability" subsection; gives 4 fixes in priority order and points to `portal_with_box.html` as the worked example.*
    - [x] 4.2.13.11 Promote `_contrast_pass/*.html` → `mocks/*.html`, remove staging folder, final commit.
    - [x] 4.2.13.12 **Defer real swipe gestures on the toy-box lid to Leptos implementation.** The static HTML/CSS mock uses a click-to-simulate shortcut on the grabber/hint (tap the pill = "swipe down"). Proper pointer-tracked swipe detection (pointerdown → move → up, distance + direction threshold, scroll-gesture conflict resolution against the figure-grid scroll) is awkward to do in vanilla JS and belongs in the Leptos `<ToyBoxLid>` component (§6.7) where it can share state with the scroll-collapse handler. The *visual affordance* — grabber pill, direction-changing hint text, lid max-heights — are locked by the mock; only the input handling is deferred. *Landed: `Browser` now drives a `BoxState::{Closed, Compact, Expanded}` signal via pointerdown/move/up on the grabber + title row (48px distance threshold, 10px tap-travel cap, `setPointerCapture` so the gesture keeps firing if the finger drifts onto the grid); SEARCH click + figure-grid `on:scroll` auto-collapse wire into the same signal. Added a minimal `.lid-open-p4.closed` CSS variant (only new visual; grabber/compact/expanded heights untouched) plus `touch-action: none` on the gesture zone.*
- [x] 4.2.14 **Destructive confirmation — hold-to-activate pattern.** User-review follow-up: RESET (figure) and SHUT DOWN (server) are single-tap today; adding a ~1.2s press-and-hold with a white progress fill sweeping left→right so a fat-finger tap can't wipe a figure or kill the app. Release-to-cancel; complete-to-fire (button flashes). Applied to `reset_confirm.html` (RESET → HOLD TO RESET) and `menu_overlay.html` (SHUT DOWN → HOLD TO SHUT DOWN). `design_language.md` §4 gains the "Destructive confirmation — hold-to-activate" subsection plus a contrast note that **recoverable** destructive actions (REMOVE) stay single-tap.
    - [x] 4.2.14.a **Post-destructive-action state animations.** Mocked in-situ in the origin files (not separate prototypes) so the design is reviewed alongside the trigger it belongs to. Each mock has demo controls in the upper-right for review, hidden on phone-viewport widths. Real flow wires the animations as noted below:
        - **RESET (figure)** — *local* to the acting phone. `mocks/reset_confirm.html`. Sequence: `.fired` flash (420ms) → 14 gold flakes fall from bezel rim (random drift + rotation, ~1000ms) while bezel/plate/title desaturate (1100ms) → panel dismisses + scrim fades + underlay unveils to a desaturated "fresh figure" detail view.
        - **CHOOSE ANOTHER GAME** — affects **every connected phone**. `mocks/menu_overlay.html`. Trigger is the server's game-exit broadcast, NOT a local button handler. Sequence: panel dismisses → underlay header recedes + `PORTAL` title 3D-folds away (rotateX −80deg, 520ms) → `PICK AN ADVENTURE` placeholder scales in. When wiring, dispatch the animation from the WS handler.
        - **SHUT DOWN** — affects **every connected phone**. `mocks/menu_overlay.html`. Trigger is server shutdown event + WS close. Sequence: panel dismisses → starfield + sky + underlay dim (brightness → 0.15, saturate → 0.4, 1500ms) → `SEE YOU NEXT TIME, PORTAL MASTER` fades in with the same 3.2s breathe as TV launcher state 8. Local button's `.fired` flash is the lead-in; broadcast drives the rest.
        - **SWITCH PROFILE** — local only, single-tap. `mocks/menu_overlay.html`. Sequence: panel dismisses → current identity (swatch + name + `PORTAL` title) desaturates and dims (800ms) → profile picker with `WELCOME, PORTAL MASTER` arrives; the just-left profile's tile renders desaturated as a visual receipt.
- [x] 4.2.15 **Portal slot loaded-selection state — REMOVE overlay.** Per PLAN 4.3.3 "tapping a loaded slot shows Remove/Reset actions directly on the portal (no box needed)." Mocked in `portal_with_box.html`: tap a loaded slot → slot gets a bright gold ring + scale 1.04 + a full-width red `REMOVE` bar (~1/3 slot height, edge-to-edge) overlays the middle of the figure portrait; tap the bar to execute, tap outside to dismiss. **Auto-dismisses after 5s** of no interaction so a stray tap doesn't leave the slot armed. Single-tap confirm (not hold) — removing a figure from the portal is recoverable (put it back with one tap). Only REMOVE for now; RESET would chain in later via the figure-detail path.

### 4.3 UX reorganization — information architecture

- [x] 4.3.1 **Portal vs browser — DECIDED: toy box lid.** Portal is the primary screen (2-wide × 4-tall slot grid to maximise bezel size). At the bottom sits a "toy box lid" — a wooden-textured bar with a gold clasp and "COLLECTION" label. The open lid sits at the top **below the header** (no overlap) as slatted wood, compact by default with a single "SEARCH" button. Tapping SEARCH expands the lid downward to reveal a search input + drill-down filters (Games → each game, Elements → list, Category → Vehicles/Traps/Minis/Items). Default sort is **current-game-compatible, then last-used** — so filters are opt-in and the figures kids want are already on top. A bottom dark-gradient pinned to the footer gives a depth illusion (figures receding into the box); a top fade makes figures scroll behind the lid rather than clip. Metaphor: kids digging through a Skylanders bin to find the right figure. Mock: `docs/aesthetic/mocks/portal_with_box.html`.

  **Gesture model** (progressive swipe-down through open states):
  - Closed → **tap** or **swipe up** on lid → Open (compact).
  - Open (compact) → **swipe down** on lid → filters expand. (Tap SEARCH does the same thing.)
  - Open (expanded) → **swipe down** again → box closes entirely.
  - Scrolling the figure grid auto-collapses expanded filters back to compact (looking deeper into the box).
  - Tap the lid's "✕" close button from any open state → box closes.
  - Picking a figure and placing it → box closes after the portal-impact animation.

  See `docs/aesthetic/design_language.md` §6.7 for the full gesture table + implementation notes (pointer events, 40–60px translate threshold, direction check).
- [x] 4.3.2 **Header composition — DECIDED.** Left-to-right: **kebab (⋮)** → profile swatch → profile name + current game → connection pip + "live" label. No inline action buttons. All actions (quit/switch/join/about) consolidated into the kebab menu overlay (see 4.12.4b). Keeps the header quiet, reduces risk of a kid accidentally tapping a red ✕ to quit mid-game, and replaces the cryptic ⌘ join-code icon.
- [x] 4.3.3 **Picking-mode flow — DECIDED: slot tap opens the box.** Tapping an empty slot opens the toy box (same as tapping the lid). No "picking for slot N" label is shown to the user — the server tracks which slot is being picked for; the client just passes the figure id and the server places it. Picking a figure closes the box and triggers the portal impact animation on the target slot. Tapping a loaded slot shows Remove / Reset actions directly on the portal (no box needed). No sticky banner.
- [x] 4.3.6 **Collection default sort — game-compatible, then last-used.** Server ranks `/api/figures` by `(compat_with_current_game DESC, last_used_at DESC NULLS LAST, canonical_name ASC)`; phone renders in received order. Compat heuristic lives in `crates/core/src/compat.rs` (`is_compatible` + `game_of_origin_from_serial`) with unit tests for the release-order and vehicle-exception rules. Per-profile `figure_usage.last_used_at` pulled via new `ProfileStore::fetch_usage`.
- [x] 4.3.7 **Figure detail view flow.** Tapping a figure in the box opens the **detail view**, not a direct placement. Transition: other figures + wooden box interior fade to ~25% opacity, selected figure "lifts up" (scale + translate animation) into a blue framed panel that fades in. The lifted figure gets a soft aura + slow-rotating rays (like the in-game figure reveal). The panel hosts: figure name + element/game metadata, an action-icon row (appearance / stats / reset — icons are placeholders in Phase 4; 3.14 wires up variant cycling, 6.3 wires up stats drill-down, 3.11.3 wires up reset-to-fresh), a compact stats preview strip, `PLACE ON PORTAL` (primary) + `BACK TO BOX` (secondary) buttons. **Back** reverses the entrance animation (panel fades, figure descends into grid, other figures restore). **Place** triggers the portal loading ring on the hero figure; on success the toy box lid closes and the portal-impact animation plays on the server-assigned slot; on failure an inline error banner slides in from the top of the panel (no toast for this — the detail view stays put so the user can retry or go back). Server still decides the slot; the phone never computes it.
- [x] 4.3.4 **Modal stack semantics.** Documented in `docs/aesthetic/navigation.md` §2. Three categories: full-screen replacements (routes), scrim modals (one-at-a-time, priority-resolved), inline overlays (portal-internal state). Key rules: no stacking (ConnectionLost always wins), no auto-dismiss on Kaos events, reconnect-survival table per modal, browser-back behavior per category.
- [x] 4.3.5 **Navigation map** documented in `docs/aesthetic/navigation.md` §1. ASCII state graph + navigation-edge table with animation direction convention (deeper=slide-up, back=slide-down, lateral=slide-right, modals=scrim-fade+spring, Kaos=own wash). §3 reserves landing spots for future screens (stats 6.3, variants 3.14, crystal confirm 3.11.4, ownership badge 3.10.7).

### 4.4 Shared Leptos components

- [x] 4.4.1 `<GoldBezel>` — circular gold frame with element-tinted inner plate. Props: `size`, `element`, `state` (default / picking / loading / loaded / errored / disabled), child content. **Primary child is an `<img>` thumbnail** (`/api/figures/{id}/image?size=thumb`) that fills the circle; the element-gradient plate shows through as a fallback for figures without wiki images (16 of 504). Profile swatches use an initial letter instead. Reused by portal slots, browser cards, profile swatches, empty-slot "+", color-picker swatches, and the hero bezel in the figure detail view.
- [x] 4.4.2 `<FramedPanel>` — parchment-blue panel with a multi-stop gold gradient border for modal surfaces (PIN, admin, resume, confirms, takeover, show-join, figure detail). **No corner brackets** — the gradient border does the framing; brackets were visual noise. Consistent across all panels.
- [x] 4.4.3 `<DisplayHeading>` — two-tone outlined title: gold fill (linear-gradient via `background-clip: text`) + dark-gold `-webkit-text-stroke` + drop-shadow. Matches the "DARES" / "IMAGINITE" treatment.
- [x] 4.4.4 `<RayHalo>` — rotating conic-gradient halo for selected / loading state, masked to a ring.
- [x] 4.4.5 `<FigureHero>` — the lifted figure presentation used in the detail view: oversized `<GoldBezel>` + soft aura + slow-rotating conic-gradient rays. Takes a `state` prop (`default`, `loading`, `errored`) that switches the bezel treatment + triggers the loading ring overlay. Reused by the Kaos swap-announcement overlay (5.3).

### 4.5 Starfield background + ambient motion

- [x] 4.5.1 Layered starfield: multiple radial gradients + tiled SVG star-dot layer + slow parallax drift (~40s loop). Single shared background on `body`, so screen changes feel continuous.
- [x] 4.5.2 Optional "magic dust" — sparse floating-particle layer; evaluate CPU cost on the HTPC before committing. **Summary:** Added `<MagicDust />` as first child of `.app` with 24 radial-gradient particles, each randomized via inline `--drift`/`--peak-opacity`/delay/duration CSS vars. `prefers-reduced-motion` hides the layer entirely.

### 4.6 Portal view reskin + state transitions

- [x] 4.6.1 Slots render through `<GoldBezel>`. Empty slot = dimmed bezel with a "+" in the center.
- [x] 4.6.2 Empty → Picking: scale 1.05 spring-ease, outer gold glow intensifies, `<RayHalo>` fades in and begins a slow rotation.
- [x] 4.6.3 Pick → Loading: halo rotation speeds up, a gold sweep travels once around the bezel ring (~1s loop), inner plate dims 20%.
- [x] 4.6.4 Loading → Loaded: "portal impact" — radial white→gold flash scales from 0 → 2× and fades (~400ms), bezel brightness spikes briefly, then settles into a subtle 4s idle float (±2px).
- [x] 4.6.5 Loaded → Cleared: desaturate + shrink, fade thumb out to element plate (~200ms).
- [x] 4.6.6 Errored: red-tinted bezel, short shake animation + persistent subdued red glow until dismissed.
- [x] 4.6.7 Slot tap feedback: inner-plate "dent" (inset shadow + 0.96 scale), spring back.

### 4.6b Figure detail view (the "lifted" hero panel)

Implemented as a new screen reached by tapping a figure inside the toy box. Shell only in Phase 4; the action-icon wiring lands with later work (3.14 variants → appearance icon; 6.3 stats → stats icon; 3.11.3 reset → reset icon).

- [x] 4.6b.1 `<FigureDetail>` Leptos component. Props: `figure: PublicFigure`, `placed_by: Option<String>` (for stats strip context), `on_back: Callback`, `on_place: Callback`. Internal state: `idle | loading | errored { message: String }`.
- [x] 4.6b.2 Entrance animation: other figures + box interior crossfade to ~25% opacity; selected `<FigureHero>` lifts from its grid position via FLIP-style transform; framed panel fades in behind (staggered ~120ms).
- [x] 4.6b.3 Action-icon row: three `<BezelButton>`s with placeholder icons + disabled state + "coming soon" tooltip until 3.14/6.3/3.11.3 wire real handlers. Keeps Phase 4 contained while locking the layout.
- [x] 4.6b.4 Stats preview strip — placeholder values in Phase 4 (shows the layout but reads from a stub). 6.3 pulls real numbers from `/api/profiles/:profile_id/figures/:figure_id/stats`.
- [x] 4.6b.5 `PLACE ON PORTAL` (primary) calls `on_place`; while awaiting the WS confirmation, state flips to `loading` and the hero bezel's loading ring spins. On WS `SlotChanged` with matching figure_id → box lid closes, portal-impact animation fires on the target slot. On error → state flips to `errored`, inline error banner slides in from top of panel. **Client-side timeout (8s default, configurable)**: if no `SlotChanged` or error arrives, auto-flip to `errored` with a "took too long — try again?" message. Prevents the user from sitting on a permanent loading spinner if the WS drops mid-operation.
- [x] 4.6b.6 `BACK TO BOX` (secondary) reverses the entrance animation and returns to the collection grid with scroll position preserved. **Must remain enabled in the loading and errored states** — the user should never be trapped waiting on the server. Backing out mid-load doesn't cancel the load (the figure may still appear on the portal once it completes); it just returns the user to the box. If the WS eventually reports success while the user is elsewhere, no UI disruption — the slot just populates.
- [x] 4.6b.7 Server contract unchanged: phone sends figure_id; server picks the slot (first-available or picking-for-specific-slot context). Phone never names a slot in the request.

### 4.7 Browser view reskin

- [x] 4.7.1 Figure cards use a smaller `<GoldBezel>` as the portrait; card itself becomes a minimal frame under the bezel.
- [x] 4.7.2 Element chips redesigned as gold-bordered pills with element-tinted fills.
- [x] 4.7.3 `on-portal` state: desaturated bezel + a "ON PORTAL" gold ribbon across the corner.
- [x] 4.7.4 Search input: gold shimmer sweep along the border on focus (one-shot).
- [x] 4.7.5 Empty/filtered-out state: themed empty-state illustration + copy, not the plain text we have today.

### 4.8 Profile picker reskin

- [x] 4.8.1 Big `<DisplayHeading>` "WELCOME, PORTAL MASTER".
- [x] 4.8.2 Profile cards: oversized gold-bezeled swatches with the initial rendered in the display font. Each profile's color tints the inner plate.
- [x] 4.8.3 "Add profile" affordance → prominent "+" bezel card instead of the placeholder button.
- [x] 4.8.4 Entry animation: cards bloom in from center, 80ms stagger.

### 4.9 PIN keypad reskin

- [x] 4.9.1 `<FramedPanel>` surround. PIN dots become mini gold bezels that fill with element-tinted plates as digits are entered.
- [x] 4.9.2 Key press feedback: inset-shadow dent + soft "click" animation (≤100ms), plus subtle haptic-adjacent bounce.
- [x] 4.9.3 Unlock success: shockwave ring outward from profile swatch + gold streak L→R sweep as the panel fades out.
- [x] 4.9.4 Lockout state: panel tinted red, countdown in the display font, keys visually disabled (not hidden).

### 4.10 Profile admin reskin

- [x] 4.10.1 `<FramedPanel>` surround. Form inputs themed (rounded, gold focus outline, display-font labels).
- [x] 4.10.2 Color picker swatches are mini gold bezels.
- [x] 4.10.3 Destructive actions (delete, reset-PIN) clearly marked with a red-tinted framing.

### 4.11 Game picker reskin

- [x] 4.11.1 Game cards with room for per-game artwork (placeholder text if we don't have the assets yet).
- [x] 4.11.2 Card entry animation: stagger-rise from below, 80ms per card.
- [x] 4.11.3 Selected-card confirmation flash before the WS signal flips the UI to portal view.

### 4.12 Modals + takeover screen

- [x] 4.12.1 Resume-last-setup modal (3.12.2 UI): `<FramedPanel>` with a figure-preview row of gold bezels, "Resume" + "Start fresh" CTAs.
- [x] 4.12.2 Reset-to-fresh confirm (3.11.3 UI). Replaces portal slot's `window.confirm()` with a red-bezeled `<FramedPanel>` modal: hold-to-confirm primary, KEEP-FIGURE tertiary, gold-flake fall + bezel desaturation on fire (per 4.2.14.a). Server `post_reset` fires immediately; ~1500ms animation budget masks IO latency, then `reset_target` clears and the modal dismisses.
- [x] 4.12.3 Takeover/Kaos screen polish — stays blue (the Kaos skin itself ships with 5.4; this is just restyled).
- [x] 4.12.4 Show-join-code sheet — folded into 4.12.4b menu overlay's QR card (no separate sheet). Shell-only: the inner `.menu-qr-inner` renders a "QR" placeholder; real QR content wiring lands with 3.10.8 follow-up.
- [x] 4.12.4b **Menu overlay** (opened by header kebab). Single surface that consolidates: (a) show-join-code QR (prominent, always visible — answers "how does my friend join?"), (b) current profile chip for context, (c) three stacked actions with icon + title + one-line description:
      - **SWITCH PROFILE** — locks current session, returns to profile picker. Other sessions on other phones keep their profiles.
      - **CHOOSE ANOTHER GAME** — quits the current RPCS3 game, returns to game picker; profile stays unlocked. Confirm required (destructive for the other player if they're mid-play).
      - **SHUT DOWN** — red/danger treatment — closes the whole server + RPCS3. Confirm required; labelled "ask a grown-up first" so kids know it's the adult exit.
      Dimmed scrim behind, tap scrim or ✕ to close. Uses `<FramedPanel>` shell. Mock: `docs/aesthetic/mocks/menu_overlay.html`. Absorbs 3.10.8 (show-join-code) entirely — that line item's UI work lands here; server-side it's already done.

### 4.13 Toasts redesign

- [x] 4.13.1 Color-coded left strip (error / warn / success / info). Consistent typography with the rest of the app.
- [x] 4.13.2 Slide-in from top-right variant for non-blocking notifications; existing bottom-center kept for critical errors.

### 4.14 Ambient polish

- [x] 4.14.1 Screen-to-screen transitions: consistent cross-fade + slight motion direction based on navigation depth (deeper = slide up, back = slide down). **Summary:** Added `NavDir` enum + `nav_dir` signal in `lib.rs`, two `Effect`s tracking `unlocked_profile`/`current_game` to set direction. Each `Show` branch wraps in `<div class={screen_cls("screen-X")}>` where `screen_cls` captures `nav_dir.get_untracked()` at branch-mount so animations don't re-trigger on direction changes mid-screen. CSS adds `.screen-fwd`/`.screen-back`/`.screen-takeover` 240ms slide+fade keyframes; `prefers-reduced-motion` neutralises.
- [x] 4.14.2 Connection-status pip in the header gets a breathe animation while connecting (1.6s scale + halo), steady green glow when connected, soft red glow when disconnected. `prefers-reduced-motion` already neutralises the breathe via 4.1.5's blanket rule.

### 4.15 egui TV launcher — design cycle + implementation

The TV launcher is a full UX surface with its own 8-state machine (see `navigation.md` §3). Gets the same mock → design doc → implement treatment as the phone app. All cloud assets are **procedurally generated** at runtime — no pre-rendered frames or game captures (copyright avoidance). Design source-of-truth is `docs/aesthetic/mocks/tv_launcher_v3.html` — v1 (CSS conic spike) and v2 (SVG turbulence) were exploration fork-points and have been removed.

#### 4.15a TV launcher design cycle (mock + iterate)

- [x] 4.15a.1 Initial HTML mock — `tv_launcher.html` (since removed). Two-state (loading/ready) with CSS conic-gradient cloud spike. Verdict: stylized but not organic enough → led to v2.
- [x] 4.15a.2 State machine documented in `navigation.md` §3. Final shape: Startup → Booting → Compiling Shaders → Awaiting Connect → Players Joined → Max Players → In-Game → Shutdown. QR card-flip on max players, iris choreography between cloud-visible and game-visible states.
- [x] 4.15a.3 Procedural cloud — `tv_launcher_v3.html`. Final approach: **WebGL fragment shader** (cylindrical-spiral simplex FBM, 5-octave noise, 10 iris arms). Three independent uniforms — `irisRadius` (0=clear / 1.6=full), `rotationSpeed` (arm spin rad/s), `inflowSpeed` (cloud drift toward center r/s) — animated independently rather than as 3 discrete modes. v2's SVG `feTurbulence` approach was a stepping stone; the shader gives organic noise without shipping assets. **Decoupling note:** arm spiral uses plain `r` (not `r + inflow*t`) so inflow doesn't compound rotation into nausea-inducing arm spin. **Seam fix:** cylindrical sampling (cos/sin of theta) eliminates the polar-seam artifact at theta=±π.
- [x] 4.15a.4 QR + player-orbit mock — folded into `tv_launcher_v3.html` states 3 (Awaiting Connect), 4 (Players Joined), 5 (Max Players, with QR card-flip). Player pips orbit on rx=560 ry=400 ellipse at z-index 9 so they pass behind the title text.
- [x] 4.15a.5 In-game transparency + shutdown mocks — folded into `tv_launcher_v3.html` states 7 (In-Game) and 8 (Shutdown Farewell). **Reconnect QR refinement vs original spec:** moved from bottom-right (always-on) to **upper-right, hidden by default**. Only appears when *every* phone has disconnected — an "everyone left, anyone come back" cue, not a persistent overlay. Shutdown sequence: 2.2s read pause on "SEE YOU NEXT TIME, PORTAL MASTER" → 1.6s fade-to-black → "(launcher will exit)" hint surfaces.
- [ ] 4.15a.6 Review round — iterate before touching egui.
- [ ] 4.15a.7 **Cloud-vortex polish: port the simplex-FBM WebGL shader to WGSL via `egui_wgpu`.** 4.15.5 ships a polar-mesh approximation using egui's native `Painter::add(Mesh)` (cheap sin/cos band noise, no wgpu integration); the ported shader gives the organic-fluff look that matches `tv_launcher_v3.html` 1:1. Work: (a) flip `eframe`'s backend from `glow` to `wgpu` in `crates/server/Cargo.toml`, (b) port `FRAG_SRC` in the mock to WGSL (simplex 3D noise + 5-octave FBM + spiral coord + iris/vignette/centre-hole composite), (c) set up a `wgpu::RenderPipeline` + uniform buffer (time, irisRadius, rotationSpeed, inflowSpeed, resolution, spiralTightness, tunnelDepth, reducedMotion) behind an `egui_wgpu::CallbackFn`, (d) replace `vortex::draw` with the wgpu version behind the same public signature (or a feature flag for A/B). Defer until after 4.15.6–4.15.13 so we don't churn on visual tuning twice. Chris already said no lingering design questions, so this is purely implementation-shape polish, not a re-design.

#### 4.15b egui implementation

- [x] 4.15.1 Shared palette via egui `Visuals` — starfield-blue background, gold accents. Readable from ~10 ft on an 86" TV (≥32pt body, ≥64pt QR label). **Summary:** new `crates/server/src/palette.rs` module holds `Color32` constants mirroring the phone's CSS tokens (`SF_1/2/3`, `GOLD`/`GOLD_BRIGHT`/`GOLD_2`/`GOLD_SHADOW`/`GOLD_INK`, `TEXT`/`TEXT_DIM`, `DANGER`, `SUCCESS_GLOW`). `palette::apply(&cc.egui_ctx)` is called once at `LauncherApp::new` to install `Visuals::dark()` with `panel_fill = SF_3`, `window_fill = SF_2`, `override_text_color = TEXT`, gold selection stroke. `ui.rs` replaces its hardcoded RGB literals (white heading, gray-180 subtext, `0xffcf3a` URL, `0x8a2020` exit button fill) with palette references. No visual regression since the previous shape was already close to the palette values; the new canonical source means 4.15.3+ pick up the same colors without drift.
- [x] 4.15.2 Display font loaded into egui so the PC-side and phone-side feel unified. **Summary:** Titan One TTF (55 KB, OFL, from the `google/fonts` GitHub mirror) committed under `crates/server/assets/fonts/TitanOne-Regular.ttf`. New `crates/server/src/fonts.rs` module `include_bytes!`-embeds it and registers it as a named `egui::FontFamily::Name("titan_one")` — opt-in per `RichText::family(...)`, not auto-applied to `Proportional` (Titan One is a heavy display face and looks cramped below ~20pt; keeping egui's default Noto/Hack stack for body/monospace). `LauncherApp::new` calls `fonts::register(&cc.egui_ctx)` alongside `palette::apply`. The launcher heading is now `SKYLANDER PORTAL` in Titan One gold at 80pt — matches the phone's display-heading treatment. Subsequent 4.15 items (QR label, status indicators, game-name) can opt in via `.family(egui::FontFamily::Name(fonts::TITAN_ONE.into()))`.
- [x] 4.15.3 QR code framed in a gold bezel equivalent. **Summary:** new `qr_in_gold_bezel` helper in `ui.rs` stacks two `egui::Frame`s — outer gold body with a `GOLD_INK` hairline stroke + soft drop shadow, inner `SF_3` bezel plate with a `GOLD_SHADOW` stroke framing the QR itself. Rectangular rather than the phone's circular `GoldBezel` (QR needs a square frame) but the same colour story. Gradient + multi-layer inset shadows from the CSS version would need a custom `egui::Painter` pass; the stacked-Frame approach is ~95% of the visual payoff for 5% of the code. Revisit via custom painter in 4.15a polish if the TV looks flat in person.
- [x] 4.15.4 Status indicators — RPCS3 connection dot (absorbs the old 2.8.4 deferral), client count, current-game name. **Summary:** new `LauncherStatus { rpcs3_running: bool, current_game: Option<String> }` on `AppState`, held behind a `std::sync::Mutex` so the eframe main thread can clone-read it every ~250ms frame without awaiting a tokio lock. Written to by `/api/launch` (on successful boot → running=true + game name) and `/api/quit` (→ running=false + None) in `http.rs`. `ui.rs`'s new `status_strip` renders a header row: a ~10px dot painted via `egui::Painter` (`SUCCESS_GLOW` while running, `TEXT_DIM` when idle, `GOLD_INK` hairline ring for contrast) + the current game name in Titan One gold at 26pt — or an italic "no game running" dim line when idle. Client count was already rendered below the QR (pre-existing); figure count stays the same. Tooltip on the dot reads "RPCS3 running" / "RPCS3 idle". The update-on-crash path lands with 4.15.14's watchdog (agent delivered) which broadcasts `GameChanged { current: None }`; we can bolt a quit-like update there later — non-blocking for 4.15.4 shipping.
- [~] 4.15.5 **Procedural cloud vortex.** Reproduce the WebGL shader from `tv_launcher_v3.html` in egui — open question is the rendering pipeline. Path A: ship a fragment shader via `egui_wgpu` custom paint callback (matches the mock 1:1, GPU-cheap, requires wgpu integration scaffolding). Path B: bake the shader to a texture atlas at startup (~60–90 frames at 960×540, generated once) and play frames back via `egui::Image` — simpler but loses continuous-knob control over `irisRadius` / `rotationSpeed` / `inflowSpeed`. Recommend Path A. No shipped image assets either way. **Shipped (Path C pragmatic): polar-mesh approximation** in the new `crates/server/src/vortex.rs` module — ~2k triangles per frame via egui's native `Painter::add(Mesh)`, no wgpu/WGSL integration required. Captures the iconic shape: concentric density ramp (SF_1 → mid-blue → warm bright), 10 rotating arms, iris open/close knob, centre hole kept clear for the QR. Cheap sin/cos "band noise" stands in for simplex FBM — reads as banded density at 10 ft rather than organic fluff, but the eye fills in the rest. Repaint cadence bumped from 250ms → 16ms (60 FPS) for smooth arm rotation. `VortexParams::default` keeps iris_radius=1.2, rotation_speed=0.08 rad/s, inflow_speed=0.18 — matches the mock's idle tuning. **Polish follow-up tracked against 4.15a:** port the simplex-FBM WebGL shader to WGSL and wire `egui_wgpu` custom paint callback for the real thing. Gets the organic-fluff look and lets 4.15.8 / 4.15.9 drive the three knobs continuously via animation rather than baking into `Default`. Would swap `eframe`'s backend from `glow` to `wgpu` (currently `features = ["default_fonts", "glow", "wayland", "x11"]` in `crates/server/Cargo.toml`). Deferred until after 4.15.6–13 land so we don't churn on visual tuning twice.
- [ ] 4.15.6 QR card-flip on max-players (Y-axis rotate, back face shows "MAXIMUM PLAYERS REACHED").
- [ ] 4.15.7 Player-orbit indicators around the QR (gold-bezeled circles with profile color + initial).
- [x] 4.15.8 In-game transparency — `eframe::Frame` transparent mode, reconnect QR overlay in corner. **Input-routing decision (per 4.15.15):** keep the egui shell fully opaque `WS_EX_TOPMOST` when a game is being booted/switched, and route the UIA-driven mouse click into RPCS3 via `PostMessage(WM_LBUTTONDOWN/WM_LBUTTONUP)` + `PostMessage(WM_KEYDOWN/WM_KEYUP)` targeted at RPCS3's main HWND with window-relative coords. This is what `zorder_probe --variant c` validates: no Z-order flash, cover stays interactive (so the reconnect QR + player-orbit indicators remain clickable). Implementation: refactor `UiaPortalDriver::boot_game_by_serial` to produce the cell's window-relative coords, then call a new `post_click_to_rpcs3(hwnd, x, y)` helper — matches `zorder_probe::boot_c_postmessage_to_rpcs3` almost verbatim. **Shipped:** (a) refactored `UiaPortalDriver::boot_game_by_serial` to post `WM_LBUTTONDOWN/UP` + `WM_KEYDOWN/UP` at RPCS3's main HWND with window-relative coords — validated by re-running the 3.7.7 live integration test green (`phone_drives_real_rpcs3_load_and_clear` in 39s). (b) `main.rs` viewport builder now sets `.with_transparent(true)` always (panels still paint opaque backgrounds in Main/Crashed/Farewell, so only the in-game path actually lets RPCS3 show through) and `.with_always_on_top()` in release so the launcher stays over RPCS3's fullscreen viewport. (c) New `ui/in_game.rs` surface — dispatched from `ui/mod.rs` whenever `rpcs3_running && screen == Main`. Renders a fully-transparent CentralPanel; the cloud vortex is deliberately skipped so the game shows through. When `connected_clients == 0`, a small gold-bezeled reconnect QR panel ("RECONNECT" Titan One header + scaled QR + "scan to rejoin" dim italic) anchors 32px inside the upper-right per 4.15a.5's "everyone left, anyone come back" refinement. Per-screen VortexParams tuning and the full-shader port still tracked against 4.15a.7.
- [ ] 4.15.9 **Game-switching transition** — phone picks another game → clouds spiral in (cover current game) → "SWITCHING GAMES..." → RPCS3 loads new game → clouds spiral out. Same choreography, different heading copy. **Input-routing:** same as 4.15.8 — during the spiral-in cover, drive the quit/boot sequence via `PostMessage` so the cover animation is never interrupted by a Z-order flash (per 4.15.15). Quit-then-launch flow likely entails `File → Exit` menu nav + relaunch; the File-menu keyboard nav used in `quit_via_file_menu` is synthesised via `SendInput` today, so that half also needs to switch to `PostMessage(WM_KEYDOWN/UP)` against RPCS3's main HWND to avoid landing Alt+arrow keystrokes in the egui cover.
- [x] 4.15.10 **Crash recovery screen** — RPCS3 process exits unexpectedly → clouds spiral in urgently (~1s) → "SOMETHING WENT WRONG" + "RESTART" gold button → respawns RPCS3 + returns to Startup. *Landed: new `LauncherScreen::{Main, Crashed { message }, Farewell}` enum on `LauncherStatus` (default `Main`); the crash watchdog now takes `launcher_status` and flips `screen = Crashed { message }` on the same frame it broadcasts `GameCrashed`, so the phone overlay and the TV screen carry identical copy. `ui.rs` dispatches on `screen` — a new `render_crashed` draws an 80pt "SOMETHING WENT WRONG" heading in Titan One gold, the watchdog's message in italic dim text, and a 40pt gold "RESTART" button (320×96px, gold fill + `GOLD_SHADOW` stroke + `GOLD_INK` label) that flips `screen` back to `Main`. Cloud vortex overlay deferred to 4.15.5. Auto-respawn of RPCS3 is TODO — tracked as an inline comment; today the button just dismisses the screen so the user lands on the QR and can phone-drive a fresh launch.*
- [x] 4.15.11 **Shutdown farewell** — clean quit from phone menu → clouds spiral in gently → "SEE YOU NEXT TIME, PORTAL MASTER" → launcher exits after 3s. *Landed: new signed `POST /api/shutdown` endpoint (alongside `/api/quit` in `http.rs`) flips `launcher_status.screen = Farewell`. `render_farewell` in `ui.rs` stamps a first-frame `Instant`, computes `FAREWELL_COUNTDOWN = 3s - elapsed`, shows a 72pt Titan One gold heading + dim italic "(launcher will exit)" subhead + a 56pt ceiling-rounded seconds counter in `GOLD_2`, and calls `ctx.send_viewport_cmd(ViewportCommand::Close)` when the remaining duration hits zero. `ctx.request_repaint_after(min(0.5s, remaining))` keeps the countdown ticking — egui is lazy by default and without this the remaining-seconds label would freeze. Phone-side menu wire-up deferred — dev curl against `/api/shutdown` is enough to validate. Cloud vortex overlay deferred to 4.15.5.*
- [ ] 4.15.12 **Shader compilation detection (research spike).** RPCS3 compiles shaders on first game launch causing stutter. If detected, cloud vortex stays up until done — first frame user sees is clean gameplay. Investigate: (a) RPCS3 log-file watcher for "Compiling shader" patterns, (b) viewport window title polling for progress strings, (c) FPS-title heuristic (<5fps for >5s). Fallback: fixed 15s delay post-boot. See `navigation.md` §3.6.
- [x] 4.15.14 **Phone-side game-crash overlay.** When RPCS3 crashes, the server broadcasts `Event::GameCrashed`. Phone renders a full-screen overlay (NOT a toast): "GAME CRASHED" heading + "Restarting..." spinner or "RETURN TO GAMES" button. Auto-dismisses on new `GameLaunched` event. Modal priority: below ConnectionLost, above KaosTakeover. See `navigation.md` §3.8. *Landed: `Event::GameCrashed { message }` added to the protocol; server-side crash watchdog (`state::spawn_crash_watchdog`) polls `RpcsProcess::is_alive` every 500ms and broadcasts `GameCrashed` + `GameChanged { current: None }` when a spawned process dies while a game is still marked current; phone `GameCrashScreen` renders a full-screen starfield + amber-alert overlay with a gold "RETURN TO GAMES" button that dismisses the overlay (and issues a best-effort `/api/quit?force` for cleanup). Stacked outside `TakeoverScreen` in `App()` so a crash preempts every other screen state. Auto-restart spinner branch deferred until 4.15.10 ships real auto-restart.*
- [ ] 4.15.13 **Shader progress visualization.** If log parsing yields `current/total` counts, show a gold progress ring (200–240px, conic-gradient fill) at the center of the cloud vortex with the count in Titan One 40px inside. "COMPILING SHADERS" heading + "preparing your adventure" subtitle below. Ring flashes gold on completion, then flow continues to Awaiting Connect. Turns a frustrating wait into a portal-powering-up moment. See `navigation.md` §3.6 for the visual spec. Mock: state 6 of `tv_launcher_v3.html`. Implementation pending egui-side.
- [x] 4.15.15 **Research spike: egui shell as RPCS3 cover + input routing.** The shell needs to stay on top of RPCS3 so the user never sees Qt's transient noise — `#32770` file pickers we already sling off-screen, plus the once-per-session Qt menu popups that clamp to upper-left during `open_dialog` / `quit_via_file_menu` (PLAN 3.6b known residual). Chris noted during 3.7 debugging that Windows also generates display-mode flashes on RPCS3 boot that are jarring on the 86" TV. Keeping egui fullscreen + `WS_EX_TOPMOST` covers all of this *visually*, but breaks our automation: `boot_game_by_serial` currently uses `SendInput` mouse clicks at screen coordinates, which land on whichever window is topmost at that position — egui, not RPCS3. **Summary:** wrote `crates/rpcs3-control/examples/zorder_probe.rs` comparing all three candidates against real RPCS3 behind a Win32-made fullscreen topmost cover on the HTPC. All three variants successfully booted `BLUS31076`; the differentiator is side-effects:

  | Variant | Cover config | Boot mechanism | Boot time | Side-effects |
  |---|---|---|---|---|
  | (a) | `WS_EX_LAYERED + WS_EX_TRANSPARENT` (click-through) | existing `SendInput` at screen coords | 1.22s | Cover can't receive *any* mouse — kills 4.15.7's clickable player-orbit indicators. |
  | (b) | Opaque `WS_EX_TOPMOST` | `SetWindowPos(HWND_BOTTOM)` → `SendInput` → `SetWindowPos(HWND_TOPMOST)` | 1.34s | Brief visible flash (~100–300ms) as the cover drops and restores. Also creates a coupling: any mid-click error must still restore Z-order or the cover gets stuck underneath. |
  | (c) | Opaque `WS_EX_TOPMOST` | `PostMessage(WM_LBUTTONDOWN/UP)` + `PostMessage(WM_KEYDOWN/UP)` targeted at RPCS3's main HWND with window-relative coords | 0.95s | None — cover stays fully topmost + interactive, no compositor round-trip, fastest. |

  **Chosen: (c).** The probe's PostMessage approach is ~15 extra lines over the existing `SendInput` helpers; recommendations written into 4.15.8 and 4.15.9. Open residual question for later: does the RPCS3 viewport window fight an egui `WS_EX_TOPMOST` overlay once a game is running (fullscreen vs windowed emulator mode)? Not probed yet — defer until 4.15.8 implementation surfaces a concrete case.

### 4.16 E2E test updates

- [ ] 4.16.1 Audit `crates/e2e-tests/` selectors after the reskin. Where possible move from class-name matches to stable `data-test` attributes so the next redesign doesn't break the harness.
- [ ] 4.16.2 Visual regression: out of scope — manual review only.
- [ ] 4.16.3 Manual multi-phone visual sanity on the HTPC (both phones running, picking flows, resume modal, takeover screen).

### 4.17 Review checkpoint

- [ ] 4.17.1 Demo on HTPC end-to-end: launcher TV readability → phone scans → profile picker → PIN → game picker → portal in all five slot states → takeover flow.
- [ ] 4.17.2 Catalogue any UX papercuts and route them to Phase 3 stragglers (ownership badge, show-join-code wiring, 3-option resume modal, crystal extra-confirm, attribution placement).

---

## Phase 5 — Kaos

- [ ] 5.1 Wall-clock timer: 20min warmup + randomized 60min windows.
- [ ] 5.2 Text-only overlay with Kaos catchphrases (curated in-repo list; text avoids audio copyright). Two distinct overlay surfaces mocked in Phase 4: (a) `kaos_takeover.html` — the stolen-seat screen with KICK BACK button; (b) `kaos_swap.html` — mid-game swap announcement with outgoing→⚡→incoming figure row + Kaos taunt. **No auto-dismiss**: phone is typically asleep during gameplay, so the overlay must stay visible until the user explicitly taps BACK TO THE BATTLE. If Kaos fires multiple times while the phone is asleep, latest-fires-wins (or queue — decide during implementation).
- [ ] 5.3 1-for-1 swap of a portal figure with a random compatible-with-current-game figure.
- [ ] 5.4 Purple/pink skin theme applied via CSS variables (rides on the `--*` tokens established in Phase 4 — should be a palette swap, not a rewrite).
- [ ] 5.5 Parent kill-switch (SPEC Q38) — hidden config knob, not in the phone UI.
- [ ] 5.6 Integration: Kaos swap must go through the standard driver flow (so tests catch regressions).

Kaos is LAST among feature work. Do not start without explicit go-ahead.

---

## Phase 6 — Post-Kaos polish (future enhancements)

- [x] 6.2 **Parse `.sky` firmware for per-figure stats** — mostly done, read-only. `crates/sky-parser/` parses the plaintext tag layout per `docs/research/sky-format/SkylanderFormat.md` (mirrored from NefariousTechSupport/Runes). RPCS3 writes plaintext `.sky` files with no AES, so no decryption needed. `GET /api/profiles/:profile_id/figures/:figure_id/stats` exposes the parse, feature-gated on `sky-stats`. 22 tests (header, variant decomposition, web code, XP/level, gold, nickname, hero points, playtime, hat history + current resolution, trinket, timestamps, quest raw u72s, CRC16 validation, area-sequence wraparound). **Still stubbed**: Trap / Vehicle / Racing Pack / CYOS (Imaginators creation-crystal) layouts — surfaced as `FigureKind::Other` with TODO pointers. Phone UI wiring (figure-card info panel) still pending — REST endpoint is ready to consume.

- [ ] 6.3 **Detailed-stats screen on the phone** — "tap a figure card → a full-screen themed detail view" showing what 6.2's parser extracted: level + XP progress bar, gold, current hat, playtime, nickname, hero points, hat history, trinket, and quest progress when decoded. Land *after* Phase 4 so the layout inherits the Skylanders starfield/gold-bezel theme instead of being re-themed twice. Hits `/api/profiles/:profile_id/figures/:figure_id/stats`. Read-only (no editing) — writing is out of scope per 6.2. Non-standard layouts (Trap/Vehicle/CYOS) render a reduced panel until 6.2's stub fills are done.

- [ ] 6.4 **Demo harness for screen recording.** A browser-viewable test session that shows the phone SPA running a representative flow (profile pick → PIN → game launch → portal → toy box → figure place → Kaos swap). Intended to be placed side-by-side with a remote-desktop view of the real HTPC running RPCS3, so the whole integrated experience can be screen-recorded as one demo. Likely: a special `/demo` route or `?demo=1` query flag that auto-drives the phone through key interactions with timed pauses, OR a pre-scripted Playwright/WebDriver recording. The mock harness lets Chris hit record once and capture phone + TV in one frame.

- [ ] 6.1 **Suppress RPCS3 window flicker during menu navigation.** The 3.6b research landed on "accept a once-per-session flicker" because Qt renders menu popups at visible screen coords when the parent is off-screen, and the Skylanders Manager dialog appears in the screen centre for a brief moment before we sling it off-screen. Our eframe launcher window launches *before* RPCS3, so it's in a position to establish Z-order priority. Ideas to explore: (a) make our launcher `WS_EX_TOPMOST` during any `open_dialog` navigation so Qt popups render behind it; (b) use `SetWinEventHook` / `EVENT_OBJECT_SHOW` filtered to RPCS3's PID to intercept the dialog creation event and move it off-screen before the first paint (Tier 2 in the 3.6b write-up); (c) hook menu popups the same way (Tier 3). Prerequisite: the real app exists and the launcher-first ordering is stable.

---

## Phase 7 — Packaging + release

Deliberately separated from Phase 3 so it's clear this only runs once the app works end-to-end. CI deliberately deferred until here per the original "no CI until features work" stance.

- [ ] 7.1 **Single-exe distribution.** Everything ships as ONE `skylander-portal.exe` — no zip, no folder structure, no "extract here" instructions. The phone SPA (`phone/dist/`), figure thumbnails (`data/images/*/thumb.png`), `figures.json`, fonts (WOFF2), and the Kaos SVG are all embedded into the binary at compile time via `include_dir!` or `rust-embed`. The server serves them from memory; the user just double-clicks the exe. Evaluate `rust-embed` (simpler, less config) vs `include_dir!` (already used pattern in Rust ecosystem). Release builds strip debug symbols + `cargo build --release` + UPX if the binary exceeds ~50MB.
- [ ] 7.2 GitHub Actions workflow: on version-tag push, build Windows release, run the fast test suite (unit + integration + workspace build — NOT the `#[ignore]`-gated e2e tests), attach zip to the release.
- [ ] 7.3 Release `README.md` spells out user-supplied bits: RPCS3 install path, firmware backup pack. Walk through the first-launch wizard experience. Link to `data/LICENSE.md` for the Fandom attribution (3.19.6).
- [ ] 7.4 Verify the release zip on a *different* Windows machine than the dev one — catches "this path exists only on my laptop" bugs.
- [ ] 7.5 Post-release monitoring plan: how do we hear about breaks? GitHub issues only for v1; formal telemetry out of scope.
- [ ] 7.6 **Trademark / IP review of shipped assets.** The Kaos sigil (`docs/aesthetic/kaos_icon.svg` → bundled as `phone/assets/kaos.svg`) and potentially box-art thumbnails (4.2.5) are Activision/Beenox IP. Decide before public release: ship literal symbol as fair-use UI accent, ship a derivative/abstracted version, or commission/draw a Kaos-adjacent custom sigil. Firmware `.sky` files and RPCS3 are already documented as user-supplied; this is the runtime-asset angle on the same question.

---

- No bundling of RPCS3 or `.sky` files (piracy concern).
- No CI until core features work.
- No Linux/Mac support.
- No user-entered figure names.
- No audio (text-only Kaos to dodge copyright).
- No live wiki scraping at runtime — data is committed to the repo.

## Risks (live list — update as we learn)

- **R1:** UI Automation may not expose enough of the RPCS3 Qt dialog to drive it reliably. Mitigation: phase 1a is the first thing we do; we stop-the-world if it's unworkable.
- **R2:** "Move portal dialog off-screen" may be blocked by Windows or cause focus loss from the game. Mitigation: acceptable fallback is minimizing the main RPCS3 window and relying on controller focus.
- **R3:** Wiki search hit rate might be below 80%. Mitigation: manual curation file checked in, layered over scraped data.
- **R4:** Leptos touch/mobile UX may prove rough. Mitigation: JS fallback is pre-authorized if needed.
- **R5:** RPCS3 version drift will silently break GUI automation. Mitigation: version check on startup, warn on mismatch; pin a known-good dev version.
