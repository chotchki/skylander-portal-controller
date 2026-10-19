//! The TV's look as data (openspec AP, design decision 6): the palette from
//! `docs/aesthetic/design_language.md` §1, the text roles of its §2 contrast
//! contract and the TV type scale from `docs/aesthetic/navigation.md` §3.7.
//! Screens draw from the [`TvTheme`] resource, never from a literal colour or
//! font size, so a Kaos skin later is another `TvTheme`. It holds what the TV
//! draws with today; the status colours join with the box that draws them.

use bevy::prelude::*;
use skylander_core::Element;

/// A step of the TV type scale, in design px (`UiScale` maps 1080 of them to
/// the screen's height). Nothing readable goes under [`Step::Caption`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum Step {
    /// 96: a screen's title.
    Hero,
    /// 64: the call to scan, the game's name, Found's verdict.
    Lg,
    /// 40: status lines, a button's label, a figure's name.
    Md,
    /// 32: messages and connection info.
    Body,
    /// 24: secondary info, the floor.
    Caption,
}

#[cfg(test)]
impl Step {
    pub(crate) const ALL: [Self; 5] = [Self::Hero, Self::Lg, Self::Md, Self::Body, Self::Caption];
}

/// What colour text takes, picked by what the text is rather than how loud it
/// should feel (§2). Every role is opaque: dimming text with alpha is the
/// pattern the contract bans.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Role {
    /// Headings, body copy, warnings: warm white.
    Primary,
    /// Display type, short Titan One labels, button text: bright gold.
    Accent,
    /// Captions, hints, secondary lines: muted gold.
    Muted,
}

#[derive(Resource, Debug, Clone, PartialEq)]
pub(crate) struct TvTheme {
    /// `--sf-1..3`, top to bottom: the sky's fall (`starfield::sky`).
    pub(crate) starfield: [Color; 3],
    /// The sky's two glows, top (`#1a3a8a`) and bottom (`#0e2464`).
    pub(crate) sky_glow: [Color; 2],
    /// The stars' colours, by `starfield::Tint`: white, warm, cool.
    pub(crate) star_tints: [Color; 3],
    /// §1's gold ramp, brightest first: `--gb` (a blue card's edge), `--g` (the
    /// progress bar's fill), `--gm`, `--gs` and `--gi` (the dark line in a
    /// bezel). Gold surfaces run through several of them (`surfaces`): flat
    /// gold is forbidden.
    pub(crate) gold_bright: Color,
    pub(crate) gold: Color,
    pub(crate) gold_mid: Color,
    pub(crate) gold_shadow: Color,
    pub(crate) gold_inner: Color,
    /// §3.2's blue card at rest, top to bottom (0, 55%, 100%): the buttons.
    pub(crate) blue_card: [Color; 3],
    /// §1's blue panel, top to bottom (0, 50%, 100%), opaque so text on it
    /// keeps one backdrop: a figure card's fill.
    pub(crate) panel: [Color; 3],
    /// §3.3's warm glow at the top of a panel, `#ffe678`. bevy_ui blends in
    /// linear light where CSS blends sRGB values, so §3.3's 8% here drew a
    /// visible band across each card at 1080p (a red channel 60 where CSS
    /// gives 44 on the panel's blue); 3% reads as the phone's 8% does.
    pub(crate) panel_glow: Color,
    /// A bezel's plate for a figure with no element (the phone's
    /// `.bezel-plate`), light stop first.
    pub(crate) plate: [Color; 2],
    /// What highlights and shadows are tinted with; each surface sets its
    /// own alpha.
    pub(crate) light: Color,
    pub(crate) shadow: Color,
    /// An empty frame's edge and the progress bar's track: muted gold at 22%.
    /// A surface, never text.
    pub(crate) faint: Color,
    /// Text by [`Role`].
    primary: Color,
    accent: Color,
    muted: Color,
    /// Dark text on a light fill (a pale owner pip): `--sf-3`.
    ink: Color,
    /// Each element's 135° gradient, light stop first, in [`element_index`]
    /// order.
    elements: [[Color; 2]; 10],
    /// The type scale, [`Step::Hero`] to [`Step::Caption`].
    scale: [f32; 5],
}

