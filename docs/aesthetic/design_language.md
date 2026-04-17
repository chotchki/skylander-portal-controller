# Design Language — Skylander Portal Controller

The shared vocabulary for every screen in the phone app and (palette/motif-aligned) the egui launcher. Locked during Phase 4 mockup review.

**Direction**: Heraldic — maximalist Skylanders faithfulness. Thick embossed gold bezels, starfield blue, chunky outlined display type, layered depth, *large* confident animations. Reference: `docs/aesthetic/ui_style_example.png`, `Screenshot 2026-04-15 17161?.png`, in-game loading vortex.

**Scope**: this doc is specification, not the implementation. 4.1 turns each section into CSS vars; 4.4 turns the material patterns into shared Leptos components. Update this doc as decisions change — treat the mocks as canonical examples.

---

## 1. Color palette

### Starfield (background)
| Var | Hex | Use |
|-----|-----|-----|
| `--sf-1` | `#0b1e52` | Top of body gradient |
| `--sf-2` | `#061436` | Mid |
| `--sf-3` | `#020818` | Bottom / default bg |

Layered with radial gradients: top ellipse `#1a3a8a → transparent 50%`, bottom ellipse `#0e2464 → transparent 50%`. A tiled SVG star-dot layer drifts at ~140s/loop. Single shared background on `body`.

### Gold (bezel, borders, display type)
| Var | Hex | Use |
|-----|-----|-----|
| `--gb` | `#ffe58a` | Gold bright — highlights, brightest bezel stop, title text |
| `--g` | `#f5c634` | Gold primary |
| `--gm` | `#c58c18` | Gold mid — bezel midtone |
| `--gs` | `#6e4a00` | Gold shadow — text shadow, inset lowlight |
| `--gi` | `#3a2500` | Gold inner — dark bezel ring, text stroke |

All bezel/border gold uses a radial or linear gradient through multiple stops. Flat gold is forbidden (kills the metallic feel).

### Skylanders blue (material surface — buttons, chips, overlay cards)
Solid, saturated. Full opacity — these are the "thing sticking out of the panel" surfaces.

| Role | Gradient |
|------|----------|
| Blue-card rest | `linear-gradient(180deg, #2f6edc 0%, #1e4fb3 55%, #153a8a 100%)` |
| Blue-card hover | `linear-gradient(180deg, #4286f0 0%, #2a62cd 55%, #1b46a0 100%)` |
| Blue-panel bg (for `<FramedPanel>`) | `linear-gradient(180deg, rgba(26,58,138,0.95), rgba(10,36,100,0.95) 50%, rgba(6,20,54,0.98))` |

Blue cards always have a full `var(--gb)` 2px border, not faded.

### Wood (toy box lid + interior)
| Var | Hex | Use |
|-----|-----|-----|
| `--wood-1` | `#6e5032` | Light plank |
| `--wood-2` | `#4a3220` | Mid plank |
| `--wood-3` | `#352416` | Shadow between planks |
| `--wood-4` | `#251810` | Deep interior |
| `--wood-gold` | `#d4a84a` | Wood gold edge |
| `--wood-gold-b` | `#f0d080` | Wood gold bright |

Closed lid: smooth wood with vertical-plank repeating-linear-gradient. Open lid: horizontal slats via `repeating-linear-gradient(180deg, …)`. Both get a gold edge trim + `radial-gradient` clasp centerpiece.

### Status colors
| Var | Hex | Use |
|-----|-----|-----|
| `--success` / pip | `#65d68c` | Connection live |
| `--danger` | `#e34c4c` | Error bezels, shutdown button, error banners |
| `--danger-dark` | `#7a1818` | Danger inner ring |
| Loading | gold (`--gb`) | Spinning ring uses gold, not a neutral |

### Elements (figure inner plates)
Linear gradients (135°):

| Element | Gradient |
|---------|----------|
| Fire | `#ff6b2a → #b13310` |
| Water | `#2aa6ff → #0f4d8a` |
| Life | `#5ac96b → #26612e` |
| Magic | `#da5ad6 → #651564` |
| Undead | `#8a62c9 → #3b215f` |
| Tech | `#ffb84d → #a56500` |
| Earth | `#a77b3a → #5a3a13` |
| Air | `#c6e6ff → #7ea9cc` (ink text, not white) |
| Light | `#fff8a0 → #c8b02c` (ink text) |
| Dark | `#2a2a3a → #05050a` |

The **bezel is always gold** regardless of element. Only the inner plate is element-tinted. This keeps the visual language consistent across the whole UI while still signaling element.

### Kaos palette (used by Kaos overlays in Phase 4 + skin swap in 5.4)
Distinct evil-lair-of-Kaos color system. Used as full palette for the two Kaos overlays (`kaos_takeover.html`, `kaos_swap.html`) and later as a `body.skin-kaos` class-swap for the whole app in Phase 5.4. Defined as sibling vars so no component CSS needs to change.

| Var | Hex | Use |
|-----|-----|-----|
| `--k-void-1` | `#1a0a40` | Top void gradient |
| `--k-void-2` | `#120628` | Mid void |
| `--k-void-3` | `#070212` | Deep void / default bg |
| `--k-violet` | `#7a4eff` | Bright violet — lightning, accent |
| `--k-violet-deep` | `#2d0f6a` | Deep violet — display text stroke |
| `--k-ember` | `#ff6b9e` | Ember pink — primary Kaos accent |
| `--k-ember-deep` | `#8a2a56` | Ember shadow — bezel lowlight |
| `--k-magenta` | `#da28a8` | Magenta mid — gradient midtone |
| `--k-magenta-bright` | `#ff4fd0` | Magenta bright — highlights, active |
| `--k-lime` | `#b8ff3a` | Lime accent (sparingly — Kaos pop) |

