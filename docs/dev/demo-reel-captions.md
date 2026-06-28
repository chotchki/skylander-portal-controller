# Demo-reel captions — the script (iterate here)

This file is the **caption script** for the two demo videos (PLAN A.7 + A.8). It's the
source of truth for the narration copy; once a `FINAL:` line reads the way you want, it
gets transcribed into the `Beat::caption` field of the matching `beat_*()` constructor in
`tools/playthrough/src/beats.rs`. (The manifest's caption is per-capture, so `beats.rs` is
where it lives durably — same bake-back rule as the speed numbers.)

**Workflow:** edit the `FINAL:` lines below → ping me → I sync them into `beats.rs` → next
capture/render burns them in (centred lower-third, timed to each beat's on-screen window).

**Status (2026-06-28):** the Hook captions below are **transcribed into `beats.rs`** (+ a new
`title` beat) — the Hook is ready to re-capture. Decisions locked:
- **Captions word-wrap** (`caption.rs`) — long lines stack to 2 lines, no clipping. Width no
  longer constrains the copy (keep it to ~2 lines; ~3 reads as a paragraph).
- **`install` = static title card**, not a driven wizard (the egui wizard isn't phone-drivable;
  needs a fresh-config capture path, built in A.8.5).
- Tour: `pick_figure` (stats) **moved after `place_figure`** (the working copy must exist first);
  `resume` + `repose` **dropped** (fluff). Consolidated order under Video 2.

## Voice rules (the bar these are held to)

Captions are audience-facing content → **full voice including the tics**, just inside a
caption-length budget. Quick checklist:

- **Density, verdict/action first.** Lead with what happens; cut the windup. Every word earns its place.
- **One ALL-CAPS word per beat** (the load-bearing one — not bold, not italic).
- **Parentheticals** for the aside / caveat / the-point (the signature move).
- **No Oxford commas.** Comma-splices + casual register are fine; spelling stays clean.
- **No marketing tells:** seamless / powerful / easily / effortless / unlock / leverage — gone. No hype `!`, no rule-of-three, no "no X, no Y, just Z", no crafted-maxim closers.
- **Correctness:** the app is **tap-to-add**, NOT drag-drop. Canonical figure names only.

---

## Video 1 — the Hook (`ingame`, real RPCS3)

The ~30-60s marquee. Eight beats (a `title` card + seven), in narrative order. **Transcribed
into `beats.rs` as of 2026-06-28** — edit here + ping me to re-sync. Shared spine; Video 2
reuses them unless a beat gets a Tour-specific variant (see below).

### `title`
_viewer sees: the TV shows the QR coin (this should be the start, no fade in)_
- **FINAL:** `Skylander Portal Controller - your device is the portal.`

### `connect`
_viewer sees: the TV shows the QR coin, the phone scans it, the profile picker mounts._
- current: `Scan the code. Your phone's the portal now.`
- proposed: `Scan the code — your phone IS the portal (no app to install).`
- **FINAL:** `You start by scanning the code on screen (NO APP to install).`

### `pick_profile`
_viewer sees: the profile picker ("PORTAL MASTER"), a profile gets tapped + unlocked._
- current: `Welcome back, Portal Master.`
- proposed: `Pick your profile — every kid gets their OWN figures (and their own PIN).`
- **FINAL:** `Pick your profile — every kid gets their OWN figures (and their own PIN).`

### `pick_game`
_viewer sees: the game picker, Giants gets tapped, the real emulator cold-boots on the TV (the boot is sped up a lot in the cut, I've chewed on a slight blur but we'll see if it needs it)._
- current: `Pick a game — it boots on the TV.`
- proposed: `Pick a game — it BOOTS the real emulator on the TV (no disc, no menu-fishing).`
- **FINAL:** `Pick your archived game — it BOOTS the real emulator on the TV (no disc, no menu-fishing).`

### `open_toybox`
_viewer sees: the toy box drawer opens, the figure collection grid fills in (we'll want to slow down and pause a little for each tag/click)._
- current: `Your collection, digital.`
- proposed: `Open the toy box — the family's WHOLE collection, no shelf-digging.`
- **FINAL:** `Open the toy box — the family's WHOLE collection, no shelf-digging.`

### `place_figure`
_viewer sees: a figure card gets tapped → PLACE → it loads onto the portal (real IPC)._
- current: `Tap a figure onto the portal.`
- proposed: `Tap a figure — it's ON the portal, loaded into the live game.`
- **FINAL:** `Tap a figure — it's ON the portal, loaded into the live game.`

### `see_in_game`
_viewer sees: the figure LANDS on RPCS3's own in-game portal — the climax._
- current: `It's in the game. No toy touched.`
- proposed: `It's IN the game. No toy touched.` _(yours already — just CAPS'd `IN`)_
- **FINAL:** `It's IN the game. No toy touched.`

### `kaos`
_viewer sees: a real Kaos swap fires — a portal figure is swapped for a compatible one, taunt overlay shows. (fade out)_
- current: `Beware, Kaos can strike anytime!`
- proposed: `Then Kaos STRIKES — a figure swaps mid-game (chaos, on purpose).`
- **FINAL:** `Then Kaos STRIKES — a figure swaps mid-game (optional chaos, on purpose).`

---

## Video 2 — the Tour (`walkthrough`, real RPCS3)

The comprehensive feature walk. It **reuses the Hook spine** above and inserts the feature
detours below. Two parts to iterate:

**Consolidated order (locked 2026-06-28):**
`title → install (static card) → connect → create_profile → pick_profile → pick_game (boot) → open_toybox → search → filters → appearance_swap → place_figure → see_in_game → pick_figure (stats, now real) → remove → ownership → join_qr → konami_admin → kaos → farewell`
- `pick_figure` (stats) sits **after** `place_figure` so the working copy exists + the numbers are real (your call).
- `resume` + `repose` **dropped** (fluff).
- `title` + `install` (static card) open the Tour before `connect`.

### Spine beats — Tour-specific variants?

The Hook is punchy; the Tour can afford a half-beat more explanation. For each shared beat,
leave the variant BLANK to reuse the Hook caption as-is, or write a Tour-only line. (A
non-blank variant means a separate beat constructor / caption override in `beats.rs`.)

- `connect` → **TOUR:** (blank = reuse Hook)
- `pick_profile` → **TOUR:** (blank = reuse Hook)
- `pick_game` → **TOUR:** (blank = reuse Hook)
- `place_figure` → **TOUR:** (blank = reuse Hook)
- `see_in_game` → **TOUR:** (blank = reuse Hook)
- `kaos` → **TOUR:** (blank = reuse Hook)

### New Tour beats (PROVISIONAL — exact names/order land in A.8.1)

These beats don't exist yet; I'll author the drive fns in A.8.1. Drafting their captions now
is fine — the names may shift slightly when the code lands. Slots are empty for you to fill;
proposals are starting points, not commitments.

### `title`
_viewer sees: the TV the first step of the first run wizard_
- **FINAL:** `Skylander Portal Controller - your device is the portal. (the TOUR)`

### `install`
_viewer sees: a STATIC held shot of the first-run wizard (no field-filling — the egui wizard isn't phone-drivable; needs a fresh-config capture path, A.8.5)._
- **FINAL:** `First, point us at your RPCS3 install + dumped games (an NFC scanner's optional).`

### `connect`

#### `create_profile`
_viewer sees: the "+ ADD" wizard — name (reroll), colour swatch, choose + confirm a PIN._
- proposed: `New player? Name, a colour, a PIN — they're in (4 profiles, one per kid).`
- **FINAL:** `New player? Name, a colour, a PIN — they're in (4 profiles, one per kid).`

### `open_toybox`
_viewer sees: the toy box drawer opens, the figure collection grid fills in (we'll want to slow down and pause a little for each tag/click)._
- **FINAL:** `This is your collection! Everyone gets their own copy!`

#### `search`
_viewer sees: the toy box search field filters the grid live as a name is typed._
- proposed: `Type a name — the whole collection filters as you go (reposes and all).`
- **FINAL:** `Type a name — the whole collection filters as you go (reposes and all).`

#### `filters`
_viewer sees: the GAMES / ELEMENTS / CATEGORY chip rows narrow the grid._
- proposed: `Filter by game, element OR type — find the one figure in a pile of 300.`
- **FINAL:** `Filter by game, element OR type — find the one figure in a pile of 300.`

#### `pick_figure`
_viewer sees: a specific figure (probably Spyro selected)
- **FINAL:** `Most stats load from the figure when you loaded it.`

#### `appearance_swap`
_viewer sees: in figure detail, APPEARANCE swaps to the next variant._
- proposed: `Swap the look right from the figure — APPEARANCE flips between variants.`
- **FINAL:** `Swap the look right from the figure — APPEARANCE flips between variants.`

#### `remove`
_viewer sees: a loaded slot is tapped → REMOVE → the slot clears._
- proposed: `Done with a figure? Tap the slot, REMOVE — off the portal it goes.`
- **FINAL:** `A Skylander too tired? Tap the slot, REMOVE — off the portal it goes.`

#### `ownership`
_viewer sees: a second player (Bob) places a figure; the slot shows his owner pip._
- proposed: `Two players, one portal — each slot shows WHOSE figure it is.`
- **FINAL:** `Two players, one portal — each slot shows WHOSE figure it is.`

#### `join_qr`
_viewer sees: the kebab menu's "INVITE A PLAYER" join-QR card._
- proposed: `Hand off the game — show the join code, a second phone hops IN.`
- **FINAL:** `Hand off the game — show the join code, a second phone hops IN.`

#### `konami_admin`
_viewer sees: the Konami gate ("grown-ups only"), then the admin hub (delete, PIN-reset, window-mode + 2× toggles)._
- proposed: `Grown-up menu, tucked behind a tap combo (the kids never find it).`
- **FINAL:** `Admin menu, requires some knowledge of gaming.`

#### `farewell`
_viewer sees: the app shutdown screen with fade to black_
- **FINAL:** `May this keep your love of Skylanders alive.`

---

## Notes / open questions (jot here)

- (chotchki — leave anything for me here: beats to drop, captions that feel off, order tweaks)
