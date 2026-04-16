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
- [ ] 3.7.2 `lifecycle_launch_load_clear_quit` — update to launch without EBOOT arg (library view), UIA-boot the game by serial via the recipe proven in `examples/boot_game.rs` (`SelectionItemPattern.select()` + `set_focus()` + Enter), then run the full load→clear flow. Re-run from a direct-desktop session.
- [ ] 3.7.3 `offscreen_hide_really_hides` — re-run from direct-desktop session.
- [ ] 3.7.4 `file_dialog_hidden_while_manager_hidden` — re-run from direct-desktop session.
- [ ] 3.7.5 Replace the existing EBOOT-based launch contract in tests with launch-then-UIA-boot. Shutdown via `File → Exit` menu nav (mirror the Manage menu approach) to get clean exits and let RPCS3 release its lockfile normally.

**Review checkpoint:** 3.1 – 3.6 are green; 3.6b and 3.7 run on the HTPC at Chris's pace and don't block the rest of Phase 3 fanning out.

---

### 3.8 Name reconciliation (carryover from Phase 2)

- [ ] 3.8.1 Driver worker knows the `figure_id` it asked to load; it overrides RPCS3's `display_name` with `figures[figure_id].canonical_name` before broadcasting `SlotChanged` so unknowns don't show as `"Unknown (Id:N Var:M)"`.
- [ ] 3.8.2 On `RefreshPortal` (no figure_id context), attempt a name-to-id reverse match against the indexed figures; fall back to the raw display name with a visual "?" badge if unmatched.

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
| 3.10f | 3.10e.1–5 — multi-phone e2e scenarios exercising registry + WS behaviour | next |
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

- [ ] 4.1.1 Palette: starfield blues (`--sf-1/2/3`), gold bezel stops (`--gold-bright/gold/gold-mid/gold-shadow/gold-inner`), per-element gradients (`--el-*`), status colors (loading/error/success). Kaos (5.4) variants defined as sibling vars upfront.
- [ ] 4.1.2 Typography: **Titan One** (display), **Fraunces** (body). Self-host under `phone/assets/fonts/`. Both are OFL-licensed; note in `data/LICENSE.md`.
- [ ] 4.1.3 Easing + timing tokens — `--ease-spring`, `--ease-sweep`, `--dur-tap`, `--dur-loading-sweep` (0.9s), `--dur-impact` (600ms), `--dur-shudder` (400ms), `--dur-halo-slow` (3.4s), `--dur-idle-float` (4.5s).
- [ ] 4.1.4 CSS vars restructured so a body-class swap (`.skin-kaos`) repalettes the whole app without touching component CSS.
- [ ] 4.1.5 `prefers-reduced-motion` kill-switch disables ambient drift + halo rotations app-wide.

### 4.2 Static mockups

Direction locked to Option A. Standalone HTML/CSS in `docs/aesthetic/mocks/`, viewable by opening directly. One screen per file.

