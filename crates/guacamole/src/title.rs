//! The TV's titles through a shader (openspec AP, design decision 7): the
//! seven screen titles and the portal's game name, in caps (§2: Titan One is
//! for caps), drawn with `design_language.md` §2's display treatment.
//!
//! A title is a wrapper holding its own `Text`, hidden, as the layout box, so
//! a screen lays out as it would with a plain line, headless included; the
//! [`TitleMaterial`] surface laid over it, grown by the treatment's margin;
//! and behind a screen title §2's ray halo. The string renders once, white,
//! into an image the surface's size, by a camera of its own (order -1, cleared
//! to `Color::NONE`, a UI root on it under `UiTargetCamera`); the fragment
//! ([`title.wesl`](title.wesl), embedded in the binary) builds the stack from
//! that image's alpha. The camera draws the frame its glyphs land and goes
//! inactive the next, keeping its image (AP.8), so a title renders again only
//! when its string, its size or the TV's scale factor changes: a window moved
//! between a Retina display and another renders its titles again at the new
//! scale, since the title camera's scale factor follows the TV camera's
//! `target_scaling_factor()` (AP.8: at the image's own 1.0 a Retina window
//! drew every title at half size).
//!
//! Every title camera is a second `Camera2d`, so anything after the TV's own
//! finds it by [`TvCamera`], never by `Camera2d`. A title's camera and ink go
//! with its surface, so a screen leaving takes its titles' with it.

use bevy::asset::embedded_asset;
use bevy::camera::{ClearColorConfig, ImageRenderTarget, RenderTarget};
use bevy::picking::Pickable;
use bevy::prelude::*;
use bevy::reflect::TypePath;
use bevy::render::render_resource::{AsBindGroup, ShaderType, TextureFormat};
use bevy::render::view::Msaa;
use bevy::shader::ShaderRef;
use bevy::text::{FontSize, LetterSpacing, TextLayoutInfo};
use bevy::ui::{
    ComputedNode, InterpolationColorSpace, RadialGradientShape, UiPosition, UiSystems,
    UiTargetCamera,
};

use crate::surfaces;
use crate::theme::{Role, Step, TvTheme};
use crate::tv::TvCamera;

/// Where [`TitleShader`] embeds `title.wesl` (`embedded_asset!`'s path for
/// this file).
const SHADER: &str = "embedded://guacamole/title.wesl";

/// §2's display letter spacing, at its tight end for title sizes.
const LETTER_SPACING_EM: f32 = 0.04;

/// §2's drop, `0 5px 10px rgba(0,0,0,0.5)`: CSS's 10 px blur is a Gaussian of
/// sigma 5.
const DROP_OFFSET: f32 = 5.0;
const DROP_SIGMA: f32 = 5.0;
const DROP_ALPHA: f32 = 0.5;

/// §2's glow, `0 0 20-40px rgba(245,198,52,0.3)`: `--g` at 0.3.
const GLOW_ALPHA: f32 = 0.3;

/// The halo past a screen title's box (the TV launcher mock's `.title-rays`,
/// `inset: -60px -120px`), and its gold at the centre (`rgba(245,198,52,
/// 0.12)`). The mock's ellipse runs past its own box and is cut off by it, a
/// faint edge each side; this one fades to nothing at the box's edge.
const HALO_PAD: Vec2 = Vec2::new(120.0, 60.0);
const HALO_ALPHA: f32 = 0.12;

/// Stops the halo's fade is matched to CSS at; bevy_ui interpolates between.
const HALO_STOPS: u16 = 8;

/// Slack on the ink's width, in design px, so float error never wraps a word
/// the layout `Text` kept on its line.
const INK_SLACK: f32 = 0.5;

/// A title's wrapper: its hidden layout `Text`, the surface and, behind a
/// screen title, the halo.
#[derive(Component, Debug, Clone, Copy)]
pub(crate) struct Title;

/// A title's own `Text`, hidden: the layout box and the string drawn.
#[derive(Component, Debug)]
pub(crate) struct TitleText;

/// §2's ray halo ([`halo_bundle`]); only screen titles have one.
#[derive(Component, Debug)]
pub(crate) struct TitleHalo;

/// A title camera: one per title on screen, never the TV's.
#[derive(Component, Debug)]
pub(crate) struct TitleCamera;

/// A title camera's UI root, outside every screen.
#[derive(Component, Debug)]
pub(crate) struct TitleInk;

/// The surface the treatment draws on, over the layout box grown by the
/// margin, and what it last rendered.
#[derive(Component, Debug)]
pub(crate) struct TitleSurface {
    layout: Entity,
    step: Step,
    drawn: Option<Drawn>,
}

impl TitleSurface {
    /// The title camera drawing (or keeping) this surface's glyphs, its ink
    /// root, the ink `Text` and where the draw is.
    #[cfg(test)]
    pub(crate) fn drawn(&self) -> Option<(Entity, Entity, Entity, Phase)> {
        self.drawn
            .as_ref()
            .map(|d| (d.camera, d.root, d.ink, d.phase))
    }
}

#[derive(Debug)]
struct Drawn {
    key: Key,
    camera: Entity,
    root: Entity,
    ink: Entity,
    phase: Phase,
}

