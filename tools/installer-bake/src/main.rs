//! Rasterises the brand SVG into installer-tier icons:
//!
//!     assets/branding/icon.ico   (Windows multi-res — winres + MSI shortcut)
//!     assets/branding/icon.icns  (macOS — .app bundle, modern RGBA32 set)
//!
//! Source is `phone/assets/icons/icon.svg` — the same gold-bezeled
//! "portal viewed from above" the phone PWA uses, so the Windows
//! shortcut, the Mac dock icon, and the iOS home-screen icon all
//! match.
//!
//! Run after editing the source SVG:
//!
//!     cargo run -p skylander-installer-bake
//!
//! Output is committed to the repo; the release workflow consumes
//! the baked files directly so CI doesn't need resvg+ico+icns
//! crates in its build.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result, anyhow};
use icns::{IconFamily, IconType, Image as IcnsImage, PixelFormat};
use ico::{IconDir, IconDirEntry, IconImage, ResourceType};
use resvg::{
    tiny_skia::{Pixmap, Transform},
    usvg::{Options, Tree},
};

/// Sizes packed into the multi-res `.ico`. Standard Windows icon
/// pyramid: 16 (small UI / shortcut), 32 (file explorer), 48 (large
/// icon view), 64 (Windows 11 jumbo), 128 (high-DPI taskbar), 256
/// (Vista+ "extra-large icons" + MSI shortcut at high zoom).
const ICO_SIZES: &[u32] = &[16, 32, 48, 64, 128, 256];

/// `.icns` modern RGBA32 set. Pairs each IconType with the pixel
/// dimensions to rasterise — `_2x` variants live at 2× pixel size in
/// the file even though Finder/Dock UI labels them at the @1x point
/// size. Modern macOS (Big Sur+) sources almost everything from this
/// set; the legacy RGB24 + Mask8 pre-Mountain-Lion variants are not
/// worth shipping for a 2026-era binary.
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

fn main() -> Result<()> {
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let repo_root = manifest_dir
        .ancestors()
        .nth(2)
        .ok_or_else(|| anyhow!("CARGO_MANIFEST_DIR has no grandparent"))?
        .to_path_buf();

    let svg_path = repo_root
        .join("phone")
        .join("assets")
        .join("icons")
        .join("icon.svg");
    let out_dir = repo_root.join("assets").join("branding");
    std::fs::create_dir_all(&out_dir)
        .with_context(|| format!("create {}", out_dir.display()))?;

    let svg_data = std::fs::read(&svg_path)
        .with_context(|| format!("read SVG: {}", svg_path.display()))?;
    // Default options — the SVG is self-contained, no external refs,
    // no fonts, no images. Same setup as `tools/icon-bake/src/main.rs`.
    let opts = Options::default();
    let tree = Tree::from_data(&svg_data, &opts)
        .with_context(|| format!("parse SVG: {}", svg_path.display()))?;

    bake_ico(&tree, &out_dir.join("icon.ico"))?;
    bake_icns(&tree, &out_dir.join("icon.icns"))?;

    Ok(())
}

/// Rasterise + pack a multi-resolution Windows `.ico`.
fn bake_ico(tree: &Tree, out_path: &Path) -> Result<()> {
    let mut dir = IconDir::new(ResourceType::Icon);
    for &size in ICO_SIZES {
        let pixmap = render(tree, size, size)?;
        // ico::IconImage::from_rgba_data takes raw RGBA8. Pixmap is
        // already premultiplied RGBA — Windows expects straight alpha
        // in the embedded PNG, but for sizes < 256 the IconImage path
        // packs as PNG anyway and most Windows shells composite over
        // any background, so the slight over-darkening on partial
        // alpha is invisible at icon scale.
        let image = IconImage::from_rgba_data(size, size, pixmap.data().to_vec());
        let entry = IconDirEntry::encode(&image)
            .with_context(|| format!("encode {size}px ico entry"))?;
        dir.add_entry(entry);
    }
    let mut file = std::fs::File::create(out_path)
        .with_context(|| format!("create {}", out_path.display()))?;
    dir.write(&mut file)
        .with_context(|| format!("write {}", out_path.display()))?;
    eprintln!("baked {}", out_path.display());
    Ok(())
}

/// Rasterise + pack a macOS `.icns` with the modern RGBA32 set.
fn bake_icns(tree: &Tree, out_path: &Path) -> Result<()> {
    let mut family = IconFamily::new();
    for &(icon_type, size) in ICNS_TYPES {
        let pixmap = render(tree, size, size)?;
        let img = IcnsImage::from_data(PixelFormat::RGBA, size, size, pixmap.data().to_vec())
            .map_err(|e| anyhow!("icns image construct {size}px: {e}"))?;
        family
            .add_icon_with_type(&img, icon_type)
            .map_err(|e| anyhow!("icns add_icon_with_type {icon_type:?}: {e}"))?;
    }
    let mut file = std::fs::File::create(out_path)
        .with_context(|| format!("create {}", out_path.display()))?;
    family
        .write(&mut file)
        .with_context(|| format!("write {}", out_path.display()))?;
    eprintln!("baked {}", out_path.display());
    Ok(())
}

/// Rasterise the SVG tree into a fresh pixmap. Computes a uniform
/// scale that fits the source bounds into the target dimensions.
fn render(tree: &Tree, w: u32, h: u32) -> Result<Pixmap> {
    let mut pixmap = Pixmap::new(w, h).ok_or_else(|| anyhow!("alloc {w}x{h} pixmap"))?;
    let src = tree.size();
    let scale = (w as f32 / src.width()).min(h as f32 / src.height());
    let transform = Transform::from_scale(scale, scale);
    resvg::render(tree, transform, &mut pixmap.as_mut());
    Ok(pixmap)
}
