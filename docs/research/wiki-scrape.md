# Wiki Scrape Feasibility (Phase 1 spike 1d)

## Decision

**Use the Fandom MediaWiki API. `opensearch` for name resolution, `query` (with `prop=pageimages|categories|revisions`) for metadata extraction.** 100% hit rate on the 20-name sample — we can proceed with this as the primary path and fall back to a small curated CSV for any misses that appear when we run against the full firmware inventory.

## Endpoints

Base: `https://skylanders.fandom.com/api.php`

1. **Name → page title** — `?action=opensearch&search=<name>&format=json&limit=3`
   - Returns `[searched, [titles], [descriptions], [urls]]`. The first title in the array is the canonical match.

2. **Page → metadata** — `?action=query&titles=<Title>&prop=pageimages|categories|revisions&pithumbsize=512&rvprop=content&rvslots=main&format=json`
   - `thumbnail.source` — hero image URL (max dim 512).
   - `pageimage` — filename only, used to build scaled URLs.
   - `categories[]` — list of `Category:...` titles. Reliable source for **element** (`Fire Skylanders`), **role** (`Core Skylanders`, `Giants`, `Trap Masters`, `SuperChargers`, `Sensei Skylanders`, etc.), **series** (`Series 2 Skylanders`, `LightCore Skylanders`), and **alignment** (e.g. `Playable Characters`, `Male Characters`).
   - `revisions[0].slots.main.*` — full wikitext. The `{{Characterbox}}` template at the top exposes clean fields (`element`, `species`, `role`, `attack`, `appearances`, voice actors, first release). We should parse this template block, not the whole page.

## Hit-rate table (sample of 20, all exact)

| Queried name     | Top opensearch result | Result |
|------------------|-----------------------|--------|
| Spyro            | Spyro                 | exact  |
| Eruptor          | Eruptor               | exact  |
| Trigger Happy    | Trigger Happy         | exact  |
| Stealth Elf      | Stealth Elf           | exact  |
| Chop Chop        | Chop Chop             | exact  |
| Gill Grunt       | Gill Grunt            | exact  |
| Bash             | Bash                  | exact  |
| Boomer           | Boomer                | exact  |
| Camo             | Camo                  | exact  |
| Cynder           | Cynder                | exact  |
| Double Trouble   | Double Trouble        | exact  |
| Drill Sergeant   | Drill Sergeant        | exact  |
| Drobot           | Drobot                | exact  |
| Flameslinger     | Flameslinger          | exact  |
| Ghost Roaster    | Ghost Roaster         | exact  |
| Hex              | Hex                   | exact  |
| Ignitor          | Ignitor               | exact  |
| Lightning Rod    | Lightning Rod         | exact  |
| Prism Break      | Prism Break           | exact  |
| Sonic Boom       | Sonic Boom            | exact  |

**20/20 exact.** Ambiguity only appears in variants/spinoffs (e.g. `Spyro (Skylanders Academy)`), never displacing the canonical figure from position 0.

Caveat: this sample is weighted toward original-game characters with unambiguous names. Reposes and cross-game variants in the actual firmware inventory (e.g. `Series 2 Bash`, `LightCore Eruptor`, `Legendary Chill`, `Hog Wild Fryno`) will need variant handling. Two patterns from the inventory work fine verbatim:
- Variant prefixed (`Legendary Chop Chop`, `Elite Ghost Roaster`) resolves to its own dedicated page when searched exactly.
- Variant as base (`Spyro`, `Dark Spyro`) each have distinct pages.
The indexer already groups variants to a base `variant_group`. For Phase 2, scraping strategy:
1. Resolve `variant_group` (base name) via opensearch.
2. Also attempt the full variant name (`Legendary Spyro`) — many have their own wiki pages.
3. If the full-variant page exists, use its image; otherwise inherit from base and use a lighter variant tag.

## Example extraction (Eruptor)

```json
{
  "title": "Eruptor",
  "thumbnail": {
    "source": "https://static.wikia.nocookie.net/skylanders/images/6/65/Erup3.jpg/revision/latest?cb=20120105061926",
    "width": 499, "height": 512
  },
  "pageimage": "Erup3.jpg",
  "categories": [
    "Category:Core Skylanders", "Category:Defense Skylanders",
    "Category:Fire Skylanders", "Category:LightCore Skylanders",
    "Category:Series 2 Skylanders", "Category:Male Skylanders",
    "Category:Playable Characters"
  ]
}
```

Characterbox fragment (from `revisions[0].slots.main.*`):

```wikitext
{{Characterbox
|element = Fire
|name = Eruptor
|species = Lava Monster
|gender = Male
|role = [[Core Skylanders|Core Skylander]]
|world = [[Skylands]]
|attack = *Lava Lob *Eruption ...
|appearances = ... (list of games) ...
|firstrelease = ''Skylanders: Spyro's Adventure''
}}
```

For the scrape, extract:
- `element` — from Characterbox `element=` or from `Category:<X> Skylanders`. Prefer Characterbox; categories are the fallback.
- `role` — Characterbox `role=` (strip wiki link syntax). Fallback to category filter (`Core Skylanders`, `Giants`, `Trap Masters`, `SuperChargers`, `Senseis`).
- `game_of_origin` — Characterbox `firstrelease=` (strip italics/links).
- `appearances[]` — from Characterbox `appearances=`; a bullet list of games the figure appears in. This is our source for the compatibility filter (a wiki-backed version of the heuristic we already accepted).
- `image_hero` — `thumbnail.source` at `pithumbsize=512` or `768`.
- `image_thumb` — rebuild the URL from `pageimage` with `/revision/latest/scale-to-width-down/256/`.