/// What a render depends on: another of any of these renders the title again.
#[derive(Debug, Clone, PartialEq)]
struct Key {
    text: String,
    /// The surface in physical px, rounded up: the image's size.
    size: UVec2,
    /// The TV camera's target scale factor (2 on a Retina window).
    scale_factor: f32,
    ui_scale: f32,
}

/// Where a title camera is in its one draw.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Phase {
    /// Spawned, its ink not laid out yet.
    Waiting,
    /// The ink landed: the camera draws the glyphs this frame.
    Inked,
    /// Inactive, its image kept.
    Kept,
}

/// What [`TitleMaterial`] hands `title.wesl`, field for field (a test holds
/// the two in step): §2's layers in linear colour, and their sizes in design
/// px.
#[derive(ShaderType, Reflect, Clone, Debug, PartialEq)]
pub(crate) struct TitleParams {
    /// `--gb` at the top of the layout box, `--g` at its bottom.
    pub(crate) fill_top: LinearRgba,
    pub(crate) fill_bottom: LinearRgba,
    /// `--gi`.
    pub(crate) stroke: LinearRgba,
    /// `--gs` at +2 px, `--gi` at +3 px.
    pub(crate) shadow_near: LinearRgba,
    pub(crate) shadow_far: LinearRgba,
    /// Black at 0.5, blurred.
    pub(crate) drop: LinearRgba,
    /// `--g` at 0.3, blurred wide.
    pub(crate) glow: LinearRgba,
    /// The sky behind the title, which the shader matches CSS's blend on.
    pub(crate) backdrop: LinearRgba,
    /// Physical px per design px: the TV camera's scale factor × `UiScale`.
    pub(crate) scale: f32,
    pub(crate) margin: f32,
    pub(crate) stroke_width: f32,
    pub(crate) drop_offset: f32,
    pub(crate) drop_sigma: f32,
    pub(crate) glow_sigma: f32,
}

/// §2's display treatment, drawn by `title.wesl` from a title's glyphs.
#[derive(Asset, TypePath, AsBindGroup, Clone, Debug)]
pub(crate) struct TitleMaterial {
    #[uniform(0)]
    pub(crate) params: TitleParams,
    /// The title camera's image.
    #[texture(1)]
    #[sampler(2)]
    pub(crate) glyphs: Handle<Image>,
}

impl UiMaterial for TitleMaterial {
    fn fragment_shader() -> ShaderRef {
        SHADER.into()
    }
}

/// A title size's stroke (§2's 1-2 px, by size), glow (§2's 20-40 px blur,
/// as a Gaussian's sigma: half the blur) and the margin the surface grows
/// past the layout box, which holds every effect: the glow's 2.5 sigma is the
/// farthest any of them reaches.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Treatment {
    pub(crate) stroke: f32,
    pub(crate) glow_sigma: f32,
    pub(crate) margin: f32,
}

/// The treatment for a title at `step`: a hero title's 96, or 64 (the call to
/// scan, NoGiants' title, the game's name). Nothing smaller is a title.
pub(crate) fn treatment(step: Step) -> Treatment {
    match step {
        Step::Hero => Treatment {
            stroke: 2.0,
            glow_sigma: 15.0,
            margin: 40.0,
        },
        Step::Lg | Step::Md | Step::Body | Step::Caption => Treatment {
            stroke: 1.5,
            glow_sigma: 10.0,
            margin: 28.0,
        },
    }
}

/// The sky behind a title, which the shader and the halo match CSS's blend
/// on: `--sf-2`, the starfield's middle stop. Over the sky's other stops the
/// difference is that backdrop's times the transmittance, a level or two in
/// the glow at most.
fn backdrop(theme: &TvTheme) -> Color {
    theme.starfield[1]
}

/// The uniform for a title at `step` drawn at `scale` physical px a design px.
pub(crate) fn params(theme: &TvTheme, step: Step, scale: f32) -> TitleParams {
    let t = treatment(step);
    TitleParams {
        fill_top: theme.gold_bright.to_linear(),
        fill_bottom: theme.gold.to_linear(),
        stroke: theme.gold_inner.to_linear(),
        shadow_near: theme.gold_shadow.to_linear(),
        shadow_far: theme.gold_inner.to_linear(),
        drop: theme.shadow.with_alpha(DROP_ALPHA).to_linear(),
        glow: theme.gold.with_alpha(GLOW_ALPHA).to_linear(),
        backdrop: backdrop(theme).to_linear(),
        scale,
        margin: t.margin,
        stroke_width: t.stroke,
        drop_offset: DROP_OFFSET,
        drop_sigma: DROP_SIGMA,
        glow_sigma: t.glow_sigma,
    }
}

/// The running TV's half: `title.wesl` embedded in the binary (no assets
/// folder in a release build) and the material's render pipeline. The
/// headless tests have no render app and leave it out.
pub(crate) struct TitleShader;

impl Plugin for TitleShader {
    fn build(&self, app: &mut App) {
        embedded_asset!(app, "title.wesl");
        app.add_plugins(UiMaterialPlugin::<TitleMaterial>::default());
    }
}

/// The titles' upkeep, in `Screens` so the headless tests run it: each
/// surface rendered when it needs to be, its camera retired after the draw,
/// and its camera and ink despawned with it.
pub(crate) struct Titles;

