# Skylander Portal Controller — Execution Plan

Active work toward MVP. Closed work lives in [PLAN_ARCHIVE.md](PLAN_ARCHIVE.md) — Phases 0–3 plus most of Phase 4 + parts of Phase 6.

Conventions:
- `[ ]` pending, `[x]` done, `[~]` in progress, `[?]` blocked / needs discussion.
- New tasks should always be numbered and have a checkbox so they're traceable.
- Don't skip a review checkpoint; the point is to re-plan with new information.

---

## Phase 4 — Aesthetic + UX pass (residuals)

Most of Phase 4 shipped — see [PLAN_ARCHIVE.md](PLAN_ARCHIVE.md) for §4.1–4.14 + done pieces of 4.15/4.18/4.19/4.20/4.21. Remaining items below.

### 4.15 egui TV launcher
- [ ] 4.15.9 **Game-switching transition.** Phone picks another game → clouds spiral in → "SWITCHING GAMES…" → RPCS3 loads → clouds spiral out. Same `PostMessage` input routing as 4.15.8; `quit_via_file_menu` must also move `SendInput` → `PostMessage`.
- [ ] 4.15.12 **Shader compilation detection (research spike).** Investigate (a) log-file watcher, (b) viewport title polling, (c) FPS <5 for >5s heuristic. Fallback: fixed 15s post-boot.
- [ ] 4.15.13 **Shader progress visualization** (depends 4.15.12). Gold conic-gradient ring (200–240px), count in Titan One, ring flashes on completion.

### 4.16 E2E test updates
- [ ] 4.16.1 Audit `crates/e2e-tests/` selectors post-reskin — move to `data-test` attrs where possible.
- [ ] 4.16.2 Visual regression out of scope — manual review only.
- [ ] 4.16.3 Manual multi-phone visual sanity on HTPC.

### 4.17 Review checkpoint
- [ ] 4.17.1 HTPC end-to-end demo: launcher → phone scan → profile → PIN → game picker → portal in all 5 slot states → takeover.
- [ ] 4.17.2 Catalogue UX papercuts → route to Phase 3 carryover.

### 4.18 Phone UI drift reconciliation (residuals)
Tags: **[bug]** wrong behavior, **[feature]** missing capability, **[judgment]** mock is one opinion shipped is another, **[verify]** may already be done.

- [~] 4.18.1 **Mobile viewport / address-bar.** 100dvh + 100svh + safe-area-inset landed; PWA install is the workable path. Follow-up 4.18.1c open.
- [ ] 4.18.1c **Service worker for PWA cache + update detection.** Today static assets return `Cache-Control: no-store`. Add `phone/assets/sw.js`: hashed wasm/js/css/font immutable, `index.html` + manifest `no-cache`, delete stale cache entries on activation, post "new version" message to running SPA. Only mechanism that survives iOS PWA app-shell caching across long backgrounding.
- [~] 4.18.5c **Menu overlay → Konami-gate transition.** Bug half-done (empty-chip hide, commit `439e0d4`). Judgment open: gate-rise + entry-cascade vs plain cross-fade.
- [ ] 4.18.6 *[judgment]* CreateProfileForm pacing: single form vs 4-step wizard.
- [ ] 4.18.9 *[judgment]* PIN reset: 1-step vs 2-step (Konami as authentication vs defence-in-depth).
- [ ] 4.18.10 *[feature]* Profile "last used N days ago" subtext. Needs `MAX(figure_usage.last_used_at)` or `profiles.last_used_at`.
- [ ] 4.18.11 *[verify]* Profile manage DEL uses HOLD TO DELETE, not `window.confirm()`.
- [ ] 4.18.12 *[feature]* Per-card tagline + "currently playing" marker.
- [ ] 4.18.14 *[feature]* GAMES drill-down chip row in `BrowserHead`.
- [ ] 4.18.15 *[feature]* CATEGORY drill-down chip row (Vehicles / Traps / Minis / Items). Additive with elements.
- [ ] 4.18.16 *[verify]* Toy-box lid grabber pill + swipe-hint copy shipped.
- [ ] 4.18.19 *[verify]* Hero-aura + hero-rays behind lifted figure bezel.
- [ ] 4.18.20 *[judgment]* Ghost-grid / box-backdrop context on figure detail.
- [ ] 4.18.22 *[feature]* ResumeModal element-tinted bezel plates.
- [ ] 4.18.23 *[feature]* ResumeModal relative-time subtext. Needs `saved_at` on `ResumeOffer`.
- [ ] 4.18.24 *[judgment]* MenuOverlay post-action transitions (identity-drain / fold-away / lights-dim vs shared clean exit).
- [ ] 4.18.25 Re-run iOS browser smoke-test after 4.18.1c ships.
- [ ] 4.18.26 Once parity reached, 4.17.1's end-to-end demo can proceed.
- [ ] 4.18.27 *[bug → feature]* **Profile-create → staged wizard.** Leptos port of 4.2.8's `profile_create.html` crunched the staged flow into one long form; on iPhone the confirm keypad scrolls off-screen. `.screen-profile-picker { overflow-y: auto }` was the minimum-viable unblock 2026-04-23; real fix is the 4-step wizard that matches the mock. Keep the scroll-fix in place as defensive fallback.
- [ ] 4.18.28 *[bug]* **Profile-create color scheme alignment.** Still uses pre-Phase-4 gray (`#161616` bg, `#333` border). Swap to `FramedPanel` + `ActionButton` / gold-bezel button treatments. Pair with 4.18.27.
- [ ] 4.18.29 *[bug]* **Reset browser search on game change.** `search` signal in `phone/src/lib.rs` persists across `current_game` transitions. Also applies to `element_filter` / `game_filter` / `category_filter` (less disruptive since visibly toggled). Fix: `Effect` that clears all four when `current_game` changes (skip initial `None → Some(_)` run). Surfaced 2026-04-23 during 6.6 live verification.

