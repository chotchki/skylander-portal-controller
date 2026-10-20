//! The level on the TV (openspec AG.19, design decision 9): a level's scene
//! from the IR on AE's material in the remaster, lit by its own lights. The
//! arithmetic the TV's loader (the binary's `level.rs`), the warm and the
//! tests share:
//!
//! - What's drawn ([`drawn`]): every tfbmulti2 segment, the scene's sky
//!   among them, on its own material, the one with a gloss map (AG.22)
//!   included. The other effects' (the grass's `0xAA6`, `0xAA8`, the warp)
//!   are AG.24's stand-ins and draw nothing yet.
//! - The light ([`level_lights`], [`view_scene`]): `_worldLight1`-`3`, each
//!   travelling the way it faces, as AE.13 reads the sprite rig, and the
//!   placed point lights; fog from a [`LevelFog`] the TV holds, a placeholder
//!   until AG.37 sets the fog AG.13 measures; no shadows (AG.23). The shader
//!   lights in view space, so the scene is the lights seen from a camera.
//! - The camera AF's fresh state shows ([`fresh_state_transform`],
//!   [`fresh_state_projection`]), which the dev entry draws the level at.

use bevy::camera::PerspectiveProjection;
use bevy::math::{Mat4, Vec3};
use bevy::prelude::Transform;
use guac_ir::level::{LevelMaterial, Lights, SceneGraph};
use guac_ir::model::{self, Segment};
use guac_ir::shading::{DirectionalLight, PointLight, Scene};

use crate::character::key::bit;
use crate::character::remaster;
use crate::character::warm::{self, WarmError, WarmKey};

/// The directional lights tfbmulti2 reads, by name. The rig's fourth,
/// `_causticLight`, plays a role nobody has read yet (AG.13's capture may
/// show it), so the level draws without it.
pub const WORLD_LIGHTS: [&str; 3] = ["_worldLight1", "_worldLight2", "_worldLight3"];
/// A placed light's `_type` for a point light (level 213's one).
pub const POINT_LIGHT: u32 = 1;
/// The most point lights tfbmulti2 takes (bits 11-13).
pub const MAX_POINT_LIGHTS: usize = 3;

/// The level's fog: its colour as a disc stores one (encoded for display,
/// decoded like every colour the remaster takes) and where it starts and
/// ends along the view, in metres. The TV holds one as a resource, every
/// level material's fog term reads it, and a change relights them all; it
/// starts at [`LevelFog::PLACEHOLDER`].
#[derive(bevy::prelude::Resource, Debug, Clone, Copy, PartialEq)]
pub struct LevelFog {
    pub colour: [u8; 3],
    pub near: f32,
    pub far: f32,
}

impl Default for LevelFog {
    fn default() -> Self {
        Self::PLACEHOLDER
    }
}

impl LevelFog {
    /// A placeholder, not a reading, until AG.37 puts in the fog AG.13
    /// measures in play: on, since every level material keys fog and a
    /// spec-mapped part needs the eye direction it brings, but black and
    /// starting past the camera's far clip, so it adds nothing.
    pub const PLACEHOLDER: Self = Self {
        colour: [0, 0, 0],
        near: 1.0e4,
        far: 2.0e4,
    };
}

