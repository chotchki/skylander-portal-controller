//! The levels guacamole plays off the dump (openspec AG.15, design decision
//! 8): chapter 1, `level/level_213_tutotial.bld`, into IR v3's level
//! records ([`guac_ir::level`]) in eight parts, each a file of its own under
//! `levels/<archive>/`, the textures into the characters' store. Writing
//! them is the level stage's ([`crate::stages`]).
//!
//! - The scene: every scene's draws on AE's segment and material records,
//!   tfbmulti2's read as a level's ([`Drawn::Level`]: the gloss map taken,
//!   bits 6 and 9 refused by name), the grass's, `0xAA8`'s and tfbwarp's as
//!   [`EffectMaterial`]s, and the sky's groups. A draw under a transform is
//!   refused: level 213's scenes have none.
//! - The props: each model a placement names, a skinned one as a
//!   character's is read (its skeleton, slot 0 as its idle when that poses
//!   it), a static one through its body's graph. One that doesn't read is
//!   listed in the part, not fatal to it.
//! - The collision (AG.2's reader), the placements and the lights (AG.3's).
//! - The atmosphere (AG.41): the zones the level's definition script adds
//!   at its start on a PS3, read statically ([`guac_formats::atmosphere`]).
//!   An extract written before it gains this part alone on its next start.
//! - The script (AH.22, design decision 7): the level's `level.bld` as the
//!   disc holds it, decompressed, `levels/<archive>/script.igz`, checked to
//!   parse and its op graph to read; whole, since a script refers to its own
//!   file's placements, structures and sets. The same file in either IR
//!   format.
//! - The palettes (AH.22): every slot of each skinned model a placement
//!   running a whitelisted script names ([`crate::scripts::WHITELIST`]: the
//!   level's and the walkers' own, AG.162): chapter 1's Chompies and Norticus.
//!
//! A part that doesn't read is the level stage's failure for that part
//! alone: the other parts land, and the next start tries it again.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use guac_formats::FormatError;
use guac_formats::collision;
use guac_formats::edge::{BoneMap, Segment as EdgeSegment};
use guac_formats::iga::Archive;
use guac_formats::igz::Igz;
use guac_formats::igz::layouts::{
    Actor2, AttrList, Hero, Layout, ObjectRef, Passes, PhysicsBody, PhysicsModel, PhysicsWorld,
    RuntimeTechniqueInstance, is_a, read_all,
};
use guac_formats::level::{GroupKind, IDENTITY, Placements as FormatPlacements, StaticModel};
use guac_formats::material::{self, Drawn, TFBMULTI2};
use guac_formats::model::{self as fmodel, IDLE_SLOT, JointMap};
use guac_ir::level::{
    Atmosphere, AtmosphericZone, Collision, CollisionMesh, EffectMaterial, LevelMaterial, Lights,
    PlacedLight, Placement, Placements, Prop, Props, RefusedProp, Scene, SceneGraph, SkyGroup,
    UnitTexture, Waypoint, ZoneFog, ZoneLight,
};
use guac_ir::model::{Mask, Segment};
use guac_ir::palette::LevelPalettes;
use guac_ir::{DumpIdentity, RigLight};

use crate::extract::Manifest;
use crate::models::{self, Textures};
use crate::roster::LEVEL;

/// Where levels go in the extract: `levels/<archive>/<part>.<ron|postcard>`.
pub const LEVEL_DIR: &str = "levels";
/// The folder holding the level archives, under `PS3_GAME/USRDIR`.
pub const LEVEL_FOLDER: &str = "level";
/// The levels this build plays, by archive name: chapter 1, "Time of the
/// Giants" (ag-motion section 4; the archive's own spelling).
pub const LEVELS: [&str; 1] = ["level_213_tutotial"];

/// The parts, in the order the stage reads them.
pub const SCENE: &str = "scene";
pub const PROPS: &str = "props";
pub const COLLISION: &str = "collision";
pub const PLACEMENTS: &str = "placements";
pub const LIGHTS: &str = "lights";
pub const ATMOSPHERE: &str = "atmosphere";
pub const SCRIPT: &str = "script";
pub const PALETTES: &str = "palettes";
pub const PARTS: [&str; 8] = [
    SCENE, PROPS, COLLISION, PLACEMENTS, LIGHTS, ATMOSPHERE, SCRIPT, PALETTES,
];