**Kaos cards** (buttons, taunt cards) replace the blue-card material: gradient `#ff4fd0 → #da28a8 → #8a2a56`, border `#ff6b9e`, same inset highlight/shadow depth structure.

**Kaos framed panel** bg: `linear-gradient(180deg, rgba(45,15,106,0.92), rgba(26,10,64,0.95))`. Gradient border through Kaos hues: `linear-gradient(135deg, #ff6b9e, #da28a8, #7a4eff, #ff4fd0)`.

**Kaos background surface**: layered `radial-gradient` of void-violet + ember-deep hotspots on `--k-void` base. Hex-tile `radial-gradient` dot pattern at 120×104px tile, animated `hex-pulse` at 4s ease-in-out. Crackling magenta/ember sparks layer at 5s flicker. Edge vignette pulling toward black.

---

## 2. Typography

### Fonts
| Role | Family | License | Source |
|------|--------|---------|--------|
| Display | **Titan One** | OFL | Google Fonts |
| Body | **Fraunces** | OFL | Google Fonts |
| Mono (readouts, debug) | JetBrains Mono | OFL | Google Fonts |

Self-host as WOFF2 under `phone/assets/fonts/` — no external CDN in production (see feedback memory). Declare `@font-face` with `font-display: swap`.

### Scale
| Token | Size | Weight | Use |
|-------|------|--------|-----|
| `--t-display-hero` | 46px (mobile) / 64px (egui) | Titan One | Welcome / Portal title |
| `--t-display-lg` | 38px | Titan One | Profile name in PIN, hero figure name |
| `--t-display-md` | 26–32px | Titan One | Game titles, section titles |
| `--t-display-sm` | 16px | Titan One | Button labels, panel titles |
| `--t-display-xs` | 10–13px | Titan One | Slot labels, chip labels |
| `--t-body` | 14px | Fraunces regular | Body copy |
| `--t-body-italic` | 12–13px | Fraunces italic | Labels, subtitles, captions |
| `--t-body-sm` | 10–11px | Fraunces italic | Hints, taglines |

### Display treatment (always)
Every Titan One heading — top-to-bottom layered:

1. `color: var(--gb)` (gold fill)
2. `-webkit-text-stroke: 1–2px var(--gi)` (dark gold edge, thickness by font size)
3. `text-shadow:
     0 2px 0 var(--gs),
     0 3px 0 var(--gi),
     0 5px 10px rgba(0,0,0,0.5),
     0 0 20–40px rgba(245,198,52,0.3)` (drop shadow + soft gold glow)

Wrap larger titles in a `.title-wrap` with a `.title-rays` pseudo (radial gold ellipse) behind for an ambient halo.

### Letter spacing
- Display: `0.04em` – `0.22em` depending on size (more at smaller sizes to hold presence)
- Italic labels: `0.12em` – `0.25em` (keep them breathy)
- Body: `normal`

### Never
- Flat white titles (they vanish into the starfield)
- Lowercase Titan One (the font is for CAPS)
- Body text in gold (readability cliff — use the gold for accent only)

### Contrast & readability contract
Contrast failures on this project have one root cause: **semi-transparent text**. The rule:

- **All text is opaque.** No `rgba(..., 0.5)` / `opacity: 0.6` fade-outs on type. Alpha-dimmed text is the pattern we ban.
- **Pick text color by role, not by "how prominent should this feel."** The three roles + their palette on dark surfaces:

| Role | Examples | Color on starfield/blue panel | Color on danger/Kaos panel |
|------|----------|-------------------------------|----------------------------|
| **Primary** — headings, sub-heads, body paragraphs, warning text | "WELCOME BACK", "pick up where you left off?", reset warning | `#fff4e0` warm-white (italic often Fraunces 600+ for body weight) | `#f3e8e4` / `#ffd9ee` |
| **Accent** — display glyphs, short Titan One labels, button text | "RESUME", "— YOUR LAST ADVENTURE —" section dividers | `var(--gb)` / `var(--g)` gold | `var(--k-magenta-bright)` magenta |
| **Muted** — captions, timestamps, tertiary hints, small italic labels | "saved 3 days ago", "live" pip label, "— AVA" caption under swatch | `#d9c88a` muted gold *or* `#e8e4d8` warm off-white | `#ffd6d6` |

- **Minimum sizes on dark surfaces**: body 13px, caption 11px. Below that, bump weight (Fraunces 600+) or switch to Titan One — never shrink + fade together. Add a single-pixel dark `text-shadow: 0 1px 0 rgba(0,0,0,0.5)` to anchor italic Fraunces on glowy backgrounds.
- **If text needs to sit on a busy / gradient / dimmed surface, wrap it in a blue-card (or inner plate) so it has a solid contrasting backdrop.** Menu overlay's QR + profile chip are the canonical example; reset-confirm's warning block follows the same pattern inside a danger-bordered panel.
- Gold fill (`--g`, `--gb`) is an **accent** color. Fine for display type + small Titan One labels; forbidden for paragraph/body copy — too much saturated yellow at reading size fatigues fast.
- Target WCAG 4.5:1 for body. Short display type can relax to 3:1 since size carries legibility. If a text color fails on the intended surface, the fix is a stronger text color or an inset card — not opacity.
- **Flavor-text carve-out.** Non-actionable decorative typography — Kaos quote glyphs, taunt attributions, ambient lair-of-Kaos insult copy, opening/closing `"` marks around quotes, etc. — may dial opacity down for atmosphere (roughly 0.5–0.75 max), *provided* the core insult/quote remains readable at a glance on the target device. Think of these as a visual flourish, not UI copy. The insult text itself (what the user is being razzed with) stays opaque per the main rule.

