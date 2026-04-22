# Skylander Portal Controller — Execution Plan

Phases 0–3 are complete — see [PLAN_ARCHIVE.md](PLAN_ARCHIVE.md) for the full history. Carryover items from those phases that are still open live under "Phase 3 carryover" below. Active work is Phase 4 (phone + launcher UI reskin) and forward.

Conventions:
- `[ ]` pending, `[x]` done, `[~]` in progress, `[?]` blocked / needs discussion.
- New tasks should always be numbered and have a checkbox so they're traceable.
- Don't skip a review checkpoint; the point is to re-plan with new information.

---

## Phase 3 carryover — open items

Everything else from Phase 3 is archived.

- [~] **3.7.8 Game-list hardening.** Failure mode: phone picks a serial that `games.yml` advertises + EBOOT.BIN exists for, but RPCS3's library doesn't have the entry (user removed it in RPCS3's UI without cleaning the yml). `rpcs3.exe --help` confirms there's no CLI dump flag.
  - [x] **Phase 1: verify-at-launch.** `PortalDriver::enumerate_games` walks `game_list_table`; new `DriverJob::EnumerateGames` runs it; `/api/launch` calls it between `wait_ready` and `BootGame` and on miss quits RPCS3 + returns 404 with "{display_name} ({serial}) isn't in RPCS3's library. Re-scan games in RPCS3 and try again." Empty enumeration (no library) and UIA errors fall through to boot's own error path so transient issues don't block every launch. Mock + handler tested via existing workspace suite; live integration scenario deferred (would exercise real UIA enumerate against the HTPC's library — same pattern as 3.7.7).
  - [ ] **Phase 2: truth-from-UIA at picker time** — **unblocked 2026-04-21** now that 4.15.16 landed. Server startup spawns RPCS3 + library view is always live, so `/api/games` can call `driver.enumerate_games()` at request time (or cache on startup + refresh periodically) and drop the games.yml dependency. Can land independently of any 4.18 work.
- [ ] **3.10.7 Ownership badge — final aesthetic pass.** Pip data + colour shipped as 4.18.17; this is the post-Phase-4 styling pass using the final design-system tokens.
- [x] **3.10.8 Show-join-code.** Done 2026-04-22. Server pre-renders the round join QR at startup (`crates/server/src/round_qr.rs` — shared renderer also used by the TV launcher) and serves it from `GET /api/join-qr.png` as pre-rendered PNG bytes cached in `AppState::join_qr_png: Arc<Vec<u8>>`. Phone `menu_overlay.rs` renders `<img src="/api/join-qr.png" class="menu-qr-img">` inside `.menu-qr-frame`; CSS gives the frame a gold gradient ring + circular clip so the round QR reads as a scaled cousin of the launcher coin. Shared renderer means the launcher, phone menu, and in-game reconnect coin all produce identical pixel layouts.
- [ ] **3.10.9 Two-player disconnect-cleanup semantics.** What happens to P2's figures when P1 drops; how kick-back restores layout. Revisit with 3.17 reconnect work once real failure modes surface.
- [ ] **3.10e.6 Ownership-badge e2e test.** Lands with 3.10.7.
- [ ] **3.11.4 Creation Crystals extra-confirm** (type "RESET"). Defer until 4.2.14's hold-to-activate pattern is generalized.
- [ ] **3.12.4 Three-option resume modal** (clear + resume / alongside / fresh) for the 2-phone case. Follow-up once Phase 4 ships.
- [ ] **3.14 Reposes — variant cycling.**
  - [ ] 3.14.1 Browser collapses figures sharing a `variant_group` into a single card with "N variants" badge.
  - [ ] 3.14.2 Tap variant badge → cycle between variants in place (SPEC Q76).
  - [ ] 3.14.3 Loaded variant reflected on the slot's display_name.
- [ ] **3.16.5 First-launch wizard live-desktop verify.** Blocked on SSH session isolation; needs physical-console run.
- [ ] **3.16.6 "Re-run wizard" affordance** once a general app-settings area exists. Escape hatch today: delete `config.json` and relaunch.
- [ ] **3.17.2 "Can't connect" button → network-interface picker** (SPEC Q49 fallback). Note: 3.17.1 (TV-side reconnect QR overlay) effectively shipped as 4.15.8's upper-right overlay; this 3.17.2 is the remaining UI.
- [ ] **3.19.6 Fandom CC BY-SA attribution surface on the phone.** Footer link, About screen, or per-figure credit (blocks on 6.3). Placement TBD once Phase 4 layout settles. **Pre-release blocker** (Phase 7 gate).

---

## Phase 4 — Aesthetic + UX pass

Phase 3 got the app *working*. Phase 4 makes it feel like Skylanders. Two intertwined workstreams:

- **Visual / aesthetic** — gold bezels, starfield, typography, ambient + state-transition animations. References: `docs/aesthetic/ui_style_example.png`, `kaos_lair_feel.png`.
- **UX / information architecture** — the portal-vs-browser drawer model (SPEC says they're separate; current implementation had them co-mounted), header composition, picking-mode flow, modal stack, navigation.

**Direction locked: Option A — Heraldic** (thick embossed gold bezels, Titan One display + Fraunces body, starfield, filigree plaques). Mocks: `docs/aesthetic/mocks/`. Options B (arcane hex) and C (modernized thin-ring) filed for reference.

**Milestones.**
- A (4.1 – 4.3): design tokens + per-screen mockups + IA agreed. ✅
- B (4.4 – 4.14): reskin + transitions landed screen-by-screen. ✅
- C (4.15 – 4.17): egui launcher parity, e2e selector fix-ups, demo. In flight (4.15.9, 4.15.12/13 open; 4.16/4.17 pending parity).
- D (4.18 – 4.19): phone + launcher drift reconciliation (code vs shipped design). In flight.

### 4.1 Design tokens + CSS architecture

- [x] 4.1.1 Palette: starfield blues, gold bezel stops, per-element gradients, status colors. Kaos (5.4) variants defined upfront as sibling vars.
- [x] 4.1.2 Typography: Titan One (display) + Fraunces (body), self-hosted OFL under `phone/assets/fonts/`.
- [x] 4.1.3 Easing + timing tokens — spring, sweep, tap, impact, shudder, halo, idle-float.
- [x] 4.1.4 Body-class swap (`.skin-kaos`) repalettes the whole app without touching component CSS.
- [x] 4.1.5 `prefers-reduced-motion` disables ambient drift + halo rotations app-wide.

### 4.2 Static mockups

Standalone HTML/CSS in `docs/aesthetic/mocks/` (one screen per file).

- [x] 4.2.1 Portal view with all slot states — `option_a_heraldic.html`.
- [x] 4.2.2 Slot state-transition demo — `transitions.html`.
- [x] 4.2.3 Profile picker — `profile_picker.html`.
- [x] 4.2.4 PIN keypad — `pin_keypad.html`.
- [x] 4.2.5 Game picker — `game_picker.html`. Box art as faded background behind each title.
- [x] 4.2.6 Browser / collection view (toy box lid) — `portal_with_box.html`.
- [x] 4.2.7 Picking-mode — integrated into box-open state; no separate mock needed.
- [x] 4.2.8 Profile creation flow — `profile_create.html` (name → color → PIN → confirm).
- [x] 4.2.8b **(Konami gate)** Konami-gated profile management — `profile_manage.html`. Gate = ↑↑↓↓←→←→BA; wrong input resets whole sequence. Delete is hold-to-confirm.
- [x] 4.2.8b **(figure detail)** Figure detail view — `figure_detail.html` (default / loading / errored states). *Numbering dupe is pre-existing; kept to avoid breaking back-references.*
- [x] 4.2.9 Takeover / Kaos — `kaos_takeover.html` + `kaos_swap.html`. Full Kaos palette; app-wide skin swap deferred to 5.4.
- [x] 4.2.10 Resume modal, reset-to-fresh confirm, show-join-code — `resume_prompt.html`, `reset_confirm.html`, folded into `menu_overlay.html`.
- [x] 4.2.11 Screen-transition animation demo — `screen_transitions.html`. Locks direction + timing (420ms ease-spring default). Custom polish effects land in 4.14.
- [ ] 4.2.12 Review round with user. Iterate before touching Leptos.
- [x] 4.2.13 Contrast + readability cleanup pass — opaque-text contract added to `design_language.md` §2; 43 alpha→solid edits across 10 mocks. Sub-items 4.2.13.1–12 all landed (swipe-gesture deferral shipped into the Leptos `<Browser>` with pointer-capture drag thresholds).
- [x] 4.2.14 Destructive confirmation: hold-to-activate pattern — ~1.2s press-and-hold with progress fill. Applied to RESET + SHUT DOWN. REMOVE stays single-tap (recoverable). Post-action animations documented per-action (4.2.14.a — RESET local, CHOOSE ANOTHER GAME + SHUT DOWN broadcast, SWITCH PROFILE local).
- [x] 4.2.15 Portal slot loaded-selection state (REMOVE overlay) — auto-dismiss 5s, single-tap confirm.
- [x] 4.2.16 Connection-lost overlay — `connection_lost.html`.
- [x] 4.2.17 Empty states — `empty_states.html` (no profiles / no matches / no games).

### 4.3 UX reorganization — information architecture

- [x] 4.3.1 **Portal vs browser: toy box lid.** Portal is primary (2×4 grid). "Collection" lives in a wood-textured lid at top. Tap/swipe-up opens; swipe-down expands filters, again closes; scrolling the grid auto-collapses. Gesture table in `design_language.md` §6.7.
- [x] 4.3.2 **Header composition.** Left→right: kebab (⋮) → profile swatch → profile name + current game → connection pip. Actions consolidated into the kebab menu overlay (4.12.4b).
- [x] 4.3.3 **Picking-mode flow.** Tapping an empty slot opens the toy box. Server tracks which slot is being picked for. Tapping a loaded slot shows Remove/Reset directly on the portal.
- [x] 4.3.4 **Modal stack semantics** — `navigation.md` §2. Three categories (full-screen replacements, scrim modals, inline overlays), no stacking (ConnectionLost always wins).
- [x] 4.3.5 **Navigation map** — `navigation.md` §1. Deeper = slide-up, back = slide-down, modals = scrim-fade.
- [x] 4.3.6 **Collection default sort — game-compatible, then last-used.** `crates/core/src/compat.rs` holds the heuristic.
- [x] 4.3.7 **Figure detail view flow.** Tap figure → panel lifts in, others dim to 25%. PLACE ON PORTAL / BACK TO BOX. Detail view stays put on failure so user can retry.

### 4.4 Shared Leptos components

- [x] 4.4.1 `<GoldBezel>` — circular gold frame with element-tinted inner plate. Props: size, element, state, child.
- [x] 4.4.2 `<FramedPanel>` — parchment-blue panel with gold gradient border. No corner brackets.
- [x] 4.4.3 `<DisplayHeading>` — two-tone outlined title (gold fill + dark-gold stroke + drop-shadow).
- [x] 4.4.4 `<RayHalo>` — rotating conic-gradient halo masked to a ring.
- [x] 4.4.5 `<FigureHero>` — lifted figure presentation (oversized bezel + aura + rotating rays). State prop (`default`/`loading`/`errored`). Reused by Kaos swap overlay (5.3).

### 4.5 Starfield background + ambient motion

- [x] 4.5.1 Layered starfield on `body` (radial gradients + tiled SVG stars + ~40s parallax drift).
- [x] 4.5.2 `<MagicDust>` sparse particle layer (24 particles); `prefers-reduced-motion` hides entirely.

### 4.6 Portal view reskin + state transitions

- [x] 4.6.1 Slots render through `<GoldBezel>`. Empty = dimmed bezel with "+".
- [x] 4.6.2 Empty → Picking: scale 1.05 spring, outer gold glow, `<RayHalo>` slow rotation.
- [x] 4.6.3 Pick → Loading: halo speeds up, gold sweep around ring (~1s), inner plate dims 20%.
- [x] 4.6.4 Loading → Loaded: "portal impact" radial flash (~400ms), bezel brightness spike, 4s idle float (±2px).
- [x] 4.6.5 Loaded → Cleared: desaturate + shrink, fade thumb to element plate (~200ms).
- [x] 4.6.6 Errored: red-tinted bezel, shake + subdued red glow.
- [x] 4.6.7 Slot tap feedback: inner-plate dent, 0.96 scale spring-back.

### 4.6b Figure detail view

- [x] 4.6b.1 `<FigureDetail>` component. Props: figure, placed_by, on_back, on_place. States: idle / loading / errored.
- [x] 4.6b.2 Entrance: other figures + box crossfade to 25%; selected `<FigureHero>` lifts via FLIP-style transform; panel fades in.
- [x] 4.6b.3 Action-icon row: three `<BezelButton>`s (appearance / stats / reset) disabled until 3.14, 6.3, 3.11.3 wire real handlers.
- [x] 4.6b.4 Stats preview strip — placeholder values (6.3 wires the stats endpoint).
- [x] 4.6b.5 PLACE ON PORTAL → loading ring; on WS `SlotChanged` → box closes, portal-impact fires. Client-side 8s timeout flips to errored.
- [x] 4.6b.6 BACK TO BOX stays enabled in loading/errored — backing out doesn't cancel the load.
- [x] 4.6b.7 Server contract unchanged: phone sends figure_id; server picks the slot.

### 4.7 Browser view reskin

- [x] 4.7.1 Figure cards use a smaller `<GoldBezel>` portrait.
- [x] 4.7.2 Element chips — gold-bordered pills with element-tinted fills.
- [x] 4.7.3 `on-portal` state: desaturated bezel + "ON PORTAL" gold ribbon.
- [x] 4.7.4 Search input: gold shimmer sweep on focus.
- [x] 4.7.5 Empty/filtered-out state: themed illustration + copy.

### 4.8 Profile picker reskin

- [x] 4.8.1 Big `<DisplayHeading>` "WELCOME, PORTAL MASTER".
- [x] 4.8.2 Oversized gold-bezeled swatches with display-font initial; profile colour tints inner plate.
- [x] 4.8.3 "+ Add profile" as prominent bezel card.
- [x] 4.8.4 Entry animation: cards bloom from center, 80ms stagger.

### 4.9 PIN keypad reskin

- [x] 4.9.1 `<FramedPanel>` surround. PIN dots are mini gold bezels.
- [x] 4.9.2 Key press: inset-shadow dent + bounce (<100ms).
- [x] 4.9.3 Unlock success: shockwave ring + gold L→R streak as panel fades.
- [x] 4.9.4 Lockout: red-tinted panel, countdown in display font, keys disabled (visible, not hidden).

### 4.10 Profile admin reskin

- [x] 4.10.1 `<FramedPanel>` surround; themed inputs with gold focus outline.
- [x] 4.10.2 Color-picker swatches as mini bezels.
- [x] 4.10.3 Destructive actions clearly red-tinted.

### 4.11 Game picker reskin

- [x] 4.11.1 Game cards with room for per-game artwork (placeholder if no assets).
- [x] 4.11.2 Stagger-rise entry (80ms per card).
- [x] 4.11.3 Selected-card confirmation flash before WS flips to portal view.

### 4.12 Modals + takeover

- [x] 4.12.1 Resume-last-setup modal — `<FramedPanel>` + figure-preview bezels + Resume / Start fresh CTAs.
- [x] 4.12.2 Reset-to-fresh confirm — red-bezeled panel, hold-to-confirm, gold-flake fall + desaturation on fire (per 4.2.14.a).
- [x] 4.12.3 Takeover/Kaos screen polish (stays blue; Kaos skin ships with 5.4).
- [x] 4.12.4 Show-join-code sheet — folded into 4.12.4b menu QR card. Shell-only; real QR content wiring tracked in 3.10.8 carryover.
- [x] 4.12.4b **Menu overlay** (header kebab). Single surface: join QR, profile chip, three actions (SWITCH PROFILE / CHOOSE ANOTHER GAME / SHUT DOWN). Mock: `menu_overlay.html`. Absorbs 3.10.8's UI.

### 4.13 Toasts redesign

- [x] 4.13.1 Color-coded left strip (error / warn / success / info).
- [x] 4.13.2 Slide-in from top-right for non-blocking; bottom-center kept for critical errors.

### 4.14 Ambient polish

- [x] 4.14.1 Screen-to-screen transitions: cross-fade + direction-based motion. `NavDir` signal + `screen_cls` helper captures direction at mount time so animations don't re-trigger mid-screen.
- [x] 4.14.2 Connection pip: breathe while connecting, steady green when connected, soft red when disconnected. `prefers-reduced-motion` neutralizes.

### 4.15 egui TV launcher

Design source-of-truth: `docs/aesthetic/mocks/tv_launcher_v3.html`. All cloud assets procedurally generated (no shipped frames).

#### 4.15a Design cycle

- [x] 4.15a.1 Initial HTML mock (CSS conic) — superseded and removed.
- [x] 4.15a.2 State machine documented in `navigation.md` §3 (Startup → Booting → Compiling Shaders → Awaiting Connect → Players Joined → Max Players → In-Game → Shutdown + Crashed + Switching Games branches).
- [x] 4.15a.3 Procedural cloud — WebGL simplex-FBM fragment shader with 10 iris arms. Cylindrical sampling fixes polar seam. Three independent knobs: `irisRadius` / `rotationSpeed` / `inflowSpeed`. Arm spiral decoupled from inflow to avoid nausea.
- [x] 4.15a.4 QR + player orbit (folded into v3 states 3–5; card-flip at max players).
- [x] 4.15a.5 In-game transparency + shutdown (folded into v3 states 7–8). Reconnect QR: upper-right, hidden by default, visible only when all phones disconnected.
- [ ] 4.15a.6 Review round — iterate before touching egui.
- [ ] 4.15a.7 **Port WebGL shader → WGSL via `egui_wgpu` custom paint callback.** 4.15.5 ships a polar-mesh approximation; full port gives the organic-fluff look that matches `tv_launcher_v3.html` 1:1. Requires flipping eframe backend `glow → wgpu` in `crates/server/Cargo.toml`. Deferred until after 4.15.6–4.15.13 so we don't churn on visual tuning twice.

#### 4.15b egui implementation

- [x] 4.15.1 Shared palette in `crates/server/src/palette.rs` mirroring phone CSS tokens.
- [x] 4.15.2 Titan One registered as named font family; launcher heading in Titan One gold.
- [x] 4.15.3 QR framed in gold bezel (`qr_in_gold_bezel` — stacked `egui::Frame`). Gradient + multi-layer inset shadows deferred; revisit via custom `egui::Painter` pass if the TV reads flat.
- [x] 4.15.4 Status strip: RPCS3 pip + current-game name. `LauncherStatus` on `AppState` behind a `Mutex`, updated by `/api/launch` + `/api/quit`.
- [~] 4.15.5 **Procedural cloud vortex — approximation shipped, full port pending.** Polar-mesh in `crates/server/src/vortex.rs` (~2k triangles/frame via native `Painter::add(Mesh)`). Captures the iconic shape (iris knob, 10 arms, centre hole). Sin/cos band noise stands in for simplex FBM — reads as banded density rather than organic fluff at 10 ft. Repaint at 60 fps. Full WGSL port tracked at 4.15a.7.
- [x] 4.15.6 QR card-flip on max-players (Y-axis tent-wave scale, content swap at midpoint).
- [x] 4.15.7 Player-orbit indicators. `SessionPip { color, initial }` for each registered session.
- [x] 4.15.8 **In-game transparency.** eframe `.with_transparent(true)` + `.with_always_on_top()` in release. `boot_game_by_serial` refactored to `PostMessage(WM_LBUTTONDOWN/UP + WM_KEYDOWN/UP)` targeting RPCS3's HWND with window-relative coords — cover stays fully topmost + interactive. New `ui/in_game.rs` surface. Reconnect QR panel anchors 32px inside upper-right when `connected_clients == 0`.
- [ ] 4.15.9 **Game-switching transition.** Phone picks another game → clouds spiral in → "SWITCHING GAMES…" → RPCS3 loads new game → clouds spiral out. Input routing: same PostMessage approach as 4.15.8; File→Exit keyboard nav in `quit_via_file_menu` must also move from `SendInput` to `PostMessage(WM_KEYDOWN/UP)` to avoid landing Alt+arrow keys in the egui cover.
- [x] 4.15.10 **Crash recovery screen.** `LauncherScreen::{Main, Crashed{message}, Farewell}` enum. 500ms-poll watchdog flips screen + broadcasts `GameCrashed`. "SOMETHING WENT WRONG" + RESTART button. Auto-respawn of RPCS3 still TODO (inline comment); today's button just dismisses.
- [x] 4.15.11 **Shutdown farewell.** Signed `POST /api/shutdown` → `render_farewell` (3s countdown + `ViewportCommand::Close`). `ctx.request_repaint_after` keeps the countdown ticking.
- [ ] 4.15.12 **Shader compilation detection (research spike).** Investigate (a) log-file watcher for "Compiling shader" patterns, (b) viewport title polling, (c) FPS <5 for >5s heuristic. Fallback: fixed 15s delay post-boot.
- [ ] 4.15.13 **Shader progress visualization** (depends on 4.15.12). Gold conic-gradient ring (200–240px), count in centre in Titan One, ring flashes on completion.
- [x] 4.15.14 **Phone-side game-crash overlay.** `Event::GameCrashed{message}` + `GameChanged{current:None}` broadcast by the crash watchdog; phone `GameCrashScreen` with "RETURN TO GAMES" button. Stacked outside `TakeoverScreen` so a crash preempts every other screen state.
- [x] 4.15.15 **Research spike: cover + input routing.** Tried three Z-order strategies against real RPCS3; chose (c) opaque `WS_EX_TOPMOST` cover + `PostMessage` to RPCS3. No cover flash, cover stays interactive, fastest (0.95 s boot). Probe: `crates/rpcs3-control/examples/zorder_probe.rs`. Recommendations written into 4.15.8 + 4.15.9.
- [x] 4.15.16 **RPCS3 lifecycle: launch at server startup.** Done 2026-04-21. RPCS3 now spawns in main.rs's tokio task right before `axum::serve`, gated on `driver_kind == Uia` (mock skips). The wait_ready blocks axum startup, which is the right trade — the egui launcher's `LaunchPhase::Startup` cloud vortex carries the user through the wait, and a failed spawn hard-fails via `report_fatal` straight to `LauncherScreen::ServerError`. `/api/launch` dropped its `launch_library` + `wait_ready` block: it now just `EnumerateGames` + `BootGame` against the already-running RPCS3, with precondition `process.is_some() && current.is_none()` (503 if startup still in flight). `/api/quit` dropped `shutdown_graceful` in favour of `DriverJob::StopEmulation`: the process stays alive, only `current` clears. `?force=true` keeps the old kill-and-respawn path as a wedge-breaker. `/api/shutdown` now stops any running game + shuts down RPCS3 gracefully before flipping to Farewell so caches + config flush cleanly. Crash watchdog (PLAN 4.15.14) gained auto-respawn: on process death it broadcasts `GameCrashed` (only if a game was running), then immediately calls `launch_library` + `wait_ready` to restore the always-running contract. Library-view crashes during auto-respawn are silent (cloud vortex covers the restart). After 3 consecutive respawn failures, launcher flips to `ServerError`. New surface: `PortalDriver::stop_emulation(timeout)` + `DriverJob::StopEmulation`. UIA impl walks for a Button/MenuItem named "Stop" / "Stop Emulation", prefers `UIInvokePattern`, falls back to `PostMessage` click on the bounding rect. Unblocks 3.7.8 phase 2 (truth-from-UIA picker at startup, since RPCS3 is always running at library view). Open: the stop button / menu item name detection may need probe validation on the HTPC — the candidate list (`stop`, `stop emulation`, `&stop`, `stop game`) covers the likely Qt 6 names but if RPCS3's build has a different label we'll see a "no Stop button or menu item found" error from `/api/quit`.

### 4.16 E2E test updates

- [ ] 4.16.1 Audit `crates/e2e-tests/` selectors after the reskin — where possible move from class-name matches to stable `data-test` attributes so the next redesign doesn't break the harness.
- [ ] 4.16.2 Visual regression out of scope — manual review only.
- [ ] 4.16.3 Manual multi-phone visual sanity on the HTPC (both phones running, picking flows, resume modal, takeover screen).

### 4.17 Review checkpoint

- [ ] 4.17.1 Demo on HTPC end-to-end: launcher readability → phone scans → profile picker → PIN → game picker → portal in all five slot states → takeover flow.
- [ ] 4.17.2 Catalogue UX papercuts, route them to Phase 3 carryover (ownership badge, show-join-code, 3-option resume, crystal confirm, attribution).

### 4.18 Phone UI drift reconciliation

Mac-side browser smoke-test 2026-04-17 found drift between shipped Leptos and the Phase 4 mocks. Reframe 2026-04-18: **code behavior pinned by tests is the long-term truth; mocks are point-in-time references.** Each drift is evaluated on its merits; the chosen outcome is locked in with a test where coverage is missing.

Item tags: **[bug]** wrong behavior, **[feature]** missing capability, **[judgment]** mock is one opinion shipped code is another, **[verify]** may already be done.

- [~] 4.18.1 **Mobile viewport / address-bar doesn't hide.** Partial fix landed 2026-04-17: `100dvh` + `100svh` fallbacks, `env(safe-area-inset-top)` on `.app` padding, landscape-block overlay. iOS Safari won't auto-hide its chrome without scrollable content → the workable path is "install to home screen" as a PWA. Follow-ups: 4.18.1a ✅, 4.18.1b ✅, 4.18.1c open.
- [x] 4.18.1a **mDNS/Bonjour advertisement for stable URL.** Done 2026-04-19. `crates/server/src/mdns.rs` builds `http://<os-hostname>.local:<port>/#k=…` via `GetComputerNameExW`; relies on Windows 10 v2004+'s auto-published `<computername>.local`. Prior iterations (mdns-sd, native `DnsServiceRegister` with custom A record) both failed because Windows owns the mDNS responder and ignored our advertisements. Diagnostic `#[ignore]` test `os_hostname_resolves_via_local` catches regressions. Closes 4.19.10b.
- [x] 4.18.1b **"Add to Home Screen" hint.** Done 2026-04-19. `phone/src/pwa.rs` gates on `is_ios_safari() && !is_standalone() && !hint_dismissed()`. Banner on profile picker; "NOT NOW" writes localStorage. 10 unit tests on the decision truth table.
- [ ] 4.18.1c **Service worker for PWA cache + update detection.** Today static assets return `Cache-Control: no-store`. Add `phone/assets/sw.js`: hashed wasm/js/css/font immutable, `index.html` + manifest `no-cache`, delete stale cache entries on activation, post "new version" message to running SPA. Only mechanism that survives iOS PWA's app-shell caching across long backgrounding.
- [x] 4.18.1d **iOS dark-band at bottom — fixed.** Chris flagged 2026-04-21. Tried (1) `height: 100dvh` on body + `overscroll-behavior: none`, (2) `background-attachment: fixed` on body, (3) `body::before { position: fixed; inset: 0 }` gradient layer. Third attempt matches the `body::after` starfield pattern that already works on iOS, but a thin dark band still renders at the bottom of the visible viewport on Chris's iPhone. Headless Chrome can't reproduce (no address bar transition; visible area equals `100dvh` exactly) — see `crates/e2e-tests/tests/visual_debug_background.rs` harness. **Candidates to investigate**: (a) iOS home-indicator strip rendering over the PWA at a depth our pseudo can't cover, (b) `env(safe-area-inset-bottom)` padding on `.app` stealing height from the visible background area, (c) iOS compositor layer caching the pseudo against an outdated viewport rect when the URL bar retracts. Low priority until we have a decisive repro (physical device + Web Inspector). **Update 2026-04-21**: `tools/ios-inspect/` (PLAN 4.21) now provides a sim-side probe loop — re-attempt tracked as 4.21.4, caveat that sim reports `safe-area-inset-bottom = 0` so candidate (b) needs PWA-standalone mode or real-device validation. **Diagnosis complete (4.21.4)**: dark band is `html`'s flat `#020818` exposed below the fixed `body::before` pseudo when iOS retracts the URL bar — color-seams against the gradient's lighter bottom-center radial highlight (~`#0e2464`). Fixed by 4.18.1e (gradient onto html) + follow-up `html { min-height: 100vh }` (commit `a785499`, 2026-04-22) — without the min-height, html's auto-height tracked content (~845px on iPhone) while the URL-bar-retracted viewport is ~874px, leaving a ~29px strip unpainted. Chris confirmed clean on real device 2026-04-22.
- [x] 4.18.1e **Fix dark band by moving gradient off `body::before` onto `html`.** Done 2026-04-21 (sim-validated; real-device confirmed 2026-04-22 after a `min-height: 100vh` follow-up on html — see 4.18.1d). In `phone/assets/app.css`: replaced `html { background: var(--sf-3); }` with the full 3-layer gradient stack (top radial highlight, bottom radial highlight, linear sf-1→sf-2→sf-3), and deleted the `body::before` rule entirely (its only job was carrying the gradient). `body` stays transparent with its 100dvh + overflow:hidden + overscroll-behavior:none layout behaviour. `html`'s bg paints across the full canvas including the URL-bar-retraction region, so the colour seam that 4.21.4 diagnosed can't form. **Kaos skin: class goes on `<html>`, not `<body>`.** Moved both `body.skin-kaos { --sf-*: … }` and `body.skin-kaos { background: … }` to `html.skin-kaos` (body-scoped custom properties can't cascade to ancestors, so the html gradient rule would never see Kaos's overrides otherwise). Added comment in §4.1.4 flagging that the future pwa.rs skin-toggle hook must target `document.documentElement`, not `document.body`. Sim validation via `ios-inspect`: `getComputedStyle(document.documentElement).backgroundImage` now contains both `radial-gradient` and `linear-gradient`; `body::before` reports `content: none` (pseudo doesn't render); screenshots before/after are visually identical in the happy case; the forced-exposure probe (`body::after { bottom: 50px }` to simulate URL-bar retract) shows the gradient continuing unbroken where previously it showed the dark-band seam (see `/tmp/probe_exposed_html.png` vs `/tmp/after_fix_exposed.png`).

**Shared header.**

- [x] 4.18.2 Drop "Skylander Portal" brand text. Done 2026-04-18.
- [x] 4.18.3 Profile swatch beside kebab. Done 2026-04-18 (`<GoldBezel size=Sm>` with profile initial + `--profile-color`).
- [x] 4.18.4 Pulsing pip only (text label removed; `aria-label` + `title` kept). Done 2026-04-18.
- [x] 4.18.5 Single Header component handles all states reactively via `Option::map`. Done 2026-04-18.
- [x] 4.18.5a MANAGE PROFILES moved into kebab menu overlay (gear ⚙). Done 2026-04-18.
- [x] 4.18.5b MenuOverlay context-aware: SWITCH PROFILE + HOLD TO SWITCH GAMES gated by `<Show>`. Done 2026-04-18.
- [~] 4.18.5c **Menu overlay → Konami-gate transition.** Mocks done 2026-04-18. **Bug half-done** (empty chip hidden via `<Show when=unlocked_profile.is_some()>`, commit `439e0d4`). **Judgment open**: gate-rise + entry-cascade animations vs a plain cross-fade. Whichever lands, lock DOM class sequencing with a test.

**Profile picker / create / manage.**

- [ ] 4.18.6 *[judgment]* CreateProfileForm pacing: single form vs 4-step wizard.
- [x] 4.18.6a *[bug]* **CreateProfileForm visual parity with PinEntry.** Done 2026-04-21. Passed a constant-false `locked_out` signal into both `PinPad` calls so they render the heraldic gold-bezel keypad instead of the Phase-2 legacy grey keys. Introduced a small `<HeraldicPinDots>` helper that reuses the existing heraldic `.pin-dot-ring` / `.pin-dot-fill` markup without requiring the caller to wrap in `.pin-entry-screen`; the CSS scoping rule widens to `.pin-entry-screen .pin-dot, .pin-dots-heraldic .pin-dot`. Action buttons (`.btn-primary` / `.btn-cancel`) were already gold heraldic — the user's "buttons don't match" complaint was the keypad glyphs (sans-serif `.pin-key` at 22px vs Titan One `.pin-hkey` at 24px), now resolved.
- [x] 4.18.7 *[feature]* Prefilled random Skylander name + reroll button. Done 2026-04-21. Name field now prefills with a random pick from a 20-name `SKYLANDER_NAMES` const (matches the mock's list). `.roll-btn` next to the input (gold coin, 54px, ↻ glyph, rotates -60° on press) picks a new one. Hint line beneath: "anything you like · or tap ↻ for a random one". No submit-time whitelist per PLAN 4.2.8.
- [x] 4.18.8 *[bug]* PIN confirm mismatch feedback. Done 2026-04-19. Second `<PinPad>` labelled "Confirm PIN". Mismatch → `.shake` + `.pin-mismatch-banner` + confirm-entry wipe. Keypad UX prerequisites (no iOS double-tap zoom, visible press-flash via `.pressed` class on `pointerdown`) landed alongside.
- [ ] 4.18.9 *[judgment]* PIN reset: 1-step vs 2-step (Konami gate as authentication vs defence-in-depth).
- [ ] 4.18.10 *[feature]* Profile "last used N days ago" / "never used" subtext. Needs `MAX(figure_usage.last_used_at)` or `profiles.last_used_at`.
- [ ] 4.18.11 *[verify]* Profile manage DEL uses HOLD TO DELETE pattern, not `window.confirm()`.

**Game picker.**

- [ ] 4.18.12 *[feature]* Per-card tagline + "currently playing" marker.

**Portal + Browser.**

- [x] 4.18.13 *[bug]* "PORTAL" `<DisplayHeading>` above slot grid. Done 2026-04-19.
- [ ] 4.18.14 *[feature]* GAMES drill-down chip row in `BrowserHead`.
- [ ] 4.18.15 *[feature]* CATEGORY drill-down chip row (Vehicles / Traps / Minis / Items). Additive with elements.
- [ ] 4.18.16 *[verify]* Toy-box lid grabber pill + swipe-hint copy shipped in Leptos `<Browser>`.
- [x] 4.18.17 *[bug]* Ownership pip per loaded slot. Done 2026-04-19. Bottom-right pip with profile colour + initial, dimmed during Loading. `resolve_owner` helper + 4 unit tests. Final aesthetic styling pass tracked as 3.10.7 carryover.

**Figure detail.**

- [x] 4.18.18 *[bug]* Action buttons gained visible labels (APPEARANCE / STATS / RESET). Done 2026-04-19.
- [ ] 4.18.19 *[verify]* Hero-aura + hero-rays paint behind lifted figure bezel.
- [ ] 4.18.20 *[judgment]* Ghost-grid / box-backdrop context on figure detail (replacement vs 25%-opacity overlay).

**Modals & overlays.**

- [x] 4.18.21 *[bug]* ConnectionLost overlay. Done 2026-04-19. Pulsing red pip, reconnect spinner, manual TRY AGAIN after 3 failed auto-retries. Persistence fix: drives off `reconnect_attempts > 0` rather than `conn == Disconnected` so overlay stays visible through the reconnect cycle. E2E regression in `crates/e2e-tests/tests/connection_lost.rs`. Phone→server log-forwarder (`POST /api/_dev/log` + `dev_log!` macros) ships disconnect traces to the launcher's PowerShell.
- [ ] 4.18.22 *[feature]* ResumeModal element-tinted bezel plates.
- [ ] 4.18.23 *[feature]* ResumeModal relative-time subtext ("saved today" / "N days ago"). Needs `saved_at` on `ResumeOffer`.
- [ ] 4.18.24 *[judgment]* MenuOverlay post-action transitions (identity-drain / fold-away / lights-dim vs shared clean exit).

**Wrap-up.**

- [ ] 4.18.25 Re-run browser smoke-test on real iOS after 4.18.1c ships; walk each screen against the agreed behavior, add missed drift items.
- [ ] 4.18.26 Once parity reached, 4.17.1's end-to-end demo can proceed against a known-correct phone UI.

### 4.19 egui TV-launcher drift reconciliation

Same lens as 4.18: code + tests are truth, mocks/spec are reference.

- [x] 4.19.1 Inventory drift. Done 2026-04-19. Headline: spec defines 8 states, code collapses to 3 (Main / Crashed / Farewell). Several visual moments never render. Itemised as 4.19.2–4.19.22.

**States (§3.1) — state machine collapsed.**

- [ ] 4.19.2 *[feature]* **No "Booting" surface.** Spec: iris closes + "LOADING" + game name + boot status. Today `main_screen.rs` renders QR + brand heading even mid-launch.
- [x] 4.19.2a *[feature]* **Startup surface.** Done 2026-04-19 (visuals refined same day). `crates/server/src/ui/launch_phase.rs` introduces `LaunchPhase::{Startup, Transitioning, AwaitingConnect}` derived from `time_since_mount` + activity. Reusable infra shipped alongside: `vortex::paint_sky_background`, `paint_starfield`, `paint_radial_ellipse` (now `pub`), `paint_vertical_gradient`, `paint_heraldic_title`. `palette::apply` now pins `ThemePreference::Dark` first (fixes Windows-11 light-mode clobber).
- [ ] 4.19.3 *[feature]* **No "Switching Games" surface** (iris-close between In-Game and next Booting).
- [ ] 4.19.4 *[feature]* **No "Compiling Shaders" surface** (depends on 4.19.17 detection).

**Cloud + iris (§3.2) — tuning is static.**

- [ ] 4.19.5 *[judgment]* **Vortex shader: polar-mesh approximation vs spec'd simplex FBM.** Downgraded from [feature] 2026-04-19 — the surrounding improvements (sky backdrop + starfield + halo glow) carry most visual weight. Re-eval before committing to the `egui_wgpu` port.
- [ ] 4.19.6 *[feature]* **Iris locked at 1.2 for every state.** Spec calls for per-state tuning (Booting 2.5s ease-out, Crash ~1s urgent, Shutdown gentle, In-Game ~1.8s ease-in). Coupled with 4.15a.7 polish.
- [ ] 4.19.7 *[verify]* **Halo focal-glow missing** behind QR / progress ring. Primitive ready: `paint_radial_ellipse`.
- [ ] 4.19.8 *[verify]* **Pip orbit speed: code 0.10 rad/s vs spec 0.08.** Either fix constant or update comment.

**QR + orbit (§3.3).**

- [ ] 4.19.9 *[bug]* **Max-players copy mismatch.** Code: "MAXIMUM PLAYERS REACHED"; spec: "PORTAL IS FULL".
- [ ] 4.19.10 *[verify]* **"SCAN TO CONNECT" label position + size** (above 36px vs below 64px).
- [ ] 4.19.10a *[bug]* **URL string rendered on screen** — spec says no URL text, QR carries the URL. Drop the `RichText` render.
- [x] 4.19.10b *[feature]* **QR URL mDNS-based.** Closed 2026-04-19 alongside 4.18.1a.
- [ ] 4.19.11 *[verify]* **QR bezel size: 320px code vs ~280px spec.** Likely intentional 4K headroom; confirm on TV.
- [ ] 4.19.22 *[bug]* **Awaiting Connect surface carries debug noise.** Spec shows only QR + heraldic label; today's render adds brand heading + status strip + URL + client count + figures-indexed debug. Drop all five. Keep Exit-to-Desktop button (not in mock; pragmatism).
- [x] 4.19.23 *[feature]* **Server-error launcher state.** Done 2026-04-19. `LauncherScreen::ServerError{message}`; tokio thread reports startup failures via `report_fatal`. Replaces the old `expect("bind")` panic so a port-in-use no longer takes down the process.

**In-Game transparency (§3.4).**

- [ ] 4.19.12 *[bug]* **Reconnect QR has no fade-in animation** (spec: 1.0s ease-out).
- [ ] 4.19.13 *[verify]* **Reconnect QR copy + inset** ("scan to rejoin" 11px 32px-inset vs "REJOIN" 60px-inset).

**Shutdown (§3.5).**

- [ ] 4.19.14 *[bug]* **No breathe pulse on farewell heading** (spec: 2.4s opacity + scale ±2.5%).
- [ ] 4.19.15 *[feature]* **No black-overlay fade-out + hint sequence.** egui has no native full-screen overlay equivalent; either implement via custom paint or accept current behavior.
- [ ] 4.19.16 *[verify]* **Heading size: 72px code vs 64px spec.**

**Shader compilation (§3.6).**

- [ ] 4.19.17 *[feature]* **Detection unimplemented.** Duplicates 4.15.12 — whichever lands first closes both.
- [ ] 4.19.18 *[feature]* **Progress ring + heading missing** (depends on 4.19.17). Duplicates 4.15.13.

**Typography (§3.7).**

- [ ] 4.19.19 *[verify, half-done]* **Hero size 80px code vs 96px spec**; farewell 72 vs 64. 4.19.2a landed 140px embossed for Startup; steady-state main + crash + farewell still flat-and-small. Extracting from `vertical_centered` into direct painter calls is the remaining work.

**Wrap-up.**

- [ ] 4.19.20 Re-walk every state on the HTPC once 4.19.2–4.19.19 land.
- [ ] 4.19.21 Once parity reached, launcher can be demo subject of 6.4.

### 4.20 Design system consolidation

Close the drift between `docs/aesthetic/design_language.md`, the shipped phone CSS, and the Leptos component tree. Three workstreams, each sub-item individually shippable:

- **A. Finish the component extraction.** `design_language.md` §6 lists 11 components; only 6.1–6.5 landed as `phone/src/components/*.rs`. 6.6–6.10 are inlined inside screens, duplicating material styles and making the Kaos skin swap (5.4) harder than it needs to be.
- **B. Standardize constants.** Typography scale + several motion tokens are documented in §2 / §5 but not declared in `:root` — shipped CSS hardcodes `font-size: Npx` 80+ times. Declare the tokens, migrate the call sites.
- **C. Fix the doc drift** caught in the 2026-04-20 audit.

**Component extraction (A).**

- [~] 4.20.1 Extract `<ActionButton>` (§6.6). Done 2026-04-20 for the menu-action call sites. New `phone/src/components/action_button.rs` exposes `<ActionButton title description icon variant hold_duration on_fire>` covering single-tap and press-and-hold modes; the latter owns the holding/fired state machine + spawn_local timer + `.fired` linger so callers just provide an `on_fire: Callback<()>`. Reuses the existing `.menu-action*` CSS so MenuOverlay needs no CSS change. MenuOverlay's 4 actions (SWITCH PROFILE, MANAGE PROFILES, HOLD TO SWITCH GAMES, HOLD TO SHUT DOWN) all migrated; modals.rs −136 / +65 lines for the same surface. ResetConfirmModal's HOLD TO RESET button stays inline — its red-bezel + flake-fall + bezel-drain cascade is too far from the menu-action visual to share without parameter creep. The figure_detail action row icons are circular `BezelButton`s, not blue cards — also stay separate. **Remaining**: a future sub-extract that DRYs the hold timer state machine into a smaller `use_hold_to_fire` helper for the ResetConfirmModal call site, if the duplication starts to ache. 1 unit test for `ActionVariant::css_modifier`.
- [x] 4.20.2 Extract `<ToyBoxLid>` / `<ToyBoxInterior>` (§6.7). Done 2026-04-20. Both live in `phone/src/components/toy_box.rs` (paired components share the `pub enum BoxState` so they co-locate in one file). `ToyBoxLid` owns the gesture state machine (pointerdown/move/up/cancel + tap/swipe-up/swipe-down apply functions + pointer-capture). `ToyBoxInterior` owns the scroll-collapse rule. `browser.rs` is now a thin composer: figures + filtered/loaded/loading memos + `<ToyBoxLid box_state>{<BrowserFilters/>}</ToyBoxLid> + <ToyBoxInterior box_state>{grid}</ToyBoxInterior>`. New `<BrowserFilters>` component (private to browser.rs) hosts the search input + element chip row that fills the lid's expanded area. `BrowserHead` retired; lid-internal logic moved to the component. browser.rs went 392 → 241 lines (-283/+131); toy_box.rs is the new 244-line home. `children: ChildrenFn` (not `Children`) on `ToyBoxLid` because Show's body closure must be Sync. 1 unit test for `BoxState::css_modifier`.
- [x] 4.20.3 Extract `MenuOverlay` to its own file. Done 2026-04-20. Moved from `screens/modals.rs` (was housing 5 components in 639 lines) to a dedicated `screens/menu_overlay.rs`. No API change — kept the open/profile/game/manage_gate/toasts props as-is; the `actions: &[MenuAction]` data-driven shape from the original PLAN entry was over-engineering for a single consumer. modals.rs −162 lines (now 477, holding ResumeModal + TakeoverScreen + GameCrashScreen + ResetConfirmModal + ResetFlakes). 3.10.8 later replaced the `.menu-qr-inner` placeholder with an `<img src="/api/join-qr.png">` inside `.menu-qr-frame`. Render-equivalent: trunk build green, 23 phone lib tests + 98 server tests + 6 mock pass.
- [x] 4.20.4 Relocate `Header` (§6.9) from `screens/header.rs` to `components/header.rs`. Done 2026-04-20. `git mv` preserves history; mod.rs entries swapped between screens/ and components/; `lib.rs` gained an explicit `use crate::components::Header` (was previously transitively re-exported through `screens::*`). No code change inside the file. Trunk green; 23 phone lib tests pass.
- [~] 4.20.5 Scaffold `<KaosOverlay>` (§6.10). Done 2026-04-20 for the takeover variant. New `phone/src/components/kaos_overlay.rs` holds what used to be `TakeoverScreen` in `screens/modals.rs` — renamed to `KaosOverlay` so the Phase 5.3 swap variant lands as a new branch inside the same component surface rather than a parallel screen-level one-off. Same `takeover: RwSignal<Option<TakeoverReason>>` prop + identical markup; only the call-site symbol changed. Module-scoped `#![allow(private_interfaces)]` (same reason as `game_crash_screen.rs`). Swap variant stub not included — 5.3 will add an additional signal prop and variant dispatch when the server event ships.
- [x] 4.20.5a Extract three full-screen overlays into `components/`. Done 2026-04-20. All three moved per Chris's "as components" call:
  - `ConnectionLost` — `git mv` from `screens/connection_lost.rs` to `components/connection_lost.rs` (history preserved). No file changes.
  - `GameCrashScreen` — extracted from `screens/modals.rs` (-88 lines) to new `components/game_crash_screen.rs`. `#![allow(private_interfaces)]` at module top because `#[component]` macro emits a wrapper with broader visibility than the `pub(crate)` parameter types (`GameCrashReason`, `ToastMsg` from `lib.rs`); the lint fires on macro expansion so a fn-level allow doesn't take. Crate-internal intent unchanged.
  - `PwaHint` — extracted from `screens/profile_picker.rs` (-53 lines) to new `components/pwa_hint.rs`. ProfilePicker imports it from `crate::components::PwaHint`.
  - `lib.rs` gained `use crate::components::{ConnectionLost, GameCrashScreen, Header}`. modals.rs orphan imports cleaned.
  - 23 phone lib tests pass; trunk build green.

**Token standardization (B).**

- [x] 4.20.6 **Declare typography scale in `:root`.** Done 2026-04-20. 8 tokens declared in `phone/assets/app.css` (`--t-display-hero/lg/md/sm/xs`, `--t-body`, `--t-body-italic`, `--t-body-sm`). Values match design_language.md §2 spec; ranges in the doc collapsed to single canonical values (e.g. display-md was "26-32px", now 28px) since multiple competing in-between sizes in shipped CSS were UI accidents rather than design choices. Doc §2 table updated to lock the canonical values. 4.20.7 migrates the 81 hardcoded `font-size: Npx` rules to these tokens.
- [~] 4.20.7 **Migrate hardcoded `font-size`** to tokens. Done 2026-04-20 for the 88 sites whose px exactly matched a declared token (`8`, `12`, `13`, `14`, `16`, `28`, `38`, `46` — all eight tokens covered). Pure literal-to-`var(--t-*)` swap, no rendering change. **Remaining 58 off-token sites** stay as literal px since each was a deliberate per-call-site size between tokens (`15` ×9, `20` ×7, `22` ×6, `9` ×5, `24/17/10` ×4 each, `18/42` ×3 each, `48/36/26` ×2 each, `72/68/54/40/32/12.5/8` ×1 each). Blanket-rounding would lose intent. Two follow-up choices for these: (a) add intermediate tokens (`--t-body-lg` 15, `--t-display-md-sm` 20, `--t-display-md-lg` 32) if the same off-token sizes recur enough, or (b) leave as call-site overrides per the §2 doc note. Decide once we've done a contrast/legibility pass on the actual TV — some of these are likely tunings that should propagate, others are clearly one-offs.
- [x] 4.20.8 **Declare motion tokens in `:root`.** Done 2026-04-20. Added `--dur-impact: 600ms`, `--dur-shudder: 400ms`, `--dur-sky-drift: 140s`, `--dur-hold-confirm: 1200ms` to `phone/assets/app.css`. `--dur-hold-confirm` was previously only referenced inline with a 1200ms fallback. Doc §5 timing table updated to include `--dur-hold-confirm` and to collapse `--dur-quick`'s "200-250ms" range to the shipped 200ms.
- [~] 4.20.9 **Migrate hardcoded durations** to motion tokens. Done 2026-04-20 for the unambiguous semantic-match sites: 4 targeted edits (`140s` → `--dur-sky-drift`, `0.4s` → `--dur-shudder`, `200ms` → `--dur-quick`, all 13 `0.2s` instances → `--dur-quick`). Token uses 11 → 27. **Skipped, deliberately**: (a) seconds-form synonyms whose semantic doesn't match an existing token (`0.6s` for a quote-in animation isn't `--dur-impact`/portal-flash), (b) ambiguous values where multiple tokens share a duration (`400ms` × 7 maps to either `--dur-shudder` or `--dur-panel-in` depending on use-site), (c) off-token tunings (`0.15s` × 16 is the most common — likely deserves a `--dur-tap-faster` token if recurrence holds, but that's a token-design question outside this migration). Follow-up: per-site audit of the 400ms ambiguous sites and consider adding 1-2 intermediate tokens for the 0.15s / 0.3s / 0.1s buckets.
- [x] 4.20.10 Launcher typography constants in `palette.rs`. Done 2026-04-20. Added eight pt-size constants (`HERO_INTRO`/`HEADING_LG`/`COUNTDOWN`/`HEADING`/`SUBHEAD`/`BODY`/`CAPTION`/`CAPTION_SM`) reflecting shipped launcher sizes (140/64/56/28/20/18/14/11). Migrated 11 `.size(N.0)` literals across `crates/server/src/ui/{crashed, farewell, in_game, main_screen, server_error}.rs` plus the dynamic `clamp(20.0, 56.0)` in `paint_card_text` and the FontId::new sites. navigation.md §3.7's spec values (96/64/40/32/24) diverge from the constants; reconciliation is tracked in PLAN 4.19.19, not here — the constants reflect what's actually rendering today, doc updates separately. Workspace tests green.

**Doc fixes (C).**

- [x] 4.20.11 Fix §6.1 mock reference. Done 2026-04-20. Now points to `portal_with_box.html` (real composition with overlay badges) + `transitions.html` (state-machine demo).
- [x] 4.20.12 Extend §3.1 bezel states with overlay-badge treatments. Done 2026-04-20. Added a four-corner table: top-left = slot index, top-right = unmatched "?" badge (3.8.2), bottom-left reserved, bottom-right = ownership pip (4.18.17). Future badges claim a free corner rather than stack.
- [x] 4.20.13 Update §10 open question on egui cloud vortex. Done 2026-04-20. Replaces A/B-only framing with three paths: Path C (shipped, polar-mesh), Path A (deferred to 4.15a.7, WGSL port), Path B (rejected, frame atlas — loses continuous-knob control).

**Wrap-up.**

- [ ] 4.20.14 Re-run the design language audit after 4.20.1–13 land. Any remaining drift either folds into 4.17 or surfaces a new 4.20.x.

### 4.21 iOS inspector tool (`tools/ios-inspect/`)

A dev-only tool that lets Claude iterate on iOS-specific phone UI bugs against the iOS Simulator without the user in the loop. Stack: `xcrun simctl` to drive the simulator + Safari, `ios-webkit-debug-proxy` (Homebrew) to expose Safari's Web Inspector protocol as a local WebSocket, and a small CLI that speaks the protocol. Lives alongside the other `tools/` crates — not part of the workspace build, not in CI.

**Why this exists:** iOS-specific layout quirks (safe-area, Dynamic Island, address-bar behavior) don't reproduce under the chromedriver e2e harness, and require a physical-device loop today. Simulator + inspector protocol closes that loop autonomously. Direct next use: 4.18.1d (dark-band-at-bottom).

- [x] 4.21.1 **Spike — prove the stack end-to-end before designing the tool.** Done 2026-04-21. End-to-end stack works on iPhone 17 Pro / iOS 26.2 sim: `trunk serve` → `xcrun simctl openurl` → `ios_webkit_debug_proxy -s unix:…` → WS protocol → `Runtime.evaluate` + `DOM.querySelector` + `CSS.getComputedStyleForNode` + `Page.snapshotRect` all verified.
  - [x] 4.21.1a **Prereqs.** Full Xcode + iOS Simulator runtime installed (Chris confirmed 2026-04-21). Any iPhone-with-Dynamic-Island model works; pick one via `xcrun simctl list devices`.
  - [x] 4.21.1b **Proxy installed + verified.** `brew install ios-webkit-debug-proxy` done 2026-04-21; spike confirmed it only lists the simulator when run with `-s unix:<webinspectord_sim.socket>` (the socket lives at `/private/tmp/com.apple.launchd.*/com.apple.webinspectord_sim.socket` and can be found via `lsof -U | grep webinspect`). The `-s` flag was the gotcha — default invocation only scans USB-tethered devices and returns an empty list.
  - [x] 4.21.1c **Serve the phone SPA from the Mac.** Done. `cd phone && trunk serve --address 0.0.0.0 --port 8090` is enough; `xcrun simctl openurl booted http://<mac-ip>:8090/` reliably opens sim Safari to the SPA. Dev-data'd server not required — static trunk-served SPA reaches the ConnectionLost screen (no `/ws`), which is fine for layout probing.
  - [x] 4.21.1d **Protocol handshake.** Done via Python+websockets scratch script. Key finding: WebKit Web Inspector protocol requires wrapping per-page commands in `Target.sendMessageToTarget` (after receiving `Target.targetCreated` on connect) and unwrapping responses from `Target.dispatchMessageFromTarget`. Unlike Chrome's CDP, WebKit has no `DOM.enable`/`CSS.enable` (domains are always-on); calling them returns method-not-found. Runtime.evaluate returned `"Skylander Portal"` on first try.
  - [x] 4.21.1e **Screenshot.** Both routes verified. `xcrun simctl io booted screenshot /tmp/x.png` produces the full device frame including Safari chrome (1.3 MB, viewable). `Page.snapshotRect` returns a `data:image/png;base64,…` dataURL — works but the ~2.8 MB payload blows past the default 1 MB WS frame limit; the tool must set `max_message_size` on tokio-tungstenite (or equivalent). simctl is the default recommendation; inspector-side is for web-content-only grabs that exclude the sim chrome.
  - [x] 4.21.1f **Computed-style probe.** Done. `DOM.getDocument` → `DOM.querySelector` → `CSS.getComputedStyleForNode` round-trip works; probed `.app`, `body`, `html`. Also exercised a DOM-side resolver for `env(safe-area-inset-bottom)` by appending a probe element with `height: env(...)` and reading its bounding rect — returns a resolved px value. **Sim-fidelity finding**: iPhone 17 Pro iOS 26.2 sim reports `safe-area-inset-bottom = 0` in Safari (non-standalone), which differs from real hardware's ~34px for Dynamic Island models. 4.18.1d follow-up will need either a PWA-standalone repro (Add-to-Home-Screen in the sim) or a real-device fallback.
  - [x] 4.21.1g **Language decision for the tool: Rust.** Protocol is trivial JSON-over-WS; the Target.sendMessageToTarget wrapping is ~30 lines. `tokio-tungstenite` + `serde_json` already in Cargo.lock (e2e-tests); no new toolchain. The Python spike translates to Rust rote. Only caveat for the Rust impl: bump `WebSocketConfig::max_message_size` to ~16 MB so Page.snapshotRect payloads don't hit the default 1 MB frame cap.

- [x] 4.21.2 **Build the tool.** Done 2026-04-21. `tools/ios-inspect/` is a standalone cargo project (opt-out of root workspace via empty `[workspace]` table) with 8 subcommands verified end-to-end against iPhone 17 Pro / iOS 26.2 sim: `boot [--device]`, `open <url>`, `eval '<js>' [--raw]`, `computed-style <selector> [--filter]`, `dump-dom [--selector] [--depth]`, `screenshot -o <path> [--web-only]`, `tabs`, `shutdown`. Modules: `simulator.rs` (xcrun simctl wrappers + Dynamic-Island auto-pick), `proxy.rs` (ios_webkit_debug_proxy lifecycle + tab listing, `setsid`-detached), `protocol.rs` (WebKit Web Inspector WS client with 32 MB max_message_size, Target.sendMessageToTarget wrapping hidden behind a `Session` API), `state.rs` (persistent state at `/tmp/ios-inspect-state.json`). **Self-healing proxy** (per Chris's request to keep proxy-juggling out of the user's hands): every proxy-touching command calls `ensure_proxy_healthy()` which `lsof`'s for the live `webinspectord_sim.socket` and transparently respawns the proxy if the cached path has drifted — needed because the socket moves between `/private/tmp/com.apple.launchd.*/` and `/private/var/tmp/com.apple.launchd.*/` whenever webinspectord_sim restarts. Also dropped glob-based socket discovery for `lsof -U -c launchd_s` (stale sockets linger alongside live ones, breaking glob). README.md at `tools/ios-inspect/README.md` captures prereqs, session examples, and known limitations (sim safe-area fidelity gap, WebKit-vs-CDP protocol quirks, single-tab/single-sim).

- [x] 4.21.3 **Document in CLAUDE.md.** Done 2026-04-21. Added "Claude-driven iOS repro" bullet to the Aesthetic section (alongside the existing mock-on-iPhone + safe-area-pattern bullets): one-shot bringup command, ios-inspect vs chromedriver-e2e routing rule (layout/CSS probes → ios-inspect, functional regressions → e2e), reminder of the sim fidelity gap, pointer to `tools/ios-inspect/README.md` for full docs. Cross-link in 4.18.1d entry added pointing to PLAN 4.21 / 4.21.4.

- [x] 4.21.4 **Re-attempt 4.18.1d with the tool in hand.** Done 2026-04-21. **Root cause identified — color-seam between fixed pseudo and html's flat bg.** Probe findings: (1) `html { background: var(--sf-3) }` = flat `#020818` (dark navy). (2) `body { background-color: rgba(0,0,0,0) }` (transparent). (3) `body::before { position: fixed; inset: 0; … }` paints the full gradient stack but fills only the **layout viewport** (714px in sim). (4) The gradient's bottom radial highlight peaks at `#0e2464` (a much lighter mid-navy) at center-bottom — the linear-gradient only reaches `#020818` at exactly y=100%, and the radial highlight brightens that center strip further. **Mechanism**: on iOS Safari, URL-bar retraction grows the **visual** viewport taller than the layout viewport by ~34–52px; the strip below the layout viewport is uncovered by `body::before`/`body::after` (both `position: fixed`) and shows `html`'s flat `#020818`. The color mismatch between body::before's `#0e2464`-ish bottom-center and html's `#020818` is the visible dark band. Sim doesn't reproduce the bug (fixed Safari chrome, no retract animation, `env(safe-area-inset-bottom) = 0`) — but **forcibly shrinking `body::before { bottom: 50px }` in the sim reproduces the exact dark-band pattern** via web-only screenshot, confirming the mechanism. Candidate (a) ruled out (no iOS overlay involved). Candidate (b) ruled out (`.app` padding-bottom is 16px even with 0 inset — healthy, not the culprit). Candidate (c) was closest — it's a viewport-region exposure issue, not a compositor cache bug per se. **Recommended fix**: move the full gradient stack from `body::before` onto `html` directly (no fixed-positioning needed — `html` paints its background across the full canvas, which always includes the URL-bar-retraction region). Filed as new task under 4.18.1d (4.18.1e).

**Scope discipline:** if 4.21.1 surfaces protocol bugs or Xcode/proxy incompatibility the brew install didn't catch, bail on the whole approach and revisit real-device tethering instead of forcing the tool through. The spike decides whether 4.21.2 happens.

---

## Phase 5 — Kaos

- [ ] 5.1 Wall-clock timer: 20min warmup + randomized 60min windows.
- [ ] 5.2 Text-only overlay with Kaos catchphrases (curated in-repo list; text avoids audio copyright). Two surfaces mocked in Phase 4: `kaos_takeover.html` (stolen-seat + KICK BACK) and `kaos_swap.html` (mid-game swap + BACK TO THE BATTLE). **No auto-dismiss** — phone is typically asleep during gameplay. Multiple fires while asleep: latest-wins or queue (decide during impl).
- [ ] 5.3 1-for-1 swap of a portal figure with a random compatible-with-current-game figure.
- [ ] 5.4 Purple/pink Kaos skin via CSS variable swap (rides on Phase 4's `--*` tokens; should be a palette swap, not a rewrite).
- [ ] 5.5 Parent kill-switch (SPEC Q38) — hidden config knob, not in the phone UI.
- [ ] 5.6 Kaos swap goes through the standard driver flow (so tests catch regressions).

Kaos is LAST among feature work. Do not start without explicit go-ahead.

---

## Phase 6 — Post-Kaos polish

- [~] 6.2 **Parse `.sky` firmware for per-figure stats** — read-only. `crates/sky-parser/` parses tag layout per `docs/research/sky-format/SkylanderFormat.md` (additional crypto logic detailed here: https://marijnkneppers.dev/posts/reverse-engineering-skylanders-toys-to-life-mechanics/ ). `GET /api/profiles/:profile_id/figures/:figure_id/stats` feature-gated on `sky-stats`. 22 tests (header, variant decomposition, web code, XP/level, gold, nickname, hero points, playtime, hat history, trinket, timestamps, quest raw u72s, CRC16) — all against **synthetic plaintext fixtures**, which 6.2.0 has now shown don't match production reality. **Status (2026-04-22)**: identity fields (figure_id, variant, serial — block-0 manufacturer data, always plaintext) decode correctly on real RPCS3 dumps; payload fields (XP, gold, nickname, playtime, etc.) are *encrypted* in real dumps and currently decode as garbage. Stats endpoint can expose identity + variant info today; level / gold / nickname / etc. gated on decryption landing in 6.2.0b. Pivot: reopening this parent with `[~]` until 6.2.0b lands and all 22 tests have a ciphertext-fixture counterpart.
  - A web based tool for generating skylanders can be found here: https://github.com/mandar1jn/portfolio/blob/master/src/pages/skylanders/generator.astro (MIT license)
  - Blog post above remains the reference for the common/default-figure layer (Key A CRC48-ECMA-182, CRC-16/CCITT-FALSE, MD5→AES-128-ECB block keys). Its Appendices A–D are MIT-licensed sample code — Appendix B (AES+MD5 decrypt) is the direct input to 6.2.0b. Blog does **not** cover Trap / Vehicle / Racing / CYOS internals — those come from our own format doc.
- [x] 6.2.0 **Validate the "RPCS3 emits plaintext `.sky`" assumption against real samples.** Done 2026-04-22. Chris provided 127 samples in `./dev-data/validation-figures/` (gitignored) — Standard figures across SSA→Imaginators, Trap Masters subdir, traps subdir (element traps), sensais, and a creation-crystal dir. Validator at `crates/sky-parser/examples/validate_samples.rs` walks any dir and reports per-file parse + CRC16 verdict. Run with `cargo run --example validate_samples -p skylander-sky-parser -- ./dev-data/validation-figures`. **Result: encryption present.** All 151 `.sky` files parsed without structural errors; 0/151 had valid CRC16s; payload fields decoded as random bytes (every figure shows level 20, XP values in the billions, UTF-16 nicknames as Unicode garbage, playtimes 80–130 years). Identity bytes (figure_id + variant) decoded correctly everywhere — consistent with Mifare Classic's plaintext block-0 manufacturer data + AES-encrypted data blocks. 22 existing tests passed only because synthetic fixtures happened to be plaintext. **Follow-up: 6.2.0b implements AES-128-ECB decryption with MD5-derived block keys per blog Appendix B; 6.2.1–6.2.6 gate on it.**
- [x] 6.2.0b **Implement AES-128-ECB decryption layer with MD5-derived per-block keys.** Done 2026-04-22. Added `aes` + `md-5` deps to `crates/sky-parser/Cargo.toml`. New module-level `decrypt_figure` / `encrypt_figure` / `block_key` / `hash_in_template` + `is_plaintext_block` (matches blog Appendix D exactly: skip blocks 0–7 and every sector trailer `(i+1) % 4 == 0`, skip all-zero blocks, AES-128-ECB with key = MD5(block_0 || block_1 || block_index || " Copyright (C) 2010 Activision. All Rights Reserved. ")). `parse()` now decrypts unconditionally. `Fixture::build()` encrypts its synthetic plaintext before returning so all 22 existing tests run the full decrypt→parse path as a roundtrip. Validator (`cargo run --example validate_samples -p skylander-sky-parser -- ./dev-data/validation-figures`) went from **0/151 → 141/151 CRC valid**; payload fields decode as sensible real-world values (Tree Rex level 10, maxed XP, 18.6h playtime; Chop Chop level 5, 15 min play; Lightning Rod placed once for 8 seconds). Note: the 0x35-byte copyright constant has both leading and trailing spaces — the blog's prose omits the trailing one but its hex dump shows it (last byte 0x20). Remaining 10 CRC failures are Imaginators Senseis / Creation-Crystal-era figures (Crash Bandicoot, King Pen, Air Strike, Dr. Neo Cortex, Wild Storm, Sheep Creep) plus Trap Team Adventure Packs (Midnight Museum, Mirror of Mystery, Nightmare Express, Sunscraper Spire) — these aren't decryption problems, they're layout/kind classification gaps owned by 6.2.2.
- [ ] 6.2.1 **UI determination pass for Trap / Vehicle / CYOS.** Mock reduced Figure Detail variants in `docs/aesthetic/mocks/` before any parser work — fields we don't render aren't worth decoding. Targets: Trap → captured villain name + portrait as the headline row; Vehicle → SSCR level as the headline cell (mirrors Standard's LEVEL cell), adornment names below; CYOS → battle class + element + nickname with missing-field tolerance (dimmed placeholders for fields that failed partial-parse per 2026-04-21 scope call). Racing Pack gets no mock — kind continues to show the default "STATS COMING SOON" strip.
- [x] 6.2.2 **Split `FigureKind::Other` into `Trap` / `Vehicle` / `RacingPack` / `Cyos`.** Done 2026-04-22. Extended `FigureKind` in `crates/sky-parser/src/lib.rs` with `Trap`, `Vehicle`, `RacingPack`, `Cyos` alongside `Standard` / `Other`. **Community-sourced ranges didn't match reality** — previous table had Traps at 0x1F4..=0x225, Vehicles at 0x2BC..=0x2ED, CYOS at 0x320..=0x383; all 151 real samples classified as `Standard` under those. Validator walk of real RPCS3 dumps shows trap tags at `0x0D2..=0x0DC` (11 element traps + Kaos trap at 0xDC); captured-villain identity lives in the encrypted payload per spec, not the figure_id. New range-table entry `0x0D2..=0x0DC => Trap`; Vehicle + CYOS ranges left as inline-documented TODOs pointing at 6.2.9. Test renamed `other_figure_kind_skips_standard_payload` → `trap_kind_skips_standard_payload` with boundary fids 0xD2 / 0xD7 / 0xDC. 22 tests pass; validator now reports 60 Trap + 91 Standard across the sample set (141/151 CRC-valid — unchanged, decryption layer unaffected by enum split).
- [ ] 6.2.3 **Trap payload: captured villain identity.** Parse the villain cache (6 slots, villain ID + flags per slot) per `docs/research/sky-format/SkylanderFormat.md` line 39ff. Extend `SkyFigureStats` with `trap: Option<TrapData>` carrying the active-slot villain ID. New server-side lookup table (`data/villains.json`) maps villain ID → display name + portrait filename; artwork sourced from firmware pack first, Fandom fallback reusing 3.19.6's attribution pipeline. Phone only sees display name + portrait URL, never raw IDs. ~3 tests: known-trap fixture, empty cache, CRC16 survival.
- [ ] 6.2.4 **Vehicle payload: SSCR level.** Parse SSCR XP + level derivation per spec §"Vehicle" (line 672 area). Extend `SkyFigureStats` with `vehicle: Option<VehicleData>` carrying level (headline) and optionally decoration / topper / neon / shout IDs. New `data/vehicle_adornments.json` maps IDs → display names; same sourcing path as 6.2.3's villain table. Skip gearbits / vehicle-flags / mod-flags this pass — low UI value for the extra parsing surface. ~3 tests: fixture + level boundary + missing-adornment slots.
- [ ] 6.2.5 **CYOS payload: class / element / nickname best-effort.** Parse the deterministic parts of the spec (battle class at known offset, element byte); attempt nickname recovery from the 0x65-byte CYOS data payload. Return `Option<CyosData>` with *individually-optional fields* so the phone hides missing rows instead of failing the whole parse. Spec flags this layout as "may be incorrect, actively being worked on" — emit a structured warning log on CRC-pass-but-field-mismatch so real user files inform future spec corrections.
- [ ] 6.2.7 **Parse-driven figure discovery (identity-only, no decryption required).** Today `crates/indexer/` walks the firmware pack's folder structure and derives figure identity from path segments (game / element / name). Brittle against real-world dumps — `./dev-data/validation-figures/` has filenames like `Bad juju(villian.sky`, `grave clobber (villan.sky`, `Wolfgang (vilian).sky`, and a `traps/` subdir that per our range-table classifies contents as `Standard` (likely captured-villain figure_id). Parse-driven discovery: read block 0 of every `.sky` file in the pack, use `(figure_id, variant)` as the stable key, join against `data/figures.json` for canonical name + element + game. Filenames become cosmetic; parent directories irrelevant. Independent of 6.2.0b — block 0 is already plaintext. Risk to watch: trap crystals apparently store a captured-villain figure_id rather than a distinct "trap figure" ID (see validator output for `traps/Air Trap.sky` at fid=0x0000D4), so identity lookup alone won't distinguish "Air Trap" from "level-1 captured villain with the same ID" — `figures.json` needs a trap-villain section or the indexer needs to consider additional bytes. Pin the disambiguation rule in a test once 6.2.0b's decryption lets us look at the full data. This item's scope: the indexer refactor + `figures.json` schema bump; disambiguation lands alongside or after 6.2.3 (Trap parser).
- [ ] 6.2.6 **Wire per-kind payloads through `/api/profiles/:profile_id/figures/:figure_id/stats` + `phone/src/screens/figure_detail.rs`.** Extend the JSON response with nested `trap` / `vehicle` / `cyos` objects (null for kinds without a payload; Racing Pack is always null). `FigureDetail` branches on kind to render the variant from 6.2.1's mocks. Covers the non-Standard half of 6.3; Standard-stats UI (replacing the "STATS COMING SOON" strip with the three-cell LEVEL/GOLD/PLAYED row) stays 6.3's responsibility. Uses the existing `sky-stats` feature flag — no new flag.
- [ ] 6.2.8 **Investigate the 10 CRC-failing samples from 6.2.0b.** Each parses without structural error but the decrypted CRC16 doesn't match — candidates for in-game inspection to confirm whether the figures themselves are damaged/never-played, whether they're a distinct encoding layer, or whether our payload offsets are wrong. Chris to load each in the emulator and compare observed state vs parser output. **Failing filenames (from `./dev-data/validation-figures/`)**:
  - Trap Team Adventure Packs (figure_ids 0x131–0x134): `Mirror of Mystery` (0x131), `Nightmare Express` (0x132), `Sunscraper Spire` (0x133), `Midnight Museum` (0x134).
  - Imaginators Senseis / Creation-Crystal-era: `King Pen` (0x259), `Wild Storm` (0x274), `Crash Bandicoot` (0x276), `Dr. Neo Cortex` (0x277), `Air Strike` (0x25F), `Sheep Creep` (0xC82 — fid out of expected Sensei range, flagging for closer look).
  - Plausible hypotheses without touching code: (a) Adventure Packs use a different CRC scope or a second independent area; (b) Senseis have an extended data layout post-Imaginators that shifts the CRC footprint; (c) these specific figures have never been placed on a real portal so the state areas are factory-blank with no CRC yet — would be indistinguishable from layout mismatch via parser alone. In-emulator notes resolve the ambiguity.
- [ ] 6.2.9 **Pin Vehicle + CYOS `figure_id` ranges against real dumps.** 6.2.2 left these ranges commented out because the community-sourced values (Vehicles 0x2BC..=0x2ED, CYOS 0x320..=0x383) missed every real sample. Observed vehicle-looking figure_ids cluster near 0x0C9x..=0x0CAx but overlap with SuperChargers characters, so we need larger SuperChargers + CYOS sample sets before we can draw a safe range. Blocks 6.2.4 / 6.2.5 (can't write a Vehicle or CYOS parser that's never reached). Deliverables: (a) Chris drops SuperChargers vehicle dumps + at least one CYOS dump into `./dev-data/validation-figures/`; (b) extend `FigureKind` range table with the new entries + a test per variant using real observed fids; (c) revise 6.2.7's identity join logic if `(figure_id, variant)` alone doesn't disambiguate vehicles from same-ID characters.
- [ ] 6.3 **Detailed-stats screen on the phone.** Level + XP, gold, current hat, playtime, nickname, hero points, hat history, trinket, quest progress. Hits `/api/profiles/:profile_id/figures/:figure_id/stats`. Read-only (no editing per 6.2). Non-standard layouts render a reduced panel until 6.2's stubs fill. Lands after Phase 4 so the layout inherits the design system. **Current placeholder** (2026-04-21): `FigureDetail` renders a single muted "STATS COMING SOON" strip (`.detail-stats-soon` in `phone/assets/app.css`) in the slot where the three-cell `LEVEL / GOLD / PLAYED` row used to sit with hardcoded `--` values. When this item ships, delete `.detail-stats-soon*` + the soon-label span and reinstate the three `.detail-stat-cell` blocks, wired to the fetched stats. Scoped to post-Kaos because sky-parser stabilisation is more useful after Kaos has exercised the portal flow end-to-end.
- [ ] 6.4 **Demo harness for screen recording.** Browser-viewable test session that drives the phone SPA through a representative flow (profile pick → PIN → game launch → portal → toy box → place → Kaos swap). Runs side-by-side with a remote-desktop view of the HTPC so the integrated experience can be screen-recorded in one frame.
- [ ] 6.1 **Suppress RPCS3 window flicker during menu navigation.** Our launcher starts before RPCS3 → establish Z-order priority. Ideas: (a) launcher `WS_EX_TOPMOST` during `open_dialog` nav so Qt popups render behind, (b) `SetWinEventHook` / `EVENT_OBJECT_SHOW` filtered to RPCS3 PID to intercept dialog creation and move off-screen before first paint, (c) hook menu popups the same way.
- [ ] 6.5 Support dumping skylanders from a scan using a acs122u reader. I have them lying around and that would greatly help discourage piracy since we would be encouraging scanning over just looking at a folder of dumps.

---

## Phase 7 — Packaging + release

Deliberately separated from Phase 3 so it's clear this only runs once the app works end-to-end. CI deferred until here per the original "no CI until features work" stance.

- [ ] 7.1 **Single-exe distribution.** Everything ships as ONE `skylander-portal.exe`. Phone SPA + images + `figures.json` + fonts (WOFF2) + Kaos SVG embedded via `include_dir!` or `rust-embed`. Release builds strip debug symbols + `cargo build --release` + UPX if binary exceeds ~50 MB.
- [ ] 7.2 GitHub Actions workflow on version-tag push: Windows release build + fast test suite (unit + integration + workspace build, NOT `#[ignore]`-gated e2e tests), attach zip to release.
- [ ] 7.3 Release `README.md` — user-supplied bits (RPCS3 install path, firmware backup pack). Walk through first-launch wizard. Link to `data/LICENSE.md` for Fandom attribution (3.19.6).
- [ ] 7.4 Verify release zip on a *different* Windows machine than the dev one.
- [ ] 7.5 Post-release monitoring plan (GitHub issues only for v1).
- [ ] 7.6 **Trademark / IP review of shipped assets.** Kaos sigil (`docs/aesthetic/kaos_icon.svg` → bundled as `phone/assets/kaos.svg`) and any box-art thumbnails (4.2.5). Decide fair-use vs derivative vs custom-drawn before public release.

---

## Non-goals

- No bundling of RPCS3 or `.sky` files (piracy concern).
- No CI until core features work.
- No Linux/Mac support.
- No user-entered figure names.
- No audio (text-only Kaos to dodge copyright).
- No live wiki scraping at runtime — data is committed to the repo.

## Risks (live list — update as we learn)

- **R1:** UI Automation may not expose enough of the RPCS3 Qt dialog to drive it reliably. Resolved: phase 1a was the first spike; Alt-keyboard-nav workaround validated (CLAUDE.md "RPCS3 window/menu gotchas").
- **R2:** "Move portal dialog off-screen" may be blocked by Windows or cause focus loss from the game. Resolved: Win32 `SetWindowPos` works; `hide_dialog_offscreen` + RAII restore guard in `crates/rpcs3-control/src/hide.rs`.
- **R3:** Wiki search hit rate might be below 80%. Resolved: 504/504 coverage achieved (3.19.5); manual curation file layered over.
- **R4:** Leptos touch/mobile UX may prove rough. Mitigation: ongoing through Phase 4.18 on-device iteration; PWA install path as fallback when Safari chrome won't hide.
