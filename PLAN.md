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

Deliberately deferred to Phase 3: PIN-gated profiles, multi-profile, working-copies, session resume, game launching, wiki scrape, aesthetic pass, Chaos, takeover, security signing. We want the smallest possible end-to-end slice first.

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

### 3.5 E2E harness scaffolding (`crates/e2e-tests/`) — DONE (pending chromedriver install)

- [x] 3.5.1 Workspace crate `crates/e2e-tests/`. Runs via `cargo test -p skylander-e2e-tests`.
- [x] 3.5.2 `Phone::new` connects fantoccini to `http://localhost:4444`. One-time chromedriver install steps documented in `crates/e2e-tests/README.md`.
- [x] 3.5.3 `TestServer::spawn()` writes a temp `.env.dev`, runs `cargo run -p skylander-server --features test-hooks`, multiplexes stdout + stderr into a channel, scrapes `serving on http://…`.
- [x] 3.5.4 `Phone` helpers: `wait_for_portal`, `tap_slot`, `tap_figure_named`, `slot_text`, `search`, `toast_count`, `last_toast_text`, `wait_until`. `launch_giants` + `inject_load_outcomes` + `set_game` REST helpers exported at crate root.
- [x] 3.5.5 `ChildGuard::Drop` kills the cargo-run child; fantoccini clients torn down via `Phone::close` (or their own Drop).

### 3.6 Phase 2 regression scenarios as named tests — DRAFTED (needs chromedriver to run)

- [x] 3.6.1 `spam_click_same_slot` drafted — five rapid clicks, expect ≤1 toast, slot eventually Loaded.
- [x] 3.6.2 `dup_figure_across_slots` drafted — first load injected OK, second injected FileInUse, slot 2 stays Empty.
- [x] 3.6.3 `clear_then_load_sequence` drafted — load → Remove → load a different card → slot shows new figure.
- [x] 3.6.4 `error_toast_never_populates_slot` drafted — parameterised over FileInUse and QtModal variants.
- [x] 3.6.5 `ws_reconnect` drafted — currently implemented via a page reload since fantoccini can't easily reach into a JS `WebSocket` handle; a lower-level WS-drop approach is a future refinement.
- [x] 3.6.6 `on_portal_figures_disabled` drafted — loads, asserts `.card.on-portal` class + "Already" toast on tap.
- **To verify:** install chromedriver (+ `cd phone && trunk build`) and run `cargo test -p skylander-e2e-tests -- --ignored --nocapture`. Drafts likely need tweaks once we see them run.

### 3.7 Optional: real-RPCS3 e2e (heavier, manual trigger)

- [ ] 3.7.1 `tests/e2e/live/` — tests that `RpcsProcess::launch` + drive actual RPCS3. Gated behind `--ignored` so they're never part of the default `cargo test`.
- [ ] 3.7.2 `lifecycle_launch_load_clear_quit` — launches SSA (or whichever the user marks), loads Eruptor, clears, quits.
- [ ] 3.7.3 `offscreen_hide_really_hides` — confirmed in 3.2.4 on Skylanders Manager; re-verify after code changes.
- [ ] 3.7.4 `file_dialog_hidden_while_manager_hidden` — drive a load while the manager is off-screen; assert the Windows common file dialog also doesn't appear on-screen (we'd expect it modally parents to the manager and inherits its coords, but the Phase 2 probe showed it hovering at `@33,41` which is clearly on-screen, so we need explicit coverage). If it does appear, add a "hide-child-dialogs" helper to the driver.

**Review checkpoint:** once 3.1 – 3.6 are green, feature work below is safe to fan out in parallel.

---

### 3.8 Name reconciliation (carryover from Phase 2)

- [ ] 3.8.1 Driver worker knows the `figure_id` it asked to load; it overrides RPCS3's `display_name` with `figures[figure_id].canonical_name` before broadcasting `SlotChanged` so unknowns don't show as `"Unknown (Id:N Var:M)"`.
- [ ] 3.8.2 On `RefreshPortal` (no figure_id context), attempt a name-to-id reverse match against the indexed figures; fall back to the raw display name with a visual "?" badge if unmatched.

### 3.9 Profile system + PINs (covers SPEC.md Q20-Q24)

- [ ] 3.9.1 `crates/server`: add SQLite via `sqlx`. Schema: `profiles(id, display_name, pin_hash, created_at)`, `sessions(profile_id, last_portal_layout_json, updated_at)`, `figure_usage(profile_id, figure_id, last_used_at)`.
- [ ] 3.9.2 Profile-picker screen — "chest" visual placeholder until aesthetic pass.
- [ ] 3.9.3 PIN keypad: 4-digit, large touch targets. Three-strikes backoff (5s lockout) to discourage a sibling from guessing.
- [ ] 3.9.4 Profile admin UI: create/delete profile, reset PIN (no admin PIN per SPEC Q21 — just trust the keyboard at the PC for admin ops for now).
- [ ] 3.9.5 Guest mode deferred to 3.15 or later.

### 3.10 Working copies + reset-to-fresh

- [ ] 3.10.1 Working copy location: `%APPDATA%/skylander-portal-controller/working/<profile_id>/<figure_id>.sky`.
- [ ] 3.10.2 On first pick of a figure by a profile: fork from the pack's fresh `.sky` into the working path; load that instead of the pack file.
- [ ] 3.10.3 Reset-to-fresh action on a loaded slot: confirm → clear → copy fresh over working → reload.
- [ ] 3.10.4 Creation Crystals (Imaginators) per-profile; reset blocked behind an extra confirm per SPEC Q61.

### 3.11 Session resume + layout memory

