//! Brand-asset baker — see Cargo.toml for the full rationale.
//!
//! Usage (from repo root):
//!     cargo run -p skylander-brand-bake -- outline   # flatten text -> paths
//!     cargo run -p skylander-brand-bake -- icon       # icon.ico + icon.icns
//!     cargo run -p skylander-brand-bake -- steam       # steam/*.png
//!     cargo run -p skylander-brand-bake -- phone-icons # phone/assets/icons/icon{,-dev}.svg
//!     cargo run -p skylander-brand-bake -- all
//!     cargo run -p skylander-brand-bake -- debug-png <in.svg> <out.png> <w> <h>

use std::path::{Path, PathBuf};

use anyhow::{Context, Result, anyhow};
use icns::{IconFamily, IconType, Image as IcnsImage, PixelFormat};
use ico::{IconDir, IconDirEntry, IconImage, ResourceType};
use resvg::{
    tiny_skia::{
        BlendMode, Color, FilterQuality, GradientStop, LinearGradient, Paint, Pixmap, PixmapPaint,
        Point, Rect, SpreadMode, Transform,
    },
    usvg::{Options, Tree, WriteOptions},
};

/// Text-bearing SVGs that must be flattened to paths so they render
/// identically without the brand fonts installed.
const TEXT_SVGS: &[&str] = &[
    "logo.svg",
    "logo-mono-white.svg",
    "logo-mono-dark.svg",
    "icon-1024.svg",
];

/// Windows multi-res `.ico` pyramid.
const ICO_SIZES: &[u32] = &[16, 32, 48, 64, 128, 256];

/// macOS modern RGBA32 `.icns` set: (type, pixel size).
const ICNS_TYPES: &[(IconType, u32)] = &[
    (IconType::RGBA32_16x16, 16),
    (IconType::RGBA32_16x16_2x, 32),
    (IconType::RGBA32_32x32, 32),
    (IconType::RGBA32_32x32_2x, 64),
    (IconType::RGBA32_64x64, 64),
    (IconType::RGBA32_128x128, 128),
    (IconType::RGBA32_128x128_2x, 256),
    (IconType::RGBA32_256x256, 256),
    (IconType::RGBA32_256x256_2x, 512),
    (IconType::RGBA32_512x512, 512),
    (IconType::RGBA32_512x512_2x, 1024),
];

/// Deepest starfield navy (`#020818`) — used for scrim fills.
const SF3: (u8, u8, u8) = (0x02, 0x08, 0x18);

/// Prod-emblem palette → Kaos palette (the dev/Kaos phone-icon variant, per
/// `docs/aesthetic/design_language.md` §1 "Kaos palette"). Gold → magenta,
/// starfield navy → void-violet, portal blues → ember pink. Applied as plain
/// hex substitution on the outlined emblem (usvg emits lowercase 6-digit hex,
/// and the "to" colours are disjoint from the "from" set, so order is safe).
const KAOS_REMAP: &[(&str, &str)] = &[
    ("#ffe58a", "#ff8ade"), // gold bright   -> ember bright
    ("#f5c634", "#ff4fd0"), // gold base     -> magenta
    ("#c58c18", "#da28a8"), // gold mid      -> magenta mid
    ("#3a2500", "#3d0028"), // gold deep/ink -> ember deep
    ("#0b1e52", "#1a0a40"), // starfield     -> void
    ("#020818", "#070212"), // deep navy     -> void black
    ("#0b2a7a", "#3d0028"), // portal-fill deep blue -> ember deep
    ("#6dceff", "#ff8ade"), // portal glow blue
    ("#4ec0ff", "#ff4fd0"), // portal fill blue
    ("#1a6fbf", "#da28a8"), // portal glow mid
    ("#8fd4ff", "#ff8ade"), // vortex ring blue
    ("#c8f0ff", "#ffe8f8"), // bright cyan   -> ember white
    ("#fdf5dc", "#ffe8f8"), // monogram cream -> ember white
];

fn repo_root() -> Result<PathBuf> {
    Ok(PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .ok_or_else(|| anyhow!("CARGO_MANIFEST_DIR has no grandparent"))?
        .to_path_buf())
}

/// Build parse options with a fontdb carrying the brand fonts:
/// Titan One (repo) for the wordmark, Georgia (system, ships on Win+mac)
/// for the serif subtitle / monogram. Only needed before `outline`; the
/// committed SVGs are font-free paths afterwards.
fn brand_options(root: &Path) -> Result<Options<'static>> {
    let mut opts = Options::default();
    let db = opts.fontdb_mut();
    db.load_system_fonts();
    let titan = root.join("crates/server/assets/fonts/TitanOne-Regular.ttf");
    db.load_font_file(&titan)
        .with_context(|| format!("load Titan One: {}", titan.display()))?;
    Ok(opts)
}

