//! The world's button prompts by the walking pad (openspec AG.290;
//! stand-in ledger: `button-prompt`). The cannon's prompt is a clone the
//! level's scripts make (`Dialog_Button_c`, hung over the barrel by
//! `global.bld`'s dialogue update, AH.226) on the disc's own
//! `ButtonIcons_wii_c.mdl`, whose colour map is the PS3's grey disc with a
//! green △: drawn as is, it named the PlayStation's top button on every pad,
//! where pad-input names the button printed on the pad in hand. So a clone
//! on a button-icon model ([`BUTTON_ICONS`]: the model, the position the
//! original's icon stands for) draws its colour map swapped for a prompt of
//! our own: a grey disc with the walking pad's letter at that position in
//! green ([`pad::prompt`]: X on chotchki's 8BitDo in Switch mode, Y on an
//! Xbox-lettered pad), or the position's glyph on a pad the TV can't place.
//! Only the texture changes, so the material's key and pipeline stay the
//! census's. The letters are drawn in code, not the TV's font: the disc's
//! own button glyphs are images AT's interface reads, which this table can
//! later point at instead.

use std::collections::HashMap;

use bevy::asset::RenderAssetUsages;
use bevy::input::gamepad::Gamepad;
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};

use guacamole::character::CharacterMaterial;
#[cfg(test)]
use guacamole::character::MaterialKey;

use crate::pad::{self, Face, FacesPrinted, Labels, LevelPads, Prompt};

/// The disc's button-icon models and the position each one's icon stands
/// for, by file name.
pub(crate) const BUTTON_ICONS: &[(&str, Face)] = &[
    // The PS3's △: the top button.
    ("ButtonIcons_wii_c.mdl", Face::North),
];

/// The position a model's icon stands for, by its file name (any case);
/// none for a model that isn't a button icon.
pub(crate) fn icon_face(model: &str) -> Option<Face> {
    let file = model.rsplit(['/', '\\']).next().unwrap_or(model);
    BUTTON_ICONS
        .iter()
        .find(|(name, _)| name.eq_ignore_ascii_case(file))
        .map(|(_, face)| *face)
}

/// A clone drawn on a button-icon model: the position its icon stands for.
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ButtonPrompt(pub(crate) Face);

/// One of a prompt clone's parts: the material the spawn part gave it and
/// the prompt it draws now.
#[derive(Component, Debug, Clone, PartialEq)]
pub(crate) struct PromptPart {
    pub(crate) original: Handle<CharacterMaterial>,
    pub(crate) shown: Option<Prompt>,
}

/// The prompts drawn so far: each one's image, and each material copied
/// with it in place of its colour map.
#[derive(Resource, Default)]
pub(crate) struct PromptLook {
    images: HashMap<Prompt, Handle<Image>>,
    materials: HashMap<(AssetId<CharacterMaterial>, Prompt), Handle<CharacterMaterial>>,
}

impl PromptLook {
    /// `original` drawing `prompt`: the same material with the prompt's
    /// image as its colour map, made once.
    fn material(
        &mut self,
        original: &Handle<CharacterMaterial>,
        prompt: Prompt,
        images: &mut Assets<Image>,
        materials: &mut Assets<CharacterMaterial>,
    ) -> Option<Handle<CharacterMaterial>> {
        if let Some(m) = self.materials.get(&(original.id(), prompt)) {
            return Some(m.clone());
        }
        let mut copy = materials.get(original)?.clone();
        let image = self
            .images
            .entry(prompt)
            .or_insert_with(|| images.add(prompt_image(prompt)))
            .clone();
        copy.colour = Some(image);
        let handle = materials.add(copy);
        self.materials
            .insert((original.id(), prompt), handle.clone());
        Some(handle)
    }
}

/// The labels on the pad that walks the hero; a pad the TV can't place,
/// or none walking, its positions.
pub(crate) fn walking_labels(
    pads: &LevelPads,
    gamepads: &Query<(&Gamepad, Option<&FacesPrinted>)>,
) -> Labels {
    pads.seat()
        .and_then(|s| s.walking())
        .and_then(|pad| gamepads.get(pad).ok())
        .map_or(Labels::Position, |(pad, printed)| pad::labels(pad, printed))
}

