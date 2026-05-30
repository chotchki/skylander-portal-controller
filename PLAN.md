# Skylander Portal Controller — Execution Plan

Active work toward MVP. Closed work lives in [PLAN_ARCHIVE.md](PLAN_ARCHIVE.md) — Phases 0–3 plus most of Phase 4 + parts of Phase 6.

Conventions:
- `[ ]` pending, `[x]` done, `[~]` in progress, `[?]` blocked / needs discussion.
- New tasks should always be numbered and have a checkbox so they're traceable.
- Don't skip a review checkpoint; the point is to re-plan with new information.

---

### 4.18 Phone UI drift reconciliation (residuals)
Tags: **[bug]** wrong behavior, **[feature]** missing capability, **[judgment]** mock is one opinion shipped is another, **[verify]** may already be done.

- [-] 4.18.1 **Mobile viewport / address-bar.** 100dvh + 100svh + safe-area-inset landed; PWA install is the workable path. Follow-up 4.18.1c open.
- [ ] 4.18.1c **Service worker for PWA cache + update detection.** Today static assets return `Cache-Control: no-store`. Add `phone/assets/sw.js`: hashed wasm/js/css/font immutable, `index.html` + manifest `no-cache`, delete stale cache entries on activation, post "new version" message to running SPA. Only mechanism that survives iOS PWA app-shell caching across long backgrounding.
- [-] 4.18.5c **Menu overlay → Konami-gate transition.** Bug half-done (empty-chip hide, commit `439e0d4`). Judgment open: gate-rise + entry-cascade vs plain cross-fade.
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

## Phase 5 - Kaos

Kaos is LAST among feature work. Do not start without explicit go-ahead.

