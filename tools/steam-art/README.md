# steam-art

One-off generator for the Steam library artwork (PLAN 10.8.5) shipped in
`steam/`. Composes the project's starfield/gold aesthetic from
`assets/branding/icon.ico` + `crates/server/assets/fonts/TitanOne-Regular.ttf`.

```
python -m pip install Pillow      # dev-only, not a build dependency
python tools/steam-art/generate.py
```

Outputs (committed, re-run to refresh after a branding change):

- `steam/library_600x900.png` — portrait/grid capsule (the "title card")
- `steam/library_hero_1920x620.png` — hero banner
- `steam/library_header_920x430.png` — horizontal capsule
- `steam/logo.png` — transparent wordmark
- `steam/icon_256.png` — icon

End users apply these via Steam's right-click → **Manage → Set Custom Artwork**
(Steam can't auto-apply artwork to a non-Steam shortcut). See the README's
"Releases" section.