### Kaos display treatment (Kaos overlays only)
When a title sits on a Kaos background instead of the starfield, swap the heraldic gold treatment for the villainous one:
- `color: var(--k-magenta-bright)` (fill)
- `-webkit-text-stroke: 1.5px var(--k-violet-deep)` (deep-violet edge)
- `text-shadow: 0 2px 0 var(--k-ember-deep), 0 3px 0 var(--k-void-3), 0 0 28px var(--k-magenta), 0 0 56px rgba(255,107,158,0.45)` (layered drop + magenta/ember outer glow)
- Add a subtle `title-shake` glitch animation (~2.8s cycle, brief translateX + skewX at 90–94% marks) — makes the text feel unstable/villainous. Keep the main idle position still so it's readable.

Kaos taunt body copy: Fraunces italic, `color: #ffd9ee`, with soft magenta text-shadow for presence on the dark void. Closed with a `— KAOS` attribution in Titan One magenta.

---

## 3. Materials

Three material patterns do 95% of the work. Every surface is one of these.

### 3.1 Gold bezel
The signature motif. Any round-figure presentation (portal slot, figure card, profile swatch, action icon, PIN dot, game emblem).

**Structure** (outer → inner):
```
[bezel-ring]
  background: radial-gradient(circle at 30% 25%, var(--gb), var(--g) 22%, var(--gm) 55%, var(--gs) 100%)
  box-shadow:
    inset 0 0 0 2px var(--gi)         /* inner dark ring */
    inset 0 3px 4px rgba(255,255,255,0.3)  /* top highlight */
    inset 0 -3px 4px rgba(0,0,0,0.5)  /* bottom shadow */
    0 0 0 1px #000                     /* outer outline */
    0 4px 10px rgba(0,0,0,0.6)        /* drop shadow */

[plate]  <- inset 10% of the bezel
  background: var(--el-<element>)
  box-shadow:
    inset 0 0 0 1px rgba(0,0,0,0.6)
    inset 0 4px 8px rgba(255,255,255,0.25)
    inset 0 -6px 10px rgba(0,0,0,0.5)
  contains: <img class="thumb"> (primary) or letter label (fallback / profile)
```

**Sizes**: 32px (chip icon) · 40–56px (emblems) · 100–128px (portal slot) · 160px (figure hero).

**States** (applied to `.slot` or wrapper):
- `default`: no extra treatment
- `picking`: scale 1.05, outer glow `0 0 22px 4px rgba(255,230,120,0.75)`, rotating ray halo (see 5.2)
- `loading`: gold sweep conic-gradient at 0.9s/rev, plate dims to 0.55 opacity
- `loaded`: idle float (see 5.3)
- `errored`: bezel swaps to red gradient `#ff9b9b → #e34c4c → #7a1818`, shudders 2× at 0.4s
- `disabled`: bezel dims to `rgba(245,198,52,0.2) → rgba(110,74,0,0.25) → rgba(0,0,0,0.35)`, plate goes deep blue

### 3.2 Blue card
Any tappable or groupable surface inside an overlay/panel. Profile chips, action buttons, QR wrappers, stats strips.

```
background: linear-gradient(180deg, #2f6edc 0%, #1e4fb3 55%, #153a8a 100%)
border: 1.5–2px solid var(--gb)    /* always full-bright gold */
border-radius: 12px (button) | 16px (card) | 999px (pill)
box-shadow:
  inset 0 2px 0 rgba(255,255,255,0.25)     /* top highlight */
  inset 0 -2px 0 rgba(0,0,0,0.25)          /* bottom lowlight */
  0 3px 0 rgba(0,0,0,0.45)                 /* offset drop (physical button depth) */
  0 5px 12px rgba(0,0,0,0.4)               /* ambient shadow */
```

**Hover/active**: gradient brightens, gold glow `0 0 16–20px rgba(245,198,52,0.3)` added. Active: translateY(2px) and collapse the offset drop so it looks pressed.

**Danger variant**: swap blues for `#c23a3a → #9a2424 → #6e1414`, border to `#ffc0c0`. Keep everything else identical.

**Never**: faded/transparent fills. That's the trap we fell into — washed-out blue loses the material's identity. Solid gradient always.

### 3.3 Framed panel
The overlay/modal surface — PIN keypad, figure detail, menu overlay, resume modal, takeover.

```
padding: 20–26px
border-radius: 18–28px
background:
  radial-gradient(ellipse 120% 80% at 50% 0%, rgba(255,230,120,0.08–0.10), transparent 60%),
  linear-gradient(180deg, rgba(26,58,138,0.92), rgba(10,36,100,0.95) 50%, rgba(6,20,54,0.96))

/* gold gradient border via :before */
&::before {
  content: ""; position: absolute; inset: -2px;
  border-radius: <outer radius>;
  background: linear-gradient(135deg, var(--gb) 0%, var(--gm) 30%, var(--gb) 50%, var(--gm) 70%, var(--gb) 100%);
  z-index: -1;
}
```

