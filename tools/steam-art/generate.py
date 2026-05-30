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
GOLD_DEEP = (0x3A, 0x25, 0x00)   # dark outline so gold-on-gold text keeps edges
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
    """Load the app .ico (Pillow picks its largest frame) as an RGBA square of
    `target` px."""
    ico = Image.open(ICON)
    ico.size  # force the .ico loader to select the largest frame
    return ico.convert("RGBA").resize((target, target), Image.LANCZOS)


def place_icon(base, center, size, opacity=255, glow_alpha=130, blur=0):
    """Composite the app icon (rounded-square portal) with a soft gold halo
    behind it. No synthetic bezel — the icon already carries the design, and a
    circular gold ring around a square icon read as a sticker (the hero-layering
    complaint). The halo separates the blue icon from the blue background."""
    cx, cy = center
    if glow_alpha:
        # Gold halo gives the blue icon a bright rim so it doesn't melt into the
        # starfield. Sized a touch larger than the icon.
        base.alpha_composite(radial_glow(base.size, center, int(size * 0.78),
                                         GOLD, glow_alpha))
    icon = load_icon(size)
    if blur:
        icon = icon.filter(ImageFilter.GaussianBlur(blur))
    if opacity < 255:
        a = icon.split()[3].point(lambda v: int(v * opacity / 255))
        icon.putalpha(a)
    base.alpha_composite(icon, (cx - size // 2, cy - size // 2))


def titan(px):
    return ImageFont.truetype(FONT, px)


def text_centered(base, cx, y, text, px, fill=WHITE, outline=GOLD, ow=None):
    """Centered TitanOne line: a blurred near-black drop shadow for contrast on
    any background, then the fill with a crisp outline. The shadow (not a bright
    glow) is what makes white/gold text readable over the starfield."""
    font = titan(px)
    if ow is None:
        ow = max(3, px // 9)
    draw = ImageDraw.Draw(base)
    bbox = draw.textbbox((0, 0), text, font=font, stroke_width=ow)
    tw = bbox[2] - bbox[0]
    x = cx - tw // 2 - bbox[0]
    off = max(3, px // 20)
    # Dark drop shadow (blurred) underneath — grounds the text on bright areas.
    sh = Image.new("RGBA", base.size, (0, 0, 0, 0))
    ImageDraw.Draw(sh).text(
        (x + off, y + off), text, font=font,
        fill=(0, 0, 0, 235), stroke_width=ow, stroke_fill=(0, 0, 0, 235),
    )
    sh = sh.filter(ImageFilter.GaussianBlur(max(2, px // 14)))
    base.alpha_composite(sh)
    draw.text((x, y), text, font=font, fill=fill + (255,),
              stroke_width=ow, stroke_fill=outline + (255,))


def bottom_scrim(base, start_frac=0.46, end_alpha=215):
    """Darken the lower portion (where the title sits) with a transparent->SF3
    vertical gradient, so the text block always has a dark base to read against."""
    w, h = base.size
    layer = Image.new("RGBA", (w, h), (0, 0, 0, 0))
    px = layer.load()
    y0 = int(h * start_frac)
    for y in range(y0, h):
        t = (y - y0) / max(1, h - y0)
        a = int(end_alpha * (t ** 1.25))
        row = SF3 + (a,)
        for x in range(w):
            px[x, y] = row
    base.alpha_composite(layer)


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
    base.alpha_composite(radial_glow((w, h), (w // 2, int(h * 0.30)), 320, SF1, 80))
    place_icon(base, (w // 2, int(h * 0.30)), 300)
    # Dark scrim under the title block → high text contrast.
    bottom_scrim(base, start_frac=0.50, end_alpha=225)
    text_centered(base, w // 2, int(h * 0.605), "SKYLANDER", 78)
    text_centered(base, w // 2, int(h * 0.705), "PORTAL", 100,
                  fill=GOLD_BRIGHT, outline=GOLD_DEEP)
    text_centered(base, w // 2, int(h * 0.825), "CONTROLLER", 56)
    vignette(base, 100)
    base.convert("RGB").save(os.path.join(OUT, "library_600x900.png"))


def make_header():
    w, h = 920, 430
    base = background(w, h, 23)
    base.alpha_composite(radial_glow((w, h), (int(w * 0.22), h // 2), 280, SF1, 80))
    place_icon(base, (int(w * 0.22), h // 2), 260)
    text_centered(base, int(w * 0.62), int(h * 0.31), "SKYLANDER", 54)
    text_centered(base, int(w * 0.62), int(h * 0.47), "PORTAL", 72,
                  fill=GOLD_BRIGHT, outline=GOLD_DEEP)
    text_centered(base, int(w * 0.62), int(h * 0.66), "CONTROLLER", 42)
    vignette(base, 90)
    base.convert("RGB").save(os.path.join(OUT, "library_header_920x430.png"))


def make_hero():
    w, h = 1920, 620
    base = background(w, h, 37)
    base.alpha_composite(radial_glow((w, h), (int(w * 0.72), int(h * 0.45)), 620, SF1, 95))
    # Faint, slightly-blurred icon watermark — atmospheric background element,
    # not a hard sticker, so Steam's logo overlay layers cleanly on top.
    place_icon(base, (int(w * 0.72), int(h * 0.46)), 520,
               opacity=70, glow_alpha=70, blur=2)
    vignette(base, 120)
    base.convert("RGB").save(os.path.join(OUT, "library_hero_1920x620.png"))


def make_logo():
    # Transparent wordmark for Steam's logo overlay (sits on the dark hero).
    w, h = 1400, 760
    base = Image.new("RGBA", (w, h), (0, 0, 0, 0))
    place_icon(base, (w // 2, int(h * 0.24)), 280, glow_alpha=90)
    text_centered(base, w // 2, int(h * 0.49), "SKYLANDER", 96)
    text_centered(base, w // 2, int(h * 0.62), "PORTAL", 122,
                  fill=GOLD_BRIGHT, outline=GOLD_DEEP)
    text_centered(base, w // 2, int(h * 0.79), "CONTROLLER", 72)
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