fn parse(root: &Path, svg_path: &Path) -> Result<Tree> {
    let opts = brand_options(root)?;
    let data = std::fs::read(svg_path).with_context(|| format!("read {}", svg_path.display()))?;
    Tree::from_data(&data, &opts).with_context(|| format!("parse {}", svg_path.display()))
}

fn load(root: &Path, rel: &str) -> Result<Tree> {
    parse(root, &root.join("assets/branding").join(rel))
}

fn tsize(t: &Tree) -> (f32, f32) {
    let s = t.size();
    (s.width(), s.height())
}

/// Render an SVG to a stretched `w`×`h` pixmap (aspect forced — use only
/// when the target matches the SVG's aspect, e.g. icons).
fn render_exact(tree: &Tree, w: u32, h: u32) -> Result<Pixmap> {
    let (sw, sh) = tsize(tree);
    let mut pm = Pixmap::new(w, h).ok_or_else(|| anyhow!("alloc {w}x{h} pixmap"))?;
    resvg::render(
        tree,
        Transform::from_scale(w as f32 / sw, h as f32 / sh),
        &mut pm.as_mut(),
    );
    Ok(pm)
}

/// Render an SVG at a uniform scale into its own pixmap (aspect preserved).
fn render_scaled(tree: &Tree, scale: f32) -> Result<Pixmap> {
    let (sw, sh) = tsize(tree);
    let w = (sw * scale).round().max(1.0) as u32;
    let h = (sh * scale).round().max(1.0) as u32;
    let mut pm = Pixmap::new(w, h).ok_or_else(|| anyhow!("alloc {w}x{h} pixmap"))?;
    resvg::render(tree, Transform::from_scale(scale, scale), &mut pm.as_mut());
    Ok(pm)
}

/// Composite `src` onto `base` at integer `(x, y)` with a blend mode + opacity.
fn blit(base: &mut Pixmap, src: &Pixmap, x: i32, y: i32, blend: BlendMode, opacity: f32) {
    let paint = PixmapPaint {
        opacity,
        blend_mode: blend,
        quality: FilterQuality::Bilinear,
    };
    base.as_mut()
        .draw_pixmap(x, y, src.as_ref(), &paint, Transform::identity(), None);
}

/// Cover-fit an SVG across the whole `base` (scaled up to fill, centre-cropped).
fn draw_cover(base: &mut Pixmap, tree: &Tree, blend: BlendMode, opacity: f32) -> Result<()> {
    let (bw, bh) = (base.width() as f32, base.height() as f32);
    let (sw, sh) = tsize(tree);
    let scale = (bw / sw).max(bh / sh);
    let pm = render_scaled(tree, scale)?;
    let x = ((bw - pm.width() as f32) / 2.0).round() as i32;
    let y = ((bh - pm.height() as f32) / 2.0).round() as i32;
    blit(base, &pm, x, y, blend, opacity);
    Ok(())
}

/// Place an SVG scaled to a target *height*, anchored by horizontal centre + top.
fn place_by_height(
    base: &mut Pixmap,
    tree: &Tree,
    target_h: f32,
    center_x: f32,
    top_y: f32,
    opacity: f32,
) -> Result<()> {
    let (_, sh) = tsize(tree);
    let pm = render_scaled(tree, target_h / sh)?;
    let x = (center_x - pm.width() as f32 / 2.0).round() as i32;
    blit(
        base,
        &pm,
        x,
        top_y.round() as i32,
        BlendMode::SourceOver,
        opacity,
    );
    Ok(())
}

/// Place an SVG scaled to a target *width*, anchored by horizontal centre + top.
fn place_by_width(
    base: &mut Pixmap,
    tree: &Tree,
    target_w: f32,
    center_x: f32,
    top_y: f32,
    opacity: f32,
) -> Result<()> {
    let (sw, _) = tsize(tree);
    let pm = render_scaled(tree, target_w / sw)?;
    let x = (center_x - pm.width() as f32 / 2.0).round() as i32;
    blit(
        base,
        &pm,
        x,
        top_y.round() as i32,
        BlendMode::SourceOver,
        opacity,
    );
    Ok(())
}