**No corner brackets.** The gradient border does the framing — corners were visual noise. This rule is uniform across every panel.

Entrance animation: `panel-in` keyframe (opacity 0→1, scale 0.92→1) at 0.4s cubic-bezier(.3,1.3,.5,1).

### 3.4 Wood (toy box)
Closed lid (bottom of portal screen), open lid (slatted top), box interior (dark well).

**Closed lid (solid plank)**:
```
background: linear-gradient(180deg,
  var(--wood-gold-b) 0%,
  var(--wood-gold) 3%,
  var(--wood-1) 6%, var(--wood-1) 12%,
  var(--wood-2) 50%,
  var(--wood-3) 100%)
border-top: 3px solid var(--wood-gold)
height: 64px (closed) | auto with max-height (open, expandable)
```

Vertical plank seams via `::before { background: repeating-linear-gradient(90deg, transparent 48px, rgba(0,0,0,0.08) 50px); }`.

**Open lid (slatted, at top of box interior)**:
```
background:
  repeating-linear-gradient(180deg,
    var(--wood-2) 0, var(--wood-1) 2px, var(--wood-1) 18px,
    var(--wood-3) 19px, rgba(0,0,0,0.4) 20px, var(--wood-2) 21px),
  linear-gradient(180deg, var(--wood-1), var(--wood-2))
border-top: 2px solid var(--wood-gold)
border-bottom: 3px solid var(--wood-gold)
margin-top: var(--header-h)  /* never overlaps the header */
```

**Box interior (dark well)**:
```
background: linear-gradient(180deg, var(--wood-4) 0%, #1a0e06 4%, #0e0804 20%, #080402 100%)
```
Side walls (6px strips via `::before` / `::after`) give the illusion of sitting *inside* a box. Top fade (40px gradient from wood-4 to transparent) makes figures scroll behind the lid. Bottom fade (120–140px from transparent to near-black) creates the "receding into the depths" illusion.

### Clasp / fittings (gold hardware)
Used on the closed-lid centerpiece and anywhere gold-metal accent is called for. Multi-stop radial gradient:
```
background: radial-gradient(circle at 40% 30%, var(--gb), var(--gm) 50%, var(--gs))
box-shadow:
  inset 0 2px 3px rgba(255,255,255,0.4)
  inset 0 -2px 3px rgba(0,0,0,0.5)
  0 2px 6px rgba(0,0,0,0.6)
```

---

## 4. Layout

### Grid
- Phone canvas: **390 × 844** (design viewport, scales fluidly)
- Safe padding: 16–20px horizontal, 44px top (iOS notch), 24–32px bottom
- Header height: **88px** (fixed, never overlapped by lids/modals)
- Closed lid height: **64px** (visible peek at bottom of portal)

### Portal grid
**2 wide × 4 tall.** Not 4×2. Large bezels, big tap targets. Gap: 16px vertical, 20px horizontal. Slot labels (Titan One 11px) sit below each bezel.

### Collection figure grid (inside the box)
**4 wide.** Denser — people are scanning for one specific figure. Gap: 14px vertical, 10px horizontal. Figure names (Fraunces 10px italic) below each bezel, max-width 80px.

### Button hierarchy
- **Primary action**: solid gold (`.btn-primary` — Titan One label, gold gradient fill, offset drop shadow). Used sparingly — 1 per screen max. PLACE ON PORTAL is the archetype.
- **Secondary action**: blue card (`.action` in the menu overlay pattern). Multiple OK.
- **Destructive**: red card (danger variant of blue card). Isolated; never next to primary gold.
- **Tertiary / utility**: outline-only (transparent bg, faded gold border). "Back", "Cancel".

### Hold-to-activate — impactful & destructive actions
Actions that are **either** irreversible **or** forcibly change state for other connected players require a **press-and-hold** interaction instead of a single tap. Three tiers:

| Action class | Example | Pattern |
|--------------|---------|---------|
| **Destructive** — data gone, can't be restored | `RESET <figure>`, `SHUT DOWN`, future `DELETE PROFILE` | Hold-to-activate + **danger** (red) visual |
| **Impactful but recoverable** — reshapes session state for everyone connected, but no data loss | `CHOOSE ANOTHER GAME` (kicks all phones out of current game) | Hold-to-activate + **normal** (blue-card) visual |
| **Local / recoverable** — affects only this phone, or the undo is one tap away | `SWITCH PROFILE`, `REMOVE figure` from portal slot, `RESUME`, `BACK TO BOX` | Single-tap, no hold |

Hold mechanics (shared across tiers 1–2):

- **Label carries the verb** — "HOLD TO RESET", "HOLD TO SHUT DOWN", "HOLD TO SWITCH GAMES". Not just "RESET" + hidden hold behavior. Make the gesture part of the copy.
- **Progress fill** sweeps left→right across the button (`::before` / `.hold-fill` layer, `transform: scaleX(0→1)`) over `--dur-hold-confirm: 1200ms`, `mix-blend-mode: screen` so the label stays readable as it passes under.
  - Default (blue-card / normal action): warm gold→white gradient fill — heraldic on blue.
  - Danger variant (red-card): pink-white gradient fill + red glow on the `.fired` flash.
