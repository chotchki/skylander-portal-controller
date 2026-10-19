//! The TV's materials (openspec AP, design decision 8): `design_language.md`
//! §3's gold bezel, framed panel and blue card, drawn with bevy_ui 0.20's own
//! gradients, borders and shadows from [`TvTheme`]. bevy_ui has no inset
//! shadow, so a ring's dark inner line is a border (or a hard radial stop) and
//! its inset light and shade are gradient layers over its fill. Sizes are the
//! phone CSS's (`gold_bezel.css`, `portal.css`, `action_button.css`) scaled by
//! each surface's size over the phone's, so each keeps its proportions.
//!
//! One departure from §3.1, taken from the phone: a figure's ring wears its
//! element's gradient instead of gold (`gold_bezel.css`, chotchki's 2026-05-04
//! playtest: "color code the element instead of just a gold ring"). The
//! portrait covers the plate, and the card's edge is gold, so a gold ring
//! would leave the element showing nowhere but the card's caption.

use bevy::prelude::*;
use bevy::ui::{Gradient, InterpolationColorSpace, RadialGradientShape, UiPosition};
use skylander_core::Element;

use crate::theme::TvTheme;

/// A card's portrait bezel, in design px: §3.1's hero size.
pub(crate) const BEZEL: f32 = 160.0;

/// The bezel over the phone's portal bezel (128 px).
const BEZEL_SCALE: f32 = BEZEL / 128.0;

/// The ring's dark `--gi` line (`inset 0 0 0 2px`), a border.
const RING_LINE: f32 = 2.0 * BEZEL_SCALE;

/// The plate sits 10% in from the bezel's edge.
const PLATE_INSET: f32 = 0.1 * BEZEL;

/// The plate's dark edge (`inset 0 0 0 1px rgba(0,0,0,0.6)`), a border.
const PLATE_EDGE: f32 = BEZEL_SCALE;

/// The owner pip, in design px, and its gap from the card's top-right
/// corner; the phone's pip is 24.
pub(crate) const PIP: f32 = 60.0;
const PIP_INSET: f32 = 10.0;
const PIP_SCALE: f32 = PIP / 24.0;

/// The pip's gold ring: the phone's plate sits 14% in.
const PIP_RING: f32 = 0.14 * PIP;

/// The buttons are the phone's blue card at twice its size.
const BUTTON_SCALE: f32 = 2.0;

/// A button face's gold edge and corner radius (the phone's 2 and 12).
pub(crate) const BUTTON_BORDER: f32 = 2.0 * BUTTON_SCALE;
pub(crate) const BUTTON_RADIUS: f32 = 12.0 * BUTTON_SCALE;

/// CSS's default radial size from a centre at 30% 25% of a square: the
/// farthest corner, √(0.7² + 0.75²) of the width.
const FAR_CORNER: f32 = 102.6;

/// A portrait's bezel, the ring node, so tests can find it.
#[derive(Component, Debug)]
pub(crate) struct Bezel;

/// The bezel's plate: the element (or the plain plate) under the portrait.
#[derive(Component, Debug)]
pub(crate) struct Plate;

/// Gradients interpolate in sRGB, as CSS's do.
fn linear(angle: f32, stops: Vec<ColorStop>) -> Gradient {
    LinearGradient::new(angle, stops)
        .in_color_space(InterpolationColorSpace::Srgba)
        .into()
}

/// An inset shadow's soft edge: `color` at `alpha` on the edge the gradient
/// starts from (`angle`), gone `depth` px in.
fn fade(angle: f32, color: Color, alpha: f32, depth: f32) -> Gradient {
    linear(
        angle,
        vec![
            ColorStop::px(color.with_alpha(alpha), 0.0),
            ColorStop::px(color.with_alpha(0.0), depth),
        ],
    )
}

