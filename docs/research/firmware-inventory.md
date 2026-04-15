# Firmware Pack Inventory — Phase 1c

Indexed pack root: `C:\Users\chris\workspace\Skylanders Characters Pack for RPCS3`

Output: `docs/research/firmware-inventory.json` (one JSON array entry per `.sky` file).

Builder tool: `tools/inventory/` — its own Cargo workspace so it doesn't perturb
the root crate. Re-run with:

```
cargo run --manifest-path tools/inventory/Cargo.toml -- \
    "C:/Users/chris/workspace/Skylanders Characters Pack for RPCS3" \
    docs/research/firmware-inventory.json
```

## Top-line counts

- **Total entries:** 504 (out of 512 `.sky` files; 8 in the duplicate top-level
  `Sidekicks/` folder are skipped per spec).

### By game

| Game             | Count |
|------------------|------:|
| Spyros Adventure |    50 |
| Giants           |    74 |
| Swap Force       |   110 |
| Trap Team        |   147 |
| Superchargers    |    67 |
| Imaginators      |    56 |

### By category

| Category          | Count | Notes                                                                         |
|-------------------|------:|-------------------------------------------------------------------------------|
| figure            |   328 | Element-folder skylanders, including reposes in `Alternate types/`.           |
| item              |    86 | Top-level `Items/<game>/` plus `Trap Team/Traps/<element>/` (see oddities).   |
| sidekick          |    26 | Giants `Sidekicks/` (8) plus Trap Team `Minis/` (18) — see "Minis" oddity.    |
| other             |    27 | `Superchargers/<element>/Vehicle/` files; vehicle isn't in the spec enum.     |
| adventure-pack    |    12 | `Adventure Packs/<game>/`.                                                    |
| giant             |    12 | Giants game's `Giants/` subfolder (Tree Rex, Bouncer, …).                     |
| creation-crystal  |    10 | Imaginators `Creation Crystals/`.                                             |
| kaos              |     3 | Imaginators `Kaos/Kaos.sky` plus 2 Trap Team `Traps/Kaos/*.sky`.              |

### Reposes per game

The "repose" count is `variant_tag != "base"`. This includes:

- Files in any `Alternate types/` folder.
- Files in a base element folder whose name starts with a known variant prefix
  ("Series 2", "LightCore", "Eon's Elite", "Legendary", "Dark", etc.) — Giants
  and Superchargers put a lot of reposes there, not in `Alternate types/`.

| Game             | Reposes |
|------------------|--------:|
| Spyros Adventure |       5 |
| Giants           |      46 |
| Swap Force       |      74 |
| Trap Team        |      25 |
| Superchargers    |      36 |
| Imaginators      |      13 |

Swap Force's high count comes from Top/Bottom swap halves' alternate variants
each being filed twice; the base Top/Bottom pair is **not** counted as a repose
(both are tagged `base (Top)` / `base (Bottom)` and share one `variant_group`).

## ID hash algorithm

**Algorithm:** SHA-256 of `"<game>|<element-or-empty>|<relative-path>"`, hex-encoded,
**truncated to 16 hex chars (64 bits)**.

Why SHA-256:

- It's in the `sha2` crate which we'll already pull in for HMAC signing in
  Phase 1.5; no extra dependency surface.
- Stability matters more than cryptographic strength here (the user explicitly
  said so), and SHA-256 is trivially stable across platforms and Rust versions.
- 64 bits is plenty: with ~500 entries the birthday-collision probability is
  ~6e-15. We can widen later without breaking IDs by re-hashing with a longer
  truncation if a collision ever appears.
- Blake3 would also have been fine; SHA-256 wins on ecosystem familiarity.

The `(game, element, relative_path)` tuple is the stability contract:
- `relative_path` is the canonical path inside the pack root with `/` separators.
- `element` is included even though it's part of the path, so re-classifying
  the element string (e.g. capitalization) regenerates the ID; this is a
  feature — if we change classification, we want IDs to migrate.
- `game` is the human-friendly short name (`"Spyros Adventure"` not the folder
  name `"Skylanders Spyros Adventure"`); same reasoning.

## Classification decisions

A few cases the spec didn't enumerate. All choices documented here so they can
be revisited:

