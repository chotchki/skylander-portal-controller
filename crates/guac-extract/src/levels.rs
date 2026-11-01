//! The levels guacamole plays off the dump (openspec AG.15, design decision
//! 8): chapter 1, `level/level_213_tutotial.bld`, into IR v3's level
//! records ([`guac_ir::level`]) in nine parts, each a file of its own under
//! `levels/<archive>/`, the textures into the characters' store. Writing
//! them is the level stage's ([`crate::stages`]).
//!
//! - The scene: every scene's draws on AE's segment and material records,
//!   tfbmulti2's read as a level's ([`Drawn::Level`]: the gloss map taken,
//!   bits 6 and 9 refused by name), the grass's, `0xAA8`'s and tfbwarp's as
//!   [`EffectMaterial`]s, and the sky's groups. A draw under a transform is
//!   baked into the level's frame ([`bake`], openspec AS.3): 10 of the
//!   disc's levels hold 54, level 213's scenes none. One a keyframe source
//!   moves is baked at the matrix its file stores, which the level's
//!   animation drives at run time ([`animated_draws`] lists them).
//! - The props: each model a placement names, a skinned one as a
//!   character's is read (its skeleton, slot 0 as its idle when that poses
//!   it), a static one through its body's graph, a draw under a still
//!   transform baked as the scene's are. One that doesn't read is listed in
//!   the part, not fatal to it: a draw an animated transform moves is (the
//!   wings of level 213's fairy gem), until the animation plays.
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
//! - The nodes (openspec AH.138): each placed model's attach list, a skinned
//!   one's nodes on its joints, so what a script hangs on a creature's node
//!   rides it (Norticus' turban on his node 2). An extract written before it
//!   gains this part alone on its next start.
//! - The sprites (openspec AT.8): the level's sprite templates, fonts and
//!   English text as the interface part reads an archive's
//!   ([`crate::interface`]), their images into the store. An extract
//!   written before it gains this part alone on its next start.
//!
//! A part that doesn't read is the level stage's failure for that part
//! alone: the other parts land, and the next start tries it again.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use guac_formats::FormatError;
use guac_formats::attachments as fattach;
use guac_formats::collision;
use guac_formats::edge::{BoneMap, Segment as EdgeSegment};
use guac_formats::iga::Archive;
use guac_formats::igz::Igz;
use guac_formats::igz::layouts::Mat44;
use guac_formats::igz::layouts::{
    Actor2, AttrList, Hero, Layout, ObjectRef, Passes, PhysicsBody, PhysicsModel, PhysicsWorld,
    RuntimeTechniqueInstance, is_a, read_all,
};
use guac_formats::level::{
    Draw, Graph, GroupKind, IDENTITY, Placements as FormatPlacements, StaticModel, transform_point,
};
use guac_formats::material::{self, Drawn, OwnEffects, TFBMULTI2};
use guac_formats::model::{self as fmodel, IDLE_SLOT, JointMap};
use guac_ir::level::{
    Atmosphere, AtmosphericZone, Collision, CollisionMesh, EffectMaterial, LevelMaterial, Lights,
    PlacedLight, Placement, Placements, Prop, Props, RefusedProp, Scene, SceneGraph, SkyGroup,
    UnitTexture, Waypoint, ZoneFog, ZoneLight,
};
use guac_ir::model::{Mask, Segment};
use guac_ir::nodes::{AttachNode, AttachNodes, LevelNodes};
use guac_ir::palette::LevelPalettes;
use guac_ir::{DumpIdentity, RigLight};

use crate::extract::Manifest;
use crate::models::{self, Textures};
use crate::roster::LEVEL;

/// Where levels go in the extract: `levels/<archive>/<part>.<ron|postcard>`.
pub const LEVEL_DIR: &str = "levels";
/// The folder holding the level archives, under `PS3_GAME/USRDIR`.
pub const LEVEL_FOLDER: &str = "level";
/// The levels a first run extracts, by archive name (openspec AS.1): chapter
/// 1, "Time of the Giants" (ag-motion section 4; the archive's own
/// spelling). Every other level on the disc ([`on_disc`]) is extracted when
/// asked for ([`crate::stages::extract_level`]), and only these decide
/// whether an extract is current.
pub const FIRST_RUN: [&str; 1] = ["level_213_tutotial"];
/// [`FIRST_RUN`] under the name guacamole reads it by (chapter 1's archive),
/// until the TV loads a level by name (openspec AS.13).
pub const LEVELS: [&str; 1] = FIRST_RUN;

