# Art handoff — what to deliver for the image pipeline

A guide for the artist/designer helping with **Skylander Portal Controller**'s
marketing + app artwork (Steam library cards, app icon, etc.). It's written to
be forwarded directly — you don't need any of the project's code context, just
this page.

The whole point: an automated pipeline (`tools/brand-bake`, run with
`cargo run -p skylander-brand-bake`) composes the final images (Steam needs ~6
different sizes/aspect ratios, plus icons in every platform size). So the most
useful thing you can hand over is the **set of building blocks**, not finished
compositions. Deliver layers and the pipeline re-frames, resizes, and recolors
them for every target — and re-runs for free when you tweak something.

> **Status (filled by PR #3, Alicea Hotchkiss):** the shopping list below is
> delivered as SVGs in `assets/branding/` — `logo.svg` (+ mono variants),
> `icon-1024.svg`, `character.svg`, and `bg-layer-{gradient,stars,vortex}.svg`.
> They're wired into `tools/brand-bake`. To tweak: edit a source SVG, then
> `cargo run -p skylander-brand-bake` to re-bake the icon + Steam set.

## The one principle: components, not compositions

| Hand off this (flexible) | Not this (brittle) |
| --- | --- |
| Logo as its own transparent file | Logo baked into a 600×900 capsule |
| Icon as a 1024px master | Icon pre-sized to 32px, 48px, 256px… |
| Background plate with **no** text on it | A finished card we can't recompose |
| Character cutout on transparency | Character flattened onto a background |

If a size or layout changes later (Steam tweaks dimensions, we add a new
surface), the pipeline handles it with no new work from you.

## The shopping list (priority order)

### 1. Logo / wordmark — "SKYLANDER PORTAL CONTROLLER"
- **Best:** SVG (vector — scales to anything, stays crisp, recolorable).
- **If you paint in raster:** transparent PNG ≥ 3000px wide **+** the layered source.
- Design it to sit on a **dark** background (that's where it lives).
- A plain white/mono version too, if easy — handy for flexibility.

### 2. App icon — the rounded-square "portal" emblem
- A **1024×1024 transparent master** (SVG or PNG).
- Everything derives from this one file: the Windows `.ico` (all sizes), the
  macOS `.icns`, web/PWA favicons, **and** the emblem inside the Steam capsules.
- The current icon is a navy rounded-square with a gold ring and a glowing portal
  center (see `assets/branding/icon.ico`) — keep that DNA or evolve it, your call.

### 3. Background plate(s) — the starfield / atmosphere, **with no text or logo on it**
- One oversized image (~**4000 × 2400**) so the pipeline can crop it tall for the
  portrait capsule (2:3) and wide for the hero (≈3:1) from the same source.
- Even better as **separate layers**: base gradient · star field · glow/vortex.
  That lets us reposition or parallax elements per target.

### 4. (Optional, high-impact) Key / character art
- An original "portal of power" or creature illustration as a **transparent
  cutout**, ≥ 2000px tall. This is what makes a Steam capsule read like a real
  game instead of a logo on a gradient.
- ⚠️ **Original art only** — see the copyright note below.

## What makes a file "pipeline-ready"

- **Transparent PNG or SVG** — never a pre-flattened JPEG at final size.
- **High resolution** — at least 2× the largest target. The hero is 1920px wide,
  so a ~4K master is safe. Downscaling looks great; upscaling turns to mush.
- **sRGB color, 8- or 16-bit** — not CMYK or a print color profile.
- **Keep the editable source** (PSD / Affinity / Figma) so future edits don't
  restart from scratch — exports (PNG/SVG) for the pipeline, source for you.
- **Composition safe-areas:**
  - Leave the **hero's left ~40% clear** — Steam overlays the logo there.
  - Keep important elements off the extreme edges — Steam rounds/crops corners.

## Palette (so it blends with the in-app look)

| Role | Hex |
| --- | --- |
| Starfield (light → deep) | `#0b1e52` · `#061436` · `#020818` |
| Gold base | `#f5c634` |
| Gold bright (highlights) | `#ffe58a` |
| Gold mid (shadows on gold) | `#c58c18` |
| Gold deep (outlines/ink) | `#3a2500` |

Established look + feel to match: `docs/aesthetic/ui_style_example.png`,
`docs/aesthetic/design_language.md`, and the live HTML mocks under
`docs/aesthetic/mocks/` (open `index.html`). Vibe target: the Skylanders game UI —
starfield-blue backgrounds, circular gold-bezeled portraits, bold white titles
with a gold outline, cartoony and warm.

## Target sizes (for framing reference — you don't export each)

You design the components; the pipeline produces all of these:

| Surface | Size | Aspect |
| --- | --- | --- |
| Steam portrait / grid capsule (the "title card") | 600 × 900 | 2:3 |
| Steam hero (logo overlaid) | 1920 × 620 | ≈3.1:1 |
| Steam header / horizontal capsule | 920 × 430 | ≈2.15:1 |
| Steam small capsule | 462 × 174 | ≈2.65:1 |
| Logo (transparent overlay) | ~1280 × 720 canvas | — |
| App icon master | 1024 × 1024 | 1:1 |

The **one** piece worth hand-composing if you want to art-direct it is the
**portrait capsule** — a designer's hand-laid version usually beats an
auto-composite. Hand that one over finished if you like; the pipeline still
auto-produces the rest from the components.

## ⚠️ Copyright — original art only

This project ships publicly (open-source, GPL) and on GitHub Releases, and it
deliberately contains **zero** game or character IP. So the art must be
**original**: no Skylanders characters, no Activision logos or trade dress,
no traced/edited official assets. "Inspired by the genre" (portals, elemental
crystals, toys-to-life vibes) is great; copying Activision's specific
characters/marks is not.

## Handing it off

Drop the files anywhere convenient (a shared folder, a zip, a PR) and we wire
them into the pipeline:

- `logo.svg` (or `logo.png` ≥3000px) → composited onto capsules + the launcher.
- `icon-1024.png` (or `icon.svg`) → generates the `.ico` / `.icns` / web icons +
  the capsule emblem.
- `background.png` (~4000px) → cropped/positioned per aspect ratio.
- `character.png` (optional cutout) → layered into the hero + portrait.

Then it's a single `cargo run -p skylander-brand-bake` to re-bake everything,
and every future tweak is the same one command. Thank you! 💛