- **Release-to-cancel.** Lifting the finger before the fill completes reverts the fill (`transition: transform 0.2s ease-out` resets to 0). No action fires.
- **Complete-to-fire.** Hold through the full duration → the button flashes (`.fired` state, ~420ms keyframe) and the action is committed.
- **Label shadow is heavy** — `text-shadow: 0 0 2px rgba(0,0,0,0.9), 0 1px 0 rgba(0,0,0,0.7), 0 2px 6px rgba(0,0,0,0.5)` — so white text stays readable on both the idle bg *and* the bright-white filled bg.

Shared implementation: the `[data-hold]` attribute + a single JS snippet that binds `pointerdown/up/leave/cancel`. Danger variant only changes the fill color + flash palette. See `reset_confirm.html`, `menu_overlay.html` (CHOOSE ANOTHER GAME + SHUT DOWN).

**Don't over-apply hold-friction.** Every extra hold-gated tap is a child mashing a button that won't obey. The bar is: *would a single mistaken tap hurt someone other than me?* If the answer is no (REMOVE puts the figure back in the box; SWITCH PROFILE only re-locks my session), stay with single-tap. `REMOVE` uses a **selection-then-confirm** pattern instead: tap the slot to select (gold-ringed bezel + scale 1.04); a full-width red `REMOVE` bar (~1/3 slot height, edge-to-edge) overlays the middle of the figure portrait; tap the bar to execute, tap outside to dismiss. The selection **auto-dismisses after 5s** of no interaction so a stray tap doesn't leave the slot armed behind the player's back. See `portal_with_box.html`.

### Spacing
Stick to multiples of 4: 4, 6, 8, 10, 12, 14, 16, 20, 24, 32. Don't invent 7px or 13px gaps.

### Safe areas
Respect `env(safe-area-inset-*)`. Corner radius on the device matters — keep tappable targets 12px+ from the viewport edge.

### Tap-target reachability
**Never clip interactive controls.** Buttons, chips, inputs, and other tappable elements must remain fully visible and reachable at every state their container can be in. The failure mode this rule exists to prevent:

- A panel/drawer/lid has `overflow: hidden` + a `max-height` that's smaller than its content ⇒ the last row of chips or a button gets silently truncated.
- A sticky footer or toast overlaps the last action in a scrolling list ⇒ user can see the button but can't tap it.
- An absolutely-positioned decoration (ornament, ray halo, grabber pill) is stacked *over* a chip/button instead of under or beside it.

The fixes, in order of preference:

1. **Let the container grow** if it can — prefer `min-height` + content-flow over hard `max-height` with `overflow: hidden`.
2. **Scroll internally** when the container has a fixed envelope (lids, modals, side sheets). Add `overflow-y: auto`, give the scroll region enough `min-height: 0`, and surface a **fade-mask at the clipping edge** (e.g. `mask-image: linear-gradient(180deg, #000 0, #000 calc(100% - 14px), transparent 100%)`) so the user sees "there's more below" instead of assuming the content ends.
3. **Reserve footer safe-space** for any actions or bottom-pinned UI so scrollable content never parks a button behind them.
4. **Decorative overlays get `pointer-events: none`** and sit behind interactive siblings in the stacking context.

The toy-box lid's expanded filter area (`portal_with_box.html`) is the canonical worked example.

### Stable layout for expected error states
**Common-error feedback must not shove the surrounding layout.** Any error / validation / status banner that can appear on a screen should **reserve its slot up front** — `min-height`-sized, `visibility: hidden` + `opacity: 0`, inserted into the normal flow. When the error fires, only opacity / transform animate; the dots above and the keypad below stay put.

Failure modes this rule prevents:

- User taps a wrong PIN, the mismatch banner slides in between the dots and keypad, pushing the keypad *into* the action buttons — thumbs mis-hit because the target just moved.
- User clears a bad filter, the empty-state banner disappears, dots stop shifting vertically half a second after the user started reading.
- A toast pushes the main action off-screen during load → fail-state glitch.

The rule is strict for **expected** errors (`PIN mismatch`, `name already taken`, `no figures match filter`) — the ones you know can happen on that screen. One-off system errors (offline, server 500) can use a toast/overlay that *does* take temporary layout space, since they're surprises anyway and shouldn't be the common path.

Pattern (see `profile_create.html` step 4 for the worked example):

```css
.pin-error {
  min-height: 36px;        /* reserve the slot — never 0 */
  visibility: hidden;
  opacity: 0;
  transform: translateY(-4px) scale(0.96);
  transition: opacity 0.22s ease-out, transform 0.28s cubic-bezier(.3,1.5,.5,1);
}
.screen.mismatch .pin-error {
  visibility: visible;
  opacity: 1;
  transform: translateY(0) scale(1);
  animation: mismatch-pop 0.42s cubic-bezier(.3,1.5,.5,1);
}
```

Pair the banner animation with a brief shake on the offending input (`.pin-dots` here) so the correction surface is as obvious as the message.

---

## 5. Motion

### Timing tokens
| Token | Value | Use |
|-------|-------|-----|
| `--dur-tap` | 120ms | Button press feedback |
| `--dur-quick` | 200–250ms | State-fade transitions |
| `--dur-impact` | 600ms | Portal impact flash |
| `--dur-shudder` | 400ms (×2) | Error shake |
| `--dur-loading-sweep` | 900ms | Loading ring rotation period |
| `--dur-halo-slow` | 3.4s | Picking ray halo period |
| `--dur-idle-float` | 4.5s | Loaded-slot breathe |
| `--dur-panel-in` | 400ms | Modal/panel entrance |
| `--dur-sky-drift` | 140s | Starfield parallax |

