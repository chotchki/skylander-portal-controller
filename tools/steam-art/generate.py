#!/usr/bin/env python3
"""Generate Steam library artwork for the non-Steam shortcut (PLAN 10.8.5).

Steam shows a *blank* library capsule for "Add a Non-Steam Game" because there's
no store artwork. This produces a first-pass set in the project's starfield/gold
aesthetic (docs/aesthetic) so users can apply it via Steam's right-click ->
Manage -> Set Custom Artwork. Reproducible + committed so re-running picks up
icon/branding changes.

Inputs:  assets/branding/icon.ico, crates/server/assets/fonts/TitanOne-Regular.ttf
Outputs: steam/*.png  (portrait/grid capsule, hero, header, logo, icon)

Run from the repo root:  python tools/steam-art/generate.py
Requires Pillow (a dev-only, one-off dependency — not part of the build).
"""

import os
import random
from PIL import Image, ImageDraw, ImageFont, ImageFilter

ROOT = os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
ICON = os.path.join(ROOT, "assets", "branding", "icon.ico")
FONT = os.path.join(ROOT, "crates", "server", "assets", "fonts", "TitanOne-Regular.ttf")
OUT = os.path.join(ROOT, "steam")

# Palette (matches phone/styles/input.css --color-sf-* / --color-gold*).
SF1 = (0x0B, 0x1E, 0x52)   # starfield blue (lighter)
SF2 = (0x06, 0x14, 0x36)   # mid
SF3 = (0x02, 0x08, 0x18)   # deep, near-black
GOLD = (0xF5, 0xC6, 0x34)
GOLD_BRIGHT = (0xFF, 0xE5, 0x8A)
GOLD_MID = (0xC5, 0x8C, 0x18)
WHITE = (0xFF, 0xFF, 0xFF)


def lerp(a, b, t):
    return tuple(int(round(a[i] + (b[i] - a[i]) * t)) for i in range(3))


def vertical_gradient(w, h, stops):
    """stops: list of (pos 0..1, rgb). Vertical top->bottom."""
    img = Image.new("RGB", (w, h))
    px = img.load()
    seg = list(zip(stops, stops[1:]))
    col = []
    for y in range(h):
        t = y / max(1, h - 1)
        c = stops[-1][1]
        for (p0, c0), (p1, c1) in seg:
            if p0 <= t <= p1:
                tt = (t - p0) / max(1e-6, (p1 - p0))
                c = lerp(c0, c1, tt)
                break
        col.append(c)
    for y in range(h):
        c = col[y]
        for x in range(w):
            px[x, y] = c
    return img


def radial_glow(size, center, radius, color, max_alpha):
    """A soft radial alpha glow as an RGBA layer."""
    w, h = size
    layer = Image.new("L", (w, h), 0)
    px = layer.load()
    cx, cy = center
    r2 = radius * radius
    for y in range(h):
        dy = y - cy
        for x in range(w):
            dx = x - cx
            d2 = dx * dx + dy * dy
            if d2 < r2:
                t = 1.0 - (d2 / r2) ** 0.5
                px[x, y] = int(max_alpha * (t ** 1.6))
    rgba = Image.new("RGBA", (w, h), color + (0,))
    rgba.putalpha(layer)
    return rgba


def add_stars(img, count, seed, sizes=(1, 1, 2), gold_ratio=0.18):
    rng = random.Random(seed)
    w, h = img.size
    draw = ImageDraw.Draw(img, "RGBA")
    for _ in range(count):
        x = rng.randint(0, w - 1)
        y = rng.randint(0, h - 1)
        s = rng.choice(sizes)
        a = rng.randint(70, 200)
        col = GOLD_BRIGHT if rng.random() < gold_ratio else WHITE
        draw.ellipse([x, y, x + s, y + s], fill=col + (a,))


def load_icon(target):
    ico = Image.open(ICON)
    # Pick the largest frame.
    best = ico
    try:
        best = max(
            (f.copy() for f in [ico] + [ico.copy()]),
            key=lambda im: im.size[0],
        )
    except Exception:
        pass
    ico = Image.open(ICON)
    ico.size  # noqa
    big = ico
    # Pillow loads the largest by default for .ico via .size; force resize.
    big = ico.convert("RGBA").resize((target, target), Image.LANCZOS)
    return big


def gold_bezel(size):
    """A gold ring with a darker inner well, as an RGBA badge of `size`px."""
    s = size
    ss = s * 4  # supersample for smooth edges
    img = Image.new("RGBA", (ss, ss), (0, 0, 0, 0))
    d = ImageDraw.Draw(img)
    pad = int(ss * 0.04)
    # Outer gold ring.
    d.ellipse([pad, pad, ss - pad, ss - pad], fill=GOLD_MID + (255,))
    d.ellipse([pad, pad, ss - pad, int(ss * 0.5)], fill=GOLD_BRIGHT + (255,))
    inner = int(ss * 0.12)
    d.ellipse([inner, inner, ss - inner, ss - inner], fill=GOLD + (255,))
    well = int(ss * 0.16)
    d.ellipse([well, well, ss - well, ss - well], fill=SF2 + (255,))
    return img.resize((s, s), Image.LANCZOS)