- **Trap Team `Traps/<element>/`** — Traps are elemental items; classified as
  `category: "item"` with `element` set. They're consumable accessories that
  lock to a slot, not figures.
- **Trap Team `Traps/Kaos/`** — `Kaos Trap.sky` and `Ultimate Kaos Trap.sky`
  are classified `category: "kaos"` (matching `Imaginators/Kaos/Kaos.sky`),
  with `element: null`. Kaos isn't an element.
- **Trap Team `Minis/`** — Minis are pint-sized sidekicks of existing
  skylanders, so `category: "sidekick"`. `element` is set to `null` to match
  the Giants `Sidekicks/` folder layout (folder doesn't carry element info).
- **Superchargers `<element>/Vehicle/`** — Vehicles aren't in the spec enum;
  classified as `category: "other"`. They have a real element and behave like
  their own first-class entity. **Recommend Phase 2 add `"vehicle"` to the enum.**
- **Imaginators `Creation Crystals/`** — Element parsed from the all-caps token
  in the filename (`CRYSTAL_-_AIR_Lantern.key.sky` → `Air`). `name` retains the
  raw filename (including the `.key` second-extension) so the original asset is
  unambiguously identifiable; `variant_group` is the cleaned token form.
- **Imaginators `Kaos/Kaos.sky`** — `category: "kaos"`, no element.
- **Giants game's `Giants/` subfolder** — `category: "giant"`, no element. The
  giant skylanders are a distinct figure class with no element folder structure.

### Variant grouping heuristic

`variant_group` collapses reposes onto their base by stripping a curated list
of variant prefixes (`Series 2`, `LightCore`, `Legendary`, `Dark`, `Eon's Elite`,
`Polar`, `Molten`, `Jade`, `Royal`, `Punch`, `Volcanic`, `Hurricane`,
`Power Punch`, `Power Blue`, `Big Bubble`, `Bone Bash`, `Birthday Bash`,
`Super Shot`, `Shark Shooter`, `Deep Dive`, `Double Dare`, `Steel Plated`,
`Missile-Tow`, `Hyper Beam`, `Horn Blast`, `Fire Bone`, `Lava Lance`,
`Lava Barf`, `Knockout`, `Eggcellent`, `Eggcited`, `Frightful`, `Jolly`,
`Scarlet`, `Gnarly`, `Granite`, `Turbo`, `E3`, `Golden`, `Nitro`, `Gold`,
`Platinum`, `Ultimate`) plus parenthetical halves like `(Top)`/`(Bottom)`.

For Imaginators, names use underscores for spaces and a single dash to mark
the variant suffix (`Hoodsickle-Steelplated`, `Golden_Queen-Dark`). The split
recognises a curated tag list (`Dark`, `Legendary`, `Mystical`, `Eggbomber`,
`Steelplated`, `Hardboiled`, `Jinglebell`, `Solarflare`, `Candy-Coated`).

This is **best-effort** grouping. Six entries fall through to the literal
fallback `variant_tag: "Alternate"` (event/seasonal variants whose name is
itself novel: Enchanted Star Strike, Kickoff Countdown, Springtime Trigger
Happy, King Cobra Cadabra, Love Potion Pop Fizz, Winterfest Lob-Star). Their
`variant_group` is the full name, so they won't collapse onto a base. Phase 2
should curate these by hand once we wire in the wiki scrape.

## Oddities

Concrete weirdness in the pack, anything that surprised the spec:

1. **`Skylanders Trap Team/Traps/<element>/...` is a whole second tree** the
   spec didn't anticipate. 64 trap files across 11 sub-buckets (10 elements +
   `Kaos`). The `Kaos` trap subfolder has no element symbol PNG; every other
   trap element folder has one (we ignore it like the figure-side ones).
2. **`Skylanders Trap Team/Minis/`** — 18 mini sidekicks, with their own
   `Alternate types/` containing `Eggcellent Weeruptor` and `Power Punch Pet
   Vac`. Spec mentioned only Giants' `Sidekicks/`.
3. **`Skylanders Superchargers/<element>/Vehicle/`** — every element has a
   `Vehicle/` subfolder (27 vehicles total). Light has a `Legendary Sun Runner`
   directly in `Vehicle/`, not in an `Alternate types/` subfolder. Vehicles are
   elementally typed but mechanically distinct from figures.