/// An inset shadow with no blur: a hard line `width` px wide, just inside a
/// border `from` px wide (bevy_ui doesn't draw a background under a border).
fn line(angle: f32, color: Color, alpha: f32, from: f32, width: f32) -> Gradient {
    let at = from + width;
    linear(
        angle,
        vec![
            ColorStop::px(color.with_alpha(alpha), at),
            ColorStop::px(color.with_alpha(0.0), at),
        ],
    )
}

/// §3.1's gold: `radial-gradient(circle at 30% 25%, --gb, --g 22%, --gm 55%,
/// --gs)`, lit from the top left.
pub(crate) fn gold_ring(theme: &TvTheme) -> Gradient {
    RadialGradient::new(
        UiPosition::top_left(percent(30), percent(25)),
        RadialGradientShape::Circle(percent(FAR_CORNER)),
        vec![
            ColorStop::percent(theme.gold_bright, 0),
            ColorStop::percent(theme.gold, 22),
            ColorStop::percent(theme.gold_mid, 55),
            ColorStop::percent(theme.gold_shadow, 100),
        ],
    )
    .in_color_space(InterpolationColorSpace::Srgba)
    .into()
}

/// A 135° two-stop fill, light first: an element, or the plain plate.
fn diagonal([light, dark]: [Color; 2]) -> Gradient {
    linear(
        LinearGradient::TO_BOTTOM_RIGHT,
        vec![ColorStop::percent(light, 0), ColorStop::percent(dark, 100)],
    )
}

/// A figure's ring: its element's gradient, or §3.1's gold for a figure with
/// none (the module's note says why).
pub(crate) fn ring(theme: &TvTheme, element: Option<Element>) -> Gradient {
    theme
        .element(element)
        .map_or_else(|| gold_ring(theme), diagonal)
}

/// A portrait's bezel per §3.1: the ring (its fill, the `--gi` line as a
/// border, the top light and bottom shade, a 1 px black outline and the drop)
/// and the plate 10% in, holding the portrait over the element.
pub(crate) fn spawn_bezel(
    parent: &mut ChildSpawnerCommands,
    theme: &TvTheme,
    element: Option<Element>,
    portrait: Option<Handle<Image>>,
) {
    let depth = 7.0 * BEZEL_SCALE;
    parent
        .spawn((
            Bezel,
            Node {
                width: px(BEZEL),
                height: px(BEZEL),
                flex_shrink: 0.0,
                border: UiRect::all(px(RING_LINE)),
                border_radius: BorderRadius::MAX,
                ..default()
            },
            BorderColor::all(theme.gold_inner),
            BackgroundGradient(vec![
                ring(theme, element),
                // inset 0 3px 4px rgba(255,255,255,0.3) and 0 -3px 4px rgba(0,0,0,0.5)
                fade(LinearGradient::TO_BOTTOM, theme.light, 0.3, depth),
                fade(LinearGradient::TO_TOP, theme.shadow, 0.5, depth),
            ]),
            // Back to front: 0 4px 10px rgba(0,0,0,0.6), then 0 0 0 1px #000.
            BoxShadow(vec![
                ShadowStyle {
                    color: theme.shadow.with_alpha(0.6),
                    x_offset: px(0),
                    y_offset: px(4.0 * BEZEL_SCALE),
                    spread_radius: px(0),
                    blur_radius: px(10.0 * BEZEL_SCALE),
                },
                ShadowStyle {
                    color: theme.shadow,
                    x_offset: px(0),
                    y_offset: px(0),
                    spread_radius: px(BEZEL_SCALE),
                    blur_radius: px(0),
                },
            ]),
        ))
        .with_children(|ring| {
            let plate = theme.element(element).unwrap_or(theme.plate);
            let mut spawned = ring.spawn((
                Plate,
                // Absolute inside the ring's line.
                Node {
                    position_type: PositionType::Absolute,
                    left: px(PLATE_INSET - RING_LINE),
                    top: px(PLATE_INSET - RING_LINE),
                    width: px(BEZEL - 2.0 * PLATE_INSET),
                    height: px(BEZEL - 2.0 * PLATE_INSET),
                    border: UiRect::all(px(PLATE_EDGE)),
                    border_radius: BorderRadius::MAX,
                    ..default()
                },
                BorderColor::all(theme.shadow.with_alpha(0.6)),
                BackgroundGradient(vec![
                    diagonal(plate),
                    // inset 0 4px 8px rgba(255,255,255,0.25), 0 -6px 10px rgba(0,0,0,0.5):
                    // under the portrait, so they show only on a plate without one.
                    fade(
                        LinearGradient::TO_BOTTOM,
                        theme.light,
                        0.25,
                        12.0 * BEZEL_SCALE,
                    ),
                    fade(
                        LinearGradient::TO_TOP,
                        theme.shadow,
                        0.5,
                        16.0 * BEZEL_SCALE,
                    ),
                ]),
            ));
            if let Some(image) = portrait {
                spawned.insert(ImageNode::new(image));
            }
        });
}

