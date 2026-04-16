# Phase 4 — Aesthetic direction mockups

Three standalone HTML files exploring different visual directions for the phone app. Open each file directly in a browser — no build, no JS framework, no server. A phone-shaped frame renders on desktop; on a phone-sized viewport they go full-bleed.

All three are the **portal view** with the same slot composition (loaded, loading, picking, empty, errored, ownership dots) so they're directly comparable.

## The three directions

### A. Heraldic (`option_a_heraldic.html`)
The "faithful" option. Thick embossed gold bezels with multi-layer shading, starfield background, chunky display type with gold stroke + drop-shadow. Ornate plaque with filigree corners for the recent/browse section. Closest to the `ui_style_example.png` reference. Maximalist and the most "Saturday-morning-cartoon" of the three.

**Feel:** loud, decorative, unambiguously Skylanders.
**Risk:** busy on small phones; tap targets compete with ornamentation.

### B. Portal Arcane (`option_b_arcane.html`)
Hex-shaped bezels echoing the in-game portal tile floor, runic display font, purple-magenta mixed with gold. Glowing energy conduits between loaded slots. Draws from `kaos_lair_feel.png` (minus the purple evil skin — that's the 5.4 Kaos palette swap). More mystical / ritual-circle than plaque-and-parchment.

**Feel:** magical, ritualistic, slightly older-skewing.
**Risk:** hexes tessellate awkwardly on a 4-wide grid; kids may read it as "grown-up."

### C. Modernized (`option_c_modernized.html`)
Clean 2×4 grid with larger tap targets. Thin gold rings (2-3px, not embossed), generous spacing, typography does the heavy lifting. Keeps the gold-circle portrait motif and starfield but removes the ornamentation. Reads closer to a contemporary mobile game UI that *references* Skylanders rather than reconstructs it.

**Feel:** calm, ergonomic, age-neutral.
**Risk:** might read as too plain / insufficiently on-brand.

## Transitions demo

`transitions.html` — a single slot cycling through Empty → Picking → Loading → Loaded → Cleared, with playback controls and timing readouts. Uses Option A's visual language since it has the strongest baseline to animate against.

## What to decide

1. **Which direction** (or a blend — "A's bezel on C's layout", etc.).
2. **How loud the transitions feel** — the demo runs the "theatrical" version; dialing down the portal-impact flash or the halo rotation is a one-line change.
3. **Whether the browse surface is a peek plaque** (like A's "RECENT" strip) **or a separate screen** (pressed in C).

None of these are final — the idea is to see the directions next to each other and pick which conversation to have next.
