//! The TV's sky (openspec AP, design decision 8): `design_language.md` §1's
//! starfield on every screen root, as three stacked gradients (the `--sf-1` to
//! `--sf-3` fall and the glows at the top and bottom edges) under a layer of
//! stars generated here, never a disc or wiki image. The stars drift one loop
//! every 140 s (§5's `--dur-sky-drift`) unless [`ReducedMotion`] holds them.

use std::time::Duration;

use bevy::asset::RenderAssetUsages;
use bevy::image::ImageSampler;
use bevy::picking::Pickable;
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};
use bevy::ui::{InterpolationColorSpace, RadialGradientShape, UiPosition, Val2};

use crate::motion::ReducedMotion;
use crate::theme::TvTheme;

/// One tile of stars, in design px: half the 1080-tall design screen each way.
pub(crate) const TILE: Vec2 = Vec2::new(960.0, 540.0);

/// The tile's texture resolution: 1:1 on a 4K target, minified 2:1 at 1080p.
const TEXELS_PER_PX: f32 = 2.0;

/// Tiles in the layer: one tile past the screen each way, so the drift never
/// shows an edge, for any screen up to 32:9 (the design height is always
/// 1080, so the width is what varies).
const COLUMNS: usize = 5;
const ROWS: usize = 3;

/// The tile is split into this grid and each cell holds one star, so the sky
/// has neither clumps nor holes.
const CELLS: UVec2 = UVec2::new(4, 4);

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

/// How far a loop carries the stars up and to the left, in tiles: one
/// across and three up (960 design px for 1620), about the phone's angle (400
/// px across for 800 up) and, at couch distance, about its angular speed.
const DRIFT_TILES: Vec2 = Vec2::new(1.0, 3.0);

/// A star's colour: the phone's white, warm (`#ffe6b4`) and cool (`#b4dcff`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Tint {
    White,
    Warm,
    Cool,
}

/// One star of a tile: a soft disc fading from `alpha` at its centre to
/// nothing at `radius`, as the phone's `radial-gradient(.., color, transparent)`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Star {
    /// The centre, in design px from the tile's top-left.
    pub(crate) at: Vec2,
    pub(crate) radius: f32,
    pub(crate) tint: Tint,
    pub(crate) alpha: f32,
}

/// The stars' layer on a screen root; the drift moves it.
#[derive(Component)]
pub(crate) struct StarLayer;

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

