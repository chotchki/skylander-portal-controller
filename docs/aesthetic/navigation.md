# Navigation Map & Modal Stack Semantics

Covers PLAN 4.3.4 (modal stack) and 4.3.5 (navigation map). Authoritative for the Leptos routing + overlay logic.

---

## 1. Screen graph

```
                 ┌──────────────┐
            ┌───▸│ KonamiGate   │
            │    └──────┬───────┘
            │        correct code
            │    ┌──────▾───────┐
            │    │ProfileManage  │──▸ EditProfile
            │    │              │──▸ PinReset
            │    └──────┬───────┘
            │       ← LOCK
┌─────────┐ │  ┌─────────┐    ┌──────────┐    ┌──────────┐
│ (entry) │─┴─▸│Profile  │───▸│  PIN     │───▸│  Game    │
│ QR scan │    │Picker   │    │  Entry   │    │  Picker  │
└─────────┘    │         │    └──────────┘    └────┬─────┘
               │ manage  │                         │
               │ button ─┘                    select game
               └────┬────┘                         │
                    ▴                               ▾
               SWITCH PROFILE              ┌────────────────────────────────┐
                    │                      │           PORTAL               │
                    │                      │  ┌──────────────────────────┐  │
                    │                      │  │ 2×4 slot grid            │  │
                    │                      │  │  tap empty → ToyBox      │  │
                    │                      │  │  tap loaded → REMOVE (5s)│  │
                    │                      │  └──────────────────────────┘  │
                    │                      │  ┌──────────────────────────┐  │
                    │                      │  │ ToyBox (lid open)        │  │
                    │                      │  │  tap fig → FigureDetail  │  │
                    │                      │  │  PLACE → impact, close   │  │
                    │                      │  │  BACK → box grid         │  │
                    │                      │  └──────────────────────────┘  │
                    │                      │                                │
                    │                      │  kebab ──┐                     │
                    │                      └──────────┼─────────────────────┘
                    │                                 ▾
                    │                      ┌─ ─ ─ ─ ─ ─ ─ ─ ─ ─┐
                    ├─────────────────────── MenuOverlay (scrim)
                    │  SWITCH GAMES        │  • profile chip      │
                    │  (hold)                • show-join QR
                    │     │                │  • SWITCH PROFILE     │
                    │     │                  • SWITCH GAMES (hold)
                    │     │                │  • SHUT DOWN (hold)   │
                    │     ▾                └─ ─ ─ ─ ─ ─ ─ ─ ─ ─┘
                    │  GamePicker                     │
                    │                            SHUT DOWN
                    │                            (hold)
                    │                                 ▾
                    │                            app closes

                 ════════════════════════════════════
                 OVERLAYS THAT CAN APPEAR OVER ANY
                 ACTIVE SCREEN (server-pushed events):
                 ════════════════════════════════════

                 WS disconnect ──▸ ConnectionLost overlay
                 WS TakenOver  ──▸ KaosTakeover (full replacement)
                 WS KaosSwap   ──▸ KaosSwap overlay (portal peeks through)
                 WS ResumePrompt ──▸ ResumePrompt modal (on unlock only)
```

### Navigation edges