### Easing
| Token | Curve | Use |
|-------|-------|-----|
| `--ease-spring` | `cubic-bezier(.3,1.3,.5,1)` | Entrance, card rise, panel in (overshoots slightly — feels toy-like) |
| `--ease-tap` | `cubic-bezier(.4,2.2,.5,1)` | Button active scale |
| `--ease-smooth` | `cubic-bezier(.4,.0,.2,1)` | Drawers, lid open/close |
| `--ease-linear` | `linear` | Continuous rotations (halos, starfield drift) |

### 5.1 Card entrance (stagger-rise)
Profile cards, game cards, slot reveals. 80ms stagger per item, 500ms spring-ease.
```css
@keyframes card-rise {
  from { opacity: 0; transform: translateY(24px) scale(0.97); }
  to   { opacity: 1; transform: translateY(0) scale(1); }
}
```

### 5.2 Ray halo (picking / hero)
Slow-rotating soft conic gradient masked to a ring. Tight to the bezel (`inset: -8px`), `filter: blur(8px)`, masked with `radial-gradient(circle, black 50%, transparent 90%)` so it never bleeds onto surrounding surfaces. No `mix-blend-mode: screen` (learned: it lights the whole panel gold).

### 5.3 Idle float (loaded)
Subtle vertical drift — ±2px on slots, ±6px on the hero. 4.5s ease-in-out, staggered via `animation-delay` negatives so adjacent slots aren't in sync.

### 5.4 Portal impact (loading → loaded)
Radial white→gold flash, 600ms, scale 0 → 2×, opacity 0 → 1 at 30% → 0. Triggered on the target slot after WS confirms the figure landed. Combined with a brief bezel brightness spike.

### 5.5 Error shudder
2× 400ms translateX ±4px. Bezel crossfades to red gradient simultaneously.

### 5.6 Motion focal rule
**One focal animation at a time.** When a state-indicating animation starts (loading ring, shudder), suppress ambient animations on the same element (auras, idle floats, rays). Restore them when returning to idle. Competing motion makes status unreadable.

### 5.7 Accessibility
`prefers-reduced-motion: reduce` — kill all ambient rotations, halo spins, idle floats, starfield drift. Keep entrance animations (they communicate state) but shorten to 200ms linear. State-indicating motion (loading ring, shudder) stays but can be reduced to a pulse opacity.

---

## 6. Components (Leptos targets for 4.4)

Every component in this list has a mock that exercises it. Implementation follows the mocks.

### 6.1 `<GoldBezel>`
Props: `size`, `element`, `state`, `thumb_src`, `fallback_letter`. Renders the bezel + plate + (img or letter). Reused by: portal slots, browser figure cards, profile swatches, game emblems, hero figure, action icons, color-picker swatches. See mock: `option_a_heraldic.html`.

### 6.2 `<FramedPanel>`
Props: `size_variant` (modal / sheet / detail), children. No corner brackets; gold gradient border only. See mocks: `pin_keypad.html`, `figure_detail.html`, `menu_overlay.html`.

### 6.3 `<DisplayHeading>`
Props: `size_variant`, optional `with_rays: bool`, children (text). Applies the full gold-fill + stroke + shadow treatment. Optionally wraps in `.title-rays` halo. See mock: every screen.

### 6.4 `<RayHalo>`
Props: `speed` (slow/fast), `tight` (bool — `inset: -8px` vs more). For picking/loading states. `prefers-reduced-motion` aware. See mocks: `transitions.html`, `figure_detail.html`.

### 6.5 `<FigureHero>`
Props: `figure`, `state` (default/loading/errored). Composition: oversized `<GoldBezel>` + soft aura + `<RayHalo>` + optional `loading-ring` overlay. Used by `<FigureDetail>` and Kaos swap overlay. See mock: `figure_detail.html`.

### 6.6 `<BlueCard>` / `<ActionButton>`
Props: `variant` (default / danger), optional leading icon, title, description, `on_click`. See mock: `menu_overlay.html`.

### 6.7 `<ToyBoxLid>` + `<ToyBoxInterior>`
The signature layout component. Three visual states + a gesture model.

**States:**
1. **Closed** — wooden plank across the bottom of the portal view (64px tall).
2. **Open (compact)** — slatted wood lid pinned at top below the header, single SEARCH button visible, figure grid fills the box interior below.
3. **Open (filters expanded)** — lid grows down (~320px max-height) revealing search input + drill-down filter chips (GAMES / ELEMENTS / CATEGORY).

**Gesture model** (progressive swipe-down through the open states):

| Current state | Tap lid | Swipe up on lid | Swipe down on lid |
|---------------|---------|------------------|---------------------|
| Closed | Open (compact) | Open (compact) | — |
| Open (compact) | Tap ✕ = Close | — | Expand filters |
| Open (expanded) | Tap ✕ = Close | Collapse filters | Close |

Tap SEARCH button also expands filters (equivalent to swipe-down-on-lid). Scrolling the figure grid collapses expanded filters back to compact (looking deeper into the box). Picking a figure in the detail view (section 6.5) closes the box entirely after placement.

Implementation: Leptos signal-driven state machine `BoxState::{Closed, Open(CompactOrExpanded)}`. Touch handlers use `pointerdown` / `pointerup` / `pointermove` with a `translateY` threshold (40–60px) + direction check. The same handlers live on the lid surface in both states — the translation target changes. Scroll listener on the `box-scroll` element handles the auto-collapse.