impl Plugin for Titles {
    fn build(&self, app: &mut App) {
        // `TitleShader`'s `UiMaterialPlugin` registers the asset where
        // there's a render app; headless needs only the type.
        if !app.world().contains_resource::<Assets<TitleMaterial>>() {
            app.init_asset::<TitleMaterial>();
        }
        app.add_observer(drop_title)
            .add_systems(PostUpdate, draw_titles.after(UiSystems::PostLayout));
    }
}

/// A title in caps under `parent`: the wrapper, the hidden layout `Text`
/// (carrying `marker`, at `step` in `font`), the surface over it and, with
/// `halo`, the halo behind. Returns the wrapper.
#[allow(clippy::too_many_arguments)]
pub(crate) fn spawn(
    parent: &mut ChildSpawnerCommands,
    theme: &TvTheme,
    font: Handle<Font>,
    text: &str,
    step: Step,
    justify: Justify,
    halo: bool,
    marker: impl Bundle,
) -> Entity {
    let margin = treatment(step).margin;
    let size = theme.size(step);
    let mut wrapper = parent.spawn((
        Title,
        Node {
            max_width: percent(100),
            flex_shrink: 0.0,
            ..default()
        },
    ));
    let id = wrapper.id();
    wrapper.with_children(|title| {
        if halo {
            title.spawn(halo_bundle(theme));
        }
        let layout = title
            .spawn((
                TitleText,
                Text::new(text.to_uppercase()),
                TextFont {
                    font: font.into(),
                    font_size: FontSize::Px(size),
                    ..default()
                },
                LetterSpacing::Px(size * LETTER_SPACING_EM),
                TextColor(theme.role(Role::Accent)),
                TextLayout::justify(justify),
                Node {
                    max_width: percent(100),
                    ..default()
                },
                // Keeps its layout space; the surface draws it.
                Visibility::Hidden,
                marker,
            ))
            .id();
        title.spawn((
            TitleSurface {
                layout,
                step,
                drawn: None,
            },
            Node {
                position_type: PositionType::Absolute,
                left: px(-margin),
                right: px(-margin),
                top: px(-margin),
                bottom: px(-margin),
                ..default()
            },
            // Until its image exists.
            Visibility::Hidden,
            Pickable::IGNORE,
        ));
    });
    id
}

/// §2's `.title-rays`: a radial gold ellipse behind a screen title, the TV
/// launcher mock's size and gold, fading to nothing at its edge. Its stops
/// go through [`surfaces::css_over`], so the halo lands over the sky where a
/// browser's sRGB blend puts the mock's, not brighter as bevy_ui's linear
/// blend would draw the same alpha.
fn halo_bundle(theme: &TvTheme) -> impl Bundle + use<> {
    (
        TitleHalo,
        Node {
            position_type: PositionType::Absolute,
            left: px(-HALO_PAD.x),
            right: px(-HALO_PAD.x),
            top: px(-HALO_PAD.y),
            bottom: px(-HALO_PAD.y),
            ..default()
        },
        BackgroundGradient::from(
            RadialGradient::new(
                UiPosition::CENTER,
                RadialGradientShape::Ellipse(percent(50), percent(50)),
                halo_stops(theme),
            )
            .in_color_space(InterpolationColorSpace::LinearRgba),
        ),
        Pickable::IGNORE,
    )
}

/// The halo's fade, `--g` at [`HALO_ALPHA`] at the centre to nothing at the
/// edge, linear in CSS's alpha as the mock's two-stop gradient is.
pub(crate) fn halo_stops(theme: &TvTheme) -> Vec<ColorStop> {
    (0..=HALO_STOPS)
        .map(|i| {
            let at = f32::from(i) / f32::from(HALO_STOPS);
            let alpha = HALO_ALPHA * (1.0 - at);
            ColorStop::percent(
                surfaces::css_over(backdrop(theme), theme.gold, alpha),
                100.0 * at,
            )
        })
        .collect()
}