| From | Trigger | To | Animation direction |
|------|---------|----|--------------------|
| QR scan / URL | page load | ProfilePicker | fade-in |
| ProfilePicker | tap profile | PinEntry | slide up |
| PinEntry | correct PIN | GamePicker (+ ResumePrompt if layout exists) | slide up |
| ProfilePicker | manage button | KonamiGate | slide right |
| KonamiGate | correct code | ProfileManage | gold flash → crossfade |
| ProfileManage | EDIT | EditProfile | slide right |
| ProfileManage | PIN | PinReset | slide right |
| ProfileManage | LOCK | KonamiGate → ProfilePicker | slide left |
| GamePicker | select game | Portal | slide up |
| Portal | kebab tap | MenuOverlay | scrim fade-in + panel slide |
| Portal | tap empty slot | ToyBox opens (lid slides up) | drawer slide |
| ToyBox | tap figure | FigureDetail | lift + panel fade-in |
| FigureDetail | PLACE | Portal (impact animation, lid closes) | reverse lift + drawer close |
| FigureDetail | BACK | ToyBox grid | reverse lift |
| MenuOverlay | SWITCH PROFILE | ProfilePicker | slide down |
| MenuOverlay | SWITCH GAMES (hold) | GamePicker | slide down (all phones, triggered by server event) |
| MenuOverlay | SHUT DOWN (hold) | app closes | lights-out animation (all phones) |
| Any | WS disconnect | ConnectionLost overlay | fade-in over current |
| ConnectionLost | reconnect | previous screen restored | fade-out overlay |
| Any | WS TakenOver | KaosTakeover (replaces everything) | Kaos void wash |
| KaosTakeover | KICK BACK | page reload → ProfilePicker | full reload |
| Portal | WS KaosSwap | KaosSwap overlay | Kaos wash + portal dims |
| KaosSwap | BACK TO BATTLE | Portal | overlay fade-out |
| PinEntry (on unlock) | WS ResumePrompt | ResumePrompt modal | scrim + panel spring |
| ResumePrompt | RESUME | Portal (auto-loads saved layout) | panel fade-out → slot impacts |
| ResumePrompt | START FRESH | Portal (empty) | panel fade-out |

### Direction convention

- **Deeper** in the flow (profile → PIN → game → portal): **slide up** (ascending into the adventure).
- **Back / escape** (portal → game picker, switch profile): **slide down**.
- **Lateral / admin** (PIN → Konami gate, manage → edit/pin-reset): **slide right** (going, left coming back).
- **Modals / overlays** (menu, resume, confirms, connection lost): **scrim fade + panel spring** from below. No directional slide — they sit on top of the current context.
- **Kaos events**: their own void/wash animation — not part of the normal directional system.

---

## 2. Modal stack semantics

### Taxonomy

Every overlay in the app falls into one of three categories:

| Category | Examples | Behavior |
|----------|----------|----------|
| **Full-screen replacement** | KaosTakeover, ProfilePicker, PinEntry, GamePicker, ProfileCreate, KonamiGate, ProfileManage | Replaces the entire viewport. No underlying screen is visible or interactive. Not a "modal" in the overlay sense — it's a route change. |
| **Scrim modal** | MenuOverlay, ResumePrompt, ResetConfirm, ConnectionLost | Dims the underlying screen with a scrim. The portal/box is visible but not interactive (pointer-events: none behind the scrim). Exactly one at a time — opening a new one replaces the current. |
| **Inline overlay** | FigureDetail (lifted panel), REMOVE slot bar, KaosSwap (portal peeks through), ToyBox (drawer over portal) | The underlying screen is partially visible and contextually relevant. Interaction with the underlay is blocked but the visual context matters. These are NOT modals in the traditional sense — they're state transitions within the portal screen. |

### Rules

1. **No modal stacking.** At most one scrim modal is active at a time. If a second scrim event fires while one is open (e.g., WS disconnect while menu is open), the new one **replaces** the current, not stacks on top. The system priority resolves which wins:

   **Priority (highest first):**
   1. `ConnectionLost` — always wins, always on top, auto-dismisses on reconnect.
   2. `KaosTakeover` — replaces everything (session is gone).
   3. `KaosSwap` — overlays the portal; if menu was open, menu closes first.
   4. `ResumePrompt` — only fires on profile unlock; no competing modal can be open at that moment.
   5. `ResetConfirm` — only opens from FigureDetail; FigureDetail must be active (not another modal).
   6. `MenuOverlay` — user-initiated; can be dismissed to make room.

2. **Inline overlays are not part of the modal stack.** FigureDetail, REMOVE bar, and ToyBox are portal-screen internal states. They coexist with the portal — they don't "stack" with scrim modals. If a scrim modal opens while FigureDetail is showing, FigureDetail stays underneath the scrim (dimmed like the rest of the portal).