- [ ] 3.11.1 Persist the last portal layout to `sessions` on every successful load/clear.
- [ ] 3.11.2 On profile unlock, prompt "resume last setup?" — confirm runs the load sequence automatically (respecting back-pressure).
- [ ] 3.11.3 Skip if the profile has no prior layout.

### 3.12 Takeover + kick-back

- [ ] 3.12.1 Server tracks the currently-authorized WS client (by connection id, not profile).
- [ ] 3.12.2 New WS connection claims control; prior client receives a `TakenOver { by: chaos_flavor }` event.
- [ ] 3.12.3 1-minute cooldown per SPEC Q31. "Kick back" button on the evicted phone re-initiates the handshake.
- [ ] 3.12.4 Takeover locks the profile (clears the unlock state) but preserves the game/portal.

### 3.13 HMAC command signing

- [ ] 3.13.1 Server generates a random key at startup; encodes it into the QR payload alongside the URL.
- [ ] 3.13.2 Phone includes `X-Skyportal-Sig: hmac-sha256(payload + timestamp)` on every mutating request.
- [ ] 3.13.3 Server rejects unsigned or stale (>30s) signatures with 401.
- [ ] 3.13.4 `on_portal` READ endpoints are unauthenticated (defence-in-depth is overkill on a trusted LAN; the gate is the mutating path).

### 3.14 Reposes: collapse + variant cycling

- [ ] 3.14.1 Browser collapses figures sharing a `variant_group` into a single card showing the base figure + "N variants" badge.
- [ ] 3.14.2 Tap the variant badge → cycle between variants in place (per SPEC Q76).
- [ ] 3.14.3 Loaded variant reflected on the slot's display_name.

### 3.15 Aesthetic pass (Skylanders-style CSS)

- [ ] 3.15.1 Implement the visual direction from `docs/aesthetic/ui_style_example.png`: circular gold-bezeled figure portraits, starfield blue background, bold white titles with gold outline.
- [ ] 3.15.2 Element gradient palette tuned against in-game references.
- [ ] 3.15.3 Card-state transitions (Pick → Loading → Loaded) get subtle animation.
- [ ] 3.15.4 Use the `frontend-design` skill for the heavier visual work.

### 3.16 First-launch config wizard (egui)

- [ ] 3.16.1 On first launch (no `config.json` in APPDATA), egui shows a wizard: RPCS3 path picker, firmware pack picker, Done.
- [ ] 3.16.2 Validate each path (rpcs3.exe exists; pack contains `.sky` files).
- [ ] 3.16.3 Writes `%APPDATA%/skylander-portal-controller/config.json` and reloads.

### 3.17 Reconnect overlay + network fallback

- [ ] 3.17.1 When the authorised phone disconnects and doesn't return within ~10s, render a small always-on-top overlay window (separate eframe viewport) in the lower-right showing a reconnect QR.
- [ ] 3.17.2 "Can't connect" button on the launcher opens a network-interface picker (Q49 fallback).

### 3.18 Packaging + GitHub Releases

- [ ] 3.18.1 `cargo dist` or hand-rolled zip script: bundle `server.exe` + `phone/dist/` + `README.md`.
- [ ] 3.18.2 GitHub Action: on tag push, build Windows zip, attach to release.
- [ ] 3.18.3 Release README spells out the required user-supplied bits (RPCS3 install + firmware backups).

### 3.19 Wiki scrape (second pass)

- [ ] 3.19.1 `tools/wiki-scrape/` — Rust one-shot per Phase 1d plan. Reads `docs/research/firmware-inventory.json`, hits Fandom API, emits `data/figures.json` + `data/images/<figure_id>/{thumb,hero}.png`.
- [ ] 3.19.2 Commit results to repo. Manual curation file `data/figures.manual.json` overlays the scrape.
- [ ] 3.19.3 Server serves images at `/api/figures/<id>/image?size={thumb,hero}` (with fallback to element icon if absent).
- [ ] 3.19.4 Phone card icon becomes the figure's hero image when available.

---

## Phase 4 — Chaos

- [ ] 4.1 Wall-clock timer: 20min warmup + randomized 60min windows.
- [ ] 4.2 Text-only overlay with Kaos catchphrases (curated in-repo list; text avoids audio copyright).
- [ ] 4.3 1-for-1 swap of a portal figure with a random compatible-with-current-game figure.
- [ ] 4.4 Purple/pink skin theme applied via CSS variables.
- [ ] 4.5 Parent kill-switch (SPEC Q38) — hidden config knob, not in the phone UI.
- [ ] 4.6 Integration: Chaos swap must go through the standard driver flow (so tests catch regressions).

Chaos is LAST. Do not start without explicit go-ahead.

---

## Non-goals (explicit)

- No bundling of RPCS3 or `.sky` files (piracy concern).
- No CI until core features work.
- No Linux/Mac support.
- No user-entered figure names.
- No audio (text-only Chaos to dodge copyright).
- No live wiki scraping at runtime — data is committed to the repo.

## Risks (live list — update as we learn)

- **R1:** UI Automation may not expose enough of the RPCS3 Qt dialog to drive it reliably. Mitigation: phase 1a is the first thing we do; we stop-the-world if it's unworkable.
- **R2:** "Move portal dialog off-screen" may be blocked by Windows or cause focus loss from the game. Mitigation: acceptable fallback is minimizing the main RPCS3 window and relying on controller focus.
- **R3:** Wiki search hit rate might be below 80%. Mitigation: manual curation file checked in, layered over scraped data.
- **R4:** Leptos touch/mobile UX may prove rough. Mitigation: JS fallback is pre-authorized if needed.
- **R5:** RPCS3 version drift will silently break GUI automation. Mitigation: version check on startup, warn on mismatch; pin a known-good dev version.
