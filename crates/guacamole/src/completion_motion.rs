//! The completion screen's tally in motion (openspec AS.607): what the
//! objectives screen's scripts animate while the pads are locked, timed on
//! one clock from the tally's start ([`super::completion::TabCycle`]'s
//! `clock`) so a star still slamming in carries over the next tab's
//! rebuild. Read off the scripts' keyframers (`UI Animations` in
//! `global.bld`, through `Create Entry`, `Create Star` and their `Animation
//! -` macros) and written here in our own numbers:
//!
//! - each row slides in from 20 units above its line and fades in over
//!   0.1666 s, at its own (k + 1) * 0.15 s on its tab ([`Anim::Slide`]);
//! - a row the run completed has its check slammed in 0.2 s after its row
//!   came in: from nothing to 2.9 times its size at 180/255 alpha, 3 times
//!   whole, 0.9, then its own size, 0.166 s a step ([`Anim::Slam`]);
//! - once a tab's last row is in (its check's slam or its slide), its
//!   earned star is made, the star's landing sound starting with it, and
//!   0.2 s on slams in turning: whole at 3 times its size from 216 degrees
//!   round to upright, then down to its size, 0.766 s in all
//!   ([`Anim::Star`]); at the slam's last step the screen shakes 4 units,
//!   three quarters of that across, dying over 0.25 s in 0.05 s steps
//!   (`Keyframer Logic - Shake Screen on Star Slam`, `MainUI - Animate
//!   Sprite Shake`); a tab without rows shows its star at once, unslammed.
//!
//! Between keys a value moves in a straight line; the keyframers' eases
//! aren't read (stand-in ledger: `tally-motion`). Once the tally's over
//! everything stands at its last key, and a tab a player moves to after it
//! is drawn still.

use bevy::prelude::*;
use guacamole::level_end::{LevelResults, Status};

use crate::completion::{NEXT_TAB_DELAY, ROW_DELAY, TABS, TabCycle, tab_time};

/// One key of a keyframer: reached `secs` after the key before it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Key {
    pub(crate) secs: f32,
    pub(crate) alpha: f32,
    pub(crate) scale: f32,
    /// Degrees, clockwise.
    pub(crate) turn: f32,
}

const fn key(secs: f32, alpha: f32, scale: f32, turn: f32) -> Key {
    Key {
        secs,
        alpha,
        scale,
        turn,
    }
}

/// What a piece of the tally does as it comes in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Anim {
    /// A row: down from [`SLIDE_RISE`] above its line, fading in.
    Slide,
    /// A completed row's check.
    Slam,
    /// A tab's earned star.
    Star,
}

/// How far above its line a row starts, in the screen's units.
pub(crate) const SLIDE_RISE: f32 = 20.0;
/// The slide's one key.
pub(crate) const SLIDE: [Key; 1] = [key(0.1666, 1.0, 1.0, 0.0)];
/// The check's slam, from nothing.
pub(crate) const SLAM: [Key; 4] = [
    key(0.166, 180.0 / 255.0, 2.9, 0.0),
    key(0.166, 1.0, 3.0, 0.0),
    key(0.166, 1.0, 0.9, 0.0),
    key(0.166, 1.0, 1.0, 0.0),
];
/// The star's turn as it's made.
pub(crate) const STAR_TURN: f32 = 216.0;
/// The star's slam, from nothing at [`STAR_TURN`].
pub(crate) const STAR: [Key; 5] = [
    key(0.1, 1.0, 3.0, 216.0),
    key(0.1, 1.0, 3.0, 356.0),
    key(0.2, 1.0, 3.0, 359.9),
    key(0.166, 1.0, 1.0, 359.9),
    key(0.2, 1.0, 1.0, 359.9),
];
/// The check's slam after its row's slide starts.
pub(crate) const CHECK_DELAY: f32 = 0.2;
/// The star's slam after it's made.
pub(crate) const STAR_DELAY: f32 = 0.2;
/// The star's key whose start shakes the screen.
pub(crate) const SHAKE_KEY: usize = 4;
/// The shake: its size in units (down; across three quarters of it), how
/// long it dies over and how often it moves.
pub(crate) const SHAKE_UNITS: f32 = 4.0;
pub(crate) const SHAKE_ACROSS: f32 = 0.75;
pub(crate) const SHAKE_SECS: f32 = 0.25;
pub(crate) const SHAKE_STEP: f32 = 0.05;

impl Anim {
    pub(crate) fn keys(self) -> &'static [Key] {
        match self {
            Anim::Slide => &SLIDE,
            Anim::Slam => &SLAM,
            Anim::Star => &STAR,
        }
    }

    /// Where it starts from.
    fn from(self) -> Key {
        match self {
            Anim::Slide => key(0.0, 0.0, 1.0, 0.0),
            Anim::Slam => key(0.0, 0.0, 0.0, 0.0),
            Anim::Star => key(0.0, 0.0, 0.0, STAR_TURN),
        }
    }

    /// Seconds from its start to its last key.
    pub(crate) fn secs(self) -> f32 {
        self.keys().iter().map(|k| k.secs).sum()
    }
}