/// Darken the lower portion with a transparent→SF3 vertical gradient so a
/// title/logo block always has a dark base to read against.
fn bottom_scrim(base: &mut Pixmap, start_frac: f32, max_alpha: u8) -> Result<()> {
    let (w, h) = (base.width() as f32, base.height() as f32);
    let y0 = h * start_frac;
    let shader = LinearGradient::new(
        Point::from_xy(0.0, y0),
        Point::from_xy(0.0, h),
        vec![
            GradientStop::new(0.0, Color::from_rgba8(SF3.0, SF3.1, SF3.2, 0)),
            GradientStop::new(1.0, Color::from_rgba8(SF3.0, SF3.1, SF3.2, max_alpha)),
        ],
        SpreadMode::Pad,
        Transform::identity(),
    )
    .ok_or_else(|| anyhow!("build scrim gradient"))?;
    let paint = Paint {
        shader,
        ..Default::default()
    };
    base.fill_rect(
        Rect::from_xywh(0.0, y0, w, h - y0).ok_or_else(|| anyhow!("scrim rect"))?,
        &paint,
        Transform::identity(),
        None,
    );
    Ok(())
}

/// Composite the three background plates: gradient (base) + stars + vortex,
/// the latter two screened so their flat dark fills drop out and only the
/// sparkle / glow add over the gradient.
fn make_bg(root: &Path, w: u32, h: u32) -> Result<Pixmap> {
    let mut base = Pixmap::new(w, h).ok_or_else(|| anyhow!("alloc bg"))?;
    draw_cover(
        &mut base,
        &load(root, "bg-layer-gradient.svg")?,
        BlendMode::SourceOver,
        1.0,
    )?;
    draw_cover(
        &mut base,
        &load(root, "bg-layer-stars.svg")?,
        BlendMode::Screen,
        1.0,
    )?;
    draw_cover(
        &mut base,
        &load(root, "bg-layer-vortex.svg")?,
        BlendMode::Screen,
        1.0,
    )?;
    Ok(base)
}

fn save(pm: &Pixmap, root: &Path, name: &str) -> Result<()> {
    let dir = root.join("steam");
    std::fs::create_dir_all(&dir)?;
    let path = dir.join(name);
    pm.save_png(&path)
        .with_context(|| format!("write {}", path.display()))?;
    eprintln!("wrote {}", path.display());
    Ok(())
}

fn outline(root: &Path) -> Result<()> {
    let dir = root.join("assets/branding");
    let wopts = WriteOptions::default();
    for name in TEXT_SVGS {
        let path = dir.join(name);
        let tree = parse(root, &path)?;
        std::fs::write(&path, tree.to_string(&wopts).as_bytes())
            .with_context(|| format!("write {}", path.display()))?;
        eprintln!("outlined {}", path.display());
    }
    Ok(())
}

fn bake_icon(root: &Path) -> Result<()> {
    let tree = load(root, "icon-1024.svg")?;
    let out = root.join("assets/branding");

    let mut dir = IconDir::new(ResourceType::Icon);
    for &s in ICO_SIZES {
        let pm = render_exact(&tree, s, s)?;
        let img = IconImage::from_rgba_data(s, s, pm.data().to_vec());
        dir.add_entry(IconDirEntry::encode(&img).with_context(|| format!("encode {s}px ico"))?);
    }
    let ico_path = out.join("icon.ico");
    dir.write(&mut std::fs::File::create(&ico_path)?)
        .with_context(|| format!("write {}", ico_path.display()))?;
    eprintln!("baked {}", ico_path.display());

    let mut family = IconFamily::new();
    for &(ty, s) in ICNS_TYPES {
        let pm = render_exact(&tree, s, s)?;
        let img = IcnsImage::from_data(PixelFormat::RGBA, s, s, pm.data().to_vec())
            .with_context(|| format!("icns image {s}px"))?;
        family
            .add_icon_with_type(&img, ty)
            .with_context(|| format!("add icns {s}px"))?;
    }
    let icns_path = out.join("icon.icns");
    family
        .write(&mut std::fs::File::create(&icns_path)?)
        .with_context(|| format!("write {}", icns_path.display()))?;
    eprintln!("baked {}", icns_path.display());
    Ok(())
}