/// §3.3's framed panel as a figure card: the blue panel under its warm top
/// glow (an ellipse 120% × 80% from the top centre, gone at 60%), edged in
/// the gold run `--gb`, `--gm` 30%, `--gb` 50%, `--gm` 70%, `--gb` at 135°.
/// The edge's width is the card node's border.
pub(crate) fn card(theme: &TvTheme) -> impl Bundle + use<> {
    let [top, middle, bottom] = theme.panel;
    (
        BackgroundGradient(vec![
            linear(
                LinearGradient::TO_BOTTOM,
                vec![
                    ColorStop::percent(top, 0),
                    ColorStop::percent(middle, 50),
                    ColorStop::percent(bottom, 100),
                ],
            ),
            RadialGradient::new(
                UiPosition::TOP,
                RadialGradientShape::Ellipse(percent(120), percent(80)),
                vec![
                    ColorStop::percent(theme.panel_glow, 0),
                    ColorStop::percent(theme.panel_glow.with_alpha(0.0), 60),
                ],
            )
            .in_color_space(InterpolationColorSpace::Srgba)
            .into(),
        ]),
        BorderGradient(vec![gold_run(theme)]),
    )
}

/// §3.3's border: bright and mid gold alternating on the diagonal, so the
/// edge catches light twice along its length instead of reading flat.
fn gold_run(theme: &TvTheme) -> Gradient {
    let (bright, mid) = (theme.gold_bright, theme.gold_mid);
    linear(
        LinearGradient::TO_BOTTOM_RIGHT,
        vec![
            ColorStop::percent(bright, 0),
            ColorStop::percent(mid, 30),
            ColorStop::percent(bright, 50),
            ColorStop::percent(mid, 70),
            ColorStop::percent(bright, 100),
        ],
    )
}

/// §3.2's blue card at rest, on a button's face: the three-stop fill with
/// its top light and bottom shade lines (`inset 0 ±2px 0`) just inside a
/// solid `--gb` edge ([`BUTTON_BORDER`] wide, the one gold §3.2 keeps flat:
/// "always full-bright gold"), over the offset drop that gives it depth
/// (`0 3px 0`) and an ambient shadow (`0 5px 12px`).
pub(crate) fn blue_card(theme: &TvTheme) -> impl Bundle + use<> {
    (
        blue_card_face(theme, 0.0),
        BorderColor::all(theme.gold_bright),
    )
}

