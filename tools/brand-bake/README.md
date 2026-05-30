# brand-bake

One-shot, author-machine baker that turns the hand-authored brand SVGs in
`assets/branding/` (PR #3, by Alicea Hotchkiss) into the artifacts the
distribution consumes. Supersedes the old `tools/installer-bake` (icon) and
`tools/steam-art` (Python/Pillow Steam set) — one Rust toolchain (resvg +
tiny-skia + ico + icns), no Python, no brand fonts needed at bake time.

```
cargo run -p skylander-brand-bake                # outline + icon + steam + phone-icons
cargo run -p skylander-brand-bake -- outline     # flatten text → vector paths
cargo run -p skylander-brand-bake -- icon        # icon.ico + icon.icns
cargo run -p skylander-brand-bake -- steam       # steam/*.png
cargo run -p skylander-brand-bake -- phone-icons # phone/assets/icons/icon{,-dev}.svg
```

After `phone-icons`, run `cargo run -p skylander-icon-bake` to rasterise the phone
icon PNGs (favicon / PWA / the launcher window icon).

## Source SVGs (`assets/branding/`)

| File | Role |
| --- | --- |
| `icon-1024.svg` | App-icon master → `.ico` / `.icns` + Steam emblem |
| `logo.svg` | Wordmark → Steam capsules + logo overlay |
| `logo-mono-white.svg`, `logo-mono-dark.svg` | Mono variants (flexibility) |
| `character.svg` | "Portal of Power" cutout → Steam capsule/hero subject |
| `bg-layer-gradient.svg` | Starfield base |
| `bg-layer-stars.svg` | Star field (screened over the gradient) |
| `bg-layer-vortex.svg` | Portal glow (screened over the gradient) |

## Outputs (committed; release pipeline consumes them directly)

- `assets/branding/icon.ico` — Windows multi-res 16/32/48/64/128/256
  (embedded in the exe via `crates/server/build.rs`/winres; MSI shortcut via
  `wix/main.wxs`).
- `assets/branding/icon.icns` — macOS RGBA32 set (`.app` bundle via
  `tools/build-macos-app.sh`).
- `steam/library_600x900.png` · `library_hero_1920x620.png` ·
  `library_header_920x430.png` · `logo.png` · `icon_256.png` — Steam library
  artwork (users apply via right-click → **Manage → Set Custom Artwork**).
- `phone/assets/icons/icon.svg` (prod) + `icon-dev.svg` (Kaos dev variant) —
  emitted from the one emblem master so the phone favicon/PWA icon and the
  desktop launcher window icon stay in sync. `tools/icon-bake` rasterises them.

## The `outline` step

The logo/icon SVGs are authored with live `<text>` in Titan One (the brand
display font, in `crates/server/assets/fonts/`) and Georgia (serif subtitle /
monogram). `outline` parses them through `usvg` with both fonts loaded and
rewrites them with the glyphs flattened to vector `<path>`s, so the committed
SVGs render identically with **no fonts installed** and the `icon`/`steam`
bakes need no font setup. Run it on a machine that has the fonts available
(Titan One ships in-repo; Georgia ships with Windows and macOS). It's
idempotent — re-running on already-outlined SVGs is a no-op.
