//! Shared colour palette for the egui TV launcher (PLAN 4.15.1).
//!
//! These mirror the phone SPA's CSS tokens in `phone/assets/app.css` so the
//! PC-side and phone-side share a visual identity. Source-of-truth is the
//! phone stylesheet; update both together if the palette shifts.

use egui::{Color32, Visuals};

// ---- Starfield-blue background gradient ---------------------------------

/// Deepest starfield blue — used as the base fill behind the cloud vortex
/// and everywhere we need "truly black-but-not-black".
pub const SF_3: Color32 = Color32::from_rgb(0x02, 0x08, 0x18);
/// Mid starfield blue.
pub const SF_2: Color32 = Color32::from_rgb(0x06, 0x14, 0x36);
/// Top starfield blue (lifts near the heading / vortex core).
pub const SF_1: Color32 = Color32::from_rgb(0x0b, 0x1e, 0x52);

// ---- Gold accent family -------------------------------------------------

/// Bright-highlight gold — bezel inner ring, hover ticks.
pub const GOLD_BRIGHT: Color32 = Color32::from_rgb(0xff, 0xe5, 0x8a);
/// Primary gold — bezel body, headline outline, QR label.
pub const GOLD: Color32 = Color32::from_rgb(0xf5, 0xc6, 0x34);
/// Secondary gold, used for the `.game-name` text fill equivalents.
pub const GOLD_2: Color32 = Color32::from_rgb(0xe9, 0xa7, 0x14);
/// Mid-dark gold — third stop in the bezel's radial gradient (the
/// "shadow side" of the embossed metal before it fades to deep
/// shadow). Mirrors the phone's `--gm: #c58c18`. Distinct from
/// `GOLD_2` (which is a brighter mid-tone for text); the bezel
/// gradient needs this darker step or the embossed lighting reads
/// as flat (Chris flagged 2026-04-19).
pub const GOLD_MID: Color32 = Color32::from_rgb(0xc5, 0x8c, 0x18);
/// Gold shadow — bezel outer shadow ring, text drop shadow.
pub const GOLD_SHADOW: Color32 = Color32::from_rgb(0x6e, 0x4a, 0x00);
/// Gold ink — darkest tone, for inset engraving effects.
pub const GOLD_INK: Color32 = Color32::from_rgb(0x3a, 0x25, 0x00);

// ---- Text -----------------------------------------------------------------

/// Primary text — nearly white with a hint of warmth.
pub const TEXT: Color32 = Color32::from_rgb(0xf7, 0xf7, 0xfb);
/// Dimmed text (~66% opacity over a dark background). Used for secondary
/// labels: "Scan to connect", figure count, etc.
pub const TEXT_DIM: Color32 = Color32::from_rgba_premultiplied(0xa3, 0xa3, 0xa6, 0xa8);

// ---- Status semantics (for indicators, toasts, etc.) --------------------

pub const DANGER: Color32 = Color32::from_rgb(0xd1, 0x44, 0x44);
pub const SUCCESS_GLOW: Color32 = Color32::from_rgb(0x5a, 0xc9, 0x6b);

// ---- Typography (TV viewing distance, ~10 ft) ---------------------------
//
// These point sizes are tuned for the 86" TV — separate from the phone's
// `--t-*` tokens (which target handheld 6-12in viewing). Sizes match the
// values currently shipped in `ui/*.rs`; navigation.md §3.7's spec
// reference diverges (96/64/40/32/24) — that reconciliation is tracked
// in PLAN 4.19.19, not this constant set. Per PLAN 4.20.10.
//
// Code is truth: when the launcher's visual tuning lands a different
// size on real hardware, update the constant here and call sites
// follow automatically.

/// "STARTING" intro title — embossed paint via `paint_heraldic_title`.
pub const HERO_INTRO: f32 = 140.0;
/// Heraldic subtitle bands (e.g. game-name during boot, "SCAN TO CONNECT").
pub const HEADING_LG: f32 = 64.0;
/// Farewell-screen large numerals (countdown).
pub const COUNTDOWN: f32 = 56.0;
/// Display heading on sub-screens (RESTART label, Exit button, status copy).
pub const HEADING: f32 = 28.0;
/// Common subhead / context line (under headings, button-row labels,
/// shader-compile progress).
pub const SUBHEAD: f32 = 20.0;
/// Body copy on error / info screens (server-error message text).
pub const BODY: f32 = 18.0;
/// Small overlay text on the in-game transparent surface (RECONNECT
/// label).
pub const CAPTION: f32 = 14.0;
/// Tiniest secondary text (in-game "scan to rejoin" subline).
pub const CAPTION_SM: f32 = 11.0;

// ---- Apply to egui::Context ---------------------------------------------

/// Install the palette into an egui context. Called once from
/// `LauncherApp::new`. Overrides the dark-mode defaults so the whole
/// launcher reads as starfield-blue rather than egui's default near-black.
///
/// **Pins the theme preference to Dark first.** egui 0.29 follows the OS
/// theme by default (`ThemePreference::System`), and on a Windows 11 box
/// set to Light Mode it would apply `Visuals::light()` *after* our
/// `set_visuals` call, clobbering the starfield panel_fill back to the
/// cream ~#F8F8F8 default — Chris hit exactly that on the HTPC
/// 2026-04-19 (the launcher rendered with a near-white background and
/// the gold title floating in the middle). Setting the preference to
/// `Dark` stops egui from overriding our visuals on subsequent frames.
pub fn apply(ctx: &egui::Context) {
    ctx.set_theme(egui::ThemePreference::Dark);

    let mut v = Visuals::dark();
    // Background surfaces — central panel, windows, popups.
    v.panel_fill = SF_3;
    v.window_fill = SF_2;
    v.extreme_bg_color = SF_3;
    v.faint_bg_color = SF_2;
    // Primary text. Sub-widgets (buttons etc.) pick this up unless they
    // override in their own RichText::color call.
    v.override_text_color = Some(TEXT);
    v.hyperlink_color = GOLD;
    // Selection highlight — gold accent so the first-launch wizard's text
    // fields don't look out-of-place.
    v.selection.bg_fill = GOLD_INK;
    v.selection.stroke.color = GOLD;
    ctx.set_visuals(v);
}
