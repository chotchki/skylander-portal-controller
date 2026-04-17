//! Titan One font for the egui TV launcher (PLAN 4.15.2).
//!
//! Registers the same display face the phone uses so the PC-side and
//! phone-side feel unified. Not applied to every `Proportional` text slot —
//! Titan One is a heavy display face and looks cramped below ~20pt — so we
//! expose it as a named family. Callers opt in per-RichText via
//! `egui::FontFamily::Name("titan_one".into())`.
//!
//! Source: [Google Fonts Titan One](https://fonts.google.com/specimen/Titan+One),
//! OFL. TTF committed under `crates/server/assets/fonts/`. File is ~55 KB;
//! not worth the build-time complexity of decompressing the WOFF2 the phone
//! ships — let the two copies coexist.

use egui::{FontData, FontDefinitions, FontFamily};

/// Family name to pass to `egui::RichText::family(FontFamily::Name(..))`.
pub const TITAN_ONE: &str = "titan_one";

/// Install Titan One into the given context. Call once at `LauncherApp::new`
/// alongside `palette::apply`.
pub fn register(ctx: &egui::Context) {
    let mut fonts = FontDefinitions::default();

    fonts.font_data.insert(
        TITAN_ONE.into(),
        FontData::from_static(include_bytes!("../assets/fonts/TitanOne-Regular.ttf")),
    );

    // Named family — opt-in per RichText. Kept separate from
    // `FontFamily::Proportional` so egui's default Noto/Hack fallbacks keep
    // handling body / monospace text; only headings we explicitly set to
    // `Name("titan_one")` pick up Titan One.
    fonts
        .families
        .insert(FontFamily::Name(TITAN_ONE.into()), vec![TITAN_ONE.into()]);

    ctx.set_fonts(fonts);
}