/// The script part's file, `levels/<archive>/script.igz`: raw, so the same
/// in either IR format.
pub fn script_path(level: &str) -> String {
    format!("{LEVEL_DIR}/{level}/{SCRIPT}.igz")
}

/// `part`'s file in `ir`'s format: its IR, or the script's raw entry.
pub fn part_path(ir: crate::intros::IrFormat, level: &str, part: &str) -> String {
    if part == SCRIPT {
        script_path(level)
    } else {
        ir.level_path(level, part)
    }
}

/// `level/<archive>.bld` under the dump's `PS3_GAME/USRDIR`.
pub fn archive_path(usrdir: &Path, level: &str) -> PathBuf {
    usrdir.join(LEVEL_FOLDER).join(format!("{level}.bld"))
}

/// Whether `m` holds every part of every level this build plays.
pub fn complete(m: &Manifest) -> bool {
    LEVELS.iter().all(|level| {
        m.levels
            .get(*level)
            .is_some_and(|l| PARTS.iter().all(|p| l.parts.contains_key(*p)))
    })
}

/// The `level.bld` a level archive keeps the level in.
pub fn level_bld(archive: &[u8]) -> Result<Vec<u8>, String> {
    let arc = Archive::parse(archive).map_err(|e| e.to_string())?;
    let entry = arc.find(LEVEL).ok_or("no level.bld")?;
    Ok(arc.read(entry).map_err(|e| e.to_string())?.into_owned())
}

/// One part read: its IR and the texture files it binds.
#[derive(Debug, Clone)]
pub struct Read<T> {
    pub ir: T,
    pub textures: Vec<(String, Vec<u8>)>,
}

fn s(e: FormatError) -> String {
    e.to_string()
}

/// The `igAttrList` a technique's `_materialPass` names, if that's what it
/// is.
fn pass(igz: &Igz<'_>, t: &RuntimeTechniqueInstance<'_>) -> Option<usize> {
    match t.material_pass {
        ObjectRef::Object(m) if is_a(igz.type_name(&igz.objects()[m]), AttrList::TYPE) => Some(m),
        _ => None,
    }
}

/// The technique at `t`'s material as the IR keeps it: tfbmulti2's on AE's
/// record, read as a level's, or another effect's.
fn level_material(
    igz: &Igz<'_>,
    passes: Passes<'_, '_>,
    t: usize,
    technique: &RuntimeTechniqueInstance<'_>,
    textures: &mut Textures,
) -> Result<LevelMaterial, String> {
    let list = pass(igz, technique);
    if material::effect(igz, t, technique).map_err(s)? == TFBMULTI2 {
        let m = material::Material::read_technique(igz, passes, t, technique, list, Drawn::Level)
            .map_err(s)?;
        return models::multi2(igz, &m, textures).map(LevelMaterial::Multi2);
    }
    let m = material::EffectMaterial::read(igz, passes, t, technique, list).map_err(s)?;
    let mut out = Vec::with_capacity(m.textures.len());
    for b in &m.textures {
        out.push(UnitTexture {
            unit: b.unit,
            texture: textures.path(igz, b.image).map_err(s)?,
            image: b.image_name.unwrap_or_default().to_owned(),
            sampler: models::sampler(&b.sampler)?,
        });
    }
    let mask = |m: Option<guac_formats::igz::layouts::VariantMask>| {
        m.map(|m| Mask {
            base: m.base_mask,
            ignore: m.ignore_mask,
        })
    };
    Ok(LevelMaterial::Effect(EffectMaterial {
        name: m.name.unwrap_or_default().to_owned(),
        effect: m.effect,
        pixel: mask(m.pixel),
        vertex: mask(m.vertex),
        textures: out,
        states: models::states(&m.states),
        scrolls: m.scrolls.iter().map(models::scroll).collect(),
        tracks: m.tracks.iter().map(models::track).collect(),
        unread: m.unread.iter().map(|u| (*u).to_owned()).collect(),
    }))
}

