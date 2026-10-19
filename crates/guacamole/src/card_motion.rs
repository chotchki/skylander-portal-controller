//! The portal's cards in motion (openspec AP.14, design decision 9), from
//! `docs/aesthetic/design_language.md` §5:
//!
//! - The rise (§5.1): on the portal's entrance each card comes up from 24
//!   design px below its place at 0.97 over 500 ms of `--ease-spring`, 80 ms
//!   after the card before it. A placement while the portal is up rises
//!   alone, at once.
//! - The impact (§5.4): a placement flashes white to gold over its bezel, 0
//!   to 2× over `--dur-impact`, brightest 30% in, each leg on CSS's
//!   `ease-out` as the phone's keyframes run it. The card's face clips the
//!   flash, so at 2× it never covers a neighbour.
//! - The idle float (§5.3): a loaded bezel breathes ±2 design px over
//!   `--dur-idle-float`, each slot a fixed share of the period off its
//!   neighbours (the phone staggers with negative delays).
//! - One focal animation at a time (§5.6): a bezel's float waits for its
//!   card's rise and impact to end, then eases in over `--dur-quick`, so it
//!   never jumps.
//!
//! Only inner nodes move (design decision 9): the card's face (the node
//! inside `CardSlot` that wears the card) rises, the bezel on it floats and
//! the flash sits over the bezel. `CardSlot` keeps the rect the mirror, the
//! layout report and the tests read. Under [`ReducedMotion`] a card is at rest
//! from the frame it's drawn: risen, no flash, no float.
//!
//! The rise is §5.1's translate and scale without its fade (design decision
//! 9's default, put to chotchki under AP.16): bevy_ui 0.20 has no opacity for
//! a subtree, so a fade would mean easing every colour on the card. A card
//! waiting for its turn is hidden, as the CSS's from-frame (`opacity: 0`,
//! held through the delay by the phone's `both` fill) hides it, and shows at
//! the start of its rise.

use std::f64::consts::TAU;
use std::time::Duration;

use bevy::prelude::*;
use bevy::ui::Val2;

use crate::motion::{DUR_IDLE_FLOAT, DUR_IMPACT, DUR_QUICK, Easing, ReducedMotion};
use crate::surfaces;
use crate::theme::TvTheme;

/// §5.1: a card's rise, and how long each card of the entrance waits after
/// the one before it.
pub(crate) const RISE: Duration = Duration::from_millis(500);
pub(crate) const STAGGER: Duration = Duration::from_millis(80);

/// §5.1's from-frame: 24 design px below its place, at 0.97.
pub(crate) const RISE_DROP: f32 = 24.0;
pub(crate) const RISE_SCALE: f32 = 0.97;

/// §5.4: the flash grows to twice the bezel and is brightest 30% in.
pub(crate) const FLASH_SCALE: f32 = 2.0;
const FLASH_PEAK: f32 = 0.3;

/// §5.3: how far a bezel floats each way, in design px.
pub(crate) const FLOAT_REACH: f32 = 2.0;

/// The float easing in once the focal animations end: `--dur-quick`, a
/// state's fade.
const FLOAT_IN: Duration = DUR_QUICK;

/// When a card was drawn and what plays on it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Schedule {
    slot: u8,
    /// The clock (`Time::elapsed`) in the frame it was drawn.
    drawn: Duration,
    /// How long it waits before it rises.
    delay: Duration,
    /// A placement, which flashes; the entrance doesn't.
    impact: bool,
}

/// How long the `order`-th card of the entrance (0 first) waits.
pub(crate) fn stagger(order: usize) -> Duration {
    STAGGER * order as u32
}

impl Schedule {
    /// The `order`-th card (0 first, in slot order) of the portal's entrance.
    pub(crate) fn entrance(slot: u8, order: usize, drawn: Duration) -> Self {
        Self {
            slot,
            drawn,
            delay: stagger(order),
            impact: false,
        }
    }

    /// A card placed in `slot` while the portal is up.
    pub(crate) fn placement(slot: u8, drawn: Duration) -> Self {
        Self {
            slot,
            drawn,
            delay: Duration::ZERO,
            impact: true,
        }
    }

    fn age(&self, now: Duration) -> Duration {
        now.saturating_sub(self.drawn)
    }

    /// How long after it's drawn the card's focal animations are over: its
    /// rise and, for a placement, its impact.
    pub(crate) fn focal_end(&self) -> Duration {
        let risen = self.delay + RISE;
        if self.impact {
            risen.max(DUR_IMPACT)
        } else {
            risen
        }
    }