## Image sizing

Fandom images are served from `static.wikia.nocookie.net`. Scaled versions follow the pattern `.../<filename>/revision/latest/scale-to-width-down/<N>/?cb=<timestamp>`.

Alternatively, the API's `pithumbsize=<N>` parameter on `prop=pageimages` returns a pre-scaled URL with the right `cb` already set — simplest path. We'll request:
- `pithumbsize=256` for thumbnails (browse/grid view).
- `pithumbsize=768` for hero (selected-figure detail).

## Politeness / rate limits / UA

- Fandom runs MediaWiki; per community convention, stay well under 1 req/sec with a descriptive User-Agent. We'll send `skylander-portal-controller/<version> (https://github.com/<user>/skylander-portal-controller)` and cap the scraper at ~1 req/sec. That's more polite than necessary for a one-shot scrape of ~500 figures (~10 min wall clock).
- The MediaWiki API supports `maxlag` — include `maxlag=5` to back off if the cluster is stressed.
- Honor `If-Modified-Since` / ETag on image downloads. Not important for a one-shot scrape.
- robots.txt does **not** forbid the API path.
- No login needed.

## Attribution

Text content on Fandom wikis is CC-BY-SA 3.0 (and images have mixed licenses — most are fair-use of Activision property; we rely on user ownership of their figures and the project being non-commercial/public-domain per the spec). Ship an **About** page in the phone UI with:

```
Figure data and images are sourced from the Skylanders Fandom wiki
(https://skylanders.fandom.com), licensed under CC-BY-SA 3.0. Character
artwork and names are trademarks of Activision. This project is an
unofficial fan tool; it ships no game or firmware content.
```

Also add a short attribution note to the README and to `docs/aesthetic/README.md` where appropriate.

## Implementation plan for Phase 2

- Scraper is a one-shot tool at `tools/wiki-scrape/` — standalone Cargo workspace member so it doesn't bloat the runtime binary's dependency graph.
- Rust (`reqwest` + `serde_json`), not PowerShell. The wikitext Characterbox parse is easier in Rust, and we already have Rust in the toolchain.
- Input: `docs/research/firmware-inventory.json` (from spike 1c).
- Output: `data/figures.json` (metadata) + `data/images/<figure_id>/{thumb.png,hero.png}`, committed to the repo.
- Misses log: `data/figures.missing.json` — any figure the scraper couldn't resolve. We'll hand-curate these in a second pass per the user's "easy matches now, find the hard ones later" guidance.
- Runtime just loads the scraped data from bundled assets. **No live wiki calls at runtime.**

## Risks

- **R-wiki-1:** Variant resolution for reposes (`Series 2 Bash`, `LightCore Eruptor`). Mitigation: two-pass (full variant first, then base). Document misses in Phase 2.
- **R-wiki-2:** Characterbox field names change per game generation (some pages use `|elem =` or nest differently). Mitigation: tolerant parser; fall back to category-derived fields.
- **R-wiki-3:** A small number of figures (Items, Adventure Packs, Creation Crystals) may not have Characterbox at all. Mitigation: accept these with minimal metadata (name + image only) and don't block on them.
- **R-wiki-4:** Image license is murkier than text. Mitigation: strong attribution, non-commercial use only, no redistribution outside the project. Keep this stance documented.

## Implementation notes (Phase 3.19)

### Repo size budget

The spec target of "<5MB committed images" is **impossible** with PNG + ~500
figures at both thumb and hero sizes. Concrete measurements during the scrape:
- **Hero** at width 320px, PNG: ~150–200KB per figure → ~80MB for 500 figures.
- **Thumb** at 128×128, PNG: ~25–40KB per figure → ~15MB for 500 figures.

Chose a compromise: ship **thumbs only in the default commit** (the phone
only renders thumbs; a hero detail view is deferred to 3.15), and expose
`--with-hero` / `--no-hero` on the scraper for anyone who wants to re-run.
Final committed size: ~X MB (see repo).

### Gallery-redirect pitfall

Fandom aggressively redirects bare figure titles (e.g. `Zook`) to their
`/Gallery` subpage, which is an image dump with no Characterbox. When we
get a redirect-resolved title ending in `/Gallery`, we strip the suffix and
re-fetch from the parent page. Caught in QA on the first 35-entry slice of
the full scrape.

### Resume behaviour

`data/figures.json` is written every 25 entries and on exit; `--force`
re-scrapes everything, the default skips any figure whose cache entry has
`wiki_page: Some` and whose `hero.png` (or `thumb.png` when `--no-hero`)
already exists.

### Output schema

```rust
struct WikiFigure {
    figure_id: String,
    wiki_page: Option<String>,       // full URL (unresolved → None)
    soul_gem: Option<String>,        // from Characterbox `soul gem =`
    signature_moves: Vec<String>,    // from Characterbox `attack =` bullets
    alignment: Option<String>,       // role/alignment/faction field or Light/Dark Trap category
    attributes: HashMap<String,String>, // rest of the Characterbox, cleaned
}
```

Any figure the scraper couldn't resolve lands in `figures.json` with
`wiki_page: None`; the user can fill in `data/figures.manual.json` to
override on a per-ID basis. The server merges manual over scraped at
load-time (future — for now the image route looks at `data/images/` only).