### 4.19 egui TV-launcher drift reconciliation (residuals)

**States (§3.1) — state machine collapsed.**
- [ ] 4.19.2 *[feature]* **No "Booting" surface.** Spec: iris closes + "LOADING" + game name + boot status. Today mid-launch renders QR + brand heading.
- [ ] 4.19.3 *[feature]* **No "Switching Games" surface** (iris-close between In-Game and next Booting).
- [ ] 4.19.4 *[feature]* **No "Compiling Shaders" surface** (depends 4.19.17).

**Cloud + iris (§3.2) — tuning is static.**
- [ ] 4.19.5 *[judgment]* **Vortex shader: polar-mesh approximation vs spec'd simplex FBM.** Downgraded from [feature] 2026-04-19 — surrounding improvements (sky backdrop + starfield + halo glow) carry most weight. Re-eval before committing to `egui_wgpu` port.
- [ ] 4.19.6 *[feature]* **Iris locked at 1.2 for every state.** Spec: per-state tuning (Booting 2.5s ease-out, Crash ~1s urgent, Shutdown gentle, In-Game ~1.8s ease-in). Coupled with 4.15a.7.
- [ ] 4.19.7 *[verify]* **Halo focal-glow missing** behind QR / progress ring. Primitive ready: `paint_radial_ellipse`.
- [ ] 4.19.8 *[verify]* **Pip orbit speed: code 0.10 rad/s vs spec 0.08.**

**QR + orbit (§3.3).**
- [ ] 4.19.9 *[bug]* **Max-players copy mismatch.** Code: "MAXIMUM PLAYERS REACHED"; spec: "PORTAL IS FULL".
- [ ] 4.19.10 *[verify]* **"SCAN TO CONNECT" label position + size** (above 36px vs below 64px).
- [ ] 4.19.10a *[bug]* **URL string rendered on screen** — spec says QR carries it, no text.
- [ ] 4.19.11 *[verify]* **QR bezel size: 320px code vs ~280px spec.** Likely intentional 4K headroom.
- [ ] 4.19.22 *[bug]* **AwaitingConnect debug noise.** Today adds brand heading + status strip + URL + client count + figures-indexed. Drop all five. Keep Exit-to-Desktop button.

**In-Game transparency (§3.4).**
- [ ] 4.19.12 *[bug]* **Reconnect QR no fade-in** (spec: 1.0s ease-out).
- [ ] 4.19.13 *[verify]* **Reconnect QR copy + inset.**

**Shutdown (§3.5).**
- [ ] 4.19.14 *[bug]* **No breathe pulse on farewell heading** (spec: 2.4s opacity + scale ±2.5%).
- [ ] 4.19.15 *[feature]* **No black-overlay fade-out + hint sequence.** egui has no native full-screen overlay equivalent; either custom paint or accept current.
- [ ] 4.19.16 *[verify]* **Heading size: 72px code vs 64px spec.**