    /// The face at `now`: how far below its place (design px) and at what
    /// scale; none while it waits for its turn, when it's hidden.
    pub(crate) fn rise(&self, now: Duration) -> Option<(f32, f32)> {
        let rising = self.age(now).checked_sub(self.delay)?;
        let left = 1.0 - Easing::Spring.ease(share(rising, RISE));
        Some((RISE_DROP * left, 1.0 - (1.0 - RISE_SCALE) * left))
    }

    /// The flash at `now`: its scale and its alpha (of full strength); none
    /// outside a placement's impact.
    pub(crate) fn flash(&self, now: Duration) -> Option<(f32, f32)> {
        let age = self.age(now);
        if !self.impact || age >= DUR_IMPACT {
            return None;
        }
        let t = share(age, DUR_IMPACT);
        let alpha = if t < FLASH_PEAK {
            Easing::Out.ease(t / FLASH_PEAK)
        } else {
            1.0 - Easing::Out.ease((t - FLASH_PEAK) / (1.0 - FLASH_PEAK))
        };
        Some((FLASH_SCALE * Easing::Out.ease(t), alpha))
    }

    /// The bezel's float at `now` in design px (up is negative): nothing
    /// until the focal animations are over, then easing in to the breath.
    pub(crate) fn float(&self, now: Duration) -> f32 {
        let floating = self.age(now).saturating_sub(self.focal_end());
        Easing::InOut.ease(share(floating, FLOAT_IN)) * breath(self.slot, now)
    }
}

/// `t` as a share of `of`; past 1 once it's over.
fn share(t: Duration, of: Duration) -> f32 {
    t.as_secs_f32() / of.as_secs_f32()
}

/// Slot `slot`'s share of the float's period: 3/8 a slot, wrapped, so a
/// card is 3/8 of a breath off the one beside it and half a breath off the
/// one above or below it (four to a row).
pub(crate) fn phase(slot: u8) -> f64 {
    f64::from(3 * u32::from(slot) % 8) / 8.0
}

/// Where slot `slot`'s breath is at `now`: ±[`FLOAT_REACH`], rising first, on
/// the clock. A sine, which is what §5.3's ease-in-out between its two ends
/// traces.
pub(crate) fn breath(slot: u8, now: Duration) -> f32 {
    let turns = now.as_secs_f64() / DUR_IDLE_FLOAT.as_secs_f64() + phase(slot);
    -(f64::from(FLOAT_REACH) * (TAU * turns).sin()) as f32
}

/// A card's face: the node inside `CardSlot` that wears the card, and rises.
#[derive(Component, Debug, Clone, Copy)]
pub(crate) struct CardFace(pub(crate) Schedule);

/// The bezel that takes §5.3's idle float, held while its card's rise and
/// impact run.
#[derive(Component, Debug, Clone, Copy)]
pub(crate) struct Floats(pub(crate) Schedule);

/// A placement's flash, over its bezel.
#[derive(Component, Debug, Clone, Copy)]
pub(crate) struct ImpactFlash(pub(crate) Schedule);

/// Puts a card's motion on its bezel: the float, and a placement's flash
/// over it as its last child, so it draws over the portrait.
pub(crate) fn on_bezel(mut bezel: EntityCommands, schedule: Schedule, theme: &TvTheme) {
    bezel.insert(Floats(schedule));
    if schedule.impact {
        bezel.with_child((
            ImpactFlash(schedule),
            surfaces::flash(theme),
            Visibility::Hidden,
            Pickable::IGNORE,
        ));
    }
}

type FaceOnly = (Without<Floats>, Without<ImpactFlash>);
type BezelOnly = (Without<CardFace>, Without<ImpactFlash>);
type FlashOnly = (Without<CardFace>, Without<Floats>);

