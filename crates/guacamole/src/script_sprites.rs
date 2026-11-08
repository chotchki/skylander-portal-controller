//! The scripts' own sprites over a level (openspec AS.99; level-scene's A
//! carried bomb shows its count): the interface's model lays them out and
//! says what they draw ([`guacamole::interface_host`]'s `DRAWN_SPRITES`), and
//! this puts each image on the screen at its place in the model's 1280 by 720
//! frame, a node a glyph, its alpha what the scripts slide it to. The model
//! runs for the scripts whether or not a sprite shows; only a listed one's
//! clone costs a layout here.
//!
//! Each image comes off the extract the first time one of its quads shows.

use std::collections::HashMap;

use bevy::asset::RenderAssetUsages;
use bevy::image::{CompressedImageFormats, ImageSampler, ImageType};
use bevy::prelude::*;
use bevy::render::renderer::RenderDevice;
use guacamole::interface_host::SpriteQuad;
use guacamole::scripts::ScriptWorld;
use serde::Serialize;
use tracing::warn;

use crate::completion::glyph_tint;
use crate::hero_scripts::ScriptSource;
use crate::pad::LevelPads;
use crate::play::PlaySystems;

/// Over the level and its HUD (800), beside a bite's numbers (850), under
/// the announcements (860) and all that takes the screen.
const SPRITE_Z: i32 = 850;

/// The model's frame, pixels.
const FRAME: [f32; 2] = [1280.0, 720.0];

/// Marks what's drawn, which the shots and the mirror find it by.
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ScriptSprites;

/// One image drawn, its part of `/api/_test/tv`.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct SpriteShot {
    pub sprite: String,
    pub value: f32,
    pub rect: [f32; 4],
    pub alpha: u8,
}

/// What's drawn, its part of `/api/_test/tv`, and the images that didn't
/// load, by path with why.
#[derive(Resource, Debug, Clone, Default, PartialEq, Serialize)]
pub struct SpriteReport {
    pub shown: Vec<SpriteShot>,
    pub refused: Vec<(String, String)>,
}

/// The images read so far, none for one that didn't load.
#[derive(Resource, Default)]
struct Images(HashMap<String, Option<Handle<Image>>>);

pub(crate) struct ScriptSpritesPlugin;

impl Plugin for ScriptSpritesPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<SpriteReport>()
            .init_resource::<Images>()
            .add_systems(Update, show.after(PlaySystems));
    }
}

/// `rect`, in the model's frame, as shares of the picture: left, top, width,
/// height, percent.
pub(crate) fn place(rect: [f32; 4]) -> [f32; 4] {
    [
        rect[0] / FRAME[0] * 100.0,
        rect[1] / FRAME[1] * 100.0,
        (rect[2] - rect[0]) / FRAME[0] * 100.0,
        (rect[3] - rect[1]) / FRAME[1] * 100.0,
    ]
}

/// `rel` off the extract at `dir`.
fn read_image(
    dir: &std::path::Path,
    rel: &str,
    formats: CompressedImageFormats,
) -> Result<Image, String> {
    let bytes = std::fs::read(dir.join(rel)).map_err(|e| e.to_string())?;
    Image::from_buffer(
        &bytes,
        ImageType::Extension("ktx2"),
        formats,
        true,
        ImageSampler::linear(),
        RenderAssetUsages::RENDER_WORLD,
    )
    .map_err(|e| e.to_string())
}

/// `quad`'s node: where the model puts it, its cut of the image and its tint.
fn node(quad: &SpriteQuad, size: Vec2, image: Handle<Image>) -> (Node, ImageNode) {
    let [left, top, width, height] = place(quad.rect);
    let [r, g, b, a] = quad.colour;
    (
        Node {
            position_type: PositionType::Absolute,
            left: percent(left),
            top: percent(top),
            width: percent(width),
            height: percent(height),
            ..default()
        },
        ImageNode {
            color: glyph_tint(Color::srgba_u8(r, g, b, a)),
            rect: Some(Rect::new(
                quad.uv[0] * size.x,
                quad.uv[1] * size.y,
                quad.uv[2] * size.x,
                quad.uv[3] * size.y,
            )),
            ..ImageNode::new(image)
        },
    )
}