/// Renders each title surface whose string, size or scale changed since its
/// last render, and retires each title camera the frame after its glyphs
/// landed. After the layout, so the surface's size is this frame's.
#[allow(clippy::too_many_arguments)]
fn draw_titles(
    mut commands: Commands,
    theme: Res<TvTheme>,
    ui_scale: Res<UiScale>,
    tv: Query<&Camera, (With<TvCamera>, Without<TitleCamera>)>,
    mut surfaces: Query<(
        Entity,
        &mut TitleSurface,
        &ComputedNode,
        &mut Visibility,
        Option<&MaterialNode<TitleMaterial>>,
    )>,
    layouts: Query<
        (
            &Text,
            &TextFont,
            &TextLayout,
            Option<&LetterSpacing>,
            &ComputedNode,
        ),
        With<TitleText>,
    >,
    inks: Query<(&ComputedNode, &TextLayoutInfo), Without<TitleSurface>>,
    mut cameras: Query<&mut Camera, (With<TitleCamera>, Without<TvCamera>)>,
    mut images: ResMut<Assets<Image>>,
    mut materials: ResMut<Assets<TitleMaterial>>,
) {
    let Some(scale_factor) = tv.single().ok().and_then(Camera::target_scaling_factor) else {
        return;
    };
    for (entity, mut surface, node, mut visibility, material_node) in &mut surfaces {
        let Ok((text, font, layout, spacing, laid)) = layouts.get(surface.layout) else {
            continue;
        };
        let size = node.size();
        if text.0.is_empty() || size.x < 1.0 || size.y < 1.0 {
            if let Some(old) = surface.drawn.take() {
                old.despawn(&mut commands);
            }
            visibility.set_if_neq(Visibility::Hidden);
            continue;
        }
        let key = Key {
            text: text.0.clone(),
            size: size.ceil().as_uvec2(),
            scale_factor,
            ui_scale: ui_scale.0,
        };
        if let Some(drawn) = surface.drawn.as_mut().filter(|d| d.key == key) {
            match drawn.phase {
                Phase::Waiting => {
                    if inks
                        .get(drawn.ink)
                        .is_ok_and(|(n, info)| n.size().x > 0.0 && !info.glyphs.is_empty())
                    {
                        drawn.phase = Phase::Inked;
                    }
                }
                Phase::Inked => {
                    if let Ok(mut camera) = cameras.get_mut(drawn.camera) {
                        camera.is_active = false;
                    }
                    drawn.phase = Phase::Kept;
                }
                Phase::Kept => {}
            }
            continue;
        }
        if let Some(old) = surface.drawn.take() {
            old.despawn(&mut commands);
        }
        let image = images.add(Image::new_target_texture(
            key.size.x,
            key.size.y,
            TextureFormat::Rgba8UnormSrgb,
            None,
        ));
        let camera = commands
            .spawn((
                TitleCamera,
                Camera2d,
                Camera {
                    order: -1,
                    // The default clear is the opaque `ClearColor`, which
                    // would make every texel a glyph.
                    clear_color: ClearColorConfig::Custom(Color::NONE),
                    ..default()
                },
                RenderTarget::Image(ImageRenderTarget {
                    handle: image.clone(),
                    scale_factor,
                }),
                Msaa::Off,
            ))
            .id();
        let margin = treatment(surface.step).margin;
        let root = commands
            .spawn((
                TitleInk,
                Node {
                    width: percent(100),
                    height: percent(100),
                    padding: UiRect::all(px(margin)),
                    align_items: AlignItems::FlexStart,
                    justify_content: JustifyContent::FlexStart,
                    ..default()
                },
                UiTargetCamera(camera),
                Pickable::IGNORE,
            ))
            .id();
        // As wide as the layout box, so a title that wraps wraps the same.
        let width = laid.unrounded_size.x * laid.inverse_scale_factor + INK_SLACK;
        let ink = commands
            .spawn((
                Text::new(text.0.clone()),
                font.clone(),
                spacing.copied().unwrap_or(LetterSpacing::Px(0.0)),
                TextColor(Color::WHITE),
                *layout,
                Node {
                    width: px(width),
                    flex_shrink: 0.0,
                    ..default()
                },
                ChildOf(root),
            ))
            .id();
        let params = params(&theme, surface.step, scale_factor * ui_scale.0);
        let material = TitleMaterial {
            params,
            glyphs: image,
        };
        match material_node
            .map(|m| m.0.clone())
            .filter(|h| materials.contains(h))
        {
            Some(handle) => {
                if let Some(mut drawn) = materials.get_mut(&handle) {
                    *drawn = material;
                }
            }
            None => {
                let handle = materials.add(material);
                commands.entity(entity).insert(MaterialNode(handle));
            }
        }
        visibility.set_if_neq(Visibility::Inherited);
        debug!(
            title = key.text,
            size = %key.size,
            scale_factor,
            "title rendering"
        );
        surface.drawn = Some(Drawn {
            key,
            camera,
            root,
            ink,
            phase: Phase::Waiting,
        });
    }
}

impl Drawn {
    fn despawn(self, commands: &mut Commands) {
        commands.entity(self.camera).try_despawn();
        commands.entity(self.root).try_despawn();
    }
}

/// A surface leaving (its screen did) takes its camera and ink with it.
fn drop_title(
    remove: On<Remove<TitleSurface>>,
    mut surfaces: Query<&mut TitleSurface>,
    mut commands: Commands,
) {
    if let Some(drawn) = surfaces
        .get_mut(remove.entity)
        .ok()
        .and_then(|mut s| s.drawn.take())
    {
        drawn.despawn(&mut commands);
    }
}

#[cfg(test)]
mod tests {
    use bevy::reflect::structs::Struct;

    use super::*;

    #[test]
    fn the_uniform_is_the_themes_gold_stack() {
        let theme = TvTheme::default();
        let p = params(&theme, Step::Hero, 2.0);
        assert_eq!(p.fill_top, theme.gold_bright.to_linear());
        assert_eq!(p.fill_bottom, theme.gold.to_linear());
        assert_eq!(p.stroke, theme.gold_inner.to_linear());
        assert_eq!(p.shadow_near, theme.gold_shadow.to_linear());
        assert_eq!(p.shadow_far, theme.gold_inner.to_linear());
        assert_eq!(p.drop, Color::srgba(0.0, 0.0, 0.0, 0.5).to_linear());
        assert_eq!(p.glow, theme.gold.with_alpha(0.3).to_linear());
        assert_eq!(p.backdrop, theme.starfield[1].to_linear());
        assert_eq!(
            Srgba::from(p.fill_top).to_hex(),
            "#FFE58A",
            "§1's --gb, not a literal"
        );
        assert_eq!(
            (p.scale, p.drop_offset, p.drop_sigma),
            (2.0, 5.0, 5.0),
            "§2's 0 5px 10px drop"
        );
        // From the theme, so a skin is another theme.
        let mut skin = TvTheme::default();
        skin.gold = Color::srgb_u8(0xda, 0x5a, 0xd6);
        let q = params(&skin, Step::Hero, 2.0);
        assert_eq!(q.glow, skin.gold.with_alpha(0.3).to_linear());
        assert_eq!(q.fill_bottom, skin.gold.to_linear());
        assert_ne!(q, p);
    }