- [x] 4.2.1 Portal view with all slot states — `option_a_heraldic.html`.
- [x] 4.2.2 Slot state-transition demo — `transitions.html`.
- [ ] 4.2.3 Profile picker — "WELCOME, PORTAL MASTER" heading, gold-bezeled profile swatches, "+ Add" card.
- [ ] 4.2.4 PIN keypad — framed panel, gold-bezel dot display, oversized keys, lockout state.
- [x] 4.2.5 Game picker — names only, big and simple. No serial numbers or figure counts (those are dev/filter concerns, not pick-a-game concerns). Stagger-rise animation. Box art as faded background behind each card title (production: `/api/games/{serial}/boxart` endpoint, bundled images; mock uses gradient placeholders evoking each game's palette). "Launching" state dims other cards, pulses the selected title.
- [x] 4.2.6 Browser / collection view — **resolved: toy box lid** overlay with collapsible search/filter lid + figure grid. Mock: `portal_with_box.html` (checkbox toggles open/closed; click SEARCH to expand lid, scroll to collapse).
- [x] 4.2.7 Picking-mode — integrated into box-open state. "PICKING FOR SLOT N" label at top of collection. No separate banner mock needed.
- [ ] 4.2.8 Profile creation flow — name entry → color pick → PIN set → PIN confirm → done. Reuses the PIN keypad layout with a "choose your pin" / "confirm your pin" label swap. Name entry needs a themed text input (or preset name picker — decide in mock).
- [x] 4.2.8b Figure detail view — mock: `docs/aesthetic/mocks/figure_detail.html`. Default / loading / errored states (demo controls in the corner toggle between them).
- [ ] 4.2.9 Takeover / Kaos screen (blue palette; purple Kaos skin deferred to 5.4).
- [ ] 4.2.10 Resume-last-setup modal, reset-to-fresh confirm, show-join-code sheet — all use `<FramedPanel>` treatment.
- [ ] 4.2.11 Screen-transition animation demo — **after 4.3**. Animates the full navigation sequence: profile picker → PIN → game picker → portal. Locks the feel for screen-level motion.
- [ ] 4.2.12 Review round with user. Iterate before touching Leptos.

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
- [ ] 4.3.6 **Collection default sort — game-compatible, then last-used.** Because filters are opt-in (hidden behind SEARCH), the initial grid order must surface the most likely picks at the top. Primary key: figures compatible with the currently-launched game (per the Kaos-compatibility heuristic in CLAUDE.md — game-of-origin + all later games, with known exceptions like vehicles=SuperChargers-only). Secondary key: `figure_usage.last_used_at` descending (already persisted per-profile in SQLite). Server ranks; phone renders in order. Tie-break: canonical name alphabetical.
- [x] 4.3.7 **Figure detail view flow.** Tapping a figure in the box opens the **detail view**, not a direct placement. Transition: other figures + wooden box interior fade to ~25% opacity, selected figure "lifts up" (scale + translate animation) into a blue framed panel that fades in. The lifted figure gets a soft aura + slow-rotating rays (like the in-game figure reveal). The panel hosts: figure name + element/game metadata, an action-icon row (appearance / stats / reset — icons are placeholders in Phase 4; 3.14 wires up variant cycling, 6.3 wires up stats drill-down, 3.11.3 wires up reset-to-fresh), a compact stats preview strip, `PLACE ON PORTAL` (primary) + `BACK TO BOX` (secondary) buttons. **Back** reverses the entrance animation (panel fades, figure descends into grid, other figures restore). **Place** triggers the portal loading ring on the hero figure; on success the toy box lid closes and the portal-impact animation plays on the server-assigned slot; on failure an inline error banner slides in from the top of the panel (no toast for this — the detail view stays put so the user can retry or go back). Server still decides the slot; the phone never computes it.
- [ ] 4.3.4 **Modal stack semantics.** PIN entry, admin, resume prompt, reset confirm, takeover, show-join-code. Which stack, which block the portal, which auto-dismiss, which survive reconnect.
- [ ] 4.3.5 **Navigation map** documented in `docs/aesthetic/navigation.md` — state graph so future screens (stats 6.3, reposes 3.14) have a place to land.

### 4.4 Shared Leptos components

- [ ] 4.4.1 `<GoldBezel>` — circular gold frame with element-tinted inner plate. Props: `size`, `element`, `state` (default / picking / loading / loaded / errored / disabled), child content. **Primary child is an `<img>` thumbnail** (`/api/figures/{id}/image?size=thumb`) that fills the circle; the element-gradient plate shows through as a fallback for figures without wiki images (16 of 504). Profile swatches use an initial letter instead. Reused by portal slots, browser cards, profile swatches, empty-slot "+", color-picker swatches, and the hero bezel in the figure detail view.
- [ ] 4.4.2 `<FramedPanel>` — parchment-blue panel with a multi-stop gold gradient border for modal surfaces (PIN, admin, resume, confirms, takeover, show-join, figure detail). **No corner brackets** — the gradient border does the framing; brackets were visual noise. Consistent across all panels.
- [ ] 4.4.3 `<DisplayHeading>` — two-tone outlined title: gold fill (linear-gradient via `background-clip: text`) + dark-gold `-webkit-text-stroke` + drop-shadow. Matches the "DARES" / "IMAGINITE" treatment.
- [ ] 4.4.4 `<RayHalo>` — rotating conic-gradient halo for selected / loading state, masked to a ring.
- [ ] 4.4.5 `<FigureHero>` — the lifted figure presentation used in the detail view: oversized `<GoldBezel>` + soft aura + slow-rotating conic-gradient rays. Takes a `state` prop (`default`, `loading`, `errored`) that switches the bezel treatment + triggers the loading ring overlay. Reused by the Kaos swap-announcement overlay (5.3).

### 4.5 Starfield background + ambient motion

- [ ] 4.5.1 Layered starfield: multiple radial gradients + tiled SVG star-dot layer + slow parallax drift (~40s loop). Single shared background on `body`, so screen changes feel continuous.
- [ ] 4.5.2 Optional "magic dust" — sparse floating-particle layer; evaluate CPU cost on the HTPC before committing.

### 4.6 Portal view reskin + state transitions

- [ ] 4.6.1 Slots render through `<GoldBezel>`. Empty slot = dimmed bezel with a "+" in the center.
- [ ] 4.6.2 Empty → Picking: scale 1.05 spring-ease, outer gold glow intensifies, `<RayHalo>` fades in and begins a slow rotation.
- [ ] 4.6.3 Pick → Loading: halo rotation speeds up, a gold sweep travels once around the bezel ring (~1s loop), inner plate dims 20%.
- [ ] 4.6.4 Loading → Loaded: "portal impact" — radial white→gold flash scales from 0 → 2× and fades (~400ms), bezel brightness spikes briefly, then settles into a subtle 4s idle float (±2px).
- [ ] 4.6.5 Loaded → Cleared: desaturate + shrink, fade thumb out to element plate (~200ms).
- [ ] 4.6.6 Errored: red-tinted bezel, short shake animation + persistent subdued red glow until dismissed.
- [ ] 4.6.7 Slot tap feedback: inner-plate "dent" (inset shadow + 0.96 scale), spring back.

### 4.6b Figure detail view (the "lifted" hero panel)

Implemented as a new screen reached by tapping a figure inside the toy box. Shell only in Phase 4; the action-icon wiring lands with later work (3.14 variants → appearance icon; 6.3 stats → stats icon; 3.11.3 reset → reset icon).

- [ ] 4.6b.1 `<FigureDetail>` Leptos component. Props: `figure: PublicFigure`, `placed_by: Option<String>` (for stats strip context), `on_back: Callback`, `on_place: Callback`. Internal state: `idle | loading | errored { message: String }`.
- [ ] 4.6b.2 Entrance animation: other figures + box interior crossfade to ~25% opacity; selected `<FigureHero>` lifts from its grid position via FLIP-style transform; framed panel fades in behind (staggered ~120ms).
- [ ] 4.6b.3 Action-icon row: three `<BezelButton>`s with placeholder icons + disabled state + "coming soon" tooltip until 3.14/6.3/3.11.3 wire real handlers. Keeps Phase 4 contained while locking the layout.
- [ ] 4.6b.4 Stats preview strip — placeholder values in Phase 4 (shows the layout but reads from a stub). 6.3 pulls real numbers from `/api/profiles/:profile_id/figures/:figure_id/stats`.
- [ ] 4.6b.5 `PLACE ON PORTAL` (primary) calls `on_place`; while awaiting the WS confirmation, state flips to `loading` and the hero bezel's loading ring spins. On WS `SlotChanged` with matching figure_id → box lid closes, portal-impact animation fires on the target slot. On error → state flips to `errored`, inline error banner slides in from top of panel. **Client-side timeout (8s default, configurable)**: if no `SlotChanged` or error arrives, auto-flip to `errored` with a "took too long — try again?" message. Prevents the user from sitting on a permanent loading spinner if the WS drops mid-operation.
- [ ] 4.6b.6 `BACK TO BOX` (secondary) reverses the entrance animation and returns to the collection grid with scroll position preserved. **Must remain enabled in the loading and errored states** — the user should never be trapped waiting on the server. Backing out mid-load doesn't cancel the load (the figure may still appear on the portal once it completes); it just returns the user to the box. If the WS eventually reports success while the user is elsewhere, no UI disruption — the slot just populates.
- [ ] 4.6b.7 Server contract unchanged: phone sends figure_id; server picks the slot (first-available or picking-for-specific-slot context). Phone never names a slot in the request.

### 4.7 Browser view reskin

- [ ] 4.7.1 Figure cards use a smaller `<GoldBezel>` as the portrait; card itself becomes a minimal frame under the bezel.
- [ ] 4.7.2 Element chips redesigned as gold-bordered pills with element-tinted fills.
- [ ] 4.7.3 `on-portal` state: desaturated bezel + a "ON PORTAL" gold ribbon across the corner.
- [ ] 4.7.4 Search input: gold shimmer sweep along the border on focus (one-shot).
- [ ] 4.7.5 Empty/filtered-out state: themed empty-state illustration + copy, not the plain text we have today.

### 4.8 Profile picker reskin

- [ ] 4.8.1 Big `<DisplayHeading>` "WELCOME, PORTAL MASTER".
- [ ] 4.8.2 Profile cards: oversized gold-bezeled swatches with the initial rendered in the display font. Each profile's color tints the inner plate.
- [ ] 4.8.3 "Add profile" affordance → prominent "+" bezel card instead of the placeholder button.
- [ ] 4.8.4 Entry animation: cards bloom in from center, 80ms stagger.

### 4.9 PIN keypad reskin

- [ ] 4.9.1 `<FramedPanel>` surround. PIN dots become mini gold bezels that fill with element-tinted plates as digits are entered.
- [ ] 4.9.2 Key press feedback: inset-shadow dent + soft "click" animation (≤100ms), plus subtle haptic-adjacent bounce.
- [ ] 4.9.3 Unlock success: shockwave ring outward from profile swatch + gold streak L→R sweep as the panel fades out.
- [ ] 4.9.4 Lockout state: panel tinted red, countdown in the display font, keys visually disabled (not hidden).

### 4.10 Profile admin reskin

- [ ] 4.10.1 `<FramedPanel>` surround. Form inputs themed (rounded, gold focus outline, display-font labels).
- [ ] 4.10.2 Color picker swatches are mini gold bezels.
- [ ] 4.10.3 Destructive actions (delete, reset-PIN) clearly marked with a red-tinted framing.

### 4.11 Game picker reskin

- [ ] 4.11.1 Game cards with room for per-game artwork (placeholder text if we don't have the assets yet).
- [ ] 4.11.2 Card entry animation: stagger-rise from below, 80ms per card.
- [ ] 4.11.3 Selected-card confirmation flash before the WS signal flips the UI to portal view.

### 4.12 Modals + takeover screen

- [ ] 4.12.1 Resume-last-setup modal (3.12.2 UI): `<FramedPanel>` with a figure-preview row of gold bezels, "Resume" + "Start fresh" CTAs.
- [ ] 4.12.2 Reset-to-fresh confirm (3.11.3 UI).
- [ ] 4.12.3 Takeover/Kaos screen polish — stays blue (the Kaos skin itself ships with 5.4; this is just restyled).
- [ ] 4.12.4 Show-join-code sheet scaffolded — `<FramedPanel>` with a gold-bezeled QR and join URL. The real content wiring to 3.10.8 happens in the post-Phase-4 follow-up; this milestone just delivers the modal shell and a header entry point that navigates to it.
- [ ] 4.12.4b **Menu overlay** (opened by header kebab). Single surface that consolidates: (a) show-join-code QR (prominent, always visible — answers "how does my friend join?"), (b) current profile chip for context, (c) three stacked actions with icon + title + one-line description:
      - **SWITCH PROFILE** — locks current session, returns to profile picker. Other sessions on other phones keep their profiles.
      - **CHOOSE ANOTHER GAME** — quits the current RPCS3 game, returns to game picker; profile stays unlocked. Confirm required (destructive for the other player if they're mid-play).
      - **SHUT DOWN** — red/danger treatment — closes the whole server + RPCS3. Confirm required; labelled "ask a grown-up first" so kids know it's the adult exit.
      Dimmed scrim behind, tap scrim or ✕ to close. Uses `<FramedPanel>` shell. Mock: `docs/aesthetic/mocks/menu_overlay.html`. Absorbs 3.10.8 (show-join-code) entirely — that line item's UI work lands here; server-side it's already done.

### 4.13 Toasts redesign

- [ ] 4.13.1 Color-coded left strip (error / warn / success / info). Consistent typography with the rest of the app.
- [ ] 4.13.2 Slide-in from top-right variant for non-blocking notifications; existing bottom-center kept for critical errors.

### 4.14 Ambient polish

- [ ] 4.14.1 Screen-to-screen transitions: consistent cross-fade + slight motion direction based on navigation depth (deeper = slide up, back = slide down).
- [ ] 4.14.2 Connection-status pip in the header gets a breathe animation while connecting, steady glow when connected, soft red when disconnected.

### 4.15 egui launcher reskin

- [ ] 4.15.1 Shared palette via egui `Visuals` — starfield-blue background, gold accents. Readable from ~10 ft on an 86" TV (≥32pt body, ≥64pt QR label).
- [ ] 4.15.2 Display font loaded into egui so the PC-side and phone-side feel unified.
- [ ] 4.15.3 QR code framed in a gold bezel equivalent (static, no conic gradients — egui doesn't need the web flourish).
- [ ] 4.15.4 Status indicators — RPCS3 connection dot (absorbs the old 2.8.4 deferral), client count, figure count, current-game name.
- [ ] 4.15.5 **Cloud vortex loading animation.** While RPCS3 boots a game, the egui window shows a swirling blue-white cloud vortex inspired by the in-game "PLEASE PLACE YOUR SKYLANDER ON THE PORTAL OF POWER" screen (reference: `docs/aesthetic/loading_screen.png`). Clouds rotate inward toward a bright center. Once the game is loaded (RPCS3 window detected), the clouds part/fade outward to reveal the portal QR / status underneath. Renders via egui `Painter` custom shapes or a pre-rendered animated texture if custom painting is too expensive for smooth rotation.

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

- [ ] 6.1 **Suppress RPCS3 window flicker during menu navigation.** The 3.6b research landed on "accept a once-per-session flicker" because Qt renders menu popups at visible screen coords when the parent is off-screen, and the Skylanders Manager dialog appears in the screen centre for a brief moment before we sling it off-screen. Our eframe launcher window launches *before* RPCS3, so it's in a position to establish Z-order priority. Ideas to explore: (a) make our launcher `WS_EX_TOPMOST` during any `open_dialog` navigation so Qt popups render behind it; (b) use `SetWinEventHook` / `EVENT_OBJECT_SHOW` filtered to RPCS3's PID to intercept the dialog creation event and move it off-screen before the first paint (Tier 2 in the 3.6b write-up); (c) hook menu popups the same way (Tier 3). Prerequisite: the real app exists and the launcher-first ordering is stable.

---

## Phase 7 — Packaging + release

Deliberately separated from Phase 3 so it's clear this only runs once the app works end-to-end. CI deliberately deferred until here per the original "no CI until features work" stance.

- [ ] 7.1 Bundle `server.exe` + `phone/dist/` + `data/images/*/thumb.png` + `data/figures.json` + `README.md` into a GitHub Releases zip. Evaluate `cargo dist` vs a hand-rolled PowerShell/zip script; prefer `cargo dist` if it handles the non-Rust assets cleanly.
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
