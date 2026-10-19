//! The TV's sky (openspec AP, design decision 8): `design_language.md` §1's
//! starfield on every screen root, as three stacked gradients (the `--sf-1` to
//! `--sf-3` fall and the glows at the top and bottom edges) under a layer of
//! stars generated here, never a disc or wiki image. The stars drift one loop
//! every 140 s (§5's `--dur-sky-drift`) unless [`ReducedMotion`] holds them.
//!
//! The sky repeats, but never inside one screen up to 32:9: it repeats every
//! [`PERIOD`]`.x` across, wider than any such screen, and otherwise only a
//! loop's [`DRIFT`] away, 1620 design px up or down, taller than any screen
//! (the design height is 1080 whatever the window). Each star is a node of
//! its own, drawn once, at its one copy in the window ([`wrap`]), which runs
//! from just outside the screen's top-left corner to well past its far
//! edges, so a star leaving one edge comes back in at the other off the
//! screen.

use std::time::Duration;

use bevy::asset::RenderAssetUsages;
use bevy::image::ImageSampler;
use bevy::picking::Pickable;
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};
use bevy::ui::{InterpolationColorSpace, RadialGradientShape, UiPosition, Val2};

use crate::motion::ReducedMotion;
use crate::theme::TvTheme;

/// One star's cell, in design px: AP.11's 960×540 tile split 4×4, so the sky
/// is as dense as it was (64 stars on a 16:9 screen).
pub(crate) const CELL: Vec2 = Vec2::new(240.0, 135.0);

/// The sky's period in cells: 17 across, the fewest wider than a 32:9
/// screen (3840 design px) with the window's margins; 12 down, so it's one
/// loop's drift tall.
const COLUMNS: u32 = 17;
const ROWS: u32 = 12;

/// The sky's period, in design px: whole cells each way, so the cells line
/// up across a repeat and a star is never closer to its neighbour there than
/// anywhere else.
pub(crate) const PERIOD: Vec2 = Vec2::new(4080.0, 1620.0);

/// One loop's drift up and to the left, in design px: AP.11's (one 960×540
/// tile across, three up), about the phone's angle (400 px across for 800
/// up) and, at couch distance, about its angular speed. A whole number of
/// cells each way and exactly the period's height, so it's one of the sky's
/// repeats: a loop lands every star on a copy of itself, with no seam.
pub(crate) const DRIFT: Vec2 = Vec2::new(960.0, 1620.0);

/// How far the window starts before the screen's top and left edges, in
/// design px: past the farthest a sprite reaches from its centre (3.5), so a
/// star wrapping there is off the screen on both sides of the jump.
const MARGIN: f32 = 8.0;

/// A sprite's resolution: 1:1 on a 4K target, minified 2:1 at 1080p, as
/// AP.11's tile was.
const TEXELS_PER_PX: f32 = 2.0;

/// The phone's two star sizes (1 and 2 CSS px), at couch distance: a design
/// px on the 86" TV at 3 m subtends about 2/3 of a CSS px on a phone at 30 cm.
const SMALL_RADIUS: f32 = 1.5;
const BIG_RADIUS: f32 = 3.0;

/// The phone's mix: 4 of its 18 stars are big and tinted.
const BIG_SHARE: f32 = 4.0 / 18.0;

/// Clear space between a star's disc and its cell's edge, in design px.
const EDGE: f32 = 1.0;

/// The seed every TV draws its sky from, so every run shows the same stars.
pub(crate) const SEED: u64 = 0x5EED_57A2;

/// `--dur-sky-drift`.
const LOOP: Duration = Duration::from_secs(140);

/// A star's colour: the phone's white, warm (`#ffe6b4`) and cool (`#b4dcff`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Tint {
    White,
    Warm,
    Cool,
}

/// One star of the sky: a soft disc fading from `alpha` at its centre to
/// nothing at `radius`, as the phone's `radial-gradient(.., color, transparent)`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Star {
    /// The centre, in design px from the period's top-left.
    pub(crate) at: Vec2,
    pub(crate) radius: f32,
    pub(crate) tint: Tint,
    pub(crate) alpha: f32,
}

