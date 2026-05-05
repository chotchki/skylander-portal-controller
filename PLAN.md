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

## Phase 10 Items

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

### 10.2 `ios-inspect` multi-device orchestration
Today `ios-inspect boot` auto-picks the newest Dynamic-Island iPhone
and `state.json` tracks one device + one webinspectord_sim socket. The
2-phone product feature (PLAN 8.1, 8.2a) needs an iPad and an iPhone
booted simultaneously, both pointed at the same server, each driveable
independently. `xcrun simctl` and `ios-webkit-debug-proxy` both support
multiple simulators concurrently — the limitation is purely in
`ios-inspect`'s state model.

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

### 10.3 E2E harness portable to macOS
The fantoccini suite in `crates/e2e-tests/` already exercises the mock
driver — which is the only driver available on Mac — but is hardcoded
for Windows in two places: the firmware-pack default path and the
chromedriver discovery fallback. Fix those, then drive the existing
suite green on macOS.

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
- [~] 10.3.3 — Harness end-to-end works on Mac under the matched
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
- [~] 10.3.6 — **Reconcile chromedriver suite with current SPA.**
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
- [~] 10.3.6b — **working_copies.rs (2 tests).** 1/2 green:
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

### 10.4 Simultaneous iPad + iPhone simulator e2e
This is the core user value: drive both an iPad and an iPhone
simulator against a single server instance, exactly the way the
2-phone feature ships. Combines 10.2's multi-device `ios-inspect`
with 10.3's portable harness.

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

### 10.5 Continuous-integration lane
CI was deferred until the app worked. With 10.1–10.4 in place, the
e2e suite has a credible automated home and the "fully automated"
half of the user goal becomes real. Decide CI host first since it
shapes everything downstream.

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

### 10.6 macOS production release artifact
Today the release pipeline produces a Windows zip (`release.yml` runs
`generate_release_notes` and uploads `skylander-portal-controller.exe`).
Mac becomes a parallel artifact: same binary, mock-only driver, same
GitHub Releases attachment shape. No `.app` bundle in the first cut —
a CLI binary in a tar.gz mirrors the Windows zip's friction level and
keeps the release pipeline simple. `.app` + signing + notarization are
deferred unless a real user reports the bare-binary UX as blocking.

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

### 10.8 v1.2 HTPC field-test bugs
First non-dev install pass on the living-room HTPC (2026-05-03,
v1.2.0 zip). Binary launches and the phone reaches the QR, but a
cluster of distribution / Windows-integration rough edges showed up
that don't surface on the dev box. Triage individually — most are
independent.

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
- [~] 10.8.6 **Phone app: figure + game images all render as
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
  PNG (not the element icon) and `/api/games/BLUS30906/image`
  returns 200.

### 10.8.7 Game-launch state machine + cover-before-kill (sequel to 10.8.4)

10.8.4 closed the keystroke-fragility bug at the driver layer (UIA
pattern menu nav) and rewired the server flow to direct-boot. Field
testing v1.3.0 → v1.3.4 surfaced a sequence of UI-layer issues that
aren't separate bugs but symptoms of an implicit, ad-hoc state
machine for game-launch lifecycle:

- **v1.3.3** "iris jumped open before shaders compile" — the
  game_playable signal was a fragile log-quiet heuristic that
  defaulted to "playable" when the watchdog hadn't seen any compile
  lines yet (race against RPCS3's freshly-spawned log writes).
- **v1.3.4** "screen black during launch" — premature transparent
  in-game render over a still-compiling (visually black) RPCS3
  viewport. Tweaked watchdog moved the bug, didn't kill it.
- **v1.3.4** "launcher hung on /api/shutdown" — the in-game branch
  was fully reactive (no `request_repaint`) and never woke for the
  `screen = Farewell` flip. Fixed by 4 Hz heartbeat in 10.8.4 final
  commit, but the underlying lesson is that state transitions need
  to be observable, not "set a flag and hope a frame fires".
- The proposed graceful-quit flow has a race: kill RPCS3 →
  in-game transparent panel briefly shows desktop through, before
  the launcher transitions to AwaitingConnect.

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

Pre-req tooling already shipped:
- `tools/build-msi.sh` — local Windows MSI build for HTPC
  iteration (commit `9c7e1d1`). Sub-minute build cycle.
- `release.yml` auto-publishes as prerelease so MSI is
  downloadable from the Releases page without unhiding drafts
  (commit `71e6d01`).
- Direct-boot driver layer (commits in the 10.8.4 chain
  `1ebbbc9` → `ca7af77`).

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
- [~] 10.9.2 **Code signing + notarization.**
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
- [~] 10.9.4 **macOS `.dmg`** wrapping a `.app` bundle (subsumes
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

## Non-goals

- No bundling of RPCS3 or `.sky` files (piracy concern).
- No Linux support — production targets are Windows + macOS. macOS
  ships the mock driver only (no AXUIElement-based driver to talk to
  Mac RPCS3); .app bundle + code signing are deferred (10.6.3) unless
  Gatekeeper friction proves blocking.
- No user-entered figure names.
- No audio (text-only Kaos to dodge copyright).
- No live wiki scraping at runtime — data is committed to the repo.

## Risks (live list — update as we learn)

- **R1:** UI Automation may not expose enough of the RPCS3 Qt dialog to drive it reliably. Resolved: Alt-keyboard-nav workaround (CLAUDE.md "RPCS3 window/menu gotchas").
- **R2:** "Move portal dialog off-screen" may be blocked by Windows. Resolved: Win32 `SetWindowPos` works; `hide_dialog_offscreen` + RAII guard in `crates/rpcs3-control/src/hide.rs`.
- **R3:** Wiki search hit rate might be below 80%. Resolved: 504/504 coverage (3.19.5).
- **R4:** Leptos touch/mobile UX may prove rough. Mitigation: ongoing Phase 4.18 on-device iteration; PWA install fallback.
