//! Rasterises the phone app's SVG icons into PNGs at every size browsers,
//! iOS PWAs, and Android adaptive icons need. Output goes alongside the
//! source SVGs in `phone/assets/` so trunk picks them up via `data-trunk
//! rel="copy-file"` directives in `phone/index.html`.
//!
//! Run after editing either SVG:
//!
//!     cargo run -p skylander-icon-bake
//!
//! Sizes:
//!   - 32  — browser favicon (tab + history)
//!   - 180 — iOS apple-touch-icon (home-screen pinning)
//!   - 192 — Android PWA standard
//!   - 512 — Android PWA splash / store listings
//!
//! Output naming:
//!   - `icon-{32,180,192,512}.png`     (production)
//!   - `icon-dev-{32,180,192,512}.png` (dev)
//!
//! The server picks which of the two sets to serve at request time —
//! see `crates/server/src/http.rs` `serve_icon` for the dev-tools-feature
//! gate. PWA install captures the icon at install time, so the dev
//! variant ends up on the home screen iff you pinned from a dev server.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result, anyhow};
use resvg::{
    tiny_skia::{Pixmap, Transform},
    usvg::{Options, Tree},
};

/// Sizes to bake. Keep in sync with `phone/index.html` and the server's
/// icon route handler.
const SIZES: &[u32] = &[32, 180, 192, 512];

fn main() -> Result<()> {
    // Tool is invoked from the workspace root via `cargo run -p
    // skylander-icon-bake`. CARGO_MANIFEST_DIR points at this crate; walk
    // up to the repo root to find phone/assets/.
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let repo_root = manifest_dir
        .ancestors()
        .nth(2)
        .ok_or_else(|| anyhow!("CARGO_MANIFEST_DIR has no grandparent"))?
        .to_path_buf();
    let icons = repo_root.join("phone").join("assets").join("icons");

    let prod_svg = icons.join("icon.svg");
    let dev_svg = icons.join("icon-dev.svg");

    bake_set(&prod_svg, &icons, "icon")?;
    bake_set(&dev_svg, &icons, "icon-dev")?;

    Ok(())
}

fn bake_set(svg_path: &Path, out_dir: &Path, prefix: &str) -> Result<()> {
    let svg_data =
        std::fs::read(svg_path).with_context(|| format!("read SVG: {}", svg_path.display()))?;

    // Default options are fine — our SVGs are inline, no external refs,
    // no fonts, no images. usvg parses → tiny_skia rasterises.
    let opts = Options::default();
    let tree = Tree::from_data(&svg_data, &opts)
        .with_context(|| format!("parse SVG: {}", svg_path.display()))?;

    // Source SVGs are authored at 512×512. Compute uniform scale per
    // target size so the rendered output fills the pixmap exactly.
    let src = tree.size();
    let src_max = src.width().max(src.height());

    for &size in SIZES {
        let mut pixmap =
            Pixmap::new(size, size).ok_or_else(|| anyhow!("alloc {size}×{size} pixmap"))?;
        let scale = size as f32 / src_max;
        let transform = Transform::from_scale(scale, scale);
        resvg::render(&tree, transform, &mut pixmap.as_mut());

        let out_path = out_dir.join(format!("{prefix}-{size}.png"));
        pixmap
            .save_png(&out_path)
            .with_context(|| format!("write PNG: {}", out_path.display()))?;
        eprintln!("baked {}", out_path.display());
    }

    Ok(())
}