impl Star {
    pub(crate) fn big(&self) -> bool {
        self.radius > SMALL_RADIUS
    }
}

/// The stars' layer on a screen root, holding one [`SkyStar`] a star.
#[derive(Component)]
pub(crate) struct StarLayer;

/// A star drawn on a screen's sky: where it sits in the period with no
/// drift and half its sprite's side, for [`drift`] to place it.
#[derive(Component, Debug, Clone, Copy, PartialEq)]
pub(crate) struct SkyStar {
    pub(crate) at: Vec2,
    pub(crate) half: f32,
}

impl SkyStar {
    /// Where its centre is drawn with the sky drifted by `drift`.
    pub(crate) fn centre(&self, drift: Vec2) -> Vec2 {
        wrap(self.at - drift)
    }

    /// Its node's translation with the sky drifted by `drift`: the sprite's
    /// corner, from the layer's, so its centre lands on [`Self::centre`].
    pub(crate) fn placed(&self, drift: Vec2) -> UiTransform {
        let corner = self.centre(drift) - self.half;
        UiTransform::from_translation(Val2::px(corner.x, corner.y))
    }
}

/// The stars' two sprites, built once a run (`TvAssets`).
#[derive(Debug, Clone)]
pub(crate) struct Sprites {
    pub(crate) small: Handle<Image>,
    pub(crate) big: Handle<Image>,
}

impl Sprites {
    pub(crate) fn new(images: &mut Assets<Image>) -> Self {
        Self {
            small: images.add(sprite_image(SMALL_RADIUS)),
            big: images.add(sprite_image(BIG_RADIUS)),
        }
    }

    fn of(&self, star: &Star) -> Handle<Image> {
        if star.big() {
            self.big.clone()
        } else {
            self.small.clone()
        }
    }
}

/// SplitMix64: a few lines of seeded randomness, so the sky needs no crate.
struct SplitMix64(u64);

impl SplitMix64 {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    /// Uniform in [0, 1).
    fn unit(&mut self) -> f32 {
        (self.next() >> 40) as f32 / (1u64 << 24) as f32
    }
}

/// The sky's stars from `seed`: one in each [`CELL`] of the period at a
/// random spot [`EDGE`] clear of the cell's sides, so there are neither
/// clumps nor holes, even across a repeat; the phone's mix of small white
/// stars at full, 0.8 and 0.7 alpha and big warm or cool ones at 0.9.
pub(crate) fn stars(seed: u64) -> Vec<Star> {
    let mut rng = SplitMix64(seed);
    let mut out = Vec::with_capacity((COLUMNS * ROWS) as usize);
    for row in 0..ROWS {
        for col in 0..COLUMNS {
            let (radius, tint, alpha) = if rng.unit() < BIG_SHARE {
                let tint = if rng.unit() < 0.5 {
                    Tint::Warm
                } else {
                    Tint::Cool
                };
                (BIG_RADIUS, tint, 0.9)
            } else {
                // The phone's 14 small stars: 9 at full alpha, 3 at 0.8, 2 at 0.7.
                let pick = rng.unit() * 14.0;
                let alpha = if pick < 9.0 {
                    1.0
                } else if pick < 12.0 {
                    0.8
                } else {
                    0.7
                };
                (SMALL_RADIUS, Tint::White, alpha)
            };
            let margin = radius + EDGE;
            let corner = Vec2::new(col as f32, row as f32) * CELL + margin;
            let span = CELL - 2.0 * margin;
            let at = corner + Vec2::new(rng.unit(), rng.unit()) * span;
            out.push(Star {
                at,
                radius,
                tint,
                alpha,
            });
        }
    }
    out
}

/// A sprite's side in design px: the disc and a texel clear all round.
fn sprite_side(radius: f32) -> f32 {
    2.0 * radius + 2.0 / TEXELS_PER_PX
}

