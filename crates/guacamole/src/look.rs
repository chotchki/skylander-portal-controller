//! A choice's focus look and its press (openspec AP.13, design decision 9),
//! on the button's face: the node inside the `Button` that wears §3.2's blue
//! card. The `Button` node keeps its rect, which the mirror, the router and
//! the layout tests read; only the face scales, so neither look moves a rect
//! anyone measures.
//!
//! - Focused (and the focus showing, `InputFocusVisible`): §3.2's brightened
//!   gradient plus §3.1's picking state, scale 1.05 and the outer glow,
//!   eased in and out with `--ease-spring` over `--dur-quick`, as the phone's
//!   `.bezel-picking` eases its scale. The scale runs the curve's overshoot;
//!   the fill and the glow, which can't overshoot, follow it clamped.
//! - `Pressed`: the tap, scale 0.96 (the phone's portal slot,
//!   `.p4-slot:active`) with `--ease-tap` over `--dur-tap`, both ways. It
//!   multiplies the focus scale, as the phone's slot does its bezel's.
//!
//! Under [`ReducedMotion`] both land at their end the frame they change, so a
//! shot never catches one mid-way. A face whose focus look changes writes
//! [`Redrawn::Focus`], which arms a shot.

use bevy::input_focus::{InputFocus, InputFocusVisible};
use bevy::prelude::*;
use bevy::ui::Pressed;
// Not the prelude's `Button`: that's bevy_ui's deprecated one.
use bevy::ui_widgets::Button;

use crate::motion::{DUR_QUICK, DUR_TAP, Easing, ReducedMotion, Tween};
use crate::surfaces;
use crate::theme::TvTheme;
use crate::tv::Redrawn;

/// §3.1's picking scale.
pub(crate) const FOCUS_SCALE: f32 = 1.05;

/// The phone's tap dent on a portal slot.
pub(crate) const TAP_SCALE: f32 = 0.96;

/// A button's face, and how far along its focus look and press are.
#[derive(Component, Debug, Clone, Copy, PartialEq)]
pub(crate) struct ChoiceFace {
    focus: Tween,
    press: Tween,
}

impl Default for ChoiceFace {
    /// At rest.
    fn default() -> Self {
        Self {
            focus: Tween::new(Easing::Spring, DUR_QUICK),
            press: Tween::new(Easing::Tap, DUR_TAP),
        }
    }
}

/// The face's scale with the focus look `focus` of the way on and the press
/// `press` of the way down (each 0 at rest and 1 on, past 1 in an overshoot).
pub(crate) fn face_scale(focus: f32, press: f32) -> f32 {
    (1.0 + (FOCUS_SCALE - 1.0) * focus) * (1.0 - (1.0 - TAP_SCALE) * press)
}

