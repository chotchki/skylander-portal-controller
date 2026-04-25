# Skylander Portal Controller — Execution Plan

Active work toward MVP. Closed work lives in [PLAN_ARCHIVE.md](PLAN_ARCHIVE.md) — Phases 0–3 plus most of Phase 4 + parts of Phase 6.

Conventions:
- `[ ]` pending, `[x]` done, `[~]` in progress, `[?]` blocked / needs discussion.
- New tasks should always be numbered and have a checkbox so they're traceable.
- Don't skip a review checkpoint; the point is to re-plan with new information.

---

### 4.18 Phone UI drift reconciliation (residuals)
Tags: **[bug]** wrong behavior, **[feature]** missing capability, **[judgment]** mock is one opinion shipped is another, **[verify]** may already be done.

- [~] 4.18.1 **Mobile viewport / address-bar.** 100dvh + 100svh + safe-area-inset landed; PWA install is the workable path. Follow-up 4.18.1c open.
- [ ] 4.18.1c **Service worker for PWA cache + update detection.** Today static assets return `Cache-Control: no-store`. Add `phone/assets/sw.js`: hashed wasm/js/css/font immutable, `index.html` + manifest `no-cache`, delete stale cache entries on activation, post "new version" message to running SPA. Only mechanism that survives iOS PWA app-shell caching across long backgrounding.
- [~] 4.18.5c **Menu overlay → Konami-gate transition.** Bug half-done (empty-chip hide, commit `439e0d4`). Judgment open: gate-rise + entry-cascade vs plain cross-fade.
- [ ] 4.18.9 *[judgment]* PIN reset: 1-step vs 2-step (Konami as authentication vs defence-in-depth).
- [ ] 4.18.10 *[feature]* Profile "last used N days ago" subtext. Needs `MAX(figure_usage.last_used_at)` or `profiles.last_used_at`.
- [ ] 4.18.12 *[feature]* Per-card tagline + "currently playing" marker.
- [ ] 4.18.14 *[feature]* GAMES drill-down chip row in `BrowserHead`.
- [ ] 4.18.15 *[feature]* CATEGORY drill-down chip row (Vehicles / Traps / Minis / Items). Additive with elements.
- [ ] 4.18.20 *[judgment]* Ghost-grid / box-backdrop context on figure detail.
- [ ] 4.18.22 *[feature]* ResumeModal element-tinted bezel plates.
- [ ] 4.18.23 *[feature]* ResumeModal relative-time subtext. Needs `saved_at` on `ResumeOffer`.
- [ ] 4.18.24 *[judgment]* MenuOverlay post-action transitions (identity-drain / fold-away / lights-dim vs shared clean exit).
- [ ] 4.18.25 Re-run iOS browser smoke-test after 4.18.1c ships.
- [ ] 4.18.26 Once parity reached, 4.17.1's end-to-end demo can proceed.

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

---

## Phase 7 — Packaging + release

Deliberately separated so it's clear this only runs once the app works end-to-end. CI deferred until here.

- [ ] 7.1 **Single-exe distribution.** Everything ships as ONE `skylander-portal.exe`. Phone SPA + images + `figures.json` + fonts (WOFF2) + Kaos SVG embedded via `include_dir!` or `rust-embed`. Release builds strip debug + `cargo build --release` + UPX if >~50 MB.
- [ ] 7.2 GitHub Actions on version-tag push: Windows release build + fast test suite (unit + integration + workspace build, NOT `#[ignore]`-gated e2e), attach zip to release.
- [ ] 7.3 Release `README.md` — user-supplied bits (RPCS3 install path, firmware backup pack). Walk through first-launch wizard. Link `data/LICENSE.md` for Fandom attribution (3.19.6).
- [ ] 7.4 Verify release zip on a *different* Windows machine than the dev one.
- [ ] 7.5 Post-release monitoring plan (GitHub issues only for v1).
- [ ] 7.6 **Trademark / IP review of shipped assets.** Kaos sigil (`docs/aesthetic/kaos_icon.svg` → `phone/assets/kaos.svg`) and any box-art thumbnails (4.2.5). Decide fair-use vs derivative vs custom-drawn before public release.

## Phase 8 Items

### 8.1 Ghost sessions (sticky disconnect)
Today: WS drop → server's `disconnect cleanup` clears the departing
profile's portal slots immediately. PWA backgrounding triggers it
constantly. Goal: keep a phone's figures on the portal across a
disconnect, replay missed events on reconnect, and only evict when a
new phone genuinely takes over the slot.

