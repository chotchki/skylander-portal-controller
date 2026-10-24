//! The level on the TV (openspec AG.19, design decision 9): a level's scene
//! from the IR on AE's material in the remaster, lit by its own lights. The
//! arithmetic the TV's loader (the binary's `level.rs`), the warm and the
//! tests share:
//!
//! - What's drawn ([`drawn`]): every tfbmulti2 segment, the scene's sky
//!   among them, on its own material, the one with a gloss map (AG.22)
//!   included. The other effects' (the grass's `0xAA6`, `0xAA8`, the warp)
//!   draw nothing yet; AG.24 is to draw the grass and the warp without
//!   their wind and distortion (stand-in ledger: `grass-warp`).
//! - The light ([`level_lights`], [`view_scene`]): the atmospheric zone the
//!   level's definition sets up at its start (AG.41: AG.13 found the game
//!   lights and fogs the opening area by it, not by the level file's
//!   `_worldLight1`-`3`), each light travelling the way it faces, as AE.13
//!   reads the sprite rig, and the placed point lights; fog from a
//!   [`LevelFog`] the TV holds, set from the same zone ([`LevelFog::from_zone`]),
//!   which the sky dome takes its zone's share of ([`sky_fogged`]); no shadows
//!   (AG.23). The shader lights in view space, so the scene is the lights
//!   seen from a camera ([`view_scene`]); the TV hands it the world's
//!   ([`world_scene`]) and the shader turns them by its view (AG.21).
//! - The camera AF's fresh state shows ([`fresh_state_transform`],
//!   [`fresh_state_projection`]), which the dev entry draws the level at.
//! - The walk (AG.21): the level's collision as static seam bodies
//!   ([`solids`]), where the hero starts ([`start_point`]) and the scene he
//!   draws in ([`hero_scene`]).

use std::sync::Arc;

use bevy::camera::PerspectiveProjection;
use bevy::math::{Mat4, Vec3};
use bevy::prelude::{GlobalTransform, Transform};
use guac_ir::level::{
    Atmosphere, AtmosphericZone, Collision, LevelMaterial, Lights, Placements, SceneGraph, ZoneFog,
};
use guac_ir::model::{self, Segment};
use guac_ir::shading::{DirectionalLight, PointLight, Scene};
use guac_motion::seam::{Body, BodyId, BodyKind, Collider, Mesh, MeshError, Pose};

use crate::character::key::bit;
use crate::character::remaster;
use crate::character::warm::{self, WarmError, WarmKey};

/// The directional lights tfbmulti2 reads, by name: the start's zone sets
/// their colours and angles but for lights 2 and 3's ambients, which it
/// leaves as the level file has them. The rig's fourth, `_causticLight`, is
/// the viewport's caustic effect's, off at the start (AG.13), so the level
/// draws without it.
pub const WORLD_LIGHTS: [&str; 3] = ["_worldLight1", "_worldLight2", "_worldLight3"];
/// A placed light's `_type` for a point light (level 213's one).
pub const POINT_LIGHT: u32 = 1;
/// The most point lights tfbmulti2 takes (bits 11-13).
pub const MAX_POINT_LIGHTS: usize = 3;

/// A foot, in metres: the scripts give distances in feet.
pub const FOOT: f32 = 0.3048;

/// The level's fog: its colour as a disc stores one (encoded for display,
/// decoded like every colour the remaster takes) and where it starts and
/// ends along the view, in metres. The TV holds one as a resource, every
/// level material's fog term reads it, and a change relights them all; it
/// starts at [`LevelFog::NONE`] and a level's read sets it to its start's
/// zone's ([`LevelFog::from_zone`]).
#[derive(bevy::prelude::Resource, Debug, Clone, Copy, PartialEq)]
pub struct LevelFog {
    pub colour: [u8; 3],
    pub near: f32,
    pub far: f32,
}

impl Default for LevelFog {
    fn default() -> Self {
        Self::NONE
    }
}

impl LevelFog {
    /// Fog that adds nothing: black, starting past anything the level
    /// draws (its sky reaches about 750 m), so a material keyed for fog
    /// draws as without. Before a level is read, and for a zone whose fog is
    /// off.
    pub const NONE: Self = Self {
        colour: [0, 0, 0],
        near: 1.0e4,
        far: 2.0e4,
    };

    /// A zone's fog as the original sets it up (AG.13, at AF's fresh
    /// state): on, the fog fades in linearly with depth along the view from
    /// its start to its start plus its range, wholly fogged past that (the
    /// viewport's fog max is the zone's "fog on", 1, which the shader's
    /// fade reaches); off (0), nothing. Any other "fog on" is refused.
    pub fn from_zone(fog: &ZoneFog) -> Result<Self, LevelError> {
        match fog.on {
            0 => Ok(Self::NONE),
            1 => Ok(Self {
                colour: rgb(fog.colour),
                near: fog.start as f32 * FOOT,
                far: (fog.start + fog.range) as f32 * FOOT,
            }),
            on => Err(LevelError::Zone("fog on", on)),
        }
    }
}

fn rgb(c: [u8; 4]) -> [u8; 3] {
    [c[0], c[1], c[2]]
}

/// The zone the level's definition sets up at its start.
pub fn start_zone(atmosphere: &Atmosphere) -> Result<&AtmosphericZone, LevelError> {
    atmosphere.start_zone().ok_or(LevelError::NoStartZone)
}