---
- [ ] 5.1 Wall-clock timer: 20min warmup + randomized 60min windows.
- [ ] 5.2 Text-only overlay with Kaos catchphrases (curated in-repo list; text avoids audio copyright). Two surfaces mocked in Phase 4: `kaos_takeover.html` + `kaos_swap.html`. **No auto-dismiss.** Multiple fires while asleep: latest-wins or queue (decide during impl).
- [ ] 5.3 1-for-1 swap of a portal figure with a random compatible-with-current-game figure.
- [ ] 5.4 Purple/pink Kaos skin via CSS variable swap (rides on Phase 4's `--*` tokens; palette swap, not rewrite).
- [ ] 5.5 Parent kill-switch (SPEC Q38) — hidden config knob, not in the phone UI.
- [ ] 5.6 Kaos swap goes through the standard driver flow.


## Phase 6 - Post-Kaos polish (residuals)

### 6.2 Parse `.sky` firmware for per-figure stats (partial)
Encryption handled (6.2.0 + 6.2.0b archived). Identity fields decode correctly; payload fields decode post-decryption. 141/151 CRC-valid on real dumps.
### 6.3 Detailed-stats screen on the phone
### 6.4 Demo harness for screen recording
---
- [ ] 6.1 **Suppress RPCS3 window flicker during menu navigation.** Launcher starts before RPCS3 → establish Z-order priority. Ideas: (a) launcher `WS_EX_TOPMOST` during `open_dialog` nav so Qt popups render behind, (b) `SetWinEventHook` / `EVENT_OBJECT_SHOW` filtered to RPCS3 PID to intercept dialog creation and move off-screen before first paint, (c) hook menu popups the same way.


- [-] 6.2 parent `[~]` until per-kind coverage lands + all 22 tests have ciphertext-fixture counterparts.
- [ ] 6.2.1 **UI determination pass for Trap / Vehicle / CYOS.** Mock reduced Figure Detail variants before parser work — fields we don't render aren't worth decoding. Targets: Trap → captured villain name + portrait headline, Vehicle → SSCR level headline + adornment names, CYOS → class + element + nickname with missing-field tolerance. Racing Pack keeps default "STATS COMING SOON" strip.
- [ ] 6.2.3 **Trap payload: captured villain identity.** Parse villain cache per `docs/research/sky-format/SkylanderFormat.md` line 39ff. `SkyFigureStats.trap: Option<TrapData>`. `data/villains.json` lookup for display name + portrait. ~3 tests.
- [ ] 6.2.4 **Vehicle payload: SSCR level.** Parse XP + level derivation. `SkyFigureStats.vehicle: Option<VehicleData>` + `data/vehicle_adornments.json`. Skip gearbits/flags/mod-flags this pass. ~3 tests.
- [ ] 6.2.5 **CYOS payload: class / element / nickname best-effort.** Parse deterministic bits + attempt nickname recovery from the 0x65-byte payload. Return `Option<CyosData>` with *individually-optional fields*. Emit structured warning log on CRC-pass-but-field-mismatch.
- [ ] 6.2.6 **Wire per-kind payloads through `/api/profiles/:profile_id/figures/:figure_id/stats` + `phone/src/screens/figure_detail.rs`.** JSON response gains nested `trap` / `vehicle` / `cyos` (null for Racing Pack). Uses existing `sky-stats` feature flag.
- [ ] 6.2.8 **Investigate the 10 CRC-failing samples from 6.2.0b.** Trap Team Adventure Packs (fids 0x131–0x134), Imaginators Senseis/Creation-Crystal era (King Pen, Wild Storm, Crash Bandicoot, Dr. Neo Cortex, Air Strike, Sheep Creep 0xC82). Hypotheses: (a) different CRC scope, (b) extended Sensei data layout, (c) factory-blank never-played figures. Chris to load in emulator + compare observed vs parser output.
- [ ] 6.2.9 **Pin Vehicle + CYOS `figure_id` ranges against real dumps.** 6.2.2 left these ranges commented out — community values missed every real sample. Observed vehicle-looking IDs cluster near `0x0C9x..=0x0CAx` (overlap with SuperChargers characters). First live CYOS data point: creation crystal fid=`0x0002AD`. Blocks 6.2.4 / 6.2.5. Deliverables: (a) more SuperChargers vehicle + CYOS dumps via 6.5.0's scan tool; (b) extend `FigureKind` range table with real-observed fids + tests; (c) smoke-test `decode_nickname` against new CYOS samples.

- [ ] 6.3 Level + XP, gold, current hat, playtime, nickname, hero points, hat history, trinket, quest progress. Hits stats endpoint; read-only. Non-standard layouts render reduced panel until 6.2 stubs fill. Placeholder today: `.detail-stats-soon` strip in `phone/assets/app.css`; when this ships, delete `.detail-stats-soon*` + soon-label span and reinstate the three `.detail-stat-cell` blocks wired to fetched stats.

- [ ] 6.4 Browser-viewable test session driving the phone SPA through a representative flow (profile → PIN → game → portal → toy box → place → Kaos swap). Runs side-by-side with remote-desktop HTPC view for single-frame recording.


## Phase 8 - Items

### 8.1 Ghost sessions (sticky disconnect)
Today: WS drop → server's `disconnect cleanup` clears the departing
profile's portal slots immediately. PWA backgrounding triggers it
constantly. Goal: keep a phone's figures on the portal across a
disconnect, replay missed events on reconnect, and only evict when a
new phone genuinely takes over the slot.

### 8.2a Kickback cooldown countdown UI
Today: kickback button is enabled immediately on the Kaos takeover
screen; server returns 401-RetryAfter if the 60s cooldown hasn't
elapsed. Should grey-out + count down instead.
### 8.2b Kaos feature (CLAUDE.md "Kaos feature")
The Skylanders-themed mid-game disruption — wall-clock timer fires,
a portal figure gets swapped for a random compatible one from the
owner's collection, a Kaos catchphrase overlays for ~5s.
### 8.3 Hide empty portal spots
PLAY_TEST round 2: kid tried to tap empty portal slots expecting
something to happen. Empty slots are inert (placement happens via
the toy-box lid), so they're a tap-target lie. Goal: hide empty
slots entirely; let the toy-box arrow + hint be the only visible
affordance when nothing is placed; populated slots reappear when a
figure lands and push the arrow hint down.
### 8.4 Release 1.1
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

- [ ] 8.4.1 — Release notes drafted from commits since v1.0.0
  (`generate_release_notes` already wired in `release.yml`).
- [ ] 8.4.2 — Tag → CI release workflow → draft release with the new
  exe → publish.

## Phase 9 - Items

### 9.x Tailwind v4 migration (phone CSS rewrite)
Today: ~3000-line monolithic `phone/assets/app.css` with active visual
bugs that have been deferred because every fix cascades into unrelated
breakage. Goal: replace the CSS layer with Tailwind v4 utilities so
the cascade is gone, latent bugs surface (and get fixed) per
component, and future iteration is locally-scoped to whatever element
is being changed. Bounded migration by tranche, with the Phase 8
screenshot tour acting as the per-tranche regression contract.

### 9.7 iPad + iPhone layout pass
Now that 9.1–9.6 have landed plus Phase 10 (Mac server, multi-device
`tools/ios-inspect/`, iOS-Simulator e2e), tackle the responsive pass
that the monolithic CSS made painful. With utility-first markup,
breakpoint variants (`md:`, `lg:`) live next to the base utilities
and the iPad layout becomes additive, not a separate stylesheet
branch.
Driver: 2026-05-04 son-playtest on a real iPad surfaced text-too-small
across every screen. Phone-first px-pinned typography tokens
(`--text-display-*` / `--text-body-*` in `phone/styles/input.css`)
don't scale on iPad's wider viewport.
### 9.8 Collapse + standardize repeated patterns
Once every component lives in its own file (9.4) and responsive
variants are in (9.7), the per-tranche escape-hatch CSS will have
accumulated duplicate declarations across files — same multi-layer
shadow stacks, same gradient stops, same hex literals that should
have been tokens, same text-shadow combinations on the heraldic
text treatment. The migration emphasised *identity preservation*
(class names + visuals stay byte-stable), so consolidation was
deliberately deferred. 9.8 is the after-the-fact refactor pass that
surfaces and unifies those duplicates so the design system has one
source of truth per pattern.
**Sequenced after 9.7** so the consolidation pass sees the full set
of patterns (including responsive variants) at once.
- [x] 9.1 — Stand up Tailwind v4 + cached CLI downloader. New
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

- [x] 9.2 — Pilot on a shared primitive. Pick `GoldBezel` or
  `FramedPanel` (small, used widely; better proof of the migration
  pattern + `@theme` token plumbing than starting on the elaborate
  Kaos overlays). Port its rules from `app.css` to utility classes
  in the `view! {}` macro; for any class string that's becoming
  unmanageable, drop into `@apply` inside a small per-component CSS
  file (`phone/styles/components/<name>.css`) imported from
  `input.css`. Run the screenshot tour, eyeball the diff, commit the
  pilot.

- [x] 9.3 — Lock the screenshot-tour baseline as the regression
  contract. Document the per-tranche workflow in
  `crates/e2e-tests/README.md`: "rebuild phone bundle → run tour
  → `git diff docs/assets/screens/` → reconcile any visual drift
  before commit." Since the tour drives a real browser at fixed
  420×900 viewport with deterministic seeds, frame-to-frame the PNGs
  should be byte-stable; visible drift means a real regression or an
  intentional design tweak.

- [x] 9.4 — Migrate components in tranches. Bottom-up so containers
  inherit migrated primitives:
  - [x] 9.4a — Shared primitives: `GoldBezel`, `FramedPanel`, `RayHalo`,
    `ActionButton`, `DisplayHeading`, `Header`.
  - [x] 9.4b — Overlays: `ConnectionLost`, `GameCrashScreen`,
    `PairingRequired`, `StaleVersion`, `ScanOverlay`, `MenuOverlay`,
    `ResumeModal`, `ResetConfirmModal`.
  - [x] 9.4c — Screens: `ProfilePicker` (largest — Konami gate,
    PIN keypad, profile grid), `GamePicker`, `FigureDetail`.
  - [x] 9.4d — Portal + toy box: `Portal`, `ToyBoxLid`, `Browser`. Most
    visually complex; expect `@apply` escape hatches for the lid's
    swipe-state CSS + `:has()` selectors that drive the
    `.screen-portal:has(.lid-open-p4.closed)` cross-component
    coupling. Migration may also be the natural moment to remove
    those `:has()` selectors entirely in favor of explicit
    Leptos-signal-driven classes.
    (FigureHero rode along — it should have shipped in 9.4a but
    slipped through. The `:has()` coupling stayed as raw CSS per
    9.5 escape-hatch policy; lifting `box_state` up to
    `.screen-portal` is on the table for 9.8 / a future refactor.)
  - [x] 9.4e — Kaos overlays: `KaosOverlay`. Multi-layer pseudo-element
    decoration + conic-gradient lens + custom keyframes; almost
    certainly retains a small per-component CSS file with raw
    `@keyframes` + `@apply`. Acceptable.
    (Result: ~280 lines, seven keyframes, mostly raw CSS as
    expected. Six-layer composition — void / hexgrid / sparks /
    vignette / viewport / sigil — kept identical to source.)

  Each tranche: port → trunk build → screenshot tour → diff PNGs →
  commit if intentional.

- [x] 9.5 — Escape-hatch policy. Document in `CLAUDE.md` when to
  reach for a per-component CSS file vs inline utilities:
  - **Inline utilities (default):** layout, spacing, typography,
    colour, single-layer effects.
  - **`@apply` in a component CSS file:** repeated complex shadow
    stacks where the inline class would exceed readability (rule
    of thumb: >12 utility classes on a single element).
  - **Raw CSS (rare):** `@keyframes`, `@font-face`, complex
    pseudo-element content, `:has()` selectors that can't be
    expressed with utilities. Co-located with the component.

- [x] 9.6 — Sweep + post-condition. Diff `app.css` before/after.
  Final state: `app.css` slims to design-token `:root` vars +
  `@font-face` declarations + body baseline + the handful of
  component CSS files imported by `input.css`. Rename to
  `phone/styles/base.css` for clarity. Update `CLAUDE.md` Phase 4 +
  4.18 / 4.20 references (most reconciliation residuals fold into
  "use a utility" or "fix the markup"). Re-run full screenshot tour
  + commit any intentional drift.



- [x] 9.7 — Optimize for iPad + iPhone layouts. Whole-session playtest
  pass 2026-05-04: rem-based typography + tablet root-size bump,
  Tailwind cascade-layer fix (button rule moved to `@layer base` so
  component overrides win), opacity-only screen entrance animation
  (kills the residual `transform` that pinned `.btn-back` to
  `.screen` instead of the viewport), `.btn-back` top aligned to
  `.app` padding-top, mock-driver `/api/launch` skips games.yml,
  iPad lid 2× max-heights + bottom-bleed code scoped to `.closed`
  only (was cutting off the top of the figure cards), search input
  swapped inline with the SEARCH toggle when expanded, drill chips
  rem-scaled, element-tinted bezel rings (scoped to figure-cards +
  portal slots, not profile bezels), TRAP badge, vehicle terrain
  badges (LAND/SKY/SEA from `data/vehicle_terrain.json`), white
  text everywhere instead of dark-on-dark, variant-count badge
  rem-scaled, kebab Kaos toggle relocated to AdminEdit, profile-
  management cards stack on iPhone, PIN reset is single-step with
  unlock-style layout + auto-fire, profile delete restyled to the
  destructive ActionButton vocabulary + drops PIN check (Konami is
  the gate), profile-bezel coloured on PIN entry, closed-lid chevron
  ~50% lid height, Kaos overlay sigil mask asset (was 404'ing through
  SPA fallback), Kaos title rem + tablet bump, swap variant centred
  with `mx-auto` and now hold-to-dismiss with bordered pill instead
  of tap-to-dismiss + 5s auto-clear.
- [x] 9.7.2 — *[bug]* iPad typography too small (playtest 2026-05-04).
  Folded into the 9.7 sweep above.
- [x] 9.7.1 — *[bug]* `PwaHint` suppressed on iPad. `pwa.rs` gains
  `is_tablet_ua` (pure: UA contains "ipad" OR macintosh + touch-mac
  probe) + an `is_tablet()` web_sys wrapper; `should_show_hint`
  takes the new gate as its third arg. 5 new unit tests cover
  (iPad/iPad-Pro-as-Mac/iPhone/real-Mac/Android) + the truth-table
  case for tablet=true → false. PwaHint component reads the new
  signal on mount. End-to-end verified by
  `crates/e2e-tests/tests/ios_pwa_hint.rs` — boots iPhone + iPad
  sims, opens the SPA, asserts `.pwa-hint` count == 1 on iPhone +
  == 0 on iPad. First load-bearing demonstration that the iOS-sim
  e2e lane (PLAN 10.4) catches and pins real product bugs.



- [ ] 9.8 — Sweep the `phone/styles/components/*.css` files for
  repeated patterns and consolidate. Three target shapes, in order
  of preference:
  1. **`@theme` token** — add to `phone/styles/input.css`'s `@theme`
     block when the value is a design constant (a colour, a font, a
     duration, a gradient stop set). Example candidates: the
     "blue-card" gradient `linear-gradient(180deg, #2f6edc 0%,
     #1e4fb3 55%, #153a8a 100%)` (currently inlined in
     `action_button.css`; will recur in MenuOverlay), the "gold
     radial" `radial-gradient(circle at 30% 25%, var(--gold-bright)
     0%, var(--gold) 40%, var(--gold-mid) 80%)` (in `gold_bezel.css`
     ring + `action_button.css` icon), the dark-stroked heraldic
     text-shadow stack from `display_heading.css`. Promote to
     `--gradient-blue-card`, `--gradient-gold-radial`, etc.
  2. **`@utility`** — Tailwind v4's mechanism for a custom utility
     that composes other utilities. Use when a pattern is utility-
     shaped (single property or a tight cluster) and gets applied
     in markup. Example: the "raised button" elevation shadow
     (`inset 0 2px 0 rgba(...)`, `inset 0 -2px 0 ...`, `0 3px 0 ...`,
     `0 5px 12px ...`) appears on `.menu-action` and probably on
     ResetConfirmModal's HOLD button; lift to `@utility shadow-raised
     { box-shadow: ... }` so callers get `class="shadow-raised"`.
  3. **Shared `@layer components` class** — last resort for
     multi-property patterns that cross several declarations and
     can't be expressed as a utility. Co-locate in a new
     `phone/styles/components/_shared.css` (underscore prefix flags
     "not a component, a kit").

  Procedure per consolidation:
  - Identify the duplicate (`grep -F` against component CSS files).
  - Extract to whichever shape fits.
  - Update the component files to reference the new token /
     utility / shared class.
  - Run `trunk build` + screenshot tour; reconcile any drift before
     committing per the 9.3 contract. Most consolidations should be
     pixel-identical because the underlying declaration is the same.

  Out of scope: chasing single-occurrence values into tokens (`--color-foo`
  used once isn't a token, it's an indirection). The bar is "this
  pattern shows up in ≥2 component files" or "this hex literal is
  brand-meaningful and should be discoverable in `@theme`."

## Phase 10 - Items

Goal: bring macOS up as a first-class platform — production-ready
server binary plus the multi-device iPad+iPhone simulator orchestration
that makes the 2-phone product feature exercisable end-to-end without a
Windows machine. Independent of Phase 9 — the Tailwind port is the
visual baseline whether or not 9.1–9.6 have landed when 10.x starts.

The UIA driver remains Windows-only; on macOS the mock driver is the
only available `DriverKind`. That's a real product limitation (no Mac
RPCS3 integration), not a build-only gap — Mac users get a working
portal-controller against an in-memory mock instead of a live emulator,
which is useful for demo, family-member play, and dev iteration but
isn't the full Skylanders flow. AXUIElement-based Mac driver work is
out of scope for Phase 10.

Deliverables, in dependency order:
- 10.1 — server compiles + runs cleanly on macOS under the mock driver.
- 10.2 — `ios-inspect` boots and drives two simulators at once.
- 10.3 — existing chromedriver e2e suite goes green on macOS.
- 10.4 — new simulator-driven tests exercise iPad + iPhone simultaneously.
- 10.5 — CI lane on a Mac runner so the suite stays green automatically.
- 10.6 — macOS release artifact published alongside the Windows zip.
- 10.7 — replace the launcher's 2D "badge spin" with a real 3D rotating
  disc rendered through the existing glow / egui_glow plumbing
  (visual polish, independent of the macOS work).

### 10.1 Server builds and runs on macOS (mock-driver only)
Today: workspace mostly cfg-gates Windows-only code paths, but
`cargo check -p skylander-server --no-default-features --features dev-tools`
on macOS fails with `bail!` not in scope inside the non-Windows arm of
`RpcsProcess::launch_library` / `attach`. Beyond the immediate fix, the
cross-platform scaffold needs a smoke-pass before the rest of Phase 10
has anything to stand on.

### 10.2 `ios-inspect` multi-device orchestration
Today `ios-inspect boot` auto-picks the newest Dynamic-Island iPhone
and `state.json` tracks one device + one webinspectord_sim socket. The
2-phone product feature (PLAN 8.1, 8.2a) needs an iPad and an iPhone
booted simultaneously, both pointed at the same server, each driveable
independently. `xcrun simctl` and `ios-webkit-debug-proxy` both support
multiple simulators concurrently — the limitation is purely in
`ios-inspect`'s state model.
### 10.3 E2E harness portable to macOS
The fantoccini suite in `crates/e2e-tests/` already exercises the mock
driver — which is the only driver available on Mac — but is hardcoded
for Windows in two places: the firmware-pack default path and the
chromedriver discovery fallback. Fix those, then drive the existing
suite green on macOS.
### 10.4 Simultaneous iPad + iPhone simulator e2e
This is the core user value: drive both an iPad and an iPhone
simulator against a single server instance, exactly the way the
2-phone feature ships. Combines 10.2's multi-device `ios-inspect`
with 10.3's portable harness.
### 10.5 Continuous-integration lane
CI was deferred until the app worked. With 10.1–10.4 in place, the
e2e suite has a credible automated home and the "fully automated"
half of the user goal becomes real. Decide CI host first since it
shapes everything downstream.
### 10.6 macOS production release artifact
Today the release pipeline produces a Windows zip (`release.yml` runs
`generate_release_notes` and uploads `skylander-portal-controller.exe`).
Mac becomes a parallel artifact: same binary, mock-only driver, same
GitHub Releases attachment shape. No `.app` bundle in the first cut —
a CLI binary in a tar.gz mirrors the Windows zip's friction level and
keeps the release pipeline simple. `.app` + signing + notarization are
deferred unless a real user reports the bare-binary UX as blocking.
### 10.7 Real 3D rotating badge for the launcher intro
Today the launcher's centre badge (QR card / error card / brand
intro) "spins in" via a 2D horizontal-scale hack — `LaunchPhase
::badge_scale` returns `sin(progress * π/2)`, the renderer
applies it as `rect.shrink_x(...)`, and an alpha-gate hides the
line-shaped phase so the user doesn't see a vertical line slide
across the screen. End result: a flat circle that gets thin and
fat, with no honest depth, lighting, or perspective. Chris
2026-05-02 watching it during a long CI wait: "killing me with
how awful it looks." It's not a regression — it's been like this
since the Phase 4 launcher polish — but with the rest of the
launcher choreography now solid, the badge is the conspicuous
weak point.
Goal: replace the 2D scale trick with an actual 3D disc rendered
via the existing glow / egui_glow plumbing (PLAN 4.19.6 promoted
that stack from spike to runtime for the vortex backdrop). The
disc is a textured cylinder cap — front face textured with the
QR + brand text, edge has visible thickness, lit so rotation
reads as physical motion rather than a CSS animation. Rotation is
y-axis around the disc's vertical centre, driven from the same
`LaunchPhase` state machine so the new motion plugs into the
existing intro / closing transitions without ripple.
Independent of 10.6.3 (.app bundle) — purely a quality-of-launch-
experience improvement. Sequencing-wise can land before 10.6.3
or after, doesn't matter.
### 10.8 v1.2 HTPC field-test bugs
First non-dev install pass on the living-room HTPC (2026-05-03,
v1.2.0 zip). Binary launches and the phone reaches the QR, but a
cluster of distribution / Windows-integration rough edges showed up
that don't surface on the dev box. Triage individually — most are
independent.
### 10.8.7 Game-launch state machine + cover-before-kill (sequel to 10.8.4)
10.8.4 closed the keystroke-fragility bug at the driver layer (UIA
pattern menu nav) and rewired the server flow to direct-boot. Field
testing v1.3.0 → v1.3.4 surfaced a sequence of UI-layer issues that
aren't separate bugs but symptoms of an implicit, ad-hoc state
machine for game-launch lifecycle:
- **v1.3.3** "iris jumped open before shaders compile" — the
- **v1.3.4** "screen black during launch" — premature transparent
- **v1.3.4** "launcher hung on /api/shutdown" — the in-game branch
- The proposed graceful-quit flow has a race: kill RPCS3 →
Decision (Chris 2026-05-04): replace the implicit launch state
machine with explicit, contract-tested states. Ground the "playable"
signal in RPCS3's own per-frame FPS counter (which it embeds in the
viewport title), not log tails. Add a cover-before-kill mechanic for
graceful exits so the launcher's opaque cover is up before RPCS3
dies. **Strict invariant: the only legitimate black render anywhere
in the app is the Farewell black-fade overlay.** Any other "screen
goes black" symptom is a bug.
States + render contract:
| State | Launcher render | Iris radius |
|---|---|---|
| `Idle` | Main, AwaitingConnect (QR front) | open, steady |
| `Spawning` / `Booting` / `Compiling` | Main, AwaitingConnect (LOADING back-face) | open, steady |
| `Playable` | in-game (transparent CentralPanel) | 0 (closed, conceptual — no launcher visible) |
| `QuitCovering` | Main, **`BackFace::Returning`** ("RETURNING TO PORTAL") | **0→open** (ReturnFromGame animation) |
| `SwitchCovering` | Main, `BackFace::Switching` ("SWITCHING GAMES") | **0→open**, holds open until next launch closes back to 0 |
| `Crashed` | Crashed screen, message + RESTART | **0→open** (uses `screen_intro` on crash-from-game, already wired) |
| `ShuttingDown` | Farewell screen, GOODBYE back-face, then black-fade overlay | **0→open**, then fade (the only black) |
Cover-before-kill mechanic: server sets `cover_active = true`,
sleeps ~200 ms (≥1 tick of the in-game branch's 4 Hz heartbeat,
giving the launcher time to render the cover), then proceeds with
the kill. RPCS3 dies *behind* the opaque cover; user never sees a
flash of desktop or a black panel.
Pre-req tooling already shipped:
- `tools/build-msi.sh` — local Windows MSI build for HTPC
- `release.yml` auto-publishes as prerelease so MSI is
- Direct-boot driver layer (commits in the 10.8.4 chain
### 10.9 Real installers (.msi / .dmg)
v1.2 + 10.8.6's data-bundle fix mean the release artifact is no
longer a single exe — it's a folder containing the binary, ~20 MB
of tracked figure / box-art assets, and (after 10.8.5) a `steam/`
artwork subdir. "Unzip and run" doesn't scale with that shape;
real installer packaging is the next step. Pairs with 10.8.1 +
10.8.2 (a signed installer can carry SmartScreen reputation +
register the inbound firewall rule from the same UAC prompt) and
subsumes 10.6.3 (Mac `.app`) as the delivery wrapper rather than
the artifact itself.
Reshapes 10.8.5 too: with an installer carrying a proper Start
Menu shortcut + embedded icon, only the Steam-Grid artwork
(capsule / hero / logo) needs to ship as loose files for users to
hand-import. The `.ico` half of 10.8.5 collapses into 10.9.3.
- [x] 10.1.1 — Imported `anyhow::bail` in `crates/rpcs3-control/src/lib.rs`
  for the non-Windows arms. `cargo check -p skylander-server
  --no-default-features --features dev-tools` is clean on macOS.
- [x] 10.1.2 — `cargo check --workspace` is clean. The 9
  `crates/rpcs3-control/examples/*` files are `#![cfg(windows)]` (file
  body vanishes on Mac → "no main function" under `--all-targets`).
  Fixed by listing each example in `crates/rpcs3-control/Cargo.toml`
  with `required-features = ["uia-examples"]`; Mac skips them by
  default, Windows contributors opt in with the flag.
- [x] 10.1.3 — Confirmed: `DriverKind::Mock` short-circuits both
  `build_driver` and the spawn task before either touches `RPCS3_EXE`,
  so the dev sentinel path never gets opened. The eframe `wayland`/`x11`
  features in `crates/server/Cargo.toml` turned out to be inert on Mac
  (eframe wires them only on `cfg(target_os = "linux")` internally), so
  no per-target gating needed there either.
- [x] 10.1.4 — pcsc + skylander-nfc-reader build cleanly on Mac via
  `cargo check -p skylander-server --features nfc-import` — pcsc-lite's
  `build.rs` finds the system PCSC framework via pkg-config without
  intervention. No code changes needed.
- [x] 10.1.5 — End-to-end smoke run captured. `cargo run -p
  skylander-server` indexes 504 figures, the SPA serves at
  `http://<en0-ip>:8765/`, `/api/join-qr.png` returns a 37 KB PNG.
  Bringup doc: `docs/dev/macos-bringup.md` (excluded from the Jekyll
  site via `docs/_config.yml`).
- [x] 10.1.6 — Added `crates/server/src/mdns/mac.rs` reading
  `scutil --get LocalHostName` and wired it into
  `mdns::os_dns_hostname` via `#[cfg(target_os = "macos")]`. Verified
  end-to-end: server logs `phone URL http://christophers-macbook-pro.local:8765/?k=…`
  and `curl` against that URL returns 200. The ignored
  `os_hostname_resolves_via_local` test passes on this Mac.
- [x] 10.1.7 — Added a "macOS dev workflow" section to `CLAUDE.md`
  pointing at `docs/dev/macos-bringup.md` + `tools/ios-inspect/README.md`.
  Neither `CLAUDE.md` nor `SPEC.md` had a "No Linux/Mac support"
  non-goal in the first place — only PLAN.md did, and that was
  flipped in the same commit as the Phase 10 expansion.


- [x] 10.2.1 — `ios-inspect boot --device <name>` is now repeatable; UDID
  list persists in `/tmp/ios-inspect-state.json`. Re-running boot
  against an existing session adds new devices without re-spawning
  proxies for already-booted ones. Pre-10.2 single-device JSON is
  auto-migrated on read.
- [x] 10.2.2 — `--device <name|udid>` added to every read/eval command;
  no-filter fans out to all booted devices and prefixes each line with
  the device label (`[iphone-17-pro]`, `[ipad-pro-13-inch-m5]`).
  Verified end-to-end with `eval`, `computed-style`, `tabs`.
- [x] 10.2.3 — Restructured `proxy.rs` for multi-device. Each device
  gets its own proxy on a deterministic port pair (`9221+2N` control,
  `+1` device); state pins both ports so commands skip the HTML re-
  parse. Boot-time socket attribution via diff (`find_live_sim_sockets`
  before/after `simctl boot`). The proxy uses `-c "null:<ctrl>,:<dev>"`
  config CSV — earlier `-p` attempt was the wrong flag (silent crash).
- [x] 10.2.4 — `screenshot --output <dir>` with multiple booted devices
  writes one PNG per device using `<label>.png`. Single-device case
  still takes a file path. Verified: iPhone 17 Pro produced 402×714,
  iPad Pro 13" produced 1032×1290 in one command.
- [x] 10.2.5 — `shutdown` iterates every device in state, killing each
  proxy + shutting each sim. Idempotent. Stale state entries (proxy
  PID dead) get reaped at next `boot` so they don't pile up.
- [x] 10.2.6 — `tools/ios-inspect/README.md` rewritten with a
  single-device "legacy default" section + a multi-device walkthrough.
  Caveats list updated: same-name-across-runtimes ambiguity called
  out as a known wart (substring match picks first by HashMap order).


- [ ] 10.3.1 — `crates/e2e-tests/src/lib.rs::TestServer::spawn` +
  `spawn_live`: replace the hard-coded
  `C:\Users\chris\workspace\Skylanders Characters Pack for RPCS3`
  default with a per-target default (Mac/Linux:
  `<repo>/dev-data/firmware-pack/`). `SKYLANDER_PACK_ROOT` override
  stays as the escape hatch.
- [ ] 10.3.2 — `locate_chromedriver`: add macOS branches checking
  `/opt/homebrew/bin/chromedriver`, `/usr/local/bin/chromedriver`, then
  `which chromedriver`. Document the install
  (`brew install --cask chromedriver`) in
  `crates/e2e-tests/README.md`. Windows-only winget fallback stays
  cfg-gated.
- [-] 10.3.3 — Harness end-to-end works on Mac under the matched
  ChromeDriver (`smoke.rs` passes after a one-line selector update).
  Server boot, Bonjour URL scrape, dev-data pack auto-resolve all
  green. Real triage finding: the rest of the chromedriver suite has
  significant DOM drift (assertions still target pre-Phase-4 selectors
  like `.game-picker h2` that were replaced by the `DisplayHeading`
  component, and headless Chrome's `getElementText` returns "" for
  text styled with `-webkit-text-stroke` even when visually present).
  Concrete failures observed: `multi_phone.rs` 5/6 fail (DOM drift in
  2-phone scenarios). Pulled out as **10.3.6** so 10.3.3 itself
  closes on the harness fitness; the SPA-vs-test reconciliation gets
  its own tracked workstream.
- [x] 10.3.4 — Decision: **re-baseline on Mac.** All 10 tour PNGs
  in `docs/assets/screens/` were regenerated from the macOS
  CoreText-rendered tour run (file sizes ~10–25 % bigger than
  the previous Windows DirectWrite originals; visually clean +
  expected). New baseline committed alongside 10.3.6e. Mac is the
  primary contributor platform now (Phase 10), so re-baselining
  there is straightforward. The README screenshot-baseline note
  already calls out the per-OS gotcha — anyone editing a screen
  regenerates on the same OS that committed the baseline. Folded
  into 10.3.6e's commit.
- [x] 10.3.5 — `crates/e2e-tests/README.md` rewritten with macOS
  install commands (`brew install --cask chromedriver`), version-
  mismatch troubleshooting (Chrome ↔ ChromeDriver major-version
  pin), per-OS screenshot baseline note (CoreText vs DirectWrite).
- [-] 10.3.6 — **Reconcile chromedriver suite with current SPA.**
  Foundational work done across two rounds; per-test flow rewrites
  remain (multi-PR workstream — see 10.3.6a–10.3.6e below for the
  per-file bites).

  **Round 1 (committed):**
  - `Phone` helpers modernized for Phase-4 selectors
    (`.portal-p4`, `.fig-card-p4:not(.scan-new)`, `.search-input-p4`,
    etc.) and a new `Phone::inner_text(selector)` helper that reads
    via `element.innerText` JS so `-webkit-text-stroke` headings
    (DisplayHeading family) actually return their rendered text —
    WebDriver's `getElementText` returns "" for those.
  - `regressions.rs` + `working_copies.rs` direct-selector tokens
    swept; slot-label string constants moved to Phase-4 lowercase
    (`"Empty"` → `"empty"`, `"Loading…"` → `"…"`).
  - Smoke verified still green.

  **Round 2 (committed):**
  - Process-group cleanup landed on `ChildGuard`: the spawned
    `cargo run`-server child and the chromedriver child are both
    now spawned with `Command::process_group(0)` (Unix), and
    `kill_now`/`Drop` send SIGKILL to the entire group via
    `kill -KILL -<pgid>`. A panicked test no longer leaves dozens
    of orphaned `Google Chrome Helper` processes — verified by
    deliberately running a panicking test and observing zero
    residual `enable-automation.*webdriver` Chromes. Side-leak:
    Chrome's user-data scoped temp dirs survive SIGKILL (Chrome
    can't run cleanup hooks under SIGKILL); they're filesystem-only
    and the OS reaps `/var/folders` eventually.
  - `connection_lost.rs` ported to the new `inner_text` helper —
    test passes (~22 s).
  - CI lane (`e2e-mock-macos`) expanded to run `connection_lost`,
    `hmac`, and `shutdown` alongside `smoke`. The hmac + shutdown
    tests don't use chromedriver (raw HTTP against the spawned
    server) but were never wired into CI before; rolling them in
    now adds a bit of HTTP-API coverage essentially for free.

  **Why the rest is multi-PR — the SPA's *flow* changed too, not
  just selectors:**
  - PLAY_TEST PLAN 8.3 made empty portal slots inert
    (`<Show when=!is_empty>`). Tests that did "tap empty slot → see
    figure picker" no longer have anything to tap. Placement is
    now via the toy-box lid only.
  - Phase-4 hides `.search-input-p4` behind the toy-box lid +
    a `.search-toggle-p4` toggle. `phone.search()` from the portal
    screen no longer finds an input.
  - Profile picker moved from form-based create to wizard + PIN-pad
    UX. Selectors AND interaction sequence both changed
    (`profiles.rs::profile_create_and_unlock_lands_on_game_picker`
    is the worst-affected file).

- [x] 10.3.6a — **regressions.rs (6 tests).** All green locally
  (~34 s for the file). Adopted the new `Phone` helpers
  (`open_toy_box_lid`, `place_first_figure`, `remove_slot`,
  `wait_for_slot_empty`) and rewrote each test for the Phase-4
  placement model. Notable per-test adaptations: `dup_figure_across_slots`
  asserts the on-portal ribbon + slot-2 stays out of DOM (auto-pick
  doesn't apply to already-placed figures); `clear_then_load_sequence`
  needed the `wait_for_slot_empty` helper since removed slots
  vanish; `on_portal_figures_disabled` switched from "place button
  is disabled" to "clicking place surfaces `.detail-error-banner`"
  (current SPA shows an error banner rather than disabling the
  button — the `already` short-circuit lives in
  `figure_detail.rs::on_place`). Not yet wired into CI — see
  10.3.6f below.
- [-] 10.3.6b — **working_copies.rs (2 tests).** 1/2 green:
  `load_uses_canonical_name_not_filename` passes (~19 s). The
  `resume_prompt_offers_prior_layout` test is `#[ignore]`-ed with
  a WIP note — the resume modal isn't appearing after
  `location.reload()` + re-unlock; either the persist_layout
  write is racing the reload (500 ms wait may not be enough
  post-Phase-4 SlotChanged → save chain) or the post-reload phone
  never re-handshakes the HMAC key cleanly. Needs deeper
  investigation against the live emit path.
- [x] 10.3.6c — All 6 multi_phone tests green (~45 s for the file
  serial). `forced_eviction_cooldown` worked as-is once
  BUILD_TOKEN was pinned. `independent_profile_unlock` +
  `third_connection_evicts_oldest` were one-line selector swaps
  (`.profile-chip` → `.header-identity .header-profile-name`,
  `.takeover` → `.takeover-void`). The three "P1 places, P2
  places" tests (`concurrent_edits`, `ownership_pip`,
  `disconnect_ghosts`) needed the new place flow — rewritten with
  P1 placing serially (waits for slot 1 loaded) before P2's
  place picks `.fig-card-p4:not(.scan-new)[1]` (a different card
  to avoid the on-portal short-circuit). Wall-clock acceptable:
  the supposed Event::Welcome timing concern from the original
  triage ran turned out to be Chrome-resource contention from
  intra-file parallelism, not a real WS regression.
- [x] 10.3.6d — Replaced `profile_create_and_unlock_lands_on_game_picker`
  with `existing_profile_unlock_lands_on_game_picker` (~5 s). The
  original walked the form-based create wizard which no longer
  exists; the new test exercises the daily-use flow (inject
  profile → tap card → tap 4 PIN keys → land on game picker).
  PIN keypad uses `.pin-keypad-heraldic .pin-hkey` per the
  current SPA; auto-submits on the 4th digit. The create-wizard
  itself isn't covered by automation any more — `inject_profile`
  + the wizard's own unit tests are sufficient given the
  cost/benefit of automating a multi-step Konami-gated form.
  Added to the CI lane (no fixture pack needed — uses
  inject_profile + asserts on game-card count from the mock
  driver's fixed 6-game enumeration).
- [x] 10.3.6e — `screenshot_tour.rs` already used modern
  selectors + a custom `tap_via_pointer` helper (the tour predates
  the broader test drift). After the BUILD_TOKEN-pin fix landed
  on the harness it ran clean (~38 s for the full 10-scene tour).
  All 10 PNGs in `docs/assets/screens/` re-baselined on macOS;
  closes 10.3.4 too.

- [x] **BUILD_TOKEN pin** (cross-cut): `TestServer::spawn` +
  `spawn_live` now pass `BUILD_TOKEN=e2e-test` to the cargo-run
  server child, and the README + CI both build the phone bundle
  with the same env. Without this the phone's
  `<git-hash>-dirty` baked at trunk-build-time and the server's
  same value rebuilt later drift apart whenever the working tree's
  dirty state changes between builds, raising a StaleVersion
  modal mid-test that blocks every click. Caught while running
  the screenshot tour; root-caused via curl /api/version vs.
  strings on the wasm bundle.
- [x] 10.3.6f — **CI fixture-pack story: synthetic stub pack.**
  The indexer is lenient — when sky-parser fails on a `.sky` file
  it logs a warning and falls back to `FigureId::new("sha:<hash>")`
  + the filename stem as the canonical name. So a tree of empty
  stub files produces a tiny but valid library of pseudo-figures
  for CI tests to find / click. CLAUDE.md's "no bundling .sky"
  rule is satisfied — the stubs aren't real firmware, they're
  empty files with names that match what the tests select for.
  CI lane (`e2e-mock-macos` + `e2e-ios-sim`) generates the stub
  tree before running tests via `mkdir -p` + `touch`:
  Spyro / Eruptor / Voodood / Bash / Gill Grunt / Whirlwind under
  Skylanders Giants/{Fire,Magic,Earth,Water,Air}/. This unlocks
  working_copies::load_uses_canonical_name (searches "Spyro"),
  all 6 regressions tests, all 6 multi_phone tests, and
  screenshot_tour — bringing the e2e CI lane from 4 → 18 tests.
  Verified locally: every previously-local-only test passes
  against the stub pack. CI uploads no PNGs from
  screenshot_tour — the per-scene captures are just ephemeral
  artifacts of the tour-script-not-crashing regression contract.

  **Iteration tips for any of the above:**
  - Run a single test file at a time with `--test-threads=1` to
    avoid Chrome contention.
  - `export CHROMEDRIVER=$HOME/Library/Application\ Support/skylander-portal-controller/chromedriver-147`
    until brew chromedriver and local Chrome land on a matched
    pair.
  - Process-group cleanup is now automatic — no manual `pkill`
    between runs needed. (If a test ever leaks again, that's a
    real bug in `ChildGuard`'s Drop, not a missing cleanup step.)
  - Add each test back to `e2e-mock-macos` in `ci.yml` as it
    goes green, so we don't regress what we've fixed.


- [x] 10.4.1 — `tools/ios-inspect/` now ships a `[lib]` (`ios_inspect`)
  alongside the existing `[[bin]]`. `src/lib.rs` exposes the four
  modules + high-level helpers (`boot_devices`, `open_url`,
  `wait_for_selector`, `query_selector_count`,
  `query_selector_text`, `eval_js`, `session_id`,
  `wait_for_session_id`, `screenshot_web`, `shutdown_all`,
  `tear_down`). `src/main.rs` now `use ios_inspect::*` instead of
  declaring the modules inline. The crate stays standalone (its own
  `[workspace]` block kept) so its CLI deps don't pollute the main
  workspace; root `Cargo.toml` has `exclude = ["tools/ios-inspect"]`
  so e2e-tests' path-dep doesn't trip Cargo's "multiple workspace
  roots" guard.
- [x] 10.4.2 — `crates/e2e-tests/Cargo.toml` gains
  `[target.'cfg(target_os = "macos")'.dependencies]
   ios-inspect = { path = "../../tools/ios-inspect" }`. Lives under
  regular `dependencies` (not `dev-dependencies`) so the symbol is
  reachable from `tests/*.rs` source — Rust integration tests
  resolve via the dependent crate's regular dep graph.
- [x] 10.4.3 — `crates/e2e-tests/tests/ios_simulator_smoke.rs`
  passes end-to-end in ~19 s. Boots one iPhone sim, opens the
  Bonjour SPA URL, waits for `.game-card` to render, asserts
  `count == 6`. `TeardownGuard` Drop impl runs `shutdown_all`
  via `tokio::task::block_in_place + Handle::current().block_on`
  so a panic mid-test still cleans up. Required tightening
  `wait_for_selector` to forgive transient "no tabs visible"
  errors during the 1-3 s window after `simctl openurl` while
  Safari is registering the page with `webinspectord_sim`.
- [x] 10.4.4 — `tests/ios_two_phone.rs` passes in ~64 s. Boots
  iPad + iPhone, opens SPA on each, calls `inject_profile` for
  Alpha+Beta + `unlock_session(Alpha)`, lets iPhone consume the
  pending unlock, calls `set_session_profile(s_ipad, beta)`,
  asserts each device's `.header-identity .header-profile-name`
  shows the right name. Mirrors `multi_phone.rs::independent_profile_unlock`
  shape with the post-design-system selectors. Figure-placement
  + cross-device portal sync deferred to a follow-up — the
  session/profile contract is the load-bearing 2-phone product
  contract; figure placement layers on top once the chromedriver
  suite (10.3.6) re-pins those selectors first.
- [ ] 10.4.5 — Visual regression: snap paired screenshots per scene
  (profile picker, portal, toy box, kaos overlay) on iPad + iPhone,
  commit as `docs/assets/screens/ios/<scene>-{ipad,iphone}.png`. Reuse
  the chromedriver tour's scene list (`tests/screenshot_tour.rs`) as
  the source of truth for *which* scenes to capture. Blocked on a
  `set_game` / portal-screen helper extension to drive each
  device past the GamePicker into a stable scene before snapping.
- [x] 10.4.6 — `crates/e2e-tests/README.md` gains an "iOS Simulator
  lane" section: prereqs (Xcode + iPad + iPhone runtimes +
  `brew install ios-webkit-debug-proxy`), per-test wall-clock
  budgets, manual-cleanup escape hatch, caveats list (sim safe-
  area-inset-bottom always 0; same-name-across-runtimes pick is
  non-deterministic; multi-thread tokio runtime required for the
  TeardownGuard Drop).


- [x] 10.5.1 — `docs/dev/ci.md` captures the host-choice decision.
  GH-hosted `macos-14` for everything Mac-side (including the
  iOS-sim lane). Tradeoffs documented: cold simulator boot vs.
  operational simplicity; switch to self-hosted when iOS-sim usage
  grows past free macOS minutes or warm cache becomes load-bearing.
- [x] 10.5.2 — New `clippy-build-test-macos` job in `ci.yml`,
  parallel to the existing Windows job. Same shape: clippy
  --workspace --all-targets -D warnings, build --all-features, test
  --workspace, plus a production-flavored `cargo build --release
  --features sky-stats,mock-driver-runtime` that mirrors what
  `release.yml` ships. Catches feature-flag regressions in the lane
  that actually goes to users.
- [x] 10.5.3 — New `e2e-mock-macos` job runs the chromedriver e2e
  suite against the mock driver on macos-14. ChromeDriver pulled
  from the Chrome-for-Testing CDN matched to the runner's Chrome
  version (avoids the "ChromeDriver version N only supports Chrome
  N" mismatch a brew install would risk). Currently whitelisted to
  `tests/smoke.rs` only — broader suite reconciliation tracked in
  10.3.6 will expand the `--test` list as each chromedriver test
  gets a selector update.
- [x] 10.5.4 — New `e2e-ios-sim` job runs `tests/ios_simulator_smoke`,
  `tests/ios_pwa_hint`, and `tests/ios_two_phone` in sequence on
  macos-14. Gated to PR label `run-ios-sim` OR manual
  `workflow_dispatch`; skipped on push so the macOS-minutes budget
  doesn't blow up on every commit. Brew-installs
  `ios-webkit-debug-proxy` per run; uses Xcode + iOS runtimes
  pre-installed on the macos-14 image. Tests run in sequence (not
  parallel) to avoid two simultaneous reads/writes of
  `/tmp/ios-inspect-state.json`.
- [x] 10.5.5 — `.githooks/pre-push` is a Bash hook that runs
  `cargo fmt --all --check` (workspace + phone) + `cargo check
  --workspace` + `cargo test --workspace`. Activate with
  `git config core.hooksPath .githooks`; bypass with `git push
  --no-verify`. Documented in CLAUDE.md's "Git workflow" section.
  Doesn't run clippy — CI does that, and the gain over `cargo
  check` is small for the daily loop.


- [x] 10.6.1 — `.github/workflows/release.yml` gains a
  `build-macos-release` job on `macos-14` (Apple Silicon). Mirrors
  the Windows lane: install Rust + wasm + trunk, build phone bundle,
  `cargo test --workspace`, build server with production features,
  bash-stage the binary + README + LICENSE into a tar.gz, upload as
  artifact, attach to the draft release on tag pushes. Both jobs run
  in parallel; second one to land just appends its artifact to the
  same draft.
- [x] 10.6.2 — `docs/setup.md` rewritten to describe both
  distributions: Windows zip with the UIA driver vs. macOS tar.gz
  with the mock driver. New "Mac install notes" subsection covers
  Gatekeeper "right-click + Open" workaround + the no-wizard
  default-config behaviour landed in 10.6.4. Hardware section
  updated to call out Apple Silicon as a supported target.
- [x] 10.6.4 — Mac production binary skips the wizard. New
  `mock-driver-runtime` Cargo feature ungates `MockPortalDriver` in
  release builds without pulling in the rest of `dev-tools` (relaxed
  HMAC, `/api/_dev/log`, `.env.dev` parsing). `dev-tools` now implies
  `mock-driver-runtime` so existing dev workflows are unaffected.
  `config::load` (release branch) writes a sensible default
  `config.json` (driver=mock, empty rpcs3 + pack paths) on macOS
  instead of running the wizard — Mac binary boots clean on first
  launch and goes straight to serving the QR. Also fixed a
  Windows-tuned `paths::runtime_dir_unchecked` that was walking up
  one too many directory levels on macOS — now stops at
  `~/Library/Application Support/skylander-portal-controller/`
  instead of polluting `~/Library/Application Support/`.
  Smoke-verified: `cargo build --release -p skylander-server
  --no-default-features --features sky-stats,mock-driver-runtime`
  produces a binary that boots cleanly, writes a config, and
  serves `/` → 200.
- [ ] 10.6.3 — Optional follow-up (not gating Phase 10 close): bundle
  the binary as `Skylander Portal Controller.app` for dock + Launchpad
  discoverability. Skip code signing / notarization unless a user
  flags the Gatekeeper friction as a blocker — `$99/yr Apple
  Developer ID + manual notarization run` is non-trivial overhead for
  a hobby project. If signed: integrate `codesign` + `notarytool` into
  the release lane; if not: keep the right-click-open instruction in
  the setup doc.




- [x] 10.7.1 — Spike landed. New `crates/server/src/badge.rs`
  ships a `BadgeRig` mirroring `vortex.rs`'s shape (program +
  empty VAO, since the vertex shader generates a 64-segment
  TRIANGLE_FAN disc procedurally from `gl_VertexID`). Hardcoded
  60° FOV perspective projection, camera at +Z=2.5; gold-orange
  placeholder fill with rim falloff; back-face culled so the
  front face is solo. Lazy-init alongside the vortex rig in
  `LauncherApp::update`, destroyed in `on_exit`. Gated behind
  the `LAUNCHER_3D_BADGE` env var — set it to swap the legacy
  2D `qr_card_flip` body for `badge::paint_badge` in the same
  rect, no other UI changes. Verified against `cargo run`: a
  round disc renders in the middle of the launcher, no
  shader-init errors, no crash, vortex backdrop still paints
  underneath. Three load-bearing assumptions confirmed —
  second `ShaderRig` shape coexists in egui_glow's GL context,
  PaintCallback maps to the badge rect cleanly, and
  perspective-projected disc renders inside it.

- [x] 10.7.2 — QR texture wired through. Refactored
  `main_screen::render_qr_texture` into `render_qr_pixels` (raw
  RGBA + dims via `round_qr::render`) + `pixels_to_egui_texture`
  (the egui-side packing). `LauncherApp` keeps the pixels around
  in `qr_pixels: RoundQrPixels`; the lazy-init in `update()`
  hands them to `BadgeRig::new(gl, &qr_pixels)`, which uploads a
  single RGBA8 texture (LINEAR min/mag, CLAMP_TO_EDGE wrap, no
  mipmaps — the disc fills ~80 % of the badge rect at face-on
  and edge-on poses are barely visible anyway). Fragment shader
  samples `texture(u_qr, v_uv).rgba` and discards pixels outside
  the analytic disc to clean up the 64-gon polygon-vs-circle
  sliver near the spokes. Verified: with `LAUNCHER_3D_BADGE=1`
  the round QR pixels render directly on the disc face, sharing
  the exact buffer the egui-side texture uses (no
  re-rasterisation). Brand-text texture (the "SCAN TO CONNECT"
  caption + the in-game profile-name surface) is deferred to
  10.7.5 when we decide whether to bake the text into the disc
  geometry or keep it as a 2D egui overlay.

- [x] 10.7.3 — `LaunchPhase::badge_rotation_y(self) -> f32`
  added alongside (not replacing) `badge_scale`. Linear
  interpolation between π/2 (edge-on) and 0 (face-on) — the 2D
  badge_scale's sine warp was a perceptual hack to fake what
  perspective gives you for free in real 3D, so we don't need to
  bake the ease into the rotation curve. Same 20%-into-intro
  start as badge_scale so the choreography stays in lockstep.
  `qr_card_flip` gains a `badge_rotation_y` parameter and feeds
  it to `paint_badge` when the env-gate is set; the 2D path is
  untouched. Verified with `cargo run` — Chris confirmed "it
  rotates!" — disc spins from edge-on to face-on during intro,
  the existing flip animation (QR → Loading back-face) still
  reads on the 2D fallback. `badge_scale` survives until 10.7.7
  cleanup.

- [x] 10.7.4 — Cylinder-cap geometry + Lambert lighting landed.
  One shader, three draw calls per frame branched on a `u_face`
  uniform: front fan (textured QR, CCW winding, z=+0.04), back fan
  (gold, **reversed angle step** so screen-space winding is CW from
  +Z view — without this both fans would draw at θ=0 and the
  no-depth-test pass would race for centre pixels), side-wall
  TRIANGLE_STRIP cylinder (gold, radial outward normals).
  Directional Lambert from `normalize(0.3, 0.5, 1.0)` with a 0.4
  ambient floor; light direction is in view space (which here =
  world space) so it sweeps across the side wall as the disc
  rotates. Chris confirmed 2026-05-02: "coin was a good thickness"
  — 8% diameter ratio reads as poker-chip / heavy-coin chunk
  without overweighing the front face. 10.7.5 is the next decision.

- [x] 10.7.5 — Bezel ring landed as 3D geometry (path (a) from
  the original decision). Added a flatter elliptical torus
  around the disc as `u_face=3` in the shader (procedural
  TRIANGLES via gl_VertexID, 64×16 quad grid → 6144 verts), plus
  a lit-gold annulus on the disc face from `QR_RADIUS=1.0` to
  `OUTER_RADIUS=1.04` so there's no gap between the QR's outer
  edge and the torus's inner cross-section. All four faces share
  a Blinn-Phong specular term (warm-white tint, shininess 48,
  strength 0.85) so the highlight sweeps consistently across the
  whole gold "frame" as the disc rotates — the QR front face is
  Lambert-only since printed surfaces don't catch a highlight.
  Final geometry after iteration: torus R=1.16, W_RAD=0.13,
  W_Z=0.05 (cross-section the user landed on as "perfect
  shape"), gold ring 0.04 thick, perspective bumped to
  CAM_DISTANCE=3.5 / F=2.425 to stop the torus's near rim
  clipping the rect at θ≈45°. Same projection change fixed a
  long-standing depth-mapping bug (NDC.z was flat at -1 for
  every fragment, so depth_test was a no-op and the
  cylinder/torus interaction had been carried entirely by
  cull-face).

- [x] 10.7.6b — Rotate-on-text-change + back-face textures landed.
  ab_glyph promoted to a direct dep of `crates/server`; new
  `badge_text` module rasterises each `BackFace` variant's lines
  via TitanOne onto a starfield-blue (`palette::SF_1`) inscribed
  disc with corners transparent. Faux-emboss in three glyph
  passes per line — black shadow at +emboss_offset, white
  highlight at -emboss_offset, near-white body at zero — sized
  as 2 % of the chosen px_size and clamped to [1, 4] px so small
  text stays crisp and large text gets readable depth.
  `BadgeRig` now owns a Vec<glow::Texture> (index 0 = QR, 1.. =
  back faces in `BackFace::ALL` order); `paint` takes a
  `texture_index` + `apply_texture_spec` so the Blinn-Phong
  highlight tracks across the back-face metal but stays off the
  QR raster (where it'd wash out the data). Flip choreography in
  `qr_card_flip`'s 3D branch detects `back_face` changes via
  egui-memory-tracked `displayed` state, kicks off a 360°
  additional rotation via `flip_start` timestamp, and swaps
  `displayed` at flip midpoint when the disc is on its gold back
  face and the swap is invisible. `request_repaint` while a flip
  is in flight so the rotation reads smoothly even if no other
  state would request frames. ChrisCheck 2026-05-03: rotate
  "looked good", blue bg + faux-emboss "acceptable" — defers
  per-pixel normal-map embossing (10.7.6c) unless future polish
  demands it.

- [x] 10.7.6 — Choreography landed: `LaunchPhase::badge_alpha_3d`
  / `badge_scale_3d` plus a multi-turn `badge_rotation_y`
  rewrite, all plumbed to the badge shader as `u_alpha` and
  `u_scale` uniforms. Curves:
    - **Startup hold (intro 0–20%)**: badge invisible while the
      iris reveal beat plays.
    - **Fade-in (20–25%)**: alpha smoothsteps 0→1 with the disc
      held edge-on at `SCALE_MIN_3D = 0.10`.
    - **Spin + grow (25–100%)**: `SPIN_TURNS = 3` full
      revolutions + 90° landing-lap (3.25 turns total) on
      ease-out cubic, with the 2D scale envelope smoothstepping
      0.10 → 1.0 *independent* of rotation so the multi-turn
      cosine cycles don't pulse the envelope (Chris's
      "growing spin" feedback — pure rotation lost the physical-
      object materialising beat the legacy 2D `badge_scale`
      sine provided).
    - **Close mirrors**: 3-turn spin out from face-on shrinking
      to 0.10 over the first 60% of close, then alpha
      smoothsteps 1→0 over the last 40% so the badge dissolves
      into the iris-close beat instead of freezing as a sliver.
  QR texture is always sampled; the smoothstep growth puts the
  disc at ~77% size by the start of the final rotation so the QR
  is legibly on the face well before landing — no texture
  pop-in. ChrisCheck 2026-05-02: "looks so much better."

- [x] 10.7.7 — Default 3D badge on; 2D deadcode dropped.
  Removed the `LAUNCHER_3D_BADGE` env gate from
  `qr_card_flip` — Main now always renders through the GL rig.
  Trimmed the call site to the four args the 3D path actually
  consumes (`back_face`, `badge_rig`, `badge_rotation_y`,
  `badge_scale_3d`, `badge_alpha_3d`); dropped `tex`,
  `phase_scale`, `bezel_alpha`, `content_alpha` from
  `qr_card_flip`'s signature. Deleted `LaunchPhase::badge_scale`
  + `badge_alpha` (only consumers were the now-gone 2D path)
  and their tests. Deleted `paint_qr_front`, `paint_loading_halos`
  and `paint_halo_arc` — also only used by the 2D path. Kept
  `paint_titled_card`, `paint_bezel`, `paint_radial_gradient_disc`
  because the non-Main screens (Crashed / Farewell / ServerError)
  still render their title cards in 2D via
  `paint_centered_back_card`. Smoke-launched without the env
  prefix to confirm 3D is the default. Mac smoke ✓; Windows smoke
  via CI's Windows lane (release build path through the same
  PaintCallback machinery).

- [x] 10.7.8 — Pips ported to GL via the existing badge rig.
  Chris's insight: instead of building a new pip shader, just
  reuse the `BadgeRig` shrunken down — a pip is a single-letter
  + colour disc, exactly what the badge already renders for
  back-face textures. Made it work end-to-end:
    - New `badge_text::render_pip(color, initial, ghost, size)`:
      profile-coloured (or desaturated for ghost) inscribed disc
      with the initial centred in TitanOne + the same faux-emboss
      the back-face textures use.
    - `BadgeRig` gained a `pip_cache: HashMap<(u32 colour-key,
      char, bool), usize>` and lazy `ensure_pip_texture(gl, …)`
      that rasterises + uploads on first sight of a `(color,
      initial, ghost)` triple, returns the cached texture index
      on subsequent calls. `paint_pip` glues that to the existing
      `paint` so each pip draws the same coin geometry as the
      main badge.
    - `main_screen::paint_player_orbit_3d` replaces the old 2D
      `paint_player_orbit` / `paint_pip`. Per-slot flip-on-
      profile-change choreography mirrors `qr_card_flip`'s back-
      face flip: each slot tracks `displayed` + `flip_start` in
      egui memory, kicks off a 360° spin when the session's
      face changes, swaps texture at flip midpoint.
    - Default pip face = `palette::SF_1` blue + white "?", so
      unauthenticated sessions match the back-face card aesthetic
      and flip to profile colour on PIN unlock.
  Pips now fade with the same `badge_alpha_3d` curve as the
  main badge, so the iris-close beat dims them out together
  instead of leaving them as full-opacity orbiting dots. Chris
  confirmed 2026-05-03: "looked good".

- [x] 10.7.10 — Crashed / Farewell / ServerError now render
  through the 3D badge. New
  `paint_centered_3d_back_card(ui, back_face, badge_rig,
  scale, alpha)` helper allocates the same `CARD_SIZE` square
  the legacy 2D path used and drives it through the existing
  `BadgeRig::paint` with the matching `BackFace` variant's
  texture bound + `apply_texture_spec=true` so the gold-on-
  blue title catches the same Blinn-Phong sheen as the
  surrounding ring/torus. Each screen's render fn now takes
  `badge_rig: Arc<Mutex<Option<BadgeRig>>>` and the dispatcher
  in `mod.rs` clones it through; `BackFace::Farewell.lines()`
  updated from `["GOODBYE"]` → `["FAREWELL", "PORTAL",
  "MASTER"]` to keep the legacy farewell copy. Farewell's
  breathe pulse multiplies into the single `u_scale`
  uniform (3D rotation handles the foreshortening, no
  separate horizontal/vertical squash needed). Big code
  cleanup followed: dropped the entire 2D back-card surface
  — `paint_centered_back_card`, `paint_titled_card`,
  `paint_bezel`, `paint_radial_gradient_disc`, `lerp_color`,
  `BEZEL_RING_PX`, `SCREEN_RIM_PX` — all dead now that no
  call site uses the 2D path. `main_screen.rs` shrank by
  ~245 lines. ChrisCheck 2026-05-03: "looked good".

- [x] 10.7.9 — Not actually a leak; in-game surface was burning
  60fps cycles on an empty transparent panel. Diagnostic log
  on pip texture uploads confirmed the cache works (one
  upload per unique `(color, initial, ghost)` triple per
  BadgeRig lifetime); RSS snapshots over a 6-minute window
  showed memory steady (292 MB → 228 MB, even decreased).
  Real culprit: `mod.rs::update`'s unconditional
  `request_repaint_after(16ms)` ran for every surface,
  including in-game where the launcher paints a transparent
  panel + occasional reconnect-QR fade-in. Even with the
  launcher window offscreen behind RPCS3, egui kept
  redrawing 60× / second + the compositor was alpha-blending
  the transparent panel against the game underneath →
  laptop fans kicked in.
  Fix: moved the 60fps repaint request *out* of `update`'s
  prologue and into per-branch positions:
    - Launcher branch (Main / Crashed / Farewell /
      ServerError) keeps 60fps — vortex shader animates
      clouds continuously, badge has multi-turn animations.
    - In-game branch only requests 60fps when the
      reconnect-QR fade-in is live (`reconnect_fade_elapsed_s`
      ∈ (0, RECONNECT_FADE_IN_S)). Otherwise reactive —
      tokio status-change handlers wake egui via
      `Context::request_repaint`, so we don't miss
      transitions (game crash, switching, new client).
  ChrisCheck 2026-05-03: "worked perfectly" — laptop temp +
  CPU drop materially once in-game. Pip-texture diagnostic
  log left in place at info level — fires once per unique
  pip per BadgeRig lifetime, useful if a future regression
  turns the cache off.


- [ ] 10.8.1 **Windows Defender SmartScreen "unknown publisher" gate
  on first launch.** Non-technical users will bounce off the
  "Windows protected your PC" dialog. Likely needs Authenticode
  code-signing for the released `.exe` (~$200/yr third-party CA, or
  EV cert for instant SmartScreen reputation). Investigate cheaper
  paths first — winget submission, Microsoft Store listing — before
  committing to a yearly cert spend. Pairs with 10.8.2 (any signed
  installer can request firewall + SmartScreen handling in one UAC
  prompt instead of three separate gates).
- [x] 10.8.2 **Windows Firewall doesn't auto-create the inbound
  listening rule.** Closed by 10.9.1: the MSI ships a
  `<fire:FirewallException>` element that registers the inbound
  TCP/8765 rule at install time under the same UAC prompt that
  drops the binary in Program Files. Smoke-tested on the HTPC —
  `Get-NetFirewallRule -DisplayName 'Skylander Portal Controller'`
  shows the rule post-install and gone post-uninstall.
- [ ] 10.8.3 **Launcher "exit to desktop" affordance cut off after
  phone connect.** Suspected fullscreen / overflow bug in the eframe
  launcher on the 86" TV at 4K — connected-state layout pushes the
  exit control past the visible area. Reproduce on an external
  display; check whether the in-game branch's panel positions the
  exit button at a coordinate beyond the viewport edge or whether
  fullscreen scaling clips it.
- [x] 10.8.4 **"Couldn't access the file dialog for portal."** Root
  cause: synthesised-keystroke menu nav for both `open_dialog` (Manage
  → Portals → Skylanders Portal) and `quit_via_file_menu` (File →
  Exit) was fragile — game viewport, RPCS3 update popup, Steam Overlay,
  and shader-compile load all competed for keyboard focus mid-nav.

  Closed end-to-end across `main`. The library-view + select+Enter
  architecture is retired entirely; RPCS3 is now spawned on demand
  with the picked game's EBOOT.BIN as a CLI arg, and the Manager
  dialog opens via three UIA pattern calls with no focus dependency.

  Driver layer (commits `1ebbbc9`, `6d05f65`, `0991814`):
  - `tools/uia-probe` confirmed current RPCS3 (Qt 6.11) advertises
    `Invoke` + `ExpandCollapse` + `LegacyIAccessible` on every
    `MenuItem` — the old CLAUDE.md gotcha that Qt6 menus didn't
    honour these patterns is stale.
  - `trigger_dialog_via_menu` and `quit_via_file_menu` both rewritten
    as Expand+Invoke pattern calls. ~200 lines of retry loop +
    keystroke-synthesis machinery deleted (`focus_main_window`,
    `send_key`, `key_input`, `expect_focused_menu_item`,
    `collect_focused_menu_items`, `normalise_menu_name` — all gone).
  - Detection switched from classname (`skylander_dialog`) to title
    (`"Skylanders Manager"`) — current Qt unifies every Qt window
    under `Qt6110QWindowIcon` so class-match silently mis-resolved.
  - `UiaRpcsProcess::launch_with_eboot(exe, eboot)` added; direct
    EBOOT.BIN launch works with pattern menu nav (the CLAUDE.md
    "EBOOT-arg launch breaks menu nav" caveat was keystroke-specific).
  - `games_yml` module: parser for RPCS3's
    `<install>/config/games.yml` serial → game_dir map.
  - `examples/production_open_dialog.rs` end-to-end smoke verified
    twice on the HTPC against Skylanders Giants — boots, opens
    dialog, kills cleanly.

  Server cutover (commit `616fdf2` + cleanup `9e20233`):
  - main.rs at startup: skipped `launch_library`, populates
    catalogue from `games_yml::read_games_yml` (mock driver still
    seeds the full SKYLANDERS_SERIALS list). RPCS3 stays dead
    until the user picks a game.
  - http.rs `/api/launch`: looks up the picked serial in
    `state.games_yml`, dispatches `DriverJob::BootDirect
    { eboot_path, expected_name, display_name, serial }`. 180s
    timeout (first-launch shader compile can hit 60–120s). Drops
    the pre-boot UIA `EnumerateGames` validation — the games.yml
    lookup either resolves to a real EBOOT or returns 404 with
    the same "isn't in RPCS3's library" message.
  - state.rs `BootDirect` handler: `launch_with_eboot` →
    `wait_ready` → poll `read_viewport_title` for `"FPS:"` +
    expected name → `driver.open_dialog()`. Updates
    `RpcsLifecycle.process / current / current_eboot`,
    `LauncherStatus.rpcs3_running / current_game`, broadcasts
    `Event::GameChanged`.
  - state.rs crash watchdog: captures `current_eboot` at crash
    detect; auto-respawn via `launch_with_eboot` re-launches the
    same game (no library-view fallback). Without an EBOOT
    recorded, skip respawn and leave the user on Crashed.
  - http.rs `/api/quit`: kills RPCS3 entirely (no
    library-view return). `?force=true` is now a no-op
    (kill is the only path); `?switch=true` still arms the
    launcher transition.
  - Retired: `DriverJob::{BootGame, EnumerateGames, StopEmulation}`,
    `PortalDriver::{boot_game_by_serial, enumerate_games,
    stop_emulation}` trait methods + UIA + Mock impls,
    `verify_viewport_title` + 4 unit tests, `post_click` /
    `post_key` / `pack_xy_lparam` (Z-order-bypass synthesis),
    `prep_main_offscreen` / `OffscreenMainGuard` (off-screen
    menu nav), `examples/zorder_probe.rs`,
    `examples/stop_probe.rs`. **Net diff: ~600 lines deleted.**
  - `tests/live_lifecycle.rs` migrated to direct-boot via
    `launch_with_eboot` + `read_viewport_title` polling.

  All 41 workspace test groups pass post-refactor.
- [ ] 10.8.5 **Steam Big Picture: missing icon + banner artwork.**
  Steam library tile shows the generic placeholder; needs `.ico` +
  `library_capsule` / `library_hero` / `library_logo` images
  registered with the non-Steam shortcut. Add an asset-bundling pass
  to the release zip (icons + Steam Grid artwork in a `steam/`
  subdir) plus a setup-doc step for "right-click shortcut → Manage →
  Set custom artwork" on Big Picture.
- [-] 10.8.6 **Phone app: figure + game images all render as
  placeholders on HTPC install.** Diagnosis confirmed: release
  binary's `data_root` resolves to `<exe_parent>/data/`
  (`wizard.rs::from_user_paths` and `::macos_default`), but
  `release.yml`'s staging step only copied the binary + README +
  LICENSE — the ~20 MB committed `data/` tree (`images/<id>/thumb.png`
  ×474, `games/<serial>.png` ×6, `figures.json`, `LICENSE.md`)
  never made it into the zip/tarball. On HTPC install, figure thumbs
  fell through to the firmware-pack element-icon fallback at
  `http.rs:460` (what Chris saw as "the placeholder"); game box-art
  has no fallback so those `<img>` tags rendered broken. Fix landed
  in the release workflow: both lanes now `Copy-Item -Recurse data`
  / `cp -R data` into the staged folder. `data/scanned/` and
  `data/images/**/hero.png` are gitignored so the CI checkout
  doesn't include them — only the committed thumbs + box-art ship.
  Verify on next tag (or `workflow_dispatch` rehearsal): inspect the
  release zip → confirm `data/images/000000-0000/thumb.png` is
  present, install on a clean Windows host → curl
  `/api/figures/000000-0000/image?size=thumb` returns the scraped
  PNG (not the element icon) and `/api/games/BLUS30779/image`
  returns 200.



  game_playable signal was a fragile log-quiet heuristic that
  defaulted to "playable" when the watchdog hadn't seen any compile
  lines yet (race against RPCS3's freshly-spawned log writes).
  in-game render over a still-compiling (visually black) RPCS3
  viewport. Tweaked watchdog moved the bug, didn't kill it.
  was fully reactive (no `request_repaint`) and never woke for the
  `screen = Farewell` flip. Fixed by 4 Hz heartbeat in 10.8.4 final
  commit, but the underlying lesson is that state transitions need
  to be observable, not "set a flag and hope a frame fires".
  in-game transparent panel briefly shows desktop through, before
  the launcher transitions to AwaitingConnect.





- [x] 10.8.7a **Black-screen audit (read-only).** Walked every
  render path. Findings:

  *Render dispatch* (`ui/mod.rs::update`):
  - `clear_color = [0,0,0,0]` (transparent). Only matters when
    the CentralPanel ALSO doesn't paint a given pixel.
  - One transparent CentralPanel: the in-game branch (line ~478).
    Predicate: `close_complete && rpcs3_running &&
    current_game.is_some() && !switching && screen=Main`. If
    true, panel is `Frame::none().fill(TRANSPARENT)` — only the
    optional reconnect QR paints. RPCS3 viewport shows through.
  - Every other path uses an opaque CentralPanel that always
    paints (in order): `paint_sky_background` (dark-blue
    gradient ellipses), `paint_starfield`, vortex shader,
    `paint_starfield` again on top, then per-screen content
    (`render_main` / `crashed::render` / `farewell::render` /
    `server_error::render`). Each per-screen branch paints its
    own opaque surface (badge + text + buttons). No path leaves
    a hole.

  *Vortex shader iris semantics* (`vortex.rs:280–285`):
  - `edge = smoothstep(iris_radius - softness, iris_radius, radius)`
    is 0 inside the iris boundary, 1 outside.
  - `iris_mode = Reveal`: `iris = 1 - edge`. Clouds visible
    INSIDE the iris radius, transparent outside (sky+starfield
    show through).
  - `iris_mode = DarkHole`: `iris = edge`. Transparent INSIDE
    (sky+starfield show through, including the dark-blue sky
    gradient + stars), clouds outside.
  - **Critical finding**: `DarkHole` is misnamed. The "dark
    hole" isn't an opaque black region — it's a transparent
    region revealing the sky+starfield underneath. So the
    current `ClosingToInGame` animation that uses `DarkHole` +
    `iris_radius → 0` does NOT render flat black. It renders
    "vortex cloud pattern grows inward over the dark-blue sky +
    starfield." Animated, never flat black. Same for Farewell
    (uses the same `ClosingToInGame` phase). The user-perceived
    "darkness" of these transitions is the dark-blue sky +
    starfield, which is what every other launcher state shows
    behind its content too — just no per-screen content drawn
    over it once the iris fully closes (clouds + stars only).

  *Black-render risks identified* (the actual bugs):
  - **Risk A — in-game branch with RPCS3 not rendering**: when
    the in-game predicate fires but RPCS3's game viewport hasn't
    started rendering frames (early shader compile, splash
    screens that render black), the transparent CentralPanel
    reveals a black RPCS3 viewport. **This is the v1.3.4 "screen
    is black when game launches" bug.** Fix: 10.8.7c (FPS-based
    `game_playable` so we only enter in-game when RPCS3 is
    actually rendering).
  - **Risk B — cover-before-kill race**: `/api/quit` clears
    `current_game` then kills RPCS3. Between the state flip and
    the launcher's next frame, the launcher's last-rendered
    frame might be the transparent in-game one, and if RPCS3 is
    already dead, the user sees through to desktop. With the 4 Hz
    in-game heartbeat (commit `ca7af77`) the lag is at most
    ~250 ms but it's still visible. Fix: 10.8.7b (`cover_active`
    flag forces the launcher out of in-game render BEFORE the
    server starts the kill).
  - **Risk C — Farewell black-fade overlay**: legitimate, the
    only intentional flat-black render. Stays as-is.

  *Animation-grammar findings* (not bugs, just design
  consistency):
  - `ClosingToInGame` and Farewell both use `DarkHole + iris→0`,
    which reads as "vortex closes in." The user's preferred
    grammar is `0→open` for cover transitions and a different
    "transparency expands from centre" for the launch-to-game
    transition (10.8.7d/e). The current animation isn't showing
    black, but it's not the model we want either. Replacing it
    is a polish/consistency change, not a black-render fix.

  *Vortex shader `iris_radius > IRIS_FULL` behavior*: the
  smoothstep clamps cleanly — at `radius < iris_radius - softness`
  the edge stays 0, so a very-large `iris_radius` in `Reveal`
  mode means the "inside" iris area covers the entire panel,
  giving a fully-cloud-visible state with no boundary edge
  visible. With `iris_softness` defaulted, this transitions
  smoothly. **No shader changes needed for 10.8.7e** — the
  proposed `RevealingGame` phase can extend `iris_radius`
  beyond `IRIS_FULL` and the shader will render correctly.

  *Conclusion*: the strict "only Farewell shows black"
  invariant is currently met for static states. The two real
  black-screen bugs (Risks A+B) are timing/race issues, not
  render-path bugs. Animation-grammar fixes (10.8.7d/e) are
  about visual consistency, not bug fixes.
- [x] 10.8.7b **Cover-before-kill mechanic.** Landed:
  - `LauncherStatus.cover_active: bool` added.
  - In-game render predicate gates on `&& !cover_active`.
  - Sequencer's `kill_close` adds `|| cover_active` so a cover
    arming during in-game clears the close timer. `want_close_start`
    adds `&& !cover_active` so the cover doesn't re-arm a close-to-
    in-game animation mid-cover.
  - `detect_returning_from_game` extended: `want_in_game_now`
    gates on `!cover_active` so the cover arming triggers
    ReturnFromGame even though RPCS3 is still alive at that
    point. The iris-open animation starts BEFORE the kill, not
    after.
  - `/api/quit` rewritten as 4 phases: arm cover (set
    `cover_active=true` or `switching=true` for `?switch=true`),
    sleep 300 ms (≥1 tick of the in-game branch's 4 Hz heartbeat
    + safety margin), kill RPCS3 behind the cover, clear
    lifecycle flags. Returns 202 to phone after the full
    sequence — the kill is sequential not detached.
  - 4 new sequencer tests exercising the cover state machine:
    cover clears latched close timer, cover blocks close-timer
    set, cover triggers return-from-game when armed mid-game,
    cover without prior in-game does NOT trigger phantom return.
  - `/api/shutdown` not changed — it already uses `screen =
    Farewell` as its cover (Farewell renders its own opaque
    surface immediately), and the existing 1.2 s pre-kill sleep
    serves the same purpose as the 300 ms `cover_active` window.

  Skipped from original spec: a synthetic `was_in_game_or_covering`
  field. Once `detect_returning_from_game` was updated to gate
  `want_in_game_now` on `!cover_active`, the existing
  `was_in_game` (set true on the previous in-game frame) carries
  the transition correctly: cover arms → in-game predicate fails
  this frame → `was_in_game=true` from previous frame → returning
  detected. No new state needed.

  41 workspace test groups still green.
- [x] 10.8.7c **FPS-based playable signal.** Landed:
  - New `state::spawn_fps_sampler` background task. Polls
    `read_viewport_title()` every 250 ms; parses the FPS field
    via `parse_fps_from_title` (substring after `"FPS: "`,
    parsed as `f32`). Rolling 4-sample `VecDeque`. `game_playable
    = rpcs3_running && all_samples ≥ 10.0`. Sample buffer
    cleared when `rpcs3_running` flips false so a fresh
    BootDirect for a different game starts clean.
  - Spawn point in `main.rs` next to the existing crash watchdog
    + shader-compile watchdog.
  - `spawn_shader_compile_watchdog` retained but reduced to
    subtitle-only role (`shader_compile_text` for the LOADING
    badge subtitle text). Dropped the `last_compile_at` quiet
    heuristic, `rpcs3_running_since` fallback timer, and
    `game_playable` write. The "FPS in title is RPCS3's
    authoritative per-frame counter" replaces every log-tail
    proxy.
  - 7 unit tests for `parse_fps_from_title`: typical title,
    integer FPS, zero (pre-first-frame), low decimal
    (mid-compile), no-prefix (RPCS3 main window not viewport),
    prefix-but-garbage (defensive), empty.

  41 workspace test groups still green; release-features build
  also clean.
- [x] 10.8.7d **`BackFace::Returning` + Farewell iris flip.**
  Landed:
  - `BackFace::Returning` already added in 10.8.7b along with the
    cover_active back-face precedence wiring.
  - Farewell iris direction flipped from `IRIS_FULL → 0` (close)
    to `0 → IRIS_FULL` (open). Implementation: in `ui/mod.rs`'s
    launch_phase computation for `screen == Farewell`, the
    `close_timers.shutdown_at`-derived `closing_elapsed_s` is
    now passed as the *returning* argument (was: closing). So
    `LaunchPhase::compute` returns `ReturnFromGame { progress }`
    instead of `ClosingToInGame { progress }`. Iris animates
    `0 → IRIS_FULL` over `INTRO_TRANSITION_S` (1.8 s), badge
    spins in / grows / fades in over `ScreenIntro::DURATION_S`
    (1.2 s).
  - `farewell::render` now receives the live `screen_intro`
    (was: `ScreenIntro::landed()`), so the badge animates in
    alongside the iris. Iris-open + badge spin-in land together,
    then the GOODBYE pose holds for the rest of the 3 s
    countdown, then the existing black-fade overlay kicks in
    (in `farewell::render`, painting `Color32::from_rgba(0,0,0,
    alpha)` across the panel — this is the strict-invariant's
    only legitimate flat-black render).
  - `ScreenIntro::landed()` deleted as dead code (only caller
    was the Farewell render that no longer needs it).

  Badge-flip continuity audit: the back-face card uses
  `qr_card_flip` which animates a 3D Y-rotation between
  back-face changes. Transitions in scope:
  - in-game → cover (`cover_active=true` flips back-face to
    `Returning`): card flips from QR (front) → Returning (back).
    Smooth, no jump.
  - in-game → switch (`switching=true` flips back-face to
    `Switching`): same flip mechanics.
  - in-game → shutdown (`screen=Farewell` switches the per-
    screen render entirely): launcher's `Main` → `Farewell`
    branch transition. The Farewell badge is rendered via
    `paint_centered_3d_back_card(BackFace::Farewell, …)` which
    is a fresh card, not a flip; visual reset is acceptable
    because the iris-open animation supplies new motion.
  - Returning → QR (after quit completes): cover_active flips
    false, back-face precedence falls through to `None` (QR
    front), card flips back. ✓

  41 workspace test groups still green.
- [x] 10.8.7e **`Compiling` → `Playable` (in-game) transition spec.**
  Landed: `crates/server/src/ui/launch_phase.rs` replaces
  `ClosingToInGame` with `RevealingGame { progress }`, two-phase
  animation. Phase 1 (`progress 0..REVEAL_PHASE_SPLIT=0.43`) keeps
  iris at `IRIS_FULL` with `iris_mode = Reveal`, plays badge
  spin-out (`badge_rotation_y`, `badge_scale_3d`, `badge_alpha_3d`
  curves run reverse of `IntroTransitioning`). Phase 2 (`0.43..1.0`)
  grows iris from 0 → `IRIS_FILL_SCREEN = 3.0` with `iris_mode =
  DarkHole` (the *iris hole* is the transparent passthrough; outer
  vortex retreats). In-game predicate in `ui/mod.rs` now gates on
  `launch_phase.reveal_complete()`. Dead helpers `ScreenIntro::landed`
  and `ease_in_cubic` removed. Tests:
  `iris_radius_progresses_with_phase`,
  `iris_mode_flips_during_phase_two_of_revealing`,
  `reveal_complete_only_at_progress_one`,
  `badge_alpha_3d_fades_through_phase_one_of_revealing` — all 15
  `launch_phase` tests pass; full workspace `cargo test` clean.

  This is the *inverse* of the cover transitions, with its own
  two-phase animation. **No iris-close-to-dark.** The launcher
  retreats by spinning its badge away and then opening
  transparency from the centre outward.

  *Trigger:* state machine reaches `Playable` (FPS ≥ 10 sustained
  for 1 s, per 10.8.7c).

  *Animation (as built):*

  | Phase | Duration | Iris | Badge | Panel |
  |---|---|---|---|---|
  | **1: badge spin-out** | ~300 ms | held at `IRIS_FULL`, `iris_mode = Reveal` (vortex visible everywhere, same as `AwaitingConnect`) | reverse of intro: rotation runs backward across the same `SWEEP`, scale 1 → 0.1, alpha 1 → 0 | opaque (sky + starfield + full vortex) |
  | **2: vortex retreats from centre** | ~400 ms | `iris_radius` grows `0 → IRIS_FILL_SCREEN (3.0)`, `iris_mode = DarkHole` (cloudless area expands from centre, clouds get pushed outward) | already gone | still opaque (sky + stars beneath, vortex retreating) |
  | (post) | — | `reveal_complete()` flips true | — | in-game branch takes over, `CentralPanel` switches to transparent, game viewport shows through |

  *Why DarkHole, not Reveal:* the shader fragment is `iris = (mode == Reveal) ? (1 - edge) : edge`. With `Reveal` and growing radius, the visible-clouds region expands outward — the OPPOSITE of what we want. With `DarkHole`, the cloudless inner region expands as the radius grows, which is the visible "vortex retreats from centre" effect. The earlier spec line ("Reveal throughout") was wrong — it confused `DarkHole` (cloudless inner region; sky beneath shows through) with literal black, but `DarkHole` only suppresses the shader's cloud output, not the underlying sky. The "only Farewell shows black" rule is preserved: phase 2 shows sky + stars + retreating vortex ring, never a black hole.

  *Implementation:*
  - `LaunchPhase::RevealingGame { progress }` replaces `ClosingToInGame`. `progress` covers both phases: `0..REVEAL_PHASE_SPLIT (0.43)` is phase 1, `REVEAL_PHASE_SPLIT..1.0` is phase 2.
  - `iris_radius()`: phase 1 holds at `IRIS_FULL`; phase 2 grows `0 → IRIS_FILL_SCREEN (3.0)` linearly (large enough to cover any corner of the screen).
  - `iris_mode()`: phase 1 = `Reveal`; phase 2 = `DarkHole`.
  - Badge curves (`badge_rotation_y`, `badge_scale_3d`, `badge_alpha_3d`): driven by `p1 = progress / REVEAL_PHASE_SPLIT`, so the spin-out completes at the phase boundary and the badge stays gone through phase 2.
  - In-game predicate gates on `launch_phase.reveal_complete()`. Dead helpers `ScreenIntro::landed` and `ease_in_cubic` removed.

  *Known seam (resolved in 10.8.7e2):* at `progress = 1.0`, the launcher panel originally flipped from opaque (sky beneath the retreated vortex) to transparent (game shows through). 10.8.7e2 removes the panel-flip entirely.
- [x] 10.8.7e2 **Iris-as-real-punch-through (no panel flip).** Followup to 10.8.7e after Chris flagged the residual 1-frame pop: "the whole point of the iris is so opening and closing shows the game window underneath, NO sudden flips." The iris must be a literal alpha-mask through to whatever's behind the launcher window (the RPCS3 viewport during in-game / cover transitions), not a state-transition signal that flips between opaque/transparent panels.

  *Architecture:*
  - One render branch in `LauncherApp::update`. Always uses `Frame::none().fill(TRANSPARENT)` for the `CentralPanel`. The legacy "in-game branch" is gone — its predicate (`reveal_complete && rpcs3_running && current_game.is_some() && Main && !switching && !cover_active`) is now `is_in_game`, a logical state used only to (a) drive the reconnect-QR overlay paint, (b) pick the 4 Hz repaint cadence, (c) feed `was_in_game` into `detect_returning_from_game`. No panel flip.
  - **`game_underneath`** = `rpcs3_running && current_game.is_some()`. When true, the CPU sky/starfield paints route through `_masked` variants in `vortex.rs` that alpha-multiply by `iris_factor` per vertex/star. When false (boot, picker, post-shutdown), the legacy unmasked paths fire so the launcher is fully opaque (eframe window is transparent — without an opaque sky, the user would see the desktop).
  - `vortex::iris_factor(rect, point, mask)` mirrors the shader's `iris` computation — `uv = (point - center) * scale`, `radius = length(uv)`, `edge = smoothstep(R - softness, R, radius)`, `factor = (mode == Reveal) ? 1 - edge : edge`. Same math the GPU uses, so CPU and GPU layers align at the boundary.
  - `vortex::paint_sky_background_masked` tessellates the rect into a 33×33 grid mesh with per-vertex alpha = `iris_factor`. The decorative top/bottom radial-glow ellipses are skipped during punch-through (subtle enough that omission is imperceptible, and they're not radial about the iris centre so tessellating them is awkward).
  - `vortex::paint_starfield_masked` multiplies each star's alpha by `iris_factor` at the star's centre. Stars at factor=0 are skipped.

  *Mode-flip continuity at the in-game boundary:* the cover→game transition does `Reveal + R = IRIS_FULL` (steady launcher) → `DarkHole + R = 0` (start of phase 2 of `RevealingGame`). At both endpoints, `iris_factor = 1` everywhere — `Reveal+1.5` because edge=0 inside R=1.5; `DarkHole+0` because edge=1 outside R=0. No visible flip across the mode change. Symmetric on the way out.

  *Cover-before-kill timing:* `/api/quit`'s sleep was 300 ms ("4 Hz heartbeat + safety"). With true punch-through, killing RPCS3 mid-iris-animation drops the game viewport behind a half-grown opaque disc — desktop visible at the corners. Bumped to 1500 ms so the cover lands (`ease_out_cubic(1500/1800) ≈ 0.97 → R ≈ 1.45 ≥ corner_radius + softness`) before the kill executes. The shutdown handler's existing 1200 ms is unchanged — Farewell uses `screen_intro` (`DURATION_S = 1.2`) so the Farewell badge lands at 1200 ms anyway, and 1200 ms is enough for `iris_radius` to fully cover corners under `INTRO_TRANSITION_S`.

  *Tests:* full workspace `cargo test` passes (15 launch_phase, 127 server-lib, 26 sky-parser, etc.). The mask helpers + `iris_factor` aren't separately unit-tested — visual correctness depends on continuity properties that are easier to validate with the eyeball-on-HTPC pass in 10.8.7f than with a CPU-side raster comparison.
- [ ] 10.8.7f **Local-cycle testing per phase.** Each phase ends
  with `tools/build-msi.sh --install` on the HTPC, walk through
  the affected flows visually, confirm behavior matches the
  contract before the next phase. Tag a CI release only after
  all phases land + a clean full-flow local pass (start →
  picker → game → quit → switch → game → shutdown).

  iteration (commit `9c7e1d1`). Sub-minute build cycle.
  downloadable from the Releases page without unhiding drafts
  (commit `71e6d01`).
  `1ebbbc9` → `ca7af77`).



- [x] 10.9.1 **Windows `.msi`** via `cargo-wix` (WiX 3 toolset).
  Scaffold + four iteration fixes shipped on the HTPC, smoke-tested
  install/uninstall locally, MSI lane re-dispatched against CI:
  - `wix/main.wxs` — Product + Package + MajorUpgrade scaffold
    matching `cargo wix init`'s default template, customised:
    binary lives directly under `APPLICATIONFOLDER` (not `bin/`)
    so `data_root = <exe_parent>/data` resolves; Start Menu +
    Desktop shortcuts (Advertise='yes', no explicit `Icon=` — they
    inherit the exe's embedded icon from winresource per 10.9.3,
    avoiding ICE50 keyfile-extension validation);
    `<fire:FirewallException>` on port 8765 (folds 10.8.2);
    `ARPPRODUCTICON`/`ARPHELPLINK`/`ARPURLINFOABOUT` for
    Add/Remove Programs polish. Stable UpgradeCode
    `E6FC979F-…-44D5E` and Path GUID `57B7D393-…-7F11` committed
    (must not change across releases or Windows treats future
    MSIs as new products).
  - `crates/server/Cargo.toml` `[package.metadata.wix]` block
    pointing at `../../wix/main.wxs` + heat fragments;
    `license = false` + `eula = false` (MIT doesn't warrant a
    click-through dialog).
  - `release.yml` Windows lane: WiX 3 toolset path discovery,
    `cargo install cargo-wix --locked`, heat harvest (`-var
    var.DataSourceDir` so paths resolve via `-dDataSourceDir=
    <abspath>` at candle time), `cargo wix --no-build
    --install-version <tag>` invoked with `Push-Location
    crates/server` (cargo-wix v0.3.9 resolves `[metadata.wix]
    include` paths from cwd, not manifest dir), MSI artifact
    upload alongside the existing zip.
  - Zip stays as the "portable" fallback artifact (PLAN 10.9.5).

  Open questions resolved during HTPC iteration:
  (a) WiX path discovery — finds `heat.exe`/`candle.exe`/`light.exe`
      under `C:\Program Files (x86)\WiX Toolset v3.14\bin`. ✓
  (b) heat output references — matches `DataDir`/`DataFiles`
      placeholders in `main.wxs`; the only catch was `Source=
      "SourceDir\..."` resolving from cargo-wix's cwd
      (`crates/server/`) instead of workspace root, fixed by
      switching to `-var var.DataSourceDir`. ✓
  (c) `<fire:FirewallException>` — needs both `-C -ext -C
      WixFirewallExtension` (candle) AND `-L -ext -L
      WixFirewallExtension` (light). `WixUIExtension` is
      auto-loaded by cargo-wix; passing it explicitly causes
      LGHT0091 duplicate-symbol on `WixUI_FeatureTree`. ✓
  (d) MSI install + uninstall cleanly — verified on the HTPC:
      `C:\Program Files\Skylander Portal Controller\` populated
      (~14 MB exe + 83 MB data/ tree), Start Menu + Public Desktop
      shortcuts present, inbound TCP/8765 firewall rule registered
      (`Get-NetFirewallRule`), ARP entry shows publisher + version
      1.2.1 + help/about URLs, uninstall removes every artifact
      (folder, shortcuts, firewall rule, ARP entry). ✓

  Side fixes during iteration: stripped `--` literals from the
  `wix/main.wxs` comment block (XML disallows it), corrected
  `..\assets\branding\icon.ico` to `..\..\assets\...` (relative
  to candle's cwd `crates/server/`), removed broken heat `-t
  XSLT` fallback in `release.yml` (referenced a non-existent
  file), guarded the `--install-version` parser against non-tag
  refs (`workflow_dispatch` from a branch sets `GITHUB_REF_NAME=
  main`, which broke cargo-wix's semver parser).
- [-] 10.9.2 **Code signing + notarization.**
  - **macOS Tier-2 (Developer ID + notarization)** wired and
    ready, blocked on user creating GitHub Secrets. Chris already
    has an Apple Developer license — the remaining work is
    one-time setup of the `release` GitHub Environment + 7 secrets
    documented in `docs/dev/release-signing.md`.
    `release.yml`'s macOS lane gains: cert import to throwaway
    keychain → build signed `.app`+`.dmg` (via
    `tools/build-macos-app.sh` with `SIGN_IDENTITY`) → `xcrun
    notarytool submit --wait` → `xcrun stapler staple` →
    `spctl -a` verify → upload to draft release alongside the
    unsigned tarball fallback. Security model: `release`
    Environment scopes secrets, branch-restricts to `v*.*.*`
    tags; `github.repository == ...` belt-and-suspenders guard;
    `release.yml` has no `pull_request_target` trigger so fork
    PRs structurally cannot exfiltrate secrets.
    `tools/build-macos-app.sh` extended with `codesign --options
    runtime --timestamp` for binary + bundle + dmg when
    `SIGN_IDENTITY` is set; no-ops cleanly on local unsigned
    iteration when it isn't.
    **Currently disabled in release.yml** (4 steps wrapped in
    `if: false`) — the Apple secrets haven't been provisioned in
    the `release` Environment yet, so each dispatch was failing on
    the "Import Developer ID certificate" step. Tarball lane still
    ships; users right-click → Open to bypass Gatekeeper. Flip the
    `if:` guards back to the repo-owner check + re-add
    `${{ env.RELEASE_DMG }}` to the publish step's `files:` list
    once the secrets land.
  - **Windows Authenticode** still deferred. Cert procurement is
    the gate — third-party CA ~$200/yr, EV cert for instant
    SmartScreen reputation, or skip the cert spend and document
    the "More info → Run anyway" workaround. Punt the decision
    until after 10.9.1's MSI lands so we have something concrete
    to sign + can measure SmartScreen friction with vs. without.
- [x] 10.9.3 **Embedded `.exe` icon + `VERSIONINFO` via
  `winresource`.** `crates/server/build.rs` extended (existing
  `BUILD_TOKEN` stamper kept) with a `cfg(windows)`-gated
  `embed_windows_resources()` that calls
  `winresource::WindowsResource::new()` / `set_icon` /
  `set("ProductName", …)` / `compile()`. Icon path
  `../../assets/branding/icon.ico` (the multi-res `.ico` baked
  by 10.9.4's `tools/installer-bake/`). VERSIONINFO strings:
  ProductName + FileDescription = "Skylander Portal Controller",
  CompanyName + LegalCopyright = "Christopher Hotchkiss",
  ProductVersion + FileVersion from `CARGO_PKG_VERSION`,
  OriginalFilename = `skylander-portal-controller.exe`.
  Verified on the HTPC: `Get-Item ...exe | Select VersionInfo`
  shows every field populated, and the MSI's Start Menu / Desktop
  shortcuts inherit the embedded icon directly off the exe (which
  is why the wxs `Icon='ProductICO'` attribute on shortcuts could
  be dropped to clear ICE50 — the embedded icon does the job).
- [-] 10.9.4 **macOS `.dmg`** wrapping a `.app` bundle (subsumes
  10.6.3). Local build path landed:
  - `tools/installer-bake/` (Rust, resvg + ico + icns) bakes
    `assets/branding/icon.{ico,icns}` from
    `phone/assets/icons/icon.svg`. 6-entry `.ico` (16/32/48/64/
    128/256), 11-entry `.icns` modern RGBA32 set @1x and @2x
    through 1024px.
  - `tools/build-macos-app.sh` assembles the `.app`
    (`Contents/MacOS/{binary,data/}` + `Contents/Resources/icon.icns`
    + `Info.plist` with bundle ID
    `io.hotchkiss.skylander-portal-controller`, version from
    `GITHUB_REF_NAME`/`git describe`/dev fallback) and `hdiutil
    create -format UDZO` into `dist/`. `data/` ships next to the
    binary in `Contents/MacOS/` rather than `Contents/Resources/`
    so the existing `data_root = <exe_parent>/data` resolution
    works without code changes — same convention as the Windows
    zip layout.
  - 29 MB UDZO `.dmg` smoke-tested locally — mounts, bundle
    structure validated via `plutil -lint` and `plutil -p`,
    `Skylander Portal Controller.app` launches cleanly from the
    mounted volume.
  Steam Grid artwork (post-10.8.5) will land in
  `Contents/MacOS/steam/` alongside `data/` once that's done.
  Remaining: wire into release.yml (10.9.5). Skip codesigning +
  notarization unless real Gatekeeper friction reports surface
  (same rationale as 10.6.3 — $99/yr Apple Developer ID is
  non-trivial overhead for a hobby project).
- [ ] 10.9.5 **`release.yml` rewire.** Replace the current zip /
  tarball staging with installer builds on each lane. Keep the
  zip / tarball as a "portable" fallback artifact for users who
  prefer not to run an installer (and for CI smoke-testing
  without an MSI install step in the loop).

## Phase 11 - Skylander Stat Editing (level + gold)

Wire the disabled **STATS** placeholder on the figure-detail screen
(`phone/src/screens/figure_detail.rs:245-255`) into a real edit
sheet so phone users can bump level + gold before placing a figure
on the portal. Eliminates a huge playtest friction point: testing
level-20-only features used to require hours of grinding.
Confirmed scope: editable when `core::Category` is one of `Figure
/ Sidekick / Giant / Kaos` (other categories — Item / Trap /
AdventurePack / CreationCrystal / Vehicle / Other — get a
disabled button + tooltip); edit allowed only when figure is off
the portal; level + gold only (the level threshold table at
`docs/research/sky-format/SkylanderFormat.md:246-267` is global,
levels 1–20 → 0–199 535 XP, no per-era variants). Healing /
respawn deferred: the documented spec has no HP or knockout field
— strong evidence HP lives in the per-game save state, not on the
figure — so any "full heal" affordance is a separate phase that
would drive RPCS3 via GUI automation (clear-slot → wait →
reload), not a `.sky` mutation. Full design log:
`/Users/chotchki/.claude/plans/federated-sleeping-mochi.md`.

**Research findings already in hand (informs the tasks below):**
- The single canonical level→XP table is the spec doc table at
  `:246-267`. No per-era variants for thresholds.
- XP is stored across three slots (2011 u24 at block 0x08, 2012 u16
  at block 0x11+0x03, 2013 u32 at block 0x11+0x08), and the game
  reads "current XP" as the **sum**. Slot caps (33 000 / 63 500 /
  ~103 035) define a fill-order: 2011 first, then 2012, then 2013.
- Data is mirrored across two regions per area, with a sequence
  byte determining the live region. The parser already handles
  this on the read path (`crates/sky-parser/src/lib.rs:701-721`);
  the **write path must write to the *other* (older-sequence)
  region with the sequence bumped** — not just to a fixed offset.
- Editability filter lives on the indexer's `Category` enum
  (`crates/core/src/figure.rs:250-271`), not the parser's
  `FigureKind` (which today only distinguishes `Trap` vs
  `Standard` and lumps Vehicles, Creation Crystals, Senseis all
  into `Standard` — too coarse for our gate).

- [ ] 11.1 **Spec nail-downs (research-first, no code).** Confirm
  CRC scopes, XP-table values, game-era detection, and kind
  predicate against real `.sky` dumps before any write code lands.
  - [x] 11.1.1 — ~~Confirm header CRC scope~~ **Done.** Header
    CRC at file `0x1E` covers only file bytes `0x00..0x1E`
    (figure_id, serial, error byte, trading card, variant) —
    independent of all data-area mutations. Verified empirically:
    parser code at `crates/sky-parser/src/lib.rs:619-620` matches
    the spec, validates cleanly against all 512 figures in the
    firmware pack post-11.1.7 fix. Edits to gold / XP / playtime
    / etc. do NOT require header-CRC recompute. Documented in the
    new **Write path notes** section of `SkylanderFormat.md`.
  - [x] 11.1.2 — ~~Enumerate CRC regions~~ **Done.** Full
    enumeration written up under **Write path notes** in
    `docs/research/sky-format/SkylanderFormat.md`. Key result for
    the level+gold pipeline: only 2 CRCs need recomputing per
    edit — the Region-A CRC14 (covers struct `0x00..0x0E`, picks
    up gold + XP_2011 + area-A sequence) and the Region-B 0x70-CRC
    (covers `0x72..0xB0`, picks up XP_2012 + XP_2013 + area-B
    sequence). The CRC30 at `B + 0x0C` and the big CRC at
    `B + 0x0A` are NOT touched by level+gold edits (they cover
    blocks holding nickname / heroic challenges / hat history /
    timestamps — none of which we mutate).
  - [x] 11.1.3 — ~~Lock in XP→level tables~~ **Done by spec doc.**
    The level→XP table at `docs/research/sky-format/SkylanderFormat.md:246-267`
    is canonical and global. 11.3 implements a single
    `LEVEL_THRESHOLDS: [u32; 21]` constant — no per-era variant.
  - [x] 11.1.4 — ~~Validate the slot-distribution algorithm~~
    **Effectively done by `validate_samples` against the real pack
    at `~/Games/ps3/skylanders/`** (151 figures, 141 with valid
    CRC). All figures with `xp_2011 = 33000` reported level ≥ 10;
    all Trap-Team-era figures with sum-of-slots crossing thresholds
    reported the expected level. Slot-distribution implicit in the
    parser's read-side logic is consistent with the planned
    write-side `distribute_xp`. See 11.1.7 for the per-generation
    nuance that this surfaced.
  - [x] 11.1.5 — ~~Spot-check editable-categories~~ **Confirmed
    against the real pack.** Trap IDs in the parser's range
    `0x0D2..=0x0DC` (60 entries) all classify as Trap; Vehicle IDs
    (e.g. Hot Streak `0x0C98`, Reef Ripper `0x0C96`, Sheep Creep
    `0x0C82`) fall through the parser's coarse `FigureKind` as
    `Standard` but the **indexer's `Category`** correctly puts them
    in `Vehicle`. Trap Master skylander figures
    (Blastermind `0x1D2`, Snap Shot `0x1CE`, etc.) classify as
    `Category::Figure` and are editable as expected. Imaginators
    Senseis (Crash `0x276`, Cortex `0x277`, Wild Storm `0x274`,
    King Pen `0x259`) are editable — these are playable
    characters. Adventure Packs (`0x131`..=`0x134` in this pack)
    classify as `Category::AdventurePack` and are correctly
    excluded.
  - [ ] 11.1.6 — **Per-generation level model.** The parser
    (`crates/sky-parser/src/lib.rs:797-806`) computes level from a
    per-generation XP pool, NOT from the spec's "sum of all
    experience values" rule: SSA/Giants figures use `xp_2011` only
    (cap = level 10); SwapForce uses `xp_2011 + xp_2012` (cap =
    level 15); TrapTeam/SuperChargers/Imaginators sum all three
    (cap = level 20). This is empirically what playtest will see:
    a Giants-era Tree Rex maxed in Imaginators still reads as
    level 10 to a Giants game. **Implications:**
    - `distribute_xp` needs a generation parameter to know which
      slot(s) to fill. Update 11.3.3 accordingly.
    - The phone-side level stepper must clamp to the per-figure
      max (10/15/20) — pull `SkyGeneration` from the parser's
      `variant_decoded.year_code` or from the indexer.
    - Doc this in the SPEC.md Q&A (11.10.1).
  - [x] 11.1.7 — ~~Blank-figure CRC failures~~ **Fixed.** Hypothesis
    confirmed via `cargo run --example crc_probe`: fresh dumps
    have entirely-zero data areas AND zero stored area-CRC bytes
    (game writes the CRC on the first mutation, not at
    manufacture). Added blank-area exemption to
    `parse_standard`'s checksum check at
    `crates/sky-parser/src/lib.rs:809-845`: if active region's
    14-byte header + 0x30 payload are all zero AND both stored
    CRCs are zero, accept as valid blank state. New unit test
    `blank_data_areas_with_valid_header_pass_checksum` covers it.
    Validation now reports **151/151 valid** on
    `~/Games/ps3/skylanders` (was 141/151) and **512/512 valid**
    on `~/Games/ps3/Skylanders Characters Pack for RPCS3`.
    Existing tamper tests (`header_crc_tamper_flips_valid_flag`,
    `area_crc_tamper_flips_valid_flag`) still pass — the fix
    only relaxes the blank case, not the populated-but-wrong
    case.

- [x] 11.2 **`sky-parser` write path** — **Done.** Added to
  `crates/sky-parser/src/lib.rs` adjacent to the read-side area
  helpers. All mutators are area-sequence-aware: each picks the
  older mirror, copies the active mirror's data wholesale to the
  target, applies the field mutation, bumps the sequence byte,
  and recomputes the affected CRC for the target only. CRC
  recompute is encapsulated inside each mutator — callers can't
  forget, which makes the planned separate `recompute_crcs` entry
  point unnecessary (deviation from plan, simpler API). 5 new
  unit tests added (38/38 sky-parser tests pass; 151/151 real
  `.sky` dumps still validate).
  - [x] 11.2.1 — `encrypt_figure` was already `pub` at
    `crates/sky-parser/src/lib.rs:150` from earlier work; no
    promotion needed.
  - [x] 11.2.2 — `pub fn pick_write_region(plain: &[u8]) ->
    WriteRegion` added. Inverts the parser's read-side pick at
    `:701-721`. Returns `{region_a_dst_base, region_b_dst_base,
    next_seq_a, next_seq_b}`. Test `pick_write_region_inverts_read_side_pick`
    covers fresh / asymmetric / wraparound cases.
  - [x] 11.2.3 — `pub fn set_gold(plain: &mut [u8;
    SKY_FILE_LEN], gold: u16)` added. Test `set_gold_round_trip_preserves_other_fields`
    confirms gold mutates and CRC30-covered fields (heroic
    challenges, nickname, etc.) remain intact.
  - [x] 11.2.4 — `pub fn set_xp(plain: &mut [u8; SKY_FILE_LEN],
    slots: SlotXp)` added. Takes pre-computed `SlotXp` rather
    than `(total_xp, generation)` — keeps `set_xp` byte-twiddling
    only; the generation-aware distribution happens at the
    caller via `distribute_xp` (11.3.3). Test
    `set_xp_round_trip_writes_all_three_slots` confirms slot
    values land correctly and pre-existing other-slot progress
    is wiped per the documented MVP tradeoff.
  - [x] 11.2.5 — ~~Separate `recompute_crcs` entry point~~ **Not
    needed.** CRC recompute is encapsulated inside `set_gold` /
    `set_xp` — they each handle their own affected CRC (CRC14
    for Region A in `set_gold`; CRC14 + Region-B 0x70-CRC in
    `set_xp`). The unchanged-by-edit CRCs (CRC30 at `B+0x0C`,
    big-CRC at `B+0x0A`) ride along correctly because the
    copy-then-mutate pattern preserves them with their original
    data.
  - [x] 11.2.6 — Round-trip property tests:
    `set_gold_round_trip_preserves_other_fields` and
    `set_xp_round_trip_writes_all_three_slots` both follow the
    `decrypt → mutate → encrypt → parse` flow and assert mutated
    field landed + unrelated fields preserved + checksums valid.
  - [x] 11.2.7 — Multi-write idempotence test
    `multi_write_idempotence_lands_in_alternating_mirrors` runs
    five back-to-back `set_gold` calls with different values and
    asserts (a) the final value is what the parser reads (b) the
    older mirror holds the *previous* value, not the original.
    Confirms the area-sequence dance works across repeated writes.
    Plus `set_gold_then_set_xp_composes_cleanly` exercises the
    cross-function composition case (each mutator independently
    picking its write target after the other ran).
  - [x] 11.2.8 — ~~Golden-byte test against a real `.sky` dump~~
    **Skipped** per CLAUDE.md's no-`.sky`-in-repo rule
    (committing a real dump would violate the no-piracy
    distribution policy). The round-trip + idempotence tests
    above plus the 151/151 + 512/512 `validate_samples` reality
    check together cover the same ground without the brittle
    byte-equality coupling. If we need higher-confidence
    real-file coverage, the natural home is an env-gated
    integration test that points at a user-supplied dump path —
    deferrable until / unless a regression slips through.

- [x] 11.3 **Level ↔ XP mapping** — **Done.** Added to
  `crates/sky-parser/src/lib.rs` alongside the existing
  `level_from_xp` (rather than a separate `xp.rs` module — kept the
  existing single-file layout convention; the additions are ~100
  lines so a new file wasn't justified). Single global threshold
  table; per-slot fill order in `distribute_xp`. 6 new unit tests
  added (33/33 sky-parser tests pass).
  - [x] 11.3.1 — `pub const LEVEL_THRESHOLDS: [u32; 20]` populated
    from spec doc `:246-267`. (Plan said `[u32; 21]` with index 0
    unused; the actual layout uses 20 entries indexed by
    `level - 1` to match the pre-existing inline constant — no
    functional difference, less wasted space.)
  - [x] 11.3.2 — `pub fn xp_for_level(level: u8) -> u32` added.
    The existing `level_from_xp(xp: u32) -> u8` already serves
    the inverse direction; kept its name to avoid call-site
    churn (plan called it `level_for_xp` — minor variance).
  - [x] 11.3.3 — `pub fn distribute_xp(total: u32, generation:
    SkyGeneration) -> SlotXp` with per-generation slot rules. Note:
    plan called the parameter `gen`; that's now a reserved keyword
    in Rust 2024 edition, so it's `generation`. Semantics: slots
    not used by the figure's generation get zero (set_xp will
    write all three slots, which means setting level on a
    Giants-era figure wipes any progress from later games it's
    been played in — documented as intentional MVP tradeoff, will
    surface in SPEC.md Q&A in 11.10.1).
  - [x] 11.3.4 — `pub fn max_level_for(generation: SkyGeneration)
    -> u8` returns 10 / 15 / 20. `Unknown` is permissive (treated
    as 20) — never block the user on a figure we couldn't classify.
  - [x] 11.3.5 — Unit tests added: `xp_for_level_round_trips_against_level_from_xp`
    (monotonicity + boundary + clamp), `max_level_for_matches_generation_caps`,
    `distribute_xp_ssa_caps_at_2011_slot`, `distribute_xp_swap_force_cascades_into_2012_slot`,
    `distribute_xp_trap_team_plus_uses_all_three_slots`, and
    `distribute_xp_round_trips_through_level_from_xp_for_target_gen`
    (the big one — for every reachable level in every generation,
    confirms the slot distribution sums to the target AND the
    parser's per-gen level computation reads back the original
    level).

- [x] 11.4 **Server edit endpoint** — **Done.** New module
  `crates/server/src/sky_edit.rs`, feature-gated behind `sky-stats`
  (matches the read-side stats endpoint). Route registered in
  `http.rs:272-283`. Builds clean; workspace tests all green.
  - [x] 11.4.1 — `POST /api/profiles/:profile_id/figures/
    :figure_id/edit` mounted alongside existing per-figure routes.
    Body: `EditBody { level: u8, gold: u16 }`.
  - [x] 11.4.2 — Validation chain implemented in order:
    catalog-lookup → 404; `Category` check → 422; portal-occupancy
    check (`state.portal.lock().await` — tokio Mutex, not std) →
    409; level range (clamped to `max_level_for(generation)`) →
    422. Per-generation max comes from the figure's `GameOfOrigin`
    via the new `game_to_generation` helper.
  - [x] 11.4.3 — Mutation pipeline: `working_copies::resolve_load_path`
    (forks from pack on first edit) → `tokio::fs::read` → in-place
    decrypt → `set_gold` + `set_xp` (area-aware per 11.2) → in-place
    encrypt → atomic write via tmp file + `tokio::fs::rename`. Best-effort
    cleanup of the tmp file on rename failure.
  - [x] 11.4.4 — Broadcasts `Event::FigureUpdated { figure_id, level,
    gold }` via `state.events.send(...)`. Silent on no subscribers
    (the `let _ =` swallows the SendError that happens when no
    `/ws` listeners exist — non-fatal).
  - [x] 11.4.5 / 11.4.6 — **Deferred to 11.9 e2e.** The existing
    server-side integration test pattern
    (`crates/server/tests/profiles.rs:6-7`) explicitly avoids HTTP
    plumbing ("the HTTP plumbing is covered by the e2e suite").
    AppState construction is too heavy to stand up for a focused
    unit test (database + broadcast channels + indexer pack scan
    all required). The unit-testable bits — `game_to_generation`
    mapping — are covered by `sky_edit::tests::game_to_generation_covers_all_arms`.
    The HTTP-level behaviour (4xx for each validation failure,
    202 on success, working-copy bytes match expected after edit)
    lands in 11.9 alongside the chromedriver flow.

- [x] 11.5 **Protocol additions** — **Done.** Both variants added
  to `crates/core/src/protocol.rs`; serde round-trip tests added
  to `crates/core/src/lib.rs::tests`. 11/11 core tests pass.
  - [x] 11.5.1 — `Command::EditFigure { figure_id, level, gold }`
    added with the standard `#[serde(tag = "kind",
    rename_all = "snake_case")]` envelope. Test
    `command_discriminants` extended to cover the new variant.
  - [x] 11.5.2 — `Event::FigureUpdated { figure_id, level, gold }`
    added.
  - [x] 11.5.3 — `event_figure_updated_roundtrip` test added.

- [x] 11.6 **Phone UI: enable stats button** — **Done.**
  Modifications in `phone/src/screens/figure_detail.rs`. Phone WASM
  bundle builds clean; workspace tests all green.
  - [x] 11.6.1 — Removed `disabled=true` on the STATS button;
    wired `on:click` to flip a local `show_edit_sheet: RwSignal<bool>`.
    Sheet renders conditionally inside the figure-detail view.
  - [x] 11.6.2 — `stats_editable = editable_category && !on_portal`
    as a derived signal. Disabled button gets an `aria-label` +
    per-case `title` tooltip ("Editing not supported for {category}"
    / "Remove from portal before editing" / "Edit level + gold").
  - [x] 11.6.3 — The `on_portal` signal `.get()`s the portal
    RwSignal, so SlotChanged broadcasts that update the portal
    state automatically re-evaluate `stats_editable`. No extra
    subscription wiring needed.
  - [x] 11.6.4 — Single-phone refresh handled via a local
    `stats_rev: RwSignal<u32>` that the edit sheet bumps on save;
    the stats LocalResource depends on it and re-fetches. The
    multi-phone cross-session refresh via the `FigureUpdated`
    broadcast event lands as a follow-up — for now `ws.rs` just
    logs the event (the variant is wired through phone Event so
    WS deserialization doesn't fail).

- [x] 11.7 **Phone UI: edit sheet** — **Done.**
  `phone/src/screens/figure_edit_sheet.rs` is the new component;
  `phone/styles/components/figure_edit_sheet.css` ships the
  styling (imported via `phone/styles/input.css`).
  - [x] 11.7.1 — `FigureEditSheet` component takes
    `figure_name / profile_id / figure_id / initial_level /
    initial_gold / max_level / on_close / on_saved`. Seeds from
    current stats or `(1, 0)` if no working copy yet (first edit
    forks from pack server-side via `working_copies::resolve_load_path`).
  - [x] 11.7.2 — LEVEL stepper: ± buttons, step 1, clamped to
    `1..=max_level` (per-generation, 10 / 15 / 20). GOLD stepper:
    five buttons — `«` `−` value `+` `»` — small chevrons step
    1000, main buttons step 100. Avoided long-press handling
    (simpler + works the same with rapid taps).
  - [x] 11.7.3 — SAVE → `post_edit_figure` → on 202, fires
    `on_saved` (bumps `stats_rev`) + `on_close` (hides sheet).
    CANCEL → `on_close` only.
  - [x] 11.7.4 — 4xx errors render as `.edit-error` text inside
    the sheet (instead of a toast — sheet stays open with current
    edits intact so user can correct + retry). `saving` flag
    resets on error; SAVE re-enables.
  - [x] 11.7.5 — CSS follows the resume_modal.css pattern:
    `@apply` for layout primitives, raw `box-shadow` stacks +
    linear gradients for the gold-bezel buttons and panel framing.
    Safe-area-aware top/bottom padding so the sheet doesn't
    collide with iPhone Dynamic Island / home indicator.

- [ ] 11.8 **Aesthetic polish.**
  - [ ] 11.8.1 — Mock first: `docs/aesthetic/mocks/
    figure-edit-sheet.html` per the "mocks vs code" feedback
    memory. Add to `docs/aesthetic/mocks/index.html` under the
    figure-detail flow group.
  - [ ] 11.8.2 — Match Skylanders aesthetic: starfield bg behind
    sheet (or semitransparent dim of detail screen), gold-bezel
    around stepper values, bold-white-gold-outline numbers.
    Reuse `gold_bezel.css` patterns.
  - [ ] 11.8.3 — Validate on real iPhone (Mac → Bonjour → iPhone
    Safari) per the post-Tailwind Mac-validation memory. Check
    safe-area-insets if sheet pins to top/bottom edges.
  - [ ] 11.8.4 — Multi-device sim parity via `tools/ios-inspect` —
    iPad + iPhone simulator at the same time, confirm the sheet
    renders correctly at both viewport sizes.

- [x] 11.9 **E2E tests** — **Done.** New file
  `crates/e2e-tests/tests/figure_edit.rs` with two chromedriver
  tests. Both pass locally against a freshly-spawned `TestServer`
  + chromedriver-148. Run with
  `CHROMEDRIVER=… cargo test -p skylander-e2e-tests --test figure_edit -- --ignored`
  (the `--ignored` opt-in mirrors the rest of the e2e suite —
  these don't run in CI, per CLAUDE.md "Testing" section).
  - [x] 11.9.1 — `edit_level_and_gold_round_trips_through_stats_strip`:
    open a Spyro-family detail screen → tap STATS → bump level to
    5 (four `+` taps) → bump gold to 300 (three `+100` taps) →
    SAVE → assert sheet closes → assert stats-strip LEVEL +
    GOLD cells refresh to the new values. Server log line
    "edited working copy ... level=5 gold=300" confirms the
    server side of the round trip.
  - [x] 11.9.2 — `stats_button_disabled_while_figure_on_portal`:
    place a Spyro card → re-open the same figure's detail → assert
    the STATS button has the `disabled` attribute AND a `title`
    tooltip that mentions "portal".

- [x] 11.10 **Docs** — **Done.**
  - [x] 11.10.1 — SPEC.md gained a "Round 7 — Stat Editing
    (PLAN 11)" Q&A section: scope + gating decisions,
    per-generation level cap rationale (with the Tree Rex
    empirical reference), edit semantics (set-N writes flat,
    wipes unused-by-generation slots), healing/knockout
    deferral, and cross-phone refresh deferral.
  - [x] 11.10.2 — Already landed during 11.1.7 + 11.1.2:
    `docs/research/sky-format/SkylanderFormat.md` has both
    a "Blank-tag state (factory-fresh figures)" section
    (read-side exemption rule) and a "Write path notes"
    section (CRC region map + per-mutation recompute table +
    area-sequence inversion rule).
  - [x] 11.10.3 — `README.md` "Latest release" line + new
    `docs/features.md` "Stat editing (v1.5.0)" section. Both
    pitched as the playtest-friction-killer they actually are.

- [x] 11.11 **Hotfix: stat-edit write path rejected by RPCS3** —
  **Fixed.** Root cause was a parser-level round-trip bug in
  `encrypt_figure`: it skipped blocks whose plaintext was all-zero
  on the assumption those blocks were also zero in ciphertext. Real
  `.sky` dumps have a third category — blocks the game wrote with
  empty field data (empty nickname / unused hat slot / etc.) where
  the ciphertext is `AES_encrypt(key, zeros)` (non-zero bytes that
  decrypt back to zero). Skipping those in re-encrypt produced
  all-zero ciphertext at positions where the game expected real
  AES output, which RPCS3 rejected. Fix: new
  `encrypt_figure_preserving_unwritten(plain, source_cipher)` uses
  the original ciphertext (not the mutated plaintext) to decide
  per-block whether to skip. Server's edit endpoint and `edit_diff`
  example both switched. Legacy `encrypt_figure` kept for fixtures.
  - [x] 11.11.1 — Empirical reproduction: new `edit_diff` example
    (`crates/sky-parser/examples/edit_diff.rs`) reads a real `.sky`,
    applies the edit pipeline, and diffs plaintext + ciphertext
    against the source. Surfaced the round-trip failure cleanly —
    "decrypt → encrypt of the source file does NOT match the
    source bytes, 128 mismatched bytes."
  - [x] 11.11.2 — ~~Byte-diff against known-good~~ **Not needed.**
    The round-trip diagnostic (11.11.1) pinpointed the bug at the
    parser layer without needing a known-good game-produced
    reference file. User had a controller ready for play-to-level-2
    diff as a fallback diagnostic; no longer required.
  - [x] 11.11.3 — ~~Suspect inventory~~ **Obsoleted by 11.11.1.**
    Actual cause wasn't any of the listed CRC / sector-trailer /
    hidden-field hypotheses — it was the asymmetric encrypt skip.
    All listed suspects (a)–(e) cleared.
  - [x] 11.11.4 — Fix + regression tests. Two new unit tests in
    `crates/sky-parser/src/lib.rs::tests`:
    `encrypt_preserving_unwritten_round_trips_written_empty_block`
    constructs the exact failure-mode scenario (manually
    encrypting an all-zero plaintext block to simulate the game's
    real ciphertext) and asserts the new path round-trips
    byte-identically; `encrypt_figure_legacy_loses_written_empty_blocks`
    documents the legacy skip behaviour as the failure mode the
    new path fixes. 40/40 sky-parser tests pass; 151/151 real
    `.sky` dumps still validate.

- [x] 11.12 **Wire up the RESET button** — **Done.** Hand-edit
  pattern same as STATS in 11.6: same enable conditions
  (editable category + off-portal), same per-case tooltip.
  Reuses the existing `ResetConfirmModal` (extended
  `ResetTarget` with `Option<u8>` slot + a required
  `profile_id`); modal branches between `post_reset(slot, …)`
  and `post_reset_figure(profile_id, …)` based on slot context.
  No new modal component needed.
  - [x] 11.12.1 — `post_reset_figure(profile_id, figure_id)`
    added to `phone/src/api.rs:391-403`, mirrors
    `post_edit_figure` shape.
  - [x] 11.12.2 — `reset_figure` handler added to
    `crates/server/src/sky_edit.rs`; route mounted in `http.rs`
    at `POST /api/profiles/:profile_id/figures/:figure_id/reset`.
    Same validation chain as edit (catalog 404, category 422,
    portal-occupancy 409). Calls `working_copies::reset_to_fresh`.
  - [x] 11.12.3 — `phone/src/screens/figure_detail.rs` RESET
    button: removed `disabled=true`, on-click sets
    `reset_target` with `slot: None` + the profile + figure ids;
    the app-root `ResetConfirmModal` handles the rest with its
    hold-to-confirm gesture + drain animation.
  - [x] 11.12.4 — Server broadcasts `Event::FigureUpdated {
    figure_id, level: 1, gold: 0 }` on success. **Phone-side
    auto-refresh of the stats strip post-reset is deferred to
    v1.5.2** — same as the cross-phone refresh deferral from
    11.6.4. Workaround for v1.5.1: user backs out of detail
    + re-enters to see the pack-fresh stats.

- [x] 11.13 **Edit sheet UX hotfix.** Two real-world bugs from v1.5.1 play
  testing: (a) iOS Safari's long-press selection callout fired on
  any label in the sheet (heading, stepper labels, value, "max N"
  caption) — toddlers tap-mashing the steppers were triggering the
  "Look Up / Translate" popup. Fix: `user-select: none` +
  `-webkit-touch-callout: none` on `.edit-panel` — the whole sheet
  is button-driven, no text needs to be selectable. (b) Gold near
  `u16::MAX` caused Skylanders games to reject the figure on load
  (observed at 65535). Capped UI at `GOLD_MAX = 65000` and
  repurposed the outer `<<` / `>>` chevrons as jump-to-bounds
  buttons (0 and 65000 respectively) instead of ±1000 steppers.
  Aria-labels updated accordingly. New e2e
  `gold_chevrons_jump_to_bounds_and_max_round_trips` in
  `crates/e2e-tests/tests/figure_edit.rs`: one `>>` tap → 65000
  (asserts both the chevron + inner `+100` disable), one `<<` tap
  → 0, save round-trips 65000 through the server's edit endpoint
  + stats strip. Also smoke-tests `.edit-panel`'s computed
  `user-select` for the (a) fix — chromedriver can't render iOS's
  long-press callout but it can verify the CSS layer that blocks
  it.

- [x] 11.14 **Wire FigureUpdated WS broadcast to figure_detail
  refresh + add save toast.** Server-side broadcast was correct
  (`crates/server/src/sky_edit.rs:167`); phone WS handler was
  log-only (`phone/src/ws.rs:360`), so cross-phone refresh + the
  defense-in-depth path for the local same-phone race never
  fired. Adds a `figure_updates_rev: RwSignal<u32>` that ws.rs
  bumps on every `Event::FigureUpdated`; figure_detail's stats
  LocalResource subscribes to it alongside the local
  `stats_rev`. Plus a "Stats saved" toast on edit-sheet save
  success so the user gets instant confirmation independent of
  the LocalResource refresh latency.

- [x] 11.15 **Gate ResumeModal on current_game.is_some().** Server
  emits `Event::ResumePrompt` on profile unlock
  (`crates/server/src/http.rs:1870`) — before the user picks +
  launches a game, so RPCS3 isn't running yet. Tapping RESUME
  fires `post_load`s that 500 because the driver isn't ready.
  Phone-side fix: extend the `<Show>` predicate on the
  ResumeModal mount (`phone/src/lib.rs:418`) with `&&
  current_game.get().is_some()` so the modal queues until the
  game actually launches, then surfaces.

- [x] 11.16 **Wire the APPEARANCE button on figure detail.** Button
  has been `disabled=true` placeholder since the detail screen
  shipped (`phone/src/screens/figure_detail.rs:307-315`). Browser
  already implements variant cycling via the per-card N-variants
  badge; bring the same affordance to the detail screen so a user
  who taps a figure to inspect it can also cycle to its other
  reposes / skins without backing out to the toy box. Mechanism:
  Browser computes the sorted variant cluster for the
  currently-selected figure and hands FigureDetail a `siblings`
  list + an `on_pick_variant` callback. APPEARANCE disabled when
  cluster size ≤ 1; on click, picks the next variant in order
  (base first, then alphabetical by `variant_tag`, same as the
  toy-box card cycling). Selecting a new variant re-mounts
  FigureDetail with the new figure_id — fresh stats fetch, fresh
  edit/reset enablement evaluation.

- [x] 11.17 **Doc sweep — README + project site brought up to
  v1.6.0 reality.** README pointed at v1.5.1, `docs/index.md`
  status callout still said "v1.1.0 shipped" with v1.1.0-only
  feature list, `docs/roadmap.md` had no Phase 11/12/13/14
  sections, `docs/features.md` "Stat editing" claimed gold
  spans the full `u16` (0–65535) with ±100/±1000 chevrons
  (both wrong post-11.13), didn't mention the wired APPEARANCE
  button (11.16) or the resume-modal game-launch gate (11.15).
  All four files rewritten to current state. Screenshot regen
  attempted via `screenshot_tour` e2e but the test is currently
  red on a `.stale-body` overlay intercepting clicks
  (`phone/src/components/stale_version.rs` hard-modal triggers
  during the harness's test-hook session bind, predates the
  v1.6.0 changes). Screenshot refresh deferred to **Phase 15
  play-through harness**, which will replace the static
  screenshot tour with the side-by-side narrated capture flow
  anyway.

## Phase 12 - Real Mac driver (AXUIElement)

macOS originally shipped as mock-driver-only — CLAUDE.md's "macOS
support" section explicitly calls a real Mac driver "an explicit
non-goal." PLAN 11's v1.5.0 ship inverted the value calc: every
write-path bug requires a Mac → Windows zip round-trip to validate,
and 11.11 surfaced exactly how painful that is. AXUIElement is
macOS's accessibility API — Qt apps generally expose accessibility
on both platforms, so the Windows UIA driver's general shape should
port. **Status of AX exposure in RPCS3 is unknown until we probe**
(12.1.2 is the go/no-go gate).

If AX doesn't work, fallback paths are: (a) AppleScript via
`osascript` (cruder, less reliable); (b) keystroke synthesis via
`cliclick` (Steam-Overlay-vulnerable, same class of problem the
Windows driver moved away from in CLAUDE.md "RPCS3 window/menu
gotchas"). Either fallback is its own meaningful detour; flag it at
12.1.2 outcome.

## Phase 13 - winget distribution

Goal: ship the Windows MSI through `winget` so `winget install
ChristopherHotchkiss.SkylanderPortalController` is the primary
install path. Solves the SmartScreen "unknown publisher" problem
without an Authenticode cert — winget runs as a trusted authority
and bypasses the manual "More info → Run anyway" gate that plain
MSI downloads hit. Per-machine scope retained (matches the v1.5.1
install layout so the upgrade path stays clean); UAC prompt is
acceptable because winget elevates automatically. Decisions locked
2026-05-24: keep perMachine, auto-PR via `winget-releaser`, defer
Authenticode signing indefinitely as superseded by winget.

- [x] 13.1 **Pre-flight: MSI meets winget submission criteria.**
  - [x] 13.1.1 Confirmed `wix/main.wxs` already has the required
    identifiers — verified inline: stable `UpgradeCode`
    `E6FC979F-32C8-48C9-92BF-5CFFDF544D5E`, `Manufacturer
    'Christopher Hotchkiss'`, `InstallScope='perMachine'`,
    `ProductVersion` from `CARGO_PKG_VERSION` via build script.
    No wxs changes required.
  - [x] 13.1.2 Package Identifier locked in:
    `ChristopherHotchkiss.SkylanderPortalController` (winget
    convention: `<Publisher>.<PackageName>`, PascalCase, no
    spaces). License: **Unlicense** (per `LICENSE` — public
    domain dedication, not MIT as the earlier plan draft
    hand-waved).
  - [x] 13.1.3 Released MSI confirmed publicly downloadable +
    introspectable. Bootstrap target: **v1.5.1**, asset
    `skylander-portal-controller-1.5.1-windows-x86_64.msi`,
    SHA-256 `be8f250b09415369d13794427b7043ab0a32f1277a9d3096e8b7f6f336c97eb3`,
    21.5 MB. URL:
    `https://github.com/chotchki/skylander-portal-controller/releases/download/v1.5.1/skylander-portal-controller-1.5.1-windows-x86_64.msi`.
    MSI metadata (ProductCode etc.) gets introspected by
    `wingetcreate new` in 13.2.2; `winget validate` operates
    on YAML manifests so it lands later in 13.2.3 against the
    scaffolded output. Note: v1.5.1 is still flagged
    `prerelease: true` in GitHub (the `prerelease=false` flip
    lands on the next tag); winget moderation accepts
    prerelease packages, so this doesn't gate submission.

  - [x] 13.1.4 Relicense from Unlicense to MIT for winget moderation friendliness.
