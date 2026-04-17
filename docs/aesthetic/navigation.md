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

### 3.1 States

```
[Startup]              starry sky, calm, no clouds
    │ RPCS3 process spawned
    ▼
[Booting]              clouds SPIRAL IN from edges toward center
    │ RPCS3 menu loaded (UIA detects main window)
    ▼
[Awaiting Connect]     QR spins in at center over the swirling clouds
    │                  "SCAN TO CONNECT" below QR
    │                  clouds still swirling around QR
    │
    ├─ phone connects → player avatar orbits the QR
    │                   (up to 2 avatars orbiting)
    │
    ├─ 2nd phone joins → 2 avatars orbiting
    │
    ├─ max reached ───→ QR FLIPS (card-rotate) to show
    │                   "MAXIMUM PLAYERS REACHED" on the back
    │                   (flip back when a slot opens)
    │
    ▼ game selected (any phone picks a game)
[In-Game]              clouds SPIRAL OUT (expand outward + fade)
                       launcher window becomes TRANSPARENT
                       → RPCS3 game viewport shows through
                       small reconnect QR available as a
                       semi-transparent overlay if needed

    │ game quit (phone picks another game)
    ▼
[Switching Game]       clouds SPIRAL IN again (cover the old game)
    │                  "SWITCHING GAMES..." heading
    │                  same as Booting but with game-change context
    │ RPCS3 loads new game
    ▼
[Compiling Shaders]    (if detected — see §3.6)
    │                  clouds still swirling
    │                  "COMPILING SHADERS..." + progress if available
    │                  keeps the game hidden until shaders are done
    │                  so the first frame the user sees is clean
    │ shaders done / game viewport stable
    ▼
[In-Game]              clouds spiral out → transparent

    │ RPCS3 crashes / game exits unexpectedly
    ▼
[Crashed]              clouds spiral in quickly (~1s, urgent feel)
                       "SOMETHING WENT WRONG" heading
                       "RPCS3 exited unexpectedly" body
                       "RESTART" gold button → back to [Startup]
                       auto-detects if RPCS3 process is gone

    │ clean quit (phone shuts down via menu)
    ▼
[Shutdown]             clouds spiral in gently
                       "SEE YOU NEXT TIME, PORTAL MASTER"
                       launcher exits after 3s
```

### 3.2 Cloud choreography

All clouds are **procedurally generated** — no pre-rendered frames or game captures (copyright avoidance). The approach:

| Technique | Pros | Cons |
|-----------|------|------|
| **Perlin/Simplex noise texture** (generated once at app start, rotated/animated in Painter) | Organic look, efficient, no shipped assets | Needs a noise implementation in Rust |
| **SVG feTurbulence** (for HTML mocks) | Organic, built into browsers, zero assets | Not available in egui — mock-only |
| **egui Painter arcs/meshes** (the CSS spike approach) | Pure code, no texture pipeline | Looks stylized, not organic |
| **Runtime-generated texture atlas** | Best of both — generate Perlin noise frames at startup, cache as textures, animate by cycling | Startup cost (~200ms for 60 frames at 960×540), but then smooth playback |

**Recommended for production:** Runtime-generated texture atlas. At startup, the egui app generates 60–90 frames of Perlin noise cloud animation into an in-memory texture atlas. Each frame is a 960×540 RGBA buffer with:
- Base: radial Perlin noise with turbulence octaves (gives the organic cloud shape)
- Tint: blue-white color ramp matching the starfield palette
- Alpha: radial falloff from center (clouds dense at center, transparent at edges)
- Per-frame: rotate the noise sampling coordinates slightly (gives the swirl)

Three animation modes share the same atlas but play it differently:
- **Spiral-in:** frames play forward + scale from 2× → 1× (clouds converge)
- **Idle swirl:** frames loop continuously at 1× scale
- **Spiral-out:** frames play forward + scale from 1× → 2× + fade opacity (clouds expand and vanish)

### 3.3 QR + player orbit

- QR code renders inside a gold bezel (same `GoldBezel` material as phone, but large: ~280px equivalent on a 1080p TV)
- **No URL text shown** — just the QR + "SCAN TO CONNECT" in Titan One
- When phones connect, a small **player indicator** (gold-bezeled circle with the profile's color + initial) orbits the QR in a slow elliptical path
- Up to 2 orbiting indicators; each offset by 180° so they balance visually
- **Max-reached flip:** the QR bezel does a CSS-style Y-axis `rotateY(180deg)` card flip. The "back" shows a framed panel with "MAXIMUM PLAYERS REACHED" in Titan One gold. Flips back when a slot opens.

### 3.4 In-Game transparency

Once a game is selected:
1. Clouds spiral out (2s animation)
2. Launcher window fades to transparent (`eframe::Frame::set_transparent(true)` + clear to alpha 0)
3. The RPCS3 game viewport is now visible through the launcher window
4. A **small reconnect QR** hovers in the bottom-right corner as a semi-transparent overlay — always available but unobtrusive
5. If the QR needs attention (phone disconnects), it brightens + pulses briefly

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