/// A piece's pose: its alpha, scale and turn, and how far above its place
/// it stands (units).
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Pose {
    pub(crate) alpha: f32,
    pub(crate) scale: f32,
    pub(crate) turn: f32,
    pub(crate) rise: f32,
}

impl Pose {
    /// Standing in its place.
    pub(crate) const STILL: Pose = Pose {
        alpha: 1.0,
        scale: 1.0,
        turn: 0.0,
        rise: 0.0,
    };
}

/// `anim`'s pose `t` seconds after its start (before it: where it starts
/// from, unseen).
pub(crate) fn pose(anim: Anim, t: f32) -> Pose {
    let mut from = anim.from();
    let mut at = 0.0;
    let mut out = None;
    for k in anim.keys() {
        if t < at + k.secs {
            let f = ((t - at) / k.secs).clamp(0.0, 1.0);
            let lerp = |a: f32, b: f32| a + (b - a) * f;
            out = Some(key(
                0.0,
                lerp(from.alpha, k.alpha),
                lerp(from.scale, k.scale),
                lerp(from.turn, k.turn),
            ));
            break;
        }
        at += k.secs;
        from = *k;
    }
    let out = out.unwrap_or(from);
    let rise = match anim {
        Anim::Slide => SLIDE_RISE * (1.0 - (t / SLIDE[0].secs).clamp(0.0, 1.0)),
        _ => 0.0,
    };
    Pose {
        alpha: out.alpha,
        scale: out.scale,
        turn: out.turn % 360.0,
        rise,
    }
}

/// When each piece of tab `tab` comes in, on the tab's own clock: each
/// row's slide and, for a row the run completed, its check's slam; and the
/// end of its last row's own (`last entry keyframer ref`), when its star's
/// made. `None` there for a tab without rows.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct TabTimes {
    pub(crate) rows: Vec<(f32, Option<f32>)>,
    pub(crate) last_in: Option<f32>,
}

pub(crate) fn tab_times(results: Option<&LevelResults>, tab: usize) -> TabTimes {
    let rows: Vec<(f32, Option<f32>)> = results
        .into_iter()
        .flat_map(|r| r.objectives.iter())
        .filter(|o| o.category as usize == tab)
        .enumerate()
        .map(|(k, o)| {
            let slide = (k + 1) as f32 * ROW_DELAY;
            (
                slide,
                (o.status == Status::NewlyDone).then_some(slide + CHECK_DELAY),
            )
        })
        .collect();
    let last_in = rows.last().map(|&(slide, slam)| match slam {
        Some(s) => s + Anim::Slam.secs(),
        None => slide + Anim::Slide.secs(),
    });
    TabTimes { rows, last_in }
}

/// When tab `tab` comes up on the tally's clock: the tabs before it, each
/// its rows and the screen's delay.
pub(crate) fn tab_start(results: Option<&LevelResults>, tab: usize) -> f32 {
    (0..tab).map(|t| tab_time(results, t)).sum()
}

/// On the tally's clock, when tab `tab`'s star is made (its landing sound
/// with it), if the tab's earned; `Some(None)` for an earned tab without
/// rows, whose star shows unslammed from its tab's start.
pub(crate) fn star_made(results: Option<&LevelResults>, tab: usize) -> Option<Option<f32>> {
    let earned = results.is_some_and(|r| r.stars.get(tab).copied().unwrap_or(false));
    earned.then(|| {
        tab_times(results, tab)
            .last_in
            .map(|t| tab_start(results, tab) + t)
    })
}

/// The screen's shake at `clock` on the tally's clock, in units (x across,
/// y down): each slammed star's from the start of its [`SHAKE_KEY`],
/// dying over [`SHAKE_SECS`], its side flipping every [`SHAKE_STEP`].
pub(crate) fn shake(results: Option<&LevelResults>, clock: f32) -> Vec2 {
    let at_key: f32 = STAR[..SHAKE_KEY].iter().map(|k| k.secs).sum();
    let mut out = Vec2::ZERO;
    for tab in 0..TABS.len() {
        let Some(Some(made)) = star_made(results, tab) else {
            continue;
        };
        let t = clock - (made + STAR_DELAY + at_key);
        if !(0.0..SHAKE_SECS).contains(&t) {
            continue;
        }
        let left = 1.0 - t / SHAKE_SECS;
        let side = if (t / SHAKE_STEP) as u32 % 2 == 0 {
            1.0
        } else {
            -1.0
        };
        out += Vec2::new(SHAKE_ACROSS, 1.0) * SHAKE_UNITS * left * side;
    }
    out
}