def draw_emblem(base, center, diameter):
    """Gold-bezel ring + glow + icon, composited onto `base` (RGBA)."""
    cx, cy = center
    glow = radial_glow(base.size, center, int(diameter * 0.95), GOLD, 110)
    base.alpha_composite(glow)
    bez = gold_bezel(diameter)
    base.alpha_composite(bez, (cx - diameter // 2, cy - diameter // 2))
    icon_d = int(diameter * 0.66)
    icon = load_icon(icon_d)
    base.alpha_composite(icon, (cx - icon_d // 2, cy - icon_d // 2))


def titan(px):
    return ImageFont.truetype(FONT, px)


def text_centered(base, cx, y, text, px, fill=WHITE, outline=GOLD, ow=None):
    """Centered TitanOne line with a gold outline + soft glow."""
    font = titan(px)
    if ow is None:
        ow = max(2, px // 12)
    draw = ImageDraw.Draw(base)
    bbox = draw.textbbox((0, 0), text, font=font, stroke_width=ow)
    tw = bbox[2] - bbox[0]
    x = cx - tw // 2 - bbox[0]
    # Soft glow underlay.
    glow = Image.new("RGBA", base.size, (0, 0, 0, 0))
    gd = ImageDraw.Draw(glow)
    gd.text((x, y), text, font=font, fill=GOLD_BRIGHT + (140,),
            stroke_width=ow + 3, stroke_fill=GOLD_BRIGHT + (140,))
    glow = glow.filter(ImageFilter.GaussianBlur(px // 10))
    base.alpha_composite(glow)
    draw.text((x, y), text, font=font, fill=fill + (255,),
              stroke_width=ow, stroke_fill=outline + (255,))
    return px + ow * 2  # advance height-ish


def vignette(base, strength=120):
    w, h = base.size
    mask = Image.new("L", (w, h), 0)
    md = ImageDraw.Draw(mask)
    md.ellipse([-w * 0.25, -h * 0.25, w * 1.25, h * 1.25], fill=255)
    mask = mask.filter(ImageFilter.GaussianBlur(min(w, h) // 6))
    dark = Image.new("RGBA", (w, h), SF3 + (strength,))
    inv = Image.eval(mask, lambda v: 255 - v)
    dark.putalpha(inv)
    base.alpha_composite(dark)


def background(w, h, star_seed):
    img = vertical_gradient(w, h, [(0.0, SF3), (0.45, SF2), (1.0, SF1)]).convert("RGBA")
    add_stars(img, int(w * h / 2600), star_seed)
    return img


def make_portrait():
    w, h = 600, 900
    base = background(w, h, 11)
    base.alpha_composite(radial_glow((w, h), (w // 2, int(h * 0.34)), 360, SF1, 90))
    draw_emblem(base, (w // 2, int(h * 0.34)), 360)
    text_centered(base, w // 2, int(h * 0.60), "SKYLANDER", 78)
    text_centered(base, w // 2, int(h * 0.70), "PORTAL", 96, fill=GOLD_BRIGHT)
    text_centered(base, w // 2, int(h * 0.815), "CONTROLLER", 58)
    vignette(base, 110)
    base.convert("RGB").save(os.path.join(OUT, "library_600x900.png"))


def make_header():
    w, h = 920, 430
    base = background(w, h, 23)
    base.alpha_composite(radial_glow((w, h), (int(w * 0.24), h // 2), 300, SF1, 90))
    draw_emblem(base, (int(w * 0.22), h // 2), 300)
    text_centered(base, int(w * 0.63), int(h * 0.30), "SKYLANDER", 56)
    text_centered(base, int(w * 0.63), int(h * 0.46), "PORTAL", 72, fill=GOLD_BRIGHT)
    text_centered(base, int(w * 0.63), int(h * 0.66), "CONTROLLER", 44)
    vignette(base, 90)
    base.convert("RGB").save(os.path.join(OUT, "library_header_920x430.png"))


def make_hero():
    w, h = 1920, 620
    base = background(w, h, 37)
    base.alpha_composite(radial_glow((w, h), (w // 2, int(h * 0.42)), 520, SF1, 80))
    # Big faint emblem watermark, off to one side.
    draw_emblem(base, (int(w * 0.78), int(h * 0.5)), 460)
    vignette(base, 110)
    base.convert("RGB").save(os.path.join(OUT, "library_hero_1920x620.png"))


def make_logo():
    # Transparent wordmark for Steam's logo overlay.
    w, h = 1400, 760
    base = Image.new("RGBA", (w, h), (0, 0, 0, 0))
    draw_emblem(base, (w // 2, int(h * 0.26)), 300)
    text_centered(base, w // 2, int(h * 0.50), "SKYLANDER", 96)
    text_centered(base, w // 2, int(h * 0.63), "PORTAL", 120, fill=GOLD_BRIGHT)
    text_centered(base, w // 2, int(h * 0.80), "CONTROLLER", 72)
    base.save(os.path.join(OUT, "logo.png"))


def make_icon():
    load_icon(256).save(os.path.join(OUT, "icon_256.png"))


def main():
    os.makedirs(OUT, exist_ok=True)
    make_portrait()
    make_header()
    make_hero()
    make_logo()
    make_icon()
    print(f"wrote Steam artwork to {OUT}")


if __name__ == "__main__":
    main()
