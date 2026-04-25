# Skylander Portal Controller — website

Source for the project's GitHub Pages site at
<https://chotchki.github.io/skylander-portal-controller/>.

A Jekyll site on the `minima` theme with a Skylanders-aesthetic skin
layered on top — starfield background, gold accents, Titan One headings
mirroring the phone UI's design tokens (`phone/assets/app.css`). The
override lives in `assets/main.scss` (imports minima then redefines
colors / typography); the Google Fonts link is in
`_includes/head-custom.html`.

Source-of-truth for the project lives in [`CLAUDE.md`](../../CLAUDE.md),
[`SPEC.md`](../../SPEC.md), [`PLAN.md`](../../PLAN.md), and
[`docs/aesthetic/`](../aesthetic/); these pages are a readable summary for
people evaluating the project, not authoritative docs.

## Local preview

You need Ruby (3.1+) and Bundler. One-time:

```
cd docs
bundle install
```

Then serve. Pass `--baseurl ""` so links resolve at the root of the local
server rather than at `/skylander-portal-controller/`:

```
bundle exec jekyll serve --baseurl ""
```

Opens on <http://127.0.0.1:4000/>. Live-reloads on file save.

## Deployment

GitHub Pages serves this site. The repo has to be configured to point
at this folder — Pages only allows `/` or `/docs` as the source path,
which is why the Jekyll content lives directly under `docs/` rather
than `docs/website/`. The `research/` and `aesthetic/` siblings are
excluded from the Jekyll build via `_config.yml` so they don't end up
on the public site.

One-time setup in the repo's GitHub settings:

1. Repo → **Settings** → **Pages**.
2. **Source:** "Deploy from a branch".
3. **Branch:** `main`, **folder:** `/docs`.
4. Save.

After that, every push to `main` that touches `docs/` triggers a
GitHub Pages build. The public URL
<https://chotchki.github.io/skylander-portal-controller/> picks up the new
content within a minute or two. Build errors show up under the Pages tab
in repo settings and also as repo-level GitHub emails.

> Note: the `baseurl` in `_config.yml` is `/skylander-portal-controller` so
> links resolve correctly under the Pages subpath. Local previews with
> `--baseurl ""` override that at serve time; the committed config is what
> ships.

## Editing

- Top-level pages are Markdown files in this directory (`index.md`,
  `features.md`, `setup.md`, `roadmap.md`, `about.md`). Each has a Jekyll
  front-matter block. Add a new page by dropping a new `.md` file here and
  appending its filename to `minima.nav_pages` in `_config.yml`.
- Tone: honest, technical, solo-developer. No marketing fluff, no emoji.
- Screenshots: placeholders are marked with `{% comment %}TODO{% endcomment %}`
  blocks. When adding real ones, drop the image under `assets/` and link via
  `{{ '/assets/<name>.png' | relative_url }}`.
- Anything substantive about the project's design lives in the repo's
  source-of-truth docs, not here. If a detail on the site starts to diverge
  from those, fix the site.

## What is _not_ in here

- `docs/research/` — research spike writeups. Stays in the repo, not promoted
  to the public site. Worth preserving for our own reference; not polished
  for outside readers.
- `docs/aesthetic/` — UI reference images, design-language notes, and the
  HTML mocks. Leave alone.
