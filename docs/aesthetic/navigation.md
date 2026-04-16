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
┌───────────┐    ┌────▾────┐    ┌──────────┐    ┌──────────┐
│  (entry)  │───▸│Profile  │───▸│  PIN     │───▸│  Game    │
│  QR scan  │    │Picker   │    │  Entry   │    │  Picker  │
└───────────┘    └─────────┘    └──────────┘    └────┬─────┘
                      ▴               │              │
                      │          manage btn      select game
                 SWITCH PROFILE       │              │
                      │               ▾              ▾
                 ┌────┴─────────────────────────────────────┐
                 │                 PORTAL                    │
                 │  ┌────────────────────────────────────┐  │
                 │  │ 2×4 slot grid                      │  │
                 │  │  tap empty → opens ToyBox           │  │
                 │  │  tap loaded → REMOVE overlay (5s)   │  │
                 │  └────────────────────────────────────┘  │
                 │  ┌────────────────────────────────────┐  │
                 │  │ ToyBox (lid open)                  │  │
                 │  │  tap figure → FigureDetail panel    │  │
                 │  │  PLACE → portal impact, lid closes  │  │
                 │  │  BACK → returns to box grid         │  │
                 │  └────────────────────────────────────┘  │
                 │                                          │
                 │  kebab → MenuOverlay                     │
                 └──────────────────────────────────────────┘
                      │              │                │
                 SWITCH GAMES    SHUT DOWN     show-join QR
                      │              │         (inline in menu)
                      ▾              ▾
                 GamePicker      app closes

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
| PinEntry | manage btn | KonamiGate | slide right |
| KonamiGate | correct code | ProfileManage | gold flash → crossfade |
| ProfileManage | EDIT | EditProfile | slide right |
| ProfileManage | PIN | PinReset | slide right |
| ProfileManage | LOCK | KonamiGate → PinEntry | slide left |
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

## 3. Future screen landing spots

Screens not yet implemented that need a place in the graph:

| Future screen | Entry point | Category |
|--------------|-------------|----------|
| **Stats drill-down** (6.3) | FigureDetail → stats action icon | Inline overlay (extends the FigureDetail panel or slides a sub-panel) |
| **Variant cycling** (3.14) | FigureDetail → appearance action icon | Inline UI within FigureDetail (carousel or swipe on the hero bezel) |
| **Reset-to-fresh flow** (3.11.3) | FigureDetail → reset action icon | Opens ResetConfirm scrim modal |
| **Ownership badge** (3.10.7) | Portal slot — always visible on occupied slots | Portal-screen decoration, not a navigation target |
| **Show-join-code** (3.10.8) | MenuOverlay — QR is inline, not a separate screen | Already in MenuOverlay mock |
| **Crystal extra-confirm** (3.11.4) | ResetConfirm variant for Imaginators creation crystals | Same modal, stronger warning copy |