See mock: `portal_with_box.html` (click to toggle; swipe gestures + scroll-collapse wired in the implementation).

### 6.8 `<KebabMenuOverlay>`
The header-kebab overlay. Contains `<FramedPanel>` + profile chip (`<BlueCard pill>`) + join-code card (`<BlueCard>` wrapping a gold-framed QR) + action list. See mock: `menu_overlay.html`.

### 6.9 Header
`<AppHeader>` — kebab (⋮ left) → profile swatch → profile name + current game → "live" pip (right). Fixed 88px height. See any in-game mock.

### 6.10 `<KaosOverlay>` (Phase 4 shell, wired in 5.x)
Full-screen overlay used by both Kaos moments. Variants:
- **`takeover`** — evicted session; shows Kaos sigil (128px) + "KAOS REIGNS!" title + quote card + `KICK BACK IN` button. Used by `TakenOver` WS event handler. See `kaos_takeover.html`.
- **`swap`** — mid-game 1-for-1 figure swap; shows Kaos sigil (72px) + "KAOS STRIKES!" title + outgoing→⚡→incoming figure row + taunt card + `BACK TO THE BATTLE` button. See `kaos_swap.html`.

**Background composition** (bottom-to-top, both variants):
1. **Void base** — radial gradients of `--k-violet-deep` (top, 50%/30%), `--k-ember-deep` (bottom-left, 30%/80%), `--k-violet` @ 40% alpha (right, 80%/60%) over the `--k-void-1 → --k-void-2 → --k-void-3` vertical gradient.
2. **Hex-grid dot pattern** — magenta/ember radial dots tiled at 120×104px, `opacity 0.9`, animated `hex-pulse` 4s ease-in-out (50% → 0.5).
3. **Sparks** — 7-point scattered magenta/ember/violet radial dots over a 400×800 tile, `sparks-flicker` 5s alternate (50% → opacity 0.5, scale 1.05).
4. **Edge vignette** — radial `transparent → rgba(0,0,0,0.7)` from 50% outward, plus a faint `135deg` magenta lightning scar stripe.

For the **swap** variant only: the live portal/box view shows through at ~35% opacity + 2px blur beneath the void. Players retain spatial context for what just happened.

**Swap motion sequence** (drives the center row):
1. t=0 — outgoing figure at center, full opacity, full saturation.
2. t=100ms — outgoing shifts left ~60px, `filter: grayscale(0.6) hue-rotate(-20deg)`, red `✕` overlay fades in.
3. t=400ms — lightning `⚡` bolt flashes between the two positions (opacity 0 → 1 → 0 over 500ms, slight rotate ±8°).
4. t=600ms — incoming figure lifts in from below, `FigureHero` treatment with magenta aura instead of gold, `card-rise` tuned shorter (280ms spring).
5. Taunt card + button fade in at ~t=900ms.

**No auto-dismiss.** Phones usually sleep during gameplay, so a timer-driven close would leave users confused ("what just happened?"). The overlay persists until the user explicitly taps the dismiss button. If Kaos fires multiple times while the overlay is active, latest-fires-wins (or queue — decide in 5.2 implementation).

**Quote / taunt card**: a framed panel with its gold gradient border swapped for a Kaos gradient (`135deg, --k-ember → --k-magenta → --k-violet → --k-magenta-bright`). Body copy uses Kaos display-italic spec (§2). Decorative opening/closing quote glyphs in Titan One magenta at 50% opacity flank the text. Closes with a `— KAOS` Titan One attribution in `--k-ember` with magenta glow.

**Buttons** (`KICK BACK IN`, `BACK TO THE BATTLE`): Kaos card material from §1. Titan One label, 0.22em tracking, ember-pink `kick-pulse` box-shadow animation (~2.5s) so the dismiss affordance is always visible even while the sigil is the focal point. The motion focal rule (§5.6) does not demote these — they're the only interactive element, so their pulse is the exception.

Full Kaos palette + materials. Reuses `<FigureHero>` (section 6.5) for the incoming figure lift. The outgoing figure gets a crossed-out overlay + a desaturating hue-rotate filter to show it's being rejected.

### 6.11 Kaos sigil (`.kaos-sigil`)
The stylized Kaos skull/crown is instantly recognizable and the signature brand element for Kaos moments. Asset: `docs/aesthetic/kaos_icon.svg` (hand-traced from an in-game reference screenshot). Bundled at `phone/assets/kaos.svg` for production.