/// A star's sprite, [`TEXELS_PER_PX`] texels a design px: its disc in white,
/// fading from 1 at its centre to 0 at `radius`; its node tints it and sets
/// its alpha. The texels past the disc are white at alpha 0, so filtering at
/// the disc's edge doesn't pull the star toward black.
pub(crate) fn sprite_image(radius: f32) -> Image {
    let side = (sprite_side(radius) * TEXELS_PER_PX).round() as u32;
    let centre = side as f32 / 2.0;
    let mut data = Vec::with_capacity((side * side * 4) as usize);
    for y in 0..side {
        for x in 0..side {
            let texels = (Vec2::new(x as f32 + 0.5, y as f32 + 0.5) - centre).length();
            let alpha = (1.0 - texels / TEXELS_PER_PX / radius).max(0.0);
            data.extend_from_slice(&[255, 255, 255, (alpha * 255.0).round() as u8]);
        }
    }
    let mut image = Image::new(
        Extent3d {
            width: side,
            height: side,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        data,
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::RENDER_WORLD,
    );
    image.sampler = ImageSampler::linear();
    image
}

/// §1's starfield as stacked gradients, bottom first (bevy_ui draws a node's
/// gradients in order, the last on top; CSS lists them top first): the
/// `--sf-1`, `--sf-2` at 60%, `--sf-3` fall, the bottom glow (an 80% × 50%
/// ellipse on the bottom edge) and the top glow (120% × 60% on the top edge),
/// each glow gone at half its radius. Interpolated in sRGB, as CSS does.
pub(crate) fn sky(theme: &TvTheme) -> BackgroundGradient {
    let [top, middle, bottom] = theme.starfield;
    let [glow_top, glow_bottom] = theme.sky_glow;
    let glow = |at: UiPosition, w: f32, h: f32, color: Color| {
        RadialGradient::new(
            at,
            RadialGradientShape::Ellipse(percent(w), percent(h)),
            vec![
                ColorStop::percent(color, 0),
                ColorStop::percent(color.with_alpha(0.0), 50),
            ],
        )
        .in_color_space(InterpolationColorSpace::Srgba)
    };
    BackgroundGradient(vec![
        LinearGradient::to_bottom(vec![
            ColorStop::percent(top, 0),
            ColorStop::percent(middle, 60),
            ColorStop::percent(bottom, 100),
        ])
        .in_color_space(InterpolationColorSpace::Srgba)
        .into(),
        glow(UiPosition::BOTTOM, 80.0, 50.0, glow_bottom).into(),
        glow(UiPosition::TOP, 120.0, 60.0, glow_top).into(),
    ])
}

/// The stars' layer, a screen root's first child so it draws under the rest:
/// an absolute node over the whole root from its top-left corner (the
/// overscan inset is padding, which an absolute node ignores), out of the
/// layout and out of picking, holding a node a star at its place with no
/// drift, which [`drift`] moves, unrounded so it draws at its own size. The
/// small stars go first and the big ones after, so the sky draws in two
/// batches, one a sprite.
pub(crate) fn spawn_stars(root: &mut ChildSpawnerCommands, sprites: &Sprites, theme: &TvTheme) {
    let mut sky = stars(SEED);
    sky.sort_by_key(Star::big);
    root.spawn((
        StarLayer,
        Node {
            position_type: PositionType::Absolute,
            left: px(0),
            top: px(0),
            width: percent(100),
            height: percent(100),
            ..default()
        },
        Pickable::IGNORE,
    ))
    .with_children(|layer| {
        for star in &sky {
            let side = sprite_side(star.radius);
            let placed = SkyStar {
                at: star.at,
                half: side / 2.0,
            };
            layer.spawn((
                placed,
                Node {
                    position_type: PositionType::Absolute,
                    left: px(0),
                    top: px(0),
                    width: px(side),
                    height: px(side),
                    ..default()
                },
                placed.placed(Vec2::ZERO),
                // A sprite a few px across rounded to whole pixels would draw
                // up to half a pixel larger or smaller at a scale like 720p's.
                LayoutConfig {
                    use_rounding: false,
                },
                ImageNode {
                    image: sprites.of(star),
                    color: theme.star_tints[star.tint as usize].with_alpha(star.alpha),
                    image_mode: NodeImageMode::Stretch,
                    ..default()
                },
                Pickable::IGNORE,
            ));
        }
    });
}

/// Where a point of the infinite sky at `at` (design px from the screen's
/// corner) is drawn: its one copy in the window, `[-MARGIN, PERIOD - MARGIN)`
/// each way, a loop's [`DRIFT`] at a time down it and then [`PERIOD`]`.x` at
/// a time across.
pub(crate) fn wrap(at: Vec2) -> Vec2 {
    let rows = ((at.y + MARGIN) / PERIOD.y).floor();
    let at = at - rows * DRIFT;
    let columns = ((at.x + MARGIN) / PERIOD.x).floor();
    Vec2::new(at.x - columns * PERIOD.x, at.y)
}

/// How far the sky has drifted up and to the left `elapsed` into the run:
/// [`DRIFT`] every [`LOOP`], from nothing at each loop's start, where every
/// star is back on a copy of itself.
pub(crate) fn drift_at(elapsed: Duration) -> Vec2 {
    let phase = (elapsed.as_secs_f64() % LOOP.as_secs_f64()) / LOOP.as_secs_f64();
    (DRIFT.as_dvec2() * phase).as_vec2()
}

/// Moves every star to the drift's place now, so a new screen's sky carries
/// on where the last one's was; [`ReducedMotion`] leaves them at their start.
pub(crate) fn drift(
    time: Res<Time>,
    reduced: Res<ReducedMotion>,
    mut stars: Query<(&SkyStar, &mut UiTransform)>,
) {
    if reduced.0 {
        return;
    }
    let drift = drift_at(time.elapsed());
    for (star, mut node) in &mut stars {
        node.set_if_neq(star.placed(drift));
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// The widest screen the sky covers without a star shown twice, in design
    /// px: 32:9 at the design height.
    const WIDEST: Vec2 = Vec2::new(3840.0, 1080.0);

    /// The sky's repeats near the origin: every `i` periods across plus `j`
    /// loops' drift, but none.
    pub(crate) fn repeats() -> impl Iterator<Item = Vec2> {
        (-3..=3_i8).flat_map(|i| {
            (-3..=3_i8)
                .filter(move |j| (i, *j) != (0, 0))
                .map(move |j| f32::from(i) * Vec2::new(PERIOD.x, 0.0) + f32::from(j) * DRIFT)
        })
    }

    /// Where `at` sits within the sky's period: the same for every copy of a
    /// star, so two stars on a screen at the same offset are one star twice.
    pub(crate) fn offset_in_period(at: Vec2) -> Vec2 {
        let rows = (at.y / PERIOD.y).floor();
        let at = at - rows * DRIFT;
        Vec2::new(at.x.rem_euclid(PERIOD.x), at.y)
    }

    #[test]
    fn the_same_seed_gives_the_same_stars() {
        let a = stars(SEED);
        assert_eq!(a.len(), (COLUMNS * ROWS) as usize);
        assert_eq!(a, stars(SEED));
        assert_ne!(a, stars(SEED + 1));
        for radius in [SMALL_RADIUS, BIG_RADIUS] {
            assert_eq!(sprite_image(radius).data, sprite_image(radius).data);
        }
    }

    #[test]
    fn the_period_is_its_cells_and_wider_than_32_9() {
        assert_eq!(PERIOD, CELL * Vec2::new(COLUMNS as f32, ROWS as f32));
        // The drift is one of the sky's repeats, on the cells' grid.
        assert_eq!(DRIFT.y, PERIOD.y);
        let cells = DRIFT / CELL;
        assert_eq!(cells, cells.round(), "{cells}");
        // Fewer columns wouldn't cover 32:9 with a star's sprite past the window's margin.
        let reach = sprite_side(BIG_RADIUS) / 2.0;
        assert!(PERIOD.x - MARGIN - reach >= WIDEST.x, "{PERIOD}");
        assert!((COLUMNS - 1) as f32 * CELL.x - MARGIN - reach < WIDEST.x);
        assert!(PERIOD.y - MARGIN - reach >= WIDEST.y, "{PERIOD}");
        assert!(MARGIN >= reach);
    }

    #[test]
    fn every_star_is_inside_the_period_one_to_a_cell() {
        for seed in [SEED, 0, 1, u64::MAX] {
            let sky = stars(seed);
            assert_eq!(sky.len(), (COLUMNS * ROWS) as usize);
            let mut cells: Vec<UVec2> = Vec::new();
            for star in &sky {
                let disc = Rect::from_center_half_size(star.at, Vec2::splat(star.radius));
                let period = Rect::from_corners(Vec2::splat(EDGE), PERIOD - EDGE);
                assert!(
                    period.contains(disc.min) && period.contains(disc.max),
                    "{star:?}"
                );
                let at = (star.at / CELL).floor().as_uvec2();
                let own = Rect::from_corners(at.as_vec2() * CELL, (at.as_vec2() + 1.0) * CELL);
                assert!(
                    own.contains(disc.min - EDGE) && own.contains(disc.max + EDGE),
                    "{star:?} crowds its cell's edge"
                );
                cells.push(at);
            }
            cells.sort_by_key(|c| (c.y, c.x));
            cells.dedup();
            assert_eq!(cells.len(), sky.len(), "a cell with two stars: {seed}");
            for star in &sky {
                assert_eq!(star.big(), star.tint != Tint::White, "{star:?}");
                assert!([1.0, 0.9, 0.8, 0.7].contains(&star.alpha), "{star:?}");
            }
            // The phone's mix, about 2 in 9 big: 45 of 204 expected.
            let big = sky.iter().filter(|s| s.big()).count();
            assert!((25..=70).contains(&big), "{seed}: {big} big");
        }
    }

    #[test]
    fn a_sprite_is_its_disc_in_white_and_clear_at_its_edges() {
        for radius in [SMALL_RADIUS, BIG_RADIUS] {
            let image = sprite_image(radius);
            let side = (sprite_side(radius) * TEXELS_PER_PX) as u32;
            assert_eq!(image.size(), UVec2::splat(side), "{radius}");
            let data = image.data.as_ref().unwrap();
            let alpha = |x: u32, y: u32| data[((y * side + x) * 4 + 3) as usize];
            assert!(data.chunks(4).all(|t| t[..3] == [255, 255, 255]));
            for i in 0..side {
                let edges = [
                    alpha(i, 0),
                    alpha(i, side - 1),
                    alpha(0, i),
                    alpha(side - 1, i),
                ];
                assert_eq!(edges, [0; 4], "{radius}: {i}");
            }
            // Brightest at the centre, and as much light as the cone holds
            // (a third of the disc's area), so the radius is the star's.
            let c = side / 2;
            let peak = [
                alpha(c - 1, c - 1),
                alpha(c, c - 1),
                alpha(c - 1, c),
                alpha(c, c),
            ];
            // The four centre texels, half a texel's diagonal from the centre.
            let near = 1.0 - std::f32::consts::FRAC_1_SQRT_2 / TEXELS_PER_PX / radius;
            let near = (near * 255.0).round() as u8;
            assert_eq!(peak, [near; 4], "{radius}");
            assert!(data.chunks(4).all(|t| t[3] <= peak[0]));
            let light: f32 = data.chunks(4).map(|t| f32::from(t[3]) / 255.0).sum();
            let cone = std::f32::consts::PI * (radius * TEXELS_PER_PX).powi(2) / 3.0;
            assert!(
                (light / cone - 1.0).abs() < 0.05,
                "{radius}: {light} for {cone}"
            );
        }
    }

    #[test]
    fn the_drift_loops_every_140_s_up_and_to_the_left() {
        let at = |s: f64| drift_at(Duration::from_secs_f64(s));
        assert_eq!(at(0.0), Vec2::ZERO);
        assert_eq!(at(140.0), Vec2::ZERO);
        assert_eq!(at(70.0), Vec2::new(480.0, 810.0));
        assert_eq!(at(35.0), Vec2::new(240.0, 405.0));
        assert_eq!(at(35.0), at(35.0 + 140.0 * 7.0));
        for tenth in 0..1400 {
            let d = at(f64::from(tenth) / 10.0);
            assert!(d.cmpge(Vec2::ZERO).all() && d.cmplt(DRIFT).all(), "{d}");
        }
        // About 7 design px a second across and 12 up, AP.11's.
        let speed = at(1.0);
        assert!((speed.x - 960.0 / 140.0).abs() < 1e-3, "{speed}");
        assert!((speed.y - 1620.0 / 140.0).abs() < 1e-3, "{speed}");
        // A loop's drift lands every star on a copy of itself.
        for star in stars(SEED) {
            let placed = SkyStar {
                at: star.at,
                half: 0.0,
            };
            let (start, end) = (placed.centre(Vec2::ZERO), placed.centre(DRIFT));
            assert!(start.abs_diff_eq(end, 1e-3), "{star:?}: {start} {end}");
        }
    }

    /// The fix for AP.11's tile, which repeated 2×2 on a 16:9 screen: a star
    /// and any copy of it are never both on a screen up to 32:9, not even a
    /// sliver of each at opposite edges.
    #[test]
    fn no_screen_up_to_32_9_shows_a_star_twice() {
        let reach = 2.0 * (sprite_side(BIG_RADIUS) / 2.0);
        for repeat in repeats() {
            let apart = repeat.abs();
            assert!(
                apart.x >= WIDEST.x + reach || apart.y >= WIDEST.y + reach,
                "{repeat}"
            );
        }
        // The window holds one copy of every point of the sky.
        let window = Rect::from_corners(Vec2::splat(-MARGIN), PERIOD - MARGIN);
        for at in [
            Vec2::ZERO,
            Vec2::new(-9.0, 5000.0),
            Vec2::new(12345.6, -2000.0),
        ] {
            let drawn = wrap(at);
            assert!(window.contains(drawn), "{at} at {drawn}");
            assert!(offset_in_period(drawn).abs_diff_eq(offset_in_period(at), 1e-2));
        }
    }

    /// Every star moves with the drift a frame at a time and jumps only from
    /// one off-screen spot to another, on any screen up to 32:9; and wherever
    /// the drift is, every 2×2 cells' patch of that screen holds a star.
    #[test]
    fn a_star_wraps_only_off_the_screen_and_the_sky_has_no_holes() {
        let reach = sprite_side(BIG_RADIUS) / 2.0;
        let screen = Rect::from_corners(Vec2::ZERO, WIDEST);
        let shows = |c: Vec2| {
            !Rect::from_center_half_size(c, Vec2::splat(reach))
                .intersect(screen)
                .is_empty()
        };
        let sky = stars(SEED);
        let frame = Duration::from_secs(1) / 60;
        // A frame's drift, across a loop's end too: the drift starts again
        // there, a whole loop back, which every star's copy hides.
        let moved = -DRIFT * (frame.as_secs_f32() / LOOP.as_secs_f32());
        let patch = 2.0 * CELL;
        let patches = (WIDEST / patch).as_uvec2();
        let mut jumps = 0;
        // Through a loop and a second past its end, so the seam is crossed.
        let through = (LOOP + Duration::from_secs(1)).as_nanos() / frame.as_nanos();
        let mut crossed = 0;
        for n in 0..through as u32 {
            let (now, next) = (drift_at(frame * n), drift_at(frame * (n + 1)));
            crossed += usize::from(next.x < now.x);
            let mut held = std::collections::BTreeSet::new();
            for star in &sky {
                let placed = SkyStar {
                    at: star.at,
                    half: reach,
                };
                let (a, b) = (placed.centre(now), placed.centre(next));
                if !(b - a).abs_diff_eq(moved, 1e-2) {
                    jumps += 1;
                    assert!(!shows(a) && !shows(b), "{star:?} jumps {a} to {b}");
                }
                if a.cmpge(Vec2::ZERO).all() && a.cmplt(WIDEST).all() {
                    held.insert((a / patch).as_uvec2().to_array());
                }
            }
            assert_eq!(held.len() as u32, patches.x * patches.y, "frame {n}");
        }
        assert!(jumps > 0, "the loop wraps stars");
        assert_eq!(crossed, 1, "the loop's end, once");
    }
}