#[allow(clippy::too_many_arguments)]
fn show(
    mut commands: Commands,
    pads: Res<LevelPads>,
    world: Option<ResMut<ScriptWorld>>,
    source: Option<Res<ScriptSource>>,
    device: Option<Res<RenderDevice>>,
    mut images: ResMut<Assets<Image>>,
    mut read: ResMut<Images>,
    mut report: ResMut<SpriteReport>,
    mut shown: Local<Option<Entity>>,
) {
    let quads = match world {
        Some(w) if pads.shows() && w.draws_sprites() => w.into_inner().drawn_sprites(),
        _ => Vec::new(),
    };
    if let Some(root) = shown.take_if(|_| report.shown.is_empty() && quads.is_empty())
        && let Ok(mut root) = commands.get_entity(root)
    {
        root.despawn();
    }
    if quads.is_empty() && report.shown.is_empty() {
        return;
    }
    let formats = device.as_deref().map_or(CompressedImageFormats::BC, |d| {
        CompressedImageFormats::from_features(d.features())
    });
    let dir = source.as_deref().and_then(ScriptSource::extract);
    let mut drawn = Vec::new();
    let mut shots = Vec::new();
    for quad in &quads {
        if !read.0.contains_key(&quad.image) {
            let image = dir
                .ok_or("the run's extract isn't known".to_owned())
                .and_then(|(dir, _)| read_image(dir, &quad.image, formats));
            match &image {
                Ok(_) => {}
                Err(why) => {
                    warn!(image = %quad.image, reason = %why, "a sprite's image didn't load");
                    report.refused.push((quad.image.clone(), why.clone()));
                }
            }
            let handle = image.ok().map(|i| images.add(i));
            read.0.insert(quad.image.clone(), handle);
        }
        let Some(Some(handle)) = read.0.get(&quad.image) else {
            continue;
        };
        let Some(size) = images.get(handle).map(|i| i.size_f32()) else {
            continue;
        };
        drawn.push(node(quad, size, handle.clone()));
        shots.push(SpriteShot {
            sprite: quad.sprite.clone(),
            value: quad.value,
            rect: quad.rect,
            alpha: quad.colour[3],
        });
    }
    // Rebuilt whole: a few glyphs, and only while one shows.
    if let Some(root) = shown.take()
        && let Ok(mut root) = commands.get_entity(root)
    {
        root.despawn();
    }
    if !drawn.is_empty() {
        let root = commands
            .spawn((
                ScriptSprites,
                Node {
                    position_type: PositionType::Absolute,
                    left: px(0),
                    top: px(0),
                    width: percent(100),
                    height: percent(100),
                    ..default()
                },
                GlobalZIndex(SPRITE_Z),
            ))
            .with_children(|b| {
                for quad in drawn {
                    b.spawn(quad);
                }
            })
            .id();
        *shown = Some(root);
    }
    if report.shown != shots {
        report.shown = shots;
    }
}

#[cfg(test)]
mod tests {
    use guac_interface::model::Str;
    use guac_interface::sprite::{SpriteId, kind, props};
    use guac_interface::{Keeper, Model};
    use guac_ir::interface::{Content, Font, Glyph, Template};
    use guac_script::{Obj, Value};
    use guacamole::interface_host::{DRAWN_SPRITES, SpriteQuad};
    use guacamole::scripts::{Files, ScriptWorld, headless_handles, headless_objects};

    use super::*;

    fn glyph(c: char) -> Glyph {
        Glyph {
            char: c as u16,
            step: 40.0,
            offset: [0.0, 0.0],
            size: [40.0, 82.0],
            uv_min: [0.25, 0.5],
            uv_max: [0.5, 1.0],
            kern: [0; 4],
        }
    }

    fn template(name: &str) -> Template {
        Template {
            name: name.into(),
            object: 1,
            properties: props::VISIBLE | (1 << 3) | kind::TEXT,
            flags: 0,
            position: [0, 0],
            z_depth: 0.0,
            priority: 0.0,
            width: 0.0,
            height: 0.0,
            scale: [100.0; 3],
            color: [255, 255, 255, 255],
            replace_color: [0x80, 0x80, 0x80, 0],
            num_value: 0,
            depth: 0.0,
            init_matrix: Default::default(),
            matrix: Default::default(),
            content: Content::Container,
            string: None,
        }
    }

    /// A world with a counter's clone and another text's, each `7` in a font
    /// off `textures/font.ktx2`: the world and the counter's clone.
    fn world() -> (ScriptWorld, SpriteId) {
        world_with(|_, _| {})
    }

    /// [`world`], `then` given the model and the counter's clone first.
    fn world_with(then: impl FnOnce(&mut Model, SpriteId)) -> (ScriptWorld, SpriteId) {
        let mut m = Model::default();
        let font = m.add_font(Font {
            name: None,
            source: None,
            height: 82.0,
            base_line: 0.0,
            char_padding: 0.0,
            kern_amount: 0.0,
            color: [0.5, 0.5, 0.5, 1.0],
            atlas: "textures/font.ktx2".into(),
            atlas_size: [1024, 1024],
            glyphs: vec![glyph('7')],
        });
        let s = m.add_file_string(Obj { unit: 0, index: 3 }, "7", Str::default());
        let counter = m.add_template(None, &template(DRAWN_SPRITES[0]), s);
        let other = m.add_template(None, &template("Generic_Text"), s);
        m.set_font(counter, Some(font));
        m.set_font(other, Some(font));
        let clone = m.spawn(counter);
        m.spawn(other);
        let point = guac_interface::find("spriteinfo", "point size").unwrap();
        m.set(clone, point, Value::Int(82)).unwrap();
        then(&mut m, clone);
        let files = Files::default();
        let handles = headless_handles(&files);
        let mut w = ScriptWorld::new(&files, &handles, headless_objects(&handles));
        w.set_interface(Keeper::new(m));
        (w, clone)
    }