Rendered via CSS `mask-image` so the fill inherits whatever palette the context uses — Kaos palette by default, but a blue/gold variant could exist for "connected to the portal" indicators if we ever want one. SVG scales crisply to any size (86" TV / launcher egui included).

```css
.kaos-sigil {
  width: Npx; height: Npx;  /* 72px in swap overlay, 128px in takeover */
  position: relative;
  filter: drop-shadow(0 0 Npx var(--k-magenta)) drop-shadow(0 0 Npx var(--k-ember));
}
.kaos-sigil::before {
  content: "";
  position: absolute; inset: 0;
  background: radial-gradient(circle at 50% 35%, #fff 0%, var(--k-ember) 35%, var(--k-magenta) 65%, var(--k-violet-deep) 100%);
  -webkit-mask: url(/assets/kaos.svg) center / contain no-repeat;
          mask: url(/assets/kaos.svg) center / contain no-repeat;
}
```

Animated with a 1.5–2.2s `sigil-pulse` — synchronized scale + drop-shadow intensity swell. Slower in the takeover (more menacing), faster in the swap (more frantic).

**Authoring / loading notes**:
- Source SVG is a single black-filled path, viewBox `0 0 271.79761 244.31441`, produced by hand-tracing in Inkscape from an in-game screenshot. Color of the fill is irrelevant — only the silhouette shape matters because it's used as a mask.
- Browsers block `mask-image: url(...svg)` on some contexts when loaded from `file://` (CORS on local SVG resources). In mocks, the SVG is inlined as a **base64 data URI** directly in the CSS rule so it works when opening the file directly in a browser. Production loads the real `/assets/kaos.svg` over HTTP where CORS isn't an issue — data URI can be dropped from the Leptos component.
- When the SVG changes, regenerate the data URI used in any mock via:
  ```bash
  python -c "import base64, pathlib; \
    uri = 'data:image/svg+xml;base64,' + base64.b64encode(pathlib.Path('kaos_icon.svg').read_bytes()).decode(); \
    pathlib.Path('kaos_icon.datauri.txt').write_text(uri)"
  ```
  Then replace `url('../kaos_icon.svg')` with `url('<contents of datauri.txt>')` in the `.kaos-sigil::before` rule.

**Attribution / trademark note**: the Kaos symbol is Activision/Beenox IP. See PLAN 3.19.6 / Phase 7 release checklist (7.6) — pre-release review must decide whether to ship the literal symbol, a derivative, or a custom Kaos-adjacent sigil. Used in internal mocks + dev builds freely; reconsider before public distribution.

---

## 7. Content rules

### What the user sees
- **Profile names, figure names**: always the canonical strings (never filesystem paths, never IDs).
- **Game names**: short form ("Trap Team", not "Skylanders: Trap Team · BLUS31442").
- **Slot assignment**: hidden. The server picks the slot; the UI never shows "picking for slot N".
- **Stats**: integers and human times ("4h 22m" not "15720s").
- **Counts**: "504 figures" is OK on admin/wiki attribution surfaces. Not on the game picker — kids don't care.
- **Errors**: short plain English. "File in use · Eruptor is already on the portal in another slot." Not "HTTP 409 · resource conflict".

### Voice
- Titles: imperative + short ("PICK A GAME", "ENTER YOUR PIN").
- Subtitles: italic lowercase, breathy. "choose your adventure", "summon thy heroes".
- Button labels: uppercase Titan One. "PLACE ON PORTAL", "SWITCH PROFILE".
- Destructive actions: add a safety cue ("ask a grown-up first"). Always behind a confirm.

### Never
- Serial numbers on user-facing surfaces.
- Technical jargon in toasts ("WebSocket disconnected" → "lost connection · reconnecting…").
- Microcopy that assumes reading level > 7 yo.

---

## 8. Asset policy

- **All assets bundled**. No external CDN links in production. Fonts as WOFF2 under `phone/assets/fonts/`. Images served locally via `/api/figures/...` or bundled via `include_dir!`.
- **Figure thumbs**: `data/images/<figure_id>/thumb.png` from the wiki scrape. 488 / 504 have thumbs; the element-gradient plate is the fallback.
- **Box art**: to source for 4.2.5 game picker (placeholder gradients until real images land).
- **Attribution**: footer line "Data & images from the Skylanders Wiki · CC BY-SA" on surfaces that show wiki-sourced content. See `data/LICENSE.md` + PLAN 3.19.6.

---

## 9. Checklist for new screens

Before opening a new mock file, run through:

1. **Color**: using the palette vars — no new shades without adding them here first.
2. **Typography**: Titan One (display) + Fraunces (body) only. Display treatment applied to every heading.
3. **Material**: every surface is a gold-bezel, blue-card, framed-panel, or wood piece. If it's none of those, add the pattern here before using it.
4. **Header**: if the screen has one, it's the standard 88px kebab/profile/pip composition. Don't reinvent.
5. **No corner brackets on panels.** Gold gradient border only.
6. **Motion**: one focal animation at a time. Ambient suppressed during state transitions. `prefers-reduced-motion` path considered.
7. **Content**: check against section 7 rules. No serials, no tech jargon, no slot numbers in user-facing messaging.
8. **Assets**: everything local. No Google Fonts `<link>` in anything that'll ship.

---

## 10. Open questions

Tracked as PLAN items, parked here for visibility:

- **Box art sourcing**: how do we bundle box art for the game picker without bloating the release zip? (Maybe: one 200px thumb per game, ~60kb total.)
- **Egui cloud vortex (4.15.5)**: web-target aesthetic is settled — `mocks/tv_launcher_v3.html` uses a WebGL fragment shader (5-octave simplex FBM on a cylindrical spiral, 10 iris arms, circular hole). egui port has two candidate paths:
  - **Path A (preferred if feasible)** — port the shader via an `egui_wgpu` custom paint callback. Pixel-for-pixel match to the mock; GPU-cheap at 3840×2160. Spike needed to confirm wgpu integration and the decoupling rule (spiral speed independent of inflow speed) survives the port.
  - **Path B (fallback)** — pre-render 240 frames of the shader at 1920×1080, pack into an atlas texture, sample in egui with `Painter::image`. Larger binary, fixed resolution, but zero GPU risk.