/// A static graph's draws as materials and segments, where the graph has
/// them: a draw under a transform is refused.
fn static_draws(
    igz: &Igz<'_>,
    graph: &guac_formats::level::Graph<'_>,
    textures: &mut Textures,
    materials: &mut Vec<LevelMaterial>,
    segments: &mut Vec<Segment>,
    mut each: impl FnMut(&guac_formats::level::Draw<'_>, u32),
) -> Result<(), String> {
    let lists = graph.material_lists(igz).map_err(s)?;
    for draw in &graph.draws {
        let name = draw.technique.node.name.unwrap_or_default();
        if draw.world != IDENTITY {
            return Err(format!(
                "{name} (technique #{}) sits under a transform",
                draw.technique_index
            ));
        }
        let m = level_material(
            igz,
            lists.passes(),
            draw.technique_index,
            &draw.technique,
            textures,
        )
        .map_err(|e| format!("{name} (technique #{}): {e}", draw.technique_index))?;
        let normals = matches!(m, LevelMaterial::Multi2(_));
        let index = u16::try_from(materials.len()).map_err(|_| "65,536 materials")?;
        materials.push(m);
        for (i, layout) in graph.draw_segments(draw) {
            let decoded = EdgeSegment::decode(igz, i, layout).map_err(s)?;
            let seg = models::static_segment(&decoded, index, i as u32, normals)
                .map_err(|e| format!("segment #{i}: {e}"))?;
            each(draw, segments.len() as u32);
            segments.push(seg);
        }
    }
    Ok(())
}

/// The scene part: every `igSceneInfo` with the physics world that names
/// it.
pub fn scene(igz: &Igz<'_>, level: &str, identity: DumpIdentity) -> Result<Read<Scene>, String> {
    let mut worlds: BTreeMap<usize, String> = BTreeMap::new();
    for (_, w) in read_all::<PhysicsWorld>(igz) {
        let w = w.map_err(s)?;
        if let ObjectRef::Object(scene) = w.world.scene_info {
            let name = w.world.data_info.info.name.unwrap_or_default().to_owned();
            if worlds.insert(scene, name).is_some() {
                return Err(format!("scene #{scene} named by two physics worlds"));
            }
        }
    }
    let mut textures = Textures::default();
    let mut scenes = Vec::new();
    for (index, sc) in guac_formats::level::scenes(igz) {
        let sc = sc.map_err(s)?;
        let world = worlds
            .get(&index)
            .cloned()
            .ok_or_else(|| format!("scene #{index}: no physics world names it"))?;
        let (mut materials, mut segments) = (Vec::new(), Vec::new());
        let mut sky: BTreeMap<usize, Vec<u32>> = BTreeMap::new();
        static_draws(
            igz,
            &sc,
            &mut textures,
            &mut materials,
            &mut segments,
            |draw, segment| {
                if let Some(u) = draw.user_info {
                    sky.entry(u).or_default().push(segment);
                }
            },
        )
        .map_err(|e| format!("scene #{index}: {e}"))?;
        let sky = sky
            .into_iter()
            .map(|(u, segments)| SkyGroup {
                name: match &sc.groups[&u].kind {
                    GroupKind::UserInfo(g) => g.group.node.name.unwrap_or_default().to_owned(),
                    _ => String::new(),
                },
                segments,
            })
            .collect();
        scenes.push(SceneGraph {
            world,
            materials,
            segments,
            sky,
        });
    }
    Ok(Read {
        ir: Scene {
            identity,
            level: level.to_owned(),
            scenes,
        },
        textures: textures.files,
    })
}

/// A skinned prop, read as a character's model is: Havok's skeleton with
/// Alchemy's inverse joints, each technique's material and skinned
/// segments, slot 0 as its idle when that is a clip posing it.
fn skinned_prop(
    igz: &Igz<'_>,
    model: usize,
    path: &str,
    textures: &mut Textures,
) -> Result<Prop, String> {
    let hero = Hero::read(igz, model).map_err(s)?;
    let havok = fmodel::skeleton(igz, &hero).map_err(s)?;
    let joints = JointMap::new(&havok, &hero.bones).map_err(s)?;
    let inverse = joints
        .inverse_joints(&hero.bones, &hero.skeleton.inverse_joints)
        .map_err(s)?;
    let skeleton = models::skeleton(&havok, &inverse)?;
    let idle = fmodel::slot_clip(igz, &hero, &havok, IDLE_SLOT)
        .ok()
        .flatten()
        .map(|c| models::clip(&c.clip, c.file.unwrap_or_default()));
    let (mut materials, mut segments) = (Vec::new(), Vec::new());
    for mesh in &hero.meshes {
        let name = mesh.technique.node.name.unwrap_or_default();
        let m = level_material(
            igz,
            hero.passes(),
            mesh.technique_index,
            &mesh.technique,
            textures,
        )
        .map_err(|e| format!("{name}: {e}"))?;
        let index = u16::try_from(materials.len()).map_err(|_| "65,536 materials")?;
        materials.push(m);
        let palette = mesh
            .palette
            .and_then(|p| hero.palettes.get(&p)?.indices)
            .and_then(|i| hero.int_lists.get(&i))
            .ok_or_else(|| format!("{name}: a technique without a skin palette"))?;
        let bone_map = BoneMap::new(&palette.values, &hero.bones);
        for (i, layout) in hero.mesh_segments(mesh) {
            let decoded = EdgeSegment::decode(igz, i, layout).map_err(s)?;
            segments.push(
                models::segment(&decoded, index, i as u32, &bone_map, &joints)
                    .map_err(|e| format!("segment #{i}: {e}"))?,
            );
        }
    }
    Ok(Prop {
        path: path.to_owned(),
        skeleton: Some(skeleton),
        materials,
        segments,
        idle,
    })
}

/// A static prop: its body's graph, in the model's frame.
fn static_prop(
    igz: &Igz<'_>,
    model: usize,
    path: &str,
    textures: &mut Textures,
) -> Result<Prop, String> {
    let sm = StaticModel::read(igz, model).map_err(s)?;
    let (mut materials, mut segments) = (Vec::new(), Vec::new());
    static_draws(
        igz,
        &sm.graph,
        textures,
        &mut materials,
        &mut segments,
        |_, _| {},
    )?;
    Ok(Prop {
        path: path.to_owned(),
        skeleton: None,
        materials,
        segments,
        idle: None,
    })
}

/// The model at `model` as a prop, its textures into `textures`: skinned
/// when its body's node is an `igActor2`, static otherwise. A character's
/// attachments read this way too (AE.23).
pub fn prop(
    igz: &Igz<'_>,
    model: usize,
    path: &str,
    textures: &mut Textures,
) -> Result<Prop, String> {
    let skinned = PhysicsModel::read(igz, model)
        .ok()
        .and_then(|m| m.model.tfb_body)
        .and_then(|b| PhysicsBody::read(igz, b).ok())
        .and_then(|b| b.body.node)
        .is_some_and(|n| is_a(igz.type_name(&igz.objects()[n]), Actor2::TYPE));
    if skinned {
        skinned_prop(igz, model, path, textures)
    } else {
        static_prop(igz, model, path, textures)
    }
}

/// The props part: each model a placement names, by its path, skinned when
/// its body's node is an `igActor2`. One that doesn't read is listed with
/// why, and its textures stay out.
pub fn props(igz: &Igz<'_>, level: &str, identity: DumpIdentity) -> Result<Read<Props>, String> {
    let placements = FormatPlacements::read(igz).map_err(s)?;
    let mut textures = Textures::default();
    let (mut props, mut refused) = (Vec::new(), Vec::new());
    for (&model, info) in &placements.models {
        let path = info
            .info
            .name
            .ok_or_else(|| format!("model #{model} has no name"))?;
        if props.iter().any(|p: &Prop| p.path == path)
            || refused.iter().any(|r: &RefusedProp| r.path == path)
        {
            return Err(format!("two models named {path}"));
        }
        // A prop's textures land only if all of it reads.
        let mut own = textures.clone();
        match prop(igz, model, path, &mut own) {
            Ok(p) => {
                textures = own;
                props.push(p);
            }
            Err(reason) => refused.push(RefusedProp {
                path: path.to_owned(),
                reason,
            }),
        }
    }
    Ok(Read {
        ir: Props {
            identity,
            level: level.to_owned(),
            props,
            refused,
        },
        textures: textures.files,
    })
}

/// The collision part: each world's mesh where the level has it.
pub fn collision(
    igz: &Igz<'_>,
    level: &str,
    identity: DumpIdentity,
) -> Result<Read<Collision>, String> {
    let mut meshes = Vec::new();
    for (_, w) in collision::worlds(igz) {
        let w = w.map_err(s)?;
        let Some(body) = &w.collision.body else {
            continue;
        };
        meshes.push(CollisionMesh {
            world: w.name().unwrap_or_default().to_owned(),
            vertices: body.world_vertices().collect(),
            triangles: body.mesh.triangles.iter().map(|t| t.indices).collect(),
            materials: body.mesh.materials.clone(),
            radius: body.mesh.radius,
            welding: body.mesh.welding_type,
        });
    }
    Ok(Read {
        ir: Collision {
            identity,
            level: level.to_owned(),
            meshes,
        },
        textures: Vec::new(),
    })
}

/// The placements part: every actor and the waypoint lists they name.
pub fn placements(
    igz: &Igz<'_>,
    level: &str,
    identity: DumpIdentity,
) -> Result<Read<Placements>, String> {
    let p = FormatPlacements::read(igz).map_err(s)?;
    let lists: BTreeMap<usize, u32> = p
        .waypoint_lists
        .keys()
        .enumerate()
        .map(|(i, &k)| (k, i as u32))
        .collect();
    let waypoints = p
        .waypoint_lists
        .values()
        .map(|list| {
            list.iter()
                .map(|w| Waypoint {
                    name: w.name.unwrap_or_default().to_owned(),
                    position: w.init_matrix.position,
                    rotation: w.init_matrix.rotation,
                    height: w.height,
                })
                .collect()
        })
        .collect();
    let actors = p
        .actors
        .iter()
        .map(|a| {
            let at = &a.placement().placement;
            Placement {
                object: a.index as u32,
                id: at.id,
                name: a.name().unwrap_or_default().to_owned(),
                script: a.script.map(str::to_owned),
                position: at.init_matrix.position,
                rotation: at.init_matrix.rotation,
                scale: a.placement().scale,
                model: p.model_path(a).map(str::to_owned),
                solid: a.collision.as_ref().map(models::solid),
                movement: a.parameters.as_ref().map(models::movement),
                waypoints: a.info.waypoints.and_then(|l| lists.get(&l).copied()),
                activity_state: at.activity_state,
                activation_range: a.placement().activation_range,
            }
        })
        .collect();
    Ok(Read {
        ir: Placements {
            identity,
            level: level.to_owned(),
            actors,
            waypoints,
        },
        textures: Vec::new(),
    })
}

/// The lights part: the directional rig and the placed lights.
pub fn lights(igz: &Igz<'_>, level: &str, identity: DumpIdentity) -> Result<Read<Lights>, String> {
    let directional = guac_formats::level::directional_lights(igz)
        .map_err(s)?
        .into_iter()
        .map(|(_, l)| RigLight {
            name: l.name.unwrap_or_default().to_owned(),
            diffuse: l.diffuse,
            ambient: l.ambient,
            specular: l.specular,
            multiplier: l.multiplier,
            position: l.init_matrix.position,
            rotation: l.init_matrix.rotation,
        })
        .collect();
    let placed = FormatPlacements::read(igz)
        .map_err(s)?
        .lights
        .into_iter()
        .map(|(_, l)| {
            let at = &l.placement.placement;
            PlacedLight {
                name: at.name.unwrap_or_default().to_owned(),
                light_type: l.light_type,
                visible: l.visible,
                position: at.init_matrix.position,
                rotation: at.init_matrix.rotation,
                falloff_rate: l.falloff_rate,
                cutoff_angle: l.cutoff_angle,
                attenuation: [
                    l.constant_attenuation,
                    l.linear_attenuation,
                    l.quadratic_attenuation,
                ],
                diffuse: l.diffuse,
                ambient: l.ambient,
                specular: l.specular,
                multiplier: l.multiplier,
            }
        })
        .collect();
    Ok(Read {
        ir: Lights {
            identity,
            level: level.to_owned(),
            directional,
            placed,
        },
        textures: Vec::new(),
    })
}

/// The atmosphere part: the zones the level's definition adds at its start.
pub fn atmosphere(
    igz: &Igz<'_>,
    level: &str,
    identity: DumpIdentity,
) -> Result<Read<Atmosphere>, String> {
    let a = guac_formats::atmosphere::Atmosphere::read(igz).map_err(s)?;
    let zones = a
        .zones
        .iter()
        .map(|z| AtmosphericZone {
            id: z.id,
            ambient: z.ambient,
            lights: z.lights.map(|l| ZoneLight {
                diffuse: l.diffuse,
                specular: l.specular,
                facing: l.facing,
                tilt: l.tilt,
            }),
            fog: ZoneFog {
                on: z.fog.on,
                start: z.fog.start,
                range: z.fog.range,
                colour: z.fog.colour,
                sky_dome_percent: z.fog.sky_dome_percent,
                sky_dome_colour: z.fog.sky_dome_colour,
                yon_colour: z.fog.yon_colour,
            },
        })
        .collect();
    Ok(Read {
        ir: Atmosphere {
            identity,
            level: level.to_owned(),
            script: a.script.to_owned(),
            zones,
            start: a.start.map(|i| i as u32),
        },
        textures: Vec::new(),
    })
}

/// The script part: the scripts the level's entry holds, once it parses
/// and its op graph reads; the stage writes the entry itself.
pub fn script(igz: &Igz<'_>) -> Result<usize, String> {
    guac_formats::script::Graph::read(igz)
        .map(|g| g.scripts.len())
        .map_err(s)
}

/// The palettes part: every slot of each skinned model a placement running
/// one of the level's whitelisted scripts names, each model once, in
/// object order. A model with no skeleton (a breakable, a coin) has none;
/// one whose skeleton won't read refuses the part.
pub fn palettes(
    igz: &Igz<'_>,
    level: &str,
    identity: DumpIdentity,
) -> Result<Read<LevelPalettes>, String> {
    // The level's whitelisted scripts and the walkers' own (AG.162); a level
    // off the whitelist runs none of its scripts, theirs included.
    let mut listed = crate::scripts::level_scripts(level).to_vec();
    if !listed.is_empty() {
        listed.extend(crate::scripts::walker_scripts());
    }
    let placements = FormatPlacements::read(igz).map_err(s)?;
    let mut creatures: BTreeMap<usize, &str> = BTreeMap::new();
    for a in &placements.actors {
        let Some(model) = a.info.model else {
            continue;
        };
        if !a.script.is_some_and(|p| crate::scripts::listed(&listed, p)) {
            continue;
        }
        let Some(info) = placements.models.get(&model) else {
            continue;
        };
        creatures.insert(model, info.info.name.unwrap_or_default());
    }
    let mut palettes = Vec::new();
    for (model, path) in creatures {
        let Ok(hero) = Hero::read(igz, model) else {
            continue;
        };
        let skeleton = fmodel::skeleton(igz, &hero).map_err(|e| format!("{path}: {e}"))?;
        palettes.push(models::palette(
            igz,
            &hero,
            &skeleton,
            path,
            identity.clone(),
        ));
    }
    Ok(Read {
        ir: LevelPalettes {
            identity,
            level: level.to_owned(),
            palettes,
        },
        textures: Vec::new(),
    })
}

#[cfg(test)]
mod tests;