    /// AS.99: a listed sprite's clone draws its glyph; another's doesn't;
    /// taken off, none is left.
    #[test]
    fn only_a_listed_sprites_clone_draws_and_none_once_its_gone() {
        let (mut w, _) = world();
        assert!(w.draws_sprites());
        let quads = w.drawn_sprites();
        assert_eq!(quads.len(), 1, "{quads:?}");
        let q = &quads[0];
        assert_eq!(q.sprite, DRAWN_SPRITES[0]);
        assert_eq!(q.image, "textures/font.ktx2");
        assert_eq!(q.uv, [0.25, 0.5, 0.5, 1.0]);
        assert_eq!(q.colour, [255, 255, 255, 255], "its colour by its letter's");
    }

    /// AS.99: a glyph drawn takes its letter's tint over its sprite's colour
    /// (the text colour converter's colour set), as the model draws it.
    #[test]
    fn a_drawn_glyph_takes_its_letters_tint() {
        let (mut w, _) = world_with(|m, clone| {
            let sprite = m.value_of(clone);
            let text = guac_interface::find("textcolorconverter", "text sprite").unwrap();
            m.set_global(text, sprite, &guac_interface::NoWorld)
                .unwrap();
            let colours = guac_interface::find("textcolorconverter", "color set").unwrap();
            let letter = [0x40, 0x80, 0xFF, 0xFF];
            m.set_global_elements(colours, &[Value::Color(u32::from_be_bytes(letter))])
                .unwrap();
        });
        let quads = w.drawn_sprites();
        assert_eq!(quads.len(), 1, "{quads:?}");
        assert_eq!(
            quads[0].colour,
            [127, 255, 255, 255],
            "half red, the rest full"
        );
    }

    /// AS.99: a quad lands at its share of the picture, its cut in the
    /// image's pixels, its alpha the sprite's, its body the colour asked.
    #[test]
    fn a_quad_lands_at_its_share_of_the_picture() {
        assert_eq!(place([640.0, 360.0, 704.0, 432.0]), [50.0, 50.0, 5.0, 10.0]);
        let quad = SpriteQuad {
            sprite: "Bomb_Counter".into(),
            image: "x".into(),
            rect: [640.0, 360.0, 704.0, 432.0],
            uv: [0.25, 0.5, 0.5, 1.0],
            colour: [255, 255, 255, 127],
            value: 7.0,
        };
        let (node, image) = node(&quad, Vec2::new(1024.0, 512.0), Handle::default());
        assert_eq!(node.left, percent(50.0));
        assert_eq!(node.height, percent(10.0));
        assert_eq!(
            image.rect,
            Some(Rect::new(256.0, 256.0, 512.0, 512.0)),
            "the glyph's cut"
        );
        assert!((image.color.alpha() - 127.0 / 255.0).abs() < 1e-3);
    }

    fn app(world: ScriptWorld) -> App {
        let mut pads = LevelPads::default();
        pads.show();
        let mut app = App::new();
        app.add_plugins(MinimalPlugins)
            .init_resource::<Assets<Image>>()
            .insert_resource(world)
            .insert_resource(pads)
            .add_plugins(ScriptSpritesPlugin);
        app
    }

    /// AS.99: on the TV a listed clone draws a node a glyph, reported with
    /// its number; its image not on the extract, it's refused by name and
    /// nothing draws; the sprite gone, the overlay goes with it.
    #[test]
    fn a_sprite_draws_and_goes_and_one_with_no_image_is_refused() {
        let (w, _) = world();
        let mut app = app(w);
        app.update();
        let report = app.world().resource::<SpriteReport>();
        assert_eq!(report.shown, []);
        assert_eq!(report.refused.len(), 1, "{report:?}");
        assert_eq!(report.refused[0].0, "textures/font.ktx2");
        // The read is made once, for the image.
        app.update();
        assert_eq!(app.world().resource::<SpriteReport>().refused.len(), 1);
        // Read: the image in, the sprite draws.
        let handle = app
            .world_mut()
            .resource_mut::<Assets<Image>>()
            .add(Image::default());
        app.world_mut()
            .resource_mut::<Images>()
            .0
            .insert("textures/font.ktx2".into(), Some(handle));
        app.update();
        app.update();
        let shown = app.world().resource::<SpriteReport>().shown.clone();
        assert_eq!(shown.len(), 1, "{shown:?}");
        assert_eq!(shown[0].alpha, 255);
        let mut roots = app
            .world_mut()
            .query_filtered::<&Children, With<ScriptSprites>>();
        let drawn: Vec<usize> = roots.iter(app.world()).map(|c| c.len()).collect();
        assert_eq!(drawn, [1], "one root, a node a glyph");
        // Gone, as the timer's shutdown takes it off.
        app.world_mut()
            .resource_mut::<ScriptWorld>()
            .set_interface(Keeper::new(Model::default()));
        app.update();
        app.update();
        assert_eq!(app.world().resource::<SpriteReport>().shown, []);
        let mut roots = app
            .world_mut()
            .query_filtered::<Entity, With<ScriptSprites>>();
        assert_eq!(roots.iter(app.world()).count(), 0, "the overlay gone");
    }
}