fn bake_steam(root: &Path) -> Result<()> {
    let character = load(root, "character.svg")?;
    let logo = load(root, "logo.svg")?;
    let icon = load(root, "icon-1024.svg")?;

    // Portrait / grid capsule (600×900, 2:3). The character art is 2:3, so it
    // fills the card edge-to-edge over the starfield; logo over a bottom scrim.
    {
        let mut base = make_bg(root, 600, 900)?;
        place_by_height(&mut base, &character, 900.0, 300.0, 0.0, 1.0)?;
        bottom_scrim(&mut base, 0.62, 235)?;
        place_by_width(&mut base, &logo, 520.0, 300.0, 760.0, 1.0)?;
        save(&base, root, "library_600x900.png")?;
    }

    // Hero (1920×620, ~3:1). Steam overlays its own logo on the left ~40%, so
    // the character sits on the right; left stays clear.
    {
        let mut base = make_bg(root, 1920, 620)?;
        // Scaled so the portal (the focal point, ~0.5 of the art's height) lands
        // near the vertical centre; arch bleeds top/bottom.
        place_by_height(&mut base, &character, 820.0, 1480.0, -90.0, 1.0)?;
        bottom_scrim(&mut base, 0.72, 130)?;
        save(&base, root, "library_hero_1920x620.png")?;
    }

    // Header / horizontal capsule (920×430). Wordmark on the left, character
    // emblem on the right.
    {
        let mut base = make_bg(root, 920, 430)?;
        place_by_height(&mut base, &character, 470.0, 720.0, -10.0, 1.0)?;
        bottom_scrim(&mut base, 0.55, 170)?;
        place_by_width(&mut base, &logo, 440.0, 330.0, 175.0, 1.0)?;
        save(&base, root, "library_header_920x430.png")?;
    }

    // Transparent logo overlay (1280×760): icon emblem above the wordmark.
    {
        let mut base = Pixmap::new(1280, 760).ok_or_else(|| anyhow!("alloc logo"))?;
        place_by_height(&mut base, &icon, 300.0, 640.0, 40.0, 1.0)?;
        place_by_width(&mut base, &logo, 1060.0, 640.0, 400.0, 1.0)?;
        save(&base, root, "logo.png")?;
    }

    // 256px icon.
    save(&render_exact(&icon, 256, 256)?, root, "icon_256.png")?;
    Ok(())
}

/// Emit the phone app's two icon SVGs from the single branding master, so the
/// phone favicon/PWA icon and the desktop launcher window icon (which embeds
/// `phone/assets/icons/icon-192.png`, see `crates/server/src/main.rs`) all
/// match the desktop/Steam emblem. `tools/icon-bake` then rasterises these to
/// the PNG sizes browsers/PWAs need. Keeping them generated (not hand-edited
/// copies) means a future emblem tweak only touches `icon-1024.svg`.
fn phone_icons(root: &Path) -> Result<()> {
    let emblem = std::fs::read_to_string(root.join("assets/branding/icon-1024.svg"))
        .context("read icon-1024.svg")?;
    let icons = root.join("phone/assets/icons");

    let header = "<!-- GENERATED by brand-bake (phone-icons mode) from \
                  assets/branding/icon-1024.svg — do not hand-edit. -->\n";
    let prod = icons.join("icon.svg");
    std::fs::write(&prod, format!("{header}{emblem}"))
        .with_context(|| format!("write {}", prod.display()))?;
    eprintln!("wrote {}", prod.display());

    // Kaos-recoloured dev variant (distinct home-screen icon for dev installs).
    let mut dev = emblem.clone();
    for (from, to) in KAOS_REMAP {
        dev = dev.replace(from, to);
    }
    let dev_header =
        "<!-- GENERATED Kaos dev variant — see brand-bake KAOS_REMAP. Do not hand-edit. -->\n";
    let dev_path = icons.join("icon-dev.svg");
    std::fs::write(&dev_path, format!("{dev_header}{dev}"))
        .with_context(|| format!("write {}", dev_path.display()))?;
    eprintln!("wrote {}", dev_path.display());
    Ok(())
}

fn debug_png(root: &Path, infile: &str, outfile: &str, w: u32, h: u32) -> Result<()> {
    let tree = parse(root, Path::new(infile))?;
    render_exact(&tree, w, h)?.save_png(outfile)?;
    eprintln!("rendered {outfile} ({w}x{h})");
    Ok(())
}

fn main() -> Result<()> {
    let root = repo_root()?;
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mode = args.first().map(String::as_str).unwrap_or("all");
    match mode {
        "outline" => outline(&root)?,
        "icon" => bake_icon(&root)?,
        "steam" => bake_steam(&root)?,
        "phone-icons" => phone_icons(&root)?,
        "all" => {
            outline(&root)?;
            bake_icon(&root)?;
            bake_steam(&root)?;
            phone_icons(&root)?;
        }
        "debug-png" => {
            let infile = args
                .get(1)
                .ok_or_else(|| anyhow!("debug-png <in> <out> <w> <h>"))?;
            let outfile = args.get(2).ok_or_else(|| anyhow!("need <out>"))?;
            let w: u32 = args.get(3).map(|s| s.parse()).transpose()?.unwrap_or(512);
            let h: u32 = args.get(4).map(|s| s.parse()).transpose()?.unwrap_or(w);
            debug_png(&root, infile, outfile, w, h)?;
        }
        other => return Err(anyhow!("unknown mode: {other}")),
    }
    Ok(())
}