- [ ] 13.2 **Manifest authoring (first submission).** Step-by-step
  HTPC runbook: `docs/dev/winget-submission.md` — exact prompts,
  expected outputs, troubleshooting. Work it cold when HTPC
  keyboard time opens up.
  - [ ] 13.2.1 Install `wingetcreate` on the HTPC
    (`winget install Microsoft.WingetCreate`).
  - [ ] 13.2.2 `wingetcreate new <release-asset-url>` to scaffold
    the three-file manifest (installer.yaml, locale.yaml,
    version.yaml). Hand-tune: ShortDescription (≤256 chars),
    Description, Tags (`skylanders`, `rpcs3`, `portal`,
    `emulator`), License (MIT — matches LICENSE file),
    LicenseUrl, PublisherUrl, PackageUrl (GitHub repo),
    ReleaseNotesUrl (release page).
  - [ ] 13.2.3 Local install test:
    `winget install --manifest .\manifests\c\ChristopherHotchkiss\SkylanderPortalController\<version>\`.
    Confirm Start Menu shortcut, `%ProgramFiles%` install
    location, `data\` dir present, server launches via the
    shortcut + RPCS3 menu nav still works end-to-end.

- [ ] 13.3 **First PR to microsoft/winget-pkgs.**
  - [ ] 13.3.1 Fork `microsoft/winget-pkgs`, push the manifest
    tree, open the PR. Title format: `New version:
    ChristopherHotchkiss.SkylanderPortalController <version>`.
  - [ ] 13.3.2 Address moderation feedback (typical SLA 1–7
    days). Common asks: tighten ShortDescription, normalize
    Tags, fix LicenseUrl 404s. **Don't** start 13.4 until 13.3
    merges — auto-update can't bootstrap a non-existent package.
  - [ ] 13.3.3 Post-merge smoke test:
    `winget install ChristopherHotchkiss.SkylanderPortalController`
    on a clean Windows install (or `winget uninstall` + reinstall
    on the HTPC). Confirm install location, shortcuts, and that
    `winget list` reports the package version correctly.

- [ ] 13.4 **CI auto-PR for subsequent releases.**
  - [ ] 13.4.1 Generate a fine-grained PAT scoped to a personal
    fork of `microsoft/winget-pkgs` (scope: `Contents: read+write`
    + `Pull requests: read+write` on the fork only). Store as
    repo secret `WINGET_PAT`. Rotation: 90-day expiry; calendar
    reminder is the only safety net (no auto-rotation).
  - [ ] 13.4.2 Add `vedantmgoyal9/winget-releaser@latest` step
    to `release.yml`'s `build-windows-release` job, gated on
    `startsWith(github.ref, 'refs/tags/v')`, runs after the
    `softprops/action-gh-release` step (the action needs the
    release assets to exist before it computes the installer
    hash). Inputs: `identifier`, `installers-regex` matching the
    MSI asset name pattern, `token: ${{ secrets.WINGET_PAT }}`.
  - [ ] 13.4.3 Smoke-test on the next real tag (v1.6.0 or later).
    Verify the auto-PR appears within ~5min of the release going
    live, merges through moderation cleanly, and that
    `winget upgrade` on the HTPC picks up the new version.

- [ ] 13.5 **Docs + supersede the disabled-signing block.**
  - [ ] 13.5.1 `README.md` Install section: `winget install
    ChristopherHotchkiss.SkylanderPortalController` as the
    primary path; direct GitHub Release MSI as fallback for
    Windows 10 < 1809 (winget unavailable there) and for users
    who'd rather not use the package manager.
  - [ ] 13.5.2 Mark PLAN 10.9.2 Windows Authenticode section as
    **superseded by Phase 13** — winget eliminates the
    SmartScreen friction that was the original motivation. The
    macOS Tier-2 Developer ID/notarization half of 10.9.2 is
    unaffected (it's still blocked on user-provisioned secrets).
  - [ ] 13.5.3 Update the disabled-signing comment in
    `release.yml` to reference winget as the primary path,
    rather than the noncommittal "future signing work" framing.
    The mac signing steps stay `if: false` (different OS, different
    solution).
  - [ ] 13.5.4 CLAUDE.md "Distribution" section: replace the
    GitHub Releases bullet with the winget-first message, keep
    the no-RPCS3/no-.sky non-goals.

- [ ] 13.6 **Risks + fallbacks.**
  - [ ] 13.6.1 Moderation rejection (publisher conflict, missing
    metadata, license URL 404). Mitigation: GitHub Release MSI
    download stays as permanent fallback. Document the manual
    install path in README regardless of winget outcome.
  - [ ] 13.6.2 PAT rotation lapse → auto-PR step fails silently.
    Mitigation: 90-day calendar reminder; release.yml step
    failure should surface as a CI red so the next tag catches
    a stale token within one release cycle.
  - [ ] 13.6.3 winget moderation latency on hotfix tags
    (e.g. v1.5.2-style emergency ship). Mitigation: direct MSI
    download stays the immediate path; winget catches up on its
    own SLA. Don't gate a hotfix on the winget PR landing.

## Phase 14 - macOS code-signing bring-up (walkthrough)

Goal: unblock PLAN 10.9.2's macOS Tier-2 path by provisioning the
Apple Developer cert + the 7 GitHub Environment secrets it depends
on, then flipping the four `if: false` guards back on and verifying
a tag push produces a signed + notarized + stapled `.dmg` that
Gatekeeper accepts without the "unknown developer" prompt. Most of
this phase is Chris executing steps on his Mac + GitHub UI; the
code path (release.yml, tools/build-macos-app.sh,
docs/dev/release-signing.md) already exists and is the canonical
reference. This PLAN walks the order + the verification gates so
the eventual flip-the-switch tag-push has zero surprises.

## Non-goals
- No bundling of RPCS3 or `.sky` files (piracy concern).
- No Linux support — production targets are Windows + macOS. Both
- No user-entered figure names.
- No audio (text-only Kaos to dodge copyright).
- No live wiki scraping at runtime — data is committed to the repo.
## Risks (live list — update as we learn)
- **R1:** UI Automation may not expose enough of the RPCS3 Qt dialog to drive it reliably. Resolved: Alt-keyboard-nav workaround (CLAUDE.md "RPCS3 window/menu gotchas").
- **R2:** "Move portal dialog off-screen" may be blocked by Windows. Resolved: Win32 `SetWindowPos` works; `hide_dialog_offscreen` + RAII guard in `crates/rpcs3-control/src/hide.rs`.
- **R3:** Wiki search hit rate might be below 80%. Resolved: 504/504 coverage (3.19.5).
- **R4:** Leptos touch/mobile UX may prove rough. Mitigation: ongoing Phase 4.18 on-device iteration; PWA install fallback.
- [ ] 14.1 **Apple Developer prereqs (on the Mac).**
  - [ ] 14.1.1 Confirm Apple Developer membership active at
    <https://developer.apple.com> → Membership. Capture the
    10-char Team ID — feeds `APPLE_TEAM_ID` in 14.3.2.
  - [ ] 14.1.2 Generate (or confirm existence of) "Developer ID
    Application" cert. Xcode → Settings → Accounts → Manage
    Certificates → `+` → Developer ID Application. Cert lands in
    the login keychain. Distinct from the "Apple Development"
    cert Xcode auto-creates for local builds — the Developer ID
    flavor is what Gatekeeper trusts.
  - [ ] 14.1.3 Generate an app-specific password at
    <https://appleid.apple.com> → Sign-in and Security →
    App-Specific Passwords. Label `spc-notarize`. Copy
    immediately — Apple shows it once. Feeds `APPLE_APP_PASSWORD`.

- [ ] 14.2 **Export cert + collect secret values.**
  - [ ] 14.2.1 Keychain Access → My Certificates → right-click
    `Developer ID Application: Christopher Hotchkiss (TEAMID)`
    → Export. Format: Personal Information Exchange (`.p12`).
    Set a strong passphrase (`MACOS_CERT_PASSWORD`).
  - [ ] 14.2.2 `base64 -i <export.p12> | pbcopy` — paste
    into `MACOS_CERT_P12_BASE64`. Stash the passphrase in
    1Password (or equivalent) before moving on.
  - [ ] 14.2.3 `security find-identity -v -p codesigning` — copy
    the full `Developer ID Application: Christopher Hotchkiss
    (TEAMID)` line into `MACOS_CERT_IDENTITY`. Has to match
    exactly; the `(TEAMID)` suffix matters.
  - [ ] 14.2.4 `openssl rand -base64 32` → throwaway
    `KEYCHAIN_PASSWORD` for the CI keychain. Doesn't need to be
    memorable, just unique per credential rotation.

- [ ] 14.3 **GitHub Environment + 7 secrets.**
  - [ ] 14.3.1 Repo Settings → Environments → New environment →
    name `release`. Deployment tag rule: Selected → `v*.*.*`.
    Optional: add yourself as Required Reviewer for a manual
    gate before the macOS job runs (useful if surprise
    tag-pushes are a concern).
  - [ ] 14.3.2 Add 7 secrets per the table in
    `docs/dev/release-signing.md`: `MACOS_CERT_P12_BASE64`,
    `MACOS_CERT_PASSWORD`, `MACOS_CERT_IDENTITY`,
    `KEYCHAIN_PASSWORD`, `APPLE_ID` (chris@hotchkiss.io),
    `APPLE_APP_PASSWORD`, `APPLE_TEAM_ID`. Confirm the
    environment shows "7 secrets" after.
  - [ ] 14.3.3 Delete the local `.p12` export — no longer
    needed once the secret's saved. The cert itself stays in
    your login keychain for local builds.

- [ ] 14.4 **Local end-to-end dry run (catches mistakes
  before CI does).**
  - [ ] 14.4.1 `SIGN_IDENTITY="Developer ID Application:
    Christopher Hotchkiss (TEAMID)" tools/build-macos-app.sh` —
    should produce a signed `dist/Skylander-Portal-Controller-*.dmg`.
    `codesign -dvv dist/*.dmg` reports the signature info.
  - [ ] 14.4.2 `xcrun notarytool submit dist/*.dmg --apple-id
    chris@hotchkiss.io --team-id TEAMID --password
    <app-specific> --wait` — catches team-id / password
    mismatches locally. Expected runtime ~2 min for a ~30MB dmg.
  - [ ] 14.4.3 `xcrun stapler staple dist/*.dmg` + `spctl -a -t
    open --context context:primary-signature -vv dist/*.dmg`.
    Green here = green in CI.