3. **Dismiss behavior per modal:**

   | Modal | Dismiss trigger | Result |
   |-------|----------------|--------|
   | MenuOverlay | Tap scrim, ✕, or any action button | Closes overlay, returns to portal |
   | ResumePrompt | RESUME or START FRESH (explicit choice required) | Closes, proceeds to portal state |
   | ResetConfirm | KEEP (tap) or HOLD TO RESET (hold complete) | Closes, returns to FigureDetail |
   | ConnectionLost | WS reconnect succeeds (automatic) | Fades out, restores previous screen |
   | KaosTakeover | KICK BACK IN (tap) | Full page reload → ProfilePicker |
   | KaosSwap | BACK TO THE BATTLE (tap) | Overlay fades, portal restored |

4. **No auto-dismiss on Kaos events.** Phone screen is typically off during gameplay. Both KaosTakeover and KaosSwap persist until explicit tap. If multiple Kaos swaps fire while the phone is sleeping, latest-fires-wins — the last swap's figures are what the user sees on wake.

5. **Reconnect survival:**

   | Modal | Survives reconnect? | Rationale |
   |-------|--------------------|-----------| 
   | ResumePrompt | Yes — server re-offers on reconnect if layout still exists | The offer is tied to the profile's saved layout, not the WS session |
   | KaosSwap | Yes — persists until dismissed | User needs to see what happened |
   | MenuOverlay | No — dismiss on reconnect | Server state may have changed (game switched, other player's actions) |
   | ResetConfirm | No — dismiss on reconnect | Figure state may have been modified externally |
   | FigureDetail | No — dismiss on reconnect | Portal state may have changed; stale detail is misleading |
   | REMOVE bar | No — dismiss on reconnect | Slot may have already been cleared |

6. **Back-button / swipe-back behavior (browser navigation):**
   - Scrim modals: browser-back dismisses the modal (acts like tapping the scrim).
   - Full-screen replacements: browser-back goes to the previous route (GamePicker → PinEntry, etc.). Standard Leptos router history.
   - Inline overlays (FigureDetail, ToyBox): browser-back reverses to the portal idle state.
   - Kaos screens: browser-back does nothing (can't dismiss a takeover with a swipe; must tap the button).

---

## 3. TV Launcher — egui state machine (PLAN 4.15)

The TV launcher is a separate egui window running on the PC, shown fullscreen on the 86" TV. It's NOT a web page — it's a native `eframe` app. But it follows the same design language (starfield, gold, Titan One) adapted for lean-back 10-foot viewing.

**Design source-of-truth:** `docs/aesthetic/mocks/tv_launcher_v3.html` — a WebGL/HTML mock that demonstrates every state, transition, and timing. The egui implementation reproduces what that mock shows.

### 3.1 States

```
[1: Startup]           starry sky, calm, no clouds (irisRadius=0)
    │ RPCS3 process spawned
    ▼
[2: Booting]           iris CLOSES (clouds spiral in to fill screen)
    │                  "LOADING" + game name + boot status line
    │                  Transition tuning: iris ramps over `irisDuration`
    │                  (default 2500ms easeOut), but rotation/inflow
    │                  SNAP to target in 200ms — slow-ramping them with
    │                  the iris reads as a second inward sweep once the
    │                  iris settles. Clouds aren't visible during the
    │                  200ms snap (irisRadius still ~0), so it's free.
    │ RPCS3 menu loaded (UIA detects main window)
    ▼
[3: Compiling Shaders] (only if detection §3.6 fires)
    │                  Iris stays closed. Gold progress ring sits in
    │                  the central hole (where QR would otherwise be).
    │                  "COMPILING SHADERS" + "preparing your adventure"
    │ shaders done
    ▼
[4: Awaiting Connect]  Iris stays closed. QR spins into the central hole.
    │                  "SCAN TO CONNECT" below QR. Clouds keep swirling.
    │
    ├─ phone joins ───▸ [5: Players Joined] — player pip orbits the QR
    │                   (up to 2 pips, offset 180°). Clouds + QR unchanged.
    │
    ├─ max reached ───▸ [6: Max Players] — QR FLIPS (Y-axis rotate) to
    │                   "PORTAL IS FULL". Title swaps. Flips back when
    │                   a slot opens.
    │
    ▼ game selected (any phone picks a game)
[7: In-Game]           Iris OPENS (clouds spiral out). Launcher window
                       fades to transparent → RPCS3 game viewport shows
                       through. Reconnect QR sits in the upper-right
                       corner, HIDDEN by default — only fades in when
                       every phone has disconnected (see §3.4).

    │ game quit (phone picks another game)
    ▼
[Switching Game]       Iris closes again (covers the old game). Same as
    │                  Booting visually, with "SWITCHING GAMES..." copy.
    │ RPCS3 loads new game
    ▼ (back through Compiling Shaders if detected)
[7: In-Game]           Iris opens → transparent

    │ RPCS3 crashes / game exits unexpectedly
    ▼
[Crashed]              Iris snaps closed (~1s, urgent). "SOMETHING WENT
                       WRONG" + "RESTART" gold button → back to Startup.
                       Auto-detects RPCS3 process exit.

    │ clean quit (phone shuts down via menu)
    ▼
[8: Shutdown Farewell] Iris closes (clouds spiral in gently).
                       "SEE YOU NEXT TIME, PORTAL MASTER" with breathe
                       pulse. ~2.2s read pause, then 1.6s ease-in fade
                       to black. "(launcher will exit)" hint surfaces
                       after the fade lands. Launcher exits ~3s total.
```

### 3.2 Cloud choreography — WebGL fragment shader

The v3 mock established **WebGL fragment shader** as the technique. All clouds are procedurally generated — no pre-rendered frames or game captures (copyright avoidance).

**Shader sketch** (full source in `tv_launcher_v3.html`):

- **Base noise:** 5-octave simplex FBM (Ashima Arts/Stefan Gustavson 3D simplex). Three samples per fragment at different scales, mixed for cloud bulk + detail.
- **Cylindrical sampling:** noise input is `(cos(spiral) * scale, sin(spiral) * scale, effectiveR * scale + t * 0.06)` where `spiral = theta + r * tightness + t * rotationSpeed`. Cylindrical coords (vs feeding `theta * scale` directly) eliminate the polar-seam artifact at theta=±π — the noise becomes continuous around the loop.
- **Iris arms:** `armPattern = smoothstep(0.05, 0.95, sin(spiral * 10) * 0.5 + 0.5)` — 10 arms, integer count so the pattern stays continuous at the seam.
- **Iris hole + edge:** `iris = smoothstep(irisRadius + edge, irisRadius - edge, r)` carves out the central hole (or fills it). At `irisRadius = 0` the alpha goes to zero everywhere → clouds invisible. At `irisRadius = 1.6` clouds extend past the screen corners (corner r ≈ 1.02).

**Three independent uniforms** drive the look — these are continuous knobs animated by JS, NOT discrete modes:

| Uniform | Range | Idle target | Role |
|---------|-------|-------------|------|
| `irisRadius` | 0.0 – 1.6 | depends on state | Iris open (0=clear) / closed (1.6=full coverage) |
| `rotationSpeed` | 0 – 0.3 rad/s | 0.08 | Arm spin rate (constant in cloud states) |
| `inflowSpeed` | 0 – 0.4 r/s | 0.15 | Radial cloud drift toward center (z-coord scroll) |

**Critical decoupling rule:** the arm spiral term uses **plain `r`**, not `r + inflow*t`. Compounding inflow into the spiral was nausea-inducing — it added `inflow * tightness ≈ 0.18 * 4 = 0.72 rad/s` on top of the 0.08 rotation, a ~9× speedup during iris transitions. Inflow only scrolls the noise z-coordinate now; the visible arms rotate at `rotationSpeed` only.

**Halo composition:** the central focal-glow that sits behind QR / progress ring / hero title uses `mix-blend-mode: screen` with a soft 6-stop radial gradient. This makes the halo *additively brighten* the clouds beneath it rather than reading as its own opaque disk. Without screen-blend, the rim of the halo was visible as a hard circle.

**Egui port path** (PLAN 4.15.5):
- **Path A (recommended):** ship the fragment shader via `egui_wgpu` custom paint callback. Reproduces the mock 1:1, GPU-cheap, gives continuous-knob control over the three uniforms.
- **Path B:** bake the shader into a texture atlas at startup (60–90 frames at 960×540) and cycle frames. Loses the continuous-knob control — animations have to be choreographed at bake time.

### 3.3 QR + player orbit

- QR code renders inside a gold bezel (same `GoldBezel` material as phone, but large: ~280px equivalent on a 1080p TV).
- **No URL text shown** — just the QR + "SCAN TO CONNECT" in Titan One.
- Focal-glow halo sits behind the QR (mix-blend-mode: screen, 760px diameter, breathing pulse 3.5s).
- When phones connect, a small **player indicator** (gold-bezeled circle with the profile's color + initial, 84px) orbits the QR on an ellipse (rx=560, ry=400). Pips sit at z-index 9 — *below* the title text — so they pass behind "SCAN TO CONNECT" rather than crossing it.
- Up to 2 orbiting pips; each offset by 180° so they balance visually.
- **Max-reached flip:** the QR bezel does a Y-axis `rotateY(180deg)` card flip. The "back" shows a framed panel with "PORTAL IS FULL" in Titan One gold. Title swaps to match. Flips back when a slot opens.

### 3.4 In-Game transparency

Once a game is selected:
1. Iris animates open over ~1.8s easeIn (clouds spiral out from the center, alpha fades to 0). Rotation/inflow ramp to 0 in parallel.
2. Launcher window becomes transparent (`eframe::Frame::set_transparent(true)` + clear to alpha 0). The RPCS3 game viewport is now visible through the launcher window.
3. **Reconnect QR placement:** **upper-right** corner (was bottom-right in early spec — moved to keep it out of HUD/subtitle territory at the bottom of most games).
4. **Reconnect QR visibility — NOT always-on.** The QR is HIDDEN by default. It only fades in when *every* phone has disconnected from the server. The intent: an "everyone left, anyone come back" cue, not a persistent overlay that competes with the game. When at least one phone is connected, the launcher stays fully out of the way.
5. Fade timing: 1.0s opacity transition. The mock delays the show-up by ~1.4s after iris-open so it doesn't fight the iris animation for attention; the production rule is the same — wait until clouds have mostly cleared before introducing a UI element.

### 3.5 Shutdown farewell

Triggered by clean quit from the phone menu (server emits a shutdown event + drops WS):

1. Iris closes (clouds spiral in gently). Same shader machinery as Booting; what differs is the next beat.
2. "SEE YOU NEXT TIME, PORTAL MASTER" displays on the cloud field, in TV-display-lg (64px) gold with the standard breathe pulse (2.4s opacity + scale ±2.5%).
3. ~2.2s read pause — long enough to register, short enough to not feel like the app froze.
4. Full-screen black overlay fades in over 1.6s ease-in.
5. After fade lands, a faint "( launcher will exit )" hint surfaces in Fraunces italic — secondary cue, low alpha.
6. Launcher process exits.

The shutdown sequence is **deliberately slower than Crashed** — Crashed snaps closed in ~1s with an urgent feel; Shutdown is the gentle "thanks for playing" ramp-down.

### 3.6 Shader compilation detection (research needed)

RPCS3 compiles shaders on first launch of a game (and after GPU driver updates). This causes stutter if the game viewport is visible during compilation. If we can detect this phase, the cloud vortex stays up until shaders are done — so the first frame the user sees is clean gameplay.

**Detection approaches to investigate (priority order):**

1. **RPCS3 log file** — RPCS3 writes shader compilation progress to its log (`RPCS3.log` next to the executable). Look for patterns like `"Compiling shader"`, `"SPU cache"`, `"PPU module"` lines. A burst of these lines = compilation in progress; silence for ~2s = done. Cheapest to implement (file watcher).

2. **Window title** — RPCS3 sometimes updates the game viewport window title with compilation progress (e.g., `"Compiling shaders... (42/189)"`). We already have UIA access to the window titles. Poll the viewport title for a `"Compiling"` substring.

3. **CPU heuristic** — shader compilation pegs the CPU. If RPCS3's process CPU usage is >80% for >5s after game boot, assume compilation. Fragile but requires no RPCS3-specific knowledge.

4. **Frame rate heuristic** — if the game viewport exists but FPS title shows <5fps for >5s, assume compilation stutter. We already parse the `"FPS:"` prefix from the viewport window title.

**Fallback if none work:** Use a fixed delay after game-boot detection (~15s) before spiraling the clouds out. Not ideal but guarantees the worst shader stutter is hidden.

#### Progress visualization (if log parsing works)

If we can extract `current / total` counts from the log, the TV launcher shows a **gold progress ring** at the center of the cloud vortex — the same conic-gradient sweep used by `GoldBezel`'s loading state, but large (200–240px). The ring fills proportionally as shaders compile.

```
        ╭──────────────╮
       ╱   ◠◡◠ clouds ◠◡◠  ╲
      │                      │
      │     ┌──────────┐     │
      │     │ 42 / 189 │     │    ← gold progress ring (partial fill)
      │     └──────────┘     │
      │                      │
       ╲   ◠◡◠ clouds ◠◡◠  ╱
        ╰──────────────╯
           COMPILING SHADERS
```

- Ring: conic-gradient from `var(--gi)` (unfilled) to `var(--gb)` (filled), sweeping clockwise proportional to `current / total`
- Count: `"42 / 189"` in Titan One 40px gold inside the ring
- Heading: `"COMPILING SHADERS"` below the ring in Titan One 64px gold
- Subtitle: `"preparing your adventure"` in Fraunces italic 32px warm-white
- The cloud vortex continues swirling behind the ring — the ring sits on top
- When compilation finishes: ring does a quick bright flash (like the portal-impact flash), then the normal Awaiting Connect flow takes over

This turns a frustrating wait into an anticipation-building moment — the ring charging up feels like the portal is powering on.

### 3.7 TV typography scale

Everything is sized for 86" at ~10 feet:

| Token | Size | Use |
|-------|------|-----|
| TV display hero | 96px | "LOADING...", state titles |
| TV display lg | 64px | "SCAN TO CONNECT" |
| TV display md | 40px | Game name, status |
| TV body | 32px | Status messages, connection info |
| TV caption | 24px | Timestamps, secondary info |

Minimum: 24px for anything readable. Nothing smaller.

---

## 3.8 Phone-side crash handling

When RPCS3 crashes, the **TV launcher** shows the crash recovery screen (§3.1). But the **phone** also needs to react — the portal is now dead, and any tap will fail silently.

**Server event:** `Event::GameCrashed { message: String }` — broadcast to all connected phones when the server detects the RPCS3 process exit unexpectedly (distinguishable from a clean quit by exit code or process-gone-while-game-still-expected).

**Phone behavior:**
- Full-screen overlay (same priority level as `ConnectionLost` — replaces whatever the user is looking at)
- NOT a toast — this is a session-breaking event, not a transient error
- Heading: "GAME CRASHED" in gold display treatment
- Body: "The emulator stopped unexpectedly. Hang tight — we're working on it." in primary warm-white
- Two options depending on server state:
  - If server is auto-restarting RPCS3: show a spinner + "Restarting..." (auto-dismiss when new `GameLaunched` event arrives)
  - If server can't restart: "RETURN TO GAMES" gold button → navigates to GamePicker
- Dismisses automatically when a new game launches (server sends `GameLaunched` event)

**Modal priority:** slots between `ConnectionLost` (highest) and `KaosTakeover`:
1. ConnectionLost
2. GameCrashed (new)
3. KaosTakeover
4. KaosSwap
5. ... (rest unchanged)

---

## 4. Future screen landing spots

Screens not yet implemented that need a place in the graph:

| Future screen | Entry point | Category |
|--------------|-------------|----------|
| **Stats drill-down** (6.3) | FigureDetail → stats action icon | Inline overlay (extends the FigureDetail panel or slides a sub-panel) |
| **Variant cycling** (3.14) | FigureDetail → appearance action icon | Inline UI within FigureDetail (carousel or swipe on the hero bezel) |
| **Reset-to-fresh flow** (3.11.3) | FigureDetail → reset action icon | Opens ResetConfirm scrim modal |
| **Ownership badge** (3.10.7) | Portal slot — always visible on occupied slots | Portal-screen decoration, not a navigation target |
| **Show-join-code** (3.10.8) | MenuOverlay — QR is inline, not a separate screen | Already in MenuOverlay mock |
| **Crystal extra-confirm** (3.11.4) | ResetConfirm variant for Imaginators creation crystals | Same modal, stronger warning copy |