- [ ] 8.1.1 — Introduce a *ghost session* state on the server. When a
  WS drops with an unlocked profile + placed slots, mark the session
  ghost (profile id + placed-slot snapshot + last-seen timestamp).
  Don't clear figures, don't fire `disconnect cleanup`. Ghost stays
  in the registry's slot allocation.
- [ ] 8.1.2 — Per-ghost replay buffer for events that arrived after
  the WS dropped but the phone needs on reconnect. At minimum the
  KaosTaunt event (8.2b.4 fires while the phone is asleep / PWA
  backgrounded) plus any post-disconnect SlotChanged for the
  ghost's own slots. Bounded ring (last N ≤ ~32 events; pre-2026 we
  capped at "last 10 minutes worth" — pick whichever fits).
- [ ] 8.1.3 — On reconnect, match the incoming Welcome's profile-id
  hint (cookie? localStorage echo? we'll need a phone-side handle)
  against ghost sessions. If a ghost matches, *adopt* it — keep the
  same session id, skip the resume modal entirely (figures are
  already where the user left them), and drain the replay buffer
  into the phone in event order so the Kaos taunt etc. lands.
- [ ] 8.1.4 — 2-phone cap counts ghosts as occupying a slot. A 3rd
  connection still FIFO-evicts the oldest, ghost or live. Forced-
  eviction cooldown still applies. When a ghost is evicted, ITS
  slots clear (deferred cleanup runs at evict time, not disconnect
  time).
- [ ] 8.1.5 — Time-bound ghosts (1 hour idle) so they don't pile up
  forever after a real abandon. After timeout: evict + cleanup.
- [ ] 8.1.6 — UI: live phones see ghost-placed figures with their
  existing `placed_by` attribution; surface a subtle "(away)" hint
  on the orbit pip so it's clear which phones are responsive.
- [ ] 8.1.7 — Tests: ghost create/adopt/evict/expire round-trips
  against the in-memory session registry; replay buffer ordering;
  KaosTaunt-during-disconnect replay scenario.

### 8.2a Kickback cooldown countdown UI
Today: kickback button is enabled immediately on the Kaos takeover
screen; server returns 401-RetryAfter if the 60s cooldown hasn't
elapsed. Should grey-out + count down instead.

- [ ] 8.2a.1 — Server includes `cooldown_remaining_secs` in the
  `TakenOver` event payload.
- [ ] 8.2a.2 — Phone Kaos overlay starts a local 1Hz countdown from
  that value.
- [ ] 8.2a.3 — Button styled disabled while countdown > 0 (grey +
  ring or seconds-remaining caption); enables at zero.

### 8.2b Kaos feature (CLAUDE.md "Kaos feature")
The Skylanders-themed mid-game disruption — wall-clock timer fires,
a portal figure gets swapped for a random compatible one from the
owner's collection, a Kaos catchphrase overlays for ~5s.

- [ ] 8.2b.1 — Per-profile Kaos enable toggle (kebab menu; off by
  default while we tune the cadence).
- [ ] 8.2b.2 — Server timer task: 20-min warmup from session unlock,
  then uniformly-random fire within each hour window.
- [ ] 8.2b.3 — Compatibility-aware swap selection: pick a portal
  figure + a compatible replacement from the owning profile's
  collection via `compat::is_compatible` (vehicles SuperChargers-
  only edge case already handled there).
- [ ] 8.2b.4 — Execute the swap as a clear+load pair, broadcast
  `Event::KaosTaunt { profile_id, slot, taunt }` with a random
  catchphrase from `data/kaos_taunts.json`. Pairs with 8.1.2 — the
  taunt has to land even if the targeted phone is backgrounded /
  briefly disconnected when it fires.
- [ ] 8.2b.5 — Phone `KaosOverlay` component (already exists)
  renders the taunt + visual treatment for ~5s, then dismisses.
- [ ] 8.2b.6 — Tests: timer math, swap selection (vehicles edge
  case), taunt rotation, replay-on-reconnect.

### 8.3 Release 1.1
- [ ] 8.3.1 — Release notes drafted from commits since v1.0.0
  (`generate_release_notes` already wired in `release.yml`).
- [ ] 8.3.2 — Tag → CI release workflow → draft release with the new
  exe → publish.

## Phase 9 Items
- [ ] 9.1 - Add MacOS support
- [ ] 9.2 - Add fully automated e2e testing since we can run it all on a Mac
- [ ] 9.3 - Spike evaluate a frontend component framework to make the app.css more reasonable
- [ ] 9.4 - Optimize ipad and iphone layouts

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
