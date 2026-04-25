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

## Phase 8 Items

### 8.1 Ghost sessions (sticky disconnect)
Today: WS drop → server's `disconnect cleanup` clears the departing
profile's portal slots immediately. PWA backgrounding triggers it
constantly. Goal: keep a phone's figures on the portal across a
disconnect, replay missed events on reconnect, and only evict when a
new phone genuinely takes over the slot.

- [x] 8.1.1 — Introduce a *ghost session* state on the server. When a
  WS drops with an unlocked profile + placed slots, mark the session
  ghost (profile id + placed-slot snapshot + last-seen timestamp).
  Don't clear figures, don't fire `disconnect cleanup`. Ghost stays
  in the registry's slot allocation. `SessionState::ghosted_at` +
  `SessionRegistry::ghost`; WS exit path routes profile-bound
  sessions here instead of `remove`.
- [x] 8.1.2 — Per-ghost replay buffer for events that arrived after
  the WS dropped but the phone needs on reconnect. At minimum the
  KaosTaunt event (8.2b.4 fires while the phone is asleep / PWA
  backgrounded) plus any post-disconnect SlotChanged for the
  ghost's own slots. Bounded ring (last N ≤ ~32 events; pre-2026 we
  capped at "last 10 minutes worth" — pick whichever fits).
  `REPLAY_BUFFER_LIMIT = 32` ring on `SessionState`;
  `push_replay_for_profile` + `drain_replay`. Producer-side wiring
  (which events fan into the buffer) lands with the consumers in
  8.2b.4.