- [ ] 14.5 **Re-enable the disabled CI steps.**
  - [ ] 14.5.1 Flip the four `if: false` guards in `release.yml`
    back to `if: github.repository ==
    'chotchki/skylander-portal-controller'`. Steps: "Import
    Developer ID certificate", "Build signed .app + .dmg",
    "Notarize and staple .dmg", "Upload signed DMG artifact".
  - [ ] 14.5.2 Add `${{ env.RELEASE_DMG }}` back to the
    macOS-lane "Publish release" step's `files:` list (removed
    in PLAN 11.13's signing-disable patch).
  - [ ] 14.5.3 Update the disabled-signing comment block in
    `release.yml` — drop the "secrets pending" framing,
    reference `docs/dev/release-signing.md` as the live runbook.

- [ ] 14.6 **CI dry run via `workflow_dispatch`.**
  - [ ] 14.6.1 Actions tab → Release → Run workflow on `main`.
    Non-tag run — secrets flow via the `release` Environment
    only if 14.3.1's deployment-tag rule explicitly allows
    `main`, so flip the rule temporarily (Selected branches:
    `main`) OR push a throwaway `v0.0.0-dryrun` tag, your call.
    Latter is cleaner — delete the tag + draft release after.
  - [ ] 14.6.2 Watch the macOS job. The 5 signing-related steps
    should all be green (Import cert / Build signed / Notarize
    / Upload). Expected total runtime ~10-15 min.
  - [ ] 14.6.3 Download the uploaded signed `.dmg` artifact,
    mount on a clean Mac, confirm `spctl -a` green. If yes,
    revert the 14.6.1 deployment-rule loosening (back to
    `v*.*.*` only).