/// Poses every card's face, bezel and flash for this frame, or at rest under
/// [`ReducedMotion`].
pub(crate) fn move_cards(
    time: Res<Time>,
    reduced: Res<ReducedMotion>,
    theme: Res<TvTheme>,
    mut faces: Query<(&CardFace, &mut UiTransform, &mut Visibility), FaceOnly>,
    mut bezels: Query<(&Floats, &mut UiTransform), BezelOnly>,
    mut flashes: Query<
        (
            &ImpactFlash,
            &mut UiTransform,
            &mut Visibility,
            &mut BackgroundGradient,
        ),
        FlashOnly,
    >,
) {
    let now = time.elapsed();
    let still = reduced.0;
    for (face, mut transform, mut shown) in &mut faces {
        let pose = if still {
            Some((0.0, 1.0))
        } else {
            face.0.rise(now)
        };
        let ((drop, scale), visibility) = match pose {
            Some(pose) => (pose, Visibility::Inherited),
            None => ((RISE_DROP, RISE_SCALE), Visibility::Hidden),
        };
        transform.set_if_neq(UiTransform {
            translation: Val2::px(0.0, drop),
            scale: Vec2::splat(scale),
            rotation: Rot2::IDENTITY,
        });
        shown.set_if_neq(visibility);
    }
    for (bezel, mut transform) in &mut bezels {
        let lift = if still { 0.0 } else { bezel.0.float(now) };
        transform.set_if_neq(UiTransform::from_translation(Val2::px(0.0, lift)));
    }
    for (flash, mut transform, mut shown, mut fill) in &mut flashes {
        // Nothing to draw at no size or no alpha: its start, and its end.
        let drawn = |(scale, alpha): &(f32, f32)| !still && *scale > 0.0 && *alpha > 0.0;
        match flash.0.flash(now).filter(drawn) {
            Some((scale, alpha)) => {
                transform.set_if_neq(UiTransform::from_scale(Vec2::splat(scale)));
                fill.set_if_neq(surfaces::flash_fill(&theme, alpha));
                shown.set_if_neq(Visibility::Inherited);
            }
            None => {
                shown.set_if_neq(Visibility::Hidden);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const MS: Duration = Duration::from_millis(1);
    const DRAWN: Duration = Duration::from_secs(7);

    /// Every millisecond from `from` for `ms`, as clock times.
    fn each_ms(from: Duration, ms: u32) -> impl Iterator<Item = Duration> {
        (0..=ms).map(move |i| from + i * MS)
    }

    #[test]
    fn the_entrance_staggers_80_ms_a_card_and_a_placement_waits_for_none() {
        let waits: Vec<u64> = (0..8).map(|k| stagger(k).as_millis() as u64).collect();
        assert_eq!(waits, [0, 80, 160, 240, 320, 400, 480, 560]);
        for order in 0..8 {
            let card = Schedule::entrance(order as u8, order, DRAWN);
            let start = DRAWN + stagger(order);
            assert!(
                each_ms(DRAWN, stagger(order).as_millis() as u32)
                    .filter(|t| *t < start)
                    .all(|t| card.rise(t).is_none()),
                "card {order} waits, hidden"
            );
            assert_eq!(card.rise(start), Some((RISE_DROP, RISE_SCALE)), "{order}");
            assert_eq!(card.flash(start), None, "the entrance doesn't flash");
            assert_eq!(card.focal_end(), stagger(order) + RISE);
        }
        let placed = Schedule::placement(3, DRAWN);
        assert_eq!(placed.rise(DRAWN), Some((RISE_DROP, RISE_SCALE)), "at once");
        assert_eq!(placed.flash(DRAWN), Some((0.0, 0.0)), "and flashes");
    }

    #[test]
    fn a_rise_springs_up_from_24_px_and_0_97_to_rest() {
        let card = Schedule::placement(0, DRAWN);
        let poses: Vec<(f32, f32)> = each_ms(DRAWN, 600).map(|t| card.rise(t).unwrap()).collect();
        assert_eq!(poses[0], (24.0, 0.97));
        // --ease-spring runs about 3% past its end: the card ends a little
        // above its place and over 1 before settling on it.
        let highest = poses.iter().map(|p| p.0).fold(f32::MAX, f32::min);
        let largest = poses.iter().map(|p| p.1).fold(f32::MIN, f32::max);
        assert!((-0.9..-0.5).contains(&highest), "{highest}");
        assert!(largest > 1.0 && largest < 1.0012, "{largest}");
        assert!(
            poses[100].0 > 0.0 && poses[100].0 < 12.0,
            "{:?}",
            poses[100]
        );
        assert!(
            poses[100].1 > 0.97 && poses[100].1 < 1.0,
            "{:?}",
            poses[100]
        );
        assert!(
            poses[500..].iter().all(|p| *p == (0.0, 1.0)),
            "at rest from 500 ms"
        );
    }

    #[test]
    fn the_impact_flashes_out_to_twice_the_bezel_and_fades() {
        let card = Schedule::placement(2, DRAWN);
        let flashes: Vec<(f32, f32)> = each_ms(DRAWN, 599)
            .map(|t| card.flash(t).unwrap())
            .collect();
        assert_eq!(flashes[0], (0.0, 0.0), "from nothing");
        assert!(
            flashes.windows(2).all(|w| w[0].0 <= w[1].0),
            "it only grows"
        );
        assert!(flashes[599].0 > 1.99 && flashes[599].0 <= FLASH_SCALE);
        let (peak_at, peak) =
            flashes.iter().enumerate().fold(
                (0, f32::MIN),
                |b, (i, f)| if f.1 > b.1 { (i, f.1) } else { b },
            );
        assert!(
            (179..=181).contains(&peak_at),
            "brightest 30% in: {peak_at}"
        );
        assert!(peak > 0.999 && peak <= 1.0, "{peak}");
        assert!(flashes[..peak_at].windows(2).all(|w| w[0].1 <= w[1].1));
        assert!(flashes[peak_at..].windows(2).all(|w| w[0].1 >= w[1].1));
        assert!(flashes[599].1 < 0.01, "{:?}", flashes[599]);
        assert_eq!(card.flash(DRAWN + DUR_IMPACT), None, "over at 600 ms");
        assert_eq!(card.flash(DRAWN + 9 * DUR_IMPACT), None);
    }

    /// The box's focal rule: a placement's impact holds its bezel's float
    /// until it ends, and an entrance card's rise holds it the same way.
    #[test]
    fn a_placements_impact_holds_its_float_until_it_ends() {
        for slot in 0..8 {
            let placed = Schedule::placement(slot, DRAWN);
            assert_eq!(
                placed.focal_end(),
                DUR_IMPACT,
                "the impact outlasts the rise"
            );
            assert!(
                each_ms(DRAWN, 600).all(|t| placed.float(t) == 0.0),
                "slot {slot}: held through the impact"
            );
            // Eased in over --dur-quick, then the breath itself.
            let after = DRAWN + DUR_IMPACT;
            let ramp: Vec<f32> = each_ms(after, 200).map(|t| placed.float(t)).collect();
            for (i, lift) in ramp.iter().enumerate() {
                let full = breath(slot, after + i as u32 * MS);
                assert!(lift.abs() <= full.abs() + 1e-6, "slot {slot} at +{i} ms");
            }
            assert!(
                each_ms(after + FLOAT_IN, 4500)
                    .all(|t| (placed.float(t) - breath(slot, t)).abs() < 1e-6),
                "slot {slot}: the whole breath once eased in"
            );
            let reach = each_ms(after + FLOAT_IN, 4500)
                .map(|t| placed.float(t).abs())
                .fold(0.0, f32::max);
            assert!(reach > 1.99 && reach <= FLOAT_REACH + 1e-6, "{reach}");

            let entering = Schedule::entrance(slot, usize::from(slot), DRAWN);
            let risen = DRAWN + entering.focal_end();
            assert!(
                each_ms(DRAWN, (risen - DRAWN).as_millis() as u32)
                    .all(|t| entering.float(t) == 0.0),
                "slot {slot}: held through its wait and its rise"
            );
        }
    }

    #[test]
    fn neighbours_breathe_out_of_step() {
        // Circular distance between two slots' phases, in periods.
        let apart = |a: u8, b: u8| {
            let d = (phase(a) - phase(b)).abs();
            d.min(1.0 - d)
        };
        let mut phases: Vec<f64> = (0..8).map(phase).collect();
        phases.sort_by(f64::total_cmp);
        phases.dedup();
        assert_eq!(phases.len(), 8, "every slot its own phase");
        for slot in 0..8u8 {
            if slot % 4 != 3 {
                assert!(apart(slot, slot + 1) >= 0.25, "{slot} and the next");
            }
            if slot < 4 {
                assert_eq!(apart(slot, slot + 4), 0.5, "{slot} and below");
            }
        }
        // ±2 px, rising first from slot 0's start, once every 4.5 s.
        for slot in 0..8 {
            let at = |t: Duration| breath(slot, t);
            for ms in (0..9000).step_by(37) {
                let t = DRAWN + ms * MS;
                assert!(at(t).abs() <= FLOAT_REACH + 1e-6);
                assert!((at(t) - at(t + DUR_IDLE_FLOAT)).abs() < 1e-3, "{slot} {ms}");
            }
        }
        assert_eq!(breath(0, Duration::ZERO), 0.0);
        assert!(breath(0, 100 * MS) < 0.0, "up first");
        assert!((breath(0, DUR_IDLE_FLOAT / 4) + FLOAT_REACH).abs() < 1e-5);
    }
}