impl Default for TvTheme {
    /// The Skylanders look.
    fn default() -> Self {
        let hex = Color::srgb_u8;
        Self {
            starfield: [
                hex(0x0b, 0x1e, 0x52),
                hex(0x06, 0x14, 0x36),
                hex(0x02, 0x08, 0x18),
            ],
            sky_glow: [hex(0x1a, 0x3a, 0x8a), hex(0x0e, 0x24, 0x64)],
            star_tints: [
                hex(0xff, 0xff, 0xff),
                hex(0xff, 0xe6, 0xb4),
                hex(0xb4, 0xdc, 0xff),
            ],
            gold_bright: hex(0xff, 0xe5, 0x8a),
            gold: hex(0xf5, 0xc6, 0x34),
            gold_mid: hex(0xc5, 0x8c, 0x18),
            gold_shadow: hex(0x6e, 0x4a, 0x00),
            gold_inner: hex(0x3a, 0x25, 0x00),
            blue_card: [
                hex(0x2f, 0x6e, 0xdc),
                hex(0x1e, 0x4f, 0xb3),
                hex(0x15, 0x3a, 0x8a),
            ],
            panel: [
                hex(0x1a, 0x3a, 0x8a),
                hex(0x0a, 0x24, 0x64),
                hex(0x06, 0x14, 0x36),
            ],
            panel_glow: Color::srgba_u8(0xff, 0xe6, 0x78, 8),
            plate: [hex(0x1a, 0x2a, 0x4a), hex(0x0a, 0x16, 0x30)],
            light: Color::WHITE,
            shadow: Color::BLACK,
            faint: Color::srgba_u8(0xd9, 0xc8, 0x8a, 56),
            primary: hex(0xff, 0xf4, 0xe0),
            accent: hex(0xff, 0xe5, 0x8a),
            muted: hex(0xd9, 0xc8, 0x8a),
            ink: hex(0x02, 0x08, 0x18),
            elements: [
                [hex(0xc6, 0xe6, 0xff), hex(0x7e, 0xa9, 0xcc)], // Air
                [hex(0x2a, 0x2a, 0x3a), hex(0x05, 0x05, 0x0a)], // Dark
                [hex(0xa7, 0x7b, 0x3a), hex(0x5a, 0x3a, 0x13)], // Earth
                [hex(0xff, 0x6b, 0x2a), hex(0xb1, 0x33, 0x10)], // Fire
                [hex(0x5a, 0xc9, 0x6b), hex(0x26, 0x61, 0x2e)], // Life
                [hex(0xff, 0xf8, 0xa0), hex(0xc8, 0xb0, 0x2c)], // Light
                [hex(0xda, 0x5a, 0xd6), hex(0x65, 0x15, 0x64)], // Magic
                [hex(0xff, 0xb8, 0x4d), hex(0xa5, 0x65, 0x00)], // Tech
                [hex(0x8a, 0x62, 0xc9), hex(0x3b, 0x21, 0x5f)], // Undead
                [hex(0x2a, 0xa6, 0xff), hex(0x0f, 0x4d, 0x8a)], // Water
            ],
            scale: [96.0, 64.0, 40.0, 32.0, 24.0],
        }
    }
}

impl TvTheme {
    pub(crate) fn size(&self, step: Step) -> f32 {
        self.scale[step as usize]
    }

    /// The walk test's check that a drawn size came from a [`Step`].
    #[cfg(test)]
    pub(crate) fn on_scale(&self, px: f32) -> bool {
        self.scale.contains(&px)
    }

    pub(crate) fn role(&self, role: Role) -> Color {
        match role {
            Role::Primary => self.primary,
            Role::Accent => self.accent,
            Role::Muted => self.muted,
        }
    }

    /// A figure's element as its 135° gradient's two stops, light first; none
    /// for a figure without one.
    pub(crate) fn element(&self, element: Option<Element>) -> Option<[Color; 2]> {
        element.map(|e| self.elements[element_index(e)])
    }

    /// Dark ink on a light fill, warm white on a dark one.
    pub(crate) fn ink_on(&self, fill: Srgba) -> Color {
        let luma = 0.2126 * fill.red + 0.7152 * fill.green + 0.0722 * fill.blue;
        if luma > 0.6 { self.ink } else { self.primary }
    }

    /// Every colour text may take: the roles, and the ink a pale fill gets.
    #[cfg(test)]
    pub(crate) fn text_colors(&self) -> [Color; 4] {
        [self.primary, self.accent, self.muted, self.ink]
    }
}