- [ ] 14.7 **Real-tag validation + close out 10.9.2.**
  - [ ] 14.7.1 Cut the next real release tag. Watch the
    macOS lane; signed + notarized + stapled dmg lands on the
    GitHub Release alongside the tarball.
  - [ ] 14.7.2 Download from the Release page, open on a Mac
    that's never seen this app before (or `xattr -d
    com.apple.quarantine` first to simulate fresh download).
    Confirm Gatekeeper allows direct double-click — no
    "unknown developer" prompt, no right-click → Open dance.
  - [ ] 14.7.3 Mark PLAN 10.9.2's macOS Tier-2 bullet **done**.
    Update `docs/dev/release-signing.md` "Status" callout from
    "secrets pending" to "live as of v<X.Y.Z>".
  - [ ] 14.7.4 CLAUDE.md "macOS support" → "Release artifact"
    line: replace "No `.app` bundle / code signing /
    notarization in v1; users right-click + Open" with the
    signed+notarized reality.

- [ ] 14.8 **Rotation reminders (calendar).**
  - [ ] 14.8.1 App-specific password — Apple revokes if dormant.
    1-year calendar reminder to rotate
    (`APPLE_APP_PASSWORD`).
  - [ ] 14.8.2 Developer ID Application cert — 5-year validity
    from issue date. Calendar reminder 11 months before expiry;
    rotation playbook in `docs/dev/release-signing.md` "Cert
    rotation / revocation".

## Phase 15 - Play-through capture harness

Goal: a scripted harness that records a full demo run — egui
launcher + two phone PWAs + timed narration text — and outputs an
MP4 plus a scrollable HTML gallery for the docs site. Replaces
the existing static `screenshot_tour` (which has bit-rotted; the
stale-version overlay intercepts clicks now, and per-screen stills
don't show the *flow* anyway). Per-surface capture pipelines run
independently, ffmpeg stitches them in post — egui can't be
iframed alongside the PWAs, and per-surface capture decouples the
chromedriver lifecycle from the native window capture so a stutter
in one doesn't desync the others.

**Decisions locked in 2026-05-24:** per-surface capture +
ffmpeg-stitch composite (not single-viewport iframes — egui is
native), MP4 + scrollable HTML gallery output, web-overlay
narration for the PWA streams, ffmpeg drawtext burn-in for any
narration that needs to land over the launcher.

- [ ] 15.1 **Architecture + scaffolding (research-first).**
  - [ ] 15.1.1 Capture-mechanism choice per surface, written up
    in this PLAN entry: macOS ffmpeg `-f avfoundation` for the
    egui launcher (Windows: `-f gdigrab`); chromedriver-driven
    per-frame screenshot loop for each PWA (simpler than
    Page.screencast over CDP and gives deterministic frames);
    ffmpeg drawtext + filter_complex for compositing.
  - [ ] 15.1.2 Time-sync model. Single harness owns the wall
    clock. Each surface emits a start-timestamp at first frame;
    harness records a `(t_offset, action, narration?)` timeline
    JSON. Compositor reads the JSON to align streams + overlay
    narration. Decouples frame drops in one surface from
    bricking the whole capture.
  - [ ] 15.1.3 New crate at `tools/playthrough/` (Rust binary).
    Reuses `skylander-e2e-tests` helpers for server spawn +
    profile injection + game launch. Adds: native screen-region
    capture spawning, ffmpeg child orchestration, timeline JSON
    serialisation, scenario DSL.

- [ ] 15.2 **Per-surface capture — egui launcher.**
  - [ ] 15.2.1 Launch server in a known-geometry window (fixed
    1920x1080 fullscreen on a virtual display, or a known-x-y
    top-left at a fixed size). Determinism is the point — the
    compositor crops to a known rect.
  - [ ] 15.2.2 macOS: `ffmpeg -f avfoundation -framerate 30
    -video_size 1920x1080 -i "<screen_idx>:none" -t <duration>
    egui.mp4`. Screen index resolved by enumerating
    `system_profiler SPDisplaysDataType` once at harness start.
  - [ ] 15.2.3 Windows: `ffmpeg -f gdigrab -framerate 30
    -offset_x X -offset_y Y -video_size 1920x1080 -i desktop
    egui.mp4`. Same crop semantics, different input filter.
  - [ ] 15.2.4 RAII guard around the ffmpeg child so a panic in
    the scenario kills the recorder + flushes the MP4. No
    orphaned ffmpeg processes after a failed run.

- [ ] 15.3 **Per-surface capture — phone PWAs.**
  - [ ] 15.3.1 Per-PWA chromedriver session (2× by default;
    extensible to N). Each at a phone viewport (390x844, iPhone
    13 Pro). Connect to the same server instance the egui
    harness spawned.
  - [ ] 15.3.2 30 Hz screenshot loop per phone — fantoccini's
    `screenshot()` returns PNG bytes, write each to a
    timestamped frame file. Async task per phone so neither
    blocks the other.
  - [ ] 15.3.3 Per-phone ffmpeg pass at scenario end:
    `ffmpeg -framerate 30 -i frame_%05d.png phone-1.mp4`.
    Lossless or near-lossless H.264 so the composite step can
    re-encode without compounding artifacts.
  - [ ] 15.3.4 Frame-drop tolerance: if a screenshot fails
    (chromedriver hiccup), reuse the previous frame's bytes
    rather than aborting. The compositor cares about steady
    frame rate, not perfect frame freshness.

- [ ] 15.4 **Narration scripting.**
  - [ ] 15.4.1 Scenario DSL exposes a `narrate(text, surface)`
    primitive. `surface = PhoneA|PhoneB|Both` injects an
    overlay div into the corresponding chromedriver session via
    `execute()`; `surface = Launcher|Composite` adds the
    narration to the timeline JSON for the ffmpeg drawtext
    burn-in pass.
  - [ ] 15.4.2 PWA-side overlay: a Leptos component
    `<NarrationOverlay>` gated on a `?demo=1` query-string flag
    so it never renders in real user sessions. Position:
    bottom-center, semi-translucent dark plate, Titan One. Same
    aesthetic as the in-game subtitle bar (`docs/aesthetic/mocks/`
    for reference).
  - [ ] 15.4.3 Timeline JSON schema:
    `{ events: [{ t: 0.5, kind: "narrate", text: "...", surface: "..."}], ... }`.
    Compositor reads this on the burn-in pass.

- [ ] 15.5 **ffmpeg compositing pipeline.**
  - [ ] 15.5.1 Layout: 1920x1080 final canvas. Top half (1920x540)
    is the launcher; bottom half is the two PWAs side-by-side
    (960x540 each, viewport scaled). Narration burns into a 60px
    strip across the bottom of the composite when `surface =
    Composite`.
  - [ ] 15.5.2 `ffmpeg -i egui.mp4 -i phone-1.mp4 -i phone-2.mp4
    -filter_complex "[0]scale=1920:540[a];[1]scale=960:540[b];
    [2]scale=960:540[c];[b][c]hstack=2[bc];[a][bc]vstack=2[full];
    [full]drawtext=...[out]" -map "[out]" composite.mp4`.
    drawtext reads from the timeline JSON via a generated
    drawtext filter expression (or split into multiple drawtext
    nodes per narration line — easier).
  - [ ] 15.5.3 Audio track: silence by default (Skylanders
    audio is copyrighted, per CLAUDE.md Kaos non-goal). Leave
    the audio rail in the container so users can drop in their
    own narration voiceover post-hoc.