4. **Reposes in the base element folder, not in `Alternate types/`** — Giants
   files things like `Series 2 Bash`, `LightCore Eruptor`, `Legendary Chill`
   directly in the element folder. Superchargers does the same with `Eon's
   Elite X`, `Dark X`, `Steel Plated X`. This is why the variant-prefix peel
   list exists; without it those wouldn't group with their bases.
5. **Imaginators uses two completely different naming styles** from the rest of
   the pack: spaces become underscores (`Bad_Juju.sky`, `Golden_Queen.sky`),
   and reposes use a dash suffix instead of a prefix (`Hoodsickle-Steelplated`,
   `Golden_Queen-Dark`, `Tri-Tip-Legendary`). Some files (`Star_Cast`,
   `Crash_Bandicoot`, `Dr_Krankcase`, `Dr_Neocortex`, `Chain_Reaction`) only
   have an underscored base and no repose.
6. **Imaginators `Creation Crystals` use a third naming style** — all-caps with
   underscore-dash-underscore separator and a `.key.sky` double-extension
   (`CRYSTAL_-_AIR_Lantern.key.sky`). The element is embedded in the filename,
   not in the directory.
7. **Unicode in adventure-pack names** — `Adventure Packs/Spyros Adventure/Dragon's Peak.sky`
   uses U+2019 (right single quotation mark), not ASCII apostrophe. Anything
   that compares figure names to wiki names will need NFC/NFKC normalisation.
   `Eon's Elite *` files and `Eon's Elite Voodood.sky` use the same char.
8. **Inconsistent poster casing** — `Skylanders Spyros Adventure/Poster.png`
   (capital P) vs `Skylanders Giants/poster.png` (lowercase). My ignore filter
   is case-insensitive so it's fine, but a strict check would miss one.
9. **One stray non-image, non-symbol PNG** — `Skylanders Giants/Giants/Skylanders-Giants-Logo.png`.
   Hard-coded to ignore by exact filename.
10. **Spelling typos preserved in filenames** — `Drill Seargeant.sky` (sic),
    `Drobit.sky` (Trap Team Mini for Drobot — intentional but confusing). We
    expose these verbatim in the JSON so the file load works; we'll need a
    display-name remap layer in Phase 2.
11. **Cross-game name reuse** — `Giants/Tech/Sprocket.sky` exists, and the same
    figure also appears in later games' element folders. IDs are unique because
    they include game + relative path, but variant_group will collide. Not a
    bug today; flag it for the Phase 2 collection-merge logic.
12. **Adventure-Pack `Imaginators` only has 2 files** (`Enchanted Elven Forest`,
    `Gryphon Park Observatory`) — much less than the other games. Unsure if the
    pack is incomplete or if Imaginators only ever shipped two adventure packs.
    Worth verifying against the wiki in Phase 1d.

## Known unknowns / Phase 2 action items

- **Add `"vehicle"` to the category enum.** 27 entries currently bucketed as
  `"other"` should be first-class.
- **Add `"trap"` (or keep "item")?** Decide whether traps are distinct enough
  from items to deserve their own category (they probably are: they have an
  element, items mostly don't).
- **Decide canonical handling of swap halves.** Right now Top/Bottom share a
  `variant_group` and both are `base (Top)` / `base (Bottom)`. A real portal
  load uses both halves combined; we'll need to know if the working-copy model
  copies one file per half or pairs them.
- **Curate the 6 fallback `"Alternate"` tags** when wiki data lands. They
  should map to known seasonal/event lines.
- **Confirm Adventure Pack Imaginators completeness** with the wiki.
- **Verify cross-game figure-name reuse** and design how the collection view
  collapses across games. The spec says "figure file shared across games (one
  working copy per profile+figure)" — so we need a stable cross-game key. SHA
  of the file contents is the obvious answer; current path-based ID won't dedupe.
- **Stable cross-game identity:** the path-based ID will change if the user
  reorganises the pack. We may want a content-hash supplemental ID for
  detecting "same file moved" situations.
- **Verify file-extension assumption.** Every payload here is `.sky`. SPEC.md
  notes "firmware file extension is `.sky`, not `.dump`" — confirmed: zero
  `.dump` files in the pack.