    #[test]
    fn each_title_size_gets_its_stroke_and_glow() {
        let hero = treatment(Step::Hero);
        let lg = treatment(Step::Lg);
        for (step, t) in [(Step::Hero, hero), (Step::Lg, lg)] {
            assert!((1.0..=2.0).contains(&t.stroke), "{step:?}: §2's 1-2 px");
            let blur = 2.0 * t.glow_sigma;
            assert!((20.0..=40.0).contains(&blur), "{step:?}: §2's 20-40 px");
            assert_eq!(
                params(&TvTheme::default(), step, 1.0).stroke_width,
                t.stroke
            );
            assert_eq!(
                params(&TvTheme::default(), step, 1.0).glow_sigma,
                t.glow_sigma
            );
            assert_eq!(params(&TvTheme::default(), step, 1.0).margin, t.margin);
        }
        assert_eq!((hero.stroke, hero.glow_sigma), (2.0, 15.0));
        assert_eq!((lg.stroke, lg.glow_sigma), (1.5, 10.0));
        // Nothing under 64 is a title; were one drawn, it takes 64's.
        for step in [Step::Md, Step::Body, Step::Caption] {
            assert_eq!(treatment(step), lg, "{step:?}");
        }
    }

    #[test]
    fn the_margin_holds_every_effect() {
        for step in [Step::Hero, Step::Lg] {
            let t = treatment(step);
            let reaches = [
                ("the glow's 2.5 sigma", 2.5 * t.glow_sigma),
                ("the drop", DROP_OFFSET + 2.5 * DROP_SIGMA),
                // §2's deeper hard shadow, 0 3px 0 --gi, under the stroke.
                ("the stroke over the deeper shadow", t.stroke / 2.0 + 3.0),
            ];
            for (what, reach) in reaches {
                assert!(t.margin >= reach, "{step:?}: {what} reaches {reach}");
            }
            // And no more than a few px past the farthest.
            let farthest = reaches.iter().map(|r| r.1).fold(0.0, f32::max);
            assert!(t.margin - farthest <= 4.0, "{step:?}: {}", t.margin);
        }
    }

    /// `title.wesl`'s `TitleParams` has [`TitleParams`]' fields in the same
    /// order, `vec4<f32>` for each colour and `f32` for each size, so the
    /// uniform's bytes land where the shader reads them.
    #[test]
    fn the_shader_reads_the_uniform_as_written() {
        let wesl = include_str!("title.wesl");
        let body = wesl
            .split("struct TitleParams {")
            .nth(1)
            .and_then(|s| s.split('}').next())
            .expect("TitleParams in title.wesl");
        let shader: Vec<(String, String)> = body
            .lines()
            .map(|l| l.split("//").next().unwrap().trim())
            .filter(|l| !l.is_empty())
            .map(|l| {
                let (name, ty) = l.trim_end_matches(',').split_once(':').unwrap();
                (name.trim().to_string(), ty.trim().to_string())
            })
            .collect();
        let p = params(&TvTheme::default(), Step::Hero, 1.0);
        let rust: Vec<(String, String)> = (0..p.field_len())
            .map(|i| {
                let ty = if p.field_at(i).unwrap().try_downcast_ref::<f32>().is_some() {
                    "f32"
                } else {
                    assert!(
                        p.field_at(i)
                            .unwrap()
                            .try_downcast_ref::<LinearRgba>()
                            .is_some()
                    );
                    "vec4<f32>"
                };
                (p.name_at(i).unwrap().to_string(), ty.to_string())
            })
            .collect();
        assert_eq!(shader, rust);
    }

    #[test]
    fn the_shader_is_embedded_where_the_material_looks() {
        let path = bevy::asset::embedded_path!("title.wesl");
        assert_eq!(format!("embedded://{}", path.display()), SHADER);
    }

    #[test]
    fn the_halo_fades_from_the_mocks_gold_to_nothing_at_its_edge() {
        let theme = TvTheme::default();
        let stops = halo_stops(&theme);
        assert_eq!(stops.len(), usize::from(HALO_STOPS) + 1);
        let sky = backdrop(&theme);
        let mut last = f32::MAX;
        for stop in &stops {
            let Val::Percent(at) = stop.point else {
                panic!("{:?}", stop.point);
            };
            // Each stop lands over the sky where the browser's blend of the
            // mock's gold at its alpha there does.
            let want = HALO_ALPHA * (1.0 - at / 100.0);
            assert_eq!(
                stop.color,
                surfaces::css_over(sky, theme.gold, want),
                "{at}%"
            );
            let lands = surfaces::linear_over(sky, stop.color);
            assert!(lands.red <= last + 1e-6, "fading: {at}%");
            last = lands.red;
        }
        assert_eq!(stops[0].point, Val::Percent(0.0));
        assert_eq!(stops.last().unwrap().point, Val::Percent(100.0));
        assert_eq!(
            stops.last().unwrap().color.alpha(),
            0.0,
            "nothing at the edge"
        );
        // The mock's 0.12 at the centre: CSS's transmittance, so stars behind
        // dim as much as in a browser.
        assert!((stops[0].color.alpha() - HALO_ALPHA).abs() < 1e-6);
    }