#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum LevelError {
    #[error("the level has no {0}")]
    MissingLight(&'static str),
    #[error("segment {segment} names material {material}, past the scene's {materials}")]
    Segment {
        segment: usize,
        material: u16,
        materials: usize,
    },
    #[error(transparent)]
    Keys(#[from] WarmError),
}

/// The way a light faces, in the level's frame: a `tfbEulerTransform`'s
/// tilt and heading in degrees as AF.9 pinned them (heading θ faces
/// (sin θ, 0, cos θ), a positive tilt lifts it; roll leaves it).
pub fn facing(rotation: [f32; 3]) -> Vec3 {
    let (tilt, heading) = (rotation[0].to_radians(), rotation[1].to_radians());
    Vec3::new(
        tilt.cos() * heading.sin(),
        tilt.sin(),
        tilt.cos() * heading.cos(),
    )
}

/// The level's lights in its own frame, before a camera sees them.
#[derive(Debug, Clone, PartialEq)]
pub struct LevelLights {
    /// `_worldLight1`-`3`: each one's colours decoded as the remaster
    /// decodes them, its travel the way it faces, in the level's frame.
    pub directional: [DirectionalLight; 3],
    /// Visible point lights, at their positions in the level's frame.
    pub points: Vec<PointLight>,
    /// The lights not drawn, each with why.
    pub left_out: Vec<(String, &'static str)>,
}

/// The level's lights off its lights part. Each light's multiplier is 1 on
/// the disc and what another would do is unread, so none is applied (as on
/// the turntable). A point light's falloff takes the light's linear and
/// quadratic attenuation as the shader's two terms and drops its constant,
/// which the shader's falloff has no place for: a reading, not a
/// measurement, since how the runtime fills those terms is ae-shader's open
/// item (level 213's one sits at the origin with (0, 0.5, 0), about 103 m
/// from the start, where its light is under 1 % of its colour).
pub fn level_lights(lights: &Lights) -> Result<LevelLights, LevelError> {
    let rgb = |c: [u8; 4]| [c[0], c[1], c[2]];
    let mut directional = [DirectionalLight::OFF; 3];
    for (slot, name) in directional.iter_mut().zip(WORLD_LIGHTS) {
        let l = lights
            .directional
            .iter()
            .find(|l| l.name == name)
            .ok_or(LevelError::MissingLight(name))?;
        *slot = remaster::light(
            facing(l.rotation).to_array(),
            rgb(l.ambient),
            rgb(l.diffuse),
            rgb(l.specular),
        );
    }
    let mut left_out: Vec<(String, &'static str)> = lights
        .directional
        .iter()
        .filter(|l| !WORLD_LIGHTS.contains(&l.name.as_str()))
        .map(|l| (l.name.clone(), "not one of the three the materials read"))
        .collect();
    let mut points = Vec::new();
    for l in &lights.placed {
        let why = if l.light_type != POINT_LIGHT {
            Some("not a point light")
        } else if !l.visible {
            Some("not visible")
        } else if points.len() == MAX_POINT_LIGHTS {
            Some("past the three point lights tfbmulti2 takes")
        } else {
            None
        };
        if let Some(why) = why {
            left_out.push((l.name.clone(), why));
            continue;
        }
        points.push(PointLight {
            position: l.position,
            colour: remaster::linear(rgb(l.diffuse)),
            linear: l.attenuation[1],
            quadratic: l.attenuation[2],
        });
    }
    Ok(LevelLights {
        directional,
        points,
        left_out,
    })
}

/// The scene the level's materials draw in, seen from a camera whose view
/// is `view_from_world`: the directional lights' travel and the point
/// lights' positions in its view space (the space the shader lights in),
/// fog from `fog`, no shadows until AG.23, neutral tints and no replace
/// colour.
pub fn view_scene(lights: &LevelLights, fog: &LevelFog, view_from_world: Mat4) -> Scene {
    let mut scene = Scene::new();
    scene.lights = lights.directional.map(|mut l| {
        l.travel = view_from_world
            .transform_vector3(Vec3::from_array(l.travel))
            .normalize_or_zero()
            .to_array();
        l
    });
    scene.point_lights = lights
        .points
        .iter()
        .map(|p| PointLight {
            position: view_from_world
                .transform_point3(Vec3::from_array(p.position))
                .to_array(),
            ..*p
        })
        .collect();
    scene.fog = true;
    scene.fog_colour = remaster::linear(fog.colour);
    (scene.fog_near, scene.fog_far) = (fog.near, fog.far);
    scene
}

/// A second layer's exponent and detail weight (`layer2ExtraValues`) when
/// its material sets none, as the first layer's default is.
pub const LAYER2_EXTRA: [f32; 4] = [1.0, 1.0, 1.0, 0.25];
/// A second layer's blend when its material sets none: over.
pub const LAYER2_BLEND: [f32; 4] = [1.0, 0.0, 0.0, 0.0];
/// A third layer's blend when its material sets none: multiply.
pub const LAYER3_BLEND: [f32; 4] = [0.0, 0.0, 1.0, 0.0];

/// What a level material draws as here: tfbmulti2's, with a second or
/// third layer's values the material doesn't set at the original's
/// defaults; `None` for another effect's. Most of level 213's layered materials set
/// none (no third layer sets a blend; of the 177 second layers, 174 set no
/// blend and 148 no exponent), and the June state's draws of them read
/// these values in every patched copy bar the few that set their own
/// (`ag19/notes/layer_defaults.txt`, over ae-shader's savestate check).
pub fn drawn_material(m: &LevelMaterial) -> Option<model::Material> {
    let LevelMaterial::Multi2(m) = m else {
        return None;
    };
    let mut m = m.clone();
    let p = &mut m.params;
    if m.pixel.base >> bit::LAYER2 & 1 == 1 {
        p.layer2_extra.get_or_insert(LAYER2_EXTRA);
        p.layer2_blend.get_or_insert(LAYER2_BLEND);
    }
    if m.pixel.base >> bit::LAYER3 & 1 == 1 {
        p.layer3_blend.get_or_insert(LAYER3_BLEND);
    }
    Some(m)
}

/// A scene's drawn part: each material it draws once, and the segments
/// drawing with them, their materials renumbered into that list.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Drawn {
    pub materials: Vec<model::Material>,
    pub segments: Vec<Segment>,
    /// Each drawn segment's index in its scene.
    pub sources: Vec<usize>,
    /// The scene's materials and segments not drawn (other effects').
    pub left_out_materials: usize,
    pub left_out_segments: usize,
    /// The sky's segments among the drawn, by drawn index.
    pub sky: Vec<usize>,
}

impl Drawn {
    /// Another scene's drawn part after this one's, its materials numbered
    /// on from this one's.
    pub fn append(&mut self, other: Drawn) {
        let (materials, segments) = (self.materials.len() as u16, self.segments.len());
        self.materials.extend(other.materials);
        self.segments
            .extend(other.segments.into_iter().map(|mut s| {
                s.material += materials;
                s
            }));
        self.sources.extend(other.sources);
        self.sky.extend(other.sky.into_iter().map(|s| s + segments));
        self.left_out_materials += other.left_out_materials;
        self.left_out_segments += other.left_out_segments;
    }
}

/// `graph`'s drawn part, taking its segments.
pub fn drawn(graph: SceneGraph) -> Result<Drawn, LevelError> {
    let mut renumber = Vec::with_capacity(graph.materials.len());
    let mut materials = Vec::new();
    for m in &graph.materials {
        renumber.push(drawn_material(m).map(|m| {
            materials.push(m);
            (materials.len() - 1) as u16
        }));
    }
    let sky: std::collections::HashSet<usize> = graph
        .sky
        .iter()
        .flat_map(|g| g.segments.iter().map(|&s| s as usize))
        .collect();
    let mut out = Drawn {
        left_out_materials: graph.materials.len() - materials.len(),
        materials,
        ..Drawn::default()
    };
    for (i, mut s) in graph.segments.into_iter().enumerate() {
        let index = renumber
            .get(usize::from(s.material))
            .ok_or(LevelError::Segment {
                segment: i,
                material: s.material,
                materials: renumber.len(),
            })?;
        let Some(index) = *index else {
            out.left_out_segments += 1;
            continue;
        };
        s.material = index;
        if sky.contains(&i) {
            out.sky.push(out.segments.len());
        }
        out.segments.push(s);
        out.sources.push(i);
    }
    Ok(out)
}

/// Every key the drawn part draws in `scene`, each once, with the drawn
/// material that first draws it: what the warm compiles before the level
/// shows.
pub fn keys(drawn: &Drawn, scene: &Scene) -> Result<Vec<(WarmKey, usize)>, LevelError> {
    Ok(warm::segment_keys(
        &drawn.materials,
        &drawn.segments,
        scene,
    )?)
}

/// Where AF's fresh state's camera stands: the `default` viewport AG.7's
/// first run read at F0 (`actors-00002.json`), metres in the level's frame.
pub const FRESH_STATE_EYE: [f32; 3] = [6.51073, 52.266262, -107.12627];
/// Its tilt and heading, degrees, as a `tfbEulerTransform` holds them.
pub const FRESH_STATE_TILT: f32 = -23.359688;
pub const FRESH_STATE_HEADING: f32 = 14.99966;
/// The projection's y scale (`P[1][1]`) in AC.13's capture of the same
/// camera (0.6 mm and 0.0003° off this pose): 53.96° from top to bottom,
/// at 16:9.
pub const FRESH_STATE_Y_SCALE: f32 = 1.964_283_2;
/// The capture's near plane, and camera 0's far clip (800 ft).
pub const FRESH_STATE_NEAR: f32 = 0.1;
pub const FRESH_STATE_FAR: f32 = 800.0 * 0.3048;

/// The fresh state's camera: at [`FRESH_STATE_EYE`], looking the way its
/// tilt and heading face, unrolled.
pub fn fresh_state_transform() -> Transform {
    Transform::from_translation(Vec3::from_array(FRESH_STATE_EYE)).looking_to(
        facing([FRESH_STATE_TILT, FRESH_STATE_HEADING, 0.0]),
        Vec3::Y,
    )
}

/// The fresh state's projection; the aspect follows the target.
pub fn fresh_state_projection() -> PerspectiveProjection {
    PerspectiveProjection {
        fov: 2.0 * (1.0 / FRESH_STATE_Y_SCALE).atan(),
        near: FRESH_STATE_NEAR,
        far: FRESH_STATE_FAR,
        ..PerspectiveProjection::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::character::key::{MaterialKey, Pass, StateKey, def};
    use crate::character::material::{CharacterMaterial, ShadowLookup};
    use crate::character::warm::MeshShape;
    use bevy::asset::Handle;
    use bevy::shader::ShaderDefVal;
    use guac_ir::level::{EffectMaterial, PlacedLight, SkyGroup};
    use guac_ir::model::{
        Blend, Filter, Glow, Mask, Params, RenderStates, Sampler, TextureRole, TextureSlot, Wrap,
    };
    use guac_ir::{DumpIdentity, RigLight, Sha256};

    fn close(a: [f32; 3], b: [f32; 3], tolerance: f32) -> bool {
        a.iter().zip(b).all(|(x, y)| (x - y).abs() < tolerance)
    }

    fn identity() -> DumpIdentity {
        DumpIdentity {
            title_id: "BLUS30968".into(),
            app_ver: "01.00".into(),
            manifest_hash: Sha256([4; 32]),
        }
    }

    fn rig_light(name: &str, rotation: [f32; 3]) -> RigLight {
        // Made-up colours, none of the disc's.
        RigLight {
            name: name.into(),
            diffuse: [200, 100, 50, 255],
            ambient: [10, 20, 30, 255],
            specular: [60, 60, 60, 255],
            multiplier: 1.0,
            position: [0.0; 3],
            rotation,
        }
    }

    fn point(name: &str, light_type: u32, visible: bool) -> PlacedLight {
        PlacedLight {
            name: name.into(),
            light_type,
            visible,
            position: [1.0, 2.0, 3.0],
            rotation: [0.0; 3],
            falloff_rate: 0.0,
            cutoff_angle: 0.0,
            attenuation: [0.25, 0.5, 0.75],
            diffuse: [255, 128, 0, 255],
            ambient: [0, 0, 0, 255],
            specular: [0, 0, 0, 255],
            multiplier: 1.0,
        }
    }

    fn lights() -> Lights {
        Lights {
            identity: identity(),
            level: "level_213_tutotial".into(),
            directional: vec![
                rig_light("_worldLight1", [-45.0, 0.0, 0.0]),
                rig_light("_worldLight3", [-45.0, 90.0, 0.0]),
                rig_light("_worldLight2", [45.0, 180.0, 0.0]),
                rig_light("_causticLight", [-45.0, 0.0, 0.0]),
            ],
            placed: vec![
                point("beam", POINT_LIGHT, true),
                point("hidden", POINT_LIGHT, false),
                point("spot", 2, true),
            ],
        }
    }

    #[test]
    fn the_world_lights_travel_the_way_they_face_and_the_caustic_stays_out() {
        let l = level_lights(&lights()).unwrap();
        let s = std::f32::consts::FRAC_1_SQRT_2;
        // In name order, whatever the part's order.
        assert!(close(l.directional[0].travel, [0.0, -s, s], 1e-6));
        assert!(close(l.directional[1].travel, [0.0, s, -s], 1e-6));
        assert!(close(l.directional[2].travel, [s, -s, 0.0], 1e-6));
        // Colours decoded, each light with its own ambient.
        assert_eq!(l.directional[0].diffuse, remaster::linear([200, 100, 50]));
        assert_eq!(l.directional[0].ambient, remaster::linear([10, 20, 30]));
        assert_eq!(l.directional[0].specular, remaster::linear([60, 60, 60]));
        // The point light: its colour decoded, its linear and quadratic
        // attenuation the two terms, the constant dropped.
        assert_eq!(l.points.len(), 1);
        let p = l.points[0];
        assert_eq!(p.position, [1.0, 2.0, 3.0]);
        assert_eq!(p.colour, remaster::linear([255, 128, 0]));
        assert_eq!((p.linear, p.quadratic), (0.5, 0.75));
        let out: Vec<&str> = l.left_out.iter().map(|(n, _)| n.as_str()).collect();
        assert_eq!(out, ["_causticLight", "hidden", "spot"]);

        let mut missing = lights();
        missing.directional.retain(|l| l.name != "_worldLight2");
        assert_eq!(
            level_lights(&missing),
            Err(LevelError::MissingLight("_worldLight2"))
        );
        // A fourth point light has no place in the shader.
        let mut many = lights();
        many.placed = (0..5).map(|i| point(&format!("p{i}"), 1, true)).collect();
        let l = level_lights(&many).unwrap();
        assert_eq!((l.points.len(), l.left_out.len()), (3, 3));
    }

    #[test]
    fn a_camera_sees_the_lights_in_its_view_space_and_the_fog_from_the_level() {
        let l = level_lights(&lights()).unwrap();
        let fog = LevelFog {
            colour: [115, 83, 49],
            near: 30.0,
            far: 300.0,
        };
        // A camera at (0, 0, 10) looking along +Z, as a heading-0 one does:
        // Bevy's view looks down -Z, so +Z turns into -Z and +X into -X.
        let camera = Transform::from_xyz(0.0, 0.0, 10.0).looking_to(Vec3::Z, Vec3::Y);
        let view = camera.to_matrix().inverse();
        let scene = view_scene(&l, &fog, view);
        let s = std::f32::consts::FRAC_1_SQRT_2;
        // The key light, down and away along +Z, goes down and away from
        // the camera, AE.13's turntable reading of the same angles.
        assert!(close(scene.lights[0].travel, [0.0, -s, -s], 1e-6));
        assert!(close(scene.lights[2].travel, [-s, -s, 0.0], 1e-6));
        // The point light 7 m behind the camera's look and 1 m to its left
        // in view, as Bevy's view space has it.
        assert!(close(
            scene.point_lights[0].position,
            [-1.0, 2.0, 7.0],
            1e-5
        ));
        assert_eq!(scene.point_lights[0].linear, 0.5);
        assert!(scene.fog && !scene.shadows);
        assert_eq!(scene.fog_colour, remaster::linear([115, 83, 49]));
        assert_eq!((scene.fog_near, scene.fog_far), (30.0, 300.0));
        // The placeholder starts past the far clip, so it fogs nothing drawn.
        const { assert!(LevelFog::PLACEHOLDER.near > FRESH_STATE_FAR) };
    }

    fn slot(role: TextureRole) -> TextureSlot {
        TextureSlot {
            role,
            texture: "textures/x.ktx2".into(),
            image: "x".into(),
            sampler: Sampler {
                wrap_u: Wrap::Repeat,
                wrap_v: Wrap::Repeat,
                mag: Filter::Linear,
                min: Filter::Linear,
                mip: Some(Filter::Linear),
            },
        }
    }

    /// A level material with these masks and the maps they need; made-up
    /// numbers.
    fn multi2(pixel: Mask, vertex: Mask) -> model::Material {
        let mut textures = vec![slot(TextureRole::Layer1ColorMap)];
        for (b, role) in [
            (bit::LAYER2, TextureRole::Layer2ColorMap),
            (bit::LAYER3, TextureRole::Layer3ColorMap),
            (bit::GLOSS_MAP, TextureRole::Layer3ColorMap),
            (bit::RIM, TextureRole::Layer4ColorMap),
            (bit::NORMAL_MAP, TextureRole::Layer1BaseNormalMap),
            (bit::SPEC_MAP, TextureRole::Layer1BaseSpecMap),
        ] {
            if pixel.base >> b & 1 == 1 {
                textures.push(slot(role));
            }
        }
        for (b, role) in [
            (bit::NORMAL_MAP, TextureRole::Layer2BaseNormalMap),
            (bit::SPEC_MAP, TextureRole::Layer2BaseSpecMap),
        ] {
            if pixel.base >> b & 1 == 1 && pixel.base & 1 == 1 {
                textures.push(slot(role));
            }
        }
        model::Material {
            name: format!("{:#x}", pixel.base),
            pixel,
            vertex,
            textures,
            params: Params {
                fixed_alpha: [-1.0, -1.0, 1.0, 0.5],
                layer1_extra: [1.0, 1.0, 1.0, 0.25],
                layer2_extra: Some([1.0, 1.0, 1.0, 0.25]),
                layer2_blend: Some([1.0, 0.0, 0.0, 0.0]),
                layer3_blend: Some([0.0, 0.0, 1.0, 0.0]),
                layer4_blend: [0.0, 1.0, 0.0, 0.0],
                layer4_values: [0.0, 1.0, 0.0, 0.0],
                material_tint: [128, 128, 128, 255],
                glow: Glow::None,
            },
            states: RenderStates {
                blend: Blend::Opaque,
                depth_write: true,
                two_sided: true,
                alpha_test: None,
            },
            scrolls: Vec::new(),
            tracks: Vec::new(),
        }
    }

    fn mask(base: u32, ignore: u32) -> Mask {
        Mask { base, ignore }
    }

    fn defs(key: &MaterialKey) -> Vec<String> {
        key.shader_defs()
            .into_iter()
            .map(|d| match d {
                ShaderDefVal::Bool(name, true) => name.into_owned(),
                other => panic!("{other:?}"),
            })
            .collect()
    }

    /// AG.19: a level material's key in the level's scene (fog on, the one
    /// point light, no shadows) and the shader defs it becomes. The masks
    /// are level 213's kinds: one leaving shadows, fog and point lights to
    /// the scene (ignore 0x2FF), one owning shadows off (0x3FF), one owning
    /// point lights off too (0x3BFF), and the gloss-mapped one, drawn with
    /// its gloss (AG.22).
    #[test]
    fn level_material_keys_become_their_shader_defs() {
        let l = level_lights(&lights()).unwrap();
        let scene = view_scene(&l, &LevelFog::PLACEHOLDER, Mat4::IDENTITY);
        let key = |m: &model::Material| {
            CharacterMaterial::from_ir(m, &scene, ShadowLookup::NONE, |_| Handle::default())
                .unwrap()
                .key
        };
        let names = |n: &[&str]| -> Vec<String> { n.iter().map(|s| s.to_string()).collect() };

        // Layers 2 and 3, normal and spec maps and vertex colours, leaving
        // fog and point lights to the scene: the eye direction comes with
        // fog, the geometric normal with the point light.
        let both = multi2(mask(0x1B, 0x2FF), mask(0x101B, 0x1AFF));
        let k = key(&both);
        assert_eq!(
            k.shader.pixel(),
            0x1B | 1 << bit::FOG | 1 << bit::POINT_LIGHTS
        );
        assert_eq!(
            defs(&k),
            names(&[
                def::LAYER2,
                def::LAYER3,
                def::NORMAL_MAP,
                def::SPEC_MAP,
                def::FOG,
                def::VERTEX_COLOUR,
                def::EYE,
                def::GEOMETRIC_NORMAL,
                def::UV1,
                def::UV2,
                def::POINT_LIGHTS[0],
            ])
        );
        // Owning shadows off changes nothing here: the level has none yet.
        let owned = multi2(mask(0x1B, 0x3FF), mask(0x101B, 0x1BFF));
        assert_eq!(key(&owned).shader, k.shader);
        // Owning point lights off: fog alone, so no geometric normal.
        let unlit = multi2(mask(0x0, 0x3BFF), mask(0x1000, 0x3BFF));
        let k = key(&unlit);
        assert_eq!(k.shader.pixel(), 1 << bit::FOG);
        assert_eq!(defs(&k), names(&[def::FOG, def::VERTEX_COLOUR]));
        // The gloss map's bit kept on both sides (AG.22): its def, and the
        // second set it reads in a third layer's place.
        let gloss = multi2(mask(0x9C, 0x3FF), mask(0x109C, 0x1BFF));
        let drawn = drawn_material(&LevelMaterial::Multi2(gloss.clone())).unwrap();
        assert_eq!((drawn.pixel.base, drawn.vertex.base), (0x9C, 0x109C));
        let k = key(&drawn);
        assert_eq!(
            defs(&k),
            names(&[
                def::RIM,
                def::NORMAL_MAP,
                def::SPEC_MAP,
                def::GLOSS_MAP,
                def::FOG,
                def::VERTEX_COLOUR,
                def::EYE,
                def::GEOMETRIC_NORMAL,
                def::UV1,
                def::POINT_LIGHTS[0],
            ])
        );
        // Its gloss strength is the third layer's fixed alpha; a gloss map
        // beside a third layer is refused.
        let built =
            CharacterMaterial::from_ir(&gloss, &scene, ShadowLookup::NONE, |_| Handle::default())
                .unwrap();
        assert_eq!(built.uniform.fixed_alpha.z, 1.0);
        let both = multi2(mask(0x9E, 0x3FF), mask(0x109E, 0x1BFF));
        let both = drawn_material(&LevelMaterial::Multi2(both)).unwrap();
        assert!(
            CharacterMaterial::from_ir(&both, &scene, ShadowLookup::NONE, |_| {
                Handle::default()
            })
            .is_err()
        );
        // The render states key the pipeline as a character's do.
        assert_eq!(
            k.states,
            StateKey {
                two_sided: true,
                ..StateKey::OPAQUE
            }
        );
        assert_eq!(k.pass, Pass::Colour);
        assert_eq!(
            drawn_material(&effect()),
            None,
            "another effect draws nothing"
        );
    }

    /// A level material that sets no values for its second or third layer
    /// draws them at the original's defaults (second over, exponent 1,
    /// detail 0.25; third multiplied); one that sets its own keeps them, and
    /// a layer it doesn't have takes none.
    #[test]
    fn a_layer_with_no_values_takes_the_defaults() {
        let mut bare = multi2(mask(0x1B, 0x3FF), mask(0x101B, 0x1BFF));
        (
            bare.params.layer2_extra,
            bare.params.layer2_blend,
            bare.params.layer3_blend,
        ) = (None, None, None);
        let drawn = drawn_material(&LevelMaterial::Multi2(bare.clone())).unwrap();
        assert_eq!(drawn.params.layer2_extra, Some(LAYER2_EXTRA));
        assert_eq!(drawn.params.layer2_blend, Some(LAYER2_BLEND));
        assert_eq!(drawn.params.layer3_blend, Some(LAYER3_BLEND));
        let r = crate::character::reference_material(&drawn).unwrap();
        let second = r.second.unwrap();
        assert_eq!(
            (second.layer.exponent, second.layer.detail_weight),
            (1.0, 0.25)
        );
        assert_eq!(
            (
                second.weights.over,
                second.weights.add,
                second.weights.multiply
            ),
            (1.0, 0.0, 0.0)
        );
        let third = r.third.unwrap();
        assert_eq!(
            (
                third.weights.over,
                third.weights.add,
                third.weights.multiply
            ),
            (0.0, 0.0, 1.0)
        );
        // Kept as the character path keeps it: refused without them.
        assert!(crate::character::reference_material(&bare).is_err());

        let mut own = bare.clone();
        own.params.layer2_blend = Some([0.0, 0.0, 1.0, 0.0]);
        own.params.layer2_extra = Some([1.0, 1.0, 6.5, 0.25]);
        let drawn = drawn_material(&LevelMaterial::Multi2(own)).unwrap();
        assert_eq!(drawn.params.layer2_blend, Some([0.0, 0.0, 1.0, 0.0]));
        assert_eq!(drawn.params.layer2_extra, Some([1.0, 1.0, 6.5, 0.25]));

        let mut single = bare;
        single.pixel.base = 0x18;
        single.vertex.base = 0x1018;
        let drawn = drawn_material(&LevelMaterial::Multi2(single)).unwrap();
        assert_eq!(
            (
                drawn.params.layer2_extra,
                drawn.params.layer2_blend,
                drawn.params.layer3_blend
            ),
            (None, None, None)
        );
    }

    fn segment(material: u16, colours: bool) -> Segment {
        let mut s = MeshShape {
            uv_sets: 1,
            tangents: false,
            colours,
            skinned: false,
        }
        .triangle(material);
        s.positions = vec![[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 0.0, 1.0]];
        s
    }

    fn effect() -> LevelMaterial {
        LevelMaterial::Effect(EffectMaterial {
            name: "grass_HIRES".into(),
            effect: 0xAA6,
            pixel: None,
            vertex: None,
            textures: Vec::new(),
            states: RenderStates {
                blend: Blend::Opaque,
                depth_write: true,
                two_sided: true,
                alpha_test: Some(0.5),
            },
            scrolls: Vec::new(),
            tracks: Vec::new(),
            unread: Vec::new(),
        })
    }

    /// A scene's drawn part: tfbmulti2's segments with their materials
    /// renumbered, the effects' left out and counted, the sky kept; its
    /// keys one per material key and mesh shape; two scenes in one list.
    #[test]
    fn a_scene_draws_its_tfbmulti2_segments_and_keys_them_once() {
        let ground = multi2(mask(0x1A, 0x3FF), mask(0x101A, 0x1BFF));
        let sky = multi2(mask(0x0, 0x3BFF), mask(0x1000, 0x3BFF));
        let graph = SceneGraph {
            world: "Levels/Test/Test_ART_MASTER.ma".into(),
            materials: vec![
                effect(),
                LevelMaterial::Multi2(ground.clone()),
                LevelMaterial::Multi2(sky),
            ],
            segments: vec![
                segment(1, false),
                segment(0, false),
                segment(2, true),
                segment(1, false),
                segment(1, true),
            ],
            sky: vec![SkyGroup {
                name: "sky".into(),
                segments: vec![2],
            }],
        };
        let d = drawn(graph.clone()).unwrap();
        assert_eq!(d.materials.len(), 2);
        assert_eq!((d.left_out_materials, d.left_out_segments), (1, 1));
        assert_eq!(d.sources, [0, 2, 3, 4]);
        assert_eq!(
            d.segments.iter().map(|s| s.material).collect::<Vec<_>>(),
            [0, 1, 0, 0]
        );
        assert_eq!(d.sky, [1]);

        let l = level_lights(&lights()).unwrap();
        let scene = view_scene(&l, &LevelFog::PLACEHOLDER, Mat4::IDENTITY);
        let k = keys(&d, &scene).unwrap();
        // The ground on two shapes, the sky on one: three keys, in segment
        // order, each with its drawn material.
        assert_eq!(
            k.iter()
                .map(|(k, m)| (k.mesh.colours, *m))
                .collect::<Vec<_>>(),
            [(false, 0), (true, 1), (true, 0)]
        );
        assert!(k.iter().all(|(k, _)| !k.mesh.skinned));

        // A second scene numbers its materials on from the first's.
        let mut both = d.clone();
        both.append(
            drawn(SceneGraph {
                world: "Levels/Test/Test_GRASS.ma".into(),
                materials: vec![LevelMaterial::Multi2(ground), effect()],
                segments: vec![segment(1, false), segment(0, true)],
                sky: Vec::new(),
            })
            .unwrap(),
        );
        assert_eq!(both.materials.len(), 3);
        assert_eq!(
            both.segments.iter().map(|s| s.material).collect::<Vec<_>>(),
            [0, 1, 0, 0, 2]
        );
        assert_eq!(both.sources, [0, 2, 3, 4, 1]);
        assert_eq!((both.left_out_materials, both.left_out_segments), (2, 2));
        assert_eq!(both.sky, [1]);

        let mut bad = graph;
        bad.segments.push(segment(3, false));
        assert_eq!(
            drawn(bad),
            Err(LevelError::Segment {
                segment: 5,
                material: 3,
                materials: 3
            })
        );
    }

    /// The fresh state's camera looks where AC.13's capture of it does:
    /// forward (0.23760, -0.39650, 0.88675) and up (0.10262, 0.91803,
    /// 0.38299), 53.96° from top to bottom.
    #[test]
    fn the_fresh_state_camera_looks_where_the_capture_does() {
        let t = fresh_state_transform();
        assert!(close(
            t.forward().to_array(),
            [0.23760, -0.39650, 0.88675],
            1e-4
        ));
        assert!(close(t.up().to_array(), [0.10262, 0.91803, 0.38299], 1e-4));
        assert_eq!(t.translation.to_array(), FRESH_STATE_EYE);
        let p = fresh_state_projection();
        assert!((p.fov.to_degrees() - 53.96).abs() < 0.005, "{}", p.fov);
        assert!((p.far - 243.84).abs() < 1e-4);
    }
}