/// A piece of the tally that comes in by `anim` at `start` on the tally's
/// clock.
#[derive(Component, Debug, Clone, Copy, PartialEq)]
pub(crate) struct TallyMotion {
    pub(crate) anim: Anim,
    pub(crate) start: f32,
}

/// The panel the screen's shake moves.
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct TallyShake;

/// An image's or a text's own alpha, kept as a faded piece first meets it.
#[derive(Component, Debug, Clone, Copy, PartialEq)]
pub(crate) struct OwnAlpha(f32);

/// The screen's units to the TV's pixels.
fn px_of(units: f32) -> f32 {
    units * crate::completion::UNIT
}

/// Each piece of the tally posed at the tally's clock (still once it's
/// over), its alpha laid on it and everything under it; the panel shaken.
#[allow(clippy::type_complexity)]
pub(crate) fn pose_tally(
    mut commands: Commands,
    cycle: Res<TabCycle>,
    world: Option<Res<guacamole::scripts::ScriptWorld>>,
    mut pieces: Query<(Entity, &TallyMotion, &mut UiTransform)>,
    mut panels: Query<&mut UiTransform, (With<TallyShake>, Without<TallyMotion>)>,
    children: Query<&Children>,
    mut images: Query<(&mut ImageNode, Option<&OwnAlpha>)>,
    mut texts: Query<(&mut TextColor, Option<&OwnAlpha>), Without<ImageNode>>,
) {
    let clock = if cycle.running() {
        cycle.clock
    } else {
        f32::INFINITY
    };
    for (entity, motion, mut transform) in &mut pieces {
        let p = if clock.is_finite() {
            pose(motion.anim, clock - motion.start)
        } else {
            Pose::STILL
        };
        transform.set_if_neq(UiTransform {
            translation: Val2::px(0.0, -px_of(p.rise)),
            scale: Vec2::splat(p.scale),
            rotation: Rot2::degrees(p.turn),
        });
        let mut stack = vec![entity];
        while let Some(e) = stack.pop() {
            if let Ok((mut image, own)) = images.get_mut(e) {
                let base = own.map_or(image.color.alpha(), |o| o.0);
                if own.is_none() {
                    commands.entity(e).insert(OwnAlpha(base));
                }
                let want = base * p.alpha;
                if image.color.alpha() != want {
                    image.color.set_alpha(want);
                }
            } else if let Ok((mut text, own)) = texts.get_mut(e) {
                let base = own.map_or(text.0.alpha(), |o| o.0);
                if own.is_none() {
                    commands.entity(e).insert(OwnAlpha(base));
                }
                let want = base * p.alpha;
                if text.0.alpha() != want {
                    text.0.set_alpha(want);
                }
            }
            stack.extend(children.get(e).into_iter().flatten());
        }
    }
    let results = world
        .as_deref()
        .and_then(|w| w.sequence.completion.as_ref())
        .and_then(|c| c.results.as_ref().ok());
    let s = if clock.is_finite() {
        shake(results, clock)
    } else {
        Vec2::ZERO
    };
    for mut panel in &mut panels {
        panel.set_if_neq(UiTransform::from_translation(Val2::px(
            px_of(s.x),
            px_of(s.y),
        )));
    }
}

/// How long the tally lasts by its tabs, unheld by fireworks: each tab's
/// rows and the screen's delay (the check's tests).
#[cfg(test)]
pub(crate) fn tally_secs(results: Option<&LevelResults>) -> f32 {
    tab_start(results, TABS.len() - 1) + tab_time(results, TABS.len() - 1)
}

#[cfg(test)]
mod tests {
    use super::*;
    use guacamole::level_end::{Objective, ObjectiveKind};

    fn o(category: i32, status: Status) -> Objective {
        Objective {
            category,
            kind: ObjectiveKind::Check,
            status,
            description: None,
            count: 0.0,
            total: 0.0,
        }
    }

    fn results() -> LevelResults {
        LevelResults {
            objectives: vec![
                o(0, Status::Done),
                o(0, Status::NewlyDone),
                o(1, Status::Open),
                o(1, Status::Open),
                o(1, Status::NewlyDone),
            ],
            stars: vec![true, true, true],
            ..LevelResults::default()
        }
    }