    // ---- headless: the real screens, no render app ----------------------

    use bevy::camera::RenderTargetInfo;
    use guac_server::config::WindowMode;

    use crate::cards::PortalCards;
    use crate::screen::TvScreen;
    use crate::tv::layout_tests::{portal_app, tv_camera};
    use crate::tv::{Current, scale_ui};

    /// What bevy_render's `camera_system` does for an image-targeted camera,
    /// which the headless app has no render app to run: the target's size is
    /// its image's, its scale factor the `ImageRenderTarget`'s. So a title's
    /// ink lays out headless at the scale it renders at.
    fn size_image_cameras(
        images: Res<Assets<Image>>,
        mut cameras: Query<(&mut Camera, &RenderTarget), With<TitleCamera>>,
    ) {
        for (mut camera, target) in &mut cameras {
            let RenderTarget::Image(target) = target else {
                continue;
            };
            if let Some(image) = images.get(&target.handle) {
                camera.computed.target_info = Some(RenderTargetInfo {
                    physical_size: image.size(),
                    scale_factor: target.scale_factor,
                });
            }
        }
    }

    fn titled(width: u32, height: u32, mode: WindowMode, screen: TvScreen) -> App {
        let mut app = portal_app(width, height, mode, PortalCards::default());
        app.add_systems(PostUpdate, size_image_cameras.before(UiSystems::Prepare));
        app.world_mut().resource_mut::<Current>().screen = screen;
        app
    }

    fn giants(game: &str) -> TvScreen {
        TvScreen::Portal { game: game.into() }
    }

    /// The TV camera's target as a window `physical` px big at `scale_factor`.
    fn set_tv(app: &mut App, physical: UVec2, scale_factor: f32) {
        let world = app.world_mut();
        let tv = tv_camera(world);
        world.get_mut::<Camera>(tv).unwrap().computed.target_info = Some(RenderTargetInfo {
            physical_size: physical,
            scale_factor,
        });
    }

    fn frames(app: &mut App, n: usize) {
        for _ in 0..n {
            app.update();
        }
    }

    /// Every title surface: its entity and what it drew.
    fn surfaces(app: &mut App) -> Vec<(Entity, Option<(Entity, Entity, Entity, Phase)>)> {
        let world = app.world_mut();
        let mut q = world.query::<(Entity, &TitleSurface)>();
        q.iter(world).map(|(e, s)| (e, s.drawn())).collect()
    }

    /// The one title on screen: its camera, ink and phase.
    fn only(app: &mut App) -> (Entity, Entity, Phase) {
        let all = surfaces(app);
        assert_eq!(all.len(), 1, "{all:?}");
        let (camera, _, ink, phase) = all[0].1.expect("drawn");
        (camera, ink, phase)
    }

    fn active(app: &App, camera: Entity) -> bool {
        app.world().get::<Camera>(camera).unwrap().is_active
    }

    /// A camera's image target: its scale factor and its image's size.
    fn target(app: &App, camera: Entity) -> (f32, UVec2, Handle<Image>) {
        let Some(RenderTarget::Image(target)) = app.world().get::<RenderTarget>(camera) else {
            panic!("a title camera draws into an image");
        };
        let size = app
            .world()
            .resource::<Assets<Image>>()
            .get(&target.handle)
            .unwrap()
            .size();
        (target.scale_factor, size, target.handle.clone())
    }

    /// The surface's material: its uniform and the image it samples.
    fn material(app: &App, surface: Entity) -> (TitleParams, Handle<Image>) {
        let node = app
            .world()
            .get::<MaterialNode<TitleMaterial>>(surface)
            .unwrap();
        let m = app
            .world()
            .resource::<Assets<TitleMaterial>>()
            .get(&node.0)
            .unwrap();
        (m.params.clone(), m.glyphs.clone())
    }

    fn count<F: bevy::ecs::query::QueryFilter>(app: &mut App) -> usize {
        let world = app.world_mut();
        world.query_filtered::<(), F>().iter(world).count()
    }

    /// The glyphs laid out for `text`: where each sits and on which line.
    fn glyphs(app: &App, text: Entity) -> Vec<(Vec2, u32)> {
        app.world()
            .get::<TextLayoutInfo>(text)
            .unwrap()
            .glyphs
            .iter()
            .map(|g| (g.position, g.line_index))
            .collect()
    }

