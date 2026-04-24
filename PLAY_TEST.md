# Play-test bug list (2026-04-24)

Bugs surfaced during HTPC + phone end-to-end walkthrough. Each item is
triaged into a severity bucket. Within a bucket, order is "closest to
the main flow first". `[x]` = shipped, `[~]` = in progress, `[-]` =
won't-do / defer, `[ ]` = pending.

## Latest run, need triage

- [x] **Startup feels dead during RPCS3 boot (~10s).** Calm starfield
  + static "STARTING" title was the only content during the server-
  boot window. Added a gentle breathing on the brand-intro alpha
  while `LaunchPhase::Startup` is active (2s period, 55→100% of
  phase alpha, cosine eased). Confirmed 2026-04-24 ("much better").
- [x] Stale PWA from yesterday: now loudly surfaced by the BUILD_TOKEN
  handshake (`/api/version`). Overlay verified firing today against a
  deliberately mismatched server token. Same path also catches a bad
  HMAC key → pairing overlay. Closes the "Pin entry fails, 1234" and
  "Had a pin entry error, deleting pwa and trying again" notes.
- Profile PIN entry back/scroll + Konami layout scroll: pending live
  spot-check against today's freshly installed bundle.

## Blockers — break the core flow

- [~] **21. Switch game 401 toaster "quit failed".** HMAC signature
  included the path's query string on the phone (`/api/quit?switch=true`)
  but the server only hashed the path (`/api/quit`). Fix queued — server
  now uses `uri().path_and_query()`. Pending rebuild+relaunch.
- [x] **9. After picking Giants, phone hangs on game-select while
  LOADING is showing above.** Phone should transition to the portal
  view the moment the driver accepts the launch, not wait for compile.
  Confirmed working 2026-04-24.
- [x] **15. In-game overlay isn't clear — looks like everything is
  dark.** Root cause: eframe's default `App::clear_color` returns the
  visuals' `window_fill` (dark grey), which painted a dim layer over
  RPCS3 during the in-game transparent surface. `Frame::none().fill(
  TRANSPARENT)` on the CentralPanel only skips the panel paint, not
  the pre-panel GL clear. Fix: override `clear_color` to `[0,0,0,0]`.
  Confirmed 2026-04-24.
- [x] **19. "Ok button not found" toaster on first place.** File-
  dialog UIA subtree didn't settle before the first Open-button
  lookup; added a 500ms retry loop + accept button by AutomationId
  "1" (IDOK, locale-independent) alongside the localized "Open"
  name. Confirmed 2026-04-24.
