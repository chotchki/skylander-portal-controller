//! A made-up chapter 1 and the walk's parts beside it, from level's tests;
//! play's tests stand on them too.

use std::path::Path;

use bevy::prelude::*;
use guac_extract::intros::IrFormat;
use guac_extract::levels::{ATMOSPHERE, COLLISION, LIGHTS, PLACEMENTS, PROPS, SCENE};
use guac_formats::texture::{Format, Texture};
use guac_ir::RigLight;
use guac_ir::level::{
    Atmosphere, AtmosphericZone, EffectMaterial, LevelMaterial, Lights, PlacedLight, SceneGraph,
    SkyGroup, ZoneFog, ZoneLight,
};
use guac_ir::model::{
    self, Blend, Filter, Glow, Mask, Params, RenderStates, Sampler, Scroll, TextureRole,
    TextureSlot, Wrap,
};
use guacamole::character::warm::MeshShape;

use crate::level::*;
use crate::test_support::turntable::identity;

pub(crate) const LEVEL: &str = "level_213_tutotial";

fn slot(role: TextureRole, texture: &str) -> TextureSlot {
    TextureSlot {
        role,
        texture: format!("textures/{texture}.ktx2"),
        image: texture.into(),
        sampler: Sampler {
            wrap_u: Wrap::Repeat,
            wrap_v: Wrap::Repeat,
            mag: Filter::Linear,
            min: Filter::Linear,
            mip: None,
        },
    }
}