- [ ] 15.6 **Scenario library.**
  - [ ] 15.6.1 First-time-user flow **from empty profile DB**:
    server boots cold with no profiles → launcher shows the
    "add your first profile" empty state → QR scan → first-
    profile-create wizard → PIN set → game pick → portal
    first-load. The empty-DB bootstrap is the point — shows
    the genuine zero-state launcher + zero-state phone-side
    create flow that a pre-seeded harness skips over.
    ~60-90s.
  - [ ] 15.6.2 Co-op flow: phone B joins, both phones place
    figures, eviction demo. ~60-120s.
  - [ ] 15.6.3 Stat-editing flow (Phase 11 hero): open figure
    detail, STATS button, level + gold edit, save, see stats
    refresh on Phone B simultaneously. ~45s. Showcases 11.14
    cross-phone refresh.
  - [ ] 15.6.4 APPEARANCE cycling demo (Phase 11.16 hero):
    open Spyro detail, tap APPEARANCE, cycle Dark Spyro →
    Legendary Spyro → base. ~30s.
  - [ ] 15.6.5 Kaos surprise demo: mid-game taunt fires, swap
    lands, dismiss flow. ~30s.
  - [ ] 15.6.6 Admin / manage-profiles screen tour: from the
    kebab menu's MANAGE PROFILES action, walk the Konami gate
    → admin grid → per-profile EDIT screen (rename, recolour,
    Kaos opt-in toggle, PIN reset, delete-with-confirm). The
    only flow that surfaces the parent-side controls — non-
    obvious if you haven't gone hunting for them. ~45-60s.