- [x] 8.1.3 — On reconnect, match the incoming Welcome's profile-id
  hint (cookie? localStorage echo? we'll need a phone-side handle)
  against ghost sessions. If a ghost matches, *adopt* it — keep the
  same session id, skip the resume modal entirely (figures are
  already where the user left them), and drain the replay buffer
  into the phone in event order so the Kaos taunt etc. lands.
  Server-side: `SessionRegistry::claim_ghost` + WS handler accepts
  `?reclaim=<profile_id>` and flushes the drained buffer to the new
  socket. Phone-side reclaim hint (localStorage echo) is the
  follow-up bullet — the server side is in place and falls back to
  `register()` when the hint is absent.
- [x] 8.1.4 — 2-phone cap counts ghosts as occupying a slot. A 3rd
  connection still FIFO-evicts the oldest, ghost or live. Forced-
  eviction cooldown still applies. When a ghost is evicted, ITS
  slots clear (deferred cleanup runs at evict time, not disconnect
  time). Implemented end-to-end: `RegistrationOutcome::AdmittedByEvicting`
  now carries `evicted_ghost_profile`; the WS handler runs
  `clear_slots_for_profile` on that profile inline. WS disconnect path
  routes profile-bound sessions through `SessionRegistry::ghost`
  instead of `remove`, so figures stay on the portal until claim or
  expiry.
- [x] 8.1.5 — Time-bound ghosts (1 hour idle) so they don't pile up
  forever after a real abandon. After timeout: evict + cleanup.
  `AppState::sweep_expired_ghosts` runs every 60s from a tokio task
  spawned in `main.rs`; `GHOST_TIMEOUT = 1h`.
- [x] 8.1.6 — UI: live phones see ghost-placed figures with their
  existing `placed_by` attribution; surface a subtle "(away)" hint
  on the orbit pip so it's clear which phones are responsive.
  `SessionPip.is_ghost`; ghost pips render with a desaturated profile
  colour + half-alpha bezel + dimmed glyph. Updates fan in via the
  existing `publish_session_snapshot` hooks on `ghost`, `claim_ghost`,
  and the periodic sweep.
- [x] 8.1.7 — Tests: ghost create/adopt/evict/expire round-trips
  against the in-memory session registry; replay buffer ordering;
  KaosTaunt-during-disconnect replay scenario.
  14 tests in `crates/server/tests/profiles.rs` cover ghost-stickiness,
  expire-only-stale, replay matching + overflow, claim picks-oldest,
  ghost-counts-toward-cap (with profile_id surfacing on force-evict),
  and a full chained round-trip standing in for KaosTaunt-during-
  disconnect via `Event::Error`. The KaosTaunt variant lands with
  8.2b.4; the buffer's behavior is variant-agnostic so adding the
  new event will plug straight into the existing path.

### 8.2a Kickback cooldown countdown UI
Today: kickback button is enabled immediately on the Kaos takeover
screen; server returns 401-RetryAfter if the 60s cooldown hasn't
elapsed. Should grey-out + count down instead.

- [x] 8.2a.1 — Server includes `cooldown_remaining_secs` in the
  `TakenOver` event payload. Sourced from `FORCED_EVICT_COOLDOWN`
  at the AdmittedByEvicting site; phone-side wire is `#[serde(default)]`
  so a stale phone bundle still parses TakenOver against a newer server.
- [x] 8.2a.2 — Phone Kaos overlay starts a local 1Hz countdown from
  that value. `KaosOverlay` runs an `Effect` that seeds a
  `cooldown_remaining` signal off the takeover prop and ticks via
  a self-cancelling `setInterval` (clears itself when the count
  hits zero or the overlay dismisses).
- [x] 8.2a.3 — Button styled disabled while countdown > 0 (grey +
  ring or seconds-remaining caption); enables at zero. `disabled=`
  binding + `takeover-kick-btn--cooldown` class drive the muted
  visual; label appends ` · {n}s` while ticking, reverts to plain
  "KICK BACK IN" on zero.

### 8.2b Kaos feature (CLAUDE.md "Kaos feature")
The Skylanders-themed mid-game disruption — wall-clock timer fires,
a portal figure gets swapped for a random compatible one from the
owner's collection, a Kaos catchphrase overlays for ~5s.

- [x] 8.2b.1 — Per-profile Kaos enable toggle (kebab menu; off by
  default while we tune the cadence). `profiles.kaos_enabled`
  migration + `POST /api/profiles/:id/kaos`; kebab menu surfaces
  ENABLE / DISABLE action; server rebroadcasts `ProfileChanged` on
  flip so both co-op phones update.
- [x] 8.2b.2 — Server timer task: 20-min warmup from session unlock,
  then uniformly-random fire within each hour window.
  `AppState::tick_kaos` on a 10s tokio ticker; warmup seeds on
  first tick, subsequent fires pick random 1min–1hr gaps via
  `kaos::random_gap`. Schedule lives on `SessionState` so
  ghost/reclaim preserves it across disconnects.
- [x] 8.2b.3 — Compatibility-aware swap selection: pick a portal
  figure + a compatible replacement from the owning profile's
  collection via `compat::is_compatible` (vehicles SuperChargers-
  only edge case already handled there). `kaos::select_swap` —
  pure fn, 7 unit tests covering placed-by filter, vehicles edge
  case, already-on-portal exclusion, same-figure rejection.
- [x] 8.2b.4 — Execute the swap as a clear+load pair, broadcast
  `Event::KaosTaunt { profile_id, slot, taunt }` with a random
  catchphrase from `data/kaos_taunts.json`. Pairs with 8.1.2 — the
  taunt has to land even if the targeted phone is backgrounded /
  briefly disconnected when it fires.
  `AppState::execute_kaos_swap`: flips portal state to Loading,
  queues ClearSlot + LoadFigure driver jobs, pushes the taunt
  into any matching ghost's replay buffer before broadcasting (so
  a backgrounded phone still sees it on reconnect), then
  broadcasts the `KaosTaunt` event. Taunts inlined in
  `kaos::KAOS_SWAP_TAUNTS` rather than loaded from JSON — simpler
  and ships with the exe.
- [x] 8.2b.5 — Phone `KaosOverlay` component (already exists)
  renders the taunt + visual treatment for ~5s, then dismisses.
  `KaosOverlay` now branches on `takeover.is_some()`: terminal
  takeover UI, or transient swap banner with a 5s auto-dismiss
  timer + tap-to-dismiss-early. Shared surface vocabulary
  (starfield, sigil, quote-card); swap variant drops the info
  line + kickback button. Co-author signal `kaos_swap` threaded
  through `ws::connect` + `App`.