    /// Each title's ink lays out glyph for glyph (within half a physical px)
    /// as its hidden layout `Text`, in an image exactly its surface's size,
    /// and the surface is the layout box grown by the margin. Returns the
    /// most lines a title took.
    fn assert_inked_as_laid_out(app: &mut App, at: &str) -> u32 {
        let mut most = 0;
        for (surface, drawn) in surfaces(app) {
            let (camera, _, ink, _) = drawn.unwrap_or_else(|| panic!("{at}: not drawn"));
            let world = app.world();
            let s = world.get::<TitleSurface>(surface).unwrap();
            let layout = s.layout;
            let margin = treatment(s.step).margin;
            let laid = world.get::<ComputedNode>(layout).unwrap();
            let node = world.get::<ComputedNode>(surface).unwrap();
            let px = 1.0 / laid.inverse_scale_factor;
            let grown = laid.size() + Vec2::splat(2.0 * margin * px);
            assert!(
                (node.size() - grown).abs().max_element() <= 1.0,
                "{at}: surface {} vs layout + margin {grown}",
                node.size()
            );
            let (_, image, _) = target(app, camera);
            assert_eq!(image, node.size().ceil().as_uvec2(), "{at}: the image");
            let (want, got) = (glyphs(app, layout), glyphs(app, ink));
            let text = &app.world().get::<Text>(layout).unwrap().0;
            assert!(!want.is_empty(), "{at}: {text:?} laid out no glyphs");
            assert_eq!(want.len(), got.len(), "{at}: {text:?}");
            for ((a, la), (b, lb)) in want.iter().zip(&got) {
                assert_eq!(la, lb, "{at}: {text:?} breaks its lines elsewhere");
                assert!(
                    (*a - *b).abs().max_element() <= 0.5,
                    "{at}: {text:?} glyph at {a} inked at {b}"
                );
            }
            most = most.max(want.iter().map(|g| g.1 + 1).max().unwrap_or(0));
        }
        most
    }