**Shader compilation (§3.6).**
- [ ] 4.19.17 *[feature]* **Detection unimplemented.** Duplicates 4.15.12.
- [ ] 4.19.18 *[feature]* **Progress ring + heading missing** (depends 4.19.17). Duplicates 4.15.13.

**Typography (§3.7).**
- [ ] 4.19.19 *[verify, half-done]* **Hero size 80px code vs 96px spec**; farewell 72 vs 64. 4.19.2a landed 140px embossed for Startup; steady-state still flat-and-small.

**Wrap-up.**
- [ ] 4.19.20 Re-walk every state on HTPC once 4.19.2–4.19.19 land.
- [ ] 4.19.21 Once parity reached, launcher can be demo subject of 6.4.

### 4.20 Design system consolidation (residual)
- [ ] 4.20.14 Re-run design language audit after 4.20.1–13 land. Any remaining drift either folds into 4.17 or surfaces a new 4.20.x.

---

## Phase 5 — Kaos

Kaos is LAST among feature work. Do not start without explicit go-ahead.

- [ ] 5.1 Wall-clock timer: 20min warmup + randomized 60min windows.
- [ ] 5.2 Text-only overlay with Kaos catchphrases (curated in-repo list; text avoids audio copyright). Two surfaces mocked in Phase 4: `kaos_takeover.html` + `kaos_swap.html`. **No auto-dismiss.** Multiple fires while asleep: latest-wins or queue (decide during impl).
- [ ] 5.3 1-for-1 swap of a portal figure with a random compatible-with-current-game figure.
- [ ] 5.4 Purple/pink Kaos skin via CSS variable swap (rides on Phase 4's `--*` tokens; palette swap, not rewrite).
- [ ] 5.5 Parent kill-switch (SPEC Q38) — hidden config knob, not in the phone UI.
- [ ] 5.6 Kaos swap goes through the standard driver flow.

---

## Phase 6 — Post-Kaos polish (residuals)

- [ ] 6.1 **Suppress RPCS3 window flicker during menu navigation.** Launcher starts before RPCS3 → establish Z-order priority. Ideas: (a) launcher `WS_EX_TOPMOST` during `open_dialog` nav so Qt popups render behind, (b) `SetWinEventHook` / `EVENT_OBJECT_SHOW` filtered to RPCS3 PID to intercept dialog creation and move off-screen before first paint, (c) hook menu popups the same way.

### 6.2 Parse `.sky` firmware for per-figure stats (partial)
Encryption handled (6.2.0 + 6.2.0b archived). Identity fields decode correctly; payload fields decode post-decryption. 141/151 CRC-valid on real dumps.

- [~] 6.2 parent `[~]` until per-kind coverage lands + all 22 tests have ciphertext-fixture counterparts.
- [ ] 6.2.1 **UI determination pass for Trap / Vehicle / CYOS.** Mock reduced Figure Detail variants before parser work — fields we don't render aren't worth decoding. Targets: Trap → captured villain name + portrait headline, Vehicle → SSCR level headline + adornment names, CYOS → class + element + nickname with missing-field tolerance. Racing Pack keeps default "STATS COMING SOON" strip.
- [ ] 6.2.3 **Trap payload: captured villain identity.** Parse villain cache per `docs/research/sky-format/SkylanderFormat.md` line 39ff. `SkyFigureStats.trap: Option<TrapData>`. `data/villains.json` lookup for display name + portrait. ~3 tests.
- [ ] 6.2.4 **Vehicle payload: SSCR level.** Parse XP + level derivation. `SkyFigureStats.vehicle: Option<VehicleData>` + `data/vehicle_adornments.json`. Skip gearbits/flags/mod-flags this pass. ~3 tests.
- [ ] 6.2.5 **CYOS payload: class / element / nickname best-effort.** Parse deterministic bits + attempt nickname recovery from the 0x65-byte payload. Return `Option<CyosData>` with *individually-optional fields*. Emit structured warning log on CRC-pass-but-field-mismatch.
- [ ] 6.2.6 **Wire per-kind payloads through `/api/profiles/:profile_id/figures/:figure_id/stats` + `phone/src/screens/figure_detail.rs`.** JSON response gains nested `trap` / `vehicle` / `cyos` (null for Racing Pack). Uses existing `sky-stats` feature flag.
- [ ] 6.2.8 **Investigate the 10 CRC-failing samples from 6.2.0b.** Trap Team Adventure Packs (fids 0x131–0x134), Imaginators Senseis/Creation-Crystal era (King Pen, Wild Storm, Crash Bandicoot, Dr. Neo Cortex, Air Strike, Sheep Creep 0xC82). Hypotheses: (a) different CRC scope, (b) extended Sensei data layout, (c) factory-blank never-played figures. Chris to load in emulator + compare observed vs parser output.
- [ ] 6.2.9 **Pin Vehicle + CYOS `figure_id` ranges against real dumps.** 6.2.2 left these ranges commented out — community values missed every real sample. Observed vehicle-looking IDs cluster near `0x0C9x..=0x0CAx` (overlap with SuperChargers characters). First live CYOS data point: creation crystal fid=`0x0002AD`. Blocks 6.2.4 / 6.2.5. Deliverables: (a) more SuperChargers vehicle + CYOS dumps via 6.5.0's scan tool; (b) extend `FigureKind` range table with real-observed fids + tests; (c) smoke-test `decode_nickname` against new CYOS samples.

### 6.3 Detailed-stats screen on the phone
- [ ] 6.3 Level + XP, gold, current hat, playtime, nickname, hero points, hat history, trinket, quest progress. Hits stats endpoint; read-only. Non-standard layouts render reduced panel until 6.2 stubs fill. Placeholder today: `.detail-stats-soon` strip in `phone/assets/app.css`; when this ships, delete `.detail-stats-soon*` + soon-label span and reinstate the three `.detail-stat-cell` blocks wired to fetched stats.

### 6.4 Demo harness for screen recording
- [ ] 6.4 Browser-viewable test session driving the phone SPA through a representative flow (profile → PIN → game → portal → toy box → place → Kaos swap). Runs side-by-side with remote-desktop HTPC view for single-frame recording.

### 6.5 NFC-scan import (residuals)
Pipeline landed; see PLAN_ARCHIVE.md §6.5. Small follow-ups:

- [~] 6.5.2 Timeout path not live-tested — watch for during in-game tests.

---

## Phase 7 — Packaging + release

Deliberately separated so it's clear this only runs once the app works end-to-end. CI deferred until here.

- [ ] 7.1 **Single-exe distribution.** Everything ships as ONE `skylander-portal.exe`. Phone SPA + images + `figures.json` + fonts (WOFF2) + Kaos SVG embedded via `include_dir!` or `rust-embed`. Release builds strip debug + `cargo build --release` + UPX if >~50 MB.
- [ ] 7.2 GitHub Actions on version-tag push: Windows release build + fast test suite (unit + integration + workspace build, NOT `#[ignore]`-gated e2e), attach zip to release.
- [ ] 7.3 Release `README.md` — user-supplied bits (RPCS3 install path, firmware backup pack). Walk through first-launch wizard. Link `data/LICENSE.md` for Fandom attribution (3.19.6).
- [ ] 7.4 Verify release zip on a *different* Windows machine than the dev one.
- [ ] 7.5 Post-release monitoring plan (GitHub issues only for v1).
- [ ] 7.6 **Trademark / IP review of shipped assets.** Kaos sigil (`docs/aesthetic/kaos_icon.svg` → `phone/assets/kaos.svg`) and any box-art thumbnails (4.2.5). Decide fair-use vs derivative vs custom-drawn before public release.

---

## Non-goals

- No bundling of RPCS3 or `.sky` files (piracy concern).
- No CI until core features work.
- No Linux/Mac support.
- No user-entered figure names.
- No audio (text-only Kaos to dodge copyright).
- No live wiki scraping at runtime — data is committed to the repo.

## Risks (live list — update as we learn)

- **R1:** UI Automation may not expose enough of the RPCS3 Qt dialog to drive it reliably. Resolved: Alt-keyboard-nav workaround (CLAUDE.md "RPCS3 window/menu gotchas").
- **R2:** "Move portal dialog off-screen" may be blocked by Windows. Resolved: Win32 `SetWindowPos` works; `hide_dialog_offscreen` + RAII guard in `crates/rpcs3-control/src/hide.rs`.
- **R3:** Wiki search hit rate might be below 80%. Resolved: 504/504 coverage (3.19.5).
- **R4:** Leptos touch/mobile UX may prove rough. Mitigation: ongoing Phase 4.18 on-device iteration; PWA install fallback.