/// A tile's stars from `seed`: one in each cell of the grid at a random spot
/// [`EDGE`] clear of the cell's sides, so every star is inside its tile and
/// the tile repeats with no seam; the phone's mix of small white stars at
/// full, 0.8 and 0.7 alpha and big warm or cool ones at 0.9.
pub(crate) fn stars(seed: u64) -> Vec<Star> {
    let mut rng = SplitMix64(seed);
    let cell = TILE / CELLS.as_vec2();
    let mut out = Vec::with_capacity((CELLS.x * CELLS.y) as usize);
    for row in 0..CELLS.y {
        for col in 0..CELLS.x {
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
            let corner = Vec2::new(col as f32, row as f32) * cell + margin;
            let span = cell - 2.0 * margin;
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

/// The tile's texture, [`TEXELS_PER_PX`] texels a design px, transparent but
/// for the stars. A texel just past each disc carries its tint at alpha 0, so
/// filtering at the disc's edge doesn't pull the star toward black.
pub(crate) fn tile_image(stars: &[Star], tints: [Color; 3]) -> Image {
    let size = (TILE * TEXELS_PER_PX).as_uvec2();
    let mut data = vec![0u8; (size.x * size.y * 4) as usize];
    for star in stars {
        let rgb = Srgba::from(tints[star.tint as usize]).to_u8_array_no_alpha();
        let centre = star.at * TEXELS_PER_PX;
        let radius = star.radius * TEXELS_PER_PX;
        let lo = (centre - radius - 1.0).floor().max(Vec2::ZERO).as_uvec2();
        let hi = (centre + radius + 1.0)
            .ceil()
            .min(size.as_vec2())
            .as_uvec2();
        for y in lo.y..hi.y {
            for x in lo.x..hi.x {
                let d = (Vec2::new(x as f32 + 0.5, y as f32 + 0.5) - centre).length();
                let alpha = (star.alpha * (1.0 - d / radius)).max(0.0);
                let alpha = (alpha * 255.0).round() as u8;
                let i = ((y * size.x + x) * 4) as usize;
                if alpha >= data[i + 3] {
                    data[i..i + 3].copy_from_slice(&rgb);
                    data[i + 3] = alpha;
                }
            }
        }
    }
    let mut image = Image::new(
        Extent3d {
            width: size.x,
            height: size.y,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        data,
        TextureFormat::Rgba8UnormSrgb,
        // The GPU keeps it; the 8 MB on the CPU go once it's uploaded.
        RenderAssetUsages::RENDER_WORLD,
    );
    image.sampler = ImageSampler::linear();
    image
}

/// Built once a run (`TvAssets`): every screen's layer draws the same tile.
pub(crate) fn tile(images: &mut Assets<Image>, theme: &TvTheme) -> Handle<Image> {
    images.add(tile_image(&stars(SEED), theme.star_tints))
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
/// [`COLUMNS`] × [`ROWS`] tiles from the root's top-left corner (the overscan
/// inset is padding, which an absolute node ignores), out of the layout and
/// out of picking.
pub(crate) fn spawn_stars(root: &mut ChildSpawnerCommands, tile: &Handle<Image>) {
    root.spawn((
        StarLayer,
        Node {
            position_type: PositionType::Absolute,
            left: px(0),
            top: px(0),
            width: px(TILE.x * COLUMNS as f32),
            height: px(TILE.y * ROWS as f32),
            flex_direction: FlexDirection::Row,
            flex_wrap: FlexWrap::Wrap,
            align_content: AlignContent::FlexStart,
            ..default()
        },
        UiTransform::IDENTITY,
        Pickable::IGNORE,
    ))
    .with_children(|layer| {
        for _ in 0..COLUMNS * ROWS {
            layer.spawn((
                Node {
                    width: px(TILE.x),
                    height: px(TILE.y),
                    flex_shrink: 0.0,
                    ..default()
                },
                ImageNode {
                    image: tile.clone(),
                    image_mode: NodeImageMode::Stretch,
                    ..default()
                },
                Pickable::IGNORE,
            ));
        }
    });
}

/// How far the stars have drifted up and to the left `elapsed` into the run,
/// wrapped to a tile: [`DRIFT_TILES`] every [`LOOP`], so the layer is back
/// where it started each loop and the tiles hide the wrap.
pub(crate) fn drift_at(elapsed: Duration) -> Vec2 {
    let phase = (elapsed.as_secs_f64() % LOOP.as_secs_f64()) / LOOP.as_secs_f64();
    let moved = (TILE * DRIFT_TILES).as_dvec2() * phase;
    let tile = TILE.as_dvec2();
    Vec2::new((moved.x % tile.x) as f32, (moved.y % tile.y) as f32)
}

/// Moves every star layer to the drift's place now, so a new screen's sky
/// carries on where the last one's was; [`ReducedMotion`] leaves them at
/// their start.
pub(crate) fn drift(
    time: Res<Time>,
    reduced: Res<ReducedMotion>,
    mut layers: Query<&mut UiTransform, With<StarLayer>>,
) {
    if reduced.0 {
        return;
    }
    let at = drift_at(time.elapsed());
    let moved = UiTransform::from_translation(Val2::px(-at.x, -at.y));
    for mut layer in &mut layers {
        layer.set_if_neq(moved);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_same_seed_gives_the_same_stars() {
        let a = stars(SEED);
        assert_eq!(a, stars(SEED));
        assert_ne!(a, stars(SEED + 1));
        let tints = TvTheme::default().star_tints;
        assert_eq!(
            tile_image(&a, tints).data,
            tile_image(&stars(SEED), tints).data
        );
    }

    #[test]
    fn every_star_is_inside_its_tile_one_to_a_cell() {
        let cell = TILE / CELLS.as_vec2();
        for seed in [SEED, 0, 1, u64::MAX] {
            let sky = stars(seed);
            assert_eq!(sky.len(), (CELLS.x * CELLS.y) as usize);
            let mut cells: Vec<UVec2> = Vec::new();
            for star in &sky {
                let disc = Rect::from_center_half_size(star.at, Vec2::splat(star.radius));
                let tile = Rect::from_corners(Vec2::splat(EDGE), TILE - EDGE);
                assert!(
                    tile.contains(disc.min) && tile.contains(disc.max),
                    "{star:?}"
                );
                let at = (star.at / cell).floor().as_uvec2();
                let own = Rect::from_corners(at.as_vec2() * cell, (at.as_vec2() + 1.0) * cell);
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
                let big = star.radius == BIG_RADIUS;
                assert_eq!(big, star.tint != Tint::White, "{star:?}");
                assert!([1.0, 0.9, 0.8, 0.7].contains(&star.alpha), "{star:?}");
            }
        }
    }

    #[test]
    fn the_tile_texture_is_clear_at_its_edges_so_it_repeats_without_a_seam() {
        let tints = TvTheme::default().star_tints;
        let image = tile_image(&stars(SEED), tints);
        let size = (TILE * TEXELS_PER_PX).as_uvec2();
        assert_eq!(image.size(), size);
        let data = image.data.as_ref().unwrap();
        let alpha = |x: u32, y: u32| data[((y * size.x + x) * 4 + 3) as usize];
        for x in 0..size.x {
            assert_eq!((alpha(x, 0), alpha(x, size.y - 1)), (0, 0), "column {x}");
        }
        for y in 0..size.y {
            assert_eq!((alpha(0, y), alpha(size.x - 1, y)), (0, 0), "row {y}");
        }
        // Each star is drawn: its centre texel near its alpha, in its tint.
        for star in stars(SEED) {
            let c = (star.at * TEXELS_PER_PX).as_uvec2();
            let i = ((c.y * size.x + c.x) * 4) as usize;
            let rgb = Srgba::from(tints[star.tint as usize]).to_u8_array_no_alpha();
            assert_eq!(data[i..i + 3], rgb, "{star:?}");
            assert!(
                f32::from(data[i + 3]) / 255.0 > star.alpha * 0.6,
                "{star:?}"
            );
        }
    }

    #[test]
    fn the_drift_loops_every_140_s_up_and_to_the_left() {
        let at = |s: f64| drift_at(Duration::from_secs_f64(s));
        assert_eq!(at(0.0), Vec2::ZERO);
        assert_eq!(at(140.0), Vec2::ZERO);
        assert_eq!(at(70.0), Vec2::new(480.0, 270.0));
        assert_eq!(at(35.0), Vec2::new(240.0, 405.0));
        assert_eq!(at(35.0), at(35.0 + 140.0 * 7.0));
        for tenth in 0..1400 {
            let d = at(f64::from(tenth) / 10.0);
            assert!(d.cmpge(Vec2::ZERO).all() && d.cmplt(TILE).all(), "{d}");
        }
        // About 7 design px a second across and 12 up.
        let speed = at(1.0);
        assert!((speed.x - 960.0 / 140.0).abs() < 1e-3, "{speed}");
        assert!((speed.y - 1620.0 / 140.0).abs() < 1e-3, "{speed}");
    }
}