/// Each prompt clone's parts drawn with the walking pad's prompt, again
/// whenever another pad takes the hero over.
pub(crate) fn skin_prompts(
    pads: Res<LevelPads>,
    gamepads: Query<(&Gamepad, Option<&FacesPrinted>)>,
    clones: Query<(&ButtonPrompt, &Children)>,
    mut parts: Query<(&mut PromptPart, &mut MeshMaterial3d<CharacterMaterial>)>,
    mut look: ResMut<PromptLook>,
    mut images: ResMut<Assets<Image>>,
    mut materials: ResMut<Assets<CharacterMaterial>>,
) {
    let labels = walking_labels(&pads, &gamepads);
    for (ButtonPrompt(face), children) in &clones {
        let prompt = pad::prompt(labels, *face);
        for child in children.iter() {
            let Ok((mut part, mut material)) = parts.get_mut(child) else {
                continue;
            };
            if part.shown == Some(prompt) {
                continue;
            }
            if let Some(m) = look.material(&part.original, prompt, &mut images, &mut materials) {
                material.0 = m;
                part.shown = Some(prompt);
            }
        }
    }
}

/// The prompts' side of the TV.
pub(crate) struct ButtonPromptPlugin;

impl Plugin for ButtonPromptPlugin {
    fn build(&self, app: &mut App) {
        // The clones' parts are spawned in the fixed steps
        // (`hero_scripts::draw_clones`), ahead of this frame's update.
        app.init_resource::<PromptLook>()
            .add_systems(Update, skin_prompts);
    }
}

/// The prompt image's side, texels.
pub(crate) const SIZE: u32 = 128;
/// The disc's grey, its rim's and the letter's green, sRGB.
const GREY: [u8; 3] = [150, 150, 150];
const RIM: [u8; 3] = [96, 96, 96];
const GREEN: [u8; 3] = [40, 196, 64];
/// A position glyph's other dots.
const DOT: [u8; 3] = [70, 70, 70];

/// The strokes each letter is drawn with, in a unit square, y down.
fn strokes(letter: &str) -> &'static [[(f32, f32); 2]] {
    match letter {
        "A" => &[
            [(0.22, 0.84), (0.5, 0.16)],
            [(0.5, 0.16), (0.78, 0.84)],
            [(0.34, 0.58), (0.66, 0.58)],
        ],
        "B" => &[
            [(0.3, 0.16), (0.3, 0.84)],
            [(0.3, 0.16), (0.58, 0.16)],
            [(0.58, 0.16), (0.68, 0.25)],
            [(0.68, 0.25), (0.68, 0.41)],
            [(0.68, 0.41), (0.58, 0.5)],
            [(0.3, 0.5), (0.62, 0.5)],
            [(0.62, 0.5), (0.72, 0.59)],
            [(0.72, 0.59), (0.72, 0.75)],
            [(0.72, 0.75), (0.62, 0.84)],
            [(0.62, 0.84), (0.3, 0.84)],
        ],
        "X" => &[[(0.24, 0.18), (0.76, 0.82)], [(0.76, 0.18), (0.24, 0.82)]],
        "Y" => &[
            [(0.24, 0.18), (0.5, 0.5)],
            [(0.76, 0.18), (0.5, 0.5)],
            [(0.5, 0.5), (0.5, 0.84)],
        ],
        _ => &[],
    }
}

/// How far `p` lies from the segment `a`-`b`.
fn to_segment(p: (f32, f32), [a, b]: [(f32, f32); 2]) -> f32 {
    let (dx, dy) = (b.0 - a.0, b.1 - a.1);
    let t = (((p.0 - a.0) * dx + (p.1 - a.1) * dy) / (dx * dx + dy * dy)).clamp(0.0, 1.0);
    ((p.0 - a.0 - t * dx).powi(2) + (p.1 - a.1 - t * dy).powi(2)).sqrt()
}

/// How much of a texel `d` (in the image's units) inside an edge covers.
fn cover(d: f32) -> f32 {
    (d * SIZE as f32 + 0.5).clamp(0.0, 1.0)
}

fn mix(a: [u8; 3], b: [u8; 3], t: f32) -> [u8; 3] {
    std::array::from_fn(|i| {
        (f32::from(a[i]) + (f32::from(b[i]) - f32::from(a[i])) * t).round() as u8
    })
}

