# Skylanders Wiki Scraper

One-shot CLI that populates `data/figures.json` + `data/images/<figure_id>/*.png`
+ `data/games/<serial>.png` from the [Skylanders Fandom wiki][wiki]. Its
output is **committed to the repo**; the runtime never touches the wiki.

## Running

```powershell
# Full scrape (~30–45 min at 1 req/sec rate limit)
cargo run --release -p skylander-wiki-scrape

# Smoke test against 5 entries
cargo run -p skylander-wiki-scrape -- --limit 5

# Force re-scrape (ignore the cached figures.json)
cargo run --release -p skylander-wiki-scrape -- --force

# Skip the hero.png download (thumbs are usually all we need — see LICENSE.md)
cargo run --release -p skylander-wiki-scrape -- --no-hero

# Scrape game box art for the phone's game picker cards (6 entries, fast).
# Saves to data/games/<PS3_SERIAL>.png at ~600px on the long side.
cargo run -p skylander-wiki-scrape -- --mode boxart
cargo run -p skylander-wiki-scrape -- --mode boxart --force
```

Reads `docs/research/firmware-inventory.json`, writes under `data/`:

- `figures.json` — array of `WikiFigure` records keyed by `figure_id`
- `figures.manual.json` — empty-object template; fill in to override the scrape
- `images/<figure_id>/thumb.png` — 128×128 center-cropped PNG
- `images/<figure_id>/hero.png` — downsized infobox portrait (~320px wide)
- `games/<PS3_SERIAL>.png` — game box art (~600px on the long side), written
  by the `--mode boxart` sub-run. Six entries today, one per supported game.
- `LICENSE.md` — CC BY-SA + trademark attribution

## Resume semantics

Re-running without `--force` preserves any entry whose cached record has
`wiki_page: Some` **and** whose `hero.png` already exists on disk. Entries
without a match (`wiki_page: None`) are retried on every run so future
improvements to the name-resolution heuristics pick them up for free.

To retry a specific failed entry, either delete its cache row from
`figures.json` or run `--force`.

## Hit-rate target

SPEC R3 requires ≥80% auto-match. Anything below that goes into
`figures.manual.json` for hand curation — the server merges manual over
scraped at load time so curation always wins.

## Licensing

Text (CC BY-SA 3.0) + low-res identification images (fair-use, owner-must-
have-physical-figure). See `data/LICENSE.md`.

[wiki]: https://skylanders.fandom.com/