/// Whether the sky dome takes the start's fog: its share is the zone's
/// "fog skydome percent". At 0, level 213's, the original draws its sky
/// with the shader's fog switched off (AG.13's capture: its 14 sky draws,
/// out to 657 m, are the only unfogged tfbmulti2 draws), so the sky's
/// materials own their fog off ([`drawn`]). Another share isn't drawn yet
/// and is refused.
pub fn sky_fogged(zone: &AtmosphericZone) -> Result<bool, LevelError> {
    match zone.fog.sky_dome_percent {
        0 => Ok(false),
        percent => Err(LevelError::Zone("fog skydome percent", percent)),
    }
}

#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum LevelError {
    #[error("the level has no {0}")]
    MissingLight(&'static str),
    #[error("the level sets up no atmospheric zone at its start")]
    NoStartZone,
    #[error("the start's zone has {0} {1}, which isn't drawn")]
    Zone(&'static str, i32),
    #[error("segment {segment} names material {material}, past the scene's {materials}")]
    Segment {
        segment: usize,
        material: u16,
        materials: usize,
    },
    #[error(transparent)]
    Keys(#[from] WarmError),
    #[error("the level has no collision mesh")]
    NoCollision,
    #[error("the {world} collision mesh: {error}")]
    Collision { world: String, error: MeshError },
    #[error("the level has no {START_PLACEMENT} with a waypoint")]
    NoStart,
}

/// The placement whose first waypoint is where the first player stands at
/// the chapter's start (ag-motion section 4; AF.9: Eruptor idles on it,
/// settled onto the ground, facing 0°).
pub const START_PLACEMENT: &str = "213_Init_Location";

/// The way the hero faces at the start, degrees (AF.9: heading 0).
pub const START_FACING: f32 = 0.0;

/// Where the hero starts: [`START_PLACEMENT`]'s first waypoint, before he's
/// settled onto the ground.
pub fn start_point(placements: &Placements) -> Result<Vec3, LevelError> {
    placements
        .actors
        .iter()
        .find(|a| a.name == START_PLACEMENT)
        .and_then(|a| {
            placements
                .waypoints
                .get(usize::try_from(a.waypoints?).ok()?)
        })
        .and_then(|path| path.first())
        .map(|w| Vec3::from_array(w.position))
        .filter(|p| p.is_finite())
        .ok_or(LevelError::NoStart)
}

/// The first body id the level's collision meshes take, one each in the
/// part's order; placements' solids (AG.25) number past them.
pub const LEVEL_BODIES: u32 = 1;

/// The level's static solids: each world's collision mesh as a static seam
/// body in the level's frame (its vertices already there, AG.15), every
/// triangle's material kept.
pub fn solids(collision: &Collision) -> Result<Vec<Body>, LevelError> {
    if collision.meshes.is_empty() {
        return Err(LevelError::NoCollision);
    }
    collision
        .meshes
        .iter()
        .zip(LEVEL_BODIES..)
        .map(|(m, id)| {
            let mesh = Mesh::new(
                m.vertices.iter().map(|v| Vec3::from_array(*v)).collect(),
                m.triangles.iter().map(|t| t.map(u32::from)).collect(),
                m.materials.clone(),
            )
            .map_err(|error| LevelError::Collision {
                world: m.world.clone(),
                error,
            })?;
            Ok(Body {
                id: BodyId(id),
                kind: BodyKind::Static,
                pose: Pose::IDENTITY,
                collider: Collider::Mesh(Arc::new(mesh)),
            })
        })
        .collect()
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
    /// `_worldLight1`-`3` as the start's zone sets them: each one's colours
    /// decoded as the remaster decodes them, its travel the way it faces, in
    /// the level's frame.
    pub directional: [DirectionalLight; 3],
    /// Visible point lights, at their positions in the level's frame.
    pub points: Vec<PointLight>,
    /// The lights not drawn, each with why.
    pub left_out: Vec<(String, &'static str)>,
}

/// The level's lights: the three directional lights as `zone` sets them up
/// ("Game - Setup Atmospherics": light 1's ambient, each light's diffuse,
/// specular, facing and tilt), lights 2 and 3's ambients as the lights part
/// has them, and the placed point lights off the lights part. Each light's
/// multiplier is 1 on the disc and what another would do is unread, so
/// none is applied (as on the turntable). A point light's falloff takes the
/// light's linear and quadratic attenuation as the shader's two terms and
/// drops its constant, which the shader's falloff has no place for: a
/// reading, not a measurement, since how the runtime fills those terms is
/// ae-shader's open item (level 213's one sits at the origin with
/// (0, 0.5, 0), about 103 m from the start, where its light is under 1 % of
/// its colour).
pub fn level_lights(lights: &Lights, zone: &AtmosphericZone) -> Result<LevelLights, LevelError> {
    let mut directional = [DirectionalLight::OFF; 3];
    for (k, (slot, name)) in directional.iter_mut().zip(WORLD_LIGHTS).enumerate() {
        let file = lights
            .directional
            .iter()
            .find(|l| l.name == name)
            .ok_or(LevelError::MissingLight(name))?;
        let set = &zone.lights[k];
        let ambient = if k == 0 { zone.ambient } else { file.ambient };
        *slot = remaster::light(
            facing([set.tilt as f32, set.facing as f32, 0.0]).to_array(),
            rgb(ambient),
            rgb(set.diffuse),
            rgb(set.specular),
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

/// A point light that adds nothing: a slot no light takes.
pub const DARK: PointLight = PointLight {
    position: [0.0; 3],
    colour: [0.0; 3],
    linear: 0.0,
    quadratic: 0.0,
};

/// The scene the level's materials draw in, seen from a camera whose view
/// is `view_from_world`: the directional lights' travel and the point
/// lights' positions in its view space (the space the shader lights in),
/// fog from `fog`, no shadows until AG.23, neutral tints and no replace
/// colour. Its point lights fill all three of tfbmulti2's slots, the placed
/// ones first and the rest [`DARK`] (openspec AH.65): the scripts light
/// more as attacks go off, each frame's three are the level's buffer's
/// ([`frame_point_lights`]), so a part's key never changes with how many
/// are lit and nothing compiles mid-fight.
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
        .chain(std::iter::repeat(DARK))
        .take(MAX_POINT_LIGHTS)
        .collect();
    scene.fog = true;
    scene.fog_colour = remaster::linear(fog.colour);
    (scene.fog_near, scene.fog_far) = (fog.near, fog.far);
    scene
}

/// The scene the hero draws in, seen from a camera whose view is
/// `view_from_world`: the level's as [`view_scene`] gives it, its three
/// point lights too (openspec AH.65: AH.7's and AH.9's frames show his own
/// ball and eruption lighting him, and of his materials only those that
/// own their point lights off leave them to the card; AG.21 drew him
/// without, on his card's keys), so his keys in the level are the census's
/// [`crate::census::HEROES`], not a card's. No shadows until AG.23.
pub fn hero_scene(lights: &LevelLights, fog: &LevelFog, view_from_world: Mat4) -> Scene {
    view_scene(lights, fog, view_from_world)
}

/// A light the scripts lit, as the level draws it (openspec AH.65): at
/// `at` (the level's frame, metres), its diffuse decoded as the remaster
/// decodes every colour and scaled by its multiplier, its linear and
/// quadratic attenuation the falloff's two terms, its constant dropped as a
/// placed one's is ([`level_lights`]).
pub fn script_light(at: Vec3, l: &crate::script_engine::Light) -> PointLight {
    let [r, g, b, _] = l.diffuse.to_be_bytes();
    PointLight {
        position: at.to_array(),
        colour: remaster::linear([r, g, b]).map(|c| c * l.multiplier),
        linear: l.attenuation[1],
        quadratic: l.attenuation[2],
    }
}

/// The three point lights a frame draws the level with (openspec AH.65):
/// of the level's placed ones and those the scripts lit, the three nearest
/// `focus` (the hero, else the level's start) that add anything, the rest
/// [`DARK`]. Which three the original's draws take when more are lit is
/// unread; its point-light manager lights three at most, so past those
/// only a placed light (level 213's one, about 103 m off) competes.
pub fn frame_point_lights(
    placed: &[PointLight],
    lit: &[PointLight],
    focus: Vec3,
) -> [PointLight; 3] {
    let mut all: Vec<(f32, PointLight)> = placed
        .iter()
        .chain(lit)
        .filter(|p| p.colour.iter().any(|c| *c > 0.0) && (p.linear > 0.0 || p.quadratic > 0.0))
        .map(|p| (Vec3::from_array(p.position).distance(focus), *p))
        .collect();
    all.sort_by(|a, b| a.0.total_cmp(&b.0));
    let mut out = [DARK; MAX_POINT_LIGHTS];
    for (slot, (_, p)) in out.iter_mut().zip(all) {
        *slot = p;
    }
    out
}

/// The scene the level's materials are lit in, in the world's frame: the
/// directional lights' travels and the point lights' positions as the level
/// has them, which the shader turns by the view it draws for (a uniform lit
/// in the world, `CharacterUniform::lit_in_world`, AG.21).
pub fn world_scene(lights: &LevelLights, fog: &LevelFog) -> Scene {
    view_scene(lights, fog, Mat4::IDENTITY)
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
    /// The sky's segments among the drawn, by drawn index, in the order the
    /// scene lists them (the order the original draws its sky in, AG.79).
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

/// `m` owning its fog off: its masks hold the fog bit, unset, so it draws
/// unfogged in any scene.
pub fn unfogged(mut m: model::Material) -> model::Material {
    for mask in [&mut m.pixel, &mut m.vertex] {
        mask.ignore |= 1 << bit::FOG;
        mask.base &= !(1 << bit::FOG);
    }
    m
}

/// `m` as the sky draws it: writing no depth, whatever its states say, and
/// owning its fog off unless the sky takes the fog (`fogged`). AG.13's
/// capture of the fresh state has the original draw its 14 sky segments
/// after the scenery that hides them and before the rest of what blends,
/// each depth-tested and none written, its opaque dome first and then the
/// others in the scene's order (AG.79: written, a cloud plane drawn before
/// the sky's cap cut the cap away in its own outline, the hard-edged slab
/// across the sky chotchki saw).
pub fn sky_material(m: model::Material, fogged: bool) -> model::Material {
    let mut m = if fogged { m } else { unfogged(m) };
    m.states.depth_write = false;
    m
}

/// The sort bias that draws the sky's blended parts before anything else
/// that blends and in the scene's order, `rank` being the place of the
/// first sky segment drawing with the material among the sky's: a sorted
/// part's view depth (metres, increasing toward the camera, back to front)
/// plus its bias orders the blended pass, and the level's sky reaches 750 m,
/// so a step of 10 km keeps the ranks apart wherever the camera stands.
pub fn sky_bias(rank: usize) -> f32 {
    SKY_BIAS + SKY_BIAS_STEP * rank as f32
}

/// The first sky rank's bias, and the step between ranks.
pub const SKY_BIAS: f32 = -1.0e7;
pub const SKY_BIAS_STEP: f32 = 1.0e4;

/// Each material's sort bias in `drawn`: [`sky_bias`] of its rank for one
/// the sky draws with, 0 for the rest (sorted by depth alone).
pub fn depth_biases(drawn: &Drawn) -> Vec<f32> {
    let mut out = vec![0.0; drawn.materials.len()];
    let mut ranked = vec![false; drawn.materials.len()];
    let mut rank = 0;
    for &i in &drawn.sky {
        let k = usize::from(drawn.segments[i].material);
        if !ranked[k] {
            ranked[k] = true;
            out[k] = sky_bias(rank);
            rank += 1;
        }
    }
    out
}

/// `graph`'s drawn part, taking its segments. The sky's segments draw on
/// copies of their materials that write no depth and, unless the sky dome
/// takes the fog (`sky_fogged`, [`sky_fogged`]), own their fog off
/// ([`sky_material`]): one a material the sky draws with, beside the
/// material itself where the rest of the scene draws with it too.
pub fn drawn(graph: SceneGraph, sky_fogged: bool) -> Result<Drawn, LevelError> {
    // The sky's segments in the order the scene lists them, each once.
    let mut listed: Vec<usize> = Vec::new();
    for s in graph.sky.iter().flat_map(|g| &g.segments) {
        if !listed.contains(&(*s as usize)) {
            listed.push(*s as usize);
        }
    }
    let sky: std::collections::HashSet<usize> = listed.iter().copied().collect();
    let n = graph.materials.len();
    // Which kinds of segment draw with each material: the sky's on a copy
    // of its own ([`sky_material`]).
    let (mut by_sky, mut by_rest) = (vec![false; n], vec![false; n]);
    for (i, s) in graph.segments.iter().enumerate() {
        let k = usize::from(s.material);
        if k >= n {
            return Err(LevelError::Segment {
                segment: i,
                material: s.material,
                materials: n,
            });
        }
        if sky.contains(&i) {
            by_sky[k] = true;
        } else {
            by_rest[k] = true;
        }
    }
    let mut materials = Vec::new();
    let (mut rest, mut sky_copy) = (vec![None; n], vec![None; n]);
    let mut multi2 = 0;
    for (k, m) in graph.materials.iter().enumerate() {
        let Some(d) = drawn_material(m) else {
            continue;
        };
        multi2 += 1;
        if by_rest[k] || !by_sky[k] {
            materials.push(d.clone());
            rest[k] = Some((materials.len() - 1) as u16);
        }
        if by_sky[k] {
            materials.push(sky_material(d, sky_fogged));
            sky_copy[k] = Some((materials.len() - 1) as u16);
        }
    }
    let mut out = Drawn {
        left_out_materials: n - multi2,
        materials,
        ..Drawn::default()
    };
    for (i, mut s) in graph.segments.into_iter().enumerate() {
        let is_sky = sky.contains(&i);
        let index = if is_sky {
            sky_copy[usize::from(s.material)]
        } else {
            rest[usize::from(s.material)]
        };
        let Some(index) = index else {
            out.left_out_segments += 1;
            continue;
        };
        s.material = index;
        out.segments.push(s);
        out.sources.push(i);
    }
    out.sky = listed
        .iter()
        .filter_map(|&i| out.sources.iter().position(|&s| s == i))
        .collect();
    Ok(out)
}

/// Every key the drawn part draws in `scene`, each once, with the drawn
/// material that first draws it, every glowing part's level glow mask after
/// (openspec AH.65, [`warm::level_segment_keys`]): what the warm compiles
/// before the level shows.
pub fn keys(drawn: &Drawn, scene: &Scene) -> Result<Vec<(WarmKey, usize)>, LevelError> {
    Ok(warm::level_segment_keys(
        &drawn.materials,
        &drawn.segments,
        scene,
    )?)
}

/// A level as it draws at its start: its drawn part, the world lights and
/// fog its start's zone sets up, and the keys it draws at the fresh state's
/// camera ([`at_start`]).
#[derive(Debug, Clone)]
pub struct AtStart {
    pub drawn: Drawn,
    pub lights: LevelLights,
    pub fog: LevelFog,
    pub keys: Vec<(WarmKey, usize)>,
}

/// `scene`'s drawn part (its scenes' in one list, materials numbered across
/// them), lit and fogged by `atmosphere`'s start zone over `lights`' placed
/// lights, the sky taking the zone's share of the fog, and the keys it
/// draws seen from [`fresh_state_view`]. The TV's loader reads a level
/// through this, and `tests/giants_census.rs` holds the census's level
/// table to it, so the start's pass compiles the loader's own keys.
pub fn at_start(
    scene: guac_ir::level::Scene,
    lights: &Lights,
    atmosphere: &Atmosphere,
) -> Result<AtStart, LevelError> {
    let zone = start_zone(atmosphere)?;
    let lights = level_lights(lights, zone)?;
    let fog = LevelFog::from_zone(&zone.fog)?;
    let sky = sky_fogged(zone)?;
    let mut all = Drawn::default();
    for graph in scene.scenes {
        all.append(drawn(graph, sky)?);
    }
    let keys = keys(&all, &view_scene(&lights, &fog, fresh_state_view()))?;
    Ok(AtStart {
        drawn: all,
        lights,
        fog,
        keys,
    })
}

/// The fresh state's camera as a view (`view_from_world`), through the
/// `GlobalTransform` Bevy propagates for it, so a camera placed there
/// matches what the materials were lit for to the bit and its first frame
/// relights nothing.
pub fn fresh_state_view() -> Mat4 {
    GlobalTransform::from(fresh_state_transform())
        .to_matrix()
        .inverse()
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

    fn rig_light(name: &str, ambient: [u8; 4]) -> RigLight {
        // Made-up colours, none of the disc's, and an angle the zone's
        // replace.
        RigLight {
            name: name.into(),
            diffuse: [7, 7, 7, 255],
            ambient,
            specular: [9, 9, 9, 255],
            multiplier: 1.0,
            position: [0.0; 3],
            rotation: [30.0, 30.0, 0.0],
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
                rig_light("_worldLight1", [99, 99, 99, 255]),
                rig_light("_worldLight3", [4, 5, 6, 255]),
                rig_light("_worldLight2", [1, 2, 3, 255]),
                rig_light("_causticLight", [99, 99, 99, 255]),
            ],
            placed: vec![
                point("beam", POINT_LIGHT, true),
                point("hidden", POINT_LIGHT, false),
                point("spot", 2, true),
            ],
        }
    }

    /// A zone with made-up colours, its lights at the angles the tests
    /// below turn: down and away along +Z, up and back, down along +X.
    fn zone() -> AtmosphericZone {
        let light = |n: u8, facing, tilt| guac_ir::level::ZoneLight {
            diffuse: [200, 100, n, 0],
            specular: [60, 60, n, 0],
            facing,
            tilt,
        };
        AtmosphericZone {
            id: 0,
            ambient: [10, 20, 30, 0],
            lights: [light(50, 0, -45), light(51, 180, 45), light(52, 90, -45)],
            fog: ZoneFog {
                on: 1,
                start: 100,
                range: 1000,
                colour: [115, 83, 49, 0],
                sky_dome_percent: 0,
                sky_dome_colour: [128, 128, 128, 0],
                yon_colour: [115, 83, 49, 0],
            },
        }
    }

    /// AG.41: the start's zone lights the world lights: each travels the
    /// way the zone faces it, with the zone's colours, light 1 with the
    /// zone's ambient and 2 and 3 with the level file's; the caustic light
    /// stays out.
    #[test]
    fn the_start_zone_lights_the_world_lights_and_the_caustic_stays_out() {
        let l = level_lights(&lights(), &zone()).unwrap();
        let s = std::f32::consts::FRAC_1_SQRT_2;
        // In name order, whatever the part's order.
        assert!(close(l.directional[0].travel, [0.0, -s, s], 1e-6));
        assert!(close(l.directional[1].travel, [0.0, s, -s], 1e-6));
        assert!(close(l.directional[2].travel, [s, -s, 0.0], 1e-6));
        // Colours decoded: the zone's, light 1 with its ambient.
        for (k, n) in [50, 51, 52].into_iter().enumerate() {
            assert_eq!(l.directional[k].diffuse, remaster::linear([200, 100, n]));
            assert_eq!(l.directional[k].specular, remaster::linear([60, 60, n]));
        }
        assert_eq!(l.directional[0].ambient, remaster::linear([10, 20, 30]));
        assert_eq!(l.directional[1].ambient, remaster::linear([1, 2, 3]));
        assert_eq!(l.directional[2].ambient, remaster::linear([4, 5, 6]));
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
            level_lights(&missing, &zone()),
            Err(LevelError::MissingLight("_worldLight2"))
        );
        // A fourth point light has no place in the shader.
        let mut many = lights();
        many.placed = (0..5).map(|i| point(&format!("p{i}"), 1, true)).collect();
        let l = level_lights(&many, &zone()).unwrap();
        assert_eq!((l.points.len(), l.left_out.len()), (3, 3));
    }

    /// AG.41: the zone's fog, as AG.13 read the opening area's at AF's fresh
    /// state: (115, 83, 49) from 100 ft to 100 + 1,000 ft, 30.48 m to
    /// 335.28 m; off, it fogs nothing; the sky dome's 0 % keeps the sky
    /// unfogged. What isn't drawn is refused, and a level whose start sets
    /// up no zone says so.
    #[test]
    fn the_start_zones_fog_is_in_metres_and_the_sky_takes_none() {
        let z = zone();
        let fog = LevelFog::from_zone(&z.fog).unwrap();
        assert_eq!(fog.colour, [115, 83, 49]);
        assert!((fog.near - 30.48).abs() < 1e-4, "{fog:?}");
        assert!((fog.far - 335.28).abs() < 1e-4, "{fog:?}");
        let mut off = z.fog;
        off.on = 0;
        assert_eq!(LevelFog::from_zone(&off), Ok(LevelFog::NONE));
        off.on = 2;
        assert_eq!(
            LevelFog::from_zone(&off),
            Err(LevelError::Zone("fog on", 2))
        );
        assert_eq!(sky_fogged(&z), Ok(false));
        let mut full = z;
        full.fog.sky_dome_percent = 100;
        assert_eq!(
            sky_fogged(&full),
            Err(LevelError::Zone("fog skydome percent", 100))
        );
        let mut atmosphere = Atmosphere {
            identity: identity(),
            level: "level_213_tutotial".into(),
            script: "Level_Definition_900.ai".into(),
            zones: vec![z],
            start: Some(0),
        };
        assert_eq!(start_zone(&atmosphere), Ok(&z));
        atmosphere.start = None;
        assert_eq!(start_zone(&atmosphere), Err(LevelError::NoStartZone));
    }

    #[test]
    fn a_camera_sees_the_lights_in_its_view_space_and_the_fog_from_the_level() {
        let l = level_lights(&lights(), &zone()).unwrap();
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
        // No fog starts past the far clip, so it fogs nothing drawn.
        const { assert!(LevelFog::NONE.near > FRESH_STATE_FAR) };
    }

    /// AH.65 (AG.21 drew him without its point lights): the hero draws in
    /// the level's lights, fog and three point lights' slots as the camera
    /// sees them, the placed one first and two dark, so a material leaving
    /// point lights and fog to the scene keys as the level's does, not as a
    /// card draws it (fog on, nothing else).
    #[test]
    fn the_hero_draws_in_the_levels_light_fog_and_point_lights() {
        let l = level_lights(&lights(), &zone()).unwrap();
        let fog = LevelFog {
            colour: [115, 83, 49],
            near: 30.48,
            far: 335.28,
        };
        let view = Transform::from_xyz(3.0, 2.0, 10.0)
            .looking_to(Vec3::new(1.0, -0.5, 2.0), Vec3::Y)
            .to_matrix()
            .inverse();
        let level = view_scene(&l, &fog, view);
        let hero = hero_scene(&l, &fog, view);
        assert_eq!(hero.lights, level.lights, "the level's lights, in view");
        assert_eq!(level.point_lights.len(), MAX_POINT_LIGHTS);
        assert_eq!(hero.point_lights, level.point_lights);
        assert_ne!(level.point_lights[0], DARK, "the placed one first");
        assert_eq!(level.point_lights[1..], [DARK, DARK]);
        assert!(hero.fog && !hero.shadows);
        assert_eq!(
            (hero.fog_colour, hero.fog_near, hero.fog_far),
            (level.fog_colour, 30.48, 335.28)
        );
        let key = |m: &model::Material, scene: &Scene| {
            CharacterMaterial::from_ir(m, scene, ShadowLookup::NONE, |_| Handle::default())
                .unwrap()
                .key
        };
        let card = Scene {
            fog: true,
            ..Scene::new()
        };
        let open = multi2(mask(0x1B, 0x2FF), mask(0x101B, 0x1AFF));
        assert_eq!(key(&open, &hero), key(&open, &level));
        assert_ne!(key(&open, &hero), key(&open, &card));
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

    /// AG.19: a level material's key in the level's scene (fog on, its
    /// three point lights' slots (AH.65), no shadows) and the shader defs it
    /// becomes. The masks
    /// are level 213's kinds: one leaving shadows, fog and point lights to
    /// the scene (ignore 0x2FF), one owning shadows off (0x3FF), one owning
    /// point lights off too (0x3BFF), and the gloss-mapped one, drawn with
    /// its gloss (AG.22).
    #[test]
    fn level_material_keys_become_their_shader_defs() {
        let l = level_lights(&lights(), &zone()).unwrap();
        let scene = view_scene(&l, &LevelFog::NONE, Mat4::IDENTITY);
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
            0x1B | 1 << bit::FOG | 0b111 << bit::POINT_LIGHTS
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
                def::POINT_LIGHTS[1],
                def::POINT_LIGHTS[2],
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
                def::POINT_LIGHTS[1],
                def::POINT_LIGHTS[2],
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
        let d = drawn(graph.clone(), true).unwrap();
        assert_eq!(d.materials.len(), 2);
        assert_eq!((d.left_out_materials, d.left_out_segments), (1, 1));
        assert_eq!(d.sources, [0, 2, 3, 4]);
        assert_eq!(
            d.segments.iter().map(|s| s.material).collect::<Vec<_>>(),
            [0, 1, 0, 0]
        );
        assert_eq!(d.sky, [1]);

        let l = level_lights(&lights(), &zone()).unwrap();
        let scene = view_scene(&l, &LevelFog::NONE, Mat4::IDENTITY);
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
            drawn(
                SceneGraph {
                    world: "Levels/Test/Test_GRASS.ma".into(),
                    materials: vec![LevelMaterial::Multi2(ground), effect()],
                    segments: vec![segment(1, false), segment(0, true)],
                    sky: Vec::new(),
                },
                true,
            )
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
            drawn(bad, true),
            Err(LevelError::Segment {
                segment: 5,
                material: 3,
                materials: 3
            })
        );
    }

    /// AG.41: with the sky dome taking none of the fog, the sky's segments
    /// draw on copies of their materials that own their fog off, so in the
    /// level's fogged scene the sky keys without fog and the rest with it,
    /// as the original's sky draws run unfogged programs; a material the
    /// sky and the ground share draws on both.
    #[test]
    fn an_unfogged_sky_draws_on_copies_that_own_their_fog_off() {
        let ground = multi2(mask(0x1A, 0x3FF), mask(0x101A, 0x1BFF));
        let sky = multi2(mask(0x0, 0x3BFF), mask(0x1000, 0x3BFF));
        let graph = SceneGraph {
            world: "Levels/Test/Test_ART_MASTER.ma".into(),
            materials: vec![
                effect(),
                LevelMaterial::Multi2(ground.clone()),
                LevelMaterial::Multi2(sky.clone()),
            ],
            // The ground's material also draws one sky segment.
            segments: vec![
                segment(1, false),
                segment(2, true),
                segment(1, false),
                segment(0, false),
            ],
            sky: vec![SkyGroup {
                name: "sky".into(),
                segments: vec![1, 2],
            }],
        };
        let d = drawn(graph, false).unwrap();
        // The ground, the ground's copy for the sky and the sky's copy; the
        // sky's own material, which only the sky draws, isn't kept beside it.
        assert_eq!(d.materials.len(), 3);
        assert_eq!((d.left_out_materials, d.left_out_segments), (1, 1));
        assert_eq!(
            d.segments.iter().map(|s| s.material).collect::<Vec<_>>(),
            [0, 2, 1]
        );
        assert_eq!(d.sky, [1, 2]);
        assert_eq!(
            d.materials[0],
            drawn_material(&LevelMaterial::Multi2(ground.clone())).unwrap()
        );
        for (k, of) in [(1, &ground), (2, &sky)] {
            let m = &d.materials[k];
            assert_eq!(m.pixel.ignore >> bit::FOG & 1, 1, "owned");
            assert_eq!(m.pixel.base >> bit::FOG & 1, 0, "off");
            assert_eq!(m.vertex.ignore >> bit::FOG & 1, 1);
            assert_eq!(m.pixel.base, of.pixel.base);
            assert_eq!(m.textures, of.textures);
        }
        let l = level_lights(&lights(), &zone()).unwrap();
        let fog = LevelFog::from_zone(&zone().fog).unwrap();
        let scene = view_scene(&l, &fog, Mat4::IDENTITY);
        let k = keys(&d, &scene).unwrap();
        let fogged: Vec<(usize, bool)> = k
            .iter()
            .map(|(k, m)| (*m, k.material.shader.switches().fog))
            .collect();
        assert_eq!(fogged, [(0, true), (2, false), (1, false)]);
    }

    /// AG.79: the sky writes no depth, whatever its materials' states say,
    /// fogged or not, and its blended parts sort before anything else that
    /// blends, in the scene's order, wherever their middles are: AG.13's
    /// capture draws the clouds, the horizon rings and then the cap, none
    /// written, where a cloud plane drawn first and written cut the cap.
    #[test]
    fn the_sky_writes_no_depth_and_blends_first_in_the_scenes_order() {
        let blended = |depth_write| RenderStates {
            blend: Blend::Alpha,
            depth_write,
            two_sided: false,
            alpha_test: None,
        };
        let mut leaves = multi2(mask(0x1A, 0x3FF), mask(0x101A, 0x1BFF));
        leaves.states = blended(true);
        let dome = multi2(mask(0x0, 0x3BFF), mask(0x0, 0x3BFF));
        let mut clouds = multi2(mask(0x0, 0x3BFF), mask(0x1000, 0x3BFF));
        clouds.states = blended(true);
        let mut cap = clouds.clone();
        cap.name = "cap".into();
        let graph = SceneGraph {
            world: "Levels/Test/Test_ART_MASTER.ma".into(),
            materials: vec![
                LevelMaterial::Multi2(leaves),
                LevelMaterial::Multi2(cap),
                LevelMaterial::Multi2(clouds),
                LevelMaterial::Multi2(dome),
            ],
            segments: vec![
                segment(0, false),
                segment(1, true),
                segment(3, false),
                segment(2, true),
            ],
            // The scene lists the dome, then the clouds, then the cap.
            sky: vec![SkyGroup {
                name: "sky".into(),
                segments: vec![2, 3, 1],
            }],
        };
        for fogged in [false, true] {
            let d = drawn(graph.clone(), fogged).unwrap();
            let by_segment: Vec<&model::Material> = d
                .segments
                .iter()
                .map(|s| &d.materials[usize::from(s.material)])
                .collect();
            assert!(by_segment[0].states.depth_write, "the leaves keep theirs");
            for (i, m) in by_segment.iter().enumerate().skip(1) {
                assert!(!m.states.depth_write, "sky segment {i}");
                assert_eq!(
                    m.pixel.base >> bit::FOG & 1 == 0 && m.pixel.ignore >> bit::FOG & 1 == 1,
                    !fogged,
                    "sky segment {i} owns its fog off unless the sky's fogged"
                );
            }
            let bias = depth_biases(&d);
            let of = |segment: usize| bias[usize::from(d.segments[segment].material)];
            assert_eq!(
                [of(0), of(2), of(3), of(1)],
                [0.0, sky_bias(0), sky_bias(1), sky_bias(2)]
            );
            // Bevy's blended pass sorts by view depth plus bias, back to
            // front: the cap's middle 600 m nearer than the clouds' still
            // draws after them, and everything else after the sky.
            let sorted = |depth: f32, segment: usize| depth + of(segment);
            assert!(sorted(-700.0, 3) < sorted(-100.0, 1));
            assert!(sorted(-100.0, 1) < sorted(-1000.0, 0));
            assert!(sorted(-1.0, 1) < sorted(-250.0, 0));
        }
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

    /// AH.65: the three point lights a frame draws the level with, the
    /// nearest the hero first of the placed ones and the scripts', those
    /// that add nothing (dark, or no falloff) left out, the slots past them
    /// dark.
    #[test]
    fn the_frames_point_lights_are_the_three_nearest_that_add_light() {
        let at = |x: f32, colour: f32| PointLight {
            position: [x, 0.0, 0.0],
            colour: [colour; 3],
            linear: 1.0,
            quadratic: 0.0,
        };
        // Level 213's one at the origin, 103 m off.
        let placed = [at(0.0, 0.3)];
        let hero = Vec3::new(103.0, 0.0, 0.0);
        let lit = [
            at(110.0, 0.5),
            at(100.0, 0.5),
            at(104.0, 0.0),
            at(90.0, 0.5),
        ];
        let frame = frame_point_lights(&placed, &lit, hero);
        assert_eq!(frame, [at(100.0, 0.5), at(110.0, 0.5), at(90.0, 0.5)]);
        let no_falloff = PointLight {
            linear: 0.0,
            ..at(103.0, 0.5)
        };
        assert_eq!(
            frame_point_lights(&placed, &[no_falloff], hero),
            [at(0.0, 0.3), DARK, DARK]
        );
        assert_eq!(frame_point_lights(&[], &[], hero), [DARK; 3]);
    }

    /// AH.65: a light the scripts lit draws its diffuse decoded as every
    /// colour is, times its multiplier, its linear and quadratic
    /// attenuation the falloff's terms (Eruptor's ball's light, as the
    /// point-light manager copies it from `009_Eruptor_projectile_light`).
    #[test]
    fn a_scripts_light_draws_its_diffuse_decoded_times_its_multiplier() {
        let l = crate::script_engine::Light {
            diffuse: 0x5F29_1BFF,
            ambient: 0xFF,
            specular: 0x3B26_12FF,
            multiplier: 2.0,
            attenuation: [0.25, 1.5, 0.5],
            lift: 0.0,
        };
        let p = script_light(Vec3::new(1.0, 2.0, 3.0), &l);
        assert_eq!(p.position, [1.0, 2.0, 3.0]);
        assert_eq!(
            p.colour,
            remaster::linear([0x5F, 0x29, 0x1B]).map(|c| c * 2.0)
        );
        assert_eq!((p.linear, p.quadratic), (1.5, 0.5));
    }

    fn placement(name: &str, waypoints: Option<u32>) -> guac_ir::level::Placement {
        guac_ir::level::Placement {
            object: 7,
            id: 0,
            name: name.into(),
            script: None,
            position: [0.0; 3],
            rotation: [0.0; 3],
            scale: [100.0; 3],
            model: None,
            solid: None,
            movement: None,
            waypoints,
            activity_state: 4,
            activation_range: 0.0,
        }
    }

    fn waypoint(position: [f32; 3]) -> guac_ir::level::Waypoint {
        guac_ir::level::Waypoint {
            name: "wp".into(),
            position,
            rotation: [0.0, 90.0, 0.0],
            height: 0.0,
        }
    }

    /// AG.21: the hero starts at `213_Init_Location`'s first waypoint, not
    /// at the placement itself or another path's; a level without one, or
    /// whose path is empty or out of range, has no start.
    #[test]
    fn the_start_is_the_init_locations_first_waypoint() {
        let first = [10.191352, 43.37944, -93.393616];
        let mut p = Placements {
            identity: identity(),
            level: "level_213_tutotial".into(),
            actors: vec![
                placement("Sheep_1", Some(0)),
                placement(START_PLACEMENT, Some(1)),
            ],
            waypoints: vec![
                vec![waypoint([1.0, 2.0, 3.0])],
                vec![waypoint(first), waypoint([0.0; 3])],
            ],
        };
        assert_eq!(start_point(&p), Ok(Vec3::from_array(first)));
        assert_eq!(START_FACING, 0.0);
        p.waypoints[1].clear();
        assert_eq!(start_point(&p), Err(LevelError::NoStart));
        p.actors[1].waypoints = Some(9);
        assert_eq!(start_point(&p), Err(LevelError::NoStart));
        p.actors[1].waypoints = None;
        assert_eq!(start_point(&p), Err(LevelError::NoStart));
        p.actors.pop();
        assert_eq!(start_point(&p), Err(LevelError::NoStart));
    }

    /// AG.21: each world's collision mesh is a static body in the level's
    /// frame, numbered from 1, its triangles and their materials kept; no
    /// mesh, or one the seam refuses, is the level's error.
    #[test]
    fn the_levels_collision_becomes_static_bodies() {
        let mesh = |world: &str, y: f32| guac_ir::level::CollisionMesh {
            world: world.into(),
            vertices: vec![[0.0, y, 0.0], [1.0, y, 0.0], [0.0, y, 1.0], [1.0, y, 1.0]],
            triangles: vec![[0, 2, 1], [1, 2, 3]],
            materials: vec![1, 3],
            radius: 0.05,
            welding: 4,
        };
        let mut c = Collision {
            identity: identity(),
            level: "level_213_tutotial".into(),
            meshes: vec![mesh("a", 0.0), mesh("b", 5.0)],
        };
        let bodies = solids(&c).unwrap();
        assert_eq!(bodies.len(), 2);
        for (body, (id, y)) in bodies.iter().zip([(1, 0.0), (2, 5.0)]) {
            assert_eq!(
                (body.id, body.kind, body.pose),
                (BodyId(id), BodyKind::Static, Pose::IDENTITY)
            );
            let Collider::Mesh(m) = &body.collider else {
                panic!("{body:?}")
            };
            assert_eq!(m.triangles(), &[[0, 2, 1], [1, 2, 3]]);
            assert_eq!(m.vertices()[3], Vec3::new(1.0, y, 1.0));
            assert_eq!(m.materials(), &[1, 3]);
        }
        c.meshes[1].triangles[1] = [1, 2, 4];
        assert!(matches!(
            solids(&c),
            Err(LevelError::Collision { world, .. }) if world == "b"
        ));
        c.meshes.clear();
        assert_eq!(solids(&c).err(), Some(LevelError::NoCollision));
    }
}