- [x] 8.2b.6 — Tests: timer math, swap selection (vehicles edge
  case), taunt rotation, replay-on-reconnect. 10 tests:
  `random_gap_stays_within_bounds`, `random_gap_rotates_across_*`,
  `taunt_rotation_has_multiple_entries`, plus the 7 selection
  tests. Replay-on-reconnect is covered transitively by 8.1.7's
  `ghost_reclaim_full_roundtrip` (KaosTaunt is variant-agnostic
  in the replay buffer). Kaos toggle round-trip in
  `tests/profiles.rs::kaos_toggle_roundtrips_against_store`.

### 8.3 Hide empty portal spots
PLAY_TEST round 2: kid tried to tap empty portal slots expecting
something to happen. Empty slots are inert (placement happens via
the toy-box lid), so they're a tap-target lie. Goal: hide empty
slots entirely; let the toy-box arrow + hint be the only visible
affordance when nothing is placed; populated slots reappear when a
figure lands and push the arrow hint down.

- [x] 8.3.1 — Wrap each `<SlotView>` in a `<Show when=!is_empty>` so
  Empty-state slots fall out of the DOM. Loading / Loaded / Error
  stay visible (the user has actionable state on each). Grid auto-
  flows the survivors top-to-bottom, left-to-right; original slot
  index badges still render on each visible slot for diagnostic
  honesty.
- [x] 8.3.2 — Drop the `any_empty` gate on `.portal-empty-hint` so
  the toy-box arrow renders unconditionally. Empty portal: hint is
  the only call-to-action (PORTAL heading + hint, nothing in
  between). Populated portal: hint sits below the visible slots,
  pushed down by the grid.

### 8.4 Release 1.1
- [ ] 8.4.1 — Release notes drafted from commits since v1.0.0
  (`generate_release_notes` already wired in `release.yml`).
- [ ] 8.4.2 — Tag → CI release workflow → draft release with the new
  exe → publish.

## Phase 9 Items

### 9.x Tailwind v4 migration (phone CSS rewrite)
Today: ~3000-line monolithic `phone/assets/app.css` with active visual
bugs that have been deferred because every fix cascades into unrelated
breakage. Goal: replace the CSS layer with Tailwind v4 utilities so
the cascade is gone, latent bugs surface (and get fixed) per
component, and future iteration is locally-scoped to whatever element
is being changed. Bounded migration by tranche, with the Phase 8
screenshot tour acting as the per-tranche regression contract.

- [ ] 9.1 — Stand up Tailwind v4 + cached CLI downloader. New
  `tools/tailwind-build/` Rust helper crate: pins `TAILWIND_VERSION`,
  downloads the standalone `tailwindcss` CLI binary into
  `phone/.tailwind-cache/` (gitignored) on first run, reuses on
  subsequent builds, then invokes it with the project's `input.css`
  → `phone/dist/tailwind.css`. Trunk `Trunk.toml` `[[hooks]]
  pre_build` runs `cargo run -p tailwind-build` so `trunk build` /
  `trunk serve` regenerate the bundle automatically. `phone/styles/
  input.css` carries `@import "tailwindcss"` + `@theme {}` mapping
  the existing tokens (gold scale, starfield blues, Titan One /
  Fraunces / JetBrains Mono fonts, k-magenta / k-violet for Kaos)
  into Tailwind's design system. `phone/index.html` swaps its CSS
  link from `app.css` to the generated `tailwind.css`. CI release
  workflow + ci workflow gain `actions/cache` keyed on
  `TAILWIND_VERSION + os` so the binary doesn't re-download every
  run.

- [ ] 9.2 — Pilot on a shared primitive. Pick `GoldBezel` or
  `FramedPanel` (small, used widely; better proof of the migration
  pattern + `@theme` token plumbing than starting on the elaborate
  Kaos overlays). Port its rules from `app.css` to utility classes
  in the `view! {}` macro; for any class string that's becoming
  unmanageable, drop into `@apply` inside a small per-component CSS
  file (`phone/styles/components/<name>.css`) imported from
  `input.css`. Run the screenshot tour, eyeball the diff, commit the
  pilot.