/// The colour at `p` (a unit square, y down) of `prompt`'s face.
fn texel(prompt: Prompt, p: (f32, f32)) -> [u8; 3] {
    let centre = (0.5, 0.5);
    let r = ((p.0 - centre.0).powi(2) + (p.1 - centre.1).powi(2)).sqrt();
    // The disc, its rim darker; outside it the rim's grey, so a face that
    // maps past the disc shows nothing new.
    let mut c = mix(RIM, GREY, cover(0.42 - r));
    match prompt {
        Prompt::Letter(letter) => {
            // The letter in the disc's inner 60 %.
            let q = ((p.0 - 0.2) / 0.6, (p.1 - 0.2) / 0.6);
            let d = strokes(letter)
                .iter()
                .map(|s| to_segment(q, *s))
                .fold(f32::INFINITY, f32::min);
            c = mix(c, GREEN, cover((0.075 - d) * 0.6));
        }
        Prompt::Position(face) => {
            for (f, at) in [
                (Face::North, (0.5, 0.24)),
                (Face::East, (0.76, 0.5)),
                (Face::South, (0.5, 0.76)),
                (Face::West, (0.24, 0.5)),
            ] {
                let d = ((p.0 - at.0).powi(2) + (p.1 - at.1).powi(2)).sqrt();
                let ink = if f == face { GREEN } else { DOT };
                c = mix(c, ink, cover(0.1 - d));
            }
        }
    }
    c
}