/// A made-up tfbmulti2 material on these masks, its maps one texture
/// each.
pub(crate) fn multi2(
    name: &str,
    pixel: Mask,
    vertex: Mask,
    maps: &[TextureRole],
) -> model::Material {
    model::Material {
        name: name.into(),
        pixel,
        vertex,
        textures: maps
            .iter()
            .map(|r| slot(*r, &format!("{name}{r:?}")))
            .collect(),
        params: Params {
            fixed_alpha: [-1.0, -1.0, 1.0, -1.0],
            layer1_extra: [1.0, 1.0, 1.0, 0.25],
            layer2_extra: None,
            layer2_blend: None,
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

pub(crate) fn segment(material: u16, shape: MeshShape, at: f32) -> model::Segment {
    let mut s = shape.triangle(material);
    s.positions = vec![[at, 0.0, 0.0], [at + 1.0, 0.0, 0.0], [at, 0.0, 1.0]];
    s
}

fn rig_light(name: &str, rotation: [f32; 3]) -> RigLight {
    RigLight {
        name: name.into(),
        diffuse: [90, 80, 70, 255],
        ambient: [20, 20, 20, 255],
        specular: [60, 60, 60, 255],
        multiplier: 1.0,
        position: [0.0; 3],
        rotation,
    }
}

/// Made-up lights at the world rig's angles, and a point light.
pub(crate) fn lights() -> Lights {
    Lights {
        identity: identity(),
        level: LEVEL.into(),
        directional: vec![
            rig_light("_worldLight1", [-45.0, 0.0, 0.0]),
            rig_light("_worldLight2", [45.0, 180.0, 0.0]),
            rig_light("_worldLight3", [-45.0, 90.0, 0.0]),
            rig_light("_causticLight", [-45.0, 0.0, 0.0]),
        ],
        placed: vec![PlacedLight {
            name: "beam".into(),
            light_type: 1,
            visible: true,
            position: [0.0; 3],
            rotation: [0.0; 3],
            falloff_rate: 0.0,
            cutoff_angle: 0.0,
            attenuation: [0.0, 0.5, 0.0],
            diffuse: [80, 40, 20, 255],
            ambient: [0, 0, 0, 255],
            specular: [0, 0, 0, 255],
            multiplier: 1.0,
        }],
    }
}

/// The start's zone: made-up lights at the rig's angles, and the fog
/// AG.13 read off the opening area at AF's fresh state, (115, 83, 49)
/// from 100 ft over 1,000 ft, the sky dome taking none.
pub(crate) fn zone() -> AtmosphericZone {
    let light = |facing, tilt| ZoneLight {
        diffuse: [90, 80, 70, 0],
        specular: [60, 60, 60, 0],
        facing,
        tilt,
    };
    AtmosphericZone {
        id: 0,
        ambient: [20, 20, 20, 0],
        lights: [light(0, -45), light(180, 45), light(90, -45)],
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

pub(crate) fn atmosphere() -> Atmosphere {
    Atmosphere {
        identity: identity(),
        level: LEVEL.into(),
        script: "C:/tfb/Content/Levels/Level_900/Scripts/Level_Definition_900.ai".into(),
        zones: vec![zone()],
        start: Some(0),
    }
}

/// A made-up chapter 1 beside `extract`'s models: a ground layered and
/// normal-mapped on two mesh shapes, a scrolling sky, and grass, which
/// isn't drawn; its five textures.
pub(crate) fn write_level(dir: &Path, lights: &Lights) -> LevelSource {
    let plain = MeshShape {
        uv_sets: 2,
        tangents: true,
        colours: false,
        skinned: false,
    };
    let ground = multi2(
        "ground",
        Mask {
            base: 0x1A,
            ignore: 0x3FF,
        },
        Mask {
            base: 0x101A,
            ignore: 0x1BFF,
        },
        &[
            TextureRole::Layer1ColorMap,
            TextureRole::Layer3ColorMap,
            TextureRole::Layer1BaseNormalMap,
            TextureRole::Layer1BaseSpecMap,
        ],
    );
    let mut sky = multi2(
        "sky",
        Mask {
            base: 0,
            ignore: 0x3BFF,
        },
        Mask {
            base: 0x1000,
            ignore: 0x3BFF,
        },
        &[TextureRole::Layer1ColorMap],
    );
    // Sliding along u, half a texture along at the start.
    sky.scrolls = vec![Scroll {
        layer: 0,
        scale: [1.0, 1.0],
        velocity: [0.25, 0.0],
        amplitude: [0.5, 0.0],
        frequency: [1.0, 0.0],
        phase: [std::f32::consts::FRAC_PI_2, 0.0],
        rotation_phase: 0.0,
    }];
    let grass = LevelMaterial::Effect(EffectMaterial {
        name: "grass_HIRES".into(),
        effect: 0xAA6,
        pixel: None,
        vertex: None,
        textures: Vec::new(),
        states: sky.states,
        scrolls: Vec::new(),
        tracks: Vec::new(),
        unread: Vec::new(),
    });
    let scene = guac_ir::level::Scene {
        identity: identity(),
        level: LEVEL.into(),
        scenes: vec![SceneGraph {
            world: "Levels/Test/Test_ART_MASTER.ma".into(),
            materials: vec![
                LevelMaterial::Multi2(ground.clone()),
                grass,
                LevelMaterial::Multi2(sky.clone()),
            ],
            segments: vec![
                segment(0, plain, 0.0),
                segment(
                    0,
                    MeshShape {
                        colours: true,
                        ..plain
                    },
                    2.0,
                ),
                segment(
                    1,
                    MeshShape {
                        uv_sets: 1,
                        ..plain
                    },
                    4.0,
                ),
                segment(
                    2,
                    MeshShape {
                        uv_sets: 1,
                        tangents: false,
                        ..plain
                    },
                    6.0,
                ),
                segment(0, plain, 8.0),
            ],
            sky: vec![SkyGroup {
                name: "sky".into(),
                segments: vec![3],
            }],
        }],
    };
    let ir = IrFormat::Ron;
    let at = dir.join("levels").join(LEVEL);
    std::fs::create_dir_all(&at).unwrap();
    std::fs::create_dir_all(dir.join("textures")).unwrap();
    std::fs::write(
        dir.join(ir.level_path(LEVEL, SCENE)),
        guac_ir::to_ron(&scene).unwrap(),
    )
    .unwrap();
    std::fs::write(
        dir.join(ir.level_path(LEVEL, LIGHTS)),
        guac_ir::to_ron(lights).unwrap(),
    )
    .unwrap();
    std::fs::write(
        dir.join(ir.level_path(LEVEL, ATMOSPHERE)),
        guac_ir::to_ron(&atmosphere()).unwrap(),
    )
    .unwrap();
    write_walk(dir);
    // A 4x4 BC1 block of one colour: built here, none of the disc's.
    let ktx2 = Texture {
        format: Format::Bc1RgbaUnorm,
        width: 4,
        height: 4,
        levels: vec![vec![0x1F, 0xF8, 0x1F, 0xF8, 0, 0, 0, 0]],
    }
    .to_ktx2();
    for m in [&ground, &sky] {
        for t in &m.textures {
            std::fs::write(dir.join(&t.texture), &ktx2).unwrap();
        }
    }
    LevelSource {
        dir: dir.to_path_buf(),
        ir,
        identity: identity(),
        level: LEVEL.into(),
    }
}

/// Where [`write_walk`]'s hero starts, and its floor's height.
pub(crate) const START: [f32; 3] = [10.0, 40.0, -90.0];

pub(crate) const FLOOR: f32 = 39.99;

/// How far ahead of the start, along +z, the wall stands.
pub(crate) const WALL: f32 = 6.0;

/// The walk's two parts beside a made-up level: a 40 m wide floor under
/// the start, ending [`WALL`] ahead of it along +z at a 3 m wall that
/// shares its edge (level 213's mesh is indexed the same way), and
/// `213_Init_Location` with its first waypoint at the start.
pub(crate) fn write_walk(dir: &Path) {
    let ir = IrFormat::Ron;
    let [x, z] = [START[0], START[2]];
    let wall_z = z + WALL;
    let floor = guac_ir::level::CollisionMesh {
        world: "Levels/Test/Test_ART_MASTER.ma".into(),
        vertices: vec![
            [x - 20.0, FLOOR, z - 20.0],
            [x + 20.0, FLOOR, z - 20.0],
            [x + 20.0, FLOOR, wall_z],
            [x - 20.0, FLOOR, wall_z],
            [x + 20.0, FLOOR + 3.0, wall_z],
            [x - 20.0, FLOOR + 3.0, wall_z],
        ],
        // Up, then the wall's face toward the start (-z).
        triangles: vec![[0, 2, 1], [0, 3, 2], [3, 4, 2], [3, 5, 4]],
        materials: vec![1; 4],
        radius: 0.05,
        welding: 4,
    };
    let collision = guac_ir::level::Collision {
        identity: identity(),
        level: LEVEL.into(),
        meshes: vec![floor],
    };
    let placements = guac_ir::level::Placements {
        identity: identity(),
        level: LEVEL.into(),
        actors: vec![guac_ir::level::Placement {
            object: 1,
            id: 0,
            name: guacamole::level_scene::START_PLACEMENT.into(),
            script: None,
            position: [0.0; 3],
            rotation: [0.0; 3],
            scale: [100.0; 3],
            model: None,
            solid: None,
            movement: None,
            waypoints: Some(0),
            activity_state: 4,
            activation_range: 0.0,
        }],
        waypoints: vec![vec![guac_ir::level::Waypoint {
            name: "wp0".into(),
            position: START,
            rotation: [0.0; 3],
            height: 0.0,
        }]],
    };
    std::fs::create_dir_all(dir.join("levels").join(LEVEL)).unwrap();
    std::fs::write(
        dir.join(ir.level_path(LEVEL, COLLISION)),
        guac_ir::to_ron(&collision).unwrap(),
    )
    .unwrap();
    std::fs::write(
        dir.join(ir.level_path(LEVEL, PLACEMENTS)),
        guac_ir::to_ron(&placements).unwrap(),
    )
    .unwrap();
    // No props.
    write_props(dir, Vec::new(), Vec::new());
}

/// The props part holding `models`, and the placements part with
/// `actors` after [`write_walk`]'s `213_Init_Location`.
pub(crate) fn write_props(
    dir: &Path,
    models: Vec<guac_ir::level::Prop>,
    actors: Vec<guac_ir::level::Placement>,
) {
    let ir = IrFormat::Ron;
    let props = guac_ir::level::Props {
        identity: identity(),
        level: LEVEL.into(),
        props: models,
        refused: Vec::new(),
    };
    std::fs::write(
        dir.join(ir.level_path(LEVEL, PROPS)),
        guac_ir::to_ron(&props).unwrap(),
    )
    .unwrap();
    if actors.is_empty() {
        return;
    }
    let path = dir.join(ir.level_path(LEVEL, PLACEMENTS));
    let mut placements: guac_ir::level::Placements =
        guac_ir::from_ron(&std::fs::read_to_string(&path).unwrap()).unwrap();
    placements.actors.extend(actors);
    std::fs::write(path, guac_ir::to_ron(&placements).unwrap()).unwrap();
}

/// A made-up emitter (AH.87): a stream of quads adding up a texture the
/// level's sky already draws, glowing, 50 a second for 0.4 s each, and a
/// generator with no texture, which draws nothing.
pub(crate) fn trail() -> guac_ir::particle::Emitter {
    use guac_ir::particle::{Blend as P, Direction, Generator, Keyed, Kind, Sprite, Volume};
    fn keyed<T: Copy>(start: T, end: T, zero: T) -> Keyed<T> {
        Keyed {
            start,
            start_random: zero,
            mid: start,
            mid_random: zero,
            end,
            end_random: zero,
        }
    }
    let sprite = Sprite {
        spawn_rate: 50.0,
        max_alive: 24,
        lifespan: 0.4,
        lifespan_random: 0.0,
        max_distance: 30.0,
        midpoint_percent: -50.0,
        next_gen_scale: 1.0,
        volume: Some(Volume::Box),
        volume_size: [0.2; 3],
        uniform: false,
        inherits_translation: false,
        direction: Some(Direction::Orientation),
        speed: 1.0,
        speed_random: 0.0,
        spread: 0.5,
        drag: [0.0; 3],
        gravity: [0.0, 0.0, -2.0],
        colour: keyed([127, 64, 0, 255], [127, 32, 0, 0], [0; 4]),
        size: keyed([0.5; 3], [0.1; 3], [0.0; 3]),
        rotation: 0.0,
        rotation_random: 6.0,
        spin: keyed(1.0, 2.0, 0.0),
        random_spin_direction: true,
        view_alignment: 0,
    };
    let generator = |texture: Option<guac_ir::particle::Texture>| Generator {
        blend: P::Add,
        overlay_layer: 5,
        bloom: 5.0,
        offset: guac_ir::model::Transform {
            translation: [0.0; 3],
            rotation: [0.0, 0.0, 0.0, 1.0],
            scale: [1.0; 3],
        },
        start_delay: 0.0,
        texture,
        kind: Kind::Sprite(sprite.clone()),
    };
    let sky = slot(
        TextureRole::Layer1ColorMap,
        &format!("sky{:?}", TextureRole::Layer1ColorMap),
    );
    guac_ir::particle::Emitter {
        name: "Trail".into(),
        object: 1,
        generators: vec![
            generator(Some(guac_ir::particle::Texture {
                texture: sky.texture,
                image: sky.image,
                sampler: sky.sampler,
            })),
            generator(None),
        ],
        unread: Vec::new(),
    }
}

/// The particle part (AH.87): [`trail`] in the fighter's archive, nothing
/// in `global.bld`'s.
pub(crate) fn write_particles(dir: &Path) {
    write_particles_of(dir, trail());
}

/// [`write_particles`] with `emitter` as the fighter's one.
pub(crate) fn write_particles_of(dir: &Path, emitter: guac_ir::particle::Emitter) {
    let ir = IrFormat::Ron;
    std::fs::create_dir_all(dir.join(guac_extract::particles::DIR)).unwrap();
    for archive in guac_extract::particles::archives() {
        let emitters = guac_ir::particle::Emitters {
            identity: identity(),
            archive: archive.clone(),
            emitters: if archive.contains("eruptor") {
                vec![emitter.clone()]
            } else {
                Vec::new()
            },
            refused: Vec::new(),
        };
        std::fs::write(
            dir.join(guac_extract::particles::path(ir, &archive)),
            guac_ir::to_ron(&emitters).unwrap(),
        )
        .unwrap();
    }
}