/// The face's fill and shadows `lit` of the way (0 to 1, clamped) from
/// §3.2's rest look to its focused one: the brightened gradient (the phone's
/// hover) and §3.1's picking glow (`0 0 22px 4px` at twice the phone's size,
/// as the rest of the button is, through [`css_shadow`]) between the two
/// drops. At 0 it's the rest look exactly, with no glow drawn.
pub(crate) fn blue_card_face(theme: &TvTheme, lit: f32) -> (BackgroundGradient, BoxShadow) {
    let lit = lit.clamp(0.0, 1.0);
    let [top, middle, bottom] = std::array::from_fn(|i| {
        let rest = Srgba::from(theme.blue_card[i]);
        Color::from(rest.mix(&Srgba::from(theme.blue_card_lit[i]), lit))
    });
    let width = 2.0 * BUTTON_SCALE;
    let fill = BackgroundGradient(vec![
        linear(
            LinearGradient::TO_BOTTOM,
            vec![
                ColorStop::percent(top, 0),
                ColorStop::percent(middle, 55),
                ColorStop::percent(bottom, 100),
            ],
        ),
        line(
            LinearGradient::TO_BOTTOM,
            theme.light,
            0.25,
            BUTTON_BORDER,
            width,
        ),
        line(
            LinearGradient::TO_TOP,
            theme.shadow,
            0.25,
            BUTTON_BORDER,
            width,
        ),
    ]);
    // Back to front: 0 5px 12px rgba(0,0,0,0.4), the glow, then 0 3px 0
    // rgba(0,0,0,0.45), the order the phone's hover stacks them in.
    let mut shadows = vec![ShadowStyle {
        color: theme.shadow.with_alpha(0.4),
        x_offset: px(0),
        y_offset: px(5.0 * BUTTON_SCALE),
        spread_radius: px(0),
        blur_radius: px(12.0 * BUTTON_SCALE),
    }];
    if lit > 0.0 {
        let glow = theme.focus_glow;
        shadows.push(css_shadow(
            glow.with_alpha(glow.alpha() * lit),
            Vec2::ZERO,
            22.0 * BUTTON_SCALE,
            4.0 * BUTTON_SCALE,
        ));
    }
    shadows.push(ShadowStyle {
        color: theme.shadow.with_alpha(0.45),
        x_offset: px(0),
        y_offset: px(3.0 * BUTTON_SCALE),
        spread_radius: px(0),
        blur_radius: px(0),
    });
    (fill, BoxShadow(shadows))
}

/// A CSS `box-shadow` (`offset`, `blur` and `spread` in design px, already
/// scaled to the TV) as bevy_ui draws the same thing: bevy_ui's blur radius
/// is its Gaussian's sigma where CSS's is twice the sigma, and its spread
/// grows the node's width by the value (the height in proportion) where
/// CSS's grows each side by it (`bevy_ui_render`'s `box_shadow.rs` and
/// `.wesl`). So a CSS shadow passed through as written draws twice as soft
/// and half as spread.
pub(crate) fn css_shadow(color: Color, offset: Vec2, blur: f32, spread: f32) -> ShadowStyle {
    ShadowStyle {
        color,
        x_offset: px(offset.x),
        y_offset: px(offset.y),
        spread_radius: px(2.0 * spread),
        blur_radius: px(blur / 2.0),
    }
}