- [ ] 15.7 **Output: MP4 + scrollable HTML gallery.**
  - [ ] 15.7.1 Per-scenario MP4 dropped to `docs/assets/videos/`.
  - [ ] 15.7.2 Scenario-driven HTML gallery generator pulls
    scene boundaries from the timeline JSON, extracts a still
    per scene via `ffmpeg -ss <t> -frames:v 1`, emits a Jekyll
    page at `docs/tour.md` (replacing the current static
    screenshot tour) that lists each scenario with embedded
    `<video>` + per-scene stills + narration text.
  - [ ] 15.7.3 Site integration: `docs/index.md` hero gets the
    first-time-user MP4 embedded. `docs/features.md` each
    feature section links to the matching scenario.

- [ ] 15.8 **Retire `screenshot_tour`.**
  - [ ] 15.8.1 Delete `crates/e2e-tests/tests/screenshot_tour.rs`
    once 15.7 lands. The play-through harness produces both the
    video AND the per-scene stills, so the static tour has no
    job left.
  - [ ] 15.8.2 Delete `docs/assets/screens/*.png` (May-6 stills
    will be obsolete once `docs/assets/videos/` exists).
  - [ ] 15.8.3 Update CLAUDE.md "Aesthetic" section: drop the
    "screenshots TODO" comments in `docs/index.md` /
    `docs/features.md`, point at the play-through harness as
    the canonical demo path.

- [ ] 15.9 **CI integration (optional).**
  - [ ] 15.9.1 Decide: does the play-through harness run in CI
    on every push (catches regressions in the demo flow), or
    only on tag (regen the docs videos per release)? Tag-only
    is cheaper; per-push is more useful as a UX-regression
    canary. Per-push wins if runtime stays under 5min per
    scenario; pin at tag-only otherwise.
  - [ ] 15.9.2 `release.yml` hook to upload generated MP4s as
    release assets so the docs site can reference them by tag
    instead of bundling them into the repo (each MP4 is likely
    10-30 MB; bundling 5+ scenarios at every tag is ~150MB into
    git history per release — bad).

- [ ] 12.1 **Research + scaffolding (research-first, no code
  commits).**
  - [ ] 12.1.1 — AX Rust crate audit. Candidates: `accessibility-sys`
    (raw bindings), `accessibility` (higher-level),
    `objc2-app-kit` + `objc2-accessibility` (modern + actively
    maintained). Pick one based on API completeness for menu
    walking + action invocation, maintenance status, license.
    Output: choice + rationale in this PLAN entry.
  - [ ] 12.1.2 — **RPCS3 AX exposure probe (go/no-go gate).**
    Mac equivalent of `tools/uia-probe/`. Walk RPCS3's process
    tree and dump the AX hierarchy of (a) main window menu bar,
    (b) running game window, (c) Skylanders Manager dialog
    (open it manually first). Critical question: does Qt-on-Mac
    expose `AXMenu` / `AXMenuItem` with usable `AXPress`
    actions, or does it route everything through generic groups?
    If the latter, drop to a fallback path before writing any
    driver code.
  - [ ] 12.1.3 — Permission flow research. `AXIsProcessTrusted()`
    API; the System Settings deep-link URL
    (`x-apple.systempreferences:com.apple.preference.security?Privacy_Accessibility`).
    UX: on first launch, prompt the user, deep-link them to the
    right pane, poll until granted.
  - [ ] 12.1.4 — Restructure decision (recommended (C)):
    - (A) Keep `crates/rpcs3-control/` shared with
      `cfg(target_os = …)` modules alongside the existing Windows
      ones. Simpler; shared trait + types.
    - (B) Split into `rpcs3-control-windows` and
      `rpcs3-control-macos` with shared trait in
      `rpcs3-control-core`. Cleaner build; more file churn.
    - (C) Hybrid: shared trait + mock in `rpcs3-control`,
      platform-specific code via cfg-gated submodules. Lowest
      churn; matches existing pattern. Pick + document.