/// What a level file on the disc is, by its archive's name (openspec AS.1):
/// the 143 on the Giants disc are 20 story levels and the 4 hub stages, 18
/// PvP arenas, 45 challenge levels and 21 arena challenges, the 9 mesh
/// bundles those draw from and a voice pack, 19 ship parts, the title, the
/// credits and 4 minion and misc packs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Kind {
    Story,
    Hub,
    Pvp,
    Challenge,
    ArenaChallenge,
    MeshBundle,
    VoicePack,
    ShipPart,
    Title,
    Credits,
    Pack,
    /// A name none of the others fits.
    Other,
}

impl Kind {
    /// By the archive's name (`pvp_level_001_cubedungeon`).
    pub fn of(name: &str) -> Self {
        let starts = |p: &str| name.starts_with(p);
        if starts("level_hub_") {
            Self::Hub
        } else if starts("level_") {
            Self::Story
        } else if starts("pvp_") {
            Self::Pvp
        } else if starts("challenge_level_") {
            Self::Challenge
        } else if starts("arena_challenge_") {
            Self::ArenaChallenge
        } else if starts("challengemesh_") || starts("arena_mesh_") {
            Self::MeshBundle
        } else if starts("challengevo") {
            Self::VoicePack
        } else if starts("ship_part_") {
            Self::ShipPart
        } else if name == "title" {
            Self::Title
        } else if starts("credits") {
            Self::Credits
        } else if starts("_minions") || starts("_misc") {
            Self::Pack
        } else {
            Self::Other
        }
    }
}

/// A level file on the disc: its archive's name, kind and size.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiscLevel {
    pub name: String,
    pub kind: Kind,
    pub bytes: u64,
}

/// Every level file the dump holds (openspec AS.1, design decision 2), by
/// name: each `level/<name>.bld` the dump scan recorded in `m`'s sources,
/// the folder the game's own loader opens a level from by name.
pub fn on_disc(m: &Manifest) -> Vec<DiscLevel> {
    let prefix = format!("{LEVEL_FOLDER}/");
    let mut out: Vec<DiscLevel> = m
        .sources
        .iter()
        .filter_map(|s| {
            let name = s.path.strip_prefix(&prefix)?.strip_suffix(".bld")?;
            (!name.is_empty() && !name.contains('/')).then(|| DiscLevel {
                name: name.to_owned(),
                kind: Kind::of(name),
                bytes: s.size,
            })
        })
        .collect();
    out.sort_by(|a, b| a.name.cmp(&b.name));
    out
}

/// The parts, in the order the stage reads them.
pub const SCENE: &str = "scene";
pub const PROPS: &str = "props";
pub const COLLISION: &str = "collision";
pub const PLACEMENTS: &str = "placements";
pub const LIGHTS: &str = "lights";
pub const ATMOSPHERE: &str = "atmosphere";
pub const SCRIPT: &str = "script";
pub const PALETTES: &str = "palettes";
pub const NODES: &str = "nodes";
/// The level's own interface (openspec AT.8, [`crate::interface`]): its
/// templates, fonts and English text, read with its other parts.
pub const SPRITES: &str = "sprites";
pub const PARTS: [&str; 10] = [
    SCENE, PROPS, COLLISION, PLACEMENTS, LIGHTS, ATMOSPHERE, SCRIPT, PALETTES, NODES, SPRITES,
];
/// The parts the TV reads when it loads a level: every part but the
/// sprites, which nothing draws until the interface does (openspec AT.9), so
/// a first run hands chapter 1 over once these are in and reads the sprites
/// after (AT.26).
pub const PLAYED: [&str; 9] = [
    SCENE, PROPS, COLLISION, PLACEMENTS, LIGHTS, ATMOSPHERE, SCRIPT, PALETTES, NODES,
];