    fn every_title() -> [(TvScreen, &'static str); 8] {
        [
            (TvScreen::Setup, "WHERE ARE YOUR DISC DUMPS?"),
            (TvScreen::NoGiants, "NO SKYLANDERS GIANTS DUMP FOUND"),
            (TvScreen::Found, "FOUND YOUR GAMES"),
            (TvScreen::Join, "SCAN TO PLAY"),
            (giants("Skylanders: Giants"), "SKYLANDERS: GIANTS"),
            (TvScreen::AlreadyRunning, "ALREADY RUNNING"),
            (
                TvScreen::ServerError {
                    message: "bind 0.0.0.0:8765: address in use".into(),
                },
                "SOMETHING WENT WRONG",
            ),
            (TvScreen::Farewell, "GOODBYE, PORTAL MASTER!"),
        ]
    }

    /// The box's: a title camera is a second `Camera2d`, and `UiScale`
    /// still follows the TV's camera, never the title's image.
    #[test]
    fn a_title_camera_leaves_the_ui_scale_with_the_tvs_camera() {
        let mut app = titled(1920, 1080, WindowMode::Tv, TvScreen::Join);
        app.add_systems(Update, scale_ui);
        frames(&mut app, 3);
        assert_eq!(
            count::<With<Camera2d>>(&mut app),
            2,
            "the TV's and the title's"
        );
        assert_eq!(count::<With<TitleCamera>>(&mut app), 1);
        assert_eq!(app.world().resource::<UiScale>().0, 1.0);
        let (camera, ..) = only(&mut app);
        let (_, image, _) = target(&app, camera);
        assert!(image.y < 540, "a title's image, {image}, is no screen");

        set_tv(&mut app, UVec2::new(3840, 2160), 1.0);
        frames(&mut app, 3);
        assert_eq!(app.world().resource::<UiScale>().0, 2.0);
        let (camera, ..) = only(&mut app);
        let (_, at_4k, _) = target(&app, camera);
        assert!(
            (at_4k.as_vec2() - 2.0 * image.as_vec2())
                .abs()
                .max_element()
                <= 2.0,
            "drawn again at 4K: {image} then {at_4k}"
        );
        // Two physical px a design px: `UiScale` times the target's scale.
        let surface = surfaces(&mut app)[0].0;
        assert_eq!(material(&app, surface).0.scale, 2.0);
    }

    /// A title camera draws the frame its glyphs land and is inactive from
    /// the next, and the title renders again only for a new string: nothing
    /// touched, nor a source changed with nothing new, spawns a camera.
    #[test]
    fn a_title_renders_once_and_again_only_for_a_new_string() {
        let mut app = titled(1920, 1080, WindowMode::Tv, giants("Skylanders: Giants"));
        app.update();
        let (camera, _, phase) = only(&mut app);
        assert_eq!(phase, Phase::Waiting);
        assert!(active(&app, camera));
        app.update();
        assert_eq!(only(&mut app), (camera, only(&mut app).1, Phase::Inked));
        assert!(active(&app, camera), "it draws the glyphs this frame");
        app.update();
        assert_eq!(only(&mut app).2, Phase::Kept);
        assert!(!active(&app, camera), "and keeps them");
        let surface = surfaces(&mut app)[0].0;
        let (_, kept) = material(&app, surface);
        assert_eq!(
            target(&app, camera).2,
            kept,
            "the surface samples its camera's image"
        );

        frames(&mut app, 10);
        let world = app.world_mut();
        world.resource_mut::<Current>().set_changed();
        world.resource_mut::<crate::tv::Setup>().set_changed();
        world.resource_mut::<UiScale>().set_changed();
        frames(&mut app, 3);
        assert_eq!(only(&mut app).0, camera);
        assert_eq!(only(&mut app).2, Phase::Kept);
        assert!(!active(&app, camera));
        assert_eq!(material(&app, surface).1, kept);
        assert_eq!(count::<With<TitleCamera>>(&mut app), 1);

        // Another game is no transition, but another string.
        app.world_mut().resource_mut::<Current>().screen = giants("Skylanders: Swap Force");
        app.update();
        let (again, ink, phase) = only(&mut app);
        assert_ne!(again, camera);
        assert!(
            app.world().get_entity(camera).is_err(),
            "the old camera went"
        );
        assert_eq!(phase, Phase::Waiting);
        assert_eq!(
            app.world().get::<Text>(ink).unwrap().0,
            "SKYLANDERS: SWAP FORCE"
        );
        assert_ne!(material(&app, surface).1, kept);
        frames(&mut app, 2);
        assert_eq!(only(&mut app).2, Phase::Kept);
        assert!(!active(&app, again));
        assert_eq!(count::<With<TitleCamera>>(&mut app), 1);
        assert_eq!(count::<With<TitleInk>>(&mut app), 1);
    }

    /// A Retina window draws its titles at its scale factor, not the image's
    /// own 1.0 (AP.8: half size), and a window moved to a display of
    /// another scale draws them again at that one.
    #[test]
    fn the_title_camera_follows_the_tvs_scale_factor() {
        // 1920x1080 logical at 2: a Retina window.
        let mut app = titled(3840, 2160, WindowMode::Tv, TvScreen::Join);
        app.add_systems(Update, scale_ui);
        set_tv(&mut app, UVec2::new(3840, 2160), 2.0);
        frames(&mut app, 4);
        assert_eq!(app.world().resource::<UiScale>().0, 1.0);
        let (camera, _, phase) = only(&mut app);
        assert_eq!(phase, Phase::Kept);
        let (scale, retina, _) = target(&app, camera);
        assert_eq!(scale, 2.0);
        let surface = surfaces(&mut app)[0].0;
        assert_eq!(material(&app, surface).0.scale, 2.0);
        assert_inked_as_laid_out(&mut app, "retina");

        // Dragged to a 1x display: the same window, half the pixels.
        set_tv(&mut app, UVec2::new(1920, 1080), 1.0);
        frames(&mut app, 4);
        let (moved, _, phase) = only(&mut app);
        assert_ne!(moved, camera);
        assert_eq!(phase, Phase::Kept);
        let (scale, plain, _) = target(&app, moved);
        assert_eq!(scale, 1.0);
        assert!(
            (retina.as_vec2() - 2.0 * plain.as_vec2())
                .abs()
                .max_element()
                <= 2.0,
            "{retina} then {plain}"
        );
        assert_eq!(material(&app, surface).0.scale, 1.0);
        assert_inked_as_laid_out(&mut app, "1x");
    }

    /// Every title, at 1080p and 4K on the TV, in the desktop window and in
    /// a portrait one narrow enough to wrap them: the ink lays out glyph for
    /// glyph as the hidden layout `Text`, line breaks included, in caps.
    #[test]
    fn every_title_inks_glyph_for_glyph_as_laid_out() {
        let mut wrapped = 0;
        for (w, h, mode) in [
            (1920, 1080, WindowMode::Tv),
            (3840, 2160, WindowMode::Tv),
            (1280, 720, WindowMode::Desktop),
            (1080, 1350, WindowMode::Desktop),
        ] {
            for (screen, caps) in every_title() {
                let at = format!("{caps} at {w}x{h}");
                let mut app = titled(w, h, mode, screen);
                frames(&mut app, 4);
                let world = app.world_mut();
                let drawn: Vec<String> = world
                    .query_filtered::<&Text, With<TitleText>>()
                    .iter(world)
                    .map(|t| t.0.clone())
                    .collect();
                assert_eq!(drawn, [caps], "{at}");
                let lines = assert_inked_as_laid_out(&mut app, &at);
                if w > h {
                    assert_eq!(lines, 1, "{at}: one line on a 16:9 screen");
                }
                wrapped += usize::from(lines > 1);
            }
        }
        assert!(
            wrapped >= 3,
            "the portrait window wraps the long ones: {wrapped}"
        );
    }

    /// Each screen's own title wears §2's halo, behind it; the portal's game
    /// name, heading the cards from the corner, doesn't; "Starting…" isn't a
    /// title.
    #[test]
    fn the_screen_titles_wear_the_halo_and_the_game_name_doesnt() {
        let mut screens: Vec<(TvScreen, usize)> =
            every_title().into_iter().map(|(s, _)| (s, 1)).collect();
        screens[4].1 = 0;
        screens.push((TvScreen::Starting, 0));
        for (screen, halos) in screens {
            let at = format!("{screen:?}");
            let starting = screen == TvScreen::Starting;
            let mut app = titled(1920, 1080, WindowMode::Tv, screen);
            frames(&mut app, 2);
            assert_eq!(count::<With<TitleHalo>>(&mut app), halos, "{at}");
            assert_eq!(
                count::<With<Title>>(&mut app),
                usize::from(!starting),
                "{at}"
            );
            let world = app.world_mut();
            let mut q = world.query_filtered::<&Children, With<Title>>();
            for kids in q.iter(world) {
                let first = kids[0];
                assert_eq!(
                    world.get::<TitleHalo>(first).is_some(),
                    halos == 1,
                    "{at}: behind"
                );
            }
        }
    }
}