- [ ] 12.2 **AX probe tool (`tools/ax-probe/`).**
  - [ ] 12.2.1 — New crate at `tools/ax-probe/` with Cargo.toml +
    `src/main.rs`. CLI accepts `--pid <rpcs3_pid>` and walks the
    AX tree printing roles / titles / positions / actions. Saves
    output to `docs/research/mac-ax-probe.md` as a permanent
    reference.
  - [ ] 12.2.2 — Findings + Mac gotchas section added to
    `docs/research/game-launch-window-mgmt.md` (or a Mac-specific
    sibling) in the same format as the Windows section already
    there.

- [ ] 12.3 **`AxPortalDriver` implementation
  (`crates/rpcs3-control/`).**
  - [ ] 12.3.1 — Mac-side trait impl using the AX crate from
    12.1.1. Mirror `UiaPortalDriver`'s public surface so the
    `PortalDriver` trait abstraction holds.
  - [ ] 12.3.2 — Menu navigation: walk Menu Bar → Manage →
    Portals and Gates → Skylanders Portal. Use `AXPress` if
    available; fall back to keystroke synthesis only if 12.1.2
    confirms AX is unusable for menus.
  - [ ] 12.3.3 — Slot operations (LoadFigure, ClearSlot) via
    the Skylanders Manager dialog's per-slot buttons.
  - [ ] 12.3.4 — File dialog (NSOpenPanel) interaction for the
    file picker. Options: AX-driven path entry on the path field,
    or `cmd-shift-G` "Go to Folder" + type path. Pick whichever
    AX exposes cleanly.
  - [ ] 12.3.5 — Off-screen window hide. Mac equivalent of Win32
    `SetWindowPos`: AX `kAXPositionAttribute` (or fall back to
    `osascript`). RAII guard like `hide_dialog_offscreen` on
    Windows.

- [ ] 12.4 **Mac `RpcsProcess`.**
  - [ ] 12.4.1 — Spawn via `open -na <RPCS3.app> --args
    <eboot_path>` OR direct exec of
    `<RPCS3.app>/Contents/MacOS/rpcs3`. Decision documented
    (open -na for proper bundle launch semantics; direct exec
    for tighter process management).
  - [ ] 12.4.2 — EBOOT.BIN CLI passthrough (`launch_with_eboot`
    parity).
  - [ ] 12.4.3 — Crash detection (parent process wait + reap).
  - [ ] 12.4.4 — Force-kill on shutdown.
  - [ ] 12.4.5 — `RPCS3.buf` lockfile cleanup (same hazard as
    Windows — see CLAUDE.md "RPCS3 window/menu gotchas").

- [ ] 12.5 **Wizard + config (Mac-aware).**
  - [ ] 12.5.1 — RPCS3.app picker. Default search paths:
    `/Applications/RPCS3.app`, `~/Applications/RPCS3.app`.
  - [ ] 12.5.2 — Firmware-pack root picker. Same flow as Windows.
  - [ ] 12.5.3 — AX permission detection + prompt-to-grant. If
    `AXIsProcessTrusted()` is false, surface a modal explaining
    what's needed and offer a deep-link button. Poll until
    granted (or user dismisses).

- [ ] 12.6 **Driver wiring + feature flags.**
  - [ ] 12.6.1 — `SKYLANDER_PORTAL_DRIVER=ax` env var routing in
    `main.rs`. `mock` and `uia` still work on their respective
    platforms.
  - [ ] 12.6.2 — `cfg(target_os = "macos")` gating for the AX
    driver impl + its dependencies (Linux/Windows builds don't
    pull in macOS-only deps).
  - [ ] 12.6.3 — Default driver selection per platform: Windows →
    `uia`, macOS → `ax`, others → `mock`. CLI/env override always
    wins.

- [ ] 12.7 **Tests.**
  - [ ] 12.7.1 — `crates/e2e-tests/tests/live_mac_integration.rs`
    — Mac equivalent of `live_integration.rs`. Spawn RPCS3 with a
    real game, drive a load/clear cycle, assert via the WS event
    stream. `#[ignore]` (requires real RPCS3 + game on the
    running machine).
  - [ ] 12.7.2 — AX-permission-missing path: unit test the
    wizard's detection + error message (mock `AXIsProcessTrusted`).
  - [ ] 12.7.3 — Manual verification checklist in
    `docs/dev/macos-bringup.md`: install RPCS3 + Skylanders Trap
    Team, grant AX permission, run server with
    `SKYLANDER_PORTAL_DRIVER=ax`, edit a figure, verify it loads
    in the actual game.

- [ ] 12.8 **Docs.**
  - [ ] 12.8.1 — SPEC.md Round 8 Q&A: why we reversed the
    Mac-driver non-goal (v1.5.0 playtest friction; Mac → Windows
    round-trips were dominating debug time).
  - [ ] 12.8.2 — CLAUDE.md update: remove the "Mac driver is an
    explicit non-goal" claim from the macOS section; add an AX
    gotchas section paralleling the existing "RPCS3 window/menu
    gotchas" for Windows.
  - [ ] 12.8.3 — `docs/dev/macos-bringup.md` gets a "Real driver
    path" section explaining the AX permission grant + the same
    env var setup as Windows.


  now ship with real drivers (Windows UIA, macOS AX); Linux can
  build and run with the mock driver but no plans to add Linux
  GUI automation. .app bundle + code signing on Mac are deferred
  (10.6.3) unless Gatekeeper friction proves blocking.




## Phase 16 - RPCS3 integration via patched upstream + IPC (research-first)

Replace the fragile GUI-automation control path (Windows UIA / planned macOS
AX) with a thin **patch series on a pinned upstream RPCS3** + arm's-length
**IPC**, run in **no-GUI** mode. Keeps RPCS3 a separate process (crash
isolation) and tracks upstream cheaply by keeping every patch shallow + additive.
Decision record + rejected alternatives (strip-down fork, virtual USB, in-process
link): `docs/research/rpcs3-integration-strategy.md`. Gated on the 16.1 spike.

- [x] 16.1 **Spike / go-no-go gate (no production code).** — GO (HTPC 2026-05-28).
  - [x] 16.1.1 — Clone RPCS3 at a pinned tag; locate the emulated Skylander USB
    device + the GS-frame window-creation point. Record file paths + function
    seams in the decision doc.
  - [x] 16.1.2 — P1 prototype: feed figure bytes into the emulated Skylander
    device over an AF_UNIX socket; confirm a no-GUI direct-boot game sees a
    load/clear with zero dialog interaction. **DONE** — Giants saw Stealth Elf
    load/clear over the socket; patch ≈ one file. Added a clean STATE query +
    1 Hz heartbeat (Emu status + g_progr compile progress + RSX flip count).
  - [x] 16.1.3 — P2 prototype: create the game window borderless at a supplied
    geometry without stealing focus, and report its native handle over IPC.
    **DONE** — gs_frame env-gated borderless + no-focus-steal; IPC `WINDOW`
    returned the HWND; controller `SetWindowPos`'d it (verified on-screen).
  - [x] 16.1.4 — Go/no-go decision recorded: patch depth confirmed shallow →
    proceed. **GO** — P1 ≈ one file, P2 ≈ two small hunks, both additive on
    rarely-churning seams. See `docs/research/rpcs3-integration-strategy.md`.

- [x] 16.2 **Vendored RPCS3 + CI.** — done 2026-05-29.
  - [x] 16.2.1 — Add RPCS3 as a git submodule pinned to the spike's known-good tag.
    **DONE** — `vendor/rpcs3` → `RPCS3/rpcs3` pinned (commit, not branch) at
    `c11979d` (pristine upstream; patches stay external). Added with
    `--reference D:\workspace\rpcs3 --dissociate` so it borrowed the local clone's
    objects (no multi-hundred-MB re-download) yet is self-contained.
  - [x] 16.2.2 — Patch-series layout (P1/P2 as ordered patches or a tracked
    branch) + a documented rebase-onto-new-tag procedure.
    **DONE** — `rpcs3-patches/0001-P1…`, `0002-P2…` (git format-patch, apply in
    order; P2 depends on P1's global), `apply.sh` (`git am --3way`), and a
    `README.md` with the pin table + step-by-step rebase-onto-newer-commit
    procedure. Editable home = dev clone branch `spike-patches`. `.gitattributes`
    pins `*.patch` to `-text` so EOL conversion can't corrupt them.
  - [x] 16.2.3 — CI lane: apply the patch series to the pinned tag and build the
    patched RPCS3 for Windows + macOS.
    **DONE (tiered)** — `.github/workflows/rpcs3-patched.yml`: (1) `patch-apply-check`
    (Ubuntu, always-on, path-filtered) `git am`'s the series onto the pinned
    submodule + asserts the touched-file set — the real anti-patch-rot guard
    (verified locally). (2) `build-windows` / `build-macos` mirror upstream RPCS3's
    own `.ci/` recipe but are **gated** behind `workflow_dispatch` + a `full_build`
    input (a from-scratch RPCS3 build is ~1h — opt-in, not every push). The
    canonical build stays the HTPC recipe; the gated jobs are a best-effort CI
    mirror, not yet run-green.
  - [x] 16.2.4 — **Relicense MIT → GPL-2.0-only.** `rpcs3-patches/` is a derivative
    of RPCS3's GPLv2 source, so the published combined work must be GPL-2.0 (RPCS3
    is GPL-2.0-*only* → can't be v3). `/LICENSE` = verbatim GPLv2; updated
    `Cargo.toml` (`license`), `README.md`, `docs/about.md` (was stale "public
    domain"), `docs/dev/winget-submission.md`. Follow-up: the WiX installer
    (`license = false`/`eula = false`, PLAN 13.x) may want to surface the GPL now.

- [x] 16.3 **P1 — IPC portal control patch (upstream side).** — landed in the
  16.1 spike; formalised here. The patch is in `rpcs3-patches/0001-P1…`.
  - [x] 16.3.1 — IPC endpoint on the emulated Skylander device exposing
    load / clear(slot) / query_state(). **DONE** — `LOAD <path>` (emulator
    assigns the slot, see the slot-model note), `CLEAR <slot>`, `STATUS`, `STATE`,
    `WINDOW`, `PING` + 1 Hz `HB`. **Slot model (decision 2026-05-29):** the
    *emulator* owns slot numbering — `LOAD` is **not** slot-targeted; it places in
    the first free slot and reports which. The user cares about portal *contents*,
    not positions, and the phone never surfaced a slot choice, so no slot-targeted
    `load_skylander` variant is needed (which kept the P1 patch a one-file add).
  - [x] 16.3.2 — Protocol definition (framing, slot model, error responses)
    shared with the controller. **DONE** — codified as the controller's
    `crates/rpcs3-control/src/ipc/proto.rs` (pure codec + unit tests) with the
    wire grammar in its module doc; cross-referenced from `rpcs3-patches/README.md`.

- [ ] 16.4 **P2 — window-lifecycle patch (upstream side).**
  - [ ] 16.4.1 — Game window created borderless/undecorated at a
    controller-supplied geometry, no focus-steal.
  - [ ] 16.4.2 — Window emits its native handle over the IPC channel on creation.

- [ ] 16.5 **`IpcPortalDriver` (controller side).**
  - [x] 16.5.1 — New PortalDriver impl talking the P1 protocol over the socket;
    drops in beside UiaPortalDriver / MockPortalDriver. **DONE** —
    `crates/rpcs3-control/src/ipc/` (driver `mod.rs` + codec `proto.rs`). Blocking
    AF_UNIX (`std::os::unix::net` on unix, `uds_windows` 1.x on Windows — sync, to
    match the sync trait + the server's `spawn_blocking` worker; chose the mature
    blocking crate over async `uds-stream-windows` 0.0.1). One connection per op
    (stateless), heartbeat lines skipped while awaiting a reply. `open_dialog` is a
    no-op (no dialog). Extra non-trait methods `read_state` / `window_handle` /
    `ping` for the 16.6/16.7 consumers. Tested by `tests/ipc_loopback.rs` — an
    in-process fake P1 server (runs in CI on Win+mac, no real RPCS3) + proto unit
    tests. clippy/fmt clean.
  - [ ] 16.5.2 — Driver selection: IpcPortalDriver becomes the default production
    driver on Windows + macOS; mock unchanged; UIA retained as fallback behind
    the env var.
    - [x] 16.5.2.1 — **Selection plumbing done.** `DriverKind::Ipc` (+
      `PersistedDriverKind::Ipc`), selectable via `SKYLANDER_PORTAL_DRIVER=ipc`;
      `build_driver` constructs `IpcPortalDriver` (cross-platform, no cfg gate); IPC
      resolves games from `games.yml` exactly like UIA. **Default stays UIA** until
      proven. clippy/fmt clean on both the `()` and `test-hooks` feature branches.
    - [x] 16.5.2.2 — **Done via the trait, not threading.** Added default-`None`
      `ipc_socket_path` / `emu_state` / `game_window_handle` to `PortalDriver`
      (overridden by `IpcPortalDriver`), so consumers reach the IPC state/handle
      through the existing `Arc<dyn PortalDriver>` — no concrete-handle plumbing
      through `AppState`/worker. Used by 16.6.1's boot path; ready for 16.6.2/16.6.3.
    - [ ] 16.5.2.3 — Flip the default to IPC (Win+mac) once 16.6.1-2 are proven on
      the HTPC; keep `uia` as the env fallback.
  - [ ] 16.5.3 — **Collapse the positional-slot model server-side** (the other
    half of the 16.3.1 slot-model decision). Today the worker sets
    `portal[phone_chosen_slot]`; under emulator-owned numbering the mirror must
    reflect the *emulator-assigned* slot (thread the assigned slot back from
    `load`, or rebuild the mirror from `STATUS`), preserving `placed_by` ownership.
    Phone's `/api/portal/slot/:n/*` routes become "add/remove a figure" (the `:n`
    is already a holdover the UI doesn't surface). Bundle with 16.6.

- [ ] 16.6 **No-GUI launch + window coordination.** *(scoped 2026-05-29)*

  **Design decisions (scoping):** (1) **Launcher stays overlaid** — keep the
  current transparent fullscreen always-on-top eframe window compositing over the
  game; the corner reconnect-QR is load-bearing. (2) **Coordinated siblings, not
  `SetParent`** — position the borderless game fullscreen *under* the topmost
  launcher; this is the only model that keeps the overlay on top (a `SetParent`
  child renders *above* the parent's egui surface, occluding the overlay).
  (3) **Resolution flicker is transition-only** (boot/launch + entering/leaving a
  game — *not* mid-game or steady-state per the user) → fix with a **STATE-gated
  opaque transition cover**, NOT `SetParent` and NOT a Vulkan/wgpu port (the
  launcher is glow/OpenGL with custom `BadgeRig`+`ShaderRig` GL pipelines; porting
  is a big rewrite for a benefit the cover already delivers). (4) **Keep UIA** as
  an env-gated fallback (`SKYLANDER_PORTAL_DRIVER=uia`) through HTPC proving;
  delete in 16.8. (5) **Readiness via IPC `STATE`/heartbeat**, retiring the
  window-title FPS scraper + log-tail. (6) macOS = cross-platform position API +
  **stub** (no patched mac binary yet — 16.2.3 mac build is gated). Critical files:
  `crates/server/src/state.rs` (BootDirect + the scrapers to retire),
  `crates/rpcs3-control/src/process.rs` (launch + window scrapers),
  `crates/server/src/main.rs` (`build_driver`), `crates/server/src/ui/mod.rs`
  (z-order + transition cover), `crates/server/src/config.rs` (`DriverKind`).

  Depends on **16.5.2** (driver selection plumbing) + lands alongside **16.5.3**
  (slot-model collapse). Sequence: 16.5.2 plumbing (keep UIA default) → 16.6.1
  launch+readiness → 16.6.3.1 STATE poller (with 16.6.1, so `game_playable` never
  goes dark) → 16.6.2 window position + flicker cover → flip default to IPC once
  proven on the HTPC → 16.5.3 → delete scrapers (16.6.3.2-3). UIA stays compiled
  throughout. Acceptance = `crates/rpcs3-control/tests/live_ipc.rs` on the HTPC.

  - [ ] 16.6.1 — **No-GUI launch.** *(core implemented; live end-to-end pending)*
    - [x] 16.6.1.1 — **Done.** `UiaRpcsProcess::launch_no_gui(exe, eboot, ipc_path)`
      (+ `RpcsProcess::launch_no_gui`): spawns `--no-gui <EBOOT>` with env
      `SKYLANDER_BORDERLESS=1` + `SKYLANDER_IPC_PATH`, keeping the Job Object; reuses
      the existing spawn/Job body. (Mirrors the validated `run_game.bat` launch.)
    - [x] 16.6.1.2 — **Done.** `BootDirect` branches on `driver.ipc_socket_path()`:
      the IPC arm spawns via `launch_no_gui` and waits on `driver.emu_state()`
      `is_playable()` (status=running + frames advancing), replacing the
      `read_viewport_title()` `[SERIAL]` loop; no `open_dialog`. The legacy UIA arm
      is unchanged. **Driver capability via the trait** (not a threaded concrete
      handle): added default-`None` `ipc_socket_path` / `emu_state` /
      `game_window_handle` to `PortalDriver`, overridden by `IpcPortalDriver` — this
      also satisfies **16.5.2.2**. clippy/fmt clean. *Live end-to-end run (server +
      `SKYLANDER_PORTAL_DRIVER=ipc` + a real launch) still to do on the HTPC.*
    - [ ] 16.6.1.3 — Shutdown: `WM_CLOSE` to the IPC `WINDOW` handle, falling
      through to the Job-Object force path (works without the `"RPCS3 "` title).
      Today shutdown force-kills via the Job Object (fine); refinement pending.
      A clean IPC `QUIT`/`STOP` command is a 16.7 follow-up.
  - [ ] 16.6.2 — **Window coordination + transition-flicker elimination.**
    - [ ] 16.6.2.1 — Cross-platform `position_game_window(handle, monitor_rect)`
      (Windows: extend `hide::set_position_raw` with size + `SWP_NOACTIVATE`;
      macOS stub). After playable, read `ipc.window_handle()` and place the
      borderless game fullscreen at the launcher's monitor, under the topmost
      launcher (siblings).
    - [ ] 16.6.2.2 — Kill the per-frame z-order churn: delete
      `push_rpcs3_main_to_bottom_via_win32` (`ui/mod.rs:312`; no menu window under
      borderless) and set launcher-topmost + game-position **once on state change**,
      not every frame.
    - [ ] 16.6.2.3 — **STATE-gated opaque transition cover (the flicker fix).**
      Launcher paints an *opaque* starfield/loading cover during boot and on
      enter/leave a game; fades to transparent to reveal the game only once
      `STATE.is_playable()` is stable for N consecutive samples; on leave, cover
      *before* tearing the game window down. Hides the boot + enter/leave resize
      flicker entirely (the user's reported cases).
    - [ ] 16.6.2.4 — *(deferred spike, optional)* `SetParent` nesting for
      single-window alt-tab/minimize — only if desired later; needs the overlay
      re-layered above the game child. NOT on the 16.6 critical path (the cover
      solves the flicker that motivated it).
  - [ ] 16.6.3 — **Retire obsoleted scrapers (UIA kept as fallback).**
    - [ ] 16.6.3.1 — `spawn_state_poller` (IPC `STATE` → `game_playable`, optional
      `progr=a/b` → compile subtitle). Lands with 16.6.1.
    - [ ] 16.6.3.2 — Delete `spawn_fps_sampler` + `spawn_shader_compile_watchdog` +
      `parse_fps_from_title` once the STATE poller is proven.
    - [ ] 16.6.3.3 — Delete the window-title scrapers `read_viewport_title` /
      `find_compile_progress_text` / `read_main_window_title` /
      `list_all_visible_window_titles` (exports in `lib.rs`, bodies in `process.rs`)
      after confirming no remaining callers.
    - [ ] 16.6.3.4 — Leave `uia.rs` + dialog-nav + `hide.rs` dialog fns intact
      behind the `uia` env fallback; tag for deletion in 16.8.

- [ ] 16.7 **Crash/freeze supervisor (independent of portal path).**
  - [ ] 16.7.1 — Detect crash (process exit) and freeze (liveness signal —
    frame/log progress stall).
  - [ ] 16.7.2 — Auto-restart: relaunch RPCS3, re-boot the game, restore portal
    slot state.
  - [ ] 16.7.3 — Phone-side "reconnecting…" overlay during recovery.
  - [ ] 16.7.4 — Per-game pinned-build + config presets (SPU block size,
    accuracy, renderer, framelimit).

- [ ] 16.8 **Supersede obsoleted work + docs.**
  - [ ] 16.8.1 — Mark Phase 12 (Mac AX driver) superseded — IPC control is
    cross-platform; defer/delete the AX-driver tasks.
  - [ ] 16.8.2 — Fold Phase 6.1 (window-flicker suppression) into 16.6 — no
    dialog means no flicker hack.
  - [ ] 16.8.3 — CLAUDE.md: new "RPCS3 integration (IPC)" section; demote UIA
    driver to fallback; drop the Mac-AX-as-production framing.
  - [ ] 16.8.4 — SPEC.md Q&A entry: why we moved from GUI automation to a
    patched-upstream IPC path.

- [ ] 16.9 **Emulator configuration with the GUI hijacked (no-GUI runtime).**
  RPCS3 config is file-based YAML and `--no-gui` is per-launch (not a build-time
  removal) — the full settings GUI is one plain launch away. So:
  - [ ] 16.9.1 — Controller writes/ships RPCS3 config files directly: global
    `config.yml`, per-game `config/custom_configs/config_<SERIAL>.yml` (this IS
    the 16.7.4 per-game tuning), `config/vfs.yml`, input configs. Curated,
    version-controlled, applied before boot — no GUI.
  - [ ] 16.9.2 — First-launch machine setup: firmware install via
    `rpcs3 --installfw <PUP>` (headless) or one GUI pass; default Vulkan +
    generated sane `config.yml`. Folds into the existing first-launch flow.
  - [ ] 16.9.3 — On-demand escape hatch: launcher "RPCS3 Settings" action runs
    the SAME binary WITHOUT `--no-gui` for rare/advanced config; it writes the
    same YAML the `--no-gui` boots consume.
