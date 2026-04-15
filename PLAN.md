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

### 1a. RPCS3 portal control
- [ ] Read RPCS3 source (it's open source) around the skylander portal emulation to understand: how does the "configure portal" dialog work? What widget tree does Qt expose? What file-picker flow triggers a figure load? What's the "clear slot" action?
- [ ] Enumerate the widget tree on a running RPCS3 via Windows UI Automation (`uiautomation` crate or raw Win32) — confirm we can see the portal dialog, its slot buttons, and file-picker children by name/AutomationId.
- [ ] Spike: can we drive "load figure X into slot N" end-to-end headlessly (dialog off-screen or backgrounded)? Report latency.
- [ ] Investigate whether RPCS3 exposes any log events, IPC, or command-line hooks we can use to *confirm* a slot load happened (so our spinner has a real signal). Fallback: filesystem/timing heuristic.
- [ ] Is the "configure portal" dialog a child window of the emulator, or top-level? Can we move it off-screen on Windows 11 without Windows snapping it back?
- [ ] Writeup: `docs/research/rpcs3-control.md` with chosen mechanism, widget paths, latency numbers, confidence level, and known risks.

### 1b. RPCS3 launch & version detection
- [ ] Determine RPCS3 CLI flags relevant to us (boot title by serial? `--no-gui`? stdout/exit behavior?).
- [ ] Write a tiny spike that launches RPCS3 with a game and captures the process handle + main window handle.
- [ ] Read the version from the running RPCS3 (UIA window title, or file version info of the .exe).
- [ ] Writeup: `docs/research/rpcs3-launch.md`.

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

**Review checkpoint (end of Phase 1):** 1c/1d/1e done. 1a and 1b still required (need interactive RPCS3). If 1a shows UIA is viable — proceed. If not, branch to emulator-source modification or rethink. Update PLAN.md with Phase 2 scope after 1a/1b land.

---

## Phase 2 — Minimal end-to-end slice (post-research, to be detailed after review)

Target: **one phone connects → picks one hardcoded profile → picks one hardcoded game → picks one figure → figure appears on slot 1 in a running game.** No polish, no PINs, no multi-profile. Just prove the whole pipeline works.

Rough sketch (to be filled in after Phase 1 review):
- [ ] Minimal SQLite schema (profiles, figures, working-copies).
- [ ] Figure indexer writes to SQLite on first run.
- [ ] Server: REST for static data, WebSocket for portal events.
- [ ] Phone SPA: one-screen MVP — profile pick (hardcoded list) → game pick → portal view with 8 slots and a figure browser.
- [ ] PC egui: fullscreen, shows QR + "X connected" status.
- [ ] RPCS3 control adapter (from 1a) wired in. Load figure, clear slot. Spinner until RPCS3 confirms.
- [ ] `dev-tools` feature flag; logs to `./logs/`; `.env.dev` for paths.
- [ ] Smoke e2e test: headless browser drives the phone SPA through one round-trip. Scrapes QR URL from log file.

**Review checkpoint (end of Phase 2):** demo the slice end-to-end. Decide what pain points are worst and plan Phase 3 accordingly.

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
