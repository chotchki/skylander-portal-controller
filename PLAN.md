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

### 2.3 RPCS3 control (`crates/rpcs3-control/`)

- [ ] 2.3.1 Port `tools/uia-drive/src/main.rs` helpers into the library's private `uia/` module (find_dialog, find_group_box, find_row, find_descendant, value_of, poll_until_changes).
- [ ] 2.3.2 Define `pub trait PortalDriver { fn open_dialog(&self) -> Result<()>; fn read_slots(&self) -> Result<[SlotState; 8]>; fn load(&self, slot: SlotIndex, path: &Path) -> Result<LoadedFigureName>; fn clear(&self, slot: SlotIndex) -> Result<()>; }` using the core `SlotState`.
- [ ] 2.3.3 Implement `UiaPortalDriver` — holds a `UIAutomation` handle, re-resolves the dialog on every call (tolerates `WA_DeleteOnClose`).
- [ ] 2.3.4 `open_dialog()`: find the RPCS3 main window; if dialog not present, click `Manage` menu → `Manage Skylanders Portal`. Block until dialog appears.
- [ ] 2.3.5 `read_slots()`: walk rows 1–8, read each `QLineEdit` via `ValuePattern`. "None" → `Empty`, anything else → `Loaded { display_name }`. No figure_id yet — reconciliation happens at a higher layer via name lookup.
- [ ] 2.3.6 `load(slot, path)`: if slot not Empty, call `clear()` first; invoke Load button; wait for file dialog ("Select Skylander File"); set file-name edit (AutomationId 1148) via ValuePattern; invoke Open (AutomationId 1); poll slot QLineEdit until value changes from prior; return the new value. Timeout 10s with clear error.
- [ ] 2.3.7 `clear(slot)`: invoke row's Clear button; poll until value becomes "None". Timeout 3s.
- [ ] 2.3.8 Error-modal handling: if a `QMessageBox` top-level window appears during any action, capture its text, dismiss it, surface the message as the operation's error.
- [ ] 2.3.9 Off-screen helper: `hide_dialog_offscreen()` uses Win32 `SetWindowPos` via `NativeWindowHandle` to move the dialog to `(-4000,-4000)`. Verify UIA accessibility still works (the 1a probe said yes in principle; confirm on RPCS3 here).
- [ ] 2.3.10 `structured tracing` events at every step (`rpcs3.driver.open`, `.find_row`, `.invoke_load`, `.poll_value`, `.success`, `.error`).
- [ ] 2.3.11 `MockPortalDriver` behind a `mock` feature flag — in-memory slot state, sleeps 50ms to simulate latency, deterministic for tests.
- [ ] 2.3.12 Integration test: `ignore`d by default (requires interactive RPCS3); `cargo test --ignored rpcs3_live_load` drives one load end-to-end using the real driver.
- [ ] 2.3.13 Unit tests using `MockPortalDriver` for load/clear/full-slot flows.
- [ ] 2.3.14 Serialize driver actions via `tokio::sync::Mutex` inside the server — not the crate's job, but document the requirement in the trait docs.

### 2.4 Indexer (`crates/indexer/`) — DONE (mostly)

- [x] 2.4.1 Port `tools/inventory/src/main.rs` into a library crate. Public API: `pub fn scan(root: &Path) -> Result<Vec<Figure>>`.
- [x] 2.4.2 Preserve the classification rules and the variant-prefix peel list from the spike.
- [x] 2.4.3 `category = "vehicle"` first-class (was "other" in the spike). 27 entries.
- [x] 2.4.4 `category = "trap"` first-class (was "item" with element). 57 entries.
- [x] 2.4.5 Element icon path resolved per figure (~97.6% coverage on real pack).
- [~] 2.4.6 Snapshot test deferred. Replaced with a count-check integration test (`tests/real_pack.rs`, gated by `SKYLANDER_PACK_ROOT` env var) that verifies per-game + per-category totals match Phase 1c exactly. A full JSON snapshot is overkill for now.
- [x] 2.4.7 `tools/inventory/` left as-is — it's the historical Phase 1c builder. Future regeneration should use the library.

### 2.5 Dev config + bootstrap

- [ ] 2.5.1 Read `.env.dev` at startup when the `dev-tools` feature flag is on (otherwise first-launch config wizard — Phase 3). Keys: `RPCS3_EXE`, `FIRMWARE_PACK_ROOT`, `GAMES_YAML` (optional, defaults to `<RPCS3_EXE dir>/config/games.yml`), `BIND_PORT` (default 8765).
- [ ] 2.5.2 Add `dev-tools` Cargo feature to `crates/server/` (default ON during development, OFF in release).
- [ ] 2.5.3 Commit `.env.dev.example` with placeholders; actual `.env.dev` stays gitignored.
- [ ] 2.5.4 Log destination: `./logs/server.log` when `dev-tools`; `%APPDATA%/skylander-portal-controller/logs/` otherwise. Daily rotation, 7-day retention (use `tracing-appender`).
- [ ] 2.5.5 On startup, log the resolved config to the log file and to stdout (visible to the e2e test harness for URL-scraping).

### 2.6 Server (`crates/server/`)