- [x] **20. Emulator library screen not pushed back under the game.**
  Root cause: UIA `Invoke` on the Skylanders Manager dialog's Load/
  Clear buttons (parented to RPCS3's main window) transiently promotes
  main over the game viewport on each figure swap. Fix: per-frame
  `SetWindowPos(HWND_BOTTOM, SWP_NOACTIVATE)` on the main window while
  `rpcs3_running && current_game.is_some() && !switching`. Confirmed
  2026-04-24.
- [x] **23. Exit failed to kill the app / overlay.** Confirmed working
  2026-04-24.
- [x] **24. Desktop flash during game switch.** `switching=true` set
  before stop_emulation completes, but the in-game transparent branch
  still fired while `current_game` was still `Some`, so the player saw
  a frame of desktop before the launcher turned opaque. Fix: gate the
  in-game branch on `!switching`; also clear `in_game_at` so the iris
  isn't pinned closed during the switch. Paired with the new SWITCHING
  GAMES QR back-face so the bridge visual is a halo-spinning coin flip
  rather than a bare heading. Confirmed 2026-04-24.
- [x] **25. Wrong game booted (BLUS31545 requested → BLUS31600 ran).**
  Root cause: UIA returned the cell's bounding rect in physical pixels
  at 4K DPI (cx=1600, cy=1293), and the naive `cx - window_rect.left`
  translation didn't respect the window's DPI context — the computed
  client-area coords landed on a neighbouring row. Fix: use Win32
  `ScreenToClient` so DPI/scaling is handled by the OS. Also added a
  post-boot viewport-title verifier (catches wrong-game boots) and a
  3-attempt retry loop with `stop_emulation` recovery between attempts
  so a rare mis-click is self-correcting. Confirmed 2026-04-24.

## Layout / polish

- [~] **1. Konami back button over the title.** Should be by the kebab
  (same spot as PIN entry's back). Should also use the PIN entry's
  button styling, not the current variant.
  - Fix: `.btn-back` now `position: fixed` at
    `calc(env(safe-area-inset-left) + 64px)` (right of the kebab),
    text-only gold+arrow treatment (PIN-entry style). Same class
    serves Konami, Admin, PIN entry, and create-profile. Pending live
    verify.
- [~] **2. Konami controls cut off at bottom.** Dpad's down arrow and
  everything below it clipped.
  - Fix: dropped `margin-top: auto` on `.gate-pad` (which pushed the
    pad against a bottom that small iPhones couldn't reach) and added
    `overflow-y: auto` on `.konami-gate` as a safety net. Pending live
    verify.
- [~] **3. Create-PIN vs Confirm-PIN cards slightly different width.**
  Root cause: the step title is setting the wizard card's width, and
  titles differ between steps. Title width shouldn't flex the card.
  - Fix: `.create-profile-panel { width: 100% }` + matching width lock
    on `.create-step-title`. Pending live verify.
- [~] **5. Create-profile back button styled different than PIN
  entry's.** Standardize on the PIN entry treatment.
  - Fix: floating `.btn-back` added at the top of CreateProfileForm
    (same class as Konami/PIN/Admin). Bottom-row BACK/CANCEL removed;
    only NEXT/CREATE stays in the actions row. Pending live verify.
- [ ] **7. PIN entry screen pushed down + scrolls.** Should fit without
  scroll on a normal iPhone viewport.
- [~] **8. PIN entry back button isn't by the overlay dots (kebab).**
  Same placement issue as #1.
  - Fix: rolled into #1 — PIN entry now uses the shared `.btn-back`
    fixed to the right of the kebab. Pending live verify.
- [ ] **10. Iris opens while shaders are still compiling.** Should
  hold iris-closed until `game_playable` stays true for the full debounce
  window (not just the first true reading).
  - agreed
- [ ] **14. After phone wake-up, reconnect lands on profile login.**
  Session should survive a phone wake (WS reconnect), or at least skip
  the profile picker if the profile was last-used this session.
  - 
- [ ] **16. Toy-box thumbnails not showing.** No image renders on any
  card. Likely `/api/figures/:id/image` isn't resolving for the new
  `toy_type-variant` id format (6.6.3 touched this area).
- [ ] **17. Astroblast stats say "coming soon".** Expected per 6.3
  placeholder — not a regression, just unshipped UI.
- [ ] **18. Astroblast figure-detail lift-up animation not playing.**
- [~] **22. Portal shows "pick a skylander for slot 6" + plus buttons
  on empty slots.** Chris's preference: empty portal should show a
  floating hint to open the toy box, not per-slot "+" placeholders.
  - Fix: dropped per-slot `+` glyphs (empty bezel only), retired the
    "Pick a Skylander for slot N" banner, added a single bobbing
    `.portal-empty-hint` (↓ open the toy box to add a figure) below
    the grid when any slot is empty. Empty-slot taps are now no-ops.
    Errored slot taps clear the slot (escape hatch). Pending live
    verify.

## Toaster styling

- [~] **6. Profile-create success toast styled as error.** Wrong
  variant selected on success path.
  - Fix: `push_toast` defaults to `ToastLevel::Error`; "Profile
    created." now uses `push_toast_level(_, Success)`. Same treatment
    for "PIN updated." and "Profile edit saved". Validation prompts
    ("Name required.", "PIN must be 4 digits.", "Portal is full")
    flipped to `Warn`. Pending live verify.
- [~] **11. Giants-loading success toast styled as error.** Same class
  of bug as #6. Probably a shared success-path helper using the wrong
  toast level.
  - Fix: `game_picker.rs` "Launched {n}" now uses
    `push_toast_level(_, Success)`. Pending live verify.

## Reconnect / backgrounding

- [ ] **12. Reconnect overlay shows the moment the phone locks** —
  disruptive mid-play. The 10s auto-retry grace needs to be a lot more
  generous before surfacing "you've been disconnected" UX, or the sleep
  transition should route through a quieter state.
  - We should consider/try service worker/push notification if possible
- [ ] **13. Phone sleep produces the same reconnect overlay, slower.**
  Sub-case of #12; same fix.
---

**Already shipped / informational** (carryover from earlier rounds, not
needing revisit right now):

- 4.15.9 / SWITCHING GAMES transition
- 4.19.12 Reconnect QR fade-in
- 4.19.14 Farewell breathe pulse
- 4.19.15 Farewell fade-to-black
- Manifest `start_url: /?k=<hex>` injection (so PWA pin carries the
  HMAC key)
- Kaos sigil (real `kaos.svg` mask), Kaos taunt randomization, Kaos
  z-index bump
- PairingRequired overlay