/// `prompt`'s face as an sRGB image, opaque, the letter upright with its
/// rows top first as the disc's textures keep theirs.
pub(crate) fn prompt_image(prompt: Prompt) -> Image {
    let mut data = Vec::with_capacity((SIZE * SIZE * 4) as usize);
    for y in 0..SIZE {
        for x in 0..SIZE {
            let p = (
                (x as f32 + 0.5) / SIZE as f32,
                (y as f32 + 0.5) / SIZE as f32,
            );
            let [r, g, b] = texel(prompt, p);
            data.extend([r, g, b, 255]);
        }
    }
    Image::new(
        Extent3d {
            width: SIZE,
            height: SIZE,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        data,
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::RENDER_WORLD,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_cannons_icon_is_the_top_button_by_its_file_name() {
        assert_eq!(
            icon_face("C:/tfb/Content/Models/UI/ButtonIcons_wii_c.mdl"),
            Some(Face::North)
        );
        assert_eq!(icon_face("buttonicons_WII_C.MDL"), Some(Face::North));
        assert_eq!(icon_face("C:/tfb/Content/Models/UI/Dialog_Arrow.mdl"), None);
    }

    /// Each prompt draws its own face: the four letters and the four
    /// positions all differ, green where a stroke or the position's dot
    /// is, the disc's grey where nothing is.
    #[test]
    fn each_prompt_draws_its_own_face() {
        let prompts = [
            Prompt::Letter("A"),
            Prompt::Letter("B"),
            Prompt::Letter("X"),
            Prompt::Letter("Y"),
            Prompt::Position(Face::South),
            Prompt::Position(Face::East),
            Prompt::Position(Face::West),
            Prompt::Position(Face::North),
        ];
        let images: Vec<Vec<u8>> = prompts
            .iter()
            .map(|p| prompt_image(*p).data.unwrap())
            .collect();
        for (i, a) in images.iter().enumerate() {
            assert_eq!(a.len(), (SIZE * SIZE * 4) as usize);
            for b in &images[i + 1..] {
                assert_ne!(a, b, "{:?}", prompts[i]);
            }
        }
        // X's centre is a stroke; the disc between X's arms isn't.
        assert_eq!(texel(Prompt::Letter("X"), (0.5, 0.5)), GREEN);
        assert_eq!(texel(Prompt::Letter("X"), (0.5, 0.3)), GREY);
        // The top position's dot green, the bottom's not.
        assert_eq!(texel(Prompt::Position(Face::North), (0.5, 0.24)), GREEN);
        assert_eq!(texel(Prompt::Position(Face::North), (0.5, 0.76)), DOT);
        // Past the disc, its rim's grey.
        assert_eq!(texel(Prompt::Letter("A"), (0.02, 0.02)), RIM);
    }

    /// An opaque card on neutral tints, its colour map `colour`.
    fn card(colour: Handle<Image>) -> CharacterMaterial {
        use guacamole::character::key::{MaterialKey, Pass, ShaderKey, StateKey};
        use guacamole::character::material::CharacterTextures;
        let key = MaterialKey {
            shader: ShaderKey::new(0, 0).unwrap(),
            states: StateKey::OPAQUE,
            pass: Pass::Colour,
        };
        CharacterMaterial::new(
            key,
            &guac_ir::shading::Material::new(),
            &guac_ir::shading::Scene::new(),
            0.0,
            CharacterTextures {
                colour: Some(colour),
                ..CharacterTextures::default()
            },
        )
        .unwrap()
    }

    /// AG.290: a prompt clone's part draws the walking pad's top letter in
    /// place of the disc's △: X for the 8BitDo in Switch mode (Nintendo's
    /// letters), Y once an Xbox-lettered pad walks, the top position with
    /// none walking; never the model's own map, its key the same.
    #[test]
    fn the_prompt_names_the_walking_pads_top_button() {
        use crate::test_support::test_pad;
        let mut app = App::new();
        app.add_plugins((MinimalPlugins, bevy::input::InputPlugin))
            .init_resource::<Assets<Image>>()
            .init_resource::<Assets<CharacterMaterial>>()
            .init_resource::<LevelPads>()
            .add_plugins(ButtonPromptPlugin);
        let disc = app
            .world_mut()
            .resource_mut::<Assets<Image>>()
            .add(Image::default());
        let original = app
            .world_mut()
            .resource_mut::<Assets<CharacterMaterial>>()
            .add(card(disc.clone()));
        let clone = app.world_mut().spawn(ButtonPrompt(Face::North)).id();
        let part = app
            .world_mut()
            .spawn((
                PromptPart {
                    original: original.clone(),
                    shown: None,
                },
                MeshMaterial3d(original.clone()),
                ChildOf(clone),
            ))
            .id();
        let drawn = |app: &App| -> (Prompt, Vec<u8>, MaterialKey) {
            let part = app.world().entity(part);
            let shown = part.get::<PromptPart>().unwrap().shown.unwrap();
            let handle = &part.get::<MeshMaterial3d<CharacterMaterial>>().unwrap().0;
            assert_ne!(handle, &original, "never the model's own");
            let m = app
                .world()
                .resource::<Assets<CharacterMaterial>>()
                .get(handle)
                .unwrap();
            let image = m.colour.as_ref().unwrap();
            let data = app.world().resource::<Assets<Image>>().get(image).unwrap();
            (shown, data.data.clone().unwrap(), m.key)
        };
        let want = |p: Prompt| prompt_image(p).data.unwrap();
        // None walking: the top position.
        app.update();
        let (shown, data, key) = drawn(&app);
        assert_eq!(shown, Prompt::Position(Face::North));
        assert_eq!(data, want(Prompt::Position(Face::North)));
        let original_key = app
            .world()
            .resource::<Assets<CharacterMaterial>>()
            .get(&original)
            .unwrap()
            .key;
        assert_eq!(key, original_key, "the key the census's");
        // The 8BitDo in Switch mode walking: X.
        let pad = test_pad::connect(&mut app, None, None);
        app.update();
        app.world_mut().entity_mut(pad).insert(FacesPrinted([
            Some('B'),
            Some('A'),
            Some('Y'),
            Some('X'),
        ]));
        let frame = crate::pad::PadFrame {
            id: pad,
            stick: Vec2::ZERO,
            pressed: Vec::new(),
            held: [false; 4],
            shoulders: [false; 4],
        };
        {
            let mut pads = app.world_mut().resource_mut::<LevelPads>();
            pads.show();
            pads.seat_mut().unwrap().step(&[frame]);
            assert_eq!(pads.seat().unwrap().walking(), Some(pad));
        }
        app.update();
        let (shown, data, _) = drawn(&app);
        assert_eq!(shown, Prompt::Letter("X"));
        assert_eq!(data, want(Prompt::Letter("X")));
        // An Xbox-lettered pad's letters: Y.
        app.world_mut().entity_mut(pad).insert(FacesPrinted([
            Some('A'),
            Some('B'),
            Some('X'),
            Some('Y'),
        ]));
        app.update();
        let (shown, data, _) = drawn(&app);
        assert_eq!(shown, Prompt::Letter("Y"));
        assert_eq!(data, want(Prompt::Letter("Y")));
    }
}