/// The owner pip in its card's top-right corner (design decision 8: the
/// phone's bottom-right would sit under player 2's UI), a small bezel as the
/// phone's `.p4-slot-owner`: a gold ring round a plate in the profile's
/// colour. The ring is the node's border, so the node's own fill is the
/// plate's, which is what the mirror reads.
pub(crate) fn pip(theme: &TvTheme, fill: Color) -> impl Bundle + use<> {
    let depth = 2.5 * PIP_SCALE;
    // inset 0 0 0 1px var(--gi): a hard stop just inside the ring's edge.
    let line_at = 100.0 * (1.0 - PIP_SCALE / (PIP / 2.0));
    let inner_line = RadialGradient::new(
        UiPosition::CENTER,
        RadialGradientShape::ClosestSide,
        vec![
            ColorStop::percent(theme.gold_inner.with_alpha(0.0), line_at),
            ColorStop::percent(theme.gold_inner, line_at),
        ],
    )
    .in_color_space(InterpolationColorSpace::Srgba);
    (
        Node {
            position_type: PositionType::Absolute,
            top: px(PIP_INSET),
            right: px(PIP_INSET),
            width: px(PIP),
            height: px(PIP),
            border: UiRect::all(px(PIP_RING)),
            border_radius: BorderRadius::MAX,
            align_items: AlignItems::Center,
            justify_content: JustifyContent::Center,
            ..default()
        },
        BackgroundColor(fill),
        BorderGradient(vec![
            gold_ring(theme),
            // inset 0 1px 1.5px rgba(255,255,255,0.3), 0 -1px 1.5px rgba(0,0,0,0.5)
            fade(LinearGradient::TO_BOTTOM, theme.light, 0.3, depth),
            fade(LinearGradient::TO_TOP, theme.shadow, 0.5, depth),
            inner_line.into(),
        ]),
        // Back to front: 0 2px 4px rgba(0,0,0,0.55), then 0 0 0 1px #000.
        BoxShadow(vec![
            ShadowStyle {
                color: theme.shadow.with_alpha(0.55),
                x_offset: px(0),
                y_offset: px(2.0 * PIP_SCALE),
                spread_radius: px(0),
                blur_radius: px(4.0 * PIP_SCALE),
            },
            ShadowStyle {
                color: theme.shadow,
                x_offset: px(0),
                y_offset: px(0),
                spread_radius: px(PIP_SCALE),
                blur_radius: px(0),
            },
        ]),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stops(gradient: &Gradient) -> Vec<Color> {
        match gradient {
            Gradient::Linear(g) => g.stops.iter().map(|s| s.color).collect(),
            Gradient::Radial(g) => g.stops.iter().map(|s| s.color).collect(),
            Gradient::Conic(g) => g.stops.iter().map(|s| s.color).collect(),
        }
    }

    #[test]
    fn a_figures_ring_is_its_element_and_without_one_it_is_gold() {
        use Element::*;
        let theme = TvTheme::default();
        for element in [
            Air, Dark, Earth, Fire, Life, Light, Magic, Tech, Undead, Water,
        ] {
            let Gradient::Linear(g) = ring(&theme, Some(element)) else {
                panic!("{element:?}'s ring isn't its 135° gradient");
            };
            assert_eq!(g.angle, 135f32.to_radians(), "{element:?}");
            let [light, dark] = theme.element(Some(element)).unwrap();
            assert_eq!(stops(&g.into()), [light, dark], "{element:?}");
        }
        let Gradient::Radial(g) = ring(&theme, None) else {
            panic!("a figure with no element has a gold ring");
        };
        assert_eq!(
            g.position,
            UiPosition::top_left(percent(30), percent(25)),
            "lit from the top left"
        );
        assert_eq!(
            stops(&g.into()),
            [
                theme.gold_bright,
                theme.gold,
                theme.gold_mid,
                theme.gold_shadow
            ]
        );
    }

    #[test]
    fn no_gold_surface_is_flat() {
        // §1: gold always runs through several stops.
        let theme = TvTheme::default();
        let gold = [
            theme.gold_bright,
            theme.gold,
            theme.gold_mid,
            theme.gold_shadow,
            theme.gold_inner,
        ];
        for (what, gradient) in [("ring", gold_ring(&theme)), ("edge", gold_run(&theme))] {
            let colours = stops(&gradient);
            assert!(colours.iter().all(|c| gold.contains(c)), "{what}");
            let mut distinct = colours.clone();
            distinct.dedup();
            assert!(distinct.len() >= 3, "{what} has {} stops", distinct.len());
        }
    }

    #[test]
    fn the_far_corner_is_where_css_puts_it() {
        let reach = (0.7f32.powi(2) + 0.75f32.powi(2)).sqrt() * 100.0;
        assert!((reach - FAR_CORNER).abs() < 0.05, "{reach}");
    }
}