/// The script part's file, `levels/<archive>/script.igz`: raw, so the same
/// in either IR format.
pub fn script_path(level: &str) -> String {
    format!("{LEVEL_DIR}/{level}/{SCRIPT}.igz")
}

/// `part`'s file in `ir`'s format: its IR, the script's raw entry, or the
/// sprites' IR named by its record's shape ([`crate::interface::level_path`]).
pub fn part_path(ir: crate::intros::IrFormat, level: &str, part: &str) -> String {
    match part {
        SCRIPT => script_path(level),
        SPRITES => crate::interface::level_path(ir, level),
        _ => ir.level_path(level, part),
    }
}

/// `level/<archive>.bld` under the dump's `PS3_GAME/USRDIR`.
pub fn archive_path(usrdir: &Path, level: &str) -> PathBuf {
    usrdir.join(LEVEL_FOLDER).join(format!("{level}.bld"))
}

/// Whether `m` holds every part of every level a first run extracts
/// ([`FIRST_RUN`]); a level extracted on demand, or not, changes nothing.
pub fn complete(m: &Manifest) -> bool {
    FIRST_RUN.iter().all(|level| {
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
/// record, read as a level's, or another effect's; in a file naming its own
/// effects, `own` says what each goes by (`global.bld`'s, openspec AH.39).
fn level_material(
    igz: &Igz<'_>,
    passes: Passes<'_, '_>,
    t: usize,
    technique: &RuntimeTechniqueInstance<'_>,
    textures: &mut Textures,
    own: Option<&OwnEffects>,
) -> Result<LevelMaterial, String> {
    let list = pass(igz, technique);
    if material::effect_in(igz, t, technique, own).map_err(s)? == TFBMULTI2 {
        let m = material::Material::read_technique_in(
            igz,
            passes,
            t,
            technique,
            list,
            Drawn::Level,
            own,
        )
        .map_err(s)?;
        return models::multi2(igz, &m, textures).map(LevelMaterial::Multi2);
    }
    let m = material::EffectMaterial::read_in(igz, passes, t, technique, list, own).map_err(s)?;
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

/// What a graph's draws under a transform get (openspec AS.3): a scene's
/// are baked whatever moves them, a prop's only when no keyframe source
/// does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Moved {
    Bake,
    BakeStill,
}

/// Whether a transform at or above `draw` has a keyframe source.
fn animated(graph: &Graph<'_>, draw: &Draw<'_>) -> bool {
    let mut at = draw.parent;
    while let Some(g) = at {
        let group = &graph.groups[&g];
        if matches!(&group.kind, GroupKind::Transform(t) if t.is_animated()) {
            return true;
        }
        at = group.parent;
    }
    false
}

fn det3(a: &[[f64; 3]; 3]) -> f64 {
    a[0][0] * (a[1][1] * a[2][2] - a[1][2] * a[2][1])
        - a[0][1] * (a[1][0] * a[2][2] - a[1][2] * a[2][0])
        + a[0][2] * (a[1][0] * a[2][1] - a[1][1] * a[2][0])
}

/// `v` at `of`'s length (the file's normals and tangents aren't all unit
/// length, and a bake keeps each as long as it was); a zero vector stays
/// zero.
fn as_long(v: [f64; 3], of: [f32; 3]) -> [f32; 3] {
    let len = |v: [f64; 3]| (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
    let (new, old) = (len(v), len(of.map(f64::from)));
    if new > 0.0 {
        v.map(|x| (x * (old / new)) as f32)
    } else {
        [0.0; 3]
    }
}

/// `segment` moved by `m`, a row-vector affine (a point p lands at p · m),
/// into the frame its draw sits in (openspec AS.3): positions through `m`,
/// normals through the inverse transpose of its 3 × 3 part and tangents
/// through that part, each kept at its own length, and under a mirror (a negative
/// determinant) each tangent's handedness negated and each triangle's
/// winding swapped, so its front stays its front. Refused: a matrix that
/// flattens (its 3 × 3 part has no inverse) or isn't finite.
pub fn bake(segment: &mut Segment, m: &Mat44) -> Result<(), String> {
    let a: [[f64; 3]; 3] = std::array::from_fn(|r| std::array::from_fn(|c| f64::from(m[r][c])));
    let det = det3(&a);
    if !det.is_finite() || det.abs() < 1e-12 || m.as_flattened().iter().any(|x| !x.is_finite()) {
        return Err(format!("a transform that flattens or isn't finite: {m:?}"));
    }
    // The inverse by cofactors: inv[c][r] = cofactor(r, c) / det.
    let cof = |r: usize, c: usize| {
        let (r1, r2) = ((r + 1) % 3, (r + 2) % 3);
        let (c1, c2) = ((c + 1) % 3, (c + 2) % 3);
        a[r1][c1] * a[r2][c2] - a[r1][c2] * a[r2][c1]
    };
    let inv: [[f64; 3]; 3] = std::array::from_fn(|i| std::array::from_fn(|j| cof(j, i) / det));
    for p in &mut segment.positions {
        *p = transform_point(*p, m);
    }
    // n · (M⁻¹)ᵀ: component c is Σ_k n_k inv[c][k].
    for n in &mut segment.normals {
        let v = std::array::from_fn(|c| (0..3).map(|k| f64::from(n[k]) * inv[c][k]).sum());
        *n = as_long(v, *n);
    }
    let mirror = det < 0.0;
    if let Some(tangents) = &mut segment.tangents {
        for t in tangents {
            let v = std::array::from_fn(|c| (0..3).map(|k| f64::from(t[k]) * a[k][c]).sum());
            let [x, y, z] = as_long(v, [t[0], t[1], t[2]]);
            *t = [x, y, z, if mirror { -t[3] } else { t[3] }];
        }
    }
    if mirror {
        for tri in segment.indices.as_chunks_mut::<3>().0 {
            tri.swap(1, 2);
        }
    }
    Ok(())
}

/// Each scene draw a keyframe source moves (openspec AS.3), by its
/// technique's name, the scene's object and whether its matrix is the
/// identity: baked at the matrix its file stores until the animation plays.
pub fn animated_draws(igz: &Igz<'_>) -> Result<Vec<(usize, String, bool)>, String> {
    let mut out = Vec::new();
    for (index, sc) in guac_formats::level::scenes(igz) {
        let sc = sc.map_err(s)?;
        for d in sc.draws.iter().filter(|d| animated(&sc, d)) {
            let name = d.technique.node.name.unwrap_or_default().to_owned();
            out.push((index, name, d.world == IDENTITY));
        }
    }
    Ok(out)
}

/// A static graph's draws as materials and segments, where the graph has
/// them, each under a transform baked by `moved`'s rule. `own`:
/// [`level_material`]'s.
#[allow(clippy::too_many_arguments)]
fn static_draws(
    igz: &Igz<'_>,
    graph: &Graph<'_>,
    moved: Moved,
    textures: &mut Textures,
    materials: &mut Vec<LevelMaterial>,
    segments: &mut Vec<Segment>,
    own: Option<&OwnEffects>,
    mut each: impl FnMut(&Draw<'_>, u32),
) -> Result<(), String> {
    let lists = graph.material_lists(igz).map_err(s)?;
    for draw in &graph.draws {
        let name = draw.technique.node.name.unwrap_or_default();
        let baked = draw.world != IDENTITY;
        if baked && moved == Moved::BakeStill && animated(graph, draw) {
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
            own,
        )
        .map_err(|e| format!("{name} (technique #{}): {e}", draw.technique_index))?;
        let normals = matches!(m, LevelMaterial::Multi2(_));
        let index = u16::try_from(materials.len()).map_err(|_| "65,536 materials")?;
        materials.push(m);
        for (i, layout) in graph.draw_segments(draw) {
            let decoded = EdgeSegment::decode(igz, i, layout).map_err(s)?;
            let mut seg = models::static_segment(&decoded, index, i as u32, normals)
                .map_err(|e| format!("segment #{i}: {e}"))?;
            if baked {
                bake(&mut seg, &draw.world)
                    .map_err(|e| format!("{name} (technique #{}): {e}", draw.technique_index))?;
            }
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
            Moved::Bake,
            &mut textures,
            &mut materials,
            &mut segments,
            None,
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
    own: Option<&OwnEffects>,
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
            own,
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
    own: Option<&OwnEffects>,
) -> Result<Prop, String> {
    let sm = StaticModel::read(igz, model).map_err(s)?;
    let (mut materials, mut segments) = (Vec::new(), Vec::new());
    static_draws(
        igz,
        &sm.graph,
        Moved::BakeStill,
        textures,
        &mut materials,
        &mut segments,
        own,
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
/// attachments read this way too (AE.23). `own`: the effects a file of
/// `global.bld`'s own names by object (openspec AH.39), `None` for any other
/// file, whose techniques import theirs.
pub fn prop(
    igz: &Igz<'_>,
    model: usize,
    path: &str,
    textures: &mut Textures,
    own: Option<&OwnEffects>,
) -> Result<Prop, String> {
    if skinned(igz, model) {
        skinned_prop(igz, model, path, textures, own)
    } else {
        static_prop(igz, model, path, textures, own)
    }
}

/// Whether the model at `model` is skinned: its body's node an `igActor2`.
fn skinned(igz: &Igz<'_>, model: usize) -> bool {
    PhysicsModel::read(igz, model)
        .ok()
        .and_then(|m| m.model.tfb_body)
        .and_then(|b| PhysicsBody::read(igz, b).ok())
        .and_then(|b| b.body.node)
        .is_some_and(|n| is_a(igz.type_name(&igz.objects()[n]), Actor2::TYPE))
}

/// The nodes part (openspec AH.138): each model a placement names whose
/// attach list holds any, by its path, a skinned one's nodes on its Havok
/// joints by name (the props part's joints, in its order), a static one's on
/// none; one whose list or skeleton won't read listed with why.
pub fn nodes(
    igz: &Igz<'_>,
    level: &str,
    identity: DumpIdentity,
) -> Result<Read<LevelNodes>, String> {
    let placements = FormatPlacements::read(igz).map_err(s)?;
    let (mut models, mut refused) = (Vec::new(), Vec::new());
    for (&model, info) in &placements.models {
        let path = info
            .info
            .name
            .ok_or_else(|| format!("model #{model} has no name"))?;
        let read = (|| -> Result<Vec<AttachNode>, String> {
            let points = fattach::attach_points(igz, model).map_err(s)?;
            if points.is_empty() {
                return Ok(Vec::new());
            }
            let joints: Vec<&str> = if skinned(igz, model) {
                let hero = Hero::read(igz, model).map_err(s)?;
                fmodel::skeleton(igz, &hero)
                    .map_err(s)?
                    .bones
                    .iter()
                    .map(|b| b.name.unwrap_or_default())
                    .collect()
            } else {
                Vec::new()
            };
            Ok(points
                .iter()
                .map(|p| models::attach_node(p, &joints))
                .collect())
        })();
        match read {
            Ok(nodes) if nodes.is_empty() => {}
            Ok(nodes) => models.push(AttachNodes {
                identity: identity.clone(),
                load_file: path.to_owned(),
                nodes,
            }),
            Err(reason) => refused.push(RefusedProp {
                path: path.to_owned(),
                reason,
            }),
        }
    }
    Ok(Read {
        ir: LevelNodes {
            identity,
            level: level.to_owned(),
            models,
            refused,
        },
        textures: Vec::new(),
    })
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
        match prop(igz, model, path, &mut own, None) {
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
/// one of the level's whitelisted scripts or a walker's (AG.162) names, each
/// model once, in object order. A model with no skeleton (a breakable, a
/// coin) has none; one whose skeleton won't read refuses the part.
pub fn palettes(
    igz: &Igz<'_>,
    level: &str,
    identity: DumpIdentity,
) -> Result<Read<LevelPalettes>, String> {
    // The level's whitelisted scripts and the walkers' own (AG.162); a level
    // off the whitelist runs none of its scripts, theirs included.
    let mut listed = crate::scripts::level_scripts(level).to_vec();
    if !listed.is_empty() {
        listed.extend(crate::scripts::whitelisted_walker_scripts());
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