/// Each face follows its button: focused and showing it, `Pressed` or not.
pub(crate) fn ease_faces(
    time: Res<Time>,
    reduced: Res<ReducedMotion>,
    theme: Res<TvTheme>,
    focus: Option<Res<InputFocus>>,
    visible: Option<Res<InputFocusVisible>>,
    buttons: Query<Has<Pressed>, With<Button>>,
    mut faces: Query<(
        &ChildOf,
        &mut ChoiceFace,
        &mut UiTransform,
        &mut BackgroundGradient,
        &mut BoxShadow,
    )>,
    mut redrawn: MessageWriter<Redrawn>,
) {
    let now = time.elapsed();
    let focused = focus.and_then(|f| f.get());
    let showing = visible.is_some_and(|v| v.0);
    let mut moved = false;
    for (button, mut face, mut transform, mut fill, mut shadows) in &mut faces {
        let button = button.parent();
        let on = showing && focused == Some(button);
        let pressed = buttons.get(button).unwrap_or(false);
        // Only this system reads the tweens, so aiming them marks nothing
        // changed; the look below changes only when its values do.
        let face = face.bypass_change_detection();
        moved |= face.focus.aim(f32::from(u8::from(on)), now);
        face.press.aim(f32::from(u8::from(pressed)), now);
        if reduced.0 {
            face.focus.settle();
            face.press.settle();
        }
        let (lit, press) = (face.focus.at(now), face.press.at(now));
        transform.set_if_neq(UiTransform::from_scale(Vec2::splat(face_scale(lit, press))));
        let (new_fill, new_shadows) = surfaces::blue_card_face(&theme, lit);
        fill.set_if_neq(new_fill);
        shadows.set_if_neq(new_shadows);
    }
    if moved {
        redrawn.write(Redrawn::Focus);
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;

    const MS: Duration = Duration::from_millis(1);

    /// The face's scale every millisecond of `tween` from `start` for `ms`.
    fn scales(tween: &Tween, start: Duration, ms: u32, scale: impl Fn(f32) -> f32) -> Vec<f32> {
        (0..=ms).map(|i| scale(tween.at(start + i * MS))).collect()
    }

    #[test]
    fn the_focus_pop_starts_at_rest_overshoots_and_ends_at_1_05() {
        let mut focus = ChoiceFace::default().focus;
        focus.aim(1.0, Duration::ZERO);
        let s = scales(&focus, Duration::ZERO, 300, |f| face_scale(f, 0.0));
        assert_eq!(s[0], 1.0, "starts at rest");
        let (at, top) =
            s.iter().enumerate().fold(
                (0, f32::MIN),
                |b, (i, v)| if *v > b.1 { (i, *v) } else { b },
            );
        assert!(top > FOCUS_SCALE && top < 1.052, "overshoots to {top}");
        assert!((100..150).contains(&at), "at {at} ms");
        assert_eq!(s[200], FOCUS_SCALE, "ends on 1.05 at --dur-quick");
        assert!(s[200..].iter().all(|v| *v == FOCUS_SCALE), "and stays");
        // Losing the focus eases back the same way, dipping just under rest.
        focus.aim(0.0, 300 * MS);
        let s = scales(&focus, 300 * MS, 300, |f| face_scale(f, 0.0));
        assert_eq!(s[0], FOCUS_SCALE);
        let low = s.iter().copied().fold(f32::MAX, f32::min);
        assert!(low < 1.0 && low > 0.998, "dips to {low}");
        assert_eq!(s[200], 1.0);
    }

    #[test]
    fn the_tap_dips_past_0_96_and_springs_back() {
        let mut press = ChoiceFace::default().press;
        press.aim(1.0, Duration::ZERO);
        let s = scales(&press, Duration::ZERO, 200, |p| face_scale(0.0, p));
        assert_eq!(s[0], 1.0, "starts at rest");
        let low = s.iter().copied().fold(f32::MAX, f32::min);
        // --ease-tap runs about a third past its end: 1 - 0.04 × 1.33.
        assert!(low < 0.95 && low > 0.945, "dips to {low}");
        assert_eq!(s[120], TAP_SCALE, "ends on 0.96 at --dur-tap");
        // Let go: back up past rest and down to it.
        press.aim(0.0, 200 * MS);
        let s = scales(&press, 200 * MS, 200, |p| face_scale(0.0, p));
        assert_eq!(s[0], TAP_SCALE);
        let high = s.iter().copied().fold(f32::MIN, f32::max);
        assert!(high > 1.0 && high < 1.015, "springs to {high}");
        assert_eq!(s[120], 1.0);
    }

    #[test]
    fn a_press_on_a_focused_face_multiplies_its_pop() {
        assert_eq!(face_scale(0.0, 0.0), 1.0);
        assert_eq!(face_scale(1.0, 0.0), FOCUS_SCALE);
        assert_eq!(face_scale(0.0, 1.0), TAP_SCALE);
        assert!((face_scale(1.0, 1.0) - FOCUS_SCALE * TAP_SCALE).abs() < 1e-6);
    }

    #[test]
    fn the_fill_and_the_glow_follow_the_focus_without_overshooting() {
        let theme = TvTheme::default();
        let stops = |lit: f32| {
            let (fill, shadows) = surfaces::blue_card_face(&theme, lit);
            let bevy::ui::Gradient::Linear(base) = &fill.0[0] else {
                panic!("{fill:?}");
            };
            let colours: Vec<Color> = base.stops.iter().map(|s| s.color).collect();
            (colours, shadows.0)
        };
        let (rest, shadows) = stops(0.0);
        assert_eq!(rest, theme.blue_card, "at rest");
        assert_eq!(shadows.len(), 2, "no glow at rest");
        let (lit, shadows) = stops(1.0);
        assert_eq!(lit, theme.blue_card_lit, "focused");
        assert_eq!(shadows.len(), 3);
        assert_eq!(
            shadows[1].color, theme.focus_glow,
            "the glow, between the drops"
        );
        // 0 0 22px 4px at twice the phone's size, as bevy_ui reads a blur
        // (the sigma, half CSS's) and a spread (the whole width's growth).
        assert_eq!(shadows[1].blur_radius, Val::Px(22.0));
        assert_eq!(shadows[1].spread_radius, Val::Px(16.0));
        assert_eq!(
            (shadows[1].x_offset, shadows[1].y_offset),
            (Val::Px(0.0), Val::Px(0.0))
        );
        // Past the end (the spring's overshoot) is the end; before it, rest.
        assert_eq!(stops(1.03), stops(1.0));
        assert_eq!(stops(-0.01), stops(0.0));
        // Half way: every stop between its two ends, the glow half as strong.
        let (half, shadows) = stops(0.5);
        for ((h, r), l) in half.iter().zip(&rest).zip(&lit) {
            let [h, r, l] = [*h, *r, *l].map(|c| Srgba::from(c).to_f32_array());
            for c in 0..3 {
                let (lo, hi) = (r[c].min(l[c]), r[c].max(l[c]));
                assert!(lo < h[c] && h[c] < hi, "{h:?} between {r:?} and {l:?}");
            }
        }
        assert!((shadows[1].color.alpha() - 0.375).abs() < 1e-6);
    }
}