- [ ] 9.3 — Lock the screenshot-tour baseline as the regression
  contract. Document the per-tranche workflow in
  `crates/e2e-tests/README.md`: "rebuild phone bundle → run tour
  → `git diff docs/assets/screens/` → reconcile any visual drift
  before commit." Since the tour drives a real browser at fixed
  420×900 viewport with deterministic seeds, frame-to-frame the PNGs
  should be byte-stable; visible drift means a real regression or an
  intentional design tweak.

- [ ] 9.4 — Migrate components in tranches. Bottom-up so containers
  inherit migrated primitives:
  - 9.4a — Shared primitives: `GoldBezel`, `FramedPanel`, `RayHalo`,
    `ActionButton`, `DisplayHeading`, `Header`.
  - 9.4b — Overlays: `ConnectionLost`, `GameCrashScreen`,
    `PairingRequired`, `StaleVersion`, `ScanOverlay`, `MenuOverlay`,
    `ResumeModal`, `ResetConfirmModal`.
  - 9.4c — Screens: `ProfilePicker` (largest — Konami gate,
    PIN keypad, profile grid), `GamePicker`, `FigureDetail`.
  - 9.4d — Portal + toy box: `Portal`, `ToyBoxLid`, `Browser`. Most
    visually complex; expect `@apply` escape hatches for the lid's
    swipe-state CSS + `:has()` selectors that drive the
    `.screen-portal:has(.lid-open-p4.closed)` cross-component
    coupling. Migration may also be the natural moment to remove
    those `:has()` selectors entirely in favor of explicit
    Leptos-signal-driven classes.
  - 9.4e — Kaos overlays: `KaosOverlay`. Multi-layer pseudo-element
    decoration + conic-gradient lens + custom keyframes; almost
    certainly retains a small per-component CSS file with raw
    `@keyframes` + `@apply`. Acceptable.

  Each tranche: port → trunk build → screenshot tour → diff PNGs →
  commit if intentional.

- [ ] 9.5 — Escape-hatch policy. Document in `CLAUDE.md` when to
  reach for a per-component CSS file vs inline utilities:
  - **Inline utilities (default):** layout, spacing, typography,
    colour, single-layer effects.
  - **`@apply` in a component CSS file:** repeated complex shadow
    stacks where the inline class would exceed readability (rule
    of thumb: >12 utility classes on a single element).
  - **Raw CSS (rare):** `@keyframes`, `@font-face`, complex
    pseudo-element content, `:has()` selectors that can't be
    expressed with utilities. Co-located with the component.

- [ ] 9.6 — Sweep + post-condition. Diff `app.css` before/after.
  Final state: `app.css` slims to design-token `:root` vars +
  `@font-face` declarations + body baseline + the handful of
  component CSS files imported by `input.css`. Rename to
  `phone/styles/base.css` for clarity. Update `CLAUDE.md` Phase 4 +
  4.18 / 4.20 references (most reconciliation residuals fold into
  "use a utility" or "fix the markup"). Re-run full screenshot tour
  + commit any intentional drift.

### 9.7 iPad + iPhone layout pass
Once 9.1–9.6 land, tackle the responsive pass that the monolithic CSS
made painful. With utility-first markup, breakpoint variants
(`md:`, `lg:`) live next to the base utilities and the iPad layout
becomes additive, not a separate stylesheet branch.

- [ ] 9.7 — Optimize for iPad + iPhone layouts. Inventory which
  components need a wider-viewport variant (toy-box grid columns,
  portal slot row layout, Header chip density). Drop `md:` /
  `lg:` overrides per-component; verify on iOS Simulator + a real
  iPad via the Tour gallery.

## Phase 10 Items
- [ ] 10.1 - Add MacOS support
- [ ] 10.2 - Add fully automated e2e testing since we can run it all on a Mac

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