/// An element's row in [`TvTheme::elements`]: §1's table, alphabetical.
fn element_index(element: Element) -> usize {
    match element {
        Element::Air => 0,
        Element::Dark => 1,
        Element::Earth => 2,
        Element::Fire => 3,
        Element::Life => 4,
        Element::Light => 5,
        Element::Magic => 6,
        Element::Tech => 7,
        Element::Undead => 8,
        Element::Water => 9,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hex(color: Color) -> String {
        Srgba::from(color).to_hex()
    }

    #[test]
    fn the_scale_is_the_tv_scale_with_a_24_px_floor() {
        let theme = TvTheme::default();
        let sizes = Step::ALL.map(|s| theme.size(s));
        assert_eq!(sizes, [96.0, 64.0, 40.0, 32.0, 24.0]);
        assert!(sizes.windows(2).all(|w| w[0] > w[1]), "{sizes:?}");
        assert_eq!(theme.size(Step::Caption), 24.0);
        for px in [22.0, 28.0, 30.0, 36.0, 56.0, 80.0, 88.0] {
            assert!(!theme.on_scale(px), "{px}");
        }
    }

    #[test]
    fn the_roles_are_the_contracts_colours_and_opaque() {
        let theme = TvTheme::default();
        assert_eq!(hex(theme.role(Role::Primary)), "#FFF4E0");
        assert_eq!(hex(theme.role(Role::Accent)), "#FFE58A");
        assert_eq!(hex(theme.role(Role::Muted)), "#D9C88A");
        for color in theme.text_colors() {
            assert_eq!(color.alpha(), 1.0, "{}", hex(color));
        }
        assert_eq!(
            [
                theme.gold_bright,
                theme.gold,
                theme.gold_mid,
                theme.gold_shadow,
                theme.gold_inner
            ]
            .map(hex),
            ["#FFE58A", "#F5C634", "#C58C18", "#6E4A00", "#3A2500"]
        );
        assert_eq!(theme.blue_card.map(hex), ["#2F6EDC", "#1E4FB3", "#153A8A"]);
        assert_eq!(theme.panel.map(hex), ["#1A3A8A", "#0A2464", "#061436"]);
        assert!(theme.panel.iter().all(|c| c.alpha() == 1.0));
        assert_eq!(theme.plate.map(hex), ["#1A2A4A", "#0A1630"]);
        assert_eq!(theme.starfield.map(hex), ["#0B1E52", "#061436", "#020818"]);
        assert_eq!(theme.sky_glow.map(hex), ["#1A3A8A", "#0E2464"]);
        assert_eq!(theme.star_tints.map(hex), ["#FFFFFF", "#FFE6B4", "#B4DCFF"]);
    }

    #[test]
    fn every_element_has_its_own_opaque_gradient() {
        use Element::*;
        let theme = TvTheme::default();
        let all = [
            Air, Dark, Earth, Fire, Life, Light, Magic, Tech, Undead, Water,
        ];
        let stops: Vec<[String; 2]> = all
            .iter()
            .map(|e| theme.element(Some(*e)).unwrap().map(hex))
            .collect();
        assert_eq!(stops[3], ["#FF6B2A", "#B13310"], "Fire");
        assert_eq!(stops[9], ["#2AA6FF", "#0F4D8A"], "Water");
        assert_eq!(stops[1], ["#2A2A3A", "#05050A"], "Dark");
        let mut unique = stops.clone();
        unique.sort();
        unique.dedup();
        assert_eq!(unique.len(), all.len(), "{stops:?}");
        for e in all {
            let [light, dark] = theme.element(Some(e)).unwrap();
            assert!(light.alpha() == 1.0 && dark.alpha() == 1.0, "{e:?}");
            assert_ne!(light, dark, "{e:?}");
        }
        assert_eq!(theme.element(None), None);
    }

    #[test]
    fn a_pale_fill_gets_dark_ink_and_a_dark_one_warm_white() {
        let theme = TvTheme::default();
        assert_eq!(hex(theme.ink_on(Srgba::rgb(1.0, 1.0, 0.6))), "#020818");
        assert_eq!(
            theme.ink_on(Srgba::rgb(0.1, 0.1, 0.5)),
            theme.role(Role::Primary)
        );
    }
}