- [ ] 2.6.1 Scaffold app state: `AppState { figures: Vec<Figure>, driver: Arc<dyn PortalDriver>, portal_state: Arc<Mutex<[SlotState; 8]>>, broadcast: broadcast::Sender<Event> }`.
- [ ] 2.6.2 Startup sequence: (1) read config, (2) build indexer → `figures`, (3) construct `UiaPortalDriver`, (4) kick off a tokio task to do an initial `driver.read_slots()` and populate `portal_state`, (5) start Axum, (6) start eframe.
- [ ] 2.6.3 REST: `GET /api/figures` returns `Vec<PublicFigure>` (phone-safe).
- [ ] 2.6.4 REST: `GET /api/portal` returns current `[SlotState; 8]`.
- [ ] 2.6.5 REST: `POST /api/portal/slot/{n}/load` body `{ figure_id }` — validates slot range, looks up figure, enqueues a driver job, returns 202 with a correlation ID.
- [ ] 2.6.6 REST: `POST /api/portal/slot/{n}/clear` — same pattern.
- [ ] 2.6.7 WebSocket `/ws`: on connect, send a `PortalSnapshot`; subscribe to broadcast for subsequent `SlotChanged` events.
- [ ] 2.6.8 Driver job queue: a single-worker tokio task drains a `mpsc` of commands, invokes the driver serially, updates `portal_state`, broadcasts `SlotChanged`. Serialising here keeps the UIA driver sane.
- [ ] 2.6.9 Serve the phone SPA static bundle via `tower_http::services::ServeDir` pointed at `phone/dist/` (dev) or an `include_dir!`-embedded copy (release). Gate behind the `dev-tools` feature.
- [ ] 2.6.10 Serve element icon PNGs at `/assets/element/{Element}.png`, resolved from the firmware pack.
- [ ] 2.6.11 First-non-loopback-IP pick at startup; log the URL clearly.
- [ ] 2.6.12 Unit tests for the routes using `axum::test` + `MockPortalDriver`.

### 2.7 Phone SPA (`phone/`)

Keep the scope surgical. One screen. Three columns on iPad, one-above-the-other on phone.

- [ ] 2.7.1 Promote `tools/phone-smoke/` to `phone/`. Rename crate to `skylander-portal-phone`.
- [ ] 2.7.2 WS client: connect on mount, auto-reconnect with backoff, dispatch incoming `Event` into a Leptos signal.
- [ ] 2.7.3 REST helpers: typed `fetch_figures()` and `post_load(slot, figure_id)` / `post_clear(slot)`.
- [ ] 2.7.4 Component `<Portal />`: renders 8 slots (the 8th shown only if the MVP needs it; show all 8 for simplicity). Each slot shows current `display_name` or "Empty"; a Remove button when loaded; a "pick figure" affordance when empty.
- [ ] 2.7.5 Component `<FigureBrowser />`: a grid of figure cards. Element icon + canonical name. Filter: element dropdown + text search (minimum viable; fuller filters in Phase 3).
- [ ] 2.7.6 Interaction flow: user selects a slot (tap) → `FigureBrowser` goes into "picking for slot N" mode → tap a figure → POST `/load` → optimistic "loading" state on the slot → WS `SlotChanged` flips it to loaded. No drag-drop.
- [ ] 2.7.7 Minimal CSS — function over form. The Skylanders aesthetic pass is Phase 3. Use `phone/assets/spike.css` as a starting point and only enough styling to be legible.
- [ ] 2.7.8 Error display: transient toasts from `Event::Error`.
- [ ] 2.7.9 Add `Trunk.toml` with release profile + a build step that outputs to `phone/dist/` consistently.
- [ ] 2.7.10 Manually verify the SPA builds and the button interactions against a running server on the HTPC.

### 2.8 eframe launcher window

- [ ] 2.8.1 Port the Phase 1 spike's eframe `SpikeApp` into `crates/server/src/ui.rs`. Fullscreen, 86"@10ft sizing (large fonts).
- [ ] 2.8.2 Show: big QR code of the server URL, URL text, "Clients connected: N" counter, RPCS3 connection status (has-dialog? unknown?).
- [ ] 2.8.3 Share a single `AppState` pointer between Axum and eframe.
- [ ] 2.8.4 No buttons in MVP (Phase 3 adds hide-dialog / restart-game / exit).

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
- [ ] 2.10.3 Test case: load the URL, assert the figure grid renders, click slot 1, click first figure, assert the slot shows Loading then Loaded (WS driven).
- [ ] 2.10.4 Use `MockPortalDriver` — the e2e suite doesn't need RPCS3. Driver selection gated by `SKYLANDER_PORTAL_DRIVER=mock` env var honored when the `dev-tools` feature is active.
- [ ] 2.10.5 Keep this suite manually run locally; no CI.

### 2.11 Cleanup + commit hygiene

- [ ] 2.11.1 Delete the Phase 1 `src/main.rs` and `assets/spike_index.html` once 2.1.7 has moved the useful parts into `crates/server/`.
- [ ] 2.11.2 Delete `tools/phone-smoke/` once 2.7.1 has migrated to `phone/`.
- [ ] 2.11.3 Update CLAUDE.md with the final Phase 2 workspace layout.
- [ ] 2.11.4 Update `README.md` with a brief "how to run" for developers.

---

**Review checkpoint (end of Phase 2):** demo the end-to-end slice on the HTPC. Identify the three biggest pain points. Plan Phase 3 (profiles + PINs + session resume + game launching) accordingly.

---

## Phase 3+ — To be planned after the MVP works

Likely areas (order TBD):
- Full profile system with PINs and per-profile working copies.
- Takeover + kick-back flow with Chaos-themed kicked screen.
- Full collection browse view (filters: element, game of origin, works-with, type).
- Reposes: collapse + variant cycling.
- Session resume / layout memory.
- Reset-to-fresh action.
- HMAC signing of commands.
- Aesthetic pass (Skylanders-style CSS; reference `docs/aesthetic/ui_style_example.png`).
- Reconnect QR overlay window.
- Windows firewall UX and network-interface fallback picker.
- First-launch config wizard (egui side).
- Game-graceful-quit with 30s kill timer.
- Packaging / GitHub Releases pipeline (CI still deferred).
- Wiki scrape second pass for misses.
- Chaos feature (LAST, explicit user go-ahead required).

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