    /// The keys as read: a row from 20 units up and unseen to its place
    /// whole in 0.1666 s; a check from nothing through 2.9 and 3 times and
    /// 0.9 to its size at 0.664 s; a star from nothing turned 216 degrees,
    /// whole at 3 times by 0.1 s, upright by 0.4 s, its size at 0.566 s.
    #[test]
    fn each_piece_comes_in_by_its_keys() {
        let near = |a: f32, b: f32| (a - b).abs() < 1e-3;
        let s = pose(Anim::Slide, -0.05);
        assert!(near(s.alpha, 0.0) && near(s.rise, SLIDE_RISE), "{s:?}");
        let s = pose(Anim::Slide, 0.0833);
        assert!(near(s.alpha, 0.5) && near(s.rise, 10.0), "{s:?}");
        assert_eq!(pose(Anim::Slide, 0.2), Pose::STILL);

        let c = pose(Anim::Slam, -0.01);
        assert!(near(c.alpha, 0.0) && near(c.scale, 0.0), "{c:?}");
        let c = pose(Anim::Slam, 0.166);
        assert!(near(c.scale, 2.9) && near(c.alpha, 180.0 / 255.0), "{c:?}");
        let c = pose(Anim::Slam, 0.332);
        assert!(near(c.scale, 3.0) && near(c.alpha, 1.0), "{c:?}");
        assert!(near(pose(Anim::Slam, 0.498).scale, 0.9));
        assert!(near(Anim::Slam.secs(), 0.664));
        assert_eq!(pose(Anim::Slam, 0.7), Pose::STILL);

        let t = pose(Anim::Star, 0.0);
        assert!(near(t.scale, 0.0) && near(t.turn, STAR_TURN), "{t:?}");
        let t = pose(Anim::Star, 0.1);
        assert!(near(t.scale, 3.0) && near(t.turn, 216.0), "{t:?}");
        let t = pose(Anim::Star, 0.2);
        assert!(near(t.turn, 356.0), "{t:?}");
        let t = pose(Anim::Star, 0.566);
        assert!(near(t.scale, 1.0) && near(t.alpha, 1.0), "{t:?}");
        assert!(near(Anim::Star.secs(), 0.766));
    }

    /// The tab's times: row k slides at (k + 1) * 0.15, a completed one's
    /// check 0.2 later; the tab's last row in at its check's end (or its
    /// slide's); the star made then on the tally's clock, past the tabs
    /// before; none for an unearned tab, unslammed for an empty one.
    #[test]
    fn the_rows_checks_and_stars_come_in_at_the_scripts_times() {
        let r = results();
        let t = tab_times(Some(&r), 0);
        assert_eq!(t.rows.len(), 2);
        assert!((t.rows[0].0 - 0.15).abs() < 1e-5 && t.rows[0].1.is_none());
        assert!((t.rows[1].1.unwrap() - 0.5).abs() < 1e-5);
        assert!((t.last_in.unwrap() - (0.5 + 0.664)).abs() < 1e-4);
        let t = tab_times(Some(&r), 1);
        assert!((t.last_in.unwrap() - (0.45 + 0.2 + 0.664)).abs() < 1e-4);
        // The tab's last row in before the next tab comes up.
        for tab in 0..3 {
            if let Some(last) = tab_times(Some(&r), tab).last_in {
                assert!(last < tab_time(Some(&r), tab), "tab {tab}");
            }
        }
        let start1 = 2.0 * ROW_DELAY + NEXT_TAB_DELAY;
        assert!((tab_start(Some(&r), 1) - start1).abs() < 1e-5);
        let made = star_made(Some(&r), 1).unwrap().unwrap();
        assert!((made - (start1 + 0.45 + 0.2 + 0.664)).abs() < 1e-4);
        assert_eq!(star_made(Some(&r), 2), Some(None), "no rows: still");
        let mut unearned = r.clone();
        unearned.stars[1] = false;
        assert_eq!(star_made(Some(&unearned), 1), None);
        assert!((tally_secs(Some(&r)) - (5.0 * ROW_DELAY + 3.0)).abs() < 1e-4);
    }

    /// The shake starts at the star's slam's last key and dies over 0.25 s,
    /// flipping side each 0.05 s; nothing outside it or for an unearned
    /// tab.
    #[test]
    fn a_slammed_star_shakes_the_screen_once() {
        let r = results();
        let made = star_made(Some(&r), 0).unwrap().unwrap();
        let at = made + STAR_DELAY + 0.566;
        assert_eq!(shake(Some(&r), at - 0.01), Vec2::ZERO);
        let first = shake(Some(&r), at + 0.001);
        assert!((first.y - SHAKE_UNITS).abs() < 0.05, "{first:?}");
        assert!((first.x - SHAKE_UNITS * SHAKE_ACROSS).abs() < 0.05);
        let next = shake(Some(&r), at + 0.06);
        assert!(next.y < 0.0 && next.y.abs() < first.y, "{next:?}");
        assert_eq!(shake(Some(&r), at + SHAKE_SECS + 0.01), Vec2::ZERO);
        let mut unearned = r.clone();
        unearned.stars[0] = false;
        assert_eq!(shake(Some(&unearned), at + 0.001), Vec2::ZERO);
    }
}
